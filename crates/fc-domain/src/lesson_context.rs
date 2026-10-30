//! Durable actual lesson identity and bounded coverage evidence. No wall-clock
//! rollback, later default-teacher edit or terminated override can invent history.
use crate::{
    auth::ActorContext,
    identity::{IdentityError, IdentityState},
    ids::*,
    schedule::*,
    schedule_document::ScheduleDocument,
    time::*,
};

pub const MAX_LESSON_INTERVALS: usize = 64;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LessonContextError {
    Identity(IdentityError),
    LessonContextChanged,
    RevisionConflict { current: Revision },
    InvalidTarget,
    Capacity,
    VersionOverflow,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LessonAction {
    Replace(CourseCell),
    Restore,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveLessonContext {
    pub key: LessonInstanceKey,
    pub base: CourseCell,
    pub effective: CourseCell,
    pub teacher_id: Option<PrincipalId>,
    pub effective_from: LocalDateTime,
    pub effective_until: LocalDateTime,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveLessonInterval {
    pub context: EffectiveLessonContext,
    pub ended_at: LocalDateTime,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LessonOverrideContext {
    pub id: OverrideId,
    pub key: LessonInstanceKey,
    pub target: CourseCell,
    pub actor_id: PrincipalId,
    pub created_at: LocalDateTime,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LessonContextSnapshot {
    pub revision: Revision,
    pub identity_revision: Revision,
    pub observed_at: Option<LocalDateTime>,
    pub class_id: Option<ClassId>,
    pub blocked_until: Option<LocalDateTime>,
    pub current: Option<EffectiveLessonContext>,
    pub active_override: Option<LessonOverrideContext>,
    pub history: Vec<EffectiveLessonInterval>,
}
impl LessonContextSnapshot {
    pub fn validate(&self) -> Result<(), LessonContextError> {
        let invalid = || LessonContextError::InvalidTarget;
        if self.history.len() > MAX_LESSON_INTERVALS
            || (self.observed_at.is_some() && self.identity_revision.get() == 0)
            || (self.observed_at.is_none()
                && (self.current.is_some()
                    || !self.history.is_empty()
                    || self.active_override.is_some()
                    || self.blocked_until.is_some()))
        {
            return Err(invalid());
        }
        if (self.current.is_some()
            || !self.history.is_empty()
            || self.active_override.is_some()
            || self.blocked_until.is_some())
            && self.revision.get() == 0
        {
            return Err(invalid());
        }
        if self.blocked_until.is_some()
            && (self.current.is_some()
                || self.active_override.is_some()
                || !self.history.is_empty()
                || self.blocked_until != self.observed_at)
        {
            return Err(invalid());
        }
        for c in self
            .current
            .iter()
            .chain(self.history.iter().map(|h| &h.context))
        {
            if c.key.actual_date != c.effective_from.date
                || c.key.actual_date != c.effective_until.date
                || c.effective_from >= c.effective_until
                || c.effective_from.second_of_day() < u32::from(c.key.range.begin.get()) * 60
                || c.effective_until.second_of_day() != u32::from(c.key.range.end.get()) * 60
                || self.class_id != Some(c.key.class_id)
                || !self
                    .observed_at
                    .is_some_and(|at| at.date == c.key.actual_date && at >= c.effective_from)
            {
                return Err(invalid());
            }
            match c.effective.teacher {
                TeacherBinding::Explicit(t) if c.teacher_id != Some(t) => return Err(invalid()),
                TeacherBinding::Unassigned if c.teacher_id.is_some() => return Err(invalid()),
                _ => {}
            }
        }
        if self
            .current
            .as_ref()
            .is_some_and(|c| self.observed_at.is_some_and(|at| at >= c.effective_until))
        {
            return Err(invalid());
        }
        let mut previous = None;
        for h in &self.history {
            if h.ended_at <= h.context.effective_from
                || h.ended_at > h.context.effective_until
                || self.observed_at.is_none_or(|at| h.ended_at > at)
                || previous.is_some_and(|at| h.context.effective_from < at)
            {
                return Err(invalid());
            }
            previous = Some(h.ended_at);
        }
        if self
            .current
            .as_ref()
            .is_some_and(|c| previous.is_some_and(|at| c.effective_from < at))
        {
            return Err(invalid());
        }
        if self
            .current
            .as_ref()
            .is_some_and(|c| self.active_override.is_none() && c.effective != c.base)
        {
            return Err(invalid());
        }
        if let Some(o) = &self.active_override {
            if o.target.teacher == TeacherBinding::Default
                || !self.current.as_ref().is_some_and(|c| {
                    c.key == o.key
                        && c.effective == o.target
                        && o.created_at == c.effective_from
                        && o.created_at.date == o.key.actual_date
                })
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn material_source(
        &self,
        identity: &IdentityState,
        schedule: &ScheduleDocument,
        class: ClassId,
        now: LocalDateTime,
    ) -> Result<Option<crate::materials::MaterialSource>, crate::materials::MaterialError> {
        use crate::materials::*;
        self.validate().map_err(|_| MaterialError::InvalidContext)?;
        if self.blocked_until.is_some() || self.class_id != Some(class) {
            return Ok(None);
        }
        let selected = if let Some(c) = &self.current {
            let day = schedule
                .resolve_day(class, now.date)
                .map_err(|_| MaterialError::InvalidContext)?;
            if !matches!(resolve_lesson(&day,now,schedule.automatic_gaps),LessonState::Course{ref key,..} if key==&c.key)
            {
                return Ok(None);
            }
            Some((MaterialSourceKind::CurrentLesson, c))
        } else {
            let base = resolve_material_lesson(schedule, class, now).ok().flatten();
            base.filter(|b| b.0 == MaterialSourceKind::RetainedBreak)
                .and_then(|b| {
                    self.history
                        .iter()
                        .rev()
                        .find(|h| h.context.key == b.1 && h.ended_at == h.context.effective_until)
                        .map(|h| (MaterialSourceKind::RetainedBreak, &h.context))
                })
        };
        let Some((kind, c)) = selected else {
            return Ok(None);
        };
        let Some(teacher_id) = c.teacher_id else {
            return Ok(None);
        };
        if !identity.is_teacher(teacher_id, class, c.effective.subject_id) {
            return Ok(None);
        }
        Ok(Some(MaterialSource {
            kind,
            lesson: c.key.clone(),
            subject_id: c.effective.subject_id,
            teacher_id,
            evidence: MaterialSourceEvidence::PersistedLesson {
                revision: self.revision,
            },
        }))
    }
    fn bump(&mut self) -> Result<(), LessonContextError> {
        self.revision = self
            .revision
            .next()
            .map_err(|_| LessonContextError::VersionOverflow)?;
        Ok(())
    }
    fn finish(&mut self, now: LocalDateTime) -> Result<(), LessonContextError> {
        if let Some(c) = self.current.take() {
            let end = now.min(c.effective_until);
            if end > c.effective_from {
                if self.history.len() == MAX_LESSON_INTERVALS {
                    self.history.remove(0);
                }
                self.history.push(EffectiveLessonInterval {
                    context: c,
                    ended_at: end,
                });
            }
        }
        Ok(())
    }
    pub fn observe(
        &self,
        identity: &IdentityState,
        schedule: &ScheduleDocument,
        now: LocalDateTime,
    ) -> Result<Self, LessonContextError> {
        self.observe_with_floor(identity, schedule, now, None)
    }
    pub fn observe_with_floor(
        &self,
        identity: &IdentityState,
        schedule: &ScheduleDocument,
        now: LocalDateTime,
        floor: Option<LocalDateTime>,
    ) -> Result<Self, LessonContextError> {
        let mut next = self.clone();
        let witnessed = self.observed_at.into_iter().chain(floor).max();
        let class = identity.installation.as_ref().and_then(|i| i.current_class);
        let rollback = witnessed.is_some_and(|t| now < t);
        let day_changed = self.observed_at.is_some_and(|t| t.date != now.date);
        if class != self.class_id || day_changed {
            next.current = None;
            next.active_override = None;
            next.history.clear();
            next.class_id = class;
        }
        if rollback {
            next.blocked_until = witnessed;
            next.current = None;
            next.active_override = None;
            next.history.clear();
        }
        if next.blocked_until.is_some_and(|t| now >= t) {
            next.blocked_until = None;
        }
        let blocked = next.blocked_until.is_some();
        let lesson = if let Some(class) = class {
            let day = schedule
                .resolve_day(class, now.date)
                .map_err(|_| LessonContextError::LessonContextChanged)?;
            resolve_lesson(&day, now, schedule.automatic_gaps)
        } else {
            LessonState::Idle
        };
        let current = match lesson {
            LessonState::Course { key, cell } if !blocked => Some((key, cell)),
            _ => None,
        };
        let override_valid = next.active_override.as_ref().is_some_and(|o| {
            current.as_ref().is_some_and(|(k, _)| k == &o.key)
                && target_valid(identity, o.key.class_id, &o.target)
        });
        if !override_valid {
            next.active_override = None;
        }
        if let Some((key, base)) = current {
            let effective = next
                .active_override
                .as_ref()
                .map_or_else(|| base.clone(), |o| o.target.clone());
            let teacher = teacher(identity, key.class_id, &effective);
            let same = next.current.as_ref().is_some_and(|c| {
                c.key == key
                    && c.base == base
                    && c.effective == effective
                    && c.teacher_id == teacher
            });
            if !same {
                next.finish(now)?;
                next.current = Some(EffectiveLessonContext {
                    effective_from: now,
                    effective_until: LocalDateTime::new(
                        key.actual_date,
                        u32::from(key.range.end.get()) * 60,
                    )
                    .map_err(|_| LessonContextError::InvalidTarget)?,
                    key,
                    base,
                    effective,
                    teacher_id: teacher,
                });
            }
        } else {
            next.finish(now)?;
        }
        next.identity_revision = identity.revision;

        if next.current != self.current
            || next.active_override != self.active_override
            || next.history != self.history
            || next.class_id != self.class_id
            || next.blocked_until != self.blocked_until
        {
            next.bump()?;
            next.observed_at = Some(witnessed.map_or(now, |t| t.max(now)));
        } else if next.identity_revision != self.identity_revision || self.observed_at.is_none() {
            next.observed_at = Some(witnessed.map_or(now, |t| t.max(now)));
        }
        next.validate()?;
        Ok(next)
    }
    // The commit transition deliberately takes the exact actor, authority and clock.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &self,
        expected: Revision,
        key: &LessonInstanceKey,
        action: LessonAction,
        id: OverrideId,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<Self, LessonContextError> {
        if self.blocked_until.is_some()
            || self.observed_at.is_some_and(|at| now < at)
            || self
                .current
                .as_ref()
                .is_some_and(|c| now < c.effective_from)
        {
            return Err(LessonContextError::LessonContextChanged);
        }
        identity
            .authorize_management(actor, now)
            .map_err(LessonContextError::Identity)?;
        if expected != self.revision {
            return Err(LessonContextError::RevisionConflict {
                current: self.revision,
            });
        }
        let current = self
            .current
            .as_ref()
            .filter(|c| {
                &c.key == key
                    && key.actual_date == now.date
                    && key.range.contains_second(now.second_of_day())
            })
            .ok_or(LessonContextError::LessonContextChanged)?;
        let mut next = self.clone();
        let target = match action {
            LessonAction::Replace(ref target) => {
                if !target_valid(identity, key.class_id, target) {
                    return Err(LessonContextError::InvalidTarget);
                }
                target.clone()
            }
            LessonAction::Restore => {
                if self.active_override.is_none() {
                    return Err(LessonContextError::LessonContextChanged);
                }
                current.base.clone()
            }
        };
        next.finish(now)?;
        next.current = Some(EffectiveLessonContext {
            effective: target.clone(),
            teacher_id: teacher(identity, key.class_id, &target),
            effective_from: now,
            ..current.clone()
        });
        next.active_override = match action {
            LessonAction::Replace(_) => Some(LessonOverrideContext {
                id,
                key: key.clone(),
                target,
                actor_id: actor.principal_id,
                created_at: now,
            }),
            LessonAction::Restore => None,
        };
        next.observed_at = Some(now);
        next.bump()?;
        next.validate()?;
        Ok(next)
    }
}
pub fn target_valid(identity: &IdentityState, class: ClassId, cell: &CourseCell) -> bool {
    if !identity
        .subjects
        .iter()
        .any(|s| s.id == cell.subject_id && s.enabled)
    {
        return false;
    }
    match cell.teacher {
        TeacherBinding::Explicit(t) => identity.is_teacher(t, class, cell.subject_id),
        TeacherBinding::Unassigned => !identity.assignments.iter().any(|a| {
            a.enabled
                && a.class_id == class
                && a.subject_id == cell.subject_id
                && identity.is_teacher(a.teacher_id, class, cell.subject_id)
        }),
        TeacherBinding::Default => false,
    }
}
fn teacher(identity: &IdentityState, class: ClassId, cell: &CourseCell) -> Option<PrincipalId> {
    match cell.teacher {
        TeacherBinding::Unassigned => None,
        TeacherBinding::Default => identity.resolve_teacher(class, cell.subject_id, None).ok(),
        TeacherBinding::Explicit(t) => identity
            .resolve_teacher(class, cell.subject_id, Some(t))
            .ok(),
    }
}

#[cfg(test)]
mod tests;
