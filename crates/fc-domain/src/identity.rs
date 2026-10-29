//! Installation and identity state. Credentials and live channel leases are deliberately
//! absent: persist credential records separately and publish only this non-secret state.
use crate::{auth::*, ids::*, time::LocalDateTime};

pub const MAX_ACCOUNTS: usize = 512;
pub const MAX_CLASSES: usize = 128;
pub const MAX_SUBJECTS: usize = 256;
pub const MAX_ASSIGNMENTS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    pub school_id: SchoolId,
    pub school_name: String,
    pub current_class: Option<ClassId>,
}

#[cfg(test)]
mod tests;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchoolClass {
    pub id: ClassId,
    pub school_id: SchoolId,
    pub name: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchoolSubject {
    pub id: SubjectId,
    pub school_id: SchoolId,
    pub name: String,
    pub enabled: bool,
    pub color: Rgb,
    pub revision: Revision,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserAccount {
    pub principal: Principal,
    pub login_name: String,
    pub display_name: String,
    pub must_change_password: bool,
    /// Monotonic references into the protected credential store, never credential bytes.
    pub credential_version: u64,
    pub recovery_version: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssignmentScope {
    pub class_id: ClassId,
    pub subject_id: SubjectId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultTeacher {
    pub class_id: ClassId,
    pub subject_id: SubjectId,
    pub teacher_id: PrincipalId,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdentityState {
    pub revision: Revision,
    pub installation: Option<Installation>,
    pub accounts: Vec<UserAccount>,
    pub classes: Vec<SchoolClass>,
    pub subjects: Vec<SchoolSubject>,
    pub assignments: Vec<TeachingAssignment>,
    pub default_teachers: Vec<DefaultTeacher>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAccount {
    pub id: PrincipalId,
    pub login_name: String,
    pub display_name: String,
    pub role: Role,
    pub scopes: Vec<AssignmentScope>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityCommand {
    FirstAdministrator {
        school_id: SchoolId,
        school_name: String,
        id: PrincipalId,
        login_name: String,
        display_name: String,
    },
    CreateClass {
        id: ClassId,
        name: String,
    },
    RenameSchool {
        name: String,
    },
    UpdateClass {
        id: ClassId,
        name: String,
        enabled: bool,
    },
    BindCurrentClass {
        id: ClassId,
    },
    PutSubject {
        id: SubjectId,
        name: String,
        enabled: bool,
        color: Rgb,
        /// None creates; Some requires exact existing revision.
        expected_revision: Option<Revision>,
    },
    CreateAccount(NewAccount),
    UpdateAccount {
        id: PrincipalId,
        display_name: String,
        role: Role,
        enabled: bool,
        scopes: Vec<AssignmentScope>,
    },
    SetDefaultTeacher(DefaultTeacher),
    /// Trusted credential processor only: password verification/preparation and this
    /// command must be bound to the same version and persisted in one transaction.
    PasswordChanged {
        id: PrincipalId,
        expected_credential_version: u64,
    },
    /// Trusted recovery processor only, after verifying the one-use recovery record.
    /// Replaces BOTH credential and recovery records atomically and invalidates all actors.
    RecoveryConsumed {
        id: PrincipalId,
        expected_recovery_version: u64,
    },
    /// Trusted credential processor only after re-verifying the current password.
    RecoveryRotated {
        id: PrincipalId,
        expected_recovery_version: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityEvent {
    Initialized,
    CatalogChanged,
    AccountChanged(PrincipalId),
    AuthorizationRevoked(PrincipalId),
    AllAuthorizationRevoked,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityTransition {
    pub next: IdentityState,
    pub events: Vec<IdentityEvent>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityError {
    Permission(PermissionError),
    Uninitialized,
    AlreadyInitialized,
    InvalidText,
    DuplicateName,
    DuplicateId,
    NotFound,
    InvalidScope,
    ScopeRequired,
    LastAdministrator,
    RevisionConflict,
    VersionOverflow,
    Capacity,
    InvalidState,
    PasswordChangeRequired,
    AmbiguousTeacher,
}
impl From<PermissionError> for IdentityError {
    fn from(value: PermissionError) -> Self {
        Self::Permission(value)
    }
}
fn next_version(v: u64) -> Result<u64, IdentityError> {
    v.checked_add(1).ok_or(IdentityError::VersionOverflow)
}
/// Locale-independent identity comparison: trim, collapse Unicode whitespace and
/// lowercase Unicode scalar values. The original bounded spelling is kept for display.
pub fn normalized_name(value: &str, max_chars: usize) -> Result<String, IdentityError> {
    if value.len() > max_chars.saturating_mul(4)
        || value.chars().count() > max_chars
        || value.chars().any(|c| c.is_control())
    {
        return Err(IdentityError::InvalidText);
    }
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if normalized.is_empty() {
        Err(IdentityError::InvalidText)
    } else {
        Ok(normalized)
    }
}
impl IdentityState {
    pub fn account(&self, id: PrincipalId) -> Result<&UserAccount, IdentityError> {
        self.accounts
            .iter()
            .find(|a| a.principal.id == id)
            .ok_or(IdentityError::NotFound)
    }
    pub fn account_by_login(&self, login: &str) -> Result<&UserAccount, IdentityError> {
        let key = normalized_name(login, 64)?;
        self.accounts
            .iter()
            .find(|a| normalized_name(&a.login_name, 64).as_ref() == Ok(&key))
            .ok_or(IdentityError::NotFound)
    }
    fn school(&self) -> Result<SchoolId, IdentityError> {
        self.installation
            .as_ref()
            .map(|s| s.school_id)
            .ok_or(IdentityError::Uninitialized)
    }
    pub fn authority(&self, id: PrincipalId) -> Result<AuthorityContext, IdentityError> {
        let account = self.account(id)?;
        Ok(AuthorityContext {
            principal: account.principal.clone(),
            installation_school: self.school()?,
            managed_classes: self
                .classes
                .iter()
                .filter(|c| c.enabled)
                .map(|c| c.id)
                .collect(),
            assignments: self
                .assignments
                .iter()
                .filter(|a| {
                    a.teacher_id == id
                        && a.enabled
                        && self
                            .subjects
                            .iter()
                            .any(|s| s.id == a.subject_id && s.enabled)
                        && self.classes.iter().any(|c| c.id == a.class_id && c.enabled)
                })
                .cloned()
                .collect(),
        })
    }
    pub fn authorize_management(
        &self,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<(), IdentityError> {
        self.authority(actor.principal_id)?
            .authorize_global_local(actor, now)?;
        if self.account(actor.principal_id)?.must_change_password {
            return Err(IdentityError::PasswordChangeRequired);
        }
        Ok(())
    }
    fn validate_scopes(
        &self,
        role: Role,
        scopes: &[AssignmentScope],
        require_active: bool,
    ) -> Result<(), IdentityError> {
        if scopes.len() > MAX_ASSIGNMENTS {
            return Err(IdentityError::Capacity);
        }
        if role == Role::TeacherAdmin && scopes.is_empty() {
            return Err(IdentityError::ScopeRequired);
        }
        if role == Role::User && !scopes.is_empty() {
            return Err(IdentityError::InvalidScope);
        }
        for (i, s) in scopes.iter().enumerate() {
            if scopes[..i].contains(s)
                || !self
                    .classes
                    .iter()
                    .any(|c| c.id == s.class_id && (!require_active || c.enabled))
                || !self
                    .subjects
                    .iter()
                    .any(|c| c.id == s.subject_id && (!require_active || c.enabled))
            {
                return Err(IdentityError::InvalidScope);
            }
        }
        Ok(())
    }
    fn revoke(
        &mut self,
        id: PrincipalId,
        events: &mut Vec<IdentityEvent>,
    ) -> Result<(), IdentityError> {
        let a = self
            .accounts
            .iter_mut()
            .find(|a| a.principal.id == id)
            .ok_or(IdentityError::NotFound)?;
        a.principal.auth_epoch = a
            .principal
            .auth_epoch
            .next()
            .map_err(|_| IdentityError::VersionOverflow)?;
        events.push(IdentityEvent::AuthorizationRevoked(id));
        Ok(())
    }
    /// Validate reconstructed state before exposing it. A missing/corrupt credential
    /// record must additionally fail closed in the storage adapter, never reset this state.
    pub fn validate(&self) -> Result<(), IdentityError> {
        if self.accounts.len() > MAX_ACCOUNTS
            || self.classes.len() > MAX_CLASSES
            || self.subjects.len() > MAX_SUBJECTS
            || self.assignments.len() > MAX_ASSIGNMENTS
            || self.default_teachers.len() > MAX_ASSIGNMENTS
        {
            return Err(IdentityError::Capacity);
        }
        let Some(installation) = &self.installation else {
            return if self.accounts.is_empty()
                && self.classes.is_empty()
                && self.subjects.is_empty()
                && self.assignments.is_empty()
                && self.default_teachers.is_empty()
                && self.revision == Revision::default()
            {
                Ok(())
            } else {
                Err(IdentityError::InvalidState)
            };
        };
        normalized_name(&installation.school_name, 128)?;
        if !self
            .accounts
            .iter()
            .any(|a| a.principal.enabled && a.principal.role == Role::GlobalAdmin)
        {
            return Err(IdentityError::LastAdministrator);
        }
        for (i, a) in self.accounts.iter().enumerate() {
            let key = normalized_name(&a.login_name, 64)?;
            normalized_name(&a.display_name, 128)?;
            if a.principal.school_id != installation.school_id
                || a.credential_version == 0
                || a.recovery_version == 0
                || a.principal.auth_epoch.get() == 0
            {
                return Err(IdentityError::InvalidState);
            }
            if self.accounts[..i].iter().any(|b| {
                a.principal.id == b.principal.id
                    || normalized_name(&b.login_name, 64).as_ref() == Ok(&key)
            }) {
                return Err(IdentityError::DuplicateName);
            }
            let scopes: Vec<_> = self
                .assignments
                .iter()
                .filter(|s| s.teacher_id == a.principal.id && s.enabled)
                .map(|s| AssignmentScope {
                    class_id: s.class_id,
                    subject_id: s.subject_id,
                })
                .collect();
            // Disabled catalog entries retain their stable relationships, but cannot authorize.
            if a.principal.role == Role::User && !scopes.is_empty() {
                return Err(IdentityError::InvalidScope);
            }
            if a.principal.role == Role::TeacherAdmin && scopes.is_empty() {
                return Err(IdentityError::ScopeRequired);
            }
        }
        for (i, c) in self.classes.iter().enumerate() {
            let key = normalized_name(&c.name, 128)?;
            if c.school_id != installation.school_id {
                return Err(IdentityError::InvalidState);
            }
            if self.classes[..i]
                .iter()
                .any(|d| c.id == d.id || normalized_name(&d.name, 128).as_ref() == Ok(&key))
            {
                return Err(IdentityError::DuplicateName);
            }
        }
        for (i, s) in self.subjects.iter().enumerate() {
            let key = normalized_name(&s.name, 64)?;
            if s.school_id != installation.school_id || s.revision.get() == 0 {
                return Err(IdentityError::InvalidState);
            }
            if self.subjects[..i]
                .iter()
                .any(|t| s.id == t.id || normalized_name(&t.name, 64).as_ref() == Ok(&key))
            {
                return Err(IdentityError::DuplicateName);
            }
        }
        if installation
            .current_class
            .is_some_and(|id| !self.classes.iter().any(|c| c.id == id && c.enabled))
        {
            return Err(IdentityError::InvalidScope);
        }
        for (i, s) in self.assignments.iter().enumerate() {
            let account = self.account(s.teacher_id)?;
            if account.principal.role == Role::User
                || !self.classes.iter().any(|c| c.id == s.class_id)
                || !self.subjects.iter().any(|c| c.id == s.subject_id)
                || self.assignments[..i].iter().any(|t| {
                    t.teacher_id == s.teacher_id
                        && t.class_id == s.class_id
                        && t.subject_id == s.subject_id
                })
            {
                return Err(IdentityError::InvalidScope);
            }
        }
        for (i, d) in self.default_teachers.iter().enumerate() {
            if self.default_teachers[..i]
                .iter()
                .any(|e| e.class_id == d.class_id && e.subject_id == d.subject_id)
                || !self.is_teacher(d.teacher_id, d.class_id, d.subject_id)
            {
                return Err(IdentityError::InvalidScope);
            }
        }
        Ok(())
    }
    pub fn is_teacher(&self, id: PrincipalId, class: ClassId, subject: SubjectId) -> bool {
        self.account(id)
            .is_ok_and(|a| a.principal.enabled && a.principal.role != Role::User)
            && self.classes.iter().any(|c| c.id == class && c.enabled)
            && self.subjects.iter().any(|s| s.id == subject && s.enabled)
            && self.assignments.iter().any(|a| {
                a.teacher_id == id && a.class_id == class && a.subject_id == subject && a.enabled
            })
    }
    /// Single candidate may be filled automatically; multiple candidates require an
    /// explicit stable teacher ID (or an administrator-selected default).
    pub fn resolve_teacher(
        &self,
        class: ClassId,
        subject: SubjectId,
        explicit: Option<PrincipalId>,
    ) -> Result<PrincipalId, IdentityError> {
        if let Some(id) = explicit {
            return if self.is_teacher(id, class, subject) {
                Ok(id)
            } else {
                Err(IdentityError::InvalidScope)
            };
        }
        if let Some(d) = self
            .default_teachers
            .iter()
            .find(|d| d.class_id == class && d.subject_id == subject)
        {
            return if self.is_teacher(d.teacher_id, class, subject) {
                Ok(d.teacher_id)
            } else {
                Err(IdentityError::InvalidScope)
            };
        }
        let candidates: Vec<_> = self
            .accounts
            .iter()
            .filter(|a| self.is_teacher(a.principal.id, class, subject))
            .map(|a| a.principal.id)
            .take(2)
            .collect();
        match candidates.as_slice() {
            [id] => Ok(*id),
            [] => Err(IdentityError::NotFound),
            _ => Err(IdentityError::AmbiguousTeacher),
        }
    }
    /// Pure transaction preparation. Adapters must call again against the latest writer
    /// state, then atomically persist `next`, credential records and command outcome.
    pub fn transition(
        &self,
        command: &IdentityCommand,
        actor: Option<&ActorContext>,
        now: LocalDateTime,
    ) -> Result<IdentityTransition, IdentityError> {
        self.validate()?;
        if !matches!(
            command,
            IdentityCommand::FirstAdministrator { .. } | IdentityCommand::RecoveryConsumed { .. }
        ) {
            let actor = actor.ok_or(IdentityError::Permission(PermissionError::RoleDenied))?;
            match command {
                IdentityCommand::PasswordChanged { id, .. }
                | IdentityCommand::RecoveryRotated { id, .. } => {
                    self.authority(actor.principal_id)?
                        .validate_actor(actor, now)?;
                    if *id != actor.principal_id {
                        return Err(IdentityError::Permission(PermissionError::WrongOwner));
                    }
                }
                _ => self.authorize_management(actor, now)?,
            }
        }
        let mut next = self.clone();
        let mut events = Vec::new();
        match command {
            IdentityCommand::FirstAdministrator {
                school_id,
                school_name,
                id,
                login_name,
                display_name,
            } => {
                if self.installation.is_some() {
                    return Err(IdentityError::AlreadyInitialized);
                }
                normalized_name(school_name, 128)?;
                normalized_name(login_name, 64)?;
                normalized_name(display_name, 128)?;
                next.installation = Some(Installation {
                    school_id: *school_id,
                    school_name: school_name.clone(),
                    current_class: None,
                });
                next.accounts.push(UserAccount {
                    principal: Principal {
                        id: *id,
                        school_id: *school_id,
                        role: Role::GlobalAdmin,
                        enabled: true,
                        auth_epoch: AuthEpoch::new(1),
                    },
                    login_name: login_name.clone(),
                    display_name: display_name.clone(),
                    must_change_password: false,
                    credential_version: 1,
                    recovery_version: 1,
                });
                events.push(IdentityEvent::Initialized);
            }
            IdentityCommand::CreateClass { id, name } => {
                normalized_name(name, 128)?;
                if next.classes.iter().any(|c| c.id == *id) {
                    return Err(IdentityError::DuplicateId);
                }
                next.classes.push(SchoolClass {
                    id: *id,
                    school_id: self.school()?,
                    name: name.clone(),
                    enabled: true,
                });
                events.push(IdentityEvent::CatalogChanged);
            }
            IdentityCommand::RenameSchool { name } => {
                normalized_name(name, 128)?;
                next.installation
                    .as_mut()
                    .ok_or(IdentityError::Uninitialized)?
                    .school_name = name.clone();
                events.push(IdentityEvent::CatalogChanged);
            }
            IdentityCommand::UpdateClass { id, name, enabled } => {
                normalized_name(name, 128)?;
                if !enabled
                    && next
                        .installation
                        .as_ref()
                        .is_some_and(|i| i.current_class == Some(*id))
                {
                    return Err(IdentityError::InvalidScope);
                }
                let class = next
                    .classes
                    .iter_mut()
                    .find(|c| c.id == *id)
                    .ok_or(IdentityError::NotFound)?;
                let changed = class.enabled != *enabled;
                class.name = name.clone();
                class.enabled = *enabled;
                if changed {
                    let ids: Vec<_> = next
                        .assignments
                        .iter()
                        .filter(|a| a.class_id == *id && a.enabled)
                        .map(|a| a.teacher_id)
                        .collect();
                    let mut seen = Vec::new();
                    for id in ids {
                        if !seen.contains(&id) {
                            next.revoke(id, &mut events)?;
                            seen.push(id);
                        }
                    }
                    if !enabled {
                        next.default_teachers.retain(|d| d.class_id != *id);
                    }
                }
                events.push(IdentityEvent::CatalogChanged);
            }
            IdentityCommand::BindCurrentClass { id } => {
                if !next.classes.iter().any(|c| c.id == *id && c.enabled) {
                    return Err(IdentityError::InvalidScope);
                }
                next.installation
                    .as_mut()
                    .ok_or(IdentityError::Uninitialized)?
                    .current_class = Some(*id);
                events.push(IdentityEvent::CatalogChanged);
            }
            IdentityCommand::PutSubject {
                id,
                name,
                enabled,
                color,
                expected_revision,
            } => {
                normalized_name(name, 64)?;
                let existing = next.subjects.iter().position(|s| s.id == *id);
                match (existing, expected_revision) {
                    (Some(index), Some(expected)) if next.subjects[index].revision == *expected => {
                        let old = next.subjects[index].clone();
                        next.subjects[index] = SchoolSubject {
                            id: *id,
                            school_id: self.school()?,
                            name: name.clone(),
                            enabled: *enabled,
                            color: *color,
                            revision: old
                                .revision
                                .next()
                                .map_err(|_| IdentityError::VersionOverflow)?,
                        };
                        if old.enabled != *enabled {
                            let ids: Vec<_> = next
                                .assignments
                                .iter()
                                .filter(|a| a.subject_id == *id && a.enabled)
                                .map(|a| a.teacher_id)
                                .collect();
                            let mut seen = Vec::new();
                            for id in ids {
                                if !seen.contains(&id) {
                                    next.revoke(id, &mut events)?;
                                    seen.push(id);
                                }
                            }
                            if !enabled {
                                next.default_teachers.retain(|d| d.subject_id != *id);
                            }
                        }
                    }
                    (None, None) => next.subjects.push(SchoolSubject {
                        id: *id,
                        school_id: self.school()?,
                        name: name.clone(),
                        enabled: *enabled,
                        color: *color,
                        revision: Revision::new(1),
                    }),
                    _ => return Err(IdentityError::RevisionConflict),
                }
                events.push(IdentityEvent::CatalogChanged);
            }
            IdentityCommand::CreateAccount(spec) => {
                self.validate_scopes(spec.role, &spec.scopes, true)?;
                normalized_name(&spec.login_name, 64)?;
                normalized_name(&spec.display_name, 128)?;
                if self.accounts.iter().any(|a| a.principal.id == spec.id) {
                    return Err(IdentityError::DuplicateId);
                }
                next.accounts.push(UserAccount {
                    principal: Principal {
                        id: spec.id,
                        school_id: self.school()?,
                        role: spec.role,
                        enabled: true,
                        auth_epoch: AuthEpoch::new(1),
                    },
                    login_name: spec.login_name.clone(),
                    display_name: spec.display_name.clone(),
                    must_change_password: true,
                    credential_version: 1,
                    recovery_version: 1,
                });
                next.assignments
                    .extend(spec.scopes.iter().map(|s| TeachingAssignment {
                        teacher_id: spec.id,
                        class_id: s.class_id,
                        subject_id: s.subject_id,
                        enabled: true,
                    }));
                events.push(IdentityEvent::AccountChanged(spec.id));
            }
            IdentityCommand::UpdateAccount {
                id,
                display_name,
                role,
                enabled,
                scopes,
            } => {
                normalized_name(display_name, 128)?;
                self.validate_scopes(*role, scopes, *enabled)?;
                let old = self.account(*id)?;
                if old.principal.role == Role::GlobalAdmin
                    && old.principal.enabled
                    && (*role != Role::GlobalAdmin || !enabled)
                    && !self.accounts.iter().any(|a| {
                        a.principal.id != *id
                            && a.principal.enabled
                            && a.principal.role == Role::GlobalAdmin
                    })
                {
                    return Err(IdentityError::LastAdministrator);
                }
                let a = next
                    .accounts
                    .iter_mut()
                    .find(|a| a.principal.id == *id)
                    .ok_or(IdentityError::NotFound)?;
                a.display_name = display_name.clone();
                a.principal.role = *role;
                a.principal.enabled = *enabled;
                next.assignments.retain(|s| s.teacher_id != *id);
                next.assignments
                    .extend(scopes.iter().map(|s| TeachingAssignment {
                        teacher_id: *id,
                        class_id: s.class_id,
                        subject_id: s.subject_id,
                        enabled: true,
                    }));
                // Revoke even when only display data changes: queued admin commands cannot
                // accidentally retain authority across an account-management transaction.
                next.revoke(*id, &mut events)?;
                next.default_teachers.retain(|d| {
                    d.teacher_id != *id
                        || (*enabled
                            && scopes
                                .iter()
                                .any(|s| s.class_id == d.class_id && s.subject_id == d.subject_id))
                });
                events.push(IdentityEvent::AccountChanged(*id));
            }
            IdentityCommand::SetDefaultTeacher(default) => {
                if !self.is_teacher(default.teacher_id, default.class_id, default.subject_id) {
                    return Err(IdentityError::InvalidScope);
                }
                next.default_teachers.retain(|d| {
                    d.class_id != default.class_id || d.subject_id != default.subject_id
                });
                next.default_teachers.push(*default);
                events.push(IdentityEvent::CatalogChanged);
            }
            IdentityCommand::PasswordChanged {
                id,
                expected_credential_version,
            } => {
                let a = next
                    .accounts
                    .iter_mut()
                    .find(|a| a.principal.id == *id)
                    .ok_or(IdentityError::NotFound)?;
                if a.credential_version != *expected_credential_version {
                    return Err(IdentityError::RevisionConflict);
                }
                a.credential_version = next_version(a.credential_version)?;
                a.must_change_password = false;
                next.revoke(*id, &mut events)?;
            }
            IdentityCommand::RecoveryRotated {
                id,
                expected_recovery_version,
            } => {
                let a = next
                    .accounts
                    .iter_mut()
                    .find(|a| a.principal.id == *id)
                    .ok_or(IdentityError::NotFound)?;
                if a.recovery_version != *expected_recovery_version {
                    return Err(IdentityError::RevisionConflict);
                }
                a.recovery_version = next_version(a.recovery_version)?;
                next.revoke(*id, &mut events)?;
            }
            IdentityCommand::RecoveryConsumed {
                id,
                expected_recovery_version,
            } => {
                let a = next
                    .accounts
                    .iter_mut()
                    .find(|a| a.principal.id == *id)
                    .ok_or(IdentityError::NotFound)?;
                if !a.principal.enabled {
                    return Err(IdentityError::Permission(PermissionError::Disabled));
                }
                if a.recovery_version != *expected_recovery_version {
                    return Err(IdentityError::RevisionConflict);
                }
                a.recovery_version = next_version(a.recovery_version)?;
                a.credential_version = next_version(a.credential_version)?;
                a.must_change_password = false;
                let ids: Vec<_> = next.accounts.iter().map(|a| a.principal.id).collect();
                for id in ids {
                    next.revoke(id, &mut events)?;
                }
                events.push(IdentityEvent::AllAuthorizationRevoked);
            }
        }
        next.revision = self
            .revision
            .next()
            .map_err(|_| IdentityError::VersionOverflow)?;
        next.validate()?;
        Ok(IdentityTransition { next, events })
    }
}
