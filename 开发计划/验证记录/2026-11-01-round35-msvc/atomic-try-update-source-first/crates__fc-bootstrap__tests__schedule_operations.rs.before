//! Real Backend/SQLite publication boundaries for native timetable restructuring.
//! Draft operations are pure; only an exact local lease and document CAS publish.
use fc_application::identity::{SessionError, SessionLease};
use fc_bootstrap::backend::{Backend, BackendError, Initialize, SessionControl};
use fc_domain::{
    identity::{IdentityCommand, Rgb},
    ids::*,
    schedule::*,
    schedule_document::*,
    time::*,
};
use fc_ports::{Clock, MonotonicClock, ScheduleRepositoryError};
use fc_security::{CredentialRecordCodec, Password};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::ThreadId,
    time::{SystemTime, UNIX_EPOCH},
};

// Actual PBKDF admission is process-wide. Other integration binaries have their
// own process; only this binary's credential-bearing fixtures share this mutex.
static CREDENTIAL_FIXTURE: Mutex<()> = Mutex::new(());
static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const PASSWORD: &str = "Synthetic-schedule-operation-2026";

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "FutureClassroom-ScheduleOperations-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn database(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

struct TestClock {
    caller: ThreadId,
    writer_reads_until_lock: AtomicU64,
    lock_fired: AtomicBool,
    lock: Mutex<Option<SessionControl>>,
}
impl TestClock {
    fn new() -> Self {
        Self {
            caller: std::thread::current().id(),
            writer_reads_until_lock: AtomicU64::new(0),
            lock_fired: AtomicBool::new(false),
            lock: Mutex::new(None),
        }
    }
    fn revoke_after_evaluation(&self, control: SessionControl) {
        *self.lock.lock().unwrap() = Some(control);
        self.lock_fired.store(false, Ordering::Release);
        // SaveSchedule samples writer wall time for session synchronization,
        // actor, management authorization, transition, then changed_at. The
        // fifth sample is after successful transition and before its final
        // precommit authorization. UI-thread actor reads do not count.
        self.writer_reads_until_lock.store(5, Ordering::Release);
    }
}
impl Clock for TestClock {
    fn now(&self) -> LocalDateTime {
        if std::thread::current().id() != self.caller
            && self
                .writer_reads_until_lock
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
                == Ok(1)
        {
            self.lock.lock().unwrap().as_ref().unwrap().lock_local();
            self.lock_fired.store(true, Ordering::Release);
        }
        LocalDateTime::new(date(30), 3600).unwrap()
    }
}
impl MonotonicClock for TestClock {
    fn monotonic_millis(&self) -> u64 {
        0
    }
}
struct Joined(Backend);
impl Drop for Joined {
    fn drop(&mut self) {
        let result = self.0.close();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn open(directory: &Directory, clock: Arc<TestClock>) -> Joined {
    Joined(
        Backend::open(
            directory.database(),
            CredentialRecordCodec::from_key([39; 32]).unwrap(),
            clock.clone(),
            clock,
        )
        .unwrap(),
    )
}
fn password() -> Password {
    Password::new(PASSWORD.into()).unwrap()
}
fn initialize(backend: &mut Backend) -> SessionLease {
    drop(
        backend
            .initialize(
                Initialize {
                    school_name: "Synthetic schedule operations".into(),
                    login_name: "admin".into(),
                    display_name: "Fixture administrator".into(),
                },
                password(),
            )
            .unwrap(),
    );
    let lease = backend.login("admin", password()).unwrap();
    for command in [
        IdentityCommand::CreateClass {
            id: class(),
            name: "Synthetic class".into(),
        },
        IdentityCommand::PutSubject {
            id: SubjectId::new(1).unwrap(),
            name: "Synthetic math".into(),
            enabled: true,
            color: Rgb {
                red: 40,
                green: 80,
                blue: 160,
            },
            expected_revision: None,
        },
    ] {
        backend.identity(lease, command).unwrap();
    }
    lease
}
fn date(day: u8) -> LocalDate {
    LocalDate::new(2026, 10, day).unwrap()
}
fn class() -> ClassId {
    ClassId::new(1).unwrap()
}
fn semester() -> SemesterId {
    SemesterId::new(1).unwrap()
}
fn table(id: u64) -> TableId {
    TableId::new(id).unwrap()
}
fn row(id: u64) -> RowId {
    RowId::new(id).unwrap()
}
fn dual_document() -> ScheduleDocument {
    let mut details = Vec::new();
    let mut cells = Vec::new();
    let tables = [(2, TableRole::Odd, 10), (3, TableRole::Even, 20)]
        .into_iter()
        .map(|(tid, role, rid)| {
            let course = TimetableRow {
                id: row(rid),
                logical_key: LogicalRowKey::new(100).unwrap(),
                group: DayGroup::Morning,
                kind: RowKind::Course,
                range: MinuteRange::new(
                    Minute::from_hm(8, 0).unwrap(),
                    Minute::from_hm(9, 0).unwrap(),
                )
                .unwrap(),
                enabled_days: [true, false, true, false, true, false, false],
                courses: std::array::from_fn(|day| {
                    [0, 2, 4].contains(&day).then(|| CourseCell {
                        subject_id: SubjectId::new(1).unwrap(),
                        teacher: TeacherBinding::Unassigned,
                    })
                }),
            };
            let activity = TimetableRow {
                id: row(rid + 1),
                logical_key: LogicalRowKey::new(101).unwrap(),
                group: DayGroup::Morning,
                kind: RowKind::Activity,
                range: MinuteRange::new(
                    Minute::from_hm(9, 15).unwrap(),
                    Minute::from_hm(9, 30).unwrap(),
                )
                .unwrap(),
                enabled_days: [false, true, false, true, false, true, false],
                courses: std::array::from_fn(|_| None),
            };
            details.extend([
                RowDetails {
                    row_id: course.id,
                    name: format!("{role:?} course"),
                },
                RowDetails {
                    row_id: activity.id,
                    name: format!("{role:?} assembly"),
                },
            ]);
            for day in [Weekday::Monday, Weekday::Wednesday, Weekday::Friday] {
                cells.push(CellDetails {
                    row_id: course.id,
                    day,
                    room: format!("{role:?} room"),
                    note: format!("{role:?} private note"),
                });
            }
            WeekTable {
                id: table(tid),
                role,
                rows: vec![course, activity],
            }
        })
        .collect();
    ScheduleDocument {
        semesters: vec![SemesterPlan {
            semester: Semester {
                id: semester(),
                class_id: class(),
                revision: Revision::default(),
                enabled: true,
                start: date(1),
                end: date(31),
                first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
                tables,
            },
            name: "Synthetic dual term".into(),
            timing_sync: true,
            dual_history: None,
            row_details: details,
            cell_details: cells,
        }],
        ..ScheduleDocument::default()
    }
}
fn selected_table(document: &ScheduleDocument, day: u8) -> WeekTable {
    match document.resolve_day(class(), date(day)).unwrap() {
        AcademicDay::Scheduled(day) => day.table,
        other => panic!("expected scheduled day, got {other:?}"),
    }
}
type DailyRow = (RowKind, DayGroup, MinuteRange, Option<CourseCell>, String);
fn daily_content(document: &ScheduleDocument) -> Vec<Vec<DailyRow>> {
    (12..26)
        .map(|day| {
            let table = selected_table(document, day);
            let weekday = date(day).weekday().index();
            table
                .rows
                .iter()
                .filter(|row| row.enabled_days[weekday])
                .map(|row| {
                    let name = document.semesters[0]
                        .row_details
                        .iter()
                        .find(|detail| detail.row_id == row.id)
                        .map_or_else(String::new, |detail| detail.name.clone());
                    (
                        row.kind,
                        row.group,
                        row.range,
                        row.courses[weekday].clone(),
                        name,
                    )
                })
                .collect()
        })
        .collect()
}
fn assert_retired(result: Result<ScheduleDocument, BackendError>) {
    assert!(matches!(
        result,
        Err(BackendError::Schedule(ScheduleRepositoryError::Domain(
            ScheduleDocumentError {
                kind: DocumentErrorKind::RetiredIdentity,
                ..
            }
        )))
    ));
}

#[test]
fn public_calendar_real_lease_cache_review_adoption_manual_priority_and_offline_reopen() {
    use fc_ports::calendar::*;
    let _credentials = CREDENTIAL_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    let directory = Directory::new();
    let clock = Arc::new(TestClock::new());
    let mut service = open(&directory, clock.clone());
    let lease = initialize(&mut service.0);
    let mut draft = dual_document();
    draft.public_calendar.entries.push(CalendarEntry {
        actual_semester_id: semester(),
        date: date(14),
        mapping: DayMapping::SourceDate(date(12)),
    });
    let initial = service
        .0
        .save_schedule(lease, Revision::default(), draft)
        .unwrap();
    let CalendarReservation::Reserved(ticket) =
        service.0.reserve_calendar_fetch(lease, 2026).unwrap()
    else {
        panic!("first fetch must reserve");
    };
    let feed = br#"{"year":2026,"papers":["https://www.gov.cn/zhengce/content/202501/content_1000000.htm"],"days":[{"date":"2026-10-14","name":"synthetic rest","isOffDay":true},{"date":"2026-10-15","name":"synthetic makeup","isOffDay":false}]}"#;
    let candidate = fc_protocol::calendar_feed::parse(2026, feed, clock.now(), None, None).unwrap();
    let status = service
        .0
        .finish_calendar_fetch(ticket, CalendarFetchOutcome::Downloaded(candidate))
        .unwrap();
    assert!(!status.policy.enabled);
    assert_eq!(service.0.read_schedule(lease).unwrap(), initial);
    let reference = CalendarCandidateRef {
        year: 2026,
        revision: status.years[0].candidate_revision,
    };
    service.0.session_control().lock_local();
    assert!(service
        .0
        .adopt_calendar(lease, initial.revision, vec![reference])
        .is_err());
    assert!(service.0.calendar_status(lease, 2026).is_err());
    let lease = service.0.login("admin", password()).unwrap();
    assert_eq!(service.0.calendar_status(lease, 2026).unwrap(), status);
    assert_eq!(service.0.read_schedule(lease).unwrap(), initial);
    let adopted = service
        .0
        .adopt_calendar(lease, initial.revision, vec![reference])
        .unwrap();
    assert_eq!(
        adopted.public_calendar.entries,
        initial.public_calendar.entries
    );
    assert_eq!(adopted.public_calendar_data.years.len(), 1);
    assert_eq!(
        adopted.public_calendar.mapping_revision,
        initial.public_calendar.mapping_revision
    );
    assert_eq!(adopted.school_calendar, initial.school_calendar);
    assert!(matches!(
        adopted.resolve_day(class(), date(14)).unwrap(),
        AcademicDay::Scheduled(_)
    ));
    assert!(matches!(
        adopted.resolve_day(class(), date(15)).unwrap(),
        AcademicDay::Unconfirmed
    ));
    assert!(matches!(
        service
            .0
            .adopt_calendar(lease, initial.revision, vec![reference]),
        Err(BackendError::Calendar(CalendarRepositoryError::Stale))
    ));
    let global = service.0.global_revision().unwrap();
    let mut forged = adopted.clone();
    forged.public_calendar_data.years.clear();
    assert!(service
        .0
        .save_schedule(lease, adopted.revision, forged)
        .is_err());
    assert_eq!(service.0.global_revision().unwrap(), global);
    drop(service);
    let mut service = open(&directory, clock);
    let lease = service.0.login("admin", password()).unwrap();
    assert_eq!(service.0.read_schedule(lease).unwrap(), adopted);
    assert!(service
        .0
        .calendar_status(lease, 2026)
        .unwrap()
        .candidate
        .is_some());
    assert!(matches!(
        service.0.reserve_calendar_fetch(lease, 2026).unwrap(),
        CalendarReservation::RateLimited { .. }
    ));
}

#[test]
fn role_swap_publishes_selection_only_and_survives_restart_with_document_cas() {
    let _credentials = CREDENTIAL_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    let directory = Directory::new();
    let clock = Arc::new(TestClock::new());
    let mut service = open(&directory, clock.clone());
    let lease = initialize(&mut service.0);
    let initial = service
        .0
        .save_schedule(lease, Revision::default(), dual_document())
        .unwrap();
    let identity_revision = service.0.state().unwrap().revision;
    assert_eq!(selected_table(&initial, 12).id, table(2));
    assert_eq!(selected_table(&initial, 19).id, table(3));
    let draft = initial.swap_table_roles(semester()).unwrap();
    assert_eq!(service.0.read_schedule(lease).unwrap(), initial);
    let saved = service
        .0
        .save_schedule(lease, initial.revision, draft.clone())
        .unwrap();
    assert_eq!(selected_table(&saved, 12).id, table(3));
    assert_eq!(selected_table(&saved, 19).id, table(2));
    assert_eq!(
        saved.semesters[0].row_details,
        initial.semesters[0].row_details
    );
    assert_eq!(
        saved.semesters[0].cell_details,
        initial.semesters[0].cell_details
    );
    for (before, after) in initial.semesters[0]
        .semester
        .tables
        .iter()
        .zip(&saved.semesters[0].semester.tables)
    {
        assert_eq!(before.id, after.id);
        assert_eq!(before.rows, after.rows);
        assert_ne!(before.role, after.role);
    }
    let global_revision = service.0.global_revision().unwrap();
    assert!(matches!(
        service.0.save_schedule(lease, initial.revision, draft),
        Err(BackendError::Schedule(ScheduleRepositoryError::Domain(
            ScheduleDocumentError {
                kind: DocumentErrorKind::RevisionConflict { current },
                ..
            }
        ))) if current == saved.revision
    ));
    assert_eq!(service.0.global_revision().unwrap(), global_revision);
    assert_eq!(service.0.state().unwrap().revision, identity_revision);
    drop(service);
    let mut service = open(&directory, clock);
    let lease = service.0.login("admin", password()).unwrap();
    assert_eq!(service.0.read_schedule(lease).unwrap(), saved);
    assert_eq!(service.0.global_revision().unwrap(), global_revision);
}

#[test]
fn activity_split_and_merge_preserve_both_week_contents_and_retire_all_replaced_rows() {
    let _credentials = CREDENTIAL_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    let directory = Directory::new();
    let clock = Arc::new(TestClock::new());
    let mut service = open(&directory, clock.clone());
    let lease = initialize(&mut service.0);
    let initial = service
        .0
        .save_schedule(lease, Revision::default(), dual_document())
        .unwrap();
    let expected_content = daily_content(&initial);
    let draft = initial
        .split_activity(semester(), table(2), row(11))
        .unwrap();
    assert_eq!(daily_content(&draft), expected_content);
    assert_eq!(service.0.read_schedule(lease).unwrap(), initial);
    let split = service
        .0
        .save_schedule(lease, initial.revision, draft)
        .unwrap();
    for (old, new) in initial.semesters[0]
        .semester
        .tables
        .iter()
        .zip(&split.semesters[0].semester.tables)
    {
        assert_eq!(new.rows.len(), 4);
        assert_eq!(new.rows[0], old.rows[0]);
        for activity in new.rows.iter().filter(|row| row.kind == RowKind::Activity) {
            assert!(activity.id.get() > initial.id_high_water);
            assert!(activity.logical_key.get() > initial.id_high_water);
        }
    }
    assert_eq!(
        split.semesters[0].cell_details,
        initial.semesters[0].cell_details
    );
    drop(service);
    let mut service = open(&directory, clock);
    let lease = service.0.login("admin", password()).unwrap();
    assert_eq!(service.0.read_schedule(lease).unwrap(), split);
    let ids: Vec<_> = split.semesters[0].semester.tables[0]
        .rows
        .iter()
        .filter(|row| row.kind == RowKind::Activity)
        .map(|row| row.id)
        .collect();
    let merged_draft = split.merge_activities(semester(), table(2), &ids).unwrap();
    assert_eq!(daily_content(&merged_draft), expected_content);
    let merged = service
        .0
        .save_schedule(lease, split.revision, merged_draft)
        .unwrap();
    assert_eq!(daily_content(&merged), expected_content);
    for table in &merged.semesters[0].semester.tables {
        assert_eq!(table.rows.len(), 2);
        assert!(table.rows[1].id.get() > split.id_high_water);
        assert!(table.rows[1].logical_key.get() > split.id_high_water);
    }
    // Neither the original shared identity nor any intermediate physical or
    // logical identity may be resurrected by a forged full-document Save.
    for (old_physical, old_logical) in std::iter::once((row(11), LogicalRowKey::new(101).unwrap()))
        .chain(
            split.semesters[0].semester.tables[0]
                .rows
                .iter()
                .filter(|row| row.kind == RowKind::Activity)
                .map(|row| (row.id, row.logical_key)),
        )
    {
        let mut forged = merged.clone();
        let current_id = forged.semesters[0].semester.tables[0].rows[1].id;
        forged.semesters[0].semester.tables[0].rows[1].id = old_physical;
        forged.semesters[0]
            .row_details
            .iter_mut()
            .filter(|detail| detail.row_id == current_id)
            .for_each(|detail| detail.row_id = old_physical);
        assert_retired(service.0.save_schedule(lease, merged.revision, forged));
        let mut forged = merged.clone();
        for table in &mut forged.semesters[0].semester.tables {
            table.rows[1].logical_key = old_logical;
        }
        assert_retired(service.0.save_schedule(lease, merged.revision, forged));
    }
    assert_eq!(service.0.read_schedule(lease).unwrap(), merged);
}

#[test]
fn conversion_requires_explicit_save_clears_both_weeks_and_cannot_cross_revoked_lease() {
    let _credentials = CREDENTIAL_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    let directory = Directory::new();
    let clock = Arc::new(TestClock::new());
    let mut service = open(&directory, clock.clone());
    let lease = initialize(&mut service.0);
    let initial = service
        .0
        .save_schedule(lease, Revision::default(), dual_document())
        .unwrap();
    let draft = initial
        .convert_row_kind(semester(), table(2), row(10), RowKind::Activity)
        .unwrap();
    assert!(draft.semesters[0].cell_details.is_empty());
    for (old, new) in initial.semesters[0]
        .semester
        .tables
        .iter()
        .zip(&draft.semesters[0].semester.tables)
    {
        assert_eq!(new.rows[0].id, old.rows[0].id);
        assert_ne!(new.rows[0].logical_key, old.rows[0].logical_key);
        assert_eq!(new.rows[0].kind, RowKind::Activity);
        assert!(new.rows[0].courses.iter().all(Option::is_none));
        assert_eq!(new.rows[1], old.rows[1]);
    }
    assert_eq!(service.0.read_schedule(lease).unwrap(), initial);
    // A rejected full draft does not partly publish its cleared cells or metadata.
    let mut invalid = draft.clone();
    invalid.semesters[0].semester.end = date(1);
    invalid.semesters[0].semester.start = date(31);
    assert!(service
        .0
        .save_schedule(lease, initial.revision, invalid)
        .is_err());
    assert_eq!(service.0.read_schedule(lease).unwrap(), initial);
    let before = service.0.global_revision().unwrap();
    service.0.session_control().lock_local();
    assert_eq!(
        service
            .0
            .save_schedule(lease, initial.revision, draft.clone()),
        Err(BackendError::Session(SessionError::Revoked)),
    );
    let lease = service.0.login("admin", password()).unwrap();
    assert_eq!(service.0.read_schedule(lease).unwrap(), initial);
    clock.revoke_after_evaluation(service.0.session_control());
    assert!(service
        .0
        .save_schedule(lease, initial.revision, draft.clone())
        .is_err());
    assert!(clock.lock_fired.load(Ordering::Acquire));
    assert_eq!(service.0.global_revision().unwrap(), before);
    let lease = service.0.login("admin", password()).unwrap();
    assert_eq!(service.0.read_schedule(lease).unwrap(), initial);
    let saved = service
        .0
        .save_schedule(lease, initial.revision, draft)
        .unwrap();
    assert!(saved.semesters[0].cell_details.is_empty());
    assert_eq!(
        saved.semesters[0].row_details,
        initial.semesters[0].row_details
    );
    drop(service);
    let mut service = open(&directory, clock);
    let lease = service.0.login("admin", password()).unwrap();
    assert_eq!(service.0.read_schedule(lease).unwrap(), saved);
    // Converting back deliberately creates empty courses, never restores old
    // private notes or resurrects the retired logical lesson identity.
    let back = saved
        .convert_row_kind(semester(), table(2), row(10), RowKind::Course)
        .unwrap();
    let back = service
        .0
        .save_schedule(lease, saved.revision, back)
        .unwrap();
    assert!(back.semesters[0].cell_details.is_empty());
    assert!(back.semesters[0]
        .semester
        .tables
        .iter()
        .all(|table| table.rows[0].courses.iter().all(Option::is_none)));
    assert!(
        back.semesters[0].semester.tables[0].rows[0]
            .logical_key
            .get()
            > saved.id_high_water
    );
}

#[test]
fn every_published_identity_namespace_stays_retired_after_full_term_replacement_and_restart() {
    let _credentials = CREDENTIAL_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
    let directory = Directory::new();
    let clock = Arc::new(TestClock::new());
    let mut service = open(&directory, clock.clone());
    let lease = initialize(&mut service.0);
    let initial = service
        .0
        .save_schedule(lease, Revision::default(), dual_document())
        .unwrap();
    let mut fresh = initial.clone();
    fresh.semesters[0].semester.id = SemesterId::new(1000).unwrap();
    for table in &mut fresh.semesters[0].semester.tables {
        table.id = TableId::new(table.id.get() + 1000).unwrap();
        for row in &mut table.rows {
            row.id = RowId::new(row.id.get() + 1000).unwrap();
            row.logical_key = LogicalRowKey::new(row.logical_key.get() + 1000).unwrap();
        }
    }
    for detail in &mut fresh.semesters[0].row_details {
        detail.row_id = RowId::new(detail.row_id.get() + 1000).unwrap();
    }
    for detail in &mut fresh.semesters[0].cell_details {
        detail.row_id = RowId::new(detail.row_id.get() + 1000).unwrap();
    }
    // Forged counter metadata cannot exhaust the real persisted allocator.
    fresh.id_high_water = u64::MAX;
    let saved = service
        .0
        .save_schedule(lease, initial.revision, fresh)
        .unwrap();
    assert_eq!(saved.id_high_water, 1101);
    drop(service);
    let mut service = open(&directory, clock);
    let lease = service.0.login("admin", password()).unwrap();
    let current = service.0.read_schedule(lease).unwrap();
    assert_eq!(current, saved);
    let global = service.0.global_revision().unwrap();
    for namespace in 0..4 {
        let mut forged = current.clone();
        forged.id_high_water = 0;
        match namespace {
            0 => forged.semesters[0].semester.id = semester(),
            1 => forged.semesters[0].semester.tables[0].id = table(2),
            2 => {
                forged.semesters[0].semester.tables[0].rows[0].id = row(10);
                for detail in &mut forged.semesters[0].row_details {
                    if detail.row_id == row(1010) {
                        detail.row_id = row(10);
                    }
                }
                for detail in &mut forged.semesters[0].cell_details {
                    if detail.row_id == row(1010) {
                        detail.row_id = row(10);
                    }
                }
            }
            3 => {
                for table in &mut forged.semesters[0].semester.tables {
                    table.rows[0].logical_key = LogicalRowKey::new(100).unwrap();
                }
            }
            _ => unreachable!(),
        }
        assert_retired(service.0.save_schedule(lease, current.revision, forged));
        assert_eq!(service.0.read_schedule(lease).unwrap(), current);
        assert_eq!(service.0.global_revision().unwrap(), global);
    }
    let mut next = current.clone();
    assert_eq!(next.allocate_id().unwrap(), 1102);
}
