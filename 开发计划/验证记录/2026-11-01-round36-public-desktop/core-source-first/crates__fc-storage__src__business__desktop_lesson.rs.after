//! Actorless native desktop commands on the existing bounded writer. A scoped
//! memory capability is required; an absent actor is never treated as authority.
use super::*;
use fc_domain::{lesson_context::*, schedule_document::ScheduleDocument};
use fc_ports::{
    DesktopLessonBinding, DesktopLessonRepository, DesktopLessonRequest, DesktopLessonSource,
    DesktopLessonTeacher, LessonRepositoryError,
};

fn check_guard(guard: &Option<DesktopLessonGuard>) -> Result<(), StorageError> {
    if guard
        .as_ref()
        .is_some_and(|guard| catch_unwind(AssertUnwindSafe(|| guard())).unwrap_or(false))
    {
        Ok(())
    } else {
        Err(StorageError::ExecutionCancelled)
    }
}
fn changed() -> StorageError {
    StorageError::Lesson(LessonContextError::LessonContextChanged)
}
fn metadata(
    identity: &IdentityState,
    schedule: &ScheduleDocument,
    binding: &DesktopLessonBinding,
    now: fc_domain::time::LocalDateTime,
) -> Result<(), StorageError> {
    if identity.revision != binding.expected_identity_revision
        || schedule.revision != binding.expected_schedule_revision
        || identity.installation.as_ref().and_then(|i| i.current_class)
            != Some(binding.lesson.class_id)
        || !identity
            .classes
            .iter()
            .any(|c| c.id == binding.lesson.class_id && c.enabled)
        || binding.lesson.actual_date != now.date
        || !binding.lesson.range.contains_second(now.second_of_day())
    {
        return Err(changed());
    }
    Ok(())
}
fn current(
    context: &LessonContextSnapshot,
    binding: &DesktopLessonBinding,
    exact_revision: bool,
) -> Result<(), StorageError> {
    if context.blocked_until.is_some()
        || !context
            .current
            .as_ref()
            .is_some_and(|c| c.key == binding.lesson)
    {
        return Err(changed());
    }
    if exact_revision && context.revision != binding.expected_revision {
        return Err(StorageError::Lesson(LessonContextError::RevisionConflict {
            current: context.revision,
        }));
    }
    Ok(())
}
fn source(
    identity: &IdentityState,
    schedule: &ScheduleDocument,
    context: LessonContextSnapshot,
) -> DesktopLessonSource {
    let class = context.class_id;
    let mut subjects: Vec<_> = identity
        .subjects
        .iter()
        .filter(|s| s.enabled)
        .cloned()
        .collect();
    subjects.sort_unstable_by_key(|s| s.id);
    let mut teachers: Vec<_> = identity
        .accounts
        .iter()
        .filter(|account| account.principal.enabled && account.principal.role != Role::User)
        .filter_map(|account| {
            // Same eligibility as is_teacher, scanning only actual assignments
            // rather than the account × subject × assignment Cartesian product.
            let mut ids: Vec<_> = identity
                .assignments
                .iter()
                .filter(|assignment| {
                    assignment.enabled
                        && assignment.teacher_id == account.principal.id
                        && Some(assignment.class_id) == class
                        && subjects
                            .binary_search_by_key(&assignment.subject_id, |s| s.id)
                            .is_ok()
                })
                .map(|assignment| assignment.subject_id)
                .collect();
            ids.sort_unstable();
            ids.dedup();
            (!ids.is_empty()).then(|| DesktopLessonTeacher {
                id: account.principal.id,
                display_name: account.display_name.clone(),
                subject_ids: ids,
            })
        })
        .collect();
    teachers.sort_unstable_by_key(|t| t.id);
    DesktopLessonSource {
        context,
        catalog_revision: identity.revision,
        schedule_revision: schedule.revision,
        subjects,
        teachers,
    }
}
fn subject_value(s: &SchoolSubject) -> Value {
    json!({"id":s.id.get(),"school":s.school_id.get(),"name":s.name,"enabled":s.enabled,
        "rgb":[s.color.red,s.color.green,s.color.blue],"revision":s.revision.get()})
}
fn value(s: &DesktopLessonSource) -> Value {
    json!({"format":1,"context":lesson_codec::snapshot(&s.context),"catalog_revision":s.catalog_revision.get(),
        "schedule_revision":s.schedule_revision.get(),"subjects":s.subjects.iter().map(subject_value).collect::<Vec<_>>(),
        "teachers":s.teachers.iter().map(|t|json!({"id":t.id.get(),"display_name":t.display_name,"subjects":t.subject_ids.iter().map(|id|id.get()).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}
fn read(value: &Value) -> Result<DesktopLessonSource, StorageError> {
    codec::object_fields(
        value,
        &[
            "format",
            "context",
            "catalog_revision",
            "schedule_revision",
            "subjects",
            "teachers",
        ],
    )?;
    if codec::number(value, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let rows = codec::array(value, "subjects")?;
    if rows.len() > MAX_SUBJECTS {
        return Err(StorageError::Database);
    }
    let subjects = rows
        .iter()
        .map(|s| {
            codec::object_fields(s, &["id", "school", "name", "enabled", "rgb", "revision"])?;
            let rgb = codec::array(s, "rgb")?;
            if rgb.len() != 3 {
                return Err(StorageError::Database);
            }
            let channel = |n: usize| {
                u8::try_from(rgb[n].as_u64().ok_or(StorageError::Database)?)
                    .map_err(|_| StorageError::Database)
            };
            let subject = SchoolSubject {
                id: SubjectId::new(codec::number(s, "id")?).map_err(|_| StorageError::Database)?,
                school_id: SchoolId::new(codec::number(s, "school")?)
                    .map_err(|_| StorageError::Database)?,
                name: codec::text(s, "name")?,
                enabled: codec::boolean(s, "enabled")?,
                color: Rgb {
                    red: channel(0)?,
                    green: channel(1)?,
                    blue: channel(2)?,
                },
                revision: Revision::new(codec::number(s, "revision")?),
            };
            if !subject.enabled
                || subject.revision.get() == 0
                || normalized_name(&subject.name, 64).is_err()
            {
                return Err(StorageError::Database);
            }
            Ok(subject)
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    if subjects.windows(2).any(|s| s[0].id >= s[1].id)
        || subjects
            .first()
            .is_some_and(|first| subjects.iter().any(|s| s.school_id != first.school_id))
    {
        return Err(StorageError::Database);
    }
    let rows = codec::array(value, "teachers")?;
    if rows.len() > MAX_ACCOUNTS {
        return Err(StorageError::Database);
    }
    let teachers = rows
        .iter()
        .map(|t| {
            codec::object_fields(t, &["id", "display_name", "subjects"])?;
            let ids = codec::array(t, "subjects")?;
            if ids.is_empty() || ids.len() > MAX_SUBJECTS {
                return Err(StorageError::Database);
            }
            let subject_ids = ids
                .iter()
                .map(|id| {
                    SubjectId::new(id.as_u64().ok_or(StorageError::Database)?)
                        .map_err(|_| StorageError::Database)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if subject_ids.windows(2).any(|s| s[0] >= s[1])
                || subject_ids
                    .iter()
                    .any(|id| !subjects.iter().any(|s| s.id == *id))
            {
                return Err(StorageError::Database);
            }
            let display_name = codec::text(t, "display_name")?;
            normalized_name(&display_name, 128).map_err(|_| StorageError::Database)?;
            Ok(DesktopLessonTeacher {
                id: PrincipalId::new(codec::number(t, "id")?)
                    .map_err(|_| StorageError::Database)?,
                display_name,
                subject_ids,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    if teachers.windows(2).any(|t| t[0].id >= t[1].id) {
        return Err(StorageError::Database);
    }
    let result = DesktopLessonSource {
        context: lesson_codec::read_snapshot(&value["context"])?,
        catalog_revision: Revision::new(codec::number(value, "catalog_revision")?),
        schedule_revision: Revision::new(codec::number(value, "schedule_revision")?),
        subjects,
        teachers,
    };
    if result.catalog_revision.get() == 0
        || result.context.identity_revision != result.catalog_revision
    {
        return Err(StorageError::Database);
    }
    Ok(result)
}
impl<S: Send + 'static> DesktopLessonRepository for BusinessRepository<S> {
    fn desktop_lesson(
        &self,
        binding: DesktopLessonBinding,
    ) -> Result<DesktopLessonSource, LessonRepositoryError> {
        check_guard(&self.desktop_lesson_guard).map_err(lesson::error)?;
        let bytes = self
            .handle
            .try_transaction(Task {
                binding,
                request: None,
                guard: self.desktop_lesson_guard.clone(),
                observations: self.lesson_observations.clone(),
                clock: self.clock.clone(),
                factory: self.factory.clone(),
            })
            .and_then(|t| t.wait())
            .map_err(lesson::error)?;
        check_guard(&self.desktop_lesson_guard).map_err(lesson::error)?;
        let source = read(&codec::value(&bytes).map_err(lesson::error)?).map_err(lesson::error)?;
        check_guard(&self.desktop_lesson_guard).map_err(lesson::error)?;
        Ok(source)
    }
    fn execute_desktop_lesson(
        &mut self,
        request: DesktopLessonRequest,
    ) -> Result<DesktopLessonSource, LessonRepositoryError> {
        check_guard(&self.desktop_lesson_guard).map_err(lesson::error)?;
        let bytes = self
            .handle
            .try_transaction(Task {
                binding: request.binding.clone(),
                request: Some(request),
                guard: self.desktop_lesson_guard.clone(),
                observations: self.lesson_observations.clone(),
                clock: self.clock.clone(),
                factory: self.factory.clone(),
            })
            .and_then(|t| t.wait())
            .map_err(lesson::error)?;
        check_guard(&self.desktop_lesson_guard).map_err(lesson::error)?;
        let source = read(&codec::value(&bytes).map_err(lesson::error)?).map_err(lesson::error)?;
        check_guard(&self.desktop_lesson_guard).map_err(lesson::error)?;
        Ok(source)
    }
}
struct Task<S> {
    binding: DesktopLessonBinding,
    request: Option<DesktopLessonRequest>,
    guard: Option<DesktopLessonGuard>,
    observations: lesson::Observations,
    clock: Arc<dyn Clock + Send + Sync>,
    factory: Factory<S>,
}
impl<S: Send + 'static> TransactionTask<S> for Task<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        check_guard(&self.guard)?;
        let identity = load_identity(d)?;
        let schedule = schedule::load(d, &identity)?;
        let now = self.clock.now();
        metadata(&identity, &schedule, &self.binding, now)?;
        let memory_floor = lesson::witnessed(&self.observations, now)?;
        let date_floor = desktop::read_watermark(d, &identity)?
            .filter(|day| *day > now.date)
            .map(|day| {
                fc_domain::time::LocalDateTime::new(day, 0).map_err(|_| StorageError::Database)
            })
            .transpose()?;
        let (observed, observation) = lesson::observe_with_floor(
            d,
            &identity,
            &schedule,
            now,
            memory_floor.into_iter().chain(date_floor).max(),
        )?;
        current(&observed, &self.binding, self.request.is_none())?;
        let mut context = observed.clone();
        let mut mutations = vec![];
        if let Some(request) = &self.request {
            let fingerprint = fingerprint(request)?;
            let receipt_key = format!(
                "business/receipt/desktop-lesson/{}",
                request.command_id.get()
            );
            if let Some(stored) = d.get(&receipt_key)? {
                let receipt = codec::value(&stored.value)?;
                codec::object_fields(&receipt, &["fingerprint", "result", "digest"])?;
                let old: [u8; 32] = serde_json::from_value(receipt["fingerprint"].clone())
                    .map_err(|_| StorageError::Database)?;
                if old != fingerprint {
                    return Err(StorageError::CommandIdReused);
                }
                let result = display::encode(&receipt["result"], MAX_VALUE_BYTES)?;
                let digest: [u8; 32] = serde_json::from_value(receipt["digest"].clone())
                    .map_err(|_| StorageError::Database)?;
                if digest.as_slice() != Sha256::digest(&result).as_slice() {
                    return Err(StorageError::Database);
                }
                context = lesson_codec::read_snapshot(&receipt["result"])?;
                validate_replay(&context, request)?;
                let outcome = display::encode(
                    &value(&source(&identity, &schedule, context)),
                    MAX_VALUE_BYTES,
                )?;
                check_guard(&self.guard)?;
                let binding = self.binding;
                let guard = self.guard;
                let clock = self.clock;
                let observations = self.observations;
                return Ok(TransactionDecision {
                    outcome,
                    commit: None,
                    precommit: Some(Box::new(move || {
                        recheck(
                            &identity,
                            &schedule,
                            &observed,
                            &binding,
                            &guard,
                            &clock,
                            &observations,
                        )?;
                        Ok(())
                    })),
                });
            }
            context = observed
                .apply_local_desktop(
                    request.binding.expected_revision,
                    &request.binding.lesson,
                    request.action.clone(),
                    OverrideId::new(request.command_id.get())
                        .map_err(|_| StorageError::InvalidCommand)?,
                    &identity,
                    now,
                )
                .map_err(StorageError::Lesson)?;
            let result = lesson_codec::snapshot(&context);
            let bytes = display::encode(&result, MAX_VALUE_BYTES)?;
            mutations.push(Mutation::Put{key:receipt_key,value:display::encode(&json!({"fingerprint":fingerprint,"result":result,"digest":Sha256::digest(&bytes).to_vec()}),MAX_VALUE_BYTES)?});
        }
        if observation.is_some() || self.request.is_some() {
            mutations.push(lesson::mutation(&context)?);
        }
        let outcome = display::encode(
            &value(&source(&identity, &schedule, context)),
            MAX_VALUE_BYTES,
        )?;
        check_guard(&self.guard)?;
        let commit = if mutations.is_empty() {
            None
        } else {
            Some((
                mutations,
                (self.factory)(
                    &identity,
                    d.revision()?
                        .next()
                        .map_err(|_| StorageError::InvalidCommand)?,
                )?,
            ))
        };
        let binding = self.binding;
        let request = self.request;
        let guard = self.guard;
        let clock = self.clock;
        let observations = self.observations;
        Ok(TransactionDecision {
            outcome,
            commit,
            precommit: Some(Box::new(move || {
                let fresh = recheck(
                    &identity,
                    &schedule,
                    &observed,
                    &binding,
                    &guard,
                    &clock,
                    &observations,
                )?;
                if let Some(request) = request {
                    fresh
                        .apply_local_desktop(
                            request.binding.expected_revision,
                            &request.binding.lesson,
                            request.action,
                            OverrideId::new(request.command_id.get())
                                .map_err(|_| StorageError::InvalidCommand)?,
                            &identity,
                            clock.now(),
                        )
                        .map_err(StorageError::Lesson)?;
                } else {
                    current(&fresh, &binding, true)?;
                }
                check_guard(&guard)
            })),
        })
    }
}
fn fingerprint(request: &DesktopLessonRequest) -> Result<[u8; 32], StorageError> {
    let mut hash = Sha256::new();
    hash.update(b"future-classroom/local-desktop-lesson/v1\0");
    hash.update(display::encode(&json!({"origin":"local-desktop","expected":request.binding.expected_revision.get(),"catalog":request.binding.expected_identity_revision.get(),"schedule":request.binding.expected_schedule_revision.get(),"lesson":materials_codec::lesson(&request.binding.lesson),"action":lesson_codec::action(&request.action)}),MAX_VALUE_BYTES)?);
    Ok(hash.finalize().into())
}
fn validate_replay(
    context: &LessonContextSnapshot,
    request: &DesktopLessonRequest,
) -> Result<(), StorageError> {
    if context.revision
        != request
            .binding
            .expected_revision
            .next()
            .map_err(|_| StorageError::Database)?
        || context.identity_revision != request.binding.expected_identity_revision
        || !context
            .current
            .as_ref()
            .is_some_and(|c| c.key == request.binding.lesson)
    {
        return Err(StorageError::Database);
    }
    match &request.action {
        LessonAction::Replace(target)
            if !context.active_override.as_ref().is_some_and(|o| {
                o.id.get() == request.command_id.get()
                    && o.key == request.binding.lesson
                    && &o.target == target
                    && o.initiator == LessonInitiator::LocalDesktop
            }) =>
        {
            Err(StorageError::Database)
        }
        LessonAction::Restore if context.active_override.is_some() => Err(StorageError::Database),
        _ => Ok(()),
    }
}
fn recheck(
    identity: &IdentityState,
    schedule: &ScheduleDocument,
    observed: &LessonContextSnapshot,
    binding: &DesktopLessonBinding,
    guard: &Option<DesktopLessonGuard>,
    clock: &Arc<dyn Clock + Send + Sync>,
    observations: &lesson::Observations,
) -> Result<LessonContextSnapshot, StorageError> {
    check_guard(guard)?;
    let now = clock.now();
    metadata(identity, schedule, binding, now)?;
    let floor = lesson::witnessed(observations, now)?;
    let fresh = observed
        .observe_with_floor(identity, schedule, now, floor)
        .map_err(StorageError::Lesson)?;
    if fresh.current != observed.current
        || fresh.active_override != observed.active_override
        || fresh.blocked_until != observed.blocked_until
        || fresh.history != observed.history
        || fresh.base_trust != observed.base_trust
    {
        return Err(changed());
    }
    current(&fresh, binding, false)?;
    check_guard(guard)?;
    Ok(fresh)
}

#[cfg(test)]
mod tests;
