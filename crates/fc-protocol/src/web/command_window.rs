//! Explicit v2 command boundary. A decoded pair is requested identity only;
//! issuance, owner/family binding, expiry and original business permission stay
//! on the server's sole Writer and authenticated transport.
use crate::{parse, ProtocolError};
use fc_domain::ids::CommandId;
use fc_ports::command_window::{
    CommandFamily, CommandTicket, IssuedCommand, COMMAND_WINDOW_MILLIS,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandReference {
    pub ticket: CommandTicket,
    pub command_id: CommandId,
}
impl CommandReference {
    pub fn path(value: &str) -> Option<Self> {
        if value.len() > 76 {
            return None;
        }
        let (ticket, id) = value.split_once('/')?;
        Some(Self {
            ticket: CommandTicket::parse(ticket).ok()?,
            command_id: CommandId::new(super::decimal(id, false)?).ok()?,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct IssueDto {
    schema_version: u16,
    family: String,
}
pub fn issue(bytes: &[u8]) -> Result<CommandFamily, ProtocolError> {
    if bytes.len() > 128 {
        return Err(ProtocolError::TooLarge);
    }
    let dto: IssueDto = parse(bytes)?;
    if dto.schema_version != 2 {
        return Err(ProtocolError::UnsupportedSchema);
    }
    match dto.family.as_str() {
        "content" => Ok(CommandFamily::Content),
        "materials" => Ok(CommandFamily::Materials),
        "voice" => Ok(CommandFamily::Voice),
        "updates" => Ok(CommandFamily::Updates),
        _ => Err(ProtocolError::InvalidCommand),
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IssueReply {
    schema_version: u16,
    status: &'static str,
    family: &'static str,
    command_id: String,
    command_ticket: String,
    expires_in_millis: u64,
}
pub fn issued(command: IssuedCommand) -> Result<Vec<u8>, ProtocolError> {
    if !matches!(
        command.family,
        CommandFamily::Content
            | CommandFamily::Materials
            | CommandFamily::Voice
            | CommandFamily::Updates
    ) || command.window_millis != COMMAND_WINDOW_MILLIS
    {
        return Err(ProtocolError::InvalidCommand);
    }
    super::responses::encode(&IssueReply {
        schema_version: 2,
        status: "ISSUED",
        family: command.family.name(),
        command_id: command.command_id.get().to_string(),
        command_ticket: command.ticket.to_string(),
        expires_in_millis: COMMAND_WINDOW_MILLIS,
    })
}
/// Flat mutation DTOs retain all historic business fields. Schema1 cannot
/// smuggle a v2 ticket and schema2 cannot omit its explicit namespace.
pub(super) fn provided_ticket<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}
pub(super) fn ticket(
    version: u16,
    text: Option<&str>,
) -> Result<Option<CommandTicket>, ProtocolError> {
    match (version, text) {
        (1, None) => Ok(None),
        (2, Some(text)) => CommandTicket::parse(text)
            .map(Some)
            .map_err(|_| ProtocolError::InvalidCommand),
        (1 | 2, _) => Err(ProtocolError::InvalidCommand),
        _ => Err(ProtocolError::UnsupportedSchema),
    }
}
/// Upgrade an already bounded trusted command response, not an arbitrary
/// request. Original domain payload/fingerprint serialization is untouched.
pub fn bind_response(bytes: &[u8], command: CommandReference) -> Result<Vec<u8>, ProtocolError> {
    if bytes.len() > crate::MAX_JSON_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| ProtocolError::InvalidCommand)?;
    let object = value.as_object_mut().ok_or(ProtocolError::InvalidCommand)?;
    if object
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
        || object.get("commandId").and_then(serde_json::Value::as_str)
            != Some(command.command_id.get().to_string().as_str())
        || !matches!(
            object.get("status").and_then(serde_json::Value::as_str),
            Some("COMMITTED" | "COMMAND_RESULT" | "CANCEL_REQUESTED")
        )
        || object.contains_key("commandTicket")
    {
        return Err(ProtocolError::InvalidCommand);
    }
    object.insert("schemaVersion".into(), serde_json::json!(2));
    object.insert(
        "commandTicket".into(),
        serde_json::json!(command.ticket.to_string()),
    );
    super::responses::encode(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference() -> CommandReference {
        CommandReference {
            ticket: CommandTicket::new([7; 16], 1).unwrap(),
            command_id: CommandId::new(u64::MAX).unwrap(),
        }
    }
    #[test]
    fn issue_has_closed_family_version_and_exact_fields() {
        assert_eq!(
            issue(br#"{"schemaVersion":2,"family":"content"}"#),
            Ok(CommandFamily::Content)
        );
        for input in [
            br#"{"schemaVersion":2,"family":"identity"}"#.as_slice(),
            br#"{"schemaVersion":2,"family":"Content"}"#,
            br#"{"schemaVersion":1,"family":"content"}"#,
            br#"{"schemaVersion":2,"family":"content","commandId":"1"}"#,
            br#"{"schemaVersion":2,"family":"content","family":"voice"}"#,
        ] {
            assert!(issue(input).is_err());
        }
    }
    #[test]
    fn full_path_has_no_numeric_fallback_or_normalization() {
        let reference = reference();
        assert_eq!(
            CommandReference::path(&format!(
                "{}/{}",
                reference.ticket,
                reference.command_id.get()
            )),
            Some(reference)
        );
        for path in [
            "1".to_owned(),
            format!("{}/01", reference.ticket),
            format!("{}/1/2", reference.ticket),
            format!("{}/0", reference.ticket),
            format!("{}/1?x=1", reference.ticket),
            format!("{}/%31", reference.ticket),
        ] {
            assert!(CommandReference::path(&path).is_none());
        }
    }
    #[test]
    fn mutation_version_and_ticket_cannot_be_mixed() {
        let text = reference().ticket.to_string();
        assert_eq!(ticket(1, None), Ok(None));
        assert_eq!(ticket(2, Some(&text)), Ok(Some(reference().ticket)));
        assert!(ticket(1, Some(&text)).is_err());
        assert!(ticket(2, None).is_err());
        assert!(ticket(2, Some("1")).is_err());
    }
    #[test]
    fn response_binding_checks_original_identity_and_does_not_change_business_data() {
        let command = reference();
        let source = serde_json::json!({"schemaVersion":1,"status":"COMMITTED","commandId":command.command_id.get().to_string(),"content":{"text":"original","revision":"7"}});
        let bound = bind_response(&serde_json::to_vec(&source).unwrap(), command).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bound).unwrap();
        assert_eq!(value["content"], source["content"]);
        assert_eq!(value["commandTicket"], command.ticket.to_string());
        assert_eq!(value["schemaVersion"], 2);
        let wrong = CommandReference {
            command_id: CommandId::new(1).unwrap(),
            ..command
        };
        assert!(bind_response(&serde_json::to_vec(&source).unwrap(), wrong).is_err());
        assert!(bind_response(&bound, command).is_err());
    }
}
