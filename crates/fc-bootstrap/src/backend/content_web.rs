//! Teacher content composition, invoked on the shared application worker. All
//! persistence, date observations and final write guards belong to the writer.
use super::*;

pub const CONTENT_SCOPE_PAGE: usize = 20;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebContentKind {
    Homework,
    Reminder,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebContentScope {
    pub class_id: ClassId,
    pub class_name: String,
    pub subject_id: Option<SubjectId>,
    pub subject_name: Option<String>,
    pub owner_teacher_id: Option<PrincipalId>,
    pub owner_display_name: Option<String>,
}
#[derive(Clone, Debug)]
pub struct WebContentScopes {
    pub kind: WebContentKind,
    pub today: LocalDate,
    pub identity_revision: Revision,
    pub offset: usize,
    pub total: usize,
    pub items: Vec<WebContentScope>,
}
#[derive(Clone, Debug)]
pub struct WebContentDocument {
    pub key: ContentKey,
    pub now: LocalDateTime,
    pub expires_at: LocalDateTime,
    /// None means no persisted row. A cleared row is Some and retains its CAS.
    pub record: Option<ContentRecord>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentViewState {
    Visible,
    HiddenByApp,
    HiddenBySession,
    RenderFailed,
    PendingRender,
    Replaced,
    InactiveSubject,
    HiddenByComponent,
    Expired,
}
/// Actual native publication, never the complete set of fetched facts. No text,
/// resource data, database objects or native handles cross this memory boundary.
#[derive(Clone, Debug)]
pub struct ContentPresentationObservation {
    pub generation: SceneGeneration,
    pub source_revision: Revision,
    pub captured_at: LocalDateTime,
    pub valid_until: Option<LocalDateTime>,
    pub observed_at: LocalDateTime,
    pub health_epoch: u64,
    pub visibility_generation: u64,
    pub class_id: Option<ClassId>,
    pub homework_enabled: bool,
    pub reminder_enabled: bool,
    pub displayed: Vec<ContentDisplayRef>,
}
#[derive(Clone, Debug)]
pub struct WebContentPresentation {
    pub key: ContentKey,
    pub now: LocalDateTime,
    pub expires_at: LocalDateTime,
    pub committed_revision: Revision,
    pub current_revision: Revision,
    pub displayed_revision: Option<Revision>,
    pub state: ContentViewState,
    pub observed_at: Option<LocalDateTime>,
    pub cleared: bool,
}
pub(super) struct ContentPresentationMemory {
    clock: Arc<dyn Clock + Send + Sync>,
    visibility_generation: u64,
    high_scene: SceneGeneration,
    invalid_through: SceneGeneration,
    state: ContentViewState,
    observed_at: Option<LocalDateTime>,
    observation: Option<ContentPresentationObservation>,
    published_at: Option<Instant>,
}
impl ContentPresentationMemory {
    pub(super) fn new(clock: Arc<dyn Clock + Send + Sync>) -> Self {
        Self {
            clock,
            visibility_generation: 1,
            high_scene: SceneGeneration::default(),
            invalid_through: SceneGeneration::default(),
            state: ContentViewState::PendingRender,
            observed_at: None,
            observation: None,
            published_at: None,
        }
    }
}
impl SessionControl {
    pub fn content_presentation_generation(&self) -> u64 {
        self.0
            .content_presentation
            .lock()
            .map_or(0, |s| s.visibility_generation)
    }
    pub fn clear_content_presentation(&self, state: ContentViewState) {
        match self.0.content_presentation.lock() {
            Ok(mut s) => {
                // Saturation stops future publications; it never reuses a token.
                s.visibility_generation = s.visibility_generation.saturating_add(1);
                s.invalid_through = s.high_scene;
                s.observation = None;
                s.published_at = None;
                s.state = state;
                s.observed_at = Some(s.clock.now());
            }
            Err(poisoned) => {
                poisoned.into_inner().observation = None;
                self.0.health.fault();
            }
        }
    }
    pub fn publish_content_presentation(
        &self,
        observation: ContentPresentationObservation,
    ) -> bool {
        let Ok(mut s) = self.0.content_presentation.lock() else {
            self.0.health.fault();
            return false;
        };
        let health = self.0.health.status();
        let now = s.clock.now();
        if self.0.closed.load(Ordering::Acquire)
            || self.is_recovering()
            || health.health != fc_runtime::health::Health::Healthy
            || health.revision != observation.source_revision
            || health.health_epoch != observation.health_epoch
            || s.visibility_generation == u64::MAX
            || observation.visibility_generation != s.visibility_generation
            || observation.generation < s.high_scene
            || observation.generation <= s.invalid_through
            || observation.captured_at.date != now.date
            || observation.captured_at > observation.observed_at
            || observation.observed_at > now
            || observation.valid_until.is_some_and(|end| now >= end)
            || observation.displayed.len() > 64
            || observation
                .displayed
                .iter()
                .any(|r| day(&r.key) != now.date || r.revision.get() == 0)
        {
            return false;
        }
        s.high_scene = observation.generation;
        s.state = ContentViewState::Visible;
        s.observed_at = Some(observation.observed_at);
        s.published_at = Some(Instant::now());
        s.observation = Some(observation);
        true
    }
}
fn identity_error(error: IdentityError) -> BackendError {
    BackendError::Identity(IdentityRepositoryError::Domain(error))
}
fn denied(reason: PermissionError) -> BackendError {
    BackendError::Content(RepositoryError::Domain(ContentError::Permission(reason)))
}
fn wrong_day() -> BackendError {
    BackendError::Content(RepositoryError::Domain(ContentError::WrongDate))
}
fn unavailable() -> BackendError {
    BackendError::Content(RepositoryError::Unavailable)
}
fn day(key: &ContentKey) -> LocalDate {
    match key {
        ContentKey::Homework(k) => k.actual_date,
        ContentKey::SharedReminder(k) => k.actual_date,
    }
}
fn valid_text(text: &str) -> bool {
    text.len() <= 16384
        && !text.chars().any(|c| {
            (c <= '\u{1f}' && !matches!(c, '\t' | '\n' | '\r'))
                || ('\u{7f}'..='\u{9f}').contains(&c)
        })
}
impl Backend {
    fn web_content_actor(
        &self,
        state: &IdentityState,
        lease: SessionLease,
    ) -> Result<(ActorContext, u64)> {
        if lease.origin() != TrustedOrigin::Web {
            return Err(denied(PermissionError::WebOnly));
        }
        let actor = self.actor(state, lease, SessionPurpose::Manage)?;
        if state
            .account(actor.0.principal_id)
            .map_err(identity_error)?
            .principal
            .role
            == Role::User
        {
            return Err(denied(PermissionError::RoleDenied));
        }
        Ok(actor)
    }
    fn web_content_target(
        state: &IdentityState,
        actor: &ActorContext,
        key: &ContentKey,
        now: LocalDateTime,
    ) -> Result<()> {
        if day(key) != now.date {
            return Err(wrong_day());
        }
        let authority = state
            .authority(actor.principal_id)
            .map_err(identity_error)?;
        match key {
            ContentKey::Homework(k) => {
                authority
                    .authorize_owned_content(
                        actor,
                        now,
                        k.class_id,
                        k.subject_id,
                        k.owner_teacher_id,
                    )
                    .map_err(denied)?;
                if !state.is_teacher(k.owner_teacher_id, k.class_id, k.subject_id) {
                    return Err(denied(PermissionError::OutsideTeachingScope));
                }
            }
            ContentKey::SharedReminder(k) => authority
                .authorize_shared_reminder(actor, now, k.class_id)
                .map_err(denied)?,
        }
        Ok(())
    }
    fn web_content_document(
        &self,
        lease: SessionLease,
        key: ContentKey,
        record: Option<ContentRecord>,
    ) -> Result<WebContentDocument> {
        let state = self.state()?;
        let (actor, _) = self.web_content_actor(&state, lease)?;
        let now = self.clock.now();
        Self::web_content_target(&state, &actor, &key, now)?;
        if record
            .as_ref()
            .is_some_and(|r| r.key != key || r.updated_at.date != now.date || !valid_text(&r.text))
        {
            return Err(unavailable());
        }
        let expires_at = LocalDateTime::new(next_day(now)?.date, 0).map_err(|_| unavailable())?;
        // State/ledger validation is deliberately after the writer result and
        // again after building the response metadata. No input renewal occurs.
        self.web_content_actor(&state, lease)?;
        if self.clock.now().date != now.date {
            return Err(wrong_day());
        }
        Ok(WebContentDocument {
            key,
            now,
            expires_at,
            record,
        })
    }
    pub fn web_content_scopes(
        &self,
        lease: SessionLease,
        kind: WebContentKind,
        offset: usize,
    ) -> Result<WebContentScopes> {
        if offset > fc_domain::identity::MAX_ASSIGNMENTS {
            return Err(BackendError::InvalidInput);
        }
        let state = self.state()?;
        let (actor, generation) = self.web_content_actor(&state, lease)?;
        let now = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .observe_content_day(&actor)?;
        let role = state
            .account(actor.principal_id)
            .map_err(identity_error)?
            .principal
            .role;
        // Sort compact references and clone names only for the requested page,
        // rather than materialize 4096 copies of class/subject/teacher labels.
        let classes: std::collections::BTreeMap<_, _> = state
            .classes
            .iter()
            .filter(|c| c.enabled)
            .map(|c| (c.id, c))
            .collect();
        let subjects: std::collections::BTreeMap<_, _> = state
            .subjects
            .iter()
            .filter(|s| s.enabled)
            .map(|s| (s.id, s))
            .collect();
        let owners: std::collections::BTreeMap<_, _> = state
            .accounts
            .iter()
            .filter(|a| a.principal.enabled && a.principal.role != Role::User)
            .map(|a| (a.principal.id, a))
            .collect();
        let (total, items) = match kind {
            WebContentKind::Homework => {
                let mut choices: Vec<_> = state
                    .assignments
                    .iter()
                    .filter(|a| {
                        a.enabled
                            && classes.contains_key(&a.class_id)
                            && subjects.contains_key(&a.subject_id)
                            && owners.contains_key(&a.teacher_id)
                            && (role == Role::GlobalAdmin || a.teacher_id == actor.principal_id)
                    })
                    .collect();
                choices.sort_by_key(|a| (a.class_id, a.subject_id, a.teacher_id));
                let total = choices.len();
                let items = choices
                    .into_iter()
                    .skip(offset)
                    .take(CONTENT_SCOPE_PAGE)
                    .map(|a| WebContentScope {
                        class_id: a.class_id,
                        class_name: classes[&a.class_id].name.clone(),
                        subject_id: Some(a.subject_id),
                        subject_name: Some(subjects[&a.subject_id].name.clone()),
                        owner_teacher_id: Some(a.teacher_id),
                        owner_display_name: Some(owners[&a.teacher_id].display_name.clone()),
                    })
                    .collect();
                (total, items)
            }
            WebContentKind::Reminder => {
                let total = classes.len();
                let items = classes
                    .values()
                    .skip(offset)
                    .take(CONTENT_SCOPE_PAGE)
                    .map(|c| WebContentScope {
                        class_id: c.id,
                        class_name: c.name.clone(),
                        subject_id: None,
                        subject_name: None,
                        owner_teacher_id: None,
                        owner_display_name: None,
                    })
                    .collect();
                (total, items)
            }
        };
        if offset > total {
            return Err(BackendError::InvalidInput);
        }
        let fresh = self.state()?;
        self.web_content_actor(&fresh, lease)?;
        if fresh.revision != state.revision {
            return Err(unavailable());
        }
        if self.clock.now().date != now.date {
            return Err(wrong_day());
        }
        Ok(WebContentScopes {
            kind,
            today: now.date,
            identity_revision: state.revision,
            offset,
            total,
            items,
        })
    }
    pub fn web_content_read(
        &self,
        lease: SessionLease,
        key: ContentKey,
    ) -> Result<WebContentDocument> {
        let state = self.state()?;
        let (actor, generation) = self.web_content_actor(&state, lease)?;
        let record = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .content_for_web(key.clone(), &actor)?;
        self.web_content_document(lease, key, record)
    }
    pub fn web_content_save(
        &mut self,
        lease: SessionLease,
        command: ContentCommand,
    ) -> Result<WebContentDocument> {
        if !valid_text(&command.text) {
            return Err(BackendError::InvalidInput);
        }
        let state = self.state()?;
        let (actor, generation) = self.web_content_actor(&state, lease)?;
        // Check representable expiry before any write, rather than discover it
        // only after a successful COMMIT on the last supported civil date.
        next_day(self.clock.now())?;
        let key = command.key.clone();
        let record = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .execute_content_for_web(command, &actor)?;
        // Any failure after a successful write is an unknown result to the
        // client, even when the immediate cause was a midnight/authorization race.
        self.web_content_document(lease, key, Some(record))
            .map_err(|_| unavailable())
    }
    pub fn web_content_outcome(
        &self,
        lease: SessionLease,
        command_id: CommandId,
    ) -> Result<Option<WebContentDocument>> {
        let state = self.state()?;
        let (actor, generation) = self.web_content_actor(&state, lease)?;
        let record = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .content_outcome(command_id, &actor)?;
        match record {
            Some(record) => self
                .web_content_document(lease, record.key.clone(), Some(record))
                .map(Some),
            None => {
                let state = self.state()?;
                self.web_content_actor(&state, lease)?;
                Ok(None)
            }
        }
    }
    /// Read the original immutable receipt and current slot together, then join
    /// only a fresh native observation of that exact writer/health generation.
    /// The HTTP representation contains metadata only, including after midnight.
    pub fn web_content_presentation(
        &self,
        lease: SessionLease,
        command_id: CommandId,
    ) -> Result<Option<WebContentPresentation>> {
        let identity = self.state()?;
        let (actor, generation) = self.web_content_actor(&identity, lease)?;
        let Some((committed, current_revision, cleared, revision)) = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .content_presentation_metadata(command_id, &actor)?
        else {
            self.web_content_actor(&self.state()?, lease)?;
            return Ok(None);
        };
        let now = self.clock.now();
        let start = LocalDateTime::new(day(&committed.key), 0).map_err(|_| unavailable())?;
        let expires_at = LocalDateTime::new(next_day(start)?.date, 0).map_err(|_| unavailable())?;
        let (displayed_revision, observed_at, native_state) = {
            let memory = self
                .control
                .0
                .content_presentation
                .lock()
                .map_err(|_| unavailable())?;
            let health = self.control.0.health.status();
            let observation = memory.observation.as_ref().filter(|o| {
                health.health == fc_runtime::health::Health::Healthy
                    && health.revision == revision
                    && o.source_revision == revision
                    && o.health_epoch == health.health_epoch
                    && o.visibility_generation == memory.visibility_generation
                    && o.generation > memory.invalid_through
                    && o.captured_at.date == now.date
                    && o.captured_at <= now
                    && o.observed_at <= now
                    && o.valid_until.is_none_or(|end| now < end)
                    && memory
                        .published_at
                        .is_some_and(|at| at.elapsed() < Duration::from_secs(2))
            });
            if let Some(o) = observation {
                let class_id = match &committed.key {
                    ContentKey::Homework(k) => k.class_id,
                    ContentKey::SharedReminder(k) => k.class_id,
                };
                let enabled = match &committed.key {
                    ContentKey::Homework(_) => o.homework_enabled,
                    ContentKey::SharedReminder(_) => o.reminder_enabled,
                };
                let displayed = o
                    .displayed
                    .iter()
                    .find(|r| r.key == committed.key && r.revision == current_revision)
                    .map(|r| r.revision);
                let state = if o.class_id != Some(class_id) {
                    ContentViewState::InactiveSubject
                } else if !enabled {
                    ContentViewState::HiddenByComponent
                } else if displayed == Some(committed.revision) {
                    ContentViewState::Visible
                } else {
                    // A real current-page observation without this entry is not
                    // evidence that other pages or the entire batch are visible.
                    ContentViewState::PendingRender
                };
                (displayed, Some(o.observed_at), state)
            } else if memory.state != ContentViewState::Visible {
                (
                    None,
                    memory
                        .observed_at
                        .filter(|t| t.date == now.date && *t <= now),
                    memory.state,
                )
            } else {
                (None, None, ContentViewState::PendingRender)
            }
        };
        let state = if day(&committed.key) < now.date {
            ContentViewState::Expired
        } else if current_revision != committed.revision {
            ContentViewState::Replaced
        } else if cleared {
            ContentViewState::HiddenByComponent
        } else {
            native_state
        };
        self.web_content_actor(&self.state()?, lease)?;
        if self.clock.now().date != now.date {
            return Err(wrong_day());
        }
        self.check_open()?;
        Ok(Some(WebContentPresentation {
            key: committed.key,
            now,
            expires_at,
            committed_revision: committed.revision,
            current_revision,
            displayed_revision,
            state,
            observed_at,
            cleared,
        }))
    }
}
