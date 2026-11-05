//! Bounded update management commands. These requests convey local intent only;
//! publisher verification and native installation admission stay independent.
use super::{command_window::CommandReference, responses};
use crate::{parse, ProtocolError};
use fc_domain::{
    ids::{CommandId, Revision},
    update_state::{UpdateInstallMode, UpdateMaintenanceState},
};
use fc_ports::command_window::CommandTicket;
use serde::{Deserialize, Serialize};

pub const MAX_IMPORT_BYTES: u64 = 128 * 1024 * 1024;
pub const CHUNK_BYTES: u64 = 65536;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Status,
    Policy,
    Check,
    GroupCreate,
    GroupJoin,
    GroupExport,
    ImportBegin,
    ImportBytes { token: [u8; 16], offset: u64 },
    ImportFinish { token: [u8; 16] },
    ImportCancel { token: [u8; 16] },
}
impl Resource {
    pub fn post(self) -> bool {
        !matches!(self, Self::Status | Self::GroupExport)
    }
    pub fn body_limit(self) -> u64 {
        if matches!(self, Self::ImportBytes { .. }) {
            CHUNK_BYTES
        } else if self.post() {
            4096
        } else {
            0
        }
    }
}
pub fn token(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    {
        return None;
    }
    let mut result = [0; 16];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    (result != [0; 16]).then_some(result)
}
pub fn token_text(token: [u8; 16]) -> String {
    token.iter().map(|v| format!("{v:02x}")).collect()
}
pub fn route(path: &str) -> Option<Resource> {
    Some(match path {
        "updates/status" => Resource::Status,
        "updates/policy" => Resource::Policy,
        "updates/check" => Resource::Check,
        "updates/group/create" => Resource::GroupCreate,
        "updates/group/join" => Resource::GroupJoin,
        "updates/group/export" => Resource::GroupExport,
        "updates/import/begin" => Resource::ImportBegin,
        _ => {
            let (id, tail) = path.strip_prefix("updates/import/")?.split_once('/')?;
            let token = token(id)?;
            match tail {
                "finish" => Resource::ImportFinish { token },
                "cancel" => Resource::ImportCancel { token },
                _ => {
                    let offset = super::decimal(tail.strip_prefix("bytes/")?, true)?;
                    if offset >= MAX_IMPORT_BYTES {
                        return None;
                    }
                    Resource::ImportBytes { token, offset }
                }
            }
        }
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Command {
    schema_version: u16,
    command_id: String,
    command_ticket: String,
    pub expected_revision: String,
}
impl Command {
    pub fn checked(&self) -> Result<(CommandReference, Revision), ProtocolError> {
        if self.schema_version != 2 {
            return Err(ProtocolError::UnsupportedSchema);
        }
        Ok((
            CommandReference {
                command_id: CommandId::new(
                    super::decimal(&self.command_id, false).ok_or(ProtocolError::InvalidCommand)?,
                )
                .map_err(|_| ProtocolError::InvalidCommand)?,
                ticket: CommandTicket::parse(&self.command_ticket)
                    .map_err(|_| ProtocolError::InvalidCommand)?,
            },
            Revision::new(
                super::decimal(&self.expected_revision, true)
                    .ok_or(ProtocolError::InvalidCommand)?,
            ),
        ))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PolicyDto {
    schema_version: u16,
    command_id: String,
    command_ticket: String,
    expected_revision: String,
    enabled: bool,
    download: bool,
    share: bool,
    install_mode: String,
}
pub struct PolicyChange {
    pub command: CommandReference,
    pub expected_revision: Revision,
    pub enabled: bool,
    pub download: bool,
    pub share: bool,
    pub install_mode: UpdateInstallMode,
}
pub fn policy(bytes: &[u8]) -> Result<PolicyChange, ProtocolError> {
    let v: PolicyDto = parse(bytes)?;
    let (command, expected_revision) = Command {
        schema_version: v.schema_version,
        command_id: v.command_id,
        command_ticket: v.command_ticket,
        expected_revision: v.expected_revision,
    }
    .checked()?;
    let install_mode = match v.install_mode.as_str() {
        "automatic" => UpdateInstallMode::AutomaticAfterDownload,
        "manual" => UpdateInstallMode::ManualInstall,
        "notify" => UpdateInstallMode::NotifyOnly,
        "maintenance" => UpdateInstallMode::MaintenanceWindow,
        _ => return Err(ProtocolError::InvalidCommand),
    };
    if !v.enabled && (v.download || v.share) {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(PolicyChange {
        command,
        expected_revision,
        enabled: v.enabled,
        download: v.download,
        share: v.share,
        install_mode,
    })
}
pub fn command(bytes: &[u8]) -> Result<(CommandReference, Revision), ProtocolError> {
    let v: Command = parse(bytes)?;
    v.checked()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct JoinDto {
    schema_version: u16,
    command_id: String,
    command_ticket: String,
    expected_revision: String,
    group: String,
}
pub fn join(bytes: &[u8]) -> Result<(CommandReference, Revision, String), ProtocolError> {
    let v: JoinDto = parse(bytes)?;
    let (c, r) = Command {
        schema_version: v.schema_version,
        command_id: v.command_id,
        command_ticket: v.command_ticket,
        expected_revision: v.expected_revision,
    }
    .checked()?;
    if v.group.len() > 160
        || !v.group.starts_with("fc-update-group-v1:")
        || !v.group.is_ascii()
        || v.group.bytes().any(|v| v <= b' ' || v == 127)
    {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok((c, r, v.group))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ImportDto {
    schema_version: u16,
    command_id: String,
    command_ticket: String,
    expected_revision: String,
    length: u64,
}
pub fn import(bytes: &[u8]) -> Result<(CommandReference, Revision, u64), ProtocolError> {
    let v: ImportDto = parse(bytes)?;
    let (c, r) = Command {
        schema_version: v.schema_version,
        command_id: v.command_id,
        command_ticket: v.command_ticket,
        expected_revision: v.expected_revision,
    }
    .checked()?;
    if v.length == 0 || v.length > MAX_IMPORT_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    Ok((c, r, v.length))
}
pub fn install_mode(mode: UpdateInstallMode) -> &'static str {
    match mode {
        UpdateInstallMode::AutomaticAfterDownload => "automatic",
        UpdateInstallMode::ManualInstall => "manual",
        UpdateInstallMode::NotifyOnly => "notify",
        UpdateInstallMode::MaintenanceWindow => "maintenance",
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status<'a> {
    pub schema_version: u16,
    pub version: &'a str,
    pub automatic_updates: bool,
    pub update_available: bool,
    pub update_message: &'a str,
    pub action_token: &'a str,
    pub phase: &'a str,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub group_id: Option<&'a str>,
    pub revision: String,
    pub policy_revision: String,
    pub enabled: bool,
    pub download: bool,
    pub share: bool,
    pub install_mode: &'static str,
}
pub fn status(
    state: &UpdateMaintenanceState,
    runtime: super::responses::About<'_>,
) -> Result<Vec<u8>, ProtocolError> {
    responses::encode(&Status {
        schema_version: 1,
        version: runtime.version,
        automatic_updates: runtime.automatic_updates,
        update_available: runtime.update_available,
        update_message: runtime.update_message,
        action_token: runtime.action_token,
        phase: runtime.phase,
        received_bytes: runtime.received_bytes,
        total_bytes: runtime.total_bytes,
        group_id: runtime.group_id,
        revision: state.revision.get().to_string(),
        policy_revision: state.policy.revision.get().to_string(),
        enabled: state.policy.enabled,
        download: state.policy.download,
        share: state.policy.share,
        install_mode: install_mode(state.policy.install_mode),
    })
}

pub fn group_export(group: &str) -> Result<Vec<u8>, ProtocolError> {
    if group.len() > 160 || !group.starts_with("fc-update-group-v1:") {
        return Err(ProtocolError::InvalidCommand);
    }
    responses::encode(&serde_json::json!({"schemaVersion":1,"group":group}))
}
pub fn upload_begin(value: &str) -> Result<Vec<u8>, ProtocolError> {
    if token(value).is_none() {
        return Err(ProtocolError::InvalidCommand);
    }
    responses::encode(&serde_json::json!({"schemaVersion":1,"uploadToken":value}))
}
pub fn uploaded(received: u64) -> Result<Vec<u8>, ProtocolError> {
    if received > MAX_IMPORT_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    responses::encode(&serde_json::json!({"schemaVersion":1,"receivedBytes":received}))
}
pub fn queued() -> Result<Vec<u8>, ProtocolError> {
    responses::encode(&serde_json::json!({"schemaVersion":1,"status":"QUEUED"}))
}
pub fn cancelled() -> Result<Vec<u8>, ProtocolError> {
    responses::encode(&serde_json::json!({"schemaVersion":1,"status":"CANCELLED"}))
}
