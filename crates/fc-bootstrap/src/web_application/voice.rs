//! Voice Web adaptation: a durable command acknowledgement is neither native
//! playback nor evidence that anyone heard the recording.
use super::*;
use crate::backend::{BackendError, VoiceService};
use fc_application::identity::SessionLease;
use fc_domain::voice::*;
use fc_ports::voice::VoiceRepositoryError;
use fc_protocol::web::command_window::CommandReference;
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
        Route::VoiceList => match service.web_jobs_bound(lease) {
            Ok(jobs) => response(200, wire::jobs_bound(&jobs), None),
            Err(e) => error(e, false),
        },
        Route::VoiceStatus { job_id } => match service.status_bound(lease, job_id, None) {
            Ok(job) => response(200, wire::status_bound(&job, None), None),
            Err(e) => error(e, false),
        },
        Route::VoiceBoundStatus {
            job_id,
            command_ticket,
        } => match service.status_bound(lease, job_id, Some(command_ticket)) {
            Ok(job) => response(200, wire::status_bound(&job, Some(command_ticket)), None),
            Err(e) => error(e, false),
        },
        Route::VoiceWindowCommand { command } => {
            match service.command_outcome(lease, command.ticket, command.command_id) {
                Ok(result) => response(
                    200,
                    wire::command_outcome(
                        command,
                        result.as_ref().map(|(job, ticket)| (job, *ticket)),
                    ),
                    None,
                ),
                Err(e) => error(e, false),
            }
        }
        Route::VoiceSubmit => {
            let (request, command_ticket) = match fc_protocol::web::voice_submit_window(body) {
                Ok(r) => r,
                Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
            };
            let command_id = request.command_id;
            match service.submit_window(lease, request, command_ticket) {
                Ok((job, submit_ticket)) => mutation(
                    service,
                    lease,
                    match command_ticket {
                        Some(ticket) => wire::committed_window(
                            CommandReference { ticket, command_id },
                            &job,
                            submit_ticket,
                        ),
                        None => wire::committed(&job),
                    },
                ),
                Err(e) => error(e, true),
            }
        }
        Route::VoiceCancel { job_id } => {
            let (request, command_ticket, job_command_ticket) =
                match fc_protocol::web::voice_cancel_window(job_id, body) {
                    Ok(r) => r,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
            let id = request.command_id;
            match service.cancel_window(lease, request, command_ticket, job_command_ticket) {
                Ok((job, submit_ticket)) => mutation(
                    service,
                    lease,
                    match command_ticket {
                        Some(ticket) => wire::cancelled_window(
                            CommandReference {
                                ticket,
                                command_id: id,
                            },
                            &job,
                            submit_ticket,
                        ),
                        None => wire::cancelled(id, &job),
                    },
                ),
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
                    crate::backend::WebVoiceDeviceAvailability::Unverified => {
                        wire::DeviceAvailability::Unverified
                    }
                    crate::backend::WebVoiceDeviceAvailability::Unavailable => {
                        wire::DeviceAvailability::Unavailable
                    }
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
    if let Some(response) = command_failure(&error) {
        return response;
    }
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
    let session = app
        .authenticate_route(head, route)?
        .ok_or_else(|| failure(401, responses::ErrorCode::Unauthorized))?;
    Ok((session.lease(), class_id))
}
