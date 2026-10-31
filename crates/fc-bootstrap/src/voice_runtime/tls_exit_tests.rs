//! Windows thread handles stay unsignalled while native/Rust TLS destructors
//! still execute, even after the audio closure and its domain have returned.
use super::*;
use std::{
    cell::RefCell,
    sync::{atomic::AtomicUsize, mpsc},
    time::Instant,
};

#[derive(Default)]
struct ExitGate {
    released: Mutex<bool>,
    changed: Condvar,
    entered: AtomicBool,
}
impl ExitGate {
    fn hold(&self) {
        self.entered.store(true, Ordering::Release);
        self.changed.notify_all();
        let mut released = self
            .released
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while !*released {
            released = self
                .changed
                .wait(released)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
    fn release(&self) {
        *self
            .released
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.changed.notify_all();
    }
}
struct TlsExit(Arc<ExitGate>);
impl Drop for TlsExit {
    fn drop(&mut self) {
        self.0.hold();
    }
}
thread_local! {
    static EXIT: RefCell<Option<TlsExit>> = const { RefCell::new(None) };
}
struct ReleaseOnDrop(Arc<ExitGate>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct Factory {
    gate: Arc<ExitGate>,
    created: AtomicUsize,
    finished_domain: AtomicBool,
    dropped_domain: AtomicBool,
}
struct Domain {
    factory: Arc<Factory>,
    stopping: bool,
}
struct FactoryHandle(Arc<Factory>);
impl RuntimeFactory for FactoryHandle {
    fn invalidate_runtime(&self) {}
    fn create(&self, _: RuntimeControl) -> Result<Box<dyn AudioDomain>, RuntimeError> {
        self.0.created.fetch_add(1, Ordering::AcqRel);
        EXIT.with(|slot| *slot.borrow_mut() = Some(TlsExit(self.0.gate.clone())));
        Ok(Box::new(Domain {
            factory: self.0.clone(),
            stopping: false,
        }))
    }
}
impl AudioDomain for Domain {
    fn interrupt(&mut self, reasons: StopReasons) -> Result<(), RuntimeError> {
        self.stopping |= reasons.contains(StopReason::Shutdown);
        Ok(())
    }
    fn step(&mut self) -> Result<DomainProgress, RuntimeError> {
        Ok(if self.stopping {
            DomainProgress::Idle
        } else {
            DomainProgress::Active
        })
    }
    fn has_native_ownership(&self) -> bool {
        false
    }
    fn resume_admission(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn finish(&mut self) -> Result<bool, RuntimeError> {
        self.factory.finished_domain.store(true, Ordering::Release);
        Ok(true)
    }
}
impl Drop for Domain {
    fn drop(&mut self) {
        self.factory.dropped_domain.store(true, Ordering::Release);
    }
}
fn eventually(predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "audio/TLS transition did not arrive"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn audio_reaper_waits_for_windows_tls_exit_without_joining_the_live_thread() {
    let gate = Arc::new(ExitGate::default());
    let _release_on_failure = ReleaseOnDrop(gate.clone());
    let factory = Arc::new(Factory {
        gate: gate.clone(),
        created: AtomicUsize::new(0),
        finished_domain: AtomicBool::new(false),
        dropped_domain: AtomicBool::new(false),
    });
    let runtime = VoiceRuntime::new(Arc::new(FactoryHandle(factory.clone())));
    // Windows runs native TLS detach under the loader lock. A caller created
    // only AFTER TLS is held cannot enter its Rust closure, independently of
    // whether the reaper blocks. Start it first and prove it is waiting for us.
    let (start_send, start_receive) = mpsc::channel();
    let (ready_send, ready_receive) = mpsc::channel();
    let (send, receive) = mpsc::channel();
    let owned = runtime.clone();
    let probe = thread::spawn(move || {
        let _ = ready_send.send(());
        if start_receive.recv().is_ok() {
            let _ = send.send(owned.reap_nonblocking());
        }
    });
    ready_receive
        .recv_timeout(Duration::from_secs(15))
        .expect("reaper caller did not start before TLS teardown");
    runtime.set_available(true).unwrap();
    eventually(|| factory.created.load(Ordering::Acquire) == 1);
    runtime.shutdown();
    eventually(|| gate.entered.load(Ordering::Acquire));
    assert!(factory.finished_domain.load(Ordering::Acquire));
    assert!(factory.dropped_domain.load(Ordering::Acquire));
    let before = runtime.status().unwrap();
    assert!(
        before.ack_generation >= before.stop_generation,
        "the fixture must have already acknowledged the logical shutdown"
    );
    assert!(
        before.physical_thread_running,
        "TLS destruction still owns the actual Windows thread"
    );
    assert_ne!(
        before.phase,
        RuntimePhase::Closed,
        "Closed must await OS thread termination, not just domain finish"
    );
    assert_eq!(before.physical_threads_started, 1);

    // Execute the potentially regressed join on a disposable caller. On timeout
    // release TLS before failing, so the regression cannot hang the test process.
    start_send.send(()).unwrap();
    let result = match receive.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(error) => {
            gate.release();
            probe.join().unwrap();
            panic!("nonblocking audio reaper waited on held TLS destruction: {error}");
        }
    };
    result.unwrap();
    let held = runtime.status().unwrap();
    assert!(held.physical_thread_running);
    assert_ne!(held.phase, RuntimePhase::Closed);
    assert_eq!(held.physical_threads_started, 1);
    assert_eq!(factory.created.load(Ordering::Acquire), 1);
    assert_eq!(runtime.wake(), Err(RuntimeError::Closed));

    gate.release();
    // Caller termination also passes through Windows loader teardown. Its
    // reply proves the reaper returned; join only after the held TLS is free.
    probe.join().unwrap();
    eventually(|| {
        runtime.reap_nonblocking().unwrap();
        let status = runtime.status().unwrap();
        status.phase == RuntimePhase::Closed
            && !status.physical_thread_running
            && status.ack_generation >= status.stop_generation
    });
    let final_status = runtime.status().unwrap();
    assert_eq!(final_status.physical_threads_started, 1);
    assert_eq!(factory.created.load(Ordering::Acquire), 1);
}
