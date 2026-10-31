//! Physical shutdown proof for the deployment's cold restore. Cancellation or
//! a published Idle status never substitutes for the real worker/audio joins.
use crate::{
    backend::{Backend, BackendError, ConfirmedRestore, PreparedRecoveryDatabase},
    voice_runtime::{RuntimePhase, VoiceRuntime},
};
use fc_runtime::worker::{Task, WorkerPool};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupError {
    Backend(BackendError),
    Files(crate::recovery_files::RecoveryFileError),
}

/// Run on the existing shared file worker. The source metadata revision is
/// checked again after ALL immutable originals have been pinned; no Backend
/// mutex spans SQLite projection, file copying, decoders or package publication.
#[cfg(windows)]
pub fn export_ordinary_backup(
    paths: &fc_windows::system::DataPaths,
    access: crate::backend::OrdinaryBackupAccess,
    execution: fc_storage::recovery::ExecutionFence,
) -> Result<crate::recovery_files::ExportReceipt, BackupError> {
    let access = Arc::new(access);
    let bound = access.clone();
    let fence: fc_storage::recovery::ExecutionFence = Arc::new(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execution())).unwrap_or(false)
            && bound.check_revision().is_ok()
    });
    let files = crate::recovery_files::RecoveryFiles::new(paths).map_err(BackupError::Files)?;
    let workspace = files.workspace().map_err(BackupError::Files)?;
    let ordinary = workspace.ordinary_database();
    let receipt = access
        .export_database(ordinary.clone(), fence.clone())
        .map_err(BackupError::Backend)?
        .wait()
        .map_err(|e| BackupError::Backend(e.into()))?;
    access.check_revision().map_err(BackupError::Backend)?;
    let pinned = files
        .pin_export(&receipt, &ordinary, &*fence)
        .map_err(BackupError::Files)?;
    access.check_revision().map_err(BackupError::Backend)?;
    files
        .export_pinned(pinned, &*fence)
        .map_err(BackupError::Files)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShutdownError {
    WrongDeployment,
    NotRecovering,
    AlreadySettled,
    Worker,
    Audio,
    Backend(BackendError),
}

/// Non-Clone proof consumed by the fixed-path file adapter. There is no public
/// constructor and no boolean shortcut for a still-running physical task.
pub struct ColdRestorePermit {
    prepared: PreparedRecoveryDatabase,
}
impl ColdRestorePermit {
    pub fn prepared_database(&self) -> &PreparedRecoveryDatabase {
        &self.prepared
    }
    pub fn live_database(&self) -> &Path {
        &self.prepared.live_database
    }
    pub fn candidate_database(&self) -> &Path {
        &self.prepared.candidate_database
    }
    pub fn generation(&self) -> u64 {
        self.prepared.receipt.recovery_generation
    }
    pub fn revision(&self) -> fc_domain::ids::Revision {
        self.prepared.receipt.revision
    }
    pub fn manifest_sha256(&self) -> [u8; 32] {
        self.prepared.manifest_sha256
    }
}
pub struct ColdShutdown<T: Task> {
    backend: Arc<Mutex<Backend>>,
    workers: Option<WorkerPool<T>>,
    audio: VoiceRuntime,
    confirmed: Option<ConfirmedRestore>,
    failed: bool,
}
impl<T: Task> ColdShutdown<T> {
    /// The caller owns InstanceGuard and has physically drained external IO.
    /// Production coordinators retain every physical owner even if deployment
    /// validation fails. The accompanying error forbids switching, but cannot
    /// release a live worker or audio thread.
    pub fn begin_retained(
        confirmed: ConfirmedRestore,
        backend: Arc<Mutex<Backend>>,
        workers: WorkerPool<T>,
        audio: VoiceRuntime,
    ) -> (Self, Option<ShutdownError>) {
        let error = match backend.lock() {
            Ok(owned) if owned.recovery_database_path() != confirmed.live_database() => {
                Some(ShutdownError::WrongDeployment)
            }
            Ok(owned) if !owned.session_control().is_recovering() => {
                Some(ShutdownError::NotRecovering)
            }
            Ok(_) => None,
            Err(_) => Some(ShutdownError::Backend(BackendError::Closed)),
        };
        workers.request_stop();
        audio.shutdown();
        let failed = error.is_some();
        (
            Self {
                backend,
                workers: Some(workers),
                audio,
                confirmed: Some(confirmed),
                failed,
            },
            error,
        )
    }
    /// Poll from the shutdown coordinator, outside the UI and shared workers.
    /// None retains this SAME ownership. No deadline frees a live native owner.
    pub fn poll(&mut self) -> Result<Option<DrainedRestore>, ShutdownError> {
        if self.failed || self.confirmed.is_none() {
            return Err(ShutdownError::AlreadySettled);
        }
        let result = self.poll_owned();
        if result.is_err() {
            self.failed = true;
            if let Ok(backend) = self.backend.lock() {
                backend.session_control().fault();
            }
        }
        result
    }
    /// Fault settlement only, on the independent coordinator. A logical error
    /// does not prove any native owner has returned. Never publishes a permit.
    pub fn finish_failed_nonblocking(&mut self) -> Result<bool, ShutdownError> {
        if !self.failed {
            return Err(ShutdownError::NotRecovering);
        }
        let _ = self.audio.reap_nonblocking();
        let status = match self.audio.status() {
            Ok(status) => status,
            Err(_) => return Ok(false),
        };
        if status.phase != RuntimePhase::Closed
            || status.physical_thread_running
            || status.ack_generation < status.stop_generation
            || self
                .workers
                .as_ref()
                .is_some_and(|pool| !pool.is_finished_with(crate::physical_thread::is_finished))
        {
            return Ok(false);
        }
        if let Some(pool) = self.workers.take() {
            // A faulted result still joins both already-finished threads. The
            // original operation remains failed; this only settles ownership.
            let _ = pool.join();
        }
        self.backend
            .lock()
            .map_err(|_| ShutdownError::Backend(BackendError::Closed))?
            .close()
            .map_err(ShutdownError::Backend)?;
        self.confirmed.take();
        Ok(true)
    }
    fn poll_owned(&mut self) -> Result<Option<DrainedRestore>, ShutdownError> {
        self.audio
            .reap_nonblocking()
            .map_err(|_| ShutdownError::Audio)?;
        let status = self.audio.status().map_err(|_| ShutdownError::Audio)?;
        if status.faulted || matches!(status.phase, RuntimePhase::Retained | RuntimePhase::Halted) {
            return Err(ShutdownError::Audio);
        }
        if status.phase != RuntimePhase::Closed
            || status.physical_thread_running
            || status.ack_generation < status.stop_generation
        {
            return Ok(None);
        }
        if self
            .workers
            .as_ref()
            .is_some_and(|pool| !pool.is_finished_with(crate::physical_thread::is_finished))
        {
            return Ok(None);
        }
        if let Some(pool) = self.workers.take() {
            pool.join().map_err(|_| ShutdownError::Worker)?;
        }
        // Keep the writer alive for one final candidate built from the latest
        // current security state, including any KDF/voice completion during drain.
        Ok(Some(DrainedRestore {
            backend: self.backend.clone(),
            confirmed: self.confirmed.take().ok_or(ShutdownError::AlreadySettled)?,
        }))
    }
}

/// Exclusive post-drain coordinator. The candidate is deliberately constructed
/// AFTER physical completion so late login failures cannot be rolled back.
pub struct DrainedRestore {
    backend: Arc<Mutex<Backend>>,
    confirmed: ConfirmedRestore,
}
impl DrainedRestore {
    pub fn prepare_database(
        self,
        destination: std::path::PathBuf,
        fence: fc_storage::recovery::ExecutionFence,
    ) -> Result<ColdRestorePermit, ShutdownError> {
        let prepared = self.confirmed.prepare_database(destination, fence);
        // Failure also closes the writer. The original content and advanced
        // security floor remain authoritative for the next controlled startup.
        self.backend
            .lock()
            .map_err(|_| ShutdownError::Backend(BackendError::Closed))?
            .close()
            .map_err(ShutdownError::Backend)?;
        Ok(ColdRestorePermit {
            prepared: prepared.map_err(ShutdownError::Backend)?,
        })
    }
}

#[cfg(all(test, windows))]
mod integration_windows;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
