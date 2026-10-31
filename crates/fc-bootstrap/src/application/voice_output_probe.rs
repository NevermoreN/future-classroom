//! Real hidden native controls -> original two shared workers -> WinMM query.
//! No fake hardware, audio task, output stream, microphone, or volume operation.
use super::*;
use fc_bootstrap::backend::{
    VoiceDeviceCheckResult, VoiceDeviceObservation, VoiceManagementSnapshot,
};
use voice_editor as v;
use voice_management_probe::{
    checked, login, make_app, no_jobs, open, open_settings, pump, require, same_saved, saved,
    stop_owned_app, submit, LOGIN_NAME, PASSWORD_VALUE,
};

type Check<T> = std::result::Result<T, String>;
const ORDER: [&str; 15] = [
    "native-setup-global-administrator",
    "native-open-voice-settings",
    "saved-default-enabled-no-jobs",
    "native-draft-refresh-no-write",
    "native-nonempty-check-rejected",
    "native-explicit-output-query",
    "query-keeps-saved-state-and-draft",
    "native-refresh-retains-exact-observation",
    "real-thirty-second-observation-expiry",
    "native-explicit-recheck-new-observation",
    "native-lock-discards-observation-and-draft",
    "native-relogin-new-lease",
    "native-reopen-unchecked-original-settings",
    "app-physical-shutdown",
    "isolated-deployment-cleanup",
];
fn mark(step: &mut usize, name: &str) -> Check<()> {
    require(ORDER.get(*step) == Some(&name), "exact output-probe step")?;
    println!("VOICE_OUTPUT_NATIVE_STEP index={} name={name}", *step);
    *step += 1;
    Ok(())
}
fn observation(app: &App, state: &VoiceManagementSnapshot) -> Option<VoiceDeviceObservation> {
    app.voice_editor
        .as_ref()?
        .retained_device_observation(state, app.lease?)
}
fn draft_kept(app: &App) -> Check<()> {
    let view = app.view();
    for (id, expected) in [
        (v::DURATION_SECONDS, "2.5"),
        (v::PENDING_JOBS, "3"),
        (v::PENDING_PER_OWNER, "1"),
        (v::REPEAT_TOTAL, "2"),
    ] {
        require(
            view.fields
                .iter()
                .any(|f| f.id == id && f.value == expected),
            "actual native draft retained without saving",
        )?;
    }
    require(
        app.voice_editor
            .as_ref()
            .is_some_and(|editor| editor.has_unsaved_changes()),
        "unsaved draft remains unsaved",
    )
}
fn explicit_check(app: &mut App, deadline: Instant) -> Check<VoiceDeviceObservation> {
    checked(
        app.window.probe_submit(v::CHECK_OUTPUT, FormData::new()),
        "real hidden empty-form output button",
    )?;
    pump(app, deadline)?;
    observation(app, &saved(app)?).ok_or_else(|| "voice-output-probe: query receipt missing".into())
}
fn workflow(
    app: &mut App,
    paths: &DataPaths,
    step: &mut usize,
    deadline: Instant,
) -> Check<(VoiceDeviceCheckResult, VoiceDeviceCheckResult)> {
    open(app, paths, deadline)?;
    require(app.screen == Screen::Setup, "fresh output-probe deployment")?;
    submit(
        app,
        SAVE,
        &[
            (SCHOOL, "Synthetic voice output school"),
            (LOGIN, LOGIN_NAME),
            (NAME, "Synthetic voice output administrator"),
            (PASSWORD, PASSWORD_VALUE),
            (CONFIRM, PASSWORD_VALUE),
        ],
        deadline,
    )?;
    require(app.screen == Screen::Code, "real native setup committed")?;
    submit(app, ACK, &[], deadline)?;
    if app.screen == Screen::Login {
        login(app, deadline)?;
    }
    require(
        app.screen == Screen::Home,
        "local global administrator ready",
    )?;
    mark(step, "native-setup-global-administrator")?;
    open_settings(app, deadline)?;
    mark(step, "native-open-voice-settings")?;
    let initial = saved(app)?;
    no_jobs(&initial)?;
    require(
        initial.enabled && observation(app, &initial).is_none(),
        "default enabled is not an output observation",
    )?;
    mark(step, "saved-default-enabled-no-jobs")?;
    submit(
        app,
        v::REFRESH,
        &[
            (v::DURATION_SECONDS, "2.5"),
            (v::PENDING_JOBS, "3"),
            (v::PENDING_PER_OWNER, "1"),
            (v::REPEAT_TOTAL, "2"),
        ],
        deadline,
    )?;
    draft_kept(app)?;
    same_saved(&initial, &saved(app)?)?;
    mark(step, "native-draft-refresh-no-write")?;
    let mut forbidden = FormData::new();
    checked(
        forbidden.insert(v::DURATION_SECONDS, "2.5".into()),
        "negative form",
    )?;
    require(
        app.window.probe_submit(v::CHECK_OUTPUT, forbidden).is_err(),
        "nonempty check rejected by actual native adapter",
    )?;
    pump(app, deadline)?;
    require(
        observation(app, &saved(app)?).is_none(),
        "rejected check has no receipt",
    )?;
    same_saved(&initial, &saved(app)?)?;
    draft_kept(app)?;
    mark(step, "native-nonempty-check-rejected")?;
    let first = explicit_check(app, deadline)?;
    require(
        first.is_fresh(),
        "actual query returned a fresh observation",
    )?;
    mark(step, "native-explicit-output-query")?;
    same_saved(&initial, &saved(app)?)?;
    draft_kept(app)?;
    mark(step, "query-keeps-saved-state-and-draft")?;
    submit(app, v::REFRESH, &[], deadline)?;
    require(
        observation(app, &saved(app)?) == Some(first.clone()),
        "explicit refresh retains exact original observation including Instant",
    )?;
    draft_kept(app)?;
    same_saved(&initial, &saved(app)?)?;
    mark(step, "native-refresh-retains-exact-observation")?;
    // Real monotonic 30-second TTL, no fake clock or renewed observation budget.
    while first.is_fresh() {
        pump(app, deadline)?;
        std::thread::sleep(Duration::from_millis(20));
    }
    pump(app, deadline)?;
    require(
        app.pending.is_none()
            && observation(app, &saved(app)?).is_none()
            && app
                .view()
                .description
                .contains("上次检查结果已过期，请重新检查。"),
        "ordinary local polling expires the receipt without an automatic query",
    )?;
    draft_kept(app)?;
    same_saved(&initial, &saved(app)?)?;
    mark(step, "real-thirty-second-observation-expiry")?;
    let second = explicit_check(app, deadline)?;
    require(
        second.is_fresh()
            && second != first
            && second.observed_monotonic_millis > first.observed_monotonic_millis,
        "only a second explicit query yields a new completion timestamp",
    )?;
    same_saved(&initial, &saved(app)?)?;
    draft_kept(app)?;
    mark(step, "native-explicit-recheck-new-observation")?;
    let old_lease = app.lease.ok_or("voice-output-probe: lease missing")?;
    submit(app, LOCK, &[], deadline)?;
    require(
        app.screen == Screen::Login && app.lease.is_none() && app.voice_editor.is_none(),
        "real lock discards owned editor and observation/draft",
    )?;
    mark(step, "native-lock-discards-observation-and-draft")?;
    login(app, deadline)?;
    require(
        app.lease != Some(old_lease),
        "relogin has a different exact lease",
    )?;
    mark(step, "native-relogin-new-lease")?;
    open_settings(app, deadline)?;
    let reopened = saved(app)?;
    same_saved(&initial, &reopened)?;
    require(
        observation(app, &reopened).is_none()
            && app
                .lease
                .is_some_and(|lease| !second.is_current_for(&reopened, lease))
            && app
                .voice_editor
                .as_ref()
                .is_some_and(|editor| !editor.has_unsaved_changes())
            && !app.view().description.contains("检查时间："),
        "new editor has no old observation or unsaved draft",
    )?;
    mark(step, "native-reopen-unchecked-original-settings")?;
    Ok((first.result, second.result))
}

pub(super) fn run() -> Check<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(120);
    let mut random = [0u8; 16];
    checked(SystemRandom.fill(&mut random), "output deployment nonce")?;
    let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-VoiceOutput-{}-{nonce}",
        std::process::id()
    ));
    let paths = checked(
        DataPaths::isolated_test(root.clone()),
        "output isolated paths",
    )?;
    let deployment = checked(
        paths.prepare_for_current_user(),
        "output protected deployment",
    )?;
    checked(deployment.read_back(), "output ACL readback")?;
    let instance = checked(
        InstanceGuard::acquire(paths.root()),
        "output instance guard",
    )?;
    let mut step = 0;
    let mut app = make_app(&paths, deadline)?;
    // Owned windows remain hidden and no foreground mutation is requested.
    // This probe does not sample or claim global foreground-window stability.
    let outcome = workflow(&mut app, &paths, &mut step, deadline);
    // Even an assertion failure must physically finish the original workers,
    // writer, shell, and audio owner before this deployment can be deleted.
    stop_owned_app(app, deadline)?;
    drop(instance);
    drop(deployment);
    checked(fs::remove_dir_all(&root), "remove closed output deployment")?;
    require(!root.exists(), "output deployment cleanup complete")?;
    let (first_result, second_result) = outcome?;
    mark(&mut step, "app-physical-shutdown")?;
    mark(&mut step, "isolated-deployment-cleanup")?;
    require(step == ORDER.len(), "all fixed output-probe steps complete")?;
    println!("VOICE_OUTPUT_NATIVE_PASS steps={step} shared_workers=2 first_result={first_result:?} second_result={second_result:?} ttl_expired=PASS audio_jobs=NONE physical_playback=NOT_EXERCISED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms={}", started.elapsed().as_millis());
    Ok(())
}
