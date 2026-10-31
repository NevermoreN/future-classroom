//! Isolated real controls, SQLite and shared workers; desktop surfaces remain
//! hidden. No account is used for any public lesson operation.
use super::*;
use public_lesson_editor as p;
use voice_management_probe::{
    checked, make_app, open, pump, require, stop_owned_app, submit, LOGIN_NAME, PASSWORD_VALUE,
};
type Check<T> = std::result::Result<T, String>;
pub(super) const ORDER: [&str; 20] = [
    "setup-administrator-fixture",
    "create-bound-class-and-subjects",
    "seed-current-lesson",
    "management-lock-removes-session",
    "hidden-desktop-rendered",
    "native-public-button-without-session",
    "public-catalog-only",
    "cancel-without-change",
    "new-native-public-entry",
    "unassigned-review-without-write",
    "local-account-lock-keeps-public-flow",
    "explicit-public-change-closes",
    "changed-desktop-rendered",
    "old-scene-entry-rejected",
    "new-native-restore-entry",
    "explicit-public-restore-closes",
    "restored-desktop-rendered",
    "public-close-hides-chooser",
    "physical-shared-workers-shutdown",
    "isolated-deployment-cleanup",
];
fn mark(step: &mut usize, name: &str) -> Check<()> {
    require(
        ORDER.get(*step) == Some(&name),
        "public desktop exact step order",
    )?;
    println!("PUBLIC_DESKTOP_NATIVE_STEP index={} name={name}", *step);
    *step += 1;
    Ok(())
}
fn day(date: fc_domain::time::LocalDate) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}
fn fixture(app: &mut App, paths: &DataPaths, step: &mut usize, deadline: Instant) -> Check<()> {
    open(app, paths, deadline)?;
    submit(
        app,
        SAVE,
        &[
            (SCHOOL, "Synthetic public desktop school"),
            (LOGIN, LOGIN_NAME),
            (NAME, "Synthetic setup administrator"),
            (PASSWORD, PASSWORD_VALUE),
            (CONFIRM, PASSWORD_VALUE),
        ],
        deadline,
    )?;
    submit(app, ACK, &[], deadline)?;
    if app.screen == Screen::Login {
        voice_management_probe::login(app, deadline)?;
    }
    mark(step, "setup-administrator-fixture")?;
    submit(app, TO_SCHOOL, &[], deadline)?;
    submit(
        app,
        ADD_CLASS,
        &[
            (SCHOOL, "Synthetic public desktop school"),
            (CLASS, "Synthetic classroom"),
        ],
        deadline,
    )?;
    submit(app, TO_SCHOOL, &[], deadline)?;
    submit(
        app,
        BIND_CLASS,
        &[
            (SCHOOL, "Synthetic public desktop school"),
            (CLASS, "Synthetic classroom"),
        ],
        deadline,
    )?;
    for name in ["Synthetic original", "Synthetic temporary"] {
        submit(app, TO_SUBJECTS, &[], deadline)?;
        submit(
            app,
            SAVE,
            &[(OLD_NAME, ""), (SUBJECT, name), (COLOR, "70A5D8")],
            deadline,
        )?;
        require(
            app.screen == Screen::Home,
            "subject save returned to actual home page",
        )?;
    }
    mark(step, "create-bound-class-and-subjects")?;
    submit(app, TO_SCHEDULE, &[], deadline)?;
    submit(app, schedule_editor::NEW_TERM, &[], deadline)?;
    let now = fc_ports::Clock::now(app.clock.as_ref());
    // This private deployment has one all-day synthetic row. The helper below
    // binds its actual current date and all weekday cells through SaveSchedule.
    let date = day(now.date);
    let mut monday = now.date;
    while monday.weekday() != fc_domain::time::Weekday::Monday {
        monday = (1..=31)
            .rev()
            .find_map(|d| {
                fc_domain::time::LocalDate::new(monday.year(), monday.month(), d)
                    .ok()
                    .filter(|date| *date < monday)
            })
            .or_else(|| {
                let (year, month) = if monday.month() == 1 {
                    (monday.year() - 1, 12)
                } else {
                    (monday.year(), monday.month() - 1)
                };
                (28..=31)
                    .rev()
                    .find_map(|d| fc_domain::time::LocalDate::new(year, month, d).ok())
            })
            .ok_or("public fixture date")?;
    }
    let anchor = day(monday);
    submit(
        app,
        schedule_editor::APPLY,
        &[
            (schedule_editor::TERM_NAME, "Synthetic public day"),
            (schedule_editor::START, &date),
            (schedule_editor::END, &date),
            (schedule_editor::ANCHOR, &anchor),
        ],
        deadline,
    )?;
    submit(app, schedule_editor::ADD_ROW, &[], deadline)?;
    submit(
        app,
        schedule_editor::APPLY,
        &[
            (schedule_editor::ROW_NAME, "Synthetic current lesson"),
            (schedule_editor::RANGE, "00002359"),
        ],
        deadline,
    )?;
    let mut stage = 0;
    lesson_probe_step(app, &mut stage)?;
    pump(app, deadline)?;
    mark(step, "seed-current-lesson")?;
    app.lock();
    app.window.hide();
    app.render()?;
    require(
        app.lease.is_none()
            && app
                .control
                .as_ref()
                .is_some_and(SessionControl::is_local_locked),
        "no local account session",
    )?;
    mark(step, "management-lock-removes-session")
}
fn desktop(app: &mut App, deadline: Instant) -> Check<fc_bootstrap::backend::DesktopLessonStamp> {
    loop {
        require(Instant::now() < deadline, "public desktop render deadline")?;
        app.poll()?;
        app.desktop.tick(
            app.backend.as_ref(),
            app.control.as_ref(),
            &app.pool,
            &app.clock,
            false,
            false,
            HostHints::default(),
        )?;
        if app.desktop.probe_ready() {
            if let Some(stamp) = app.desktop.probe_lesson_stamp() {
                return Ok(stamp);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn click(app: &mut App, deadline: Instant) -> Check<()> {
    app.desktop.probe_lesson_click()?;
    app.desktop.tick(
        app.backend.as_ref(),
        app.control.as_ref(),
        &app.pool,
        &app.clock,
        false,
        false,
        HostHints::default(),
    )?;
    let stamp = app
        .desktop
        .take_lesson_event()
        .ok_or("actual native lesson click did not produce captured stamp")?;
    app.open_public_lesson(stamp)?;
    pump(app, deadline)?;
    require(
        app.public_lesson_flow.is_some()
            && app.public_lesson_editor.is_some()
            && app.lease.is_none(),
        "public chooser uses no account",
    )
}
fn run_inner(app: &mut App, paths: &DataPaths, step: &mut usize, deadline: Instant) -> Check<()> {
    fixture(app, paths, step, deadline)?;
    let original = desktop(app, deadline)?;
    mark(step, "hidden-desktop-rendered")?;
    click(app, deadline)?;
    mark(step, "native-public-button-without-session")?;
    let editor = app.public_lesson_editor.as_ref().ok_or("public editor")?;
    let original_revision = editor.source.context.revision;
    require(
        editor.source.teachers.is_empty()
            && app.view().fields.len() == 2
            && app
                .view()
                .actions
                .iter()
                .all(|a| matches!(a.id, p::REPLACE | p::RESTORE | p::RELOAD | p::CLOSE)),
        "only public business catalog and actions",
    )?;
    app.event(Event::Submit(LOCK, FormData::new()))?;
    app.event(Event::Submit(TO_DISPLAY, FormData::new()))?;
    require(
        app.public_lesson_flow.is_some() && app.screen == Screen::Lesson && app.lease.is_none(),
        "public page cannot route to authentication or settings",
    )?;
    mark(step, "public-catalog-only")?;
    submit(app, p::CLOSE, &[], deadline)?;
    require(
        app.public_lesson_flow.is_none() && !app.window.is_visible() && app.lease.is_none(),
        "cancel hides public chooser",
    )?;
    let again = desktop(app, deadline)?;
    require(
        again.binding.expected_revision == original_revision,
        "cancel did not write",
    )?;
    mark(step, "cancel-without-change")?;
    click(app, deadline)?;
    mark(step, "new-native-public-entry")?;
    let subject = app
        .state
        .subjects
        .iter()
        .find(|s| s.name == "Synthetic temporary")
        .ok_or("temporary synthetic subject")?
        .id
        .get()
        .to_string();
    submit(
        app,
        p::REPLACE,
        &[(p::SUBJECT, &subject), (p::TEACHER, "unassigned")],
        deadline,
    )?;
    require(
        app.public_lesson_editor.as_ref().is_some_and(|e| {
            e.view("", false)
                .actions
                .iter()
                .any(|a| a.id == p::CONFIRM_UNASSIGNED)
        }),
        "normal unassigned confirmation",
    )?;
    require(
        app.public_lesson_editor
            .as_ref()
            .is_some_and(|e| e.source.context.revision == original_revision),
        "review did not write",
    )?;
    mark(step, "unassigned-review-without-write")?;
    app.lock();
    app.render()?;
    require(
        app.public_lesson_flow.is_some() && app.lease.is_none(),
        "account lock does not authenticate or cancel public action",
    )?;
    mark(step, "local-account-lock-keeps-public-flow")?;
    submit(app, p::CONFIRM_UNASSIGNED, &[], deadline)?;
    require(
        app.public_lesson_flow.is_none() && !app.window.is_visible(),
        "successful public change closes chooser",
    )?;
    mark(step, "explicit-public-change-closes")?;
    let changed = desktop(app, deadline)?;
    require(
        changed.binding.expected_revision > original_revision,
        "public change persisted and refreshed",
    )?;
    mark(step, "changed-desktop-rendered")?;
    require(
        app.control
            .as_ref()
            .unwrap()
            .begin_desktop_lesson_flow(&original)
            .is_err(),
        "old full scene is rejected",
    )?;
    mark(step, "old-scene-entry-rejected")?;
    click(app, deadline)?;
    require(
        app.public_lesson_editor.as_ref().is_some_and(|e| {
            e.source.context.active_override.as_ref().is_some_and(|o| {
                o.initiator == fc_domain::lesson_context::LessonInitiator::LocalDesktop
            })
        }),
        "stored initiator is local desktop",
    )?;
    mark(step, "new-native-restore-entry")?;
    submit(app, p::RESTORE, &[], deadline)?;
    require(
        app.public_lesson_flow.is_none() && !app.window.is_visible(),
        "restore consumes public flow",
    )?;
    mark(step, "explicit-public-restore-closes")?;
    let restored = desktop(app, deadline)?;
    require(
        restored.binding.expected_revision > changed.binding.expected_revision,
        "restored revision refreshed",
    )?;
    mark(step, "restored-desktop-rendered")?;
    click(app, deadline)?;
    require(
        app.public_lesson_editor
            .as_ref()
            .is_some_and(|e| e.source.context.active_override.is_none()),
        "restore is persisted",
    )?;
    submit(app, p::CLOSE, &[], deadline)?;
    require(
        !app.window.is_visible() && app.lease.is_none(),
        "public close remains hidden without authentication",
    )?;
    // Close immediately after submission while the original ticket is still
    // owned. The late success/error must be consumed without reopening Login,
    // refreshing a lease, or manufacturing a new command.
    desktop(app, deadline)?;
    app.desktop.probe_lesson_click()?;
    app.desktop.tick(
        app.backend.as_ref(),
        app.control.as_ref(),
        &app.pool,
        &app.clock,
        false,
        false,
        HostHints::default(),
    )?;
    let stamp = app
        .desktop
        .take_lesson_event()
        .ok_or("late-close native entry")?;
    app.open_public_lesson(stamp)?;
    require(
        app.pending.is_some(),
        "original public worker ticket exists",
    )?;
    app.event(Event::Close)?;
    require(
        app.pending.is_some() && app.public_lesson_flow.is_none(),
        "close retains physical worker ticket",
    )?;
    pump(app, deadline)?;
    require(
        app.pending.is_none()
            && app.public_lesson_flow.is_none()
            && app.lease.is_none()
            && !app.window.is_visible(),
        "late original completion cannot reopen or authenticate",
    )?;
    mark(step, "public-close-hides-chooser")
}
pub(super) fn run() -> Check<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(120);
    let mut random = [0u8; 16];
    checked(SystemRandom.fill(&mut random), "public probe nonce")?;
    let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-PublicDesktop-{}-{nonce}",
        std::process::id()
    ));
    let paths = checked(
        DataPaths::isolated_test(root.clone()),
        "public isolated paths",
    )?;
    let deployment = checked(
        paths.prepare_for_current_user(),
        "public protected deployment",
    )?;
    checked(deployment.read_back(), "public ACL readback")?;
    let instance = checked(
        InstanceGuard::acquire(paths.root()),
        "public instance guard",
    )?;
    let mut app = make_app(&paths, deadline)?;
    let mut step = 0;
    let result = run_inner(&mut app, &paths, &mut step, deadline);
    // A physical shutdown failure bypasses directory deletion and retains proof.
    stop_owned_app(app, deadline)?;
    if result.is_ok() {
        mark(&mut step, "physical-shared-workers-shutdown")?;
    }
    drop(instance);
    drop(deployment);
    checked(
        fs::remove_dir_all(&root),
        "public closed deployment removal",
    )?;
    require(!root.exists(), "public deployment absent")?;
    result?;
    mark(&mut step, "isolated-deployment-cleanup")?;
    require(step == ORDER.len(), "complete public desktop workflow")?;
    println!("PUBLIC_DESKTOP_NATIVE_PASS steps={step} shared_workers=2 public_session=NONE database_reopen=NOT_EXERCISED change_restore=PASS old_scene=REJECTED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms={}",started.elapsed().as_millis());
    Ok(())
}
