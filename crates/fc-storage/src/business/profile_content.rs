//! Sole-writer per-node publication. No file operations, KDF or writer reentry.
//! Heads contain immutable version references; reservations remain charged until
//! trusted physical deletion has completed and finalize commits.
use super::{profile_content_codec as dto, *};
use fc_domain::{layout::*, profile::*, profile_content::*, time::LocalDateTime};
use fc_ports::{
    ProfileContentOutcome, ProfileContentRepository, ProfileContentRepositoryError as Error,
    ProfileContentStatus as Status,
};
use sha2::{Digest, Sha256};
#[path = "profile_content_media.rs"]
mod media;
pub use media::{MediaEntry, MediaInventory, MediaReservation, MediaRetirement, MediaState};
const ROOT: &str = "business/profile-content/v1/";
const COUNTER: &str = "business/profile-content/v1/counter";
const MAX_HEADS: u64 = 16384;
const MAX_RECEIPTS: u64 = 65536;
type PageRule = Box<dyn FnOnce(LocalDateTime) -> Result<(), StorageError> + Send>;
pub type ProfileContentSceneGuard = Arc<
    dyn Fn(ContentSlotKey, Revision, Revision, Revision) -> Result<(), StorageError> + Send + Sync,
>;
fn err(e: StorageError) -> Error {
    match e {
        StorageError::ProfileContent(x) => Error::Domain(x),
        StorageError::Identity(x) => Error::Domain(ProfileContentError::Identity(x)),
        StorageError::ResourceQuota => Error::QuotaExceeded,
        StorageError::CommandIdReused => Error::IdempotencyConflict,
        StorageError::DocumentTooLarge => Error::DocumentTooLarge,
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => Error::Faulted,
        _ => Error::Unavailable,
    }
}
fn pe(e: ProfileContentError) -> StorageError {
    StorageError::ProfileContent(e)
}
fn suffix(s: ContentSlotKey) -> String {
    match s.scope {
        LayoutScope::Public => format!("public/{:020}", s.node_id.get()),
        LayoutScope::Subject(k) => format!(
            "subject/{:020}/{:020}/{:020}/{:020}",
            k.class_id.get(),
            k.subject_id.get(),
            k.teacher_id.get(),
            s.node_id.get()
        ),
    }
}
fn head_key(s: ContentSlotKey) -> String {
    format!("{ROOT}heads/{}", suffix(s))
}
fn index_key(s: ContentSlotKey) -> String {
    format!("{ROOT}slots/{}", suffix(s))
}
fn version_key(s: ContentSlotKey, r: Revision) -> String {
    format!("{ROOT}versions/{}/{:020}", suffix(s), r.get())
}
fn receipt_key(c: CommandId) -> String {
    format!("{ROOT}receipts/{:020}", c.get())
}
fn read(d: &dyn TransactionDocuments, k: &str) -> Result<Option<Value>, StorageError> {
    d.get(k)?.map(|r| codec::value(&r.value)).transpose()
}
fn put(k: String, v: Value) -> Result<Mutation, StorageError> {
    Ok(Mutation::Put {
        key: k,
        value: display::encode(&v, MAX_VALUE_BYTES)?,
    })
}
fn counter(d: &dyn TransactionDocuments) -> Result<(u64, u64), StorageError> {
    let Some(v) = read(d, COUNTER)? else {
        if !d.scan_prefix(ROOT, None, 1)?.entries.is_empty() {
            return Err(StorageError::Database);
        }
        return Ok((0, 0));
    };
    codec::object_fields(&v, &["format", "heads", "receipts"])?;
    let h = codec::number(&v, "heads")?;
    let r = codec::number(&v, "receipts")?;
    if codec::number(&v, "format")? != 1 || h > MAX_HEADS || r > MAX_RECEIPTS {
        return Err(StorageError::Database);
    }
    Ok((h, r))
}
fn counter_put(h: u64, r: u64) -> Result<Mutation, StorageError> {
    put(COUNTER.into(), json!({"format":1,"heads":h,"receipts":r}))
}
fn authorize(
    i: &IdentityState,
    a: &ActorContext,
    c: &dyn Clock,
    g: &SessionGuard,
) -> Result<(), StorageError> {
    g(i, a)?;
    i.authorize_management(a, c.now())
        .map_err(StorageError::Identity)
}
fn target(
    d: &dyn TransactionDocuments,
    i: &IdentityState,
    s: ContentSlotKey,
) -> Result<(LayoutDocument, Option<SubjectProfileDocument>), StorageError> {
    Ok((
        profiles::load_public(d, i)?,
        match s.scope {
            LayoutScope::Public => None,
            LayoutScope::Subject(k) => {
                Some(profiles::load_subject(d, i, k)?.ok_or(pe(ProfileContentError::InvalidSlot))?)
            }
        },
    ))
}
fn load_version(
    d: &dyn TransactionDocuments,
    s: ContentSlotKey,
    r: Revision,
) -> Result<ContentVersion, StorageError> {
    let v = read(d, &version_key(s, r))?.ok_or(StorageError::Database)?;
    codec::object_fields(&v, &["format", "slot", "version"])?;
    if codec::number(&v, "format")? != 1 || dto::read_slot(&v["slot"])? != s {
        return Err(StorageError::Database);
    }
    let v = dto::read_version(&v["version"], s)?;
    if v.revision != r || v.revision > d.revision()? {
        return Err(StorageError::Database);
    }
    Ok(v)
}
fn load(
    d: &dyn TransactionDocuments,
    s: ContentSlotKey,
) -> Result<(ProfileContentDocument, bool, bool), StorageError> {
    let v = read(d, &head_key(s))?;
    let index = read(d, &index_key(s))?;
    if v.is_some() != index.is_some() {
        return Err(StorageError::Database);
    }
    let Some(v) = v else {
        return Ok((ProfileContentDocument::new(s), false, false));
    };
    if index != Some(dto::slot(s)) {
        return Err(StorageError::Database);
    }
    codec::object_fields(
        &v,
        &[
            "format", "slot", "revision", "sequence", "view", "page", "current", "previous",
            "pending", "retired",
        ],
    )?;
    if codec::number(&v, "format")? != 1 || dto::read_slot(&v["slot"])? != s {
        return Err(StorageError::Database);
    }
    let retired = codec::boolean(&v, "retired")?;
    let doc = ProfileContentDocument {
        slot: s,
        revision: Revision::new(codec::number(&v, "revision")?),
        latest_publish_sequence: codec::number(&v, "sequence")?,
        view_revision: Revision::new(codec::number(&v, "view")?),
        current_page: u8::try_from(codec::number(&v, "page")?)
            .map_err(|_| StorageError::Database)?,
        current: if v["current"].is_null() {
            None
        } else {
            Some(load_version(
                d,
                s,
                Revision::new(codec::number(&v, "current")?),
            )?)
        },
        previous: if v["previous"].is_null() {
            None
        } else {
            Some(load_version(
                d,
                s,
                Revision::new(codec::number(&v, "previous")?),
            )?)
        },
        pending: if v["pending"].is_null() {
            None
        } else {
            Some(dto::read_admission(&v["pending"])?)
        },
    };
    if retired {
        if doc.current.is_some() || doc.previous.is_some() || doc.pending.is_some() {
            return Err(StorageError::Database);
        }
    } else {
        doc.validate_structure()
            .map_err(|_| StorageError::Database)?;
    }
    if doc.revision > d.revision()?
        || doc.view_revision > d.revision()?
        || doc.latest_publish_sequence > d.revision()?.get()
    {
        return Err(StorageError::Database);
    }
    Ok((doc, retired, true))
}
fn head(doc: &ProfileContentDocument, retired: bool) -> Result<Mutation, StorageError> {
    put(
        head_key(doc.slot),
        json!({"format":1,"slot":dto::slot(doc.slot),"revision":doc.revision.get(),"sequence":doc.latest_publish_sequence,"view":doc.view_revision.get(),"page":doc.current_page,"current":doc.current.as_ref().map(|v|v.revision.get()),"previous":doc.previous.as_ref().map(|v|v.revision.get()),"pending":doc.pending.as_ref().map(dto::admission),"retired":retired}),
    )
}
fn document_value(doc: &ProfileContentDocument) -> Value {
    json!({"slot":dto::slot(doc.slot),"revision":doc.revision.get(),"sequence":doc.latest_publish_sequence,"view":doc.view_revision.get(),"page":doc.current_page,"current":doc.current.as_ref().map(dto::version),"previous":doc.previous.as_ref().map(dto::version),"pending":doc.pending.as_ref().map(dto::admission)})
}
fn read_document(v: &Value) -> Result<ProfileContentDocument, StorageError> {
    codec::object_fields(
        v,
        &[
            "slot", "revision", "sequence", "view", "page", "current", "previous", "pending",
        ],
    )?;
    let s = dto::read_slot(&v["slot"])?;
    let d = ProfileContentDocument {
        slot: s,
        revision: Revision::new(codec::number(v, "revision")?),
        latest_publish_sequence: codec::number(v, "sequence")?,
        view_revision: Revision::new(codec::number(v, "view")?),
        current_page: u8::try_from(codec::number(v, "page")?)
            .map_err(|_| StorageError::Database)?,
        current: if v["current"].is_null() {
            None
        } else {
            Some(dto::read_version(&v["current"], s)?)
        },
        previous: if v["previous"].is_null() {
            None
        } else {
            Some(dto::read_version(&v["previous"], s)?)
        },
        pending: if v["pending"].is_null() {
            None
        } else {
            Some(dto::read_admission(&v["pending"])?)
        },
    };
    d.validate_structure().map_err(|_| StorageError::Database)?;
    Ok(d)
}
fn status(s: Status) -> &'static str {
    match s {
        Status::Accepted => "accepted",
        Status::Committed => "committed",
        Status::Superseded => "superseded",
        Status::Failed => "failed",
        Status::Cancelled => "cancelled",
        Status::Expired => "expired",
        Status::RevisionConflict => "conflict",
    }
}
fn read_status(v: &Value) -> Result<Status, StorageError> {
    match v.as_str() {
        Some("accepted") => Ok(Status::Accepted),
        Some("committed") => Ok(Status::Committed),
        Some("superseded") => Ok(Status::Superseded),
        Some("failed") => Ok(Status::Failed),
        Some("cancelled") => Ok(Status::Cancelled),
        Some("expired") => Ok(Status::Expired),
        Some("conflict") => Ok(Status::RevisionConflict),
        _ => Err(StorageError::Database),
    }
}
#[derive(Clone)]
struct Receipt {
    admission: ContentAdmission,
    status: Status,
    snapshot: Option<PublishedContentSnapshot>,
    digest: Option<[u8; 32]>,
}
impl Receipt {
    fn outcome(&self) -> ProfileContentOutcome {
        ProfileContentOutcome {
            command_id: self.admission.request.command_id,
            slot: self.admission.request.slot,
            sequence: self.admission.sequence,
            status: self.status,
            committed_revision: self.snapshot.as_ref().map(|s| s.revision),
        }
    }
    fn value(&self) -> Value {
        json!({"format":1,"admission":dto::admission(&self.admission),"status":status(self.status),"snapshot":self.snapshot.as_ref().map(dto::snapshot),"digest":self.digest})
    }
    fn mutation(&self) -> Result<Mutation, StorageError> {
        put(receipt_key(self.admission.request.command_id), self.value())
    }
}
fn receipt(d: &dyn TransactionDocuments, c: CommandId) -> Result<Option<Receipt>, StorageError> {
    let Some(v) = read(d, &receipt_key(c))? else {
        return Ok(None);
    };
    codec::object_fields(&v, &["format", "admission", "status", "snapshot", "digest"])?;
    let r = Receipt {
        admission: dto::read_admission(&v["admission"])?,
        status: read_status(&v["status"])?,
        snapshot: if v["snapshot"].is_null() {
            None
        } else {
            Some(dto::read_snapshot(&v["snapshot"])?)
        },
        digest: if v["digest"].is_null() {
            None
        } else {
            Some(dto::hash(&v["digest"])?)
        },
    };
    let global = d.revision()?;
    if codec::number(&v, "format")? != 1
        || r.admission.sequence > global.get()
        || r.snapshot
            .as_ref()
            .is_some_and(|s| s.revision > global || s.view_revision > global)
        || r.admission.request.command_id != c
        || (r.status == Status::Committed) != (r.snapshot.is_some() && r.digest.is_some())
        || r.snapshot.as_ref().is_some_and(|s| {
            s.slot != r.admission.request.slot
                || s.revision
                    != r.admission
                        .request
                        .expected_revision
                        .next()
                        .unwrap_or_default()
        })
    {
        return Err(StorageError::Database);
    }
    Ok(Some(r))
}
fn outcome_value(o: &ProfileContentOutcome) -> Value {
    json!({"command":o.command_id.get(),"slot":dto::slot(o.slot),"sequence":o.sequence,"status":status(o.status),"revision":o.committed_revision.map(|r|r.get())})
}
fn read_outcome(v: &Value) -> Result<ProfileContentOutcome, StorageError> {
    codec::object_fields(v, &["command", "slot", "sequence", "status", "revision"])?;
    Ok(ProfileContentOutcome {
        command_id: profiles_codec::id(v, "command", CommandId::new)?,
        slot: dto::read_slot(&v["slot"])?,
        sequence: codec::number(v, "sequence")?,
        status: read_status(&v["status"])?,
        committed_revision: if v["revision"].is_null() {
            None
        } else {
            Some(Revision::new(codec::number(v, "revision")?))
        },
    })
}
#[derive(Clone)]
enum Op {
    Read(ContentSlotKey),
    ReadVersion(ContentSlotKey, Revision),
    Begin(BeginContentRequest),
    Finish(ContentAdmission, ContentPayload, ContentDigests),
    Fail(ContentAdmission),
    Outcome(CommandId),
    Page(PageContentRequest),
    Hydrate(ContentSlotKey, Revision, Revision),
}
struct Task<S> {
    op: Op,
    actor: Option<ActorContext>,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    scene: ProfileContentSceneGuard,
    factory: Factory<S>,
}
impl<S: Send + 'static> BusinessRepository<S> {
    fn content_task(&self, op: Op, actor: Option<&ActorContext>) -> Result<Vec<u8>, Error> {
        self.handle
            .try_transaction(Task {
                op,
                actor: actor.cloned(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                scene: self.profile_content_scene.clone(),
                factory: self.factory.clone(),
            })
            .and_then(|t| t.wait())
            .map_err(err)
    }
    pub fn with_profile_content_scene_guard(mut self, g: ProfileContentSceneGuard) -> Self {
        self.profile_content_scene = g;
        self
    }
    pub fn read_current_profile_content_version(
        &self,
        slot: ContentSlotKey,
        revision: Revision,
        expected_global_revision: Revision,
    ) -> Result<Option<ContentVersion>, Error> {
        let b = self.content_task(Op::Hydrate(slot, revision, expected_global_revision), None)?;
        let v = codec::value(&b).map_err(err)?;
        if v.is_null() {
            Ok(None)
        } else {
            dto::read_version(&v, slot).map(Some).map_err(err)
        }
    }
}
impl<S: Send + 'static> ProfileContentRepository for BusinessRepository<S> {
    fn profile_content(
        &self,
        s: ContentSlotKey,
        a: &ActorContext,
    ) -> Result<ProfileContentDocument, Error> {
        let b = self.content_task(Op::Read(s), Some(a))?;
        read_document(&codec::value_with_limit(&b, 3 * MAX_VALUE_BYTES).map_err(err)?).map_err(err)
    }
    fn read_profile_content_version(
        &self,
        s: ContentSlotKey,
        r: Revision,
        a: &ActorContext,
    ) -> Result<Option<ContentVersion>, Error> {
        let b = self.content_task(Op::ReadVersion(s, r), Some(a))?;
        let v = codec::value(&b).map_err(err)?;
        if v.is_null() {
            Ok(None)
        } else {
            dto::read_version(&v, s).map(Some).map_err(err)
        }
    }
    fn begin_profile_content(
        &mut self,
        r: BeginContentRequest,
        a: &ActorContext,
        _: &dyn Clock,
    ) -> Result<ContentAdmission, Error> {
        let b = self.content_task(Op::Begin(r), Some(a))?;
        dto::read_admission(&codec::value(&b).map_err(err)?).map_err(err)
    }
    fn commit_profile_content(
        &mut self,
        p: fc_ports::PreparedProfileContent,
        a: &ActorContext,
        _: &dyn Clock,
    ) -> Result<PublishedContentSnapshot, Error> {
        let (ad, p, dig) = p.into_parts();
        let b = self.content_task(Op::Finish(ad, p, dig), Some(a))?;
        dto::read_snapshot(&codec::value(&b).map_err(err)?).map_err(err)
    }
    fn fail_profile_content(
        &mut self,
        ad: ContentAdmission,
        a: &ActorContext,
        _: &dyn Clock,
    ) -> Result<ProfileContentOutcome, Error> {
        let b = self.content_task(Op::Fail(ad), Some(a))?;
        read_outcome(&codec::value(&b).map_err(err)?).map_err(err)
    }
    fn profile_content_outcome(
        &self,
        c: CommandId,
        a: &ActorContext,
        _: &dyn Clock,
    ) -> Result<Option<ProfileContentOutcome>, Error> {
        let b = self.content_task(Op::Outcome(c), Some(a))?;
        let v = codec::value(&b).map_err(err)?;
        if v.is_null() {
            Ok(None)
        } else {
            read_outcome(&v).map(Some).map_err(err)
        }
    }
    fn page_profile_content(
        &mut self,
        r: PageContentRequest,
        _: &dyn Clock,
    ) -> Result<PublishedContentSnapshot, Error> {
        let b = self.content_task(Op::Page(r), None)?;
        dto::read_snapshot(&codec::value(&b).map_err(err)?).map_err(err)
    }
}
fn selected(
    d: &dyn TransactionDocuments,
    i: &IdentityState,
    s: ContentSlotKey,
    now: LocalDateTime,
) -> Result<(), StorageError> {
    if desktop::read_watermark(d, i)?.is_some_and(|f| now.date < f) {
        return Err(StorageError::AuthorityUnavailable);
    }
    let schedule = schedule::load(d, i)?;
    let (ctx, _) = lesson::observe_with_floor(d, i, &schedule, now, None)?;
    let selection = profiles::select(d, i, &ctx, now)?;
    let tree = selection
        .subject_profile
        .as_ref()
        .map_or(&selection.public_layout.tree, |p| &p.tree);
    if tree.scope != s.scope
        || !tree.nodes.iter().any(|n| {
            n.id == s.node_id
                && matches!(
                    n.kind,
                    NodeKind::Widget(WidgetKind::SubjectText | WidgetKind::SubjectImage)
                )
        })
    {
        return Err(pe(ProfileContentError::InvalidSlot));
    }
    Ok(())
}
impl<S: Send> TransactionTask<S> for Task<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>().saturating_add(match &self.op {
            Op::Finish(_, p, _) => p.allocation_bytes().unwrap_or(usize::MAX),
            _ => 0,
        })
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let i = load_identity(d)?;
        if let Some(a) = &self.actor {
            authorize(&i, a, self.clock.as_ref(), &self.session)?;
        }
        let mut mutations = Vec::new();
        let mut recheck: Option<(
            ProfileContentDocument,
            ContentAdmission,
            ContentPayload,
            ContentDigests,
            LayoutDocument,
            Option<SubjectProfileDocument>,
        )> = None;
        let mut pagecheck = None;
        let mut page_rule: Option<PageRule> = None;
        let value = match &self.op {
            Op::Read(s) | Op::ReadVersion(s, _) => {
                let (public, subject) = target(d, &i, *s)?;
                let tree = subject.as_ref().map_or(&public.tree, |p| &p.tree);
                if !tree.nodes.iter().any(|n| {
                    n.id == s.node_id
                        && matches!(
                            n.kind,
                            NodeKind::Widget(WidgetKind::SubjectText | WidgetKind::SubjectImage)
                        )
                }) {
                    return Err(pe(ProfileContentError::InvalidSlot));
                }
                let (doc, retired, _) = load(d, *s)?;
                if retired {
                    return Err(pe(ProfileContentError::InvalidSlot));
                }
                match self.op {
                    Op::Read(_) => document_value(&doc),
                    Op::ReadVersion(_, r) => doc
                        .current
                        .as_ref()
                        .or(doc.previous.as_ref())
                        .filter(|v| v.revision == r)
                        .or_else(|| doc.previous.as_ref().filter(|v| v.revision == r))
                        .map_or(Value::Null, dto::version),
                    _ => unreachable!(),
                }
            }
            Op::Outcome(c) => match receipt(d, *c)? {
                Some(mut r)
                    if self
                        .actor
                        .as_ref()
                        .is_some_and(|a| a.principal_id == r.admission.actor_id) =>
                {
                    if r.status == Status::Accepted
                        && self.clock.now() >= r.admission.request.expires_at
                    {
                        let (doc, retired, _) = load(d, r.admission.request.slot)?;
                        if !retired && doc.pending.as_ref() == Some(&r.admission) {
                            mutations.push(head(&doc.fail(&r.admission).map_err(pe)?, false)?);
                        }
                        r.status = Status::Expired;
                        mutations.push(r.mutation()?);
                    }
                    outcome_value(&r.outcome())
                }
                _ => Value::Null,
            },
            Op::Begin(r) => {
                let a = self
                    .actor
                    .as_ref()
                    .ok_or(StorageError::AuthorityUnavailable)?;
                let (public, subject) = target(d, &i, r.slot)?;
                let t = ContentTarget {
                    public: &public,
                    subject: subject.as_ref(),
                };
                t.kind(r.slot, &i).map_err(pe)?;
                if let Some(old) = receipt(d, r.command_id)? {
                    if old.admission.request != *r
                        || old.admission.actor_id != a.principal_id
                        || old.admission.auth_epoch != a.auth_epoch
                    {
                        return Err(StorageError::CommandIdReused);
                    }
                    dto::admission(&old.admission)
                } else {
                    let (old, retired, exists) = load(d, r.slot)?;
                    if retired {
                        return Err(pe(ProfileContentError::InvalidSlot));
                    }
                    let (next, ad) = old
                        .begin(r.clone(), &i, a, self.clock.now(), &t)
                        .map_err(pe)?;
                    let (h, c) = counter(d)?;
                    if c >= MAX_RECEIPTS || !exists && h >= MAX_HEADS {
                        return Err(StorageError::DocumentTooLarge);
                    }
                    if let Some(p) = &old.pending {
                        let mut receipt =
                            receipt(d, p.request.command_id)?.ok_or(StorageError::Database)?;
                        receipt.status = Status::Superseded;
                        mutations.push(receipt.mutation()?)
                    }
                    mutations.push(head(&next, false)?);
                    if !exists {
                        mutations.push(put(index_key(r.slot), dto::slot(r.slot))?)
                    }
                    mutations.push(counter_put(h + u64::from(!exists), c + 1)?);
                    mutations.push(
                        Receipt {
                            admission: ad.clone(),
                            status: Status::Accepted,
                            snapshot: None,
                            digest: None,
                        }
                        .mutation()?,
                    );
                    dto::admission(&ad)
                }
            }
            Op::Finish(ad, p, dig) => {
                if dto::digests(ad.request.slot, p)? != *dig {
                    return Err(StorageError::Database);
                }
                let a = self
                    .actor
                    .as_ref()
                    .ok_or(StorageError::AuthorityUnavailable)?;
                let mut rc = receipt(d, ad.request.command_id)?
                    .ok_or(pe(ProfileContentError::AdmissionMismatch))?;
                if rc.admission != *ad {
                    return Err(StorageError::CommandIdReused);
                }
                if self
                    .actor
                    .as_ref()
                    .is_none_or(|a| a.principal_id != ad.actor_id || a.auth_epoch != ad.auth_epoch)
                {
                    return Err(pe(ProfileContentError::AdmissionMismatch));
                }
                if rc.status == Status::Committed {
                    if rc.digest != Some(dig.payload_sha256) {
                        return Err(StorageError::CommandIdReused);
                    }
                    dto::snapshot(rc.snapshot.as_ref().ok_or(StorageError::Database)?)
                } else {
                    let (old, retired, _) = load(d, ad.request.slot)?;
                    if retired {
                        return Err(pe(ProfileContentError::Superseded));
                    }
                    let (public, subject) = target(d, &i, ad.request.slot)?;
                    let next = old
                        .finish(
                            ad,
                            (p.clone(), *dig),
                            &i,
                            a,
                            self.clock.now(),
                            &ContentTarget {
                                public: &public,
                                subject: subject.as_ref(),
                            },
                        )
                        .map_err(pe)?;
                    mutations.extend(media::update_refs(d, &old, &next, Some(ad))?);
                    let v = next.current.as_ref().ok_or(StorageError::Database)?;
                    mutations.push(put(
                        version_key(next.slot, v.revision),
                        json!({"format":1,"slot":dto::slot(next.slot),"version":dto::version(v)}),
                    )?);
                    if let Some(prev) = &old.previous {
                        mutations.push(Mutation::Delete {
                            key: version_key(old.slot, prev.revision),
                        })
                    }
                    mutations.push(head(&next, false)?);
                    let snap = next.snapshot().map_err(pe)?;
                    rc.status = Status::Committed;
                    rc.snapshot = Some(snap.clone());
                    rc.digest = Some(dig.payload_sha256);
                    mutations.push(rc.mutation()?);
                    recheck = Some((old, ad.clone(), p.clone(), *dig, public, subject));
                    dto::snapshot(&snap)
                }
            }
            Op::Fail(ad) => {
                let mut rc = receipt(d, ad.request.command_id)?
                    .ok_or(pe(ProfileContentError::AdmissionMismatch))?;
                if rc.admission != *ad {
                    return Err(StorageError::CommandIdReused);
                }
                if self
                    .actor
                    .as_ref()
                    .is_none_or(|a| a.principal_id != ad.actor_id || a.auth_epoch != ad.auth_epoch)
                {
                    return Err(pe(ProfileContentError::AdmissionMismatch));
                }
                if rc.status == Status::Accepted {
                    let (old, retired, _) = load(d, ad.request.slot)?;
                    if !retired && old.pending.as_ref() == Some(ad) {
                        let next = old.fail(ad).map_err(pe)?;
                        mutations.push(head(&next, false)?);
                        rc.status = Status::Failed;
                    } else {
                        rc.status = Status::Superseded
                    }
                    mutations.push(rc.mutation()?);
                }
                outcome_value(&rc.outcome())
            }
            Op::Hydrate(s, r, g) => {
                if d.revision()? != *g {
                    return Err(StorageError::Conflict {
                        current: d.revision()?,
                    });
                }
                selected(d, &i, *s, self.clock.now())?;
                let (doc, retired, _) = load(d, *s)?;
                if retired || doc.revision != *r {
                    return Err(pe(ProfileContentError::InvalidState));
                }
                doc.current.as_ref().map_or(Value::Null, dto::version)
            }
            Op::Page(r) => {
                selected(d, &i, r.slot, self.clock.now())?;
                let (old, retired, _) = load(d, r.slot)?;
                if retired {
                    return Err(pe(ProfileContentError::InvalidSlot));
                }
                (self.scene)(r.slot, old.revision, old.view_revision, d.revision()?)?;
                let schedule = schedule::load(d, &i)?;
                let ledger = lesson::load(d)?;
                let captured = self.clock.now();
                let expected = ledger
                    .observe_with_floor(&i, &schedule, captured, None)
                    .map_err(StorageError::Lesson)?
                    .current;
                let identity = i.clone();
                page_rule = Some(Box::new(move |now| {
                    if now < captured
                        || now.date != captured.date
                        || ledger
                            .observe_with_floor(&identity, &schedule, now, None)
                            .map_err(StorageError::Lesson)?
                            .current
                            != expected
                    {
                        return Err(StorageError::AuthorityUnavailable);
                    }
                    Ok(())
                }));
                let pk = format!("{ROOT}page-receipts/{:020}", r.command_id.get());
                let fingerprint:Vec<u8>=Sha256::digest(codec::bytes(&json!({"slot":dto::slot(r.slot),"content":r.expected_content_revision.get(),"view":r.expected_view_revision.get(),"page":r.page}))?).to_vec();
                if let Some(v) = read(d, &pk)? {
                    codec::object_fields(&v, &["fingerprint", "snapshot"])?;
                    if dto::hash(&v["fingerprint"])? != fingerprint.as_slice() {
                        return Err(StorageError::CommandIdReused);
                    }
                    let snap = dto::read_snapshot(&v["snapshot"])?;
                    if snap.slot != r.slot
                        || snap.revision != r.expected_content_revision
                        || (snap.view_revision != r.expected_view_revision
                            && snap.view_revision
                                != r.expected_view_revision
                                    .next()
                                    .map_err(|_| StorageError::Database)?)
                        || snap.current_page != r.page
                    {
                        return Err(StorageError::Database);
                    }
                    v["snapshot"].clone()
                } else {
                    let (h, c) = counter(d)?;
                    if c >= MAX_RECEIPTS {
                        return Err(StorageError::DocumentTooLarge);
                    }
                    mutations.push(counter_put(h, c + 1)?);
                    let next = old.page(r).map_err(pe)?;
                    let snap = next.snapshot().map_err(pe)?;
                    mutations.push(head(&next, false)?);
                    mutations.push(put(
                        pk,
                        json!({"fingerprint":fingerprint,"snapshot":dto::snapshot(&snap)}),
                    )?);
                    pagecheck = Some((r.slot, old.revision, old.view_revision, d.revision()?));
                    dto::snapshot(&snap)
                }
            }
        };
        if let Some(a) = &self.actor {
            authorize(&i, a, self.clock.as_ref(), &self.session)?;
        }
        if let Op::Hydrate(s, _, _) = self.op {
            selected(d, &i, s, self.clock.now())?;
        }
        let outcome = display::encode(&value, 3 * MAX_VALUE_BYTES)?;
        let commit = if mutations.is_empty() {
            None
        } else {
            Some((
                mutations,
                (self.factory)(
                    &i,
                    d.revision()?
                        .next()
                        .map_err(|_| StorageError::InvalidCommand)?,
                )?,
            ))
        };
        let actor = self.actor;
        let clock = self.clock;
        let session = self.session;
        let scene = self.scene;
        let begin = match self.op {
            Op::Begin(r) => Some(r),
            _ => None,
        };
        Ok(TransactionDecision {
            outcome,
            commit,
            precommit: Some(Box::new(move || {
                if let Some(a) = &actor {
                    authorize(&i, a, clock.as_ref(), &session)?;
                }
                if let Some(r) = begin {
                    if clock.now() >= r.expires_at {
                        return Err(pe(ProfileContentError::Expired));
                    }
                }
                if let Some((old, ad, p, dig, public, subject)) = recheck {
                    old.finish(
                        &ad,
                        (p, dig),
                        &i,
                        actor.as_ref().ok_or(StorageError::AuthorityUnavailable)?,
                        clock.now(),
                        &ContentTarget {
                            public: &public,
                            subject: subject.as_ref(),
                        },
                    )
                    .map_err(pe)?;
                }
                if let Some(rule) = page_rule {
                    rule(clock.now())?;
                }
                if let Some((s, c, v, g)) = pagecheck {
                    scene(s, c, v, g)?;
                }
                Ok(())
            })),
        })
    }
}

pub(super) fn primary_bytes(d: &dyn TransactionDocuments) -> Result<u64, StorageError> {
    media::primary_bytes(d)
}
/// Active lightweight facts; body bytes remain exclusively in immutable versions.
pub(super) fn snapshots(
    d: &dyn TransactionDocuments,
    p: &ProfileSelectionSnapshot,
) -> Result<Vec<PublishedContentSnapshot>, StorageError> {
    let t = p
        .subject_profile
        .as_ref()
        .map_or(&p.public_layout.tree, |s| &s.tree);
    let mut out = Vec::new();
    for n in &t.nodes {
        if matches!(
            n.kind,
            NodeKind::Widget(WidgetKind::SubjectText | WidgetKind::SubjectImage)
        ) {
            let (doc, retired, _) = load(
                d,
                ContentSlotKey {
                    scope: t.scope,
                    node_id: n.id,
                },
            )?;
            if retired {
                return Err(StorageError::Database);
            }
            out.push(doc.snapshot().map_err(|_| StorageError::Database)?);
        }
    }
    Ok(out)
}
/// Node IDs are never reused. Retired heads remain declared tombstones, while
/// previous/current immutable documents and registry references drop atomically.
pub(super) fn remove_nodes(
    d: &dyn TransactionDocuments,
    old: &LayoutTree,
    next: &LayoutTree,
) -> Result<Vec<Mutation>, StorageError> {
    let mut writes = Vec::new();
    for n in &old.nodes {
        if matches!(
            n.kind,
            NodeKind::Widget(WidgetKind::SubjectText | WidgetKind::SubjectImage)
        ) && !next.nodes.iter().any(|v| v.id == n.id && v.kind == n.kind)
        {
            let (mut doc, retired, exists) = load(
                d,
                ContentSlotKey {
                    scope: old.scope,
                    node_id: n.id,
                },
            )?;
            if !exists || retired {
                continue;
            }
            let old = doc.clone();
            doc.current = None;
            doc.previous = None;
            doc.pending = None;
            doc.current_page = 0;
            writes.extend(media::update_refs(d, &old, &doc, None)?);
            for v in [&old.current, &old.previous].into_iter().flatten() {
                writes.push(Mutation::Delete {
                    key: version_key(old.slot, v.revision),
                });
            }
            if let Some(a) = old.pending {
                let mut r = receipt(d, a.request.command_id)?.ok_or(StorageError::Database)?;
                r.status = Status::Cancelled;
                writes.push(r.mutation()?);
            }
            writes.push(head(&doc, true)?);
        }
    }
    Ok(writes)
}
pub(super) fn startup_check(
    d: &dyn TransactionDocuments,
    i: &IdentityState,
) -> Result<(), StorageError> {
    use std::collections::{BTreeMap, HashSet};
    let (h, r) = counter(d)?;
    let mut heads = 0;
    let mut receipts = 0;
    let mut expected_versions = HashSet::new();
    let mut expected_media: BTreeMap<MediaVersionId, (u64, MediaReference)> = BTreeMap::new();
    let mut after = None;
    loop {
        let p = d.scan_prefix(&format!("{ROOT}heads/"), after.as_deref(), 128)?;
        for (k, v) in p.entries {
            heads += 1;
            if heads > MAX_HEADS {
                return Err(StorageError::Database);
            }
            let raw = codec::value(&v.value)?;
            let s = dto::read_slot(&raw["slot"])?;
            if k != head_key(s) {
                return Err(StorageError::Database);
            }
            let (doc, retired, _) = load(d, s)?;
            if i.installation.is_none() {
                return Err(StorageError::Database);
            }
            if !retired {
                let (public, subject) = target(d, i, s)?;
                let tree = subject.as_ref().map_or(&public.tree, |p| &p.tree);
                if !tree.nodes.iter().any(|n| {
                    n.id == s.node_id
                        && matches!(
                            n.kind,
                            NodeKind::Widget(WidgetKind::SubjectText | WidgetKind::SubjectImage)
                        )
                }) {
                    return Err(StorageError::Database);
                }
                let kind = match tree
                    .nodes
                    .iter()
                    .find(|n| n.id == s.node_id)
                    .map(|n| &n.kind)
                {
                    Some(NodeKind::Widget(WidgetKind::SubjectText)) => ContentPayloadKind::Text,
                    Some(NodeKind::Widget(WidgetKind::SubjectImage)) => ContentPayloadKind::Images,
                    _ => return Err(StorageError::Database),
                };
                for v in [&doc.current, &doc.previous].into_iter().flatten() {
                    if v.payload.kind().is_some_and(|actual| actual != kind)
                        || i.account(v.last_actor_id).is_err()
                    {
                        return Err(StorageError::Database);
                    }
                    expected_versions.insert(version_key(s, v.revision));
                    if let ContentPayload::Images(a) = &v.payload {
                        for r in a {
                            let entry =
                                expected_media.entry(r.version_id).or_insert((0, r.clone()));
                            if entry.1 != *r {
                                return Err(StorageError::Database);
                            }
                            entry.0 += 1;
                        }
                    }
                }
                if let Some(a) = &doc.pending {
                    let r = receipt(d, a.request.command_id)?.ok_or(StorageError::Database)?;
                    if r.admission != *a || r.status != Status::Accepted {
                        return Err(StorageError::Database);
                    }
                }
            }
        }
        after = p.next_key;
        if after.is_none() {
            break;
        }
    }
    let mut slots = 0;
    let mut versions = 0;
    let mut after = None;
    loop {
        let p = d.scan_prefix(ROOT, after.as_deref(), 128)?;
        for (k, v) in p.entries {
            let raw = codec::value(&v.value)?;
            if k == COUNTER || k.starts_with(&format!("{ROOT}heads/")) {
                continue;
            } else if k.starts_with(&format!("{ROOT}slots/")) {
                slots += 1;
                if slots > h {
                    return Err(StorageError::Database);
                }
                let s = dto::read_slot(&raw)?;
                if k != index_key(s) || !load(d, s)?.2 {
                    return Err(StorageError::Database);
                }
            } else if k.starts_with(&format!("{ROOT}versions/")) {
                versions += 1;
                if versions > expected_versions.len() {
                    return Err(StorageError::Database);
                }
                if !expected_versions.contains(&k) {
                    return Err(StorageError::Database);
                }
            } else if k.starts_with(&format!("{ROOT}receipts/")) {
                receipts += 1;
                if receipts > r {
                    return Err(StorageError::Database);
                }
                let a = dto::read_admission(&raw["admission"])?;
                if k != receipt_key(a.request.command_id) {
                    return Err(StorageError::Database);
                }
                receipt(d, a.request.command_id)?.ok_or(StorageError::Database)?;
            } else if k.starts_with(&format!("{ROOT}page-receipts/")) {
                receipts += 1;
                if receipts > r {
                    return Err(StorageError::Database);
                }
                let id = k
                    .strip_prefix(&format!("{ROOT}page-receipts/"))
                    .and_then(|s| s.parse::<u64>().ok())
                    .and_then(|n| CommandId::new(n).ok())
                    .ok_or(StorageError::Database)?;
                if k != format!("{ROOT}page-receipts/{:020}", id.get()) {
                    return Err(StorageError::Database);
                }
                codec::object_fields(&raw, &["fingerprint", "snapshot"])?;
                dto::hash(&raw["fingerprint"])?;
                dto::read_snapshot(&raw["snapshot"])?;
            } else {
                return Err(StorageError::Database);
            }
        }
        after = p.next_key;
        if after.is_none() {
            break;
        }
    }
    if heads != h || slots != h || receipts != r || versions != expected_versions.len() {
        return Err(StorageError::Database);
    }
    media::startup_check(d, &expected_media)
}
