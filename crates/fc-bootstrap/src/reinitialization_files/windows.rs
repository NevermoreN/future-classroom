use super::*;
use fc_ports::RandomSource;
use fc_windows::{
    materials::files::{ControlledFiles, OwnedObjectPin, PinnedDirectory},
    system::DataPaths,
};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
mod journal;
mod objects;
pub use journal::{ResetCommitReceipt, ResetStartup};
use objects::*;
const NAMESPACE: &str = "reinitialization";
const OWNER: &[u8] = b"FUTURE_CLASSROOM_REINITIALIZATION_V1\n";
type Identity = (u32, u64);
fn repair(_: impl std::fmt::Debug) -> ReinitializationFileError {
    ReinitializationFileError::RepairRequired
}
fn unavailable(_: impl std::fmt::Debug) -> ReinitializationFileError {
    ReinitializationFileError::Unavailable
}
fn ensure(check: &dyn Fn() -> bool) -> Result<()> {
    if check() {
        Ok(())
    } else {
        Err(ReinitializationFileError::Cancelled)
    }
}
fn valid_operation(name: &str) -> bool {
    name.len() == 35
        && name.starts_with("op-")
        && name[3..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn write_new(files: &ControlledFiles, path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = files.create_new(path).map_err(unavailable)?;
    file.file_mut().write_all(bytes).map_err(unavailable)?;
    file.finish().map_err(unavailable)?;
    Ok(())
}
fn read_small(path: &Path, max: usize) -> Result<Vec<u8>> {
    let mut file = OwnedObjectPin::file_allow_empty(path).map_err(repair)?;
    let len = file.file().metadata().map_err(repair)?.len();
    if len > max as u64 {
        return Err(ReinitializationFileError::RepairRequired);
    }
    let mut out = Vec::with_capacity(len as usize);
    file.file_mut()
        .take(max as u64 + 1)
        .read_to_end(&mut out)
        .map_err(repair)?;
    if out.len() > max {
        return Err(ReinitializationFileError::RepairRequired);
    }
    Ok(out)
}
fn exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(repair(e)),
    }
}
/// Trusted worker-owned filesystem service. The UI keeps the InstanceGuard;
/// every capability here pins root/runtime and checks the same OS objects.
pub struct ReinitializationFiles {
    paths: DataPaths,
    files: ControlledFiles,
    root: PinnedDirectory,
    runtime: PinnedDirectory,
}
/// The original positive terminal record, checked against this physical root.
/// It is neither a reset authorization nor permission to overwrite live files.
/// No Clone/constructor: the journal module alone can mint this capability.
pub struct ResetInitializationHandoff {
    paths: DataPaths,
    operation: PathBuf,
    owner: journal::OperationOwner,
    operation_pin: PinnedDirectory,
    complete: Stamp,
    journal: Option<Stamp>,
    installed: Option<[Stamp; 13]>,
    _live: Vec<PinnedDirectory>,
}
/// Only a verified CLEAN_DONE_INSTALLED record and its original empty directory
/// set can mint this one-shot permission. Dropping it preserves all evidence.
pub struct ResetInitializationPermit {
    handoff: ResetInitializationHandoff,
}
impl ResetInitializationHandoff {
    pub fn authorize_initialization(
        self,
        files: &ReinitializationFiles,
        check: &dyn Fn() -> bool,
    ) -> Result<ResetInitializationPermit> {
        journal::verify_handoff(files, &self, true, check)?;
        Ok(ResetInitializationPermit { handoff: self })
    }
    /// Trusted composition calls this only after normal marker/key validation,
    /// DPAPI decoding and Backend::open have succeeded. The database may already
    /// be open for writing, so this service never reopens or interprets it.
    /// Only this operation's terminal metadata is retired, never teaching data.
    pub fn complete_after_backend_open(
        self,
        files: &ReinitializationFiles,
        check: &dyn Fn() -> bool,
    ) -> Result<()> {
        journal::consume_handoff(files, self, check)
    }
}
impl ResetInitializationPermit {
    /// Call at the last possible point before the existing fresh-open branch.
    /// Revalidates the original receipts and every empty installed tree.
    pub fn verify_before_initialization(
        &self,
        root: &Path,
        check: &dyn Fn() -> bool,
    ) -> Result<()> {
        if root != self.handoff.paths.root() {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let files = ReinitializationFiles::new(&self.handoff.paths)?;
        journal::verify_handoff(&files, &self.handoff, true, check)
    }
    pub fn complete_after_backend_open(
        self,
        files: &ReinitializationFiles,
        check: &dyn Fn() -> bool,
    ) -> Result<()> {
        self.handoff.complete_after_backend_open(files, check)
    }
}
/// A scope review grants no authority to read old credentials or modify data.
/// It binds the physical deployment and a UI-owned one-shot sequence only.
#[derive(Clone, Debug)]
pub struct ResetReview {
    root: PathBuf,
    sequence: u64,
    root_identity: Identity,
    runtime_identity: Identity,
    presence: u16,
}
impl ResetReview {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn present_tree_count(&self) -> usize {
        self.presence.count_ones() as usize
    }
}
/// No recursive I/O in Drop. Cancellation must explicitly call discard on a
/// worker, or let the next startup reconcile this exact protected operation.
pub struct PreparedReinitialization {
    service: ReinitializationFiles,
    operation: PathBuf,
    owner: journal::OperationOwner,
    operation_pin: Option<PinnedDirectory>,
    deferred: Option<DeferredReplacement>,
}
struct DeferredReplacement {
    review: ResetReview,
    terminal: journal::CompletedResetMetadata,
}
impl PreparedReinitialization {
    pub fn root(&self) -> &Path {
        self.service.paths.root()
    }
    pub fn sequence(&self) -> u64 {
        self.owner.sequence
    }
    pub fn commit_cold(
        mut self,
        permit: crate::reinitialization::ColdReinitializationPermit,
        check: &dyn Fn() -> bool,
    ) -> Result<ResetCommitReceipt> {
        if permit.root() != self.root() || permit.sequence() != self.sequence() {
            return Err(ReinitializationFileError::InvalidReview);
        }
        if let Some(deferred) = self.deferred.take() {
            // This branch is reachable only after the real current drain proof
            // above. Preparing/cancelling a repeated repair never retires data.
            journal::retire_for_repeat(&self.service, deferred.terminal, check)?;
            let replacement = self.service.prepare(deferred.review, check)?;
            return journal::commit(replacement, check);
        }
        journal::commit(self, check)
    }
    pub fn discard(mut self, check: &dyn Fn() -> bool) -> Result<()> {
        if self.deferred.take().is_some() {
            // No fresh operation exists; dropping the pins is the whole cancel.
            return ensure(check);
        }
        self.operation_pin.take();
        journal::discard_unarmed(&self.service, &self.operation, check)
    }
}
impl ReinitializationFiles {
    pub fn new(paths: &DataPaths) -> Result<Self> {
        paths.verify_root_for_current_user().map_err(repair)?;
        let root = PinnedDirectory::open(paths.root()).map_err(repair)?;
        let runtime = PinnedDirectory::open(&paths.runtime()).map_err(repair)?;
        paths.verify_root_for_current_user().map_err(repair)?;
        Ok(Self {
            paths: paths.clone(),
            files: ControlledFiles::new(paths.root()).map_err(repair)?,
            root,
            runtime,
        })
    }
    fn identities(&self) -> Result<(Identity, Identity)> {
        Ok((
            self.root.identity().map_err(repair)?,
            self.runtime.identity().map_err(repair)?,
        ))
    }
    fn check_scope(&self, check: &dyn Fn() -> bool) -> Result<u16> {
        ensure(check)?;
        self.paths.verify_root_for_current_user().map_err(repair)?;
        let root = PinnedDirectory::open(self.paths.root()).map_err(repair)?;
        let runtime = PinnedDirectory::open(&self.paths.runtime()).map_err(repair)?;
        if (
            root.identity().map_err(repair)?,
            runtime.identity().map_err(repair)?,
        ) != self.identities()?
        {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let mut presence = 0;
        for entry in entries(&self.files, self.paths.root(), 16)? {
            ensure(check)?;
            let name = entry
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(ReinitializationFileError::RepairRequired)?;
            if let Some(n) = OWNED_TREES.iter().position(|v| *v == name) {
                presence |= 1 << n;
            } else if name != "runtime" && name != NAMESPACE {
                return Err(ReinitializationFileError::RepairRequired);
            }
            // Directory handles only; healthy SQLite/WAL files are not opened.
            let _ = PinnedDirectory::open(&entry).map_err(repair)?;
        }
        Ok(presence)
    }
    fn namespace(&self, create: bool) -> Result<Option<PathBuf>> {
        let namespace = self.paths.root().join(NAMESPACE);
        if !exists(&namespace)? {
            if !create {
                return Ok(None);
            }
            self.paths
                .prepare_reinitialization_namespace()
                .map_err(repair)?;
        } else {
            self.paths
                .prepare_reinitialization_namespace()
                .map_err(repair)?;
        }
        let _pin = PinnedDirectory::open(&namespace).map_err(repair)?;
        let marker = namespace.join(".future-classroom-owner");
        let entries = self.files.entries(&namespace, 34).map_err(repair)?;
        if !entries.contains(&marker) {
            if !entries.is_empty() {
                return Err(ReinitializationFileError::RepairRequired);
            }
            if !create {
                return Ok(Some(namespace));
            }
            write_new(&self.files, &marker, OWNER)?;
        } else if read_small(&marker, OWNER.len())? != OWNER {
            return Err(ReinitializationFileError::RepairRequired);
        }
        for entry in entries.iter().filter(|p| **p != marker) {
            if !entry
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(valid_operation)
            {
                return Err(ReinitializationFileError::RepairRequired);
            }
            let _ = PinnedDirectory::open(entry).map_err(repair)?;
        }
        Ok(Some(namespace))
    }
    pub fn review(&self, sequence: u64, check: &dyn Fn() -> bool) -> Result<ResetReview> {
        if sequence == 0 {
            return Err(ReinitializationFileError::InvalidReview);
        }
        let presence = self.check_scope(check)?;
        let _ = self.namespace(false)?;
        let (root_identity, runtime_identity) = self.identities()?;
        Ok(ResetReview {
            root: self.paths.root().to_owned(),
            sequence,
            root_identity,
            runtime_identity,
            presence,
        })
    }
    pub fn prepare(
        &self,
        review: ResetReview,
        check: &dyn Fn() -> bool,
    ) -> Result<PreparedReinitialization> {
        ensure(check)?;
        if review.root != self.paths.root()
            || (review.root_identity, review.runtime_identity) != self.identities()?
        {
            return Err(ReinitializationFileError::InvalidReview);
        }
        let presence = self.check_scope(check)?;
        let namespace = self
            .namespace(true)?
            .ok_or(ReinitializationFileError::RepairRequired)?;
        // Keep this parent stable through fresh operation creation. The new
        // operation pin then retains this same ancestor until final retirement.
        let _namespace_pin = PinnedDirectory::open(&namespace).map_err(repair)?;
        let owner = journal::OperationOwner {
            sequence: review.sequence,
            root: review.root_identity,
            runtime: review.runtime_identity,
            presence,
        };
        if self.files.entries(&namespace, 34).map_err(repair)?.len() != 1 {
            // Only a fully checked positive metadata-only terminal can defer a
            // repeated repair. No random nonce, new operation or deletion yet.
            let terminal = journal::replacement_terminal(self, check)?
                .ok_or(ReinitializationFileError::RepairRequired)?;
            return Ok(PreparedReinitialization {
                service: Self::new(&self.paths)?,
                operation: terminal.operation.clone(),
                owner,
                operation_pin: None,
                deferred: Some(DeferredReplacement { review, terminal }),
            });
        }
        let mut random = [0; 16];
        fc_windows::SystemRandom
            .fill(&mut random)
            .map_err(unavailable)?;
        let name: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let operation = namespace.join(format!("op-{name}"));
        self.paths
            .prepare_reinitialization_operation(&operation)
            .map_err(|error| ReinitializationFileError::NativePrepare {
                stage: "operation",
                hresult: error.code().0,
            })?;
        let pin = PinnedDirectory::open(&operation).map_err(repair)?;
        write_new(&self.files, &operation.join("owner"), &owner.encode())?;
        ensure(check)?;
        self.paths
            .prepare_reinitialization_candidate(&operation.join("new"))
            .map_err(|error| ReinitializationFileError::NativePrepare {
                stage: "candidate",
                hresult: error.code().0,
            })?;
        ensure(check)?;
        Ok(PreparedReinitialization {
            service: Self::new(&self.paths)?,
            operation,
            owner,
            operation_pin: Some(pin),
            deferred: None,
        })
    }
    pub fn reconcile_startup(&self, check: &dyn Fn() -> bool) -> Result<ResetStartup> {
        journal::reconcile(self, check)
    }
    /// Call after the original startup reconciler, before ordinary recovery or
    /// fresh initialization creates anything in the installed empty tree set.
    /// A receipt whose journal was retired can only finish metadata retirement;
    /// it can never authorize a new school, even when every live tree is empty.
    pub fn initialization_handoff(
        &self,
        check: &dyn Fn() -> bool,
    ) -> Result<Option<ResetInitializationHandoff>> {
        journal::initialization_handoff(self, check)
    }
}
#[cfg(test)]
mod tests;
