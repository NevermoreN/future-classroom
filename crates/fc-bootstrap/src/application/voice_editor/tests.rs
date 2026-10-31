use super::*;

fn snapshot(enabled: bool) -> VoiceManagementSnapshot {
    VoiceManagementSnapshot {
        publication_revision: Revision::new(30),
        identity_revision: Revision::new(7),
        revision: Revision::new(12),
        enabled,
        limits: VoiceLimits::default(),
        admitting: enabled,
        runtime_generation: 3,
        active_jobs: 0,
        queued_jobs: 0,
        playing_jobs: 0,
        device_observation: None,
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
            "每遍之间固定静音 1 秒",
            "已保存限制：单段最多 60 秒；全机最多 4 个任务；本人最多 2 个；播放最多 10 遍，新录制默认 3 遍",
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

fn limits_form(duration: &str, pending: &str, owner: &str, repeats: &str) -> FormData {
    let mut data = FormData::new();
    for (id, value) in [
        (DURATION_SECONDS, duration),
        (PENDING_JOBS, pending),
        (PENDING_PER_OWNER, owner),
        (REPEAT_TOTAL, repeats),
    ] {
        data.insert(id, value.into()).unwrap();
    }
    data
}
fn custom_limits() -> VoiceLimits {
    VoiceLimits {
        max_duration_millis: 1250,
        max_pending_jobs: 3,
        max_pending_per_owner: 1,
        max_repeat_total: 2,
    }
}
#[test]
fn exact_millisecond_parser_accepts_boundaries_without_floats() {
    for (text, expected) in [
        ("0.5", 500),
        ("0.500", 500),
        ("1.001", 1001),
        ("1.25", 1250),
        ("59.999", 59999),
        ("60", 60000),
        ("60.000", 60000),
    ] {
        let mut e = editor(true);
        e.capture(limits_form(text, "3", "1", "2")).unwrap();
        assert_eq!(e.draft.parse().unwrap().max_duration_millis, expected);
    }
    for text in [
        "",
        "0.499",
        "60.001",
        "61",
        "1.0000",
        "1.",
        ".5",
        " 1",
        "1 ",
        "+1",
        "1e1",
        "NaN",
        "１",
        "1..2",
        "4294967295",
        "9999999999999999",
    ] {
        let mut e = editor(true);
        assert!(
            e.handle(SAVE_LIMITS, limits_form(text, "3", "1", "2"))
                .is_err(),
            "{text}"
        );
        assert!(e.review.is_none());
    }
}
#[test]
fn limit_bounds_and_owner_global_relationship_are_checked() {
    for (pending, owner, repeats) in [
        ("0", "1", "2"),
        ("5", "1", "2"),
        ("1", "2", "2"),
        ("2", "0", "2"),
        ("4", "3", "2"),
        ("4", "1", "0"),
        ("4", "1", "11"),
        ("3.0", "1", "2"),
        ("3", " 1", "2"),
        ("3", "1", "+2"),
    ] {
        let mut e = editor(true);
        assert!(e
            .handle(SAVE_LIMITS, limits_form("1.25", pending, owner, repeats))
            .is_err());
        assert!(e.review.is_none());
    }
    for (pending, owner, repeats) in [("1", "1", "1"), ("4", "2", "10")] {
        let mut e = editor(true);
        assert!(matches!(
            e.handle(SAVE_LIMITS, limits_form("0.5", pending, owner, repeats))
                .unwrap(),
            Command::Stay
        ));
    }
}
#[test]
fn save_limits_requires_exact_complete_fields_without_versions() {
    let mut e = editor(true);
    assert!(act(&mut e, SAVE_LIMITS).is_err());
    let mut partial = limits_form("1.25", "3", "1", "2");
    partial.take(REPEAT_TOTAL);
    assert!(e.handle(SAVE_LIMITS, partial).is_err());
    let mut forged = limits_form("1.25", "3", "1", "2");
    forged.insert(FieldId(899), "999".into()).unwrap();
    assert!(e.handle(SAVE_LIMITS, forged).is_err());
    assert!(e.review.is_none());
    assert!(!e.has_unsaved_changes());
}
#[test]
fn limits_review_captures_all_versions_and_requires_explicit_ack() {
    let mut e = editor(true);
    e.handle(SAVE_LIMITS, limits_form("1.250", "3", "1", "2"))
        .unwrap();
    let view = e.view("", false);
    view.validate().unwrap();
    assert_eq!(
        view.fields
            .iter()
            .find(|field| field.id == ACK)
            .unwrap()
            .value,
        "0"
    );
    assert!(view.description.contains("取消当前和排队任务及旧上传"));
    assert!(view
        .description
        .contains("不会改写已录制或已排队任务的遍数"));
    assert!(act(&mut e, CONFIRM).is_err());
    let mut forged = acknowledged();
    forged.insert(DURATION_SECONDS, "60".into()).unwrap();
    assert!(e.handle(CONFIRM, forged).is_err());
    assert!(
        matches!(e.handle(CONFIRM,acknowledged()).unwrap(), Command::SetLimits { expected,expected_identity,expected_runtime,limits } if expected==Revision::new(12) && expected_identity==Revision::new(7) && expected_runtime==3 && limits==custom_limits())
    );
    assert!(e.requires_refresh);
    assert!(e.handle(CONFIRM, acknowledged()).is_err());
    let mut updated = snapshot(true);
    updated.revision = Revision::new(13);
    updated.limits = custom_limits();
    e.loaded(updated);
    assert!(!e.has_unsaved_changes());
}
#[test]
fn cancel_with_visible_draft_and_checked_ack_does_not_submit_and_keeps_draft() {
    let mut e = editor(true);
    e.handle(SAVE_LIMITS, limits_form("1.25", "3", "1", "2"))
        .unwrap();
    let mut form = limits_form("1.25", "3", "1", "2");
    form.insert(ACK, "1".into()).unwrap();
    assert!(matches!(e.handle(CANCEL, form).unwrap(), Command::Stay));
    assert!(e.review.is_none());
    assert_eq!(e.draft.parse().unwrap(), custom_limits());
    assert_eq!(e.snapshot.limits, VoiceLimits::default());
    assert!(e.has_unsaved_changes());
    assert!(matches!(
        act(&mut e, DISCARD_LIMITS).unwrap(),
        Command::Stay
    ));
    assert!(!e.has_unsaved_changes());
}
#[test]
fn visible_drafts_are_preserved_by_switch_results_and_refresh_without_saving() {
    let mut e = editor(true);
    assert!(matches!(
        e.handle(DISABLE, limits_form("1.25", "3", "1", "2"))
            .unwrap(),
        Command::Stay
    ));
    assert!(matches!(
        e.handle(CONFIRM, acknowledged()).unwrap(),
        Command::SetEnabled { enabled: false, .. }
    ));
    let mut disabled = snapshot(false);
    disabled.revision = Revision::new(13);
    e.loaded(disabled);
    assert_eq!(e.draft.parse().unwrap(), custom_limits());
    assert_eq!(e.snapshot.limits, VoiceLimits::default());
    assert!(matches!(
        e.handle(REFRESH, limits_form("1.25", "3", "1", "2"))
            .unwrap(),
        Command::Refresh
    ));
    assert!(e.requires_refresh);
    assert!(e
        .handle(SAVE_LIMITS, limits_form("1.25", "3", "1", "2"))
        .is_err());
    e.loaded(snapshot(false));
    assert_eq!(e.draft.parse().unwrap(), custom_limits());
    assert!(e.has_unsaved_changes());
    assert!(e
        .handle(SAVE_LIMITS, limits_form("1.25", "3", "1", "2"))
        .is_ok());
}
#[test]
fn invalidation_load_and_close_discard_consent_but_only_close_discards_dirty_draft() {
    let mut e = editor(true);
    e.handle(SAVE_LIMITS, limits_form("1.25", "3", "1", "2"))
        .unwrap();
    e.invalidate();
    assert!(e.review.is_none());
    assert!(e.handle(CONFIRM, acknowledged()).is_err());
    e.loaded(snapshot(true));
    assert_eq!(e.draft.parse().unwrap(), custom_limits());
    e.handle(SAVE_LIMITS, limits_form("1.25", "3", "1", "2"))
        .unwrap();
    let mut newer = snapshot(true);
    newer.identity_revision = Revision::new(8);
    e.loaded(newer);
    assert!(e.review.is_none());
    assert!(e.handle(CONFIRM, acknowledged()).is_err());
    assert!(matches!(
        e.handle(CLOSE, limits_form("1.25", "3", "1", "2")).unwrap(),
        Command::Close
    ));
    assert!(!e.has_unsaved_changes());
}
#[test]
fn clean_load_updates_defaults_but_invalid_nonempty_draft_survives_load() {
    let mut e = editor(true);
    let mut newer = snapshot(true);
    newer.limits = custom_limits();
    e.loaded(newer.clone());
    assert_eq!(e.draft.parse().unwrap(), custom_limits());
    assert!(!e.has_unsaved_changes());
    e.capture(limits_form("", "bad", "1", "2")).unwrap();
    e.loaded(newer);
    assert_eq!(e.draft.values[0], "");
    assert_eq!(e.draft.values[1], "bad");
    assert!(e.has_unsaved_changes());
    let view = e.view("", false);
    view.validate().unwrap();
    assert!(view.actions.len() <= 12);
    assert_eq!(view.fields.len(), 4);
}

#[test]
fn malformed_draft_can_be_cancelled_refreshed_or_used_with_switch_without_implicit_save() {
    for action in [REFRESH, DISABLE, CLOSE, DISCARD_LIMITS] {
        let mut e = editor(true);
        let result = e.handle(action, limits_form("bad", "", "1", "2")).unwrap();
        assert!(!matches!(result, Command::SetLimits { .. }));
        assert_eq!(e.snapshot.limits, VoiceLimits::default());
        if action == REFRESH || action == DISABLE {
            assert_eq!(e.draft.values[0], "bad");
            assert!(e.has_unsaved_changes());
        } else {
            assert!(!e.has_unsaved_changes());
        }
    }
}

// This projection is deliberately presentation-only: receipt minting and exact
// lease/four-version acceptance are tested by the backend device-check suite.
fn show_device(e: &mut Editor, result: VoiceDeviceCheckResult, expires: Instant) {
    e.device = Some(DeviceDisplay {
        result,
        observed_at: LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(),
            9 * 3600 + 8 * 60 + 7,
        )
        .unwrap(),
        expires,
        current: true,
    });
}

#[test]
fn output_check_accepts_only_empty_form_and_captures_all_loaded_versions() {
    let mut e = editor(false);
    e.capture(limits_form("1.25", "3", "1", "2")).unwrap();
    let before = e.draft.values.clone();
    for data in [limits_form("60", "4", "2", "10"), acknowledged(), {
        let mut data = FormData::new();
        data.insert(FieldId(999), "lease/version/path".into())
            .unwrap();
        data
    }] {
        assert!(e.handle(CHECK_OUTPUT, data).is_err());
        assert_eq!(e.draft.values, before);
        assert!(!e.is_reviewing());
    }
    assert!(
        matches!(act(&mut e, CHECK_OUTPUT).unwrap(), Command::CheckOutput {
        expected_publication, expected, expected_identity, expected_runtime,
    } if expected_publication == Revision::new(30)
        && expected == Revision::new(12)
        && expected_identity == Revision::new(7)
        && expected_runtime == 3)
    );
    assert_eq!(e.draft.values, before);
    assert!(e.has_unsaved_changes());
    // Neither checking nor its successful load saves or replaces the draft.
    e.loaded(snapshot(false));
    assert_eq!(e.draft.values, before);
    assert_eq!(e.snapshot.limits, VoiceLimits::default());
    assert!(e.has_unsaved_changes());
}

#[test]
fn output_check_does_not_replace_a_pending_confirmation_or_its_default_ack() {
    let mut e = editor(false);
    act(&mut e, ENABLE).unwrap();
    assert!(act(&mut e, CHECK_OUTPUT).is_err());
    assert!(e.handle(CHECK_OUTPUT, acknowledged()).is_err());
    assert!(e.is_reviewing());
    let view = e.view("", false);
    assert_eq!(view.fields[0].id, ACK);
    assert_eq!(view.fields[0].value, "0");
    assert!(!view.actions.iter().any(|action| action.id == CHECK_OUTPUT));
    assert!(act(&mut e, CONFIRM).is_err());
    act(&mut e, CANCEL).unwrap();
    assert!(matches!(
        act(&mut e, CHECK_OUTPUT).unwrap(),
        Command::CheckOutput { .. }
    ));
}

#[test]
fn output_observation_expires_at_original_boundary_without_automatic_query() {
    let mut e = editor(false);
    e.capture(limits_form("1.25", "3", "1", "2")).unwrap();
    let draft = e.draft.values.clone();
    let start = Instant::now();
    let boundary = start + std::time::Duration::from_secs(30);
    show_device(&mut e, VoiceDeviceCheckResult::Supported, boundary);
    assert!(!e.needs_tick(boundary - std::time::Duration::from_nanos(1), true));
    assert!(!e.tick(boundary - std::time::Duration::from_nanos(1), true));
    assert!(e
        .view("", false)
        .description
        .contains("上次检查：支持播报格式"));
    assert!(e.needs_tick(boundary, true));
    assert!(e.tick(boundary, true));
    assert!(!e.tick(boundary, true));
    let view = e.view("", false);
    assert!(view.description.contains("上次检查结果已过期，请重新检查"));
    assert!(!view.description.contains("上次检查：支持播报格式"));
    assert_eq!(e.draft.values, draft);
    assert!(
        !e.requires_refresh,
        "TTL expiry alone requires another explicit check, not a load"
    );
    assert!(matches!(
        act(&mut e, CHECK_OUTPUT).unwrap(),
        Command::CheckOutput { .. }
    ));
}

#[test]
fn output_observation_context_invalidation_preserves_draft_and_requires_a_load() {
    let mut e = editor(false);
    e.capture(limits_form("1.25", "3", "1", "2")).unwrap();
    let draft = e.draft.values.clone();
    let now = Instant::now();
    show_device(
        &mut e,
        VoiceDeviceCheckResult::Supported,
        now + std::time::Duration::from_secs(30),
    );
    assert_eq!(e.loaded_versions(), (Revision::new(30), 3));
    assert!(e.needs_tick(now, false));
    assert!(e.tick(now, false));
    assert!(!e.tick(now, false));
    assert_eq!(e.draft.values, draft);
    assert!(act(&mut e, CHECK_OUTPUT).is_err());
    let view = e.view("", false);
    assert!(view.description.contains("上次检查结果已过期"));
    assert!(
        !view
            .actions
            .iter()
            .find(|action| action.id == CHECK_OUTPUT)
            .unwrap()
            .enabled
    );
    assert!(matches!(act(&mut e, REFRESH).unwrap(), Command::Refresh));
    assert!(act(&mut e, CHECK_OUTPUT).is_err());
    e.loaded(snapshot(false));
    assert_eq!(e.draft.values, draft);
    assert!(matches!(
        act(&mut e, CHECK_OUTPUT).unwrap(),
        Command::CheckOutput { .. }
    ));
    assert!(
        !e.device.as_ref().unwrap().current,
        "loading without a valid receipt cannot revive history"
    );
}

#[test]
fn output_history_cannot_survive_a_version_change_as_current() {
    for change in 0..4 {
        let mut e = editor(false);
        show_device(
            &mut e,
            VoiceDeviceCheckResult::Supported,
            Instant::now() + std::time::Duration::from_secs(30),
        );
        let mut updated = snapshot(false);
        match change {
            0 => updated.publication_revision = Revision::new(31),
            1 => updated.identity_revision = Revision::new(8),
            2 => updated.revision = Revision::new(13),
            _ => updated.runtime_generation = 4,
        }
        e.loaded(updated);
        assert!(!e.device.as_ref().unwrap().current);
        assert!(e.view("", false).description.contains("上次检查结果已过期"));
        assert!(
            matches!(act(&mut e, CHECK_OUTPUT).unwrap(), Command::CheckOutput {
            expected_publication, expected_identity, expected, expected_runtime,
        } if expected_publication == Revision::new(if change == 0 {31} else {30})
            && expected_identity == Revision::new(if change == 1 {8} else {7})
            && expected == Revision::new(if change == 2 {13} else {12})
            && expected_runtime == if change == 3 {4} else {3})
        );
    }
}

#[test]
fn output_result_labels_are_historical_timed_and_never_assert_audibility() {
    for (result, expected) in [
        (
            VoiceDeviceCheckResult::Supported,
            "上次检查：支持播报格式（不代表实际有声）",
        ),
        (
            VoiceDeviceCheckResult::FormatUnsupported,
            "上次检查：默认输出不支持播报格式",
        ),
        (
            VoiceDeviceCheckResult::Unavailable,
            "上次检查：未发现可用默认输出",
        ),
        (
            VoiceDeviceCheckResult::Failed,
            "上次检查：检查失败，请稍后重新检查",
        ),
    ] {
        let mut e = editor(false);
        show_device(
            &mut e,
            result,
            Instant::now() + std::time::Duration::from_secs(30),
        );
        for busy in [false, true] {
            let view = e.view("", busy);
            view.validate().unwrap();
            assert!(view.actions.len() <= 12);
            assert!(view.description.contains(expected));
            assert!(view.description.contains("检查时间：2026-10-31 09:08:07"));
            assert!(view
                .description
                .contains("上次格式检查不代表设备当前可用或教室实际有声"));
            assert!(view.description.contains("不播放声音"));
            assert_eq!(
                view.actions
                    .iter()
                    .find(|action| action.id == CHECK_OUTPUT)
                    .unwrap()
                    .enabled,
                !busy
            );
        }
    }
}

#[test]
fn output_history_is_invalidated_by_save_close_or_lock_style_reset() {
    for action in [CONFIRM, CLOSE] {
        let mut e = editor(false);
        show_device(
            &mut e,
            VoiceDeviceCheckResult::Supported,
            Instant::now() + std::time::Duration::from_secs(30),
        );
        if action == CONFIRM {
            act(&mut e, ENABLE).unwrap();
            e.handle(CONFIRM, acknowledged()).unwrap();
        } else {
            act(&mut e, CLOSE).unwrap();
        }
        assert!(!e.device.as_ref().unwrap().current);
        assert!(e.snapshot.device_observation.is_none());
        assert!(e.requires_refresh);
    }
    // App::lock drops this editor entirely; a new session cannot inherit it.
    let mut e = editor(false);
    show_device(
        &mut e,
        VoiceDeviceCheckResult::Supported,
        Instant::now() + std::time::Duration::from_secs(30),
    );
    e.invalidate();
    assert!(!e.device.as_ref().unwrap().current);
    drop(e);
    assert!(editor(false).device.is_none());
}
