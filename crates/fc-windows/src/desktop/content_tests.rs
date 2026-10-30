use super::*;
use fc_domain::{
    ids::{AuthEpoch, LayoutNodeId, MediaVersionId},
    layout::{LayoutScope, WidgetKind},
    profile::ProfileSelectionReason,
    profile_content::ImageFormat,
};
use fc_presentation::{
    layout::PaintGlass,
    scene::{SceneImageContent, ScenePaintNode, ScenePaintTree},
};
use fc_runtime::authority::AuthorityStore;
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
fn identity(id: u64) -> SceneContentIdentity {
    SceneContentIdentity {
        slot: ContentSlotKey {
            scope: LayoutScope::Public,
            node_id: LayoutNodeId::new(id).unwrap(),
        },
        revision: Revision::new(2),
        view_revision: Revision::new(3),
    }
}
fn reference(id: u64) -> MediaReference {
    MediaReference {
        owner: identity(id).slot,
        version_id: MediaVersionId::new(1).unwrap(),
        sha256: [
            237, 138, 223, 91, 9, 234, 181, 68, 189, 135, 125, 199, 189, 22, 107, 247, 80, 64, 90,
            124, 78, 77, 87, 247, 13, 35, 224, 116, 35, 120, 183, 43,
        ],
        bytes: 78,
        width: 4,
        height: 2,
        format: ImageFormat::Bmp,
    }
}
fn card(id: u64, image: bool) -> SceneCard {
    SceneCard {
        content_identity: Some(identity(id)),
        kind: if image {
            ComponentKind::SubjectImage
        } else {
            ComponentKind::SubjectText
        },
        rect: Rect {
            x: 20. + (id - 20) as f32 * 420.,
            y: 20.,
            width: 400.,
            height: 300.,
        },
        font_dip: 18.,
        component_alpha: 1.,
        surface_alpha: 0.,
        edge_alpha: 0.,
        content: if image {
            CardContent::Image(SceneImageContent {
                current: reference(id),
                current_page: 0,
                total_pages: 3,
            })
        } else {
            CardContent::StandaloneText(Arc::from("独立正文\n".repeat(500)))
        },
        page: Default::default(),
        visible_leaf_limit: 1,
    }
}
fn scene(cards: Vec<SceneCard>) -> SceneSnapshot {
    let now = crate::native::local_now().unwrap();
    let nodes = cards
        .iter()
        .enumerate()
        .map(|(index, card)| ScenePaintNode {
            key: SceneNodeKey {
                scope: LayoutScope::Public,
                id: card.content_identity.unwrap().slot.node_id,
            },
            parent: None,
            rect: card.rect,
            clip: card.rect,
            local_opacity: card.component_alpha,
            effective_opacity: card.component_alpha,
            glass: Some(PaintGlass {
                surface_alpha: card.surface_alpha,
                edge_alpha: card.edge_alpha,
            }),
            card_index: Some(index),
            widget_kind: Some(if card.kind == ComponentKind::SubjectImage {
                WidgetKind::SubjectImage
            } else {
                WidgetKind::SubjectText
            }),
        })
        .collect();
    SceneSnapshot {
        generation: SceneGeneration::new(10),
        source_revision: Revision::new(5),
        captured_at: now,
        valid_until: None,
        cards,
        paint_tree: Some(ScenePaintTree {
            nodes,
            public_layout_revision: Revision::new(1),
            subject_profile: None,
            selection_reason: ProfileSelectionReason::Public,
        }),
        text_tone: TextTone::Light,
        clock_rollback: false,
        public_calendar_unverified: false,
        lesson_control: None,
    }
}
fn authority() -> (fc_runtime::authority::AuthorityPublisher<()>, HealthGate) {
    let (mut p, r) = AuthorityStore::new((), Revision::new(5), AuthEpoch::new(1));
    p.initialize().unwrap();
    (p, r.gate())
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
#[test]
fn premultiplied_blend_scales_without_touching_padding_or_double_alpha() {
    let mut pixels = vec![0; 4 * 4 * 4];
    blend_pixels(
        &mut pixels,
        16,
        (1, 1, 3, 3),
        &[0, 0, 128, 128],
        4,
        1,
        1,
        0.5,
    );
    assert_eq!(&pixels[20..24], &[0, 0, 64, 64]);
    assert_eq!(&pixels[..20], &[0; 20]);
    blend_pixels(
        &mut pixels,
        16,
        (1, 1, 3, 3),
        &[0, 0, 128, 128],
        4,
        1,
        1,
        0.5,
    );
    assert_eq!(&pixels[20..24], &[0, 0, 112, 112]);
    assert_eq!(&pixels[28..32], &[0; 4]);
    let thin = pixel_bounds(
        Rect {
            x: 1.99999,
            y: 0.,
            width: 0.00002,
            height: 4.,
        },
        4,
        4,
    )
    .unwrap();
    assert!(thin.2 > thin.0 && thin.3 > thin.1);
    let mut blank = card(20, false);
    blank.content = CardContent::StandaloneText(Arc::from(""));
    assert!(validate_identity(&blank).is_ok());
}
#[test]
fn scoped_requests_reject_owner_revision_and_tree_aliases() {
    let good = scene(vec![card(20, true), card(21, true)]);
    let requests = requests(&good).unwrap();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].identity.slot, requests[1].identity.slot);
    let mut bad = good.clone();
    bad.cards[1].content_identity = bad.cards[0].content_identity;
    assert!(requests_for(&bad).is_err());
    let mut bad = good.clone();
    bad.cards[0].content_identity.as_mut().unwrap().revision = Revision::new(0);
    assert!(requests_for(&bad).is_err());
    let mut bad = good.clone();
    bad.paint_tree.as_mut().unwrap().nodes[0].key.id = LayoutNodeId::new(99).unwrap();
    assert!(requests_for(&bad).is_err());
    let mut bad = good.clone();
    if let CardContent::Image(image) = &mut bad.cards[0].content {
        image.current_page = 50;
    }
    assert!(requests_for(&bad).is_err());
}
fn requests_for(scene: &SceneSnapshot) -> Result<Vec<ImageRenderRequest>> {
    requests(scene)
}
#[test]
fn actual_hidden_two_text_nodes_page_only_the_exact_scoped_node() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let scene = scene(vec![card(20, false), card(21, false)]);
    let (_p, gate) = authority();
    let mut renderer = DesktopRenderer::new_hidden().unwrap();
    assert_eq!(
        renderer
            .present(&scene, scene.captured_at, &guard(&gate, scene.generation))
            .unwrap(),
        PresentOutcome::Offscreen
    );
    assert!(renderer
        .probe_next_page(ComponentKind::SubjectText, true)
        .is_err());
    renderer
        .probe_next_page_node(
            SceneNodeKey {
                scope: LayoutScope::Public,
                id: LayoutNodeId::new(21).unwrap(),
            },
            true,
        )
        .unwrap();
    assert!(renderer.take_events().unwrap().is_empty());
    assert_eq!(renderer.cards[0].page, 0);
    assert_eq!(renderer.cards[1].page, 1);
    assert!(renderer.last_metrics().dib_bytes <= DIB_BUDGET);
}
struct Fixture {
    root: PathBuf,
    instance: Option<crate::InstanceGuard>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-DesktopContent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let deployment = crate::system::DataPaths::isolated_test(root.clone())
            .unwrap()
            .prepare_for_current_user()
            .unwrap();
        deployment.read_back().unwrap();
        let instance = Some(crate::InstanceGuard::acquire(&root).unwrap());
        Self { root, instance }
    }
    fn bmp(&self) -> crate::materials::files::LockedFile {
        let files = crate::materials::files::ControlledFiles::new(&self.root).unwrap();
        let path = files
            .directory(&["content-quarantine".into()])
            .unwrap()
            .join("synthetic.bmp");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&78u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&54u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&4i32.to_le_bytes());
        bytes.extend_from_slice(&2i32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&24u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        for _ in 0..2 {
            bytes.extend_from_slice(&[0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255]);
        }
        let mut file = files.create_new(&path).unwrap();
        file.file_mut().write_all(&bytes).unwrap();
        file.seal().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.instance.take());
        let result = std::fs::remove_dir_all(&self.root);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
#[test]
fn actual_hidden_image_waits_for_verified_frame_pages_by_metadata_and_retains_physical_owner() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let fixture = Fixture::new();
    let source = fixture.bmp();
    let scene = scene(vec![card(20, true)]);
    let (_p, gate) = authority();
    let cache = Arc::new(RasterCache::new());
    let mut renderer = DesktopRenderer::new_hidden().unwrap();
    renderer.set_raster_cache(cache.clone());
    assert_eq!(
        renderer
            .present(&scene, scene.captured_at, &guard(&gate, scene.generation))
            .unwrap(),
        PresentOutcome::ContentPending
    );
    assert!(renderer.cards.is_empty());
    assert_eq!(renderer.last_metrics().input_regions, 0);
    let request = requests(&scene).unwrap().remove(0);
    let frame = cache
        .decode_verified(
            &source,
            &request.reference,
            request.reference.sha256,
            request.max_pixel_width,
            request.max_pixel_height,
            || true,
        )
        .unwrap();
    drop(frame);
    assert_eq!(
        renderer
            .present(&scene, scene.captured_at, &guard(&gate, scene.generation))
            .unwrap(),
        PresentOutcome::Offscreen
    );
    let rect = geometry(&scene.cards[0]).unwrap().image;
    let c = &renderer.cards[0];
    let index = ((rect.y + rect.height / 4.) as usize * c.surface.dib.width as usize
        + (rect.x + rect.width / 8.) as usize)
        * 4;
    assert_eq!(&c.surface.dib.pixels()[index..index + 4], &[0, 0, 255, 255]);
    assert!(renderer.probe_image_page(identity(21).slot, 1).is_err());
    renderer.probe_image_page(identity(20).slot, 1).unwrap();
    assert_eq!(
        renderer.take_events().unwrap(),
        vec![DesktopEvent::ImagePage {
            generation: scene.generation,
            slot: identity(20).slot,
            content_revision: Revision::new(2),
            view_revision: Revision::new(3),
            page: 1
        }]
    );
    assert!(
        matches!(&renderer.cards[0].source.content,CardContent::Image(image) if image.current_page==0),
        "input never commits a page"
    );
    // A queued click from the old view cannot survive a fresh scene/view gate.
    renderer.probe_image_page(identity(20).slot, 1).unwrap();
    let mut newer = scene.clone();
    newer.generation = SceneGeneration::new(11);
    newer.cards[0]
        .content_identity
        .as_mut()
        .unwrap()
        .view_revision = Revision::new(4);
    assert_eq!(
        renderer
            .present(&newer, newer.captured_at, &guard(&gate, newer.generation))
            .unwrap(),
        PresentOutcome::Offscreen
    );
    assert!(renderer.take_events().unwrap().is_empty());
    // Logical zero-opacity content retains geometry but never exposes a hit.
    newer.generation = SceneGeneration::new(12);
    newer.cards[0].component_alpha = 0.;
    let node = &mut newer.paint_tree.as_mut().unwrap().nodes[0];
    node.local_opacity = 0.;
    node.effective_opacity = 0.;
    assert_eq!(
        renderer
            .present(&newer, newer.captured_at, &guard(&gate, newer.generation))
            .unwrap(),
        PresentOutcome::Offscreen
    );
    assert!(renderer.probe_image_page(identity(20).slot, 1).is_err());
    cache.clear().unwrap();
    assert_eq!(cache.bytes_in_use(), 32);
    renderer.release_surfaces();
    assert_eq!(cache.bytes_in_use(), 0);
    assert_eq!(
        renderer
            .present(&scene, scene.captured_at, &guard(&gate, scene.generation))
            .unwrap(),
        PresentOutcome::ContentPending
    );
    assert!(renderer.probe_image_page(identity(20).slot, 1).is_err());
}
