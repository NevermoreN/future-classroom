use super::*;
fn at(second: u32) -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), second).unwrap()
}
fn evidence() -> LessonContextSnapshot {
    let cell = CourseCell {
        subject_id: SubjectId::new(1).unwrap(),
        teacher: TeacherBinding::Explicit(PrincipalId::new(2).unwrap()),
    };
    let key = LessonInstanceKey {
        actual_date: at(0).date,
        class_id: ClassId::new(1).unwrap(),
        range: MinuteRange::new(Minute::new(480).unwrap(), Minute::new(540).unwrap()).unwrap(),
        version: ResolvedScheduleVersion {
            actual_semester_id: SemesterId::new(1).unwrap(),
            actual_revision: Revision::new(1),
            source_semester_id: SemesterId::new(1).unwrap(),
            source_revision: Revision::new(1),
            calendar_mode: CalendarMode::School,
            calendar_mode_revision: Revision::new(1),
            applied_calendar_revision: Revision::new(1),
            makeup_mapping_revision: Revision::new(1),
            source_table_id: TableId::new(1).unwrap(),
            source_row_id: RowId::new(1).unwrap(),
            rules_revision: Revision::new(1),
        },
    };
    let current = EffectiveLessonContext {
        key,
        base: cell.clone(),
        effective: cell,
        teacher_id: Some(PrincipalId::new(2).unwrap()),
        effective_from: at(8 * 3600 + 15 * 60),
        effective_until: at(9 * 3600),
    };
    LessonContextSnapshot {
        revision: Revision::new(3),
        identity_revision: Revision::new(5),
        observed_at: Some(at(8 * 3600 + 30 * 60)),
        class_id: Some(ClassId::new(1).unwrap()),
        history: vec![EffectiveLessonInterval {
            context: EffectiveLessonContext {
                effective_from: at(8 * 3600),
                ..current.clone()
            },
            ended_at: at(8 * 3600 + 15 * 60),
        }],
        current: Some(current),
        ..Default::default()
    }
}
#[test]
fn durable_evidence_rejects_teacher_substitution_future_overlap_and_unproved_override() {
    let valid = evidence();
    valid.validate().unwrap();
    for mutation in 0..13 {
        let mut invalid = valid.clone();
        match mutation {
            0 => invalid.current.as_mut().unwrap().teacher_id = Some(PrincipalId::new(3).unwrap()),
            1 => invalid.current.as_mut().unwrap().effective.teacher = TeacherBinding::Unassigned,
            2 => invalid.history[0].ended_at = at(8 * 3600 + 20 * 60),
            3 => invalid.history[0].ended_at = at(8 * 3600 + 31 * 60),
            4 => invalid.current.as_mut().unwrap().effective_from = at(7 * 3600 + 59 * 60),
            5 => invalid.current.as_mut().unwrap().effective_until = at(9 * 3600 + 1),
            6 => invalid.observed_at = None,
            7 => invalid.revision = Revision::default(),
            8 => {
                invalid.current.as_mut().unwrap().effective.subject_id = SubjectId::new(2).unwrap()
            }
            9 => invalid.observed_at = Some(at(9 * 3600)),
            10 => {
                invalid.history[0].context.key.actual_date = LocalDate::new(2026, 10, 29).unwrap()
            }
            11 => invalid.history.push(invalid.history[0].clone()),
            _ => invalid.blocked_until = Some(at(9 * 3600)),
        }
        assert!(
            invalid.validate().is_err(),
            "malformed durable evidence accepted: {mutation}"
        );
    }
}
