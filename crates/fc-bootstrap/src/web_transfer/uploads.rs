//! One bounded worker-owned upload domain. Network descriptors hold Weak only;
//! no file, pin, proof or permit is carried by a job's positive reply.
use super::{
    TransferCancellation, TransferControl, TransferLifetime, UploadAck, UploadKind, VOICE_BUDGET,
};
use crate::{
    backend::{web::WebService, Backend},
    web_application::{
        quarantined_uploads, retry_quarantined_uploads, MaterialTransferBinding,
        ReceivingMaterialUpload, ReceivingWebVoiceUpload, WebApplication, WebResponse,
    },
};
use fc_ports::http::{RequestHead, RequestToken, CHUNK_BYTES};
use fc_runtime::worker::JobContext;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, Weak,
};
use std::time::Instant;

const SLOTS: usize = 2;
static OWNER: AtomicBool = AtomicBool::new(false);
// An abandoned owner remains bounded and cannot be replaced by another domain.
// It is recovered only by the final coordinator, never by a Net descriptor.
static RETAINED: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadHubError {
    Busy,
    Closed,
    Cancelled,
    Invalid,
    Stale,
    Retained,
    SerialExhausted,
}
pub enum UploadHubFailure {
    Hub(UploadHubError),
    Response(WebResponse),
}
impl From<UploadHubError> for UploadHubFailure {
    fn from(error: UploadHubError) -> Self {
        Self::Hub(error)
    }
}
impl From<WebResponse> for UploadHubFailure {
    fn from(response: WebResponse) -> Self {
        Self::Response(response)
    }
}
/// Correlation only. The opaque serial prevents a delayed old job from touching
/// a replacement; every application operation still rechecks live authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadKey {
    slot: u8,
    serial: u64,
    token: RequestToken,
    kind: UploadKind,
    upload_id: u64,
}
impl UploadKey {
    pub fn token(&self) -> RequestToken {
        self.token
    }
    pub fn kind(&self) -> UploadKind {
        self.kind
    }
    pub fn upload_id(&self) -> u64 {
        self.upload_id
    }
}
/// Memory-only admission facts. Dropping this does not cancel a Ready material
/// transfer; explicit transport failure uses its weak atomic cancellation handle.
pub struct UploadAdmission {
    pub key: UploadKey,
    pub expected_bytes: u64,
    pub deadline: Instant,
    pub cancellation: TransferCancellation,
}
/// Finished response plus, only after physical retirement, the ORIGINAL memory
/// control for its network response. Never a file, proof, pin or upload permit.
pub struct UploadFinished {
    pub response: WebResponse,
    pub network_control: Option<TransferControl>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Descriptor/quarantine counts. Full physical retirement also requires the
/// original shared workers to have actually joined, as drain_after_join does.
pub struct UploadHubCleanup {
    pub pending_slots: usize,
    pub quarantined: usize,
}
impl UploadHubCleanup {
    pub fn is_drained(&self) -> bool {
        self.pending_slots == 0 && self.quarantined == 0
    }
}
#[allow(clippy::large_enum_variant)] // Two inline physical owners, no per-call boxing.
enum Stage {
    Starting,
    Material(ReceivingMaterialUpload),
    Voice(ReceivingWebVoiceUpload),
    MaterialReady,
    // The worker unwound, or a consuming operation could not prove clean
    // retirement. Only explicit cleanup plus pending/quarantine checks clears it.
    Retained,
    Complete,
}
struct Entry {
    key: UploadKey,
    // A new authenticated Append request may consume the old Ready stage and
    // then fail before Backend admission. Only that successful preparation can
    // bind its complete request token to this original negative-cleanup owner.
    cleanup_alias: Option<RequestToken>,
    control: Option<TransferControl>,
    life: TransferLifetime,
    expected: u64,
    stage: Stage,
}
#[allow(clippy::large_enum_variant)] // Exactly two fixed descriptor/owner slots.
enum Slot {
    Vacant,
    Owned(Entry),
    Busy {
        key: UploadKey,
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
    slots: Mutex<[Slot; SLOTS]>,
}
impl Shared {
    fn check_application(&self, app: &WebApplication) -> Result<(), UploadHubError> {
        let service = app.binding();
        if service.has_execution_fence() {
            return Err(UploadHubError::Invalid);
        }
        let application = app.upload_instance_key();
        let mut binding = self.binding.lock().map_err(|_| UploadHubError::Retained)?;
        match binding.as_ref() {
            Some(saved)
                if !saved.service.same_instance(&service)
                    || !Weak::ptr_eq(&saved.application, &application) =>
            {
                Err(UploadHubError::Invalid)
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
        let slots = self.slots.lock().unwrap_or_else(|p| p.into_inner());
        for slot in slots.iter() {
            match slot {
                Slot::Owned(entry) => {
                    if let Some(control) = &entry.control {
                        control.cancel();
                    }
                }
                Slot::Busy { cancellation, .. } => cancellation.cancel(),
                Slot::Vacant => {}
            }
        }
        // A Busy entry's weak atomic still cancels the exact in-flight owner;
        // final drain also waits for the actual worker join before taking it back.
    }
    fn take(self: &Arc<Self>, key: UploadKey) -> Result<Lease, UploadHubError> {
        let mut slots = self.slots.try_lock().map_err(|_| UploadHubError::Busy)?;
        let slot = slots
            .get_mut(usize::from(key.slot))
            .ok_or(UploadHubError::Stale)?;
        match slot {
            Slot::Owned(entry) if entry.key == key => {}
            Slot::Busy { key: saved, .. } if *saved == key => return Err(UploadHubError::Busy),
            _ => return Err(UploadHubError::Stale),
        }
        let cancellation = match slot {
            Slot::Owned(entry) => entry
                .control
                .as_ref()
                .expect("live upload control")
                .cancellation_handle(),
            _ => unreachable!(),
        };
        let Slot::Owned(entry) = std::mem::replace(slot, Slot::Busy { key, cancellation }) else {
            unreachable!()
        };
        Ok(Lease {
            shared: self.clone(),
            reserved: key,
            entry: Some(entry),
        })
    }
    fn reserve(
        self: &Arc<Self>,
        control: TransferControl,
        life: TransferLifetime,
        kind: UploadKind,
        expected: u64,
    ) -> Result<Lease, UploadHubError> {
        let mut slots = self.slots.try_lock().map_err(|_| UploadHubError::Busy)?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err(UploadHubError::Closed);
        }
        if slots.iter().any(|slot| match slot {
            Slot::Owned(entry) => {
                entry.key.token == life.token() || entry.cleanup_alias == Some(life.token())
            }
            Slot::Busy { key, .. } => key.token == life.token(),
            Slot::Vacant => false,
        }) {
            return Err(UploadHubError::Busy);
        }
        let slot = slots
            .iter()
            .position(|s| matches!(s, Slot::Vacant))
            .ok_or(UploadHubError::Busy)?;
        let serial = self
            .serial
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .map_err(|_| {
                self.accepting.store(false, Ordering::Release);
                UploadHubError::SerialExhausted
            })?
            + 1;
        let key = UploadKey {
            slot: slot as u8,
            serial,
            token: life.token(),
            kind,
            upload_id: 0,
        };
        slots[slot] = Slot::Busy {
            key,
            cancellation: control.cancellation_handle(),
        };
        Ok(Lease {
            shared: self.clone(),
            reserved: key,
            entry: Some(Entry {
                key,
                cleanup_alias: None,
                control: Some(control),
                life,
                expected,
                stage: Stage::Starting,
            }),
        })
    }
    fn pending(&self) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|s| !matches!(s, Slot::Vacant))
            .count()
    }
    fn bind_cleanup_alias(
        &self,
        entry: &mut Entry,
        token: RequestToken,
    ) -> Result<(), UploadHubError> {
        if entry.cleanup_alias.is_some_and(|saved| saved != token) {
            return Err(UploadHubError::Busy);
        }
        let slots = self.slots.lock().map_err(|_| UploadHubError::Retained)?;
        if slots.iter().any(|slot| match slot {
            Slot::Owned(other) if other.key != entry.key => {
                other.key.token == token || other.cleanup_alias == Some(token)
            }
            Slot::Busy { key, .. } if *key != entry.key => key.token == token,
            _ => false,
        }) {
            return Err(UploadHubError::Busy);
        }
        entry.cleanup_alias = Some(token);
        Ok(())
    }
}
struct Lease {
    shared: Arc<Shared>,
    reserved: UploadKey,
    entry: Option<Entry>,
}
impl Lease {
    fn entry(&mut self) -> &mut Entry {
        self.entry.as_mut().expect("leased entry")
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let Some(mut entry) = self.entry.take() else {
            return;
        };
        if std::thread::panicking() {
            self.shared.accepting.store(false, Ordering::Release);
            if let Some(control) = &entry.control {
                control.cancel();
            }
            // Native disposal stays on the unwinding worker. Failed exact file
            // removal transfers the original physical permit to quarantine.
            drop(std::mem::replace(&mut entry.stage, Stage::Retained));
        }
        let complete = matches!(entry.stage, Stage::Complete);
        let mut slots = self.shared.slots.lock().unwrap_or_else(|p| p.into_inner());
        let slot = &mut slots[usize::from(self.reserved.slot)];
        assert!(
            matches!(slot, Slot::Busy { key: saved, .. } if *saved == self.reserved),
            "exact upload lease owner"
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

pub struct UploadHubOwner {
    shared: Option<Arc<Shared>>,
    drained: bool,
}
#[derive(Clone)]
pub struct UploadHub {
    shared: Weak<Shared>,
}
impl UploadHubOwner {
    /// Keep this owner and the original base WebApplication alive through the
    /// actual shared-pool join and explicit drain. A replacement is forbidden
    /// while an abandoned or incompletely drained domain remains retained.
    #[allow(clippy::new_ret_no_self)] // Construct the sole owner and its strictly weak handle together.
    pub fn new() -> Result<(Self, UploadHub), UploadHubError> {
        OWNER
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| UploadHubError::Busy)?;
        let shared = Arc::new(Shared {
            accepting: AtomicBool::new(true),
            serial: AtomicU64::new(0),
            binding: Mutex::new(None),
            slots: Mutex::new(std::array::from_fn(|_| Slot::Vacant)),
        });
        let handle = UploadHub {
            shared: Arc::downgrade(&shared),
        };
        Ok((
            Self {
                shared: Some(shared),
                drained: false,
            },
            handle,
        ))
    }
    /// Final coordinator only, after the original shared worker threads joined.
    /// Recovering preserves the closed domain; it never reopens old admission.
    pub fn recover_retained() -> Option<Self> {
        RETAINED
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .map(|shared| Self {
                shared: Some(shared),
                drained: false,
            })
    }
    pub fn handle(&self) -> UploadHub {
        UploadHub {
            shared: Arc::downgrade(self.shared.as_ref().expect("live upload owner")),
        }
    }
    /// MUST run on the final coordinator after actual shared-pool join. No old
    /// JobContext is fabricated or reused. Busy/failed cleanup leaves the owner
    /// retained and forbids a replacement domain.
    pub fn drain_after_join(&mut self, app: &WebApplication) -> UploadHubCleanup {
        let shared = self.shared.as_ref().expect("live upload owner");
        shared.cancel_all();
        if shared.check_application(app).is_ok() {
            let keys: [Option<UploadKey>; SLOTS] = {
                let slots = shared.slots.lock().unwrap_or_else(|p| p.into_inner());
                std::array::from_fn(|i| match &slots[i] {
                    Slot::Owned(entry) => Some(entry.key),
                    _ => None,
                })
            };
            for key in keys.into_iter().flatten() {
                if let Ok(mut lease) = shared.take(key) {
                    cleanup_entry(lease.entry(), app);
                }
            }
            // Residue retries may perform real IO; the Hub metadata lock is not held.
            retry_quarantined_uploads();
            let _ = app.sweep_material_uploads();
            // A just-completed quarantine retry may now prove a tombstone clean.
            let keys = {
                let slots = shared.slots.lock().unwrap_or_else(|p| p.into_inner());
                std::array::from_fn::<_, SLOTS, _>(|i| match &slots[i] {
                    Slot::Owned(entry) => Some(entry.key),
                    _ => None,
                })
            };
            for key in keys.into_iter().flatten() {
                if let Ok(mut lease) = shared.take(key) {
                    cleanup_entry(lease.entry(), app);
                }
            }
        }
        let status = UploadHubCleanup {
            pending_slots: shared.pending(),
            quarantined: quarantined_uploads(),
        };
        self.drained = status.is_drained();
        status
    }
}
impl Drop for UploadHubOwner {
    fn drop(&mut self) {
        let Some(shared) = self.shared.take() else {
            return;
        };
        shared.cancel_all();
        if self.drained && shared.pending() == 0 && quarantined_uploads() == 0 {
            drop(shared);
            OWNER.store(false, Ordering::Release);
        } else {
            let mut retained = RETAINED.lock().unwrap_or_else(|p| p.into_inner());
            assert!(retained.is_none(), "one original upload domain");
            *retained = Some(shared);
        }
    }
}

fn worker_shared(
    hub: &UploadHub,
    context: &JobContext,
    positive: bool,
) -> Result<Arc<Shared>, UploadHubError> {
    context
        .checkpoint()
        .map_err(|_| UploadHubError::Cancelled)?;
    let shared = hub.shared.upgrade().ok_or(UploadHubError::Closed)?;
    if positive && !shared.accepting.load(Ordering::Acquire) {
        return Err(UploadHubError::Closed);
    }
    Ok(shared)
}
fn active_entry(entry: &Entry, context: &JobContext) -> Result<(), UploadHubError> {
    if !entry.life.is_active() || context.checkpoint().is_err() {
        Err(UploadHubError::Cancelled)
    } else {
        Ok(())
    }
}
fn finished_response(entry: &mut Entry, response: WebResponse) -> UploadFinished {
    let network_control = if matches!(entry.stage, Stage::Complete) {
        entry.control.take()
    } else {
        None
    };
    UploadFinished {
        response,
        network_control,
    }
}
fn cleanup_entry(entry: &mut Entry, app: &WebApplication) -> bool {
    if let Some(control) = &entry.control {
        control.cancel();
    }
    let cleaned = match &mut entry.stage {
        Stage::Material(owner) => owner.abort(),
        Stage::Voice(owner) => owner.abort(),
        Stage::MaterialReady => {
            app.discard_material_transfer_in_cleanup(entry.key.token, entry.key.upload_id)
                .is_ok()
                || !app.material_transfer_pending(entry.key.token, entry.key.upload_id)
        }
        Stage::Retained if entry.key.kind == UploadKind::Material && entry.key.upload_id != 0 => {
            match app.material_transfer_binding(entry.key.upload_id, &entry.life) {
                MaterialTransferBinding::Absent
                | MaterialTransferBinding::Unbound
                | MaterialTransferBinding::BoundElsewhere => true,
                MaterialTransferBinding::Busy => false,
                MaterialTransferBinding::Exact => {
                    // Only this exact complete token AND original cancellation
                    // cell can have been transferred here. Never cancel an
                    // unauthenticated Waiting descriptor or another stream.
                    let _ = app
                        .discard_material_transfer_in_cleanup(entry.key.token, entry.key.upload_id);
                    let _ = app.sweep_material_uploads();
                    matches!(
                        app.material_transfer_binding(entry.key.upload_id, &entry.life),
                        MaterialTransferBinding::Absent
                            | MaterialTransferBinding::Unbound
                            | MaterialTransferBinding::BoundElsewhere
                    )
                }
            }
        }
        Stage::Retained => true,
        Stage::Starting | Stage::Complete => true,
    };
    if !cleaned {
        return false;
    }
    // Drop every owner held by this entry before descriptor reuse. An absent
    // App binding alone cannot prove another worker dropped its local Arc/pin:
    // that worker's original physical permit still enforces the shared budget,
    // and complete domain retirement still requires actual shared-pool join.
    drop(std::mem::replace(&mut entry.stage, Stage::Retained));
    if quarantined_uploads() != 0
        || (entry.key.kind == UploadKind::Material
            && entry.key.upload_id != 0
            && matches!(
                app.material_transfer_binding(entry.key.upload_id, &entry.life),
                MaterialTransferBinding::Exact | MaterialTransferBinding::Busy
            ))
    {
        return false;
    }
    entry.stage = Stage::Complete;
    true
}
fn material_absence_candidate(entry: &Entry) -> bool {
    entry.key.kind == UploadKind::Material
        && entry.key.upload_id != 0
        && matches!(entry.stage, Stage::MaterialReady | Stage::Retained)
}
fn reap_inactive(
    shared: &Arc<Shared>,
    app: &WebApplication,
    context: &JobContext,
) -> Result<(), UploadHubError> {
    // Snapshot at most two metadata-only candidates, including live lifetimes
    // whose Ready/Retained App descriptor may already have been cancelled or
    // consumed. Never query the App registry while holding the Hub mutex.
    let candidates: [Option<(UploadKey, TransferLifetime, bool)>; SLOTS] = {
        let slots = shared.slots.try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => UploadHubError::Busy,
            std::sync::TryLockError::Poisoned(_) => UploadHubError::Retained,
        })?;
        std::array::from_fn(|i| match &slots[i] {
            Slot::Owned(entry) if !entry.life.is_active() || material_absence_candidate(entry) => {
                Some((
                    entry.key,
                    entry.life.clone(),
                    material_absence_candidate(entry),
                ))
            }
            _ => None,
        })
    };
    for (key, life, absent_eligible) in candidates.into_iter().flatten() {
        context
            .checkpoint()
            .map_err(|_| UploadHubError::Cancelled)?;
        if life.is_active()
            && (!absent_eligible
                || app.material_transfer_binding(key.upload_id, &life)
                    != MaterialTransferBinding::Absent)
        {
            continue;
        }
        let mut lease = match shared.take(key) {
            Ok(lease) => lease,
            // A different worker may already be cleaning or have actually
            // retired the exact snapshot. Never touch its replacement serial.
            Err(UploadHubError::Busy | UploadHubError::Stale) => continue,
            Err(error) => return Err(error),
        };
        let entry = lease.entry();
        // The exact key may have changed stage since the snapshot. Recheck
        // under its exclusive lease, with no Hub mutex spanning this App view.
        if !entry.life.is_active()
            || (material_absence_candidate(entry)
                && app.material_transfer_binding(entry.key.upload_id, &entry.life)
                    == MaterialTransferBinding::Absent)
        {
            // A failed abort/drop keeps this original entry and physical
            // charge. It does not prevent an independently vacant slot from
            // being used, and never changes the original absolute deadline.
            cleanup_entry(entry, app);
        }
    }
    context.checkpoint().map_err(|_| UploadHubError::Cancelled)
}
impl UploadHub {
    /// Worker-only bounded maintenance, using a fresh current job. Cleans only
    /// cancelled/expired non-Busy entries or App-absent material Ready/Retained
    /// descriptors of this exact application binding. Counts alone are not a
    /// full physical-release proof; final domain drain still requires real join.
    pub fn reap_expired(
        &self,
        app: &WebApplication,
        context: &JobContext,
    ) -> Result<UploadHubCleanup, UploadHubError> {
        let shared = worker_shared(self, context, false)?;
        shared.check_application(app)?;
        reap_inactive(&shared, app, context)?;
        Ok(UploadHubCleanup {
            pending_slots: shared.pending(),
            quarantined: quarantined_uploads(),
        })
    }
    /// Worker-only correlation lookup. Includes original tokens on Busy stages
    /// and authenticated Append aliases on Owned stages. Neither a lookup nor
    /// None proves a queued/in-flight task finished: wait for its actual done
    /// before using a possibly newly-bound alias for negative cleanup.
    pub fn key_for_request(
        &self,
        token: RequestToken,
        context: &JobContext,
    ) -> Result<Option<UploadKey>, UploadHubError> {
        let shared = worker_shared(self, context, false)?;
        let key = {
            let slots = shared.slots.try_lock().map_err(|error| match error {
                std::sync::TryLockError::WouldBlock => UploadHubError::Busy,
                std::sync::TryLockError::Poisoned(_) => UploadHubError::Retained,
            })?;
            slots.iter().find_map(|slot| match slot {
                Slot::Owned(entry)
                    if entry.key.token == token || entry.cleanup_alias == Some(token) =>
                {
                    Some(entry.key)
                }
                Slot::Busy { key, .. } if key.token == token => Some(*key),
                _ => None,
            })
        };
        context
            .checkpoint()
            .map_err(|_| UploadHubError::Cancelled)?;
        Ok(key)
    }
    /// Worker-only metadata lookup for a new Append request. The numeric ID
    /// grants no lease, CSRF or scope authority; append_ready reauthenticates all
    /// of them. Do not suppress an expired Ready key: it still needs cleanup.
    pub fn ready_material_key(
        &self,
        upload_id: u64,
        context: &JobContext,
    ) -> Result<Option<UploadKey>, UploadHubError> {
        let shared = worker_shared(self, context, false)?;
        let key = {
            let slots = shared.slots.try_lock().map_err(|error| match error {
                std::sync::TryLockError::WouldBlock => UploadHubError::Busy,
                std::sync::TryLockError::Poisoned(_) => UploadHubError::Retained,
            })?;
            let mut found = None;
            for slot in slots.iter() {
                match slot {
                    Slot::Owned(entry)
                        if entry.key.kind == UploadKind::Material
                            && entry.key.upload_id == upload_id
                            && matches!(entry.stage, Stage::MaterialReady) =>
                    {
                        found = Some(entry.key);
                    }
                    Slot::Busy { key, .. }
                        if key.kind == UploadKind::Material && key.upload_id == upload_id =>
                    {
                        return Err(UploadHubError::Busy)
                    }
                    _ => {}
                }
            }
            found
        };
        context
            .checkpoint()
            .map_err(|_| UploadHubError::Cancelled)?;
        Ok(key)
    }
    /// Fresh negative cleanup after the original Begin task is actually done,
    /// even if its positive reply was suppressed and only the token is known.
    pub fn cleanup_request(
        &self,
        token: RequestToken,
        app: &WebApplication,
        context: &JobContext,
    ) -> Result<(), UploadHubError> {
        match self.key_for_request(token, context)? {
            Some(key) => self.cleanup(key, app, context),
            None => Ok(()),
        }
    }
    fn begin(
        &self,
        app: &WebApplication,
        head: &RequestHead,
        control: TransferControl,
        life: TransferLifetime,
        context: &JobContext,
        kind: UploadKind,
    ) -> Result<UploadAdmission, UploadHubFailure> {
        if head.token != life.token()
            || !Arc::ptr_eq(&control.cancelled, &life.cancelled)
            || !life.is_active()
        {
            return Err(UploadHubError::Invalid.into());
        }
        let life = if kind == UploadKind::Voice {
            life.bounded(VOICE_BUDGET)
                .map_err(|_| UploadHubError::Invalid)?
        } else {
            life
        };
        let expected = head
            .content_length
            .filter(|n| *n > 0)
            .ok_or(UploadHubError::Invalid)?;
        let shared = worker_shared(self, context, true)?;
        shared.check_application(app)?;
        reap_inactive(&shared, app, context)?;
        let mut lease = shared.reserve(control, life, kind, expected)?;
        let entry = lease.entry();
        if kind == UploadKind::Material {
            let hinted = match app.route_hint(head) {
                Ok(fc_protocol::web::Route::Materials {
                    route: fc_protocol::web::materials::MaterialRoute::UploadBytes { upload_id },
                }) => upload_id,
                _ => {
                    entry.stage = Stage::Complete;
                    return Err(UploadHubError::Invalid.into());
                }
            };
            // Correlation only: the exact lifecycle view below is what proves
            // whether begin actually authenticated and transferred this stage.
            entry.key.upload_id = hinted;
        }
        let result = match kind {
            UploadKind::Material => app
                .begin_material_upload(head, entry.life.clone(), context)
                .map(|owner| {
                    entry.key.upload_id = owner.upload_id();
                    entry.life.deadline = entry.life.deadline.min(owner.deadline());
                    entry.stage = Stage::Material(owner);
                }),
            UploadKind::Voice => app
                .begin_voice_upload_in_job(head, entry.life.clone(), context)
                .map(|owner| {
                    entry.key.upload_id = owner.upload_id();
                    entry.stage = Stage::Voice(owner);
                }),
        };
        if let Err(response) = result {
            let unowned = kind != UploadKind::Material
                || matches!(
                    app.material_transfer_binding(entry.key.upload_id, &entry.life),
                    MaterialTransferBinding::Absent
                        | MaterialTransferBinding::Unbound
                        | MaterialTransferBinding::BoundElsewhere
                );
            entry.stage = if unowned && quarantined_uploads() == 0 {
                Stage::Complete
            } else {
                Stage::Retained
            };
            return Err(response.into());
        }
        active_entry(entry, context)?;
        Ok(UploadAdmission {
            key: entry.key,
            expected_bytes: entry.expected,
            deadline: entry.life.deadline(),
            cancellation: entry
                .control
                .as_ref()
                .expect("admitted upload control")
                .cancellation_handle(),
        })
    }
    pub fn begin_material(
        &self,
        app: &WebApplication,
        head: &RequestHead,
        control: TransferControl,
        life: TransferLifetime,
        context: &JobContext,
    ) -> Result<UploadAdmission, UploadHubFailure> {
        self.begin(app, head, control, life, context, UploadKind::Material)
    }
    pub fn begin_voice(
        &self,
        app: &WebApplication,
        head: &RequestHead,
        control: TransferControl,
        life: TransferLifetime,
        context: &JobContext,
    ) -> Result<UploadAdmission, UploadHubFailure> {
        self.begin(app, head, control, life, context, UploadKind::Voice)
    }
    pub fn write(
        &self,
        key: UploadKey,
        sequence: u64,
        offset: u64,
        bytes: &[u8],
        context: &JobContext,
    ) -> Result<UploadAck, UploadHubFailure> {
        if bytes.is_empty() || bytes.len() > CHUNK_BYTES {
            return Err(UploadHubError::Invalid.into());
        }
        let shared = worker_shared(self, context, true)?;
        let mut lease = shared.take(key)?;
        let entry = lease.entry();
        active_entry(entry, context)?;
        let ack = match &mut entry.stage {
            Stage::Material(owner) => owner.write(key.token, sequence, offset, bytes, context),
            Stage::Voice(owner) => owner.write_chunk(key.token, sequence, offset, bytes, context),
            _ => return Err(UploadHubError::Invalid.into()),
        }?;
        active_entry(entry, context)?;
        Ok(ack)
    }
    pub fn finish(
        &self,
        key: UploadKey,
        app: &WebApplication,
        context: &JobContext,
    ) -> Result<UploadFinished, UploadHubFailure> {
        let shared = worker_shared(self, context, true)?;
        shared.check_application(app)?;
        let mut lease = shared.take(key)?;
        let entry = lease.entry();
        active_entry(entry, context)?;
        let stage = std::mem::replace(&mut entry.stage, Stage::Retained);
        let response = match stage {
            Stage::Material(owner) => {
                let response = owner.finish(key.token, context);
                if response.head.status == 200 {
                    entry.stage = Stage::MaterialReady;
                }
                response
            }
            Stage::Voice(owner) => {
                let response = match owner.finish(context) {
                    Ok(prepared) => {
                        let response = app.commit_voice_upload_in_job(prepared, context);
                        if quarantined_uploads() == 0 {
                            entry.stage = Stage::Complete;
                        }
                        response
                    }
                    // Consuming validation failure still needs a fresh explicit
                    // cleanup witness before the retained slot/control can go.
                    Err(response) => response,
                };
                // Publication attempts retain their final bytes for the existing
                // durable reconciliation. They are never unpublished cleanup.
                response
            }
            other => {
                entry.stage = other;
                return Err(UploadHubError::Invalid.into());
            }
        };
        Ok(finished_response(entry, response))
    }
    #[allow(clippy::too_many_arguments)] // All borrowed request facts stay in one worker call.
    pub fn append_ready(
        &self,
        key: UploadKey,
        app: &WebApplication,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        body: &[u8],
        new_life: &TransferLifetime,
        context: &JobContext,
    ) -> Result<WebResponse, UploadHubFailure> {
        if key.kind != UploadKind::Material
            || head.token != new_life.token()
            || !new_life.is_active()
            || context.deadline() > new_life.deadline()
        {
            return Err(UploadHubError::Invalid.into());
        }
        if body.len() > fc_protocol::MAX_JSON_BYTES
            || !fc_protocol::web::materials::append(body)
                .is_ok_and(|input| input.upload_id == key.upload_id)
        {
            return Err(UploadHubError::Invalid.into());
        }
        let shared = worker_shared(self, context, true)?;
        shared.check_application(app)?;
        let mut lease = shared.take(key)?;
        let entry = lease.entry();
        active_entry(entry, context)?;
        if !matches!(entry.stage, Stage::MaterialReady) {
            return Err(UploadHubError::Invalid.into());
        }
        // upload_id is correlation, never authorization. Existing preparation
        // parses JSON and checks the new cookie/CSRF plus the original exact
        // lease/scope, classes, immutable stage and original absolute lifetime.
        let prepared = app.prepare_material_append_with_context(head, body, context)?;
        let old_life = entry.life.clone();
        let new_life = new_life.clone();
        let job = context.execution_fence();
        // Preparation's slow validation is complete before taking Backend. On
        // contention, the local proof is disposed on this worker and the exact
        // tombstone remains for explicit cleanup/reconciliation.
        entry.stage = Stage::Retained;
        shared.bind_cleanup_alias(entry, head.token)?;
        let mut backend = backend.try_lock().map_err(|_| UploadHubError::Busy)?;
        let mut scoped = backend.begin_execution_scope(Arc::new(move || {
            old_life.is_active() && new_life.is_active() && job()
        }));
        // A prepared proof stays local to this same worker call through SQL.
        let response = app.commit_material_append_with_context(&mut scoped, prepared, context);
        if !app.material_transfer_pending(key.token, key.upload_id) && quarantined_uploads() == 0 {
            entry.stage = Stage::Complete;
        }
        Ok(response)
    }
    /// Fresh worker cleanup ignores the expired transfer lifetime. It can only
    /// retire this exact old owner, not authenticate or publish a new operation.
    pub fn cleanup(
        &self,
        key: UploadKey,
        app: &WebApplication,
        context: &JobContext,
    ) -> Result<(), UploadHubError> {
        let shared = worker_shared(self, context, false)?;
        shared.check_application(app)?;
        let mut lease = shared.take(key)?;
        if cleanup_entry(lease.entry(), app) {
            Ok(())
        } else {
            Err(UploadHubError::Retained)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
