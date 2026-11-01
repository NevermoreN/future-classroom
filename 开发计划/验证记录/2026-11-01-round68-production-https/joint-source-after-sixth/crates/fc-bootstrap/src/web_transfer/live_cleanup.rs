//! Negative file cleanup on the original shared workers, with no pool shutdown.
use super::{DownloadHub, UploadHub};
use crate::web_application::WebApplication;
use fc_runtime::worker::JobContext;
use std::{sync::Arc, time::Instant};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CleanupStatus {
    pub upload_pending: usize,
    pub download_pending: usize,
    pub quarantined: usize,
    pub checked: bool,
}
impl CleanupStatus {
    pub fn is_drained(&self) -> bool {
        self.checked
            && self.upload_pending == 0
            && self.download_pending == 0
            && self.quarantined == 0
    }
}
pub struct CleanupTask {
    uploads: UploadHub,
    downloads: DownloadHub,
    app: Arc<WebApplication>,
    deadline: Instant,
}
impl CleanupTask {
    pub(crate) fn new(
        uploads: UploadHub,
        downloads: DownloadHub,
        app: Arc<WebApplication>,
        deadline: Instant,
    ) -> Self {
        Self {
            uploads,
            downloads,
            app,
            deadline,
        }
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        Some(std::mem::size_of::<Self>())
    }
    pub fn run(self, context: &JobContext) -> CleanupStatus {
        // Inactive positive transfer lifetimes are expected here. Only this fresh
        // negative worker job's context is checked, never a fabricated old context.
        let uploads = self.uploads.cleanup_live(&self.app, context);
        let downloads = self.downloads.cleanup_live(context);
        match (uploads, downloads) {
            (Ok(a), Ok(b)) => CleanupStatus {
                upload_pending: a.pending_slots,
                download_pending: b.pending_slots,
                quarantined: a.quarantined,
                checked: true,
            },
            _ => CleanupStatus::default(),
        }
    }
}
