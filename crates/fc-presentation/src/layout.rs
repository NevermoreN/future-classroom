//! Width-first, bounded layout of the selected public/subject trees. A height
//! budget is not a percentage reference: content height stays unknown until its
//! children have been measured. No native handles or renderer caches live here.
use crate::Rect;
use fc_domain::{ids::LayoutNodeId, layout::*};
use fc_ports::TextMeasure;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneNodeKey {
    pub scope: LayoutScope,
    pub id: LayoutNodeId,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintGlass {
    /// Independent primitive alpha; ancestor/group alpha is applied once by the renderer.
    pub surface_alpha: f32,
    pub edge_alpha: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ScenePaintNode {
    pub key: SceneNodeKey,
    pub parent: Option<SceneNodeKey>,
    pub rect: Rect,
    pub clip: Rect,
    pub local_opacity: f32,
    /// For hit testing and diagnostics only, never an additional brush multiplier.
    pub effective_opacity: f32,
    pub glass: Option<PaintGlass>,
    pub card_index: Option<usize>,
    pub widget_kind: Option<WidgetKind>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ScenePaintTree {
    /// Parent precedes children. Scoped IDs are stable across geometry changes.
    pub nodes: Vec<ScenePaintNode>,
    pub public_layout_revision: fc_domain::ids::Revision,
    pub subject_profile: Option<SceneProfileStamp>,
    pub selection_reason: fc_domain::profile::ProfileSelectionReason,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneProfileStamp {
    pub id: fc_domain::ids::ProfileId,
    pub key: fc_domain::profile::SubjectProfileKey,
    pub revision: fc_domain::ids::Revision,
    pub layout_revision: fc_domain::ids::Revision,
}
#[derive(Clone, Debug)]
pub struct WidgetMeasure {
    pub kind: WidgetKind,
    pub text: String,
    pub font_dip: f32,
    pub opacity: f32,
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub preferred_width: f32,
    pub glass: PaintGlass,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeLayoutError {
    InvalidTree(LayoutError),
    InvalidGeometry,
    InvalidMeasurement,
    OutOfSpace,
    Capacity,
}
impl From<LayoutError> for TreeLayoutError {
    fn from(value: LayoutError) -> Self {
        Self::InvalidTree(value)
    }
}
fn finite_rect(r: Rect) -> bool {
    [r.x, r.y, r.width, r.height, r.x + r.width, r.y + r.height]
        .into_iter()
        .all(f32::is_finite)
        && r.width >= 0.
        && r.height >= 0.
}
fn intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Rect {
        x,
        y,
        width: (a.x + a.width).min(b.x + b.width).max(x) - x,
        height: (a.y + a.height).min(b.y + b.height).max(y) - y,
    }
}
#[derive(Clone, Copy)]
struct Ref<'a> {
    tree: &'a LayoutTree,
    id: LayoutNodeId,
}
impl<'a> Ref<'a> {
    fn node(self) -> &'a LayoutNode {
        self.tree.node(self.id).expect("validated tree")
    }
    fn key(self) -> SceneNodeKey {
        SceneNodeKey {
            scope: self.tree.scope,
            id: self.id,
        }
    }
}
struct BoxNode<'a> {
    node: Ref<'a>,
    rect: Rect,
    children: Vec<BoxNode<'a>>,
}
struct Solver<'a> {
    subject: Option<&'a LayoutTree>,
    specs: &'a [WidgetMeasure],
    measure: &'a dyn TextMeasure,
}
impl<'a> Solver<'a> {
    fn spec(&self, kind: WidgetKind) -> Option<&WidgetMeasure> {
        self.specs.iter().find(|s| s.kind == kind)
    }
    fn children(&self, n: Ref<'a>) -> Vec<Ref<'a>> {
        if matches!(n.node().kind, NodeKind::Outlet { .. }) {
            if let Some(tree) = self.subject {
                return vec![Ref {
                    tree,
                    id: tree.root,
                }];
            }
        }
        n.node()
            .children
            .iter()
            .map(|id| Ref {
                tree: n.tree,
                id: *id,
            })
            .filter(|c| !matches!(c.node().kind, NodeKind::Widget(WidgetKind::Recess)))
            .collect()
    }
    fn active(&self, n: Ref<'a>) -> bool {
        match n.node().kind {
            NodeKind::Widget(w) => self.spec(w).is_some(),
            _ => self.children(n).into_iter().any(|c| self.active(c)),
        }
    }
    fn axes(&self, n: Ref<'a>) -> Option<(AxisSize, AxisSize)> {
        match n.node().kind {
            NodeKind::Root { width, height, .. } | NodeKind::Outlet { width, height } => {
                Some((width, height))
            }
            NodeKind::Container(ref c) => Some((c.width, c.height)),
            _ => None,
        }
    }
    fn flow(&self, n: Ref<'a>) -> (Flow, f32, f32) {
        match n.node().kind {
            NodeKind::Root {
                flow, padding, gap, ..
            } => (flow, f32::from(padding), f32::from(gap)),
            NodeKind::Container(ref c) => (c.flow, f32::from(c.padding), f32::from(c.gap)),
            _ => (Flow::Column, 0., 0.),
        }
    }
    fn axis(
        axis: AxisSize,
        parent: Option<f32>,
        intrinsic: f32,
    ) -> Result<(f32, f32, f32), TreeLayoutError> {
        Ok(match axis {
            AxisSize::Fixed(v) => (v as f32, v as f32, v as f32),
            AxisSize::Content {
                min,
                preferred,
                max,
            } => {
                let min = min as f32;
                let max = max as f32;
                (min, (preferred as f32).max(intrinsic).clamp(min, max), max)
            }
            AxisSize::Percent {
                basis_points,
                min,
                max,
            } => {
                let value = (parent.ok_or(TreeLayoutError::InvalidTree(LayoutError::SizeCycle))?
                    * (basis_points as f32 / 10000.))
                    .clamp(min as f32, max as f32);
                (value, value, value)
            }
        })
    }
    fn widths(&self, n: Ref<'a>, available: f32) -> Result<(f32, f32, f32), TreeLayoutError> {
        if let NodeKind::Widget(k) = n.node().kind {
            let s = self.spec(k).ok_or(TreeLayoutError::InvalidGeometry)?;
            return Ok((
                s.min_width,
                s.preferred_width.clamp(s.min_width, s.max_width),
                s.max_width,
            ));
        }
        let (flow, pad, gap) = self.flow(n);
        let children: Vec<_> = self
            .children(n)
            .into_iter()
            .filter(|c| self.active(*c))
            .collect();
        let mut min: f32 = 0.;
        let mut pref: f32 = 0.;
        for c in &children {
            let (a, b, _) = self.widths(*c, (available - pad * 2.).max(0.))?;
            if flow == Flow::Row {
                min += a;
                pref += b;
            } else {
                min = min.max(a);
                pref = pref.max(b);
            }
        }
        if flow == Flow::Row {
            let gaps = gap * children.len().saturating_sub(1) as f32;
            min += gaps;
            pref += gaps;
        }
        min += pad * 2.;
        pref += pad * 2.;
        let axis = self.axes(n).expect("container").0;
        // The subject root gets its external viewport only after the outlet's
        // width has been allocated. Its percentage is not an intrinsic minimum
        // of the surrounding content-sized public outlet.
        if matches!(n.tree.scope, LayoutScope::Subject(_)) && n.id == n.tree.root {
            if let AxisSize::Percent {
                min: floor, max, ..
            } = axis
            {
                let min = min.max(floor as f32);
                if min > max as f32 {
                    return Err(TreeLayoutError::OutOfSpace);
                }
                return Ok((min, pref.max(min).min(max as f32), max as f32));
            }
        }
        let (a, b, c) = Self::axis(axis, Some(available), pref)?;
        if min > c + 0.01 {
            return Err(TreeLayoutError::OutOfSpace);
        }
        Ok((a.max(min), b.max(min), c))
    }
    fn min_height(&self, n: Ref<'a>) -> f32 {
        if let NodeKind::Widget(k) = n.node().kind {
            return self.spec(k).map_or(0., |s| s.min_height);
        }
        let (flow, pad, gap) = self.flow(n);
        let cs: Vec<_> = self
            .children(n)
            .into_iter()
            .filter(|c| self.active(*c))
            .collect();
        let content = if flow == Flow::Column {
            cs.iter().map(|c| self.min_height(*c)).sum::<f32>()
                + gap * cs.len().saturating_sub(1) as f32
        } else {
            cs.iter().map(|c| self.min_height(*c)).fold(0., f32::max)
        } + pad * 2.;
        let min = match self.axes(n).expect("container").1 {
            AxisSize::Fixed(v) => v as f32,
            AxisSize::Content { min, .. } | AxisSize::Percent { min, .. } => min as f32,
        };
        content.max(min)
    }
    fn layout(
        &self,
        n: Ref<'a>,
        width: f32,
        height_budget: f32,
        parent_height: Option<f32>,
    ) -> Result<BoxNode<'a>, TreeLayoutError> {
        if width <= 0. || height_budget < 0. {
            return Err(TreeLayoutError::OutOfSpace);
        }
        if let NodeKind::Widget(k) = n.node().kind {
            let s = self.spec(k).ok_or(TreeLayoutError::InvalidGeometry)?;
            if width + 0.01 < s.min_width || height_budget + 0.01 < s.min_height {
                return Err(TreeLayoutError::OutOfSpace);
            }
            let width = width.min(s.max_width);
            let measured = self
                .measure
                .height(&s.text, (width - 32.).max(1.), s.font_dip)
                .map_err(|_| TreeLayoutError::InvalidMeasurement)?;
            if !measured.is_finite() || !(0. ..=16_777_216.).contains(&measured) {
                return Err(TreeLayoutError::InvalidMeasurement);
            }
            let height = (measured.ceil() + 32.)
                .max(s.min_height)
                .min(s.max_height)
                .min(height_budget);
            return Ok(BoxNode {
                node: n,
                rect: Rect {
                    x: 0.,
                    y: 0.,
                    width,
                    height,
                },
                children: vec![],
            });
        }
        let (flow, pad, gap) = self.flow(n);
        let axis = self.axes(n).expect("container").1;
        let (_, preferred, max) = Self::axis(axis, parent_height, 0.)?;
        let known_height = if axis.is_content() {
            None
        } else {
            Some(preferred.min(height_budget))
        };
        let cap = known_height.unwrap_or(height_budget.min(max));
        if width < pad * 2. || cap < pad * 2. {
            return Err(TreeLayoutError::OutOfSpace);
        }
        let inner_w = width - pad * 2.;
        let inner_h = cap - pad * 2.;
        let cs: Vec<_> = self
            .children(n)
            .into_iter()
            .filter(|c| self.active(*c))
            .collect();
        let mut placed = Vec::new();
        let mut used_w: f32 = 0.;
        let mut used_h: f32 = 0.;
        if flow == Flow::Column {
            for (i, c) in cs.iter().enumerate() {
                let reserve = cs[i + 1..].iter().map(|c| self.min_height(*c)).sum::<f32>()
                    + gap * cs.len().saturating_sub(i + 1) as f32;
                let (min, pref, max) = self.widths(*c, inner_w)?;
                if min > inner_w + 0.01 {
                    return Err(TreeLayoutError::OutOfSpace);
                }
                // Column children use the allocated width when auto-sized, while
                // fixed/percent axes retain their declared extent.
                let cw = match self.axes(*c) {
                    Some((a, _)) if !a.is_content() => {
                        Self::axis(a, Some(inner_w), pref)?.1.min(inner_w)
                    }
                    _ => inner_w.min(max),
                };
                let mut b = self.layout(
                    *c,
                    cw,
                    inner_h - used_h - reserve,
                    known_height.map(|v| (v - pad * 2.).max(0.)),
                )?;
                b.rect.x = pad;
                b.rect.y = pad + used_h;
                if let NodeKind::Root { anchor, .. } = c.node().kind {
                    if matches!(anchor, Anchor::TopRight | Anchor::BottomRight) {
                        b.rect.x = pad + inner_w - b.rect.width;
                    }
                    if matches!(anchor, Anchor::BottomLeft | Anchor::BottomRight)
                        && known_height.is_some()
                    {
                        b.rect.y = pad + inner_h - b.rect.height;
                    }
                }
                used_h += b.rect.height;
                if i + 1 < cs.len() {
                    used_h += gap;
                }
                used_w = used_w.max(b.rect.width);
                placed.push(b);
            }
        } else {
            // Wrap breaks only at child boundaries. Width is allocated before
            // measuring any line's text; no width/height feedback loop exists.
            let mut start = 0;
            while start < cs.len() {
                let mut end = start;
                let mut minimum = 0.;
                while end < cs.len() {
                    let m = self.widths(cs[end], inner_w)?.0;
                    let next = minimum + if end > start { gap } else { 0. } + m;
                    if next > inner_w + 0.01 {
                        if end == start {
                            return Err(TreeLayoutError::OutOfSpace);
                        }
                        break;
                    }
                    minimum = next;
                    end += 1;
                    if flow == Flow::Row {
                        continue;
                    }
                }
                if flow == Flow::Row && end < cs.len() {
                    return Err(TreeLayoutError::OutOfSpace);
                }
                let bounds: Vec<_> = cs[start..end]
                    .iter()
                    .map(|c| self.widths(*c, inner_w))
                    .collect::<Result<_, _>>()?;
                let free = (inner_w - minimum).max(0.);
                let demand = bounds.iter().map(|(a, b, _)| b - a).sum::<f32>();
                let mut x = 0.;
                let mut line_h: f32 = 0.;
                for (c, (min, pref, _)) in cs[start..end].iter().zip(bounds) {
                    let w = min
                        + if demand > 0. {
                            (pref - min) * (free / demand).min(1.)
                        } else {
                            0.
                        };
                    let mut b = self.layout(
                        *c,
                        w,
                        inner_h - used_h,
                        known_height.map(|v| (v - pad * 2.).max(0.)),
                    )?;
                    b.rect.x = pad + x;
                    b.rect.y = pad + used_h;
                    x += b.rect.width + gap;
                    line_h = line_h.max(b.rect.height);
                    placed.push(b);
                }
                used_w = used_w.max((x - gap).max(0.));
                used_h += line_h;
                start = end;
                if start < cs.len() {
                    used_h += gap;
                }
            }
        }
        let content_h = used_h + pad * 2.;
        let (min, pref, max) = Self::axis(axis, parent_height, content_h)?;
        let height = known_height.unwrap_or(pref.min(height_budget));
        if height + 0.01 < min || content_h > height + 0.01 || height > max + 0.01 {
            return Err(TreeLayoutError::OutOfSpace);
        }
        Ok(BoxNode {
            node: n,
            rect: Rect {
                x: 0.,
                y: 0.,
                width,
                height,
            },
            children: placed,
        })
    }
    fn publish(
        &self,
        b: BoxNode<'a>,
        parent: Option<SceneNodeKey>,
        origin: (f32, f32),
        clip: Rect,
        alpha: f32,
        out: &mut Vec<ScenePaintNode>,
    ) {
        let rect = Rect {
            x: origin.0 + b.rect.x,
            y: origin.1 + b.rect.y,
            ..b.rect
        };
        let clip = intersect(rect, clip);
        let (local, glass, widget) = match &b.node.node().kind {
            NodeKind::Container(c) => (
                c.group_opacity_percent as f32 / 100.,
                c.material.map(|m| PaintGlass {
                    surface_alpha: m.surface_percent as f32 / 100.,
                    edge_alpha: m.edge_percent as f32 / 100.,
                }),
                None,
            ),
            NodeKind::Widget(k) => {
                let s = self.spec(*k).expect("active widget");
                (s.opacity, Some(s.glass), Some(*k))
            }
            _ => (1., None, None),
        };
        let key = b.node.key();
        out.push(ScenePaintNode {
            key,
            parent,
            rect,
            clip,
            local_opacity: local,
            effective_opacity: alpha * local,
            glass,
            card_index: None,
            widget_kind: widget,
        });
        for child in b.children {
            self.publish(child, Some(key), (rect.x, rect.y), clip, alpha * local, out);
        }
    }
}
/// Compose only the selected subject tree inside its public outlet. The caller
/// supplies an already authorized exact profile; no catalog search occurs here.
pub fn compose_layout(
    public: &LayoutTree,
    subject: Option<&LayoutTree>,
    specs: &[WidgetMeasure],
    work: Rect,
    measure: &dyn TextMeasure,
) -> Result<ScenePaintTree, TreeLayoutError> {
    public.validate_composition(subject)?;
    if !finite_rect(work)
        || work.width < 480.
        || work.height < 240.
        || work.width > 16_384.
        || work.height > 16_384.
        || work.x + work.width <= work.x
        || work.y + work.height <= work.y
    {
        return Err(TreeLayoutError::InvalidGeometry);
    }
    let mut kinds = std::collections::HashSet::new();
    for s in specs {
        if !kinds.insert(s.kind) || s.text.len() > 16_384 {
            return Err(TreeLayoutError::Capacity);
        }
        if [
            s.font_dip,
            s.opacity,
            s.min_width,
            s.max_width,
            s.min_height,
            s.max_height,
            s.preferred_width,
            s.glass.surface_alpha,
            s.glass.edge_alpha,
        ]
        .into_iter()
        .any(|v| !v.is_finite())
            || !(10. ..=72.).contains(&s.font_dip)
            || !(0. ..=1.).contains(&s.opacity)
            || s.min_width <= 0.
            || s.min_width > s.max_width
            || s.min_height <= 0.
            || s.min_height > s.max_height
            || !(0. ..=1.).contains(&s.glass.surface_alpha)
            || !(0. ..=1.).contains(&s.glass.edge_alpha)
        {
            return Err(TreeLayoutError::InvalidGeometry);
        }
    }
    let solver = Solver {
        subject,
        specs,
        measure,
    };
    let root = Ref {
        tree: public,
        id: public.root,
    };
    let (min, pref, _) = solver.widths(root, work.width)?;
    if min > work.width {
        return Err(TreeLayoutError::OutOfSpace);
    }
    let mut b = solver.layout(root, pref.min(work.width), work.height, Some(work.height))?;
    let anchor = match root.node().kind {
        NodeKind::Root { anchor, .. } => anchor,
        _ => unreachable!(),
    };
    b.rect.x = work.x
        + if matches!(anchor, Anchor::TopRight | Anchor::BottomRight) {
            work.width - b.rect.width
        } else {
            0.
        };
    b.rect.y = work.y
        + if matches!(anchor, Anchor::BottomLeft | Anchor::BottomRight) {
            work.height - b.rect.height
        } else {
            0.
        };
    let public_rect = b.rect;
    let mut nodes = Vec::new();
    solver.publish(b, None, (0., 0.), work, 1., &mut nodes);
    if let Some(n) = public
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Widget(WidgetKind::Recess)))
    {
        if let Some(s) = solver.spec(WidgetKind::Recess) {
            let candidates = [
                Rect {
                    x: work.x,
                    y: work.y,
                    width: (public_rect.x - work.x - 12.).max(0.),
                    height: work.height,
                },
                Rect {
                    x: public_rect.x + public_rect.width + 12.,
                    y: work.y,
                    width: (work.x + work.width - public_rect.x - public_rect.width - 12.).max(0.),
                    height: work.height,
                },
                Rect {
                    x: work.x,
                    y: work.y,
                    width: work.width,
                    height: (public_rect.y - work.y - 12.).max(0.),
                },
                Rect {
                    x: work.x,
                    y: public_rect.y + public_rect.height + 12.,
                    width: work.width,
                    height: (work.y + work.height - public_rect.y - public_rect.height - 12.)
                        .max(0.),
                },
            ];
            if let Some(rect) = candidates
                .into_iter()
                .filter(|r| r.width >= s.min_width.max(200.) && r.height >= s.min_height.max(160.))
                .max_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height)))
            {
                // Recess is a reserved sibling region, not clipped to the public
                // root's occupied columns. It still has its exact public node ID.
                nodes.push(ScenePaintNode {
                    key: SceneNodeKey {
                        scope: public.scope,
                        id: n.id,
                    },
                    parent: None,
                    rect,
                    clip: rect,
                    local_opacity: s.opacity,
                    effective_opacity: s.opacity,
                    glass: Some(s.glass),
                    card_index: None,
                    widget_kind: Some(WidgetKind::Recess),
                });
            }
        }
    }
    if nodes.len() > MAX_LAYOUT_NODES {
        return Err(TreeLayoutError::Capacity);
    }
    Ok(ScenePaintTree {
        nodes,
        public_layout_revision: Default::default(),
        subject_profile: None,
        selection_reason: fc_domain::profile::ProfileSelectionReason::Public,
    })
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
