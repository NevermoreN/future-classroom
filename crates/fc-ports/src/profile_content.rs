//! Per-node content uses the same exact-lease/sole-writer/CAS boundary as daily
//! content. It does not grant teachers layout or publication management rights.
use crate::Clock;
use fc_domain::{
    auth::ActorContext,
    ids::{CommandId, Revision},
    profile_content::{
        BeginContentRequest, ContentAdmission, ContentDigests, ContentOperation, ContentPayload,
        ContentSlotKey, ContentVersion, PageContentRequest, ProfileContentDocument,
        ProfileContentError, PublishedContentSnapshot,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileContentRepositoryError {
    Domain(ProfileContentError),
    Unavailable,
    Faulted,
    IdempotencyConflict,
    DocumentTooLarge,
    QuotaExceeded,
    Cancelled,
    ResultWindowExpired,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileContentStatus {
    Accepted,
    Committed,
    Superseded,
    Failed,
    Cancelled,
    Expired,
    RevisionConflict,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileContentOutcome {
    pub command_id: CommandId,
    pub slot: ContentSlotKey,
    pub sequence: u64,
    pub status: ProfileContentStatus,
    pub committed_revision: Option<Revision>,
}
/// Trusted worker port. Text/TXT are decoded and bounded; image imports are
/// copied/hash-checked, fully WIC-decoded, flushed and sealed at fixed internal
/// versions before this returns. Source handles and paths never enter a DTO.
/// The immutable input must match admission.request.payload_fingerprint; that
/// digest is canonical trusted metadata, not a client-supplied authorization.
pub trait ProfileContentPreparation {
    type Input;
    fn prepare(
        &mut self,
        admission: &ContentAdmission,
        input: Self::Input,
    ) -> Result<ContentPayload, ProfileContentRepositoryError>;
    /// Actual SHA-256, injected because ports/domain have no cryptography dependency.
    fn sha256(&self, bytes: &[u8]) -> Result<[u8; 32], ProfileContentRepositoryError>;
}
/// No public constructor, Deserialize, Clone, or mutable request accessor.
/// Only a trusted preparation port can bind verified media to an admission.
pub struct PreparedProfileContent {
    admission: ContentAdmission,
    payload: ContentPayload,
    digests: ContentDigests,
}
impl PreparedProfileContent {
    pub fn admission(&self) -> &ContentAdmission {
        &self.admission
    }
    pub fn payload(&self) -> &ContentPayload {
        &self.payload
    }
    pub fn digests(&self) -> ContentDigests {
        self.digests
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.payload.allocation_bytes()
    }
    /// Sole-writer trusted composition consumes proof and repeats stored intent,
    /// latest sequence, current layout, exact lease, resource owner/quota and CAS.
    pub fn into_parts(self) -> (ContentAdmission, ContentPayload, ContentDigests) {
        (self.admission, self.payload, self.digests)
    }
}
pub fn prepare_profile_content<P: ProfileContentPreparation>(
    preparation: &mut P,
    admission: ContentAdmission,
    input: P::Input,
) -> Result<PreparedProfileContent, ProfileContentRepositoryError> {
    let payload = preparation.prepare(&admission, input)?;
    payload
        .validate(admission.request.slot)
        .map_err(ProfileContentRepositoryError::Domain)?;
    let valid = match admission.request.operation {
        ContentOperation::Publish(kind) => payload.kind() == Some(kind),
        ContentOperation::Clear | ContentOperation::Undo { .. } => payload == ContentPayload::Empty,
    };
    if !valid {
        return Err(ProfileContentRepositoryError::Domain(
            ProfileContentError::KindMismatch,
        ));
    }
    let canonical = payload
        .fingerprint_bytes(admission.request.slot)
        .map_err(ProfileContentRepositoryError::Domain)?;
    let digests = ContentDigests {
        payload_sha256: preparation.sha256(&canonical)?,
        text_sha256: match &payload {
            ContentPayload::Text(text) => Some(preparation.sha256(text.as_bytes())?),
            _ => None,
        },
    };
    Ok(PreparedProfileContent {
        admission,
        payload,
        digests,
    })
}

pub trait ProfileContentRepository {
    /// Local GlobalAdmin management read; never release pending/history as guest facts.
    fn profile_content(
        &self,
        slot: ContentSlotKey,
        actor: &ActorContext,
    ) -> Result<ProfileContentDocument, ProfileContentRepositoryError>;
    /// Management hydration only, immutable version identity. Current-scene
    /// hydration requires a separate exact published-scene capability in composition.
    fn read_profile_content_version(
        &self,
        slot: ContentSlotKey,
        revision: Revision,
        actor: &ActorContext,
    ) -> Result<Option<ContentVersion>, ProfileContentRepositoryError>;
    /// Persist sequence and command fingerprint before scheduling preparation.
    fn begin_profile_content(
        &mut self,
        request: BeginContentRequest,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<ContentAdmission, ProfileContentRepositoryError>;
    fn commit_profile_content(
        &mut self,
        prepared: PreparedProfileContent,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<PublishedContentSnapshot, ProfileContentRepositoryError>;
    /// Clearing pending intent cannot release a live worker/file quota token.
    /// Internal restart/cancellation reconciliation requires its own trusted path.
    fn fail_profile_content(
        &mut self,
        admission: ContentAdmission,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<ProfileContentOutcome, ProfileContentRepositoryError>;
    /// Read-only page action, not publication authority. Implementation MUST have
    /// an exact current scene generation/source guard, deny by default without it,
    /// and revalidate current node/content/view revision before COMMIT. Never
    /// manufacture an administrator ActorContext for a desktop visitor.
    fn page_profile_content(
        &mut self,
        request: PageContentRequest,
        clock: &dyn Clock,
    ) -> Result<PublishedContentSnapshot, ProfileContentRepositoryError>;
    fn profile_content_outcome(
        &self,
        command_id: CommandId,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<Option<ProfileContentOutcome>, ProfileContentRepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::{
        ids::*,
        layout::LayoutScope,
        profile_content::{ContentPayloadKind, ImageFormat, MediaReference},
        time::{LocalDate, LocalDateTime},
    };
    use std::cell::RefCell;
    struct Preparation {
        payload: Option<ContentPayload>,
        inputs: RefCell<Vec<Vec<u8>>>,
        reject_digest: bool,
    }
    impl ProfileContentPreparation for Preparation {
        type Input = ();
        fn prepare(
            &mut self,
            _: &ContentAdmission,
            _: (),
        ) -> Result<ContentPayload, ProfileContentRepositoryError> {
            self.payload
                .take()
                .ok_or(ProfileContentRepositoryError::Unavailable)
        }
        fn sha256(&self, bytes: &[u8]) -> Result<[u8; 32], ProfileContentRepositoryError> {
            self.inputs.borrow_mut().push(bytes.to_vec());
            if self.reject_digest {
                return Err(ProfileContentRepositoryError::Unavailable);
            }
            // Test-only digest-port substitute. Production must inject SHA-256;
            // this verifies binding/call boundaries, not cryptographic strength.
            Ok([bytes.len() as u8; 32])
        }
    }
    fn admission(kind: ContentPayloadKind) -> ContentAdmission {
        ContentAdmission {
            request: BeginContentRequest {
                command_id: CommandId::new(1).unwrap(),
                slot: ContentSlotKey {
                    scope: LayoutScope::Public,
                    node_id: LayoutNodeId::new(20).unwrap(),
                },
                expected_revision: Revision::default(),
                operation: ContentOperation::Publish(kind),
                payload_fingerprint: [199; 32],
                expires_at: LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 7200)
                    .unwrap(),
            },
            sequence: 1,
            actor_id: PrincipalId::new(1).unwrap(),
            auth_epoch: AuthEpoch::new(1),
        }
    }
    #[test]
    fn prepared_proof_hashes_actual_payload_and_text_instead_of_the_intent_fingerprint() {
        let admission = admission(ContentPayloadKind::Text);
        let text = "\n中文😀\n";
        let payload = ContentPayload::Text(text.into());
        let canonical = payload.fingerprint_bytes(admission.request.slot).unwrap();
        let mut preparation = Preparation {
            payload: Some(payload.clone()),
            inputs: RefCell::new(vec![]),
            reject_digest: false,
        };
        let proof = prepare_profile_content(&mut preparation, admission.clone(), ()).unwrap();
        assert_eq!(
            *preparation.inputs.borrow(),
            vec![canonical.clone(), text.as_bytes().to_vec()]
        );
        assert_eq!(proof.admission(), &admission);
        assert_eq!(proof.payload(), &payload);
        assert_eq!(proof.digests().payload_sha256, [canonical.len() as u8; 32]);
        assert_eq!(proof.digests().text_sha256, Some([text.len() as u8; 32]));
        assert_ne!(
            proof.digests().payload_sha256,
            admission.request.payload_fingerprint
        );
        let mut failed = Preparation {
            payload: Some(payload),
            inputs: RefCell::new(vec![]),
            reject_digest: true,
        };
        assert!(matches!(
            prepare_profile_content(&mut failed, admission, ()),
            Err(ProfileContentRepositoryError::Unavailable)
        ));
    }
    #[test]
    fn wrong_resource_owner_is_rejected_before_any_digest_proof_is_returned() {
        let admission = admission(ContentPayloadKind::Images);
        let mut owner = admission.request.slot;
        owner.node_id = LayoutNodeId::new(21).unwrap();
        let mut preparation = Preparation {
            payload: Some(ContentPayload::Images(vec![MediaReference {
                version_id: MediaVersionId::new(1).unwrap(),
                owner,
                sha256: [8; 32],
                bytes: 100,
                width: 1,
                height: 1,
                format: ImageFormat::Png,
            }])),
            inputs: RefCell::new(vec![]),
            reject_digest: false,
        };
        assert!(matches!(
            prepare_profile_content(&mut preparation, admission, ()),
            Err(ProfileContentRepositoryError::Domain(
                ProfileContentError::InvalidMedia
            ))
        ));
        assert!(preparation.inputs.borrow().is_empty());
    }
}
