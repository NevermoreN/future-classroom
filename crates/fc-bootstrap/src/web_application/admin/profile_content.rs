use super::*;
use fc_domain::profile_content::{
    ContentOperation, ContentPayload, ContentPayloadKind, ContentSlotKey,
};
pub(super) fn dispatch(
    backend: &mut Backend,
    lease: SessionLease,
    slot: ContentSlotKey,
    body: Option<&[u8]>,
) -> WebResponse {
    let kind = match backend.web_profile_content_kind(lease, slot) {
        Ok(kind) => kind,
        Err(error) => return backend_error(error),
    };
    let Some(body) = body else {
        return match backend.read_profile_content(lease, slot) {
            Ok(document) => data_response(wire::profile_content::document(
                &document,
                kind,
                backend.web_profile_content_can_page(lease, &document),
            )),
            Err(error) => backend_error(error),
        };
    };
    let (expected, change) = match wire::profile_content::mutation(body) {
        Ok(value) => value,
        Err(_) => return invalid(),
    };
    use wire::profile_content::Change;
    let (operation, payload) = match change {
        Change::PublishText(text) => (
            ContentOperation::Publish(ContentPayloadKind::Text),
            ContentPayload::Text(text),
        ),
        Change::Clear => (ContentOperation::Clear, ContentPayload::Empty),
        Change::Undo(target_revision) => (
            ContentOperation::Undo { target_revision },
            ContentPayload::Empty,
        ),
        Change::Page {
            page,
            expected_view_revision,
        } => {
            return match backend.page_profile_content_as_manager(
                lease,
                slot,
                expected,
                expected_view_revision,
                page,
            ) {
                Ok(document) => data_response(wire::profile_content::document(
                    &document,
                    kind,
                    backend.web_profile_content_can_page(lease, &document),
                )),
                Err(error) => backend_error(error),
            }
        }
    };
    match backend.publish_web_profile_content(lease, slot, expected, operation, payload) {
        Ok(document) => data_response(wire::profile_content::document(
            &document,
            kind,
            backend.web_profile_content_can_page(lease, &document),
        )),
        Err(error) => backend_error(error),
    }
}
