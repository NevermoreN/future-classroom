//! Explicit hidden native controls against a protected synthetic deployment.
//! Public certificate reference only; no listener, trust install or playback.
use super::*;
use fc_ports::HttpsSettingsState;
use fc_windows::tls_frontdoor::certificate_query::{CertificateCheck, CertificateFact};
use https_settings_editor as h;
use voice_management_probe::{
    checked, login, make_app, open, pump, require, stop_owned_app, submit as submit_native,
    LOGIN_NAME, PASSWORD_VALUE,
};
type Check<T> = std::result::Result<T, String>;
const ORDER: [&str; 22] = [
    "setup-local-global-administrator",
    "native-device-page-production-unavailable",
    "native-open-https-config",
    "unconfigured-no-default-port",
    "invalid-port-rejected-no-write",
    "cancel-draft-confirmed-no-write",
    "native-save-localhost-reference",
    "saved-config-no-listener",
    "explicit-native-certificate-check",
    "native-draft-retained-across-lock",
    "native-relogin-preserves-draft",
    "native-original-save-admitted",
    "real-commit-delivery-cancelled",
    "original-request-retained-after-cancel",
    "lock-and-relogin-original-owner",
    "native-explicit-original-outcome-query",
    "historical-receipt-not-current-cas",
    "explicit-risk-confirmed-current-reload",
    "current-config-readback",
    "device-page-remains-unavailable",
    "actual-shared-worker-writer-shutdown",
    "owned-deployment-cleanup",
];
fn submit(
    app: &mut App,
    action: ActionId,
    overrides: &[(FieldId, &str)],
    deadline: Instant,
) -> Check<()> {
    submit_native(app, action, overrides, deadline)?;
    drain_unadmitted(app, deadline)
}
fn drain_unadmitted(app: &mut App, deadline: Instant) -> Check<()> {
    // The only retryable object here is the SAME returned, never-run Busy Work.
    // Its original absolute deadline is retained by the controller. No accepted
    // save, query, native inspection or SQL operation is replayed by this loop.
    while app.https_unadmitted.is_some() {
        require(
            Instant::now() < deadline,
            "HTTPS finite unadmitted Busy wait",
        )?;
        std::thread::park_timeout(Duration::from_millis(1));
        app.retry_https_unadmitted()?;
        pump(app, deadline)?;
    }
    Ok(())
}
fn mark(step: &mut usize, name: &str) -> Check<()> {
    require(ORDER.get(*step) == Some(&name), "HTTPS exact step order")?;
    println!("HTTPS_SETTINGS_NATIVE_STEP index={} name={name}", *step);
    *step += 1;
    Ok(())
}
fn saved(app: &App) -> Check<HttpsSettingsState> {
    checked(
        app.backend
            .as_ref()
            .ok_or("HTTPS backend absent")?
            .lock()
            .map_err(|_| "HTTPS backend poisoned")?
            .read_https_settings(app.lease.ok_or("HTTPS lease absent")?),
        "HTTPS read-only persisted observer",
    )
}
fn field(app: &App, id: FieldId) -> Option<String> {
    app.view()
        .fields
        .into_iter()
        .find(|f| f.id == id)
        .map(|f| f.value)
}
fn settings(app: &mut App, deadline: Instant) -> Check<()> {
    submit(app, TO_WEB, &[], deadline)?;
    submit(app, web_editor::HTTPS_SETTINGS, &[], deadline)?;
    require(app.screen == Screen::HttpsSettings, "HTTPS native page")
}
// Wire strings are a closed set of constants. Only integers/null are formatted
// from observations; no configuration, provider text or user input is encoded.
fn optional_number(value: Option<i64>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}
fn fact(value: CertificateFact) -> String {
    let check = match value.check {
        CertificateCheck::Satisfied => "Satisfied",
        CertificateCheck::Rejected => "Rejected",
        CertificateCheck::Unverified => "Unverified",
        CertificateCheck::NotChecked => "NotChecked",
    };
    format!(
        "{{\"check\":\"{check}\",\"nativeError\":{}}}",
        optional_number(value.native_error.map(i64::from))
    )
}
fn workflow(
    app: &mut App,
    paths: &DataPaths,
    reference: &str,
    owned: bool,
    step: &mut usize,
    deadline: Instant,
) -> Check<()> {
    open(app, paths, deadline)?;
    submit(
        app,
        SAVE,
        &[
            (SCHOOL, "Synthetic HTTPS settings school"),
            (LOGIN, LOGIN_NAME),
            (NAME, "Synthetic HTTPS administrator"),
            (PASSWORD, PASSWORD_VALUE),
            (CONFIRM, PASSWORD_VALUE),
        ],
        deadline,
    )?;
    require(app.screen == Screen::Code, "HTTPS native setup")?;
    submit(app, ACK, &[], deadline)?;
    if app.screen == Screen::Login {
        login(app, deadline)?;
    }
    mark(step, "setup-local-global-administrator")?;
    submit(app, TO_WEB, &[], deadline)?;
    require(
        app.view().description.contains(web_editor::NOT_READY)
            && app
                .view()
                .actions
                .iter()
                .filter(|a| [web_editor::ENABLE, web_editor::BEGIN].contains(&a.id))
                .all(|a| !a.enabled),
        "production Web remains unavailable",
    )?;
    mark(step, "native-device-page-production-unavailable")?;
    submit(app, web_editor::HTTPS_SETTINGS, &[], deadline)?;
    mark(step, "native-open-https-config")?;
    let initial = saved(app)?;
    require(
        initial.settings.configuration.is_none() && field(app, h::PORT).as_deref() == Some(""),
        "no invented default port",
    )?;
    mark(step, "unconfigured-no-default-port")?;
    submit(
        app,
        h::SAVE_CONFIG,
        &[
            (h::HOST, "localhost"),
            (h::PORT, "0"),
            (h::CERTIFICATE, reference),
            (h::SOURCE_NETWORKS, "127.0.0.1/32"),
        ],
        deadline,
    )?;
    require(saved(app)? == initial, "invalid port did not write")?;
    mark(step, "invalid-port-rejected-no-write")?;
    submit(app, h::CANCEL_DRAFT, &[], deadline)?;
    submit(app, h::CONFIRM_DISCARD, &[], deadline)?;
    require(
        saved(app)? == initial && field(app, h::PORT).as_deref() == Some(""),
        "cancel discarded only unsaved draft",
    )?;
    mark(step, "cancel-draft-confirmed-no-write")?;
    submit(
        app,
        h::SAVE_CONFIG,
        &[
            (h::HOST, "localhost"),
            (h::PORT, "8443"),
            (h::CERTIFICATE, reference),
            (h::SOURCE_NETWORKS, "127.0.0.1/32"),
        ],
        deadline,
    )?;
    mark(step, "native-save-localhost-reference")?;
    let first = saved(app)?;
    require(
        first.settings.configuration.as_ref().is_some_and(|c| {
            c.host.as_str() == "localhost" && c.port == 8443 && c.certificate.to_hex() == reference
        }) && app.status == h::SAVED,
        "configuration stored only",
    )?;
    require(
        matches!(app.web_readiness, web_editor::Readiness::Unavailable)
            && app.web_service.is_none(),
        "no listener or logical readiness",
    )?;
    mark(step, "saved-config-no-listener")?;
    submit(app, h::CHECK_CERTIFICATE, &[], deadline)?;
    let observation = app
        .https_editor
        .as_ref()
        .and_then(h::Editor::certificate_observation)
        .ok_or("HTTPS actual certificate observation absent")?;
    require(
        app.view().grid.as_ref().is_some_and(|g| g.rows.len() == 7),
        "seven native rendered certificate facts",
    )?;
    if owned {
        require(
            [
                observation.report.found.check,
                observation.report.current_time.check,
                observation.report.private_key.check,
                observation.report.server_auth_eku.check,
            ]
            .iter()
            .all(|v| *v == CertificateCheck::Satisfied),
            "owned fixture four independent native facts",
        )?;
    }
    let report = &observation.report;
    println!(
        concat!(
            "HTTPS_SETTINGS_CERTIFICATE_OBSERVATION ",
            "{{\"scope\":\"CurrentUserMyCacheOnly\",",
            "\"publicationRevision\":{},\"identityRevision\":{},\"settingsRevision\":{},",
            "\"found\":{},\"currentTime\":{},\"privateKey\":{},\"serverAuthEku\":{},",
            "\"serverName\":{},\"sslChainPolicy\":{},\"cachedRevocation\":{},",
            "\"chainTrustErrors\":{},\"sslPolicyError\":{},\"readiness\":\"NOT_GRANTED\"}}"
        ),
        observation.publication_revision.get(),
        observation.identity_revision.get(),
        observation.settings_revision.get(),
        fact(report.found),
        fact(report.current_time),
        fact(report.private_key),
        fact(report.server_auth_eku),
        fact(report.server_name),
        fact(report.ssl_chain_policy),
        fact(report.cached_revocation),
        optional_number(report.chain_trust_errors.map(i64::from)),
        optional_number(report.ssl_policy_error.map(i64::from))
    );
    mark(step, "explicit-native-certificate-check")?;
    // Actual native draft capture: request reload with changed form, then
    // choose to KEEP the unsaved controls. Neither action saves or queries.
    submit(app, h::RELOAD, &[(h::PORT, "8444")], deadline)?;
    submit(app, h::KEEP, &[], deadline)?;
    require(
        app.https_editor
            .as_ref()
            .and_then(h::Editor::certificate_observation)
            .is_none(),
        "editing invalidates prior certificate facts",
    )?;
    submit(app, LOCK, &[], deadline)?;
    require(
        app.screen == Screen::Login
            && app.lease.is_none()
            && app
                .view()
                .fields
                .iter()
                .all(|f| ![h::HOST, h::PORT, h::CERTIFICATE, h::SOURCE_NETWORKS].contains(&f.id)),
        "locked page hides configuration draft",
    )?;
    mark(step, "native-draft-retained-across-lock")?;
    login(app, deadline)?;
    settings(app, deadline)?;
    require(
        field(app, h::PORT).as_deref() == Some("8444") && saved(app)? == first,
        "same-owner unsaved draft retained",
    )?;
    mark(step, "native-relogin-preserves-draft")?;
    let mut data = FormData::new();
    for f in app.view().fields {
        if f.kind != FieldKind::ReadOnly {
            data.insert(f.id, f.value).map_err(|_| "HTTPS form")?;
        }
    }
    app.window
        .probe_submit(h::SAVE_CONFIG, data)
        .map_err(|_| "HTTPS original native save admission")?;
    for event in checked(app.window.take_events(), "HTTPS exact native save events")? {
        app.event(event)?;
    }
    while app.https_unadmitted.is_some() {
        require(
            Instant::now() < deadline,
            "HTTPS original unadmitted Busy wait",
        )?;
        std::thread::park_timeout(Duration::from_millis(1));
        app.retry_https_unadmitted()?;
    }
    let original = app
        .https_editor
        .as_ref()
        .and_then(h::Editor::query_request)
        .ok_or("HTTPS original accepted request absent")?;
    require(app.pending.is_some(), "HTTPS original shared worker ticket")?;
    mark(step, "native-original-save-admitted")?;
    while !app
        .pending
        .as_ref()
        .ok_or("HTTPS original ticket lost")?
        .ticket
        .is_finished()
    {
        require(
            Instant::now() < deadline,
            "HTTPS original actual completion deadline",
        )?;
        std::thread::sleep(Duration::from_millis(1));
    }
    let committed = saved(app)?;
    require(
        committed
            .settings
            .configuration
            .as_ref()
            .is_some_and(|c| c.port == 8444),
        "actual SQLite COMMIT before delivery cancellation",
    )?;
    app.window
        .probe_submit(h::CANCEL_WAIT, FormData::new())
        .map_err(|_| "HTTPS actual native cancel-wait action")?;
    for event in checked(app.window.take_events(), "HTTPS cancel-wait native events")? {
        app.event(event)?;
    }
    pump(app, deadline)?;
    mark(step, "real-commit-delivery-cancelled")?;
    require(
        app.https_editor.as_ref().and_then(h::Editor::query_request) == Some(original.clone())
            && app
                .https_editor
                .as_ref()
                .is_some_and(h::Editor::has_unknown),
        "original request survives suppressed delivery",
    )?;
    mark(step, "original-request-retained-after-cancel")?;
    submit(app, LOCK, &[], deadline)?;
    login(app, deadline)?;
    settings(app, deadline)?;
    require(
        app.https_editor.as_ref().and_then(h::Editor::query_request) == Some(original),
        "new valid same-owner lease queries original stable actor",
    )?;
    mark(step, "lock-and-relogin-original-owner")?;
    submit(app, h::QUERY, &[], deadline)?;
    mark(step, "native-explicit-original-outcome-query")?;
    require(
        app.view().description.contains("历史记录：已保存为版本")
            && app
                .https_editor
                .as_ref()
                .is_some_and(h::Editor::has_unknown)
            && saved(app)? == committed,
        "historical query neither rebases nor writes",
    )?;
    mark(step, "historical-receipt-not-current-cas")?;
    submit(app, h::RELOAD, &[], deadline)?;
    submit(app, h::CONFIRM_DISCARD, &[], deadline)?;
    mark(step, "explicit-risk-confirmed-current-reload")?;
    require(
        saved(app)? == committed
            && field(app, h::PORT).as_deref() == Some("8444")
            && app.https_editor.as_ref().is_some_and(|e| !e.has_unknown()),
        "fresh explicit current read",
    )?;
    mark(step, "current-config-readback")?;
    submit(app, h::CLOSE, &[], deadline)?;
    require(
        app.screen == Screen::Web
            && app.view().description.contains(web_editor::NOT_READY)
            && app
                .view()
                .actions
                .iter()
                .filter(|a| [web_editor::ENABLE, web_editor::BEGIN].contains(&a.id))
                .all(|a| !a.enabled),
        "ordinary listener still gated",
    )?;
    mark(step, "device-page-remains-unavailable")?;
    Ok(())
}
pub(super) fn run(reference: Option<&str>) -> Check<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(120);
    let owned = reference.is_some();
    let reference = fc_domain::https_settings::HttpsCertificateRef::parse(
        reference.unwrap_or("0000000000000000000000000000000000000000"),
    )
    .map_err(|_| "HTTPS probe expects exact40hex public reference")?
    .to_hex();
    let mut nonce = [0; 16];
    checked(SystemRandom.fill(&mut nonce), "HTTPS nonce")?;
    let nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-HttpsSettings-{}-{nonce}",
        std::process::id()
    ));
    let paths = checked(
        DataPaths::isolated_test(root.clone()),
        "HTTPS owned deployment",
    )?;
    let deployment = checked(paths.prepare_for_current_user(), "HTTPS protected root")?;
    checked(deployment.read_back(), "HTTPS ACL readback")?;
    let instance = checked(InstanceGuard::acquire(paths.root()), "HTTPS instance")?;
    let mut app = make_app(&paths, deadline)?;
    let mut steps = 0;
    let result = workflow(&mut app, &paths, &reference, owned, &mut steps, deadline);
    stop_owned_app(app, deadline)?;
    mark(&mut steps, "actual-shared-worker-writer-shutdown")?;
    drop(instance);
    drop(deployment);
    checked(fs::remove_dir_all(&root), "HTTPS closed deployment cleanup")?;
    require(!root.exists(), "HTTPS exact root gone")?;
    result?;
    mark(&mut steps, "owned-deployment-cleanup")?;
    require(steps == ORDER.len(), "HTTPS all ordered steps")?;
    println!("HTTPS_SETTINGS_NATIVE_PASS steps={steps} shared_workers=2 original_result_query=PASS listener=NOT_ENABLED certificate_scope=CurrentUserMyCacheOnly certificate_query_count=1 readiness=NOT_GRANTED physical_shutdown=PASS isolated_cleanup=PASS elapsed_ms={}",started.elapsed().as_millis());
    Ok(())
}
