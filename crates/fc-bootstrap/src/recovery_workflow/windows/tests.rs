use super::*;
use crate::recovery::test_support::{Fixture, QuietFactory, QuietTask};
use std::{path::PathBuf, time::Instant};

fn preview(fixture: &Fixture) -> (RecoveryWorkflow, ReviewBundle) {
    let paths = DataPaths::isolated_test(fixture.root.clone()).unwrap();
    // The lightweight backend fixture does not perform Work::Open's content
    // namespace startup. Establish/verify the same live third tree before any
    // export; cold replacement must continue to reject a genuinely missing tree.
    let content_files = crate::profile_content_files::ControlledContentFiles::new(&fixture.root)
        .expect("initialize the production content namespace");
    crate::profile_content_startup::reconcile_after_restart(
        &fixture.backend.lock().unwrap(),
        &content_files,
    )
    .expect("verify the empty deployment's production content startup");
    drop(content_files);
    let workflow = RecoveryWorkflow::new(paths);
    let revision = fixture.revision();
    let access = fixture
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(fixture.lease, revision)
        .unwrap();
    let exported = workflow.export(access, Arc::new(|| true)).unwrap();
    std::fs::copy(
        exported.path,
        fixture.root.join("quarantine/workflow.fcbak"),
    )
    .unwrap();
    let revision = fixture.revision();
    let access = fixture
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(fixture.lease, revision)
        .unwrap();
    let bundle = workflow
        .preflight(
            SelectedFile::isolated_probe(&fixture.root, "workflow.fcbak").unwrap(),
            access,
            Arc::new(|| true),
        )
        .unwrap();
    assert_eq!(bundle.summary().material_count, 0);
    assert!(bundle.summary().document_count > 0);
    assert!(bundle.summary().database_bytes > 0);
    (workflow, bundle)
}
fn operation_root(workflow: &RecoveryWorkflow) -> PathBuf {
    workflow
        .slot
        .inspect(|_, payload| match payload.unwrap() {
            Payload::Preview { prepared, .. } | Payload::Confirmed { prepared, .. } => {
                prepared.ordinary_database().parent().unwrap().to_owned()
            }
        })
        .unwrap()
}
fn finish<T: Task>(operation: &mut RestoreOperation<T>) -> RestoreStatus {
    let until = Instant::now() + Duration::from_secs(30);
    while !operation.reap_nonblocking() {
        assert!(Instant::now() < until, "{:?}", operation.status());
        thread::sleep(Duration::from_millis(5));
    }
    operation.status()
}
#[test]
fn abandoned_review_is_only_removed_by_the_existing_file_worker() {
    let fixture = Fixture::new();
    let (workflow, bundle) = preview(&fixture);
    let operation = operation_root(&workflow);
    drop(bundle);
    assert!(operation.exists());
    assert!(!workflow.is_idle());
    assert!(workflow.retirement_pending());
    workflow.request_close();
    let worker = thread::spawn(move || {
        assert!(workflow.retire_pending_on_worker());
        assert!(workflow.is_idle());
    });
    worker.join().unwrap();
    assert!(!operation.exists());
}
#[test]
fn lost_confirmation_notification_remains_claimable_and_commits_after_real_drain() {
    let fixture = Fixture::new();
    let (workflow, bundle) = preview(&fixture);
    let confirmed = workflow
        .confirm(
            bundle,
            fixture.backend.clone(),
            fixture.lease,
            Arc::new(|| true),
        )
        .unwrap();
    drop(confirmed); // Models a stale/cancelled worker ticket dropping its output.
    assert!(workflow.has_confirmed());
    assert!(!workflow.retire_pending_on_worker());
    let coordinator = PreparedCoordinator::<QuietTask>::prepare().unwrap();
    let pool = WorkerPool::start().unwrap();
    let audio = VoiceRuntime::new(Arc::new(QuietFactory));
    let mut operation = coordinator.start(
        workflow.confirmed_bundle().unwrap(),
        fixture.backend.clone(),
        pool,
        audio,
    );
    let status = finish(&mut operation);
    assert_eq!(status.phase, RestorePhase::Finished);
    let receipt = status
        .result
        .expect("physically finished coordinator must publish its terminal result")
        .expect("the retained confirmation must complete the real cold restore");
    assert!(receipt.generation > 0);
    assert!(workflow.is_idle());
    assert!(fixture.backend.lock().unwrap().state().is_err());
    let paths = DataPaths::isolated_test(fixture.root.clone()).unwrap();
    let files = RecoveryFiles::new(&paths).unwrap();
    assert_eq!(files.reconcile_startup(&|| true).unwrap().installed, 1);
    *fixture.backend.lock().unwrap() = fixture.reopen(&paths.database());
    assert!(fixture.backend.lock().unwrap().state().is_ok());
}
struct HeldTask {
    started: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}
struct ReleaseOnDrop(Arc<AtomicBool>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
impl Task for HeldTask {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(0)
    }
    fn run(self, _: &fc_runtime::worker::JobContext) {
        self.started.store(true, Ordering::Release);
        while !self.release.load(Ordering::Acquire) {
            thread::park_timeout(Duration::from_millis(5));
        }
    }
}
#[test]
fn cancelling_cold_restore_keeps_real_worker_and_source_pin_until_it_exits() {
    let fixture = Fixture::new();
    let (workflow, bundle) = preview(&fixture);
    let root = operation_root(&workflow);
    let confirmed = workflow
        .confirm(
            bundle,
            fixture.backend.clone(),
            fixture.lease,
            Arc::new(|| true),
        )
        .unwrap();
    let pool = WorkerPool::<HeldTask>::start().unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let _release_on_failure = ReleaseOnDrop(release.clone());
    let _ticket = pool
        .handle()
        .try_submit(
            HeldTask {
                started: started.clone(),
                release: release.clone(),
            },
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap_or_else(|_| panic!("submit"));
    let until = Instant::now() + Duration::from_secs(15);
    while !started.load(Ordering::Acquire) {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    let coordinator = PreparedCoordinator::<HeldTask>::prepare().unwrap();
    coordinator.state.cancel.store(true, Ordering::Release);
    let mut operation = coordinator.start(
        confirmed,
        fixture.backend.clone(),
        pool,
        VoiceRuntime::new(Arc::new(QuietFactory)),
    );
    let until = Instant::now() + Duration::from_secs(15);
    while operation.status().phase == RestorePhase::Waiting {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    assert!(!operation.reap_nonblocking());
    assert!(!workflow.is_idle());
    assert!(root.exists());
    assert!(fixture
        .backend
        .lock()
        .unwrap()
        .session_control()
        .is_recovering());
    release.store(true, Ordering::Release);
    let status = finish(&mut operation);
    assert!(status.result.unwrap().is_err());
    assert!(workflow.is_idle());
    assert!(!root.exists());
    assert!(fixture.backend.lock().unwrap().state().is_err());
    let paths = DataPaths::isolated_test(fixture.root.clone()).unwrap();
    *fixture.backend.lock().unwrap() = fixture.reopen(&paths.database());
    assert!(fixture.backend.lock().unwrap().state().is_ok());
}
#[test]
fn panicked_coordinator_never_publishes_a_shutdown_completion() {
    let state = Arc::new(CoordinatorState::<QuietTask> {
        input: Mutex::new(None),
        wake: Condvar::new(),
        aborted: AtomicBool::new(false),
        panicked: AtomicBool::new(false),
        cancel: AtomicBool::new(false),
        status: Mutex::new(RestoreStatus {
            phase: RestorePhase::Draining,
            cancellation_requested: false,
            result: None,
        }),
    });
    let handle = thread::spawn(|| panic!("synthetic coordinator fault before physical proof"));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !crate::physical_thread::is_finished(&handle) {
        assert!(
            Instant::now() < deadline,
            "panicked Windows thread did not physically terminate"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let mut operation = RestoreOperation {
        state,
        thread: Some(handle),
    };
    for _ in 0..3 {
        assert!(!operation.reap_nonblocking());
        assert_eq!(operation.status().phase, RestorePhase::Retained);
        assert!(operation.status().result.is_none());
    }
}
#[test]
fn abort_without_restore_capability_waits_for_the_real_worker_and_keeps_original_data() {
    let fixture = Fixture::new();
    let (_, confirmed) = fixture.confirm();
    drop(confirmed); // Reproduce irreversible recovering state without a cap.
    assert!(fixture
        .backend
        .lock()
        .unwrap()
        .session_control()
        .is_recovering());
    let pool = WorkerPool::<HeldTask>::start().unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let _release_on_failure = ReleaseOnDrop(release.clone());
    let _ticket = pool
        .handle()
        .try_submit(
            HeldTask {
                started: started.clone(),
                release: release.clone(),
            },
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap_or_else(|_| panic!("submit"));
    let until = Instant::now() + Duration::from_secs(15);
    while !started.load(Ordering::Acquire) {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    let coordinator = PreparedCoordinator::<HeldTask>::prepare().unwrap();
    let mut operation = coordinator.start_abort_original(
        fixture.backend.clone(),
        pool,
        VoiceRuntime::new(Arc::new(QuietFactory)),
    );
    let until = Instant::now() + Duration::from_secs(15);
    while operation.status().phase == RestorePhase::Waiting {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    assert!(!operation.reap_nonblocking());
    assert!(operation.status().result.is_none());
    release.store(true, Ordering::Release);
    let status = finish(&mut operation);
    assert_eq!(status.phase, RestorePhase::Finished);
    assert_eq!(status.result, Some(Err(WorkflowError::Cancelled)));
    let paths = DataPaths::isolated_test(fixture.root.clone()).unwrap();
    *fixture.backend.lock().unwrap() = fixture.reopen(&paths.database());
    let mut backend = fixture.backend.lock().unwrap();
    assert!(backend
        .login("admin", crate::recovery::test_support::password())
        .is_ok());
    assert!(backend.state().unwrap().installation.is_some());
    let recovery = fixture.root.join("recovery");
    assert!(!recovery.exists() || std::fs::read_dir(recovery).unwrap().next().is_none());
}
