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
                .identity(
                    admin,
                    IdentityCommand::CreateClass {
                        id: ClassId::new(n).unwrap(),
                        name: format!("Class {n}"),
                    },
                )
                .unwrap();
            fixture
                .backend
                .identity(
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
                .unwrap();
        }
        fixture
            .backend
            .identity(
                admin,
                IdentityCommand::BindCurrentClass {
                    id: ClassId::new(1).unwrap(),
                },
            )
            .unwrap();
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
        let state = self.backend.state().unwrap();
        let principal = match &command {
            IdentityCommand::FirstAdministrator { id, .. } => *id,
            IdentityCommand::CreateAccount(spec) => spec.id,
            _ => unreachable!(),
        };
        let request = self.backend.request(&state, command).unwrap();
        let (change, _) =
            prepare_initial_credentials(&mut SyntheticCredentials, &(), request).unwrap();
        let (actor, generation) = admin
            .map(|lease| {
                self.backend
                    .actor(&state, lease, SessionPurpose::Manage)
                    .unwrap()
            })
            .unzip();
        let mut repository = match admin {
            Some(lease) => self
                .backend
                .guarded(lease, generation.unwrap(), SessionPurpose::Manage),
            None => self.backend.repository.clone(),
        };
        let transition = repository
            .execute_credentials(
                self.backend.encode(change, principal, 1, 1).unwrap(),
                actor.as_ref(),
                self.clock.as_ref(),
            )
            .unwrap();
        self.backend.apply(&transition).unwrap();
        if admin.is_some() {
            let lease = self.issue(principal.get(), TrustedOrigin::Web);
            let state = self.backend.state().unwrap();
            let request = self
                .backend
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
            let (actor, generation) = self
                .backend
                .actor(&state, lease, SessionPurpose::ChangeInitialPassword)
                .unwrap();
            let mut repository =
                self.backend
                    .guarded(lease, generation, SessionPurpose::ChangeInitialPassword);
            let transition = repository
                .execute_credentials(
                    self.backend.encode(change, principal, 2, 1).unwrap(),
                    Some(&actor),
                    self.clock.as_ref(),
                )
                .unwrap();
            self.backend.apply(&transition).unwrap();
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
        .create_account_at(
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
        .unwrap();
    drop(code);
    fixture.assert_credentials(5);
    let before = fixture.backend.state().unwrap();
    let global = fixture.backend.global_revision().unwrap();
    assert_eq!(
        fixture.backend.identity_at(
            admin,
            captured,
            update(5, "stale edit", Role::TeacherAdmin, true, vec![scope(1, 2)])
        ),
        Err(identity_error(IdentityError::RevisionConflict))
    );
    assert_eq!(fixture.backend.state().unwrap(), before);
    assert_eq!(fixture.backend.global_revision().unwrap(), global);
    assert_eq!(
        fixture
            .backend
            .create_account_at(
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
        .identity_at(
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
        .identity(
            web,
            update(2, "forbidden", Role::TeacherAdmin, true, vec![scope(1, 1)])
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
        .identity(
            teacher,
            update(2, "forbidden", Role::TeacherAdmin, true, vec![scope(1, 1)])
        )
        .is_err());
    let admin = fixture.admin();
    fixture
        .backend
        .identity(
            admin,
            update(3, "Disabled second", Role::GlobalAdmin, false, vec![]),
        )
        .unwrap();
    let before = fixture.backend.state().unwrap();
    let revision = fixture.backend.global_revision().unwrap();
    assert_eq!(
        fixture.backend.identity_at(
            admin,
            before.revision,
            update(1, "Cannot leave", Role::User, true, vec![])
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
        .identity(
            admin,
            update(3, "Enabled second", Role::GlobalAdmin, true, vec![]),
        )
        .unwrap();
    let renamed = fixture
        .backend
        .identity(
            admin,
            update(1, "Self renamed", Role::GlobalAdmin, true, vec![]),
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
        .identity(fresh, update(1, "Self demoted", Role::User, true, vec![]))
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
        .identity(
            fresh,
            IdentityCommand::RenameSchool {
                name: "stale session".into()
            }
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
        .identity(
            teacher,
            update(
                2,
                "Forbidden Web management",
                Role::TeacherAdmin,
                true,
                vec![self::scope(1, 1)]
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
    let material = fc_ports::prepare_material_append(
        &mut MaterialFixture,
        (),
        fc_ports::MaterialAppendMetadata {
            command_id: CommandId::new(900).unwrap(),
            scope,
            material_id: MaterialId::new(900).unwrap(),
            display_name: "Synthetic account material.txt".into(),
            allowed_class_ids: vec![ClassId::new(1).unwrap(), ClassId::new(2).unwrap()],
        },
    )
    .unwrap();
    fixture.backend.append_material(admin, material).unwrap();
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
    let job = fixture
        .backend
        .voice_submit(
            teacher,
            VoiceSubmitRequest {
                command_id: CommandId::new(901).unwrap(),
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
        .identity(
            admin,
            update(
                2,
                "Class two only",
                Role::TeacherAdmin,
                true,
                vec![self::scope(2, 1), self::scope(2, 2)],
            ),
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
        .identity(admin, update(2, "Read only user", Role::User, true, vec![]))
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
        .identity(admin, update(2, "Disabled user", Role::User, false, vec![]))
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
                .identity(
                    admin,
                    update(4, "Not currently teaching", Role::User, true, vec![]),
                )
                .unwrap();
        }
        if mode == "default" {
            fixture
                .backend
                .identity(
                    admin,
                    IdentityCommand::SetDefaultTeacher(DefaultTeacher {
                        class_id: ClassId::new(1).unwrap(),
                        subject_id: SubjectId::new(1).unwrap(),
                        teacher_id: id(2),
                    }),
                )
                .unwrap();
        }
        let document = fixture
            .backend
            .save_schedule(admin, Revision::default(), timetable(mode))
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
                fixture.backend.identity_at(admin, before.revision, command),
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
            .identity(
                admin,
                IdentityCommand::CreateClass {
                    id: ClassId::new(class).unwrap(),
                    name: format!("Class {class}"),
                },
            )
            .unwrap();
    }
    for subject in 3..=32 {
        fixture
            .backend
            .identity(
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
        fixture.backend.identity_at(admin, before.revision, command),
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
        .identity_at(
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
