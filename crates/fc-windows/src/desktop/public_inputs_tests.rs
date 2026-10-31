//! Actual hidden native input surfaces: passive facts have no hit targets.
use super::*;
use fc_domain::ids::AuthEpoch;
use fc_presentation::scene::PageInfo;
use fc_runtime::authority::AuthorityStore;
use std::time::{Duration, Instant};
#[test]
fn passive_facts_have_no_flat_input_or_queued_page_route_but_lesson_control_does() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (mut publisher, reader) = AuthorityStore::new((), Revision::new(5), AuthEpoch::new(1));
    publisher.initialize().unwrap();
    let gate = reader.gate();
    let now = crate::native::local_now().unwrap();
    let mut scene = SceneSnapshot {
        generation: SceneGeneration::new(1),
        source_revision: Revision::new(5),
        captured_at: now,
        valid_until: None,
        cards: vec![],
        paint_tree: None,
        text_tone: TextTone::Light,
        clock_rollback: false,
        public_calendar_unverified: false,
        lesson_control: None,
    };
    for (i, kind) in [
        ComponentKind::Clock,
        ComponentKind::Date,
        ComponentKind::Schedule,
        ComponentKind::DateCountdown,
        ComponentKind::Recess,
    ]
    .into_iter()
    .enumerate()
    {
        scene.cards.push(SceneCard {
            kind,
            content_identity: None,
            rect: Rect {
                x: i as f32 * 210.,
                y: 0.,
                width: 200.,
                height: 180.,
            },
            font_dip: 18.,
            component_alpha: 1.,
            surface_alpha: 0.03,
            edge_alpha: 0.5,
            content: CardContent::Text("只读事实".into()),
            visible_leaf_limit: 64,
            page: PageInfo {
                offset: 0,
                shown: 1,
                total: 2,
            },
        });
    }
    let guard = PublicationGuard {
        gate: &gate,
        lease: gate
            .lease_for_authorized_scope(DESKTOP_SCOPE, Instant::now() + Duration::from_secs(30))
            .unwrap(),
        current_generation: scene.generation,
        settings_open: false,
    };
    let mut renderer = DesktopRenderer::new_hidden().unwrap();
    renderer.present(&scene, now, &guard).unwrap();
    assert!(renderer.cards.iter().all(|c| c.input.is_none()));
    assert_eq!(renderer.diagnostics().input_regions, 0);
    for index in 0..renderer.cards.len() {
        let before = renderer.cards[index].page;
        route_action(
            &mut renderer.cards[index],
            scene.generation,
            InputAction::Page(true),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(renderer.cards[index].page, before);
    }
    assert!(renderer.take_events().unwrap().is_empty());
    scene.lesson_control = Some(fc_presentation::scene::LessonControlContent {
        rect: Rect {
            x: 0.,
            y: 200.,
            width: 400.,
            height: 80.,
        },
        caption: "本节临时换课".into(),
        can_change: true,
    });
    renderer.present(&scene, now, &guard).unwrap();
    renderer.probe_lesson_control().unwrap();
    assert_eq!(
        renderer.take_events().unwrap(),
        vec![DesktopEvent::LessonManage {
            generation: scene.generation
        }]
    );
    gate.stop();
}
#[test]
fn passive_overflow_is_explicit_capacity_error_after_complete_measurement() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let renderer = DesktopRenderer::new_hidden().unwrap();
    let graphics = &renderer.graphics;
    let card = SceneCard {
        kind: ComponentKind::Schedule,
        content_identity: None,
        rect: Rect {
            x: 0.,
            y: 0.,
            width: 220.,
            height: 100.,
        },
        font_dip: 18.,
        component_alpha: 1.,
        surface_alpha: 0.03,
        edge_alpha: 0.5,
        content: CardContent::Text("完整课表内容\n".repeat(20)),
        visible_leaf_limit: 64,
        page: Default::default(),
    };
    let error = prepare_content(graphics, &card, false)
        .err()
        .expect("must not publish first inaccessible page");
    assert!(is_read_only_capacity_error(&error));
    let mut readable = card;
    readable.kind = ComponentKind::SubjectText;
    assert!(
        prepare_content(graphics, &readable, false)
            .unwrap()
            .pages
            .len()
            > 1
    );
}
