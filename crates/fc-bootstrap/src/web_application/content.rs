//! Teacher-content adaptation owns no authority: each operation revalidates the
//! original authenticated Web lease in Backend, including the writer guard.
use super::*;
use crate::backend::{BackendError, WebContentDocument, WebContentKind, WebContentScopes};
use fc_application::identity::SessionLease;
use fc_domain::content::{ContentCommand, ContentError};
use fc_ports::RepositoryError;
use fc_protocol::web::content::{self as wire, Kind, Scope};

pub(super) fn dispatch(
    backend: &mut Backend,
    lease: SessionLease,
    route: Route,
    body: &[u8],
) -> WebResponse {
    match route {
        Route::ContentScopes { kind, offset } => match backend.web_content_scopes(
            lease,
            match kind {
                Kind::Homework => WebContentKind::Homework,
                Kind::Reminder => WebContentKind::Reminder,
            },
            offset,
        ) {
            Ok(page) => response(200, scope_bytes(&page), None),
            Err(error) => backend_error(error, false),
        },
        Route::SubjectVisual { key } => {
            let fc_domain::content::ContentKey::Homework(key) = key.into_domain() else {
                return failure(400, responses::ErrorCode::InvalidRequest);
            };
            match backend.web_subject_visual_preview(lease, key) {
                Ok(p) => response(
                    200,
                    fc_protocol::web::subject_visual::preview(
                        fc_protocol::web::subject_visual::Preview {
                            key: &p.key,
                            subject_name: &p.subject_name,
                            subject_revision: p.subject_revision,
                            display_revision: p.display_revision,
                            effective_rgb: p.effective_rgb,
                            filled: p.filled,
                            homework_fill_percent: p.homework_fill_percent,
                            course_fill_percent: p.course_fill_percent,
                            homework_component_opacity_percent: p
                                .homework_component_opacity_percent,
                            course_component_opacity_percent: p.course_component_opacity_percent,
                            homework_visible: p.homework_visible,
                            course_visible: p.course_visible,
                        },
                    ),
                    None,
                ),
                Err(error) => backend_error(error, false),
            }
        }
        Route::ContentRead { key } => match backend.web_content_read(lease, key.into_domain()) {
            Ok(document) => response(200, document_bytes(&document), None),
            Err(error) => backend_error(error, false),
        },
        Route::ContentSave { key } => {
            let input = match wire::save(body) {
                Ok(input) => input,
                Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
            };
            let id = input.command_id;
            let command = input.command_ticket.map(|ticket| {
                fc_protocol::web::command_window::CommandReference {
                    ticket,
                    command_id: id,
                }
            });
            let issued = match command
                .map(|command| {
                    backend.resolve_web_command(
                        lease,
                        fc_ports::command_window::CommandFamily::Content,
                        command.ticket,
                        id,
                    )
                })
                .transpose()
            {
                Ok(issued) => issued,
                Err(error) => return backend_error(error, true),
            };
            if let Some(issued) = issued {
                let mut scope = match backend.begin_command_scope(issued) {
                    Ok(scope) => scope,
                    Err(error) => return backend_error(error, true),
                };
                perform_save(&mut scope, lease, key, input, command)
            } else {
                perform_save(backend, lease, key, input, command)
            }
        }
        Route::ContentWindowCommand { command } => {
            let issued = match backend.resolve_web_command(
                lease,
                fc_ports::command_window::CommandFamily::Content,
                command.ticket,
                command.command_id,
            ) {
                Ok(issued) => issued,
                Err(error) => return backend_error(error, false),
            };
            let scope = match backend.begin_command_scope(issued) {
                Ok(scope) => scope,
                Err(error) => return backend_error(error, false),
            };
            match scope.web_content_outcome(lease, command.command_id) {
                Ok(document) => {
                    let bytes = match document.as_ref() {
                        None => wire::command_result(command.command_id, None),
                        Some(document) => match document.record.as_ref() {
                            Some(record) => wire::command_result(
                                command.command_id,
                                Some((&document.key, document.now, document.expires_at, record)),
                            ),
                            None => return failure(503, responses::ErrorCode::ServiceUnavailable),
                        },
                    };
                    response(
                        200,
                        bytes.and_then(|bytes| {
                            fc_protocol::web::command_window::bind_response(&bytes, command)
                        }),
                        None,
                    )
                }
                Err(error) => backend_error(error, false),
            }
        }
        Route::ContentCommand { command_id } => {
            match backend.web_content_outcome(lease, command_id) {
                Ok(None) => response(200, wire::command_result(command_id, None), None),
                Ok(Some(document)) => {
                    let Some(record) = document.record.as_ref() else {
                        return failure(503, responses::ErrorCode::ServiceUnavailable);
                    };
                    response(
                        200,
                        wire::command_result(
                            command_id,
                            Some((&document.key, document.now, document.expires_at, record)),
                        ),
                        None,
                    )
                }
                Err(error) => backend_error(error, false),
            }
        }
        _ => failure(404, responses::ErrorCode::NotFound),
    }
}

fn perform_save(
    backend: &mut Backend,
    lease: SessionLease,
    key: wire::Key,
    input: wire::SaveInput,
    command: Option<fc_protocol::web::command_window::CommandReference>,
) -> WebResponse {
    let id = input.command_id;
    match backend.web_content_save(
        lease,
        ContentCommand {
            command_id: id,
            key: key.into_domain(),
            expected_revision: input.expected_revision,
            text: input.text,
        },
    ) {
        Ok(document) => {
            let Some(record) = document.record.as_ref() else {
                return failure(503, responses::ErrorCode::ResultUnknown);
            };
            // A durable mutation may already have committed even if the
            // response cannot be encoded. Never label it uncommitted.
            match wire::committed(id, &document.key, document.now, document.expires_at, record)
                .and_then(|bytes| match command {
                    Some(command) => {
                        fc_protocol::web::command_window::bind_response(&bytes, command)
                    }
                    None => Ok(bytes),
                }) {
                Ok(body) => response(200, Ok(body), None),
                Err(_) => failure(503, responses::ErrorCode::ResultUnknown),
            }
        }
        Err(error) => backend_error(error, true),
    }
}
fn document_bytes(document: &WebContentDocument) -> Result<Vec<u8>, ProtocolError> {
    wire::document(
        &document.key,
        document.now,
        document.expires_at,
        document.record.as_ref(),
    )
}
fn scope_bytes(page: &WebContentScopes) -> Result<Vec<u8>, ProtocolError> {
    let kind = match page.kind {
        WebContentKind::Homework => Kind::Homework,
        WebContentKind::Reminder => Kind::Reminder,
    };
    let items = page
        .items
        .iter()
        .map(|item| {
            Ok(match kind {
                Kind::Homework => Scope::Homework {
                    class_id: item.class_id,
                    class_name: &item.class_name,
                    subject_id: item.subject_id.ok_or(ProtocolError::InvalidCommand)?,
                    subject_name: item
                        .subject_name
                        .as_deref()
                        .ok_or(ProtocolError::InvalidCommand)?,
                    owner_teacher_id: item.owner_teacher_id.ok_or(ProtocolError::InvalidCommand)?,
                    owner_display_name: item
                        .owner_display_name
                        .as_deref()
                        .ok_or(ProtocolError::InvalidCommand)?,
                },
                Kind::Reminder => Scope::Reminder {
                    class_id: item.class_id,
                    class_name: &item.class_name,
                },
            })
        })
        .collect::<Result<Vec<_>, ProtocolError>>()?;
    wire::scopes(
        kind,
        page.today,
        page.identity_revision,
        page.offset,
        page.total,
        &items,
    )
}
fn backend_error(error: BackendError, mutating: bool) -> WebResponse {
    if let Some(response) = command_failure(&error) {
        return response;
    }
    match error {
        BackendError::SubjectVisual(
            fc_ports::subject_visual::SubjectVisualRepositoryError::Content(error),
        ) => backend_error(
            BackendError::Content(RepositoryError::Domain(error)),
            mutating,
        ),
        BackendError::Identity(fc_ports::IdentityRepositoryError::Domain(
            fc_domain::identity::IdentityError::Permission(_),
        )) => failure(403, responses::ErrorCode::Forbidden),
        BackendError::SubjectVisual(
            fc_ports::subject_visual::SubjectVisualRepositoryError::Domain(
                fc_domain::subject_visual::SubjectVisualError::Identity(
                    fc_domain::identity::IdentityError::Permission(_),
                ),
            ),
        ) => failure(403, responses::ErrorCode::Forbidden),
        BackendError::Content(RepositoryError::Domain(error)) => match error {
            ContentError::Permission(_) => failure(403, responses::ErrorCode::Forbidden),
            ContentError::WrongDate => failure(409, responses::ErrorCode::DayChanged),
            ContentError::RevisionConflict { .. } => {
                failure(409, responses::ErrorCode::RevisionConflict)
            }
            ContentError::IdempotencyConflict => {
                failure(409, responses::ErrorCode::IdempotencyConflict)
            }
            ContentError::TextTooLarge if mutating => {
                failure(400, responses::ErrorCode::InvalidRequest)
            }
            _ => failure(
                503,
                if mutating {
                    responses::ErrorCode::ResultUnknown
                } else {
                    responses::ErrorCode::ServiceUnavailable
                },
            ),
        },
        BackendError::Session(_) if !mutating => failure(403, responses::ErrorCode::Forbidden),
        BackendError::InvalidInput if !mutating => {
            failure(400, responses::ErrorCode::InvalidRequest)
        }
        _ => failure(
            503,
            if mutating {
                responses::ErrorCode::ResultUnknown
            } else {
                responses::ErrorCode::ServiceUnavailable
            },
        ),
    }
}
