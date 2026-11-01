//! Real Hub ownership on Windows handles, pairing and SQLite.
//! Request metadata is a port fixture, not proof of a connected TLS listener.
use super::*;
use crate::web_transfer::TransferControl;
use fc_domain::ids::*;
use fc_domain::{auth::Role, identity::*, time::*};
use fc_ports::http::{Header, ListenPrefix};
use fc_ports::{http::TokenIssuer, Clock, MonotonicClock};
use fc_runtime::worker::{Task, WorkerPool};
use fc_security::{CredentialRecordCodec, Password};
use std::{
    fs::OpenOptions,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use std::{path::PathBuf, time::Duration};

const PASSWORD: &str = "Synthetic-Upload-Hub-Password-2026";
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
            "FutureClassroom-Upload-Hub-{}-{}",
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
pub(crate) struct Fixture {
    pub(crate) backend: Arc<Mutex<Backend>>,
    pub(crate) app: Arc<WebApplication>,
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
    pub(crate) fn new() -> Self {
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
    pub(crate) fn head(&self, route: &str, len: usize) -> RequestHead {
        request("POST", route, len, Some(&self.cookie), Some(&self.csrf))
    }
    pub(crate) fn reserve(&mut self, bytes: usize) -> (u64, RequestHead, PathBuf) {
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
    pub(crate) fn append(&self, id: u64, command: u64) -> (RequestHead, Vec<u8>) {
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
        Some(CHUNK_BYTES * 3)
    }
    fn run(self, context: &JobContext) {
        (self.0)(context);
    }
}
pub(crate) struct Pool(Option<WorkerPool<Job>>);
impl Pool {
    pub(crate) fn new() -> Self {
        Self(Some(WorkerPool::start().unwrap()))
    }
    pub(crate) fn call<T: Send + 'static>(
        &self,
        call: impl FnOnce(&JobContext) -> T + Send + 'static,
    ) -> T {
        let result = Arc::new(Mutex::new(None));
        let written = result.clone();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut task = Job(Box::new(move |context| {
            *written.lock().unwrap() = Some(call(context));
        }));
        let ticket = loop {
            match self.0.as_ref().unwrap().handle().try_submit(task, deadline) {
                Ok(ticket) => break ticket,
                Err(error)
                    if error.kind == fc_runtime::worker::Rejection::Busy
                        && Instant::now() < deadline =>
                {
                    // Busy has not accepted this task. Retry its original owner
                    // under the same absolute deadline, without replaying work.
                    task = error.task;
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("test worker admission {:?}", error.kind),
            }
        };
        ticket.wait().unwrap();
        let returned = result.lock().unwrap().take().unwrap();
        returned
    }
    pub(crate) fn join(&mut self) -> Result<(), fc_runtime::worker::JoinError> {
        self.0.take().unwrap().join()
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        if let Some(pool) = self.0.take() {
            pool.join().expect("real Hub workers joined");
        }
    }
}
fn success<T>(result: Result<T, UploadHubFailure>) -> T {
    match result {
        Ok(value) => value,
        Err(UploadHubFailure::Hub(error)) => panic!("Hub {error:?}"),
        Err(UploadHubFailure::Response(response)) => {
            panic!("upload response {}", response.head.status)
        }
    }
}
fn begin(
    pool: &Pool,
    hub: &UploadHub,
    f: &Fixture,
    head: RequestHead,
) -> (UploadAdmission, TransferLifetime) {
    let (control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let stable = life.clone();
    let hub = hub.clone();
    let app = f.app.clone();
    (
        pool.call(move |context| success(hub.begin_material(&app, &head, control, life, context))),
        stable,
    )
}
fn write(pool: &Pool, hub: &UploadHub, key: UploadKey, bytes: &[u8]) -> UploadAck {
    let hub = hub.clone();
    let bytes = bytes.to_vec();
    pool.call(move |context| success(hub.write(key, 1, 0, &bytes, context)))
}
fn ready(
    pool: &Pool,
    hub: &UploadHub,
    f: &Fixture,
    head: RequestHead,
    bytes: &[u8],
) -> (UploadAdmission, TransferLifetime) {
    let (admission, life) = begin(pool, hub, f, head);
    write(pool, hub, admission.key, bytes);
    let worker = hub.clone();
    let app = f.app.clone();
    let key = admission.key;
    let finished = pool.call(move |context| success(worker.finish(key, &app, context)));
    assert!(finished.network_control.is_none());
    expect(finished.response, 200);
    (admission, life)
}
// Deterministic expiry seam: tighten only this Hub entry's original deadline.
// This exercises real worker/native cleanup, not a 120-second wallclock claim.
fn expire_hub_entry(pool: &Pool, hub: &UploadHub, key: UploadKey) {
    let worker = hub.clone();
    pool.call(move |context| {
        let shared = worker_shared(&worker, context, true).unwrap();
        let mut lease = shared.take(key).unwrap();
        let deadline = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        assert!(deadline < lease.entry().life.deadline);
        lease.entry().life.deadline = deadline;
    });
}
#[allow(
    clippy::permissions_set_readonly_false,
    reason = "Windows-only fixture: restore the exact test inode's readonly attribute"
)]
fn restore_readonly(path: &std::path::Path) -> std::io::Result<()> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut permissions = metadata.permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(path, permissions)
}

#[test]
fn authenticated_json_cancel_and_consumed_retained_are_reaped_without_cancelling_live_ready() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let (first_id, first_head, first_path) = f.reserve(4);
    let (first, original_life) = ready(&pool, &hub, &f, first_head, b"old1");
    let (_, live_head, live_path) = f.reserve(4);
    let (live, live_life) = ready(&pool, &hub, &f, live_head, b"live");
    let live_deadline = live_life.deadline();
    let body = br#"{"schemaVersion":1}"#.to_vec();
    let cancel_head = f.head(&format!("materials/uploads/{first_id}/cancel"), body.len());
    assert_ne!(cancel_head.token, first.key.token());
    let (cancel_control, cancel_life) =
        TransferControl::new(cancel_head.token, Instant::now()).unwrap();
    let app = f.app.clone();
    let backend = f.backend.clone();
    // A genuine authenticated JSON operation on the same real shared pool,
    // not an atomic cancellation of the old binary connection's lifetime.
    let cancelled = pool.call(move |context| {
        app.dispatch_in_job(
            &mut backend.lock().unwrap(),
            &cancel_head,
            &body,
            &cancel_life,
            context,
        )
    });
    assert_eq!(cancelled.head.status, 200);
    assert!(String::from_utf8_lossy(&cancelled.body).contains("UPLOAD_CANCELLED"));
    drop(cancel_control);
    assert!(!first_path.exists());
    assert_eq!(
        f.app.material_transfer_binding(first_id, &original_life),
        MaterialTransferBinding::Absent
    );
    assert!(
        original_life.is_active(),
        "ordinary JSON cancel did not cancel the original Hub control"
    );
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.ready_material_key(first_id, context))
            .unwrap(),
        Some(first.key),
        "the old logical Ready descriptor reproduces the registry gap"
    );
    let (next_id, next_head, _) = f.reserve(4);
    let (next, next_life) = ready(&pool, &hub, &f, next_head, b"next");
    assert_eq!(next.key.slot, first.key.slot);
    assert!(next.key.serial > first.key.serial);
    assert!(!original_life.is_active());
    assert!(live_life.is_active());
    assert_eq!(live_life.deadline(), live_deadline);
    assert!(live_path.exists());
    assert!(OpenOptions::new().write(true).open(&live_path).is_err());
    // Real successful preparation followed by Backend contention leaves an
    // active Retained descriptor after its local proof and App owner retired.
    // Admission can reclaim it without waiting for its original 120s deadline.
    let (append_head, append_body) = f.append(next_id, 91004);
    let alias = append_head.token;
    let (append_control, append_life) = TransferControl::new(alias, Instant::now()).unwrap();
    let held_backend = f.backend.lock().unwrap();
    let worker = hub.clone();
    let app = f.app.clone();
    let backend = f.backend.clone();
    assert!(pool.call(move |context| matches!(
        worker.append_ready(
            next.key,
            &app,
            &backend,
            &append_head,
            &append_body,
            &append_life,
            context
        ),
        Err(UploadHubFailure::Hub(UploadHubError::Busy))
    )));
    drop(held_backend);
    assert!(next_life.is_active());
    assert_eq!(
        f.app.material_transfer_binding(next_id, &next_life),
        MaterialTransferBinding::Absent
    );
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(alias, context))
            .unwrap(),
        Some(next.key)
    );
    let (_, replacement_head, _) = f.reserve(4);
    let (replacement, _) = begin(&pool, &hub, &f, replacement_head);
    assert_eq!(replacement.key.slot, next.key.slot);
    assert!(replacement.key.serial > next.key.serial);
    assert!(!next_life.is_active());
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(alias, context))
            .unwrap(),
        None
    );
    assert!(live_life.is_active());
    assert_eq!(live_life.deadline(), live_deadline);
    assert!(live_path.exists());
    for key in [live.key, replacement.key] {
        let worker = hub.clone();
        let app = f.app.clone();
        pool.call(move |context| worker.cleanup(key, &app, context))
            .unwrap();
    }
    drop(append_control);
    pool.join().unwrap();
    assert!(owner.drain_after_join(&f.app).is_drained());
    drop(owner);
}

#[test]
fn expired_ready_worker_reap_and_cancelled_ready_begin_reap_reuse_real_slots() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let (first_id, first_head, first_path) = f.reserve(4);
    let (first, _) = ready(&pool, &hub, &f, first_head, b"old1");
    let (second_id, second_head, second_path) = f.reserve(4);
    let (second, second_life) = ready(&pool, &hub, &f, second_head, b"live");
    let unchanged_deadline = second_life.deadline();
    expire_hub_entry(&pool, &hub, first.key);
    assert!(first_path.exists());
    // Same Backend/service but a distinct application registry must not reap
    // another App's numeric upload ID, even when its Hub lifetime has elapsed.
    let foreign = WebApplication::new(
        &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
        f.backend.lock().unwrap().web_service(),
    )
    .unwrap();
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.reap_expired(&foreign, context)),
        Err(UploadHubError::Invalid)
    );
    assert!(first_path.exists());
    let worker = hub.clone();
    let app = f.app.clone();
    let status = pool
        .call(move |context| worker.reap_expired(&app, context))
        .unwrap();
    assert_eq!(status.pending_slots, 1);
    assert_eq!(status.quarantined, 0);
    assert!(!first_path.exists());
    assert!(!f.app.material_transfer_pending(first.key.token(), first_id));
    assert!(second_path.exists());
    assert!(second_life.is_active());
    assert_eq!(second_life.deadline(), unchanged_deadline);
    let (_, third_head, third_path) = f.reserve(4);
    let (third, _) = ready(&pool, &hub, &f, third_head, b"old3");
    assert_eq!(third.key.slot, first.key.slot);
    assert!(third.key.serial > first.key.serial);
    // App's ordinary metadata sweep can retire the cancelled physical slot,
    // while the Hub still holds its old Ready descriptor. Begin must reap that
    // descriptor itself rather than remain permanently at capacity.
    third.cancellation.cancel();
    let (_, fourth_head, fourth_path) = f.reserve(4);
    assert!(!third_path.exists());
    let (fourth, fourth_life) = begin(&pool, &hub, &f, fourth_head);
    assert_eq!(fourth.key.slot, third.key.slot);
    assert!(fourth.key.serial > third.key.serial);
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(third.key.token(), context))
            .unwrap(),
        None
    );
    third.cancellation.cancel();
    assert!(fourth_life.is_active());
    assert!(fourth_path.exists());
    assert!(f
        .app
        .material_transfer_pending(second.key.token(), second_id));
    for key in [second.key, fourth.key] {
        let worker = hub.clone();
        let app = f.app.clone();
        pool.call(move |context| worker.cleanup(key, &app, context))
            .unwrap();
    }
    pool.join().unwrap();
    assert!(owner.drain_after_join(&f.app).is_drained());
    drop(owner);
}

#[test]
fn expired_ready_real_delete_failure_retains_charge_until_worker_retry() {
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = restore_readonly(&self.0);
        }
    }
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let (id, head, path) = f.reserve(4);
    let (first, _) = ready(&pool, &hub, &f, head, b"old1");
    let (_, second_head, _) = f.reserve(4);
    let (second, second_life) = ready(&pool, &hub, &f, second_head, b"live");
    let _restore = Restore(path.clone());
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    expire_hub_entry(&pool, &hub, first.key);
    let worker = hub.clone();
    let app = f.app.clone();
    let status = pool
        .call(move |context| worker.reap_expired(&app, context))
        .unwrap();
    assert_eq!(status.pending_slots, 2, "failed deletion retains Hub owner");
    assert_eq!(status.quarantined, 0, "original registry still owns charge");
    assert!(path.exists());
    assert!(f.app.material_transfer_pending(first.key.token(), id));
    assert!(second_life.is_active());
    // Actual metadata admission sweeps the now-cancelled registry owner, whose
    // failed exact deletion moves its ORIGINAL permit to bounded quarantine.
    let body = br#"{"schemaVersion":1,"displayName":"next.txt","bytes":4}"#;
    let response = f.app.dispatch(
        &mut f.backend.lock().unwrap(),
        &f.head("materials/uploads/1/1", body.len()),
        body,
    );
    expect(response, 429);
    assert_eq!(quarantined_uploads(), 1);
    assert!(path.exists());
    let restore = path.clone();
    let worker = hub.clone();
    let app = f.app.clone();
    let status = pool
        .call(move |context| {
            restore_readonly(&restore).unwrap();
            let retried = retry_quarantined_uploads();
            assert_eq!(retried.reclaimed, 1);
            worker.reap_expired(&app, context)
        })
        .unwrap();
    assert_eq!(status.pending_slots, 1);
    assert_eq!(status.quarantined, 0);
    assert!(!path.exists());
    let (_, next_head, _) = f.reserve(4);
    let (next, _) = begin(&pool, &hub, &f, next_head);
    assert_eq!(next.key.slot, first.key.slot);
    assert!(next.key.serial > first.key.serial);
    for key in [second.key, next.key] {
        let worker = hub.clone();
        let app = f.app.clone();
        pool.call(move |context| worker.cleanup(key, &app, context))
            .unwrap();
    }
    pool.join().unwrap();
    assert!(owner.drain_after_join(&f.app).is_drained());
    drop(owner);
}

#[test]
fn authenticated_append_backend_busy_binds_new_cleanup_token_but_failed_csrf_does_not() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let (id, head, path) = f.reserve(4);
    let (first, original_life) = ready(&pool, &hub, &f, head, b"text");
    let (_, second_head, second_path) = f.reserve(4);
    let (second, second_life) = ready(&pool, &hub, &f, second_head, b"live");
    let (mut bad_head, bad_body) = f.append(id, 91003);
    bad_head
        .headers
        .iter_mut()
        .find(|header| header.name == "X-FC-CSRF")
        .unwrap()
        .value = b"invalid-fixture-csrf".to_vec();
    let bad_token = bad_head.token;
    let (bad_control, bad_life) = TransferControl::new(bad_token, Instant::now()).unwrap();
    let worker = hub.clone();
    let app = f.app.clone();
    let backend = f.backend.clone();
    assert!(pool.call(move |context| matches!(
        worker.append_ready(first.key, &app, &backend, &bad_head, &bad_body, &bad_life, context),
        Err(UploadHubFailure::Response(_))
    )));
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(bad_token, context))
            .unwrap(),
        None,
        "failed authentication cannot acquire an original upload cleanup alias"
    );
    let worker = hub.clone();
    let app = f.app.clone();
    pool.call(move |context| worker.cleanup_request(bad_token, &app, context))
        .unwrap();
    assert!(path.exists());
    assert!(original_life.is_active());
    let (head, body) = f.append(id, 91003);
    let new_token = head.token;
    assert_ne!(new_token, first.key.token());
    let (control, life) = TransferControl::new(new_token, Instant::now()).unwrap();
    // Real validation/publication preparation must complete without holding
    // Backend. Only its final short commit sees this intentional contention.
    let held_backend = f.backend.lock().unwrap();
    let worker = hub.clone();
    let app = f.app.clone();
    let backend = f.backend.clone();
    assert!(pool.call(move |context| matches!(
        worker.append_ready(first.key, &app, &backend, &head, &body, &life, context),
        Err(UploadHubFailure::Hub(UploadHubError::Busy))
    )));
    drop(held_backend);
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(new_token, context))
            .unwrap(),
        Some(first.key),
        "after actual Append task completion, Net's new token finds old owner"
    );
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.ready_material_key(id, context))
            .unwrap(),
        None,
        "successful preparation consumed Ready before Backend was busy"
    );
    assert!(!path.exists());
    assert!(!f.app.material_transfer_pending(first.key.token(), id));
    let worker = hub.clone();
    let app = f.app.clone();
    pool.call(move |context| worker.cleanup_request(new_token, &app, context))
        .unwrap();
    assert!(!original_life.is_active());
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(first.key.token(), context))
            .unwrap(),
        None
    );
    assert!(second_path.exists());
    assert!(second_life.is_active());
    let (_, next_head, _) = f.reserve(4);
    let (next, _) = begin(&pool, &hub, &f, next_head);
    assert_eq!(next.key.slot, first.key.slot);
    assert!(next.key.serial > first.key.serial);
    for key in [second.key, next.key] {
        let worker = hub.clone();
        let app = f.app.clone();
        pool.call(move |context| worker.cleanup(key, &app, context))
            .unwrap();
    }
    drop(control);
    drop(bad_control);
    pool.join().unwrap();
    assert!(owner.drain_after_join(&f.app).is_drained());
    drop(owner);
}

#[test]
fn material_hub_keeps_ready_across_ticket_drop_and_appends_with_new_authenticated_request() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let bytes = b"Synthetic Hub UTF-8 material\n";
    let (id, head, stage_path) = f.reserve(bytes.len());
    let old_token = head.token;
    let (admission, stable) = begin(&pool, &hub, &f, head);
    let key = admission.key;
    assert_eq!(key.upload_id(), id);
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(old_token, context))
            .unwrap(),
        Some(key)
    );
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.ready_material_key(id, context))
            .unwrap(),
        None
    );
    let ack = write(&pool, &hub, key, bytes);
    assert_eq!(ack.next_offset(), bytes.len() as u64);
    let app = f.app.clone();
    let worker = hub.clone();
    let finished = pool.call(move |context| success(worker.finish(key, &app, context)));
    assert!(
        finished.network_control.is_none(),
        "Ready control belongs to Hub"
    );
    expect(finished.response, 200);
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.ready_material_key(id, context))
            .unwrap(),
        Some(key)
    );
    let unrelated = TokenIssuer::new().unwrap().issue(0).unwrap();
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(unrelated, context))
            .unwrap(),
        None
    );
    // The successful bytes connection has retired; dropping its metadata weak
    // handle cannot cancel the Ready owner's original stable lifetime.
    drop(admission);
    assert!(stable.is_active());
    assert!(stage_path.exists());
    assert!(OpenOptions::new().write(true).open(&stage_path).is_err());
    let (append_head, body) = f.append(id, 91001);
    assert_ne!(append_head.token, old_token);
    let (append_control, append_life) =
        TransferControl::new(append_head.token, Instant::now()).unwrap();
    let wrong_body = format!(
        r#"{{"schemaVersion":1,"commandId":"91001","uploadId":"{}","allowedClassIds":["1"]}}"#,
        id + 1
    )
    .into_bytes();
    let app = f.app.clone();
    let backend = f.backend.clone();
    let worker = hub.clone();
    let head = append_head.clone();
    let life = append_life.clone();
    assert!(pool.call(move |context| matches!(
        worker.append_ready(key, &app, &backend, &head, &wrong_body, &life, context),
        Err(UploadHubFailure::Hub(UploadHubError::Invalid))
    )));
    assert!(stable.is_active());
    let app = f.app.clone();
    let backend = f.backend.clone();
    let worker = hub.clone();
    let response = pool.call(move |context| {
        success(worker.append_ready(
            key,
            &app,
            &backend,
            &append_head,
            &body,
            &append_life,
            context,
        ))
    });
    expect(response, 200);
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.ready_material_key(id, context))
            .unwrap(),
        None
    );
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(old_token, context))
            .unwrap(),
        None
    );
    drop(append_control);
    assert!(!stage_path.exists());
    assert!(
        !stable.is_active(),
        "only factual append retirement drops original control"
    );
    pool.join().unwrap();
    let cleanup = owner.drain_after_join(&f.app);
    assert!(cleanup.is_drained(), "{cleanup:?}");
    drop(owner);
    assert!(!OWNER.load(Ordering::Acquire));
}

#[test]
fn cancelled_receiving_owner_keeps_slot_until_worker_cleanup_then_reuse_rejects_old_serial() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let (_, first_head, first_path) = f.reserve(4);
    let first_token = first_head.token;
    let duplicate_head = first_head.clone();
    let (first, _) = begin(&pool, &hub, &f, first_head);
    let app = f.app.clone();
    let worker = hub.clone();
    let (duplicate_control, duplicate_life) =
        TransferControl::new(duplicate_head.token, Instant::now()).unwrap();
    assert!(pool.call(move |context| matches!(
        worker.begin_material(
            &app,
            &duplicate_head,
            duplicate_control,
            duplicate_life,
            context
        ),
        Err(UploadHubFailure::Hub(UploadHubError::Busy))
    )));
    let (_, second_head, _) = f.reserve(4);
    let (second, _) = begin(&pool, &hub, &f, second_head);
    let app = f.app.clone();
    let worker = hub.clone();
    let head = f.head("voice/uploads/1", 44);
    let (control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
    assert!(pool.call(move |context| matches!(
        worker.begin_voice(&app, &head, control, life, context),
        Err(UploadHubFailure::Hub(UploadHubError::Busy))
    )));
    first.cancellation.cancel();
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(first_token, context))
            .unwrap(),
        Some(first.key)
    );
    let worker = hub.clone();
    let key = first.key;
    assert!(pool.call(move |context| matches!(
        worker.write(key, 1, 0, b"test", context),
        Err(UploadHubFailure::Hub(UploadHubError::Cancelled))
    )));
    assert!(first_path.exists());
    let app = f.app.clone();
    let worker = hub.clone();
    pool.call(move |context| worker.cleanup_request(first_token, &app, context))
        .unwrap();
    assert!(!first_path.exists());
    let (_, mut replacement_head, _) = f.reserve(4);
    // Even an accidentally reused full transport token cannot match an old
    // operation: this exact physical Hub slot has a fresh non-reused serial.
    replacement_head.token = first_token;
    let (replacement, replacement_life) = begin(&pool, &hub, &f, replacement_head);
    let mut delayed = first.key;
    delayed.upload_id = replacement.key.upload_id;
    let worker = hub.clone();
    assert!(pool.call(move |context| matches!(
        worker.write(delayed, 1, 0, b"late", context),
        Err(UploadHubFailure::Hub(UploadHubError::Stale))
    )));
    first.cancellation.cancel();
    assert!(replacement_life.is_active());
    write(&pool, &hub, replacement.key, b"next");
    for key in [second.key, replacement.key] {
        let app = f.app.clone();
        let worker = hub.clone();
        pool.call(move |context| worker.cleanup(key, &app, context))
            .unwrap();
    }
    // Begin's stage can survive after its worker reply is lost/cancelled. The
    // network retains only the original token; a fresh job must recover the
    // opaque key instead of fabricating it or releasing the physical owner.
    let (_, lost_head, lost_path) = f.reserve(4);
    let lost_token = lost_head.token;
    let (control, life) = TransferControl::new(lost_token, Instant::now()).unwrap();
    let (entered, seen) = std::sync::mpsc::channel();
    let (release, held) = std::sync::mpsc::channel();
    let worker = hub.clone();
    let app = f.app.clone();
    let ticket = match pool.0.as_ref().unwrap().handle().try_submit(
        Job(Box::new(move |context| {
            let admission =
                success(worker.begin_material(&app, &lost_head, control, life, context));
            drop(admission); // Its metadata is deliberately not delivered to Net.
            entered.send(()).unwrap();
            let _ = held.recv_timeout(Duration::from_secs(3));
        })),
        Instant::now() + Duration::from_secs(15),
    ) {
        Ok(ticket) => ticket,
        Err(error) => panic!("lost begin worker admission {:?}", error.kind),
    };
    seen.recv_timeout(Duration::from_secs(3)).unwrap();
    ticket.cancel();
    release.send(()).unwrap();
    assert_eq!(ticket.wait(), Err(fc_runtime::worker::JobError::Cancelled));
    assert!(lost_path.exists());
    let worker = hub.clone();
    let recovered = pool
        .call(move |context| worker.key_for_request(lost_token, context))
        .unwrap()
        .expect("original token recovers retained Begin owner");
    assert_eq!(recovered.token(), lost_token);
    let worker = hub.clone();
    let app = f.app.clone();
    pool.call(move |context| worker.cleanup_request(lost_token, &app, context))
        .unwrap();
    assert!(!lost_path.exists());
    pool.join().unwrap();
    assert!(owner.drain_after_join(&f.app).is_drained());
    drop(owner);
}

#[test]
fn abandoned_owner_and_worker_panic_retain_exact_domain_until_join_and_explicit_final_cleanup() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (owner, hub) = UploadHubOwner::new().unwrap();
    let (_, head, path) = f.reserve(4);
    let (admission, stable) = begin(&pool, &hub, &f, head);
    let key = admission.key;
    let (entered, seen) = std::sync::mpsc::channel();
    let (release, held) = std::sync::mpsc::channel();
    let worker = hub.clone();
    let ticket = match pool.0.as_ref().unwrap().handle().try_submit(
        Job(Box::new(move |context| {
            let shared = worker_shared(&worker, context, true).unwrap();
            let _lease = shared.take(key).unwrap();
            entered.send(std::thread::current().id()).unwrap();
            let _ = held.recv_timeout(Duration::from_secs(3));
            panic!("diagnostic upload operation unwind");
        })),
        Instant::now() + Duration::from_secs(15),
    ) {
        Ok(ticket) => ticket,
        Err(error) => panic!("worker admission {:?}", error.kind),
    };
    let actual_worker = seen.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_ne!(actual_worker, std::thread::current().id());
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.key_for_request(key.token(), context))
            .unwrap(),
        Some(key)
    );
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.ready_material_key(key.upload_id(), context)),
        Err(UploadHubError::Busy)
    );
    admission.cancellation.cancel();
    let worker = hub.clone();
    let app = f.app.clone();
    let busy = pool
        .call(move |context| worker.reap_expired(&app, context))
        .unwrap();
    assert_eq!(busy.pending_slots, 1, "Busy owner is never stolen for reap");
    assert!(path.exists());
    drop(owner);
    drop(hub);
    assert!(
        !stable.is_active(),
        "Busy slot's weak cell is cancelled without touching its stage"
    );
    assert!(matches!(UploadHubOwner::new(), Err(UploadHubError::Busy)));
    assert!(path.exists());
    assert!(
        OpenOptions::new().write(true).open(&path).is_err(),
        "Net weak drops do not close held native owner"
    );
    release.send(()).unwrap();
    assert_eq!(ticket.wait(), Err(fc_runtime::worker::JobError::Panicked));
    assert_eq!(pool.join(), Err(fc_runtime::worker::JoinError::Faulted));
    let mut recovered = UploadHubOwner::recover_retained().expect("same original bounded owner");
    let cleanup = recovered.drain_after_join(&f.app);
    assert!(cleanup.is_drained(), "{cleanup:?}");
    assert!(!path.exists());
    drop(recovered);
    let (mut next, _) = UploadHubOwner::new().expect("only actual retirement permits next owner");
    assert!(next.drain_after_join(&f.app).is_drained());
    drop(next);
}

#[test]
fn same_token_with_different_control_cell_is_rejected_before_stage_admission() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let (_, head, path) = f.reserve(4);
    let retry_head = head.clone();
    let (wrong_control, wrong_life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let (right_control, right_life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let app = f.app.clone();
    let worker = hub.clone();
    assert!(pool.call(move |context| matches!(
        worker.begin_material(&app, &head, wrong_control, right_life, context),
        Err(UploadHubFailure::Hub(UploadHubError::Invalid))
    )));
    drop(right_control);
    assert!(!wrong_life.is_active());
    assert!(
        path.exists(),
        "a rejected correlation cannot cancel another Waiting descriptor"
    );
    let mut unauthenticated = retry_head.clone();
    unauthenticated.token = TokenIssuer::new().unwrap().issue(0).unwrap();
    unauthenticated
        .headers
        .retain(|header| !header.name.eq_ignore_ascii_case("cookie"));
    let (unauthed_control, unauthed_life) =
        TransferControl::new(unauthenticated.token, Instant::now()).unwrap();
    let app = f.app.clone();
    let worker = hub.clone();
    assert!(pool.call(move |context| matches!(
        worker.begin_material(
            &app,
            &unauthenticated,
            unauthed_control,
            unauthed_life,
            context
        ),
        Err(UploadHubFailure::Response(_))
    )));
    assert!(
        path.exists(),
        "failed authentication leaves Unbound stage intact"
    );
    let (admitted, _) = begin(&pool, &hub, &f, retry_head);
    let key = admitted.key;
    let app = f.app.clone();
    let worker = hub.clone();
    pool.call(move |context| worker.cleanup(key, &app, context))
        .unwrap();
    assert!(!path.exists());
    pool.join().unwrap();
    assert!(owner.drain_after_join(&f.app).is_drained());
    drop(owner);
}

#[test]
fn finished_response_hands_original_memory_control_to_net_until_response_retires() {
    let _physical = crate::web_application::upload_test_guard();
    let directory = Directory::new();
    let clock = Arc::new(Time(AtomicU64::new(1000)));
    let mut backend = Backend::open(
        directory.0.join("state.sqlite3"),
        CredentialRecordCodec::from_key([66; 32]).unwrap(),
        clock.clone(),
        clock,
    )
    .unwrap();
    let app = WebApplication::new(
        &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
        backend.web_service(),
    )
    .unwrap();
    let mut pool = Pool::new();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let token = TokenIssuer::new().unwrap().issue(0).unwrap();
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let original = life.clone();
    let worker = hub.clone();
    // Exercise the exact production response handoff once the physical stage
    // is retired. This is a memory-lifecycle regression, not a fake WAV publish.
    let finished = pool.call(move |context| {
        let shared = worker_shared(&worker, context, true).unwrap();
        let mut lease = shared
            .reserve(control, life, UploadKind::Voice, 44)
            .unwrap();
        lease.entry().stage = Stage::Complete;
        finished_response(
            lease.entry(),
            WebResponse {
                head: fc_ports::http::ResponseHead {
                    status: 201,
                    headers: vec![],
                    content_length: 0,
                },
                body: vec![],
            },
        )
    });
    assert!(
        original.is_active(),
        "physical retirement cannot cancel the unsent 201"
    );
    let returned = finished
        .network_control
        .expect("same original control moves to Net");
    assert!(Arc::ptr_eq(&returned.cancelled, &original.cancelled));
    drop(returned);
    assert!(
        !original.is_active(),
        "only actual Net retirement cancels this source"
    );
    pool.join().unwrap();
    assert!(owner.drain_after_join(&app).is_drained());
    drop(owner);
    backend.close().unwrap();
}

#[test]
fn live_stop_drains_material_ready_and_live_download_pin_without_joining_shared_workers() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (download_head, download_path, _) =
        crate::web_transfer::downloads::tests::published(&mut f, &pool);
    let (mut upload_owner, hub) = UploadHubOwner::new().unwrap();
    let (mut download_owner, downloads) = crate::web_transfer::DownloadHubOwner::new().unwrap();
    let (id, head, staged) = f.reserve(4);
    let (admitted, life) = ready(&pool, &hub, &f, head, b"live");
    assert!(staged.exists());
    assert!(f.app.material_transfer_pending(admitted.key.token(), id));
    assert!(OpenOptions::new().write(true).open(&staged).is_err());
    let (control, download_life) =
        TransferControl::new(download_head.token, Instant::now()).unwrap();
    let worker = downloads.clone();
    let app = f.app.clone();
    let backend = f.backend.clone();
    pool.call(move |context| {
        assert!(worker
            .begin(&app, &backend, &download_head, download_life, context)
            .is_ok());
    });
    assert!(OpenOptions::new().write(true).open(&download_path).is_err());
    // A second real Begin outlives suppression of its worker reply. Its
    // physical stage and permit remain charged until this live stop cleanup.
    let (_, lost_head, lost_path) = f.reserve(4);
    let (lost_control, lost_life) = TransferControl::new(lost_head.token, Instant::now()).unwrap();
    let (entered, seen) = std::sync::mpsc::sync_channel(1);
    let (release, held) = std::sync::mpsc::sync_channel(1);
    let worker = hub.clone();
    let app = f.app.clone();
    let ticket = match pool.0.as_ref().unwrap().handle().try_submit(
        Job(Box::new(move |context| {
            let admitted =
                success(worker.begin_material(&app, &lost_head, lost_control, lost_life, context));
            drop(admitted);
            entered.send(()).unwrap();
            held.recv_timeout(Duration::from_secs(3)).unwrap();
        })),
        Instant::now() + Duration::from_secs(15),
    ) {
        Ok(ticket) => ticket,
        Err(error) => panic!("suppressed Begin admission {:?}", error.kind),
    };
    seen.recv_timeout(Duration::from_secs(3)).unwrap();
    ticket.cancel();
    release.send(()).unwrap();
    assert_eq!(ticket.wait(), Err(fc_runtime::worker::JobError::Cancelled));
    assert!(lost_path.exists());
    let service = f.backend.lock().unwrap().web_service();
    service.negative_runtime_stop();
    assert!(service.settle_runtime_stop());
    hub.request_domain_stop();
    downloads.request_domain_stop();
    let worker = hub.clone();
    let download = downloads.clone();
    let app = f.app.clone();
    let status = pool.call(move |context| {
        crate::web_transfer::live_cleanup::CleanupTask::new(
            worker,
            download,
            app,
            context.deadline(),
        )
        .run(context)
    });
    assert!(status.is_drained(), "{status:?}");
    assert!(!life.is_active());
    assert!(!staged.exists());
    assert!(!lost_path.exists());
    assert!(!f.app.material_transfer_pending(admitted.key.token(), id));
    assert_eq!(
        f.app.material_transfer_binding(id, &life),
        MaterialTransferBinding::Absent
    );
    drop(OpenOptions::new().write(true).open(&download_path).unwrap());
    assert!(upload_owner.confirm_live_drained());
    assert!(download_owner.confirm_live_drained());
    drop(upload_owner);
    drop(download_owner);
    drop(control);
    // The very same original pool accepts further work: live cleanup did not
    // borrow the final-coordinator after-join shortcut or terminate a worker.
    assert!(pool.call(|context| context.checkpoint().is_ok()));
    let (mut next_owner, next) = UploadHubOwner::new().unwrap();
    let old = hub.clone();
    assert_eq!(
        pool.call(move |context| old.ready_material_key(id, context)),
        Err(UploadHubError::Closed)
    );
    next.request_domain_stop();
    assert!(next_owner.confirm_live_drained());
    drop(next_owner);
    pool.join().unwrap();
}
