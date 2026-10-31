//! Download jobs for the existing shared task enum. No pool/thread is created.
use super::{
    downloads::{
        DownloadAdmission, DownloadChunk, DownloadHub, DownloadHubError, DownloadHubFailure,
        DownloadKey,
    },
    work::head_allocation,
    TransferError, TransferLifetime,
};
use crate::{backend::Backend, web_application::WebApplication};
use fc_ports::http::{RequestHead, RequestToken, MAX_TARGET_BYTES};
use fc_runtime::worker::{JobContext, Task};
use std::{
    sync::{Arc, Mutex, Weak},
    time::Instant,
};

enum Operation {
    Begin {
        application: Weak<WebApplication>,
        backend: Weak<Mutex<Backend>>,
        head: RequestHead,
        life: TransferLifetime,
    },
    Read {
        key: DownloadKey,
        sequence: u64,
        offset: u64,
    },
    CleanupRequest,
}
pub struct DownloadTask {
    hub: DownloadHub,
    token: RequestToken,
    deadline: Instant,
    operation: Operation,
}
pub enum DownloadOperationResult {
    Admitted(DownloadAdmission),
    Read(DownloadChunk),
    CleanedRequest(RequestToken),
}
pub struct DownloadCompletion {
    pub token: RequestToken,
    pub result: Result<DownloadOperationResult, DownloadHubFailure>,
}
impl DownloadTask {
    pub fn begin(
        hub: DownloadHub,
        application: &Arc<WebApplication>,
        backend: &Arc<Mutex<Backend>>,
        head: RequestHead,
        life: TransferLifetime,
    ) -> Result<Self, TransferError> {
        if head.token != life.token()
            || head.method != "GET"
            || head.has_body
            || head.content_length.is_some_and(|value| value != 0)
            || head.target.len() > MAX_TARGET_BYTES
            || fc_ports::http::validate_headers(&head.headers).is_err()
        {
            return Err(TransferError::InvalidAdmission);
        }
        Ok(Self {
            token: head.token,
            deadline: life.deadline(),
            hub,
            operation: Operation::Begin {
                application: Arc::downgrade(application),
                backend: Arc::downgrade(backend),
                head,
                life,
            },
        })
    }
    pub fn read(
        hub: DownloadHub,
        key: DownloadKey,
        sequence: u64,
        offset: u64,
        deadline: Instant,
    ) -> Result<Self, TransferError> {
        if sequence == 0 || offset > fc_ports::http::MAX_TRANSFER_BYTES {
            return Err(TransferError::InvalidAdmission);
        }
        Ok(Self {
            hub,
            token: key.token(),
            deadline,
            operation: Operation::Read {
                key,
                sequence,
                offset,
            },
        })
    }
    pub fn cleanup_request(hub: DownloadHub, token: RequestToken, deadline: Instant) -> Self {
        Self {
            hub,
            token,
            deadline,
            operation: Operation::CleanupRequest,
        }
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    fn execute(self, context: &JobContext) -> Result<DownloadOperationResult, DownloadHubFailure> {
        context
            .checkpoint()
            .map_err(|_| DownloadHubError::Cancelled)?;
        if context.deadline() > self.deadline {
            return Err(DownloadHubError::Invalid.into());
        }
        match self.operation {
            Operation::Begin {
                application,
                backend,
                head,
                life,
            } => {
                let app = application.upgrade().ok_or(DownloadHubError::Closed)?;
                let backend = backend.upgrade().ok_or(DownloadHubError::Closed)?;
                self.hub
                    .begin(&app, &backend, &head, life, context)
                    .map(DownloadOperationResult::Admitted)
            }
            Operation::Read {
                key,
                sequence,
                offset,
            } => self
                .hub
                .read(key, sequence, offset, context)
                .map(DownloadOperationResult::Read),
            Operation::CleanupRequest => {
                self.hub.cleanup_request(self.token, context)?;
                Ok(DownloadOperationResult::CleanedRequest(self.token))
            }
        }
    }
}
impl Task for DownloadTask {
    type Output = DownloadCompletion;
    fn allocation_bytes(&self) -> Option<usize> {
        match &self.operation {
            Operation::Begin { head, .. } => head_allocation(head),
            Operation::Read { .. } | Operation::CleanupRequest => Some(0),
        }
    }
    fn run(self, context: &JobContext) -> DownloadCompletion {
        let token = self.token;
        DownloadCompletion {
            token,
            result: self.execute(context),
        }
    }
}

#[cfg(test)]
mod tests;
