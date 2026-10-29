//! The publisher is a unique writer capability; readers only share immutable data.
use crate::{
    control::{ControlSignal, ControlSignals},
    health::{GateError, Health, HealthGate},
};
use fc_domain::ids::{AuthEpoch, Revision};
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct AuthoritySnapshot<S> {
    pub data: S,
    pub revision: Revision,
    pub auth_epoch: AuthEpoch,
}

/// Allocation and validation occur before opening the authority update barrier.
/// S must have no panicking/custom external side effects in Drop.
pub struct PreparedAuthority<S>(Arc<AuthoritySnapshot<S>>);

struct AuthorityInner<S> {
    gate: HealthGate,
    current: RwLock<Arc<AuthoritySnapshot<S>>>,
}

pub struct AuthorityStore;
pub struct AuthorityPublisher<S> {
    inner: Arc<AuthorityInner<S>>,
}
pub struct AuthorityReader<S> {
    inner: Arc<AuthorityInner<S>>,
}

impl<S> Clone for AuthorityReader<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Dropping an unsettled transaction makes the entire process fail closed.
/// This also handles an unwinding database call without guessing its outcome.
pub struct AuthorityUpdateToken {
    gate: HealthGate,
    epoch: u64,
    settled: bool,
}

impl Drop for AuthorityUpdateToken {
    fn drop(&mut self) {
        if !self.settled {
            self.gate.fault();
        }
    }
}

impl AuthorityStore {
    // Split capabilities at construction so no cloneable store ever exposes a
    // way to manufacture an additional writer.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<S>(
        data: S,
        revision: Revision,
        auth_epoch: AuthEpoch,
    ) -> (AuthorityPublisher<S>, AuthorityReader<S>) {
        Self::with_controls(data, revision, auth_epoch, Vec::new())
    }
    /// Register a distinct independent control handle for every execution domain.
    pub fn with_controls<S>(
        data: S,
        revision: Revision,
        auth_epoch: AuthEpoch,
        controls: Vec<ControlSignals>,
    ) -> (AuthorityPublisher<S>, AuthorityReader<S>) {
        let inner = Arc::new(AuthorityInner {
            gate: HealthGate::new(revision, auth_epoch, controls),
            current: RwLock::new(Arc::new(AuthoritySnapshot {
                data,
                revision,
                auth_epoch,
            })),
        });
        (
            AuthorityPublisher {
                inner: inner.clone(),
            },
            AuthorityReader { inner },
        )
    }
}

impl<S> AuthorityReader<S> {
    pub fn gate(&self) -> HealthGate {
        self.inner.gate.clone()
    }
    /// Prepare owned candidate data before submitting a command to the writer.
    /// This allocates memory, but grants no publication or authorization rights;
    /// the writer still checks CAS, permissions and the candidate revision.
    pub fn prepare_update(
        &self,
        data: S,
        revision: Revision,
        auth_epoch: AuthEpoch,
    ) -> Result<PreparedAuthority<S>, GateError> {
        prepare(&self.inner.gate, data, revision, auth_epoch)
    }
    /// A snapshot's Arc lifetime never grants permission to use it later.
    /// Every sensitive action must also obtain a fresh execution StartPermit.
    pub fn snapshot(&self) -> Result<Arc<AuthoritySnapshot<S>>, GateError> {
        let mut state = self.inner.gate.lock();
        HealthGate::require_healthy(&state)?;
        match self.inner.current.read() {
            Ok(snapshot) => Ok(snapshot.clone()),
            Err(_) => {
                self.inner.gate.fault_locked(&mut state);
                Err(GateError::Faulted)
            }
        }
    }
}

impl<S> AuthorityPublisher<S> {
    pub fn gate(&self) -> HealthGate {
        self.inner.gate.clone()
    }
    pub fn initialize(&mut self) -> Result<(), GateError> {
        let mut state = self.inner.gate.lock();
        if state.status.health != Health::Starting {
            return Err(GateError::VersionConflict);
        }
        state.status.health = Health::Healthy;
        Ok(())
    }
    pub fn prepare(
        &self,
        data: S,
        revision: Revision,
        auth_epoch: AuthEpoch,
    ) -> Result<PreparedAuthority<S>, GateError> {
        prepare(&self.inner.gate, data, revision, auth_epoch)
    }
    pub fn begin_update(&mut self) -> Result<AuthorityUpdateToken, GateError> {
        let mut state = self.inner.gate.lock();
        HealthGate::require_healthy(&state)?;
        let epoch = match state.status.health_epoch.checked_add(1) {
            Some(epoch) => epoch,
            None => {
                self.inner.gate.fault_locked(&mut state);
                return Err(GateError::EpochExhausted);
            }
        };
        state.status.health_epoch = epoch;
        state.status.cancellation_generation = self.inner.gate.0.cancel.cancel();
        if state.status.cancellation_generation == u64::MAX {
            self.inner.gate.fault_locked(&mut state);
            return Err(GateError::EpochExhausted);
        }
        state.status.health = Health::AuthorityUpdating;
        self.inner.gate.raise(ControlSignal::AuthorityInvalidated);
        Ok(AuthorityUpdateToken {
            gate: self.inner.gate.clone(),
            epoch,
            settled: false,
        })
    }
    /// Call only after COMMIT is known successful. No allocation or observer callback
    /// occurs between the snapshot exchange and returning the gate to Healthy.
    pub fn publish(
        &mut self,
        mut update: AuthorityUpdateToken,
        snapshot: PreparedAuthority<S>,
    ) -> Result<(), GateError> {
        update.settled = true;
        let mut state = self.inner.gate.lock();
        if !Arc::ptr_eq(&update.gate.0, &self.inner.gate.0)
            || state.status.health != Health::AuthorityUpdating
            || state.status.health_epoch != update.epoch
        {
            self.inner.gate.fault_locked(&mut state);
            drop(state);
            update.gate.fault();
            return Err(GateError::Faulted);
        }
        if snapshot.0.revision <= state.status.revision
            || snapshot.0.auth_epoch < state.status.auth_epoch
        {
            self.inner.gate.fault_locked(&mut state);
            return Err(GateError::VersionConflict);
        }
        let mut current = match self.inner.current.write() {
            Ok(current) => current,
            Err(_) => {
                self.inner.gate.fault_locked(&mut state);
                return Err(GateError::Faulted);
            }
        };
        state.status.revision = snapshot.0.revision;
        state.status.auth_epoch = snapshot.0.auth_epoch;
        let old = std::mem::replace(&mut *current, snapshot.0);
        state.status.health = Health::Healthy;
        drop(current);
        drop(state);
        // Destruction and observer notification cannot run in the control lock.
        drop(old);
        Ok(())
    }
    /// Proof of rollback belongs to the transaction owner. This never restores an
    /// old health epoch or cancellation generation, so old leases cannot revive.
    pub fn rollback(&mut self, mut update: AuthorityUpdateToken) -> Result<(), GateError> {
        update.settled = true;
        let mut state = self.inner.gate.lock();
        if !Arc::ptr_eq(&update.gate.0, &self.inner.gate.0)
            || state.status.health != Health::AuthorityUpdating
            || state.status.health_epoch != update.epoch
        {
            self.inner.gate.fault_locked(&mut state);
            drop(state);
            update.gate.fault();
            return Err(GateError::Faulted);
        }
        state.status.health = Health::Healthy;
        Ok(())
    }
    pub fn fault(&mut self) {
        self.inner.gate.fault();
    }
}

fn prepare<S>(
    gate: &HealthGate,
    data: S,
    revision: Revision,
    auth_epoch: AuthEpoch,
) -> Result<PreparedAuthority<S>, GateError> {
    let current = gate.check_sensitive()?;
    if revision <= current.revision || auth_epoch < current.auth_epoch {
        return Err(GateError::VersionConflict);
    }
    Ok(PreparedAuthority(Arc::new(AuthoritySnapshot {
        data,
        revision,
        auth_epoch,
    })))
}

impl<S> fc_ports::AuthorityPublisher for AuthorityPublisher<S> {
    type Snapshot = PreparedAuthority<S>;
    type Update = AuthorityUpdateToken;
    type Error = GateError;
    fn snapshot_revision(&self, snapshot: &Self::Snapshot) -> Revision {
        snapshot.0.revision
    }
    fn begin_update(&mut self) -> Result<Self::Update, Self::Error> {
        self.begin_update()
    }
    fn publish(
        &mut self,
        update: Self::Update,
        snapshot: Self::Snapshot,
    ) -> Result<(), Self::Error> {
        self.publish(update, snapshot)
    }
    fn rollback(&mut self, update: Self::Update) -> Result<(), Self::Error> {
        self.rollback(update)
    }
    fn fault(&mut self) {
        self.fault();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control::ControlSignal,
        health::{EffectIntent, StartPermit},
    };
    use std::{
        panic::{catch_unwind, AssertUnwindSafe},
        sync::Barrier,
        thread,
        time::{Duration, Instant},
    };

    fn ready() -> (
        AuthorityPublisher<&'static str>,
        AuthorityReader<&'static str>,
    ) {
        let (mut writer, reader) = AuthorityStore::new("old", Revision::new(1), AuthEpoch::new(1));
        assert!(matches!(reader.snapshot(), Err(GateError::Starting)));
        writer.initialize().unwrap();
        (writer, reader)
    }

    fn start(reader: &AuthorityReader<&'static str>) -> StartPermit {
        let gate = reader.gate();
        let lease = gate
            .lease_for_authorized_scope(4, Instant::now() + Duration::from_secs(30))
            .unwrap();
        gate.try_start(
            lease,
            EffectIntent {
                scope: 4,
                job_id: 8,
            },
        )
        .unwrap()
    }

    #[test]
    fn rollback_keeps_old_data_but_never_revives_old_permits() {
        let (mut writer, reader) = ready();
        let permit = start(&reader);
        let old_health_epoch = reader.gate().status().health_epoch;
        let cancelled = reader.gate().cancellation();
        let update = writer.begin_update().unwrap();
        assert!(cancelled.is_cancelled());
        assert!(matches!(reader.snapshot(), Err(GateError::Updating)));
        assert_eq!(
            reader.gate().check_permit(&permit),
            Err(GateError::Updating)
        );
        writer.rollback(update).unwrap();
        assert_eq!(reader.snapshot().unwrap().data, "old");
        let status = reader.gate().status();
        assert_eq!(status.auth_epoch, AuthEpoch::new(1));
        assert_eq!(status.revision, Revision::new(1));
        assert!(status.health_epoch > old_health_epoch);
        assert_eq!(
            reader.gate().check_permit(&permit),
            Err(GateError::StaleLease)
        );
        assert!(reader.gate().check_permit(&start(&reader)).is_ok());
    }

    #[test]
    fn publish_swaps_authority_before_opening_the_gate() {
        let (mut writer, reader) = ready();
        let pending = writer
            .prepare("new", Revision::new(2), AuthEpoch::new(2))
            .unwrap();
        let old = reader.snapshot().unwrap();
        let old_permit = start(&reader);
        let update = writer.begin_update().unwrap();
        writer.publish(update, pending).unwrap();
        let current = reader.snapshot().unwrap();
        assert_eq!(old.data, "old"); // Arc retention does not preserve authority.
        assert_eq!(current.data, "new");
        assert_eq!(current.auth_epoch, reader.gate().status().auth_epoch);
        assert_eq!(current.revision, reader.gate().status().revision);
        assert_eq!(
            reader.gate().check_permit(&old_permit),
            Err(GateError::StaleLease)
        );
    }

    #[test]
    fn publication_failure_after_commit_faults_instead_of_serving_old_snapshot() {
        let ui = ControlSignals::default();
        let audio = ControlSignals::default();
        let (mut writer, reader) = AuthorityStore::with_controls(
            "old",
            Revision::new(1),
            AuthEpoch::new(1),
            vec![ui.clone(), audio.clone()],
        );
        writer.initialize().unwrap();
        let old_permit = start(&reader);
        let pending = writer
            .prepare("committed", Revision::new(2), AuthEpoch::new(2))
            .unwrap();
        let update = writer.begin_update().unwrap();
        // Simulate failure of the actual snapshot synchronization primitive at
        // the post-COMMIT boundary, rather than a fake success/error callback.
        let inner = writer.inner.clone();
        assert!(thread::spawn(move || {
            let _snapshot = inner.current.write().unwrap();
            panic!("injected publication lock failure");
        })
        .join()
        .is_err());
        assert_eq!(writer.publish(update, pending), Err(GateError::Faulted));
        assert!(matches!(reader.snapshot(), Err(GateError::Faulted)));
        assert_eq!(
            reader.gate().check_permit(&old_permit),
            Err(GateError::Faulted)
        );
        assert!(ui.take().contains(ControlSignal::ForegroundHidden));
        assert!(audio.take().contains(ControlSignal::Fault));
        assert!(writer.initialize().is_err());
        reader.gate().stop();
        assert_eq!(reader.gate().status().health, Health::Faulted);
        assert!(writer
            .prepare("restart forbidden", Revision::new(3), AuthEpoch::new(3))
            .is_err());
    }

    #[test]
    fn unknown_commit_unwind_and_abandoned_update_are_fail_closed() {
        let (mut writer, reader) = ready();
        let cancelled = reader.gate().cancellation();
        assert!(catch_unwind(AssertUnwindSafe(|| {
            let _update = writer.begin_update().unwrap();
            panic!("unknown database outcome");
        }))
        .is_err());
        assert_eq!(reader.gate().status().health, Health::Faulted);
        assert!(cancelled.is_cancelled());
        let (mut other, other_reader) = ready();
        drop(other.begin_update().unwrap());
        assert_eq!(other_reader.gate().status().health, Health::Faulted);
    }

    #[test]
    fn a_foreign_update_token_faults_both_stores_without_deadlocking() {
        let (mut first, first_reader) = ready();
        let (mut second, second_reader) = ready();
        let token = first.begin_update().unwrap();
        assert_eq!(second.rollback(token), Err(GateError::Faulted));
        assert_eq!(first_reader.gate().status().health, Health::Faulted);
        assert_eq!(second_reader.gate().status().health, Health::Faulted);
    }

    #[test]
    fn stale_prepared_snapshot_after_commit_is_rejected_and_faults() {
        let (mut writer, reader) = ready();
        let stale = writer
            .prepare("stale", Revision::new(2), AuthEpoch::new(1))
            .unwrap();
        let current = writer
            .prepare("current", Revision::new(3), AuthEpoch::new(1))
            .unwrap();
        let token = writer.begin_update().unwrap();
        writer.publish(token, current).unwrap();
        let token = writer.begin_update().unwrap();
        assert_eq!(
            writer.publish(token, stale),
            Err(GateError::VersionConflict)
        );
        assert!(matches!(reader.snapshot(), Err(GateError::Faulted)));
    }

    #[test]
    fn start_registration_and_authority_barrier_share_one_linearization_lock() {
        for _ in 0..64 {
            let (mut writer, reader) = ready();
            let gate = reader.gate();
            let lease = gate
                .lease_for_authorized_scope(4, Instant::now() + Duration::from_secs(30))
                .unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let concurrent_gate = gate.clone();
            let concurrent_barrier = barrier.clone();
            let starter = thread::spawn(move || {
                concurrent_barrier.wait();
                concurrent_gate.try_start(
                    lease,
                    EffectIntent {
                        scope: 4,
                        job_id: 8,
                    },
                )
            });
            barrier.wait();
            let update = writer.begin_update().unwrap();
            match starter.join().unwrap() {
                Ok(before_update) => {
                    assert_eq!(before_update.health_epoch(), lease.health_epoch);
                    assert_eq!(gate.check_permit(&before_update), Err(GateError::Updating));
                }
                Err(error) => assert_eq!(error, GateError::Updating),
            }
            // Once begin_update has returned, an old queued lease cannot start.
            assert!(matches!(
                gate.try_start(
                    lease,
                    EffectIntent {
                        scope: 4,
                        job_id: 9
                    }
                ),
                Err(GateError::Updating)
            ));
            writer.rollback(update).unwrap();
            assert!(matches!(
                gate.try_start(
                    lease,
                    EffectIntent {
                        scope: 4,
                        job_id: 10
                    }
                ),
                Err(GateError::StaleLease)
            ));
        }
    }
}
