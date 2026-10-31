//! Bounded worker-owned download pins. Public network handles are strictly Weak;
//! positive task replies contain metadata/bytes and never the native file owner.
use super::{ExecutionFence, TransferCancellation, TransferLifetime};
use crate::{
    backend::{web::WebService, Backend},
    web_application::{MaterialDownload, WebApplication, WebResponse},
};
use fc_ports::http::{RequestHead, RequestToken, ResponseHead, CHUNK_BYTES, MAX_REQUESTS};
use fc_runtime::worker::JobContext;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, Weak,
};

static OWNER: AtomicBool = AtomicBool::new(false);
static RETAINED: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadHubError {
    Busy,
    Closed,
    Cancelled,
    Invalid,
    Stale,
    Retained,
    SerialExhausted,
    ReadFailed,
}
pub enum DownloadHubFailure {
    Hub(DownloadHubError),
    Response(WebResponse),
}
impl From<DownloadHubError> for DownloadHubFailure {
    fn from(error: DownloadHubError) -> Self {
        Self::Hub(error)
    }
}
impl From<WebResponse> for DownloadHubFailure {
    fn from(response: WebResponse) -> Self {
        Self::Response(response)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadKey {
    slot: u8,
    serial: u64,
    token: RequestToken,
}
impl DownloadKey {
    pub fn token(&self) -> RequestToken {
        self.token
    }
}
/// The first full-file hash check has succeeded, and the first data chunk is
/// still cached with its worker owner. This reply cannot dispose of that pin.
pub struct DownloadAdmission {
    pub key: DownloadKey,
    pub head: ResponseHead,
    pub send_fence: ExecutionFence,
}
pub struct DownloadChunk {
    pub key: DownloadKey,
    pub sequence: u64,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub eof: bool,
}
/// These counts describe the registry. Only drain_after_join, after actual
/// shared-worker exit, can use them as final native-owner retirement evidence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DownloadHubCleanup {
    pub pending_slots: usize,
}
impl DownloadHubCleanup {
    pub fn is_drained(&self) -> bool {
        self.pending_slots == 0
    }
}
enum Stage {
    Starting,
    Live(Box<MaterialDownload>),
    Retained,
    Complete,
}
struct Entry {
    key: DownloadKey,
    life: TransferLifetime,
    stage: Stage,
    first: Option<Vec<u8>>,
    expected: u64,
    next_sequence: u64,
    next_offset: u64,
    failed: bool,
    eof: bool,
}
enum Slot {
    Vacant,
    Owned(Entry),
    Busy {
        key: DownloadKey,
        cancellation: TransferCancellation,
    },
}
struct Binding {
    service: WebService,
    application: Weak<()>,
}
struct Shared {
    accepting: AtomicBool,
    serial: AtomicU64,
    binding: Mutex<Option<Binding>>,
    slots: Mutex<[Slot; MAX_REQUESTS]>,
}
fn cancellation(life: &TransferLifetime) -> TransferCancellation {
    TransferCancellation {
        cancelled: Arc::downgrade(&life.cancelled),
    }
}
impl Shared {
    fn check_application(&self, app: &WebApplication) -> Result<(), DownloadHubError> {
        let service = app.binding();
        if service.has_execution_fence() {
            return Err(DownloadHubError::Invalid);
        }
        let application = app.upload_instance_key();
        let mut binding = self
            .binding
            .lock()
            .map_err(|_| DownloadHubError::Retained)?;
        match binding.as_ref() {
            Some(saved)
                if !saved.service.same_instance(&service)
                    || !Weak::ptr_eq(&saved.application, &application) =>
            {
                Err(DownloadHubError::Invalid)
            }
            Some(_) => Ok(()),
            None => {
                *binding = Some(Binding {
                    service,
                    application,
                });
                Ok(())
            }
        }
    }
    fn cancel_all(&self) {
        self.accepting.store(false, Ordering::Release);
        let slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        for slot in slots.iter() {
            match slot {
                Slot::Owned(entry) => cancellation(&entry.life).cancel(),
                Slot::Busy { cancellation, .. } => cancellation.cancel(),
                Slot::Vacant => {}
            }
        }
    }
    fn reserve(self: &Arc<Self>, life: TransferLifetime) -> Result<Lease, DownloadHubError> {
        let mut slots = self.slots.try_lock().map_err(|_| DownloadHubError::Busy)?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err(DownloadHubError::Closed);
        }
        if slots.iter().any(|slot| match slot {
            Slot::Owned(entry) => entry.key.token == life.token(),
            Slot::Busy { key, .. } => key.token == life.token(),
            Slot::Vacant => false,
        }) {
            return Err(DownloadHubError::Busy);
        }
        let slot = slots
            .iter()
            .position(|slot| matches!(slot, Slot::Vacant))
            .ok_or(DownloadHubError::Busy)?;
        let serial = self
            .serial
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| {
                self.accepting.store(false, Ordering::Release);
                DownloadHubError::SerialExhausted
            })?
            + 1;
        let key = DownloadKey {
            slot: slot as u8,
            serial,
            token: life.token(),
        };
        slots[slot] = Slot::Busy {
            key,
            cancellation: cancellation(&life),
        };
        Ok(Lease {
            shared: self.clone(),
            key,
            entry: Some(Entry {
                key,
                life,
                stage: Stage::Starting,
                first: None,
                expected: 0,
                next_sequence: 1,
                next_offset: 0,
                failed: false,
                eof: false,
            }),
        })
    }
    fn take(self: &Arc<Self>, key: DownloadKey) -> Result<Lease, DownloadHubError> {
        let mut slots = self.slots.try_lock().map_err(|_| DownloadHubError::Busy)?;
        let slot = slots
            .get_mut(usize::from(key.slot))
            .ok_or(DownloadHubError::Stale)?;
        let cancel = match slot {
            Slot::Owned(entry) if entry.key == key => cancellation(&entry.life),
            Slot::Busy { key: saved, .. } if *saved == key => return Err(DownloadHubError::Busy),
            _ => return Err(DownloadHubError::Stale),
        };
        let Slot::Owned(entry) = std::mem::replace(
            slot,
            Slot::Busy {
                key,
                cancellation: cancel,
            },
        ) else {
            unreachable!()
        };
        Ok(Lease {
            shared: self.clone(),
            key,
            entry: Some(entry),
        })
    }
    fn key_for_request(
        &self,
        token: RequestToken,
    ) -> Result<Option<DownloadKey>, DownloadHubError> {
        let slots = self.slots.try_lock().map_err(|_| DownloadHubError::Busy)?;
        for slot in slots.iter() {
            match slot {
                Slot::Owned(entry) if entry.key.token == token => return Ok(Some(entry.key)),
                Slot::Busy { key, .. } if key.token == token => return Err(DownloadHubError::Busy),
                _ => {}
            }
        }
        Ok(None)
    }
    fn pending(&self) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter(|slot| !matches!(slot, Slot::Vacant))
            .count()
    }
}
struct Lease {
    shared: Arc<Shared>,
    key: DownloadKey,
    entry: Option<Entry>,
}
impl Lease {
    fn entry(&mut self) -> &mut Entry {
        self.entry.as_mut().expect("download lease owner")
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let Some(mut entry) = self.entry.take() else {
            return;
        };
        if std::thread::panicking() {
            self.shared.accepting.store(false, Ordering::Release);
            cancellation(&entry.life).cancel();
            // Native pin disposal occurs on the unwinding worker, before its
            // ticket completion; retain a closed-domain tombstone after that.
            drop(std::mem::replace(&mut entry.stage, Stage::Retained));
            entry.first.take();
            entry.failed = true;
        }
        let complete = matches!(entry.stage, Stage::Complete);
        let mut slots = self
            .shared
            .slots
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let slot = &mut slots[usize::from(self.key.slot)];
        assert!(
            matches!(slot, Slot::Busy { key, .. } if *key == self.key),
            "exact download lease"
        );
        if complete {
            *slot = Slot::Vacant;
            drop(slots);
            drop(entry);
        } else {
            *slot = Slot::Owned(entry);
        }
    }
}
pub struct DownloadHubOwner {
    shared: Option<Arc<Shared>>,
    drained: bool,
}
#[derive(Clone)]
pub struct DownloadHub {
    shared: Weak<Shared>,
}
impl DownloadHubOwner {
    #[allow(clippy::new_ret_no_self)] // Sole strong owner plus a strictly weak public handle.
    pub fn new() -> Result<(Self, DownloadHub), DownloadHubError> {
        OWNER
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| DownloadHubError::Busy)?;
        let shared = Arc::new(Shared {
            accepting: AtomicBool::new(true),
            serial: AtomicU64::new(0),
            binding: Mutex::new(None),
            slots: Mutex::new(std::array::from_fn(|_| Slot::Vacant)),
        });
        let hub = DownloadHub {
            shared: Arc::downgrade(&shared),
        };
        Ok((
            Self {
                shared: Some(shared),
                drained: false,
            },
            hub,
        ))
    }
    pub fn handle(&self) -> DownloadHub {
        DownloadHub {
            shared: Arc::downgrade(self.shared.as_ref().expect("live download owner")),
        }
    }
    /// Final coordinator only. Recovery never reopens old admission.
    pub fn recover_retained() -> Option<Self> {
        RETAINED
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .map(|shared| Self {
                shared: Some(shared),
                drained: false,
            })
    }
    /// MUST follow actual shared-pool join. No synthetic/expired JobContext is
    /// used to close files. A Busy lease prevents successful final retirement.
    pub fn drain_after_join(&mut self) -> DownloadHubCleanup {
        let shared = self.shared.as_ref().expect("live download owner");
        shared.cancel_all();
        let keys: [Option<DownloadKey>; MAX_REQUESTS] = {
            let slots = shared
                .slots
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::array::from_fn(|index| match &slots[index] {
                Slot::Owned(entry) => Some(entry.key),
                _ => None,
            })
        };
        for key in keys.into_iter().flatten() {
            if let Ok(mut lease) = shared.take(key) {
                close_entry(lease.entry());
            }
        }
        let status = DownloadHubCleanup {
            pending_slots: shared.pending(),
        };
        self.drained = status.is_drained();
        status
    }
}
impl Drop for DownloadHubOwner {
    fn drop(&mut self) {
        let Some(shared) = self.shared.take() else {
            return;
        };
        shared.cancel_all();
        if self.drained && shared.pending() == 0 {
            drop(shared);
            OWNER.store(false, Ordering::Release);
        } else {
            let mut retained = RETAINED.lock().unwrap_or_else(|error| error.into_inner());
            assert!(retained.is_none(), "one original retained download domain");
            *retained = Some(shared);
        }
    }
}
fn worker_shared(
    hub: &DownloadHub,
    context: &JobContext,
    positive: bool,
) -> Result<Arc<Shared>, DownloadHubError> {
    context
        .checkpoint()
        .map_err(|_| DownloadHubError::Cancelled)?;
    let shared = hub.shared.upgrade().ok_or(DownloadHubError::Closed)?;
    if positive && !shared.accepting.load(Ordering::Acquire) {
        return Err(DownloadHubError::Closed);
    }
    Ok(shared)
}
fn active(entry: &Entry, context: &JobContext) -> Result<(), DownloadHubError> {
    if entry.failed
        || !entry.life.is_active()
        || context.checkpoint().is_err()
        || context.deadline() > entry.life.deadline()
    {
        return Err(DownloadHubError::Cancelled);
    }
    match &entry.stage {
        Stage::Live(owner) if (owner.send_fence())() => Ok(()),
        _ => Err(DownloadHubError::Cancelled),
    }
}
fn close_entry(entry: &mut Entry) {
    // No network descriptor can run this Drop: only an actual current worker
    // or the coordinator after actual worker join owns the exclusive lease.
    drop(std::mem::replace(&mut entry.stage, Stage::Retained));
    entry.first.take();
    entry.failed = true;
    entry.stage = Stage::Complete;
}
impl DownloadHub {
    pub fn begin(
        &self,
        app: &WebApplication,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        life: TransferLifetime,
        context: &JobContext,
    ) -> Result<DownloadAdmission, DownloadHubFailure> {
        if head.token != life.token() || !life.is_active() || context.deadline() > life.deadline() {
            return Err(DownloadHubError::Invalid.into());
        }
        let shared = worker_shared(self, context, true)?;
        shared.check_application(app)?;
        let mut lease = shared.reserve(life)?;
        let entry = lease.entry();
        let owner = match app.begin_material_download_in_job(backend, head, &entry.life, context) {
            Ok(owner) => owner,
            Err(response) => {
                entry.stage = Stage::Complete;
                return Err(response.into());
            }
        };
        entry.stage = Stage::Live(Box::new(owner));
        let Stage::Live(owner) = &mut entry.stage else {
            unreachable!()
        };
        if owner.head().validate().is_err()
            || owner.head().status != 200
            || owner.head().content_length == 0
        {
            entry.failed = true;
            return Err(DownloadHubError::Invalid.into());
        }
        entry.expected = owner.head().content_length;
        // This call really verifies the entire bounded file hash before any
        // response head escapes. Its first bytes remain in this fixed registry.
        let first = match owner.read_chunk_in_job(&entry.life, context) {
            Ok(bytes) if !bytes.is_empty() && bytes.len() <= CHUNK_BYTES => bytes.to_vec(),
            _ => {
                entry.failed = true;
                return Err(DownloadHubError::ReadFailed.into());
            }
        };
        entry.first = Some(first);
        active(entry, context)?;
        let Stage::Live(owner) = &entry.stage else {
            unreachable!()
        };
        Ok(DownloadAdmission {
            key: entry.key,
            head: owner.head().clone(),
            send_fence: owner.send_fence(),
        })
    }
    pub fn read(
        &self,
        key: DownloadKey,
        sequence: u64,
        offset: u64,
        context: &JobContext,
    ) -> Result<DownloadChunk, DownloadHubFailure> {
        let shared = worker_shared(self, context, true)?;
        let mut lease = shared.take(key)?;
        let entry = lease.entry();
        active(entry, context)?;
        if sequence != entry.next_sequence || offset != entry.next_offset || entry.eof {
            entry.failed = true;
            return Err(DownloadHubError::Invalid.into());
        }
        let bytes = if let Some(first) = entry.first.take() {
            first
        } else {
            let Stage::Live(owner) = &mut entry.stage else {
                unreachable!()
            };
            match owner.read_chunk_in_job(&entry.life, context) {
                Ok(bytes) if !bytes.is_empty() && bytes.len() <= CHUNK_BYTES => bytes.to_vec(),
                _ => {
                    entry.failed = true;
                    return Err(DownloadHubError::ReadFailed.into());
                }
            }
        };
        active(entry, context)?;
        let next = offset
            .checked_add(bytes.len() as u64)
            .filter(|next| *next <= entry.expected)
            .ok_or(DownloadHubError::Invalid)?;
        entry.next_sequence = sequence
            .checked_add(1)
            .ok_or(DownloadHubError::SerialExhausted)?;
        entry.next_offset = next;
        entry.eof = next == entry.expected;
        Ok(DownloadChunk {
            key,
            sequence,
            offset,
            bytes,
            eof: entry.eof,
        })
    }
    /// Fresh worker-only negative cleanup, AFTER the original task is actually
    /// finished. Works without knowing a suppressed positive admission's key.
    /// Closing a pin ignores its expired original life; this current job must
    /// still be live. No publication, new stage or deadline renewal is allowed.
    pub fn cleanup_request(
        &self,
        token: RequestToken,
        context: &JobContext,
    ) -> Result<(), DownloadHubError> {
        let shared = worker_shared(self, context, false)?;
        let Some(key) = shared.key_for_request(token)? else {
            return Ok(());
        };
        let mut lease = shared.take(key)?;
        close_entry(lease.entry());
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests;
