//! Bounded profile metadata on the existing writer. No per-profile worker/cache.
//! A small manifest plus fixed-size pages declares every committed slot; startup
//! checks both directions so a missing document cannot become a new empty draft.
use super::{profiles_codec as dto, *};
use fc_domain::{desktop::DisplaySettings, materials::MaterialScope, profile::*};

const ROOT: &str = "business/profiles/v1/";
const MANIFEST: &str = "business/profiles/v1/manifest";
const PUBLIC: &str = "business/profiles/v1/public";
const PAGE_SIZE: u64 = 64;
const MAX_PROFILES: u64 = 4096;
#[derive(Default)]
struct Catalog {
    count: u64,
    public: bool,
}
fn slot_key(k: SubjectProfileKey) -> String {
    format!(
        "{ROOT}slots/{:020}/{:020}/{:020}",
        k.class_id.get(),
        k.subject_id.get(),
        k.teacher_id.get()
    )
}
fn document_key(id: ProfileId) -> String {
    format!("{ROOT}documents/{:020}", id.get())
}
fn page_key(page: u64) -> String {
    format!("{ROOT}pages/{page:020}")
}
fn put(key: String, value: Value) -> Result<Mutation, StorageError> {
    Ok(Mutation::Put {
        key,
        value: display::encode(&value, MAX_VALUE_BYTES)?,
    })
}
fn read(d: &dyn TransactionDocuments, key: &str) -> Result<Option<Value>, StorageError> {
    d.get(key)?.map(|v| codec::value(&v.value)).transpose()
}
impl Catalog {
    fn load(d: &dyn TransactionDocuments) -> Result<Self, StorageError> {
        let Some(v) = read(d, MANIFEST)? else {
            if !d.scan_prefix(ROOT, None, 1)?.entries.is_empty() {
                return Err(StorageError::Database);
            }
            return Ok(Self::default());
        };
        codec::object_fields(&v, &["format", "count", "public"])?;
        if codec::number(&v, "format")? != 1 {
            return Err(StorageError::Database);
        }
        let c = Self {
            count: codec::number(&v, "count")?,
            public: codec::boolean(&v, "public")?,
        };
        if c.count > MAX_PROFILES {
            return Err(StorageError::Database);
        }
        Ok(c)
    }
    fn mutation(&self) -> Result<Mutation, StorageError> {
        put(
            MANIFEST.into(),
            json!({"format":1,"count":self.count,"public":self.public}),
        )
    }
    fn allocate(&mut self) -> Result<ProfileId, StorageError> {
        if self.count >= MAX_PROFILES {
            return Err(StorageError::DocumentTooLarge);
        }
        self.count += 1;
        ProfileId::new(self.count).map_err(|_| StorageError::Database)
    }
}
fn entry(v: &Value) -> Result<fc_ports::ProfileSummary, StorageError> {
    codec::object_fields(v, &["id", "key", "revision", "enabled"])?;
    let e = fc_ports::ProfileSummary {
        id: dto::id(v, "id", ProfileId::new)?,
        key: dto::read_key(&v["key"])?,
        revision: Revision::new(codec::number(v, "revision")?),
        enabled: codec::boolean(v, "enabled")?,
    };
    if e.revision == Revision::default() {
        return Err(StorageError::Database);
    }
    Ok(e)
}
fn summary(p: &SubjectProfileDocument) -> fc_ports::ProfileSummary {
    fc_ports::ProfileSummary {
        id: p.id,
        key: p.key,
        revision: p.revision,
        enabled: p.enabled,
    }
}
fn summary_value(e: &fc_ports::ProfileSummary) -> Value {
    json!({"id":e.id.get(),"key":dto::key(e.key),"revision":e.revision.get(),"enabled":e.enabled})
}
fn read_page(
    d: &dyn TransactionDocuments,
    page: u64,
    count: u64,
) -> Result<Vec<fc_ports::ProfileSummary>, StorageError> {
    let v = read(d, &page_key(page))?.ok_or(StorageError::Database)?;
    codec::object_fields(&v, &["format", "entries"])?;
    if codec::number(&v, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let a = codec::array(&v, "entries")?;
    let expected = count.saturating_sub(page * PAGE_SIZE).min(PAGE_SIZE);
    if a.len() as u64 != expected || expected == 0 {
        return Err(StorageError::Database);
    }
    a.iter()
        .enumerate()
        .map(|(i, v)| {
            let e = entry(v)?;
            if e.id.get() != page * PAGE_SIZE + i as u64 + 1 {
                return Err(StorageError::Database);
            }
            Ok(e)
        })
        .collect()
}
fn page_mutation(
    page: u64,
    entries: &[fc_ports::ProfileSummary],
) -> Result<Mutation, StorageError> {
    put(
        page_key(page),
        json!({"format":1,"entries":entries.iter().map(summary_value).collect::<Vec<_>>()}),
    )
}
fn index_mutations(
    d: &dyn TransactionDocuments,
    catalog: &mut Catalog,
    p: &SubjectProfileDocument,
    new: bool,
) -> Result<Vec<Mutation>, StorageError> {
    let old_count = catalog.count;
    let id = if new { catalog.allocate()? } else { p.id };
    if id != p.id {
        return Err(StorageError::Database);
    }
    let page = (id.get() - 1) / PAGE_SIZE;
    let mut entries = if new && old_count.is_multiple_of(PAGE_SIZE) {
        Vec::new()
    } else {
        read_page(d, page, old_count)?
    };
    if new {
        entries.push(summary(p))
    } else {
        let e = entries
            .get_mut(((id.get() - 1) % PAGE_SIZE) as usize)
            .ok_or(StorageError::Database)?;
        if e.key != p.key {
            return Err(StorageError::Database);
        }
        *e = summary(p)
    }
    let mut mutations = vec![page_mutation(page, &entries)?];
    if new {
        mutations.push(catalog.mutation()?);
        mutations.push(put(slot_key(p.key), json!({"id":id.get()}))?)
    }
    Ok(mutations)
}
fn folder(
    d: &dyn TransactionDocuments,
    key: SubjectProfileKey,
) -> Result<Option<fc_domain::materials::TeacherMaterialFolder>, StorageError> {
    materials::find_folder(
        d,
        MaterialScope {
            teacher_id: key.teacher_id,
            subject_id: key.subject_id,
        },
    )
}
fn historical_refs(
    d: &dyn TransactionDocuments,
    state: &IdentityState,
    p: &SubjectProfileDocument,
) -> Result<(), StorageError> {
    let school = state
        .installation
        .as_ref()
        .ok_or(StorageError::Database)?
        .school_id;
    if state.account(p.key.teacher_id).is_err()
        || !state
            .classes
            .iter()
            .any(|c| c.id == p.key.class_id && c.school_id == school)
        || !state
            .subjects
            .iter()
            .any(|s| s.id == p.key.subject_id && s.school_id == school)
    {
        return Err(StorageError::Database);
    }
    if let Some(id) = p.folder_id {
        let f = folder(d, p.key)?.ok_or(StorageError::Database)?;
        if f.id != id {
            return Err(StorageError::Database);
        }
    }
    Ok(())
}
pub(super) fn load_public(
    d: &dyn TransactionDocuments,
    state: &IdentityState,
) -> Result<LayoutDocument, StorageError> {
    let catalog = Catalog::load(d)?;
    let raw = read(d, PUBLIC)?;
    if catalog.public != raw.is_some() {
        return Err(StorageError::Database);
    }
    let Some(v) = raw else {
        return Ok(LayoutDocument::default());
    };
    if state.installation.is_none() {
        return Err(StorageError::Database);
    }
    let p = dto::read_public(&v)?;
    if p.revision == Revision::default() || p.revision > d.revision()? {
        return Err(StorageError::Database);
    }
    Ok(p)
}
fn load_subject(
    d: &dyn TransactionDocuments,
    state: &IdentityState,
    key: SubjectProfileKey,
) -> Result<Option<SubjectProfileDocument>, StorageError> {
    let catalog = Catalog::load(d)?;
    let Some(index) = read(d, &slot_key(key))? else {
        return Ok(None);
    };
    codec::object_fields(&index, &["id"])?;
    let id = dto::id(&index, "id", ProfileId::new)?;
    if id.get() > catalog.count {
        return Err(StorageError::Database);
    }
    let p = dto::read_subject(&read(d, &document_key(id))?.ok_or(StorageError::Database)?)?;
    if p.key != key
        || p.id != id
        || p.revision == Revision::default()
        || p.revision > d.revision()?
    {
        return Err(StorageError::Database);
    }
    historical_refs(d, state, &p)?;
    Ok(Some(p))
}
fn visit(
    d: &dyn TransactionDocuments,
    prefix: &str,
    mut f: impl FnMut(&str, &Value) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut after = None;
    loop {
        let page = d.scan_prefix(prefix, after.as_deref(), 128)?;
        for (k, v) in page.entries {
            f(&k, &codec::value(&v.value)?)?
        }
        match page.next_key {
            None => return Ok(()),
            Some(next) => {
                if after.as_ref().is_some_and(|old| next <= *old) {
                    return Err(StorageError::Database);
                }
                after = Some(next)
            }
        }
    }
}
pub(super) fn startup_check(
    d: &dyn TransactionDocuments,
    state: &IdentityState,
) -> Result<(), StorageError> {
    let catalog = Catalog::load(d)?;
    if state.installation.is_none() && (catalog.count != 0 || catalog.public) {
        return Err(StorageError::Database);
    }
    load_public(d, state)?;
    let mut seen = std::collections::HashSet::new();
    for page in 0..catalog.count.div_ceil(PAGE_SIZE) {
        for e in read_page(d, page, catalog.count)? {
            if !seen.insert(e.key) {
                return Err(StorageError::Database);
            }
            let p = load_subject(d, state, e.key)?.ok_or(StorageError::Database)?;
            if summary(&p) != e {
                return Err(StorageError::Database);
            }
        }
    }
    let mut docs = 0;
    visit(d, &format!("{ROOT}documents/"), |k, v| {
        docs += 1;
        if docs > MAX_PROFILES {
            return Err(StorageError::Database);
        }
        let p = dto::read_subject(v)?;
        if k != document_key(p.id)
            || !seen.contains(&p.key)
            || load_subject(d, state, p.key)? != Some(p)
        {
            return Err(StorageError::Database);
        }
        Ok(())
    })?;
    let mut slots = 0;
    visit(d, &format!("{ROOT}slots/"), |k, v| {
        slots += 1;
        if slots > MAX_PROFILES {
            return Err(StorageError::Database);
        }
        codec::object_fields(v, &["id"])?;
        let id = dto::id(v, "id", ProfileId::new)?;
        let p = dto::read_subject(&read(d, &document_key(id))?.ok_or(StorageError::Database)?)?;
        if k != slot_key(p.key) || p.id != id || !seen.contains(&p.key) {
            return Err(StorageError::Database);
        }
        Ok(())
    })?;
    let mut pages = 0;
    visit(d, &format!("{ROOT}pages/"), |k, _| {
        if k != page_key(pages) {
            return Err(StorageError::Database);
        }
        pages += 1;
        Ok(())
    })?;
    if docs != catalog.count || slots != catalog.count || pages != catalog.count.div_ceil(PAGE_SIZE)
    {
        return Err(StorageError::Database);
    }
    Ok(())
}

fn authorize(
    state: &IdentityState,
    actor: &ActorContext,
    clock: &dyn Clock,
    session: &SessionGuard,
) -> Result<(), StorageError> {
    session(state, actor)?;
    state
        .authorize_management(actor, clock.now())
        .map_err(StorageError::Identity)
}
fn public_state(layout: &LayoutDocument, display: &DisplaySettings) -> Value {
    json!({"layout":dto::public(layout),"display":display_codec::settings(display)})
}
fn read_public_state(v: &Value) -> Result<fc_ports::PublicLayoutState, StorageError> {
    codec::object_fields(v, &["layout", "display"])?;
    Ok(fc_ports::PublicLayoutState {
        layout: dto::read_public(&v["layout"])?,
        display: display_codec::read_settings(&v["display"])?,
    })
}
fn repository_error(e: StorageError) -> fc_ports::ProfileRepositoryError {
    use fc_ports::ProfileRepositoryError as E;
    match e {
        StorageError::Profile(e) => E::Domain(e),
        StorageError::Identity(e) => E::Domain(ProfileError::Identity(e)),
        StorageError::Display(e) => E::Display(e),
        StorageError::DocumentTooLarge => E::DocumentTooLarge,
        StorageError::CommandIdReused => E::IdempotencyConflict,
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => E::Faulted,
        _ => E::Unavailable,
    }
}
fn receipt_keys(command: CommandId) -> (String, String) {
    (
        format!("{ROOT}receipts/{:020}", command.get()),
        format!("{ROOT}results/{:020}", command.get()),
    )
}
fn fingerprint(kind: &str, actor: &ActorContext, input: &Value) -> Result<Vec<u8>, StorageError> {
    let mut h = Sha256::new();
    h.update(b"future-classroom/profile-command/v1\0");
    h.update(kind.as_bytes());
    h.update(codec::bytes(&codec::actor(actor))?);
    h.update(display::encode(input, MAX_VALUE_BYTES)?);
    Ok(h.finalize().to_vec())
}
fn replay(
    d: &dyn TransactionDocuments,
    command: CommandId,
    kind: &str,
    fingerprint: &[u8],
) -> Result<Option<Vec<u8>>, StorageError> {
    let (r, k) = receipt_keys(command);
    let Some(v) = read(d, &r)? else {
        if d.get(&k)?.is_some() {
            return Err(StorageError::Database);
        }
        return Ok(None);
    };
    codec::object_fields(&v, &["format", "kind", "fingerprint", "result_digest"])?;
    if codec::number(&v, "format")? != 1
        || !matches!(v["kind"].as_str(), Some("public" | "subject"))
    {
        return Err(StorageError::Database);
    }
    let old: Vec<u8> =
        serde_json::from_value(v["fingerprint"].clone()).map_err(|_| StorageError::Database)?;
    let digest: Vec<u8> =
        serde_json::from_value(v["result_digest"].clone()).map_err(|_| StorageError::Database)?;
    if old.len() != 32 || digest.len() != 32 {
        return Err(StorageError::Database);
    }
    if old != fingerprint || v["kind"].as_str() != Some(kind) {
        return Err(StorageError::CommandIdReused);
    }
    let result = d.get(&k)?.ok_or(StorageError::Database)?.value;
    if Sha256::digest(&result).as_slice() != digest {
        return Err(StorageError::Database);
    }
    Ok(Some(result))
}
fn receipt(
    command: CommandId,
    kind: &str,
    fingerprint: Vec<u8>,
    outcome: &[u8],
) -> Result<Vec<Mutation>, StorageError> {
    let (r, k) = receipt_keys(command);
    Ok(vec![
        put(
            r,
            json!({"format":1,"kind":kind,"fingerprint":fingerprint,"result_digest":Sha256::digest(outcome).to_vec()}),
        )?,
        Mutation::Put {
            key: k,
            value: outcome.to_vec(),
        },
    ])
}
fn revision_result(actual: Revision, expected: Revision) -> bool {
    actual == expected || expected.next().ok() == Some(actual)
}
impl<S: Send + 'static> fc_ports::ProfileRepository for BusinessRepository<S> {
    fn list_subject_profiles(
        &self,
        after: Option<ProfileId>,
        actor: &ActorContext,
    ) -> Result<fc_ports::ProfilePage, fc_ports::ProfileRepositoryError> {
        let out = self
            .handle
            .try_transaction(ListProfiles {
                after,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .and_then(|t| t.wait())
            .map_err(repository_error)?;
        read_profile_page(&codec::value(&out).map_err(repository_error)?).map_err(repository_error)
    }
    fn public_layout(
        &self,
        actor: &ActorContext,
    ) -> Result<fc_ports::PublicLayoutState, fc_ports::ProfileRepositoryError> {
        let out = self
            .handle
            .try_transaction(ReadProfile {
                key: None,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .and_then(|t| t.wait())
            .map_err(repository_error)?;
        read_public_state(&codec::value(&out).map_err(repository_error)?).map_err(repository_error)
    }
    fn subject_profile(
        &self,
        key: SubjectProfileKey,
        actor: &ActorContext,
    ) -> Result<Option<SubjectProfileDocument>, fc_ports::ProfileRepositoryError> {
        let out = self
            .handle
            .try_transaction(ReadProfile {
                key: Some(key),
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .and_then(|t| t.wait())
            .map_err(repository_error)?;
        let v = codec::value(&out).map_err(repository_error)?;
        if v.is_null() {
            Ok(None)
        } else {
            dto::read_subject(&v).map(Some).map_err(repository_error)
        }
    }
    fn execute_public_layout(
        &mut self,
        request: fc_ports::PublicLayoutRequest,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<fc_ports::PublicLayoutState, fc_ports::ProfileRepositoryError> {
        let out = self
            .handle
            .try_transaction(SavePublic {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .and_then(|t| t.wait())
            .map_err(repository_error)?;
        read_public_state(&codec::value(&out).map_err(repository_error)?).map_err(repository_error)
    }
    fn execute_subject_profile(
        &mut self,
        request: fc_ports::ProfileRequest,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<SubjectProfileDocument, fc_ports::ProfileRepositoryError> {
        let out = self
            .handle
            .try_transaction(SaveSubject {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .and_then(|t| t.wait())
            .map_err(repository_error)?;
        dto::read_subject(&codec::value(&out).map_err(repository_error)?).map_err(repository_error)
    }
}
struct ReadProfile {
    key: Option<SubjectProfileKey>,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for ReadProfile {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
        let value = match self.key {
            None => public_state(&load_public(d, &state)?, &display::load(d, &state)?),
            Some(k) => load_subject(d, &state, k)?
                .as_ref()
                .map_or(Value::Null, dto::subject),
        };
        let outcome = display::encode(&value, MAX_VALUE_BYTES)?;
        authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}
struct SavePublic<S> {
    request: fc_ports::PublicLayoutRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for SavePublic<S> {
    fn allocation_bytes(&self) -> usize {
        self.request
            .layout
            .allocation_bytes()
            .unwrap_or(usize::MAX)
            .saturating_add(
                self.request
                    .display
                    .components
                    .capacity()
                    .saturating_mul(std::mem::size_of::<fc_domain::desktop::ComponentSettings>()),
            )
            .saturating_add(self.request.display.countdown.label.capacity())
            .saturating_add(std::mem::size_of::<Self>())
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
        self.request
            .layout
            .tree
            .validate_structure()
            .map_err(|e| StorageError::Profile(ProfileError::Layout(e)))?;
        self.request
            .display
            .validate()
            .map_err(StorageError::Display)?;
        let hash = fingerprint(
            "public",
            &self.actor,
            &json!({"expected_layout":self.request.expected_layout_revision.get(),"expected_display":self.request.expected_display_revision.get(),"layout":dto::public(&self.request.layout),"display":display_codec::settings(&self.request.display)}),
        )?;
        if let Some(outcome) = replay(d, self.request.command_id, "public", &hash)? {
            let result = read_public_state(&codec::value(&outcome)?)?;
            if !revision_result(
                result.layout.revision,
                self.request.expected_layout_revision,
            ) || !revision_result(
                result.display.revision,
                self.request.expected_display_revision,
            ) {
                return Err(StorageError::Database);
            }
            authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
            return Ok(TransactionDecision {
                outcome,
                commit: None,
                precommit: None,
            });
        }
        let current = load_public(d, &state)?;
        let old_display = display::load(d, &state)?;
        if old_display.revision != self.request.expected_display_revision {
            return Err(StorageError::Display(
                fc_domain::desktop::DisplayError::RevisionConflict {
                    current: old_display.revision,
                },
            ));
        }
        let layout = current
            .transition(
                self.request.layout.clone(),
                self.request.expected_layout_revision,
                &state,
                &self.actor,
                self.clock.now(),
            )
            .map_err(StorageError::Profile)?;
        let mut display_candidate = self.request.display.clone();
        display_candidate.revision = old_display.revision;
        let settings = if display_candidate == old_display {
            old_display.clone()
        } else {
            old_display
                .transition(
                    display_candidate,
                    self.request.expected_display_revision,
                    &state,
                    &self.actor,
                    self.clock.now(),
                )
                .map_err(StorageError::Display)?
        };
        let outcome = display::encode(&public_state(&layout, &settings), MAX_VALUE_BYTES)?;
        let mut mutations = receipt(self.request.command_id, "public", hash, &outcome)?;
        if layout != current {
            mutations.push(put(PUBLIC.into(), dto::public(&layout))?);
            let mut catalog = Catalog::load(d)?;
            catalog.public = true;
            mutations.push(catalog.mutation()?)
        }
        if settings != old_display {
            mutations.push(display::mutation(&settings)?)
        }
        let snapshot = (self.factory)(
            &state,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let actor = self.actor;
        let clock = self.clock;
        let session = self.session;
        let request = self.request;
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                authorize(&state, &actor, clock.as_ref(), &session)?;
                current
                    .transition(
                        request.layout,
                        request.expected_layout_revision,
                        &state,
                        &actor,
                        clock.now(),
                    )
                    .map_err(StorageError::Profile)?;
                Ok(())
            })),
        })
    }
}
struct SaveSubject<S> {
    request: fc_ports::ProfileRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for SaveSubject<S> {
    fn allocation_bytes(&self) -> usize {
        self.request
            .candidate
            .allocation_bytes()
            .unwrap_or(usize::MAX)
            .saturating_add(std::mem::size_of::<Self>())
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
        let current = load_subject(d, &state, self.request.key)?;
        let mut candidate = self.request.candidate.clone();
        if candidate.key != self.request.key {
            return Err(StorageError::InvalidCommand);
        }
        // A draft for a new slot may carry a temporary UI id. Once committed,
        // the stable document identity is part of the captured edit contract.
        if self.request.expected_revision != Revision::default()
            && current.as_ref().is_some_and(|old| old.id != candidate.id)
        {
            return Err(StorageError::Profile(ProfileError::IdMismatch));
        }
        let mut catalog = Catalog::load(d)?;
        candidate.id = match &current {
            Some(p) => p.id,
            None => ProfileId::new(catalog.count + 1).map_err(|_| StorageError::InvalidCommand)?,
        };
        candidate
            .tree
            .validate_structure()
            .map_err(|e| StorageError::Profile(ProfileError::Layout(e)))?;
        candidate
            .materials
            .validate()
            .map_err(StorageError::Profile)?;
        let hash = fingerprint(
            "subject",
            &self.actor,
            &json!({"key":dto::key(self.request.key),"expected":self.request.expected_revision.get(),"candidate":dto::subject(&candidate)}),
        )?;
        let owner_folder = folder(d, self.request.key)?;
        if let Some(outcome) = replay(d, self.request.command_id, "subject", &hash)? {
            let result = dto::read_subject(&codec::value(&outcome)?)?;
            if result.id != candidate.id
                || result.key != self.request.key
                || !revision_result(result.revision, self.request.expected_revision)
            {
                return Err(StorageError::Database);
            }
            historical_refs(d, &state, &result)?;
            result
                .validate_references(&state, owner_folder.as_ref())
                .map_err(StorageError::Profile)?;
            authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
            return Ok(TransactionDecision {
                outcome,
                commit: None,
                precommit: None,
            });
        }
        let next = SubjectProfileDocument::prepare_transition(
            current.as_ref(),
            candidate.clone(),
            self.request.expected_revision,
            &state,
            &self.actor,
            self.clock.now(),
            owner_folder.as_ref(),
        )
        .map_err(StorageError::Profile)?;
        if next.enabled {
            load_public(d, &state)?
                .tree
                .validate_composition(Some(&next.tree))
                .map_err(|e| StorageError::Profile(ProfileError::Layout(e)))?;
        }
        historical_refs(d, &state, &next)?;
        let mut mutations = index_mutations(d, &mut catalog, &next, current.is_none())?;
        let outcome = display::encode(&dto::subject(&next), MAX_VALUE_BYTES)?;
        mutations.push(Mutation::Put {
            key: document_key(next.id),
            value: outcome.clone(),
        });
        mutations.extend(receipt(self.request.command_id, "subject", hash, &outcome)?);
        let snapshot = (self.factory)(
            &state,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let request = self.request;
        let actor = self.actor;
        let session = self.session;
        let clock = self.clock;
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                authorize(&state, &actor, clock.as_ref(), &session)?;
                SubjectProfileDocument::prepare_transition(
                    current.as_ref(),
                    candidate,
                    request.expected_revision,
                    &state,
                    &actor,
                    clock.now(),
                    owner_folder.as_ref(),
                )
                .map_err(StorageError::Profile)?;
                Ok(())
            })),
        })
    }
}

fn read_profile_page(v: &Value) -> Result<fc_ports::ProfilePage, StorageError> {
    codec::object_fields(v, &["items", "total", "next_after"])?;
    let items = codec::array(v, "items")?;
    let total = codec::number(v, "total")?;
    if items.len() > fc_ports::PROFILE_PAGE_SIZE || total > MAX_PROFILES {
        return Err(StorageError::Database);
    }
    let items = items.iter().map(entry).collect::<Result<Vec<_>, _>>()?;
    if items.windows(2).any(|w| w[0].id >= w[1].id) || items.iter().any(|e| e.id.get() > total) {
        return Err(StorageError::Database);
    }
    let next_after = if v["next_after"].is_null() {
        None
    } else {
        Some(dto::id(v, "next_after", ProfileId::new)?)
    };
    if let Some(next) = next_after {
        if items.last().map(|e| e.id) != Some(next) || next.get() >= total {
            return Err(StorageError::Database);
        }
    }
    Ok(fc_ports::ProfilePage {
        items,
        total,
        next_after,
    })
}
struct ListProfiles {
    after: Option<ProfileId>,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for ListProfiles {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
        let catalog = Catalog::load(d)?;
        let start = self.after.map_or(0, ProfileId::get).min(catalog.count);
        let end = start
            .saturating_add(fc_ports::PROFILE_PAGE_SIZE as u64)
            .min(catalog.count);
        let mut items = Vec::new();
        if start < end {
            for page in start / PAGE_SIZE..=(end - 1) / PAGE_SIZE {
                for e in read_page(d, page, catalog.count)? {
                    if e.id.get() > start && e.id.get() <= end {
                        items.push(summary_value(&e))
                    }
                }
            }
        }
        let next_after = (end < catalog.count).then_some(end);
        let outcome = display::encode(
            &json!({"items":items,"total":catalog.count,"next_after":next_after}),
            MAX_VALUE_BYTES,
        )?;
        authorize(&state, &self.actor, self.clock.as_ref(), &self.session)?;
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}

pub(super) fn select(
    d: &dyn TransactionDocuments,
    state: &IdentityState,
    context: &fc_domain::lesson_context::LessonContextSnapshot,
    now: fc_domain::time::LocalDateTime,
) -> Result<ProfileSelectionSnapshot, StorageError> {
    let mut selected = ProfileSelectionSnapshot {
        public_layout: load_public(d, state)?,
        subject_profile: None,
        reason: ProfileSelectionReason::Public,
    };
    let Some(current) = &context.current else {
        return Ok(selected);
    };
    let Some(teacher_id) = current.teacher_id else {
        selected.reason = ProfileSelectionReason::Unassigned;
        return Ok(selected);
    };
    let key = SubjectProfileKey {
        class_id: current.key.class_id,
        subject_id: current.effective.subject_id,
        teacher_id,
    };
    if !state.is_teacher(teacher_id, key.class_id, key.subject_id) {
        selected.reason = ProfileSelectionReason::InvalidAssignment;
        return Ok(selected);
    }
    let Some(profile) = load_subject(d, state, key)? else {
        selected.reason = ProfileSelectionReason::Missing;
        return Ok(selected);
    };
    if !profile.enabled {
        selected.reason = ProfileSelectionReason::Disabled;
        return Ok(selected);
    }
    let folder = folder(d, key)?;
    profile
        .validate_active(state, folder.as_ref())
        .map_err(StorageError::Profile)?;
    if selected
        .public_layout
        .tree
        .validate_composition(Some(&profile.tree))
        .is_err()
    {
        selected.reason = ProfileSelectionReason::LayoutMismatch;
        return Ok(selected);
    }
    selected.reason = ProfileSelectionReason::Selected;
    selected.subject_profile = Some(profile);
    selected
        .validate(state, context, now, folder.as_ref())
        .map_err(StorageError::Profile)?;
    Ok(selected)
}

#[cfg(test)]
mod tests;
