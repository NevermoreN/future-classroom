//! Hidden real controls, shared workers and real SQLite. No colour dialog,
//! desktop capture, listener, audio, or external application is opened.
use super::*;
use display_editor as d;
use voice_management_probe::{
    checked, login, make_app, open, require, stop_owned_app, submit, LOGIN_NAME, PASSWORD_VALUE,
};
const ORDER: [&str; 22] = [
    "setup-global-admin",
    "create-subject",
    "open-joint-display",
    "loaded-original-rgb",
    "invalid-rgb-no-write",
    "native-joint-draft-preview",
    "lock-keeps-partial-draft",
    "relogin-restores-draft",
    "cancel-review-no-write",
    "cancel-reloads-both",
    "native-joint-save",
    "readback-both-saved",
    "native-palette-draft",
    "rename-subject-preserves-color",
    "stale-palette-save-rejected",
    "cancel-reload-after-conflict",
    "first-app-physical-shutdown",
    "second-app-real-reopen",
    "relogin-reopened-deployment",
    "reopened-joint-state",
    "second-app-physical-shutdown",
    "isolated-deployment-cleanup",
];
type Check<T> = std::result::Result<T, String>;
fn mark(step: &mut usize, name: &str) -> Check<()> {
    require(ORDER.get(*step) == Some(&name), "visuals exact step order")?;
    println!("SUBJECT_VISUAL_NATIVE_STEP index={} name={name}", *step);
    *step += 1;
    Ok(())
}
fn saved(app: &App) -> Check<fc_ports::SubjectVisualState> {
    checked(
        app.backend
            .as_ref()
            .ok_or("visuals backend absent")?
            .lock()
            .map_err(|_| "visuals backend poisoned")?
            .read_subject_visuals(app.lease.ok_or("visuals lease absent")?),
        "visuals persisted observer",
    )
}
fn value(app: &App, id: FieldId) -> Option<String> {
    app.view()
        .fields
        .into_iter()
        .find(|f| f.id == id)
        .map(|f| f.value)
}
fn colors(app: &mut App, deadline: Instant) -> Check<()> {
    submit(app, TO_DISPLAY, &[], deadline)?;
    require(app.screen == Screen::Display, "visuals display open")?;
    submit(app, d::VISUALS, &[], deadline)
}
fn initial(
    app: &mut App,
    paths: &DataPaths,
    step: &mut usize,
    deadline: Instant,
) -> Check<fc_ports::SubjectVisualState> {
    open(app, paths, deadline)?;
    submit(
        app,
        SAVE,
        &[
            (SCHOOL, "Synthetic subject visual school"),
            (LOGIN, LOGIN_NAME),
            (NAME, "Synthetic palette administrator"),
            (PASSWORD, PASSWORD_VALUE),
            (CONFIRM, PASSWORD_VALUE),
        ],
        deadline,
    )?;
    require(app.screen == Screen::Code, "visuals real setup")?;
    submit(app, ACK, &[], deadline)?;
    if app.screen == Screen::Login {
        login(app, deadline)?;
    }
    mark(step, "setup-global-admin")?;
    submit(app, TO_SUBJECTS, &[], deadline)?;
    submit(
        app,
        SAVE,
        &[
            (OLD_NAME, ""),
            (SUBJECT, "Synthetic mathematics"),
            (COLOR, "1234AB"),
        ],
        deadline,
    )?;
    require(
        app.screen == Screen::Home,
        "subject save returned to actual home page",
    )?;
    mark(step, "create-subject")?;
    colors(app, deadline)?;
    mark(step, "open-joint-display")?;
    let before = saved(app)?;
    require(
        value(app, d::OPAQUE_RGB).as_deref() == Some("1234AB"),
        "real original opaque colour loaded",
    )?;
    mark(step, "loaded-original-rgb")?;
    submit(
        app,
        d::APPLY,
        &[(d::OPAQUE_RGB, "GGGGGG"), (d::COURSE_ALPHA, "73")],
        deadline,
    )?;
    require(
        saved(app)? == before
            && app
                .display
                .as_ref()
                .is_some_and(|e| e.has_unsaved_changes()),
        "invalid colour no partial write",
    )?;
    mark(step, "invalid-rgb-no-write")?;
    submit(
        app,
        d::VISUAL_PREVIEW,
        &[
            (d::OPAQUE_RGB, "D02080"),
            (d::FILL, "1"),
            (d::COURSE_ALPHA, "73"),
            (d::HOMEWORK_ALPHA, "41"),
        ],
        deadline,
    )?;
    require(
        app.preview_active && saved(app)? == before,
        "real hidden D2D preview leaves database unchanged",
    )?;
    mark(step, "native-joint-draft-preview")?;
    checked(
        app.window.probe_edit(d::OPAQUE_RGB, "AA"),
        "native partial RGB",
    )?;
    submit(app, LOCK, &[], deadline)?;
    require(
        app.screen == Screen::Login
            && !app.preview_active
            && app
                .display
                .as_ref()
                .is_some_and(|e| e.has_unsaved_changes()),
        "lock keeps nonsecret draft and removes preview",
    )?;
    mark(step, "lock-keeps-partial-draft")?;
    login(app, deadline)?;
    submit(app, TO_DISPLAY, &[], deadline)?;
    require(
        value(app, d::OPAQUE_RGB).as_deref() == Some("AA"),
        "same-owner re-login retains partial colour",
    )?;
    mark(step, "relogin-restores-draft")?;
    submit(app, d::CANCEL, &[], deadline)?;
    require(saved(app)? == before, "cancel review no write")?;
    mark(step, "cancel-review-no-write")?;
    submit(app, d::CONFIRM_CANCEL, &[], deadline)?;
    submit(app, d::VISUALS, &[], deadline)?;
    require(
        saved(app)? == before && value(app, d::OPAQUE_RGB).as_deref() == Some("1234AB"),
        "cancel reloads both originals",
    )?;
    mark(step, "cancel-reloads-both")?;
    submit(
        app,
        d::SAVE_ALL,
        &[
            (d::OPAQUE_RGB, "D02080"),
            (d::FILL, "0"),
            (d::COURSE_ALPHA, "73"),
            (d::HOMEWORK_ALPHA, "41"),
        ],
        deadline,
    )?;
    mark(step, "native-joint-save")?;
    let after = saved(app)?;
    require(
        after.palette.revision != before.palette.revision
            && after.display.revision
                == before
                    .display
                    .revision
                    .next()
                    .map_err(|_| "visuals revision overflow")?
            && !after.display.subject_fill
            && after.display.course_fill_percent == 73
            && after.display.homework_fill_percent == 41
            && after.palette.subjects[0].color
                == Rgb {
                    red: 0xD0,
                    green: 0x20,
                    blue: 0x80,
                }
            && !app
                .display
                .as_ref()
                .is_some_and(|e| e.has_unsaved_changes()),
        "both settings really saved",
    )?;
    mark(step, "readback-both-saved")?;
    submit(app, d::APPLY, &[(d::OPAQUE_RGB, "102030")], deadline)?;
    require(saved(app)? == after, "new palette draft stays local")?;
    mark(step, "native-palette-draft")?;
    submit(app, d::EDIT_BACK, &[], deadline)?;
    submit(app, d::EDIT_BACK, &[], deadline)?;
    submit(app, TO_SUBJECTS, &[], deadline)?;
    submit(
        app,
        SAVE,
        &[
            (OLD_NAME, "Synthetic mathematics"),
            (SUBJECT, "Synthetic renamed mathematics"),
            (COLOR, "FFFFFF"),
        ],
        deadline,
    )?;
    require(
        app.screen == Screen::Home,
        "subject save returned to actual home page",
    )?;
    let renamed = saved(app)?;
    require(
        renamed.palette.subjects[0].id == after.palette.subjects[0].id
            && renamed.palette.subjects[0].color == after.palette.subjects[0].color
            && renamed.display == after.display,
        "rename keeps colour and display",
    )?;
    mark(step, "rename-subject-preserves-color")?;
    colors(app, deadline)?;
    submit(app, d::SAVE_ALL, &[], deadline)?;
    require(
        saved(app)? == renamed
            && app
                .display
                .as_ref()
                .is_some_and(|e| e.has_unsaved_changes())
            && value(app, d::OPAQUE_RGB).as_deref() == Some("102030"),
        "old palette vector rejected without rebasing draft",
    )?;
    mark(step, "stale-palette-save-rejected")?;
    submit(app, d::CANCEL, &[], deadline)?;
    submit(app, d::CONFIRM_CANCEL, &[], deadline)?;
    require(
        saved(app)? == renamed,
        "conflict recovery only explicit reload",
    )?;
    mark(step, "cancel-reload-after-conflict")?;
    Ok(renamed)
}
pub(super) fn run() -> Check<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(120);
    let mut random = [0; 16];
    checked(SystemRandom.fill(&mut random), "visuals nonce")?;
    let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-SubjectVisuals-{}-{nonce}",
        std::process::id()
    ));
    let paths = checked(
        DataPaths::isolated_test(root.clone()),
        "visuals isolated paths",
    )?;
    let deployment = checked(
        paths.prepare_for_current_user(),
        "visuals protected deployment",
    )?;
    checked(deployment.read_back(), "visuals ACL readback")?;
    let instance = checked(
        InstanceGuard::acquire(paths.root()),
        "visuals instance guard",
    )?;
    let mut step = 0;
    let mut first = make_app(&paths, deadline)?;
    let result = initial(&mut first, &paths, &mut step, deadline);
    stop_owned_app(first, deadline)?;
    let mut second_physically_closed = true;
    let result = (|| {
        let expected = result?;
        mark(&mut step, "first-app-physical-shutdown")?;
        let mut second = make_app(&paths, deadline)?;
        second_physically_closed = false;
        let reopened: Check<()> = (|| {
            open(&mut second, &paths, deadline)?;
            require(
                second.screen == Screen::Login,
                "actual database reopened locked",
            )?;
            mark(&mut step, "second-app-real-reopen")?;
            login(&mut second, deadline)?;
            mark(&mut step, "relogin-reopened-deployment")?;
            colors(&mut second, deadline)?;
            require(
                saved(&second)? == expected
                    && value(&second, d::OPAQUE_RGB).as_deref() == Some("D02080"),
                "both settings durable across independent app reopen",
            )?;
            mark(&mut step, "reopened-joint-state")?;
            Ok(())
        })();
        stop_owned_app(second, deadline)?;
        second_physically_closed = true;
        reopened?;
        mark(&mut step, "second-app-physical-shutdown")?;
        Ok(())
    })();
    if !second_physically_closed {
        // A failed physical close retains the original deployment. Propagate
        // before file removal; an assertion failure with successful close may
        // still clean up normally below.
        return result;
    }
    drop(instance);
    drop(deployment);
    checked(
        fs::remove_dir_all(&root),
        "visuals remove closed deployment",
    )?;
    require(!root.exists(), "visuals deployment removed")?;
    result?;
    mark(&mut step, "isolated-deployment-cleanup")?;
    require(step == ORDER.len(), "visuals all steps")?;
    println!("SUBJECT_VISUAL_NATIVE_PASS steps={step} shared_workers=2 real_sqlite_reopen=PASS joint_save=PASS stale_rejected=PASS synthetic_d2d=PASS colour_dialog=NOT_OPENED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms={}",started.elapsed().as_millis());
    Ok(())
}
