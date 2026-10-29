use crate::{auth::*, ids::*, time::*};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HomeworkKey {
    pub class_id: ClassId,
    pub actual_date: LocalDate,
    pub subject_id: SubjectId,
    pub owner_teacher_id: PrincipalId,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReminderKey {
    pub class_id: ClassId,
    pub actual_date: LocalDate,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ContentKey {
    Homework(HomeworkKey),
    SharedReminder(ReminderKey),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentRecord {
    pub key: ContentKey,
    pub text: String,
    pub revision: Revision,
    pub last_actor_id: PrincipalId,
    pub updated_at: LocalDateTime,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentCommand {
    pub command_id: CommandId,
    pub key: ContentKey,
    pub expected_revision: Revision,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentError {
    Permission(PermissionError),
    WrongDate,
    TextTooLarge,
    RevisionConflict { current: Revision },
    VersionOverflow,
    IdempotencyConflict,
}

impl ContentCommand {
    /// Same function is called at admission and again within the repository's transaction.
    pub fn validate(
        &self,
        authority: &AuthorityContext,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<(), ContentError> {
        if self.text.len() > 16 * 1024 {
            return Err(ContentError::TextTooLarge);
        }
        match &self.key {
            ContentKey::Homework(k) => {
                if k.actual_date != now.date {
                    return Err(ContentError::WrongDate);
                }
                authority.authorize_owned_content(
                    actor,
                    now,
                    k.class_id,
                    k.subject_id,
                    k.owner_teacher_id,
                )
            }
            ContentKey::SharedReminder(k) => {
                if k.actual_date != now.date {
                    return Err(ContentError::WrongDate);
                }
                authority.authorize_shared_reminder(actor, now, k.class_id)
            }
        }
        .map_err(ContentError::Permission)
    }
    pub fn normalized_text(&self) -> String {
        self.text.trim().to_owned()
    }
    /// Transaction adapter supplies the current authoritative row, not a caller's draft.
    pub fn apply(
        &self,
        authority: &AuthorityContext,
        actor: &ActorContext,
        now: LocalDateTime,
        current: Option<&ContentRecord>,
    ) -> Result<ContentRecord, ContentError> {
        self.validate(authority, actor, now)?;
        if current.is_some_and(|r| r.key != self.key) {
            return Err(ContentError::IdempotencyConflict);
        }
        let revision = current.map_or(Revision::default(), |r| r.revision);
        if revision != self.expected_revision {
            return Err(ContentError::RevisionConflict { current: revision });
        }
        Ok(ContentRecord {
            key: self.key.clone(),
            text: self.normalized_text(),
            revision: revision.next().map_err(|_| ContentError::VersionOverflow)?,
            last_actor_id: actor.principal_id,
            updated_at: now,
        })
    }
}
impl ContentRecord {
    /// Midnight expiration is enforced on reads, independently of eventual cleanup.
    pub fn visible_text(&self, today: LocalDate) -> Option<&str> {
        let date = match &self.key {
            ContentKey::Homework(k) => k.actual_date,
            ContentKey::SharedReminder(k) => k.actual_date,
        };
        if date == today && !self.text.is_empty() {
            Some(&self.text)
        } else {
            None
        }
    }
}
