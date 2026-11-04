use super::*;
use fc_domain::{
    desktop::DisplaySettings,
    layout::LayoutScope,
    profile::{LayoutDocument, MaterialsDisplaySettings},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Materials {
    component: display::Component,
    visible_rows: u8,
    row_gap_dip: u16,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Candidate {
    layout: serde_json::Value,
    display: Option<display::Data>,
    materials: Option<Materials>,
    expected_publication_revision: String,
    expected_identity_revision: String,
    expected_display_revision: String,
}
pub struct Draft {
    pub expected: Revision,
    pub layout: LayoutDocument,
    pub display: Option<DisplaySettings>,
    pub materials: Option<MaterialsDisplaySettings>,
    pub publication: Revision,
    pub identity: Revision,
    pub display_revision: Revision,
}
pub fn draft(
    bytes: &[u8],
    scope: LayoutScope,
    layout_revision: Revision,
) -> Result<Draft, ProtocolError> {
    let request = Mutation::decode(bytes)?;
    if request.action != "preview" {
        return Err(ProtocolError::InvalidCommand);
    }
    let (expected, c) = request.candidate::<Candidate>()?;
    let display_revision = revision(&c.expected_display_revision)?;
    let materials = c
        .materials
        .map(|v| {
            Ok::<_, ProtocolError>(MaterialsDisplaySettings {
                component: v.component.domain()?,
                visible_rows: v.visible_rows,
                row_gap_dip: v.row_gap_dip,
            })
        })
        .transpose()?;
    if let Some(m) = &materials {
        m.validate().map_err(|_| ProtocolError::InvalidCommand)?
    }
    Ok(Draft {
        expected,
        layout: layout::candidate(c.layout, layout_revision, scope)?,
        display: c.display.map(|v| v.domain(display_revision)).transpose()?,
        materials,
        publication: revision(&c.expected_publication_revision)?,
        identity: revision(&c.expected_identity_revision)?,
        display_revision,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub publication_revision: String,
    pub identity_revision: String,
    pub display_revision: String,
    pub public_layout_revision: String,
}
#[derive(Serialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: &'static str,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub clip: Rect,
    pub text: Option<String>,
    pub font_dip: Option<f32>,
    pub text_truncated: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Geometry {
    pub width: u16,
    pub height: u16,
    pub nodes: Vec<Node>,
    pub source_publication_revision: String,
    pub identity_revision: String,
    pub display_revision: String,
    pub public_layout_revision: String,
    pub subject_profile_revision: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_draft_rejects_server_paths_and_non_preview_actions() {
        let mut input = serde_json::json!({"schemaVersion":1,"expectedRevision":"0","action":"preview","candidate":{"layout":layout::value(&LayoutDocument::default()),"display":null,"materials":null,"expectedPublicationRevision":"4","expectedIdentityRevision":"2","expectedDisplayRevision":"0"}});
        assert!(draft(
            &serde_json::to_vec(&input).unwrap(),
            LayoutScope::Public,
            Revision::default()
        )
        .is_ok());
        input["candidate"]["path"] = serde_json::json!("C:\\private");
        assert!(draft(
            &serde_json::to_vec(&input).unwrap(),
            LayoutScope::Public,
            Revision::default()
        )
        .is_err());
        input["candidate"].as_object_mut().unwrap().remove("path");
        input["action"] = serde_json::json!("save");
        assert!(draft(
            &serde_json::to_vec(&input).unwrap(),
            LayoutScope::Public,
            Revision::default()
        )
        .is_err());
    }
}
