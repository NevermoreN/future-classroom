//! Captured local-administrator permissions. Slow projection work never holds
//! the Backend mutex. A confirmed restore closes ordinary admission for this
//! process; it does not issue a replacement session or reopen the network.
use super::*;
use fc_domain::recovery::{BackupEntry, BackupManifest};
use fc_storage::recovery::{ExecutionFence, RecoveryBarrier, RecoveryDatabaseReceipt};
use std::path::PathBuf;

pub struct OrdinaryBackupAccess {
    repository: Repository,
    actor: ActorContext,
    authority: AuthorityReader<IdentityState>,
    session: fc_storage::business::SessionGuard,
    expected_revision: Revision,
    clock: Arc<dyn Clock + Send + Sync>,
    installation_id: [u8; 32],
}
impl OrdinaryBackupAccess {
    /// Revalidate after pinning every immutable object. A revision change during
    /// pin acquisition rejects the package rather than combining generations.
    pub fn check_revision(&self) -> Result<()> {
        let snapshot = self
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        if snapshot.revision != self.expected_revision {
            return Err(StorageError::Conflict {
                current: snapshot.revision,
            }
            .into());
        }
        (self.session)(&snapshot.data, &self.actor)?;
        snapshot
            .data
            .authorize_management(&self.actor, self.clock.now())
            .map_err(StorageError::Identity)?;
        Ok(())
    }
    /// Destination is a new, protected, pinned staging file selected by the
    /// deployment adapter. Hold every media pin through final package publish.
    pub fn export_database(
        &self,
        destination: PathBuf,
        fence: ExecutionFence,
    ) -> Result<fc_storage::Ticket<RecoveryDatabaseReceipt>> {
        self.check_revision()?;
        Ok(self
            .repository
            .try_export_ordinary(destination, &self.actor, fence)?)
    }
    pub fn expected_revision(&self) -> Revision {
        self.expected_revision
    }
    /// Full typed isolation preflight. The file adapter retains its input pin;
    /// the preview database contains CURRENT local security and never leaves
    /// protected staging. It grants no permission to replace live content.
    pub fn preflight_restore(
        &self,
        source: PathBuf,
        destination: PathBuf,
        manifest: BackupManifest,
        manifest_sha256: [u8; 32],
        fence: ExecutionFence,
    ) -> Result<RestorePreview> {
        self.check_revision()?;
        manifest
            .validate()
            .map_err(|_| BackendError::InvalidInput)?;
        let encoded = fc_protocol::recovery::encode_manifest(&manifest)
            .map_err(|_| BackendError::InvalidInput)?;
        if fc_security::sha256(&encoded) != manifest_sha256 {
            return Err(BackendError::InvalidInput);
        }
        if manifest.installation_id != self.installation_id
            || manifest.school_id != self.actor.school_id
        {
            return Err(StorageError::RecoveryConflict.into());
        }
        let receipt = self
            .repository
            .try_preflight_ordinary_restore(source.clone(), destination, &self.actor, fence)?
            .wait()?;
        self.check_revision()?;
        if receipt.current_revision != self.expected_revision
            || !manifest_matches(&manifest, &receipt)
        {
            return Err(StorageError::RecoveryInvalidPackage.into());
        }
        Ok(RestorePreview {
            manifest,
            manifest_sha256,
            current_revision: receipt.current_revision,
            source_database: source,
            document_count: receipt.document_count,
        })
    }
}

pub struct RestorePreview {
    manifest: BackupManifest,
    manifest_sha256: [u8; 32],
    current_revision: Revision,
    source_database: PathBuf,
    document_count: u64,
}
impl RestorePreview {
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }
    pub fn current_revision(&self) -> Revision {
        self.current_revision
    }
    pub fn document_count(&self) -> u64 {
        self.document_count
    }
}

pub struct RestoreConfirmation {
    access: OrdinaryBackupAccess,
    control: SessionControl,
    web: web::WebService,
    voice: VoiceService,
    ledger: Arc<Mutex<Ledger>>,
    database_path: PathBuf,
    manifest: BackupManifest,
    manifest_sha256: [u8; 32],
    source_database: PathBuf,
}
pub struct ConfirmedRestore {
    repository: Repository,
    barrier: RecoveryBarrier,
    database_path: PathBuf,
    manifest: BackupManifest,
    manifest_sha256: [u8; 32],
    source_database: PathBuf,
}
/// Created only by a successfully confirmed, exact-floor writer preparation.
/// The file adapter must retain the candidate's physical pin through switching.
pub struct PreparedRecoveryDatabase {
    pub(crate) live_database: PathBuf,
    pub(crate) candidate_database: PathBuf,
    pub(crate) receipt: RecoveryDatabaseReceipt,
    pub(crate) manifest_sha256: [u8; 32],
}
impl PreparedRecoveryDatabase {
    pub fn receipt(&self) -> &RecoveryDatabaseReceipt {
        &self.receipt
    }
    pub fn manifest_sha256(&self) -> [u8; 32] {
        self.manifest_sha256
    }
    pub fn candidate_database(&self) -> &Path {
        &self.candidate_database
    }
}
impl RestoreConfirmation {
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }
    pub fn manifest_sha256(&self) -> [u8; 32] {
        self.manifest_sha256
    }
    /// Invoke on the shared worker after explicit review. The security barrier
    /// is durable in the ORIGINAL database before any candidate can replace it.
    /// Even a later rollback retains this advanced authorization floor.
    pub fn confirm(self, fence: ExecutionFence) -> Result<ConfirmedRestore> {
        self.access.check_revision()?;
        self.web.close();
        self.voice.invalidate_runtime();
        let result = self
            .access
            .repository
            .try_advance_recovery_barrier(self.access.expected_revision, &self.access.actor, fence)
            .and_then(fc_storage::Ticket::wait);
        let barrier = match result {
            Ok(barrier) => barrier,
            Err(error) => {
                if matches!(
                    error,
                    StorageError::Indeterminate
                        | StorageError::PublicationFailed { .. }
                        | StorageError::WorkerPanicked
                        | StorageError::AuthorityUnavailable
                ) {
                    self.control.fault();
                }
                return Err(error.into());
            }
        };
        // Keep trusted audio interruption possible until its native ownership
        // has drained. Ordinary local/Web operations are already denied.
        self.control.0.recovering.store(true, Ordering::Release);
        self.control.lock_local();
        self.web.close();
        self.voice.invalidate_runtime();
        match self.ledger.lock() {
            Ok(mut ledger) => {
                ledger
                    .sessions
                    .apply_events(&[IdentityEvent::AllAuthorizationRevoked]);
                ledger.local = None;
            }
            Err(_) => {
                self.control.fault();
                return Err(BackendError::Closed);
            }
        }
        Ok(ConfirmedRestore {
            repository: self.access.repository,
            barrier,
            database_path: self.database_path,
            manifest: self.manifest,
            manifest_sha256: self.manifest_sha256,
            source_database: self.source_database,
        })
    }
}
impl ConfirmedRestore {
    pub(crate) fn live_database(&self) -> &Path {
        &self.database_path
    }
    /// The ordinary input and candidate paths must remain physically pinned.
    /// Failure leaves user admission closed; restart uses the original database
    /// with its advanced security floor. This method never mutates live content.
    pub(crate) fn prepare_database(
        self,
        destination: PathBuf,
        fence: ExecutionFence,
    ) -> Result<PreparedRecoveryDatabase> {
        let receipt = self
            .repository
            .try_prepare_ordinary_restore(
                self.source_database,
                destination.clone(),
                self.barrier,
                fence,
            )?
            .wait()?;
        if !manifest_matches(&self.manifest, &receipt) {
            return Err(StorageError::RecoveryInvalidPackage.into());
        }
        Ok(PreparedRecoveryDatabase {
            live_database: self.database_path,
            candidate_database: destination,
            receipt,
            manifest_sha256: self.manifest_sha256,
        })
    }
}
fn manifest_matches(manifest: &BackupManifest, receipt: &RecoveryDatabaseReceipt) -> bool {
    matches!(manifest.entries.first(),Some(BackupEntry::Database{bytes,sha256})
        if *bytes==receipt.source_database_bytes && *sha256==receipt.source_database_sha256)
        && receipt.school_id == manifest.school_id
        && receipt.installation_id == manifest.installation_id
        && receipt.source_revision == manifest.revision
        && media_match(&manifest.entries[1..], &receipt.media)
}
fn media_match(expected: &[BackupEntry], actual: &[BackupEntry]) -> bool {
    fn key(entry: &BackupEntry) -> Option<(u8, u64)> {
        match entry {
            BackupEntry::Database { .. } => None,
            BackupEntry::Material { version_id, .. } => Some((0, version_id.get())),
            BackupEntry::ProfileMedia { version_id, .. } => Some((1, version_id.get())),
        }
    }
    if expected.len() != actual.len() {
        return false;
    }
    let mut entries = std::collections::BTreeMap::new();
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
impl Backend {
    pub(crate) fn recovery_database_path(&self) -> &Path {
        &self.database_path
    }
    pub fn ordinary_backup_access(
        &self,
        lease: SessionLease,
        expected_revision: Revision,
    ) -> Result<OrdinaryBackupAccess> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        state
            .authorize_management(&actor, self.clock.now())
            .map_err(StorageError::Identity)?;
        let access = OrdinaryBackupAccess {
            repository: self.guarded(lease, generation, SessionPurpose::Manage),
            actor,
            authority: self.authority.clone(),
            session: self.session_guard(lease, generation, SessionPurpose::Manage),
            expected_revision,
            clock: self.clock.clone(),
            installation_id: self.repository.recovery_installation()?,
        };
        access.check_revision()?;
        Ok(access)
    }
    /// Bind the immutable, fully preflighted package shown in the native review.
    /// Framing alone does not authenticate a package or grant restore permission.
    pub fn review_preflighted_restore(
        &self,
        lease: SessionLease,
        preview: RestorePreview,
    ) -> Result<RestoreConfirmation> {
        self.review_ordinary_restore(
            lease,
            preview.current_revision,
            preview.manifest,
            preview.manifest_sha256,
            preview.source_database,
        )
    }
    fn review_ordinary_restore(
        &self,
        lease: SessionLease,
        expected_revision: Revision,
        manifest: BackupManifest,
        manifest_sha256: [u8; 32],
        source_database: PathBuf,
    ) -> Result<RestoreConfirmation> {
        manifest
            .validate()
            .map_err(|_| BackendError::InvalidInput)?;
        let encoded = fc_protocol::recovery::encode_manifest(&manifest)
            .map_err(|_| BackendError::InvalidInput)?;
        if fc_security::sha256(&encoded) != manifest_sha256 {
            return Err(BackendError::InvalidInput);
        }
        let access = self.ordinary_backup_access(lease, expected_revision)?;
        if self.repository.recovery_installation()? != manifest.installation_id {
            return Err(StorageError::RecoveryConflict.into());
        }
        let snapshot = self
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        if snapshot.data.installation.as_ref().map(|i| i.school_id) != Some(manifest.school_id) {
            return Err(StorageError::RecoveryConflict.into());
        }
        Ok(RestoreConfirmation {
            access,
            control: self.control.clone(),
            web: self.web.clone(),
            voice: self.voice.clone(),
            ledger: self.ledger.clone(),
            database_path: self.database_path.clone(),
            manifest,
            manifest_sha256,
            source_database,
        })
    }
}

#[cfg(test)]
mod tests;
