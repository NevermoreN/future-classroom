//! Real SQLite + real authority publication at the composition boundary.
//! Every test writer is joined before its host-local temporary directory is removed.
use fc_domain::ids::{AuthEpoch, Revision};
use fc_ports::AuthorityPublisher as PublicationPort;
use fc_runtime::{
    authority::{
        AuthorityPublisher, AuthorityReader, AuthorityStore, AuthorityUpdateToken,
        PreparedAuthority,
    },
    control::{ControlSignal, ControlSignals},
    health::{EffectIntent, GateError, Health},
};
use fc_storage::{CommitCommand, Document, Mutation, StorageError, StorageHandle, StorageWriter};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
type Candidate = PreparedAuthority<Vec<u8>>;

struct TestDirectory(PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let unique = format!(
            "FutureClassroom-RuntimeStorage-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        );
        // Resolve on the executing host, not the cross-compilation host.
        let directory = std::env::temp_dir().join(unique);
        std::fs::create_dir(&directory).unwrap();
        Self(directory)
    }
    fn database(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let cleanup = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            cleanup.expect("isolated SQLite test cleanup");
        }
    }
}

/// StorageWriter intentionally avoids a blocking Drop in production. Tests own
/// this joining guard so assertion failure also closes SQLite before cleanup.
struct JoinedWriter(Option<StorageWriter<Candidate>>);
impl JoinedWriter {
    fn new<P>(directory: &TestDirectory, publisher: P) -> Self
    where
        P: PublicationPort<Snapshot = Candidate> + Send + 'static,
    {
        Self(Some(
            StorageWriter::open(directory.database(), 8, publisher).unwrap(),
        ))
    }
    fn handle(&self) -> StorageHandle<Candidate> {
        self.0.as_ref().unwrap().handle()
    }
    fn shutdown(&mut self) {
        self.0.take().unwrap().shutdown().unwrap();
    }
}
impl Drop for JoinedWriter {
    fn drop(&mut self) {
        if let Some(writer) = self.0.take() {
            let stopped = writer.shutdown();
            if !std::thread::panicking() {
                stopped.expect("test SQLite writer join");
            }
        }
    }
}

fn put(id: &str, expected: u64, value: &[u8]) -> CommitCommand {
    CommitCommand::new(
        id.into(),
        Revision::new(expected),
        vec![Mutation::Put {
            key: "lesson".into(),
            value: value.to_vec(),
        }],
    )
    .unwrap()
}

fn candidate(reader: &AuthorityReader<Vec<u8>>, revision: u64, value: &[u8]) -> Candidate {
    reader
        .prepare_update(value.to_vec(), Revision::new(revision), AuthEpoch::new(7))
        .unwrap()
}

fn document(handle: &StorageHandle<Candidate>) -> Document {
    handle
        .try_get("lesson".into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap()
}

/// Startup-only reads occur while the real authority gate is STARTING. This
/// does not expose an old healthy snapshot merely to obtain recovery facts.
fn read_recovery_facts(directory: &TestDirectory) -> (Revision, Option<Document>) {
    let (publisher, reader) =
        AuthorityStore::new(Vec::<u8>::new(), Revision::new(0), AuthEpoch::new(7));
    let mut writer = JoinedWriter::new(directory, publisher);
    assert!(matches!(reader.snapshot(), Err(GateError::Starting)));
    let handle = writer.handle();
    let revision = handle.try_revision().unwrap().wait().unwrap();
    let row = handle.try_get("lesson".into()).unwrap().wait().unwrap();
    assert_eq!(reader.gate().check_sensitive(), Err(GateError::Starting));
    writer.shutdown();
    (revision, row)
}

#[test]
fn commit_cas_journal_and_recovery_keep_database_and_authority_consistent() {
    let directory = TestDirectory::new();
    let (mut publisher, reader) =
        AuthorityStore::new(Vec::<u8>::new(), Revision::new(0), AuthEpoch::new(7));
    publisher.initialize().unwrap();
    let mut writer = JoinedWriter::new(&directory, publisher);
    let handle = writer.handle();
    let receipt = handle
        .try_commit(
            put("first", 0, b"teacher A"),
            candidate(&reader, 1, b"teacher A"),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(receipt.revision, Revision::new(1));
    assert!(!receipt.replayed);
    let snapshot = reader.snapshot().unwrap();
    let persisted = document(&handle);
    assert_eq!(snapshot.data, persisted.value);
    assert_eq!(snapshot.revision, persisted.revision);
    let committed_health = reader.gate().status();
    assert_eq!(committed_health.health, Health::Healthy);
    assert_eq!(committed_health.revision, receipt.revision);
    assert_eq!(committed_health.auth_epoch, AuthEpoch::new(7));

    assert_eq!(
        handle
            .try_commit(
                put("stale", 0, b"wrong teacher"),
                candidate(&reader, 2, b"wrong teacher")
            )
            .unwrap()
            .wait(),
        Err(StorageError::Conflict {
            current: Revision::new(1)
        })
    );
    assert_eq!(
        reader.gate().status(),
        committed_health,
        "CAS rejection must not begin an authority update"
    );
    assert_eq!(document(&handle).value, b"teacher A");

    let replay = handle
        .try_commit(
            put("first", 0, b"teacher A"),
            candidate(&reader, 2, b"must never publish"),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.revision, Revision::new(1));
    assert_eq!(
        reader.gate().status(),
        committed_health,
        "journal replay must not republish a candidate"
    );
    assert_eq!(reader.snapshot().unwrap().data, b"teacher A");
    assert_eq!(
        handle
            .try_commit(
                put("first", 0, b"different payload"),
                candidate(&reader, 2, b"wrong")
            )
            .unwrap()
            .wait(),
        Err(StorageError::CommandIdReused)
    );
    assert_eq!(reader.gate().status(), committed_health);

    handle
        .try_commit(
            put("second", 1, b"teacher B"),
            candidate(&reader, 2, b"teacher B"),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(reader.snapshot().unwrap().data, b"teacher B");
    writer.shutdown();

    let (revision, recovered) = read_recovery_facts(&directory);
    let recovered = recovered.unwrap();
    assert_eq!(revision, Revision::new(2));
    assert_eq!(recovered.value, b"teacher B");
    assert_eq!(recovered.revision, revision);
    let (mut publisher, restored_reader) =
        AuthorityStore::new(recovered.value, revision, AuthEpoch::new(7));
    publisher.initialize().unwrap();
    let mut restored = JoinedWriter::new(&directory, publisher);
    let restored_handle = restored.handle();
    let replay = restored_handle
        .try_commit(
            put("first", 0, b"teacher A"),
            candidate(&restored_reader, 3, b"do not replay write"),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.revision, Revision::new(1));
    assert_eq!(
        restored_handle.try_revision().unwrap().wait().unwrap(),
        Revision::new(2)
    );
    assert_eq!(restored_reader.snapshot().unwrap().data, b"teacher B");
    assert_eq!(document(&restored_handle).value, b"teacher B");
    restored.shutdown();
}

#[derive(Clone, Copy)]
enum Failure {
    Error,
    Panic,
}

/// Delegate begin/rollback/fault and tokens to the production runtime. Only the
/// post-COMMIT publication call is injected; SQLite is never a test double.
struct FailPublication {
    real: AuthorityPublisher<Vec<u8>>,
    failure: Failure,
}
impl PublicationPort for FailPublication {
    type Snapshot = Candidate;
    type Update = AuthorityUpdateToken;
    type Error = GateError;
    fn snapshot_revision(&self, snapshot: &Candidate) -> Revision {
        self.real.snapshot_revision(snapshot)
    }
    fn begin_update(&mut self) -> Result<Self::Update, Self::Error> {
        self.real.begin_update()
    }
    fn publish(&mut self, _update: Self::Update, _snapshot: Candidate) -> Result<(), Self::Error> {
        match self.failure {
            Failure::Error => Err(GateError::Faulted),
            Failure::Panic => panic!("injected post-COMMIT authority publication panic"),
        }
        // _update's real Drop also faults if publication did not settle it.
    }
    fn rollback(&mut self, update: Self::Update) -> Result<(), Self::Error> {
        self.real.rollback(update)
    }
    fn fault(&mut self) {
        self.real.fault();
    }
}

fn verify_committed_fault(failure: Failure) {
    let directory = TestDirectory::new();
    let ui = ControlSignals::default();
    let audio = ControlSignals::default();
    let (mut publisher, reader) = AuthorityStore::with_controls(
        b"old authority".to_vec(),
        Revision::new(0),
        AuthEpoch::new(7),
        vec![ui.clone(), audio.clone()],
    );
    publisher.initialize().unwrap();
    let old_snapshot = reader.snapshot().unwrap();
    let gate = reader.gate();
    let old_lease = gate
        .lease_for_authorized_scope(12, Instant::now() + Duration::from_secs(30))
        .unwrap();
    let already_registered = gate
        .try_start(
            old_lease,
            EffectIntent {
                scope: 12,
                job_id: 1,
            },
        )
        .unwrap();
    let cancelled = gate.cancellation();
    let mut writer = JoinedWriter::new(
        &directory,
        FailPublication {
            real: publisher,
            failure,
        },
    );
    let handle = writer.handle();
    let prepared = candidate(&reader, 1, b"new committed authority");
    assert_eq!(
        handle
            .try_commit(
                put("committed-before-fault", 0, b"new committed authority"),
                prepared
            )
            .unwrap()
            .wait(),
        Err(StorageError::PublicationFailed {
            committed_revision: Revision::new(1)
        })
    );
    assert_eq!(gate.status().health, Health::Faulted);
    assert!(cancelled.is_cancelled());
    assert_eq!(
        old_snapshot.data, b"old authority",
        "old Arc is alive but cannot authorize a read"
    );
    assert!(matches!(reader.snapshot(), Err(GateError::Faulted)));
    assert_eq!(
        gate.check_permit(&already_registered),
        Err(GateError::Faulted)
    );
    assert!(matches!(
        gate.try_start(
            old_lease,
            EffectIntent {
                scope: 12,
                job_id: 2
            }
        ),
        Err(GateError::Faulted)
    ));
    assert_eq!(
        handle.try_get("lesson".into()).unwrap().wait(),
        Err(StorageError::AuthorityUnavailable)
    );
    assert!(ui.take().contains(ControlSignal::ForegroundHidden));
    assert!(audio.take().contains(ControlSignal::Fault));
    writer.shutdown();

    let (revision, committed) = read_recovery_facts(&directory);
    let committed = committed.unwrap();
    assert_eq!(
        revision,
        Revision::new(1),
        "COMMITTED fact survives publication failure"
    );
    assert_eq!(committed.revision, revision);
    assert_eq!(committed.value, b"new committed authority");
    let (mut publisher, restored_reader) =
        AuthorityStore::new(committed.value, revision, AuthEpoch::new(7));
    publisher.initialize().unwrap();
    let mut restored = JoinedWriter::new(&directory, publisher);
    let replay = restored
        .handle()
        .try_commit(
            put("committed-before-fault", 0, b"new committed authority"),
            candidate(&restored_reader, 2, b"never reexecute"),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        restored_reader.snapshot().unwrap().data,
        b"new committed authority"
    );
    assert_eq!(restored_reader.gate().status().revision, Revision::new(1));
    assert_eq!(
        gate.status().health,
        Health::Faulted,
        "restart never revives the old process gate"
    );
    restored.shutdown();
}

#[test]
fn sqlite_commit_then_publication_error_blocks_old_permissions_and_recovers_facts() {
    verify_committed_fault(Failure::Error);
}

#[test]
fn sqlite_commit_then_publication_panic_blocks_old_permissions_and_recovers_facts() {
    verify_committed_fault(Failure::Panic);
}
