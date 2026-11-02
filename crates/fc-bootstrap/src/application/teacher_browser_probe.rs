//! Explicit protected G03 fixture. Browser business is DOM-only; this module
//! performs real local setup, native pairing approval and fixed diagnostic phases.
use super::*;
use fc_ports::{Clock, MonotonicClock};
use https_settings_editor as h;
use std::io::Write;
use std::os::windows::fs::MetadataExt;
use voice_management_probe::{
    checked, login, open, pump, require, stop_owned_app, submit, LOGIN_NAME, PASSWORD_VALUE,
};
type Check<T> = std::result::Result<T, String>;
const TEACHER_LOGIN: &str = "synthetic-ordinary-teacher";
// Fields and constructor private. A raw marker, bool or client ACK cannot create this token.
pub(super) struct OwnedDeployment {
    root: PathBuf,
    marker: String,
}
impl OwnedDeployment {
    fn create(old: &Path, new: &Path, marker: &str) -> Check<Self> {
        require(
            marker
                .strip_prefix("FutureClassroom-Test-TLS-")
                .is_some_and(|v| {
                    v.len() == 32
                        && v.bytes().all(|b| {
                            b.is_ascii_hexdigit()
                                && (!b.is_ascii_alphabetic() || b.is_ascii_lowercase())
                        })
                }),
            "exact owned G03 marker",
        )?;
        let root = old.parent().ok_or("owned package parent")?;
        require(
            old.is_absolute()
                && new.is_absolute()
                && old.file_name().is_some_and(|n| n == "web-old")
                && new.file_name().is_some_and(|n| n == "web-new")
                && new.parent() == Some(root)
                && root.file_name().is_some_and(|n| n == marker),
            "fixed owned old/new package siblings",
        )?;
        let meta = checked(fs::symlink_metadata(root), "owned package root metadata")?;
        require(
            meta.is_dir() && meta.file_attributes() & 0x400 == 0,
            "owned package root no reparse",
        )?;
        require(
            checked(
                read_bounded(&root.join(".tls-probe-root"), 256),
                "owned original marker",
            )? == marker.as_bytes(),
            "original marker exact bytes",
        )?;
        for package in [old, new] {
            let meta = checked(fs::symlink_metadata(package), "package metadata")?;
            require(
                meta.is_dir() && meta.file_attributes() & 0x400 == 0,
                "package no reparse",
            )?;
        }
        let a = checked(
            read_bounded(&old.join("fc-assets.json"), 65536),
            "old actual asset ledger",
        )?;
        let b = checked(
            read_bounded(&new.join("fc-assets.json"), 65536),
            "new actual asset ledger",
        )?;
        require(
            a.len() <= 65536 && b.len() <= 65536 && a != b,
            "different real asset ledgers required",
        )?;
        Ok(Self {
            root: root.into(),
            marker: marker.into(),
        })
    }
    fn publish(&self, leaf: &str, value: &str) -> Check<()> {
        let mut f = checked(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.root.join(leaf)),
            "fresh fixed rendezvous",
        )?;
        checked(f.write_all(value.as_bytes()), "fixed rendezvous write")?;
        checked(f.sync_all(), "rendezvous flush")
    }
}
fn wait(app: &mut App, deadline: Instant, condition: impl Fn(&App) -> bool) -> Check<()> {
    while !condition(app) {
        require(Instant::now() < deadline, "original native deadline")?;
        pump(app, deadline)?;
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
fn running(app: &App) -> Option<u64> {
    match app.web_readiness {
        web_editor::Readiness::Running { generation } if generation != 0 => Some(generation),
        _ => None,
    }
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

fn request(
    app: &mut App,
    owned: &OwnedDeployment,
    index: u8,
    action: &str,
    deadline: Instant,
) -> Check<()> {
    let path = owned.root.join(format!("teacher-action-{index}.request"));
    while !path.exists() {
        pump(app, deadline)?;
        std::thread::sleep(Duration::from_millis(10));
    }
    let meta = checked(fs::symlink_metadata(&path), "fixed action metadata")?;
    require(
        meta.is_file() && meta.file_attributes() & 0x400 == 0 && meta.len() <= 256,
        "fixed action bounded nonreparse",
    )?;
    require(
        checked(read_bounded(&path, 256), "fixed action read")?
            == format!("G03_ACTION_V1\n{}\n{index}\n{action}\n", owned.marker).as_bytes(),
        "fixed ordered action only",
    )
}
fn ack(owned: &OwnedDeployment, index: u8, action: &str, fields: &str) -> Check<()> {
    owned.publish(&format!("teacher-action-{index}.ack"),&format!("{{\"type\":\"ACK\",\"action\":\"{action}\",\"marker\":\"{}\",\"status\":\"PASS\"{fields}}}\n",owned.marker))
}
fn begin_pair(
    app: &mut App,
    owned: &OwnedDeployment,
    cycle: u8,
    deadline: Instant,
) -> Check<fc_security::web::SecretText> {
    submit(app, web_editor::REFRESH, &[], deadline)?;
    submit(app, web_editor::BEGIN, &[], deadline)?;
    let code = app
        .web_editor
        .as_ref()
        .and_then(web_editor::Editor::code_text)
        .ok_or("actual pairing code absent")?;
    require(
        code.as_str().len() == 8 && code.as_str().bytes().all(|b| b.is_ascii_digit()),
        "actual eight digit native code",
    )?;
    owned.publish(
        &format!("teacher-pairing-{cycle}.secret"),
        &format!(
            "G03_PAIR_V1\n{}\n{cycle}\n{}\n{TEACHER_LOGIN}\n",
            owned.marker,
            code.as_str()
        ),
    )?;
    println!(
        "TEACHER_BROWSER_PAIR_READY cycle={cycle} marker={}",
        owned.marker
    );
    checked(std::io::stdout().flush(), "pair event flush")?;
    Ok(code)
}
fn approve_pair(app: &mut App, owned: &OwnedDeployment, cycle: u8, deadline: Instant) -> Check<()> {
    let request = loop {
        submit(app, web_editor::REFRESH, &[], deadline)?;
        let choices = app
            .view()
            .fields
            .into_iter()
            .find_map(|f| {
                if f.id == web_editor::REQUEST {
                    if let FieldKind::Choice(c) = f.kind {
                        Some(c)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .ok_or("pair list absent")?;
        require(choices.len() <= 1, "only original teacher browser request")?;
        if let Some(choice) = choices.first() {
            break choice.value.clone();
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    checked(
        app.window.probe_choice(web_editor::REQUEST, &request),
        "original native request choice",
    )?;
    pump(app, deadline)?;
    require(
        request.parse::<u64>().is_ok_and(|id| id != 0),
        "original actual request ID",
    )?;
    submit(
        app,
        web_editor::APPROVE,
        &[(web_editor::REQUEST, &request)],
        deadline,
    )?;
    let teacher = app
        .state
        .account_by_login(TEACHER_LOGIN)
        .map_err(|_| "approved synthetic teacher missing")?;
    require(
        teacher.principal.role == Role::TeacherAdmin && teacher.principal.enabled,
        "actual independent teacher approval role",
    )?;
    println!(
        "TEACHER_BROWSER_APPROVED cycle={cycle} marker={} request={request} teacherId={} role=TEACHER_ADMIN",
        owned.marker,
        teacher.principal.id.get()
    );
    checked(
        std::io::stdout().flush(),
        "public original approval observation flush",
    )
}
fn disable_https(app: &mut App, deadline: Instant) -> Check<()> {
    let began = Instant::now();
    let stop_deadline = deadline.min(began + Duration::from_secs(5));
    submit(app, web_editor::DISABLE, &[], stop_deadline)?;
    submit(app, web_editor::CONFIRM, &[], stop_deadline)?;
    wait(app, stop_deadline, |a| {
        a.teacher_https_quiescent() && a.web_readiness == web_editor::Readiness::Disabled
    })?;
    let report = app
        .teacher_https
        .owner
        .as_ref()
        .and_then(|o| o.network_report())
        .ok_or("original network drain report absent")?;
    require(
        report.native_drained
            && report.active_records == 0
            && report.pending_jobs == 0
            && report.cleanup_pending == 0
            && report.retained_cleanup == 0,
        "original TLS/network resources physically drained",
    )
}
fn revoke(app: &mut App, deadline: Instant) -> Check<()> {
    submit(app, BACK, &[], deadline)?;
    submit(app, TO_ACCOUNTS, &[], deadline)?;
    let row = app
        .view()
        .grid
        .as_ref()
        .and_then(|g| {
            g.rows
                .iter()
                .position(|r| r.cells.first().is_some_and(|s| s == TEACHER_LOGIN))
        })
        .ok_or("actual teacher grid row absent")?;
    checked(
        app.window.probe_select_grid(row, 0),
        "select actual teacher row",
    )?;
    pump(app, deadline)?;
    submit(app, accounts_editor::EDIT, &[], deadline)?;
    submit(
        app,
        accounts_editor::REVIEW,
        &[(accounts_editor::ENABLED, "0")],
        deadline,
    )?;
    submit(app, accounts_editor::CONFIRM, &[], deadline)?;
    require(
        !app.state
            .account_by_login(TEACHER_LOGIN)
            .map_err(|_| "teacher readback")?
            .principal
            .enabled,
        "native disabled teacher durable readback",
    )
}
fn workflow(
    app: &mut App,
    paths: &DataPaths,
    deployment: (&str, u16, &Path),
    owned: &OwnedDeployment,
    midnight: &clock_source::MidnightController,
    deadline: Instant,
) -> Check<()> {
    let (reference, port, new_package) = deployment;
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
    create_teacher(app, deadline)?;
    // Hidden fixtures start Unavailable; Disabled grants no runtime readiness.
    app.set_web_readiness(web_editor::Readiness::Disabled);
    submit(app, TO_WEB, &[], deadline)?;
    require(
        app.teacher_https_quiescent() && app.web_readiness == web_editor::Readiness::Disabled,
        "ordinary HTTPS default off",
    )?;
    submit(app, web_editor::HTTPS_SETTINGS, &[], deadline)?;
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
    require(
        app.teacher_https_quiescent() && app.web_readiness == web_editor::Readiness::Disabled,
        "save never starts listener",
    )?;
    submit(app, h::CLOSE, &[], deadline)?;
    submit(app, web_editor::ENABLE, &[], deadline)?;
    wait(app, deadline, |a| running(a).is_some())?;
    println!(
        "TEACHER_BROWSER_READY origin=https://127.0.0.1:{port} marker={}",
        owned.marker
    );
    checked(std::io::stdout().flush(), "public teacher ready flush")?;

    // Voice is enabled by a real local administrator action; output remains real
    // WaveOut and is never called fake/silent-driver evidence by this fixture.
    submit(app, BACK, &[], deadline)?;
    submit(app, TO_DISPLAY, &[], deadline)?;
    submit(app, TO_VOICE, &[], deadline)?;
    let settings = voice_management_probe::saved(app)?;
    if settings.enabled {
        submit(app, voice_editor::DISABLE, &[], deadline)?;
        submit(
            app,
            voice_editor::CONFIRM,
            &[(voice_editor::ACK, "1")],
            deadline,
        )?;
    }
    submit(app, voice_editor::ENABLE, &[], deadline)?;
    submit(
        app,
        voice_editor::CONFIRM,
        &[(voice_editor::ACK, "1")],
        deadline,
    )?;
    require(
        voice_management_probe::saved(app)?.enabled,
        "actual remote voice enabled",
    )?;
    submit(app, voice_editor::CLOSE, &[], deadline)?;
    submit(app, TO_WEB, &[], deadline)?;
    begin_pair(app, owned, 1, deadline)?;
    approve_pair(app, owned, 1, deadline)?;
    request(app, owned, 0, "CACHE_UPGRADE", deadline)?;
    disable_https(app, deadline)?;
    app.teacher_https.probe_package = Some(new_package.to_owned());
    submit(app, web_editor::ENABLE, &[], deadline)?;
    wait(app, deadline, |a| running(a).is_some())?;
    let code = begin_pair(app, owned, 2, deadline)?;
    ack(
        owned,
        0,
        "CACHE_UPGRADE",
        &format!(
            ",\"oldOwnerDrained\":true,\"sameOrigin\":\"https://127.0.0.1:{port}\",\"pairingCode\":\"{}\"", code.as_str()
        ),
    )?;
    approve_pair(app, owned, 2, deadline)?;
    request(app, owned, 1, "VOICE_SETTLED", deadline)?;
    wait(app, deadline, |a| {
        a.audio
            .as_ref()
            .and_then(|v| v.status().ok())
            .is_some_and(|s| !s.physical_thread_running && !s.faulted)
    })?;
    let audio = app.audio.as_ref().ok_or("actual audio runtime absent")?;
    checked(
        audio.reap_nonblocking(),
        "original returned audio thread join/reap",
    )?;
    require(
        audio.status().is_ok_and(|s| {
            s.phase == RuntimePhase::Idle && !s.physical_thread_running && !s.faulted
        }),
        "original audio owner actually idle after nonblocking reap",
    )?;
    let voice = voice_management_probe::saved(app)?;
    require(
        voice.active_jobs == 0 && voice.queued_jobs == 0 && voice.playing_jobs == 0,
        "voice durable terminal owner",
    )?;
    ack(
        owned,
        1,
        "VOICE_SETTLED",
        ",\"activeJobs\":0,\"audioOwnerReturned\":true",
    )?;
    request(app, owned, 2, "MIDNIGHT", deadline)?;
    let tick = app.clock.monotonic_millis();
    checked(midnight.advance_once(owned), "one instance-local midnight")?;
    require(
        app.clock.monotonic_millis() >= tick
            && app.clock.now().date
                == fc_domain::time::LocalDate::new(2026, 11, 3).map_err(|_| "fixed date")?,
        "clock source actual advanced",
    )?;
    ack(
        owned,
        2,
        "MIDNIGHT",
        ",\"clockScope\":\"ISOLATED_CIVIL_ONLY\",\"oldDate\":\"2026-11-02\",\"newDate\":\"2026-11-03\",\"osClockChanged\":false",
    )?;
    request(app, owned, 3, "REVOKE", deadline)?;
    revoke(app, deadline)?;
    ack(
        owned,
        3,
        "REVOKE",
        ",\"actualIdentityWrite\":true,\"teacherLogin\":\"synthetic-ordinary-teacher\"",
    )?;
    request(app, owned, 4, "COMPLETE", deadline)?;
    submit(app, accounts_editor::CLOSE, &[], deadline)?;
    submit(app, TO_WEB, &[], deadline)?;
    disable_https(app, deadline)?;
    ack(owned, 4, "COMPLETE", ",\"nativeHttpsDrained\":true")
}
pub(super) fn run(reference: &str, port: u16, old: &Path, new: &Path, marker: &str) -> Check<()> {
    require(port >= 1024, "diagnostic high port")?;
    let reference = fc_domain::https_settings::HttpsCertificateRef::parse(reference)
        .map_err(|_| "public certificate reference")?
        .to_hex();
    let owned = OwnedDeployment::create(old, new, marker)?;
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-TeacherBrowser-{}-{marker}",
        std::process::id()
    ));
    require(!root.exists(), "fresh isolated teacher deployment")?;
    let paths = checked(
        DataPaths::isolated_test(root.clone()),
        "isolated teacher paths",
    )?;
    let deployment = checked(paths.prepare_for_current_user(), "real protected ACL root")?;
    checked(deployment.read_back(), "original ACL readback")?;
    let instance = checked(InstanceGuard::acquire(paths.root()), "isolated instance")?;
    // Construct the diagnostic civil instance only after the owned token AND
    // actual protected deployment readback. Normal startup keeps AppClock::new.
    let (clock, midnight) = CurrentClock::for_owned_teacher_probe(&owned);
    let started = Instant::now();
    let deadline = started + Duration::from_secs(120);
    let mut app = create_app(Arc::new(clock), true)?;
    app.root = paths.root().to_owned();
    app.desktop.set_root(paths.root().to_owned());
    app.next_content_maintenance = deadline;
    app.next_voice_maintenance = deadline;
    app.next_calendar_maintenance = deadline;
    app.teacher_https.probe_package = Some(old.to_owned());
    let result = workflow(
        &mut app,
        &paths,
        (&reference, port, new),
        &owned,
        &midnight,
        deadline,
    );
    // Never delete a live/unknown worker, audio, TLS, writer owner after failure.
    stop_owned_app(app, deadline)?;
    drop(instance);
    drop(deployment);
    checked(
        fs::remove_dir_all(&root),
        "physically retired isolated teacher fixture cleanup",
    )?;
    require(!root.exists(), "isolated deployment actually absent")?;
    result?;
    println!(
        "TEACHER_BROWSER_NATIVE_PASS pairing=ACTUAL_DOM_REQUIRED business=SEPARATE_DOM_REPORT clock=ISOLATED_CIVIL_ONLY audio=ZERO_WAV_CAPTURE_PHYSICAL_OWNER_SEPARATE osClock=UNTOUCHED windows7=NOT_RUN windows10=NOT_RUN elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(())
}
