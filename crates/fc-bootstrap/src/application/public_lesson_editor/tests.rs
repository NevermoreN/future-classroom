use super::*;
use fc_domain::{identity::Rgb, lesson_context::*, schedule::*, time::*};
fn fixture() -> Editor {
    let class = ClassId::new(1).unwrap();
    let school = SchoolId::new(1).unwrap();
    let subject = SubjectId::new(1).unwrap();
    let teacher = PrincipalId::new(2).unwrap();
    let date = LocalDate::new(2026, 10, 30).unwrap();
    let range = MinuteRange::new(Minute::new(480).unwrap(), Minute::new(540).unwrap()).unwrap();
    let key = LessonInstanceKey {
        actual_date: date,
        class_id: class,
        range,
        version: ResolvedScheduleVersion {
            actual_semester_id: SemesterId::new(1).unwrap(),
            actual_revision: Revision::new(2),
            source_semester_id: SemesterId::new(1).unwrap(),
            source_revision: Revision::new(2),
            calendar_mode: CalendarMode::School,
            calendar_mode_revision: Revision::new(1),
            applied_calendar_revision: Revision::new(1),
            makeup_mapping_revision: Revision::new(1),
            source_table_id: TableId::new(1).unwrap(),
            source_row_id: RowId::new(1).unwrap(),
            rules_revision: Revision::new(1),
        },
    };
    let cell = CourseCell {
        subject_id: subject,
        teacher: TeacherBinding::Explicit(teacher),
    };
    let snapshot = LessonContextSnapshot {
        revision: Revision::new(7),
        identity_revision: Revision::new(5),
        observed_at: Some(LocalDateTime::new(date, 480 * 60).unwrap()),
        class_id: Some(class),
        current: Some(EffectiveLessonContext {
            key,
            base: cell.clone(),
            effective: cell,
            teacher_id: Some(teacher),
            effective_from: LocalDateTime::new(date, 480 * 60).unwrap(),
            effective_until: LocalDateTime::new(date, 540 * 60).unwrap(),
        }),
        ..LessonContextSnapshot::default()
    };

    Editor::new(DesktopLessonSource {
        context: snapshot,
        catalog_revision: Revision::new(5),
        schedule_revision: Revision::new(2),
        subjects: (1..=3)
            .map(|id| SchoolSubject {
                id: SubjectId::new(id).unwrap(),
                school_id: school,
                name: format!("科目{id}"),
                enabled: true,
                color: Rgb {
                    red: 0,
                    green: 100,
                    blue: 200,
                },
                revision: Revision::new(1),
            })
            .collect(),
        teachers: vec![
            fc_ports::DesktopLessonTeacher {
                id: teacher,
                display_name: "任课甲".into(),
                subject_ids: vec![subject],
            },
            fc_ports::DesktopLessonTeacher {
                id: PrincipalId::new(3).unwrap(),
                display_name: "任课乙".into(),
                subject_ids: vec![subject],
            },
        ],
    })
}
fn form(editor: &Editor) -> FormData {
    let mut data = FormData::new();
    for field in editor.view("", false).fields {
        data.insert(field.id, field.value).unwrap();
    }
    data
}
#[test]
fn public_choices_use_only_display_names_and_never_offer_account_navigation() {
    let editor = fixture();
    let v = editor.view("", false);
    v.validate().unwrap();
    assert_eq!(v.fields.len(), 2);
    assert!(v
        .actions
        .iter()
        .all(|a| matches!(a.id, REPLACE | RESTORE | RELOAD | CLOSE)));
    let FieldKind::Choice(teachers) = &v.fields[1].kind else {
        panic!()
    };
    assert!(teachers
        .iter()
        .any(|t| t.value == "2" && t.label == "任课甲"));
    assert!(teachers
        .iter()
        .any(|t| t.value == "3" && t.label == "任课乙"));
}
#[test]
fn arbitrary_identity_or_revision_fields_and_management_actions_are_rejected() {
    for action in [LOCK, BACK, TO_ACCOUNTS, TO_SCHEDULE] {
        let mut editor = fixture();
        let data = form(&editor);
        assert!(editor.handle(action, data).is_err());
    }
    for action in [REPLACE, RESTORE, RELOAD, CLOSE] {
        let mut editor = fixture();
        let mut data = form(&editor);
        data.insert(FieldId(999), "7".into()).unwrap();
        assert!(editor.handle(action, data).is_err());
    }
}
#[test]
fn selected_teacher_must_belong_to_captured_subject_and_cancel_never_writes() {
    let mut editor = fixture();
    let mut data = form(&editor);
    data.take(TEACHER);
    data.insert(TEACHER, "999".into()).unwrap();
    assert!(editor.handle(REPLACE, data).is_err());
    let data = form(&editor);
    assert!(matches!(editor.handle(CLOSE, data), Ok(Command::Close)));
}
#[test]
fn unassigned_is_explicit_single_business_confirmation_without_account_authority() {
    let mut editor = fixture();
    assert!(editor.handle(CONFIRM_UNASSIGNED, FormData::new()).is_err());
    let mut data = form(&editor);
    data.take(SUBJECT);
    data.insert(SUBJECT, "2".into()).unwrap();
    data.take(TEACHER);
    data.insert(TEACHER, "unassigned".into()).unwrap();
    assert!(matches!(editor.handle(REPLACE, data), Ok(Command::Stay)));
    assert!(editor.view("", false).fields.is_empty());
    assert!(matches!(
        editor.handle(CANCEL_UNASSIGNED, FormData::new()),
        Ok(Command::Stay)
    ));
    assert!(!editor.review);
    let data = form(&editor);
    assert!(matches!(editor.handle(REPLACE, data), Ok(Command::Stay)));
    assert!(
        matches!(editor.handle(CONFIRM_UNASSIGNED,FormData::new()),Ok(Command::Apply(LessonAction::Replace(CourseCell{subject_id,teacher:TeacherBinding::Unassigned}))) if subject_id.get()==2)
    );
    assert_eq!(editor.source.context.revision, Revision::new(7));
    assert_eq!(editor.source.catalog_revision, Revision::new(5));
    assert_eq!(editor.source.schedule_revision, Revision::new(2));
}
#[test]
fn replacement_contains_only_business_target_and_does_not_rebind_original_lesson() {
    let mut editor = fixture();
    let key = editor.source.context.current.as_ref().unwrap().key.clone();
    let data = form(&editor);
    assert!(
        matches!(editor.handle(REPLACE,data),Ok(Command::Apply(LessonAction::Replace(CourseCell{teacher:TeacherBinding::Explicit(id),..}))) if id.get()==2)
    );
    assert_eq!(editor.source.context.current.as_ref().unwrap().key, key);
}
