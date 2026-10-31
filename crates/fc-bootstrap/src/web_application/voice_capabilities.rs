//! One request-local capability query owner. Authentication happens while the
//! Backend composition is borrowed; the synchronous native query happens after
//! that borrow and its outer mutex guard have been released by WebTask.
use super::*;
use crate::{
    backend::VoiceService,
    web_transfer::{ExecutionFence, TransferLifetime},
};
use fc_application::identity::SessionLease;
use fc_ports::audio::{AudioOutputError, DefaultOutputSupport};
use fc_runtime::worker::JobContext;
use std::{sync::Arc, time::Instant};

pub(crate) struct PreparedVoiceCapabilities {
    service: VoiceService,
    lease: SessionLease,
    deadline: Instant,
    fence: ExecutionFence,
}
impl WebApplication {
    pub(crate) fn prepare_voice_capabilities(
        &self,
        backend: &mut Backend,
        head: &RequestHead,
        body: &[u8],
        life: &TransferLifetime,
        context: &JobContext,
        deadline: Instant,
    ) -> Result<PreparedVoiceCapabilities, WebResponse> {
        let original = life.job_fence(context);
        let deadline = deadline.min(life.deadline()).min(context.deadline());
        let fence: ExecutionFence = Arc::new(move || Instant::now() < deadline && original());
        if head.token != life.token() || !fence() {
            return Err(failure(503, responses::ErrorCode::ServiceUnavailable));
        }
        let app = self.for_execution(fence.clone());
        let scope = backend.begin_execution_scope(fence.clone());
        if !app.service.same_instance(&scope.web_service()) {
            return Err(failure(503, responses::ErrorCode::ServiceUnavailable));
        }
        let route = app
            .boundary
            .preflight(head)
            .map_err(|error| app.request_boundary_error(head, error))?;
        if route != Route::VoiceCapabilities {
            return Err(failure(404, responses::ErrorCode::NotFound));
        }
        if !body.is_empty() || head.content_length.unwrap_or(0) != 0 {
            return Err(boundary_error(WebError::Framing));
        }
        let authenticated = app
            .authenticate_route(head, route)?
            .ok_or_else(|| failure(401, responses::ErrorCode::Unauthorized))?;
        if !fence() {
            return Err(failure(503, responses::ErrorCode::ServiceUnavailable));
        }
        Ok(PreparedVoiceCapabilities {
            service: scope.voice_service(),
            lease: authenticated.lease(),
            deadline,
            fence,
        })
    }
}
impl PreparedVoiceCapabilities {
    pub(crate) fn query(
        self,
        query: impl FnOnce() -> Result<DefaultOutputSupport, AudioOutputError>,
    ) -> WebResponse {
        let capabilities = match self.service.web_capabilities_with_output(
            self.lease,
            self.deadline,
            query,
        ) {
            Ok(capabilities) => capabilities,
            Err(error) => return voice::error(error, false),
        };
        let result = voice::capabilities_response(&capabilities);
        // Encoding does not grant a new request interval. Recheck the exact
        // actor and coherent publication before any response can escape.
        let current = self.service.check_web_capabilities_output(self.lease, &capabilities);
        if !(self.fence)() || current.is_err() {
            return failure(503, responses::ErrorCode::ServiceUnavailable);
        }
        result
    }
}
