use super::*;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: String,
    pub name: String,
    pub primary: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub label: String,
    pub value: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub application_version: String,
    pub school_name: Option<String>,
    pub current_class_id: Option<String>,
    pub desktop_state: String,
    pub listener_state: String,
    pub storage_state: String,
    pub monitors: Vec<Monitor>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub certificate_thumbprint: Option<String>,
    pub source_networks: Vec<String>,
    pub listener_state: String,
    pub local_management_url: String,
    pub teacher_url: Option<String>,
    pub certificate_trusted: Option<bool>,
    pub certificate_expires_at: Option<String>,
    pub certificate_public_pem: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Autostart {
    pub desired_enabled: bool,
    pub actual_state: String,
    pub last_error: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SaveNetwork {
    pub host: String,
    pub port: u16,
    pub certificate_thumbprint: String,
    pub source_networks: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GenerateCertificate {
    pub host: String,
    pub port: u16,
    pub source_networks: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enabled {
    enabled: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
pub enum NetworkChange {
    Save(SaveNetwork),
    Generate(GenerateCertificate),
    Start,
    Stop,
}
pub fn network_mutation(bytes: &[u8]) -> Result<(Revision, NetworkChange), ProtocolError> {
    let request = Mutation::decode(bytes)?;
    let expected = revision(&request.expected_revision)?;
    let change = match request.action.as_str() {
        "save" => NetworkChange::Save(request.candidate::<SaveNetwork>()?.1),
        "generateCertificate" => {
            NetworkChange::Generate(request.candidate::<GenerateCertificate>()?.1)
        }
        "start" => {
            request.candidate::<Empty>()?;
            NetworkChange::Start
        }
        "stop" => {
            request.candidate::<Empty>()?;
            NetworkChange::Stop
        }
        _ => return Err(ProtocolError::InvalidCommand),
    };
    Ok((expected, change))
}
pub enum AutostartChange {
    Set(bool),
    Repair,
}
pub fn autostart_mutation(bytes: &[u8]) -> Result<(Revision, AutostartChange), ProtocolError> {
    let request = Mutation::decode(bytes)?;
    let expected = revision(&request.expected_revision)?;
    let change = match request.action.as_str() {
        "set" => AutostartChange::Set(request.candidate::<Enabled>()?.1.enabled),
        "repair" => {
            request.candidate::<Empty>()?;
            AutostartChange::Repair
        }
        _ => return Err(ProtocolError::InvalidCommand),
    };
    Ok((expected, change))
}
