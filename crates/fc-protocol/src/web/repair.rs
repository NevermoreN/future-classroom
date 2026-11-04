//! Explicit operating-system initiated repair, separate from administrator login.
use super::admin;
use crate::{ProtocolError, SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
pub const BASE: &str = "/fc/api/v1/repair/";
pub const MAX_BODY: usize = 4096;
pub const SESSION_HEADER: &str = "x-fc-repair-session";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Session,
    Status,
    Review,
    Confirm,
    Cancel,
    Exit,
}
pub fn recognize(path: &str) -> Option<Resource> {
    Some(match path.strip_prefix(BASE)? {
        "session" => Resource::Session,
        "status" => Resource::Status,
        "review" => Resource::Review,
        "confirm" => Resource::Confirm,
        "cancel" => Resource::Cancel,
        "exit" => Resource::Exit,
        _ => return None,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Exchange {
    schema_version: u16,
    pub capability: String,
}
impl Exchange {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let v: Self = admin::parse(bytes)?;
        if bytes.len() > MAX_BODY {
            return Err(ProtocolError::TooLarge);
        }
        if v.schema_version != SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedSchema);
        }
        Ok(v)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Confirm {
    schema_version: u16,
    pub sequence: String,
    pub reviewed_scope: bool,
    pub understood_deletion: bool,
    pub understood_new_identity: bool,
}
impl Confirm {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let v: Self = admin::parse(bytes)?;
        if bytes.len() > MAX_BODY {
            return Err(ProtocolError::TooLarge);
        }
        if v.schema_version != SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedSchema);
        }
        admin::id(&v.sequence)?;
        if !v.reviewed_scope || !v.understood_deletion || !v.understood_new_identity {
            return Err(ProtocolError::InvalidCommand);
        }
        Ok(v)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Phase {
    Initializing,
    Ready,
    ReviewReady,
    Preparing,
    Draining,
    Committing,
    Retained,
    Finished,
    Cancelled,
    Failed,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub sequence: String,
    pub phase: Phase,
    pub scope: Option<super::maintenance::ResetScope>,
    pub can_cancel: bool,
    pub cancellation_requested: bool,
    pub error: Option<String>,
    pub succeeded: Option<bool>,
}
pub fn status(value: &Status) -> Result<Vec<u8>, ProtocolError> {
    #[derive(Serialize)]
    struct Reply<'a> {
        status: &'a Status,
    }
    admin::encode(Default::default(), &Reply { status: value })
}
pub fn session(token: &str, csrf: &str) -> Result<Vec<u8>, ProtocolError> {
    #[derive(Serialize)]
    struct Reply<'a> {
        session: &'a str,
        csrf: &'a str,
    }
    admin::encode(
        Default::default(),
        &Reply {
            session: token,
            csrf,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repair_requests_require_exact_scope_confirmation_and_no_unknown_fields() {
        assert!(
            Exchange::decode(br#"{"schemaVersion":1,"capability":"test","admin":true}"#).is_err()
        );
        assert!(
            Exchange::decode(br#"{"schemaVersion":1,"capability":"a","capability":"b"}"#).is_err()
        );
        assert!(Confirm::decode(br#"{"schemaVersion":1,"sequence":"42","reviewedScope":true,"understoodDeletion":true,"understoodNewIdentity":false}"#).is_err());
        assert!(Confirm::decode(br#"{"schemaVersion":1,"sequence":"42","reviewedScope":true,"understoodDeletion":true,"understoodNewIdentity":true}"#).is_ok());
        assert!(recognize("/fc/api/v1/repair/confirm/extra").is_none());
    }
}
