//! Real reset controls in independently launched copies of the current EXE.
//! Every file is synthetic and below one marked temporary test deployment.
use super::*;
use backup_probe::{join_app, open_native_backup, ordinary_shutdown, pump, submit};
use fc_bootstrap::recovery_workflow::RecoveryWorkflow;
use fc_protocol::recovery::PackageReader;
use fc_windows::materials::{
    chooser::SelectedFile,
    files::{LockedFile, PinnedDirectory},
};
use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

type Check<T> = std::result::Result<T, String>;
const MARKER: &str = "reinitialization-probe.marker";
const OLD_LOGIN: &str = "synthetic-former-administrator";
const NEW_LOGIN: &str = "synthetic-new-administrator";
const INITIAL_PASSWORD: &str = "Synthetic reset initial 2026!";
const NEW_PASSWORD: &str = "Synthetic reset replacement 2026!";
fn checked<T, E: std::fmt::Debug>(v: std::result::Result<T, E>, context: &str) -> Check<T> {
    v.map_err(|e| format!("reinitialization-probe: {context}: {e:?}"))
}
fn require(v: bool, context: &str) -> Check<()> {
    if v {
        Ok(())
    } else {
        Err(format!("reinitialization-probe: {context}"))
    }
}
fn manifest(path: &Path) -> Check<fc_domain::recovery::BackupManifest> {
    let reader = checked(
        PackageReader::new(checked(File::open(path), "open synthetic backup")?),
        "read backup manifest",
    )?;
    Ok(reader.manifest().clone())
}
fn synthetic_key_hash(root: &Path) -> Check<[u8; 32]> {
    let ciphertext = checked(
        fs::read(root.join("config/credential-key.dpapi")),
        "synthetic key blob",
    )?;
    require(ciphertext.len() <= 4096, "bounded synthetic key blob")?;
    let secret = checked(
        CurrentUserDpapi::default().unprotect(&ciphertext),
        "decrypt only owned synthetic key",
    )?;
    require(secret.expose().len() == 32, "synthetic codec key size")?;
    // Only a digest of this synthetic key leaves its zeroizing owner. Neither
    // plaintext nor digest is printed, archived or written to the deployment.
    Ok(fc_security::sha256(secret.expose()))
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
fn setup(app: &mut App, fresh: bool, deadline: Instant) -> Check<()> {
    require(
        app.screen == Screen::Setup,
        "fresh startup requires first administrator",
    )?;
    let (school, login, password) = if fresh {
        ("Synthetic new school", NEW_LOGIN, NEW_PASSWORD)
    } else {
        ("Synthetic former school", OLD_LOGIN, INITIAL_PASSWORD)
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
        "new credentials committed through native setup",
    )?;
    submit(app, ACK, &[], deadline)?;
    if app.screen == Screen::Login {
        submit(
            app,
            LOG_IN,
            &[(LOGIN, login), (PASSWORD, password)],
            deadline,
        )?;
    }
    require(
        app.screen == Screen::Home,
        "new administrator owns native session",
    )
}
fn export(app: &mut App, deadline: Instant) -> Check<PathBuf> {
    open_native_backup(app, deadline)?;
    submit(app, backup_editor::EXPORT, &[], deadline)?;
    app.backup
        .exported
        .as_ref()
        .map(|r| r.path.clone())
        .ok_or_else(|| "reinitialization-probe: backup export absent".into())
}
fn subprocess(root: &Path, nonce: &str, stage: &str, deadline: Instant) -> Check<()> {
    let mut child = checked(
        Command::new(checked(std::env::current_exe(), "current EXE")?)
            .arg("--reinitialization-probe-stage")
            .arg(root)
            .arg(nonce)
            .arg(stage)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x0800_0000)
            .spawn(),
        "launch owned independent child",
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
                eprintln!("owned child deadline/poll failure: {result:?}");
                break checked(child.wait(), "join owned child before preserving fixture")?;
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
        "independent reset child must exit successfully",
    )?;
    require(
        out.lines()
            .any(|line| line == format!("REINITIALIZATION_CHILD_PASS stage={stage} nonce={nonce}")),
        "exact child completion marker",
    )
}
pub(super) fn run() -> Check<()> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut random = [0u8; 16];
    checked(SystemRandom.fill(&mut random), "nonce")?;
    let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-Reinitialize-{}-{nonce}",
        std::process::id()
    ));
    let paths = checked(DataPaths::isolated_test(root.clone()), "isolated path")?;
    let deployment = checked(
        paths.prepare_for_current_user(),
        "isolated protected deployment",
    )?;
    checked(deployment.read_back(), "ACL readback")?;
    let instance = checked(InstanceGuard::acquire(&root), "owned instance")?;
    let mut app = new_app(&paths, deadline)?;
    setup(&mut app, false, deadline)?;
    let old_backup = export(&mut app, deadline)?;
    let old_manifest = manifest(&old_backup)?;
    // The external test copy deliberately lives beside the deployment. Only the
    // owned parent removes it after every child has physically exited.
    let external = root.with_extension("fcbak");
    checked(
        fs::copy(&old_backup, &external),
        "save synthetic backup outside deployment",
    )?;
    let external_hash = fc_security::sha256(&checked(fs::read(&external), "hash external backup")?);
    ordinary_shutdown(&mut app, deadline)?;
    join_app(app)?;
    let database_hash = fc_security::sha256(&checked(
        fs::read(paths.database()),
        "closed original database",
    )?);
    let key_hash = fc_security::sha256(&checked(
        fs::read(root.join("config/credential-key.dpapi")),
        "original synthetic DPAPI blob",
    )?);
    let original_codec_key_hash = synthetic_key_hash(&root)?;
    let mut marker = checked(
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(paths.runtime().join(MARKER)),
        "exclusive owned marker",
    )?;
    checked(marker.write_all(nonce.as_bytes()), "write owner marker")?;
    checked(marker.sync_all(), "flush owner marker")?;
    drop(marker);
    drop(instance);
    drop(deployment);
    subprocess(&root, &nonce, "cancel", deadline)?;
    require(
        fc_security::sha256(&checked(
            fs::read(paths.database()),
            "cancelled original database",
        )?) == database_hash,
        "cancel preserves exact closed database",
    )?;
    require(
        fc_security::sha256(&checked(
            fs::read(root.join("config/credential-key.dpapi")),
            "cancelled DPAPI blob",
        )?) == key_hash,
        "cancel preserves exact credentials",
    )?;
    println!("REINITIALIZATION_NATIVE_STEP cancel_preserves_database_and_key=PASS");
    subprocess(&root, &nonce, "commit", deadline)?;
    require(
        !paths.database().exists() && !root.join("config/credential-key.dpapi").exists(),
        "committed reset has no old database or key",
    )?;
    require(
        fc_security::sha256(&checked(
            fs::read(&external),
            "external saved copy after reset",
        )?) == external_hash,
        "external saved backup unchanged",
    )?;
    println!("REINITIALIZATION_NATIVE_STEP explicit_reset_deletes_local_database_and_key_external_copy_unchanged=PASS");
    subprocess(&root, &nonce, "reopen", deadline)?;
    require(synthetic_key_hash(&root)? != original_codec_key_hash, "fresh restart generates a different codec key, independently of DPAPI ciphertext randomness")?;
    require(
        fc_security::sha256(&checked(
            fs::read(root.join("config/credential-key.dpapi")),
            "fresh DPAPI blob",
        )?) != key_hash,
        "fresh restart creates a new protected key",
    )?;
    let new_manifest = manifest(&root.join("runtime/new-installation.fcbak"))?;
    require(
        new_manifest.installation_id != old_manifest.installation_id,
        "fresh administrator creates a distinct installation identity",
    )?;
    checked(
        fs::remove_dir_all(&root),
        "remove only closed owned test deployment",
    )?;
    checked(
        fs::remove_file(&external),
        "remove owned external saved copy",
    )?;
    println!("reinitialization_native=PASS child_processes=3 cancel_exact_preservation=PASS default_and_partial_confirmation_denied=PASS full_reset=PASS new_dpapi=PASS new_installation=PASS native_setup=PASS old_account_absent=PASS old_backup_direct_restore_denied=PASS external_saved_copy=UNCHANGED foreground=UNTOUCHED real_run=UNTOUCHED isolated_cleanup=PASS");
    Ok(())
}
pub(super) fn child(root: &Path, nonce: &str, mode: &str) -> Check<()> {
    require(
        root.is_absolute()
            && root.parent() == Some(std::env::temp_dir().as_path())
            && nonce.len() == 32
            && nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && matches!(mode, "cancel" | "commit" | "reopen"),
        "strict owned child scope",
    )?;
    let suffix = root
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("FutureClassroom-Test-Reinitialize-"))
        .ok_or("unowned child root")?;
    let (pid, bound_nonce) = suffix.split_once('-').ok_or("invalid child root")?;
    require(
        !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()) && bound_nonce == nonce,
        "root nonce binding",
    )?;
    let paths = checked(DataPaths::isolated_test(root.to_owned()), "child paths")?;
    checked(paths.verify_root_for_current_user(), "existing child ACL")?;
    let _pin = checked(PinnedDirectory::open(root), "pin owned root")?;
    let mut marker = checked(
        LockedFile::open(&paths.runtime().join(MARKER), 32),
        "pin owned marker",
    )?;
    let mut actual = [0u8; 32];
    checked(
        marker.file_mut().read_exact(&mut actual),
        "read owned marker",
    )?;
    require(
        actual.as_slice() == nonce.as_bytes(),
        "marker nonce matches",
    )?;
    // Keep the nonce marker and physical root pinned through every mode.
    let deadline = Instant::now() + Duration::from_secs(120);
    if mode == "reopen" {
        let instance = checked(InstanceGuard::acquire(root), "fresh child instance")?;
        let mut app = new_app(&paths, deadline)?;
        require(
            app.screen == Screen::Setup && app.state.accounts.is_empty(),
            "reset restarts into setup without old accounts",
        )?;
        setup(&mut app, true, deadline)?;
        require(
            app.state.account_by_login(OLD_LOGIN).is_err(),
            "former account was not adopted",
        )?;
        let fresh_export = export(&mut app, deadline)?;
        let external = root.with_extension("fcbak");
        let old = manifest(&external)?;
        let fresh_manifest = manifest(&fresh_export)?;
        require(
            fresh_manifest.installation_id != old.installation_id
                && fresh_manifest.school_id == old.school_id,
            "independent installation UUID changed even with the same numeric school ID",
        )?;
        checked(
            fs::copy(
                &fresh_export,
                paths.runtime().join("new-installation.fcbak"),
            ),
            "retain synthetic new installation evidence",
        )?;
        checked(
            fs::copy(&fresh_export, root.join("quarantine/current.fcbak")),
            "current-installation positive control",
        )?;
        app.preflight_selected_backup(checked(
            SelectedFile::isolated_probe(root, "current.fcbak"),
            "current backup capability",
        )?)?;
        pump(&mut app, deadline)?;
        require(
            app.backup.review.is_some(),
            "same current installation backup preflight succeeds",
        )?;
        submit(&mut app, backup_editor::CANCEL, &[], deadline)?;
        {
            let backend = checked(
                app.backend.as_ref().ok_or("fresh backend absent")?.lock(),
                "fresh backend lock",
            )?;
            let access = checked(
                backend.ordinary_backup_access(
                    app.lease.ok_or("fresh lease absent")?,
                    checked(backend.global_revision(), "fresh revision")?,
                ),
                "fresh ordinary access",
            )?;
            let encoded = checked(
                fc_protocol::recovery::encode_manifest(&old),
                "valid old manifest",
            )?;
            let result = access.preflight_restore(
                root.join("quarantine/unused-old.sqlite3"),
                root.join("quarantine/unused-preview.sqlite3"),
                old,
                fc_security::sha256(&encoded),
                Arc::new(|| true),
            );
            require(
                matches!(
                    result,
                    Err(BackendError::Storage(
                        fc_storage::StorageError::RecoveryConflict
                    ))
                ),
                "old install rejected synchronously by typed installation guard before any SQL preview",
            )?;
            require(
                !root.join("quarantine/unused-preview.sqlite3").exists(),
                "foreign installation cannot create a SQL preview",
            )?;
        }
        checked(
            fs::copy(&external, root.join("quarantine/old.fcbak")),
            "isolated old backup chooser fixture",
        )?;
        app.preflight_selected_backup(checked(
            SelectedFile::isolated_probe(root, "old.fcbak"),
            "old backup capability",
        )?)?;
        pump(&mut app, deadline)?;
        require(
            app.backup.review.is_none() && !app.backup.irreversible(),
            "old installation backup cannot directly restore into new installation",
        )?;
        ordinary_shutdown(&mut app, deadline)?;
        join_app(app)?;
        drop(instance);
    } else {
        let mut stage = 0u8;
        crate::reinitialize_application::run_at_with(paths, true, |h, _| {
            require(Instant::now() < deadline, "native reset deadline")?;
            if stage == 0 && h.ready() {
                require(
                    h.sequence() != 0,
                    "native review has a fresh nonzero sequence",
                )?;
                checked(
                    h.window().probe_submit(ActionId(700), FormData::new()),
                    "native prepare",
                )?;
                stage = 1;
            } else if stage == 1 && h.view().fields.len() == 2 {
                let mut unchecked = FormData::new();
                for f in h.view().fields {
                    checked(unchecked.insert(f.id, f.value), "default acknowledgement")?;
                }
                checked(
                    h.window().probe_submit(ActionId(701), unchecked),
                    "native unchecked confirmation",
                )?;
                stage = 2;
            } else if stage == 2 && h.view().fields.len() == 2 {
                require(!h.finished(), "unchecked cannot finish")?;
                let mut partial = FormData::new();
                for (i, f) in h.view().fields.into_iter().enumerate() {
                    let value = if i == 0 {
                        let FieldKind::Choice(c) = f.kind else {
                            return Err("reset acknowledgement not a choice".into());
                        };
                        c.into_iter()
                            .find(|c| c.value != "0")
                            .ok_or("choice absent")?
                            .value
                    } else {
                        f.value
                    };
                    checked(partial.insert(f.id, value), "partial acknowledgement")?;
                }
                checked(
                    h.window().probe_submit(ActionId(701), partial),
                    "native partial confirmation",
                )?;
                stage = 3;
            } else if stage == 3 && h.view().fields.len() == 2 {
                require(!h.finished(), "partial cannot finish")?;
                if mode == "cancel" {
                    checked(
                        h.window().probe_submit(ActionId(702), FormData::new()),
                        "native cancel",
                    )?;
                    stage = 4;
                } else {
                    let mut data = FormData::new();
                    for f in h.view().fields {
                        let FieldKind::Choice(c) = f.kind else {
                            return Err("reset acknowledgement not a choice".into());
                        };
                        checked(
                            data.insert(
                                f.id,
                                c.into_iter()
                                    .find(|c| c.value != "0")
                                    .ok_or("explicit choice absent")?
                                    .value,
                            ),
                            "full acknowledgement",
                        )?;
                    }
                    checked(
                        h.window().probe_submit(ActionId(701), data),
                        "native full confirmation",
                    )?;
                    stage = 4;
                }
            } else if stage == 4 && mode == "commit" && h.finished() {
                require(
                    h.succeeded(),
                    "native reset succeeded before physical close",
                )?;
                h.close();
                stage = 5;
            }
            Ok(())
        })?;
        require(
            stage == if mode == "cancel" { 4 } else { 5 },
            "native reset reached expected physical terminal",
        )?;
    }
    println!("REINITIALIZATION_CHILD_PASS stage={mode} nonce={nonce}");
    Ok(())
}
