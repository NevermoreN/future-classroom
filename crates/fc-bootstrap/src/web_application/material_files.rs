//! Controlled Windows file worker. This synchronous Read adapter accepts only
//! already-arrived, owned local File/Cursor bytes. A potentially blocking network
//! reader is NOT a deadline or a cancelled IOCP operation and must not be passed
//! here. HTTP event/staging scheduling remains a separate disabled-listener gate.
use super::*;
use crate::backend::BackendError;
use crate::web_transfer::{TransferLifetime, UploadAck, UploadKind, TRANSFER_BUDGET};
use fc_ports::http::RequestToken;
use fc_runtime::worker::JobContext;
#[cfg(test)]
#[path = "material_download_stream_tests.rs"]
mod download_stream_tests;
#[cfg(test)]
#[path = "material_stream_tests.rs"]
mod stream_tests;
use fc_application::identity::SessionLease;
use fc_domain::{ids::*, materials::*};
use fc_materials::office::{ContainerError, ValidationControl};
use fc_ports::{
    prepare_material_append, MaterialAppendMetadata, MaterialPreparation, MaterialRepositoryError,
    PreparedMaterialAppend,
};
use fc_protocol::web::materials::{self as wire, MaterialRoute};
use fc_windows::materials::{
    files::{ControlledFiles, LockedFile, NewFile},
    helper, image,
};
use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
const TTL: Duration = Duration::from_secs(300);
const STREAM_TIME: Duration = Duration::from_secs(120);
const CHUNK: usize = 65536;
fn io_error(_: impl std::fmt::Debug) -> io::Error {
    io::Error::other("controlled material file unavailable")
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "material authority expired",
    )
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "material validation failed")
}
fn unavailable() -> WebResponse {
    failure(503, responses::ErrorCode::ServiceUnavailable)
}
fn ext(kind: MaterialKind) -> &'static str {
    match kind {
        MaterialKind::Png => "png",
        MaterialKind::Jpeg => "jpg",
        MaterialKind::Bmp => "bmp",
        MaterialKind::Txt => "txt",
        MaterialKind::Pdf => "pdf",
        MaterialKind::Pptx => "pptx",
        MaterialKind::Docx => "docx",
        MaterialKind::Xlsx => "xlsx",
    }
}
fn path(
    files: &ControlledFiles,
    scope: MaterialScope,
    id: MaterialId,
    resource: &MaterialResource,
) -> io::Result<PathBuf> {
    let dir = files
        .directory(&[
            "teachers".into(),
            scope.teacher_id.get().to_string(),
            "subjects".into(),
            scope.subject_id.get().to_string(),
            id.get().to_string(),
        ])
        .map_err(io_error)?;
    Ok(dir.join(format!(
        "{}.{}",
        resource.version_id.get(),
        ext(resource.kind)
    )))
}
use super::shared_upload::{quarantine_cleanup, PhysicalUploadPermit};
enum Phase {
    Waiting(NewFile),
    Ready(LockedFile),
    Receiving,
    Consuming,
}
struct Slot {
    _permit: Option<PhysicalUploadPermit>,
    identity: (u32, u64),
    lease: SessionLease,
    scope: MaterialScope,
    material_id: MaterialId,
    version: ResourceVersionId,
    name: String,
    bytes: u64,
    expires: Instant,
    lifetime: Option<TransferLifetime>,
    phase: Phase,
    path: Option<PathBuf>,
    files: Arc<ControlledFiles>,
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.phase = Phase::Consuming;
        if let Some(path) = &self.path {
            if self
                .files
                .remove_object_expected(path, false, self.identity, |_| true)
                .is_err()
            {
                if let Some(permit) = self._permit.take() {
                    let files = self.files.clone();
                    let path = path.clone();
                    let identity = self.identity;
                    quarantine_cleanup(
                        permit,
                        Box::new(move || {
                            files
                                .remove_object_expected(&path, false, identity, |_| true)
                                .is_ok()
                        }),
                    );
                }
            }
        }
    }
}
pub(super) struct MaterialFiles {
    files: Arc<ControlledFiles>,
    slots: Vec<(u64, Arc<Mutex<Slot>>)>,
}
impl MaterialFiles {
    fn new(root: &Path) -> io::Result<Self> {
        let files = Arc::new(ControlledFiles::new(root).map_err(io_error)?);
        // This sibling namespace cannot be mistaken for the native adapter's
        // flat staging directory. Configure only after the deployment's ACL
        // read-back and exclusive instance lock, with no old workers running.
        let staging = files
            .directory(&["web-material-staging".into()])
            .map_err(io_error)?;
        let marker = staging.join(".future-classroom-owner");
        const OWNER: &[u8] = b"FUTURE_CLASSROOM_WEB_MATERIAL_STAGE_V1\n";
        let entries = files.entries(&staging, 3).map_err(io_error)?;
        if !entries.contains(&marker) {
            if !entries.is_empty() {
                return Err(denied());
            }
            let mut owned = files.create_new(&marker).map_err(io_error)?;
            owned.file_mut().write_all(OWNER)?;
            owned.finish().map_err(io_error)?;
        }
        let mut owned = LockedFile::open(&marker, OWNER.len() as u64).map_err(io_error)?;
        let mut magic = [0u8; OWNER.len()];
        if owned.len().map_err(io_error)? != OWNER.len() as u64 {
            return Err(denied());
        }
        owned.file_mut().read_exact(&mut magic[..OWNER.len()])?;
        if &magic[..OWNER.len()] != OWNER {
            return Err(denied());
        }
        // Validate the complete bounded namespace before deleting any crash
        // residue. No arbitrary school file or unexpected name is adopted.
        for path in entries.iter().filter(|p| **p != marker) {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(denied)?;
            let id = name
                .strip_prefix("web-material-")
                .and_then(|n| n.strip_suffix(".part"))
                .and_then(|n| n.parse::<u64>().ok())
                .filter(|id| *id != 0)
                .ok_or_else(denied)?;
            if name != format!("web-material-{id}.part") {
                return Err(denied());
            }
        }
        for path in entries.iter().filter(|p| **p != marker) {
            files.remove(path).map_err(io_error)?;
        }
        Ok(Self {
            files,
            slots: Vec::with_capacity(2),
        })
    }
    fn find(&self, id: u64) -> Option<Arc<Mutex<Slot>>> {
        self.slots
            .iter()
            .find(|(saved, _)| *saved == id)
            .map(|(_, slot)| slot.clone())
    }
}

fn remove_owned_slot(
    registry: &std::sync::Weak<Mutex<Option<MaterialFiles>>>,
    upload_id: u64,
    owner: &Arc<Mutex<Slot>>,
) {
    if let Some(registry) = registry.upgrade() {
        if let Ok(mut registry) = registry.lock() {
            if let Some(files) = registry.as_mut() {
                // The caller still owns this Arc, so no file/permit destructor
                // can run while the registry lock is held.
                files
                    .slots
                    .retain(|(id, slot)| *id != upload_id || !Arc::ptr_eq(slot, owner));
            }
        }
    }
}

/// Worker-only stage owner. Correlation ACKs, never this owner or its file,
/// cross back to the network domain. Each call uses its current worker ticket.
pub struct ReceivingMaterialUpload {
    file: Option<NewFile>,
    sealed: Option<LockedFile>,
    owner: Arc<Mutex<Slot>>,
    registry: std::sync::Weak<Mutex<Option<MaterialFiles>>>,
    service: WebService,
    lease: SessionLease,
    scope: MaterialScope,
    expected: u64,
    lifetime: TransferLifetime,
    expires: Instant,
    upload_id: u64,
    written: u64,
    next_sequence: u64,
    failed: bool,
    finished: bool,
}
impl ReceivingMaterialUpload {
    pub fn token(&self) -> RequestToken {
        self.lifetime.token()
    }
    pub fn upload_id(&self) -> u64 {
        self.upload_id
    }
    pub fn deadline(&self) -> Instant {
        self.expires
    }
    pub fn offset(&self) -> u64 {
        self.written
    }

    /// Negative cleanup on the owning worker/final coordinator. A failed
    /// deletion retains this exact owner and original charge for another call.
    /// Even success does not release that charge until the caller drops self.
    pub fn abort(&mut self) -> bool {
        if self.finished {
            return true;
        }
        self.failed = true;
        let (files, path, identity) = {
            let mut slot = match self.owner.try_lock() {
                Ok(slot) => slot,
                Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => return false,
            };
            if slot
                .lifetime
                .as_ref()
                .is_none_or(|life| life.token() != self.token())
                || !matches!(slot.phase, Phase::Receiving | Phase::Consuming)
            {
                return false;
            }
            let Some(path) = slot.path.clone() else {
                return false;
            };
            slot.phase = Phase::Consuming;
            (slot.files.clone(), path, slot.identity)
        };
        drop(self.file.take());
        drop(self.sealed.take());
        if files
            .remove_object_expected(&path, false, identity, |_| true)
            .is_err()
        {
            return false;
        }
        {
            let mut slot = match self.owner.try_lock() {
                Ok(slot) => slot,
                Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => return false,
            };
            slot.path = None;
        }
        remove_owned_slot(&self.registry, self.upload_id, &self.owner);
        self.finished = true;
        true
    }

    fn check(&self, current: &dyn Fn() -> bool) -> Result<(), WebResponse> {
        if self.failed
            || self.finished
            || !current()
            || !self.lifetime.is_active()
            || Instant::now() >= self.expires
        {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        self.service
            .check_material_lease(self.lease, self.scope, &[])
            .map_err(authentication_error)
    }
    pub fn write(
        &mut self,
        token: RequestToken,
        sequence: u64,
        offset: u64,
        bytes: &[u8],
        context: &JobContext,
    ) -> Result<UploadAck, WebResponse> {
        self.write_checked(token, sequence, offset, bytes, &context.execution_fence())
    }
    fn write_checked(
        &mut self,
        token: RequestToken,
        sequence: u64,
        offset: u64,
        bytes: &[u8],
        current: &dyn Fn() -> bool,
    ) -> Result<UploadAck, WebResponse> {
        let result = (|| {
            self.check(current)?;
            let next = offset
                .checked_add(bytes.len() as u64)
                .filter(|next| *next <= self.expected);
            if token != self.token()
                || sequence != self.next_sequence
                || offset != self.written
                || bytes.is_empty()
                || bytes.len() > CHUNK
                || next.is_none()
            {
                return Err(failure(400, responses::ErrorCode::InvalidRequest));
            }
            let sequence_after = sequence.checked_add(1).ok_or_else(unavailable)?;
            self.file
                .as_mut()
                .ok_or_else(unavailable)?
                .file_mut()
                .write_all(bytes)
                .map_err(|_| unavailable())?;
            self.check(current)?;
            self.written = next.ok_or_else(unavailable)?;
            self.next_sequence = sequence_after;
            Ok(UploadAck::written(
                token,
                UploadKind::Material,
                self.upload_id,
                sequence,
                self.written,
            ))
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn finish(self, token: RequestToken, context: &JobContext) -> WebResponse {
        self.finish_checked(token, &context.execution_fence())
    }
    fn finish_checked(mut self, token: RequestToken, current: &dyn Fn() -> bool) -> WebResponse {
        let result = (|| -> Result<(), WebResponse> {
            self.check(current)?;
            if token != self.token() || self.written != self.expected {
                return Err(failure(400, responses::ErrorCode::InvalidResource));
            }
            let file = self.file.take().ok_or_else(unavailable)?;
            self.sealed = Some(file.seal().map_err(|_| unavailable())?);
            if self
                .sealed
                .as_ref()
                .ok_or_else(unavailable)?
                .len()
                .map_err(|_| unavailable())?
                != self.expected
            {
                return Err(failure(400, responses::ErrorCode::InvalidResource));
            }
            self.check(current)?;
            let registered = {
                let registry = self.registry.upgrade().ok_or_else(unavailable)?;
                let guard = registry.try_lock().map_err(|_| unavailable())?;
                guard
                    .as_ref()
                    .and_then(|files| files.find(self.upload_id))
                    .is_some_and(|slot| Arc::ptr_eq(&slot, &self.owner))
            };
            if !registered {
                return Err(failure(409, responses::ErrorCode::UploadExpired));
            }
            let mut slot = self.owner.try_lock().map_err(|_| unavailable())?;
            if !matches!(slot.phase, Phase::Receiving) || slot.lease != self.lease {
                return Err(failure(409, responses::ErrorCode::UploadNotReady));
            }
            if !current() || !self.lifetime.is_active() || Instant::now() >= self.expires {
                return Err(failure(403, responses::ErrorCode::Forbidden));
            }
            slot.phase = Phase::Ready(self.sealed.take().ok_or_else(unavailable)?);
            self.finished = true;
            Ok(())
        })();
        match result {
            Ok(()) => response(200, wire::uploaded(self.upload_id, self.written), None),
            Err(error) => error,
        }
    }
}
impl Drop for ReceivingMaterialUpload {
    fn drop(&mut self) {
        // Close the actual writer/read pin before the last charged Arc can go.
        drop(self.file.take());
        drop(self.sealed.take());
        if !self.finished {
            remove_owned_slot(&self.registry, self.upload_id, &self.owner);
        }
    }
}

struct PreparationClaim {
    registry: std::sync::Weak<Mutex<Option<MaterialFiles>>>,
    upload_id: u64,
    owner: Arc<Mutex<Slot>>,
    keep: bool,
}
impl Drop for PreparationClaim {
    fn drop(&mut self) {
        if !self.keep {
            remove_owned_slot(&self.registry, self.upload_id, &self.owner);
        }
    }
}
/// Opaque validated bytes pinned through the durable short SQL commit. A rejected
/// or unknown attempt keeps the final immutable object for existing reconciliation.
/// The proof cannot be constructed from a browser payload or a pathname.
pub struct PreparedWebMaterial {
    lease: SessionLease,
    scope: MaterialScope,
    command_id: CommandId,
    proof: Option<PreparedMaterialAppend>,
    pin: Option<LockedFile>,
    path: PathBuf,
    attempted: bool,
    service: WebService,
    registry: std::sync::Weak<Mutex<Option<MaterialFiles>>>,
    upload_id: u64,
    expires: Instant,
    lifetime: Option<TransferLifetime>,
    _physical_owner: Arc<Mutex<Slot>>,
}
impl Drop for PreparedWebMaterial {
    fn drop(&mut self) {
        self.pin.take();
        if !self.attempted {
            self._physical_owner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .path = Some(self.path.clone());
        }
        remove_owned_slot(&self.registry, self.upload_id, &self._physical_owner);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialTransferBinding {
    Absent,
    Unbound,
    Exact,
    BoundElsewhere,
    Busy,
}
impl WebApplication {
    /// Pure memory ownership observation for negative cleanup. No lease is
    /// authenticated here, and an unbound descriptor cannot be claimed by ID.
    pub(crate) fn material_transfer_binding(
        &self,
        upload_id: u64,
        life: &TransferLifetime,
    ) -> MaterialTransferBinding {
        let owner = {
            let Ok(registry) = self.material_files.try_lock() else {
                return MaterialTransferBinding::Busy;
            };
            let Some(files) = registry.as_ref() else {
                return MaterialTransferBinding::Absent;
            };
            files.find(upload_id)
        };
        let Some(owner) = owner else {
            return MaterialTransferBinding::Absent;
        };
        let Ok(slot) = owner.try_lock() else {
            return MaterialTransferBinding::Busy;
        };
        match slot.lifetime.as_ref() {
            None => MaterialTransferBinding::Unbound,
            Some(saved) if saved.same_owner(life) => MaterialTransferBinding::Exact,
            Some(_) => MaterialTransferBinding::BoundElsewhere,
        }
    }
    pub fn configure_material_files(&mut self, root: &Path) -> io::Result<()> {
        if self.material_files.lock().map_err(io_error)?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "material worker configured",
            ));
        }
        // Configuration requires an exclusive application reference. Filesystem
        // ownership/marker IO still occurs outside its short metadata lock.
        let configured = MaterialFiles::new(root)?;
        let mut guard = self.material_files.lock().map_err(io_error)?;
        if guard.is_some() {
            drop(guard);
            drop(configured);
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "material worker configured",
            ));
        }
        *guard = Some(configured);
        Ok(())
    }
    /// Explicit maintenance hook for the owning application tick/worker shutdown.
    /// Expired/revoked slots are also swept on every metadata or stream admission.
    pub fn sweep_material_uploads(&self) -> io::Result<()> {
        let candidates = {
            let guard = self.material_files.lock().map_err(io_error)?;
            guard
                .as_ref()
                .map(|files| files.slots.clone())
                .unwrap_or_default()
        };
        for (id, owner) in candidates {
            let facts = match owner.try_lock() {
                // A prepared proof still owns the immutable inode and permit.
                Ok(slot) if matches!(slot.phase, Phase::Consuming) && slot.path.is_none() => {
                    continue
                }
                Ok(slot) => Some((
                    slot.lease,
                    slot.scope,
                    slot.expires,
                    slot.lifetime.clone(),
                    matches!(slot.phase, Phase::Consuming),
                )),
                Err(std::sync::TryLockError::WouldBlock) => continue,
                Err(_) => None,
            };
            // Business checks can wait for a publication; hold neither file
            // registry nor individual slot lock across that check.
            let keep = facts.is_some_and(|(lease, scope, expires, lifetime, consuming)| {
                !consuming
                    && Instant::now() < expires
                    && lifetime.as_ref().is_none_or(TransferLifetime::is_active)
                    && matches!(
                        self.service.check_material_lease(lease, scope, &[]),
                        Ok(()) | Err(WebServiceError::Busy | WebServiceError::Unavailable)
                    )
            });
            if !keep {
                // A preparation may have claimed the stage since the snapshot.
                let claimed = owner
                    .try_lock()
                    .map(|slot| matches!(slot.phase, Phase::Consuming) && slot.path.is_none())
                    .unwrap_or(true);
                if !claimed {
                    remove_owned_slot(&Arc::downgrade(&self.material_files), id, &owner);
                }
            }
            // Retired files/permits are destroyed only after both locks end.
        }
        Ok(())
    }
    fn material_authenticate(
        &self,
        head: &RequestHead,
    ) -> Result<(SessionLease, MaterialRoute), WebResponse> {
        let route = self
            .boundary
            .preflight(head)
            .map_err(|error| self.request_boundary_error(head, error))?;
        let Route::Materials { route } = route else {
            return Err(failure(404, responses::ErrorCode::NotFound));
        };
        let mut cookies = head
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("cookie"));
        let header = match (cookies.next(), cookies.next()) {
            (Some(h), None) => h,
            (None, _) => return Err(failure(401, responses::ErrorCode::Unauthorized)),
            _ => return Err(boundary_error(WebError::Headers)),
        };
        let cookie = cookie_secret(&header.value).map_err(authentication_error)?;
        let session = self
            .service
            .authenticate(&cookie)
            .map_err(authentication_error)?;
        self.boundary
            .check(head, Some(session.csrf.as_str()))
            .map_err(|error| self.request_boundary_error(head, error))?;
        Ok((session.lease(), route))
    }
    pub(super) fn material_create(
        &self,
        backend: &mut Backend,
        lease: SessionLease,
        scope: MaterialScope,
        input: wire::UploadInput,
    ) -> WebResponse {
        if let Err(e) = backend.web_material_authorize(lease, scope, &[]) {
            return materials::error(e, false);
        }
        if self.sweep_material_uploads().is_err() {
            return unavailable();
        }
        let controlled = {
            let guard = match self.material_files.try_lock() {
                Ok(g) => g,
                Err(_) => return unavailable(),
            };
            let Some(files) = guard.as_ref() else {
                return unavailable();
            };
            if files.slots.len() >= 2 {
                return failure(429, responses::ErrorCode::Capacity);
            }
            files.files.clone()
        };
        let (_, material_id, version) = match backend.web_material_identifiers(lease, scope) {
            Ok(ids) => ids,
            Err(e) => return materials::error(e, false),
        };
        // Equal server IDs make a consumed upload's receipt replay check durable,
        // without retaining an unbounded in-memory command/upload mapping.
        let id = version.get();
        let stage = match controlled.directory(&["web-material-staging".into()]) {
            Ok(p) => p.join(format!("web-material-{id}.part")),
            Err(_) => return unavailable(),
        };
        let Some(permit) = PhysicalUploadPermit::take() else {
            return failure(429, responses::ErrorCode::Capacity);
        };
        let file = match controlled.create_new(&stage) {
            Ok(f) => f,
            Err(_) => return unavailable(),
        };
        let identity = match file.identity() {
            Ok(identity) => identity,
            Err(_) => {
                // Even a metadata failure may not discard a created inode's
                // physical charge. Retain its original handle until a retry can
                // capture identity, then close and remove only that object.
                let mut original = Some(file);
                let mut identity = None;
                quarantine_cleanup(permit, move || {
                    if identity.is_none() {
                        identity = original.as_ref().and_then(|file| file.identity().ok());
                        if identity.is_none() {
                            return false;
                        }
                        drop(original.take());
                    }
                    controlled
                        .remove_object_expected(
                            &stage,
                            false,
                            identity.expect("captured identity"),
                            |_| true,
                        )
                        .is_ok()
                });
                return unavailable();
            }
        };
        let slot = Arc::new(Mutex::new(Slot {
            _permit: Some(permit),
            identity,
            lease,
            scope,
            material_id,
            version,
            name: input.display_name,
            bytes: input.bytes,
            expires: Instant::now() + TTL,
            lifetime: None,
            phase: Phase::Waiting(file),
            path: Some(stage),
            files: controlled,
        }));
        if self
            .service
            .check_material_lease(lease, scope, &[])
            .is_err()
        {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        let inserted = {
            let mut guard = match self.material_files.try_lock() {
                Ok(g) => g,
                Err(_) => return unavailable(),
            };
            let Some(files) = guard.as_mut() else {
                return unavailable();
            };
            if files.slots.len() >= 2 {
                false
            } else {
                files.slots.push((id, slot.clone()));
                true
            }
        };
        if !inserted {
            return failure(429, responses::ErrorCode::Capacity);
        }
        response(201, wire::ready(id), None)
    }
    pub(super) fn material_cancel(&self, lease: SessionLease, upload_id: u64) -> WebResponse {
        if self.sweep_material_uploads().is_err() {
            return unavailable();
        }
        let owner = {
            let guard = match self.material_files.try_lock() {
                Ok(g) => g,
                Err(_) => return unavailable(),
            };
            guard.as_ref().and_then(|files| files.find(upload_id))
        };
        let Some(owner) = owner else {
            return failure(409, responses::ErrorCode::UploadExpired);
        };
        let scope = {
            let slot = match owner.try_lock() {
                Ok(s) => s,
                Err(_) => return unavailable(),
            };
            if slot.lease != lease {
                return failure(403, responses::ErrorCode::Forbidden);
            }
            slot.scope
        };
        if self
            .service
            .check_material_lease(lease, scope, &[])
            .is_err()
        {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        let (phase, files, path, identity) = {
            let mut slot = match owner.try_lock() {
                Ok(s) => s,
                Err(_) => return unavailable(),
            };
            if matches!(slot.phase, Phase::Receiving)
                || matches!(slot.phase, Phase::Consuming) && slot.path.is_none()
            {
                return failure(409, responses::ErrorCode::UploadNotReady);
            }
            let Some(path) = slot.path.take() else {
                return unavailable();
            };
            (
                std::mem::replace(&mut slot.phase, Phase::Consuming),
                slot.files.clone(),
                path,
                slot.identity,
            )
        };
        // Close the exclusive writer/read pin before deletion, all outside locks.
        drop(phase);
        if files
            .remove_object_expected(&path, false, identity, |_| true)
            .is_err()
        {
            if let Ok(mut slot) = owner.lock() {
                slot.path = Some(path);
            }
            return unavailable();
        }
        remove_owned_slot(&Arc::downgrade(&self.material_files), upload_id, &owner);
        response(200, wire::cancelled(upload_id), None)
    }
    /// Begin on an existing worker. No body is admitted before authentication,
    /// exact descriptor comparison and transfer of the already charged stage.
    pub fn begin_material_upload(
        &self,
        head: &RequestHead,
        lifetime: TransferLifetime,
        context: &JobContext,
    ) -> Result<ReceivingMaterialUpload, WebResponse> {
        self.begin_material_upload_checked(head, lifetime, &context.execution_fence())
    }
    /// Negative cleanup for a retained Ready transfer, on a fresh cleanup job.
    /// Correlation can retire only its own exact stage, never a replacement.
    pub fn discard_material_transfer(
        &self,
        token: RequestToken,
        upload_id: u64,
        context: &JobContext,
    ) -> Result<(), WebResponse> {
        if context.checkpoint().is_err() {
            return Err(unavailable());
        }
        self.discard_material_transfer_in_cleanup(token, upload_id)
    }
    /// Negative-only cleanup of the original correlation, on a worker/final
    /// coordinator. It grants no authority to write, prepare or commit bytes.
    pub fn discard_material_transfer_in_cleanup(
        &self,
        token: RequestToken,
        upload_id: u64,
    ) -> Result<(), WebResponse> {
        let owner = {
            let registry = self.material_files.try_lock().map_err(|_| unavailable())?;
            registry.as_ref().and_then(|files| files.find(upload_id))
        }
        .ok_or_else(|| failure(409, responses::ErrorCode::UploadExpired))?;
        let (phase, files, path, identity) = {
            let mut slot = owner.try_lock().map_err(|_| unavailable())?;
            if slot
                .lifetime
                .as_ref()
                .is_none_or(|life| life.token() != token)
            {
                return Err(failure(403, responses::ErrorCode::Forbidden));
            }
            if !matches!(slot.phase, Phase::Ready(_) | Phase::Consuming) || slot.path.is_none() {
                return Err(failure(409, responses::ErrorCode::UploadNotReady));
            }
            let path = slot.path.take().ok_or_else(unavailable)?;
            (
                std::mem::replace(&mut slot.phase, Phase::Consuming),
                slot.files.clone(),
                path,
                slot.identity,
            )
        };
        drop(phase);
        if files
            .remove_object_expected(&path, false, identity, |_| true)
            .is_err()
        {
            if let Ok(mut slot) = owner.lock() {
                slot.path = Some(path);
            }
            return Err(unavailable());
        }
        remove_owned_slot(&Arc::downgrade(&self.material_files), upload_id, &owner);
        Ok(())
    }
    /// Memory-only conservative witness for a panic tombstone. A contended or
    /// not-yet-bound stage remains pending; only an observed different token or
    /// absent registry entry proves this original key no longer owns a slot.
    pub fn material_transfer_pending(&self, token: RequestToken, upload_id: u64) -> bool {
        let owner = {
            let Ok(registry) = self.material_files.try_lock() else {
                return true;
            };
            registry.as_ref().and_then(|files| files.find(upload_id))
        };
        let Some(owner) = owner else {
            return false;
        };
        let Ok(slot) = owner.try_lock() else {
            return true;
        };
        slot.lifetime
            .as_ref()
            .is_none_or(|life| life.token() == token)
    }
    fn begin_material_upload_checked(
        &self,
        head: &RequestHead,
        lifetime: TransferLifetime,
        current: &dyn Fn() -> bool,
    ) -> Result<ReceivingMaterialUpload, WebResponse> {
        if head.token != lifetime.token() || !current() || !lifetime.is_active() {
            return Err(unavailable());
        }
        let lifetime = lifetime
            .bounded(TRANSFER_BUDGET)
            .map_err(|_| unavailable())?;
        let (lease, route) = self.material_authenticate(head)?;
        let MaterialRoute::UploadBytes { upload_id } = route else {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        };
        self.sweep_material_uploads().map_err(|_| unavailable())?;
        let owned = {
            let guard = self.material_files.try_lock().map_err(|_| unavailable())?;
            guard.as_ref().and_then(|files| files.find(upload_id))
        }
        .ok_or_else(|| failure(409, responses::ErrorCode::UploadExpired))?;
        let (file, expires, scope, expected) = {
            let mut slot = owned
                .try_lock()
                .map_err(|_| failure(429, responses::ErrorCode::Capacity))?;
            if slot.lease != lease {
                return Err(failure(403, responses::ErrorCode::Forbidden));
            }
            if head.content_length != Some(slot.bytes) {
                return Err(failure(400, responses::ErrorCode::InvalidRequest));
            }
            if Instant::now() >= slot.expires {
                return Err(failure(409, responses::ErrorCode::UploadExpired));
            }
            if !matches!(slot.phase, Phase::Waiting(_)) {
                return Err(failure(409, responses::ErrorCode::UploadNotReady));
            }
            let Phase::Waiting(file) = std::mem::replace(&mut slot.phase, Phase::Receiving) else {
                unreachable!()
            };
            slot.expires = slot.expires.min(lifetime.deadline());
            slot.lifetime = Some(lifetime.clone());
            (file, slot.expires, slot.scope, slot.bytes)
        };
        let mut receiver = ReceivingMaterialUpload {
            file: Some(file),
            sealed: None,
            owner: owned,
            registry: Arc::downgrade(&self.material_files),
            service: self.service.clone(),
            lease,
            scope,
            expected,
            lifetime,
            expires,
            upload_id,
            written: 0,
            next_sequence: 1,
            failed: false,
            finished: false,
        };
        if receiver.check(current).is_err() {
            receiver.failed = true;
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        Ok(receiver)
    }
    /// Compatibility for already-arrived finite local File/Cursor bytes only.
    /// Blocking network Read is not a cancellable transport and must not be used.
    pub fn dispatch_material_upload<R: Read>(
        &self,
        head: &RequestHead,
        mut input: R,
    ) -> WebResponse {
        let lifetime = match TransferLifetime::local(head.token, STREAM_TIME) {
            Ok(v) => v,
            Err(_) => return unavailable(),
        };
        let mut receiver = match self.begin_material_upload_checked(head, lifetime, &|| true) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let expected = head.content_length.unwrap_or(0);
        let mut chunk = [0u8; CHUNK];
        while receiver.written < expected {
            if receiver.check(&|| true).is_err() {
                return failure(403, responses::ErrorCode::Forbidden);
            }
            let limit = (expected - receiver.written).min(CHUNK as u64) as usize;
            let count = match input.read(&mut chunk[..limit]) {
                Ok(n) if n != 0 => n,
                _ => return failure(400, responses::ErrorCode::InvalidResource),
            };
            let sequence = receiver.next_sequence;
            let offset = receiver.written;
            if let Err(e) =
                receiver.write_checked(head.token, sequence, offset, &chunk[..count], &|| true)
            {
                return e;
            }
        }
        let mut extra = [0u8; 1];
        if input.read(&mut extra).ok() != Some(0) {
            return failure(400, responses::ErrorCode::InvalidResource);
        }
        receiver.finish_checked(head.token, &|| true)
    }
    /// Long validation/publication phase. Invoke on the existing bounded file
    /// worker with NO Backend lock held, then pass its opaque result to commit.
    pub fn prepare_material_append(
        &self,
        head: &RequestHead,
        body: &[u8],
    ) -> Result<PreparedWebMaterial, WebResponse> {
        self.prepare_material_append_checked(head, body, &|| true)
    }
    pub fn prepare_material_append_with_context(
        &self,
        head: &RequestHead,
        body: &[u8],
        context: &JobContext,
    ) -> Result<PreparedWebMaterial, WebResponse> {
        self.prepare_material_append_checked(head, body, &context.execution_fence())
    }
    fn prepare_material_append_checked(
        &self,
        head: &RequestHead,
        body: &[u8],
        current: &dyn Fn() -> bool,
    ) -> Result<PreparedWebMaterial, WebResponse> {
        if !current() {
            return Err(unavailable());
        }
        let (lease, route) = self.material_authenticate(head)?;
        let MaterialRoute::Append { scope } = route else {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        };
        if body.len() as u64 != head.content_length.unwrap_or(0) {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        }
        let input =
            wire::append(body).map_err(|_| failure(400, responses::ErrorCode::InvalidRequest))?;
        self.service
            .check_material_lease(lease, scope, &input.allowed_class_ids)
            .map_err(authentication_error)?;
        self.sweep_material_uploads().map_err(|_| unavailable())?;
        let owned_slot = {
            let guard = self.material_files.try_lock().map_err(|_| unavailable())?;
            guard.as_ref().and_then(|files| files.find(input.upload_id))
        }
        .ok_or_else(|| failure(409, responses::ErrorCode::UploadExpired))?;
        let (file, files, id, version, name, expected, expires, lifetime, stage_path) = {
            let mut slot = owned_slot
                .try_lock()
                .map_err(|_| failure(429, responses::ErrorCode::Capacity))?;
            if slot.lease != lease || slot.scope != scope {
                return Err(failure(403, responses::ErrorCode::Forbidden));
            }
            if Instant::now() >= slot.expires
                || slot.lifetime.as_ref().is_some_and(|life| !life.is_active())
            {
                return Err(failure(409, responses::ErrorCode::UploadExpired));
            }
            if !matches!(slot.phase, Phase::Ready(_)) {
                return Err(failure(409, responses::ErrorCode::UploadNotReady));
            }
            let stage_path = slot.path.take().ok_or_else(unavailable)?;
            let Phase::Ready(file) = std::mem::replace(&mut slot.phase, Phase::Consuming) else {
                unreachable!()
            };
            (
                file,
                slot.files.clone(),
                slot.material_id,
                slot.version,
                slot.name.clone(),
                slot.bytes,
                slot.expires,
                slot.lifetime.clone(),
                stage_path,
            )
        };
        let mut claim = PreparationClaim {
            registry: Arc::downgrade(&self.material_files),
            upload_id: input.upload_id,
            owner: owned_slot.clone(),
            keep: false,
        };
        let mut preparation = Preparation {
            cleanup_owner: owned_slot.clone(),
            files: files.clone(),
            scope,
            id,
            version,
            name: name.clone(),
            expected,
            control: Control {
                service: &self.service,
                lease,
                scope,
                classes: &input.allowed_class_ids,
                start: Instant::now(),
                expires,
                revision: None,
                lifetime: lifetime.as_ref(),
                current: Some(current),
            },
            pin: None,
            path: stage_path,
            keep: false,
        };
        let proof = prepare_material_append(
            &mut preparation,
            file,
            MaterialAppendMetadata {
                command_id: input.command_id,
                scope,
                material_id: id,
                display_name: name,
                allowed_class_ids: input.allowed_class_ids.clone(),
            },
        )
        .map_err(|e| materials::error(BackendError::Material(e), false))?;
        if preparation.control.canceled() {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        let result = PreparedWebMaterial {
            lease,
            scope,
            command_id: input.command_id,
            proof: Some(proof),
            pin: preparation.pin.take(),
            path: preparation.path.clone(),
            attempted: false,
            service: self.service.clone(),
            registry: Arc::downgrade(&self.material_files),
            upload_id: input.upload_id,
            expires,
            lifetime: lifetime.clone(),
            _physical_owner: owned_slot,
        };
        preparation.keep = true;
        claim.keep = true;
        Ok(result)
    }
    pub fn commit_material_append(
        &self,
        backend: &mut Backend,
        prepared: PreparedWebMaterial,
    ) -> WebResponse {
        if let Some(lifetime) = &prepared.lifetime {
            let lifetime = lifetime.clone();
            let expires = prepared.expires;
            let mut scoped = backend.begin_execution_scope(Arc::new(move || {
                lifetime.is_active() && Instant::now() < expires
            }));
            self.commit_material_append_checked(&mut scoped, prepared, &|| true)
        } else {
            self.commit_material_append_checked(backend, prepared, &|| true)
        }
    }
    pub fn commit_material_append_with_context(
        &self,
        backend: &mut Backend,
        prepared: PreparedWebMaterial,
        context: &JobContext,
    ) -> WebResponse {
        let current = context.execution_fence();
        let fence: crate::web_transfer::ExecutionFence = match &prepared.lifetime {
            Some(lifetime) => lifetime.job_fence(context),
            None => Arc::new(context.execution_fence()),
        };
        let expires = prepared.expires;
        let mut scoped =
            backend.begin_execution_scope(Arc::new(move || fence() && Instant::now() < expires));
        self.commit_material_append_checked(&mut scoped, prepared, &current)
    }
    fn commit_material_append_checked(
        &self,
        backend: &mut Backend,
        mut prepared: PreparedWebMaterial,
        current: &dyn Fn() -> bool,
    ) -> WebResponse {
        if !self.service.same_instance(&backend.web_service())
            || !self.service.same_instance(&prepared.service)
        {
            return unavailable();
        }
        if !current()
            || prepared
                .lifetime
                .as_ref()
                .is_some_and(|life| !life.is_active())
            || Instant::now() >= prepared.expires
            || self
                .service
                .check_material_lease(prepared.lease, prepared.scope, &[])
                .is_err()
        {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        prepared.attempted = true;
        let proof = prepared.proof.take().expect("opaque proof consumed once");
        match backend.web_material_append(prepared.lease, proof) {
            Ok(record) => {
                if !current()
                    || prepared
                        .lifetime
                        .as_ref()
                        .is_some_and(|life| !life.is_active())
                {
                    return failure(503, responses::ErrorCode::ResultUnknown);
                }
                materials::committed(prepared.command_id, prepared.scope, &record)
            }
            Err(e) => {
                // Domain/CAS/quota/idempotency failures are returned before
                // COMMIT; only uncertain completion retains unreferenced bytes.
                if matches!(
                    &e,
                    BackendError::Material(
                        MaterialRepositoryError::Domain(_)
                            | MaterialRepositoryError::IdempotencyConflict
                            | MaterialRepositoryError::QuotaExceeded
                            | MaterialRepositoryError::DocumentTooLarge
                            | MaterialRepositoryError::Cancelled
                    )
                ) {
                    prepared.attempted = false;
                }
                materials::error(e, true)
            }
        }
    }
    /// Read the exact durable prior result before consulting any ephemeral
    /// upload/Ready registry. This is a worker-only new-request operation: the
    /// old upload lifetime grants no permission and is never renewed here.
    pub(crate) fn material_append_receipt_in_job(
        &self,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        body: &[u8],
        life: &TransferLifetime,
        context: &JobContext,
    ) -> Result<Option<WebResponse>, WebResponse> {
        let current = || life.is_active() && context.checkpoint().is_ok();
        if head.token != life.token() || context.deadline() > life.deadline() || !current() {
            return Err(unavailable());
        }
        let (lease, route) = self.material_authenticate(head)?;
        let MaterialRoute::Append { scope } = route else {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        };
        if body.len() as u64 != head.content_length.unwrap_or(0) {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        }
        let input =
            wire::append(body).map_err(|_| failure(400, responses::ErrorCode::InvalidRequest))?;
        if !current() {
            return Err(unavailable());
        }
        let prior = {
            let mut backend = backend.try_lock().map_err(|_| unavailable())?;
            let scoped = backend.begin_execution_scope(life.job_fence(context));
            if !self.service.same_instance(&scoped.web_service()) {
                return Err(unavailable());
            }
            scoped
                .web_material_outcome(lease, input.command_id)
                .map_err(|error| materials::error(error, false))?
        };
        // The read may have waited for publication/SQL. Reauthenticate the exact
        // new cookie/CSRF and lease after releasing Backend; no file work occurs.
        if !current() {
            return Err(unavailable());
        }
        let (latest, latest_route) = self.material_authenticate(head)?;
        if latest != lease || latest_route != route {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        if !current() {
            return Err(unavailable());
        }
        let Some(prior) = prior else {
            return Ok(None);
        };
        if prior.kind != fc_ports::MaterialCommandKind::Append
            || prior.scope != scope
            || prior.record.resource.version_id.get() != input.upload_id
            || prior.record.allowed_class_ids != input.allowed_class_ids
        {
            return Err(failure(409, responses::ErrorCode::IdempotencyConflict));
        }
        let response = materials::committed(input.command_id, scope, &prior.record);
        if !current() {
            return Err(unavailable());
        }
        Ok(Some(response))
    }

    /// JSON append orchestration for the file worker: only receipt lookup and
    /// durable commit hold Backend; slow validation runs between those sections.
    pub fn dispatch_material_append(
        &self,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        body: &[u8],
    ) -> WebResponse {
        let (lease, route) = match self.material_authenticate(head) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let MaterialRoute::Append { scope } = route else {
            return failure(400, responses::ErrorCode::InvalidRequest);
        };
        if body.len() as u64 != head.content_length.unwrap_or(0) {
            return failure(400, responses::ErrorCode::InvalidRequest);
        }
        let input = match wire::append(body) {
            Ok(i) => i,
            Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
        };
        {
            let backend = match backend.try_lock() {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            if !self.service.same_instance(&backend.web_service()) {
                return unavailable();
            }
            match backend.web_material_outcome(lease, input.command_id) {
                Ok(Some(prior)) => {
                    if prior.kind == fc_ports::MaterialCommandKind::Append
                        && prior.scope == scope
                        && prior.record.resource.version_id.get() == input.upload_id
                        && prior.record.allowed_class_ids == input.allowed_class_ids
                    {
                        return materials::committed(input.command_id, scope, &prior.record);
                    }
                    return failure(409, responses::ErrorCode::IdempotencyConflict);
                }
                Ok(None) => {}
                Err(e) => return materials::error(e, false),
            }
        }
        let prepared = match self.prepare_material_append(head, body) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let mut backend = match backend.try_lock() {
            Ok(b) => b,
            Err(_) => return unavailable(),
        };
        self.commit_material_append(&mut backend, prepared)
    }
    pub(super) fn material_append(
        &self,
        _backend: &mut Backend,
        _lease: SessionLease,
        _scope: MaterialScope,
        _input: wire::AppendInput,
    ) -> WebResponse {
        // Slice dispatch is deliberately not a long-file-worker scheduler.
        unavailable()
    }
    pub fn begin_material_download(
        &self,
        backend: &Backend,
        head: &RequestHead,
    ) -> Result<MaterialDownload, WebResponse> {
        let life = TransferLifetime::local(head.token, STREAM_TIME).map_err(|_| unavailable())?;
        let current = life.execution_fence();
        self.plan_material_download(backend, head, life, &*current)?
            .open(&*current)
    }
    /// The trusted transport's original admission time spans queueing, hashing,
    /// reading and actual sends. A fresh worker never renews those 120 seconds.
    pub fn begin_material_download_in_job(
        &self,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        life: &TransferLifetime,
        context: &JobContext,
    ) -> Result<MaterialDownload, WebResponse> {
        if context.deadline() > life.deadline() || context.checkpoint().is_err() {
            return Err(unavailable());
        }
        let life = life.bounded(STREAM_TIME).map_err(|_| unavailable())?;
        let current = life.job_fence(context);
        let plan = {
            let mut backend = backend.try_lock().map_err(|_| unavailable())?;
            let scoped = backend.begin_execution_scope(current.clone());
            self.plan_material_download(&scoped, head, life, &*current)?
        };
        // In particular CreateFile is outside Backend, just like later hash/read.
        plan.open(&*current)
    }
    fn plan_material_download(
        &self,
        backend: &Backend,
        head: &RequestHead,
        life: TransferLifetime,
        current: &dyn Fn() -> bool,
    ) -> Result<MaterialDownloadPlan, WebResponse> {
        if head.token != life.token() || !current() || !life.is_active() {
            return Err(unavailable());
        }
        if !self.service.same_instance(&backend.web_service()) {
            return Err(unavailable());
        }
        let (lease, route) = self.material_authenticate(head)?;
        let MaterialRoute::Download {
            scope,
            material_id,
            version,
        } = route
        else {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        };
        // Mint BEFORE any metadata/revision read. Even a publication that rolls
        // back cancels this original token permanently; no later snapshot wins.
        let (revision, send_fence) = self
            .service
            .material_send_fence(lease, scope, &[], life.execution_fence())
            .map_err(authentication_error)?;
        let record = backend
            .web_material_download_record(lease, scope, material_id, version)
            .map_err(|e| materials::error(e, false))?;
        let files = {
            let guard = self.material_files.try_lock().map_err(|_| unavailable())?;
            guard.as_ref().ok_or_else(unavailable)?.files.clone()
        };
        if !current() || !send_fence() {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        Ok(MaterialDownloadPlan {
            files,
            record,
            material_id,
            lease,
            scope,
            service: self.service.clone(),
            revision,
            control: backend.session_control(),
            lifetime: life,
            send_fence,
        })
    }
}
/// Metadata only, confined to this worker's begin call. No caller can submit an
/// unmetered native open or retain a plan as a network reply.
struct MaterialDownloadPlan {
    files: Arc<ControlledFiles>,
    record: MaterialRecord,
    material_id: MaterialId,
    lease: SessionLease,
    scope: MaterialScope,
    service: WebService,
    revision: Revision,
    control: crate::backend::SessionControl,
    lifetime: TransferLifetime,
    send_fence: crate::web_transfer::ExecutionFence,
}
impl MaterialDownloadPlan {
    fn open(self, current: &dyn Fn() -> bool) -> Result<MaterialDownload, WebResponse> {
        let Self {
            files,
            record,
            material_id,
            lease,
            scope,
            service,
            revision,
            control,
            lifetime: life,
            send_fence,
        } = self;
        if !current() || !send_fence() {
            return Err(unavailable());
        }
        let file = LockedFile::open(
            &path(&files, scope, material_id, &record.resource).map_err(|_| unavailable())?,
            MAX_MATERIAL_BYTES,
        )
        .map_err(|_| unavailable())?;
        if !current() || !send_fence() {
            return Err(unavailable());
        }
        let hash: String = record
            .resource
            .sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let filename: String = record
            .display_name
            .as_bytes()
            .iter()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(b) {
                    (*b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        let bytes = record.resource.bytes;
        Ok(MaterialDownload {
            head: ResponseHead {
                status: 200,
                headers: vec![
                    Header {
                        name: "Content-Type".into(),
                        value: b"application/octet-stream".to_vec(),
                    },
                    Header {
                        name: "Content-Disposition".into(),
                        value: format!("attachment; filename*=UTF-8''{filename}").into_bytes(),
                    },
                    Header {
                        name: "Cache-Control".into(),
                        value: b"no-store".to_vec(),
                    },
                    Header {
                        name: "X-Content-Type-Options".into(),
                        value: b"nosniff".to_vec(),
                    },
                    Header {
                        name: "X-FC-SHA256".into(),
                        value: hash.into_bytes(),
                    },
                ],
                content_length: bytes,
            },
            file,
            lease,
            scope,
            service,
            revision,
            control,
            expires: life.deadline(),
            lifetime: life,
            send_fence,
            read: 0,
            verified: false,
            expected_sha: record.resource.sha256,
            buffer: Box::new([0; CHUNK]),
        })
    }
}
pub struct MaterialDownload {
    pub head: ResponseHead,
    file: LockedFile,
    lease: SessionLease,
    scope: MaterialScope,
    service: WebService,
    revision: Revision,
    control: crate::backend::SessionControl,
    expires: Instant,
    lifetime: TransferLifetime,
    send_fence: crate::web_transfer::ExecutionFence,
    read: u64,
    verified: bool,
    expected_sha: [u8; 32],
    buffer: Box<[u8; CHUNK]>,
}
impl MaterialDownload {
    pub fn read_chunk(&mut self, backend: &Backend) -> io::Result<&[u8]> {
        if !self.service.same_instance(&backend.web_service()) {
            return Err(denied());
        }
        self.read_chunk_without_backend()
    }
    /// HTTP send workers use the captured same-instance capability after begin;
    /// any authority revision (including delete) cancels this conservative download.
    pub fn read_chunk_without_backend(&mut self) -> io::Result<&[u8]> {
        self.read_chunk_checked(None)
    }
    /// Metadata only. Native owners must call this check immediately before
    /// each real WSASend; this Arc never owns a file or proves physical release.
    pub fn send_fence(&self) -> crate::web_transfer::ExecutionFence {
        self.send_fence.clone()
    }
    pub fn head(&self) -> &ResponseHead {
        &self.head
    }
    pub fn read_chunk_in_job(
        &mut self,
        life: &TransferLifetime,
        context: &JobContext,
    ) -> io::Result<&[u8]> {
        if !self.lifetime.same_owner(life)
            || life.deadline() != self.lifetime.deadline()
            || context.deadline() > self.expires
        {
            return Err(denied());
        }
        let current = life.job_fence(context);
        self.read_chunk_checked(Some(&*current))
    }
    fn read_chunk_checked(&mut self, current: Option<&dyn Fn() -> bool>) -> io::Result<&[u8]> {
        if !self.lifetime.is_active()
            || current.is_some_and(|check| !check())
            || !(self.send_fence)()
        {
            return Err(denied());
        }
        if !self.verified {
            let send_fence = self.send_fence.clone();
            let checking = || current.is_none_or(|check| check()) && send_fence();
            let check = Control {
                service: &self.service,
                lease: self.lease,
                scope: self.scope,
                classes: &[],
                start: Instant::now(),
                expires: self.expires,
                revision: Some((&self.control, self.revision)),
                lifetime: Some(&self.lifetime),
                current: Some(&checking),
            };
            let (bytes, sha) = fc_materials::validation::hash_file(self.file.file_mut(), &check)
                .map_err(io_error)?;
            if bytes != self.head.content_length || sha != self.expected_sha {
                return Err(invalid());
            }
            self.file.file_mut().seek(SeekFrom::Start(0))?;
            self.verified = true;
        }
        let valid = || {
            self.lifetime.is_active() && current.is_none_or(|check| check()) && (self.send_fence)()
        };
        if !valid() {
            return Err(denied());
        }
        let remaining = self
            .head
            .content_length
            .checked_sub(self.read)
            .ok_or_else(invalid)?;
        if remaining == 0 {
            return Ok(&[]);
        }
        let n = self
            .file
            .file_mut()
            .read(&mut self.buffer[..remaining.min(CHUNK as u64) as usize])?;
        if n == 0 {
            return Err(invalid());
        }
        if !valid() {
            return Err(denied());
        }
        self.read += n as u64;
        Ok(&self.buffer[..n])
    }
}
/// Final release check is deliberately AFTER the potentially waiting authority
/// callback. A delete may publish while that callback adopts a newer snapshot.
pub(super) fn download_checkpoint(
    control: &crate::backend::SessionControl,
    revision: Revision,
    expires: Instant,
    authorize: impl FnOnce() -> bool,
) -> bool {
    if Instant::now() >= expires || !authorize() {
        return false;
    }
    let status = control.health_gate().status();
    status.health == fc_runtime::health::Health::Healthy
        && status.revision == revision
        && Instant::now() < expires
}
struct Control<'a> {
    service: &'a WebService,
    lease: SessionLease,
    scope: MaterialScope,
    classes: &'a [ClassId],
    start: Instant,
    expires: Instant,
    revision: Option<(&'a crate::backend::SessionControl, Revision)>,
    lifetime: Option<&'a TransferLifetime>,
    current: Option<&'a dyn Fn() -> bool>,
}
impl ValidationControl for Control<'_> {
    fn canceled(&self) -> bool {
        self.current.is_some_and(|current| !current())
            || self.lifetime.is_some_and(|life| !life.is_active())
            || self.start.elapsed() >= Duration::from_secs(30)
            || Instant::now() >= self.expires
            || self
                .service
                .check_material_lease(self.lease, self.scope, self.classes)
                .is_err()
            || self.revision.is_some_and(|(control, revision)| {
                !download_checkpoint(control, revision, self.expires, || true)
            })
    }
    fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
        fc_windows::system::current_thread_cpu_time().map_err(|_| ContainerError::ClockUnavailable)
    }
}
struct Preparation<'a> {
    cleanup_owner: Arc<Mutex<Slot>>,
    files: Arc<ControlledFiles>,
    scope: MaterialScope,
    id: MaterialId,
    version: ResourceVersionId,
    name: String,
    expected: u64,
    control: Control<'a>,
    pin: Option<LockedFile>,
    path: PathBuf,
    keep: bool,
}
impl Drop for Preparation<'_> {
    fn drop(&mut self) {
        self.pin.take();
        if !self.keep {
            self.cleanup_owner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .path = Some(self.path.clone());
        }
    }
}
impl MaterialPreparation for Preparation<'_> {
    type Input = LockedFile;
    fn prepare(
        &mut self,
        mut file: LockedFile,
    ) -> Result<MaterialResource, MaterialRepositoryError> {
        let work = (|| -> io::Result<MaterialResource> {
            if self.control.canceled() {
                return Err(denied());
            }
            let kind = MaterialKind::from_name(&self.name).ok_or_else(invalid)?;
            let (bytes, sha) = if kind == MaterialKind::Pdf {
                let (bytes, sha) =
                    fc_materials::validation::hash_file(file.file_mut(), &self.control)
                        .map_err(io_error)?;
                let report = helper::validate_pdf(file.path(), || self.control.canceled())
                    .map_err(io_error)?;
                let line = std::str::from_utf8(&report.stdout).map_err(io_error)?;
                let parts: Vec<_> = line.split_whitespace().collect();
                let expected: String = sha.iter().map(|b| format!("{b:02x}")).collect();
                if report.exit_code != Some(0)
                    || report.peak_private_bytes > helper::PDF_MEMORY_BYTES
                    || parts.len() != 5
                    || parts[0] != "PDF_VALIDATED"
                    || parts[1].parse::<u64>().ok() != Some(bytes)
                    || parts[2] != expected
                    || !parts[3]
                        .parse::<u32>()
                        .is_ok_and(|p| (1..=2000).contains(&p))
                    || !parts[4]
                        .parse::<u32>()
                        .is_ok_and(|n| (1..=20000).contains(&n))
                {
                    return Err(invalid());
                }
                (bytes, sha)
            } else {
                let report = fc_materials::validation::validate_file(
                    file.file_mut(),
                    &self.name,
                    &self.control,
                )
                .map_err(io_error)?;
                if let Some(expected) = report.image {
                    let decoded = image::validate_image(&file, kind, || self.control.canceled())
                        .map_err(io_error)?;
                    if (decoded.width, decoded.height) != (expected.width, expected.height) {
                        return Err(invalid());
                    }
                }
                (report.bytes, report.sha256)
            };
            if bytes != self.expected || self.control.canceled() {
                return Err(invalid());
            }
            let resource = MaterialResource {
                version_id: self.version,
                sha256: sha,
                kind,
                bytes,
            };
            resource.validate().map_err(io_error)?;
            let destination = path(&self.files, self.scope, self.id, &resource)?;
            let file = file.publish(&self.files, &destination).map_err(io_error)?;
            self.path = destination;
            self.pin = Some(file);
            Ok(resource)
        })();
        work.map_err(|e| {
            if e.kind() == io::ErrorKind::PermissionDenied {
                MaterialRepositoryError::Cancelled
            } else {
                MaterialRepositoryError::Domain(MaterialError::InvalidResource)
            }
        })
    }
}
