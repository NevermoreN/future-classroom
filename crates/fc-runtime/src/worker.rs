//! Shared, fixed two-thread computation domain for KDF and material preparation.
//! Compose heterogeneous work as one Task enum; do not create a pool per feature.
//! Jobs prepare owned results only. Authorization/SQLite/UI side effects belong to
//! their respective domains, which must revalidate authority before applying output.
use crate::{
    cancel::{CancellationSource, CancellationToken},
    mailbox::{
        self, MailboxMessage, MailboxReceiver, MailboxSender, ReceiveError, SubmitErrorKind,
    },
};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc, OnceLock,
    },
    thread::{self, JoinHandle, Thread},
    time::Instant,
};

pub const WORKER_THREADS: usize = 2;
pub const QUEUED_JOBS: usize = mailbox::WORKER_CAPACITY;
pub const QUEUED_BYTES: usize = mailbox::DESCRIPTOR_BYTE_BUDGET;

pub trait Task: Send + 'static {
    type Output: Send + 'static;
    /// Actual owned heap capacity, including nested buffers and secrets. Inline Task
    /// storage is counted automatically. None rejects arithmetic overflow.
    fn allocation_bytes(&self) -> Option<usize>;
    /// Cooperatively check context between bounded work chunks. Cancellation does
    /// not kill threads or free a running CPU slot. Secret buffers stay owned here.
    fn run(self, context: &JobContext) -> Self::Output;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobError {
    Cancelled,
    Deadline,
    Stopped,
    Faulted,
    Panicked,
    Disconnected,
    Consumed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Busy,
    TooLarge,
    Stopped,
    Faulted,
    Deadline,
}
/// No Debug implementation: a rejected job may still own passwords/file bytes.
pub struct SubmitError<T> {
    pub kind: Rejection,
    pub task: T,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinError {
    Faulted,
    ThreadPanicked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolStats {
    pub queued: usize,
    pub queued_bytes: usize,
    pub running: usize,
}

struct Control {
    stop: AtomicBool,
    fault: AtomicBool,
    running: AtomicUsize,
    completions: AtomicU64,
    threads: OnceLock<Vec<Thread>>,
    notify: Arc<dyn Fn() + Send + Sync>,
}
impl Control {
    fn wake(&self) {
        if let Some(threads) = self.threads.get() {
            for t in threads {
                t.unpark();
            }
        }
    }
    fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.wake();
    }
    fn fault(&self) {
        self.fault.store(true, Ordering::Release);
        self.stop();
    }
    fn error(&self) -> Option<JobError> {
        if self.fault.load(Ordering::Acquire) {
            Some(JobError::Faulted)
        } else if self.stop.load(Ordering::Acquire) {
            Some(JobError::Stopped)
        } else {
            None
        }
    }
    fn completed(&self) {
        let _ = self
            .completions
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                Some(n.saturating_add(1))
            });
        // A completion hook must only post a non-blocking wakeup. Never run UI,
        // application, database or KDF work here. Hook panic closes this domain.
        if catch_unwind(AssertUnwindSafe(|| (self.notify)())).is_err() {
            self.fault();
        }
    }
}
pub struct JobContext {
    control: Arc<Control>,
    cancellation: CancellationToken,
    deadline: Instant,
}
impl JobContext {
    pub fn checkpoint(&self) -> Result<(), JobError> {
        Self::check_execution(&self.control, &self.cancellation, self.deadline)
    }
    fn check_execution(
        control: &Control,
        cancellation: &CancellationToken,
        deadline: Instant,
    ) -> Result<(), JobError> {
        if let Some(error) = control.error() {
            return Err(error);
        }
        if cancellation.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(JobError::Deadline);
        }
        Ok(())
    }
    /// Memory-only lifetime check for the final execution owner (for example a
    /// SQLite precommit callback). Carries the same pool stop/fault, cancellation
    /// token and monotonic deadline; it grants no actor or publication authority.
    pub fn execution_fence(&self) -> impl Fn() -> bool + Send + Sync + 'static {
        let control = self.control.clone();
        let cancellation = self.cancellation.clone();
        let deadline = self.deadline;
        move || Self::check_execution(&control, &cancellation, deadline).is_ok()
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }
}
struct Completion<O> {
    reply: SyncSender<Result<O, JobError>>,
    done: Arc<AtomicBool>,
    control: Arc<Control>,
    armed: bool,
}
impl<O> Completion<O> {
    fn finish(&mut self, result: Result<O, JobError>) {
        if !self.armed || self.done.swap(true, Ordering::AcqRel) {
            return;
        }
        // Exactly one terminal result fits in the one-slot reply channel. This call
        // never blocks, even when the ticket has already been abandoned.
        let _ = self.reply.try_send(result);
        self.control.completed();
    }
}
impl<O> Drop for Completion<O> {
    fn drop(&mut self) {
        if self.armed && !self.done.load(Ordering::Acquire) {
            self.control.fault();
            self.finish(Err(JobError::Panicked));
        }
    }
}
struct Job<T: Task> {
    task: Option<T>,
    heap_bytes: usize,
    context: JobContext,
    completion: Completion<T::Output>,
}
impl<T: Task> MailboxMessage for Job<T> {
    fn heap_bytes(&self) -> Option<usize> {
        Some(self.heap_bytes)
    }
}
impl<T: Task> Job<T> {
    fn reject(mut self, error: JobError) {
        self.completion.finish(Err(error));
    }
    fn execute(mut self) {
        if let Err(error) = self.context.checkpoint() {
            self.completion.finish(Err(error));
            return;
        }
        let Some(task) = self.task.take() else {
            self.context.control.fault();
            self.completion.finish(Err(JobError::Faulted));
            return;
        };
        let running = Running(self.context.control.clone());
        running.0.running.fetch_add(1, Ordering::AcqRel);
        let result = catch_unwind(AssertUnwindSafe(|| task.run(&self.context)));
        drop(running);
        match result {
            Ok(output) => match self.context.checkpoint() {
                Ok(()) => self.completion.finish(Ok(output)),
                Err(error) => self.completion.finish(Err(error)),
            },
            Err(_) => {
                self.context.control.fault();
                self.completion.finish(Err(JobError::Panicked));
            }
        }
    }
}
struct Running(Arc<Control>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Shared<T: Task> {
    control: Arc<Control>,
    receive: MailboxReceiver<Job<T>>,
}
impl<T: Task> Shared<T> {
    fn stop(&self) {
        self.receive.close();
        self.control.stop();
    }
    fn fault(&self) {
        self.receive.close();
        self.control.fault();
    }
}
/// Cancellation is a separate atomic signal and cannot be delayed by a full queue.
pub struct JobTicket<O> {
    receive: Option<Receiver<Result<O, JobError>>>,
    cancellation: CancellationSource,
    done: Arc<AtomicBool>,
    control: Arc<Control>,
}
impl<O> JobTicket<O> {
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
    pub fn is_finished(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
    pub fn try_result(&mut self) -> Result<Option<O>, JobError> {
        let receive = self.receive.as_ref().ok_or(JobError::Consumed)?;
        match receive.try_recv() {
            Ok(result) => {
                self.receive = None;
                if matches!(&result, Err(JobError::Panicked)) {
                    return Err(JobError::Panicked);
                }
                if let Some(error) = self.control.error() {
                    return Err(error);
                }
                if self.cancellation.generation() != 0 {
                    return Err(JobError::Cancelled);
                }
                result.map(Some)
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.receive = None;
                Err(self.control.error().unwrap_or(JobError::Disconnected))
            }
        }
    }
    /// Blocking convenience for headless diagnostics/shutdown coordinators only.
    pub fn wait(mut self) -> Result<O, JobError> {
        let receive = self.receive.take().ok_or(JobError::Consumed)?;
        let result = receive
            .recv()
            .map_err(|_| self.control.error().unwrap_or(JobError::Disconnected))?;
        if matches!(&result, Err(JobError::Panicked)) {
            return Err(JobError::Panicked);
        }
        if let Some(error) = self.control.error() {
            return Err(error);
        }
        if self.cancellation.generation() != 0 {
            return Err(JobError::Cancelled);
        }
        result
    }
}
impl<O> Drop for JobTicket<O> {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub struct WorkerHandle<T: Task> {
    send: MailboxSender<Job<T>>,
    shared: Arc<Shared<T>>,
}
impl<T: Task> Clone for WorkerHandle<T> {
    fn clone(&self) -> Self {
        Self {
            send: self.send.clone(),
            shared: self.shared.clone(),
        }
    }
}
impl<T: Task> WorkerHandle<T> {
    pub fn try_submit(
        &self,
        task: T,
        deadline: Instant,
    ) -> Result<JobTicket<T::Output>, SubmitError<T>> {
        let rejection = match self.shared.control.error() {
            Some(JobError::Faulted) => Some(Rejection::Faulted),
            Some(_) => Some(Rejection::Stopped),
            None => None,
        };
        if let Some(kind) = rejection {
            return Err(SubmitError { kind, task });
        }
        if Instant::now() >= deadline {
            return Err(SubmitError {
                kind: Rejection::Deadline,
                task,
            });
        }
        let heap_bytes = match catch_unwind(AssertUnwindSafe(|| task.allocation_bytes())) {
            Ok(Some(bytes))
                if bytes
                    .checked_add(std::mem::size_of::<Job<T>>())
                    .is_some_and(|bytes| bytes <= QUEUED_BYTES) =>
            {
                bytes
            }
            Ok(_) => {
                return Err(SubmitError {
                    kind: Rejection::TooLarge,
                    task,
                })
            }
            Err(_) => {
                self.shared.fault();
                return Err(SubmitError {
                    kind: Rejection::Faulted,
                    task,
                });
            }
        };
        let cancellation = CancellationSource::new();
        let done = Arc::new(AtomicBool::new(false));
        let (reply, receive) = mpsc::sync_channel(1);
        let job = Job {
            task: Some(task),
            heap_bytes,
            context: JobContext {
                control: self.shared.control.clone(),
                cancellation: cancellation.token(),
                deadline,
            },
            completion: Completion {
                reply,
                done: done.clone(),
                control: self.shared.control.clone(),
                armed: true,
            },
        };
        match self.send.try_send(job) {
            Ok(()) => {
                self.shared.control.wake();
                Ok(JobTicket {
                    receive: Some(receive),
                    cancellation,
                    done,
                    control: self.shared.control.clone(),
                })
            }
            Err(error) => {
                let mut job = error.message;
                job.completion.armed = false;
                // This private Option is populated by the constructor and no worker
                // can access a message returned by the mailbox on rejected submission.
                let task = match job.task.take() {
                    Some(task) => task,
                    None => unreachable!("rejected job retains its task"),
                };
                let kind = if self.shared.control.fault.load(Ordering::Acquire) {
                    Rejection::Faulted
                } else {
                    match error.kind {
                        SubmitErrorKind::TooLarge => Rejection::TooLarge,
                        SubmitErrorKind::Closed => Rejection::Stopped,
                        SubmitErrorKind::Faulted => {
                            self.shared.fault();
                            Rejection::Faulted
                        }
                        SubmitErrorKind::Busy => Rejection::Busy,
                    }
                };
                Err(SubmitError { kind, task })
            }
        }
    }
    pub fn request_stop(&self) {
        self.shared.stop();
    }
    pub fn is_faulted(&self) -> bool {
        self.shared.control.fault.load(Ordering::Acquire)
    }
    /// Monotonic saturating notification generation; polling never consumes events.
    pub fn completion_generation(&self) -> u64 {
        self.shared.control.completions.load(Ordering::Acquire)
    }
    pub fn try_stats(&self) -> Option<PoolStats> {
        self.send.try_stats().map(|q| PoolStats {
            queued: q.items,
            queued_bytes: q.bytes,
            running: self.shared.control.running.load(Ordering::Acquire),
        })
    }
}

pub struct WorkerPool<T: Task> {
    handle: WorkerHandle<T>,
    threads: Vec<JoinHandle<()>>,
}
impl<T: Task> WorkerPool<T> {
    pub fn start() -> Result<Self, std::io::Error> {
        Self::start_with_notifier(Arc::new(|| {}))
    }
    /// Startup only. The hook executes on a worker and must only post a non-blocking
    /// notification. Pool creation/Drop never starts a replacement worker after fault.
    pub fn start_with_notifier(
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, std::io::Error> {
        let (send, receive) = mailbox::bounded(QUEUED_JOBS, QUEUED_BYTES)
            .map_err(|_| std::io::Error::other("invalid fixed worker budget"))?;
        let control = Arc::new(Control {
            stop: AtomicBool::new(false),
            fault: AtomicBool::new(false),
            running: AtomicUsize::new(0),
            completions: AtomicU64::new(0),
            threads: OnceLock::new(),
            notify,
        });
        let shared = Arc::new(Shared { control, receive });
        let mut threads: Vec<JoinHandle<()>> = Vec::with_capacity(WORKER_THREADS);
        for index in 0..WORKER_THREADS {
            let worker = shared.clone();
            match thread::Builder::new()
                .name(format!("fc-worker-{}", index + 1))
                .spawn(move || worker_loop(worker))
            {
                Ok(thread) => threads.push(thread),
                Err(error) => {
                    shared.stop();
                    // No task can be submitted before construction returns. Wake and
                    // detach only these empty startup threads; never block an event loop.
                    for thread in &threads {
                        thread.thread().unpark();
                    }
                    return Err(error);
                }
            }
        }
        let _ = shared
            .control
            .threads
            .set(threads.iter().map(|t| t.thread().clone()).collect());
        shared.control.wake();
        Ok(Self {
            handle: WorkerHandle { send, shared },
            threads,
        })
    }
    pub fn handle(&self) -> WorkerHandle<T> {
        self.handle.clone()
    }
    pub fn request_stop(&self) {
        self.handle.request_stop();
    }
    /// True only after both physical threads have exited, not merely after cancel.
    pub fn is_finished(&self) -> bool {
        self.threads.iter().all(JoinHandle::is_finished)
    }
    pub fn is_faulted(&self) -> bool {
        self.handle.is_faulted()
    }
    /// FINAL shutdown on a non-UI coordinator. Potentially unbounded for a task that
    /// ignores cancellation; poll is_finished and apply the app shutdown deadline first.
    pub fn join(mut self) -> Result<(), JoinError> {
        self.request_stop();
        let mut panicked = false;
        for thread in self.threads.drain(..) {
            panicked |= thread.join().is_err();
        }
        if panicked {
            Err(JoinError::ThreadPanicked)
        } else if self.is_faulted() {
            Err(JoinError::Faulted)
        } else {
            Ok(())
        }
    }
}
impl<T: Task> Drop for WorkerPool<T> {
    fn drop(&mut self) {
        self.request_stop();
    }
}

fn worker_loop<T: Task>(shared: Arc<Shared<T>>) {
    loop {
        let state = shared.control.error();
        match shared.receive.try_recv() {
            Ok(job) => {
                // Destructors can also panic. Completion's RAII guard ensures the
                // ticket becomes terminal, then this worker continues draining safely.
                if catch_unwind(AssertUnwindSafe(|| match state {
                    Some(error) => job.reject(error),
                    None => job.execute(),
                }))
                .is_err()
                {
                    shared.fault();
                }
                if shared.control.fault.load(Ordering::Acquire) {
                    shared.receive.close();
                }
            }
            Err(ReceiveError::Empty) => {
                if state.is_some() {
                    return;
                }
                thread::park();
            }
            Err(ReceiveError::Busy) => thread::yield_now(),
            Err(ReceiveError::Closed) => return,
            Err(ReceiveError::Faulted) => {
                shared.fault();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
