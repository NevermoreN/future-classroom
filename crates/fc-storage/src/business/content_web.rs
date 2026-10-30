//! Authenticated receipt queries on the sole writer, without changing v1 receipts.
use super::*;
use fc_domain::time::{LocalDate, LocalDateTime};
use fc_ports::ContentOutcomeRepository;

pub(super) fn key_date(key: &ContentKey) -> LocalDate {
    match key {
        ContentKey::Homework(key) => key.actual_date,
        ContentKey::SharedReminder(key) => key.actual_date,
    }
}
pub(super) fn authorize_key(
    key: &ContentKey,
    authority: &AuthorityContext,
    actor: &ActorContext,
    now: LocalDateTime,
) -> Result<(), StorageError> {
    match key {
        ContentKey::Homework(key) => authority.authorize_owned_content(
            actor,
            now,
            key.class_id,
            key.subject_id,
            key.owner_teacher_id,
        ),
        ContentKey::SharedReminder(key) => {
            authority.authorize_shared_reminder(actor, now, key.class_id)
        }
    }
    .map_err(|error| StorageError::Content(ContentError::Permission(error)))
}
/// HTTP callers cannot use a global role to publish a dangling teacher/subject
/// reference. Native's historic global override semantics stay in the old API.
pub(super) fn validate_web_target(
    identity: &IdentityState,
    key: &ContentKey,
    actor: &ActorContext,
) -> Result<(), StorageError> {
    if actor.origin != TrustedOrigin::Web {
        return Err(StorageError::Content(ContentError::Permission(
            PermissionError::WebOnly,
        )));
    }
    validate_homework_reference(identity, key)
}
fn validate_homework_reference(
    identity: &IdentityState,
    key: &ContentKey,
) -> Result<(), StorageError> {
    if let ContentKey::Homework(key) = key {
        if !identity.is_teacher(key.owner_teacher_id, key.class_id, key.subject_id) {
            return Err(StorageError::Content(ContentError::Permission(
                PermissionError::OutsideTeachingScope,
            )));
        }
    }
    Ok(())
}
/// Persisted observation of a later day is authoritative even after wall rollback.
pub(super) fn date_is_current(
    documents: &dyn TransactionDocuments,
    identity: &IdentityState,
    key: &ContentKey,
    now: LocalDateTime,
) -> Result<bool, StorageError> {
    Ok(key_date(key) == now.date
        && !desktop::read_watermark(documents, identity)?.is_some_and(|floor| now.date < floor))
}
pub(super) fn require_current_day(
    documents: &dyn TransactionDocuments,
    identity: &IdentityState,
    key: &ContentKey,
    now: LocalDateTime,
) -> Result<(), StorageError> {
    if date_is_current(documents, identity, key, now)? {
        Ok(())
    } else {
        Err(StorageError::Content(ContentError::WrongDate))
    }
}
/// v1 has a payload fingerprint and record, not a separate result digest. Keep
/// that schema, checking exact fields, 32 fingerprint bytes and canonical record.
pub(super) fn receipt(value: &Value) -> Result<ContentRecord, StorageError> {
    codec::object_fields(value, &["fingerprint", "record"])?;
    let _: [u8; 32] =
        serde_json::from_value(value["fingerprint"].clone()).map_err(|_| StorageError::Database)?;
    codec::read_record(&value["record"])
}
fn authorize_observation(
    identity: &IdentityState,
    actor: &ActorContext,
    now: LocalDateTime,
) -> Result<(), StorageError> {
    let current = authority(identity, actor)?;
    current
        .validate_actor(actor, now)
        .map_err(|e| StorageError::Content(ContentError::Permission(e)))?;
    if actor.origin != TrustedOrigin::Web {
        return Err(StorageError::Content(ContentError::Permission(
            PermissionError::WebOnly,
        )));
    }
    if !matches!(
        current.principal.role,
        Role::GlobalAdmin | Role::TeacherAdmin
    ) {
        return Err(StorageError::Content(ContentError::Permission(
            PermissionError::RoleDenied,
        )));
    }
    Ok(())
}
/// Phase one contains no slot/receipt/body and therefore cannot reveal foreign
/// commands. Only a first authenticated observation of a new day writes/publishes.
struct ObserveContentDay<S> {
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for ObserveContentDay<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        documents: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(documents)?;
        (self.session)(&identity, &self.actor)?;
        let now = self.clock.now();
        authorize_observation(&identity, &self.actor, now)?;
        if desktop::read_watermark(documents, &identity)?.is_some_and(|floor| now.date < floor) {
            return Err(StorageError::Content(ContentError::WrongDate));
        }
        let mutation = desktop::advance_watermark(documents, &identity, now.date)?;
        let commit = if let Some(mutation) = mutation {
            let snapshot = (self.factory)(
                &identity,
                documents
                    .revision()?
                    .next()
                    .map_err(|_| StorageError::InvalidCommand)?,
            )?;
            Some((vec![mutation], snapshot))
        } else {
            None
        };
        (self.session)(&identity, &self.actor)?;
        let final_now = self.clock.now();
        authorize_observation(&identity, &self.actor, final_now)?;
        if final_now.date != now.date {
            return Err(StorageError::Content(ContentError::WrongDate));
        }
        let session = self.session.clone();
        let actor = self.actor.clone();
        let clock = self.clock.clone();
        let precommit = commit.as_ref().map(|_| {
            Box::new(move || {
                session(&identity, &actor)?;
                let final_now = clock.now();
                authorize_observation(&identity, &actor, final_now)?;
                if final_now.date != now.date {
                    return Err(StorageError::Content(ContentError::WrongDate));
                }
                Ok(())
            }) as Box<dyn FnOnce() -> Result<(), StorageError> + Send>
        });
        Ok(TransactionDecision {
            precommit,
            outcome: codec::bytes(&codec::time(now))?,
            commit,
        })
    }
}
struct ReadOutcome {
    command_id: CommandId,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for ReadOutcome {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        documents: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(documents)?;
        (self.session)(&identity, &self.actor)?;
        let authority = authority(&identity, &self.actor)?;
        authority
            .validate_actor(&self.actor, self.clock.now())
            .map_err(|e| StorageError::Content(ContentError::Permission(e)))?;
        let stored = documents.get(&format!(
            "business/receipt/content/{}",
            self.command_id.get()
        ))?;
        let record = stored
            .as_ref()
            .map(|row| codec::value(&row.value).and_then(|value| receipt(&value)))
            .transpose()?;
        let owned = record
            .as_ref()
            .filter(|record| record.last_actor_id == self.actor.principal_id);
        if let Some(record) = owned {
            require_current_day(documents, &identity, &record.key, self.clock.now())?;
            validate_homework_reference(&identity, &record.key)?;
            authorize_key(&record.key, &authority, &self.actor, self.clock.now())?;
        }
        // This final check also gates None: reads must not renew or outlive the
        // captured exact lease while disk/SQLite was slow.
        (self.session)(&identity, &self.actor)?;
        let now = self.clock.now();
        authority
            .validate_actor(&self.actor, now)
            .map_err(|e| StorageError::Content(ContentError::Permission(e)))?;
        let value = match owned {
            Some(record) => {
                require_current_day(documents, &identity, &record.key, now)?;
                validate_homework_reference(&identity, &record.key)?;
                authorize_key(&record.key, &authority, &self.actor, now)?;
                codec::record(record)
            }
            _ => Value::Null,
        };
        Ok(TransactionDecision {
            precommit: None,
            outcome: codec::bytes(&value)?,
            commit: None,
        })
    }
}
impl<S: Send + 'static> BusinessRepository<S> {
    /// Observe Web's current local day without any content lookup. Only the
    /// first new day commits/publishes; repeat reads preserve global revision.
    pub fn observe_content_day(
        &self,
        actor: &ActorContext,
    ) -> Result<LocalDateTime, RepositoryError> {
        // A slow COMMIT can finish just past midnight. One bounded second
        // observation makes that new day durable before returning it to Web.
        for _ in 0..2 {
            let bytes = self
                .handle
                .try_transaction(ObserveContentDay {
                    actor: actor.clone(),
                    clock: self.clock.clone(),
                    session: self.session.clone(),
                    factory: self.factory.clone(),
                })
                .map_err(repository_error)?
                .wait()
                .map_err(repository_error)?;
            let observed = codec::read_time(&codec::value(&bytes).map_err(repository_error)?)
                .map_err(repository_error)?;
            let current = self.clock.now();
            if current.date == observed.date {
                return Ok(observed);
            }
            if current.date < observed.date {
                return Err(RepositoryError::Domain(ContentError::WrongDate));
            }
        }
        Err(RepositoryError::Unavailable)
    }
    /// Web slot reads preserve a cleared slot's revision and explicitly reject
    /// a stale request date or a rollback below the durable observed-day floor.
    pub fn content_for_web(
        &self,
        key: ContentKey,
        actor: &ActorContext,
    ) -> Result<Option<ContentRecord>, RepositoryError> {
        self.observe_content_day(actor)?;
        let bytes = self
            .handle
            .try_transaction(ReadContent {
                web_strict: true,
                key,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        let value = codec::value(&bytes).map_err(repository_error)?;
        if value.is_null() {
            Ok(None)
        } else {
            codec::read_record(&value)
                .map(Some)
                .map_err(repository_error)
        }
    }
    /// Same writer path as native content commits, with additional fresh Web
    /// origin/teacher/subject/class reference checks, including pre-COMMIT.
    pub fn execute_content_for_web(
        &mut self,
        command: ContentCommand,
        actor: &ActorContext,
    ) -> Result<ContentRecord, RepositoryError> {
        self.observe_content_day(actor)?;
        let command_id = command.command_id;
        let bytes = self
            .handle
            .try_transaction(ContentTask {
                web_strict: true,
                command,
                actor: actor.clone(),
                clock: self.clock.clone(),
                factory: self.factory.clone(),
                session: self.session.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        // The write is already COMMITTED. A slow COMMIT/publication can cross
        // midnight or lose its lease. Release only through a new read transaction;
        // any failure here is RESULT_UNKNOWN, never a definite pre-write denial.
        codec::read_record(&codec::value(&bytes).map_err(|_| RepositoryError::Unavailable)?)
            .map_err(|_| RepositoryError::Unavailable)?;
        self.content_outcome(command_id, actor)
            .map_err(|_| RepositoryError::Unavailable)?
            .ok_or(RepositoryError::Unavailable)
    }
    /// Historical COMMITTED result, never a claim that its text is still visible.
    pub fn content_outcome(
        &self,
        command_id: CommandId,
        actor: &ActorContext,
    ) -> Result<Option<ContentRecord>, RepositoryError> {
        self.observe_content_day(actor)?;
        let bytes = self
            .handle
            .try_transaction(ReadOutcome {
                command_id,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        let value = codec::value(&bytes).map_err(repository_error)?;
        if value.is_null() {
            Ok(None)
        } else {
            codec::read_record(&value)
                .map(Some)
                .map_err(repository_error)
        }
    }
}
impl<S: Send + 'static> ContentOutcomeRepository for BusinessRepository<S> {
    fn content_outcome(
        &self,
        command_id: CommandId,
        actor: &ActorContext,
    ) -> Result<Option<ContentRecord>, RepositoryError> {
        BusinessRepository::content_outcome(self, command_id, actor)
    }
}
