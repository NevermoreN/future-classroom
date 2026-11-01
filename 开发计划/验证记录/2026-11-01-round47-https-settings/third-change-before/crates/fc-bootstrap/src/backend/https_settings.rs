//! Local HTTPS configuration only. No listener, certificate inspection, OS
//! configuration, or positive production readiness capability is created here.
use super::*;
use fc_domain::https_settings::HttpsSettingsError;
use fc_ports::{
    HttpsSettingsRepository, HttpsSettingsRepositoryError, HttpsSettingsRequest, HttpsSettingsState,
};
fn unavailable() -> BackendError {
    BackendError::HttpsSettings(HttpsSettingsRepositoryError::Unavailable)
}
impl Backend {
    fn https_manager(&self, lease: SessionLease) -> Result<(ActorContext, u64)> {
        let identity = self.state()?;
        let (actor, generation) = self.actor(&identity, lease, SessionPurpose::Manage)?;
        identity
            .authority(actor.principal_id)
            .map_err(|e| BackendError::Identity(IdentityRepositoryError::Domain(e)))?
            .authorize_global_local(&actor, self.clock.now())
            .map_err(|e| {
                BackendError::Identity(IdentityRepositoryError::Domain(IdentityError::Permission(
                    e,
                )))
            })?;
        identity
            .authorize_management(&actor, self.clock.now())
            .map_err(|e| BackendError::Identity(IdentityRepositoryError::Domain(e)))?;
        Ok((actor, generation))
    }
    pub fn read_https_settings(&self, lease: SessionLease) -> Result<HttpsSettingsState> {
        let (actor, generation) = self.https_manager(lease)?;
        let result = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .https_settings(&actor)?;
        self.https_manager(lease)?;
        self.control.check(lease, generation)?;
        if self.state()?.revision != result.identity_revision {
            return Err(unavailable());
        }
        Ok(result)
    }
    /// Keep the original command id for unknown-result reconciliation. A failed
    /// delivery after SQL COMMIT does not prove rollback or permit a new retry id.
    pub fn save_https_settings(
        &mut self,
        lease: SessionLease,
        request: HttpsSettingsRequest,
    ) -> Result<HttpsSettingsState> {
        self.check_open()?;
        let permit = self
            .web
            .configuration_edit(lease)
            .map_err(|error| match error {
                web::WebServiceError::Busy => {
                    BackendError::HttpsSettings(HttpsSettingsRepositoryError::Domain(
                        HttpsSettingsError::ConfigurationUnavailable,
                    ))
                }
                _ => unavailable(),
            })?;
        let (actor, generation) = self.https_manager(lease)?;
        let mut repository = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .with_https_configuration_guard(permit.guard());
        let result = repository.execute_https_settings(request, &actor)?;
        self.https_manager(lease)?;
        self.control.check(lease, generation)?;
        // This can be the historical original-command result. Fresh exact
        // authorization above is mandatory; an unrelated current identity
        // revision must not turn that committed receipt into a new command.
        Ok(result)
    }
}
