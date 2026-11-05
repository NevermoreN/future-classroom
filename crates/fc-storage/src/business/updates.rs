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
        StorageError::InvalidCommand => UpdateRepositoryError::Invalid,
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
pub(crate) fn execute(
    db: &mut Connection,
    task: Task,
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
            if !policy.validate() || policy.revision != state.policy.revision {
                return Err(StorageError::InvalidCommand);
            }
            if policy.group_id != state.policy.group_id
                && state.jobs.iter().any(|job| !job.phase.settled())
            {
                return Err(StorageError::Busy);
            }
            state.policy = policy.clone();
            state.policy.revision = state
                .policy
                .revision
                .next()
                .map_err(|_| StorageError::ResourceQuota)?;
        }
        UpdateMutation::PrepareJob { release } => {
            if !state.policy.enabled || release.payload_sha256 == [0; 32] {
                return Err(StorageError::InvalidCommand);
            }
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
                principal_id: task.actor.principal_id,
                auth_epoch: task.actor.auth_epoch,
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
    let bytes = encode(&state)?;
    transaction.execute("INSERT INTO update_maintenance(singleton,value) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET value=excluded.value", [&bytes])
        .map_err(|_| StorageError::Database)?;
    if stop.load(Ordering::Acquire) {
        return Err(StorageError::Stopping);
    }
    task.authorize(&identity)?;
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
        "download":policy.download,"share":policy.share,"mode":match policy.install_mode { UpdateInstallMode::NotifyOnly=>0,UpdateInstallMode::ManualInstall=>1,UpdateInstallMode::MaintenanceWindow=>2 },
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
fn encode(state: &UpdateMaintenanceState) -> Result<Vec<u8>, StorageError> {
    if !state.validate() {
        return Err(StorageError::InvalidCommand);
    }
    let jobs: Vec<_> = state.jobs.iter().map(|job| json!({"id":job.id,"command":job.command_id.get(),"principal":job.principal_id.get(),
        "epoch":job.auth_epoch.get(),"policy":job.policy_revision.get(),"sequence":job.release.sequence.get(),"hash":job.release.payload_sha256,"phase":phase(job.phase)})).collect();
    let receipts: Vec<_> = state.receipts.iter().map(|receipt| json!({"command":receipt.command_id.get(),"expected":receipt.expected_revision.get(),
        "principal":receipt.principal_id.get(),"epoch":receipt.auth_epoch.get(),"hash":receipt.fingerprint})).collect();
    let bytes = codec::bytes(
        &json!({"format":1,"revision":state.revision.get(),"sequence":state.sequence,"policy":policy_value(&state.policy),"jobs":jobs,"receipts":receipts}),
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
fn decode(bytes: &[u8]) -> Result<UpdateMaintenanceState, StorageError> {
    let value = codec::value(bytes)?;
    codec::object_fields(
        &value,
        &[
            "format", "revision", "sequence", "policy", "jobs", "receipts",
        ],
    )?;
    if number(&value, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let mut state = UpdateMaintenanceState {
        revision: Revision::new(number(&value, "revision")?),
        sequence: number(&value, "sequence")?,
        policy: policy_read(&value["policy"])?,
        jobs: Vec::new(),
        receipts: Vec::new(),
    };
    let jobs = value["jobs"].as_array().ok_or(StorageError::Database)?;
    let receipts = value["receipts"].as_array().ok_or(StorageError::Database)?;
    if jobs.len() > MAX_UPDATE_JOBS || receipts.len() > MAX_UPDATE_RECEIPTS {
        return Err(StorageError::Database);
    }
    for value in jobs {
        codec::object_fields(
            value,
            &[
                "id",
                "command",
                "principal",
                "epoch",
                "policy",
                "sequence",
                "hash",
                "phase",
            ],
        )?;
        state.jobs.push(UpdateJob {
            id: number(value, "id")?,
            command_id: CommandId::new(number(value, "command")?)
                .map_err(|_| StorageError::Database)?,
            principal_id: PrincipalId::new(number(value, "principal")?)
                .map_err(|_| StorageError::Database)?,
            auth_epoch: AuthEpoch::new(number(value, "epoch")?),
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
    if !state.validate() {
        return Err(StorageError::Database);
    }
    Ok(state)
}

#[cfg(test)]
#[path = "tests/updates.rs"]
mod tests;
