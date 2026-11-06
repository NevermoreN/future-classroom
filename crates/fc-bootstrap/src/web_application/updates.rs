//! Authenticated maintenance forms and bounded binary file chunks. None of
//! these routes executes an installer in the HTTP worker.
use super::*;
use fc_application::identity::SessionLease;
use fc_domain::{auth::Role, ids::Revision};
use fc_ports::{command_window::CommandFamily, updates::UpdateRepositoryError};
use fc_protocol::web::{
    command_window::CommandReference,
    updates::{self as wire, Resource},
};
use std::time::{Duration, Instant};
pub(super) struct Upload {
    token: String,
    lease: SessionLease,
    command: CommandReference,
    length: u64,
    received: u64,
    started: Instant,
}
fn runtime_wire(status: &crate::updates::UpdateStatus) -> responses::About<'_> {
    responses::About {
        schema_version: 1,
        version: &status.version,
        automatic_updates: status.automatic_updates,
        update_available: status.update_available,
        update_message: &status.update_message,
        action_token: &status.action_token,
        phase: &status.phase,
        received_bytes: status.received_bytes,
        total_bytes: status.total_bytes,
        group_id: status.group_id.as_deref(),
    }
}
fn invalid() -> WebResponse {
    failure(400, responses::ErrorCode::InvalidRequest)
}
fn unavailable() -> WebResponse {
    failure(503, responses::ErrorCode::ServiceUnavailable)
}
fn backend_error(error: crate::backend::BackendError) -> WebResponse {
    use crate::backend::BackendError;
    match error {
        BackendError::Updates(UpdateRepositoryError::Stale) => {
            failure(409, responses::ErrorCode::RevisionConflict)
        }
        BackendError::Updates(UpdateRepositoryError::CommandIdReused) => {
            failure(409, responses::ErrorCode::IdempotencyConflict)
        }
        BackendError::Updates(UpdateRepositoryError::Denied) => {
            failure(403, responses::ErrorCode::Forbidden)
        }
        BackendError::Updates(UpdateRepositoryError::Invalid) => invalid(),
        BackendError::Updates(UpdateRepositoryError::Capacity) => {
            failure(429, responses::ErrorCode::Capacity)
        }
        other => admin::backend_error(other),
    }
}
fn status(app: &WebApplication, backend: &Backend, lease: SessionLease) -> WebResponse {
    let state = match backend.read_update_status(lease) {
        Ok(state) => state,
        Err(error) => return backend_error(error),
    };
    let Some(control) = app
        .update_control
        .lock()
        .ok()
        .and_then(|control| control.clone())
    else {
        return unavailable();
    };
    let runtime = control.status();
    response(200, wire::status(&state, runtime_wire(&runtime)), None)
}
fn save_policy(
    backend: &mut Backend,
    lease: SessionLease,
    command: CommandReference,
    expected: Revision,
    policy: fc_domain::update_state::UpdatePolicy,
) -> Result<(), crate::backend::BackendError> {
    let issued = backend.resolve_web_command(
        lease,
        CommandFamily::Updates,
        command.ticket,
        command.command_id,
    )?;
    let mut scoped = backend.begin_command_scope(issued)?;
    scoped.save_update_policy(lease, expected, policy)?;
    Ok(())
}
fn validate_command(
    backend: &Backend,
    lease: SessionLease,
    command: CommandReference,
    expected: Revision,
) -> Result<(), crate::backend::BackendError> {
    backend.resolve_web_command(
        lease,
        CommandFamily::Updates,
        command.ticket,
        command.command_id,
    )?;
    if backend.read_update_status(lease)?.revision != expected {
        return Err(crate::backend::BackendError::Updates(
            UpdateRepositoryError::Stale,
        ));
    }
    Ok(())
}
impl WebApplication {
    pub(super) fn about_response(&self) -> WebResponse {
        if !self.local_control {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        if let Some(control) = self
            .update_control
            .lock()
            .ok()
            .and_then(|control| control.clone())
        {
            let runtime = control.status();
            return response(200, responses::about(&runtime_wire(&runtime)), None);
        }
        response(
            200,
            responses::about(&responses::About {
                schema_version: 1,
                version: fc_domain::PRODUCT_DISPLAY_VERSION,
                automatic_updates: true,
                update_available: false,
                update_message: "自动更新默认开启，更新服务暂未启动。",
                action_token: "",
                phase: "unavailable",
                received_bytes: 0,
                total_bytes: 0,
                group_id: None,
            }),
            None,
        )
    }
    pub(super) fn check_update_response(&self, head: &RequestHead, body: &[u8]) -> WebResponse {
        if !self.local_control {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        if fc_protocol::web::pairing::logout(body).is_err() {
            return invalid();
        }
        let mut headers = head
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case("x-fc-update"));
        let token = match (headers.next(), headers.next()) {
            (Some(header), None) => match std::str::from_utf8(&header.value) {
                Ok(value) => value,
                Err(_) => return invalid(),
            },
            _ => return failure(403, responses::ErrorCode::Forbidden),
        };
        let Some(control) = self
            .update_control
            .lock()
            .ok()
            .and_then(|control| control.clone())
        else {
            return unavailable();
        };
        match control.request_check(token) {
            Ok(()) => self.about_response(),
            Err(_) => failure(403, responses::ErrorCode::Forbidden),
        }
    }
    pub(super) fn retire_update_uploads(&self, lease: SessionLease) {
        let Ok(mut uploads) = self.update_uploads.lock() else {
            return;
        };
        if let Some(control) = self
            .update_control
            .lock()
            .ok()
            .and_then(|control| control.clone())
        {
            uploads.retain(|upload| {
                if upload.lease == lease {
                    let _ = control.cancel_import(&upload.token);
                    false
                } else {
                    true
                }
            });
        }
    }
    pub(super) fn update_response(
        &self,
        backend: &mut Backend,
        lease: SessionLease,
        resource: Resource,
        body: &[u8],
    ) -> WebResponse {
        if !self.local_control {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        match backend.role(lease) {
            Ok(Role::GlobalAdmin) => (),
            Ok(_) => return failure(403, responses::ErrorCode::Forbidden),
            Err(error) => return backend_error(error),
        }
        let Some(control) = self
            .update_control
            .lock()
            .ok()
            .and_then(|control| control.clone())
        else {
            return unavailable();
        };
        match resource {
            Resource::Status => status(self, backend, lease),
            Resource::GroupExport => match control.export_group() {
                Ok(group) => response(200, wire::group_export(&group), None),
                Err(_) => unavailable(),
            },
            Resource::Policy => {
                let change = match wire::policy(body) {
                    Ok(value) => value,
                    Err(_) => return invalid(),
                };
                let mut policy = match backend.read_update_status(lease) {
                    Ok(state) => state.policy,
                    Err(error) => return backend_error(error),
                };
                policy.enabled = change.enabled;
                policy.download = change.download;
                policy.share = change.share;
                policy.install_mode = change.install_mode;
                if let Err(error) = save_policy(
                    backend,
                    lease,
                    change.command,
                    change.expected_revision,
                    policy,
                ) {
                    return backend_error(error);
                }
                let runtime = control.status();
                let _ = control.request_check(&runtime.action_token);
                status(self, backend, lease)
            }
            Resource::GroupCreate | Resource::GroupJoin => {
                let (command, expected, join) = if resource == Resource::GroupCreate {
                    match wire::command(body) {
                        Ok((command, expected)) => (command, expected, None),
                        Err(_) => return invalid(),
                    }
                } else {
                    match wire::join(body) {
                        Ok((command, expected, group)) => (command, expected, Some(group)),
                        Err(_) => return invalid(),
                    }
                };
                if let Err(error) = validate_command(backend, lease, command, expected) {
                    return backend_error(error);
                }
                let enrollment = match join {
                    Some(ref group) => control.prepare_group_join(group),
                    None => control.prepare_group_create(),
                };
                let enrollment = match enrollment {
                    Ok(value) => value,
                    Err(_) => return invalid(),
                };
                let mut policy = match backend.read_update_status(lease) {
                    Ok(state) => state.policy,
                    Err(error) => {
                        let _ = control.discard_group(enrollment.token);
                        return backend_error(error);
                    }
                };
                policy.group_id = Some(enrollment.group_id);
                if let Err(error) = save_policy(backend, lease, command, expected, policy) {
                    if matches!(
                        error,
                        crate::backend::BackendError::Updates(
                            UpdateRepositoryError::Denied
                                | UpdateRepositoryError::Invalid
                                | UpdateRepositoryError::Stale
                                | UpdateRepositoryError::CommandIdReused
                                | UpdateRepositoryError::Capacity
                                | UpdateRepositoryError::Busy
                        )
                    ) {
                        let _ = control.discard_group(enrollment.token);
                    }
                    return backend_error(error);
                }
                if control.commit_group(enrollment.token).is_err() {
                    return unavailable();
                }
                status(self, backend, lease)
            }
            Resource::Check => {
                let (command, expected) = match wire::command(body) {
                    Ok(value) => value,
                    Err(_) => return invalid(),
                };
                if let Err(error) = validate_command(backend, lease, command, expected) {
                    return backend_error(error);
                }
                let runtime = control.status();
                if control.request_check(&runtime.action_token).is_err() {
                    return unavailable();
                }
                status(self, backend, lease)
            }
            Resource::ImportBegin => {
                let (command, expected, length) = match wire::import(body) {
                    Ok(value) => value,
                    Err(_) => return invalid(),
                };
                if let Err(error) = validate_command(backend, lease, command, expected) {
                    return backend_error(error);
                }
                let mut uploads = match self.update_uploads.lock() {
                    Ok(value) => value,
                    Err(_) => return unavailable(),
                };
                uploads.retain(|upload| {
                    if upload.started.elapsed() > Duration::from_secs(600)
                        || backend.role(upload.lease) != Ok(Role::GlobalAdmin)
                    {
                        let _ = control.cancel_import(&upload.token);
                        false
                    } else {
                        true
                    }
                });
                if let Some(upload) = uploads
                    .iter()
                    .find(|upload| upload.lease == lease && upload.command == command)
                {
                    return if upload.length == length {
                        response(200, wire::upload_begin(&upload.token), None)
                    } else {
                        invalid()
                    };
                }
                if uploads.len() >= 2 {
                    return failure(429, responses::ErrorCode::Capacity);
                }
                let token = match control.begin_import(length) {
                    Ok(token) if wire::token(&token).is_some() => token,
                    _ => return unavailable(),
                };
                uploads.push(Upload {
                    token: token.clone(),
                    lease,
                    command,
                    length,
                    received: 0,
                    started: Instant::now(),
                });
                response(200, wire::upload_begin(&token), None)
            }
            Resource::ImportBytes { token, .. }
            | Resource::ImportFinish { token }
            | Resource::ImportCancel { token } => {
                let text = wire::token_text(token);
                let mut uploads = match self.update_uploads.lock() {
                    Ok(value) => value,
                    Err(_) => return unavailable(),
                };
                let Some(index) = uploads
                    .iter()
                    .position(|upload| upload.token == text && upload.lease == lease)
                else {
                    return failure(404, responses::ErrorCode::NotFound);
                };
                let upload = &mut uploads[index];
                if upload.started.elapsed() > Duration::from_secs(600) {
                    let _ = control.cancel_import(&text);
                    uploads.remove(index);
                    return failure(410, responses::ErrorCode::UploadExpired);
                }
                if backend
                    .resolve_web_command(
                        lease,
                        CommandFamily::Updates,
                        upload.command.ticket,
                        upload.command.command_id,
                    )
                    .is_err()
                {
                    return failure(403, responses::ErrorCode::Forbidden);
                }
                match resource {
                    Resource::ImportBytes { offset, .. } => {
                        if offset != upload.received
                            || body.is_empty()
                            || body.len() as u64 > wire::CHUNK_BYTES
                            || body.len() as u64 > upload.length - upload.received
                        {
                            return invalid();
                        }
                        #[cfg(windows)]
                        let Some(_permit) = super::shared_upload::PhysicalUploadPermit::take() else {
                            return failure(429, responses::ErrorCode::Capacity);
                        };
                        if control.append_import(&text, offset, body).is_err() {
                            return unavailable();
                        }
                        upload.received += body.len() as u64;
                        response(200, wire::uploaded(upload.received), None)
                    }
                    Resource::ImportFinish { .. } => {
                        if fc_protocol::web::pairing::logout(body).is_err()
                            || upload.received != upload.length
                        {
                            return invalid();
                        }
                        if control.finish_import(&text).is_err() {
                            return unavailable();
                        }
                        uploads.remove(index);
                        response(202, wire::queued(), None)
                    }
                    Resource::ImportCancel { .. } => {
                        if fc_protocol::web::pairing::logout(body).is_err() {
                            return invalid();
                        }
                        if control.cancel_import(&text).is_err() {
                            return unavailable();
                        }
                        uploads.remove(index);
                        response(200, wire::cancelled(), None)
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}
