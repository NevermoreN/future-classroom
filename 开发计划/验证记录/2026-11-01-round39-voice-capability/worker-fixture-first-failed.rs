//! The production WebTask route with real Cookie authentication/SQLite and an
//! injected read-only driver result. This is not a Windows-device runtime claim.
use super::*;
use crate::web_transfer::{TransferControl, WebCompletion, WebTask};
use fc_ports::audio::{AudioOutputError, DefaultOutputSupport};
use fc_runtime::worker::{JobContext, Rejection, Task, WorkerPool};
use std::{sync::Mutex, time::Instant};

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
    let mut f = fixture();
    catalog(&mut f);
    let (cookie, _, _) = auth(&mut f, "admin", "Synthetic capability browser");
    let BackendFixture {
        backend,
        app,
        service: _,
        admin: _,
        clock,
        directory,
    } = f;
    let backend = Arc::new(Mutex::new(backend));
    let app = Arc::new(app);
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
    backend.lock().unwrap().close().unwrap();
    drop(backend);
    drop(app);
    drop(directory);
}
