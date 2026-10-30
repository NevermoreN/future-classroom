use super::*;
use fc_domain::{layout::*, profile::*, profile_content::*};
use std::{cell::RefCell, sync::Arc};

#[derive(Default)]
struct Cache {
    entries: Vec<(ContentSlotKey, Revision, [u8; 32], Arc<str>)>,
    queries: RefCell<Vec<ContentSlotKey>>,
}
impl ContentTextSource for Cache {
    fn text(&self, slot: ContentSlotKey, revision: Revision, hash: [u8; 32]) -> Option<Arc<str>> {
        self.queries.borrow_mut().push(slot);
        self.entries
            .iter()
            .find(|(s, r, h, _)| *s == slot && *r == revision && *h == hash)
            .map(|(_, _, _, text)| text.clone())
    }
}
fn nid(n: u64) -> LayoutNodeId {
    LayoutNodeId::new(n).unwrap()
}
fn text_snapshot(
    scope: LayoutScope,
    node: u64,
    text: &str,
    revision: u64,
    marker: u8,
) -> (
    PublishedContentSnapshot,
    (ContentSlotKey, Revision, [u8; 32], Arc<str>),
) {
    let slot = ContentSlotKey {
        scope,
        node_id: nid(node),
    };
    let rev = Revision::new(revision);
    (
        PublishedContentSnapshot {
            slot,
            revision: rev,
            view_revision: Revision::new(revision + 1),
            current_page: 0,
            content: PublishedContentDescriptor::Text {
                characters: text.chars().count() as u32,
                utf8_bytes: text.len() as u32,
                sha256: [marker; 32],
            },
        },
        (slot, rev, [marker; 32], Arc::from(text)),
    )
}
fn content_facts(kind: ContentPayloadKind) -> DesktopFacts {
    let mut f = fixture();
    let public = &mut f.profiles.public_layout;
    public.revision = Revision::new(1);
    public.node_id_high_water = 15;
    public.tree.nodes.retain(|n| [1, 10].contains(&n.id.get()));
    public.tree.nodes[0].children = vec![nid(10)];
    public.tree.nodes[0].kind = NodeKind::Root {
        flow: Flow::Column,
        padding: 0,
        gap: 0,
        anchor: Anchor::TopLeft,
        width: AxisSize::Fixed(500),
        height: AxisSize::Content {
            min: 0,
            preferred: 0,
            max: 2160,
        },
    };
    public.tree.nodes[1].children = vec![nid(14), nid(15)];
    public.tree.nodes[1].kind = NodeKind::Outlet {
        width: AxisSize::Percent {
            basis_points: 10000,
            min: 160,
            max: 800,
        },
        height: AxisSize::Content {
            min: 0,
            preferred: 0,
            max: 2160,
        },
    };
    for n in [14, 15] {
        public.tree.nodes.push(LayoutNode {
            id: nid(n),
            kind: NodeKind::Widget(match kind {
                ContentPayloadKind::Text => WidgetKind::SubjectText,
                ContentPayloadKind::Images => WidgetKind::SubjectImage,
            }),
            children: vec![],
        });
        public
            .content_styles
            .push(ContentNodeStyle::new(nid(n), kind));
    }
    f
}
fn text_facts() -> (DesktopFacts, Cache) {
    let mut f = content_facts(ContentPayloadKind::Text);
    let mut cache = Cache::default();
    for (n, text, rev, hash) in [
        (14, "第一块独立正文", 10, 1),
        (15, "第二块\n更长的独立正文😀\n保留每一行", 20, 2),
    ] {
        let (snapshot, entry) = text_snapshot(LayoutScope::Public, n, text, rev, hash);
        f.profile_contents.push(snapshot);
        cache.entries.push(entry);
    }
    (f, cache)
}
fn render(f: &DesktopFacts, cache: &dyn ContentTextSource) -> Result<SceneSnapshot, SceneError> {
    build_scene_with_content(
        f,
        SceneGeneration::new(8),
        Rect {
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1040.,
        },
        &Measure,
        cache,
    )
}
#[test]
fn two_same_kind_nodes_hydrate_distinct_texts_styles_and_exact_identity() {
    let (mut f, cache) = text_facts();
    f.profiles.public_layout.content_styles[1]
        .component
        .font_dip = 30;
    let s = render(&f, &cache).unwrap();
    let cards: Vec<_> = s
        .cards
        .iter()
        .filter(|c| c.kind == ComponentKind::SubjectText)
        .collect();
    assert_eq!(cards.len(), 2);
    assert_ne!(cards[0].rect.height, cards[1].rect.height);
    for (i, c) in cards.iter().enumerate() {
        assert!(
            matches!(&c.content,CardContent::StandaloneText(text) if text.as_ref()==cache.entries[i].3.as_ref())
        );
        let identity = c.content_identity.unwrap();
        assert_eq!(
            (identity.slot, identity.revision, identity.view_revision),
            (
                f.profile_contents[i].slot,
                f.profile_contents[i].revision,
                f.profile_contents[i].view_revision
            )
        );
    }
    assert_eq!((cards[0].font_dip, cards[1].font_dip), (22., 30.));
    assert!(matches!(
        build_scene(
            &f,
            SceneGeneration::new(8),
            Rect {
                x: 0.,
                y: 0.,
                width: 1920.,
                height: 1040.
            },
            &Measure
        ),
        Err(SceneError::ContentPending(_))
    ));
}
#[test]
fn wrong_cache_revision_hash_scope_or_body_never_publishes_placeholder() {
    let (f, mut cache) = text_facts();
    let slot = f.profile_contents[0].slot;
    let original = cache.entries[0].clone();
    cache.entries[0].1 = Revision::new(99);
    assert_eq!(render(&f, &cache), Err(SceneError::ContentPending(slot)));
    cache.entries[0] = original.clone();
    cache.entries[0].2 = [9; 32];
    assert_eq!(render(&f, &cache), Err(SceneError::ContentPending(slot)));
    cache.entries[0] = original.clone();
    cache.entries[0].0.node_id = nid(99);
    assert_eq!(render(&f, &cache), Err(SceneError::ContentPending(slot)));
    cache.entries[0] = original;
    cache.entries[0].3 = Arc::from("not descriptor content");
    assert_eq!(render(&f, &cache), Err(SceneError::InvalidContent(slot)));
    let mut wrong = f.clone();
    wrong.profile_contents[0].slot.node_id = nid(99);
    assert!(matches!(
        render(&wrong, &cache),
        Err(SceneError::InvalidFacts(_))
    ));
}
#[test]
fn empty_slots_remove_geometry_but_zero_opacity_preserves_real_body() {
    let (mut f, cache) = text_facts();
    f.profile_contents[0].content = PublishedContentDescriptor::Empty;
    f.profiles.public_layout.content_styles[1]
        .component
        .opacity_percent = 0;
    let s = render(&f, &cache).unwrap();
    assert_eq!(s.cards.len(), 1);
    assert_eq!(s.cards[0].content_identity.unwrap().slot.node_id, nid(15));
    assert!(s.cards[0].rect.height > 84.);
    let n = s
        .paint_tree
        .unwrap()
        .nodes
        .into_iter()
        .find(|n| n.card_index.is_some())
        .unwrap();
    assert_eq!(n.effective_opacity, 0.);
    assert_eq!(
        cache.queries.borrow().as_slice(),
        [f.profile_contents[1].slot]
    );
}
fn image_snapshot(
    node: u64,
    width: u32,
    height: u32,
    page: u8,
    pages: u8,
) -> PublishedContentSnapshot {
    let slot = ContentSlotKey {
        scope: LayoutScope::Public,
        node_id: nid(node),
    };
    PublishedContentSnapshot {
        slot,
        revision: Revision::new(node),
        view_revision: Revision::new(node + 1),
        current_page: page,
        content: PublishedContentDescriptor::Image {
            current: MediaReference {
                version_id: MediaVersionId::new(node).unwrap(),
                owner: slot,
                sha256: [node as u8; 32],
                bytes: 1024,
                width,
                height,
                format: ImageFormat::Png,
            },
            total_pages: pages,
        },
    }
}
#[test]
fn independent_images_preserve_distinct_pages_aspect_and_shared_navigation_geometry() {
    let mut f = content_facts(ContentPayloadKind::Images);
    f.profile_contents = vec![
        image_snapshot(14, 1000, 500, 0, 1),
        image_snapshot(15, 300, 600, 49, 50),
    ];
    let s = render(&f, &Cache::default()).unwrap();
    assert!(matches!(
        build_scene(
            &f,
            SceneGeneration::new(8),
            Rect {
                x: 0.,
                y: 0.,
                width: 1920.,
                height: 1040.
            },
            &Measure
        ),
        Err(SceneError::ContentPending(_))
    ));
    assert_eq!(s.cards.len(), 2);
    assert_ne!(s.cards[0].rect.height, s.cards[1].rect.height);
    assert_eq!((s.cards[0].page.offset, s.cards[0].page.total), (0, 1));
    assert_eq!((s.cards[1].page.offset, s.cards[1].page.total), (49, 50));
    for (i, c) in s.cards.iter().enumerate() {
        let CardContent::Image(image) = &c.content else {
            panic!("image missing")
        };
        assert_eq!(image.current.owner, c.content_identity.unwrap().slot);
        let body = card_body_spec(c.kind, &c.content, false, None).unwrap();
        let g = body.image_geometry(c.rect.width, c.rect.height).unwrap();
        assert!(
            (g.image.width / g.image.height
                - image.current.width as f32 / image.current.height as f32)
                .abs()
                < 0.001
        );
        assert!(g.image.x >= g.viewport.x && g.image.y >= g.viewport.y);
        assert!(
            g.image.x + g.image.width <= g.viewport.x + g.viewport.width + 0.01
                && g.image.y + g.image.height <= g.viewport.y + g.viewport.height + 0.01
        );
        if i == 0 {
            assert!(g.pager.is_none());
        } else {
            let nav = g.pager.unwrap();
            assert_eq!((nav.previous.height, nav.next.height), (44., 44.));
            assert!(g.viewport.y + g.viewport.height + 8. <= nav.rect.y + 0.01);
        }
    }
    f.profile_contents[1].current_page = 50;
    assert!(matches!(
        render(&f, &Cache::default()),
        Err(SceneError::InvalidFacts(_))
    ));
    f.profile_contents[1].current_page = 0;
    if let PublishedContentDescriptor::Image { total_pages, .. } =
        &mut f.profile_contents[1].content
    {
        *total_pages = 51;
    }
    assert!(matches!(
        render(&f, &Cache::default()),
        Err(SceneError::InvalidFacts(_))
    ));
}
#[test]
fn selected_subject_never_hydrates_public_fallback_and_break_never_borrows_subject() {
    let (mut f, mut cache) = text_facts();
    let public = f.profile_contents.clone();
    let current = f.lesson_context.current.as_ref().unwrap();
    let key = SubjectProfileKey {
        class_id: ClassId::new(1).unwrap(),
        subject_id: current.effective.subject_id,
        teacher_id: current.teacher_id.unwrap(),
    };
    let mut profile = SubjectProfileDocument::new(ProfileId::new(1).unwrap(), key);
    profile.revision = Revision::new(1);
    profile.layout_revision = Revision::new(1);
    profile.node_id_high_water = 3;
    profile.tree.nodes[0].children = vec![nid(2), nid(3)];
    profile.tree.nodes[1].kind = NodeKind::Widget(WidgetKind::SubjectText);
    profile.tree.nodes.push(LayoutNode {
        id: nid(3),
        kind: NodeKind::Widget(WidgetKind::SubjectText),
        children: vec![],
    });
    profile.content_styles = vec![
        ContentNodeStyle::new(nid(2), ContentPayloadKind::Text),
        ContentNodeStyle::new(nid(3), ContentPayloadKind::Text),
    ];
    f.profiles.subject_profile = Some(profile);
    f.profiles.reason = ProfileSelectionReason::Selected;
    f.profile_contents.clear();
    for n in [2, 3] {
        let (snapshot, entry) = text_snapshot(
            LayoutScope::Subject(key),
            n,
            "当前老师的实际文字",
            30 + n,
            n as u8,
        );
        f.profile_contents.push(snapshot);
        cache.entries.push(entry);
    }
    let s = render(&f, &cache).unwrap();
    assert!(s
        .cards
        .iter()
        .all(|c| c.content_identity.unwrap().slot.scope == LayoutScope::Subject(key)));
    assert!(cache
        .queries
        .borrow()
        .iter()
        .all(|s| s.scope == LayoutScope::Subject(key)));
    f.captured_at = now(31260);
    f.lesson_context = f
        .lesson_context
        .observe(&f.identity, &f.schedule, f.captured_at)
        .unwrap();
    f.materials_source = f
        .lesson_context
        .material_source(
            &f.identity,
            &f.schedule,
            ClassId::new(1).unwrap(),
            f.captured_at,
        )
        .unwrap();
    assert!(matches!(
        render(&f, &cache),
        Err(SceneError::InvalidFacts(_))
    ));
    f.profiles.subject_profile = None;
    f.profiles.reason = ProfileSelectionReason::Public;
    f.profile_contents = public;
    let break_scene = render(&f, &cache).unwrap();
    assert!(break_scene
        .cards
        .iter()
        .filter_map(|c| c.content_identity)
        .all(|i| i.slot.scope == LayoutScope::Public));
}

#[test]
fn content_scalar_byte_pixel_and_card_space_bounds_are_not_weakened() {
    let (mut f, mut cache) = text_facts();
    let legal = "😀".repeat(10_000);
    let (snapshot, entry) = text_snapshot(LayoutScope::Public, 14, &legal, 10, 1);
    f.profile_contents[0] = snapshot;
    cache.entries[0] = entry;
    let s = render(&f, &cache).unwrap();
    assert!(
        matches!(&s.cards[0].content,CardContent::StandaloneText(text) if text.len()==40_000 && text.chars().count()==10_000)
    );
    let mut oversized = f.clone();
    if let PublishedContentDescriptor::Text {
        characters,
        utf8_bytes,
        ..
    } = &mut oversized.profile_contents[0].content
    {
        *characters = 10_001;
        *utf8_bytes = 40_004;
    }
    assert!(matches!(
        render(&oversized, &cache),
        Err(SceneError::InvalidFacts(_))
    ));
    let mut images = content_facts(ContentPayloadKind::Images);
    images.profile_contents = vec![
        image_snapshot(14, 4000, 2000, 0, 50),
        image_snapshot(15, 1, 1, 0, 1),
    ];
    render(&images, &Cache::default()).unwrap();
    if let PublishedContentDescriptor::Image { current, .. } =
        &mut images.profile_contents[0].content
    {
        current.height = 2001;
    }
    assert!(matches!(
        render(&images, &Cache::default()),
        Err(SceneError::InvalidFacts(_))
    ));
    images.profile_contents[0] = image_snapshot(14, 4000, 2000, 0, 50);
    if let PublishedContentDescriptor::Image { current, .. } =
        &mut images.profile_contents[0].content
    {
        current.bytes = 10 * 1024 * 1024 + 1;
    }
    assert!(matches!(
        render(&images, &Cache::default()),
        Err(SceneError::InvalidFacts(_))
    ));
    images.profile_contents[0] = image_snapshot(14, 4000, 2000, 0, 50);
    images.profiles.public_layout.content_styles[0]
        .component
        .max_height = 84;
    assert!(matches!(
        render(&images, &Cache::default()),
        Err(SceneError::TreeLayout(TreeLayoutError::OutOfSpace))
    ));
}
