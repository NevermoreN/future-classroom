use crate::{ids::*, time::LocalDateTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    GlobalAdmin,
    TeacherAdmin,
    User,
}

/// Constructed by a trusted native/Web transport after authentication. Never deserialize
/// this type from a request body; origin is transport evidence, not client input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustedOrigin {
    Local,
    Web,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorContext {
    pub principal_id: PrincipalId,
    pub school_id: SchoolId,
    pub auth_epoch: AuthEpoch,
    pub expires_at: LocalDateTime,
    pub origin: TrustedOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub id: PrincipalId,
    pub school_id: SchoolId,
    pub role: Role,
    pub enabled: bool,
    pub auth_epoch: AuthEpoch,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeachingAssignment {
    pub teacher_id: PrincipalId,
    pub class_id: ClassId,
    pub subject_id: SubjectId,
    pub enabled: bool,
}
/// Read with the content row in the writer transaction, never from a stale UI cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityContext {
    pub principal: Principal,
    pub installation_school: SchoolId,
    pub managed_classes: Vec<ClassId>,
    pub assignments: Vec<TeachingAssignment>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionError {
    Expired,
    Disabled,
    StaleAuthorization,
    WrongSchool,
    UnmanagedClass,
    RoleDenied,
    WrongOwner,
    OutsideTeachingScope,
    WebOnly,
    LocalOnly,
}

impl AuthorityContext {
    pub fn validate_actor(
        &self,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<(), PermissionError> {
        if now >= actor.expires_at {
            return Err(PermissionError::Expired);
        }
        if !self.principal.enabled {
            return Err(PermissionError::Disabled);
        }
        if self.principal.id != actor.principal_id || self.principal.auth_epoch != actor.auth_epoch
        {
            return Err(PermissionError::StaleAuthorization);
        }
        if actor.school_id != self.installation_school
            || self.principal.school_id != self.installation_school
        {
            return Err(PermissionError::WrongSchool);
        }
        Ok(())
    }
    pub fn require_class(&self, class: ClassId) -> Result<(), PermissionError> {
        if self.managed_classes.contains(&class) {
            Ok(())
        } else {
            Err(PermissionError::UnmanagedClass)
        }
    }
    pub fn authorize_owned_content(
        &self,
        actor: &ActorContext,
        now: LocalDateTime,
        class: ClassId,
        subject: SubjectId,
        owner: PrincipalId,
    ) -> Result<(), PermissionError> {
        self.validate_actor(actor, now)?;
        self.require_class(class)?;
        match self.principal.role {
            Role::GlobalAdmin => Ok(()),
            Role::User => Err(PermissionError::RoleDenied),
            Role::TeacherAdmin => {
                if owner != actor.principal_id {
                    return Err(PermissionError::WrongOwner);
                }
                if self.assignments.iter().any(|a| {
                    a.enabled
                        && a.teacher_id == owner
                        && a.class_id == class
                        && a.subject_id == subject
                }) {
                    Ok(())
                } else {
                    Err(PermissionError::OutsideTeachingScope)
                }
            }
        }
    }
    pub fn authorize_shared_reminder(
        &self,
        actor: &ActorContext,
        now: LocalDateTime,
        class: ClassId,
    ) -> Result<(), PermissionError> {
        self.validate_actor(actor, now)?;
        self.require_class(class)?;
        if actor.origin != TrustedOrigin::Web {
            return Err(PermissionError::WebOnly);
        }
        match self.principal.role {
            Role::GlobalAdmin | Role::TeacherAdmin => Ok(()),
            Role::User => Err(PermissionError::RoleDenied),
        }
    }
    pub fn authorize_global_local(
        &self,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<(), PermissionError> {
        self.validate_actor(actor, now)?;
        if self.principal.role != Role::GlobalAdmin {
            return Err(PermissionError::RoleDenied);
        }
        if actor.origin != TrustedOrigin::Local {
            return Err(PermissionError::LocalOnly);
        }
        Ok(())
    }
}
