//! Reviewed voice management on the original worker, without a Backend lock
//! around native output queries or policy runtime handoff.
use super::*;
use crate::{
    backend::{BackendError, VoiceDeviceCheckResult, VoiceManagementSnapshot, VoiceService},
    web_transfer::{ExecutionFence, TransferLifetime},
};
use fc_application::identity::SessionLease;
use fc_domain::{auth::Role, ids::Revision, voice::VoiceError};
use fc_ports::{
    audio::{AudioOutputError, DefaultOutputSupport},
    voice::VoiceRepositoryError,
};
use fc_protocol::web::admin::{self as wire, voice as dto, Resource};
use fc_runtime::worker::JobContext;
pub(crate) struct PreparedVoice {
    service: VoiceService,
    lease: SessionLease,
    change: Option<(Revision, dto::Change)>,
    fence: ExecutionFence,
}
fn error(error: BackendError) -> WebResponse {
    match error {
        BackendError::Voice(VoiceRepositoryError::Domain(
            VoiceError::RevisionConflict | VoiceError::StaleGeneration,
        )) => failure(409, responses::ErrorCode::RevisionConflict),
        BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::Permission(_))) => {
            failure(403, responses::ErrorCode::Forbidden)
        }
        BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::InvalidState)) => {
            failure(400, responses::ErrorCode::InvalidRequest)
        }
        other => admin::backend_error(other),
    }
}
fn view(snapshot: VoiceManagementSnapshot, lease: SessionLease) -> WebResponse {
    let observation = snapshot
        .current_device_observation(lease)
        .map(|v| dto::Observation {
            result: match v.result {
                VoiceDeviceCheckResult::Supported => "SUPPORTED",
                VoiceDeviceCheckResult::FormatUnsupported => "FORMAT_UNSUPPORTED",
                VoiceDeviceCheckResult::Unavailable => "UNAVAILABLE",
                VoiceDeviceCheckResult::Failed => "FAILED",
            },
            observed_at: format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                v.observed_at.date.year(),
                v.observed_at.date.month(),
                v.observed_at.date.day(),
                v.observed_at.second_of_day() / 3600,
                (v.observed_at.second_of_day() / 60) % 60,
                v.observed_at.second_of_day() % 60
            ),
            remaining_validity_millis: v.remaining_validity().as_millis() as u64,
        });
    admin::data_response(wire::encode(
        snapshot.revision,
        &dto::Data {
            publication_revision: snapshot.publication_revision.get().to_string(),
            identity_revision: snapshot.identity_revision.get().to_string(),
            runtime_generation: snapshot.runtime_generation.to_string(),
            enabled: snapshot.enabled,
            admitting: snapshot.admitting,
            active_jobs: snapshot.active_jobs,
            queued_jobs: snapshot.queued_jobs,
            playing_jobs: snapshot.playing_jobs,
            limits: snapshot.limits.into(),
            device_observation: observation,
        },
    ))
}
impl WebApplication {
    pub(crate) fn prepare_admin_voice(
        &self,
        backend: &mut Backend,
        head: &RequestHead,
        body: &[u8],
        life: &TransferLifetime,
        context: &JobContext,
    ) -> Result<PreparedVoice, WebResponse> {
        let fence = life.job_fence(context);
        if head.token != life.token() || !fence() {
            return Err(failure(503, responses::ErrorCode::ServiceUnavailable));
        }
        let app = self.for_execution(fence.clone());
        let scope = backend.begin_execution_scope(fence.clone());
        if !app.service.same_instance(&scope.web_service()) {
            return Err(failure(503, responses::ErrorCode::ServiceUnavailable));
        }
        let route = app
            .boundary
            .preflight(head)
            .map_err(|e| app.request_boundary_error(head, e))?;
        let Route::Admin {
            resource: Resource::Voice,
            write,
        } = route
        else {
            return Err(failure(404, responses::ErrorCode::NotFound));
        };
        if body.len() as u64 != head.content_length.unwrap_or(0)
            || body.len() as u64 > route.body_limit()
        {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        }
        let session = app
            .authenticate_route(head, route)?
            .ok_or_else(|| failure(401, responses::ErrorCode::Unauthorized))?;
        if session.account.principal.role != Role::GlobalAdmin {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        let lease = session.lease();
        let change = if write {
            Some(
                dto::mutation(body)
                    .map_err(|_| failure(400, responses::ErrorCode::InvalidRequest))?,
            )
        } else {
            None
        };
        let mut service = scope.voice_service();
        if change
            .as_ref()
            .is_some_and(|(_, change)| !matches!(change, dto::Change::Check { .. }))
        {
            let issued = scope
                .issue_for_web_admin(lease, fc_ports::command_window::CommandFamily::Voice)
                .map_err(error)?;
            service = service.with_command_window(lease, issued).map_err(error)?;
        }
        Ok(PreparedVoice {
            service,
            lease,
            change,
            fence,
        })
    }
}
impl PreparedVoice {
    pub(crate) fn execute(
        self,
        context: &JobContext,
        query: impl FnOnce() -> Result<DefaultOutputSupport, AudioOutputError>,
    ) -> WebResponse {
        if !(self.fence)() {
            return failure(503, responses::ErrorCode::ServiceUnavailable);
        }
        let result = match self.change {
            None => self.service.management_snapshot(self.lease),
            Some((
                voice,
                dto::Change::Check {
                    publication,
                    identity,
                    runtime,
                },
            )) => self.service.check_default_output_reviewed(
                self.lease,
                publication,
                voice,
                identity,
                runtime,
                context,
                query,
            ),
            Some((
                voice,
                dto::Change::Enabled {
                    enabled,
                    identity,
                    runtime,
                },
            )) => self
                .service
                .set_enabled_reviewed(self.lease, enabled, voice, identity, runtime)
                .and_then(|_| self.service.management_snapshot(self.lease)),
            Some((
                voice,
                dto::Change::Limits {
                    limits,
                    identity,
                    runtime,
                },
            )) => self
                .service
                .set_limits_reviewed(self.lease, limits, voice, identity, runtime)
                .and_then(|_| self.service.management_snapshot(self.lease)),
        };
        if !(self.fence)() {
            return failure(503, responses::ErrorCode::ResultUnknown);
        }
        match result {
            Ok(snapshot) => view(snapshot, self.lease),
            Err(e) => error(e),
        }
    }
}
