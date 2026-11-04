//! Local-only maintenance DTOs. File bodies use bounded streaming, never JSON.
use crate::{ProtocolError, SCHEMA_VERSION};
use fc_domain::{ids::*, recovery::MigrationMapping};
use serde::{Deserialize, Serialize};

pub const BASE: &str = "/fc/api/v1/admin/maintenance/jobs";
pub const PROOF_HEADER: &str = "x-fc-maintenance-proof";
pub const MAX_PACKAGE_BYTES: u64 = fc_ports::http::MAX_TRANSFER_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Export,
    Import,
    Reset,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Mode {
    Ordinary,
    Mapped,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Phase {
    Waiting,
    ReadyForUpload,
    Uploading,
    Uploaded,
    Working,
    MappingRequired,
    ReviewReady,
    Confirming,
    Draining,
    PreparingDatabase,
    Sealing,
    Committing,
    Retained,
    Finished,
    Cancelled,
    Failed,
}
impl Phase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Finished | Self::Cancelled | Self::Failed)
    }
    pub fn can_cancel(self) -> bool {
        !matches!(
            self,
            Self::Committing | Self::Retained | Self::Finished | Self::Cancelled | Self::Failed
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Jobs,
    Job(u64),
    Upload(u64),
    File(u64),
    Preflight(u64),
    Mapping(u64),
    Confirm(u64),
    Cancel(u64),
}
pub fn recognize(path: &str) -> Option<Resource> {
    if path == BASE {
        return Some(Resource::Jobs);
    }
    let suffix = path.strip_prefix(BASE)?.strip_prefix('/')?;
    let (id, action) = suffix.split_once('/').unwrap_or((suffix, ""));
    let id = super::admin::id(id).ok()?;
    Some(match action {
        "" => Resource::Job(id),
        "upload" => Resource::Upload(id),
        "file" => Resource::File(id),
        "preflight" => Resource::Preflight(id),
        "mapping" => Resource::Mapping(id),
        "confirm" => Resource::Confirm(id),
        "cancel" => Resource::Cancel(id),
        _ => return None,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Create {
    pub schema_version: u16,
    pub expected_revision: String,
    pub kind: Kind,
    pub mode: Option<Mode>,
    pub file_name: Option<String>,
    pub bytes: Option<String>,
}
impl Create {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let value: Self = super::admin::parse(bytes)?;
        if value.schema_version != SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedSchema);
        }
        super::admin::revision(&value.expected_revision)?;
        match value.kind {
            Kind::Import => {
                let name = value
                    .file_name
                    .as_ref()
                    .ok_or(ProtocolError::InvalidCommand)?;
                if name.is_empty()
                    || name.len() > 256
                    || name
                        .chars()
                        .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
                    || value.mode.is_none()
                {
                    return Err(ProtocolError::InvalidCommand);
                }
                let size = super::admin::id(
                    value
                        .bytes
                        .as_deref()
                        .ok_or(ProtocolError::InvalidCommand)?,
                )?;
                if size > MAX_PACKAGE_BYTES {
                    return Err(ProtocolError::TooLarge);
                }
            }
            _ if value.file_name.is_some() || value.bytes.is_some() || value.mode.is_some() => {
                return Err(ProtocolError::InvalidCommand)
            }
            _ => {}
        }
        Ok(value)
    }
    pub fn revision(&self) -> Result<Revision, ProtocolError> {
        super::admin::revision(&self.expected_revision)
    }
    pub fn expected_bytes(&self) -> Result<u64, ProtocolError> {
        super::admin::id(self.bytes.as_deref().ok_or(ProtocolError::InvalidCommand)?)
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub school_id: String,
    pub source_revision: String,
    pub current_revision: String,
    pub document_count: String,
    pub material_count: usize,
    pub profile_media_count: usize,
    pub payload_bytes: String,
    pub database_bytes: String,
    pub material_bytes: String,
    pub profile_media_bytes: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub school_id: String,
    pub school_name: String,
    pub classes: Vec<Label>,
    pub subjects: Vec<Label>,
    pub principals: Vec<Label>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingReview {
    pub source: Catalog,
    pub target: Catalog,
    pub current_revision: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub kind: Kind,
    pub phase: Phase,
    pub can_cancel: bool,
    pub cancellation_requested: bool,
    pub expected_revision: String,
    pub file_name: Option<String>,
    pub bytes: Option<String>,
    pub sha256: Option<String>,
    pub reset_review: Option<ResetScope>,
    pub review: Option<Review>,
    pub mapping: Option<MappingReview>,
    pub error: Option<String>,
    pub restart_required: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Confirm {
    pub schema_version: u16,
    pub expected_revision: String,
    pub reviewed_data: bool,
    pub understood_replacement: bool,
}
impl Confirm {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let v: Self = super::admin::parse(bytes)?;
        if v.schema_version != SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedSchema);
        }
        super::admin::revision(&v.expected_revision)?;
        if !v.reviewed_data || !v.understood_replacement {
            return Err(ProtocolError::InvalidCommand);
        }
        Ok(v)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Pair {
    pub source: String,
    pub target: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Mapping {
    pub schema_version: u16,
    pub expected_revision: String,
    pub source_school_id: String,
    pub target_school_id: String,
    pub classes: Vec<Pair>,
    pub subjects: Vec<Pair>,
    pub principals: Vec<Pair>,
}
impl Mapping {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let v: Self = super::admin::parse(bytes)?;
        if v.schema_version != SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedSchema);
        }
        super::admin::revision(&v.expected_revision)?;
        if v.classes.len() > fc_domain::identity::MAX_CLASSES
            || v.subjects.len() > fc_domain::identity::MAX_SUBJECTS
            || v.principals.len() > fc_domain::identity::MAX_ACCOUNTS
        {
            return Err(ProtocolError::TooLarge);
        }
        Ok(v)
    }
    pub fn domain(&self) -> Result<MigrationMapping, ProtocolError> {
        let id = |v: &str| super::admin::id(v);
        Ok(MigrationMapping {
            source_school_id: SchoolId::new(id(&self.source_school_id)?)
                .map_err(|_| ProtocolError::InvalidCommand)?,
            target_school_id: SchoolId::new(id(&self.target_school_id)?)
                .map_err(|_| ProtocolError::InvalidCommand)?,
            classes: self
                .classes
                .iter()
                .map(|p| {
                    Ok((
                        ClassId::new(id(&p.source)?).map_err(|_| ProtocolError::InvalidCommand)?,
                        ClassId::new(id(&p.target)?).map_err(|_| ProtocolError::InvalidCommand)?,
                    ))
                })
                .collect::<Result<_, ProtocolError>>()?,
            subjects: self
                .subjects
                .iter()
                .map(|p| {
                    Ok((
                        SubjectId::new(id(&p.source)?)
                            .map_err(|_| ProtocolError::InvalidCommand)?,
                        SubjectId::new(id(&p.target)?)
                            .map_err(|_| ProtocolError::InvalidCommand)?,
                    ))
                })
                .collect::<Result<_, ProtocolError>>()?,
            principals: self
                .principals
                .iter()
                .map(|p| {
                    Ok((
                        PrincipalId::new(id(&p.source)?)
                            .map_err(|_| ProtocolError::InvalidCommand)?,
                        PrincipalId::new(id(&p.target)?)
                            .map_err(|_| ProtocolError::InvalidCommand)?,
                    ))
                })
                .collect::<Result<_, ProtocolError>>()?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct EmptyAction {
    schema_version: u16,
}
pub fn empty_action(bytes: &[u8]) -> Result<(), ProtocolError> {
    let value: EmptyAction = super::admin::parse(bytes)?;
    if value.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    Ok(())
}
pub fn created(revision: Revision, job: &Snapshot, proof: &str) -> Result<Vec<u8>, ProtocolError> {
    #[derive(Serialize)]
    struct Created<'a> {
        job: &'a Snapshot,
        proof: &'a str,
    }
    super::admin::encode(revision, &Created { job, proof })
}
pub fn status(revision: Revision, job: Snapshot) -> Result<Vec<u8>, ProtocolError> {
    #[derive(Serialize)]
    struct Status {
        job: Snapshot,
    }
    super::admin::encode(revision, &Status { job })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetScope {
    pub present_tree_count: usize,
    pub scope: Vec<&'static str>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_and_actions_reject_untrusted_paths_and_ambiguous_fields() {
        assert!(Create::decode(br#"{"schemaVersion":1,"expectedRevision":"0","kind":"IMPORT","mode":"ORDINARY","fileName":"C:\\secrets.fcbackup","bytes":"8"}"#).is_err());
        assert!(Create::decode(br#"{"schemaVersion":1,"expectedRevision":"0","kind":"IMPORT","mode":"ORDINARY","fileName":"backup.fcbackup","bytes":"8"}"#).is_ok());
        assert!(empty_action(br#"{"schemaVersion":1}"#).is_ok());
        assert!(empty_action(br#"{"schemaVersion":1,"path":"anything"}"#).is_err());
        assert!(empty_action(br#"{"schemaVersion":1,"schemaVersion":1}"#).is_err());
        assert!(recognize("/fc/api/v1/admin/maintenance/jobs/01/file").is_none());
        assert!(recognize("/fc/api/v1/admin/maintenance/jobs/1/file/extra").is_none());
    }
}
