//! The registry effect has its own fresh memory-only permission. Database
//! tickets settle facts, and never grant permission to execute another effect.
use super::*;
use fc_ports::autostart::*;

#[derive(Clone)]
pub struct AutostartEffectGuard {
    check: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl AutostartEffectGuard {
    pub fn is_valid(&self) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.check)())).unwrap_or(false)
    }
}

impl Backend {
    /// Deployment-owned observation path. This exposes facts, never a local
    /// actor or permission to change the current-user Run item.
    pub fn autostart_reconciliation_status(&self) -> Result<AutostartState> {
        self.check_open()?;
        Ok(self.repository.autostart_reconciliation_status()?)
    }
    pub fn autostart_status(&self, lease: SessionLease) -> Result<AutostartState> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .autostart_status(&actor)?)
    }
    pub fn begin_autostart_while(
        &mut self,
        lease: SessionLease,
        expected_revision: Revision,
        desired: bool,
        target_command: String,
        previous_observed: AutostartObservation,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<AutostartOperation> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let request = AutostartRequest {
            command_id: self.command_id()?,
            expected_revision,
            desired,
            target_command,
            previous_observed,
        };
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .with_autostart_execution_guard(fence)
            .begin_autostart(request, &actor)?)
    }
    pub fn mark_autostart_applying_while(
        &mut self,
        lease: SessionLease,
        ticket: AutostartTicket,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<AutostartOperation> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .with_autostart_execution_guard(fence)
            .mark_autostart_applying(ticket, &actor)?)
    }
    pub fn finish_autostart(
        &mut self,
        ticket: AutostartTicket,
        outcome: AutostartSystemOutcome,
    ) -> Result<AutostartState> {
        self.check_open()?;
        // Completion records the physical result even after session expiry or
        // cancellation. The exact prepared capability cannot authorize an effect.
        Ok(self.repository.finish_autostart(ticket, outcome)?)
    }
    pub fn reconcile_autostart(
        &mut self,
        expected_revision: Revision,
        observed: AutostartObservation,
    ) -> Result<AutostartState> {
        self.check_open()?;
        Ok(self
            .repository
            .reconcile_autostart(expected_revision, observed)?)
    }
    pub fn autostart_effect_guard(
        &self,
        lease: SessionLease,
        ticket: &AutostartTicket,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<AutostartEffectGuard> {
        self.check_open()?;
        let initial = self
            .authority
            .snapshot()
            .map_err(|_| BackendError::InvalidCredentials)?;
        let (actor, generation) = self.actor(&initial.data, lease, SessionPurpose::Manage)?;
        if actor.principal_id != ticket.principal_id || actor.auth_epoch != ticket.auth_epoch {
            return Err(BackendError::InvalidCredentials);
        }
        let current = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .autostart_status(&actor)?;
        if !current.current().is_some_and(|operation| {
            operation.ticket == *ticket && operation.phase == AutostartPhase::Applying
        }) || self
            .authority
            .snapshot()
            .map_err(|_| BackendError::InvalidCredentials)?
            .revision
            != initial.revision
        {
            return Err(BackendError::Autostart(AutostartRepositoryError::Stale));
        }
        initial
            .data
            .authorize_management(&actor, self.clock.now())
            .map_err(|_| BackendError::InvalidCredentials)?;
        let authority = self.authority.clone();
        let control = self.control.clone();
        let guard = self.session_guard(lease, generation, SessionPurpose::Manage);
        let clock = self.clock.clone();
        let check = Arc::new(move || {
            if !fence() || control.check(lease, generation).is_err() {
                return false;
            }
            let Ok(snapshot) = authority.snapshot() else {
                return false;
            };
            if snapshot.revision != initial.revision
                || guard(&snapshot.data, &actor).is_err()
                || snapshot
                    .data
                    .authorize_management(&actor, clock.now())
                    .is_err()
            {
                return false;
            }
            let health = authority.gate().status();
            health.health == fc_runtime::health::Health::Healthy
                && health.revision == snapshot.revision
                && control.check(lease, generation).is_ok()
                && fence()
        });
        Ok(AutostartEffectGuard { check })
    }
}
