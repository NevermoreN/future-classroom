//! Current-installation HTTPS settings only; this repository grants no listener
//! permission and its configuration document is omitted from teaching backups.
use fc_domain::{
    auth::ActorContext,
    https_settings::*,
    ids::{CommandId, Revision},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsSettingsRequest {
    pub command_id: CommandId,
    pub expected_revision: Revision,
    pub expected_identity_revision: Revision,
    pub configuration: Option<HttpsConfiguration>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsSettingsState {
    pub identity_revision: Revision,
    pub settings: HttpsSettings,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpsSettingsRepositoryError {
    Domain(HttpsSettingsError),
    IdempotencyConflict,
    DocumentTooLarge,
    Unavailable,
    Faulted,
}
pub trait HttpsSettingsRepository {
    fn https_settings(
        &self,
        actor: &ActorContext,
    ) -> Result<HttpsSettingsState, HttpsSettingsRepositoryError>;
    /// Requires an exact current local GlobalAdmin and a trusted configuration
    /// permit proving logical Web admission stays disabled until this task ends.
    fn execute_https_settings(
        &mut self,
        request: HttpsSettingsRequest,
        actor: &ActorContext,
    ) -> Result<HttpsSettingsState, HttpsSettingsRepositoryError>;
}
