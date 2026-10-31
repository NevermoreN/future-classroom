//! Explicit ID mapping is separate from ordinary same-install restore. The
//! original package, target revision and writer evidence remain bound throughout.
use super::*;
use fc_domain::recovery::{MigrationCatalog, MigrationMapping};
use fc_storage::recovery::{
    migration_mapping_sha256, MappedOrdinaryReceipt, MigrationSourceReceipt,
};

#[cfg(test)]
mod tests;

/// Review evidence only: no barrier, write authorization or cold-switch proof.
pub struct MigrationInspection {
    source_database: PathBuf,
    source_manifest: BackupManifest,
    source_manifest_sha256: [u8; 32],
    source: MigrationSourceReceipt,
    target_identity: IdentityState,
    target_installation: [u8; 32],
}
impl MigrationInspection {
    pub fn source_receipt(&self) -> &MigrationSourceReceipt {
        &self.source
    }
    pub fn source_catalog(&self) -> &MigrationCatalog {
        self.source.catalog()
    }
    pub fn source_manifest(&self) -> &BackupManifest {
        &self.source_manifest
    }
    pub fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }
    pub fn target_identity(&self) -> &IdentityState {
        &self.target_identity
    }
    pub fn target_installation(&self) -> [u8; 32] {
        self.target_installation
    }
}
fn source_matches(manifest: &BackupManifest, receipt: &MigrationSourceReceipt) -> bool {
    manifest.installation_id == receipt.installation_id()
        && manifest.school_id == receipt.school_id()
        && manifest.revision == receipt.source_revision()
        && matches!(manifest.entries.first(), Some(BackupEntry::Database { bytes, sha256 })
            if *bytes == receipt.database_bytes() && *sha256 == receipt.database_sha256())
}
impl OrdinaryBackupAccess {
    /// Shared worker only. The adapter retains the original database and media
    /// pins; a catalog inspection does not claim full mapped-data preflight.
    pub fn inspect_migration_source(
        &self,
        source_database: PathBuf,
        source_manifest: BackupManifest,
        source_manifest_sha256: [u8; 32],
        fence: ExecutionFence,
    ) -> Result<MigrationInspection> {
        self.check_revision()?;
        source_manifest
            .validate()
            .map_err(|_| BackendError::InvalidInput)?;
        let encoded = fc_protocol::recovery::encode_manifest(&source_manifest)
            .map_err(|_| BackendError::InvalidInput)?;
        if fc_security::sha256(&encoded) != source_manifest_sha256 {
            return Err(BackendError::InvalidInput);
        }
        let source = self
            .repository
            .try_inspect_migration_source(source_database.clone(), &self.actor, fence)?
            .wait()?;
        self.check_revision()?;
        let target = self
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        if source.current_revision() != self.expected_revision
            || target.revision != self.expected_revision
            || !source_matches(&source_manifest, &source)
        {
            return Err(StorageError::RecoveryConflict.into());
        }
        Ok(MigrationInspection {
            source_database,
            source_manifest,
            source_manifest_sha256,
            source,
            target_identity: target.data.clone(),
            target_installation: self.installation_id,
        })
    }

    /// Produce only a protected ordinary projection. Explicit confirmation,
    /// durable revocation, real drain and the existing cold switch follow later.
    pub fn map_migration_database(
        &self,
        inspection: &MigrationInspection,
        mapping: MigrationMapping,
        destination: PathBuf,
        fence: ExecutionFence,
    ) -> Result<MappedOrdinaryReceipt> {
        self.check_revision()?;
        if inspection.source.current_revision() != self.expected_revision
            || inspection.target_installation != self.installation_id
            || inspection
                .target_identity
                .installation
                .as_ref()
                .map(|i| i.school_id)
                != Some(self.actor.school_id)
            || !source_matches(&inspection.source_manifest, &inspection.source)
        {
            return Err(StorageError::RecoveryConflict.into());
        }
        mapping
            .validate(inspection.source_catalog(), inspection.target_identity())
            .map_err(|_| BackendError::InvalidInput)?;
        let digest = migration_mapping_sha256(&mapping)?;
        let receipt = self
            .repository
            .try_map_ordinary(
                inspection.source_database.clone(),
                destination,
                inspection.source.clone(),
                mapping,
                &self.actor,
                fence,
            )?
            .wait()?;
        self.check_revision()?;
        if receipt.source() != &inspection.source
            || receipt.current_revision() != self.expected_revision
            || receipt.mapping_sha256() != digest
            || receipt.installation_id() != self.installation_id
            || receipt.school_id() != self.actor.school_id
            || receipt.revision() != inspection.source.source_revision()
        {
            return Err(StorageError::RecoveryConflict.into());
        }
        Ok(receipt)
    }
}
