//! Fixed MSI Binary ABI2 envelope; not yet connected to native admission/CA.
//! Reusing the old payload grammar does not reinterpret an old overlay as ABI2.
use super::super::{LifecycleError, Result, INSTALL_FAMILY};
use super::{carrier, manifest};

pub(crate) const MAGIC: &[u8; 8] = b"FCPM2\0\0\0";
pub(crate) const BINARY_NAME: &str = "FcPayloadManifest";
pub(crate) const QUERY: &str = "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcPayloadManifest'";
pub(crate) const HEADER_BYTES: usize = 88;
pub(crate) const MAX_BYTES: usize = HEADER_BYTES + manifest::MAX_MANIFEST;

#[derive(Debug)]
pub(crate) struct CapturedPayloadManifest {
    package: String,
    payload: manifest::Manifest,
}
impl CapturedPayloadManifest {
    pub(crate) fn package(&self) -> &str {
        &self.package
    }
    pub(crate) fn payload(&self) -> &manifest::Manifest {
        &self.payload
    }
}
fn invalid<T>() -> Result<T> {
    Err(LifecycleError::Invalid)
}
fn nonzero_uuid(s: &str) -> bool {
    manifest::uuid(s) && s.bytes().any(|b| b != b'0' && b != b'-')
}
fn canonical_version(value: &str) -> bool {
    let mut parts = value.split('.');
    for maximum in [255u32, 255, 65535] {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.is_empty()
            || part.len() > 5
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
            || !part.parse::<u32>().is_ok_and(|n| n <= maximum)
        {
            return false;
        }
    }
    parts.next().is_none()
}
fn checked_payload(raw: &[u8]) -> Result<manifest::Manifest> {
    let m = manifest::parse(raw)?;
    if !nonzero_uuid(&m.product)
        || !canonical_version(&m.version)
        || m.entries
            .iter()
            .any(|e| !nonzero_uuid(&e.component) || e.sha256 == [0; 32])
    {
        return invalid();
    }
    Ok(m)
}
pub(crate) fn parse(raw: &[u8]) -> Result<CapturedPayloadManifest> {
    if !(HEADER_BYTES + 1..=MAX_BYTES).contains(&raw.len())
        || &raw[..8] != MAGIC
        || raw[8..10] != 2u16.to_le_bytes()
        || raw[10..12] != carrier::ABI.to_le_bytes()
        || &raw[12..48] != INSTALL_FAMILY.as_bytes()
    {
        return invalid();
    }
    let package = std::str::from_utf8(&raw[48..84]).map_err(|_| LifecycleError::Invalid)?;
    if !nonzero_uuid(package) {
        return invalid();
    }
    let size = u32::from_le_bytes(
        raw[84..88]
            .try_into()
            .map_err(|_| LifecycleError::Invalid)?,
    ) as usize;
    if size == 0
        || size > manifest::MAX_MANIFEST
        || HEADER_BYTES.checked_add(size) != Some(raw.len())
    {
        return invalid();
    }
    Ok(CapturedPayloadManifest {
        package: package.to_owned(),
        payload: checked_payload(&raw[HEADER_BYTES..])?,
    })
}
/// PackageCode must already be selected for Summary9. It is data, not authority.
/// This stream intentionally contains no complete MSI hash, avoiding a cycle;
/// only the external carrier seal binds the final complete MSI hash.
pub(crate) fn encode_format(package: &str, payload_raw: &[u8]) -> Result<Vec<u8>> {
    if !nonzero_uuid(package) || payload_raw.len() > manifest::MAX_MANIFEST {
        return invalid();
    }
    checked_payload(payload_raw)?;
    let mut b = Vec::with_capacity(HEADER_BYTES + payload_raw.len());
    b.extend(MAGIC);
    b.extend(2u16.to_le_bytes());
    b.extend(carrier::ABI.to_le_bytes());
    b.extend(INSTALL_FAMILY.as_bytes());
    b.extend(package.as_bytes());
    b.extend((payload_raw.len() as u32).to_le_bytes());
    b.extend(payload_raw);
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    const PACKAGE: &str = "99378807-098D-5F04-8D4D-F181DFA218D6";
    fn body(version: &str) -> Vec<u8> {
        let mut names = [
            "fc-bootstrap.exe",
            "LICENSE",
            "PROBE-README.txt",
            "THIRD-PARTY-NOTICES.txt",
            "manifest.json",
            "MSI-CANDIDATE.txt",
            "web/index.html",
            "web/fc-assets.json",
            "web/THIRD-PARTY-NOTICES.txt",
        ];
        names.sort();
        let mut b = manifest::MAGIC.to_vec();
        b.extend(1u16.to_le_bytes());
        b.extend((names.len() as u16).to_le_bytes());
        b.extend(b"12345678-1234-1234-1234-123456789ABC");
        b.push(version.len() as u8);
        b.extend(version.as_bytes());
        for (i, name) in names.iter().enumerate() {
            b.extend((name.len() as u16).to_le_bytes());
            b.extend(name.as_bytes());
            b.extend(1u32.to_le_bytes());
            b.extend([i as u8 + 1; 32]);
            b.extend(format!("00000000-0000-0000-0000-{:012X}", i + 1).as_bytes());
        }
        b
    }
    #[test]
    fn abi2_payload_roundtrip_binds_package_family_abi_and_original_body() {
        let legacy = body("0.2.0");
        let encoded = encode_format(PACKAGE, &legacy).unwrap();
        let parsed = parse(&encoded).unwrap();
        assert_eq!(parsed.package(), PACKAGE);
        assert_eq!(
            parsed.payload().product,
            "12345678-1234-1234-1234-123456789ABC"
        );
        assert_eq!(parsed.payload().version, "0.2.0");
        assert_eq!(parsed.payload().entries.len(), 9);
        assert_eq!(&encoded[88..], legacy);
        assert_eq!(encoded.len(), 88 + legacy.len());
        assert_eq!(BINARY_NAME, "FcPayloadManifest");
        assert!(QUERY.contains(BINARY_NAME));
    }
    #[test]
    fn abi2_payload_never_reinterprets_legacy_format_or_accepts_extra_tail() {
        let legacy = body("0.2.0");
        assert!(manifest::parse(&legacy).is_ok());
        assert!(parse(&legacy).is_err());
        let mut encoded = encode_format(PACKAGE, &legacy).unwrap();
        assert!(manifest::parse(&encoded).is_err());
        encoded.push(0);
        assert!(parse(&encoded).is_err());
    }
    #[test]
    fn abi2_payload_rejects_header_aliases_and_every_truncation() {
        let encoded = encode_format(PACKAGE, &body("0.2.0")).unwrap();
        for at in [0, 8, 10, 12, 56, 84] {
            let mut b = encoded.clone();
            b[at] ^= 1;
            assert!(parse(&b).is_err());
        }
        for n in 0..encoded.len() {
            assert!(parse(&encoded[..n]).is_err());
        }
        let mut different_package = encoded.clone();
        different_package[48] = b'8';
        assert_eq!(
            parse(&different_package).unwrap().package().as_bytes()[0],
            b'8'
        );
        // A different well-formed identity is captured data, never provenance.
        assert!(encode_format("00000000-0000-0000-0000-000000000000", &body("0.2.0")).is_err());
        assert!(encode_format(&PACKAGE.to_ascii_lowercase(), &body("0.2.0")).is_err());
    }
    #[test]
    fn abi2_payload_adds_no_silent_version_or_identity_alias_to_legacy_parser() {
        let alias = body("00.2.0");
        assert!(manifest::parse(&alias).is_ok());
        assert!(encode_format(PACKAGE, &alias).is_err());
        let mut zero = body("0.2.0");
        zero[12..48].copy_from_slice(b"00000000-0000-0000-0000-000000000000");
        assert!(manifest::parse(&zero).is_ok());
        assert!(encode_format(PACKAGE, &zero).is_err());
        let mut zero = body("0.2.0");
        let first_size = 12 + 36 + 1 + 5;
        let length =
            u16::from_le_bytes(zero[first_size..first_size + 2].try_into().unwrap()) as usize;
        let digest = first_size + 2 + length + 4;
        zero[digest..digest + 32].fill(0);
        assert!(manifest::parse(&zero).is_ok());
        assert!(encode_format(PACKAGE, &zero).is_err());
    }
    #[test]
    fn abi2_codec_fixture_emit_for_cross_language_bytes_only() {
        fn hex(bytes: &[u8]) -> String {
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        }
        let prefix = carrier::tests::pe_fixture();
        let template = carrier::make_template(&prefix).unwrap();
        let inner_msi = b"ABI2 structural fixture, not an MSI database";
        let fixture_digest = [
            37, 36, 45, 170, 49, 136, 42, 174, 242, 245, 203, 85, 9, 252, 10, 171, 31, 76, 23, 68,
            183, 196, 159, 138, 162, 255, 136, 133, 53, 246, 37, 228,
        ];
        let sealed = carrier::fill_digest_format_value(&template, fixture_digest).unwrap();
        let payload = body("0.2.0");
        let envelope = encode_format(PACKAGE, &payload).unwrap();
        println!("ABI2_CODEC_FIXTURE normal_pe={} descriptor={} template={} sealed={} body={} envelope={} package={} inner_msi={}",hex(&prefix),hex(&carrier::descriptor()),hex(&template),hex(&sealed),hex(&payload),hex(&envelope),PACKAGE,hex(inner_msi));
    }
}
