//! Authenticated independent public-layout and precise subject-profile slots.
use crate::Clock;
use fc_domain::{
    auth::ActorContext,
    desktop::DisplaySettings,
    ids::{CommandId, ProfileId, Revision},
    profile::{LayoutDocument, ProfileError, SubjectProfileDocument, SubjectProfileKey},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicLayoutState {
    pub layout: LayoutDocument,
    pub display: DisplaySettings,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicLayoutRequest {
    pub command_id: CommandId,
    pub expected_layout_revision: Revision,
    pub expected_display_revision: Revision,
    pub layout: LayoutDocument,
    pub display: DisplaySettings,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileRequest {
    pub command_id: CommandId,
    pub key: SubjectProfileKey,
    pub expected_revision: Revision,
    pub candidate: SubjectProfileDocument,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileRepositoryError {
    Domain(ProfileError),
    Display(fc_domain::desktop::DisplayError),
    Unavailable,
    Faulted,
    IdempotencyConflict,
    DocumentTooLarge,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileSummary {
    pub id: ProfileId,
    pub key: SubjectProfileKey,
    pub revision: Revision,
    pub enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfilePage {
    pub items: Vec<ProfileSummary>,
    pub total: u64,
    pub next_after: Option<ProfileId>,
}
pub const PROFILE_PAGE_SIZE: usize = 20;
pub trait ProfileRepository {
    fn list_subject_profiles(
        &self,
        after: Option<ProfileId>,
        actor: &ActorContext,
    ) -> Result<ProfilePage, ProfileRepositoryError>;
    fn public_layout(
        &self,
        actor: &ActorContext,
    ) -> Result<PublicLayoutState, ProfileRepositoryError>;
    fn execute_public_layout(
        &mut self,
        request: PublicLayoutRequest,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<PublicLayoutState, ProfileRepositoryError>;
    fn subject_profile(
        &self,
        key: SubjectProfileKey,
        actor: &ActorContext,
    ) -> Result<Option<SubjectProfileDocument>, ProfileRepositoryError>;
    fn execute_subject_profile(
        &mut self,
        request: ProfileRequest,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<SubjectProfileDocument, ProfileRepositoryError>;
}
