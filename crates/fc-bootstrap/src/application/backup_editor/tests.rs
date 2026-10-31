use super::*;

fn owner() -> PrincipalId {
    PrincipalId::new(7).unwrap()
}
fn summary(path: &str) -> ReviewSummary {
    ReviewSummary {
        current_revision: Revision::new(42),
        source_revision: Revision::new(12),
        school_id: SchoolId::new(3).unwrap(),
        document_count: 77,
        material_count: 5,
        material_bytes: 12345,
        profile_media_count: 6,
        profile_media_bytes: 45678,
        database_bytes: 90123,
        selected_file: path.into(),
    }
}
fn acknowledged(sequence: u64) -> FormData {
    let mut data = FormData::new();
    data.insert(ACK, format!("restore-{sequence}")).unwrap();
    data
}
fn ready() -> Editor {
    let mut e = Editor::new(owner());
    e.begin_review(4).unwrap();
    e.set_review(owner(), 4, summary("C:\\备份\\课堂.fcbak"))
        .unwrap();
    e
}
fn act(e: &mut Editor, action: ActionId) -> Result<Command, String> {
    e.event(action, FormData::new())
}
#[test]
fn confirmation_requires_live_preflight_explicit_ack_and_is_consumed_once() {
    let mut e = Editor::new(owner());
    assert!(e.event(CONFIRM, acknowledged(4)).is_err());
    e.begin_review(4).unwrap();
    assert!(e.event(CONFIRM, acknowledged(4)).is_err());
    e.set_review(owner(), 4, summary("C:\\backup.fcbak"))
        .unwrap();
    assert!(e.has_unsaved_changes());
    assert!(act(&mut e, CONFIRM).is_err());
    assert!(e.has_unsaved_changes());
    assert!(matches!(
        e.event(CONFIRM, acknowledged(4)).unwrap(),
        Command::Confirm { sequence: 4 }
    ));
    assert!(!e.has_unsaved_changes());
    for action in [CONFIRM, CANCEL, SELECT_RESTORE, EXPORT, CLOSE] {
        assert!(e.event(action, acknowledged(4)).is_err());
    }
    assert!(e.view("", false).actions.is_empty());
}
#[test]
fn cancel_reselect_close_and_invalidation_reject_late_preflight_and_old_confirmations() {
    for action in [CANCEL, SELECT_RESTORE, CLOSE] {
        let mut e = ready();
        assert!(!matches!(
            act(&mut e, action).unwrap(),
            Command::Confirm { .. }
        ));
        assert!(!e.has_unsaved_changes());
        assert!(e.event(CONFIRM, acknowledged(4)).is_err());
        assert!(e.set_review(owner(), 4, summary("late")).is_err());
    }
    let mut e = ready();
    e.invalidate();
    assert!(!e.has_unsaved_changes());
    assert!(e.view("", false).actions.is_empty());
    assert!(e.begin_review(5).is_err());
    assert!(e.set_review(owner(), 4, summary("late")).is_err());
    assert!(e.event(CONFIRM, acknowledged(4)).is_err());
}
#[test]
fn owner_and_exact_monotone_pending_sequence_bind_review() {
    let mut e = Editor::new(owner());
    assert!(e.begin_review(0).is_err());
    e.begin_review(4).unwrap();
    assert!(e.begin_review(5).is_err());
    assert!(e
        .set_review(PrincipalId::new(8).unwrap(), 4, summary("wrong owner"))
        .is_err());
    assert!(e.set_review(owner(), 3, summary("old")).is_err());
    assert!(e.set_review(owner(), 5, summary("future")).is_err());
    act(&mut e, CANCEL).unwrap();
    assert!(e.set_review(owner(), 4, summary("late")).is_err());
    assert!(e.begin_review(4).is_err());
    e.begin_review(5).unwrap();
    e.set_review(owner(), 5, summary("current")).unwrap();
    assert_eq!(e.owner(), owner());
    assert!(matches!(
        e.event(CONFIRM, acknowledged(5)).unwrap(),
        Command::Confirm { sequence: 5 }
    ));
}
#[test]
fn queued_old_review_ack_cannot_confirm_a_new_preflight_review() {
    let mut e = ready();
    let old_view = e.view("", false);
    let old_ack = match &old_view.fields.iter().find(|f| f.id == ACK).unwrap().kind {
        FieldKind::Choice(choices) => choices
            .iter()
            .find(|c| c.value != "0")
            .unwrap()
            .value
            .clone(),
        _ => panic!("explicit acknowledgement choice required"),
    };
    act(&mut e, SELECT_RESTORE).unwrap();
    e.begin_review(5).unwrap();
    e.set_review(owner(), 5, summary("new backup")).unwrap();
    let mut stale = FormData::new();
    stale.insert(ACK, old_ack).unwrap();
    assert!(e.event(CONFIRM, stale).is_err());
    assert!(e.has_unsaved_changes());
    let current_view = e.view("", false);
    current_view.validate().unwrap();
    let current_ack = match &current_view
        .fields
        .iter()
        .find(|f| f.id == ACK)
        .unwrap()
        .kind
    {
        FieldKind::Choice(choices) => choices
            .iter()
            .find(|c| c.value != "0")
            .unwrap()
            .value
            .clone(),
        _ => panic!("explicit acknowledgement choice required"),
    };
    let mut current = FormData::new();
    current.insert(ACK, current_ack).unwrap();
    assert!(matches!(
        e.event(CONFIRM, current).unwrap(),
        Command::Confirm { sequence: 5 }
    ));
}
#[test]
fn forged_readonly_fields_cannot_replace_preflight_identity_or_path() {
    let mut e = ready();
    let mut data = acknowledged(4);
    for id in [SELECTED_PATH, VERSIONS, CONTENTS, FieldId(999)] {
        data.insert(id, "forged path revision sequence owner password".into())
            .unwrap();
    }
    let view = e.view("", false);
    view.validate().unwrap();
    assert!(view
        .fields
        .iter()
        .any(|f| f.id == SELECTED_PATH && f.value == "C:\\备份\\课堂.fcbak"));
    assert!(view
        .fields
        .iter()
        .any(|f| f.id == VERSIONS && f.value.contains("42") && f.value.contains("12")));
    assert!(matches!(
        e.event(CONFIRM, data).unwrap(),
        Command::Confirm { sequence: 4 }
    ));
}
#[test]
fn review_explains_full_replacement_retained_current_security_and_restart() {
    let e = ready();
    let v = e.view("", false);
    v.validate().unwrap();
    for text in [
        "教学学校",
        "班级",
        "科目",
        "课表",
        "校历",
        "作业",
        "提醒",
        "布局",
        "教学资料",
        "当前版",
        "上一版",
        "当前密码",
        "登录限速",
        "账号停用",
        "角色",
        "任教权限",
        "系统状态",
        "语音队列",
        "语音文件",
        "旧会话",
        "确认前可取消",
        "安全中止",
        "核对结果会保留",
        "重新启动并登录",
    ] {
        assert!(v.description.contains(text), "missing {text}");
    }
    assert_eq!(v.fields.iter().find(|f| f.id == ACK).unwrap().value, "0");
    assert!(!v.description.contains("installation_id"));
    assert!(e.view("", true).actions.iter().all(|a| !a.enabled));
    let count = v.fields.iter().find(|f| f.id == CONTENTS).unwrap();
    for text in ["77", "90123", "12345", "45678"] {
        assert!(count.value.contains(text));
    }
}
#[test]
fn long_unicode_paths_are_complete_paginated_and_readonly_without_truncation() {
    let path = format!("C:\\{}\\backup.fcbak", "校😀".repeat(1200));
    let mut e = Editor::new(owner());
    assert!(matches!(act(&mut e, SAVE_COPY).unwrap(), Command::Stay));
    e.exported(path.clone(), 123).unwrap();
    assert!(matches!(act(&mut e, SAVE_COPY).unwrap(), Command::SaveCopy));
    let mut displayed = String::new();
    loop {
        let v = e.view("", false);
        v.validate().unwrap();
        let f = v.fields.iter().find(|f| f.id == SELECTED_PATH).unwrap();
        assert_eq!(f.kind, FieldKind::ReadOnly);
        assert!(f.value.encode_utf16().count() <= PATH_PAGE_UNITS);
        displayed.push_str(&f.value);
        if !v.actions.iter().any(|a| a.id == NEXT_PATH && a.enabled) {
            break;
        }
        assert!(matches!(act(&mut e, NEXT_PATH).unwrap(), Command::Stay));
    }
    assert_eq!(displayed, path);
    assert!(e
        .view("", false)
        .description
        .contains("备份已导出，共 123 字节"));
}
#[test]
fn controls_oversize_paths_and_status_cannot_inject_unbounded_ui_text() {
    for path in [
        String::new(),
        "C:\\a\nforged".into(),
        "C:\\a\0hidden".into(),
        "C:\\a\u{202e}hidden".into(),
        "a".repeat(MAX_PATH_UNITS + 1),
    ] {
        let mut e = Editor::new(owner());
        assert!(e.exported(path.clone(), 1).is_err());
        e.begin_review(1).unwrap();
        assert!(e.set_review(owner(), 1, summary(&path)).is_err());
        assert!(e.event(CONFIRM, acknowledged(4)).is_err());
    }
    let v = ready().view(&format!("\0\n\u{202e}{}", "😀".repeat(9000)), false);
    v.validate().unwrap();
    assert!(v.status.chars().count() <= 2048);
    assert!(!v.status.chars().any(unsafe_display_char));
}
#[test]
fn opening_rendering_and_navigation_never_submit_restore_or_export_automatically() {
    let mut e = Editor::new(owner());
    e.view("", false).validate().unwrap();
    assert!(!e.has_unsaved_changes());
    assert!(matches!(act(&mut e, EXPORT).unwrap(), Command::Export));
    assert!(matches!(
        act(&mut e, SELECT_RESTORE).unwrap(),
        Command::SelectRestore
    ));
    assert!(!e.has_unsaved_changes());
    e.begin_review(1).unwrap();
    assert!(matches!(act(&mut e, EXPORT).unwrap(), Command::Stay));
    assert!(e.exported("unrelated".into(), 0).is_err());
    e.set_review(owner(), 1, summary("chosen")).unwrap();
    assert!(matches!(act(&mut e, PREVIOUS_PATH).unwrap(), Command::Stay));
    assert!(matches!(act(&mut e, NEXT_PATH).unwrap(), Command::Stay));
    e.view("", false).validate().unwrap();
    assert!(e.has_unsaved_changes());
}
#[test]
fn busy_preflight_export_and_copy_can_cancel_without_reviving_invalid_or_confirmed_review() {
    let mut e = Editor::new(owner());
    for exported in [false, true] {
        if exported {
            e.exported("C:\\backup.fcbak".into(), 123).unwrap();
        }
        let v = e.view("", true);
        v.validate().unwrap();
        assert!(v.actions.iter().any(|a| a.id == CANCEL && a.enabled));
        assert!(v
            .actions
            .iter()
            .filter(|a| a.id != CANCEL)
            .all(|a| !a.enabled));
    }
    e.begin_review(1).unwrap();
    let v = e.view("", true);
    assert!(v.actions.iter().any(|a| a.id == CANCEL && a.enabled));
    e.cancel_review();
    assert!(!e.has_unsaved_changes());
    assert!(e.set_review(owner(), 1, summary("late")).is_err());
    e.begin_review(2).unwrap();
    e.set_review(owner(), 2, summary("current")).unwrap();
    e.cancel_review();
    assert!(e.event(CONFIRM, acknowledged(4)).is_err());
    e.begin_review(3).unwrap();
    e.set_review(owner(), 3, summary("current")).unwrap();
    e.event(CONFIRM, acknowledged(3)).unwrap();
    e.cancel_review();
    assert!(e.begin_review(4).is_err());
    assert!(e.view("", false).actions.is_empty());
    e.invalidate();
    e.cancel_review();
    assert!(e.begin_review(4).is_err());
    assert!(e.view("", false).actions.is_empty());
}
