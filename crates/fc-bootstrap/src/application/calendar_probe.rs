//! Hidden native calendar workflow against protected synthetic SQLite data.
//! The fixture civil clock advances before the UI session is issued, allowing a
//! real second rate-limited fetch while the same later UI review remains open.
//! Machine time, real holidays, foreground input and system settings are untouched.
use super::*;
use calendar_editor as c;
use fc_domain::{
    calendar::CalendarCandidate,
    schedule::{
        AcademicDay, CalendarEntry, CalendarMode, DayMapping, Semester, TableRole, WeekTable,
    },
    schedule_document::SemesterPlan,
    time::{LocalDate, LocalDateTime},
};
use fc_ports::{CalendarFetchOutcome, CalendarReservation, CalendarStatus, Clock};

type Check<T> = std::result::Result<T, String>;
pub(super) const STEPS: usize = 30;
const FIXTURE_PASSWORD: &str = "Synthetic calendar probe 2026!";
struct CivilClock(Mutex<LocalDateTime>);
impl Clock for CivilClock {
    fn now(&self) -> LocalDateTime {
        *self.0.lock().expect("isolated calendar clock poisoned")
    }
}
fn checked<T, E>(value: std::result::Result<T, E>, stage: &str) -> Check<T> {
    value.map_err(|_| format!("calendar-probe: {stage}"))
}
fn require(value: bool, stage: &str) -> Check<()> {
    if value {
        Ok(())
    } else {
        Err(format!("calendar-probe: {stage}"))
    }
}
fn date(month: u8, day: u8) -> LocalDate {
    LocalDate::new(2026, month, day).expect("synthetic date")
}
fn password() -> Check<Password> {
    checked(Password::new(FIXTURE_PASSWORD.into()), "synthetic password")
}
fn candidate(year: i32, updated: bool, clock: &CurrentClock) -> Check<CalendarCandidate> {
    let extra = if updated {
        format!(r#",{{"date":"{year}-11-06","name":"合成新增休息","isOffDay":true}}"#)
    } else {
        String::new()
    };
    let body = format!(
        r#"{{"year":{year},"papers":["https://www.gov.cn/zhengce/content/2026-01/01/content_1000000.htm"],"days":[{{"date":"{year}-11-02","name":"合成休息","isOffDay":true}},{{"date":"{year}-11-03","name":"合成补班","isOffDay":false}},{{"date":"{year}-11-05","name":"合成休息","isOffDay":true}}{extra}]}}"#
    );
    checked(
        fc_protocol::calendar_feed::parse(
            year,
            body.as_bytes(),
            checked(clock.try_now(), "real local clock")?,
            Some(format!("\"synthetic-{year}-{updated}\"")),
            None,
        ),
        "strict synthetic candidate",
    )
}
fn seed(
    backend: &mut Backend,
    lease: SessionLease,
    candidate: CalendarCandidate,
) -> Check<CalendarStatus> {
    let reservation = checked(
        backend.reserve_calendar_fetch(lease, candidate.source_year),
        "real writer reserve synthetic fetch",
    )?;
    let CalendarReservation::Reserved(ticket) = reservation else {
        return Err("calendar-probe: synthetic fetch unexpectedly rate limited".into());
    };
    checked(
        backend.finish_calendar_fetch(ticket, CalendarFetchOutcome::Downloaded(candidate)),
        "real writer finish synthetic fetch",
    )
}
fn document(app: &App) -> Check<ScheduleDocument> {
    let backend = app
        .backend
        .as_ref()
        .ok_or("calendar-probe: backend missing")?;
    checked(
        checked(backend.lock(), "backend lock")?
            .read_schedule(app.lease.ok_or("calendar-probe: lease missing")?),
        "read saved schedule",
    )
}
fn status(app: &App, year: i32) -> Check<CalendarStatus> {
    let backend = app
        .backend
        .as_ref()
        .ok_or("calendar-probe: backend missing")?;
    checked(
        checked(backend.lock(), "backend lock")?
            .calendar_status(app.lease.ok_or("calendar-probe: lease missing")?, year),
        "read saved candidate",
    )
}
fn pump(app: &mut App, deadline: Instant) -> Check<()> {
    loop {
        for event in checked(app.window.take_events(), "take native events")? {
            app.event(event)?;
        }
        app.poll()?;
        require(
            !app.pool.is_faulted() && !app.stopping && !app.terminate_required,
            "worker stayed healthy",
        )?;
        if app.pending.is_none() && app.calendar_job.is_none() {
            return Ok(());
        }
        require(Instant::now() < deadline, "native workflow deadline")?;
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn submit(
    app: &mut App,
    action: ActionId,
    overrides: &[(FieldId, &str)],
    deadline: Instant,
) -> Check<()> {
    require(Instant::now() < deadline, "native submission deadline")?;
    let mut form = FormData::new();
    for field in app.view().fields.into_iter().filter(|_| action != LOCK) {
        if field.kind != FieldKind::ReadOnly {
            let value = overrides
                .iter()
                .find(|(id, _)| *id == field.id)
                .map_or(field.value, |(_, v)| (*v).into());
            checked(form.insert(field.id, value), "native form fields")?;
        }
    }
    checked(
        app.window.probe_submit(action, form),
        "native command admission",
    )?;
    pump(app, deadline)
}
fn mark(step: &mut usize, label: &str) {
    println!("CALENDAR_NATIVE_STEP index={} name={label}", *step);
    *step += 1;
}

fn exercise(app: &mut App, deadline: Instant) -> Check<usize> {
    let mut steps = 0;
    let baseline = document(app)?;
    let original_cache = status(app, 2026)?
        .candidate
        .ok_or("calendar-probe: original candidate missing")?;
    require(
        baseline.public_calendar_data.years.is_empty(),
        "cached candidate was not active",
    )?;
    mark(&mut steps, "candidate-cache-is-inactive");
    submit(app, TO_SCHEDULE, &[], deadline)?;
    require(app.screen == Screen::Schedule, "schedule entry")?;
    mark(&mut steps, "native-open-schedule");
    submit(app, schedule_editor::CALENDAR, &[], deadline)?;
    mark(&mut steps, "native-open-calendar-page");
    submit(app, schedule_editor::OPEN_PUBLIC_CALENDAR, &[], deadline)?;
    require(app.screen == Screen::Calendar, "calendar entry")?;
    mark(&mut steps, "native-open-candidates");
    submit(app, c::SELECT, &[(c::YEAR, "2026")], deadline)?;
    require(
        app.view().description.contains("原始文件SHA-256"),
        "candidate provenance displayed",
    )?;
    mark(&mut steps, "native-select-year-provenance");
    submit(app, c::REVIEW, &[], deadline)?;
    require(
        app.calendar
            .as_ref()
            .is_some_and(|e| e.has_unsaved_changes()),
        "adoption review owned",
    )?;
    mark(&mut steps, "native-review-adoption");
    submit(app, c::CANCEL_REVIEW, &[], deadline)?;
    require(
        document(app)? == baseline,
        "cancel adoption left saved state intact",
    )?;
    mark(&mut steps, "native-cancel-adoption-no-write");
    submit(app, c::REVIEW, &[], deadline)?;
    mark(&mut steps, "native-review-again");
    submit(app, c::CONFIRM_ADOPT, &[(c::ACK, "0")], deadline)?;
    require(
        document(app)? == baseline && app.has_unsaved_changes(),
        "unchecked confirmation did not adopt",
    )?;
    mark(&mut steps, "native-adoption-requires-ack");
    submit(app, c::CONFIRM_ADOPT, &[(c::ACK, "1")], deadline)?;
    let adopted = document(app)?;
    require(
        adopted.public_calendar_data.years == vec![original_cache.clone()]
            && adopted.public_calendar.entries == baseline.public_calendar.entries
            && adopted
                .resolve_day(ClassId::new(1).unwrap(), date(11, 3))
                .is_ok_and(|d| matches!(d, AcademicDay::Unconfirmed))
            && adopted
                .resolve_day(ClassId::new(1).unwrap(), date(11, 5))
                .is_ok_and(|d| matches!(d, AcademicDay::Rest))
            && adopted
                .resolve_day(ClassId::new(1).unwrap(), date(11, 2))
                .is_ok_and(
                    |d| matches!(d,AcademicDay::Scheduled(s) if s.source_date == date(11,4)),
                ),
        "saved provenance, manual override and unmapped makeup",
    )?;
    mark(&mut steps, "native-adopt-persist-provenance-manual-makeup");
    submit(app, c::AUTO_REVIEW, &[], deadline)?;
    mark(&mut steps, "native-review-auto-policy");
    submit(app, c::CANCEL_REVIEW, &[], deadline)?;
    require(
        !status(app, 2026)?.policy.enabled,
        "cancel policy retained default off",
    )?;
    mark(&mut steps, "native-cancel-policy-no-write");
    submit(app, c::AUTO_REVIEW, &[], deadline)?;
    mark(&mut steps, "native-review-enable-policy");
    submit(app, c::CONFIRM_AUTO, &[(c::ACK, "0")], deadline)?;
    require(
        !status(app, 2026)?.policy.enabled && app.has_unsaved_changes(),
        "unchecked policy did not write",
    )?;
    mark(&mut steps, "native-policy-requires-ack");
    submit(app, c::CONFIRM_AUTO, &[(c::ACK, "1")], deadline)?;
    let enabled = status(app, 2026)?.policy;
    require(
        enabled.enabled
            && enabled
                .grant
                .as_ref()
                .is_some_and(|g| Some(g.principal_id) == app.current_global_owner()),
        "persisted policy owner grant",
    )?;
    mark(&mut steps, "native-enable-policy-persist-grant");
    submit(app, c::AUTO_REVIEW, &[], deadline)?;
    mark(&mut steps, "native-review-disable-policy");
    submit(app, c::CONFIRM_AUTO, &[(c::ACK, "1")], deadline)?;
    let disabled = status(app, 2026)?.policy;
    require(
        !disabled.enabled
            && disabled.grant.is_none()
            && disabled.revision > enabled.revision
            && document(app)? == adopted,
        "disable policy retained adopted data",
    )?;
    mark(&mut steps, "native-disable-policy-retain-data");
    let old = status(app, 2026)?
        .years
        .into_iter()
        .find(|y| y.year == 2026)
        .ok_or("calendar-probe: year summary missing")?
        .candidate_revision;
    submit(app, c::REVIEW, &[], deadline)?;
    mark(&mut steps, "native-review-captured-old-candidate");
    {
        let mut backend = checked(app.backend.as_ref().unwrap().lock(), "replacement lock")?;
        let loaded = seed(
            &mut backend,
            app.lease.unwrap(),
            candidate(2026, true, &app.clock)?,
        )?;
        require(
            loaded
                .years
                .iter()
                .any(|y| y.year == 2026 && y.candidate_revision > old),
            "actual candidate revision advanced",
        )?;
    }
    require(
        document(app)? == adopted,
        "replacement candidate stayed inactive",
    )?;
    mark(&mut steps, "writer-replacement-candidate-cas-advanced");
    submit(app, c::CONFIRM_ADOPT, &[(c::ACK, "1")], deadline)?;
    let expected = error_text(&Failure::Backend(BackendError::Calendar(
        fc_ports::CalendarRepositoryError::Stale,
    )));
    require(
        app.status == expected && document(app)? == adopted,
        "native stale candidate rejected specifically as Calendar Stale",
    )?;
    {
        let mut backend = checked(app.backend.as_ref().unwrap().lock(), "stale check lock")?;
        require(
            matches!(
                backend.adopt_calendar(
                    app.lease.unwrap(),
                    adopted.revision,
                    vec![fc_ports::CalendarCandidateRef {
                        year: 2026,
                        revision: old
                    }]
                ),
                Err(BackendError::Calendar(
                    fc_ports::CalendarRepositoryError::Stale
                ))
            ),
            "backend exact Calendar Stale",
        )?;
    }
    mark(&mut steps, "native-old-candidate-cas-stale-no-write");
    submit(app, c::SELECT, &[(c::YEAR, "2026")], deadline)?;
    mark(&mut steps, "native-reload-current-candidate");
    // Fixture mode change uses the actual writer, not an editor snapshot rewrite.
    let school = {
        let mut backend = checked(app.backend.as_ref().unwrap().lock(), "school mode lock")?;
        let mut next = document_without_lock(&backend, app.lease.unwrap())?;
        next.calendar_mode = CalendarMode::School;
        next.school_calendar.school_coverage = Some((date(1, 1), date(12, 31)));
        checked(
            backend.save_schedule(app.lease.unwrap(), next.revision, next),
            "save isolated school mode",
        )?
    };
    mark(&mut steps, "writer-save-school-mode");
    submit(app, c::CLOSE, &[], deadline)?;
    require(
        app.screen == Screen::Schedule,
        "calendar close loaded saved schedule",
    )?;
    mark(&mut steps, "native-close-reloads-school-schedule");
    {
        let mut backend = checked(app.backend.as_ref().unwrap().lock(), "school cache lock")?;
        seed(
            &mut backend,
            app.lease.unwrap(),
            candidate(2027, false, &app.clock)?,
        )?;
    }
    require(
        document(app)? == school && status(app, 2027)?.candidate.is_some(),
        "school mode caches without adoption",
    )?;
    mark(&mut steps, "writer-school-mode-cache-only");
    submit(app, schedule_editor::CALENDAR, &[], deadline)?;
    mark(&mut steps, "native-open-school-calendar-page");
    submit(app, schedule_editor::OPEN_PUBLIC_CALENDAR, &[], deadline)?;
    mark(&mut steps, "native-open-school-candidates");
    submit(app, c::SELECT, &[(c::YEAR, "2027")], deadline)?;
    mark(&mut steps, "native-select-school-cached-year");
    submit(app, c::REVIEW, &[], deadline)?;
    require(
        app.view()
            .actions
            .iter()
            .any(|a| a.id == c::CONFIRM_ADOPT && !a.enabled),
        "school adoption action disabled",
    )?;
    require(
        app.window
            .probe_submit(c::CONFIRM_ADOPT, FormData::new())
            .is_err()
            && document(app)? == school,
        "native school adoption refused",
    )?;
    mark(&mut steps, "native-school-review-cannot-adopt");
    submit(app, LOCK, &[], deadline)?;
    require(
        app.lease.is_none() && app.calendar.is_none() && app.screen == Screen::Login,
        "lock discarded owner review",
    )?;
    mark(&mut steps, "native-lock-clears-review");
    submit(
        app,
        LOG_IN,
        &[(LOGIN, "calendar-probe"), (PASSWORD, FIXTURE_PASSWORD)],
        deadline,
    )?;
    require(
        app.screen == Screen::Home
            && app.calendar.is_none()
            && document(app)? == school
            && status(app, 2027)?.candidate.is_some()
            && status(app, 2026)?.candidate.is_some(),
        "real relogin retained only saved school/adopted/cache data",
    )?;
    mark(&mut steps, "native-relogin-saved-data-intact");
    require(steps == STEPS, "complete ordered workflow steps")?;
    Ok(steps)
}
fn document_without_lock(backend: &Backend, lease: SessionLease) -> Check<ScheduleDocument> {
    checked(backend.read_schedule(lease), "read writer schedule fixture")
}

pub(super) fn run() -> Check<()> {
    let started = Instant::now();
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-Calendar-{}-{}",
        std::process::id(),
        checked(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH),
            "nonce clock"
        )?
        .as_nanos()
    ));
    let paths = checked(DataPaths::isolated_test(root.clone()), "isolated path")?;
    let deployment = checked(paths.prepare_for_current_user(), "protected deployment")?;
    checked(deployment.read_back(), "protected ACL readback")?;
    let instance = checked(InstanceGuard::acquire(paths.root()), "isolated instance")?;
    let clock = Arc::new(CurrentClock::new());
    let civil = Arc::new(CivilClock(Mutex::new(
        LocalDateTime::new(date(10, 30), 36000).unwrap(),
    )));
    let codec = checked(
        CredentialRecordCodec::generate(&mut SystemRandom),
        "isolated random codec",
    )?;
    let mut backend = checked(
        Backend::open(paths.database(), codec, civil.clone(), clock.clone()),
        "open synthetic database",
    )?;
    let mut app = create_app(clock.clone(), true)?;
    app.root = root.clone();
    app.desktop.set_root(root.clone());
    app.next_content_maintenance = started + Duration::from_secs(3600);
    let setup: Check<()> = (|| {
        drop(checked(
            backend.initialize(
                Initialize {
                    school_name: "Synthetic calendar school".into(),
                    login_name: "calendar-probe".into(),
                    display_name: "Synthetic calendar administrator".into(),
                },
                password()?,
            ),
            "initialize synthetic administrator",
        )?);
        let lease = checked(
            backend.login("calendar-probe", password()?),
            "initial real login",
        )?;
        checked(
            backend.identity(
                lease,
                IdentityCommand::CreateClass {
                    id: ClassId::new(1).unwrap(),
                    name: "Synthetic calendar class".into(),
                },
            ),
            "synthetic class",
        )?;
        let mut doc = checked(backend.read_schedule(lease), "initial schedule")?;
        doc.semesters = vec![SemesterPlan {
            semester: Semester {
                id: SemesterId::new(1).unwrap(),
                class_id: ClassId::new(1).unwrap(),
                revision: Revision::default(),
                enabled: true,
                start: date(1, 1),
                end: date(12, 31),
                first_week_monday: LocalDate::new(2025, 12, 29).unwrap(),
                tables: vec![WeekTable {
                    id: TableId::new(2).unwrap(),
                    role: TableRole::Single,
                    rows: vec![],
                }],
            },
            name: "Synthetic calendar year".into(),
            timing_sync: false,
            dual_history: None,
            row_details: vec![],
            cell_details: vec![],
        }];
        doc.update_id_high_water();
        doc.public_calendar.entries = vec![CalendarEntry {
            actual_semester_id: SemesterId::new(1).unwrap(),
            date: date(11, 2),
            mapping: DayMapping::SourceDate(date(11, 4)),
        }];
        checked(
            backend.save_schedule(lease, doc.revision, doc),
            "save synthetic manual mapping",
        )?;
        seed(&mut backend, lease, candidate(2026, false, &clock)?)?;
        *checked(civil.0.lock(), "advance fixture civil clock")? =
            LocalDateTime::new(date(10, 31), 36000).unwrap();
        let lease = checked(
            backend.login("calendar-probe", password()?),
            "later real UI login",
        )?;
        app.state = Arc::new(checked(backend.state(), "real identity state")?);
        app.lease = Some(lease);
        let control = backend.session_control();
        let _ = app.control_slot.set(control.clone());
        app.control = Some(control);
        app.screen = Screen::Home;
        Ok(())
    })();
    app.backend = Some(Arc::new(Mutex::new(backend)));
    let result = setup.and_then(|()| {
        app.render()?;
        exercise(&mut app, started + Duration::from_secs(70))
    });
    app.begin_shutdown();
    let closed: Check<()> = (|| {
        while !app.pool.is_finished() {
            app.poll()?;
            require(
                Instant::now() < started + Duration::from_secs(90),
                "physical shutdown deadline; retain own root for diagnosis",
            )?;
            std::thread::sleep(Duration::from_millis(5));
        }
        checked(app.pool.join(), "join shared workers")?;
        Ok(())
    })();
    // The closure owns App because join consumes its pool. Its remaining
    // window/backend fields have dropped before deployment cleanup below.
    drop(instance);
    drop(deployment);
    closed?;
    checked(
        fs::remove_dir_all(&root),
        "remove closed isolated deployment",
    )?;
    let steps = result?;
    println!("calendar_candidates_review_adopt_policy_lock_stale_native_worker=PASS calendar_native_steps={steps} controlled_wall_clock_fixture=YES https=NOT_RUN isolated_cleanup=PASS elapsed_ms={}",started.elapsed().as_millis());
    Ok(())
}
