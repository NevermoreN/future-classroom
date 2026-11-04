//! Typed local command receipts. DTOs are not executable fetch/audio permits.
use super::*;
use crate::typed_command::{FirstExecutionPermit, Mode, StoredTyped, TypedAdmission, WindowAction};
use fc_ports::{calendar::*, command_window::*};
use rusqlite::Connection;

pub(crate) fn intent_bytes(intent: &TypedCommandIntent) -> Result<(u8, Vec<u8>), StorageError> {
    let (kind, value) = match intent {
        TypedCommandIntent::VoicePolicy(VoicePolicyIntent::Enabled {
            enabled,
            expected_voice,
            expected_identity,
            expected_runtime,
        }) => (
            1,
            json!([
                enabled,
                expected_voice.get(),
                expected_identity.get(),
                expected_runtime
            ]),
        ),
        TypedCommandIntent::VoicePolicy(VoicePolicyIntent::Limits {
            limits,
            expected_voice,
            expected_identity,
            expected_runtime,
        }) => {
            limits.validate().map_err(StorageError::Voice)?;
            (
                2,
                json!([
                    limits.max_duration_millis,
                    limits.max_pending_jobs,
                    limits.max_pending_per_owner,
                    limits.max_repeat_total,
                    expected_voice.get(),
                    expected_identity.get(),
                    expected_runtime
                ]),
            )
        }
        TypedCommandIntent::CalendarPolicy {
            enabled,
            expected_policy,
        } => (3, json!([enabled, expected_policy.get()])),
        TypedCommandIntent::CalendarManualFetch { year } => {
            if !(1..=9999).contains(year) {
                return Err(StorageError::InvalidCommand);
            }
            (4, json!([year]))
        }
        TypedCommandIntent::RestoreBarrier { expected_global } => {
            (5, json!([expected_global.get()]))
        }
    };
    Ok((kind, codec::bytes(&value)?))
}
fn manual_value(result: &CalendarManualResult) -> Value {
    let (kind, extra) = match &result.kind {
        CalendarManualResultKind::Downloaded => ("downloaded", Value::Null),
        CalendarManualResultKind::NotModified => ("not-modified", Value::Null),
        CalendarManualResultKind::Failed(f) => ("failed", json!(failure(*f))),
        CalendarManualResultKind::RateLimited { next_allowed } => {
            ("limited", codec::time(*next_allowed))
        }
    };
    json!({"year":result.year,"sequence":result.attempt_sequence,"candidate":result.candidate_revision.get(),"committed":result.committed_revision.get(),"kind":kind,"extra":extra})
}
fn read_manual(bytes: &[u8]) -> Result<CalendarManualResult, StorageError> {
    let v = codec::value(bytes)?;
    codec::object_fields(
        &v,
        &[
            "year",
            "sequence",
            "candidate",
            "committed",
            "kind",
            "extra",
        ],
    )?;
    let year = i32::try_from(codec::number(&v, "year")?).map_err(|_| StorageError::Database)?;
    if !(1..=9999).contains(&year) {
        return Err(StorageError::Database);
    }
    let sequence = if v["sequence"].is_null() {
        None
    } else {
        Some(codec::number(&v, "sequence")?)
    };
    let kind = match v["kind"].as_str() {
        Some("downloaded") => CalendarManualResultKind::Downloaded,
        Some("not-modified") => CalendarManualResultKind::NotModified,
        Some("failed") => CalendarManualResultKind::Failed(read_failure(
            v["extra"].as_str().ok_or(StorageError::Database)?,
        )?),
        Some("limited") => CalendarManualResultKind::RateLimited {
            next_allowed: codec::read_time(&v["extra"])?,
        },
        _ => return Err(StorageError::Database),
    };
    Ok(CalendarManualResult {
        year,
        attempt_sequence: sequence,
        candidate_revision: Revision::new(codec::number(&v, "candidate")?),
        committed_revision: Revision::new(codec::number(&v, "committed")?),
        kind,
    })
}
fn failure(f: CalendarFetchFailure) -> &'static str {
    match f {
        CalendarFetchFailure::Cancelled => "cancelled",
        CalendarFetchFailure::Network => "network",
        CalendarFetchFailure::Tls => "tls",
        CalendarFetchFailure::Http => "http",
        CalendarFetchFailure::InvalidData => "invalid-data",
        CalendarFetchFailure::TooLarge => "too-large",
        CalendarFetchFailure::Deadline => "deadline",
    }
}
fn read_failure(s: &str) -> Result<CalendarFetchFailure, StorageError> {
    Ok(match s {
        "cancelled" => CalendarFetchFailure::Cancelled,
        "network" => CalendarFetchFailure::Network,
        "tls" => CalendarFetchFailure::Tls,
        "http" => CalendarFetchFailure::Http,
        "invalid-data" => CalendarFetchFailure::InvalidData,
        "too-large" => CalendarFetchFailure::TooLarge,
        "deadline" => CalendarFetchFailure::Deadline,
        _ => return Err(StorageError::Database),
    })
}
pub(crate) fn outcome(
    intent: &TypedCommandIntent,
    stored: &StoredTyped,
) -> Result<TypedCommandOutcome, StorageError> {
    match stored.phase {
        1 => {
            if !stored.value.is_empty() {
                return Err(StorageError::Database);
            }
            Ok(TypedCommandOutcome::Pending)
        }
        2 => Ok(TypedCommandOutcome::Completed(match intent {
            TypedCommandIntent::VoicePolicy(_) => {
                let mut snapshot =
                    super::voice_codec::read_snapshot(&codec::value(&stored.value)?)?;
                for job in &mut snapshot.jobs {
                    job.authorization = None;
                }
                TypedCommandResult::VoicePolicy(snapshot)
            }
            TypedCommandIntent::CalendarPolicy { .. } => TypedCommandResult::CalendarPolicy(
                super::calendar_codec::read_policy(&codec::value(&stored.value)?)?,
            ),
            TypedCommandIntent::CalendarManualFetch { .. } => {
                TypedCommandResult::CalendarManualFetch(read_manual(&stored.value)?)
            }
            TypedCommandIntent::RestoreBarrier { .. } => return Err(StorageError::CommandExpired),
        })),
        3 => Ok(TypedCommandOutcome::Failed(match stored.value.as_slice() {
            [1] => TypedCommandFailure::Denied,
            [2] => TypedCommandFailure::Conflict,
            [3] => TypedCommandFailure::Stale,
            [4] => TypedCommandFailure::Cancelled,
            _ => return Err(StorageError::Database),
        })),
        _ => Err(StorageError::Database),
    }
}
pub(crate) fn prepare_reply(
    db: &Connection,
    issued: IssuedCommand,
    action: &WindowAction,
    previous: Option<&StoredTyped>,
    raw: &[u8],
) -> Result<(Vec<u8>, bool), StorageError> {
    if let Some(previous) = previous {
        if !matches!(
            action.mode,
            Mode::Complete | Mode::ManualFinish | Mode::Fail(_)
        ) {
            return Ok((reply(false, previous, &[]), false));
        }
        if previous.phase != 1 {
            return Err(StorageError::CommandIdReused);
        }
    }
    let stored = match action.mode {
        Mode::Query => return Err(StorageError::CommandNotIssued),
        Mode::Accept => StoredTyped::pending(),
        Mode::Atomic | Mode::Complete | Mode::ManualFinish => StoredTyped {
            phase: 2,
            value: raw.to_vec(),
        },
        Mode::Fail(failure) => StoredTyped {
            phase: 3,
            value: vec![match failure {
                TypedCommandFailure::Denied => 1,
                TypedCommandFailure::Conflict => 2,
                TypedCommandFailure::Stale => 3,
                TypedCommandFailure::Cancelled => 4,
            }],
        },
        Mode::Barrier => StoredTyped {
            phase: 2,
            value: Vec::new(),
        },
        Mode::ManualReserve => {
            let value = codec::value(raw)?;
            if value["kind"] == "reserved" {
                StoredTyped::pending()
            } else if value["kind"] == "limited" {
                let TypedCommandIntent::CalendarManualFetch { year } = action.intent else {
                    return Err(StorageError::InvalidCommand);
                };
                StoredTyped {
                    phase: 2,
                    value: codec::bytes(&manual_value(&CalendarManualResult {
                        year,
                        attempt_sequence: None,
                        candidate_revision: Revision::default(),
                        committed_revision: crate::read_revision(db)?
                            .next()
                            .map_err(|_| StorageError::InvalidCommand)?,
                        kind: CalendarManualResultKind::RateLimited {
                            next_allowed: codec::read_time(&value["next_allowed"])?,
                        },
                    }))?,
                }
            } else {
                return Err(StorageError::Database);
            }
        }
    };
    crate::typed_command::persist(db, issued, action, previous, &stored)?;
    Ok((
        if action.mode == Mode::Barrier {
            raw.to_vec()
        } else {
            reply(previous.is_none(), &stored, raw)
        },
        true,
    ))
}
fn reply(fresh: bool, stored: &StoredTyped, raw: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(6 + stored.value.len() + raw.len());
    result.extend_from_slice(&[u8::from(fresh), stored.phase]);
    result.extend_from_slice(&(stored.value.len() as u32).to_le_bytes());
    result.extend_from_slice(&stored.value);
    result.extend_from_slice(raw);
    result
}
pub(crate) fn read_reply(bytes: &[u8]) -> Result<(bool, StoredTyped, &[u8]), StorageError> {
    if bytes.len() < 6 || bytes.len() > 2 * MAX_VALUE_BYTES + 6 || bytes[0] > 1 {
        return Err(StorageError::Database);
    }
    let n =
        u32::from_le_bytes(bytes[2..6].try_into().map_err(|_| StorageError::Database)?) as usize;
    if n > MAX_VALUE_BYTES || n > bytes.len() - 6 {
        return Err(StorageError::Database);
    }
    Ok((
        bytes[0] == 1,
        StoredTyped {
            phase: bytes[1],
            value: bytes[6..6 + n].to_vec(),
        },
        &bytes[6 + n..],
    ))
}

pub(super) struct TypedTask<S> {
    pub(super) inner: Option<Box<dyn TransactionTask<S>>>,
    pub(super) intent: TypedCommandIntent,
    pub(super) mode: Mode,
    pub(super) original: Option<IssuedCommand>,
    pub(super) actor: Option<ActorContext>,
    pub(super) clock: Arc<dyn Clock + Send + Sync>,
    pub(super) session: SessionGuard,
    pub(super) manual: Option<(CalendarFetchTicket, CalendarManualResultKind)>,
}
impl<S: Send + 'static> TransactionTask<S> for TypedTask<S> {
    fn window_action(&self) -> Option<WindowAction> {
        Some(WindowAction {
            intent: self.intent.clone(),
            mode: self.mode,
            original: self.original,
        })
    }
    fn lifetime(&self) -> TransactionLifetime {
        if matches!(self.mode, Mode::ManualFinish | Mode::Fail(_)) {
            TransactionLifetime::OwnerReturn
        } else {
            TransactionLifetime::Ordinary
        }
    }
    fn command_family(&self) -> Option<CommandFamily> {
        if matches!(self.mode, Mode::ManualFinish | Mode::Fail(_)) {
            None
        } else {
            Some(match self.intent {
                TypedCommandIntent::VoicePolicy(_) => CommandFamily::Voice,
                TypedCommandIntent::CalendarPolicy { .. }
                | TypedCommandIntent::CalendarManualFetch { .. } => CommandFamily::Calendar,
                TypedCommandIntent::RestoreBarrier { .. } => CommandFamily::Generic,
            })
        }
    }
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.inner.as_ref().map_or(0, |t| t.allocation_bytes()))
            .saturating_add(self.manual.as_ref().map_or(0, |(ticket, _)| {
                ticket
                    .etag
                    .as_ref()
                    .map_or(0, String::capacity)
                    .saturating_add(ticket.last_modified.as_ref().map_or(0, String::capacity))
            }))
    }
    fn evaluate(
        mut self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        let voice_policy = matches!(&self.intent, TypedCommandIntent::VoicePolicy(_));
        let check = {
            let identity = identity.clone();
            let actor = self.actor.clone();
            let clock = self.clock.clone();
            let session = self.session.clone();
            move || {
                if let Some(actor) = &actor {
                    session(&identity, actor)?;
                    if actor.origin != TrustedOrigin::Local && !voice_policy {
                        return Err(if voice_policy {
                            StorageError::Voice(fc_domain::voice::VoiceError::Permission(
                                fc_domain::auth::PermissionError::LocalOnly,
                            ))
                        } else {
                            StorageError::InvalidCommand
                        });
                    }
                    identity
                        .authorize_management(actor, clock.now())
                        .map_err(StorageError::Identity)?;
                }
                Ok(())
            }
        };
        check()?;
        if d.typed_history().is_some()
            && !matches!(
                self.mode,
                Mode::Complete | Mode::ManualFinish | Mode::Fail(_)
            )
        {
            return Ok(TransactionDecision {
                outcome: Vec::new(),
                commit: None,
                precommit: Some(Box::new(check)),
            });
        }
        if matches!(self.mode, Mode::Fail(_)) {
            return Ok(TransactionDecision {
                outcome: Vec::new(),
                commit: None,
                precommit: None,
            });
        }
        let mut decision = if let Some(inner) = self.inner.take() {
            inner.evaluate(d)?
        } else {
            return Err(StorageError::CommandNotIssued);
        };
        if self.mode == Mode::Accept {
            decision.commit = None;
        }
        if let Some((ticket, kind)) = &self.manual {
            let status = super::calendar_codec::read_status(&decision.outcome)?;
            let summary = status
                .years
                .iter()
                .find(|s| s.year == ticket.year)
                .ok_or(StorageError::Database)?;
            let kind = summary
                .last_error
                .map_or_else(|| kind.clone(), CalendarManualResultKind::Failed);
            decision.outcome = codec::bytes(&manual_value(&CalendarManualResult {
                year: ticket.year,
                attempt_sequence: Some(ticket.attempt_sequence),
                candidate_revision: summary.candidate_revision,
                committed_revision: d
                    .revision()?
                    .next()
                    .map_err(|_| StorageError::InvalidCommand)?,
                kind,
            }))?;
        }
        let original = decision.precommit.take();
        decision.precommit = Some(Box::new(move || {
            check()?;
            if let Some(original) = original {
                original()?;
            }
            check()
        }));
        Ok(decision)
    }
}
impl<S: Send + 'static> BusinessRepository<S> {
    pub fn query_typed_command(
        &self,
        intent: TypedCommandIntent,
        actor: &ActorContext,
    ) -> Result<TypedCommandOutcome, StorageError> {
        let bytes = self.handle.try_transaction(TypedTask {
            inner: None,
            intent: intent.clone(),
            mode: Mode::Query,
            original: None,
            actor: Some(actor.clone()),
            clock: self.clock.clone(),
            session: self.session.clone(),
            manual: None,
        })?;
        let bytes = match &self.voice_timeout {
            Some((timeout, _)) => bytes.wait_timeout(*timeout),
            None => bytes.wait(),
        }?;
        let (_, stored, _) = read_reply(&bytes)?;
        outcome(&intent, &stored)
    }
    pub fn fail_typed_command(
        &self,
        permit: FirstExecutionPermit,
        failure: TypedCommandFailure,
    ) -> Result<(), StorageError> {
        let mut owner_handle = self.handle.clone();
        owner_handle.command_window = None;
        owner_handle.transaction_execution = None;
        let ticket = owner_handle.try_transaction(TypedTask {
            inner: None,
            intent: permit.intent,
            mode: Mode::Fail(failure),
            original: Some(permit.issued),
            actor: None,
            clock: self.clock.clone(),
            session: self.session.clone(),
            manual: None,
        })?;
        match &self.voice_timeout {
            Some((timeout, _)) => ticket.wait_timeout(*timeout),
            None => ticket.wait(),
        }?;
        Ok(())
    }
    pub(super) fn typed_admission(
        &self,
        bytes: &[u8],
        intent: TypedCommandIntent,
    ) -> Result<TypedAdmission, StorageError> {
        let (fresh, stored, _) = read_reply(bytes)?;
        if fresh && stored.phase == 1 {
            Ok(TypedAdmission::Fresh(FirstExecutionPermit {
                issued: self
                    .handle
                    .command_window
                    .ok_or(StorageError::CommandNotIssued)?,
                intent,
            }))
        } else {
            Ok(TypedAdmission::History(outcome(&intent, &stored)?))
        }
    }
}
