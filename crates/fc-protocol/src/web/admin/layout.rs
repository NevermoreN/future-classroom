use super::*;
use fc_domain::{
    ids::LayoutNodeId,
    layout::*,
    profile::LayoutDocument,
    profile_content::{ContentNodeStyle, ImageFit},
};
use serde_json::{json, Value};
#[cfg(test)]
#[test]
fn canvas_frames_web_roundtrip_and_legacy_default() {
    let original = LayoutDocument::default();
    let mut old = value(&original);
    old.as_object_mut().unwrap().remove("canvasFrames");
    assert_eq!(
        candidate(old, original.revision, LayoutScope::Public).unwrap(),
        original
    );
    let mut current = original;
    current.tree.canvas_frames = current
        .tree
        .node(current.tree.root)
        .unwrap()
        .children
        .iter()
        .filter(|id| {
            !matches!(
                current.tree.node(**id).unwrap().kind,
                NodeKind::Widget(WidgetKind::Recess)
            )
        })
        .map(|id| CanvasFrame {
            node_id: *id,
            x: 20,
            y: 30,
            width: 450,
            height: 700,
        })
        .collect();
    assert_eq!(
        candidate(value(&current), current.revision, LayoutScope::Public).unwrap(),
        current
    );
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Axis {
    Fixed {
        value: u16,
    },
    Content {
        min: u16,
        preferred: u16,
        max: u16,
    },
    Percent {
        #[serde(rename = "basisPoints")]
        basis_points: u16,
        min: u16,
        max: u16,
    },
}
impl Axis {
    fn domain(self) -> AxisSize {
        match self {
            Self::Fixed { value } => AxisSize::Fixed(value),
            Self::Content {
                min,
                preferred,
                max,
            } => AxisSize::Content {
                min,
                preferred,
                max,
            },
            Self::Percent {
                basis_points,
                min,
                max,
            } => AxisSize::Percent {
                basis_points,
                min,
                max,
            },
        }
    }
}
fn axis(a: AxisSize) -> Value {
    match a {
        AxisSize::Fixed(value) => json!({"kind":"fixed","value":value}),
        AxisSize::Content {
            min,
            preferred,
            max,
        } => json!({"kind":"content","min":min,"preferred":preferred,"max":max}),
        AxisSize::Percent {
            basis_points,
            min,
            max,
        } => json!({"kind":"percent","basisPoints":basis_points,"min":min,"max":max}),
    }
}
fn flow(f: Flow) -> &'static str {
    match f {
        Flow::Row => "row",
        Flow::Column => "column",
        Flow::Wrap => "wrap",
    }
}
fn read_flow(f: &str) -> Result<Flow, ProtocolError> {
    match f {
        "row" => Ok(Flow::Row),
        "column" => Ok(Flow::Column),
        "wrap" => Ok(Flow::Wrap),
        _ => Err(ProtocolError::InvalidCommand),
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
fn read_anchor(a: &str) -> Result<Anchor, ProtocolError> {
    match a {
        "top_left" => Ok(Anchor::TopLeft),
        "top_right" => Ok(Anchor::TopRight),
        "bottom_left" => Ok(Anchor::BottomLeft),
        "bottom_right" => Ok(Anchor::BottomRight),
        _ => Err(ProtocolError::InvalidCommand),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Material {
    surface_percent: u8,
    edge_percent: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Node {
    id: String,
    children: Vec<String>,
    kind: String,
    flow: Option<String>,
    anchor: Option<String>,
    padding: Option<u16>,
    gap: Option<u16>,
    width: Option<Axis>,
    height: Option<Axis>,
    group_opacity_percent: Option<u8>,
    material: Option<Material>,
    widget: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Style {
    node_id: String,
    component: display::Component,
    image_fit: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Data {
    node_id_high_water: String,
    root: String,
    nodes: Vec<Node>,
    content_styles: Vec<Style>,
    #[serde(default)]
    canvas_frames: Vec<Frame>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Frame {
    node_id: String,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}
fn node_id(value: &str) -> Result<LayoutNodeId, ProtocolError> {
    LayoutNodeId::new(id(value)?).map_err(|_| ProtocolError::InvalidCommand)
}
fn required<T>(v: Option<T>) -> Result<T, ProtocolError> {
    v.ok_or(ProtocolError::InvalidCommand)
}
fn widget(kind: fc_domain::desktop::ComponentKind) -> WidgetKind {
    use fc_domain::desktop::ComponentKind as C;
    match kind {
        C::Clock => WidgetKind::Clock,
        C::Date => WidgetKind::Date,
        C::Schedule => WidgetKind::Schedule,
        C::Homework => WidgetKind::Homework,
        C::Materials => WidgetKind::Materials,
        C::Reminder => WidgetKind::Reminder,
        C::DateCountdown => WidgetKind::DateCountdown,
        C::Recess => WidgetKind::Recess,
        C::SubjectText => WidgetKind::SubjectText,
        C::SubjectImage => WidgetKind::SubjectImage,
    }
}
fn read_node(n: Node) -> Result<LayoutNode, ProtocolError> {
    if n.children.len() > MAX_LAYOUT_CHILDREN {
        return Err(ProtocolError::InvalidCommand);
    }
    let kind = match n.kind.as_str() {
        "root" => NodeKind::Root {
            flow: read_flow(&required(n.flow)?)?,
            anchor: read_anchor(&required(n.anchor)?)?,
            padding: required(n.padding)?,
            gap: required(n.gap)?,
            width: required(n.width)?.domain(),
            height: required(n.height)?.domain(),
        },
        "container" => NodeKind::Container(ContainerSettings {
            flow: read_flow(&required(n.flow)?)?,
            padding: required(n.padding)?,
            gap: required(n.gap)?,
            width: required(n.width)?.domain(),
            height: required(n.height)?.domain(),
            group_opacity_percent: required(n.group_opacity_percent)?,
            material: n.material.map(|m| GlassMaterial {
                surface_percent: m.surface_percent,
                edge_percent: m.edge_percent,
            }),
        }),
        "outlet" => NodeKind::Outlet {
            width: required(n.width)?.domain(),
            height: required(n.height)?.domain(),
        },
        "widget" => NodeKind::Widget(widget(display::read_kind(&required(n.widget)?)?)),
        _ => return Err(ProtocolError::InvalidCommand),
    };
    Ok(LayoutNode {
        id: node_id(&n.id)?,
        kind,
        children: n
            .children
            .iter()
            .map(|v| node_id(v))
            .collect::<Result<_, _>>()?,
    })
}
pub fn value(d: &LayoutDocument) -> Value {
    let nodes=d.tree.nodes.iter().map(|n|{
 let mut value=json!({"id":n.id.get().to_string(),"children":n.children.iter().map(|v|v.get().to_string()).collect::<Vec<_>>()});
 match n.kind{NodeKind::Root{flow:f,padding,gap,anchor:a,width,height}=>{value["kind"]=json!("root");value["flow"]=json!(flow(f));value["padding"]=json!(padding);value["gap"]=json!(gap);value["anchor"]=json!(anchor(a));value["width"]=axis(width);value["height"]=axis(height)},NodeKind::Container(ref c)=>{value["kind"]=json!("container");value["flow"]=json!(flow(c.flow));value["padding"]=json!(c.padding);value["gap"]=json!(c.gap);value["width"]=axis(c.width);value["height"]=axis(c.height);value["groupOpacityPercent"]=json!(c.group_opacity_percent);value["material"]=c.material.map(|m|json!({"surfacePercent":m.surface_percent,"edgePercent":m.edge_percent})).unwrap_or(Value::Null)},NodeKind::Outlet{width,height}=>{value["kind"]=json!("outlet");value["width"]=axis(width);value["height"]=axis(height)},NodeKind::Widget(w)=>{value["kind"]=json!("widget");value["widget"]=json!(display::kind(w.component_kind().expect("all built-in widgets map to component kinds")))}}value
 }).collect::<Vec<_>>();
    json!({"nodeIdHighWater":d.node_id_high_water.to_string(),"root":d.tree.root.get().to_string(),"nodes":nodes,"contentStyles":d.content_styles.iter().map(|s|json!({"nodeId":s.node_id.get().to_string(),"component":display::Component::from(&s.component),"imageFit":"contain"})).collect::<Vec<_>>(),"canvasFrames":d.tree.canvas_frames.iter().map(|f|json!({"nodeId":f.node_id.get().to_string(),"x":f.x,"y":f.y,"width":f.width,"height":f.height})).collect::<Vec<_>>()})
}
pub fn document(d: &LayoutDocument) -> Result<Vec<u8>, ProtocolError> {
    encode(d.revision, &value(d))
}
pub fn candidate(
    data: Value,
    expected: Revision,
    scope: LayoutScope,
) -> Result<LayoutDocument, ProtocolError> {
    let data: Data = serde_json::from_value(data).map_err(|_| ProtocolError::InvalidCommand)?;
    domain_data(data, expected, scope)
}

pub fn mutation(bytes: &[u8]) -> Result<(Revision, LayoutDocument), ProtocolError> {
    let request = Mutation::decode(bytes)?;
    if request.action != "save" {
        return Err(ProtocolError::InvalidCommand);
    }
    let (expected, data) = request.candidate::<Data>()?;
    Ok((expected, domain_data(data, expected, LayoutScope::Public)?))
}
fn domain_data(
    data: Data,
    expected: Revision,
    scope: LayoutScope,
) -> Result<LayoutDocument, ProtocolError> {
    if data.nodes.len() > MAX_LAYOUT_NODES || data.content_styles.len() > MAX_LAYOUT_NODES {
        return Err(ProtocolError::InvalidCommand);
    }
    let layout = LayoutDocument {
        revision: expected,
        node_id_high_water: revision(&data.node_id_high_water)?.get(),
        tree: LayoutTree {
            canvas_frames: data
                .canvas_frames
                .into_iter()
                .map(|f| {
                    Ok(CanvasFrame {
                        node_id: node_id(&f.node_id)?,
                        x: f.x,
                        y: f.y,
                        width: f.width,
                        height: f.height,
                    })
                })
                .collect::<Result<_, ProtocolError>>()?,
            scope,
            root: node_id(&data.root)?,
            nodes: data
                .nodes
                .into_iter()
                .map(read_node)
                .collect::<Result<_, _>>()?,
        },
        content_styles: data
            .content_styles
            .into_iter()
            .map(|s| {
                if s.image_fit != "contain" {
                    return Err(ProtocolError::InvalidCommand);
                }
                Ok(ContentNodeStyle {
                    node_id: node_id(&s.node_id)?,
                    component: s.component.domain()?,
                    image_fit: ImageFit::Contain,
                })
            })
            .collect::<Result<_, _>>()?,
    };
    layout
        .tree
        .validate_structure()
        .map_err(|_| ProtocolError::InvalidCommand)?;
    fc_domain::profile_content::validate_content_styles(&layout.tree, &layout.content_styles)
        .map_err(|_| ProtocolError::InvalidCommand)?;
    if layout.node_id_high_water < layout.tree.maximum_id() {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(layout)
}
