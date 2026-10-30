//! Independent owner registry, shared primary-original quota with Materials.
//! Settle is a trusted worker-exit notification, never a timeout/TTL operation.
use super::*;
use std::collections::BTreeMap;
const ROOT: &str = "business/profile-media/v1/";
const CATALOG: &str = "business/profile-media/v1/catalog";
const MAX_MEDIA: u64 = 8192;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaState {
    Reserved,
    Referenced,
    Unreferenced,
    Retiring,
    Removed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaEntry {
    pub id: MediaVersionId,
    pub owner: ContentSlotKey,
    pub command_id: CommandId,
    pub charged_bytes: u64,
    pub resource: Option<MediaReference>,
    pub revision: Revision,
    pub refs: u64,
    pub state: MediaState,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaReservation {
    pub admission: ContentAdmission,
    pub ids: Vec<MediaVersionId>,
    pub bytes: Vec<u64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaRetirement {
    pub id: MediaVersionId,
    pub revision: Revision,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaInventory {
    pub items: Vec<MediaEntry>,
    pub next_after: Option<MediaVersionId>,
}
fn key(id: MediaVersionId) -> String {
    format!("{ROOT}entries/{:020}", id.get())
}
fn name(s: MediaState) -> &'static str {
    match s {
        MediaState::Reserved => "reserved",
        MediaState::Referenced => "referenced",
        MediaState::Unreferenced => "unreferenced",
        MediaState::Retiring => "retiring",
        MediaState::Removed => "removed",
    }
}
fn entry(v: &Value) -> Result<MediaEntry, StorageError> {
    codec::object_fields(
        v,
        &[
            "format", "id", "owner", "command", "bytes", "resource", "revision", "refs", "state",
        ],
    )?;
    let e = MediaEntry {
        id: profiles_codec::id(v, "id", MediaVersionId::new)?,
        owner: dto::read_slot(&v["owner"])?,
        command_id: profiles_codec::id(v, "command", CommandId::new)?,
        charged_bytes: codec::number(v, "bytes")?,
        resource: if v["resource"].is_null() {
            None
        } else {
            Some(dto::read_media(&v["resource"])?)
        },
        revision: Revision::new(codec::number(v, "revision")?),
        refs: codec::number(v, "refs")?,
        state: match codec::text(v, "state")?.as_str() {
            "reserved" => MediaState::Reserved,
            "referenced" => MediaState::Referenced,
            "unreferenced" => MediaState::Unreferenced,
            "retiring" => MediaState::Retiring,
            "removed" => MediaState::Removed,
            _ => return Err(StorageError::Database),
        },
    };
    if codec::number(v, "format")? != 1
        || e.revision == Revision::default()
        || e.charged_bytes == 0
        || e.charged_bytes > MAX_CONTENT_IMAGE_BYTES
        || e.resource.as_ref().is_some_and(|r| {
            r.owner != e.owner || r.version_id != e.id || r.bytes > e.charged_bytes
        })
        || (e.refs > 0) != (e.state == MediaState::Referenced)
        || e.state == MediaState::Referenced && e.resource.is_none()
    {
        return Err(StorageError::Database);
    }
    Ok(e)
}
fn value(e: &MediaEntry) -> Value {
    json!({"format":1,"id":e.id.get(),"owner":dto::slot(e.owner),"command":e.command_id.get(),"bytes":e.charged_bytes,"resource":e.resource.as_ref().map(dto::media),"revision":e.revision.get(),"refs":e.refs,"state":name(e.state)})
}
fn load(d: &dyn TransactionDocuments, id: MediaVersionId) -> Result<MediaEntry, StorageError> {
    let e = entry(&read(d, &key(id))?.ok_or(StorageError::Database)?)?;
    if e.id != id || e.revision > d.revision()? {
        return Err(StorageError::Database);
    }
    Ok(e)
}
fn catalog(d: &dyn TransactionDocuments) -> Result<(u64, u64), StorageError> {
    let Some(v) = read(d, CATALOG)? else {
        if !d.scan_prefix(ROOT, None, 1)?.entries.is_empty() {
            return Err(StorageError::Database);
        }
        return Ok((0, 0));
    };
    codec::object_fields(&v, &["format", "count", "bytes"])?;
    let c = codec::number(&v, "count")?;
    let b = codec::number(&v, "bytes")?;
    if codec::number(&v, "format")? != 1 || c > MAX_MEDIA || b > MAX_CONTENT_MEDIA_BYTES {
        return Err(StorageError::Database);
    }
    Ok((c, b))
}
pub(super) fn primary_bytes(d: &dyn TransactionDocuments) -> Result<u64, StorageError> {
    catalog(d).map(|c| c.1)
}
fn catalog_put(c: u64, b: u64) -> Result<Mutation, StorageError> {
    put(CATALOG.into(), json!({"format":1,"count":c,"bytes":b}))
}
fn mutation(e: &MediaEntry) -> Result<Mutation, StorageError> {
    put(key(e.id), value(e))
}
fn refs(doc: &ProfileContentDocument) -> BTreeMap<MediaVersionId, (u64, MediaReference)> {
    let mut counts = BTreeMap::new();
    for v in [&doc.current, &doc.previous].into_iter().flatten() {
        if let ContentPayload::Images(a) = &v.payload {
            for r in a {
                let e = counts.entry(r.version_id).or_insert((0, r.clone()));
                e.0 += 1;
            }
        }
    }
    counts
}
pub(super) fn update_refs(
    d: &dyn TransactionDocuments,
    old: &ProfileContentDocument,
    next: &ProfileContentDocument,
    ad: Option<&ContentAdmission>,
) -> Result<Vec<Mutation>, StorageError> {
    let old_refs = refs(old);
    let new_refs = refs(next);
    let mut ids = old_refs
        .keys()
        .chain(new_refs.keys())
        .copied()
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    let mut out = Vec::new();
    for id in ids {
        let mut e = load(d, id)?;
        let old_count = old_refs.get(&id).map_or(0, |r| r.0);
        let new_count = new_refs.get(&id).map_or(0, |r| r.0);
        if e.owner != old.slot
            || e.refs != old_count
            || matches!(e.state, MediaState::Retiring | MediaState::Removed)
        {
            return Err(StorageError::Database);
        }
        if let Some((_, r)) = new_refs.get(&id) {
            match &e.resource {
                Some(actual) if actual != r => return Err(StorageError::Database),
                None => {
                    if !ad.is_some_and(|a| {
                        e.command_id == a.request.command_id && e.owner == a.request.slot
                    }) || e.state != MediaState::Reserved
                        || r.bytes > e.charged_bytes
                    {
                        return Err(StorageError::ResourceQuota);
                    }
                    e.resource = Some(r.clone());
                }
                _ => {}
            }
        }
        e.refs = new_count;
        e.state = if new_count > 0 {
            MediaState::Referenced
        } else {
            MediaState::Unreferenced
        };
        e.revision = e
            .revision
            .next()
            .map_err(|_| StorageError::InvalidCommand)?;
        out.push(mutation(&e)?);
    }
    Ok(out)
}
#[derive(Clone)]
enum MediaOp {
    Reserve(ContentAdmission, Vec<u64>),
    Settle(MediaReservation),
    RestartSettle(MediaVersionId, Revision),
    Inventory(Option<MediaVersionId>, usize),
    Retire(MediaVersionId, Revision),
    Finalize(MediaRetirement),
}
struct MediaTask<S> {
    op: MediaOp,
    actor: Option<ActorContext>,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
}
impl<S: Send + 'static> BusinessRepository<S> {
    fn media_task(&self, op: MediaOp, actor: Option<&ActorContext>) -> Result<Value, Error> {
        let b = self
            .handle
            .try_transaction(MediaTask {
                op,
                actor: actor.cloned(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
            })
            .and_then(|t| t.wait())
            .map_err(err)?;
        codec::value_with_limit(&b, 512 * 1024).map_err(err)
    }
    pub fn reserve_profile_media(
        &self,
        admission: &ContentAdmission,
        bytes: &[u64],
        actor: &ActorContext,
    ) -> Result<MediaReservation, Error> {
        if bytes.is_empty()
            || bytes.len() > MAX_CONTENT_PAGES
            || bytes
                .iter()
                .any(|b| *b == 0 || *b > MAX_CONTENT_IMAGE_BYTES)
        {
            return Err(Error::Domain(ProfileContentError::InvalidMedia));
        }
        let v = self.media_task(
            MediaOp::Reserve(admission.clone(), bytes.to_vec()),
            Some(actor),
        )?;
        let ids = codec::array(&v, "ids")
            .map_err(err)?
            .iter()
            .map(|v| {
                MediaVersionId::new(v.as_u64().ok_or(StorageError::Database)?)
                    .map_err(|_| StorageError::Database)
            })
            .collect::<Result<_, _>>()
            .map_err(err)?;
        Ok(MediaReservation {
            admission: admission.clone(),
            ids,
            bytes: bytes.to_vec(),
        })
    }
    pub fn settle_profile_media(&self, reservation: MediaReservation) -> Result<(), Error> {
        self.media_task(MediaOp::Settle(reservation), None)
            .map(|_| ())
    }
    pub fn inventory_profile_media(
        &self,
        after: Option<MediaVersionId>,
        limit: usize,
    ) -> Result<MediaInventory, Error> {
        let v = self.media_task(MediaOp::Inventory(after, limit), None)?;
        let items = codec::array(&v, "items")
            .map_err(err)?
            .iter()
            .map(entry)
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let next_after = if v["next"].is_null() {
            None
        } else {
            Some(profiles_codec::id(&v, "next", MediaVersionId::new).map_err(err)?)
        };
        Ok(MediaInventory { items, next_after })
    }
    /// Startup only: caller owns exclusive instance lock and has started no
    /// preparation workers. This is a physical-owner handoff, never TTL expiry.
    pub fn settle_profile_media_after_restart(
        &self,
        id: MediaVersionId,
        expected: Revision,
    ) -> Result<(), Error> {
        self.media_task(MediaOp::RestartSettle(id, expected), None)
            .map(|_| ())
    }
    pub fn retire_profile_media(
        &self,
        id: MediaVersionId,
        expected: Revision,
    ) -> Result<MediaRetirement, Error> {
        let v = self.media_task(MediaOp::Retire(id, expected), None)?;
        Ok(MediaRetirement {
            id,
            revision: Revision::new(codec::number(&v, "revision").map_err(err)?),
        })
    }
    pub fn finalize_profile_media_removal(&self, t: MediaRetirement) -> Result<(), Error> {
        self.media_task(MediaOp::Finalize(t), None).map(|_| ())
    }
}
impl<S: Send> TransactionTask<S> for MediaTask<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match &self.op {
                MediaOp::Reserve(_, b) => b.capacity() * 8,
                MediaOp::Settle(r) => (r.ids.capacity() + r.bytes.capacity()) * 8,
                _ => 0,
            }
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let i = load_identity(d)?;
        if let Some(a) = &self.actor {
            authorize(&i, a, self.clock.as_ref(), &self.session)?
        }
        let (count, bytes) = catalog(d)?;
        let mut writes = Vec::new();
        let v = match &self.op {
            MediaOp::Reserve(ad, sizes) => {
                let (doc, retired, _) = super::load(d, ad.request.slot)?;
                if retired
                    || doc.pending.as_ref() != Some(ad)
                    || self.actor.as_ref().is_none_or(|a| {
                        a.principal_id != ad.actor_id || a.auth_epoch != ad.auth_epoch
                    })
                    || self.clock.now() >= ad.request.expires_at
                    || ad.request.operation != ContentOperation::Publish(ContentPayloadKind::Images)
                {
                    return Err(pe(ProfileContentError::Superseded));
                }
                let (public, subject) = target(d, &i, ad.request.slot)?;
                ContentTarget {
                    public: &public,
                    subject: subject.as_ref(),
                }
                .kind(ad.request.slot, &i)
                .map_err(pe)?;
                if sizes.is_empty()
                    || sizes.len() > MAX_CONTENT_PAGES
                    || sizes
                        .iter()
                        .any(|b| *b == 0 || *b > MAX_CONTENT_IMAGE_BYTES)
                {
                    return Err(pe(ProfileContentError::InvalidMedia));
                }
                let rk = format!("{ROOT}reservations/{:020}", ad.request.command_id.get());
                if let Some(v) = read(d, &rk)? {
                    codec::object_fields(&v, &["admission", "sizes", "ids"])?;
                    if dto::read_admission(&v["admission"])? != *ad || v["sizes"] != json!(sizes) {
                        return Err(StorageError::CommandIdReused);
                    }
                    json!({"ids":v["ids"]})
                } else {
                    let additional = sizes
                        .iter()
                        .try_fold(0u64, |s, b| s.checked_add(*b))
                        .ok_or(StorageError::ResourceQuota)?;
                    let total = bytes
                        .checked_add(additional)
                        .ok_or(StorageError::ResourceQuota)?;
                    if count + sizes.len() as u64 > MAX_MEDIA
                        || total
                            .checked_add(materials::primary_bytes(d)?)
                            .is_none_or(|n| n > MAX_CONTENT_MEDIA_BYTES)
                    {
                        return Err(StorageError::ResourceQuota);
                    }
                    let mut ids = Vec::new();
                    for (n, b) in sizes.iter().enumerate() {
                        let id = MediaVersionId::new(count + n as u64 + 1)
                            .map_err(|_| StorageError::InvalidCommand)?;
                        let e = MediaEntry {
                            id,
                            owner: ad.request.slot,
                            command_id: ad.request.command_id,
                            charged_bytes: *b,
                            resource: None,
                            revision: Revision::new(1),
                            refs: 0,
                            state: MediaState::Reserved,
                        };
                        writes.push(mutation(&e)?);
                        ids.push(id.get());
                    }
                    writes.push(catalog_put(count + sizes.len() as u64, total)?);
                    writes.push(put(
                        rk,
                        json!({"admission":dto::admission(ad),"sizes":sizes,"ids":ids}),
                    )?);
                    json!({"ids":ids})
                }
            }
            MediaOp::Settle(r) => {
                let rk = format!(
                    "{ROOT}reservations/{:020}",
                    r.admission.request.command_id.get()
                );
                let v = read(d, &rk)?.ok_or(StorageError::Database)?;
                codec::object_fields(&v, &["admission", "sizes", "ids"])?;
                if dto::read_admission(&v["admission"])? != r.admission
                    || v["sizes"] != json!(r.bytes)
                    || v["ids"] != json!(r.ids.iter().map(|i| i.get()).collect::<Vec<_>>())
                {
                    return Err(StorageError::CommandIdReused);
                }
                for id in &r.ids {
                    let mut e = load(d, *id)?;
                    if e.state == MediaState::Reserved {
                        e.state = MediaState::Unreferenced;
                        e.revision = e
                            .revision
                            .next()
                            .map_err(|_| StorageError::InvalidCommand)?;
                        writes.push(mutation(&e)?);
                    }
                }
                Value::Null
            }
            MediaOp::Inventory(after, limit) => {
                if *limit == 0 || *limit > 128 {
                    return Err(StorageError::InvalidCommand);
                }
                let after = after.map(key);
                let p = d.scan_prefix(&format!("{ROOT}entries/"), after.as_deref(), *limit)?;
                let mut items = Vec::new();
                for (k, r) in p.entries {
                    let e = entry(&codec::value(&r.value)?)?;
                    if key(e.id) != k {
                        return Err(StorageError::Database);
                    }
                    items.push(e);
                }
                let next = if p.next_key.is_some() {
                    items.last().map(|e| e.id.get())
                } else {
                    None
                };
                json!({"items":items.iter().map(value).collect::<Vec<_>>(),"next":next})
            }
            MediaOp::RestartSettle(id, expected) => {
                let mut e = load(d, *id)?;
                if e.revision != *expected {
                    return Err(StorageError::Conflict {
                        current: e.revision,
                    });
                }
                if e.state == MediaState::Reserved {
                    let mut r = receipt(d, e.command_id)?.ok_or(StorageError::Database)?;
                    if r.status == Status::Accepted {
                        let (doc, retired, _) = super::load(d, e.owner)?;
                        if !retired && doc.pending.as_ref() == Some(&r.admission) {
                            writes.push(head(&doc.fail(&r.admission).map_err(pe)?, false)?);
                        }
                        r.status = Status::Cancelled;
                        writes.push(r.mutation()?);
                    }
                    e.state = MediaState::Unreferenced;
                    e.revision = e
                        .revision
                        .next()
                        .map_err(|_| StorageError::InvalidCommand)?;
                    writes.push(mutation(&e)?);
                }
                Value::Null
            }
            MediaOp::Retire(id, expected) => {
                let mut e = load(d, *id)?;
                if e.revision != *expected {
                    return Err(StorageError::Conflict {
                        current: e.revision,
                    });
                }
                if e.refs > 0 || !matches!(e.state, MediaState::Unreferenced | MediaState::Retiring)
                {
                    return Err(StorageError::AuthorityUnavailable);
                }
                if e.state != MediaState::Retiring {
                    e.state = MediaState::Retiring;
                    e.revision = e
                        .revision
                        .next()
                        .map_err(|_| StorageError::InvalidCommand)?;
                    writes.push(mutation(&e)?);
                }
                json!({"revision":e.revision.get()})
            }
            MediaOp::Finalize(t) => {
                let mut e = load(d, t.id)?;
                if e.state != MediaState::Removed {
                    if e.revision != t.revision || e.state != MediaState::Retiring || e.refs > 0 {
                        return Err(StorageError::AuthorityUnavailable);
                    }
                    e.state = MediaState::Removed;
                    e.revision = e
                        .revision
                        .next()
                        .map_err(|_| StorageError::InvalidCommand)?;
                    writes.push(mutation(&e)?);
                    writes.push(catalog_put(
                        count,
                        bytes
                            .checked_sub(e.charged_bytes)
                            .ok_or(StorageError::Database)?,
                    )?);
                }
                Value::Null
            }
        };
        let commit = if writes.is_empty() {
            None
        } else {
            Some((
                writes,
                (self.factory)(
                    &i,
                    d.revision()?
                        .next()
                        .map_err(|_| StorageError::InvalidCommand)?,
                )?,
            ))
        };
        let actor = self.actor;
        let session = self.session;
        let clock = self.clock;
        let expires = match self.op {
            MediaOp::Reserve(a, _) => Some(a.request.expires_at),
            _ => None,
        };
        Ok(TransactionDecision {
            outcome: display::encode(&v, 512 * 1024)?,
            commit,
            precommit: Some(Box::new(move || {
                if let Some(a) = actor {
                    authorize(&i, &a, clock.as_ref(), &session)?;
                }
                if expires.is_some_and(|t| clock.now() >= t) {
                    return Err(pe(ProfileContentError::Expired));
                }
                Ok(())
            })),
        })
    }
}
pub(super) fn startup_check(
    d: &dyn TransactionDocuments,
    expected: &BTreeMap<MediaVersionId, (u64, MediaReference)>,
) -> Result<(), StorageError> {
    let (count, bytes) = catalog(d)?;
    if bytes
        .checked_add(materials::primary_bytes(d)?)
        .is_none_or(|n| n > MAX_CONTENT_MEDIA_BYTES)
    {
        return Err(StorageError::Database);
    }
    let mut actual_bytes = 0u64;
    for n in 1..=count {
        let e = load(
            d,
            MediaVersionId::new(n).map_err(|_| StorageError::Database)?,
        )?;
        let refs = expected.get(&e.id);
        if e.refs != refs.map_or(0, |v| v.0)
            || refs.is_some_and(|(_, r)| e.resource.as_ref() != Some(r))
        {
            return Err(StorageError::Database);
        }
        if e.state != MediaState::Removed {
            actual_bytes = actual_bytes
                .checked_add(e.charged_bytes)
                .ok_or(StorageError::Database)?;
        }
    }
    if actual_bytes != bytes || expected.keys().any(|id| id.get() > count) {
        return Err(StorageError::Database);
    }
    let mut after = None;
    let mut rows = 0;
    loop {
        let p = d.scan_prefix(&format!("{ROOT}entries/"), after.as_deref(), 128)?;
        for (k, r) in p.entries {
            let e = entry(&codec::value(&r.value)?)?;
            if k != key(e.id) || e.id.get() > count {
                return Err(StorageError::Database);
            }
            rows += 1;
            if rows > count {
                return Err(StorageError::Database);
            }
        }
        after = p.next_key;
        if after.is_none() {
            break;
        }
    }
    if rows != count {
        return Err(StorageError::Database);
    }
    let mut declared = std::collections::HashSet::new();
    let mut after = None;
    loop {
        let p = d.scan_prefix(ROOT, after.as_deref(), 128)?;
        for (k, row) in p.entries {
            if k == CATALOG || k.starts_with(&format!("{ROOT}entries/")) {
                continue;
            }
            if !k.starts_with(&format!("{ROOT}reservations/")) {
                return Err(StorageError::Database);
            }
            let v = codec::value(&row.value)?;
            codec::object_fields(&v, &["admission", "sizes", "ids"])?;
            let a = dto::read_admission(&v["admission"])?;
            if k != format!("{ROOT}reservations/{:020}", a.request.command_id.get()) {
                return Err(StorageError::Database);
            }
            let receipt = receipt(d, a.request.command_id)?.ok_or(StorageError::Database)?;
            if receipt.admission != a {
                return Err(StorageError::Database);
            }
            let ids = codec::array(&v, "ids")?;
            let sizes = codec::array(&v, "sizes")?;
            if ids.is_empty() || ids.len() > MAX_CONTENT_PAGES || ids.len() != sizes.len() {
                return Err(StorageError::Database);
            }
            for (id, size) in ids.iter().zip(sizes) {
                let id = MediaVersionId::new(id.as_u64().ok_or(StorageError::Database)?)
                    .map_err(|_| StorageError::Database)?;
                if declared.len() as u64 >= count || !declared.insert(id) {
                    return Err(StorageError::Database);
                }
                let e = load(d, id)?;
                if e.command_id != a.request.command_id
                    || e.owner != a.request.slot
                    || Some(e.charged_bytes) != size.as_u64()
                {
                    return Err(StorageError::Database);
                }
            }
        }
        after = p.next_key;
        if after.is_none() {
            break;
        }
    }
    if declared.len() as u64 != count {
        return Err(StorageError::Database);
    }
    Ok(())
}
