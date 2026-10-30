//! Intent is committed before any system effect; settlement records facts only.
use super::{autostart_codec as dto, *};
use fc_ports::autostart::*;
const DOCUMENT: &str = "business/autostart/v1";
fn denied() -> StorageError {
    StorageError::Identity(IdentityError::Permission(PermissionError::RoleDenied))
}
fn authorize(
    identity: &IdentityState,
    actor: &ActorContext,
    clock: &dyn Clock,
    session: &SessionGuard,
) -> Result<(), StorageError> {
    session(identity, actor)?;
    identity
        .authorize_management(actor, clock.now())
        .map_err(StorageError::Identity)
}
fn execution_check(guard: &AutostartExecutionGuard) -> Result<(), StorageError> {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| guard())).unwrap_or(false) {
        Ok(())
    } else {
        Err(StorageError::AutostartCancelled)
    }
}
fn load(d: &dyn TransactionDocuments) -> Result<AutostartState, StorageError> {
    d.get(DOCUMENT)?.map_or(Ok(AutostartState::default()), |r| {
        dto::read_state(&codec::value(&r.value)?)
    })
}
pub(super) fn startup_check(d: &dyn TransactionDocuments) -> Result<(), StorageError> {
    load(d)?;
    if d.get(DOCUMENT)?.is_some() && load_identity(d)?.installation.is_none() {
        return Err(StorageError::Database);
    }
    Ok(())
}
fn error(e: StorageError) -> AutostartRepositoryError {
    match e {
        StorageError::AutostartCancelled => AutostartRepositoryError::Cancelled,
        StorageError::Identity(_) => AutostartRepositoryError::Denied,
        StorageError::Conflict { .. } | StorageError::CommandIdReused => {
            AutostartRepositoryError::Stale
        }
        StorageError::ResourceQuota | StorageError::DocumentTooLarge => {
            AutostartRepositoryError::Capacity
        }
        StorageError::InvalidCommand => AutostartRepositoryError::Invalid,
        StorageError::Busy => AutostartRepositoryError::Busy,
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => AutostartRepositoryError::Faulted,
        _ => AutostartRepositoryError::Unavailable,
    }
}
fn encode(s: &AutostartState) -> Result<Vec<u8>, StorageError> {
    if !s.validate() {
        return Err(StorageError::InvalidCommand);
    }
    let b = codec::bytes(&dto::state(s))?;
    if b.len() > MAX_VALUE_BYTES {
        return Err(StorageError::DocumentTooLarge);
    }
    Ok(b)
}
/// Reserve enough encoded space for a factual readback before approving an OS effect.
/// One UTF-16 unit can become a six-byte JSON control escape. Both the current
/// observation and the operation's final observation must fit without truncation.
fn settlement_budget(s: &AutostartState) -> Result<(), StorageError> {
    let mut worst = s.clone();
    let observed = AutostartObservation::Command("\u{1}".repeat(MAX_AUTOSTART_COMMAND_UTF16));
    worst.observed = observed.clone();
    if let Some(op) = worst.history.last_mut() {
        op.phase = AutostartPhase::NeedsReconcile;
        op.observed = Some(observed);
        op.failure = Some(AutostartFailure::ReadbackMismatch);
        op.mutated = false;
    }
    encode(&worst).map(|_| ())
}
fn decision<S>(
    d: &dyn TransactionDocuments,
    s: AutostartState,
    identity: &IdentityState,
    factory: &Factory<S>,
    precommit: Option<Box<dyn FnOnce() -> Result<(), StorageError> + Send>>,
) -> Result<TransactionDecision<S>, StorageError> {
    let bytes = encode(&s)?;
    let snapshot = factory(
        identity,
        d.revision()?
            .next()
            .map_err(|_| StorageError::ResourceQuota)?,
    )?;
    Ok(TransactionDecision {
        outcome: bytes.clone(),
        commit: Some((
            vec![Mutation::Put {
                key: DOCUMENT.into(),
                value: bytes,
            }],
            snapshot,
        )),
        precommit,
    })
}
impl<S: Send + 'static> AutostartRepository for BusinessRepository<S> {
    fn autostart_reconciliation_status(&self) -> Result<AutostartState, AutostartRepositoryError> {
        let b = self
            .handle
            .try_transaction(Read {
                actor: None,
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_state(&codec::value(&b).map_err(error)?).map_err(error)
    }
    fn autostart_status(
        &self,
        actor: &ActorContext,
    ) -> Result<AutostartState, AutostartRepositoryError> {
        let b = self
            .handle
            .try_transaction(Read {
                actor: Some(actor.clone()),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_state(&codec::value(&b).map_err(error)?).map_err(error)
    }
    fn begin_autostart(
        &mut self,
        request: AutostartRequest,
        actor: &ActorContext,
    ) -> Result<AutostartOperation, AutostartRepositoryError> {
        let b = self
            .handle
            .try_transaction(Change {
                kind: ChangeKind::Begin(request),
                actor: Some(actor.clone()),
                clock: self.clock.clone(),
                session: self.session.clone(),
                execution: self.autostart_execution.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_state(&codec::value(&b).map_err(error)?)
            .map_err(error)?
            .current()
            .cloned()
            .ok_or(AutostartRepositoryError::Unavailable)
    }
    fn mark_autostart_applying(
        &mut self,
        ticket: AutostartTicket,
        actor: &ActorContext,
    ) -> Result<AutostartOperation, AutostartRepositoryError> {
        let b = self
            .handle
            .try_transaction(Change {
                kind: ChangeKind::Applying(ticket),
                actor: Some(actor.clone()),
                clock: self.clock.clone(),
                session: self.session.clone(),
                execution: self.autostart_execution.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_state(&codec::value(&b).map_err(error)?)
            .map_err(error)?
            .current()
            .cloned()
            .ok_or(AutostartRepositoryError::Unavailable)
    }
    fn finish_autostart(
        &mut self,
        ticket: AutostartTicket,
        outcome: AutostartSystemOutcome,
    ) -> Result<AutostartState, AutostartRepositoryError> {
        let b = self
            .handle
            .try_transaction(Change {
                kind: ChangeKind::Finish(ticket, outcome),
                actor: None,
                clock: self.clock.clone(),
                session: self.session.clone(),
                execution: self.autostart_execution.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_state(&codec::value(&b).map_err(error)?).map_err(error)
    }
    fn reconcile_autostart(
        &mut self,
        expected_revision: Revision,
        observed: AutostartObservation,
    ) -> Result<AutostartState, AutostartRepositoryError> {
        let b = self
            .handle
            .try_transaction(Change {
                kind: ChangeKind::Reconcile(expected_revision, observed),
                actor: None,
                clock: self.clock.clone(),
                session: self.session.clone(),
                execution: self.autostart_execution.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_state(&codec::value(&b).map_err(error)?).map_err(error)
    }
}
struct Read {
    actor: Option<ActorContext>,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for Read {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        if let Some(a) = &self.actor {
            authorize(&identity, a, self.clock.as_ref(), &self.session)?;
        }
        let s = load(d)?;
        let outcome = encode(&s)?;
        if let Some(a) = &self.actor {
            authorize(&identity, a, self.clock.as_ref(), &self.session)?;
        }
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}
enum ChangeKind {
    Begin(AutostartRequest),
    Applying(AutostartTicket),
    Finish(AutostartTicket, AutostartSystemOutcome),
    Reconcile(Revision, AutostartObservation),
}
struct Change<S> {
    kind: ChangeKind,
    actor: Option<ActorContext>,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    execution: AutostartExecutionGuard,
    factory: Factory<S>,
}
impl<S: Send + 'static> TransactionTask<S> for Change<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match &self.kind {
                ChangeKind::Begin(r) => {
                    r.target_command.capacity() + r.previous_observed.allocation_bytes()
                }
                ChangeKind::Finish(_, o) => o.observation.allocation_bytes(),
                ChangeKind::Reconcile(_, o) => o.allocation_bytes(),
                _ => 0,
            }
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        if identity.installation.is_none() {
            return Err(denied());
        }
        let mut s = load(d)?;
        if let Some(a) = &self.actor {
            execution_check(&self.execution)?;
            authorize(&identity, a, self.clock.as_ref(), &self.session)?;
        }
        match &self.kind {
            ChangeKind::Begin(r) => {
                if !valid_autostart_target(&r.target_command)
                    || !r.previous_observed.mutation_eligible()
                {
                    return Err(StorageError::InvalidCommand);
                }
                let a = self.actor.as_ref().ok_or_else(denied)?;
                if let Some(old) = s
                    .history
                    .iter()
                    .find(|o| o.ticket.command_id == r.command_id)
                {
                    if s.current() != Some(old)
                        || old.ticket.prepared_revision.get()
                            != r.expected_revision
                                .get()
                                .checked_add(1)
                                .ok_or(StorageError::InvalidCommand)?
                        || old.desired != r.desired
                        || old.target_command != r.target_command
                        || old.previous_observed != r.previous_observed
                        || old.ticket.principal_id != a.principal_id
                        || old.ticket.auth_epoch != a.auth_epoch
                    {
                        return Err(StorageError::CommandIdReused);
                    }
                    return Ok(TransactionDecision {
                        outcome: encode(&s)?,
                        commit: None,
                        precommit: None,
                    });
                }
                if s.revision != r.expected_revision {
                    return Err(StorageError::Conflict {
                        current: s.revision,
                    });
                }
                if s.current().is_some_and(|o| o.phase.pending()) {
                    return Err(StorageError::Conflict {
                        current: s.revision,
                    });
                }
                s.sequence = s
                    .sequence
                    .checked_add(1)
                    .ok_or(StorageError::ResourceQuota)?;
                s.revision = s.revision.next().map_err(|_| StorageError::ResourceQuota)?;
                s.desired = r.desired;
                s.target_command = Some(r.target_command.clone());
                s.observed = r.previous_observed.clone();
                s.history.push(AutostartOperation {
                    ticket: AutostartTicket {
                        command_id: r.command_id,
                        sequence: s.sequence,
                        prepared_revision: s.revision,
                        principal_id: a.principal_id,
                        auth_epoch: a.auth_epoch,
                    },
                    desired: r.desired,
                    target_command: r.target_command.clone(),
                    previous_observed: r.previous_observed.clone(),
                    phase: AutostartPhase::Prepared,
                    observed: None,
                    failure: None,
                    mutated: false,
                });
                if s.history.len() > MAX_AUTOSTART_HISTORY {
                    s.history.remove(0);
                }
                settlement_budget(&s)?;
            }
            ChangeKind::Applying(ticket) => {
                let a = self.actor.as_ref().ok_or_else(denied)?;
                let current = s.history.last_mut().filter(|o| &o.ticket == ticket).ok_or(
                    StorageError::Conflict {
                        current: s.revision,
                    },
                )?;
                if a.principal_id != ticket.principal_id || a.auth_epoch != ticket.auth_epoch {
                    return Err(denied());
                }
                if current.phase != AutostartPhase::Prepared {
                    return Err(StorageError::Conflict {
                        current: s.revision,
                    });
                }
                current.phase = AutostartPhase::Applying;
                s.revision = s.revision.next().map_err(|_| StorageError::ResourceQuota)?;
            }
            ChangeKind::Finish(ticket, outcome) => {
                if !outcome.observation.validate() {
                    return Err(StorageError::InvalidCommand);
                }
                let current = s.history.last_mut().filter(|o| &o.ticket == ticket).ok_or(
                    StorageError::Conflict {
                        current: s.revision,
                    },
                )?;
                if !current.phase.pending() {
                    if current.observed.as_ref() == Some(&outcome.observation)
                        && current.failure == outcome.failure
                        && current.mutated == outcome.mutated
                    {
                        return Ok(TransactionDecision {
                            outcome: encode(&s)?,
                            commit: None,
                            precommit: None,
                        });
                    }
                    return Err(StorageError::Conflict {
                        current: s.revision,
                    });
                }
                if current.phase == AutostartPhase::Prepared && outcome.mutated {
                    return Err(StorageError::InvalidCommand);
                }
                let success = current.phase == AutostartPhase::Applying
                    && outcome.failure.is_none()
                    && outcome
                        .observation
                        .matches(current.desired, &current.target_command);
                current.phase = if success {
                    AutostartPhase::Succeeded
                } else if outcome.mutated || !outcome.observation.mutation_eligible() {
                    AutostartPhase::NeedsReconcile
                } else {
                    AutostartPhase::Failed
                };
                current.observed = Some(outcome.observation.clone());
                current.failure = if success {
                    None
                } else {
                    Some(
                        outcome
                            .failure
                            .unwrap_or(AutostartFailure::ReadbackMismatch),
                    )
                };
                current.mutated = outcome.mutated;
                s.observed = outcome.observation.clone();
                s.revision = s.revision.next().map_err(|_| StorageError::ResourceQuota)?;
            }
            ChangeKind::Reconcile(expected, observed) => {
                if !observed.validate() {
                    return Err(StorageError::InvalidCommand);
                }
                if s.revision != *expected {
                    return Err(StorageError::Conflict {
                        current: s.revision,
                    });
                }
                let pending = s.current().is_some_and(|o| o.phase.pending());
                if s.observed == *observed && !pending {
                    return Ok(TransactionDecision {
                        outcome: encode(&s)?,
                        commit: None,
                        precommit: None,
                    });
                }
                s.observed = observed.clone();
                if let Some(o) = s.history.last_mut().filter(|o| o.phase.pending()) {
                    let success = o.phase == AutostartPhase::Applying
                        && observed.matches(o.desired, &o.target_command);
                    o.phase = if success {
                        AutostartPhase::Succeeded
                    } else {
                        AutostartPhase::NeedsReconcile
                    };
                    o.observed = Some(observed.clone());
                    o.failure = if success {
                        None
                    } else {
                        Some(AutostartFailure::Interrupted)
                    };
                }
                s.revision = s.revision.next().map_err(|_| StorageError::ResourceQuota)?;
            }
        }
        let execution = self.execution.clone();
        let clock = self.clock.clone();
        let session = self.session.clone();
        let guard_identity = identity.clone();
        let precommit = self.actor.map(|a| {
            Box::new(move || {
                execution_check(&execution)?;
                authorize(&guard_identity, &a, clock.as_ref(), &session)
            }) as Box<dyn FnOnce() -> Result<(), StorageError> + Send>
        });
        // Factory prepares a snapshot only, and executes before the actual precommit check.
        let identity = load_identity(d)?;
        decision(d, s, &identity, &self.factory, precommit)
    }
}
#[cfg(test)]
#[path = "tests/autostart.rs"]
mod tests;
