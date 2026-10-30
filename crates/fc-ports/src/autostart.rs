//! Trusted local system-change orchestration. No method accepts a Web grant.
pub use fc_domain::autostart::*;
use fc_domain::{
    auth::ActorContext,
    ids::{CommandId, Revision},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutostartRequest {
    pub command_id: CommandId,
    pub expected_revision: Revision,
    pub desired: bool,
    pub target_command: String,
    pub previous_observed: AutostartObservation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutostartRepositoryError {
    Cancelled,
    Invalid,
    Denied,
    Capacity,
    Stale,
    Busy,
    Unavailable,
    Faulted,
}
pub trait AutostartRepository {
    /// Trusted deployment composition only: read before login, never an HTTP endpoint.
    fn autostart_reconciliation_status(&self) -> Result<AutostartState, AutostartRepositoryError>;
    fn autostart_status(
        &self,
        actor: &ActorContext,
    ) -> Result<AutostartState, AutostartRepositoryError>;
    fn begin_autostart(
        &mut self,
        request: AutostartRequest,
        actor: &ActorContext,
    ) -> Result<AutostartOperation, AutostartRepositoryError>;
    fn mark_autostart_applying(
        &mut self,
        ticket: AutostartTicket,
        actor: &ActorContext,
    ) -> Result<AutostartOperation, AutostartRepositoryError>;
    /// Exact prepared capability may settle physical results after its lease expires.
    /// It cannot create intent or authorize another registry write.
    fn finish_autostart(
        &mut self,
        ticket: AutostartTicket,
        outcome: AutostartSystemOutcome,
    ) -> Result<AutostartState, AutostartRepositoryError>;
    /// Exclusive startup/composition observation only. Never changes the OS.
    fn reconcile_autostart(
        &mut self,
        expected_revision: Revision,
        observed: AutostartObservation,
    ) -> Result<AutostartState, AutostartRepositoryError>;
}
pub trait AutostartSystem {
    fn target_command(&self) -> &str;
    fn observe(&self) -> AutostartObservation;
    fn apply(
        &mut self,
        expected: &AutostartObservation,
        enabled: bool,
        fresh: &dyn Fn() -> bool,
    ) -> AutostartSystemOutcome;
}
