//! Request-wide transfer lifetime and bounded correlation. No authentication,
//! native handles, file ownership or per-chunk worker ticket lives in a reply.
use fc_ports::http::RequestToken;
use fc_runtime::worker::JobContext;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    time::{Duration, Instant},
};

pub const TRANSFER_BUDGET: Duration = Duration::from_secs(120);
pub const VOICE_BUDGET: Duration = Duration::from_secs(30);
pub type ExecutionFence = Arc<dyn Fn() -> bool + Send + Sync>;

mod work;
pub use work::{WebCompletion, WebTask};
mod executor;
pub use executor::{HttpCompletion, HttpExecutor, HttpJob, HttpTicket};
#[cfg(windows)]
pub mod network;
#[cfg(all(windows, not(test)))]
mod uploads;
#[cfg(all(windows, test))]
pub(crate) mod uploads;
#[cfg(windows)]
pub use uploads::{
    UploadAdmission, UploadFinished, UploadHub, UploadHubCleanup, UploadHubError, UploadHubFailure,
    UploadHubOwner, UploadKey,
};
#[cfg(windows)]
mod upload_work;
#[cfg(windows)]
pub use upload_work::{UploadCompletion, UploadOperationResult, UploadTask};
#[cfg(windows)]
mod downloads;
#[cfg(windows)]
pub use downloads::{
    DownloadAdmission, DownloadChunk, DownloadHub, DownloadHubCleanup, DownloadHubError,
    DownloadHubFailure, DownloadHubOwner, DownloadKey,
};
#[cfg(windows)]
mod download_work;
#[cfg(windows)]
pub use download_work::{DownloadCompletion, DownloadOperationResult, DownloadTask};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferError {
    InvalidAdmission,
    Cancelled,
    Deadline,
    InvalidAcknowledgement,
}

/// The network owner retains this one control until the whole request retires.
/// Dropping it only flips an atomic; file cleanup remains a worker operation.
pub struct TransferControl {
    cancelled: Arc<AtomicBool>,
}
/// Memory-only negative signal. A network slot cannot acquire or dispose of a
/// stage through this weak handle, and dropping it does not cancel retained
/// MaterialReady state after an ordinary successful connection close.
#[derive(Clone)]
pub struct TransferCancellation {
    cancelled: Weak<AtomicBool>,
}
impl TransferCancellation {
    pub fn cancel(&self) {
        if let Some(cancelled) = self.cancelled.upgrade() {
            cancelled.store(true, Ordering::Release);
        }
    }
}
impl TransferControl {
    /// `admitted_at` is captured by the trusted transport, never decoded from a
    /// header/body, reset at worker admission, or renewed for a subsequent chunk.
    pub fn new(
        token: RequestToken,
        admitted_at: Instant,
    ) -> Result<(Self, TransferLifetime), TransferError> {
        if admitted_at > Instant::now() {
            return Err(TransferError::InvalidAdmission);
        }
        let deadline = admitted_at
            .checked_add(TRANSFER_BUDGET)
            .ok_or(TransferError::InvalidAdmission)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        Ok((
            Self {
                cancelled: cancelled.clone(),
            },
            TransferLifetime {
                token,
                admitted_at,
                deadline,
                cancelled,
            },
        ))
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn cancellation_handle(&self) -> TransferCancellation {
        TransferCancellation {
            cancelled: Arc::downgrade(&self.cancelled),
        }
    }
}
impl Drop for TransferControl {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Cloning memory-only lifetime facts does not extend the original deadline or
/// grant any lease, stage, upload permit, actor, transport or publication right.
#[derive(Clone)]
pub struct TransferLifetime {
    token: RequestToken,
    admitted_at: Instant,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}
impl TransferLifetime {
    #[cfg(windows)]
    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        self.token == other.token && Arc::ptr_eq(&self.cancelled, &other.cancelled)
    }
    pub fn token(&self) -> RequestToken {
        self.token
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn admitted_at(&self) -> Instant {
        self.admitted_at
    }
    pub fn bounded(&self, budget: Duration) -> Result<Self, TransferError> {
        if budget.is_zero() || budget > TRANSFER_BUDGET {
            return Err(TransferError::InvalidAdmission);
        }
        let limit = self
            .admitted_at
            .checked_add(budget)
            .ok_or(TransferError::InvalidAdmission)?;
        Ok(Self {
            deadline: self.deadline.min(limit),
            ..self.clone()
        })
    }
    pub fn checkpoint(&self) -> Result<(), TransferError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(TransferError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(TransferError::Deadline)
        } else {
            Ok(())
        }
    }
    pub fn is_active(&self) -> bool {
        self.checkpoint().is_ok()
    }
    pub fn execution_fence(&self) -> ExecutionFence {
        let lifetime = self.clone();
        Arc::new(move || lifetime.is_active())
    }
    /// Use only for the current worker call/queued writer operation. Never store
    /// this job-specific fence in a stage that must survive this ticket's Drop.
    pub fn job_fence(&self, context: &JobContext) -> ExecutionFence {
        let lifetime = self.clone();
        let current = context.execution_fence();
        Arc::new(move || lifetime.is_active() && current())
    }
    /// Legacy finite-local File/Cursor adapters have no network cancellation
    /// owner. They still use the same single absolute stage/validation deadline.
    #[cfg(windows)]
    pub(crate) fn local(token: RequestToken, budget: Duration) -> Result<Self, TransferError> {
        let admitted_at = Instant::now();
        let deadline = admitted_at
            .checked_add(TRANSFER_BUDGET)
            .ok_or(TransferError::InvalidAdmission)?;
        Self {
            token,
            admitted_at,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
        .bounded(budget)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadKind {
    Material,
    Voice,
}
/// Positive write completion metadata only. It cannot release a physical owner
/// and does not authorize another chunk without the receiver's exact comparison.
#[derive(Debug, PartialEq, Eq)]
pub struct UploadAck {
    token: RequestToken,
    kind: UploadKind,
    upload_id: u64,
    sequence: u64,
    next_offset: u64,
}
impl UploadAck {
    pub fn token(&self) -> RequestToken {
        self.token
    }
    pub fn kind(&self) -> UploadKind {
        self.kind
    }
    pub fn upload_id(&self) -> u64 {
        self.upload_id
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn next_offset(&self) -> u64 {
        self.next_offset
    }
    #[cfg(any(windows, test))]
    pub(crate) fn written(
        token: RequestToken,
        kind: UploadKind,
        upload_id: u64,
        sequence: u64,
        next_offset: u64,
    ) -> Self {
        Self {
            token,
            kind,
            upload_id,
            sequence,
            next_offset,
        }
    }
}

/// Network-side progress, separate from parsing or physical file ownership. A
/// transport keeps its current plaintext chunk until this cursor accepts the
/// exact positive write ACK. Failure is terminal; a replay cannot resume it.
pub struct UploadCursor {
    token: RequestToken,
    kind: UploadKind,
    upload_id: u64,
    expected: u64,
    offset: u64,
    sequence: u64,
    pending: Option<(u64, u64)>,
    failed: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub struct ChunkPosition {
    pub sequence: u64,
    pub offset: u64,
}
impl UploadCursor {
    pub fn new(
        token: RequestToken,
        kind: UploadKind,
        upload_id: u64,
        expected: u64,
    ) -> Result<Self, TransferError> {
        if upload_id == 0 || expected == 0 || expected > fc_ports::http::MAX_TRANSFER_BYTES {
            return Err(TransferError::InvalidAdmission);
        }
        Ok(Self {
            token,
            kind,
            upload_id,
            expected,
            offset: 0,
            sequence: 0,
            pending: None,
            failed: false,
        })
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn is_complete(&self) -> bool {
        !self.failed && self.pending.is_none() && self.offset == self.expected
    }
    pub fn plan_chunk(&mut self, bytes: usize) -> Result<ChunkPosition, TransferError> {
        let end = self.offset.checked_add(bytes as u64);
        let sequence = self.sequence.checked_add(1);
        if self.failed
            || self.pending.is_some()
            || bytes == 0
            || bytes > fc_ports::http::CHUNK_BYTES
            || end.is_none_or(|end| end > self.expected)
            || sequence.is_none()
        {
            self.failed = true;
            return Err(TransferError::InvalidAcknowledgement);
        }
        let sequence = sequence.ok_or(TransferError::InvalidAcknowledgement)?;
        self.pending = Some((sequence, end.ok_or(TransferError::InvalidAcknowledgement)?));
        Ok(ChunkPosition {
            sequence,
            offset: self.offset,
        })
    }
    pub fn acknowledge(&mut self, ack: UploadAck) -> Result<(), TransferError> {
        if self.failed
            || ack.token != self.token
            || ack.kind != self.kind
            || ack.upload_id != self.upload_id
            || self.pending != Some((ack.sequence, ack.next_offset))
        {
            self.failed = true;
            return Err(TransferError::InvalidAcknowledgement);
        }
        self.sequence = ack.sequence;
        self.offset = ack.next_offset;
        self.pending = None;
        Ok(())
    }
    /// Framing EOF is insufficient while a disk write is still outstanding.
    pub fn finish(&mut self) -> Result<(), TransferError> {
        if !self.is_complete() {
            self.failed = true;
            return Err(TransferError::InvalidAcknowledgement);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
