//! Already-arrived bounded head/JSON jobs for the application's existing pool.
//! Weak references keep rejected tasks from owning Backend/native resources.
use super::{TransferError, TransferLifetime};
use crate::{
    backend::Backend,
    web_application::{WebApplication, WebResponse},
};
use fc_ports::http::{RequestHead, RequestToken, MAX_TARGET_BYTES};
use fc_protocol::{
    web::{responses, Route},
    MAX_JSON_BYTES,
};
use fc_runtime::worker::{JobContext, Task};
use std::sync::{Arc, Mutex, Weak};

#[cfg(test)]
mod tests;

enum Operation {
    Admit,
    Json(Vec<u8>),
}
pub struct WebTask {
    backend: Weak<Mutex<Backend>>,
    application: Weak<WebApplication>,
    head: RequestHead,
    life: TransferLifetime,
    operation: Operation,
}
/// Bounded response bytes and correlation only: never a stage, native pin,
/// publication proof, upload permit or physical-cleanup acknowledgment.
pub enum WebCompletion {
    Admitted {
        token: RequestToken,
        route: Route,
    },
    Response {
        token: RequestToken,
        response: WebResponse,
    },
}
impl WebTask {
    pub fn admit(
        backend: &Arc<Mutex<Backend>>,
        application: &Arc<WebApplication>,
        head: RequestHead,
        life: TransferLifetime,
    ) -> Result<Self, TransferError> {
        Self::new(backend, application, head, life, Operation::Admit)
    }
    pub fn json(
        backend: &Arc<Mutex<Backend>>,
        application: &Arc<WebApplication>,
        head: RequestHead,
        body: Vec<u8>,
        life: TransferLifetime,
    ) -> Result<Self, TransferError> {
        if body.len() > MAX_JSON_BYTES || body.len() as u64 != head.content_length.unwrap_or(0) {
            return Err(TransferError::InvalidAdmission);
        }
        Self::new(backend, application, head, life, Operation::Json(body))
    }
    fn new(
        backend: &Arc<Mutex<Backend>>,
        application: &Arc<WebApplication>,
        head: RequestHead,
        life: TransferLifetime,
        operation: Operation,
    ) -> Result<Self, TransferError> {
        if head.token != life.token()
            || head.method.is_empty()
            || head.method.len() > 16
            || head.target.len() > MAX_TARGET_BYTES
            || fc_ports::http::validate_headers(&head.headers).is_err()
            || head
                .content_length
                .is_some_and(|n| n > fc_ports::http::MAX_TRANSFER_BYTES)
        {
            return Err(TransferError::InvalidAdmission);
        }
        Ok(Self {
            backend: Arc::downgrade(backend),
            application: Arc::downgrade(application),
            head,
            life,
            operation,
        })
    }
    /// Submit with this unchanged absolute time; queueing consumes the budget.
    pub fn deadline(&self) -> std::time::Instant {
        self.life.deadline()
    }
}
fn unavailable() -> WebResponse {
    let body = responses::error(responses::ErrorCode::ServiceUnavailable)
        .unwrap_or_else(|_| b"{}".to_vec());
    WebResponse {
        head: fc_ports::http::ResponseHead {
            status: 503,
            headers: vec![
                fc_ports::http::Header {
                    name: "Content-Type".into(),
                    value: b"application/json".to_vec(),
                },
                fc_ports::http::Header {
                    name: "Cache-Control".into(),
                    value: b"no-store".to_vec(),
                },
            ],
            content_length: body.len() as u64,
        },
        body,
    }
}
impl Task for WebTask {
    type Output = WebCompletion;
    fn allocation_bytes(&self) -> Option<usize> {
        let mut bytes = head_allocation(&self.head)?;
        if let Operation::Json(body) = &self.operation {
            bytes = bytes.checked_add(body.capacity())?;
        }
        Some(bytes)
    }
    fn run(self, context: &JobContext) -> WebCompletion {
        let token = self.head.token;
        let response = |response| WebCompletion::Response { token, response };
        if !self.life.is_active() || context.checkpoint().is_err() {
            return response(unavailable());
        }
        let (Some(application), Some(backend)) =
            (self.application.upgrade(), self.backend.upgrade())
        else {
            return response(unavailable());
        };
        let Ok(mut backend) = backend.lock() else {
            return response(unavailable());
        };
        match self.operation {
            Operation::Admit => match application.admit_request_in_job(
                &mut backend,
                &self.head,
                &self.life,
                context,
            ) {
                Ok(route) => WebCompletion::Admitted { token, route },
                Err(error) => response(error),
            },
            Operation::Json(body) => {
                let result = application.dispatch_in_job(
                    &mut backend,
                    &self.head,
                    &body,
                    &self.life,
                    context,
                );
                // Streamed files/static payloads have separate chunk owners.
                if result.body.len() > MAX_JSON_BYTES
                    || result.body.len() as u64 != result.head.content_length
                    || result.head.validate().is_err()
                {
                    response(unavailable())
                } else {
                    response(result)
                }
            }
        }
    }
}

pub(super) fn head_allocation(head: &RequestHead) -> Option<usize> {
    let mut bytes = head
        .method
        .capacity()
        .checked_add(head.target.capacity())?
        .checked_add(
            head.headers
                .capacity()
                .checked_mul(std::mem::size_of::<fc_ports::http::Header>())?,
        )?;
    for header in &head.headers {
        bytes = bytes
            .checked_add(header.name.capacity())?
            .checked_add(header.value.capacity())?;
    }
    Some(bytes)
}
