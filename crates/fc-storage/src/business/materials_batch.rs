//! One ordered batch, one Writer COMMIT, one original command pair. Its receipt
//! is independent of the historical single-file format and of current items.
use super::*;
use fc_ports::{
    MaterialBatchAppendRequest, MaterialBatchCommandOutcome, MaterialBatchDeleteCommandOutcome,
    MaterialBatchDeleteOutcomeRepository, MaterialBatchDeleteRepository,
    MaterialBatchDeleteRequest, MaterialBatchOutcomeRepository, MaterialBatchRepository,
    PreparedMaterialBatchAppend,
};

/// Only checks for the opposite fixed receipt kind. Reading its exact namespace
/// avoids treating an absent alternative slot as damage to a recorded C2 entry.
/// The selected kind still goes through the normal checked CommandView read.
pub(super) fn reject_other_receipt(
    d: &dyn TransactionDocuments,
    kind: &'static str,
    id: CommandId,
) -> Result<(), StorageError> {
    let legacy = key(kind, id.get());
    let actual = if let Some(issued) = d.active_command() {
        let (slot, receipt_id) =
            super::super::command_window::legacy(&legacy).ok_or(StorageError::InvalidCommand)?;
        if receipt_id != issued.command_id || slot.family != issued.family {
            return Err(StorageError::CommandIdReused);
        }
        super::super::command_window::namespaced(slot, issued.ticket)
    } else {
        legacy
    };
    if d.get(&actual)?.is_some() {
        return Err(StorageError::CommandIdReused);
    }
    Ok(())
}
fn input(scope: MaterialScope, appends: &[MaterialAppend]) -> Value {
    json!({"operation":"append-batch","scope":dto::scope(scope),"appends":appends.iter().map(|a|json!({
        "material":a.material_id.get(),"name":a.display_name,"resource":dto::resource(&a.resource),
        "classes":a.allowed_class_ids.iter().map(|c|c.get()).collect::<Vec<_>>()
    })).collect::<Vec<_>>()})
}
fn fingerprint(
    scope: MaterialScope,
    appends: &[MaterialAppend],
    actor: &ActorContext,
) -> Result<Vec<u8>, StorageError> {
    let mut hash = Sha256::new();
    hash.update(b"future-classroom/material-batch/v1\0");
    hash.update(codec::bytes(&codec::actor(actor))?);
    hash.update(codec::bytes(&input(scope, appends))?);
    Ok(hash.finalize().to_vec())
}
fn records_value(records: &[MaterialRecord]) -> Value {
    Value::Array(records.iter().map(dto::record).collect())
}
fn records(v: &Value) -> Result<Vec<MaterialRecord>, StorageError> {
    let array = v.as_array().ok_or(StorageError::Database)?;
    if array.is_empty() || array.len() > MAX_MATERIAL_BATCH_FILES {
        return Err(StorageError::Database);
    }
    let records = array
        .iter()
        .map(dto::read_record)
        .collect::<Result<Vec<_>, _>>()?;
    let first = &records[0];
    let mut ids = std::collections::HashSet::new();
    let mut versions = std::collections::HashSet::new();
    for (index, record) in records.iter().enumerate() {
        if record.folder_id != first.folder_id
            || record.added_at != first.added_at
            || record.added_sequence
                != first
                    .added_sequence
                    .checked_add(index as u64)
                    .ok_or(StorageError::Database)?
            || record.status != MaterialStatus::Published
            || record.revision != Revision::new(1)
            || !ids.insert(record.id)
            || !versions.insert(record.resource.version_id)
        {
            return Err(StorageError::Database);
        }
    }
    Ok(records)
}
fn read_receipt(
    v: &Value,
    actor: &ActorContext,
) -> Result<(MaterialScope, Vec<MaterialRecord>, Vec<u8>), StorageError> {
    codec::object_fields(
        v,
        &[
            "format",
            "actor",
            "scope",
            "fingerprint",
            "records",
            "result_digest",
        ],
    )?;
    if codec::number(v, "format")? != 1 || v["actor"] != codec::actor(actor) {
        return Err(StorageError::CommandIdReused);
    }
    let scope = dto::read_scope(&v["scope"])?;
    let records = records(&v["records"])?;
    let digest: Vec<u8> =
        serde_json::from_value(v["fingerprint"].clone()).map_err(|_| StorageError::Database)?;
    let result_digest: Vec<u8> =
        serde_json::from_value(v["result_digest"].clone()).map_err(|_| StorageError::Database)?;
    let appends = records
        .iter()
        .map(|r| MaterialAppend {
            material_id: r.id,
            display_name: r.display_name.clone(),
            resource: r.resource.clone(),
            allowed_class_ids: r.allowed_class_ids.clone(),
        })
        .collect::<Vec<_>>();
    if digest.len() != 32
        || digest != fingerprint(scope, &appends, actor)?
        || result_digest.len() != 32
        || result_digest.as_slice() != Sha256::digest(encode(&v["records"])?).as_slice()
    {
        return Err(StorageError::Database);
    }
    Ok((scope, records, digest))
}
struct AppendTask<S> {
    request: MaterialBatchAppendRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for AppendTask<S> {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((
            fc_ports::command_window::CommandFamily::Materials,
            self.request.command_id,
        ))
    }
    fn allocation_bytes(&self) -> usize {
        self.request
            .allocation_bytes()
            .and_then(|n| n.checked_add(std::mem::size_of::<Self>()))
            .unwrap_or(usize::MAX)
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let scope = self.request.scope;
        let state = load_identity(d)?;
        authorize(
            &state,
            &self.actor,
            scope,
            self.clock.as_ref(),
            &self.session,
        )?;
        if self.request.appends.is_empty() || self.request.appends.len() > MAX_MATERIAL_BATCH_FILES
        {
            return Err(material_error(MaterialError::Capacity));
        }
        let digest = fingerprint(scope, &self.request.appends, &self.actor)?;
        reject_other_receipt(d, "receipts", self.request.command_id)?;
        reject_other_receipt(d, "web-outcomes", self.request.command_id)?;
        reject_other_receipt(d, "delete-batch-receipts", self.request.command_id)?;
        let journal = key("batch-receipts", self.request.command_id.get());
        if let Some(prior) = read(d, &journal)? {
            let (prior_scope, results, prior_digest) = read_receipt(&prior, &self.actor)?;
            if prior_scope != scope || prior_digest != digest {
                return Err(StorageError::CommandIdReused);
            }
            let folder = find_folder(d, scope)?.ok_or(StorageError::Database)?;
            if results[0].folder_id != folder.id {
                return Err(StorageError::Database);
            }
            authorize(
                &state,
                &self.actor,
                scope,
                self.clock.as_ref(),
                &self.session,
            )?;
            return Ok(TransactionDecision {
                outcome: encode(&records_value(&results))?,
                commit: None,
                precommit: None,
            });
        }
        let mut catalog = Catalog::read(d)?;
        let current = match find_folder(d, scope)? {
            Some(folder) => load_state(d, folder)?,
            None => new_folder(&mut catalog, scope)?,
        };
        // Domain checks every member and all permissions before constructing the
        // new state. One folder revision publishes all members in this COMMIT.
        let next = current
            .append_batch(&self.request.appends, &state, &self.actor, self.clock.now())
            .map_err(material_error)?;
        let count = self.request.appends.len() as u64;
        catalog.records = catalog
            .records
            .checked_add(count)
            .filter(|n| *n <= MAX_RECORDS)
            .ok_or(material_error(MaterialError::Capacity))?;
        catalog.resources = catalog
            .resources
            .checked_add(count)
            .filter(|n| *n <= MAX_RESOURCES)
            .ok_or(material_error(MaterialError::Capacity))?;
        let mut bytes = 0u64;
        for append in &self.request.appends {
            if d.get(&key("items", append.material_id.get()))?.is_some()
                || d.get(&key("resources", append.resource.version_id.get()))?
                    .is_some()
            {
                return Err(material_error(MaterialError::AlreadyExists));
            }
            bytes = bytes
                .checked_add(append.resource.bytes)
                .ok_or(StorageError::ResourceQuota)?;
        }
        let content_bytes = super::super::profile_content::primary_bytes(d)?;
        catalog.bytes = catalog
            .bytes
            .checked_add(bytes)
            .filter(|n| {
                n.checked_add(content_bytes)
                    .is_some_and(|total| total <= ORIGINAL_BYTE_LIMIT)
            })
            .ok_or(StorageError::ResourceQuota)?;
        let results = next
            .records
            .get(current.records.len()..)
            .ok_or(StorageError::Database)?
            .to_vec();
        if results.len() != self.request.appends.len() {
            return Err(StorageError::Database);
        }
        let result_value = records_value(&results);
        // Both the response and persisted receipt keep the existing 64 KiB limit.
        let outcome = encode(&result_value)?;
        let mut mutations = folder_mutations(&next.folder)?;
        for result in &results {
            let entry = ResourceInventory {
                resource: result.resource.clone(),
                scope,
                material_id: result.id,
                revision: Revision::new(1),
                references: 1,
                state: ResourceState::Referenced,
            };
            mutations.extend([
                put(key("items", result.id.get()), dto::record(result))?,
                put(
                    member_key(result.folder_id, result.id),
                    json!({"id":result.id.get()}),
                )?,
                put(
                    key("resources", entry.resource.version_id.get()),
                    inventory(&entry),
                )?,
                put(index_key(result), json!({"id":result.id.get()}))?,
            ]);
        }
        mutations.push(catalog.mutation()?);
        mutations.push(put(journal, json!({"format":1,"actor":codec::actor(&self.actor),"scope":dto::scope(scope),
            "fingerprint":digest,"records":result_value,"result_digest":Sha256::digest(&outcome).to_vec()}))?);
        let snapshot = (self.factory)(
            &state,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let actor = self.actor;
        let clock = self.clock;
        let guard = self.session;
        let appends = self.request.appends;
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                authorize(&state, &actor, scope, clock.as_ref(), &guard)?;
                current
                    .append_batch(&appends, &state, &actor, clock.now())
                    .map(|_| ())
                    .map_err(material_error)
            })),
        })
    }
}
struct OutcomeTask {
    id: CommandId,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for OutcomeTask {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((fc_ports::command_window::CommandFamily::Materials, self.id))
    }
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        // Fresh session and role checks also apply to an unknown result; expiry
        // and owner matching are checked by the original Writer C2 view first.
        let check = || -> Result<(), StorageError> {
            (self.session)(&state, &self.actor)?;
            let authority = state
                .authority(self.actor.principal_id)
                .map_err(StorageError::Identity)?;
            authority
                .validate_actor(&self.actor, self.clock.now())
                .map_err(|e| material_error(MaterialError::Permission(e)))?;
            let account = state
                .account(self.actor.principal_id)
                .map_err(StorageError::Identity)?;
            if account.must_change_password {
                return Err(material_error(MaterialError::Identity(
                    IdentityError::PasswordChangeRequired,
                )));
            }
            if !matches!(
                account.principal.role,
                Role::GlobalAdmin | Role::TeacherAdmin
            ) {
                return Err(material_error(MaterialError::Permission(
                    PermissionError::RoleDenied,
                )));
            }
            Ok(())
        };
        check()?;
        reject_other_receipt(d, "receipts", self.id)?;
        reject_other_receipt(d, "web-outcomes", self.id)?;
        reject_other_receipt(d, "delete-batch-receipts", self.id)?;
        let value = match read(d, &key("batch-receipts", self.id.get()))? {
            None => Value::Null,
            Some(v) => {
                let (scope, results, _) = read_receipt(&v, &self.actor)?;
                authorize(
                    &state,
                    &self.actor,
                    scope,
                    self.clock.as_ref(),
                    &self.session,
                )?;
                let folder = find_folder(d, scope)?.ok_or(StorageError::Database)?;
                if results[0].folder_id != folder.id {
                    return Err(StorageError::Database);
                }
                json!({"scope":dto::scope(scope),"records":records_value(&results)})
            }
        };
        check()?;
        Ok(TransactionDecision {
            outcome: encode(&value)?,
            commit: None,
            precommit: None,
        })
    }
}
impl<S: Send + 'static> MaterialBatchRepository for BusinessRepository<S> {
    fn append_material_batch(
        &mut self,
        prepared: PreparedMaterialBatchAppend,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<Vec<MaterialRecord>, MaterialRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(AppendTask {
                request: prepared.into_request(),
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        records(&codec::value(&bytes).map_err(repository_error)?).map_err(repository_error)
    }
}
impl<S: Send + 'static> MaterialBatchOutcomeRepository for BusinessRepository<S> {
    fn material_batch_outcome(
        &self,
        command_id: CommandId,
        actor: &ActorContext,
    ) -> Result<Option<MaterialBatchCommandOutcome>, MaterialRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(OutcomeTask {
                id: command_id,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        let value = codec::value(&bytes).map_err(repository_error)?;
        if value.is_null() {
            return Ok(None);
        }
        codec::object_fields(&value, &["scope", "records"]).map_err(repository_error)?;
        Ok(Some(MaterialBatchCommandOutcome {
            command_id,
            scope: dto::read_scope(&value["scope"]).map_err(repository_error)?,
            records: records(&value["records"]).map_err(repository_error)?,
        }))
    }
}

fn delete_input(scope: MaterialScope, targets: &[MaterialDeleteTarget]) -> Value {
    json!({"operation":"delete-batch","scope":dto::scope(scope),"targets":targets.iter().map(|t|json!({
        "material":t.material_id.get(),"version":t.resource_version_id.get(),
        "revision":t.expected_material_revision.get()
    })).collect::<Vec<_>>()})
}
fn delete_fingerprint(
    scope: MaterialScope,
    targets: &[MaterialDeleteTarget],
    actor: &ActorContext,
) -> Result<Vec<u8>, StorageError> {
    let mut hash = Sha256::new();
    hash.update(b"future-classroom/material-delete-batch/v1\0");
    hash.update(codec::bytes(&codec::actor(actor))?);
    hash.update(codec::bytes(&delete_input(scope, targets))?);
    Ok(hash.finalize().to_vec())
}
fn deleted_records(v: &Value) -> Result<Vec<MaterialRecord>, StorageError> {
    let array = v.as_array().ok_or(StorageError::Database)?;
    if array.is_empty() || array.len() > MAX_MATERIAL_BATCH_FILES {
        return Err(StorageError::Database);
    }
    let records = array
        .iter()
        .map(dto::read_record)
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids = std::collections::HashSet::new();
    let mut versions = std::collections::HashSet::new();
    for record in &records {
        if record.folder_id != records[0].folder_id
            || record.status != MaterialStatus::Deleted
            || record.revision.get() < 2
            || !ids.insert(record.id)
            || !versions.insert(record.resource.version_id)
        {
            return Err(StorageError::Database);
        }
    }
    Ok(records)
}
fn read_delete_receipt(
    v: &Value,
    actor: &ActorContext,
) -> Result<(MaterialScope, Vec<MaterialRecord>, Vec<u8>), StorageError> {
    codec::object_fields(
        v,
        &[
            "format",
            "actor",
            "scope",
            "fingerprint",
            "records",
            "result_digest",
        ],
    )?;
    if codec::number(v, "format")? != 1 || v["actor"] != codec::actor(actor) {
        return Err(StorageError::CommandIdReused);
    }
    let scope = dto::read_scope(&v["scope"])?;
    let records = deleted_records(&v["records"])?;
    let targets = records
        .iter()
        .map(|r| MaterialDeleteTarget {
            material_id: r.id,
            resource_version_id: r.resource.version_id,
            expected_material_revision: Revision::new(r.revision.get() - 1),
        })
        .collect::<Vec<_>>();
    let digest: Vec<u8> =
        serde_json::from_value(v["fingerprint"].clone()).map_err(|_| StorageError::Database)?;
    let result_digest: Vec<u8> =
        serde_json::from_value(v["result_digest"].clone()).map_err(|_| StorageError::Database)?;
    if digest.len() != 32
        || digest != delete_fingerprint(scope, &targets, actor)?
        || result_digest.len() != 32
        || result_digest.as_slice() != Sha256::digest(encode(&v["records"])?).as_slice()
    {
        return Err(StorageError::Database);
    }
    Ok((scope, records, digest))
}
struct DeleteTask<S> {
    request: MaterialBatchDeleteRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for DeleteTask<S> {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((
            fc_ports::command_window::CommandFamily::Materials,
            self.request.command_id,
        ))
    }
    fn allocation_bytes(&self) -> usize {
        self.request
            .allocation_bytes()
            .and_then(|n| n.checked_add(std::mem::size_of::<Self>()))
            .unwrap_or(usize::MAX)
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let scope = self.request.scope;
        let state = load_identity(d)?;
        authorize(
            &state,
            &self.actor,
            scope,
            self.clock.as_ref(),
            &self.session,
        )?;
        // Public entry validates capacities before queue admission; the domain
        // repeats cardinality, uniqueness, versions and every item's authority.
        let digest = delete_fingerprint(scope, &self.request.targets, &self.actor)?;
        for kind in ["receipts", "web-outcomes", "batch-receipts"] {
            reject_other_receipt(d, kind, self.request.command_id)?;
        }
        let journal = key("delete-batch-receipts", self.request.command_id.get());
        if let Some(prior) = read(d, &journal)? {
            let (prior_scope, results, prior_digest) = read_delete_receipt(&prior, &self.actor)?;
            if prior_scope != scope || prior_digest != digest {
                return Err(StorageError::CommandIdReused);
            }
            let folder = find_folder(d, scope)?.ok_or(StorageError::Database)?;
            if results[0].folder_id != folder.id {
                return Err(StorageError::Database);
            }
            authorize(
                &state,
                &self.actor,
                scope,
                self.clock.as_ref(),
                &self.session,
            )?;
            return Ok(TransactionDecision {
                outcome: encode(&records_value(&results))?,
                commit: None,
                precommit: None,
            });
        }
        let folder = find_folder(d, scope)?.ok_or(material_error(MaterialError::NotFound))?;
        let current = load_state(d, folder)?;
        let next = current
            .delete_batch(&self.request.targets, &state, &self.actor, self.clock.now())
            .map_err(material_error)?;
        let results = self
            .request
            .targets
            .iter()
            .map(|target| {
                next.records
                    .iter()
                    .find(|r| r.id == target.material_id)
                    .cloned()
                    .ok_or(StorageError::Database)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let result_value = records_value(&results);
        let outcome = encode(&result_value)?;
        let mut mutations = folder_mutations(&next.folder)?;
        for result in &results {
            let mut entry = resource_entry(d, result.resource.version_id)?;
            if entry.resource != result.resource
                || entry.scope != scope
                || entry.material_id != result.id
                || entry.references != 1
                || entry.state != ResourceState::Referenced
            {
                return Err(StorageError::Database);
            }
            entry.references = 0;
            entry.state = ResourceState::Unreferenced;
            entry.revision = entry.revision.next().map_err(|_| StorageError::Database)?;
            mutations.extend([
                put(key("items", result.id.get()), dto::record(result))?,
                put(
                    key("resources", entry.resource.version_id.get()),
                    inventory(&entry),
                )?,
                Mutation::Delete {
                    key: index_key(result),
                },
            ]);
        }
        // Original bytes remain charged until the existing physical GC worker
        // retires, removes and finalizes each exact resource version.
        mutations.push(put(journal,json!({"format":1,"actor":codec::actor(&self.actor),"scope":dto::scope(scope),
            "fingerprint":digest,"records":result_value,"result_digest":Sha256::digest(&outcome).to_vec()}))?);
        let snapshot = (self.factory)(
            &state,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let actor = self.actor;
        let clock = self.clock;
        let guard = self.session;
        let targets = self.request.targets;
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                authorize(&state, &actor, scope, clock.as_ref(), &guard)?;
                current
                    .delete_batch(&targets, &state, &actor, clock.now())
                    .map(|_| ())
                    .map_err(material_error)
            })),
        })
    }
}
struct DeleteOutcomeTask {
    id: CommandId,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for DeleteOutcomeTask {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((fc_ports::command_window::CommandFamily::Materials, self.id))
    }
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        let check = || -> Result<(), StorageError> {
            (self.session)(&state, &self.actor)?;
            state
                .authority(self.actor.principal_id)
                .map_err(StorageError::Identity)?
                .validate_actor(&self.actor, self.clock.now())
                .map_err(|e| material_error(MaterialError::Permission(e)))?;
            let account = state
                .account(self.actor.principal_id)
                .map_err(StorageError::Identity)?;
            if account.must_change_password {
                return Err(material_error(MaterialError::Identity(
                    IdentityError::PasswordChangeRequired,
                )));
            }
            if !matches!(
                account.principal.role,
                Role::GlobalAdmin | Role::TeacherAdmin
            ) {
                return Err(material_error(MaterialError::Permission(
                    PermissionError::RoleDenied,
                )));
            }
            Ok(())
        };
        check()?;
        for kind in ["receipts", "web-outcomes", "batch-receipts"] {
            reject_other_receipt(d, kind, self.id)?;
        }
        let value = match read(d, &key("delete-batch-receipts", self.id.get()))? {
            None => Value::Null,
            Some(v) => {
                let (scope, results, _) = read_delete_receipt(&v, &self.actor)?;
                authorize(
                    &state,
                    &self.actor,
                    scope,
                    self.clock.as_ref(),
                    &self.session,
                )?;
                let folder = find_folder(d, scope)?.ok_or(StorageError::Database)?;
                if results[0].folder_id != folder.id {
                    return Err(StorageError::Database);
                }
                json!({"scope":dto::scope(scope),"records":records_value(&results)})
            }
        };
        check()?;
        Ok(TransactionDecision {
            outcome: encode(&value)?,
            commit: None,
            precommit: None,
        })
    }
}
impl<S: Send + 'static> MaterialBatchDeleteRepository for BusinessRepository<S> {
    fn delete_material_batch(
        &mut self,
        request: MaterialBatchDeleteRequest,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<Vec<MaterialRecord>, MaterialRepositoryError> {
        request.validate()?;
        let bytes = self
            .handle
            .try_transaction(DeleteTask {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        deleted_records(&codec::value(&bytes).map_err(repository_error)?).map_err(repository_error)
    }
}
impl<S: Send + 'static> MaterialBatchDeleteOutcomeRepository for BusinessRepository<S> {
    fn material_batch_delete_outcome(
        &self,
        command_id: CommandId,
        actor: &ActorContext,
    ) -> Result<Option<MaterialBatchDeleteCommandOutcome>, MaterialRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(DeleteOutcomeTask {
                id: command_id,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        let value = codec::value(&bytes).map_err(repository_error)?;
        if value.is_null() {
            return Ok(None);
        }
        codec::object_fields(&value, &["scope", "records"]).map_err(repository_error)?;
        Ok(Some(MaterialBatchDeleteCommandOutcome {
            command_id,
            scope: dto::read_scope(&value["scope"]).map_err(repository_error)?,
            records: deleted_records(&value["records"]).map_err(repository_error)?,
        }))
    }
}
