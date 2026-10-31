//! Cross-layer checks: presentation assigns the card rectangle, then actual
//! DirectWrite lines, native clipping and input rectangles must fit that box.
//! No expected positions are copied from CardBodySpec::arrange.
use super::*;
use fc_domain::{
    ids::{AuthEpoch, PrincipalId, SubjectId},
    layout::{default_public_layout, WidgetKind},
    profile::ProfileSelectionReason,
};
use fc_presentation::{
    layout::{compose_layout, PaintGlass, WidgetMeasure},
    scene::{HomeworkEntry, PageInfo},
};
use fc_runtime::authority::AuthorityStore;
use std::time::{Duration, Instant};

fn gate() -> (fc_runtime::authority::AuthorityPublisher<()>, HealthGate) {
    let (mut publisher, reader) = AuthorityStore::new((), Revision::new(5), AuthEpoch::new(1));
    publisher.initialize().unwrap();
    (publisher, reader.gate())
}
fn guard(gate: &HealthGate, generation: SceneGeneration) -> PublicationGuard<'_> {
    PublicationGuard {
        gate,
        lease: gate
            .lease_for_authorized_scope(DESKTOP_SCOPE, Instant::now() + Duration::from_secs(120))
            .unwrap(),
        current_generation: generation,
        settings_open: false,
    }
}
fn material_scene() -> SceneSnapshot {
    super::tests::material_scene(crate::native::local_now().unwrap())
}
fn assigned(mut scene: SceneSnapshot, note: bool) -> SceneSnapshot {
    let card = &mut scene.cards[0];
    let kind = match card.kind {
        ComponentKind::Materials => WidgetKind::Materials,
        ComponentKind::Homework => WidgetKind::Homework,
        ComponentKind::Schedule => WidgetKind::Schedule,
        ComponentKind::Reminder => WidgetKind::Reminder,
        _ => panic!("static body fixture"),
    };
    let reason = if note {
        ProfileSelectionReason::LayoutMismatch
    } else {
        ProfileSelectionReason::Public
    };
    let warning = fc_presentation::scene::profile_configuration_warning(reason);
    let body = card_body_spec(
        card.kind,
        &card.content,
        scene.public_calendar_unverified,
        warning,
    )
    .unwrap();
    let measure = NativeTextMeasure::new().unwrap();
    let mut tree = compose_layout(
        &default_public_layout(),
        None,
        &[WidgetMeasure {
            key: fc_presentation::scene::SceneNodeKey {
                scope: fc_domain::layout::LayoutScope::Public,
                id: default_public_layout()
                    .nodes
                    .iter()
                    .find(|n| n.kind == fc_domain::layout::NodeKind::Widget(kind))
                    .unwrap()
                    .id,
            },
            kind,
            body,
            font_dip: card.font_dip,
            opacity: 1.,
            min_width: 520.,
            max_width: 520.,
            preferred_width: 520.,
            min_height: 64.,
            max_height: 2048.,
            glass: PaintGlass {
                surface_alpha: 0.03,
                edge_alpha: 0.5,
            },
        }],
        Rect {
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1080.,
        },
        &measure,
    )
    .unwrap();
    let node = tree
        .nodes
        .iter_mut()
        .find(|n| n.widget_kind == Some(kind))
        .unwrap();
    card.rect = node.rect;
    node.card_index = Some(0);
    tree.selection_reason = reason;
    scene.paint_tree = Some(tree);
    scene
}
fn line_lengths(block: &Block) -> usize {
    let mut count = 0;
    unsafe {
        let _ = block.layout.GetLineMetrics(None, &mut count);
    }
    let mut lines = vec![DWRITE_LINE_METRICS::default(); count as usize];
    unsafe {
        block
            .layout
            .GetLineMetrics(Some(&mut lines), &mut count)
            .unwrap();
    }
    lines.iter().map(|l| l.length as usize).sum()
}
fn covered_lines(card: &Card, single_page: bool) {
    if single_page {
        assert_eq!(
            card.body.pages.len(),
            1,
            "presentation admitted the complete body but native paged"
        );
    }
    let viewport = card.source.rect.height - card.body.top_inset - card.body.bottom_inset;
    for block in &card.body.blocks {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe {
            block.layout.GetMetrics(&mut metrics).unwrap();
        }
        assert!(
            block.height + 0.01 >= metrics.height + 16.,
            "native block clips its actual DWrite bottom padding"
        );
        assert!(metrics.width <= card.source.rect.width - 48. + 0.1);
        for (line, height) in &block.lines {
            assert!(
                card.body.pages.iter().enumerate().any(|(i, offset)| {
                    let end = card
                        .body
                        .pages
                        .get(i + 1)
                        .copied()
                        .unwrap_or(*offset + viewport);
                    *line >= *offset - 0.1 && *line + *height <= end + 0.1
                }),
                "an actual DWrite line is never completely visible on any page: {line}/{height}"
            );
            if single_page {
                assert!(
                    card.body.top_inset + *line + *height
                        <= card.source.rect.height - card.body.bottom_inset + 0.1
                );
            }
        }
    }
    if single_page {
        let last = card.body.blocks.last().unwrap();
        assert!(
            last.top + last.height <= viewport + 0.1,
            "full final block padding exceeds native clip"
        );
    }
}
fn render(
    scene: &SceneSnapshot,
) -> (
    fc_runtime::authority::AuthorityPublisher<()>,
    HealthGate,
    DesktopRenderer,
) {
    let (publisher, gate) = gate();
    let mut renderer = DesktopRenderer::new_hidden().unwrap();
    assert_eq!(
        renderer
            .present(scene, scene.captured_at, &guard(&gate, scene.generation))
            .unwrap(),
        PresentOutcome::Offscreen
    );
    assert_eq!(renderer.last_metrics().visible_windows, 0);
    assert!(
        renderer.last_metrics().dib_bytes + renderer.last_metrics().scratch_peak_bytes
            <= DIB_BUDGET
    );
    (publisher, gate, renderer)
}

#[test]
fn shared_body_materials_actual_compositor_one_page_with_gaps_rows_note_and_full_more_target() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    for gap in [23, 64] {
        for count in 1..=5 {
            let mut scene = material_scene();
            let CardContent::Materials(list) = &mut scene.cards[0].content else {
                unreachable!()
            };
            list.rows.truncate(count);
            list.visible_rows = count as u8;
            list.row_gap_dip = gap;
            list.next_cursor.as_mut().unwrap().offset = count as u64;
            scene.cards[0].page.shown = count as u64;
            scene.cards[0].font_dip = 14.;
            let scene = assigned(scene, true);
            let (_publisher, gate, mut renderer) = render(&scene);
            let card = &renderer.cards[0];
            covered_lines(card, true);
            // Warning and subject/teacher heading precede the rows. Their
            // presence must not move file actions onto header blocks.
            assert!(card.body.blocks[0].action.is_none());
            assert!(card.body.blocks[1].action.is_none());
            let targets = card.input.as_ref().unwrap().state.targets.borrow();
            let file_targets: Vec<_> = targets
                .iter()
                .filter_map(|t| match t.action {
                    InputAction::MaterialOpen { material_id, .. } => Some((material_id, t.rect)),
                    _ => None,
                })
                .collect();
            assert_eq!(file_targets.len(), count);
            for (i, (id, rect)) in file_targets.iter().enumerate() {
                assert_eq!(id.get(), i as u64 + 1);
                assert!(rect.height >= 44. - 0.1);
                assert!(
                    rect.y >= card.body.top_inset
                        && rect.y + rect.height
                            <= card.source.rect.height - card.body.bottom_inset + 0.1
                );
            }
            let more = targets
                .iter()
                .find(|t| matches!(t.action, InputAction::MaterialPage { .. }))
                .unwrap();
            assert!(
                more.rect.height >= 44. - 0.1,
                "more must remain a complete44DIP button"
            );
            assert!(
                more.rect.y + more.rect.height
                    <= card.source.rect.height - card.body.bottom_inset + 0.1
            );
            let actions: Vec<_> = card
                .body
                .blocks
                .iter()
                .filter(|b| b.action.is_some())
                .collect();
            assert_eq!(actions.len(), count + 1);
            for pair in actions.windows(2) {
                assert!((pair[1].top - pair[0].top - pair[0].height - f32::from(gap)).abs() < 0.1);
            }
            drop(targets);
            assert_eq!(renderer.last_metrics().pages[0].pages, 1);
            // Exercise the actual clipped tree input aggregate, not just the
            // local card rectangles inspected above.
            for id in 1..=count as u64 {
                renderer
                    .probe_material_open(MaterialId::new(id).unwrap())
                    .unwrap();
                let events = renderer.take_events().unwrap();
                assert!(matches!(events.as_slice(), [DesktopEvent::MaterialOpen {
                    generation, material_id, resource_version, ..
                }] if *generation == scene.generation && material_id.get() == id && resource_version.get() == id + 100));
            }
            renderer.probe_material_more().unwrap();
            assert!(
                matches!(renderer.take_events().unwrap().as_slice(), [DesktopEvent::MaterialPage {generation, cursor, ..}]
                if *generation == scene.generation && cursor.offset == count as u64)
            );
            gate.stop();
        }
    }
}
fn homework(count: usize, body: &str) -> Vec<HomeworkEntry> {
    (0..count)
        .map(|i| HomeworkEntry {
            subject_id: SubjectId::new(i as u64 + 1).unwrap(),
            teacher_id: PrincipalId::new(i as u64 + 1).unwrap(),
            subject_name: format!("长学科名称第{i}组 {}", "综合实践学习内容".repeat(4)),
            teacher_name: format!("实际老师{i}号 {}", "多范围任课教师".repeat(3)),
            text: body.into(),
            color: Rgb {
                red: 80,
                green: 120,
                blue: 160,
            },
            fill_alpha: 0.03,
        })
        .collect()
}
#[test]
fn shared_body_homework_labels_and_text_use_actual_width_full_utf16_and_page_line_coverage() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let rows = homework(
        2,
        "逐行阅读并回答问题。😀\n保留每条完整正文。\n完成后核对。",
    );
    let expected_units: usize = rows
        .iter()
        .map(|r| {
            format!("{} · {}", r.subject_name, r.teacher_name)
                .encode_utf16()
                .count()
                + r.text.encode_utf16().count()
        })
        .sum();
    let mut scene = material_scene();
    scene.cards[0].kind = ComponentKind::Homework;
    scene.cards[0].content = CardContent::Homework(rows);
    scene.cards[0].font_dip = 14.;
    scene.cards[0].page = PageInfo::default();
    let mut scene = assigned(scene, false);
    let (_publisher, _gate, renderer) = render(&scene);
    let card = &renderer.cards[0];
    covered_lines(card, true);
    assert_eq!(card.body.blocks.len(), 4);
    assert_eq!(card.body.units, expected_units);
    assert_eq!(
        card.body.blocks.iter().map(line_lengths).sum::<usize>(),
        expected_units
    );
    for pair in card.body.blocks.chunks_exact(2) {
        assert!(pair[0].fill.is_some() && pair[1].fill.is_none());
        assert_eq!(pair[0].leaf, pair[1].leaf);
    }
    drop(renderer);
    scene.paint_tree = None;
    scene.cards[0].rect.height = 150.;
    let (_publisher, _gate, renderer) = render(&scene);
    assert!(renderer.cards[0].body.pages.len() > 1);
    covered_lines(&renderer.cards[0], false);
    assert_eq!(renderer.cards[0].body.units, expected_units);
}
#[test]
fn shared_body_course_unverified_subtitle_reserves_real_header_and_complete_capsule_padding() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut heights = Vec::new();
    for unverified in [false, true] {
        let mut scene = material_scene();
        let CardContent::Materials(list) = &scene.cards[0].content else {
            unreachable!()
        };
        let key = list.source.lesson.clone();
        scene.cards[0].kind = ComponentKind::Schedule;
        scene.cards[0].content = CardContent::Courses(
            (0..3)
                .map(|i| fc_presentation::scene::CourseCapsule {
                    key: key.clone(),
                    subject_id: SubjectId::new(i + 1).unwrap(),
                    name: format!("完整课程名称{i}：阅读、讨论与实践"),
                    color: Rgb {
                        red: 100,
                        green: 140,
                        blue: 180,
                    },
                    current: i == 0,
                    temporary: i == 0,
                    fill_alpha: 0.2,
                    stroke_width: 0.,
                })
                .collect(),
        );
        scene.cards[0].font_dip = 18.;
        scene.public_calendar_unverified = unverified;
        let scene = assigned(scene, false);
        let (_publisher, _gate, renderer) = render(&scene);
        let card = &renderer.cards[0];
        covered_lines(card, true);
        assert_eq!(card.body.top_inset, if unverified { 50. } else { 32. });
        if unverified {
            let subtitle = renderer
                .graphics
                .layout("调休资料未核验", card.source.rect.width - 32., 11., false)
                .unwrap();
            let mut metrics = DWRITE_TEXT_METRICS::default();
            unsafe {
                subtitle.GetMetrics(&mut metrics).unwrap();
            }
            assert!(
                34. + metrics.height <= card.body.top_inset + 0.1,
                "the actual unverified subtitle overlaps the first course block"
            );
        }
        assert_eq!(card.body.blocks.len(), 3);
        assert!(
            card.body.blocks[0].lines.len() >= 2,
            "temporary-course status must occupy real lines"
        );
        heights.push(card.source.rect.height);
    }
    assert!((heights[1] - heights[0] - 18.).abs() < 0.1);
}
#[test]
fn shared_body_text_over_fifty_pages_and_full_sixty_four_homework_rows_keep_existing_bounds() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut scene = material_scene();
    scene.paint_tree = None;
    scene.cards[0].kind = ComponentKind::Reminder;
    let text = "完整行😀不得丢失。\n".repeat(700);
    let expected = text.encode_utf16().count();
    scene.cards[0].content = CardContent::Text(text);
    scene.cards[0].rect.width = 320.;
    scene.cards[0].rect.height = 160.;
    scene.cards[0].font_dip = 18.;
    let (_publisher, _gate, renderer) = render(&scene);
    let card = &renderer.cards[0];
    assert!(
        card.body.pages.len() > 50,
        "text retains the16384page limit;50 belongs to image documents"
    );
    covered_lines(card, false);
    assert_eq!(card.body.units, expected);
    assert_eq!(
        card.body.blocks.iter().map(line_lengths).sum::<usize>(),
        expected
    );
    drop(renderer);
    scene.cards[0].kind = ComponentKind::Homework;
    let rows = homework(64, "每位老师的完整作业😀");
    let expected: usize = rows
        .iter()
        .map(|r| {
            format!("{} · {}", r.subject_name, r.teacher_name)
                .encode_utf16()
                .count()
                + r.text.encode_utf16().count()
        })
        .sum();
    scene.cards[0].content = CardContent::Homework(rows);
    scene.cards[0].rect.width = 520.;
    scene.cards[0].rect.height = 300.;
    scene.cards[0].font_dip = 14.;
    let (_publisher, _gate, renderer) = render(&scene);
    assert_eq!(renderer.cards[0].body.blocks.len(), 128);
    assert_eq!(renderer.cards[0].body.units, expected);
    covered_lines(&renderer.cards[0], false);
    drop(renderer);
    let _apartment = Apartment::new().unwrap();
    let graphics = Graphics::new().unwrap();
    scene.cards[0].content = CardContent::Text("a".repeat(MAX_TEXT_UNITS));
    let full = prepare_content(&graphics, &scene.cards[0], false).unwrap();
    assert_eq!(full.units, MAX_TEXT_UNITS);
    assert_eq!(
        full.blocks.iter().map(line_lengths).sum::<usize>(),
        MAX_TEXT_UNITS
    );
    drop(full);
    scene.cards[0].content = CardContent::Text("a".repeat(MAX_TEXT_UNITS + 1));
    assert!(prepare_content(&graphics, &scene.cards[0], false).is_err());
    scene.cards[0].content = CardContent::Homework(homework(65, "超出界限"));
    assert!(prepare_content(&graphics, &scene.cards[0], false).is_err());
    let material = material_scene();
    let CardContent::Materials(list) = &material.cards[0].content else {
        unreachable!()
    };
    let courses = |count| {
        CardContent::Courses(
            (0..count)
                .map(|i| fc_presentation::scene::CourseCapsule {
                    key: list.source.lesson.clone(),
                    subject_id: SubjectId::new(i + 1).unwrap(),
                    name: format!("完整课程{i}"),
                    color: Rgb {
                        red: 100,
                        green: 140,
                        blue: 180,
                    },
                    current: false,
                    temporary: false,
                    fill_alpha: 0.03,
                    stroke_width: 0.,
                })
                .collect(),
        )
    };
    scene.cards[0].kind = ComponentKind::Schedule;
    scene.cards[0].content = courses(128);
    let error = prepare_content(&graphics, &scene.cards[0], false)
        .err()
        .expect("long passive timetable must report capacity");
    assert!(is_read_only_capacity_error(&error));
    scene.cards[0].content = courses(129);
    assert!(prepare_content(&graphics, &scene.cards[0], false).is_err());
}
