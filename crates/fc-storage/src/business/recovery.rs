//! Ordinary recovery is a projection, never a database-page copy. This module
//! owns the business allowlist and its replay-inert transformations.
use super::*;
use crate::recovery::{ExecutionFence, RecoveryBarrier, RecoveryDatabaseReceipt};
use fc_domain::{recovery::BackupEntry, time::*};

pub(crate) const SECURITY: &str = "business/security/recovery/v1";
pub(crate) const INSTALLATION: &str = "business/security/installation/v1";
const AUDIT: &str = "business/recovery/lesson-audit/v1";
pub(crate) fn validate_installation_id(id: [u8; 32]) -> Result<(), StorageError> {
    if id == [0; 32] {
        Err(StorageError::InvalidCommand)
    } else {
        Ok(())
    }
}
pub(crate) fn installation_bytes(id: [u8; 32]) -> Result<Vec<u8>, StorageError> {
    validate_installation_id(id)?;
    codec::bytes(&json!({"format":1,"id":id.iter().map(|b|format!("{b:02x}")).collect::<String>()}))
}
pub(crate) fn installation(d: &dyn TransactionDocuments) -> Result<Option<[u8; 32]>, StorageError> {
    let Some(row) = d.get(INSTALLATION)? else {
        return Ok(None);
    };
    let v = codec::value(&row.value)?;
    codec::object_fields(&v, &["format", "id"])?;
    let text = codec::text(&v, "id")?;
    if codec::number(&v, "format")? != 1 || validate_retired_hash(&text).is_err() {
        return Err(StorageError::Database);
    }
    let mut id = [0u8; 32];
    for (n, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[n * 2..n * 2 + 2], 16).map_err(|_| StorageError::Database)?;
    }
    if id == [0; 32] {
        return Err(StorageError::Database);
    }
    Ok(Some(id))
}

pub(crate) fn generation(d: &dyn TransactionDocuments) -> Result<u64, StorageError> {
    let Some(row) = d.get(SECURITY)? else {
        return Ok(0);
    };
    let v = codec::value(&row.value)?;
    codec::object_fields(
        &v,
        &["format", "generation", "revision", "principal", "epochs"],
    )?;
    if codec::number(&v, "format")? != 1
        || codec::number(&v, "generation")? == 0
        || codec::number(&v, "revision")? > d.revision()?.get()
        || codec::number(&v, "principal")? == 0
    {
        return Err(StorageError::Database);
    }
    let i = load_identity(d)?;
    let mut seen = std::collections::BTreeSet::new();
    for row in codec::array(&v, "epochs")? {
        codec::object_fields(row, &["principal", "epoch"])?;
        let id = PrincipalId::new(codec::number(row, "principal")?)
            .map_err(|_| StorageError::Database)?;
        let epoch = codec::number(row, "epoch")?;
        if epoch == 0
            || !seen.insert(id)
            || i.account(id)
                .map_err(StorageError::Identity)?
                .principal
                .auth_epoch
                .get()
                < epoch
        {
            return Err(StorageError::Database);
        }
    }
    if seen.is_empty() {
        return Err(StorageError::Database);
    }
    codec::number(&v, "generation")
}
pub(crate) fn startup_check(d: &dyn TransactionDocuments) -> Result<(), StorageError> {
    if installation(d)?.is_some() && load_identity(d)?.installation.is_none() {
        return Err(StorageError::Database);
    }
    let recovery_generation = generation(d)?;
    let mut after = None;
    loop {
        let page = d.scan_prefix(crate::recovery::RETIRED, after.as_deref(), 128)?;
        for (key, row) in page.entries {
            validate_retired_hash(
                key.strip_prefix(crate::recovery::RETIRED)
                    .ok_or(StorageError::Database)?,
            )?;
            let v = codec::value(&row.value)?;
            codec::object_fields(&v, &["format", "generation"])?;
            let g = codec::number(&v, "generation")?;
            if codec::number(&v, "format")? != 1 || g == 0 || g > recovery_generation {
                return Err(StorageError::Database);
            }
        }
        after = page.next_key;
        if after.is_none() {
            break;
        }
    }
    if let Some(row) = d.get(AUDIT)? {
        let s = lesson_codec::read_snapshot(&codec::value(&row.value)?)?;
        let i = load_identity(d)?;
        if s.current.is_some()
            || s.active_override.is_some()
            || s.base_trust.is_some()
            || s.revision > d.revision()?
            || s.identity_revision > i.revision
        {
            return Err(StorageError::Database);
        }
        validate_lesson_refs(&s, &i)?;
    }
    Ok(())
}
pub(crate) fn security_value(
    generation: u64,
    revision: Revision,
    principal: PrincipalId,
    state: &IdentityState,
) -> Value {
    json!({"format":1,"generation":generation,"revision":revision.get(),"principal":principal.get(),
        "epochs":state.accounts.iter().map(|a|json!({"principal":a.principal.id.get(),"epoch":a.principal.auth_epoch.get()})).collect::<Vec<_>>()})
}
pub(crate) fn advance_identity(state: &IdentityState) -> Result<IdentityState, StorageError> {
    let mut next = state.clone();
    next.revision = next
        .revision
        .next()
        .map_err(|_| StorageError::RecoveryConflict)?;
    for a in &mut next.accounts {
        a.principal.auth_epoch = AuthEpoch::new(
            a.principal
                .auth_epoch
                .get()
                .checked_add(1)
                .ok_or(StorageError::RecoveryConflict)?,
        );
    }
    next.validate().map_err(StorageError::Identity)?;
    Ok(next)
}

impl<S: Send + 'static> BusinessRepository<S> {
    /// Startup-only migration while the deployment owns its exclusive lock and
    /// has admitted no network/media jobs. Existing valid identifiers are kept;
    /// corrupt identifiers are never overwritten with a newly generated value.
    pub fn ensure_recovery_installation(&self, id: [u8; 32]) -> Result<[u8; 32], StorageError> {
        validate_installation_id(id)?;
        let b = self
            .handle
            .try_transaction(InstallationTask {
                seed: Some(id),
                factory: self.factory.clone(),
            })?
            .wait()?;
        b.try_into().map_err(|_| StorageError::Database)
    }
    pub fn recovery_installation(&self) -> Result<[u8; 32], StorageError> {
        let b = self
            .handle
            .try_transaction(InstallationTask {
                seed: None,
                factory: self.factory.clone(),
            })?
            .wait()?;
        b.try_into().map_err(|_| StorageError::Database)
    }
    /// Trusted composition only: destination must be a new, protected staging
    /// path. Caller owns maintenance/quiescence and media pins through packaging.
    /// The writer supplies one consistent metadata revision; it cannot pin files.
    pub fn try_export_ordinary(
        &self,
        destination: PathBuf,
        actor: &ActorContext,
        fence: ExecutionFence,
    ) -> Result<Ticket<RecoveryDatabaseReceipt>, StorageError> {
        crate::recovery::validate_path(&destination)?;
        self.handle.send(|reply| {
            Message::Recovery(
                crate::recovery::Job::Export {
                    destination,
                    actor: actor.clone(),
                    session: self.session.clone(),
                    clock: self.clock.clone(),
                    fence,
                },
                reply,
            )
        })
    }
    /// Persist the security floor before any cold replacement. An error during
    /// later staging never undoes these revoked authorizations or login backoff.
    pub fn try_advance_recovery_barrier(
        &self,
        expected_revision: Revision,
        actor: &ActorContext,
        fence: ExecutionFence,
    ) -> Result<Ticket<RecoveryBarrier>, StorageError> {
        self.handle.send(|reply| {
            Message::RecoveryBarrier(
                Box::new(BarrierTask {
                    expected_revision,
                    actor: actor.clone(),
                    session: self.session.clone(),
                    clock: self.clock.clone(),
                    factory: self.factory.clone(),
                    fence,
                }),
                reply,
            )
        })
    }
    /// Both paths are trusted, pinned staging paths. Source is opened read-only;
    /// the capability is consumed and exact live revision/floor are rechecked.
    pub fn try_prepare_ordinary_restore(
        &self,
        source: PathBuf,
        destination: PathBuf,
        barrier: RecoveryBarrier,
        fence: ExecutionFence,
    ) -> Result<Ticket<RecoveryDatabaseReceipt>, StorageError> {
        crate::recovery::validate_path(&source)?;
        crate::recovery::validate_path(&destination)?;
        if source == destination {
            return Err(StorageError::InvalidCommand);
        }
        self.handle.send(|reply| {
            Message::Recovery(
                crate::recovery::Job::Candidate {
                    source,
                    destination,
                    authorization: crate::recovery::PreparationAuthorization::Barrier(barrier),
                    fence,
                    clock: self.clock.clone(),
                },
                reply,
            )
        })
    }
    /// Full isolated preview before confirmation. This performs the same
    /// schema, reference, current-security and quota validation as preparation,
    /// while leaving live epochs/generation unchanged and issuing no capability.
    pub fn try_preflight_ordinary_restore(
        &self,
        source: PathBuf,
        destination: PathBuf,
        actor: &ActorContext,
        fence: ExecutionFence,
    ) -> Result<Ticket<RecoveryDatabaseReceipt>, StorageError> {
        crate::recovery::validate_path(&source)?;
        crate::recovery::validate_path(&destination)?;
        if source == destination {
            return Err(StorageError::InvalidCommand);
        }
        self.handle.send(|reply| {
            Message::Recovery(
                crate::recovery::Job::Candidate {
                    source,
                    destination,
                    authorization: crate::recovery::PreparationAuthorization::Preview {
                        actor: actor.clone(),
                        session: self.session.clone(),
                    },
                    fence,
                    clock: self.clock.clone(),
                },
                reply,
            )
        })
    }
}
struct InstallationTask<S> {
    seed: Option<[u8; 32]>,
    factory: Factory<S>,
}
impl<S: Send> TransactionTask<S> for InstallationTask<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let i = load_identity(d)?;
        if i.installation.is_none() {
            return Err(StorageError::RecoveryConflict);
        }
        if let Some(id) = installation(d)? {
            return Ok(TransactionDecision {
                precommit: None,
                outcome: id.to_vec(),
                commit: None,
            });
        }
        let id = self.seed.ok_or(StorageError::RecoveryConflict)?;
        validate_installation_id(id)?;
        let r = d
            .revision()?
            .next()
            .map_err(|_| StorageError::RecoveryConflict)?;
        let snapshot = (self.factory)(&i, r)?;
        Ok(TransactionDecision {
            precommit: None,
            outcome: id.to_vec(),
            commit: Some((
                vec![Mutation::Put {
                    key: INSTALLATION.into(),
                    value: installation_bytes(id)?,
                }],
                snapshot,
            )),
        })
    }
}
struct BarrierTask<S> {
    expected_revision: Revision,
    actor: ActorContext,
    session: SessionGuard,
    clock: Arc<dyn Clock + Send + Sync>,
    factory: Factory<S>,
    fence: ExecutionFence,
}
impl<S: Send> TransactionTask<S> for BarrierTask<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let old = load_identity(d)?;
        let check = || {
            crate::recovery::check_fence(&self.fence)?;
            (self.session)(&old, &self.actor)?;
            old.authorize_management(&self.actor, self.clock.now())
                .map_err(StorageError::Identity)
        };
        check()?;
        if d.revision()? != self.expected_revision {
            return Err(StorageError::Conflict {
                current: d.revision()?,
            });
        }
        let next = advance_identity(&old)?;
        let revision = d
            .revision()?
            .next()
            .map_err(|_| StorageError::RecoveryConflict)?;
        let generation = generation(d)?
            .checked_add(1)
            .ok_or(StorageError::RecoveryConflict)?;
        let mutations = vec![
            Mutation::Put {
                key: IDENTITY.into(),
                value: codec::bytes(&codec::identity(&next))?,
            },
            Mutation::Put {
                key: SECURITY.into(),
                value: codec::bytes(&security_value(
                    generation,
                    revision,
                    self.actor.principal_id,
                    &next,
                ))?,
            },
        ];
        let prepared = (self.factory)(&next, revision)?;
        let session = self.session;
        let clock = self.clock;
        let actor = self.actor;
        let fence = self.fence;
        let outcome = codec::bytes(
            &json!({"revision":revision.get(),"generation":generation,"principal":actor.principal_id.get()}),
        )?;
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, prepared)),
            precommit: Some(Box::new(move || {
                crate::recovery::check_fence(&fence)?;
                session(&old, &actor)?;
                old.authorize_management(&actor, clock.now())
                    .map_err(StorageError::Identity)
            })),
        })
    }
}

/// This DTO deliberately has no security account objects, roles, credentials, assignments,
/// authorization epochs, recovery records or persisted automatic policy grants.
pub(crate) fn catalog(s: &IdentityState) -> Result<Value, StorageError> {
    let i = s
        .installation
        .as_ref()
        .ok_or(StorageError::RecoveryConflict)?;
    Ok(
        json!({"format":1,"school":i.school_id.get(),"name":i.school_name,"class":i.current_class.map(ClassId::get),
        "classes":s.classes.iter().map(|c|json!({"id":c.id.get(),"name":c.name})).collect::<Vec<_>>(),
        "subjects":s.subjects.iter().map(|v|json!({"id":v.id.get(),"name":v.name,"color":[v.color.red,v.color.green,v.color.blue]})).collect::<Vec<_>>(),
        "defaults":s.default_teachers.iter().map(|d|json!({"class":d.class_id.get(),"subject":d.subject_id.get(),"teacher":d.teacher_id.get()})).collect::<Vec<_>>(),
        "account_labels":s.accounts.iter().map(|a|json!({"id":a.principal.id.get(),"display_name":a.display_name})).collect::<Vec<_>>()}),
    )
}
pub(crate) fn merge_catalog(
    current: &IdentityState,
    v: &Value,
) -> Result<IdentityState, StorageError> {
    codec::object_fields(
        v,
        &[
            "format",
            "school",
            "name",
            "class",
            "classes",
            "subjects",
            "defaults",
            "account_labels",
        ],
    )?;
    let mut s = current.clone();
    let i = s
        .installation
        .as_mut()
        .ok_or(StorageError::RecoveryConflict)?;
    if codec::number(v, "format")? != 1 || codec::number(v, "school")? != i.school_id.get() {
        return Err(StorageError::RecoveryConflict);
    }
    i.school_name = codec::text(v, "name")?;
    let class = if v["class"].is_null() {
        None
    } else {
        Some(
            ClassId::new(codec::number(v, "class")?)
                .map_err(|_| StorageError::RecoveryInvalidPackage)?,
        )
    };
    if class.is_some_and(|id| !s.classes.iter().any(|c| c.id == id && c.enabled)) {
        return Err(StorageError::RecoveryConflict);
    }
    i.current_class = class;
    let mut seen = std::collections::BTreeSet::new();
    for c in codec::array(v, "classes")? {
        codec::object_fields(c, &["id", "name"])?;
        let id = codec::number(c, "id")?;
        if !seen.insert(id) {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        s.classes
            .iter_mut()
            .find(|c| c.id.get() == id)
            .ok_or(StorageError::RecoveryConflict)?
            .name = codec::text(c, "name")?;
    }
    seen.clear();
    for c in codec::array(v, "subjects")? {
        codec::object_fields(c, &["id", "name", "color"])?;
        let id = codec::number(c, "id")?;
        if !seen.insert(id) {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        let target = s
            .subjects
            .iter_mut()
            .find(|c| c.id.get() == id)
            .ok_or(StorageError::RecoveryConflict)?;
        target.name = codec::text(c, "name")?;
        let rgb = codec::array(c, "color")?;
        if rgb.len() != 3 {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        let channel = |n: usize| {
            u8::try_from(
                rgb[n]
                    .as_u64()
                    .ok_or(StorageError::RecoveryInvalidPackage)?,
            )
            .map_err(|_| StorageError::RecoveryInvalidPackage)
        };
        target.color = Rgb {
            red: channel(0)?,
            green: channel(1)?,
            blue: channel(2)?,
        };
        target.revision = target
            .revision
            .next()
            .map_err(|_| StorageError::RecoveryConflict)?;
    }
    s.default_teachers.clear();
    let mut defaults = std::collections::BTreeSet::new();
    for d in codec::array(v, "defaults")? {
        codec::object_fields(d, &["class", "subject", "teacher"])?;
        let class_id = ClassId::new(codec::number(d, "class")?)
            .map_err(|_| StorageError::RecoveryInvalidPackage)?;
        let subject_id = SubjectId::new(codec::number(d, "subject")?)
            .map_err(|_| StorageError::RecoveryInvalidPackage)?;
        let teacher_id = PrincipalId::new(codec::number(d, "teacher")?)
            .map_err(|_| StorageError::RecoveryInvalidPackage)?;
        // Defaults select only an already-current enabled teaching scope; they
        // cannot reinstate an assignment or account that was revoked later.
        if !s.is_teacher(teacher_id, class_id, subject_id)
            || !defaults.insert((class_id, subject_id))
        {
            return Err(StorageError::RecoveryConflict);
        }
        s.default_teachers.push(DefaultTeacher {
            class_id,
            subject_id,
            teacher_id,
        });
    }
    let mut labels = std::collections::BTreeSet::new();
    for label in codec::array(v, "account_labels")? {
        codec::object_fields(label, &["id", "display_name"])?;
        let id = PrincipalId::new(codec::number(label, "id")?)
            .map_err(|_| StorageError::RecoveryInvalidPackage)?;
        if !labels.insert(id) {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        s.accounts
            .iter_mut()
            .find(|a| a.principal.id == id)
            .ok_or(StorageError::RecoveryConflict)?
            .display_name = codec::text(label, "display_name")?;
    }
    s.validate().map_err(StorageError::Identity)?;
    Ok(s)
}

pub(crate) fn allowed(key: &str) -> bool {
    key.starts_with(crate::recovery::ORDINARY_RETIRED)
        || matches!(
            key,
            "business/display/v1"
                | "business/schedule/v1"
                | "business/schedule/last-changed/v1"
                | "business/desktop/max-observed-day/v1"
                | "business/lesson-context/v1"
                | "business/voice/v1"
                | AUDIT
                | "business/calendar/meta/v1"
        )
        || key.starts_with("business/homework/")
        || key.starts_with("business/reminder/")
        || key.starts_with("business/calendar/candidate/v1/")
        || [
            "business/profiles/v1/manifest",
            "business/profiles/v1/public",
        ]
        .contains(&key)
        || [
            "business/profiles/v1/documents/",
            "business/profiles/v1/pages/",
            "business/profiles/v1/slots/",
            "business/material/v1/folders/",
            "business/material/v1/scope/",
            "business/material/v1/items/",
            "business/material/v1/folder-items/",
            "business/material/v1/published/",
            "business/material/v1/resources/",
            "business/profile-content/v1/heads/",
            "business/profile-content/v1/slots/",
            "business/profile-content/v1/versions/",
            "business/profile-media/v1/entries/",
            "business/profile-media/v1/recovery-owners/",
        ]
        .iter()
        .any(|p| key.starts_with(p))
        || [
            "business/material/v1/catalog",
            "business/profile-content/v1/counter",
            "business/profile-media/v1/catalog",
        ]
        .contains(&key)
}
pub(crate) fn protected(key: &str) -> bool {
    key == IDENTITY
        || key.starts_with("business/credential/")
        || key.starts_with("business/security/")
        || key == "business/autostart/v1"
        || key.starts_with("business/voice-rate/")
}
pub(crate) fn known_omitted(key: &str) -> bool {
    protected(key)
        || [
            "business/receipt/",
            "business/result/",
            "business/voice-receipt/",
            "business/voice-rate/",
            "business/profiles/v1/receipts/",
            "business/profiles/v1/results/",
            "business/material/v1/receipts/",
            "business/material/v1/web-outcomes/",
            "business/profile-content/v1/receipts/",
            "business/profile-content/v1/page-receipts/",
            "business/profile-media/v1/reservations/",
        ]
        .iter()
        .any(|p| key.starts_with(p))
}

pub(crate) fn project(
    key: &str,
    bytes: &[u8],
    floor: LocalDate,
    revision: Option<Revision>,
    identity: &IdentityState,
) -> Result<Option<Vec<u8>>, StorageError> {
    if !allowed(key) {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    let mut v = codec::value(bytes)?;
    if let Some(id) = key.strip_prefix("business/profile-media/v1/recovery-owners/") {
        codec::object_fields(&v, &["format", "id"])?;
        let n = codec::number(&v, "id")?;
        if n == 0 || id != format!("{n:020}") || codec::number(&v, "format")? != 1 {
            return Err(StorageError::RecoveryInvalidPackage);
        }
    } else if let Some(hash) = key.strip_prefix(crate::recovery::ORDINARY_RETIRED) {
        validate_retired_hash(hash)?;
        codec::object_fields(&v, &["format"])?;
        if codec::number(&v, "format")? != 1 {
            return Err(StorageError::RecoveryInvalidPackage);
        }
    } else if key.starts_with("business/homework/") || key.starts_with("business/reminder/") {
        let record = codec::read_record(&v)?;
        if content_key(&record.key) != key {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        if identity.account(record.last_actor_id).is_err() {
            return Err(StorageError::RecoveryConflict);
        }
        match record.key {
            ContentKey::SharedReminder(k) if k.actual_date < floor => return Ok(None),
            ContentKey::Homework(k)
                if identity.account(k.owner_teacher_id).is_err()
                    || !identity.classes.iter().any(|c| c.id == k.class_id)
                    || !identity.subjects.iter().any(|s| s.id == k.subject_id) =>
            {
                return Err(StorageError::RecoveryConflict)
            }
            ContentKey::SharedReminder(k)
                if !identity.classes.iter().any(|c| c.id == k.class_id) =>
            {
                return Err(StorageError::RecoveryConflict)
            }
            _ => {}
        }
        if let Some(r) = revision {
            v["revision"] = json!(r.get())
        }
    } else if key == "business/desktop/max-observed-day/v1" {
        codec::object_fields(&v, &["format", "date"])?;
        codec::read_date(&v["date"])?;
        v["date"] = codec::date(floor);
    } else if key == "business/voice/v1" {
        let old = voice_codec::read_snapshot(&v)?;
        let mut s = old
            .backup_snapshot(old.observed_at.unwrap_or(0))
            .map_err(StorageError::Voice)?;
        if let Some(r) = revision {
            s.revision = r;
        }
        v = voice_codec::snapshot(&s);
    } else if key == "business/lesson-context/v1" || key == AUDIT {
        let mut s = lesson_codec::read_snapshot(&v)?;
        // Preserve witnessed historical intervals as inert audit only; an old
        // active override and baseline proof are never restored as live evidence.
        if let Some(c) = s.current.take() {
            if let Some(at) = s.observed_at {
                let end = at.min(c.effective_until);
                if end > c.effective_from {
                    if s.history.len() == fc_domain::lesson_context::MAX_LESSON_INTERVALS {
                        s.history.remove(0);
                    }
                    s.history
                        .push(fc_domain::lesson_context::EffectiveLessonInterval {
                            context: c,
                            ended_at: end,
                        });
                }
            }
        }
        s.active_override = None;
        s.base_trust = None;
        if let Some(r) = revision {
            s.revision = r;
            s.identity_revision = identity.revision;
        }
        s.validate().map_err(StorageError::Lesson)?;
        v = lesson_codec::snapshot(&s);
    } else if key == "business/profile-content/v1/counter" {
        codec::object_fields(&v, &["format", "heads", "receipts"])?;
        v["receipts"] = json!(0);
    } else if key.starts_with("business/profile-content/v1/heads/") {
        v["pending"] = Value::Null;
        if let Some(r) = revision {
            // Published versions form an adjacent immutable pair. Preserve
            // their exact history; fresh authority/layout generations and the
            // retired-command ledger fence old positive commands. View-only
            // callbacks additionally receive a new CAS generation.
            if !v["current"].is_null() {
                v["view"] = json!(r.get());
            }
            v["sequence"] = json!(codec::number(&v, "sequence")?.max(r.get()));
        }
    } else if key.starts_with("business/profile-media/v1/entries/") {
        if matches!(v["state"].as_str(), Some("reserved")) {
            v["state"] = json!("removed");
            v["resource"] = Value::Null;
        } else if v["state"] == "retiring" {
            v["state"] = json!("unreferenced");
        }
        if let Some(r) = revision {
            v["revision"] = json!(r.get());
        }
    } else if key.starts_with("business/material/v1/resources/") {
        if v["state"] == "retiring" {
            v["state"] = json!("unreferenced");
        }
        if let Some(r) = revision {
            v["revision"] = json!(r.get());
        }
    } else if key == "business/calendar/meta/v1" {
        let mut c = calendar_codec::read_cache(&v)?;
        c.policy.enabled = false;
        c.policy.grant = None;
        if let Some(r) = revision {
            c.sequence = c.sequence.max(r.get());
            c.policy.revision = r;
        }
        for y in &mut c.years {
            y.ticket = None;
            y.summary.pending = false;
        }
        v = calendar_codec::cache(&c);
    } else if let Some(r) = revision {
        if key == "business/display/v1"
            || key == "business/schedule/v1"
            || key == "business/profiles/v1/public"
            || key.starts_with("business/profiles/v1/documents/")
            || key.starts_with("business/material/v1/folders/")
            || key.starts_with("business/material/v1/items/")
        {
            v["revision"] = json!(r.get());
            if key.starts_with("business/material/v1/folders/") {
                v["sequence"] = json!(codec::number(&v, "sequence")?.max(r.get()));
            }
        }
        if key.starts_with("business/profiles/v1/pages/") {
            for e in v["entries"]
                .as_array_mut()
                .ok_or(StorageError::RecoveryInvalidPackage)?
            {
                e["revision"] = json!(r.get());
            }
        }
        if key.starts_with("business/profiles/v1/documents/") {
            v["layout_revision"] = json!(r.get());
        }
    }
    Ok(Some(display::encode(&v, MAX_VALUE_BYTES)?))
}
fn validate_lesson_refs(
    s: &fc_domain::lesson_context::LessonContextSnapshot,
    i: &IdentityState,
) -> Result<(), StorageError> {
    if s.class_id
        .is_some_and(|id| !i.classes.iter().any(|c| c.id == id))
    {
        return Err(StorageError::RecoveryConflict);
    }
    for c in s.current.iter().chain(s.history.iter().map(|h| &h.context)) {
        if !i.classes.iter().any(|x| x.id == c.key.class_id)
            || !i.subjects.iter().any(|x| x.id == c.base.subject_id)
            || !i.subjects.iter().any(|x| x.id == c.effective.subject_id)
            || c.teacher_id.is_some_and(|id| i.account(id).is_err())
        {
            return Err(StorageError::RecoveryConflict);
        }
    }
    Ok(())
}
pub(crate) fn validate_ordinary(
    d: &dyn TransactionDocuments,
    i: &IdentityState,
) -> Result<(), StorageError> {
    schedule::load(d, i)?;
    schedule::change_proof(d)?;
    display::load(d, i)?;
    profiles::startup_check(d, i)?;
    materials::startup_check(d, i)?;
    profile_content::startup_check(d, i)?;
    voice::startup_check(d, i)?;
    calendar::startup_check(d)?;
    desktop::read_watermark(d, i)?;
    for key in ["business/lesson-context/v1", AUDIT] {
        if let Some(row) = d.get(key)? {
            let s = lesson_codec::read_snapshot(&codec::value(&row.value)?)?;
            validate_lesson_refs(&s, i)?;
            if s.current.is_some()
                || s.active_override.is_some()
                || s.base_trust.is_some()
                || s.revision > d.revision()?
            {
                return Err(StorageError::RecoveryInvalidPackage);
            }
        }
    }
    let mut after = None;
    loop {
        let page = d.scan_prefix("business/", after.as_deref(), 128)?;
        for (k, row) in page.entries {
            if !allowed(&k) {
                return Err(StorageError::RecoveryInvalidPackage);
            }
            let v = codec::value(&row.value)?;
            if let Some(hash) = k.strip_prefix(crate::recovery::ORDINARY_RETIRED) {
                validate_retired_hash(hash)?;
                codec::object_fields(&v, &["format"])?;
                if codec::number(&v, "format")? != 1 {
                    return Err(StorageError::RecoveryInvalidPackage);
                }
            }
            if k.starts_with("business/material/v1/folders/") {
                let f = materials_codec::read_folder(&v)?;
                if i.account(f.scope.teacher_id).is_err()
                    || !i.subjects.iter().any(|s| s.id == f.scope.subject_id)
                {
                    return Err(StorageError::RecoveryConflict);
                }
            } else if k.starts_with("business/material/v1/items/") {
                let r = materials_codec::read_record(&v)?;
                if r.allowed_class_ids
                    .iter()
                    .any(|id| !i.classes.iter().any(|c| c.id == *id))
                {
                    return Err(StorageError::RecoveryConflict);
                }
            } else if k.starts_with("business/profile-media/v1/entries/") {
                if matches!(v["state"].as_str(), Some("reserved" | "retiring")) {
                    return Err(StorageError::RecoveryInvalidPackage);
                }
            } else if k.starts_with("business/profile-content/v1/heads/") {
                if !v["pending"].is_null() {
                    return Err(StorageError::RecoveryInvalidPackage);
                }
            } else if k == "business/voice/v1" {
                let s = voice_codec::read_snapshot(&v)?;
                if !s.assets.is_empty()
                    || s.jobs
                        .iter()
                        .any(|j| !j.state.terminal() || j.asset_id.is_some())
                {
                    return Err(StorageError::RecoveryInvalidPackage);
                }
            } else if k == "business/calendar/meta/v1" {
                let c = calendar_codec::read_cache(&v)?;
                if c.policy.enabled
                    || c.policy.grant.is_some()
                    || c.years
                        .iter()
                        .any(|y| y.ticket.is_some() || y.summary.pending)
                {
                    return Err(StorageError::RecoveryInvalidPackage);
                }
            } else if k.starts_with("business/material/v1/resources/") && v["state"] == "retiring" {
                return Err(StorageError::RecoveryInvalidPackage);
            }
        }
        after = page.next_key;
        if after.is_none() {
            break;
        }
    }
    Ok(())
}
pub(crate) fn validate_retired_hash(hash: &str) -> Result<(), StorageError> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Err(StorageError::RecoveryInvalidPackage)
    } else {
        Ok(())
    }
}

pub(crate) fn media(d: &dyn TransactionDocuments) -> Result<Vec<BackupEntry>, StorageError> {
    let mut result = Vec::new();
    let mut bytes = 0u64;
    for (prefix, material) in [
        ("business/material/v1/resources/", true),
        ("business/profile-media/v1/entries/", false),
    ] {
        let mut after = None;
        loop {
            let page = d.scan_prefix(prefix, after.as_deref(), 128)?;
            for (_, row) in page.entries {
                let v = codec::value(&row.value)?;
                let entry = if material {
                    let r = materials::read_inventory(&v)?;
                    if r.state == materials::ResourceState::Removed {
                        continue;
                    }
                    BackupEntry::Material {
                        teacher_id: r.scope.teacher_id,
                        subject_id: r.scope.subject_id,
                        material_id: r.material_id,
                        version_id: r.resource.version_id,
                        kind: r.resource.kind,
                        bytes: r.resource.bytes,
                        sha256: r.resource.sha256,
                    }
                } else {
                    if v["state"] == "removed" {
                        continue;
                    }
                    let r = profile_content_codec::read_media(&v["resource"])?;
                    BackupEntry::ProfileMedia {
                        version_id: r.version_id,
                        format: r.format,
                        bytes: r.bytes,
                        sha256: r.sha256,
                        width: r.width,
                        height: r.height,
                    }
                };
                bytes = bytes
                    .checked_add(entry.bytes())
                    .filter(|n| *n <= fc_domain::recovery::MAX_MEDIA_BYTES)
                    .ok_or(StorageError::ResourceQuota)?;
                result.push(entry);
                if result.len() > 16384 {
                    return Err(StorageError::ResourceQuota);
                }
            }
            after = page.next_key;
            if after.is_none() {
                break;
            }
        }
    }
    Ok(result)
}
pub(crate) fn read_floor(
    d: &dyn TransactionDocuments,
    i: &IdentityState,
) -> Result<Option<LocalDate>, StorageError> {
    desktop::read_watermark(d, i)
}
pub(crate) fn read_floor_untrusted(
    d: &dyn TransactionDocuments,
) -> Result<Option<LocalDate>, StorageError> {
    let Some(row) = d.get("business/desktop/max-observed-day/v1")? else {
        return Ok(None);
    };
    let v = codec::value(&row.value)?;
    codec::object_fields(&v, &["format", "date"])?;
    if codec::number(&v, "format")? != 1 {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    codec::read_date(&v["date"]).map(Some)
}
pub(crate) fn reset_lesson(
    db: &Connection,
    i: &IdentityState,
    day: LocalDate,
    revision: Revision,
) -> Result<(), StorageError> {
    let at = LocalDateTime::new(day, 0).map_err(|_| StorageError::RecoveryConflict)?;
    let s = fc_domain::lesson_context::LessonContextSnapshot {
        revision,
        identity_revision: i.revision,
        observed_at: Some(at),
        class_id: i.installation.as_ref().and_then(|s| s.current_class),
        blocked_until: Some(at),
        ..Default::default()
    };
    s.validate().map_err(StorageError::Lesson)?;
    db.execute("INSERT INTO documents(key,value,revision) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=excluded.revision",
        params!["business/lesson-context/v1",codec::bytes(&lesson_codec::snapshot(&s))?,revision.get()]).map_err(|_|StorageError::Database)?;
    Ok(())
}
pub(crate) fn retain_lesson_audit(
    db: &Connection,
    i: &IdentityState,
    r: Revision,
) -> Result<(), StorageError> {
    let read = |key: &str| -> Result<
        Option<fc_domain::lesson_context::LessonContextSnapshot>,
        StorageError,
    > {
        row_value(db, key)?
            .map(|v| lesson_codec::read_snapshot(&v))
            .transpose()
    };
    let old = read(AUDIT)?;
    let new = read("business/lesson-context/v1")?;
    let mut s = match (old, new) {
        (Some(mut old), Some(new)) if !old.history.is_empty() => {
            if !new.history.is_empty() {
                if old.class_id == new.class_id
                    && old.observed_at.map(|t| t.date) == new.observed_at.map(|t| t.date)
                {
                    for h in new.history {
                        if !old.history.contains(&h) {
                            old.history.push(h);
                        }
                    }
                    old.history
                        .sort_by_key(|h| (h.context.effective_from, h.ended_at));
                    if old.history.len() > fc_domain::lesson_context::MAX_LESSON_INTERVALS {
                        old.history.drain(
                            ..old.history.len() - fc_domain::lesson_context::MAX_LESSON_INTERVALS,
                        );
                    }
                    old.observed_at = old.observed_at.into_iter().chain(new.observed_at).max();
                } else if new.observed_at > old.observed_at {
                    old = new;
                }
            }
            old
        }
        (old, new) => new.or(old).unwrap_or_default(),
    };
    s.current = None;
    s.active_override = None;
    s.base_trust = None;
    s.revision = r;
    s.identity_revision = i.revision;
    s.validate().map_err(StorageError::Lesson)?;
    write(db, AUDIT, &lesson_codec::snapshot(&s), r)
}
pub(crate) fn merge_calendar_floor(
    db: &Connection,
    current: &Connection,
    revision: Revision,
) -> Result<(), StorageError> {
    let row = TransactionView(db).get("business/calendar/meta/v1")?;
    let old = TransactionView(current).get("business/calendar/meta/v1")?;
    if row.is_none() && old.is_none() {
        return Ok(());
    }
    let mut c = row
        .map(|r| calendar_codec::read_cache(&codec::value(&r.value)?))
        .transpose()?
        .unwrap_or_default();
    let old = old
        .map(|r| calendar_codec::read_cache(&codec::value(&r.value)?))
        .transpose()?
        .unwrap_or_default();
    c.sequence = c
        .sequence
        .max(old.sequence)
        .checked_add(1)
        .ok_or(StorageError::RecoveryConflict)?;
    c.adoption_count = c.adoption_count.max(old.adoption_count);
    c.watermark = c.watermark.into_iter().chain(old.watermark).max();
    c.policy.enabled = false;
    c.policy.grant = None;
    c.policy.revision = revision;
    for y in &mut c.years {
        y.ticket = None;
        y.summary.pending = false;
        if let Some(prior) = old.years.iter().find(|o| o.summary.year == y.summary.year) {
            y.summary.last_attempt = y
                .summary
                .last_attempt
                .into_iter()
                .chain(prior.summary.last_attempt)
                .max();
            y.summary.next_allowed = y
                .summary
                .next_allowed
                .into_iter()
                .chain(prior.summary.next_allowed)
                .max();
        }
    }
    // Retain CURRENT attempt-only years as well: restoring an older package
    // cannot erase today's fetch backoff or permit another daily request.
    for mut prior in old.years {
        if !c.years.iter().any(|r| r.summary.year == prior.summary.year) {
            prior.ticket = None;
            prior.summary.pending = false;
            // Keep the current candidate bytes with its exact metadata.
            let key = format!("business/calendar/candidate/v1/{:04}", prior.summary.year);
            if let Some(row) = TransactionView(current).get(&key)? {
                db.execute(
                    "INSERT INTO documents(key,value,revision) VALUES(?1,?2,?3)",
                    params![key, row.value, revision.get()],
                )
                .map_err(|_| StorageError::Database)?;
            }
            c.years.push(prior);
        }
    }
    c.years.sort_by_key(|y| y.summary.year);
    c.validate()?;
    db.execute("INSERT INTO documents(key,value,revision) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=excluded.revision",
        params!["business/calendar/meta/v1",codec::bytes(&calendar_codec::cache(&c))?,revision.get()]).map_err(|_|StorageError::Database)?;
    Ok(())
}
fn row_value(db: &Connection, key: &str) -> Result<Option<Value>, StorageError> {
    TransactionView(db)
        .get(key)?
        .map(|r| codec::value(&r.value))
        .transpose()
}
fn write(db: &Connection, key: &str, v: &Value, r: Revision) -> Result<(), StorageError> {
    let bytes = display::encode(v, MAX_VALUE_BYTES)?;
    db.execute("INSERT INTO documents(key,value,revision) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=excluded.revision",
        params![key,bytes,r.get()]).map_err(|_|StorageError::Database)?;
    Ok(())
}
/// Keep CURRENT allocator floors even when business contents go back in time.
/// Sequential registries retain empty/Removed tombstones so existing codecs'
/// bidirectional count/index checks continue to prove their complete shape.
pub(crate) fn merge_allocator_floors(
    db: &Connection,
    current: &Connection,
    i: &IdentityState,
    r: Revision,
) -> Result<(), StorageError> {
    let old_voice = TransactionView(current)
        .get("business/voice/v1")?
        .map(|row| voice_codec::read_snapshot(&codec::value(&row.value)?))
        .transpose()?
        .unwrap_or_default();
    let imported_voice = TransactionView(db)
        .get("business/voice/v1")?
        .map(|row| voice_codec::read_snapshot(&codec::value(&row.value)?))
        .transpose()?;
    if imported_voice.is_some() || old_voice.revision.get() > 0 {
        let mut s = imported_voice.unwrap_or_default();
        s.revision = r;
        s.next_sequence = s
            .next_sequence
            .max(old_voice.next_sequence)
            .checked_add(1)
            .ok_or(StorageError::RecoveryConflict)?;
        s.next_generation = s
            .next_generation
            .max(old_voice.next_generation)
            .checked_add(1)
            .ok_or(StorageError::RecoveryConflict)?;
        s.observed_at = s.observed_at.into_iter().chain(old_voice.observed_at).max();
        s.validate().map_err(StorageError::Voice)?;
        write(db, "business/voice/v1", &voice_codec::snapshot(&s), r)?;
    }
    let old_i = load_identity(&TransactionView(current))?;
    let old_schedule = schedule::load(&TransactionView(current), &old_i)?;
    let mut s = schedule::load(&TransactionView(db), i)?;
    s.id_high_water = s.id_high_water.max(old_schedule.id_high_water);
    if s.id_high_water > 0 || s.revision.get() > 0 {
        s.revision = r;
        write(db, "business/schedule/v1", &schedule_codec::document(&s), r)?;
    }
    let old_public = profiles::load_public(&TransactionView(current), &old_i)?;
    let mut public = profiles::load_public(&TransactionView(db), i)?;
    public.node_id_high_water = public.node_id_high_water.max(old_public.node_id_high_water);
    let mut manifest = row_value(db, "business/profiles/v1/manifest")?
        .unwrap_or(json!({"format":1,"count":0,"public":false}));
    if public.node_id_high_water > fc_domain::profile::LayoutDocument::default().node_id_high_water
        || public.revision.get() > 0
    {
        public.revision = r;
        write(
            db,
            "business/profiles/v1/public",
            &profiles_codec::public(&public),
            r,
        )?;
        manifest["public"] = json!(true);
    }
    let old_manifest = row_value(current, "business/profiles/v1/manifest")?
        .unwrap_or(json!({"format":1,"count":0,"public":false}));
    let count = codec::number(&manifest, "count")?.max(codec::number(&old_manifest, "count")?);
    let mut summaries = Vec::with_capacity(64);
    for n in 1..=count {
        let key = format!("business/profiles/v1/documents/{n:020}");
        let old = row_value(current, &key)?;
        let imported = row_value(db, &key)?;
        let mut p = match imported {
            Some(v) => profiles_codec::read_subject(&v)?,
            None => {
                let mut p = profiles_codec::read_subject(
                    &old.clone().ok_or(StorageError::RecoveryConflict)?,
                )?;
                p.enabled = false;
                p
            }
        };
        if let Some(old) = old {
            let old = profiles_codec::read_subject(&old)?;
            if old.key != p.key {
                return Err(StorageError::RecoveryConflict);
            }
            p.node_id_high_water = p.node_id_high_water.max(old.node_id_high_water);
        }
        p.revision = r;
        p.layout_revision = r;
        // A retained high-numbered empty shell preserves its stable slot only;
        // no teacher content heads or original media are copied from CURRENT.
        let slot = format!(
            "business/profiles/v1/slots/{:020}/{:020}/{:020}",
            p.key.class_id.get(),
            p.key.subject_id.get(),
            p.key.teacher_id.get()
        );
        if let Some(v) = row_value(db, &slot)? {
            if codec::number(&v, "id")? != n {
                return Err(StorageError::RecoveryConflict);
            }
        }
        write(db, &slot, &json!({"id":n}), r)?;
        write(db, &key, &profiles_codec::subject(&p), r)?;
        summaries.push(
            json!({"id":n,"key":profiles_codec::key(p.key),"revision":r.get(),"enabled":p.enabled}),
        );
        if n % 64 == 0 || n == count {
            let page = (n - 1) / 64;
            write(
                db,
                &format!("business/profiles/v1/pages/{page:020}"),
                &json!({"format":1,"entries":summaries}),
                r,
            )?;
            summaries.clear();
        }
    }
    manifest["count"] = json!(count);
    if count > 0 || manifest["public"] == true {
        write(db, "business/profiles/v1/manifest", &manifest, r)?;
    }
    let mut catalog = row_value(db, "business/material/v1/catalog")?
        .unwrap_or(json!({"format":1,"folders":0,"records":0,"resources":0,"bytes":0}));
    let old_catalog = row_value(current, "business/material/v1/catalog")?
        .unwrap_or(json!({"format":1,"folders":0,"records":0,"resources":0,"bytes":0}));
    let folders = codec::number(&catalog, "folders")?.max(codec::number(&old_catalog, "folders")?);
    for n in 1..=folders {
        let key = format!("business/material/v1/folders/{n:020}");
        let old = row_value(current, &key)?;
        let mut v = row_value(db, &key)?
            .or_else(|| old.clone())
            .ok_or(StorageError::RecoveryConflict)?;
        let f = materials_codec::read_folder(&v)?;
        if let Some(old) = old {
            let old = materials_codec::read_folder(&old)?;
            if old.scope != f.scope {
                return Err(StorageError::RecoveryConflict);
            }
            v["sequence"] = json!(codec::number(&v, "sequence")?
                .max(old.last_added_sequence)
                .max(r.get()));
        }
        v["revision"] = json!(r.get());
        v["sequence"] = json!(codec::number(&v, "sequence")?.max(r.get()));
        let scope = format!(
            "business/material/v1/scope/{:020}/{:020}",
            f.scope.teacher_id.get(),
            f.scope.subject_id.get()
        );
        if let Some(v) = row_value(db, &scope)? {
            if codec::number(&v, "id")? != n {
                return Err(StorageError::RecoveryConflict);
            }
        }
        write(db, &scope, &json!({"id":n}), r)?;
        write(db, &key, &v, r)?;
    }
    catalog["folders"] = json!(folders);
    if folders > 0 {
        write(db, "business/material/v1/catalog", &catalog, r)?;
    }
    let mut catalog = row_value(db, "business/profile-media/v1/catalog")?
        .unwrap_or(json!({"format":1,"count":0,"bytes":0}));
    let old_catalog = row_value(current, "business/profile-media/v1/catalog")?
        .unwrap_or(json!({"format":1,"count":0,"bytes":0}));
    let start = codec::number(&catalog, "count")?;
    let count = start.max(codec::number(&old_catalog, "count")?);
    for n in start + 1..=count {
        let key = format!("business/profile-media/v1/entries/{n:020}");
        let mut v = row_value(current, &key)?.ok_or(StorageError::RecoveryConflict)?;
        v["state"] = json!("removed");
        v["resource"] = Value::Null;
        v["refs"] = json!(0);
        v["revision"] = json!(r.get());
        write(db, &key, &v, r)?;
        write(
            db,
            &format!("business/profile-media/v1/recovery-owners/{n:020}"),
            &json!({"format":1,"id":n}),
            r,
        )?;
    }
    catalog["count"] = json!(count);
    if count > 0 {
        write(db, "business/profile-media/v1/catalog", &catalog, r)?;
    }
    Ok(())
}
