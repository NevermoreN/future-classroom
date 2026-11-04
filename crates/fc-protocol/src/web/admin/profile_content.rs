use super::*;
use fc_domain::{ids::*, layout::LayoutScope, profile::SubjectProfileKey, profile_content::*};
use serde_json::json;
pub fn slot(path: &str) -> Option<ContentSlotKey> {
    let parts = path.split('/').collect::<Vec<_>>();
    let (scope, node) = match parts.as_slice() {
        ["public", node] => (LayoutScope::Public, *node),
        ["subject", class, subject, teacher, node] => (
            LayoutScope::Subject(SubjectProfileKey {
                class_id: ClassId::new(id(class).ok()?).ok()?,
                subject_id: SubjectId::new(id(subject).ok()?).ok()?,
                teacher_id: PrincipalId::new(id(teacher).ok()?).ok()?,
            }),
            *node,
        ),
        _ => return None,
    };
    Some(ContentSlotKey {
        scope,
        node_id: LayoutNodeId::new(id(node).ok()?).ok()?,
    })
}
pub fn document(
    document: &ProfileContentDocument,
    kind: ContentPayloadKind,
    can_page: bool,
) -> Result<Vec<u8>, ProtocolError> {
    let current = document.current.as_ref();
    let text = match current.map(|v| &v.payload) {
        Some(ContentPayload::Text(text)) => Some(text),
        _ => None,
    };
    let images = match current.map(|v| &v.payload) {
        Some(ContentPayload::Images(pages)) => pages.as_slice(),
        _ => &[],
    };
    encode(
        document.revision,
        &json!({"canPage":can_page,"kind":match kind{ContentPayloadKind::Text=>"text",ContentPayloadKind::Images=>"images"},"text":text,"pages":images.iter().map(|p|json!({"mediaVersionId":p.version_id.get().to_string(),"width":p.width,"height":p.height,"bytes":p.bytes,"sha256":p.sha256.iter().map(|b|format!("{b:02x}")).collect::<String>(),"format":match p.format{ImageFormat::Png=>"png",ImageFormat::Jpeg=>"jpeg",ImageFormat::Bmp=>"bmp"}})).collect::<Vec<_>>(),"previousRevision":document.previous.as_ref().map(|v|v.revision.get().to_string()),"viewRevision":document.view_revision.get().to_string(),"currentPage":document.current_page,"pending":document.pending.is_some(),"updatedAt":current.map(|v|format!("{}T{:02}:{:02}:{:02}",date_text(v.updated_at.date),v.updated_at.second_of_day()/3600,(v.updated_at.second_of_day()/60)%60,v.updated_at.second_of_day()%60))}),
    )
}
pub enum Change {
    PublishText(String),
    Clear,
    Undo(Revision),
    Page {
        page: u8,
        expected_view_revision: Revision,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Text {
    text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Undo {
    target_revision: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Page {
    page: u8,
    expected_view_revision: String,
}
pub fn mutation(bytes: &[u8]) -> Result<(Revision, Change), ProtocolError> {
    let request = Mutation::decode(bytes)?;
    let revision = revision(&request.expected_revision)?;
    let change = match request.action.as_str() {
        "publishText" => {
            let text = request.candidate::<Text>()?.1.text;
            ContentPayload::Text(text.clone())
                .validate(ContentSlotKey {
                    scope: LayoutScope::Public,
                    node_id: LayoutNodeId::new(1).map_err(|_| ProtocolError::InvalidCommand)?,
                })
                .map_err(|_| ProtocolError::InvalidCommand)?;
            Change::PublishText(text)
        }
        "clear" => {
            request.candidate::<Empty>()?;
            Change::Clear
        }
        "undo" => Change::Undo(super::revision(
            &request.candidate::<Undo>()?.1.target_revision,
        )?),
        "page" => {
            let page = request.candidate::<Page>()?.1;
            if page.page as usize >= MAX_CONTENT_PAGES {
                return Err(ProtocolError::InvalidCommand);
            }
            Change::Page {
                page: page.page,
                expected_view_revision: super::revision(&page.expected_view_revision)?,
            }
        }
        _ => return Err(ProtocolError::InvalidCommand),
    };
    Ok((revision, change))
}
