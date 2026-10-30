//! Self logout crosses the real writer, SQLite transaction and durable epoch boundary.
use super::*;
use fc_domain::time::{LocalDate, LocalDateTime};
use std::sync::atomic::AtomicU64;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "FutureClassroom-Logout-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        Self(directory)
    }
    fn path(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("logout fixture cleanup");
    }
}
fn now() -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 3600).unwrap()
}
struct Time;
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        now()
    }
}
struct Publisher {
    active: Arc<AtomicBool>,
    revoke_at_update: Arc<AtomicBool>,
}
impl fc_ports::AuthorityPublisher for Publisher {
    type Snapshot = Revision;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, snapshot: &Revision) -> Revision {
        *snapshot
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        if self.revoke_at_update.swap(false, Ordering::AcqRel) {
            self.active.store(false, Ordering::Release);
        }
        Ok(())
    }
    fn publish(&mut self, _: (), _: Revision) -> Result<(), ()> {
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn fault(&mut self) {}
}
fn principal(id: u64) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn actor(state: &IdentityState, id: u64, origin: TrustedOrigin) -> ActorContext {
    ActorContext {
        principal_id: principal(id),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: state.account(principal(id)).unwrap().principal.auth_epoch,
        expires_at: LocalDateTime::new(now().date, 7200).unwrap(),
        origin,
    }
}
fn request(id: u64, state: &IdentityState, command: IdentityCommand) -> IdentityRequest {
    IdentityRequest {
        command_id: CommandId::new(id).unwrap(),
        expected_revision: state.revision,
        command,
    }
}
fn logout(id: PrincipalId, epoch: AuthEpoch) -> IdentityCommand {
    IdentityCommand::LogoutAccount {
        id,
        expected_auth_epoch: epoch,
    }
}
struct Fixture {
    writer: Option<StorageWriter<Revision>>,
    repository: BusinessRepository<Revision>,
    directory: Directory,
    active: Arc<AtomicBool>,
    revoke_at_update: Arc<AtomicBool>,
}
impl Fixture {
    fn open(
        directory: &Directory,
        active: Arc<AtomicBool>,
        revoke_at_update: Arc<AtomicBool>,
    ) -> (StorageWriter<Revision>, BusinessRepository<Revision>) {
        let writer = StorageWriter::open(
            directory.path(),
            8,
            Publisher {
                active: active.clone(),
                revoke_at_update,
            },
        )
        .unwrap();
        let repository = BusinessRepository::new(
            writer.handle(),
            Arc::new(Time),
            Arc::new(|_, revision| Ok(revision)),
        )
        .with_session_guard(Arc::new(move |state, actor| {
            if !active.load(Ordering::Acquire) {
                return Err(StorageError::AuthorityUnavailable);
            }
            state
                .authority(actor.principal_id)
                .map_err(StorageError::Identity)?
                .validate_actor(actor, now())
                .map_err(|e| StorageError::Identity(IdentityError::Permission(e)))
        }));
        (writer, repository)
    }
    fn new(complete_initial_password: bool) -> Self {
        let directory = Directory::new();
        let active = Arc::new(AtomicBool::new(true));
        let revoke_at_update = Arc::new(AtomicBool::new(false));
        let (writer, mut repository) =
            Self::open(&directory, active.clone(), revoke_at_update.clone());
        let empty = repository.load_identity().unwrap();
        let admin = repository
            .execute_identity_with_credentials(
                request(
                    1,
                    &empty,
                    IdentityCommand::FirstAdministrator {
                        school_id: SchoolId::new(1).unwrap(),
                        school_name: "学校".into(),
                        id: principal(1),
                        login_name: "admin".into(),
                        display_name: "管理员".into(),
                    },
                ),
                None,
                PreparedCredentials {
                    principal_id: principal(1),
                    password: Some((1, vec![11])),
                    recovery: Some((1, vec![12])),
                },
            )
            .unwrap()
            .next;
        let admin_actor = actor(&admin, 1, TrustedOrigin::Local);
        let user = repository
            .execute_identity_with_credentials(
                request(
                    2,
                    &admin,
                    IdentityCommand::CreateAccount(NewAccount {
                        id: principal(2),
                        login_name: "user".into(),
                        display_name: "用户".into(),
                        role: Role::User,
                        scopes: vec![],
                    }),
                ),
                Some(&admin_actor),
                PreparedCredentials {
                    principal_id: principal(2),
                    password: Some((1, vec![21])),
                    recovery: Some((1, vec![22])),
                },
            )
            .unwrap()
            .next;
        if complete_initial_password {
            let user_actor = actor(&user, 2, TrustedOrigin::Web);
            repository
                .execute_identity_with_credentials(
                    request(
                        3,
                        &user,
                        IdentityCommand::PasswordChanged {
                            id: principal(2),
                            expected_credential_version: 1,
                        },
                    ),
                    Some(&user_actor),
                    PreparedCredentials {
                        principal_id: principal(2),
                        password: Some((2, vec![23])),
                        recovery: None,
                    },
                )
                .unwrap();
        }
        Self {
            writer: Some(writer),
            repository,
            directory,
            active,
            revoke_at_update,
        }
    }
    fn reopen(&mut self) {
        self.writer.take().unwrap().shutdown().unwrap();
        let (writer, repository) = Self::open(
            &self.directory,
            self.active.clone(),
            self.revoke_at_update.clone(),
        );
        self.writer = Some(writer);
        self.repository = repository;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            writer.shutdown().expect("logout writer shutdown");
        }
    }
}

#[test]
fn logout_sqlite_epoch_survives_restart_and_old_local_and_web_actors_stay_revoked() {
    for origin in [TrustedOrigin::Local, TrustedOrigin::Web] {
        let mut fixture = Fixture::new(true);
        let before = fixture.repository.load_identity().unwrap();
        let old_local = actor(&before, 2, TrustedOrigin::Local);
        let old_web = actor(&before, 2, TrustedOrigin::Web);
        let initiator = actor(&before, 2, origin);
        let command = request(4, &before, logout(principal(2), initiator.auth_epoch));
        let transition = fixture
            .repository
            .execute_identity(command.clone(), Some(&initiator), &Time)
            .unwrap();
        assert_eq!(
            transition.events,
            vec![IdentityEvent::AuthorizationRevoked(principal(2))]
        );
        assert_eq!(transition.next.revision, before.revision.next().unwrap());
        assert_eq!(transition.next.accounts[0], before.accounts[0]);
        assert_eq!(transition.next.accounts[1].credential_version, 2);
        assert_eq!(transition.next.accounts[1].recovery_version, 1);
        assert_eq!(
            transition.next.accounts[1].principal.auth_epoch,
            initiator.auth_epoch.next().unwrap()
        );
        assert_eq!(fixture.repository.load_identity().unwrap(), transition.next);
        for old in [&old_local, &old_web] {
            assert_eq!(
                fixture
                    .repository
                    .execute_identity(command.clone(), Some(old), &Time),
                Err(IdentityRepositoryError::Domain(IdentityError::Permission(
                    PermissionError::StaleAuthorization
                )))
            );
        }
        fixture.reopen();
        let reopened = fixture.repository.load_identity().unwrap();
        assert_eq!(reopened, transition.next);
        for old in [&old_local, &old_web] {
            assert_eq!(
                fixture
                    .repository
                    .execute_identity(command.clone(), Some(old), &Time),
                Err(IdentityRepositoryError::Domain(IdentityError::Permission(
                    PermissionError::StaleAuthorization
                )))
            );
        }
        let new = actor(&reopened, 2, origin);
        assert_eq!(
            fixture
                .repository
                .execute_identity(command, Some(&new), &Time),
            Err(IdentityRepositoryError::IdempotencyConflict),
            "a receipt cannot be transplanted to a different actor epoch"
        );
        assert_eq!(fixture.repository.load_identity().unwrap(), reopened);
        fixture
            .repository
            .execute_identity(
                request(5, &reopened, logout(principal(2), new.auth_epoch)),
                Some(&new),
                &Time,
            )
            .unwrap();
    }
}

#[test]
fn logout_sqlite_rejects_other_accounts_stale_expected_epoch_and_initial_password() {
    let mut fixture = Fixture::new(true);
    let before = fixture.repository.load_identity().unwrap();
    let user = actor(&before, 2, TrustedOrigin::Web);
    for (id, command, expected) in [
        (
            4,
            logout(principal(1), before.accounts[0].principal.auth_epoch),
            IdentityError::Permission(PermissionError::WrongOwner),
        ),
        (
            5,
            logout(principal(2), AuthEpoch::new(1)),
            IdentityError::RevisionConflict,
        ),
    ] {
        assert_eq!(
            fixture
                .repository
                .execute_identity(request(id, &before, command), Some(&user), &Time),
            Err(IdentityRepositoryError::Domain(expected))
        );
        assert_eq!(fixture.repository.load_identity().unwrap(), before);
    }
    let mut initial = Fixture::new(false);
    let before = initial.repository.load_identity().unwrap();
    let user = actor(&before, 2, TrustedOrigin::Local);
    assert_eq!(
        initial.repository.execute_identity(
            request(3, &before, logout(principal(2), user.auth_epoch)),
            Some(&user),
            &Time
        ),
        Err(IdentityRepositoryError::Domain(
            IdentityError::PasswordChangeRequired
        ))
    );
    assert_eq!(initial.repository.load_identity().unwrap(), before);
}

#[test]
fn logout_sqlite_revalidates_live_session_after_authority_update_before_commit() {
    let mut fixture = Fixture::new(true);
    let before = fixture.repository.load_identity().unwrap();
    let user = actor(&before, 2, TrustedOrigin::Web);
    fixture.revoke_at_update.store(true, Ordering::Release);
    let command = request(4, &before, logout(principal(2), user.auth_epoch));
    assert_eq!(
        fixture
            .repository
            .execute_identity(command.clone(), Some(&user), &Time),
        Err(IdentityRepositoryError::Faulted)
    );
    assert_eq!(fixture.repository.load_identity().unwrap(), before);
    let db = Connection::open(fixture.directory.path()).unwrap();
    let receipts: u64 = db
        .query_row(
            "SELECT count(*) FROM documents WHERE key='business/receipt/identity/4'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipts, 0);
    drop(db);
    fixture.active.store(true, Ordering::Release);
    fixture
        .repository
        .execute_identity(command, Some(&user), &Time)
        .unwrap();
}
