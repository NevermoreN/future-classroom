use crate::{
    reinitialization::{ColdReinitialization, ReinitializationBackend, ReinitializationError},
    reinitialization_files::{
        PreparedReinitialization, ReinitializationFileError, ResetCommitReceipt,
    },
    voice_runtime::VoiceRuntime,
};
use fc_runtime::worker::{Task, WorkerPool};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReinitializationWorkflowError {
    Core(ReinitializationError),
    Files(ReinitializationFileError),
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReinitializationPhase {
    Waiting,
    Draining,
    Committing,
    Retained,
    Finished,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReinitializationStatus {
    pub phase: ReinitializationPhase,
    pub cancellation_requested: bool,
    pub result: Option<Result<ResetCommitReceipt, ReinitializationWorkflowError>>,
}
struct Inputs<T: Task> {
    prepared: PreparedReinitialization,
    backend: ReinitializationBackend,
    workers: WorkerPool<T>,
    audio: Option<VoiceRuntime>,
}
struct State<T: Task> {
    input: Mutex<Option<Inputs<T>>>,
    wake: Condvar,
    abandoned: AtomicBool,
    cancel: AtomicBool,
    panicked: AtomicBool,
    status: Mutex<ReinitializationStatus>,
}
pub struct PreparedReinitializationCoordinator<T: Task> {
    state: Arc<State<T>>,
    thread: Option<JoinHandle<()>>,
}
pub struct ReinitializationOperation<T: Task> {
    state: Arc<State<T>>,
    thread: Option<JoinHandle<()>>,
}
impl<T: Task> PreparedReinitializationCoordinator<T> {
    /// Create the thread before moving physical owners out of the native App.
    /// Spawn failure leaves the prepared files, pool, proof and audio untouched.
    pub fn prepare() -> std::io::Result<Self> {
        let state = Arc::new(State {
            input: Mutex::new(None),
            wake: Condvar::new(),
            abandoned: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            panicked: AtomicBool::new(false),
            status: Mutex::new(ReinitializationStatus {
                phase: ReinitializationPhase::Waiting,
                cancellation_requested: false,
                result: None,
            }),
        });
        let owned = state.clone();
        let thread = thread::Builder::new()
            .name("fc-reinitialize-close".into())
            .spawn(move || run(owned))?;
        Ok(Self {
            state,
            thread: Some(thread),
        })
    }
    pub fn start(
        mut self,
        prepared: PreparedReinitialization,
        backend: ReinitializationBackend,
        workers: WorkerPool<T>,
        audio: Option<VoiceRuntime>,
    ) -> ReinitializationOperation<T> {
        *self
            .state
            .input
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Inputs {
            prepared,
            backend,
            workers,
            audio,
        });
        self.state.wake.notify_one();
        ReinitializationOperation {
            state: self.state.clone(),
            thread: self.thread.take(),
        }
    }
}
impl<T: Task> Drop for PreparedReinitializationCoordinator<T> {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.state.abandoned.store(true, Ordering::Release);
            self.state.wake.notify_one();
        }
    }
}
impl<T: Task> ReinitializationOperation<T> {
    pub fn cancel(&self) {
        self.state.cancel.store(true, Ordering::Release);
    }
    pub fn status(&self) -> ReinitializationStatus {
        let mut status = self
            .state
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        status.cancellation_requested = self.state.cancel.load(Ordering::Acquire);
        status
    }
    /// A published result does not permit pump exit until the OS thread itself,
    /// including native TLS teardown, has terminated and this join has completed.
    pub fn reap_nonblocking(&mut self) -> bool {
        if self.state.panicked.load(Ordering::Acquire) {
            return false;
        }
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| !crate::physical_thread::is_finished(thread))
        {
            return false;
        }
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                self.state.panicked.store(true, Ordering::Release);
                let mut status = self
                    .state
                    .status
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                status.phase = ReinitializationPhase::Retained;
                status.result = None;
                return false;
            }
        }
        true
    }
}
impl<T: Task> Drop for ReinitializationOperation<T> {
    fn drop(&mut self) {
        // The independent coordinator still owns every input. The App must retain
        // and reap its operation; cancellation never releases a physical slot.
        self.state.cancel.store(true, Ordering::Release);
    }
}
fn phase<T: Task>(state: &State<T>, phase: ReinitializationPhase) {
    state
        .status
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .phase = phase;
}
fn run<T: Task>(state: Arc<State<T>>) {
    let mut input = state
        .input
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    while input.is_none() && !state.abandoned.load(Ordering::Acquire) {
        input = state
            .wake
            .wait(input)
            .unwrap_or_else(|error| error.into_inner());
    }
    let Some(Inputs {
        prepared,
        backend,
        workers,
        audio,
    }) = input.take()
    else {
        return;
    };
    drop(input);
    phase(&state, ReinitializationPhase::Draining);
    let mut drain = ColdReinitialization::begin(
        prepared.root().to_owned(),
        prepared.sequence(),
        backend,
        workers,
        audio,
    );
    let permit = loop {
        match drain.poll() {
            Ok(Some(permit)) => break Ok(permit),
            Ok(None) => {
                if drain.is_retained() {
                    phase(&state, ReinitializationPhase::Retained);
                }
            }
            Err(error) => break Err(ReinitializationWorkflowError::Core(error)),
        }
        thread::park_timeout(Duration::from_millis(10));
    };
    // This fence belongs to THIS coordinator. It is independent of the stopped
    // workers' cancellation/generation, and remains alive throughout file commit.
    let check = || !state.cancel.load(Ordering::Acquire);
    let result = match permit {
        Err(error) => {
            // Physical owners have settled; retire only this unarmed work area
            // on the final coordinator. Failure preserves it for startup repair.
            let _ = prepared.discard(&|| true);
            Err(error)
        }
        Ok(_) if !check() => match prepared.discard(&|| true) {
            Ok(()) => Err(ReinitializationWorkflowError::Cancelled),
            Err(error) => Err(ReinitializationWorkflowError::Files(error)),
        },
        Ok(permit) => {
            phase(&state, ReinitializationPhase::Committing);
            prepared
                .commit_cold(permit, &check)
                .map_err(ReinitializationWorkflowError::Files)
        }
    };
    let mut status = state
        .status
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    status.phase = ReinitializationPhase::Finished;
    status.result = Some(result);
}
