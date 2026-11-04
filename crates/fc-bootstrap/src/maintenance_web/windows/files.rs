//! Backup streams own only server-selected, pinned staging files. Invoke every
//! operation and retirement on the existing bounded worker pool.
use super::*;
use fc_security::Sha256Stream;
use fc_windows::materials::files::{LockedFile, NewFile};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

pub struct UploadedBackup {
    pin: Option<LockedFile>,
    path: PathBuf,
    identity: (u32, u64),
    files: ControlledFiles,
}
impl UploadedBackup {
    pub(super) fn take_pin(&mut self) -> Result<LockedFile> {
        self.pin.take().ok_or(MaintenanceError::Conflict)
    }
    pub(super) fn retire(&mut self) -> Result<()> {
        self.pin.take();
        self.files
            .remove_object_expected(&self.path, false, self.identity, |_| true)
            .map_err(|_| MaintenanceError::Retained)?;
        Ok(())
    }
}
pub struct MaintenanceUpload {
    service: Arc<MaintenanceService>,
    job: JobContext,
    file: Option<NewFile>,
    path: PathBuf,
    identity: (u32, u64),
    finished: bool,
    files: ControlledFiles,
    expected: u64,
    offset: u64,
    hash: Sha256Stream,
    access: OrdinaryBackupAccess,
}
impl MaintenanceUpload {
    pub fn start(
        service: Arc<MaintenanceService>,
        id: u64,
        proof: &WebSecret,
        lease: SessionLease,
        access: OrdinaryBackupAccess,
        context: &WorkerContext,
    ) -> Result<Self> {
        let job = service.context(id, proof, lease)?;
        let active = service
            .active
            .try_lock()
            .map_err(|_| MaintenanceError::Busy)?;
        let state = active
            .as_ref()
            .filter(|v| v.job.id() == id && v.kind == Kind::Import)
            .ok_or(MaintenanceError::Conflict)?;
        if service.control.snapshot(id, proof)?.phase != Phase::ReadyForUpload {
            return Err(MaintenanceError::Conflict);
        }
        if context.checkpoint().is_err() || !job.active() || access.check_revision().is_err() {
            return Err(MaintenanceError::Denied);
        }
        let expected = state.expected_bytes.ok_or(MaintenanceError::Invalid)?;
        let files = ControlledFiles::new(service.paths.root())
            .map_err(|_| MaintenanceError::Unavailable)?;
        let dir = files
            .directory(&["quarantine".into(), "maintenance-web".into()])
            .map_err(|_| MaintenanceError::Unavailable)?;
        // Job identifiers never reuse within this process. An abandoned older
        // staging entry is never overwritten or trusted as a new upload.
        let file = files
            .create_new(&dir.join(format!("upload-{}.fcbackup", state.stage_name)))
            .map_err(|_| MaintenanceError::Unavailable)?;
        let path = file.path().to_owned();
        let identity = file.identity().map_err(|_| MaintenanceError::Unavailable)?;
        job.update(Phase::Uploading, |_| {})?;
        drop(active);
        Ok(Self {
            service,
            job,
            file: Some(file),
            path,
            identity,
            finished: false,
            files,
            expected,
            offset: 0,
            hash: Sha256Stream::default(),
            access,
        })
    }
    pub fn abort(&mut self) -> bool {
        if self.finished {
            return true;
        }
        drop(self.file.take());
        if self
            .files
            .remove_object_expected(&self.path, false, self.identity, |_| true)
            .is_err()
        {
            return false;
        }
        self.finished = true;
        let _ = self.job.fail("备份上传已取消，请重新选择文件。");
        true
    }
    pub fn bytes(&self) -> u64 {
        self.expected
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        Some(std::mem::size_of::<Self>() + 4096)
    }
    pub fn write_chunk(
        &mut self,
        offset: u64,
        bytes: &[u8],
        context: &WorkerContext,
    ) -> Result<()> {
        if context.checkpoint().is_err()
            || !self.job.active()
            || self.access.check_revision().is_err()
        {
            return Err(MaintenanceError::Denied);
        }
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or(MaintenanceError::Invalid)?;
        if offset != self.offset
            || bytes.is_empty()
            || bytes.len() > fc_ports::http::CHUNK_BYTES
            || end > self.expected
        {
            return Err(MaintenanceError::Invalid);
        }
        self.file
            .as_mut()
            .ok_or(MaintenanceError::Conflict)?
            .file_mut()
            .write_all(bytes)
            .map_err(|_| MaintenanceError::Unavailable)?;
        self.hash.update(bytes);
        self.offset = end;
        Ok(())
    }
    pub fn finish(&mut self, context: &WorkerContext) -> Result<Snapshot> {
        if self.offset != self.expected
            || context.checkpoint().is_err()
            || !self.job.active()
            || self.access.check_revision().is_err()
        {
            return Err(MaintenanceError::Denied);
        }
        let mut active = self
            .service
            .active
            .try_lock()
            .map_err(|_| MaintenanceError::Busy)?;
        let state = active
            .as_mut()
            .filter(|v| v.job.id() == self.job.id())
            .ok_or(MaintenanceError::Stale)?;
        if state.uploaded.is_some() {
            return Err(MaintenanceError::Conflict);
        }
        let files = ControlledFiles::new(self.service.paths.root())
            .map_err(|_| MaintenanceError::Unavailable)?;
        let pin = self
            .file
            .take()
            .ok_or(MaintenanceError::Conflict)?
            .seal()
            .map_err(|_| MaintenanceError::Unavailable)?;
        state.uploaded = Some(UploadedBackup {
            pin: Some(pin),
            path: self.path.clone(),
            identity: self.identity,
            files,
        });
        self.finished = true;
        self.job.update(Phase::Uploaded, |s| {
            s.sha256 = Some(hex(&std::mem::take(&mut self.hash).finish()))
        })?;
        self.job.snapshot()
    }
}
pub struct MaintenanceDownload {
    file: LockedFile,
    job: JobContext,
    access: Arc<OrdinaryBackupAccess>,
    bytes: u64,
    offset: u64,
    sha256: [u8; 32],
}
impl MaintenanceDownload {
    pub fn open(
        service: &MaintenanceService,
        id: u64,
        proof: &WebSecret,
        lease: SessionLease,
        access: OrdinaryBackupAccess,
        context: &WorkerContext,
    ) -> Result<Self> {
        let job = service.context(id, proof, lease)?;
        let active = service
            .active
            .try_lock()
            .map_err(|_| MaintenanceError::Busy)?;
        let state = active
            .as_ref()
            .filter(|v| v.job.id() == id && v.kind == Kind::Export)
            .ok_or(MaintenanceError::Stale)?;
        let receipt = state.exported.as_ref().ok_or(MaintenanceError::Conflict)?;
        let mut file = LockedFile::open(&receipt.path, wire::MAX_PACKAGE_BYTES)
            .map_err(|_| MaintenanceError::Unavailable)?;
        if file.len().map_err(|_| MaintenanceError::Unavailable)? != receipt.bytes {
            return Err(MaintenanceError::Conflict);
        }
        let mut hash = Sha256Stream::default();
        let mut buffer = [0u8; fc_ports::http::CHUNK_BYTES];
        loop {
            if context.checkpoint().is_err() || !job.active() || access.check_revision().is_err() {
                return Err(MaintenanceError::Denied);
            }
            let n = file
                .file_mut()
                .read(&mut buffer)
                .map_err(|_| MaintenanceError::Unavailable)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if hash.finish() != receipt.package_sha256 {
            return Err(MaintenanceError::Conflict);
        }
        file.file_mut()
            .seek(SeekFrom::Start(0))
            .map_err(|_| MaintenanceError::Unavailable)?;
        Ok(Self {
            file,
            job,
            access: Arc::new(access),
            bytes: receipt.bytes,
            offset: 0,
            sha256: receipt.package_sha256,
        })
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn send_fence(&self) -> ExecutionFence {
        let job = self.job.clone();
        let access = self.access.clone();
        Arc::new(move || job.active() && access.check_revision().is_ok())
    }
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        Some(std::mem::size_of::<Self>() + 4096)
    }
    pub fn read_chunk(
        &mut self,
        offset: u64,
        maximum: usize,
        context: &WorkerContext,
    ) -> Result<Vec<u8>> {
        if context.checkpoint().is_err()
            || !self.job.active()
            || self.access.check_revision().is_err()
        {
            return Err(MaintenanceError::Denied);
        }
        if offset != self.offset || maximum == 0 || maximum > fc_ports::http::CHUNK_BYTES {
            return Err(MaintenanceError::Invalid);
        }
        let n = (self.bytes - self.offset).min(maximum as u64) as usize;
        let mut bytes = vec![0; n];
        self.file
            .file_mut()
            .read_exact(&mut bytes)
            .map_err(|_| MaintenanceError::Unavailable)?;
        self.offset += n as u64;
        Ok(bytes)
    }
}
