//! Compiled reviewed publication records, not runtime artifact authorization.
//! No CLI/JSON/path/hash constructor exists. The initial registry is EMPTY:
//! no ABI2 carrier has been published/validated yet. A fixture is never a record.
//! Lookup returns only a trusted expected identity; native whole-image/CNG peer
//! proof and original owner/guard/effect checks are still independently required.
use super::{
    super::{LifecycleError, Result},
    carrier,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublishedHostId {
    Abi2Initial,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostTarget {
    I686Win7Msvc,
}
#[derive(Debug)]
pub(crate) struct PublishedInstallerHost {
    id: PublishedHostId,
    abi: u16,
    target: HostTarget,
    source_commit: [u8; 20],
    source_manifest_sha256: [u8; 32],
    compiler_artifact_sha256: [u8; 32],
    producer_report_sha256: [u8; 32],
    template_bytes: u32,
    template_sha256: [u8; 32],
    normal_profile_test: bool,
}
// Adding an entry requires separately reviewed original CI/producer/source and
// immutable ABI2 template evidence. Never fill this from self-reported metadata.
const PUBLISHED: &[PublishedInstallerHost] = &[];
impl PublishedInstallerHost {
    pub(crate) fn id(&self) -> PublishedHostId {
        self.id
    }
    pub(crate) fn abi(&self) -> u16 {
        self.abi
    }
    pub(crate) fn source_commit(&self) -> [u8; 20] {
        self.source_commit
    }
    pub(crate) fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }
    pub(crate) fn compiler_artifact_sha256(&self) -> [u8; 32] {
        self.compiler_artifact_sha256
    }
    pub(crate) fn producer_report_sha256(&self) -> [u8; 32] {
        self.producer_report_sha256
    }
    pub(crate) fn template_bytes(&self) -> u32 {
        self.template_bytes
    }
    pub(crate) fn expected_template_sha256(&self) -> [u8; 32] {
        self.template_sha256
    }
    fn well_formed(&self) -> bool {
        self.abi == carrier::ABI
            && self.target == HostTarget::I686Win7Msvc
            && self.source_commit != [0; 20]
            && [
                self.source_manifest_sha256,
                self.compiler_artifact_sha256,
                self.producer_report_sha256,
                self.template_sha256,
            ]
            .iter()
            .all(|v| *v != [0; 32])
            && (carrier::MIN_IMAGE_BYTES..=carrier::MAX_IMAGE_BYTES)
                .contains(&(self.template_bytes as usize))
            && !self.normal_profile_test
    }
}
pub(crate) fn lookup(id: PublishedHostId) -> Result<&'static PublishedInstallerHost> {
    let mut found = PUBLISHED.iter().filter(|r| r.id == id);
    let record = found.next().ok_or(LifecycleError::Invalid)?;
    if found.next().is_some() || !record.well_formed() {
        return Err(LifecycleError::Invalid);
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_record_not_published() -> PublishedInstallerHost {
        PublishedInstallerHost {
            id: PublishedHostId::Abi2Initial,
            abi: 2,
            target: HostTarget::I686Win7Msvc,
            source_commit: [1; 20],
            source_manifest_sha256: [2; 32],
            compiler_artifact_sha256: [3; 32],
            producer_report_sha256: [4; 32],
            template_bytes: 1156,
            template_sha256: [5; 32],
            normal_profile_test: false,
        }
    }
    #[test]
    fn abi2_publication_registry_has_no_fixture_or_runtime_hash_authority() {
        assert!(lookup(PublishedHostId::Abi2Initial).is_err());
        let fixture = fixture_record_not_published();
        assert!(fixture.well_formed());
        assert!(lookup(fixture.id()).is_err());
        assert_eq!(fixture.abi(), 2);
        assert_eq!(fixture.source_commit(), [1; 20]);
        assert_eq!(fixture.source_manifest_sha256(), [2; 32]);
        assert_eq!(fixture.compiler_artifact_sha256(), [3; 32]);
        assert_eq!(fixture.producer_report_sha256(), [4; 32]);
        assert_eq!(fixture.template_bytes(), 1156);
        assert_eq!(fixture.expected_template_sha256(), [5; 32]);
    }
    #[test]
    fn abi2_publication_metadata_rejects_zero_provenance_test_artifact_and_wrong_abi() {
        let mut r = fixture_record_not_published();
        r.source_commit = [0; 20];
        assert!(!r.well_formed());
        let mut r = fixture_record_not_published();
        r.normal_profile_test = true;
        assert!(!r.well_formed());
        let mut r = fixture_record_not_published();
        r.abi = 1;
        assert!(!r.well_formed());
        let mut r = fixture_record_not_published();
        r.template_sha256 = [0; 32];
        assert!(!r.well_formed());
        let mut r = fixture_record_not_published();
        r.template_bytes = 0;
        assert!(!r.well_formed());
    }
}
