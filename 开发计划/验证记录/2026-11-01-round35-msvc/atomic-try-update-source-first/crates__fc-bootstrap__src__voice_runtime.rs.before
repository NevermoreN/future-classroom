//! On-demand single physical audio thread. Producers never join a live thread;
//! the application polls `reap_nonblocking` to settle an idle exit/wake race.
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Condvar, Mutex, Weak,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum StopReason {
    Emergency = 1,
    SessionUnavailable = 2,
    Suspend = 4,
    Shutdown = 8,
    DeviceChanged = 16,
    Fault = 32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StopReasons(u32);
impl StopReasons {
    pub fn contains(self, reason: StopReason) -> bool {
        self.0 & reason as u32 != 0
    }
    pub fn bits(self) -> u32 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    Closed,
    Halted,
    Spawn,
    Panicked,
    Poisoned,
    Factory,
    Storage,
    Driver,
    Timeout,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePhase {
    Idle,
    Starting,
    Running,
    Stopping,
    Retained,
    Halted,
    Closed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub phase: RuntimePhase,
    pub physical_threads_started: u64,
    pub physical_thread_running: bool,
    pub stop_generation: u64,
    pub ack_generation: u64,
    pub available: bool,
    pub faulted: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainProgress {
    /// Fresh inventory proves no nonterminal task AND no native ownership.
    Idle,
    Active,
    Gap,
    Draining,
    Retained,
}
/// Created only in the dedicated audio thread; this object need not be Send.
pub trait AudioDomain {
    /// Persist generation-invalidating interruption before ordinary reset. On a
    /// writer timeout/unknown result, still stop native output and return error.
    fn interrupt(&mut self, reasons: StopReasons) -> Result<(), RuntimeError>;
    fn step(&mut self) -> Result<DomainProgress, RuntimeError>;
    fn has_native_ownership(&self) -> bool;
    /// Bound to the exact service stop generation; cannot reopen a newer stop.
    fn resume_admission(&mut self) -> Result<(), RuntimeError>;
    /// Close/unregister platform listeners only after all output ownership ended.
    /// False retains this SAME domain for callback drain; never create a replacement.
    fn finish(&mut self) -> Result<bool, RuntimeError>;
}
pub trait RuntimeFactory: Send + Sync + 'static {
    /// Strictly atomic: callable from COM/WTS/power callbacks, no mutex/DB/UI.
    fn invalidate_runtime(&self);
    fn create(&self, control: RuntimeControl) -> Result<Box<dyn AudioDomain>, RuntimeError>;
}
struct State {
    phase: RuntimePhase,
    thread: Option<JoinHandle<()>>,
    starts: u64,
    ack_generation: u64,
}
struct Shared {
    factory: Arc<dyn RuntimeFactory>,
    state: Mutex<State>,
    signal: Condvar,
    generation: AtomicU64,
    reasons: AtomicU32,
    needs_worker: AtomicBool,
    wake_sequence: AtomicU64,
    available: AtomicBool,
    closed: AtomicBool,
    faulted: AtomicBool,
}
#[derive(Clone)]
pub struct RuntimeControl {
    shared: Arc<Shared>,
}
impl RuntimeControl {
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }
    pub fn available(&self) -> bool {
        self.shared.available.load(Ordering::Acquire)
            && !self.shared.closed.load(Ordering::Acquire)
            && !self.shared.faulted.load(Ordering::Acquire)
    }
    pub fn permits(&self, generation: u64) -> bool {
        self.available()
            && self.generation() == generation
            && self.shared.reasons.load(Ordering::Acquire) == 0
    }
    /// No state mutex, database, thread spawn, join, or user message. Native
    /// per-buffer authorization sees the invalidation before this method returns.
    pub fn request_stop(&self, reason: StopReason) {
        if matches!(reason, StopReason::SessionUnavailable | StopReason::Suspend) {
            self.shared.available.store(false, Ordering::Release);
        }
        self.shared.factory.invalidate_runtime();
        if self
            .shared
            .generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| v.checked_add(1))
            .is_err()
        {
            self.shared.faulted.store(true, Ordering::Release);
            self.shared.closed.store(true, Ordering::Release);
        }
        self.shared
            .reasons
            .fetch_or(reason as u32, Ordering::AcqRel);
        self.shared.wake_sequence.fetch_add(1, Ordering::AcqRel);
        self.shared.needs_worker.store(true, Ordering::Release);
        self.shared.signal.notify_all();
    }
}
#[derive(Clone)]
pub struct VoiceRuntime {
    shared: Arc<Shared>,
}
impl VoiceRuntime {
    /// Does not start an idle background thread. The initial admission fence is
    /// opened by set_available(true) only after trusted startup recovery/drain.
    pub fn new(factory: Arc<dyn RuntimeFactory>) -> Self {
        factory.invalidate_runtime();
        Self {
            shared: Arc::new(Shared {
                factory,
                state: Mutex::new(State {
                    phase: RuntimePhase::Idle,
                    thread: None,
                    starts: 0,
                    ack_generation: 0,
                }),
                signal: Condvar::new(),
                generation: AtomicU64::new(1),
                reasons: AtomicU32::new(StopReason::SessionUnavailable as u32),
                needs_worker: AtomicBool::new(false),
                wake_sequence: AtomicU64::new(0),
                available: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                faulted: AtomicBool::new(false),
            }),
        }
    }
    pub fn control(&self) -> RuntimeControl {
        RuntimeControl {
            shared: self.shared.clone(),
        }
    }
    pub fn weak_waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let shared: Weak<Shared> = Arc::downgrade(&self.shared);
        Arc::new(move || {
            if let Some(shared) = shared.upgrade() {
                let _ = Self { shared }.wake();
            }
        })
    }
    pub fn request_stop(&self, reason: StopReason) {
        self.control().request_stop(reason);
    }
    pub fn set_available(&self, available: bool) -> Result<(), RuntimeError> {
        if !available {
            if self.shared.available.swap(false, Ordering::AcqRel) {
                self.request_stop(StopReason::SessionUnavailable);
            }
            return Ok(());
        }
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(RuntimeError::Closed);
        }
        if self.shared.faulted.load(Ordering::Acquire) {
            return Err(RuntimeError::Halted);
        }
        if !self.shared.available.swap(true, Ordering::AcqRel) {
            self.wake()?;
        }
        Ok(())
    }
    pub fn wake(&self) -> Result<(), RuntimeError> {
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(RuntimeError::Closed);
        }
        if self.shared.faulted.load(Ordering::Acquire) {
            return Err(RuntimeError::Halted);
        }
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?;
        self.shared.wake_sequence.fetch_add(1, Ordering::AcqRel);
        self.shared.needs_worker.store(true, Ordering::Release);
        self.reap_locked(&mut state)?;
        self.start_locked(&mut state)?;
        self.shared.signal.notify_all();
        Ok(())
    }
    /// Call from the existing application timer/event loop. A live/returning old
    /// thread is NEVER joined or replaced, even after it published logical Idle.
    pub fn reap_nonblocking(&self) -> Result<(), RuntimeError> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?;
        self.reap_locked(&mut state)?;
        self.start_locked(&mut state)
    }
    fn reap_locked(&self, state: &mut State) -> Result<(), RuntimeError> {
        if state
            .thread
            .as_ref()
            .is_some_and(crate::physical_thread::is_finished)
        {
            // is_finished proves the physical audio thread and its destructors
            // have completed. This join cannot wait on native or writer work.
            if state
                .thread
                .take()
                .expect("finished handle")
                .join()
                .is_err()
            {
                self.shared.factory.invalidate_runtime();
                self.shared.faulted.store(true, Ordering::Release);
                state.phase = RuntimePhase::Halted;
                return Err(RuntimeError::Panicked);
            }
        }
        if self.shared.closed.load(Ordering::Acquire)
            && state.thread.is_none()
            && state.ack_generation >= self.shared.generation.load(Ordering::Acquire)
        {
            state.phase = RuntimePhase::Closed;
        }
        Ok(())
    }
    fn start_locked(&self, state: &mut State) -> Result<(), RuntimeError> {
        if state.thread.is_some() || !self.shared.needs_worker.load(Ordering::Acquire) {
            return Ok(());
        }
        if self.shared.faulted.load(Ordering::Acquire) {
            return Err(RuntimeError::Halted);
        }
        if state.phase == RuntimePhase::Closed {
            return Ok(());
        }
        state.phase = RuntimePhase::Starting;
        let shared = self.shared.clone();
        let spawned = thread::Builder::new()
            .name("fc-audio".into())
            .spawn(move || run(shared));
        match spawned {
            Ok(handle) => {
                state.starts += 1;
                state.thread = Some(handle);
                Ok(())
            }
            Err(_) => {
                self.shared.factory.invalidate_runtime();
                self.shared.faulted.store(true, Ordering::Release);
                state.phase = RuntimePhase::Halted;
                Err(RuntimeError::Spawn)
            }
        }
    }
    pub fn shutdown(&self) {
        if !self.shared.closed.swap(true, Ordering::AcqRel) {
            self.request_stop(StopReason::Shutdown);
        }
    }
    pub fn status(&self) -> Result<RuntimeStatus, RuntimeError> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?;
        Ok(RuntimeStatus {
            phase: state.phase,
            physical_threads_started: state.starts,
            physical_thread_running: state
                .thread
                .as_ref()
                .is_some_and(|t| !crate::physical_thread::is_finished(t)),
            stop_generation: self.shared.generation.load(Ordering::Acquire),
            ack_generation: state.ack_generation,
            available: self.shared.available.load(Ordering::Acquire),
            faulted: self.shared.faulted.load(Ordering::Acquire),
        })
    }
}

fn phase(shared: &Shared, phase: RuntimePhase) {
    if let Ok(mut state) = shared.state.lock() {
        state.phase = phase;
    }
}
fn run(shared: Arc<Shared>) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_owned(&shared)));
    if result.is_err() {
        shared.factory.invalidate_runtime();
        shared.faulted.store(true, Ordering::Release);
        phase(&shared, RuntimePhase::Halted);
    }
}
fn run_owned(shared: &Arc<Shared>) {
    let control = RuntimeControl {
        shared: shared.clone(),
    };
    let mut domain = match shared.factory.create(control.clone()) {
        Ok(domain) => domain,
        Err(_) => {
            shared.factory.invalidate_runtime();
            shared.faulted.store(true, Ordering::Release);
            phase(shared, RuntimePhase::Halted);
            return;
        }
    };
    let mut interrupted_generation = 0;
    let mut stop_proven = true;
    let mut stopping = false;
    let mut retiring: Option<std::time::Instant> = None;
    loop {
        let observed_wake = shared.wake_sequence.load(Ordering::Acquire);
        let reasons = shared.reasons.swap(0, Ordering::AcqRel);
        let generation = control.generation();
        if reasons != 0 || stopping && (generation != interrupted_generation || !stop_proven) {
            stopping = true;
            interrupted_generation = generation;
            phase(shared, RuntimePhase::Stopping);
            stop_proven = domain.interrupt(StopReasons(reasons)).is_ok();
            if !stop_proven {
                shared.factory.invalidate_runtime();
                shared.faulted.store(true, Ordering::Release);
            }
        }
        // Once listener teardown begins, never call normal step again: a new
        // explicit job must use a newly registered listener in the next domain.
        let progress = if retiring.is_some() {
            if domain.has_native_ownership() {
                DomainProgress::Retained
            } else {
                DomainProgress::Idle
            }
        } else {
            match domain.step() {
                Ok(progress) => progress,
                Err(_) => {
                    shared.factory.invalidate_runtime();
                    shared.faulted.store(true, Ordering::Release);
                    stopping = true;
                    interrupted_generation = control.generation();
                    phase(shared, RuntimePhase::Stopping);
                    stop_proven = domain
                        .interrupt(StopReasons(StopReason::Fault as u32))
                        .is_ok();
                    DomainProgress::Draining
                }
            }
        };
        if progress == DomainProgress::Retained {
            shared.factory.invalidate_runtime();
            shared.faulted.store(true, Ordering::Release);
            phase(shared, RuntimePhase::Retained);
        } else if !stopping && retiring.is_none() && !shared.faulted.load(Ordering::Acquire) {
            phase(shared, RuntimePhase::Running);
        }
        if progress == DomainProgress::Idle && !domain.has_native_ownership() && stop_proven {
            if retiring.is_none() {
                let state = match shared.state.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                if shared.reasons.load(Ordering::Acquire) != 0
                    || stopping && control.generation() != interrupted_generation
                    || shared.wake_sequence.load(Ordering::Acquire) != observed_wake
                {
                    continue;
                }
                shared.needs_worker.store(false, Ordering::Release);
                // Stop callbacks do not take this mutex. A racing sequence
                // restores pending work; a later stop sets pending itself.
                if shared.wake_sequence.load(Ordering::Acquire) != observed_wake {
                    shared.needs_worker.store(true, Ordering::Release);
                    continue;
                }
                retiring = Some(std::time::Instant::now());
                drop(state);
            }
            // COM unregister and metadata/file cleanup NEVER hold the UI lock.
            match domain.finish() {
                Ok(true) => {
                    if shared.reasons.load(Ordering::Acquire) != 0
                        || stopping && control.generation() != interrupted_generation
                    {
                        continue;
                    }
                    if !shared.faulted.load(Ordering::Acquire)
                        && control.available()
                        && domain.resume_admission().is_err()
                    {
                        if shared.reasons.load(Ordering::Acquire) != 0 {
                            continue;
                        }
                        shared.factory.invalidate_runtime();
                        shared.faulted.store(true, Ordering::Release);
                        stopping = true;
                        stop_proven = false;
                        phase(shared, RuntimePhase::Stopping);
                        continue;
                    }
                    let mut state = match shared.state.lock() {
                        Ok(state) => state,
                        Err(_) => return,
                    };
                    if stopping
                        && control.generation() == interrupted_generation
                        && shared.reasons.load(Ordering::Acquire) == 0
                    {
                        state.ack_generation = interrupted_generation;
                    }
                    // The domain and native TLS destructors have not exited yet.
                    // Only reap_locked may publish Closed after the OS confirms
                    // physical termination and the join has completed.
                    state.phase = if shared.closed.load(Ordering::Acquire) {
                        RuntimePhase::Stopping
                    } else if shared.faulted.load(Ordering::Acquire) {
                        RuntimePhase::Halted
                    } else {
                        RuntimePhase::Idle
                    };
                    drop(state);
                    drop(domain);
                    return;
                }
                Ok(false) if retiring.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) => {
                    phase(shared, RuntimePhase::Stopping);
                }
                _ => {
                    shared.factory.invalidate_runtime();
                    shared.faulted.store(true, Ordering::Release);
                    phase(shared, RuntimePhase::Retained);
                }
            }
        }
        let state = match shared.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        if shared.reasons.load(Ordering::Acquire) == 0
            && shared.wake_sequence.load(Ordering::Acquire) == observed_wake
        {
            let wait = if progress == DomainProgress::Gap {
                50
            } else {
                10
            };
            drop(
                shared
                    .signal
                    .wait_timeout(state, Duration::from_millis(wait)),
            );
        }
    }
}

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::NativeFactory;
#[cfg(test)]
mod tests;
#[cfg(all(test, windows))]
mod tls_exit_tests;
