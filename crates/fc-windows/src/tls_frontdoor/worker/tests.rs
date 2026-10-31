use super::*;
use crate::tls_frontdoor::{Call, Step};
use fc_ports::http::TokenIssuer;
use std::{sync::atomic::AtomicUsize, time::Instant};

pub(in crate::tls_frontdoor) static SERIAL: Mutex<()> = Mutex::new(());
struct Immediate;
impl Driver for Immediate {
    fn call(&mut self, frame: &mut Frame) -> Result<Step, TlsError> {
        if frame.call == Call::Retire {
            frame.wipe();
            Ok(Step::Retired)
        } else if frame.call == Call::Shutdown {
            frame.output[0] = 1;
            frame.output_len = 1;
            Ok(Step::Shutdown { complete: true })
        } else {
            frame.output[0] = 42;
            frame.output_len = 1;
            Ok(Step::Encrypted)
        }
    }
}
struct Held {
    entered: SyncSender<()>,
    release: Receiver<()>,
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl Driver for Held {
    fn call(&mut self, _: &mut Frame) -> Result<Step, TlsError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        self.entered.send(()).unwrap();
        self.release.recv().unwrap();
        Ok(Step::Handshake {
            consumed: 0,
            complete: true,
        })
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::AcqRel);
    }
}
struct Release(Option<SyncSender<()>>);
impl Release {
    fn now(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.try_send(());
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.now();
    }
}
fn until(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(
            Instant::now() < deadline,
            "actual worker completion deadline"
        );
        thread::yield_now();
    }
}
fn ready(worker: &mut Worker) -> Result<(), TlsError> {
    let mut result = None;
    until(|| {
        result = worker.poll_ready();
        result.is_some()
    });
    result.unwrap()
}
fn submit(worker: &mut Worker, token: RequestToken) {
    assert!(worker.try_submit(Frame::new(token)).is_ok());
}
fn take(worker: &mut Worker) -> Box<Frame> {
    let mut result = None;
    until(|| {
        result = worker.poll().unwrap();
        result.is_some()
    });
    result.unwrap()
}
fn stop_reap(worker: &mut Worker) {
    worker.request_stop();
    until(|| worker.reap_nonblocking().unwrap());
}
fn immediate(generation: u64) -> Worker {
    Worker::start_with_factory(generation, || Ok(Box::new(Immediate))).unwrap()
}

#[test]
fn initialization_is_owned_nonblocking_and_stop_cannot_destroy_held_factory() {
    let _serial = SERIAL.lock().unwrap();
    let issuer = TokenIssuer::new().unwrap();
    let (entered_send, entered) = mpsc::sync_channel(1);
    let (release_send, release_rx) = mpsc::sync_channel(1);
    let mut release = Release(Some(release_send));
    let mut worker = Worker::start_with_factory(issuer.generation(), move || {
        entered_send.send(()).unwrap();
        release_rx.recv().unwrap();
        Ok(Box::new(Immediate))
    })
    .unwrap();
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(worker.poll_ready().is_none());
    assert!(matches!(
        Worker::start_with_factory(issuer.generation(), || Ok(Box::new(Immediate))),
        Err(TlsError::Busy)
    ));
    worker.request_stop();
    assert_eq!(worker.reap_nonblocking(), Ok(false));
    release.now();
    assert_eq!(ready(&mut worker), Ok(()));
    stop_reap(&mut worker);
}

#[test]
fn readiness_must_be_collected_and_initialization_failure_keeps_its_error() {
    let _serial = SERIAL.lock().unwrap();
    let mut issuer = TokenIssuer::new().unwrap();
    let mut worker = immediate(issuer.generation());
    let token = issuer.issue(0).unwrap();
    let frame = Frame::new(token);
    let address = (&*frame) as *const Frame;
    let (error, frame) = worker.try_submit(frame).err().unwrap();
    assert_eq!(error, TlsError::Busy);
    assert_eq!((&*frame) as *const Frame, address);
    assert_eq!(ready(&mut worker), Ok(()));
    assert!(worker.poll_ready().is_none());
    assert!(worker.try_submit(frame).is_ok());
    assert_eq!(take(&mut worker).result, Some(Ok(Step::Encrypted)));
    stop_reap(&mut worker);
    drop(worker);
    let mut worker =
        Worker::start_with_factory(issuer.generation(), || Err(TlsError::Certificate)).unwrap();
    assert_eq!(ready(&mut worker), Err(TlsError::Certificate));
    assert_eq!(
        worker.try_submit(Frame::new(token)).err().unwrap().0,
        TlsError::Certificate
    );
    stop_reap(&mut worker);
}

#[test]
fn four_linear_permits_survive_stop_until_real_call_and_collection() {
    let _serial = SERIAL.lock().unwrap();
    let mut issuer = TokenIssuer::new().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let (entered_send, entered) = mpsc::sync_channel(1);
    let (release_send, release_rx) = mpsc::sync_channel(1);
    let mut release = Release(Some(release_send));
    let driver = Held {
        entered: entered_send,
        release: release_rx,
        calls: Arc::clone(&calls),
        drops: Arc::clone(&drops),
    };
    // Only the Send factory crosses the boundary; Driver itself need not Send.
    let mut worker =
        Worker::start_with_factory(issuer.generation(), move || Ok(Box::new(driver))).unwrap();
    assert_eq!(ready(&mut worker), Ok(()));
    let first = issuer.issue(0).unwrap();
    submit(&mut worker, first);
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    for slot in 1..CONNECTION_SLOTS {
        submit(&mut worker, issuer.issue(slot).unwrap());
    }
    let frame = Frame::new(issuer.issue(0).unwrap());
    let address = (&*frame) as *const Frame;
    let (error, frame) = worker.try_submit(frame).err().unwrap();
    assert_eq!(error, TlsError::Busy);
    assert_eq!((&*frame) as *const Frame, address);
    worker.request_stop();
    assert!(worker.has_in_flight());
    assert!(worker.poll().unwrap().is_none());
    assert_eq!(worker.reap_nonblocking(), Ok(false));
    assert_eq!(drops.load(Ordering::Acquire), 0);
    release.now();
    let first_result = take(&mut worker);
    assert_eq!(first_result.token, first);
    assert_eq!(
        first_result.result,
        Some(Ok(Step::Handshake {
            consumed: 0,
            complete: true
        }))
    );
    assert!(worker.has_in_flight());
    for _ in 1..CONNECTION_SLOTS {
        assert_eq!(take(&mut worker).result, Some(Err(TlsError::Stopped)));
    }
    assert!(!worker.has_in_flight());
    stop_reap(&mut worker);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(drops.load(Ordering::Acquire), 1);
}

#[test]
fn stale_generation_and_serial_cannot_rebind_an_existing_slot() {
    let _serial = SERIAL.lock().unwrap();
    let mut issuer = TokenIssuer::new().unwrap();
    let mut other = TokenIssuer::new().unwrap();
    let mut worker = immediate(issuer.generation());
    assert_eq!(ready(&mut worker), Ok(()));
    assert_eq!(
        worker
            .try_submit(Frame::new(other.issue(0).unwrap()))
            .err()
            .unwrap()
            .0,
        TlsError::InvalidInput
    );
    let old = issuer.issue(0).unwrap();
    submit(&mut worker, old);
    let mut returned = take(&mut worker);
    returned.call = Call::Decrypt;
    // The same connection can make another sequential call with its same token.
    assert!(worker.try_submit(returned).is_ok());
    let mut returned = take(&mut worker);
    let next = issuer.issue(0).unwrap();
    assert_eq!(
        worker.try_submit(Frame::new(next)).err().unwrap().0,
        TlsError::InvalidInput
    );
    returned.call = Call::Retire;
    returned.input_len = 0;
    assert!(worker.try_submit(returned).is_ok());
    assert_eq!(take(&mut worker).result, Some(Ok(Step::Retired)));
    submit(&mut worker, next);
    drop(take(&mut worker));
    assert_eq!(
        worker.try_submit(Frame::new(old)).err().unwrap().0,
        TlsError::InvalidInput
    );
    stop_reap(&mut worker);
}

#[test]
fn native_driver_is_constructed_on_worker_and_need_not_implement_send() {
    let _serial = SERIAL.lock().unwrap();
    struct Local(std::rc::Rc<()>);
    impl Driver for Local {
        fn call(&mut self, _: &mut Frame) -> Result<Step, TlsError> {
            assert_eq!(std::rc::Rc::strong_count(&self.0), 1);
            Ok(Step::NeedInput)
        }
    }
    let mut issuer = TokenIssuer::new().unwrap();
    let mut worker = Worker::start_with_factory(issuer.generation(), || {
        Ok(Box::new(Local(std::rc::Rc::new(()))))
    })
    .unwrap();
    assert_eq!(ready(&mut worker), Ok(()));
    submit(&mut worker, issuer.issue(0).unwrap());
    assert_eq!(take(&mut worker).result, Some(Ok(Step::NeedInput)));
    stop_reap(&mut worker);
}

#[test]
fn panic_returns_linear_frame_and_requires_actual_reap() {
    let _serial = SERIAL.lock().unwrap();
    struct Panic;
    impl Driver for Panic {
        fn call(&mut self, _: &mut Frame) -> Result<Step, TlsError> {
            panic!("synthetic TLS driver panic");
        }
    }
    let mut issuer = TokenIssuer::new().unwrap();
    let mut worker =
        Worker::start_with_factory(issuer.generation(), || Ok(Box::new(Panic))).unwrap();
    assert_eq!(ready(&mut worker), Ok(()));
    submit(&mut worker, issuer.issue(0).unwrap());
    assert_eq!(take(&mut worker).result, Some(Err(TlsError::ThreadPanic)));
    until(|| worker.owner.as_ref().unwrap().actual_finished());
    assert_eq!(worker.reap_nonblocking(), Err(TlsError::ThreadPanic));
    assert!(!worker.has_in_flight());
    drop(worker);
    let mut next = immediate(issuer.generation());
    assert_eq!(ready(&mut next), Ok(()));
    stop_reap(&mut next);
}

#[test]
fn dropping_live_owner_quarantines_every_capability_until_physical_exit() {
    let _serial = SERIAL.lock().unwrap();
    let mut issuer = TokenIssuer::new().unwrap();
    let (entered_send, entered) = mpsc::sync_channel(1);
    let (release_send, release_rx) = mpsc::sync_channel(1);
    let mut release = Release(Some(release_send));
    let drops = Arc::new(AtomicUsize::new(0));
    let driver = Held {
        entered: entered_send,
        release: release_rx,
        calls: Arc::new(AtomicUsize::new(0)),
        drops: Arc::clone(&drops),
    };
    let mut worker =
        Worker::start_with_factory(issuer.generation(), move || Ok(Box::new(driver))).unwrap();
    assert_eq!(ready(&mut worker), Ok(()));
    submit(&mut worker, issuer.issue(0).unwrap());
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(worker);
    assert_eq!(drops.load(Ordering::Acquire), 0);
    assert!(matches!(
        Worker::start_with_factory(issuer.generation(), || Ok(Box::new(Immediate))),
        Err(TlsError::Busy)
    ));
    assert!(QUARANTINE.lock().unwrap().as_ref().unwrap().in_flight[0].is_some());
    release.now();
    until(|| {
        reap_quarantine();
        QUARANTINE.lock().unwrap().is_none()
    });
    let mut next = immediate(issuer.generation());
    assert_eq!(ready(&mut next), Ok(()));
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert!(QUARANTINE.lock().unwrap().is_none());
    stop_reap(&mut next);
}

#[test]
fn native_reap_keeps_uncollected_results_and_the_single_admission_lease() {
    let _serial = SERIAL.lock().unwrap();
    let mut issuer = TokenIssuer::new().unwrap();
    let mut worker = immediate(issuer.generation());
    assert_eq!(ready(&mut worker), Ok(()));
    for slot in 0..CONNECTION_SLOTS {
        submit(&mut worker, issuer.issue(slot).unwrap());
    }
    worker.request_stop();
    until(|| worker.owner.as_ref().unwrap().actual_finished());
    assert_eq!(worker.reap_nonblocking(), Ok(true));
    assert!(worker.has_in_flight());
    assert!(matches!(
        Worker::start_with_factory(issuer.generation(), || Ok(Box::new(Immediate))),
        Err(TlsError::Busy)
    ));
    for _ in 0..CONNECTION_SLOTS {
        let frame = take(&mut worker);
        assert!(matches!(
            frame.result,
            Some(Ok(Step::Encrypted)) | Some(Err(TlsError::Stopped))
        ));
    }
    assert!(!worker.has_in_flight());
    let mut next = immediate(issuer.generation());
    assert_eq!(ready(&mut next), Ok(()));
    stop_reap(&mut next);
}

#[cfg(windows)]
#[test]
fn rust_packet_completion_does_not_reap_a_held_windows_tls_destructor() {
    let _serial = SERIAL.lock().unwrap();
    struct HeldTls {
        entered: SyncSender<()>,
        release: Receiver<()>,
    }
    impl Drop for HeldTls {
        fn drop(&mut self) {
            let _ = self.entered.send(());
            let _ = self.release.recv();
        }
    }
    thread_local! {
        #[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "nightly-2026-10-29 rebuilt Win7 std wraps this const initializer in a non-const function"))]
        static HELD_TLS: std::cell::RefCell<Option<HeldTls>> = const { std::cell::RefCell::new(None) };
    }
    let issuer = TokenIssuer::new().unwrap();
    let (entered_send, entered) = mpsc::sync_channel(1);
    let (release_send, release_rx) = mpsc::sync_channel(1);
    let mut release = Release(Some(release_send));
    let mut worker = Worker::start_with_factory(issuer.generation(), move || {
        HELD_TLS.with(|slot| {
            *slot.borrow_mut() = Some(HeldTls {
                entered: entered_send,
                release: release_rx,
            });
        });
        Ok(Box::new(Immediate))
    })
    .unwrap();
    assert_eq!(ready(&mut worker), Ok(()));
    worker.request_stop();
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    let thread = worker.owner.as_ref().unwrap().thread.as_ref().unwrap();
    let id = thread.thread().id();
    assert!(thread.is_finished(), "Rust result packet already completed");
    let before = Instant::now();
    assert_eq!(worker.reap_nonblocking(), Ok(false));
    assert!(before.elapsed() < Duration::from_millis(50));
    assert_eq!(
        worker
            .owner
            .as_ref()
            .unwrap()
            .thread
            .as_ref()
            .unwrap()
            .thread()
            .id(),
        id
    );
    // Do not spawn a detector/replacement while a TLS destructor holds the
    // Windows loader lock. The owner itself performs the zero-timeout query.
    release.now();
    stop_reap(&mut worker);
}

#[test]
fn checked_close_error_blocks_replacement_in_an_independent_process() {
    let _serial = SERIAL.lock().unwrap();
    const CHILD_ENV: &str = "FC_TLS_CHECKED_CLOSE_FAULT_CHILD";
    const TEST_NAME: &str = "tls_frontdoor::worker::tests::checked_close_error_blocks_replacement_in_an_independent_process";
    const ERROR: TlsError = TlsError::Cleanup(-20261031);
    if let Ok(mode) = std::env::var(CHILD_ENV) {
        struct FailingClose;
        impl Driver for FailingClose {
            fn call(&mut self, _: &mut Frame) -> Result<Step, TlsError> {
                Ok(Step::NeedInput)
            }
            fn close(&mut self) -> Result<(), TlsError> {
                Err(ERROR)
            }
        }
        struct FailingRetire;
        impl Driver for FailingRetire {
            fn call(&mut self, frame: &mut Frame) -> Result<Step, TlsError> {
                if frame.call == Call::Retire {
                    Err(ERROR)
                } else {
                    Ok(Step::NeedInput)
                }
            }
        }
        struct FailingDrop;
        impl Driver for FailingDrop {
            fn call(&mut self, _: &mut Frame) -> Result<Step, TlsError> {
                Ok(Step::NeedInput)
            }
        }
        impl Drop for FailingDrop {
            fn drop(&mut self) {
                panic!("synthetic native driver destructor panic");
            }
        }
        let mut issuer = TokenIssuer::new().unwrap();
        let mut worker = match mode.as_str() {
            "driver" => {
                Worker::start_with_factory(issuer.generation(), || Ok(Box::new(FailingClose)))
                    .unwrap()
            }
            "retire" => {
                Worker::start_with_factory(issuer.generation(), || Ok(Box::new(FailingRetire)))
                    .unwrap()
            }
            "factory" => Worker::start_with_factory(issuer.generation(), || Err(ERROR)).unwrap(),
            "factory-panic" => Worker::start_with_factory(issuer.generation(), || {
                panic!("synthetic partial native initialization panic");
            })
            .unwrap(),
            "drop-panic" => {
                Worker::start_with_factory(issuer.generation(), || Ok(Box::new(FailingDrop)))
                    .unwrap()
            }
            _ => panic!("unexpected independent close fixture mode"),
        };
        let expected_error = if mode.ends_with("panic") {
            TlsError::ThreadPanic
        } else {
            ERROR
        };
        assert_eq!(
            ready(&mut worker),
            if matches!(mode.as_str(), "driver" | "drop-panic" | "retire") {
                Ok(())
            } else {
                Err(expected_error)
            }
        );
        if mode == "retire" {
            let token = issuer.issue(0).unwrap();
            submit(&mut worker, token);
            let mut frame = take(&mut worker);
            frame.call = Call::Retire;
            frame.input_len = 0;
            assert!(worker.try_submit(frame).is_ok());
            assert_eq!(take(&mut worker).result, Some(Err(ERROR)));
        }
        worker.request_stop();
        until(|| worker.owner.as_ref().unwrap().actual_finished());
        assert_eq!(worker.reap_nonblocking(), Err(expected_error));
        assert_eq!(worker.reap_nonblocking(), Err(expected_error));
        assert!(ADMITTED.load(Ordering::Acquire));
        assert!(CLEANUP_FAULTED.load(Ordering::Acquire));
        drop(worker);
        assert!(matches!(
            Worker::start_with_factory(issuer.generation(), || Ok(Box::new(Immediate))),
            Err(TlsError::Faulted)
        ));
        println!("TLS_CHECKED_CLOSE_CHILD_PASS mode={mode}");
        return;
    }
    // Sticky production admission is never reset for testing. Each native
    // close/factory failure is verified in its own exact-test subprocess.
    for mode in ["driver", "factory", "factory-panic", "drop-panic", "retire"] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", TEST_NAME, "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, mode)
            .env("RUST_BACKTRACE", "0")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("independent checked-close fixture deadline: {mode}");
            }
            thread::sleep(Duration::from_millis(1));
        }
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{mode}: {stdout}\n{stderr}");
        assert!(stdout.contains(&format!("TLS_CHECKED_CLOSE_CHILD_PASS mode={mode}")));
    }
    assert!(!CLEANUP_FAULTED.load(Ordering::Acquire));
    let issuer = TokenIssuer::new().unwrap();
    let mut worker = immediate(issuer.generation());
    assert_eq!(ready(&mut worker), Ok(()));
    stop_reap(&mut worker);
}

#[test]
fn shutdown_and_retire_require_exact_live_token_and_retire_collection_before_reuse() {
    let _serial = SERIAL.lock().unwrap();
    let mut issuer = TokenIssuer::new().unwrap();
    let old = issuer.issue(0).unwrap();
    let new = issuer.issue(0).unwrap();
    let mut worker = immediate(issuer.generation());
    ready(&mut worker).unwrap();
    for call in [Call::Shutdown, Call::Retire] {
        let mut frame = Frame::new(old);
        frame.call = call;
        assert_eq!(
            worker.try_submit(frame).err().unwrap().0,
            TlsError::InvalidInput
        );
    }
    submit(&mut worker, old);
    let mut frame = take(&mut worker);
    for call in [Call::Shutdown, Call::Retire] {
        let mut forged = Frame::new(new);
        forged.call = call;
        assert_eq!(
            worker.try_submit(forged).err().unwrap().0,
            TlsError::InvalidInput
        );
    }
    frame.call = Call::Shutdown;
    assert!(worker.try_submit(frame).is_ok());
    let mut frame = take(&mut worker);
    assert_eq!(frame.result, Some(Ok(Step::Shutdown { complete: true })));
    assert_eq!(frame.output_len, 1);
    assert_eq!(
        worker.try_submit(Frame::new(new)).err().unwrap().0,
        TlsError::InvalidInput
    );
    frame.call = Call::Retire;
    frame.input_len = 0;
    assert!(worker.try_submit(frame).is_ok());
    let mut frame = take(&mut worker);
    assert_eq!(frame.result, Some(Ok(Step::Retired)));
    assert!(frame
        .input
        .iter()
        .chain(frame.output.iter())
        .all(|byte| *byte == 0));
    assert_eq!(frame.input_len, 0);
    assert_eq!(frame.output_len, 0);
    let mut duplicate = Frame::new(old);
    duplicate.call = Call::Retire;
    assert_eq!(
        worker.try_submit(duplicate).err().unwrap().0,
        TlsError::InvalidInput
    );
    frame.token = new;
    frame.call = Call::Handshake;
    assert!(worker.try_submit(frame).is_ok());
    drop(take(&mut worker));
    stop_reap(&mut worker);
}

#[test]
fn held_real_retire_call_never_releases_permit_before_result_is_collected() {
    let _serial = SERIAL.lock().unwrap();
    struct HeldRetire {
        entered: SyncSender<()>,
        release: Receiver<()>,
        returned: Arc<AtomicBool>,
    }
    impl Driver for HeldRetire {
        fn call(&mut self, frame: &mut Frame) -> Result<Step, TlsError> {
            if frame.call == Call::Retire {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
                frame.wipe();
                self.returned.store(true, Ordering::Release);
                Ok(Step::Retired)
            } else {
                Ok(Step::NeedInput)
            }
        }
    }
    let mut issuer = TokenIssuer::new().unwrap();
    let old = issuer.issue(0).unwrap();
    let new = issuer.issue(0).unwrap();
    let (entered_tx, entered) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let mut release = Release(Some(release_tx));
    let returned = Arc::new(AtomicBool::new(false));
    let observed = returned.clone();
    let mut worker = Worker::start_with_factory(issuer.generation(), move || {
        Ok(Box::new(HeldRetire {
            entered: entered_tx,
            release: release_rx,
            returned: observed,
        }))
    })
    .unwrap();
    ready(&mut worker).unwrap();
    submit(&mut worker, old);
    let mut frame = take(&mut worker);
    frame.call = Call::Retire;
    assert!(worker.try_submit(frame).is_ok());
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(
        worker.try_submit(Frame::new(new)).err().unwrap().0,
        TlsError::Busy
    );
    assert!(worker.has_in_flight());
    assert!(worker.poll().unwrap().is_none());
    release.now();
    until(|| returned.load(Ordering::Acquire));
    // Even after allowing the native call to return, only poll() may settle the
    // submitted identity; merely receiving a wake does not free its slot.
    assert_eq!(
        worker.try_submit(Frame::new(new)).err().unwrap().0,
        TlsError::Busy
    );
    let mut frame = take(&mut worker);
    assert_eq!(frame.result, Some(Ok(Step::Retired)));
    frame.token = new;
    frame.call = Call::Handshake;
    assert!(worker.try_submit(frame).is_ok());
    drop(take(&mut worker));
    stop_reap(&mut worker);
}

#[test]
fn queued_exact_retire_on_stop_returns_stopped_without_retiring_or_releasing_owner() {
    let _serial = SERIAL.lock().unwrap();
    struct StopQueuedRetire {
        call_entered: SyncSender<()>,
        call_release: Receiver<()>,
        close_entered: SyncSender<()>,
        close_release: Receiver<()>,
        retire_calls: Arc<AtomicUsize>,
        closed: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    }
    impl Driver for StopQueuedRetire {
        fn call(&mut self, frame: &mut Frame) -> Result<Step, TlsError> {
            if frame.call == Call::Retire {
                self.retire_calls.fetch_add(1, Ordering::AcqRel);
                frame.wipe();
                return Ok(Step::Retired);
            }
            if frame.token.slot() == 0 {
                self.call_entered.send(()).unwrap();
                self.call_release.recv().unwrap();
            }
            Ok(Step::NeedInput)
        }
        fn close(&mut self) -> Result<(), TlsError> {
            self.close_entered.send(()).unwrap();
            self.close_release.recv().unwrap();
            self.closed.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }
    impl Drop for StopQueuedRetire {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::AcqRel);
        }
    }
    let mut issuer = TokenIssuer::new().unwrap();
    let live = issuer.issue(1).unwrap();
    let next = issuer.issue(1).unwrap();
    let held = issuer.issue(0).unwrap();
    let (call_entered_tx, call_entered) = mpsc::sync_channel(1);
    let (call_release_tx, call_release) = mpsc::sync_channel(1);
    let mut release_call = Release(Some(call_release_tx));
    let (close_entered_tx, close_entered) = mpsc::sync_channel(1);
    let (close_release_tx, close_release) = mpsc::sync_channel(1);
    let mut release_close = Release(Some(close_release_tx));
    let retire_calls = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let driver = StopQueuedRetire {
        call_entered: call_entered_tx,
        call_release,
        close_entered: close_entered_tx,
        close_release,
        retire_calls: retire_calls.clone(),
        closed: closed.clone(),
        dropped: dropped.clone(),
    };
    let mut worker =
        Worker::start_with_factory(issuer.generation(), move || Ok(Box::new(driver))).unwrap();
    ready(&mut worker).unwrap();
    // Establish slot 1 first, so its queued retirement has an exact live token.
    submit(&mut worker, live);
    let mut retiring = take(&mut worker);
    assert_eq!(retiring.token, live);
    assert_eq!(retiring.result, Some(Ok(Step::NeedInput)));
    submit(&mut worker, held);
    call_entered.recv_timeout(Duration::from_secs(3)).unwrap();
    retiring.call = Call::Retire;
    retiring.input_len = 0;
    assert!(worker.try_submit(retiring).is_ok());
    assert_eq!(worker.owner.as_ref().unwrap().in_flight[1], Some(live));
    assert_eq!(retire_calls.load(Ordering::Acquire), 0);
    worker.request_stop();
    assert_eq!(worker.reap_nonblocking(), Ok(false));
    release_call.now();
    let returned_held = take(&mut worker);
    assert_eq!(returned_held.token, held);
    assert_eq!(returned_held.result, Some(Ok(Step::NeedInput)));
    let returned_retire = take(&mut worker);
    assert_eq!(returned_retire.token, live);
    assert_eq!(returned_retire.call, Call::Retire);
    assert_eq!(returned_retire.result, Some(Err(TlsError::Stopped)));
    assert_eq!(
        retire_calls.load(Ordering::Acquire),
        0,
        "the stopped queued Retire must not invoke Driver::call"
    );
    assert!(!worker.owner.as_ref().unwrap().retired[1]);
    assert_eq!(worker.owner.as_ref().unwrap().bound[1], Some(live));
    assert_eq!(
        worker.try_submit(Frame::new(next)).err().unwrap().0,
        TlsError::Stopped
    );
    assert!(!worker.has_in_flight());
    close_entered.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(closed.load(Ordering::Acquire), 0);
    assert_eq!(dropped.load(Ordering::Acquire), 0);
    assert_eq!(worker.reap_nonblocking(), Ok(false));
    assert!(ADMITTED.load(Ordering::Acquire));
    assert!(matches!(
        Worker::start_with_factory(issuer.generation(), || Ok(Box::new(Immediate))),
        Err(TlsError::Busy)
    ));
    // Frame return is not checked native cleanup or physical thread completion.
    release_close.now();
    until(|| worker.owner.as_ref().unwrap().actual_finished());
    assert_eq!(closed.load(Ordering::Acquire), 1);
    assert_eq!(dropped.load(Ordering::Acquire), 1);
    assert!(
        ADMITTED.load(Ordering::Acquire),
        "only actual reap may release the process admission lease"
    );
    assert_eq!(worker.reap_nonblocking(), Ok(true));
    assert!(!ADMITTED.load(Ordering::Acquire));
    let mut replacement = immediate(issuer.generation());
    ready(&mut replacement).unwrap();
    stop_reap(&mut replacement);
}
