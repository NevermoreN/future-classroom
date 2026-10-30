//! Material metadata lives on the existing writer. This module never opens a
//! resource file: trusted preparation and retire/delete/finalize I/O live outside
//! SQLite. Keys, counters, indexes, receipts and authority publish share COMMIT.
use super::{materials_codec as dto, *};
use fc_domain::{materials::*, schedule_document::ScheduleDocument, time::*};
use fc_ports::{
    MaterialAppendRequest, MaterialDeleteRequest, MaterialListRequest, MaterialRepository,
    MaterialRepositoryError, PreparedMaterialAppend,
};
const ROOT: &str = "business/material/v1/";
const CATALOG: &str = "business/material/v1/catalog";
const MAX_FOLDERS: u64 = 4096;
const MAX_RECORDS: u64 = 16384;
const MAX_RESOURCES: u64 = 8192;
pub const ORIGINAL_BYTE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const RESPONSE_LIMIT: usize = 256 * 1024;
fn key(kind: &str, id: u64) -> String {
    format!("{ROOT}{kind}/{id:020}")
}
fn scope_key(scope: MaterialScope) -> String {
    format!(
        "{ROOT}scope/{:020}/{:020}",
        scope.teacher_id.get(),
        scope.subject_id.get()
    )
}
fn folder_prefix(folder: FolderId) -> String {
    format!("{ROOT}folder-items/{:020}/", folder.get())
}
fn member_key(folder: FolderId, id: MaterialId) -> String {
    format!("{}{:020}", folder_prefix(folder), id.get())
}
fn index_prefix(folder: FolderId) -> String {
    format!("{ROOT}published/{:020}/", folder.get())
}
fn index_key(record: &MaterialRecord) -> String {
    format!(
        "{}{:020}",
        index_prefix(record.folder_id),
        u64::MAX - record.added_sequence
    )
}
fn encode(v: &Value) -> Result<Vec<u8>, StorageError> {
    display::encode(v, MAX_VALUE_BYTES)
}
fn put(key: String, v: Value) -> Result<Mutation, StorageError> {
    Ok(Mutation::Put {
        key,
        value: encode(&v)?,
    })
}
fn read(d: &dyn TransactionDocuments, key: &str) -> Result<Option<Value>, StorageError> {
    d.get(key)?.map(|row| codec::value(&row.value)).transpose()
}
fn reference(v: &Value) -> Result<MaterialId, StorageError> {
    codec::object_fields(v, &["id"])?;
    MaterialId::new(codec::number(v, "id")?).map_err(|_| StorageError::Database)
}
fn material_error(error: MaterialError) -> StorageError {
    StorageError::Material(error)
}
pub(super) fn repository_error(error: StorageError) -> MaterialRepositoryError {
    match error {
        StorageError::Material(e) => MaterialRepositoryError::Domain(e),
        StorageError::ResourceQuota => MaterialRepositoryError::QuotaExceeded,
        StorageError::CommandIdReused => MaterialRepositoryError::IdempotencyConflict,
        StorageError::DocumentTooLarge => MaterialRepositoryError::DocumentTooLarge,
        StorageError::AuthorityUnavailable
        | StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::WorkerPanicked => MaterialRepositoryError::Faulted,
        _ => MaterialRepositoryError::Unavailable,
    }
}
#[derive(Default)]
struct Catalog {
    folders: u64,
    records: u64,
    resources: u64,
    bytes: u64,
}
impl Catalog {
    fn read(d: &dyn TransactionDocuments) -> Result<Self, StorageError> {
        let Some(v) = read(d, CATALOG)? else {
            return Ok(Self::default());
        };
        codec::object_fields(&v, &["format", "folders", "records", "resources", "bytes"])?;
        if codec::number(&v, "format")? != 1 {
            return Err(StorageError::Database);
        }
        let s = Self {
            folders: codec::number(&v, "folders")?,
            records: codec::number(&v, "records")?,
            resources: codec::number(&v, "resources")?,
            bytes: codec::number(&v, "bytes")?,
        };
        if s.folders > MAX_FOLDERS
            || s.records > MAX_RECORDS
            || s.resources > MAX_RESOURCES
            || s.bytes > ORIGINAL_BYTE_LIMIT
        {
            return Err(StorageError::Database);
        }
        Ok(s)
    }
    fn mutation(&self) -> Result<Mutation, StorageError> {
        put(
            CATALOG.into(),
            json!({"format":1,"folders":self.folders,"records":self.records,"resources":self.resources,"bytes":self.bytes}),
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceState {
    Referenced,
    Unreferenced,
    Retiring,
    Removed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceInventory {
    pub resource: MaterialResource,
    pub scope: MaterialScope,
    pub material_id: MaterialId,
    pub revision: Revision,
    pub references: u64,
    pub state: ResourceState,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceInventoryPage {
    pub entries: Vec<ResourceInventory>,
    pub next_cursor: Option<ResourceVersionId>,
    pub original_bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceRetirement {
    pub resource: MaterialResource,
    pub scope: MaterialScope,
    pub material_id: MaterialId,
    pub revision: Revision,
}
fn inventory(v: &ResourceInventory) -> Value {
    json!({"format":1,"resource":dto::resource(&v.resource),"scope":dto::scope(v.scope),"material":v.material_id.get(),"revision":v.revision.get(),"references":v.references,"state":match v.state{ResourceState::Referenced=>"referenced",ResourceState::Unreferenced=>"unreferenced",ResourceState::Retiring=>"retiring",ResourceState::Removed=>"removed"}})
}
fn read_inventory(v: &Value) -> Result<ResourceInventory, StorageError> {
    codec::object_fields(
        v,
        &[
            "format",
            "resource",
            "scope",
            "material",
            "revision",
            "references",
            "state",
        ],
    )?;
    if codec::number(v, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let s = ResourceInventory {
        resource: dto::read_resource(&v["resource"])?,
        scope: dto::read_scope(&v["scope"])?,
        material_id: MaterialId::new(codec::number(v, "material")?)
            .map_err(|_| StorageError::Database)?,
        revision: Revision::new(codec::number(v, "revision")?),
        references: codec::number(v, "references")?,
        state: match v["state"].as_str() {
            Some("referenced") => ResourceState::Referenced,
            Some("unreferenced") => ResourceState::Unreferenced,
            Some("retiring") => ResourceState::Retiring,
            Some("removed") => ResourceState::Removed,
            _ => return Err(StorageError::Database),
        },
    };
    if s.revision == Revision::default()
        || s.references > 1
        || (s.state == ResourceState::Referenced) != (s.references == 1)
    {
        return Err(StorageError::Database);
    }
    Ok(s)
}
fn resource_entry(
    d: &dyn TransactionDocuments,
    id: ResourceVersionId,
) -> Result<ResourceInventory, StorageError> {
    let v = read(d, &key("resources", id.get()))?.ok_or(StorageError::Database)?;
    let r = read_inventory(&v)?;
    if r.resource.version_id != id {
        return Err(StorageError::Database);
    }
    Ok(r)
}
fn load_record(
    d: &dyn TransactionDocuments,
    id: MaterialId,
) -> Result<MaterialRecord, StorageError> {
    let v =
        read(d, &key("items", id.get()))?.ok_or(StorageError::Material(MaterialError::NotFound))?;
    let r = dto::read_record(&v)?;
    if r.id != id {
        return Err(StorageError::Database);
    }
    Ok(r)
}
fn find_folder(
    d: &dyn TransactionDocuments,
    scope: MaterialScope,
) -> Result<Option<TeacherMaterialFolder>, StorageError> {
    let Some(v) = read(d, &scope_key(scope))? else {
        return Ok(None);
    };
    codec::object_fields(&v, &["id"])?;
    let id = FolderId::new(codec::number(&v, "id")?).map_err(|_| StorageError::Database)?;
    let f = dto::read_folder(&read(d, &key("folders", id.get()))?.ok_or(StorageError::Database)?)?;
    if f.id != id || f.scope != scope || f.revision == Revision::default() {
        return Err(StorageError::Database);
    }
    Ok(Some(f))
}
fn visit(
    d: &dyn TransactionDocuments,
    prefix: &str,
    mut f: impl FnMut(&str, &Value) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut cursor = None;
    loop {
        let page = d.scan_prefix(prefix, cursor.as_deref(), 128)?;
        for (key, row) in page.entries {
            f(&key, &codec::value(&row.value)?)?;
        }
        match page.next_key {
            Some(next) => {
                if cursor.as_ref().is_some_and(|prior| next <= *prior) {
                    return Err(StorageError::Database);
                }
                cursor = Some(next);
            }
            None => break,
        }
    }
    Ok(())
}
fn load_state(
    d: &dyn TransactionDocuments,
    folder: TeacherMaterialFolder,
) -> Result<MaterialFolderState, StorageError> {
    let mut records = Vec::new();
    visit(d, &folder_prefix(folder.id), |key, v| {
        if records.len() >= MAX_FOLDER_RECORDS {
            return Err(StorageError::Database);
        }
        let id = reference(v)?;
        if key != member_key(folder.id, id) {
            return Err(StorageError::Database);
        }
        let r = load_record(d, id)?;
        if r.folder_id != folder.id {
            return Err(StorageError::Database);
        }
        records.push(r);
        Ok(())
    })?;
    let s = MaterialFolderState { folder, records };
    s.validate().map_err(|_| StorageError::Database)?;
    Ok(s)
}
fn new_folder(
    catalog: &mut Catalog,
    scope: MaterialScope,
) -> Result<MaterialFolderState, StorageError> {
    if catalog.folders >= MAX_FOLDERS {
        return Err(material_error(MaterialError::Capacity));
    }
    catalog.folders += 1;
    Ok(MaterialFolderState::empty(
        FolderId::new(catalog.folders).map_err(|_| StorageError::Database)?,
        scope,
    ))
}
fn folder_mutations(folder: &TeacherMaterialFolder) -> Result<Vec<Mutation>, StorageError> {
    Ok(vec![
        put(key("folders", folder.id.get()), dto::folder(folder))?,
        put(scope_key(folder.scope), json!({"id":folder.id.get()}))?,
    ])
}
fn authorize(
    state: &IdentityState,
    actor: &ActorContext,
    scope: MaterialScope,
    clock: &dyn Clock,
    guard: &SessionGuard,
) -> Result<(), StorageError> {
    guard(state, actor)?;
    authorize_material_folder(state, actor, scope, clock.now()).map_err(material_error)
}
fn page(
    d: &dyn TransactionDocuments,
    identity: &IdentityState,
    folder: TeacherMaterialFolder,
    source: Option<MaterialSource>,
    cursor: Option<MaterialCursor>,
    limit: usize,
) -> Result<MaterialPage, StorageError> {
    let class_id = source.as_ref().map(|s| s.lesson.class_id);
    let offset = if let Some(c) = cursor {
        if c.folder_id != folder.id
            || c.folder_revision != folder.revision
            || c.scope != folder.scope
            || c.source != source
            || c.class_id != class_id
        {
            return Err(material_error(MaterialError::StaleCursor));
        }
        c.offset
    } else {
        0
    };
    let mut total = 0u64;
    let mut items = Vec::with_capacity(limit);
    let mut scanned = 0usize;
    visit(d, &index_prefix(folder.id), |index, v| {
        scanned += 1;
        if scanned > MAX_FOLDER_RECORDS {
            return Err(StorageError::Database);
        }
        let r = load_record(d, reference(v)?)?;
        if index != index_key(&r)
            || r.folder_id != folder.id
            || r.status != MaterialStatus::Published
            || r.revision > folder.revision
        {
            return Err(StorageError::Database);
        }
        let registry = resource_entry(d, r.resource.version_id)?;
        if registry.state != ResourceState::Referenced
            || registry.references != 1
            || registry.material_id != r.id
            || registry.scope != folder.scope
            || registry.resource != r.resource
        {
            return Err(StorageError::Database);
        }
        if class_id.is_some_and(|class| !r.allowed_class_ids.contains(&class)) {
            return Ok(());
        }
        if total >= offset && items.len() < limit {
            items.push(r);
        }
        total += 1;
        Ok(())
    })?;
    if offset > total {
        return Err(material_error(MaterialError::StaleCursor));
    }
    let end = offset + items.len() as u64;
    let next_cursor = (end < total).then(|| MaterialCursor {
        folder_id: folder.id,
        folder_revision: folder.revision,
        scope: folder.scope,
        class_id,
        source: source.clone(),
        offset: end,
    });
    let p = MaterialPage {
        folder,
        source,
        class_id,
        offset,
        total,
        items,
        next_cursor,
    };
    p.validate(identity, limit)
        .map_err(|_| StorageError::Database)?;
    Ok(p)
}
pub(super) fn desktop_page(
    d: &dyn TransactionDocuments,
    context: &fc_domain::lesson_context::LessonContextSnapshot,
    identity: &IdentityState,
    schedule: &ScheduleDocument,
    now: LocalDateTime,
    cursor: Option<MaterialCursor>,
) -> Result<(Option<MaterialSource>, Option<MaterialPage>), StorageError> {
    let Some(class) = identity.installation.as_ref().and_then(|i| i.current_class) else {
        if cursor.is_some() {
            return Err(material_error(MaterialError::StaleCursor));
        }
        return Ok((None, None));
    };
    let source = context
        .material_source(identity, schedule, class, now)
        .map_err(StorageError::Material)?;
    let Some(s) = &source else {
        if cursor.is_some() {
            return Err(material_error(MaterialError::StaleCursor));
        }
        return Ok((None, None));
    };
    let scope = MaterialScope {
        teacher_id: s.teacher_id,
        subject_id: s.subject_id,
    };
    let Some(folder) = find_folder(d, scope)? else {
        if cursor.is_some() {
            return Err(material_error(MaterialError::StaleCursor));
        }
        return Ok((source, None));
    };
    let p = page(
        d,
        identity,
        folder,
        source.clone(),
        cursor,
        DESKTOP_MATERIAL_PAGE,
    )?;
    Ok((source, Some(p)))
}
pub(super) fn resolve_source(
    d: &dyn TransactionDocuments,
    identity: &IdentityState,
    schedule: &ScheduleDocument,
    class: ClassId,
    now: LocalDateTime,
    floor: Option<LocalDateTime>,
) -> Result<Option<MaterialSource>, StorageError> {
    let (context, _) = super::lesson::observe_with_floor(d, identity, schedule, now, floor)?;
    super::lesson::source(&context, identity, schedule, class, now)
}
impl<S: Send + 'static> MaterialRepository for BusinessRepository<S> {
    fn materials(
        &self,
        request: MaterialListRequest,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<MaterialPage, MaterialRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(ListTask {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        let p = dto::read_page(
            &codec::value_with_limit(&bytes, RESPONSE_LIMIT).map_err(repository_error)?,
        )
        .map_err(repository_error)?;
        Ok(p)
    }
    fn append_material(
        &mut self,
        prepared: PreparedMaterialAppend,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<MaterialRecord, MaterialRepositoryError> {
        self.mutate_material(Operation::Append(prepared.into_request()), actor)
    }
    fn delete_material(
        &mut self,
        request: MaterialDeleteRequest,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<MaterialRecord, MaterialRepositoryError> {
        self.mutate_material(Operation::Delete(request), actor)
    }
    fn published_material(
        &self,
        source: &MaterialSource,
        generation: SceneGeneration,
        material_id: MaterialId,
        resource_version: ResourceVersionId,
        _clock: &dyn Clock,
    ) -> Result<MaterialRecord, MaterialRepositoryError> {
        self.observe_lesson()
            .map_err(|_| MaterialRepositoryError::Unavailable)?;
        let result = self
            .handle
            .try_transaction(PublishedTask {
                source: source.clone(),
                generation,
                material_id,
                resource_version,
                clock: self.clock.clone(),
                scene: self.material_scene.clone(),
                observations: self.lesson_observations.clone(),
            })
            .and_then(|ticket| ticket.wait());
        if matches!(
            &result,
            Err(StorageError::Busy
                | StorageError::Lesson(_)
                | StorageError::Material(fc_domain::materials::MaterialError::InvalidContext))
        ) {
            self.observe_lesson()
                .map_err(|_| MaterialRepositoryError::Unavailable)?;
        }
        let bytes = result.map_err(repository_error)?;
        dto::read_record(&codec::value(&bytes).map_err(repository_error)?).map_err(repository_error)
    }
}
impl<S: Send + 'static> BusinessRepository<S> {
    fn mutate_material(
        &mut self,
        operation: Operation,
        actor: &ActorContext,
    ) -> Result<MaterialRecord, MaterialRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(MutationTask {
                operation,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        dto::read_record(&codec::value(&bytes).map_err(repository_error)?).map_err(repository_error)
    }
}
struct ListTask<S> {
    request: MaterialListRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for ListTask<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        authorize(
            &state,
            &self.actor,
            self.request.scope,
            self.clock.as_ref(),
            &self.session,
        )?;
        let mut catalog = Catalog::read(d)?;
        let mut created = None;
        let folder = match find_folder(d, self.request.scope)? {
            Some(f) => f,
            None => {
                if self.request.cursor.is_some() {
                    return Err(material_error(MaterialError::StaleCursor));
                }
                let mut empty = new_folder(&mut catalog, self.request.scope)?;
                empty.folder.revision = Revision::new(1);
                created = Some(empty.folder.clone());
                empty.folder
            }
        };
        let p = page(
            d,
            &state,
            folder,
            None,
            self.request.cursor,
            MANAGEMENT_MATERIAL_PAGE,
        )?;
        let outcome = display::encode(&dto::page(&p), RESPONSE_LIMIT)?;
        authorize(
            &state,
            &self.actor,
            self.request.scope,
            self.clock.as_ref(),
            &self.session,
        )?;
        let Some(folder) = created else {
            return Ok(TransactionDecision {
                outcome,
                commit: None,
                precommit: None,
            });
        };
        let mut mutations = folder_mutations(&folder)?;
        mutations.push(catalog.mutation()?);
        let snapshot = (self.factory)(
            &state,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let actor = self.actor;
        let clock = self.clock;
        let session = self.session;
        let scope = self.request.scope;
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                authorize(&state, &actor, scope, clock.as_ref(), &session)
            })),
        })
    }
}
enum Operation {
    Append(MaterialAppendRequest),
    Delete(MaterialDeleteRequest),
}
impl Operation {
    fn scope(&self) -> MaterialScope {
        match self {
            Self::Append(r) => r.scope,
            Self::Delete(r) => r.scope,
        }
    }
    fn id(&self) -> CommandId {
        match self {
            Self::Append(r) => r.command_id,
            Self::Delete(r) => r.command_id,
        }
    }
    fn payload(&self) -> Value {
        match self {
            Self::Append(r) => {
                json!({"operation":"append","scope":dto::scope(r.scope),"material":r.append.material_id.get(),"name":r.append.display_name,"resource":dto::resource(&r.append.resource),"classes":r.append.allowed_class_ids.iter().map(|c|c.get()).collect::<Vec<_>>()})
            }
            Self::Delete(r) => {
                json!({"operation":"delete","scope":dto::scope(r.scope),"material":r.material_id.get(),"revision":r.expected_material_revision.get()})
            }
        }
    }
}
struct MutationTask<S> {
    operation: Operation,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for MutationTask<S> {
    fn allocation_bytes(&self) -> usize {
        match &self.operation {
            Operation::Append(r) => r
                .append
                .allocation_bytes()
                .and_then(|n| n.checked_add(std::mem::size_of::<Self>()))
                .unwrap_or(usize::MAX),
            Operation::Delete(_) => std::mem::size_of::<Self>(),
        }
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        let scope = self.operation.scope();
        authorize(
            &state,
            &self.actor,
            scope,
            self.clock.as_ref(),
            &self.session,
        )?;
        if let Operation::Append(r) = &self.operation {
            r.append.validate().map_err(material_error)?;
        }
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"future-classroom/material/v1\0");
        fingerprint.update(codec::bytes(&codec::actor(&self.actor))?);
        fingerprint.update(codec::bytes(&self.operation.payload())?);
        let digest = fingerprint.finalize().to_vec();
        let journal = key("receipts", self.operation.id().get());
        if let Some(v) = read(d, &journal)? {
            codec::object_fields(&v, &["fingerprint", "record", "result_digest"])?;
            let prior: Vec<u8> = serde_json::from_value(v["fingerprint"].clone())
                .map_err(|_| StorageError::Database)?;
            if prior.len() != 32 {
                return Err(StorageError::Database);
            }
            if prior != digest {
                return Err(StorageError::CommandIdReused);
            }
            let result_digest: Vec<u8> = serde_json::from_value(v["result_digest"].clone())
                .map_err(|_| StorageError::Database)?;
            if result_digest.len() != 32
                || result_digest.as_slice() != Sha256::digest(encode(&v["record"])?).as_slice()
            {
                return Err(StorageError::Database);
            }
            let result = dto::read_record(&v["record"])?;
            match &self.operation {
                Operation::Append(r)
                    if result.id == r.append.material_id
                        && result.resource == r.append.resource
                        && result.display_name == r.append.display_name
                        && result.allowed_class_ids == r.append.allowed_class_ids
                        && result.status == MaterialStatus::Published => {}
                Operation::Delete(r)
                    if result.id == r.material_id
                        && result.revision
                            == r.expected_material_revision
                                .next()
                                .map_err(|_| StorageError::Database)?
                        && result.status == MaterialStatus::Deleted => {}
                _ => return Err(StorageError::Database),
            }
            let folder = find_folder(d, scope)?.ok_or(StorageError::Database)?;
            if result.folder_id != folder.id {
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
                outcome: encode(&dto::record(&result))?,
                commit: None,
                precommit: None,
            });
        }
        let mut catalog = Catalog::read(d)?;
        let current = match find_folder(d, scope)? {
            Some(f) => load_state(d, f)?,
            None => new_folder(&mut catalog, scope)?,
        };
        let at = self.clock.now();
        let (next, result, entry, index_mutation) = match &self.operation {
            Operation::Append(r) => {
                if catalog.records >= MAX_RECORDS || catalog.resources >= MAX_RESOURCES {
                    return Err(material_error(MaterialError::Capacity));
                }
                if d.get(&key("items", r.append.material_id.get()))?.is_some()
                    || d.get(&key("resources", r.append.resource.version_id.get()))?
                        .is_some()
                {
                    return Err(material_error(MaterialError::AlreadyExists));
                }
                catalog.bytes = catalog
                    .bytes
                    .checked_add(r.append.resource.bytes)
                    .filter(|n| *n <= ORIGINAL_BYTE_LIMIT)
                    .ok_or(StorageError::ResourceQuota)?;
                catalog.records += 1;
                catalog.resources += 1;
                let next = current
                    .append(&r.append, &state, &self.actor, at)
                    .map_err(material_error)?;
                let result = next.records.last().ok_or(StorageError::Database)?.clone();
                let entry = ResourceInventory {
                    resource: result.resource.clone(),
                    scope,
                    material_id: result.id,
                    revision: Revision::new(1),
                    references: 1,
                    state: ResourceState::Referenced,
                };
                let index = put(index_key(&result), json!({"id":result.id.get()}))?;
                (next, result, entry, index)
            }
            Operation::Delete(r) => {
                let next = current
                    .delete(
                        r.material_id,
                        r.expected_material_revision,
                        &state,
                        &self.actor,
                        at,
                    )
                    .map_err(material_error)?;
                let result = next
                    .records
                    .iter()
                    .find(|x| x.id == r.material_id)
                    .ok_or(StorageError::Database)?
                    .clone();
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
                let index = Mutation::Delete {
                    key: index_key(&result),
                };
                (next, result, entry, index)
            }
        };
        let mut mutations = folder_mutations(&next.folder)?;
        mutations.extend([
            put(key("items", result.id.get()), dto::record(&result))?,
            put(
                member_key(result.folder_id, result.id),
                json!({"id":result.id.get()}),
            )?,
            put(
                key("resources", entry.resource.version_id.get()),
                inventory(&entry),
            )?,
            index_mutation,
            catalog.mutation()?,
            put(
                journal,
                json!({"fingerprint":digest,"record":dto::record(&result),"result_digest":Sha256::digest(encode(&dto::record(&result))?).to_vec()}),
            )?,
        ]);
        let outcome = encode(&dto::record(&result))?;
        let snapshot = (self.factory)(
            &state,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let actor = self.actor;
        let clock = self.clock;
        let guard = self.session;
        let operation = self.operation;
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                authorize(&state, &actor, scope, clock.as_ref(), &guard)?;
                match operation {
                    Operation::Append(r) => current
                        .append(&r.append, &state, &actor, clock.now())
                        .map(|_| ())
                        .map_err(material_error),
                    Operation::Delete(r) => current
                        .delete(
                            r.material_id,
                            r.expected_material_revision,
                            &state,
                            &actor,
                            clock.now(),
                        )
                        .map(|_| ())
                        .map_err(material_error),
                }
            })),
        })
    }
}
struct PublishedTask {
    source: MaterialSource,
    generation: SceneGeneration,
    material_id: MaterialId,
    resource_version: ResourceVersionId,
    clock: Arc<dyn Clock + Send + Sync>,
    scene: MaterialSceneGuard,
    observations: super::lesson::Observations,
}
impl<S> TransactionTask<S> for PublishedTask {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        let schedule = schedule::load(d, &identity)?;
        let check = || -> Result<(), StorageError> {
            (self.scene)(&self.source, self.generation)?;
            let now = self.clock.now();
            if desktop::read_watermark(d, &identity)?.is_some_and(|day| now.date < day) {
                return Err(material_error(MaterialError::InvalidContext));
            }
            let class = identity
                .installation
                .as_ref()
                .and_then(|i| i.current_class)
                .ok_or(material_error(MaterialError::InvalidContext))?;
            if resolve_source(
                d,
                &identity,
                &schedule,
                class,
                now,
                super::lesson::witnessed(&self.observations, now)?,
            )? != Some(self.source.clone())
            {
                return Err(material_error(MaterialError::InvalidContext));
            }
            Ok(())
        };
        check()?;
        let r = load_record(d, self.material_id)?;
        let scope = MaterialScope {
            teacher_id: self.source.teacher_id,
            subject_id: self.source.subject_id,
        };
        let folder = find_folder(d, scope)?.ok_or(material_error(MaterialError::NotFound))?;
        let resource = resource_entry(d, r.resource.version_id)?;
        if r.folder_id != folder.id
            || r.status != MaterialStatus::Published
            || r.resource.version_id != self.resource_version
            || !r.allowed_class_ids.contains(&self.source.lesson.class_id)
            || resource.references != 1
            || resource.state != ResourceState::Referenced
            || resource.scope != scope
            || resource.material_id != r.id
            || resource.resource != r.resource
        {
            return Err(material_error(MaterialError::InvalidContext));
        }
        let outcome = encode(&dto::record(&r))?;
        check()?;
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}

pub(super) fn startup_check(
    d: &dyn TransactionDocuments,
    identity: &IdentityState,
) -> Result<(), StorageError> {
    let catalog = Catalog::read(d)?;
    if identity.installation.is_none() {
        if !d.scan_prefix(ROOT, None, 1)?.entries.is_empty() {
            return Err(StorageError::Database);
        }
        return Ok(());
    }
    let mut folders = 0u64;
    visit(d, &format!("{ROOT}folders/"), |k, v| {
        folders += 1;
        if folders > MAX_FOLDERS {
            return Err(StorageError::Database);
        }
        let f = dto::read_folder(v)?;
        if k != key("folders", f.id.get())
            || f.revision == Revision::default()
            || find_folder(d, f.scope)? != Some(f.clone())
        {
            return Err(StorageError::Database);
        }
        load_state(d, f)?;
        Ok(())
    })?;
    let mut scopes = 0u64;
    visit(d, &format!("{ROOT}scope/"), |k, v| {
        scopes += 1;
        if scopes > MAX_FOLDERS {
            return Err(StorageError::Database);
        }
        codec::object_fields(v, &["id"])?;
        let id = FolderId::new(codec::number(v, "id")?).map_err(|_| StorageError::Database)?;
        let f =
            dto::read_folder(&read(d, &key("folders", id.get()))?.ok_or(StorageError::Database)?)?;
        if k != scope_key(f.scope) {
            return Err(StorageError::Database);
        }
        Ok(())
    })?;
    let mut records = 0u64;
    let mut active_records = 0u64;
    visit(d, &format!("{ROOT}items/"), |k, v| {
        records += 1;
        if records > MAX_RECORDS {
            return Err(StorageError::Database);
        }
        let r = dto::read_record(v)?;
        if k != key("items", r.id.get())
            || reference(&read(d, &member_key(r.folder_id, r.id))?.ok_or(StorageError::Database)?)?
                != r.id
        {
            return Err(StorageError::Database);
        }
        let f = dto::read_folder(
            &read(d, &key("folders", r.folder_id.get()))?.ok_or(StorageError::Database)?,
        )?;
        let e = resource_entry(d, r.resource.version_id)?;
        if e.material_id != r.id
            || e.scope != f.scope
            || e.resource != r.resource
            || (r.status == MaterialStatus::Published) != (e.state == ResourceState::Referenced)
        {
            return Err(StorageError::Database);
        }
        let index = read(d, &index_key(&r))?;
        if r.status == MaterialStatus::Published {
            active_records += 1;
            if reference(&index.ok_or(StorageError::Database)?)? != r.id {
                return Err(StorageError::Database);
            }
        } else if index.is_some() {
            return Err(StorageError::Database);
        }
        Ok(())
    })?;
    let mut members = 0u64;
    visit(d, &format!("{ROOT}folder-items/"), |k, v| {
        members += 1;
        if members > MAX_RECORDS {
            return Err(StorageError::Database);
        }
        let r = load_record(d, reference(v)?)?;
        if k != member_key(r.folder_id, r.id) {
            return Err(StorageError::Database);
        }
        Ok(())
    })?;
    let mut published = 0u64;
    visit(d, &format!("{ROOT}published/"), |k, v| {
        published += 1;
        if published > MAX_RECORDS {
            return Err(StorageError::Database);
        }
        let r = load_record(d, reference(v)?)?;
        if k != index_key(&r) || r.status != MaterialStatus::Published {
            return Err(StorageError::Database);
        }
        Ok(())
    })?;
    let mut resources = 0u64;
    let mut bytes = 0u64;
    visit(d, &format!("{ROOT}resources/"), |k, v| {
        resources += 1;
        if resources > MAX_RESOURCES {
            return Err(StorageError::Database);
        }
        let e = read_inventory(v)?;
        if k != key("resources", e.resource.version_id.get()) {
            return Err(StorageError::Database);
        }
        let r = load_record(d, e.material_id)?;
        let f = dto::read_folder(
            &read(d, &key("folders", r.folder_id.get()))?.ok_or(StorageError::Database)?,
        )?;
        if r.resource != e.resource
            || e.scope != f.scope
            || (r.status == MaterialStatus::Published) != (e.references == 1)
        {
            return Err(StorageError::Database);
        }
        if e.state != ResourceState::Removed {
            bytes = bytes
                .checked_add(e.resource.bytes)
                .filter(|b| *b <= ORIGINAL_BYTE_LIMIT)
                .ok_or(StorageError::Database)?;
        }
        Ok(())
    })?;
    if catalog.folders != folders
        || folders != scopes
        || catalog.records != records
        || catalog.resources != resources
        || catalog.bytes != bytes
        || members != records
        || published != active_records
    {
        return Err(StorageError::Database);
    }
    Ok(())
}
impl<S: Send + 'static> BusinessRepository<S> {
    /// Trusted maintenance worker only. No caller path or filesystem handle enters SQLite.
    pub fn resource_inventory(
        &self,
        after: Option<ResourceVersionId>,
    ) -> Result<ResourceInventoryPage, MaterialRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(InventoryTask { after })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        let v = codec::value_with_limit(&bytes, RESPONSE_LIMIT).map_err(repository_error)?;
        codec::object_fields(&v, &["entries", "next", "bytes"]).map_err(repository_error)?;
        let array = codec::array(&v, "entries").map_err(repository_error)?;
        if array.len() > 128 {
            return Err(MaterialRepositoryError::Unavailable);
        }
        Ok(ResourceInventoryPage {
            entries: array
                .iter()
                .map(read_inventory)
                .collect::<Result<Vec<_>, _>>()
                .map_err(repository_error)?,
            next_cursor: if v["next"].is_null() {
                None
            } else {
                Some(
                    ResourceVersionId::new(codec::number(&v, "next").map_err(repository_error)?)
                        .map_err(|_| MaterialRepositoryError::Unavailable)?,
                )
            },
            original_bytes: codec::number(&v, "bytes").map_err(repository_error)?,
        })
    }
    pub fn retire_resource(
        &self,
        id: ResourceVersionId,
        revision: Revision,
    ) -> Result<ResourceRetirement, MaterialRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(GcTask {
                operation: GcOperation::Retire(id, revision),
                factory: self.factory.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        let e = read_inventory(&codec::value(&bytes).map_err(repository_error)?)
            .map_err(repository_error)?;
        Ok(ResourceRetirement {
            resource: e.resource,
            scope: e.scope,
            material_id: e.material_id,
            revision: e.revision,
        })
    }
    /// Call only after the pinned controlled file was removed (or confirmed absent).
    pub fn finalize_resource_removal(
        &self,
        ticket: ResourceRetirement,
    ) -> Result<(), MaterialRepositoryError> {
        self.handle
            .try_transaction(GcTask {
                operation: GcOperation::Finalize(ticket),
                factory: self.factory.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        Ok(())
    }
    pub fn managed_material(
        &self,
        scope: MaterialScope,
        id: MaterialId,
        version: ResourceVersionId,
        actor: &ActorContext,
    ) -> Result<MaterialRecord, MaterialRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(ManagedTask {
                scope,
                id,
                version,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        dto::read_record(&codec::value(&bytes).map_err(repository_error)?).map_err(repository_error)
    }
}
struct InventoryTask {
    after: Option<ResourceVersionId>,
}
impl<S> TransactionTask<S> for InventoryTask {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        if state.installation.is_none() {
            return Err(StorageError::InvalidCommand);
        }
        let after = self.after.map(|id| key("resources", id.get()));
        let page = d.scan_prefix(&format!("{ROOT}resources/"), after.as_deref(), 128)?;
        let mut entries = Vec::new();
        for (k, row) in &page.entries {
            let e = read_inventory(&codec::value(&row.value)?)?;
            if *k != key("resources", e.resource.version_id.get()) {
                return Err(StorageError::Database);
            }
            entries.push(inventory(&e));
        }
        let next = if page.next_key.is_some() {
            entries
                .last()
                .map(|v| v["resource"]["version"].clone())
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let outcome = display::encode(
            &json!({"entries":entries,"next":next,"bytes":Catalog::read(d)?.bytes}),
            RESPONSE_LIMIT,
        )?;
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}
enum GcOperation {
    Retire(ResourceVersionId, Revision),
    Finalize(ResourceRetirement),
}
struct GcTask<S> {
    operation: GcOperation,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for GcTask<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        if state.installation.is_none() {
            return Err(StorageError::InvalidCommand);
        }
        let (id, expected) = match &self.operation {
            GcOperation::Retire(id, rev) => (*id, *rev),
            GcOperation::Finalize(t) => (t.resource.version_id, t.revision),
        };
        let mut e = resource_entry(d, id)?;
        let r = load_record(d, e.material_id)?;
        if r.status != MaterialStatus::Deleted || e.references != 0 || r.resource != e.resource {
            return Err(material_error(MaterialError::InvalidState));
        }
        let mut catalog = Catalog::read(d)?;
        let unchanged = match &self.operation {
            GcOperation::Retire(_, _) => {
                e.state == ResourceState::Retiring && e.revision == expected
            }
            GcOperation::Finalize(t) => {
                e.state == ResourceState::Removed
                    && e.revision == expected.next().map_err(|_| StorageError::Database)?
                    && e.resource == t.resource
                    && e.scope == t.scope
                    && e.material_id == t.material_id
            }
        };
        if unchanged {
            return Ok(TransactionDecision {
                outcome: encode(&inventory(&e))?,
                commit: None,
                precommit: None,
            });
        }
        if e.revision != expected {
            return Err(material_error(MaterialError::RevisionConflict {
                current: e.revision,
            }));
        }
        match self.operation {
            GcOperation::Retire(_, _) => {
                if e.state != ResourceState::Unreferenced {
                    return Err(material_error(MaterialError::InvalidState));
                }
                e.state = ResourceState::Retiring;
            }
            GcOperation::Finalize(t) => {
                if e.state != ResourceState::Retiring
                    || e.resource != t.resource
                    || e.scope != t.scope
                    || e.material_id != t.material_id
                {
                    return Err(material_error(MaterialError::InvalidState));
                }
                catalog.bytes = catalog
                    .bytes
                    .checked_sub(e.resource.bytes)
                    .ok_or(StorageError::Database)?;
                e.state = ResourceState::Removed;
            }
        }
        e.revision = e.revision.next().map_err(|_| StorageError::Database)?;
        let snapshot = (self.factory)(
            &state,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        Ok(TransactionDecision {
            outcome: encode(&inventory(&e))?,
            commit: Some((
                vec![
                    put(key("resources", id.get()), inventory(&e))?,
                    catalog.mutation()?,
                ],
                snapshot,
            )),
            precommit: None,
        })
    }
}
struct ManagedTask {
    scope: MaterialScope,
    id: MaterialId,
    version: ResourceVersionId,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for ManagedTask {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        authorize(
            &state,
            &self.actor,
            self.scope,
            self.clock.as_ref(),
            &self.session,
        )?;
        let folder = find_folder(d, self.scope)?.ok_or(material_error(MaterialError::NotFound))?;
        let r = load_record(d, self.id)?;
        let e = resource_entry(d, self.version)?;
        if r.folder_id != folder.id
            || r.status != MaterialStatus::Published
            || r.resource.version_id != self.version
            || e.resource != r.resource
            || e.scope != self.scope
            || e.material_id != r.id
            || e.references != 1
            || e.state != ResourceState::Referenced
        {
            return Err(material_error(MaterialError::InvalidContext));
        }
        let outcome = encode(&dto::record(&r))?;
        authorize(
            &state,
            &self.actor,
            self.scope,
            self.clock.as_ref(),
            &self.session,
        )?;
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}
