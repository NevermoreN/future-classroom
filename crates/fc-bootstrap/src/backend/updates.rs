//! Software maintenance uses its independent revision on the existing writer.
use super::*;
use fc_domain::update_state::{UpdateMaintenanceState, UpdatePolicy};
use fc_ports::updates::{
    UpdateChangeRequest, UpdateMaintenanceRepository, UpdateMutation, UpdateRepositoryError,
};
impl From<UpdateRepositoryError> for BackendError {
    fn from(error: UpdateRepositoryError) -> Self {
        Self::Updates(error)
    }
}
impl Backend {
    pub fn accept_update_release(
        &mut self,
        expected_policy_revision: Revision,
        release: fc_domain::updates::ReleaseIdentity,
    ) -> Result<UpdateMaintenanceState> {
        self.check_open()?;
        Ok(self
            .repository
            .accept_update_release(expected_policy_revision, release)?)
    }
    pub fn update_runtime_state(&self) -> Result<UpdateMaintenanceState> {
        self.check_open()?;
        Ok(self.repository.update_runtime_state()?)
    }
    pub fn begin_background_update(
        &mut self,
        expected_revision: Revision,
        expected_policy_revision: Revision,
        release: fc_domain::updates::ReleaseIdentity,
    ) -> Result<UpdateMaintenanceState> {
        self.check_open()?;
        Ok(self.repository.begin_background_update(
            expected_revision,
            expected_policy_revision,
            release,
        )?)
    }
    pub fn advance_background_update(
        &mut self,
        job: &fc_domain::update_state::UpdateJob,
        phase: fc_domain::update_state::UpdateJobPhase,
    ) -> Result<UpdateMaintenanceState> {
        self.check_open()?;
        Ok(self.repository.advance_background_update(job, phase)?)
    }
    fn update_manager(&self, lease: SessionLease) -> Result<(ActorContext, u64)> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        state
            .authority(actor.principal_id)
            .map_err(|error| BackendError::Identity(IdentityRepositoryError::Domain(error)))?
            .authorize_global(&actor, self.clock.now())
            .map_err(|error| {
                BackendError::Identity(IdentityRepositoryError::Domain(IdentityError::Permission(
                    error,
                )))
            })?;
        state
            .authorize_management(&actor, self.clock.now())
            .map_err(|error| BackendError::Identity(IdentityRepositoryError::Domain(error)))?;
        Ok((actor, generation))
    }
    pub fn read_update_status(&self, lease: SessionLease) -> Result<UpdateMaintenanceState> {
        let (actor, generation) = self.update_manager(lease)?;
        let state = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .update_status(&actor)?;
        self.update_manager(lease)?;
        self.control.check(lease, generation)?;
        Ok(state)
    }
    pub fn save_update_policy(
        &mut self,
        lease: SessionLease,
        expected_revision: Revision,
        policy: UpdatePolicy,
    ) -> Result<UpdateMaintenanceState> {
        let (actor, generation) = self.update_manager(lease)?;
        let request = UpdateChangeRequest {
            command_id: self.issued_command_id()?,
            expected_revision,
            mutation: UpdateMutation::SetPolicy(policy),
        };
        let state = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .change_update(request, &actor)?;
        self.update_manager(lease)?;
        self.control.check(lease, generation)?;
        Ok(state)
    }
}
