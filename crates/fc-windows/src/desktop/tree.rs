//! True premultiplied subtree groups. Only root groups own HWNDs. Leaf DIBs
//! cache raw primitives; group opacity is applied after their combined subtree.
//! All application RGBA storage, including simultaneous scratch capacities, is
//! charged to the same 16 MiB admission budget before any allocation.
use super::*;
use fc_presentation::scene::{SceneNodeKey, ScenePaintTree};

pub struct DesktopPreview {
    pub width: u16,
    pub height: u16,
    pub bgra: Vec<u8>,
    pub source_work: Rect,
    pub scale_x: f32,
    pub scale_y: f32,
}
pub(super) struct TreeRenderer {
    tree: ScenePaintTree,
    original_cards: Vec<SceneCard>,
    tone: TextTone,
    unverified: bool,
    pub(super) card_count: usize,
    groups: Vec<Group>,
    backgrounds: Vec<(usize, Surface)>,
    special: Vec<usize>,
    base_bytes: u64,
    scratch_peak: u64,
}
struct Group {
    node: usize,
    input: InputStrip,
    dirty: Option<RECT>,
}
impl TreeRenderer {
    pub(super) fn hide(&self) {
        for g in &self.groups {
            g.input.hide();
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Area {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
impl Area {
    fn rect(r: Rect) -> Self {
        Self {
            left: r.x.round() as i32,
            top: r.y.round() as i32,
            right: (r.x.round() as i32).saturating_add(r.width.ceil() as i32),
            bottom: (r.y.round() as i32).saturating_add(r.height.ceil() as i32),
        }
    }
    fn clip(r: Rect) -> Self {
        Self {
            left: r.x.ceil() as i32,
            top: r.y.ceil() as i32,
            right: (r.x + r.width).floor() as i32,
            bottom: (r.y + r.height).floor() as i32,
        }
    }
    fn intersection(self, b: Self) -> Self {
        Self {
            left: self.left.max(b.left),
            top: self.top.max(b.top),
            right: self.right.min(b.right),
            bottom: self.bottom.min(b.bottom),
        }
    }
    fn empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
    fn width(self) -> usize {
        (i64::from(self.right) - i64::from(self.left)).max(0) as usize
    }
    fn height(self) -> usize {
        (i64::from(self.bottom) - i64::from(self.top)).max(0) as usize
    }
    fn bytes(self) -> Result<u64> {
        (self.width() as u64)
            .checked_mul(self.height() as u64)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| Error::from_hresult(E_OUTOFMEMORY))
    }
    fn logical(self) -> Rect {
        Rect {
            x: self.left as f32,
            y: self.top as f32,
            width: self.width() as f32,
            height: self.height() as f32,
        }
    }
    fn union(self, b: Self) -> Self {
        if self.empty() {
            return b;
        }
        if b.empty() {
            return self;
        }
        Self {
            left: self.left.min(b.left),
            top: self.top.min(b.top),
            right: self.right.max(b.right),
            bottom: self.bottom.max(b.bottom),
        }
    }
}
fn oom() -> Error {
    Error::from_hresult(E_OUTOFMEMORY)
}
fn invalid() -> Error {
    Error::from_hresult(E_INVALIDARG)
}
struct Budget {
    base: u64,
    live: u64,
    peak: u64,
}
impl Budget {
    fn reserve(&mut self, n: u64) -> Result<()> {
        let live = self.live.checked_add(n).ok_or_else(oom)?;
        if self.base.checked_add(live).is_none_or(|v| v > DIB_BUDGET) {
            return Err(oom());
        }
        self.live = live;
        self.peak = self.peak.max(live);
        Ok(())
    }
}
fn charge(total: &mut u64, r: Rect) -> Result<()> {
    *total = total
        .checked_add(surface_bytes(r)?)
        .filter(|v| *v <= DIB_BUDGET)
        .ok_or_else(oom)?;
    Ok(())
}
fn headless(rect: Rect) -> Result<Surface> {
    let mut dib = Dib::new(rect.width.ceil() as i32, rect.height.ceil() as i32)?;
    dib.pixels_mut().fill(0);
    Ok(Surface {
        window: NativeWindow(HWND::default()),
        dib,
        rect,
        dirty: true,
    })
}
fn state() -> Box<InputState> {
    Box::new(InputState {
        context: RefCell::new(None),
        targets: RefCell::new(Vec::new()),
        layout_epoch: Cell::new(0),
        pressed: Cell::new(None),
        turns: RefCell::new(VecDeque::with_capacity(2)),
    })
}
fn headless_input(rect: Rect) -> Result<InputStrip> {
    Ok(InputStrip {
        surface: headless(rect)?,
        state: state(),
    })
}
fn render_reserve(card: &Card) -> Result<u64> {
    std::iter::once(&card.surface)
        .chain(card.dynamic.iter())
        .chain(card.input.iter().map(|i| &i.surface))
        .map(|s| surface_bytes(s.rect))
        .try_fold(0, |a, b| Ok(a.max(b?)))
}
fn make_card(
    renderer: &DesktopRenderer,
    card: &SceneCard,
    scene: &SceneSnapshot,
    headless_leaf: bool,
    total: &mut u64,
) -> Result<Card> {
    validate_card(card)?;
    let note = if card.kind == ComponentKind::Materials {
        scene
            .paint_tree
            .as_ref()
            .and_then(|t| fc_presentation::scene::profile_configuration_warning(t.selection_reason))
    } else {
        None
    };
    let body = prepare_content_with_note(
        &renderer.graphics,
        card,
        scene.public_calendar_unverified,
        note,
    )?;
    let dynamic_rect = match card.content {
        CardContent::Clock(_) => Some(Rect {
            x: card.rect.x + 16.,
            y: card.rect.y + 42.,
            width: card.rect.width - 32.,
            height: card.rect.height - 54.,
        }),
        CardContent::Recess { .. } => {
            let width = (card.rect.width - 32.).min(560.);
            Some(Rect {
                x: card.rect.x + (card.rect.width - width) / 2.,
                y: card.rect.y + (card.rect.height - 100.) / 2.,
                width,
                height: 100.,
            })
        }
        _ => None,
    };
    let input_rect = if matches!(
        card.content,
        CardContent::LessonControl {
            can_change: true,
            ..
        }
    ) {
        Some(Rect {
            x: card.rect.x + 8.,
            y: card.rect.y + 8.,
            width: 144.,
            height: 40.,
        })
    } else if read_only_classroom(card) {
        None
    } else if matches!(&card.content,CardContent::Image(image) if image.total_pages>1)
        || matches!(&card.content,CardContent::Materials(list) if !list.rows.is_empty()||list.next_cursor.is_some()||body.pages.len()>1)
    {
        Some(card.rect)
    } else if matches!(card.content, CardContent::Materials(_)) {
        None
    } else if body.pages.len() > 1
        || card.kind == ComponentKind::Homework
            && (card.page.offset > 0
                || card.page.offset.saturating_add(card.page.shown) < card.page.total)
    {
        Some(Rect {
            x: card.rect.x + card.rect.width - 86.,
            y: card.rect.y + 8.,
            width: 72.,
            height: 24.,
        })
    } else {
        None
    };
    let static_rect = if matches!(card.content, CardContent::Recess { .. }) {
        Rect {
            x: card.rect.x,
            y: card.rect.y,
            width: card.rect.width.min(560.),
            height: 48.,
        }
    } else {
        card.rect
    };
    for r in std::iter::once(static_rect)
        .chain(dynamic_rect)
        .chain(input_rect)
    {
        charge(total, r)?;
    }
    let backdrop =
        if matches!(card.content, CardContent::Recess { .. }) && card.component_alpha > 0. {
            let excluded: Vec<_> = std::iter::once(static_rect).chain(dynamic_rect).collect();
            Some(Backdrop::new_partitioned(
                &renderer.class,
                card.rect,
                card.surface_alpha,
                card.edge_alpha,
                card.component_alpha,
                &excluded,
            )?)
        } else {
            None
        };
    let surface = if headless_leaf {
        headless(static_rect)?
    } else {
        renderer.surface(static_rect)?
    };
    let dynamic = dynamic_rect
        .map(|r| {
            if headless_leaf {
                headless(r)
            } else {
                renderer.surface(r)
            }
        })
        .transpose()?;
    let input = input_rect
        .map(|r| {
            if headless_leaf {
                headless_input(r)
            } else {
                renderer.input_strip(r)
            }
        })
        .transpose()?;
    let mut source = card.clone();
    // Cached primitives are raw. Every component/ancestor is composed by its
    // tree group, except the explicitly partitioned Recess root handled below.
    if headless_leaf || backdrop.is_some() {
        source.component_alpha = 1.;
    }
    Ok(Card {
        image: content::frame(&renderer.raster, card)?,
        source,
        tone: scene.text_tone,
        calendar_unverified: scene.public_calendar_unverified,
        render_pending: true,
        input,
        body,
        page: 0,
        surface,
        backdrop,
        dynamic,
        clock_tick: None,
    })
}
fn parent_index(tree: &ScenePaintTree, index: usize) -> Result<Option<usize>> {
    tree.nodes[index]
        .parent
        .map(|key| {
            tree.nodes[..index]
                .iter()
                .position(|n| n.key == key)
                .ok_or_else(invalid)
        })
        .transpose()
}
fn descendant(tree: &ScenePaintTree, index: usize, ancestor: usize) -> bool {
    let mut p = Some(index);
    while let Some(i) = p {
        if i == ancestor {
            return true;
        }
        p = parent_index(tree, i).ok().flatten();
    }
    false
}
fn drawable(tree: &ScenePaintTree, index: usize) -> bool {
    tree.nodes
        .iter()
        .enumerate()
        .any(|(i, n)| descendant(tree, i, index) && (n.glass.is_some() || n.card_index.is_some()))
}
fn validate(tree: &ScenePaintTree, cards: &[SceneCard]) -> Result<Vec<usize>> {
    if tree.nodes.len() > 128 || cards.len() > 64 {
        return Err(invalid());
    }
    let mut seen = Vec::new();
    let mut card_seen = vec![false; cards.len()];
    let mut roots = Vec::new();
    let mut top_glass = 0;
    for (i, n) in tree.nodes.iter().enumerate() {
        if seen.contains(&n.key)
            || ![n.local_opacity, n.effective_opacity]
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
        {
            return Err(invalid());
        }
        seen.push(n.key);
        let p = parent_index(tree, i)?;
        let expected = p.map_or(1., |p| tree.nodes[p].effective_opacity) * n.local_opacity;
        if (expected - n.effective_opacity).abs() > 0.0001 {
            return Err(invalid());
        }
        let a = Area::rect(n.rect);
        let clip = Area::clip(n.clip);
        if ![
            n.rect.x,
            n.rect.y,
            n.rect.width,
            n.rect.height,
            n.clip.x,
            n.clip.y,
            n.clip.width,
            n.clip.height,
        ]
        .iter()
        .all(|v| v.is_finite())
            || n.rect.width < 0.
            || n.rect.height < 0.
            || n.clip.width < 0.
            || n.clip.height < 0.
            || ((a.empty() || clip.empty()) && drawable(tree, i))
            || n.rect.width > 8192.
            || n.rect.height > 8192.
            || (!clip.empty() && a.intersection(clip) != clip)
        {
            return Err(invalid());
        }
        if let Some(p) = p {
            if !clip.empty() && Area::clip(tree.nodes[p].clip).intersection(clip) != clip {
                return Err(invalid());
            }
        }
        if let Some(card) = n.card_index {
            if card >= cards.len()
                || card_seen[card]
                || cards[card].rect != n.rect
                || (cards[card].component_alpha - n.local_opacity).abs() > 0.0001
            {
                return Err(invalid());
            }
            let m = n.glass.ok_or_else(invalid)?;
            if (m.surface_alpha - cards[card].surface_alpha).abs() > 0.0001
                || (m.edge_alpha - cards[card].edge_alpha).abs() > 0.0001
                || n.widget_kind.and_then(|k| k.component_kind()) != Some(cards[card].kind)
            {
                return Err(invalid());
            }
            if let Some(identity) = cards[card].content_identity {
                if identity.slot.scope != n.key.scope || identity.slot.node_id != n.key.id {
                    return Err(invalid());
                }
            }
            card_seen[card] = true;
        } else if n.widget_kind.is_some() {
            return Err(invalid());
        }
        let mut ancestor = p;
        let mut glass_depth = usize::from(n.glass.is_some());
        let mut graph_depth = 1;
        while let Some(a) = ancestor {
            graph_depth += 1;
            glass_depth += usize::from(tree.nodes[a].glass.is_some());
            ancestor = parent_index(tree, a)?;
        }
        let implicit = usize::from(n.card_index.is_some_and(|i| {
            matches!(
                cards[i].content,
                CardContent::Courses(_) | CardContent::Homework(_)
            )
        }));
        if graph_depth > 16 || glass_depth + implicit > 2 {
            return Err(invalid());
        }
        if let Some(m) = n.glass {
            if ![m.surface_alpha, m.edge_alpha]
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            {
                return Err(invalid());
            }
            if glass_depth == 1 {
                top_glass += 1;
            }
        }
        if n.card_index
            .is_some_and(|c| matches!(cards[c].content, CardContent::Recess { .. }))
        {
            continue;
        }
        if (n.glass.is_some() || n.local_opacity < 1.)
            && drawable(tree, i)
            && !roots.iter().any(|root| descendant(tree, i, *root))
        {
            roots.push(i);
        }
    }
    if card_seen.contains(&false) || top_glass > 8 {
        return Err(invalid());
    }
    if cards
        .iter()
        .map(|c| c.visible_leaf_limit as usize)
        .sum::<usize>()
        > 64
    {
        return Err(invalid());
    }
    Ok(roots)
}
fn group_bounds(tree: &ScenePaintTree, root: usize) -> Result<Rect> {
    let mut area = Area {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    for (i, n) in tree.nodes.iter().enumerate() {
        if descendant(tree, i, root) && (n.glass.is_some() || n.card_index.is_some()) {
            area = area.union(Area::rect(n.rect).intersection(Area::clip(n.clip)));
        }
    }
    if area.empty() {
        return Err(invalid());
    }
    Ok(area.logical())
}
fn background(g: &Graphics, node: &fc_presentation::scene::ScenePaintNode) -> Result<Surface> {
    let s = headless(node.rect)?;
    let target = g.target(&s.dib)?;
    let m = node.glass.ok_or_else(invalid)?;
    // SAFETY: this own headless DIB and local target remain on this UI thread;
    // only raw container fill/edge are cached, no ancestor opacity is baked in.
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F::default()));
        let fill = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.,
                g: 1.,
                b: 1.,
                a: m.surface_alpha,
            },
            None,
        )?;
        let edge = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.,
                g: 1.,
                b: 1.,
                a: m.edge_alpha,
            },
            None,
        )?;
        let shape = bounds(1., 1., node.rect.width - 1., node.rect.height - 1., 20.);
        target.FillRoundedRectangle(&shape, &fill);
        target.DrawRoundedRectangle(&shape, &edge, 0.75, None);
        target.EndDraw(None, None)?;
    }
    Ok(s)
}
fn blend(dst: &mut [u8], area: Area, src: &[u8], source: Area, clip: Area, opacity: f32) {
    let part = area.intersection(source).intersection(clip);
    if part.empty() {
        return;
    }
    let opacity = (opacity * 255.).round() as u16;
    for y in part.top..part.bottom {
        for x in part.left..part.right {
            let si = ((y - source.top) as usize * source.width() + (x - source.left) as usize) * 4;
            let di = ((y - area.top) as usize * area.width() + (x - area.left) as usize) * 4;
            let alpha = (u16::from(src[si + 3]) * opacity + 127) / 255;
            for k in 0..4 {
                let value = (u16::from(src[si + k]) * opacity + 127) / 255;
                dst[di + k] =
                    (value + (u16::from(dst[di + k]) * (255 - alpha) + 127) / 255).min(255) as u8;
            }
        }
    }
}
fn contents(
    t: &TreeRenderer,
    index: usize,
    cards: &[Card],
    dst: &mut [u8],
    area: Area,
    budget: &mut Budget,
) -> Result<()> {
    let node = &t.tree.nodes[index];
    let clip = area.intersection(Area::clip(node.clip));
    if clip.empty() {
        return Ok(());
    }
    if let Some((_, bg)) = t.backgrounds.iter().find(|(i, _)| *i == index) {
        blend(dst, area, bg.dib.pixels(), Area::rect(bg.rect), clip, 1.);
    }
    if let Some(card) = node.card_index {
        let c = &cards[card];
        for s in std::iter::once(&c.surface)
            .chain(c.dynamic.iter())
            .chain(c.input.iter().map(|i| &i.surface))
        {
            blend(dst, area, s.dib.pixels(), Area::rect(s.rect), clip, 1.);
        }
    }
    for (child, n) in t.tree.nodes.iter().enumerate() {
        if n.parent == Some(node.key) {
            paint(t, child, cards, dst, area, budget)?;
        }
    }
    Ok(())
}
fn paint(
    t: &TreeRenderer,
    index: usize,
    cards: &[Card],
    dst: &mut [u8],
    area: Area,
    budget: &mut Budget,
) -> Result<()> {
    let n = &t.tree.nodes[index];
    if n.local_opacity == 0. || area.intersection(Area::clip(n.clip)).empty() {
        return Ok(());
    }
    if n.local_opacity < 1. {
        let clipped = area.intersection(Area::clip(n.clip));
        let bytes = clipped.bytes()?;
        budget.reserve(bytes)?;
        let mut group = vec![0; bytes as usize];
        // Vec<u8> has exact initialized length; charge any allocator-selected
        // capacity before allowing this owner to coexist with prior scratch.
        if group.capacity() > group.len() {
            budget.reserve((group.capacity() - group.len()) as u64)?;
        }
        let charged = group.capacity() as u64;
        contents(t, index, cards, &mut group, clipped, budget)?;
        blend(dst, area, &group, clipped, clipped, n.local_opacity);
        drop(group);
        budget.live -= charged;
    } else {
        contents(t, index, cards, dst, area, budget)?;
    }
    Ok(())
}
fn compose_group(t: &mut TreeRenderer, group: usize, cards: &[Card], patch: Area) -> Result<()> {
    let root = t.groups[group].node;
    let rect = Area::rect(t.groups[group].input.surface.rect);
    let patch = patch.intersection(rect);
    if patch.empty() {
        return Ok(());
    }
    let bytes = patch.bytes()?;
    let mut budget = Budget {
        base: t.base_bytes,
        live: 0,
        peak: 0,
    };
    budget.reserve(bytes)?;
    let mut pixels = vec![0; bytes as usize];
    if pixels.capacity() > pixels.len() {
        budget.reserve((pixels.capacity() - pixels.len()) as u64)?;
    }
    paint(t, root, cards, &mut pixels, patch, &mut budget)?;
    let dst = t.groups[group].input.surface.dib.pixels_mut();
    for y in patch.top..patch.bottom {
        let from = (y - patch.top) as usize * patch.width() * 4;
        let to = ((y - rect.top) as usize * rect.width() + (patch.left - rect.left) as usize) * 4;
        dst[to..to + patch.width() * 4].copy_from_slice(&pixels[from..from + patch.width() * 4]);
    }
    t.scratch_peak = t.scratch_peak.max(budget.peak);
    let relative = RECT {
        left: patch.left - rect.left,
        top: patch.top - rect.top,
        right: patch.right - rect.left,
        bottom: patch.bottom - rect.top,
    };
    t.groups[group].dirty = Some(match t.groups[group].dirty {
        None => relative,
        Some(r) => RECT {
            left: r.left.min(relative.left),
            top: r.top.min(relative.top),
            right: r.right.max(relative.right),
            bottom: r.bottom.max(relative.bottom),
        },
    });
    Ok(())
}
fn aggregate_inputs(t: &mut TreeRenderer, cards: &[Card]) {
    for group in &mut t.groups {
        let mut targets = Vec::new();
        let origin = group.input.surface.rect;
        for (i, node) in t.tree.nodes.iter().enumerate() {
            let Some(card_index) = node.card_index else {
                continue;
            };
            if !descendant(&t.tree, i, group.node) || node.effective_opacity == 0. {
                continue;
            }
            let Some(input) = &cards[card_index].input else {
                continue;
            };
            for hit in input.state.targets.borrow().iter() {
                let rect = Rect {
                    x: input.surface.rect.x + hit.rect.x,
                    y: input.surface.rect.y + hit.rect.y,
                    ..hit.rect
                };
                let rect = Area::rect(rect)
                    .intersection(Area::clip(node.clip))
                    .intersection(Area::rect(origin));
                if rect.empty() {
                    continue;
                }
                let action = match hit.action {
                    InputAction::Page(forward) => InputAction::TreePage {
                        key: node.key,
                        card_index,
                        forward,
                    },
                    _ => hit.action.clone(),
                };
                targets.push(HitTarget {
                    rect: Rect {
                        x: (rect.left - origin.x.round() as i32) as f32,
                        y: (rect.top - origin.y.round() as i32) as f32,
                        width: rect.width() as f32,
                        height: rect.height() as f32,
                    },
                    action,
                });
            }
        }
        if *group.input.state.targets.borrow() != targets {
            group.input.state.pressed.set(None);
            group
                .input
                .state
                .layout_epoch
                .set(group.input.state.layout_epoch.get().saturating_add(1));
            group.input.state.targets.replace(targets);
        }
    }
}
fn context(scene: &SceneSnapshot, g: &PublicationGuard<'_>, probe: bool) -> InputContext {
    InputContext {
        generation: scene.generation,
        source_revision: scene.source_revision,
        captured_at: scene.captured_at,
        valid_until: scene.valid_until,
        gate: g.gate.clone(),
        lease: g.lease,
        probe,
    }
}
fn upload_patch(group: &mut Group, force: bool) -> Result<bool> {
    let Some(dirty) = group.dirty.or_else(|| {
        force.then_some(RECT {
            left: 0,
            top: 0,
            right: group.input.surface.dib.width,
            bottom: group.input.surface.dib.height,
        })
    }) else {
        return Ok(false);
    };
    let s = &group.input.surface;
    let point = POINT {
        x: s.rect.x.round() as i32,
        y: s.rect.y.round() as i32,
    };
    let size = SIZE {
        cx: s.dib.width,
        cy: s.dib.height,
    };
    let src = POINT::default();
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let info = UPDATELAYEREDWINDOWINFO {
        cbSize: std::mem::size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
        hdcSrc: s.dib.dc,
        pptDst: &point,
        psize: &size,
        pptSrc: &src,
        pblend: &blend,
        dwFlags: ULW_ALPHA,
        prcDirty: if force { std::ptr::null() } else { &dirty },
        ..Default::default()
    };
    // SAFETY: Vista+ API; every pointer and own selected DIB survives this
    // synchronous call. A fresh publication gate precedes it. Dirty is in this
    // root HWND's client coordinates; it never targets any external window.
    if !unsafe { UpdateLayeredWindowIndirect(s.window.0, &info) }.as_bool() {
        return Err(Error::from_thread());
    }
    group.dirty = None;
    Ok(true)
}

pub(super) fn present(
    renderer: &mut DesktopRenderer,
    scene: &SceneSnapshot,
    now: LocalDateTime,
    guard: &PublicationGuard<'_>,
) -> Result<PresentOutcome> {
    let source = scene.paint_tree.as_ref().ok_or_else(invalid)?;
    let changed = renderer.last_scene != Some(scene.generation)
        || renderer.last_revision != Some(scene.source_revision)
        || renderer.tree.as_ref().is_none_or(|t| {
            t.tree != *source
                || t.original_cards != scene.cards
                || t.tone != scene.text_tone
                || t.unverified != scene.public_calendar_unverified
        });
    if changed {
        renderer.hide();
        renderer.tree = None;
        renderer.cards.clear();
        renderer.last_scene = None;
        let roots = validate(source, &scene.cards)?;
        let mut total = 0;
        let mut target_reserve = 0;
        let mut special = Vec::new();
        for (i, card) in scene.cards.iter().enumerate() {
            let recess = matches!(card.content, CardContent::Recess { .. });
            if recess {
                special.push(i);
            }
            let mut c = make_card(renderer, card, scene, !recess, &mut total)?;
            target_reserve = target_reserve.max(render_reserve(&c)?);
            if total + target_reserve > DIB_BUDGET {
                return Err(oom());
            }
            render_card(&renderer.graphics, &mut c)?;
            if recess {
                scale_dib(&mut c.surface.dib, card.component_alpha);
            }
            renderer.metrics.drawn_frames += 1;
            renderer.metrics.text_layouts += c.body.blocks.len() as u64;
            renderer.cards.push(c);
        }
        if let Some(control) = &scene.lesson_control {
            special.push(renderer.cards.len());
            let card = SceneCard {
                content_identity: None,
                kind: ComponentKind::Schedule,
                rect: control.rect,
                font_dip: 16.,
                component_alpha: 1.,
                surface_alpha: 0.03,
                edge_alpha: 0.5,
                content: CardContent::LessonControl {
                    caption: control.caption.clone(),
                    can_change: control.can_change,
                },
                page: Default::default(),
                visible_leaf_limit: 1,
            };
            let mut c = make_card(renderer, &card, scene, false, &mut total)?;
            target_reserve = target_reserve.max(render_reserve(&c)?);
            if total + target_reserve > DIB_BUDGET {
                return Err(oom());
            }
            render_card(&renderer.graphics, &mut c)?;
            renderer.cards.push(c);
            renderer.metrics.drawn_frames += 1;
        }
        let mut backgrounds = Vec::new();
        for (i, n) in source.nodes.iter().enumerate() {
            if n.glass.is_some() && n.card_index.is_none() {
                charge(&mut total, n.rect)?;
                target_reserve = target_reserve.max(surface_bytes(n.rect)?);
                if total + target_reserve > DIB_BUDGET {
                    return Err(oom());
                }
                backgrounds.push((i, background(&renderer.graphics, n)?));
            }
        }
        let mut groups = Vec::new();
        for node in roots {
            let rect = group_bounds(source, node)?;
            charge(&mut total, rect)?;
            groups.push(Group {
                node,
                input: renderer.input_strip(rect)?,
                dirty: None,
            });
        }
        if total + target_reserve > DIB_BUDGET {
            return Err(oom());
        }
        let mut t = TreeRenderer {
            tree: source.clone(),
            original_cards: scene.cards.clone(),
            tone: scene.text_tone,
            unverified: scene.public_calendar_unverified,
            card_count: scene.cards.len(),
            groups,
            backgrounds,
            special,
            base_bytes: total,
            scratch_peak: target_reserve,
        };
        for i in 0..t.groups.len() {
            let patch = Area::rect(t.groups[i].input.surface.rect);
            compose_group(&mut t, i, &renderer.cards, patch)?;
        }
        aggregate_inputs(&mut t, &renderer.cards);
        renderer.tree = Some(t);
        renderer.last_scene = Some(scene.generation);
        renderer.last_revision = Some(scene.source_revision);
    }
    let mut t = renderer.tree.take().ok_or_else(invalid)?;
    let result = (|| -> Result<PresentOutcome> {
        let mut patches = Vec::new();
        let mut updated = false;
        for (index, card) in renderer.cards.iter_mut().enumerate() {
            let effective = if index < t.card_count {
                t.tree
                    .nodes
                    .iter()
                    .find(|n| n.card_index == Some(index))
                    .map_or(0., |n| n.effective_opacity)
            } else {
                card.source.component_alpha
            };
            if effective == 0. {
                continue;
            }
            let mut patch = None;
            if card.render_pending {
                render_card(&renderer.graphics, card)?;
                if matches!(card.source.content, CardContent::Recess { .. }) {
                    scale_dib(
                        &mut card.surface.dib,
                        t.original_cards[index].component_alpha,
                    );
                }
                patch = Some(Area::rect(card.source.rect));
                renderer.metrics.drawn_frames += 1;
                updated = true;
            }
            let tick = match card.source.content {
                CardContent::Clock(v) => Some(if v.show_seconds {
                    now.second_of_day()
                } else {
                    now.second_of_day() / 60
                }),
                CardContent::Recess { .. } => Some(now.second_of_day()),
                _ => None,
            };
            if tick.is_some() && card.clock_tick != tick {
                render_dynamic(&renderer.graphics, card, now)?;
                if matches!(card.source.content, CardContent::Recess { .. }) {
                    if let Some(s) = &mut card.dynamic {
                        scale_dib(&mut s.dib, t.original_cards[index].component_alpha);
                    }
                }
                card.clock_tick = tick;
                renderer.metrics.drawn_frames += 1;
                updated = true;
                if let Some(d) = &card.dynamic {
                    patch = Some(patch.map_or(Area::rect(d.rect), |r| r.union(Area::rect(d.rect))));
                }
            }
            if index < t.card_count {
                if let Some(patch) = patch {
                    patches.push((index, patch));
                }
            }
        }
        for (card, patch) in patches {
            let node = t
                .tree
                .nodes
                .iter()
                .position(|n| n.card_index == Some(card))
                .ok_or_else(invalid)?;
            for group in 0..t.groups.len() {
                if descendant(&t.tree, node, t.groups[group].node) {
                    compose_group(&mut t, group, &renderer.cards, patch)?;
                }
            }
        }
        aggregate_inputs(&mut t, &renderer.cards);
        for group in &t.groups {
            group
                .input
                .state
                .context
                .replace(Some(context(scene, guard, renderer.hidden_probe)));
        }
        for index in &t.special {
            if let Some(input) = &renderer.cards[*index].input {
                input
                    .state
                    .context
                    .replace(Some(context(scene, guard, renderer.hidden_probe)));
            }
        }
        if renderer.hidden_probe {
            return Ok(PresentOutcome::Offscreen);
        }
        let restoring = renderer.hidden;
        for group in &mut t.groups {
            if !publication_allowed(scene, guard)? {
                return Ok(PresentOutcome::Hidden);
            }
            let partial = !restoring && group.dirty.is_some();
            if upload_patch(group, restoring)? {
                renderer.metrics.uploaded_frames += 1;
                if partial {
                    renderer.metrics.partial_uploads += 1;
                }
                if restoring {
                    renderer.metrics.forced_uploads += 1;
                }
            }
        }
        for index in &t.special {
            let card = &mut renderer.cards[*index];
            let alpha = if *index < t.card_count {
                t.original_cards[*index].component_alpha
            } else {
                card.source.component_alpha
            };
            if alpha == 0. {
                continue;
            }
            if card.surface.dirty || restoring {
                if let Some(backdrop) = &card.backdrop {
                    for layer in [&backdrop.fill, &backdrop.edge] {
                        if !publication_allowed(scene, guard)? {
                            return Ok(PresentOutcome::Hidden);
                        }
                        layer.apply()?;
                        renderer.metrics.backdrop_updates += 1;
                    }
                }
                if !publication_allowed(scene, guard)? {
                    return Ok(PresentOutcome::Hidden);
                }
                upload(&card.surface)?;
                card.surface.dirty = false;
                renderer.metrics.uploaded_frames += 1;
            }
            for s in card
                .dynamic
                .iter_mut()
                .chain(card.input.iter_mut().map(|i| &mut i.surface))
            {
                if s.dirty || restoring {
                    if !publication_allowed(scene, guard)? {
                        return Ok(PresentOutcome::Hidden);
                    }
                    upload(s)?;
                    s.dirty = false;
                    renderer.metrics.uploaded_frames += 1;
                }
            }
        }
        if restoring {
            for group in &t.groups {
                if !publication_allowed(scene, guard)? {
                    return Ok(PresentOutcome::Hidden);
                }
                // SAFETY: exact own root HWND; no nested node owns a window. The
                // scene/lease/system sample was rechecked immediately above.
                unsafe {
                    let _ = ShowWindow(group.input.surface.window.0, SW_SHOWNOACTIVATE);
                }
            }
            for index in &t.special {
                let card = &renderer.cards[*index];
                let alpha = if *index < t.card_count {
                    t.original_cards[*index].component_alpha
                } else {
                    card.source.component_alpha
                };
                if alpha == 0. {
                    continue;
                }
                let mut windows = Vec::new();
                if let Some(b) = &card.backdrop {
                    windows.push(b.fill.window.0);
                    windows.push(b.edge.window.0);
                }
                windows.push(card.surface.window.0);
                windows.extend(card.dynamic.iter().map(|s| s.window.0));
                windows.extend(card.input.iter().map(|i| i.surface.window.0));
                for window in windows {
                    if !publication_allowed(scene, guard)? {
                        return Ok(PresentOutcome::Hidden);
                    }
                    // SAFETY: only this scene's own small utility/backdrop windows.
                    unsafe {
                        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
                    }
                }
            }
            renderer.hidden = false;
        }
        Ok(if changed || updated || restoring {
            PresentOutcome::Published
        } else {
            PresentOutcome::Unchanged
        })
    })();
    renderer.tree = Some(t);
    if result.as_ref().is_ok_and(|o| *o == PresentOutcome::Hidden) || result.is_err() {
        renderer.hide();
    }
    refresh_metrics(renderer);
    result
}
fn scale_dib(dib: &mut Dib, alpha: f32) {
    let alpha = (alpha * 255.).round() as u16;
    for v in dib.pixels_mut() {
        *v = ((u16::from(*v) * alpha + 127) / 255) as u8;
    }
}

pub(super) fn take_events(renderer: &mut DesktopRenderer) -> Result<Vec<DesktopEvent>> {
    let Some(tree) = &renderer.tree else {
        return Ok(Vec::new());
    };
    let mut actions = Vec::new();
    for group in &tree.groups {
        let turns: Vec<_> = group.input.state.turns.borrow_mut().drain(..).collect();
        if !group
            .input
            .state
            .context
            .borrow()
            .as_ref()
            .is_some_and(input_allowed)
        {
            continue;
        }
        for (generation, action) in turns {
            if renderer.last_scene == Some(generation) {
                actions.push((generation, action));
            }
        }
    }
    let mut output = Vec::new();
    for (generation, action) in actions {
        if let InputAction::TreePage {
            key,
            card_index,
            forward,
        } = action
        {
            let t = renderer.tree.as_ref().ok_or_else(invalid)?;
            if !t.tree.nodes.iter().any(|n| {
                n.key == key && n.card_index == Some(card_index) && n.effective_opacity > 0.
            }) {
                continue;
            }
            route_action(
                &mut renderer.cards[card_index],
                generation,
                InputAction::Page(forward),
                &mut output,
            )?;
        } else if let InputAction::ImagePage { identity, page } = action {
            if renderer
                .cards
                .iter()
                .any(|c| content::valid_page(&c.source, identity, page))
            {
                route_nonpage(
                    generation,
                    InputAction::ImagePage { identity, page },
                    &mut output,
                );
            }
        } else {
            // Non-page metadata carries its original source/version IDs. It is
            // already bound to a fresh real group context and original generation.
            route_nonpage(generation, action, &mut output);
        }
    }
    Ok(output)
}
pub(super) fn probe_input(
    renderer: &DesktopRenderer,
    kind: ComponentKind,
    select: &impl Fn(&InputAction) -> bool,
) -> Option<Result<()>> {
    let tree = renderer.tree.as_ref()?;
    for group in &tree.groups {
        let hit = group
            .input
            .state
            .targets
            .borrow()
            .iter()
            .find(|hit| {
                let translated = match hit.action {
                    InputAction::TreePage { forward, .. } => InputAction::Page(forward),
                    _ => hit.action.clone(),
                };
                let owner = match hit.action {
                    InputAction::TreePage { card_index, .. } => tree
                        .original_cards
                        .get(card_index)
                        .is_some_and(|c| c.kind == kind),
                    _ => kind == ComponentKind::Materials,
                };
                owner && select(&translated)
            })
            .cloned();
        if let Some(hit) = hit {
            return Some(send_probe(&group.input, hit.rect));
        }
    }
    None
}
fn send_probe(input: &InputStrip, rect: Rect) -> Result<()> {
    let x = (rect.x + rect.width / 2.).round() as i32;
    let y = (rect.y + rect.height / 2.).round() as i32;
    if !(0..=i16::MAX as i32).contains(&x) || !(0..=i16::MAX as i32).contains(&y) {
        return Err(invalid());
    }
    let position = LPARAM(((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize);
    // SAFETY: explicit hidden diagnostic messages to this exact owned root HWND;
    // no global input, cursor, foreground or activation operation is performed.
    unsafe {
        let _ = SendMessageW(
            input.surface.window.0,
            WM_LBUTTONDOWN,
            Some(WPARAM(0)),
            Some(position),
        );
        let _ = SendMessageW(
            input.surface.window.0,
            WM_LBUTTONUP,
            Some(WPARAM(0)),
            Some(position),
        );
    }
    Ok(())
}
pub(super) fn refresh_metrics(r: &mut DesktopRenderer) {
    let Some(t) = &r.tree else { return };
    r.metrics.components = r.cards.len();
    r.metrics.tree_groups = t.groups.len();
    r.metrics.scratch_peak_bytes = t.scratch_peak;
    r.metrics.dib_bytes = t.base_bytes + t.scratch_peak;
    r.metrics.visible_leaves = r
        .cards
        .iter()
        .take(t.card_count)
        .enumerate()
        .filter(|(i, _)| {
            t.tree
                .nodes
                .iter()
                .any(|n| n.card_index == Some(*i) && n.effective_opacity > 0.)
        })
        .map(|(_, c)| c.body.visible_leaves[c.page])
        .sum();
    r.metrics.gdi_backdrop_layers = t
        .special
        .iter()
        .filter(|i| r.cards[**i].backdrop.is_some())
        .count()
        * 2;
    r.metrics.backdrop_covered_pixels = t
        .special
        .iter()
        .filter(|i| r.cards[**i].backdrop.is_some())
        .map(|i| {
            let rect = r.cards[*i].source.rect;
            rect.width.ceil() as u64 * rect.height.ceil() as u64
        })
        .sum();
    let special_surfaces = t
        .special
        .iter()
        .map(|i| {
            let c = &r.cards[*i];
            1 + usize::from(c.dynamic.is_some())
                + usize::from(c.input.is_some())
                + usize::from(c.backdrop.is_some()) * 2
        })
        .sum::<usize>();
    r.metrics.surfaces = t.groups.len() + special_surfaces;
    r.metrics.visible_windows = if r.hidden { 0 } else { r.metrics.surfaces };
    r.metrics.input_regions = if r.hidden {
        0
    } else {
        t.groups
            .iter()
            .map(|g| g.input.state.targets.borrow().len())
            .sum::<usize>()
            + t.special
                .iter()
                .filter_map(|i| r.cards[*i].input.as_ref())
                .map(|i| i.state.targets.borrow().len())
                .sum::<usize>()
    };
    r.metrics.pages = r
        .cards
        .iter()
        .map(|c| PageMetrics {
            component: c.source.kind,
            page: c.page,
            pages: c.body.pages.len(),
            utf16_units: c.body.units,
        })
        .collect();
}
fn composite_surface(
    pixels: &mut [u8],
    work: Rect,
    width: usize,
    height: usize,
    surface: &Surface,
) {
    let area = Area {
        left: work.x.round() as i32,
        top: work.y.round() as i32,
        right: work.x.round() as i32 + width as i32,
        bottom: work.y.round() as i32 + height as i32,
    };
    blend(
        pixels,
        area,
        surface.dib.pixels(),
        Area::rect(surface.rect),
        area,
        1.,
    );
}
pub(super) fn composite_groups(
    r: &DesktopRenderer,
    pixels: &mut [u8],
    work: Rect,
    width: usize,
    height: usize,
) -> Result<()> {
    if let Some(t) = &r.tree {
        for g in &t.groups {
            composite_surface(pixels, work, width, height, &g.input.surface);
        }
        for index in &t.special {
            if *index >= t.card_count {
                continue;
            }
            let c = &r.cards[*index];
            if t.original_cards[*index].component_alpha == 0. {
                continue;
            }
            if let Some(b) = &c.backdrop {
                b.fill.composite(pixels, work, width, height)?;
                b.edge.composite(pixels, work, width, height)?;
            }
            for s in std::iter::once(&c.surface).chain(c.dynamic.iter()) {
                composite_surface(pixels, work, width, height, s);
            }
        }
    }
    Ok(())
}
impl DesktopRenderer {
    /// Hidden diagnostic input targets only this renderer's exact current slot.
    pub fn probe_image_page(&self, slot: ContentSlotKey, page: u8) -> Result<()> {
        if !self.hidden_probe {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let tree = self.tree.as_ref().ok_or_else(invalid)?;
        for group in &tree.groups {
            let rect=group.input.state.targets.borrow().iter().find(|t|matches!(t.action,InputAction::ImagePage{identity,page:p} if identity.slot==slot&&p==page)).map(|t|t.rect);
            if let Some(rect) = rect {
                return send_probe(&group.input, rect);
            }
        }
        Err(invalid())
    }
    pub fn probe_next_page_node(&self, key: SceneNodeKey, forward: bool) -> Result<()> {
        if !self.hidden_probe {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let tree = self.tree.as_ref().ok_or_else(invalid)?;
        for g in &tree.groups {
            let rect=g.input.state.targets.borrow().iter().find(|t|matches!(t.action,InputAction::TreePage{key:k,forward:f,..} if k==key&&f==forward)).map(|t|t.rect);
            if let Some(rect) = rect {
                return send_probe(&g.input, rect);
            }
        }
        Err(invalid())
    }
    pub fn export_preview_bgra(
        &self,
        work: Rect,
        max_width: u16,
        max_height: u16,
    ) -> Result<DesktopPreview> {
        if !self.hidden_probe
            || max_width == 0
            || max_height == 0
            || max_width > 960
            || max_height > 540
            || surface_bytes(work).is_err()
        {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let scale = (max_width as f32 / work.width)
            .min(max_height as f32 / work.height)
            .min(1.);
        let width = (work.width * scale).floor().max(1.) as u16;
        let height = (work.height * scale).floor().max(1.) as u16;
        let bytes = width as usize * height as usize * 4;
        let base = self
            .tree
            .as_ref()
            .map_or(self.metrics.dib_bytes, |t| t.base_bytes);
        if bytes > 2 * 1024 * 1024 || base + bytes as u64 > DIB_BUDGET {
            return Err(oom());
        }
        let mut pixels = vec![0; bytes];
        if pixels.capacity() > 2 * 1024 * 1024 || base + pixels.capacity() as u64 > DIB_BUDGET {
            return Err(oom());
        }
        let sx = width as f32 / work.width;
        let sy = height as f32 / work.height;
        let mut surfaces = Vec::new();
        if let Some(t) = &self.tree {
            surfaces.extend(t.groups.iter().map(|g| &g.input.surface));
            for index in &t.special {
                let c = &self.cards[*index];
                let alpha = if *index < t.card_count {
                    t.original_cards[*index].component_alpha
                } else {
                    c.source.component_alpha
                };
                if alpha == 0. {
                    continue;
                }
                if let Some(b) = &c.backdrop {
                    b.fill
                        .composite_scaled(&mut pixels, work, width as usize, height as usize)?;
                    b.edge
                        .composite_scaled(&mut pixels, work, width as usize, height as usize)?;
                }
                surfaces.push(&c.surface);
                surfaces.extend(c.dynamic.iter());
                surfaces.extend(c.input.iter().map(|i| &i.surface));
            }
        } else {
            for c in &self.cards {
                if c.source.component_alpha == 0. {
                    continue;
                }
                if let Some(b) = &c.backdrop {
                    b.fill
                        .composite_scaled(&mut pixels, work, width as usize, height as usize)?;
                    b.edge
                        .composite_scaled(&mut pixels, work, width as usize, height as usize)?;
                }
                surfaces.push(&c.surface);
                surfaces.extend(c.dynamic.iter());
                surfaces.extend(c.input.iter().map(|i| &i.surface));
            }
        }
        for surface in surfaces {
            let source = surface.dib.pixels();
            let a = Area::rect(surface.rect);
            for y in 0..height as usize {
                let wy = work.y + (y as f32 + 0.5) / sy;
                let iy = wy.floor() as i32 - a.top;
                if iy < 0 || iy >= surface.dib.height {
                    continue;
                }
                for x in 0..width as usize {
                    let wx = work.x + (x as f32 + 0.5) / sx;
                    let ix = wx.floor() as i32 - a.left;
                    if ix < 0 || ix >= surface.dib.width {
                        continue;
                    }
                    let si = (iy as usize * surface.dib.width as usize + ix as usize) * 4;
                    let di = (y * width as usize + x) * 4;
                    let alpha = source[si + 3] as u16;
                    for k in 0..4 {
                        pixels[di + k] = (source[si + k] as u16
                            + (pixels[di + k] as u16 * (255 - alpha) + 127) / 255)
                            .min(255) as u8;
                    }
                }
            }
        }
        Ok(DesktopPreview {
            width,
            height,
            bgra: pixels,
            source_work: work,
            scale_x: sx,
            scale_y: sy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::{ids::*, layout::LayoutScope};
    use fc_presentation::scene::{ClockContent, PaintGlass, ScenePaintNode};
    use fc_runtime::authority::{AuthorityPublisher, AuthorityStore};
    fn key(n: u64) -> SceneNodeKey {
        SceneNodeKey {
            scope: LayoutScope::Public,
            id: LayoutNodeId::new(n).unwrap(),
        }
    }
    pub(super) fn now() -> LocalDateTime {
        LocalDateTime::new(crate::native::local_now().unwrap().date, 0).unwrap()
    }
    pub(super) fn authority() -> (AuthorityPublisher<()>, HealthGate) {
        let (mut p, a) = AuthorityStore::new((), Revision::new(5), AuthEpoch::new(1));
        p.initialize().unwrap();
        (p, a.gate())
    }
    pub(super) fn guard(gate: &HealthGate, generation: SceneGeneration) -> PublicationGuard<'_> {
        PublicationGuard {
            gate,
            lease: gate
                .lease_for_authorized_scope(
                    DESKTOP_SCOPE,
                    std::time::Instant::now() + std::time::Duration::from_secs(300),
                )
                .unwrap(),
            current_generation: generation,
            settings_open: false,
        }
    }
    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }
    pub(super) fn sample() -> SceneSnapshot {
        let outer = rect(0., 0., 240., 220.);
        let inner = rect(20., 20., 180., 140.);
        SceneSnapshot {
            generation: SceneGeneration::new(1),
            source_revision: Revision::new(5),
            captured_at: now(),
            valid_until: None,
            cards: vec![SceneCard {
                content_identity: None,
                kind: ComponentKind::Materials,
                rect: inner,
                font_dip: 18.,
                component_alpha: 1.,
                surface_alpha: 1.,
                edge_alpha: 0.,
                content: CardContent::Text("合成子卡".into()),
                page: Default::default(),
                visible_leaf_limit: 1,
            }],
            paint_tree: Some(ScenePaintTree {
                public_layout_revision: Revision::new(1),
                subject_profile: None,
                selection_reason: fc_domain::profile::ProfileSelectionReason::Public,
                nodes: vec![
                    ScenePaintNode {
                        key: key(1),
                        parent: None,
                        rect: outer,
                        clip: outer,
                        local_opacity: 1.,
                        effective_opacity: 1.,
                        glass: None,
                        card_index: None,
                        widget_kind: None,
                    },
                    ScenePaintNode {
                        key: key(2),
                        parent: Some(key(1)),
                        rect: outer,
                        clip: outer,
                        local_opacity: 0.5,
                        effective_opacity: 0.5,
                        glass: Some(PaintGlass {
                            surface_alpha: 1.,
                            edge_alpha: 0.,
                        }),
                        card_index: None,
                        widget_kind: None,
                    },
                    ScenePaintNode {
                        key: key(3),
                        parent: Some(key(2)),
                        rect: inner,
                        clip: inner,
                        local_opacity: 1.,
                        effective_opacity: 0.5,
                        glass: Some(PaintGlass {
                            surface_alpha: 1.,
                            edge_alpha: 0.,
                        }),
                        card_index: Some(0),
                        widget_kind: Some(fc_domain::layout::WidgetKind::Materials),
                    },
                ],
            }),
            text_tone: TextTone::Light,
            clock_rollback: false,
            public_calendar_unverified: false,
            lesson_control: None,
        }
    }
    #[test]
    fn empty_transparent_model_group_remains_selectable_metadata_without_a_surface() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_p, gate) = authority();
        let mut s = sample();
        s.paint_tree.as_mut().unwrap().nodes.push(ScenePaintNode {
            key: key(4),
            parent: Some(key(1)),
            rect: rect(0., 0., 0., 0.),
            clip: rect(0., 0., 0., 0.),
            local_opacity: 0.5,
            effective_opacity: 0.5,
            glass: None,
            card_index: None,
            widget_kind: None,
        });
        let mut r = DesktopRenderer::new_hidden().unwrap();
        r.present(&s, now(), &guard(&gate, s.generation)).unwrap();
        let t = r.tree.as_ref().unwrap();
        assert_eq!(t.groups.len(), 1);
        assert!(t.tree.nodes.iter().any(|n| n.key == key(4)));
        r.release_surfaces();
        assert_eq!(r.metrics.dib_bytes, 0);
        assert_eq!(r.metrics.input_regions, 0);
        assert!(r.tree.is_none());
        assert!(r.cards.is_empty());
    }
    #[test]
    fn resident_and_preview_renderers_share_class_registration_until_last_owner() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_p, gate) = authority();
        let s = sample();
        let resident = DesktopRenderer::new_hidden().unwrap();
        let mut preview = DesktopRenderer::new_hidden().unwrap();
        assert_eq!(*CLASS_OWNERS.lock().unwrap(), 2);
        preview
            .present(&s, now(), &guard(&gate, s.generation))
            .unwrap();
        drop(resident);
        assert_eq!(*CLASS_OWNERS.lock().unwrap(), 1);
        preview
            .present(&s, now(), &guard(&gate, s.generation))
            .unwrap();
        drop(preview);
        assert_eq!(*CLASS_OWNERS.lock().unwrap(), 0);
    }
    #[test]
    fn true_subtree_group_has_single_alpha_in_overlap_and_no_leaf_hwnds() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_p, gate) = authority();
        let s = sample();
        let mut r = DesktopRenderer::new_hidden().unwrap();
        assert_eq!(
            r.present(&s, now(), &guard(&gate, s.generation)).unwrap(),
            PresentOutcome::Offscreen
        );
        let t = r.tree.as_ref().unwrap();
        assert_eq!(t.groups.len(), 1);
        assert!(r.cards.iter().all(|c| c.surface.window.0.is_invalid()));
        let dib = &t.groups[0].input.surface.dib;
        let at = |x: usize, y: usize| dib.pixels()[(y * dib.width as usize + x) * 4 + 3];
        assert_eq!(at(100, 100), 128);
        assert_eq!(at(100, 190), 128);
        // Independently attenuated overlapping opaque parent/child would be 192.
        assert_ne!(at(100, 100), 192);
        assert!(r.metrics.dib_bytes <= DIB_BUDGET);
        assert_eq!(r.metrics.visible_windows, 0);
        let image = r
            .export_preview_bgra(rect(0., 0., 1920., 1080.), 960, 540)
            .unwrap();
        assert_eq!((image.width, image.height), (960, 540));
        assert_eq!(image.bgra.len(), 960 * 540 * 4);
        assert_eq!(image.scale_x, 0.5);
        assert!(r
            .export_preview_bgra(rect(0., 0., 1920., 1080.), 961, 540)
            .is_err());
    }
    #[test]
    fn native_tree_page_is_bound_to_exact_node_when_component_kind_repeats() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_p, gate) = authority();
        let mut s = sample();
        let first = rect(20., 20., 180., 120.);
        let second = rect(20., 160., 180., 120.);
        let outer = rect(0., 0., 240., 320.);
        s.cards[0].rect = first;
        s.cards[0].content = CardContent::Text("完整不可截断的合成资料正文\n".repeat(30));
        let mut c = s.cards[0].clone();
        c.rect = second;
        s.cards.push(c);
        let t = s.paint_tree.as_mut().unwrap();
        t.nodes[0].rect = outer;
        t.nodes[0].clip = outer;
        t.nodes[1].rect = outer;
        t.nodes[1].clip = outer;
        t.nodes[2].rect = first;
        t.nodes[2].clip = first;
        let mut n = t.nodes[2].clone();
        n.key = key(4);
        n.rect = second;
        n.clip = second;
        n.card_index = Some(1);
        t.nodes.push(n);
        let mut r = DesktopRenderer::new_hidden().unwrap();
        r.present(&s, now(), &guard(&gate, s.generation)).unwrap();
        assert!(r.cards.iter().all(|c| c.body.pages.len() > 1));
        r.probe_next_page_node(key(4), true).unwrap();
        assert!(r.take_events().unwrap().is_empty());
        assert_eq!(r.cards[0].page, 0);
        assert_eq!(r.cards[1].page, 1);
        r.present(&s, now(), &guard(&gate, s.generation)).unwrap();
        r.probe_next_page_node(key(4), true).unwrap();
        s.generation = SceneGeneration::new(2);
        r.present(&s, now(), &guard(&gate, s.generation)).unwrap();
        assert!(r.take_events().unwrap().is_empty());
        assert_eq!(r.cards[1].page, 0);
        gate.fault();
        assert!(r.probe_next_page_node(key(4), true).is_ok());
        assert!(r.take_events().unwrap().is_empty());
    }
    #[test]
    fn native_clock_tick_changes_only_owned_patch_without_remeasuring_tree() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_p, gate) = authority();
        let mut s = sample();
        s.cards[0].kind = ComponentKind::Clock;
        s.cards[0].content = CardContent::Clock(ClockContent {
            mode: ClockMode::Digital24,
            show_seconds: true,
        });
        s.paint_tree.as_mut().unwrap().nodes[2].widget_kind =
            Some(fc_domain::layout::WidgetKind::Clock);
        let mut r = DesktopRenderer::new_hidden().unwrap();
        r.present(&s, now(), &guard(&gate, s.generation)).unwrap();
        let before = r.tree.as_ref().unwrap().groups[0]
            .input
            .surface
            .dib
            .pixels()
            .to_vec();
        let layouts = r.metrics.text_layouts;
        r.tree.as_mut().unwrap().groups[0].dirty = None;
        let later = LocalDateTime::new(now().date, 1).unwrap();
        r.present(&s, later, &guard(&gate, s.generation)).unwrap();
        let group = &r.tree.as_ref().unwrap().groups[0];
        let dirty = group.dirty.unwrap();
        let dynamic = Area::rect(r.cards[0].dynamic.as_ref().unwrap().rect);
        assert_eq!(
            dirty,
            RECT {
                left: dynamic.left,
                top: dynamic.top,
                right: dynamic.right,
                bottom: dynamic.bottom
            }
        );
        let after = group.input.surface.dib.pixels();
        let w = group.input.surface.dib.width as usize;
        for y in 0..group.input.surface.dib.height as usize {
            for x in 0..w {
                if x < dynamic.left as usize
                    || x >= dynamic.right as usize
                    || y < dynamic.top as usize
                    || y >= dynamic.bottom as usize
                {
                    let i = (y * w + x) * 4;
                    assert_eq!(&before[i..i + 4], &after[i..i + 4]);
                }
            }
        }
        assert_eq!(layouts, r.metrics.text_layouts);
        assert_eq!(r.metrics.visible_windows, 0);
    }
    #[test]
    fn combined_root_cache_and_simultaneous_scratch_are_rejected_at_original_budget() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_p, gate) = authority();
        let mut s = sample();
        let large = rect(0., 0., 1600., 1000.);
        for n in s.paint_tree.as_mut().unwrap().nodes.iter_mut().take(2) {
            n.rect = large;
            n.clip = large;
        }
        let mut r = DesktopRenderer::new_hidden().unwrap();
        let result = r.present(&s, now(), &guard(&gate, s.generation));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), E_OUTOFMEMORY);
        assert_eq!(r.metrics.visible_windows, 0);
        assert!(r.tree.is_none());
    }
    #[test]
    fn native_homework_title_and_body_share_one_leaf_and_real_pages() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let _apartment = Apartment::new().unwrap();
        let g = Graphics::new().unwrap();
        // Text-based rows isolate paging semantics. Homework heading/body share
        // one leaf, rather than counting two blocks as two independent leaves.
        let mut c = sample().cards.remove(0);
        c.kind = ComponentKind::Homework;
        c.rect = rect(0., 0., 400., 1200.);
        c.visible_leaf_limit = 1;
        c.content = CardContent::Homework(
            (1..=4)
                .map(|i| fc_presentation::scene::HomeworkEntry {
                    subject_id: SubjectId::new(i).unwrap(),
                    teacher_id: PrincipalId::new(i).unwrap(),
                    subject_name: format!("科目{i}"),
                    teacher_name: format!("老师{i}"),
                    text: "完整正文".into(),
                    color: Rgb {
                        red: 100,
                        green: 140,
                        blue: 180,
                    },
                    fill_alpha: 0.03,
                })
                .collect(),
        );
        let body = prepare_content(&g, &c, false).unwrap();
        assert_eq!(body.blocks.len(), 8);
        assert_eq!(body.pages.len(), 4);
        assert_eq!(body.visible_leaves, vec![1, 1, 1, 1]);
    }
    #[test]
    fn passive_tree_facts_never_aggregate_input_or_route_paging() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_p, gate) = authority();
        for kind in [
            ComponentKind::Clock,
            ComponentKind::Date,
            ComponentKind::Schedule,
            ComponentKind::DateCountdown,
            ComponentKind::Recess,
        ] {
            let mut s = sample();
            s.cards[0].kind = kind;
            // The authoritative tree must describe the same widget as its card.
            // sample() starts as Materials; changing only the card is invalid.
            s.paint_tree.as_mut().unwrap().nodes[2].widget_kind = Some(match kind {
                ComponentKind::Clock => fc_domain::layout::WidgetKind::Clock,
                ComponentKind::Date => fc_domain::layout::WidgetKind::Date,
                ComponentKind::Schedule => fc_domain::layout::WidgetKind::Schedule,
                ComponentKind::DateCountdown => fc_domain::layout::WidgetKind::DateCountdown,
                ComponentKind::Recess => fc_domain::layout::WidgetKind::Recess,
                _ => unreachable!("only passive builtins in this fixture"),
            });
            assert!(validate(s.paint_tree.as_ref().unwrap(), &s.cards).is_ok());
            s.cards[0].content = CardContent::Text("只读事实".into());
            s.cards[0].page = fc_presentation::scene::PageInfo {
                offset: 0,
                shown: 1,
                total: 2,
            };
            let mut renderer = DesktopRenderer::new_hidden().unwrap();
            renderer
                .present(&s, now(), &guard(&gate, s.generation))
                .unwrap();
            assert!(renderer.cards.iter().all(|c| c.input.is_none()));
            assert!(renderer.tree.as_ref().unwrap().groups.iter().all(|g| g
                .input
                .state
                .targets
                .borrow()
                .is_empty()));
            assert!(renderer.take_events().unwrap().is_empty());
            // A changed lesson strip belongs to a new immutable scene generation.
            s.generation = SceneGeneration::new(2);
            s.lesson_control = Some(fc_presentation::scene::LessonControlContent {
                rect: rect(0., 240., 400., 80.),
                caption: "本节临时换课".into(),
                can_change: true,
            });
            renderer
                .present(&s, now(), &guard(&gate, s.generation))
                .unwrap();
            renderer.probe_lesson_control().unwrap();
            assert_eq!(
                renderer.take_events().unwrap(),
                vec![DesktopEvent::LessonManage {
                    generation: s.generation
                }]
            );
        }
    }
}
#[cfg(test)]
mod additional_tests {
    use super::*;
    #[test]
    fn material_visible_rows_and_row_gap_are_actual_native_pagination_constraints() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let _apartment = Apartment::new().unwrap();
        let g = Graphics::new().unwrap();
        let mut s = super::super::tests::material_scene(crate::native::local_now().unwrap());
        let c = &mut s.cards[0];
        c.rect.height = 1200.;
        c.visible_leaf_limit = 1;
        if let CardContent::Materials(list) = &mut c.content {
            list.visible_rows = 1;
            list.row_gap_dip = 23;
        }
        let body = prepare_content(&g, c, false).unwrap();
        assert_eq!(body.pages.len(), 5);
        assert_eq!(body.visible_leaves, vec![1; 5]);
        assert!(
            (body.blocks[2].top - body.blocks[1].top - body.blocks[1].height - 23.).abs() < 0.01
        );
        for (i, block) in body.blocks.iter().skip(1).take(5).enumerate() {
            assert!(
                matches!(block.action,Some(InputAction::MaterialOpen{material_id,..}) if material_id.get()==i as u64+1)
            );
        }
        if let CardContent::Materials(list) = &c.content {
            assert_eq!(list.rows.len(), 5);
        } else {
            panic!()
        }
    }
}
#[cfg(test)]
mod recess_tests {
    use super::*;
    #[test]
    fn four_k_recess_partitions_native_background_holes_and_combines_alpha_once() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_publisher, gate) = super::tests::authority();
        let mut scene = super::tests::sample();
        let rect = Rect {
            x: 0.,
            y: 0.,
            width: 3040.,
            height: 2160.,
        };
        let card = &mut scene.cards[0];
        card.kind = ComponentKind::Recess;
        card.rect = rect;
        card.component_alpha = 0.5;
        card.surface_alpha = 0.03;
        card.edge_alpha = 0.5;
        card.content = CardContent::Recess {
            deadline: LocalDateTime::new(scene.captured_at.date, 120).unwrap(),
        };
        let mut node = scene.paint_tree.as_ref().unwrap().nodes[2].clone();
        node.parent = None;
        node.rect = rect;
        node.clip = rect;
        node.local_opacity = 0.5;
        node.effective_opacity = 0.5;
        node.glass = Some(fc_presentation::scene::PaintGlass {
            surface_alpha: 0.03,
            edge_alpha: 0.5,
        });
        node.widget_kind = Some(fc_domain::layout::WidgetKind::Recess);
        scene.paint_tree.as_mut().unwrap().nodes = vec![node];
        let mut r = DesktopRenderer::new_hidden().unwrap();
        r.present(
            &scene,
            scene.captured_at,
            &super::tests::guard(&gate, scene.generation),
        )
        .unwrap();
        assert_eq!(r.metrics.tree_groups, 0);
        assert_eq!(r.metrics.gdi_backdrop_layers, 2);
        assert!(r.metrics.dib_bytes < 1024 * 1024);
        let card = &r.cards[0];
        let backdrop = card.backdrop.as_ref().unwrap();
        backdrop.fill.verify_native_paint().unwrap();
        backdrop.edge.verify_native_paint().unwrap();
        for layer in [&backdrop.fill, &backdrop.edge] {
            // SAFETY: exact owned hidden backdrop HWND, owned temporary HRGN,
            // read-only region membership, and synchronous deletion afterwards.
            unsafe {
                let region = CreateRectRgn(0, 0, 0, 0);
                assert!(!region.is_invalid());
                assert_ne!(GetWindowRgn(layer.window.0, region), RGN_ERROR);
                for small in std::iter::once(&card.surface).chain(card.dynamic.iter()) {
                    let x = (small.rect.x + small.rect.width / 2.).round() as i32;
                    let y = (small.rect.y + small.rect.height / 2.).round() as i32;
                    assert!(!PtInRegion(region, x, y).as_bool());
                }
                assert!(DeleteObject(region.into()).as_bool());
            }
        }
        let static_alpha =
            card.surface.dib.pixels()[(35 * card.surface.dib.width as usize + 200) * 4 + 3];
        assert_eq!(static_alpha, 4);
        let dynamic = card.dynamic.as_ref().unwrap();
        assert_eq!(
            dynamic.dib.pixels()[(2 * dynamic.dib.width as usize + 2) * 4 + 3],
            4
        );
        let image = r
            .export_preview_bgra(
                Rect {
                    x: 0.,
                    y: 0.,
                    width: 3840.,
                    height: 2160.,
                },
                960,
                540,
            )
            .unwrap();
        assert_eq!(image.bgra.len(), 960 * 540 * 4);
        assert_eq!(r.metrics.visible_windows, 0);
    }
}
