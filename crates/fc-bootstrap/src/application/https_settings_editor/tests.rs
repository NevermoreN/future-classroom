use super::*;
use fc_domain::https_settings::HttpsSettings;

fn owner(id: u64) -> Owner {
    Owner {
        school: SchoolId::new(1).unwrap(),
        principal: PrincipalId::new(id).unwrap(),
    }
}
fn base(revision: u64) -> HttpsSettingsState {
    HttpsSettingsState {
        identity_revision: Revision::new(12),
        settings: HttpsSettings {
            revision: Revision::new(revision),
            configuration: None,
        },
    }
}
fn configured(revision: u64) -> HttpsSettingsState {
    let mut state = base(revision);
    state.settings.configuration = Some(
        HttpsConfiguration::new(
            HttpsHost::parse("localhost").unwrap(),
            8443,
            HttpsCertificateRef::parse(&"ab".repeat(20)).unwrap(),
        )
        .unwrap(),
    );
    state
}
fn form(host: &str, port: &str) -> FormData {
    let mut data = FormData::new();
    data.insert(HOST, host.into()).unwrap();
    data.insert(PORT, port.into()).unwrap();
    data.insert(CERTIFICATE, "ab".repeat(20)).unwrap();
    data.insert(SOURCE_NETWORKS, "127.0.0.1/32".into()).unwrap();
    data
}
fn edit() -> Editor {
    let mut e = Editor::new(owner(1));
    e.loaded(base(0));
    e
}
fn id(value: u64) -> CommandId {
    CommandId::new(value).unwrap()
}

#[test]
fn no_configuration_has_blank_port_and_no_fake_certificate_or_listener_readiness() {
    let e = edit();
    let view = e.view(owner(1), "", false);
    view.validate().unwrap();
    assert_eq!(view.fields.iter().find(|f| f.id == PORT).unwrap().value, "");
    assert!(view.description.contains("不启动网络监听"));
    assert!(
        !view
            .actions
            .iter()
            .find(|a| a.id == CHECK_CERTIFICATE)
            .unwrap()
            .enabled
    );
    assert!(view.grid.is_none());
}
#[test]
fn parsers_refuse_url_zero_overflow_and_an_extra_actor_or_revision_field() {
    for (host, port) in [
        ("https://board.school", "8443"),
        ("board.school", "0"),
        ("board.school", "65536"),
        ("board.school", "+443"),
    ] {
        let mut e = edit();
        assert!(e.handle(owner(1), SAVE_CONFIG, form(host, port)).is_err());
        assert!(e.original.is_none());
    }
    let mut data = form("board.school", "8443");
    data.insert(FieldId(999), "lease/revision".into()).unwrap();
    assert!(edit().capture(data).is_err());
}
#[test]
fn save_captures_both_loaded_versions_before_admission_and_busy_keeps_id_and_deadline() {
    let mut e = edit();
    e.capture(form("Board.School", "8443")).unwrap();
    let now = Instant::now();
    let first = e.prepare_save(id(8), now).unwrap();
    let until = e.save_deadline().unwrap();
    assert_eq!(first.expected_revision, Revision::new(0));
    assert_eq!(first.expected_identity_revision, Revision::new(12));
    assert_eq!(
        first.configuration.as_ref().unwrap().host.as_str(),
        "board.school"
    );
    assert_eq!(
        e.prepare_save(id(9), now + Duration::from_secs(1)).unwrap(),
        first
    );
    assert_eq!(e.save_deadline(), Some(until));
    assert!(e.query_request().is_none());
    assert!(e.prepare_save(id(10), until).is_err());
    assert_eq!(e.original.as_ref().unwrap().request, first);
}
#[test]
fn accepted_write_never_replays_save_and_query_does_not_read_mutated_form_payload() {
    let mut e = edit();
    e.capture(form("localhost", "8443")).unwrap();
    let original = e.prepare_save(id(11), Instant::now()).unwrap();
    e.accepted();
    e.capture(form("changed.school", "9443")).unwrap();
    assert_eq!(e.query_request(), Some(original.clone()));
    assert!(e.handle(owner(1), SAVE_CONFIG, FormData::new()).is_err());
    assert!(e.prepare_save(id(12), Instant::now()).is_err());
    assert_eq!(e.query_request(), Some(original));
}
#[test]
fn query_none_preserves_draft_original_and_never_says_not_saved() {
    let mut e = edit();
    e.capture(form("localhost", "8443")).unwrap();
    let original = e.prepare_save(id(13), Instant::now()).unwrap();
    e.accepted();
    let draft = e.draft.clone();
    e.queried(None);
    let view = e.view(owner(1), "", false);
    assert!(view.description.contains("未查到原操作结果"));
    assert!(!view.description.contains("未保存"));
    assert!(e.draft == draft);
    assert_eq!(e.query_request(), Some(original));
    assert!(e.has_unknown());
    assert!(
        !view
            .actions
            .iter()
            .find(|a| a.id == SAVE_CONFIG)
            .unwrap()
            .enabled
    );
    assert!(view.actions.iter().find(|a| a.id == QUERY).unwrap().enabled);
}
#[test]
fn historical_receipt_does_not_become_current_cas_or_erase_draft() {
    let mut e = edit();
    e.capture(form("localhost", "8443")).unwrap();
    let original = e.prepare_save(id(14), Instant::now()).unwrap();
    e.accepted();
    let historical = configured(17);
    e.queried(Some(historical));
    assert_eq!(e.base.as_ref().unwrap().settings.revision, Revision::new(0));
    assert_eq!(e.query_request(), Some(original));
    assert!(e
        .view(owner(1), "", false)
        .description
        .contains("历史记录：已保存为版本17"));
    assert!(e.certificate_state().is_none());
}
#[test]
fn cancel_and_reload_require_explicit_risk_choice_and_cancel_never_constructs_write() {
    let mut e = edit();
    e.capture(form("localhost", "8443")).unwrap();
    assert!(matches!(
        e.handle(owner(1), CANCEL_DRAFT, FormData::new()).unwrap(),
        Command::Stay
    ));
    assert!(e.original.is_none());
    assert!(e.confirmation.is_some());
    assert!(matches!(
        e.handle(owner(1), KEEP, FormData::new()).unwrap(),
        Command::Stay
    ));
    assert!(e.dirty());
    let request = e.prepare_save(id(15), Instant::now()).unwrap();
    e.accepted();
    assert!(matches!(
        e.handle(owner(1), RELOAD, FormData::new()).unwrap(),
        Command::Stay
    ));
    assert_eq!(e.query_request(), Some(request));
    assert!(matches!(
        e.handle(owner(1), CONFIRM_DISCARD, FormData::new())
            .unwrap(),
        Command::Reload
    ));
    assert!(e.original.is_none());
    assert_eq!(e.base.as_ref().unwrap().settings.revision, Revision::new(0));
}
#[test]
fn another_owner_sees_no_draft_or_receipt_and_cannot_query_or_rebind_it() {
    let mut e = edit();
    e.capture(form("private-board.school", "8443")).unwrap();
    let request = e.prepare_save(id(16), Instant::now()).unwrap();
    e.accepted();
    e.queried(Some(configured(7)));
    let view = e.view(owner(2), "", false);
    view.validate().unwrap();
    assert!(view.fields.is_empty());
    assert!(view.grid.is_none());
    assert!(!view.description.contains("private-board"));
    assert!(!view.description.contains("版本7"));
    assert!(e.handle(owner(2), QUERY, FormData::new()).is_err());
    assert_eq!(e.query_request(), Some(request));
    assert!(matches!(
        e.handle(owner(2), CONFIRM_DISCARD, FormData::new())
            .unwrap(),
        Command::Reload
    ));
    assert!(e.owns(owner(2)));
    assert!(e.query_request().is_none());
}
#[test]
fn locking_retains_original_capsule_without_permission_to_replay_it() {
    let mut e = edit();
    e.capture(form("localhost", "8443")).unwrap();
    let r = e.prepare_save(id(17), Instant::now()).unwrap();
    e.accepted();
    e.suspended();
    assert_eq!(e.query_request(), Some(r));
    assert!(e.dirty());
    assert!(e.prepare_save(id(18), Instant::now()).is_err());
}
#[test]
fn certificate_inspection_is_explicit_saved_clean_only_and_never_a_readiness_action() {
    let mut e = edit();
    assert!(e.certificate_state().is_none());
    e.loaded(configured(3));
    assert!(matches!(
        e.handle(owner(1), CHECK_CERTIFICATE, FormData::new())
            .unwrap(),
        Command::CheckCertificate
    ));
    e.capture(form("changed.school", "8443")).unwrap();
    assert!(e.certificate_state().is_none());
    assert!(e
        .handle(owner(1), CHECK_CERTIFICATE, FormData::new())
        .is_err());
    let mut report = CertificateQueryReport::default();
    report.found.check = CertificateCheck::Satisfied;
    let grid = certificate_grid(&report);
    grid.validate().unwrap();
    assert_eq!(grid.rows.len(), 7);
    assert_eq!(grid.rows[0].cells[0], "检查时满足");
    assert_eq!(grid.rows[1].cells[0], "未检查");
    assert!(grid.columns[1].contains("不表示可启用"));
}

#[test]
fn pending_page_allows_only_cancel_wait_and_lock_with_readonly_fields() {
    let e = edit();
    let view = e.view(owner(1), "正在保存", true);
    view.validate().unwrap();
    assert!(!view.busy);
    assert!(view.fields.iter().all(|f| f.kind == FieldKind::ReadOnly));
    assert!(
        view.actions
            .iter()
            .find(|a| a.id == CANCEL_WAIT)
            .unwrap()
            .enabled
    );
    assert!(view
        .actions
        .iter()
        .filter(|a| a.enabled)
        .all(|a| [CANCEL_WAIT, LOCK].contains(&a.id)));
}

#[test]
fn source_policy_must_be_explicit_and_is_part_of_original_unknown_command() {
    let mut absent = form("localhost", "8443");
    absent.take(SOURCE_NETWORKS);
    assert!(edit().capture(absent).is_err());
    for invalid in ["", "0.0.0.0/0", "192.168.1.1/24", "::/0"] {
        let mut data = form("localhost", "8443");
        data.take(SOURCE_NETWORKS);
        data.insert(SOURCE_NETWORKS, invalid.into()).unwrap();
        assert!(edit().handle(owner(1), SAVE_CONFIG, data).is_err());
    }
    let mut e = edit();
    e.capture(form("localhost", "8443")).unwrap();
    let request = e.prepare_save(id(78), Instant::now()).unwrap();
    assert_eq!(
        request
            .configuration
            .as_ref()
            .unwrap()
            .source_networks
            .as_ref()
            .unwrap()
            .to_text(),
        "127.0.0.1/32"
    );
    e.accepted();
    let mut changed = form("localhost", "8443");
    changed.take(SOURCE_NETWORKS);
    changed
        .insert(SOURCE_NETWORKS, "192.168.10.0/24".into())
        .unwrap();
    e.capture(changed).unwrap();
    assert_eq!(e.query_request(), Some(request));
    assert!(e.dirty());
    assert!(e.handle(owner(1), SAVE_CONFIG, FormData::new()).is_err());
}
#[test]
fn legacy_saved_configuration_does_not_invent_source_networks() {
    let mut e = Editor::new(owner(1));
    e.loaded(configured(3));
    let v = e.view(owner(1), "", false);
    assert_eq!(
        v.fields
            .iter()
            .find(|f| f.id == SOURCE_NETWORKS)
            .unwrap()
            .value,
        ""
    );
    assert!(e.prepare_save(id(79), Instant::now()).is_err());
}
