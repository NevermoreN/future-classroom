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
}
