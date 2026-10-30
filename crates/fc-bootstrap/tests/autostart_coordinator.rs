//! Genuine bounded worker and SQLite; a private synthetic system port. No
//! registry, network, machine clock or global input is touched by these tests.
use fc_application::identity::SessionLease;
use fc_bootstrap::{
    autostart::{Error, Job, Loaded, Operation},
    backend::{Backend, BackendError, Initialize},
};
use fc_domain::{autostart::*, time::*};
use fc_ports::{
    autostart::{AutostartRepositoryError, AutostartSystem},
    Clock, MonotonicClock,
};
use fc_runtime::worker::{JobContext, JobTicket, Task, WorkerPool};
use fc_security::{CredentialRecordCodec, Password};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
static SERIAL: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);
const TARGET: &str = "\"C:\\Synthetic classroom\\FutureClassroom.exe\" --autostart";
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "FutureClassroom-AutostartCoordinator-R21-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let r = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            r.unwrap();
        }
    }
}
struct Time(AtomicU64);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 28800).unwrap()
    }
}
impl MonotonicClock for Time {
    fn monotonic_millis(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}
#[derive(Clone, Default)]
enum Mode {
    #[default]
    Normal,
    FailBefore,
    FailAfter,
    ExpireBefore,
    LockAfter,
    CloseAfter,
    AdvanceJournalDuringObserve,
    Wait {
        started: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
    },
}
#[derive(Clone)]
struct System {
    target: String,
    backend: Arc<Mutex<Backend>>,
    clock: Arc<Time>,
    observed: Arc<Mutex<AutostartObservation>>,
    calls: Arc<AtomicU64>,
    writes: Arc<AtomicU64>,
    mode: Mode,
}
impl AutostartSystem for System {
    fn target_command(&self) -> &str {
        &self.target
    }
    fn observe(&self) -> AutostartObservation {
        assert!(
            self.backend.try_lock().is_ok(),
            "Backend mutex held across physical observation"
        );
        if matches!(self.mode, Mode::AdvanceJournalDuringObserve) {
            let mut backend = self.backend.lock().unwrap();
            let current = backend.autostart_reconciliation_status().unwrap();
            backend
                .reconcile_autostart(
                    current.revision,
                    AutostartObservation::Command("external journal fact".into()),
                )
                .unwrap();
        }
        self.observed.lock().unwrap().clone()
    }
    fn apply(
        &mut self,
        expected: &AutostartObservation,
        enabled: bool,
        fresh: &dyn Fn() -> bool,
    ) -> AutostartSystemOutcome {
        assert!(
            self.backend.try_lock().is_ok(),
            "Backend mutex held across physical effect"
        );
        self.calls.fetch_add(1, Ordering::AcqRel);
        if matches!(self.mode, Mode::ExpireBefore) {
            self.clock.0.store(86_400_001, Ordering::Release);
        }
        let observation = self.observe();
        let failure = if &observation != expected {
            Some(AutostartFailure::PreimageChanged)
        } else if !fresh() {
            Some(AutostartFailure::Cancelled)
        } else if matches!(self.mode, Mode::FailBefore) {
            Some(AutostartFailure::AccessDenied)
        } else {
            None
        };
        if failure.is_some() {
            return AutostartSystemOutcome {
                observation,
                failure,
                mutated: false,
            };
        }
        *self.observed.lock().unwrap() = if enabled {
            AutostartObservation::Command(self.target.clone())
        } else {
            AutostartObservation::Absent
        };
        self.writes.fetch_add(1, Ordering::AcqRel);
        match &self.mode {
            Mode::LockAfter => self.backend.lock().unwrap().session_control().lock_local(),
            Mode::CloseAfter => self.backend.lock().unwrap().close().unwrap(),
            Mode::Wait { started, release } => {
                started.store(true, Ordering::Release);
                while !release.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
            }
            _ => {}
        }
        AutostartSystemOutcome {
            observation: self.observe(),
            failure: matches!(self.mode, Mode::FailAfter)
                .then_some(AutostartFailure::ReadbackMismatch),
            mutated: true,
        }
    }
}
type Answer = Arc<Mutex<Option<Result<Loaded, Error>>>>;
struct Work {
    job: Job,
    system: System,
    answer: Answer,
}
impl Task for Work {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        self.job.allocation_bytes()
    }
    fn run(mut self, context: &JobContext) {
        *self.answer.lock().unwrap() = Some(self.job.run(context, &mut self.system));
    }
}
struct Fixture {
    backend: Arc<Mutex<Backend>>,
    clock: Arc<Time>,
    lease: SessionLease,
    system: System,
    pool: Option<WorkerPool<Work>>,
    directory: Directory,
}
impl Fixture {
    fn new() -> Self {
        let directory = Directory::new();
        let clock = Arc::new(Time(AtomicU64::new(0)));
        let mut backend = Backend::open(
            directory.0.join("state.sqlite3"),
            CredentialRecordCodec::from_key([81; 32]).unwrap(),
            clock.clone(),
            clock.clone(),
        )
        .unwrap();
        let password = || Password::new("Synthetic-autostart-coordinator-2026".into()).unwrap();
        drop(
            backend
                .initialize(
                    Initialize {
                        school_name: "Synthetic autostart".into(),
                        login_name: "admin".into(),
                        display_name: "Synthetic admin".into(),
                    },
                    password(),
                )
                .unwrap(),
        );
        let lease = backend.login("admin", password()).unwrap();
        let backend = Arc::new(Mutex::new(backend));
        let system = System {
            target: TARGET.into(),
            backend: backend.clone(),
            clock: clock.clone(),
            observed: Arc::new(Mutex::new(AutostartObservation::Absent)),
            calls: Arc::new(AtomicU64::new(0)),
            writes: Arc::new(AtomicU64::new(0)),
            mode: Mode::Normal,
        };
        Self {
            backend,
            clock,
            lease,
            system,
            pool: Some(WorkerPool::start().unwrap()),
            directory,
        }
    }
    fn submit(&self, operation: Operation, mode: Mode) -> (JobTicket<()>, Answer) {
        let answer = Arc::new(Mutex::new(None));
        let mut system = self.system.clone();
        system.mode = mode;
        let job = Job {
            backend: self.backend.clone(),
            lease: Some(self.lease),
            operation,
        };
        let ticket = self
            .pool
            .as_ref()
            .unwrap()
            .handle()
            .try_submit(
                Work {
                    job,
                    system,
                    answer: answer.clone(),
                },
                Instant::now() + Duration::from_secs(15),
            )
            .unwrap_or_else(|e| panic!("submit {:?}", e.kind));
        (ticket, answer)
    }
    fn result(ticket: JobTicket<()>, answer: Answer) -> Result<Loaded, Error> {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !ticket.is_finished() {
            assert!(
                Instant::now() < deadline,
                "worker did not physically return"
            );
            std::thread::park_timeout(Duration::from_millis(1));
        }
        let _terminal = ticket.wait();
        let r = answer.lock().unwrap().take().unwrap();
        r
    }
    fn run(&self, operation: Operation, mode: Mode) -> Result<Loaded, Error> {
        let (t, a) = self.submit(operation, mode);
        Self::result(t, a)
    }
    fn load(&self) -> Loaded {
        self.run(Operation::Load, Mode::Normal).unwrap()
    }
    fn change(&self, desired: bool, mode: Mode) -> Result<Loaded, Error> {
        let current = self.load();
        self.run(
            Operation::Change {
                reviewed_target: current.target_command,
                expected: current.state.unwrap().revision,
                desired,
                previous_observed: current.observed,
            },
            mode,
        )
    }
    fn status(&self) -> AutostartState {
        self.backend
            .lock()
            .unwrap()
            .autostart_reconciliation_status()
            .unwrap()
    }
    fn reopen(&mut self) {
        let backend = Backend::open(
            self.directory.0.join("state.sqlite3"),
            CredentialRecordCodec::from_key([81; 32]).unwrap(),
            self.clock.clone(),
            self.clock.clone(),
        )
        .unwrap();
        *self.backend.lock().unwrap() = backend;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            let r = pool.join();
            if !std::thread::panicking() {
                r.unwrap();
            }
        }
        let r = self.backend.lock().unwrap().close();
        if !std::thread::panicking() {
            r.unwrap();
        }
    }
}
#[test]
fn enable_disable_and_old_path_repair_are_explicit_and_release_backend_mutex() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    *f.system.observed.lock().unwrap() = AutostartObservation::Command(
        "\"C:\\Old version\\FutureClassroom.exe\" --autostart".into(),
    );
    let loaded = f.load();
    assert_eq!(f.system.writes.load(Ordering::Acquire), 0);
    assert_ne!(
        loaded.observed,
        AutostartObservation::Command(TARGET.into())
    );
    let enabled = f.change(true, Mode::Normal).unwrap();
    assert_eq!(
        enabled.state.unwrap().current().unwrap().phase,
        AutostartPhase::Succeeded
    );
    let disabled = f.change(false, Mode::Normal).unwrap();
    assert_eq!(disabled.observed, AutostartObservation::Absent);
    assert_eq!(f.system.writes.load(Ordering::Acquire), 2);
}
#[test]
fn failures_before_and_after_effect_preserve_separate_intent_and_observation() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    let failed = f.change(true, Mode::FailBefore).unwrap().state.unwrap();
    assert!(failed.desired);
    assert_eq!(failed.observed, AutostartObservation::Absent);
    assert_eq!(failed.current().unwrap().phase, AutostartPhase::Failed);
    assert_eq!(f.system.writes.load(Ordering::Acquire), 0);
    let uncertain = f.change(true, Mode::FailAfter).unwrap().state.unwrap();
    assert_eq!(
        uncertain.current().unwrap().phase,
        AutostartPhase::NeedsReconcile
    );
    assert_eq!(f.system.writes.load(Ordering::Acquire), 1);
    f.load();
    assert_eq!(f.system.writes.load(Ordering::Acquire), 1);
}
#[test]
fn expired_lease_before_effect_blocks_write_but_exact_ticket_can_settle() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    let loaded = f.change(true, Mode::ExpireBefore).unwrap();
    assert_eq!(f.system.writes.load(Ordering::Acquire), 0);
    let state = loaded.state.unwrap();
    assert_eq!(state.current().unwrap().phase, AutostartPhase::Failed);
    assert_eq!(
        state.current().unwrap().failure,
        Some(AutostartFailure::Cancelled)
    );
    assert_eq!(f.status(), state);
}
#[test]
fn revocation_during_effect_records_uncertainty_without_rollback() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    let loaded = f.change(true, Mode::LockAfter).unwrap();
    let state = loaded.state.unwrap();
    assert_eq!(
        state.current().unwrap().phase,
        AutostartPhase::NeedsReconcile
    );
    assert_eq!(
        state.current().unwrap().failure,
        Some(AutostartFailure::Cancelled)
    );
    assert_eq!(f.system.writes.load(Ordering::Acquire), 1);
    assert_eq!(
        f.system.observe(),
        AutostartObservation::Command(TARGET.into())
    );
}
#[test]
fn finalize_failure_remains_unsettled_and_startup_only_observes() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut f = Fixture::new();
    let error = f.change(true, Mode::CloseAfter).unwrap_err();
    assert!(matches!(
        error,
        Error::Unsettled {
            outcome: AutostartSystemOutcome { mutated: true, .. },
            error,
        } if *error == BackendError::Closed
    ));
    f.reopen();
    let loaded = f.run(Operation::ObserveStartup, Mode::Normal).unwrap();
    assert_eq!(
        loaded.observed,
        AutostartObservation::Command(TARGET.into())
    );
    assert!(!loaded.state.unwrap().current().unwrap().phase.pending());
    assert_eq!(f.system.writes.load(Ordering::Acquire), 1);
}
#[test]
fn stale_review_and_completion_cannot_overwrite_newer_intent() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    let initial = f.load();
    let enabled = f.change(true, Mode::Normal).unwrap();
    let ticket = enabled
        .state
        .as_ref()
        .unwrap()
        .current()
        .unwrap()
        .ticket
        .clone();
    let stale = f
        .run(
            Operation::Change {
                reviewed_target: initial.target_command,
                expected: initial.state.unwrap().revision,
                desired: false,
                previous_observed: initial.observed,
            },
            Mode::Normal,
        )
        .unwrap_err();
    assert!(matches!(
        stale,
        Error::Backend(BackendError::Autostart(AutostartRepositoryError::Stale))
    ));
    let disabled = f.change(false, Mode::Normal).unwrap().state.unwrap();
    assert!(matches!(
        f.backend
            .lock()
            .unwrap()
            .autostart_effect_guard(f.lease, &ticket, Arc::new(|| true)),
        Err(BackendError::Autostart(AutostartRepositoryError::Stale))
    ));
    let old = f
        .backend
        .lock()
        .unwrap()
        .finish_autostart(ticket, enabled.operation_outcome.unwrap());
    assert!(matches!(
        old,
        Err(BackendError::Autostart(AutostartRepositoryError::Stale))
    ));
    assert_eq!(f.status(), disabled);
    assert_eq!(f.system.writes.load(Ordering::Acquire), 2);
}
#[test]
fn external_policy_removal_is_drift_and_never_auto_repaired() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    f.change(true, Mode::Normal).unwrap();
    *f.system.observed.lock().unwrap() = AutostartObservation::Absent;
    let loaded = f.load();
    let state = loaded.state.unwrap();
    assert!(state.desired && state.drifted());
    assert_eq!(f.system.writes.load(Ordering::Acquire), 1);
    assert_eq!(f.system.calls.load(Ordering::Acquire), 1);
}
#[test]
fn cancellation_does_not_release_physical_gate_before_native_return() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    let initial = f.load();
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let (ticket, answer) = f.submit(
        Operation::Change {
            reviewed_target: initial.target_command,
            expected: initial.state.unwrap().revision,
            desired: true,
            previous_observed: initial.observed,
        },
        Mode::Wait {
            started: started.clone(),
            release: release.clone(),
        },
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !started.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    ticket.cancel();
    assert_eq!(
        f.run(Operation::ObserveStartup, Mode::Normal),
        Err(Error::Busy)
    );
    assert!(!ticket.is_finished());
    release.store(true, Ordering::Release);
    let loaded = Fixture::result(ticket, answer).unwrap();
    assert_eq!(
        loaded.state.unwrap().current().unwrap().phase,
        AutostartPhase::NeedsReconcile
    );
    assert_eq!(f.system.writes.load(Ordering::Acquire), 1);
    f.load();
}
#[test]
fn startup_prepared_record_is_interrupted_without_replaying_registry() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut f = Fixture::new();
    let current = f.load();
    f.backend
        .lock()
        .unwrap()
        .begin_autostart_while(
            f.lease,
            current.state.unwrap().revision,
            true,
            TARGET.into(),
            current.observed,
            Arc::new(|| true),
        )
        .unwrap();
    f.backend.lock().unwrap().close().unwrap();
    f.reopen();
    let loaded = f.run(Operation::ObserveStartup, Mode::Normal).unwrap();
    let state = loaded.state.unwrap();
    assert!(state.desired);
    assert_eq!(
        state.current().unwrap().phase,
        AutostartPhase::NeedsReconcile
    );
    assert_eq!(
        state.current().unwrap().failure,
        Some(AutostartFailure::Interrupted)
    );
    assert_eq!(f.system.writes.load(Ordering::Acquire), 0);
}
#[test]
fn late_observation_cannot_overwrite_a_newer_journal_revision() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    assert!(matches!(
        f.run(Operation::ObserveStartup, Mode::AdvanceJournalDuringObserve),
        Err(Error::Backend(BackendError::Autostart(
            AutostartRepositoryError::Stale
        )))
    ));
    assert_eq!(
        f.status().observed,
        AutostartObservation::Command("external journal fact".into())
    );
    assert_eq!(f.system.writes.load(Ordering::Acquire), 0);
}
#[test]
fn changed_physical_preimage_requires_a_new_review_without_overwriting_it() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    let current = f.load();
    let external = AutostartObservation::Command("external replacement".into());
    *f.system.observed.lock().unwrap() = external.clone();
    let loaded = f
        .run(
            Operation::Change {
                reviewed_target: current.target_command,
                expected: current.state.unwrap().revision,
                desired: true,
                previous_observed: current.observed,
            },
            Mode::Normal,
        )
        .unwrap();
    assert_eq!(loaded.observed, external);
    assert_eq!(
        loaded.state.unwrap().current().unwrap().failure,
        Some(AutostartFailure::PreimageChanged)
    );
    assert_eq!(f.system.writes.load(Ordering::Acquire), 0);
}

#[test]
fn startup_before_initialization_only_reads_the_fixed_system_item() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = Directory::new();
    let clock = Arc::new(Time(AtomicU64::new(0)));
    let backend = Arc::new(Mutex::new(
        Backend::open(
            directory.0.join("state.sqlite3"),
            CredentialRecordCodec::from_key([82; 32]).unwrap(),
            clock.clone(),
            clock.clone(),
        )
        .unwrap(),
    ));
    let before = backend.lock().unwrap().global_revision().unwrap();
    let system = System {
        target: TARGET.into(),
        backend: backend.clone(),
        clock,
        observed: Arc::new(Mutex::new(AutostartObservation::Absent)),
        calls: Arc::new(AtomicU64::new(0)),
        writes: Arc::new(AtomicU64::new(0)),
        mode: Mode::Normal,
    };
    let answer = Arc::new(Mutex::new(None));
    let pool = WorkerPool::start().unwrap();
    let ticket = pool
        .handle()
        .try_submit(
            Work {
                job: Job {
                    backend: backend.clone(),
                    lease: None,
                    operation: Operation::ObserveStartup,
                },
                system: system.clone(),
                answer: answer.clone(),
            },
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap_or_else(|e| panic!("submit {:?}", e.kind));
    let loaded = Fixture::result(ticket, answer).unwrap();
    assert_eq!(loaded.state, None);
    assert_eq!(loaded.observed, AutostartObservation::Absent);
    assert_eq!(backend.lock().unwrap().global_revision().unwrap(), before);
    assert_eq!(system.writes.load(Ordering::Acquire), 0);
    pool.join().unwrap();
    backend.lock().unwrap().close().unwrap();
}

#[test]
fn moved_executable_after_review_requires_a_new_review_before_persisting_intent() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut f = Fixture::new();
    let reviewed = f.load();
    let before = f.status();
    f.system.target = "\"C:\\Relocated classroom\\FutureClassroom.exe\" --autostart".into();
    let result = f.run(
        Operation::Change {
            expected: reviewed.state.unwrap().revision,
            desired: true,
            reviewed_target: reviewed.target_command,
            previous_observed: reviewed.observed,
        },
        Mode::Normal,
    );
    assert!(matches!(
        result,
        Err(Error::Backend(BackendError::Autostart(
            AutostartRepositoryError::Stale
        )))
    ));
    assert_eq!(
        f.status(),
        before,
        "unreviewed target must not create a journal intent"
    );
    assert_eq!(f.system.calls.load(Ordering::Acquire), 0);
    assert_eq!(f.system.writes.load(Ordering::Acquire), 0);
    assert_eq!(f.system.observe(), AutostartObservation::Absent);
}
