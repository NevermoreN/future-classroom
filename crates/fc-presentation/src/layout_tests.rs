use super::*;
use fc_domain::{ids::*, profile::*};
use std::cell::Cell;
struct Measure {
    calls: Cell<usize>,
}
impl TextMeasure for Measure {
    fn height(&self, text: &str, width: f32, font: f32) -> Result<f32, fc_ports::MeasureError> {
        self.calls.set(self.calls.get() + 1);
        Ok((text.chars().count() as f32 * font / width).ceil().max(1.) * font)
    }
}
fn measure() -> Measure {
    Measure {
        calls: Cell::new(0),
    }
}
fn work() -> Rect {
    Rect {
        x: 40.,
        y: 30.,
        width: 1600.,
        height: 900.,
    }
}
fn specs() -> Vec<WidgetMeasure> {
    [
        WidgetKind::Clock,
        WidgetKind::Date,
        WidgetKind::Schedule,
        WidgetKind::Homework,
        WidgetKind::Materials,
    ]
    .into_iter()
    .map(|kind| WidgetMeasure {
        kind,
        body: crate::card_body::card_body_spec(
            fc_domain::desktop::ComponentKind::Date,
            &crate::scene::CardContent::Text("full owned content".into()),
            false,
            None,
        )
        .unwrap(),
        font_dip: 22.,
        opacity: 1.,
        min_width: 160.,
        max_width: 800.,
        min_height: 84.,
        max_height: 1200.,
        preferred_width: 280.,
        glass: PaintGlass {
            surface_alpha: 0.03,
            edge_alpha: 0.5,
        },
    })
    .collect()
}
fn key() -> SubjectProfileKey {
    SubjectProfileKey {
        class_id: ClassId::new(1).unwrap(),
        subject_id: SubjectId::new(2).unwrap(),
        teacher_id: PrincipalId::new(3).unwrap(),
    }
}
fn id(n: u64) -> LayoutNodeId {
    LayoutNodeId::new(n).unwrap()
}
fn enclose_clock(tree: &mut LayoutTree, opacity: u8, glass: bool) {
    let clock = tree
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Widget(WidgetKind::Clock))
        .unwrap()
        .id;
    let parent = tree
        .nodes
        .iter_mut()
        .find(|n| n.children.contains(&clock))
        .unwrap();
    *parent.children.iter_mut().find(|i| **i == clock).unwrap() = id(90);
    tree.nodes.push(LayoutNode {
        id: id(90),
        kind: NodeKind::Container(ContainerSettings {
            flow: Flow::Column,
            padding: 10,
            gap: 3,
            width: AxisSize::Content {
                min: 180,
                preferred: 280,
                max: 800,
            },
            height: AxisSize::Content {
                min: 0,
                preferred: 0,
                max: 1200,
            },
            group_opacity_percent: opacity,
            material: glass.then_some(GlassMaterial {
                surface_percent: 7,
                edge_percent: 40,
            }),
        }),
        children: vec![clock],
    });
}
#[test]
fn nested_parent_group_has_real_geometry_clip_and_independent_fill() {
    let mut public = default_public_layout();
    enclose_clock(&mut public, 50, true);
    let mut ss = specs();
    ss[0].opacity = 0.4;
    let tree = compose_layout(&public, None, &ss, work(), &measure()).unwrap();
    let parent = tree.nodes.iter().find(|n| n.key.id == id(90)).unwrap();
    let child = tree
        .nodes
        .iter()
        .find(|n| n.widget_kind == Some(WidgetKind::Clock))
        .unwrap();
    assert_eq!(child.parent, Some(parent.key));
    assert_eq!(parent.local_opacity, 0.5);
    assert_eq!(child.local_opacity, 0.4);
    assert_eq!(child.effective_opacity, 0.2);
    assert_eq!(parent.glass.unwrap().surface_alpha, 0.07);
    assert_eq!(child.glass.unwrap().surface_alpha, 0.03);
    assert_eq!(child.rect.x, parent.rect.x + 10.);
    assert_eq!(child.rect.y, parent.rect.y + 10.);
    assert!(child.clip.x >= parent.clip.x && child.clip.y >= parent.clip.y);
    assert!(child.clip.x + child.clip.width <= parent.clip.x + parent.clip.width + 0.01);
}
#[test]
fn subject_replaces_only_outlet_with_scoped_ids_and_stays_inside_public_region() {
    let public = default_public_layout();
    let subject = default_subject_layout(key());
    let tree = compose_layout(&public, Some(&subject), &specs(), work(), &measure()).unwrap();
    let outlet = tree
        .nodes
        .iter()
        .find(|n| n.key.scope == LayoutScope::Public && n.key.id == id(10))
        .unwrap();
    let subroot = tree
        .nodes
        .iter()
        .find(|n| n.key.scope == LayoutScope::Subject(key()) && n.key.id == id(1))
        .unwrap();
    assert_eq!(subroot.parent, Some(outlet.key));
    assert!(tree
        .nodes
        .iter()
        .any(|n| n.key.scope == LayoutScope::Public && n.key.id == id(1)));
    assert!(!tree
        .nodes
        .iter()
        .any(|n| n.key.scope == LayoutScope::Public && n.key.id == id(12)));
    assert_eq!(subroot.rect.x, outlet.rect.x);
    assert!((subroot.rect.width - outlet.rect.width).abs() < 0.01);
    assert!(subroot.rect.y + subroot.rect.height <= outlet.rect.y + outlet.rect.height + 0.01);
    for kind in [
        WidgetKind::Clock,
        WidgetKind::Schedule,
        WidgetKind::Homework,
    ] {
        assert_eq!(
            tree.nodes
                .iter()
                .filter(|n| n.widget_kind == Some(kind))
                .count(),
            1
        );
    }
}
#[test]
fn content_height_budget_never_becomes_a_percentage_reference() {
    let mut subject = default_subject_layout(key());
    subject.nodes.insert(
        1,
        LayoutNode {
            id: id(3),
            kind: NodeKind::Container(ContainerSettings {
                flow: Flow::Column,
                padding: 0,
                gap: 0,
                width: AxisSize::Fixed(200),
                height: AxisSize::Percent {
                    basis_points: 5000,
                    min: 0,
                    max: 1000,
                },
                group_opacity_percent: 100,
                material: None,
            }),
            children: vec![id(2)],
        },
    );
    subject.nodes[0].children = vec![id(3)];
    let m = measure();
    assert_eq!(
        compose_layout(
            &default_public_layout(),
            Some(&subject),
            &specs(),
            work(),
            &m
        ),
        Err(TreeLayoutError::InvalidTree(LayoutError::SizeCycle))
    );
    assert_eq!(m.calls.get(), 0);
}
#[test]
fn fixed_height_supports_percent_child_without_size_feedback() {
    let mut subject = default_subject_layout(key());
    if let NodeKind::Root { height, .. } = &mut subject.nodes[0].kind {
        *height = AxisSize::Fixed(220);
    }
    subject.nodes.insert(
        1,
        LayoutNode {
            id: id(3),
            kind: NodeKind::Container(ContainerSettings {
                flow: Flow::Column,
                padding: 0,
                gap: 0,
                width: AxisSize::Fixed(200),
                height: AxisSize::Percent {
                    basis_points: 5000,
                    min: 0,
                    max: 1000,
                },
                group_opacity_percent: 100,
                material: None,
            }),
            children: vec![id(2)],
        },
    );
    subject.nodes[0].children = vec![id(3)];
    let tree = compose_layout(
        &default_public_layout(),
        Some(&subject),
        &specs(),
        work(),
        &measure(),
    )
    .unwrap();
    assert_eq!(
        tree.nodes
            .iter()
            .find(|n| n.key.scope == subject.scope && n.key.id == id(3))
            .unwrap()
            .rect
            .height,
        110.
    );
}
#[test]
fn zero_group_opacity_keeps_geometry_but_disables_descendant_input() {
    let mut public = default_public_layout();
    enclose_clock(&mut public, 0, false);
    let zero = compose_layout(&public, None, &specs(), work(), &measure()).unwrap();
    if let NodeKind::Container(c) = &mut public.nodes.last_mut().unwrap().kind {
        c.group_opacity_percent = 100;
    }
    let full = compose_layout(&public, None, &specs(), work(), &measure()).unwrap();
    assert_eq!(zero.nodes.len(), full.nodes.len());
    for (a, b) in zero.nodes.iter().zip(&full.nodes) {
        assert_eq!(a.rect, b.rect);
    }
    assert_eq!(
        zero.nodes
            .iter()
            .find(|n| n.widget_kind == Some(WidgetKind::Clock))
            .unwrap()
            .effective_opacity,
        0.
    );
}
#[test]
fn unknown_hidden_widget_removes_its_gap_without_changing_ids() {
    let mut ss = specs();
    let public = default_public_layout();
    let full = compose_layout(&public, None, &ss, work(), &measure()).unwrap();
    ss.retain(|s| s.kind != WidgetKind::Date);
    let hidden = compose_layout(&public, None, &ss, work(), &measure()).unwrap();
    assert!(!hidden
        .nodes
        .iter()
        .any(|n| n.widget_kind == Some(WidgetKind::Date)));
    let a = full
        .nodes
        .iter()
        .find(|n| n.widget_kind == Some(WidgetKind::Schedule))
        .unwrap();
    let b = hidden
        .nodes
        .iter()
        .find(|n| n.widget_kind == Some(WidgetKind::Schedule))
        .unwrap();
    assert_eq!(a.key, b.key);
    assert!(a.rect.y > b.rect.y);
}
#[test]
fn invalid_geometry_and_impossible_minimum_fail_before_measurement() {
    let mut w = work();
    w.x = f32::NAN;
    let m = measure();
    assert_eq!(
        compose_layout(&default_public_layout(), None, &specs(), w, &m),
        Err(TreeLayoutError::InvalidGeometry)
    );
    let mut ss = specs();
    ss[0].min_width = 800.;
    assert_eq!(
        compose_layout(&default_public_layout(), None, &ss, work(), &m),
        Err(TreeLayoutError::OutOfSpace)
    );
    assert_eq!(m.calls.get(), 0);
}
#[test]
fn recess_uses_continuous_remaining_desktop_without_intersecting_public_tree() {
    let mut ss = specs();
    let mut recess = ss[0].clone();
    recess.kind = WidgetKind::Recess;
    ss.push(recess);
    let tree = compose_layout(&default_public_layout(), None, &ss, work(), &measure()).unwrap();
    let r = tree
        .nodes
        .iter()
        .find(|n| n.widget_kind == Some(WidgetKind::Recess))
        .unwrap();
    assert_eq!(r.parent, None);
    for n in tree
        .nodes
        .iter()
        .filter(|n| n.widget_kind.is_some() && n.widget_kind != Some(WidgetKind::Recess))
    {
        let overlap = intersect(r.rect, n.rect);
        assert_eq!(overlap.width * overlap.height, 0.);
    }
}

#[test]
fn wrap_breaks_between_nodes_and_reflows_without_shrinking_fonts() {
    let mut subject = default_subject_layout(key());
    subject.nodes[0].children = vec![id(5)];
    subject.nodes.push(LayoutNode {
        id: id(5),
        kind: NodeKind::Container(ContainerSettings {
            flow: Flow::Wrap,
            padding: 0,
            gap: 12,
            width: AxisSize::Content {
                min: 0,
                preferred: 0,
                max: 800,
            },
            height: AxisSize::Content {
                min: 0,
                preferred: 0,
                max: 1200,
            },
            group_opacity_percent: 100,
            material: Some(GlassMaterial {
                surface_percent: 3,
                edge_percent: 50,
            }),
        }),
        children: vec![id(2), id(3), id(4)],
    });
    subject.nodes.extend([3, 4].map(|n| LayoutNode {
        id: id(n),
        kind: NodeKind::Widget(WidgetKind::Materials),
        children: vec![],
    }));
    let public = default_public_layout();
    let mut ss = specs();
    let material = ss
        .iter_mut()
        .find(|s| s.kind == WidgetKind::Materials)
        .unwrap();
    material.min_width = 200.;
    let large = compose_layout(&public, Some(&subject), &ss, work(), &measure()).unwrap();
    let rows: Vec<_> = large
        .nodes
        .iter()
        .filter(|n| n.widget_kind == Some(WidgetKind::Materials))
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].rect.y, rows[1].rect.y);
    assert!(rows[2].rect.y > rows[0].rect.y);
    let mut small = work();
    small.width = 1000.;
    let narrow = compose_layout(&public, Some(&subject), &ss, small, &measure()).unwrap();
    let rows: Vec<_> = narrow
        .nodes
        .iter()
        .filter(|n| n.widget_kind == Some(WidgetKind::Materials))
        .collect();
    assert!(rows.windows(2).all(|w| w[1].rect.y > w[0].rect.y));
    assert!(rows.iter().all(|n| n.rect.width >= 200.));
    assert_eq!(
        ss.iter()
            .find(|s| s.kind == WidgetKind::Materials)
            .unwrap()
            .font_dip,
        22.
    );
}
#[test]
fn bottom_right_subject_anchor_is_relative_to_its_fixed_outlet() {
    let mut public = default_public_layout();
    if let NodeKind::Outlet { width, height } = &mut public
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
        .unwrap()
        .kind
    {
        *width = AxisSize::Fixed(400);
        *height = AxisSize::Fixed(300);
    }
    let mut subject = default_subject_layout(key());
    if let NodeKind::Root {
        anchor,
        width,
        height,
        ..
    } = &mut subject.nodes[0].kind
    {
        *anchor = Anchor::BottomRight;
        *width = AxisSize::Fixed(200);
        *height = AxisSize::Fixed(120);
    }
    let tree = compose_layout(&public, Some(&subject), &specs(), work(), &measure()).unwrap();
    let outlet = tree
        .nodes
        .iter()
        .find(|n| n.key.scope == LayoutScope::Public && n.key.id == id(10))
        .unwrap();
    let root = tree
        .nodes
        .iter()
        .find(|n| n.key.scope == subject.scope && n.key.id == subject.root)
        .unwrap();
    assert!((root.rect.x + root.rect.width - outlet.rect.x - outlet.rect.width).abs() < 0.01);
    assert!((root.rect.y + root.rect.height - outlet.rect.y - outlet.rect.height).abs() < 0.01);
}

#[test]
fn full_typed_body_is_measured_at_final_width_even_when_leaf_opacity_is_zero() {
    use crate::{card_body::card_body_spec, scene::CardContent};
    use std::cell::RefCell;
    struct ExactWidth(RefCell<Vec<(String, f32)>>);
    impl TextMeasure for ExactWidth {
        fn height(&self, text: &str, width: f32, _: f32) -> Result<f32, fc_ports::MeasureError> {
            self.0.borrow_mut().push((text.into(), width));
            Ok(100.)
        }
    }
    let mut public = default_public_layout();
    public.nodes.retain(|n| [1, 10, 12].contains(&n.id.get()));
    let root = public
        .nodes
        .iter_mut()
        .find(|n| n.id == public.root)
        .unwrap();
    root.children = vec![id(10)];
    root.kind = NodeKind::Root {
        flow: Flow::Column,
        padding: 0,
        gap: 0,
        anchor: Anchor::TopLeft,
        width: AxisSize::Fixed(400),
        height: AxisSize::Content {
            min: 0,
            preferred: 0,
            max: 2160,
        },
    };
    public
        .nodes
        .iter_mut()
        .find(|n| n.id == id(10))
        .unwrap()
        .kind = NodeKind::Outlet {
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
    let full = format!("前{}终", "完整材料🦀".repeat(4000));
    let mut spec = specs()
        .into_iter()
        .find(|s| s.kind == WidgetKind::Materials)
        .unwrap();
    spec.opacity = 0.;
    spec.body = card_body_spec(
        fc_domain::desktop::ComponentKind::Materials,
        &CardContent::Text(full.clone()),
        false,
        None,
    )
    .unwrap();
    let measure = ExactWidth(RefCell::new(vec![]));
    let tree = compose_layout(&public, None, &[spec], work(), &measure).unwrap();
    assert_eq!(measure.0.borrow().as_slice(), [(full, 352.)]);
    let leaf = tree
        .nodes
        .iter()
        .find(|n| n.widget_kind == Some(WidgetKind::Materials))
        .unwrap();
    assert_eq!((leaf.rect.width, leaf.rect.height), (400., 160.));
    assert_eq!(leaf.effective_opacity, 0.);
    assert!(tree.nodes.iter().any(|n| n.key.id == id(10)));
}
