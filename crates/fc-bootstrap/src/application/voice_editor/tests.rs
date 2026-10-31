use super::*;

fn snapshot(enabled: bool) -> VoiceManagementSnapshot {
    VoiceManagementSnapshot {
        publication_revision: Revision::new(30),
        identity_revision: Revision::new(7),
        revision: Revision::new(12),
        enabled,
        admitting: enabled,
        runtime_generation: 3,
        active_jobs: 0,
        queued_jobs: 0,
        playing_jobs: 0,
    }
}

fn editor(enabled: bool) -> Editor {
    Editor::new(PrincipalId::new(4).unwrap(), snapshot(enabled))
}

fn act(editor: &mut Editor, action: ActionId) -> Result<Command, String> {
    editor.handle(action, FormData::new())
}

fn acknowledged() -> FormData {
    let mut data = FormData::new();
    data.insert(ACK, "1".into()).unwrap();
    data
}

#[test]
fn review_starts_unconfirmed_and_does_not_submit_without_explicit_consent() {
    let mut e = editor(false);
    assert!(e.handle(CONFIRM, acknowledged()).is_err());
    assert!(!e.has_unsaved_changes());
    assert!(matches!(act(&mut e, ENABLE).unwrap(), Command::Stay));
    assert!(e.has_unsaved_changes());
    let view = e.view("", false);
    view.validate().unwrap();
    assert_eq!(view.fields.len(), 1);
    assert_eq!(view.fields[0].id, ACK);
    assert_eq!(view.fields[0].value, "0");
    assert!(act(&mut e, CONFIRM).is_err());
    assert!(e.has_unsaved_changes());
    assert!(matches!(
        e.handle(CONFIRM, acknowledged()).unwrap(),
        Command::SetEnabled {
            expected,
            expected_identity,
            expected_runtime,
            enabled: true,
        } if expected == Revision::new(12)
            && expected_identity == Revision::new(7)
            && expected_runtime == 3
    ));
    assert!(!e.has_unsaved_changes());
    assert!(e.handle(CONFIRM, acknowledged()).is_err());
    assert!(act(&mut e, DISABLE).is_err());
    assert_eq!(e.owner(), PrincipalId::new(4).unwrap());
}

#[test]
fn forged_fields_or_a_second_action_cannot_change_captured_intent_or_revision() {
    let mut e = editor(true);
    act(&mut e, DISABLE).unwrap();
    assert!(act(&mut e, ENABLE).is_err());
    let mut data = acknowledged();
    data.insert(FieldId(851), "999999".into()).unwrap();
    data.insert(FieldId(852), "true".into()).unwrap();
    assert!(e.handle(CONFIRM, data).is_err());
    assert!(e.has_unsaved_changes());
    assert!(matches!(
        e.handle(CONFIRM, acknowledged()).unwrap(),
        Command::SetEnabled {
            expected,
            expected_identity,
            expected_runtime,
            enabled: false,
        } if expected == Revision::new(12)
            && expected_identity == Revision::new(7)
            && expected_runtime == 3
    ));
}

#[test]
fn every_successful_load_discards_old_review_even_with_the_same_configuration() {
    for change in 0..4 {
        let mut e = editor(false);
        act(&mut e, ENABLE).unwrap();
        let mut updated = snapshot(false);
        match change {
            0 => {}
            1 => updated.revision = Revision::new(13),
            2 => updated.identity_revision = Revision::new(8),
            _ => updated.runtime_generation = 4,
        }
        e.loaded(updated);
        assert!(!e.has_unsaved_changes());
        assert!(e.handle(CONFIRM, acknowledged()).is_err());
        act(&mut e, ENABLE).unwrap();
        assert!(matches!(
            e.handle(CONFIRM, acknowledged()).unwrap(),
            Command::SetEnabled { expected, expected_identity, expected_runtime, enabled: true }
                if expected == Revision::new(if change == 1 { 13 } else { 12 })
                    && expected_identity == Revision::new(if change == 2 { 8 } else { 7 })
                    && expected_runtime == if change == 3 { 4 } else { 3 }
        ));
    }
}

#[test]
fn invalidation_and_a_refresh_request_require_an_actual_successful_load() {
    let mut e = editor(false);
    act(&mut e, ENABLE).unwrap();
    e.invalidate();
    assert!(!e.has_unsaved_changes());
    assert!(e.handle(CONFIRM, acknowledged()).is_err());
    assert!(act(&mut e, ENABLE).is_err());
    assert!(matches!(act(&mut e, REFRESH).unwrap(), Command::Refresh));
    assert!(act(&mut e, ENABLE).is_err());
    assert!(e
        .view("", false)
        .actions
        .iter()
        .filter(|action| action.id == ENABLE || action.id == DISABLE)
        .all(|action| !action.enabled));
    e.loaded(snapshot(false));
    assert!(matches!(act(&mut e, ENABLE).unwrap(), Command::Stay));
}

#[test]
fn cancel_close_and_refresh_never_submit_a_setting_change() {
    for action in [CANCEL, CLOSE, REFRESH] {
        let mut e = editor(true);
        act(&mut e, DISABLE).unwrap();
        let command = act(&mut e, action).unwrap();
        assert!(!matches!(command, Command::SetEnabled { .. }));
        assert!(!e.has_unsaved_changes());
        assert!(e.snapshot.enabled, "consent cannot mutate the snapshot");
        assert!(e.handle(CONFIRM, acknowledged()).is_err());
        if action == CANCEL {
            assert!(matches!(act(&mut e, DISABLE).unwrap(), Command::Stay));
        }
    }
}

#[test]
fn setting_state_counts_and_unproven_playback_and_connection_are_distinct() {
    for enabled in [false, true] {
        let mut state = snapshot(enabled);
        state.active_jobs = 4;
        state.queued_jobs = 3;
        state.playing_jobs = 1;
        let mut e = Editor::new(PrincipalId::new(4).unwrap(), state);
        let view = e.view("", false);
        view.validate().unwrap();
        assert!(view.description.contains(if enabled {
            "远程语音配置：开启"
        } else {
            "远程语音配置：关闭"
        }));
        for text in [
            "活跃任务：4；排队任务：3；播放中任务：1",
            "教师连接服务当前未就绪",
            "全机最多 4 个任务，每位教师最多 2 个",
            "0.5–60 秒",
            "1–10 遍，默认 3 遍，遍间隔 1 秒",
            "取消全部当前和排队任务",
            "物理播放停止尚未完成时，不能声称已停止",
            "不恢复旧任务",
            "不能证明音频设备可用",
            "不能证明教室已听到声音",
        ] {
            assert!(view.description.contains(text), "missing: {text}");
        }
        if !enabled {
            assert!(view.description.contains("停止结果待实际完成"));
        }
        act(&mut e, if enabled { DISABLE } else { ENABLE }).unwrap();
        let review = e.view("", false);
        review.validate().unwrap();
        assert!(review.description.contains(if enabled {
            "待确认：关闭"
        } else {
            "待确认：开启"
        }));
    }
}

#[test]
fn busy_page_disables_all_actions_and_status_is_bounded_non_control_text() {
    let mut e = editor(false);
    for reviewing in [false, true] {
        if reviewing {
            act(&mut e, ENABLE).unwrap();
        }
        let view = e.view(&format!("\0\u{202e}{}", "x".repeat(9000)), true);
        view.validate().unwrap();
        assert!(view.actions.iter().all(|action| !action.enabled));
        assert!(view.status.chars().count() <= 2048);
        assert!(!view.status.contains('\0'));
        assert!(!view.status.contains('\u{202e}'));
    }
}

#[test]
fn state_already_selected_unknown_actions_and_unexpected_parameters_do_not_review() {
    for enabled in [false, true] {
        let mut e = editor(enabled);
        assert!(act(&mut e, if enabled { ENABLE } else { DISABLE }).is_err());
        assert!(act(&mut e, ActionId(856)).is_err());
        let mut data = FormData::new();
        data.insert(ACK, "1".into()).unwrap();
        assert!(e
            .handle(if enabled { DISABLE } else { ENABLE }, data)
            .is_err());
        assert!(!e.has_unsaved_changes());
    }
}

#[test]
fn native_cancel_discards_its_visible_acknowledgement_without_submitting() {
    for value in ["0", "1"] {
        let mut e = editor(true);
        act(&mut e, DISABLE).unwrap();
        let mut data = FormData::new();
        data.insert(ACK, value.into()).unwrap();
        assert!(matches!(e.handle(CANCEL, data).unwrap(), Command::Stay));
        assert!(!e.has_unsaved_changes());
        assert!(e.snapshot.enabled);
    }
}
