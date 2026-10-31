//! Real two-worker saturation: a full ordinary queue must delay writer-close
//! admission, rather than silently stopping the pool and releasing its owner.
use super::tests::{fixture, wait_for, SERIAL};
use super::*;
use std::sync::Condvar;

#[derive(Default)]
struct Gate {
    entered: usize,
    released: bool,
}
#[derive(Default)]
pub(super) struct Hold {
    state: Mutex<Gate>,
    changed: Condvar,
}
impl Hold {
    pub(super) fn new() -> Self {
        Self::default()
    }
    /// Deliberately ignores JobContext cancellation until the real owner has
    /// been released. A cancelled ticket cannot manufacture physical completion.
    pub(super) fn wait(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.entered += 1;
        self.changed.notify_all();
        while !state.released {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
    fn release(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.released = true;
        self.changed.notify_all();
    }
    fn wait_for_entered(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        while state.entered < expected {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("physical worker did not enter the held task");
            let (next, timeout) = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
            assert!(
                !timeout.timed_out() || state.entered >= expected,
                "physical worker did not enter the held task"
            );
        }
    }
}
struct ReleaseOnDrop(Arc<Hold>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn submit_hold(app: &App, hold: Arc<Hold>) -> JobTicket<Result<Outcome>> {
    let until = Instant::now() + Duration::from_secs(30);
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut task = Work::HoldForShutdown(hold);
    loop {
        match app.pool.handle().try_submit(task, deadline) {
            Ok(ticket) => return ticket,
            Err(rejected)
                if rejected.kind == fc_runtime::worker::Rejection::Busy
                    && Instant::now() < until =>
            {
                // try_submit also reports Busy for transient mailbox lock
                // contention. Preserve the SAME task until actual admission.
                task = rejected.task;
                std::thread::park_timeout(Duration::from_millis(1));
            }
            Err(rejected) => panic!(
                "held work admission failed: {:?}; stopping={} terminate={}",
                rejected.kind, app.stopping, app.terminate_required
            ),
        }
    }
}

#[test]
fn full_shared_queue_delays_shutdown_until_the_writer_can_really_close() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app) = fixture();
    wait_for(&mut app, |app| app.pending.is_none());
    app.audio.as_ref().unwrap().shutdown();
    wait_for(&mut app, |app| {
        app.audio.as_ref().unwrap().status().is_ok_and(|status| {
            status.phase == RuntimePhase::Closed
                && !status.physical_thread_running
                && status.ack_generation >= status.stop_generation
        }) && app
            .pool
            .handle()
            .try_stats()
            .is_some_and(|stats| stats.running == 0 && stats.queued == 0)
    });
    assert!(app
        .backend
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .state()
        .is_ok());

    let hold = Arc::new(Hold::new());
    let _release_on_failure = ReleaseOnDrop(hold.clone());
    let mut tickets = Vec::new();
    for _ in 0..fc_runtime::worker::WORKER_THREADS {
        tickets.push(submit_hold(&app, hold.clone()));
    }
    hold.wait_for_entered(fc_runtime::worker::WORKER_THREADS);
    for _ in 0..fc_runtime::worker::QUEUED_JOBS {
        tickets.push(submit_hold(&app, hold.clone()));
    }
    let stats = app
        .pool
        .handle()
        .try_stats()
        .expect("pool counters unavailable");
    assert_eq!(stats.running, fc_runtime::worker::WORKER_THREADS);
    assert_eq!(stats.queued, fc_runtime::worker::QUEUED_JOBS);

    app.begin_shutdown();
    assert!(app.stopping);
    assert!(
        app.shutdown.is_none(),
        "a full queue admitted a writer-close task"
    );
    assert!(
        !app.terminate_required,
        "ordinary queue saturation became fatal"
    );
    assert!(!app.pool.is_finished());
    let rejected = match app.pool.handle().try_submit(
        Work::HoldForShutdown(hold.clone()),
        Instant::now() + Duration::from_secs(120),
    ) {
        Ok(_) => panic!("the held queue was not really full"),
        Err(error) => error,
    };
    assert_eq!(
        rejected.kind,
        fc_runtime::worker::Rejection::Busy,
        "shutdown stopped the shared pool before it admitted writer close"
    );
    assert!(
        app.backend
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .state()
            .is_ok(),
        "writer must remain live while its closing task is waiting for admission"
    );

    hold.release();
    wait_for(&mut app, |app| app.pool.is_finished());
    assert_eq!(
        hold.state.lock().unwrap().entered,
        fc_runtime::worker::WORKER_THREADS + fc_runtime::worker::QUEUED_JOBS,
        "shutdown discarded queued owners instead of retrying close admission"
    );
    assert!(app.shutdown.is_none());
    assert!(app.writer_closed);
    assert!(!app.terminate_required);
    assert!(
        app.backend
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .state()
            .is_err(),
        "physical pool completion did not close and join the SQLite writer"
    );
    // A Windows UI pump may tick again after the close acknowledgement while
    // native thread teardown completes. Already-closed storage must not be
    // resubmitted to a stopped pool and turned into a fatal shutdown.
    for _ in 0..3 {
        app.poll().unwrap();
        assert!(app.shutdown.is_none());
        assert!(!app.terminate_required);
        assert!(app.pool.is_finished());
        assert!(app.writer_closed);
    }
    app.pool.join().unwrap();
    drop(tickets);
}
