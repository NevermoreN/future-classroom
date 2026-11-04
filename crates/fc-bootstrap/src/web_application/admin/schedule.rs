//! Timetable mutations reuse the original aggregate, authorization and CAS writer.
use crate::backend::Backend;
use crate::web_application::{failure, WebResponse};
use fc_application::identity::SessionLease;
use fc_protocol::web::{admin::schedule, responses};

pub(super) fn dispatch(
    backend: &mut Backend,
    lease: SessionLease,
    body: Option<&[u8]>,
) -> WebResponse {
    let current = match backend.read_schedule(lease) {
        Ok(v) => v,
        Err(e) => return super::backend_error(e),
    };
    let document = match body {
        None => current,
        Some(bytes) => {
            let identity = match backend.management_identity(lease) {
                Ok(value) => value,
                Err(error) => return super::backend_error(error),
            };
            let (expected, candidate) = match schedule::mutation(bytes, &current, &identity) {
                Ok(v) => v,
                Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
            };
            let issued = match backend
                .issue_for_web_admin(lease, fc_ports::command_window::CommandFamily::Schedule)
            {
                Ok(issued) => issued,
                Err(error) => return super::backend_error(error),
            };
            let mut scoped = match backend.begin_command_scope(issued) {
                Ok(scoped) => scoped,
                Err(error) => return super::backend_error(error),
            };
            match scoped.save_schedule(lease, expected, candidate) {
                Ok(v) => v,
                Err(e) => return super::backend_error(e),
            }
        }
    };
    super::data_response(schedule::document(&document))
}
