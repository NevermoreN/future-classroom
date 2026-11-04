//! Administrator routes reuse existing authenticated use cases and the single writer.
use super::*;
use crate::backend::BackendError;
use fc_application::identity::SessionLease;
use fc_domain::{auth::Role, ids::Revision};
use fc_protocol::web::admin::{self as wire, Resource};
mod profile_content;
mod profiles;
mod schedule;
pub enum PlatformRequest {
    Status,
    Network,
    Autostart,
    ChangeNetwork {
        expected_revision: Revision,
        change: wire::platform::NetworkChange,
    },
    ChangeAutostart {
        expected_revision: Revision,
        change: wire::platform::AutostartChange,
    },
}
pub enum PlatformReply {
    Status(fc_domain::ids::Revision, wire::platform::Status),
    Network(Revision, wire::platform::Network),
    Autostart(Revision, wire::platform::Autostart),
}
#[derive(Clone, Copy)]
pub enum PlatformError {
    Denied,
    Invalid,
    Conflict,
    Unavailable,
}
pub type PlatformHook = std::sync::Arc<
    dyn Fn(
            SessionLease,
            PlatformRequest,
            &fc_runtime::worker::JobContext,
        ) -> Result<PlatformReply, PlatformError>
        + Send
        + Sync,
>;
pub(crate) fn data_response(result: Result<Vec<u8>, ProtocolError>) -> WebResponse {
    response(200, result, None)
}
pub(super) fn backend_error(error: BackendError) -> WebResponse {
    if let Some(reply) = super::command_failure(&error) {
        return reply;
    }
    match error {
        BackendError::Session(_) | BackendError::InvalidCredentials => {
            failure(401, responses::ErrorCode::Unauthorized)
        }
        BackendError::Identity(fc_ports::IdentityRepositoryError::Domain(
            fc_domain::identity::IdentityError::RevisionConflict,
        )) => failure(409, responses::ErrorCode::RevisionConflict),
        BackendError::Display(fc_ports::DisplayRepositoryError::Domain(
            fc_domain::desktop::DisplayError::RevisionConflict { .. },
        )) => failure(409, responses::ErrorCode::RevisionConflict),
        BackendError::Schedule(fc_ports::ScheduleRepositoryError::Domain(
            fc_domain::schedule_document::ScheduleDocumentError {
                kind: fc_domain::schedule_document::DocumentErrorKind::RevisionConflict { .. },
                ..
            },
        )) => failure(409, responses::ErrorCode::RevisionConflict),
        BackendError::Profile(fc_ports::ProfileRepositoryError::Domain(
            fc_domain::profile::ProfileError::RevisionConflict { .. },
        )) => failure(409, responses::ErrorCode::RevisionConflict),
        BackendError::InvalidInput
        | BackendError::Identity(_)
        | BackendError::Display(_)
        | BackendError::Schedule(_)
        | BackendError::Profile(_) => failure(400, responses::ErrorCode::InvalidRequest),
        _ => failure(503, responses::ErrorCode::ServiceUnavailable),
    }
}
fn invalid() -> WebResponse {
    failure(400, responses::ErrorCode::InvalidRequest)
}
pub(super) fn dispatch(
    backend: &mut Backend,
    lease: SessionLease,
    resource: Resource,
    body: Option<&[u8]>,
    hook: Option<&PlatformHook>,
) -> WebResponse {
    match backend.role(lease) {
        Ok(Role::GlobalAdmin) => {}
        Ok(_) => return failure(403, responses::ErrorCode::Forbidden),
        Err(e) => return backend_error(e),
    }
    if let Resource::ProfileContent { slot } = resource {
        return profile_content::dispatch(backend, lease, slot, body);
    }
    if let Resource::Profiles { after } = resource {
        return if body.is_none() {
            profiles::list(backend, lease, after)
        } else {
            invalid()
        };
    }
    if let Resource::Profile { key } = resource {
        return profiles::dispatch(backend, lease, key, body);
    }
    if matches!(
        resource,
        Resource::Status | Resource::Network | Resource::Autostart
    ) {
        let request = match (resource, body) {
            (Resource::Status, None) => PlatformRequest::Status,
            (Resource::Network, None) => PlatformRequest::Network,
            (Resource::Autostart, None) => PlatformRequest::Autostart,
            (Resource::Network, Some(bytes)) => match wire::platform::network_mutation(bytes) {
                Ok((expected_revision, change)) => PlatformRequest::ChangeNetwork {
                    expected_revision,
                    change,
                },
                Err(_) => return invalid(),
            },
            (Resource::Autostart, Some(bytes)) => match wire::platform::autostart_mutation(bytes) {
                Ok((expected_revision, change)) => PlatformRequest::ChangeAutostart {
                    expected_revision,
                    change,
                },
                Err(_) => return invalid(),
            },
            _ => return invalid(),
        };
        // Synchronous dispatch owns the Backend mutex. The WebTask platform
        // preparation path releases that lock before invoking the native hook.
        let _ = (request, hook);
        return failure(503, responses::ErrorCode::ServiceUnavailable);
    }
    if resource == Resource::Schedule {
        return schedule::dispatch(backend, lease, body);
    }
    if body.is_none() {
        return match resource {
            Resource::Identity => match backend.management_identity(lease) {
                Ok(value) => data_response(wire::identity::document(&value)),
                Err(error) => backend_error(error),
            },
            Resource::Display => match backend.read_display(lease) {
                Ok(value) => data_response(wire::display::document(&value)),
                Err(error) => backend_error(error),
            },
            Resource::Layout => match backend.read_public_layout(lease) {
                Ok(value) => data_response(wire::layout::document(&value.layout)),
                Err(error) => backend_error(error),
            },
            _ => invalid(),
        };
    }
    let bytes = body.unwrap_or_default();
    let family = match resource {
        Resource::Identity => fc_ports::command_window::CommandFamily::Identity,
        Resource::Display => fc_ports::command_window::CommandFamily::Display,
        Resource::Layout => fc_ports::command_window::CommandFamily::Profiles,
        _ => return invalid(),
    };
    let issued = match backend.issue_for_web_admin(lease, family) {
        Ok(issued) => issued,
        Err(error) => return backend_error(error),
    };
    let mut scoped = match backend.begin_command_scope(issued) {
        Ok(scoped) => scoped,
        Err(error) => return backend_error(error),
    };
    match resource {
        Resource::Identity => {
            let current = match scoped.management_identity(lease) {
                Ok(state) => state,
                Err(error) => return backend_error(error),
            };
            let (expected, change) = match wire::identity::mutation(bytes, &current) {
                Ok(value) => value,
                Err(_) => return invalid(),
            };
            let result = match change {
                wire::identity::Change::Identity(command) => {
                    scoped.identity_at(lease, expected, command)
                }
                wire::identity::Change::Create { account, password } => {
                    let password = match fc_security::Password::new(password) {
                        Ok(value) => value,
                        Err(_) => return invalid(),
                    };
                    match scoped.create_account_at(lease, expected, account, password) {
                        Ok(recovery) => {
                            drop(recovery);
                            scoped.management_identity(lease)
                        }
                        Err(error) => Err(error),
                    }
                }
            };
            match result {
                Ok(value) => data_response(wire::identity::document(&value)),
                Err(error) => backend_error(error),
            }
        }
        Resource::Display => {
            let (expected, candidate) = match wire::display::mutation(bytes) {
                Ok(value) => value,
                Err(_) => return invalid(),
            };
            match scoped.save_display(lease, expected, candidate) {
                Ok(value) => data_response(wire::display::document(&value)),
                Err(error) => backend_error(error),
            }
        }
        Resource::Layout => {
            let (expected, candidate) = match wire::layout::mutation(bytes) {
                Ok(value) => value,
                Err(_) => return invalid(),
            };
            let current = match scoped.read_public_layout(lease) {
                Ok(value) => value,
                Err(error) => return backend_error(error),
            };
            match scoped.save_public_layout(
                lease,
                expected,
                current.display.revision,
                candidate,
                current.display,
            ) {
                Ok(value) => data_response(wire::layout::document(&value.layout)),
                Err(error) => backend_error(error),
            }
        }
        _ => invalid(),
    }
}
