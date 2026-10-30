//! Timetable aggregate on the existing sole writer: independent business CAS,
//! exact local administrator lease, atomic document/result/journal publication.
use super::schedule_codec as dto;
use super::*;
use fc_domain::{schedule::*, schedule_document::*};
use fc_ports::{ScheduleRepository, ScheduleRepositoryError, ScheduleRequest};
const DOCUMENT: &str = "business/schedule/v1";
const CHANGE: &str = "business/schedule/last-changed/v1";
pub(super) struct ChangeProof {
    pub(super) at: fc_domain::time::LocalDateTime,
    pub(super) observed_at: Option<fc_domain::time::LocalDateTime>,
}
pub(super) fn change_proof(
    d: &dyn TransactionDocuments,
) -> Result<Option<ChangeProof>, StorageError> {
    let Some(row) = d.get(CHANGE)? else {
        return Ok(None);
    };
    if d.get(DOCUMENT)?.is_none() {
        return Err(StorageError::Database);
    }
    let v = codec::value(&row.value)?;
    codec::object_fields(&v, &["format", "at", "unsettled", "observed_at"])?;
    if codec::number(&v, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let at = codec::read_time(&v["at"])?;
    let unsettled = codec::boolean(&v, "unsettled")?;
    let observed_at = if v["observed_at"].is_null() {
        None
    } else {
        Some(codec::read_time(&v["observed_at"])?)
    };
    if unsettled != observed_at.is_none() || observed_at.is_some_and(|observed| observed < at) {
        return Err(StorageError::Database);
    }
    Ok(Some(ChangeProof { at, observed_at }))
}
pub(super) fn confirm_change(
    d: &dyn TransactionDocuments,
    now: fc_domain::time::LocalDateTime,
) -> Result<Option<Mutation>, StorageError> {
    let Some(proof) = change_proof(d)? else {
        return Ok(None);
    };
    if proof.observed_at.is_some() {
        return Ok(None);
    }
    Ok(Some(Mutation::Put {
        key: CHANGE.into(),
        value: codec::bytes(
            &json!({"format":1,"at":codec::time(proof.at),"unsettled":false,"observed_at":codec::time(proof.at.max(now))}),
        )?,
    }))
}

struct LimitedBuffer(Vec<u8>);
impl std::io::Write for LimitedBuffer {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        if input.len() > MAX_VALUE_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("schedule document allocation limit"));
        }
        self.0.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode(value: &Value) -> Result<Vec<u8>, StorageError> {
    let mut output = LimitedBuffer(Vec::with_capacity(4096));
    serde_json::to_writer(&mut output, value).map_err(|_| StorageError::DocumentTooLarge)?;
    Ok(output.0)
}
fn read_document(bytes: &[u8]) -> Result<ScheduleDocument, StorageError> {
    let doc = dto::read_document(&codec::value(bytes)?)?;
    doc.validate_structure()
        .map_err(|_| StorageError::Database)?;
    Ok(doc)
}
pub(super) fn load(
    d: &dyn TransactionDocuments,
    identity: &IdentityState,
) -> Result<ScheduleDocument, StorageError> {
    let Some(row) = d.get(DOCUMENT)? else {
        return Ok(ScheduleDocument::default());
    };
    if identity.installation.is_none() {
        return Err(StorageError::Database);
    }
    let doc = read_document(&row.value)?;
    if doc.revision == Revision::default() {
        return Err(StorageError::Database);
    }
    doc.validate(identity).map_err(|_| StorageError::Database)?;
    Ok(doc)
}
/// Catalog edits cannot silently switch a Default binding or leave any live or
/// retained timetable history referencing disabled/nonexistent teaching scope.
pub(super) fn check_identity_change(
    d: &dyn TransactionDocuments,
    old: &IdentityState,
    next: &IdentityState,
) -> Result<(), StorageError> {
    let document = load(d, old)?;
    document
        .validate(next)
        .map_err(|_| StorageError::Identity(IdentityError::InvalidScope))?;
    for plan in &document.semesters {
        let tables = plan
            .semester
            .tables
            .iter()
            .chain(plan.dual_history.iter().flat_map(|h| h.tables.iter()));
        for row in tables.flat_map(|t| t.rows.iter()) {
            for cell in row.courses.iter().flatten() {
                if cell.teacher == TeacherBinding::Default {
                    let before = old.resolve_teacher(plan.semester.class_id, cell.subject_id, None);
                    let after = next.resolve_teacher(plan.semester.class_id, cell.subject_id, None);
                    if before != after {
                        return Err(StorageError::Identity(IdentityError::InvalidScope));
                    }
                }
            }
        }
    }
    Ok(())
}
fn error(error: StorageError) -> ScheduleRepositoryError {
    match error {
        StorageError::Schedule(e) => ScheduleRepositoryError::Domain(e),
        StorageError::CommandIdReused => ScheduleRepositoryError::IdempotencyConflict,
        StorageError::DocumentTooLarge => ScheduleRepositoryError::DocumentTooLarge,
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => ScheduleRepositoryError::Faulted,
        _ => ScheduleRepositoryError::Unavailable,
    }
}
impl<S: Send + 'static> ScheduleRepository for BusinessRepository<S> {
    fn schedule(&self, actor: &ActorContext) -> Result<ScheduleDocument, ScheduleRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(ReadSchedule {
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        read_document(&bytes).map_err(error)
    }
    fn execute_schedule(
        &mut self,
        request: ScheduleRequest,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<ScheduleDocument, ScheduleRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(SaveSchedule {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        read_document(&bytes).map_err(error)
    }
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
        .map_err(|error| {
            StorageError::Schedule(ScheduleDocumentError {
                kind: DocumentErrorKind::Identity(error),
                location: ErrorLocation::default(),
            })
        })
}
struct ReadSchedule {
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for ReadSchedule {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        let document = load(d, &identity)?;
        let outcome = encode(&dto::document(&document))?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        Ok(TransactionDecision {
            precommit: None,
            outcome,
            commit: None,
        })
    }
}
struct SaveSchedule<S> {
    request: ScheduleRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for SaveSchedule<S> {
    fn allocation_bytes(&self) -> usize {
        self.request
            .candidate
            .allocation_bytes()
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>()))
            .unwrap_or(usize::MAX)
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        self.request
            .candidate
            .validate_draft_structure()
            .map_err(StorageError::Schedule)?;
        let candidate = encode(&dto::document(&self.request.candidate))?;
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"future-classroom/schedule/v1\0");
        fingerprint.update(self.request.expected_revision.get().to_be_bytes());
        fingerprint.update(codec::bytes(&codec::actor(&self.actor))?);
        fingerprint.update(candidate);
        let fingerprint = fingerprint.finalize().to_vec();
        let journal = format!(
            "business/receipt/schedule/{}",
            self.request.command_id.get()
        );
        let result_key = format!("business/result/schedule/{}", self.request.command_id.get());
        if let Some(receipt) = d.get(&journal)? {
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
            let result_digest: Vec<u8> = serde_json::from_value(value["result_digest"].clone())
                .map_err(|_| StorageError::Database)?;
            if result_digest.len() != 32
                || result_digest.as_slice() != Sha256::digest(&result.value).as_slice()
            {
                return Err(StorageError::Database);
            }
            let doc = read_document(&result.value)?;
            if doc.revision != Revision::new(codec::number(&value, "revision")?)
                || doc.revision
                    != self
                        .request
                        .expected_revision
                        .next()
                        .map_err(|_| StorageError::Database)?
            {
                return Err(StorageError::Database);
            }
            authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
            return Ok(TransactionDecision {
                precommit: None,
                outcome: result.value,
                commit: None,
            });
        }
        let current = load(d, &identity)?;
        let next = current
            .transition(
                &self.request.candidate,
                self.request.expected_revision,
                &identity,
                &self.actor,
                self.clock.now(),
            )
            .map_err(StorageError::Schedule)?;
        let outcome = encode(&dto::document(&next))?;
        let changed_at = self.clock.now();
        let snapshot = (self.factory)(
            &identity,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let journal_bytes = codec::bytes(
            &json!({"fingerprint":fingerprint,"revision":next.revision.get(),"result_digest":Sha256::digest(&outcome).to_vec()}),
        )?;
        let actor = self.actor.clone();
        let clock = self.clock.clone();
        let session = self.session.clone();
        let request = self.request;
        let (_, lesson_change) = super::lesson::observe(d, &identity, &next, changed_at)?;
        let mut mutations = vec![
            Mutation::Put {
                key: DOCUMENT.into(),
                value: outcome.clone(),
            },
            Mutation::Put {
                key: result_key,
                value: outcome.clone(),
            },
            Mutation::Put {
                key: journal,
                value: journal_bytes,
            },
            Mutation::Put {
                key: CHANGE.into(),
                value: codec::bytes(
                    &json!({"format":1,"at":codec::time(changed_at),"unsettled":true,"observed_at":null}),
                )?,
            },
        ];
        if let Some(change) = lesson_change {
            mutations.push(change);
        }
        Ok(TransactionDecision {
            outcome: outcome.clone(),
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                authorize(&identity, &actor, clock.as_ref(), &session)?;
                current
                    .transition(
                        &request.candidate,
                        request.expected_revision,
                        &identity,
                        &actor,
                        clock.now(),
                    )
                    .map(|_| ())
                    .map_err(StorageError::Schedule)
            })),
        })
    }
}
