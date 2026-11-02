use super::*;
use crate::{auth::*, identity::*, schedule::*, schedule_document::*, time::*};
fn id(n: u64) -> PrincipalId {
    PrincipalId::new(n).unwrap()
}
fn class(n: u64) -> ClassId {
    ClassId::new(n).unwrap()
}
fn subject() -> SubjectId {
    SubjectId::new(1).unwrap()
}
fn date() -> LocalDate {
    LocalDate::new(2026, 10, 30).unwrap()
}
fn now(second: u32) -> LocalDateTime {
    LocalDateTime::new(date(), second).unwrap()
}
fn actor(identity: &IdentityState, n: u64) -> ActorContext {
    ActorContext {
        principal_id: id(n),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: identity.account(id(n)).unwrap().principal.auth_epoch,
        expires_at: now(86399),
        origin: TrustedOrigin::Local,
    }
}
fn identity() -> IdentityState {
    let mut state = IdentityState::default()
        .transition(
            &IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "学校".into(),
                id: id(1),
                login_name: "admin".into(),
                display_name: "管理".into(),
            },
            None,
            now(30000),
        )
        .unwrap()
        .next;
    for command in [
        IdentityCommand::CreateClass {
            id: class(1),
            name: "一班".into(),
        },
        IdentityCommand::CreateClass {
            id: class(2),
            name: "二班".into(),
        },
        IdentityCommand::PutSubject {
            id: subject(),
            name: "数学".into(),
            enabled: true,
            color: Rgb {
                red: 10,
                green: 20,
                blue: 30,
            },
            expected_revision: None,
        },
        IdentityCommand::BindCurrentClass { id: class(1) },
        IdentityCommand::CreateAccount(NewAccount {
            id: id(2),
            login_name: "teacher2".into(),
            display_name: "二老师".into(),
            role: Role::TeacherAdmin,
            scopes: vec![
                AssignmentScope {
                    class_id: class(1),
                    subject_id: subject(),
                },
                AssignmentScope {
                    class_id: class(2),
                    subject_id: subject(),
                },
            ],
        }),
        IdentityCommand::CreateAccount(NewAccount {
            id: id(3),
            login_name: "teacher3".into(),
            display_name: "三老师".into(),
            role: Role::TeacherAdmin,
            scopes: vec![AssignmentScope {
                class_id: class(1),
                subject_id: subject(),
            }],
        }),
        IdentityCommand::CreateAccount(NewAccount {
            id: id(4),
            login_name: "user4".into(),
            display_name: "用户".into(),
            role: Role::User,
            scopes: vec![],
        }),
    ] {
        let admin = actor(&state, 1);
        state = state
            .transition(&command, Some(&admin), now(30000))
            .unwrap()
            .next;
    }
    for n in [2, 3, 4] {
        let a = actor(&state, n);
        state = state
            .transition(
                &IdentityCommand::PasswordChanged {
                    id: id(n),
                    expected_credential_version: 1,
                },
                Some(&a),
                now(30000),
            )
            .unwrap()
            .next;
    }
    state
}
fn scope() -> MaterialScope {
    MaterialScope {
        teacher_id: id(2),
        subject_id: subject(),
    }
}
fn empty() -> MaterialFolderState {
    MaterialFolderState::empty(FolderId::new(1).unwrap(), scope())
}
fn append(n: u64) -> MaterialAppend {
    MaterialAppend {
        material_id: MaterialId::new(n).unwrap(),
        display_name: "同名资料.txt".into(),
        resource: MaterialResource {
            version_id: ResourceVersionId::new(n).unwrap(),
            sha256: [n as u8; 32],
            kind: MaterialKind::Txt,
            bytes: 100,
        },
        allowed_class_ids: vec![class(1)],
    }
}
fn saved(identity: &IdentityState) -> MaterialFolderState {
    empty()
        .append(&append(1), identity, &actor(identity, 2), now(30000))
        .unwrap()
}
fn schedule_fixture(identity: &IdentityState) -> ScheduleDocument {
    let range = |begin, end| {
        MinuteRange::new(Minute::new(begin).unwrap(), Minute::new(end).unwrap()).unwrap()
    };
    let row = |n, kind, begin, end, teacher: Option<PrincipalId>| TimetableRow {
        id: RowId::new(n).unwrap(),
        logical_key: LogicalRowKey::new(n + 100).unwrap(),
        group: DayGroup::Morning,
        kind,
        range: range(begin, end),
        enabled_days: [true; 7],
        courses: std::array::from_fn(|_| {
            teacher.map(|teacher| CourseCell {
                subject_id: subject(),
                teacher: TeacherBinding::Explicit(teacher),
            })
        }),
    };
    let semester = Semester {
        id: SemesterId::new(1).unwrap(),
        class_id: class(1),
        revision: Revision::default(),
        enabled: true,
        start: LocalDate::new(2026, 10, 26).unwrap(),
        end: LocalDate::new(2026, 10, 31).unwrap(),
        first_week_monday: LocalDate::new(2026, 10, 26).unwrap(),
        tables: vec![WeekTable {
            id: TableId::new(2).unwrap(),
            role: TableRole::Single,
            rows: vec![
                row(10, RowKind::Course, 480, 520, Some(id(2))),
                row(11, RowKind::Rest(RestPurpose::Recess), 520, 530, None),
                row(12, RowKind::Course, 530, 570, Some(id(3))),
            ],
        }],
    };
    let mut candidate = ScheduleDocument::default();
    candidate
        .semesters
        .push(SemesterPlan::new(semester, "秋季".into()));
    ScheduleDocument::default()
        .transition(
            &candidate,
            Revision::default(),
            identity,
            &actor(identity, 1),
            now(30000),
        )
        .unwrap()
}
#[test]
fn independent_appends_commit_order_even_with_same_name_and_wall_clock_rollback() {
    let identity = identity();
    let a = append(1);
    let b = append(2);
    let first = empty()
        .append(&a, &identity, &actor(&identity, 2), now(30000))
        .unwrap();
    let second = first
        .append(&b, &identity, &actor(&identity, 2), now(29000))
        .unwrap();
    assert_eq!(second.folder.revision, Revision::new(2));
    assert_eq!(second.records[0].revision, Revision::new(1));
    let page = second
        .management_page(&identity, &actor(&identity, 2), now(31000), None)
        .unwrap();
    assert_eq!(
        page.items.iter().map(|r| r.id.get()).collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert!(page.items[0].added_at < page.items[1].added_at);
    assert_eq!(
        page.items[0].resource.version_id,
        ResourceVersionId::new(2).unwrap()
    );
}
#[test]
fn teachers_cannot_cross_owner_or_class_and_users_cannot_write() {
    let identity = identity();
    assert!(matches!(
        empty().append(&append(1), &identity, &actor(&identity, 3), now(30000)),
        Err(MaterialError::Permission(PermissionError::WrongOwner))
    ));
    assert!(matches!(
        empty().append(&append(1), &identity, &actor(&identity, 4), now(30000)),
        Err(MaterialError::Permission(PermissionError::RoleDenied))
    ));
    let mut wrong = append(1);
    wrong.allowed_class_ids = vec![class(3)];
    assert_eq!(
        empty().append(&wrong, &identity, &actor(&identity, 1), now(30000)),
        Err(MaterialError::InvalidScope)
    );
    let first = saved(&identity);
    assert!(
        matches!(first.delete(MaterialId::new(1).unwrap(),Revision::new(2),&identity,&actor(&identity,2),now(30000)),Err(MaterialError::RevisionConflict{current})if current==Revision::new(1))
    );
}
#[test]
fn delete_is_target_cas_and_does_not_depend_on_unrelated_append() {
    let identity = identity();
    let first = saved(&identity);
    let added = first
        .append(&append(2), &identity, &actor(&identity, 2), now(31000))
        .unwrap();
    let deleted = added
        .delete(
            MaterialId::new(1).unwrap(),
            Revision::new(1),
            &identity,
            &actor(&identity, 2),
            now(32000),
        )
        .unwrap();
    assert_eq!(deleted.folder.last_added_sequence, 2);
    assert_eq!(deleted.records[0].status, MaterialStatus::Deleted);
    assert_eq!(deleted.records[0].revision, Revision::new(2));
    assert_eq!(
        deleted
            .management_page(&identity, &actor(&identity, 2), now(32000), None)
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        deleted.append(&append(1), &identity, &actor(&identity, 2), now(32000)),
        Err(MaterialError::AlreadyExists)
    );
}
#[test]
fn management_twenty_desktop_five_and_cursor_is_folder_source_and_revision_bound() {
    let identity = identity();
    let mut state = empty();
    for n in 1..=21 {
        state = state
            .append(&append(n), &identity, &actor(&identity, 2), now(30000))
            .unwrap();
    }
    let management = state
        .management_page(&identity, &actor(&identity, 2), now(30000), None)
        .unwrap();
    assert_eq!(management.items.len(), 20);
    let source = resolve_material_source(
        &identity,
        &schedule_fixture(&identity),
        class(1),
        now(30000),
    )
    .unwrap()
    .unwrap();
    let desktop = state.published_page(&identity, &source, None).unwrap();
    assert_eq!(desktop.items.len(), 5);
    assert_eq!(desktop.items[0].id.get(), 21);
    let cursor = desktop.next_cursor.clone().unwrap();
    let second = state
        .published_page(&identity, &source, Some(&cursor))
        .unwrap();
    assert_eq!(second.items[0].id.get(), 16);
    let mutated = state
        .append(&append(22), &identity, &actor(&identity, 2), now(30000))
        .unwrap();
    assert_eq!(
        mutated.published_page(&identity, &source, Some(&cursor)),
        Err(MaterialError::StaleCursor)
    );
    let mut cross = cursor;
    cross.class_id = Some(class(2));
    assert_eq!(
        state.published_page(&identity, &source, Some(&cross)),
        Err(MaterialError::StaleCursor)
    );
}
#[test]
fn current_recess_and_next_course_switch_exact_teacher_without_borrowing() {
    let identity = identity();
    let schedule = schedule_fixture(&identity);
    let first = resolve_material_source(&identity, &schedule, class(1), now(8 * 3600 + 35 * 60))
        .unwrap()
        .unwrap();
    assert_eq!(first.teacher_id, id(2));
    assert_eq!(first.kind, MaterialSourceKind::CurrentLesson);
    let recess = resolve_material_source(&identity, &schedule, class(1), now(8 * 3600 + 45 * 60))
        .unwrap()
        .unwrap();
    assert_eq!(recess.teacher_id, id(2));
    assert_eq!(recess.lesson, first.lesson);
    assert_eq!(recess.kind, MaterialSourceKind::RetainedBreak);
    assert_eq!(recess.evidence, MaterialSourceEvidence::BaseTimetableOnly);
    let next = resolve_material_source(&identity, &schedule, class(1), now(8 * 3600 + 50 * 60))
        .unwrap()
        .unwrap();
    assert_eq!(next.teacher_id, id(3));
    assert_ne!(first.lesson, next.lesson);
    assert_eq!(
        empty().published_page(&identity, &next, None),
        Err(MaterialError::InvalidContext)
    );
    assert!(
        resolve_material_source(&identity, &schedule, class(1), now(12 * 3600))
            .unwrap()
            .is_none()
    );
}
#[test]
fn unassigned_does_not_silently_choose_a_teacher_and_activity_clears_recess_source() {
    let identity = identity();
    let mut schedule = schedule_fixture(&identity);
    for cell in schedule.semesters[0].semester.tables[0].rows[0]
        .courses
        .iter_mut()
        .flatten()
    {
        cell.teacher = TeacherBinding::Unassigned;
    }
    assert!(
        resolve_material_source(&identity, &schedule, class(1), now(30000))
            .unwrap()
            .is_none()
    );
    let mut schedule = schedule_fixture(&identity);
    let table = &mut schedule.semesters[0].semester.tables[0];
    table.rows[0].range.end = Minute::new(515).unwrap();
    let mut activity = table.rows[0].clone();
    activity.id = RowId::new(50).unwrap();
    activity.logical_key = LogicalRowKey::new(150).unwrap();
    activity.kind = RowKind::Activity;
    activity.range =
        MinuteRange::new(Minute::new(515).unwrap(), Minute::new(520).unwrap()).unwrap();
    activity.courses = std::array::from_fn(|_| None);
    table.rows.push(activity);
    schedule.update_id_high_water();
    assert!(
        resolve_material_source(&identity, &schedule, class(1), now(8 * 3600 + 45 * 60))
            .unwrap()
            .is_none()
    );
}
#[test]
fn names_type_size_bounds_and_sequence_overflow_fail_without_mutation() {
    for name in [
        "../x.txt",
        "CON.txt",
        "COM1.docx",
        "LPT¹.txt",
        "CON .txt",
        "file.exe",
        "a.docm",
        "file.txt ",
        "x\0.txt",
    ] {
        assert!(validate_material_name(name).is_err(), "{name}");
    }
    let identity = identity();
    let mut input = append(1);
    input.resource.kind = MaterialKind::Pdf;
    assert_eq!(
        empty().append(&input, &identity, &actor(&identity, 2), now(30000)),
        Err(MaterialError::InvalidResource)
    );
    let mut state = saved(&identity);
    state.folder.revision = Revision::new(u64::MAX);
    state.folder.last_added_sequence = u64::MAX;
    assert_eq!(
        state.append(&append(2), &identity, &actor(&identity, 2), now(30000)),
        Err(MaterialError::VersionOverflow)
    );
}

#[test]
fn revocation_hides_published_access_but_global_can_cleanup_retained_metadata() {
    let mut identity = identity();
    let state = saved(&identity);
    let admin = actor(&identity, 1);
    identity = identity
        .transition(
            &IdentityCommand::UpdateAccount {
                id: id(2),
                display_name: "已停用".into(),
                role: Role::TeacherAdmin,
                enabled: false,
                scopes: vec![
                    AssignmentScope {
                        class_id: class(1),
                        subject_id: subject(),
                    },
                    AssignmentScope {
                        class_id: class(2),
                        subject_id: subject(),
                    },
                ],
            },
            Some(&admin),
            now(31000),
        )
        .unwrap()
        .next;
    state.validate().unwrap();
    assert!(state
        .management_page(&identity, &actor(&identity, 2), now(31000), None)
        .is_err());
    let cleaned = state
        .delete(
            MaterialId::new(1).unwrap(),
            Revision::new(1),
            &identity,
            &actor(&identity, 1),
            now(31000),
        )
        .unwrap();
    assert_eq!(cleaned.records[0].status, MaterialStatus::Deleted);
}

#[test]
fn default_teacher_is_used_only_for_explicit_default_binding() {
    let mut identity = identity();
    let admin = actor(&identity, 1);
    identity = identity
        .transition(
            &IdentityCommand::SetDefaultTeacher(DefaultTeacher {
                class_id: class(1),
                subject_id: subject(),
                teacher_id: id(2),
            }),
            Some(&admin),
            now(30000),
        )
        .unwrap()
        .next;
    let mut schedule = schedule_fixture(&identity);
    for cell in schedule.semesters[0].semester.tables[0].rows[0]
        .courses
        .iter_mut()
        .flatten()
    {
        cell.teacher = TeacherBinding::Default;
    }
    assert_eq!(
        resolve_material_source(&identity, &schedule, class(1), now(30000))
            .unwrap()
            .unwrap()
            .teacher_id,
        id(2)
    );
    for cell in schedule.semesters[0].semester.tables[0].rows[0]
        .courses
        .iter_mut()
        .flatten()
    {
        cell.teacher = TeacherBinding::Explicit(id(3));
    }
    assert_eq!(
        resolve_material_source(&identity, &schedule, class(1), now(30000))
            .unwrap()
            .unwrap()
            .teacher_id,
        id(3)
    );
}

#[test]
fn bounded_batch_is_one_candidate_with_input_order_and_unchanged_single_append_semantics() {
    let identity = identity();
    let owner = actor(&identity, 2);
    let initial = empty()
        .append(&append(100), &identity, &owner, now(30000))
        .unwrap();
    let batch = vec![append(7), append(3), append(9)];
    let next = initial
        .append_batch(&batch, &identity, &owner, now(29000))
        .unwrap();
    next.validate().unwrap();
    assert_eq!(initial.records.len(), 1);
    assert_eq!(next.folder.revision, Revision::new(2));
    assert_eq!(next.folder.last_added_sequence, 4);
    assert_eq!(next.records[0], initial.records[0]);
    for (offset, (record, input)) in next.records[1..].iter().zip(&batch).enumerate() {
        assert_eq!(record.id, input.material_id);
        assert_eq!(record.resource, input.resource);
        assert_eq!(record.allowed_class_ids, input.allowed_class_ids);
        assert_eq!(record.added_sequence, offset as u64 + 2);
        assert_eq!(record.added_at, now(29000));
        assert_eq!(record.revision, Revision::new(1));
    }
    let page = next
        .management_page(&identity, &owner, now(31000), None)
        .unwrap();
    assert_eq!(
        page.items.iter().map(|r| r.id.get()).collect::<Vec<_>>(),
        vec![9, 3, 7, 100]
    );
    let single = next
        .append(&append(11), &identity, &owner, now(28000))
        .unwrap();
    assert_eq!(single.folder.revision, Revision::new(3));
    assert_eq!(single.folder.last_added_sequence, 5);
    assert_eq!(single.records.last().unwrap().revision, Revision::new(1));
    single.validate().unwrap();
}

#[test]
fn batch_rejects_a_bad_last_item_without_changing_any_original_record() {
    let identity = identity();
    let owner = actor(&identity, 2);
    let state = saved(&identity);
    let before = state.clone();
    let mut invalid = append(3);
    invalid.resource.bytes = 0;
    assert_eq!(
        state.append_batch(&[append(2), invalid], &identity, &owner, now(31000)),
        Err(MaterialError::InvalidResource)
    );
    let mut wrong_class = append(3);
    wrong_class.allowed_class_ids = vec![class(3)];
    assert_eq!(
        state.append_batch(
            &[append(2), wrong_class],
            &identity,
            &actor(&identity, 1),
            now(31000)
        ),
        Err(MaterialError::InvalidScope)
    );
    assert!(matches!(
        state.append_batch(&[append(2)], &identity, &actor(&identity, 3), now(31000)),
        Err(MaterialError::Permission(PermissionError::WrongOwner))
    ));
    assert!(matches!(
        state.append_batch(&[append(2)], &identity, &actor(&identity, 4), now(31000)),
        Err(MaterialError::Permission(PermissionError::RoleDenied))
    ));
    assert_eq!(state, before);
}

#[test]
fn batch_rejects_duplicate_material_and_resource_versions_within_and_before_batch() {
    let identity = identity();
    let owner = actor(&identity, 2);
    let state = saved(&identity);
    let before = state.clone();
    let mut repeated_version = append(3);
    repeated_version.resource.version_id = ResourceVersionId::new(2).unwrap();
    for batch in [
        vec![append(2), append(2)],
        vec![append(2), repeated_version],
        vec![append(2), append(1)],
    ] {
        assert_eq!(
            state.append_batch(&batch, &identity, &owner, now(31000)),
            Err(MaterialError::AlreadyExists)
        );
    }
    let mut old_version = append(2);
    old_version.resource.version_id = state.records[0].resource.version_id;
    assert_eq!(
        state.append_batch(&[old_version], &identity, &owner, now(31000)),
        Err(MaterialError::AlreadyExists)
    );
    let deleted = state
        .delete(
            MaterialId::new(1).unwrap(),
            Revision::new(1),
            &identity,
            &owner,
            now(31000),
        )
        .unwrap();
    assert_eq!(
        deleted.append_batch(&[append(1)], &identity, &owner, now(32000)),
        Err(MaterialError::AlreadyExists)
    );
    assert_eq!(state, before);
}

#[test]
fn batch_limits_folder_capacity_and_counter_overflow_remain_atomic() {
    let identity = identity();
    let owner = actor(&identity, 2);
    assert_eq!(
        empty().append_batch(&[], &identity, &owner, now(30000)),
        Err(MaterialError::Capacity)
    );
    let twenty = (1..=MAX_MATERIAL_BATCH_FILES as u64)
        .map(append)
        .collect::<Vec<_>>();
    let full_batch = empty()
        .append_batch(&twenty, &identity, &owner, now(30000))
        .unwrap();
    assert_eq!(full_batch.records.len(), 20);
    assert_eq!(full_batch.folder.revision, Revision::new(1));
    assert_eq!(full_batch.folder.last_added_sequence, 20);
    full_batch.validate().unwrap();
    let too_many = (1..=21).map(append).collect::<Vec<_>>();
    assert_eq!(
        empty().append_batch(&too_many, &identity, &owner, now(30000)),
        Err(MaterialError::Capacity)
    );
    let mut near_full = empty();
    for n in 1..MAX_FOLDER_RECORDS as u64 {
        let input = append(n);
        near_full.records.push(MaterialRecord {
            id: input.material_id,
            folder_id: near_full.folder.id,
            display_name: input.display_name,
            resource: input.resource,
            allowed_class_ids: input.allowed_class_ids,
            added_at: now(30000),
            added_sequence: n,
            status: MaterialStatus::Published,
            revision: Revision::new(1),
        });
    }
    near_full.folder.revision = Revision::new((MAX_FOLDER_RECORDS - 1) as u64);
    near_full.folder.last_added_sequence = (MAX_FOLDER_RECORDS - 1) as u64;
    near_full.validate().unwrap();
    let before = near_full.clone();
    assert_eq!(
        near_full.append_batch(&[append(2048), append(2049)], &identity, &owner, now(31000)),
        Err(MaterialError::Capacity)
    );
    assert_eq!(near_full, before);
    let full = near_full
        .append_batch(&[append(2048)], &identity, &owner, now(31000))
        .unwrap();
    assert_eq!(full.records.len(), MAX_FOLDER_RECORDS);
    full.validate().unwrap();
    for (revision, last_sequence) in [(u64::MAX, 1), (u64::MAX - 1, u64::MAX - 1)] {
        let mut overflow = saved(&identity);
        overflow.folder.revision = Revision::new(revision);
        overflow.folder.last_added_sequence = last_sequence;
        let before = overflow.clone();
        assert_eq!(
            overflow.append_batch(&[append(2), append(3)], &identity, &owner, now(31000)),
            Err(MaterialError::VersionOverflow)
        );
        assert_eq!(overflow, before);
    }
}

#[test]
fn twenty_batch_preserves_atomic_folder_and_desktop_global_revision() {
    use crate::{
        desktop::DesktopFacts,
        lesson_context::LessonContextSnapshot,
        profile::{ProfileSelectionReason, ProfileSelectionSnapshot},
    };
    let identity = identity();
    let schedule = schedule_fixture(&identity);
    let at = now(30000);
    let context = LessonContextSnapshot::default()
        .observe(&identity, &schedule, at)
        .unwrap();
    let source = context
        .material_source(&identity, &schedule, class(1), at)
        .unwrap()
        .unwrap();
    let mut state = empty();
    state.folder.revision = Revision::new(1); // A persisted empty folder is one prior operation.
    let global_before = Revision::new(
        identity
            .revision
            .get()
            .max(schedule.revision.get())
            .max(context.revision.get())
            + 1,
    );
    let mut facts = DesktopFacts {
        global_revision: global_before,
        captured_at: at,
        date_watermark: at.date,
        clock_rollback: false,
        identity: identity.clone(),
        schedule,
        lesson_context: context,
        display: Default::default(),
        profiles: ProfileSelectionSnapshot {
            reason: ProfileSelectionReason::Missing,
            ..Default::default()
        },
        profile_contents: vec![],
        homework_offset: 0,
        homework_eligible_total: 0,
        homework: vec![],
        reminder: None,
        materials_source: Some(source.clone()),
        materials_page: None,
    };
    facts.validate().unwrap();
    let appends = (1..=MAX_MATERIAL_BATCH_FILES as u64)
        .map(append)
        .collect::<Vec<_>>();
    let batch = state
        .append_batch(&appends, &identity, &actor(&identity, 2), at)
        .unwrap();
    facts.global_revision = global_before.next().unwrap(); // One actual atomic Writer operation.
    facts.materials_page = Some(batch.published_page(&identity, &source, None).unwrap());
    eprintln!(
        "global_before={} global_after={} folder_before={} folder_after={} last_sequence={}",
        global_before.get(),
        facts.global_revision.get(),
        state.folder.revision.get(),
        batch.folder.revision.get(),
        batch.folder.last_added_sequence
    );
    assert_eq!(facts.validate(), Ok(()));
    assert_eq!(batch.folder.revision, state.folder.revision.next().unwrap());
    assert_eq!(batch.folder.last_added_sequence, 20);
    assert_eq!(
        facts
            .materials_page
            .as_ref()
            .unwrap()
            .items
            .iter()
            .map(|r| r.added_sequence)
            .collect::<Vec<_>>(),
        vec![20, 19, 18, 17, 16]
    );
    // The global relationship remains strict, independently of sequence count.
    facts.materials_page.as_mut().unwrap().folder.revision = facts.global_revision.next().unwrap();
    assert!(facts.validate().is_err());
}

#[test]
fn batch_sequence_bound_preserves_invalid_state_checks_and_atomic_delete_revision() {
    let identity = identity();
    let owner = actor(&identity, 2);
    let appends = (1..=MAX_MATERIAL_BATCH_FILES as u64)
        .map(append)
        .collect::<Vec<_>>();
    let batch = empty()
        .append_batch(&appends, &identity, &owner, now(30000))
        .unwrap();
    assert_eq!(batch.folder.revision, Revision::new(1));
    batch.validate().unwrap();
    for mutation in 0..6 {
        let mut invalid = batch.clone();
        match mutation {
            0 => invalid.folder.last_added_sequence = MAX_MATERIAL_BATCH_FILES as u64 + 1,
            1 => invalid.folder.revision = Revision::default(),
            2 => invalid.records[1].id = invalid.records[0].id,
            3 => invalid.records[1].added_sequence = invalid.records[0].added_sequence,
            4 => invalid.records[1].revision = Revision::new(2),
            _ => invalid.records[1].folder_id = FolderId::new(999).unwrap(),
        }
        assert_eq!(invalid.validate(), Err(MaterialError::InvalidState));
    }
    let mut invalid_empty = empty();
    invalid_empty.folder.revision = Revision::new(2);
    assert_eq!(invalid_empty.validate(), Err(MaterialError::InvalidState));
    let deleted = batch
        .delete(
            MaterialId::new(1).unwrap(),
            Revision::new(1),
            &identity,
            &owner,
            now(31000),
        )
        .unwrap();
    assert_eq!(deleted.folder.revision, Revision::new(2));
    assert_eq!(deleted.folder.last_added_sequence, 20);
    deleted.validate().unwrap();
    let appended = deleted
        .append(&append(21), &identity, &owner, now(32000))
        .unwrap();
    assert_eq!(appended.folder.revision, Revision::new(3));
    assert_eq!(appended.folder.last_added_sequence, 21);
    appended.validate().unwrap();
    // A two-member batch needs only one remaining revision; sequence overflow
    // is independently checked, and a saturated validation bound must not wrap.
    let mut near_max = saved(&identity);
    near_max.folder.revision = Revision::new(u64::MAX - 1);
    let final_revision = near_max
        .append_batch(&[append(2), append(3)], &identity, &owner, now(32000))
        .unwrap();
    assert_eq!(final_revision.folder.revision, Revision::new(u64::MAX));
    assert_eq!(final_revision.folder.last_added_sequence, 3);
    final_revision.validate().unwrap();
}
