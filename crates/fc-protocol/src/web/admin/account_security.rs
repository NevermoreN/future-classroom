use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Input {
    pub schema_version: u16,
    pub action: String,
    pub expected_revision: Option<String>,
    pub target_id: Option<String>,
    pub ticket: Option<String>,
    pub current_password: Option<String>,
    pub reviewed_target: Option<bool>,
    pub understood_revocation: Option<bool>,
}
pub fn input(bytes: &[u8]) -> Result<Input, ProtocolError> {
    if bytes.len() > 4096 {
        return Err(ProtocolError::TooLarge);
    }
    let value: Input = parse(bytes)?;
    if value.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    Ok(value)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Recover {
    pub schema_version: u16,
    pub login_name: String,
    pub recovery_code: String,
    pub new_password: String,
}
pub fn recover(bytes: &[u8]) -> Result<Recover, ProtocolError> {
    if bytes.len() > 4096 {
        return Err(ProtocolError::TooLarge);
    }
    let v: Recover = parse(bytes)?;
    if v.schema_version != SCHEMA_VERSION
        || v.login_name.is_empty()
        || v.login_name.chars().count() > 64
        || v.login_name.chars().any(char::is_control)
        || v.recovery_code.len() != 68
        || !(12..=128).contains(&v.new_password.chars().count())
        || v.new_password.len() > 512
    {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(v)
}
pub fn code(revision: Revision, code: &str) -> Result<Vec<u8>, ProtocolError> {
    encode(revision, &serde_json::json!({"recoveryCode":code}))
}
pub fn ticket(revision: Revision, ticket: &str) -> Result<Vec<u8>, ProtocolError> {
    encode(revision, &serde_json::json!({"ticket":ticket}))
}
pub fn outcome(
    revision: Revision,
    state: &str,
    original: Option<Revision>,
) -> Result<Vec<u8>, ProtocolError> {
    encode(
        revision,
        &serde_json::json!({"state":state,"revision":original.map(|v|v.get().to_string())}),
    )
}
pub fn session_code(bytes: &[u8], code: &str) -> Result<Vec<u8>, ProtocolError> {
    let mut v: serde_json::Value = parse(bytes)?;
    v["recoveryCode"] = serde_json::json!(code);
    serde_json::to_vec(&v).map_err(|_| ProtocolError::InvalidJson)
}
