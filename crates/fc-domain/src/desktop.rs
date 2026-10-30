//! Owned desktop facts and global display policy. No platform state or secrets.
use crate::{
    auth::ActorContext,
    content::{ContentKey, ContentRecord},
    identity::{IdentityError, IdentityState},
    ids::Revision,
    schedule_document::ScheduleDocument,
    time::{LocalDate, LocalDateTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    Digital24,
    Digital12,
    Analog,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextTone {
    Light,
    Dark,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    Clock,
    Date,
    Schedule,
    Homework,
    Materials,
    Reminder,
    DateCountdown,
    Recess,
}
impl ComponentKind {
    pub const ALL: [Self; 8] = [
        Self::Clock,
        Self::Date,
        Self::Schedule,
        Self::Homework,
        Self::Materials,
        Self::Reminder,
        Self::DateCountdown,
        Self::Recess,
    ];
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentSettings {
    pub kind: ComponentKind,
    pub visible: bool,
    pub opacity_percent: u8,
    pub font_dip: u16,
    pub min_width: u16,
    pub max_width: u16,
    pub min_height: u16,
    pub max_height: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountdownExpired {
    Hide,
    ShowCompleted,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateCountdownSettings {
    pub target: Option<LocalDate>,
    pub label: String,
    pub expired: CountdownExpired,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplaySettings {
    pub revision: Revision,
    pub clock_mode: ClockMode,
    pub clock_seconds: bool,
    pub recess_enabled: bool,
    pub subject_fill: bool,
    pub course_fill_percent: u8,
    pub homework_fill_percent: u8,
    pub text_tone: TextTone,
    pub glass_surface_percent: u8,
    pub edge_reflection_percent: u8,
    pub components: Vec<ComponentSettings>,
    pub countdown: DateCountdownSettings,
}
impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            revision: Revision::default(),
            clock_mode: ClockMode::Digital24,
            clock_seconds: false,
            recess_enabled: true,
            subject_fill: true,
            course_fill_percent: 20,
            homework_fill_percent: 3,
            text_tone: TextTone::Light,
            glass_surface_percent: 3,
            edge_reflection_percent: 50,
            components: ComponentKind::ALL
                .into_iter()
                .map(|kind| ComponentSettings {
                    kind,
                    visible: true,
                    opacity_percent: 100,
                    font_dip: if kind == ComponentKind::Clock { 40 } else { 22 },
                    min_width: 160,
                    max_width: 800,
                    min_height: 64,
                    max_height: 1200,
                })
                .collect(),
            countdown: DateCountdownSettings {
                target: None,
                label: "目标日期".into(),
                expired: CountdownExpired::Hide,
            },
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplayError {
    Identity(IdentityError),
    InvalidSettings,
    InvalidFacts,
    Capacity,
    RevisionConflict { current: Revision },
    VersionOverflow,
}
impl DisplaySettings {
    pub fn component(&self, kind: ComponentKind) -> Option<&ComponentSettings> {
        self.components.iter().find(|c| c.kind == kind)
    }
    pub fn validate(&self) -> Result<(), DisplayError> {
        if self.glass_surface_percent != 3
            || self.edge_reflection_percent != 50
            || self.components.len() != 8
            || [
                self.course_fill_percent,
                self.homework_fill_percent,
                self.glass_surface_percent,
                self.edge_reflection_percent,
            ]
            .into_iter()
            .any(|v| v > 100)
        {
            return Err(DisplayError::InvalidSettings);
        }
        if self.countdown.label.len() > 256 || self.countdown.label.chars().count() > 64 {
            return Err(DisplayError::Capacity);
        }
        if self.countdown.label.trim().is_empty()
            || self.countdown.label.chars().any(char::is_control)
        {
            return Err(DisplayError::InvalidSettings);
        }
        for kind in ComponentKind::ALL {
            if self.components.iter().filter(|c| c.kind == kind).count() != 1 {
                return Err(DisplayError::InvalidSettings);
            }
        }
        for c in &self.components {
            if c.opacity_percent > 100
                || !(10..=72).contains(&c.font_dip)
                || !(120..=4096).contains(&c.min_width)
                || c.min_width > c.max_width
                || c.max_width > 4096
                || !(32..=2160).contains(&c.min_height)
                || c.min_height > c.max_height
                || c.max_height > 2160
            {
                return Err(DisplayError::InvalidSettings);
            }
        }
        Ok(())
    }
    pub fn transition(
        &self,
        mut candidate: Self,
        expected_revision: Revision,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<Self, DisplayError> {
        identity.validate().map_err(DisplayError::Identity)?;
        identity
            .authorize_management(actor, now)
            .map_err(DisplayError::Identity)?;
        self.validate()?;
        if expected_revision != self.revision {
            return Err(DisplayError::RevisionConflict {
                current: self.revision,
            });
        }
        candidate.validate()?;
        candidate.revision = self
            .revision
            .next()
            .map_err(|_| DisplayError::VersionOverflow)?;
        Ok(candidate)
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.components
            .capacity()
            .checked_mul(std::mem::size_of::<ComponentSettings>())?
            .checked_add(self.countdown.label.capacity())
    }
}
/// Repository must read every field under one writer transaction. Paging bounds full
/// homework texts plus the separate reminder slot to 64 full texts. No actor credential,
/// password record, lease, source filename or platform handle belongs in this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopFacts {
    pub global_revision: Revision,
    pub captured_at: LocalDateTime,
    pub date_watermark: LocalDate,
    pub clock_rollback: bool,
    pub identity: IdentityState,
    pub schedule: ScheduleDocument,
    pub lesson_context: crate::lesson_context::LessonContextSnapshot,
    pub display: DisplaySettings,
    pub homework_offset: u64,
    pub homework_eligible_total: u64,
    pub homework: Vec<ContentRecord>,
    pub reminder: Option<ContentRecord>,
    pub materials_source: Option<crate::materials::MaterialSource>,
    pub materials_page: Option<crate::materials::MaterialPage>,
}
impl DesktopFacts {
    pub fn validate(&self) -> Result<(), DisplayError> {
        self.identity.validate().map_err(DisplayError::Identity)?;
        self.schedule
            .validate(&self.identity)
            .map_err(|_| DisplayError::InvalidFacts)?;
        self.display.validate()?;
        if self.identity.installation.is_none()
            || self.identity.revision.get() == 0
            || self.global_revision.get() == 0
            || self.identity.revision > self.global_revision
            || self.schedule.revision > self.global_revision
            || self.display.revision > self.global_revision
            || self.clock_rollback != (self.captured_at.date < self.date_watermark)
            || self.date_watermark < self.captured_at.date
        {
            return Err(DisplayError::InvalidFacts);
        }
        if self.homework.len() + usize::from(self.reminder.is_some()) > 64
            || self.homework_eligible_total > crate::identity::MAX_ASSIGNMENTS as u64
        {
            return Err(DisplayError::Capacity);
        }
        let end = self
            .homework_offset
            .checked_add(self.homework.len() as u64)
            .ok_or(DisplayError::Capacity)?;
        if end > self.homework_eligible_total || self.homework_offset > self.homework_eligible_total
        {
            return Err(DisplayError::InvalidFacts);
        }
        if self.clock_rollback
            && (!self.homework.is_empty()
                || self.reminder.is_some()
                || self.homework_eligible_total != 0)
        {
            return Err(DisplayError::InvalidFacts);
        }
        let class = self
            .identity
            .installation
            .as_ref()
            .and_then(|i| i.current_class);
        if class.is_none()
            && (!self.homework.is_empty()
                || self.reminder.is_some()
                || self.homework_eligible_total != 0)
        {
            return Err(DisplayError::InvalidFacts);
        }
        self.lesson_context
            .validate()
            .map_err(|_| DisplayError::InvalidFacts)?;
        if self.lesson_context.revision > self.global_revision {
            return Err(DisplayError::InvalidFacts);
        }
        if (class.is_some() || self.lesson_context.observed_at.is_some())
            && (self.lesson_context.class_id != class
                || self.lesson_context.identity_revision != self.identity.revision
                || self.lesson_context.observed_at.is_none())
        {
            return Err(DisplayError::InvalidFacts);
        }
        if !self.clock_rollback && self.lesson_context.blocked_until.is_none() {
            let observed = self
                .lesson_context
                .observe(&self.identity, &self.schedule, self.captured_at)
                .map_err(|_| DisplayError::InvalidFacts)?;
            // Facts cannot mix a new timetable with a stale course or override.
            // The storage adapter must observe the complete context in its transaction.
            if observed.current != self.lesson_context.current
                || observed.active_override != self.lesson_context.active_override
                || observed.history != self.lesson_context.history
                || observed.blocked_until != self.lesson_context.blocked_until
            {
                return Err(DisplayError::InvalidFacts);
            }
        }
        let expected_source = if self.clock_rollback {
            None
        } else if let Some(class) = class {
            self.lesson_context
                .material_source(&self.identity, &self.schedule, class, self.captured_at)
                .map_err(|_| DisplayError::InvalidFacts)?
        } else {
            None
        };
        if self.materials_source != expected_source {
            return Err(DisplayError::InvalidFacts);
        }
        if let Some(page) = &self.materials_page {
            if page.source != self.materials_source
                || self.materials_source.is_none()
                || page.class_id != class
                || page.folder.revision > self.global_revision
                || page.items.iter().any(|r| r.revision > self.global_revision)
            {
                return Err(DisplayError::InvalidFacts);
            }
            page.validate(&self.identity, crate::materials::DESKTOP_MATERIAL_PAGE)
                .map_err(|_| DisplayError::InvalidFacts)?;
        }
        for (i, record) in self.homework.iter().enumerate() {
            self.validate_record(record)?;
            let ContentKey::Homework(k) = &record.key else {
                return Err(DisplayError::InvalidFacts);
            };
            if Some(k.class_id) != class
                || k.actual_date != self.captured_at.date
                || !self
                    .identity
                    .is_teacher(k.owner_teacher_id, k.class_id, k.subject_id)
                || self.homework[..i].iter().any(|r| r.key == record.key)
            {
                return Err(DisplayError::InvalidFacts);
            }
        }
        if let Some(record) = &self.reminder {
            self.validate_record(record)?;
            let ContentKey::SharedReminder(k) = &record.key else {
                return Err(DisplayError::InvalidFacts);
            };
            if Some(k.class_id) != class || k.actual_date != self.captured_at.date {
                return Err(DisplayError::InvalidFacts);
            }
        }
        Ok(())
    }
    fn validate_record(&self, r: &ContentRecord) -> Result<(), DisplayError> {
        if r.text.len() > 16 * 1024 {
            return Err(DisplayError::Capacity);
        }
        if r.text.trim().is_empty()
            || r.revision.get() == 0
            || r.revision > self.global_revision
            || r.updated_at.date != self.captured_at.date
            || !self
                .identity
                .accounts
                .iter()
                .any(|a| a.principal.id == r.last_actor_id)
        {
            return Err(DisplayError::InvalidFacts);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{auth::*, identity::IdentityCommand, ids::*};
    fn fixture() -> (IdentityState, ActorContext, LocalDateTime) {
        let date = LocalDate::new(2026, 10, 30).unwrap();
        let now = LocalDateTime::new(date, 36000).unwrap();
        let identity = IdentityState::default()
            .transition(
                &IdentityCommand::FirstAdministrator {
                    school_id: SchoolId::new(1).unwrap(),
                    school_name: "学校".into(),
                    id: PrincipalId::new(1).unwrap(),
                    login_name: "admin".into(),
                    display_name: "管理员".into(),
                },
                None,
                now,
            )
            .unwrap()
            .next;
        let actor = ActorContext {
            principal_id: PrincipalId::new(1).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            auth_epoch: AuthEpoch::new(1),
            expires_at: LocalDateTime::new(date, 86399).unwrap(),
            origin: TrustedOrigin::Local,
        };
        (identity, actor, now)
    }
    #[test]
    fn exact_components_bounded_percent_font_and_fixed_glass() {
        let settings = DisplaySettings::default();
        settings.validate().unwrap();
        assert_eq!(settings.course_fill_percent, 20);
        assert_eq!(settings.homework_fill_percent, 3);
        for mutation in 0..6 {
            let mut candidate = settings.clone();
            match mutation {
                0 => candidate.components[0].opacity_percent = 101,
                1 => candidate.components[0].font_dip = 9,
                2 => candidate.components[1].kind = ComponentKind::Clock,
                3 => candidate.components[0].min_width = 4097,
                4 => candidate.glass_surface_percent = 100,
                _ => candidate.edge_reflection_percent = 0,
            };
            assert_eq!(candidate.validate(), Err(DisplayError::InvalidSettings))
        }
    }
    #[test]
    fn management_cas_ignores_forged_revision_and_denies_web() {
        let (identity, mut actor, now) = fixture();
        let original = DisplaySettings::default();
        let mut candidate = original.clone();
        candidate.revision = Revision::new(u64::MAX);
        let saved = original
            .transition(
                candidate.clone(),
                Revision::default(),
                &identity,
                &actor,
                now,
            )
            .unwrap();
        assert_eq!(saved.revision, Revision::new(1));
        assert!(matches!(
            saved.transition(
                candidate.clone(),
                Revision::default(),
                &identity,
                &actor,
                now
            ),
            Err(DisplayError::RevisionConflict { .. })
        ));
        actor.origin = TrustedOrigin::Web;
        assert!(matches!(
            original.transition(candidate, Revision::default(), &identity, &actor, now),
            Err(DisplayError::Identity(_))
        ))
    }
    #[test]
    fn date_watermark_and_paging_are_consistent_even_without_binding() {
        let (identity, _, now) = fixture();
        let mut facts = DesktopFacts {
            global_revision: Revision::new(10),
            captured_at: now,
            date_watermark: now.date,
            clock_rollback: false,
            identity,
            schedule: ScheduleDocument::default(),
            lesson_context: Default::default(),
            display: DisplaySettings::default(),
            homework_offset: 0,
            homework_eligible_total: 0,
            homework: vec![],
            reminder: None,
            materials_source: None,
            materials_page: None,
        };
        facts.validate().unwrap();
        facts.date_watermark = LocalDate::new(2026, 10, 31).unwrap();
        assert_eq!(facts.validate(), Err(DisplayError::InvalidFacts));
        facts.clock_rollback = true;
        facts.validate().unwrap();
        facts.homework_eligible_total = 1;
        assert_eq!(facts.validate(), Err(DisplayError::InvalidFacts));
    }
}
