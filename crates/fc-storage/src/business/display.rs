//! Independently versioned display preferences on the existing sole writer.
use super::{display_codec as dto, *};
use fc_domain::desktop::*;
use fc_ports::{DisplayRepository, DisplayRepositoryError, DisplayRequest};

const DOCUMENT: &str = "business/display/v1";
pub(super) const MAX_FACTS_BYTES: usize = 2 * 1024 * 1024;
struct LimitedBuffer {
    value: Vec<u8>,
    maximum: usize,
}
impl std::io::Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.value.len()) {
            return Err(std::io::Error::other("display response allocation limit"));
        }
        self.value.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn encode(value: &Value, maximum: usize) -> Result<Vec<u8>, StorageError> {
    let mut output = LimitedBuffer {
        value: Vec::with_capacity(4096.min(maximum)),
        maximum,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| StorageError::DocumentTooLarge)?;
    Ok(output.value)
}
pub(super) fn load(
    d: &dyn TransactionDocuments,
    identity: &IdentityState,
) -> Result<DisplaySettings, StorageError> {
    let Some(row) = d.get(DOCUMENT)? else {
        return Ok(DisplaySettings::default());
    };
    if identity.installation.is_none() {
        return Err(StorageError::Database);
    }
    let settings = dto::read_settings(&codec::value(&row.value)?)?;
    if settings.revision == Revision::default() {
        return Err(StorageError::Database);
    }
    Ok(settings)
}
pub(super) fn error(error: StorageError) -> DisplayRepositoryError {
    match error {
        StorageError::Display(e) => DisplayRepositoryError::Domain(e),
        StorageError::CommandIdReused => DisplayRepositoryError::IdempotencyConflict,
        StorageError::DocumentTooLarge => DisplayRepositoryError::DocumentTooLarge,
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => DisplayRepositoryError::Faulted,
        _ => DisplayRepositoryError::Unavailable,
    }
}
fn authorize(
    state: &IdentityState,
    actor: &ActorContext,
    clock: &dyn Clock,
    session: &SessionGuard,
) -> Result<(), StorageError> {
    session(state, actor)?;
    state
        .authorize_management(actor, clock.now())
        .map_err(|error| StorageError::Display(DisplayError::Identity(error)))
}
impl<S: Send + 'static> DisplayRepository for BusinessRepository<S> {
    fn display(&self, actor: &ActorContext) -> Result<DisplaySettings, DisplayRepositoryError> {
        let outcome = self
            .handle
            .try_transaction(ReadDisplay {
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_settings(&codec::value(&outcome).map_err(error)?).map_err(error)
    }
    fn execute_display(
        &mut self,
        request: DisplayRequest,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<DisplaySettings, DisplayRepositoryError> {
        let outcome = self
            .handle
            .try_transaction(SaveDisplay {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_settings(&codec::value(&outcome).map_err(error)?).map_err(error)
    }
    fn desktop_facts(
        &self,
        offset: u64,
        _clock: &dyn Clock,
    ) -> Result<DesktopFacts, DisplayRepositoryError> {
        self.desktop_facts_with_material_cursor(offset, None, _clock)
    }
    fn desktop_facts_with_material_cursor(
        &self,
        offset: u64,
        cursor: Option<fc_domain::materials::MaterialCursor>,
        _clock: &dyn Clock,
    ) -> Result<DesktopFacts, DisplayRepositoryError> {
        let result = self
            .handle
            .try_transaction(super::desktop::ReadDesktop {
                offset,
                material_cursor: cursor,
                lesson_observations: self.lesson_observations.clone(),
                clock: self.clock.clone(),
                factory: self.factory.clone(),
            })
            .and_then(|ticket| ticket.wait());
        if matches!(
            &result,
            Err(StorageError::Busy
                | StorageError::Lesson(_)
                | StorageError::Material(fc_domain::materials::MaterialError::InvalidContext))
        ) {
            self.observe_lesson_rollback_barrier()
                .map_err(|_| DisplayRepositoryError::Unavailable)?;
        }
        let outcome = result.map_err(error)?;
        dto::read_facts(&codec::value_with_limit(&outcome, MAX_FACTS_BYTES).map_err(error)?)
            .map_err(error)
    }
}
struct ReadDisplay {
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for ReadDisplay {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
        let settings = load(d, &state)?;
        let outcome = encode(&dto::settings(&settings), MAX_VALUE_BYTES)?;
        authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}
struct SaveDisplay<S> {
    request: DisplayRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for SaveDisplay<S> {
    fn allocation_bytes(&self) -> usize {
        self.request
            .candidate
            .components
            .capacity()
            .saturating_mul(std::mem::size_of::<ComponentSettings>())
            .saturating_add(self.request.candidate.countdown.label.capacity())
            .saturating_add(std::mem::size_of::<Self>())
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
        self.request
            .candidate
            .validate()
            .map_err(StorageError::Display)?;
        let candidate = encode(&dto::settings(&self.request.candidate), MAX_VALUE_BYTES)?;
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"future-classroom/display/v1\0");
        fingerprint.update(self.request.expected_revision.get().to_be_bytes());
        fingerprint.update(codec::bytes(&codec::actor(&self.actor))?);
        fingerprint.update(candidate);
        let fingerprint = fingerprint.finalize().to_vec();
        let receipt_key = format!("business/receipt/display/{}", self.request.command_id.get());
        let result_key = format!("business/result/display/{}", self.request.command_id.get());
        if let Some(receipt) = d.get(&receipt_key)? {
            let value = codec::value(&receipt.value)?;
            codec::object_fields(&value, &["fingerprint", "revision", "result_digest"])?;
            let original: Vec<u8> = serde_json::from_value(value["fingerprint"].clone())
                .map_err(|_| StorageError::Database)?;
            if original.len() != 32 {
                return Err(StorageError::Database);
            }
            if original != fingerprint {
                return Err(StorageError::CommandIdReused);
            }
            let result = d.get(&result_key)?.ok_or(StorageError::Database)?;
            let digest: Vec<u8> = serde_json::from_value(value["result_digest"].clone())
                .map_err(|_| StorageError::Database)?;
            if digest.len() != 32 || digest.as_slice() != Sha256::digest(&result.value).as_slice() {
                return Err(StorageError::Database);
            }
            let settings = dto::read_settings(&codec::value(&result.value)?)?;
            if settings.revision != Revision::new(codec::number(&value, "revision")?)
                || settings.revision
                    != self
                        .request
                        .expected_revision
                        .next()
                        .map_err(|_| StorageError::Database)?
            {
                return Err(StorageError::Database);
            }
            authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
            return Ok(TransactionDecision {
                outcome: result.value,
                commit: None,
                precommit: None,
            });
        }
        let current = load(d, &state)?;
        let next = current
            .transition(
                self.request.candidate.clone(),
                self.request.expected_revision,
                &state,
                &self.actor,
                self.clock.now(),
            )
            .map_err(StorageError::Display)?;
        let outcome = encode(&dto::settings(&next), MAX_VALUE_BYTES)?;
        let snapshot = (self.factory)(
            &state,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let receipt = codec::bytes(
            &json!({"fingerprint":fingerprint,"revision":next.revision.get(),"result_digest":Sha256::digest(&outcome).to_vec()}),
        )?;
        let request = self.request;
        let actor = self.actor;
        let clock = self.clock;
        let session = self.session;
        Ok(TransactionDecision {
            outcome: outcome.clone(),
            commit: Some((
                vec![
                    Mutation::Put {
                        key: DOCUMENT.into(),
                        value: outcome.clone(),
                    },
                    Mutation::Put {
                        key: result_key,
                        value: outcome,
                    },
                    Mutation::Put {
                        key: receipt_key,
                        value: receipt,
                    },
                ],
                snapshot,
            )),
            precommit: Some(Box::new(move || {
                authorize(&state, &actor, clock.as_ref(), &session)?;
                current
                    .transition(
                        request.candidate.clone(),
                        request.expected_revision,
                        &state,
                        &actor,
                        clock.now(),
                    )
                    .map(|_| ())
                    .map_err(StorageError::Display)
            })),
        })
    }
}
