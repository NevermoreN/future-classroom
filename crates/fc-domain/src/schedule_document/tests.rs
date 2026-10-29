use super::*;
use crate::auth::*;
use crate::identity::{AssignmentScope, IdentityCommand, NewAccount, Rgb};

fn date(day: u8) -> LocalDate {
    LocalDate::new(2026, 10, day).unwrap()
}
fn now() -> LocalDateTime {
    LocalDateTime::new(date(26), 36000).unwrap()
}
fn principal(id: u64) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn row_id(id: u64) -> RowId {
    RowId::new(id).unwrap()
}
fn table(id: u64) -> TableId {
    TableId::new(id).unwrap()
}
fn term_id(id: u64) -> SemesterId {
    SemesterId::new(id).unwrap()
}
fn class() -> ClassId {
    ClassId::new(1).unwrap()
}
fn subject() -> SubjectId {
    SubjectId::new(1).unwrap()
}
fn actor() -> ActorContext {
    ActorContext {
        principal_id: principal(1),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(date(26), 40000).unwrap(),
        origin: TrustedOrigin::Local,
    }
}
fn identity() -> IdentityState {
    let mut state = IdentityState::default()
        .transition(
            &IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "学校".into(),
                id: principal(1),
                login_name: "admin".into(),
                display_name: "管理员".into(),
            },
            None,
            now(),
        )
        .unwrap()
        .next;
    for command in [
        IdentityCommand::CreateClass {
            id: class(),
            name: "一班".into(),
        },
        IdentityCommand::PutSubject {
            id: subject(),
            name: "数学".into(),
            enabled: true,
            color: Rgb {
                red: 1,
                green: 2,
                blue: 3,
            },
            expected_revision: None,
        },
        IdentityCommand::CreateAccount(NewAccount {
            id: principal(2),
            login_name: "teacher".into(),
            display_name: "教师".into(),
            role: Role::TeacherAdmin,
            scopes: vec![AssignmentScope {
                class_id: class(),
                subject_id: subject(),
            }],
        }),
    ] {
        state = state
            .transition(&command, Some(&actor()), now())
            .unwrap()
            .next;
    }
    state
}
fn cell() -> CourseCell {
    CourseCell {
        subject_id: subject(),
        teacher: TeacherBinding::Explicit(principal(2)),
    }
}
fn row(id: u64, start: u16) -> TimetableRow {
    TimetableRow {
        id: row_id(id),
        logical_key: LogicalRowKey::new(id).unwrap(),
        group: DayGroup::Morning,
        kind: RowKind::Course,
        range: MinuteRange::new(
            Minute::new(start).unwrap(),
            Minute::new(start + 45).unwrap(),
        )
        .unwrap(),
        enabled_days: [true, true, true, true, true, false, false],
        courses: std::array::from_fn(|_| Some(cell())),
    }
}
fn plan(id: u64, start: u8) -> SemesterPlan {
    SemesterPlan::new(
        Semester {
            id: term_id(id),
            class_id: class(),
            revision: Revision::default(),
            enabled: true,
            start: date(start),
            end: date(start + 4),
            first_week_monday: date(start),
            tables: vec![WeekTable {
                id: table(id),
                role: TableRole::Single,
                rows: vec![row(id * 10, 600), row(id * 10 + 1, 660)],
            }],
        },
        format!("学期{id}"),
    )
}
fn candidate() -> ScheduleDocument {
    let mut doc = ScheduleDocument::default();
    doc.semesters.push(plan(1, 12));
    doc
}
fn saved() -> ScheduleDocument {
    ScheduleDocument::default()
        .transition(
            &candidate(),
            Revision::default(),
            &identity(),
            &actor(),
            now(),
        )
        .unwrap()
}
fn publish(old: &ScheduleDocument, candidate: &ScheduleDocument) -> ScheduleDocument {
    old.transition(candidate, old.revision, &identity(), &actor(), now())
        .unwrap()
}
fn split(plan: &SemesterPlan) -> SemesterPlan {
    plan.enable_dual(EnableDual::Rebuild {
        odd_id: table(10),
        even_id: table(11),
        odd_rows: vec![row_id(100), row_id(101)],
        even_rows: vec![row_id(110), row_id(111)],
    })
    .unwrap()
}

#[test]
fn save_owns_all_revisions_and_supports_new_zero_revision_terms_in_existing_draft() {
    let mut draft = candidate();
    draft.revision = Revision::new(u64::MAX);
    draft.semesters[0].semester.revision = Revision::new(u64::MAX);
    draft.calendar_mode_revision = Revision::new(u64::MAX);
    draft.school_calendar.applied_revision = Revision::new(u64::MAX);
    let doc = ScheduleDocument::default()
        .transition(&draft, Revision::default(), &identity(), &actor(), now())
        .unwrap();
    assert_eq!(doc.revision, Revision::new(1));
    assert_eq!(doc.semesters[0].semester.revision, Revision::new(1));
    assert_eq!(doc.school_calendar.applied_revision, Revision::new(1));
    let mut forged = doc.clone();
    forged.semesters[0].semester.revision = Revision::new(99);
    forged.public_calendar.mapping_revision = Revision::new(99);
    let same = publish(&doc, &forged);
    assert_eq!(same.semesters[0].semester.revision, Revision::new(1));
    assert_eq!(same.public_calendar.mapping_revision, Revision::new(1));
    let mut draft = same.clone();
    draft.semesters.push(plan(20, 26));
    assert!(draft.validate_draft(&identity()).is_ok());
    assert!(draft.validate_structure().is_err());
    let next = publish(&same, &draft);
    assert_eq!(next.semesters[1].semester.revision, Revision::new(1));
    assert!(matches!(
        same.transition(&draft, Revision::default(), &identity(), &actor(), now())
            .unwrap_err()
            .kind,
        DocumentErrorKind::RevisionConflict { .. }
    ));
}
#[test]
fn content_and_rules_changes_change_the_joint_lesson_version_without_trusting_candidate_values() {
    let old = saved();
    let key = |doc: &ScheduleDocument| match resolve_lesson(
        &doc.resolve_day(class(), date(12)).unwrap(),
        LocalDateTime::new(date(12), 601 * 60).unwrap(),
        doc.automatic_gaps,
    ) {
        LessonState::Course { key, .. } => key,
        _ => panic!("course"),
    };
    let before = key(&old);
    let mut draft = old.clone();
    draft.semesters[0].semester.tables[0].rows[0].courses[0]
        .as_mut()
        .unwrap()
        .teacher = TeacherBinding::Unassigned;
    draft.semesters[0].semester.revision = old.semesters[0].semester.revision;
    let changed = publish(&old, &draft);
    assert_eq!(changed.semesters[0].semester.revision, Revision::new(2));
    assert_ne!(key(&changed), before);
    let mut draft = changed.clone();
    draft.automatic_gaps = false;
    draft.rules_revision = Revision::new(1);
    let rules = publish(&changed, &draft);
    assert_eq!(rules.rules_revision, Revision::new(2));
    assert_ne!(key(&rules), key(&changed));
}
#[test]
fn school_and_public_calendar_are_independent_and_mode_switch_is_versioned() {
    let doc = saved();
    let mut draft = doc.clone();
    draft.school_calendar.school_coverage = Some((date(12), date(30)));
    draft.school_calendar.entries.push(CalendarEntry {
        actual_semester_id: term_id(1),
        date: date(12),
        mapping: DayMapping::Rest,
    });
    draft.public_calendar.entries.push(CalendarEntry {
        actual_semester_id: term_id(1),
        date: date(13),
        mapping: DayMapping::Unconfirmed,
    });
    let public = publish(&doc, &draft);
    assert!(matches!(
        public.resolve_day(class(), date(12)).unwrap(),
        AcademicDay::Scheduled(_)
    ));
    let mut draft = public.clone();
    draft.calendar_mode = CalendarMode::School;
    let school = publish(&public, &draft);
    assert_eq!(school.calendar_mode_revision, Revision::new(2));
    assert_eq!(
        school.resolve_day(class(), date(12)).unwrap(),
        AcademicDay::Rest
    );
    let mut draft = school.clone();
    draft.public_calendar.public_verified_years.push(2026);
    let changed = publish(&school, &draft);
    assert_eq!(changed.school_calendar, school.school_calendar);
    assert_eq!(
        changed.public_calendar.applied_revision,
        school.public_calendar.applied_revision.next().unwrap()
    );
    assert_eq!(changed.calendar_mode, CalendarMode::School);
    let mut invalid = changed.clone();
    invalid.school_calendar.public_verified_years.push(2026);
    assert!(invalid.validate_draft_structure().is_err());
}
#[test]
fn makeup_uses_another_terms_base_table_tracks_both_revisions_and_rejects_cycles() {
    let mut draft = candidate();
    draft.semesters.push(plan(2, 26));
    draft.public_calendar.entries = vec![
        CalendarEntry {
            actual_semester_id: term_id(2),
            date: date(26),
            mapping: DayMapping::SourceDate(date(12)),
        },
        CalendarEntry {
            actual_semester_id: term_id(1),
            date: date(12),
            mapping: DayMapping::Rest,
        },
    ];
    let doc = publish(&ScheduleDocument::default(), &draft);
    let day = doc.resolve_day(class(), date(26)).unwrap();
    assert!(matches!(day, AcademicDay::Scheduled(_)));
    let mut draft = doc.clone();
    draft.semesters[0].name = "修改来源学期".into();
    let changed = publish(&doc, &draft);
    let (AcademicDay::Scheduled(old), AcademicDay::Scheduled(new)) =
        (day, changed.resolve_day(class(), date(26)).unwrap())
    else {
        panic!()
    };
    assert_eq!(old.actual_revision, new.actual_revision);
    assert_ne!(old.source_revision, new.source_revision);
    let mut draft = doc.clone();
    draft.public_calendar.entries[1].mapping = DayMapping::SourceDate(date(26));
    assert_eq!(
        draft.validate_draft_structure().unwrap_err().kind,
        DocumentErrorKind::MappingCycle
    );
    draft.public_calendar.entries[1].mapping = DayMapping::Rest;
    draft.public_calendar.entries[0].mapping = DayMapping::SourceDate(date(5));
    assert!(draft.validate_draft_structure().is_err());
    draft.public_calendar.entries[0].mapping = DayMapping::SourceDate(date(26));
    assert!(draft.validate_draft_structure().is_err());
}
#[test]
fn explicit_default_and_unassigned_teacher_choices_use_real_identity_relations() {
    let mut doc = candidate();
    let mut identity = identity();
    doc.semesters[0].semester.tables[0].rows[0].courses[0]
        .as_mut()
        .unwrap()
        .teacher = TeacherBinding::Default;
    assert!(doc.validate_draft(&identity).is_ok());
    identity = identity
        .transition(
            &IdentityCommand::CreateAccount(NewAccount {
                id: principal(3),
                login_name: "another".into(),
                display_name: "另一教师".into(),
                role: Role::TeacherAdmin,
                scopes: vec![AssignmentScope {
                    class_id: class(),
                    subject_id: subject(),
                }],
            }),
            Some(&actor()),
            now(),
        )
        .unwrap()
        .next;
    assert!(doc.validate_draft(&identity).is_err());
    identity
        .default_teachers
        .push(crate::identity::DefaultTeacher {
            class_id: class(),
            subject_id: subject(),
            teacher_id: principal(2),
        });
    assert!(doc.validate_draft(&identity).is_ok());
    doc.semesters[0].semester.tables[0].rows[0].courses[0]
        .as_mut()
        .unwrap()
        .teacher = TeacherBinding::Explicit(principal(1));
    let error = doc.validate_draft(&identity).unwrap_err();
    assert_eq!(error.kind, DocumentErrorKind::InvalidReference);
    assert_eq!(error.location.day, Some(Weekday::Monday));
    assert_eq!(error.location.row_id, Some(row_id(10)));
    doc.semesters[0].semester.tables[0].rows[0].courses[0]
        .as_mut()
        .unwrap()
        .teacher = TeacherBinding::Unassigned;
    assert!(doc.validate_draft(&identity).is_ok());
    identity.subjects[0].enabled = false;
    identity.default_teachers.clear();
    assert!(doc.validate_draft(&identity).is_err());
}
#[test]
fn double_table_history_and_metadata_are_deep_independent_and_restore_is_explicit() {
    let mut source = plan(1, 12);
    source.row_details.push(RowDetails {
        row_id: row_id(10),
        name: "第一节".into(),
    });
    source.cell_details.push(CellDetails {
        row_id: row_id(10),
        day: Weekday::Monday,
        room: "一教室".into(),
        note: "独立备注".into(),
    });
    let mut dual = split(&source);
    assert_eq!(dual.cell_details.len(), 2);
    dual.semester.tables[0].rows[0].courses[0] = None;
    dual.cell_details.retain(|d| d.row_id != row_id(100));
    let mut single = dual.disable_dual(table(11)).unwrap();
    assert_eq!(single.semester.tables[0].role, TableRole::Single);
    single.semester.tables[0].rows[0].courses[0] = None;
    single.cell_details.clear();
    let restored = single.enable_dual(EnableDual::RestoreHistory).unwrap();
    assert!(restored.dual_history.is_none());
    assert!(restored.semester.tables[0].rows[0].courses[0].is_none());
    assert!(restored.semester.tables[1].rows[0].courses[0].is_some());
    assert_eq!(restored.cell_details.len(), 1);
    let rebuilt = single
        .enable_dual(EnableDual::Rebuild {
            odd_id: table(20),
            even_id: table(21),
            odd_rows: vec![row_id(200), row_id(201)],
            even_rows: vec![row_id(210), row_id(211)],
        })
        .unwrap();
    assert!(rebuilt.dual_history.is_none());
    assert!(rebuilt
        .semester
        .tables
        .iter()
        .all(|t| t.rows[0].courses[0].is_none()));
    assert!(single
        .enable_dual(EnableDual::Rebuild {
            odd_id: table(10),
            even_id: table(11),
            odd_rows: vec![row_id(100), row_id(101)],
            even_rows: vec![row_id(110), row_id(111)]
        })
        .is_err());
}
#[test]
fn grid_edits_sync_structure_but_never_other_tables_cells_and_preserve_revision() {
    let mut doc = candidate();
    doc.semesters[0] = split(&doc.semesters[0]);
    let identity = identity();
    let cell_edit = DraftEdit::SetCell {
        semester_id: term_id(1),
        table_id: table(10),
        row_id: row_id(100),
        day: Weekday::Monday,
        cell: None,
    };
    let edited = doc.edit(&cell_edit, &identity).unwrap();
    assert!(edited.semesters[0].semester.tables[0].rows[0].courses[0].is_none());
    assert!(edited.semesters[0].semester.tables[1].rows[0].courses[0].is_some());
    let mut changed = edited.semesters[0].semester.tables[0].rows[0].clone();
    changed.range = MinuteRange::new(Minute::new(590).unwrap(), Minute::new(635).unwrap()).unwrap();
    let edited = edited
        .edit(
            &DraftEdit::SetRow {
                semester_id: term_id(1),
                table_id: table(10),
                row: changed,
            },
            &identity,
        )
        .unwrap();
    assert_eq!(
        edited.semesters[0].semester.tables[0].rows[0].range,
        edited.semesters[0].semester.tables[1].rows[0].range
    );
    assert!(edited.semesters[0].semester.tables[1].rows[0].courses[0].is_some());
    let mut inserted = row(500, 720);
    inserted.logical_key = LogicalRowKey::new(999).unwrap();
    let edited = edited
        .edit(
            &DraftEdit::InsertRow {
                semester_id: term_id(1),
                table_id: table(10),
                index: 2,
                row: inserted,
            },
            &identity,
        )
        .unwrap();
    assert_eq!(
        edited.semesters[0].semester.tables[0].rows[2].id,
        row_id(500)
    );
    assert_eq!(
        edited.semesters[0].semester.tables[1].rows[2].id,
        row_id(1000)
    );
    assert!(edited.semesters[0].semester.tables[1].rows[2]
        .courses
        .iter()
        .all(Option::is_none));
    let removed = edited
        .edit(
            &DraftEdit::RemoveRow {
                semester_id: term_id(1),
                table_id: table(10),
                row_id: row_id(500),
            },
            &identity,
        )
        .unwrap();
    assert!(removed.semesters[0]
        .semester
        .tables
        .iter()
        .all(|t| t.rows.len() == 2));
    assert_eq!(removed.revision, doc.revision);
    let moved = removed
        .edit(
            &DraftEdit::MoveRow {
                semester_id: term_id(1),
                table_id: table(10),
                row_id: row_id(100),
                index: 1,
            },
            &identity,
        )
        .unwrap();
    assert_eq!(
        moved.semesters[0].semester.tables[0].rows[1].id,
        row_id(100)
    );
    assert_eq!(
        moved.semesters[0].semester.tables[1].rows[1].id,
        row_id(110)
    );
}
#[test]
fn incomplete_overlaps_are_draftable_but_saving_returns_precise_location_atomically() {
    let doc = candidate();
    let mut bad = doc.semesters[0].semester.tables[0].rows[1].clone();
    bad.range = doc.semesters[0].semester.tables[0].rows[0].range;
    let draft = doc
        .edit(
            &DraftEdit::SetRow {
                semester_id: term_id(1),
                table_id: table(1),
                row: bad,
            },
            &identity(),
        )
        .unwrap();
    let err = ScheduleDocument::default()
        .transition(&draft, Revision::default(), &identity(), &actor(), now())
        .unwrap_err();
    assert_eq!(err.location.table_id, Some(table(1)));
    assert_eq!(err.location.day, Some(Weekday::Monday));
    assert_eq!(err.location.row_id, Some(row_id(11)));
    assert_eq!(
        doc.semesters[0].semester.tables[0].rows[1]
            .range
            .begin
            .get(),
        660
    );
    let mut empty = candidate();
    empty.semesters[0].semester.tables[0].rows.clear();
    assert!(empty.validate_draft(&identity()).is_ok());
}
#[test]
fn bounds_global_ids_permissions_and_overflow_are_enforced_without_mutation() {
    let mut doc = candidate();
    doc.semesters[0].semester.tables[0].rows = vec![row(10, 600); MAX_ROWS_PER_TABLE + 1];
    assert_eq!(
        doc.validate_draft_structure().unwrap_err().kind,
        DocumentErrorKind::Capacity
    );
    let mut doc = candidate();
    let mut second = plan(2, 26);
    second.semester.tables[0].rows[0].id = row_id(10);
    doc.semesters.push(second);
    assert!(doc.validate_draft_structure().is_err());
    let doc = saved();
    let mut web = actor();
    web.origin = TrustedOrigin::Web;
    assert!(doc
        .transition(&doc, doc.revision, &identity(), &web, now())
        .is_err());
    let mut max = doc.clone();
    max.revision = Revision::new(u64::MAX);
    assert_eq!(
        max.transition(&doc, max.revision, &identity(), &actor(), now())
            .unwrap_err()
            .kind,
        DocumentErrorKind::VersionOverflow
    );
    assert_eq!(doc.revision, Revision::new(1));
}
#[test]
fn allocation_counts_actual_nested_capacity_including_retained_history_and_metadata() {
    let mut doc = candidate();
    let base = doc.allocation_bytes().unwrap();
    let old_capacity = doc.semesters[0].name.capacity();
    doc.semesters[0].name.reserve(8192);
    let added = doc.semesters[0].name.capacity() - old_capacity;
    assert!(doc.allocation_bytes().unwrap() >= base + added);
    doc.semesters[0] = split(&doc.semesters[0]);
    doc.semesters[0] = doc.semesters[0].disable_dual(table(10)).unwrap();
    let before = doc.allocation_bytes().unwrap();
    let mut note = String::with_capacity(4096);
    note.push_str("备注");
    doc.semesters[0]
        .dual_history
        .as_mut()
        .unwrap()
        .cell_details
        .push(CellDetails {
            row_id: row_id(100),
            day: Weekday::Monday,
            room: String::with_capacity(1024),
            note,
        });
    assert!(doc.allocation_bytes().unwrap() >= before + 5120);
}

#[test]
fn sync_flag_requires_corresponding_structure_and_explicit_sync_preserves_cells() {
    let mut doc = candidate();
    doc.semesters[0] = split(&doc.semesters[0]);
    doc.semesters[0].timing_sync = false;
    doc.semesters[0].semester.tables[1].rows[0].range =
        MinuteRange::new(Minute::new(590).unwrap(), Minute::new(635).unwrap()).unwrap();
    doc.semesters[0].semester.tables[1].rows[0].courses[0] = None;
    assert!(doc.validate_draft(&identity()).is_ok());
    doc.semesters[0].timing_sync = true;
    assert_eq!(
        doc.validate_draft(&identity()).unwrap_err().kind,
        DocumentErrorKind::InvalidDraft
    );
    doc.semesters[0] = doc.semesters[0].synchronize_timing(table(10)).unwrap();
    assert!(doc.validate_draft(&identity()).is_ok());
    assert!(doc.semesters[0].semester.tables[1].rows[0].courses[0].is_none());
    let before = doc.clone();
    doc.semesters[0].semester.tables[1].rows.remove(0);
    let row = before.semesters[0].semester.tables[0].rows[0].clone();
    assert!(doc
        .edit(
            &DraftEdit::SetRow {
                semester_id: term_id(1),
                table_id: table(10),
                row
            },
            &identity()
        )
        .is_err());
}
#[test]
fn structural_receipts_remain_readable_when_current_identity_disables_a_teacher() {
    let doc = saved();
    let mut identity = identity();
    identity.accounts[1].principal.enabled = false;
    assert!(doc.validate_structure().is_ok());
    assert!(doc.validate(&identity).is_err());
    let mut draft = doc.clone();
    draft.semesters[0] = split(&draft.semesters[0]);
    draft.semesters[0] = draft.semesters[0].disable_dual(table(10)).unwrap();
    draft.semesters[0].dual_history.as_mut().unwrap().tables[1].rows[0].courses[0]
        .as_mut()
        .unwrap()
        .subject_id = SubjectId::new(99).unwrap();
    assert!(draft.validate_draft_structure().is_ok());
    assert!(draft
        .validate_draft(&crate::schedule_document::tests::identity())
        .is_err());
}
#[test]
fn component_overflow_and_a_forged_uncommitted_base_cannot_reset_versions() {
    let doc = saved();
    let mut base = doc.clone();
    base.semesters[0].semester.revision = Revision::new(u64::MAX);
    let mut changed = base.clone();
    changed.semesters[0].name = "更名".into();
    assert_eq!(
        base.transition(&changed, base.revision, &identity(), &actor(), now())
            .unwrap_err()
            .kind,
        DocumentErrorKind::VersionOverflow
    );
    let mut base = doc.clone();
    base.public_calendar.mapping_revision = Revision::new(u64::MAX);
    let mut changed = base.clone();
    changed.public_calendar.entries.push(CalendarEntry {
        actual_semester_id: term_id(1),
        date: date(13),
        mapping: DayMapping::Rest,
    });
    assert_eq!(
        base.transition(&changed, base.revision, &identity(), &actor(), now())
            .unwrap_err()
            .kind,
        DocumentErrorKind::VersionOverflow
    );
    let forged = candidate();
    assert_eq!(
        forged
            .transition(&forged, Revision::default(), &identity(), &actor(), now())
            .unwrap_err()
            .kind,
        DocumentErrorKind::InvalidDraft
    );
}

#[test]
fn deleted_published_row_and_logical_ids_stay_retired_after_save_and_reopen() {
    let old = saved();
    assert_eq!(old.id_high_water, 11);
    let mut draft = old.clone();
    draft.semesters[0].semester.tables[0].rows.remove(1);
    draft.id_high_water = 0;
    let committed = publish(&old, &draft);
    assert_eq!(committed.id_high_water, 11);
    // Reopening retains the persisted watermark; real SQLite round trips are
    // separately exercised by the storage integration suite.
    let mut reopened = committed.clone();
    let row_number = reopened.allocate_id().unwrap();
    let logical = reopened.allocate_id().unwrap();
    assert_eq!((row_number, logical), (12, 13));
    let mut added = row(row_number, 660);
    added.logical_key = LogicalRowKey::new(logical).unwrap();
    let candidate = reopened
        .edit(
            &DraftEdit::InsertRow {
                semester_id: term_id(1),
                table_id: table(1),
                index: 1,
                row: added,
            },
            &identity(),
        )
        .unwrap();
    let current = publish(&committed, &candidate);
    assert_eq!(current.id_high_water, 13);
    let mut reuse = current.clone();
    let mut retired = row(11, 720);
    retired.logical_key = LogicalRowKey::new(14).unwrap();
    reuse.semesters[0].semester.tables[0].rows.push(retired);
    assert_eq!(
        current
            .transition(&reuse, current.revision, &identity(), &actor(), now())
            .unwrap_err()
            .kind,
        DocumentErrorKind::RetiredIdentity
    );
    let mut reuse = current.clone();
    let mut retired = row(14, 720);
    retired.logical_key = LogicalRowKey::new(11).unwrap();
    reuse.semesters[0].semester.tables[0].rows.push(retired);
    assert_eq!(
        current
            .transition(&reuse, current.revision, &identity(), &actor(), now())
            .unwrap_err()
            .kind,
        DocumentErrorKind::RetiredIdentity
    );
    let mut forged = current.clone();
    forged.id_high_water = u64::MAX;
    assert_eq!(publish(&current, &forged).id_high_water, 13);
    let mut exhausted = current.clone();
    exhausted.id_high_water = u64::MAX;
    let before = exhausted.clone();
    assert_eq!(
        exhausted.allocate_id().unwrap_err().kind,
        DocumentErrorKind::VersionOverflow
    );
    assert_eq!(exhausted, before);
}
#[test]
fn deleting_an_entire_term_does_not_reopen_its_semester_or_table_identity() {
    let old = saved();
    let mut draft = old.clone();
    draft.semesters.clear();
    let reopened = publish(&old, &draft);
    assert_eq!(reopened.maximum_id(), 0);
    assert_eq!(reopened.id_high_water, 11);
    let mut reused = reopened.clone();
    reused.semesters.push(plan(1, 12));
    assert_eq!(
        reopened
            .transition(&reused, reopened.revision, &identity(), &actor(), now())
            .unwrap_err()
            .kind,
        DocumentErrorKind::RetiredIdentity
    );
    let mut reused = reopened.clone();
    let mut fresh = plan(12, 12);
    fresh.semester.tables[0].id = table(1);
    reused.semesters.push(fresh);
    let error = reopened
        .transition(&reused, reopened.revision, &identity(), &actor(), now())
        .unwrap_err();
    assert_eq!(error.kind, DocumentErrorKind::RetiredIdentity);
    assert_eq!(error.location.table_id, Some(table(1)));
}
#[test]
fn metadata_draft_guard_allows_unfinished_timing_but_rejects_poisoning_metadata() {
    let mut doc = candidate();
    let range = doc.semesters[0].semester.tables[0].rows[0].range;
    doc.semesters[0].semester.tables[0].rows[1].range = range;
    doc.semesters[0].semester.start = date(16);
    doc.semesters[0].semester.end = date(12);
    doc.semesters[0].cell_details.push(CellDetails {
        row_id: row_id(10),
        day: Weekday::Monday,
        room: "一教室".into(),
        note: "待确认作息时仍可编辑".into(),
    });
    assert!(doc.validate_draft_metadata().is_ok());
    assert!(doc.validate_draft_structure().is_err());
    doc.semesters[0].cell_details[0].note = "字".repeat(513);
    assert_eq!(
        doc.validate_draft_metadata().unwrap_err().kind,
        DocumentErrorKind::InvalidText
    );
    doc.semesters[0].cell_details[0].note.clear();
    doc.semesters[0].cell_details[0].row_id = row_id(999);
    assert_eq!(
        doc.validate_draft_metadata().unwrap_err().kind,
        DocumentErrorKind::InvalidMetadata
    );
    let details = CellDetails {
        row_id: row_id(10),
        day: Weekday::Monday,
        room: String::new(),
        note: String::new(),
    };
    doc.semesters[0].cell_details = vec![details; MAX_CELL_DETAILS + 1];
    assert_eq!(
        doc.validate_draft_metadata().unwrap_err().kind,
        DocumentErrorKind::Capacity
    );
}
