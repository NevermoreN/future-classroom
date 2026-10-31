//! Hidden native controls and real recovery ownership against a new protected
//! synthetic deployment. Picker return capabilities are simulated only through
//! their strictly isolated constructors; no visible dialog/foreground is used.
use super::*;
use backup_editor as b;
use fc_bootstrap::recovery_workflow::{ConfirmationState, RecoveryWorkflow};
use fc_windows::materials::chooser::{BackupDestination, SelectedFile};
use fc_windows::materials::files::{LockedFile, PinnedDirectory};
use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

type Check<T> = std::result::Result<T, String>;
const LOGIN_NAME: &str = "recovery-probe-admin";
const INITIAL_PASSWORD: &str = "Synthetic recovery initial 2026!";
const CURRENT_PASSWORD: &str = "Synthetic recovery current 2026!";
const ACCOUNT_PASSWORD: &str = "Synthetic recovery account 2026!";
const CHANGED_ACCOUNT_PASSWORD: &str = "Synthetic recovery account changed 2026!";
const ORIGINAL_SCHOOL: &str = "Synthetic recovery original school";
const ALTERED_SCHOOL: &str = "Synthetic recovery changed school";
const STALE_SCHOOL: &str = "Synthetic recovery newer school";
const COPY_NAME: &str = "restore.fcbak";
const OWNER_MARKER: &str = "recovery-probe-owner.marker";
const CHILD_OUTPUT_LIMIT: usize = 64 * 1024;

fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>, stage: &str) -> Check<T> {
    value.map_err(|error| format!("recovery-probe: {stage}: {error:?}"))
}
fn require(value: bool, stage: &str) -> Check<()> {
    if value {
        Ok(())
    } else {
        Err(format!("recovery-probe: {stage}"))
    }
}
fn mark(steps: &mut usize, label: &str) {
    println!("RECOVERY_NATIVE_STEP index={} name={label}", *steps);
    *steps += 1;
}
fn open_native_backup(app: &mut App, deadline: Instant) -> Check<()> {
    if app.screen == Screen::Home {
        submit(app, TO_DISPLAY, &[], deadline)?;
    }
    submit(app, TO_BACKUP, &[], deadline)
}
fn tick(app: &mut App, deadline: Instant) -> Check<()> {
    require(
        Instant::now() < deadline,
        "deadline; retain owned deployment until physical shutdown",
    )?;
    for event in checked(app.window.take_events(), "take native events")? {
        app.event(event)?;
    }
    app.poll()?;
    require(
        !app.pool.is_faulted() && !app.terminate_required,
        "healthy application/coordinator",
    )
}
fn pump(app: &mut App, deadline: Instant) -> Check<()> {
    loop {
        tick(app, deadline)?;
        let retiring = app
            .backup
            .workflow
            .as_ref()
            .is_some_and(RecoveryWorkflow::retirement_pending);
        if app.pending.is_none()
            && app.backup.job.is_none()
            && app.backup.cleanup.is_none()
            && !retiring
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn submit(
    app: &mut App,
    action: ActionId,
    overrides: &[(FieldId, &str)],
    deadline: Instant,
) -> Check<()> {
    let mut data = FormData::new();
    for field in app.view().fields.into_iter().filter(|_| action != LOCK) {
        if field.kind != FieldKind::ReadOnly {
            let value = overrides
                .iter()
                .find(|(id, _)| *id == field.id)
                .map_or(field.value, |(_, value)| (*value).into());
            checked(data.insert(field.id, value), "native form field")?;
        }
    }
    checked(
        app.window.probe_submit(action, data),
        "native control admission",
    )?;
    pump(app, deadline)
}
fn login(app: &mut App, login: &str, password: &str, deadline: Instant) -> Check<()> {
    submit(
        app,
        LOG_IN,
        &[(LOGIN, login), (PASSWORD, password)],
        deadline,
    )
}
fn revision(app: &App) -> Check<Revision> {
    checked(
        app.backend
            .as_ref()
            .ok_or("recovery-probe: backend absent")?
            .lock(),
        "backend lock",
    )?
    .global_revision()
    .map_err(|error| format!("recovery-probe: global revision: {error:?}"))
}
fn school(app: &App) -> Check<String> {
    let backend = checked(
        app.backend
            .as_ref()
            .ok_or("recovery-probe: backend absent")?
            .lock(),
        "backend lock",
    )?;
    Ok(checked(backend.state(), "identity snapshot")?
        .installation
        .ok_or("recovery-probe: installation absent")?
        .school_name)
}
fn rename(app: &mut App, name: &str) -> Check<()> {
    let lease = app.lease.ok_or("recovery-probe: lease absent")?;
    let mut backend = checked(
        app.backend
            .as_ref()
            .ok_or("recovery-probe: backend absent")?
            .lock(),
        "backend lock",
    )?;
    checked(
        backend.identity(lease, IdentityCommand::RenameSchool { name: name.into() }),
        "synthetic external teaching edit",
    )?;
    Ok(())
}
fn ack(app: &App) -> Check<String> {
    let view = app.view();
    let field = view
        .fields
        .iter()
        .find(|f| f.id == b::ACK)
        .ok_or("recovery-probe: acknowledgement missing")?;
    require(
        field.value == "0",
        "explicit review defaults unacknowledged",
    )?;
    let FieldKind::Choice(choices) = &field.kind else {
        return Err("recovery-probe: acknowledgement is not a native choice".into());
    };
    choices
        .iter()
        .find(|c| c.value != "0")
        .map(|c| c.value.clone())
        .ok_or("recovery-probe: acknowledgement choice absent".into())
}
fn preflight(app: &mut App, root: &Path, deadline: Instant) -> Check<()> {
    app.preflight_selected_backup(checked(
        SelectedFile::isolated_probe(root, COPY_NAME),
        "isolated source picker capability",
    )?)?;
    pump(app, deadline)?;
    require(
        app.backup.review.is_some()
            && app
                .backup
                .editor
                .as_ref()
                .is_some_and(|e| e.has_unsaved_changes()),
        "real full preflight review",
    )
}
fn deny_entry(app: &mut App, login_name: &str, deadline: Instant) -> Check<()> {
    login(app, login_name, ACCOUNT_PASSWORD, deadline)?;
    require(
        app.screen == Screen::Password,
        "new negative-role fixture requires password change",
    )?;
    submit(
        app,
        SAVE,
        &[
            (PASSWORD, ACCOUNT_PASSWORD),
            (NEW_PASSWORD, CHANGED_ACCOUNT_PASSWORD),
            (CONFIRM, CHANGED_ACCOUNT_PASSWORD),
        ],
        deadline,
    )?;
    login(app, login_name, CHANGED_ACCOUNT_PASSWORD, deadline)?;
    require(app.screen == Screen::Home, "negative-role real login")?;
    require(
        !app.view().actions.iter().any(|a| a.id == TO_BACKUP),
        "nonadministrator entry hidden",
    )?;
    require(
        app.window.probe_submit(TO_BACKUP, FormData::new()).is_err(),
        "native control rejects absent privileged action",
    )?;
    // Deliberate hostile dispatch bypasses native-control admission: production
    // application authorization must independently reject the same action.
    app.event(Event::Submit(TO_BACKUP, FormData::new()))?;
    pump(app, deadline)?;
    require(
        app.screen == Screen::Login
            && app.lease.is_none()
            && app.backup.editor.is_none()
            && app.backup.review.is_none(),
        "forged role entry independently rejected",
    )
}
fn seed_roles(app: &mut App) -> Check<()> {
    let lease = app.lease.ok_or("recovery-probe: seed lease absent")?;
    let mut backend = checked(
        app.backend
            .as_ref()
            .ok_or("recovery-probe: seed backend absent")?
            .lock(),
        "seed backend lock",
    )?;
    checked(
        backend.identity(
            lease,
            IdentityCommand::CreateClass {
                id: ClassId::new(1).unwrap(),
                name: "Synthetic class".into(),
            },
        ),
        "seed class",
    )?;
    checked(
        backend.identity(
            lease,
            IdentityCommand::PutSubject {
                id: SubjectId::new(1).unwrap(),
                name: "Synthetic subject".into(),
                enabled: true,
                color: Rgb {
                    red: 20,
                    green: 80,
                    blue: 140,
                },
                expected_revision: None,
            },
        ),
        "seed subject",
    )?;
    for (id, login, role, scopes) in [
        (2, "recovery-probe-user", Role::User, vec![]),
        (
            3,
            "recovery-probe-teacher",
            Role::TeacherAdmin,
            vec![AssignmentScope {
                class_id: ClassId::new(1).unwrap(),
                subject_id: SubjectId::new(1).unwrap(),
            }],
        ),
    ] {
        drop(checked(
            backend.create_account(
                lease,
                NewAccount {
                    id: PrincipalId::new(id).unwrap(),
                    login_name: login.into(),
                    display_name: login.into(),
                    role,
                    scopes,
                },
                checked(
                    Password::new(ACCOUNT_PASSWORD.into()),
                    "synthetic account password",
                )?,
            ),
            "seed scoped credentials",
        )?);
    }
    app.state = Arc::new(checked(backend.state(), "seed identity refresh")?);
    Ok(())
}
fn exercise(
    app: &mut App,
    root: &Path,
    deadline: Instant,
    steps: &mut usize,
) -> Check<SessionLease> {
    require(
        app.screen == Screen::Setup,
        "fresh deployment opens setup via production worker",
    )?;
    mark(steps, "worker-open-fresh-dpapi-and-sqlite");
    submit(
        app,
        SAVE,
        &[
            (SCHOOL, ORIGINAL_SCHOOL),
            (LOGIN, LOGIN_NAME),
            (NAME, "Synthetic recovery administrator"),
            (PASSWORD, INITIAL_PASSWORD),
            (CONFIRM, INITIAL_PASSWORD),
        ],
        deadline,
    )?;
    require(
        app.screen == Screen::Code,
        "first administrator real credential commit",
    )?;
    submit(app, ACK, &[], deadline)?;
    login(app, LOGIN_NAME, INITIAL_PASSWORD, deadline)?;
    require(
        app.screen == Screen::Home && app.audio.is_some(),
        "real administrator login and audio ownership",
    )?;
    mark(steps, "native-setup-login-with-real-credential-codec");
    seed_roles(app)?;
    submit(app, LOCK, &[], deadline)?;
    deny_entry(app, "recovery-probe-user", deadline)?;
    mark(steps, "ordinary-user-native-and-forged-entry-denied");
    deny_entry(app, "recovery-probe-teacher", deadline)?;
    mark(steps, "scoped-teacher-native-and-forged-entry-denied");
    login(app, LOGIN_NAME, INITIAL_PASSWORD, deadline)?;
    open_native_backup(app, deadline)?;
    require(
        app.screen == Screen::Backup,
        "administrator native backup entry",
    )?;
    mark(steps, "administrator-native-backup-entry");
    let baseline = revision(app)?;
    submit(app, b::EXPORT, &[], deadline)?;
    let receipt = app
        .backup
        .exported
        .clone()
        .ok_or("recovery-probe: export receipt absent")?;
    require(
        receipt.bytes > 0 && revision(app)? == baseline,
        "export read-only consistent snapshot",
    )?;
    mark(steps, "native-export-real-database-and-package");
    app.save_backup_to(checked(
        BackupDestination::isolated_probe(root, COPY_NAME),
        "isolated save picker capability",
    )?)?;
    pump(app, deadline)?;
    let copied = checked(
        fs::read(root.join("quarantine").join(COPY_NAME)),
        "read synthetic saved package",
    )?;
    require(
        copied.len() as u64 == receipt.bytes
            && fc_security::sha256(&copied) == receipt.package_sha256,
        "opaque-destination saved bytes and sha256 match trusted export",
    )?;
    drop(copied);
    mark(steps, "native-save-worker-verified-package-copy");
    preflight(app, root, deadline)?;
    require(
        revision(app)? == baseline && school(app)? == ORIGINAL_SCHOOL,
        "full preflight does not modify live teaching",
    )?;
    mark(steps, "full-preflight-review-live-unchanged");
    submit(app, b::CONFIRM, &[], deadline)?;
    require(
        !app.backup.irreversible() && revision(app)? == baseline && app.backup.review.is_some(),
        "default-unchecked confirmation rejected",
    )?;
    mark(steps, "native-unchecked-confirmation-rejected");
    submit(app, b::CANCEL, &[], deadline)?;
    require(
        app.backup.review.is_none()
            && app
                .backup
                .workflow
                .as_ref()
                .is_some_and(RecoveryWorkflow::is_idle)
            && revision(app)? == baseline,
        "cancel releases actual review slot after worker retirement",
    )?;
    mark(steps, "cancel-retires-actual-workspace-on-worker");
    preflight(app, root, deadline)?;
    let old_ack = ack(app)?;
    submit(app, LOCK, &[], deadline)?;
    require(
        app.screen == Screen::Login
            && app.backup.review.is_none()
            && app.backup.editor.is_none()
            && app
                .backup
                .workflow
                .as_ref()
                .is_some_and(RecoveryWorkflow::is_idle),
        "lock destroys owned review and physically retires slot",
    )?;
    mark(steps, "lock-invalidates-confirmation-and-retires-slot");
    login(app, LOGIN_NAME, INITIAL_PASSWORD, deadline)?;
    open_native_backup(app, deadline)?;
    preflight(app, root, deadline)?;
    let mut stale = FormData::new();
    checked(
        stale.insert(b::ACK, old_ack),
        "stale acknowledgement fixture",
    )?;
    app.event(Event::Submit(b::CONFIRM, stale))?;
    pump(app, deadline)?;
    require(
        !app.backup.irreversible() && app.backup.review.is_some() && revision(app)? == baseline,
        "old native-review acknowledgement cannot confirm new review",
    )?;
    mark(steps, "old-review-ack-rejected-after-new-preflight");
    submit(app, b::CANCEL, &[], deadline)?;
    submit(app, b::CLOSE, &[], deadline)?;
    submit(app, TO_PASSWORD, &[], deadline)?;
    submit(
        app,
        SAVE,
        &[
            (PASSWORD, INITIAL_PASSWORD),
            (NEW_PASSWORD, CURRENT_PASSWORD),
            (CONFIRM, CURRENT_PASSWORD),
        ],
        deadline,
    )?;
    require(
        app.screen == Screen::Login,
        "current password changed after export",
    )?;
    login(app, LOGIN_NAME, CURRENT_PASSWORD, deadline)?;
    mark(steps, "current-password-changed-after-export");
    rename(app, ALTERED_SCHOOL)?;
    open_native_backup(app, deadline)?;
    preflight(app, root, deadline)?;
    require(
        school(app)? == ALTERED_SCHOOL,
        "review does not apply backed-up school name",
    )?;
    mark(steps, "preflight-newer-teaching-live-unchanged");
    let stale_ack = ack(app)?;
    rename(app, STALE_SCHOOL)?;
    let newer = revision(app)?;
    submit(app, b::CONFIRM, &[(b::ACK, &stale_ack)], deadline)?;
    require(
        !app.backup.irreversible() && revision(app)? == newer && school(app)? == STALE_SCHOOL,
        "stale-revision confirmation has no recovery barrier or teaching effect",
    )?;
    mark(steps, "stale-live-revision-confirmation-rejected");
    // A pre-barrier failure consumes its confirmation and must return to a
    // usable fresh page without reviving the old review capability.
    if app.screen == Screen::Home {
        open_native_backup(app, deadline)?;
    }
    require(
        app.view()
            .actions
            .iter()
            .any(|a| a.id == b::SELECT_RESTORE && a.enabled),
        "pre-barrier failure permits a fresh selection without reviving old confirmation",
    )?;
    preflight(app, root, deadline)?;
    let current_ack = ack(app)?;
    let old_lease = app.lease.ok_or("recovery-probe: final lease absent")?;
    mark(steps, "fresh-complete-preflight-after-stale-rejection");
    submit(app, b::CONFIRM, &[(b::ACK, &current_ack)], deadline)?;
    require(
        app.backup.irreversible() && app.screen == Screen::Restoring && app.lease.is_none(),
        "native explicit confirmation hands off to irreversible progress",
    )?;
    mark(
        steps,
        "explicit-native-confirm-revokes-session-and-handoffs",
    );
    require(
        app.backup
            .workflow
            .as_ref()
            .is_some_and(|w| w.confirmation_state() == ConfirmationState::Confirmed),
        "slot retains durable confirmed capability",
    )?;
    mark(steps, "durable-barrier-capability-retained-in-slot");
    while !app.backup.finished {
        tick(app, deadline)?;
        std::thread::sleep(Duration::from_millis(5));
    }
    require(
        app.backup
            .coordinator
            .as_ref()
            .is_some_and(|c| c.status().result.as_ref().is_some_and(|r| r.is_ok())),
        "coordinator physically joined and whole cold switch successful",
    )?;
    require(
        app.audio.is_none() && app.pool.is_finished(),
        "actual worker and audio ownership transferred and drained",
    )?;
    require(
        !app.backup.exit_ready(),
        "completion requires explicit native close acknowledgement",
    )?;
    mark(steps, "physical-worker-audio-join-and-cold-commit-complete");
    let close = app
        .view()
        .actions
        .iter()
        .find(|a| a.enabled)
        .ok_or("recovery-probe: close result action absent")?
        .id;
    submit(app, close, &[], deadline)?;
    require(
        app.backup.exit_ready(),
        "native result acknowledgement enables safe exit",
    )?;
    mark(steps, "native-result-acknowledged-before-exit");
    Ok(old_lease)
}
fn ordinary_shutdown(app: &mut App, deadline: Instant) -> Check<()> {
    app.begin_shutdown();
    while !app.pool.is_finished() || !app.backup.retirement_complete() {
        tick(app, deadline)?;
        std::thread::sleep(Duration::from_millis(5));
    }
    // App's joining worker already closed SQLite. The owned pool is joined by
    // the caller after moving it out of the App.
    Ok(())
}
fn join_app(app: App) -> Check<()> {
    let App { pool, .. } = app;
    checked(pool.join(), "join closed App shared workers")
}
fn capture_child_output(
    mut input: impl Read + Send + 'static,
) -> std::thread::JoinHandle<std::io::Result<(Vec<u8>, bool)>> {
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut overflow = false;
        let mut buffer = [0u8; 4096];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                return Ok((output, overflow));
            }
            let keep = count.min(CHILD_OUTPUT_LIMIT - output.len());
            output.extend_from_slice(&buffer[..keep]);
            overflow |= keep != count;
            // Continue draining bounded chunks even after the memory limit so
            // the owned diagnostic child cannot stall on a full pipe.
        }
    })
}
fn subprocess_reopen(root: &Path, deadline: Instant) -> Check<()> {
    require(Instant::now() < deadline, "subprocess launch deadline")?;
    let mut random = [0u8; 16];
    checked(SystemRandom.fill(&mut random), "owned marker CSPRNG nonce")?;
    let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut marker = checked(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(OWNER_MARKER)),
        "create exclusive owned marker",
    )?;
    checked(marker.write_all(nonce.as_bytes()), "write owned marker")?;
    checked(marker.sync_all(), "flush owned marker")?;
    drop(marker);
    let mut child = checked(
        Command::new(checked(
            std::env::current_exe(),
            "same diagnostic executable",
        )?)
        .arg("--recovery-probe-reopen")
        .arg(root)
        .arg(&nonce)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // CREATE_NO_WINDOW: this diagnostic must never acquire a console or
        // activate a foreground window when it validates persisted state.
        .creation_flags(0x0800_0000)
        .spawn(),
        "spawn independent reopen process",
    )?;
    let stdout = capture_child_output(
        child
            .stdout
            .take()
            .ok_or("recovery-probe: child stdout absent")?,
    );
    let stderr = capture_child_output(
        child
            .stderr
            .take()
            .ok_or("recovery-probe: child stderr absent")?,
    );
    let mut expired = false;
    let mut poll_failed = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                poll_failed = true;
                let _ = child.kill();
                break checked(child.wait(), "join child after process polling failure")?;
            }
        }
        if Instant::now() >= deadline {
            expired = true;
            let _ = child.kill();
            break checked(
                child.wait(),
                "join terminated deadline child before retaining deployment",
            )?;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let (out, out_overflow) = checked(
        checked(stdout.join(), "join child stdout capture")?,
        "child stdout read",
    )?;
    let (err, err_overflow) = checked(
        checked(stderr.join(), "join child stderr capture")?,
        "child stderr read",
    )?;
    let output = String::from_utf8_lossy(&out);
    print!("{output}");
    eprint!("{}", String::from_utf8_lossy(&err));
    require(
        !poll_failed,
        "independent reopen polling failed; terminated child and retained owned root",
    )?;
    require(
        !expired,
        "independent reopen exceeded total deadline; owned root retained",
    )?;
    require(
        !out_overflow && !err_overflow,
        "independent reopen output exceeded bound; owned root retained",
    )?;
    require(
        status.success(),
        "independent reopen process failed; owned root retained",
    )?;
    require(output.lines().any(|line|line==format!("RECOVERY_SUBPROCESS_REOPEN_PASS nonce={nonce} school_restored=PASS current_password=PASS physical_shutdown=PASS")),"independent process exact bound completion marker")
}
/// Invoked only by this diagnostic's separately launched executable. This path
/// never creates a deployment: it requires the strict temp-root shape, protected
/// existing ACLs, exclusive instance ownership and a matching pinned marker.
pub(super) fn reopen(root: &Path, nonce: &str) -> Check<()> {
    let suffix = root
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("FutureClassroom-Test-Recovery-"))
        .ok_or("recovery-probe-reopen: unowned root prefix")?;
    let parts: Vec<_> = suffix.split('-').collect();
    require(
        root.is_absolute()
            && root.parent() == Some(std::env::temp_dir().as_path())
            && parts.len() == 2
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            && nonce.len() == 32
            && nonce
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "strict child owned root and nonce",
    )?;
    let paths = checked(
        DataPaths::isolated_test(root.to_owned()),
        "existing child deployment path",
    )?;
    checked(
        paths.verify_root_for_current_user(),
        "verify existing child ACL without creation",
    )?;
    let _root_pin = checked(PinnedDirectory::open(root), "pin existing child deployment")?;
    let mut marker = checked(
        LockedFile::open(&root.join(OWNER_MARKER), 32),
        "pin owned marker without reparse or hardlinks",
    )?;
    require(
        checked(marker.len(), "owned marker length")? == 32,
        "exact owned marker length",
    )?;
    let mut bytes = [0u8; 32];
    checked(
        marker.file_mut().read_exact(&mut bytes),
        "read pinned marker",
    )?;
    require(
        bytes.as_slice() == nonce.as_bytes(),
        "owned marker matches process nonce",
    )?;
    let _instance = checked(
        InstanceGuard::acquire(root),
        "independent child instance guard",
    )?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let clock = Arc::new(CurrentClock::new());
    let mut app = create_app(clock.clone(), true)?;
    app.root = root.to_owned();
    app.desktop.set_root(root.to_owned());
    app.backup.workflow = Some(RecoveryWorkflow::new(paths.clone()));
    app.next_content_maintenance = deadline;
    app.next_voice_maintenance = deadline;
    app.next_calendar_maintenance = deadline;
    app.submit(Work::Open(paths, clock))?;
    pump(&mut app, deadline)?;
    require(
        app.screen == Screen::Login && school(&app)? == ORIGINAL_SCHOOL,
        "independent process production startup restores teaching and stays locked",
    )?;
    login(&mut app, LOGIN_NAME, CURRENT_PASSWORD, deadline)?;
    require(
        app.screen == Screen::Home,
        "independent process authenticates current post-backup password",
    )?;
    ordinary_shutdown(&mut app, deadline)?;
    join_app(app)?;
    println!("RECOVERY_SUBPROCESS_REOPEN_PASS nonce={nonce} school_restored=PASS current_password=PASS physical_shutdown=PASS");
    Ok(())
}
pub(super) fn run() -> Check<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(250);
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-Recovery-{}-{}",
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
    let instance = checked(
        InstanceGuard::acquire(paths.root()),
        "isolated instance lock",
    )?;
    let clock = Arc::new(CurrentClock::new());
    let mut app = create_app(clock.clone(), true)?;
    app.root = root.clone();
    app.desktop.set_root(root.clone());
    app.backup.workflow = Some(RecoveryWorkflow::new(paths.clone()));
    app.next_content_maintenance = deadline;
    app.next_voice_maintenance = deadline;
    app.next_calendar_maintenance = deadline;
    let mut steps = 0;
    app.submit(Work::Open(paths.clone(), clock))?;
    let exercised =
        pump(&mut app, deadline).and_then(|()| exercise(&mut app, &root, deadline, &mut steps));
    let old_lease = match exercised {
        Ok(lease) => lease,
        Err(error) => {
            eprintln!(
                "RECOVERY_NATIVE_FAILURE steps={steps} root_retained={} error={error}",
                root.display()
            );
            // Never recursively remove a workspace while a real worker or final
            // coordinator might still own it. The outer harness records this
            // failure and cleans only after the diagnostic process is terminal.
            if !app.backup.irreversible() {
                let _ = ordinary_shutdown(&mut app, deadline);
            }
            return Err(error);
        }
    };
    join_app(app)?;
    // A fresh native App and new production Open worker reread DPAPI, reconcile
    // the journal and open SQLite; no in-memory fixture state is carried over.
    let clock = Arc::new(CurrentClock::new());
    let mut reopened = create_app(clock.clone(), true)?;
    reopened.root = root.clone();
    reopened.desktop.set_root(root.clone());
    reopened.backup.workflow = Some(RecoveryWorkflow::new(paths.clone()));
    reopened.next_content_maintenance = deadline;
    reopened.next_voice_maintenance = deadline;
    reopened.next_calendar_maintenance = deadline;
    reopened.submit(Work::Open(paths, clock))?;
    pump(&mut reopened, deadline)?;
    require(
        reopened.screen == Screen::Login && school(&reopened)? == ORIGINAL_SCHOOL,
        "fresh production startup reconciles restored teaching and stays locked",
    )?;
    mark(
        &mut steps,
        "fresh-App-production-Open-restores-school-and-locks",
    );
    login(&mut reopened, LOGIN_NAME, CURRENT_PASSWORD, deadline)?;
    require(
        reopened.screen == Screen::Home,
        "current post-backup credentials retained on reopen",
    )?;
    {
        let backend = checked(
            reopened
                .backend
                .as_ref()
                .ok_or("recovery-probe: reopened backend absent")?
                .lock(),
            "reopened backend lock",
        )?;
        require(
            backend.management_identity(old_lease).is_err(),
            "pre-restore lease cannot authorize fresh backend",
        )?;
    }
    mark(&mut steps, "current-password-login-and-old-lease-rejection");
    ordinary_shutdown(&mut reopened, deadline)?;
    join_app(reopened)?;
    drop(instance);
    drop(deployment);
    subprocess_reopen(&root, deadline)?;
    mark(
        &mut steps,
        "independent-executable-production-reopen-and-current-password-login",
    );
    checked(
        fs::remove_dir_all(&root),
        "remove only physically closed synthetic deployment",
    )?;
    println!("backup_export_save_preflight_cancel_lock_stale_explicit_cold_restore_native_worker=PASS recovery_native_steps={steps} reopen=fresh_App_AND_independent_executable_production_Open old_lease=fresh_App_only picker=ISOLATED_CAPABILITY_ONLY visible_dialog=NOT_TESTED foreground=UNTOUCHED real_run=READ_ONLY isolated_cleanup=PASS elapsed_ms={}",started.elapsed().as_millis());
    Ok(())
}
