//! The production WebTask route with real Cookie authentication/SQLite and an
//! injected read-only driver result. This is not a Windows-device runtime claim.
use super::*;
use crate::web_transfer::{TransferControl, WebCompletion, WebTask};
use fc_ports::audio::{AudioOutputError, DefaultOutputSupport};
use fc_runtime::worker::{JobContext, Rejection, Task, WorkerPool};
use std::{sync::Mutex, time::Instant};

/// This worker fixture owns the actual Backend rather than moving fields out
/// of the parent's Drop implementation. The last field is removed only after
/// the shared workers have joined and Drop has physically closed the writer.
struct SharedFixture {
    backend: Arc<Mutex<Backend>>,
    app: Arc<WebApplication>,
    clock: Arc<Time>,
    cookie: String,
    _directory: Directory,
}
impl SharedFixture {
    fn new() -> Self {
        #[cfg(target_os = "linux")]
        let root = std::path::PathBuf::from("/dev/shm");
        #[cfg(not(target_os = "linux"))]
        let root = std::env::temp_dir();
        let directory = Directory(root.join(format!(
            "FutureClassroom-Web-Voice-Capability-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
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
        for id in [1, 2] {
            backend
                .identity(
                    admin,
                    IdentityCommand::CreateClass {
                        id: class(id),
                        name: format!("Synthetic voice class {id}"),
                    },
                )
                .unwrap();
        }
        backend
            .identity(admin, IdentityCommand::BindCurrentClass { id: class(1) })
            .unwrap();
        let service = backend.web_service();
        service.set_enabled(admin, true).unwrap();
        let app = WebApplication::new(
            &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
            service.clone(),
        )
        .unwrap();
        let window = service.begin_pairing(admin).unwrap();
        let code = window.code.expose_text();
        let body = format!(
            r#"{{"schemaVersion":1,"code":"{}","loginName":"admin","deviceName":"Synthetic capability browser"}}"#,
            code.as_str()
        );
        let created = app.dispatch(
            &mut backend,
            &head("POST", "pairing/requests", body.len(), None, None),
            body.as_bytes(),
        );
        assert_eq!(created.head.status, 202);
        let id: u64 = field(&created.body, "requestId").parse().unwrap();
        let secret = field(&created.body, "pollSecret");
        let review = service
            .pending(admin)
            .unwrap()
            .into_iter()
            .find(|r| r.request_id == id)
            .unwrap();
        service
            .approve(admin, id, review.identity_revision)
            .unwrap();
        let poll = format!(r#"{{"schemaVersion":1,"pollSecret":"{secret}"}}"#);
        let authorized = app.dispatch(
            &mut backend,
            &head(
                "POST",
                &format!("pairing/requests/{id}/poll"),
                poll.len(),
                None,
                None,
            ),
            poll.as_bytes(),
        );
        assert_eq!(authorized.head.status, 200);
        assert_eq!(
            text(&authorized.body),
            r#"{"schemaVersion":1,"status":"AUTHORIZED"}"#
        );
        assert!(!text(&authorized.body).contains(&secret));
        let cookie = text(
            &authorized
                .head
                .headers
                .iter()
                .find(|h| h.name == "Set-Cookie")
                .unwrap()
                .value,
        );
        for flag in ["Path=/", "Secure", "HttpOnly", "SameSite=Strict"] {
            assert!(cookie.contains(flag));
        }
        let cookie = cookie.split(';').next().unwrap().to_owned();
        let session = app.dispatch(
            &mut backend,
            &head("GET", "session", 0, Some(&cookie), None),
            b"",
        );
        assert_eq!(session.head.status, 200);
        let _csrf = field(&session.body, "csrf");
        let proof = cookie_secret(cookie.as_bytes()).unwrap();
        let _lease = service.authenticate(&proof).unwrap().lease();
        Self {
            backend: Arc::new(Mutex::new(backend)),
            app: Arc::new(app),
            clock,
            cookie,
            _directory: directory,
        }
    }
}
impl Drop for SharedFixture {
    fn drop(&mut self) {
        self.backend
            .lock()
            .unwrap()
            .close()
            .expect("voice capability fixture writer shutdown");
    }
}

struct QueryTask {
    task: WebTask,
    query: Box<dyn FnOnce() -> Result<DefaultOutputSupport, AudioOutputError> + Send>,
}
impl Task for QueryTask {
    type Output = WebCompletion;
    fn allocation_bytes(&self) -> Option<usize> {
        self.task.allocation_bytes()
    }
    fn run(self, context: &JobContext) -> WebCompletion {
        self.task.run_with_output_query(context, self.query)
    }
}
fn dispatch(
    pool: &WorkerPool<QueryTask>,
    backend: &Arc<Mutex<Backend>>,
    app: &Arc<WebApplication>,
    head: RequestHead,
    query: impl FnOnce() -> Result<DefaultOutputSupport, AudioOutputError> + Send + 'static,
) -> WebResponse {
    let token = head.token;
    let (_control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let task = WebTask::json(backend, app, head, vec![], life).unwrap();
    let deadline = task.deadline();
    let mut task = QueryTask {
        task,
        query: Box::new(query),
    };
    let ticket = loop {
        match pool.handle().try_submit(task, deadline) {
            Ok(ticket) => break ticket,
            Err(error) if error.kind == Rejection::Busy && Instant::now() < deadline => {
                task = error.task;
                std::thread::yield_now();
            }
            Err(error) => panic!("capability task rejected {:?}", error.kind),
        }
    };
    match ticket.wait().unwrap() {
        WebCompletion::Response {
            token: actual,
            response,
        } => {
            assert_eq!(actual, token);
            response.head.validate().unwrap();
            assert_eq!(response.head.content_length, response.body.len() as u64);
            response
        }
        _ => panic!("expected correlated JSON response"),
    }
}

#[test]
fn actual_web_worker_releases_backend_for_query_authenticates_cache_hits_and_never_claims_available(
) {
    let f = SharedFixture::new();
    let backend = f.backend.clone();
    let app = f.app.clone();
    let clock = f.clock.clone();
    let cookie = f.cookie.clone();
    let pool = WorkerPool::start().unwrap();
    let query_backend = backend.clone();
    let negative = dispatch(
        &pool,
        &backend,
        &app,
        head("GET", "voice/capabilities", 0, Some(&cookie), None),
        move || {
            assert!(
                query_backend.try_lock().is_ok(),
                "native query must release outer Backend mutex"
            );
            Ok(DefaultOutputSupport::Unavailable)
        },
    );
    expect(&negative, 200, "\"deviceAvailability\":\"UNAVAILABLE\"");
    let cached = dispatch(
        &pool,
        &backend,
        &app,
        head("GET", "voice/capabilities", 0, Some(&cookie), None),
        || panic!("cache hit must not repeat native query"),
    );
    expect(&cached, 200, "\"deviceAvailability\":\"UNAVAILABLE\"");
    let unauthenticated = dispatch(
        &pool,
        &backend,
        &app,
        head("GET", "voice/capabilities", 0, None, None),
        || panic!("cache cannot replace exact authentication"),
    );
    assert_eq!(unauthenticated.head.status, 401);
    let mut foreign_host = head("GET", "voice/capabilities", 0, Some(&cookie), None);
    foreign_host
        .headers
        .iter_mut()
        .find(|h| h.name == "Host")
        .unwrap()
        .value = b"foreign.school".to_vec();
    let rejected = dispatch(&pool, &backend, &app, foreign_host, || {
        panic!("foreign Host must not query")
    });
    assert_eq!(rejected.head.status, 403);
    // Exact monotonic expiry is not renewed by either successful cache delivery.
    clock.0.store(31_000, Ordering::Release);
    let unknown = dispatch(
        &pool,
        &backend,
        &app,
        head("GET", "voice/capabilities", 0, Some(&cookie), None),
        || Ok(DefaultOutputSupport::Supported),
    );
    expect(&unknown, 200, "\"deviceAvailability\":\"UNVERIFIED\"");
    assert!(!text(&unknown.body).contains("\"AVAILABLE\""));
    pool.request_stop();
    pool.join().unwrap();
    drop(backend);
    drop(app);
    drop(clock);
    drop(f);
}
