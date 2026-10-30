//! Typed material metadata only. Binary upload/download uses bounded stream ports.
use crate::{parse, ProtocolError, SCHEMA_VERSION};
use fc_domain::{ids::*, materials::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialRoute {
    Scopes {
        offset: usize,
    },
    Classes {
        scope: MaterialScope,
        offset: usize,
    },
    List {
        scope: MaterialScope,
        cursor: Option<(FolderId, Revision, u64)>,
    },
    UploadCreate {
        scope: MaterialScope,
    },
    UploadBytes {
        upload_id: u64,
    },
    UploadCancel {
        upload_id: u64,
    },
    Append {
        scope: MaterialScope,
    },
    Delete {
        scope: MaterialScope,
    },
    Command {
        command_id: CommandId,
    },
    Download {
        scope: MaterialScope,
        material_id: MaterialId,
        version: ResourceVersionId,
    },
}
impl MaterialRoute {
    pub fn post(self) -> bool {
        matches!(
            self,
            Self::UploadCreate { .. }
                | Self::UploadBytes { .. }
                | Self::UploadCancel { .. }
                | Self::Append { .. }
                | Self::Delete { .. }
        )
    }
    pub fn body_limit(self) -> u64 {
        if matches!(self, Self::UploadBytes { .. }) {
            MAX_MATERIAL_BYTES
        } else if self.post() {
            crate::MAX_JSON_BYTES as u64
        } else {
            0
        }
    }
}
fn id(v: &str) -> Option<u64> {
    super::decimal(v, false)
}
pub(super) fn route(path: &str) -> Option<MaterialRoute> {
    let p: Vec<_> = path.split('/').collect();
    let scope = |a: &str, b: &str| {
        Some(MaterialScope {
            teacher_id: PrincipalId::new(id(a)?).ok()?,
            subject_id: SubjectId::new(id(b)?).ok()?,
        })
    };
    Some(match p.as_slice() {
        ["scopes", o] => MaterialRoute::Scopes {
            offset: usize::try_from(super::decimal(o, true)?)
                .ok()
                .filter(|n| *n <= 4096)?,
        },
        ["classes", a, b, o] => MaterialRoute::Classes {
            scope: scope(a, b)?,
            offset: usize::try_from(super::decimal(o, true)?)
                .ok()
                .filter(|n| *n <= 128)?,
        },
        ["folders", a, b] => MaterialRoute::List {
            scope: scope(a, b)?,
            cursor: None,
        },
        ["folders", a, b, f, r, o] => MaterialRoute::List {
            scope: scope(a, b)?,
            cursor: Some((
                FolderId::new(id(f)?).ok()?,
                Revision::new(id(r)?),
                super::decimal(o, true).filter(|n| *n <= 2048)?,
            )),
        },
        ["uploads", a, b] if *b != "bytes" && *b != "cancel" => MaterialRoute::UploadCreate {
            scope: scope(a, b)?,
        },
        ["uploads", i, "bytes"] => MaterialRoute::UploadBytes { upload_id: id(i)? },
        ["uploads", i, "cancel"] => MaterialRoute::UploadCancel { upload_id: id(i)? },
        ["folders", a, b, "append"] => MaterialRoute::Append {
            scope: scope(a, b)?,
        },
        ["folders", a, b, "delete"] => MaterialRoute::Delete {
            scope: scope(a, b)?,
        },
        ["commands", i] => MaterialRoute::Command {
            command_id: CommandId::new(id(i)?).ok()?,
        },
        ["download", a, b, i, v] => MaterialRoute::Download {
            scope: scope(a, b)?,
            material_id: MaterialId::new(id(i)?).ok()?,
            version: ResourceVersionId::new(id(v)?).ok()?,
        },
        _ => return None,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct UploadDto {
    schema_version: u16,
    display_name: String,
    bytes: u64,
}
pub struct UploadInput {
    pub display_name: String,
    pub bytes: u64,
}
pub fn upload(bytes: &[u8]) -> Result<UploadInput, ProtocolError> {
    let d: UploadDto = parse(bytes)?;
    schema(d.schema_version)?;
    validate_material_name(&d.display_name).map_err(|_| ProtocolError::InvalidCommand)?;
    let kind = MaterialKind::from_name(&d.display_name).ok_or(ProtocolError::InvalidCommand)?;
    if d.bytes == 0
        || d.bytes > MAX_MATERIAL_BYTES
        || matches!(
            kind,
            MaterialKind::Png | MaterialKind::Jpeg | MaterialKind::Bmp
        ) && d.bytes > 10 * 1024 * 1024
    {
        return Err(ProtocolError::TooLarge);
    }
    Ok(UploadInput {
        display_name: d.display_name,
        bytes: d.bytes,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AppendDto {
    schema_version: u16,
    command_id: String,
    upload_id: String,
    allowed_class_ids: Vec<String>,
}
pub struct AppendInput {
    pub command_id: CommandId,
    pub upload_id: u64,
    pub allowed_class_ids: Vec<ClassId>,
}
pub fn append(bytes: &[u8]) -> Result<AppendInput, ProtocolError> {
    let d: AppendDto = parse(bytes)?;
    schema(d.schema_version)?;
    if d.allowed_class_ids.is_empty()
        || d.allowed_class_ids.len() > fc_domain::identity::MAX_CLASSES
    {
        return Err(ProtocolError::InvalidCommand);
    }
    let mut classes = Vec::new();
    for c in d.allowed_class_ids {
        let c = ClassId::new(id(&c).ok_or(ProtocolError::InvalidCommand)?)
            .map_err(|_| ProtocolError::InvalidCommand)?;
        if classes.contains(&c) {
            return Err(ProtocolError::InvalidCommand);
        }
        classes.push(c)
    }
    Ok(AppendInput {
        command_id: CommandId::new(id(&d.command_id).ok_or(ProtocolError::InvalidCommand)?)
            .map_err(|_| ProtocolError::InvalidCommand)?,
        upload_id: id(&d.upload_id).ok_or(ProtocolError::InvalidCommand)?,
        allowed_class_ids: classes,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DeleteDto {
    schema_version: u16,
    command_id: String,
    material_id: String,
    resource_version_id: String,
    expected_material_revision: String,
}
pub struct DeleteInput {
    pub command_id: CommandId,
    pub material_id: MaterialId,
    pub resource_version_id: ResourceVersionId,
    pub expected_material_revision: Revision,
}
pub fn delete(bytes: &[u8]) -> Result<DeleteInput, ProtocolError> {
    let d: DeleteDto = parse(bytes)?;
    schema(d.schema_version)?;
    Ok(DeleteInput {
        command_id: CommandId::new(id(&d.command_id).ok_or(ProtocolError::InvalidCommand)?)
            .map_err(|_| ProtocolError::InvalidCommand)?,
        material_id: MaterialId::new(id(&d.material_id).ok_or(ProtocolError::InvalidCommand)?)
            .map_err(|_| ProtocolError::InvalidCommand)?,
        resource_version_id: ResourceVersionId::new(
            id(&d.resource_version_id).ok_or(ProtocolError::InvalidCommand)?,
        )
        .map_err(|_| ProtocolError::InvalidCommand)?,
        expected_material_revision: Revision::new(
            id(&d.expected_material_revision).ok_or(ProtocolError::InvalidCommand)?,
        ),
    })
}
fn schema(v: u16) -> Result<(), ProtocolError> {
    if v == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ProtocolError::UnsupportedSchema)
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub owner_teacher_id: String,
    pub subject_id: String,
}
impl From<MaterialScope> for Scope {
    fn from(s: MaterialScope) -> Self {
        Self {
            owner_teacher_id: s.teacher_id.get().to_string(),
            subject_id: s.subject_id.get().to_string(),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeClass<'a> {
    pub class_id: String,
    pub class_name: &'a str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeOption<'a> {
    pub owner_teacher_id: String,
    pub owner_display_name: &'a str,
    pub subject_id: String,
    pub subject_name: &'a str,
    pub classes: Vec<ScopeClass<'a>>,
    pub class_total: usize,
    pub next_class_offset: Option<usize>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Scopes<'a> {
    schema_version: u16,
    status: &'static str,
    identity_revision: String,
    offset: usize,
    total: usize,
    next_offset: Option<usize>,
    items: &'a [ScopeOption<'a>],
}
pub fn scopes(
    revision: Revision,
    offset: usize,
    total: usize,
    items: &[ScopeOption<'_>],
) -> Result<Vec<u8>, ProtocolError> {
    if items.iter().any(|i| {
        i.class_total == 0
            || i.class_total > 128
            || i.classes.len() > 20
            || i.classes.len() > i.class_total
            || i.next_class_offset != (i.classes.len() < i.class_total).then_some(i.classes.len())
    }) || items.len() > 20
        || offset > total
        || total > 4096
        || offset + items.len() > total
        || items.is_empty() && offset < total
    {
        return Err(ProtocolError::InvalidCommand);
    }
    super::responses::encode(&Scopes {
        schema_version: 1,
        status: "MATERIAL_SCOPES",
        identity_revision: revision.get().to_string(),
        offset,
        total,
        next_offset: (offset + items.len() < total).then_some(offset + items.len()),
        items,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Classes<'a> {
    schema_version: u16,
    status: &'static str,
    identity_revision: String,
    scope: Scope,
    offset: usize,
    total: usize,
    next_offset: Option<usize>,
    items: &'a [ScopeClass<'a>],
}
pub fn classes(
    revision: Revision,
    scope: MaterialScope,
    offset: usize,
    total: usize,
    items: &[ScopeClass<'_>],
) -> Result<Vec<u8>, ProtocolError> {
    if items.len() > 20
        || total > 128
        || offset > total
        || offset + items.len() > total
        || items.is_empty() && offset < total
    {
        return Err(ProtocolError::InvalidCommand);
    }
    super::responses::encode(&Classes {
        schema_version: 1,
        status: "MATERIAL_CLASSES",
        identity_revision: revision.get().to_string(),
        scope: scope.into(),
        offset,
        total,
        next_offset: (offset + items.len() < total).then_some(offset + items.len()),
        items,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record<'a> {
    material_id: String,
    folder_id: String,
    display_name: &'a str,
    resource_version_id: String,
    kind: &'static str,
    bytes: u64,
    sha256: String,
    allowed_class_ids: Vec<String>,
    added_at: String,
    added_sequence: String,
    status: &'static str,
    revision: String,
}
pub fn record(r: &MaterialRecord) -> Record<'_> {
    Record {
        material_id: r.id.get().to_string(),
        folder_id: r.folder_id.get().to_string(),
        display_name: &r.display_name,
        resource_version_id: r.resource.version_id.get().to_string(),
        kind: match r.resource.kind {
            MaterialKind::Png => "PNG",
            MaterialKind::Jpeg => "JPEG",
            MaterialKind::Bmp => "BMP",
            MaterialKind::Txt => "TXT",
            MaterialKind::Pdf => "PDF",
            MaterialKind::Pptx => "PPTX",
            MaterialKind::Docx => "DOCX",
            MaterialKind::Xlsx => "XLSX",
        },
        bytes: r.resource.bytes,
        sha256: r
            .resource
            .sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        allowed_class_ids: r
            .allowed_class_ids
            .iter()
            .map(|c| c.get().to_string())
            .collect(),
        added_at: super::content::datetime(r.added_at),
        added_sequence: r.added_sequence.to_string(),
        status: match r.status {
            MaterialStatus::Published => "PUBLISHED",
            MaterialStatus::Deleted => "DELETED",
        },
        revision: r.revision.get().to_string(),
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Cursor {
    folder_id: String,
    folder_revision: String,
    offset: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Page<'a> {
    schema_version: u16,
    status: &'static str,
    scope: Scope,
    folder_id: String,
    folder_revision: String,
    offset: u64,
    total: u64,
    items: Vec<Record<'a>>,
    next_cursor: Option<Cursor>,
}
pub fn page(p: &MaterialPage) -> Result<Vec<u8>, ProtocolError> {
    super::responses::encode(&Page {
        schema_version: 1,
        status: "MATERIALS",
        scope: p.folder.scope.into(),
        folder_id: p.folder.id.get().to_string(),
        folder_revision: p.folder.revision.get().to_string(),
        offset: p.offset,
        total: p.total,
        items: p.items.iter().map(record).collect(),
        next_cursor: p.next_cursor.as_ref().map(|c| Cursor {
            folder_id: c.folder_id.get().to_string(),
            folder_revision: c.folder_revision.get().to_string(),
            offset: c.offset,
        }),
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Committed<'a> {
    schema_version: u16,
    status: &'static str,
    command_id: String,
    scope: Scope,
    record: Record<'a>,
}
pub fn committed(
    id: CommandId,
    scope: MaterialScope,
    r: &MaterialRecord,
) -> Result<Vec<u8>, ProtocolError> {
    super::responses::encode(&Committed {
        schema_version: 1,
        status: "COMMITTED",
        command_id: id.get().to_string(),
        scope: scope.into(),
        record: record(r),
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Command<'a> {
    schema_version: u16,
    status: &'static str,
    command_id: String,
    state: &'static str,
    scope: Option<Scope>,
    record: Option<Record<'a>>,
}
pub fn command(
    id: CommandId,
    result: Option<(MaterialScope, &MaterialRecord)>,
) -> Result<Vec<u8>, ProtocolError> {
    super::responses::encode(&Command {
        schema_version: 1,
        status: "COMMAND_RESULT",
        command_id: id.get().to_string(),
        state: if result.is_some() {
            "COMMITTED"
        } else {
            "NOT_FOUND"
        },
        scope: result.map(|r| r.0.into()),
        record: result.map(|r| record(r.1)),
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Ready {
    schema_version: u16,
    status: &'static str,
    upload_id: String,
    expires_in_seconds: u16,
    max_chunk_bytes: u32,
}
pub fn ready(id: u64) -> Result<Vec<u8>, ProtocolError> {
    super::responses::encode(&Ready {
        schema_version: 1,
        status: "UPLOAD_READY",
        upload_id: id.to_string(),
        expires_in_seconds: 300,
        max_chunk_bytes: 65536,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Uploaded {
    schema_version: u16,
    status: &'static str,
    upload_id: String,
    bytes: u64,
}
pub fn uploaded(id: u64, bytes: u64) -> Result<Vec<u8>, ProtocolError> {
    super::responses::encode(&Uploaded {
        schema_version: 1,
        status: "UPLOADED",
        upload_id: id.to_string(),
        bytes,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CancelDto {
    schema_version: u16,
}
pub fn cancel(bytes: &[u8]) -> Result<(), ProtocolError> {
    let d: CancelDto = parse(bytes)?;
    schema(d.schema_version)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Cancelled {
    schema_version: u16,
    status: &'static str,
    upload_id: String,
}
pub fn cancelled(upload_id: u64) -> Result<Vec<u8>, ProtocolError> {
    super::responses::encode(&Cancelled {
        schema_version: 1,
        status: "UPLOAD_CANCELLED",
        upload_id: upload_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_and_strict_inputs() {
        assert!(matches!(
            route("uploads/12/cancel"),
            Some(MaterialRoute::UploadCancel { upload_id: 12 })
        ));
        assert!(cancel(br#"{"schemaVersion":1,"uploadId":"2"}"#).is_err());
        assert!(cancel(br#"{"schemaVersion":1}"#).is_ok());
        assert!(matches!(
            route("folders/1/2/3/4/20"),
            Some(MaterialRoute::List {
                cursor: Some(_),
                ..
            })
        ));
        for p in [
            "scopes/00",
            "scopes/4097",
            "folders/01/2",
            "uploads/0/bytes",
            "download/1/2/3/4/x",
        ] {
            assert!(route(p).is_none(), "{p}")
        }
        assert!(upload(br#"{"schemaVersion":1,"displayName":"x.pptx","bytes":52428800}"#).is_ok());
        assert!(upload(br#"{"schemaVersion":1,"displayName":"x.png","bytes":10485761}"#).is_err());
        assert!(upload(br#"{"schemaVersion":1,"displayName":"../x.txt","bytes":1}"#).is_err());
        assert!(append(
            br#"{"schemaVersion":1,"commandId":"1","uploadId":"2","allowedClassIds":["3","3"]}"#
        )
        .is_err());
        assert!(delete(br#"{"schemaVersion":1,"commandId":"1","materialId":"2","resourceVersionId":"3","expectedMaterialRevision":"1","actorId":"4"}"#).is_err());
        assert!(
            upload(br#"{"schemaVersion":1,"displayName":"x.txt","bytes":1,"bytes":2}"#).is_err()
        );
    }
    #[test]
    fn scope_serialization_budget_keeps_an_entire_large_group() {
        let name = "😀".repeat(128);
        let make = || ScopeOption {
            owner_teacher_id: "1".into(),
            owner_display_name: &name,
            subject_id: "2".into(),
            subject_name: &name,
            class_total: 128,
            next_class_offset: Some(20),
            classes: (1..=20)
                .map(|id| ScopeClass {
                    class_id: id.to_string(),
                    class_name: &name,
                })
                .collect(),
        };
        let group = vec![make()];
        assert!(scopes(Revision::new(1), 0, 20, &group).unwrap().len() < crate::MAX_JSON_BYTES);
        let many: Vec<_> = (0..20).map(|_| make()).collect();
        assert_eq!(
            scopes(Revision::new(1), 0, 20, &many).unwrap_err(),
            ProtocolError::TooLarge
        );
        assert!(scopes(Revision::new(1), 1, 20, &[]).is_err());
    }
}
