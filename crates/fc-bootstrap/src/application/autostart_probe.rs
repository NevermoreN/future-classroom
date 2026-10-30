//! Hidden native workflow: protected synthetic SQLite plus a generated HKCU
//! scratch leaf. Never modifies the real Run key or launches a startup program.
use super::*;
use autostart_editor as a;
use fc_domain::autostart::{AutostartObservation, AutostartPhase, AutostartState};
use fc_ports::autostart::AutostartSystem;
use fc_windows::system::RunScope;

type Check<T> = std::result::Result<T, String>;
pub(super) const STEPS: usize = 25;
const FIXTURE_PASSWORD: &str = "Synthetic autostart probe 2026!";
fn checked<T, E>(value: std::result::Result<T, E>, stage: &str) -> Check<T> {
    value.map_err(|_| format!("autostart-probe: {stage}"))
}
fn require(value: bool, stage: &str) -> Check<()> {
    if value {
        Ok(())
    } else {
        Err(format!("autostart-probe: {stage}"))
    }
}
fn password() -> Check<Password> {
    checked(Password::new(FIXTURE_PASSWORD.into()), "synthetic password")
}
fn state(app: &App) -> Check<AutostartState> {
    let backend = app
        .backend
        .as_ref()
        .ok_or("autostart-probe: missing backend")?;
    checked(
        checked(backend.lock(), "backend lock")?
            .autostart_status(app.lease.ok_or("autostart-probe: missing lease")?),
        "read factual autostart state",
    )
}
fn pump(app: &mut App, deadline: Instant) -> Check<()> {
    loop {
        for event in checked(app.window.take_events(), "take native events")? {
            app.event(event)?;
        }
        app.poll()?;
        require(
            !app.pool.is_faulted() && !app.stopping && !app.terminate_required,
            "healthy shared workers",
        )?;
        if app.pending.is_none() && app.autostart_job.is_none() {
            return Ok(());
        }
        require(Instant::now() < deadline, "native workflow deadline")?;
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn submit(
    app: &mut App,
    action: ActionId,
    overrides: &[(FieldId, &str)],
    deadline: Instant,
) -> Check<()> {
    require(Instant::now() < deadline, "submission deadline")?;
    let mut form = FormData::new();
    for field in app.view().fields.into_iter().filter(|_| action != LOCK) {
        if field.kind != FieldKind::ReadOnly {
            let value = overrides
                .iter()
                .find(|(id, _)| *id == field.id)
                .map_or(field.value, |(_, value)| (*value).into());
            checked(form.insert(field.id, value), "native form values")?;
        }
    }
    checked(
        app.window.probe_submit(action, form),
        "native command admission",
    )?;
    pump(app, deadline)
}
fn mark(step: &mut usize, label: &str) {
    println!("AUTOSTART_NATIVE_STEP index={} name={label}", *step);
    *step += 1;
}
fn review(app: &mut App, desired: bool, deadline: Instant) -> Check<()> {
    submit(
        app,
        if desired { a::ENABLE } else { a::DISABLE },
        &[],
        deadline,
    )?;
    require(
        app.autostart
            .as_ref()
            .is_some_and(|e| e.has_unsaved_changes()),
        "owned explicit review",
    )
}
fn confirm(app: &mut App, deadline: Instant) -> Check<()> {
    submit(app, a::CONFIRM, &[(a::ACK, "1")], deadline)
}
fn success(app: &App, scope: &RunScope, desired: bool) -> Check<()> {
    let saved = state(app)?;
    require(
        saved.desired == desired
            && saved
                .current()
                .is_some_and(|op| op.phase == AutostartPhase::Succeeded),
        "durable successful operation",
    )?;
    require(
        saved.observed.matches(desired, scope.expected_command()),
        "saved actual observation",
    )?;
    require(
        checked(scope.open(), "open scratch observer")?
            .observe()
            .matches(desired, scope.expected_command()),
        "fresh physical scratch observation",
    )
}
fn exercise(app: &mut App, scope: &RunScope, deadline: Instant) -> Check<usize> {
    let mut steps = 0;
    submit(app, TO_DISPLAY, &[], deadline)?;
    submit(app, TO_AUTOSTART, &[], deadline)?;
    require(app.screen == Screen::Autostart, "native settings screen")?;
    mark(&mut steps, "native-open-settings");
    let initial = state(app)?;
    require(
        scope.is_scratch()
            && initial.history.is_empty()
            && initial.observed == AutostartObservation::Absent
            && app.autostart.as_ref().is_some_and(|editor| {
                let loaded = editor.loaded();
                loaded.state.as_ref() == Some(&initial)
                    && loaded.observed == initial.observed
                    && loaded.target_command == scope.expected_command()
            }),
        "native editor reflects real writer and scratch observation without an intent",
    )?;
    mark(&mut steps, "generated-scratch-and-no-startup-intent");
    review(app, true, deadline)?;
    require(
        state(app)? == initial
            && checked(scope.open(), "scratch observer")?.observe() == AutostartObservation::Absent,
        "review itself has no effect",
    )?;
    mark(&mut steps, "native-enable-review-no-effect");
    submit(app, a::CANCEL, &[], deadline)?;
    require(state(app)? == initial, "cancel has no intent")?;
    mark(&mut steps, "native-cancel-no-intent");
    review(app, true, deadline)?;
    mark(&mut steps, "native-enable-review-again");
    confirm(app, deadline)?;
    success(app, scope, true)?;
    mark(&mut steps, "native-confirm-enable-real-readback");
    let enabled = state(app)?;
    submit(app, a::REFRESH, &[], deadline)?;
    require(
        state(app)?.history == enabled.history,
        "refresh never creates an operation",
    )?;
    mark(&mut steps, "native-refresh-is-observation-only");
    review(app, false, deadline)?;
    mark(&mut steps, "native-disable-review");
    submit(app, LOCK, &[], deadline)?;
    require(
        app.lease.is_none() && app.autostart.is_none() && app.screen == Screen::Login,
        "lock discards owner confirmation",
    )?;
    require(
        checked(scope.open(), "scratch observer")?
            .observe()
            .matches(true, scope.expected_command()),
        "lock does not undo successful startup intent",
    )?;
    mark(&mut steps, "native-lock-clears-pending-review");
    submit(
        app,
        LOG_IN,
        &[(LOGIN, "autostart-probe"), (PASSWORD, FIXTURE_PASSWORD)],
        deadline,
    )?;
    require(
        app.screen == Screen::Home && app.autostart.is_none(),
        "real local relogin",
    )?;
    mark(&mut steps, "native-relogin");
    submit(app, TO_DISPLAY, &[], deadline)?;
    submit(app, TO_AUTOSTART, &[], deadline)?;
    mark(&mut steps, "native-reopen-saved-state");
    review(app, false, deadline)?;
    mark(&mut steps, "native-disable-review-again");
    confirm(app, deadline)?;
    success(app, scope, false)?;
    mark(&mut steps, "native-confirm-disable-real-readback");
    review(app, true, deadline)?;
    let stale = state(app)?;
    mark(&mut steps, "native-capture-review-revision");
    {
        let mut external = checked(scope.open(), "scratch external writer")?;
        let outcome = external.apply(&AutostartObservation::Absent, true, &|| true);
        require(
            outcome.failure.is_none(),
            "synthetic external scratch change",
        )?;
        let mut backend = checked(
            app.backend
                .as_ref()
                .ok_or("autostart-probe: missing backend")?
                .lock(),
            "backend lock",
        )?;
        checked(
            backend.reconcile_autostart(stale.revision, outcome.observation),
            "startup-style factual reconciliation",
        )?;
    }
    let changed = state(app)?;
    require(
        changed.revision != stale.revision && changed.history == stale.history && changed.drifted(),
        "external drift changes facts without adding intent",
    )?;
    mark(&mut steps, "external-drift-observed-without-repair");
    confirm(app, deadline)?;
    require(
        state(app)? == changed
            && checked(scope.open(), "scratch observer")?
                .observe()
                .matches(true, scope.expected_command()),
        "stale review rejected without a registry effect",
    )?;
    mark(&mut steps, "native-stale-review-rejected");
    submit(app, a::REFRESH, &[], deadline)?;
    require(
        !app.autostart
            .as_ref()
            .is_some_and(|e| e.has_unsaved_changes()),
        "fresh observation clears stale confirmation",
    )?;
    mark(&mut steps, "native-refresh-current-drift");
    review(app, false, deadline)?;
    mark(&mut steps, "native-review-disable-drift");
    confirm(app, deadline)?;
    success(app, scope, false)?;
    mark(&mut steps, "native-confirm-disable-drift");
    {
        // Simulate interruption after a physical effect by intentionally omitting
        // Finish. This is a real writer ticket and real scratch registry write.
        let saved = state(app)?;
        let lease = app.lease.ok_or("autostart-probe: missing lease")?;
        let mut backend = checked(
            app.backend
                .as_ref()
                .ok_or("autostart-probe: missing backend")?
                .lock(),
            "backend lock",
        )?;
        let prepared = checked(
            backend.begin_autostart_while(
                lease,
                saved.revision,
                true,
                scope.expected_command().into(),
                saved.observed,
                Arc::new(|| true),
            ),
            "prepare interrupted operation",
        )?;
        checked(
            backend.mark_autostart_applying_while(lease, prepared.ticket, Arc::new(|| true)),
            "mark interrupted operation applying",
        )?;
        let mut external = checked(scope.open(), "scratch effect")?;
        require(
            external
                .apply(&AutostartObservation::Absent, true, &|| true)
                .failure
                .is_none(),
            "interrupted scratch physical effect",
        )?;
    }
    mark(&mut steps, "simulate-interruption-after-physical-effect");
    app.submit_autostart(fc_bootstrap::autostart::Operation::ObserveStartup)?;
    pump(app, deadline)?;
    let matched = state(app)?;
    require(
        matched
            .current()
            .is_some_and(|op| op.phase == AutostartPhase::Succeeded)
            && matched.observed.matches(true, scope.expected_command()),
        "startup confirms the actual matching target without replaying an effect",
    )?;
    {
        // A second intent stops before Applying. A fresh observation must not
        // claim its disabled target or perform the missing registry operation.
        let lease = app.lease.ok_or("autostart-probe: missing lease")?;
        let mut backend = checked(
            app.backend
                .as_ref()
                .ok_or("autostart-probe: missing backend")?
                .lock(),
            "backend lock",
        )?;
        checked(
            backend.begin_autostart_while(
                lease,
                matched.revision,
                false,
                scope.expected_command().into(),
                matched.observed,
                Arc::new(|| true),
            ),
            "prepare interrupted pre-effect disable intent",
        )?;
    }
    app.submit_autostart(fc_bootstrap::autostart::Operation::ObserveStartup)?;
    pump(app, deadline)?;
    let reconciled = state(app)?;
    require(
        reconciled
            .current()
            .is_some_and(|op| op.phase == AutostartPhase::NeedsReconcile)
            && reconciled.observed.matches(true, scope.expected_command())
            && !reconciled.desired,
        "startup retains actual enabled fact and marks unexecuted disable for reconciliation",
    )?;
    mark(&mut steps, "startup-observe-interrupted-facts");
    submit(app, a::REFRESH, &[], deadline)?;
    require(
        state(app)?.history == reconciled.history
            && checked(scope.open(), "scratch observer")?
                .observe()
                .matches(true, scope.expected_command()),
        "refresh neither replays nor rolls back interrupted effect",
    )?;
    mark(
        &mut steps,
        "native-refresh-never-replays-interrupted-intent",
    );
    review(app, false, deadline)?;
    mark(&mut steps, "native-review-explicit-recovery");
    confirm(app, deadline)?;
    success(app, scope, false)?;
    mark(&mut steps, "native-confirm-explicit-recovery");
    submit(app, a::CLOSE, &[], deadline)?;
    require(app.screen == Screen::Home, "native return home")?;
    mark(&mut steps, "native-close-settings");
    require(steps == STEPS, "complete ordered workflow")?;
    Ok(steps)
}

pub(super) fn run() -> Check<()> {
    let started = Instant::now();
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-Autostart-{}-{}",
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
    let instance = checked(InstanceGuard::acquire(paths.root()), "isolated instance")?;
    let scope = checked(RunScope::scratch(), "generated scratch scope")?;
    let clock = Arc::new(CurrentClock::new());
    let codec = checked(
        CredentialRecordCodec::generate(&mut SystemRandom),
        "isolated credential codec",
    )?;
    let mut backend = checked(
        Backend::open(paths.database(), codec, clock.clone(), clock.clone()),
        "open synthetic database",
    )?;
    let mut app = create_app(clock, true)?;
    app.root = root.clone();
    app.desktop.set_root(root.clone());
    app.autostart_scope = Some(scope.clone());
    app.next_content_maintenance = started + Duration::from_secs(3600);
    let setup: Check<()> = (|| {
        drop(checked(
            backend.initialize(
                Initialize {
                    school_name: "Synthetic autostart school".into(),
                    login_name: "autostart-probe".into(),
                    display_name: "Synthetic autostart administrator".into(),
                },
                password()?,
            ),
            "initialize synthetic administrator",
        )?);
        let lease = checked(
            backend.login("autostart-probe", password()?),
            "real local login",
        )?;
        app.state = Arc::new(checked(backend.state(), "real identity state")?);
        app.lease = Some(lease);
        let control = backend.session_control();
        let _ = app.control_slot.set(control.clone());
        app.control = Some(control);
        app.screen = Screen::Home;
        Ok(())
    })();
    app.backend = Some(Arc::new(Mutex::new(backend)));
    let result = setup.and_then(|()| {
        app.render()?;
        exercise(&mut app, &scope, started + Duration::from_secs(70))
    });
    app.begin_shutdown();
    let closed: Check<()> = (|| {
        while !app.pool.is_finished() {
            app.poll()?;
            require(
                Instant::now() < started + Duration::from_secs(90),
                "physical shutdown deadline; retain test roots for diagnosis",
            )?;
            std::thread::sleep(Duration::from_millis(5));
        }
        checked(app.pool.join(), "join shared workers")?;
        Ok(())
    })();
    drop(instance);
    drop(deployment);
    closed?;
    checked(
        scope.cleanup(),
        "remove only owned scratch leaf after workers returned",
    )?;
    checked(
        fs::remove_dir_all(&root),
        "remove closed isolated deployment",
    )?;
    let steps = result?;
    println!("autostart_intent_readback_confirm_cancel_lock_stale_drift_reconcile_native_worker=PASS autostart_native_steps={steps} real_run=UNTOUCHED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms={}", started.elapsed().as_millis());
    Ok(())
}
