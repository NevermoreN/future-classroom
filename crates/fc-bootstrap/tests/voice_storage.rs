//! Durable voice against real SQLite, exact leases and authority publication.
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
#[test]
fn sqlite_voice_commit_idempotency_cancel_release_and_restart_never_replay() {
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
    let web = actor(2, 2, TrustedOrigin::Web);
    let asset = ready(&repo, 100, &web);
    let request = VoiceSubmitRequest {
        command_id: CommandId::new(1000).unwrap(),
        asset_id: asset.id,
        expected_asset_revision: asset.revision,
        class_id: asset.class_id,
        repeat_total: 3,
    };
    let queued = repo.submit_voice(request.clone(), &web).unwrap();
    assert_eq!(repo.submit_voice(request.clone(), &web).unwrap(), queued);
    assert_eq!(
        repo.submit_voice(
            VoiceSubmitRequest {
                repeat_total: 4,
                ..request.clone()
            },
            &web
        ),
        Err(VoiceRepositoryError::IdempotencyConflict)
    );
    let mut playing = drive(&repo, &queued, VoiceDriverAction::Start).unwrap();
    assert_eq!(playing.state, VoiceState::Starting);
    playing = drive(&repo, &playing, VoiceDriverAction::FirstWrite).unwrap();
    assert_eq!(playing.state, VoiceState::Playing);
    let cancel = VoiceCancelRequest {
        command_id: CommandId::new(1001).unwrap(),
        job_id: playing.id,
        expected_revision: playing.revision,
    };
    let stopping = repo.cancel_voice(cancel.clone(), &web).unwrap();
    assert_eq!(stopping.state, VoiceState::Stopping);
    assert_eq!(repo.cancel_voice(cancel, &web).unwrap(), stopping);
    assert_eq!(stopping.completed_repeats, 0);
    assert_eq!(
        repo.drive_voice(VoiceDriverUpdate {
            job_id: playing.id,
            expected_revision: stopping.revision,
            generation: playing.generation,
            action: VoiceDriverAction::NaturalComplete
        }),
        Err(VoiceRepositoryError::Domain(VoiceError::StaleGeneration))
    );
    let done = drive(&repo, &stopping, VoiceDriverAction::Released).unwrap();
    assert_eq!(done.state, VoiceState::Canceled);
    assert_eq!(done.completed_repeats, 0);
    let second = ready(&repo, 101, &web);
    let _queued = submit(&repo, 1002, &second, 10, &web);
    let third = ready(&repo, 102, &actor(3, 2, TrustedOrigin::Web));
    let _third = submit(&repo, 1003, &third, 1, &actor(3, 2, TrustedOrigin::Web));
    drop(repo);
    drop(writer);
    let startup = fc_storage::business::startup_state(dir.path()).unwrap();
    let (writer, repo) = open(&dir, clock.clone(), startup.identity, startup.revision);
    let recovered = repo.recover_voice().unwrap();
    assert_eq!(
        recovered
            .jobs
            .iter()
            .filter(|j| j.state == VoiceState::Interrupted)
            .count(),
        2
    );
    assert!(recovered.jobs.iter().all(|j| j.state.terminal()));
    assert!(recovered
        .assets
        .iter()
        .all(|a| a.state == VoiceAssetState::DeletePending));
    let bytes = recovered.charged_bytes().unwrap();
    for a in recovered.assets {
        repo.finalize_voice_delete(a.id, a.revision).unwrap();
    }
    assert!(bytes > 0);
    let clean = repo.voice_inventory().unwrap();
    assert_eq!(clean.charged_bytes().unwrap(), 0);
    assert!(clean.jobs.iter().all(|j| j.asset_id.is_none()));
    drop(repo);
    drop(writer);
}
#[test]
fn sqlite_voice_exact_lease_binding_revocation_and_fifo_limits() {
    use std::sync::atomic::AtomicBool;
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
    let allowed = Arc::new(AtomicBool::new(true));
    let check = allowed.clone();
    repo = repo.with_session_guard(Arc::new(move |_, _| {
        if check.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(StorageError::AuthorityUnavailable)
        }
    }));
    let a = actor(2, 2, TrustedOrigin::Web);
    let b = actor(3, 2, TrustedOrigin::Web);
    let global = actor(1, 1, TrustedOrigin::Web);
    assert_eq!(
        repo.reserve_voice_upload(
            VoiceAssetId::new(1).unwrap(),
            ClassId::new(1).unwrap(),
            &actor(2, 2, TrustedOrigin::Local)
        ),
        Err(VoiceRepositoryError::Domain(VoiceError::Permission(
            PermissionError::WebOnly
        )))
    );
    let x = ready(&repo, 1, &a);
    let first = submit(&repo, 100, &x, 1, &a);
    let x = ready(&repo, 2, &a);
    let _ = submit(&repo, 101, &x, 1, &a);
    let x = ready(&repo, 3, &a);
    assert_eq!(
        repo.submit_voice(
            VoiceSubmitRequest {
                command_id: CommandId::new(102).unwrap(),
                asset_id: x.id,
                expected_asset_revision: x.revision,
                class_id: x.class_id,
                repeat_total: 1
            },
            &a
        ),
        Err(VoiceRepositoryError::Domain(VoiceError::OwnerLimit))
    );
    let x = ready(&repo, 4, &b);
    let next = submit(&repo, 103, &x, 1, &b);
    let x = ready(&repo, 5, &b);
    let _ = submit(&repo, 104, &x, 1, &b);
    let x = ready(&repo, 6, &global);
    assert_eq!(
        repo.submit_voice(
            VoiceSubmitRequest {
                command_id: CommandId::new(105).unwrap(),
                asset_id: x.id,
                expected_asset_revision: x.revision,
                class_id: x.class_id,
                repeat_total: 1
            },
            &global
        ),
        Err(VoiceRepositoryError::Domain(VoiceError::QueueFull))
    );
    assert_eq!(
        repo.voice_job(first.id, &b),
        Err(VoiceRepositoryError::Domain(VoiceError::Permission(
            PermissionError::WrongOwner
        )))
    );
    assert_eq!(
        drive(&repo, &next, VoiceDriverAction::Start),
        Err(VoiceRepositoryError::Domain(VoiceError::InvalidState))
    );
    let started = drive(&repo, &first, VoiceDriverAction::Start).unwrap();
    allowed.store(false, Ordering::Release);
    assert_eq!(
        drive(&repo, &started, VoiceDriverAction::FirstWrite),
        Err(VoiceRepositoryError::Faulted)
    );
    // Cancellation/release control is independent of the expired/revoked Web lease.
    let stopped = repo.emergency_stop_voice().unwrap();
    assert!(stopped
        .jobs
        .iter()
        .all(|j| j.state == VoiceState::Interrupted || j.state == VoiceState::Stopping));
    let current = stopped.jobs.iter().find(|j| j.id == first.id).unwrap();
    let released = drive(&repo, current, VoiceDriverAction::Released).unwrap();
    assert_eq!(released.state, VoiceState::Interrupted);
    assert_eq!(released.completed_repeats, 0);
    drop(repo);
    drop(writer);
}
#[test]
fn sqlite_voice_canonical_three_repeats_deadline_and_backup_strip_active_bytes() {
    let dir = Directory::new();
    let restored = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (writer, mut repo) = open(
        &dir,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    initialize(&mut repo, &clock);
    bind(&mut repo, &clock);
    let a = actor(2, 2, TrustedOrigin::Web);
    let asset = ready(&repo, 1, &a);
    let mut j = submit(&repo, 100, &asset, 3, &a);
    j = drive(&repo, &j, VoiceDriverAction::Start).unwrap();
    j = drive(&repo, &j, VoiceDriverAction::FirstWrite).unwrap();
    let live = repo.voice_inventory().unwrap();
    writer
        .0
        .as_ref()
        .unwrap()
        .handle()
        .try_backup(restored.path())
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(repo.voice_inventory().unwrap(), live);
    let restored_startup = fc_storage::business::startup_state(restored.path()).unwrap();
    let (restored_writer, restored_repo) = open(
        &restored,
        clock.clone(),
        restored_startup.identity,
        restored_startup.revision,
    );
    let backup = restored_repo.voice_inventory().unwrap();
    assert!(backup.assets.is_empty());
    assert_eq!(backup.jobs[0].state, VoiceState::Interrupted);
    assert!(backup.jobs[0].asset_id.is_none());
    assert!(backup.jobs[0].authorization.is_none());
    assert!(restored_writer
        .0
        .as_ref()
        .unwrap()
        .handle()
        .try_get("business/voice-receipt/v1/00000000000000000100".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    drop(restored_repo);
    drop(restored_writer);
    for n in 1..=3 {
        j = drive(&repo, &j, VoiceDriverAction::NaturalComplete).unwrap();
        assert_eq!(j.completed_repeats, n);
        if n < 3 {
            assert_eq!(j.state, VoiceState::Gap);
            j = drive(&repo, &j, VoiceDriverAction::Released).unwrap();
            assert_eq!(
                drive(&repo, &j, VoiceDriverAction::Start),
                Err(VoiceRepositoryError::Domain(VoiceError::InvalidState))
            );
            *clock.0.lock().unwrap() = LocalDateTime::new(date(30), 3600 + u32::from(n)).unwrap();
            j = drive(&repo, &j, VoiceDriverAction::Start).unwrap();
            j = drive(&repo, &j, VoiceDriverAction::FirstWrite).unwrap();
        } else {
            assert_eq!(j.state, VoiceState::Stopping);
            j = drive(&repo, &j, VoiceDriverAction::Released).unwrap();
            assert_eq!(j.state, VoiceState::Completed);
        }
    }
    let a2 = ready(&repo, 2, &a);
    let expired = submit(&repo, 101, &a2, 1, &a);
    *clock.0.lock().unwrap() = LocalDateTime::new(date(30), 3903).unwrap();
    let snapshot = repo.maintain_voice().unwrap();
    assert_eq!(
        snapshot
            .jobs
            .iter()
            .find(|j| j.id == expired.id)
            .unwrap()
            .state,
        VoiceState::Expired
    );
    assert_eq!(repo.voice_job(expired.id, &a).unwrap().completed_repeats, 0);
    drop(repo);
    drop(writer);
}
#[test]
fn sqlite_voice_audit_and_receipts_are_capped_and_age_out_together() {
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
    let a = actor(2, 2, TrustedOrigin::Web);
    for n in 1..=105 {
        *clock.0.lock().unwrap() = LocalDateTime::new(date(30), 3600 + n as u32 * 7).unwrap();
        let asset = ready(&repo, n, &a);
        let j = submit(&repo, 1000 + n, &asset, 3, &a);
        repo.cancel_voice(
            VoiceCancelRequest {
                command_id: CommandId::new(2000 + n).unwrap(),
                job_id: j.id,
                expected_revision: j.revision,
            },
            &a,
        )
        .unwrap();
        let s = repo.voice_inventory().unwrap();
        let a = s.assets.iter().find(|a| a.id == asset.id).unwrap();
        repo.finalize_voice_delete(a.id, a.revision).unwrap();
    }
    let s = repo.voice_inventory().unwrap();
    assert_eq!(s.jobs.len(), 100);
    assert_eq!(s.charged_bytes().unwrap(), 0);
    assert!(s
        .jobs
        .iter()
        .all(|j| j.authorization.is_none() && j.asset_id.is_none()));
    assert!(writer
        .0
        .as_ref()
        .unwrap()
        .handle()
        .try_get("business/voice-receipt/v1/00000000000000001001".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    *clock.0.lock().unwrap() = LocalDateTime::new(date(31), 5000).unwrap();
    let s = repo.maintain_voice().unwrap();
    assert!(s.jobs.is_empty());
    assert!(writer
        .0
        .as_ref()
        .unwrap()
        .handle()
        .try_get("business/voice-receipt/v1/00000000000000001105".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    drop(repo);
    drop(writer);
}
#[test]
fn sqlite_voice_first_boot_idle_recovery_is_read_only_and_expired_reads_are_redacted() {
    let dir = Directory::new();
    let clock = Arc::new(Time(Mutex::new(time(30))));
    let (writer, repo) = open(
        &dir,
        clock.clone(),
        IdentityState::default(),
        Revision::default(),
    );
    assert_eq!(repo.recover_voice().unwrap(), VoiceSnapshot::default());
    assert_eq!(
        writer
            .0
            .as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap(),
        Revision::default()
    );
    drop(repo);
    drop(writer);
    let startup = fc_storage::business::startup_state(dir.path()).unwrap();
    let (writer, mut repo) = open(&dir, clock.clone(), startup.identity, startup.revision);
    initialize(&mut repo, &clock);
    bind(&mut repo, &clock);
    let revision = writer
        .0
        .as_ref()
        .unwrap()
        .handle()
        .try_revision()
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(repo.recover_voice().unwrap(), VoiceSnapshot::default());
    assert_eq!(
        writer
            .0
            .as_ref()
            .unwrap()
            .handle()
            .try_revision()
            .unwrap()
            .wait()
            .unwrap(),
        revision
    );
    let a = actor(2, 2, TrustedOrigin::Web);
    let asset = ready(&repo, 1, &a);
    let request = VoiceSubmitRequest {
        command_id: CommandId::new(100).unwrap(),
        asset_id: asset.id,
        expected_asset_revision: asset.revision,
        class_id: asset.class_id,
        repeat_total: 3,
    };
    let j = repo.submit_voice(request.clone(), &a).unwrap();
    assert!(j.authorization.is_none());
    let cancel = VoiceCancelRequest {
        command_id: CommandId::new(101).unwrap(),
        job_id: j.id,
        expected_revision: j.revision,
    };
    let done = repo.cancel_voice(cancel.clone(), &a).unwrap();
    assert_eq!(done.state, VoiceState::Canceled);
    let inventory = repo.voice_inventory().unwrap();
    let resource = &inventory.assets[0];
    repo.finalize_voice_delete(resource.id, resource.revision)
        .unwrap();
    let retried = repo.submit_voice(request.clone(), &a).unwrap();
    assert_eq!(retried.state, VoiceState::Canceled);
    assert!(retried.authorization.is_none() && retried.asset_id.is_none());
    assert_eq!(
        repo.cancel_voice(cancel, &a).unwrap().state,
        VoiceState::Canceled
    );
    let row = writer
        .0
        .as_ref()
        .unwrap()
        .handle()
        .try_get("business/voice-receipt/v1/00000000000000000100".into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    let text = std::str::from_utf8(&row.value).unwrap();
    assert!(!text.contains("actor") && !text.contains("asset") && !text.contains("expiry"));
    *clock.0.lock().unwrap() = LocalDateTime::new(date(31), 3601).unwrap();
    assert_eq!(
        repo.voice_job(j.id, &a),
        Err(VoiceRepositoryError::Domain(VoiceError::Missing))
    );
    assert_eq!(
        repo.submit_voice(request, &a),
        Err(VoiceRepositoryError::Domain(VoiceError::Missing))
    );
    assert!(repo.maintain_voice().unwrap().jobs.is_empty());
    drop(repo);
    drop(writer);
}
#[test]
fn sqlite_voice_expire_before_write_ignores_revoked_lease_but_requires_exact_release() {
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
    let a = actor(2, 2, TrustedOrigin::Web);
    let asset = ready(&repo, 1, &a);
    let queued = submit(&repo, 100, &asset, 3, &a);
    assert_eq!(
        drive(&repo, &queued, VoiceDriverAction::ExpireBeforeWrite),
        Err(VoiceRepositoryError::Domain(VoiceError::InvalidState))
    );
    let starting = drive(&repo, &queued, VoiceDriverAction::Start).unwrap();
    *clock.0.lock().unwrap() = LocalDateTime::new(date(30), 3900).unwrap();
    repo = repo.with_session_guard(Arc::new(|_, _| Err(StorageError::AuthorityUnavailable)));
    let stopping = drive(&repo, &starting, VoiceDriverAction::ExpireBeforeWrite).unwrap();
    assert_eq!(stopping.state, VoiceState::Stopping);
    assert_eq!(stopping.pending_terminal, Some(VoiceState::Expired));
    assert_eq!(stopping.release_generation, Some(starting.generation));
    assert_ne!(stopping.generation, starting.generation);
    assert!(stopping.first_started_at.is_none());
    assert_eq!(stopping.completed_repeats, 0);
    assert_eq!(
        repo.drive_voice(VoiceDriverUpdate {
            job_id: stopping.id,
            expected_revision: stopping.revision,
            generation: stopping.generation,
            action: VoiceDriverAction::Released
        }),
        Err(VoiceRepositoryError::Domain(VoiceError::StaleGeneration))
    );
    let expired = drive(&repo, &stopping, VoiceDriverAction::Released).unwrap();
    assert_eq!(expired.state, VoiceState::Expired);
    assert!(expired.first_started_at.is_none());
    assert_eq!(expired.completed_repeats, 0);
    repo = repo.with_session_guard(Arc::new(|_, _| Ok(())));
    let asset = ready(&repo, 2, &a);
    let queued = submit(&repo, 101, &asset, 1, &a);
    *clock.0.lock().unwrap() = LocalDateTime::new(date(30), 4200).unwrap();
    repo = repo.with_session_guard(Arc::new(|_, _| Err(StorageError::AuthorityUnavailable)));
    let expired = drive(&repo, &queued, VoiceDriverAction::ExpireBeforeWrite).unwrap();
    assert_eq!(expired.state, VoiceState::Expired);
    assert_eq!(expired.generation, 0);
    assert!(expired.release_generation.is_none());
    drop(repo);
    drop(writer);
}
