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
                    match wire::committed(
                        id,
                        &document.key,
                        document.now,
                        document.expires_at,
                        record,
                    ) {
                        Ok(body) => response(200, Ok(body), None),
                        Err(_) => failure(503, responses::ErrorCode::ResultUnknown),
                    }
                }
                Err(error) => backend_error(error, true),
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
