//! Strict inputs for pairing bootstrap. These values express only a request;
//! account lookup, approval, expiry and secret verification belong to the host.
use crate::{parse, ProtocolError, SCHEMA_VERSION};
use serde::Deserialize;

pub const MAX_PAIRING_JSON_BYTES: usize = 4096;
pub const MAX_LOGIN_NAME_CHARS: usize = 64;
pub const MAX_DEVICE_NAME_CHARS: usize = 64;

/// Deliberately has no Debug: the one-time pairing code must not enter logs.
pub struct CreateInput {
    pub code: String,
    pub login_name: String,
    pub device_name: String,
}
/// Deliberately has no Debug or serialization: this 256-bit bearer secret is
/// accepted only in a POST body, never a URL, response mirror, or log record.
pub struct PollInput {
    pub poll_secret: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CreateDto {
    schema_version: u16,
    code: String,
    login_name: String,
    device_name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PollDto {
    schema_version: u16,
    poll_secret: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LogoutDto {
    schema_version: u16,
}
fn bounded<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ProtocolError> {
    if bytes.len() > MAX_PAIRING_JSON_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    parse(bytes)
}
fn schema(version: u16) -> Result<(), ProtocolError> {
    if version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    Ok(())
}
fn valid_name(value: &str, max_chars: usize) -> bool {
    value.len() <= max_chars * 4
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control)
        && value.chars().any(|c| !c.is_whitespace())
}
pub fn create(bytes: &[u8]) -> Result<CreateInput, ProtocolError> {
    let dto: CreateDto = bounded(bytes)?;
    schema(dto.schema_version)?;
    if dto.code.len() != 8
        || !dto.code.bytes().all(|b| b.is_ascii_digit())
        || !valid_name(&dto.login_name, MAX_LOGIN_NAME_CHARS)
        || !valid_name(&dto.device_name, MAX_DEVICE_NAME_CHARS)
    {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(CreateInput {
        code: dto.code,
        login_name: dto.login_name,
        device_name: dto.device_name,
    })
}
pub fn poll(bytes: &[u8]) -> Result<PollInput, ProtocolError> {
    let dto: PollDto = bounded(bytes)?;
    schema(dto.schema_version)?;
    if dto.poll_secret.len() != 64
        || !dto
            .poll_secret
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(PollInput {
        poll_secret: dto.poll_secret,
    })
}
pub fn logout(bytes: &[u8]) -> Result<(), ProtocolError> {
    let dto: LogoutDto = bounded(bytes)?;
    schema(dto.schema_version)
}

#[cfg(test)]
mod tests {
    use super::*;
    const CREATE: &str = r#"{"schemaVersion":1,"code":"01234567","loginName":"张老师","deviceName":"Office tablet"}"#;
    #[test]
    fn pairing_inputs_preserve_spelling_without_claiming_authority() {
        let input = create(CREATE.as_bytes()).unwrap();
        assert_eq!(input.code, "01234567");
        assert_eq!(input.login_name, "张老师");
        assert_eq!(input.device_name, "Office tablet");
        let secret = "0123456789abcdef".repeat(4);
        let input =
            poll(format!(r#"{{"schemaVersion":1,"pollSecret":"{secret}"}}"#).as_bytes()).unwrap();
        assert_eq!(input.poll_secret, secret);
        assert!(logout(br#"{"schemaVersion":1}"#).is_ok());
    }
    #[test]
    fn exact_schema_duplicate_and_unknown_fields_are_rejected() {
        for field in [
            r#""actor":{}"#,
            r#""role":"GLOBAL_ADMIN""#,
            r#""scopes":[]"#,
            r#""principalId":"1""#,
            r#""code":"01234567""#,
            r#""co\u0064e":"01234567""#,
        ] {
            let input = format!("{},{} }}", &CREATE[..CREATE.len() - 1], field);
            assert!(create(input.as_bytes()).is_err(), "{field}");
        }
        assert!(matches!(
            create(CREATE.replace("Version\":1", "Version\":2").as_bytes()),
            Err(ProtocolError::UnsupportedSchema)
        ));
        assert!(poll(br#"{"schemaVersion":1,"pollSecret":"aaaa","requestId":"1"}"#).is_err());
        assert!(logout(br#"{"schemaVersion":1,"principalId":"2"}"#).is_err());
        assert!(logout(br#"{"schemaVersion":1,"schemaVersion":1}"#).is_err());
        assert!(create(&vec![b' '; MAX_PAIRING_JSON_BYTES + 1]).is_err());
        assert!(poll(&vec![b' '; MAX_PAIRING_JSON_BYTES + 1]).is_err());
    }
    #[test]
    fn codes_secrets_and_names_have_independent_strict_bounds() {
        for code in [
            "1234567",
            "123456789",
            "1234567x",
            "１２３４５６７８",
            "+1234567",
            "1234567 ",
        ] {
            assert!(create(CREATE.replace("01234567", code).as_bytes()).is_err());
        }
        for name in ["".into(), " ".into(), "a".repeat(65), "字".repeat(65)] {
            assert!(create(CREATE.replace("张老师", &name).as_bytes()).is_err());
            assert!(create(CREATE.replace("Office tablet", &name).as_bytes()).is_err());
        }
        for control in [r#"\n"#, r#"\u0000"#, r#"\u007f"#] {
            assert!(create(CREATE.replace("张老师", control).as_bytes()).is_err());
            assert!(create(CREATE.replace("Office tablet", control).as_bytes()).is_err());
        }
        for secret in [
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            "é".repeat(32),
        ] {
            assert!(
                poll(format!(r#"{{"schemaVersion":1,"pollSecret":"{secret}"}}"#).as_bytes())
                    .is_err()
            );
        }
    }
}
