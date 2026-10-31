//! Worker-only joint palette/display management and single-subject Web preview.
//! Preview is a disclosure, never a layout or palette-management capability.
use super::*;
use fc_domain::{
    desktop::ComponentKind,
    subject_visual::{PaletteRevision, SubjectColor},
};
use fc_ports::subject_visual::{
    SubjectVisualRepository, SubjectVisualRepositoryError, SubjectVisualRequest, SubjectVisualState,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSubjectVisualPreview {
    pub key: HomeworkKey,
    pub subject_name: String,
    pub subject_revision: Revision,
    pub display_revision: Revision,
    pub effective_rgb: Rgb,
    pub filled: bool,
    pub homework_fill_percent: u8,
    pub course_fill_percent: u8,
    pub homework_component_opacity_percent: u8,
    pub course_component_opacity_percent: u8,
    pub homework_visible: bool,
    pub course_visible: bool,
}
fn permission(error: PermissionError) -> BackendError {
    BackendError::Identity(IdentityRepositoryError::Domain(IdentityError::Permission(
        error,
    )))
}
impl Backend {
    fn visual_manager(&self, lease: SessionLease) -> Result<(ActorContext, u64)> {
        if lease.origin() != TrustedOrigin::Local {
            return Err(permission(PermissionError::LocalOnly));
        }
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        state
            .authority(actor.principal_id)
            .map_err(|e| BackendError::Identity(IdentityRepositoryError::Domain(e)))?
            .authorize_global_local(&actor, self.clock.now())
            .map_err(permission)?;
        self.control.check(lease, generation)?;
        Ok((actor, generation))
    }
    pub fn read_subject_visuals(&self, lease: SessionLease) -> Result<SubjectVisualState> {
        let (actor, generation) = self.visual_manager(lease)?;
        let result = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .subject_visuals(&actor)?;
        self.visual_manager(lease)?;
        self.control.check(lease, generation)?;
        Ok(result)
    }
    pub fn save_subject_visuals(
        &mut self,
        lease: SessionLease,
        expected_palette_revision: PaletteRevision,
        expected_display_revision: Revision,
        colors: Vec<SubjectColor>,
        display: DisplaySettings,
    ) -> Result<SubjectVisualState> {
        let (actor, generation) = self.visual_manager(lease)?;
        let request = SubjectVisualRequest {
            command_id: self.command_id()?,
            expected_palette_revision,
            expected_display_revision,
            colors,
            display,
        };
        let result = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .execute_subject_visuals(request, &actor, self.clock.as_ref())?;
        // The writer already published updated identity. Synchronize the real
        // ledger with that state; colors do not mint/revoke authorization.
        let next = self.state()?;
        self.apply(&IdentityTransition {
            next,
            events: vec![IdentityEvent::CatalogChanged],
        })?;
        self.visual_manager(lease)?;
        self.control.check(lease, generation)?;
        Ok(result)
    }
    fn visual_web_actor(
        &self,
        lease: SessionLease,
        key: &HomeworkKey,
    ) -> Result<(ActorContext, u64)> {
        if lease.origin() != TrustedOrigin::Web {
            return Err(permission(PermissionError::WebOnly));
        }
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let now = self.clock.now();
        if key.actual_date != now.date {
            return Err(BackendError::Content(RepositoryError::Domain(
                ContentError::WrongDate,
            )));
        }
        state
            .authority(actor.principal_id)
            .map_err(|e| BackendError::Identity(IdentityRepositoryError::Domain(e)))?
            .authorize_owned_content(
                &actor,
                now,
                key.class_id,
                key.subject_id,
                key.owner_teacher_id,
            )
            .map_err(permission)?;
        if !state.is_teacher(key.owner_teacher_id, key.class_id, key.subject_id) {
            return Err(permission(PermissionError::OutsideTeachingScope));
        }
        self.control.check(lease, generation)?;
        Ok((actor, generation))
    }
    /// Same writer transaction reads live identity/color and display settings.
    /// Only the exact authorized day's subject is returned, never the palette.
    pub fn web_subject_visual_preview(
        &self,
        lease: SessionLease,
        key: HomeworkKey,
    ) -> Result<WebSubjectVisualPreview> {
        let (actor, generation) = self.visual_web_actor(lease, &key)?;
        let source = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .subject_visual_preview(key.clone(), &actor)?;
        let unavailable = || BackendError::SubjectVisual(SubjectVisualRepositoryError::Unavailable);
        if source.subject.id != key.subject_id
            || !source.subject.enabled
            || source.display.validate().is_err()
        {
            return Err(unavailable());
        }
        let homework = source
            .display
            .components
            .iter()
            .find(|c| c.kind == ComponentKind::Homework)
            .ok_or_else(unavailable)?;
        let course = source
            .display
            .components
            .iter()
            .find(|c| c.kind == ComponentKind::Schedule)
            .ok_or_else(unavailable)?;
        let result = WebSubjectVisualPreview {
            key: key.clone(),
            subject_name: source.subject.name,
            subject_revision: source.subject.revision,
            display_revision: source.display.revision,
            effective_rgb: fc_presentation::scene::soft_subject_color(source.subject.color),
            filled: source.display.subject_fill,
            homework_fill_percent: source.display.homework_fill_percent,
            course_fill_percent: source.display.course_fill_percent,
            homework_component_opacity_percent: homework.opacity_percent,
            course_component_opacity_percent: course.opacity_percent,
            homework_visible: homework.visible,
            course_visible: course.visible,
        };
        self.visual_web_actor(lease, &key)?;
        self.control.check(lease, generation)?;
        self.check_open()?;
        Ok(result)
    }
}
