//! Real file pins, SQLite receipts and original/current worker lifetimes.
use super::*;
use crate::web_transfer::{uploads::tests::Fixture, TransferControl};
use fc_runtime::worker::{JobTicket, Task, WorkerPool};
use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

type Call = Box<dyn FnOnce(&JobContext) + Send>;
struct Job(Call);
impl Task for Job {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(CHUNK * 3)
    }
    fn run(self, context: &JobContext) {
        (self.0)(context);
    }
}
struct Pool(Option<WorkerPool<Job>>);
impl Pool {
    fn new() -> Self {
        Self(Some(WorkerPool::start().unwrap()))
    }
    fn submit(&self, call: impl FnOnce(&JobContext) + Send + 'static) -> JobTicket<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let handle = self.0.as_ref().unwrap().handle();
        let mut job = Job(Box::new(call));
        loop {
            match handle.try_submit(job, deadline) {
                Ok(ticket) => return ticket,
                Err(error)
                    if error.kind == fc_runtime::worker::Rejection::Busy
                        && Instant::now() < deadline =>
                {
                    // Busy did not admit or execute this exact closure. Keep
                    // the returned task and its original absolute deadline.
                    job = error.task;
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("download worker rejected {:?}", error.kind),
            }
        }
    }
    fn call<T: Send + 'static>(&self, call: impl FnOnce(&JobContext) -> T + Send + 'static) -> T {
        let result = Arc::new(Mutex::new(None));
        let output = result.clone();
        self.submit(move |ctx| *output.lock().unwrap() = Some(call(ctx)))
            .wait()
            .unwrap();
        let value = result.lock().unwrap().take().unwrap();
        value
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        self.0
            .take()
            .unwrap()
            .join()
            .expect("download workers actually joined");
    }
}
fn field(body: &[u8], name: &str) -> String {
    std::str::from_utf8(body)
        .unwrap()
        .split_once(&format!("\"{name}\":\""))
        .unwrap()
        .1
        .split('"')
        .next()
        .unwrap()
        .to_owned()
}
fn published(f: &mut Fixture, pool: &Pool) -> (RequestHead, PathBuf, Vec<u8>) {
    let bytes = b"Synthetic exact download data\n".repeat(5000);
    let (id, head, _) = f.reserve(bytes.len());
    let app = f.app.clone();
    let source = bytes.clone();
    let response = pool.call(move |_| app.dispatch_material_upload(&head, Cursor::new(source)));
    assert_eq!(response.head.status, 200);
    let (head, body) = f.append(id, 98201);
    let app = f.app.clone();
    let backend = f.backend.clone();
    let response = pool.call(move |_| app.dispatch_material_append(&backend, &head, &body));
    assert_eq!(
        response.head.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    let material_id = field(&response.body, "materialId");
    let mut head = f.head(&format!("materials/download/1/1/{material_id}/{id}"), 0);
    head.method = "GET".into();
    head.content_length = None;
    head.has_body = false;
    head.headers.retain(|h| {
        !h.name.eq_ignore_ascii_case("Content-Type")
            && !h.name.eq_ignore_ascii_case("Content-Length")
    });
    let files = f
        .app
        .material_files
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .files
        .clone();
    let path = files
        .directory(&[
            "teachers".into(),
            "1".into(),
            "subjects".into(),
            "1".into(),
            material_id,
        ])
        .unwrap()
        .join(format!("{id}.txt"));
    (head, path, bytes)
}
type Owner = Arc<Mutex<Option<MaterialDownload>>>;
fn open(pool: &Pool, f: &Fixture, head: RequestHead, life: TransferLifetime) -> Owner {
    let owner = Arc::new(Mutex::new(None));
    let output = owner.clone();
    let app = f.app.clone();
    let backend = f.backend.clone();
    pool.call(move |context| {
        let download = match app.begin_material_download_in_job(&backend, &head, &life, context) {
            Ok(download) => download,
            Err(response) => panic!("download begin {}", response.head.status),
        };
        *output.lock().unwrap() = Some(download);
    });
    owner
}
fn close(pool: &Pool, owner: Owner) {
    pool.call(move |_| drop(owner.lock().unwrap().take()));
}
#[test]
fn network_download_preserves_original_deadline_across_distinct_worker_tickets() {
    let _physical = super::super::shared_upload::test_guard();
    let mut fixture = Fixture::new();
    let pool = Pool::new();
    let (head, path, bytes) = published(&mut fixture, &pool);
    let admitted = Instant::now() - Duration::from_secs(40);
    let (control, life) = TransferControl::new(head.token, admitted).unwrap();
    let owner = open(&pool, &fixture, head, life.clone());
    assert_eq!(
        owner.lock().unwrap().as_ref().unwrap().expires,
        life.deadline()
    );
    assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
    let fence = owner.lock().unwrap().as_ref().unwrap().send_fence();
    assert!(
        fence(),
        "dropping begin's ticket cannot poison the request-wide send fence"
    );
    let mut actual = Vec::new();
    loop {
        let current = owner.clone();
        let life = life.clone();
        // Backend is deliberately locked while long hash/read executes. The
        // current worker must make progress without that unrelated mutex.
        let held = fixture.backend.lock().unwrap();
        let chunk = pool.call(move |context| {
            current
                .lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .read_chunk_in_job(&life, context)
                .unwrap()
                .to_vec()
        });
        drop(held);
        if chunk.is_empty() {
            break;
        }
        assert!(chunk.len() <= CHUNK);
        actual.extend_from_slice(&chunk);
    }
    assert_eq!(actual, bytes);
    control.cancel();
    assert!(!fence());
    let current = owner.clone();
    assert!(pool.call(move |context| current
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .read_chunk_in_job(&life, context)
        .is_err()));
    close(&pool, owner);
    assert!(std::fs::OpenOptions::new().read(true).open(&path).is_ok());
}
#[test]
fn expired_or_foreign_download_lifetime_cannot_open_or_rebind_the_original_pin() {
    let _physical = super::super::shared_upload::test_guard();
    let mut fixture = Fixture::new();
    let pool = Pool::new();
    let (head, _, _) = published(&mut fixture, &pool);
    let (expired_control, expired) =
        TransferControl::new(head.token, Instant::now() - STREAM_TIME).unwrap();
    let app = fixture.app.clone();
    let backend = fixture.backend.clone();
    let request = head.clone();
    assert_eq!(
        pool.call(move |context| app
            .begin_material_download_in_job(&backend, &request, &expired, context)
            .err()
            .unwrap()
            .head
            .status),
        503
    );
    drop(expired_control);
    let (control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let owner = open(&pool, &fixture, head.clone(), life);
    let (foreign_control, foreign) = TransferControl::new(head.token, Instant::now()).unwrap();
    let current = owner.clone();
    assert!(pool.call(move |context| current
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .read_chunk_in_job(&foreign, context)
        .is_err()));
    close(&pool, owner);
    drop((control, foreign_control));
}
#[test]
fn current_job_cancel_during_real_hash_keeps_the_same_file_pin_until_worker_cleanup() {
    let _physical = super::super::shared_upload::test_guard();
    let mut fixture = Fixture::new();
    let pool = Pool::new();
    let (head, path, _) = published(&mut fixture, &pool);
    let (control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let owner = open(&pool, &fixture, head, life);
    let identity = owner
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .file
        .identity()
        .unwrap();
    let (entered, observe) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    let failed = Arc::new(AtomicBool::new(false));
    let result = failed.clone();
    let current = owner.clone();
    let ticket = pool.submit(move |context| {
        let checks = std::cell::Cell::new(0);
        let fence = || {
            let n = checks.get() + 1;
            checks.set(n);
            if n == 4 {
                entered.send(()).unwrap();
                resume.recv().unwrap();
            }
            context.checkpoint().is_ok()
        };
        // The same helper used by read_chunk_in_job; block precisely at the
        // control check AFTER the first real 32KiB hash read, then cancel this
        // actual ticket. No fake file reader or native release acknowledgment.
        let mut stored = current.lock().unwrap();
        let download = stored.as_mut().unwrap();
        result.store(
            download.read_chunk_checked(Some(&fence)).is_err() && !download.verified,
            Ordering::Release,
        );
    });
    observe.recv_timeout(Duration::from_secs(5)).unwrap();
    ticket.cancel();
    release.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while !ticket.is_finished() {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    assert!(failed.load(Ordering::Acquire));
    assert_eq!(
        owner
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .file
            .identity()
            .unwrap(),
        identity
    );
    assert!(
        std::fs::OpenOptions::new().write(true).open(&path).is_err(),
        "cancel is not physical pin release"
    );
    close(&pool, owner);
    drop(control);
}
