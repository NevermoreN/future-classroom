//! Separate maintenance record on the existing SQLite writer. These commits
//! neither increment state_meta nor enter the teaching authority publisher.
use super::*;
use fc_domain::{
    time::Minute,
    update_state::*,
    updates::{MaintenanceWindow, ReleaseIdentity, ReleaseSequence},
};
use fc_ports::updates::*;

pub(crate) struct Task {
    actor: ActorContext,
    request: Option<UpdateChangeRequest>,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    fence: Option<TransactionExecutionFence>,
    issued: Option<fc_ports::command_window::IssuedCommand>,
}
impl Task {
    pub(crate) fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self.request.as_ref().map(|r| &r.mutation) {
                Some(UpdateMutation::SetPolicy(policy)) => policy.channel.capacity(),
                _ => 0,
            }
    }
    fn authorize(&self, identity: &IdentityState) -> Result<(), StorageError> {
        if let Some(fence) = &self.fence {
            check_transaction_execution(fence)?;
        }
        (self.session)(identity, &self.actor)?;
        identity
            .authorize_management(&self.actor, self.clock.now())
            .map_err(StorageError::Identity)
    }
}
fn error(error: StorageError) -> UpdateRepositoryError {
    match error {
        StorageError::Identity(_) => UpdateRepositoryError::Denied,
        StorageError::Conflict { .. } => UpdateRepositoryError::Stale,
        StorageError::CommandIdReused => UpdateRepositoryError::CommandIdReused,
        StorageError::InvalidCommand
        | StorageError::CommandNotIssued
        | StorageError::CommandExpired
        | StorageError::CommandWindowClock => UpdateRepositoryError::Invalid,
        StorageError::ResourceQuota | StorageError::DocumentTooLarge => {
            UpdateRepositoryError::Capacity
        }
        StorageError::Busy => UpdateRepositoryError::Busy,
        StorageError::ExecutionCancelled | StorageError::Stopping => {
            UpdateRepositoryError::Cancelled
        }
        StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => UpdateRepositoryError::Faulted,
        _ => UpdateRepositoryError::Unavailable,
    }
}
impl<S: Send + 'static> UpdateMaintenanceRepository for BusinessRepository<S> {
    fn update_runtime_state(&self) -> Result<UpdateMaintenanceState, UpdateRepositoryError> {
        self.handle
            .send(|reply| Message::UpdateRuntime(RuntimeTask::Read, reply))
            .map_err(error)?
            .wait()
            .map_err(error)
    }
    fn accept_update_release(
        &mut self,
        expected_policy_revision: Revision,
        release: ReleaseIdentity,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError> {
        self.handle
            .send(|reply| {
                Message::UpdateRuntime(
                    RuntimeTask::Accept {
                        expected_policy_revision,
                        release,
                    },
                    reply,
                )
            })
            .map_err(error)?
            .wait()
            .map_err(error)
    }
    fn begin_background_update(
        &mut self,
        expected_revision: Revision,
        expected_policy_revision: Revision,
        release: ReleaseIdentity,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError> {
        self.handle
            .send(|reply| {
                Message::UpdateRuntime(
                    RuntimeTask::Begin {
                        expected_revision,
                        expected_policy_revision,
                        release,
                    },
                    reply,
                )
            })
            .map_err(error)?
            .wait()
            .map_err(error)
    }
    fn advance_background_update(
        &mut self,
        job: &UpdateJob,
        phase: UpdateJobPhase,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError> {
        self.handle
            .send(|reply| {
                Message::UpdateRuntime(
                    RuntimeTask::Advance {
                        job: job.clone(),
                        phase,
                    },
                    reply,
                )
            })
            .map_err(error)?
            .wait()
            .map_err(error)
    }
    fn update_status(
        &self,
        actor: &ActorContext,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError> {
        let bytes = self
            .handle
            .send(|reply| {
                Message::UpdateMaintenance(
                    Task {
                        actor: actor.clone(),
                        request: None,
                        clock: self.clock.clone(),
                        session: self.session.clone(),
                        fence: self.handle.transaction_execution.clone(),
                        issued: self.handle.command_window,
                    },
                    reply,
                )
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        decode(&bytes).map_err(error)
    }
    fn change_update(
        &mut self,
        request: UpdateChangeRequest,
        actor: &ActorContext,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError> {
        // Admission bounds before queueing prevent an oversized channel retaining
        // arbitrary client memory on the sole writer queue.
        if matches!(&request.mutation, UpdateMutation::SetPolicy(policy) if !policy.validate()) {
            return Err(UpdateRepositoryError::Invalid);
        }
        let bytes = self
            .handle
            .send(|reply| {
                Message::UpdateMaintenance(
                    Task {
                        actor: actor.clone(),
                        request: Some(request),
                        clock: self.clock.clone(),
                        session: self.session.clone(),
                        fence: self.handle.transaction_execution.clone(),
                        issued: self.handle.command_window,
                    },
                    reply,
                )
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        decode(&bytes).map_err(error)
    }
}

impl<S: Send + 'static> BusinessRepository<S> {
    /// Trusted reset confirmation only. The original writer serializes this
    /// negative gate with every update Handoff; no caller may reopen it.
    pub fn close_update_recovery_admission(&self) -> Result<(), StorageError> {
        self.handle.send(Message::UpdateRecoveryGate)?.wait()
    }
}
pub(crate) fn ensure_recovery_allowed(db: &Connection) -> Result<(), StorageError> {
    if load(db)?.jobs.iter().any(|job| {
        matches!(
            job.phase,
            UpdateJobPhase::Handoff | UpdateJobPhase::Installing | UpdateJobPhase::NeedsReconcile
        )
    }) {
        return Err(StorageError::Busy);
    }
    Ok(())
}
pub(crate) fn close_recovery_admission(
    db: &mut Connection,
    stop: &AtomicBool,
) -> Result<(), StorageError> {
    if stop.load(Ordering::Acquire) {
        return Err(StorageError::Stopping);
    }
    let transaction = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| StorageError::Database)?;
    ensure_recovery_allowed(&transaction)?;
    // No school/update document is changed. The sole writer closes its own
    // admission bit before it dequeues another positive update request.
    transaction.rollback().map_err(|_| StorageError::Database)
}

pub(crate) fn initialize(db: &Connection) -> Result<(), StorageError> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS update_maintenance (singleton INTEGER PRIMARY KEY CHECK(singleton=1), value BLOB NOT NULL CHECK(length(value)<=65536));")
        .map_err(|_| StorageError::Database)?;
    load(db).map(|_| ())
}
fn load(db: &Connection) -> Result<UpdateMaintenanceState, StorageError> {
    let bytes = db
        .query_row(
            "SELECT value FROM update_maintenance WHERE singleton=1",
            [],
            |row| read_limited_blob(row, 0),
        )
        .optional()
        .map_err(|_| StorageError::Database)?;
    bytes.map_or(Ok(UpdateMaintenanceState::default()), |bytes| {
        decode(&bytes)
    })
}

fn accept_release(
    state: &mut UpdateMaintenanceState,
    release: ReleaseIdentity,
) -> Result<(), StorageError> {
    if release.payload_sha256 == [0; 32]
        || state.accepted_release.is_some_and(|accepted| {
            release.sequence < accepted.sequence
                || (release.sequence == accepted.sequence
                    && release.payload_sha256 != accepted.payload_sha256)
        })
    {
        return Err(StorageError::InvalidCommand);
    }
    state.accepted_release = Some(release);
    Ok(())
}
fn record_installed(
    state: &mut UpdateMaintenanceState,
    release: ReleaseIdentity,
) -> Result<(), StorageError> {
    if state.installed_release.is_some_and(|installed| {
        release.sequence < installed.sequence
            || (release.sequence == installed.sequence
                && release.payload_sha256 != installed.payload_sha256)
    }) {
        return Err(StorageError::InvalidCommand);
    }
    state.installed_release = Some(release);
    Ok(())
}

/// Closed, process-internal messages. No HTTP DTO deserializes this enum.
pub(crate) enum RuntimeTask {
    Read,
    Accept {
        expected_policy_revision: Revision,
        release: ReleaseIdentity,
    },
    Begin {
        expected_revision: Revision,
        expected_policy_revision: Revision,
        release: ReleaseIdentity,
    },
    Advance {
        job: UpdateJob,
        phase: UpdateJobPhase,
    },
}
pub(crate) fn execute_runtime(
    db: &mut Connection,
    task: RuntimeTask,
    recovery_closed: bool,
    stop: &AtomicBool,
) -> Result<UpdateMaintenanceState, StorageError> {
    if stop.load(Ordering::Acquire) {
        return Err(StorageError::Stopping);
    }
    let transaction = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| StorageError::Database)?;
    let mut state = load(&transaction)?;
    if recovery_closed
        && !matches!(
            &task,
            RuntimeTask::Read
                | RuntimeTask::Advance {
                    phase: UpdateJobPhase::Succeeded
                        | UpdateJobPhase::Failed
                        | UpdateJobPhase::Cancelled
                        | UpdateJobPhase::NeedsReconcile,
                    ..
                }
        )
    {
        return Err(StorageError::Busy);
    }
    match task {
        RuntimeTask::Read => {
            transaction.rollback().map_err(|_| StorageError::Database)?;
            return Ok(state);
        }
        RuntimeTask::Accept {
            expected_policy_revision,
            release,
        } => {
            if state.policy.revision != expected_policy_revision {
                return Err(StorageError::Conflict {
                    current: state.revision,
                });
            }
            if !state.policy.participation_enabled() {
                return Err(StorageError::InvalidCommand);
            }
            if state.accepted_release == Some(release) {
                transaction.rollback().map_err(|_| StorageError::Database)?;
                return Ok(state);
            }
            accept_release(&mut state, release)?;
        }
        RuntimeTask::Begin {
            expected_revision,
            expected_policy_revision,
            release,
        } => {
            if state.revision != expected_revision
                || state.policy.revision != expected_policy_revision
            {
                return Err(StorageError::Conflict {
                    current: state.revision,
                });
            }
            if !state.policy.participation_enabled() || release.payload_sha256 == [0; 32] {
                return Err(StorageError::InvalidCommand);
            }
            if state.jobs.iter().any(|job| !job.phase.settled()) {
                return Err(StorageError::Busy);
            }
            if state
                .installed_release
                .is_some_and(|installed| release.sequence <= installed.sequence)
            {
                return Err(StorageError::InvalidCommand);
            }
            accept_release(&mut state, release)?;
            if !state.reserve_job() {
                return Err(StorageError::ResourceQuota);
            }
            state.sequence = state
                .sequence
                .checked_add(1)
                .filter(|s| *s < (1u64 << 63))
                .ok_or(StorageError::ResourceQuota)?;
            let command_id = CommandId::new((1u64 << 63) | state.sequence)
                .map_err(|_| StorageError::ResourceQuota)?;
            if state.jobs.iter().any(|j| j.command_id == command_id)
                || state.receipts.iter().any(|r| r.command_id == command_id)
            {
                return Err(StorageError::CommandIdReused);
            }
            state.jobs.push(UpdateJob {
                id: state.sequence,
                command_id,
                owner: state.policy_owner,
                policy_revision: state.policy.revision,
                release,
                phase: UpdateJobPhase::Prepared,
            });
        }
        RuntimeTask::Advance { job, phase } => {
            let current = state
                .jobs
                .iter_mut()
                .find(|current| current.id == job.id)
                .ok_or(StorageError::InvalidCommand)?;
            let mut completed = job.clone();
            completed.phase = phase;
            if current == &completed {
                transaction.rollback().map_err(|_| StorageError::Database)?;
                return Ok(state);
            }
            if current != &job || !current.phase.can_advance_to(phase) {
                return Err(StorageError::InvalidCommand);
            }
            if job.policy_revision != state.policy.revision
                && !matches!(
                    phase,
                    UpdateJobPhase::Succeeded
                        | UpdateJobPhase::Failed
                        | UpdateJobPhase::Cancelled
                        | UpdateJobPhase::NeedsReconcile
                )
            {
                return Err(StorageError::InvalidCommand);
            }
            current.phase = phase;
            if phase == UpdateJobPhase::Succeeded {
                record_installed(&mut state, job.release)?;
            }
        }
    }
    state.revision = state
        .revision
        .next()
        .map_err(|_| StorageError::ResourceQuota)?;
    let bytes = encode(&state)?;
    transaction.execute("INSERT INTO update_maintenance(singleton,value) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET value=excluded.value", [&bytes])
        .map_err(|_|StorageError::Database)?;
    if stop.load(Ordering::Acquire) {
        return Err(StorageError::Stopping);
    }
    transaction
        .commit()
        .map_err(|_| StorageError::Indeterminate)?;
    Ok(state)
}
pub(crate) fn execute(
    db: &mut Connection,
    task: Task,
    window: &crate::command_window::Window,
    recovery_closed: bool,
    stop: &AtomicBool,
) -> Result<Vec<u8>, StorageError> {
    if stop.load(Ordering::Acquire) {
        return Err(StorageError::Stopping);
    }
    let transaction = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| StorageError::Database)?;
    let identity = load_identity(&TransactionView(&transaction))?;
    task.authorize(&identity)?;
    let mut state = load(&transaction)?;
    let Some(request) = &task.request else {
        let bytes = encode(&state)?;
        task.authorize(&identity)?;
        transaction.rollback().map_err(|_| StorageError::Database)?;
        return Ok(bytes);
    };
    let issued = task.issued.ok_or(StorageError::CommandNotIssued)?;
    if issued.family != fc_ports::command_window::CommandFamily::Updates
        || issued.command_id != request.command_id
        || issued.owner != fc_ports::command_window::CommandOwner::authenticated(&task.actor)
    {
        return Err(StorageError::CommandNotIssued);
    }
    window.check(&transaction, issued)?;
    crate::typed_command::raw_check(&transaction, issued)?;
    if recovery_closed {
        return Err(StorageError::Busy);
    }
    let mutation = mutation_value(&request.mutation);
    let fingerprint: [u8; 32] = Sha256::digest(codec::bytes(&mutation)?).into();
    if let Some(receipt) = state
        .receipts
        .iter()
        .find(|receipt| receipt.command_id == request.command_id)
    {
        if receipt.expected_revision != request.expected_revision
            || receipt.principal_id != task.actor.principal_id
            || receipt.auth_epoch != task.actor.auth_epoch
            || receipt.fingerprint != fingerprint
        {
            return Err(StorageError::CommandIdReused);
        }
        task.authorize(&identity)?;
        transaction.rollback().map_err(|_| StorageError::Database)?;
        return encode(&state);
    }
    let consumed: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM command_window_consumption WHERE serial=?1)",
            [issued.ticket.serial()],
            |r| r.get(0),
        )
        .map_err(|_| StorageError::Database)?;
    if consumed {
        return Err(StorageError::CommandIdReused);
    }
    // Job identity outlives the rotating command log. Never let an old prepare
    // command become a new policy/phase mutation once its receipt is evicted.
    if state
        .jobs
        .iter()
        .any(|job| job.command_id == request.command_id)
    {
        return Err(StorageError::CommandIdReused);
    }
    if state.revision != request.expected_revision {
        return Err(StorageError::Conflict {
            current: state.revision,
        });
    }
    let next = state
        .revision
        .next()
        .map_err(|_| StorageError::ResourceQuota)?;
    match &request.mutation {
        UpdateMutation::SetPolicy(policy) => {
            // Once Handoff commits, it is the serialized installation intent.
            // Policy cannot report withdrawal while that physical owner runs.
            if state.jobs.iter().any(|job| {
                matches!(
                    job.phase,
                    UpdateJobPhase::Handoff
                        | UpdateJobPhase::Installing
                        | UpdateJobPhase::NeedsReconcile
                )
            }) {
                return Err(StorageError::Busy);
            }
            if !policy.validate() || policy.revision != state.policy.revision {
                return Err(StorageError::InvalidCommand);
            }
            if policy.group_id != state.policy.group_id
                && state.jobs.iter().any(|job| !job.phase.settled())
            {
                return Err(StorageError::Busy);
            }
            state.policy = policy.clone();
            state.policy_owner = Some(UpdatePolicyOwner {
                principal_id: task.actor.principal_id,
                auth_epoch: task.actor.auth_epoch,
            });
            state.policy.revision = state
                .policy
                .revision
                .next()
                .map_err(|_| StorageError::ResourceQuota)?;
        }
        UpdateMutation::PrepareJob { release } => {
            if !state.policy.participation_enabled() || release.payload_sha256 == [0; 32] {
                return Err(StorageError::InvalidCommand);
            }
            if state
                .installed_release
                .is_some_and(|installed| release.sequence <= installed.sequence)
            {
                return Err(StorageError::InvalidCommand);
            }
            accept_release(&mut state, *release)?;
            if !state.reserve_job() {
                return Err(StorageError::ResourceQuota);
            }
            state.sequence = state
                .sequence
                .checked_add(1)
                .ok_or(StorageError::ResourceQuota)?;
            state.jobs.push(UpdateJob {
                id: state.sequence,
                command_id: request.command_id,
                owner: Some(UpdatePolicyOwner {
                    principal_id: task.actor.principal_id,
                    auth_epoch: task.actor.auth_epoch,
                }),
                policy_revision: state.policy.revision,
                release: *release,
                phase: UpdateJobPhase::Prepared,
            });
        }
        UpdateMutation::AdvanceJob { job_id, phase } => {
            let job = state
                .jobs
                .iter_mut()
                .find(|job| job.id == *job_id)
                .ok_or(StorageError::InvalidCommand)?;
            if !job.phase.can_advance_to(*phase) {
                return Err(StorageError::InvalidCommand);
            }
            job.phase = *phase;
            let release = job.release;
            if *phase == UpdateJobPhase::Succeeded {
                record_installed(&mut state, release)?;
            }
        }
    }
    state.revision = next;
    if state.receipts.len() == MAX_UPDATE_RECEIPTS {
        state.receipts.remove(0);
    }
    state.receipts.push(UpdateCommandReceipt {
        command_id: request.command_id,
        expected_revision: request.expected_revision,
        principal_id: task.actor.principal_id,
        auth_epoch: task.actor.auth_epoch,
        fingerprint,
    });
    crate::typed_command::raw_claim(&transaction, issued)?;
    let bytes = encode(&state)?;
    transaction.execute("INSERT INTO update_maintenance(singleton,value) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET value=excluded.value", [&bytes])
        .map_err(|_| StorageError::Database)?;
    if stop.load(Ordering::Acquire) {
        return Err(StorageError::Stopping);
    }
    task.authorize(&identity)?;
    window.check(&transaction, issued)?;
    transaction
        .commit()
        .map_err(|_| StorageError::Indeterminate)?;
    Ok(bytes)
}

const PHASES: [UpdateJobPhase; 10] = [
    UpdateJobPhase::Prepared,
    UpdateJobPhase::Downloading,
    UpdateJobPhase::Ready,
    UpdateJobPhase::WaitingIdle,
    UpdateJobPhase::Handoff,
    UpdateJobPhase::Installing,
    UpdateJobPhase::NeedsReconcile,
    UpdateJobPhase::Succeeded,
    UpdateJobPhase::Failed,
    UpdateJobPhase::Cancelled,
];
fn phase(phase: UpdateJobPhase) -> usize {
    PHASES.iter().position(|value| *value == phase).unwrap()
}
fn policy_value(policy: &UpdatePolicy) -> Value {
    json!({"revision":policy.revision.get(),"group":policy.group_id,"enabled":policy.enabled,"channel":policy.channel,
        "download":policy.download,"share":policy.share,"mode":match policy.install_mode { UpdateInstallMode::NotifyOnly=>0,UpdateInstallMode::ManualInstall=>1,UpdateInstallMode::MaintenanceWindow=>2,UpdateInstallMode::AutomaticAfterDownload=>3 },
        "start":policy.maintenance.start.get(),"end":policy.maintenance.end.get(),"downloadRate":policy.download_bytes_per_second,"uploadRate":policy.upload_bytes_per_second})
}
fn mutation_value(mutation: &UpdateMutation) -> Value {
    match mutation {
        UpdateMutation::SetPolicy(policy) => json!(["policy", policy_value(policy)]),
        UpdateMutation::PrepareJob { release } => {
            json!(["prepare", release.sequence.get(), release.payload_sha256])
        }
        UpdateMutation::AdvanceJob {
            job_id,
            phase: value,
        } => json!(["advance", job_id, phase(*value)]),
    }
}
fn release_value(release: Option<ReleaseIdentity>) -> Value {
    release.map_or(Value::Null, |r| json!([r.sequence.get(), r.payload_sha256]))
}
fn release_read(value: &Value) -> Result<Option<ReleaseIdentity>, StorageError> {
    if value.is_null() {
        return Ok(None);
    }
    let fields = value
        .as_array()
        .filter(|v| v.len() == 2)
        .ok_or(StorageError::Database)?;
    Ok(Some(ReleaseIdentity {
        sequence: ReleaseSequence::new(fields[0].as_u64().ok_or(StorageError::Database)?)
            .map_err(|_| StorageError::Database)?,
        payload_sha256: array(&fields[1])?,
    }))
}
fn encode(state: &UpdateMaintenanceState) -> Result<Vec<u8>, StorageError> {
    if !state.validate() {
        return Err(StorageError::InvalidCommand);
    }
    // Existing administrator-owned records retain the exact format3 layout. Only
    // built-in automatic jobs require the explicit nullable owner in format4.
    let format = if state.jobs.iter().any(|job| job.owner.is_none()) {
        4
    } else {
        3
    };
    let jobs: Vec<_> = state.jobs.iter().map(|job| {
        let mut value = json!({"id":job.id,"command":job.command_id.get(),"policy":job.policy_revision.get(),"sequence":job.release.sequence.get(),"hash":job.release.payload_sha256,"phase":phase(job.phase)});
        if format == 4 {
            value["owner"] = job.owner.map_or(Value::Null, |o| json!([o.principal_id.get(),o.auth_epoch.get()]));
        } else if let Some(owner) = job.owner {
            value["principal"] = json!(owner.principal_id.get());
            value["epoch"] = json!(owner.auth_epoch.get());
        }
        value
    }).collect();
    let receipts: Vec<_> = state.receipts.iter().map(|receipt| json!({"command":receipt.command_id.get(),"expected":receipt.expected_revision.get(),
        "principal":receipt.principal_id.get(),"epoch":receipt.auth_epoch.get(),"hash":receipt.fingerprint})).collect();
    let bytes = codec::bytes(
        &json!({"format":format,"revision":state.revision.get(),"sequence":state.sequence,"policy":policy_value(&state.policy),"owner":state.policy_owner.map(|o|json!([o.principal_id.get(),o.auth_epoch.get()])),"accepted":release_value(state.accepted_release),"installed":release_value(state.installed_release),"jobs":jobs,"receipts":receipts}),
    )?;
    if bytes.len() > MAX_VALUE_BYTES {
        return Err(StorageError::DocumentTooLarge);
    }
    Ok(bytes)
}
fn array<const N: usize>(value: &Value) -> Result<[u8; N], StorageError> {
    let array = value.as_array().ok_or(StorageError::Database)?;
    if array.len() != N {
        return Err(StorageError::Database);
    }
    let mut bytes = [0; N];
    for (slot, value) in bytes.iter_mut().zip(array) {
        *slot = value
            .as_u64()
            .and_then(|number| u8::try_from(number).ok())
            .ok_or(StorageError::Database)?;
    }
    Ok(bytes)
}
fn number(value: &Value, key: &str) -> Result<u64, StorageError> {
    codec::number(value, key)
}
fn boolean(value: &Value, key: &str) -> Result<bool, StorageError> {
    value[key].as_bool().ok_or(StorageError::Database)
}
fn policy_read(value: &Value) -> Result<UpdatePolicy, StorageError> {
    codec::object_fields(
        value,
        &[
            "revision",
            "group",
            "enabled",
            "channel",
            "download",
            "share",
            "mode",
            "start",
            "end",
            "downloadRate",
            "uploadRate",
        ],
    )?;
    let minute = |key| {
        Minute::new(u16::try_from(number(value, key)?).map_err(|_| StorageError::Database)?)
            .map_err(|_| StorageError::Database)
    };
    Ok(UpdatePolicy {
        revision: Revision::new(number(value, "revision")?),
        group_id: if value["group"].is_null() {
            None
        } else {
            Some(array(&value["group"])?)
        },
        enabled: boolean(value, "enabled")?,
        channel: value["channel"]
            .as_str()
            .ok_or(StorageError::Database)?
            .into(),
        download: boolean(value, "download")?,
        share: boolean(value, "share")?,
        install_mode: match number(value, "mode")? {
            0 => UpdateInstallMode::NotifyOnly,
            1 => UpdateInstallMode::ManualInstall,
            2 => UpdateInstallMode::MaintenanceWindow,
            3 => UpdateInstallMode::AutomaticAfterDownload,
            _ => return Err(StorageError::Database),
        },
        maintenance: MaintenanceWindow {
            start: minute("start")?,
            end: minute("end")?,
        },
        download_bytes_per_second: u32::try_from(number(value, "downloadRate")?)
            .map_err(|_| StorageError::Database)?,
        upload_bytes_per_second: u32::try_from(number(value, "uploadRate")?)
            .map_err(|_| StorageError::Database)?,
    })
}
fn owner_read(value: &Value) -> Result<Option<UpdatePolicyOwner>, StorageError> {
    if value.is_null() {
        return Ok(None);
    }
    let owner = value
        .as_array()
        .filter(|v| v.len() == 2)
        .ok_or(StorageError::Database)?;
    Ok(Some(UpdatePolicyOwner {
        principal_id: PrincipalId::new(owner[0].as_u64().ok_or(StorageError::Database)?)
            .map_err(|_| StorageError::Database)?,
        auth_epoch: AuthEpoch::new(owner[1].as_u64().ok_or(StorageError::Database)?),
    }))
}
fn decode(bytes: &[u8]) -> Result<UpdateMaintenanceState, StorageError> {
    let value = codec::value(bytes)?;
    let format = number(&value, "format")?;
    let fields_v1 = &[
        "format", "revision", "sequence", "policy", "jobs", "receipts",
    ][..];
    let fields_v2 = &[
        "format", "revision", "sequence", "policy", "owner", "jobs", "receipts",
    ][..];
    let fields_v3 = &[
        "format",
        "revision",
        "sequence",
        "policy",
        "owner",
        "accepted",
        "installed",
        "jobs",
        "receipts",
    ][..];
    codec::object_fields(
        &value,
        match format {
            1 => fields_v1,
            2 => fields_v2,
            3 | 4 => fields_v3,
            _ => return Err(StorageError::Database),
        },
    )?;
    let mut state = UpdateMaintenanceState {
        revision: Revision::new(number(&value, "revision")?),
        sequence: number(&value, "sequence")?,
        policy: policy_read(&value["policy"])?,
        policy_owner: if format == 1 {
            None
        } else {
            owner_read(&value["owner"])?
        },
        accepted_release: if format < 3 {
            None
        } else {
            release_read(&value["accepted"])?
        },
        installed_release: if format < 3 {
            None
        } else {
            release_read(&value["installed"])?
        },
        jobs: Vec::new(),
        receipts: Vec::new(),
    };
    let jobs = value["jobs"].as_array().ok_or(StorageError::Database)?;
    let receipts = value["receipts"].as_array().ok_or(StorageError::Database)?;
    if jobs.len() > MAX_UPDATE_JOBS || receipts.len() > MAX_UPDATE_RECEIPTS {
        return Err(StorageError::Database);
    }
    for value in jobs {
        let old_fields = &[
            "id",
            "command",
            "principal",
            "epoch",
            "policy",
            "sequence",
            "hash",
            "phase",
        ][..];
        let new_fields = &[
            "id", "command", "owner", "policy", "sequence", "hash", "phase",
        ][..];
        codec::object_fields(value, if format == 4 { new_fields } else { old_fields })?;
        let owner = if format == 4 {
            owner_read(&value["owner"])?
        } else {
            Some(UpdatePolicyOwner {
                principal_id: PrincipalId::new(number(value, "principal")?)
                    .map_err(|_| StorageError::Database)?,
                auth_epoch: AuthEpoch::new(number(value, "epoch")?),
            })
        };
        state.jobs.push(UpdateJob {
            id: number(value, "id")?,
            command_id: CommandId::new(number(value, "command")?)
                .map_err(|_| StorageError::Database)?,
            owner,
            policy_revision: Revision::new(number(value, "policy")?),
            release: ReleaseIdentity {
                sequence: ReleaseSequence::new(number(value, "sequence")?)
                    .map_err(|_| StorageError::Database)?,
                payload_sha256: array(&value["hash"])?,
            },
            phase: *PHASES
                .get(usize::try_from(number(value, "phase")?).map_err(|_| StorageError::Database)?)
                .ok_or(StorageError::Database)?,
        });
    }
    for value in receipts {
        codec::object_fields(
            value,
            &["command", "expected", "principal", "epoch", "hash"],
        )?;
        state.receipts.push(UpdateCommandReceipt {
            command_id: CommandId::new(number(value, "command")?)
                .map_err(|_| StorageError::Database)?,
            expected_revision: Revision::new(number(value, "expected")?),
            principal_id: PrincipalId::new(number(value, "principal")?)
                .map_err(|_| StorageError::Database)?,
            auth_epoch: AuthEpoch::new(number(value, "epoch")?),
            fingerprint: array(&value["hash"])?,
        });
    }
    if format < 3 {
        state.accepted_release = state
            .jobs
            .iter()
            .map(|j| j.release)
            .max_by_key(|r| r.sequence);
        state.installed_release = state
            .jobs
            .iter()
            .filter(|j| j.phase == UpdateJobPhase::Succeeded)
            .map(|j| j.release)
            .max_by_key(|r| r.sequence);
    }
    if !state.validate() {
        return Err(StorageError::Database);
    }
    Ok(state)
}

#[cfg(test)]
#[path = "tests/updates.rs"]
mod tests;
