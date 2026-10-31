//! Stage jobs composed into the SAME application worker enum as head/JSON work.
//! Inputs are already-arrived bounded bytes and weak composition references.
use super::{
    uploads::{
        UploadAdmission, UploadFinished, UploadHub, UploadHubError, UploadHubFailure, UploadKey,
    },
    work::head_allocation,
    TransferControl, TransferError, TransferLifetime, UploadAck, UploadKind,
};
use crate::{
    backend::Backend,
    web_application::{WebApplication, WebResponse},
};
use fc_ports::http::{RequestHead, RequestToken, CHUNK_BYTES};
use fc_runtime::worker::{JobContext, Task};
use std::{
    sync::{Arc, Mutex, Weak},
    time::Instant,
};

#[cfg(test)]
mod tests;

enum Operation {
    Begin {
        kind: UploadKind,
        head: RequestHead,
        control: TransferControl,
        life: TransferLifetime,
    },
    Write {
        key: UploadKey,
        sequence: u64,
        offset: u64,
        bytes: Vec<u8>,
    },
    Finish(UploadKey),
    Append {
        key: Option<UploadKey>,
        backend: Weak<Mutex<Backend>>,
        head: RequestHead,
        body: Vec<u8>,
        life: TransferLifetime,
    },
    Cleanup(UploadKey),
    CleanupRequest,
}
pub struct UploadTask {
    hub: UploadHub,
    application: Weak<WebApplication>,
    token: RequestToken,
    deadline: Instant,
    operation: Operation,
}
pub enum UploadOperationResult {
    Admitted(UploadAdmission),
    Written(UploadAck),
    Finished(UploadFinished),
    Appended(WebResponse),
    Cleaned(UploadKey),
    CleanedRequest(RequestToken),
}
/// Every variant is metadata/bytes/atomic control only. A canceled positive
/// reply dropped on the network caller cannot retire a native owner or permit.
pub struct UploadCompletion {
    pub token: RequestToken,
    pub result: Result<UploadOperationResult, UploadHubFailure>,
}
impl UploadTask {
    pub fn begin(
        hub: UploadHub,
        application: &Arc<WebApplication>,
        kind: UploadKind,
        head: RequestHead,
        control: TransferControl,
        life: TransferLifetime,
    ) -> Result<Self, TransferError> {
        if head.token != life.token()
            || !Arc::ptr_eq(&control.cancelled, &life.cancelled)
            || head
                .content_length
                .is_none_or(|n| n == 0 || n > fc_ports::http::MAX_TRANSFER_BYTES)
            || fc_ports::http::validate_headers(&head.headers).is_err()
            || head.target.len() > fc_ports::http::MAX_TARGET_BYTES
            || head.method.len() > 16
        {
            return Err(TransferError::InvalidAdmission);
        }
        let deadline = if kind == UploadKind::Voice {
            life.bounded(super::VOICE_BUDGET)?.deadline()
        } else {
            life.deadline()
        };
        Ok(Self {
            hub,
            application: Arc::downgrade(application),
            token: head.token,
            deadline,
            operation: Operation::Begin {
                kind,
                head,
                control,
                life,
            },
        })
    }
    pub fn write(
        hub: UploadHub,
        key: UploadKey,
        sequence: u64,
        offset: u64,
        bytes: Vec<u8>,
        deadline: Instant,
    ) -> Result<Self, TransferError> {
        if bytes.is_empty() || bytes.len() > CHUNK_BYTES || sequence == 0 {
            return Err(TransferError::InvalidAdmission);
        }
        Ok(Self {
            hub,
            application: Weak::new(),
            token: key.token(),
            deadline,
            operation: Operation::Write {
                key,
                sequence,
                offset,
                bytes,
            },
        })
    }
    pub fn finish(
        hub: UploadHub,
        application: &Arc<WebApplication>,
        key: UploadKey,
        deadline: Instant,
    ) -> Self {
        Self {
            hub,
            application: Arc::downgrade(application),
            token: key.token(),
            deadline,
            operation: Operation::Finish(key),
        }
    }
    pub fn append(
        hub: UploadHub,
        application: &Arc<WebApplication>,
        backend: &Arc<Mutex<Backend>>,
        key: UploadKey,
        head: RequestHead,
        body: Vec<u8>,
        life: TransferLifetime,
    ) -> Result<Self, TransferError> {
        if key.kind() != UploadKind::Material
            || head.token != life.token()
            || body.len() > fc_protocol::MAX_JSON_BYTES
            || body.len() as u64 != head.content_length.unwrap_or(0)
            || fc_ports::http::validate_headers(&head.headers).is_err()
        {
            return Err(TransferError::InvalidAdmission);
        }
        Ok(Self {
            hub,
            application: Arc::downgrade(application),
            token: head.token,
            deadline: life.deadline(),
            operation: Operation::Append {
                key: Some(key),
                backend: Arc::downgrade(backend),
                head,
                body,
                life,
            },
        })
    }
    /// The HTTP request supplies a numeric upload ID in strict JSON, never an
    /// opaque server key. Read an authenticated exact durable receipt first;
    /// only an absent receipt may resolve Ready and enter append_ready, which
    /// checks the original lease/scope and both request lifetimes again.
    pub fn append_request(
        hub: UploadHub,
        application: &Arc<WebApplication>,
        backend: &Arc<Mutex<Backend>>,
        head: RequestHead,
        body: Vec<u8>,
        life: TransferLifetime,
    ) -> Result<Self, TransferError> {
        if head.token != life.token()
            || body.len() > fc_protocol::MAX_JSON_BYTES
            || body.len() as u64 != head.content_length.unwrap_or(0)
            || fc_ports::http::validate_headers(&head.headers).is_err()
        {
            return Err(TransferError::InvalidAdmission);
        }
        Ok(Self {
            hub,
            application: Arc::downgrade(application),
            token: head.token,
            deadline: life.deadline(),
            operation: Operation::Append {
                key: None,
                backend: Arc::downgrade(backend),
                head,
                body,
                life,
            },
        })
    }
    /// A fresh cleanup deadline grants only negative cleanup of the exact key.
    /// It never renews the expired upload's admission or SQL execution lifetime.
    pub fn cleanup(
        hub: UploadHub,
        application: &Arc<WebApplication>,
        key: UploadKey,
        cleanup_deadline: Instant,
    ) -> Self {
        Self {
            hub,
            application: Arc::downgrade(application),
            token: key.token(),
            deadline: cleanup_deadline,
            operation: Operation::Cleanup(key),
        }
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Use only after the original Begin task is physically complete. This
    /// recovers a key whose positive admission reply was suppressed by cancel.
    pub fn cleanup_request(
        hub: UploadHub,
        application: &Arc<WebApplication>,
        token: RequestToken,
        cleanup_deadline: Instant,
    ) -> Self {
        Self {
            hub,
            application: Arc::downgrade(application),
            token,
            deadline: cleanup_deadline,
            operation: Operation::CleanupRequest,
        }
    }
    fn execute(self, context: &JobContext) -> Result<UploadOperationResult, UploadHubFailure> {
        context
            .checkpoint()
            .map_err(|_| UploadHubError::Cancelled)?;
        if context.deadline() > self.deadline {
            return Err(UploadHubError::Invalid.into());
        }
        if let Operation::Write {
            key,
            sequence,
            offset,
            bytes,
        } = &self.operation
        {
            return self
                .hub
                .write(*key, *sequence, *offset, bytes, context)
                .map(UploadOperationResult::Written);
        }
        let application = self.application.upgrade().ok_or(UploadHubError::Closed)?;
        match self.operation {
            Operation::Begin {
                kind,
                head,
                control,
                life,
            } => match kind {
                UploadKind::Material => {
                    self.hub
                        .begin_material(&application, &head, control, life, context)
                }
                UploadKind::Voice => {
                    self.hub
                        .begin_voice(&application, &head, control, life, context)
                }
            }
            .map(UploadOperationResult::Admitted),
            Operation::Finish(key) => self
                .hub
                .finish(key, &application, context)
                .map(UploadOperationResult::Finished),
            Operation::Append {
                key,
                backend,
                head,
                body,
                life,
            } => {
                let input = fc_protocol::web::materials::append(&body)
                    .map_err(|_| UploadHubError::Invalid)?;
                if key.is_some_and(|key| {
                    key.kind() != UploadKind::Material || key.upload_id() != input.upload_id
                }) {
                    return Err(UploadHubError::Invalid.into());
                }
                let backend = backend.upgrade().ok_or(UploadHubError::Closed)?;
                if let Some(prior) = application
                    .material_append_receipt_in_job(&backend, &head, &body, &life, context)?
                {
                    return Ok(UploadOperationResult::Appended(prior));
                }
                let key = match key {
                    Some(key) => key,
                    None => self
                        .hub
                        .ready_material_key(input.upload_id, context)?
                        .ok_or(UploadHubError::Stale)?,
                };
                self.hub
                    .append_ready(key, &application, &backend, &head, &body, &life, context)
                    .map(UploadOperationResult::Appended)
            }
            Operation::Cleanup(key) => {
                self.hub.cleanup(key, &application, context)?;
                Ok(UploadOperationResult::Cleaned(key))
            }
            Operation::CleanupRequest => {
                self.hub
                    .cleanup_request(self.token, &application, context)?;
                Ok(UploadOperationResult::CleanedRequest(self.token))
            }
            Operation::Write { .. } => unreachable!("write handled without upgrading composition"),
        }
    }
}
impl Task for UploadTask {
    type Output = UploadCompletion;
    fn allocation_bytes(&self) -> Option<usize> {
        match &self.operation {
            Operation::Begin { head, .. } => head_allocation(head),
            Operation::Write { bytes, .. } => Some(bytes.capacity()),
            Operation::Append { head, body, .. } => {
                head_allocation(head)?.checked_add(body.capacity())
            }
            Operation::Finish(_) | Operation::Cleanup(_) | Operation::CleanupRequest => Some(0),
        }
    }
    fn run(self, context: &JobContext) -> UploadCompletion {
        let token = self.token;
        UploadCompletion {
            token,
            result: self.execute(context),
        }
    }
}
