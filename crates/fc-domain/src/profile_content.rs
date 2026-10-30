//! Per-node published content. Admission order is durable intent, not completion
//! order; resource preparation and native publication remain outside this model.
use crate::{
    auth::ActorContext,
    desktop::{ComponentKind, ComponentSettings},
    identity::{IdentityError, IdentityState},
    ids::*,
    layout::{LayoutScope, LayoutTree, NodeKind, WidgetKind, MAX_LAYOUT_NODES},
    profile::{LayoutDocument, SubjectProfileDocument},
    time::LocalDateTime,
};

pub const MAX_CONTENT_CHARACTERS: usize = 10_000;
pub const MAX_CONTENT_TEXT_BYTES: usize = 40_000;
pub const MAX_CONTENT_PAGES: usize = 50;
pub const MAX_CONTENT_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_CONTENT_IMAGE_PIXELS: u64 = 8_000_000;
/// Shared primary-original policy, including material originals; namespaces and
/// ownership remain separate. Rebuildable 200 MiB cache is not this allowance.
pub const MAX_CONTENT_MEDIA_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentSlotKey {
    pub scope: LayoutScope,
    pub node_id: LayoutNodeId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Bmp,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaReference {
    pub version_id: MediaVersionId,
    pub owner: ContentSlotKey,
    pub sha256: [u8; 32],
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
}
impl MediaReference {
    pub fn validate(&self) -> Result<(), ProfileContentError> {
        if self.bytes == 0
            || self.bytes > MAX_CONTENT_IMAGE_BYTES
            || self.width == 0
            || self.height == 0
            || u64::from(self.width) * u64::from(self.height) > MAX_CONTENT_IMAGE_PIXELS
        {
            return Err(ProfileContentError::InvalidMedia);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentPayloadKind {
    Text,
    Images,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentPayload {
    Empty,
    /// TXT import uses the same scalar/byte limits; formatting is preserved.
    Text(String),
    /// Ordered pages, including the single-image case. No slideshow timer.
    Images(Vec<MediaReference>),
}
impl ContentPayload {
    /// Stable domain fingerprint bytes, not a database/transport encoding.
    /// The trusted preparation port and writer independently SHA-256 these bytes.
    pub fn fingerprint_bytes(&self, slot: ContentSlotKey) -> Result<Vec<u8>, ProfileContentError> {
        self.validate(slot)?;
        let mut bytes = b"fc-content-payload-v1\0".to_vec();
        match self {
            Self::Empty => bytes.push(0),
            Self::Text(text) => {
                bytes.push(1);
                bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
                bytes.extend_from_slice(text.as_bytes());
            }
            Self::Images(pages) => {
                bytes.extend_from_slice(&[2, pages.len() as u8]);
                for page in pages {
                    bytes.extend_from_slice(&page.version_id.get().to_le_bytes());
                    match page.owner.scope {
                        LayoutScope::Public => bytes.push(0),
                        LayoutScope::Subject(key) => {
                            bytes.push(1);
                            for id in [
                                key.class_id.get(),
                                key.subject_id.get(),
                                key.teacher_id.get(),
                            ] {
                                bytes.extend_from_slice(&id.to_le_bytes());
                            }
                        }
                    }
                    bytes.extend_from_slice(&page.owner.node_id.get().to_le_bytes());
                    bytes.extend_from_slice(&page.sha256);
                    bytes.extend_from_slice(&page.bytes.to_le_bytes());
                    bytes.extend_from_slice(&page.width.to_le_bytes());
                    bytes.extend_from_slice(&page.height.to_le_bytes());
                    bytes.push(match page.format {
                        ImageFormat::Png => 0,
                        ImageFormat::Jpeg => 1,
                        ImageFormat::Bmp => 2,
                    });
                }
            }
        }
        Ok(bytes)
    }
    pub fn validate(&self, slot: ContentSlotKey) -> Result<(), ProfileContentError> {
        match self {
            Self::Empty => Ok(()),
            Self::Text(text) => {
                if text.len() > MAX_CONTENT_TEXT_BYTES
                    || text.chars().count() > MAX_CONTENT_CHARACTERS
                {
                    return Err(ProfileContentError::TextTooLarge);
                }
                if text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                {
                    return Err(ProfileContentError::InvalidText);
                }
                Ok(())
            }
            Self::Images(pages) => {
                if pages.is_empty() || pages.len() > MAX_CONTENT_PAGES {
                    return Err(ProfileContentError::Capacity);
                }
                for (i, page) in pages.iter().enumerate() {
                    page.validate()?;
                    if page.owner != slot
                        || pages[..i]
                            .iter()
                            .any(|old| old.version_id == page.version_id && old != page)
                    {
                        return Err(ProfileContentError::InvalidMedia);
                    }
                }
                Ok(())
            }
        }
    }
    pub fn kind(&self) -> Option<ContentPayloadKind> {
        match self {
            Self::Empty => None,
            Self::Text(_) => Some(ContentPayloadKind::Text),
            Self::Images(_) => Some(ContentPayloadKind::Images),
        }
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        match self {
            Self::Empty => Some(0),
            Self::Text(text) => Some(text.capacity()),
            Self::Images(pages) => pages
                .capacity()
                .checked_mul(std::mem::size_of::<MediaReference>()),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFit {
    Contain,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentNodeStyle {
    pub node_id: LayoutNodeId,
    pub component: ComponentSettings,
    pub image_fit: ImageFit,
}
impl ContentNodeStyle {
    pub fn new(node_id: LayoutNodeId, kind: ContentPayloadKind) -> Self {
        Self {
            node_id,
            component: ComponentSettings {
                kind: match kind {
                    ContentPayloadKind::Text => ComponentKind::SubjectText,
                    ContentPayloadKind::Images => ComponentKind::SubjectImage,
                },
                visible: true,
                opacity_percent: 100,
                font_dip: 22,
                min_width: 160,
                max_width: 800,
                min_height: 64,
                max_height: 1200,
            },
            image_fit: ImageFit::Contain,
        }
    }
    pub fn validate(&self) -> Result<(), ProfileContentError> {
        let c = &self.component;
        if !matches!(
            c.kind,
            ComponentKind::SubjectText | ComponentKind::SubjectImage
        ) || c.opacity_percent > 100
            || !(10..=72).contains(&c.font_dip)
            || !(120..=4096).contains(&c.min_width)
            || c.min_width > c.max_width
            || c.max_width > 4096
            || !(32..=2160).contains(&c.min_height)
            || c.min_height > c.max_height
            || c.max_height > 2160
        {
            return Err(ProfileContentError::InvalidStyle);
        }
        Ok(())
    }
}
/// Exactly one style per content node, none on built-ins/containers. Missing
/// fields in a declared new schema must not silently acquire default styles.
pub fn validate_content_styles(
    tree: &LayoutTree,
    styles: &[ContentNodeStyle],
) -> Result<(), ProfileContentError> {
    if styles.len() > MAX_LAYOUT_NODES {
        return Err(ProfileContentError::Capacity);
    }
    for (i, style) in styles.iter().enumerate() {
        style.validate()?;
        if styles[..i].iter().any(|old| old.node_id == style.node_id) {
            return Err(ProfileContentError::InvalidStyle);
        }
        let expected = match tree.node(style.node_id).map(|node| &node.kind) {
            Some(NodeKind::Widget(WidgetKind::SubjectText)) => ComponentKind::SubjectText,
            Some(NodeKind::Widget(WidgetKind::SubjectImage)) => ComponentKind::SubjectImage,
            _ => return Err(ProfileContentError::InvalidStyle),
        };
        if style.component.kind != expected {
            return Err(ProfileContentError::InvalidStyle);
        }
    }
    let count = tree
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.kind,
                NodeKind::Widget(WidgetKind::SubjectText | WidgetKind::SubjectImage)
            )
        })
        .count();
    if count != styles.len() {
        return Err(ProfileContentError::InvalidStyle);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentOperation {
    Publish(ContentPayloadKind),
    Clear,
    Undo { target_revision: Revision },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeginContentRequest {
    pub command_id: CommandId,
    pub slot: ContentSlotKey,
    pub expected_revision: Revision,
    pub operation: ContentOperation,
    /// Canonical intent fingerprint is computed by the trusted adapter/writer,
    /// including immutable input handles. Never accept a client's hash as proof.
    pub payload_fingerprint: [u8; 32],
    pub expires_at: LocalDateTime,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentAdmission {
    pub request: BeginContentRequest,
    pub sequence: u64,
    pub actor_id: PrincipalId,
    pub auth_epoch: AuthEpoch,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentVersion {
    pub revision: Revision,
    pub payload: ContentPayload,
    pub digests: ContentDigests,
    pub last_actor_id: PrincipalId,
    pub updated_at: LocalDateTime,
}
/// Owned metadata, not a cryptographic proof. Trusted ports and the writer must
/// compute/verify these against actual bytes; domain has no crypto dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentDigests {
    pub payload_sha256: [u8; 32],
    pub text_sha256: Option<[u8; 32]>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileContentDocument {
    pub slot: ContentSlotKey,
    /// Published content CAS. Begin/fail and view-only page changes do not advance it.
    pub revision: Revision,
    pub latest_publish_sequence: u64,
    pub current: Option<ContentVersion>,
    /// One bounded previous committed version; Undo names its exact revision.
    pub previous: Option<ContentVersion>,
    pub view_revision: Revision,
    pub current_page: u8,
    pub pending: Option<ContentAdmission>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageContentRequest {
    pub command_id: CommandId,
    pub slot: ContentSlotKey,
    pub expected_content_revision: Revision,
    pub expected_view_revision: Revision,
    /// Zero-based index into the immutable ordered page manifest.
    pub page: u8,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedContentSnapshot {
    pub slot: ContentSlotKey,
    pub revision: Revision,
    pub view_revision: Revision,
    pub current_page: u8,
    pub content: PublishedContentDescriptor,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishedContentDescriptor {
    Empty,
    Text {
        characters: u32,
        utf8_bytes: u32,
        sha256: [u8; 32],
    },
    Image {
        current: MediaReference,
        total_pages: u8,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileContentError {
    Identity(IdentityError),
    InvalidSlot,
    InvalidState,
    InvalidStyle,
    KindMismatch,
    InvalidText,
    TextTooLarge,
    InvalidMedia,
    Capacity,
    RevisionConflict { current: Revision },
    ViewRevisionConflict { current: Revision },
    Superseded,
    AdmissionMismatch,
    Expired,
    UndoUnavailable,
    VersionOverflow,
}
/// Writer supplies current documents from its transaction, never caller drafts.
pub struct ContentTarget<'a> {
    pub public: &'a LayoutDocument,
    pub subject: Option<&'a SubjectProfileDocument>,
}
impl ContentTarget<'_> {
    pub fn kind(
        &self,
        slot: ContentSlotKey,
        identity: &IdentityState,
    ) -> Result<ContentPayloadKind, ProfileContentError> {
        let tree = match slot.scope {
            LayoutScope::Public => {
                self.public
                    .validate_structure()
                    .map_err(|_| ProfileContentError::InvalidSlot)?;
                &self.public.tree
            }
            LayoutScope::Subject(key) => {
                let profile = self
                    .subject
                    .filter(|p| p.key == key && p.enabled)
                    .ok_or(ProfileContentError::InvalidSlot)?;
                profile
                    .validate_structure()
                    .map_err(|_| ProfileContentError::InvalidSlot)?;
                if !identity.is_teacher(key.teacher_id, key.class_id, key.subject_id) {
                    return Err(ProfileContentError::InvalidSlot);
                }
                &profile.tree
            }
        };
        if tree.scope != slot.scope {
            return Err(ProfileContentError::InvalidSlot);
        }
        match tree.node(slot.node_id).map(|node| &node.kind) {
            Some(NodeKind::Widget(WidgetKind::SubjectText)) => Ok(ContentPayloadKind::Text),
            Some(NodeKind::Widget(WidgetKind::SubjectImage)) => Ok(ContentPayloadKind::Images),
            _ => Err(ProfileContentError::InvalidSlot),
        }
    }
}
fn authorize(
    identity: &IdentityState,
    actor: &ActorContext,
    now: LocalDateTime,
) -> Result<(), ProfileContentError> {
    identity.validate().map_err(ProfileContentError::Identity)?;
    identity
        .authorize_management(actor, now)
        .map_err(ProfileContentError::Identity)
}
fn validate_version(
    version: &ContentVersion,
    slot: ContentSlotKey,
) -> Result<(), ProfileContentError> {
    if version.revision.get() == 0
        || matches!(version.payload, ContentPayload::Text(_))
            != version.digests.text_sha256.is_some()
    {
        return Err(ProfileContentError::InvalidState);
    }
    version.payload.validate(slot)
}
fn validate_page(payload: Option<&ContentPayload>, page: u8) -> Result<(), ProfileContentError> {
    match payload {
        Some(ContentPayload::Images(pages)) if usize::from(page) < pages.len() => Ok(()),
        Some(ContentPayload::Images(_)) => Err(ProfileContentError::InvalidState),
        _ if page == 0 => Ok(()),
        _ => Err(ProfileContentError::InvalidState),
    }
}
impl ProfileContentDocument {
    pub fn new(slot: ContentSlotKey) -> Self {
        Self {
            slot,
            revision: Revision::default(),
            latest_publish_sequence: 0,
            current: None,
            previous: None,
            view_revision: Revision::default(),
            current_page: 0,
            pending: None,
        }
    }
    pub fn validate_structure(&self) -> Result<(), ProfileContentError> {
        match &self.current {
            Some(current) => {
                validate_version(current, self.slot)?;
                if current.revision != self.revision || self.view_revision < self.revision {
                    return Err(ProfileContentError::InvalidState);
                }
            }
            None if self.revision.get() != 0
                || self.previous.is_some()
                || self.view_revision.get() != 0 =>
            {
                return Err(ProfileContentError::InvalidState);
            }
            None => {}
        }
        if let Some(previous) = &self.previous {
            validate_version(previous, self.slot)?;
            if previous.revision.next().ok() != Some(self.revision) {
                return Err(ProfileContentError::InvalidState);
            }
            if self
                .current
                .as_ref()
                .and_then(|v| v.payload.kind())
                .zip(previous.payload.kind())
                .is_some_and(|(current, old)| current != old)
            {
                return Err(ProfileContentError::InvalidState);
            }
        } else if self.revision.get() > 1 {
            return Err(ProfileContentError::InvalidState);
        }
        if self.revision.get() > self.latest_publish_sequence {
            return Err(ProfileContentError::InvalidState);
        }
        if let Some(pending) = &self.pending {
            if pending.request.slot != self.slot
                || pending.sequence == 0
                || pending.sequence != self.latest_publish_sequence
                || pending.request.expected_revision != self.revision
            {
                return Err(ProfileContentError::InvalidState);
            }
        }
        validate_page(self.current.as_ref().map(|v| &v.payload), self.current_page)
    }
    pub fn begin(
        &self,
        request: BeginContentRequest,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
        target: &ContentTarget<'_>,
    ) -> Result<(Self, ContentAdmission), ProfileContentError> {
        self.validate_structure()?;
        authorize(identity, actor, now)?;
        if request.slot != self.slot {
            return Err(ProfileContentError::InvalidSlot);
        }
        let kind = target.kind(self.slot, identity)?;
        if let ContentOperation::Publish(requested) = request.operation {
            if kind != requested {
                return Err(ProfileContentError::KindMismatch);
            }
        }
        if request.expected_revision != self.revision {
            return Err(ProfileContentError::RevisionConflict {
                current: self.revision,
            });
        }
        if now >= request.expires_at || request.expires_at > actor.expires_at {
            return Err(ProfileContentError::Expired);
        }
        if let ContentOperation::Undo { target_revision } = request.operation {
            if self
                .previous
                .as_ref()
                .is_none_or(|v| v.revision != target_revision)
            {
                return Err(ProfileContentError::UndoUnavailable);
            }
        }
        let sequence = self
            .latest_publish_sequence
            .checked_add(1)
            .ok_or(ProfileContentError::VersionOverflow)?;
        let admission = ContentAdmission {
            request,
            sequence,
            actor_id: actor.principal_id,
            auth_epoch: actor.auth_epoch,
        };
        let mut next = self.clone();
        next.latest_publish_sequence = sequence;
        next.pending = Some(admission.clone());
        Ok((next, admission))
    }
    fn check_admission(&self, admission: &ContentAdmission) -> Result<(), ProfileContentError> {
        self.validate_structure()?;
        if admission.sequence != self.latest_publish_sequence {
            return Err(ProfileContentError::Superseded);
        }
        if self.pending.as_ref() != Some(admission) {
            return Err(ProfileContentError::AdmissionMismatch);
        }
        Ok(())
    }
    pub fn finish(
        &self,
        admission: &ContentAdmission,
        prepared: (ContentPayload, ContentDigests),
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
        target: &ContentTarget<'_>,
    ) -> Result<Self, ProfileContentError> {
        self.check_admission(admission)?;
        authorize(identity, actor, now)?;
        if actor.principal_id != admission.actor_id || actor.auth_epoch != admission.auth_epoch {
            return Err(ProfileContentError::AdmissionMismatch);
        }
        if now >= admission.request.expires_at {
            return Err(ProfileContentError::Expired);
        }
        let kind = target.kind(self.slot, identity)?;
        if admission.request.expected_revision != self.revision {
            return Err(ProfileContentError::RevisionConflict {
                current: self.revision,
            });
        }
        let (payload, supplied_digests) = prepared;
        let (payload, digests) = match admission.request.operation {
            ContentOperation::Publish(expected) => {
                if payload.kind() != Some(expected) || expected != kind {
                    return Err(ProfileContentError::KindMismatch);
                }
                (payload, supplied_digests)
            }
            ContentOperation::Clear => {
                if payload != ContentPayload::Empty {
                    return Err(ProfileContentError::KindMismatch);
                }
                (ContentPayload::Empty, supplied_digests)
            }
            ContentOperation::Undo { target_revision } => {
                if payload != ContentPayload::Empty {
                    return Err(ProfileContentError::KindMismatch);
                }
                let previous = self
                    .previous
                    .as_ref()
                    .filter(|v| v.revision == target_revision)
                    .ok_or(ProfileContentError::UndoUnavailable)?;
                (previous.payload.clone(), previous.digests)
            }
        };
        payload.validate(self.slot)?;
        if payload.kind().is_some_and(|actual| actual != kind) {
            return Err(ProfileContentError::KindMismatch);
        }
        let revision = self
            .revision
            .next()
            .map_err(|_| ProfileContentError::VersionOverflow)?;
        let mut next = self.clone();
        next.previous = self.current.clone();
        next.current = Some(ContentVersion {
            revision,
            payload,
            digests,
            last_actor_id: actor.principal_id,
            updated_at: now,
        });
        next.revision = revision;
        next.view_revision = self
            .view_revision
            .next()
            .map_err(|_| ProfileContentError::VersionOverflow)?;
        next.current_page = 0;
        next.pending = None;
        next.validate_structure()?;
        Ok(next)
    }
    /// Trusted cancellation/failure settlement only. Resource quota remains
    /// charged independently until its physical files/worker ownership settle.
    pub fn fail(&self, admission: &ContentAdmission) -> Result<Self, ProfileContentError> {
        self.check_admission(admission)?;
        let mut next = self.clone();
        next.pending = None;
        Ok(next)
    }
    /// Caller must establish an exact current published-scene read capability.
    /// This transition does not grant management or mutate publication intent.
    pub fn page(&self, request: &PageContentRequest) -> Result<Self, ProfileContentError> {
        self.validate_structure()?;
        if request.slot != self.slot {
            return Err(ProfileContentError::InvalidSlot);
        }
        if request.expected_content_revision != self.revision {
            return Err(ProfileContentError::RevisionConflict {
                current: self.revision,
            });
        }
        if request.expected_view_revision != self.view_revision {
            return Err(ProfileContentError::ViewRevisionConflict {
                current: self.view_revision,
            });
        }
        if !matches!(
            self.current.as_ref().map(|v| &v.payload),
            Some(ContentPayload::Images(_))
        ) {
            return Err(ProfileContentError::KindMismatch);
        }
        validate_page(self.current.as_ref().map(|v| &v.payload), request.page)?;
        let mut next = self.clone();
        if request.page != self.current_page {
            next.view_revision = self
                .view_revision
                .next()
                .map_err(|_| ProfileContentError::VersionOverflow)?;
            next.current_page = request.page;
        }
        Ok(next)
    }
    pub fn snapshot(&self) -> Result<PublishedContentSnapshot, ProfileContentError> {
        self.validate_structure()?;
        let content = match self.current.as_ref() {
            None => PublishedContentDescriptor::Empty,
            Some(version) => match &version.payload {
                ContentPayload::Empty => PublishedContentDescriptor::Empty,
                ContentPayload::Text(text) => PublishedContentDescriptor::Text {
                    characters: text.chars().count() as u32,
                    utf8_bytes: text.len() as u32,
                    sha256: version
                        .digests
                        .text_sha256
                        .ok_or(ProfileContentError::InvalidState)?,
                },
                ContentPayload::Images(pages) => PublishedContentDescriptor::Image {
                    current: pages
                        .get(usize::from(self.current_page))
                        .ok_or(ProfileContentError::InvalidState)?
                        .clone(),
                    total_pages: pages.len() as u8,
                },
            },
        };
        Ok(PublishedContentSnapshot {
            slot: self.slot,
            revision: self.revision,
            view_revision: self.view_revision,
            current_page: self.current_page,
            content,
        })
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        let current = self
            .current
            .as_ref()
            .map_or(Some(0), |v| v.payload.allocation_bytes())?;
        current.checked_add(
            self.previous
                .as_ref()
                .map_or(Some(0), |v| v.payload.allocation_bytes())?,
        )
    }
}
impl PublishedContentSnapshot {
    pub fn validate(&self) -> Result<(), ProfileContentError> {
        if (self.revision.get() == 0) != (self.view_revision.get() == 0)
            || self.view_revision < self.revision
        {
            return Err(ProfileContentError::InvalidState);
        }
        match &self.content {
            PublishedContentDescriptor::Empty if self.current_page == 0 => Ok(()),
            PublishedContentDescriptor::Text {
                characters,
                utf8_bytes,
                ..
            } if self.revision.get() != 0
                && self.current_page == 0
                && *characters <= MAX_CONTENT_CHARACTERS as u32
                && *utf8_bytes <= MAX_CONTENT_TEXT_BYTES as u32
                && *utf8_bytes >= *characters
                && u64::from(*utf8_bytes) <= u64::from(*characters) * 4 =>
            {
                Ok(())
            }
            PublishedContentDescriptor::Image {
                current,
                total_pages,
            } if self.revision.get() != 0
                && (1..=MAX_CONTENT_PAGES as u8).contains(total_pages)
                && self.current_page < *total_pages
                && current.owner == self.slot =>
            {
                current.validate()
            }
            _ => Err(ProfileContentError::InvalidState),
        }
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::TrustedOrigin, identity::IdentityCommand, layout::LayoutNode, time::LocalDate,
    };
    fn fixture(
        kind: ContentPayloadKind,
    ) -> (
        IdentityState,
        ActorContext,
        LocalDateTime,
        LayoutDocument,
        ContentSlotKey,
    ) {
        let now = LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 3600).unwrap();
        let identity = IdentityState::default()
            .transition(
                &IdentityCommand::FirstAdministrator {
                    school_id: SchoolId::new(1).unwrap(),
                    school_name: "合成学校".into(),
                    id: PrincipalId::new(1).unwrap(),
                    login_name: "admin".into(),
                    display_name: "管理".into(),
                },
                None,
                now,
            )
            .unwrap()
            .next;
        let actor = ActorContext {
            principal_id: PrincipalId::new(1).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            auth_epoch: AuthEpoch::new(1),
            expires_at: LocalDateTime::new(now.date, 7200).unwrap(),
            origin: TrustedOrigin::Local,
        };
        let mut public = LayoutDocument::default();
        // Fresh ID avoids converting the existing Materials slot's identity.
        let id = public.allocate_id().unwrap();
        let outlet = public
            .tree
            .nodes
            .iter_mut()
            .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
            .unwrap();
        let previous = outlet.children.remove(0);
        outlet.children.push(id);
        public.tree.nodes.retain(|node| node.id != previous);
        public.tree.nodes.push(LayoutNode {
            id,
            kind: NodeKind::Widget(match kind {
                ContentPayloadKind::Text => WidgetKind::SubjectText,
                ContentPayloadKind::Images => WidgetKind::SubjectImage,
            }),
            children: vec![],
        });
        public.content_styles.push(ContentNodeStyle::new(id, kind));
        public.validate_structure().unwrap();
        (
            identity,
            actor,
            now,
            public,
            ContentSlotKey {
                scope: LayoutScope::Public,
                node_id: id,
            },
        )
    }
    fn request(
        slot: ContentSlotKey,
        id: u64,
        expected: u64,
        op: ContentOperation,
        actor: &ActorContext,
    ) -> BeginContentRequest {
        BeginContentRequest {
            command_id: CommandId::new(id).unwrap(),
            slot,
            expected_revision: Revision::new(expected),
            operation: op,
            payload_fingerprint: [id as u8; 32],
            expires_at: actor.expires_at,
        }
    }
    // Pure-rule fixture digests, explicitly not a cryptographic implementation.
    fn prepared(payload: ContentPayload) -> (ContentPayload, ContentDigests) {
        let text_sha256 = matches!(payload, ContentPayload::Text(_)).then_some([8; 32]);
        (
            payload,
            ContentDigests {
                payload_sha256: [7; 32],
                text_sha256,
            },
        )
    }
    fn image(slot: ContentSlotKey, id: u64) -> MediaReference {
        MediaReference {
            version_id: MediaVersionId::new(id).unwrap(),
            owner: slot,
            sha256: [4; 32],
            bytes: MAX_CONTENT_IMAGE_BYTES,
            width: 4000,
            height: 2000,
            format: ImageFormat::Png,
        }
    }
    #[test]
    fn later_failed_intent_does_not_revive_earlier_preparation_or_replace_current() {
        let (identity, actor, now, public, slot) = fixture(ContentPayloadKind::Text);
        let target = ContentTarget {
            public: &public,
            subject: None,
        };
        let doc = ProfileContentDocument::new(slot);
        let (doc, first) = doc
            .begin(
                request(
                    slot,
                    1,
                    0,
                    ContentOperation::Publish(ContentPayloadKind::Text),
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let doc = doc
            .finish(
                &first,
                prepared(ContentPayload::Text("已提交".into())),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let (doc, slow) = doc
            .begin(
                request(
                    slot,
                    2,
                    1,
                    ContentOperation::Publish(ContentPayloadKind::Text),
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let (doc, newer) = doc
            .begin(
                request(
                    slot,
                    3,
                    1,
                    ContentOperation::Publish(ContentPayloadKind::Text),
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let doc = doc.fail(&newer).unwrap();
        assert_eq!(doc.latest_publish_sequence, 3);
        assert_eq!(doc.revision, Revision::new(1));
        assert_eq!(
            doc.current.as_ref().unwrap().payload,
            ContentPayload::Text("已提交".into())
        );
        assert_eq!(
            doc.finish(
                &slow,
                prepared(ContentPayload::Text("旧图迟到".into())),
                &identity,
                &actor,
                now,
                &target
            ),
            Err(ProfileContentError::Superseded)
        );
    }
    #[test]
    fn clear_and_revision_targeted_undo_create_new_versions_and_bound_history() {
        let (identity, actor, now, public, slot) = fixture(ContentPayloadKind::Text);
        let target = ContentTarget {
            public: &public,
            subject: None,
        };
        let (doc, admission) = ProfileContentDocument::new(slot)
            .begin(
                request(
                    slot,
                    1,
                    0,
                    ContentOperation::Publish(ContentPayloadKind::Text),
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let doc = doc
            .finish(
                &admission,
                prepared(ContentPayload::Text("第一版".into())),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let (doc, clear) = doc
            .begin(
                request(slot, 2, 1, ContentOperation::Clear, &actor),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let doc = doc
            .finish(
                &clear,
                prepared(ContentPayload::Empty),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        assert_eq!(doc.current.as_ref().unwrap().payload, ContentPayload::Empty);
        assert_eq!(doc.previous.as_ref().unwrap().revision, Revision::new(1));
        assert_eq!(
            doc.begin(
                request(
                    slot,
                    3,
                    1,
                    ContentOperation::Undo {
                        target_revision: Revision::new(1)
                    },
                    &actor
                ),
                &identity,
                &actor,
                now,
                &target
            ),
            Err(ProfileContentError::RevisionConflict {
                current: Revision::new(2)
            })
        );
        let (doc, undo) = doc
            .begin(
                request(
                    slot,
                    3,
                    2,
                    ContentOperation::Undo {
                        target_revision: Revision::new(1),
                    },
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let doc = doc
            .finish(
                &undo,
                prepared(ContentPayload::Empty),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        assert_eq!(
            doc.current.as_ref().unwrap().payload,
            ContentPayload::Text("第一版".into())
        );
        assert_eq!(doc.revision, Revision::new(3));
        assert_eq!(doc.previous.as_ref().unwrap().revision, Revision::new(2));
    }
    #[test]
    fn page_is_separate_cas_and_does_not_supersede_pending_publish() {
        let (identity, actor, now, public, slot) = fixture(ContentPayloadKind::Images);
        let target = ContentTarget {
            public: &public,
            subject: None,
        };
        let (doc, admission) = ProfileContentDocument::new(slot)
            .begin(
                request(
                    slot,
                    1,
                    0,
                    ContentOperation::Publish(ContentPayloadKind::Images),
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let doc = doc
            .finish(
                &admission,
                prepared(ContentPayload::Images(vec![image(slot, 1), image(slot, 2)])),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let (doc, pending) = doc
            .begin(
                request(
                    slot,
                    2,
                    1,
                    ContentOperation::Publish(ContentPayloadKind::Images),
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let page = PageContentRequest {
            command_id: CommandId::new(3).unwrap(),
            slot,
            expected_content_revision: doc.revision,
            expected_view_revision: doc.view_revision,
            page: 1,
        };
        let doc = doc.page(&page).unwrap();
        assert_eq!(doc.revision, Revision::new(1));
        assert_eq!(doc.latest_publish_sequence, 2);
        assert_eq!(doc.pending, Some(pending.clone()));
        assert_eq!(doc.current_page, 1);
        assert!(matches!(
            doc.page(&page),
            Err(ProfileContentError::ViewRevisionConflict { .. })
        ));
        let doc = doc
            .finish(
                &pending,
                prepared(ContentPayload::Images(vec![image(slot, 3)])),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        assert_eq!(doc.current_page, 0);
        assert!(matches!(
            doc.page(&page),
            Err(ProfileContentError::RevisionConflict { .. })
        ));
        assert!(matches!(
            doc.snapshot().unwrap().content,
            PublishedContentDescriptor::Image { total_pages: 1, .. }
        ));
    }
    #[test]
    fn image_limits_order_fifty_pages_and_exact_owner_are_checked_without_pixels() {
        let (_, _, _, _, slot) = fixture(ContentPayloadKind::Images);
        let pages: Vec<_> = (1..=50).map(|id| image(slot, id)).collect();
        ContentPayload::Images(pages.clone())
            .validate(slot)
            .unwrap();
        let mut oversized = pages.clone();
        oversized.push(image(slot, 51));
        assert_eq!(
            ContentPayload::Images(oversized).validate(slot),
            Err(ProfileContentError::Capacity)
        );
        let mut wrong_owner = pages.clone();
        wrong_owner[0].owner.node_id = LayoutNodeId::new(99).unwrap();
        assert_eq!(
            ContentPayload::Images(wrong_owner).validate(slot),
            Err(ProfileContentError::InvalidMedia)
        );
        let mut too_many_pixels = image(slot, 1);
        too_many_pixels.width += 1;
        assert_eq!(
            too_many_pixels.validate(),
            Err(ProfileContentError::InvalidMedia)
        );
        let mut too_many_bytes = image(slot, 1);
        too_many_bytes.bytes += 1;
        assert_eq!(
            too_many_bytes.validate(),
            Err(ProfileContentError::InvalidMedia)
        );
        let mut reversed = pages.clone();
        reversed.reverse();
        assert_ne!(
            ContentPayload::Images(pages)
                .fingerprint_bytes(slot)
                .unwrap(),
            ContentPayload::Images(reversed)
                .fingerprint_bytes(slot)
                .unwrap()
        );
    }
    #[test]
    fn text_counts_unicode_scalars_preserves_formatting_and_never_truncates() {
        let (identity, actor, now, public, slot) = fixture(ContentPayloadKind::Text);
        let text = "😀".repeat(10_000);
        ContentPayload::Text(text.clone()).validate(slot).unwrap();
        assert_eq!(
            ContentPayload::Text(format!("{text}x")).validate(slot),
            Err(ProfileContentError::TextTooLarge)
        );
        assert_eq!(
            ContentPayload::Text("bad\0text".into()).validate(slot),
            Err(ProfileContentError::InvalidText)
        );
        let formatted = "\n  保留缩进\t与换行\r\n";
        let target = ContentTarget {
            public: &public,
            subject: None,
        };
        let (doc, admission) = ProfileContentDocument::new(slot)
            .begin(
                request(
                    slot,
                    1,
                    0,
                    ContentOperation::Publish(ContentPayloadKind::Text),
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let doc = doc
            .finish(
                &admission,
                prepared(ContentPayload::Text(formatted.into())),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        assert_eq!(
            doc.current.as_ref().unwrap().payload,
            ContentPayload::Text(formatted.into())
        );
        assert!(
            matches!(doc.snapshot().unwrap().content, PublishedContentDescriptor::Text { characters, utf8_bytes, sha256 } if sha256 == [8; 32] && characters == formatted.chars().count() as u32 && utf8_bytes == formatted.len() as u32)
        );
    }
    #[test]
    fn fresh_local_global_and_current_node_are_required_at_both_stages() {
        let (identity, actor, now, public, slot) = fixture(ContentPayloadKind::Text);
        let target = ContentTarget {
            public: &public,
            subject: None,
        };
        let doc = ProfileContentDocument::new(slot);
        let mut web = actor.clone();
        web.origin = TrustedOrigin::Web;
        assert!(matches!(
            doc.begin(
                request(
                    slot,
                    1,
                    0,
                    ContentOperation::Publish(ContentPayloadKind::Text),
                    &actor
                ),
                &identity,
                &web,
                now,
                &target
            ),
            Err(ProfileContentError::Identity(_))
        ));
        let (doc, admission) = doc
            .begin(
                request(
                    slot,
                    1,
                    0,
                    ContentOperation::Publish(ContentPayloadKind::Text),
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        assert!(matches!(
            doc.finish(
                &admission,
                prepared(ContentPayload::Text("x".into())),
                &identity,
                &web,
                now,
                &target
            ),
            Err(ProfileContentError::Identity(_))
        ));
        let mut removed = public.clone();
        removed.reset_tree().unwrap();
        assert_eq!(
            doc.finish(
                &admission,
                prepared(ContentPayload::Text("x".into())),
                &identity,
                &actor,
                now,
                &ContentTarget {
                    public: &removed,
                    subject: None
                }
            ),
            Err(ProfileContentError::InvalidSlot)
        );
        assert_eq!(
            doc.finish(
                &admission,
                prepared(ContentPayload::Text("x".into())),
                &identity,
                &actor,
                actor.expires_at,
                &target
            ),
            Err(ProfileContentError::Identity(IdentityError::Permission(
                crate::auth::PermissionError::Expired
            )))
        );
    }
    #[test]
    fn independent_same_kind_styles_and_identity_cannot_be_repurposed() {
        let (identity, actor, now, mut public, _) = fixture(ContentPayloadKind::Text);
        let second = public.allocate_id().unwrap();
        public
            .tree
            .nodes
            .iter_mut()
            .find(|node| matches!(node.kind, NodeKind::Outlet { .. }))
            .unwrap()
            .children
            .push(second);
        public.tree.nodes.push(LayoutNode {
            id: second,
            kind: NodeKind::Widget(WidgetKind::SubjectText),
            children: vec![],
        });
        let mut style = ContentNodeStyle::new(second, ContentPayloadKind::Text);
        style.component.opacity_percent = 0;
        public.content_styles.push(style);
        // Default public uses all eight glass slots; remove recess for the added content card.
        let recess = public
            .tree
            .nodes
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Widget(WidgetKind::Recess)))
            .unwrap()
            .id;
        public.tree.nodes.retain(|node| node.id != recess);
        for node in &mut public.tree.nodes {
            node.children.retain(|id| *id != recess);
        }
        public.validate_structure().unwrap();
        assert_eq!(public.content_styles[0].component.opacity_percent, 100);
        assert_eq!(public.content_styles[1].component.opacity_percent, 0);
        let mut candidate = public.clone();
        candidate
            .tree
            .nodes
            .iter_mut()
            .find(|n| n.id == second)
            .unwrap()
            .kind = NodeKind::Widget(WidgetKind::SubjectImage);
        candidate.content_styles[1] = ContentNodeStyle::new(second, ContentPayloadKind::Images);
        assert!(public
            .transition(candidate, public.revision, &identity, &actor, now)
            .is_err());
        let mut bad = public.clone();
        bad.content_styles.pop();
        assert!(bad.validate_structure().is_err());
    }
    #[test]
    fn exhausted_sequence_and_corrupt_published_metadata_fail_without_wrapping() {
        let (identity, actor, now, public, slot) = fixture(ContentPayloadKind::Text);
        let target = ContentTarget {
            public: &public,
            subject: None,
        };
        let mut exhausted = ProfileContentDocument::new(slot);
        exhausted.latest_publish_sequence = u64::MAX;
        assert_eq!(
            exhausted.begin(
                request(
                    slot,
                    1,
                    0,
                    ContentOperation::Publish(ContentPayloadKind::Text),
                    &actor
                ),
                &identity,
                &actor,
                now,
                &target
            ),
            Err(ProfileContentError::VersionOverflow)
        );
        let mut descriptor = PublishedContentSnapshot {
            slot,
            revision: Revision::new(1),
            view_revision: Revision::new(1),
            current_page: 0,
            content: PublishedContentDescriptor::Text {
                characters: 1,
                utf8_bytes: 5,
                sha256: [1; 32],
            },
        };
        assert_eq!(
            descriptor.validate(),
            Err(ProfileContentError::InvalidState)
        );
        descriptor.content = PublishedContentDescriptor::Image {
            current: image(slot, 1),
            total_pages: 51,
        };
        assert_eq!(
            descriptor.validate(),
            Err(ProfileContentError::InvalidState)
        );
        let (doc, admission) = ProfileContentDocument::new(slot)
            .begin(
                request(
                    slot,
                    1,
                    0,
                    ContentOperation::Publish(ContentPayloadKind::Text),
                    &actor,
                ),
                &identity,
                &actor,
                now,
                &target,
            )
            .unwrap();
        let mut stale = identity.clone();
        stale.accounts[0].principal.auth_epoch = AuthEpoch::new(2);
        assert!(matches!(
            doc.finish(
                &admission,
                prepared(ContentPayload::Text("x".into())),
                &stale,
                &actor,
                now,
                &target
            ),
            Err(ProfileContentError::Identity(_))
        ));
    }
}
