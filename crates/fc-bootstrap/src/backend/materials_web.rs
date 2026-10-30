//! Exact Web lease composition for material metadata and controlled file workers.
use super::*;
use fc_ports::{MaterialCommandOutcome, MaterialOutcomeRepository};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub struct WebMaterialClass {
    pub class_id: ClassId,
    pub class_name: String,
}
#[derive(Clone, Debug)]
pub struct WebMaterialScope {
    pub scope: MaterialScope,
    pub owner_display_name: String,
    pub subject_name: String,
    pub classes: Vec<WebMaterialClass>,
    pub class_total: usize,
}
#[derive(Clone, Debug)]
pub struct WebMaterialClasses {
    pub identity_revision: Revision,
    pub scope: MaterialScope,
    pub offset: usize,
    pub total: usize,
    pub items: Vec<WebMaterialClass>,
}
#[derive(Clone, Debug)]
pub struct WebMaterialScopes {
    pub identity_revision: Revision,
    pub offset: usize,
    pub total: usize,
    pub items: Vec<WebMaterialScope>,
}
fn invalid(e: MaterialError) -> BackendError {
    BackendError::Material(MaterialRepositoryError::Domain(e))
}
impl Backend {
    fn web_material_actor(
        &self,
        state: &IdentityState,
        lease: SessionLease,
    ) -> Result<(ActorContext, u64)> {
        if lease.origin() != TrustedOrigin::Web {
            return Err(invalid(MaterialError::Permission(PermissionError::WebOnly)));
        }
        let pair = self.actor(state, lease, SessionPurpose::Manage)?;
        let account = state
            .account(pair.0.principal_id)
            .map_err(|e| invalid(e.into()))?;
        if account.principal.role == Role::User {
            return Err(invalid(MaterialError::Permission(
                PermissionError::RoleDenied,
            )));
        }
        Ok(pair)
    }
    /// Revalidate the exact device, current identity, owner, subject and all classes.
    /// No activity/input lease is extended by preparation, polling or chunks.
    pub fn web_material_authorize(
        &self,
        lease: SessionLease,
        scope: MaterialScope,
        classes: &[ClassId],
    ) -> Result<()> {
        let state = self.state()?;
        let (actor, _) = self.web_material_actor(&state, lease)?;
        authorize_material_folder(&state, &actor, scope, self.clock.now()).map_err(invalid)?;
        if classes.len() > MAX_CLASSES
            || classes.iter().enumerate().any(|(i, c)| {
                classes[..i].contains(c)
                    || !state.is_teacher(scope.teacher_id, *c, scope.subject_id)
            })
        {
            return Err(invalid(MaterialError::InvalidScope));
        }
        self.web_material_actor(&state, lease)?;
        Ok(())
    }
    pub fn web_material_scopes(
        &self,
        lease: SessionLease,
        offset: usize,
    ) -> Result<WebMaterialScopes> {
        let state = self.state()?;
        let (actor, _) = self.web_material_actor(&state, lease)?;
        let global = state
            .account(actor.principal_id)
            .map_err(|e| invalid(e.into()))?
            .principal
            .role
            == Role::GlobalAdmin;
        let mut groups: BTreeMap<(PrincipalId, SubjectId), BTreeSet<ClassId>> = BTreeMap::new();
        for a in &state.assignments {
            if (global || a.teacher_id == actor.principal_id)
                && state.is_teacher(a.teacher_id, a.class_id, a.subject_id)
            {
                groups
                    .entry((a.teacher_id, a.subject_id))
                    .or_default()
                    .insert(a.class_id);
            }
        }
        let total = groups.len();
        if offset > total || offset > 4096 {
            return Err(BackendError::InvalidInput);
        }
        let mut items = Vec::new();
        for ((owner, subject), classes) in groups.into_iter().skip(offset).take(20) {
            let a = state.account(owner).map_err(|e| invalid(e.into()))?;
            let subject_name = state
                .subjects
                .iter()
                .find(|s| s.id == subject)
                .ok_or(BackendError::InvalidInput)?
                .name
                .clone();
            let class_total = classes.len();
            let classes = classes
                .into_iter()
                .take(20)
                .map(|c| {
                    Ok(WebMaterialClass {
                        class_id: c,
                        class_name: state
                            .classes
                            .iter()
                            .find(|v| v.id == c)
                            .ok_or(BackendError::InvalidInput)?
                            .name
                            .clone(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            items.push(WebMaterialScope {
                scope: MaterialScope {
                    teacher_id: owner,
                    subject_id: subject,
                },
                owner_display_name: a.display_name.clone(),
                subject_name,
                classes,
                class_total,
            });
        }
        let latest = self.state()?;
        self.web_material_actor(&latest, lease)?;
        if latest.revision != state.revision {
            return Err(BackendError::Material(MaterialRepositoryError::Unavailable));
        }
        Ok(WebMaterialScopes {
            identity_revision: state.revision,
            offset,
            total,
            items,
        })
    }
    pub fn web_material_classes(
        &self,
        lease: SessionLease,
        scope: MaterialScope,
        offset: usize,
    ) -> Result<WebMaterialClasses> {
        let state = self.state()?;
        let (actor, _) = self.web_material_actor(&state, lease)?;
        authorize_material_folder(&state, &actor, scope, self.clock.now()).map_err(invalid)?;
        let classes: BTreeSet<_> = state
            .assignments
            .iter()
            .filter(|a| {
                a.teacher_id == scope.teacher_id
                    && a.subject_id == scope.subject_id
                    && state.is_teacher(scope.teacher_id, a.class_id, scope.subject_id)
            })
            .map(|a| a.class_id)
            .collect();
        let total = classes.len();
        if offset > total || offset > MAX_CLASSES {
            return Err(BackendError::InvalidInput);
        }
        let items = classes
            .into_iter()
            .skip(offset)
            .take(20)
            .map(|id| {
                Ok(WebMaterialClass {
                    class_id: id,
                    class_name: state
                        .classes
                        .iter()
                        .find(|c| c.id == id)
                        .ok_or(BackendError::InvalidInput)?
                        .name
                        .clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let latest = self.state()?;
        self.web_material_actor(&latest, lease)?;
        if latest.revision != state.revision {
            return Err(BackendError::Material(MaterialRepositoryError::Unavailable));
        }
        Ok(WebMaterialClasses {
            identity_revision: state.revision,
            scope,
            offset,
            total,
            items,
        })
    }
    pub fn web_materials(
        &self,
        lease: SessionLease,
        request: MaterialListRequest,
    ) -> Result<MaterialPage> {
        self.web_material_authorize(lease, request.scope, &[])?;
        let page = self.materials(lease, request)?;
        self.web_material_authorize(lease, page.folder.scope, &[])?;
        Ok(page)
    }
    pub fn web_material_append(
        &mut self,
        lease: SessionLease,
        prepared: PreparedMaterialAppend,
    ) -> Result<MaterialRecord> {
        self.web_material_authorize(
            lease,
            prepared.request().scope,
            &prepared.request().append.allowed_class_ids,
        )?;
        let scope = prepared.request().scope;
        let record = self.append_material(lease, prepared)?;
        self.web_material_authorize(lease, scope, &record.allowed_class_ids)
            .map_err(|_| BackendError::Material(MaterialRepositoryError::Unavailable))?;
        Ok(record)
    }
    pub fn web_material_delete(
        &mut self,
        lease: SessionLease,
        request: MaterialDeleteRequest,
        version: ResourceVersionId,
    ) -> Result<MaterialRecord> {
        self.web_material_authorize(lease, request.scope, &[])?;
        if let Some(prior) = self.web_material_outcome(lease, request.command_id)? {
            if prior.kind == fc_ports::MaterialCommandKind::Delete
                && prior.scope == request.scope
                && prior.record.id == request.material_id
                && prior.record.resource.version_id == version
                && Some(prior.record.revision) == request.expected_material_revision.next().ok()
            {
                return Ok(prior.record);
            }
            return Err(BackendError::Material(
                MaterialRepositoryError::IdempotencyConflict,
            ));
        }
        let state = self.state()?;
        let (actor, generation) = self.web_material_actor(&state, lease)?;
        self.guarded(lease, generation, SessionPurpose::Manage)
            .managed_material(request.scope, request.material_id, version, &actor)?;
        let scope = request.scope;
        let record = self.delete_material(lease, request)?;
        self.web_material_authorize(lease, scope, &[])
            .map_err(|_| BackendError::Material(MaterialRepositoryError::Unavailable))?;
        Ok(record)
    }
    pub fn web_material_outcome(
        &self,
        lease: SessionLease,
        command: CommandId,
    ) -> Result<Option<MaterialCommandOutcome>> {
        let state = self.state()?;
        let (actor, generation) = self.web_material_actor(&state, lease)?;
        let result = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .material_outcome(command, &actor)?;
        if let Some(result) = &result {
            self.web_material_authorize(lease, result.scope, &[])?;
        } else {
            self.web_material_actor(&self.state()?, lease)?;
        }
        Ok(result)
    }
    /// Allocated by the trusted server, never from a caller path/name.
    pub fn web_material_identifiers(
        &mut self,
        lease: SessionLease,
        scope: MaterialScope,
    ) -> Result<(u64, MaterialId, ResourceVersionId)> {
        self.web_material_authorize(lease, scope, &[])?;
        Ok((
            self.command_id()?.get(),
            MaterialId::new(self.command_id()?.get()).map_err(|_| BackendError::Overflow)?,
            ResourceVersionId::new(self.command_id()?.get()).map_err(|_| BackendError::Overflow)?,
        ))
    }
    pub fn web_material_download_record(
        &self,
        lease: SessionLease,
        scope: MaterialScope,
        id: MaterialId,
        version: ResourceVersionId,
    ) -> Result<MaterialRecord> {
        self.web_material_authorize(lease, scope, &[])?;
        let state = self.state()?;
        let (actor, generation) = self.web_material_actor(&state, lease)?;
        let record = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .managed_material(scope, id, version, &actor)?;
        self.web_material_authorize(lease, scope, &[])?;
        Ok(record)
    }
}
