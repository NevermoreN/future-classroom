use super::*;
use crate::web_transfer::{HttpCompletion, HttpExecutor, HttpJob, TransferControl};
use fc_domain::time::{LocalDate, LocalDateTime};
use fc_ports::{
    http::{Header, ListenPrefix, TokenIssuer},
    Clock, MonotonicClock,
};
use fc_runtime::worker::{Rejection, WorkerPool, QUEUED_BYTES};
use fc_security::CredentialRecordCodec;
use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
struct Fixture {
    backend: Arc<Mutex<Backend>>,
    app: Arc<WebApplication>,
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "FutureClassroom-Web-Worker-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let backend = Backend::open(
            directory.join("state.sqlite3"),
            CredentialRecordCodec::from_key([93; 32]).unwrap(),
            Arc::new(Time),
            Arc::new(Time),
        )
        .unwrap();
        let app = Arc::new(
            WebApplication::new(
                &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
                backend.web_service(),
            )
            .unwrap(),
        );
        Self {
            backend: Arc::new(Mutex::new(backend)),
            app,
            directory,
        }
    }
    fn head(&self) -> RequestHead {
        RequestHead {
            token: TokenIssuer::new().unwrap().issue(0).unwrap(),
            method: "GET".into(),
            target: "/fc/api/v1/status".into(),
            headers: vec![Header {
                name: "Host".into(),
                value: b"board.school".to_vec(),
            }],
            tls: true,
            content_length: None,
            has_body: false,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.backend.lock().unwrap().close().unwrap();
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}
fn run(pool: &WorkerPool<HttpJob>, task: WebTask) -> WebCompletion {
    let task = HttpJob::Web(Box::new(task));
    let ticket = match pool.handle().try_submit_http(task) {
        Ok(t) => t,
        Err(e) => panic!("unexpected {:?}", e.kind),
    };
    match ticket.wait() {
        Ok(HttpCompletion::Web(v)) => v,
        #[cfg(windows)]
        Ok(HttpCompletion::Upload(_)) => panic!("wrong job correlation"),
        #[cfg(windows)]
        Ok(HttpCompletion::Download(_)) => panic!("wrong job correlation"),
        Err(e) => panic!("unexpected {e:?}"),
    }
}
fn status(completed: WebCompletion, token: RequestToken) -> u16 {
    match completed {
        WebCompletion::Response {
            token: actual,
            response,
        } => {
            assert_eq!(actual, token);
            response.head.validate().unwrap();
            assert_eq!(response.body.len() as u64, response.head.content_length);
            response.head.status
        }
        _ => panic!("expected response"),
    }
}

#[test]
fn capability_head_and_json_share_original_hundred_milliseconds_including_queue_time() {
    let fixture = Fixture::new();
    let mut head = fixture.head();
    head.target = "/fc/api/v1/voice/capabilities".into();
    let admitted = Instant::now() - Duration::from_millis(150);
    let (_control, life) = TransferControl::new(head.token, admitted).unwrap();
    assert!(life.is_active()); // Original ordinary 120-second lifetime remains live.
    let first = WebTask::admit(&fixture.backend, &fixture.app, head.clone(), life.clone()).unwrap();
    let second = WebTask::json(
        &fixture.backend,
        &fixture.app,
        head.clone(),
        vec![],
        life.clone(),
    )
    .unwrap();
    assert_eq!(first.deadline(), admitted + Duration::from_millis(100));
    assert_eq!(second.deadline(), first.deadline());
    let pool = WorkerPool::start().unwrap();
    let deadline = second.deadline();
    assert_eq!(
        pool.handle()
            .try_submit(HttpJob::Web(Box::new(second)), deadline)
            .err()
            .unwrap()
            .kind,
        Rejection::Deadline
    );
    let shorter = life.bounded(Duration::from_millis(50)).unwrap();
    let task = WebTask::json(
        &fixture.backend,
        &fixture.app,
        head,
        vec![],
        shorter.clone(),
    )
    .unwrap();
    assert_eq!(task.deadline(), shorter.deadline());
    pool.request_stop();
    pool.join().unwrap();
}

#[test]
fn real_worker_head_then_json_share_original_lifetime_without_creating_another_pool() {
    let fixture = Fixture::new();
    let pool = WorkerPool::start().unwrap();
    let head = fixture.head();
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let deadline = life.deadline();
    let task = WebTask::admit(&fixture.backend, &fixture.app, head.clone(), life.clone()).unwrap();
    match run(&pool, task) {
        WebCompletion::Admitted {
            token: actual,
            route,
        } => {
            assert_eq!(actual, token);
            assert_eq!(route, Route::Status)
        }
        _ => panic!("head admission"),
    }
    assert!(life.is_active());
    let task = WebTask::json(&fixture.backend, &fixture.app, head, vec![], life.clone()).unwrap();
    assert_eq!(task.deadline(), deadline);
    assert_eq!(status(run(&pool, task), token), 200);
    assert!(life.is_active());
    drop(control);
    pool.request_stop();
    pool.join().unwrap();
}

#[test]
fn canceled_or_expired_request_returns_no_positive_body_and_next_request_survives() {
    let fixture = Fixture::new();
    let pool = WorkerPool::start().unwrap();
    let head = fixture.head();
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let task = WebTask::json(&fixture.backend, &fixture.app, head, vec![], life).unwrap();
    control.cancel();
    assert_eq!(status(run(&pool, task), token), 503);
    let head = fixture.head();
    let token = head.token;
    let (_control, life) =
        TransferControl::new(token, Instant::now() - Duration::from_secs(121)).unwrap();
    let task = WebTask::json(&fixture.backend, &fixture.app, head, vec![], life).unwrap();
    // The runtime rejects an expired original queue deadline without running it.
    let deadline = task.deadline();
    assert_eq!(
        pool.handle()
            .try_submit(HttpJob::Web(Box::new(task)), deadline)
            .err()
            .unwrap()
            .kind,
        Rejection::Deadline
    );
    let head = fixture.head();
    let token = head.token;
    let (_control, life) = TransferControl::new(token, Instant::now()).unwrap();
    assert_eq!(
        status(
            run(
                &pool,
                WebTask::json(&fixture.backend, &fixture.app, head, vec![], life).unwrap()
            ),
            token
        ),
        200
    );
    pool.request_stop();
    pool.join().unwrap();
}

#[test]
fn weak_job_cannot_keep_a_native_application_alive_and_unused_capacity_is_charged() {
    let mut fixture = Fixture::new();
    let pool = WorkerPool::start().unwrap();
    let head = fixture.head();
    let token = head.token;
    let (_control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let task = WebTask::json(&fixture.backend, &fixture.app, head, vec![], life).unwrap();
    assert_eq!(Arc::strong_count(&fixture.app), 1);
    let replacement = Arc::new(
        WebApplication::new(
            &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
            fixture.backend.lock().unwrap().web_service(),
        )
        .unwrap(),
    );
    let weak = Arc::downgrade(&fixture.app);
    fixture.app = replacement;
    assert!(weak.upgrade().is_none());
    assert_eq!(status(run(&pool, task), token), 503);
    let head = fixture.head();
    let (_control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let task = WebTask::json(
        &fixture.backend,
        &fixture.app,
        head,
        Vec::with_capacity(QUEUED_BYTES + 1),
        life,
    )
    .unwrap();
    assert!(task.allocation_bytes().unwrap() > QUEUED_BYTES);
    let deadline = task.deadline();
    assert_eq!(
        pool.handle()
            .try_submit(HttpJob::Web(Box::new(task)), deadline)
            .err()
            .unwrap()
            .kind,
        Rejection::TooLarge
    );
    pool.request_stop();
    pool.join().unwrap();
}
