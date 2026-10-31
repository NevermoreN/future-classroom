use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Default)]
struct Gate {
    blocked: Mutex<bool>,
    signal: Condvar,
    entered: AtomicBool,
}
impl Gate {
    fn wait(&self) {
        self.entered.store(true, Ordering::Release);
        let mut blocked = self.blocked.lock().unwrap();
        while *blocked {
            blocked = self.signal.wait(blocked).unwrap();
        }
    }
    fn block(&self) {
        *self.blocked.lock().unwrap() = true;
        self.entered.store(false, Ordering::Release);
    }
    fn open(&self) {
        *self.blocked.lock().unwrap() = false;
        self.signal.notify_all();
    }
}
#[derive(Default)]
struct FakeFactory {
    state: AtomicU64,
    queued: AtomicUsize,
    completed: AtomicUsize,
    interrupted: AtomicUsize,
    alive: AtomicUsize,
    maximum: AtomicUsize,
    created: AtomicUsize,
    native: AtomicBool,
    hold_play: AtomicBool,
    hold_release: AtomicBool,
    retained: AtomicBool,
    fail_step: AtomicBool,
    finish_pending: AtomicBool,
    finish: Gate,
    resume: Gate,
}
impl FakeFactory {
    fn admit(&self) -> bool {
        self.state.load(Ordering::Acquire) & 1 == 1
    }
    fn submit(&self, count: usize) {
        assert!(self.admit());
        self.queued.fetch_add(count, Ordering::AcqRel);
    }
}
struct FakeDomain {
    factory: Arc<FakeFactory>,
    control: RuntimeControl,
    stop_epoch: u64,
    interrupted: bool,
}
struct Factory(Arc<FakeFactory>);
impl RuntimeFactory for Factory {
    fn invalidate_runtime(&self) {
        let _ = self
            .0
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |w| {
                Some(((w >> 1) + 1) << 1)
            });
    }
    fn create(&self, control: RuntimeControl) -> Result<Box<dyn AudioDomain>, RuntimeError> {
        let alive = self.0.alive.fetch_add(1, Ordering::AcqRel) + 1;
        self.0.maximum.fetch_max(alive, Ordering::AcqRel);
        self.0.created.fetch_add(1, Ordering::AcqRel);
        Ok(Box::new(FakeDomain {
            stop_epoch: self.0.state.load(Ordering::Acquire) >> 1,
            factory: self.0.clone(),
            control,
            interrupted: false,
        }))
    }
}
impl AudioDomain for FakeDomain {
    fn interrupt(&mut self, _: StopReasons) -> Result<(), RuntimeError> {
        self.interrupted = true;
        self.stop_epoch = self.factory.state.load(Ordering::Acquire) >> 1;
        self.factory.interrupted.fetch_add(
            self.factory.queued.swap(0, Ordering::AcqRel),
            Ordering::AcqRel,
        );
        Ok(())
    }
    fn step(&mut self) -> Result<DomainProgress, RuntimeError> {
        if self.factory.fail_step.swap(false, Ordering::AcqRel) {
            return Err(RuntimeError::Driver);
        }
        if self.interrupted {
            if self.factory.hold_release.load(Ordering::Acquire)
                && self.factory.native.load(Ordering::Acquire)
            {
                return Ok(if self.factory.retained.load(Ordering::Acquire) {
                    DomainProgress::Retained
                } else {
                    DomainProgress::Draining
                });
            }
            self.factory.native.store(false, Ordering::Release);
            return Ok(DomainProgress::Idle);
        }
        if self.factory.queued.load(Ordering::Acquire) > 0 {
            self.factory.native.store(true, Ordering::Release);
            if self.factory.hold_play.load(Ordering::Acquire) {
                return Ok(DomainProgress::Active);
            }
            if !self.control.available() {
                return Ok(DomainProgress::Draining);
            }
            self.factory.queued.fetch_sub(1, Ordering::AcqRel);
            self.factory.completed.fetch_add(1, Ordering::AcqRel);
            self.factory.native.store(false, Ordering::Release);
            if self.factory.queued.load(Ordering::Acquire) > 0 {
                Ok(DomainProgress::Active)
            } else {
                Ok(DomainProgress::Idle)
            }
        } else {
            Ok(DomainProgress::Idle)
        }
    }
    fn has_native_ownership(&self) -> bool {
        self.factory.native.load(Ordering::Acquire)
    }
    fn resume_admission(&mut self) -> Result<(), RuntimeError> {
        self.factory.resume.wait();
        let state = self.factory.state.load(Ordering::Acquire);
        if state & 1 == 1 {
            return Ok(());
        }
        self.factory
            .state
            .compare_exchange(
                self.stop_epoch << 1,
                self.stop_epoch << 1 | 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|_| RuntimeError::Storage)
    }
    fn finish(&mut self) -> Result<bool, RuntimeError> {
        self.factory.finish.wait();
        assert!(!self.has_native_ownership());
        Ok(!self.factory.finish_pending.load(Ordering::Acquire))
    }
}
impl Drop for FakeDomain {
    fn drop(&mut self) {
        self.factory.alive.fetch_sub(1, Ordering::AcqRel);
    }
}
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(Instant::now() < deadline, "runtime condition timed out");
        thread::sleep(Duration::from_millis(2));
    }
}
fn ready() -> (VoiceRuntime, Arc<FakeFactory>) {
    let factory = Arc::new(FakeFactory::default());
    let runtime = VoiceRuntime::new(Arc::new(Factory(factory.clone())));
    runtime.set_available(true).unwrap();
    until(|| {
        runtime.reap_nonblocking().unwrap();
        factory.admit() && !runtime.status().unwrap().physical_thread_running
    });
    (runtime, factory)
}
fn close(runtime: &VoiceRuntime) {
    runtime.shutdown();
    until(|| {
        let _ = runtime.reap_nonblocking();
        let s = runtime.status().unwrap();
        s.phase == RuntimePhase::Closed && !s.physical_thread_running
    });
}
#[test]
fn concurrent_wake_and_idle_exit_never_lose_jobs_or_create_two_audio_domains() {
    let (runtime, factory) = ready();
    factory.submit(40);
    let mut producers = Vec::new();
    for _ in 0..8 {
        let runtime = runtime.clone();
        producers.push(thread::spawn(move || {
            for _ in 0..5 {
                runtime.wake().unwrap();
            }
        }));
    }
    for producer in producers {
        producer.join().unwrap();
    }
    until(|| {
        runtime.reap_nonblocking().unwrap();
        factory.completed.load(Ordering::Acquire) == 40
            && !runtime.status().unwrap().physical_thread_running
    });
    assert_eq!(factory.maximum.load(Ordering::Acquire), 1);
    assert_eq!(factory.queued.load(Ordering::Acquire), 0);
    assert_eq!(factory.alive.load(Ordering::Acquire), 0);
    close(&runtime);
}
#[test]
fn wake_during_native_finish_is_pending_until_the_old_physical_thread_exits() {
    let (runtime, factory) = ready();
    factory.finish.block();
    factory.submit(1);
    runtime.wake().unwrap();
    until(|| factory.finish.entered.load(Ordering::Acquire));
    let before = runtime.status().unwrap().physical_threads_started;
    factory.submit(1);
    runtime.wake().unwrap();
    runtime.reap_nonblocking().unwrap();
    assert_eq!(runtime.status().unwrap().physical_threads_started, before);
    assert_eq!(factory.completed.load(Ordering::Acquire), 1);
    factory.finish.open();
    until(|| {
        runtime.reap_nonblocking().unwrap();
        factory.completed.load(Ordering::Acquire) == 2
            && !runtime.status().unwrap().physical_thread_running
    });
    assert_eq!(factory.maximum.load(Ordering::Acquire), 1);
    close(&runtime);
}
#[test]
fn ordinary_listener_callback_drain_does_not_fault_or_start_new_jobs_on_unregistered_listener() {
    let (runtime, factory) = ready();
    factory.finish.entered.store(false, Ordering::Release);
    factory.finish_pending.store(true, Ordering::Release);
    factory.submit(1);
    runtime.wake().unwrap();
    until(|| factory.finish.entered.load(Ordering::Acquire));
    factory.submit(1);
    runtime.wake().unwrap();
    thread::sleep(Duration::from_millis(30));
    assert_eq!(factory.completed.load(Ordering::Acquire), 1);
    assert!(!runtime.status().unwrap().faulted);
    factory.finish_pending.store(false, Ordering::Release);
    until(|| {
        runtime.reap_nonblocking().unwrap();
        factory.completed.load(Ordering::Acquire) == 2
            && !runtime.status().unwrap().physical_thread_running
    });
    assert_eq!(factory.maximum.load(Ordering::Acquire), 1);
    close(&runtime);
}
#[test]
fn writer_resume_and_finish_never_hold_the_supervisor_ui_lock() {
    let factory = Arc::new(FakeFactory::default());
    factory.resume.block();
    let runtime = VoiceRuntime::new(Arc::new(Factory(factory.clone())));
    runtime.set_available(true).unwrap();
    until(|| factory.resume.entered.load(Ordering::Acquire));
    assert!(runtime.status().unwrap().physical_thread_running);
    runtime.reap_nonblocking().unwrap();
    let old = runtime.control().generation();
    runtime.request_stop(StopReason::Emergency);
    assert!(runtime.control().generation() > old);
    assert!(!factory.admit());
    factory.resume.open();
    until(|| {
        runtime.reap_nonblocking().unwrap();
        factory.admit() && !runtime.status().unwrap().physical_thread_running
    });
    close(&runtime);
}
#[test]
fn independent_stop_blocks_writes_then_ack_requires_native_release_and_no_old_replay() {
    let (runtime, factory) = ready();
    factory.hold_play.store(true, Ordering::Release);
    factory.hold_release.store(true, Ordering::Release);
    factory.submit(2);
    runtime.wake().unwrap();
    until(|| factory.native.load(Ordering::Acquire));
    let old = runtime.control().generation();
    runtime.request_stop(StopReason::SessionUnavailable);
    assert!(!runtime.control().permits(old));
    assert!(!factory.admit());
    until(|| factory.interrupted.load(Ordering::Acquire) == 2);
    assert!(runtime.status().unwrap().ack_generation < runtime.status().unwrap().stop_generation);
    runtime.set_available(true).unwrap();
    assert!(!factory.admit());
    factory.hold_release.store(false, Ordering::Release);
    until(|| {
        runtime.reap_nonblocking().unwrap();
        factory.admit() && !runtime.status().unwrap().physical_thread_running
    });
    assert_eq!(factory.completed.load(Ordering::Acquire), 0);
    factory.hold_play.store(false, Ordering::Release);
    factory.submit(1);
    runtime.wake().unwrap();
    until(|| {
        runtime.reap_nonblocking().unwrap();
        factory.completed.load(Ordering::Acquire) == 1
            && !runtime.status().unwrap().physical_thread_running
    });
    close(&runtime);
}
#[test]
fn retained_driver_never_gets_a_replacement_and_shutdown_waits_for_actual_release() {
    let (runtime, factory) = ready();
    factory.hold_play.store(true, Ordering::Release);
    factory.hold_release.store(true, Ordering::Release);
    factory.retained.store(true, Ordering::Release);
    factory.submit(1);
    runtime.wake().unwrap();
    until(|| factory.native.load(Ordering::Acquire));
    runtime.request_stop(StopReason::Emergency);
    until(|| runtime.status().unwrap().phase == RuntimePhase::Retained);
    let count = runtime.status().unwrap().physical_threads_started;
    assert_eq!(runtime.wake(), Err(RuntimeError::Halted));
    assert_eq!(runtime.set_available(true), Err(RuntimeError::Halted));
    runtime.shutdown();
    runtime.reap_nonblocking().unwrap();
    assert!(runtime.status().unwrap().physical_thread_running);
    assert_eq!(runtime.status().unwrap().physical_threads_started, count);
    factory.hold_release.store(false, Ordering::Release);
    until(|| {
        let _ = runtime.reap_nonblocking();
        let s = runtime.status().unwrap();
        s.phase == RuntimePhase::Closed && !s.physical_thread_running
    });
    assert!(!factory.admit());
    assert_eq!(factory.maximum.load(Ordering::Acquire), 1);
}
#[test]
fn service_waker_is_weak_and_empty_shutdown_needs_no_audio_device() {
    let (runtime, factory) = ready();
    let wake = runtime.weak_waker();
    close(&runtime);
    assert_eq!(factory.completed.load(Ordering::Acquire), 0);
    assert_eq!(factory.alive.load(Ordering::Acquire), 0);
    drop(runtime);
    wake();
}
