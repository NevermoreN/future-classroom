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
