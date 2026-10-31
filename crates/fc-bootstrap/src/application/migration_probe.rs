//! Hidden native mapping controls, real protected source/target deployments and
//! an independent production reopen. Only nonce-marked synthetic roots are used.
use super::*;
use backup_probe::{join_app, open_native_backup, ordinary_shutdown, pump, submit};
use fc_bootstrap::recovery_workflow::RecoveryWorkflow;
use fc_protocol::recovery::PackageReader;
use fc_windows::materials::{
    chooser::{BackupDestination, SelectedFile},
    files::{LockedFile, PinnedDirectory},
};
use migration_editor as m;
use migration_probe_fixture as fixture;
use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

type Check<T> = std::result::Result<T, String>;
const SOURCE_LOGIN: &str = "migration-source-admin";
const TARGET_LOGIN: &str = "migration-target-admin";
const SOURCE_PASSWORD: &str = "Synthetic migration source admin 2026!";
const TARGET_PASSWORD: &str = "Synthetic migration target admin 2026!";
const SOURCE_SCHOOL: &str = "Synthetic migration source school";
const TARGET_SCHOOL: &str = "Synthetic migration preserved target school";
const COPY: &str = "migration.fcbak";
const MARKER: &str = "migration-probe.marker";
const EXPECTED: &str = "migration-probe-expected.bin";

fn checked<T, E: std::fmt::Debug>(v: std::result::Result<T, E>, stage: &str) -> Check<T> {
    v.map_err(|e| format!("migration-probe: {stage}: {e:?}"))
}
fn require(v: bool, stage: &str) -> Check<()> {
    if v {
        Ok(())
    } else {
        Err(format!("migration-probe: {stage}"))
    }
}
fn mark(steps: &mut usize, name: &str) {
    println!("MIGRATION_NATIVE_STEP index={} name={name}", *steps);
    *steps += 1;
}
fn new_app(paths: &DataPaths, deadline: Instant) -> Check<App> {
    let clock = Arc::new(CurrentClock::new());
    let mut app = create_app(clock.clone(), true)?;
    app.root = paths.root().to_owned();
    app.desktop.set_root(paths.root().to_owned());
    app.backup.workflow = Some(RecoveryWorkflow::new(paths.clone()));
    app.next_content_maintenance = deadline;
    app.next_voice_maintenance = deadline;
    app.next_calendar_maintenance = deadline;
    app.submit(Work::Open(paths.clone(), clock))?;
    pump(&mut app, deadline)?;
    Ok(app)
}
fn setup(app: &mut App, source: bool, deadline: Instant) -> Check<()> {
    require(app.screen == Screen::Setup, "fresh production setup")?;
    let (school, login, password) = if source {
        (SOURCE_SCHOOL, SOURCE_LOGIN, SOURCE_PASSWORD)
    } else {
        (TARGET_SCHOOL, TARGET_LOGIN, TARGET_PASSWORD)
    };
    submit(
        app,
        SAVE,
        &[
            (SCHOOL, school),
            (LOGIN, login),
            (NAME, login),
            (PASSWORD, password),
            (CONFIRM, password),
        ],
        deadline,
    )?;
    require(
        app.screen == Screen::Code,
        "real first credential committed",
    )?;
    submit(app, ACK, &[], deadline)?;
    submit(
        app,
        LOG_IN,
        &[(LOGIN, login), (PASSWORD, password)],
        deadline,
    )?;
    require(
        app.screen == Screen::Home,
        "current administrator authenticated",
    )
}
fn package_manifest(path: &Path) -> Check<fc_domain::recovery::BackupManifest> {
    Ok(checked(
        PackageReader::new(checked(File::open(path), "open own package")?),
        "parse own manifest",
    )?
    .manifest()
    .clone())
}
fn blob_sha(root: &Path) -> Check<[u8; 32]> {
    let bytes = checked(
        fs::read(root.join("config/credential-key.dpapi")),
        "own DPAPI ciphertext",
    )?;
    require(bytes.len() <= 4096, "bounded own key ciphertext")?;
    Ok(fc_security::sha256(&bytes))
}
fn revision(app: &App) -> Check<Revision> {
    checked(
        checked(
            app.backend.as_ref().ok_or("backend absent")?.lock(),
            "backend lock",
        )?
        .global_revision(),
        "current revision",
    )
}
fn inspect(app: &mut App, paths: &DataPaths, deadline: Instant) -> Check<()> {
    app.inspect_selected_migration(checked(
        SelectedFile::isolated_probe(paths.root(), COPY),
        "own mapping picker",
    )?)?;
    pump(app, deadline)?;
    require(
        app.backup.mapping.is_some() && app.backup.migration.is_some(),
        "source-only inspection ready",
    )
}
fn choose_page(app: &mut App, deadline: Instant) -> Check<bool> {
    let view = app.view();
    let mut values = Vec::new();
    for field in &view.fields {
        let FieldKind::Choice(choices) = &field.kind else {
            continue;
        };
        let parts: Vec<_> = field.value.split('-').collect();
        require(
            parts.len() == 6 && parts[0] == "map" && parts[5] == "none",
            "no automatic mapping",
        )?;
        let target = match (parts[3], parts[4]) {
            ("class", "1") => "11",
            ("subject", "1") => "12",
            ("account", "1") => "1",
            ("account", "2") => "22",
            _ => return Err("migration-probe: unexpected synthetic source row".into()),
        };
        let selected = choices
            .iter()
            .find(|c| c.value.rsplit('-').next() == Some(target))
            .ok_or("migration-probe: explicit current target missing")?;
        require(
            selected.label.starts_with(&format!("#{target} ")),
            "target label agrees with explicit ID",
        )?;
        values.push((field.id, selected.value.clone()));
    }
    let overrides: Vec<_> = values
        .iter()
        .map(|(id, value)| (*id, value.as_str()))
        .collect();
    let next = view.actions.iter().any(|a| a.id == m::NEXT && a.enabled);
    submit(
        app,
        if next { m::NEXT } else { m::SAVE_DRAFT },
        &overrides,
        deadline,
    )?;
    Ok(next)
}
fn ack(app: &App, id: FieldId) -> Check<String> {
    let view = app.view();
    let field = view
        .fields
        .iter()
        .find(|f| f.id == id)
        .ok_or("migration-probe: ack absent")?;
    require(
        field.value == "0",
        "both acknowledgements default unchecked",
    )?;
    let FieldKind::Choice(choices) = &field.kind else {
        return Err("migration-probe: ack is not choice".into());
    };
    choices
        .iter()
        .find(|c| c.value != "0")
        .map(|c| c.value.clone())
        .ok_or("migration-probe: explicit ack absent".into())
}
fn finish(app: &mut App, deadline: Instant) -> Check<()> {
    while !app.backup.finished {
        require(Instant::now() < deadline, "physical cold-switch deadline")?;
        for event in checked(app.window.take_events(), "cold-switch native events")? {
            app.event(event)?;
        }
        app.poll()?;
        std::thread::sleep(Duration::from_millis(5));
    }
    require(
        app.backup
            .coordinator
            .as_ref()
            .is_some_and(|c| c.status().result.as_ref().is_some_and(|r| r.is_ok()))
            && app.audio.is_none()
            && app.pool.is_finished()
            && !app.backup.exit_ready(),
        "actual coordinator/SQLite/audio drain and three-tree cold commit",
    )
}
fn exercise_target(
    app: &mut App,
    paths: &DataPaths,
    deadline: Instant,
    steps: &mut usize,
) -> Check<()> {
    let before = revision(app)?;
    require(
        app.screen == Screen::Backup,
        "target baseline backup page remains open",
    )?;
    app.preflight_selected_backup(checked(
        SelectedFile::isolated_probe(paths.root(), COPY),
        "own ordinary picker",
    )?)?;
    pump(app, deadline)?;
    require(
        app.backup.review.is_none() && !app.backup.irreversible() && revision(app)? == before,
        "ordinary foreign installation stays denied",
    )?;
    mark(steps, "ordinary-foreign-installation-denied");
    app.inspect_selected_migration(checked(
        SelectedFile::isolated_probe(paths.root(), COPY),
        "pending own picker",
    )?)?;
    require(app.backup.job.is_some(), "inspection is actually pending")?;
    submit(app, backup_editor::CANCEL, &[], deadline)?;
    require(
        app.backup.mapping.is_none()
            && app.backup.migration.is_none()
            && app
                .backup
                .workflow
                .as_ref()
                .is_some_and(RecoveryWorkflow::is_idle)
            && app.backup.migration_inspection_sequence.is_none(),
        "native pending cancel cannot publish late source",
    )?;
    mark(steps, "pending-inspection-native-cancel-retired");
    inspect(app, paths, deadline)?;
    submit(app, m::PREFLIGHT, &[], deadline)?;
    require(
        app.backup.mapping.is_some() && app.backup.review.is_none() && revision(app)? == before,
        "incomplete mapping cannot start full preflight",
    )?;
    mark(steps, "incomplete-mapping-preflight-rejected");
    let mut pages = 0;
    loop {
        pages += 1;
        require(pages <= 8, "bounded synthetic mapping pages")?;
        if !choose_page(app, deadline)? {
            break;
        }
    }
    mark(steps, "explicit-class-subject-principal-choices");
    submit(app, m::PREFLIGHT, &[], deadline)?;
    require(
        app.backup.mapping.is_none()
            && app.backup.review.is_some()
            && revision(app)? == before
            && !app.backup.irreversible(),
        "mapped full preflight keeps live unchanged",
    )?;
    mark(steps, "mapped-full-preflight-live-unchanged");
    submit(app, m::CONFIRM, &[], deadline)?;
    require(
        app.backup.review.is_some() && !app.backup.irreversible() && revision(app)? == before,
        "default acknowledgement cannot confirm",
    )?;
    mark(steps, "native-default-confirmation-rejected");
    let replacement = ack(app, m::ACK_REPLACEMENT)?;
    let authority = ack(app, m::ACK_AUTHORITY)?;
    submit(
        app,
        m::CONFIRM,
        &[(m::ACK_REPLACEMENT, &replacement)],
        deadline,
    )?;
    require(
        app.backup.review.is_some() && !app.backup.irreversible() && revision(app)? == before,
        "partial acknowledgement cannot confirm",
    )?;
    mark(steps, "native-partial-confirmation-rejected");
    submit(
        app,
        m::CONFIRM,
        &[
            (m::ACK_REPLACEMENT, &replacement),
            (m::ACK_AUTHORITY, &authority),
        ],
        deadline,
    )?;
    require(
        app.backup.irreversible() && app.screen == Screen::Restoring && app.lease.is_none(),
        "explicit double consent revokes current session and hands off",
    )?;
    mark(steps, "explicit-double-ack-revokes-and-handoffs");
    finish(app, deadline)?;
    mark(steps, "physical-shutdown-and-three-tree-cold-commit");
    let close = app
        .view()
        .actions
        .iter()
        .find(|a| a.enabled)
        .ok_or("result close absent")?
        .id;
    submit(app, close, &[], deadline)?;
    require(app.backup.exit_ready(), "native result close acknowledged")?;
    mark(steps, "native-result-close-acknowledged");
    Ok(())
}
fn subprocess(root: &Path, nonce: &str, deadline: Instant) -> Check<()> {
    let mut child = checked(
        Command::new(checked(std::env::current_exe(), "current EXE")?)
            .arg("--migration-probe-reopen")
            .arg(root)
            .arg(nonce)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x0800_0000)
            .spawn(),
        "owned independent child",
    )?;
    let stdout =
        backup_probe::capture_child_output(child.stdout.take().ok_or("child stdout absent")?);
    let stderr =
        backup_probe::capture_child_output(child.stderr.take().ok_or("child stderr absent")?);
    let mut expired = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            result => {
                expired = true;
                let _ = child.kill();
                eprintln!("owned migration child deadline/poll failure: {result:?}");
                break checked(
                    child.wait(),
                    "join terminated own child before preserving root",
                )?;
            }
        }
    };
    let (out, overflow) = checked(
        checked(stdout.join(), "join child stdout")?,
        "read child stdout",
    )?;
    let (err, err_overflow) = checked(
        checked(stderr.join(), "join child stderr")?,
        "read child stderr",
    )?;
    let out = String::from_utf8_lossy(&out);
    print!("{out}");
    eprint!("{}", String::from_utf8_lossy(&err));
    require(
        !expired && !overflow && !err_overflow && status.success(),
        "independent reopen completes physically",
    )?;
    require(out.lines().any(|line|line==format!("MIGRATION_SUBPROCESS_REOPEN_PASS nonce={nonce} target_credentials=PASS mapped_content=PASS physical_shutdown=PASS")),"exact nonce-bound child completion")
}
pub(super) fn reopen(root: &Path, nonce: &str) -> Check<()> {
    let suffix = root
        .file_name()
        .and_then(|x| x.to_str())
        .and_then(|x| x.strip_prefix("FutureClassroom-Test-Migration-t-"))
        .ok_or("migration child root prefix")?;
    let (pid, bound) = suffix.split_once('-').ok_or("migration child root shape")?;
    require(
        root.is_absolute()
            && root.parent() == Some(std::env::temp_dir().as_path())
            && !pid.is_empty()
            && pid.bytes().all(|b| b.is_ascii_digit())
            && !bound.is_empty()
            && bound.bytes().all(|b| b.is_ascii_digit())
            && nonce.len() == 32
            && nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "strict own child root/nonce",
    )?;
    let paths = checked(
        DataPaths::isolated_test(root.to_owned()),
        "existing child paths",
    )?;
    checked(paths.verify_root_for_current_user(), "existing child ACL")?;
    let _root_pin = checked(PinnedDirectory::open(root), "existing child root pin")?;
    let mut marker = checked(
        LockedFile::open(&root.join("runtime").join(MARKER), 32),
        "pin own marker",
    )?;
    let mut marker_bytes = [0; 32];
    require(
        checked(marker.len(), "marker length")? == 32,
        "exact marker length",
    )?;
    checked(
        marker.file_mut().read_exact(&mut marker_bytes),
        "read marker",
    )?;
    require(
        marker_bytes.as_slice() == nonce.as_bytes(),
        "matching pinned marker",
    )?;
    let mut expectation = checked(
        LockedFile::open(&root.join("runtime").join(EXPECTED), 64),
        "pin own expectation",
    )?;
    require(
        checked(expectation.len(), "expectation length")? == 64,
        "exact expectation length",
    )?;
    let mut expected = [0; 64];
    checked(
        expectation.file_mut().read_exact(&mut expected),
        "read nonsecret expectation",
    )?;
    require(
        blob_sha(root)?.as_slice() == &expected[32..],
        "target DPAPI ciphertext retained",
    )?;
    let _instance = checked(InstanceGuard::acquire(root), "independent current instance")?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut app = new_app(&paths, deadline)?;
    require(
        app.screen == Screen::Login,
        "independent normal Open stays locked",
    )?;
    submit(
        &mut app,
        LOG_IN,
        &[(LOGIN, TARGET_LOGIN), (PASSWORD, TARGET_PASSWORD)],
        deadline,
    )?;
    require(
        app.screen == Screen::Home,
        "independent current target password login",
    )?;
    fixture::verify_mapped(&mut app, &paths)?;
    require(
        app.state
            .installation
            .as_ref()
            .is_some_and(|s| s.school_name == TARGET_SCHOOL)
            && app.state.accounts.iter().all(|a| {
                a.login_name != SOURCE_LOGIN && a.login_name != fixture::SOURCE_TEACHER_LOGIN
            }),
        "current target school/account names retained without source adoption",
    )?;
    open_native_backup(&mut app, deadline)?;
    submit(&mut app, backup_editor::EXPORT, &[], deadline)?;
    let receipt = app.backup.exported.as_ref().ok_or("reopen export absent")?;
    require(
        package_manifest(&receipt.path)?.installation_id.as_slice() == &expected[..32],
        "current target installation retained",
    )?;
    ordinary_shutdown(&mut app, deadline)?;
    join_app(app)?;
    println!("MIGRATION_SUBPROCESS_REOPEN_PASS nonce={nonce} target_credentials=PASS mapped_content=PASS physical_shutdown=PASS");
    Ok(())
}
pub(super) fn run() -> Check<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(280);
    let mut random = [0; 16];
    checked(SystemRandom.fill(&mut random), "CSPRNG owner nonce")?;
    let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
    // Keep synthetic roots short enough to exercise the unchanged legacy Win7
    // file APIs. The full CSPRNG nonce remains bound by the pinned child marker.
    let root_suffix = u32::from_le_bytes(random[..4].try_into().unwrap());
    let root_for = |kind| {
        std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Migration-{kind}-{}-{root_suffix}",
            std::process::id()
        ))
    };
    let source = checked(
        DataPaths::isolated_test(root_for("s")),
        "source synthetic paths",
    )?;
    let target = checked(
        DataPaths::isolated_test(root_for("t")),
        "target synthetic paths",
    )?;
    let source_deployment = checked(source.prepare_for_current_user(), "source protected root")?;
    checked(source_deployment.read_back(), "source ACL readback")?;
    let source_instance = checked(
        InstanceGuard::acquire(source.root()),
        "source exclusive instance",
    )?;
    let mut steps = 0;
    let mut app = new_app(&source, deadline)?;
    setup(&mut app, true, deadline)?;
    fixture::seed_source(&mut app, &source)?;
    mark(&mut steps, "source-native-setup-real-teaching");
    open_native_backup(&mut app, deadline)?;
    submit(&mut app, backup_editor::EXPORT, &[], deadline)?;
    let source_receipt = app.backup.exported.clone().ok_or("source export absent")?;
    app.save_backup_to(checked(
        BackupDestination::isolated_probe(source.root(), COPY),
        "source isolated save",
    )?)?;
    pump(&mut app, deadline)?;
    let source_copy = source.root().join("quarantine").join(COPY);
    let bytes = checked(fs::read(&source_copy), "saved synthetic source package")?;
    require(
        bytes.len() as u64 == source_receipt.bytes
            && fc_security::sha256(&bytes) == source_receipt.package_sha256,
        "saved source bytes/SHA",
    )?;
    drop(bytes);
    let source_manifest = package_manifest(&source_copy)?;
    mark(&mut steps, "source-package-export-saved");
    ordinary_shutdown(&mut app, deadline)?;
    join_app(app)?;
    drop(source_instance);
    drop(source_deployment);
    let target_deployment = checked(target.prepare_for_current_user(), "target protected root")?;
    checked(target_deployment.read_back(), "target ACL readback")?;
    let target_instance = checked(
        InstanceGuard::acquire(target.root()),
        "target exclusive instance",
    )?;
    let mut app = new_app(&target, deadline)?;
    let result = (|| {
        setup(&mut app, false, deadline)?;
        fixture::seed_target(&mut app, &target)?;
        mark(&mut steps, "target-native-setup-distinct-authority");
        open_native_backup(&mut app, deadline)?;
        submit(&mut app, backup_editor::EXPORT, &[], deadline)?;
        let receipt = app
            .backup
            .exported
            .as_ref()
            .ok_or("target baseline export absent")?;
        let target_manifest = package_manifest(&receipt.path)?;
        require(
            target_manifest.installation_id != source_manifest.installation_id,
            "independently generated installations",
        )?;
        let key_sha = blob_sha(target.root())?;
        let mut expected = Vec::from(target_manifest.installation_id);
        expected.extend_from_slice(&key_sha);
        checked(
            fs::write(target.root().join("runtime").join(EXPECTED), expected),
            "write only nonsecret own expectation",
        )?;
        checked(
            fs::write(target.root().join("runtime").join(MARKER), nonce.as_bytes()),
            "write own marker",
        )?;
        checked(
            fs::copy(&source_copy, target.root().join("quarantine").join(COPY)),
            "copy synthetic external source",
        )?;
        exercise_target(&mut app, &target, deadline, &mut steps)?;
        Ok(key_sha)
    })();
    let key_sha = match result {
        Ok(sha) => sha,
        Err(error) => {
            eprintln!("MIGRATION_NATIVE_FAILURE steps={steps} source_retained={} target_retained={} error={error}",source.root().display(),target.root().display());
            if !app.backup.irreversible() {
                let _ = ordinary_shutdown(&mut app, deadline);
            }
            return Err(error);
        }
    };
    join_app(app)?;
    drop(target_instance);
    drop(target_deployment);
    subprocess(target.root(), &nonce, deadline)?;
    mark(
        &mut steps,
        "independent-process-current-target-login-and-mapped-content",
    );
    require(
        blob_sha(target.root())? == key_sha
            && fc_security::sha256(&checked(fs::read(&source_copy), "unchanged source copy")?)
                == source_receipt.package_sha256,
        "source package and current target key unchanged",
    )?;
    mark(&mut steps, "source-package-and-target-key-unchanged");
    checked(
        fs::remove_dir_all(source.root()),
        "remove physically closed own source",
    )?;
    checked(
        fs::remove_dir_all(target.root()),
        "remove physically closed own target",
    )?;
    mark(&mut steps, "owned-synthetic-deployments-cleaned");
    require(steps == 16, "complete native proof sequence")?;
    println!("migration_native=PASS migration_native_steps={steps} reopen=independent_executable_production_Open picker=ISOLATED_CAPABILITY_ONLY visible_dialog=NOT_TESTED foreground=UNTOUCHED real_run=READ_ONLY isolated_cleanup=PASS elapsed_ms={}",started.elapsed().as_millis());
    Ok(())
}
