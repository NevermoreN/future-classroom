//! One admitted native owner. Mailbox capacity and slot permits account for the
//! same four linear frames until their actual completion is collected.
use super::{Call, Driver, Frame, Step, TlsError, CONNECTION_SLOTS};
use fc_ports::http::RequestToken;
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

static ADMITTED: AtomicBool = AtomicBool::new(false);
// A failed checked native close forbids every replacement in this process,
// including after the original OS thread has actually terminated.
static CLEANUP_FAULTED: AtomicBool = AtomicBool::new(false);
// A dropped live worker retains its complete native/linear owner here. The
// process-wide admission lease bounds this quarantine to exactly one owner.
static QUARANTINE: Mutex<Option<Owner>> = Mutex::new(None);

struct Shared {
    stop: AtomicBool,
    faulted: AtomicBool,
    cleanup_error: Mutex<Option<TlsError>>,
}
impl Shared {
    fn record_cleanup_error(&self, error: TlsError) {
        CLEANUP_FAULTED.store(true, Ordering::Release);
        self.stop.store(true, Ordering::Release);
        let mut recorded = self
            .cleanup_error
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if recorded.is_none() {
            *recorded = Some(error);
        }
    }
    fn cleanup_error(&self) -> Option<TlsError> {
        *self
            .cleanup_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}
struct Owner {
    generation: u64,
    pending: SyncSender<Box<Frame>>,
    complete: Receiver<Box<Frame>>,
    ready: Receiver<Result<(), TlsError>>,
    ready_state: ReadyState,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    in_flight: [Option<RequestToken>; CONNECTION_SLOTS],
    serials: [u64; CONNECTION_SLOTS],
    bound: [Option<RequestToken>; CONNECTION_SLOTS],
    retired: [bool; CONNECTION_SLOTS],
    in_flight_calls: [Option<Call>; CONNECTION_SLOTS],
    admitted: bool,
}
#[derive(Clone, Copy)]
enum ReadyState {
    Pending,
    Ready,
    Failed(TlsError),
}
impl Owner {
    fn actual_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(actual_finished)
    }
    fn release_admission(&mut self) {
        if self.admitted && !CLEANUP_FAULTED.load(Ordering::Acquire) {
            self.admitted = false;
            ADMITTED.store(false, Ordering::Release);
        }
    }
    fn reap(&mut self) -> Result<bool, TlsError> {
        if !self.actual_finished() {
            return Ok(false);
        }
        let panicked = self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err());
        if panicked {
            // An unwind outside the explicitly guarded call/close/drop paths
            // leaves cleanup unproven even after the native thread has ended.
            self.shared.record_cleanup_error(TlsError::ThreadPanic);
        }
        if let Some(error) = self.shared.cleanup_error() {
            return Err(error);
        }
        if self.in_flight.iter().all(Option::is_none) {
            self.release_admission();
        }
        if self.shared.faulted.load(Ordering::Acquire) {
            Err(TlsError::ThreadPanic)
        } else {
            Ok(true)
        }
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        // Owner is only dropped once the actual thread is gone, or spawn failed.
        debug_assert!(self.actual_finished());
        self.release_admission();
    }
}
fn actual_finished(thread: &JoinHandle<()>) -> bool {
    #[cfg(windows)]
    {
        crate::system::thread_is_finished(thread)
    }
    #[cfg(not(windows))]
    {
        thread.is_finished()
    }
}
fn reap_quarantine() {
    let owner = QUARANTINE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    let Some(mut owner) = owner else {
        return;
    };
    // Admission stays held while temporarily outside the quarantine mutex.
    // Neither native thread queries nor a proven-terminal join run under it.
    if owner.actual_finished() {
        let _ = owner.reap();
        // Its caller abandoned these frames. Physical termination now makes
        // their final wipes safe; they were never returned as early permits.
        drop(owner);
    } else {
        let mut quarantine = QUARANTINE.lock().unwrap_or_else(|error| error.into_inner());
        debug_assert!(quarantine.is_none());
        *quarantine = Some(owner);
    }
}

pub(super) struct Worker {
    owner: Option<Owner>,
}
impl Worker {
    #[cfg(windows)]
    pub(super) fn start(
        generation: u64,
        thumbprint: super::CertificateThumbprint,
    ) -> Result<Self, TlsError> {
        Self::start_with_factory(generation, move || {
            Ok(Box::new(super::schannel::SchannelSet::new(thumbprint)?))
        })
    }
    pub(super) fn start_with_factory(
        generation: u64,
        factory: impl FnOnce() -> Result<Box<dyn Driver>, TlsError> + Send + 'static,
    ) -> Result<Self, TlsError> {
        if generation == 0 {
            return Err(TlsError::InvalidInput);
        }
        reap_quarantine();
        if CLEANUP_FAULTED.load(Ordering::Acquire) {
            return Err(TlsError::Faulted);
        }
        ADMITTED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| TlsError::Busy)?;
        let (pending, receive) = mpsc::sync_channel(CONNECTION_SLOTS);
        let (send, complete) = mpsc::sync_channel(CONNECTION_SLOTS);
        let (ready_send, ready) = mpsc::sync_channel(1);
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            faulted: AtomicBool::new(false),
            cleanup_error: Mutex::new(None),
        });
        let worker_shared = Arc::clone(&shared);
        let thread = match thread::Builder::new()
            .name("fc-tls-sspi".into())
            .stack_size(1024 * 1024)
            .spawn(move || run(factory, receive, send, ready_send, worker_shared))
        {
            Ok(thread) => thread,
            Err(_) => {
                ADMITTED.store(false, Ordering::Release);
                return Err(TlsError::Faulted);
            }
        };
        Ok(Self {
            owner: Some(Owner {
                generation,
                pending,
                complete,
                ready,
                ready_state: ReadyState::Pending,
                shared,
                thread: Some(thread),
                in_flight: [None; CONNECTION_SLOTS],
                serials: [0; CONNECTION_SLOTS],
                bound: [None; CONNECTION_SLOTS],
                retired: [false; CONNECTION_SLOTS],
                in_flight_calls: [None; CONNECTION_SLOTS],
                admitted: true,
            }),
        })
    }
    pub(super) fn poll_ready(&mut self) -> Option<Result<(), TlsError>> {
        let owner = self.owner.as_mut()?;
        if !matches!(owner.ready_state, ReadyState::Pending) {
            return None;
        }
        let result = match owner.ready.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(TlsError::ThreadPanic),
        };
        owner.ready_state = match result {
            Ok(()) => ReadyState::Ready,
            Err(error) => ReadyState::Failed(error),
        };
        Some(result)
    }
    pub(super) fn try_submit(
        &mut self,
        mut frame: Box<Frame>,
    ) -> Result<(), (TlsError, Box<Frame>)> {
        let Some(owner) = self.owner.as_mut() else {
            return Err((TlsError::Stopped, frame));
        };
        let error = match owner.ready_state {
            ReadyState::Pending => Some(TlsError::Busy),
            ReadyState::Failed(error) => Some(error),
            ReadyState::Ready => None,
        };
        if let Some(error) = error {
            return Err((error, frame));
        }
        if owner.shared.stop.load(Ordering::Acquire) {
            return Err((TlsError::Stopped, frame));
        }
        let token = frame.token;
        let slot = token.slot();
        if token.generation() != owner.generation
            || slot >= CONNECTION_SLOTS
            || token.serial() == 0
            || token.serial() < owner.serials[slot]
            || frame.input_len > frame.input.len()
        {
            return Err((TlsError::InvalidInput, frame));
        }
        if owner.in_flight[slot].is_some() {
            return Err((TlsError::Busy, frame));
        }
        let same = owner.bound[slot] == Some(token);
        if (same && owner.retired[slot])
            || (!same
                && (frame.call != Call::Handshake
                    || token.serial() <= owner.serials[slot]
                    || owner.bound[slot].is_some() && !owner.retired[slot]))
            || (matches!(frame.call, Call::Shutdown | Call::Retire) && frame.input_len != 0)
        {
            return Err((TlsError::InvalidInput, frame));
        }
        let call = frame.call;
        frame.result = None;
        frame.output_len = 0;
        match owner.pending.try_send(frame) {
            Ok(()) => {
                owner.in_flight[slot] = Some(token);
                owner.serials[slot] = token.serial();
                owner.bound[slot] = Some(token);
                owner.retired[slot] = false;
                owner.in_flight_calls[slot] = Some(call);
                Ok(())
            }
            Err(TrySendError::Full(frame)) => Err((TlsError::Busy, frame)),
            Err(TrySendError::Disconnected(frame)) => Err((TlsError::Stopped, frame)),
        }
    }
    pub(super) fn poll(&mut self) -> Result<Option<Box<Frame>>, TlsError> {
        let Some(owner) = self.owner.as_mut() else {
            return Ok(None);
        };
        let frame = match owner.complete.try_recv() {
            Ok(frame) => frame,
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) => {
                return if owner.in_flight.iter().any(Option::is_some) {
                    Err(TlsError::UnexpectedCompletion)
                } else {
                    Ok(None)
                };
            }
        };
        let slot = frame.token.slot();
        if slot >= CONNECTION_SLOTS
            || owner.in_flight[slot] != Some(frame.token)
            || frame.result.is_none()
            || owner.in_flight_calls[slot] != Some(frame.call)
            || frame.call == Call::Retire
                && frame
                    .result
                    .as_ref()
                    .is_some_and(|result| result.is_ok() && *result != Ok(Step::Retired))
        {
            owner.shared.stop.store(true, Ordering::Release);
            return Err(TlsError::UnexpectedCompletion);
        }
        owner.in_flight[slot] = None;
        owner.in_flight_calls[slot] = None;
        if frame.call == Call::Retire && frame.result == Some(Ok(Step::Retired)) {
            owner.retired[slot] = true;
        }
        if owner.thread.is_none() && owner.in_flight.iter().all(Option::is_none) {
            owner.release_admission();
        }
        Ok(Some(frame))
    }
    pub(super) fn request_stop(&self) {
        if let Some(owner) = &self.owner {
            owner.shared.stop.store(true, Ordering::Release);
        }
    }
    pub(super) fn reap_nonblocking(&mut self) -> Result<bool, TlsError> {
        match self.owner.as_mut() {
            Some(owner) => owner.reap(),
            None => Ok(true),
        }
    }
    pub(super) fn has_in_flight(&self) -> bool {
        self.owner
            .as_ref()
            .is_some_and(|owner| owner.in_flight.iter().any(Option::is_some))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.request_stop();
        let Some(mut owner) = self.owner.take() else {
            return;
        };
        if owner.actual_finished() {
            let _ = owner.reap();
            return;
        }
        let mut quarantine = QUARANTINE.lock().unwrap_or_else(|error| error.into_inner());
        // The single admission lease proves no other live/quarantined owner can
        // occupy this slot. Retain all channels, frames and the OS thread handle.
        assert!(quarantine.is_none(), "TLS admission owner must be unique");
        *quarantine = Some(owner);
    }
}

fn run(
    factory: impl FnOnce() -> Result<Box<dyn Driver>, TlsError>,
    pending: Receiver<Box<Frame>>,
    complete: SyncSender<Box<Frame>>,
    ready: SyncSender<Result<(), TlsError>>,
    shared: Arc<Shared>,
) {
    let initialized = catch_unwind(AssertUnwindSafe(factory));
    let mut driver = match initialized {
        Ok(Ok(driver)) => driver,
        Ok(Err(error)) => {
            if matches!(error, TlsError::Cleanup(_)) {
                shared.record_cleanup_error(error);
            }
            shared.stop.store(true, Ordering::Release);
            let _ = ready.try_send(Err(error));
            return;
        }
        Err(_) => {
            // No Driver was returned, so partially initialized native resources
            // have no successful checked-close proof. Never replace this owner.
            shared.record_cleanup_error(TlsError::ThreadPanic);
            let _ = ready.try_send(Err(TlsError::ThreadPanic));
            return;
        }
    };
    if ready.try_send(Ok(())).is_err() {
        shared.stop.store(true, Ordering::Release);
    }
    loop {
        let mut frame = match pending.recv_timeout(Duration::from_millis(10)) {
            Ok(frame) => frame,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if shared.stop.load(Ordering::Acquire) {
                    break;
                }
                continue;
            }
        };
        let token = frame.token;
        let call = frame.call;
        let result = if shared.stop.load(Ordering::Acquire) {
            Err(TlsError::Stopped)
        } else {
            match catch_unwind(AssertUnwindSafe(|| driver.call(&mut frame))) {
                Ok(result) => result,
                Err(_) => {
                    shared.stop.store(true, Ordering::Release);
                    shared.faulted.store(true, Ordering::Release);
                    Err(TlsError::ThreadPanic)
                }
            }
        };
        if let Err(error @ TlsError::Cleanup(_)) = result {
            shared.record_cleanup_error(error);
        }
        frame.result = Some(
            if frame.token == token && frame.call == call && frame.output_len <= frame.output.len()
            {
                result
            } else {
                shared.stop.store(true, Ordering::Release);
                frame.token = token;
                frame.call = call;
                Err(TlsError::UnexpectedCompletion)
            },
        );
        // At most four accepted frames exist across queue, native call and
        // result queue. Each completion therefore has a reserved result slot.
        if complete.try_send(frame).is_err() {
            shared.stop.store(true, Ordering::Release);
            shared.faulted.store(true, Ordering::Release);
            break;
        }
    }
    // No native context is destroyed while Driver::call is executing. Checked
    // native cleanup runs on its owning thread and never under a shared lock.
    let cleanup = catch_unwind(AssertUnwindSafe(|| driver.close()));
    match cleanup {
        Ok(Ok(())) => {}
        Ok(Err(error)) => shared.record_cleanup_error(error),
        Err(_) => shared.record_cleanup_error(TlsError::ThreadPanic),
    }
    // Its final drop also stays on this thread; a failed checked close remains
    // a sticky admission fault even though the actual OS thread can terminate.
    if catch_unwind(AssertUnwindSafe(|| drop(driver))).is_err() {
        shared.record_cleanup_error(TlsError::ThreadPanic);
    }
}

#[cfg(test)]
pub(in crate::tls_frontdoor) mod tests;
