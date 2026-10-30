//! Controlled Windows file worker. This synchronous Read adapter accepts only
//! already-arrived, owned local File/Cursor bytes. A potentially blocking network
//! reader is NOT a deadline or a cancelled IOCP operation and must not be passed
//! here. HTTP event/staging scheduling remains a separate disabled-listener gate.
use super::*;
use crate::backend::BackendError;
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
use super::shared_upload::PhysicalUploadPermit;
enum Phase {
    Waiting(NewFile),
    Ready(LockedFile),
    Consuming,
}
struct Slot {
    _permit: PhysicalUploadPermit,
    lease: SessionLease,
    scope: MaterialScope,
    material_id: MaterialId,
    version: ResourceVersionId,
    name: String,
    bytes: u64,
    expires: Instant,
    phase: Phase,
    path: Option<PathBuf>,
    files: Arc<ControlledFiles>,
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.phase = Phase::Consuming;
        if let Some(path) = &self.path {
            let _ = self.files.remove(path);
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
    fn sweep(&mut self, service: &WebService) {
        self.slots.retain(|(_, s)| match s.try_lock() {
            // A proof still owns the immutable final inode. Expiry/revocation
            // prevents commit but never returns its physical permit early.
            Ok(s) if matches!(s.phase, Phase::Consuming) && s.path.is_none() => true,
            Ok(s) => {
                Instant::now() < s.expires
                    && (!matches!(s.phase, Phase::Consuming) || s.path.is_none())
                    && matches!(
                        service.check_material_lease(s.lease, s.scope, &[]),
                        Ok(()) | Err(WebServiceError::Busy | WebServiceError::Unavailable)
                    )
            }
            Err(std::sync::TryLockError::WouldBlock) => true,
            Err(_) => false,
        });
    }
    fn find(&self, id: u64) -> Option<Arc<Mutex<Slot>>> {
        self.slots
            .iter()
            .find(|(saved, _)| *saved == id)
            .map(|(_, slot)| slot.clone())
    }
    fn remove(&mut self, id: u64) {
        self.slots.retain(|(saved, _)| *saved != id);
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
    files: Arc<ControlledFiles>,
    path: PathBuf,
    attempted: bool,
    service: WebService,
    registry: std::sync::Weak<Mutex<Option<MaterialFiles>>>,
    upload_id: u64,
    expires: Instant,
    _physical_owner: Arc<Mutex<Slot>>,
}
impl Drop for PreparedWebMaterial {
    fn drop(&mut self) {
        self.pin.take();
        if !self.attempted {
            let _ = self.files.remove(&self.path);
        }
        if let Some(registry) = self.registry.upgrade() {
            if let Ok(mut registry) = registry.lock() {
                if let Some(files) = registry.as_mut() {
                    files.remove(self.upload_id)
                }
            }
        }
    }
}
impl WebApplication {
    pub fn configure_material_files(&mut self, root: &Path) -> io::Result<()> {
        let mut guard = self.material_files.lock().map_err(io_error)?;
        if guard.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "material worker configured",
            ));
        }
        *guard = Some(MaterialFiles::new(root)?);
        Ok(())
    }
    /// Explicit maintenance hook for the owning application tick/worker shutdown.
    /// Expired/revoked slots are also swept on every metadata or stream admission.
    pub fn sweep_material_uploads(&self) -> io::Result<()> {
        let mut guard = self.material_files.lock().map_err(io_error)?;
        if let Some(files) = guard.as_mut() {
            files.sweep(&self.service)
        }
        Ok(())
    }
    fn material_authenticate(
        &self,
        head: &RequestHead,
    ) -> Result<(SessionLease, MaterialRoute), WebResponse> {
        let route = self.boundary.preflight(head).map_err(boundary_error)?;
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
            .map_err(boundary_error)?;
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
        let mut guard = match self.material_files.try_lock() {
            Ok(g) => g,
            Err(_) => return unavailable(),
        };
        let Some(files) = guard.as_mut() else {
            return unavailable();
        };
        files.sweep(&self.service);
        if files.slots.len() >= 2 {
            return failure(429, responses::ErrorCode::Capacity);
        }
        let (_, material_id, version) = match backend.web_material_identifiers(lease, scope) {
            Ok(ids) => ids,
            Err(e) => return materials::error(e, false),
        };
        // Equal server IDs make a consumed upload's receipt replay check durable,
        // without retaining an unbounded in-memory command/upload mapping.
        let id = version.get();
        let stage = match files.files.directory(&["web-material-staging".into()]) {
            Ok(p) => p.join(format!("web-material-{id}.part")),
            Err(_) => return unavailable(),
        };
        let Some(permit) = PhysicalUploadPermit::take() else {
            return failure(429, responses::ErrorCode::Capacity);
        };
        let file = match files.files.create_new(&stage) {
            Ok(f) => f,
            Err(_) => return unavailable(),
        };
        if self
            .service
            .check_material_lease(lease, scope, &[])
            .is_err()
        {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        files.slots.push((
            id,
            Arc::new(Mutex::new(Slot {
                _permit: permit,
                lease,
                scope,
                material_id,
                version,
                name: input.display_name,
                bytes: input.bytes,
                expires: Instant::now() + TTL,
                phase: Phase::Waiting(file),
                path: Some(stage),
                files: files.files.clone(),
            })),
        ));
        response(201, wire::ready(id), None)
    }
    pub(super) fn material_cancel(&self, lease: SessionLease, upload_id: u64) -> WebResponse {
        let mut guard = match self.material_files.try_lock() {
            Ok(g) => g,
            Err(_) => return unavailable(),
        };
        let Some(files) = guard.as_mut() else {
            return unavailable();
        };
        files.sweep(&self.service);
        let Some(slot) = files.find(upload_id) else {
            return failure(409, responses::ErrorCode::UploadExpired);
        };
        let mut slot = match slot.try_lock() {
            Ok(s) => s,
            Err(_) => return failure(429, responses::ErrorCode::Capacity),
        };
        if slot.lease != lease {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        if self
            .service
            .check_material_lease(lease, slot.scope, &[])
            .is_err()
        {
            return failure(403, responses::ErrorCode::Forbidden);
        }
        if matches!(slot.phase, Phase::Consuming) {
            return failure(409, responses::ErrorCode::UploadNotReady);
        }
        slot.phase = Phase::Consuming;
        let Some(path) = slot.path.as_ref() else {
            return unavailable();
        };
        if slot.files.remove(path).is_err() {
            return unavailable();
        }
        slot.path = None;
        drop(slot);
        files.remove(upload_id);
        response(200, wire::cancelled(upload_id), None)
    }
    /// Fixed-buffer copy of already arrived local bytes. It never locks Backend.
    /// Checks an absolute elapsed budget between local reads; no claim is made
    /// that this can interrupt an arbitrary blocking network Read implementation.
    pub fn dispatch_material_upload<R: Read>(
        &self,
        head: &RequestHead,
        mut input: R,
    ) -> WebResponse {
        let (lease, route) = match self.material_authenticate(head) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let MaterialRoute::UploadBytes { upload_id } = route else {
            return failure(400, responses::ErrorCode::InvalidRequest);
        };
        let slot = {
            let mut guard = match self.material_files.try_lock() {
                Ok(g) => g,
                Err(_) => return unavailable(),
            };
            let Some(files) = guard.as_mut() else {
                return unavailable();
            };
            files.sweep(&self.service);
            files.find(upload_id)
        };
        let Some(slot) = slot else {
            return failure(409, responses::ErrorCode::UploadExpired);
        };
        let mut started_transfer = false;
        let result = (|| -> Result<u64, WebResponse> {
            let mut slot = slot
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
            let phase = std::mem::replace(&mut slot.phase, Phase::Consuming);
            let Phase::Waiting(mut file) = phase else {
                return Err(failure(409, responses::ErrorCode::UploadNotReady));
            };
            started_transfer = true;
            let start = Instant::now();
            let mut count = 0u64;
            let mut chunk = [0u8; CHUNK];
            loop {
                if start.elapsed() >= STREAM_TIME
                    || Instant::now() >= slot.expires
                    || self
                        .service
                        .check_material_lease(lease, slot.scope, &[])
                        .is_err()
                {
                    return Err(failure(403, responses::ErrorCode::Forbidden));
                }
                let remaining = slot.bytes - count;
                let budget = usize::try_from(remaining.min(CHUNK as u64)).unwrap();
                if budget == 0 {
                    break;
                }
                let n = input
                    .read(&mut chunk[..budget])
                    .map_err(|_| failure(400, responses::ErrorCode::InvalidResource))?;
                if n == 0 {
                    return Err(failure(400, responses::ErrorCode::InvalidResource));
                }
                if start.elapsed() >= STREAM_TIME
                    || Instant::now() >= slot.expires
                    || self
                        .service
                        .check_material_lease(lease, slot.scope, &[])
                        .is_err()
                {
                    return Err(failure(403, responses::ErrorCode::Forbidden));
                }
                file.file_mut()
                    .write_all(&chunk[..n])
                    .map_err(|_| unavailable())?;
                count += n as u64;
            }
            // Full framed body must end here; no truncated or overlong descriptor may
            // become ready. This adapter's input is an owned finite local stream.
            let mut extra = [0u8; 1];
            if input.read(&mut extra).map_err(|_| unavailable())? != 0 {
                return Err(failure(400, responses::ErrorCode::InvalidResource));
            }
            if start.elapsed() >= STREAM_TIME
                || Instant::now() >= slot.expires
                || self
                    .service
                    .check_material_lease(lease, slot.scope, &[])
                    .is_err()
            {
                return Err(failure(403, responses::ErrorCode::Forbidden));
            }
            let stage = file.finish().map_err(|_| unavailable())?;
            let pinned =
                LockedFile::open_prepared(&stage, MAX_MATERIAL_BYTES).map_err(|_| unavailable())?;
            if pinned.len().map_err(|_| unavailable())? != slot.bytes {
                return Err(failure(400, responses::ErrorCode::InvalidResource));
            }
            slot.phase = Phase::Ready(pinned);
            Ok(count)
        })();
        match result {
            Ok(bytes) => response(200, wire::uploaded(upload_id, bytes), None),
            Err(error) => {
                if started_transfer {
                    if let Ok(mut files) = self.material_files.lock() {
                        if let Some(files) = files.as_mut() {
                            files.remove(upload_id)
                        }
                    }
                }
                error
            }
        }
    }
    /// Long validation/publication phase. Invoke on the existing bounded file
    /// worker with NO Backend lock held, then pass its opaque result to commit.
    pub fn prepare_material_append(
        &self,
        head: &RequestHead,
        body: &[u8],
    ) -> Result<PreparedWebMaterial, WebResponse> {
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
        let owned_slot = {
            let mut guard = self.material_files.try_lock().map_err(|_| unavailable())?;
            let files = guard.as_mut().ok_or_else(unavailable)?;
            files.sweep(&self.service);
            files.find(input.upload_id)
        }
        .ok_or_else(|| failure(409, responses::ErrorCode::UploadExpired))?;
        let mut slot = owned_slot
            .try_lock()
            .map_err(|_| failure(429, responses::ErrorCode::Capacity))?;
        if slot.lease != lease || slot.scope != scope {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        if Instant::now() >= slot.expires {
            return Err(failure(409, responses::ErrorCode::UploadExpired));
        }
        if !matches!(slot.phase, Phase::Ready(_)) {
            return Err(failure(409, responses::ErrorCode::UploadNotReady));
        }
        let phase = std::mem::replace(&mut slot.phase, Phase::Consuming);
        let Phase::Ready(file) = phase else {
            return Err(failure(409, responses::ErrorCode::UploadNotReady));
        };
        let mut preparation = Preparation {
            files: slot.files.clone(),
            scope,
            id: slot.material_id,
            version: slot.version,
            name: slot.name.clone(),
            expected: slot.bytes,
            control: Control {
                service: &self.service,
                lease,
                scope,
                classes: &input.allowed_class_ids,
                start: Instant::now(),
                expires: slot.expires,
                revision: None,
            },
            pin: None,
            path: slot.path.clone().ok_or_else(unavailable)?,
            keep: false,
        };
        let proof = prepare_material_append(
            &mut preparation,
            file,
            MaterialAppendMetadata {
                command_id: input.command_id,
                scope,
                material_id: slot.material_id,
                display_name: slot.name.clone(),
                allowed_class_ids: input.allowed_class_ids.clone(),
            },
        )
        .map_err(|e| materials::error(BackendError::Material(e), false))?;
        let result = PreparedWebMaterial {
            lease,
            scope,
            command_id: input.command_id,
            proof: Some(proof),
            pin: preparation.pin.take(),
            files: slot.files.clone(),
            path: preparation.path.clone(),
            attempted: false,
            service: self.service.clone(),
            registry: Arc::downgrade(&self.material_files),
            upload_id: input.upload_id,
            expires: slot.expires,
            _physical_owner: owned_slot.clone(),
        };
        // The final proof owns this inode now, so Slot::drop must not remove it.
        preparation.keep = true;
        slot.path = None;
        drop(preparation);
        drop(slot);
        Ok(result)
    }
    pub fn commit_material_append(
        &self,
        backend: &mut Backend,
        mut prepared: PreparedWebMaterial,
    ) -> WebResponse {
        if !self.service.same_instance(&backend.web_service())
            || !self.service.same_instance(&prepared.service)
        {
            return unavailable();
        }
        if Instant::now() >= prepared.expires
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
            Ok(record) => materials::committed(prepared.command_id, prepared.scope, &record),
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
        // Capture before reading metadata: an already queued writer may finish
        // after its caller timed out, despite the short Backend mutex section.
        let revision = backend
            .global_revision()
            .map_err(|e| materials::error(e, false))?;
        let record = backend
            .web_material_download_record(lease, scope, material_id, version)
            .map_err(|e| materials::error(e, false))?;
        let guard = self.material_files.try_lock().map_err(|_| unavailable())?;
        let files = guard.as_ref().ok_or_else(unavailable)?;
        let file = LockedFile::open(
            &path(&files.files, scope, material_id, &record.resource).map_err(|_| unavailable())?,
            MAX_MATERIAL_BYTES,
        )
        .map_err(|_| unavailable())?;
        let control = backend.session_control();
        if !download_checkpoint(&control, revision, Instant::now() + STREAM_TIME, || {
            self.service.check_material_lease(lease, scope, &[]).is_ok()
        }) {
            return Err(failure(403, responses::ErrorCode::Forbidden));
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
            service: self.service.clone(),
            revision,
            control,
            expires: Instant::now() + STREAM_TIME,
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
        if !self.verified {
            let check = Control {
                service: &self.service,
                lease: self.lease,
                scope: self.scope,
                classes: &[],
                start: Instant::now(),
                expires: self.expires,
                revision: Some((&self.control, self.revision)),
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
            download_checkpoint(&self.control, self.revision, self.expires, || {
                self.service
                    .check_material_lease(self.lease, self.scope, &[])
                    .is_ok()
            })
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
}
impl ValidationControl for Control<'_> {
    fn canceled(&self) -> bool {
        self.start.elapsed() >= Duration::from_secs(30)
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
            let _ = self.files.remove(&self.path);
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
