//! Explicit response disclosures. Secret-bearing DTOs borrow secret text only
//! while serializing; public directory DTOs retain bounded labels and addresses.
use crate::{ProtocolError, MAX_JSON_BYTES, SCHEMA_VERSION};
use fc_domain::{auth::*, identity::UserAccount, ids::ClassId};
use serde::Serialize;
use std::io::{self, Write};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    RateLimited,
    ServiceUnavailable,
    ResultUnknown,
    CommandExpired,
    CommandNotIssued,
    CommandWindowFull,
    CommandWindowUnavailable,
    RevisionConflict,
    DayChanged,
    IdempotencyConflict,
    StaleCursor,
    UploadExpired,
    UploadNotReady,
    InvalidResource,
    Capacity,
    VoiceDisabled,
    TargetChanged,
    InvalidWav,
    QueueFull,
    OwnerLimit,
    Expired,
}
#[derive(Default)]
struct BoundedJson {
    bytes: Vec<u8>,
    limit_exceeded: bool,
}
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // Reject before reserving or copying. The whole response is private
        // until serialization succeeds, so a rejected document is never sent
        // as a partial or silently truncated assignment list.
        if bytes.len() > MAX_JSON_BYTES - self.bytes.len() {
            self.limit_exceeded = true;
            return Err(io::Error::other("JSON response byte limit"));
        }
        let length = self.bytes.len() + bytes.len();
        if length > self.bytes.capacity() {
            let capacity = length.next_power_of_two().clamp(256, MAX_JSON_BYTES);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| io::Error::other("JSON response allocation"))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    let mut output = BoundedJson::default();
    if serde_json::to_writer(&mut output, value).is_err() {
        return Err(if output.limit_exceeded {
            ProtocolError::TooLarge
        } else {
            ProtocolError::InvalidCommand
        });
    }
    Ok(output.bytes)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicStatus {
    schema_version: u16,
    api_version: u16,
    ui_compatibility_version: u16,
    online: bool,
}
pub fn public_status() -> Result<Vec<u8>, ProtocolError> {
    encode(&PublicStatus {
        schema_version: SCHEMA_VERSION,
        api_version: 1,
        ui_compatibility_version: 1,
        online: true,
    })
}

/// Only local product/version facts. No account or school identity disclosure.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About<'a> {
    pub schema_version: u16,
    pub version: &'a str,
    pub automatic_updates: bool,
    pub update_available: bool,
    pub update_message: &'a str,
}
pub fn about(value: &About<'_>) -> Result<Vec<u8>, ProtocolError> {
    encode(value)
}
/// Minimal public connection information; no account, TLS key or session data.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickConnect<'a> {
    pub schema_version: u16,
    pub school_name: &'a str,
    pub class_name: Option<&'a str>,
    pub entry_url: Option<&'a str>,
    pub unavailable_reason: Option<&'a str>,
}
pub fn quick_connect(value: &QuickConnect<'_>) -> Result<Vec<u8>, ProtocolError> {
    encode(value)
}
/// Public, unverified LAN candidates. TLS and each classroom's own login remain
/// mandatory when opening an address from this list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassroomEntry {
    pub address: String,
    pub school_name: String,
    pub class_name: Option<String>,
    pub is_current: bool,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Classrooms {
    pub schema_version: u16,
    pub discovery_available: bool,
    pub rooms: Vec<ClassroomEntry>,
}
pub fn classrooms(value: &Classrooms) -> Result<Vec<u8>, ProtocolError> {
    encode(value)
}

/// Validate a UDP candidate at the trust boundary, including its actual sender.
pub fn parse_lan_discovery(
    bytes: &[u8],
    peer: std::net::Ipv4Addr,
) -> Result<ClassroomEntry, ProtocolError> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Discovery {
        schema_version: u16,
        product: String,
        address: String,
        school_name: String,
        class_name: Option<String>,
    }
    if bytes.len() > 1024 || !peer.is_private() {
        return Err(ProtocolError::InvalidCommand);
    }
    let value: Discovery = crate::parse(bytes)?;
    let endpoint = value
        .address
        .strip_prefix("https://")
        .and_then(|address| address.strip_suffix("/fc/"))
        .and_then(|host| host.parse::<std::net::SocketAddrV4>().ok())
        .ok_or(ProtocolError::InvalidCommand)?;
    if value.schema_version != 1
        || value.product != "FutureClassroom"
        || *endpoint.ip() != peer
        || endpoint.port() == 0
        || value.school_name.len() > 512
        || value.school_name.trim().is_empty()
        || value
            .class_name
            .as_ref()
            .is_some_and(|name| name.len() > 512 || name.trim().is_empty())
        || value.school_name.chars().any(char::is_control)
        || value
            .class_name
            .as_ref()
            .is_some_and(|name| name.chars().any(char::is_control))
    {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(ClassroomEntry {
        address: format!("https://{endpoint}/fc/"),
        school_name: value.school_name,
        class_name: value.class_name,
        is_current: false,
    })
}
pub fn lan_discovery(
    address: &str,
    school_name: &str,
    class_name: Option<&str>,
) -> Result<Vec<u8>, ProtocolError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Discovery<'a> {
        schema_version: u16,
        product: &'static str,
        address: &'a str,
        school_name: &'a str,
        class_name: Option<&'a str>,
    }
    let bytes = encode(&Discovery {
        schema_version: 1,
        product: "FutureClassroom",
        address,
        school_name,
        class_name,
    })?;
    if bytes.len() > 1024 {
        return Err(ProtocolError::TooLarge);
    }
    Ok(bytes)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingRequested<'a> {
    schema_version: u16,
    status: &'static str,
    request_id: String,
    poll_secret: &'a str,
    retry_after_ms: u16,
}
pub fn pairing_requested(id: u64, poll_secret: &str) -> Result<Vec<u8>, ProtocolError> {
    encode(&PairingRequested {
        schema_version: SCHEMA_VERSION,
        status: "PENDING",
        request_id: id.to_string(),
        poll_secret,
        retry_after_ms: 500,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingPending {
    schema_version: u16,
    status: &'static str,
    retry_after_ms: u16,
}
pub fn pairing_pending() -> Result<Vec<u8>, ProtocolError> {
    encode(&PairingPending {
        schema_version: SCHEMA_VERSION,
        status: "PENDING",
        retry_after_ms: 500,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    schema_version: u16,
    status: &'static str,
}
pub fn authorized() -> Result<Vec<u8>, ProtocolError> {
    encode(&Status {
        schema_version: SCHEMA_VERSION,
        status: "AUTHORIZED",
    })
}
pub fn logged_out() -> Result<Vec<u8>, ProtocolError> {
    encode(&Status {
        schema_version: SCHEMA_VERSION,
        status: "LOGGED_OUT",
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Principal<'a> {
    id: String,
    display_name: &'a str,
    role: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Assignment {
    class_id: String,
    subject_id: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Session<'a> {
    schema_version: u16,
    status: &'static str,
    principal: Principal<'a>,
    current_class_id: Option<String>,
    assignments: Vec<Assignment>,
    csrf: &'a str,
    must_change_password: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_token: Option<&'a str>,
}
pub fn session(
    account: &UserAccount,
    current_class: Option<ClassId>,
    assignments: &[TeachingAssignment],
    csrf: &str,
) -> Result<Vec<u8>, ProtocolError> {
    page_session(account, current_class, assignments, csrf, None)
}
pub fn page_session(
    account: &UserAccount,
    current_class: Option<ClassId>,
    assignments: &[TeachingAssignment],
    csrf: &str,
    page_token: Option<&str>,
) -> Result<Vec<u8>, ProtocolError> {
    encode(&Session {
        schema_version: SCHEMA_VERSION,
        status: "AUTHENTICATED",
        principal: Principal {
            id: account.principal.id.get().to_string(),
            display_name: &account.display_name,
            role: match account.principal.role {
                Role::GlobalAdmin => "GLOBAL_ADMIN",
                Role::TeacherAdmin => "TEACHER_ADMIN",
                Role::User => "USER",
            },
        },
        current_class_id: current_class.map(|id| id.get().to_string()),
        assignments: assignments
            .iter()
            .filter(|a| a.enabled)
            .map(|a| Assignment {
                class_id: a.class_id.get().to_string(),
                subject_id: a.subject_id.get().to_string(),
            })
            .collect(),
        csrf,
        page_token,
        must_change_password: account.must_change_password,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Error {
    schema_version: u16,
    code: ErrorCode,
}
pub fn error(code: ErrorCode) -> Result<Vec<u8>, ProtocolError> {
    encode(&Error {
        schema_version: SCHEMA_VERSION,
        code,
    })
}

pub fn initialization_status(initialized: bool) -> Result<Vec<u8>, ProtocolError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Status {
        schema_version: u16,
        api_version: u16,
        ui_compatibility_version: u16,
        online: bool,
        initialized: bool,
    }
    encode(&Status {
        schema_version: SCHEMA_VERSION,
        api_version: 1,
        ui_compatibility_version: 1,
        online: true,
        initialized,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn discovery_rejects_blank_labels_and_accepts_bounded_chinese_names() {
        let address = "https://192.168.10.2:8443/fc/";
        let peer = "192.168.10.2".parse().unwrap();
        for school in ["", "  ", "　"] {
            let bytes = super::lan_discovery(address, school, None).unwrap();
            assert!(super::parse_lan_discovery(&bytes, peer).is_err());
        }
        for class in ["", "  ", "　"] {
            let bytes = super::lan_discovery(address, "学校", Some(class)).unwrap();
            assert!(super::parse_lan_discovery(&bytes, peer).is_err());
        }
        let name = "学".repeat(128);
        let bytes = super::lan_discovery(address, &name, Some("一班")).unwrap();
        assert_eq!(
            super::parse_lan_discovery(&bytes, peer)
                .unwrap()
                .school_name,
            name
        );
    }

    use super::*;
    #[test]
    fn public_response_is_an_exact_non_sensitive_field_set() {
        let value: serde_json::Value = serde_json::from_slice(&public_status().unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"schemaVersion":1,"apiVersion":1,"uiCompatibilityVersion":1,"online":true})
        );
    }
    #[test]
    fn request_ids_keep_all_64_bits_and_only_creation_discloses_poll_secret() {
        let secret = "a".repeat(64);
        let value: serde_json::Value =
            serde_json::from_slice(&pairing_requested(u64::MAX, &secret).unwrap()).unwrap();
        assert_eq!(value["requestId"], u64::MAX.to_string());
        assert_eq!(value["pollSecret"], secret);
        assert_eq!(value["retryAfterMs"], 500);
        for body in [
            pairing_pending().unwrap(),
            authorized().unwrap(),
            logged_out().unwrap(),
        ] {
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(value.get("pollSecret").is_none());
            assert!(value.get("cookie").is_none());
        }
    }
    #[test]
    fn outbound_json_exact_byte_limit_and_rejection_before_growth() {
        let exact = "a".repeat(MAX_JSON_BYTES - 2); // JSON string quotes count.
        assert_eq!(encode(&exact).unwrap().len(), MAX_JSON_BYTES);
        assert_eq!(encode(&(exact + "a")), Err(ProtocolError::TooLarge));
        let mut output = BoundedJson::default();
        output.write_all(&vec![b'a'; MAX_JSON_BYTES]).unwrap();
        let capacity = output.bytes.capacity();
        assert!(capacity <= MAX_JSON_BYTES);
        assert!(output.write_all(b"x").is_err());
        assert_eq!(output.bytes.len(), MAX_JSON_BYTES);
        assert_eq!(output.bytes.capacity(), capacity);
        assert!(output.limit_exceeded);
    }
    #[test]
    fn outbound_limit_counts_json_escaping_and_utf8_bytes() {
        let escaped = "\n".repeat((MAX_JSON_BYTES - 2) / 2);
        assert!(escaped.len() < MAX_JSON_BYTES);
        let body = encode(&escaped).unwrap();
        assert_eq!(body.len(), MAX_JSON_BYTES);
        assert_eq!(serde_json::from_slice::<String>(&body).unwrap(), escaped);
        assert_eq!(encode(&(escaped + "\n")), Err(ProtocolError::TooLarge));
        let unicode = "字".repeat((MAX_JSON_BYTES - 2) / 3) + "aa";
        assert!(unicode.chars().count() < MAX_JSON_BYTES / 2);
        assert_eq!(encode(&unicode).unwrap().len(), MAX_JSON_BYTES);
        assert_eq!(encode(&(unicode + "a")), Err(ProtocolError::TooLarge));
    }
    #[test]
    fn full_assignment_session_is_rejected_without_truncating_scope() {
        use fc_domain::{identity::MAX_ASSIGNMENTS, ids::*};
        let teacher = PrincipalId::new(1).unwrap();
        let account = UserAccount {
            principal: fc_domain::auth::Principal {
                id: teacher,
                school_id: SchoolId::new(1).unwrap(),
                role: Role::TeacherAdmin,
                enabled: true,
                auth_epoch: AuthEpoch::new(1),
            },
            login_name: "teacher".into(),
            display_name: "Synthetic teacher".into(),
            must_change_password: false,
            credential_version: 1,
            recovery_version: 1,
        };
        let assignments: Vec<_> = (0..MAX_ASSIGNMENTS)
            .map(|index| TeachingAssignment {
                teacher_id: teacher,
                class_id: ClassId::new(index as u64 / 32 + 1).unwrap(),
                subject_id: SubjectId::new(index as u64 % 32 + 1).unwrap(),
                enabled: true,
            })
            .collect();
        let csrf = "a".repeat(64);
        assert_eq!(
            session(&account, None, &assignments, &csrf),
            Err(ProtocolError::TooLarge)
        );
        let body = session(&account, None, &assignments[..1], &csrf).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value["assignments"],
            serde_json::json!([{"classId":"1","subjectId":"1"}])
        );
        assert_eq!(value["principal"]["role"], "TEACHER_ADMIN");
        assert_eq!(value["csrf"], csrf);
    }
}
