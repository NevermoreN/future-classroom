//! Public calendar management from the HTTPS loopback control plane.
use super::*;
use crate::{backend::CalendarManualStart, web_transfer::TransferLifetime};
use fc_application::identity::SessionPurpose;
use fc_ports::{calendar::*, command_window::CommandFamily};
use fc_runtime::worker::{JobContext, JobError};
use std::sync::Mutex;

impl WebApplication {
    pub(crate) fn calendar_in_job(
        &self,
        backend: &Mutex<Backend>,
        head: &RequestHead,
        body: Option<&[u8]>,
        year: i32,
        life: &TransferLifetime,
        context: &JobContext,
    ) -> WebResponse {
        if !self.local_control {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        let route = match self.boundary.preflight(head) {
            Ok(value) => value,
            Err(error) => return self.request_boundary_error(head, error),
        };
        if body.map_or(0, |bytes| bytes.len() as u64) != head.content_length.unwrap_or(0)
            || head.content_length.unwrap_or(0) > route.body_limit()
        {
            return boundary_error(WebError::Framing);
        }
        let session = match self.authenticate_route(head, route) {
            Ok(Some(session)) if session.account.principal.role == Role::GlobalAdmin => session,
            Ok(_) => return failure(403, responses::ErrorCode::Forbidden),
            Err(response) => return response,
        };
        let command = match body.map(wire::calendar::command).transpose() {
            Ok(command) => command,
            Err(_) => return invalid(),
        };
        let lease = session.lease();
        let execution = life.job_fence(context);
        let admit = || {
            if !execution() {
                return Err(BackendError::Closed);
            }
            let mut backend = backend.lock().map_err(|_| BackendError::Closed)?;
            if !self.service.same_instance(&backend.web_service()) {
                return Err(BackendError::Closed);
            }
            let mut scoped = backend.begin_execution_scope(execution.clone());
            scoped.check_session(lease, SessionPurpose::Manage)?;
            let Some(command) = command else {
                return Ok(None);
            };
            let issued = scoped.issue_for_web_admin(lease, CommandFamily::Calendar)?;
            let mut writer = scoped.begin_command_scope(issued)?;
            match command {
                wire::calendar::Command::Download => match writer
                    .begin_calendar_manual_fetch_while(lease, year, execution.clone())?
                {
                    CalendarManualStart::Fresh(permit) => Ok(Some(permit)),
                    CalendarManualStart::History(_) => Ok(None),
                },
                wire::calendar::Command::Adopt {
                    expected,
                    candidate,
                } => {
                    writer.adopt_calendar_while(
                        lease,
                        expected,
                        vec![CalendarCandidateRef {
                            year,
                            revision: candidate,
                        }],
                        execution.clone(),
                    )?;
                    Ok(None)
                }
                wire::calendar::Command::SetAutomatic { expected, enabled } => {
                    writer.set_calendar_auto_policy_while(
                        lease,
                        expected,
                        enabled,
                        execution.clone(),
                    )?;
                    Ok(None)
                }
            }
        };
        match admit() {
            Err(error) => return calendar_error(error),
            Ok(None) => (),
            Ok(Some(permit)) => {
                // The same admitted shared worker owns the fixed provider and
                // its exact writer receipt. No DB lock spans native HTTPS.
                let cancelled = || !execution() || !self.service.web_session_current(lease);
                let candidate = fc_windows::calendar_download::HolidayCnProvider.fetch(
                    permit.year(),
                    Some(&permit.validators()),
                    &cancelled,
                );
                let outcome = match candidate {
                    _ if context.checkpoint() == Err(JobError::Deadline) => {
                        CalendarFetchOutcome::Failed(CalendarFetchFailure::Deadline)
                    }
                    _ if cancelled() => {
                        CalendarFetchOutcome::Failed(CalendarFetchFailure::Cancelled)
                    }
                    Ok(CalendarProviderResponse::NotModified) => CalendarFetchOutcome::NotModified,
                    Ok(CalendarProviderResponse::Downloaded(candidate)) => {
                        CalendarFetchOutcome::Downloaded(candidate)
                    }
                    Err(error) => CalendarFetchOutcome::Failed(error),
                };
                let Ok(mut backend) = backend.lock() else {
                    return failure(503, responses::ErrorCode::ServiceUnavailable);
                };
                if let Err(error) =
                    backend.finish_calendar_manual_fetch_while(permit, outcome, execution.clone())
                {
                    return calendar_error(error);
                }
            }
        }
        let load = || {
            let Ok(mut backend) = backend.lock() else {
                return Err(BackendError::Closed);
            };
            let scoped = backend.begin_execution_scope(execution);
            Ok((
                scoped.read_schedule(lease)?,
                scoped.calendar_status(lease, year)?,
                scoped.management_identity(lease)?,
            ))
        };
        match load() {
            Ok((schedule, status, identity)) => {
                data_response(wire::calendar::document(&schedule, &status, &identity))
            }
            Err(error) => calendar_error(error),
        }
    }
}

fn calendar_error(error: BackendError) -> WebResponse {
    match error {
        BackendError::Storage(fc_storage::StorageError::Conflict { .. })
        | BackendError::Calendar(CalendarRepositoryError::Stale) => {
            failure(409, responses::ErrorCode::RevisionConflict)
        }
        BackendError::Calendar(CalendarRepositoryError::Denied) => {
            failure(403, responses::ErrorCode::Forbidden)
        }
        BackendError::Calendar(CalendarRepositoryError::Invalid) => invalid(),
        other => backend_error(other),
    }
}
