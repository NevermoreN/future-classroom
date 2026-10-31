//! Real SQLite, credentials, canonical zero PCM and Windows controlled files.
//! No socket/native audio is constructed by these worker-side regressions.
use super::*;
use crate::web_transfer::TransferControl;
use fc_domain::{identity::IdentityCommand, ids::ClassId, time::*};
use fc_ports::{http::TokenIssuer, Clock, MonotonicClock};
use fc_runtime::worker::{Task, WorkerPool};
use fc_security::{CredentialRecordCodec, Password};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Time;
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 36000).unwrap()
    }
}
impl MonotonicClock for Time {
    fn monotonic_millis(&self) -> u64 {
        1000
    }
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Fixture {
    backend: Backend,
    app: Arc<WebApplication>,
    head: RequestHead,
    admin: SessionLease,
    directory: Directory,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.backend.close().unwrap();
    }
}
fn positive<T>(result: Result<T, WebResponse>) -> T {
    result.unwrap_or_else(|response| {
        panic!(
            "status {}: {}",
            response.head.status,
            String::from_utf8_lossy(&response.body)
        )
    })
}
fn wav() -> Vec<u8> {
    let data = 16000u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((data + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(32000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.resize(44 + data as usize, 0);
    bytes
}
fn fixture() -> Fixture {
    let directory = Directory(std::env::temp_dir().join(format!(
            "FutureClassroom-Voice-Stream-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&directory.0).unwrap();
    let clock = Arc::new(Time);
    let mut backend = Backend::open(
        directory.0.join("state.sqlite3"),
        CredentialRecordCodec::from_key([37; 32]).unwrap(),
        clock.clone(),
        clock,
    )
    .unwrap();
    const PASSWORD: &str = "Synthetic-Voice-Stream-Password-2026";
    drop(
        backend
            .initialize(
                crate::backend::Initialize {
                    school_name: "Synthetic stream".into(),
                    login_name: "admin".into(),
                    display_name: "Synthetic admin".into(),
                },
                Password::new(PASSWORD.into()).unwrap(),
            )
            .unwrap(),
    );
    let admin = backend
        .login("admin", Password::new(PASSWORD.into()).unwrap())
        .unwrap();
    let class = ClassId::new(1).unwrap();
    backend
        .identity(
            admin,
            IdentityCommand::CreateClass {
                id: class,
                name: "Synthetic class".into(),
            },
        )
        .unwrap();
    backend
        .identity(admin, IdentityCommand::BindCurrentClass { id: class })
        .unwrap();
    let service = backend.web_service();
    service.set_enabled(admin, true).unwrap();
    let window = service.begin_pairing(admin).unwrap();
    let receipt = service
        .request_pairing(
            PairingCode::parse(window.code.expose_text().as_str().to_owned()).unwrap(),
            "admin",
            "stream-device",
        )
        .unwrap();
    let review = service
        .pending(admin)
        .unwrap()
        .into_iter()
        .find(|r| r.request_id == receipt.request_id)
        .unwrap();
    service
        .approve(admin, receipt.request_id, review.identity_revision)
        .unwrap();
    let PairingPoll::Granted(session) = service
        .poll(receipt.request_id, &receipt.poll_secret)
        .unwrap()
    else {
        panic!("pairing not granted")
    };
    let cookie = session_cookie(&session.cookie);
    let mut app = WebApplication::new(
        &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
        service,
    )
    .unwrap();
    app.configure_voice_files(&backend, &directory.0).unwrap();
    let bytes = wav();
    let header = |name: &str, value: &[u8]| Header {
        name: name.into(),
        value: value.to_vec(),
    };
    let head = RequestHead {
        token: TokenIssuer::new().unwrap().issue(0).unwrap(),
        method: "POST".into(),
        target: "/fc/api/v1/voice/uploads?classId=1".into(),
        tls: true,
        content_length: Some(bytes.len() as u64),
        has_body: true,
        headers: vec![
            header("Host", b"board.school"),
            header("Origin", b"https://board.school"),
            header("Content-Type", b"audio/wav"),
            header("Content-Length", bytes.len().to_string().as_bytes()),
            header(
                "Cookie",
                cookie.as_str().split(';').next().unwrap().as_bytes(),
            ),
            header("X-FC-CSRF", session.csrf.as_str().as_bytes()),
        ],
    };
    Fixture {
        backend,
        app: Arc::new(app),
        head,
        admin,
        directory,
    }
}
fn begin(f: &Fixture) -> (TransferControl, ReceivingWebVoiceUpload) {
    let (control, life) = TransferControl::new(f.head.token, Instant::now()).unwrap();
    (control, positive(f.app.begin_voice_upload(&f.head, life)))
}
fn fill(owner: &mut ReceivingWebVoiceUpload) {
    let bytes = wav();
    for chunk in bytes.chunks(1379) {
        let sequence = owner.sequence + 1;
        let offset = owner.offset;
        let ack = positive(owner.write_with_fence(
            owner.token(),
            sequence,
            offset,
            chunk,
            owner.life.execution_fence(),
        ));
        assert_eq!(ack.token(), owner.token());
        assert_eq!(ack.kind(), UploadKind::Voice);
        assert_eq!(ack.upload_id(), owner.asset_id());
        assert_eq!(ack.sequence(), sequence);
        assert_eq!(ack.next_offset(), offset + chunk.len() as u64);
    }
}

struct Step {
    action: Box<dyn FnOnce(&JobContext) + Send>,
    heap: usize,
}
impl Step {
    fn new<F: FnOnce(&JobContext) + Send + 'static>(action: F) -> Self {
        Self {
            action: Box::new(action),
            heap: std::mem::size_of::<F>(),
        }
    }
}
impl Task for Step {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(self.heap)
    }
    fn run(self, context: &JobContext) {
        (self.action)(context)
    }
}
fn run(pool: &WorkerPool<Step>, step: Step) {
    let ticket = match pool
        .handle()
        .try_submit(step, Instant::now() + Duration::from_secs(10))
    {
        Ok(ticket) => ticket,
        Err(_) => panic!("shared worker rejected test"),
    };
    ticket.wait().unwrap();
}

#[test]
fn voice_stream_begin_ticket_drop_does_not_cancel_the_same_stage_next_worker_calls() {
    let _scenario = super::super::shared_upload::test_guard();
    let f = fixture();
    let pool = WorkerPool::<Step>::start().unwrap();
    let slot = Arc::new(std::sync::Mutex::new(None));
    let proof = Arc::new(std::sync::Mutex::new(None));
    let (control, life) = TransferControl::new(f.head.token, Instant::now()).unwrap();
    let app = f.app.clone();
    let head = f.head.clone();
    let receiving = slot.clone();
    run(
        &pool,
        Step::new(move |ctx| {
            *receiving.lock().unwrap() =
                Some(positive(app.begin_voice_upload_in_job(&head, life, ctx)))
        }),
    );
    let receiving = slot.clone();
    run(
        &pool,
        Step::new(move |ctx| {
            let mut guard = receiving.lock().unwrap();
            let owner = guard.as_mut().unwrap();
            let bytes = wav();
            let token = owner.token();
            let ack = positive(owner.write_chunk(token, 1, 0, &bytes, ctx));
            assert_eq!(ack.next_offset(), bytes.len() as u64);
        }),
    );
    let receiving = slot.clone();
    let prepared = proof.clone();
    run(
        &pool,
        Step::new(move |ctx| {
            let owner = receiving.lock().unwrap().take().unwrap();
            let deadline = owner.deadline();
            let result = positive(owner.finish(ctx));
            assert_eq!(result.life.deadline(), deadline);
            *prepared.lock().unwrap() = Some(result);
        }),
    );
    let app = f.app.clone();
    let prepared = proof.clone();
    run(
        &pool,
        Step::new(move |ctx| {
            assert_eq!(
                app.commit_voice_upload_in_job(prepared.lock().unwrap().take().unwrap(), ctx)
                    .head
                    .status,
                201
            )
        }),
    );
    pool.request_stop();
    pool.join().unwrap();
    let inventory = f.backend.voice_service().inventory().unwrap();
    assert_eq!(inventory.assets.len(), 1);
    assert_eq!(inventory.assets[0].state, VoiceAssetState::Ready);
    drop(control);
}

#[test]
fn voice_stream_header_permissions_and_token_are_checked_before_reservation() {
    let _scenario = super::super::shared_upload::test_guard();
    let f = fixture();
    for name in ["Origin", "X-FC-CSRF", "Cookie"] {
        let mut head = f.head.clone();
        head.headers
            .iter_mut()
            .find(|h| h.name == name)
            .unwrap()
            .value = b"invalid".to_vec();
        let (_control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
        assert!(f.app.begin_voice_upload(&head, life).is_err());
    }
    let (_control, life) = TransferControl::new(
        TokenIssuer::new().unwrap().issue(0).unwrap(),
        Instant::now(),
    )
    .unwrap();
    assert!(f.app.begin_voice_upload(&f.head, life).is_err());
    assert!(f
        .backend
        .voice_service()
        .inventory()
        .unwrap()
        .assets
        .is_empty());
    let one = PhysicalUploadPermit::take().unwrap();
    let two = PhysicalUploadPermit::take().unwrap();
    assert!(PhysicalUploadPermit::take().is_none());
    drop(two);
    drop(one);
}

#[test]
fn voice_stream_bad_token_sequence_offset_or_chunk_cannot_be_replayed() {
    let _scenario = super::super::shared_upload::test_guard();
    // A fresh real database per case prevents failed reservations masking the
    // safety assertion with SQL capacity/rate limits.
    for kind in 0..6 {
        let f = fixture();
        let (_control, mut owner) = begin(&f);
        let bytes = match kind {
            4 => vec![0; CHUNK_BYTES + 1],
            5 => vec![0; wav().len() + 1],
            _ => vec![1; 4],
        };
        let token = if kind == 0 {
            TokenIssuer::new().unwrap().issue(0).unwrap()
        } else {
            owner.token()
        };
        let sequence = if kind == 1 { 2 } else { 1 };
        let offset = if kind == 2 { 1 } else { 0 };
        let data = if kind == 3 { &bytes[..0] } else { &bytes[..] };
        assert!(owner
            .write_with_fence(token, sequence, offset, data, owner.life.execution_fence())
            .is_err());
        assert_eq!(owner.offset(), 0);
        assert!(owner.failed);
        assert!(owner
            .write_with_fence(owner.token(), 1, 0, b"a", owner.life.execution_fence())
            .is_err());
        drop(owner);
        assert!(std::fs::read_dir(f.directory.0.join("voice/staging"))
            .unwrap()
            .next()
            .is_none());
    }
}

#[test]
fn voice_stream_expired_admission_has_no_reservation_and_midstream_deadline_cannot_reset() {
    let _scenario = super::super::shared_upload::test_guard();
    let f = fixture();
    let (_control, life) =
        TransferControl::new(f.head.token, Instant::now() - Duration::from_secs(31)).unwrap();
    assert!(f.app.begin_voice_upload(&f.head, life).is_err());
    assert!(f
        .backend
        .voice_service()
        .inventory()
        .unwrap()
        .assets
        .is_empty());
    let (_control, mut owner) = begin(&f);
    fill(&mut owner);
    owner.life = owner.life.bounded(Duration::from_nanos(1)).unwrap();
    let fence = owner.life.execution_fence();
    assert!(owner.finish_with_fence(fence).is_err());
    assert!(std::fs::read_dir(f.directory.0.join("voice/active"))
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn voice_stream_stop_resume_never_adopts_a_new_epoch_and_revoked_device_cannot_finish() {
    let _scenario = super::super::shared_upload::test_guard();
    for revoke in [false, true] {
        let f = fixture();
        let (_control, mut owner) = begin(&f);
        let service = f.backend.voice_service();
        if revoke {
            f.backend.web_service().set_enabled(f.admin, false).unwrap();
        } else {
            let epoch = service.invalidate_runtime();
            service.resume_after_stop(epoch).unwrap();
            assert!(service.is_admitting());
        }
        assert!(owner
            .write_with_fence(owner.token(), 1, 0, b"abc", owner.life.execution_fence())
            .is_err());
        let fence = owner.life.execution_fence();
        assert!(owner.finish_with_fence(fence).is_err());
        assert!(service
            .inventory()
            .unwrap()
            .assets
            .iter()
            .all(|a| a.state != VoiceAssetState::Ready));
    }
}

#[test]
fn voice_stream_cancelled_prepared_proof_keeps_its_permit_until_real_pin_cleanup() {
    let _scenario = super::super::shared_upload::test_guard();
    let f = fixture();
    let (control, mut owner) = begin(&f);
    fill(&mut owner);
    let fence = owner.life.execution_fence();
    let prepared = positive(owner.finish_with_fence(fence));
    let one = PhysicalUploadPermit::take().expect("proof uses exactly one physical permit");
    assert!(PhysicalUploadPermit::take().is_none());
    control.cancel();
    assert_eq!(f.app.commit_voice_upload(prepared).head.status, 503);
    let reclaimed =
        PhysicalUploadPermit::take().expect("pin cleanup must finish before permit returns");
    assert!(std::fs::read_dir(f.directory.0.join("voice/active"))
        .unwrap()
        .next()
        .is_none());
    drop(reclaimed);
    drop(one);
}

#[test]
fn voice_stream_after_commit_cancel_reports_unknown_and_keeps_the_real_durable_object() {
    let _scenario = super::super::shared_upload::test_guard();
    let f = fixture();
    let (control, mut owner) = begin(&f);
    fill(&mut owner);
    let fence = owner.life.execution_fence();
    let prepared = positive(owner.finish_with_fence(fence));
    f.backend
        .voice_service()
        .after_next_commit(Arc::new(move || control.cancel()));
    let result = f.app.commit_voice_upload(prepared);
    assert_eq!(result.head.status, 503);
    assert!(String::from_utf8_lossy(&result.body).contains("RESULT_UNKNOWN"));
    assert_eq!(
        f.backend.voice_service().inventory().unwrap().assets[0].state,
        VoiceAssetState::Ready
    );
    assert_eq!(
        std::fs::read_dir(f.directory.0.join("voice/active"))
            .unwrap()
            .count(),
        1
    );
}

struct MethodNeverRead;
impl std::io::Read for MethodNeverRead {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        panic!("wrong HTTP method must reject before any voice body read")
    }
}
fn method_rejection<T>(result: Result<T, WebResponse>) -> WebResponse {
    match result {
        Err(response) => response,
        Ok(_) => panic!("wrong method cannot admit a voice stage or prepared proof"),
    }
}
fn expect_method_allow(response: WebResponse, allow: &str) {
    assert_eq!(
        response.head.status,
        405,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    let headers: Vec<_> = response
        .head
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("allow"))
        .collect();
    assert_eq!(
        headers.len(),
        1,
        "405 requires exactly one recognized-target Allow"
    );
    assert_eq!(headers[0].value.as_slice(), allow.as_bytes());
    assert!(String::from_utf8_lossy(&response.body).contains("METHOD_NOT_ALLOWED"));
}
fn assert_no_voice_admission(f: &Fixture, revision: fc_domain::ids::Revision) {
    assert_eq!(
        f.backend.global_revision().unwrap(),
        revision,
        "wrong method cannot write a SQL reservation"
    );
    assert!(f
        .backend
        .voice_service()
        .inventory()
        .unwrap()
        .assets
        .is_empty());
    for directory in ["staging", "active"] {
        assert!(
            std::fs::read_dir(f.directory.0.join("voice").join(directory))
                .unwrap()
                .next()
                .is_none(),
            "wrong method cannot create a voice stage or published object"
        );
    }
}
#[test]
fn voice_typed_methods_reject_before_body_reservation_or_physical_permit_and_preserve_exact_allow()
{
    let _scenario = super::super::shared_upload::test_guard();
    let f = fixture();
    let revision = f.backend.global_revision().unwrap();
    // Even a saturated physical budget cannot replace an earlier method error
    // with CAPACITY, and no body reader may run before this decision.
    let first = PhysicalUploadPermit::take().unwrap();
    let second = PhysicalUploadPermit::take().unwrap();
    assert!(PhysicalUploadPermit::take().is_none());
    for (method, target, allow) in [
        ("GET", "/fc/api/v1/voice/uploads?classId=1", "POST"),
        ("DELETE", "/fc/api/v1/voice/uploads?classId=1", "POST"),
        ("DELETE", "/fc/api/v1/voice/broadcasts", "GET, POST"),
    ] {
        let mut head = f.head.clone();
        head.method = method.into();
        head.target = target.into();
        let (control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
        expect_method_allow(
            method_rejection(f.app.begin_voice_upload(&head, life)),
            allow,
        );
        drop(control);
        expect_method_allow(
            method_rejection(f.app.prepare_voice_upload(&head, MethodNeverRead)),
            allow,
        );
        expect_method_allow(f.app.dispatch_voice_upload(&head, MethodNeverRead), allow);
        assert_no_voice_admission(&f, revision);
    }
    drop((first, second));
    let first = PhysicalUploadPermit::take().expect("first original physical charge returned");
    let second = PhysicalUploadPermit::take().expect("second original physical charge returned");
    assert!(PhysicalUploadPermit::take().is_none());
    drop((first, second));
}

/// Retry only the exact already-owned test job with its ORIGINAL absolute
/// deadline. Actual finished is observed before reading the memory-only reply.
fn run_method_step(pool: &WorkerPool<Step>, mut step: Step, deadline: Instant) {
    let ticket = loop {
        match pool.handle().try_submit(step, deadline) {
            Ok(ticket) => break ticket,
            Err(rejected) => {
                assert_eq!(rejected.kind, fc_runtime::worker::Rejection::Busy);
                assert!(
                    Instant::now() < deadline,
                    "fixed method-regression admission deadline"
                );
                step = rejected.task;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    };
    while !ticket.is_finished() {
        assert!(
            Instant::now() < deadline,
            "actual method-regression worker retirement deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    ticket.wait().unwrap();
}
#[test]
fn voice_upload_task_without_head_admit_returns_exact_405_and_releases_hub_after_actual_completion()
{
    use crate::web_transfer::{HttpCompletion, HttpJob, UploadHubOwner, UploadTask};
    let _scenario = super::super::shared_upload::test_guard();
    let f = fixture();
    let revision = f.backend.global_revision().unwrap();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let pool = WorkerPool::<Step>::start().unwrap();
    let first = PhysicalUploadPermit::take().unwrap();
    let second = PhysicalUploadPermit::take().unwrap();
    for (target, allow) in [
        ("/fc/api/v1/voice/uploads?classId=1", "POST"),
        ("/fc/api/v1/voice/broadcasts", "GET, POST"),
    ] {
        let mut head = f.head.clone();
        head.method = "DELETE".into();
        head.target = target.into();
        head.token = TokenIssuer::new().unwrap().issue(0).unwrap();
        let token = head.token;
        let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
        let original_life = life.clone();
        let task =
            UploadTask::begin(hub.clone(), &f.app, UploadKind::Voice, head, control, life).unwrap();
        let deadline = task.deadline();
        let reply = Arc::new(std::sync::Mutex::new(None));
        let produced = reply.clone();
        // Actual production heterogeneous job on the existing two-worker test
        // pool; WebTask::admit is intentionally never invoked in this chain.
        run_method_step(
            &pool,
            Step::new(move |context| {
                *produced.lock().unwrap() = Some(HttpJob::Upload(Box::new(task)).run(context));
            }),
            deadline,
        );
        let HttpCompletion::Upload(completion) = reply.lock().unwrap().take().unwrap() else {
            panic!("voice method regression returned the wrong task kind")
        };
        assert_eq!(completion.token, token);
        let response = match completion.result {
            Err(crate::web_transfer::UploadHubFailure::Response(response)) => response,
            _ => panic!("voice worker must preserve the original boundary 405"),
        };
        expect_method_allow(response, allow);
        assert!(
            !original_life.is_active(),
            "failed begin disposed its original control on the actual worker"
        );
        let worker_hub = hub.clone();
        run_method_step(
            &pool,
            Step::new(move |context| {
                assert_eq!(worker_hub.key_for_request(token, context).unwrap(), None,
                "failed begin returned the Hub descriptor before actual completion was observed");
            }),
            deadline,
        );
        assert_no_voice_admission(&f, revision);
    }
    pool.join().unwrap();
    assert!(owner.drain_after_join(&f.app).is_drained());
    drop((first, second));
    let first = PhysicalUploadPermit::take()
        .expect("first physical permit remains available after worker drain");
    let second = PhysicalUploadPermit::take()
        .expect("second physical permit remains available after worker drain");
    assert!(PhysicalUploadPermit::take().is_none());
    drop((first, second));
}
