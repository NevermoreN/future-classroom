//! Hidden real controls and the ordinary shared workers against one protected,
//! synthetic deployment. This probe submits no audio asset or playback job.
use super::*;
use fc_bootstrap::backend::VoiceManagementSnapshot;
use fc_domain::voice::VoiceLimits;
use voice_editor as v;

type Check<T> = std::result::Result<T, String>;
pub(super) const STEPS: usize = 36;
const LOGIN_NAME: &str = "synthetic-voice-settings-admin";
const PASSWORD_VALUE: &str = "Synthetic voice settings 2026!";
const ORDER: [&str; STEPS] = [
    "native-setup-global-administrator",
    "native-open-voice-settings",
    "saved-default-enabled-no-jobs",
    "native-disable-review-no-write",
    "native-unchecked-confirm-rejected",
    "native-cancel-no-write",
    "native-confirm-disable-persisted",
    "native-refresh-disabled",
    "native-enable-review-no-write",
    "native-lock-discards-review",
    "native-relogin",
    "native-reopen-saved-disabled",
    "native-limits-edit-review-no-write",
    "native-limits-unchecked-confirm-rejected",
    "native-limits-cancel-keeps-unsaved-draft",
    "native-limits-confirm-custom-persisted",
    "first-app-physical-shutdown",
    "second-app-real-database-reopen",
    "native-relogin-after-database-reopen",
    "native-reopen-persisted-disabled",
    "native-limits-persisted-after-database-reopen",
    "native-confirm-enable-persisted",
    "native-disable-review-captured",
    "external-concurrent-cas-disable",
    "native-stale-review-rejected",
    "native-refresh-current-config",
    "native-confirm-enable-after-refresh",
    "native-limits-capture-review",
    "external-concurrent-limits-cas",
    "native-limits-stale-review-rejected",
    "native-limits-refresh-preserves-draft",
    "native-limits-confirm-after-refresh",
    "native-limits-restore-defaults",
    "native-close-settings",
    "second-app-physical-shutdown",
    "isolated-deployment-cleanup",
];

fn checked<T, E>(value: std::result::Result<T, E>, stage: &str) -> Check<T> {
    value.map_err(|_| format!("voice-settings-probe: {stage}"))
}
fn require(value: bool, stage: &str) -> Check<()> {
    if value {
        Ok(())
    } else {
        Err(format!("voice-settings-probe: {stage}"))
    }
}
fn mark(step: &mut usize, label: &str) -> Check<()> {
    require(ORDER.get(*step) == Some(&label), "exact ordered step")?;
    println!("VOICE_SETTINGS_NATIVE_STEP index={} name={label}", *step);
    *step += 1;
    Ok(())
}

fn pump(app: &mut App, deadline: Instant) -> Check<()> {
    loop {
        require(Instant::now() < deadline, "native workflow deadline")?;
        for event in checked(app.window.take_events(), "take hidden native events")? {
            app.event(event)?;
        }
        app.poll()?;
        require(
            !app.pool.is_faulted() && !app.stopping && !app.terminate_required,
            "healthy shared workers",
        )?;
        if app.pending.is_none() {
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
        "hidden native control admission",
    )?;
    pump(app, deadline)
}

/// A diagnostic observer only: the writer performs the real bounded read. It
/// does not update the editor, renew a lease or submit a configuration change.
/// All product changes, including refresh, still use App::submit through its
/// normal native event route. Only the explicitly marked concurrent writer below
/// invokes set_enabled directly.
fn saved(app: &App) -> Check<VoiceManagementSnapshot> {
    checked(
        app.voice_service
            .as_ref()
            .ok_or("voice-settings-probe: voice service absent")?
            .management_snapshot(
                app.lease
                    .ok_or("voice-settings-probe: local lease absent")?,
            ),
        "read actual persisted voice metadata",
    )
}

fn no_jobs(state: &VoiceManagementSnapshot) -> Check<()> {
    require(
        state.active_jobs == 0 && state.queued_jobs == 0 && state.playing_jobs == 0,
        "no audio tasks created by settings probe",
    )
}
fn shown(app: &App, state: &VoiceManagementSnapshot) -> Check<()> {
    require(
        app.view().description.contains(if state.enabled {
            "远程语音配置：开启"
        } else {
            "远程语音配置：关闭"
        }) && app
            .view()
            .description
            .contains("活跃任务：0；排队任务：0；播放中任务：0"),
        "native rendered metadata matches actual saved setting and zero job counts",
    )
}
fn same_saved(before: &VoiceManagementSnapshot, after: &VoiceManagementSnapshot) -> Check<()> {
    require(
        before.identity_revision == after.identity_revision
            && before.revision == after.revision
            && before.enabled == after.enabled
            && before.limits == after.limits,
        "review, rejection or cancel must not change persisted configuration",
    )?;
    no_jobs(after)
}
fn has_review(app: &App) -> bool {
    app.voice_editor.is_some() && app.view().fields.iter().any(|field| field.id == v::ACK)
}
fn review(app: &mut App, enabled: bool, deadline: Instant) -> Check<()> {
    submit(
        app,
        if enabled { v::ENABLE } else { v::DISABLE },
        &[],
        deadline,
    )?;
    require(has_review(app), "owned local consent review")?;
    require(
        app.view()
            .fields
            .iter()
            .any(|field| field.id == v::ACK && field.value == "0"),
        "review defaults unacknowledged",
    )
}
fn limits_review(app: &mut App, values: &[&str; 4], deadline: Instant) -> Check<()> {
    submit(
        app,
        v::SAVE_LIMITS,
        &[
            (v::DURATION_SECONDS, values[0]),
            (v::PENDING_JOBS, values[1]),
            (v::PENDING_PER_OWNER, values[2]),
            (v::REPEAT_TOTAL, values[3]),
        ],
        deadline,
    )?;
    require(
        has_review(app)
            && app
                .view()
                .fields
                .iter()
                .any(|field| field.id == v::ACK && field.value == "0"),
        "limits consent defaults unacknowledged",
    )
}
fn custom_limits() -> VoiceLimits {
    VoiceLimits {
        max_duration_millis: 1250,
        max_pending_jobs: 3,
        max_pending_per_owner: 1,
        max_repeat_total: 2,
    }
}
fn limits_changed(
    app: &App,
    before: &VoiceManagementSnapshot,
    limits: VoiceLimits,
) -> Check<VoiceManagementSnapshot> {
    let after = saved(app)?;
    require(
        after.revision != before.revision
            && after.limits == limits
            && after.enabled == before.enabled
            && !has_review(app),
        "actual persisted limits changed without switching configuration",
    )?;
    no_jobs(&after)?;
    Ok(after)
}
fn confirm(app: &mut App, deadline: Instant) -> Check<()> {
    submit(app, v::CONFIRM, &[(v::ACK, "1")], deadline)
}
fn changed(
    app: &mut App,
    previous: &VoiceManagementSnapshot,
    enabled: bool,
    deadline: Instant,
) -> Check<VoiceManagementSnapshot> {
    let actual = saved(app)?;
    require(
        actual.enabled == enabled && actual.revision != previous.revision,
        "confirmed native change has a new real persisted revision",
    )?;
    no_jobs(&actual)?;
    if let Err(render_error) = shown(app, &actual) {
        let view = app.view();
        let guarded_refresh = app.status == "语音状态已更新，请重新读取后核对。"
            && !has_review(app)
            && [v::ENABLE, v::DISABLE, v::SAVE_LIMITS].iter().all(|id| {
                view.actions
                    .iter()
                    .any(|action| action.id == *id && !action.enabled)
            });
        // A diagnostic of this one failed observation, never a retry of a write.
        // Do not print school/account data, audio, paths or opaque credentials.
        let status: String = app.status.chars().take(128).collect();
        println!("VOICE_SETTINGS_NATIVE_REFRESH_OBSERVATION status={status:?} rendered_enabled={} rendered_disabled={} saved_enabled={} saved_voice_revision={} saved_identity_revision={} saved_publication_revision={} saved_runtime_generation={} guarded_refresh={guarded_refresh}",
            view.description.contains("远程语音配置：开启"),
            view.description.contains("远程语音配置：关闭"),
            actual.enabled, actual.revision.get(), actual.identity_revision.get(),
            actual.publication_revision.get(), actual.runtime_generation);
        if !guarded_refresh {
            return Err(render_error);
        }
        // Exactly one explicit native read is authorized by the canonical stale
        // state. A rendering error or a still-enabled mutation must still fail.
        // The original persisted change and its revision are never replayed.
        submit(app, v::REFRESH, &[], deadline)?;
        let refreshed = saved(app)?;
        same_saved(&actual, &refreshed)?;
        shown(app, &actual)?;
    }
    require(
        !has_review(app),
        "successful change clears captured consent",
    )?;
    Ok(actual)
}

fn make_app(paths: &DataPaths, deadline: Instant) -> Check<App> {
    let mut app = create_app(Arc::new(CurrentClock::new()), true)?;
    app.root = paths.root().to_owned();
    app.desktop.set_root(paths.root().to_owned());
    // Keep unrelated periodic content/calendar work outside this bounded probe.
    // Voice runtime availability and its real shutdown remain ordinary App code.
    app.next_content_maintenance = deadline;
    app.next_voice_maintenance = deadline;
    app.next_calendar_maintenance = deadline;
    Ok(app)
}
fn open(app: &mut App, paths: &DataPaths, deadline: Instant) -> Check<()> {
    app.submit(Work::Open(paths.clone(), app.clock.clone()))?;
    pump(app, deadline)
}
fn login(app: &mut App, deadline: Instant) -> Check<()> {
    submit(
        app,
        LOG_IN,
        &[(LOGIN, LOGIN_NAME), (PASSWORD, PASSWORD_VALUE)],
        deadline,
    )?;
    require(
        app.screen == Screen::Home
            && app.lease.is_some_and(|lease| {
                lease.origin() == TrustedOrigin::Local
                    && app.current_global_owner() == Some(lease.principal_id())
            }),
        "real local global administrator login",
    )
}
fn open_settings(app: &mut App, deadline: Instant) -> Check<()> {
    checked(
        app.view().validate(),
        "valid home within original action budget",
    )?;
    submit(app, TO_DISPLAY, &[], deadline)?;
    require(
        app.screen == Screen::Display,
        "native system settings navigation",
    )?;
    checked(
        app.view().validate(),
        "valid settings within original action budget",
    )?;
    submit(app, TO_VOICE, &[], deadline)?;
    require(
        app.screen == Screen::Voice
            && app
                .voice_editor
                .as_ref()
                .is_some_and(|editor| app.current_global_owner() == Some(editor.owner())),
        "real native voice page owned by current global administrator",
    )?;
    let view = app.view();
    checked(view.validate(), "valid native voice view")?;
    require(
        view.description.contains("教师连接服务当前未就绪")
            && view.description.contains("不能证明教室已听到声音"),
        "settings do not claim production connection or audibility",
    )
}

fn first_app(
    app: &mut App,
    paths: &DataPaths,
    steps: &mut usize,
    deadline: Instant,
) -> Check<Revision> {
    open(app, paths, deadline)?;
    require(app.screen == Screen::Setup, "fresh isolated native setup")?;
    submit(
        app,
        SAVE,
        &[
            (SCHOOL, "Synthetic voice settings school"),
            (LOGIN, LOGIN_NAME),
            (NAME, "Synthetic voice settings administrator"),
            (PASSWORD, PASSWORD_VALUE),
            (CONFIRM, PASSWORD_VALUE),
        ],
        deadline,
    )?;
    require(app.screen == Screen::Code, "native setup committed")?;
    submit(app, ACK, &[], deadline)?;
    if app.screen == Screen::Login {
        login(app, deadline)?;
    }
    require(
        app.screen == Screen::Home,
        "native first administrator ready",
    )?;
    mark(steps, "native-setup-global-administrator")?;
    open_settings(app, deadline)?;
    mark(steps, "native-open-voice-settings")?;
    let initial = saved(app)?;
    require(
        initial.enabled && !has_review(app),
        "default enabled without a consent review",
    )?;
    no_jobs(&initial)?;
    shown(app, &initial)?;
    mark(steps, "saved-default-enabled-no-jobs")?;
    review(app, false, deadline)?;
    same_saved(&initial, &saved(app)?)?;
    mark(steps, "native-disable-review-no-write")?;
    submit(app, v::CONFIRM, &[], deadline)?;
    require(
        has_review(app) && !app.status.is_empty(),
        "unchecked native confirmation rejected",
    )?;
    same_saved(&initial, &saved(app)?)?;
    mark(steps, "native-unchecked-confirm-rejected")?;
    submit(app, v::CANCEL, &[], deadline)?;
    require(!has_review(app), "native cancel discards review")?;
    same_saved(&initial, &saved(app)?)?;
    mark(steps, "native-cancel-no-write")?;
    review(app, false, deadline)?;
    confirm(app, deadline)?;
    let disabled = changed(app, &initial, false, deadline)?;
    mark(steps, "native-confirm-disable-persisted")?;
    submit(app, v::REFRESH, &[], deadline)?;
    same_saved(&disabled, &saved(app)?)?;
    shown(app, &disabled)?;
    mark(steps, "native-refresh-disabled")?;
    review(app, true, deadline)?;
    same_saved(&disabled, &saved(app)?)?;
    mark(steps, "native-enable-review-no-write")?;
    submit(app, LOCK, &[], deadline)?;
    require(
        app.lease.is_none() && app.voice_editor.is_none() && app.screen == Screen::Login,
        "independent native lock discards owner review",
    )?;
    mark(steps, "native-lock-discards-review")?;
    login(app, deadline)?;
    mark(steps, "native-relogin")?;
    open_settings(app, deadline)?;
    same_saved(&disabled, &saved(app)?)?;
    shown(app, &disabled)?;
    require(!has_review(app), "local relogin cannot restore old consent")?;
    mark(steps, "native-reopen-saved-disabled")?;
    let before_limits = saved(app)?;
    limits_review(app, &["1.250", "3", "1", "2"], deadline)?;
    same_saved(&before_limits, &saved(app)?)?;
    mark(steps, "native-limits-edit-review-no-write")?;
    submit(app, v::CONFIRM, &[], deadline)?;
    require(
        has_review(app) && !app.status.is_empty(),
        "unchecked limits confirmation rejected",
    )?;
    same_saved(&before_limits, &saved(app)?)?;
    mark(steps, "native-limits-unchecked-confirm-rejected")?;
    submit(app, v::CANCEL, &[], deadline)?;
    require(
        !has_review(app)
            && app
                .voice_editor
                .as_ref()
                .is_some_and(|editor| editor.has_unsaved_changes()),
        "limits cancellation keeps unsaved draft without consent",
    )?;
    same_saved(&before_limits, &saved(app)?)?;
    require(
        app.view()
            .fields
            .iter()
            .any(|field| field.id == v::DURATION_SECONDS && field.value == "1.25"),
        "native draft retained exact milliseconds",
    )?;
    mark(steps, "native-limits-cancel-keeps-unsaved-draft")?;
    limits_review(app, &["1.25", "3", "1", "2"], deadline)?;
    confirm(app, deadline)?;
    let custom = limits_changed(app, &before_limits, custom_limits())?;
    mark(steps, "native-limits-confirm-custom-persisted")?;
    Ok(custom.revision)
}

fn second_app(
    app: &mut App,
    paths: &DataPaths,
    persisted_revision: Revision,
    steps: &mut usize,
    deadline: Instant,
) -> Check<()> {
    open(app, paths, deadline)?;
    require(
        app.screen == Screen::Login && app.state.installation.is_some(),
        "real new Backend reopened existing protected deployment",
    )?;
    mark(steps, "second-app-real-database-reopen")?;
    login(app, deadline)?;
    mark(steps, "native-relogin-after-database-reopen")?;
    open_settings(app, deadline)?;
    let reopened = saved(app)?;
    require(
        !reopened.enabled && reopened.revision == persisted_revision && !has_review(app),
        "real database close and reopen retained disabled configuration",
    )?;
    no_jobs(&reopened)?;
    shown(app, &reopened)?;
    mark(steps, "native-reopen-persisted-disabled")?;
    require(
        reopened.limits == custom_limits(),
        "custom limits survived actual SQLite close and new Backend open",
    )?;
    mark(steps, "native-limits-persisted-after-database-reopen")?;
    review(app, true, deadline)?;
    confirm(app, deadline)?;
    let enabled = changed(app, &reopened, true, deadline)?;
    mark(steps, "native-confirm-enable-persisted")?;
    review(app, false, deadline)?;
    same_saved(&enabled, &saved(app)?)?;
    mark(steps, "native-disable-review-captured")?;
    // The only direct mutation: simulate a concurrent trusted administrator
    // changing the real same service with its captured, current revision.
    checked(
        app.voice_service
            .as_ref()
            .ok_or("voice-settings-probe: service absent")?
            .set_enabled(
                app.lease.ok_or("voice-settings-probe: lease absent")?,
                false,
                enabled.revision,
            ),
        "external same-service concurrent compare-and-swap",
    )?;
    let concurrent = saved(app)?;
    require(
        !concurrent.enabled && concurrent.revision != enabled.revision,
        "external change has a real different durable revision",
    )?;
    no_jobs(&concurrent)?;
    mark(steps, "external-concurrent-cas-disable")?;
    confirm(app, deadline)?;
    same_saved(&concurrent, &saved(app)?)?;
    require(
        !has_review(app) && !app.status.is_empty(),
        "stale native review rejected and invalidated without a write",
    )?;
    require(
        app.view()
            .actions
            .iter()
            .filter(|action| action.id == v::ENABLE || action.id == v::DISABLE)
            .all(|action| !action.enabled),
        "failed stale change requires an actual successful refresh",
    )?;
    mark(steps, "native-stale-review-rejected")?;
    submit(app, v::REFRESH, &[], deadline)?;
    same_saved(&concurrent, &saved(app)?)?;
    shown(app, &concurrent)?;
    mark(steps, "native-refresh-current-config")?;
    review(app, true, deadline)?;
    confirm(app, deadline)?;
    changed(app, &concurrent, true, deadline)?;
    mark(steps, "native-confirm-enable-after-refresh")?;
    let before_limits = saved(app)?;
    let target_limits = VoiceLimits {
        max_duration_millis: 2500,
        max_pending_jobs: 4,
        max_pending_per_owner: 2,
        max_repeat_total: 3,
    };
    limits_review(app, &["2.5", "4", "2", "3"], deadline)?;
    same_saved(&before_limits, &saved(app)?)?;
    mark(steps, "native-limits-capture-review")?;
    let external_limits = VoiceLimits {
        max_duration_millis: 2000,
        max_pending_jobs: 2,
        max_pending_per_owner: 1,
        max_repeat_total: 4,
    };
    checked(
        app.voice_service
            .as_ref()
            .ok_or("voice-settings-probe: service absent")?
            .set_limits_reviewed(
                app.lease.ok_or("voice-settings-probe: lease absent")?,
                external_limits,
                before_limits.revision,
                before_limits.identity_revision,
                before_limits.runtime_generation,
            ),
        "external same-service limits compare-and-swap",
    )?;
    let external = saved(app)?;
    require(
        external.limits == external_limits && external.revision != before_limits.revision,
        "external limits CAS committed",
    )?;
    mark(steps, "external-concurrent-limits-cas")?;
    confirm(app, deadline)?;
    same_saved(&external, &saved(app)?)?;
    require(
        !has_review(app) && !app.status.is_empty(),
        "stale limits confirmation rejected",
    )?;
    mark(steps, "native-limits-stale-review-rejected")?;
    submit(app, v::REFRESH, &[], deadline)?;
    same_saved(&external, &saved(app)?)?;
    require(
        app.view()
            .fields
            .iter()
            .any(|field| field.id == v::DURATION_SECONDS && field.value == "2.5"),
        "refresh retained unsaved limits draft",
    )?;
    mark(steps, "native-limits-refresh-preserves-draft")?;
    limits_review(app, &["2.5", "4", "2", "3"], deadline)?;
    confirm(app, deadline)?;
    let configured = limits_changed(app, &external, target_limits)?;
    mark(steps, "native-limits-confirm-after-refresh")?;
    limits_review(app, &["60", "4", "2", "10"], deadline)?;
    confirm(app, deadline)?;
    limits_changed(app, &configured, VoiceLimits::default())?;
    require(
        !app.voice_editor
            .as_ref()
            .is_some_and(|editor| editor.has_unsaved_changes()),
        "restored defaults have no lingering draft",
    )?;
    mark(steps, "native-limits-restore-defaults")?;
    submit(app, v::CLOSE, &[], deadline)?;
    require(
        app.screen == Screen::Home && app.voice_editor.is_none(),
        "native return to workbench",
    )?;
    mark(steps, "native-close-settings")
}

fn stop_owned_app(mut app: App, deadline: Instant) -> Check<()> {
    app.begin_shutdown();
    while !app.shutdown_complete() {
        require(
            Instant::now() < deadline,
            "physical shutdown deadline; retain isolated deployment for diagnosis",
        )?;
        app.poll()?;
        require(
            !app.terminate_required,
            "physical shutdown remains recoverable",
        )?;
        std::thread::sleep(Duration::from_millis(5));
    }
    require(
        (app.backend.is_none() || app.writer_closed)
            && app.pool.is_finished()
            && app.shell.is_finished(),
        "writer, shared workers and shell physically retired",
    )?;
    require(
        app.audio.as_ref().is_none_or(|audio| {
            audio.status().is_ok_and(|state| {
                state.phase == RuntimePhase::Closed && !state.physical_thread_running
            })
        }),
        "voice runtime physically closed without a playback claim",
    )?;
    let App { pool, .. } = app;
    checked(pool.join(), "join physically finished shared workers")
}

pub(super) fn run() -> Check<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(120);
    let mut random = [0u8; 16];
    checked(SystemRandom.fill(&mut random), "owned deployment nonce")?;
    let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-VoiceSettings-{}-{nonce}",
        std::process::id()
    ));
    let paths = checked(DataPaths::isolated_test(root.clone()), "isolated paths")?;
    let deployment = checked(paths.prepare_for_current_user(), "protected deployment")?;
    checked(deployment.read_back(), "protected ACL readback")?;
    let instance = checked(InstanceGuard::acquire(paths.root()), "owned instance guard")?;
    let mut steps = 0;
    let mut first = make_app(&paths, deadline)?;
    let first_result = first_app(&mut first, &paths, &mut steps, deadline);
    // Always finish physical shutdown, including after a failed assertion. A
    // failed shutdown returns before root deletion so diagnosis is retained.
    stop_owned_app(first, deadline)?;
    let outcome = match first_result {
        Ok(persisted_revision) => {
            mark(&mut steps, "first-app-physical-shutdown")?;
            let mut second = make_app(&paths, deadline)?;
            let result = second_app(
                &mut second,
                &paths,
                persisted_revision,
                &mut steps,
                deadline,
            );
            // This `?` belongs to run, not an assertion-only closure: a failed
            // physical shutdown must bypass the deployment removal below.
            stop_owned_app(second, deadline)?;
            result.and_then(|()| mark(&mut steps, "second-app-physical-shutdown"))
        }
        Err(error) => Err(error),
    };
    drop(instance);
    drop(deployment);
    checked(
        fs::remove_dir_all(&root),
        "remove only physically closed owned deployment",
    )?;
    require(!root.exists(), "owned deployment is absent after cleanup")?;
    outcome?;
    mark(&mut steps, "isolated-deployment-cleanup")?;
    require(steps == STEPS, "complete fixed native workflow")?;
    println!(
        "VOICE_SETTINGS_NATIVE_PASS steps={steps} shared_workers=2 database_reopen=PASS limits_reopen=PASS limits_stale_cas=PASS limits_defaults_restored=PASS audio_jobs=NONE physical_playback=NOT_EXERCISED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(())
}
