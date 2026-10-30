use super::*;
use crate::scene::{HomeworkEntry, MaterialListContent, SceneMaterialRow};
use fc_domain::{ids::*, materials::*, schedule::*, time::*};
use std::cell::RefCell;

struct Measure {
    calls: RefCell<Vec<(String, f32, f32)>>,
    height: f32,
}
impl TextMeasure for Measure {
    fn height(&self, text: &str, width: f32, font: f32) -> Result<f32, fc_ports::MeasureError> {
        self.calls.borrow_mut().push((text.into(), width, font));
        Ok(self.height)
    }
}
fn measure(height: f32) -> Measure {
    Measure {
        calls: RefCell::new(Vec::new()),
        height,
    }
}
fn color() -> Rgb {
    Rgb {
        red: 100,
        green: 150,
        blue: 200,
    }
}
fn homework() -> CardContent {
    CardContent::Homework(vec![HomeworkEntry {
        subject_id: SubjectId::new(1).unwrap(),
        teacher_id: PrincipalId::new(2).unwrap(),
        subject_name: "多行科目名称\n第二行".into(),
        teacher_name: "教师名字\n末行".into(),
        text: "正文第一行\n第二行\n正文最后一行".into(),
        color: color(),
        fill_alpha: 0.03,
    }])
}
fn materials(rows: usize, gap: u16, more: bool) -> CardContent {
    let class_id = ClassId::new(1).unwrap();
    let subject_id = SubjectId::new(2).unwrap();
    let teacher_id = PrincipalId::new(3).unwrap();
    let folder_id = FolderId::new(4).unwrap();
    let source = MaterialSource {
        kind: MaterialSourceKind::CurrentLesson,
        subject_id,
        teacher_id,
        evidence: MaterialSourceEvidence::BaseTimetableOnly,
        lesson: LessonInstanceKey {
            actual_date: LocalDate::new(2026, 10, 31).unwrap(),
            class_id,
            range: MinuteRange::new(Minute::new(480).unwrap(), Minute::new(525).unwrap()).unwrap(),
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
        },
    };
    CardContent::Materials(Box::new(MaterialListContent {
        source: source.clone(),
        folder_id: Some(folder_id),
        folder_revision: Revision::new(1),
        teacher_name: "真实老师长名字".repeat(6),
        subject_name: "独立稳定科目".into(),
        rows: (0..rows)
            .map(|n| SceneMaterialRow {
                material_id: MaterialId::new(n as u64 + 1).unwrap(),
                resource_version: ResourceVersionId::new(n as u64 + 1).unwrap(),
                name: format!("{}-{}-终.pptx", "长名字🦀".repeat(20), n),
            })
            .collect(),
        next_cursor: more.then_some(MaterialCursor {
            folder_id,
            folder_revision: Revision::new(1),
            scope: MaterialScope {
                teacher_id,
                subject_id,
            },
            class_id: Some(class_id),
            source: Some(source),
            offset: rows as u64,
        }),
        total: rows as u64 + u64::from(more),
        visible_rows: 5,
        row_gap_dip: gap,
    }))
}
#[test]
fn homework_measures_wrapped_label_and_body_as_two_complete_native_blocks() {
    let content = homework();
    let spec = card_body_spec(ComponentKind::Homework, &content, false, None).unwrap();
    assert_eq!(spec.blocks.len(), 2);
    assert_eq!(spec.blocks[0].text, "多行科目名称\n第二行 · 教师名字\n末行");
    assert_eq!(spec.blocks[1].text, "正文第一行\n第二行\n正文最后一行");
    assert_eq!(spec.blocks[0].leaf, Some(0));
    assert_eq!(spec.blocks[1].leaf, Some(0));
    let geometry = spec.arrange(&[54., 108.]).unwrap();
    assert_eq!(
        geometry.blocks,
        [
            CardBodyPosition {
                top: 0.,
                height: 70.
            },
            CardBodyPosition {
                top: 78.,
                height: 124.
            }
        ]
    );
    assert_eq!(geometry.total_body_height, 202.);
    let m = measure(54.);
    assert_eq!(
        spec.intrinsic_height(300., 22., &m).unwrap(),
        32. + 70. + 8. + 70. + 12.
    );
    let calls = m.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|(_, width, _)| *width == 252.));
    assert_eq!(calls[1].0, spec.blocks[1].text);
}
#[test]
fn material_rows_and_more_share_44_dip_and_exact_gap_without_trailing_gap() {
    for count in [1, 3, 5] {
        for gap in [0, 23, 64] {
            let spec = card_body_spec(
                ComponentKind::Materials,
                &materials(count, gap, true),
                false,
                Some("配置缺失，公共默认"),
            )
            .unwrap();
            assert_eq!(spec.blocks.len(), count + 3);
            assert_eq!(spec.gap, f32::from(gap));
            for i in 0..count {
                let b = &spec.blocks[i + 2];
                assert_eq!(b.role, CardBodyRole::MaterialRow(i));
                assert_eq!(b.min_height, 44.);
                assert!(b.text.ends_with("终.pptx"));
            }
            assert_eq!(spec.blocks.last().unwrap().role, CardBodyRole::MaterialMore);
            let heights = vec![20.; spec.blocks.len()];
            let geometry = spec.arrange(&heights).unwrap();
            let expected =
                2. * 36. + (count as f32 + 1.) * 44. + f32::from(gap) * (count as f32 + 2.);
            assert_eq!(geometry.total_body_height, expected);
            assert_eq!(geometry.blocks.last().unwrap().top + 44., expected);
            assert_eq!(
                spec.intrinsic_height(360., 18., &measure(20.)).unwrap(),
                32. + expected + 12.
            );
        }
    }
}
#[test]
fn material_warning_and_unverified_schedule_heading_are_not_lost_in_measurement() {
    let ordinary = card_body_spec(
        ComponentKind::Schedule,
        &CardContent::Text("普通课表".into()),
        false,
        None,
    )
    .unwrap();
    let unverified = card_body_spec(
        ComponentKind::Schedule,
        &CardContent::Text("普通课表".into()),
        true,
        None,
    )
    .unwrap();
    assert_eq!((ordinary.top_inset, unverified.top_inset), (32., 50.));
    assert_eq!(
        unverified
            .intrinsic_height(200., 18., &measure(20.))
            .unwrap()
            - ordinary.intrinsic_height(200., 18., &measure(20.)).unwrap(),
        18.
    );
    let neutral = card_body_spec(
        ComponentKind::Materials,
        &CardContent::MaterialsUnavailable,
        false,
        Some("精确科目未配置 · 使用公共布局"),
    )
    .unwrap();
    assert_eq!(neutral.blocks[0].text, "精确科目未配置 · 使用公共布局");
    assert_eq!(neutral.blocks[1].text, "资料尚未就绪");
    assert!(card_body_spec(
        ComponentKind::Date,
        &CardContent::Text("日期".into()),
        false,
        Some("不应串入其它卡")
    )
    .is_err());
}
#[test]
fn final_line_padding_is_part_of_intrinsic_height_even_for_plain_last_block() {
    let spec = card_body_spec(
        ComponentKind::Date,
        &CardContent::Text("最后一行".into()),
        false,
        None,
    )
    .unwrap();
    let geometry = spec.arrange(&[27.5]).unwrap();
    assert_eq!(geometry.total_body_height, 43.5);
    assert_eq!(
        spec.intrinsic_height(160., 18., &measure(27.5)).unwrap(),
        87.5
    );
}
#[test]
fn long_utf16_bodies_are_preserved_completely_instead_of_16k_samples() {
    let text = format!("前{}终", "🦀".repeat(MAX_CARD_TEXT_UNITS / 2 - 1));
    assert_eq!(text.encode_utf16().count(), MAX_CARD_TEXT_UNITS);
    let spec = card_body_spec(
        ComponentKind::Homework,
        &CardContent::Text(text.clone()),
        false,
        None,
    )
    .unwrap();
    assert_eq!(spec.text_units().unwrap(), MAX_CARD_TEXT_UNITS);
    assert_eq!(spec.blocks[0].text, text);
    let m = measure(10000.);
    spec.intrinsic_height(800., 18., &m).unwrap();
    assert_eq!(m.calls.borrow()[0].0, text);
    let too_long = CardContent::Text(format!("{}x", text));
    assert_eq!(
        card_body_spec(ComponentKind::Homework, &too_long, false, None),
        Err(CardBodyError::ContentLimit)
    );
}
#[test]
fn original_course_homework_material_counts_and_utf16_limits_are_enforced() {
    let CardContent::Materials(folder) = materials(1, 8, false) else {
        panic!()
    };
    let course = crate::scene::CourseCapsule {
        key: folder.source.lesson.clone(),
        subject_id: folder.source.subject_id,
        name: "课程名称".into(),
        color: color(),
        current: false,
        temporary: false,
        fill_alpha: 0.2,
        stroke_width: 0.,
    };
    let courses = card_body_spec(
        ComponentKind::Schedule,
        &CardContent::Courses(vec![course.clone(); 128]),
        true,
        None,
    )
    .unwrap();
    assert_eq!(courses.blocks.len(), 128);
    assert!(card_body_spec(
        ComponentKind::Schedule,
        &CardContent::Courses(vec![course; 129]),
        false,
        None
    )
    .is_err());
    let CardContent::Homework(entries) = homework() else {
        panic!()
    };
    let spec = card_body_spec(
        ComponentKind::Homework,
        &CardContent::Homework(vec![entries[0].clone(); 64]),
        false,
        None,
    )
    .unwrap();
    assert_eq!(spec.blocks.len(), 128);
    assert!(card_body_spec(
        ComponentKind::Homework,
        &CardContent::Homework(vec![entries[0].clone(); 65]),
        false,
        None
    )
    .is_err());
    assert!(card_body_spec(
        ComponentKind::Materials,
        &materials(6, 8, false),
        false,
        None
    )
    .is_err());
    assert!(card_body_spec(
        ComponentKind::Materials,
        &materials(1, 65, false),
        false,
        None
    )
    .is_err());
    let mut bad = spec.clone();
    bad.blocks[0].fill.as_mut().unwrap().1 = f32::NAN;
    assert_eq!(bad.validate(), Err(CardBodyError::InvalidContent));
}
#[test]
fn invalid_measures_geometry_count_and_total_cannot_enter_positions() {
    let spec = card_body_spec(
        ComponentKind::Date,
        &CardContent::Text("合法".into()),
        false,
        None,
    )
    .unwrap();
    for height in [f32::NAN, f32::INFINITY, -1., f32::MAX] {
        assert_eq!(
            spec.arrange(&[height]),
            Err(CardBodyError::InvalidMeasurement)
        );
        assert_eq!(
            spec.intrinsic_height(200., 18., &measure(height)),
            Err(CardBodyError::InvalidMeasurement)
        );
    }
    assert_eq!(spec.arrange(&[]), Err(CardBodyError::InvalidMeasurement));
    for width in [0., 48., f32::NAN, f32::INFINITY] {
        assert!(spec.intrinsic_height(width, 18., &measure(20.)).is_err());
    }
    struct Fails;
    impl TextMeasure for Fails {
        fn height(&self, _: &str, _: f32, _: f32) -> Result<f32, fc_ports::MeasureError> {
            Err(fc_ports::MeasureError::Unmeasurable)
        }
    }
    assert_eq!(
        spec.intrinsic_height(200., 18., &Fails),
        Err(CardBodyError::Measure)
    );
}
#[test]
fn dynamic_clock_and_control_use_their_native_patch_width_and_insets() {
    for mode in [ClockMode::Digital24, ClockMode::Digital12] {
        let spec = card_body_spec(
            ComponentKind::Clock,
            &CardContent::Clock(ClockContent {
                mode,
                show_seconds: true,
            }),
            false,
            None,
        )
        .unwrap();
        let m = measure(42.);
        assert_eq!(spec.intrinsic_height(280., 40., &m).unwrap(), 96.);
        assert_eq!(m.calls.borrow()[0].1, 248.);
        assert!(spec.blocks.is_empty());
    }
    let analog = card_body_spec(
        ComponentKind::Clock,
        &CardContent::Clock(ClockContent {
            mode: ClockMode::Analog,
            show_seconds: true,
        }),
        false,
        None,
    )
    .unwrap();
    let m = measure(200.);
    assert_eq!(analog.intrinsic_height(280., 40., &m).unwrap(), 160.);
    assert!(m.calls.borrow().is_empty());
    let control = card_body_spec(
        ComponentKind::Schedule,
        &CardContent::LessonControl {
            caption: "明确的本节控制".into(),
            can_change: true,
        },
        false,
        None,
    )
    .unwrap();
    let m = measure(40.);
    assert_eq!(control.intrinsic_height(400., 16., &m).unwrap(), 64.);
    assert_eq!(m.calls.borrow()[0].1, 220.);
}
