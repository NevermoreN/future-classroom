use super::*;

fn sid() -> SemesterId {
    SemesterId::new(1).unwrap()
}
fn tid(id: u64) -> TableId {
    TableId::new(id).unwrap()
}
fn rid(id: u64) -> RowId {
    RowId::new(id).unwrap()
}
fn day(day: u8) -> LocalDate {
    LocalDate::new(2026, 10, day).unwrap()
}
fn activity() -> TimetableRow {
    TimetableRow {
        id: rid(20),
        logical_key: LogicalRowKey::new(30).unwrap(),
        group: DayGroup::Morning,
        kind: RowKind::Activity,
        range: MinuteRange::new(Minute::new(600).unwrap(), Minute::new(605).unwrap()).unwrap(),
        enabled_days: [true, true, true, true, true, false, false],
        courses: std::array::from_fn(|_| None),
    }
}
fn document(dual: bool) -> ScheduleDocument {
    let mut document = ScheduleDocument::default();
    let mut tables = vec![WeekTable {
        id: tid(10),
        role: if dual {
            TableRole::Odd
        } else {
            TableRole::Single
        },
        rows: vec![activity()],
    }];
    if dual {
        let mut row = activity();
        row.id = rid(21);
        tables.push(WeekTable {
            id: tid(11),
            role: TableRole::Even,
            rows: vec![row],
        });
    }
    let mut plan = SemesterPlan::new(
        Semester {
            id: sid(),
            class_id: ClassId::new(1).unwrap(),
            revision: Revision::default(),
            enabled: true,
            start: day(26),
            end: LocalDate::new(2026, 11, 8).unwrap(),
            first_week_monday: day(26),
            tables,
        },
        "合成学期".into(),
    );
    plan.row_details.push(RowDetails {
        row_id: rid(20),
        name: "晨间活动".into(),
    });
    if dual {
        plan.row_details.push(RowDetails {
            row_id: rid(21),
            name: "双周活动".into(),
        });
    }
    document.semesters.push(plan);
    document.id_high_water = 40;
    document
}
fn current_rows(document: &ScheduleDocument, table: usize) -> &Vec<TimetableRow> {
    &document.semesters[0].semester.tables[table].rows
}
fn row_ids(document: &ScheduleDocument, table: usize) -> Vec<RowId> {
    current_rows(document, table)
        .iter()
        .map(|row| row.id)
        .collect()
}
fn label(document: &ScheduleDocument, id: RowId) -> &str {
    document.semesters[0]
        .row_details
        .iter()
        .find(|detail| detail.row_id == id)
        .map(|detail| detail.name.as_str())
        .unwrap()
}
fn state(document: &ScheduleDocument, date: LocalDate, seconds: u32) -> LessonState {
    let resolved = document
        .resolve_day(ClassId::new(1).unwrap(), date)
        .unwrap();
    resolve_lesson(&resolved, LocalDateTime::new(date, seconds).unwrap(), true)
}
fn expect_failure(
    original: &ScheduleDocument,
    result: Result<ScheduleDocument, ScheduleDocumentError>,
    table: TableId,
    row: RowId,
) -> ScheduleDocumentError {
    let before = original.clone();
    let failure = result.unwrap_err();
    assert_eq!(failure.location.semester_id, Some(sid()));
    assert_eq!(failure.location.table_id, Some(table));
    assert_eq!(failure.location.row_id, Some(row));
    assert_eq!(*original, before);
    failure
}

#[test]
fn split_and_merge_preserve_actual_activity_days_and_labels_with_fresh_ids() {
    let original = document(false);
    let split = original.split_activity(sid(), tid(10), rid(20)).unwrap();
    assert_eq!(current_rows(&original, 0).len(), 1);
    assert_eq!(original.id_high_water, 40);
    assert_eq!(current_rows(&split, 0).len(), 5);
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    for (index, row) in current_rows(&split, 0).iter().enumerate() {
        assert!(row.id.get() > original.id_high_water);
        assert!(row.logical_key.get() > original.id_high_water);
        assert!(ids.insert(row.id.get()));
        assert!(keys.insert(row.logical_key.get()));
        assert_eq!(row.enabled_days, std::array::from_fn(|day| day == index));
        assert_eq!(row.range, activity().range);
        assert_eq!(row.group, activity().group);
        assert_eq!(label(&split, row.id), "晨间活动");
    }
    assert!(ids.is_disjoint(&keys));
    let mut selected = row_ids(&split, 0);
    selected.reverse();
    let merged = split.merge_activities(sid(), tid(10), &selected).unwrap();
    let row = &current_rows(&merged, 0)[0];
    assert_eq!(row.enabled_days, activity().enabled_days);
    assert!(row.id.get() > split.id_high_water);
    assert!(row.logical_key.get() > split.id_high_water);
    assert_eq!(label(&merged, row.id), "晨间活动");
    assert_eq!(merged.revision, original.revision);
    assert_eq!(
        merged.semesters[0].semester.revision,
        original.semesters[0].semester.revision
    );
    for date in [day(26), day(27), day(28), day(29), day(30)] {
        for candidate in [&original, &split, &merged] {
            assert_eq!(state(candidate, date, 600 * 60), LessonState::Activity);
            assert_eq!(state(candidate, date, 605 * 60), LessonState::Idle);
        }
    }
    assert_eq!(state(&merged, day(31), 600 * 60), LessonState::Idle);
}

#[test]
fn synchronized_split_and_merge_pair_keys_but_keep_each_tables_name_and_ids() {
    let original = document(true);
    // Start from the even table: target enumeration order must not select odd data.
    let split = original.split_activity(sid(), tid(11), rid(21)).unwrap();
    for (odd, even) in current_rows(&split, 0).iter().zip(current_rows(&split, 1)) {
        assert_eq!(odd.logical_key, even.logical_key);
        assert_ne!(odd.id, even.id);
        assert_eq!(odd.enabled_days, even.enabled_days);
        assert_eq!(label(&split, odd.id), "晨间活动");
        assert_eq!(label(&split, even.id), "双周活动");
    }
    let merged = split
        .merge_activities(sid(), tid(11), &row_ids(&split, 1))
        .unwrap();
    assert_eq!(current_rows(&merged, 0).len(), 1);
    assert_eq!(current_rows(&merged, 1).len(), 1);
    assert_eq!(
        current_rows(&merged, 0)[0].logical_key,
        current_rows(&merged, 1)[0].logical_key
    );
    assert_eq!(label(&merged, current_rows(&merged, 0)[0].id), "晨间活动");
    assert_eq!(label(&merged, current_rows(&merged, 1)[0].id), "双周活动");
}

#[test]
fn unsynchronized_operations_only_change_the_selected_table() {
    let mut original = document(true);
    original.semesters[0].timing_sync = false;
    let split = original.split_activity(sid(), tid(10), rid(20)).unwrap();
    assert_eq!(
        split.semesters[0].semester.tables[1],
        original.semesters[0].semester.tables[1]
    );
    let merged = split
        .merge_activities(sid(), tid(10), &row_ids(&split, 0))
        .unwrap();
    assert_eq!(
        merged.semesters[0].semester.tables[1],
        original.semesters[0].semester.tables[1]
    );
    let converted = original
        .convert_row_kind(sid(), tid(10), rid(20), RowKind::Course)
        .unwrap();
    assert_eq!(
        converted.semesters[0].semester.tables[1],
        original.semesters[0].semester.tables[1]
    );
}

#[test]
fn swap_only_changes_roles_and_actual_odd_even_resolution_then_swaps_back_exactly() {
    let mut original = document(true);
    original.semesters[0].timing_sync = false;
    original.semesters[0].semester.tables[1].rows[0].kind = RowKind::Rest(RestPurpose::Lunch);
    let even_date = LocalDate::new(2026, 11, 2).unwrap();
    assert_eq!(state(&original, day(26), 600 * 60), LessonState::Activity);
    assert_eq!(state(&original, even_date, 600 * 60), LessonState::Lunch);
    let swapped = original.swap_table_roles(sid()).unwrap();
    assert_eq!(state(&swapped, day(26), 600 * 60), LessonState::Lunch);
    assert_eq!(state(&swapped, even_date, 600 * 60), LessonState::Activity);
    let mut expected = original.clone();
    expected.semesters[0].semester.tables[0].role = TableRole::Even;
    expected.semesters[0].semester.tables[1].role = TableRole::Odd;
    assert_eq!(swapped, expected);
    assert_eq!(swapped.swap_table_roles(sid()).unwrap(), original);
}

#[test]
fn swap_rejects_single_duplicate_and_missing_roles_with_semester_location() {
    for mut source in [document(false), document(true)] {
        if source.semesters[0].semester.tables.len() == 2 {
            source.semesters[0].semester.tables[1].role = TableRole::Odd;
        }
        let failure = source.swap_table_roles(sid()).unwrap_err();
        assert_eq!(
            failure.kind,
            DocumentErrorKind::Schedule(ScheduleError::InvalidRoles)
        );
        assert_eq!(failure.location.semester_id, Some(sid()));
    }
    let failure = document(true)
        .swap_table_roles(SemesterId::new(99).unwrap())
        .unwrap_err();
    assert_eq!(failure.kind, DocumentErrorKind::NotFound);
    assert_eq!(
        failure.location.semester_id,
        Some(SemesterId::new(99).unwrap())
    );
}

#[test]
fn selected_sync_shape_mismatch_or_missing_partner_rejects_before_mutation() {
    for variant in 0..5 {
        let mut source = document(true);
        let other = &mut source.semesters[0].semester.tables[1].rows[0];
        match variant {
            0 => other.logical_key = LogicalRowKey::new(31).unwrap(),
            1 => other.enabled_days[0] = false,
            2 => other.range.end = Minute::new(606).unwrap(),
            3 => other.group = DayGroup::Afternoon,
            _ => other.kind = RowKind::Rest(RestPurpose::Other),
        }
        let before = source.clone();
        let failure = source.split_activity(sid(), tid(10), rid(20)).unwrap_err();
        assert_eq!(failure.kind, DocumentErrorKind::InvalidDraft);
        assert_eq!(failure.location.table_id, Some(tid(11)));
        assert!(source
            .convert_row_kind(sid(), tid(10), rid(20), RowKind::Course)
            .is_err());
        assert_eq!(source, before);
    }
}

#[test]
fn activity_split_refuses_nonactivity_and_less_than_two_enabled_days() {
    for variant in 0..4 {
        let mut source = document(false);
        let row = &mut source.semesters[0].semester.tables[0].rows[0];
        match variant {
            0 => row.enabled_days = [false; 7],
            1 => row.enabled_days = [true, false, false, false, false, false, false],
            2 => row.kind = RowKind::Course,
            _ => row.kind = RowKind::Rest(RestPurpose::Recess),
        }
        let failure = expect_failure(
            &source,
            source.split_activity(sid(), tid(10), rid(20)),
            tid(10),
            rid(20),
        );
        assert_eq!(failure.kind, DocumentErrorKind::InvalidDraft);
    }
}

#[test]
fn merge_rejects_lossy_name_time_group_and_weekday_combinations_and_bad_counts() {
    let split = document(false)
        .split_activity(sid(), tid(10), rid(20))
        .unwrap();
    for variant in 0..5 {
        let mut source = split.clone();
        let id = current_rows(&source, 0)[1].id;
        match variant {
            0 => {
                source.semesters[0]
                    .row_details
                    .iter_mut()
                    .find(|d| d.row_id == id)
                    .unwrap()
                    .name = "不同活动".into()
            }
            1 => {
                source.semesters[0].semester.tables[0].rows[1].range.end = Minute::new(606).unwrap()
            }
            2 => source.semesters[0].semester.tables[0].rows[1].group = DayGroup::Afternoon,
            3 => {
                source.semesters[0].semester.tables[0].rows[1].enabled_days =
                    [true, false, false, false, false, false, false]
            }
            _ => source.semesters[0].semester.tables[0].rows[1].enabled_days = [false; 7],
        }
        let ids = row_ids(&source, 0);
        let before = source.clone();
        let failure = source
            .merge_activities(sid(), tid(10), &ids[..2])
            .unwrap_err();
        assert_eq!(failure.kind, DocumentErrorKind::InvalidDraft);
        assert_eq!(failure.location.table_id, Some(tid(10)));
        assert_eq!(failure.location.row_id, Some(id));
        assert_eq!(source, before);
    }
    let ids = row_ids(&split, 0);
    for selected in [vec![], vec![ids[0]], vec![ids[0]; 2], vec![ids[0]; 8]] {
        assert!(split.merge_activities(sid(), tid(10), &selected).is_err());
    }
}

#[test]
fn another_tables_internal_name_conflict_rejects_joint_merge_atomically() {
    let mut split = document(true)
        .split_activity(sid(), tid(10), rid(20))
        .unwrap();
    let even_id = current_rows(&split, 1)[1].id;
    split.semesters[0]
        .row_details
        .iter_mut()
        .find(|d| d.row_id == even_id)
        .unwrap()
        .name = "不可丢弃".into();
    let before = split.clone();
    let failure = split
        .merge_activities(sid(), tid(10), &row_ids(&split, 0))
        .unwrap_err();
    assert_eq!(failure.location.table_id, Some(tid(11)));
    assert_eq!(failure.location.row_id, Some(even_id));
    assert_eq!(split, before);
}

#[test]
fn merge_uses_earliest_selected_position_and_preserves_unselected_order_and_names() {
    let mut split = document(false)
        .split_activity(sid(), tid(10), rid(20))
        .unwrap();
    let mut unselected = activity();
    unselected.id = rid(100);
    unselected.logical_key = LogicalRowKey::new(101).unwrap();
    unselected.range =
        MinuteRange::new(Minute::new(700).unwrap(), Minute::new(705).unwrap()).unwrap();
    split.semesters[0].semester.tables[0]
        .rows
        .insert(1, unselected.clone());
    split.semesters[0].semester.tables[0].rows.swap(0, 2);
    let ids = [current_rows(&split, 0)[5].id, current_rows(&split, 0)[2].id];
    let untouched: Vec<_> = current_rows(&split, 0)
        .iter()
        .filter(|r| !ids.contains(&r.id))
        .cloned()
        .collect();
    let merged = split.merge_activities(sid(), tid(10), &ids).unwrap();
    assert_eq!(current_rows(&merged, 0)[1], unselected);
    assert_eq!(
        current_rows(&merged, 0)[2].enabled_days,
        [true, false, false, false, true, false, false]
    );
    let retained: Vec<_> = current_rows(&merged, 0)
        .iter()
        .filter(|r| untouched.iter().any(|old| old.id == r.id))
        .cloned()
        .collect();
    assert_eq!(retained, untouched);
}

#[test]
fn conversion_retains_ids_and_names_but_retires_logical_key_and_clears_each_tables_courses() {
    let mut original = document(true);
    let plan = &mut original.semesters[0];
    for (index, table) in plan.semester.tables.iter_mut().enumerate() {
        table.rows[0].kind = RowKind::Course;
        table.rows[0].courses[0] = Some(CourseCell {
            subject_id: SubjectId::new(index as u64 + 1).unwrap(),
            teacher: TeacherBinding::Unassigned,
        });
        plan.cell_details.push(CellDetails {
            row_id: table.rows[0].id,
            day: Weekday::Monday,
            room: format!("合成教室{index}"),
            note: format!("必须确认才能移除{index}"),
        });
    }
    let converted = original
        .convert_row_kind(sid(), tid(10), rid(20), RowKind::Activity)
        .unwrap();
    assert!(converted.semesters[0].cell_details.is_empty());
    assert_eq!(original.semesters[0].cell_details.len(), 2);
    assert_eq!(
        converted.semesters[0].row_details,
        original.semesters[0].row_details
    );
    for (before, after) in original.semesters[0]
        .semester
        .tables
        .iter()
        .zip(&converted.semesters[0].semester.tables)
    {
        assert_eq!(before.id, after.id);
        assert_eq!(before.rows[0].id, after.rows[0].id);
        assert_ne!(before.rows[0].logical_key, after.rows[0].logical_key);
        assert!(after.rows[0].logical_key.get() > original.id_high_water);
        assert!(after.rows[0].courses.iter().all(Option::is_none));
        assert_eq!(after.rows[0].kind, RowKind::Activity);
    }
    assert_eq!(
        current_rows(&converted, 0)[0].logical_key,
        current_rows(&converted, 1)[0].logical_key
    );
    let restored_kind = converted
        .convert_row_kind(sid(), tid(10), rid(20), RowKind::Course)
        .unwrap();
    assert!(current_rows(&restored_kind, 0)[0]
        .courses
        .iter()
        .all(Option::is_none));
    assert!(restored_kind.semesters[0].cell_details.is_empty());
    assert!(original
        .convert_row_kind(sid(), tid(10), rid(20), RowKind::Course)
        .is_err());
}

#[test]
fn operations_preserve_retained_history_and_allocate_above_its_highest_identity() {
    let mut source = document(true);
    // A retained pair may share the physical identity of the selected current table.
    source.semesters[0].semester.tables[1].rows[0].id = rid(9000);
    source.semesters[0].row_details[1].row_id = rid(9000);
    source.semesters[0] = source.semesters[0].disable_dual(tid(10)).unwrap();
    let history = source.semesters[0].dual_history.clone();
    let split = source.split_activity(sid(), tid(10), rid(20)).unwrap();
    assert_eq!(split.semesters[0].dual_history, history);
    assert!(current_rows(&split, 0)
        .iter()
        .all(|row| row.id.get() > 9000 && row.logical_key.get() > 9000));
    let merged = split
        .merge_activities(sid(), tid(10), &row_ids(&split, 0))
        .unwrap();
    assert_eq!(merged.semesters[0].dual_history, history);
    let converted = source
        .convert_row_kind(sid(), tid(10), rid(20), RowKind::Course)
        .unwrap();
    assert_eq!(converted.semesters[0].dual_history, history);
    assert!(current_rows(&converted, 0)[0].logical_key.get() > 9000);
}

#[test]
fn operations_allow_unrelated_incomplete_timing_dates_and_calendar_drafts() {
    let mut source = document(true);
    let mut unfinished = activity();
    unfinished.id = rid(50);
    unfinished.logical_key = LogicalRowKey::new(60).unwrap();
    unfinished.range.end = unfinished.range.begin;
    for (index, table) in source.semesters[0].semester.tables.iter_mut().enumerate() {
        let mut row = unfinished.clone();
        row.id = rid(50 + index as u64);
        table.rows.push(row);
    }
    source.semesters[0].semester.end = day(25);
    source.school_calendar.school_coverage = Some((day(30), day(26)));
    assert!(source.validate_draft_structure().is_err());
    assert!(source.swap_table_roles(sid()).is_ok());
    let split = source.split_activity(sid(), tid(10), rid(20)).unwrap();
    let selected: Vec<_> = current_rows(&split, 0)
        .iter()
        .filter(|row| row.id != rid(50))
        .map(|row| row.id)
        .collect();
    assert!(split.merge_activities(sid(), tid(10), &selected).is_ok());
    assert!(source
        .convert_row_kind(sid(), tid(10), rid(20), RowKind::Course)
        .is_ok());
    assert_eq!(source.semesters[0].semester.tables[0].rows[1], unfinished);
}

#[test]
fn split_rejects_capacity_before_replacing_either_table() {
    let mut source = document(true);
    // The even table reaches its limit first. Its selected logical partner is valid.
    for index in 1..MAX_ROWS_PER_TABLE {
        let mut row = activity();
        row.id = rid(100 + index as u64);
        row.logical_key = LogicalRowKey::new(1000 + index as u64).unwrap();
        source.semesters[0].semester.tables[1].rows.push(row);
    }
    let before = source.clone();
    let failure = source.split_activity(sid(), tid(10), rid(20)).unwrap_err();
    assert_eq!(failure.kind, DocumentErrorKind::Capacity);
    assert_eq!(failure.location.table_id, Some(tid(11)));
    assert_eq!(source, before);
}

#[test]
fn overflow_after_partial_allocation_cannot_change_the_original_or_retire_its_rows() {
    let mut source = document(false);
    source.id_high_water = u64::MAX - 7;
    let before = source.clone();
    let failure = source.split_activity(sid(), tid(10), rid(20)).unwrap_err();
    assert_eq!(failure.kind, DocumentErrorKind::VersionOverflow);
    assert_eq!(failure.location.row_id, Some(rid(20)));
    assert_eq!(source, before);
    source.id_high_water = u64::MAX;
    let failure = source
        .convert_row_kind(sid(), tid(10), rid(20), RowKind::Course)
        .unwrap_err();
    assert_eq!(failure.kind, DocumentErrorKind::VersionOverflow);
    // Swapping roles does not allocate, even at the watermark limit.
    let mut dual = document(true);
    dual.id_high_water = u64::MAX;
    assert_eq!(
        dual.swap_table_roles(sid()).unwrap().id_high_water,
        u64::MAX
    );
}

#[test]
fn bad_source_rows_duplicate_identities_and_dangling_metadata_are_rejected() {
    for variant in 0..4 {
        let mut source = document(false);
        match variant {
            0 => {
                source.semesters[0].semester.tables[0].rows[0].range.end = Minute::new(599).unwrap()
            }
            1 => {
                source.semesters[0].semester.tables[0].rows[0].courses[0] = Some(CourseCell {
                    subject_id: SubjectId::new(1).unwrap(),
                    teacher: TeacherBinding::Unassigned,
                })
            }
            2 => {
                let duplicate = source.semesters[0].semester.tables[0].rows[0].clone();
                source.semesters[0].semester.tables[0].rows.push(duplicate);
            }
            _ => source.semesters[0].row_details.push(RowDetails {
                row_id: rid(999),
                name: "悬空".into(),
            }),
        }
        assert!(source.split_activity(sid(), tid(10), rid(20)).is_err());
        assert!(source
            .convert_row_kind(sid(), tid(10), rid(20), RowKind::Course)
            .is_err());
    }
    let source = document(false);
    assert_eq!(
        source
            .split_activity(sid(), tid(99), rid(20))
            .unwrap_err()
            .kind,
        DocumentErrorKind::NotFound
    );
    assert_eq!(
        source
            .split_activity(sid(), tid(10), rid(99))
            .unwrap_err()
            .kind,
        DocumentErrorKind::NotFound
    );
}

#[test]
fn partial_week_and_all_seven_days_are_exact_and_unnamed_activities_stay_unnamed() {
    for mask in [[true, false, true, false, false, false, false], [true; 7]] {
        let mut source = document(false);
        source.semesters[0].row_details.clear();
        source.semesters[0].semester.tables[0].rows[0].enabled_days = mask;
        let split = source.split_activity(sid(), tid(10), rid(20)).unwrap();
        assert_eq!(
            current_rows(&split, 0).len(),
            mask.iter().filter(|enabled| **enabled).count()
        );
        let merged = split
            .merge_activities(sid(), tid(10), &row_ids(&split, 0))
            .unwrap();
        assert_eq!(current_rows(&merged, 0)[0].enabled_days, mask);
        assert!(split.semesters[0].row_details.is_empty());
        assert!(merged.semesters[0].row_details.is_empty());
    }
}
