use super::*;
use fc_application::identity::IdentitySessions;
use fc_bootstrap::backend::web::{PairingReview, WebDevice, WindowStatus};
use fc_domain::time::{LocalDate, LocalDateTime};

fn fixture(origin: TrustedOrigin) -> (IdentityState, SessionLease) {
    let now = LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 0).unwrap();
    let state = IdentityState::default()
        .transition(
            &IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "Synthetic school".into(),
                id: PrincipalId::new(1).unwrap(),
                login_name: "admin".into(),
                display_name: "Administrator".into(),
            },
            None,
            now,
        )
        .unwrap()
        .next;
    let lease = IdentitySessions::default()
        .issue_verified(
            &state.accounts[0],
            origin,
            0,
            1000,
            LocalDateTime::new(now.date, 1).unwrap(),
        )
        .unwrap();
    (state, lease)
}
fn review(revision: u64) -> PairingReview {
    PairingReview {
        request_id: 17,
        identity_revision: Revision::new(revision),
        principal_id: PrincipalId::new(1).unwrap(),
        login_name: "admin".into(),
        display_name: "Administrator".into(),
        device_name: "Synthetic tablet".into(),
        role: Role::GlobalAdmin,
        assignments: vec![],
        current_class: None,
        approved: false,
    }
}
fn snapshot() -> ManagementSnapshot {
    ManagementSnapshot {
        enabled: false,
        window: None,
        pending: vec![],
        devices: vec![],
    }
}
fn loaded(snapshot: ManagementSnapshot) -> Loaded {
    Loaded {
        snapshot,
        observed: Instant::now(),
        opened: None,
        changed: false,
    }
}
fn form(request: Option<u64>, device: Option<u64>) -> FormData {
    let mut form = FormData::new();
    if let Some(id) = request {
        form.insert(REQUEST, id.to_string()).unwrap();
    }
    if let Some(id) = device {
        form.insert(DEVICE, id.to_string()).unwrap();
    }
    form
}

#[test]
fn unavailable_transport_has_no_false_enable_and_rejects_forged_positive_actions() {
    let (identity, lease) = fixture(TrustedOrigin::Local);
    let mut editor = Editor::new(
        lease.principal_id(),
        Readiness::Unavailable,
        loaded(snapshot()),
    );
    let view = editor.view(&identity, "", false);
    view.validate().unwrap();
    assert!(view.description.contains(NOT_READY));
    for action in [ENABLE, BEGIN] {
        assert!(
            !view
                .actions
                .iter()
                .find(|a| a.id == action)
                .unwrap()
                .enabled
        );
        assert!(editor
            .handle(action, &FormData::new(), &identity, lease)
            .is_err());
    }
    assert!(!view.description.contains("HTTPS已启用"));
}

#[test]
fn approval_captures_original_revision_and_refresh_never_rebinds_the_selection() {
    let (identity, lease) = fixture(TrustedOrigin::Local);
    let mut current = snapshot();
    current.pending.push(review(9));
    let mut editor = Editor::new(
        lease.principal_id(),
        Readiness::SyntheticFixture,
        loaded(current.clone()),
    );
    let data = form(Some(17), None);
    editor.input(&data);
    assert!(matches!(editor.handle(APPROVE, &data, &identity, lease),
        Ok(Command::Run(Operation::Approve { request_id:17, expected })) if expected == Revision::new(9)));
    current.pending[0].identity_revision = Revision::new(10);
    editor.loaded(loaded(current));
    assert!(editor.handle(APPROVE, &data, &identity, lease).is_err());
    assert!(editor.selected_request.is_none());
    editor.input(&data);
    assert!(matches!(editor.handle(APPROVE, &data, &identity, lease),
        Ok(Command::Run(Operation::Approve { expected, .. })) if expected == Revision::new(10)));
}

#[test]
fn pairing_code_is_not_in_view_and_consumption_expiry_and_leave_clear_sensitive_state() {
    let (identity, lease) = fixture(TrustedOrigin::Local);
    let mut current = snapshot();
    current.enabled = true;
    current.window = Some(WindowStatus {
        id: 4,
        remaining_millis: 1000,
        code_available: true,
    });
    let mut start = loaded(current.clone());
    start.opened = Some(PairingWindow {
        id: 4,
        code: PairingCode::parse("01234567".into()).unwrap(),
        expires_in_millis: 1000,
    });
    let mut editor = Editor::new(lease.principal_id(), Readiness::SyntheticFixture, start);
    assert_eq!(editor.code_text().unwrap().as_str(), "01234567");
    let view = editor.view(&identity, "", false);
    assert!(view
        .fields
        .iter()
        .find(|f| f.id == CODE)
        .unwrap()
        .value
        .is_empty());
    assert!(!format!("{view:?}").contains("01234567"));
    current.window.as_mut().unwrap().code_available = false;
    current.pending.push(review(1));
    editor.loaded(loaded(current.clone()));
    assert!(editor.code_text().is_none());
    assert!(editor.tick(Instant::now() + Duration::from_secs(2)));
    assert!(editor.snapshot.pending.is_empty());
    assert!(editor.snapshot.window.is_none());
    editor.snapshot.pending.push(review(1));
    editor.clear_sensitive();
    assert!(editor.snapshot.pending.is_empty());
    assert!(editor.code_text().is_none());
}

#[test]
fn device_confirmation_keeps_exact_id_and_cannot_be_redirected_by_new_form_values() {
    let (identity, lease) = fixture(TrustedOrigin::Local);
    let mut current = snapshot();
    current.devices = [21, 22]
        .into_iter()
        .map(|id| WebDevice {
            device_id: id,
            principal_id: lease.principal_id(),
            device_name: format!("device{id}"),
        })
        .collect();
    let mut editor = Editor::new(
        lease.principal_id(),
        Readiness::Unavailable,
        loaded(current.clone()),
    );
    let original = form(None, Some(21));
    editor.input(&original);
    assert!(matches!(
        editor.handle(REVOKE, &original, &identity, lease),
        Ok(Command::Stay)
    ));
    let changed = form(None, Some(22));
    editor.input(&changed);
    assert!(matches!(
        editor.handle(CONFIRM, &changed, &identity, lease),
        Ok(Command::Run(Operation::Revoke(21)))
    ));
    editor.input(&original);
    editor.handle(REVOKE, &original, &identity, lease).unwrap();
    current.devices.retain(|d| d.device_id != 21);
    editor.loaded(loaded(current));
    assert!(editor.handle(CONFIRM, &changed, &identity, lease).is_err());
    editor.view(&identity, "", false).validate().unwrap();
}

#[test]
fn only_the_original_local_global_manage_owner_can_use_the_editor() {
    let (mut identity, lease) = fixture(TrustedOrigin::Local);
    let mut editor = Editor::new(
        lease.principal_id(),
        Readiness::Unavailable,
        loaded(snapshot()),
    );
    identity.accounts[0].principal.role = Role::TeacherAdmin;
    assert!(editor
        .handle(REFRESH, &FormData::new(), &identity, lease)
        .is_err());
    identity.accounts[0].principal.role = Role::GlobalAdmin;
    identity.accounts[0].must_change_password = true;
    assert!(editor
        .handle(REFRESH, &FormData::new(), &identity, lease)
        .is_err());
    let (identity, web) = fixture(TrustedOrigin::Web);
    assert!(editor
        .handle(REFRESH, &FormData::new(), &identity, web)
        .is_err());
    let mut identity = identity;
    let mut other = identity.accounts[0].clone();
    other.principal.id = PrincipalId::new(2).unwrap();
    other.login_name = "other".into();
    identity.accounts.push(other.clone());
    let other = IdentitySessions::default()
        .issue_verified(
            &other,
            TrustedOrigin::Local,
            0,
            1000,
            LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 1).unwrap(),
        )
        .unwrap();
    assert!(editor
        .handle(REFRESH, &FormData::new(), &identity, other)
        .is_err());
}

#[test]
fn all_teaching_scopes_are_paged_without_changing_the_captured_approval() {
    let (identity, lease) = fixture(TrustedOrigin::Local);
    let mut pending = review(9);
    pending.assignments = (1..=17)
        .map(|id| TeachingAssignment {
            teacher_id: lease.principal_id(),
            class_id: ClassId::new(id).unwrap(),
            subject_id: SubjectId::new(id).unwrap(),
            enabled: true,
        })
        .collect();
    let mut current = snapshot();
    current.pending.push(pending);
    let mut editor = Editor::new(
        lease.principal_id(),
        Readiness::SyntheticFixture,
        loaded(current),
    );
    let data = form(Some(17), None);
    editor.input(&data);
    for _ in 0..20 {
        editor.handle(NEXT_SCOPE, &data, &identity, lease).unwrap();
    }
    let view = editor.view(&identity, "", false);
    view.validate().unwrap();
    assert!(
        !view
            .actions
            .iter()
            .find(|a| a.id == NEXT_SCOPE)
            .unwrap()
            .enabled
    );
    let grid = view.grid.unwrap();
    assert!(grid
        .rows
        .iter()
        .any(|r| r.cells.iter().any(|s| s.contains("第3/3页"))));
    assert!(grid
        .rows
        .iter()
        .any(|r| r.cells.iter().any(|s| s == "班级17 / 科目17")));
    assert!(matches!(editor.handle(APPROVE, &data, &identity, lease),
        Ok(Command::Run(Operation::Approve { request_id:17, expected })) if expected == Revision::new(9)));
}

fn native_events(app: &mut App) {
    for event in app.window.take_events().unwrap() {
        app.event(event).unwrap();
    }
}
fn native_form(app: &App) -> FormData {
    let mut data = FormData::new();
    for field in app.view().fields {
        if field.kind != FieldKind::ReadOnly {
            data.insert(field.id, field.value).unwrap();
        }
    }
    data
}
fn native_action(app: &mut App, action: ActionId, data: FormData) {
    let data = super::super::prepare_probe_form(app, action, data).unwrap();
    app.window.probe_submit(action, data).unwrap();
    native_events(app);
    super::super::tests::wait_for(app, |app| app.pending.is_none());
}
fn native_click(app: &mut App, action: ActionId) {
    native_action(app, action, native_form(app));
}
fn native_grant(app: &mut App, name: &'static str) -> fc_bootstrap::backend::web::IssuedWebSession {
    native_click(app, BEGIN);
    let code = app.web_editor.as_ref().unwrap().code_text().unwrap();
    assert!(!format!("{:?}", app.view()).contains(code.as_str()));
    assert!(app.window.draft_nonsecret().unwrap().get(CODE).is_none());
    let code = PairingCode::parse(code.as_str().to_owned()).unwrap();
    let service = app.web_service.as_ref().unwrap().clone();
    // A separate synthetic teacher client, not an application UI or global input.
    let receipt = std::thread::spawn(move || service.request_pairing(code, "admin", name))
        .join()
        .unwrap()
        .unwrap();
    native_click(app, REFRESH);
    assert!(app.web_editor.as_ref().unwrap().code_text().is_none());
    app.window
        .probe_choice(REQUEST, &receipt.request_id.to_string())
        .unwrap();
    native_events(app);
    assert!(app.view().grid.is_some());
    native_click(app, APPROVE);
    let service = app.web_service.as_ref().unwrap().clone();
    match std::thread::spawn(move || service.poll(receipt.request_id, &receipt.poll_secret))
        .join()
        .unwrap()
        .unwrap()
    {
        fc_bootstrap::backend::web::PairingPoll::Granted(session) => session,
        fc_bootstrap::backend::web::PairingPoll::Pending => {
            panic!("selected native approval was not committed")
        }
    }
}

#[test]
fn native_teacher_device_page_uses_the_shared_worker_and_exact_confirmed_revocations() {
    let _serial = super::super::tests::SERIAL.lock().unwrap();
    let (_scratch, mut app) = super::super::tests::fixture();
    super::super::tests::wait_for(&mut app, |app| app.pending.is_none());
    let mut setup = FormData::new();
    for (id, value) in [
        (SCHOOL, "Synthetic native pairing school"),
        (LOGIN, "admin"),
        (NAME, "Native administrator"),
        (PASSWORD, "Native-pairing-password-2026"),
        (super::super::CONFIRM, "Native-pairing-password-2026"),
    ] {
        setup.insert(id, value.into()).unwrap();
    }
    native_action(&mut app, SAVE, setup);
    native_click(&mut app, ACK);
    let mut login = FormData::new();
    login.insert(LOGIN, "admin".into()).unwrap();
    login
        .insert(PASSWORD, "Native-pairing-password-2026".into())
        .unwrap();
    native_action(&mut app, LOG_IN, login);
    app.view().validate().unwrap();
    assert!(app.view().actions.iter().any(|a| a.id == TO_WEB));
    assert!(!app.view().actions.iter().any(|a| a.id == TO_ROTATE));
    native_click(&mut app, TO_PASSWORD);
    assert!(app.view().actions.iter().any(|a| a.id == TO_ROTATE));
    native_click(&mut app, TO_ROTATE);
    assert!(app.screen == Screen::Rotate);
    native_click(&mut app, BACK);
    native_click(&mut app, TO_WEB);
    assert!(app.screen == Screen::Web);
    assert!(app.view().description.contains(NOT_READY));
    assert!(
        !app.view()
            .actions
            .iter()
            .find(|a| a.id == ENABLE)
            .unwrap()
            .enabled
    );
    let service = app.web_service.as_ref().unwrap().clone();
    let admin = app.lease.unwrap();
    // The worker independently rejects forged positive operations despite bypassed UI.
    app.submit(Work::Web(
        service,
        admin,
        Operation::Enable,
        Readiness::Unavailable,
    ))
    .unwrap();
    super::super::tests::wait_for(&mut app, |app| app.pending.is_none());
    assert!(!app.web_editor.as_ref().unwrap().snapshot.enabled);
    native_click(&mut app, BACK);
    super::super::tests::wait_for(&mut app, |app| {
        app.close_web.is_none() && app.web_close_job.is_none()
    });
    app.web_readiness = Readiness::SyntheticFixture;
    native_click(&mut app, TO_WEB);
    native_click(&mut app, ENABLE);
    let service = app.web_service.as_ref().unwrap().clone();
    app.submit(Work::Web(
        service,
        admin,
        Operation::Begin,
        Readiness::Unavailable,
    ))
    .unwrap();
    super::super::tests::wait_for(&mut app, |app| app.pending.is_none());
    assert!(app.web_editor.as_ref().unwrap().snapshot.window.is_none());
    let first = native_grant(&mut app, "first synthetic tablet");
    let second = native_grant(&mut app, "second synthetic tablet");
    native_click(&mut app, REFRESH);
    assert_eq!(app.web_editor.as_ref().unwrap().snapshot.devices.len(), 2);
    app.window
        .probe_choice(DEVICE, &first.device.device_id.to_string())
        .unwrap();
    native_events(&mut app);
    native_click(&mut app, REVOKE);
    assert!(app.web_editor.as_ref().unwrap().confirmation.is_some());
    native_click(&mut app, CONFIRM);
    assert_eq!(app.web_editor.as_ref().unwrap().snapshot.devices.len(), 1);
    let service = app.web_service.as_ref().unwrap().clone();
    std::thread::spawn(move || {
        assert!(service.authenticate(&first.cookie).is_err());
        assert!(service.authenticate(&second.cookie).is_ok());
    })
    .join()
    .unwrap();
    native_click(&mut app, DISABLE);
    native_click(&mut app, CONFIRM);
    assert!(!app.web_editor.as_ref().unwrap().snapshot.enabled);
    assert!(app.web_editor.as_ref().unwrap().snapshot.devices.is_empty());
    native_click(&mut app, ENABLE);
    native_click(&mut app, BEGIN);
    assert!(app.web_editor.as_ref().unwrap().code_text().is_some());
    native_click(&mut app, BACK);
    assert!(app.web_editor.is_none());
    super::super::tests::wait_for(&mut app, |app| {
        app.close_web.is_none() && app.web_close_job.is_none()
    });
    native_click(&mut app, TO_WEB);
    assert!(app.web_editor.as_ref().unwrap().snapshot.window.is_none());
    native_click(&mut app, BEGIN);
    assert!(app.web_editor.as_ref().unwrap().code_text().is_some());
    native_click(&mut app, LOCK);
    assert!(app.web_editor.is_none());
    assert!(app.lease.is_none());
    super::super::tests::drain(app);
}

#[test]
fn local_https_navigation_does_not_enable_unavailable_teacher_connections_or_expand_action_bound() {
    let (state, lease) = fixture(TrustedOrigin::Local);
    let mut e = Editor::new(
        lease.principal_id(),
        Readiness::Unavailable,
        loaded(snapshot()),
    );
    let view = e.view(&state, "", false);
    view.validate().unwrap();
    assert!(view
        .actions
        .iter()
        .any(|a| a.id == HTTPS_SETTINGS && a.enabled));
    assert!(matches!(
        e.handle(HTTPS_SETTINGS, &FormData::new(), &state, lease)
            .unwrap(),
        Command::OpenHttpsSettings
    ));
    for id in [ENABLE, BEGIN] {
        assert!(!view.actions.iter().find(|a| a.id == id).unwrap().enabled);
    }
}
