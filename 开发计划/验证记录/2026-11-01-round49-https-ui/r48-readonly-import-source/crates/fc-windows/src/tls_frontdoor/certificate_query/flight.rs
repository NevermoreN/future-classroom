//! One process-wide physical inspection, independent of logical cancellation.
//! This is a metadata ledger, not a worker/resource pool. No native call runs
//! while its short lock is held. There is intentionally no reset/retry API.
use super::CertificateQueryError;
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Active,
    Faulted,
}

struct State<T> {
    phase: Phase,
    retained: Option<T>,
}

pub(super) struct FlightLedger<T> {
    state: Mutex<State<T>>,
}

impl<T> FlightLedger<T> {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                phase: Phase::Idle,
                retained: None,
            }),
        }
    }

    pub fn claim(&self) -> Result<Flight<'_, T>, CertificateQueryError> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                // Keep any retained owner; a poisoned ledger is never reset.
                poisoned.into_inner().phase = Phase::Faulted;
                return Err(CertificateQueryError::Faulted);
            }
        };
        match state.phase {
            Phase::Active => return Err(CertificateQueryError::Busy),
            Phase::Faulted => return Err(CertificateQueryError::Faulted),
            Phase::Idle => {}
        }
        if state.retained.is_some() {
            state.phase = Phase::Faulted;
            return Err(CertificateQueryError::Faulted);
        }
        state.phase = Phase::Active;
        Ok(Flight {
            ledger: self,
            finished: false,
        })
    }
}

pub(super) struct Flight<'a, T> {
    ledger: &'a FlightLedger<T>,
    finished: bool,
}

impl<T> Flight<'_, T> {
    /// Only after every native reference has actually been released.
    pub fn released(mut self) -> Result<(), CertificateQueryError> {
        let mut state = match self.ledger.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                poisoned.into_inner().phase = Phase::Faulted;
                self.finished = true;
                return Err(CertificateQueryError::Faulted);
            }
        };
        if state.phase != Phase::Active || state.retained.is_some() {
            state.phase = Phase::Faulted;
            self.finished = true;
            return Err(CertificateQueryError::Faulted);
        }
        state.phase = Phase::Idle;
        self.finished = true;
        Ok(())
    }

    pub fn retain(mut self, owner: T) {
        let mut state = self.ledger.state.lock().unwrap_or_else(|p| p.into_inner());
        state.phase = Phase::Faulted;
        if state.retained.is_some() {
            // The private, non-Clone Flight can only originate from an empty
            // Active slot, so collision is unreachable through this API. If a
            // future violation creates two physical owners, terminate without
            // dropping/reusing either, rather than lose/replace the original.
            std::process::abort();
        }
        state.retained = Some(owner);
        self.finished = true;
    }
}

impl<T> Drop for Flight<'_, T> {
    fn drop(&mut self) {
        if !self.finished {
            self.ledger
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .phase = Phase::Faulted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct ObservedOwner(Arc<AtomicUsize>);
    impl Drop for ObservedOwner {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn certificate_query_flight_is_busy_until_explicit_physical_release() {
        let ledger = FlightLedger::<ObservedOwner>::new();
        let flight = ledger.claim().unwrap();
        assert!(matches!(ledger.claim(), Err(CertificateQueryError::Busy)));
        // Logical cancellation does not itself call released().
        assert!(matches!(ledger.claim(), Err(CertificateQueryError::Busy)));
        flight.released().unwrap();
        ledger.claim().unwrap().released().unwrap();
    }

    #[test]
    fn certificate_query_failed_owner_is_retained_and_fault_is_permanent() {
        let drops = Arc::new(AtomicUsize::new(0));
        let ledger = FlightLedger::new();
        ledger
            .claim()
            .unwrap()
            .retain(ObservedOwner(Arc::clone(&drops)));
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        for _ in 0..3 {
            assert!(matches!(
                ledger.claim(),
                Err(CertificateQueryError::Faulted)
            ));
        }
        assert!(ledger.state.lock().unwrap().retained.is_some());
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        // Production's retained type is ManuallyDrop and its ledger is static;
        // this local model's owner is reclaimed only when the model ends.
        drop(ledger);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn certificate_query_poison_preserves_original_retained_owner_and_rejects_claim() {
        let drops = Arc::new(AtomicUsize::new(0));
        let ledger = FlightLedger::new();
        ledger
            .claim()
            .unwrap()
            .retain(ObservedOwner(Arc::clone(&drops)));
        let _ = std::panic::catch_unwind(|| {
            let _guard = ledger.state.lock().unwrap();
            panic!("synthetic metadata poison");
        });
        assert!(matches!(
            ledger.claim(),
            Err(CertificateQueryError::Faulted)
        ));
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let retained = match ledger.state.lock() {
            Err(poisoned) => poisoned.into_inner().retained.is_some(),
            Ok(_) => panic!("model must preserve poison"),
        };
        assert!(retained);
    }

    #[test]
    fn certificate_query_unfinished_claim_cannot_silently_publish_idle() {
        let ledger = FlightLedger::<ObservedOwner>::new();
        drop(ledger.claim().unwrap());
        assert!(matches!(
            ledger.claim(),
            Err(CertificateQueryError::Faulted)
        ));
    }
}
