#![forbid(unsafe_code)]
//! Durable opaque documents and command receipts. SQLite is created, used and
//! destroyed on one writer thread. Authentication belongs to the application;
//! callers must check the runtime health gate before exposing sensitive reads.
use fc_domain::ids::Revision;
use fc_ports::AuthorityPublisher;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub mod business;
pub mod command_window;
mod command_window_legacy;
#[cfg(test)]
pub(crate) mod fixture_commands;
pub mod login;
pub mod recovery;
pub mod typed_command;

pub const SCHEMA_VERSION: i64 = 1;
pub const MAX_COMMAND_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_QUEUED_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_VALUE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    InvalidCommand,
    /// The scoped request no longer owns permission to execute or expose output.
    ExecutionCancelled,
    CalendarCancelled,
    AutostartCancelled,
    DocumentTooLarge,
    Schedule(fc_domain::schedule_document::ScheduleDocumentError),
    Display(fc_domain::desktop::DisplayError),
    HttpsSettings(fc_domain::https_settings::HttpsSettingsError),
    SubjectVisual(fc_domain::subject_visual::SubjectVisualError),
    Profile(fc_domain::profile::ProfileError),
    ProfileContent(fc_domain::profile_content::ProfileContentError),
    Material(fc_domain::materials::MaterialError),
    Lesson(fc_domain::lesson_context::LessonContextError),
    Voice(fc_domain::voice::VoiceError),
    ResourceQuota,
    LoginRateLimited {
        retry_after_millis: u64,
    },
    LoginInProgress,
    LoginCapacity,
    LoginReservationExpired,
    LoginStaleCredential,
    LoginClockRegression,
    CredentialCorrupt,
    Content(fc_domain::content::ContentError),
    Identity(fc_domain::identity::IdentityError),
    Busy,
    Stopping,
    WorkerDisconnected,
    Database,
    UnsupportedSchema,
    Conflict {
        current: Revision,
    },
    CommandIdReused,
    CommandExpired,
    CommandNotIssued,
    CommandWindowCapacity,
    CommandWindowClock,
    AuthorityUnavailable,
    /// SQLite committed. The authority exchange failed and the process must fail closed.
    PublicationFailed {
        committed_revision: Revision,
    },
    Indeterminate,
    WorkerPanicked,
    BackupDestinationExists,
    BackupTimedOut,
    RecoveryInvalidPackage,
    RecoveryConflict,
    RecoveryCancelled,
}

impl StorageError {
    pub fn command_window_error(&self) -> Option<fc_ports::command_window::CommandWindowError> {
        use fc_ports::command_window::CommandWindowError as E;
        match self {
            Self::CommandExpired => Some(E::Expired),
            Self::CommandNotIssued => Some(E::NotIssued),
            Self::CommandWindowCapacity => Some(E::Capacity),
            Self::CommandWindowClock => Some(E::Clock),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub value: Vec<u8>,
    pub revision: Revision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mutation {
    Put { key: String, value: Vec<u8> },
    Delete { key: String },
}

#[derive(Debug)]
pub struct CommitCommand {
    id: String,
    expected_revision: Revision,
    mutations: Vec<Mutation>,
    fingerprint: [u8; 32],
}

impl CommitCommand {
    pub fn new(
        id: String,
        expected_revision: Revision,
        mutations: Vec<Mutation>,
    ) -> Result<Self, StorageError> {
        if id.is_empty() || id.len() > 128 || mutations.is_empty() || mutations.len() > 1024 {
            return Err(StorageError::InvalidCommand);
        }
        let mut bytes = id.len();
        let mut seen = std::collections::BTreeSet::new();
        let mut digest = Sha256::new();
        digest.update(b"future-classroom/storage-command/v1\0");
        digest.update(expected_revision.get().to_be_bytes());
        for mutation in &mutations {
            let (key, value) = match mutation {
                Mutation::Put { key, value } => (key, Some(value)),
                Mutation::Delete { key } => (key, None),
            };
            if key.is_empty() || key.len() > 256 || !seen.insert(key) {
                return Err(StorageError::InvalidCommand);
            }
            bytes = bytes
                .checked_add(key.len() + value.map_or(0, Vec::len))
                .ok_or(StorageError::InvalidCommand)?;
            if bytes > MAX_COMMAND_BYTES || value.is_some_and(|v| v.len() > MAX_VALUE_BYTES) {
                return Err(StorageError::InvalidCommand);
            }
            digest.update([u8::from(value.is_some())]);
            digest.update((key.len() as u64).to_be_bytes());
            digest.update(key.as_bytes());
            if let Some(value) = value {
                digest.update((value.len() as u64).to_be_bytes());
                digest.update(value);
            }
        }
        Ok(Self {
            id,
            expected_revision,
            mutations,
            fingerprint: digest.finalize().into(),
        })
    }

    pub fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    fn allocation_bytes(&self) -> Option<usize> {
        let mut bytes = self.id.capacity().checked_add(
            self.mutations
                .capacity()
                .checked_mul(std::mem::size_of::<Mutation>())?,
        )?;
        for mutation in &self.mutations {
            let allocation = match mutation {
                Mutation::Put { key, value } => key.capacity().checked_add(value.capacity())?,
                Mutation::Delete { key } => key.capacity(),
            };
            bytes = bytes.checked_add(allocation)?;
        }
        Some(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitReceipt {
    pub revision: Revision,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupReceipt {
    /// Revision read from the completed backup, never an earlier source sample.
    pub revision: Revision,
    pub schema_version: i64,
}

/// Poll on UI/HTTP threads; blocking wait is for workers and startup/shutdown only.
/// A scoped execution fence also checks positive reply delivery. Cancellation at
/// delivery suppresses output; it cannot undo a write that previously committed
/// while its fence was valid. Do not infer rollback or retry from that error.
pub struct Ticket<T>(
    mpsc::Receiver<Result<T, StorageError>>,
    Option<TransactionExecutionFence>,
);
impl<T> Ticket<T> {
    fn checked(&self, result: Result<T, StorageError>) -> Result<T, StorageError> {
        // Preserve factual/typed failures. Only positive output is suppressed by
        // expiry after the writer queued its reply, including read-only secrets.
        let output = result?;
        if let Some(fence) = &self.1 {
            check_transaction_execution(fence)?;
        }
        Ok(output)
    }
    pub fn try_result(&self) -> Result<Option<Result<T, StorageError>>, StorageError> {
        match self.0.try_recv() {
            Ok(result) => Ok(Some(self.checked(result))),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(StorageError::WorkerDisconnected),
        }
    }
    pub fn wait(self) -> Result<T, StorageError> {
        let result = self
            .0
            .recv()
            .map_err(|_| StorageError::WorkerDisconnected)?;
        self.checked(result)
    }
    /// A deadline only bounds the caller's wait; it cannot undo a COMMIT that
    /// has already started. Callers must fence further effects and reconcile an
    /// indeterminate outcome, rather than retrying a new command automatically.
    pub fn wait_timeout(self, timeout: Duration) -> Result<T, StorageError> {
        let result = self.0.recv_timeout(timeout).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => StorageError::Indeterminate,
            mpsc::RecvTimeoutError::Disconnected => StorageError::WorkerDisconnected,
        })?;
        self.checked(result)
    }
}

/// A bounded view valid only during one writer transaction. It exposes owned
/// documents, never SQLite handles. Implementations must not retain the view.
pub trait TransactionDocuments {
    fn typed_history(&self) -> Option<&typed_command::StoredTyped> {
        None
    }
    fn active_command(&self) -> Option<fc_ports::command_window::IssuedCommand> {
        None
    }
    fn get(&self, key: &str) -> Result<Option<Document>, StorageError>;
    /// Internal admitted-owner lookup. The complete persisted admission must
    /// match; this never grants client replay or a new positive command.
    fn profile_content_owner_receipt(
        &self,
        admission: &fc_domain::profile_content::ContentAdmission,
    ) -> Result<Option<(String, Document)>, StorageError> {
        let key = format!(
            "business/profile-content/v1/receipts/{:020}",
            admission.request.command_id.get()
        );
        Ok(self.get(&key)?.map(|document| (key, document)))
    }
    fn revision(&self) -> Result<Revision, StorageError>;
    /// Metadata-only ordered page, valid only inside the current writer transaction.
    /// At most 128 rows and 512 KiB of owned key/value capacity are returned.
    /// A short page can still have a continuation: follow next_key, never len().
    fn scan_prefix(
        &self,
        prefix: &str,
        after_key: Option<&str>,
        limit: usize,
    ) -> Result<DocumentPage, StorageError>;
}
pub struct DocumentPage {
    pub entries: Vec<(String, Document)>,
    pub next_key: Option<String>,
}
/// Trusted composition code only. Evaluate must be short, deterministic except
/// its injected clock, and must never reenter the writer, perform file I/O or KDF.
/// Use owned inputs; borrowed request clocks cannot cross the worker boundary.
/// Fixed trusted task classification; no request, DTO or client can select it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransactionLifetime {
    Ordinary,
    OwnerReturn,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CommandRetirement {
    None,
    IdentityRecovery,
    DatabaseRestoreBarrier,
}
pub trait TransactionTask<S>: Send {
    fn window_action(&self) -> Option<typed_command::WindowAction> {
        None
    }
    fn retirement(&self) -> CommandRetirement {
        CommandRetirement::None
    }
    fn command_family(&self) -> Option<fc_ports::command_window::CommandFamily> {
        None
    }
    fn lifetime(&self) -> TransactionLifetime {
        TransactionLifetime::Ordinary
    }

    /// Exact positive request/history identity. Internal physical-return tasks
    /// have no client request and must not manufacture a new issuance.
    fn command_requirement(
        &self,
    ) -> Option<(
        fc_ports::command_window::CommandFamily,
        fc_domain::ids::CommandId,
    )> {
        None
    }

    fn allocation_bytes(&self) -> usize;
    fn evaluate(
        self: Box<Self>,
        documents: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError>;
}
/// A short memory-only request lifetime check, never I/O or writer reentry.
/// False and panic cancel only this request; this grants no actor authority.
pub type TransactionExecutionFence = Arc<dyn Fn() -> bool + Send + Sync>;
fn check_transaction_execution(fence: &TransactionExecutionFence) -> Result<(), StorageError> {
    if catch_unwind(AssertUnwindSafe(|| fence())).unwrap_or(false) {
        Ok(())
    } else {
        Err(StorageError::ExecutionCancelled)
    }
}
struct FencedTransaction<S> {
    task: Box<dyn TransactionTask<S>>,
    fence: TransactionExecutionFence,
}
impl<S> TransactionTask<S> for FencedTransaction<S> {
    fn window_action(&self) -> Option<typed_command::WindowAction> {
        self.task.window_action()
    }
    fn retirement(&self) -> CommandRetirement {
        self.task.retirement()
    }
    fn command_family(&self) -> Option<fc_ports::command_window::CommandFamily> {
        self.task.command_family()
    }
    fn lifetime(&self) -> TransactionLifetime {
        self.task.lifetime()
    }
    fn command_requirement(
        &self,
    ) -> Option<(
        fc_ports::command_window::CommandFamily,
        fc_domain::ids::CommandId,
    )> {
        self.task.command_requirement()
    }
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>().saturating_add(self.task.allocation_bytes())
    }
    fn evaluate(
        self: Box<Self>,
        documents: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        check_transaction_execution(&self.fence)?;
        let mut decision = self.task.evaluate(documents)?;
        check_transaction_execution(&self.fence)?;
        let original = decision.precommit.take();
        let fence = self.fence;
        decision.precommit = Some(Box::new(move || {
            check_transaction_execution(&fence)?;
            if let Some(original) = original {
                original()?;
            }
            check_transaction_execution(&fence)
        }));
        Ok(decision)
    }
}
pub struct TransactionDecision<S> {
    /// Memory-only final lease/date recheck after SQL writes, immediately before
    /// COMMIT. Never perform I/O/KDF or reenter the writer here.
    pub precommit: Option<Box<dyn FnOnce() -> Result<(), StorageError> + Send>>,
    pub outcome: Vec<u8>,
    /// None performs a read-only transaction. Empty writes must use None.
    pub commit: Option<(Vec<Mutation>, S)>,
}
/// Borrow SQLite's current value and enforce the allocation bound before copying.
/// A damaged BLOB must never allocate its declared size on the Rust heap.
pub(crate) fn read_limited_blob(
    row: &rusqlite::Row<'_>,
    column: usize,
) -> rusqlite::Result<Vec<u8>> {
    match row.get_ref(column)? {
        rusqlite::types::ValueRef::Blob(value) if value.len() <= MAX_VALUE_BYTES => {
            Ok(value.to_vec())
        }
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}
struct TransactionView<'a>(&'a Connection);
impl TransactionDocuments for TransactionView<'_> {
    fn get(&self, key: &str) -> Result<Option<Document>, StorageError> {
        recovery::check_retired_command(self.0, key)?;
        self.0
            .query_row(
                "SELECT value,revision FROM documents WHERE key=?1",
                [key],
                |row| {
                    Ok(Document {
                        value: read_limited_blob(row, 0)?,
                        revision: Revision::new(row.get(1)?),
                    })
                },
            )
            .optional()
            .map_err(|_| StorageError::Database)
    }
    fn profile_content_owner_receipt(
        &self,
        admission: &fc_domain::profile_content::ContentAdmission,
    ) -> Result<Option<(String, Document)>, StorageError> {
        let key = command_window::profile_content_owner_key(self.0, admission)?;
        Ok(self.get(&key)?.map(|document| (key, document)))
    }
    fn revision(&self) -> Result<Revision, StorageError> {
        read_revision(self.0)
    }
    fn scan_prefix(
        &self,
        prefix: &str,
        after_key: Option<&str>,
        limit: usize,
    ) -> Result<DocumentPage, StorageError> {
        if prefix.is_empty()
            || prefix.len() > 128
            || !prefix.bytes().all(|b| (0x20..=0x7e).contains(&b))
            || limit == 0
            || limit > 128
            || after_key.is_some_and(|key| key.len() > 256 || !key.starts_with(prefix))
        {
            return Err(StorageError::InvalidCommand);
        }
        let mut upper = prefix.as_bytes().to_vec();
        *upper.last_mut().ok_or(StorageError::InvalidCommand)? += 1;
        let upper = String::from_utf8(upper).map_err(|_| StorageError::InvalidCommand)?;
        let mut statement=self.0.prepare("SELECT key,value,revision FROM documents WHERE key>=?1 AND key<?2 AND key>?3 ORDER BY key LIMIT ?4").map_err(|_|StorageError::Database)?;
        let mut rows = statement
            .query(params![prefix, upper, after_key.unwrap_or(""), limit + 1])
            .map_err(|_| StorageError::Database)?;
        let mut entries = Vec::with_capacity(limit);
        let mut owned = 0usize;
        let mut more = false;
        while let Some(row) = rows.next().map_err(|_| StorageError::Database)? {
            let key = match row.get_ref(0).map_err(|_| StorageError::Database)? {
                rusqlite::types::ValueRef::Text(bytes)
                    if !bytes.is_empty() && bytes.len() <= 256 =>
                {
                    std::str::from_utf8(bytes).map_err(|_| StorageError::Database)?
                }
                _ => return Err(StorageError::Database),
            };
            let value = match row.get_ref(1).map_err(|_| StorageError::Database)? {
                rusqlite::types::ValueRef::Blob(bytes) if bytes.len() <= MAX_VALUE_BYTES => bytes,
                _ => return Err(StorageError::Database),
            };
            let bytes = key
                .len()
                .checked_add(value.len())
                .ok_or(StorageError::Database)?;
            if entries.len() == limit || bytes > 512 * 1024 - owned {
                more = true;
                break;
            }
            let key = key.to_owned();
            let value = value.to_vec();
            owned = owned
                .checked_add(key.capacity() + value.capacity())
                .filter(|total| *total <= 512 * 1024)
                .ok_or(StorageError::Database)?;
            entries.push((
                key,
                Document {
                    value,
                    revision: Revision::new(row.get(2).map_err(|_| StorageError::Database)?),
                },
            ));
        }
        let next_key = if more {
            Some(entries.last().ok_or(StorageError::Database)?.0.clone())
        } else {
            None
        };
        Ok(DocumentPage { entries, next_key })
    }
}

struct CommandView<'a> {
    inner: TransactionView<'a>,
    issued: fc_ports::command_window::IssuedCommand,
    history: Option<&'a typed_command::StoredTyped>,
}
impl TransactionDocuments for CommandView<'_> {
    fn typed_history(&self) -> Option<&typed_command::StoredTyped> {
        self.history
    }
    fn active_command(&self) -> Option<fc_ports::command_window::IssuedCommand> {
        Some(self.issued)
    }

    fn profile_content_owner_receipt(
        &self,
        admission: &fc_domain::profile_content::ContentAdmission,
    ) -> Result<Option<(String, Document)>, StorageError> {
        self.inner.profile_content_owner_receipt(admission)
    }
    fn revision(&self) -> Result<Revision, StorageError> {
        self.inner.revision()
    }
    fn get(&self, key: &str) -> Result<Option<Document>, StorageError> {
        if let Some((slot, id)) = business::command_window::legacy(key) {
            if id != self.issued.command_id || slot.family != self.issued.family {
                return Err(StorageError::CommandIdReused);
            }
            let key = business::command_window::namespaced(slot, self.issued.ticket);
            command_window::check_document(self.inner.0, self.issued, &key)?;
            return self.inner.get(&key);
        }
        self.inner.get(key)
    }
    fn scan_prefix(
        &self,
        prefix: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<DocumentPage, StorageError> {
        self.inner.scan_prefix(prefix, after, limit)
    }
}

type Reply<T> = mpsc::Sender<Result<T, StorageError>>;
enum Message<S> {
    MigrationInspect(
        recovery::InspectJob,
        Reply<recovery::MigrationSourceReceipt>,
    ),
    MigrationMap(
        Box<recovery::MapJob>,
        Reply<recovery::MappedOrdinaryReceipt>,
    ),
    RecoveryBarrier(
        Box<dyn TransactionTask<S>>,
        Option<fc_ports::command_window::IssuedCommand>,
        Reply<recovery::RecoveryBarrier>,
    ),
    Recovery(recovery::Job, Reply<recovery::RecoveryDatabaseReceipt>),
    Login(login::Operation, Reply<login::Response>),
    IssueCommand(
        command_window::IssueCommand,
        Reply<fc_ports::command_window::IssuedCommand>,
    ),
    CheckCommand(fc_ports::command_window::IssuedCommand, Reply<()>),
    Transaction(
        Box<dyn TransactionTask<S>>,
        Option<fc_ports::command_window::IssuedCommand>,
        Reply<Vec<u8>>,
    ),
    Commit(
        CommitCommand,
        S,
        Option<fc_ports::command_window::IssuedCommand>,
        Reply<CommitReceipt>,
    ),
    Get(String, Reply<Option<Document>>),
    Revision(Reply<Revision>),
    Backup(PathBuf, Reply<BackupReceipt>),
}

impl<S> Message<S> {
    fn queued_bytes(&self) -> Option<usize> {
        let allocations = match self {
            Self::MigrationInspect(job, _) => job.allocation_bytes(),
            Self::MigrationMap(job, _) => job.allocation_bytes(),
            Self::RecoveryBarrier(task, _, _) => task.allocation_bytes(),
            Self::Recovery(job, _) => job.allocation_bytes(),
            Self::Login(_, _) => std::mem::size_of::<login::Operation>(),
            Self::IssueCommand(_, _) => std::mem::size_of::<command_window::IssueCommand>(),
            Self::CheckCommand(_, _) => {
                std::mem::size_of::<fc_ports::command_window::IssuedCommand>()
            }
            Self::Transaction(task, _, _) => task.allocation_bytes(),
            Self::Commit(command, _, _, _) => command.allocation_bytes()?,
            Self::Get(key, _) => key.capacity(),
            Self::Revision(_) => 0,
            Self::Backup(path, _) => path.capacity(),
        };
        std::mem::size_of::<Envelope<S>>().checked_add(allocations)
    }
}

struct QueueLease {
    used: Arc<AtomicUsize>,
    bytes: usize,
}
impl QueueLease {
    fn reserve(used: Arc<AtomicUsize>, bytes: usize) -> Result<Self, StorageError> {
        used.try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current
                .checked_add(bytes)
                .filter(|next| *next <= MAX_QUEUED_BYTES)
        })
        .map_err(|_| StorageError::Busy)?;
        Ok(Self { used, bytes })
    }
}
impl Drop for QueueLease {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
struct Envelope<S> {
    message: Message<S>,
    lease: QueueLease,
}

pub struct StorageHandle<S> {
    sender: mpsc::SyncSender<Envelope<S>>,
    stopping: Arc<AtomicBool>,
    queued_bytes: Arc<AtomicUsize>,
    transaction_execution: Option<TransactionExecutionFence>,
    command_window: Option<fc_ports::command_window::IssuedCommand>,
}
impl<S> Clone for StorageHandle<S> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            stopping: self.stopping.clone(),
            queued_bytes: self.queued_bytes.clone(),
            transaction_execution: self.transaction_execution.clone(),
            command_window: self.command_window,
        }
    }
}
impl<S> StorageHandle<S> {
    fn send<T>(
        &self,
        message: impl FnOnce(Reply<T>) -> Message<S>,
    ) -> Result<Ticket<T>, StorageError> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(StorageError::Stopping);
        }
        let (reply, receiver) = mpsc::channel();
        let message = message(reply);
        let bytes = message.queued_bytes().ok_or(StorageError::Busy)?;
        let lease = QueueLease::reserve(self.queued_bytes.clone(), bytes)?;
        match self.sender.try_send(Envelope { message, lease }) {
            Ok(()) => Ok(Ticket(receiver, None)),
            Err(mpsc::TrySendError::Full(_)) => Err(StorageError::Busy),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(StorageError::WorkerDisconnected),
        }
    }
    /// Reserve before scheduling a KDF. Hold the opaque reservation inside the
    /// real worker until it exits; a timeout never frees its physical CPU slot.
    pub fn try_login_admission(
        &self,
        principal: fc_domain::ids::PrincipalId,
        kind: login::CredentialKind,
    ) -> Result<Ticket<login::Response>, StorageError> {
        self.send(|reply| Message::Login(login::Operation::Admit(principal, kind), reply))
    }
    /// Invoke only after the owning KDF worker has finished. No session may be
    /// issued until this version-checked, durable result reports Authenticated.
    pub fn try_finish_login(
        &self,
        reservation: login::Reservation,
        outcome: login::Verification,
    ) -> Result<Ticket<login::Response>, StorageError> {
        self.send(|reply| Message::Login(login::Operation::Finish(reservation, outcome), reply))
    }
    /// Issue on the sole Writer. The backend must obtain entropy from its
    /// CSPRNG and bind the current trusted actor/family before calling this.
    pub fn try_issue_command(
        &self,
        request: command_window::IssueCommand,
    ) -> Result<Ticket<fc_ports::command_window::IssuedCommand>, StorageError> {
        self.send(|reply| Message::IssueCommand(request, reply))
    }
    pub fn try_check_command(
        &self,
        issued: fc_ports::command_window::IssuedCommand,
    ) -> Result<Ticket<()>, StorageError> {
        self.send(|reply| Message::CheckCommand(issued, reply))
    }
    pub fn try_transaction(
        &self,
        task: impl TransactionTask<S> + 'static,
    ) -> Result<Ticket<Vec<u8>>, StorageError>
    where
        S: 'static,
    {
        let task: Box<dyn TransactionTask<S>> = if let Some(fence) = &self.transaction_execution {
            Box::new(FencedTransaction {
                task: Box::new(task),
                fence: fence.clone(),
            })
        } else {
            Box::new(task)
        };
        let mut ticket =
            self.send(|reply| Message::Transaction(task, self.command_window, reply))?;
        ticket.1 = self.transaction_execution.clone();
        Ok(ticket)
    }
    /// Bind an issuance to one immutable repository/request clone. This does not
    /// grant authority: the Writer checks its ledger in the business transaction.
    pub fn with_command_window(&self, issued: fc_ports::command_window::IssuedCommand) -> Self {
        let mut scoped = self.clone();
        scoped.command_window = Some(issued);
        scoped
    }
    /// Create a request-scoped clone. Repeated binding can only tighten an
    /// existing fence. Generic commits/login/recovery and the base handle retain
    /// their existing semantics; only try_transaction carries this fence.
    pub fn with_transaction_execution_fence(&self, fence: TransactionExecutionFence) -> Self {
        let mut scoped = self.clone();
        scoped.transaction_execution = Some(match &self.transaction_execution {
            Some(previous) => {
                let previous = previous.clone();
                Arc::new(move || {
                    check_transaction_execution(&previous).is_ok()
                        && check_transaction_execution(&fence).is_ok()
                })
            }
            None => fence,
        });
        scoped
    }
    pub fn try_commit(
        &self,
        command: CommitCommand,
        prepared_snapshot: S,
    ) -> Result<Ticket<CommitReceipt>, StorageError> {
        self.send(|reply| Message::Commit(command, prepared_snapshot, self.command_window, reply))
    }
    pub fn try_get(&self, key: String) -> Result<Ticket<Option<Document>>, StorageError> {
        if key.is_empty() || key.len() > 256 {
            return Err(StorageError::InvalidCommand);
        }
        self.send(|reply| Message::Get(key, reply))
    }
    pub fn try_revision(&self) -> Result<Ticket<Revision>, StorageError> {
        self.send(Message::Revision)
    }
    /// Full local recovery backup, including opaque credentials if stored here.
    /// The trusted caller must choose a controlled directory and enforce local
    /// administrator authorization. This is not an ordinary settings export.
    /// The destination must not exist; existing files are never overwritten.
    pub fn try_backup(&self, destination: PathBuf) -> Result<Ticket<BackupReceipt>, StorageError> {
        if destination.as_os_str().is_empty() || !destination.is_absolute() {
            return Err(StorageError::InvalidCommand);
        }
        self.send(|reply| Message::Backup(destination, reply))
    }
    /// Owned command allocations and inline descriptors awaiting execution.
    /// Snapshot S must contain small values or shared immutable Arc references;
    /// allocations behind shared snapshots belong to the authority snapshot budget.
    pub fn queued_bytes(&self) -> usize {
        self.queued_bytes.load(Ordering::Acquire)
    }
    /// Independent control plane: never competes for a data queue slot.
    pub fn request_stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }
}

pub struct StorageWriter<S> {
    handle: StorageHandle<S>,
    worker: Option<JoinHandle<()>>,
}
impl<S: Send + 'static> StorageWriter<S> {
    /// Startup is blocking. The connection itself is opened inside the new thread.
    pub fn open<P>(
        path: impl AsRef<Path>,
        capacity: usize,
        publisher: P,
    ) -> Result<Self, StorageError>
    where
        P: AuthorityPublisher<Snapshot = S> + Send + 'static,
    {
        Self::open_with_login_clock(
            path,
            capacity,
            publisher,
            Arc::new(login::ProcessClock::new()),
        )
    }
    pub fn open_with_login_clock<P>(
        path: impl AsRef<Path>,
        capacity: usize,
        publisher: P,
        clock: Arc<dyn fc_ports::MonotonicClock + Send + Sync>,
    ) -> Result<Self, StorageError>
    where
        P: AuthorityPublisher<Snapshot = S> + Send + 'static,
    {
        let window_clock = Arc::new(command_window::ProcessClock {
            monotonic: clock.clone(),
        });
        Self::open_with_clocks(path, capacity, publisher, clock, window_clock)
    }
    /// Trusted composition/test clock injection. Remote callers cannot provide a
    /// clock sample. Both command and login state still use this one Writer.
    pub fn open_with_clocks<P>(
        path: impl AsRef<Path>,
        capacity: usize,
        publisher: P,
        clock: Arc<dyn fc_ports::MonotonicClock + Send + Sync>,
        window_clock: Arc<dyn command_window::WindowClock>,
    ) -> Result<Self, StorageError>
    where
        P: AuthorityPublisher<Snapshot = S> + Send + 'static,
    {
        if capacity == 0 || capacity > 128 {
            return Err(StorageError::InvalidCommand);
        }
        let path = path.as_ref().to_owned();
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let (ready, startup) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let queued_bytes = Arc::new(AtomicUsize::new(0));
        let stop = stopping.clone();
        let worker = thread::Builder::new()
            .name("fc-sqlite-writer".into())
            .spawn(move || {
                writer_loop(path, publisher, receiver, stop, ready, clock, window_clock);
            })
            .map_err(|_| StorageError::WorkerDisconnected)?;
        match startup.recv() {
            Ok(Ok(())) => Ok(Self {
                handle: StorageHandle {
                    sender,
                    stopping,
                    queued_bytes,
                    transaction_execution: None,
                    command_window: None,
                },
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(StorageError::WorkerDisconnected)
            }
        }
    }
    pub fn handle(&self) -> StorageHandle<S> {
        self.handle.clone()
    }
    /// Execute on the shutdown coordinator, never the UI thread.
    pub fn shutdown(mut self) -> Result<(), StorageError> {
        self.handle.request_stop();
        self.worker
            .take()
            .expect("worker exists until shutdown")
            .join()
            .map_err(|_| StorageError::WorkerPanicked)
    }
}
impl<S> Drop for StorageWriter<S> {
    fn drop(&mut self) {
        // Drop does not block a UI thread. Explicit shutdown is required to join;
        // this flag still guarantees the worker closes its connection promptly.
        self.handle.request_stop();
    }
}

fn open_connection(path: &Path) -> Result<Connection, StorageError> {
    let existed = path.try_exists().map_err(|_| StorageError::Database)?;
    // An existing zeroed/truncated/unversioned file is recovery evidence, never
    // a new installation. Only a path absent at startup may initialize schema 0.
    let mut db = Connection::open(path).map_err(|_| StorageError::Database)?;
    db.busy_timeout(Duration::from_millis(250))
        .map_err(|_| StorageError::Database)?;
    db.pragma_update(None, "foreign_keys", "ON")
        .map_err(|_| StorageError::Database)?;
    db.pragma_update(None, "journal_mode", "WAL")
        .map_err(|_| StorageError::Database)?;
    db.pragma_update(None, "synchronous", "FULL")
        .map_err(|_| StorageError::Database)?;
    let schema: i64 = db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| StorageError::Database)?;
    if schema > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema);
    }
    if schema == 0 {
        if existed {
            return Err(StorageError::Database);
        }
        let transaction = db.transaction().map_err(|_| StorageError::Database)?;
        transaction.execute_batch("CREATE TABLE state_meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL CHECK(revision>=0));
            INSERT INTO state_meta VALUES (1,0);
            CREATE TABLE documents (key TEXT PRIMARY KEY, value BLOB NOT NULL, revision INTEGER NOT NULL CHECK(revision>=1));
            CREATE TABLE command_journal (command_id TEXT PRIMARY KEY, fingerprint BLOB NOT NULL CHECK(length(fingerprint)=32), revision INTEGER NOT NULL CHECK(revision>=1));
            PRAGMA user_version=1;").map_err(|_| StorageError::Database)?;
        transaction.commit().map_err(|_| StorageError::Database)?;
    }
    // Validate the expected schema eagerly; corrupt/missing metadata fails startup.
    read_revision(&db)?;
    db.prepare("SELECT key,value,revision FROM documents LIMIT 0")
        .map_err(|_| StorageError::Database)?;
    db.prepare("SELECT command_id,fingerprint,revision FROM command_journal LIMIT 0")
        .map_err(|_| StorageError::Database)?;
    Ok(db)
}

fn read_revision(db: &Connection) -> Result<Revision, StorageError> {
    let value: i64 = db
        .query_row(
            "SELECT revision FROM state_meta WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(|_| StorageError::Database)?;
    u64::try_from(value)
        .map(Revision::new)
        .map_err(|_| StorageError::Database)
}

fn writer_loop<P: AuthorityPublisher>(
    path: PathBuf,
    mut publisher: P,
    receiver: mpsc::Receiver<Envelope<P::Snapshot>>,
    stop: Arc<AtomicBool>,
    ready: Reply<()>,
    clock: Arc<dyn fc_ports::MonotonicClock + Send + Sync>,
    window_clock: Arc<dyn command_window::WindowClock>,
) {
    let mut db = match open_connection(&path) {
        Ok(db) => db,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut window = match command_window::Window::open(&mut db, window_clock) {
        Ok(window) => window,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let mut login = login::Limiter::new(clock);
    let mut faulted = false;
    while !stop.load(Ordering::Acquire) {
        let Envelope { message, lease } = match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        // Queue ownership ends at pop. In-flight work is bounded to this one writer.
        drop(lease);
        match message {
            Message::Login(operation, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else if stop.load(Ordering::Acquire) {
                    Err(StorageError::Stopping)
                } else {
                    match catch_unwind(AssertUnwindSafe(|| {
                        login.execute(&mut db, operation, &stop)
                    })) {
                        Ok(result) => result,
                        Err(_) => {
                            mark_fault(&mut publisher, &mut faulted);
                            Err(StorageError::WorkerPanicked)
                        }
                    }
                };
                if matches!(result, Err(StorageError::Indeterminate)) {
                    mark_fault(&mut publisher, &mut faulted);
                }
                let _ = reply.send(result);
            }
            Message::IssueCommand(request, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else if stop.load(Ordering::Acquire) {
                    Err(StorageError::Stopping)
                } else {
                    window.issue(&mut db, request)
                };
                let _ = reply.send(result);
            }
            Message::CheckCommand(issued, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else if stop.load(Ordering::Acquire) {
                    Err(StorageError::Stopping)
                } else {
                    window
                        .maintain(&mut db)
                        .and_then(|()| window.check(&db, issued))
                };
                let _ = reply.send(result);
            }
            Message::Transaction(task, issued, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else {
                    match catch_unwind(AssertUnwindSafe(|| {
                        let original_completion = task.window_action();
                        match window.maintain(&mut db) {
                            Err(StorageError::CommandWindowClock)
                                if issued.is_none()
                                    && task.lifetime() == TransactionLifetime::OwnerReturn => {}
                            other => other?,
                        }
                        let result = execute_transaction(
                            &mut db,
                            &mut publisher,
                            task,
                            issued,
                            &window,
                            &stop,
                            &mut faulted,
                        );
                        if !faulted {
                            if let (Err(error), Some(action)) =
                                (&result, original_completion.as_ref())
                            {
                                if let Err(record_error) =
                                    typed_command::record_known_failure(&mut db, action, error)
                                {
                                    if record_error == StorageError::Indeterminate {
                                        mark_fault(&mut publisher, &mut faulted);
                                    }
                                    return Err(record_error);
                                }
                            }
                        }
                        result
                    })) {
                        Ok(result) => result,
                        Err(_) => {
                            mark_fault(&mut publisher, &mut faulted);
                            Err(StorageError::WorkerPanicked)
                        }
                    }
                };
                let _ = reply.send(result);
            }
            Message::Commit(command, snapshot, issued, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else {
                    window.maintain(&mut db).and_then(|()| {
                        execute_commit(
                            &mut db,
                            &mut publisher,
                            command,
                            snapshot,
                            issued,
                            &window,
                            &mut faulted,
                        )
                    })
                };
                let _ = reply.send(result);
            }
            Message::Get(key, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else {
                    db.query_row(
                        "SELECT value, revision FROM documents WHERE key=?1",
                        [key],
                        |row| {
                            let revision: u64 = row.get(1)?;
                            Ok(Document {
                                value: read_limited_blob(row, 0)?,
                                revision: Revision::new(revision),
                            })
                        },
                    )
                    .optional()
                    .map_err(|_| StorageError::Database)
                };
                let _ = reply.send(result);
            }
            Message::Revision(reply) => {
                let _ = reply.send(if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else {
                    read_revision(&db)
                });
            }
            Message::Backup(destination, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else {
                    backup_database(&db, &destination, &stop)
                };
                let _ = reply.send(result);
            }
            Message::MigrationInspect(job, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else {
                    match catch_unwind(AssertUnwindSafe(|| {
                        recovery::migration::inspect(&db, job, &stop)
                    })) {
                        Ok(result) => result,
                        Err(_) => Err(StorageError::WorkerPanicked),
                    }
                };
                let _ = reply.send(result);
            }
            Message::MigrationMap(job, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else {
                    match catch_unwind(AssertUnwindSafe(|| {
                        recovery::migration::map(&db, *job, &stop)
                    })) {
                        Ok(result) => result,
                        Err(_) => Err(StorageError::WorkerPanicked),
                    }
                };
                let _ = reply.send(result);
            }
            Message::Recovery(job, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else {
                    match catch_unwind(AssertUnwindSafe(|| recovery::execute(&mut db, job, &stop)))
                    {
                        Ok(result) => result,
                        Err(_) => Err(StorageError::WorkerPanicked),
                    }
                };
                let _ = reply.send(result);
            }
            Message::RecoveryBarrier(task, issued, reply) => {
                let result = if faulted {
                    Err(StorageError::AuthorityUnavailable)
                } else {
                    match catch_unwind(AssertUnwindSafe(|| {
                        window.maintain(&mut db)?;
                        execute_transaction(
                            &mut db,
                            &mut publisher,
                            task,
                            issued,
                            &window,
                            &stop,
                            &mut faulted,
                        )
                    })) {
                        Ok(result) => result.and_then(recovery::decode_barrier),
                        Err(_) => {
                            mark_fault(&mut publisher, &mut faulted);
                            Err(StorageError::WorkerPanicked)
                        }
                    }
                };
                let _ = reply.send(result);
            }
        }
    }
    // Drain only already bounded descriptors and return explicit cancellation.
    for Envelope { message, lease } in receiver.try_iter() {
        drop(lease);
        match message {
            Message::MigrationInspect(_, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::MigrationMap(_, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::RecoveryBarrier(_, _, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::Recovery(_, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::Login(_, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::IssueCommand(_, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::CheckCommand(_, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::Transaction(_, _, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::Commit(_, _, _, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::Get(_, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::Revision(reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
            Message::Backup(_, reply) => {
                let _ = reply.send(Err(StorageError::Stopping));
            }
        }
    }
}

/// Owns only a file created with create_new. A partial backup is removed after
/// SQLite has dropped its connection; success explicitly transfers ownership.
struct BackupFile {
    path: PathBuf,
    finished: bool,
}
impl Drop for BackupFile {
    fn drop(&mut self) {
        if !self.finished {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn backup_database(
    db: &Connection,
    destination: &Path,
    stop: &AtomicBool,
) -> Result<BackupReceipt, StorageError> {
    if !db.is_autocommit() {
        return Err(StorageError::Database);
    }
    // Never replace an existing database, symlink, or the source database.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                StorageError::BackupDestinationExists
            } else {
                StorageError::Database
            }
        })?;
    drop(file);
    let mut owned = BackupFile {
        path: destination.to_owned(),
        finished: false,
    };
    let mut output = Connection::open(destination).map_err(|_| StorageError::Database)?;
    output
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|_| StorageError::Database)?;
    {
        let backup =
            rusqlite::backup::Backup::new(db, &mut output).map_err(|_| StorageError::Database)?;
        let started = Instant::now();
        loop {
            if stop.load(Ordering::Acquire) {
                return Err(StorageError::Stopping);
            }
            if started.elapsed() >= Duration::from_secs(30) {
                return Err(StorageError::BackupTimedOut);
            }
            match backup.step(64).map_err(|_| StorageError::Database)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => {}
                rusqlite::backup::StepResult::Busy | rusqlite::backup::StepResult::Locked => {
                    thread::sleep(Duration::from_millis(5))
                }
                _ => return Err(StorageError::Database),
            }
        }
    }
    business::voice::sanitize_backup(&mut output)?;
    let revision = read_revision(&output)?;
    let schema_version = output
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| StorageError::Database)?;
    let integrity: String = output
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| StorageError::Database)?;
    if integrity != "ok" || schema_version != SCHEMA_VERSION {
        return Err(StorageError::Database);
    }
    // A portable recovery backup is a completed SQLite file, not a copied WAL
    // main file with live sidecars. Backup API supplied the coherent page image.
    output
        .pragma_update(None, "journal_mode", "DELETE")
        .map_err(|_| StorageError::Database)?;
    output.close().map_err(|_| StorageError::Database)?;
    owned.finished = true;
    Ok(BackupReceipt {
        revision,
        schema_version,
    })
}

fn mark_fault<P: AuthorityPublisher>(publisher: &mut P, faulted: &mut bool) {
    *faulted = true;
    // A misbehaving observer must not prevent the writer's own fail-closed flag.
    let _ = catch_unwind(AssertUnwindSafe(|| publisher.fault()));
}

/// Read authority, business rows and receipts under the very same IMMEDIATE
/// transaction that commits them. No precomputed mutation bypasses business CAS.
fn execute_transaction<P: AuthorityPublisher>(
    db: &mut Connection,
    publisher: &mut P,
    task: Box<dyn TransactionTask<P::Snapshot>>,
    issued: Option<fc_ports::command_window::IssuedCommand>,
    window: &command_window::Window,
    stop: &AtomicBool,
    faulted: &mut bool,
) -> Result<Vec<u8>, StorageError> {
    if stop.load(Ordering::Acquire) {
        return Err(StorageError::Stopping);
    }
    let transaction = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| StorageError::Database)?;
    let action = task.window_action();
    let issued = match action.as_ref().and_then(|a| a.original) {
        Some(original) => {
            if issued.is_some_and(|i| i != original) {
                return Err(StorageError::CommandIdReused);
            }
            Some(original)
        }
        None => issued,
    };
    let owner_return = action.as_ref().is_some_and(|a| a.owner_return());
    if let Some(issued) = issued {
        if !owner_return {
            window.check(&transaction, issued)?;
        }
    }
    let history = match (issued, action.as_ref()) {
        (Some(issued), Some(action)) => typed_command::read(&transaction, issued, action)?,
        (None, Some(_)) => return Err(StorageError::CommandNotIssued),
        _ => None,
    };
    let retirement = task.retirement();
    if let Some(family) = task.command_family() {
        if issued.is_none_or(|issued| issued.family != family) {
            return Err(StorageError::CommandNotIssued);
        }
    }
    let requirement = task.command_requirement();
    if let Some((family, id)) = requirement {
        match issued {
            Some(issued) if issued.family == family && issued.command_id == id => (),
            Some(_) => return Err(StorageError::CommandNotIssued),
            None => window.check_legacy(&transaction, family, id)?,
        }
    }
    if action.is_none() && requirement.is_some() {
        if let Some(issued) = issued {
            typed_command::raw_check(&transaction, issued)?;
        }
    }
    let current = read_revision(&transaction)?;
    let decision = if let Some(issued) = issued {
        task.evaluate(&CommandView {
            inner: TransactionView(&transaction),
            issued,
            history: history.as_ref(),
        })?
    } else {
        task.evaluate(&TransactionView(&transaction))?
    };
    let (outcome, typed_write) = if let (Some(issued), Some(action)) = (issued, action.as_ref()) {
        business::typed_commands::prepare_reply(
            &transaction,
            issued,
            action,
            history.as_ref(),
            &decision.outcome,
        )?
    } else {
        (decision.outcome, false)
    };
    let Some((mutations, snapshot)) = decision.commit else {
        if let Some(check) = decision.precommit {
            check()?;
        }
        if let Some(issued) = issued {
            if !owner_return {
                window.check(&transaction, issued)?;
            }
        }
        if issued.is_none() {
            if let Some((family, id)) = requirement {
                window.check_legacy(&transaction, family, id)?;
            }
        }
        if typed_write {
            if transaction.commit().is_err() {
                mark_fault(publisher, faulted);
                return Err(StorageError::Indeterminate);
            }
        } else {
            transaction.rollback().map_err(|_| StorageError::Database)?;
        }
        return Ok(outcome);
    };
    if stop.load(Ordering::Acquire) {
        return Err(StorageError::Stopping);
    }
    if action.is_none() && requirement.is_some() {
        if let Some(issued) = issued {
            typed_command::raw_claim(&transaction, issued)?;
        }
    }
    let next = current.next().map_err(|_| StorageError::InvalidCommand)?;
    let next_i64 = i64::try_from(next.get()).map_err(|_| StorageError::InvalidCommand)?;
    // Apply the exact same allocation/key/value bounds as generic writes.
    let mut validated = CommitCommand::new("transaction-validation".into(), current, mutations)?;
    command_window::register_profile_negative_receipts(&transaction, &validated.mutations)?;
    if let Some(issued) = issued {
        command_window::register_documents(&transaction, issued, &mut validated.mutations)?;
    }
    let changed_keys = validated
        .mutations
        .iter()
        .filter_map(|m| match m {
            Mutation::Put { key, .. } => Some(key.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    command_window::sync_owner_holds(&transaction, &validated.mutations, issued, next_i64)?;
    if publisher.snapshot_revision(&snapshot) != next {
        return Err(StorageError::InvalidCommand);
    }
    let update = match catch_unwind(AssertUnwindSafe(|| publisher.begin_update())) {
        Ok(Ok(update)) => update,
        Ok(Err(_)) => return Err(StorageError::AuthorityUnavailable),
        Err(_) => {
            mark_fault(publisher, faulted);
            return Err(StorageError::AuthorityUnavailable);
        }
    };
    let written = (|| -> rusqlite::Result<()> {
        for mutation in validated.mutations {
            match mutation {
                Mutation::Put { key, value } => {
                    transaction.execute("INSERT INTO documents(key,value,revision) VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=excluded.revision", params![key,value,next_i64])?;
                }
                Mutation::Delete { key } => {
                    transaction.execute("DELETE FROM documents WHERE key=?1", [key])?;
                }
            }
        }
        transaction.execute(
            "UPDATE state_meta SET revision=?1 WHERE singleton=1",
            [next_i64],
        )?;
        Ok(())
    })();
    let owner_checked = written
        .map_err(|_| StorageError::Database)
        .and_then(|()| command_window::sync_profile_content_owners(&transaction, &changed_keys));
    let checked = if let Err(error) = owner_checked {
        Err(error)
    } else if stop.load(Ordering::Acquire) {
        Err(StorageError::Stopping)
    } else {
        decision.precommit.map_or(Ok(()), |check| check())
    };
    let checked = checked.and_then(|()| match issued {
        Some(issued) if !owner_return => window.check(&transaction, issued),
        Some(_) => Ok(()),
        None => requirement.map_or(Ok(()), |(family, id)| {
            window.check_legacy(&transaction, family, id)
        }),
    });
    let checked = checked.and_then(|()| match retirement {
        CommandRetirement::None => Ok(()),
        _ => command_window::retire_epoch(&transaction, retirement, &[]),
    });
    if let Err(error) = checked {
        if transaction.rollback().is_err() {
            mark_fault(publisher, faulted);
            return Err(StorageError::Indeterminate);
        }
        return match catch_unwind(AssertUnwindSafe(|| publisher.rollback(update))) {
            Ok(Ok(())) => Err(error),
            _ => {
                mark_fault(publisher, faulted);
                Err(StorageError::AuthorityUnavailable)
            }
        };
    }
    if transaction.commit().is_err() {
        mark_fault(publisher, faulted);
        return Err(StorageError::Indeterminate);
    }
    match catch_unwind(AssertUnwindSafe(|| publisher.publish(update, snapshot))) {
        Ok(Ok(())) => Ok(outcome),
        _ => {
            mark_fault(publisher, faulted);
            Err(StorageError::PublicationFailed {
                committed_revision: next,
            })
        }
    }
}

fn execute_commit<P: AuthorityPublisher>(
    db: &mut Connection,
    publisher: &mut P,
    mut command: CommitCommand,
    snapshot: P::Snapshot,
    issued: Option<fc_ports::command_window::IssuedCommand>,
    window: &command_window::Window,
    faulted: &mut bool,
) -> Result<CommitReceipt, StorageError> {
    if let Some(issued) = issued {
        window.check(db, issued)?;
        if issued.family != fc_ports::command_window::CommandFamily::Generic
            || command.id != issued.command_id.get().to_string()
        {
            return Err(StorageError::CommandIdReused);
        }
        typed_command::raw_check(db, issued)?;
        command.id = issued.ticket.to_string();
    } else {
        window.check_legacy_journal(db, &command.id)?;
    }
    let read_journal = if issued.is_some() {
        "SELECT fingerprint,revision FROM command_journal_v2 WHERE command_id=?1"
    } else {
        "SELECT fingerprint,revision FROM command_journal WHERE command_id=?1"
    };
    let existing: Option<(Vec<u8>, u64)> = db
        .query_row(read_journal, [&command.id], |r| {
            Ok((read_limited_blob(r, 0)?, r.get(1)?))
        })
        .optional()
        .map_err(|_| StorageError::Database)?;
    if let Some(issued) = issued {
        command_window::check_journal(db, issued, existing.is_some())?;
    }
    if let Some((fingerprint, revision)) = existing {
        return if fingerprint.as_slice() == command.fingerprint {
            Ok(CommitReceipt {
                revision: Revision::new(revision),
                replayed: true,
            })
        } else {
            Err(StorageError::CommandIdReused)
        };
    }
    let current = read_revision(db)?;
    if current != command.expected_revision {
        return Err(StorageError::Conflict { current });
    }
    let next = current
        .get()
        .checked_add(1)
        .and_then(|v| i64::try_from(v).ok())
        .ok_or(StorageError::InvalidCommand)?;
    if publisher.snapshot_revision(&snapshot) != Revision::new(next as u64) {
        return Err(StorageError::InvalidCommand);
    }
    let update = match catch_unwind(AssertUnwindSafe(|| publisher.begin_update())) {
        Ok(Ok(update)) => update,
        Ok(Err(_)) => return Err(StorageError::AuthorityUnavailable),
        Err(_) => {
            mark_fault(publisher, faulted);
            return Err(StorageError::AuthorityUnavailable);
        }
    };
    let transaction = match db.transaction_with_behavior(TransactionBehavior::Immediate) {
        Ok(transaction) => transaction,
        Err(_) => return rollback_authority(publisher, update, faulted, StorageError::Database),
    };
    if let Some(issued) = issued {
        if let Err(error) = typed_command::raw_claim(&transaction, issued)
            .and_then(|()| command_window::register_journal(&transaction, issued, &command.id))
        {
            if transaction.rollback().is_err() {
                mark_fault(publisher, faulted);
                return Err(StorageError::Indeterminate);
            }
            return rollback_authority(publisher, update, faulted, error);
        }
    }
    let written = (|| -> rusqlite::Result<()> {
        // Recheck under the SQLite write transaction, including another process.
        let actual: i64 = transaction.query_row(
            "SELECT revision FROM state_meta WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if actual != next - 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        for mutation in command.mutations {
            match mutation {
                Mutation::Put { key, value } => {
                    transaction.execute("INSERT INTO documents(key,value,revision) VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value, revision=excluded.revision", params![key, value, next])?;
                }
                Mutation::Delete { key } => {
                    transaction.execute("DELETE FROM documents WHERE key=?1", [key])?;
                }
            }
        }
        transaction.execute(
            "UPDATE state_meta SET revision=?1 WHERE singleton=1",
            [next],
        )?;
        transaction.execute(
            "INSERT INTO command_journal_v2(command_id,fingerprint,revision) VALUES (?1,?2,?3)",
            params![command.id, &command.fingerprint[..], next],
        )?;
        Ok(())
    })();
    if written.is_err() {
        if transaction.rollback().is_err() {
            mark_fault(publisher, faulted);
            return Err(StorageError::Indeterminate);
        }
        return rollback_authority(publisher, update, faulted, StorageError::Database);
    }
    if let Some(issued) = issued {
        if let Err(error) = window.check(&transaction, issued) {
            if transaction.rollback().is_err() {
                mark_fault(publisher, faulted);
                return Err(StorageError::Indeterminate);
            }
            return rollback_authority(publisher, update, faulted, error);
        }
    }
    // COMMIT error can be ambiguous. Conservatively fault rather than resume
    // serving an authority snapshot which may no longer match the database.
    if transaction.commit().is_err() {
        mark_fault(publisher, faulted);
        return Err(StorageError::Indeterminate);
    }
    let revision = Revision::new(next as u64);
    match catch_unwind(AssertUnwindSafe(|| publisher.publish(update, snapshot))) {
        Ok(Ok(())) => Ok(CommitReceipt {
            revision,
            replayed: false,
        }),
        _ => {
            mark_fault(publisher, faulted);
            Err(StorageError::PublicationFailed {
                committed_revision: revision,
            })
        }
    }
}

fn rollback_authority<P: AuthorityPublisher>(
    publisher: &mut P,
    update: P::Update,
    faulted: &mut bool,
    original: StorageError,
) -> Result<CommitReceipt, StorageError> {
    match catch_unwind(AssertUnwindSafe(|| publisher.rollback(update))) {
        Ok(Ok(())) => Err(original),
        _ => {
            mark_fault(publisher, faulted);
            Err(StorageError::AuthorityUnavailable)
        }
    }
}

pub fn sqlite_version() -> &'static str {
    rusqlite::version()
}

#[cfg(test)]
mod execution_fence_tests;
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicU64, Barrier, Mutex};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    struct TempDatabase(PathBuf, crate::fixture_commands::FixtureCommands);
    impl TempDatabase {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "fc-sqlite-test-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            Self(directory, Default::default())
        }
        fn path(&self) -> PathBuf {
            self.0.join("isolated.sqlite3")
        }
    }
    impl Drop for TempDatabase {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("isolated test database cleanup");
        }
    }
    #[derive(Default)]
    struct State {
        epoch: u64,
        published: u64,
        faulted: bool,
        rollback: u64,
    }
    enum PublishMode {
        Success,
        Error,
        Panic,
    }
    struct Publisher {
        state: Arc<Mutex<State>>,
        mode: PublishMode,
        block: Option<(Arc<Barrier>, Arc<Barrier>)>,
    }
    impl Publisher {
        fn new(mode: PublishMode) -> Self {
            Self {
                state: Arc::new(Mutex::new(State::default())),
                mode,
                block: None,
            }
        }
    }
    impl AuthorityPublisher for Publisher {
        type Snapshot = u64;
        type Update = ();
        type Error = ();
        fn snapshot_revision(&self, snapshot: &u64) -> Revision {
            Revision::new(*snapshot)
        }
        fn begin_update(&mut self) -> Result<(), ()> {
            self.state.lock().unwrap().epoch += 1;
            if let Some((entered, resume)) = self.block.take() {
                entered.wait();
                resume.wait();
            }
            Ok(())
        }
        fn publish(&mut self, _: (), snapshot: u64) -> Result<(), ()> {
            match self.mode {
                PublishMode::Success => {
                    self.state.lock().unwrap().published = snapshot;
                    Ok(())
                }
                PublishMode::Error => Err(()),
                PublishMode::Panic => panic!("injected authority publication failure"),
            }
        }
        fn rollback(&mut self, _: ()) -> Result<(), ()> {
            self.state.lock().unwrap().rollback += 1;
            Ok(())
        }
        fn fault(&mut self) {
            self.state.lock().unwrap().faulted = true;
        }
    }
    fn put(id: &str, revision: u64, key: &str, value: &[u8]) -> CommitCommand {
        CommitCommand::new(
            id.to_owned(),
            Revision::new(revision),
            vec![Mutation::Put {
                key: key.to_owned(),
                value: value.to_vec(),
            }],
        )
        .unwrap()
    }

    #[test]
    fn durable_reopen_cas_journal_and_cleanup() {
        let directory;
        {
            let temp = TempDatabase::new();
            directory = temp.0.clone();
            let writer =
                StorageWriter::open(temp.path(), 8, Publisher::new(PublishMode::Success)).unwrap();
            let handle = writer.handle();
            let receipt = temp
                .1
                .commit(&handle, put("command-1", 0, "homework", b"first"), 1)
                .unwrap()
                .wait()
                .unwrap();
            assert_eq!(receipt.revision, Revision::new(1));
            assert!(!receipt.replayed);
            assert!(
                matches!(temp.1.commit(&handle,put("stale", 0, "homework", b"bad"), 2).unwrap().wait(), Err(StorageError::Conflict { current }) if current == Revision::new(1))
            );
            writer.shutdown().unwrap();

            let writer =
                StorageWriter::open(temp.path(), 8, Publisher::new(PublishMode::Success)).unwrap();
            let handle = writer.handle();
            assert_eq!(
                handle
                    .try_get("homework".into())
                    .unwrap()
                    .wait()
                    .unwrap()
                    .unwrap()
                    .value,
                b"first"
            );
            let replay = temp
                .1
                .commit(&handle, put("command-1", 0, "homework", b"first"), 999)
                .unwrap()
                .wait()
                .unwrap();
            assert!(replay.replayed);
            assert_eq!(replay.revision, Revision::new(1));
            assert_eq!(
                temp.1
                    .commit(&handle, put("command-1", 0, "homework", b"different"), 2)
                    .unwrap()
                    .wait(),
                Err(StorageError::CommandIdReused)
            );
            assert_eq!(
                handle.try_revision().unwrap().wait().unwrap(),
                Revision::new(1)
            );
            assert_eq!(
                temp.1
                    .commit(&handle, put("wrong-snapshot", 1, "homework", b"bad"), 99)
                    .unwrap()
                    .wait(),
                Err(StorageError::InvalidCommand)
            );
            writer.shutdown().unwrap();
        }
        assert!(!directory.exists());
    }

    #[test]
    fn batch_sql_failure_rolls_back_data_journal_and_authority() {
        let temp = TempDatabase::new();
        let writer =
            StorageWriter::open(temp.path(), 8, Publisher::new(PublishMode::Success)).unwrap();
        writer.shutdown().unwrap();
        {
            let db = Connection::open(temp.path()).unwrap();
            db.execute_batch("CREATE TRIGGER reject_bad BEFORE INSERT ON documents WHEN NEW.key='fail' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        }
        let publisher = Publisher::new(PublishMode::Success);
        let state = publisher.state.clone();
        let writer = StorageWriter::open(temp.path(), 8, publisher).unwrap();
        let handle = writer.handle();
        let command = CommitCommand::new(
            "failed".into(),
            Revision::new(0),
            vec![
                Mutation::Put {
                    key: "good".into(),
                    value: b"uncommitted".to_vec(),
                },
                Mutation::Put {
                    key: "fail".into(),
                    value: vec![],
                },
            ],
        )
        .unwrap();
        assert_eq!(
            temp.1.commit(&handle, command, 1).unwrap().wait(),
            Err(StorageError::Database)
        );
        assert_eq!(
            handle.try_revision().unwrap().wait().unwrap(),
            Revision::new(0)
        );
        assert_eq!(handle.try_get("good".into()).unwrap().wait().unwrap(), None);
        let status = state.lock().unwrap();
        assert_eq!(status.epoch, 1);
        assert_eq!(status.rollback, 1);
        assert!(!status.faulted);
        drop(status);
        // Failure did not retain the command ID, so a valid explicit retry can commit.
        assert!(temp
            .1
            .commit(&handle, put("failed", 0, "good", b"committed"), 1)
            .unwrap()
            .wait()
            .is_ok());
        writer.shutdown().unwrap();
    }

    fn verify_publication_failure(mode: PublishMode) {
        let temp = TempDatabase::new();
        let publisher = Publisher::new(mode);
        let state = publisher.state.clone();
        let writer = StorageWriter::open(temp.path(), 8, publisher).unwrap();
        let handle = writer.handle();
        // Both requests are issued while the original authority is healthy.
        // The later retry must test the original faulted Writer, not issuance.
        for label in ["committed", "next"] {
            temp.1
                .issue(
                    &handle,
                    fc_ports::command_window::CommandFamily::Generic,
                    label,
                    fc_ports::command_window::CommandOwner::PublicDesktop,
                )
                .unwrap();
        }
        assert_eq!(
            temp.1
                .commit(
                    &handle,
                    put("committed", 0, "protected", b"new authority"),
                    1
                )
                .unwrap()
                .wait(),
            Err(StorageError::PublicationFailed {
                committed_revision: Revision::new(1)
            })
        );
        assert!(state.lock().unwrap().faulted);
        assert_eq!(
            handle.try_get("protected".into()).unwrap().wait(),
            Err(StorageError::AuthorityUnavailable)
        );
        assert_eq!(
            temp.1
                .commit(&handle, put("next", 1, "protected", b"must fail"), 2)
                .unwrap()
                .wait(),
            Err(StorageError::AuthorityUnavailable)
        );
        writer.shutdown().unwrap();
        // Controlled restart reloads the committed database, never reexecutes the write.
        let writer =
            StorageWriter::open(temp.path(), 8, Publisher::new(PublishMode::Success)).unwrap();
        let handle = writer.handle();
        assert_eq!(
            handle
                .try_get("protected".into())
                .unwrap()
                .wait()
                .unwrap()
                .unwrap()
                .value,
            b"new authority"
        );
        assert!(
            temp.1
                .commit(
                    &handle,
                    put("committed", 0, "protected", b"new authority"),
                    1
                )
                .unwrap()
                .wait()
                .unwrap()
                .replayed
        );
        writer.shutdown().unwrap();
    }

    #[test]
    fn committed_publication_error_fails_closed() {
        verify_publication_failure(PublishMode::Error);
    }
    #[test]
    fn committed_publication_panic_fails_closed() {
        verify_publication_failure(PublishMode::Panic);
    }

    #[test]
    fn full_queue_returns_busy_and_stop_has_independent_capacity() {
        let temp = TempDatabase::new();
        let entered = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let mut publisher = Publisher::new(PublishMode::Success);
        publisher.block = Some((entered.clone(), resume.clone()));
        let writer = StorageWriter::open(temp.path(), 1, publisher).unwrap();
        let handle = writer.handle();
        for label in ["active", "queued", "full"] {
            temp.1
                .issue(
                    &handle,
                    fc_ports::command_window::CommandFamily::Generic,
                    label,
                    fc_ports::command_window::CommandOwner::PublicDesktop,
                )
                .unwrap();
        }
        let active = temp
            .1
            .commit(&handle, put("active", 0, "one", b"one"), 1)
            .unwrap();
        entered.wait();
        let queued = temp
            .1
            .commit(&handle, put("queued", 1, "two", b"two"), 2)
            .unwrap();
        let reserved = handle.queued_bytes();
        assert!(matches!(
            temp.1.commit(&handle, put("full", 2, "three", b"three"), 3),
            Err(StorageError::Busy)
        ));
        assert_eq!(
            handle.queued_bytes(),
            reserved,
            "failed send must return its byte lease"
        );
        handle.request_stop();
        assert!(matches!(handle.try_revision(), Err(StorageError::Stopping)));
        resume.wait();
        assert!(active.wait().is_ok());
        assert_eq!(queued.wait(), Err(StorageError::Stopping));
        writer.shutdown().unwrap();
        assert_eq!(
            handle.queued_bytes(),
            0,
            "stop drain returns every byte lease"
        );
    }

    #[test]
    fn byte_budget_saturates_before_slot_count_and_has_no_retry_drift() {
        let temp = TempDatabase::new();
        let entered = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let mut publisher = Publisher::new(PublishMode::Success);
        publisher.block = Some((entered.clone(), resume.clone()));
        let writer = StorageWriter::open(temp.path(), 128, publisher).unwrap();
        let handle = writer.handle();
        for label in ["active", "large", "saturated"] {
            temp.1
                .issue(
                    &handle,
                    fc_ports::command_window::CommandFamily::Generic,
                    label,
                    fc_ports::command_window::CommandOwner::PublicDesktop,
                )
                .unwrap();
        }
        let active = temp
            .1
            .commit(&handle, put("active", 0, "one", b"one"), 1)
            .unwrap();
        entered.wait();
        assert_eq!(
            handle.queued_bytes(),
            0,
            "pop releases queue accounting before execution"
        );
        let large = |id: &str| {
            CommitCommand::new(
                id.into(),
                Revision::new(1),
                (0..60)
                    .map(|i| Mutation::Put {
                        key: format!("document-{i}"),
                        value: vec![1; MAX_VALUE_BYTES],
                    })
                    .collect(),
            )
            .unwrap()
        };
        let queued = temp.1.commit(&handle, large("large"), 2).unwrap();
        let reserved = handle.queued_bytes();
        assert!(reserved > 3 * 1024 * 1024 && reserved <= MAX_QUEUED_BYTES);
        for _ in 0..32 {
            assert!(matches!(
                temp.1.commit(&handle, large("saturated"), 2),
                Err(StorageError::Busy)
            ));
            assert_eq!(
                handle.queued_bytes(),
                reserved,
                "failed budget reservation cannot drift"
            );
        }
        let small = handle.try_revision().unwrap();
        assert!(handle.queued_bytes() > reserved);
        handle.request_stop();
        resume.wait();
        active.wait().unwrap();
        assert_eq!(queued.wait(), Err(StorageError::Stopping));
        assert_eq!(small.wait(), Err(StorageError::Stopping));
        writer.shutdown().unwrap();
        assert_eq!(handle.queued_bytes(), 0);
    }

    #[test]
    fn unused_vector_capacity_is_charged_to_the_ingress_budget() {
        let temp = TempDatabase::new();
        let writer =
            StorageWriter::open(temp.path(), 128, Publisher::new(PublishMode::Success)).unwrap();
        let mut value = Vec::with_capacity(MAX_QUEUED_BYTES);
        value.push(1);
        let command = CommitCommand::new(
            "overallocated".into(),
            Revision::new(0),
            vec![Mutation::Put {
                key: "tiny-value".into(),
                value,
            }],
        )
        .unwrap();
        assert!(matches!(
            temp.1.commit(&writer.handle(), command, 1),
            Err(StorageError::Busy)
        ));
        assert_eq!(writer.handle().queued_bytes(), 0);
        writer.shutdown().unwrap();
    }

    #[test]
    fn coherent_backup_reopens_with_original_journal_and_cas() {
        let temp = TempDatabase::new();
        let writer =
            StorageWriter::open(temp.path(), 8, Publisher::new(PublishMode::Success)).unwrap();
        let handle = writer.handle();
        temp.1
            .commit(&handle, put("first", 0, "homework", b"backup contents"), 1)
            .unwrap()
            .wait()
            .unwrap();
        let destination = temp.0.join("recovery.sqlite3");
        let receipt = handle
            .try_backup(destination.clone())
            .unwrap()
            .wait()
            .unwrap();
        assert_eq!(
            receipt,
            BackupReceipt {
                revision: Revision::new(1),
                schema_version: SCHEMA_VERSION
            }
        );
        assert_eq!(
            handle.try_backup(destination.clone()).unwrap().wait(),
            Err(StorageError::BackupDestinationExists)
        );
        assert_eq!(
            handle.try_backup(temp.path()).unwrap().wait(),
            Err(StorageError::BackupDestinationExists)
        );
        temp.1
            .commit(
                &handle,
                put("second", 1, "homework", b"later source data"),
                2,
            )
            .unwrap()
            .wait()
            .unwrap();
        assert_eq!(
            handle.try_revision().unwrap().wait().unwrap(),
            Revision::new(2)
        );

        let restored =
            StorageWriter::open(destination, 8, Publisher::new(PublishMode::Success)).unwrap();
        let restored_handle = restored.handle();
        assert_eq!(
            restored_handle.try_revision().unwrap().wait().unwrap(),
            Revision::new(1)
        );
        assert_eq!(
            restored_handle
                .try_get("homework".into())
                .unwrap()
                .wait()
                .unwrap()
                .unwrap()
                .value,
            b"backup contents"
        );
        assert!(
            temp.1
                .commit(
                    &restored_handle,
                    put("first", 0, "homework", b"backup contents"),
                    1
                )
                .unwrap()
                .wait()
                .unwrap()
                .replayed
        );
        assert_eq!(
            temp.1
                .commit(&restored_handle, put("first", 0, "homework", b"changed"), 1)
                .unwrap()
                .wait(),
            Err(StorageError::CommandIdReused)
        );
        assert!(
            matches!(temp.1.commit(&restored_handle,put("stale", 0, "homework", b"stale"), 1).unwrap().wait(), Err(StorageError::Conflict { current }) if current == Revision::new(1))
        );
        temp.1
            .commit(
                &restored_handle,
                put("new-after-restore", 1, "homework", b"explicit new write"),
                2,
            )
            .unwrap()
            .wait()
            .unwrap();
        restored.shutdown().unwrap();
        writer.shutdown().unwrap();
    }

    #[test]
    fn canceled_backup_removes_only_its_new_partial_file() {
        let temp = TempDatabase::new();
        let db = open_connection(&temp.path()).unwrap();
        let destination = temp.0.join("canceled.sqlite3");
        assert_eq!(
            backup_database(&db, &destination, &AtomicBool::new(true)),
            Err(StorageError::Stopping)
        );
        assert!(!destination.exists());
        let existing = temp.0.join("keep-existing.sqlite3");
        std::fs::write(&existing, b"must stay unchanged").unwrap();
        assert_eq!(
            backup_database(&db, &existing, &AtomicBool::new(false)),
            Err(StorageError::BackupDestinationExists)
        );
        assert_eq!(std::fs::read(&existing).unwrap(), b"must stay unchanged");
        drop(db);
    }

    #[test]
    fn future_schema_and_invalid_commands_are_rejected() {
        let temp = TempDatabase::new();
        {
            let db = Connection::open(temp.path()).unwrap();
            db.pragma_update(None, "user_version", 999).unwrap();
        }
        assert!(matches!(
            StorageWriter::open(temp.path(), 1, Publisher::new(PublishMode::Success)),
            Err(StorageError::UnsupportedSchema)
        ));
        assert!(CommitCommand::new("id".into(), Revision::new(0), vec![]).is_err());
        assert!(CommitCommand::new(
            "id".into(),
            Revision::new(0),
            vec![Mutation::Put {
                key: "a".into(),
                value: vec![0; MAX_VALUE_BYTES + 1]
            }]
        )
        .is_err());
    }
    #[test]
    fn oversized_database_blob_is_rejected_at_borrow_before_allocating_a_document() {
        let temp = TempDatabase::new();
        {
            let db = open_connection(&temp.path()).unwrap();
            db.execute(
                "INSERT INTO documents VALUES ('oversize',zeroblob(?1),1)",
                [(MAX_VALUE_BYTES + 1) as u64],
            )
            .unwrap();
            db.execute(
                "INSERT INTO documents VALUES ('exact',zeroblob(?1),1)",
                [MAX_VALUE_BYTES as u64],
            )
            .unwrap();
            assert_eq!(
                TransactionView(&db).get("oversize"),
                Err(StorageError::Database)
            );
            assert!(matches!(
                TransactionView(&db).scan_prefix("over", None, 128),
                Err(StorageError::Database)
            ));
            assert_eq!(
                TransactionView(&db)
                    .get("exact")
                    .unwrap()
                    .unwrap()
                    .value
                    .len(),
                MAX_VALUE_BYTES
            );
        }
        let writer =
            StorageWriter::open(temp.path(), 8, Publisher::new(PublishMode::Success)).unwrap();
        assert_eq!(
            writer.handle().try_get("oversize".into()).unwrap().wait(),
            Err(StorageError::Database)
        );
        assert_eq!(
            writer
                .handle()
                .try_get("exact".into())
                .unwrap()
                .wait()
                .unwrap()
                .unwrap()
                .value
                .len(),
            MAX_VALUE_BYTES
        );
        writer.shutdown().unwrap();
    }
    #[test]
    fn metadata_scan_short_byte_limited_pages_keep_explicit_continuation() {
        let temp = TempDatabase::new();
        let db = open_connection(&temp.path()).unwrap();
        for index in 0..10 {
            db.execute(
                "INSERT INTO documents VALUES (?1,zeroblob(?2),1)",
                params![format!("scan/{index:03}"), MAX_VALUE_BYTES as u64],
            )
            .unwrap();
        }
        let first = TransactionView(&db)
            .scan_prefix("scan/", None, 128)
            .unwrap();
        assert_eq!(first.entries.len(), 7);
        assert!(
            first
                .entries
                .iter()
                .map(|(key, d)| key.capacity() + d.value.capacity())
                .sum::<usize>()
                <= 512 * 1024
        );
        let second = TransactionView(&db)
            .scan_prefix("scan/", first.next_key.as_deref(), 128)
            .unwrap();
        assert_eq!(second.entries.len(), 3);
        assert!(second.next_key.is_none());
        assert_eq!(first.entries[6].0, "scan/006");
        assert_eq!(second.entries[0].0, "scan/007");
        assert!(matches!(
            TransactionView(&db).scan_prefix("scan/", Some("other"), 128),
            Err(StorageError::InvalidCommand)
        ));
    }
}
