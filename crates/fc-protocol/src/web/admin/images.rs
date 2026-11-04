use super::*;
use fc_domain::{ids::*, profile_content::*};
use serde_json::json;
pub const CHUNK_BYTES: usize = 512 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u16,
    pub expected_revision: String,
    pub items: Vec<Item>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub name: String,
    pub bytes: u64,
}
pub fn manifest(bytes: &[u8]) -> Result<(Revision, Vec<Item>), ProtocolError> {
    let input: Manifest = parse(bytes)?;
    if input.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    if input.items.is_empty() || input.items.len() > MAX_CONTENT_PAGES {
        return Err(ProtocolError::InvalidCommand);
    }
    for item in &input.items {
        if item.name.is_empty()
            || item.name.chars().count() > 255
            || item.name.contains(['\\', '/', ':', '\0'])
            || item.bytes == 0
            || item.bytes > MAX_CONTENT_IMAGE_BYTES
        {
            return Err(ProtocolError::InvalidCommand);
        }
    }
    Ok((revision(&input.expected_revision)?, input.items))
}
#[derive(Deserialize)]
#[serde(tag = "source", rename_all = "camelCase", deny_unknown_fields)]
pub enum Page {
    Existing {
        #[serde(rename = "mediaVersionId")]
        media_version_id: String,
    },
    Upload {
        #[serde(rename = "uploadId")]
        upload_id: String,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Images {
    pub batch_id: Option<String>,
    pub pages: Vec<Page>,
}
pub fn publication(bytes: &[u8]) -> Result<(Revision, Images), ProtocolError> {
    let request = Mutation::decode(bytes)?;
    if request.action != "publishImages" {
        return Err(ProtocolError::InvalidCommand);
    }
    let (expected, input) = request.candidate::<Images>()?;
    if input.pages.is_empty() || input.pages.len() > MAX_CONTENT_PAGES {
        return Err(ProtocolError::InvalidCommand);
    }
    if let Some(batch) = &input.batch_id {
        id(batch)?;
    }
    for page in &input.pages {
        match page {
            Page::Existing { media_version_id } => {
                id(media_version_id)?;
            }
            Page::Upload { upload_id } => {
                if input.batch_id.is_none() {
                    return Err(ProtocolError::InvalidCommand);
                }
                id(upload_id)?;
            }
        }
    }
    Ok((expected, input))
}
pub fn created(
    batch: CommandId,
    items: &[(MediaVersionId, String, u64)],
) -> Result<Vec<u8>, ProtocolError> {
    crate::web::responses::encode(
        &json!({"schemaVersion":1,"batchId":batch.get().to_string(),"uploads":items.iter().map(|(id,name,bytes)|json!({"id":id.get().to_string(),"name":name,"bytes":bytes})).collect::<Vec<_>>()}),
    )
}
pub fn accepted() -> Result<Vec<u8>, ProtocolError> {
    crate::web::responses::encode(&json!({"schemaVersion":1,"status":"UPLOADED"}))
}
pub fn cancelled() -> Result<Vec<u8>, ProtocolError> {
    crate::web::responses::encode(&json!({"schemaVersion":1,"status":"CANCELLED"}))
}
pub fn empty(bytes: &[u8]) -> Result<(), ProtocolError> {
    crate::web::pairing::logout(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controlled_image_manifest_rejects_paths_unknown_fields_and_oversize_originals() {
        assert!(manifest(br#"{"schemaVersion":1,"expectedRevision":"0","items":[{"name":"board.png","bytes":524289}]}"#).is_ok());
        for body in [
            r#"{"schemaVersion":1,"expectedRevision":"0","items":[{"name":"../board.png","bytes":2}]}"#,
            r#"{"schemaVersion":1,"expectedRevision":"0","items":[{"name":"board.png","bytes":10485761}]}"#,
            r#"{"schemaVersion":1,"expectedRevision":"0","items":[{"name":"board.png","bytes":2,"path":"C:\\secret"}]}"#,
            r#"{"schemaVersion":1,"expectedRevision":"0","items":[]}"#,
        ] {
            assert!(manifest(body.as_bytes()).is_err());
        }
    }
    #[test]
    fn published_image_upload_ids_require_a_server_batch_and_existing_reorder_does_not() {
        assert!(publication(br#"{"schemaVersion":1,"expectedRevision":"2","action":"publishImages","candidate":{"batchId":null,"pages":[{"source":"existing","mediaVersionId":"7"}]}}"#).is_ok());
        assert!(publication(br#"{"schemaVersion":1,"expectedRevision":"2","action":"publishImages","candidate":{"batchId":null,"pages":[{"source":"upload","uploadId":"7"}]}}"#).is_err());
        assert!(publication(br#"{"schemaVersion":1,"expectedRevision":"2","action":"publishImages","candidate":{"batchId":"3","pages":[{"source":"upload","uploadId":"7"}]}}"#).is_ok());
    }
}
