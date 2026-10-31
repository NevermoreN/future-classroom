use super::*;
use fc_domain::recovery::{
    BackupManifest, MAX_DATABASE_BYTES, MAX_MANIFEST_BYTES, MAX_MEDIA_BYTES,
};
use fc_materials::office::{ContainerError, ValidationControl};
use fc_ports::RandomSource;
use fc_protocol::recovery::{encode_manifest, PackageReader, PackageWriter};
use fc_windows::{
    materials::{
        chooser::SelectedFile,
        files::{ControlledFiles, LockedFile, PinnedDirectory},
        helper, image,
    },
    system::DataPaths,
};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path},
    time::Duration,
};

mod journal;
pub use journal::{ColdCommitReceipt, StartupRecovery};
const OWNER: &[u8] = b"FUTURE_CLASSROOM_RECOVERY_FILES_V1\n";
const MAX_PACKAGE_BYTES: u64 =
    MAX_DATABASE_BYTES + MAX_MEDIA_BYTES + MAX_MANIFEST_BYTES as u64 + 20;
fn unavailable(_: impl std::fmt::Debug) -> RecoveryFileError {
    RecoveryFileError::Unavailable
}
fn invalid(_: impl std::fmt::Debug) -> RecoveryFileError {
    RecoveryFileError::InvalidPackage
}
fn framing(error: fc_protocol::recovery::BackupFormatError) -> RecoveryFileError {
    if error == fc_protocol::recovery::BackupFormatError::Canceled {
        RecoveryFileError::Cancelled
    } else {
        RecoveryFileError::InvalidPackage
    }
}
fn ensure(check: &dyn Fn() -> bool) -> Result<()> {
    if check() {
        Ok(())
    } else {
        Err(RecoveryFileError::Cancelled)
    }
}
fn nonce() -> Result<String> {
    let mut bytes = [0; 16];
    fc_windows::SystemRandom
        .fill(&mut bytes)
        .map_err(unavailable)?;
    Ok(bytes.iter().map(|n| format!("{n:02x}")).collect())
}
fn checked_directory(files: &ControlledFiles, path: &Path) -> Result<bool> {
    files.check(path).map_err(unavailable)?;
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            PinnedDirectory::open(path).map_err(unavailable)?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(unavailable(e)),
    }
}
fn write_new(files: &ControlledFiles, path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = files.create_new(path).map_err(unavailable)?;
    f.file_mut().write_all(bytes).map_err(unavailable)?;
    f.finish().map_err(unavailable)?;
    Ok(())
}
fn read_small(path: &Path, max: usize) -> Result<Vec<u8>> {
    let mut source = LockedFile::open(path, max as u64).map_err(unavailable)?;
    let mut out = Vec::with_capacity(source.len().map_err(unavailable)? as usize);
    source
        .file_mut()
        .read_to_end(&mut out)
        .map_err(unavailable)?;
    Ok(out)
}
fn create_parent(files: &ControlledFiles, path: &Path) -> Result<()> {
    let rel = path
        .parent()
        .ok_or(RecoveryFileError::InvalidPackage)?
        .strip_prefix(files.root())
        .map_err(invalid)?;
    let components = rel
        .components()
        .map(|c| match c {
            Component::Normal(n) => n
                .to_str()
                .map(str::to_owned)
                .ok_or(RecoveryFileError::InvalidPackage),
            _ => Err(RecoveryFileError::InvalidPackage),
        })
        .collect::<Result<Vec<_>>>()?;
    if !components.is_empty() {
        files.directory(&components).map_err(unavailable)?;
    }
    Ok(())
}
struct Control<'a>(&'a dyn Fn() -> bool);
impl ValidationControl for Control<'_> {
    fn canceled(&self) -> bool {
        !(self.0)()
    }
    fn thread_cpu_time(&self) -> std::result::Result<Duration, ContainerError> {
        fc_windows::system::current_thread_cpu_time().map_err(|_| ContainerError::ClockUnavailable)
    }
}
fn digest(source: &mut LockedFile, check: &dyn Fn() -> bool) -> Result<(u64, [u8; 32])> {
    source
        .file_mut()
        .seek(SeekFrom::Start(0))
        .map_err(unavailable)?;
    let out =
        fc_materials::validation::hash_file(source.file_mut(), &Control(check)).map_err(invalid)?;
    ensure(check)?;
    Ok(out)
}
fn verify_media(
    source: &mut LockedFile,
    entry: &BackupEntry,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    ensure(check)?;
    let (kind, expected_size) = match entry {
        BackupEntry::Database { .. } => return Ok(()),
        BackupEntry::Material { kind, .. } => (*kind, None),
        BackupEntry::ProfileMedia {
            format,
            width,
            height,
            ..
        } => (
            match format {
                ImageFormat::Png => MaterialKind::Png,
                ImageFormat::Jpeg => MaterialKind::Jpeg,
                ImageFormat::Bmp => MaterialKind::Bmp,
            },
            Some((*width, *height)),
        ),
    };
    if kind == MaterialKind::Pdf {
        let report = helper::validate_pdf(source.path(), || !check()).map_err(invalid)?;
        let stdout = std::str::from_utf8(&report.stdout).map_err(invalid)?;
        let words: Vec<_> = stdout.split_whitespace().collect();
        let expected: String = entry.sha256().iter().map(|n| format!("{n:02x}")).collect();
        if report.exit_code != Some(0)
            || report.peak_private_bytes > helper::PDF_MEMORY_BYTES
            || words.len() != 5
            || words[0] != "PDF_VALIDATED"
            || words[1].parse::<u64>().ok() != Some(entry.bytes())
            || words[2] != expected
            || !words[3]
                .parse::<u32>()
                .is_ok_and(|n| (1..=2000).contains(&n))
            || !words[4]
                .parse::<u32>()
                .is_ok_and(|n| (1..=20000).contains(&n))
        {
            return Err(RecoveryFileError::InvalidPackage);
        }
    } else {
        source
            .file_mut()
            .seek(SeekFrom::Start(0))
            .map_err(unavailable)?;
        let report = fc_materials::validation::validate_file(
            source.file_mut(),
            &format!("resource.{}", material_extension(kind)),
            &Control(check),
        )
        .map_err(invalid)?;
        if report.kind != kind || report.bytes != entry.bytes() || report.sha256 != entry.sha256() {
            return Err(RecoveryFileError::InvalidPackage);
        }
        if let Some(header) = report.image {
            let decoded = image::validate_image(source, kind, || !check()).map_err(invalid)?;
            if (header.width, header.height) != (decoded.width, decoded.height)
                || expected_size.is_some_and(|s| s != (decoded.width, decoded.height))
            {
                return Err(RecoveryFileError::InvalidPackage);
            }
        } else if expected_size.is_some() {
            return Err(RecoveryFileError::InvalidPackage);
        }
    }
    ensure(check)
}

/// Trusted deployment capability. Construct after verifying the existing root's
/// protected ACL and acquiring its exclusive InstanceGuard. No OS handle leaves
/// this worker-owned object; source paths always derive from the typed manifest.
pub struct RecoveryFiles {
    paths: DataPaths,
    files: ControlledFiles,
    _root: PinnedDirectory,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportReceipt {
    pub path: PathBuf,
    pub bytes: u64,
    pub manifest_sha256: [u8; 32],
}
/// Every immutable original and its ancestors remain pinned through the actual
/// package publication. Acquiring the complete set may fail/retry under GC;
/// the caller then rechecks the exact authority revision before exporting it.
pub struct PinnedBackup {
    manifest: BackupManifest,
    sources: Vec<LockedFile>,
}
impl PinnedBackup {
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }
}
impl RecoveryFiles {
    pub fn new(paths: &DataPaths) -> Result<Self> {
        paths.verify_root_for_current_user().map_err(unavailable)?;
        let root = PinnedDirectory::open(paths.root()).map_err(unavailable)?;
        paths.verify_root_for_current_user().map_err(unavailable)?;
        let files = ControlledFiles::new(paths.root()).map_err(unavailable)?;
        Ok(Self {
            paths: paths.clone(),
            files,
            _root: root,
        })
    }
    fn namespace(&self) -> Result<PathBuf> {
        let dir = self
            .files
            .directory(&["recovery".into()])
            .map_err(unavailable)?;
        let marker = dir.join(".future-classroom-owner");
        let entries = self.files.entries(&dir, 34).map_err(unavailable)?;
        if !entries.contains(&marker) {
            if !entries.is_empty() {
                return Err(RecoveryFileError::RepairRequired);
            }
            write_new(&self.files, &marker, OWNER)?;
        } else if read_small(&marker, OWNER.len())? != OWNER {
            return Err(RecoveryFileError::RepairRequired);
        }
        for entry in entries.iter().filter(|p| **p != marker) {
            let name = entry
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(RecoveryFileError::RepairRequired)?;
            if !valid_operation(name) || !checked_directory(&self.files, entry)? {
                return Err(RecoveryFileError::RepairRequired);
            }
        }
        Ok(dir)
    }
    /// Creates a fresh confidential work area for the writer's ordinary export.
    /// Dropping an unarmed workspace removes only its bounded owned tree.
    pub fn workspace(&self) -> Result<RecoveryWorkspace> {
        let operation = self.namespace()?.join(format!("op-{}", nonce()?));
        let pin = self
            .files
            .create_directory_new(&operation)
            .map_err(unavailable)?;
        let workspace = RecoveryWorkspace {
            files: ControlledFiles::new(self.paths.root()).map_err(unavailable)?,
            operation,
            pin: Some(pin),
            armed: false,
        };
        write_new(&self.files, &workspace.operation.join("owner"), OWNER)?;
        let new = workspace.new_root();
        self.paths
            .prepare_recovery_candidate(&new)
            .map_err(unavailable)?;
        // Establish the same owner marker and original namespace used at runtime.
        crate::profile_content_files::ControlledContentFiles::new(&new).map_err(unavailable)?;
        Ok(workspace)
    }
    /// The caller holds the maintenance freeze through the actual return. Each
    /// source is immutable while framed/hash-checked and only the completed
    /// package is published to the protected exports directory.
    pub fn export_package(
        &self,
        manifest: &BackupManifest,
        ordinary: &Path,
        check: &dyn Fn() -> bool,
    ) -> Result<ExportReceipt> {
        manifest.validate().map_err(invalid)?;
        self.files.check(ordinary).map_err(unavailable)?;
        ensure(check)?;
        let mut sources = Vec::with_capacity(manifest.entries.len());
        for entry in &manifest.entries {
            ensure(check)?;
            let path = if matches!(entry, BackupEntry::Database { .. }) {
                ordinary.to_owned()
            } else {
                self.paths.root().join(entry_relative_path(entry))
            };
            let source = LockedFile::open(&path, entry.bytes()).map_err(unavailable)?;
            if source.len().map_err(unavailable)? != entry.bytes() {
                return Err(RecoveryFileError::InvalidPackage);
            }
            sources.push(source);
        }
        self.export_pinned(
            PinnedBackup {
                manifest: manifest.clone(),
                sources,
            },
            check,
        )
    }
    pub fn pin_export(
        &self,
        receipt: &fc_storage::recovery::RecoveryDatabaseReceipt,
        ordinary: &Path,
        check: &dyn Fn() -> bool,
    ) -> Result<PinnedBackup> {
        self.files.check(ordinary).map_err(unavailable)?;
        ensure(check)?;
        let mut database = LockedFile::open(ordinary, MAX_DATABASE_BYTES).map_err(unavailable)?;
        let (bytes, sha256) = digest(&mut database, check)?;
        if (bytes, sha256) != (receipt.database_bytes, receipt.database_sha256) {
            return Err(RecoveryFileError::InvalidPackage);
        }
        if (
            receipt.source_database_bytes,
            receipt.source_database_sha256,
        ) != (bytes, sha256)
        {
            // Candidate receipts describe a different ordinary source and may
            // contain local credentials. They can never become ordinary exports.
            return Err(RecoveryFileError::InvalidPackage);
        }
        database
            .file_mut()
            .seek(SeekFrom::Start(0))
            .map_err(unavailable)?;
        let mut entries = Vec::with_capacity(receipt.media.len() + 1);
        entries.push(BackupEntry::Database { bytes, sha256 });
        entries.extend(receipt.media.clone());
        let manifest = BackupManifest {
            format: fc_domain::recovery::BACKUP_FORMAT,
            installation_id: receipt.installation_id,
            school_id: receipt.school_id,
            revision: receipt.source_revision,
            entries,
        };
        manifest.validate().map_err(invalid)?;
        let mut sources = Vec::with_capacity(manifest.entries.len());
        sources.push(database);
        for entry in &manifest.entries[1..] {
            ensure(check)?;
            let source = LockedFile::open(
                &self.paths.root().join(entry_relative_path(entry)),
                entry.bytes(),
            )
            .map_err(unavailable)?;
            if source.len().map_err(unavailable)? != entry.bytes() {
                return Err(RecoveryFileError::InvalidPackage);
            }
            sources.push(source);
        }
        ensure(check)?;
        Ok(PinnedBackup { manifest, sources })
    }
    pub fn export_pinned(
        &self,
        mut pinned: PinnedBackup,
        check: &dyn Fn() -> bool,
    ) -> Result<ExportReceipt> {
        ensure(check)?;
        let manifest = &pinned.manifest;
        manifest.validate().map_err(invalid)?;
        if pinned.sources.len() != manifest.entries.len() {
            return Err(RecoveryFileError::InvalidPackage);
        }
        let exports = self
            .files
            .directory(&["exports".into()])
            .map_err(unavailable)?;
        let id = nonce()?;
        let stage = exports.join(format!("backup-{id}.part"));
        let target = exports.join(format!("backup-{id}.fcbak"));
        let mut out = self.files.create_new(&stage).map_err(unavailable)?;
        struct Cleanup<'a> {
            files: &'a ControlledFiles,
            path: Option<PathBuf>,
        }
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                if let Some(path) = &self.path {
                    let _ = self.files.remove(path);
                }
            }
        }
        let mut cleanup = Cleanup {
            files: &self.files,
            path: Some(stage),
        };
        {
            let mut writer =
                PackageWriter::new_while(out.file_mut(), manifest, check).map_err(framing)?;
            for (entry, source) in manifest.entries.iter().zip(pinned.sources.iter_mut()) {
                ensure(check)?;
                if source.len().map_err(unavailable)? != entry.bytes() {
                    return Err(RecoveryFileError::InvalidPackage);
                }
                source
                    .file_mut()
                    .seek(SeekFrom::Start(0))
                    .map_err(unavailable)?;
                writer
                    .write_next_while(source.file_mut(), check)
                    .map_err(framing)?;
            }
            writer.finish_while(check).map_err(framing)?;
        }
        ensure(check)?;
        let pinned = out.seal().map_err(unavailable)?;
        let bytes = pinned.len().map_err(unavailable)?;
        ensure(check)?;
        let published = pinned.publish(&self.files, &target).map_err(unavailable)?;
        cleanup.path = Some(target.clone());
        ensure(check)?;
        drop(published);
        cleanup.path = None;
        Ok(ExportReceipt {
            path: target,
            bytes,
            manifest_sha256: fc_security::sha256(&encode_manifest(manifest).map_err(invalid)?),
        })
    }
    pub fn preflight_package(
        &self,
        selected: SelectedFile,
        check: &dyn Fn() -> bool,
    ) -> Result<PreparedRestore> {
        let source = selected.open(MAX_PACKAGE_BYTES).map_err(unavailable)?;
        self.preflight_pinned(source, check)
    }
    /// Trusted local composition/probes may already hold a picker-derived pin.
    pub fn preflight_pinned(
        &self,
        mut source: LockedFile,
        check: &dyn Fn() -> bool,
    ) -> Result<PreparedRestore> {
        ensure(check)?;
        let workspace = self.workspace()?;
        let new = workspace.new_root();
        let candidate_files = ControlledFiles::new(&new).map_err(unavailable)?;
        let mut reader = PackageReader::new_while(source.file_mut(), check).map_err(framing)?;
        let manifest = reader.manifest().clone();
        for entry in &manifest.entries {
            ensure(check)?;
            let path = if matches!(entry, BackupEntry::Database { .. }) {
                workspace.ordinary_database()
            } else {
                new.join(entry_relative_path(entry))
            };
            create_parent(&self.files, &path)?;
            let mut file = self.files.create_new(&path).map_err(unavailable)?;
            if reader
                .copy_next_while(file.file_mut(), check)
                .map_err(framing)?
                .as_ref()
                != Some(entry)
            {
                return Err(RecoveryFileError::InvalidPackage);
            }
            let path = file.finish().map_err(unavailable)?;
            let mut pin = LockedFile::open(&path, entry.bytes()).map_err(unavailable)?;
            verify_media(&mut pin, entry, check)?;
        }
        reader.finish_while(check).map_err(framing)?;
        // Keep the exact ordinary SQLite file immutable until storage has
        // validated and projected it; the package source pin can now be released.
        drop(candidate_files);
        drop(source);
        let ordinary = LockedFile::open(&workspace.ordinary_database(), MAX_DATABASE_BYTES)
            .map_err(unavailable)?;
        let manifest_sha256 = fc_security::sha256(&encode_manifest(&manifest).map_err(invalid)?);
        Ok(PreparedRestore {
            workspace,
            manifest,
            manifest_sha256,
            ordinary: Some(ordinary),
        })
    }
    pub fn reconcile_startup(&self, check: &dyn Fn() -> bool) -> Result<StartupRecovery> {
        journal::reconcile(self, check)
    }
}
fn valid_operation(name: &str) -> bool {
    name.len() == 35
        && name.starts_with("op-")
        && name[3..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// No credentials are present until storage builds candidate_database(). This
/// object owns cleanup of its exact random operation, never a user directory.
pub struct RecoveryWorkspace {
    files: ControlledFiles,
    operation: PathBuf,
    pin: Option<PinnedDirectory>,
    armed: bool,
}
impl RecoveryWorkspace {
    pub fn ordinary_database(&self) -> PathBuf {
        self.operation.join("ordinary.sqlite3")
    }
    pub fn candidate_database(&self) -> PathBuf {
        self.new_root().join("state").join("classroom.sqlite3")
    }
    pub fn new_root(&self) -> PathBuf {
        self.operation.join("new")
    }
}
fn remove_owned_tree(files: &ControlledFiles, path: &Path, remaining: &mut usize) -> Result<()> {
    if *remaining == 0 {
        return Err(RecoveryFileError::RepairRequired);
    }
    *remaining -= 1;
    files.check(path).map_err(unavailable)?;
    // Opening the directory pins it and its ancestors while enumerating. A
    // reparse is refused before any descendant can be removed.
    let pin = PinnedDirectory::open(path).map_err(unavailable)?;
    let entries = files.entries(path, 32768).map_err(unavailable)?;
    for child in entries {
        let meta = std::fs::symlink_metadata(&child).map_err(unavailable)?;
        if meta.is_dir() {
            remove_owned_tree(files, &child, remaining)?
        } else {
            files.remove(&child).map_err(unavailable)?;
        }
    }
    drop(pin);
    files.remove_empty_directory(path).map_err(unavailable)?;
    Ok(())
}
impl Drop for RecoveryWorkspace {
    fn drop(&mut self) {
        if !self.armed {
            self.pin.take();
            // Unknown links/names fail closed; cleanup never follows them.
            let _ = remove_owned_tree(&self.files, &self.operation, &mut 100_000);
        }
    }
}
pub struct PreparedRestore {
    // Rust drops fields in declaration order: release file/ancestor pins before
    // the workspace destructor tries to remove their directories.
    ordinary: Option<LockedFile>,
    manifest: BackupManifest,
    manifest_sha256: [u8; 32],
    workspace: RecoveryWorkspace,
}
impl PreparedRestore {
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }
    pub fn manifest_sha256(&self) -> [u8; 32] {
        self.manifest_sha256
    }
    pub fn ordinary_database(&self) -> &Path {
        self.ordinary
            .as_ref()
            .expect("ordinary pin retained before sealing")
            .path()
    }
    pub fn candidate_database(&self) -> PathBuf {
        self.workspace.candidate_database()
    }
    /// Only after storage's strict ordinary preflight and current-security
    /// projection succeeded. The exact candidate bytes/generation are journaled;
    /// callers must have advanced the live security floor before creating it.
    pub fn seal_candidate(
        mut self,
        prepared: &crate::backend::PreparedRecoveryDatabase,
        check: &dyn Fn() -> bool,
    ) -> Result<ColdRestore> {
        ensure(check)?;
        let generation = prepared.receipt().recovery_generation;
        if generation == 0 {
            return Err(RecoveryFileError::InvalidPackage);
        }
        if prepared.candidate_database() != self.workspace.candidate_database()
            || prepared.live_database
                != self
                    .workspace
                    .files
                    .root()
                    .join("state")
                    .join("classroom.sqlite3")
            || prepared.manifest_sha256() != self.manifest_sha256
            || prepared.receipt().school_id != self.manifest.school_id
            || prepared.receipt().installation_id != self.manifest.installation_id
            || prepared.receipt().source_revision != self.manifest.revision
        {
            return Err(RecoveryFileError::InvalidPackage);
        }
        let candidate_revision = prepared.receipt().revision;
        let original = self
            .manifest
            .entries
            .first()
            .ok_or(RecoveryFileError::InvalidPackage)?;
        if (
            prepared.receipt().source_database_bytes,
            prepared.receipt().source_database_sha256,
        ) != (original.bytes(), original.sha256())
        {
            return Err(RecoveryFileError::InvalidPackage);
        }
        let mut candidate =
            LockedFile::open(&self.workspace.candidate_database(), MAX_DATABASE_BYTES)
                .map_err(unavailable)?;
        let (bytes, sha256) = digest(&mut candidate, check)?;
        if (bytes, sha256)
            != (
                prepared.receipt().database_bytes,
                prepared.receipt().database_sha256,
            )
        {
            return Err(RecoveryFileError::InvalidPackage);
        }
        self.ordinary.take();
        self.workspace
            .files
            .remove(&self.workspace.ordinary_database())
            .map_err(unavailable)?;
        journal::seal(
            &self.workspace,
            &self.manifest,
            generation,
            candidate_revision,
            bytes,
            sha256,
        )?;
        Ok(ColdRestore {
            workspace: self.workspace,
            manifest: self.manifest,
            generation,
            candidate_bytes: bytes,
            candidate_sha256: sha256,
            candidate_revision,
            _candidate: candidate,
        })
    }
}
/// Consuming capability bound to the preflighted package and exact current-
/// security candidate. It is deliberately neither Clone nor serializable.
pub struct ColdRestore {
    // Keep the physical pin first for cancellation/error cleanup as well as
    // successful switching. Moving fields does not run this enclosing drop order.
    _candidate: LockedFile,
    manifest: BackupManifest,
    generation: u64,
    candidate_bytes: u64,
    candidate_sha256: [u8; 32],
    candidate_revision: fc_domain::ids::Revision,
    workspace: RecoveryWorkspace,
}
impl ColdRestore {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }
    pub fn commit_cold(
        self,
        permit: crate::recovery::ColdRestorePermit,
        check: &dyn Fn() -> bool,
    ) -> Result<ColdCommitReceipt> {
        journal::commit(self, permit, check)
    }
}

#[cfg(test)]
mod tests;
