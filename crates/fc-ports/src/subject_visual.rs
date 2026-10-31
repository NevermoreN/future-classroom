//! Joint school palette/display transactions and scoped, same-version previews.
use crate::Clock;
use fc_domain::{
    auth::ActorContext,
    content::{ContentError, HomeworkKey},
    desktop::{DisplayError, DisplaySettings},
    identity::SchoolSubject,
    ids::{CommandId, Revision},
    subject_visual::{PaletteRevision, SubjectColor, SubjectVisualError, SubjectVisualPalette},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectVisualState {
    pub palette: SubjectVisualPalette,
    pub display: DisplaySettings,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectVisualRequest {
    pub command_id: CommandId,
    pub expected_palette_revision: PaletteRevision,
    pub expected_display_revision: Revision,
    pub colors: Vec<SubjectColor>,
    pub display: DisplaySettings,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectVisualPreviewSource {
    pub subject: SchoolSubject,
    pub display: DisplaySettings,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubjectVisualRepositoryError {
    Domain(SubjectVisualError),
    Display(DisplayError),
    Content(ContentError),
    IdempotencyConflict,
    DocumentTooLarge,
    Unavailable,
    Faulted,
}
pub trait SubjectVisualRepository {
    /// Exact local global-administrator session only, including disabled subjects.
    fn subject_visuals(
        &self,
        actor: &ActorContext,
    ) -> Result<SubjectVisualState, SubjectVisualRepositoryError>;
    /// One short writer transaction, comparing both independent revisions.
    fn execute_subject_visuals(
        &mut self,
        request: SubjectVisualRequest,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<SubjectVisualState, SubjectVisualRepositoryError>;
    /// Exact Web session and current-day owned teaching scope only. No full
    /// catalog disclosure; both fields come from the same writer transaction.
    fn subject_visual_preview(
        &self,
        key: HomeworkKey,
        actor: &ActorContext,
    ) -> Result<SubjectVisualPreviewSource, SubjectVisualRepositoryError>;
}
