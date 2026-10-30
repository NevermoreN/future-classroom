//! Material JSON adaptation. File bytes have a separate typed, bounded stream port.
use super::*;
use crate::backend::BackendError;
use fc_application::identity::SessionLease;
use fc_domain::materials::*;
use fc_ports::{MaterialDeleteRequest, MaterialListRequest, MaterialRepositoryError};
use fc_protocol::web::materials::{self as wire, MaterialRoute};
pub(super) fn dispatch(
    app: &WebApplication,
    backend: &mut Backend,
    lease: SessionLease,
    route: MaterialRoute,
    body: &[u8],
) -> WebResponse {
    match route {
        MaterialRoute::Scopes { offset } => match backend.web_material_scopes(lease, offset) {
            Ok(page) => {
                let mut items: Vec<_> = page
                    .items
                    .iter()
                    .map(|i| wire::ScopeOption {
                        owner_teacher_id: i.scope.teacher_id.get().to_string(),
                        owner_display_name: &i.owner_display_name,
                        subject_id: i.scope.subject_id.get().to_string(),
                        subject_name: &i.subject_name,
                        class_total: i.class_total,
                        next_class_offset: (i.classes.len() < i.class_total)
                            .then_some(i.classes.len()),
                        classes: i
                            .classes
                            .iter()
                            .map(|c| wire::ScopeClass {
                                class_id: c.class_id.get().to_string(),
                                class_name: &c.class_name,
                            })
                            .collect(),
                    })
                    .collect();
                // Keep the complete first class page and explicit continuation. Shrink
                // only whole groups when escaped names need more JSON bytes.
                loop {
                    match wire::scopes(page.identity_revision, page.offset, page.total, &items) {
                        Ok(bytes) => return response(200, Ok(bytes), None),
                        Err(ProtocolError::TooLarge) if items.len() > 1 => {
                            items.pop();
                        }
                        Err(_) => return failure(503, responses::ErrorCode::ServiceUnavailable),
                    }
                }
            }
            Err(e) => error(e, false),
        },
        MaterialRoute::Classes { scope, offset } => {
            match backend.web_material_classes(lease, scope, offset) {
                Ok(page) => {
                    let items: Vec<_> = page
                        .items
                        .iter()
                        .map(|c| wire::ScopeClass {
                            class_id: c.class_id.get().to_string(),
                            class_name: &c.class_name,
                        })
                        .collect();
                    response(
                        200,
                        wire::classes(
                            page.identity_revision,
                            page.scope,
                            page.offset,
                            page.total,
                            &items,
                        ),
                        None,
                    )
                }
                Err(e) => error(e, false),
            }
        }
        MaterialRoute::List { scope, cursor } => {
            let cursor = cursor.map(|(folder_id, folder_revision, offset)| MaterialCursor {
                folder_id,
                folder_revision,
                scope,
                class_id: None,
                source: None,
                offset,
            });
            match backend.web_materials(lease, MaterialListRequest { scope, cursor }) {
                Ok(page) => response(200, wire::page(&page), None),
                Err(e) => error(e, false),
            }
        }
        MaterialRoute::Delete { scope } => {
            let input = match wire::delete(body) {
                Ok(i) => i,
                Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
            };
            match backend.web_material_delete(
                lease,
                MaterialDeleteRequest {
                    command_id: input.command_id,
                    scope,
                    material_id: input.material_id,
                    expected_material_revision: input.expected_material_revision,
                },
                input.resource_version_id,
            ) {
                Ok(record) => committed(input.command_id, scope, &record),
                Err(e) => error(e, true),
            }
        }
        MaterialRoute::Command { command_id } => {
            match backend.web_material_outcome(lease, command_id) {
                Ok(None) => response(200, wire::command(command_id, None), None),
                Ok(Some(outcome)) => response(
                    200,
                    wire::command(command_id, Some((outcome.scope, &outcome.record))),
                    None,
                ),
                Err(e) => error(e, false),
            }
        }
        MaterialRoute::UploadCreate { scope } => {
            let input = match wire::upload(body) {
                Ok(i) => i,
                Err(_) => return failure(400, responses::ErrorCode::InvalidResource),
            };
            #[cfg(windows)]
            {
                app.material_create(backend, lease, scope, input)
            }
            #[cfg(not(windows))]
            {
                let _ = (app, backend, lease, scope, input);
                failure(503, responses::ErrorCode::ServiceUnavailable)
            }
        }
        MaterialRoute::UploadCancel { upload_id } => {
            if wire::cancel(body).is_err() {
                return failure(400, responses::ErrorCode::InvalidRequest);
            }
            #[cfg(windows)]
            {
                app.material_cancel(lease, upload_id)
            }
            #[cfg(not(windows))]
            {
                let _ = (app, lease, upload_id);
                failure(503, responses::ErrorCode::ServiceUnavailable)
            }
        }
        MaterialRoute::Append { scope } => {
            let input = match wire::append(body) {
                Ok(i) => i,
                Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
            };
            #[cfg(windows)]
            {
                app.material_append(backend, lease, scope, input)
            }
            #[cfg(not(windows))]
            {
                let _ = (app, backend, lease, scope, input);
                failure(503, responses::ErrorCode::ServiceUnavailable)
            }
        }
        _ => failure(400, responses::ErrorCode::InvalidRequest),
    }
}
pub(super) fn committed(
    id: fc_domain::ids::CommandId,
    scope: MaterialScope,
    record: &MaterialRecord,
) -> WebResponse {
    match wire::committed(id, scope, record) {
        Ok(bytes) => response(200, Ok(bytes), None),
        Err(_) => failure(503, responses::ErrorCode::ResultUnknown),
    }
}
pub(super) fn error(e: BackendError, mutating: bool) -> WebResponse {
    match e {
        BackendError::Material(MaterialRepositoryError::Domain(e)) => match e {
            MaterialError::Permission(_)
            | MaterialError::InvalidScope
            | MaterialError::Identity(_) => failure(403, responses::ErrorCode::Forbidden),
            MaterialError::RevisionConflict { .. } => {
                failure(409, responses::ErrorCode::RevisionConflict)
            }
            MaterialError::StaleCursor => failure(409, responses::ErrorCode::StaleCursor),
            MaterialError::NotFound => failure(404, responses::ErrorCode::NotFound),
            MaterialError::InvalidName | MaterialError::InvalidResource => {
                failure(400, responses::ErrorCode::InvalidResource)
            }
            MaterialError::Capacity => failure(429, responses::ErrorCode::Capacity),
            _ => unknown(mutating),
        },
        BackendError::Material(MaterialRepositoryError::IdempotencyConflict) => {
            failure(409, responses::ErrorCode::IdempotencyConflict)
        }
        BackendError::Material(MaterialRepositoryError::QuotaExceeded) => {
            failure(429, responses::ErrorCode::Capacity)
        }
        BackendError::Session(_) if !mutating => failure(403, responses::ErrorCode::Forbidden),
        BackendError::InvalidInput if !mutating => {
            failure(400, responses::ErrorCode::InvalidRequest)
        }
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
