//! Real credentials/SQLite/Web boundary with validated canonical zero-PCM bytes.
//! Driver transitions below are synthetic state-machine inputs, never native
//! output or evidence of sound. Actual Windows byte/file adapters are separate.
use super::*;
use fc_domain::{auth::*, identity::*, ids::*, voice::*};
use fc_ports::voice::*;
use std::{io::Cursor, time::Duration};
fn class(id: u64) -> ClassId {
    ClassId::new(id).unwrap()
}
fn fixture() -> BackendFixture {
    #[cfg(target_os = "linux")]
    let root = std::path::PathBuf::from("/dev/shm");
    #[cfg(not(target_os = "linux"))]
    let root = std::env::temp_dir();
    let directory = Directory(root.join(format!(
            "FutureClassroom-Web-Voice-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&directory.0).unwrap();
    let clock = Arc::new(Time(AtomicU64::new(1000), AtomicU64::new(36000)));
    let mut backend = BackendFixture::open(&directory, clock.clone());
    drop(
        backend
            .initialize(
                crate::backend::Initialize {
                    school_name: "Synthetic voice school".into(),
                    login_name: "admin".into(),
                    display_name: "Synthetic voice global".into(),
                },
                Password::new(PASSWORD.into()).unwrap(),
            )
            .unwrap(),
    );
    let admin = backend
        .login("admin", Password::new(PASSWORD.into()).unwrap())
        .unwrap();
    let service = backend.web_service();
    service.set_enabled(admin, true).unwrap();
    let app = WebApplication::new(
        &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
        service.clone(),
    )
    .unwrap();
    BackendFixture {
        backend,
        app,
        service,
        admin,
        clock,
        directory,
    }
}
fn catalog(f: &mut BackendFixture) {
    for id in [1, 2] {
        f.backend
            .identity(
                f.admin,
                IdentityCommand::CreateClass {
                    id: class(id),
                    name: format!("Synthetic voice class {id}"),
                },
            )
            .unwrap();
    }
    f.backend
        .identity(f.admin, IdentityCommand::BindCurrentClass { id: class(1) })
        .unwrap();
}
fn auth(f: &mut BackendFixture, login: &str, device: &str) -> (String, String, SessionLease) {
    let cookie = f.pair_login(login, device, false);
    let result = f.dispatch(&head("GET", "session", 0, Some(&cookie), None), b"");
    assert_eq!(result.head.status, 200);
    let csrf = field(&result.body, "csrf");
    let secret = cookie_secret(cookie.as_bytes()).unwrap();
    let lease = f.service.authenticate(&secret).unwrap().lease();
    (cookie, csrf, lease)
}
fn get(f: &mut BackendFixture, cookie: &str, path: &str) -> WebResponse {
    f.dispatch(&head("GET", path, 0, Some(cookie), None), b"")
}
fn post(f: &mut BackendFixture, cookie: &str, csrf: &str, path: &str, body: &str) -> WebResponse {
    f.dispatch(
        &head("POST", path, body.len(), Some(cookie), Some(csrf)),
        body.as_bytes(),
    )
}
fn expect(r: &WebResponse, status: u16, needle: &str) {
    assert_eq!(r.head.status, status, "{}", text(&r.body));
    assert!(text(&r.body).contains(needle), "{}", text(&r.body));
}
fn wav() -> Vec<u8> {
    let data = 16000u32;
    let mut bytes = vec![0u8; 44 + data as usize];
    bytes[..4].copy_from_slice(b"RIFF");
    bytes[4..8].copy_from_slice(&(data + 36).to_le_bytes());
    bytes[8..16].copy_from_slice(b"WAVEfmt ");
    bytes[16..20].copy_from_slice(&16u32.to_le_bytes());
    bytes[20..22].copy_from_slice(&1u16.to_le_bytes());
    bytes[22..24].copy_from_slice(&1u16.to_le_bytes());
    bytes[24..28].copy_from_slice(&16000u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&32000u32.to_le_bytes());
    bytes[32..34].copy_from_slice(&2u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&16u16.to_le_bytes());
    bytes[36..40].copy_from_slice(b"data");
    bytes[40..44].copy_from_slice(&data.to_le_bytes());
    bytes
}
struct Control;
impl fc_materials::office::ValidationControl for Control {
    fn canceled(&self) -> bool {
        false
    }
    fn thread_cpu_time(&self) -> Result<Duration, fc_materials::office::ContainerError> {
        Ok(Duration::ZERO)
    }
}
struct ActualPcm;
impl VoicePreparation for ActualPcm {
    type Input = Vec<u8>;
    fn prepare(
        &mut self,
        bytes: Vec<u8>,
        _: &VoiceAsset,
    ) -> Result<VoicePcmMetadata, VoiceRepositoryError> {
        let info = fc_materials::validate_wav_header(
            bytes
                .get(..44)
                .ok_or(VoiceRepositoryError::Domain(VoiceError::InvalidWav))?
                .try_into()
                .map_err(|_| VoiceRepositoryError::Domain(VoiceError::InvalidWav))?,
            bytes.len() as u64,
        )
        .map_err(|_| VoiceRepositoryError::Domain(VoiceError::InvalidWav))?;
        let (byte_length, sha256) =
            fc_materials::validation::hash_file(&mut Cursor::new(bytes), &Control)
                .map_err(|_| VoiceRepositoryError::Domain(VoiceError::InvalidWav))?;
        Ok(VoicePcmMetadata {
            byte_length,
            frame_count: info.frames,
            sha256,
        })
    }
}
fn ready(f: &BackendFixture, lease: SessionLease) -> VoiceAsset {
    let service = f.backend.voice_service();
    let asset = service.reserve(lease, class(1)).unwrap();
    let proof = prepare_voice_upload(&mut ActualPcm, wav(), asset).unwrap();
    service.publish(lease, proof).unwrap()
}
fn submit_body(id: u64, asset: &VoiceAsset) -> String {
    format!(
        r#"{{"schemaVersion":1,"commandId":"{id}","assetId":"{}","expectedAssetRevision":"{}","classId":"1","repeatTotal":3}}"#,
        asset.id.get(),
        asset.revision.get()
    )
}
#[test]
fn voice_http_submit_is_durable_idempotent_queryable_and_never_claims_device_or_sound() {
    let mut f = fixture();
    catalog(&mut f);
    let (cookie, csrf, lease) = auth(&mut f, "admin", "voice-http");
    let cap = get(&mut f, &cookie, "voice/capabilities");
    expect(&cap, 200, "\"deviceAvailability\":\"UNVERIFIED\"");
    expect(
        &cap,
        200,
        "\"currentClass\":{\"classId\":\"1\",\"className\":\"Synthetic voice class 1\"}",
    );
    expect(&cap, 200, "\"targetAuthorized\":true");
    expect(&cap, 200, "\"defaultRepeatTotal\":3");
    assert!(f
        .backend
        .voice_service()
        .check_web_target(lease, class(2), f.backend.voice_service().generation())
        .is_err());
    let asset = ready(&f, lease);
    let body = submit_body(81001, &asset);
    expect(
        &post(
            &mut f,
            &cookie,
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "voice/broadcasts",
            &body,
        ),
        403,
        "FORBIDDEN",
    );
    assert!(f.backend.voice_inventory().unwrap().jobs.is_empty());
    let accepted = post(&mut f, &cookie, &csrf, "voice/broadcasts", &body);
    expect(&accepted, 200, "\"status\":\"COMMITTED\"");
    expect(&accepted, 200, "\"jobId\":\"81001\"");
    expect(&accepted, 200, "\"state\":\"QUEUED\"");
    for hidden in [
        "authorization",
        "expires_at",
        "owner",
        "assetId",
        "generation",
    ] {
        assert!(!text(&accepted.body).contains(hidden), "{hidden}");
    }
    expect(
        &post(&mut f, &cookie, &csrf, "voice/broadcasts", &body),
        200,
        "QUEUED",
    );
    assert_eq!(f.backend.voice_inventory().unwrap().jobs.len(), 1);
    expect(
        &post(
            &mut f,
            &cookie,
            &csrf,
            "voice/broadcasts",
            &body.replace("\"repeatTotal\":3", "\"repeatTotal\":4"),
        ),
        409,
        "IDEMPOTENCY_CONFLICT",
    );
    expect(
        &get(&mut f, &cookie, "voice/broadcasts/81001"),
        200,
        "\"status\":\"VOICE_JOB\"",
    );
    expect(
        &get(&mut f, &cookie, "voice/broadcasts/81999"),
        404,
        "NOT_FOUND",
    );
    expect(
        &get(&mut f, &cookie, "voice/broadcasts"),
        200,
        "\"status\":\"VOICE_JOBS\"",
    );
    let cancel = r#"{"schemaVersion":1,"commandId":"81002","expectedRevision":"1"}"#;
    expect(
        &post(
            &mut f,
            &cookie,
            &csrf,
            "voice/broadcasts/81001/cancel",
            cancel,
        ),
        200,
        "\"status\":\"CANCEL_REQUESTED\"",
    );
    expect(
        &get(&mut f, &cookie, "voice/broadcasts/81001"),
        200,
        "\"state\":\"CANCELED\"",
    );
    let mut binary = head(
        "POST",
        "voice/uploads?classId=1",
        wav().len(),
        Some(&cookie),
        Some(&csrf),
    );
    binary
        .headers
        .iter_mut()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .unwrap()
        .value = b"audio/wav".to_vec();
    expect(&f.dispatch(&binary, &wav()), 400, "INVALID_REQUEST");
}
fn teacher(f: &mut BackendFixture) {
    f.backend
        .identity(
            f.admin,
            IdentityCommand::PutSubject {
                id: SubjectId::new(1).unwrap(),
                name: "Synthetic voice subject".into(),
                enabled: true,
                color: Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                expected_revision: None,
            },
        )
        .unwrap();
    f.backend
        .create_account(
            f.admin,
            NewAccount {
                id: PrincipalId::new(2).unwrap(),
                login_name: "voice-teacher".into(),
                display_name: "Synthetic voice teacher".into(),
                role: Role::TeacherAdmin,
                scopes: vec![AssignmentScope {
                    class_id: class(1),
                    subject_id: SubjectId::new(1).unwrap(),
                }],
            },
            Password::new("Synthetic-Voice-Teacher-Initial-2026".into()).unwrap(),
        )
        .unwrap();
    let initial = f
        .backend
        .login(
            "voice-teacher",
            Password::new("Synthetic-Voice-Teacher-Initial-2026".into()).unwrap(),
        )
        .unwrap();
    f.backend
        .change_password(
            initial,
            Password::new("Synthetic-Voice-Teacher-Initial-2026".into()).unwrap(),
            Password::new("Synthetic-Voice-Teacher-Changed-2026".into()).unwrap(),
        )
        .unwrap();
    f.admin = f
        .backend
        .login("admin", Password::new(PASSWORD.into()).unwrap())
        .unwrap();
}
#[test]
fn voice_http_teacher_current_target_and_old_job_stop_survive_binding_disable_and_closed_admission()
{
    let mut f = fixture();
    catalog(&mut f);
    teacher(&mut f);
    let (cookie, csrf, lease) = auth(&mut f, "voice-teacher", "teacher-voice");
    let (global, _, global_lease) = auth(&mut f, "admin", "global-voice");
    let asset = ready(&f, lease);
    let body = submit_body(82001, &asset);
    expect(
        &post(&mut f, &cookie, &csrf, "voice/broadcasts", &body),
        200,
        "COMMITTED",
    );
    let service = f.backend.voice_service();
    let queued = service
        .status(lease, VoiceJobId::new(82001).unwrap())
        .unwrap();
    let started = service
        .drive(VoiceDriverUpdate {
            job_id: queued.id,
            expected_revision: queued.revision,
            generation: queued.generation,
            action: VoiceDriverAction::Start,
        })
        .unwrap();
    assert_eq!(started.state, VoiceState::Starting);
    f.backend
        .identity(f.admin, IdentityCommand::BindCurrentClass { id: class(2) })
        .unwrap();
    let cap = get(&mut f, &cookie, "voice/capabilities");
    expect(&cap, 200, "\"targetAuthorized\":false");
    expect(&cap, 200, "Synthetic voice class 2");
    service.invalidate_runtime();
    expect(
        &get(&mut f, &cookie, "voice/broadcasts/82001"),
        200,
        "STARTING",
    );
    expect(&get(&mut f, &cookie, "voice/broadcasts"), 200, "82001");
    let cancel = format!(
        r#"{{"schemaVersion":1,"commandId":"82002","expectedRevision":"{}"}}"#,
        started.revision.get()
    );
    let result = post(
        &mut f,
        &cookie,
        &csrf,
        "voice/broadcasts/82001/cancel",
        &cancel,
    );
    expect(&result, 200, "CANCEL_REQUESTED");
    expect(&result, 200, "STOPPING");
    assert!(!text(&result.body).contains("已停止"));
    let stopping = service.status(lease, queued.id).unwrap();
    let ended = service
        .drive(VoiceDriverUpdate {
            job_id: stopping.id,
            expected_revision: stopping.revision,
            generation: stopping.release_generation.unwrap(),
            action: VoiceDriverAction::Released,
        })
        .unwrap();
    assert_eq!(ended.state, VoiceState::Canceled);
    service.resume_after_stop(service.generation()).unwrap();
    assert!(service
        .check_web_target(lease, class(2), service.generation())
        .is_err());
    assert!(service
        .check_web_target(global_lease, class(1), service.generation())
        .is_err());
    assert!(service
        .check_web_target(global_lease, class(2), service.generation())
        .is_ok());
    let revision = service.inventory().unwrap().revision;
    service.set_enabled(f.admin, false, revision).unwrap();
    expect(
        &get(&mut f, &global, "voice/capabilities"),
        200,
        "\"enabled\":false",
    );
    expect(
        &get(&mut f, &cookie, "voice/broadcasts/82001"),
        200,
        "CANCELED",
    );
}

#[test]
fn voice_http_post_commit_stop_reports_unknown_and_original_command_is_queryable_without_replay() {
    let mut f = fixture();
    catalog(&mut f);
    let (cookie, csrf, lease) = auth(&mut f, "admin", "voice-unknown");
    let service = f.backend.voice_service();
    let stop = service.clone();
    service.after_next_commit(Arc::new(move || {
        stop.invalidate_runtime();
    }));
    assert!(matches!(
        service.reserve(lease, class(1)),
        Err(crate::backend::BackendError::Voice(
            VoiceRepositoryError::Unavailable
        ))
    ));
    assert_eq!(
        service.inventory().unwrap().assets.len(),
        1,
        "reserve actually COMMITTED before the deterministic stop"
    );
    service.resume_after_stop(service.generation()).unwrap();
    let asset = ready(&f, lease);
    let body = submit_body(83001, &asset);
    let stop = service.clone();
    service.after_next_commit(Arc::new(move || {
        stop.invalidate_runtime();
    }));
    expect(
        &post(&mut f, &cookie, &csrf, "voice/broadcasts", &body),
        503,
        "RESULT_UNKNOWN",
    );
    let inventory = service.inventory().unwrap();
    assert_eq!(inventory.jobs.len(), 1);
    assert_eq!(inventory.jobs[0].id.get(), 83001);
    expect(
        &get(&mut f, &cookie, "voice/broadcasts/83001"),
        200,
        "\"state\":\"QUEUED\"",
    );
    expect(&get(&mut f, &cookie, "voice/broadcasts"), 200, "83001");
    assert_eq!(service.inventory().unwrap().jobs.len(), 1);
    assert!(service.guard(VoiceJobId::new(83001).unwrap()).is_err(),"a missing post-COMMIT native capability cannot be fabricated by querying its durable result");
}
#[test]
fn voice_http_readonly_user_pairing_never_discloses_remote_voice_capabilities_or_jobs() {
    let mut f = fixture();
    catalog(&mut f);
    f.backend
        .create_account(
            f.admin,
            NewAccount {
                id: PrincipalId::new(3).unwrap(),
                login_name: "voice-user".into(),
                display_name: "Synthetic readonly user".into(),
                role: Role::User,
                scopes: vec![],
            },
            Password::new("Synthetic-Voice-User-Initial-2026".into()).unwrap(),
        )
        .unwrap();
    let initial = f
        .backend
        .login(
            "voice-user",
            Password::new("Synthetic-Voice-User-Initial-2026".into()).unwrap(),
        )
        .unwrap();
    f.backend
        .change_password(
            initial,
            Password::new("Synthetic-Voice-User-Initial-2026".into()).unwrap(),
            Password::new("Synthetic-Voice-User-Changed-2026".into()).unwrap(),
        )
        .unwrap();
    f.admin = f
        .backend
        .login("admin", Password::new(PASSWORD.into()).unwrap())
        .unwrap();
    let (cookie, _, _) = auth(&mut f, "voice-user", "readonly-user-device");
    let cap = get(&mut f, &cookie, "voice/capabilities");
    expect(&cap, 403, "FORBIDDEN");
    assert!(!text(&cap.body).contains("Synthetic voice class"));
    expect(&get(&mut f, &cookie, "voice/broadcasts"), 403, "FORBIDDEN");
}

#[cfg(windows)]
fn binary_head(
    cookie: &str,
    csrf: &str,
    path: &str,
    bytes: usize,
    content_type: &str,
) -> RequestHead {
    let mut h = head("POST", path, bytes, Some(cookie), Some(csrf));
    h.headers
        .iter_mut()
        .find(|h| h.name == "Content-Type")
        .unwrap()
        .value = content_type.as_bytes().to_vec();
    h
}
#[cfg(windows)]
fn replace_file_application(f: &mut BackendFixture) {
    let replacement = WebApplication::new(
        &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
        f.service.clone(),
    )
    .unwrap();
    drop(std::mem::replace(&mut f.app, replacement));
    f.app
        .configure_voice_files(&f.backend, &f.directory.0)
        .unwrap();
    f.app.configure_material_files(&f.directory.0).unwrap();
}
#[cfg(windows)]
fn expire_sql_reservations(f: &mut BackendFixture) {
    f.clock.0.fetch_add(601_000, Ordering::SeqCst);
    f.clock.1.fetch_add(601, Ordering::SeqCst);
    let snapshot = f.backend.voice_service().maintain().unwrap();
    assert!(
        snapshot
            .assets
            .iter()
            .all(|a| a.state != VoiceAssetState::Reserved),
        "SQL reservation capacity must not mask the independent physical permit"
    );
}
#[cfg(windows)]
struct NeverRead;
#[cfg(windows)]
impl std::io::Read for NeverRead {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        panic!("a saturated physical budget must reject before reading any body")
    }
}
#[cfg(windows)]
#[test]
fn voice_windows_physical_proofs_survive_sql_expiry_and_application_replacement() {
    let _scenario = super::super::shared_upload::test_guard();
    let mut f = fixture();
    catalog(&mut f);
    let (cookie, csrf, _) = auth(&mut f, "admin", "voice-physical-owner");
    f.app
        .configure_voice_files(&f.backend, &f.directory.0)
        .unwrap();
    let bytes = wav();
    let h = binary_head(
        &cookie,
        &csrf,
        "voice/uploads?classId=1",
        bytes.len(),
        "audio/wav",
    );
    let mut proofs = Vec::new();
    for _ in 0..2 {
        proofs.push(match f.app.prepare_voice_upload(&h, Cursor::new(&bytes)) {
            Ok(p) => p,
            Err(r) => panic!("{}", text(&r.body)),
        });
    }
    expire_sql_reservations(&mut f);
    replace_file_application(&mut f);
    let denied = match f.app.prepare_voice_upload(&h, NeverRead) {
        Err(r) => r,
        Ok(_) => panic!("old proofs cannot be forgotten by a new application"),
    };
    expect(&denied, 429, "CAPACITY");
    drop(proofs.pop());
    let replacement = match f.app.prepare_voice_upload(&h, Cursor::new(&bytes)) {
        Ok(p) => p,
        Err(r) => panic!("{}", text(&r.body)),
    };
    drop(replacement);
    drop(proofs);
    let final_proof = match f.app.prepare_voice_upload(&h, Cursor::new(&bytes)) {
        Ok(p) => p,
        Err(r) => panic!("{}", text(&r.body)),
    };
    expect(
        &f.app.commit_voice_upload(final_proof),
        201,
        "VOICE_UPLOADED",
    );
    assert_eq!(
        f.backend
            .voice_service()
            .inventory()
            .unwrap()
            .assets
            .iter()
            .filter(|a| a.state == VoiceAssetState::Ready)
            .count(),
        1
    );
}
#[cfg(windows)]
#[test]
fn voice_windows_material_and_voice_share_two_physical_owners_across_application_replacement() {
    let _scenario = super::super::shared_upload::test_guard();
    let mut f = fixture();
    catalog(&mut f);
    f.backend
        .identity(
            f.admin,
            IdentityCommand::PutSubject {
                id: SubjectId::new(1).unwrap(),
                name: "Synthetic shared subject".into(),
                enabled: true,
                color: Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                expected_revision: None,
            },
        )
        .unwrap();
    f.backend
        .identity(
            f.admin,
            IdentityCommand::UpdateAccount {
                id: PrincipalId::new(1).unwrap(),
                display_name: "Synthetic shared global".into(),
                role: Role::GlobalAdmin,
                enabled: true,
                scopes: vec![AssignmentScope {
                    class_id: class(1),
                    subject_id: SubjectId::new(1).unwrap(),
                }],
            },
        )
        .unwrap();
    f.admin = f
        .backend
        .login("admin", Password::new(PASSWORD.into()).unwrap())
        .unwrap();
    let (cookie, csrf, _) = auth(&mut f, "admin", "shared-upload-owner");
    f.app
        .configure_voice_files(&f.backend, &f.directory.0)
        .unwrap();
    f.app.configure_material_files(&f.directory.0).unwrap();
    let material = b"synthetic text\n";
    let reservation = r#"{"schemaVersion":1,"displayName":"shared.txt","bytes":15}"#;
    let ready = post(&mut f, &cookie, &csrf, "materials/uploads/1/1", reservation);
    expect(&ready, 201, "UPLOAD_READY");
    let id = field(&ready.body, "uploadId");
    expect(
        &f.app.dispatch_material_upload(
            &binary_head(
                &cookie,
                &csrf,
                &format!("materials/uploads/{id}/bytes"),
                material.len(),
                "application/octet-stream",
            ),
            Cursor::new(material),
        ),
        200,
        "UPLOADED",
    );
    let append = format!(
        r#"{{"schemaVersion":1,"commandId":"84001","uploadId":"{id}","allowedClassIds":["1"]}}"#
    );
    let material_proof = match f.app.prepare_material_append(
        &head(
            "POST",
            "materials/folders/1/1/append",
            append.len(),
            Some(&cookie),
            Some(&csrf),
        ),
        append.as_bytes(),
    ) {
        Ok(p) => p,
        Err(r) => panic!("{}", text(&r.body)),
    };
    let bytes = wav();
    let h = binary_head(
        &cookie,
        &csrf,
        "voice/uploads?classId=1",
        bytes.len(),
        "audio/wav",
    );
    let voice_proof = match f.app.prepare_voice_upload(&h, Cursor::new(&bytes)) {
        Ok(p) => p,
        Err(r) => panic!("{}", text(&r.body)),
    };
    // One SQL reservation cannot itself cause an owner-limit rejection.
    assert_eq!(
        f.backend.voice_service().inventory().unwrap().assets.len(),
        1
    );
    let denied = match f.app.prepare_voice_upload(&h, NeverRead) {
        Err(r) => r,
        Ok(_) => panic!("material + voice already physically own both slots"),
    };
    expect(&denied, 429, "CAPACITY");
    expire_sql_reservations(&mut f);
    replace_file_application(&mut f);
    expect(
        &post(&mut f, &cookie, &csrf, "materials/uploads/1/1", reservation),
        429,
        "CAPACITY",
    );
    let denied = match f.app.prepare_voice_upload(&h, NeverRead) {
        Err(r) => r,
        Ok(_) => panic!("replacement application bypassed shared physical ownership"),
    };
    expect(&denied, 429, "CAPACITY");
    drop(material_proof);
    let replacement = match f.app.prepare_voice_upload(&h, Cursor::new(&bytes)) {
        Ok(p) => p,
        Err(r) => panic!("{}", text(&r.body)),
    };
    drop(replacement);
    drop(voice_proof);
}
