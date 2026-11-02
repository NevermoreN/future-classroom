//! Short metadata reservation shared by configuration save and the physical
//! HTTPS domain. Neither cancellation nor a UI ticket releases this claim.
use super::*;

#[cfg(any(windows, test))]
pub(crate) struct WebRuntimeReservation {
    service: WebService,
    snapshot: Arc<AuthoritySnapshot<IdentityState>>,
    lease: SessionLease,
    generation: u64,
    epoch: u64,
    started: AtomicBool,
    returned: AtomicBool,
}
impl WebService {
    #[cfg(any(windows, test))]
    pub(crate) fn reserve_runtime(
        &self,
        lease: SessionLease,
    ) -> WebResult<Arc<WebRuntimeReservation>> {
        // Do not retain a previous worker's execution fence in a long-lived domain.
        if self.execution.is_some() {
            return Err(WebServiceError::Invalid);
        }
        let snapshot = self.snapshot()?;
        let state = self
            .inner
            .state
            .try_lock()
            .map_err(|_| WebServiceError::Busy)?;
        let mut ledger = self
            .inner
            .ledger
            .try_lock()
            .map_err(|_| WebServiceError::Busy)?;
        let generation = self.admin(&mut ledger, &snapshot.data, lease)?;
        self.admin_current(&snapshot, &mut ledger, lease, generation)?;
        if state.enabled || self.inner.configuration_edit.load(Ordering::Acquire) {
            return Err(WebServiceError::Busy);
        }
        if self.inner.runtime_claim.load(Ordering::Acquire) != 0 {
            return Err(WebServiceError::Busy);
        }
        // This monotonically increasing value is the exact physical claim,
        // independently retained while admission's epoch can become negative.
        let epoch = self
            .inner
            .runtime_epoch
            .try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
                word.checked_add(2).map(|word| word & !1)
            })
            .map(|previous| (previous + 2) & !1)
            .map_err(|_| WebServiceError::Overflow)?;
        self.inner
            .runtime_claim
            .compare_exchange(0, epoch, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| WebServiceError::Busy)?;
        self.inner.runtime_ready.store(false, Ordering::Release);
        Ok(Arc::new(WebRuntimeReservation {
            service: self.clone(),
            snapshot,
            lease,
            generation,
            epoch,
            started: AtomicBool::new(false),
            returned: AtomicBool::new(false),
        }))
    }
    /// Pure atomic negative path; safe even while the session ledger is held by
    /// another worker. Exact old Web SQL guards observe this before/after actor.
    pub fn runtime_execution_current(&self) -> bool {
        self.inner.runtime_epoch.load(Ordering::Acquire) & 1 == 1
    }
    #[cfg(windows)]
    pub(crate) fn runtime_domain_current(&self) -> bool {
        self.runtime_execution_current()
            && !self.inner.control.0.closed.load(Ordering::Acquire)
            && !self.inner.control.is_recovering()
            && matches!(
                self.inner.control.health_gate().status().health,
                Health::Healthy | Health::AuthorityUpdating
            )
    }
    pub fn runtime_generation(&self) -> Option<u64> {
        let epoch = self.inner.runtime_epoch.load(Ordering::Acquire);
        if epoch & 1 == 0 || !self.inner.runtime_ready.load(Ordering::Acquire) {
            return None;
        }
        let generation = self.inner.runtime_native_generation.load(Ordering::Acquire);
        (self.inner.runtime_epoch.load(Ordering::Acquire) == epoch).then_some(generation)
    }
    pub(crate) fn negative_runtime_stop(&self) {
        // One atomic word changes the epoch and clears admission. A concurrent
        // native-ready CAS can neither overwrite nor adopt this newer stop.
        let _ = self
            .inner
            .runtime_epoch
            .try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
                Some(word.saturating_add(2) & !1)
            });
        self.inner.runtime_stopping.store(true, Ordering::Release);
        self.inner.runtime_ready.store(false, Ordering::Release);
    }
    #[cfg(any(windows, test))]
    pub(crate) fn settle_runtime_stop(&self) -> bool {
        self.negative_runtime_stop();
        // HTTP must keep servicing cancellation/deadline/IOCP while another
        // worker or Writer holds metadata. This negative path never waits for
        // either lock and retries on its next bounded event-loop turn.
        let Ok(mut state) = self.inner.state.try_lock() else {
            return false;
        };
        let Ok(mut ledger) = self.inner.ledger.try_lock() else {
            return false;
        };
        state.enabled = false;
        state.clear_pairing();
        state.devices.clear();
        ledger.sessions.revoke_web();
        true
    }
}
#[cfg(any(windows, test))]
impl WebRuntimeReservation {
    #[cfg(windows)]
    pub(crate) fn identity_revision(&self) -> Revision {
        self.snapshot.data.revision
    }
    pub(crate) fn current(&self) -> bool {
        let Ok(mut ledger) = self.service.inner.ledger.try_lock() else {
            return false;
        };
        self.service
            .admin_current(&self.snapshot, &mut ledger, self.lease, self.generation)
            .is_ok()
    }
    #[cfg(windows)]
    pub(crate) fn execution_fence(&self) -> TransactionExecutionFence {
        let inner = self.service.inner.clone();
        let epoch = self.epoch | 1;
        Arc::new(move || inner.runtime_epoch.load(Ordering::Acquire) == epoch)
    }
    pub(crate) fn mark_started(&self) {
        self.started.store(true, Ordering::Release);
    }
    /// Only after native open/credential/listen succeeded and a fresh exact
    /// original authorization interval still holds. No caller-created bool is
    /// accepted as a native readiness receipt.
    pub(crate) fn activate(&self, native_generation: u64) -> WebResult<()> {
        let mut state = self
            .service
            .inner
            .state
            .try_lock()
            .map_err(|_| WebServiceError::Busy)?;
        let mut ledger = self
            .service
            .inner
            .ledger
            .try_lock()
            .map_err(|_| WebServiceError::Busy)?;
        self.service
            .admin_current(&self.snapshot, &mut ledger, self.lease, self.generation)?;
        if !self.owns_claim() {
            return Err(WebServiceError::Disabled);
        }
        // Revoke the complete previous browser epoch before opening a new one;
        // a new true admission bit must never revive an old exact lease.
        state.clear_pairing();
        state.devices.clear();
        ledger.sessions.revoke_web();
        self.service
            .inner
            .runtime_native_generation
            .store(native_generation, Ordering::Release);
        self.service
            .inner
            .runtime_epoch
            .compare_exchange(
                self.epoch,
                self.epoch | 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| WebServiceError::Disabled)?;
        self.service
            .inner
            .runtime_stopping
            .store(false, Ordering::Release);
        self.service
            .inner
            .runtime_ready
            .store(true, Ordering::Release);
        state.enabled = true;
        Ok(())
    }
    pub(crate) fn owns_claim(&self) -> bool {
        self.service.inner.runtime_claim.load(Ordering::Acquire) == self.epoch
            && !self.returned.load(Ordering::Acquire)
    }
    /// Exact old runtime owners may revoke only their own admission epoch.
    /// A delayed request_stop can never close a replacement generation.
    pub(crate) fn negative_stop(&self) -> bool {
        if !self.owns_claim() {
            return false;
        }
        let _ = self.service.inner.runtime_epoch.try_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |word| {
                if word == self.epoch || word == (self.epoch | 1) {
                    self.epoch.checked_add(2)
                } else {
                    None
                }
            },
        );
        self.service
            .inner
            .runtime_ready
            .store(false, Ordering::Release);
        self.service
            .inner
            .runtime_stopping
            .store(true, Ordering::Release);
        true
    }
    /// Caller must have observed the original HTTP thread's physical exit,
    /// after its Net/native owner and final worker cleanup already returned.
    pub(crate) fn physical_returned(&self) {
        if self.returned.swap(true, Ordering::AcqRel) {
            return;
        }
        // Clear readiness while this claim still excludes a new reservation.
        // No metadata store is permitted after relinquishing the exact claim.
        if self.service.inner.runtime_claim.load(Ordering::Acquire) == self.epoch {
            self.service
                .inner
                .runtime_ready
                .store(false, Ordering::Release);
            let _ = self.service.inner.runtime_claim.compare_exchange(
                self.epoch,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}
#[cfg(any(windows, test))]
impl Drop for WebRuntimeReservation {
    fn drop(&mut self) {
        if !self.started.load(Ordering::Acquire) && !self.returned.load(Ordering::Acquire) {
            // An unstarted plan may release only its own claim. A returned
            // reservation was already released and must never clear it again.
            let _ = self.service.inner.runtime_claim.compare_exchange(
                self.epoch,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        // Unknown native/worker retirement intentionally leaves its exact
        // runtime_claim set until process recovery; no replacement is admitted.
    }
}
