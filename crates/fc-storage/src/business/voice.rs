//! Durable voice commands on the unique writer. This module never reads a WAV
//! or calls an audio API; actual file preparation/deletion and driver execution
//! are separate bounded workers with exact revision/generation receipts.
use super::{voice_codec as dto, *};
use fc_domain::voice::*;
use fc_ports::voice::*;
const DOCUMENT: &str = "business/voice/v1";
const RECEIPT_PREFIX: &str = "business/voice-receipt/v1/";
fn receipt_key(id: CommandId) -> String {
    format!("{RECEIPT_PREFIX}{:020}", id.get())
}
fn encode(v: &Value) -> Result<Vec<u8>, StorageError> {
    display::encode(v, MAX_VALUE_BYTES)
}
fn mutation(s: &VoiceSnapshot) -> Result<Mutation, StorageError> {
    Ok(Mutation::Put {
        key: DOCUMENT.into(),
        value: encode(&dto::snapshot(s))?,
    })
}
fn load(d: &dyn TransactionDocuments) -> Result<VoiceSnapshot, StorageError> {
    let s = match d.get(DOCUMENT)? {
        None => VoiceSnapshot::default(),
        Some(row) => dto::read_snapshot(&codec::value(&row.value)?)?,
    };
    if s.revision > d.revision()? {
        return Err(StorageError::Database);
    }
    Ok(s)
}
pub(super) fn startup_check(
    d: &dyn TransactionDocuments,
    i: &IdentityState,
) -> Result<(), StorageError> {
    let s = load(d)?;
    if i.installation.is_none()
        && (!s.assets.is_empty() || !s.jobs.is_empty() || s.revision != Revision::default())
    {
        return Err(StorageError::Database);
    }
    for (owner, class_id) in s
        .assets
        .iter()
        .map(|a| (a.owner, a.class_id))
        .chain(s.jobs.iter().map(|j| (j.owner, j.class_id)))
    {
        if i.account(owner).is_err() || !i.classes.iter().any(|c| c.id == class_id) {
            return Err(StorageError::Database);
        }
    }
    Ok(())
}
fn error(e: StorageError) -> VoiceRepositoryError {
    match e {
        StorageError::Voice(e) => VoiceRepositoryError::Domain(e),
        StorageError::CommandIdReused => VoiceRepositoryError::IdempotencyConflict,
        StorageError::DocumentTooLarge => VoiceRepositoryError::DocumentTooLarge,
        StorageError::AuthorityUnavailable
        | StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::WorkerPanicked => VoiceRepositoryError::Faulted,
        _ => VoiceRepositoryError::Unavailable,
    }
}
fn err(e: VoiceError) -> StorageError {
    StorageError::Voice(e)
}
fn authorize(
    i: &IdentityState,
    actor: &ActorContext,
    class: ClassId,
    clock: &dyn Clock,
    session: &SessionGuard,
) -> Result<(), StorageError> {
    session(i, actor)?;
    authorize_submit(i, actor, class, clock.now()).map_err(err)
}
fn query_authorize(
    i: &IdentityState,
    actor: &ActorContext,
    job: &VoiceJob,
    clock: &dyn Clock,
    session: &SessionGuard,
) -> Result<(), StorageError> {
    session(i, actor)?;
    authorize_job(i, actor, job, clock.now()).map_err(err)
}
enum Operation {
    Reserve {
        id: VoiceAssetId,
        class_id: ClassId,
        actor: ActorContext,
    },
    Publish {
        reservation: VoiceAsset,
        pcm: VoicePcmMetadata,
        actor: ActorContext,
    },
    Submit {
        request: VoiceSubmitRequest,
        actor: ActorContext,
    },
    Query {
        id: VoiceJobId,
        actor: ActorContext,
    },
    Cancel {
        request: VoiceCancelRequest,
        actor: ActorContext,
    },
    Driver(VoiceDriverUpdate),
    Recover,
    Inventory,
    Maintain,
    Finalize {
        id: VoiceAssetId,
        expected: Revision,
    },
    EmergencyStop,
    SetEnabled {
        enabled: bool,
        actor: ActorContext,
        expected: Revision,
    },
}
impl Operation {
    fn positive(&self) -> bool {
        matches!(
            self,
            Self::Reserve { .. }
                | Self::Publish { .. }
                | Self::Submit { .. }
                | Self::Driver(VoiceDriverUpdate {
                    action: VoiceDriverAction::Start
                        | VoiceDriverAction::FirstWrite
                        | VoiceDriverAction::NaturalComplete,
                    ..
                })
                | Self::SetEnabled { enabled: true, .. }
        )
    }
}
impl<S: Send + 'static> BusinessRepository<S> {
    fn voice_operation(&self, op: Operation) -> Result<Value, VoiceRepositoryError> {
        let active =
            self.voice_timeout
                .as_ref()
                .filter(|_| op.positive())
                .map(|(timeout, generation)| {
                    let deadline = std::time::Instant::now().checked_add(*timeout);
                    let generation = generation.clone();
                    Arc::new(move || {
                        deadline.is_some_and(|d| std::time::Instant::now() < d) && generation()
                    }) as Arc<dyn Fn() -> bool + Send + Sync>
                });
        if active.as_ref().is_some_and(|check| !check()) {
            return Err(VoiceRepositoryError::Faulted);
        }
        let ticket = self
            .handle
            .try_transaction(VoiceTask {
                op,
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
                active,
            })
            .map_err(error)?;
        let bytes = match &self.voice_timeout {
            Some((timeout, _)) => ticket.wait_timeout(*timeout),
            None => ticket.wait(),
        }
        .map_err(error)?;
        codec::value(&bytes).map_err(error)
    }
    /// Trusted executor only. Start and FirstWrite recheck the original exact
    /// Web lease and current binding both during evaluation and before COMMIT.
    pub fn drive_voice(&self, update: VoiceDriverUpdate) -> Result<VoiceJob, VoiceRepositoryError> {
        dto::read_job(&self.voice_operation(Operation::Driver(update))?).map_err(error)
    }
    /// Startup must own the exclusive installation lock and call this before
    /// constructing any playback executor. It never exposes a replayable task.
    pub fn recover_voice(&self) -> Result<VoiceSnapshot, VoiceRepositoryError> {
        dto::read_snapshot(&self.voice_operation(Operation::Recover)?).map_err(error)
    }
    /// Internal inventory contains exact lease data for nonterminal jobs. Never
    /// return this DTO from Web APIs or put it into a normal UI snapshot.
    pub fn maintain_voice(&self) -> Result<VoiceSnapshot, VoiceRepositoryError> {
        dto::read_snapshot(&self.voice_operation(Operation::Maintain)?).map_err(error)
    }
    pub fn voice_inventory(&self) -> Result<VoiceSnapshot, VoiceRepositoryError> {
        dto::read_snapshot(&self.voice_operation(Operation::Inventory)?).map_err(error)
    }
    /// Filesystem composition must exclusively delete the exact controlled ID
    /// first, then finalize the matching asset revision to release charged bytes.
    pub fn finalize_voice_delete(
        &self,
        id: VoiceAssetId,
        expected: Revision,
    ) -> Result<(), VoiceRepositoryError> {
        self.voice_operation(Operation::Finalize { id, expected })?;
        Ok(())
    }
    /// Trusted local tray/control path, callable without manager unlock. Never
    /// route this capability through an authenticated teacher HTTP request.
    pub fn emergency_stop_voice(&self) -> Result<VoiceSnapshot, VoiceRepositoryError> {
        dto::read_snapshot(&self.voice_operation(Operation::EmergencyStop)?).map_err(error)
    }
    pub fn set_voice_enabled(
        &self,
        enabled: bool,
        expected: Revision,
        actor: &ActorContext,
    ) -> Result<VoiceSnapshot, VoiceRepositoryError> {
        dto::read_snapshot(&self.voice_operation(Operation::SetEnabled {
            enabled,
            expected,
            actor: actor.clone(),
        })?)
        .map_err(error)
    }
}
impl<S: Send + 'static> VoiceRepository for BusinessRepository<S> {
    fn reserve_voice_upload(
        &self,
        id: VoiceAssetId,
        class_id: ClassId,
        actor: &ActorContext,
    ) -> Result<VoiceAsset, VoiceRepositoryError> {
        dto::read_asset(&self.voice_operation(Operation::Reserve {
            id,
            class_id,
            actor: actor.clone(),
        })?)
        .map_err(error)
    }
    fn publish_voice_upload(
        &self,
        prepared: PreparedVoiceUpload,
        actor: &ActorContext,
    ) -> Result<VoiceAsset, VoiceRepositoryError> {
        let (reservation, pcm) = prepared.into_parts();
        dto::read_asset(&self.voice_operation(Operation::Publish {
            reservation,
            pcm,
            actor: actor.clone(),
        })?)
        .map_err(error)
    }
    fn submit_voice(
        &self,
        request: VoiceSubmitRequest,
        actor: &ActorContext,
    ) -> Result<VoiceJob, VoiceRepositoryError> {
        dto::read_job(&self.voice_operation(Operation::Submit {
            request,
            actor: actor.clone(),
        })?)
        .map_err(error)
        .map(public_job)
    }
    fn voice_job(
        &self,
        id: VoiceJobId,
        actor: &ActorContext,
    ) -> Result<VoiceJob, VoiceRepositoryError> {
        dto::read_job(&self.voice_operation(Operation::Query {
            id,
            actor: actor.clone(),
        })?)
        .map_err(error)
        .map(public_job)
    }
    fn cancel_voice(
        &self,
        request: VoiceCancelRequest,
        actor: &ActorContext,
    ) -> Result<VoiceJob, VoiceRepositoryError> {
        dto::read_job(&self.voice_operation(Operation::Cancel {
            request,
            actor: actor.clone(),
        })?)
        .map_err(error)
        .map(public_job)
    }
}
struct VoiceTask<S> {
    op: Operation,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
    active: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}
/// Fixed-size per-account limiter persists independently of reclaimable bodies.
/// Installation account bounds cap this table at MAX_ACCOUNTS documents.
fn rate(
    d: &dyn TransactionDocuments,
    owner: PrincipalId,
    now: i64,
    upload: bool,
) -> Result<Mutation, StorageError> {
    let key = format!("business/voice-rate/v1/{:020}", owner.get());
    let (mut at, mut uploads, mut submits) = match d.get(&key)? {
        None => (now, 0u64, 0u64),
        Some(row) => {
            let v = codec::value(&row.value)?;
            codec::object_fields(&v, &["at", "uploads", "submits"])?;
            let at = v["at"].as_i64().ok_or(StorageError::Database)?;
            let u = codec::number(&v, "uploads")?;
            let s = codec::number(&v, "submits")?;
            if !dto::valid_timestamp(at) || u > 10 || s > 10 {
                return Err(StorageError::Database);
            }
            (at, u, s)
        }
    };
    if now < at {
        return Err(err(VoiceError::ClockRegression));
    }
    if now.saturating_sub(at) >= 60 {
        at = now;
        uploads = 0;
        submits = 0;
    }
    let n = if upload { &mut uploads } else { &mut submits };
    if *n >= 10 {
        return Err(err(VoiceError::RateLimited));
    }
    *n += 1;
    Ok(Mutation::Put {
        key,
        value: encode(&json!({"at":at,"uploads":uploads,"submits":submits}))?,
    })
}
fn prune(
    s: &mut VoiceSnapshot,
    now: i64,
    mutations: &mut Vec<Mutation>,
) -> Result<(), StorageError> {
    for job in s.maintain(now).map_err(err)? {
        mutations.push(Mutation::Delete {
            key: receipt_key(job.command_id),
        });
        if let Some(id) = job.cancel_command {
            mutations.push(Mutation::Delete {
                key: receipt_key(id),
            });
        }
    }
    Ok(())
}
fn fingerprint(actor: &ActorContext, payload: Value) -> Result<Vec<u8>, StorageError> {
    // Deliberately excludes expiry: a refreshed lease with the same principal /
    // epoch may query its original command. The exact current session still gates.
    Ok(Sha256::digest(codec::bytes(
        &json!({"actor":codec::actor(actor),"payload":payload}),
    )?)
    .to_vec())
}
fn expired(job: &VoiceJob, now: i64) -> bool {
    job.state.terminal()
        && job
            .ended_at
            .is_some_and(|end| now.saturating_sub(end) >= AUDIT_TTL_SECONDS)
}
fn public_job(mut job: VoiceJob) -> VoiceJob {
    job.authorization = None;
    job
}
fn replay(
    d: &dyn TransactionDocuments,
    snapshot: &VoiceSnapshot,
    id: CommandId,
    digest: &[u8],
    now: i64,
) -> Result<Option<VoiceJob>, StorageError> {
    let Some(row) = d.get(&receipt_key(id))? else {
        return Ok(None);
    };
    let v = codec::value(&row.value)?;
    codec::object_fields(&v, &["fingerprint", "job", "owner", "digest"])?;
    let stored: Vec<u8> =
        serde_json::from_value(v["fingerprint"].clone()).map_err(|_| StorageError::Database)?;
    let integrity: Vec<u8> =
        serde_json::from_value(v["digest"].clone()).map_err(|_| StorageError::Database)?;
    let job_id = codec::number(&v, "job")?;
    let owner = codec::number(&v, "owner")?;
    let bound = codec::bytes(&json!({"fingerprint":stored,"job":job_id,"owner":owner}))?;
    if stored.len() != 32
        || integrity.len() != 32
        || integrity.as_slice() != Sha256::digest(bound).as_slice()
    {
        return Err(StorageError::Database);
    }
    if stored != digest {
        return Err(StorageError::CommandIdReused);
    }
    let job = snapshot
        .jobs
        .iter()
        .find(|j| j.id.get() == job_id)
        .ok_or(err(VoiceError::Missing))?;
    if expired(job, now) {
        return Err(err(VoiceError::Missing));
    }
    if job.owner.get() != owner || (job.command_id != id && job.cancel_command != Some(id)) {
        return Err(StorageError::Database);
    }
    Ok(Some(job.clone()))
}
fn receipt(id: CommandId, fingerprint: Vec<u8>, job: &VoiceJob) -> Result<Mutation, StorageError> {
    let bound = json!({"fingerprint":fingerprint,"job":job.id.get(),"owner":job.owner.get()});
    Ok(Mutation::Put {
        key: receipt_key(id),
        value: encode(
            &json!({"fingerprint":fingerprint,"job":job.id.get(),"owner":job.owner.get(),"digest":Sha256::digest(codec::bytes(&bound)?).to_vec()}),
        )?,
    })
}
impl<S: Send + 'static> TransactionTask<S> for VoiceTask<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        if self.active.as_ref().is_some_and(|check| !check()) {
            return Err(StorageError::Indeterminate);
        }
        let identity = load_identity(d)?;
        startup_check(d, &identity)?;
        let mut s = load(d)?;
        let original = s.clone();
        let now = civil_seconds(self.clock.now());
        let mut mutations = Vec::new();
        // Maintenance participates in every mutating commit; read-only queries
        // never silently move state or release any quota.
        if !matches!(
            self.op,
            Operation::Query { .. }
                | Operation::Inventory
                | Operation::Recover
                | Operation::Driver(VoiceDriverUpdate {
                    action: VoiceDriverAction::ExpireBeforeWrite,
                    ..
                })
        ) {
            prune(&mut s, now, &mut mutations)?;
        }
        let mut pre_actor: Option<(ActorContext, Option<ClassId>)> = None;
        let snapshot_outcome = matches!(
            self.op,
            Operation::Recover
                | Operation::Maintain
                | Operation::EmergencyStop
                | Operation::SetEnabled { .. }
        );
        let outcome = match self.op {
            Operation::Reserve {
                id,
                class_id,
                actor,
            } => {
                authorize(
                    &identity,
                    &actor,
                    class_id,
                    self.clock.as_ref(),
                    &self.session,
                )?;
                mutations.push(rate(d, actor.principal_id, now, true)?);
                let result = s.reserve(id, &actor, class_id, now).map_err(err)?;
                pre_actor = Some((actor, Some(class_id)));
                dto::asset(&result)
            }
            Operation::Publish {
                reservation,
                pcm,
                actor,
            } => {
                authorize(
                    &identity,
                    &actor,
                    reservation.class_id,
                    self.clock.as_ref(),
                    &self.session,
                )?;
                if reservation.owner != actor.principal_id {
                    return Err(err(VoiceError::Permission(PermissionError::WrongOwner)));
                }
                if s.assets.iter().find(|a| a.id == reservation.id) != Some(&reservation) {
                    return Err(err(VoiceError::RevisionConflict));
                }
                let result = s
                    .publish(reservation.id, reservation.revision, pcm, now)
                    .map_err(err)?;
                pre_actor = Some((actor, Some(reservation.class_id)));
                dto::asset(&result)
            }
            Operation::Submit { request: r, actor } => {
                authorize(
                    &identity,
                    &actor,
                    r.class_id,
                    self.clock.as_ref(),
                    &self.session,
                )?;
                let hash = fingerprint(
                    &actor,
                    json!({"kind":"submit","asset":r.asset_id.get(),"expected":r.expected_asset_revision.get(),"class":r.class_id.get(),"repeat":r.repeat_total}),
                )?;
                if let Some(job) = replay(d, &s, r.command_id, &hash, now)? {
                    if job.command_id != r.command_id
                        || job.id.get() != r.command_id.get()
                        || job.owner != actor.principal_id
                        || job.class_id != r.class_id
                        || job.repeat_total != r.repeat_total
                    {
                        return Err(StorageError::Database);
                    }
                    query_authorize(&identity, &actor, &job, self.clock.as_ref(), &self.session)?;
                    return Ok(TransactionDecision {
                        outcome: encode(&dto::job(&job))?,
                        commit: None,
                        precommit: None,
                    });
                }
                let asset = s
                    .assets
                    .iter()
                    .find(|a| a.id == r.asset_id)
                    .ok_or(err(VoiceError::Missing))?;
                if asset.class_id != r.class_id {
                    return Err(err(VoiceError::TargetMismatch));
                }
                mutations.push(rate(d, actor.principal_id, now, false)?);
                let result = s
                    .submit(
                        r.command_id,
                        r.asset_id,
                        r.expected_asset_revision,
                        r.repeat_total,
                        &actor,
                        now,
                    )
                    .map_err(err)?;
                mutations.push(receipt(r.command_id, hash, &result)?);
                pre_actor = Some((actor, Some(r.class_id)));
                dto::job(&result)
            }
            Operation::Query { id, actor } => {
                let job = s
                    .jobs
                    .iter()
                    .find(|j| j.id == id)
                    .ok_or(err(VoiceError::Missing))?;
                query_authorize(&identity, &actor, job, self.clock.as_ref(), &self.session)?;
                if expired(job, now) {
                    return Err(err(VoiceError::Missing));
                }
                return Ok(TransactionDecision {
                    outcome: encode(&dto::job(job))?,
                    commit: None,
                    precommit: None,
                });
            }
            Operation::Cancel { request: r, actor } => {
                let current = s
                    .jobs
                    .iter()
                    .find(|j| j.id == r.job_id)
                    .ok_or(err(VoiceError::Missing))?;
                query_authorize(
                    &identity,
                    &actor,
                    current,
                    self.clock.as_ref(),
                    &self.session,
                )?;
                let hash = fingerprint(
                    &actor,
                    json!({"kind":"cancel","job":r.job_id.get(),"expected":r.expected_revision.get()}),
                )?;
                if let Some(job) = replay(d, &s, r.command_id, &hash, now)? {
                    if job.id != r.job_id || job.cancel_command != Some(r.command_id) {
                        return Err(StorageError::Database);
                    }
                    return Ok(TransactionDecision {
                        outcome: encode(&dto::job(&job))?,
                        commit: None,
                        precommit: None,
                    });
                }
                let result = s
                    .cancel(
                        r.job_id,
                        r.expected_revision,
                        r.command_id,
                        now.max(s.observed_at.unwrap_or(now)),
                    )
                    .map_err(err)?;
                mutations.push(receipt(r.command_id, hash, &result)?);
                pre_actor = Some((actor, None));
                dto::job(&result)
            }
            Operation::Driver(update) => {
                if matches!(
                    update.action,
                    VoiceDriverAction::Start | VoiceDriverAction::FirstWrite
                ) {
                    let job = s
                        .jobs
                        .iter()
                        .find(|j| j.id == update.job_id)
                        .ok_or(err(VoiceError::Missing))?;
                    let actor = job
                        .authorization
                        .clone()
                        .ok_or(err(VoiceError::InvalidState))?;
                    authorize(
                        &identity,
                        &actor,
                        job.class_id,
                        self.clock.as_ref(),
                        &self.session,
                    )?;
                    pre_actor = Some((actor, Some(job.class_id)));
                }
                let result = s
                    .drive(
                        &update,
                        now.max(
                            if matches!(
                                update.action,
                                VoiceDriverAction::Released
                                    | VoiceDriverAction::Interrupt
                                    | VoiceDriverAction::Fail(_)
                            ) {
                                s.observed_at.unwrap_or(now)
                            } else {
                                now
                            },
                        ),
                    )
                    .map_err(err)?;
                dto::job(&result)
            }
            Operation::Recover => {
                s.recover(now).map_err(err)?;
                dto::snapshot(&s)
            }
            Operation::Maintain => dto::snapshot(&s),
            Operation::Inventory => {
                return Ok(TransactionDecision {
                    outcome: encode(&dto::snapshot(&s))?,
                    commit: None,
                    precommit: None,
                })
            }
            Operation::Finalize { id, expected } => {
                s.finalize_delete(id, expected, now).map_err(err)?;
                Value::Null
            }
            Operation::EmergencyStop => {
                let jobs: Vec<_> = s
                    .jobs
                    .iter()
                    .filter(|j| !j.state.terminal() && j.state != VoiceState::Stopping)
                    .map(|j| (j.id, j.revision, j.generation))
                    .collect();
                for (id, expected_revision, generation) in jobs {
                    s.drive(
                        &VoiceDriverUpdate {
                            job_id: id,
                            expected_revision,
                            generation,
                            action: VoiceDriverAction::Interrupt,
                        },
                        now.max(s.observed_at.unwrap_or(now)),
                    )
                    .map_err(err)?;
                }
                dto::snapshot(&s)
            }
            Operation::SetEnabled {
                enabled,
                actor,
                expected,
            } => {
                (self.session)(&identity, &actor)?;
                identity
                    .authorize_management(&actor, self.clock.now())
                    .map_err(StorageError::Identity)?;
                if s.revision != expected {
                    return Err(err(VoiceError::RevisionConflict));
                }
                s.enabled = enabled;
                // Disabling retires all accepted uploads and cancels every job,
                // retaining old device ownership until exact release receipts.
                if !enabled {
                    for asset in &mut s.assets {
                        if matches!(
                            asset.state,
                            VoiceAssetState::Reserved | VoiceAssetState::Ready
                        ) {
                            asset.state = VoiceAssetState::DeletePending;
                            asset.revision = asset
                                .revision
                                .next()
                                .map_err(|_| StorageError::InvalidCommand)?;
                        }
                    }
                    let jobs: Vec<_> = s
                        .jobs
                        .iter()
                        .filter(|j| !j.state.terminal() && j.state != VoiceState::Stopping)
                        .map(|j| (j.id, j.revision, j.generation))
                        .collect();
                    for (job_id, expected_revision, generation) in jobs {
                        s.drive(
                            &VoiceDriverUpdate {
                                job_id,
                                expected_revision,
                                generation,
                                action: VoiceDriverAction::Fail(VoiceFailure::Disabled),
                            },
                            now.max(s.observed_at.unwrap_or(now)),
                        )
                        .map_err(err)?;
                    }
                }
                s.revision = s
                    .revision
                    .next()
                    .map_err(|_| StorageError::InvalidCommand)?;
                pre_actor = Some((actor, None));
                dto::snapshot(&s)
            }
        };
        prune(&mut s, now, &mut mutations)?;
        if s == original && mutations.is_empty() {
            return Ok(TransactionDecision {
                outcome: encode(&outcome)?,
                commit: None,
                precommit: None,
            });
        }
        s.revision = original
            .revision
            .next()
            .map_err(|_| StorageError::InvalidCommand)?;
        s.validate().map_err(err)?;
        // Snapshot revision is a command-local CAS, independent of unrelated
        // identity or homework revisions in the global writer.
        mutations.push(mutation(&s)?);
        let prepared = (self.factory)(
            &identity,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        let precommit = pre_actor.map(|(actor, class)| {
            let session = self.session.clone();
            let clock = self.clock.clone();
            let identity = identity.clone();
            Box::new(move || {
                session(&identity, &actor)?;
                if let Some(class) = class {
                    authorize_submit(&identity, &actor, class, clock.now()).map_err(err)?;
                } else {
                    let a = identity
                        .authority(actor.principal_id)
                        .map_err(StorageError::Identity)?;
                    a.validate_actor(&actor, clock.now())
                        .map_err(|e| err(VoiceError::Permission(e)))?;
                }
                Ok(())
            }) as Box<dyn FnOnce() -> Result<(), StorageError> + Send>
        });
        let precommit = match self.active {
            Some(active) => Some(Box::new(move || {
                if !active() {
                    return Err(StorageError::Indeterminate);
                }
                precommit.map_or(Ok(()), |check| check())
            })
                as Box<dyn FnOnce() -> Result<(), StorageError> + Send>),
            None => precommit,
        };
        let outcome = if snapshot_outcome {
            dto::snapshot(&s)
        } else {
            outcome
        };
        Ok(TransactionDecision {
            outcome: encode(&outcome)?,
            commit: Some((mutations, prepared)),
            precommit,
        })
    }
}
/// Called on the completed backup destination, never the live connection.
/// The destination transaction strips both active bytes and original queued
/// command receipts; only redacted terminal audits remain replay-inert.
pub(crate) fn sanitize_backup(db: &mut Connection) -> Result<(), StorageError> {
    let transaction = db.transaction().map_err(|_| StorageError::Database)?;
    let row: Option<Vec<u8>> = transaction
        .query_row(
            "SELECT value FROM documents WHERE key=?1",
            [DOCUMENT],
            |r| read_limited_blob(r, 0),
        )
        .optional()
        .map_err(|_| StorageError::Database)?;
    if let Some(bytes) = row {
        let old = dto::read_snapshot(&codec::value(&bytes)?)?;
        let mut next = old
            .backup_snapshot(old.observed_at.unwrap_or(0))
            .map_err(err)?;
        next.revision = old.revision;
        transaction
            .execute(
                "UPDATE documents SET value=?1 WHERE key=?2",
                params![encode(&dto::snapshot(&next))?, DOCUMENT],
            )
            .map_err(|_| StorageError::Database)?;
    }
    transaction
        .execute(
            "DELETE FROM documents WHERE key>=?1 AND key<?2",
            params![RECEIPT_PREFIX, "business/voice-receipt/v10"],
        )
        .map_err(|_| StorageError::Database)?;
    transaction.commit().map_err(|_| StorageError::Database)
}
