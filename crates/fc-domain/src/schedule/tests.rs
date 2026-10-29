use super::*;

fn date(day: u8) -> LocalDate {
    LocalDate::new(2026, 10, day).unwrap()
}
fn now(day: u8, h: u32, m: u32, s: u32) -> LocalDateTime {
    LocalDateTime::new(date(day), h * 3600 + m * 60 + s).unwrap()
}
fn range(start: u16, end: u16) -> MinuteRange {
    MinuteRange::new(Minute::new(start).unwrap(), Minute::new(end).unwrap()).unwrap()
}
fn cell(subject: u64) -> CourseCell {
    CourseCell {
        subject_id: SubjectId::new(subject).unwrap(),
        teacher: TeacherBinding::Explicit(PrincipalId::new(1).unwrap()),
    }
}
fn row(id: u64, start: u16, end: u16, kind: RowKind) -> TimetableRow {
    TimetableRow {
        id: RowId::new(id).unwrap(),
        logical_key: LogicalRowKey::new(id).unwrap(),
        group: DayGroup::Morning,
        kind,
        range: range(start, end),
        enabled_days: [true; 7],
        courses: std::array::from_fn(|_| {
            if kind == RowKind::Course {
                Some(cell(id))
            } else {
                None
            }
        }),
    }
}
fn term() -> Semester {
    Semester {
        id: SemesterId::new(1).unwrap(),
        class_id: ClassId::new(1).unwrap(),
        revision: Revision::new(1),
        enabled: true,
        start: date(26),
        end: date(30),
        first_week_monday: date(26),
        tables: vec![WeekTable {
            id: TableId::new(1).unwrap(),
            role: TableRole::Single,
            rows: vec![
                row(1, 600, 610, RowKind::Course),
                row(2, 610, 625, RowKind::Rest(RestPurpose::Recess)),
                row(3, 625, 670, RowKind::Course),
            ],
        }],
    }
}
fn calendar() -> CalendarConfig {
    CalendarConfig {
        mode: CalendarMode::Public,
        mode_revision: Revision::new(1),
        applied_revision: Revision::new(2),
        mapping_revision: Revision::new(3),
        school_coverage: None,
        public_verified_years: vec![],
        entries: vec![],
    }
}
fn day(term: Semester) -> AcademicDay {
    resolve_day(&[term], ClassId::new(1).unwrap(), date(26), &calendar()).unwrap()
}

#[test]
fn half_open_course_recess_boundaries_and_startup() {
    let day = day(term());
    assert!(matches!(
        resolve_lesson(&day, now(26, 10, 9, 59), true),
        LessonState::Course { .. }
    ));
    assert!(matches!(
        resolve_lesson(&day, now(26, 10, 10, 0), true),
        LessonState::Recess {
            remaining_seconds: 900,
            ..
        }
    ));
    assert!(matches!(
        resolve_lesson(&day, now(26, 10, 20, 0), true),
        LessonState::Recess {
            remaining_seconds: 300,
            ..
        }
    ));
    assert!(matches!(
        resolve_lesson(&day, now(26, 10, 24, 59), true),
        LessonState::Recess {
            remaining_seconds: 1,
            ..
        }
    ));
    assert!(matches!(
        resolve_lesson(&day, now(26, 10, 25, 0), true),
        LessonState::Course { .. }
    ));
    assert_eq!(
        resolve_lesson(&day, now(27, 10, 20, 0), true),
        LessonState::OutOfTerm
    );
}
#[test]
fn recess_does_not_guess_lunch_unknown_or_after_school() {
    for purpose in [RestPurpose::Lunch, RestPurpose::Unknown, RestPurpose::Other] {
        let mut t = term();
        t.tables[0].rows[1].kind = RowKind::Rest(purpose);
        assert!(!matches!(
            resolve_lesson(&day(t), now(26, 10, 20, 0), true),
            LessonState::Recess { .. }
        ));
    }
    let mut t = term();
    t.tables[0].rows.pop();
    assert_eq!(
        resolve_lesson(&day(t), now(26, 10, 20, 0), true),
        LessonState::Rest
    );
    let mut t = term();
    t.tables[0].rows[2].group = DayGroup::Afternoon;
    assert_eq!(
        resolve_lesson(&day(t), now(26, 10, 20, 0), true),
        LessonState::Rest
    );
}
#[test]
fn rest_before_activity_counts_only_its_own_interval() {
    let mut t = term();
    t.tables[0].rows[2].range = range(630, 670);
    t.tables[0]
        .rows
        .insert(2, row(4, 625, 630, RowKind::Activity));
    let d = day(t);
    assert!(matches!(
        resolve_lesson(&d, now(26, 10, 24, 59), true),
        LessonState::Recess {
            remaining_seconds: 1,
            until_next_course: false,
            ..
        }
    ));
    assert_eq!(
        resolve_lesson(&d, now(26, 10, 25, 0), true),
        LessonState::Activity
    );
}
#[test]
fn automatic_gap_requires_adjacent_courses_in_same_group() {
    let mut t = term();
    t.tables[0].rows.remove(1);
    assert!(matches!(
        resolve_lesson(&day(t.clone()), now(26, 10, 20, 0), true),
        LessonState::Recess { .. }
    ));
    assert_eq!(
        resolve_lesson(&day(t.clone()), now(26, 10, 20, 0), false),
        LessonState::Idle
    );
    t.tables[0]
        .rows
        .insert(1, row(4, 615, 620, RowKind::Activity));
    assert_eq!(
        resolve_lesson(&day(t), now(26, 10, 20, 0), true),
        LessonState::Idle
    );
}
#[test]
fn teaching_week_uses_confirmed_monday_and_deep_copies() {
    let mut t = term();
    t.first_week_monday = date(19);
    t.tables = split_week_tables(
        &t.tables[0],
        TableId::new(2).unwrap(),
        TableId::new(3).unwrap(),
        &[11, 12, 13].map(|id| RowId::new(id).unwrap()),
        &[21, 22, 23].map(|id| RowId::new(id).unwrap()),
    )
    .unwrap();
    assert_eq!(t.teaching_week(date(26)), Ok(2));
    assert_eq!(t.table_for(date(26)).unwrap().role, TableRole::Even);
    t.tables[0].rows[0].courses[0] = Some(cell(99));
    assert_ne!(
        t.tables[0].rows[0].courses[0],
        t.tables[1].rows[0].courses[0]
    );
    t.tables[0].rows[0].range = range(590, 610);
    let synced = synchronize_timing(&t, t.tables[0].id).unwrap();
    assert_eq!(synced.tables[1].rows[0].range, range(590, 610));
    assert_eq!(synced.tables[1].rows[0].courses[0], Some(cell(1)));
    t.tables[1].rows.pop();
    assert!(synchronize_timing(&t, t.tables[0].id).is_err());
}
#[test]
fn validates_overlaps_per_enabled_weekday_and_roles() {
    let mut t = term();
    t.tables[0].rows[1].range = range(609, 625);
    assert!(matches!(t.validate(), Err(ScheduleError::Overlap { .. })));
    t.tables[0].rows[1].enabled_days = [false; 7];
    assert!(t.validate().is_ok());
    t.tables[0].role = TableRole::Odd;
    assert_eq!(t.validate(), Err(ScheduleError::InvalidRoles));
    t = term();
    t.first_week_monday = date(27);
    assert!(t.validate().is_err());
    let mut duplicate = t.clone();
    duplicate.id = SemesterId::new(2).unwrap();
    t = term();
    duplicate.first_week_monday = date(26);
    assert_eq!(
        validate_semesters(&[t, duplicate]),
        Err(ScheduleError::AmbiguousTerm)
    );
}
#[test]
fn school_missing_coverage_does_not_fall_back_to_public() {
    let t = term();
    let mut c = calendar();
    c.mode = CalendarMode::School;
    assert_eq!(
        resolve_day(std::slice::from_ref(&t), t.class_id, date(26), &c),
        Ok(AcademicDay::Unconfirmed)
    );
    c.school_coverage = Some((date(26), date(30)));
    assert!(matches!(
        resolve_day(std::slice::from_ref(&t), t.class_id, date(26), &c),
        Ok(AcademicDay::Scheduled(_))
    ));
    assert_eq!(
        resolve_day(std::slice::from_ref(&t), t.class_id, date(31), &c),
        Ok(AcademicDay::OutOfTerm)
    );
}
#[test]
fn cross_semester_makeup_is_base_only_and_tracks_both_revisions() {
    let actual = term();
    let mut source = term();
    source.id = SemesterId::new(2).unwrap();
    source.start = date(12);
    source.end = date(16);
    source.first_week_monday = date(12);
    source.revision = Revision::new(8);
    let mut c = calendar();
    c.entries = vec![
        CalendarEntry {
            actual_semester_id: actual.id,
            date: date(26),
            mapping: DayMapping::SourceDate(date(12)),
        },
        CalendarEntry {
            actual_semester_id: source.id,
            date: date(12),
            mapping: DayMapping::Rest,
        },
    ];
    let make = |a: Semester, s: Semester| {
        resolve_day(&[a, s], ClassId::new(1).unwrap(), date(26), &c).unwrap()
    };
    let d = make(actual.clone(), source.clone());
    let key = match resolve_lesson(&d, now(26, 10, 0, 0), true) {
        LessonState::Course { key, .. } => key,
        _ => panic!("expected mapped base course"),
    };
    assert_eq!(key.actual_date, date(26));
    assert_eq!(key.version.source_revision, Revision::new(8));
    source.revision = Revision::new(9);
    let changed = match resolve_lesson(&make(actual.clone(), source), now(26, 10, 0, 0), true) {
        LessonState::Course { key, .. } => key,
        _ => panic!(),
    };
    assert_ne!(key, changed);
    c.entries[0].mapping = DayMapping::SourceDate(date(5));
    assert_eq!(
        resolve_day(std::slice::from_ref(&actual), actual.class_id, date(26), &c),
        Ok(AcademicDay::Unconfirmed)
    );
}

#[test]
fn override_is_local_global_and_never_revives_after_expiry_or_clock_rollback() {
    let key = match resolve_lesson(&day(term()), now(26, 10, 0, 0), true) {
        LessonState::Course { key, .. } => key,
        _ => panic!(),
    };
    let actor = ActorContext {
        principal_id: PrincipalId::new(1).unwrap(),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: now(26, 12, 0, 0),
        origin: crate::auth::TrustedOrigin::Local,
    };
    let authority = AuthorityContext {
        principal: crate::auth::Principal {
            id: actor.principal_id,
            school_id: actor.school_id,
            role: crate::auth::Role::GlobalAdmin,
            enabled: true,
            auth_epoch: actor.auth_epoch,
        },
        installation_school: actor.school_id,
        managed_classes: vec![key.class_id],
        assignments: vec![crate::auth::TeachingAssignment {
            teacher_id: actor.principal_id,
            class_id: key.class_id,
            subject_id: SubjectId::new(5).unwrap(),
            enabled: true,
        }],
    };
    let mut o = LessonOverride::create(
        OverrideId::new(1).unwrap(),
        key.clone(),
        cell(5),
        &actor,
        &authority,
        now(26, 10, 1, 0),
    )
    .unwrap();
    assert!(o.observe(Some(&key), now(26, 10, 2, 0), true).is_some());
    assert!(o.observe(Some(&key), now(26, 10, 1, 30), true).is_none());
    assert!(o.observe(Some(&key), now(26, 10, 3, 0), true).is_none());
    let mut o = LessonOverride::create(
        OverrideId::new(2).unwrap(),
        key.clone(),
        cell(5),
        &actor,
        &authority,
        now(26, 10, 1, 0),
    )
    .unwrap();
    assert!(o.observe(Some(&key), now(26, 10, 10, 0), true).is_none());
    assert!(o.observe(Some(&key), now(26, 10, 9, 0), true).is_none());
    let mut remote = actor.clone();
    remote.origin = crate::auth::TrustedOrigin::Web;
    assert!(LessonOverride::create(
        OverrideId::new(3).unwrap(),
        key,
        cell(5),
        &remote,
        &authority,
        now(26, 10, 1, 0)
    )
    .is_err());
}

#[test]
fn persisted_final_teacher_evidence_survives_source_edits_but_not_cross_day_or_revocation() {
    let key = match resolve_lesson(&day(term()), now(26, 10, 0, 0), true) {
        LessonState::Course { key, .. } => key,
        _ => panic!(),
    };
    let history = LastLessonMaterialContext {
        source_lesson_key: key.clone(),
        actual_teacher: PrincipalId::new(9).unwrap(),
        actual_subject: SubjectId::new(8).unwrap(),
        folder_id: FolderId::new(7).unwrap(),
        effective_from: now(26, 10, 1, 0),
        effective_until: now(26, 10, 10, 0),
    };
    assert!(history.usable_for_recess(key.class_id, now(26, 10, 20, 0), true));
    assert!(!history.usable_for_recess(key.class_id, now(27, 10, 20, 0), true));
    assert!(!history.usable_for_recess(key.class_id, now(26, 10, 20, 0), false));
    let mut edited = term();
    edited.revision = Revision::new(9);
    edited.tables[0].rows[0].courses[0] = Some(cell(99));
    // Historical evidence remains immutable; the caller chooses it only for a valid recess.
    let _new_day = day(edited);
    assert_eq!(history.actual_teacher, PrincipalId::new(9).unwrap());
    let mut partial = history.clone();
    partial.effective_until = now(26, 10, 9, 0);
    assert!(!partial.usable_for_recess(key.class_id, now(26, 10, 20, 0), true));
}
