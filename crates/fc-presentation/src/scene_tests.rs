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
    schedule.public_calendar_data = fc_domain::calendar::PublicCalendarData {
        years: vec![fc_domain::calendar::CalendarCandidate {
            source_year: 2026,
            fetched_at: now(1),
            source_sha256: [7; 32],
            papers: vec!["https://www.gov.cn/zhengce/content/202501/content_1000000.htm".into()],
            days: vec![fc_domain::calendar::PublicCalendarDay {
                date: LocalDate::new(2026, 1, 1).unwrap(),
                kind: fc_domain::calendar::PublicDayKind::Rest,
                name: "Synthetic off-season fixture".into(),
            }],
            etag: None,
            last_modified: None,
        }],
    };
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
    let lesson_context = fc_domain::lesson_context::LessonContextSnapshot::default()
        .observe(&identity, &schedule, now(30300))
        .unwrap();
    let materials_source = lesson_context
        .material_source(&identity, &schedule, class, now(30300))
        .unwrap();
    DesktopFacts {
        global_revision: Revision::new(100),
        captured_at: now(30300),
        date_watermark: date(30),
        clock_rollback: false,
        identity,
        schedule,
        lesson_context,
        display: DisplaySettings::default(),
        profile_contents: Vec::new(),
        profiles: fc_domain::profile::ProfileSelectionSnapshot {
            reason: fc_domain::profile::ProfileSelectionReason::Missing,
            ..Default::default()
        },
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
    fresh.lesson_context = fresh
        .lesson_context
        .observe(&fresh.identity, &fresh.schedule, fresh.captured_at)
        .unwrap();
    if fresh.profiles.subject_profile.is_none() {
        fresh.profiles.reason = match fresh.lesson_context.current.as_ref() {
            None => fc_domain::profile::ProfileSelectionReason::Public,
            Some(c) if c.teacher_id.is_none() => {
                fc_domain::profile::ProfileSelectionReason::Unassigned
            }
            Some(_) => fc_domain::profile::ProfileSelectionReason::Missing,
        };
    }
    fresh.materials_source = if fresh.clock_rollback {
        None
    } else {
        fresh
            .identity
            .installation
            .as_ref()
            .and_then(|i| i.current_class)
            .and_then(|class| {
                fresh
                    .lesson_context
                    .material_source(&fresh.identity, &fresh.schedule, class, fresh.captured_at)
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
    facts.schedule.public_calendar_data.years.clear();
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
    facts.lesson_context = Default::default();
    facts.lesson_context = facts
        .lesson_context
        .observe(&facts.identity, &facts.schedule, facts.captured_at)
        .unwrap();
    facts.profiles.reason = fc_domain::profile::ProfileSelectionReason::Public;
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

#[test]
fn actual_override_switches_capsule_color_teacher_and_recess_source_as_one_scene() {
    use fc_domain::lesson_context::LessonAction;
    let mut facts = fixture();
    let class = ClassId::new(1).unwrap();
    let english = SubjectId::new(2).unwrap();
    for command in [
        IdentityCommand::PutSubject {
            id: english,
            name: "英语".into(),
            enabled: true,
            color: Rgb {
                red: 30,
                green: 80,
                blue: 220,
            },
            expected_revision: None,
        },
        IdentityCommand::UpdateAccount {
            id: id(1),
            display_name: "管理教师".into(),
            role: Role::GlobalAdmin,
            enabled: true,
            scopes: vec![
                AssignmentScope {
                    class_id: class,
                    subject_id: SubjectId::new(1).unwrap(),
                },
                AssignmentScope {
                    class_id: class,
                    subject_id: english,
                },
            ],
        },
    ] {
        let mut admin = actor();
        admin.auth_epoch = facts.identity.account(id(1)).unwrap().principal.auth_epoch;
        facts.identity = facts
            .identity
            .transition(&command, Some(&admin), now(30305))
            .unwrap()
            .next;
    }
    facts.lesson_context = facts
        .lesson_context
        .observe(&facts.identity, &facts.schedule, now(30310))
        .unwrap();
    let mut admin = actor();
    admin.auth_epoch = facts.identity.account(id(1)).unwrap().principal.auth_epoch;
    let key = facts.lesson_context.current.as_ref().unwrap().key.clone();
    facts.lesson_context = facts
        .lesson_context
        .apply(
            facts.lesson_context.revision,
            &key,
            LessonAction::Replace(CourseCell {
                subject_id: english,
                teacher: TeacherBinding::Explicit(id(1)),
            }),
            OverrideId::new(1).unwrap(),
            &facts.identity,
            &admin,
            now(30315),
        )
        .unwrap();
    facts.captured_at = now(30315);
    facts.materials_source = facts
        .lesson_context
        .material_source(&facts.identity, &facts.schedule, class, facts.captured_at)
        .unwrap();
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
    let current = rows.iter().find(|r| r.current).unwrap();
    assert_eq!(current.subject_id, english);
    assert_eq!(
        current.color,
        soft_subject_color(Rgb {
            red: 30,
            green: 80,
            blue: 220
        })
    );
    assert!(current.temporary && current.name.contains("原数学"));
    assert!(rows
        .iter()
        .filter(|r| !r.current)
        .all(|r| !r.temporary && r.subject_id.get() == 1));
    let control = view.lesson_control.as_ref().unwrap();
    assert!(
        control.can_change
            && control.caption.contains("管理教师")
            && control.caption.contains("仅本节有效")
    );
    assert!(view
        .cards
        .iter()
        .all(|c| c.rect.y >= control.rect.y + control.rect.height));
    assert_eq!(facts.materials_source.as_ref().unwrap().teacher_id, id(1));
    assert_eq!(facts.materials_source.as_ref().unwrap().subject_id, english);

    let mut restored = facts.clone();
    restored.lesson_context = restored
        .lesson_context
        .apply(
            restored.lesson_context.revision,
            &key,
            LessonAction::Restore,
            OverrideId::new(2).unwrap(),
            &restored.identity,
            &admin,
            now(30320),
        )
        .unwrap();
    restored.captured_at = now(30320);
    restored.materials_source = restored
        .lesson_context
        .material_source(
            &restored.identity,
            &restored.schedule,
            class,
            restored.captured_at,
        )
        .unwrap();
    let restored_view = scene(&restored);
    assert!(!restored_view
        .lesson_control
        .as_ref()
        .unwrap()
        .caption
        .contains("临时："));
    assert_eq!(
        restored.materials_source.as_ref().unwrap().teacher_id,
        id(2)
    );

    facts.captured_at = now(8 * 3600 + 45 * 60);
    facts.lesson_context = facts
        .lesson_context
        .observe(&facts.identity, &facts.schedule, facts.captured_at)
        .unwrap();
    facts.materials_source = facts
        .lesson_context
        .material_source(&facts.identity, &facts.schedule, class, facts.captured_at)
        .unwrap();
    let recess = scene(&facts);
    assert!(!recess.lesson_control.as_ref().unwrap().can_change);
    assert!(recess
        .cards
        .iter()
        .any(|c| matches!(c.content, CardContent::Recess { .. })));
    assert_eq!(
        facts.materials_source.as_ref().unwrap().kind,
        fc_domain::materials::MaterialSourceKind::RetainedBreak
    );
    assert_eq!(facts.materials_source.as_ref().unwrap().teacher_id, id(1));
    let CardContent::Courses(rows) = &recess
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Schedule)
        .unwrap()
        .content
    else {
        panic!()
    };
    assert!(rows.iter().all(|r| !r.temporary && r.subject_id.get() == 1));

    facts.captured_at = now(8 * 3600 + 55 * 60);
    facts.lesson_context = facts
        .lesson_context
        .observe(&facts.identity, &facts.schedule, facts.captured_at)
        .unwrap();
    facts.materials_source = facts
        .lesson_context
        .material_source(&facts.identity, &facts.schedule, class, facts.captured_at)
        .unwrap();
    let next = scene(&facts);
    assert!(next.lesson_control.as_ref().unwrap().can_change);
    assert_eq!(facts.materials_source.as_ref().unwrap().teacher_id, id(2));
}

fn selected_profile(facts: &mut DesktopFacts) {
    use fc_domain::profile::*;
    let current = facts.lesson_context.current.as_ref().unwrap();
    let key = SubjectProfileKey {
        class_id: current.key.class_id,
        subject_id: current.effective.subject_id,
        teacher_id: current.teacher_id.unwrap(),
    };
    let mut profile = SubjectProfileDocument::new(ProfileId::new(1).unwrap(), key);
    profile.revision = Revision::new(1);
    profile.layout_revision = Revision::new(1);
    facts.profiles.subject_profile = Some(profile);
    facts.profiles.reason = ProfileSelectionReason::Selected;
}
#[test]
fn exact_profile_changes_material_layout_style_and_rows_without_replacing_public_cards() {
    let mut facts = material_facts(5);
    let neutral = scene(&facts);
    selected_profile(&mut facts);
    let p = facts.profiles.subject_profile.as_mut().unwrap();
    p.materials.visible_rows = 1;
    p.materials.row_gap_dip = 23;
    p.materials.component.font_dip = 32;
    p.materials.component.opacity_percent = 50;
    p.materials.component.max_width = 400;
    let view = scene(&facts);
    let tree = view.paint_tree.as_ref().unwrap();
    assert_eq!(
        tree.selection_reason,
        fc_domain::profile::ProfileSelectionReason::Selected
    );
    assert_eq!(
        tree.subject_profile.unwrap().key,
        facts.profiles.subject_profile.as_ref().unwrap().key
    );
    for kind in [
        ComponentKind::Clock,
        ComponentKind::Schedule,
        ComponentKind::Homework,
    ] {
        let a = neutral.cards.iter().find(|c| c.kind == kind).unwrap();
        let b = view.cards.iter().find(|c| c.kind == kind).unwrap();
        assert_eq!(a.rect, b.rect);
        assert_eq!(a.content, b.content);
    }
    let material = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Materials)
        .unwrap();
    assert_eq!(material.font_dip, 32.);
    assert_eq!(material.component_alpha, 0.5);
    assert_eq!(material.surface_alpha, 0.03);
    assert_eq!(material.visible_leaf_limit, 1);
    let CardContent::Materials(list) = &material.content else {
        panic!()
    };
    assert_eq!(list.rows.len(), 5);
    assert_eq!(list.visible_rows, 1);
    assert_eq!(list.row_gap_dip, 23);
    let node = tree
        .nodes
        .iter()
        .find(|n| {
            n.card_index
                .is_some_and(|i| view.cards[i].kind == ComponentKind::Materials)
        })
        .unwrap();
    assert!(matches!(
        node.key.scope,
        fc_domain::layout::LayoutScope::Subject(_)
    ));
    assert_eq!(node.local_opacity, 0.5);
}
#[test]
fn full_scene_rejects_profile_for_other_teacher_instead_of_borrowing_layout() {
    let mut facts = fixture();
    selected_profile(&mut facts);
    let p = facts.profiles.subject_profile.as_mut().unwrap();
    p.key.teacher_id = if p.key.teacher_id == id(1) {
        id(2)
    } else {
        id(1)
    };
    p.tree.scope = fc_domain::layout::LayoutScope::Subject(p.key);
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
#[test]
fn public_fallback_retains_explicit_missing_reason_and_has_no_subject_nodes() {
    let facts = fixture();
    let view = scene(&facts);
    let tree = view.paint_tree.unwrap();
    assert_eq!(
        tree.selection_reason,
        fc_domain::profile::ProfileSelectionReason::Missing
    );
    assert!(tree.subject_profile.is_none());
    assert!(tree
        .nodes
        .iter()
        .all(|n| n.key.scope == fc_domain::layout::LayoutScope::Public));
}
#[test]
fn primitive_fill_is_not_premultiplied_by_component_group_alpha() {
    let mut facts = fixture();
    for k in [ComponentKind::Schedule, ComponentKind::Homework] {
        facts
            .display
            .components
            .iter_mut()
            .find(|c| c.kind == k)
            .unwrap()
            .opacity_percent = 40;
    }
    let view = scene(&facts);
    let courses = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Schedule)
        .unwrap();
    let CardContent::Courses(rows) = &courses.content else {
        panic!()
    };
    assert_eq!(rows[0].fill_alpha, 0.2);
    let homework = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Homework)
        .unwrap();
    let CardContent::Homework(rows) = &homework.content else {
        panic!()
    };
    assert_eq!(rows[0].fill_alpha, 0.03);
    assert_eq!(courses.surface_alpha, 0.03);
    assert_eq!(courses.edge_alpha, 0.5);
    assert_eq!(courses.component_alpha, 0.4);
}
#[test]
fn global_dynamic_leaf_limits_preserve_complete_owned_homework_and_material_dtos() {
    let mut facts = material_facts(5);
    let current = facts.lesson_context.current.as_ref().unwrap();
    let class_id = current.key.class_id;
    let subject_id = current.effective.subject_id;
    let template = facts.homework[0].clone();
    let mut records = Vec::new();
    for n in 10..73 {
        let teacher = id(n);
        let mut a = actor();
        a.auth_epoch = facts.identity.account(id(1)).unwrap().principal.auth_epoch;
        facts.identity = facts
            .identity
            .transition(
                &IdentityCommand::CreateAccount(NewAccount {
                    id: teacher,
                    login_name: format!("teacher{n}"),
                    display_name: format!("教师{n}"),
                    role: Role::TeacherAdmin,
                    scopes: vec![AssignmentScope {
                        class_id,
                        subject_id,
                    }],
                }),
                Some(&a),
                facts.captured_at,
            )
            .unwrap()
            .next;
        let mut record = template.clone();
        if let ContentKey::Homework(k) = &mut record.key {
            k.owner_teacher_id = teacher;
        }
        record.last_actor_id = teacher;
        records.push(record);
    }
    facts.global_revision = Revision::new(1000);
    facts.homework = records;
    facts.homework_eligible_total = 63;
    facts.lesson_context = facts
        .lesson_context
        .observe(&facts.identity, &facts.schedule, facts.captured_at)
        .unwrap();
    facts.materials_source = facts
        .lesson_context
        .material_source(
            &facts.identity,
            &facts.schedule,
            class_id,
            facts.captured_at,
        )
        .unwrap();
    facts.materials_page.as_mut().unwrap().source = facts.materials_source.clone();
    let view = scene(&facts);
    assert!(
        view.cards
            .iter()
            .map(|c| c.visible_leaf_limit as usize)
            .sum::<usize>()
            <= 64
    );
    for card in &view.cards {
        if let CardContent::Homework(rows) = &card.content {
            assert_eq!(rows.len(), 63);
            assert!(card.visible_leaf_limit < 63);
        }
        if let CardContent::Materials(rows) = &card.content {
            assert_eq!(rows.rows.len(), 5);
            assert!(card.visible_leaf_limit <= 5);
        }
    }
}
#[test]
fn transparent_public_ancestor_preserves_clock_slot_and_suppresses_seconds_work() {
    let mut facts = fixture();
    facts.display.clock_seconds = true;
    let before = scene(&facts);
    assert!(before.needs_second_tick());
    if let fc_domain::layout::NodeKind::Container(c) = &mut facts
        .profiles
        .public_layout
        .tree
        .nodes
        .iter_mut()
        .find(|n| n.id.get() == 2)
        .unwrap()
        .kind
    {
        c.group_opacity_percent = 0;
    }
    facts.profiles.public_layout.revision = Revision::new(1);
    let after = scene(&facts);
    assert_eq!(before.cards.len(), after.cards.len());
    assert!(!after.needs_second_tick());
    let a = before
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Clock)
        .unwrap();
    let b = after
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Clock)
        .unwrap();
    assert_eq!(a.rect, b.rect);
    assert_eq!(b.component_alpha, 1.);
    let node = after
        .paint_tree
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|n| n.widget_kind == Some(fc_domain::layout::WidgetKind::Clock))
        .unwrap();
    assert_eq!(node.effective_opacity, 0.);
}

#[test]
fn actual_profile_material_gap_and_warning_drive_intrinsic_scene_height() {
    // One real current teacher folder with three rows. Hide unrelated widgets so
    // height differences cannot merely be clipping against another card's budget.
    let mut facts = material_facts(3);
    selected_profile(&mut facts);
    for component in &mut facts.display.components {
        component.visible = component.kind == ComponentKind::Materials;
    }
    let profile = facts.profiles.subject_profile.as_mut().unwrap();
    profile.materials.component.font_dip = 18;
    profile.materials.component.max_height = 1200;
    profile.materials.row_gap_dip = 0;
    let zero = scene(&facts);
    facts
        .profiles
        .subject_profile
        .as_mut()
        .unwrap()
        .materials
        .row_gap_dip = 23;
    let middle = scene(&facts);
    facts
        .profiles
        .subject_profile
        .as_mut()
        .unwrap()
        .materials
        .row_gap_dip = 64;
    let large = scene(&facts);
    let height = |s: &SceneSnapshot| {
        s.cards
            .iter()
            .find(|c| c.kind == ComponentKind::Materials)
            .unwrap()
            .rect
            .height
    };
    assert!(height(&middle) > height(&zero) + 60.);
    assert!(height(&large) > height(&middle) + 100.);
    let card = large
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Materials)
        .unwrap();
    let expected = card_body_spec(card.kind, &card.content, false, None)
        .unwrap()
        .intrinsic_height(card.rect.width, card.font_dip, &Measure)
        .unwrap()
        .ceil();
    assert_eq!(card.rect.height, expected);

    let mut fallback = material_facts(3);
    for component in &mut fallback.display.components {
        component.visible = component.kind == ComponentKind::Materials;
    }
    let view = scene(&fallback);
    let card = view
        .cards
        .iter()
        .find(|c| c.kind == ComponentKind::Materials)
        .unwrap();
    let warning =
        profile_configuration_warning(view.paint_tree.as_ref().unwrap().selection_reason).unwrap();
    let spec = card_body_spec(card.kind, &card.content, false, Some(warning)).unwrap();
    assert_eq!(spec.blocks[0].text, warning);
    assert_eq!(
        card.rect.height,
        spec.intrinsic_height(card.rect.width, card.font_dip, &Measure)
            .unwrap()
            .ceil()
    );
}

#[path = "scene_content_tests.rs"]
mod independent_content;
