//! Product-specific `custom` of a TUF target, parsed only after TUF validation.
//! Parsing these claims is not proof of their signature or installation authority.
use fc_domain::updates::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Descriptor {
    schema_version: u16,
    release_id: String,
    product: String,
    channel: String,
    release_sequence: u64,
    version: String,
    minimum_windows: Windows,
    architecture: Architecture,
    installer_abi: u16,
    guard_capability: u16,
    minimum_coordinator: String,
    source_schema: Range,
    source_protocol: Range,
    allowed_predecessors: Vec<Predecessor>,
}
#[derive(Deserialize)]
enum Windows {
    Windows7Sp1,
    Windows10,
    Windows11,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Architecture {
    X86,
    X64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Range {
    first: u32,
    last: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Predecessor {
    product_code: String,
    package_code: String,
    source_commit: String,
    source_manifest_sha256: String,
    producer_sha256: String,
    cached_msi_sha256: String,
    payload_envelope_sha256: String,
    template_sha256: String,
    guard_sha256: String,
    logical_sha256: String,
    template_bytes: u32,
    installer_abi: u16,
    guard_capability: u16,
}

#[derive(Clone, Debug)]
pub struct ReleaseClaims {
    pub release_id: String,
    pub product: String,
    pub channel: String,
    pub identity: ReleaseIdentity,
    pub version: ReleaseVersion,
    pub payload_bytes: u64,
    pub minimum_windows: WindowsBaseline,
    pub architecture: UpdateArchitecture,
    pub minimum_coordinator: ReleaseVersion,
    pub source_schema: CompatibilityRange,
    pub source_protocol: CompatibilityRange,
    pub allowed_predecessors: Vec<ExactPredecessor>,
}
fn hex<const N: usize>(value: &str) -> Result<[u8; N], InvalidUpdateValue> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(InvalidUpdateValue);
    }
    let mut out = [0; N];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| InvalidUpdateValue)?;
    }
    if out == [0; N] {
        return Err(InvalidUpdateValue);
    }
    Ok(out)
}
fn guid(value: &str) -> Result<[u8; 16], InvalidUpdateValue> {
    if value.len() != 36
        || [8, 13, 18, 23]
            .into_iter()
            .any(|i| value.as_bytes()[i] != b'-')
    {
        return Err(InvalidUpdateValue);
    }
    // Canonical text order, not Windows GUID's mixed-endian memory layout.
    hex(&value.replace('-', ""))
}
fn label(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
impl ReleaseClaims {
    /// `payload_sha256` and length must come from the validated TUF TargetFile,
    /// never duplicate unsigned envelope/catalog fields.
    pub fn from_verified_custom(
        custom: &[u8],
        payload_sha256: [u8; 32],
        payload_bytes: u64,
    ) -> Result<Self, InvalidUpdateValue> {
        let d: Descriptor = crate::parse(custom).map_err(|_| InvalidUpdateValue)?;
        if d.schema_version != 1
            || d.product != "FutureClassroom"
            || !label(&d.release_id, 80)
            || !label(&d.channel, 32)
            || payload_sha256 == [0; 32]
            || !(1..=128 * 1024 * 1024).contains(&payload_bytes)
            || d.installer_abi != UPDATE_INSTALLER_ABI
            || d.guard_capability != UPDATE_GUARD_CAPABILITY
            || d.allowed_predecessors.is_empty()
            || d.allowed_predecessors.len() > 16
        {
            return Err(InvalidUpdateValue);
        }
        let mut predecessors = Vec::with_capacity(d.allowed_predecessors.len());
        for p in d.allowed_predecessors {
            if p.installer_abi != UPDATE_INSTALLER_ABI
                || p.guard_capability != UPDATE_GUARD_CAPABILITY
                // ABI2's native lower bound: 132-byte trailer + DOS header.
                || !(196..=64 * 1024 * 1024).contains(&p.template_bytes)
            {
                return Err(InvalidUpdateValue);
            }
            let p = ExactPredecessor {
                product_code: guid(&p.product_code)?,
                package_code: guid(&p.package_code)?,
                source_commit: hex(&p.source_commit)?,
                source_manifest_sha256: hex(&p.source_manifest_sha256)?,
                producer_sha256: hex(&p.producer_sha256)?,
                cached_msi_sha256: hex(&p.cached_msi_sha256)?,
                payload_envelope_sha256: hex(&p.payload_envelope_sha256)?,
                template_sha256: hex(&p.template_sha256)?,
                guard_sha256: hex(&p.guard_sha256)?,
                logical_sha256: hex(&p.logical_sha256)?,
                template_bytes: p.template_bytes,
                installer_abi: p.installer_abi,
                guard_capability: p.guard_capability,
            };
            if predecessors.contains(&p) {
                return Err(InvalidUpdateValue);
            }
            predecessors.push(p);
        }
        Ok(Self {
            release_id: d.release_id,
            product: d.product,
            channel: d.channel,
            identity: ReleaseIdentity {
                sequence: ReleaseSequence::new(d.release_sequence)?,
                payload_sha256,
            },
            version: ReleaseVersion::parse(&d.version)?,
            payload_bytes,
            minimum_windows: match d.minimum_windows {
                Windows::Windows7Sp1 => WindowsBaseline::Windows7Sp1,
                Windows::Windows10 => WindowsBaseline::Windows10,
                Windows::Windows11 => WindowsBaseline::Windows11,
            },
            architecture: match d.architecture {
                Architecture::X86 => UpdateArchitecture::X86,
                Architecture::X64 => UpdateArchitecture::X64,
            },
            minimum_coordinator: ReleaseVersion::parse(&d.minimum_coordinator)?,
            source_schema: CompatibilityRange::new(d.source_schema.first, d.source_schema.last)?,
            source_protocol: CompatibilityRange::new(
                d.source_protocol.first,
                d.source_protocol.last,
            )?,
            allowed_predecessors: predecessors,
        })
    }
    pub fn compatibility(&self) -> ReleaseCompatibility<'_> {
        ReleaseCompatibility {
            product: &self.product,
            channel: &self.channel,
            identity: self.identity,
            minimum_windows: self.minimum_windows,
            architecture: self.architecture,
            installer_abi: UPDATE_INSTALLER_ABI,
            guard_capability: UPDATE_GUARD_CAPABILITY,
            minimum_coordinator: self.minimum_coordinator,
            source_schema: self.source_schema,
            source_protocol: self.source_protocol,
            allowed_predecessors: &self.allowed_predecessors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn custom() -> serde_json::Value {
        serde_json::json!({ "schemaVersion":1, "releaseId":"fc-0.2.0-x86-1", "product":"FutureClassroom", "channel":"stable", "releaseSequence":2, "version":"0.2.0", "minimumWindows":"Windows7Sp1", "architecture":"x86", "installerAbi":2, "guardCapability":3, "minimumCoordinator":"0.1.5", "sourceSchema":{"first":1,"last":2}, "sourceProtocol":{"first":3,"last":3}, "allowedPredecessors":[{ "productCode":"99378807-098d-5f04-8d4d-f181dfa218d6", "packageCode":"99378807-098d-5f04-8d4d-f181dfa218d7", "sourceCommit":"11".repeat(20), "sourceManifestSha256":"11".repeat(32), "producerSha256":"22".repeat(32), "cachedMsiSha256":"33".repeat(32), "payloadEnvelopeSha256":"44".repeat(32), "templateSha256":"55".repeat(32), "guardSha256":"66".repeat(32), "logicalSha256":"77".repeat(32), "templateBytes":1156, "installerAbi":2, "guardCapability":3 }] })
    }
    fn parse(v: &serde_json::Value) -> Result<ReleaseClaims, InvalidUpdateValue> {
        ReleaseClaims::from_verified_custom(&serde_json::to_vec(v).unwrap(), [8; 32], 1024)
    }
    #[test]
    fn strict_custom_retains_full_predecessor_and_external_target_identity() {
        let value = custom();
        let claims = parse(&value).unwrap();
        assert_eq!(claims.identity.payload_sha256, [8; 32]);
        assert_eq!(claims.allowed_predecessors[0].cached_msi_sha256, [0x33; 32]);
        for (field, replacement) in [
            ("releaseSequence", serde_json::json!(0)),
            ("product", serde_json::json!("Other")),
            ("version", serde_json::json!("01.2.3")),
            ("installerAbi", serde_json::json!(1)),
            ("allowedPredecessors", serde_json::json!([])),
        ] {
            let mut bad = value.clone();
            bad[field] = replacement;
            assert!(parse(&bad).is_err());
        }
        let mut bad = value.clone();
        bad["allowedPredecessors"][0]["cachedMsiSha256"] = serde_json::json!("33".repeat(31));
        assert!(parse(&bad).is_err());
        let mut bad = value.clone();
        bad["extra"] = serde_json::json!(true);
        assert!(parse(&bad).is_err());
        let duplicate =
            serde_json::to_string(&value)
                .unwrap()
                .replacen("{", "{\"schemaVersion\":1,", 1);
        assert!(ReleaseClaims::from_verified_custom(duplicate.as_bytes(), [8; 32], 1024).is_err());
    }
}
