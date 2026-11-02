//! Exact builder-produced publication data. Parsing is not native admission.
//! Only SourceReader's original sealed-MSI capture can introduce this record.
use super::super::{carrier, payload_manifest};
use crate::installer_lifecycle::{LifecycleError, Result};
pub(crate) const QUERY: &str =
    "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcPublishedPredecessor'";
pub(crate) const MAGIC: &[u8; 8] = b"FCPRED2\0";
pub(crate) const HEADER_BYTES: usize = 248;
pub(crate) const MAX_BYTES: usize = HEADER_BYTES + payload_manifest::MAX_BYTES;
#[derive(Debug)]
pub(crate) struct PublishedPredecessor {
    pub(crate) target: u8,
    pub(crate) source_commit: [u8; 20],
    pub(crate) source_manifest_sha: [u8; 32],
    pub(crate) producer_sha: [u8; 32],
    pub(crate) source_msi_sha: [u8; 32],
    pub(crate) template_sha: [u8; 32],
    pub(crate) guard_sha: [u8; 32],
    pub(crate) logical_sha: [u8; 32],
    pub(crate) template_bytes: u32,
    envelope: Vec<u8>,
}
impl PublishedPredecessor {
    pub(crate) fn envelope(&self) -> &[u8] {
        &self.envelope
    }
    pub(crate) fn payload(&self) -> Result<payload_manifest::CapturedPayloadManifest> {
        payload_manifest::parse(&self.envelope)
    }
}
pub(crate) fn parse(b: &[u8]) -> Result<PublishedPredecessor> {
    if !(HEADER_BYTES + 1..=MAX_BYTES).contains(&b.len())
        || &b[..8] != MAGIC
        || b[8..10] != 1u16.to_le_bytes()
        || b[10..12] != carrier::ABI.to_le_bytes()
        || !matches!(b[12], 1 | 2)
        || b[13..16] != [0; 3]
        || b[236..248] != [0; 12]
    {
        return Err(LifecycleError::Invalid);
    }
    let size = u32::from_le_bytes(b[232..236].try_into().unwrap()) as usize;
    let template_bytes = u32::from_le_bytes(b[228..232].try_into().unwrap());
    if HEADER_BYTES.checked_add(size) != Some(b.len())
        || !(carrier::MIN_IMAGE_BYTES..=carrier::MAX_IMAGE_BYTES)
            .contains(&(template_bytes as usize))
        || b[16..36] == [0; 20]
        || b[36..228].as_chunks::<32>().0.iter().any(|v| v == &[0; 32])
    {
        return Err(LifecycleError::Invalid);
    }
    payload_manifest::parse(&b[HEADER_BYTES..])?;
    Ok(PublishedPredecessor {
        target: b[12],
        source_commit: b[16..36].try_into().unwrap(),
        source_manifest_sha: b[36..68].try_into().unwrap(),
        producer_sha: b[68..100].try_into().unwrap(),
        source_msi_sha: b[100..132].try_into().unwrap(),
        template_sha: b[132..164].try_into().unwrap(),
        guard_sha: b[164..196].try_into().unwrap(),
        logical_sha: b[196..228].try_into().unwrap(),
        template_bytes,
        envelope: b[HEADER_BYTES..].to_vec(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn valid() -> Vec<u8> {
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
        let mut body = super::super::super::manifest::MAGIC.to_vec();
        body.extend(1u16.to_le_bytes());
        body.extend((names.len() as u16).to_le_bytes());
        body.extend(b"99378807-098D-5F04-8D4D-F181DFA218D6");
        body.push(5);
        body.extend(b"0.2.0");
        for (i, name) in names.iter().enumerate() {
            body.extend((name.len() as u16).to_le_bytes());
            body.extend(name.as_bytes());
            body.extend(1u32.to_le_bytes());
            body.extend([1; 32]);
            body.extend(format!("00000000-0000-0000-0000-{:012X}", i + 1).as_bytes());
        }
        let envelope =
            payload_manifest::encode_format("99378807-098D-5F04-8D4D-F181DFA218D7", &body).unwrap();
        let mut b = vec![0; HEADER_BYTES];
        b[..8].copy_from_slice(MAGIC);
        b[8..10].copy_from_slice(&1u16.to_le_bytes());
        b[10..12].copy_from_slice(&2u16.to_le_bytes());
        b[12] = 2;
        b[16..228].fill(1);
        b[228..232].copy_from_slice(&1156u32.to_le_bytes());
        b[232..236].copy_from_slice(&(envelope.len() as u32).to_le_bytes());
        b.extend(envelope);
        b
    }
    #[test]
    fn predecessor_complete_record_rejects_truncation_tail_alias_and_zero_fields() {
        let b = valid();
        let parsed = parse(&b).unwrap();
        assert_eq!(parsed.payload().unwrap().payload().version, "0.2.0");
        assert_eq!(parsed.target, 2);
        for n in 0..b.len() {
            assert!(parse(&b[..n]).is_err());
        }
        let mut extra = b.clone();
        extra.push(0);
        assert!(parse(&extra).is_err());
        for i in [0, 8, 10, 12, 13, 14, 236] {
            let mut bad = b.clone();
            bad[i] ^= 128;
            assert!(parse(&bad).is_err());
        }
        for start in [36, 68, 100, 132, 164, 196] {
            let mut bad = b.clone();
            bad[start..start + 32].fill(0);
            assert!(parse(&bad).is_err());
        }
        let mut bad = b.clone();
        bad[16..36].fill(0);
        assert!(parse(&bad).is_err());
    }
    #[test]
    fn predecessor_rejects_empty_unknown_profile_and_zero_provenance_before_authority() {
        assert!(parse(&[]).is_err());
        let mut b = vec![0; HEADER_BYTES + 1];
        b[..8].copy_from_slice(MAGIC);
        b[8..10].copy_from_slice(&1u16.to_le_bytes());
        b[10..12].copy_from_slice(&2u16.to_le_bytes());
        for target in [0, 1, 2, 3] {
            b[12] = target;
            assert!(parse(&b).is_err());
        }
        b[13] = 1;
        assert!(parse(&b).is_err());
    }
}
