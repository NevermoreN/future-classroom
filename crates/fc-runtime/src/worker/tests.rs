use super::*;
use std::{
    sync::{atomic::AtomicUsize, mpsc},
    time::Duration,
};

enum Action {
    Value(usize),
    Kdf {
        started: mpsc::Sender<thread::ThreadId>,
        release: mpsc::Receiver<()>,
    },
    Material {
        started: mpsc::Sender<thread::ThreadId>,
        release: mpsc::Receiver<()>,
    },
    Cooperative {
        started: mpsc::Sender<thread::ThreadId>,
        release: mpsc::Receiver<()>,
    },
    Panic,
}
enum Accounting {
    Actual,
    Overflow,
    Panic,
}
struct Work {
    action: Action,
    bytes: Vec<u8>,
    accounting: Accounting,
    runs: Arc<AtomicUsize>,
}
impl Task for Work {
    type Output = usize;
    fn allocation_bytes(&self) -> Option<usize> {
        match self.accounting {
            Accounting::Actual => Some(self.bytes.capacity()),
            Accounting::Overflow => Some(usize::MAX),
            Accounting::Panic => panic!("diagnostic descriptor panic"),
        }
    }
    fn run(self, context: &JobContext) -> usize {
        self.runs.fetch_add(1, Ordering::AcqRel);
        match self.action {
            Action::Value(value) => value,
            Action::Kdf { started, release } => {
                let _ = started.send(thread::current().id());
                let _ = release.recv();
                1
            }
            Action::Material { started, release } => {
                let _ = started.send(thread::current().id());
                let _ = release.recv();
                2
            }
            Action::Cooperative { started, release } => {
                let _ = started.send(thread::current().id());
                let _ = release.recv();
                if context.checkpoint().is_ok() {
                    3
                } else {
                    0
                }
            }
            Action::Panic => panic!("diagnostic worker panic"),
        }
    }
}
fn work(action: Action) -> Work {
    Work {
        action,
        bytes: Vec::new(),
        accounting: Accounting::Actual,
        runs: Arc::new(AtomicUsize::new(0)),
    }
}
fn value(n: usize) -> Work {
    work(Action::Value(n))
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}
fn submit(handle: &WorkerHandle<Work>, task: Work) -> JobTicket<usize> {
    let until = Instant::now() + Duration::from_secs(3);
    let mut task = task;
    loop {
        match handle.try_submit(task, deadline()) {
            Ok(ticket) => return ticket,
            Err(error) if error.kind == Rejection::Busy && Instant::now() < until => {
                task = error.task;
                thread::yield_now();
            }
            Err(error) => panic!("unexpected rejection {:?}", error.kind),
        }
    }
}
fn started(receive: &mpsc::Receiver<thread::ThreadId>) -> thread::ThreadId {
    receive
        .recv_timeout(Duration::from_secs(3))
        .expect("physical worker starts")
}
fn gate(material: bool) -> (Work, mpsc::Receiver<thread::ThreadId>, mpsc::Sender<()>) {
    let (started, receive) = mpsc::channel();
    let (release, send) = mpsc::channel();
    let action = if material {
        Action::Material {
            started,
            release: send,
        }
    } else {
        Action::Kdf {
            started,
            release: send,
        }
    };
    (work(action), receive, release)
}
fn result(ticket: &mut JobTicket<usize>) -> Result<usize, JobError> {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        match ticket.try_result() {
            Ok(Some(output)) => return Ok(output),
            Err(error) => return Err(error),
            Ok(None) => {
                assert!(Instant::now() < until, "terminal ticket timeout");
                thread::yield_now();
            }
        }
    }
}
fn stats(handle: &WorkerHandle<Work>) -> PoolStats {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(stats) = handle.try_stats() {
            return stats;
        }
        assert!(Instant::now() < until);
        thread::yield_now();
    }
}
#[test]
fn two_physical_workers_are_shared_by_mixed_jobs_and_eight_more_may_queue() {
    let pool = WorkerPool::<Work>::start().unwrap();
    let handle = pool.handle();
    let (kdf, seen1, release1) = gate(false);
    let (material, seen2, release2) = gate(true);
    let mut first = submit(&handle, kdf);
    let mut second = submit(&handle, material);
    let thread1 = started(&seen1);
    let thread2 = started(&seen2);
    assert_ne!(thread1, thread2);
    let mut queued: Vec<_> = (0..QUEUED_JOBS)
        .map(|n| submit(&handle, value(n + 10)))
        .collect();
    assert_eq!(stats(&handle).queued, 8);
    assert_eq!(stats(&handle).running, 2);
    let error = match handle.try_submit(value(90), deadline()) {
        Err(error) => error,
        Ok(_) => panic!("ninth queued job accepted"),
    };
    assert_eq!(error.kind, Rejection::Busy);
    assert!(matches!(error.task.action, Action::Value(90)));
    assert_eq!(release1.send(()), Ok(()));
    assert_eq!(release2.send(()), Ok(()));
    assert_eq!(result(&mut first), Ok(1));
    assert_eq!(result(&mut second), Ok(2));
    for (n, ticket) in queued.iter_mut().enumerate() {
        assert_eq!(result(ticket), Ok(n + 10));
    }
    pool.join().unwrap();
}
#[test]
fn descriptor_budget_uses_capacity_and_rejections_return_owned_secrets() {
    let pool = WorkerPool::<Work>::start().unwrap();
    let handle = pool.handle();
    let (first, seen1, release1) = gate(false);
    let (second, seen2, release2) = gate(true);
    let _first = submit(&handle, first);
    let _second = submit(&handle, second);
    started(&seen1);
    started(&seen2);
    let mut owned = value(1);
    owned.bytes = Vec::with_capacity(QUEUED_BYTES - std::mem::size_of::<Job<Work>>());
    let mut queued = submit(&handle, owned);
    assert_eq!(stats(&handle).queued_bytes, QUEUED_BYTES);
    let mut extra = value(2);
    extra.bytes = Vec::with_capacity(32);
    extra.bytes.extend_from_slice(b"owned-secret-fixture");
    let error = match handle.try_submit(extra, deadline()) {
        Err(error) => error,
        Ok(_) => panic!("byte budget exceeded"),
    };
    assert_eq!(error.kind, Rejection::Busy);
    assert_eq!(error.task.bytes, b"owned-secret-fixture");
    assert!(error.task.bytes.capacity() >= 32);
    let mut too_large = value(3);
    too_large.bytes = Vec::with_capacity(QUEUED_BYTES);
    assert!(matches!(
        handle.try_submit(too_large, deadline()),
        Err(SubmitError {
            kind: Rejection::TooLarge,
            ..
        })
    ));
    let mut overflow = value(4);
    overflow.accounting = Accounting::Overflow;
    assert!(matches!(
        handle.try_submit(overflow, deadline()),
        Err(SubmitError {
            kind: Rejection::TooLarge,
            ..
        })
    ));
    release1.send(()).unwrap();
    release2.send(()).unwrap();
    assert_eq!(result(&mut queued), Ok(1));
    pool.join().unwrap();
}
#[test]
fn cancellation_and_stop_do_not_free_running_slots_or_start_replacements() {
    let pool = WorkerPool::<Work>::start().unwrap();
    let handle = pool.handle();
    let (first, seen1, release1) = gate(false);
    let (second, seen2, release2) = gate(true);
    let mut first = submit(&handle, first);
    let mut second = submit(&handle, second);
    started(&seen1);
    started(&seen2);
    let pending_runs = Arc::new(AtomicUsize::new(0));
    let mut pending = value(3);
    pending.runs = pending_runs.clone();
    let mut pending = submit(&handle, pending);
    first.cancel();
    pending.cancel();
    assert_eq!(stats(&handle).running, 2);
    assert!(!first.is_finished());
    assert!(!pool.is_finished());
    let before = Instant::now();
    pool.request_stop();
    assert!(before.elapsed() < Duration::from_millis(100));
    assert!(!pool.is_finished());
    assert_eq!(stats(&handle).running, 2);
    assert!(matches!(
        handle.try_submit(value(4), deadline()),
        Err(SubmitError {
            kind: Rejection::Stopped,
            ..
        })
    ));
    release1.send(()).unwrap();
    release2.send(()).unwrap();
    assert_eq!(result(&mut first), Err(JobError::Stopped));
    assert_eq!(result(&mut second), Err(JobError::Stopped));
    assert_eq!(result(&mut pending), Err(JobError::Stopped));
    assert_eq!(pending_runs.load(Ordering::Acquire), 0);
    pool.join().unwrap();
}
#[test]
fn queued_cancel_and_expired_deadline_skip_user_code_and_post_run_cancel_discards_output() {
    let pool = WorkerPool::<Work>::start().unwrap();
    let handle = pool.handle();
    let (first, seen1, release1) = gate(false);
    let (second, seen2, release2) = gate(true);
    let _first = submit(&handle, first);
    let _second = submit(&handle, second);
    started(&seen1);
    started(&seen2);
    let cancelled_runs = Arc::new(AtomicUsize::new(0));
    let mut task = value(9);
    task.runs = cancelled_runs.clone();
    let mut cancelled = submit(&handle, task);
    cancelled.cancel();
    let due = Instant::now() + Duration::from_millis(20);
    let mut expired = loop {
        match handle.try_submit(value(10), due) {
            Ok(ticket) => break ticket,
            Err(e) if e.kind == Rejection::Busy => thread::yield_now(),
            Err(e) => panic!("{:?}", e.kind),
        }
    };
    while Instant::now() < due {
        thread::yield_now();
    }
    release1.send(()).unwrap();
    release2.send(()).unwrap();
    assert_eq!(result(&mut cancelled), Err(JobError::Cancelled));
    assert_eq!(cancelled_runs.load(Ordering::Acquire), 0);
    assert_eq!(result(&mut expired), Err(JobError::Deadline));
    assert!(matches!(
        handle.try_submit(value(1), Instant::now()),
        Err(SubmitError {
            kind: Rejection::Deadline,
            ..
        })
    ));
    let (started_tx, seen) = mpsc::channel();
    let (release, receive) = mpsc::channel();
    let mut running = submit(
        &handle,
        work(Action::Cooperative {
            started: started_tx,
            release: receive,
        }),
    );
    started(&seen);
    running.cancel();
    release.send(()).unwrap();
    assert_eq!(result(&mut running), Err(JobError::Cancelled));
    pool.join().unwrap();
}
#[test]
fn task_panic_faults_entire_pool_and_suppresses_other_live_results() {
    let pool = WorkerPool::<Work>::start().unwrap();
    let handle = pool.handle();
    let (first, seen, release) = gate(false);
    let mut first = submit(&handle, first);
    started(&seen);
    let mut panic = submit(&handle, work(Action::Panic));
    let outcome = result(&mut panic);
    assert_eq!(outcome, Err(JobError::Panicked));
    assert!(pool.is_faulted());
    assert_eq!(stats(&handle).running, 1);
    assert!(!pool.is_finished());
    assert!(matches!(
        handle.try_submit(value(3), deadline()),
        Err(SubmitError {
            kind: Rejection::Faulted,
            ..
        })
    ));
    release.send(()).unwrap();
    assert_eq!(result(&mut first), Err(JobError::Faulted));
    assert_eq!(pool.join(), Err(JoinError::Faulted));
}
#[test]
fn completion_notification_and_ticket_polling_are_nonblocking_and_one_shot() {
    let notifications = Arc::new(AtomicUsize::new(0));
    let observed = notifications.clone();
    let pool = WorkerPool::<Work>::start_with_notifier(Arc::new(move || {
        observed.fetch_add(1, Ordering::AcqRel);
    }))
    .unwrap();
    let handle = pool.handle();
    let (task, seen, release) = gate(false);
    let mut ticket = submit(&handle, task);
    started(&seen);
    assert_eq!(ticket.try_result(), Ok(None));
    assert!(!ticket.is_finished());
    release.send(()).unwrap();
    assert_eq!(result(&mut ticket), Ok(1));
    assert_eq!(ticket.try_result(), Err(JobError::Consumed));
    pool.join().unwrap();
    assert_eq!(notifications.load(Ordering::Acquire), 1);
    assert_eq!(handle.completion_generation(), 1);
}
#[test]
fn descriptor_and_notification_panics_fault_closed_without_unwinding_caller() {
    let pool = WorkerPool::<Work>::start().unwrap();
    let handle = pool.handle();
    let mut task = value(1);
    task.accounting = Accounting::Panic;
    assert!(matches!(
        handle.try_submit(task, deadline()),
        Err(SubmitError {
            kind: Rejection::Faulted,
            ..
        })
    ));
    assert_eq!(pool.join(), Err(JoinError::Faulted));
    let pool =
        WorkerPool::<Work>::start_with_notifier(Arc::new(|| panic!("diagnostic wakeup panic")))
            .unwrap();
    let handle = pool.handle();
    let mut ticket = submit(&handle, value(1));
    let until = Instant::now() + Duration::from_secs(3);
    while !handle.is_faulted() {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    assert_eq!(result(&mut ticket), Err(JobError::Faulted));
    assert_eq!(pool.join(), Err(JoinError::Faulted));
}
#[test]
fn pool_drop_signals_stop_and_detaches_instead_of_joining_event_loop() {
    let pool = WorkerPool::<Work>::start().unwrap();
    let handle = pool.handle();
    let (task, seen, release) = gate(false);
    let mut ticket = submit(&handle, task);
    started(&seen);
    let before = Instant::now();
    drop(pool);
    assert!(before.elapsed() < Duration::from_millis(100));
    assert!(!ticket.is_finished());
    assert!(matches!(
        handle.try_submit(value(2), deadline()),
        Err(SubmitError {
            kind: Rejection::Stopped,
            ..
        })
    ));
    release.send(()).unwrap();
    assert_eq!(result(&mut ticket), Err(JobError::Stopped));
}

#[test]
fn completed_but_unobserved_output_cannot_cross_cancel_or_stop() {
    let pool = WorkerPool::<Work>::start().unwrap();
    let handle = pool.handle();
    let mut cancelled = submit(&handle, value(1));
    let mut stopped = submit(&handle, value(2));
    let until = Instant::now() + Duration::from_secs(3);
    while !cancelled.is_finished() || !stopped.is_finished() {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    cancelled.cancel();
    assert_eq!(result(&mut cancelled), Err(JobError::Cancelled));
    pool.request_stop();
    assert_eq!(result(&mut stopped), Err(JobError::Stopped));
    pool.join().unwrap();
}
