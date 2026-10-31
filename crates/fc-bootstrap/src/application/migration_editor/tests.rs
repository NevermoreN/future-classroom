use super::*;
use fc_domain::{
    auth::Principal,
    identity::{Installation, Rgb, SchoolClass, SchoolSubject, UserAccount},
    ids::{AuthEpoch, SchoolId},
    recovery::{MigrationClassLabel, MigrationPrincipalLabel, MigrationSubjectLabel},
};

fn owner() -> PrincipalId {
    PrincipalId::new(11).unwrap()
}
fn inspection(classes: usize) -> Inspection {
    let source_school = SchoolId::new(1).unwrap();
    let target_school = SchoolId::new(2).unwrap();
    Inspection {
        source_catalog: MigrationCatalog {
            school_id: source_school,
            school_name: "Source school".into(),
            current_class: None,
            classes: (1..=classes)
                .map(|id| MigrationClassLabel {
                    id: ClassId::new(id as u64).unwrap(),
                    name: format!("Class {id}"),
                })
                .collect(),
            subjects: vec![MigrationSubjectLabel {
                id: SubjectId::new(1).unwrap(),
                name: "Subject".into(),
                color: Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
            }],
            principals: (1..=2)
                .map(|id| MigrationPrincipalLabel {
                    id: PrincipalId::new(id).unwrap(),
                    display_name: format!("Source account {id}"),
                })
                .collect(),
            default_teachers: vec![],
        },
        source_installation: [1; 32],
        source_revision: Revision::new(8),
        target_identity: IdentityState {
            revision: Revision::new(7),
            installation: Some(Installation {
                school_id: target_school,
                school_name: "Target school".into(),
                current_class: None,
            }),
            accounts: (11..=12)
                .map(|id| UserAccount {
                    principal: Principal {
                        id: PrincipalId::new(id).unwrap(),
                        school_id: target_school,
                        role: if id == 11 {
                            Role::GlobalAdmin
                        } else {
                            Role::User
                        },
                        enabled: true,
                        auth_epoch: AuthEpoch::new(1),
                    },
                    login_name: format!("target-{id}"),
                    display_name: format!("Target account {id}"),
                    must_change_password: false,
                    credential_version: 1,
                    recovery_version: 1,
                })
                .collect(),
            classes: (1..=classes)
                .map(|id| SchoolClass {
                    id: ClassId::new(id as u64).unwrap(),
                    school_id: target_school,
                    name: format!("Class {id}"),
                    enabled: true,
                })
                .collect(),
            subjects: vec![SchoolSubject {
                id: SubjectId::new(1).unwrap(),
                school_id: target_school,
                name: "Subject".into(),
                enabled: true,
                color: Rgb {
                    red: 4,
                    green: 5,
                    blue: 6,
                },
                revision: Revision::new(2),
            }],
            assignments: vec![],
            default_teachers: vec![],
        },
        target_installation: [2; 32],
        target_global_revision: Revision::new(42),
        selected_file: "C:\\synthetic\\source.fcbak".into(),
    }
}
fn editor() -> Editor {
    Editor::new(owner(), 20, inspection(6)).unwrap()
}
fn page_form(editor: &Editor, choose: bool) -> FormData {
    let mut form = FormData::new();
    for field in editor.view("", false).fields {
        if let FieldKind::Choice(choices) = field.kind {
            let value = if choose {
                let row = usize::from((field.id.0 - ROW_FIRST - 1) / 2);
                let page = &editor.pages[editor.page];
                let source = page.sources[row];
                let target = if matches!(page.kind, Kind::Principal) {
                    source + 10
                } else {
                    source
                };
                choices
                    .into_iter()
                    .find(|choice| choice.value == editor.token(page.kind, source, Some(target)))
                    .unwrap()
                    .value
            } else {
                field.value
            };
            form.insert(field.id, value).unwrap();
        }
    }
    form
}
fn advance(editor: &mut Editor, action: ActionId, choose: bool) -> Result<Command, String> {
    let form = page_form(editor, choose);
    editor.event(action, form)
}
fn pending() -> (Editor, u64) {
    let mut editor = editor();
    while editor.page + 1 < editor.pages.len() {
        advance(&mut editor, NEXT, true).unwrap();
    }
    let Command::Preflight {
        sequence,
        draft_revision,
        mapping,
    } = advance(&mut editor, PREFLIGHT, true).unwrap()
    else {
        panic!("complete explicit mapping expected");
    };
    assert_eq!(sequence, 20);
    assert_eq!(mapping.classes.len(), 6);
    assert_eq!(mapping.subjects.len(), 1);
    assert_eq!(mapping.principals.len(), 2);
    mapping
        .validate(
            &editor.inspection.source_catalog,
            &editor.inspection.target_identity,
        )
        .unwrap();
    (editor, draft_revision)
}
fn summary() -> ReviewSummary {
    ReviewSummary {
        source_revision: Revision::new(8),
        current_revision: Revision::new(42),
        document_count: 73,
        material_count: 3,
        material_bytes: 1234,
        profile_media_count: 2,
        profile_media_bytes: 5678,
        database_bytes: 9012,
    }
}
fn reviewed() -> (Editor, u64) {
    let (mut editor, revision) = pending();
    editor.set_review(owner(), 20, revision, summary()).unwrap();
    (editor, revision)
}
fn consent(editor: &Editor, ids: &[FieldId]) -> FormData {
    let mut form = FormData::new();
    for field in editor.view("", false).fields {
        if ids.contains(&field.id) {
            let FieldKind::Choice(choices) = field.kind else {
                panic!("explicit choice required");
            };
            form.insert(
                field.id,
                choices.into_iter().find(|c| c.value != "0").unwrap().value,
            )
            .unwrap();
        }
    }
    form
}
#[test]
fn equal_numeric_ids_and_labels_are_never_preselected_or_submitted_automatically() {
    let mut editor = editor();
    let view = editor.view("", false);
    view.validate().unwrap();
    assert_eq!(view.fields.len(), 15);
    for field in view.fields {
        if let FieldKind::Choice(choices) = field.kind {
            assert!(field.value.ends_with("-none"));
            assert_eq!(choices[0].value, field.value);
        }
    }
    assert!(editor.mapping.classes.is_empty());
    assert!(advance(&mut editor, PREFLIGHT, false).is_err());
    assert!(matches!(editor.stage, Stage::Draft));
    assert!(editor.mapping.classes.is_empty());
    let form = page_form(&editor, true);
    assert!(matches!(
        editor.event(SAVE_DRAFT, form).unwrap(),
        Command::Stay
    ));
    assert_eq!(editor.mapping.classes.len(), 5);
    assert!(matches!(editor.stage, Stage::Draft));
}
#[test]
fn old_blank_page_and_old_selected_page_events_are_rejected_without_changing_the_draft() {
    let mut editor = editor();
    let old_blank = page_form(&editor, false);
    advance(&mut editor, NEXT, false).unwrap();
    let mapping = editor.mapping.clone();
    let page = editor.page;
    assert!(editor.event(SAVE_DRAFT, old_blank).is_err());
    assert_eq!(editor.mapping, mapping);
    assert_eq!(editor.page, page);
    let selected = page_form(&editor, true);
    let duplicate = page_form(&editor, true);
    editor.event(SAVE_DRAFT, selected).unwrap();
    let mapping = editor.mapping.clone();
    assert!(editor.event(NEXT, duplicate).is_err());
    assert_eq!(editor.mapping, mapping);
    assert_eq!(editor.page, page);
}
#[test]
fn duplicate_targets_are_rejected_atomically_within_a_page_and_across_pages() {
    let mut editor = editor();
    let mut data = FormData::new();
    let page = &editor.pages[editor.page];
    for (row, source) in page.sources.iter().enumerate() {
        data.insert(
            FieldId(ROW_FIRST + (row as u16) * 2 + 1),
            editor.token(page.kind, *source, Some(1)),
        )
        .unwrap();
    }
    assert!(editor.event(NEXT, data).is_err());
    assert!(editor.mapping.classes.is_empty());
    assert_eq!(editor.page, 0);
    advance(&mut editor, NEXT, true).unwrap();
    let before = editor.mapping.clone();
    let mut duplicate = FormData::new();
    duplicate
        .insert(
            FieldId(ROW_FIRST + 1),
            editor.token(Kind::Class, 6, Some(1)),
        )
        .unwrap();
    assert!(editor.event(NEXT, duplicate).is_err());
    assert_eq!(editor.mapping, before);
    assert_eq!(editor.page, 1);
}
#[test]
fn unknown_missing_duplicate_and_forged_readonly_fields_cannot_change_mapping_or_metadata() {
    let mut editor = editor();
    let before = editor.mapping.clone();
    assert!(editor.event(SAVE_DRAFT, FormData::new()).is_err());
    let mut unknown = page_form(&editor, true);
    unknown
        .insert(SOURCE, "forged source installation".into())
        .unwrap();
    assert!(editor.event(NEXT, unknown).is_err());
    assert_eq!(editor.mapping, before);
    assert_eq!(editor.page, 0);
    let mut duplicate = page_form(&editor, true);
    assert_eq!(
        duplicate.insert(FieldId(ROW_FIRST + 1), "forged target".into()),
        Err(FormError::DuplicateField)
    );
    let mut forged = FormData::new();
    for field in editor.view("", false).fields {
        if matches!(field.kind, FieldKind::Choice(_)) {
            forged.insert(field.id, "1".into()).unwrap();
        }
    }
    assert!(editor.event(SAVE_DRAFT, forged).is_err());
    assert_eq!(editor.mapping, before);
    assert_eq!(editor.inspection.source_revision, Revision::new(8));
    assert_eq!(editor.inspection.target_global_revision, Revision::new(42));
}
#[test]
fn full_mapping_preflight_is_not_restore_consent_and_preview_is_bound_to_owner_and_both_revisions()
{
    let (mut editor, revision) = pending();
    assert!(editor.event(CONFIRM, FormData::new()).is_err());
    assert!(editor
        .set_review(PrincipalId::new(12).unwrap(), 20, revision, summary())
        .is_err());
    assert!(editor.set_review(owner(), 21, revision, summary()).is_err());
    assert!(editor
        .set_review(owner(), 20, revision + 1, summary())
        .is_err());
    let mut stale = summary();
    stale.current_revision = Revision::new(43);
    assert!(editor.set_review(owner(), 20, revision, stale).is_err());
    let mut stale = summary();
    stale.source_revision = Revision::new(9);
    assert!(editor.set_review(owner(), 20, revision, stale).is_err());
    editor.set_review(owner(), 20, revision, summary()).unwrap();
    assert!(editor.set_review(owner(), 20, revision, summary()).is_err());
    assert!(editor
        .view("", false)
        .fields
        .iter()
        .filter(|field| matches!(field.kind, FieldKind::Choice(_)))
        .all(|field| field.value == "0"));
}
#[test]
fn final_two_acknowledgements_are_explicit_single_use_and_bound_to_current_review_page() {
    let (mut editor, revision) = reviewed();
    for ids in [&[][..], &[ACK_REPLACEMENT][..], &[ACK_AUTHORITY][..]] {
        let form = consent(&editor, ids);
        assert!(editor.event(CONFIRM, form).is_err());
    }
    let mut unknown = consent(&editor, &[ACK_REPLACEMENT, ACK_AUTHORITY]);
    unknown.insert(TARGET, "forged target".into()).unwrap();
    assert!(editor.event(CONFIRM, unknown).is_err());
    let form = consent(&editor, &[ACK_REPLACEMENT, ACK_AUTHORITY]);
    assert!(
        matches!(editor.event(CONFIRM,form).unwrap(),Command::Confirm {sequence:20,draft_revision} if draft_revision==revision)
    );
    assert!(!editor.has_unsaved_changes());
    assert!(editor.event(CONFIRM, FormData::new()).is_err());
    assert!(editor.view("", false).actions.is_empty());
}
#[test]
fn lock_cancel_close_and_target_change_invalidation_never_revive_an_old_preview() {
    for action in [CANCEL, CLOSE, MANAGE_TARGETS] {
        let (mut editor, revision) = reviewed();
        editor.event(action, FormData::new()).unwrap();
        assert!(editor.set_review(owner(), 20, revision, summary()).is_err());
        assert!(!editor.has_unsaved_changes());
        assert!(editor.event(CONFIRM, FormData::new()).is_err());
    }
    let (mut editor, revision) = reviewed();
    let old = consent(&editor, &[ACK_REPLACEMENT, ACK_AUTHORITY]);
    editor.invalidate();
    assert!(editor.event(CONFIRM, old).is_err());
    editor.inspection_lost(owner(), 20).unwrap();
    assert!(matches!(editor.stage, Stage::Invalid));
    assert!(editor.set_review(owner(), 20, revision, summary()).is_err());
    let (mut pending, revision) = pending();
    for action in [
        PREVIOUS,
        NEXT,
        SAVE_DRAFT,
        PREFLIGHT,
        CONFIRM,
        CLOSE,
        MANAGE_TARGETS,
        PREVIOUS_PATH,
        NEXT_PATH,
    ] {
        assert!(pending.event(action, FormData::new()).is_err());
    }
    assert!(matches!(
        pending.event(CANCEL, FormData::new()).unwrap(),
        Command::Cancel
    ));
    assert!(pending
        .set_review(owner(), 20, revision, summary())
        .is_err());
}
#[test]
fn lost_physical_inspection_invalidates_the_draft_and_requires_a_new_selection() {
    let (mut editor, revision) = pending();
    assert!(editor.inspection_lost(owner(), 21).is_err());
    assert!(editor
        .inspection_lost(PrincipalId::new(12).unwrap(), 20)
        .is_err());
    assert!(matches!(editor.stage, Stage::Pending));
    editor.inspection_lost(owner(), 20).unwrap();
    assert!(matches!(editor.stage, Stage::Invalid));
    assert!(editor.set_review(owner(), 20, revision, summary()).is_err());
    for action in [SAVE_DRAFT, PREFLIGHT, NEXT, CONFIRM] {
        assert!(editor.event(action, FormData::new()).is_err());
    }
    assert!(matches!(
        editor.event(CLOSE, FormData::new()).unwrap(),
        Command::Close
    ));
    let mut reselected = Editor::new(owner(), 21, inspection(6)).unwrap();
    while reselected.page + 1 < reselected.pages.len() {
        advance(&mut reselected, NEXT, true).unwrap();
    }
    let previous_draft_revision = reselected.draft_revision;
    let Command::Preflight { draft_revision, .. } =
        advance(&mut reselected, PREFLIGHT, true).unwrap()
    else {
        panic!("new selection must explicitly preflight");
    };
    assert!(
        draft_revision > previous_draft_revision,
        "every explicit preflight advances its receipt binding"
    );
    assert!(reselected
        .set_review(owner(), 20, revision, summary())
        .is_err());
    assert!(reselected
        .set_review(owner(), 21, previous_draft_revision, summary())
        .is_err());
    reselected
        .set_review(owner(), 21, draft_revision, summary())
        .unwrap();
    let consent = consent(&reselected, &[ACK_REPLACEMENT, ACK_AUTHORITY]);
    reselected.inspection_lost(owner(), 21).unwrap();
    assert!(reselected.event(CONFIRM, consent).is_err());
}
#[test]
fn full_source_path_is_never_truncated_and_old_ack_does_not_survive_path_navigation() {
    let path = format!("C:\\{}\\source.fcbak", "校😀".repeat(1500));
    let mut input = inspection(6);
    input.selected_file = path.clone();
    let mut editor = Editor::new(owner(), 20, input).unwrap();
    let mut displayed = String::new();
    loop {
        let view = editor.view("", false);
        view.validate().unwrap();
        displayed.push_str(&view.fields.iter().find(|f| f.id == PATH).unwrap().value);
        if !view.actions.iter().any(|a| a.id == NEXT_PATH && a.enabled) {
            break;
        }
        advance(&mut editor, NEXT_PATH, false).unwrap();
    }
    assert_eq!(displayed, path);
    while editor.page + 1 < editor.pages.len() {
        advance(&mut editor, NEXT, true).unwrap();
    }
    let Command::Preflight { draft_revision, .. } = advance(&mut editor, PREFLIGHT, true).unwrap()
    else {
        panic!("full explicit preflight");
    };
    editor
        .set_review(owner(), 20, draft_revision, summary())
        .unwrap();
    let old = consent(&editor, &[ACK_REPLACEMENT, ACK_AUTHORITY]);
    editor.event(PREVIOUS_PATH, FormData::new()).unwrap();
    assert!(editor.event(CONFIRM, old).is_err());
    let new = consent(&editor, &[ACK_REPLACEMENT, ACK_AUTHORITY]);
    assert!(matches!(
        editor.event(CONFIRM, new).unwrap(),
        Command::Confirm { .. }
    ));
}
#[test]
fn native_view_budgets_and_busy_cancel_hold_for_full_target_account_capacity() {
    let mut input = inspection(6);
    let template = input.target_identity.accounts[1].clone();
    for id in 13..=522 {
        let mut account = template.clone();
        account.principal.id = PrincipalId::new(id).unwrap();
        account.login_name = format!("target-{id}");
        account.display_name = "😀".repeat(128);
        input.target_identity.accounts.push(account);
    }
    assert_eq!(input.target_identity.accounts.len(), 512);
    let mut editor = Editor::new(owner(), u64::MAX, input).unwrap();
    while !matches!(editor.pages[editor.page].kind, Kind::Principal) {
        advance(&mut editor, NEXT, false).unwrap();
    }
    let view = editor.view(&format!("\0\n\u{202e}{}", "😀".repeat(12000)), false);
    view.validate().unwrap();
    assert!(view.fields.len() <= 16 && view.actions.len() <= 12);
    assert!(view.status.len() <= 8192);
    assert!(!view.status.chars().any(|c| safe_char(c) != c));
    for field in &view.fields {
        if let FieldKind::Choice(choices) = &field.kind {
            assert_eq!(choices.len(), 513);
        }
    }
    let busy = editor.view("", true);
    busy.validate().unwrap();
    assert!(busy.actions.iter().any(|a| a.id == CANCEL && a.enabled));
    assert!(busy
        .actions
        .iter()
        .filter(|a| a.id != CANCEL)
        .all(|a| !a.enabled));
}
#[test]
fn complete_replacement_target_identity_authority_and_missing_target_workflow_are_explicit() {
    let (editor, _) = reviewed();
    let view = editor.view("", false);
    view.validate().unwrap();
    for text in [
        "不自动对应",
        "目标学校",
        "安装标识",
        "目录名称和颜色",
        "账号登录名和显示标签",
        "当前密码和恢复码",
        "角色",
        "停用状态",
        "任教权限及系统状态全部保留",
        "映射不会授予角色或任教权限",
        "全部旧教学数据整体替换",
        "未映射的目标目录本身保留",
        "其原有教学数据也会被替换",
        "默认教师",
        "不恢复旧会话",
        "发放新的初始口令",
        "正常主管理员流程",
        "正式资料不变",
        "重新启动并用目标当前账号登录",
    ] {
        // The first sentence deliberately includes the full matching qualifier.
        if text == "不自动对应" {
            assert!(view
                .description
                .contains("不会按相同数字编号或相同名称自动对应"));
        } else {
            assert!(view.description.contains(text), "missing {text}");
        }
    }
    assert!(view
        .fields
        .iter()
        .find(|f| f.id == SOURCE)
        .unwrap()
        .value
        .contains(&installation(&[1; 32])));
    assert!(view
        .fields
        .iter()
        .find(|f| f.id == TARGET)
        .unwrap()
        .value
        .contains("Target school"));
}
