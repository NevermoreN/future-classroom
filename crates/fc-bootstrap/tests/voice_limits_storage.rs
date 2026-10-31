//! Configurable voice policy against the real sole SQLite writer.
use fc_domain::{auth::*, identity::*, ids::*, time::*};
use fc_ports::{
    prepare_initial_credentials, prepare_password_change, Clock, CredentialCrypto, CredentialError,
    CredentialIdentityRepository, IdentityRepository, IdentityRepositoryError, IdentityRequest,
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
            "FutureClassroom-VoiceLimits-{}-{}-{}",
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
use fc_domain::voice::*;
use fc_ports::voice::*;
struct Preparation;
impl VoicePreparation for Preparation {
    type Input = ();
    fn prepare(&mut self, _: (), _: &VoiceAsset) -> Result<VoicePcmMetadata, VoiceRepositoryError> {
        Ok(VoicePcmMetadata {
            byte_length: 16_044,
            frame_count: 8_000,
            sha256: [7; 32],
        })
    }
}
fn bind(repo: &mut BusinessRepository<Snapshot>, clock: &Time) {
    let revision = repo.load_identity().unwrap().revision;
    repo.execute_identity(
        IdentityRequest {
            command_id: CommandId::new(99).unwrap(),
            expected_revision: revision,
            command: IdentityCommand::BindCurrentClass {
                id: ClassId::new(1).unwrap(),
            },
        },
        Some(&actor(1, 1, TrustedOrigin::Local)),
        clock,
    )
    .unwrap();
}
fn ready(repo: &BusinessRepository<Snapshot>, id: u64, a: &ActorContext) -> VoiceAsset {
    let reservation = repo
        .reserve_voice_upload(VoiceAssetId::new(id).unwrap(), ClassId::new(1).unwrap(), a)
        .unwrap();
    let proof = prepare_voice_upload(&mut Preparation, (), reservation).unwrap();
    repo.publish_voice_upload(proof, a).unwrap()
}
fn submit(
    repo: &BusinessRepository<Snapshot>,
    id: u64,
    asset: &VoiceAsset,
    total: u8,
    a: &ActorContext,
) -> VoiceJob {
    repo.submit_voice(
        VoiceSubmitRequest {
            command_id: CommandId::new(id).unwrap(),
            asset_id: asset.id,
            expected_asset_revision: asset.revision,
            class_id: asset.class_id,
            repeat_total: total,
        },
        a,
    )
    .unwrap()
}
fn drive(
    repo: &BusinessRepository<Snapshot>,
    j: &VoiceJob,
    action: VoiceDriverAction,
) -> Result<VoiceJob, VoiceRepositoryError> {
    repo.drive_voice(VoiceDriverUpdate {
        job_id: j.id,
        expected_revision: j.revision,
        generation: if action == VoiceDriverAction::Released {
            j.release_generation.unwrap()
        } else {
            j.generation
        },
        action,
    })
}

fn small_limits() -> VoiceLimits {
    VoiceLimits {
        max_duration_millis: 500,
        max_pending_jobs: 1,
        max_pending_per_owner: 1,
        max_repeat_total: 1,
    }
}
#[test]
fn sqlite_policy_exact_authority_cas_and_guard_at_evaluation_and_precommit() {
    use std::sync::atomic::AtomicUsize;
    let dir = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (_writer, mut repo) = open(
        &dir,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let local = actor(1, 1, TrustedOrigin::Local);
    let revision = repo.load_identity().unwrap().revision;
    repo.execute_identity_with_credentials(
        IdentityRequest {
            command_id: CommandId::new(50).unwrap(),
            expected_revision: revision,
            command: IdentityCommand::CreateAccount(NewAccount {
                id: principal(4),
                login_name: "newadmin".into(),
                display_name: "New admin".into(),
                role: Role::GlobalAdmin,
                scopes: vec![],
            }),
        },
        Some(&local),
        credentials(4, 1, true),
    )
    .unwrap();
    let before = repo.voice_inventory().unwrap();
    assert!(repo
        .set_voice_limits(
            small_limits(),
            before.revision,
            &actor(4, 1, TrustedOrigin::Local)
        )
        .is_err());
    let mut expired = local.clone();
    expired.expires_at = time(29);
    assert!(repo
        .set_voice_limits(small_limits(), before.revision, &expired)
        .is_err());
    assert_eq!(
        repo.set_voice_limits(
            small_limits(),
            before.revision,
            &actor(1, 1, TrustedOrigin::Web)
        ),
        Err(VoiceRepositoryError::Domain(VoiceError::Permission(
            PermissionError::LocalOnly
        )))
    );
    assert!(repo
        .set_voice_limits(
            small_limits(),
            before.revision,
            &actor(2, 2, TrustedOrigin::Local)
        )
        .is_err());
    assert!(repo
        .set_voice_limits(
            small_limits(),
            before.revision,
            &actor(1, 2, TrustedOrigin::Local)
        )
        .is_err());
    assert_eq!(
        repo.set_voice_limits(
            VoiceLimits {
                max_pending_jobs: 0,
                ..small_limits()
            },
            before.revision,
            &local
        ),
        Err(VoiceRepositoryError::Domain(VoiceError::InvalidState))
    );
    assert_eq!(
        repo.set_voice_limits(small_limits(), Revision::new(99), &local),
        Err(VoiceRepositoryError::Domain(VoiceError::RevisionConflict))
    );
    assert_eq!(repo.voice_inventory().unwrap(), before);
    // Use the real transaction's two exact-session checkpoints; reject at the
    // second one and verify neither policy nor a stop request became durable.
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let guarded = repo.clone().with_session_guard(Arc::new(move |_, _| {
        if seen.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(())
        } else {
            Err(StorageError::AuthorityUnavailable)
        }
    }));
    assert!(guarded
        .set_voice_limits(small_limits(), before.revision, &local)
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(repo.voice_inventory().unwrap(), before);
    let denied = repo
        .clone()
        .with_session_guard(Arc::new(|_, _| Err(StorageError::AuthorityUnavailable)));
    assert!(denied
        .set_voice_limits(small_limits(), before.revision, &local)
        .is_err());
    assert_eq!(repo.voice_inventory().unwrap(), before);
    let changed = repo
        .set_voice_limits(small_limits(), before.revision, &local)
        .unwrap();
    assert_eq!(changed.limits, small_limits());
    assert!(changed.enabled);
    assert_eq!(
        repo.set_voice_limits(VoiceLimits::default(), before.revision, &local),
        Err(VoiceRepositoryError::Domain(VoiceError::RevisionConflict))
    );
}
#[test]
fn sqlite_policy_atomic_stop_retirement_exact_old_release_and_reopen() {
    let dir = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (writer, mut repo) = open(
        &dir,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    bind(&mut repo, &clock);
    let local = actor(1, 1, TrustedOrigin::Local);
    let web = actor(2, 2, TrustedOrigin::Web);
    let other = actor(3, 2, TrustedOrigin::Web);
    let a = ready(&repo, 100, &web);
    let queued = submit(&repo, 1000, &a, 10, &web);
    let starting = drive(&repo, &queued, VoiceDriverAction::Start).unwrap();
    let playing = drive(&repo, &starting, VoiceDriverAction::FirstWrite).unwrap();
    let b = ready(&repo, 101, &other);
    let waiting = submit(&repo, 1001, &b, 10, &other);
    let unused = ready(&repo, 102, &web);
    let reserved = repo
        .reserve_voice_upload(
            VoiceAssetId::new(103).unwrap(),
            ClassId::new(1).unwrap(),
            &other,
        )
        .unwrap();
    let before = repo.voice_inventory().unwrap();
    let stopped = repo
        .set_voice_limits(small_limits(), before.revision, &local)
        .unwrap();
    assert!(stopped.enabled);
    assert_eq!(stopped.limits, small_limits());
    assert!(stopped
        .assets
        .iter()
        .filter(|a| a.id == unused.id || a.id == reserved.id)
        .all(|a| a.state == VoiceAssetState::DeletePending));
    let old_owner = stopped.jobs.iter().find(|j| j.id == playing.id).unwrap();
    assert_eq!(old_owner.state, VoiceState::Stopping);
    assert_eq!(old_owner.release_generation, Some(playing.generation));
    assert_ne!(old_owner.generation, playing.generation);
    assert_eq!(old_owner.repeat_total, 10);
    assert_eq!(old_owner.completed_repeats, 0);
    assert_eq!(
        stopped
            .jobs
            .iter()
            .find(|j| j.id == waiting.id)
            .unwrap()
            .state,
        VoiceState::Failed
    );
    assert_eq!(
        stopped
            .jobs
            .iter()
            .find(|j| j.id == waiting.id)
            .unwrap()
            .repeat_total,
        10
    );
    stopped.validate().unwrap();
    assert_eq!(
        repo.drive_voice(VoiceDriverUpdate {
            job_id: old_owner.id,
            expected_revision: old_owner.revision,
            generation: old_owner.generation,
            action: VoiceDriverAction::Released
        }),
        Err(VoiceRepositoryError::Domain(VoiceError::StaleGeneration))
    );
    let released = drive(&repo, old_owner, VoiceDriverAction::Released).unwrap();
    assert_eq!(released.state, VoiceState::Failed);
    assert_eq!(released.repeat_total, 10);
    assert_eq!(released.completed_repeats, 0);
    drop(repo);
    drop(writer);
    let startup = fc_storage::business::startup_state(dir.path()).unwrap();
    let (writer, repo) = open(&dir, clock, startup.identity, startup.revision);
    let persisted = repo.voice_inventory().unwrap();
    assert_eq!(persisted.limits, small_limits());
    assert!(persisted.enabled);
    assert!(persisted.jobs.iter().all(|j| j.state.terminal()));
    assert!(persisted.jobs.iter().all(|j| j.repeat_total == 10));
    let fresh = ready(&repo, 104, &web);
    let request = VoiceSubmitRequest {
        command_id: CommandId::new(1002).unwrap(),
        asset_id: fresh.id,
        expected_asset_revision: fresh.revision,
        class_id: fresh.class_id,
        repeat_total: 2,
    };
    assert_eq!(
        repo.submit_voice(request.clone(), &web),
        Err(VoiceRepositoryError::Domain(VoiceError::RepeatOutOfRange))
    );
    let accepted = repo
        .submit_voice(
            VoiceSubmitRequest {
                repeat_total: 1,
                ..request
            },
            &web,
        )
        .unwrap();
    assert_eq!(accepted.repeat_total, 1);
    let another = ready(&repo, 105, &other);
    assert_eq!(
        repo.submit_voice(
            VoiceSubmitRequest {
                command_id: CommandId::new(1003).unwrap(),
                asset_id: another.id,
                expected_asset_revision: another.revision,
                class_id: another.class_id,
                repeat_total: 1
            },
            &other
        ),
        Err(VoiceRepositoryError::Domain(VoiceError::QueueFull))
    );
    drop(repo);
    drop(writer);
}
#[test]
fn sqlite_policy_changes_preserve_disabled_switch_and_runtime_generation_guard() {
    let dir = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (_writer, mut repo) = open(
        &dir,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    let local = actor(1, 1, TrustedOrigin::Local);
    let initial = repo.voice_inventory().unwrap();
    let disabled = repo
        .set_voice_enabled(false, initial.revision, &local)
        .unwrap();
    let no_generation = repo
        .clone()
        .with_voice_timeout(std::time::Duration::from_secs(1), Arc::new(|| false));
    assert!(no_generation
        .set_voice_limits(small_limits(), disabled.revision, &local)
        .is_err());
    assert_eq!(repo.voice_inventory().unwrap(), disabled);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = calls.clone();
    let invalidated_at_commit = repo.clone().with_voice_timeout(
        std::time::Duration::from_secs(1),
        Arc::new(move || seen.fetch_add(1, Ordering::SeqCst) < 2),
    );
    assert!(invalidated_at_commit
        .set_voice_limits(small_limits(), disabled.revision, &local)
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(repo.voice_inventory().unwrap(), disabled);
    let changed = repo
        .set_voice_limits(small_limits(), disabled.revision, &local)
        .unwrap();
    assert!(!changed.enabled);
    assert_eq!(changed.limits, small_limits());
}

struct Frames(u32);
impl VoicePreparation for Frames {
    type Input = ();
    fn prepare(&mut self, _: (), _: &VoiceAsset) -> Result<VoicePcmMetadata, VoiceRepositoryError> {
        Ok(VoicePcmMetadata {
            byte_length: 44 + 2 * u64::from(self.0),
            frame_count: self.0,
            sha256: [8; 32],
        })
    }
}
#[test]
fn sqlite_policy_publish_duration_and_per_owner_queue_are_actual_writer_checks() {
    let dir = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (_writer, mut repo) = open(
        &dir,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    bind(&mut repo, &clock);
    let local = actor(1, 1, TrustedOrigin::Local);
    let web = actor(2, 2, TrustedOrigin::Web);
    let other = actor(3, 2, TrustedOrigin::Web);
    let before = repo.voice_inventory().unwrap();
    repo.set_voice_limits(
        VoiceLimits {
            max_pending_jobs: 4,
            ..small_limits()
        },
        before.revision,
        &local,
    )
    .unwrap();
    let reserved = repo
        .reserve_voice_upload(
            VoiceAssetId::new(200).unwrap(),
            ClassId::new(1).unwrap(),
            &web,
        )
        .unwrap();
    let proof = prepare_voice_upload(&mut Frames(8_001), (), reserved.clone()).unwrap();
    let before = repo.voice_inventory().unwrap();
    assert_eq!(
        repo.publish_voice_upload(proof, &web),
        Err(VoiceRepositoryError::Domain(VoiceError::InvalidWav))
    );
    assert_eq!(repo.voice_inventory().unwrap(), before);
    let proof = prepare_voice_upload(&mut Frames(8_000), (), reserved).unwrap();
    let first = repo.publish_voice_upload(proof, &web).unwrap();
    submit(&repo, 2000, &first, 1, &web);
    let second = ready(&repo, 201, &web);
    assert_eq!(
        repo.submit_voice(
            VoiceSubmitRequest {
                command_id: CommandId::new(2001).unwrap(),
                asset_id: second.id,
                expected_asset_revision: second.revision,
                class_id: second.class_id,
                repeat_total: 1
            },
            &web
        ),
        Err(VoiceRepositoryError::Domain(VoiceError::OwnerLimit))
    );
    let independent = ready(&repo, 202, &other);
    submit(&repo, 2002, &independent, 1, &other);
    assert_eq!(
        repo.voice_inventory()
            .unwrap()
            .jobs
            .iter()
            .filter(|j| !j.state.terminal())
            .count(),
        2
    );
}
