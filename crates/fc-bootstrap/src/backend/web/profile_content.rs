use super::*;
use fc_domain::{
    layout::{LayoutScope, NodeKind, WidgetKind},
    profile_content::*,
};
use fc_ports::{prepare_profile_content, ProfileContentPreparation, ProfileContentRepositoryError};
struct Preparation {
    fingerprint: [u8; 32],
    operation: ContentOperation,
}
impl ProfileContentPreparation for Preparation {
    type Input = ContentPayload;
    fn sha256(&self, bytes: &[u8]) -> std::result::Result<[u8; 32], ProfileContentRepositoryError> {
        Ok(fc_security::sha256(bytes))
    }
    fn prepare(
        &mut self,
        admission: &ContentAdmission,
        payload: ContentPayload,
    ) -> std::result::Result<ContentPayload, ProfileContentRepositoryError> {
        if admission.request.operation != self.operation
            || admission.request.payload_fingerprint != self.fingerprint
            || fc_security::sha256(
                &payload
                    .fingerprint_bytes(admission.request.slot)
                    .map_err(ProfileContentRepositoryError::Domain)?,
            ) != self.fingerprint
        {
            return Err(ProfileContentRepositoryError::IdempotencyConflict);
        }
        Ok(payload)
    }
}
impl Backend {
    #[cfg(windows)]
    pub(crate) fn begin_web_profile_images(
        &mut self,
        lease: SessionLease,
        slot: ContentSlotKey,
        expected_revision: Revision,
        token: [u8; 32],
        bytes: &[u64],
    ) -> Result<(
        fc_ports::command_window::IssuedCommand,
        ContentAdmission,
        ProfileContentOwner,
        Option<MediaReservation>,
    )> {
        let issued = self.issue_for_web_admin(
            lease,
            fc_ports::command_window::CommandFamily::ProfileContent,
        )?;
        let mut scoped = self.begin_command_scope(issued)?;
        let now = scoped.clock.now();
        let seconds = now.second_of_day() + 120;
        let expires_at = LocalDateTime::new(
            if seconds < 86400 {
                now.date
            } else {
                next_day(now)?.date
            },
            seconds % 86400,
        )
        .map_err(|_| BackendError::InvalidInput)?;
        let fingerprint =
            crate::profile_content_files::ContentSources::web_image_fingerprint(slot, token)
                .map_err(BackendError::ProfileContent)?;
        let command_id = scoped.issued_command_id()?;
        let (admission, owner) = scoped.begin_profile_content_owned(
            lease,
            BeginContentRequest {
                command_id,
                slot,
                expected_revision,
                operation: ContentOperation::Publish(ContentPayloadKind::Images),
                payload_fingerprint: fingerprint,
                expires_at,
            },
        )?;
        let reservation = if bytes.is_empty() {
            None
        } else {
            match scoped.reserve_profile_media(lease, &admission, bytes) {
                Ok(reservation) => Some(reservation),
                Err(error) => {
                    let _ = scoped.fail_profile_content_owned(owner);
                    return Err(error);
                }
            }
        };
        Ok((issued, admission, owner, reservation))
    }
    pub fn web_profile_content_kind(
        &self,
        lease: SessionLease,
        slot: ContentSlotKey,
    ) -> Result<ContentPayloadKind> {
        let tree = match slot.scope {
            LayoutScope::Public => self.read_public_layout(lease)?.layout.tree,
            LayoutScope::Subject(key) => {
                self.read_subject_profile(lease, key)?
                    .ok_or(BackendError::InvalidInput)?
                    .tree
            }
        };
        match tree
            .nodes
            .iter()
            .find(|node| node.id == slot.node_id)
            .map(|node| &node.kind)
        {
            Some(NodeKind::Widget(WidgetKind::SubjectText)) => Ok(ContentPayloadKind::Text),
            Some(NodeKind::Widget(WidgetKind::SubjectImage)) => Ok(ContentPayloadKind::Images),
            _ => Err(BackendError::InvalidInput),
        }
    }
    pub fn publish_web_profile_content(
        &mut self,
        lease: SessionLease,
        slot: ContentSlotKey,
        expected_revision: Revision,
        operation: ContentOperation,
        payload: ContentPayload,
    ) -> Result<ProfileContentDocument> {
        let issued = self.issue_for_web_admin(
            lease,
            fc_ports::command_window::CommandFamily::ProfileContent,
        )?;
        let mut scoped = self.begin_command_scope(issued)?;
        let fingerprint =
            fc_security::sha256(&payload.fingerprint_bytes(slot).map_err(|error| {
                BackendError::ProfileContent(ProfileContentRepositoryError::Domain(error))
            })?);
        let now = scoped.clock.now();
        let seconds = now.second_of_day() + 120;
        let expires_at = fc_domain::time::LocalDateTime::new(
            if seconds < 86400 {
                now.date
            } else {
                next_day(now)?.date
            },
            seconds % 86400,
        )
        .map_err(|_| BackendError::InvalidInput)?;
        let command_id = scoped.issued_command_id()?;
        let (admission, owner) = scoped.begin_profile_content_owned(
            lease,
            BeginContentRequest {
                command_id,
                slot,
                expected_revision,
                operation,
                payload_fingerprint: fingerprint,
                expires_at,
            },
        )?;
        let prepared = match prepare_profile_content(
            &mut Preparation {
                fingerprint,
                operation,
            },
            admission,
            payload,
        ) {
            Ok(value) => value,
            Err(error) => {
                let _ = scoped.fail_profile_content_owned(owner);
                return Err(BackendError::ProfileContent(error));
            }
        };
        if let Err(error) = scoped.commit_profile_content(lease, prepared) {
            let _ = scoped.fail_profile_content_owned(owner);
            return Err(error);
        }
        scoped.read_profile_content(lease, slot)
    }
}
