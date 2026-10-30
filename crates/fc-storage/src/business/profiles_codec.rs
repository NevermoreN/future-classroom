//! Explicit, strict storage DTOs for the bounded public/subject layout documents.
use super::{codec::*, *};
use fc_domain::{layout::*, profile::*};

pub(super) fn id<T>(
    v: &Value,
    k: &str,
    make: impl FnOnce(u64) -> Result<T, InvalidId>,
) -> Result<T, StorageError> {
    make(number(v, k)?).map_err(|_| StorageError::Database)
}
pub(super) fn key(k: SubjectProfileKey) -> Value {
    json!({"class":k.class_id.get(),"subject":k.subject_id.get(),"teacher":k.teacher_id.get()})
}
pub(super) fn read_key(v: &Value) -> Result<SubjectProfileKey, StorageError> {
    object_fields(v, &["class", "subject", "teacher"])?;
    Ok(SubjectProfileKey {
        class_id: id(v, "class", ClassId::new)?,
        subject_id: id(v, "subject", SubjectId::new)?,
        teacher_id: id(v, "teacher", PrincipalId::new)?,
    })
}
fn flow(v: Flow) -> &'static str {
    match v {
        Flow::Row => "row",
        Flow::Column => "column",
        Flow::Wrap => "wrap",
    }
}
fn read_flow(v: &Value) -> Result<Flow, StorageError> {
    match v.as_str() {
        Some("row") => Ok(Flow::Row),
        Some("column") => Ok(Flow::Column),
        Some("wrap") => Ok(Flow::Wrap),
        _ => Err(StorageError::Database),
    }
}
fn axis(v: &AxisSize) -> Value {
    match v {
        AxisSize::Fixed(n) => json!({"kind":"fixed","value":n}),
        AxisSize::Content {
            min,
            preferred,
            max,
        } => json!({"kind":"content","min":min,"preferred":preferred,"max":max}),
        AxisSize::Percent {
            basis_points,
            min,
            max,
        } => json!({"kind":"percent","basis_points":basis_points,"min":min,"max":max}),
    }
}
fn u16_field(v: &Value, k: &str) -> Result<u16, StorageError> {
    u16::try_from(number(v, k)?).map_err(|_| StorageError::Database)
}
fn u8_field(v: &Value, k: &str) -> Result<u8, StorageError> {
    u8::try_from(number(v, k)?).map_err(|_| StorageError::Database)
}
fn read_axis(v: &Value) -> Result<AxisSize, StorageError> {
    match v["kind"].as_str() {
        Some("fixed") => {
            object_fields(v, &["kind", "value"])?;
            Ok(AxisSize::Fixed(u16_field(v, "value")?))
        }
        Some("content") => {
            object_fields(v, &["kind", "min", "preferred", "max"])?;
            Ok(AxisSize::Content {
                min: u16_field(v, "min")?,
                preferred: u16_field(v, "preferred")?,
                max: u16_field(v, "max")?,
            })
        }
        Some("percent") => {
            object_fields(v, &["kind", "basis_points", "min", "max"])?;
            Ok(AxisSize::Percent {
                basis_points: u16_field(v, "basis_points")?,
                min: u16_field(v, "min")?,
                max: u16_field(v, "max")?,
            })
        }
        _ => Err(StorageError::Database),
    }
}
fn widget(v: WidgetKind) -> &'static str {
    match v {
        WidgetKind::Clock => "clock",
        WidgetKind::Date => "date",
        WidgetKind::Schedule => "schedule",
        WidgetKind::Homework => "homework",
        WidgetKind::Reminder => "reminder",
        WidgetKind::DateCountdown => "countdown",
        WidgetKind::Recess => "recess",
        WidgetKind::Materials => "materials",
        WidgetKind::SubjectText => "subject_text",
        WidgetKind::SubjectImage => "subject_image",
    }
}
fn read_widget(v: &Value) -> Result<WidgetKind, StorageError> {
    match v.as_str() {
        Some("clock") => Ok(WidgetKind::Clock),
        Some("date") => Ok(WidgetKind::Date),
        Some("schedule") => Ok(WidgetKind::Schedule),
        Some("homework") => Ok(WidgetKind::Homework),
        Some("reminder") => Ok(WidgetKind::Reminder),
        Some("countdown") => Ok(WidgetKind::DateCountdown),
        Some("recess") => Ok(WidgetKind::Recess),
        Some("materials") => Ok(WidgetKind::Materials),
        Some("subject_text") => Ok(WidgetKind::SubjectText),
        Some("subject_image") => Ok(WidgetKind::SubjectImage),
        _ => Err(StorageError::Database),
    }
}
fn anchor(a: Anchor) -> &'static str {
    match a {
        Anchor::TopLeft => "top_left",
        Anchor::TopRight => "top_right",
        Anchor::BottomLeft => "bottom_left",
        Anchor::BottomRight => "bottom_right",
    }
}
fn read_anchor(v: &Value) -> Result<Anchor, StorageError> {
    match v.as_str() {
        Some("top_left") => Ok(Anchor::TopLeft),
        Some("top_right") => Ok(Anchor::TopRight),
        Some("bottom_left") => Ok(Anchor::BottomLeft),
        Some("bottom_right") => Ok(Anchor::BottomRight),
        _ => Err(StorageError::Database),
    }
}
fn node_kind(v: &NodeKind) -> Value {
    match v {
        NodeKind::Root {
            flow: f,
            padding,
            gap,
            anchor: a,
            width,
            height,
        } => {
            json!({"kind":"root","flow":flow(*f),"padding":padding,"gap":gap,"anchor":anchor(*a),"width":axis(width),"height":axis(height)})
        }
        NodeKind::Container(c) => {
            json!({"kind":"container","flow":flow(c.flow),"padding":c.padding,"gap":c.gap,"width":axis(&c.width),"height":axis(&c.height),"opacity":c.group_opacity_percent,"material":c.material.as_ref().map(|m|json!({"surface":m.surface_percent,"edge":m.edge_percent}))})
        }
        NodeKind::Outlet { width, height } => {
            json!({"kind":"outlet","width":axis(width),"height":axis(height)})
        }
        NodeKind::Widget(w) => json!({"kind":"widget","widget":widget(*w)}),
    }
}
fn read_node_kind(v: &Value) -> Result<NodeKind, StorageError> {
    match v["kind"].as_str() {
        Some("root") => {
            object_fields(
                v,
                &[
                    "kind", "flow", "padding", "gap", "anchor", "width", "height",
                ],
            )?;
            Ok(NodeKind::Root {
                flow: read_flow(&v["flow"])?,
                padding: u16_field(v, "padding")?,
                gap: u16_field(v, "gap")?,
                anchor: read_anchor(&v["anchor"])?,
                width: read_axis(&v["width"])?,
                height: read_axis(&v["height"])?,
            })
        }
        Some("container") => {
            object_fields(
                v,
                &[
                    "kind", "flow", "padding", "gap", "width", "height", "opacity", "material",
                ],
            )?;
            let material = if v["material"].is_null() {
                None
            } else {
                let m = &v["material"];
                object_fields(m, &["surface", "edge"])?;
                Some(GlassMaterial {
                    surface_percent: u8_field(m, "surface")?,
                    edge_percent: u8_field(m, "edge")?,
                })
            };
            Ok(NodeKind::Container(ContainerSettings {
                flow: read_flow(&v["flow"])?,
                padding: u16_field(v, "padding")?,
                gap: u16_field(v, "gap")?,
                width: read_axis(&v["width"])?,
                height: read_axis(&v["height"])?,
                group_opacity_percent: u8_field(v, "opacity")?,
                material,
            }))
        }
        Some("outlet") => {
            object_fields(v, &["kind", "width", "height"])?;
            Ok(NodeKind::Outlet {
                width: read_axis(&v["width"])?,
                height: read_axis(&v["height"])?,
            })
        }
        Some("widget") => {
            object_fields(v, &["kind", "widget"])?;
            Ok(NodeKind::Widget(read_widget(&v["widget"])?))
        }
        _ => Err(StorageError::Database),
    }
}
pub(super) fn tree(t: &LayoutTree) -> Value {
    json!({"scope":match t.scope {LayoutScope::Public=>json!("public"),LayoutScope::Subject(k)=>key(k)},"root":t.root.get(),"nodes":t.nodes.iter().map(|n|json!({"id":n.id.get(),"kind":node_kind(&n.kind),"children":n.children.iter().map(|c|c.get()).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}
fn read_tree(v: &Value) -> Result<LayoutTree, StorageError> {
    object_fields(v, &["scope", "root", "nodes"])?;
    let nodes = array(v, "nodes")?;
    if nodes.len() > MAX_LAYOUT_NODES {
        return Err(StorageError::Database);
    }
    let t = LayoutTree {
        scope: if v["scope"].as_str() == Some("public") {
            LayoutScope::Public
        } else {
            LayoutScope::Subject(read_key(&v["scope"])?)
        },
        root: id(v, "root", LayoutNodeId::new)?,
        nodes: nodes
            .iter()
            .map(|n| {
                object_fields(n, &["id", "kind", "children"])?;
                let children = array(n, "children")?;
                if children.len() > MAX_LAYOUT_NODES {
                    return Err(StorageError::Database);
                };
                Ok(LayoutNode {
                    id: id(n, "id", LayoutNodeId::new)?,
                    kind: read_node_kind(&n["kind"])?,
                    children: children
                        .iter()
                        .map(|c| {
                            LayoutNodeId::new(c.as_u64().ok_or(StorageError::Database)?)
                                .map_err(|_| StorageError::Database)
                        })
                        .collect::<Result<Vec<_>, StorageError>>()?,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?,
    };
    t.validate_structure().map_err(|_| StorageError::Database)?;
    Ok(t)
}
pub(super) fn public(v: &LayoutDocument) -> Value {
    let mut o = json!({"format":1,"revision":v.revision.get(),"node_high_water":v.node_id_high_water,"tree":tree(&v.tree)});
    add_styles(&mut o, &v.content_styles);
    o
}
pub(super) fn read_public(v: &Value) -> Result<LayoutDocument, StorageError> {
    let mut fields = vec!["format", "revision", "node_high_water", "tree"];
    if number(v, "format")? == 2 {
        fields.push("content_styles");
    }
    object_fields(v, &fields)?;
    if !matches!(number(v, "format")?, 1 | 2) {
        return Err(StorageError::Database);
    }
    let d = LayoutDocument {
        revision: Revision::new(number(v, "revision")?),
        node_id_high_water: number(v, "node_high_water")?,
        tree: read_tree(&v["tree"])?,
        content_styles: if number(v, "format")? == 2 {
            read_styles(&v["content_styles"])?
        } else {
            Vec::new()
        },
    };
    d.validate_structure().map_err(|_| StorageError::Database)?;
    if d.node_id_high_water < d.tree.maximum_id()
        || (d.revision == Revision::default() && d != LayoutDocument::default())
    {
        return Err(StorageError::Database);
    }
    Ok(d)
}
fn component(v: &fc_domain::desktop::ComponentSettings) -> Value {
    json!({"visible":v.visible,"opacity":v.opacity_percent,"font":v.font_dip,"min_width":v.min_width,"max_width":v.max_width,"min_height":v.min_height,"max_height":v.max_height})
}
fn read_component(v: &Value) -> Result<fc_domain::desktop::ComponentSettings, StorageError> {
    object_fields(
        v,
        &[
            "visible",
            "opacity",
            "font",
            "min_width",
            "max_width",
            "min_height",
            "max_height",
        ],
    )?;
    Ok(fc_domain::desktop::ComponentSettings {
        kind: fc_domain::desktop::ComponentKind::Materials,
        visible: boolean(v, "visible")?,
        opacity_percent: u8_field(v, "opacity")?,
        font_dip: u16_field(v, "font")?,
        min_width: u16_field(v, "min_width")?,
        max_width: u16_field(v, "max_width")?,
        min_height: u16_field(v, "min_height")?,
        max_height: u16_field(v, "max_height")?,
    })
}
pub(super) fn subject(v: &SubjectProfileDocument) -> Value {
    let mut o = json!({"format":1,"id":v.id.get(),"key":key(v.key),"enabled":v.enabled,"revision":v.revision.get(),"layout_revision":v.layout_revision.get(),"node_high_water":v.node_id_high_water,"tree":tree(&v.tree),"folder":v.folder_id.map(|f|f.get()),"materials":{"component":component(&v.materials.component),"visible_rows":v.materials.visible_rows,"row_gap":v.materials.row_gap_dip}});
    add_styles(&mut o, &v.content_styles);
    o
}
pub(super) fn read_subject(v: &Value) -> Result<SubjectProfileDocument, StorageError> {
    let mut fields = vec![
        "format",
        "id",
        "key",
        "enabled",
        "revision",
        "layout_revision",
        "node_high_water",
        "tree",
        "folder",
        "materials",
    ];
    if number(v, "format")? == 2 {
        fields.push("content_styles");
    }
    object_fields(v, &fields)?;
    if !matches!(number(v, "format")?, 1 | 2) {
        return Err(StorageError::Database);
    }
    let m = &v["materials"];
    object_fields(m, &["component", "visible_rows", "row_gap"])?;
    let d = SubjectProfileDocument {
        id: id(v, "id", ProfileId::new)?,
        key: read_key(&v["key"])?,
        enabled: boolean(v, "enabled")?,
        revision: Revision::new(number(v, "revision")?),
        layout_revision: Revision::new(number(v, "layout_revision")?),
        node_id_high_water: number(v, "node_high_water")?,
        tree: read_tree(&v["tree"])?,
        content_styles: if number(v, "format")? == 2 {
            read_styles(&v["content_styles"])?
        } else {
            Vec::new()
        },
        folder_id: if v["folder"].is_null() {
            None
        } else {
            Some(id(v, "folder", FolderId::new)?)
        },
        materials: MaterialsDisplaySettings {
            component: read_component(&m["component"])?,
            visible_rows: u8_field(m, "visible_rows")?,
            row_gap_dip: u16_field(m, "row_gap")?,
        },
    };
    d.validate_structure().map_err(|_| StorageError::Database)?;
    if d.node_id_high_water < d.tree.maximum_id()
        || d.revision == Revision::default()
        || d.layout_revision == Revision::default()
        || d.layout_revision > d.revision
    {
        return Err(StorageError::Database);
    }
    Ok(d)
}

pub(super) fn selection(s: &ProfileSelectionSnapshot) -> Value {
    json!({"public_layout":public(&s.public_layout),"subject_profile":s.subject_profile.as_ref().map(subject),"reason":match s.reason{ProfileSelectionReason::Public=>"public",ProfileSelectionReason::Selected=>"selected",ProfileSelectionReason::Missing=>"missing",ProfileSelectionReason::Disabled=>"disabled",ProfileSelectionReason::Unassigned=>"unassigned",ProfileSelectionReason::InvalidAssignment=>"invalid_assignment",ProfileSelectionReason::FolderMismatch=>"folder_mismatch",ProfileSelectionReason::LayoutMismatch=>"layout_mismatch"}})
}
pub(super) fn read_selection(v: &Value) -> Result<ProfileSelectionSnapshot, StorageError> {
    object_fields(v, &["public_layout", "subject_profile", "reason"])?;
    let reason = match v["reason"].as_str() {
        Some("public") => ProfileSelectionReason::Public,
        Some("selected") => ProfileSelectionReason::Selected,
        Some("missing") => ProfileSelectionReason::Missing,
        Some("disabled") => ProfileSelectionReason::Disabled,
        Some("unassigned") => ProfileSelectionReason::Unassigned,
        Some("invalid_assignment") => ProfileSelectionReason::InvalidAssignment,
        Some("folder_mismatch") => ProfileSelectionReason::FolderMismatch,
        Some("layout_mismatch") => ProfileSelectionReason::LayoutMismatch,
        _ => return Err(StorageError::Database),
    };
    Ok(ProfileSelectionSnapshot {
        public_layout: read_public(&v["public_layout"])?,
        subject_profile: if v["subject_profile"].is_null() {
            None
        } else {
            Some(read_subject(&v["subject_profile"])?)
        },
        reason,
    })
}

fn add_styles(v: &mut Value, styles: &[fc_domain::profile_content::ContentNodeStyle]) {
    if !styles.is_empty() {
        v["format"] = json!(2);
        v["content_styles"]=json!(styles.iter().map(|s|json!({"node":s.node_id.get(),"kind":match s.component.kind{fc_domain::desktop::ComponentKind::SubjectText=>"text",_=>"image"},"component":component(&s.component),"fit":"contain"})).collect::<Vec<_>>());
    }
}
fn read_styles(
    v: &Value,
) -> Result<Vec<fc_domain::profile_content::ContentNodeStyle>, StorageError> {
    let a = v.as_array().ok_or(StorageError::Database)?;
    if a.is_empty() || a.len() > MAX_LAYOUT_NODES {
        return Err(StorageError::Database);
    }
    a.iter()
        .map(|v| {
            object_fields(v, &["node", "kind", "component", "fit"])?;
            let mut c = read_component(&v["component"])?;
            c.kind = match text(v, "kind")?.as_str() {
                "text" => fc_domain::desktop::ComponentKind::SubjectText,
                "image" => fc_domain::desktop::ComponentKind::SubjectImage,
                _ => return Err(StorageError::Database),
            };
            if text(v, "fit")? != "contain" {
                return Err(StorageError::Database);
            }
            Ok(fc_domain::profile_content::ContentNodeStyle {
                node_id: id(v, "node", LayoutNodeId::new)?,
                component: c,
                image_fit: fc_domain::profile_content::ImageFit::Contain,
            })
        })
        .collect()
}
