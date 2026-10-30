use super::*;
use fc_domain::{auth::*, content::*, identity::*, schedule::*, schedule_document::*, time::*};
struct Measure;
impl TextMeasure for Measure {
    fn height(&self, text: &str, width: f32, font: f32) -> Result<f32, fc_ports::MeasureError> {
        Ok(text
            .split('\n')
            .map(|line| (line.chars().count() as f32 * font / width).ceil().max(1.) * font * 1.35)
            .sum())
    }
}
fn date(d: u8) -> LocalDate {
    LocalDate::new(2026, 10, d).unwrap()
}
fn now(second: u32) -> LocalDateTime {
    LocalDateTime::new(date(30), second).unwrap()
}
fn id(n: u64) -> PrincipalId {
    PrincipalId::new(n).unwrap()
}
fn actor() -> ActorContext {
    ActorContext {
        principal_id: id(1),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: now(86399),
        origin: TrustedOrigin::Local,
    }
}
fn fixture() -> DesktopFacts {
    let class = ClassId::new(1).unwrap();
    let subject = SubjectId::new(1).unwrap();
    let mut identity = IdentityState::default()
        .transition(
            &IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "学校".into(),
                id: id(1),
                login_name: "admin".into(),
                display_name: "管理教师".into(),
            },
            None,
            now(30000),
        )
        .unwrap()
        .next;
    for command in [
        IdentityCommand::CreateClass {
            id: class,
            name: "一班".into(),
        },
        IdentityCommand::PutSubject {
            id: subject,
            name: "数学".into(),
            enabled: true,
            color: Rgb {
                red: 255,
                green: 0,
                blue: 0,
            },
            expected_revision: None,
        },
        IdentityCommand::BindCurrentClass { id: class },
        IdentityCommand::CreateAccount(NewAccount {
            id: id(2),
            login_name: "teacher2".into(),
            display_name: "二老师".into(),
            role: Role::TeacherAdmin,
            scopes: vec![AssignmentScope {
                class_id: class,
                subject_id: subject,
            }],
        }),
        IdentityCommand::UpdateAccount {
            id: id(1),
            display_name: "管理教师".into(),
            role: Role::GlobalAdmin,
            enabled: true,
            scopes: vec![AssignmentScope {
                class_id: class,
                subject_id: subject,
            }],
        },
    ] {
        let mut a = actor();
        a.auth_epoch = identity.account(id(1)).unwrap().principal.auth_epoch;
        identity = identity
            .transition(&command, Some(&a), now(30000))
            .unwrap()
            .next;
    }
    let rows = [(8, 0, 8, 40), (8, 50, 9, 30)]
        .into_iter()
        .enumerate()
        .map(|(index, (bh, bm, eh, em))| TimetableRow {
            id: RowId::new(index as u64 + 10).unwrap(),
            logical_key: LogicalRowKey::new(index as u64 + 20).unwrap(),
            group: DayGroup::Morning,
            kind: RowKind::Course,
            range: MinuteRange::new(
                Minute::from_hm(bh, bm).unwrap(),
                Minute::from_hm(eh, em).unwrap(),
            )
            .unwrap(),
            enabled_days: [true; 7],
            courses: std::array::from_fn(|_| {
                Some(CourseCell {
                    subject_id: subject,
                    teacher: TeacherBinding::Explicit(id(2)),
                })
            }),
        })
        .collect();
    let semester = Semester {
        id: SemesterId::new(1).unwrap(),
        class_id: class,
        revision: Revision::default(),
        enabled: true,
        start: date(26),
        end: date(31),
        first_week_monday: date(26),
        tables: vec![WeekTable {
            id: TableId::new(2).unwrap(),
            role: TableRole::Single,
            rows,
        }],
    };
    let mut schedule = ScheduleDocument::default();
    schedule
        .semesters
        .push(SemesterPlan::new(semester, "秋季".into()));
    schedule.public_calendar.public_verified_years.push(2026);
    let mut a = actor();
    a.auth_epoch = identity.account(id(1)).unwrap().principal.auth_epoch;
    schedule = ScheduleDocument::default()
        .transition(&schedule, Revision::default(), &identity, &a, now(30000))
        .unwrap();
    let homework = [id(1), id(2)]
        .into_iter()
        .map(|owner| ContentRecord {
            key: ContentKey::Homework(HomeworkKey {
                class_id: class,
                actual_date: date(30),
                subject_id: subject,
                owner_teacher_id: owner,
            }),
            text: format!("{}老师布置的作业", owner.get()),
            revision: Revision::new(1),
            last_actor_id: owner,
            updated_at: now(30000),
        })
        .collect();
    let materials_source =
        fc_domain::materials::resolve_material_source(&identity, &schedule, class, now(30300))
            .unwrap();
    DesktopFacts {
        global_revision: Revision::new(100),
        captured_at: now(30300),
        date_watermark: date(30),
        clock_rollback: false,
        identity,
        schedule,
        display: DisplaySettings::default(),
        homework_offset: 0,
        homework_eligible_total: 2,
        homework,
        reminder: None,
        materials_source,
        materials_page: None,
    }
}
fn scene(facts: &DesktopFacts) -> SceneSnapshot {
    let mut fresh = facts.clone();
    fresh.materials_source = if fresh.clock_rollback {
        None
    } else {
        fresh
            .identity
            .installation
            .as_ref()
            .and_then(|i| i.current_class)
            .and_then(|class| {
                fc_domain::materials::resolve_material_source(
                    &fresh.identity,
                    &fresh.schedule,
                    class,
                    fresh.captured_at,
                )
                .ok()
                .flatten()
            })
    };
    build_scene(
        &fresh,
        SceneGeneration::new(1),
        Rect {
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1040.,
        },
        &Measure,
    )
    .unwrap()
}
#[test]
fn repeated_courses_and_same_subject_multi_teacher_homework_stay_distinct() {
    let facts = fixture();
    let view = scene(&facts);
    let schedule = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Schedule)
        .unwrap();
    let CardContent::Courses(courses) = &schedule.content else {
        panic!("courses expected")
    };
    assert_eq!(courses.len(), 2);
    assert_eq!(courses[0].name, "数学");
    assert_ne!(courses[0].key, courses[1].key);
    assert!(courses[0].current);
    assert!(!courses[1].current);
    assert_eq!(courses[0].fill_alpha, 0.2);
    assert_eq!(courses[0].stroke_width, 0.);
    let hw = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Homework)
        .unwrap();
    let CardContent::Homework(entries) = &hw.content else {
        panic!("homework expected")
    };
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0].teacher_id, entries[1].teacher_id);
    assert_eq!(entries[0].fill_alpha, 0.03);
    assert_eq!(entries[0].color, courses[0].color);
    assert_eq!(
        hw.page,
        PageInfo {
            offset: 0,
            shown: 2,
            total: 2
        }
    );
    assert!(!view.cards.iter().any(|c| c.kind == ComponentKind::Reminder));
}
#[test]
fn zero_fill_stays_filled_and_zero_component_retains_only_layout() {
    let mut facts = fixture();
    facts.display.course_fill_percent = 0;
    facts
        .display
        .components
        .iter_mut()
        .find(|c| c.kind == ComponentKind::Schedule)
        .unwrap()
        .opacity_percent = 0;
    let view = scene(&facts);
    let card = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Schedule)
        .unwrap();
    assert_eq!(card.component_alpha, 0.);
    let CardContent::Courses(rows) = &card.content else {
        panic!()
    };
    assert_eq!(rows[0].fill_alpha, 0.);
    assert_eq!(rows[0].stroke_width, 0.);
    facts.display.subject_fill = false;
    let view = scene(&facts);
    let CardContent::Courses(rows) = &view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Schedule)
        .unwrap()
        .content
    else {
        panic!()
    };
    assert_eq!(rows[0].stroke_width, 1.);
}
#[test]
fn recess_and_static_scene_expire_at_actual_second_and_date() {
    let mut facts = fixture();
    facts.captured_at = now(8 * 3600 + 45 * 60);
    let view = scene(&facts);
    let card = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Recess)
        .unwrap();
    let CardContent::Recess { deadline } = card.content else {
        panic!()
    };
    assert_eq!(
        recess_label(deadline, facts.captured_at),
        Some("05:00".into())
    );
    assert_eq!(recess_label(deadline, now(8 * 3600 + 50 * 60)), None);
    assert!(!view.valid_at(now(8 * 3600 + 50 * 60)));
    assert!(!view.valid_at(LocalDateTime::new(date(31), 0).unwrap()));
    assert!(!view.valid_at(now(8 * 3600 + 44 * 60)));
    facts.captured_at = now(8 * 3600 + 50 * 60);
    assert!(!scene(&facts)
        .cards
        .iter()
        .any(|c| c.kind == ComponentKind::Recess));
}
#[test]
fn public_unverified_keeps_base_courses_but_clock_rollback_is_neutral() {
    let mut facts = fixture();
    facts.schedule.public_calendar.public_verified_years.clear();
    let view = scene(&facts);
    assert!(matches!(
        view.cards
            .iter()
            .find(|c| c.kind == ComponentKind::Schedule)
            .unwrap()
            .content,
        CardContent::Courses(_)
    ));
    assert!(view.public_calendar_unverified);
    facts.captured_at = now(8 * 3600 + 45 * 60);
    assert!(scene(&facts)
        .cards
        .iter()
        .any(|c| c.kind == ComponentKind::Recess));
    facts.clock_rollback = true;
    facts.date_watermark = date(31);
    facts.homework.clear();
    facts.homework_eligible_total = 0;
    let view = scene(&facts);
    assert!(view.clock_rollback);
    assert!(!view
        .cards
        .iter()
        .any(|c| matches!(c.content, CardContent::Courses(_))));
}
#[test]
fn countdown_expired_policy_and_independent_clock_modes() {
    let mut facts = fixture();
    facts.display.countdown.target = Some(date(29));
    assert!(!scene(&facts)
        .cards
        .iter()
        .any(|c| c.kind == ComponentKind::DateCountdown));
    facts.display.countdown.expired = CountdownExpired::ShowCompleted;
    assert!(scene(&facts)
        .cards
        .iter()
        .any(|c| matches!(&c.content,CardContent::Text(t)if t.ends_with("已结束"))));
    assert_eq!(
        clock_label(
            ClockContent {
                mode: ClockMode::Digital12,
                show_seconds: false
            },
            now(0)
        ),
        "上午 12:00"
    );
    let hands = clock_hands(
        ClockContent {
            mode: ClockMode::Analog,
            show_seconds: true,
        },
        now(3 * 3600 + 15 * 60 + 30),
    );
    assert_eq!(hands.minute, 0.25);
    assert_eq!(hands.second, Some(0.5));
}

#[test]
fn default_scene_adapts_to_eight_hundred_by_six_hundred() {
    let facts = fixture();
    let scene = build_scene(
        &facts,
        SceneGeneration::new(1),
        Rect {
            x: 0.,
            y: 0.,
            width: 800.,
            height: 600.,
        },
        &Measure,
    )
    .unwrap();
    for card in scene.cards {
        assert!(card.rect.x >= 0. && card.rect.y >= 0.);
        assert!(card.rect.x + card.rect.width <= 800.);
        assert!(card.rect.y + card.rect.height <= 600.);
    }
}
#[test]
fn empty_reminder_removes_the_whole_card_and_following_gap() {
    let mut facts = fixture();
    let first = scene(&facts);
    facts.reminder = Some(ContentRecord {
        key: ContentKey::SharedReminder(ReminderKey {
            class_id: ClassId::new(1).unwrap(),
            actual_date: date(30),
        }),
        text: "请带水杯".into(),
        revision: Revision::new(1),
        last_actor_id: id(1),
        updated_at: now(30000),
    });
    let second = scene(&facts);
    assert_eq!(second.cards.len(), first.cards.len() + 1);
    facts.reminder.as_mut().unwrap().text = " \n ".into();
    assert!(matches!(
        build_scene(
            &facts,
            SceneGeneration::new(1),
            Rect {
                x: 0.,
                y: 0.,
                width: 1920.,
                height: 1040.
            },
            &Measure
        ),
        Err(SceneError::InvalidFacts(_))
    ));
    facts.reminder = None;
    assert_eq!(scene(&facts).cards, first.cards);
}

#[test]
fn analog_clock_reserves_real_dial_and_cards_reserve_heading_and_body() {
    let mut facts = fixture();
    facts.display.clock_mode = ClockMode::Analog;
    let view = build_scene(
        &facts,
        SceneGeneration::new(1),
        Rect {
            x: 0.,
            y: 0.,
            width: 800.,
            height: 600.,
        },
        &Measure,
    )
    .unwrap();
    assert!(
        view.cards
            .iter()
            .find(|c| c.kind == ComponentKind::Clock)
            .unwrap()
            .rect
            .height
            >= 160.
    );
    assert!(view.cards.iter().all(|c| c.rect.height >= 84.));
}

#[test]
fn custom_minimum_constraints_grow_cards_when_space_exists() {
    let mut facts = fixture();
    let material = facts
        .display
        .components
        .iter_mut()
        .find(|c| c.kind == ComponentKind::Materials)
        .unwrap();
    material.min_height = 200;
    material.min_width = 400;
    let view = scene(&facts);
    let material = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Materials)
        .unwrap();
    assert!(material.rect.height >= 200.);
    assert!(material.rect.width >= 400.);
}

fn material_facts(count: u64) -> DesktopFacts {
    use fc_domain::materials::*;
    let mut facts = fixture();
    let source = facts.materials_source.clone().unwrap();
    let mut state = MaterialFolderState::empty(
        FolderId::new(7).unwrap(),
        MaterialScope {
            teacher_id: source.teacher_id,
            subject_id: source.subject_id,
        },
    );
    let mut admin = actor();
    admin.auth_epoch = facts
        .identity
        .account(admin.principal_id)
        .unwrap()
        .principal
        .auth_epoch;
    for n in 1..=count {
        state = state
            .append(
                &MaterialAppend {
                    material_id: MaterialId::new(n).unwrap(),
                    display_name: format!("第{n}份课堂资料.txt"),
                    resource: MaterialResource {
                        version_id: ResourceVersionId::new(n).unwrap(),
                        sha256: [n as u8; 32],
                        kind: MaterialKind::Txt,
                        bytes: 100,
                    },
                    allowed_class_ids: vec![source.lesson.class_id],
                },
                &facts.identity,
                &admin,
                now(30000),
            )
            .unwrap();
    }
    facts.materials_page = Some(
        state
            .published_page(&facts.identity, &source, None)
            .unwrap(),
    );
    facts
}
#[test]
fn material_rows_are_current_teacher_fixed_versions_first_five_and_not_paths() {
    let facts = material_facts(7);
    let view = scene(&facts);
    let card = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Materials)
        .unwrap();
    let CardContent::Materials(list) = &card.content else {
        panic!("material list expected")
    };
    assert_eq!(list.rows.len(), 5);
    assert_eq!(list.rows[0].material_id.get(), 7);
    assert_eq!(list.rows[0].resource_version.get(), 7);
    assert_eq!(list.teacher_name, "二老师");
    assert_eq!(list.source.teacher_id, PrincipalId::new(2).unwrap());
    assert_eq!(list.total, 7);
    assert_eq!(list.next_cursor.as_ref().unwrap().offset, 5);
    assert_eq!(
        card.page,
        PageInfo {
            offset: 0,
            shown: 5,
            total: 7
        }
    );
    assert_eq!(list.rows[0].name, "第7份课堂资料.txt");
}
#[test]
fn empty_material_folder_retains_correct_teacher_and_never_borrows_another() {
    let facts = fixture();
    let view = scene(&facts);
    let CardContent::Materials(list) = &view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Materials)
        .unwrap()
        .content
    else {
        panic!()
    };
    assert_eq!(list.teacher_name, "二老师");
    assert!(list.rows.is_empty());
    assert_eq!(list.folder_id, None);
    assert!(list.next_cursor.is_none());
    let mut no_course = facts;
    no_course.captured_at = now(12 * 3600);
    let view = scene(&no_course);
    assert!(matches!(
        view.cards
            .iter()
            .find(|c| c.kind == ComponentKind::Materials)
            .unwrap()
            .content,
        CardContent::MaterialsUnavailable
    ));
}
#[test]
fn material_source_page_cross_teacher_class_date_or_joint_revision_is_rejected() {
    for mutation in 0..4 {
        let mut facts = material_facts(1);
        match mutation {
            0 => facts.materials_source.as_mut().unwrap().teacher_id = PrincipalId::new(1).unwrap(),
            1 => facts.materials_page.as_mut().unwrap().class_id = Some(ClassId::new(99).unwrap()),
            2 => facts.materials_source.as_mut().unwrap().lesson.actual_date = date(29),
            _ => {
                facts
                    .materials_source
                    .as_mut()
                    .unwrap()
                    .lesson
                    .version
                    .source_revision = Revision::new(99)
            }
        };
        assert!(matches!(
            build_scene(
                &facts,
                SceneGeneration::new(1),
                Rect {
                    x: 0.,
                    y: 0.,
                    width: 1920.,
                    height: 1040.
                },
                &Measure
            ),
            Err(SceneError::InvalidFacts(_))
        ));
    }
}
#[test]
fn withheld_recess_history_is_neutral_while_missing_current_source_is_invalid() {
    let mut facts = fixture();
    facts.materials_source = None;
    assert_eq!(facts.validate(), Err(DisplayError::InvalidFacts));
    facts.captured_at = now(8 * 3600 + 45 * 60);
    facts.validate().unwrap();
    let view = build_scene(
        &facts,
        SceneGeneration::new(1),
        Rect {
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1040.,
        },
        &Measure,
    )
    .unwrap();
    assert!(matches!(
        view.cards
            .iter()
            .find(|c| c.kind == ComponentKind::Materials)
            .unwrap()
            .content,
        CardContent::MaterialsUnavailable
    ));
    assert!(view.cards.iter().any(|c| c.kind == ComponentKind::Recess));
}
