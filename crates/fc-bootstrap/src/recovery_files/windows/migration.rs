//! Worker-owned mapping of immutable package files. The source manifest remains
//! evidence of the external package; the mapped manifest never becomes a package.
use super::*;
use fc_domain::recovery::{MigrationCatalog, MigrationMapping};
use fc_storage::recovery::{
    migration_mapping_sha256, MappedOrdinaryReceipt, MigrationSourceReceipt,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceBinding {
    current_revision: fc_domain::ids::Revision,
    source_revision: fc_domain::ids::Revision,
    school_id: fc_domain::ids::SchoolId,
    installation_id: [u8; 32],
    database_bytes: u64,
    database_sha256: [u8; 32],
    catalog_sha256: [u8; 32],
    catalog: MigrationCatalog,
}
impl From<&MigrationSourceReceipt> for SourceBinding {
    fn from(receipt: &MigrationSourceReceipt) -> Self {
        Self {
            current_revision: receipt.current_revision(),
            source_revision: receipt.source_revision(),
            school_id: receipt.school_id(),
            installation_id: receipt.installation_id(),
            database_bytes: receipt.database_bytes(),
            database_sha256: receipt.database_sha256(),
            catalog_sha256: receipt.catalog_sha256(),
            catalog: receipt.catalog().clone(),
        }
    }
}

/// Both workspaces and every immutable source pin remain worker-owned. This is
/// neither Clone nor serializable and must never be returned directly to UI.
pub struct MappingStage {
    source: PreparedRestore,
    target: RecoveryWorkspace,
    expected: SourceBinding,
    mapping: MigrationMapping,
    mapping_sha256: [u8; 32],
}

fn complete<T: Copy + Ord>(pairs: &[(T, T)], source: impl Iterator<Item = T>) -> bool {
    let expected: BTreeSet<_> = source.collect();
    let mut from = BTreeSet::new();
    let mut to = BTreeSet::new();
    pairs.iter().all(|(a, b)| from.insert(*a) && to.insert(*b)) && from == expected
}
impl PreparedRestore {
    /// `expected` is produced only by the writer's complete source inspection.
    /// Target existence and current authority are checked by that same writer;
    /// this adapter additionally refuses incomplete or aliased file mappings.
    pub fn prepare_mapping(
        self,
        expected: MigrationSourceReceipt,
        mapping: MigrationMapping,
        check: &dyn Fn() -> bool,
    ) -> Result<MappingStage> {
        self.prepare_mapping_bound(SourceBinding::from(&expected), mapping, check)
    }
    fn prepare_mapping_bound(
        mut self,
        expected: SourceBinding,
        mapping: MigrationMapping,
        check: &dyn Fn() -> bool,
    ) -> Result<MappingStage> {
        ensure(check)?;
        // Shared canonical helper bounds the mapping before any adapter sets
        // or a second owned workspace can allocate from caller-provided pairs.
        let mapping_sha256 = migration_mapping_sha256(&mapping).map_err(invalid)?;
        expected.catalog.validate().map_err(invalid)?;
        if expected.school_id != self.manifest.school_id
            || expected.installation_id != self.manifest.installation_id
            || expected.source_revision != self.manifest.revision
            || expected.catalog.school_id != expected.school_id
            || mapping.source_school_id != expected.school_id
            || !complete(
                &mapping.classes,
                expected.catalog.classes.iter().map(|x| x.id),
            )
            || !complete(
                &mapping.subjects,
                expected.catalog.subjects.iter().map(|x| x.id),
            )
            || !complete(
                &mapping.principals,
                expected.catalog.principals.iter().map(|x| x.id),
            )
            || !matches!(self.manifest.entries.first(), Some(BackupEntry::Database {bytes,sha256})
                if (*bytes,*sha256)==(expected.database_bytes,expected.database_sha256))
        {
            return Err(RecoveryFileError::InvalidPackage);
        }
        let ordinary = self
            .ordinary
            .as_mut()
            .ok_or(RecoveryFileError::InvalidPackage)?;
        if digest(ordinary, check)? != (expected.database_bytes, expected.database_sha256) {
            return Err(RecoveryFileError::InvalidPackage);
        }
        // Validate all file scopes before allocating the second workspace.
        for entry in &self.manifest.entries[1..] {
            mapped_entry(entry, &mapping)?;
        }
        let target = RecoveryFiles::new(&self.workspace.paths)?.workspace()?;
        Ok(MappingStage {
            source: self,
            target,
            expected,
            mapping,
            mapping_sha256,
        })
    }
    /// Invoke only on the original/retirement worker. Heavy recursive cleanup is
    /// explicit at the composition boundary; dropping the lightweight UI bundle
    /// merely requests retirement of this capability in the shared slot.
    pub fn discard(self, check: &dyn Fn() -> bool) -> Result<()> {
        let Self {
            ordinary,
            media,
            workspace,
            ..
        } = self;
        drop(ordinary);
        drop(media);
        workspace.discard(check)
    }
}

fn mapped_entry(entry: &BackupEntry, mapping: &MigrationMapping) -> Result<BackupEntry> {
    Ok(match entry {
        BackupEntry::Material {
            teacher_id,
            subject_id,
            material_id,
            version_id,
            kind,
            bytes,
            sha256,
        } => BackupEntry::Material {
            teacher_id: mapping.principal(*teacher_id).map_err(invalid)?,
            subject_id: mapping.subject(*subject_id).map_err(invalid)?,
            material_id: *material_id,
            version_id: *version_id,
            kind: *kind,
            bytes: *bytes,
            sha256: *sha256,
        },
        BackupEntry::ProfileMedia { .. } => entry.clone(),
        BackupEntry::Database { .. } => return Err(RecoveryFileError::InvalidPackage),
    })
}
fn media_match(expected: &[BackupEntry], actual: &[BackupEntry]) -> bool {
    fn key(entry: &BackupEntry) -> Option<(u8, u64)> {
        match entry {
            BackupEntry::Material { version_id, .. } => Some((0, version_id.get())),
            BackupEntry::ProfileMedia { version_id, .. } => Some((1, version_id.get())),
            BackupEntry::Database { .. } => None,
        }
    }
    if expected.len() != actual.len() {
        return false;
    }
    let mut entries = BTreeMap::new();
    for entry in expected {
        let Some(key) = key(entry) else {
            return false;
        };
        if entries.insert(key, entry).is_some() {
            return false;
        }
    }
    for entry in actual {
        let Some(key) = key(entry) else {
            return false;
        };
        if entries.remove(&key) != Some(entry) {
            return false;
        }
    }
    entries.is_empty()
}

// A private borrowed view makes byte/ownership tests possible without adding a
// public constructor that could forge the writer's opaque proof.
struct MappedBinding<'a> {
    source: SourceBinding,
    mapping_sha256: [u8; 32],
    current_revision: fc_domain::ids::Revision,
    school_id: fc_domain::ids::SchoolId,
    installation_id: [u8; 32],
    revision: fc_domain::ids::Revision,
    database_bytes: u64,
    database_sha256: [u8; 32],
    media: &'a [BackupEntry],
}
impl MappingStage {
    pub fn source_database(&self) -> &Path {
        self.source.ordinary_database()
    }
    /// Only the writer may create this exact new destination. It has no existing
    /// file; its held, protected operation cannot be replaced underneath writer.
    pub fn mapped_database_destination(&self) -> PathBuf {
        self.target.ordinary_database()
    }
    pub fn source_manifest(&self) -> &BackupManifest {
        self.source.manifest()
    }
    pub fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source.manifest_sha256()
    }
    pub fn mapping_sha256(&self) -> [u8; 32] {
        self.mapping_sha256
    }
    pub fn finish(
        self,
        receipt: &MappedOrdinaryReceipt,
        check: &dyn Fn() -> bool,
    ) -> Result<MappedPreparedRestore> {
        self.finish_bound(
            MappedBinding {
                source: SourceBinding::from(receipt.source()),
                mapping_sha256: receipt.mapping_sha256(),
                current_revision: receipt.current_revision(),
                school_id: receipt.school_id(),
                installation_id: receipt.installation_id(),
                revision: receipt.revision(),
                database_bytes: receipt.database_bytes(),
                database_sha256: receipt.database_sha256(),
                media: receipt.media(),
            },
            check,
        )
    }
    fn finish_bound(
        mut self,
        receipt: MappedBinding<'_>,
        check: &dyn Fn() -> bool,
    ) -> Result<MappedPreparedRestore> {
        ensure(check)?;
        if receipt.source != self.expected
            || receipt.mapping_sha256 != self.mapping_sha256
            || receipt.current_revision != self.expected.current_revision
            || receipt.school_id != self.mapping.target_school_id
            || receipt.installation_id == [0; 32]
            || receipt.revision != self.expected.source_revision
            || receipt.database_bytes == 0
            || receipt.database_bytes > MAX_DATABASE_BYTES
        {
            return Err(RecoveryFileError::InvalidPackage);
        }
        let mut entries = Vec::with_capacity(self.source.manifest.entries.len());
        entries.push(BackupEntry::Database {
            bytes: receipt.database_bytes,
            sha256: receipt.database_sha256,
        });
        for entry in &self.source.manifest.entries[1..] {
            entries.push(mapped_entry(entry, &self.mapping)?);
        }
        if !media_match(&entries[1..], receipt.media)
            || self.source.media.len() != entries.len() - 1
        {
            return Err(RecoveryFileError::InvalidPackage);
        }
        let manifest = BackupManifest {
            format: self.source.manifest.format,
            installation_id: receipt.installation_id,
            school_id: receipt.school_id,
            revision: receipt.revision,
            entries,
        };
        manifest.validate().map_err(invalid)?;
        let mut ordinary = LockedFile::open(&self.target.ordinary_database(), MAX_DATABASE_BYTES)
            .map_err(unavailable)?;
        if digest(&mut ordinary, check)? != (receipt.database_bytes, receipt.database_sha256) {
            return Err(RecoveryFileError::InvalidPackage);
        }
        let mut media = Vec::with_capacity(self.source.media.len());
        for (source, entry) in self.source.media.iter_mut().zip(&manifest.entries[1..]) {
            ensure(check)?;
            let target = self.target.new_root().join(entry_relative_path(entry));
            create_parent(&self.target.files, &target)?;
            let mut output = self.target.files.create_new(&target).map_err(unavailable)?;
            copy_exact(source, output.file_mut(), entry, check)?;
            let path = output.finish().map_err(unavailable)?;
            let mut pin = LockedFile::open(&path, entry.bytes()).map_err(unavailable)?;
            if digest(&mut pin, check)? != (entry.bytes(), entry.sha256()) {
                return Err(RecoveryFileError::InvalidPackage);
            }
            media.push(pin);
        }
        ensure(check)?;
        let manifest_sha256 = fc_security::sha256(&encode_manifest(&manifest).map_err(invalid)?);
        Ok(MappedPreparedRestore {
            mapped: PreparedRestore {
                ordinary: Some(ordinary),
                media,
                manifest,
                manifest_sha256,
                workspace: self.target,
            },
            source: self.source,
            mapping_sha256: self.mapping_sha256,
        })
    }
    pub fn discard(self, check: &dyn Fn() -> bool) -> Result<()> {
        let Self { source, target, .. } = self;
        let source_result = source.discard(check);
        let target_result = target.discard(check);
        source_result.and(target_result)
    }
}

fn copy_exact(
    source: &mut LockedFile,
    output: &mut std::fs::File,
    entry: &BackupEntry,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    if source.len().map_err(unavailable)? != entry.bytes() {
        return Err(RecoveryFileError::InvalidPackage);
    }
    source
        .file_mut()
        .seek(SeekFrom::Start(0))
        .map_err(unavailable)?;
    let mut written = DigestWriter {
        inner: output,
        hash: fc_security::Sha256Stream::default(),
    };
    let mut block = [0; 32 * 1024];
    let mut total = 0u64;
    loop {
        ensure(check)?;
        let n = source.file_mut().read(&mut block).map_err(unavailable)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(RecoveryFileError::InvalidPackage)?;
        if total > entry.bytes() {
            return Err(RecoveryFileError::InvalidPackage);
        }
        written.write_all(&block[..n]).map_err(unavailable)?;
    }
    if total != entry.bytes() || written.hash.finish() != entry.sha256() {
        return Err(RecoveryFileError::InvalidPackage);
    }
    ensure(check)
}

/// An internal mapped ordinary projection, deliberately distinct from a package
/// or PinnedBackup. The original external manifest and files remain immutable.
pub struct MappedPreparedRestore {
    mapped: PreparedRestore,
    source: PreparedRestore,
    mapping_sha256: [u8; 32],
}
impl MappedPreparedRestore {
    pub fn manifest(&self) -> &BackupManifest {
        self.mapped.manifest()
    }
    pub fn manifest_sha256(&self) -> [u8; 32] {
        self.mapped.manifest_sha256()
    }
    pub fn source_manifest(&self) -> &BackupManifest {
        self.source.manifest()
    }
    pub fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source.manifest_sha256()
    }
    pub fn mapping_sha256(&self) -> [u8; 32] {
        self.mapping_sha256
    }
    pub fn ordinary_database(&self) -> &Path {
        self.mapped.ordinary_database()
    }
    pub fn candidate_database(&self) -> PathBuf {
        self.mapped.candidate_database()
    }
    pub fn seal_candidate(
        self,
        prepared: &crate::backend::PreparedRecoveryDatabase,
        check: &dyn Fn() -> bool,
    ) -> Result<ColdRestore> {
        let Self { mapped, source, .. } = self;
        let cold = mapped.seal_candidate(prepared, check)?;
        // The original is retired only on this worker, after mapped sealing.
        source.discard(&|| true)?;
        Ok(cold)
    }
    pub fn discard(self, check: &dyn Fn() -> bool) -> Result<()> {
        let Self { mapped, source, .. } = self;
        let mapped_result = mapped.discard(check);
        let source_result = source.discard(check);
        mapped_result.and(source_result)
    }
}

#[cfg(test)]
mod tests;
