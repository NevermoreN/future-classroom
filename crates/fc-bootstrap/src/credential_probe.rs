//! Isolated Windows diagnostic fixture. This headless probe deliberately runs real
//! KDF work synchronously; the product UI must dispatch it to bounded workers.
#![cfg(windows)]

use fc_application::identity::{IdentitySessions, SessionLease, SessionPurpose};
use fc_domain::{auth::*, identity::*, ids::*, time::*};
use fc_ports::{
    prepare_initial_credentials, prepare_password_change, prepare_recovery, Clock,
    CredentialCrypto, CredentialIdentityRepository, IdentityRepository, IdentityRepositoryError,
    IdentityRequest, MonotonicClock, PreparedCredentialChange,
};
use fc_runtime::authority::{AuthorityStore, PreparedAuthority};
use fc_security::{
    CredentialCryptoAdapter, CredentialRecordCodec, Password, PasswordRecord, RecoveryRecord,
};
use fc_storage::{
    business::BusinessRepository, login::*, StorageError, StorageHandle, StorageWriter,
};
use fc_windows::{
    system::{CurrentUserDpapi, SecretBytes},
    SystemRandom,
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

type Snapshot = PreparedAuthority<IdentityState>;
type Repository = BusinessRepository<Snapshot>;
type Crypto = CredentialCryptoAdapter<SystemRandom>;
type Change = PreparedCredentialChange<PasswordRecord, RecoveryRecord>;

fn checked<T, E>(result: Result<T, E>, stage: &'static str) -> Result<T, String> {
    result.map_err(|_| format!("credential-probe: {stage}"))
}
fn require(condition: bool, stage: &'static str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(format!("credential-probe: {stage}"))
    }
}
struct ProbeClock {
    wall: LocalDateTime,
    started: Instant,
}
impl Clock for ProbeClock {
    fn now(&self) -> LocalDateTime {
        self.wall
    }
}
impl MonotonicClock for ProbeClock {
    fn monotonic_millis(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}
/// Unlike StorageWriter::Drop, diagnostic cleanup always joins before the outer
/// temporary-root guard removes SQLite/key files, including every `?` error path.
struct Joined(Option<StorageWriter<Snapshot>>);
impl Joined {
    fn close(&mut self) -> Result<(), String> {
        if let Some(writer) = self.0.take() {
            checked(writer.shutdown(), "writer shutdown")?;
        }
        Ok(())
    }
}
impl Drop for Joined {
    fn drop(&mut self) {
        if let Some(writer) = self.0.take() {
            let _ = writer.shutdown();
        }
    }
}

fn open(
    root: &Path,
    clock: Arc<ProbeClock>,
    state: IdentityState,
) -> Result<(Joined, Repository, StorageHandle<Snapshot>), String> {
    let revision = state.revision;
    let (mut publisher, reader) = AuthorityStore::new(state, revision, AuthEpoch::new(1));
    checked(publisher.initialize(), "authority initialization")?;
    let writer = checked(
        StorageWriter::open_with_login_clock(
            root.join("credentials.sqlite3"),
            16,
            publisher,
            clock.clone(),
        ),
        "SQLite open",
    )?;
    let handle = writer.handle();
    let factory = Arc::new(move |state: &IdentityState, revision: Revision| {
        reader
            .prepare_update(state.clone(), revision, AuthEpoch::new(1))
            .map_err(|_| StorageError::AuthorityUnavailable)
    });
    let repository = BusinessRepository::new(handle.clone(), clock, factory);
    Ok((Joined(Some(writer)), repository, handle))
}
fn request(
    sequence: u64,
    state: &IdentityState,
    command: IdentityCommand,
) -> Result<IdentityRequest, String> {
    Ok(IdentityRequest {
        command_id: checked(CommandId::new(sequence), "command ID")?,
        expected_revision: state.revision,
        command,
    })
}
fn encode(
    change: Change,
    codec: &CredentialRecordCodec,
    id: PrincipalId,
    password_version: u64,
    recovery_version: u64,
) -> Result<PreparedCredentialChange<Vec<u8>, Vec<u8>>, String> {
    checked(
        change.try_map(
            |p| {
                codec
                    .encode_password(id, password_version, &p)
                    .map(|e| e.into_storage_bytes())
            },
            |r| {
                codec
                    .encode_recovery(id, recovery_version, &r)
                    .map(|e| e.into_storage_bytes())
            },
        ),
        "record encoding",
    )
}
fn admission(
    handle: &StorageHandle<Snapshot>,
    id: PrincipalId,
    kind: CredentialKind,
) -> Result<Admission, String> {
    let response = checked(
        checked(
            handle.try_login_admission(id, kind),
            "login admission queue",
        )?
        .wait(),
        "login admission",
    )?;
    match response {
        Response::Admitted(a) => Ok(a),
        _ => Err("credential-probe: unexpected admission response".into()),
    }
}
fn finish(
    handle: &StorageHandle<Snapshot>,
    reservation: Reservation,
    verification: Verification,
) -> Result<Completion, String> {
    let response = checked(
        checked(
            handle.try_finish_login(reservation, verification),
            "login completion queue",
        )?
        .wait(),
        "login completion",
    )?;
    match response {
        Response::Completed(c) => Ok(c),
        _ => Err("credential-probe: unexpected completion response".into()),
    }
}
fn authenticated(
    completion: Completion,
    state: &IdentityState,
    id: PrincipalId,
) -> Result<(), String> {
    let a = checked(state.account(id), "authenticated principal")?;
    require(
        matches!(completion,Completion::Authenticated{principal_id,auth_epoch,credential_version,recovery_version}
        if principal_id==id && auth_epoch==a.principal.auth_epoch && credential_version==a.credential_version && recovery_version==a.recovery_version),
        "authenticated version check",
    )
}
fn password_record(
    codec: &CredentialRecordCodec,
    id: PrincipalId,
    admission: &Admission,
) -> Result<PasswordRecord, String> {
    checked(
        codec.decode_password(
            id,
            admission.credential_version,
            &admission.protected_record,
        ),
        "password record authentication",
    )
}
fn login(
    handle: &StorageHandle<Snapshot>,
    codec: &CredentialRecordCodec,
    crypto: &mut Crypto,
    id: PrincipalId,
    password: &Password,
) -> Result<Completion, String> {
    let a = admission(handle, id, CredentialKind::Password)?;
    let record = match password_record(codec, id, &a) {
        Ok(record) => record,
        Err(error) => {
            let _ = finish(handle, a.reservation, Verification::CorruptRecord);
            return Err(error);
        }
    };
    let verified = checked(
        crypto.verify_password(password, &record),
        "password verification",
    )?;
    finish(
        handle,
        a.reservation,
        if verified {
            Verification::Verified
        } else {
            Verification::Rejected
        },
    )
}
fn actor(
    sessions: &Arc<Mutex<IdentitySessions>>,
    state: &IdentityState,
    lease: SessionLease,
    clock: &ProbeClock,
) -> Result<ActorContext, String> {
    let sessions = checked(sessions.lock(), "session lock")?;
    checked(
        sessions.actor(
            state,
            lease,
            clock.now(),
            clock.monotonic_millis(),
            SessionPurpose::Manage,
        ),
        "session authorization",
    )
}
fn issue(
    sessions: &Arc<Mutex<IdentitySessions>>,
    state: &IdentityState,
    id: PrincipalId,
    clock: &ProbeClock,
) -> Result<SessionLease, String> {
    let account = checked(state.account(id), "session principal")?;
    let expires = checked(
        LocalDateTime::new(clock.now().date, 7200),
        "session expiry fixture",
    )?;
    checked(
        checked(sessions.lock(), "session lock")?.issue_verified(
            account,
            TrustedOrigin::Local,
            clock.monotonic_millis(),
            3_600_000,
            expires,
        ),
        "session issuance",
    )
}
fn guarded(
    repository: Repository,
    sessions: Arc<Mutex<IdentitySessions>>,
    lease: SessionLease,
    clock: Arc<ProbeClock>,
) -> Repository {
    repository.with_session_guard(Arc::new(move |state, expected| {
        let sessions = sessions
            .lock()
            .map_err(|_| StorageError::AuthorityUnavailable)?;
        let actual = sessions
            .actor(
                state,
                lease,
                clock.now(),
                clock.monotonic_millis(),
                SessionPurpose::Manage,
            )
            .map_err(|_| StorageError::AuthorityUnavailable)?;
        if &actual == expected {
            Ok(())
        } else {
            Err(StorageError::AuthorityUnavailable)
        }
    }))
}

pub fn run(root: &Path) -> Result<(), String> {
    let directory = root.join("credential-probe");
    // create_dir refuses reuse: this probe cannot overwrite an existing database/key.
    checked(std::fs::create_dir(&directory), "create isolated directory")?;
    // Fixed civil time is diagnostic fixture data, not a product clock. Security leases
    // and durable login admission use the independent real process monotonic clock.
    let day = checked(LocalDate::new(2026, 10, 30), "date fixture")?;
    let clock = Arc::new(ProbeClock {
        wall: checked(LocalDateTime::new(day, 3600), "clock fixture")?,
        started: Instant::now(),
    });
    let id = checked(PrincipalId::new(1), "principal fixture")?;
    let mut random = SystemRandom;
    let codec = checked(
        CredentialRecordCodec::generate(&mut random),
        "codec random key",
    )?;
    let dpapi = CurrentUserDpapi::default();
    let encrypted = codec.protect_key(|key| {
        let secret = SecretBytes::new(key.to_vec())?;
        dpapi.protect(&secret)
    });
    checked(
        std::fs::write(
            directory.join("codec-key.dpapi"),
            checked(encrypted, "DPAPI protect")?,
        ),
        "write protected key",
    )?;
    drop(codec);
    let encrypted = checked(
        std::fs::read(directory.join("codec-key.dpapi")),
        "reopen protected key",
    )?;
    let plaintext = checked(dpapi.unprotect(&encrypted), "DPAPI unprotect")?;
    let key = checked(<[u8; 32]>::try_from(plaintext.expose()), "codec key length")?;
    let codec = checked(CredentialRecordCodec::from_key(key), "restore codec key")?;
    drop(plaintext);
    println!("credential.dpapi-key-reopen: PASS");

    let mut crypto = CredentialCryptoAdapter::new(SystemRandom);
    // These three fixed diagnostic passwords never enter real application account data.
    let initial = checked(
        Password::new("Probe-initial-password-2026".into()),
        "initial password fixture",
    )?;
    let changed = checked(
        Password::new("Probe-changed-password-2026".into()),
        "changed password fixture",
    )?;
    let recovered = checked(
        Password::new("Probe-recovered-password-2026".into()),
        "recovered password fixture",
    )?;
    let (mut joined, mut repository, _) =
        open(&directory, clock.clone(), IdentityState::default())?;
    let r = request(
        1,
        &IdentityState::default(),
        IdentityCommand::FirstAdministrator {
            school_id: checked(SchoolId::new(1), "school fixture")?,
            school_name: "Diagnostic School".into(),
            id,
            login_name: "diagnostic-admin".into(),
            display_name: "Diagnostic Admin".into(),
        },
    )?;
    let (change, initial_recovery) = checked(
        prepare_initial_credentials(&mut crypto, &initial, r),
        "initial credential preparation",
    )?;
    let first = checked(
        repository.execute_credentials(encode(change, &codec, id, 1, 1)?, None, clock.as_ref()),
        "first administrator transaction",
    )?;
    joined.close()?;
    drop(repository);
    let (mut joined, repository, handle) = open(&directory, clock.clone(), first.next.clone())?;
    let state = checked(repository.load_identity(), "identity reopen")?;
    require(state == first.next, "identity persistence")?;
    authenticated(
        login(&handle, &codec, &mut crypto, id, &initial)?,
        &state,
        id,
    )?;
    println!("credential.sqlite-reopen-login-kdf: PASS");

    let sessions = Arc::new(Mutex::new(IdentitySessions::default()));
    let lease = issue(&sessions, &state, id, &clock)?;
    let original_actor = actor(&sessions, &state, lease, &clock)?;
    let mut local = guarded(repository.clone(), sessions.clone(), lease, clock.clone());
    let transition = checked(
        local.execute_identity(
            request(
                2,
                &state,
                IdentityCommand::CreateClass {
                    id: checked(ClassId::new(1), "class fixture")?,
                    name: "Diagnostic Class".into(),
                },
            )?,
            Some(&original_actor),
            clock.as_ref(),
        ),
        "authenticated catalog command",
    )?;
    let state = transition.next;

    let a = admission(&handle, id, CredentialKind::Password)?;
    let old_record = password_record(&codec, id, &a)?;
    let change = checked(
        prepare_password_change(
            &mut crypto,
            &initial,
            &changed,
            &old_record,
            request(
                3,
                &state,
                IdentityCommand::PasswordChanged {
                    id,
                    expected_credential_version: a.credential_version,
                },
            )?,
        ),
        "password change preparation",
    )?;
    authenticated(
        finish(&handle, a.reservation, Verification::Verified)?,
        &state,
        id,
    )?;
    let transition = checked(
        local.execute_credentials(
            encode(change, &codec, id, 2, 1)?,
            Some(&original_actor),
            clock.as_ref(),
        ),
        "atomic password change",
    )?;
    let state = transition.next;
    let old_result = local.execute_identity(
        request(
            4,
            &state,
            IdentityCommand::RenameSchool {
                name: "Must not commit".into(),
            },
        )?,
        Some(&original_actor),
        clock.as_ref(),
    );
    require(
        old_result.is_err(),
        "old lease rejected before event delivery",
    )?;
    checked(sessions.lock(), "session lock")?.apply_events(&transition.events);
    authenticated(
        login(&handle, &codec, &mut crypto, id, &changed)?,
        &state,
        id,
    )?;
    require(
        matches!(
            login(&handle, &codec, &mut crypto, id, &initial)?,
            Completion::Rejected { .. }
        ),
        "old password rejected",
    )?;
    let lease = issue(&sessions, &state, id, &clock)?;
    let current_actor = actor(&sessions, &state, lease, &clock)?;
    let mut local = guarded(repository.clone(), sessions.clone(), lease, clock.clone());
    println!("credential.password-rotation-old-lease: PASS");

    let a = admission(&handle, id, CredentialKind::Recovery)?;
    let old_recovery = checked(
        codec.decode_recovery(id, a.recovery_version, &a.protected_record),
        "recovery record authentication",
    )?;
    let (change, next_recovery) = checked(
        prepare_recovery(
            &mut crypto,
            &initial_recovery,
            &recovered,
            &old_recovery,
            request(
                10,
                &state,
                IdentityCommand::RecoveryConsumed {
                    id,
                    expected_recovery_version: a.recovery_version,
                },
            )?,
        ),
        "recovery preparation",
    )?;
    // Prepare an intentionally stale competing change while the SAME physical KDF
    // reservation is held. A predicted next CAS revision isolates the account-version
    // rejection from the separate global-revision rejection in this diagnostic fixture.
    let mut after_recovery = state.clone();
    after_recovery.revision = checked(state.revision.next(), "recovery CAS fixture")?;
    let (replay, _) = checked(
        prepare_recovery(
            &mut crypto,
            &initial_recovery,
            &recovered,
            &old_recovery,
            request(
                12,
                &after_recovery,
                IdentityCommand::RecoveryConsumed {
                    id,
                    expected_recovery_version: 1,
                },
            )?,
        ),
        "stale recovery worker fixture",
    )?;
    authenticated(
        finish(&handle, a.reservation, Verification::Verified)?,
        &state,
        id,
    )?;
    let transition = checked(
        local.execute_credentials(encode(change, &codec, id, 3, 2)?, None, clock.as_ref()),
        "atomic recovery rotation",
    )?;
    let state = transition.next;
    require(
        local
            .execute_identity(
                request(
                    11,
                    &state,
                    IdentityCommand::RenameSchool {
                        name: "Must not commit".into(),
                    },
                )?,
                Some(&current_actor),
                clock.as_ref(),
            )
            .is_err(),
        "pre-recovery lease rejected",
    )?;
    checked(sessions.lock(), "session lock")?.apply_events(&transition.events);
    let (version, bytes) = checked(
        repository.credential_record(id, true),
        "read rotated recovery",
    )?;
    let new_record = checked(
        codec.decode_recovery(id, version, &bytes),
        "decode rotated recovery",
    )?;
    require(
        checked(
            crypto.verify_recovery(&next_recovery, &new_record),
            "new recovery verification",
        )?,
        "new recovery accepted",
    )?;
    require(
        !checked(
            crypto.verify_recovery(&initial_recovery, &new_record),
            "old recovery verification",
        )?,
        "old recovery rejected",
    )?;

    // A worker holding a legitimately verified *old* record cannot consume that old
    // version again, even with a fresh global CAS revision and a new command ID.
    let result = local.execute_credentials(encode(replay, &codec, id, 4, 2)?, None, clock.as_ref());
    require(
        matches!(
            result,
            Err(IdentityRepositoryError::Domain(
                IdentityError::RevisionConflict
            ))
        ),
        "one-use recovery version enforced",
    )?;
    require(
        checked(repository.load_identity(), "state after stale recovery")? == state,
        "failed recovery leaves state intact",
    )?;
    authenticated(
        login(&handle, &codec, &mut crypto, id, &recovered)?,
        &state,
        id,
    )?;
    println!("credential.recovery-one-use-rotation: PASS");
    // Do not reveal fixture recovery codes. Product composition reveals `reveal()`
    // only once after the corresponding transaction has committed successfully.
    drop(next_recovery);
    drop(initial_recovery);
    joined.close()?;
    Ok(())
}
