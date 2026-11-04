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
    if let Some(error) = e.command_window_error() {
        return VoiceRepositoryError::CommandWindow(error);
    }
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
    SetLimits {
        limits: VoiceLimits,
        actor: ActorContext,
        expected: Revision,
    },
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
                | Self::SetLimits { .. }
        )
    }
}
fn policy_operation(
    intent: &fc_ports::command_window::VoicePolicyIntent,
    actor: &ActorContext,
) -> Operation {
    use fc_ports::command_window::VoicePolicyIntent;
    match intent {
        VoicePolicyIntent::Enabled {
            enabled,
            expected_voice,
            ..
        } => Operation::SetEnabled {
            enabled: *enabled,
            expected: *expected_voice,
            actor: actor.clone(),
        },
        VoicePolicyIntent::Limits {
            limits,
            expected_voice,
            ..
        } => Operation::SetLimits {
            limits: *limits,
            expected: *expected_voice,
            actor: actor.clone(),
        },
    }
}
impl<S: Send + 'static> BusinessRepository<S> {
    fn voice_operation(&self, op: Operation) -> Result<Value, VoiceRepositoryError> {
        let driver_guard =
            matches!(&op, Operation::Driver(_)).then(|| self.voice_driver_guard.clone());
        if !driver_allowed(driver_guard.as_ref()) {
            return Err(VoiceRepositoryError::Domain(VoiceError::StaleGeneration));
        }
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
            if !driver_allowed(driver_guard.as_ref()) {
                return Err(VoiceRepositoryError::Domain(VoiceError::StaleGeneration));
            }
            return Err(VoiceRepositoryError::Faulted);
        }
        use fc_ports::command_window::{TypedCommandIntent, VoicePolicyIntent};
        let typed = match &op {
            Operation::SetEnabled {
                enabled,
                expected,
                actor,
            } => Some((
                TypedCommandIntent::VoicePolicy(VoicePolicyIntent::Enabled {
                    enabled: *enabled,
                    expected_voice: *expected,
                    expected_identity: Revision::default(),
                    expected_runtime: 0,
                }),
                actor.clone(),
            )),
            Operation::SetLimits {
                limits,
                expected,
                actor,
            } => Some((
                TypedCommandIntent::VoicePolicy(VoicePolicyIntent::Limits {
                    limits: *limits,
                    expected_voice: *expected,
                    expected_identity: Revision::default(),
                    expected_runtime: 0,
                }),
                actor.clone(),
            )),
            _ => None,
        };
        let sql_policy = typed.is_some();
        let task = VoiceTask {
            op,
            clock: self.clock.clone(),
            session: self.session.clone(),
            factory: self.factory.clone(),
            active,
            driver_guard,
        };
        let ticket = if let Some((intent, actor)) = typed {
            self.handle
                .try_transaction(super::typed_commands::TypedTask {
                    inner: Some(Box::new(task)),
                    intent,
                    mode: crate::typed_command::Mode::Atomic,
                    original: None,
                    actor: Some(actor),
                    clock: self.clock.clone(),
                    session: self.session.clone(),
                    manual: None,
                })
        } else {
            self.handle.try_transaction(task)
        }
        .map_err(error)?;
        let bytes = match &self.voice_timeout {
            Some((timeout, _)) => ticket.wait_timeout(*timeout),
            None => ticket.wait(),
        }
        .map_err(error)?;
        if sql_policy {
            let (_, stored, _) = super::typed_commands::read_reply(&bytes).map_err(error)?;
            if stored.phase != 2 {
                return Err(VoiceRepositoryError::Faulted);
            }
            codec::value(&stored.value).map_err(error)
        } else {
            codec::value(&bytes).map_err(error)
        }
    }
    /// Durable acceptance precedes the first native stop. The review callback is
    /// evaluated only for Fresh; historical retries recheck current authority.
    pub fn accept_voice_policy(
        &self,
        intent: fc_ports::command_window::VoicePolicyIntent,
        actor: &ActorContext,
        review: SessionGuard,
    ) -> Result<crate::typed_command::TypedAdmission, StorageError> {
        use crate::{business::typed_commands::TypedTask, typed_command::Mode};
        let op = policy_operation(&intent, actor);
        let task = TypedTask {
            inner: Some(Box::new(VoiceTask {
                op,
                clock: self.clock.clone(),
                session: review,
                factory: self.factory.clone(),
                active: None,
                driver_guard: None,
            })),
            intent: fc_ports::command_window::TypedCommandIntent::VoicePolicy(intent.clone()),
            mode: Mode::Accept,
            original: None,
            actor: Some(actor.clone()),
            clock: self.clock.clone(),
            session: self.session.clone(),
            manual: None,
        };
        let bytes = self.wait_typed_voice(task)?;
        self.typed_admission(
            &bytes,
            fc_ports::command_window::TypedCommandIntent::VoicePolicy(intent),
        )
    }
    pub fn complete_voice_policy(
        &self,
        permit: crate::typed_command::FirstExecutionPermit,
        actor: &ActorContext,
    ) -> Result<fc_ports::command_window::TypedCommandOutcome, StorageError> {
        use crate::{
            business::typed_commands::{outcome, read_reply, TypedTask},
            typed_command::Mode,
        };
        let fc_ports::command_window::TypedCommandIntent::VoicePolicy(intent) = &permit.intent
        else {
            return Err(StorageError::InvalidCommand);
        };
        let op = policy_operation(intent, actor);
        let deadline = self
            .voice_timeout
            .as_ref()
            .and_then(|(t, _)| std::time::Instant::now().checked_add(*t));
        let active = self.voice_timeout.as_ref().map(|(_, current)| {
            let current = current.clone();
            Arc::new(move || deadline.is_some_and(|d| std::time::Instant::now() < d) && current())
                as Arc<dyn Fn() -> bool + Send + Sync>
        });
        let task = TypedTask {
            inner: Some(Box::new(VoiceTask {
                op,
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
                active,
                driver_guard: None,
            })),
            intent: permit.intent.clone(),
            mode: Mode::Complete,
            original: Some(permit.issued),
            actor: Some(actor.clone()),
            clock: self.clock.clone(),
            session: self.session.clone(),
            manual: None,
        };
        let bytes = self.wait_typed_voice(task)?;
        let (_, stored, _) = read_reply(&bytes)?;
        outcome(&permit.intent, &stored)
    }
    fn wait_typed_voice(
        &self,
        task: super::typed_commands::TypedTask<S>,
    ) -> Result<Vec<u8>, StorageError> {
        let ticket = self.handle.try_transaction(task)?;
        match &self.voice_timeout {
            Some((timeout, _)) => ticket.wait_timeout(*timeout),
            None => ticket.wait(),
        }
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
    /// Local global administrator policy mutation. The exact session guard is
    /// evaluated on the sole writer and immediately before COMMIT.
    pub fn set_voice_limits(
        &self,
        limits: VoiceLimits,
        expected: Revision,
        actor: &ActorContext,
    ) -> Result<VoiceSnapshot, VoiceRepositoryError> {
        dto::read_snapshot(&self.voice_operation(Operation::SetLimits {
            limits,
            expected,
            actor: actor.clone(),
        })?)
        .map_err(error)
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
    driver_guard: Option<VoiceDriverGuard>,
}
fn driver_allowed(guard: Option<&VoiceDriverGuard>) -> bool {
    guard.is_none_or(|check| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check())).unwrap_or(false)
    })
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
    prune_retaining_cancel(s, now, mutations, None)
}
fn prune_retaining_cancel(
    s: &mut VoiceSnapshot,
    now: i64,
    mutations: &mut Vec<Mutation>,
    protected_cancel: Option<VoiceJobId>,
) -> Result<(), StorageError> {
    let removed = match protected_cancel {
        Some(id) => s.maintain_after_cancel(now, id),
        None => s.maintain(now),
    }
    .map_err(err)?;
    for job in removed {
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
/// Policy changes retire uploads and request stop without fabricating a
/// physical Released receipt or rewriting historical repeat counts.
fn retire_accepted(s: &mut VoiceSnapshot, now: i64) -> Result<(), StorageError> {
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
    Ok(())
}
impl<S: Send + 'static> TransactionTask<S> for VoiceTask<S> {
    fn command_family(&self) -> Option<fc_ports::command_window::CommandFamily> {
        match self.op {
            Operation::SetEnabled { .. } | Operation::SetLimits { .. } => {
                Some(fc_ports::command_window::CommandFamily::Voice)
            }
            _ => None,
        }
    }

    fn lifetime(&self) -> TransactionLifetime {
        match self.op {
            Operation::Driver(VoiceDriverUpdate {
                action:
                    VoiceDriverAction::Released
                    | VoiceDriverAction::Fail(_)
                    | VoiceDriverAction::Interrupt
                    | VoiceDriverAction::ExpireBeforeWrite
                    | VoiceDriverAction::NaturalComplete,
                ..
            })
            | Operation::EmergencyStop
            | Operation::Recover
            | Operation::Maintain
            | Operation::Finalize { .. } => TransactionLifetime::OwnerReturn,
            _ => TransactionLifetime::Ordinary,
        }
    }

    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        match &self.op {
            Operation::Submit { request, .. } => Some(request.command_id),
            Operation::Cancel { request, .. } => Some(request.command_id),
            _ => None,
        }
        .map(|id| (fc_ports::command_window::CommandFamily::Voice, id))
    }

    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        if !driver_allowed(self.driver_guard.as_ref()) {
            return Err(err(VoiceError::StaleGeneration));
        }
        if self.active.as_ref().is_some_and(|check| !check()) {
            if !driver_allowed(self.driver_guard.as_ref()) {
                return Err(err(VoiceError::StaleGeneration));
            }
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
        let mut protected_cancel = None;
        let mut pre_actor: Option<(ActorContext, Option<ClassId>)> = None;
        let mut pre_management = false;
        let snapshot_outcome = matches!(
            self.op,
            Operation::Recover
                | Operation::Maintain
                | Operation::EmergencyStop
                | Operation::SetEnabled { .. }
                | Operation::SetLimits { .. }
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
                // The newly emitted receipt must not be Put and Delete in
                // this same transaction when the audit count crosses 100.
                protected_cancel = Some(result.id);
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
            Operation::SetLimits {
                limits,
                actor,
                expected,
            } => {
                (self.session)(&identity, &actor)?;
                identity
                    .authorize_management(&actor, self.clock.now())
                    .map_err(StorageError::Identity)?;
                limits.validate().map_err(err)?;
                if s.revision != expected {
                    return Err(err(VoiceError::RevisionConflict));
                }
                // Even a repeated value is an explicit reviewed stop boundary.
                // Preserve the persisted enable switch and old release token.
                retire_accepted(&mut s, now)?;
                s.limits = limits;
                s.revision = s
                    .revision
                    .next()
                    .map_err(|_| StorageError::InvalidCommand)?;
                pre_actor = Some((actor, None));
                pre_management = true;
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
                    retire_accepted(&mut s, now)?;
                }
                s.revision = s
                    .revision
                    .next()
                    .map_err(|_| StorageError::InvalidCommand)?;
                pre_actor = Some((actor, None));
                dto::snapshot(&s)
            }
        };
        prune_retaining_cancel(&mut s, now, &mut mutations, protected_cancel)?;
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
                if pre_management {
                    identity
                        .authorize_management(&actor, clock.now())
                        .map_err(StorageError::Identity)?;
                }
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
        let precommit = match (self.active, self.driver_guard) {
            (None, None) => precommit,
            (active, driver_guard) => Some(Box::new(move || {
                if !driver_allowed(driver_guard.as_ref()) {
                    return Err(err(VoiceError::StaleGeneration));
                }
                if active.as_ref().is_some_and(|check| !check()) {
                    if !driver_allowed(driver_guard.as_ref()) {
                        return Err(err(VoiceError::StaleGeneration));
                    }
                    return Err(StorageError::Indeterminate);
                }
                precommit.map_or(Ok(()), |check| check())?;
                if !driver_allowed(driver_guard.as_ref()) {
                    return Err(err(VoiceError::StaleGeneration));
                }
                Ok(())
            })
                as Box<dyn FnOnce() -> Result<(), StorageError> + Send>),
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
    transaction
        .execute(
            "DELETE FROM documents WHERE key>=?1 AND key<?2",
            params![COMMAND_BINDING_PREFIX, "business/voice-job-command/v3/"],
        )
        .map_err(|_| StorageError::Database)?;
    transaction.commit().map_err(|_| StorageError::Database)
}

/// Domain terminal voice states require the original Released transition.
/// Expiring a client command is deliberately absent from this decision.
pub(crate) fn command_window_holds(bytes: &[u8]) -> Result<Vec<(CommandId, bool)>, StorageError> {
    let snapshot = dto::read_snapshot(&codec::value(bytes)?)?;
    let mut holds = Vec::with_capacity(snapshot.jobs.len() * 2);
    for job in snapshot.jobs {
        holds.push((job.command_id, !job.state.terminal()));
        if let Some(cancel) = job.cancel_command {
            holds.push((cancel, !job.state.terminal()));
        }
    }
    Ok(holds)
}

pub(crate) const COMMAND_BINDING_PREFIX: &str = "business/voice-job-command/v2/";
pub(crate) fn command_binding_key(id: VoiceJobId) -> String {
    format!("{COMMAND_BINDING_PREFIX}{:020}", id.get())
}
pub(crate) fn read_command_binding(
    bytes: &[u8],
) -> Result<fc_ports::command_window::CommandTicket, StorageError> {
    let value = codec::value(bytes)?;
    codec::object_fields(&value, &["format", "ticket"])?;
    if codec::number(&value, "format")? != 2 {
        return Err(StorageError::Database);
    }
    fc_ports::command_window::CommandTicket::parse(
        value["ticket"].as_str().ok_or(StorageError::Database)?,
    )
    .map_err(|_| StorageError::Database)
}
/// Only the dedicated VoiceTask's exact prune deletes authorize retirement.
/// A job still present in the new snapshot, including a held original owner,
/// is never selected. The actual terminal transition can be in this same TX.
pub(crate) fn command_window_pruned_jobs(
    old: &[u8],
    new: &[u8],
    mutations: &[Mutation],
) -> Result<Vec<VoiceJob>, StorageError> {
    let old = dto::read_snapshot(&codec::value(old)?)?;
    let new = dto::read_snapshot(&codec::value(new)?)?;
    let mut pruned = Vec::new();
    for job in old.jobs {
        if new.jobs.iter().any(|retained| retained.id == job.id) {
            continue;
        }
        for id in std::iter::once(job.command_id).chain(job.cancel_command) {
            let key = receipt_key(id);
            if !mutations
                .iter()
                .any(|m| matches!(m, Mutation::Delete { key: deleted } if deleted == &key))
            {
                return Err(StorageError::Database);
            }
        }
        pruned.push(job);
    }
    Ok(pruned)
}
/// Validate the indexed original receipt instead of trusting a numeric alias.
pub(crate) fn command_window_receipt_matches(
    bytes: &[u8],
    job: &VoiceJob,
    command: CommandId,
) -> Result<(), StorageError> {
    let v = codec::value(bytes)?;
    codec::object_fields(&v, &["fingerprint", "job", "owner", "digest"])?;
    let fingerprint: Vec<u8> =
        serde_json::from_value(v["fingerprint"].clone()).map_err(|_| StorageError::Database)?;
    let digest: Vec<u8> =
        serde_json::from_value(v["digest"].clone()).map_err(|_| StorageError::Database)?;
    let id = codec::number(&v, "job")?;
    let owner = codec::number(&v, "owner")?;
    let bound = codec::bytes(&json!({"fingerprint":fingerprint,"job":id,"owner":owner}))?;
    if fingerprint.len() != 32
        || digest.as_slice() != Sha256::digest(bound).as_slice()
        || id != job.id.get()
        || owner != job.owner.get()
        || (command != job.command_id && job.cancel_command != Some(command))
    {
        return Err(StorageError::Database);
    }
    Ok(())
}
pub(crate) fn command_window_jobs(bytes: &[u8]) -> Result<Vec<VoiceJob>, StorageError> {
    Ok(dto::read_snapshot(&codec::value(bytes)?)?.jobs)
}

impl<S: Send + 'static> BusinessRepository<S> {
    pub fn check_command_window(
        &self,
        issued: fc_ports::command_window::IssuedCommand,
    ) -> Result<(), StorageError> {
        let ticket = self.handle.try_check_command(issued)?;
        match &self.voice_timeout {
            Some((timeout, _)) => ticket.wait_timeout(*timeout),
            None => ticket.wait(),
        }
    }
    pub fn voice_job_command_binding(
        &self,
        id: VoiceJobId,
        actor: &ActorContext,
    ) -> Result<Option<fc_ports::command_window::CommandTicket>, VoiceRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(CommandRead {
                kind: CommandReadKind::Binding(id),
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        let value = codec::value(&bytes).map_err(error)?;
        if value.is_null() {
            Ok(None)
        } else {
            fc_ports::command_window::CommandTicket::parse(
                value
                    .as_str()
                    .ok_or_else(|| error(StorageError::Database))?,
            )
            .map(Some)
            .map_err(|_| error(StorageError::Database))
        }
    }
    pub fn voice_command_outcome(
        &self,
        id: CommandId,
        actor: &ActorContext,
    ) -> Result<Option<VoiceJob>, VoiceRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(CommandRead {
                kind: CommandReadKind::Outcome(id),
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        let value = codec::value(&bytes).map_err(error)?;
        if value.is_null() {
            Ok(None)
        } else {
            dto::read_job(&value)
                .map(public_job)
                .map(Some)
                .map_err(error)
        }
    }
}
#[derive(Clone, Copy)]
enum CommandReadKind {
    Binding(VoiceJobId),
    Outcome(CommandId),
}
struct CommandRead {
    kind: CommandReadKind,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for CommandRead {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        match self.kind {
            CommandReadKind::Outcome(id) => {
                Some((fc_ports::command_window::CommandFamily::Voice, id))
            }
            _ => None,
        }
    }

    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        let snapshot = load(d)?;
        (self.session)(&identity, &self.actor)?;
        let (outcome, job) = match self.kind {
            CommandReadKind::Binding(id) => {
                let job = snapshot
                    .jobs
                    .iter()
                    .find(|job| job.id == id)
                    .ok_or(err(VoiceError::Missing))?;
                query_authorize(
                    &identity,
                    &self.actor,
                    job,
                    self.clock.as_ref(),
                    &self.session,
                )?;
                let binding = d
                    .get(&command_binding_key(id))?
                    .map(|row| read_command_binding(&row.value))
                    .transpose()?;
                (
                    binding.map_or(Value::Null, |ticket| json!(ticket.to_string())),
                    Some(job.clone()),
                )
            }
            CommandReadKind::Outcome(id) => {
                let Some(row) = d.get(&receipt_key(id))? else {
                    return Ok(TransactionDecision {
                        precommit: None,
                        outcome: encode(&Value::Null)?,
                        commit: None,
                    });
                };
                let value = codec::value(&row.value)?;
                codec::object_fields(&value, &["fingerprint", "job", "owner", "digest"])?;
                let fingerprint: Vec<u8> = serde_json::from_value(value["fingerprint"].clone())
                    .map_err(|_| StorageError::Database)?;
                let digest: Vec<u8> = serde_json::from_value(value["digest"].clone())
                    .map_err(|_| StorageError::Database)?;
                let job_id = codec::number(&value, "job")?;
                let owner = codec::number(&value, "owner")?;
                let bound =
                    codec::bytes(&json!({"fingerprint":fingerprint,"job":job_id,"owner":owner}))?;
                if fingerprint.len() != 32
                    || digest.len() != 32
                    || digest.as_slice() != Sha256::digest(bound).as_slice()
                {
                    return Err(StorageError::Database);
                }
                let job = snapshot
                    .jobs
                    .iter()
                    .find(|job| job.id.get() == job_id)
                    .ok_or(err(VoiceError::Missing))?;
                if job.owner.get() != owner
                    || job.command_id != id && job.cancel_command != Some(id)
                {
                    return Err(StorageError::Database);
                }
                query_authorize(
                    &identity,
                    &self.actor,
                    job,
                    self.clock.as_ref(),
                    &self.session,
                )?;
                (dto::job(job), Some(job.clone()))
            }
        };
        let precommit = job.map(|job| {
            Box::new(move || {
                query_authorize(
                    &identity,
                    &self.actor,
                    &job,
                    self.clock.as_ref(),
                    &self.session,
                )
            }) as Box<dyn FnOnce() -> Result<(), StorageError> + Send>
        });
        Ok(TransactionDecision {
            precommit,
            outcome: encode(&outcome)?,
            commit: None,
        })
    }
}

#[cfg(test)]
mod c2_prune_tests {
    use super::*;
    use crate::command_window::{IssueCommand, Window, WindowClock};
    use fc_ports::command_window::{CommandFamily, CommandOwner, CommandTicket, IssuedCommand};
    use rusqlite::Connection;

    struct Fixed;
    impl WindowClock for Fixed {
        fn wall_millis(&self) -> Result<u64, StorageError> {
            Ok(1000)
        }
        fn monotonic_millis(&self) -> u64 {
            0
        }
    }
    fn issue(
        db: &mut Connection,
        window: &mut Window,
        id: u64,
        actor: &ActorContext,
    ) -> IssuedCommand {
        let mut entropy = [7; 24];
        entropy[16..].copy_from_slice(&id.to_be_bytes());
        window
            .issue(
                db,
                IssueCommand {
                    family: CommandFamily::Voice,
                    owner: CommandOwner::authenticated(actor),
                    entropy,
                },
            )
            .unwrap()
    }
    fn append_job(
        snapshot: &mut VoiceSnapshot,
        actor: &ActorContext,
        asset: u64,
        command: CommandId,
        now: i64,
    ) -> VoiceJob {
        let reserved = snapshot
            .reserve(
                VoiceAssetId::new(asset).unwrap(),
                actor,
                ClassId::new(1).unwrap(),
                now,
            )
            .unwrap();
        let asset = snapshot
            .publish(
                reserved.id,
                reserved.revision,
                VoicePcmMetadata {
                    byte_length: 16044,
                    frame_count: 8000,
                    sha256: [7; 32],
                },
                now,
            )
            .unwrap();
        snapshot
            .submit(command, asset.id, asset.revision, 1, actor, now)
            .unwrap()
    }
    fn apply(db: &Connection, changes: &[Mutation]) {
        for change in changes {
            match change {
                Mutation::Put { key, value } => {
                    db.execute("INSERT INTO documents VALUES(?1,?2,1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value]).unwrap();
                }
                Mutation::Delete { key } => {
                    db.execute("DELETE FROM documents WHERE key=?1", [key])
                        .unwrap();
                }
            }
        }
    }
    fn key(issued: IssuedCommand) -> String {
        let (slot, _) =
            crate::business::command_window::legacy(&receipt_key(issued.command_id)).unwrap();
        crate::business::command_window::namespaced(slot, issued.ticket)
    }
    struct Fixture {
        db: Connection,
        window: Window,
        submit: IssuedCommand,
        cancel: IssuedCommand,
        held: IssuedCommand,
        changes: Vec<Mutation>,
    }
    fn fixture() -> Fixture {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE documents(key TEXT PRIMARY KEY,value BLOB NOT NULL,revision INTEGER NOT NULL);CREATE TABLE command_journal(command_id TEXT PRIMARY KEY,fingerprint BLOB,revision INTEGER);").unwrap();
        let mut window = Window::open(&mut db, Arc::new(Fixed)).unwrap();
        let day = fc_domain::time::LocalDate::new(2026, 10, 30).unwrap();
        let instant = fc_domain::time::LocalDateTime::new(day, 3600).unwrap();
        let now = civil_seconds(instant);
        let actor = ActorContext {
            principal_id: PrincipalId::new(1).unwrap(),
            school_id: fc_domain::ids::SchoolId::new(1).unwrap(),
            auth_epoch: fc_domain::ids::AuthEpoch::new(1),
            expires_at: fc_domain::time::LocalDateTime::new(
                fc_domain::time::LocalDate::new(2026, 11, 2).unwrap(),
                3600,
            )
            .unwrap(),
            origin: TrustedOrigin::Web,
        };
        // The retained original owner has an earlier serial than the pruned
        // commands: advancing a global watermark would incorrectly retire it.
        let held = issue(&mut db, &mut window, 22, &actor);
        let submit = issue(&mut db, &mut window, 11, &actor);
        let cancel = issue(&mut db, &mut window, 33, &actor);
        let mut snapshot = VoiceSnapshot::default();
        let first = append_job(&mut snapshot, &actor, 1, submit.command_id, now);
        let terminal = snapshot
            .cancel(first.id, first.revision, cancel.command_id, now)
            .unwrap();
        let queued = append_job(&mut snapshot, &actor, 2, held.command_id, now);
        let starting = snapshot
            .drive(
                &VoiceDriverUpdate {
                    job_id: queued.id,
                    expected_revision: queued.revision,
                    generation: queued.generation,
                    action: VoiceDriverAction::Start,
                },
                now,
            )
            .unwrap();
        let playing = snapshot
            .drive(
                &VoiceDriverUpdate {
                    job_id: starting.id,
                    expected_revision: starting.revision,
                    generation: starting.generation,
                    action: VoiceDriverAction::FirstWrite,
                },
                now,
            )
            .unwrap();
        snapshot.validate().unwrap();
        for (issued, job) in [(submit, &terminal), (cancel, &terminal), (held, &playing)] {
            crate::typed_command::raw_claim(&db, issued).unwrap();
            let mut changes = vec![receipt(issued.command_id, vec![7; 32], job).unwrap()];
            crate::command_window::register_documents(&db, issued, &mut changes).unwrap();
            apply(&db, &changes);
        }
        let old = vec![mutation(&snapshot).unwrap()];
        apply(&db, &old);
        crate::command_window::sync_owner_holds(&db, &old, Some(submit), 1).unwrap();
        crate::command_window::sync_owner_holds(&db, &old, Some(held), 1).unwrap();
        let mut changes = Vec::new();
        prune(&mut snapshot, now + AUDIT_TTL_SECONDS, &mut changes).unwrap();
        assert_eq!(snapshot.jobs.len(), 1);
        assert_eq!(snapshot.jobs[0].id, playing.id);
        assert_eq!(snapshot.jobs[0].state, VoiceState::Playing);
        changes.push(mutation(&snapshot).unwrap());
        Fixture {
            db,
            window,
            submit,
            cancel,
            held,
            changes,
        }
    }
    fn charge(db: &Connection, issued: IssuedCommand) -> u64 {
        db.query_row(
            "SELECT charged_bytes FROM command_window_entries WHERE serial=?1",
            [issued.ticket.serial()],
            |r| r.get(0),
        )
        .unwrap()
    }
    #[test]
    fn c2_voice_prune_is_atomic_replay_inert_exactly_charged_and_preserves_held_owner() {
        let mut f = fixture();
        let before = charge(&f.db, f.submit) + charge(&f.db, f.cancel);
        let held_charge = charge(&f.db, f.held);
        let removed_bytes: u64 = [f.submit, f.cancel]
            .into_iter()
            .map(|i| {
                f.db.query_row(
                    "SELECT length(value) FROM documents WHERE key=?1",
                    [key(i)],
                    |r| r.get::<_, u64>(0),
                )
                .unwrap()
                    + key(i).len() as u64
                    + 32
            })
            .sum();
        {
            let tx = f.db.transaction().unwrap();
            crate::command_window::sync_owner_holds(&tx, &f.changes, None, 2).unwrap();
            assert_eq!(
                f.window.check(&tx, f.submit),
                Err(StorageError::CommandExpired)
            );
            tx.rollback().unwrap();
        }
        assert_eq!(f.window.check(&f.db, f.submit), Ok(()));
        assert_eq!(charge(&f.db, f.submit) + charge(&f.db, f.cancel), before);
        {
            let tx = f.db.transaction().unwrap();
            crate::command_window::sync_owner_holds(&tx, &f.changes, None, 2).unwrap();
            apply(&tx, &f.changes);
            tx.commit().unwrap();
        }
        for issued in [f.submit, f.cancel] {
            assert_eq!(
                f.window.check(&f.db, issued),
                Err(StorageError::CommandExpired)
            );
            assert_eq!(
                crate::typed_command::raw_check(&f.db, issued),
                Err(StorageError::CommandExpired)
            );
            assert!(!f
                .db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM documents WHERE key=?1)",
                    [key(issued)],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap());
            assert_eq!(
                f.db.query_row(
                    "SELECT COUNT(*) FROM command_window_documents WHERE serial=?1",
                    [issued.ticket.serial()],
                    |r| r.get::<_, u64>(0)
                )
                .unwrap(),
                0
            );
            assert_eq!(charge(&f.db, issued), 256);
        }
        assert_eq!(
            before - charge(&f.db, f.submit) - charge(&f.db, f.cancel),
            removed_bytes
        );
        assert_eq!(f.window.check(&f.db, f.held), Ok(()));
        assert_eq!(charge(&f.db, f.held), held_charge);
        assert!(f
            .db
            .query_row(
                "SELECT held FROM command_window_entries WHERE serial=?1",
                [f.held.ticket.serial()],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
        assert_eq!(
            f.db.query_row("SELECT COUNT(*) FROM command_window_entries", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            3
        );
        // Re-enter the actual startup validation; raw tombstones remain denied.
        let reopened = Window::open(&mut f.db, Arc::new(Fixed)).unwrap();
        assert_eq!(
            reopened.check(&f.db, f.submit),
            Err(StorageError::CommandExpired)
        );
        assert_eq!(reopened.check(&f.db, f.held), Ok(()));
        let action = crate::typed_command::WindowAction::new(
            fc_ports::command_window::TypedCommandIntent::CalendarManualFetch { year: 2026 },
            crate::typed_command::Mode::Accept,
        );
        assert!(matches!(
            crate::typed_command::read(&f.db, f.submit, &action),
            Err(StorageError::CommandIdReused)
        ));
        // No payload is needed in a raw tombstone; all other phases fail closed.
        f.db.execute(
            "UPDATE command_window_consumption SET phase=1 WHERE serial=?1",
            [f.cancel.ticket.serial()],
        )
        .unwrap();
        assert_eq!(
            crate::typed_command::open(&f.db),
            Err(StorageError::Database)
        );
    }
    #[test]
    fn c2_voice_prune_rejects_foreign_alias_and_missing_original_prune_markers() {
        let mut f = fixture();
        let without_markers = f
            .changes
            .iter()
            .filter(|m| !matches!(m, Mutation::Delete { .. }))
            .cloned()
            .collect::<Vec<_>>();
        {
            let tx = f.db.transaction().unwrap();
            assert_eq!(
                crate::command_window::sync_owner_holds(&tx, &without_markers, None, 2),
                Err(StorageError::Database)
            );
            tx.rollback().unwrap();
        }
        let original = key(f.cancel);
        let (slot, _) = crate::business::command_window::owned(&original).unwrap();
        let foreign = crate::business::command_window::namespaced(
            slot,
            CommandTicket::new([8; 16], f.cancel.ticket.serial()).unwrap(),
        );
        f.db.execute(
            "UPDATE documents SET key=?1 WHERE key=?2",
            params![foreign, original],
        )
        .unwrap();
        f.db.execute(
            "UPDATE command_window_documents SET key=?1 WHERE key=?2",
            params![foreign, original],
        )
        .unwrap();
        let before = charge(&f.db, f.cancel);
        {
            let tx = f.db.transaction().unwrap();
            assert_eq!(
                crate::command_window::sync_owner_holds(&tx, &f.changes, None, 2),
                Err(StorageError::Database)
            );
            tx.rollback().unwrap();
        }
        assert_eq!(charge(&f.db, f.cancel), before);
        assert_eq!(f.window.check(&f.db, f.submit), Ok(()));
        assert_eq!(f.window.check(&f.db, f.held), Ok(()));
    }
}
