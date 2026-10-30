//! Single writer actual lesson observations and exact-lease override commands.
use super::{lesson_codec as dto, *};
use fc_domain::{lesson_context::*, materials::*, schedule_document::ScheduleDocument, time::*};
use fc_ports::{LessonRepository, LessonRepositoryError, LessonRequest};
pub(super) type Observations = Arc<std::sync::Mutex<Option<LocalDateTime>>>;
pub(super) fn witnessed(
    memory: &Observations,
    now: LocalDateTime,
) -> Result<Option<LocalDateTime>, StorageError> {
    let mut value = memory.lock().map_err(|_| StorageError::WorkerPanicked)?;
    let previous = *value;
    *value = Some(previous.map_or(now, |t| t.max(now)));
    Ok(previous)
}
pub(super) fn observe_with_floor(
    d: &dyn TransactionDocuments,
    i: &IdentityState,
    s: &ScheduleDocument,
    now: LocalDateTime,
    floor: Option<LocalDateTime>,
) -> Result<(LessonContextSnapshot, Option<Mutation>), StorageError> {
    let old = load(d)?;
    let next = old
        .observe_with_floor(i, s, now, floor)
        .map_err(StorageError::Lesson)?;
    let change = if old != next {
        Some(mutation(&next)?)
    } else {
        None
    };
    Ok((next, change))
}
const DOCUMENT: &str = "business/lesson-context/v1";
pub(super) fn load(d: &dyn TransactionDocuments) -> Result<LessonContextSnapshot, StorageError> {
    match d.get(DOCUMENT)? {
        Some(r) => {
            let context = dto::read_snapshot(&codec::value(&r.value)?)?;
            let identity = load_identity(d)?;
            if identity.installation.is_none()
                || context.revision > d.revision()?
                || context.identity_revision > identity.revision
            {
                return Err(StorageError::Database);
            }
            if let Some(proof) = &context.base_trust {
                if !identity.classes.iter().any(|c| c.id == proof.class_id)
                    || proof.schedule_revision > super::schedule::load(d, &identity)?.revision
                {
                    return Err(StorageError::Database);
                }
            }
            for c in context
                .current
                .iter()
                .chain(context.history.iter().map(|h| &h.context))
            {
                if !identity.classes.iter().any(|i| i.id == c.key.class_id)
                    || !identity.subjects.iter().any(|i| i.id == c.base.subject_id)
                    || !identity
                        .subjects
                        .iter()
                        .any(|i| i.id == c.effective.subject_id)
                    || c.teacher_id.is_some_and(|id| identity.account(id).is_err())
                {
                    return Err(StorageError::Database);
                }
            }
            Ok(context)
        }
        None => Ok(LessonContextSnapshot::default()),
    }
}
pub(super) fn mutation(s: &LessonContextSnapshot) -> Result<Mutation, StorageError> {
    Ok(Mutation::Put {
        key: DOCUMENT.into(),
        value: codec::bytes(&dto::snapshot(s))?,
    })
}
pub(super) fn observe(
    d: &dyn TransactionDocuments,
    i: &IdentityState,
    s: &ScheduleDocument,
    now: LocalDateTime,
) -> Result<(LessonContextSnapshot, Option<Mutation>), StorageError> {
    let old = load(d)?;
    let next = old.observe(i, s, now).map_err(StorageError::Lesson)?;
    let change = if old != next {
        Some(mutation(&next)?)
    } else {
        None
    };
    Ok((next, change))
}
pub(super) fn source(
    context: &LessonContextSnapshot,
    i: &IdentityState,
    s: &ScheduleDocument,
    class: ClassId,
    now: LocalDateTime,
) -> Result<Option<MaterialSource>, StorageError> {
    context
        .material_source(i, s, class, now)
        .map_err(StorageError::Material)
}
fn error(e: StorageError) -> LessonRepositoryError {
    match e {
        StorageError::Lesson(e) => LessonRepositoryError::Domain(e),
        StorageError::Identity(e) => LessonRepositoryError::Domain(LessonContextError::Identity(e)),
        StorageError::CommandIdReused => LessonRepositoryError::IdempotencyConflict,
        StorageError::DocumentTooLarge => LessonRepositoryError::DocumentTooLarge,
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => LessonRepositoryError::Faulted,
        _ => LessonRepositoryError::Unavailable,
    }
}
impl<S: Send + 'static> LessonRepository for BusinessRepository<S> {
    fn lesson_context(
        &self,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<LessonContextSnapshot, LessonRepositoryError> {
        let result = self
            .handle
            .try_transaction(Task {
                request: None,
                rollback_only: false,
                actor: Some(actor.clone()),
                observations: self.lesson_observations.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .and_then(|ticket| ticket.wait());
        if matches!(
            &result,
            Err(StorageError::Busy
                | StorageError::Lesson(_)
                | StorageError::Material(fc_domain::materials::MaterialError::InvalidContext))
        ) {
            self.observe_lesson()?;
        }
        let bytes = result.map_err(error)?;
        dto::read_snapshot(&codec::value(&bytes).map_err(error)?).map_err(error)
    }
    fn execute_lesson(
        &mut self,
        request: LessonRequest,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<LessonContextSnapshot, LessonRepositoryError> {
        self.observe_lesson()?;
        let result = self
            .handle
            .try_transaction(Task {
                request: Some(request),
                rollback_only: false,
                actor: Some(actor.clone()),
                observations: self.lesson_observations.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .and_then(|ticket| ticket.wait());
        if matches!(
            &result,
            Err(StorageError::Busy
                | StorageError::Lesson(_)
                | StorageError::Material(fc_domain::materials::MaterialError::InvalidContext))
        ) {
            self.observe_lesson()?;
        }
        let bytes = result.map_err(error)?;
        dto::read_snapshot(&codec::value(&bytes).map_err(error)?).map_err(error)
    }
}
impl<S: Send + 'static> BusinessRepository<S> {
    pub(super) fn observe_lesson(&self) -> Result<(), LessonRepositoryError> {
        self.observe_lesson_mode(false)
    }
    /// A rejected Facts read cannot establish positive history or baseline trust.
    /// Only a real clock regression observed by that failed transaction may be
    /// durably invalidated independently, so a restart cannot revive its source.
    pub(super) fn observe_lesson_rollback_barrier(&self) -> Result<(), LessonRepositoryError> {
        self.observe_lesson_mode(true)
    }
    fn observe_lesson_mode(&self, rollback_only: bool) -> Result<(), LessonRepositoryError> {
        self.handle
            .try_transaction(Task {
                request: None,
                rollback_only,
                actor: None,
                observations: self.lesson_observations.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        Ok(())
    }
}
fn authorize(
    i: &IdentityState,
    a: &ActorContext,
    c: &dyn Clock,
    s: &SessionGuard,
) -> Result<(), StorageError> {
    s(i, a)?;
    i.authorize_management(a, c.now())
        .map_err(StorageError::Identity)
}
struct Task<S> {
    request: Option<LessonRequest>,
    rollback_only: bool,
    actor: Option<ActorContext>,
    observations: Observations,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for Task<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        if self.rollback_only && identity.installation.is_none() {
            return Ok(TransactionDecision {
                outcome: codec::bytes(&dto::snapshot(&LessonContextSnapshot::default()))?,
                commit: None,
                precommit: None,
            });
        }
        if let Some(actor) = &self.actor {
            authorize(&identity, actor, self.clock.as_ref(), &self.session)?;
        }
        let schedule = super::schedule::load(d, &identity)?;
        let now = self.clock.now();
        let floor = witnessed(&self.observations, now)?;
        let date_floor = super::desktop::read_watermark(d, &identity)?
            .filter(|day| *day > now.date)
            .map(|day| LocalDateTime::new(day, 0).map_err(|_| StorageError::Database))
            .transpose()?;
        let (observed, observation) = observe_with_floor(
            d,
            &identity,
            &schedule,
            now,
            floor.into_iter().chain(date_floor).max(),
        )?;
        if self.rollback_only && observed.blocked_until.is_none() {
            return Ok(TransactionDecision {
                outcome: codec::bytes(&dto::snapshot(&observed))?,
                commit: None,
                precommit: None,
            });
        }
        let mut mutations = Vec::new();
        let mut final_context = observed.clone();
        if let Some(r) = &self.request {
            let mut hash = Sha256::new();
            hash.update(b"future-classroom/lesson/v1\0");
            hash.update(codec::bytes(&codec::actor(
                self.actor.as_ref().ok_or(StorageError::InvalidCommand)?,
            ))?);
            hash.update(codec::bytes(&json!({"expected":r.expected_revision.get(),"lesson":super::materials_codec::lesson(&r.lesson),"action":dto::action(&r.action)}))?);
            let fingerprint = hash.finalize().to_vec();
            let receipt = format!("business/receipt/lesson/{}", r.command_id.get());
            if let Some(row) = d.get(&receipt)? {
                let v = codec::value(&row.value)?;
                codec::object_fields(&v, &["fingerprint", "result", "digest"])?;
                let old: Vec<u8> = serde_json::from_value(v["fingerprint"].clone())
                    .map_err(|_| StorageError::Database)?;
                let result = codec::bytes(&v["result"])?;
                let digest: Vec<u8> = serde_json::from_value(v["digest"].clone())
                    .map_err(|_| StorageError::Database)?;
                if old.len() != 32
                    || digest.len() != 32
                    || digest.as_slice() != Sha256::digest(&result).as_slice()
                {
                    return Err(StorageError::Database);
                }
                if old != fingerprint {
                    return Err(StorageError::CommandIdReused);
                }
                let original = dto::read_snapshot(&v["result"])?;
                if original.revision
                    != r.expected_revision
                        .next()
                        .map_err(|_| StorageError::Database)?
                    || !original.current.as_ref().is_some_and(|c| c.key == r.lesson)
                {
                    return Err(StorageError::Database);
                }
                match &r.action {
                    LessonAction::Replace(target) => {
                        if !original.active_override.as_ref().is_some_and(|o| {
                            o.id.get() == r.command_id.get()
                                && o.actor_id
                                    == self
                                        .actor
                                        .as_ref()
                                        .map(|a| a.principal_id)
                                        .unwrap_or(o.actor_id)
                                && &o.target == target
                                && o.key == r.lesson
                        }) {
                            return Err(StorageError::Database);
                        }
                    }
                    LessonAction::Restore => {
                        if original.active_override.is_some() {
                            return Err(StorageError::Database);
                        }
                    }
                }
                if let Some(actor) = &self.actor {
                    authorize(&identity, actor, self.clock.as_ref(), &self.session)?;
                }
                return Ok(TransactionDecision {
                    outcome: result,
                    commit: None,
                    precommit: None,
                });
            }
            final_context = observed
                .apply(
                    r.expected_revision,
                    &r.lesson,
                    r.action.clone(),
                    OverrideId::new(r.command_id.get())
                        .map_err(|_| StorageError::InvalidCommand)?,
                    &identity,
                    self.actor.as_ref().ok_or(StorageError::InvalidCommand)?,
                    now,
                )
                .map_err(StorageError::Lesson)?;
            let result = dto::snapshot(&final_context);
            let bytes = codec::bytes(&result)?;
            mutations.push(Mutation::Put{key:receipt,value:codec::bytes(&json!({"fingerprint":fingerprint,"result":result,"digest":Sha256::digest(&bytes).to_vec()}))?});
        }
        if observation.is_some() || self.request.is_some() {
            mutations.push(mutation(&final_context)?)
        }
        let outcome = codec::bytes(&dto::snapshot(&final_context))?;
        if mutations.is_empty() {
            return Ok(TransactionDecision {
                outcome,
                commit: None,
                precommit: None,
            });
        }
        let snapshot = (self.factory)(
            &identity,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let actor = self.actor;
        let clock = self.clock;
        let session = self.session;
        let request = self.request;
        let observations = self.observations;
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                if let Some(actor) = &actor {
                    authorize(&identity, actor, clock.as_ref(), &session)?;
                }
                let fresh_now = clock.now();
                let floor = witnessed(&observations, fresh_now)?;
                let fresh = observed
                    .observe_with_floor(&identity, &schedule, fresh_now, floor)
                    .map_err(StorageError::Lesson)?;
                if fresh.current != observed.current
                    || fresh.active_override != observed.active_override
                    || fresh.blocked_until != observed.blocked_until
                    || fresh.history != observed.history
                    || fresh.base_trust != observed.base_trust
                {
                    return Err(StorageError::Lesson(
                        LessonContextError::LessonContextChanged,
                    ));
                }
                if let Some(r) = request {
                    fresh
                        .apply(
                            r.expected_revision,
                            &r.lesson,
                            r.action,
                            OverrideId::new(r.command_id.get())
                                .map_err(|_| StorageError::InvalidCommand)?,
                            &identity,
                            actor.as_ref().ok_or(StorageError::InvalidCommand)?,
                            clock.now(),
                        )
                        .map_err(StorageError::Lesson)?;
                }
                Ok(())
            })),
        })
    }
}
