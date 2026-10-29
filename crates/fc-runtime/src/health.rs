use crate::{
    cancel::CancellationSource,
    control::{ControlSignal, ControlSignals},
};
use fc_domain::ids::{AuthEpoch, Revision};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Health {
    Starting,
    Healthy,
    AuthorityUpdating,
    Faulted,
    Stopping,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateError {
    Starting,
    Updating,
    Faulted,
    Stopping,
    StaleLease,
    LeaseExpired,
    ScopeMismatch,
    VersionConflict,
    EpochExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthStatus {
    pub health: Health,
    pub health_epoch: u64,
    pub auth_epoch: AuthEpoch,
    pub revision: Revision,
    pub cancellation_generation: u64,
}

/// Construct only after the application authorizes this particular scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectLease {
    pub scope: u64,
    pub auth_epoch: AuthEpoch,
    pub health_epoch: u64,
    pub cancellation_generation: u64,
    pub expires_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectIntent {
    pub scope: u64,
    pub job_id: u64,
}

/// A registration at the actual execution domain, never a queued authorization.
/// It is deliberately not Clone; recheck before entering an irreversible call.
#[derive(Debug)]
pub struct StartPermit {
    pub(crate) lease: EffectLease,
    pub(crate) sequence: u64,
    pub(crate) intent: EffectIntent,
}

impl StartPermit {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn intent(&self) -> EffectIntent {
        self.intent
    }
    pub fn health_epoch(&self) -> u64 {
        self.lease.health_epoch
    }
}

pub(crate) struct ControlState {
    pub status: HealthStatus,
    next_start: u64,
}

pub(crate) struct GateInner {
    pub state: Mutex<ControlState>,
    pub cancel: CancellationSource,
    pub controls: Vec<ControlSignals>,
}

#[derive(Clone)]
pub struct HealthGate(pub(crate) Arc<GateInner>);

impl HealthGate {
    pub(crate) fn new(
        revision: Revision,
        auth_epoch: AuthEpoch,
        controls: Vec<ControlSignals>,
    ) -> Self {
        Self(Arc::new(GateInner {
            state: Mutex::new(ControlState {
                status: HealthStatus {
                    health: Health::Starting,
                    health_epoch: 0,
                    auth_epoch,
                    revision,
                    cancellation_generation: 0,
                },
                next_start: 0,
            }),
            cancel: CancellationSource::new(),
            controls,
        }))
    }
    pub(crate) fn lock(&self) -> MutexGuard<'_, ControlState> {
        match self.0.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                self.fault_locked(&mut state);
                state
            }
        }
    }
    pub fn status(&self) -> HealthStatus {
        self.lock().status
    }
    pub fn cancellation(&self) -> crate::cancel::CancellationToken {
        self.0.cancel.token()
    }
    pub fn check_sensitive(&self) -> Result<HealthStatus, GateError> {
        let state = self.lock();
        Self::require_healthy(&state)?;
        Ok(state.status)
    }
    /// Copy the original application lease deadline; never extend it on a read.
    pub fn lease_for_authorized_scope(
        &self,
        scope: u64,
        expires_at: Instant,
    ) -> Result<EffectLease, GateError> {
        let status = self.check_sensitive()?;
        if Instant::now() >= expires_at {
            return Err(GateError::LeaseExpired);
        }
        Ok(EffectLease {
            scope,
            auth_epoch: status.auth_epoch,
            health_epoch: status.health_epoch,
            cancellation_generation: status.cancellation_generation,
            expires_at,
        })
    }
    pub fn try_start(
        &self,
        lease: EffectLease,
        intent: EffectIntent,
    ) -> Result<StartPermit, GateError> {
        let mut state = self.lock();
        Self::validate_lease(&state, lease, intent.scope)?;
        let sequence = match state.next_start.checked_add(1) {
            Some(value) => value,
            None => {
                self.fault_locked(&mut state);
                return Err(GateError::EpochExhausted);
            }
        };
        state.next_start = sequence;
        Ok(StartPermit {
            lease,
            sequence,
            intent,
        })
    }
    pub fn check_permit(&self, permit: &StartPermit) -> Result<(), GateError> {
        Self::validate_lease(&self.lock(), permit.lease, permit.intent.scope)
    }
    pub fn fault(&self) {
        self.fault_locked(&mut self.lock());
    }
    pub fn stop(&self) {
        let mut state = self.lock();
        if state.status.health != Health::Faulted && state.status.health != Health::Stopping {
            state.status.health = Health::Stopping;
            state.status.health_epoch = state.status.health_epoch.saturating_add(1);
            state.status.cancellation_generation = self.0.cancel.cancel();
        }
        self.raise(ControlSignal::Stop);
        self.raise(ControlSignal::ForegroundHidden);
    }
    pub(crate) fn raise(&self, signal: ControlSignal) {
        for domain in &self.0.controls {
            domain.raise(signal);
        }
    }
    pub(crate) fn fault_locked(&self, state: &mut ControlState) {
        if state.status.health != Health::Faulted {
            state.status.health = Health::Faulted;
            state.status.health_epoch = state.status.health_epoch.saturating_add(1);
            state.status.cancellation_generation = self.0.cancel.cancel();
        }
        self.raise(ControlSignal::Fault);
        self.raise(ControlSignal::ForegroundHidden);
    }
    pub(crate) fn require_healthy(state: &ControlState) -> Result<(), GateError> {
        match state.status.health {
            Health::Healthy => Ok(()),
            Health::Starting => Err(GateError::Starting),
            Health::AuthorityUpdating => Err(GateError::Updating),
            Health::Faulted => Err(GateError::Faulted),
            Health::Stopping => Err(GateError::Stopping),
        }
    }
    fn validate_lease(
        state: &ControlState,
        lease: EffectLease,
        scope: u64,
    ) -> Result<(), GateError> {
        Self::require_healthy(state)?;
        if Instant::now() >= lease.expires_at {
            return Err(GateError::LeaseExpired);
        }
        if lease.scope != scope {
            return Err(GateError::ScopeMismatch);
        }
        if lease.health_epoch != state.status.health_epoch
            || lease.auth_epoch != state.status.auth_epoch
            || lease.cancellation_generation != state.status.cancellation_generation
        {
            return Err(GateError::StaleLease);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::AuthorityStore;
    use std::time::Duration;

    #[test]
    fn expired_queued_lease_and_scope_mismatch_never_register_a_start() {
        let (mut writer, reader) = AuthorityStore::new((), Revision::new(1), AuthEpoch::new(1));
        writer.initialize().unwrap();
        let gate = reader.gate();
        let now = Instant::now();
        assert_eq!(
            gate.lease_for_authorized_scope(1, now),
            Err(GateError::LeaseExpired)
        );
        let mut lease = gate
            .lease_for_authorized_scope(1, now + Duration::from_secs(30))
            .unwrap();
        assert!(matches!(
            gate.try_start(
                lease,
                EffectIntent {
                    scope: 2,
                    job_id: 1
                }
            ),
            Err(GateError::ScopeMismatch)
        ));
        lease.expires_at = now;
        assert!(matches!(
            gate.try_start(
                lease,
                EffectIntent {
                    scope: 1,
                    job_id: 1
                }
            ),
            Err(GateError::LeaseExpired)
        ));
        assert_eq!(gate.lock().next_start, 0);
    }

    #[test]
    fn poisoned_control_mutex_becomes_permanently_faulted() {
        let (mut writer, reader) = AuthorityStore::new((), Revision::new(1), AuthEpoch::new(1));
        writer.initialize().unwrap();
        let gate = reader.gate();
        let poison = gate.clone();
        assert!(std::thread::spawn(move || {
            let _held = poison.0.state.lock().unwrap();
            panic!("injected control lock poison");
        })
        .join()
        .is_err());
        assert_eq!(gate.check_sensitive(), Err(GateError::Faulted));
        gate.stop();
        assert_eq!(gate.status().health, Health::Faulted);
    }
}
