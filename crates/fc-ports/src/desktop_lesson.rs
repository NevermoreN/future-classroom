//! Actorless trusted native desktop lesson actions. No Web/settings capability.
use crate::LessonRepositoryError;
use fc_domain::{
    identity::SchoolSubject,
    ids::{CommandId, PrincipalId, Revision, SubjectId},
    lesson_context::{LessonAction, LessonContextSnapshot},
    schedule::LessonInstanceKey,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopLessonBinding {
    pub expected_revision: Revision,
    pub expected_identity_revision: Revision,
    pub expected_schedule_revision: Revision,
    pub lesson: LessonInstanceKey,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopLessonRequest {
    pub command_id: CommandId,
    pub binding: DesktopLessonBinding,
    pub action: LessonAction,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopLessonTeacher {
    pub id: PrincipalId,
    pub display_name: String,
    pub subject_ids: Vec<SubjectId>,
}
/// Only the current bound classroom's enabled subject/teacher choices. No login
/// names, roles, credentials, auth epochs or mutable account objects are exposed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopLessonSource {
    pub context: LessonContextSnapshot,
    pub catalog_revision: Revision,
    pub schedule_revision: Revision,
    pub subjects: Vec<SchoolSubject>,
    pub teachers: Vec<DesktopLessonTeacher>,
}
pub trait DesktopLessonRepository {
    fn desktop_lesson(
        &self,
        binding: DesktopLessonBinding,
    ) -> Result<DesktopLessonSource, LessonRepositoryError>;
    fn execute_desktop_lesson(
        &mut self,
        request: DesktopLessonRequest,
    ) -> Result<DesktopLessonSource, LessonRepositoryError>;
}
