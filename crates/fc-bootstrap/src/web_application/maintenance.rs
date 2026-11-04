//! Authenticated, local-only maintenance preparation. Preparation holds no file
//! capability; execute it on the original worker after releasing Backend's lock.
use super::*;
use crate::{
    backend::OrdinaryBackupAccess,
    maintenance_web::{self as jobs, MaintenanceControl, MaintenanceError, MaintenanceService},
};
use fc_application::identity::{SessionLease, SessionPurpose};
use fc_domain::{auth::Role, ids::Revision};
use fc_ports::RandomPort;
use fc_protocol::web::maintenance::{self as wire, Resource};
use std::sync::Arc;

pub struct MaintenanceRequest {
    lease: SessionLease,
    revision: Revision,
    action: Action,
}
enum Action {
    Create(wire::Create),
    Status(u64, WebSecret),
    Preflight(u64, WebSecret),
    Mapping(u64, WebSecret, wire::Mapping),
    Confirm(u64, WebSecret, wire::Confirm),
    Cancel(u64, WebSecret),
}
pub struct MaintenanceTransfer {
    pub lease: SessionLease,
    pub id: u64,
    pub proof: WebSecret,
    pub access: OrdinaryBackupAccess,
}
impl WebApplication {
    fn maintenance_session(
        &self,
        backend: &Backend,
        head: &RequestHead,
    ) -> Result<(Resource, SessionLease), WebResponse> {
        if !self.local_control || !self.service.same_instance(&backend.web_service()) {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        let route = self
            .boundary
            .preflight(head)
            .map_err(|e| self.request_boundary_error(head, e))?;
        let Route::Maintenance { resource, .. } = route else {
            return Err(failure(404, responses::ErrorCode::NotFound));
        };
        let session = self
            .authenticate_route(head, route)?
            .ok_or_else(|| failure(401, responses::ErrorCode::Unauthorized))?;
        if session.account.principal.role != Role::GlobalAdmin {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        backend
            .check_session(session.lease(), SessionPurpose::Manage)
            .map_err(admin::backend_error)?;
        Ok((resource, session.lease()))
    }
    pub fn prepare_maintenance(
        &self,
        backend: &Backend,
        head: &RequestHead,
        body: &[u8],
    ) -> Result<MaintenanceRequest, WebResponse> {
        let (resource, lease) = self.maintenance_session(backend, head)?;
        let route = self
            .boundary
            .preflight(head)
            .map_err(|e| self.request_boundary_error(head, e))?;
        if body.len() as u64 != head.content_length.unwrap_or(0)
            || body.len() as u64 > route.body_limit()
        {
            return Err(boundary_error(WebError::Framing));
        }
        let revision = backend.global_revision().map_err(admin::backend_error)?;
        let invalid = || failure(400, responses::ErrorCode::InvalidRequest);
        let action = match resource {
            Resource::Jobs => {
                let create = wire::Create::decode(body).map_err(|_| invalid())?;
                if create.revision().map_err(|_| invalid())? != revision {
                    return Err(failure(409, responses::ErrorCode::RevisionConflict));
                }
                Action::Create(create)
            }
            Resource::Job(id) if body.is_empty() => Action::Status(id, proof(head)?),
            Resource::Preflight(id) => {
                wire::empty_action(body).map_err(|_| invalid())?;
                Action::Preflight(id, proof(head)?)
            }
            Resource::Mapping(id) => Action::Mapping(
                id,
                proof(head)?,
                wire::Mapping::decode(body).map_err(|_| invalid())?,
            ),
            Resource::Confirm(id) => Action::Confirm(
                id,
                proof(head)?,
                wire::Confirm::decode(body).map_err(|_| invalid())?,
            ),
            Resource::Cancel(id) => {
                wire::empty_action(body).map_err(|_| invalid())?;
                Action::Cancel(id, proof(head)?)
            }
            _ => return Err(invalid()),
        };
        Ok(MaintenanceRequest {
            lease,
            revision,
            action,
        })
    }
    pub fn prepare_maintenance_transfer(
        &self,
        backend: &Backend,
        head: &RequestHead,
    ) -> Result<MaintenanceTransfer, WebResponse> {
        let (resource, lease) = self.maintenance_session(backend, head)?;
        let id = match resource {
            Resource::Upload(id) | Resource::File(id) => id,
            _ => return Err(failure(400, responses::ErrorCode::InvalidRequest)),
        };
        let revision = backend.global_revision().map_err(admin::backend_error)?;
        let access = backend
            .ordinary_backup_access(lease, revision)
            .map_err(admin::backend_error)?;
        Ok(MaintenanceTransfer {
            lease,
            id,
            proof: proof(head)?,
            access,
        })
    }
}
impl MaintenanceRequest {
    pub fn execute(
        self,
        service: &Arc<MaintenanceService>,
        random: &mut impl RandomPort,
    ) -> WebResponse {
        let revision = self.revision;
        let result = match self.action {
            Action::Create(request) => {
                return match service.create(self.lease, request, random) {
                    Ok(created) => admin::data_response(wire::created(
                        revision,
                        &created.snapshot,
                        created.proof.as_str(),
                    )),
                    Err(e) => error_response(e),
                }
            }
            Action::Status(id, proof) => service
                .context(id, &proof, self.lease)
                .and_then(|_| service.control.snapshot(id, &proof)),
            Action::Preflight(id, proof) => service
                .request_preflight(id, &proof, self.lease)
                .and_then(|_| service.control.snapshot(id, &proof)),
            Action::Mapping(id, proof, mapping) => service
                .request_mapping(id, &proof, self.lease, mapping)
                .and_then(|_| service.control.snapshot(id, &proof)),
            Action::Confirm(id, proof, confirm) => service
                .request_confirm(id, &proof, self.lease, confirm)
                .and_then(|_| service.control.snapshot(id, &proof)),
            Action::Cancel(id, proof) => service
                .context(id, &proof, self.lease)
                .and_then(|_| service.control.cancel(id, &proof))
                .and_then(|_| service.control.snapshot(id, &proof)),
        };
        snapshot_response(revision, result)
    }
}
pub fn proof(head: &RequestHead) -> Result<WebSecret, WebResponse> {
    let mut headers = head
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case(wire::PROOF_HEADER));
    let header = match (headers.next(), headers.next()) {
        (Some(v), None) => v,
        _ => return Err(failure(403, responses::ErrorCode::Forbidden)),
    };
    std::str::from_utf8(&header.value)
        .ok()
        .and_then(|v| WebSecret::parse_hex(v.to_owned()).ok())
        .ok_or_else(|| failure(403, responses::ErrorCode::Forbidden))
}
/// Caller has verified real LocalControl scope and same-origin request metadata.
/// The proof discloses only this job, never a general administrator session.
pub fn detached(
    control: &MaintenanceControl,
    resource: Resource,
    head: &RequestHead,
    body: &[u8],
) -> WebResponse {
    if body.len() as u64 != head.content_length.unwrap_or(0)
        || (matches!(resource, Resource::Cancel(_)) && wire::empty_action(body).is_err())
        || (matches!(resource, Resource::Job(_)) && !body.is_empty())
    {
        return failure(400, responses::ErrorCode::InvalidRequest);
    }
    let proof = match proof(head) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let result = match resource {
        Resource::Job(id) => control.detached_snapshot(id, &proof),
        Resource::Cancel(id) => control
            .detached_snapshot(id, &proof)
            .and_then(|_| control.cancel(id, &proof))
            .and_then(|_| control.detached_snapshot(id, &proof)),
        _ => Err(MaintenanceError::Denied),
    };
    snapshot_response(Revision::default(), result)
}
fn snapshot_response(revision: Revision, result: jobs::Result<wire::Snapshot>) -> WebResponse {
    match result {
        Ok(job) => admin::data_response(wire::status(revision, job)),
        Err(e) => error_response(e),
    }
}
pub fn error_response(error: MaintenanceError) -> WebResponse {
    match error {
        MaintenanceError::Denied => failure(403, responses::ErrorCode::Forbidden),
        MaintenanceError::Invalid => failure(400, responses::ErrorCode::InvalidRequest),
        MaintenanceError::Busy | MaintenanceError::Conflict | MaintenanceError::Stale => {
            failure(409, responses::ErrorCode::RevisionConflict)
        }
        MaintenanceError::Cancelled
        | MaintenanceError::Unavailable
        | MaintenanceError::Retained => failure(503, responses::ErrorCode::ServiceUnavailable),
    }
}
