//! Actual Windows pinned files on the existing bounded worker implementation.
use super::*;
use fc_ports::http::{Header, TokenIssuer};
use fc_runtime::worker::{JobTicket, Rejection, Task, WorkerPool};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const LARGE: &str = "e3c33f1a7c00a23610a13fa6b862df2882931be5b0262aeada7a47e282f1c679";
struct Package(PathBuf);
impl Package {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Static-Loader-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("assets")).unwrap();
        fs::write(root.join("index.html"), b"abc").unwrap();
        fs::write(
            root.join("assets/app-01234567.js"),
            vec![b'x'; 2 * CHUNK_BYTES + 17],
        )
        .unwrap();
        let raw = format!(
            r#"{{"schemaVersion":1,"apiVersion":1,"uiCompatibilityVersion":1,"assets":[{{"path":"index.html","sizeBytes":3,"sha256":"{ABC}"}},{{"path":"assets/app-01234567.js","sizeBytes":131089,"sha256":"{LARGE}"}}]}}"#
        );
        fs::write(root.join("fc-assets.json"), raw).unwrap();
        Self(root)
    }
}
impl Drop for Package {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("owned static fixture cleanup");
    }
}
type Call = Box<dyn FnOnce(&JobContext) + Send>;
struct Job(Call);
impl Task for Job {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(4096)
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
        let mut job = Job(Box::new(call));
        loop {
            match self.0.as_ref().unwrap().handle().try_submit(job, deadline) {
                Ok(ticket) => return ticket,
                Err(error) if error.kind == Rejection::Busy && Instant::now() < deadline => {
                    job = error.task;
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("static worker admission {:?}", error.kind),
            }
        }
    }
    fn call<T: Send + 'static>(&self, call: impl FnOnce(&JobContext) -> T + Send + 'static) -> T {
        let result = Arc::new(Mutex::new(None));
        let output = result.clone();
        self.submit(move |context| *output.lock().unwrap() = Some(call(context)))
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
            .expect("static workers actually joined");
    }
}
fn prefix() -> ListenPrefix {
    ListenPrefix::https("https://board.synthetic:443/fc/").unwrap()
}
#[test]
fn real_package_load_pins_one_handle_and_returns_only_verified_shared_bytes() {
    let package = Package::new();
    let pool = Pool::new();
    fs::write(
        package.0.join("undeclared-private.db"),
        b"synthetic undeclared bytes",
    )
    .unwrap();
    let root = package.0.clone();
    let assets =
        pool.call(move |context| StaticAssets::load_in_job(&prefix(), &root, context).unwrap());
    assert_eq!(assets.asset_count(), 2);
    assert_eq!(
        fs::read(package.0.join("undeclared-private.db")).unwrap(),
        b"synthetic undeclared bytes"
    );
    // The worker released every native file/parent pin before this immutable
    // result was delivered; replacing the source cannot mutate the served copy.
    fs::write(package.0.join("index.html"), b"abd").unwrap();
    let head = RequestHead {
        token: TokenIssuer::new().unwrap().issue(0).unwrap(),
        method: "GET".into(),
        target: "/fc/".into(),
        headers: vec![Header {
            name: "Host".into(),
            value: b"board.synthetic".to_vec(),
        }],
        tls: true,
        content_length: None,
        has_body: false,
    };
    let mut response = assets.dispatch(&head, b"").unwrap();
    assert_eq!(response.body.next_chunk(), Some(b"abc".as_slice()));
    let mut unlisted = head;
    unlisted.target = "/fc/undeclared-private.db".into();
    assert!(matches!(
        assets.dispatch(&unlisted, b""),
        Err(StaticError::Boundary(WebError::NotFound))
    ));
}
#[test]
fn missing_corrupt_alias_and_oversized_manifests_or_payloads_never_load() {
    let pool = Pool::new();
    for kind in 0..7 {
        let package = Package::new();
        match kind {
            0 => fs::remove_file(package.0.join("index.html")).unwrap(),
            1 => fs::write(package.0.join("index.html"), b"abd").unwrap(),
            2 => fs::write(package.0.join("index.html"), b"abcd").unwrap(),
            3 => fs::write(
                package.0.join("fc-assets.json"),
                vec![b' '; fc_protocol::MAX_JSON_BYTES + 1],
            )
            .unwrap(),
            4 => {
                let path = package.0.join("fc-assets.json");
                let raw = fs::read_to_string(&path)
                    .unwrap()
                    .replace("\"apiVersion\":1", "\"apiVersion\":2");
                fs::write(path, raw).unwrap();
            }
            5 => {
                let path = package.0.join("fc-assets.json");
                let raw = fs::read_to_string(&path)
                    .unwrap()
                    .replace("assets/app-01234567.js", "../secret.js");
                fs::write(path, raw).unwrap();
            }
            6 => {
                fs::remove_file(package.0.join("index.html")).unwrap();
                fs::hard_link(
                    package.0.join("assets/app-01234567.js"),
                    package.0.join("index.html"),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let root = package.0.clone();
        let expected = match kind {
            1 => StaticLoadError::Assets(StaticError::PayloadMismatch),
            4 | 5 => StaticLoadError::Assets(StaticError::InvalidManifest),
            _ => StaticLoadError::FileUnavailable,
        };
        assert_eq!(
            pool.call(
                move |context| StaticAssets::load_in_job(&prefix(), &root, context)
                    .err()
                    .unwrap()
            ),
            expected,
            "invalid package {kind}"
        );
    }
    assert_eq!(
        pool.call(move |context| StaticAssets::load_in_job(
            &prefix(),
            Path::new("relative-package"),
            context
        )
        .err()
        .unwrap()),
        StaticLoadError::InvalidRoot
    );
}
#[test]
fn current_job_cancel_between_real_reads_keeps_pin_until_its_worker_releases_it() {
    let package = Package::new();
    let pool = Pool::new();
    let path = package.0.join("assets/app-01234567.js");
    let (entered, observed) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    let verified = Arc::new(AtomicBool::new(false));
    let done = verified.clone();
    let ticket = pool.submit(move |context| {
        let mut pin = LockedFile::open(&path, 131089).unwrap();
        let identity = pin.identity().unwrap();
        let count = std::cell::Cell::new(0);
        let current = || {
            let n = count.get() + 1;
            count.set(n);
            if n == 3 {
                entered.send(()).unwrap();
                resume.recv().unwrap();
            }
            checkpoint(context)
        };
        let error = read_pin_checked(&mut pin, 131089, Some(131089), &current)
            .err()
            .unwrap();
        assert_eq!(error, StaticLoadError::Cancelled);
        assert_eq!(pin.identity().unwrap(), identity);
        assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
        drop(pin); // actual close remains on this same original worker
        assert!(fs::OpenOptions::new().write(true).open(&path).is_ok());
        done.store(true, Ordering::Release);
    });
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    ticket.cancel();
    release.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while !ticket.is_finished() {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    assert!(verified.load(Ordering::Acquire));
}
