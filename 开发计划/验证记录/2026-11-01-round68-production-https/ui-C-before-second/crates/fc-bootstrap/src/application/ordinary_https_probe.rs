//! Explicit finite ordinary-runtime composition against a protected synthetic
//! deployment. Actual TLS clients run in the separately owned external runner.
//! No school settings, trust store, firewall or HTTP.sys configuration is changed.
use super::*;
use https_settings_editor as h;
use voice_management_probe::{
    checked, login, make_app, open, pump, require, stop_owned_app, submit, LOGIN_NAME,
    PASSWORD_VALUE,
};
const ORDER: [&str; 20] = [
    "setup-local-global-administrator",
    "native-teacher-page-default-off",
    "native-open-https-settings",
    "native-save-explicit-ip-cidr-reference",
    "saved-config-no-listener",
    "native-enable-configured-runtime",
    "physical-first-running-generation",
    "first-real-tls-response-complete",
    "native-pairing-open-close-and-disable-review-cancel",
    "native-confirm-first-disable",
    "physical-first-domain-drained",
    "original-shared-workers-remain-operational",
    "native-enable-second-generation",
    "second-real-tls-response-complete",
    "native-lock-does-not-stop-established-domain",
    "native-relogin-confirm-second-disable",
    "physical-second-domain-drained",
    "native-return-to-local-workbench",
    "actual-shared-worker-writer-shutdown",
    "owned-deployment-cleanup",
];
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
    Ok(())
}
fn confirm_disable(app: &mut App, deadline: Instant) -> Check<()> {
    submit(app, web_editor::DISABLE, &[], deadline)?;
    submit(app, web_editor::CONFIRM, &[], deadline)
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
    mark(steps, ORDER[0])?;
    app.set_web_readiness(web_editor::Readiness::Disabled);
    submit(app, TO_WEB, &[], deadline)?;
    require(
        app.teacher_https_quiescent() && app.web_readiness == web_editor::Readiness::Disabled,
        "ordinary HTTPS default off",
    )?;
    mark(steps, ORDER[1])?;
    submit(app, web_editor::HTTPS_SETTINGS, &[], deadline)?;
    mark(steps, ORDER[2])?;
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
    mark(steps, ORDER[3])?;
    require(
        app.teacher_https_quiescent() && app.web_readiness == web_editor::Readiness::Disabled,
        "save never starts listener",
    )?;
    mark(steps, ORDER[4])?;
    submit(app, h::CLOSE, &[], deadline)?;
    submit(app, web_editor::ENABLE, &[], deadline)?;
    mark(steps, ORDER[5])?;
    wait(app, deadline, |a| running(a).is_some())?;
    let first = running(app).unwrap();
    mark(steps, ORDER[6])?;
    ready(app, 1, port, marker)?;
    wait(app, deadline, response)?;
    mark(steps, ORDER[7])?;
    // Refresh coherent logical metadata before exposing a pairing button.
    submit(app, web_editor::REFRESH, &[], deadline)?;
    submit(app, web_editor::BEGIN, &[], deadline)?;
    submit(app, web_editor::CLOSE, &[], deadline)?;
    submit(app, web_editor::DISABLE, &[], deadline)?;
    submit(app, web_editor::CANCEL, &[], deadline)?;
    require(
        running(app) == Some(first),
        "cancel disable leaves original runtime",
    )?;
    mark(steps, ORDER[8])?;
    confirm_disable(app, deadline)?;
    mark(steps, ORDER[9])?;
    wait(app, deadline, |a| {
        a.teacher_https_quiescent() && a.web_readiness == web_editor::Readiness::Disabled
    })?;
    mark(steps, ORDER[10])?;
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
    mark(steps, ORDER[11])?;
    // The original worker returns a fresh exact-manager snapshot; no replacement
    // pool or copied manager identity is used to make the second start succeed.
    submit(app, web_editor::REFRESH, &[], deadline)?;
    submit(app, web_editor::ENABLE, &[], deadline)?;
    wait(app, deadline, |a| running(a).is_some())?;
    require(
        running(app).is_some_and(|g| g != first),
        "new runtime generation differs",
    )?;
    mark(steps, ORDER[12])?;
    ready(app, 2, port, marker)?;
    wait(app, deadline, response)?;
    mark(steps, ORDER[13])?;
    let second = running(app).unwrap();
    submit(app, LOCK, &[], deadline)?;
    require(
        app.lease.is_none() && running(app) == Some(second),
        "local lock independent established Web service",
    )?;
    mark(steps, ORDER[14])?;
    login(app, deadline)?;
    submit(app, TO_WEB, &[], deadline)?;
    submit(app, web_editor::REFRESH, &[], deadline)?;
    confirm_disable(app, deadline)?;
    mark(steps, ORDER[15])?;
    wait(app, deadline, |a| {
        a.teacher_https_quiescent() && a.web_readiness == web_editor::Readiness::Disabled
    })?;
    mark(steps, ORDER[16])?;
    submit(app, BACK, &[], deadline)?;
    require(
        app.screen == Screen::Home,
        "local workbench after Web disable",
    )?;
    mark(steps, ORDER[17])
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
        mark(&mut steps, ORDER[18])?;
    }
    drop(instance);
    drop(deployment);
    checked(fs::remove_dir_all(&root), "ordinary closed fixture removal")?;
    require(!root.exists(), "ordinary fixture root absent")?;
    result?;
    mark(&mut steps, ORDER[19])?;
    println!("ORDINARY_HTTPS_NATIVE_PASS steps={steps} shared_workers=2 tls_cycles=2 physical_shutdown=PASS isolated_cleanup=PASS scope=OWNED_LOOPBACK_COMPOSITION material_ready_stop=NOT_EXERCISED campus=NOT_VALIDATED elapsed_ms={}",started.elapsed().as_millis());
    Ok(())
}
