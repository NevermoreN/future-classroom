//! Real loopback/IOCP ownership tests with an intentionally held fake Driver.
//! The held operation is not evidence that Windows cryptography is interruptible.
use super::*;
use crate::tls_frontdoor::{worker::tests::SERIAL, Driver};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    sync::{
        atomic::AtomicUsize,
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc,
    },
};

const MARKER: &str = "FutureClassroom-Test-TLS-held-owned-frame";
struct HeldDriver {
    entered: SyncSender<RequestToken>,
    release: Receiver<()>,
    closes: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl Driver for HeldDriver {
    fn call(&mut self, frame: &mut Frame) -> Result<Step, TlsError> {
        self.entered.send(frame.token).unwrap();
        self.release.recv().unwrap();
        Ok(Step::NeedInput)
    }
    fn close(&mut self) -> Result<(), TlsError> {
        self.closes.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}
impl Drop for HeldDriver {
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
struct Fixture {
    owner: Owner,
    client: TcpStream,
    release: Release,
    entered_token: RequestToken,
    closes: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
fn until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !check() {
        assert!(Instant::now() < deadline, "actual native fixture deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn fixture() -> Fixture {
    let mut owner = Owner::new(MARKER).unwrap();
    let (entered_send, entered) = mpsc::sync_channel(1);
    let (release_send, release) = mpsc::sync_channel(1);
    let release_guard = Release(Some(release_send));
    let closes = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let driver = HeldDriver {
        entered: entered_send,
        release,
        closes: Arc::clone(&closes),
        drops: Arc::clone(&drops),
    };
    let worker =
        Worker::start_with_factory(owner.generation, move || Ok(Box::new(driver))).unwrap();
    owner.start_with_worker(worker).unwrap();
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, owner.port_number));
    let mut client = TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
    client.write_all(b"x").unwrap();
    let mut entered_token = None;
    until(|| {
        owner.poll().unwrap();
        entered_token = match entered.try_recv() {
            Ok(token) => Some(token),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!("held driver exited before admission"),
        };
        entered_token.is_some()
    });
    Fixture {
        owner,
        client,
        release: release_guard,
        entered_token: entered_token.unwrap(),
        closes,
        drops,
    }
}
fn assert_closed_on_wire(client: &mut TcpStream) {
    client.set_nonblocking(true).unwrap();
    let mut closed = false;
    until(|| {
        let mut byte = [0];
        match client.read(&mut byte) {
            Ok(0) => closed = true,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                closed = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("unexpected held fixture wire result: {other:?}"),
        }
        closed
    });
}
fn assert_held_after_close(fixture: &mut Fixture) {
    let index = fixture.entered_token.slot();
    fixture.owner.stop().unwrap();
    fixture.owner.poll().unwrap();
    assert_closed_on_wire(&mut fixture.client);
    let slot = &fixture.owner.resources.slots[index];
    assert!(slot.socket == INVALID_SOCKET && slot.phase == Phase::Closed);
    assert!(
        slot.frame.is_none(),
        "native call still owns the only frame"
    );
    let worker = fixture.owner.resources.worker.as_mut().unwrap();
    assert!(worker.has_in_flight());
    assert_eq!(worker.reap_nonblocking(), Ok(false));
    assert!(!fixture.owner.drained);
    assert_eq!(fixture.drops.load(Ordering::Acquire), 0);
    assert_eq!(fixture.closes.load(Ordering::Acquire), 0);
    assert!(fixture.owner.report.closed_with_sspi_in_flight >= 1);
    assert!(Owner::new(MARKER).is_err());
    assert!(matches!(
        Worker::start_with_factory(fixture.owner.generation, || {
            panic!("replacement factory must never run")
        }),
        Err(TlsError::Busy)
    ));
}

#[test]
fn native_socket_close_keeps_held_frame_until_real_late_completion_and_reap() {
    let _serial = SERIAL.lock().unwrap();
    let mut fixture = fixture();
    let first_generation = fixture.owner.generation;
    assert_held_after_close(&mut fixture);
    fixture.release.now();
    fixture.owner.finish().unwrap();
    assert!(fixture.owner.drained);
    assert_eq!(fixture.owner.report.accepted, 1);
    assert_eq!(fixture.owner.report.responses, 0);
    assert_eq!(fixture.owner.report.sspi_submissions, 1);
    assert_eq!(fixture.owner.report.sspi_completions, 1);
    assert_eq!(fixture.owner.report.returned_frames, 1);
    assert_eq!(fixture.owner.report.slot_frames_at_drain, CONNECTION_SLOTS);
    assert_eq!(fixture.owner.report.late_sspi_completions, 1);
    assert_eq!(
        fixture.owner.report.submissions,
        fixture.owner.report.completions
    );
    assert!(fixture
        .owner
        .resources
        .slots
        .iter()
        .all(|slot| slot.pending.is_none() && slot.frame.is_some()));
    assert!(!fixture
        .owner
        .resources
        .worker
        .as_ref()
        .unwrap()
        .has_in_flight());
    assert_eq!(
        fixture
            .owner
            .resources
            .worker
            .as_mut()
            .unwrap()
            .reap_nonblocking(),
        Ok(true)
    );
    assert_eq!(fixture.closes.load(Ordering::Acquire), 1);
    assert_eq!(fixture.drops.load(Ordering::Acquire), 1);
    println!(
        "TLS_HELD_NATIVE_OWNER_PASS fakeDriver=true actualSocket=true closedWithSspiInFlight={} lateSspiCompletions={} ioSubmissions={} ioCompletions={} returnedFrames={} physicalReap=true responses={}",
        fixture.owner.report.closed_with_sspi_in_flight,
        fixture.owner.report.late_sspi_completions,
        fixture.owner.report.submissions,
        fixture.owner.report.completions,
        fixture.owner.report.returned_frames,
        fixture.owner.report.responses,
    );
    drop(fixture);
    assert!(!OWNER_FAULTED.load(Ordering::Acquire));
    struct Idle;
    impl Driver for Idle {
        fn call(&mut self, _: &mut Frame) -> Result<Step, TlsError> {
            Ok(Step::NeedInput)
        }
    }
    let mut replacement = Owner::new(MARKER).unwrap();
    assert_ne!(replacement.generation, first_generation);
    let worker = Worker::start_with_factory(replacement.generation, || Ok(Box::new(Idle))).unwrap();
    replacement.start_with_worker(worker).unwrap();
    replacement.finish().unwrap();
    assert!(replacement.drained);
}

#[test]
fn held_driver_drain_timeout_retains_native_owner_in_independent_process() {
    let _serial = SERIAL.lock().unwrap();
    const CHILD_ENV: &str = "FC_TLS_NATIVE_HELD_DRAIN_CHILD";
    const TEST_NAME: &str = "tls_frontdoor::native::tests::held_driver_drain_timeout_retains_native_owner_in_independent_process";
    const CHILD_PASS: &str = "TLS_HELD_DRAIN_FAULT_CHILD_PASS driverDropObserved=true ownerState=RETAINED_FAULTED cleanupComplete=false processTeardownScope=INDEPENDENT_CHILD";
    if std::env::var(CHILD_ENV).as_deref() == Ok("1") {
        let mut fixture = fixture();
        assert_held_after_close(&mut fixture);
        let started = Instant::now();
        let error = fixture.owner.finish().unwrap_err();
        assert!(
            started.elapsed() >= BUDGET,
            "original five-second drain must really expire"
        );
        assert!(error.contains("TLS drain deadline"), "{error}");
        assert!(!fixture.owner.drained);
        assert!(fixture
            .owner
            .resources
            .worker
            .as_ref()
            .unwrap()
            .has_in_flight());
        assert!(fixture.owner.resources.slots[fixture.entered_token.slot()]
            .frame
            .is_none());
        let Fixture {
            owner,
            client,
            mut release,
            closes,
            drops,
            ..
        } = fixture;
        drop(owner);
        drop(client);
        assert!(OWNER_FAULTED.load(Ordering::Acquire));
        assert!(OWNER_ACTIVE.load(Ordering::Acquire));
        assert!(Owner::new(MARKER).is_err());
        release.now();
        until(|| drops.load(Ordering::Acquire) == 1);
        assert_eq!(closes.load(Ordering::Acquire), 1);
        // The native Owner was deliberately retained, so its result receiver,
        // frame ledger and thread handle were not reaped. Driver destruction
        // alone is never reported as a closed or cleaned transport owner.
        assert!(OWNER_FAULTED.load(Ordering::Acquire));
        assert!(OWNER_ACTIVE.load(Ordering::Acquire));
        assert!(Owner::new(MARKER).is_err());
        assert!(matches!(
            Worker::start_with_factory(1, || panic!("retained worker cannot be replaced")),
            Err(TlsError::Busy)
        ));
        println!("{CHILD_PASS}");
        return;
    }
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST_NAME, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, "1")
        .env("RUST_BACKTRACE", "0")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("independent native drain fault fixture deadline");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let status = child.wait().unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        pipe.take(16 * 1024).read_to_end(&mut stdout).unwrap();
    }
    if let Some(pipe) = child.stderr.take() {
        pipe.take(16 * 1024).read_to_end(&mut stderr).unwrap();
    }
    let stdout = String::from_utf8_lossy(&stdout);
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains(CHILD_PASS), "{stdout}\n{stderr}");
    assert!(!OWNER_FAULTED.load(Ordering::Acquire));
    assert!(!OWNER_ACTIVE.load(Ordering::Acquire));
}
