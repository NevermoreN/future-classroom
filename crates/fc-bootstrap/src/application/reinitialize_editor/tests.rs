use super::*;

fn prepare(sequence: u64) -> Editor {
    let mut editor = Editor::new(sequence);
    assert!(matches!(
        editor.event(PREPARE, FormData::new()).unwrap(),
        Command::Prepare
    ));
    editor.set_review(sequence).unwrap();
    editor
}
fn consent_from_view(editor: &Editor, included: &[FieldId]) -> FormData {
    let mut form = FormData::new();
    for field in editor.view("", false).fields {
        if included.contains(&field.id) {
            let FieldKind::Choice(choices) = field.kind else {
                panic!("native choice required");
            };
            form.insert(
                field.id,
                choices
                    .into_iter()
                    .find(|choice| choice.value != "0")
                    .unwrap()
                    .value,
            )
            .unwrap();
        }
    }
    form
}
#[test]
fn neither_opening_nor_forged_form_can_bypass_actual_current_preparation() {
    let mut editor = Editor::new(3);
    assert_eq!(editor.sequence(), 3);
    editor.view("", false).validate().unwrap();
    assert!(editor.view("", false).fields.is_empty());
    assert!(editor.set_review(3).is_err());
    let consent = consent_from_view(&prepare(3), &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
    assert!(editor.event(CONFIRM, consent).is_err());
    assert!(matches!(
        editor.event(PREPARE, FormData::new()).unwrap(),
        Command::Prepare
    ));
    assert!(editor.set_review(2).is_err());
    assert!(editor.set_review(4).is_err());
    assert!(editor.event(PREPARE, FormData::new()).is_err());
    assert!(editor.event(CONFIRM, FormData::new()).is_err());
    editor.set_review(3).unwrap();
    assert!(editor.set_review(3).is_err());
}
#[test]
fn two_explicit_default_unconfirmed_sequence_bound_choices_are_both_required() {
    let mut editor = prepare(9);
    let view = editor.view("", false);
    assert_eq!(view.fields.len(), 2);
    for field in &view.fields {
        assert_eq!(field.value, "0");
    }
    for included in [&[][..], &[ACK_DATA_LOSS][..], &[ACK_NEW_INSTALLATION][..]] {
        let form = consent_from_view(&editor, included);
        assert!(editor.event(CONFIRM, form).is_err());
        assert!(editor
            .view("", false)
            .fields
            .iter()
            .all(|field| field.value == "0"));
    }
    let mut forged = FormData::new();
    forged.insert(ACK_DATA_LOSS, "1".into()).unwrap();
    forged.insert(ACK_NEW_INSTALLATION, "true".into()).unwrap();
    assert!(editor.event(CONFIRM, forged).is_err());
    let consent = consent_from_view(&editor, &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
    assert!(matches!(
        editor.event(CONFIRM, consent).unwrap(),
        Command::Confirm { sequence: 9 }
    ));
    assert!(editor.view("", false).actions.is_empty());
    assert!(editor.view("", false).fields.is_empty());
}
#[test]
fn stale_or_mixed_acknowledgements_cannot_confirm_a_new_preparation() {
    let old = prepare(10);
    let mut current = prepare(11);
    let stale = consent_from_view(&old, &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
    assert!(current.event(CONFIRM, stale).is_err());
    let mut mixed = consent_from_view(&old, &[ACK_DATA_LOSS]);
    let mut new = consent_from_view(&current, &[ACK_NEW_INSTALLATION]);
    mixed
        .insert(
            ACK_NEW_INSTALLATION,
            new.take(ACK_NEW_INSTALLATION).unwrap(),
        )
        .unwrap();
    assert!(current.event(CONFIRM, mixed).is_err());
    let consent = consent_from_view(&current, &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
    assert!(matches!(
        current.event(CONFIRM, consent).unwrap(),
        Command::Confirm { sequence: 11 }
    ));
}
#[test]
fn cancel_in_every_reversible_stage_has_no_irreversible_intent_and_is_terminal() {
    for stage in 0..3 {
        let mut editor = Editor::new(19);
        if stage > 0 {
            editor.event(PREPARE, FormData::new()).unwrap();
        }
        if stage > 1 {
            editor.set_review(19).unwrap();
        }
        let view = editor.view("", true);
        assert!(view.actions.iter().any(|a| a.id == CANCEL && a.enabled));
        assert!(view
            .actions
            .iter()
            .filter(|a| a.id != CANCEL)
            .all(|a| !a.enabled));
        assert!(matches!(
            editor.event(CANCEL, FormData::new()).unwrap(),
            Command::Cancel
        ));
        assert!(editor.view("", false).actions.is_empty());
        assert!(editor.set_review(19).is_err());
        for action in [PREPARE, CONFIRM, CANCEL] {
            let consent = consent_from_view(&prepare(19), &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
            assert!(editor.event(action, consent).is_err());
        }
    }
}
#[test]
fn invalidation_and_confirmation_cannot_be_revived_or_double_submitted() {
    for confirmed in [false, true] {
        let mut editor = prepare(23);
        if confirmed {
            let consent = consent_from_view(&editor, &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
            editor.event(CONFIRM, consent).unwrap();
        } else {
            editor.invalidate();
        }
        assert!(editor.set_review(23).is_err());
        for action in [PREPARE, CONFIRM, CANCEL, ActionId(719)] {
            assert!(editor.event(action, FormData::new()).is_err());
        }
        editor.invalidate();
        assert!(editor.view("", false).actions.is_empty());
        assert!(editor.set_review(23).is_err());
    }
    let mut zero = Editor::new(0);
    assert!(zero.event(PREPARE, FormData::new()).is_err());
    assert!(zero.set_review(0).is_err());
}
#[test]
fn unrelated_forged_actions_and_fields_cannot_select_files_credentials_or_sequence() {
    let mut editor = prepare(27);
    for action in [
        ActionId(0),
        ActionId(670),
        ActionId(703),
        ActionId(u16::MAX),
        PREPARE,
    ] {
        assert!(editor.event(action, FormData::new()).is_err());
    }
    let mut consent = consent_from_view(&editor, &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
    for (id, value) in [
        (FieldId(702), "9999"),
        (FieldId(703), "C:\\another deployment"),
        (FieldId(704), "forged recovery code"),
    ] {
        consent.insert(id, value.into()).unwrap();
    }
    assert!(editor.event(CONFIRM, consent).is_err());
    assert!(editor
        .view("", false)
        .fields
        .iter()
        .all(|field| field.value == "0"));
    let fresh_consent = consent_from_view(&editor, &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
    assert!(matches!(
        editor.event(CONFIRM, fresh_consent).unwrap(),
        Command::Confirm { sequence: 27 }
    ));
}
#[test]
fn duplicate_fields_are_rejected_before_submission_without_replacing_captured_consent() {
    let editor = prepare(28);
    let mut consent = consent_from_view(&editor, &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
    let expected_loss = consent.get(ACK_DATA_LOSS).unwrap().to_owned();
    let expected_setup = consent.get(ACK_NEW_INSTALLATION).unwrap().to_owned();
    assert_eq!(
        consent.insert(ACK_DATA_LOSS, "erase-data-999".into()),
        Err(FormError::DuplicateField)
    );
    assert_eq!(
        consent.insert(ACK_NEW_INSTALLATION, "new-setup-999".into()),
        Err(FormError::DuplicateField)
    );
    assert_eq!(consent.get(ACK_DATA_LOSS), Some(expected_loss.as_str()));
    assert_eq!(
        consent.get(ACK_NEW_INSTALLATION),
        Some(expected_setup.as_str())
    );
    let mut intro = Editor::new(28);
    let mut unknown = FormData::new();
    unknown
        .insert(FieldId(719), "untrusted deployment path".into())
        .unwrap();
    assert!(intro.event(PREPARE, unknown).is_err());
    assert!(matches!(
        intro.event(PREPARE, FormData::new()).unwrap(),
        Command::Prepare
    ));
    intro.set_review(28).unwrap();
}
#[test]
fn text_explains_destructive_scope_new_setup_and_exact_unchanged_system_scope() {
    let editor = prepare(29);
    let view = editor.view("", false);
    view.validate().unwrap();
    for text in [
        "教学数据",
        "资料",
        "图片",
        "学校与账号",
        "密码和恢复码",
        "设备配对",
        "语音数据",
        "不需要现有密码或恢复码",
        "不会找回旧密码或解锁原账号",
        "当前程序关闭",
        "重新启动",
        "新的主管理员",
        "不会重置 Windows",
        "Windows 登录账号",
        "网络",
        "SSH 授权",
        "其他应用",
        "默认均未确认",
        "正式资料不会被本次准备清除",
        "不能从此页撤回",
        "不要依赖本操作自动保留备份",
        "当前部署目录内导出的备份及缓存一并清除",
        "已另存到部署目录以外的备份副本不在本次清除范围",
        "不会更改 Windows 现有自动启动、网络及 SSH 授权和设置",
        "重新启动后始终必须重新设置学校并创建新的主管理员",
        "新部署不会读取旧部署保留的隔离资料",
        "不提供从这些资料恢复原账号或教学数据的入口",
    ] {
        assert!(view.description.contains(text), "missing {text}");
    }
    assert!(!view.description.contains("nonce"));
    assert!(!view.description.contains("可撤销"));
}
#[test]
fn all_stage_views_and_maximum_sequence_obey_native_utf16_and_text_budgets() {
    let mut editor = Editor::new(u64::MAX);
    let hostile = format!("\0\n\u{202e}{}", "😀".repeat(9000));
    for busy in [false, true] {
        editor.view(&hostile, busy).validate().unwrap();
    }
    editor.event(PREPARE, FormData::new()).unwrap();
    editor.view(&hostile, true).validate().unwrap();
    editor.set_review(u64::MAX).unwrap();
    let view = editor.view(&hostile, false);
    view.validate().unwrap();
    assert!(view.status.chars().count() <= 2048);
    assert!(!view
        .status
        .chars()
        .any(|c| c.is_control() || c == '\u{202e}'));
    for field in &view.fields {
        let FieldKind::Choice(choices) = &field.kind else {
            panic!("native choices required");
        };
        assert!(choices
            .iter()
            .all(|c| c.value.encode_utf16().count() <= field.max_utf16));
    }
    let consent = consent_from_view(&editor, &[ACK_DATA_LOSS, ACK_NEW_INSTALLATION]);
    editor.event(CONFIRM, consent).unwrap();
    editor.view(&hostile, true).validate().unwrap();
    editor.invalidate();
    editor.view(&hostile, false).validate().unwrap();
}
