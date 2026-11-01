//! Real Backend/SQLite exact-session profile integration. Setup credentials and
//! issued leases are synthetic; Windows codec key wrapping uses actual DPAPI.
use super::*;
use fc_ports::{CredentialError, CredentialIdentityRepository};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

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
        let path = std::env::temp_dir().join(format!(
            "FutureClassroom-ProfilesBackend-R16-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
struct Fixture {
    backend: Backend,
    directory: Directory,
    clock: Arc<ClockFixture>,
}
impl Fixture {
    fn new() -> Self {
        let directory = Directory::new();
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
            // Initial password change is a local component operation. No Web
            // pairing/listener readiness is fabricated to complete this seed.
            let lease = self.issue(principal.get(), TrustedOrigin::Local);
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
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.backend
            .close()
            .expect("account backend writer shutdown");
    }
}

fn key() -> SubjectProfileKey {
    SubjectProfileKey {
        class_id: ClassId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
        teacher_id: id(2),
    }
}
#[test]
fn profiles_backend_exact_management_lease_joint_cas_and_restart() {
    let mut f = Fixture::new();
    let admin = f.issue(1, TrustedOrigin::Local);
    let before = f.backend.read_public_layout(admin).unwrap();
    let public = f
        .backend
        .save_public_layout(
            admin,
            before.layout.revision,
            before.display.revision,
            before.layout,
            before.display,
        )
        .unwrap();
    assert_eq!(public.layout.revision, Revision::new(1));
    assert_eq!(public.display.revision, Revision::default());
    let draft = SubjectProfileDocument::new(ProfileId::new(900).unwrap(), key());
    let profile = f
        .backend
        .save_subject_profile(admin, key(), Revision::default(), draft.clone())
        .unwrap();
    assert_eq!(profile.id.get(), 1);
    assert_eq!(
        f.backend.list_subject_profiles(admin, None).unwrap().items[0].id,
        profile.id
    );
    assert_eq!(
        f.backend.read_subject_profile(admin, key()).unwrap(),
        Some(profile.clone())
    );
    assert!(
        matches!(f.backend.save_subject_profile(admin,key(),Revision::default(),draft),Err(BackendError::Profile(fc_ports::ProfileRepositoryError::Domain(fc_domain::profile::ProfileError::RevisionConflict{current})))if current==profile.revision)
    );
    let teacher = f.issue(2, TrustedOrigin::Local);
    assert!(f.backend.read_public_layout(teacher).is_err());
    assert!(f.backend.read_subject_profile(teacher, key()).is_err());
    assert!(f.backend.list_subject_profiles(teacher, None).is_err());
    assert!(f
        .backend
        .save_subject_profile(teacher, key(), profile.revision, profile.clone())
        .is_err());
    let web = f.issue(1, TrustedOrigin::Web);
    assert!(f.backend.read_public_layout(web).is_err());
    assert!(f
        .backend
        .save_subject_profile(web, key(), profile.revision, profile.clone())
        .is_err());
    let admin = f.issue(1, TrustedOrigin::Local);
    let initial = f.backend.read_public_layout(admin).unwrap();
    let mut changed = initial.display.clone();
    changed.clock_seconds = !changed.clock_seconds;
    let display = f
        .backend
        .save_display(admin, initial.display.revision, changed)
        .unwrap();
    assert!(
        matches!(f.backend.save_public_layout(admin,initial.layout.revision,initial.display.revision,initial.layout,initial.display),Err(BackendError::Profile(fc_ports::ProfileRepositoryError::Display(fc_domain::desktop::DisplayError::RevisionConflict{current})))if current==display.revision)
    );
    assert_eq!(
        f.backend.read_public_layout(admin).unwrap().layout,
        public.layout
    );
    f.backend.session_control().lock_local();
    assert!(f.backend.read_subject_profile(admin, key()).is_err());
    assert!(f
        .backend
        .save_subject_profile(admin, key(), profile.revision, profile.clone())
        .is_err());
    f.backend.close().unwrap();
    f.backend = Backend::open(
        f.directory.database(),
        f.directory.codec(),
        f.clock.clone(),
        f.clock.clone(),
    )
    .unwrap();
    assert!(f.backend.read_public_layout(admin).is_err());
    let fresh = f.issue(1, TrustedOrigin::Local);
    assert_eq!(
        f.backend.read_subject_profile(fresh, key()).unwrap(),
        Some(profile)
    );
    assert_eq!(
        f.backend.read_public_layout(fresh).unwrap().display,
        display
    );
}
#[test]
fn profiles_backend_retained_disabled_owner_can_be_managed_but_not_reenabled() {
    let mut f = Fixture::new();
    let admin = f.issue(1, TrustedOrigin::Local);
    let original = f
        .backend
        .save_subject_profile(
            admin,
            key(),
            Revision::default(),
            SubjectProfileDocument::new(ProfileId::new(900).unwrap(), key()),
        )
        .unwrap();
    let teacher = f.issue(2, TrustedOrigin::Web);
    let scopes = f
        .backend
        .state()
        .unwrap()
        .assignments
        .into_iter()
        .filter(|a| a.teacher_id == id(2))
        .map(|a| AssignmentScope {
            class_id: a.class_id,
            subject_id: a.subject_id,
        })
        .collect();
    f.backend
        .identity(
            admin,
            IdentityCommand::UpdateAccount {
                id: id(2),
                display_name: "Disabled teacher".into(),
                role: Role::TeacherAdmin,
                enabled: false,
                scopes,
            },
        )
        .unwrap();
    assert!(f
        .backend
        .check_session(teacher, SessionPurpose::Manage)
        .is_err());
    assert_eq!(
        f.backend.read_subject_profile(admin, key()).unwrap(),
        Some(original.clone())
    );
    let mut off = original.clone();
    off.enabled = false;
    let disabled = f
        .backend
        .save_subject_profile(admin, key(), original.revision, off)
        .unwrap();
    assert!(!disabled.enabled);
    assert_eq!(disabled.id, original.id);
    assert!(!f.backend.list_subject_profiles(admin, None).unwrap().items[0].enabled);
    let mut on = disabled.clone();
    on.enabled = true;
    assert_eq!(
        f.backend
            .save_subject_profile(admin, key(), disabled.revision, on),
        Err(BackendError::Profile(
            fc_ports::ProfileRepositoryError::Domain(
                fc_domain::profile::ProfileError::InvalidScope
            )
        ))
    );
    assert_eq!(
        f.backend.read_subject_profile(admin, key()).unwrap(),
        Some(disabled)
    );
}
