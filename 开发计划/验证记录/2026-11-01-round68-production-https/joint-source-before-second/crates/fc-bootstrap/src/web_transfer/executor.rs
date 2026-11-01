//! One heterogeneous task type/adapter contract. Production embeds this in its
//! existing Work enum; this module never creates a pool or another worker thread.
#[cfg(windows)]
use super::{DownloadCompletion, DownloadTask, UploadCompletion, UploadTask};
use super::{WebCompletion, WebTask};
use fc_runtime::worker::{JobContext, JobError, JobTicket, SubmitError, Task, WorkerHandle};
use std::time::Instant;

pub enum HttpJob {
    Web(Box<WebTask>),
    #[cfg(windows)]
    Upload(Box<UploadTask>),
    #[cfg(windows)]
    Download(Box<DownloadTask>),
}
pub enum HttpCompletion {
    Web(WebCompletion),
    #[cfg(windows)]
    Upload(UploadCompletion),
    #[cfg(windows)]
    Download(DownloadCompletion),
}
impl HttpJob {
    pub fn deadline(&self) -> Instant {
        match self {
            Self::Web(job) => job.deadline(),
            #[cfg(windows)]
            Self::Upload(job) => job.deadline(),
            #[cfg(windows)]
            Self::Download(job) => job.deadline(),
        }
    }
}
impl Task for HttpJob {
    type Output = HttpCompletion;
    fn allocation_bytes(&self) -> Option<usize> {
        match self {
            Self::Web(job) => job
                .allocation_bytes()?
                .checked_add(std::mem::size_of::<WebTask>()),
            #[cfg(windows)]
            Self::Upload(job) => job
                .allocation_bytes()?
                .checked_add(std::mem::size_of::<UploadTask>()),
            #[cfg(windows)]
            Self::Download(job) => job
                .allocation_bytes()?
                .checked_add(std::mem::size_of::<DownloadTask>()),
        }
    }
    fn run(self, context: &JobContext) -> HttpCompletion {
        match self {
            Self::Web(job) => HttpCompletion::Web(job.run(context)),
            #[cfg(windows)]
            Self::Upload(job) => HttpCompletion::Upload(job.run(context)),
            #[cfg(windows)]
            Self::Download(job) => HttpCompletion::Download(job.run(context)),
        }
    }
}
pub trait HttpTicket {
    fn cancel(&self);
    /// Actual worker disposal/transfer completion. Every positive output of this
    /// adapter is memory only, so late suppression has no native cleanup effect.
    fn is_finished(&self) -> bool;
    fn try_result(&mut self) -> Result<Option<HttpCompletion>, JobError>;
}
pub trait HttpExecutor {
    type Ticket: HttpTicket;
    fn try_submit_http(&self, task: HttpJob) -> Result<Self::Ticket, SubmitError<HttpJob>>;
}
// For a standalone integration executable with no other application pool. The
// desktop supplies an adapter for WorkerHandle<Work> to its original same pool.
impl HttpExecutor for WorkerHandle<HttpJob> {
    type Ticket = JobTicket<HttpCompletion>;
    fn try_submit_http(&self, task: HttpJob) -> Result<Self::Ticket, SubmitError<HttpJob>> {
        let deadline = task.deadline();
        self.try_submit(task, deadline)
    }
}
impl HttpTicket for JobTicket<HttpCompletion> {
    fn cancel(&self) {
        JobTicket::cancel(self);
    }
    fn is_finished(&self) -> bool {
        JobTicket::is_finished(self)
    }
    fn try_result(&mut self) -> Result<Option<HttpCompletion>, JobError> {
        JobTicket::try_result(self)
    }
}
