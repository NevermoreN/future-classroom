//! Prepare platform work under the Backend lock, then execute it outside that
//! lock on the original bounded worker. Native calls never reenter a locked DB.
use super::*;
use admin::{PlatformError, PlatformHook, PlatformReply, PlatformRequest};
use fc_application::identity::{SessionLease, SessionPurpose};
use fc_domain::auth::Role;
use fc_protocol::web::admin::{self as wire, Resource};

impl WebApplication {
    pub(crate) fn prepare_admin_platform(
        &self,
        backend: &Backend,
        head: &RequestHead,
        body: &[u8],
    ) -> Result<(SessionLease, PlatformRequest, PlatformHook), WebResponse> {
        if !self.service.same_instance(&backend.web_service()) {
            return Err(failure(503, responses::ErrorCode::ServiceUnavailable));
        }
        let route = self
            .boundary
            .preflight(head)
            .map_err(|e| self.request_boundary_error(head, e))?;
        let Route::Admin { resource, write } = route else {
            return Err(failure(404, responses::ErrorCode::NotFound));
        };
        if body.len() as u64 != head.content_length.unwrap_or(0)
            || body.len() as u64 > route.body_limit()
        {
            return Err(boundary_error(WebError::Framing));
        }
        let session = self
            .authenticate_route(head, route)?
            .ok_or_else(|| failure(401, responses::ErrorCode::Unauthorized))?;
        if session.account.principal.role != Role::GlobalAdmin {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        backend
            .check_session(session.lease(), SessionPurpose::Manage)
            .map_err(admin::backend_error)?;
        let request = match (resource, write) {
            (Resource::Status, false) => PlatformRequest::Status,
            (Resource::Network, false) => PlatformRequest::Network,
            (Resource::Autostart, false) => PlatformRequest::Autostart,
            (Resource::Network, true) => {
                let (expected_revision, change) = wire::platform::network_mutation(body)
                    .map_err(|_| failure(400, responses::ErrorCode::InvalidRequest))?;
                PlatformRequest::ChangeNetwork {
                    expected_revision,
                    change,
                }
            }
            (Resource::Autostart, true) => {
                let (expected_revision, change) = wire::platform::autostart_mutation(body)
                    .map_err(|_| failure(400, responses::ErrorCode::InvalidRequest))?;
                PlatformRequest::ChangeAutostart {
                    expected_revision,
                    change,
                }
            }
            _ => return Err(failure(400, responses::ErrorCode::InvalidRequest)),
        };
        let hook = self
            .admin_platform
            .clone()
            .ok_or_else(|| failure(503, responses::ErrorCode::ServiceUnavailable))?;
        Ok((session.lease(), request, hook))
    }
}
pub(crate) fn response(result: Result<PlatformReply, PlatformError>) -> WebResponse {
    match result {
        Ok(PlatformReply::Status(revision, value)) => {
            admin::data_response(wire::encode(revision, &value))
        }
        Ok(PlatformReply::Network(revision, value)) => {
            admin::data_response(wire::encode(revision, &value))
        }
        Ok(PlatformReply::Autostart(revision, value)) => {
            admin::data_response(wire::encode(revision, &value))
        }
        Err(PlatformError::Denied) => failure(403, responses::ErrorCode::Forbidden),
        Err(PlatformError::Invalid) => failure(400, responses::ErrorCode::InvalidRequest),
        Err(PlatformError::Conflict) => failure(409, responses::ErrorCode::RevisionConflict),
        Err(PlatformError::Unavailable) => failure(503, responses::ErrorCode::ServiceUnavailable),
    }
}
