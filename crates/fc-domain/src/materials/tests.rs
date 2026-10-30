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
