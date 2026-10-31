//! Request scopes on the real Backend and sole SQLite writer. Synthetic key
//! material initializes an empty fixture; no authentication claim or KDF shortcut.
use super::*;
use fc_runtime::worker::{JobContext, JobError, Task, WorkerPool};
use std::sync::{atomic::AtomicUsize, mpsc};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Time;
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 8 * 3600).unwrap()
    }
}
impl MonotonicClock for Time {
    fn monotonic_millis(&self) -> u64 {
        1_000
    }
}
struct Fixture {
    backend: Backend,
    directory: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "fc-backend-execution-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let backend = Backend::open(
            directory.join("state.sqlite3"),
            CredentialRecordCodec::from_key([61; 32]).unwrap(),
            Arc::new(Time),
            Arc::new(Time),
        )
        .unwrap();
        Self { backend, directory }
    }
    fn healthy(&self) {
        assert!(self.backend.request_execution_fence.is_none());
        assert!(self.backend.state().unwrap().installation.is_none());
        assert!(self.backend.control.health_gate().check_sensitive().is_ok());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.backend.close().expect("execution scope writer join");
        std::fs::remove_dir_all(&self.directory).expect("execution scope database cleanup");
    }
}
fn cancelled<T>(result: Result<T>) {
    assert!(matches!(
        result,
        Err(BackendError::Storage(StorageError::ExecutionCancelled))
    ));
}

#[test]
fn cancelled_scope_restores_base_but_previously_cloned_repository_stays_cancelled() {
    let mut fixture = Fixture::new();
    let active = Arc::new(AtomicBool::new(true));
    let check = active.clone();
    let captured;
    {
        let scope = fixture
            .backend
            .begin_execution_scope(Arc::new(move || check.load(Ordering::Acquire)));
        assert!(scope.state().unwrap().installation.is_none());
        captured = scope.repository.clone();
        active.store(false, Ordering::Release);
        cancelled(scope.state());
        assert_eq!(
            scope.repository.load_identity(),
            Err(StorageError::ExecutionCancelled)
        );
    }
    fixture.healthy();
    assert_eq!(
        captured.load_identity(),
        Err(StorageError::ExecutionCancelled)
    );
    {
        let scope = fixture.backend.begin_execution_scope(Arc::new(|| true));
        assert!(scope.state().is_ok());
    }
    fixture.healthy();
}

#[test]
fn nested_scope_cannot_relax_outer_fence_and_drop_restores_exact_outer_scope() {
    let mut fixture = Fixture::new();
    {
        let mut outer = fixture.backend.begin_execution_scope(Arc::new(|| false));
        {
            let inner = outer.begin_execution_scope(Arc::new(|| true));
            cancelled(inner.state());
            assert_eq!(
                inner.repository.load_identity(),
                Err(StorageError::ExecutionCancelled)
            );
        }
        cancelled(outer.state());
    }
    fixture.healthy();
    {
        let mut outer = fixture.backend.begin_execution_scope(Arc::new(|| true));
        {
            let inner = outer.begin_execution_scope(Arc::new(|| false));
            cancelled(inner.state());
        }
        assert!(outer.request_execution_fence.is_some());
        assert!(outer.state().is_ok());
        assert!(outer.repository.load_identity().is_ok());
    }
    fixture.healthy();
}

#[test]
fn unwind_of_nested_request_scopes_restores_repository_and_health_without_clearing_outer_fence() {
    let mut fixture = Fixture::new();
    {
        let mut outer = fixture.backend.begin_execution_scope(Arc::new(|| false));
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _inner = outer.begin_execution_scope(Arc::new(|| true));
            panic!("diagnostic nested request unwind");
        }));
        assert!(panic.is_err());
        cancelled(outer.state());
        assert_eq!(
            outer.repository.load_identity(),
            Err(StorageError::ExecutionCancelled)
        );
    }
    fixture.healthy();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _scope = fixture.backend.begin_execution_scope(Arc::new(|| false));
        panic!("diagnostic request unwind");
    }));
    assert!(panic.is_err());
    fixture.healthy();
}

#[test]
fn panicking_memory_fence_cancels_only_request_and_does_not_poison_backend() {
    let mut fixture = Fixture::new();
    {
        let scope = fixture
            .backend
            .begin_execution_scope(Arc::new(|| panic!("diagnostic request fence")));
        cancelled(scope.global_revision());
        assert_eq!(
            scope.repository.load_identity(),
            Err(StorageError::ExecutionCancelled)
        );
        assert!(scope.control.health_gate().check_sensitive().is_ok());
    }
    fixture.healthy();
}

struct RequestWork {
    fixture: Arc<Mutex<Fixture>>,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    cancelled_read: Arc<AtomicUsize>,
    restored: Arc<AtomicUsize>,
}
impl Task for RequestWork {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(0)
    }
    fn run(self, context: &JobContext) {
        let mut fixture = self.fixture.lock().unwrap();
        {
            let scope = fixture
                .backend
                .begin_execution_scope(Arc::new(context.execution_fence()));
            assert!(scope.state().is_ok());
            self.entered.send(()).unwrap();
            let _ = self.release.recv_timeout(Duration::from_secs(3));
            if matches!(
                scope.state(),
                Err(BackendError::Storage(StorageError::ExecutionCancelled))
            ) {
                self.cancelled_read.fetch_add(1, Ordering::AcqRel);
            }
            if scope.repository.load_identity() == Err(StorageError::ExecutionCancelled) {
                self.cancelled_read.fetch_add(1, Ordering::AcqRel);
            }
        }
        fixture.healthy();
        self.restored.fetch_add(1, Ordering::AcqRel);
    }
}
#[test]
fn actual_shared_worker_cancellation_fences_backend_scope_and_restores_next_request() {
    let fixture = Arc::new(Mutex::new(Fixture::new()));
    let pool = WorkerPool::start().unwrap();
    let (entered, seen) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let cancelled_read = Arc::new(AtomicUsize::new(0));
    let restored = Arc::new(AtomicUsize::new(0));
    let ticket = match pool.handle().try_submit(
        RequestWork {
            fixture: fixture.clone(),
            entered,
            release: held,
            cancelled_read: cancelled_read.clone(),
            restored: restored.clone(),
        },
        Instant::now() + Duration::from_secs(10),
    ) {
        Ok(ticket) => ticket,
        Err(error) => panic!("unexpected request rejection {:?}", error.kind),
    };
    seen.recv_timeout(Duration::from_secs(3)).unwrap();
    ticket.cancel();
    release.send(()).unwrap();
    assert_eq!(ticket.wait(), Err(JobError::Cancelled));
    pool.join().unwrap();
    assert_eq!(cancelled_read.load(Ordering::Acquire), 2);
    assert_eq!(restored.load(Ordering::Acquire), 1);
    fixture.lock().unwrap().healthy();
}
