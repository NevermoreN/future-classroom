//! Voice Web adaptation: a durable command acknowledgement is neither native
//! playback nor evidence that anyone heard the recording.
use super::*;
use crate::backend::{BackendError, VoiceService};
use fc_application::identity::SessionLease;
use fc_domain::voice::*;
use fc_ports::voice::VoiceRepositoryError;
use fc_protocol::web::voice as wire;
pub(super) fn dispatch(
    service: &VoiceService,
    lease: SessionLease,
    route: Route,
    body: &[u8],
) -> WebResponse {
    match route {
        Route::VoiceCapabilities => match service.web_capabilities(lease) {
            Ok(c) => capabilities_response(&c),
            Err(e) => error(e, false),
        },
        Route::VoiceList => match service.web_jobs(lease) {
            Ok(jobs) => response(200, wire::jobs(&jobs), None),
            Err(e) => error(e, false),
        },
        Route::VoiceStatus { job_id } => match service.status(lease, job_id) {
            Ok(job) => response(200, wire::status(&job), None),
            Err(e) => error(e, false),
        },
        Route::VoiceSubmit => {
            let request = match fc_protocol::web::voice_submit(body) {
                Ok(r) => r,
                Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
            };
            match service.submit(lease, request) {
                Ok(job) => mutation(service, lease, wire::committed(&job)),
                Err(e) => error(e, true),
            }
        }
        Route::VoiceCancel { job_id } => {
            let request = match fc_protocol::web::voice_cancel(job_id, body) {
                Ok(r) => r,
                Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
            };
            let id = request.command_id;
            match service.cancel(lease, request) {
                Ok(job) => mutation(service, lease, wire::cancelled(id, &job)),
                Err(e) => error(e, true),
            }
        }
        _ => failure(404, responses::ErrorCode::NotFound),
    }
}
pub(super) fn capabilities_response(c: &crate::backend::WebVoiceCapabilities) -> WebResponse {
    response(
                200,
                wire::capabilities(
                    c.identity_revision,
                    c.runtime_generation,
                    c.now,
                    c.current_class
                        .as_ref()
                        .map(|(id, name)| (*id, name.as_str())),
                    wire::CapabilityPolicy {
                        enabled: c.enabled,
                        admitting: c.admitting,
                        target_authorized: c.target_authorized,
                        device_availability: match c.device_availability {
                            crate::backend::WebVoiceDeviceAvailability::Unverified => wire::DeviceAvailability::Unverified,
                            crate::backend::WebVoiceDeviceAvailability::Unavailable => wire::DeviceAvailability::Unavailable,
                        },
                        limits: c.limits,
                    },
                ),
                None,
            )
}
fn mutation(
    service: &VoiceService,
    lease: SessionLease,
    body: Result<Vec<u8>, ProtocolError>,
) -> WebResponse {
    if service.check_web_actor(lease).is_err() {
        return failure(503, responses::ErrorCode::ResultUnknown);
    }
    match body {
        Ok(body) => response(200, Ok(body), None),
        Err(_) => failure(503, responses::ErrorCode::ResultUnknown),
    }
}
pub(super) fn error(error: BackendError, mutating: bool) -> WebResponse {
    match error {
        BackendError::Voice(VoiceRepositoryError::Domain(error)) => match error {
            VoiceError::Permission(_) => failure(403, responses::ErrorCode::Forbidden),
            VoiceError::TargetMismatch | VoiceError::StaleGeneration => {
                failure(409, responses::ErrorCode::TargetChanged)
            }
            VoiceError::Disabled => failure(409, responses::ErrorCode::VoiceDisabled),
            VoiceError::InvalidWav => failure(400, responses::ErrorCode::InvalidWav),
            VoiceError::RepeatOutOfRange => failure(400, responses::ErrorCode::InvalidRequest),
            VoiceError::QueueFull => failure(429, responses::ErrorCode::QueueFull),
            VoiceError::OwnerLimit => failure(429, responses::ErrorCode::OwnerLimit),
            VoiceError::Quota | VoiceError::Capacity => {
                failure(429, responses::ErrorCode::Capacity)
            }
            VoiceError::Missing => failure(404, responses::ErrorCode::NotFound),
            VoiceError::RevisionConflict => failure(409, responses::ErrorCode::RevisionConflict),
            VoiceError::Expired => failure(409, responses::ErrorCode::Expired),
            VoiceError::RateLimited => failure(429, responses::ErrorCode::RateLimited),
            _ => unknown(mutating),
        },
        BackendError::Voice(VoiceRepositoryError::IdempotencyConflict) => {
            failure(409, responses::ErrorCode::IdempotencyConflict)
        }
        BackendError::Session(_) if !mutating => failure(403, responses::ErrorCode::Forbidden),
        _ => unknown(mutating),
    }
}
fn unknown(mutating: bool) -> WebResponse {
    failure(
        503,
        if mutating {
            responses::ErrorCode::ResultUnknown
        } else {
            responses::ErrorCode::ServiceUnavailable
        },
    )
}
/// Shared typed binary authentication; cookie/CSRF remain server secrets and no
/// caller-supplied owner/lease field is decoded from the uploaded PCM stream.
#[cfg(windows)]
pub(super) fn upload_auth(
    app: &WebApplication,
    head: &RequestHead,
) -> Result<(SessionLease, fc_domain::ids::ClassId), WebResponse> {
    let route = app
        .boundary
        .preflight(head)
        .map_err(|error| app.request_boundary_error(head, error))?;
    let Route::VoiceUpload { class_id } = route else {
        return Err(failure(400, responses::ErrorCode::InvalidRequest));
    };
    let mut cookies = head
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("cookie"));
    let header = match (cookies.next(), cookies.next()) {
        (Some(h), None) => h,
        (None, _) => return Err(failure(401, responses::ErrorCode::Unauthorized)),
        _ => return Err(boundary_error(WebError::Headers)),
    };
    let cookie = cookie_secret(&header.value).map_err(authentication_error)?;
    let session = app
        .service
        .authenticate(&cookie)
        .map_err(authentication_error)?;
    app.boundary
        .check(head, Some(session.csrf.as_str()))
        .map_err(|error| app.request_boundary_error(head, error))?;
    Ok((session.lease(), class_id))
}
