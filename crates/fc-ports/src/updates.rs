//! Trusted software maintenance port; no HTTP input or physical installation
//! permission is conveyed by this interface. The sole writer rechecks the actor.
use fc_domain::{
    auth::ActorContext,
    ids::{CommandId, Revision},
    update_state::*,
    updates::ReleaseIdentity,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateMutation {
    SetPolicy(UpdatePolicy),
    PrepareJob {
        release: ReleaseIdentity,
    },
    /// For trusted orchestration facts only, never deserialize from a Web form.
    AdvanceJob {
        job_id: u64,
        phase: UpdateJobPhase,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateChangeRequest {
    pub command_id: CommandId,
    pub expected_revision: Revision,
    pub mutation: UpdateMutation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateRepositoryError {
    Denied,
    Invalid,
    Stale,
    CommandIdReused,
    Capacity,
    Busy,
    Cancelled,
    Unavailable,
    Faulted,
}
pub trait UpdateMaintenanceRepository {
    /// Trusted deployment/background composition only. This returns persisted
    /// intent/facts, never a browser actor or permission to launch an installer.
    fn update_runtime_state(&self) -> Result<UpdateMaintenanceState, UpdateRepositoryError>;
    /// Persist a verified publisher release independently of downloads/install.
    /// A lower sequence or same sequence with different bytes is never accepted.
    fn accept_update_release(
        &mut self,
        expected_policy_revision: Revision,
        release: ReleaseIdentity,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError>;
    /// Called only after publisher/source validation by the process update
    /// owner. The writer binds work to the exact current enrolled policy.
    fn begin_background_update(
        &mut self,
        expected_revision: Revision,
        expected_policy_revision: Revision,
        release: ReleaseIdentity,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError>;
    /// Exact durable job identity, not user-supplied Web state. Revoked policy
    /// may settle a running physical result, but cannot create a new job.
    fn advance_background_update(
        &mut self,
        job: &UpdateJob,
        phase: UpdateJobPhase,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError>;
    fn update_status(
        &self,
        actor: &ActorContext,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError>;
    fn change_update(
        &mut self,
        request: UpdateChangeRequest,
        actor: &ActorContext,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError>;
}
