use super::tests::{fixture, form, set};
use super::*;

fn activity() -> (Editor, IdentityState) {
    let (mut editor, identity) = fixture();
    let row = &mut editor.document.semesters[0].semester.tables[0].rows[0];
    row.kind = RowKind::Activity;
    row.enabled_days = [true, false, true, false, true, false, false];
    editor.document.semesters[0].row_details.push(RowDetails {
        row_id: RowId::new(3).unwrap(),
        name: "晨间活动".into(),
    });
    editor.original = editor.document.clone();
    editor.page = Page::Grid;
    (editor, identity)
}
fn dual(editor: &mut Editor) {
    let next = editor.document.semesters[0]
        .enable_dual(EnableDual::Rebuild {
            odd_id: TableId::new(10).unwrap(),
            even_id: TableId::new(11).unwrap(),
            odd_rows: vec![RowId::new(12).unwrap()],
            even_rows: vec![RowId::new(13).unwrap()],
        })
        .unwrap();
    editor.replace_plan(next);
    editor.original = editor.document.clone();
    editor.dirty = false;
}
fn submit(editor: &mut Editor, identity: &IdentityState, action: ActionId) {
    let data = form(&editor.view(identity, "", false));
    editor.handle(action, data, identity).unwrap();
}

#[test]
fn swap_is_explicit_and_preserves_exact_physical_table_selection() {
    let (mut e, identity) = activity();
    dual(&mut e);
    let original = e.document.clone();
    submit(&mut e, &identity, SWAP_ROLES);
    e.view(&identity, "", false).validate().unwrap();
    assert_eq!(e.document, original);
    assert!(e.has_unsaved_changes());
    assert!(e.handle(SAVE_ALL, FormData::new(), &identity).is_err());
    assert!(e.select_cell(0, 0, &identity).is_err());
    submit(&mut e, &identity, REVIEW_CANCEL);
    assert!(!e.has_unsaved_changes());
    submit(&mut e, &identity, SWAP_ROLES);
    // Capturing a lock's empty form must retain the unconfirmed candidate.
    e.capture(FormData::new(), &identity);
    assert_eq!(e.page, Page::Review);
    assert_eq!(e.document, original);
    submit(&mut e, &identity, REVIEW_APPLY);
    assert_eq!(e.table, 0);
    assert_eq!(
        e.document.semesters[0].semester.tables[0].id,
        TableId::new(10).unwrap()
    );
    assert_eq!(
        e.document.semesters[0].semester.tables[0].role,
        TableRole::Even
    );
    let mut normalized = e.document.clone();
    for (new, old) in normalized.semesters[0]
        .semester
        .tables
        .iter_mut()
        .zip(&original.semesters[0].semester.tables)
    {
        new.role = old.role;
    }
    assert_eq!(normalized, original);
}

#[test]
fn partial_activity_grid_exposes_disabled_days_before_and_after_split() {
    let (mut e, identity) = activity();
    let row = e.view(&identity, "", false).grid.unwrap().rows.remove(0);
    assert!(!row.merged);
    assert_eq!(row.cells.len(), 7);
    assert_eq!(row.cells[1], "不适用");
    e.select_cell(0, 0, &identity).unwrap();
    let original = e.document.clone();
    submit(&mut e, &identity, SPLIT_ACTIVITY);
    assert!(e
        .view(&identity, "", false)
        .description
        .contains("3个逐日活动"));
    submit(&mut e, &identity, REVIEW_CANCEL);
    assert_eq!(e.document, original);
    assert!(!e.has_unsaved_changes());
    submit(&mut e, &identity, SPLIT_ACTIVITY);
    submit(&mut e, &identity, REVIEW_APPLY);
    let view = e.view(&identity, "", false);
    view.validate().unwrap();
    let rows = &view.grid.unwrap().rows;
    assert_eq!(rows.len(), 3);
    for (row, day) in rows.iter().zip([0, 2, 4]) {
        assert!(!row.merged);
        assert_eq!(
            row.cells.iter().filter(|s| s.as_str() != "不适用").count(),
            1
        );
        assert_eq!(row.cells[day], "晨间活动");
    }
}

#[test]
fn merge_preserves_original_masks_until_confirmation_and_cancel_restores_choice() {
    let (mut e, identity) = activity();
    e.document = e
        .document
        .split_activity(
            SemesterId::new(1).unwrap(),
            TableId::new(2).unwrap(),
            RowId::new(3).unwrap(),
        )
        .unwrap();
    e.select_cell(0, 0, &identity).unwrap();
    submit(&mut e, &identity, MERGE_ACTIVITY);
    let original = e.document.clone();
    let partner = original.semesters[0].semester.tables[0].rows[2]
        .id
        .get()
        .to_string();
    let mut data = form(&e.view(&identity, "", false));
    set(&mut data, MERGE_WITH, &partner);
    e.handle(APPLY, data, &identity).unwrap();
    assert_eq!(e.document, original);
    submit(&mut e, &identity, REVIEW_CANCEL);
    assert_eq!(e.page, Page::Merge(0));
    assert_eq!(e.view(&identity, "", false).fields[0].value, partner);
    submit(&mut e, &identity, APPLY);
    submit(&mut e, &identity, REVIEW_APPLY);
    assert_eq!(e.document.semesters[0].semester.tables[0].rows.len(), 2);
    assert_eq!(
        e.document.semesters[0].semester.tables[0].rows[0].enabled_days,
        [true, false, false, false, true, false, false]
    );
    e.document.validate_draft(&identity).unwrap();
}

#[test]
fn split_does_not_discard_unapplied_row_fields() {
    let (mut e, identity) = activity();
    e.select_cell(0, 0, &identity).unwrap();
    let original = e.document.clone();
    let mut data = form(&e.view(&identity, "", false));
    set(&mut data, ROW_NAME, "尚未应用的活动名");
    assert!(e.handle(SPLIT_ACTIVITY, data, &identity).is_err());
    assert_eq!(e.document, original);
    assert_eq!(e.page, Page::Row(Some(0)));
    assert_eq!(
        e.view(&identity, "", false)
            .fields
            .iter()
            .find(|f| f.id == ROW_NAME)
            .unwrap()
            .value,
        "尚未应用的活动名"
    );
}

#[test]
fn changing_course_kind_requires_loss_review_for_both_tables() {
    let (mut e, identity) = fixture();
    e.page = Page::Grid;
    let p = &mut e.document.semesters[0];
    p.semester.tables[0].rows[0].courses[0] = Some(CourseCell {
        subject_id: SubjectId::new(1).unwrap(),
        teacher: TeacherBinding::Explicit(PrincipalId::new(2).unwrap()),
    });
    p.cell_details.push(CellDetails {
        row_id: RowId::new(3).unwrap(),
        day: Weekday::Monday,
        room: "教室甲".into(),
        note: "保留原备注".into(),
    });
    dual(&mut e);
    let original = e.document.clone();
    e.select_cell(0, 0, &identity).unwrap();
    let mut data = form(&e.view(&identity, "", false));
    set(&mut data, KIND, "activity");
    set(&mut data, ROW_NAME, "本次确认的活动");
    e.handle(APPLY, data, &identity).unwrap();
    let view = e.view(&identity, "", false);
    view.validate().unwrap();
    for part in [
        "单周表",
        "双周表",
        "数学",
        "用户2",
        "教室3字",
        "备注5字",
        "本次目标：活动",
    ] {
        assert!(view.description.contains(part), "missing {part}");
    }
    assert_eq!(e.document, original);
    submit(&mut e, &identity, REVIEW_CANCEL);
    assert_eq!(e.document, original);
    assert_eq!(
        e.view(&identity, "", false)
            .fields
            .iter()
            .find(|f| f.id == KIND)
            .unwrap()
            .value,
        "activity"
    );
    submit(&mut e, &identity, APPLY);
    submit(&mut e, &identity, REVIEW_APPLY);
    for table in &e.document.semesters[0].semester.tables {
        assert_eq!(table.rows[0].kind, RowKind::Activity);
        assert!(table.rows[0].courses.iter().all(Option::is_none));
    }
    assert!(e.document.semesters[0].cell_details.is_empty());
    assert_eq!(
        e.document.semesters[0].semester.tables[0].rows[0].id,
        original.semesters[0].semester.tables[0].rows[0].id
    );
    assert_ne!(
        e.document.semesters[0].semester.tables[0].rows[0].logical_key,
        original.semesters[0].semester.tables[0].rows[0].logical_key
    );
    e.document.validate_draft(&identity).unwrap();
}

#[test]
fn loss_summary_remains_bounded_for_fourteen_large_cells() {
    let (mut e, mut identity) = fixture();
    identity.subjects[0].name = "科".repeat(64);
    identity.accounts[1].display_name = "师".repeat(128);
    let p = &mut e.document.semesters[0];
    p.semester.tables[0].rows[0].courses = std::array::from_fn(|_| {
        Some(CourseCell {
            subject_id: SubjectId::new(1).unwrap(),
            teacher: TeacherBinding::Explicit(PrincipalId::new(2).unwrap()),
        })
    });
    for (day, _) in days().into_iter().enumerate() {
        p.cell_details.push(CellDetails {
            row_id: RowId::new(3).unwrap(),
            day: Weekday::from_monday_index(day as u8).unwrap(),
            room: "室".repeat(64),
            note: "注".repeat(512),
        });
    }
    dual(&mut e);
    e.select_cell(0, 0, &identity).unwrap();
    let mut data = form(&e.view(&identity, "", false));
    set(&mut data, KIND, "lunch");
    e.handle(APPLY, data, &identity).unwrap();
    e.view(&identity, "", false).validate().unwrap();
    assert!(e.view(&identity, "", false).description.len() < 8192);
}
