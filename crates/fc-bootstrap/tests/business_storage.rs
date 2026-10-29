//! Typed application commands against real SQLite and real authority publication.
use fc_domain::{auth::*, content::*, identity::*, ids::*, time::*};
use fc_ports::{
    prepare_initial_credentials, prepare_password_change, Clock, ContentRepository,
    CredentialCrypto, CredentialError, CredentialIdentityRepository, IdentityRepository,
    IdentityRepositoryError, IdentityRequest, RepositoryError,
};
use fc_runtime::authority::{AuthorityStore, PreparedAuthority};
use fc_storage::{business::BusinessRepository, StorageError, StorageWriter};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
type Snapshot = PreparedAuthority<IdentityState>;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "FutureClassroom-Business-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn path(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
struct Joined(Option<StorageWriter<Snapshot>>);
impl Drop for Joined {
    fn drop(&mut self) {
        if let Some(w) = self.0.take() {
            let r = w.shutdown();
            if !std::thread::panicking() {
                r.unwrap();
            }
        }
    }
}
struct Time(Mutex<LocalDateTime>);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        *self.0.lock().unwrap()
    }
}
fn date(day: u8) -> LocalDate {
    LocalDate::new(2026, 10, day).unwrap()
}
fn time(day: u8) -> LocalDateTime {
    LocalDateTime::new(date(day), 3600).unwrap()
}
fn principal(id: u64) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn actor(id: u64, epoch: u64, origin: TrustedOrigin) -> ActorContext {
    ActorContext {
        principal_id: principal(id),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(epoch),
        expires_at: LocalDateTime::new(date(31), 86399).unwrap(),
        origin,
    }
}
fn request(id: u64, revision: u64, command: IdentityCommand) -> IdentityRequest {
    IdentityRequest {
        command_id: CommandId::new(id).unwrap(),
        expected_revision: Revision::new(revision),
        command,
    }
}
// The cryptographic worker is outside this storage integration test. Its fake
// records intentionally test atomic version references, not cryptography.
struct PreparedCredentials {
    id: u64,
    version: u64,
    both: bool,
}
fn credentials(id: u64, version: u64, both: bool) -> PreparedCredentials {
    PreparedCredentials { id, version, both }
}
struct FixtureCrypto;
impl CredentialCrypto for FixtureCrypto {
    type Secret = Vec<u8>;
    type PasswordRecord = Vec<u8>;
    type RecoveryCode = ();
    type RecoveryRecord = Vec<u8>;
    fn password_record(&mut self, s: &Vec<u8>) -> Result<Vec<u8>, CredentialError> {
        Ok(s.clone())
    }
    fn verify_password(&mut self, _s: &Vec<u8>, _r: &Vec<u8>) -> Result<bool, CredentialError> {
        Ok(true)
    }
    fn recovery_record(&mut self) -> Result<((), Vec<u8>), CredentialError> {
        Ok(((), vec![43]))
    }
    fn verify_recovery(&mut self, _c: &(), _r: &Vec<u8>) -> Result<bool, CredentialError> {
        Ok(true)
    }
}
trait FixturePreparation {
    fn execute_identity_with_credentials(
        &mut self,
        r: IdentityRequest,
        a: Option<&ActorContext>,
        c: PreparedCredentials,
    ) -> Result<IdentityTransition, IdentityRepositoryError>;
}
impl FixturePreparation for BusinessRepository<Snapshot> {
    fn execute_identity_with_credentials(
        &mut self,
        r: IdentityRequest,
        a: Option<&ActorContext>,
        c: PreparedCredentials,
    ) -> Result<IdentityTransition, IdentityRepositoryError> {
        let secret = vec![c.id as u8, c.version as u8, 42];
        let mut crypto = FixtureCrypto;
        let change = if c.both {
            prepare_initial_credentials(&mut crypto, &secret, r)
                .unwrap()
                .0
        } else {
            prepare_password_change(&mut crypto, &secret, &secret, &secret, r).unwrap()
        };
        self.execute_credentials(change, a, &Time(Mutex::new(time(30))))
    }
}
fn open(
    d: &Directory,
    clock: Arc<Time>,
    state: IdentityState,
    revision: Revision,
) -> (Joined, BusinessRepository<Snapshot>) {
    let (mut p, r) = AuthorityStore::new(state, revision, AuthEpoch::new(1));
    p.initialize().unwrap();
    let w = StorageWriter::open(d.path(), 16, p).unwrap();
    let handle = w.handle();
    let factory = Arc::new(move |s: &IdentityState, revision: Revision| {
        r.prepare_update(s.clone(), revision, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    (
        Joined(Some(w)),
        BusinessRepository::new(handle, clock, factory).with_session_guard(Arc::new(|_, _| Ok(()))),
    )
}
fn initialize(repo: &mut BusinessRepository<Snapshot>, clock: &Time) {
    repo.execute_identity_with_credentials(
        request(
            1,
            0,
            IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "School".into(),
                id: principal(1),
                login_name: "admin".into(),
                display_name: "Admin".into(),
            },
        ),
        None,
        credentials(1, 1, true),
    )
    .unwrap();
    let admin = actor(1, 1, TrustedOrigin::Local);
    repo.execute_identity(
        request(
            2,
            1,
            IdentityCommand::CreateClass {
                id: ClassId::new(1).unwrap(),
                name: "Class".into(),
            },
        ),
        Some(&admin),
        clock,
    )
    .unwrap();
    repo.execute_identity(
        request(
            3,
            2,
            IdentityCommand::PutSubject {
                id: SubjectId::new(1).unwrap(),
                name: "Math".into(),
                enabled: true,
                color: Rgb {
                    red: 40,
                    green: 80,
                    blue: 160,
                },
                expected_revision: None,
            },
        ),
        Some(&admin),
        clock,
    )
    .unwrap();
    for (id, rev) in [(2, 3), (3, 5)] {
        repo.execute_identity_with_credentials(
            request(
                10 + id,
                rev,
                IdentityCommand::CreateAccount(NewAccount {
                    id: principal(id),
                    login_name: format!("teacher{id}"),
                    display_name: format!("Teacher {id}"),
                    role: Role::TeacherAdmin,
                    scopes: vec![AssignmentScope {
                        class_id: ClassId::new(1).unwrap(),
                        subject_id: SubjectId::new(1).unwrap(),
                    }],
                }),
            ),
            Some(&admin),
            credentials(id, 1, true),
        )
        .unwrap();
        repo.execute_identity_with_credentials(
            request(
                20 + id,
                rev + 1,
                IdentityCommand::PasswordChanged {
                    id: principal(id),
                    expected_credential_version: 1,
                },
            ),
            Some(&actor(id, 1, TrustedOrigin::Local)),
            credentials(id, 2, false),
        )
        .unwrap();
    }
}
fn command(id: u64, owner: u64, revision: u64, text: &str) -> ContentCommand {
    ContentCommand {
        command_id: CommandId::new(id).unwrap(),
        key: ContentKey::Homework(HomeworkKey {
            class_id: ClassId::new(1).unwrap(),
            actual_date: date(30),
            subject_id: SubjectId::new(1).unwrap(),
            owner_teacher_id: principal(owner),
        }),
        expected_revision: Revision::new(revision),
        text: text.into(),
    }
}
#[test]
fn typed_identity_and_content_survive_restart_with_per_owner_cas_and_receipts() {
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (w, mut repo) = open(
        &d,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let a = actor(2, 2, TrustedOrigin::Web);
    let b = actor(3, 2, TrustedOrigin::Web);
    let first =
        fc_application::execute_content(&mut repo, &*clock, &a, command(100, 2, 0, "first"))
            .unwrap();
    let second =
        fc_application::execute_content(&mut repo, &*clock, &b, command(101, 3, 0, "second"))
            .unwrap();
    assert_eq!(first.revision, Revision::new(1));
    assert_eq!(second.revision, Revision::new(1));
    assert_eq!(
        repo.execute(command(100, 2, 0, "first"), &a, &*clock)
            .unwrap(),
        first
    );
    assert_eq!(
        repo.execute(command(100, 2, 0, "changed"), &a, &*clock),
        Err(RepositoryError::Domain(ContentError::IdempotencyConflict))
    );
    assert_eq!(
        repo.execute(command(102, 2, 0, "stale"), &a, &*clock),
        Err(RepositoryError::Domain(ContentError::RevisionConflict {
            current: Revision::new(1)
        }))
    );
    assert_eq!(
        repo.execute(command(103, 2, 1, "cross owner"), &b, &*clock),
        Err(RepositoryError::Domain(ContentError::Permission(
            PermissionError::WrongOwner
        )))
    );
    let state = repo.load_identity().unwrap();
    let revision =
        w.0.as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap();
    drop(repo);
    drop(w);
    let (w, mut restored) = open(&d, clock.clone(), state.clone(), revision);
    assert_eq!(restored.load_identity().unwrap(), state);
    assert_eq!(
        restored.credential_record(principal(2), false).unwrap().0,
        2
    );
    assert_eq!(
        restored.content(first.key.clone(), &a).unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        restored
            .execute(command(100, 2, 0, "first"), &a, &*clock)
            .unwrap(),
        first
    );
    assert_eq!(
        w.0.as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap(),
        revision
    );
    *clock.0.lock().unwrap() = time(31);
    assert_eq!(restored.content(first.key.clone(), &a).unwrap(), None);
    assert_eq!(
        restored.execute(command(104, 2, 1, "yesterday"), &a, &*clock),
        Err(RepositoryError::Domain(ContentError::WrongDate))
    );
}
#[test]
fn queued_content_rechecks_current_identity_and_midnight_in_writer() {
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (_w, mut repo) = open(
        &d,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let admin = actor(1, 1, TrustedOrigin::Local);
    let teacher = actor(2, 2, TrustedOrigin::Web);
    repo.execute_identity(
        request(
            80,
            7,
            IdentityCommand::UpdateAccount {
                id: principal(2),
                display_name: "Disabled".into(),
                role: Role::TeacherAdmin,
                enabled: false,
                scopes: vec![AssignmentScope {
                    class_id: ClassId::new(1).unwrap(),
                    subject_id: SubjectId::new(1).unwrap(),
                }],
            },
        ),
        Some(&admin),
        &*clock,
    )
    .unwrap();
    assert!(matches!(
        repo.execute(command(110, 2, 0, "denied"), &teacher, &*clock),
        Err(RepositoryError::Domain(ContentError::Permission(
            PermissionError::Disabled
        )))
    ));
    let reminder = ContentCommand {
        command_id: CommandId::new(111).unwrap(),
        key: ContentKey::SharedReminder(ReminderKey {
            class_id: ClassId::new(1).unwrap(),
            actual_date: date(30),
        }),
        expected_revision: Revision::default(),
        text: "Remember".into(),
    };
    assert_eq!(
        repo.execute(reminder.clone(), &admin, &*clock),
        Err(RepositoryError::Domain(ContentError::Permission(
            PermissionError::WebOnly
        )))
    );
    let web_admin = actor(1, 1, TrustedOrigin::Web);
    repo.execute(reminder.clone(), &web_admin, &*clock).unwrap();
    *clock.0.lock().unwrap() = time(31);
    assert_eq!(repo.content(reminder.key, &web_admin).unwrap(), None);
}
#[test]
fn plain_identity_port_cannot_fabricate_credentials_and_corruption_is_not_registration() {
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    {
        let (_w, mut repo) = open(
            &d,
            clock.clone(),
            IdentityState::default(),
            Revision::default(),
        );
        let r = request(
            1,
            0,
            IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "School".into(),
                id: principal(1),
                login_name: "admin".into(),
                display_name: "Admin".into(),
            },
        );
        assert!(matches!(
            repo.execute_identity(r, None, &*clock),
            Err(IdentityRepositoryError::Domain(IdentityError::Permission(
                _
            )))
        ));
        assert_eq!(repo.load_identity().unwrap(), IdentityState::default());
    }
    std::fs::write(d.path(), b"").unwrap();
    let (p, _) = AuthorityStore::new(
        IdentityState::default(),
        Revision::default(),
        AuthEpoch::new(1),
    );
    assert!(matches!(
        StorageWriter::open(d.path(), 8, p),
        Err(StorageError::Database)
    ));
    std::fs::write(d.path(), b"invalid sqlite database").unwrap();
    let (p, _) = AuthorityStore::new(
        IdentityState::default(),
        Revision::default(),
        AuthEpoch::new(1),
    );
    assert!(matches!(
        StorageWriter::open(d.path(), 8, p),
        Err(StorageError::Database)
    ));
}

#[test]
fn commit_rechecks_midnight_after_snapshot_preparation_and_stop_rejects_new_work() {
    use std::sync::atomic::AtomicBool;
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let armed = Arc::new(AtomicBool::new(false));
    let (mut p, r) = AuthorityStore::new(
        IdentityState::default(),
        Revision::default(),
        AuthEpoch::new(1),
    );
    p.initialize().unwrap();
    let writer = StorageWriter::open(d.path(), 8, p).unwrap();
    let handle = writer.handle();
    let joined = Joined(Some(writer));
    let c = clock.clone();
    let flag = armed.clone();
    let factory = Arc::new(move |s: &IdentityState, revision: Revision| {
        if flag.load(Ordering::Acquire) {
            *c.0.lock().unwrap() = time(31);
        }
        r.prepare_update(s.clone(), revision, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    let mut repo = BusinessRepository::new(handle.clone(), clock.clone(), factory)
        .with_session_guard(Arc::new(|_, _| Ok(())));
    initialize(&mut repo, &clock);
    let before = handle.try_revision().unwrap().wait().unwrap();
    armed.store(true, Ordering::Release);
    assert_eq!(
        repo.execute(
            command(200, 2, 0, "midnight draft"),
            &actor(2, 2, TrustedOrigin::Web),
            &*clock
        ),
        Err(RepositoryError::Domain(ContentError::WrongDate))
    );
    assert_eq!(handle.try_revision().unwrap().wait().unwrap(), before);
    armed.store(false, Ordering::Release);
    *clock.0.lock().unwrap() = time(30);
    assert_eq!(
        repo.content(command(200, 2, 0, "").key, &actor(2, 2, TrustedOrigin::Web))
            .unwrap(),
        None
    );
    handle.request_stop();
    assert!(matches!(repo.load_identity(), Err(StorageError::Stopping)));
    drop(joined);
}

#[test]
fn typed_commit_then_publication_failure_is_durable_and_old_reads_fail_closed() {
    use fc_ports::AuthorityPublisher as Port;
    use fc_runtime::{
        authority::{AuthorityPublisher, AuthorityUpdateToken},
        health::{GateError, Health},
    };
    struct Broken(AuthorityPublisher<IdentityState>);
    impl Port for Broken {
        type Snapshot = Snapshot;
        type Update = AuthorityUpdateToken;
        type Error = GateError;
        fn snapshot_revision(&self, s: &Snapshot) -> Revision {
            self.0.snapshot_revision(s)
        }
        fn begin_update(&mut self) -> Result<Self::Update, GateError> {
            self.0.begin_update()
        }
        fn publish(&mut self, _t: Self::Update, _s: Snapshot) -> Result<(), GateError> {
            Err(GateError::Faulted)
        }
        fn rollback(&mut self, t: Self::Update) -> Result<(), GateError> {
            self.0.rollback(t)
        }
        fn fault(&mut self) {
            self.0.fault()
        }
    }
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (w, mut repo) = open(
        &d,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let state = repo.load_identity().unwrap();
    let revision =
        w.0.as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap();
    drop(repo);
    drop(w);
    let (mut p, r) = AuthorityStore::new(state.clone(), revision, AuthEpoch::new(1));
    p.initialize().unwrap();
    let health = r.gate();
    let writer = StorageWriter::open(d.path(), 8, Broken(p)).unwrap();
    let handle = writer.handle();
    let joined = Joined(Some(writer));
    let factory = Arc::new(move |s: &IdentityState, revision: Revision| {
        r.prepare_update(s.clone(), revision, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    let mut repo = BusinessRepository::new(handle, clock.clone(), factory)
        .with_session_guard(Arc::new(|_, _| Ok(())));
    let teacher = actor(2, 2, TrustedOrigin::Web);
    let cmd = command(300, 2, 0, "committed before publication failure");
    assert_eq!(
        repo.execute(cmd.clone(), &teacher, &*clock),
        Err(RepositoryError::Faulted)
    );
    assert_eq!(health.status().health, Health::Faulted);
    assert_eq!(
        repo.load_identity(),
        Err(StorageError::AuthorityUnavailable)
    );
    drop(repo);
    drop(joined);
    let (_w, mut restored) = open(&d, clock.clone(), state, revision.next().unwrap());
    let saved = restored
        .content(cmd.key.clone(), &teacher)
        .unwrap()
        .unwrap();
    assert_eq!(saved.text, cmd.text);
    assert_eq!(restored.execute(cmd, &teacher, &*clock).unwrap(), saved);
}

#[test]
fn exact_local_session_lock_between_admission_and_commit_revokes_local_but_keeps_web() {
    use fc_application::identity::{IdentitySessions, SessionPurpose};
    use std::sync::atomic::AtomicBool;
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let sessions = Arc::new(Mutex::new(IdentitySessions::default()));
    let arm = Arc::new(AtomicBool::new(false));
    let (mut p, r) = AuthorityStore::new(
        IdentityState::default(),
        Revision::default(),
        AuthEpoch::new(1),
    );
    p.initialize().unwrap();
    let writer = StorageWriter::open(d.path(), 8, p).unwrap();
    let handle = writer.handle();
    let _joined = Joined(Some(writer));
    let locking = sessions.clone();
    let armed = arm.clone();
    let factory = Arc::new(move |s: &IdentityState, revision: Revision| {
        if armed.load(Ordering::Acquire) {
            locking.lock().unwrap().lock_local();
        }
        r.prepare_update(s.clone(), revision, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    let mut repo = BusinessRepository::new(handle.clone(), clock.clone(), factory)
        .with_session_guard(Arc::new(|_, _| Ok(())));
    initialize(&mut repo, &clock);
    let state = repo.load_identity().unwrap();
    let account = state.account(principal(2)).unwrap();
    let expires = LocalDateTime::new(date(31), 86399).unwrap();
    let local = sessions
        .lock()
        .unwrap()
        .issue_verified(account, TrustedOrigin::Local, 0, 600000, expires)
        .unwrap();
    let web = sessions
        .lock()
        .unwrap()
        .issue_verified(account, TrustedOrigin::Web, 0, 600000, expires)
        .unwrap();
    let local_actor = sessions
        .lock()
        .unwrap()
        .actor(&state, local, clock.now(), 1, SessionPurpose::Manage)
        .unwrap();
    let web_actor = sessions
        .lock()
        .unwrap()
        .actor(&state, web, clock.now(), 1, SessionPurpose::Manage)
        .unwrap();
    let ledger = sessions.clone();
    let t = clock.clone();
    let mut local_repo = repo
        .clone()
        .with_session_guard(Arc::new(move |state, actor| {
            let fresh = ledger
                .lock()
                .unwrap()
                .actor(state, local, t.now(), 1, SessionPurpose::Manage)
                .map_err(|_| StorageError::AuthorityUnavailable)?;
            if fresh == *actor {
                Ok(())
            } else {
                Err(StorageError::AuthorityUnavailable)
            }
        }));
    let ledger = sessions.clone();
    let t = clock.clone();
    let mut web_repo = repo.with_session_guard(Arc::new(move |state, actor| {
        let fresh = ledger
            .lock()
            .unwrap()
            .actor(state, web, t.now(), 1, SessionPurpose::Manage)
            .map_err(|_| StorageError::AuthorityUnavailable)?;
        if fresh == *actor {
            Ok(())
        } else {
            Err(StorageError::AuthorityUnavailable)
        }
    }));
    let before = handle.try_revision().unwrap().wait().unwrap();
    arm.store(true, Ordering::Release);
    assert_eq!(
        fc_application::execute_content(
            &mut local_repo,
            &*clock,
            &local_actor,
            command(400, 2, 0, "locked draft")
        ),
        Err(RepositoryError::Faulted)
    );
    assert_eq!(handle.try_revision().unwrap().wait().unwrap(), before);
    arm.store(false, Ordering::Release);
    assert_eq!(
        web_repo
            .execute(
                command(401, 2, 0, "web survives local lock"),
                &web_actor,
                &*clock
            )
            .unwrap()
            .revision,
        Revision::new(1)
    );
}

// Physical KDF permits are process-wide, so only login fixtures serialize.
// Unrelated business integration tests retain their normal parallel execution.
static LOGIN_FIXTURE: Mutex<()> = Mutex::new(());
// Security counters use an independently injected monotonic clock. Moving the
// classroom's civil date cannot shorten a waiting period.
#[derive(Default)]
struct Tick(AtomicU64);
impl fc_ports::MonotonicClock for Tick {
    fn monotonic_millis(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}
fn open_login(
    d: &Directory,
    wall: Arc<Time>,
    tick: Arc<dyn fc_ports::MonotonicClock + Send + Sync>,
    state: IdentityState,
    revision: Revision,
) -> (
    Joined,
    BusinessRepository<Snapshot>,
    fc_storage::StorageHandle<Snapshot>,
) {
    let (mut publisher, reader) = AuthorityStore::new(state, revision, AuthEpoch::new(1));
    publisher.initialize().unwrap();
    let writer = StorageWriter::open_with_login_clock(d.path(), 16, publisher, tick).unwrap();
    let handle = writer.handle();
    let factory = Arc::new(move |s: &IdentityState, r: Revision| {
        reader
            .prepare_update(s.clone(), r, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    let repo = BusinessRepository::new(handle.clone(), wall, factory)
        .with_session_guard(Arc::new(|_, _| Ok(())));
    (Joined(Some(writer)), repo, handle)
}
fn admission(
    handle: &fc_storage::StorageHandle<Snapshot>,
    id: u64,
    kind: fc_storage::login::CredentialKind,
) -> Result<fc_storage::login::Admission, StorageError> {
    match handle.try_login_admission(principal(id), kind)?.wait()? {
        fc_storage::login::Response::Admitted(a) => Ok(a),
        _ => panic!("admission response"),
    }
}
fn complete(
    handle: &fc_storage::StorageHandle<Snapshot>,
    a: fc_storage::login::Admission,
    result: fc_storage::login::Verification,
) -> Result<fc_storage::login::Completion, StorageError> {
    match handle.try_finish_login(a.reservation, result)?.wait()? {
        fc_storage::login::Response::Completed(c) => Ok(c),
        _ => panic!("completion response"),
    }
}
#[test]
fn login_backoff_is_atomic_exponential_capped_and_survives_restart_and_wall_changes() {
    let _login_fixture = LOGIN_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    use fc_storage::login::{Completion, CredentialKind::Password, Verification};
    let d = Directory::new();
    let wall = Arc::new(Time(Mutex::new(time(30))));
    let tick = Arc::new(Tick::default());
    let (w, mut repo, handle) = open_login(
        &d,
        wall.clone(),
        tick.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &wall);
    let revision = handle.try_revision().unwrap().wait().unwrap();
    for (failure, wait_seconds) in [(1, 0), (2, 0), (3, 0), (4, 0), (5, 1)] {
        let a = admission(&handle, 2, Password).unwrap();
        assert_eq!(
            complete(&handle, a, Verification::Rejected).unwrap(),
            Completion::Rejected {
                failures: failure,
                wait_seconds
            }
        );
    }
    assert_eq!(
        admission(&handle, 2, Password).unwrap_err(),
        StorageError::LoginRateLimited {
            retry_after_millis: 1000
        }
    );
    assert_eq!(
        handle.try_revision().unwrap().wait().unwrap(),
        revision,
        "login security state has its own revision"
    );
    let state = repo.load_identity().unwrap();
    drop(repo);
    drop(handle);
    drop(w);
    *wall.0.lock().unwrap() = time(31);
    let reset_tick = Arc::new(Tick::default());
    let (_w, _repo, handle) = open_login(&d, wall.clone(), reset_tick.clone(), state, revision);
    assert_eq!(
        admission(&handle, 2, Password).unwrap_err(),
        StorageError::LoginRateLimited {
            retry_after_millis: 1000
        }
    );
    *wall.0.lock().unwrap() = time(29);
    reset_tick.0.store(999, Ordering::Release);
    assert_eq!(
        admission(&handle, 2, Password).unwrap_err(),
        StorageError::LoginRateLimited {
            retry_after_millis: 1
        }
    );
    reset_tick.0.store(1000, Ordering::Release);
    for (failure, wait) in [
        (6, 2),
        (7, 4),
        (8, 8),
        (9, 16),
        (10, 32),
        (11, 64),
        (12, 128),
        (13, 256),
        (14, 512),
        (15, 900),
        (16, 900),
    ] {
        let a = admission(&handle, 2, Password).unwrap();
        assert_eq!(
            complete(&handle, a, Verification::Rejected).unwrap(),
            Completion::Rejected {
                failures: failure,
                wait_seconds: wait
            }
        );
        assert_eq!(
            admission(&handle, 2, Password).unwrap_err(),
            StorageError::LoginRateLimited {
                retry_after_millis: u64::from(wait) * 1000
            }
        );
        reset_tick
            .0
            .fetch_add(u64::from(wait) * 1000, Ordering::AcqRel);
    }
    let a = admission(&handle, 2, Password).unwrap();
    assert!(matches!(
        complete(&handle, a, Verification::Verified).unwrap(),
        Completion::Authenticated { .. }
    ));
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(
        complete(&handle, a, Verification::Rejected).unwrap(),
        Completion::Rejected {
            failures: 1,
            wait_seconds: 0
        }
    );
}
#[test]
fn login_reservations_bound_actual_workers_even_after_authorization_times_out() {
    let _login_fixture = LOGIN_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    use fc_storage::login::{
        Completion,
        CredentialKind::{Password, Recovery},
        Verification, RESERVATION_MILLIS,
    };
    let d = Directory::new();
    let wall = Arc::new(Time(Mutex::new(time(30))));
    let tick = Arc::new(Tick::default());
    let (_w, mut repo, handle) = open_login(
        &d,
        wall.clone(),
        tick.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &wall);
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(
        admission(&handle, 2, Recovery).unwrap_err(),
        StorageError::LoginInProgress,
        "password/recovery share one account reservation"
    );
    let b = admission(&handle, 3, Recovery).unwrap();
    assert_eq!(
        admission(&handle, 1, Password).unwrap_err(),
        StorageError::LoginCapacity
    );
    tick.0.store(RESERVATION_MILLIS, Ordering::Release);
    assert_eq!(
        admission(&handle, 1, Password).unwrap_err(),
        StorageError::LoginCapacity,
        "timed-out KDFs still own their CPU permits"
    );
    assert_eq!(
        complete(&handle, a, Verification::Verified).unwrap_err(),
        StorageError::LoginReservationExpired
    );
    let admin = admission(&handle, 1, Password).unwrap();
    drop(b);
    assert_eq!(
        complete(&handle, admin, Verification::Rejected).unwrap(),
        Completion::Rejected {
            failures: 1,
            wait_seconds: 0
        }
    );
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(
        complete(&handle, a, Verification::Rejected).unwrap(),
        Completion::Rejected {
            failures: 2,
            wait_seconds: 0
        }
    );
}
#[test]
fn pending_login_restart_counts_failure_and_stale_or_corrupt_records_never_authenticate() {
    let _login_fixture = LOGIN_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    use fc_storage::login::{Completion, CredentialKind::Password, Verification};
    let d = Directory::new();
    let wall = Arc::new(Time(Mutex::new(time(30))));
    let tick = Arc::new(Tick::default());
    let (w, mut repo, handle) = open_login(
        &d,
        wall.clone(),
        tick.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &wall);
    let a = admission(&handle, 2, Password).unwrap();
    let state = repo.load_identity().unwrap();
    let revision = handle.try_revision().unwrap().wait().unwrap();
    drop(a);
    drop(repo);
    drop(handle);
    drop(w);
    let (_w, mut repo, handle) =
        open_login(&d, wall.clone(), Arc::new(Tick::default()), state, revision);
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(
        complete(&handle, a, Verification::Rejected).unwrap(),
        Completion::Rejected {
            failures: 2,
            wait_seconds: 0
        },
        "unsettled durable reservation survived restart as one failure"
    );
    let stale = admission(&handle, 2, Password).unwrap();
    repo.execute_identity_with_credentials(
        request(
            900,
            7,
            IdentityCommand::PasswordChanged {
                id: principal(2),
                expected_credential_version: 2,
            },
        ),
        Some(&actor(2, 2, TrustedOrigin::Local)),
        credentials(2, 3, false),
    )
    .unwrap();
    assert_eq!(
        complete(&handle, stale, Verification::Verified).unwrap_err(),
        StorageError::LoginStaleCredential
    );
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(a.credential_version, 3);
    assert_eq!(
        complete(&handle, a, Verification::CorruptRecord).unwrap_err(),
        StorageError::CredentialCorrupt
    );
    assert_eq!(
        admission(&handle, 2, Password).unwrap_err(),
        StorageError::CredentialCorrupt,
        "corrupt record requires repair rather than resetting penalties"
    );
    repo.execute_identity_with_credentials(
        request(
            901,
            8,
            IdentityCommand::PasswordChanged {
                id: principal(2),
                expected_credential_version: 3,
            },
        ),
        Some(&actor(2, 3, TrustedOrigin::Local)),
        credentials(2, 4, false),
    )
    .unwrap();
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(a.credential_version, 4);
    assert!(matches!(
        complete(&handle, a, Verification::Verified).unwrap(),
        Completion::Authenticated { .. }
    ));
}
#[test]
fn login_clock_regression_is_rejected_without_releasing_running_worker() {
    let _login_fixture = LOGIN_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    use fc_storage::login::{CredentialKind::Password, Verification};
    let d = Directory::new();
    let wall = Arc::new(Time(Mutex::new(time(30))));
    let tick = Arc::new(Tick::default());
    tick.0.store(100, Ordering::Release);
    let (_w, mut repo, handle) = open_login(
        &d,
        wall.clone(),
        tick.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &wall);
    let a = admission(&handle, 2, Password).unwrap();
    tick.0.store(99, Ordering::Release);
    assert_eq!(
        admission(&handle, 3, Password).unwrap_err(),
        StorageError::LoginClockRegression
    );
    tick.0.store(100, Ordering::Release);
    assert_eq!(
        admission(&handle, 2, Password).unwrap_err(),
        StorageError::LoginInProgress
    );
    complete(&handle, a, Verification::Rejected).unwrap();
}

struct CommitJumpClock {
    tick: AtomicU64,
    reads: AtomicU64,
    jump_at: AtomicU64,
    jump_to: AtomicU64,
}
impl Default for CommitJumpClock {
    fn default() -> Self {
        Self {
            tick: AtomicU64::new(0),
            reads: AtomicU64::new(0),
            jump_at: AtomicU64::new(u64::MAX),
            jump_to: AtomicU64::new(0),
        }
    }
}
impl fc_ports::MonotonicClock for CommitJumpClock {
    fn monotonic_millis(&self) -> u64 {
        let read = self.reads.fetch_add(1, Ordering::AcqRel) + 1;
        if read == self.jump_at.load(Ordering::Acquire) {
            self.tick
                .store(self.jump_to.load(Ordering::Acquire), Ordering::Release);
        }
        self.tick.load(Ordering::Acquire)
    }
}
impl CommitJumpClock {
    fn jump_at_commit(&self, to: u64) {
        self.jump_to.store(to, Ordering::Release);
        self.jump_at
            .store(self.reads.load(Ordering::Acquire) + 2, Ordering::Release);
    }
}
#[test]
fn login_completion_crossing_deadline_inside_transaction_cannot_authenticate() {
    let _login_fixture = LOGIN_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    use fc_storage::login::{
        Completion, CredentialKind::Password, Verification, RESERVATION_MILLIS,
    };
    let d = Directory::new();
    let wall = Arc::new(Time(Mutex::new(time(30))));
    let tick = Arc::new(CommitJumpClock::default());
    let (_w, mut repo, handle) = open_login(
        &d,
        wall.clone(),
        tick.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &wall);
    let a = admission(&handle, 2, Password).unwrap();
    tick.jump_at_commit(RESERVATION_MILLIS);
    assert_eq!(
        complete(&handle, a, Verification::Verified).unwrap_err(),
        StorageError::LoginReservationExpired,
        "expiry is checked after SQL writes before COMMIT"
    );
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(
        complete(&handle, a, Verification::Rejected).unwrap(),
        Completion::Rejected {
            failures: 2,
            wait_seconds: 0
        },
        "late successful verification was persisted as one expired failure"
    );
}
#[test]
fn login_admission_rechecks_owned_clock_before_committing_reservation() {
    let _login_fixture = LOGIN_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    use fc_storage::login::{Completion, CredentialKind::Password, Verification};
    let d = Directory::new();
    let wall = Arc::new(Time(Mutex::new(time(30))));
    let tick = Arc::new(CommitJumpClock::default());
    tick.tick.store(100, Ordering::Release);
    let (_w, mut repo, handle) = open_login(
        &d,
        wall.clone(),
        tick.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &wall);
    tick.jump_at_commit(99);
    assert_eq!(
        admission(&handle, 2, Password).unwrap_err(),
        StorageError::LoginClockRegression
    );
    tick.tick.store(100, Ordering::Release);
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(
        complete(&handle, a, Verification::Rejected).unwrap(),
        Completion::Rejected {
            failures: 1,
            wait_seconds: 0
        },
        "rolled-back admission neither leaked a CPU slot nor left a phantom pending failure"
    );
}

#[test]
fn reopening_writer_cannot_create_more_kdfs_while_old_worker_tokens_still_live() {
    let _login_fixture = LOGIN_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    use fc_storage::login::{CredentialKind::Password, Verification};
    let d = Directory::new();
    let wall = Arc::new(Time(Mutex::new(time(30))));
    let (w, mut repo, handle) = open_login(
        &d,
        wall.clone(),
        Arc::new(Tick::default()),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &wall);
    let first = admission(&handle, 2, Password).unwrap();
    let second = admission(&handle, 3, Password).unwrap();
    let state = repo.load_identity().unwrap();
    let revision = handle.try_revision().unwrap().wait().unwrap();
    drop(repo);
    drop(handle);
    drop(w);
    let (_w, _repo, handle) = open_login(&d, wall, Arc::new(Tick::default()), state, revision);
    assert_eq!(
        admission(&handle, 1, Password).unwrap_err(),
        StorageError::LoginCapacity,
        "actual worker permits outlive the old SQLite writer"
    );
    assert_eq!(
        complete(&handle, first, Verification::Verified).unwrap_err(),
        StorageError::LoginReservationExpired,
        "a prior-writer token cannot authenticate in the new generation"
    );
    let admin = admission(&handle, 1, Password).unwrap();
    drop(second);
    complete(&handle, admin, Verification::Rejected).unwrap();
}

#[test]
fn login_kdf_capacity_is_shared_across_different_databases_in_one_process() {
    let _login_fixture = LOGIN_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    use fc_storage::login::{CredentialKind::Password, Verification};
    let first = Directory::new();
    let second = Directory::new();
    let wall = Arc::new(Time(Mutex::new(time(30))));
    let (_first_writer, mut first_repo, first_handle) = open_login(
        &first,
        wall.clone(),
        Arc::new(Tick::default()),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut first_repo, &wall);
    let (_second_writer, mut second_repo, second_handle) = open_login(
        &second,
        wall.clone(),
        Arc::new(Tick::default()),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut second_repo, &wall);
    let a = admission(&first_handle, 2, Password).unwrap();
    let b = admission(&second_handle, 2, Password).unwrap();
    assert_eq!(
        admission(&second_handle, 3, Password).unwrap_err(),
        StorageError::LoginCapacity,
        "two separate databases still share the process-wide physical CPU permits"
    );
    complete(&first_handle, a, Verification::Rejected).unwrap();
    let c = admission(&second_handle, 3, Password).unwrap();
    complete(&second_handle, b, Verification::Rejected).unwrap();
    complete(&second_handle, c, Verification::Rejected).unwrap();
}

#[test]
fn sqlite_content_row_or_receipt_copied_between_owners_is_rejected_before_release() {
    use fc_storage::{CommitCommand, Mutation};
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (mut publisher, reader) = AuthorityStore::new(
        IdentityState::default(),
        Revision::default(),
        AuthEpoch::new(1),
    );
    publisher.initialize().unwrap();
    let writer = StorageWriter::open(d.path(), 16, publisher).unwrap();
    let handle = writer.handle();
    let _joined = Joined(Some(writer));
    let factory_reader = reader.clone();
    let factory = Arc::new(move |s: &IdentityState, r: Revision| {
        factory_reader
            .prepare_update(s.clone(), r, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    let mut repo = BusinessRepository::new(handle.clone(), clock.clone(), factory)
        .with_session_guard(Arc::new(|_, _| Ok(())));
    initialize(&mut repo, &clock);
    let a = actor(2, 2, TrustedOrigin::Web);
    let b = actor(3, 2, TrustedOrigin::Web);
    let acmd = command(510, 2, 0, "a-public");
    let bcmd = command(511, 3, 0, "b-private");
    repo.execute(acmd.clone(), &a, &*clock).unwrap();
    let b_record = repo.execute(bcmd.clone(), &b, &*clock).unwrap();
    let akey = "business/homework/1/2026-10-30/1/2";
    let bkey = "business/homework/1/2026-10-30/1/3";
    let original_a = handle
        .try_get(akey.into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap()
        .value;
    let foreign = handle
        .try_get(bkey.into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap()
        .value;
    // Execute actual SQLite UPDATE via the trusted raw storage boundary to model
    // corruption without defeating the real runtime publication gate.
    let inject = |key: &str, value: Vec<u8>, id: &str| {
        let revision = handle.try_revision().unwrap().wait().unwrap();
        let next = revision.next().unwrap();
        let state = repo.load_identity().unwrap();
        let snapshot = reader
            .prepare_update(state, next, AuthEpoch::new(1))
            .unwrap();
        handle
            .try_commit(
                CommitCommand::new(
                    id.into(),
                    revision,
                    vec![Mutation::Put {
                        key: key.into(),
                        value,
                    }],
                )
                .unwrap(),
                snapshot,
            )
            .unwrap()
            .wait()
            .unwrap();
    };
    inject(akey, foreign, "tamper-owner-row");
    assert_eq!(
        repo.content(acmd.key.clone(), &a),
        Err(StorageError::Database)
    );
    assert_eq!(repo.content(bcmd.key.clone(), &b).unwrap(), Some(b_record));
    let receipt = handle
        .try_get("business/receipt/content/510".into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap()
        .value;
    let tampered = String::from_utf8(receipt)
        .unwrap()
        .replace("\"owner\":2", "\"owner\":3")
        .into_bytes();
    inject(
        "business/receipt/content/510",
        tampered,
        "tamper-owner-receipt",
    );
    let zero_revision = String::from_utf8(original_a.clone())
        .unwrap()
        .replace("\"revision\":1", "\"revision\":0")
        .into_bytes();
    inject(akey, zero_revision, "tamper-zero-revision");
    assert_eq!(
        repo.content(acmd.key.clone(), &a),
        Err(StorageError::Database)
    );
    let oversize = String::from_utf8(original_a)
        .unwrap()
        .replace("a-public", &"x".repeat(16 * 1024 + 1))
        .into_bytes();
    inject(akey, oversize, "tamper-oversize-text");
    assert_eq!(
        repo.content(acmd.key.clone(), &a),
        Err(StorageError::Database)
    );
    assert_eq!(
        repo.execute(acmd, &a, &*clock),
        Err(RepositoryError::Unavailable),
        "a matching command fingerprint cannot release another owner's receipt"
    );
}

#[test]
fn corrupt_one_credential_kind_preserves_healthy_other_kind_and_repairs_only_new_versions() {
    let _login_fixture = LOGIN_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    use fc_storage::login::{
        Completion,
        CredentialKind::{Password, Recovery},
        Verification,
    };
    let d = Directory::new();
    let wall = Arc::new(Time(Mutex::new(time(30))));
    let (_w, mut repo, handle) = open_login(
        &d,
        wall.clone(),
        Arc::new(Tick::default()),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &wall);
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(
        complete(&handle, a, Verification::CorruptRecord).unwrap_err(),
        StorageError::CredentialCorrupt
    );
    let recovery = admission(&handle, 2, Recovery).unwrap();
    assert!(matches!(
        complete(&handle, recovery, Verification::Verified).unwrap(),
        Completion::Authenticated { .. }
    ));
    assert_eq!(
        admission(&handle, 2, Password).unwrap_err(),
        StorageError::CredentialCorrupt,
        "healthy recovery verification must preserve unresolved password repair"
    );
    // Recovery verification alone issues no session here: consume the one-use code
    // and replace BOTH protected versions through the trusted credential transaction.
    let (change, _code) = fc_ports::prepare_recovery(
        &mut FixtureCrypto,
        &(),
        &vec![2, 3, 42],
        &vec![43],
        request(
            920,
            7,
            IdentityCommand::RecoveryConsumed {
                id: principal(2),
                expected_recovery_version: 1,
            },
        ),
    )
    .unwrap();
    repo.execute_credentials(change, None, &*wall).unwrap();
    let a = admission(&handle, 2, Password).unwrap();
    assert_eq!(a.credential_version, 3);
    complete(&handle, a, Verification::Verified).unwrap();
    let bad_recovery = admission(&handle, 3, Recovery).unwrap();
    assert_eq!(
        complete(&handle, bad_recovery, Verification::CorruptRecord).unwrap_err(),
        StorageError::CredentialCorrupt
    );
    let healthy_password = admission(&handle, 3, Password).unwrap();
    complete(&handle, healthy_password, Verification::Verified).unwrap();
    assert_eq!(
        admission(&handle, 3, Recovery).unwrap_err(),
        StorageError::CredentialCorrupt,
        "password login does not clear unresolved recovery repair"
    );
    let (change, _code) = fc_ports::prepare_recovery_rotation(
        &mut FixtureCrypto,
        &vec![3, 2, 42],
        &vec![3, 2, 42],
        request(
            921,
            8,
            IdentityCommand::RecoveryRotated {
                id: principal(3),
                expected_recovery_version: 1,
            },
        ),
    )
    .unwrap();
    repo.execute_credentials(change, Some(&actor(3, 3, TrustedOrigin::Local)), &*wall)
        .unwrap();
    let repaired = admission(&handle, 3, Recovery).unwrap();
    assert_eq!(repaired.recovery_version, 2);
    complete(&handle, repaired, Verification::Verified).unwrap();
}

fn timetable_document(history: bool) -> fc_domain::schedule_document::ScheduleDocument {
    use fc_domain::{schedule::*, schedule_document::*};
    let row = TimetableRow {
        id: RowId::new(1).unwrap(),
        logical_key: LogicalRowKey::new(1).unwrap(),
        group: DayGroup::Morning,
        kind: RowKind::Course,
        range: MinuteRange::new(
            Minute::from_hm(8, 20).unwrap(),
            Minute::from_hm(9, 0).unwrap(),
        )
        .unwrap(),
        enabled_days: [true, false, false, false, false, false, false],
        courses: std::array::from_fn(|day| {
            if day == 0 {
                Some(CourseCell {
                    subject_id: SubjectId::new(1).unwrap(),
                    teacher: TeacherBinding::Explicit(principal(2)),
                })
            } else {
                None
            }
        }),
    };
    let mut plan = SemesterPlan {
        semester: Semester {
            id: SemesterId::new(1).unwrap(),
            class_id: ClassId::new(1).unwrap(),
            revision: Revision::default(),
            enabled: true,
            start: date(1),
            end: date(31),
            first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
            tables: vec![WeekTable {
                id: TableId::new(1).unwrap(),
                role: TableRole::Single,
                rows: vec![row],
            }],
        },
        name: "秋季学期".into(),
        timing_sync: false,
        dual_history: None,
        row_details: vec![RowDetails {
            row_id: RowId::new(1).unwrap(),
            name: "第一节".into(),
        }],
        cell_details: vec![CellDetails {
            row_id: RowId::new(1).unwrap(),
            day: Weekday::Monday,
            room: "201".into(),
            note: "保留教室与备注".into(),
        }],
    };
    if history {
        plan = plan
            .enable_dual(EnableDual::Rebuild {
                odd_id: TableId::new(2).unwrap(),
                even_id: TableId::new(3).unwrap(),
                odd_rows: vec![RowId::new(2).unwrap()],
                even_rows: vec![RowId::new(3).unwrap()],
            })
            .unwrap()
            .disable_dual(TableId::new(2).unwrap())
            .unwrap();
    }
    let mut doc = ScheduleDocument {
        semesters: vec![plan],
        calendar_mode: CalendarMode::School,
        ..ScheduleDocument::default()
    };
    doc.school_calendar.school_coverage = Some((date(1), date(31)));
    doc.school_calendar.entries = vec![
        CalendarEntry {
            actual_semester_id: SemesterId::new(1).unwrap(),
            date: date(30),
            mapping: DayMapping::SourceDate(date(26)),
        },
        CalendarEntry {
            actual_semester_id: SemesterId::new(1).unwrap(),
            date: date(31),
            mapping: DayMapping::Rest,
        },
    ];
    doc.public_calendar.public_verified_years = vec![2026];
    doc.public_calendar.entries = vec![CalendarEntry {
        actual_semester_id: SemesterId::new(1).unwrap(),
        date: date(1),
        mapping: DayMapping::Rest,
    }];
    doc
}
fn schedule_request(
    id: u64,
    revision: u64,
    candidate: fc_domain::schedule_document::ScheduleDocument,
) -> fc_ports::ScheduleRequest {
    fc_ports::ScheduleRequest {
        command_id: CommandId::new(id).unwrap(),
        expected_revision: Revision::new(revision),
        candidate,
    }
}
fn open_schedule_storage(
    d: &Directory,
    clock: Arc<Time>,
    state: IdentityState,
    revision: Revision,
) -> (
    Joined,
    BusinessRepository<Snapshot>,
    fc_runtime::authority::AuthorityReader<IdentityState>,
) {
    let (mut publisher, reader) = AuthorityStore::new(state, revision, AuthEpoch::new(1));
    publisher.initialize().unwrap();
    let writer = StorageWriter::open(d.path(), 16, publisher).unwrap();
    let handle = writer.handle();
    let factory_reader = reader.clone();
    let factory = Arc::new(move |state: &IdentityState, revision: Revision| {
        factory_reader
            .prepare_update(state.clone(), revision, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    (
        Joined(Some(writer)),
        BusinessRepository::new(handle, clock, factory).with_session_guard(Arc::new(|_, _| Ok(()))),
        reader,
    )
}
#[test]
fn timetable_storage_uses_independent_cas_persists_both_calendars_history_and_atomic_receipts() {
    use fc_domain::schedule_document::{DocumentErrorKind, ScheduleDocument};
    use fc_ports::{ScheduleRepository, ScheduleRepositoryError};
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (w, mut repo, _reader) = open_schedule_storage(
        &d,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let admin = actor(1, 1, TrustedOrigin::Local);
    assert_eq!(repo.schedule(&admin).unwrap(), ScheduleDocument::default());
    let request = schedule_request(3000, 0, timetable_document(true));
    let first = repo
        .execute_schedule(request.clone(), &admin, &*clock)
        .unwrap();
    assert_eq!(first.revision, Revision::new(1));
    assert_eq!(first.semesters[0].semester.revision, Revision::new(1));
    assert_eq!(
        first.semesters[0]
            .dual_history
            .as_ref()
            .unwrap()
            .cell_details
            .len(),
        2
    );
    assert_eq!(repo.load_identity().unwrap().revision, Revision::new(7));
    let global =
        w.0.as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap();
    assert_eq!(global, Revision::new(8));
    assert_eq!(
        repo.execute_schedule(request.clone(), &admin, &*clock)
            .unwrap(),
        first
    );
    assert_eq!(
        w.0.as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap(),
        global
    );
    let mut changed = request.clone();
    changed.candidate.automatic_gaps = false;
    assert_eq!(
        repo.execute_schedule(changed, &admin, &*clock),
        Err(ScheduleRepositoryError::IdempotencyConflict)
    );
    assert!(
        matches!(repo.execute_schedule(schedule_request(3001,0,first.clone()),&admin,&*clock),Err(ScheduleRepositoryError::Domain(e))if matches!(e.kind,DocumentErrorKind::RevisionConflict{current}if current==Revision::new(1)))
    );
    assert!(repo.schedule(&actor(2, 2, TrustedOrigin::Local)).is_err());
    assert!(repo.schedule(&actor(1, 1, TrustedOrigin::Web)).is_err());
    repo.execute(
        command(3002, 2, 0, "unrelated homework"),
        &actor(2, 2, TrustedOrigin::Web),
        &*clock,
    )
    .unwrap();
    let mut candidate = first.clone();
    candidate.automatic_gaps = false;
    let second = repo
        .execute_schedule(schedule_request(3003, 1, candidate), &admin, &*clock)
        .unwrap();
    assert_eq!(second.revision, Revision::new(2));
    assert_eq!(second.semesters[0].semester.revision, Revision::new(1));
    assert_eq!(second.rules_revision, Revision::new(2));
    assert_eq!(second.school_calendar, first.school_calendar);
    assert_eq!(second.public_calendar, first.public_calendar);
    let state = repo.load_identity().unwrap();
    let global =
        w.0.as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap();
    drop(repo);
    drop(w);
    let facts = fc_storage::business::startup_state(d.path()).unwrap();
    assert_eq!(facts.identity, state);
    assert_eq!(facts.revision, global);
    let (_w, mut restored, _) =
        open_schedule_storage(&d, clock.clone(), facts.identity, facts.revision);
    assert_eq!(restored.schedule(&admin).unwrap(), second);
    assert_eq!(
        restored.execute_schedule(request, &admin, &*clock).unwrap(),
        first
    );
    let restored_dual = second.semesters[0]
        .enable_dual(fc_domain::schedule_document::EnableDual::RestoreHistory)
        .unwrap();
    assert_eq!(restored_dual.cell_details.len(), 2);
    assert_eq!(restored_dual.semester.tables.len(), 2);
}
#[test]
fn timetable_storage_preserves_id_high_water_after_deletion_and_reopen() {
    use fc_domain::schedule_document::DraftEdit;
    use fc_ports::ScheduleRepository;
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (writer, mut repo, _) = open_schedule_storage(
        &d,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let admin = actor(1, 1, TrustedOrigin::Local);
    let mut candidate = timetable_document(false);
    let plan = &mut candidate.semesters[0];
    let highest = RowId::new(900).unwrap();
    plan.semester.tables[0].rows[0].id = highest;
    plan.semester.tables[0].rows[0].logical_key = LogicalRowKey::new(900).unwrap();
    plan.row_details[0].row_id = highest;
    plan.cell_details[0].row_id = highest;
    let saved = repo
        .execute_schedule(schedule_request(3500, 0, candidate), &admin, &*clock)
        .unwrap();
    assert_eq!(saved.id_high_water, 900);
    let old_row = saved.semesters[0].semester.tables[0].rows[0].clone();
    let identity = repo.load_identity().unwrap();
    let mut deletion = saved
        .edit(
            &DraftEdit::RemoveRow {
                semester_id: saved.semesters[0].semester.id,
                table_id: saved.semesters[0].semester.tables[0].id,
                row_id: highest,
            },
            &identity,
        )
        .unwrap();
    deletion.id_high_water = 0; // An untrusted candidate cannot roll back the allocator.
    let removed = repo
        .execute_schedule(schedule_request(3501, 1, deletion), &admin, &*clock)
        .unwrap();
    assert_eq!(removed.id_high_water, 900);
    assert!(removed.semesters[0].semester.tables[0].rows.is_empty());
    drop(repo);
    drop(writer);
    let facts = fc_storage::business::startup_state(d.path()).unwrap();
    let (_writer, mut repo, _) =
        open_schedule_storage(&d, clock.clone(), facts.identity, facts.revision);
    let mut restored = repo.schedule(&admin).unwrap();
    assert_eq!(restored.id_high_water, 900);
    let mut reused = restored.clone();
    reused.semesters[0].semester.tables[0]
        .rows
        .push(old_row.clone());
    assert!(matches!(
        repo.execute_schedule(schedule_request(3503, 2, reused), &admin, &*clock),
        Err(fc_ports::ScheduleRepositoryError::Domain(
            fc_domain::schedule_document::ScheduleDocumentError {
                kind: fc_domain::schedule_document::DocumentErrorKind::RetiredIdentity,
                ..
            }
        ))
    ));
    assert_eq!(repo.schedule(&admin).unwrap(), restored);
    let fresh = restored.allocate_id().unwrap();
    assert_eq!(fresh, 901);
    let mut row = old_row;
    row.id = RowId::new(fresh).unwrap();
    row.logical_key = LogicalRowKey::new(fresh).unwrap();
    let mut inserted = restored
        .edit(
            &DraftEdit::InsertRow {
                semester_id: restored.semesters[0].semester.id,
                table_id: restored.semesters[0].semester.tables[0].id,
                index: 0,
                row,
            },
            &repo.load_identity().unwrap(),
        )
        .unwrap();
    inserted.id_high_water = u64::MAX; // Forged draft metadata cannot exhaust the allocator.
    let saved = repo
        .execute_schedule(schedule_request(3502, 2, inserted), &admin, &*clock)
        .unwrap();
    assert_eq!(saved.id_high_water, 901);
    assert_eq!(saved.semesters[0].semester.tables[0].rows[0].id.get(), 901);
    assert_eq!(
        saved.semesters[0].semester.tables[0].rows[0]
            .logical_key
            .get(),
        901
    );
}

#[test]
fn timetable_storage_rejects_invalid_references_catalog_revocation_and_silent_default_switch() {
    use fc_domain::schedule::TeacherBinding;
    use fc_ports::ScheduleRepository;
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (_w, mut repo, _) = open_schedule_storage(
        &d,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let admin = actor(1, 1, TrustedOrigin::Local);
    let first = repo
        .execute_schedule(
            schedule_request(3100, 0, timetable_document(false)),
            &admin,
            &*clock,
        )
        .unwrap();
    let mut broken = first.clone();
    broken.semesters[0].semester.tables[0].rows[0].courses[0]
        .as_mut()
        .unwrap()
        .teacher = TeacherBinding::Explicit(principal(99));
    assert!(repo
        .execute_schedule(schedule_request(3101, 1, broken), &admin, &*clock)
        .is_err());
    assert_eq!(repo.schedule(&admin).unwrap(), first);
    let disabled = request(
        3102,
        7,
        IdentityCommand::PutSubject {
            id: SubjectId::new(1).unwrap(),
            name: "Math".into(),
            enabled: false,
            color: Rgb {
                red: 40,
                green: 80,
                blue: 160,
            },
            expected_revision: Some(Revision::new(1)),
        },
    );
    assert_eq!(
        repo.execute_identity(disabled, Some(&admin), &*clock),
        Err(IdentityRepositoryError::Domain(IdentityError::InvalidScope))
    );
    assert!(repo.load_identity().unwrap().subjects[0].enabled);
    repo.execute_identity(
        request(
            3103,
            7,
            IdentityCommand::SetDefaultTeacher(DefaultTeacher {
                class_id: ClassId::new(1).unwrap(),
                subject_id: SubjectId::new(1).unwrap(),
                teacher_id: principal(2),
            }),
        ),
        Some(&admin),
        &*clock,
    )
    .unwrap();
    let mut by_default = first.clone();
    by_default.semesters[0].semester.tables[0].rows[0].courses[0]
        .as_mut()
        .unwrap()
        .teacher = TeacherBinding::Default;
    let saved = repo
        .execute_schedule(schedule_request(3104, 1, by_default), &admin, &*clock)
        .unwrap();
    assert_eq!(
        repo.execute_identity(
            request(
                3105,
                8,
                IdentityCommand::SetDefaultTeacher(DefaultTeacher {
                    class_id: ClassId::new(1).unwrap(),
                    subject_id: SubjectId::new(1).unwrap(),
                    teacher_id: principal(3)
                })
            ),
            Some(&admin),
            &*clock
        ),
        Err(IdentityRepositoryError::Domain(IdentityError::InvalidScope))
    );
    assert_eq!(repo.schedule(&admin).unwrap(), saved);
}
#[test]
fn timetable_codec_rejects_tampered_unknown_duplicate_revision_and_oversize_without_truncation() {
    use fc_domain::{schedule::*, schedule_document::*};
    use fc_ports::{ScheduleRepository, ScheduleRepositoryError};
    use fc_storage::{CommitCommand, Mutation};
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (w, mut repo, reader) = open_schedule_storage(
        &d,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let admin = actor(1, 1, TrustedOrigin::Local);
    let first = repo
        .execute_schedule(
            schedule_request(3200, 0, timetable_document(false)),
            &admin,
            &*clock,
        )
        .unwrap();
    let handle = w.0.as_ref().unwrap().handle();
    let original = handle
        .try_get("business/schedule/v1".into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap()
        .value;
    let inject = |bytes: Vec<u8>, id: &str| {
        let revision = handle.try_revision().unwrap().wait().unwrap();
        let state = repo.load_identity().unwrap();
        let snapshot = reader
            .prepare_update(state, revision.next().unwrap(), AuthEpoch::new(1))
            .unwrap();
        handle
            .try_commit(
                CommitCommand::new(
                    id.into(),
                    revision,
                    vec![Mutation::Put {
                        key: "business/schedule/v1".into(),
                        value: bytes,
                    }],
                )
                .unwrap(),
                snapshot,
            )
            .unwrap()
            .wait()
            .unwrap();
    };
    for (field, id) in [
        ("\"unknown\":1,", "unknown-schedule"),
        ("\"format\":1,", "duplicate-schedule"),
    ] {
        let mut bytes = b"{".to_vec();
        bytes.extend_from_slice(field.as_bytes());
        bytes.extend_from_slice(&original[1..]);
        inject(bytes, id);
        assert_eq!(
            repo.schedule(&admin),
            Err(ScheduleRepositoryError::Unavailable)
        );
        assert!(matches!(
            fc_storage::business::startup_state(d.path()),
            Err(StorageError::Database)
        ));
    }
    let bad_revision = String::from_utf8(original.clone())
        .unwrap()
        .replace("\"revision\":1", "\"revision\":0")
        .into_bytes();
    inject(bad_revision, "bad-schedule-revision");
    assert_eq!(
        repo.schedule(&admin),
        Err(ScheduleRepositoryError::Unavailable)
    );
    let without_allocator = String::from_utf8(original.clone())
        .unwrap()
        .replace("\"id_high_water\":1,", "")
        .into_bytes();
    assert_ne!(without_allocator, original);
    inject(without_allocator, "missing-schedule-id-high-water");
    assert_eq!(
        repo.schedule(&admin),
        Err(ScheduleRepositoryError::Unavailable)
    );
    inject(original.clone(), "restore-schedule-fixture");
    let mut over_capacity = first.clone();
    over_capacity.semesters = vec![first.semesters[0].clone(); 33];
    assert!(matches!(
        repo.execute_schedule(schedule_request(3202, 1, over_capacity), &admin, &*clock),
        Err(ScheduleRepositoryError::Domain(ScheduleDocumentError {
            kind: DocumentErrorKind::Capacity,
            ..
        }))
    ));
    let mut huge = first.clone();
    let plan = &mut huge.semesters[0];
    plan.semester.tables[0].rows.clear();
    plan.row_details.clear();
    plan.cell_details.clear();
    for index in 0..128 {
        let id = RowId::new(1000 + index).unwrap();
        plan.semester.tables[0].rows.push(TimetableRow {
            id,
            logical_key: LogicalRowKey::new(1000 + index).unwrap(),
            group: DayGroup::Morning,
            kind: RowKind::Course,
            range: MinuteRange::new(
                Minute::new(index as u16).unwrap(),
                Minute::new(index as u16 + 1).unwrap(),
            )
            .unwrap(),
            enabled_days: [true; 7],
            courses: std::array::from_fn(|_| {
                Some(CourseCell {
                    subject_id: SubjectId::new(1).unwrap(),
                    teacher: TeacherBinding::Explicit(principal(2)),
                })
            }),
        });
        for day in 0..7 {
            plan.cell_details.push(CellDetails {
                row_id: id,
                day: Weekday::from_monday_index(day).unwrap(),
                room: "201".into(),
                note: "n".repeat(200),
            });
        }
    }
    assert_eq!(
        repo.execute_schedule(schedule_request(3201, 1, huge), &admin, &*clock),
        Err(ScheduleRepositoryError::DocumentTooLarge)
    );
    assert_eq!(repo.schedule(&admin).unwrap(), first);
    // A valid document of the same revision cannot replace the original command result.
    let mut replaced_result = String::from_utf8(original).unwrap();
    assert!(replaced_result.contains("\"automatic_gaps\":true"));
    replaced_result =
        replaced_result.replace("\"automatic_gaps\":true", "\"automatic_gaps\":false");
    let revision = handle.try_revision().unwrap().wait().unwrap();
    let snapshot = reader
        .prepare_update(
            repo.load_identity().unwrap(),
            revision.next().unwrap(),
            AuthEpoch::new(1),
        )
        .unwrap();
    handle
        .try_commit(
            CommitCommand::new(
                "replace-schedule-result".into(),
                revision,
                vec![Mutation::Put {
                    key: "business/result/schedule/3200".into(),
                    value: replaced_result.into_bytes(),
                }],
            )
            .unwrap(),
            snapshot,
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        repo.execute_schedule(
            schedule_request(3200, 0, timetable_document(false)),
            &admin,
            &*clock,
        ),
        Err(ScheduleRepositoryError::Unavailable)
    );
    assert_eq!(repo.schedule(&admin).unwrap(), first);
}

#[test]
fn timetable_commit_rechecks_captured_session_after_preparation_and_rolls_back_every_row() {
    use fc_ports::ScheduleRepository;
    use std::sync::atomic::AtomicBool;
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let armed = Arc::new(AtomicBool::new(false));
    let allowed = Arc::new(AtomicBool::new(true));
    let (mut publisher, reader) = AuthorityStore::new(
        IdentityState::default(),
        Revision::default(),
        AuthEpoch::new(1),
    );
    publisher.initialize().unwrap();
    let gate = reader.gate();
    let writer = StorageWriter::open(d.path(), 16, publisher).unwrap();
    let handle = writer.handle();
    let _joined = Joined(Some(writer));
    let arm = armed.clone();
    let permit = allowed.clone();
    let factory = Arc::new(move |identity: &IdentityState, revision: Revision| {
        if arm.load(Ordering::Acquire) {
            permit.store(false, Ordering::Release);
        }
        reader
            .prepare_update(identity.clone(), revision, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    let permit = allowed.clone();
    let mut repo = BusinessRepository::new(handle.clone(), clock.clone(), factory)
        .with_session_guard(Arc::new(move |_, _| {
            if permit.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(StorageError::AuthorityUnavailable)
            }
        }));
    initialize(&mut repo, &clock);
    let before = handle.try_revision().unwrap().wait().unwrap();
    let admin = actor(1, 1, TrustedOrigin::Local);
    armed.store(true, Ordering::Release);
    let request = schedule_request(3300, 0, timetable_document(false));
    assert!(repo
        .execute_schedule(request.clone(), &admin, &*clock)
        .is_err());
    assert_eq!(gate.status().health, fc_runtime::health::Health::Healthy);
    assert_eq!(handle.try_revision().unwrap().wait().unwrap(), before);
    for key in [
        "business/schedule/v1",
        "business/receipt/schedule/3300",
        "business/result/schedule/3300",
    ] {
        assert_eq!(handle.try_get(key.into()).unwrap().wait().unwrap(), None);
    }
    armed.store(false, Ordering::Release);
    allowed.store(true, Ordering::Release);
    assert_eq!(
        repo.execute_schedule(request, &admin, &*clock)
            .unwrap()
            .revision,
        Revision::new(1)
    );
}
#[test]
fn timetable_commit_publication_failure_blocks_old_authority_and_reopens_committed_receipt() {
    use fc_ports::{AuthorityPublisher as Port, ScheduleRepository, ScheduleRepositoryError};
    use fc_runtime::{
        authority::{AuthorityPublisher, AuthorityUpdateToken},
        health::{GateError, Health},
    };
    struct Broken(AuthorityPublisher<IdentityState>);
    impl Port for Broken {
        type Snapshot = Snapshot;
        type Update = AuthorityUpdateToken;
        type Error = GateError;
        fn snapshot_revision(&self, s: &Snapshot) -> Revision {
            self.0.snapshot_revision(s)
        }
        fn begin_update(&mut self) -> Result<Self::Update, GateError> {
            self.0.begin_update()
        }
        fn publish(&mut self, _t: Self::Update, _s: Snapshot) -> Result<(), GateError> {
            Err(GateError::Faulted)
        }
        fn rollback(&mut self, t: Self::Update) -> Result<(), GateError> {
            self.0.rollback(t)
        }
        fn fault(&mut self) {
            self.0.fault()
        }
    }
    let d = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (w, mut repo, _) = open_schedule_storage(
        &d,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let facts = fc_storage::business::StartupState {
        identity: repo.load_identity().unwrap(),
        revision: w
            .0
            .as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap(),
    };
    drop(repo);
    drop(w);
    let (mut publisher, reader) =
        AuthorityStore::new(facts.identity.clone(), facts.revision, AuthEpoch::new(1));
    publisher.initialize().unwrap();
    let gate = reader.gate();
    let writer = StorageWriter::open(d.path(), 16, Broken(publisher)).unwrap();
    let handle = writer.handle();
    let joined = Joined(Some(writer));
    let factory = Arc::new(move |state: &IdentityState, revision: Revision| {
        reader
            .prepare_update(state.clone(), revision, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    let mut repo = BusinessRepository::new(handle, clock.clone(), factory)
        .with_session_guard(Arc::new(|_, _| Ok(())));
    let admin = actor(1, 1, TrustedOrigin::Local);
    let request = schedule_request(3400, 0, timetable_document(true));
    assert_eq!(
        repo.execute_schedule(request.clone(), &admin, &*clock),
        Err(ScheduleRepositoryError::Faulted)
    );
    assert_eq!(gate.status().health, Health::Faulted);
    assert_eq!(repo.schedule(&admin), Err(ScheduleRepositoryError::Faulted));
    drop(repo);
    drop(joined);
    let facts = fc_storage::business::startup_state(d.path()).unwrap();
    assert_eq!(facts.revision, Revision::new(8));
    let (w, mut repo, _) = open_schedule_storage(&d, clock.clone(), facts.identity, facts.revision);
    let committed = repo.schedule(&admin).unwrap();
    assert_eq!(committed.revision, Revision::new(1));
    assert_eq!(
        repo.execute_schedule(request, &admin, &*clock).unwrap(),
        committed
    );
    assert_eq!(
        w.0.as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap(),
        facts.revision
    );
}
