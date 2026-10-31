use super::slot::Slot;
use crate::{
    backend::{Backend, BackendError, ConfirmedRestore, OrdinaryBackupAccess, RestorePreview},
    recovery::{BackupError, ColdShutdown, ShutdownError},
    recovery_files::{
        ColdCommitReceipt, ExportReceipt, PreparedRestore, RecoveryFileError, RecoveryFiles,
    },
    voice_runtime::VoiceRuntime,
};
use fc_application::identity::SessionLease;
use fc_domain::{ids::Revision, recovery::BackupEntry};
use fc_runtime::worker::{Task, WorkerPool};
use fc_storage::recovery::ExecutionFence;
use fc_windows::{materials::chooser::SelectedFile, system::DataPaths};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkflowError {
    Busy,
    Cancelled,
    Backend(BackendError),
    Files(RecoveryFileError),
    Shutdown(ShutdownError),
}
impl From<BackendError> for WorkflowError {
    fn from(value: BackendError) -> Self {
        Self::Backend(value)
    }
}
impl From<RecoveryFileError> for WorkflowError {
    fn from(value: RecoveryFileError) -> Self {
        Self::Files(value)
    }
}
impl From<ShutdownError> for WorkflowError {
    fn from(value: ShutdownError) -> Self {
        Self::Shutdown(value)
    }
}
type Result<T> = std::result::Result<T, WorkflowError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSummary {
    pub school_id: fc_domain::ids::SchoolId,
    pub source_revision: Revision,
    pub current_revision: Revision,
    pub document_count: u64,
    pub material_count: usize,
    pub profile_media_count: usize,
    pub payload_bytes: u64,
    pub database_bytes: u64,
    pub material_bytes: u64,
    pub profile_media_bytes: u64,
}
enum Payload {
    Preview {
        prepared: PreparedRestore,
        preview: RestorePreview,
    },
    Confirmed {
        prepared: PreparedRestore,
        confirmed: ConfirmedRestore,
    },
}
/// One preview/confirmation/coordinator slot for the entire native application.
/// No additional ordinary file worker is created here.
#[derive(Clone)]
pub struct RecoveryWorkflow {
    paths: DataPaths,
    slot: Slot<Payload>,
    confirmation: Arc<AtomicU8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmationState {
    Idle,
    Running,
    Confirmed,
    Failed,
}
struct ConfirmationAttempt(Arc<AtomicU8>);
impl Drop for ConfirmationAttempt {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(1, 3, Ordering::AcqRel, Ordering::Acquire);
    }
}
pub struct ReviewBundle {
    workflow: RecoveryWorkflow,
    generation: u64,
    summary: ReviewSummary,
    armed: bool,
}
impl ReviewBundle {
    pub fn summary(&self) -> &ReviewSummary {
        &self.summary
    }
}
impl Drop for ReviewBundle {
    fn drop(&mut self) {
        if self.armed {
            self.workflow.slot.retire(self.generation);
        }
    }
}
/// Lightweight notification only. The irreversible capability remains in the
/// workflow slot even when a cancelled/stale JobTicket discards this result.
pub struct ConfirmedBundle {
    workflow: RecoveryWorkflow,
    generation: u64,
}
struct Reservation {
    workflow: RecoveryWorkflow,
    generation: u64,
    active: bool,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if self.active {
            self.workflow.slot.finish(self.generation);
        }
    }
}
impl RecoveryWorkflow {
    pub fn new(paths: DataPaths) -> Self {
        Self {
            paths,
            slot: Slot::new(),
            confirmation: Arc::new(AtomicU8::new(0)),
        }
    }
    pub fn confirmation_state(&self) -> ConfirmationState {
        match self.confirmation.load(Ordering::Acquire) {
            1 => ConfirmationState::Running,
            2 => ConfirmationState::Confirmed,
            3 => ConfirmationState::Failed,
            _ => ConfirmationState::Idle,
        }
    }
    pub fn is_idle(&self) -> bool {
        self.slot.is_idle()
    }
    pub fn retirement_pending(&self) -> bool {
        !self.has_confirmed() && self.slot.retirement_pending()
    }
    /// UI-safe: no filesystem operation or heavy destructor runs here.
    pub fn request_close(&self) {
        self.slot.request_close();
    }
    /// Run only on an existing shared worker. The slot remains occupied through
    /// the final physical destructor, including recursive workspace removal.
    pub fn retire_pending_on_worker(&self) -> bool {
        if self.has_confirmed() {
            return false;
        }
        if let Some((generation, payload)) = self.slot.take_retired() {
            drop(payload);
            self.slot.finish(generation);
        }
        self.is_idle()
    }
    /// A durable barrier is never discarded merely because its UI job became
    /// stale, was cancelled, or was dropped before consuming its result.
    pub fn has_confirmed(&self) -> bool {
        self.slot
            .inspect(|_, value| matches!(value, Some(Payload::Confirmed { .. })))
            == Some(true)
    }
    pub fn confirmed_bundle(&self) -> Option<ConfirmedBundle> {
        self.slot
            .inspect(|generation, value| {
                matches!(value, Some(Payload::Confirmed { .. })).then(|| ConfirmedBundle {
                    workflow: self.clone(),
                    generation,
                })
            })
            .flatten()
    }
    /// Shared worker only; the export adapter rechecks exact authority after
    /// acquiring all original pins and immediately before publishing its file.
    pub fn export(
        &self,
        access: OrdinaryBackupAccess,
        fence: ExecutionFence,
    ) -> Result<ExportReceipt> {
        crate::recovery::export_ordinary_backup(&self.paths, access, fence).map_err(|error| {
            match error {
                BackupError::Backend(error) => WorkflowError::Backend(error),
                BackupError::Files(error) => WorkflowError::Files(error),
            }
        })
    }
    /// Shared worker only. Both framing/media and the complete ordinary SQL
    /// projection succeed before any review is exposed to the UI.
    pub fn preflight(
        &self,
        selected: SelectedFile,
        access: OrdinaryBackupAccess,
        execution: ExecutionFence,
    ) -> Result<ReviewBundle> {
        let generation = self.slot.reserve().ok_or(WorkflowError::Busy)?;
        let mut reservation = Reservation {
            workflow: self.clone(),
            generation,
            active: true,
        };
        let slot = self.slot.clone();
        let fence: ExecutionFence = Arc::new(move || {
            slot.permits(generation)
                && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execution()))
                    .unwrap_or(false)
        });
        let files = RecoveryFiles::new(&self.paths)?;
        let prepared = files.preflight_package(selected, &*fence)?;
        let preview = access.preflight_restore(
            prepared.ordinary_database().to_owned(),
            prepared
                .ordinary_database()
                .with_file_name("preview.sqlite3"),
            prepared.manifest().clone(),
            prepared.manifest_sha256(),
            fence.clone(),
        )?;
        if !fence() {
            return Err(WorkflowError::Cancelled);
        }
        let mut summary = ReviewSummary {
            school_id: preview.manifest().school_id,
            source_revision: preview.manifest().revision,
            current_revision: preview.current_revision(),
            document_count: preview.document_count(),
            material_count: 0,
            profile_media_count: 0,
            payload_bytes: 0,
            database_bytes: 0,
            material_bytes: 0,
            profile_media_bytes: 0,
        };
        for entry in &preview.manifest().entries {
            summary.payload_bytes = summary
                .payload_bytes
                .checked_add(entry.bytes())
                .ok_or(WorkflowError::Files(RecoveryFileError::InvalidPackage))?;
            match entry {
                BackupEntry::Material { bytes, .. } => {
                    summary.material_count += 1;
                    summary.material_bytes += bytes;
                }
                BackupEntry::ProfileMedia { bytes, .. } => {
                    summary.profile_media_count += 1;
                    summary.profile_media_bytes += bytes;
                }
                BackupEntry::Database { bytes, .. } => summary.database_bytes = *bytes,
            }
        }
        if !self
            .slot
            .publish(generation, Payload::Preview { prepared, preview })
        {
            return Err(WorkflowError::Cancelled);
        }
        reservation.active = false;
        Ok(ReviewBundle {
            workflow: self.clone(),
            generation,
            summary,
            armed: true,
        })
    }
    /// Shared worker only. No mutex spans the SQLite barrier. Cancellation can
    /// prevent its commit, but cannot erase a capability already committed.
    pub fn confirm(
        &self,
        mut bundle: ReviewBundle,
        backend: Arc<Mutex<Backend>>,
        lease: SessionLease,
        fence: ExecutionFence,
    ) -> Result<ConfirmedBundle> {
        if !self.slot.same(&bundle.workflow.slot) {
            return Err(WorkflowError::Busy);
        }
        let generation = bundle.generation;
        let payload = self
            .slot
            .take_ready(generation)
            .ok_or(WorkflowError::Cancelled)?;
        bundle.armed = false;
        let mut reservation = Reservation {
            workflow: self.clone(),
            generation,
            active: true,
        };
        self.confirmation.store(1, Ordering::Release);
        let _attempt = ConfirmationAttempt(self.confirmation.clone());
        let Payload::Preview { prepared, preview } = payload else {
            return Err(WorkflowError::Busy);
        };
        let review = backend
            .lock()
            .map_err(|_| WorkflowError::Backend(BackendError::Closed))?
            .review_preflighted_restore(lease, preview)?;
        let confirmed = review.confirm(fence)?;
        // Cancellation is intentionally not consulted after the durable barrier.
        // This slot cannot be reused while the reservation owns its generation.
        if !self.slot.publish_irreversible(
            generation,
            Payload::Confirmed {
                prepared,
                confirmed,
            },
        ) {
            // Poison is fail-closed. The backend remains recovering and ordinary
            // admission cannot resume. Normal operation never enters this arm.
            return Err(WorkflowError::Backend(BackendError::Closed));
        }
        reservation.active = false;
        self.confirmation.store(2, Ordering::Release);
        Ok(ConfirmedBundle {
            workflow: self.clone(),
            generation,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestorePhase {
    Waiting,
    Draining,
    PreparingDatabase,
    Sealing,
    Committing,
    Retained,
    Finished,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreStatus {
    pub phase: RestorePhase,
    pub cancellation_requested: bool,
    /// Published only after worker/audio ownership and workspace cleanup finish.
    pub result: Option<Result<ColdCommitReceipt>>,
}
struct Inputs<T: Task> {
    bundle: Option<ConfirmedBundle>,
    backend: Arc<Mutex<Backend>>,
    workers: WorkerPool<T>,
    audio: VoiceRuntime,
}
struct CoordinatorState<T: Task> {
    input: Mutex<Option<Inputs<T>>>,
    wake: Condvar,
    aborted: AtomicBool,
    panicked: AtomicBool,
    cancel: AtomicBool,
    status: Mutex<RestoreStatus>,
}
/// Prepare BEFORE moving the App's pool/audio owners. If thread creation fails,
/// those owners have not moved and the workflow's confirmed slot is preserved.
pub struct PreparedCoordinator<T: Task> {
    state: Arc<CoordinatorState<T>>,
    thread: Option<JoinHandle<()>>,
}
pub struct RestoreOperation<T: Task> {
    state: Arc<CoordinatorState<T>>,
    thread: Option<JoinHandle<()>>,
}
impl<T: Task> PreparedCoordinator<T> {
    pub fn prepare() -> std::io::Result<Self> {
        let state = Arc::new(CoordinatorState {
            input: Mutex::new(None),
            wake: Condvar::new(),
            aborted: AtomicBool::new(false),
            panicked: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            status: Mutex::new(RestoreStatus {
                phase: RestorePhase::Waiting,
                cancellation_requested: false,
                result: None,
            }),
        });
        let owned = state.clone();
        let thread = thread::Builder::new()
            .name("fc-restore-close".into())
            .spawn(move || coordinator(owned))?;
        Ok(Self {
            state,
            thread: Some(thread),
        })
    }
    pub fn start(
        self,
        bundle: ConfirmedBundle,
        backend: Arc<Mutex<Backend>>,
        workers: WorkerPool<T>,
        audio: VoiceRuntime,
    ) -> RestoreOperation<T> {
        self.launch(Inputs {
            bundle: Some(bundle),
            backend,
            workers,
            audio,
        })
    }
    /// Barrier failure can leave the original deployment recovering without a
    /// consumable restore capability. This path only drains/joins and closes
    /// that original writer; it never constructs or installs a candidate.
    pub fn start_abort_original(
        self,
        backend: Arc<Mutex<Backend>>,
        workers: WorkerPool<T>,
        audio: VoiceRuntime,
    ) -> RestoreOperation<T> {
        self.launch(Inputs {
            bundle: None,
            backend,
            workers,
            audio,
        })
    }
    fn launch(mut self, inputs: Inputs<T>) -> RestoreOperation<T> {
        *self
            .state
            .input
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(inputs);
        self.state.wake.notify_one();
        RestoreOperation {
            state: self.state.clone(),
            thread: self.thread.take(),
        }
    }
}
impl<T: Task> Drop for PreparedCoordinator<T> {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.state.aborted.store(true, Ordering::Release);
            self.state.wake.notify_one();
        }
    }
}
impl<T: Task> RestoreOperation<T> {
    pub fn cancel(&self) {
        self.state.cancel.store(true, Ordering::Release);
    }
    pub fn status(&self) -> RestoreStatus {
        let mut status = self
            .state
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        status.cancellation_requested = self.state.cancel.load(Ordering::Acquire);
        status
    }
    /// The result alone is insufficient: only this physical join releases the
    /// UI's operation slot. Polling is always nonblocking.
    pub fn reap_nonblocking(&mut self) -> bool {
        if self.state.panicked.load(Ordering::Acquire) {
            return false;
        }
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| !crate::physical_thread::is_finished(thread))
        {
            return false;
        }
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                self.state.panicked.store(true, Ordering::Release);
                let mut status = self
                    .state
                    .status
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                status.phase = RestorePhase::Retained;
                // An unwind may have lost the ability to prove physical drain.
                // A terminal coordinator is not evidence for its native owners.
                status.result = None;
                return false;
            }
        }
        true
    }
}
impl<T: Task> Drop for RestoreOperation<T> {
    fn drop(&mut self) {
        // The independent thread still owns its Inputs/PreparedRestore and the
        // workflow slot remains busy. App shutdown must retain and reap this
        // handle; an accidental UI drop never frees physical ownership early.
        self.state.cancel.store(true, Ordering::Release);
    }
}
fn set_phase<T: Task>(state: &CoordinatorState<T>, phase: RestorePhase) {
    state
        .status
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .phase = phase;
}
fn coordinator<T: Task>(state: Arc<CoordinatorState<T>>) {
    let mut input = state
        .input
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    while input.is_none() && !state.aborted.load(Ordering::Acquire) {
        input = state
            .wake
            .wait(input)
            .unwrap_or_else(|error| error.into_inner());
    }
    let Some(input_value) = input.take() else {
        return;
    };
    drop(input);
    let Inputs {
        bundle,
        backend,
        workers,
        audio,
    } = input_value;
    let result = if let Some(bundle) = bundle {
        let workflow = bundle.workflow;
        let generation = bundle.generation;
        let payload = workflow.slot.take_irreversible(generation);
        let result = match payload {
            Some(Payload::Confirmed {
                prepared,
                confirmed,
            }) => restore_owned(&state, prepared, confirmed, backend, workers, audio),
            other => {
                let result = abort_original_owned(&state, backend, workers, audio);
                drop(other);
                result
            }
        };
        workflow.slot.finish(generation);
        workflow.request_close();
        result
    } else {
        abort_original_owned(&state, backend, workers, audio)
    };
    let mut status = state
        .status
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    status.phase = RestorePhase::Finished;
    status.result = Some(result);
}
fn abort_original_owned<T: Task>(
    state: &CoordinatorState<T>,
    backend: Arc<Mutex<Backend>>,
    workers: WorkerPool<T>,
    audio: VoiceRuntime,
) -> Result<ColdCommitReceipt> {
    set_phase(state, RestorePhase::Draining);
    workers.request_stop();
    audio.shutdown();
    while !workers.is_finished_with(crate::physical_thread::is_finished) {
        thread::park_timeout(Duration::from_millis(10));
    }
    // A fault result still joins both physical threads; it grants no restore.
    let _ = workers.join();
    loop {
        let _ = audio.reap_nonblocking();
        if audio.status().is_ok_and(|status| {
            status.phase == crate::voice_runtime::RuntimePhase::Closed
                && !status.physical_thread_running
                && status.ack_generation >= status.stop_generation
        }) {
            break;
        }
        set_phase(state, RestorePhase::Retained);
        thread::park_timeout(Duration::from_millis(20));
    }
    let closed = backend.lock().is_ok_and(|mut owned| owned.close().is_ok());
    if !closed {
        // Poison or a failed close is not proof that the original writer was
        // joined. Keep its Arc and the coordinator alive indefinitely.
        set_phase(state, RestorePhase::Retained);
        loop {
            thread::park_timeout(Duration::from_millis(20));
        }
    }
    Err(WorkflowError::Cancelled)
}
fn restore_owned<T: Task>(
    state: &Arc<CoordinatorState<T>>,
    prepared: PreparedRestore,
    confirmed: ConfirmedRestore,
    backend: Arc<Mutex<Backend>>,
    workers: WorkerPool<T>,
    audio: VoiceRuntime,
) -> Result<ColdCommitReceipt> {
    set_phase(state, RestorePhase::Draining);
    let (mut shutdown, initial_error) =
        ColdShutdown::begin_retained(confirmed, backend, workers, audio);
    if let Some(error) = initial_error {
        set_phase(state, RestorePhase::Retained);
        loop {
            if shutdown.finish_failed_nonblocking().unwrap_or(false) {
                return Err(error.into());
            }
            thread::park_timeout(Duration::from_millis(20));
        }
    }
    let drained = loop {
        match shutdown.poll() {
            Ok(Some(drained)) => break drained,
            Ok(None) => {}
            Err(error) => {
                set_phase(state, RestorePhase::Retained);
                loop {
                    if shutdown.finish_failed_nonblocking().unwrap_or(false) {
                        return Err(error.into());
                    }
                    thread::park_timeout(Duration::from_millis(20));
                }
            }
        }
        thread::park_timeout(Duration::from_millis(10));
    };
    // Independent of the stopped shared-pool JobContext. Cancelling while
    // draining still waits for actual physical completion and writer close.
    let owned = state.clone();
    let fence: ExecutionFence = Arc::new(move || !owned.cancel.load(Ordering::Acquire));
    set_phase(state, RestorePhase::PreparingDatabase);
    let permit = drained.prepare_database(prepared.candidate_database(), fence.clone())?;
    set_phase(state, RestorePhase::Sealing);
    let sealed = prepared.seal_candidate(permit.prepared_database(), &*fence)?;
    set_phase(state, RestorePhase::Committing);
    // A partial commit owns a durable journal; its adapter retains every tree
    // needed by controlled startup reconciliation when cancellation occurs.
    sealed.commit_cold(permit, &*fence).map_err(Into::into)
}

#[cfg(test)]
mod tests;
