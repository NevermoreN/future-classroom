//! Account-management integration on the real Backend/sole SQLite writer.
//! Metadata fixtures use synthetic HMAC-protected credentials and trusted issued
//! test leases, never claim password authentication. One test creates an account
//! through the production PBKDF2 path. Windows protects/reopens the codec key
//! with actual current-user DPAPI; host uses the explicitly fake protector below.
use super::*;
use fc_ports::{CredentialError, CredentialIdentityRepository};
use fc_runtime::health::Health;
use std::{
    path::PathBuf,
    sync::atomic::AtomicU64,
    time::{SystemTime, UNIX_EPOCH},
};

static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ClockFixture;
impl Clock for ClockFixture {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 8 * 3600 + 30 * 60).unwrap()
    }
}
impl MonotonicClock for ClockFixture {
    fn monotonic_millis(&self) -> u64 {
        1_000
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self::under(std::env::temp_dir())
    }
    fn under(root: PathBuf) -> Self {
        let path = root.join(format!(
            "FutureClassroom-Accounts-R15-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }
    fn database(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
    fn codec(&self) -> CredentialRecordCodec {
        let path = self.0.join("test-codec-key.protected");
        if !path.exists() {
            let codec = CredentialRecordCodec::from_key([103; 32]).unwrap();
            #[cfg(windows)]
            let protected = codec.protect_key(|key| {
                use fc_windows::system::{CurrentUserDpapi, SecretBytes};
                CurrentUserDpapi::default()
                    .protect(&SecretBytes::new(key.to_vec()).unwrap())
                    .unwrap()
            });
            #[cfg(not(windows))]
            let protected = codec.protect_key(FakeKeyProtector::protect);
            std::fs::write(&path, protected).unwrap();
        }
        let protected = std::fs::read(path).unwrap();
        #[cfg(windows)]
        let key = {
            let clear = fc_windows::system::CurrentUserDpapi::default()
                .unprotect(&protected)
                .unwrap();
            <[u8; 32]>::try_from(clear.expose()).unwrap()
        };
        #[cfg(not(windows))]
        let key = FakeKeyProtector::unprotect(&protected);
        CredentialRecordCodec::from_key(key).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("account-management private fixture cleanup");
    }
}
#[cfg(not(windows))]
struct FakeKeyProtector;
#[cfg(not(windows))]
impl FakeKeyProtector {
    // Synthetic fixture transport only, explicitly NOT encryption or DPAPI.
    fn protect(key: &[u8]) -> Vec<u8> {
        key.iter().map(|byte| byte ^ 0x5a).collect()
    }
    fn unprotect(value: &[u8]) -> [u8; 32] {
        <[u8; 32]>::try_from(value.iter().map(|byte| byte ^ 0x5a).collect::<Vec<_>>()).unwrap()
    }
}
struct FixtureRandom(u8);
impl fc_ports::RandomPort for FixtureRandom {
    type Error = ();
    fn fill(&mut self, bytes: &mut [u8]) -> std::result::Result<(), ()> {
        self.0 += 1;
        bytes.fill(self.0);
        Ok(())
    }
}
struct SyntheticCredentials;
impl CredentialCrypto for SyntheticCredentials {
    type Secret = ();
    type PasswordRecord = PasswordRecord;
    type RecoveryCode = RecoveryCode;
    type RecoveryRecord = RecoveryRecord;
    fn password_record(&mut self, _: &()) -> std::result::Result<PasswordRecord, CredentialError> {
        PasswordRecord::from_parts(
            fc_security::PASSWORD_FORMAT,
            fc_security::PBKDF2_ITERATIONS,
            [23; 32],
            [42; 32],
        )
        .map_err(|_| CredentialError::Unavailable)
    }
    fn verify_password(
        &mut self,
        _: &(),
        _: &PasswordRecord,
    ) -> std::result::Result<bool, CredentialError> {
        Ok(true)
    }
    fn recovery_record(
        &mut self,
    ) -> std::result::Result<(RecoveryCode, RecoveryRecord), CredentialError> {
        fc_security::create_recovery_record(&mut FixtureRandom(10))
            .map_err(|_| CredentialError::Unavailable)
    }
    fn verify_recovery(
        &mut self,
        code: &RecoveryCode,
        record: &RecoveryRecord,
    ) -> std::result::Result<bool, CredentialError> {
        fc_security::verify_recovery(code, record).map_err(|_| CredentialError::Unavailable)
    }
}
fn id(value: u64) -> PrincipalId {
    PrincipalId::new(value).unwrap()
}
fn scope(class: u64, subject: u64) -> AssignmentScope {
    AssignmentScope {
        class_id: ClassId::new(class).unwrap(),
        subject_id: SubjectId::new(subject).unwrap(),
    }
}
fn update(
    target: u64,
    name: &str,
    role: Role,
    enabled: bool,
    scopes: Vec<AssignmentScope>,
) -> IdentityCommand {
    IdentityCommand::UpdateAccount {
        id: id(target),
        display_name: name.into(),
        role,
        enabled,
        scopes,
    }
}
fn identity_error(error: IdentityError) -> BackendError {
    BackendError::Identity(IdentityRepositoryError::Domain(error))
}
struct Fixture {
    backend: Backend,
    directory: Directory,
    clock: Arc<ClockFixture>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_directory(Directory::new())
    }
    fn control() -> Self {
        // This case tests exact revocation and the real 100ms voice control
        // protocol. Disk/reopen cases keep ordinary TEMP; Windows does too.
        // NAS btrfs latency can consume the control budget independently of
        // permission decisions, as covered by the voice module's own fixtures.
        #[cfg(target_os = "linux")]
        if std::path::Path::new("/dev/shm").is_dir() {
            return Self::with_directory(Directory::under(PathBuf::from("/dev/shm")));
        }
        Self::new()
    }
    fn with_directory(directory: Directory) -> Self {
        let clock = Arc::new(ClockFixture);
        let backend = Backend::open(
            directory.database(),
            directory.codec(),
            clock.clone(),
            clock.clone(),
        )
        .unwrap();
        let mut fixture = Self {
            backend,
            directory,
            clock,
        };
        fixture.seed(
            None,
            IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "Synthetic account school".into(),
                id: id(1),
                login_name: "admin".into(),
                display_name: "Admin".into(),
            },
        );
        let admin = fixture.issue(1, TrustedOrigin::Local);
        for n in 1..=2 {
            fixture
                .backend
                .test_native_command(
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(admin),
                    fc_application::identity::SessionPurpose::Manage,
                    |issued_backend| {
                        issued_backend.identity(
                            admin,
                            IdentityCommand::CreateClass {
                                id: ClassId::new(n).unwrap(),
                                name: format!("Class {n}"),
                            },
                        )
                    },
                )
                .unwrap();
            fixture
                .backend
                .test_native_command(
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(admin),
                    fc_application::identity::SessionPurpose::Manage,
                    |issued_backend| {
                        issued_backend.identity(
                            admin,
                            IdentityCommand::PutSubject {
                                id: SubjectId::new(n).unwrap(),
                                name: format!("Subject {n}"),
                                enabled: true,
                                color: Rgb {
                                    red: 20,
                                    green: 40,
                                    blue: 60,
                                },
                                expected_revision: None,
                            },
                        )
                    },
                )
                .unwrap();
        }
        fixture
            .backend
            .test_native_command(
                fc_ports::command_window::CommandFamily::Identity,
                Some(admin),
                fc_application::identity::SessionPurpose::Manage,
                |issued_backend| {
                    issued_backend.identity(
                        admin,
                        IdentityCommand::BindCurrentClass {
                            id: ClassId::new(1).unwrap(),
                        },
                    )
                },
            )
            .unwrap();
        // The preceding synthetic local login may have revoked the old admin.
        let admin = fixture.issue(1, TrustedOrigin::Local);
        fixture.seed(
            Some(admin),
            IdentityCommand::CreateAccount(NewAccount {
                id: id(2),
                login_name: "teacher".into(),
                display_name: "Teacher".into(),
                role: Role::TeacherAdmin,
                scopes: vec![scope(1, 1), scope(1, 2), scope(2, 1), scope(2, 2)],
            }),
        );
        // The preceding synthetic local login may have revoked the old admin.
        let admin = fixture.issue(1, TrustedOrigin::Local);
        fixture.seed(
            Some(admin),
            IdentityCommand::CreateAccount(NewAccount {
                id: id(3),
                login_name: "second-admin".into(),
                display_name: "Second admin".into(),
                role: Role::GlobalAdmin,
                scopes: vec![],
            }),
        );
        // The preceding synthetic local login may have revoked the old admin.
        let admin = fixture.issue(1, TrustedOrigin::Local);
        fixture.seed(
            Some(admin),
            IdentityCommand::CreateAccount(NewAccount {
                id: id(4),
                login_name: "other-teacher".into(),
                display_name: "Other teacher".into(),
                role: Role::TeacherAdmin,
                scopes: vec![scope(1, 1)],
            }),
        );
        fixture
    }
    fn seed(&mut self, admin: Option<SessionLease>, command: IdentityCommand) {
        let issued = self
            .backend
            .issue_native_command(
                fc_ports::command_window::CommandFamily::Identity,
                admin,
                SessionPurpose::Manage,
            )
            .unwrap();
        let mut backend = self.backend.begin_command_scope(issued).unwrap();
        let state = backend.state().unwrap();
        let principal = match &command {
            IdentityCommand::FirstAdministrator { id, .. } => *id,
            IdentityCommand::CreateAccount(spec) => spec.id,
            _ => unreachable!(),
        };
        let request = backend.request(&state, command).unwrap();
        let (change, _) =
            prepare_initial_credentials(&mut SyntheticCredentials, &(), request).unwrap();
        let (actor, generation) = admin
            .map(|lease| {
                backend
                    .actor(&state, lease, SessionPurpose::Manage)
                    .unwrap()
            })
            .unzip();
        let mut repository = match admin {
            Some(lease) => backend.guarded(lease, generation.unwrap(), SessionPurpose::Manage),
            None => backend.repository.clone(),
        };
        let transition = repository
            .execute_credentials(
                backend.encode(change, principal, 1, 1).unwrap(),
                actor.as_ref(),
                self.clock.as_ref(),
            )
            .unwrap();
        backend.apply(&transition).unwrap();
        drop(backend);
        if admin.is_some() {
            // Initial password change is a local component operation. No Web
            // pairing/listener readiness is fabricated to complete this seed.
            let lease = self.issue(principal.get(), TrustedOrigin::Local);
            let issued = self
                .backend
                .issue_native_command(
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(lease),
                    SessionPurpose::ChangeInitialPassword,
                )
                .unwrap();
            let mut backend = self.backend.begin_command_scope(issued).unwrap();
            let state = backend.state().unwrap();
            let request = backend
                .request(
                    &state,
                    IdentityCommand::PasswordChanged {
                        id: principal,
                        expected_credential_version: 1,
                    },
                )
                .unwrap();
            let original = SyntheticCredentials.password_record(&()).unwrap();
            let change =
                prepare_password_change(&mut SyntheticCredentials, &(), &(), &original, request)
                    .unwrap();
            let (actor, generation) = backend
                .actor(&state, lease, SessionPurpose::ChangeInitialPassword)
                .unwrap();
            let mut repository =
                backend.guarded(lease, generation, SessionPurpose::ChangeInitialPassword);
            let transition = repository
                .execute_credentials(
                    backend.encode(change, principal, 2, 1).unwrap(),
                    Some(&actor),
                    self.clock.as_ref(),
                )
                .unwrap();
            backend.apply(&transition).unwrap();
        }
    }
    fn issue(&self, principal: u64, origin: TrustedOrigin) -> SessionLease {
        let backend = &self.backend;
        let state = backend.state().unwrap();
        let generation = backend.control.generation();
        let tick = self.clock.monotonic_millis();
        let mut ledger = backend.ledger.lock().unwrap();
        ledger.synchronize(&backend.control, &state, self.clock.now(), tick);
        if origin == TrustedOrigin::Local {
            ledger.sessions.lock_local();
        }
        let lease = ledger
            .sessions
            .issue_verified(
                state.account(id(principal)).unwrap(),
                origin,
                tick,
                SESSION_MILLIS,
                next_day(self.clock.now()).unwrap(),
            )
            .unwrap();
        if origin == TrustedOrigin::Local {
            backend
                .control
                .install_input(lease, generation, tick)
                .unwrap();
            backend
                .control
                .0
                .active_principal
                .store(principal, Ordering::Release);
            backend
                .control
                .0
                .active_local
                .store(lease.id(), Ordering::Release);
            ledger.local = Some(lease);
        }
        lease
    }
    fn admin(&self) -> SessionLease {
        self.issue(1, TrustedOrigin::Local)
    }
    fn pair(&self, admin: SessionLease, login: &str) -> web::IssuedWebSession {
        let service = self.backend.web_service();
        service.set_enabled(admin, true).unwrap();
        let window = service.begin_pairing(admin).unwrap();
        let pending = service
            .request_pairing(window.code, login, "Synthetic account device")
            .unwrap();
        service
            .approve(
                admin,
                pending.request_id,
                self.backend.state().unwrap().revision,
            )
            .unwrap();
        let web::PairingPoll::Granted(session) = service
            .poll(pending.request_id, &pending.poll_secret)
            .unwrap()
        else {
            panic!("approved fixture did not issue");
        };
        session
    }
    fn actor_of(&self, session: &web::IssuedWebSession) -> SessionLease {
        self.backend
            .web_service()
            .authenticate(&session.cookie)
            .unwrap()
            .lease()
    }
    fn reopen(&mut self) {
        self.backend.close().unwrap();
        self.backend = Backend::open(
            self.directory.database(),
            self.directory.codec(),
            self.clock.clone(),
            self.clock.clone(),
        )
        .unwrap();
    }
    fn assert_credentials(&self, principal: u64) {
        let state = self.backend.state().unwrap();
        let account = state.account(id(principal)).unwrap();
        for (kind, version) in [
            ("password", account.credential_version),
            ("recovery", account.recovery_version),
        ] {
            let row = self
                .backend
                .writer
                .as_ref()
                .unwrap()
                .handle()
                .try_get(format!("business/credential/{principal}/{kind}/{version}"))
                .unwrap()
                .wait()
                .unwrap()
                .unwrap();
            assert_eq!(row.value.len(), fc_security::CREDENTIAL_RECORD_BYTES);
            if kind == "password" {
                self.backend
                    .codec
                    .decode_password(id(principal), version, &row.value)
                    .unwrap();
            } else {
                self.backend
                    .codec
                    .decode_recovery(id(principal), version, &row.value)
                    .unwrap();
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.backend
            .close()
            .expect("account backend writer shutdown");
    }
}

mod administrator_transfer_sqlite_tests {
    use super::*;
    use fc_ports::command_window::CommandFamily;

    const PASSWORD: &str = "Synthetic administrator handover 2026!";

    fn authenticated_admin(fixture: &mut Fixture) -> SessionLease {
        // The existing fixture's recovery verifier is real and reproducible.
        // Use production recovery to install a real password verifier, then
        // production login/re-verification; no synthetic password is accepted.
        let (code, _) = SyntheticCredentials.recovery_record().unwrap();
        drop(
            fixture
                .backend
                .recover("admin", code, Password::new(PASSWORD.into()).unwrap())
                .unwrap(),
        );
        fixture
            .backend
            .login("admin", Password::new(PASSWORD.into()).unwrap())
            .unwrap()
    }
    fn credential_rows(fixture: &Fixture, principal: PrincipalId) -> Vec<(u64, Vec<u8>)> {
        [false, true]
            .into_iter()
            .map(|recovery| {
                fixture
                    .backend
                    .repository
                    .credential_record(principal, recovery)
                    .unwrap()
            })
            .collect()
    }

    #[test]
    fn handover_reverifies_and_commits_both_roles_epochs_refs_and_query_without_directory_rights() {
        let mut fixture = Fixture::new();
        let admin = authenticated_admin(&mut fixture);
        fixture
            .backend
            .test_native_command(
                CommandFamily::Identity,
                Some(admin),
                SessionPurpose::Manage,
                |b| {
                    b.identity(
                        admin,
                        update(1, "Admin", Role::GlobalAdmin, true, vec![scope(1, 1)]),
                    )
                },
            )
            .unwrap();
        let admin = fixture
            .backend
            .login("admin", Password::new(PASSWORD.into()).unwrap())
            .unwrap();
        fixture
            .backend
            .test_native_command(
                CommandFamily::Identity,
                Some(admin),
                SessionPurpose::Manage,
                |b| {
                    b.identity(
                        admin,
                        IdentityCommand::SetDefaultTeacher(DefaultTeacher {
                            class_id: ClassId::new(1).unwrap(),
                            subject_id: SubjectId::new(1).unwrap(),
                            teacher_id: id(1),
                        }),
                    )
                },
            )
            .unwrap();
        let schedule = fixture
            .backend
            .test_native_command(
                CommandFamily::Schedule,
                Some(admin),
                SessionPurpose::Manage,
                |b| b.save_schedule(admin, Revision::default(), timetable("default")),
            )
            .unwrap();
        let admin_device = fixture.pair(admin, "admin");
        let teacher_device = fixture.pair(admin, "teacher");
        let admin_web = fixture.actor_of(&admin_device);
        let teacher_web = fixture.actor_of(&teacher_device);
        let before = fixture.backend.state().unwrap();
        let records = [
            credential_rows(&fixture, id(1)),
            credential_rows(&fixture, id(2)),
        ];
        let issued = fixture
            .backend
            .issue_native_command(CommandFamily::Identity, Some(admin), SessionPurpose::Manage)
            .unwrap();
        let mut proof = None;
        let saved = fixture
            .backend
            .begin_command_scope(issued)
            .unwrap()
            .transfer_administrator_at_tracked(
                admin,
                before.revision,
                id(2),
                Password::new(PASSWORD.into()).unwrap(),
                |p| {
                    proof = Some(Arc::new(p));
                    Ok(())
                },
            )
            .unwrap();
        let proof = proof.unwrap();
        assert!(saved
            .accounts
            .iter()
            .any(|a| a.principal.id == id(3) && a.principal.role == Role::GlobalAdmin));
        assert_eq!(saved.revision, before.revision.next().unwrap());
        assert_eq!(saved.assignments, before.assignments);
        assert_eq!(saved.default_teachers, before.default_teachers);
        assert_eq!(
            saved.account(id(1)).unwrap().principal.role,
            Role::TeacherAdmin
        );
        assert_eq!(
            saved.account(id(2)).unwrap().principal.role,
            Role::GlobalAdmin
        );
        for principal in [id(1), id(2)] {
            assert_eq!(
                saved.account(principal).unwrap().principal.auth_epoch,
                before
                    .account(principal)
                    .unwrap()
                    .principal
                    .auth_epoch
                    .next()
                    .unwrap()
            );
        }
        assert_eq!(
            records,
            [
                credential_rows(&fixture, id(1)),
                credential_rows(&fixture, id(2))
            ]
        );
        for old in [admin, admin_web, teacher_web] {
            assert!(fixture
                .backend
                .check_session(old, SessionPurpose::Manage)
                .is_err());
        }
        // Query the actual original receipt after dropping the original result
        // and reopening SQLite. This never installs its historical state.
        drop(saved);
        fixture.reopen();
        let former = fixture
            .backend
            .login("admin", Password::new(PASSWORD.into()).unwrap())
            .unwrap();
        assert!(fixture.backend.management_identity(former).is_err());
        let current = fixture.backend.state().unwrap();
        let revision = fixture.backend.global_revision().unwrap();
        let original = fixture
            .backend
            .begin_command_scope(issued)
            .unwrap()
            .account_outcome(former, proof.clone())
            .unwrap()
            .unwrap();
        assert!(original.matches(proof.request().expected_revision, &proof.request().command));
        assert_eq!(original.account.principal.id, id(2));
        assert_eq!(fixture.backend.state().unwrap(), current);
        assert_eq!(fixture.backend.global_revision().unwrap(), revision);
        let other = fixture.issue(3, TrustedOrigin::Local);
        assert!(fixture
            .backend
            .begin_command_scope(issued)
            .unwrap()
            .account_outcome(other, proof.clone())
            .is_err());
        assert_eq!(fixture.backend.read_schedule(other).unwrap(), schedule);
        // A later distinct identity mutation is not adopted as the handover's
        // own epoch transition, even for the same principal and unchanged password.
        fixture
            .backend
            .test_native_command(
                CommandFamily::Identity,
                Some(other),
                SessionPurpose::Manage,
                |b| {
                    b.identity(
                        other,
                        update(
                            1,
                            "Later former administrator",
                            Role::TeacherAdmin,
                            true,
                            vec![scope(1, 1)],
                        ),
                    )
                },
            )
            .unwrap();
        let former = fixture.issue(1, TrustedOrigin::Local);
        assert!(fixture
            .backend
            .begin_command_scope(issued)
            .unwrap()
            .account_outcome(former, proof)
            .is_err());
    }

    #[test]
    fn transfer_wrong_password_is_persistently_counted_and_plain_command_cas_and_late_lock_cannot_write(
    ) {
        let mut fixture = Fixture::new();
        let admin = authenticated_admin(&mut fixture);
        let before = fixture.backend.state().unwrap();
        let command = IdentityCommand::TransferAdministrator {
            from: id(1),
            target: id(2),
            expected_credential_version: before.account(id(1)).unwrap().credential_version,
        };
        assert!(fixture
            .backend
            .test_native_command(
                CommandFamily::Identity,
                Some(admin),
                SessionPurpose::Manage,
                |b| b.identity(admin, command)
            )
            .is_err());
        assert!(matches!(
            fixture.backend.test_native_command(
                CommandFamily::Identity,
                Some(admin),
                SessionPurpose::Manage,
                |b| {
                    b.transfer_administrator_at_tracked(
                        admin,
                        Revision::default(),
                        id(2),
                        Password::new(PASSWORD.into()).unwrap(),
                        |_| panic!("CAS conflict must precede publish"),
                    )
                }
            ),
            Err(BackendError::Identity(IdentityRepositoryError::Domain(
                IdentityError::RevisionConflict
            )))
        ));
        assert!(matches!(
            fixture.backend.test_native_command(
                CommandFamily::Identity,
                Some(admin),
                SessionPurpose::Manage,
                |b| {
                    b.transfer_administrator_at_tracked(
                        admin,
                        before.revision,
                        id(2),
                        Password::new("Synthetic incorrect handover password!".into()).unwrap(),
                        |_| panic!("invalid password cannot publish"),
                    )
                }
            ),
            Err(BackendError::InvalidCredentials)
        ));
        let penalty = fixture
            .backend
            .handle
            .try_get("business/security/login/1".into())
            .unwrap()
            .wait()
            .unwrap()
            .unwrap();
        assert!(!penalty.value.is_empty());
        let admitted = fixture
            .backend
            .admission(id(1), CredentialKind::Password)
            .unwrap();
        assert_eq!(
            fixture
                .backend
                .finish(admitted.reservation, Verification::Rejected)
                .unwrap(),
            Completion::Rejected {
                failures: 2,
                wait_seconds: 0
            }
        );
        assert_eq!(fixture.backend.state().unwrap(), before);
        let issued = fixture
            .backend
            .issue_native_command(CommandFamily::Identity, Some(admin), SessionPurpose::Manage)
            .unwrap();
        let control = fixture.backend.session_control();
        let mut proof = None;
        assert!(fixture
            .backend
            .begin_command_scope(issued)
            .unwrap()
            .transfer_administrator_at_tracked(
                admin,
                before.revision,
                id(2),
                Password::new(PASSWORD.into()).unwrap(),
                |p| {
                    proof = Some(Arc::new(p));
                    control.lock_local();
                    Ok(())
                }
            )
            .is_err());
        assert_eq!(fixture.backend.state().unwrap(), before);
        let admin = fixture
            .backend
            .login("admin", Password::new(PASSWORD.into()).unwrap())
            .unwrap();
        let revision = fixture.backend.global_revision().unwrap();
        assert!(fixture
            .backend
            .begin_command_scope(issued)
            .unwrap()
            .account_outcome(admin, proof.unwrap())
            .unwrap()
            .is_none());
        assert_eq!(fixture.backend.state().unwrap(), before);
        assert_eq!(fixture.backend.global_revision().unwrap(), revision);
    }
}

#[test]
fn accounts_real_create_update_multiple_scopes_captured_cas_and_reopen_protected_records() {
    let mut fixture = Fixture::new();
    let admin = fixture.admin();
    let web = fixture.pair(admin, "teacher");
    let web_lease = fixture.actor_of(&web);
    let captured = fixture.backend.management_identity(admin).unwrap().revision;
    // One real production PBKDF2 account creation, no extra login KDF loop.
    let code = fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend.create_account_at(
                    admin,
                    captured,
                    NewAccount {
                        id: id(5),
                        login_name: "multi-new".into(),
                        display_name: "New teacher".into(),
                        role: Role::TeacherAdmin,
                        scopes: vec![scope(1, 1), scope(2, 2)],
                    },
                    Password::new("Synthetic account creation 2026!".into()).unwrap(),
                )
            },
        )
        .unwrap();
    drop(code);
    fixture.assert_credentials(5);
    let before = fixture.backend.state().unwrap();
    let global = fixture.backend.global_revision().unwrap();
    assert_eq!(
        fixture.backend.test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| issued_backend.identity_at(
                admin,
                captured,
                update(5, "stale edit", Role::TeacherAdmin, true, vec![scope(1, 2)])
            )
        ),
        Err(identity_error(IdentityError::RevisionConflict))
    );
    assert_eq!(fixture.backend.state().unwrap(), before);
    assert_eq!(fixture.backend.global_revision().unwrap(), global);
    assert_eq!(
        fixture
            .backend
            .test_native_command(
                fc_ports::command_window::CommandFamily::Identity,
                Some(admin),
                fc_application::identity::SessionPurpose::Manage,
                |issued_backend| issued_backend.create_account_at(
                    admin,
                    captured,
                    NewAccount {
                        id: id(6),
                        login_name: "stale-create".into(),
                        display_name: "Stale create".into(),
                        role: Role::User,
                        scopes: vec![]
                    },
                    Password::new("Synthetic rejected create 2026!".into()).unwrap()
                )
            )
            .err(),
        Some(identity_error(IdentityError::RevisionConflict))
    );
    assert_eq!(fixture.backend.state().unwrap(), before);
    assert_eq!(fixture.backend.global_revision().unwrap(), global);
    assert!(fixture
        .backend
        .web_service()
        .authenticate(&web.cookie)
        .is_ok());
    let changed = fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend.identity_at(
                    admin,
                    before.revision,
                    update(
                        5,
                        "Three scopes",
                        Role::TeacherAdmin,
                        true,
                        vec![scope(1, 1), scope(2, 1), scope(2, 2)],
                    ),
                )
            },
        )
        .unwrap();
    let account = changed.account(id(5)).unwrap();
    assert_eq!(account.login_name, "multi-new");
    assert_eq!(account.display_name, "Three scopes");
    assert!(account.must_change_password);
    assert_eq!(
        changed
            .assignments
            .iter()
            .filter(|a| a.teacher_id == id(5))
            .count(),
        3
    );
    assert!(fixture
        .backend
        .check_session(web_lease, SessionPurpose::Manage)
        .is_ok());
    fixture.reopen();
    assert_eq!(fixture.backend.state().unwrap(), changed);
    fixture.assert_credentials(5);
    fixture.assert_credentials(2);
    assert!(fixture
        .backend
        .check_session(admin, SessionPurpose::Manage)
        .is_err());
    let next_admin = fixture.admin();
    assert_eq!(
        fixture.backend.management_identity(next_admin).unwrap(),
        changed
    );
}

#[test]
fn accounts_management_reads_and_writes_reject_teacher_web_and_last_admin_without_revocation() {
    let mut fixture = Fixture::new();
    let admin = fixture.admin();
    let admin_web = fixture.pair(admin, "admin");
    let web = fixture.actor_of(&admin_web);
    assert_eq!(
        fixture.backend.management_identity(web),
        Err(identity_error(IdentityError::Permission(
            PermissionError::LocalOnly
        )))
    );
    assert!(fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(web),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| issued_backend.identity(
                web,
                update(2, "forbidden", Role::TeacherAdmin, true, vec![scope(1, 1)])
            )
        )
        .is_err());
    let teacher = fixture.issue(2, TrustedOrigin::Local);
    assert_eq!(
        fixture.backend.management_identity(teacher),
        Err(identity_error(IdentityError::Permission(
            PermissionError::RoleDenied
        )))
    );
    assert!(fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(teacher),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| issued_backend.identity(
                teacher,
                update(2, "forbidden", Role::TeacherAdmin, true, vec![scope(1, 1)])
            )
        )
        .is_err());
    let admin = fixture.admin();
    fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend.identity(
                    admin,
                    update(3, "Disabled second", Role::GlobalAdmin, false, vec![]),
                )
            },
        )
        .unwrap();
    let before = fixture.backend.state().unwrap();
    let revision = fixture.backend.global_revision().unwrap();
    assert_eq!(
        fixture.backend.test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| issued_backend.identity_at(
                admin,
                before.revision,
                update(1, "Cannot leave", Role::User, true, vec![])
            )
        ),
        Err(identity_error(IdentityError::LastAdministrator))
    );
    assert_eq!(fixture.backend.state().unwrap(), before);
    assert_eq!(fixture.backend.global_revision().unwrap(), revision);
    assert_eq!(
        fixture
            .backend
            .session_control()
            .health_gate()
            .status()
            .health,
        Health::Healthy
    );
    assert!(fixture.backend.management_identity(admin).is_ok());
    assert!(fixture
        .backend
        .web_service()
        .authenticate(&admin_web.cookie)
        .is_ok());
    fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend.identity(
                    admin,
                    update(3, "Enabled second", Role::GlobalAdmin, true, vec![]),
                )
            },
        )
        .unwrap();
    let renamed = fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend.identity(
                    admin,
                    update(1, "Self renamed", Role::GlobalAdmin, true, vec![]),
                )
            },
        )
        .unwrap();
    assert_eq!(renamed.account(id(1)).unwrap().display_name, "Self renamed");
    assert!(fixture.backend.management_identity(admin).is_err());
    assert!(fixture
        .backend
        .web_service()
        .authenticate(&admin_web.cookie)
        .is_err());
    let fresh = fixture.admin();
    let own_web = fixture.pair(fresh, "admin");
    let demoted = fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(fresh),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend.identity(fresh, update(1, "Self demoted", Role::User, true, vec![]))
            },
        )
        .unwrap();
    assert_eq!(demoted.account(id(1)).unwrap().principal.role, Role::User);
    assert!(fixture
        .backend
        .check_session(fresh, SessionPurpose::Manage)
        .is_err());
    assert!(fixture
        .backend
        .web_service()
        .authenticate(&own_web.cookie)
        .is_err());
    assert!(fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(fresh),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| issued_backend.identity(
                fresh,
                IdentityCommand::RenameSchool {
                    name: "stale session".into()
                }
            )
        )
        .is_err());
    let remaining = fixture.issue(3, TrustedOrigin::Local);
    assert!(fixture.backend.management_identity(remaining).is_ok());
    let user = fixture.issue(1, TrustedOrigin::Local);
    assert!(fixture.backend.management_identity(user).is_err());
}

struct MaterialFixture;
impl fc_ports::MaterialPreparation for MaterialFixture {
    type Input = ();
    fn prepare(&mut self, _: ()) -> std::result::Result<MaterialResource, MaterialRepositoryError> {
        Ok(MaterialResource {
            version_id: ResourceVersionId::new(900).unwrap(),
            sha256: [7; 32],
            kind: MaterialKind::Txt,
            bytes: 10,
        })
    }
}
struct VoiceFixture;
impl fc_ports::voice::VoicePreparation for VoiceFixture {
    type Input = ();
    fn prepare(
        &mut self,
        _: (),
        _: &fc_domain::voice::VoiceAsset,
    ) -> std::result::Result<
        fc_domain::voice::VoicePcmMetadata,
        fc_ports::voice::VoiceRepositoryError,
    > {
        Ok(fc_domain::voice::VoicePcmMetadata {
            byte_length: 16_044,
            frame_count: 8_000,
            sha256: [5; 32],
        })
    }
}
#[test]
fn accounts_scope_role_and_disable_changes_revoke_exact_web_voice_and_material_access() {
    use fc_ports::voice::*;
    let mut fixture = Fixture::control();
    let admin = fixture.admin();
    let device = fixture.pair(admin, "teacher");
    let teacher = fixture.actor_of(&device);
    let web_service = fixture.backend.web_service();
    assert!(fixture.backend.management_identity(teacher).is_err());
    assert!(fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(teacher),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| issued_backend.identity(
                teacher,
                update(
                    2,
                    "Forbidden Web management",
                    Role::TeacherAdmin,
                    true,
                    vec![self::scope(1, 1)]
                )
            )
        )
        .is_err());
    let scope = MaterialScope {
        teacher_id: id(2),
        subject_id: SubjectId::new(1).unwrap(),
    };
    web_service
        .check_material_lease(
            teacher,
            scope,
            &[ClassId::new(1).unwrap(), ClassId::new(2).unwrap()],
        )
        .unwrap();
    let material_command = fixture
        .backend
        .issue_native_command(
            fc_ports::command_window::CommandFamily::Materials,
            Some(admin),
            SessionPurpose::Manage,
        )
        .unwrap();
    let material = fc_ports::prepare_material_append(
        &mut MaterialFixture,
        (),
        fc_ports::MaterialAppendMetadata {
            command_id: material_command.command_id,
            scope,
            material_id: MaterialId::new(900).unwrap(),
            display_name: "Synthetic account material.txt".into(),
            allowed_class_ids: vec![ClassId::new(1).unwrap(), ClassId::new(2).unwrap()],
        },
    )
    .unwrap();
    fixture
        .backend
        .begin_command_scope(material_command)
        .unwrap()
        .append_material(admin, material)
        .unwrap();
    let material_guard = fixture
        .backend
        .material_for_owner(
            admin,
            scope,
            MaterialId::new(900).unwrap(),
            ResourceVersionId::new(900).unwrap(),
        )
        .unwrap()
        .guard;
    let asset = fixture
        .backend
        .voice_reserve(teacher, ClassId::new(1).unwrap())
        .unwrap();
    let prepared = prepare_voice_upload(&mut VoiceFixture, (), asset).unwrap();
    let asset = fixture.backend.voice_publish(teacher, prepared).unwrap();
    let voice_command = fixture
        .backend
        .issue_web_command(teacher, fc_ports::command_window::CommandFamily::Voice)
        .unwrap();
    let job = fixture
        .backend
        .begin_command_scope(voice_command)
        .unwrap()
        .voice_submit(
            teacher,
            VoiceSubmitRequest {
                command_id: voice_command.command_id,
                asset_id: asset.id,
                expected_asset_revision: asset.revision,
                class_id: ClassId::new(1).unwrap(),
                repeat_total: 1,
            },
        )
        .unwrap();
    let voice_guard = fixture.backend.voice_guard(job.id).unwrap().guard;
    assert!(voice_guard.is_valid());
    // Re-authorize after voice metadata commits; a material preparation is bound
    // to one exact authority revision, including otherwise unrelated updates.
    let material_guard_after = fixture
        .backend
        .material_for_owner(
            admin,
            scope,
            MaterialId::new(900).unwrap(),
            ResourceVersionId::new(900).unwrap(),
        )
        .unwrap()
        .guard;
    assert!(!material_guard.is_context_valid());
    assert!(material_guard_after.is_context_valid());
    let changed = fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend.identity(
                    admin,
                    update(
                        2,
                        "Class two only",
                        Role::TeacherAdmin,
                        true,
                        vec![self::scope(2, 1), self::scope(2, 2)],
                    ),
                )
            },
        )
        .unwrap();
    assert_eq!(
        changed.account(id(2)).unwrap().principal.auth_epoch,
        AuthEpoch::new(3)
    );
    assert!(fixture
        .backend
        .check_session(teacher, SessionPurpose::Manage)
        .is_err());
    assert!(web_service.authenticate(&device.cookie).is_err());
    assert!(web_service
        .check_material_lease(teacher, scope, &[ClassId::new(2).unwrap()])
        .is_err());
    assert!(!voice_guard.is_valid());
    assert!(!material_guard_after.is_context_valid());
    assert!(fixture.backend.voice_status(teacher, job.id).is_err());
    assert!(
        fixture.backend.management_identity(admin).is_ok(),
        "another account's edit must preserve the initiating admin lease"
    );
    let refreshed = fixture.pair(admin, "teacher");
    let fresh_teacher = fixture.actor_of(&refreshed);
    web_service
        .check_material_lease(fresh_teacher, scope, &[ClassId::new(2).unwrap()])
        .unwrap();
    assert!(web_service
        .check_material_lease(fresh_teacher, scope, &[ClassId::new(1).unwrap()])
        .is_err());
    fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend
                    .identity(admin, update(2, "Read only user", Role::User, true, vec![]))
            },
        )
        .unwrap();
    assert!(fixture
        .backend
        .check_session(fresh_teacher, SessionPurpose::Manage)
        .is_err());
    assert!(web_service.authenticate(&refreshed.cookie).is_err());
    let user = fixture.pair(admin, "teacher");
    let user_lease = fixture.actor_of(&user);
    assert!(fixture
        .backend
        .check_session(user_lease, SessionPurpose::Manage)
        .is_ok());
    assert!(web_service
        .check_material_lease(user_lease, scope, &[ClassId::new(2).unwrap()])
        .is_err());
    fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend
                    .identity(admin, update(2, "Disabled user", Role::User, false, vec![]))
            },
        )
        .unwrap();
    assert!(fixture.backend.management_identity(user_lease).is_err());
    assert!(fixture
        .backend
        .check_session(user_lease, SessionPurpose::Manage)
        .is_err());
    assert!(web_service.authenticate(&user.cookie).is_err());
    let state = fixture.backend.state().unwrap();
    let mut ledger = fixture.backend.ledger.lock().unwrap();
    assert!(ledger
        .sessions
        .issue_verified(
            state.account(id(2)).unwrap(),
            TrustedOrigin::Local,
            1_000,
            SESSION_MILLIS,
            next_day(fixture.clock.now()).unwrap()
        )
        .is_err());
}

fn timetable(mode: &str) -> ScheduleDocument {
    use fc_domain::{schedule::*, schedule_document::*};
    let row = TimetableRow {
        id: RowId::new(30).unwrap(),
        logical_key: LogicalRowKey::new(40).unwrap(),
        group: DayGroup::Morning,
        kind: RowKind::Course,
        range: MinuteRange::new(
            Minute::from_hm(8, 20).unwrap(),
            Minute::from_hm(9, 0).unwrap(),
        )
        .unwrap(),
        enabled_days: [true; 7],
        courses: std::array::from_fn(|_| {
            Some(CourseCell {
                subject_id: SubjectId::new(1).unwrap(),
                teacher: if mode.contains("default") {
                    TeacherBinding::Default
                } else {
                    TeacherBinding::Explicit(id(2))
                },
            })
        }),
    };
    let table = WeekTable {
        id: TableId::new(20).unwrap(),
        role: TableRole::Single,
        rows: vec![row],
    };
    let mut plan = SemesterPlan {
        semester: Semester {
            id: SemesterId::new(10).unwrap(),
            class_id: ClassId::new(1).unwrap(),
            revision: Revision::default(),
            enabled: true,
            start: LocalDate::new(2026, 10, 1).unwrap(),
            end: LocalDate::new(2026, 11, 30).unwrap(),
            first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
            tables: vec![table],
        },
        name: "Account reference fixture".into(),
        timing_sync: true,
        dual_history: None,
        row_details: vec![],
        cell_details: vec![],
    };
    if mode == "history" {
        let mut odd = plan.semester.tables[0].clone();
        odd.role = TableRole::Odd;
        for cell in odd.rows[0].courses.iter_mut().flatten() {
            cell.teacher = TeacherBinding::Explicit(id(4));
        }
        let mut even = plan.semester.tables[0].clone();
        even.id = TableId::new(21).unwrap();
        even.role = TableRole::Even;
        even.rows[0].id = RowId::new(31).unwrap();
        plan.semester.tables = vec![odd, even];
        plan = plan.disable_dual(TableId::new(20).unwrap()).unwrap();
    }
    ScheduleDocument {
        semesters: vec![plan],
        ..ScheduleDocument::default()
    }
}

#[test]
fn accounts_active_default_and_retained_history_references_reject_atomically_without_revoking() {
    for mode in ["explicit", "default", "implicit-default", "history"] {
        let mut fixture = Fixture::new();
        let admin = fixture.admin();
        let teacher_device = fixture.pair(admin, "teacher");
        let teacher = fixture.actor_of(&teacher_device);
        if mode == "implicit-default" {
            fixture
                .backend
                .test_native_command(
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(admin),
                    fc_application::identity::SessionPurpose::Manage,
                    |issued_backend| {
                        issued_backend.identity(
                            admin,
                            update(4, "Not currently teaching", Role::User, true, vec![]),
                        )
                    },
                )
                .unwrap();
        }
        if mode == "default" {
            fixture
                .backend
                .test_native_command(
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(admin),
                    fc_application::identity::SessionPurpose::Manage,
                    |issued_backend| {
                        issued_backend.identity(
                            admin,
                            IdentityCommand::SetDefaultTeacher(DefaultTeacher {
                                class_id: ClassId::new(1).unwrap(),
                                subject_id: SubjectId::new(1).unwrap(),
                                teacher_id: id(2),
                            }),
                        )
                    },
                )
                .unwrap();
        }
        let document = fixture
            .backend
            .test_native_command(
                fc_ports::command_window::CommandFamily::Schedule,
                Some(admin),
                fc_application::identity::SessionPurpose::Manage,
                |issued_backend| {
                    issued_backend.save_schedule(admin, Revision::default(), timetable(mode))
                },
            )
            .unwrap();
        let before = fixture.backend.state().unwrap();
        let revision = fixture.backend.global_revision().unwrap();
        let commands = if mode == "implicit-default" {
            vec![update(
                4,
                "Would make default ambiguous",
                Role::TeacherAdmin,
                true,
                vec![scope(1, 1)],
            )]
        } else {
            [
                update(
                    2,
                    "Disable forbidden reference",
                    Role::TeacherAdmin,
                    false,
                    vec![scope(1, 1), scope(1, 2), scope(2, 1), scope(2, 2)],
                ),
                update(
                    2,
                    "Remove referenced scope",
                    Role::TeacherAdmin,
                    true,
                    vec![scope(2, 2)],
                ),
            ]
            .to_vec()
        };
        for command in commands {
            assert_eq!(
                fixture.backend.test_native_command(
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(admin),
                    fc_application::identity::SessionPurpose::Manage,
                    |issued_backend| issued_backend.identity_at(admin, before.revision, command)
                ),
                Err(identity_error(IdentityError::InvalidScope)),
                "reference mode={mode}"
            );
            assert_eq!(fixture.backend.state().unwrap(), before);
            assert_eq!(fixture.backend.global_revision().unwrap(), revision);
            assert_eq!(fixture.backend.read_schedule(admin).unwrap(), document);
            assert!(fixture.backend.management_identity(admin).is_ok());
            assert!(fixture
                .backend
                .check_session(teacher, SessionPurpose::Manage)
                .is_ok());
            assert!(fixture
                .backend
                .web_service()
                .authenticate(&teacher_device.cookie)
                .is_ok());
            assert_eq!(
                fixture
                    .backend
                    .session_control()
                    .health_gate()
                    .status()
                    .health,
                Health::Healthy
            );
        }
        fixture.reopen();
        assert_eq!(fixture.backend.state().unwrap(), before);
        let admin = fixture.admin();
        assert_eq!(fixture.backend.read_schedule(admin).unwrap(), document);
    }
}

#[test]
fn accounts_domain_valid_large_assignment_document_rejects_real_64k_capacity_atomically() {
    let mut fixture = Fixture::new();
    let admin = fixture.admin();
    // Domain's 4096 assignment count is a ceiling, not a promise that every
    // combination fits the current single 64KiB identity/receipt documents.
    for class in 3..=128 {
        fixture
            .backend
            .test_native_command(
                fc_ports::command_window::CommandFamily::Identity,
                Some(admin),
                fc_application::identity::SessionPurpose::Manage,
                |issued_backend| {
                    issued_backend.identity(
                        admin,
                        IdentityCommand::CreateClass {
                            id: ClassId::new(class).unwrap(),
                            name: format!("Class {class}"),
                        },
                    )
                },
            )
            .unwrap();
    }
    for subject in 3..=32 {
        fixture
            .backend
            .test_native_command(
                fc_ports::command_window::CommandFamily::Identity,
                Some(admin),
                fc_application::identity::SessionPurpose::Manage,
                |issued_backend| {
                    issued_backend.identity(
                        admin,
                        IdentityCommand::PutSubject {
                            id: SubjectId::new(subject).unwrap(),
                            name: format!("Subject {subject}"),
                            enabled: true,
                            color: Rgb {
                                red: 20,
                                green: 40,
                                blue: 60,
                            },
                            expected_revision: None,
                        },
                    )
                },
            )
            .unwrap();
    }
    let web_device = fixture.pair(admin, "teacher");
    let teacher = fixture.actor_of(&web_device);
    let before = fixture.backend.state().unwrap();
    let revision = fixture.backend.global_revision().unwrap();
    let scopes = (1..=128)
        .flat_map(|class| (1..=31).map(move |subject| scope(class, subject)))
        .collect::<Vec<_>>();
    assert_eq!(scopes.len(), 3968);
    let command = update(
        2,
        "Large but count-valid account",
        Role::TeacherAdmin,
        true,
        scopes,
    );
    let (actor, _) = fixture
        .backend
        .actor(&before, admin, SessionPurpose::Manage)
        .unwrap();
    assert!(before
        .transition(&command, Some(&actor), fixture.clock.now())
        .is_ok());
    assert_eq!(
        fixture.backend.test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| issued_backend.identity_at(admin, before.revision, command)
        ),
        Err(identity_error(IdentityError::Capacity))
    );
    assert_eq!(fixture.backend.state().unwrap(), before);
    assert_eq!(fixture.backend.global_revision().unwrap(), revision);
    assert!(fixture
        .backend
        .check_session(teacher, SessionPurpose::Manage)
        .is_ok());
    assert!(fixture
        .backend
        .web_service()
        .authenticate(&web_device.cookie)
        .is_ok());
    assert!(fixture.backend.management_identity(admin).is_ok());
    assert_eq!(
        fixture
            .backend
            .session_control()
            .health_gate()
            .status()
            .health,
        Health::Healthy
    );
    let next = fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Identity,
            Some(admin),
            fc_application::identity::SessionPurpose::Manage,
            |issued_backend| {
                issued_backend.identity_at(
                    admin,
                    before.revision,
                    update(
                        2,
                        "Small valid retry",
                        Role::TeacherAdmin,
                        true,
                        vec![scope(1, 1), scope(2, 2)],
                    ),
                )
            },
        )
        .unwrap();
    assert_eq!(
        next.account(id(2)).unwrap().display_name,
        "Small valid retry"
    );
    assert!(fixture
        .backend
        .check_session(teacher, SessionPurpose::Manage)
        .is_err());
    fixture.reopen();
    assert_eq!(fixture.backend.state().unwrap(), next);
    fixture.assert_credentials(2);
}

// Account UNKNOWN regressions through the real SQLite writer.
// These cases use the existing real SQLite/Backend fixture and real issued C2
// tickets. Dropping a committed Backend return deliberately models lost output;
// it does not claim to execute the Windows native completion-message loss path.
mod account_unknown_sqlite_tests {
    use super::*;
    use fc_ports::command_window::{CommandFamily, CommandWindowError, IssuedCommand};

    type Proof = Arc<AccountReceiptProof>;
    const INITIAL_PASSWORD: &str = "Synthetic lost-output create 2026!";
    const CHANGED_PASSWORD: &str = "Synthetic explicit later password 2026!";

    fn issue_identity(
        fixture: &Fixture,
        lease: SessionLease,
        purpose: SessionPurpose,
    ) -> IssuedCommand {
        fixture
            .backend
            .issue_native_command(CommandFamily::Identity, Some(lease), purpose)
            .unwrap()
    }
    fn row(fixture: &Fixture, key: String) -> Option<fc_storage::Document> {
        fixture.backend.handle.try_get(key).unwrap().wait().unwrap()
    }
    fn receipt_key(issued: IssuedCommand) -> String {
        let epoch = issued
            .ticket
            .epoch()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        format!(
            "business/receipt/identity/v2/{epoch}/{}",
            issued.ticket.serial()
        )
    }
    fn credentials(fixture: &Fixture, principal: u64) -> Vec<Option<fc_storage::Document>> {
        ["password", "recovery"]
            .iter()
            .flat_map(|kind| (1..=2).map(move |version| (*kind, version)))
            .map(|(kind, version)| {
                row(
                    fixture,
                    format!("business/credential/{principal}/{kind}/{version}"),
                )
            })
            .collect()
    }
    fn query(
        fixture: &mut Fixture,
        lease: SessionLease,
        issued: IssuedCommand,
        proof: Proof,
    ) -> Result<Option<OriginalAccountResult>> {
        fixture
            .backend
            .begin_command_scope(issued)?
            .account_outcome(lease, proof)
    }
    fn tracked_update(
        fixture: &mut Fixture,
        lease: SessionLease,
        command: IdentityCommand,
    ) -> (IssuedCommand, Proof, IdentityState) {
        let expected = fixture.backend.state().unwrap().revision;
        let issued = issue_identity(fixture, lease, SessionPurpose::Manage);
        let mut proof = None;
        let current = fixture
            .backend
            .begin_command_scope(issued)
            .unwrap()
            .update_account_at_tracked(lease, expected, command, |value| {
                proof = Some(Arc::new(value));
                Ok(())
            })
            .unwrap();
        let proof =
            proof.expect("original request proof must be published before writer submission");
        assert_eq!(proof.issued(), issued);
        assert_eq!(proof.request().expected_revision, expected);
        (issued, proof, current)
    }
    fn assert_read_only(fixture: &Fixture, expected: &IdentityState, global: Revision) {
        assert_eq!(
            &fixture.backend.state().unwrap(),
            expected,
            "query must not publish historical identity"
        );
        assert_eq!(
            fixture.backend.global_revision().unwrap(),
            global,
            "query must not commit business mutations"
        );
    }

    #[test]
    fn create_lost_output_original_receipt_survives_later_edits_deleted_credentials_and_backend_reopen(
    ) {
        let mut fixture = Fixture::new();
        let admin = fixture.admin();
        let expected = fixture.backend.state().unwrap().revision;
        let issued = issue_identity(&fixture, admin, SessionPurpose::Manage);
        let original = NewAccount {
            id: id(5),
            login_name: "lost-create".into(),
            display_name: "Original created teacher".into(),
            role: Role::TeacherAdmin,
            scopes: vec![scope(1, 1), scope(2, 2)],
        };
        let mut published = None;
        let code = fixture
            .backend
            .begin_command_scope(issued)
            .unwrap()
            .create_account_at_tracked(
                admin,
                expected,
                original.clone(),
                Password::new(INITIAL_PASSWORD.into()).unwrap(),
                |proof| {
                    published = Some(Arc::new(proof));
                    Ok(())
                },
            )
            .unwrap();
        // Neither revealing nor keeping the original one-time result is needed
        // to query the committed public account facts later.
        drop(code);
        let proof =
            published.expect("original prepared fingerprint survives discarded completion output");
        assert_eq!(proof.issued(), issued);
        assert_eq!(
            proof.request().command,
            IdentityCommand::CreateAccount(original.clone())
        );
        let receipt = row(&fixture, receipt_key(issued)).unwrap();
        let receipt_text = std::str::from_utf8(&receipt.value).unwrap();
        assert!(receipt_text.contains("\"fingerprint\""));
        assert!(!receipt_text.contains(INITIAL_PASSWORD));
        assert!(!receipt_text.contains("recovery_code"));
        assert!(!receipt_text.contains("\"password\""));
        fixture.assert_credentials(5);

        fixture
            .backend
            .test_native_command(
                CommandFamily::Identity,
                Some(admin),
                SessionPurpose::Manage,
                |backend| {
                    backend.identity(
                        admin,
                        update(
                            5,
                            "Later edited current teacher",
                            Role::TeacherAdmin,
                            true,
                            vec![scope(1, 2)],
                        ),
                    )
                },
            )
            .unwrap();
        let first_login = fixture.issue(5, TrustedOrigin::Local);
        let password_command =
            issue_identity(&fixture, first_login, SessionPurpose::ChangeInitialPassword);
        fixture
            .backend
            .begin_command_scope(password_command)
            .unwrap()
            .change_password(
                first_login,
                Password::new(INITIAL_PASSWORD.into()).unwrap(),
                Password::new(CHANGED_PASSWORD.into()).unwrap(),
            )
            .unwrap();
        let changed_login = fixture.issue(5, TrustedOrigin::Local);
        let rotation = issue_identity(&fixture, changed_login, SessionPurpose::Manage);
        let replacement_code = fixture
            .backend
            .begin_command_scope(rotation)
            .unwrap()
            .rotate_recovery(
                changed_login,
                Password::new(CHANGED_PASSWORD.into()).unwrap(),
            )
            .unwrap();
        drop(replacement_code);
        assert!(row(&fixture, "business/credential/5/password/1".into()).is_none());
        assert!(row(&fixture, "business/credential/5/recovery/1".into()).is_none());
        fixture.assert_credentials(5);
        let before_reopen = fixture.backend.state().unwrap();
        assert_eq!(before_reopen.account(id(5)).unwrap().credential_version, 2);
        assert_eq!(before_reopen.account(id(5)).unwrap().recovery_version, 2);
        fixture.reopen();
        let fresh_admin = fixture.admin();
        assert_ne!(
            fresh_admin, admin,
            "a genuinely new exact local lease is required after reopen"
        );
        let current = fixture.backend.management_identity(fresh_admin).unwrap();
        assert_eq!(current, before_reopen);
        let global = fixture.backend.global_revision().unwrap();
        let credential_rows = credentials(&fixture, 5);
        let anchor = issue_identity(&fixture, fresh_admin, SessionPurpose::Manage);
        let historical = query(&mut fixture, fresh_admin, issued, proof.clone())
            .unwrap()
            .unwrap();
        assert!(historical.matches(expected, &IdentityCommand::CreateAccount(original.clone())));
        assert_eq!(historical.account.display_name, original.display_name);
        assert_eq!(historical.scopes, original.scopes);
        assert!(historical.account.must_change_password);
        assert_eq!(historical.account.credential_version, 1);
        assert_eq!(historical.account.recovery_version, 1);
        assert_eq!(
            fixture
                .backend
                .management_identity(fresh_admin)
                .unwrap()
                .account(id(5))
                .unwrap()
                .display_name,
            "Later edited current teacher"
        );
        assert_eq!(
            query(&mut fixture, fresh_admin, issued, proof).unwrap(),
            Some(historical)
        );
        assert_read_only(&fixture, &current, global);
        assert_eq!(credentials(&fixture, 5), credential_rows);
        assert_eq!(row(&fixture, receipt_key(issued)), Some(receipt));
        let after = issue_identity(&fixture, fresh_admin, SessionPurpose::Manage);
        assert_eq!(after.ticket.epoch(), anchor.ticket.epoch());
        assert_eq!(
            after.ticket.serial(),
            anchor.ticket.serial() + 1,
            "history lookup must not issue replacement commands"
        );
        assert_read_only(&fixture, &current, global);
    }

    #[test]
    fn update_lost_output_returns_original_public_role_and_scopes_without_overwriting_current_or_reusing_old_lease(
    ) {
        let mut fixture = Fixture::new();
        let admin = fixture.admin();
        let original_command = update(
            2,
            "Original updated teacher",
            Role::TeacherAdmin,
            true,
            vec![scope(1, 1), scope(2, 2)],
        );
        let expected = fixture.backend.state().unwrap().revision;
        let (issued, proof, discarded_completion) =
            tracked_update(&mut fixture, admin, original_command.clone());
        drop(discarded_completion);
        let receipt = row(&fixture, receipt_key(issued)).unwrap();
        fixture
            .backend
            .test_native_command(
                CommandFamily::Identity,
                Some(admin),
                SessionPurpose::Manage,
                |backend| {
                    backend.identity(
                        admin,
                        update(2, "Later current user", Role::User, true, vec![]),
                    )
                },
            )
            .unwrap();
        let current = fixture.backend.management_identity(admin).unwrap();
        let global = fixture.backend.global_revision().unwrap();
        let credential_rows = credentials(&fixture, 2);
        let original = query(&mut fixture, admin, issued, proof.clone())
            .unwrap()
            .unwrap();
        assert!(original.matches(expected, &original_command));
        assert_eq!(original.account.principal.role, Role::TeacherAdmin);
        assert_eq!(original.scopes, vec![scope(1, 1), scope(2, 2)]);
        assert_eq!(current.account(id(2)).unwrap().principal.role, Role::User);
        assert_read_only(&fixture, &current, global);

        fixture.backend.session_control().lock_local();
        assert!(
            query(&mut fixture, admin, issued, proof.clone()).is_err(),
            "locked original lease cannot query"
        );
        let new_admin = fixture.admin();
        assert_ne!(new_admin, admin);
        assert!(fixture.backend.management_identity(admin).is_err());
        assert_eq!(
            query(&mut fixture, new_admin, issued, proof).unwrap(),
            Some(original)
        );
        assert_read_only(&fixture, &current, global);
        assert_eq!(credentials(&fixture, 2), credential_rows);
        assert_eq!(row(&fixture, receipt_key(issued)), Some(receipt));
    }

    #[test]
    fn account_history_missing_receipt_stays_missing_and_wrong_pair_or_full_payload_is_rejected_read_only(
    ) {
        let mut fixture = Fixture::new();
        let admin = fixture.admin();
        let current = fixture.backend.management_identity(admin).unwrap();
        let missing = issue_identity(&fixture, admin, SessionPurpose::Manage);
        let mut captured = None;
        let refused = fixture
            .backend
            .begin_command_scope(missing)
            .unwrap()
            .update_account_at_tracked(
                admin,
                current.revision,
                update(
                    2,
                    "Prepared but never submitted",
                    Role::TeacherAdmin,
                    true,
                    vec![scope(1, 1)],
                ),
                |proof| {
                    captured = Some(Arc::new(proof));
                    Err(BackendError::Closed)
                },
            );
        assert_eq!(refused, Err(BackendError::Closed));
        let missing_proof = captured.expect("writer is not invoked after failed metadata handoff");
        let global = fixture.backend.global_revision().unwrap();
        assert!(row(&fixture, receipt_key(missing)).is_none());
        assert_eq!(
            query(&mut fixture, admin, missing, missing_proof.clone()).unwrap(),
            None
        );
        assert_eq!(
            query(&mut fixture, admin, missing, missing_proof.clone()).unwrap(),
            None
        );
        assert!(
            row(&fixture, receipt_key(missing)).is_none(),
            "query cannot manufacture a refusal or success receipt"
        );
        assert_read_only(&fixture, &current, global);

        let (committed, proof, _) = tracked_update(
            &mut fixture,
            admin,
            update(
                2,
                "Actual committed result",
                Role::TeacherAdmin,
                true,
                vec![scope(2, 2)],
            ),
        );
        let current = fixture.backend.management_identity(admin).unwrap();
        let global = fixture.backend.global_revision().unwrap();
        let receipt = row(&fixture, receipt_key(committed)).unwrap();
        assert_eq!(
            query(&mut fixture, admin, committed, missing_proof)
                .unwrap_err()
                .command_window_error(),
            Some(CommandWindowError::NotIssued)
        );
        assert_eq!(
            query(&mut fixture, admin, missing, proof.clone())
                .unwrap_err()
                .command_window_error(),
            Some(CommandWindowError::NotIssued)
        );
        // A real alternate typed request under the original scoped ticket has
        // a different complete digest; no private proof fields are forged.
        let mut wrong_request = proof.request().clone();
        wrong_request.command = update(
            2,
            "Different original payload",
            Role::TeacherAdmin,
            true,
            vec![scope(2, 2)],
        );
        let wrong_proof = {
            let backend = fixture.backend.begin_command_scope(committed).unwrap();
            let actor = backend
                .check_session(admin, SessionPurpose::Manage)
                .unwrap();
            Arc::new(
                backend
                    .repository
                    .prepare_account_update_receipt(&wrong_request, &actor)
                    .unwrap(),
            )
        };
        assert_eq!(
            query(&mut fixture, admin, committed, wrong_proof),
            Err(BackendError::Identity(
                IdentityRepositoryError::IdempotencyConflict
            ))
        );
        assert_eq!(row(&fixture, receipt_key(committed)), Some(receipt));
        assert!(query(&mut fixture, admin, committed, proof)
            .unwrap()
            .is_some());
        assert_read_only(&fixture, &current, global);
    }

    #[test]
    fn account_history_self_update_new_epoch_and_other_owner_cannot_disclose_original_result() {
        let mut fixture = Fixture::new();
        let admin = fixture.admin();
        let (issued, proof, discarded_completion) = tracked_update(
            &mut fixture,
            admin,
            update(
                1,
                "Committed self rename with lost output",
                Role::GlobalAdmin,
                true,
                vec![],
            ),
        );
        drop(discarded_completion);
        let current = fixture.backend.state().unwrap();
        let global = fixture.backend.global_revision().unwrap();
        let receipt = row(&fixture, receipt_key(issued)).unwrap();
        assert!(
            query(&mut fixture, admin, issued, proof.clone()).is_err(),
            "the real self write revokes its original exact lease"
        );
        let reauthenticated = fixture.admin();
        assert_eq!(
            fixture
                .backend
                .management_identity(reauthenticated)
                .unwrap(),
            current
        );
        assert_eq!(
            query(&mut fixture, reauthenticated, issued, proof.clone())
                .unwrap_err()
                .command_window_error(),
            Some(CommandWindowError::NotIssued),
            "same principal with a new auth epoch cannot assume the old command owner"
        );
        assert_read_only(&fixture, &current, global);
        let second_admin = fixture.issue(3, TrustedOrigin::Local);
        assert_eq!(
            fixture.backend.management_identity(second_admin).unwrap(),
            current
        );
        assert_eq!(
            query(&mut fixture, second_admin, issued, proof.clone())
                .unwrap_err()
                .command_window_error(),
            Some(CommandWindowError::NotIssued),
            "another global administrator cannot read another owner's receipt"
        );
        assert_eq!(row(&fixture, receipt_key(issued)), Some(receipt));
        assert_read_only(&fixture, &current, global);

        fixture
            .backend
            .test_native_command(
                CommandFamily::Identity,
                Some(second_admin),
                SessionPurpose::Manage,
                |backend| {
                    backend.identity(
                        second_admin,
                        update(1, "Now demoted current user", Role::User, true, vec![]),
                    )
                },
            )
            .unwrap();
        let demoted_state = fixture.backend.state().unwrap();
        let demoted_global = fixture.backend.global_revision().unwrap();
        assert!(query(&mut fixture, reauthenticated, issued, proof.clone()).is_err());
        let demoted_lease = fixture.issue(1, TrustedOrigin::Local);
        assert!(fixture.backend.management_identity(demoted_lease).is_err());
        assert!(
            query(&mut fixture, demoted_lease, issued, proof).is_err(),
            "the ticket is not a substitute for current management permission"
        );
        assert_read_only(&fixture, &demoted_state, demoted_global);
    }
}
