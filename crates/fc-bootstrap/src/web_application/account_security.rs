//! Existing credential use cases; bounded receipt metadata never retains passwords/codes.
use super::*;
use crate::backend::{AccountReceiptProof, BackendError};
use fc_application::identity::SessionLease;
use fc_domain::ids::{PrincipalId, Revision};
use fc_protocol::web::admin::{self as wire, account_security as protocol};
use fc_security::{Password, RecoveryCode};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
pub(super) struct Receipt {
    verifier: fc_security::web::SecretVerifier,
    owner: PrincipalId,
    target: PrincipalId,
    expected: Revision,
    issued: fc_ports::command_window::IssuedCommand,
    proof: Option<Arc<AccountReceiptProof>>,
    submitted: bool,
    created: Instant,
}
fn invalid() -> WebResponse {
    failure(400, responses::ErrorCode::InvalidRequest)
}
fn reply(
    backend: &Backend,
    write: impl FnOnce(Revision) -> Result<Vec<u8>, ProtocolError>,
) -> WebResponse {
    match backend.state() {
        Ok(state) => admin::data_response(write(state.revision)),
        Err(e) => admin::backend_error(e),
    }
}
fn credential_error(error: BackendError) -> WebResponse {
    match error {
        BackendError::InvalidCredentials => failure(403, responses::ErrorCode::Forbidden),
        BackendError::Storage(fc_storage::StorageError::LoginRateLimited { .. }) => {
            failure(429, responses::ErrorCode::RateLimited)
        }
        e => admin::backend_error(e),
    }
}
pub(super) fn setup_response(
    app: &WebApplication,
    token: &PageToken,
    code: RecoveryCode,
) -> WebResponse {
    let mut result = app.page_session_response(token);
    if result.head.status == 200 {
        if let Ok(body) = protocol::session_code(&result.body, code.reveal().as_str()) {
            result.head.content_length = body.len() as u64;
            result.body = body;
        }
    }

    result
}
pub(super) fn recover(app: &WebApplication, backend: &mut Backend, bytes: &[u8]) -> WebResponse {
    if !app.local_control {
        return failure(403, responses::ErrorCode::Forbidden);
    }
    let input = match protocol::recover(bytes) {
        Ok(v) => v,
        Err(_) => return invalid(),
    };
    let code = match RecoveryCode::parse(input.recovery_code) {
        Ok(v) => v,
        Err(_) => return invalid(),
    };
    let password = match Password::new(input.new_password) {
        Ok(v) => v,
        Err(_) => return invalid(),
    };
    match backend.recover(&input.login_name, code, password) {
        Ok(code) => reply(backend, |revision| {
            protocol::code(revision, code.reveal().as_str())
        }),
        Err(e) => credential_error(e),
    }
}
pub(super) fn dispatch(
    app: &WebApplication,
    backend: &mut Backend,
    lease: SessionLease,
    bytes: &[u8],
) -> WebResponse {
    let input = match protocol::input(bytes) {
        Ok(v) => v,
        Err(_) => return invalid(),
    };
    if input.action == "rotateRecovery" {
        let password = match input.current_password.and_then(|v| Password::new(v).ok()) {
            Some(v) => v,
            None => return invalid(),
        };
        let issued = match backend.issue_for_web_account(lease) {
            Ok(v) => v,
            Err(e) => return admin::backend_error(e),
        };
        let mut scoped = match backend.begin_command_scope(issued) {
            Ok(v) => v,
            Err(e) => return admin::backend_error(e),
        };
        return match scoped.rotate_recovery(lease, password) {
            Ok(code) => reply(&scoped, |revision| {
                protocol::code(revision, code.reveal().as_str())
            }),
            Err(e) => credential_error(e),
        };
    }
    if !app.local_control {
        return failure(403, responses::ErrorCode::Forbidden);
    }
    let mut receipts = match app.account_receipts.try_lock() {
        Ok(v) => v,
        Err(_) => return failure(503, responses::ErrorCode::ServiceUnavailable),
    };
    receipts.retain(|v| v.created.elapsed() < Duration::from_secs(3600));
    if input.action == "prepareTransfer" {
        let current = match backend.management_identity(lease) {
            Ok(v) => v,
            Err(e) => return admin::backend_error(e),
        };
        let expected = match input
            .expected_revision
            .as_deref()
            .and_then(|v| wire::revision(v).ok())
        {
            Some(v) if v == current.revision => v,
            _ => return failure(409, responses::ErrorCode::RevisionConflict),
        };
        let target = match input
            .target_id
            .and_then(|v| wire::revision(&v).ok())
            .and_then(|v| PrincipalId::new(v.get()).ok())
        {
            Some(v) if v != lease.principal_id() => v,
            _ => return invalid(),
        };
        let target_account = match current.account(target) {
            Ok(v)
                if v.principal.enabled
                    && !v.must_change_password
                    && v.principal.role != fc_domain::auth::Role::GlobalAdmin =>
            {
                v
            }
            _ => return invalid(),
        };
        let _ = target_account;
        if receipts.len() >= 64 {
            return failure(503, responses::ErrorCode::ServiceUnavailable);
        }
        let issued = match backend
            .issue_for_web_admin(lease, fc_ports::command_window::CommandFamily::Identity)
        {
            Ok(v) => v,
            Err(e) => return admin::backend_error(e),
        };
        let ticket = match backend.account_ticket() {
            Ok(v) => v,
            Err(e) => return admin::backend_error(e),
        };
        receipts.push(Receipt {
            verifier: ticket.verifier(),
            owner: lease.principal_id(),
            target,
            expected,
            issued,
            proof: None,
            submitted: false,
            created: Instant::now(),
        });
        return reply(backend, |revision| {
            protocol::ticket(revision, ticket.expose_hex().as_str())
        });
    }
    let ticket = match input.ticket.and_then(|v| WebSecret::parse_hex(v).ok()) {
        Some(v) => v,
        None => return invalid(),
    };
    let entry = match receipts
        .iter_mut()
        .find(|v| v.verifier.verify(&ticket) && v.owner == lease.principal_id())
    {
        Some(v) => v,
        None => return failure(403, responses::ErrorCode::Forbidden),
    };
    let mut scoped = match backend.begin_command_scope(entry.issued) {
        Ok(v) => v,
        Err(e) => return admin::backend_error(e),
    };
    if input.action == "transfer" {
        if entry.submitted
            || input.reviewed_target != Some(true)
            || input.understood_revocation != Some(true)
        {
            return invalid();
        }
        let password = match input.current_password.and_then(|v| Password::new(v).ok()) {
            Some(v) => v,
            None => return invalid(),
        };
        entry.submitted = true;
        let result = scoped.transfer_administrator_at_tracked(
            lease,
            entry.expected,
            entry.target,
            password,
            |proof| {
                entry.proof = Some(Arc::new(proof));
                Ok(())
            },
        );
        return match result {
            Ok(_) => reply(&scoped, |revision| {
                protocol::outcome(revision, "COMMITTED", None)
            }),
            Err(e) => credential_error(e),
        };
    }
    if input.action == "outcome" {
        return match &entry.proof {
            None => reply(&scoped, |revision| {
                protocol::outcome(revision, "NOT_PREPARED", None)
            }),
            Some(proof) => match scoped.account_outcome(lease, proof.clone()) {
                Ok(value) => reply(&scoped, |revision| {
                    protocol::outcome(
                        revision,
                        if value.is_some() {
                            "COMMITTED"
                        } else {
                            "NOT_FOUND"
                        },
                        value.map(|v| v.revision),
                    )
                }),
                Err(e) => admin::backend_error(e),
            },
        };
    }
    invalid()
}
