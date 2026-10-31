use super::*;
use std::sync::atomic::AtomicU64;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fc-execution-fence-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("execution fence database cleanup");
    }
}
#[derive(Default)]
struct State {
    published: AtomicU64,
    rollbacks: AtomicUsize,
    faulted: AtomicBool,
}
struct Publisher(Arc<State>);
impl AuthorityPublisher for Publisher {
    type Snapshot = u64;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, snapshot: &u64) -> Revision {
        Revision::new(*snapshot)
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        Ok(())
    }
    fn publish(&mut self, _: (), snapshot: u64) -> Result<(), ()> {
        self.0.published.store(snapshot, Ordering::Release);
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        self.0.rollbacks.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn fault(&mut self) {
        self.0.faulted.store(true, Ordering::Release);
    }
}
struct Operation<F>(F);
impl<F> TransactionTask<u64> for Operation<F>
where
    F: FnOnce(&dyn TransactionDocuments) -> Result<TransactionDecision<u64>, StorageError> + Send,
{
    fn allocation_bytes(&self) -> usize {
        0
    }
    fn evaluate(
        self: Box<Self>,
        documents: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<u64>, StorageError> {
        (self.0)(documents)
    }
}
fn read(outcome: &[u8]) -> TransactionDecision<u64> {
    TransactionDecision {
        precommit: None,
        outcome: outcome.to_vec(),
        commit: None,
    }
}
fn write(documents: &dyn TransactionDocuments) -> Result<TransactionDecision<u64>, StorageError> {
    let next = documents.revision()?.next().unwrap().get();
    Ok(TransactionDecision {
        precommit: None,
        outcome: b"write-complete".to_vec(),
        commit: Some((
            vec![Mutation::Put {
                key: "fenced".into(),
                value: b"owned-value".to_vec(),
            }],
            next,
        )),
    })
}
fn fence(active: &Arc<AtomicBool>) -> TransactionExecutionFence {
    let active = active.clone();
    Arc::new(move || active.load(Ordering::Acquire))
}
fn healthy(handle: &StorageHandle<u64>, state: &State) {
    assert!(!state.faulted.load(Ordering::Acquire));
    assert_eq!(
        handle
            .try_transaction(Operation(|_: &dyn TransactionDocuments| Ok(read(
                b"healthy"
            ))))
            .unwrap()
            .wait()
            .unwrap(),
        b"healthy"
    );
}

#[test]
fn queued_request_expires_behind_real_sqlite_writer_without_evaluation_or_commit() {
    let database = Database::new();
    let state = Arc::new(State::default());
    let writer = StorageWriter::open(database.path(), 8, Publisher(state.clone())).unwrap();
    let handle = writer.handle();
    let (entered, seen) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let blocker = handle
        .try_transaction(Operation(move |_: &dyn TransactionDocuments| {
            entered.send(()).unwrap();
            let _ = held.recv_timeout(Duration::from_secs(3));
            Ok(read(b"blocker"))
        }))
        .unwrap();
    seen.recv_timeout(Duration::from_secs(3)).unwrap();
    let due = Instant::now() + Duration::from_millis(30);
    let scoped = handle.with_transaction_execution_fence(Arc::new(move || Instant::now() < due));
    let evaluations = Arc::new(AtomicUsize::new(0));
    let observed = evaluations.clone();
    let queued = scoped
        .try_transaction(Operation(move |documents: &dyn TransactionDocuments| {
            observed.fetch_add(1, Ordering::AcqRel);
            write(documents)
        }))
        .unwrap();
    while Instant::now() < due {
        thread::sleep(Duration::from_millis(1));
    }
    release.send(()).unwrap();
    blocker.wait().unwrap();
    assert_eq!(queued.wait(), Err(StorageError::ExecutionCancelled));
    assert_eq!(evaluations.load(Ordering::Acquire), 0);
    assert_eq!(
        handle.try_revision().unwrap().wait().unwrap(),
        Revision::default()
    );
    assert!(handle
        .try_get("fenced".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    healthy(&handle, &state);
    writer.shutdown().unwrap();
}

#[test]
fn cancellation_after_evaluation_suppresses_prepared_write_and_read() {
    let database = Database::new();
    let state = Arc::new(State::default());
    let writer = StorageWriter::open(database.path(), 8, Publisher(state.clone())).unwrap();
    let handle = writer.handle();
    for writing in [false, true] {
        let active = Arc::new(AtomicBool::new(true));
        let cancel = active.clone();
        let result = handle
            .with_transaction_execution_fence(fence(&active))
            .try_transaction(Operation(move |documents: &dyn TransactionDocuments| {
                let decision = if writing {
                    write(documents)?
                } else {
                    read(b"secret")
                };
                cancel.store(false, Ordering::Release);
                Ok(decision)
            }))
            .unwrap()
            .wait();
        assert_eq!(result, Err(StorageError::ExecutionCancelled));
    }
    assert_eq!(state.published.load(Ordering::Acquire), 0);
    assert_eq!(state.rollbacks.load(Ordering::Acquire), 0);
    healthy(&handle, &state);
    writer.shutdown().unwrap();
}

#[test]
fn original_precommit_runs_and_cancellation_after_sql_writes_rolls_back() {
    let database = Database::new();
    let state = Arc::new(State::default());
    let writer = StorageWriter::open(database.path(), 8, Publisher(state.clone())).unwrap();
    let handle = writer.handle();
    let active = Arc::new(AtomicBool::new(true));
    let cancel = active.clone();
    let checked = Arc::new(AtomicBool::new(false));
    let check = checked.clone();
    let result = handle
        .with_transaction_execution_fence(fence(&active))
        .try_transaction(Operation(move |documents: &dyn TransactionDocuments| {
            let mut decision = write(documents)?;
            decision.precommit = Some(Box::new(move || {
                check.store(true, Ordering::Release);
                cancel.store(false, Ordering::Release);
                Ok(())
            }));
            Ok(decision)
        }))
        .unwrap()
        .wait();
    assert_eq!(result, Err(StorageError::ExecutionCancelled));
    assert!(checked.load(Ordering::Acquire));
    assert_eq!(state.rollbacks.load(Ordering::Acquire), 1);
    assert_eq!(state.published.load(Ordering::Acquire), 0);
    assert_eq!(
        handle.try_revision().unwrap().wait().unwrap(),
        Revision::default()
    );
    assert!(handle
        .try_get("fenced".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    healthy(&handle, &state);
    assert_eq!(
        handle
            .try_transaction(Operation(write))
            .unwrap()
            .wait()
            .unwrap(),
        b"write-complete"
    );
    assert_eq!(state.published.load(Ordering::Acquire), 1);
    writer.shutdown().unwrap();
}

#[test]
fn original_read_precommit_executes_and_typed_failure_survives_late_cancellation() {
    let database = Database::new();
    let state = Arc::new(State::default());
    let writer = StorageWriter::open(database.path(), 8, Publisher(state.clone())).unwrap();
    let handle = writer.handle();
    for writing in [false, true] {
        let active = Arc::new(AtomicBool::new(true));
        let cancel = active.clone();
        let result = handle
            .with_transaction_execution_fence(fence(&active))
            .try_transaction(Operation(move |documents: &dyn TransactionDocuments| {
                let mut decision = if writing {
                    write(documents)?
                } else {
                    read(b"secret")
                };
                decision.precommit = Some(Box::new(move || {
                    cancel.store(false, Ordering::Release);
                    Err(StorageError::Voice(
                        fc_domain::voice::VoiceError::InvalidState,
                    ))
                }));
                Ok(decision)
            }))
            .unwrap()
            .wait();
        assert_eq!(
            result,
            Err(StorageError::Voice(
                fc_domain::voice::VoiceError::InvalidState
            ))
        );
    }
    assert_eq!(state.rollbacks.load(Ordering::Acquire), 1);
    healthy(&handle, &state);
    writer.shutdown().unwrap();
}

#[test]
fn queued_positive_read_outcomes_are_suppressed_at_all_ticket_consumption_apis() {
    let database = Database::new();
    let state = Arc::new(State::default());
    let writer = StorageWriter::open(database.path(), 8, Publisher(state.clone())).unwrap();
    let handle = writer.handle();
    let active = Arc::new(AtomicBool::new(true));
    let scoped = handle.with_transaction_execution_fence(fence(&active));
    let mut tickets = Vec::new();
    for _ in 0..3 {
        tickets.push(
            scoped
                .try_transaction(Operation(|_: &dyn TransactionDocuments| {
                    Ok(read(b"secret"))
                }))
                .unwrap(),
        );
    }
    // Writer FIFO proves all three positive read replies were already queued.
    handle.try_revision().unwrap().wait().unwrap();
    active.store(false, Ordering::Release);
    assert_eq!(
        tickets.remove(0).try_result().unwrap(),
        Some(Err(StorageError::ExecutionCancelled))
    );
    assert_eq!(
        tickets.remove(0).wait(),
        Err(StorageError::ExecutionCancelled)
    );
    assert_eq!(
        tickets.remove(0).wait_timeout(Duration::from_secs(1)),
        Err(StorageError::ExecutionCancelled)
    );
    healthy(&handle, &state);
    writer.shutdown().unwrap();
}

#[test]
fn scoped_clones_cannot_replace_a_false_fence_with_true_or_change_the_base_handle() {
    let database = Database::new();
    let state = Arc::new(State::default());
    let writer = StorageWriter::open(database.path(), 8, Publisher(state.clone())).unwrap();
    let handle = writer.handle();
    let denied = handle.with_transaction_execution_fence(Arc::new(|| false));
    for scoped in [
        denied.clone(),
        denied.with_transaction_execution_fence(Arc::new(|| true)),
    ] {
        assert_eq!(
            scoped.try_transaction(Operation(write)).unwrap().wait(),
            Err(StorageError::ExecutionCancelled)
        );
        // Non-transaction reads intentionally keep their existing semantics.
        assert_eq!(
            scoped.try_revision().unwrap().wait().unwrap(),
            Revision::default()
        );
    }
    healthy(&handle, &state);
    writer.shutdown().unwrap();
}

#[test]
fn fence_panics_before_evaluation_and_after_original_precommit_cancel_without_faulting() {
    let database = Database::new();
    let state = Arc::new(State::default());
    let writer = StorageWriter::open(database.path(), 8, Publisher(state.clone())).unwrap();
    let handle = writer.handle();
    assert_eq!(
        handle
            .with_transaction_execution_fence(Arc::new(|| panic!("diagnostic fence panic")))
            .try_transaction(Operation(write))
            .unwrap()
            .wait(),
        Err(StorageError::ExecutionCancelled)
    );
    let panic_now = Arc::new(AtomicBool::new(false));
    let check_panic = panic_now.clone();
    let after = panic_now.clone();
    let scoped = handle.with_transaction_execution_fence(Arc::new(move || {
        assert!(
            !check_panic.load(Ordering::Acquire),
            "diagnostic precommit fence panic"
        );
        true
    }));
    assert_eq!(
        scoped
            .try_transaction(Operation(move |documents: &dyn TransactionDocuments| {
                let mut decision = write(documents)?;
                decision.precommit = Some(Box::new(move || {
                    after.store(true, Ordering::Release);
                    Ok(())
                }));
                Ok(decision)
            }))
            .unwrap()
            .wait(),
        Err(StorageError::ExecutionCancelled)
    );
    assert_eq!(state.rollbacks.load(Ordering::Acquire), 1);
    assert_eq!(state.published.load(Ordering::Acquire), 0);
    healthy(&handle, &state);
    writer.shutdown().unwrap();
}

#[test]
fn business_repository_scope_inherits_and_only_tightens_storage_execution_fence() {
    struct Time;
    impl fc_ports::Clock for Time {
        fn now(&self) -> fc_domain::time::LocalDateTime {
            fc_domain::time::LocalDateTime::new(
                fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(),
                0,
            )
            .unwrap()
        }
    }
    let database = Database::new();
    let state = Arc::new(State::default());
    let writer = StorageWriter::open(database.path(), 8, Publisher(state.clone())).unwrap();
    let repository = business::BusinessRepository::new(
        writer.handle(),
        Arc::new(Time),
        Arc::new(|_, revision| Ok(revision.get())),
    );
    assert!(repository.load_identity().is_ok());
    let scoped = repository
        .clone()
        .with_execution_fence(Arc::new(|| false))
        .with_execution_fence(Arc::new(|| true));
    assert_eq!(
        scoped.load_identity(),
        Err(StorageError::ExecutionCancelled)
    );
    assert!(repository.load_identity().is_ok());
    healthy(&writer.handle(), &state);
    writer.shutdown().unwrap();
}
