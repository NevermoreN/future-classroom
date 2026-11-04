use super::*;
use fc_domain::{
    ids::ProfileId,
    profile::{SubjectProfileDocument, SubjectProfileKey},
};
pub(super) fn list(
    backend: &Backend,
    lease: SessionLease,
    after: Option<ProfileId>,
) -> WebResponse {
    match backend.list_subject_profiles(lease, after) {
        Ok(page) => data_response(wire::profiles::page(&page)),
        Err(error) => backend_error(error),
    }
}
pub(super) fn dispatch(
    backend: &mut Backend,
    lease: SessionLease,
    key: SubjectProfileKey,
    body: Option<&[u8]>,
) -> WebResponse {
    let current = match backend.read_subject_profile(lease, key) {
        Ok(current) => current,
        Err(error) => return backend_error(error),
    };
    let Some(body) = body else {
        return match current {
            Some(profile) => data_response(wire::profiles::document(&profile, true)),
            None => match ProfileId::new(1) {
                Ok(id) => data_response(wire::profiles::document(
                    &SubjectProfileDocument::new(id, key),
                    false,
                )),
                Err(_) => invalid(),
            },
        };
    };
    let (expected, candidate) = match wire::profiles::mutation(body, key, current.as_ref()) {
        Ok(value) => value,
        Err(_) => return invalid(),
    };
    let issued = match backend
        .issue_for_web_admin(lease, fc_ports::command_window::CommandFamily::Profiles)
    {
        Ok(issued) => issued,
        Err(error) => return backend_error(error),
    };
    let mut scoped = match backend.begin_command_scope(issued) {
        Ok(scoped) => scoped,
        Err(error) => return backend_error(error),
    };
    match scoped.save_subject_profile(lease, key, expected, candidate) {
        Ok(profile) => data_response(wire::profiles::document(&profile, true)),
        Err(error) => backend_error(error),
    }
}
