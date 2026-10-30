//! Explicit response disclosures. Secret text is borrowed only while serializing
//! the outbound body; these DTOs have no Debug, Deserialize, or retained state.
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
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
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
}
pub fn session(
    account: &UserAccount,
    current_class: Option<ClassId>,
    assignments: &[TeachingAssignment],
    csrf: &str,
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

#[cfg(test)]
mod tests {
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
