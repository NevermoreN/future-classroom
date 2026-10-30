//! Composition for a non-atomic SQLite/Windows operation. This module runs on
//! the existing shared worker. It never replays an intent on startup or rolls
//! back an OS effect after an uncertain result.
use crate::backend::{Backend, BackendError};
use fc_application::identity::SessionLease;
use fc_domain::{autostart::*, ids::Revision};
use fc_ports::autostart::AutostartSystem;
use fc_runtime::worker::JobContext;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

static PHYSICAL_OWNER: AtomicBool = AtomicBool::new(false);
struct PhysicalOwner;
impl PhysicalOwner {
    fn acquire() -> Result<Self, Error> {
        PHYSICAL_OWNER
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| Error::Busy)
    }
}
impl Drop for PhysicalOwner {
    fn drop(&mut self) {
        PHYSICAL_OWNER.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Backend(BackendError),
    Busy,
    Cancelled,
    InvalidSystem,
    /// The OS may already have changed. Show the observation and require a
    /// fresh read/review; never retry or reverse the registry write implicitly.
    Unsettled {
        outcome: AutostartSystemOutcome,
        error: Box<BackendError>,
    },
}
impl From<BackendError> for Error {
    fn from(error: BackendError) -> Self {
        Self::Backend(error)
    }
}

pub enum Operation {
    Load,
    Change {
        expected: Revision,
        desired: bool,
        reviewed_target: String,
        previous_observed: AutostartObservation,
    },
    ObserveStartup,
}
pub struct Job {
    pub backend: Arc<Mutex<Backend>>,
    pub lease: Option<SessionLease>,
    pub operation: Operation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// None before installation initialization; no settings row is created.
    pub state: Option<AutostartState>,
    /// Fresh physical observation, kept separate from desired and stored target.
    pub observed: AutostartObservation,
    /// Bound by the trusted system adapter to this executable.
    pub target_command: String,
    pub operation_outcome: Option<AutostartSystemOutcome>,
}

impl Job {
    pub fn allocation_bytes(&self) -> Option<usize> {
        let extra = match &self.operation {
            Operation::Change {
                previous_observed,
                reviewed_target,
                ..
            } => previous_observed
                .allocation_bytes()
                .checked_add(reviewed_target.capacity())?,
            _ => 0,
        };
        std::mem::size_of::<Self>().checked_add(extra)
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Backend>, Error> {
        self.backend
            .lock()
            .map_err(|_| Error::Backend(BackendError::Closed))
    }
    pub fn run(
        self,
        context: &JobContext,
        system: &mut dyn AutostartSystem,
    ) -> Result<Loaded, Error> {
        context.checkpoint().map_err(|_| Error::Cancelled)?;
        // Every observer participates: reconciliation must not mark a live
        // physical Applying operation as interrupted.
        let _owner = PhysicalOwner::acquire()?;
        let target = system.target_command().to_owned();
        if !valid_autostart_target(&target) {
            return Err(Error::InvalidSystem);
        }
        match &self.operation {
            Operation::Load | Operation::ObserveStartup => {
                let current = {
                    let backend = self.lock()?;
                    if backend.state()?.installation.is_none() {
                        None
                    } else {
                        Some(match self.operation {
                            Operation::Load => {
                                backend.autostart_status(self.lease.ok_or(Error::Cancelled)?)?
                            }
                            _ => backend.autostart_reconciliation_status()?,
                        })
                    }
                };
                // Capture the journal revision before observing the OS. A
                // concurrent record change cannot be overwritten by old facts.
                let observed = system.observe();
                let state = if let Some(current) = current {
                    context.checkpoint().map_err(|_| Error::Cancelled)?;
                    Some(
                        self.lock()?
                            .reconcile_autostart(current.revision, observed.clone())?,
                    )
                } else {
                    None
                };
                Ok(Loaded {
                    state,
                    observed,
                    target_command: target,
                    operation_outcome: None,
                })
            }
            Operation::Change {
                expected,
                desired,
                reviewed_target,
                previous_observed,
            } => {
                // A fresh adapter may point to a moved executable. Its fixed
                // target must still be the exact command the user reviewed.
                if reviewed_target != &target {
                    return Err(Error::Backend(BackendError::Autostart(
                        fc_ports::autostart::AutostartRepositoryError::Stale,
                    )));
                }
                let lease = self.lease.ok_or(Error::Cancelled)?;
                let prepared = self.lock()?.begin_autostart_while(
                    lease,
                    *expected,
                    *desired,
                    target.clone(),
                    previous_observed.clone(),
                    Arc::new(context.execution_fence()),
                )?;
                // The lease, epoch and job fence are all checked again by the
                // writer immediately before marking this exact intent Applying.
                let applying = self.lock()?.mark_autostart_applying_while(
                    lease,
                    prepared.ticket.clone(),
                    Arc::new(context.execution_fence()),
                );
                if applying.is_err() {
                    let outcome = AutostartSystemOutcome {
                        observation: system.observe(),
                        failure: Some(AutostartFailure::Cancelled),
                        mutated: false,
                    };
                    return self.settle(prepared.ticket, outcome, target);
                }
                let guard = self.lock()?.autostart_effect_guard(
                    lease,
                    &prepared.ticket,
                    Arc::new(context.execution_fence()),
                );
                let mut outcome = match guard {
                    Ok(guard) => {
                        // No Backend or SQLite lock is held across this physical
                        // call. The adapter rechecks both preimage and permission
                        // immediately before the native mutating API.
                        let mut physical =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                system.apply(previous_observed, *desired, &|| guard.is_valid())
                            }))
                            .unwrap_or({
                                AutostartSystemOutcome {
                                    observation: AutostartObservation::Unavailable(
                                        AutostartFailure::Interrupted,
                                    ),
                                    failure: Some(AutostartFailure::Interrupted),
                                    mutated: true,
                                }
                            });
                        // Cancellation/revocation during the syscall cannot
                        // undo an effect. Settle its factual outcome as uncertain.
                        if physical.failure.is_none() && !guard.is_valid() {
                            physical.failure = Some(AutostartFailure::Cancelled);
                        }
                        physical
                    }
                    Err(_) => AutostartSystemOutcome {
                        observation: system.observe(),
                        failure: Some(AutostartFailure::Cancelled),
                        mutated: false,
                    },
                };
                if !outcome.observation.validate() {
                    outcome.observation =
                        AutostartObservation::Unavailable(AutostartFailure::InvalidValue);
                    outcome.failure = Some(AutostartFailure::InvalidValue);
                }
                self.settle(prepared.ticket, outcome, target)
            }
        }
    }
    fn settle(
        &self,
        ticket: AutostartTicket,
        outcome: AutostartSystemOutcome,
        target: String,
    ) -> Result<Loaded, Error> {
        // Neither lease expiry nor job cancellation suppresses factual settlement.
        let mut backend = self.backend.lock().map_err(|_| Error::Unsettled {
            outcome: outcome.clone(),
            error: Box::new(BackendError::Closed),
        })?;
        let state = backend
            .finish_autostart(ticket, outcome.clone())
            .map_err(|error| Error::Unsettled {
                outcome: outcome.clone(),
                error: Box::new(error),
            })?;
        Ok(Loaded {
            state: Some(state),
            observed: outcome.observation.clone(),
            target_command: target,
            operation_outcome: Some(outcome),
        })
    }
}
