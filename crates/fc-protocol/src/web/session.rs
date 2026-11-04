//! Strict page-session inputs. Secrets deliberately have no Debug or Serialize.
use crate::{parse, ProtocolError, SCHEMA_VERSION};
use serde::Deserialize;

pub const MAX_SESSION_JSON_BYTES: usize = 4096;
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LoginInput {
    schema_version: u16,
    pub login_name: String,
    pub password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ResumeInput {
    schema_version: u16,
    pub page_token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PasswordInput {
    schema_version: u16,
    pub current_password: String,
    pub new_password: String,
}
pub fn password(bytes: &[u8]) -> Result<PasswordInput, ProtocolError> {
    if bytes.len() > MAX_SESSION_JSON_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let dto: PasswordInput = parse(bytes)?;
    if dto.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    for value in [&dto.current_password, &dto.new_password] {
        if !(12..=128).contains(&value.chars().count()) || value.len() > 512 {
            return Err(ProtocolError::InvalidCommand);
        }
    }
    Ok(dto)
}
pub fn login(bytes: &[u8]) -> Result<LoginInput, ProtocolError> {
    if bytes.len() > MAX_SESSION_JSON_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let dto: LoginInput = parse(bytes)?;
    if dto.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    if dto.login_name.is_empty()
        || dto.login_name.chars().count() > 64
        || dto.login_name.chars().any(char::is_control)
        || dto.login_name.trim().is_empty()
        || !(12..=128).contains(&dto.password.chars().count())
        || dto.password.len() > 512
    {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(dto)
}
pub fn resume(bytes: &[u8]) -> Result<ResumeInput, ProtocolError> {
    if bytes.len() > MAX_SESSION_JSON_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let dto: ResumeInput = parse(bytes)?;
    if dto.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    if dto.page_token.len() != 128
        || !dto
            .page_token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(dto)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_login_is_strict_and_client_cannot_supply_authority() {
        let input =
            br#"{"schemaVersion":1,"loginName":"admin","password":"Synthetic-password-2026"}"#;
        assert_eq!(login(input).unwrap().login_name, "admin");
        for extra in [
            r#", "role":"GLOBAL_ADMIN""#,
            r#", "loginName":"teacher""#,
            r#", "schemaVersion":1"#,
        ] {
            let mut body = input[..input.len() - 1].to_vec();
            body.extend_from_slice(extra.as_bytes());
            body.push(b'}');
            assert!(login(&body).is_err());
        }
        assert!(login(
            br#"{"schemaVersion":2,"loginName":"admin","password":"Synthetic-password-2026"}"#
        )
        .is_err());
        assert!(resume(br#"{"schemaVersion":1,"pageToken":"short"}"#).is_err());
        assert!(login(&vec![b' '; MAX_SESSION_JSON_BYTES + 1]).is_err());
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetupInput {
    schema_version: u16,
    pub school_name: String,
    pub login_name: String,
    pub display_name: String,
    pub password: String,
}
pub fn setup(bytes: &[u8]) -> Result<SetupInput, ProtocolError> {
    if bytes.len() > MAX_SESSION_JSON_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let dto: SetupInput = parse(bytes)?;
    if dto.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    for value in [&dto.school_name, &dto.display_name, &dto.login_name] {
        if value.trim().is_empty()
            || value.chars().count() > 64
            || value.chars().any(char::is_control)
        {
            return Err(ProtocolError::InvalidCommand);
        }
    }
    if !(12..=128).contains(&dto.password.chars().count()) || dto.password.len() > 512 {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(dto)
}
