//! Actual shared-worker ownership checked by the actual SQLite pre-COMMIT path.
//! These fixtures use synthetic credentials, private databases and injected civil
//! time only. They do not issue network requests or change machine clock/state.
use fc_application::identity::SessionLease;
use fc_bootstrap::backend::{Backend, BackendError, Initialize};
use fc_domain::{calendar::*, ids::*, time::*};
use fc_ports::{calendar::*, Clock, MonotonicClock};
use fc_runtime::{
    health::Health,
    worker::{JobContext, JobError, JobTicket, Task, WorkerPool},
};
use fc_security::{CredentialRecordCodec, Password};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
static CREDENTIAL_FIXTURE: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "FutureClassroom-CalendarExecution-R20-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let r = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            r.unwrap();
        }
    }
}
fn at(day: u8) -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 10, day).unwrap(), 28800).unwrap()
}
struct Time(Mutex<LocalDateTime>);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        *self.0.lock().unwrap()
    }
}
impl MonotonicClock for Time {
    fn monotonic_millis(&self) -> u64 {
        0
    }
}
fn candidate(day: u8) -> CalendarCandidate {
    CalendarCandidate {
        source_year: 2026,
        fetched_at: at(1),
        source_sha256: [day; 32],
        papers: vec!["https://www.gov.cn/zhengce/content/2025-11/01/content_1234567.htm".into()],
        days: vec![PublicCalendarDay {
            date: LocalDate::new(2026, 10, day).unwrap(),
            kind: PublicDayKind::Rest,
            name: "synthetic holiday".into(),
        }],
        etag: Some(format!("\"{day}\"")),
        last_modified: None,
    }
}
#[derive(Clone, Copy)]
enum Trip {
    Cancel,
    Deadline,
}
enum Operation {
    Reserve,
    Policy,
    Adopt,
    Finish(CalendarFetchTicket),
}
struct Facts {
    calls: u64,
    cancelled: bool,
    status: Option<CalendarStatus>,
}
struct Work {
    backend: Arc<Mutex<Backend>>,
    lease: SessionLease,
    operation: Operation,
    trip: Trip,
    ticket: Arc<Mutex<Option<JobTicket<()>>>>,
    start: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Facts>>>,
}
impl Task for Work {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(match &self.operation {
            Operation::Finish(t) => t
                .etag
                .as_ref()
                .map_or(0, String::capacity)
                .checked_add(t.last_modified.as_ref().map_or(0, String::capacity))?,
            _ => 0,
        })
    }
    fn run(self, context: &JobContext) {
        while !self.start.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let calls = Arc::new(AtomicU64::new(0));
        let observed = calls.clone();
        let cell = self.ticket.clone();
        let trip = self.trip;
        let deadline = context.deadline();
        let actual = context.execution_fence();
        // This test-owned wrapper delays/cancels exactly the second check: the
        // writer has evaluated and written SQL, and is checking before COMMIT.
        // The decision is still made by the genuine JobContext execution fence.
        let fence = Arc::new(move || {
            if observed.fetch_add(1, Ordering::AcqRel) == 1 {
                match trip {
                    Trip::Cancel => cell.lock().unwrap().as_ref().unwrap().cancel(),
                    Trip::Deadline => {
                        while Instant::now() < deadline {
                            std::thread::yield_now();
                        }
                    }
                }
            }
            actual()
        });
        let mut backend = self.backend.lock().unwrap();
        let mut status = None;
        let result = match self.operation {
            Operation::Reserve => backend
                .reserve_calendar_fetch_while(self.lease, 2026, fence)
                .map(|_| ()),
            Operation::Policy => backend
                .set_calendar_auto_policy_while(self.lease, Revision::default(), true, fence)
                .map(|_| ()),
            Operation::Adopt => backend
                .adopt_calendar_while(
                    self.lease,
                    Revision::default(),
                    vec![CalendarCandidateRef {
                        year: 2026,
                        revision: Revision::new(1),
                    }],
                    fence,
                )
                .map(|_| ()),
            Operation::Finish(ticket) => backend
                .finish_calendar_fetch_while(
                    ticket,
                    CalendarFetchOutcome::Downloaded(candidate(2)),
                    fence,
                )
                .map(|value| {
                    status = Some(value);
                }),
        };
        let cancelled = match result {
            Err(BackendError::Calendar(CalendarRepositoryError::Cancelled)) => true,
            Ok(()) => false,
            Err(e) => panic!("unexpected fixture result: {e:?}"),
        };
        *self.result.lock().unwrap() = Some(Facts {
            calls: calls.load(Ordering::Acquire),
            cancelled,
            status,
        });
    }
}
struct Fixture {
    pool: Option<WorkerPool<Work>>,
    backend: Arc<Mutex<Backend>>,
    clock: Arc<Time>,
    lease: SessionLease,
    _directory: Directory,
}
impl Fixture {
    fn new() -> Self {
        let directory = Directory::new();
        let clock = Arc::new(Time(Mutex::new(at(10))));
        let mut backend = Backend::open(
            directory.0.join("state.sqlite3"),
            CredentialRecordCodec::from_key([69; 32]).unwrap(),
            clock.clone(),
            clock.clone(),
        )
        .unwrap();
        let password = || Password::new("Synthetic-calendar-execution-2026".into()).unwrap();
        drop(
            backend
                .initialize(
                    Initialize {
                        school_name: "Synthetic calendar execution".into(),
                        login_name: "admin".into(),
                        display_name: "Fixture administrator".into(),
                    },
                    password(),
                )
                .unwrap(),
        );
        let lease = backend.login("admin", password()).unwrap();
        Self {
            pool: Some(WorkerPool::start().unwrap()),
            backend: Arc::new(Mutex::new(backend)),
            clock,
            lease,
            _directory: directory,
        }
    }
    fn reserve(&self) -> CalendarFetchTicket {
        match self
            .backend
            .lock()
            .unwrap()
            .reserve_calendar_fetch(self.lease, 2026)
            .unwrap()
        {
            CalendarReservation::Reserved(t) => t,
            _ => panic!("unexpected rate limit"),
        }
    }
    fn cache(&self) {
        let ticket = self.reserve();
        self.backend
            .lock()
            .unwrap()
            .finish_calendar_fetch(ticket, CalendarFetchOutcome::Downloaded(candidate(1)))
            .unwrap();
    }
    fn status(&self) -> CalendarStatus {
        self.backend
            .lock()
            .unwrap()
            .calendar_status(self.lease, 2026)
            .unwrap()
    }
    fn run(&self, operation: Operation, trip: Trip) -> Facts {
        let cell = Arc::new(Mutex::new(None));
        let start = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let deadline = Instant::now()
            + Duration::from_secs(match trip {
                Trip::Cancel => 10,
                Trip::Deadline => 2,
            });
        let task = Work {
            backend: self.backend.clone(),
            lease: self.lease,
            operation,
            trip,
            ticket: cell.clone(),
            start: start.clone(),
            result: result.clone(),
        };
        let ticket = self
            .pool
            .as_ref()
            .unwrap()
            .handle()
            .try_submit(task, deadline)
            .unwrap_or_else(|e| panic!("worker submission: {:?}", e.kind));
        *cell.lock().unwrap() = Some(ticket);
        start.store(true, Ordering::Release);
        while !cell.lock().unwrap().as_ref().unwrap().is_finished() {
            assert!(
                Instant::now() < deadline + Duration::from_secs(10),
                "fixture worker has not physically returned"
            );
            std::thread::park_timeout(Duration::from_millis(1));
        }
        let ticket = cell.lock().unwrap().take().unwrap();
        let completion = ticket.wait();
        assert_eq!(
            completion,
            Err(match trip {
                Trip::Cancel => JobError::Cancelled,
                Trip::Deadline => JobError::Deadline,
            })
        );
        let facts = result.lock().unwrap().take().unwrap();
        assert_eq!(
            facts.calls, 2,
            "ownership must remain valid at evaluation and expire exactly before COMMIT"
        );
        facts
    }
    fn healthy(&self) {
        assert_eq!(
            self.backend
                .lock()
                .unwrap()
                .session_control()
                .health_gate()
                .status()
                .health,
            Health::Healthy
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            let r = pool.join();
            if !std::thread::panicking() {
                r.unwrap();
            }
        }
        let r = self
            .backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .close();
        if !std::thread::panicking() {
            r.unwrap();
        }
    }
}
#[test]
fn real_worker_cancel_at_precommit_blocks_reservation_policy_and_adoption() {
    let _serial = CREDENTIAL_FIXTURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    assert!(f.run(Operation::Reserve, Trip::Cancel).cancelled);
    assert!(f.status().years.is_empty());
    f.cache();
    assert!(f.run(Operation::Policy, Trip::Cancel).cancelled);
    assert_eq!(f.status().policy, CalendarAutoPolicy::default());
    assert!(f.run(Operation::Adopt, Trip::Cancel).cancelled);
    assert_eq!(
        f.backend
            .lock()
            .unwrap()
            .read_schedule(f.lease)
            .unwrap()
            .revision,
        Revision::default()
    );
    f.healthy();
}
fn assert_cancelled_finish(f: &mut Fixture, trip: Trip) {
    f.cache();
    *f.clock.0.lock().unwrap() = at(11);
    // The durable 24-hour interval deliberately outlives the old local lease.
    // Obtain a real fresh authenticated session; never extend a fixture actor.
    f.lease = f
        .backend
        .lock()
        .unwrap()
        .login(
            "admin",
            Password::new("Synthetic-calendar-execution-2026".into()).unwrap(),
        )
        .unwrap();
    let ticket = f.reserve();
    let facts = f.run(Operation::Finish(ticket), trip);
    assert!(
        !facts.cancelled,
        "Backend must settle a rolled-back attempt with a separate failure transaction"
    );
    let status = facts.status.unwrap();
    assert_eq!(
        status.years[0].last_error,
        Some(CalendarFetchFailure::Cancelled)
    );
    assert!(!status.years[0].pending);
    assert_eq!(status.years[0].candidate_revision, Revision::new(1));
    assert_eq!(status.years[0].next_allowed, Some(at(12)));
    assert_eq!(status.candidate.unwrap().source_sha256, [1; 32]);
    assert_eq!(f.status().candidate.unwrap().source_sha256, [1; 32]);
    assert_eq!(
        f.backend
            .lock()
            .unwrap()
            .read_schedule(f.lease)
            .unwrap()
            .revision,
        Revision::default()
    );
    f.healthy();
}
#[test]
fn real_worker_cancel_cannot_publish_late_body_and_same_ticket_failure_settles() {
    let _serial = CREDENTIAL_FIXTURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut f = Fixture::new();
    assert_cancelled_finish(&mut f, Trip::Cancel);
}
#[test]
fn real_worker_deadline_at_precommit_cannot_publish_late_body() {
    let _serial = CREDENTIAL_FIXTURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut f = Fixture::new();
    assert_cancelled_finish(&mut f, Trip::Deadline);
}
#[test]
fn real_worker_deadline_at_adoption_precommit_preserves_exact_original_cas() {
    let _serial = CREDENTIAL_FIXTURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = Fixture::new();
    f.cache();
    assert!(f.run(Operation::Adopt, Trip::Deadline).cancelled);
    assert_eq!(
        f.backend
            .lock()
            .unwrap()
            .read_schedule(f.lease)
            .unwrap()
            .revision,
        Revision::default()
    );
    let adopted = f
        .backend
        .lock()
        .unwrap()
        .adopt_calendar(
            f.lease,
            Revision::default(),
            vec![CalendarCandidateRef {
                year: 2026,
                revision: Revision::new(1),
            }],
        )
        .unwrap();
    assert_eq!(adopted.revision, Revision::new(1));
    f.healthy();
}
