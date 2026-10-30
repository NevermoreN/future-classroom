//! Bounded logical layout trees. Geometry and text measurement belong to
//! presentation; these rules never depend on a renderer or platform handle.
use crate::{desktop::ComponentKind, ids::LayoutNodeId, profile::SubjectProfileKey};
use std::collections::{HashMap, HashSet};

pub const MAX_LAYOUT_NODES: usize = 128;
pub const MAX_LAYOUT_CHILDREN: usize = 128;
pub const MAX_LAYOUT_DEPTH: usize = 16;
pub const MAX_TOP_LEVEL_GLASS: usize = 8;
pub const MAX_VISIBLE_LEAVES: usize = 64;
pub const MAX_LAYOUT_DIP: u16 = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutScope {
    Public,
    Subject(SubjectProfileKey),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Row,
    Column,
    Wrap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisSize {
    Fixed(u16),
    Content {
        min: u16,
        preferred: u16,
        max: u16,
    },
    Percent {
        basis_points: u16,
        min: u16,
        max: u16,
    },
}
impl AxisSize {
    pub fn validate(self) -> Result<(), LayoutError> {
        let valid = match self {
            Self::Fixed(value) => value <= MAX_LAYOUT_DIP,
            Self::Content {
                min,
                preferred,
                max,
            } => min <= preferred && preferred <= max && max <= MAX_LAYOUT_DIP,
            Self::Percent {
                basis_points,
                min,
                max,
            } => (1..=10_000).contains(&basis_points) && min <= max && max <= MAX_LAYOUT_DIP,
        };
        if valid {
            Ok(())
        } else {
            Err(LayoutError::InvalidConstraint)
        }
    }
    pub fn is_content(self) -> bool {
        matches!(self, Self::Content { .. })
    }
    pub fn is_percent(self) -> bool {
        matches!(self, Self::Percent { .. })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlassMaterial {
    pub surface_percent: u8,
    pub edge_percent: u8,
}
impl Default for GlassMaterial {
    fn default() -> Self {
        Self {
            surface_percent: 3,
            edge_percent: 50,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerSettings {
    pub flow: Flow,
    pub padding: u16,
    pub gap: u16,
    pub width: AxisSize,
    pub height: AxisSize,
    pub group_opacity_percent: u8,
    pub material: Option<GlassMaterial>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WidgetKind {
    Clock,
    Date,
    Schedule,
    Homework,
    Reminder,
    DateCountdown,
    Recess,
    Materials,
    SubjectText,
    SubjectImage,
}
impl WidgetKind {
    pub fn component_kind(self) -> Option<ComponentKind> {
        Some(match self {
            Self::Clock => ComponentKind::Clock,
            Self::Date => ComponentKind::Date,
            Self::Schedule => ComponentKind::Schedule,
            Self::Homework => ComponentKind::Homework,
            Self::Reminder => ComponentKind::Reminder,
            Self::DateCountdown => ComponentKind::DateCountdown,
            Self::Recess => ComponentKind::Recess,
            Self::Materials => ComponentKind::Materials,
            Self::SubjectText | Self::SubjectImage => return None,
        })
    }
    pub fn is_content(self) -> bool {
        matches!(
            self,
            Self::Materials | Self::SubjectText | Self::SubjectImage
        )
    }
    /// Homework inner cards and course capsules consume a real second glass
    /// level, even though their rows are supplied dynamically at publication.
    pub fn intrinsic_glass_depth(self) -> usize {
        if matches!(self, Self::Homework | Self::Schedule) {
            2
        } else {
            1
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Root {
        flow: Flow,
        padding: u16,
        gap: u16,
        anchor: Anchor,
        width: AxisSize,
        height: AxisSize,
    },
    Container(ContainerSettings),
    Outlet {
        width: AxisSize,
        height: AxisSize,
    },
    Widget(WidgetKind),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutNode {
    pub id: LayoutNodeId,
    pub kind: NodeKind,
    pub children: Vec<LayoutNodeId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutTree {
    pub scope: LayoutScope,
    pub root: LayoutNodeId,
    pub nodes: Vec<LayoutNode>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    Capacity,
    InvalidRoot,
    DuplicateId,
    MissingNode,
    MultipleParents,
    Cycle,
    Unreachable,
    ScopeMismatch,
    ProtectedWidget,
    UnsupportedWidget,
    InvalidConstraint,
    SizeCycle,
    GlassDepth,
    TooManyTopLevel,
    TooManyLeaves,
    RetiredNodeId,
    VersionOverflow,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutMetrics {
    pub nodes: usize,
    pub leaves: usize,
    pub top_level_glass: usize,
    pub glass_depth: usize,
}
struct Walker<'a> {
    tree: &'a LayoutTree,
    index: HashMap<LayoutNodeId, &'a LayoutNode>,
    path: HashSet<LayoutNodeId>,
    visited: HashSet<LayoutNodeId>,
    widgets: HashSet<WidgetKind>,
    outlets: usize,
    metrics: LayoutMetrics,
}
impl Walker<'_> {
    fn walk(
        &mut self,
        id: LayoutNodeId,
        depth: usize,
        glass: usize,
        parent_auto: [bool; 2],
        in_outlet: bool,
    ) -> Result<(), LayoutError> {
        if depth > MAX_LAYOUT_DEPTH {
            return Err(LayoutError::Capacity);
        }
        if !self.path.insert(id) {
            return Err(LayoutError::Cycle);
        }
        if !self.visited.insert(id) {
            return Err(LayoutError::MultipleParents);
        }
        let node = *self.index.get(&id).ok_or(LayoutError::MissingNode)?;
        self.metrics.nodes += 1;
        let mut next_glass = glass;
        let mut next_auto = parent_auto;
        let mut outlet = in_outlet;
        match &node.kind {
            NodeKind::Root {
                padding,
                gap,
                width,
                height,
                ..
            } => {
                if id != self.tree.root || depth != 1 {
                    return Err(LayoutError::InvalidRoot);
                }
                validate_spacing(*padding, *gap)?;
                width.validate()?;
                height.validate()?;
                // The root's percentage axes have an external viewport as their
                // finite reference. Content axes remain unknown during measure.
                next_auto = [width.is_content(), height.is_content()];
            }
            NodeKind::Container(settings) => {
                validate_spacing(settings.padding, settings.gap)?;
                if settings.group_opacity_percent > 100
                    || settings
                        .material
                        .is_some_and(|m| m.surface_percent > 100 || m.edge_percent > 100)
                {
                    return Err(LayoutError::InvalidConstraint);
                }
                next_auto = validate_axes(settings.width, settings.height, parent_auto)?;
                if settings.material.is_some() {
                    next_glass += 1;
                    if glass == 0 {
                        self.metrics.top_level_glass += 1;
                    }
                    self.metrics.glass_depth = self.metrics.glass_depth.max(next_glass);
                }
            }
            NodeKind::Outlet { width, height } => {
                if self.tree.scope != LayoutScope::Public || in_outlet || glass != 0 {
                    return Err(LayoutError::ScopeMismatch);
                }
                next_auto = validate_axes(*width, *height, parent_auto)?;
                self.outlets += 1;
                outlet = true;
            }
            NodeKind::Widget(widget) => {
                if !node.children.is_empty() {
                    return Err(LayoutError::InvalidConstraint);
                }
                if matches!(widget, WidgetKind::SubjectText | WidgetKind::SubjectImage) {
                    return Err(LayoutError::UnsupportedWidget);
                }
                match self.tree.scope {
                    LayoutScope::Public => {
                        if widget.is_content() != in_outlet {
                            return Err(LayoutError::ProtectedWidget);
                        }
                        if !widget.is_content() && !self.widgets.insert(*widget) {
                            return Err(LayoutError::DuplicateId);
                        }
                        if *widget == WidgetKind::Recess && (depth != 2 || glass != 0) {
                            return Err(LayoutError::ProtectedWidget);
                        }
                    }
                    LayoutScope::Subject(_) if !widget.is_content() => {
                        return Err(LayoutError::ProtectedWidget);
                    }
                    LayoutScope::Subject(_) => {}
                }
                self.metrics.leaves += 1;
                if glass == 0 {
                    self.metrics.top_level_glass += 1;
                }
                self.metrics.glass_depth = self
                    .metrics
                    .glass_depth
                    .max(glass + widget.intrinsic_glass_depth());
            }
        }
        if next_glass > 2 || self.metrics.glass_depth > 2 {
            return Err(LayoutError::GlassDepth);
        }
        for child in &node.children {
            self.walk(*child, depth + 1, next_glass, next_auto, outlet)?;
        }
        self.path.remove(&id);
        Ok(())
    }
}
fn validate_spacing(padding: u16, gap: u16) -> Result<(), LayoutError> {
    if padding > 256 || gap > 256 {
        Err(LayoutError::InvalidConstraint)
    } else {
        Ok(())
    }
}
fn validate_axes(
    width: AxisSize,
    height: AxisSize,
    parent_auto: [bool; 2],
) -> Result<[bool; 2], LayoutError> {
    width.validate()?;
    height.validate()?;
    if width.is_percent() && parent_auto[0] || height.is_percent() && parent_auto[1] {
        return Err(LayoutError::SizeCycle);
    }
    Ok([width.is_content(), height.is_content()])
}
impl LayoutTree {
    pub fn node(&self, id: LayoutNodeId) -> Option<&LayoutNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn maximum_id(&self) -> u64 {
        self.nodes.iter().map(|n| n.id.get()).max().unwrap_or(0)
    }
    pub fn validate(&self) -> Result<(), LayoutError> {
        self.validate_structure()
    }
    pub fn validate_structure(&self) -> Result<(), LayoutError> {
        self.metrics().map(|_| ())
    }
    pub fn metrics(&self) -> Result<LayoutMetrics, LayoutError> {
        if self.nodes.is_empty() || self.nodes.len() > MAX_LAYOUT_NODES {
            return Err(LayoutError::Capacity);
        }
        let mut index = HashMap::new();
        let mut parents = HashMap::<LayoutNodeId, usize>::new();
        for n in &self.nodes {
            if n.children.len() > MAX_LAYOUT_CHILDREN {
                return Err(LayoutError::Capacity);
            }
            if index.insert(n.id, n).is_some() {
                return Err(LayoutError::DuplicateId);
            }
            for child in &n.children {
                let count = parents.entry(*child).or_default();
                *count += 1;
                if *count > 1 {
                    return Err(LayoutError::MultipleParents);
                }
            }
        }
        if !matches!(
            index.get(&self.root).map(|n| &n.kind),
            Some(NodeKind::Root { .. })
        ) || parents.contains_key(&self.root)
        {
            return Err(LayoutError::InvalidRoot);
        }
        if parents.keys().any(|id| !index.contains_key(id)) {
            return Err(LayoutError::MissingNode);
        }
        // Check all components for cycles, including disconnected malicious ones.
        fn visit(
            id: LayoutNodeId,
            index: &HashMap<LayoutNodeId, &LayoutNode>,
            active: &mut HashSet<LayoutNodeId>,
            done: &mut HashSet<LayoutNodeId>,
        ) -> Result<(), LayoutError> {
            if active.contains(&id) {
                return Err(LayoutError::Cycle);
            }
            if done.contains(&id) {
                return Ok(());
            }
            active.insert(id);
            for child in &index.get(&id).ok_or(LayoutError::MissingNode)?.children {
                visit(*child, index, active, done)?;
            }
            active.remove(&id);
            done.insert(id);
            Ok(())
        }
        let mut active = HashSet::new();
        let mut done = HashSet::new();
        for id in index.keys() {
            visit(*id, &index, &mut active, &mut done)?;
        }
        let mut walker = Walker {
            tree: self,
            index,
            path: HashSet::new(),
            visited: HashSet::new(),
            widgets: HashSet::new(),
            outlets: 0,
            metrics: LayoutMetrics::default(),
        };
        walker.walk(self.root, 1, 0, [false, false], false)?;
        if walker.visited.len() != self.nodes.len() {
            return Err(LayoutError::Unreachable);
        }
        if self.scope == LayoutScope::Public && walker.outlets != 1 {
            return Err(LayoutError::InvalidRoot);
        }
        let m = walker.metrics;
        if m.top_level_glass > MAX_TOP_LEVEL_GLASS {
            return Err(LayoutError::TooManyTopLevel);
        }
        if m.leaves > MAX_VISIBLE_LEAVES {
            return Err(LayoutError::TooManyLeaves);
        }
        Ok(m)
    }
    /// Count the actual selected static templates, not both the public fallback
    /// and its replacement. Dynamic child rows must additionally obey publication
    /// leaf budgets in presentation, using pagination rather than text truncation.
    pub fn validate_composition(
        &self,
        subject: Option<&Self>,
    ) -> Result<LayoutMetrics, LayoutError> {
        self.validate_structure()?;
        if self.scope != LayoutScope::Public {
            return Err(LayoutError::ScopeMismatch);
        }
        let Some(subject) = subject else {
            return self.metrics();
        };
        subject.validate_structure()?;
        if !matches!(subject.scope, LayoutScope::Subject(_)) {
            return Err(LayoutError::ScopeMismatch);
        }
        let outlet = self
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
            .ok_or(LayoutError::InvalidRoot)?;
        if let NodeKind::Outlet { height, .. } = &outlet.kind {
            if height.is_content()
                && matches!(subject.node(subject.root).map(|n| &n.kind), Some(NodeKind::Root { height, .. }) if height.is_percent())
            {
                return Err(LayoutError::SizeCycle);
            }
        }
        let mut fallback: HashSet<_> = self
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Widget(WidgetKind::Recess)))
            .map(|n| n.id)
            .collect();
        let mut pending = outlet.children.clone();
        while let Some(id) = pending.pop() {
            if fallback.insert(id) {
                pending.extend(
                    self.node(id)
                        .ok_or(LayoutError::MissingNode)?
                        .children
                        .iter()
                        .copied(),
                );
            }
        }
        let mut replaced = self.clone();
        replaced.nodes.retain(|n| !fallback.contains(&n.id));
        for node in &mut replaced.nodes {
            node.children.retain(|id| !fallback.contains(id));
        }
        let outlet = replaced
            .nodes
            .iter_mut()
            .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
            .ok_or(LayoutError::InvalidRoot)?;
        outlet.children.clear();
        let outlet_id = outlet.id;
        let a = replaced.metrics()?;
        let b = subject.metrics()?;
        fn depth(tree: &LayoutTree, target: Option<LayoutNodeId>) -> Result<usize, LayoutError> {
            let mut maximum = 0;
            let mut pending = vec![(tree.root, 1usize)];
            while let Some((id, level)) = pending.pop() {
                if target == Some(id) {
                    return Ok(level);
                }
                maximum = maximum.max(level);
                let node = tree.node(id).ok_or(LayoutError::MissingNode)?;
                pending.extend(node.children.iter().map(|id| (*id, level + 1)));
            }
            if target.is_some() {
                Err(LayoutError::MissingNode)
            } else {
                Ok(maximum)
            }
        }
        let outlet_depth = depth(&replaced, Some(outlet_id))?;
        if outlet_depth + depth(subject, None)? > MAX_LAYOUT_DEPTH {
            return Err(LayoutError::Capacity);
        }
        let result = LayoutMetrics {
            nodes: a.nodes + b.nodes,
            leaves: a.leaves + b.leaves,
            top_level_glass: a.top_level_glass + b.top_level_glass,
            glass_depth: a.glass_depth.max(b.glass_depth),
        };
        if result.nodes > MAX_LAYOUT_NODES {
            return Err(LayoutError::Capacity);
        }
        if result.leaves > MAX_VISIBLE_LEAVES {
            return Err(LayoutError::TooManyLeaves);
        }
        if result.top_level_glass > MAX_TOP_LEVEL_GLASS {
            return Err(LayoutError::TooManyTopLevel);
        }
        Ok(result)
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.nodes.iter().try_fold(
            self.nodes
                .capacity()
                .checked_mul(std::mem::size_of::<LayoutNode>())?,
            |n, node| {
                n.checked_add(
                    node.children
                        .capacity()
                        .checked_mul(std::mem::size_of::<LayoutNodeId>())?,
                )
            },
        )
    }
    /// Reset/remap without reusing any retired physical node ID. Arithmetic is
    /// checked before constructing a partial tree; the original stays untouched.
    pub fn remap_fresh_ids(&self, after: u64) -> Result<Self, LayoutError> {
        self.validate_structure()?;
        after
            .checked_add(self.nodes.len() as u64)
            .ok_or(LayoutError::VersionOverflow)?;
        let mut map = HashMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            map.insert(
                n.id,
                LayoutNodeId::new(after + i as u64 + 1)
                    .map_err(|_| LayoutError::VersionOverflow)?,
            );
        }
        let mut result = self.clone();
        result.root = *map.get(&self.root).ok_or(LayoutError::MissingNode)?;
        for n in &mut result.nodes {
            n.id = *map.get(&n.id).ok_or(LayoutError::MissingNode)?;
            for c in &mut n.children {
                *c = *map.get(c).ok_or(LayoutError::MissingNode)?;
            }
        }
        Ok(result)
    }
}
fn fixed_id(n: u64) -> LayoutNodeId {
    LayoutNodeId::new(n).expect("constant nonzero layout node identifier")
}
fn percentage() -> AxisSize {
    AxisSize::Percent {
        basis_points: 10_000,
        min: 0,
        max: MAX_LAYOUT_DIP,
    }
}
fn node(id: u64, kind: NodeKind, children: &[u64]) -> LayoutNode {
    LayoutNode {
        id: fixed_id(id),
        kind,
        children: children.iter().map(|n| fixed_id(*n)).collect(),
    }
}
pub fn default_public_layout() -> LayoutTree {
    let column = |preferred| {
        NodeKind::Container(ContainerSettings {
            flow: Flow::Column,
            padding: 0,
            gap: 12,
            width: AxisSize::Content {
                min: 160,
                preferred,
                max: 800,
            },
            height: percentage(),
            group_opacity_percent: 100,
            material: None,
        })
    };
    LayoutTree {
        scope: LayoutScope::Public,
        root: fixed_id(1),
        nodes: vec![
            node(
                1,
                NodeKind::Root {
                    flow: Flow::Row,
                    padding: 16,
                    gap: 12,
                    anchor: Anchor::TopRight,
                    width: AxisSize::Percent {
                        basis_points: 4600,
                        min: 420,
                        max: 800,
                    },
                    height: percentage(),
                },
                &[2, 3, 11],
            ),
            node(2, column(280), &[4, 5, 6, 7]),
            node(3, column(480), &[8, 9, 10]),
            node(4, NodeKind::Widget(WidgetKind::Clock), &[]),
            node(5, NodeKind::Widget(WidgetKind::Date), &[]),
            node(6, NodeKind::Widget(WidgetKind::Schedule), &[]),
            node(7, NodeKind::Widget(WidgetKind::DateCountdown), &[]),
            node(8, NodeKind::Widget(WidgetKind::Homework), &[]),
            node(9, NodeKind::Widget(WidgetKind::Reminder), &[]),
            node(
                10,
                NodeKind::Outlet {
                    width: AxisSize::Content {
                        min: 160,
                        preferred: 300,
                        max: 800,
                    },
                    height: AxisSize::Content {
                        min: 0,
                        preferred: 0,
                        max: 2160,
                    },
                },
                &[12],
            ),
            node(11, NodeKind::Widget(WidgetKind::Recess), &[]),
            node(12, NodeKind::Widget(WidgetKind::Materials), &[]),
        ],
    }
}
pub fn default_subject_layout(key: SubjectProfileKey) -> LayoutTree {
    LayoutTree {
        scope: LayoutScope::Subject(key),
        root: fixed_id(1),
        nodes: vec![
            node(
                1,
                NodeKind::Root {
                    flow: Flow::Column,
                    padding: 0,
                    gap: 12,
                    anchor: Anchor::TopLeft,
                    width: percentage(),
                    height: AxisSize::Content {
                        min: 0,
                        preferred: 0,
                        max: 2160,
                    },
                },
                &[2],
            ),
            node(2, NodeKind::Widget(WidgetKind::Materials), &[]),
        ],
    }
}
