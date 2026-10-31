//! Exact local-global content management and guest page capabilities. All SQL
//! and body hydration execute on the existing application workers, never the UI.
use super::*;
use crate::profile_content_cache::{ProfileContentTextCache, TextFrame};
use fc_domain::profile_content::*;
use fc_ports::{ProfileContentOutcome, ProfileContentRepository, ProfileContentRepositoryError};
pub use fc_storage::business::profile_content::{
    MediaEntry, MediaInventory, MediaReservation, MediaRetirement, MediaState,
};

fn check_content_execution(fence: &dyn Fn() -> bool) -> std::result::Result<(), StorageError> {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(fence)).unwrap_or(false) {
        Ok(())
    } else {
        // Task deadline/cancel/stop/fault is not an unknown authority publication.
        Err(StorageError::ProfileContent(ProfileContentError::Expired))
    }
}
fn content_execution_expired(_: StorageError) -> BackendError {
    BackendError::ProfileContent(ProfileContentRepositoryError::Domain(
        ProfileContentError::Expired,
    ))
}

/// An admitted preparation may pause through an unrelated writer publication.
/// Updating is never permission to begin another file read or decoder call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileContentPreparationState {
    Ready,
    AuthorityUpdating,
    Revoked,
}

#[derive(Clone)]
pub struct ProfileContentPreparationGuard {
    check: Arc<dyn Fn() -> ProfileContentPreparationState + Send + Sync>,
}
impl ProfileContentPreparationGuard {
    /// Memory-only and non-renewing. Call before each bounded IO/decode step;
    /// AuthorityUpdating requires waiting with cancellation/deadline checks.
    /// This is not a publication, file-open dispatch, or COMMIT authorization.
    pub fn state(&self) -> ProfileContentPreparationState {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.check)()))
            .unwrap_or(ProfileContentPreparationState::Revoked)
    }
    /// Liveness only: a transient update does not permanently cancel this job.
    /// Callers must additionally observe Ready before doing sensitive work.
    pub fn is_context_valid(&self) -> bool {
        self.state() != ProfileContentPreparationState::Revoked
    }
    pub fn is_ready(&self) -> bool {
        self.state() == ProfileContentPreparationState::Ready
    }
}

#[derive(Default)]
pub(super) struct PublishedProfileContentScene {
    high_water: u64,
    current: Option<ContentScene>,
}
struct ContentScene {
    generation: SceneGeneration,
    revision: Revision,
    local_generation: u64,
    captured_at: LocalDateTime,
    valid_until: Option<LocalDateTime>,
    entries: Vec<PublishedContentSnapshot>,
}
impl SessionControl {
    /// Call only after the renderer actually accepted this exact scene. This
    /// memory-only publication creates no ActorContext or management permission.
    pub fn publish_profile_content_scene(
        &self,
        entries: &[PublishedContentSnapshot],
        generation: SceneGeneration,
        revision: Revision,
        captured_at: LocalDateTime,
        valid_until: Option<LocalDateTime>,
    ) -> bool {
        let status = self.0.health.status();
        if (self.0.closed.load(Ordering::Acquire) || self.is_recovering())
            || status.health != fc_runtime::health::Health::Healthy
            || status.revision != revision
            || generation.get() == 0
            || entries.len() > fc_domain::layout::MAX_VISIBLE_LEAVES
            || valid_until.is_some_and(|end| end <= captured_at)
            || entries.iter().enumerate().any(|(i, entry)| {
                entry.validate().is_err()
                    || entry.revision > revision
                    || entry.view_revision > revision
                    || entries[..i].iter().any(|old| old.slot == entry.slot)
            })
        {
            return false;
        }
        let Ok(mut state) = self.0.profile_content_scene.lock() else {
            self.0.health.fault();
            return false;
        };
        let status = self.0.health.status();
        if (self.0.closed.load(Ordering::Acquire) || self.is_recovering())
            || status.health != fc_runtime::health::Health::Healthy
            || status.revision != revision
            || generation.get() < state.high_water
            || generation.get() == state.high_water && state.current.is_none()
        {
            return false;
        }
        state.high_water = generation.get();
        state.current = Some(ContentScene {
            generation,
            revision,
            local_generation: self.generation(),
            captured_at,
            valid_until,
            entries: entries.to_vec(),
        });
        true
    }
    pub fn clear_profile_content_scene(&self) {
        match self.0.profile_content_scene.lock() {
            Ok(mut state) => state.current = None,
            Err(poisoned) => {
                poisoned.into_inner().current = None;
                self.0.health.fault();
            }
        }
        if ProfileContentTextCache::shared().clear().is_err() {
            self.0.health.fault();
        }
    }
    fn profile_content_scene_matches(
        &self,
        expected: (
            ContentSlotKey,
            Revision,
            Revision,
            Revision,
            SceneGeneration,
        ),
        now: LocalDateTime,
        allow_updating: bool,
    ) -> bool {
        let (slot, content, view, global, generation) = expected;
        let healthy = |status: fc_runtime::health::HealthStatus| {
            status.revision == global
                && (status.health == fc_runtime::health::Health::Healthy
                    || allow_updating
                        && status.health == fc_runtime::health::Health::AuthorityUpdating)
        };
        if (self.0.closed.load(Ordering::Acquire) || self.is_recovering())
            || !healthy(self.0.health.status())
        {
            return false;
        }
        let Ok(state) = self.0.profile_content_scene.lock() else {
            return false;
        };
        let matches = state.current.as_ref().is_some_and(|scene| {
            scene.generation == generation
                && scene.revision == global
                && scene.local_generation == self.generation()
                && now.date == scene.captured_at.date
                && now >= scene.captured_at
                && scene.valid_until.is_none_or(|end| now < end)
                && scene.entries.iter().any(|entry| {
                    entry.slot == slot && entry.revision == content && entry.view_revision == view
                })
        });
        matches
            && !(self.0.closed.load(Ordering::Acquire) || self.is_recovering())
            && healthy(self.0.health.status())
    }
}
fn unavailable() -> BackendError {
    BackendError::ProfileContent(ProfileContentRepositoryError::Unavailable)
}
impl Backend {
    /// Local-global preparation admission. Every later check uses the current
    /// in-memory authority, not a captured DB revision. No Backend/SQLite access
    /// is needed while copying or decoding; no lock is held across either.
    pub fn profile_content_preparation_guard(
        &self,
        lease: SessionLease,
    ) -> Result<ProfileContentPreparationGuard> {
        self.check_open()?;
        let initial = self
            .authority
            .snapshot()
            .map_err(|_| BackendError::InvalidCredentials)?;
        let (actor, generation) = self.actor(&initial.data, lease, SessionPurpose::Manage)?;
        initial
            .data
            .authorize_management(&actor, self.clock.now())
            .map_err(|_| BackendError::InvalidCredentials)?;
        let authority = self.authority.clone();
        let ledger = self.ledger.clone();
        let control = self.control.clone();
        let clock = self.clock.clone();
        let monotonic = self.monotonic.clone();
        let revoked = AtomicBool::new(false);
        let check = Arc::new(move || {
            use fc_runtime::health::{GateError, Health};
            use ProfileContentPreparationState::{AuthorityUpdating, Ready, Revoked};
            if revoked.load(Ordering::Acquire) || control.check(lease, generation).is_err() {
                revoked.store(true, Ordering::Release);
                return Revoked;
            }
            let snapshot = match authority.snapshot() {
                Ok(snapshot) => snapshot,
                Err(GateError::Updating) => return AuthorityUpdating,
                Err(_) => {
                    revoked.store(true, Ordering::Release);
                    return Revoked;
                }
            };
            let authorized = {
                let Ok(mut ledger) = ledger.lock() else {
                    revoked.store(true, Ordering::Release);
                    return Revoked;
                };
                let now = clock.now();
                let tick = monotonic.monotonic_millis();
                ledger.synchronize(&control, &snapshot.data, now, tick);
                ledger
                    .sessions
                    .actor(&snapshot.data, lease, now, tick, SessionPurpose::Manage)
                    .is_ok_and(|actor| snapshot.data.authorize_management(&actor, now).is_ok())
            };
            if !authorized || control.check(lease, generation).is_err() {
                revoked.store(true, Ordering::Release);
                return Revoked;
            }
            let status = authority.gate().status();
            match status.health {
                Health::Healthy if status.revision == snapshot.revision => Ready,
                // A publication raced the memory checks. Retry a fresh snapshot
                // before any next IO; never authorize with its predecessor.
                Health::Healthy | Health::AuthorityUpdating => AuthorityUpdating,
                _ => {
                    revoked.store(true, Ordering::Release);
                    Revoked
                }
            }
        });
        Ok(ProfileContentPreparationGuard { check })
    }

    fn profile_content_actor(&self, lease: SessionLease) -> Result<(ActorContext, u64)> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        state
            .authorize_management(&actor, self.clock.now())
            .map_err(|error| {
                BackendError::ProfileContent(ProfileContentRepositoryError::Domain(
                    ProfileContentError::Identity(error),
                ))
            })?;
        Ok((actor, generation))
    }
    pub fn read_profile_content(
        &self,
        lease: SessionLease,
        slot: ContentSlotKey,
    ) -> Result<ProfileContentDocument> {
        let (actor, generation) = self.profile_content_actor(lease)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .profile_content(slot, &actor)?)
    }
    pub fn read_profile_content_version(
        &self,
        lease: SessionLease,
        slot: ContentSlotKey,
        revision: Revision,
    ) -> Result<Option<ContentVersion>> {
        let (actor, generation) = self.profile_content_actor(lease)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .read_profile_content_version(slot, revision, &actor)?)
    }
    pub fn begin_profile_content(
        &mut self,
        lease: SessionLease,
        request: BeginContentRequest,
    ) -> Result<ContentAdmission> {
        let (actor, generation) = self.profile_content_actor(lease)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .begin_profile_content(request, &actor, self.clock.as_ref())?)
    }
    pub fn commit_profile_content(
        &mut self,
        lease: SessionLease,
        prepared: fc_ports::PreparedProfileContent,
    ) -> Result<PublishedContentSnapshot> {
        let (actor, generation) = self.profile_content_actor(lease)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .commit_profile_content(prepared, &actor, self.clock.as_ref())?)
    }
    /// Both evaluation and the real writer's precommit repeat this memory-only
    /// fence alongside (never instead of) the exact authenticated session guard.
    /// Cancellation after COMMIT does not undo it: reopen/outcome must reconcile.
    pub fn commit_profile_content_while(
        &mut self,
        lease: SessionLease,
        prepared: fc_ports::PreparedProfileContent,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<PublishedContentSnapshot> {
        check_content_execution(fence.as_ref()).map_err(content_execution_expired)?;
        let (actor, generation) = self.profile_content_actor(lease)?;
        let session = self.session_guard(lease, generation, SessionPurpose::Manage);
        let guard = Arc::new(move |state: &IdentityState, actor: &ActorContext| {
            check_content_execution(fence.as_ref())?;
            session(state, actor)?;
            check_content_execution(fence.as_ref())
        });
        Ok(self
            .repository
            .clone()
            .with_session_guard(guard)
            .commit_profile_content(prepared, &actor, self.clock.as_ref())?)
    }
    pub fn fail_profile_content(
        &mut self,
        lease: SessionLease,
        admission: ContentAdmission,
    ) -> Result<ProfileContentOutcome> {
        let (actor, generation) = self.profile_content_actor(lease)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .fail_profile_content(admission, &actor, self.clock.as_ref())?)
    }
    pub fn profile_content_outcome(
        &self,
        lease: SessionLease,
        command: CommandId,
    ) -> Result<Option<ProfileContentOutcome>> {
        let (actor, generation) = self.profile_content_actor(lease)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .profile_content_outcome(command, &actor, self.clock.as_ref())?)
    }
    /// Full bodies are fetched by immutable identity on the writer, each under
    /// the exact facts global revision and currently selected node. No guest
    /// administrator identity is fabricated and no previous version is read.
    pub fn hydrate_profile_content(&self, facts: &DesktopFacts) -> Result<TextFrame> {
        self.check_open()?;
        facts.validate().map_err(|_| unavailable())?;
        if self.control.health_gate().status().revision != facts.global_revision {
            return Err(unavailable());
        }
        let mut frame = ProfileContentTextCache::shared().frame();
        for snapshot in &facts.profile_contents {
            if matches!(snapshot.content, PublishedContentDescriptor::Text { .. }) {
                let version = self
                    .repository
                    .read_current_profile_content_version(
                        snapshot.slot,
                        snapshot.revision,
                        facts.global_revision,
                    )?
                    .ok_or_else(unavailable)?;
                let canonical = version
                    .payload
                    .fingerprint_bytes(snapshot.slot)
                    .map_err(|_| unavailable())?;
                if version.revision != snapshot.revision
                    || fc_security::sha256(&canonical) != version.digests.payload_sha256
                {
                    return Err(unavailable());
                }
                let ContentPayload::Text(text) = version.payload else {
                    return Err(unavailable());
                };
                if version.digests.text_sha256 != Some(fc_security::sha256(text.as_bytes())) {
                    return Err(unavailable());
                }
                frame
                    .insert_verified(snapshot, text)
                    .map_err(|_| unavailable())?;
            }
        }
        self.check_open()?;
        let now = self.clock.now();
        if self.control.health_gate().status().revision != facts.global_revision
            || now < facts.captured_at
            || now.date != facts.captured_at.date
            || !frame.complete_for(&facts.profile_contents)
        {
            return Err(unavailable());
        }
        if facts.lesson_context.blocked_until.is_none() {
            let observed = facts
                .lesson_context
                .observe(&facts.identity, &facts.schedule, now)
                .map_err(|_| unavailable())?;
            if observed.current != facts.lesson_context.current
                || observed.active_override != facts.lesson_context.active_override
                || observed.history != facts.lesson_context.history
                || observed.blocked_until != facts.lesson_context.blocked_until
            {
                return Err(unavailable());
            }
        }
        Ok(frame)
    }
    /// Trusted image preparation fence, before and after filesystem/WIC work.
    /// It precedes scene publication and therefore requires no already-published
    /// generation and never fabricates a management actor. The sole writer
    /// re-resolves actual selection once for the whole bounded descriptor batch.
    pub fn verify_profile_content_images(
        &self,
        expected_global_revision: Revision,
        captured_at: LocalDateTime,
        valid_until: Option<LocalDateTime>,
        requests: &[(ContentSlotKey, Revision, Revision, MediaReference)],
    ) -> Result<()> {
        self.check_open()?;
        let in_window = |now: LocalDateTime| {
            now.date == captured_at.date
                && now >= captured_at
                && valid_until.is_none_or(|until| now < until)
        };
        if requests.len() > fc_domain::layout::MAX_VISIBLE_LEAVES
            || !in_window(self.clock.now())
            || self.control.health_gate().status().revision != expected_global_revision
        {
            return Err(unavailable());
        }
        let facts = self.desktop_facts(0)?;
        if facts.global_revision != expected_global_revision
            || facts.clock_rollback
            || facts.captured_at < captured_at
            || !in_window(facts.captured_at)
        {
            return Err(unavailable());
        }
        for (i, (slot, content, view, resource)) in requests.iter().enumerate() {
            if requests[..i].iter().any(|old| old.0 == *slot)
                || resource.validate().is_err() || resource.owner != *slot
                || !facts.profile_contents.iter().any(|snapshot| {
                    snapshot.slot == *slot && snapshot.revision == *content && snapshot.view_revision == *view
                        && matches!(&snapshot.content, PublishedContentDescriptor::Image { current, .. } if current == resource)
                })
            { return Err(unavailable()); }
        }
        self.check_open()?;
        if self.control.health_gate().status().revision != expected_global_revision
            || !in_window(self.clock.now())
        {
            return Err(unavailable());
        }
        Ok(())
    }
    pub fn page_profile_content(
        &mut self,
        generation: SceneGeneration,
        expected_global: Revision,
        request: PageContentRequest,
    ) -> Result<PublishedContentSnapshot> {
        self.page_profile_content_while(generation, expected_global, request, Arc::new(|| true))
    }
    /// Read-only paging remains bound to the exact published scene. Repeat the
    /// job lifetime fence at writer evaluation and precommit without inventing
    /// an administrator actor. A later cancellation cannot undo COMMIT.
    pub fn page_profile_content_while(
        &mut self,
        generation: SceneGeneration,
        expected_global: Revision,
        request: PageContentRequest,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<PublishedContentSnapshot> {
        check_content_execution(fence.as_ref()).map_err(content_execution_expired)?;
        self.check_open()?;
        let expected = (
            request.slot,
            request.expected_content_revision,
            request.expected_view_revision,
            expected_global,
            generation,
        );
        if !self
            .control
            .profile_content_scene_matches(expected, self.clock.now(), false)
        {
            return Err(unavailable());
        }
        let control = self.control.clone();
        let clock = self.clock.clone();
        let mut repository = self
            .repository
            .clone()
            .with_profile_content_scene_guard(Arc::new(move |slot, content, view, global| {
                check_content_execution(fence.as_ref())?;
                if (slot, content, view, global) != (expected.0, expected.1, expected.2, expected.3)
                    || !control.profile_content_scene_matches(expected, clock.now(), true)
                {
                    // A stale read-only page is a normal context rejection;
                    // actual authority faults are independently gated globally.
                    Err(StorageError::ProfileContent(
                        ProfileContentError::InvalidSlot,
                    ))
                } else {
                    check_content_execution(fence.as_ref())
                }
            }));
        Ok(repository.page_profile_content(request, self.clock.as_ref())?)
    }
    pub fn reserve_profile_media(
        &self,
        lease: SessionLease,
        admission: &ContentAdmission,
        bytes: &[u64],
    ) -> Result<MediaReservation> {
        let (actor, generation) = self.profile_content_actor(lease)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .reserve_profile_media(admission, bytes, &actor)?)
    }
    /// Trusted worker-exit/physical cleanup only; must remain available after a
    /// local lock so a stale original preparation cannot leak quota ownership.
    pub fn settle_profile_media(&self, reservation: MediaReservation) -> Result<()> {
        self.check_open()?;
        Ok(self.repository.settle_profile_media(reservation)?)
    }
    pub fn inventory_profile_media(
        &self,
        after: Option<MediaVersionId>,
        limit: usize,
    ) -> Result<MediaInventory> {
        self.check_open()?;
        Ok(self.repository.inventory_profile_media(after, limit)?)
    }
    pub fn retire_profile_media(
        &self,
        id: MediaVersionId,
        expected: Revision,
    ) -> Result<MediaRetirement> {
        self.check_open()?;
        Ok(self.repository.retire_profile_media(id, expected)?)
    }
    pub fn finalize_profile_media_removal(&self, ticket: MediaRetirement) -> Result<()> {
        self.check_open()?;
        Ok(self.repository.finalize_profile_media_removal(ticket)?)
    }
    /// Exclusive startup, before any old worker can remain alive. Not a UI/API.
    pub fn settle_profile_media_after_restart(
        &self,
        id: MediaVersionId,
        expected: Revision,
    ) -> Result<()> {
        self.check_open()?;
        Ok(self
            .repository
            .settle_profile_media_after_restart(id, expected)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::{layout::*, profile::LayoutDocument};
    use fc_ports::{prepare_profile_content, ProfileContentPreparation, ProfileContentStatus};
    use fc_presentation::scene::ContentTextSource;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct FixtureClock(Mutex<LocalDateTime>);
    impl Clock for FixtureClock {
        fn now(&self) -> LocalDateTime {
            *self.0.lock().unwrap()
        }
    }
    impl MonotonicClock for FixtureClock {
        fn monotonic_millis(&self) -> u64 {
            1000
        }
    }
    struct Fixture {
        backend: Backend,
        path: PathBuf,
        clock: Arc<FixtureClock>,
        slot: ContentSlotKey,
        lease: SessionLease,
    }
    impl Fixture {
        fn new(kind: ContentPayloadKind) -> Self {
            static SEQUENCE: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "FutureClassroom-ContentBackend-R18-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let now = LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 3600).unwrap();
            let clock = Arc::new(FixtureClock(Mutex::new(now)));
            let mut backend = Backend::open(
                path.join("state.sqlite3"),
                CredentialRecordCodec::from_key([43; 32]).unwrap(),
                clock.clone(),
                clock.clone(),
            )
            .unwrap();
            // Real password KDF creates credentials. Synthetic verified leases
            // below isolate service/SQLite/scene semantics, not login correctness.
            drop(
                backend
                    .initialize(
                        Initialize {
                            school_name: "Synthetic content".into(),
                            login_name: "admin".into(),
                            display_name: "Admin".into(),
                        },
                        Password::new("Synthetic content credential 2026!".into()).unwrap(),
                    )
                    .unwrap(),
            );
            let state = backend.state().unwrap();
            let account = state.account(PrincipalId::new(1).unwrap()).unwrap();
            let lease = backend
                .ledger
                .lock()
                .unwrap()
                .sessions
                .issue_verified(
                    account,
                    TrustedOrigin::Local,
                    1000,
                    SESSION_MILLIS,
                    LocalDateTime::new(now.date, 86399).unwrap(),
                )
                .unwrap();
            backend
                .control
                .0
                .active_local
                .store(lease.id(), Ordering::Release);
            backend
                .control
                .0
                .active_principal
                .store(lease.principal_id().get(), Ordering::Release);
            let mut layout = LayoutDocument::default();
            let node_id = layout.allocate_id().unwrap();
            let outlet = layout
                .tree
                .nodes
                .iter_mut()
                .find(|node| matches!(node.kind, NodeKind::Outlet { .. }))
                .unwrap();
            let removed = outlet.children.remove(0);
            outlet.children.push(node_id);
            layout.tree.nodes.retain(|node| node.id != removed);
            layout.tree.nodes.push(LayoutNode {
                id: node_id,
                kind: NodeKind::Widget(match kind {
                    ContentPayloadKind::Text => WidgetKind::SubjectText,
                    ContentPayloadKind::Images => WidgetKind::SubjectImage,
                }),
                children: vec![],
            });
            layout
                .content_styles
                .push(ContentNodeStyle::new(node_id, kind));
            backend
                .save_public_layout(
                    lease,
                    Revision::default(),
                    Revision::default(),
                    layout,
                    DisplaySettings::default(),
                )
                .unwrap();
            Self {
                backend,
                path,
                clock,
                slot: ContentSlotKey {
                    scope: LayoutScope::Public,
                    node_id,
                },
                lease,
            }
        }
        fn begin(
            &mut self,
            id: u64,
            expected: u64,
            operation: ContentOperation,
        ) -> ContentAdmission {
            self.backend
                .begin_profile_content(
                    self.lease,
                    BeginContentRequest {
                        command_id: CommandId::new(id).unwrap(),
                        slot: self.slot,
                        expected_revision: Revision::new(expected),
                        operation,
                        payload_fingerprint: [id as u8; 32],
                        expires_at: LocalDateTime::new(self.clock.now().date, 7200).unwrap(),
                    },
                )
                .unwrap()
        }
        fn commit(
            &mut self,
            admission: ContentAdmission,
            payload: ContentPayload,
        ) -> PublishedContentSnapshot {
            let prepared = prepare_profile_content(&mut Preparation, admission, payload).unwrap();
            self.backend
                .commit_profile_content(self.lease, prepared)
                .unwrap()
        }
        fn add_image_slot(&mut self) -> ContentSlotKey {
            let mut public = self.backend.read_public_layout(self.lease).unwrap();
            let expected = public.layout.revision;
            let node_id = public.layout.allocate_id().unwrap();
            let recess = public
                .layout
                .tree
                .nodes
                .iter()
                .find(|node| matches!(node.kind, NodeKind::Widget(WidgetKind::Recess)))
                .unwrap()
                .id;
            public.layout.tree.nodes.retain(|node| node.id != recess);
            for node in &mut public.layout.tree.nodes {
                node.children.retain(|id| *id != recess);
            }
            public
                .layout
                .tree
                .nodes
                .iter_mut()
                .find(|node| matches!(node.kind, NodeKind::Outlet { .. }))
                .unwrap()
                .children
                .push(node_id);
            public.layout.tree.nodes.push(LayoutNode {
                id: node_id,
                kind: NodeKind::Widget(WidgetKind::SubjectImage),
                children: vec![],
            });
            public
                .layout
                .content_styles
                .push(ContentNodeStyle::new(node_id, ContentPayloadKind::Images));
            self.backend
                .save_public_layout(
                    self.lease,
                    expected,
                    public.display.revision,
                    public.layout,
                    public.display,
                )
                .unwrap();
            ContentSlotKey {
                scope: LayoutScope::Public,
                node_id,
            }
        }
        fn two_actual_lessons(&mut self) {
            use fc_domain::{schedule::*, schedule_document::*};
            let class_id = ClassId::new(1).unwrap();
            let subject_id = SubjectId::new(1).unwrap();
            self.backend
                .identity(
                    self.lease,
                    IdentityCommand::CreateClass {
                        id: class_id,
                        name: "Synthetic class".into(),
                    },
                )
                .unwrap();
            self.backend
                .identity(
                    self.lease,
                    IdentityCommand::PutSubject {
                        id: subject_id,
                        name: "Synthetic subject".into(),
                        enabled: true,
                        color: Rgb {
                            red: 20,
                            green: 40,
                            blue: 60,
                        },
                        expected_revision: None,
                    },
                )
                .unwrap();
            self.backend
                .identity(
                    self.lease,
                    IdentityCommand::BindCurrentClass { id: class_id },
                )
                .unwrap();
            let rows = (1..=2)
                .map(|number| TimetableRow {
                    id: RowId::new(number).unwrap(),
                    logical_key: LogicalRowKey::new(number).unwrap(),
                    group: DayGroup::Morning,
                    kind: RowKind::Course,
                    range: MinuteRange::new(
                        Minute::new(59 + number as u16).unwrap(),
                        Minute::new(60 + number as u16).unwrap(),
                    )
                    .unwrap(),
                    enabled_days: [true; 7],
                    courses: std::array::from_fn(|_| {
                        Some(CourseCell {
                            subject_id,
                            teacher: TeacherBinding::Unassigned,
                        })
                    }),
                })
                .collect();
            let document = ScheduleDocument {
                semesters: vec![SemesterPlan {
                    semester: Semester {
                        id: SemesterId::new(1).unwrap(),
                        class_id,
                        revision: Revision::default(),
                        enabled: true,
                        start: LocalDate::new(2026, 10, 1).unwrap(),
                        end: LocalDate::new(2026, 10, 31).unwrap(),
                        first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
                        tables: vec![WeekTable {
                            id: TableId::new(1).unwrap(),
                            role: TableRole::Single,
                            rows,
                        }],
                    },
                    name: "Actual lesson switch".into(),
                    timing_sync: false,
                    dual_history: None,
                    row_details: vec![],
                    cell_details: vec![],
                }],
                ..ScheduleDocument::default()
            };
            self.backend
                .save_schedule(self.lease, Revision::default(), document)
                .unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.backend.close().unwrap();
            std::fs::remove_dir_all(&self.path).unwrap();
        }
    }
    struct Preparation;
    impl ProfileContentPreparation for Preparation {
        type Input = ContentPayload;
        fn prepare(
            &mut self,
            _: &ContentAdmission,
            input: Self::Input,
        ) -> std::result::Result<ContentPayload, ProfileContentRepositoryError> {
            Ok(input)
        }
        fn sha256(
            &self,
            bytes: &[u8],
        ) -> std::result::Result<[u8; 32], ProfileContentRepositoryError> {
            Ok(fc_security::sha256(bytes))
        }
    }
    struct HeldPublication {
        snapshot: Snapshot,
        entered: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
    }
    impl fc_storage::TransactionTask<Snapshot> for HeldPublication {
        fn allocation_bytes(&self) -> usize {
            std::mem::size_of::<Self>() + 256
        }
        fn evaluate(
            self: Box<Self>,
            _: &dyn fc_storage::TransactionDocuments,
        ) -> std::result::Result<fc_storage::TransactionDecision<Snapshot>, StorageError> {
            let Self {
                snapshot,
                entered,
                release,
            } = *self;
            Ok(fc_storage::TransactionDecision {
                outcome: vec![],
                commit: Some((
                    vec![fc_storage::Mutation::Put {
                        key: "synthetic/profile-content/unrelated-publication".into(),
                        value: vec![1],
                    }],
                    snapshot,
                )),
                precommit: Some(Box::new(move || {
                    entered.store(true, Ordering::Release);
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while !release.load(Ordering::Acquire) {
                        if Instant::now() >= deadline {
                            return Err(StorageError::AuthorityUnavailable);
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Ok(())
                })),
            })
        }
    }
    struct ReleasePublication(Arc<AtomicBool>);
    impl Drop for ReleasePublication {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    type ExecutionFence = Arc<dyn Fn() -> bool + Send + Sync>;
    enum FenceOwner {
        Hold {
            sender: std::sync::mpsc::SyncSender<ExecutionFence>,
            release: Arc<AtomicBool>,
        },
        Panic,
    }
    impl fc_runtime::worker::Task for FenceOwner {
        type Output = ();
        fn allocation_bytes(&self) -> Option<usize> {
            Some(0)
        }
        fn run(self, context: &fc_runtime::worker::JobContext) {
            match self {
                Self::Hold { sender, release } => {
                    sender.send(Arc::new(context.execution_fence())).unwrap();
                    while !release.load(Ordering::Acquire) && context.checkpoint().is_ok() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                Self::Panic => panic!("synthetic physical worker fault"),
            }
        }
    }
    #[derive(Clone, Copy)]
    enum StopFence {
        Cancel,
        Deadline,
        Stop,
        Fault,
    }

    fn submit_fence_owner(
        handle: &fc_runtime::worker::WorkerHandle<FenceOwner>,
        mut job: FenceOwner,
        deadline: Instant,
    ) -> fc_runtime::worker::JobTicket<()> {
        loop {
            match handle.try_submit(job, deadline) {
                Ok(ticket) => return ticket,
                // Pre-admission contention retains the returned owner; this
                // retries neither SQL nor a command that has entered a queue.
                Err(error)
                    if error.kind == fc_runtime::worker::Rejection::Busy
                        && Instant::now() < deadline =>
                {
                    job = error.task;
                    std::thread::yield_now();
                }
                Err(error) => panic!("fence owner admission: {:?}", error.kind),
            }
        }
    }

    #[test]
    fn guest_page_job_cancel_and_deadline_at_real_precommit_do_not_advance_view() {
        use fc_runtime::{
            health::Health,
            worker::{JobError, WorkerPool},
        };
        let mut f = Fixture::new(ContentPayloadKind::Images);
        let admission = f.begin(
            300,
            0,
            ContentOperation::Publish(ContentPayloadKind::Images),
        );
        let reservation = f
            .backend
            .reserve_profile_media(f.lease, &admission, &[10, 10])
            .unwrap();
        // Trusted metadata-only preparation isolates guest paging/SQLite. It
        // does not claim actual media files/WIC; payload SHA uses real crypto.
        let published = f.commit(
            admission,
            ContentPayload::Images(
                reservation
                    .ids
                    .iter()
                    .map(|id| MediaReference {
                        version_id: *id,
                        owner: f.slot,
                        sha256: fc_security::sha256(b"0123456789"),
                        bytes: 10,
                        width: 1,
                        height: 1,
                        format: ImageFormat::Bmp,
                    })
                    .collect(),
            ),
        );
        let before = f.backend.read_profile_content(f.lease, f.slot).unwrap();
        for (index, mode) in [StopFence::Cancel, StopFence::Deadline]
            .into_iter()
            .enumerate()
        {
            let facts = f.backend.desktop_facts(0).unwrap();
            let generation = SceneGeneration::new(20 + index as u64);
            assert!(f.backend.session_control().publish_profile_content_scene(
                &facts.profile_contents,
                generation,
                facts.global_revision,
                facts.captured_at,
                None
            ));
            let request = PageContentRequest {
                command_id: CommandId::new(301 + index as u64).unwrap(),
                slot: f.slot,
                expected_content_revision: published.revision,
                expected_view_revision: published.view_revision,
                page: 1,
            };
            let pool = WorkerPool::<FenceOwner>::start().unwrap();
            let handle = pool.handle();
            let (sender, receive) = std::sync::mpsc::sync_channel(1);
            let owner_release = Arc::new(AtomicBool::new(false));
            let _owner_release = ReleasePublication(owner_release.clone());
            let deadline = Instant::now()
                + if matches!(mode, StopFence::Deadline) {
                    Duration::from_secs(3)
                } else {
                    Duration::from_secs(10)
                };
            let ticket = submit_fence_owner(
                &handle,
                FenceOwner::Hold {
                    sender,
                    release: owner_release.clone(),
                },
                deadline,
            );
            let fence = receive.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(fence());
            let entered = Arc::new(AtomicBool::new(false));
            let release = Arc::new(AtomicBool::new(false));
            let _release = ReleasePublication(release.clone());
            let gate = f.backend.session_control().health_gate();
            let held_fence = {
                let entered = entered.clone();
                let release = release.clone();
                Arc::new(move || {
                    if gate.status().health == Health::AuthorityUpdating {
                        entered.store(true, Ordering::Release);
                        let until = Instant::now() + Duration::from_secs(10);
                        while !release.load(Ordering::Acquire) && Instant::now() < until {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                    fence()
                }) as ExecutionFence
            };
            let (result, ticket) = std::thread::scope(|scope| {
                let observer = scope.spawn(move || {
                    let until = Instant::now() + Duration::from_secs(10);
                    while !entered.load(Ordering::Acquire) {
                        assert!(
                            Instant::now() < until,
                            "guest page did not reach real precommit"
                        );
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    match mode {
                        StopFence::Cancel => ticket.cancel(),
                        StopFence::Deadline => {
                            while Instant::now() < deadline {
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        }
                        _ => unreachable!("this fixture covers cancel and deadline"),
                    }
                    release.store(true, Ordering::Release);
                    ticket
                });
                // No ActorContext is fabricated: the repository's exact guest
                // scene capability and the real job fence are both required.
                let result = f.backend.page_profile_content_while(
                    generation,
                    facts.global_revision,
                    request,
                    held_fence,
                );
                owner_release.store(true, Ordering::Release);
                (result, observer.join().unwrap())
            });
            assert!(matches!(
                result,
                Err(BackendError::ProfileContent(
                    ProfileContentRepositoryError::Domain(ProfileContentError::Expired)
                ))
            ));
            assert_eq!(
                ticket.wait(),
                Err(if matches!(mode, StopFence::Cancel) {
                    JobError::Cancelled
                } else {
                    JobError::Deadline
                })
            );
            pool.join().unwrap();
            assert_eq!(
                f.backend.session_control().health_gate().status().health,
                Health::Healthy
            );
            let after = f.backend.read_profile_content(f.lease, f.slot).unwrap();
            assert_eq!(after.revision, before.revision);
            assert_eq!(after.view_revision, before.view_revision);
            assert_eq!(after.current_page, before.current_page);
            assert_eq!(
                after.latest_publish_sequence,
                before.latest_publish_sequence
            );
            assert_eq!(after.current, before.current);
        }
        let facts = f.backend.desktop_facts(0).unwrap();
        let generation = SceneGeneration::new(22);
        assert!(f.backend.session_control().publish_profile_content_scene(
            &facts.profile_contents,
            generation,
            facts.global_revision,
            facts.captured_at,
            None
        ));
        let request = PageContentRequest {
            command_id: CommandId::new(303).unwrap(),
            slot: f.slot,
            expected_content_revision: published.revision,
            expected_view_revision: published.view_revision,
            page: 1,
        };
        assert!(matches!(
            f.backend.page_profile_content_while(
                generation,
                facts.global_revision,
                request.clone(),
                Arc::new(|| panic!("synthetic guest fence panic"))
            ),
            Err(BackendError::ProfileContent(
                ProfileContentRepositoryError::Domain(ProfileContentError::Expired)
            ))
        ));
        assert_eq!(
            f.backend.session_control().health_gate().status().health,
            Health::Healthy
        );

        // A completed page COMMIT remains durable even if its context owner is
        // cancelled before consuming completion. Readback resolves that result.
        let pool = WorkerPool::<FenceOwner>::start().unwrap();
        let (sender, receive) = std::sync::mpsc::sync_channel(1);
        let release = Arc::new(AtomicBool::new(false));
        let _release = ReleasePublication(release.clone());
        let ticket = submit_fence_owner(
            &pool.handle(),
            FenceOwner::Hold {
                sender,
                release: release.clone(),
            },
            Instant::now() + Duration::from_secs(10),
        );
        let fence = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        let saved = f
            .backend
            .page_profile_content_while(generation, facts.global_revision, request, fence)
            .unwrap();
        ticket.cancel();
        release.store(true, Ordering::Release);
        assert_eq!(ticket.wait(), Err(JobError::Cancelled));
        pool.join().unwrap();
        let after = f.backend.read_profile_content(f.lease, f.slot).unwrap();
        assert_eq!(saved.current_page, 1);
        assert_eq!(after.current_page, 1);
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.view_revision, saved.view_revision);
        assert!(after.view_revision > before.view_revision);
        assert_eq!(
            after.latest_publish_sequence,
            before.latest_publish_sequence
        );
        f.backend.session_control().clear_profile_content_scene();
        let global = f.backend.session_control().health_gate().status().revision;
        assert!(
            f.backend
                .page_profile_content_while(
                    generation,
                    global,
                    PageContentRequest {
                        command_id: CommandId::new(304).unwrap(),
                        slot: f.slot,
                        expected_content_revision: after.revision,
                        expected_view_revision: after.view_revision,
                        page: 0
                    },
                    Arc::new(|| true)
                )
                .is_err(),
            "a live job alone cannot authorize a guest page without a published scene"
        );
        assert_eq!(
            f.backend
                .read_profile_content(f.lease, f.slot)
                .unwrap()
                .current_page,
            1
        );
    }

    #[test]
    fn actual_job_fence_cancellation_deadline_stop_and_fault_at_sqlite_precommit_roll_back() {
        use fc_runtime::{
            health::Health,
            worker::{JobError, WorkerPool},
        };
        let mut f = Fixture::new(ContentPayloadKind::Text);
        let initial = f.begin(100, 0, ContentOperation::Publish(ContentPayloadKind::Text));
        f.commit(initial, ContentPayload::Text("durable original".into()));
        for (index, mode) in [
            StopFence::Cancel,
            StopFence::Deadline,
            StopFence::Stop,
            StopFence::Fault,
        ]
        .into_iter()
        .enumerate()
        {
            let admission = f.begin(
                101 + index as u64,
                1,
                ContentOperation::Publish(ContentPayloadKind::Text),
            );
            let sequence = admission.sequence;
            let proof = prepare_profile_content(
                &mut Preparation,
                admission.clone(),
                ContentPayload::Text("must roll back".into()),
            )
            .unwrap();
            let pool = WorkerPool::<FenceOwner>::start().unwrap();
            let handle = pool.handle();
            let (sender, receive) = std::sync::mpsc::sync_channel(1);
            let worker_release = Arc::new(AtomicBool::new(false));
            let _release_worker = ReleasePublication(worker_release.clone());
            let until = Instant::now()
                + if matches!(mode, StopFence::Deadline) {
                    Duration::from_secs(3)
                } else {
                    Duration::from_secs(10)
                };
            let ticket = submit_fence_owner(
                &handle,
                FenceOwner::Hold {
                    sender,
                    release: worker_release.clone(),
                },
                until,
            );
            // A real worker issues this opaque context lifetime capability. The
            // coordinator drives Backend synchronously while that owner remains
            // live, so no test-only clock/control state is fabricated.
            let fence = receive.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(fence());
            let entered = Arc::new(AtomicBool::new(false));
            let release = Arc::new(AtomicBool::new(false));
            let _release_commit = ReleasePublication(release.clone());
            let gate = f.backend.session_control().health_gate();
            let held_fence = {
                let entered = entered.clone();
                let release = release.clone();
                Arc::new(move || {
                    // This bounded fixture barrier runs only at the actual
                    // writer precommit (SQL mutations exist, COMMIT has not).
                    if gate.status().health == Health::AuthorityUpdating {
                        entered.store(true, Ordering::Release);
                        let limit = Instant::now() + Duration::from_secs(10);
                        while !release.load(Ordering::Acquire) && Instant::now() < limit {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                    fence()
                }) as ExecutionFence
            };
            let (result, ticket) = std::thread::scope(|scope| {
                let observer = scope.spawn(move || {
                    let limit = Instant::now() + Duration::from_secs(10);
                    while !entered.load(Ordering::Acquire) {
                        assert!(Instant::now() < limit, "did not reach actual precommit");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    match mode {
                        StopFence::Cancel => ticket.cancel(),
                        StopFence::Deadline => {
                            while Instant::now() < until {
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        }
                        StopFence::Stop => handle.request_stop(),
                        StopFence::Fault => {
                            let panic = submit_fence_owner(&handle, FenceOwner::Panic, limit);
                            while !handle.is_faulted() {
                                assert!(Instant::now() < limit);
                                std::thread::sleep(Duration::from_millis(1));
                            }
                            drop(panic);
                        }
                    }
                    release.store(true, Ordering::Release);
                    ticket
                });
                let result = f
                    .backend
                    .commit_profile_content_while(f.lease, proof, held_fence);
                worker_release.store(true, Ordering::Release);
                (result, observer.join().unwrap())
            });
            assert!(matches!(
                result,
                Err(BackendError::ProfileContent(
                    ProfileContentRepositoryError::Domain(ProfileContentError::Expired)
                ))
            ));
            let expected = match mode {
                StopFence::Cancel => JobError::Cancelled,
                StopFence::Deadline => JobError::Deadline,
                StopFence::Stop => JobError::Stopped,
                StopFence::Fault => JobError::Faulted,
            };
            assert_eq!(ticket.wait(), Err(expected));
            let joined = pool.join();
            if matches!(mode, StopFence::Fault) {
                assert!(joined.is_err());
            } else {
                joined.unwrap();
            }
            assert_eq!(
                f.backend.session_control().health_gate().status().health,
                Health::Healthy,
                "ordinary expired execution must restore known DB authority, not fault it"
            );
            let current = f.backend.read_profile_content(f.lease, f.slot).unwrap();
            assert_eq!(current.revision, Revision::new(1));
            assert_eq!(
                current.current.unwrap().payload,
                ContentPayload::Text("durable original".into())
            );
            assert_eq!(
                current.latest_publish_sequence, sequence,
                "durable Begin is not rolled back with the rejected finish"
            );
            f.backend.fail_profile_content(f.lease, admission).unwrap();
        }

        // Cancellation observed after a completed COMMIT is result loss, not an
        // undo capability. Persisted outcome/current remain the source of truth.
        let admission = f.begin(200, 1, ContentOperation::Publish(ContentPayloadKind::Text));
        let proof = prepare_profile_content(
            &mut Preparation,
            admission,
            ContentPayload::Text("committed before cancellation".into()),
        )
        .unwrap();
        let pool = WorkerPool::<FenceOwner>::start().unwrap();
        let (sender, receive) = std::sync::mpsc::sync_channel(1);
        let release = Arc::new(AtomicBool::new(false));
        let _release = ReleasePublication(release.clone());
        let ticket = submit_fence_owner(
            &pool.handle(),
            FenceOwner::Hold {
                sender,
                release: release.clone(),
            },
            Instant::now() + Duration::from_secs(10),
        );
        let fence = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        let saved = f
            .backend
            .commit_profile_content_while(f.lease, proof, fence)
            .unwrap();
        ticket.cancel();
        release.store(true, Ordering::Release);
        assert_eq!(ticket.wait(), Err(JobError::Cancelled));
        pool.join().unwrap();
        assert_eq!(saved.revision, Revision::new(2));
        let current = f.backend.read_profile_content(f.lease, f.slot).unwrap();
        assert_eq!(
            current.current.unwrap().payload,
            ContentPayload::Text("committed before cancellation".into())
        );
        assert_eq!(
            f.backend
                .profile_content_outcome(f.lease, CommandId::new(200).unwrap())
                .unwrap()
                .unwrap()
                .status,
            ProfileContentStatus::Committed
        );
    }

    #[test]
    fn backend_text_hydration_clear_undo_outcome_and_locked_original_lease_use_real_sqlite() {
        let mut f = Fixture::new(ContentPayloadKind::Text);
        let guard = f
            .backend
            .profile_content_preparation_guard(f.lease)
            .unwrap();
        assert!(guard.is_ready());
        let current = f.backend.authority.snapshot().unwrap();
        let snapshot = f
            .backend
            .authority
            .prepare_update(
                current.data.clone(),
                Revision::new(current.revision.get() + 1),
                current.auth_epoch,
            )
            .unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let _release = ReleasePublication(release.clone());
        let pending = f
            .backend
            .handle
            .try_transaction(HeldPublication {
                snapshot,
                entered: entered.clone(),
                release: release.clone(),
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            guard.state(),
            ProfileContentPreparationState::AuthorityUpdating
        );
        assert!(
            guard.is_context_valid(),
            "a barrier is not permanent revocation"
        );
        assert!(!guard.is_ready(), "a barrier never authorizes IO/decode");
        release.store(true, Ordering::Release);
        pending.wait().unwrap();
        assert!(
            guard.is_ready(),
            "revalidate current authority after a normal publication"
        );
        let admission = f.begin(1, 0, ContentOperation::Publish(ContentPayloadKind::Text));
        let published = f.commit(admission, ContentPayload::Text("\n中文😀正文\n".into()));
        assert!(
            guard.is_ready(),
            "ordinary content revisions do not revoke preparation"
        );
        let facts = f.backend.desktop_facts(0).unwrap();
        let frame = f.backend.hydrate_profile_content(&facts).unwrap();
        let hash = fc_security::sha256("\n中文😀正文\n".as_bytes());
        let held = frame.text(f.slot, published.revision, hash).unwrap();
        assert_eq!(held.as_ref(), "\n中文😀正文\n");
        let mut bad = facts.clone();
        if let PublishedContentDescriptor::Text { sha256, .. } =
            &mut bad.profile_contents[0].content
        {
            *sha256 = [9; 32];
        }
        assert!(f.backend.hydrate_profile_content(&bad).is_err());
        let state = f.backend.state().unwrap();
        let account = state.account(f.lease.principal_id()).unwrap();
        let web = f
            .backend
            .ledger
            .lock()
            .unwrap()
            .sessions
            .issue_verified(
                account,
                TrustedOrigin::Web,
                1000,
                SESSION_MILLIS,
                LocalDateTime::new(f.clock.now().date, 86399).unwrap(),
            )
            .unwrap();
        assert!(f.backend.read_profile_content(web, f.slot).is_err());
        let clear = f.begin(2, 1, ContentOperation::Clear);
        f.commit(clear, ContentPayload::Empty);
        assert!(
            f.backend.hydrate_profile_content(&facts).is_err(),
            "an old global facts revision cannot hydrate after replacement"
        );
        let undo = f.begin(
            3,
            2,
            ContentOperation::Undo {
                target_revision: Revision::new(1),
            },
        );
        let restored = f.commit(undo, ContentPayload::Empty);
        assert_eq!(restored.revision, Revision::new(3));
        assert_eq!(
            f.backend
                .profile_content_outcome(f.lease, CommandId::new(3).unwrap())
                .unwrap()
                .unwrap()
                .status,
            ProfileContentStatus::Committed
        );
        let stale = f.begin(4, 3, ContentOperation::Publish(ContentPayloadKind::Text));
        let prepared = prepare_profile_content(
            &mut Preparation,
            stale,
            ContentPayload::Text("must not commit".into()),
        )
        .unwrap();
        f.backend.session_control().lock_local();
        assert_eq!(guard.state(), ProfileContentPreparationState::Revoked);
        assert!(f.backend.commit_profile_content(f.lease, prepared).is_err());
        assert_eq!(
            held.as_ref(),
            "\n中文😀正文\n",
            "retired scene owners still own billed text"
        );
        assert!(
            ProfileContentTextCache::shared()
                .stats()
                .unwrap()
                .charged_bytes
                >= held.len()
        );
    }
    #[test]
    fn preparation_guard_rechecks_actual_account_epoch_and_fails_closed_on_health_fault() {
        let mut f = Fixture::new(ContentPayloadKind::Text);
        let guard = f
            .backend
            .profile_content_preparation_guard(f.lease)
            .unwrap();
        f.backend
            .identity(
                f.lease,
                IdentityCommand::UpdateAccount {
                    id: f.lease.principal_id(),
                    display_name: "Renamed actual account".into(),
                    role: Role::GlobalAdmin,
                    enabled: true,
                    scopes: vec![],
                },
            )
            .unwrap();
        assert_eq!(
            guard.state(),
            ProfileContentPreparationState::Revoked,
            "even a self-name edit advances security epoch and invalidates the exact old lease"
        );
        assert!(f
            .backend
            .profile_content_preparation_guard(f.lease)
            .is_err());
        let current = f.backend.authority.snapshot().unwrap();
        let account = current.data.account(f.lease.principal_id()).unwrap();
        let lease = f
            .backend
            .ledger
            .lock()
            .unwrap()
            .sessions
            .issue_verified(
                account,
                TrustedOrigin::Local,
                1000,
                SESSION_MILLIS,
                LocalDateTime::new(f.clock.now().date, 86399).unwrap(),
            )
            .unwrap();
        f.backend
            .control
            .0
            .active_local
            .store(lease.id(), Ordering::Release);
        let fresh = f.backend.profile_content_preparation_guard(lease).unwrap();
        assert!(fresh.is_ready());
        assert_eq!(
            guard.state(),
            ProfileContentPreparationState::Revoked,
            "a new local lease cannot revive the old preparation capability"
        );
        f.backend.session_control().health_gate().fault();
        assert_eq!(fresh.state(), ProfileContentPreparationState::Revoked);
        assert!(!fresh.is_context_valid());
    }
    #[test]
    fn backend_guest_page_requires_published_generation_global_view_and_live_time_window() {
        let mut f = Fixture::new(ContentPayloadKind::Images);
        let second_slot = f.add_image_slot();
        f.two_actual_lessons();
        let preparation_guard = f
            .backend
            .profile_content_preparation_guard(f.lease)
            .unwrap();
        let admission = f.begin(1, 0, ContentOperation::Publish(ContentPayloadKind::Images));
        let reservation = f
            .backend
            .reserve_profile_media(f.lease, &admission, &[10, 10])
            .unwrap();
        // Metadata-only trusted adapter fixture: tests authorization/SQLite page
        // lifecycle, not image decoding or filesystem verification.
        let pages = reservation
            .ids
            .iter()
            .map(|id| MediaReference {
                version_id: *id,
                owner: f.slot,
                sha256: fc_security::sha256(b"fixture"),
                bytes: 10,
                width: 1,
                height: 1,
                format: ImageFormat::Png,
            })
            .collect();
        let published = f.commit(admission, ContentPayload::Images(pages));
        let second_admission = f
            .backend
            .begin_profile_content(
                f.lease,
                BeginContentRequest {
                    command_id: CommandId::new(11).unwrap(),
                    slot: second_slot,
                    expected_revision: Revision::default(),
                    operation: ContentOperation::Publish(ContentPayloadKind::Images),
                    payload_fingerprint: [11; 32],
                    expires_at: LocalDateTime::new(f.clock.now().date, 7200).unwrap(),
                },
            )
            .unwrap();
        let second_reservation = f
            .backend
            .reserve_profile_media(f.lease, &second_admission, &[10])
            .unwrap();
        f.commit(
            second_admission,
            ContentPayload::Images(vec![MediaReference {
                version_id: second_reservation.ids[0],
                owner: second_slot,
                sha256: fc_security::sha256(b"second fixture"),
                bytes: 10,
                width: 1,
                height: 1,
                format: ImageFormat::Png,
            }]),
        );
        assert!(
            preparation_guard.is_ready(),
            "different-node Begin and commit keep the exact lease alive"
        );
        let facts = f.backend.desktop_facts(0).unwrap();
        let requests = |facts: &DesktopFacts| {
            facts
                .profile_contents
                .iter()
                .filter_map(|snapshot| match &snapshot.content {
                    PublishedContentDescriptor::Image { current, .. } => Some((
                        snapshot.slot,
                        snapshot.revision,
                        snapshot.view_revision,
                        current.clone(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let batch = requests(&facts);
        assert_eq!(batch.len(), 2);
        f.backend
            .verify_profile_content_images(facts.global_revision, facts.captured_at, None, &batch)
            .unwrap();
        let mut stale_view = batch.clone();
        stale_view[0].2 = Revision::default();
        assert!(f
            .backend
            .verify_profile_content_images(
                facts.global_revision,
                facts.captured_at,
                None,
                &stale_view
            )
            .is_err());
        assert_eq!(
            f.backend.session_control().health_gate().status().health,
            fc_runtime::health::Health::Healthy
        );
        let control = f.backend.session_control();
        let page = PageContentRequest {
            command_id: CommandId::new(2).unwrap(),
            slot: f.slot,
            expected_content_revision: published.revision,
            expected_view_revision: published.view_revision,
            page: 1,
        };
        assert!(f
            .backend
            .page_profile_content(SceneGeneration::new(1), facts.global_revision, page.clone())
            .is_err());
        let until = LocalDateTime::new(f.clock.now().date, 3660).unwrap();
        assert!(control.publish_profile_content_scene(
            &facts.profile_contents,
            SceneGeneration::new(1),
            facts.global_revision,
            facts.captured_at,
            Some(until)
        ));
        assert!(f
            .backend
            .page_profile_content(SceneGeneration::new(2), facts.global_revision, page.clone())
            .is_err());
        let next = f
            .backend
            .page_profile_content(SceneGeneration::new(1), facts.global_revision, page.clone())
            .unwrap();
        assert_eq!(next.revision, published.revision);
        assert_eq!(next.current_page, 1);
        assert!(next.view_revision > published.view_revision);
        assert!(f
            .backend
            .page_profile_content(SceneGeneration::new(1), facts.global_revision, page)
            .is_err());
        let fresh = f.backend.desktop_facts(0).unwrap();
        let fresh_batch = requests(&fresh);
        assert!(control.publish_profile_content_scene(
            &fresh.profile_contents,
            SceneGeneration::new(2),
            fresh.global_revision,
            fresh.captured_at,
            Some(until)
        ));
        let next_page = PageContentRequest {
            command_id: CommandId::new(3).unwrap(),
            slot: f.slot,
            expected_content_revision: next.revision,
            expected_view_revision: next.view_revision,
            page: 0,
        };
        *f.clock.0.lock().unwrap() = until;
        assert!(f
            .backend
            .page_profile_content(SceneGeneration::new(2), fresh.global_revision, next_page)
            .is_err());
        assert!(
            f.backend
                .verify_profile_content_images(
                    fresh.global_revision,
                    fresh.captured_at,
                    None,
                    &fresh_batch
                )
                .is_err(),
            "even an unexpired caller window cannot reuse an old actual lesson"
        );
        let after_lesson = f.backend.desktop_facts(0).unwrap();
        assert_ne!(
            after_lesson.lesson_context.current.as_ref().unwrap().key,
            fresh.lesson_context.current.as_ref().unwrap().key
        );
        control.clear_profile_content_scene();
        assert!(
            !control.publish_profile_content_scene(
                &after_lesson.profile_contents,
                SceneGeneration::new(2),
                after_lesson.global_revision,
                after_lesson.captured_at,
                None
            ),
            "cleared same-generation capabilities cannot revive"
        );
    }
}
