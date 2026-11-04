use super::*;
use fc_domain::{
    ids::*,
    layout::LayoutScope,
    profile::{
        LayoutDocument, MaterialsDisplaySettings, SubjectProfileDocument, SubjectProfileKey,
    },
};
use fc_ports::ProfilePage;
use serde_json::json;
pub fn key_value(key: SubjectProfileKey) -> serde_json::Value {
    json!({"classId":key.class_id.get().to_string(),"subjectId":key.subject_id.get().to_string(),"teacherId":key.teacher_id.get().to_string()})
}
pub fn page(page: &ProfilePage) -> Result<Vec<u8>, ProtocolError> {
    encode(
        Revision::default(),
        &json!({"items":page.items.iter().map(|p|json!({"id":p.id.get().to_string(),"key":key_value(p.key),"revision":p.revision.get().to_string(),"enabled":p.enabled})).collect::<Vec<_>>(),"total":page.total,"nextAfter":page.next_after.map(|id|id.get().to_string())}),
    )
}
pub fn document(profile: &SubjectProfileDocument, exists: bool) -> Result<Vec<u8>, ProtocolError> {
    let layout = LayoutDocument {
        revision: profile.layout_revision,
        node_id_high_water: profile.node_id_high_water,
        tree: profile.tree.clone(),
        content_styles: profile.content_styles.clone(),
    };
    encode(
        profile.revision,
        &json!({"exists":exists,"key":key_value(profile.key),"enabled":profile.enabled,"layoutRevision":profile.layout_revision.get().to_string(),"layout":layout::value(&layout),"folderId":profile.folder_id.map(|v|v.get().to_string()),"materials":{"component":display::Component::from(&profile.materials.component),"visibleRows":profile.materials.visible_rows,"rowGapDip":profile.materials.row_gap_dip}}),
    )
}
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
    enabled: bool,
    layout: serde_json::Value,
    folder_id: Option<String>,
    materials: Materials,
}
pub fn mutation(
    bytes: &[u8],
    key: SubjectProfileKey,
    current: Option<&SubjectProfileDocument>,
) -> Result<(Revision, SubjectProfileDocument), ProtocolError> {
    let request = Mutation::decode(bytes)?;
    if request.action != "save" {
        return Err(ProtocolError::InvalidCommand);
    }
    let (expected, c) = request.candidate::<Candidate>()?;
    let layout_revision = current.map_or(Revision::default(), |p| p.layout_revision);
    let layout = layout::candidate(c.layout, layout_revision, LayoutScope::Subject(key))?;
    let profile = SubjectProfileDocument {
        id: current.map_or(
            ProfileId::new(1).map_err(|_| ProtocolError::InvalidCommand)?,
            |p| p.id,
        ),
        key,
        enabled: c.enabled,
        revision: expected,
        layout_revision,
        node_id_high_water: layout.node_id_high_water,
        tree: layout.tree,
        content_styles: layout.content_styles,
        folder_id: c
            .folder_id
            .map(|v| FolderId::new(id(&v)?).map_err(|_| ProtocolError::InvalidCommand))
            .transpose()?,
        materials: MaterialsDisplaySettings {
            component: c.materials.component.domain()?,
            visible_rows: c.materials.visible_rows,
            row_gap_dip: c.materials.row_gap_dip,
        },
    };
    profile
        .validate_structure()
        .map_err(|_| ProtocolError::InvalidCommand)?;
    Ok((expected, profile))
}
