//! Real Windows handles and shared two-worker calls, with real pairing/SQLite.
//! HTTP metadata here is a port fixture, not evidence of a TLS listener.
use super::*;
use crate::web_transfer::TransferControl;
use fc_domain::{auth::Role, identity::*, time::*};
use fc_ports::{http::TokenIssuer, Clock, MonotonicClock};
use fc_runtime::worker::{Task, WorkerPool};
use fc_security::{CredentialRecordCodec, Password};
use std::{
    fs::OpenOptions,
    io::Cursor,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const PASSWORD: &str = "Synthetic-Material-Streaming-Password-2026";
#[allow(
    clippy::permissions_set_readonly_false,
    reason = "Windows-only fixture: clear the test inode's readonly attribute, not Unix mode bits"
)]
fn clear_windows_readonly(permissions: &mut std::fs::Permissions) {
    permissions.set_readonly(false);
}
struct Time(AtomicU64);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 36_000).unwrap()
    }
}
impl MonotonicClock for Time {
    fn monotonic_millis(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "FutureClassroom-Material-Stream-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("material stream fixture cleanup");
    }
}
struct Fixture {
    backend: Arc<Mutex<Backend>>,
    app: Arc<WebApplication>,
    cookie: String,
    csrf: String,
    directory: Directory,
}
fn field(bytes: &[u8], key: &str) -> String {
    std::str::from_utf8(bytes)
        .unwrap()
        .split_once(&format!("\"{key}\":\""))
        .unwrap()
        .1
        .split('"')
        .next()
        .unwrap()
        .to_owned()
}
fn expect(response: WebResponse, status: u16) {
    assert_eq!(
        response.head.status,
        status,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
}
fn request(
    method: &str,
    route: &str,
    len: usize,
    cookie: Option<&str>,
    csrf: Option<&str>,
) -> RequestHead {
    let mut headers = vec![Header {
        name: "Host".into(),
        value: b"board.school".to_vec(),
    }];
    if method == "POST" {
        headers.extend([
            Header {
                name: "Origin".into(),
                value: b"https://board.school".to_vec(),
            },
            Header {
                name: "Content-Type".into(),
                value: b"application/json".to_vec(),
            },
            Header {
                name: "Content-Length".into(),
                value: len.to_string().into_bytes(),
            },
        ]);
    }
    if let Some(cookie) = cookie {
        headers.push(Header {
            name: "Cookie".into(),
            value: cookie.as_bytes().to_vec(),
        });
    }
    if let Some(csrf) = csrf {
        headers.push(Header {
            name: "X-FC-CSRF".into(),
            value: csrf.as_bytes().to_vec(),
        });
    }
    RequestHead {
        token: TokenIssuer::new().unwrap().issue(0).unwrap(),
        method: method.into(),
        target: format!("/fc/api/v1/{route}"),
        headers,
        tls: true,
        content_length: (method == "POST").then_some(len as u64),
        has_body: method == "POST" && len != 0,
    }
}
impl Fixture {
    fn new() -> Self {
        let directory = Directory::new();
        let clock = Arc::new(Time(AtomicU64::new(1000)));
        let mut backend = Backend::open(
            directory.0.join("state.sqlite3"),
            CredentialRecordCodec::from_key([68; 32]).unwrap(),
            clock.clone(),
            clock,
        )
        .unwrap();
        drop(
            backend
                .initialize(
                    crate::backend::Initialize {
                        school_name: "Synthetic stream school".into(),
                        login_name: "admin".into(),
                        display_name: "Synthetic stream administrator".into(),
                    },
                    Password::new(PASSWORD.into()).unwrap(),
                )
                .unwrap(),
        );
        let admin = backend
            .login("admin", Password::new(PASSWORD.into()).unwrap())
            .unwrap();
        backend
            .identity(
                admin,
                IdentityCommand::CreateClass {
                    id: ClassId::new(1).unwrap(),
                    name: "Synthetic class".into(),
                },
            )
            .unwrap();
        backend
            .identity(
                admin,
                IdentityCommand::PutSubject {
                    id: SubjectId::new(1).unwrap(),
                    name: "Synthetic subject".into(),
                    enabled: true,
                    color: Rgb {
                        red: 2,
                        green: 3,
                        blue: 4,
                    },
                    expected_revision: None,
                },
            )
            .unwrap();
        backend
            .identity(
                admin,
                IdentityCommand::UpdateAccount {
                    id: PrincipalId::new(1).unwrap(),
                    display_name: "Synthetic stream administrator".into(),
                    role: Role::GlobalAdmin,
                    enabled: true,
                    scopes: vec![AssignmentScope {
                        class_id: ClassId::new(1).unwrap(),
                        subject_id: SubjectId::new(1).unwrap(),
                    }],
                },
            )
            .unwrap();
        let admin = backend
            .login("admin", Password::new(PASSWORD.into()).unwrap())
            .unwrap();
        let service = backend.web_service();
        service.set_enabled(admin, true).unwrap();
        let mut app = WebApplication::new(
            &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
            service.clone(),
        )
        .unwrap();
        app.configure_material_files(&directory.0).unwrap();
        let window = service.begin_pairing(admin).unwrap();
        let body = format!(
            r#"{{"schemaVersion":1,"code":"{}","loginName":"admin","deviceName":"stream worker fixture"}}"#,
            window.code.expose_text().as_str()
        );
        let created = app.dispatch(
            &mut backend,
            &request("POST", "pairing/requests", body.len(), None, None),
            body.as_bytes(),
        );
        assert_eq!(created.head.status, 202);
        let id = field(&created.body, "requestId").parse::<u64>().unwrap();
        let poll = format!(
            r#"{{"schemaVersion":1,"pollSecret":"{}"}}"#,
            field(&created.body, "pollSecret")
        );
        let review = service
            .pending(admin)
            .unwrap()
            .into_iter()
            .find(|item| item.request_id == id)
            .unwrap();
        service
            .approve(admin, id, review.identity_revision)
            .unwrap();
        let paired = app.dispatch(
            &mut backend,
            &request(
                "POST",
                &format!("pairing/requests/{id}/poll"),
                poll.len(),
                None,
                None,
            ),
            poll.as_bytes(),
        );
        assert_eq!(paired.head.status, 200);
        let cookie = std::str::from_utf8(
            &paired
                .head
                .headers
                .iter()
                .find(|h| h.name == "Set-Cookie")
                .unwrap()
                .value,
        )
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
        let session = app.dispatch(
            &mut backend,
            &request("GET", "session", 0, Some(&cookie), None),
            b"",
        );
        assert_eq!(session.head.status, 200);
        let csrf = field(&session.body, "csrf");
        Self {
            backend: Arc::new(Mutex::new(backend)),
            app: Arc::new(app),
            cookie,
            csrf,
            directory,
        }
    }
    fn head(&self, route: &str, len: usize) -> RequestHead {
        request("POST", route, len, Some(&self.cookie), Some(&self.csrf))
    }
    fn reserve(&mut self, bytes: usize) -> (u64, RequestHead, PathBuf) {
        let body = format!(r#"{{"schemaVersion":1,"displayName":"stream.txt","bytes":{bytes}}}"#);
        let head = self.head("materials/uploads/1/1", body.len());
        let response = self
            .app
            .dispatch(&mut self.backend.lock().unwrap(), &head, body.as_bytes());
        assert_eq!(
            response.head.status,
            201,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let id = field(&response.body, "uploadId").parse().unwrap();
        let mut head = self.head(&format!("materials/uploads/{id}/bytes"), bytes);
        head.headers
            .iter_mut()
            .find(|h| h.name == "Content-Type")
            .unwrap()
            .value = b"application/octet-stream".to_vec();
        let path = self
            .directory
            .0
            .join("web-material-staging")
            .join(format!("web-material-{id}.part"));
        (id, head, path)
    }
    fn append(&self, id: u64, command: u64) -> (RequestHead, Vec<u8>) {
        let body=format!(r#"{{"schemaVersion":1,"commandId":"{command}","uploadId":"{id}","allowedClassIds":["1"]}}"#).into_bytes();
        (self.head("materials/folders/1/1/append", body.len()), body)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.backend
            .lock()
            .unwrap()
            .close()
            .expect("stream backend shutdown");
    }
}

type WorkerCall = Box<dyn FnOnce(&JobContext) + Send>;
struct Job(WorkerCall);
impl Task for Job {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(CHUNK * 3)
    }
    fn run(self, context: &JobContext) {
        (self.0)(context)
    }
}
struct Pool(Option<WorkerPool<Job>>);
impl Pool {
    fn new() -> Self {
        Self(Some(WorkerPool::start().unwrap()))
    }
    fn call<T: Send + 'static>(&self, call: impl FnOnce(&JobContext) -> T + Send + 'static) -> T {
        let output = Arc::new(Mutex::new(None));
        let returned = output.clone();
        let ticket = match self.0.as_ref().unwrap().handle().try_submit(
            Job(Box::new(move |context| {
                let value = call(context);
                *returned.lock().unwrap() = Some(value);
            })),
            Instant::now() + Duration::from_secs(15),
        ) {
            Ok(ticket) => ticket,
            Err(_) => panic!("worker admission"),
        };
        ticket.wait().unwrap();
        let value = output.lock().unwrap().take().unwrap();
        value
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        self.0
            .take()
            .unwrap()
            .join()
            .expect("both stream workers joined");
        let cleanup = super::super::shared_upload::retry_quarantined_uploads();
        assert_eq!(
            cleanup.retained, 0,
            "stream fixture quarantine cleanup after actual worker join"
        );
    }
}
type Owner = Arc<Mutex<Option<ReceivingMaterialUpload>>>;
fn begin(
    pool: &Pool,
    app: Arc<WebApplication>,
    head: RequestHead,
    life: TransferLifetime,
) -> Owner {
    let owner = Arc::new(Mutex::new(None));
    let stored = owner.clone();
    pool.call(move |context| {
        let receiver = match app.begin_material_upload(&head, life, context) {
            Ok(v) => v,
            Err(r) => panic!("begin {}", r.head.status),
        };
        *stored.lock().unwrap() = Some(receiver);
    });
    owner
}
fn write(
    pool: &Pool,
    owner: &Owner,
    token: RequestToken,
    sequence: u64,
    offset: u64,
    bytes: Vec<u8>,
) -> Result<UploadAck, WebResponse> {
    let stored = owner.clone();
    pool.call(move |context| {
        let mut receiver = stored.lock().unwrap().take().unwrap();
        let result = receiver.write(token, sequence, offset, &bytes, context);
        *stored.lock().unwrap() = Some(receiver);
        result
    })
}
fn finish(pool: &Pool, owner: &Owner, token: RequestToken) -> WebResponse {
    let stored = owner.clone();
    pool.call(move |context| {
        let receiver = stored.lock().unwrap().take().unwrap();
        receiver.finish(token, context)
    })
}
fn dispose(pool: &Pool, owner: Owner) {
    pool.call(move |_| {
        let receiver = owner.lock().unwrap().take();
        drop(receiver);
    });
}
fn abort(pool: &Pool, owner: &Owner) -> bool {
    let stored = owner.clone();
    pool.call(move |_| {
        let mut receiver = stored.lock().unwrap().take().unwrap();
        let cleaned = receiver.abort();
        *stored.lock().unwrap() = Some(receiver);
        cleaned
    })
}

#[test]
fn material_stream_distinct_worker_tickets_ack_real_writes_and_keep_same_sealed_file() {
    let _guard = super::super::shared_upload::test_guard();
    let mut f = Fixture::new();
    let pool = Pool::new();
    let bytes = b"Synthetic streamed UTF-8 material\n";
    let (id, head, path) = f.reserve(bytes.len());
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let deadline = life.deadline();
    let owner = begin(&pool, f.app.clone(), head, life);
    // Begin's ticket is already retired. Its cancellation must not poison a
    // subsequent real worker call on this still-live transfer.
    assert!(OpenOptions::new().read(true).open(&path).is_err());
    let ack = match write(&pool, &owner, token, 1, 0, bytes[..9].to_vec()) {
        Ok(v) => v,
        Err(r) => panic!("write {}", r.head.status),
    };
    assert_eq!(
        (
            ack.token(),
            ack.kind(),
            ack.upload_id(),
            ack.sequence(),
            ack.next_offset()
        ),
        (token, UploadKind::Material, id, 1, 9)
    );
    assert!(write(&pool, &owner, token, 2, 9, bytes[9..].to_vec()).is_ok());
    expect(finish(&pool, &owner, token), 200);
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    let slot = f
        .app
        .material_files
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .find(id)
        .unwrap();
    let actual = pool.call(move |_| {
        let (phase, identity) = {
            let mut slot = slot.lock().unwrap();
            assert_eq!(slot.expires, deadline);
            (
                std::mem::replace(&mut slot.phase, Phase::Consuming),
                slot.identity,
            )
        };
        let Phase::Ready(mut file) = phase else {
            panic!("same sealed Ready owner")
        };
        assert_eq!(file.identity().unwrap(), identity);
        let mut actual = Vec::new();
        file.file_mut().seek(SeekFrom::Start(0)).unwrap();
        file.file_mut().read_to_end(&mut actual).unwrap();
        assert!(file.file_mut().write_all(b"mutation").is_err());
        slot.lock().unwrap().phase = Phase::Ready(file);
        actual
    });
    assert_eq!(actual, bytes);
    let (head, body) = f.append(id, 88001);
    let app = f.app.clone();
    let prepared = pool.call(move |context| {
        match app.prepare_material_append_with_context(&head, &body, context) {
            Ok(v) => v,
            Err(r) => panic!("prepare {}", r.head.status),
        }
    });
    assert_eq!(prepared.expires, deadline);
    assert_eq!(prepared.lifetime.as_ref().unwrap().token(), token);
    let app = f.app.clone();
    let backend = f.backend.clone();
    expect(
        pool.call(move |context| {
            app.commit_material_append_with_context(&mut backend.lock().unwrap(), prepared, context)
        }),
        200,
    );
    assert!(!path.exists());
    drop(control);
}

#[test]
fn material_stream_invalid_correlation_size_and_replay_are_sticky_without_early_permit_release() {
    let _guard = super::super::shared_upload::test_guard();
    let mut f = Fixture::new();
    let pool = Pool::new();
    for case in 0..6 {
        let (_, head, path) = f.reserve(CHUNK + 1);
        let token = head.token;
        let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
        let owner = begin(&pool, f.app.clone(), head, life);
        let second = PhysicalUploadPermit::take().expect("one remaining physical permit");
        assert!(PhysicalUploadPermit::take().is_none());
        let other = TokenIssuer::new().unwrap().issue(0).unwrap();
        let (token_arg, sequence, offset, bytes) = match case {
            0 => (other, 1, 0, vec![b'a']),
            1 => (token, 0, 0, vec![b'a']),
            2 => (token, 1, 1, vec![b'a']),
            3 => (token, 1, 0, vec![]),
            4 => (token, 1, 0, vec![b'a'; CHUNK + 1]),
            _ => {
                assert!(write(&pool, &owner, token, 1, 0, vec![b'a']).is_ok());
                (token, 1, 1, vec![b'b'])
            }
        };
        assert!(write(&pool, &owner, token_arg, sequence, offset, bytes).is_err());
        assert!(write(&pool, &owner, token, 1, 0, vec![b'a']).is_err());
        assert!(
            PhysicalUploadPermit::take().is_none(),
            "failed stage is still physically owned"
        );
        assert!(OpenOptions::new().read(true).open(&path).is_err());
        dispose(&pool, owner);
        assert!(!path.exists());
        assert!(PhysicalUploadPermit::take().is_some());
        drop(second);
        drop(control);
    }
    let (id, head, path) = f.reserve(CHUNK + 1);
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let owner = begin(&pool, f.app.clone(), head, life);
    assert!(write(&pool, &owner, token, 1, 0, vec![b'a'; CHUNK]).is_ok());
    assert!(write(&pool, &owner, token, 2, CHUNK as u64, vec![b'\n']).is_ok());
    expect(finish(&pool, &owner, token), 200);
    let app = f.app.clone();
    assert!(pool
        .call(move |ctx| app.discard_material_transfer(token, id, ctx))
        .is_ok());
    assert!(!path.exists());
    drop(control);
}

#[test]
fn material_stream_cancel_expiry_and_ready_cleanup_keep_original_token_and_deadline() {
    let _guard = super::super::shared_upload::test_guard();
    let mut f = Fixture::new();
    let pool = Pool::new();
    let (_, head, path) = f.reserve(4);
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let owner = begin(&pool, f.app.clone(), head, life);
    control.cancel();
    assert!(write(&pool, &owner, token, 1, 0, b"text".to_vec()).is_err());
    // Sweeping registry metadata cannot free a real in-flight writer's charge.
    f.app.sweep_material_uploads().unwrap();
    let other = PhysicalUploadPermit::take().unwrap();
    assert!(PhysicalUploadPermit::take().is_none());
    dispose(&pool, owner);
    assert!(!path.exists());
    drop(other);

    let (id, head, path) = f.reserve(4);
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let owner = begin(&pool, f.app.clone(), head, life);
    assert!(write(&pool, &owner, token, 1, 0, b"text".to_vec()).is_ok());
    expect(finish(&pool, &owner, token), 200);
    let app = f.app.clone();
    let wrong = TokenIssuer::new().unwrap().issue(0).unwrap();
    assert!(pool
        .call(move |_| app.discard_material_transfer_in_cleanup(wrong, id))
        .is_err());
    assert!(path.exists());
    assert!(f.app.material_transfer_pending(token, id));
    assert!(!f.app.material_transfer_pending(wrong, id));
    // Cleanup is a fresh worker call and remains possible after cancellation.
    control.cancel();
    let app = f.app.clone();
    assert!(pool
        .call(move |_| app.discard_material_transfer_in_cleanup(token, id))
        .is_ok());
    assert!(!path.exists());
    assert!(!f.app.material_transfer_pending(token, id));

    let (id, head, path) = f.reserve(4);
    let token = head.token;
    // Model an old reservation's original 300-second cutoff, already earlier
    // than a newly admitted transport's 120-second deadline. It must not renew.
    let expires = Instant::now() + Duration::from_secs(60);
    f.app
        .material_files
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .find(id)
        .unwrap()
        .lock()
        .unwrap()
        .expires = expires;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let owner = begin(&pool, f.app.clone(), head, life);
    let stored = owner.clone();
    assert_eq!(
        pool.call(move |_| stored.lock().unwrap().as_ref().unwrap().deadline()),
        expires
    );
    // Deadline passes are deterministic without sleeping or resetting clocks.
    let stored = owner.clone();
    pool.call(move |_| {
        stored.lock().unwrap().as_mut().unwrap().expires = Instant::now() - Duration::from_millis(1)
    });
    assert!(write(&pool, &owner, token, 1, 0, b"text".to_vec()).is_err());
    dispose(&pool, owner);
    assert!(!path.exists());
    drop(control);

    // Exercise the actual monotonic stable-lifetime deadline as well as the
    // separately tightened original reservation deadline above.
    let (_, head, path) = f.reserve(4);
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let life = life.bounded(Duration::from_secs(2)).unwrap();
    let deadline = life.deadline();
    let owner = begin(&pool, f.app.clone(), head, life);
    std::thread::sleep(
        deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(20),
    );
    assert!(write(&pool, &owner, token, 1, 0, b"text".to_vec()).is_err());
    dispose(&pool, owner);
    assert!(!path.exists());
    drop(control);
}

#[test]
fn material_stream_unadmitted_or_short_body_never_becomes_ready_and_legacy_uses_same_core() {
    let _guard = super::super::shared_upload::test_guard();
    let mut f = Fixture::new();
    let pool = Pool::new();
    let (id, head, path) = f.reserve(4);
    let token = head.token;
    let (control, life) = TransferControl::new(
        TokenIssuer::new().unwrap().issue(0).unwrap(),
        Instant::now(),
    )
    .unwrap();
    let app = f.app.clone();
    let status = pool.call(
        move |context| match app.begin_material_upload(&head, life, context) {
            Ok(_) => panic!("mismatched lifetime"),
            Err(r) => r.head.status,
        },
    );
    assert_eq!(status, 503);
    assert!(path.exists());
    let mut head = f.head(&format!("materials/uploads/{id}/bytes"), 4);
    head.token = token;
    head.headers
        .iter_mut()
        .find(|h| h.name == "Content-Type")
        .unwrap()
        .value = b"application/octet-stream".to_vec();
    let (control2, life) = TransferControl::new(token, Instant::now()).unwrap();
    let owner = begin(&pool, f.app.clone(), head, life);
    assert!(write(&pool, &owner, token, 1, 0, b"tex".to_vec()).is_ok());
    expect(finish(&pool, &owner, token), 400);
    assert!(!path.exists());
    drop(control);
    drop(control2);
    for bytes in [b"tex".as_slice(), b"text!".as_slice()] {
        let (_, head, path) = f.reserve(4);
        expect(
            f.app.dispatch_material_upload(&head, Cursor::new(bytes)),
            400,
        );
        assert!(!path.exists());
    }
    let (id, head, _) = f.reserve(4);
    expect(
        f.app.dispatch_material_upload(&head, Cursor::new(b"text")),
        200,
    );
    let (head, body) = f.append(id, 88002);
    let prepared = match f.app.prepare_material_append(&head, &body) {
        Ok(v) => v,
        Err(r) => panic!("legacy prepare {}", r.head.status),
    };
    assert!(prepared.lifetime.is_some());
    expect(
        f.app
            .commit_material_append(&mut f.backend.lock().unwrap(), prepared),
        200,
    );
}

#[test]
fn material_stream_prepared_proof_keeps_original_transfer_cancellation_through_commit() {
    let _guard = super::super::shared_upload::test_guard();
    let mut f = Fixture::new();
    let pool = Pool::new();
    let (id, head, _) = f.reserve(4);
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let owner = begin(&pool, f.app.clone(), head, life);
    assert!(write(&pool, &owner, token, 1, 0, b"text".to_vec()).is_ok());
    expect(finish(&pool, &owner, token), 200);
    let (head, body) = f.append(id, 88003);
    let app = f.app.clone();
    let prepared = pool.call(move |context| {
        match app.prepare_material_append_with_context(&head, &body, context) {
            Ok(v) => v,
            Err(r) => panic!("prepare {}", r.head.status),
        }
    });
    let published = prepared.path.clone();
    assert!(published.exists());
    let other = PhysicalUploadPermit::take().unwrap();
    assert!(PhysicalUploadPermit::take().is_none());
    control.cancel();
    let app = f.app.clone();
    let backend = f.backend.clone();
    expect(
        pool.call(move |context| {
            app.commit_material_append_with_context(&mut backend.lock().unwrap(), prepared, context)
        }),
        403,
    );
    assert!(!published.exists());
    assert!(PhysicalUploadPermit::take().is_some());
    drop(other);
    let session = f
        .app
        .service
        .authenticate(&cookie_secret(f.cookie.as_bytes()).unwrap())
        .unwrap();
    assert!(f
        .backend
        .lock()
        .unwrap()
        .web_material_outcome(session.lease(), CommandId::new(88003).unwrap())
        .unwrap()
        .is_none());
}

#[test]
fn material_stream_current_worker_cancel_rejects_write_and_disposes_actual_owner_before_join() {
    let _guard = super::super::shared_upload::test_guard();
    let mut f = Fixture::new();
    let pool = Pool::new();
    let (_, head, path) = f.reserve(4);
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let owner = begin(&pool, f.app.clone(), head, life);
    let (started_send, started_receive) = std::sync::mpsc::sync_channel(1);
    let (release_send, release_receive) = std::sync::mpsc::sync_channel(1);
    let status = Arc::new(AtomicU64::new(0));
    let result = status.clone();
    let ticket = match pool.0.as_ref().unwrap().handle().try_submit(
        Job(Box::new(move |context| {
            let mut receiver = owner.lock().unwrap().take().unwrap();
            started_send.send(()).unwrap();
            release_receive
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(context.checkpoint().is_err());
            match receiver.write(token, 1, 0, b"text", context) {
                Ok(_) => panic!("cancelled current worker cannot ACK bytes"),
                Err(response) => result.store(u64::from(response.head.status), Ordering::Release),
            }
            drop(receiver);
        })),
        Instant::now() + Duration::from_secs(15),
    ) {
        Ok(ticket) => ticket,
        Err(_) => panic!("cancellable worker admission"),
    };
    started_receive
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    ticket.cancel();
    release_send.send(()).unwrap();
    assert_eq!(ticket.wait(), Err(fc_runtime::worker::JobError::Cancelled));
    assert_eq!(status.load(Ordering::Acquire), 403);
    assert!(!path.exists());
    let first = PhysicalUploadPermit::take().unwrap();
    let second = PhysicalUploadPermit::take().unwrap();
    drop(first);
    drop(second);
    drop(control);
}

#[test]
fn material_stream_failed_native_delete_quarantines_original_permit_until_exact_retry_succeeds() {
    use super::super::shared_upload::{quarantined_uploads, retry_quarantined_uploads};
    let _guard = super::super::shared_upload::test_guard();
    let mut f = Fixture::new();
    let pool = Pool::new();
    let (id, head, _) = f.reserve(4);
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let owner = begin(&pool, f.app.clone(), head, life);
    assert!(write(&pool, &owner, token, 1, 0, b"text".to_vec()).is_ok());
    expect(finish(&pool, &owner, token), 200);
    let (head, body) = f.append(id, 88004);
    let app = f.app.clone();
    let prepared = pool.call(move |context| {
        match app.prepare_material_append_with_context(&head, &body, context) {
            Ok(v) => v,
            Err(r) => panic!("prepare {}", r.head.status),
        }
    });
    let path = prepared.path.clone();
    let physical_owner = prepared._physical_owner.clone();
    // Keep only metadata ownership after the validation pin really closes.
    // A real independent reader then makes exclusive Windows deletion fail.
    pool.call(move |_| drop(prepared));
    let held = OpenOptions::new().read(true).open(&path).unwrap();
    let remaining = PhysicalUploadPermit::take().unwrap();
    pool.call(move |_| drop(physical_owner));
    assert_eq!(quarantined_uploads(), 1);
    assert!(PhysicalUploadPermit::take().is_none());
    assert!(path.exists());
    let retry = pool.call(|_| retry_quarantined_uploads());
    assert_eq!(retry.retained, 1);
    assert!(PhysicalUploadPermit::take().is_none());
    drop(held);
    let retry = pool.call(|_| retry_quarantined_uploads());
    assert_eq!(retry.retained, 0);
    assert_eq!(quarantined_uploads(), 0);
    assert!(!path.exists());
    assert!(PhysicalUploadPermit::take().is_some());
    drop(remaining);
    drop(control);
}

#[test]
fn material_stream_abort_real_readonly_delete_failure_retries_without_releasing_original_owner() {
    struct RestoreReadonly(PathBuf);
    impl Drop for RestoreReadonly {
        fn drop(&mut self) {
            // Preserve a primary assertion failure while making its original
            // controlled test inode eligible for the owning Drop/retry path.
            if let Ok(metadata) = std::fs::metadata(&self.0) {
                let mut permissions = metadata.permissions();
                clear_windows_readonly(&mut permissions);
                let _ = std::fs::set_permissions(&self.0, permissions);
            }
        }
    }
    let _guard = super::super::shared_upload::test_guard();
    let mut f = Fixture::new();
    let pool = Pool::new();
    let (id, head, path) = f.reserve(4);
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let owner = begin(&pool, f.app.clone(), head, life);
    let _restore = RestoreReadonly(path.clone());
    assert!(write(&pool, &owner, token, 1, 0, b"text".to_vec()).is_ok());
    let stored = owner.clone();
    pool.call(move |_| {
        let mut receiver = stored.lock().unwrap().take().unwrap();
        let file = receiver.file.as_mut().unwrap().file_mut();
        let mut permissions = file.metadata().unwrap().permissions();
        permissions.set_readonly(true);
        file.set_permissions(permissions).unwrap();
        *stored.lock().unwrap() = Some(receiver);
    });
    let second = PhysicalUploadPermit::take().unwrap();
    assert!(
        !abort(&pool, &owner),
        "actual readonly Windows disposition must fail"
    );
    assert!(f.app.material_transfer_pending(token, id));
    assert!(path.exists());
    assert!(PhysicalUploadPermit::take().is_none());
    // A new independent reader proves abort really closed the exclusive writer
    // even though exact deletion failed and the owner/charge must be retained.
    drop(OpenOptions::new().read(true).open(&path).unwrap());
    assert!(write(&pool, &owner, token, 2, 4, vec![b'!']).is_err());
    let restore = path.clone();
    pool.call(move |_| {
        let mut permissions = std::fs::metadata(&restore).unwrap().permissions();
        clear_windows_readonly(&mut permissions);
        std::fs::set_permissions(&restore, permissions).unwrap();
    });
    assert!(abort(&pool, &owner));
    assert!(abort(&pool, &owner), "negative cleanup is idempotent");
    assert!(!path.exists());
    assert!(!f.app.material_transfer_pending(token, id));
    assert!(
        PhysicalUploadPermit::take().is_none(),
        "Hub must still drop the original owner after cleanup"
    );
    dispose(&pool, owner);
    assert!(PhysicalUploadPermit::take().is_some());
    drop(second);
    drop(control);
}
