//! Explicit finite ordinary-runtime composition against a protected synthetic
//! deployment. Actual TLS clients run in the separately owned external runner.
//! No school settings, trust store, firewall or HTTP.sys configuration is changed.
use super::*;
use https_settings_editor as h;
use voice_management_probe::{
    checked, login, make_app, open, pump, require, stop_owned_app, submit, LOGIN_NAME,
    PASSWORD_VALUE,
};
const ORDER: [&str; 31] = [
    "setup-local-global-administrator",
    "native-create-synthetic-class",
    "native-create-independent-teacher-and-complete-first-password-change",
    "native-teacher-page-default-off",
    "native-open-https-settings",
    "native-save-explicit-ip-cidr-reference",
    "saved-config-no-listener",
    "native-enable-configured-runtime",
    "physical-first-running-generation",
    "first-real-tls-response-complete",
    "native-open-actual-runtime-pairing-window",
    "native-approve-actual-network-pairing-request",
    "first-real-authenticated-reminder-post-and-get",
    "first-material-ready-and-partial-upload-established",
    "native-pairing-close-and-disable-review-cancel",
    "native-confirm-first-disable",
    "physical-first-domain-drained",
    "first-two-staging-files-removed",
    "original-shared-workers-remain-operational",
    "native-enable-second-generation",
    "second-real-tls-response-complete",
    "native-open-second-runtime-pairing-window",
    "native-approve-second-network-pairing-request",
    "second-old-cookie-rejected-and-two-upload-slots-reused",
    "second-typed-material-command-receipt-and-cancel",
    "native-lock-does-not-stop-established-domain",
    "native-relogin-confirm-second-disable",
    "physical-second-domain-drained",
    "native-return-to-local-workbench",
    "actual-shared-worker-writer-shutdown",
    "owned-deployment-cleanup",
];
const TEACHER_LOGIN: &str = "synthetic-ordinary-teacher";

type Check<T> = std::result::Result<T, String>;
fn mark(step: &mut usize, name: &str) -> Check<()> {
    require(
        ORDER.get(*step) == Some(&name),
        "ordinary HTTPS exact step order",
    )?;
    println!("ORDINARY_HTTPS_NATIVE_STEP index={} name={name}", *step);
    *step += 1;
    Ok(())
}
fn wait(app: &mut App, deadline: Instant, condition: impl Fn(&App) -> bool) -> Check<()> {
    loop {
        pump(app, deadline)?;
        require(
            app.web_readiness != web_editor::Readiness::Faulted,
            "ordinary HTTPS runtime not faulted",
        )?;
        if condition(app) {
            return Ok(());
        }
        require(
            Instant::now() < deadline,
            "ordinary HTTPS original finite deadline",
        )?;
        std::thread::park_timeout(Duration::from_millis(2));
    }
}
fn running(app: &App) -> Option<u64> {
    match app.web_readiness {
        web_editor::Readiness::Running { generation } if generation != 0 => Some(generation),
        _ => None,
    }
}
fn response(app: &App) -> bool {
    app.teacher_https
        .owner
        .as_ref()
        .and_then(|owner| owner.network_report())
        .is_some_and(|r| r.heads >= 1 && r.responses >= 1)
}
fn ready(app: &App, cycle: u8, port: u16, marker: &str) -> Check<()> {
    let generation = running(app).ok_or("ordinary HTTPS actual running generation absent")?;
    println!("ORDINARY_HTTPS_READY {{\"cycle\":{cycle},\"marker\":\"{marker}\",\"port\":{port},\"generation\":{generation},\"origin\":\"https://127.0.0.1:{port}\",\"target\":\"/fc/api/v1/status\"}}");
    checked(std::io::stdout().flush(), "ordinary public READY flush")?;
    Ok(())
}
fn confirm_disable(app: &mut App, cycle: u8, marker: &str, deadline: Instant) -> Check<()> {
    let generation = running(app).ok_or("stop requires original running generation")?;
    submit(app, web_editor::DISABLE, &[], deadline)?;
    let began = Instant::now();
    let stop_deadline = deadline.min(began + Duration::from_secs(5));
    submit(app, web_editor::CONFIRM, &[], stop_deadline)?;
    wait(app, stop_deadline, |a| {
        a.teacher_https_quiescent() && a.web_readiness == web_editor::Readiness::Disabled
    })?;
    let elapsed = began.elapsed();
    require(
        elapsed <= Duration::from_secs(5),
        "one original stop interval",
    )?;
    let report = app
        .teacher_https
        .owner
        .as_ref()
        .and_then(|o| o.network_report())
        .ok_or("original stopped network report absent")?;
    require(
        report.native_drained
            && report.active_records == 0
            && report.pending_jobs == 0
            && report.cleanup_pending == 0
            && report.retained_cleanup == 0,
        "original Net/native ownership actually drained",
    )?;
    println!("ORDINARY_HTTPS_STOPPED {{\"cycle\":{cycle},\"marker\":\"{marker}\",\"generation\":{generation},\"elapsedNs\":{},\"activeRecords\":0,\"pendingJobs\":0,\"cleanupPending\":0,\"retainedCleanup\":0,\"nativeDrained\":true}}", elapsed.as_nanos());
    checked(std::io::stdout().flush(), "stop observation flush")
}
fn stage_path(app: &App, id: u64) -> PathBuf {
    app.root
        .join("web-material-staging")
        .join(format!("web-material-{id}.part"))
}
fn stage_length(app: &App, id: u64) -> Check<u64> {
    use std::os::windows::fs::MetadataExt;
    let metadata = fs::symlink_metadata(stage_path(app, id))
        .map_err(|_| "exact owned material stage metadata unavailable")?;
    require(
        metadata.file_type().is_file() && metadata.file_attributes() & 0x400 == 0,
        "owned stage must be regular and non-reparse",
    )?;
    Ok(metadata.len())
}
fn absent_stage(app: &App, id: u64) -> Check<()> {
    require(
        matches!(fs::symlink_metadata(stage_path(app, id)), Err(e)
        if e.kind() == std::io::ErrorKind::NotFound),
        "exact old material stage absent",
    )
}
fn completion(
    app: &mut App,
    root: &Path,
    marker: &str,
    cycle: u8,
    generation: u64,
    deadline: Instant,
) -> Check<[u64; 2]> {
    let path = root.join(format!("ordinary-business-{cycle}.done"));
    loop {
        pump(app, deadline)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                require(
                    metadata.file_type().is_file() && metadata.len() <= 256,
                    "bounded regular completion marker",
                )?;
                let bytes = fs::read(&path).map_err(|_| "owned completion read failed")?;
                let text = std::str::from_utf8(&bytes).map_err(|_| "completion encoding")?;
                let fields: Vec<_> = text.split('\n').collect();
                require(
                    fields.len() == 7
                        && fields[0] == "FC_ORDINARY_DONE_V2"
                        && fields[1] == marker
                        && fields[2] == format!("{generation:020}")
                        && fields[3] == cycle.to_string()
                        && fields[6].is_empty(),
                    "exact completion marker/cycle/generation",
                )?;
                let mut ids = [0; 2];
                for (index, input) in fields[4..6].iter().enumerate() {
                    ids[index] = input.parse().map_err(|_| "completion upload ID")?;
                    require(
                        ids[index] > 0 && *input == format!("{:020}", ids[index]),
                        "canonical bounded completion ID",
                    )?;
                }
                require(
                    ids[0] != ids[1] && Instant::now() < deadline,
                    "distinct upload IDs before original deadline",
                )?;
                return Ok(ids);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("owned completion metadata failed".into()),
        }
        require(
            Instant::now() < deadline,
            "original authenticated transfer deadline",
        )?;
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
fn pair(
    app: &mut App,
    root: &Path,
    marker: &str,
    cycle_generation: (u8, u64),
    port: u16,
    steps: &mut usize,
    deadline: Instant,
) -> Check<()> {
    let (cycle, generation) = cycle_generation;
    submit(app, web_editor::REFRESH, &[], deadline)?;
    submit(app, web_editor::BEGIN, &[], deadline)?;
    let code = app
        .web_editor
        .as_ref()
        .and_then(web_editor::Editor::code_text)
        .ok_or("actual ordinary pairing code absent")?;
    {
        use std::io::Write;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(format!("ordinary-pairing-{cycle}.secret")))
            .map_err(|_| "fresh owned pairing sidechannel unavailable")?;
        write!(
            output,
            "FC_ORDINARY_PAIR_V2\n{marker}\n{generation:020}\n{cycle}\n{}\n{TEACHER_LOGIN}\n",
            code.as_str()
        )
        .map_err(|_| "owned pairing sidechannel write failed")?;
    }
    drop(code);
    println!("ORDINARY_HTTPS_PAIR_READY {{\"cycle\":{cycle},\"marker\":\"{marker}\",\"port\":{port},\"generation\":{generation},\"origin\":\"https://127.0.0.1:{port}\"}}");
    checked(std::io::stdout().flush(), "pairing rendezvous flush")?;
    mark(
        steps,
        if cycle == 1 {
            "native-open-actual-runtime-pairing-window"
        } else {
            "native-open-second-runtime-pairing-window"
        },
    )?;
    let request = loop {
        require(
            Instant::now() < deadline,
            "original network pairing deadline",
        )?;
        submit(app, web_editor::REFRESH, &[], deadline)?;
        let choices = app
            .view()
            .fields
            .into_iter()
            .find_map(|field| {
                if field.id != web_editor::REQUEST {
                    return None;
                }
                match field.kind {
                    FieldKind::Choice(choices) => Some(choices),
                    _ => None,
                }
            })
            .ok_or("actual native pairing list absent")?;
        require(choices.len() <= 1, "only one synthetic pairing request")?;
        if let Some(choice) = choices.first() {
            break choice.value.clone();
        }
        std::thread::park_timeout(Duration::from_millis(100));
    };
    checked(
        app.window.probe_choice(web_editor::REQUEST, &request),
        "native request selection",
    )?;
    pump(app, deadline)?;
    require(
        app.view()
            .actions
            .iter()
            .any(|a| a.id == web_editor::APPROVE && a.enabled),
        "actual selected request enables approval",
    )?;
    submit(
        app,
        web_editor::APPROVE,
        &[(web_editor::REQUEST, &request)],
        deadline,
    )?;
    mark(
        steps,
        if cycle == 1 {
            "native-approve-actual-network-pairing-request"
        } else {
            "native-approve-second-network-pairing-request"
        },
    )
}
fn fixture_password() -> Check<fc_security::web::SecretText> {
    let mut bytes = [0u8; 32];
    checked(
        SystemRandom.fill(&mut bytes),
        "private teacher fixture randomness",
    )?;
    let text = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    bytes.fill(0);
    Ok(fc_security::web::SecretText::new(text))
}
fn create_teacher(app: &mut App, deadline: Instant) -> Check<()> {
    use accounts_editor as e;
    // Real native form commands go through the original shared workers, normal
    // credential KDFs and identity transactions; no SQLite/session fabrication.
    submit(app, TO_SUBJECTS, &[], deadline)?;
    submit(
        app,
        SAVE,
        &[
            (OLD_NAME, ""),
            (SUBJECT, "Synthetic HTTPS subject"),
            (COLOR, "123456"),
        ],
        deadline,
    )?;
    require(
        app.screen == Screen::Home,
        "subject save returned to actual home page",
    )?;
    submit(app, TO_ACCOUNTS, &[], deadline)?;
    let initial = fixture_password()?;
    let changed = fixture_password()?;
    submit(app, e::NEW, &[], deadline)?;
    submit(
        app,
        e::SCOPES,
        &[
            (e::LOGIN, TEACHER_LOGIN),
            (e::NAME, "Synthetic HTTPS teacher"),
            (e::ROLE, "teacher"),
        ],
        deadline,
    )?;
    submit(
        app,
        e::ADD_SCOPE,
        &[(e::CLASS, "1"), (e::SUBJECT, "1")],
        deadline,
    )?;
    submit(app, e::RETURN, &[], deadline)?;
    submit(app, e::REVIEW, &[], deadline)?;
    submit(app, e::CONFIRM, &[], deadline)?;
    submit(
        app,
        e::CONFIRM,
        &[
            (e::PASSWORD, initial.as_str()),
            (e::CONFIRM_PASSWORD, initial.as_str()),
        ],
        deadline,
    )?;
    require(
        app.screen == Screen::Code,
        "teacher created with real one-time recovery page",
    )?;
    submit(app, ACK, &[], deadline)?;
    submit(app, LOCK, &[], deadline)?;
    submit(
        app,
        LOG_IN,
        &[(LOGIN, TEACHER_LOGIN), (PASSWORD, initial.as_str())],
        deadline,
    )?;
    require(
        app.screen == Screen::Password,
        "teacher first-login password change required",
    )?;
    submit(
        app,
        SAVE,
        &[
            (PASSWORD, initial.as_str()),
            (NEW_PASSWORD, changed.as_str()),
            (CONFIRM, changed.as_str()),
        ],
        deadline,
    )?;
    drop(initial);
    require(
        app.screen == Screen::Login,
        "real password change invalidates prior teacher lease",
    )?;
    submit(
        app,
        LOG_IN,
        &[(LOGIN, TEACHER_LOGIN), (PASSWORD, changed.as_str())],
        deadline,
    )?;
    drop(changed);
    let teacher = app
        .state
        .account_by_login(TEACHER_LOGIN)
        .map_err(|_| "teacher absent")?;
    require(
        app.screen == Screen::Home
            && teacher.principal.enabled
            && teacher.principal.role == Role::TeacherAdmin
            && !teacher.must_change_password
            && app
                .state
                .assignments
                .iter()
                .filter(|scope| {
                    scope.teacher_id == teacher.principal.id
                        && scope.enabled
                        && scope.class_id.get() == 1
                        && scope.subject_id.get() == 1
                })
                .count()
                == 1,
        "independent enabled teacher and exact class/subject scope",
    )?;
    submit(app, LOCK, &[], deadline)?;
    login(app, deadline)
}
fn workflow(
    app: &mut App,
    paths: &DataPaths,
    reference: &str,
    port: u16,
    marker: &str,
    steps: &mut usize,
    deadline: Instant,
) -> Check<()> {
    let completion_path = app
        .teacher_https
        .probe_package
        .as_ref()
        .and_then(|p| p.parent())
        .ok_or("owned probe package parent absent")?
        .join("ordinary-business-1.done");
    open(app, paths, deadline)?;
    submit(
        app,
        SAVE,
        &[
            (SCHOOL, "Synthetic ordinary HTTPS school"),
            (LOGIN, LOGIN_NAME),
            (NAME, "Synthetic administrator"),
            (PASSWORD, PASSWORD_VALUE),
            (CONFIRM, PASSWORD_VALUE),
        ],
        deadline,
    )?;
    submit(app, ACK, &[], deadline)?;
    if app.screen == Screen::Login {
        login(app, deadline)?;
    }
    require(app.screen == Screen::Home, "ordinary HTTPS setup completed")?;
    mark(steps, "setup-local-global-administrator")?;
    submit(app, TO_SCHOOL, &[], deadline)?;
    submit(
        app,
        ADD_CLASS,
        &[(CLASS, "Synthetic ordinary HTTPS class")],
        deadline,
    )?;
    require(
        app.state.classes.len() == 1,
        "exact synthetic class created through native form",
    )?;
    require(
        app.screen == Screen::Home,
        "class save returned to actual home page",
    )?;
    submit(app, TO_SCHOOL, &[], deadline)?;
    submit(
        app,
        BIND_CLASS,
        &[(CLASS, "Synthetic ordinary HTTPS class")],
        deadline,
    )?;
    require(
        app.screen == Screen::Home,
        "class binding returned to actual home page",
    )?;
    mark(steps, "native-create-synthetic-class")?;
    create_teacher(app, deadline)?;
    mark(
        steps,
        "native-create-independent-teacher-and-complete-first-password-change",
    )?;
    // Hidden fixtures start Unavailable; Disabled grants no runtime readiness.
    app.set_web_readiness(web_editor::Readiness::Disabled);
    submit(app, TO_WEB, &[], deadline)?;
    require(
        app.teacher_https_quiescent() && app.web_readiness == web_editor::Readiness::Disabled,
        "ordinary HTTPS default off",
    )?;
    mark(steps, "native-teacher-page-default-off")?;
    submit(app, web_editor::HTTPS_SETTINGS, &[], deadline)?;
    mark(steps, "native-open-https-settings")?;
    let port_text = port.to_string();
    submit(
        app,
        h::SAVE_CONFIG,
        &[
            (h::HOST, "127.0.0.1"),
            (h::PORT, &port_text),
            (h::CERTIFICATE, reference),
            (h::SOURCE_NETWORKS, "127.0.0.1/32"),
        ],
        deadline,
    )?;
    while app.https_unadmitted.is_some() {
        require(
            Instant::now() < deadline,
            "original config admission deadline",
        )?;
        app.retry_https_unadmitted()?;
        pump(app, deadline)?;
    }
    require(
        !app.https_editor
            .as_ref()
            .is_none_or(|e| e.dirty() || e.has_unknown()),
        "ordinary HTTPS config saved",
    )?;
    mark(steps, "native-save-explicit-ip-cidr-reference")?;
    require(
        app.teacher_https_quiescent() && app.web_readiness == web_editor::Readiness::Disabled,
        "save never starts listener",
    )?;
    mark(steps, "saved-config-no-listener")?;
    submit(app, h::CLOSE, &[], deadline)?;
    submit(app, web_editor::ENABLE, &[], deadline)?;
    mark(steps, "native-enable-configured-runtime")?;
    wait(app, deadline, |a| running(a).is_some())?;
    let first = running(app).unwrap();
    mark(steps, "physical-first-running-generation")?;
    ready(app, 1, port, marker)?;
    wait(app, deadline, response)?;
    mark(steps, "first-real-tls-response-complete")?;
    let package_root = completion_path
        .parent()
        .ok_or("owned completion parent absent")?;
    pair(app, package_root, marker, (1, first), port, steps, deadline)?;
    let first_ids = completion(app, package_root, marker, 1, first, deadline)?;
    require(
        stage_length(app, first_ids[0])? == 65536,
        "actual completed MaterialReady stage retained before stop",
    )?;
    let partial = stage_length(app, first_ids[1])?;
    require(
        partial > 0 && partial < 1048576,
        "actual partial body consumed before native stop",
    )?;
    wait(app, deadline, |a| {
        a.teacher_https
            .owner
            .as_ref()
            .and_then(|o| o.network_report())
            .is_some_and(|r| r.heads >= 9 && r.responses >= 9)
    })?;
    mark(steps, "first-real-authenticated-reminder-post-and-get")?;
    mark(steps, "first-material-ready-and-partial-upload-established")?;
    submit(app, web_editor::CLOSE, &[], deadline)?;
    submit(app, web_editor::DISABLE, &[], deadline)?;
    submit(app, web_editor::CANCEL, &[], deadline)?;
    require(
        running(app) == Some(first),
        "cancel disable leaves original runtime",
    )?;
    mark(steps, "native-pairing-close-and-disable-review-cancel")?;
    confirm_disable(app, 1, marker, deadline)?;
    mark(steps, "native-confirm-first-disable")?;

    mark(steps, "physical-first-domain-drained")?;
    for id in first_ids {
        absent_stage(app, id)?;
    }
    mark(steps, "first-two-staging-files-removed")?;
    require(
        !app.pool.is_finished() && !app.pool.is_faulted(),
        "ordinary disable retains original shared workers",
    )?;
    app.submit(Work::OpenWeb(
        app.backend.as_ref().ok_or("backend absent")?.clone(),
        app.lease.ok_or("local manager absent")?,
    ))?;
    pump(app, deadline)?;
    require(
        !app.pool.is_finished() && app.backend.is_some(),
        "shared worker real query after disable",
    )?;
    mark(steps, "original-shared-workers-remain-operational")?;
    // The original worker returns a fresh exact-manager snapshot; no replacement
    // pool or copied manager identity is used to make the second start succeed.
    submit(app, web_editor::REFRESH, &[], deadline)?;
    submit(app, web_editor::ENABLE, &[], deadline)?;
    wait(app, deadline, |a| running(a).is_some())?;
    require(
        running(app).is_some_and(|g| g != first),
        "new runtime generation differs",
    )?;
    mark(steps, "native-enable-second-generation")?;
    ready(app, 2, port, marker)?;
    wait(app, deadline, response)?;
    mark(steps, "second-real-tls-response-complete")?;
    let second = running(app).unwrap();
    pair(
        app,
        package_root,
        marker,
        (2, second),
        port,
        steps,
        deadline,
    )?;
    let second_ids = completion(app, package_root, marker, 2, second, deadline)?;
    for id in second_ids {
        absent_stage(app, id)?;
    }
    require(
        second_ids.iter().all(|id| !first_ids.contains(id)),
        "new material IDs do not alias old generation",
    )?;
    mark(
        steps,
        "second-old-cookie-rejected-and-two-upload-slots-reused",
    )?;
    mark(steps, "second-typed-material-command-receipt-and-cancel")?;
    submit(app, web_editor::CLOSE, &[], deadline)?;
    submit(app, LOCK, &[], deadline)?;
    require(
        app.lease.is_none() && running(app) == Some(second),
        "local lock independent established Web service",
    )?;
    mark(steps, "native-lock-does-not-stop-established-domain")?;
    login(app, deadline)?;
    submit(app, TO_WEB, &[], deadline)?;
    submit(app, web_editor::REFRESH, &[], deadline)?;
    confirm_disable(app, 2, marker, deadline)?;
    mark(steps, "native-relogin-confirm-second-disable")?;

    mark(steps, "physical-second-domain-drained")?;
    submit(app, BACK, &[], deadline)?;
    require(
        app.screen == Screen::Home,
        "local workbench after Web disable",
    )?;
    mark(steps, "native-return-to-local-workbench")
}
pub(super) fn run(reference: &str, port: u16, package: &Path, marker: &str) -> Check<()> {
    require(port >= 1024, "ordinary HTTPS diagnostic high port")?;
    let reference = fc_domain::https_settings::HttpsCertificateRef::parse(reference)
        .map_err(|_| "expected public40hex reference")?
        .to_hex();
    require(
        marker
            .strip_prefix("FutureClassroom-Test-TLS-")
            .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())),
        "owned probe marker",
    )?;
    require(
        package.is_absolute()
            && package.file_name().is_some_and(|s| s == "web-package")
            && package
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|s| s == marker),
        "owned deployment package path",
    )?;
    let completion = package
        .parent()
        .ok_or("owned package parent absent")?
        .join("ordinary-business-1.done");
    require(!completion.exists(), "fresh business completion channel")?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(120);
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-OrdinaryHttps-{}-{marker}",
        std::process::id()
    ));
    let paths = checked(
        DataPaths::isolated_test(root.clone()),
        "ordinary isolated paths",
    )?;
    let deployment = checked(paths.prepare_for_current_user(), "ordinary owned ACL root")?;
    checked(deployment.read_back(), "ordinary ACL readback")?;
    let instance = checked(InstanceGuard::acquire(paths.root()), "ordinary instance")?;
    let mut app = make_app(&paths, deadline)?;
    app.teacher_https.probe_package = Some(package.to_owned());
    let mut steps = 0;
    let result = workflow(
        &mut app, &paths, &reference, port, marker, &mut steps, deadline,
    );
    // Failed workflow still requests the exact ordinary stop and waits for
    // native/Hub/thread and original shared-pool retirement before deleting data.
    stop_owned_app(app, deadline)?;
    if result.is_ok() {
        mark(&mut steps, "actual-shared-worker-writer-shutdown")?;
    }
    drop(instance);
    drop(deployment);
    checked(fs::remove_dir_all(&root), "ordinary closed fixture removal")?;
    require(!root.exists(), "ordinary fixture root absent")?;
    result?;
    mark(&mut steps, "owned-deployment-cleanup")?;
    println!("ORDINARY_HTTPS_NATIVE_PASS steps={steps} shared_workers=2 tls_cycles=2 authenticated_reminder=PASS physical_shutdown=PASS isolated_cleanup=PASS scope=OWNED_LOOPBACK_COMPOSITION material_ready_stop=PASS partial_upload_stop=PASS upload_slots_reused=2 typed_material_receipt=PASS campus=NOT_VALIDATED elapsed_ms={}",started.elapsed().as_millis());
    Ok(())
}
