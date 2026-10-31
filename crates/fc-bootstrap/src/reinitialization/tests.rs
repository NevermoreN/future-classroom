use super::*;
use crate::recovery::test_support::{Fixture, QuietTask};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Barrier, Condvar,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Scratch {
    root: PathBuf,
    #[cfg(windows)]
    guard: Option<fc_windows::InstanceGuard>,
}
impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-ReinitializationCore-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        #[cfg(windows)]
        let guard = {
            let paths = fc_windows::system::DataPaths::isolated_test(root.clone()).unwrap();
            paths
                .prepare_for_current_user()
                .unwrap()
                .read_back()
                .unwrap();
            Some(fc_windows::InstanceGuard::acquire(&root).unwrap())
        };
        #[cfg(not(windows))]
        std::fs::create_dir_all(root.join("state")).unwrap();
        Self {
            root,
            #[cfg(windows)]
            guard,
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        #[cfg(windows)]
        self.guard.take();
        let removed = std::fs::remove_dir_all(&self.root);
        if !thread::panicking() {
            removed.unwrap();
        }
    }
}
#[derive(Default)]
struct Gate {
    released: Mutex<bool>,
    changed: Condvar,
    entered: AtomicBool,
}
impl Gate {
    fn hold(&self) {
        self.entered.store(true, Ordering::Release);
        let mut released = self
            .released
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while !*released {
            released = self
                .changed
                .wait(released)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
    fn release(&self) {
        *self
            .released
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.changed.notify_all();
    }
}
struct ReleaseOnDrop(Arc<Gate>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct Held(Arc<Gate>);
impl Task for Held {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(0)
    }
    fn run(self, _: &fc_runtime::worker::JobContext) {
        self.0.hold();
    }
}
fn eventually(predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "physical transition did not settle"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn finish<T: Task>(drain: &mut ColdReinitialization<T>) -> Result<ColdReinitializationPermit> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match drain.poll() {
            Ok(Some(permit)) => return Ok(permit),
            Err(error) => return Err(error),
            Ok(None) => {}
        }
        assert!(
            Instant::now() < deadline,
            "physical reinitialization drain did not finish"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn claimed_repair_startup_rejects_future_open_and_a_second_claim() {
    let scratch = Scratch::new();
    assert!(matches!(
        NeverOpenedProof::claim(&scratch.root, 0),
        Err(ReinitializationError::InvalidSequence)
    ));
    let proof = NeverOpenedProof::claim(&scratch.root, 7).unwrap();
    assert_eq!(proof.sequence(), 7);
    assert_eq!(proof.root(), scratch.root);
    assert_eq!(
        record_backend_open(&scratch.root.join("state/classroom.sqlite3")),
        Err(ReinitializationError::AlreadyClaimed)
    );
    assert!(matches!(
        NeverOpenedProof::claim(&scratch.root, 8),
        Err(ReinitializationError::AlreadyClaimed)
    ));
    #[cfg(windows)]
    {
        let alias = PathBuf::from(scratch.root.to_string_lossy().to_ascii_uppercase());
        assert_eq!(
            record_backend_open(&alias.join("STATE/CLASSROOM.SQLITE3")),
            Err(ReinitializationError::AlreadyClaimed)
        );
    }
    drop(proof);
}
#[test]
fn started_open_is_not_never_opened_even_before_database_creation() {
    let scratch = Scratch::new();
    record_backend_open(&scratch.root.join("state/classroom.sqlite3")).unwrap();
    assert!(!scratch.root.join("state/classroom.sqlite3").exists());
    assert!(matches!(
        NeverOpenedProof::claim(&scratch.root, 1),
        Err(ReinitializationError::AlreadyOpened)
    ));
}
#[test]
fn open_and_repair_claim_are_one_atomic_decision() {
    let scratch = Scratch::new();
    let barrier = Arc::new(Barrier::new(2));
    let claim_root = scratch.root.clone();
    let claim_barrier = barrier.clone();
    let claim = thread::spawn(move || {
        claim_barrier.wait();
        NeverOpenedProof::claim(&claim_root, 1).is_ok()
    });
    let open_root = scratch.root.clone();
    let open = thread::spawn(move || {
        barrier.wait();
        record_backend_open(&open_root.join("state/classroom.sqlite3")).is_ok()
    });
    assert_ne!(
        claim.join().unwrap(),
        open.join().unwrap(),
        "both branches acquired the same deployment"
    );
}
#[test]
fn blocked_worker_cannot_be_replaced_by_a_cancelled_ticket_or_a_reset_permit() {
    let scratch = Scratch::new();
    let proof = NeverOpenedProof::claim(&scratch.root, 3).unwrap();
    let gate = Arc::new(Gate::default());
    let _release = ReleaseOnDrop(gate.clone());
    let workers = WorkerPool::<Held>::start().unwrap();
    let ticket = workers
        .handle()
        .try_submit(Held(gate.clone()), Instant::now() + Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("submit held owner"));
    eventually(|| gate.entered.load(Ordering::Acquire));
    ticket.cancel();
    let mut drain = ColdReinitialization::begin(
        scratch.root.clone(),
        3,
        ReinitializationBackend::NeverOpened(proof),
        workers,
        None,
    );
    for _ in 0..3 {
        assert!(drain.poll().unwrap().is_none());
        assert!(drain.workers.is_some());
    }
    gate.release();
    let permit = finish(&mut drain).unwrap();
    assert_eq!(permit.root(), scratch.root);
    assert_eq!(permit.sequence(), 3);
    drop(permit);
}
#[test]
fn deployment_and_sequence_mismatch_close_owners_but_never_mint_a_permit() {
    for sequence_mismatch in [false, true] {
        let original = Scratch::new();
        let other = Scratch::new();
        let proof = NeverOpenedProof::claim(&original.root, 9).unwrap();
        let target = if sequence_mismatch {
            original.root.clone()
        } else {
            other.root.clone()
        };
        let sequence = if sequence_mismatch { 10 } else { 9 };
        let mut drain = ColdReinitialization::begin(
            target,
            sequence,
            ReinitializationBackend::NeverOpened(proof),
            WorkerPool::<QuietTask>::start().unwrap(),
            None,
        );
        assert!(matches!(
            finish(&mut drain),
            Err(ReinitializationError::WrongDeployment)
        ));
    }
}
#[test]
fn healthy_backend_revokes_admission_before_worker_drain_and_closes_writer_before_permit() {
    let fixture = Fixture::new();
    let control = fixture.backend.lock().unwrap().session_control();
    let gate = Arc::new(Gate::default());
    let _release = ReleaseOnDrop(gate.clone());
    let workers = WorkerPool::<Held>::start().unwrap();
    let _ticket = workers
        .handle()
        .try_submit(Held(gate.clone()), Instant::now() + Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("submit held owner"));
    eventually(|| gate.entered.load(Ordering::Acquire));
    let mut drain = ColdReinitialization::begin(
        fixture.root.clone(),
        4,
        ReinitializationBackend::Live(fixture.backend.clone()),
        workers,
        None,
    );
    assert!(control.is_local_locked());
    assert!(control.is_recovering());
    assert!(fixture.backend.lock().unwrap().role(fixture.lease).is_err());
    assert!(drain.poll().unwrap().is_none());
    gate.release();
    let permit = finish(&mut drain).unwrap();
    assert!(fixture.backend.lock().unwrap().state().is_err());
    assert_eq!(permit.sequence(), 4);
    drop(permit);
}
#[test]
fn poisoned_backend_retains_ownership_until_explicit_safe_close_and_never_grants_reset() {
    let fixture = Fixture::new();
    let poisoned = fixture.backend.clone();
    assert!(thread::spawn(move || {
        let _guard = poisoned.lock().unwrap();
        panic!("synthetic backend owner poison");
    })
    .join()
    .is_err());
    let mut drain = ColdReinitialization::begin(
        fixture.root.clone(),
        5,
        ReinitializationBackend::Live(fixture.backend.clone()),
        WorkerPool::<QuietTask>::start().unwrap(),
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while drain.workers.is_some() {
        assert!(drain.poll().unwrap().is_none());
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    assert!(drain.is_retained());
    assert!(drain.poll().unwrap().is_none());
    // Test-only recovery permits deterministic cleanup of this synthetic writer.
    // Production never clears poison or resumes ordinary admission.
    fixture.backend.clear_poison();
    fixture.backend.lock().unwrap().session_control().fault();
    assert!(matches!(
        finish(&mut drain),
        Err(ReinitializationError::Backend(BackendError::Closed))
    ));
}

#[cfg(windows)]
#[test]
fn live_reset_keeps_real_native_voice_cleanup_usable_until_its_thread_closes() {
    let fixture = Fixture::new();
    let service = fixture.backend.lock().unwrap().voice_service();
    assert!(service
        .runtime_handle()
        .inventory()
        .unwrap()
        .jobs
        .is_empty());
    let workers = WorkerPool::<QuietTask>::start().unwrap();
    let audio = VoiceRuntime::new(Arc::new(crate::voice_runtime::NativeFactory::new(
        service,
        fixture.root.clone(),
        Arc::new(fc_windows::system::CurrentClock::new()),
    )));
    // A real NativeDomain runs inventory/interrupt/controlled-file retirement.
    // There is no voice asset, queued speech, device monitor or native output.
    audio.wake().unwrap();
    let mut drain = ColdReinitialization::begin(
        fixture.root.clone(),
        12,
        ReinitializationBackend::Live(fixture.backend.clone()),
        workers,
        Some(audio),
    );
    assert!(fixture
        .backend
        .lock()
        .unwrap()
        .session_control()
        .is_recovering());
    let permit =
        finish(&mut drain).expect("real native voice cleanup must finish before writer close");
    let status = drain.audio.as_ref().unwrap().status().unwrap();
    assert_eq!(status.phase, RuntimePhase::Closed);
    assert!(!status.physical_thread_running);
    assert!(!status.faulted);
    assert!(status.physical_threads_started >= 1);
    assert!(fixture.backend.lock().unwrap().state().is_err());
    assert_eq!(permit.sequence(), 12);
    drop(permit);
}

#[cfg(windows)]
mod tls;
