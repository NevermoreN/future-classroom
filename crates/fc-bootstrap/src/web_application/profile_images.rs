//! Bounded controlled image batches. File owners stay on shared workers; each
//! finite chunk uses the ordinary HTTP budget, never a socket reader or base64.
use super::shared_upload::{quarantine_cleanup, PhysicalUploadPermit};
use super::*;
use crate::{
    backend::{BackendError, MediaReservation, MediaState, ProfileContentOwner},
    profile_content_files::{ContentSources, ControlledContentFiles, WebImageInput},
    web_transfer::{ExecutionFence, TransferLifetime},
};
use fc_application::identity::SessionLease;
use fc_domain::{auth::Role, ids::*, profile_content::*};
use fc_ports::{
    command_window::IssuedCommand, http::RequestHead, prepare_profile_content, RandomSource,
};
use fc_protocol::web::admin::{self as wire, images, Resource};
use fc_runtime::worker::JobContext;
use fc_windows::materials::files::{ControlledFiles, LockedFile, NewFile};
use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
struct Stage {
    id: MediaVersionId,
    name: String,
    format: ImageFormat,
    bytes: u64,
    received: u64,
    path: PathBuf,
    identity: Option<(u32, u64)>,
    file: Option<NewFile>,
    last: Option<(u64, usize, [u8; 32])>,
}
struct Batch {
    id: CommandId,
    slot: ContentSlotKey,
    principal: PrincipalId,
    page: [u8; 32],
    token: [u8; 32],
    issued: IssuedCommand,
    admission: ContentAdmission,
    owner: ProfileContentOwner,
    reservation: Option<MediaReservation>,
    backend: Weak<Mutex<Backend>>,
    expires: Instant,
    stages: Vec<Stage>,
    permit: Option<PhysicalUploadPermit>,
}
struct MediaReader {
    principal: PrincipalId,
    page: [u8; 32],
    reference: MediaReference,
    file: LockedFile,
}
pub(super) struct ImageFiles {
    files: ControlledFiles,
    content: ControlledContentFiles,
    stages: PathBuf,
    batches: Mutex<Vec<Batch>>,
    readers: Mutex<Vec<MediaReader>>,
}
fn unavailable() -> WebResponse {
    failure(503, responses::ErrorCode::ServiceUnavailable)
}
fn invalid() -> WebResponse {
    failure(400, responses::ErrorCode::InvalidRequest)
}
fn format(name: &str) -> Option<ImageFormat> {
    match fc_domain::materials::MaterialKind::from_name(name) {
        Some(fc_domain::materials::MaterialKind::Png) => Some(ImageFormat::Png),
        Some(fc_domain::materials::MaterialKind::Jpeg) => Some(ImageFormat::Jpeg),
        Some(fc_domain::materials::MaterialKind::Bmp) => Some(ImageFormat::Bmp),
        _ => None,
    }
}
fn backend_error(error: BackendError) -> WebResponse {
    match error {
        BackendError::ProfileContent(fc_ports::ProfileContentRepositoryError::Domain(
            ProfileContentError::RevisionConflict { .. }
            | ProfileContentError::ViewRevisionConflict { .. },
        )) => failure(409, responses::ErrorCode::RevisionConflict),
        BackendError::ProfileContent(fc_ports::ProfileContentRepositoryError::Domain(
            ProfileContentError::Expired,
        )) => failure(410, responses::ErrorCode::Expired),
        BackendError::ProfileContent(fc_ports::ProfileContentRepositoryError::QuotaExceeded) => {
            failure(429, responses::ErrorCode::Capacity)
        }
        BackendError::ProfileContent(_) => invalid(),
        other => admin::backend_error(other),
    }
}
impl ImageFiles {
    fn cleanup(&self, batch: &mut Batch) -> bool {
        for stage in &mut batch.stages {
            if stage.identity.is_none() {
                let Some(file) = stage.file.as_ref() else {
                    return false;
                };
                let Ok(identity) = file.identity() else {
                    return false;
                };
                stage.identity = Some(identity);
            }
            stage.file.take();
        }
        for stage in &batch.stages {
            if self
                .files
                .remove_object_expected(
                    &stage.path,
                    false,
                    stage
                        .identity
                        .expect("stage identity retained before close"),
                    |_| true,
                )
                .is_err()
            {
                return false;
            }
        }
        let Some(backend) = batch.backend.upgrade() else {
            return false;
        };
        {
            let Ok(mut locked) = backend.lock() else {
                return false;
            };
            if locked
                .fail_profile_content_owned(batch.owner.clone())
                .is_err()
            {
                return false;
            }
            if let Some(reservation) = &batch.reservation {
                if locked.settle_profile_media(reservation.clone()).is_err() {
                    return false;
                }
            }
        }
        if let Some(reservation) = &batch.reservation {
            for id in &reservation.ids {
                let ticket = {
                    let Ok(locked) = backend.lock() else {
                        return false;
                    };
                    let after = if id.get() > 1 {
                        MediaVersionId::new(id.get() - 1).ok()
                    } else {
                        None
                    };
                    let entry = match locked.inventory_profile_media(after, 1) {
                        Ok(page) => page.items.into_iter().find(|entry| entry.id == *id),
                        Err(_) => return false,
                    };
                    match entry {
                        Some(entry)
                            if matches!(
                                entry.state,
                                MediaState::Unreferenced | MediaState::Retiring
                            ) && entry.refs == 0 =>
                        {
                            match locked.retire_profile_media(entry.id, entry.revision) {
                                Ok(ticket) => Some(ticket),
                                Err(_) => return false,
                            }
                        }
                        _ => None,
                    }
                };
                if let Some(ticket) = ticket {
                    if self.content.remove_retired(&ticket).is_err() {
                        return false;
                    }
                    let Ok(locked) = backend.lock() else {
                        return false;
                    };
                    if locked.finalize_profile_media_removal(ticket).is_err() {
                        return false;
                    }
                }
            }
        }
        true
    }
    fn retire(self: &Arc<Self>, mut batch: Batch) -> bool {
        if self.cleanup(&mut batch) {
            batch.permit.take();
            true
        } else {
            if let Some(permit) = batch.permit.take() {
                let files = self.clone();
                quarantine_cleanup(permit, move || files.cleanup(&mut batch));
            }
            false
        }
    }
    fn reference(
        backend: &Backend,
        lease: SessionLease,
        slot: ContentSlotKey,
        id: MediaVersionId,
    ) -> Result<MediaReference, WebResponse> {
        let document = backend
            .read_profile_content(lease, slot)
            .map_err(backend_error)?;
        document
            .current
            .iter()
            .chain(document.previous.iter())
            .filter_map(|version| match &version.payload {
                ContentPayload::Images(pages) => Some(pages),
                _ => None,
            })
            .flatten()
            .find(|page| page.version_id == id && page.owner == slot)
            .cloned()
            .ok_or_else(|| failure(404, responses::ErrorCode::NotFound))
    }
}
impl WebApplication {
    pub(crate) fn configure_profile_images(&mut self, root: &Path) -> io::Result<()> {
        let files = ControlledFiles::new(root).map_err(io::Error::other)?;
        let stages = files
            .directory(&["quarantine".into(), "web-profile-images".into()])
            .map_err(io::Error::other)?;
        // Exclusive startup owns no Web batch yet; retire only our exact numeric stages.
        for path in files.entries(&stages, 101).map_err(io::Error::other)? {
            let name = path
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or_else(|| io::Error::other("invalid upload stage"))?;
            if name
                .strip_prefix("image-")
                .and_then(|v| v.strip_suffix(".part"))
                .is_none_or(|v| v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(io::Error::other("unowned upload stage"));
            }
            files.remove(&path).map_err(io::Error::other)?;
        }
        let content = ControlledContentFiles::new(root)
            .map_err(|_| io::Error::other("profile content files unavailable"))?;
        *self
            .profile_images
            .lock()
            .map_err(|_| io::Error::other("image files"))? = Some(Arc::new(ImageFiles {
            files,
            content,
            stages,
            batches: Mutex::new(Vec::new()),
            readers: Mutex::new(Vec::new()),
        }));
        Ok(())
    }
    pub(crate) fn cleanup_expired_profile_images(&self) {
        let Some(files) = self
            .profile_images
            .lock()
            .ok()
            .and_then(|files| files.clone())
        else {
            return;
        };
        let expired = {
            let Ok(mut batches) = files.batches.lock() else {
                return;
            };
            let mut expired = Vec::new();
            let mut index = 0;
            while index < batches.len() {
                if Instant::now() >= batches[index].expires {
                    expired.push(batches.remove(index));
                } else {
                    index += 1
                }
            }
            expired
        };
        for batch in expired {
            files.retire(batch);
        }
    }
    pub(crate) fn cleanup_profile_images(&self) -> bool {
        let Some(files) = self
            .profile_images
            .lock()
            .ok()
            .and_then(|files| files.clone())
        else {
            return true;
        };
        if let Ok(mut readers) = files.readers.lock() {
            readers.clear();
        } else {
            return false;
        }
        let batches = match files.batches.lock() {
            Ok(mut batches) => std::mem::take(&mut *batches),
            Err(_) => return false,
        };
        let mut done = true;
        for batch in batches {
            done = files.retire(batch) && done;
        }
        done
    }
    pub(crate) fn is_profile_image_request(&self, head: &RequestHead, body: &[u8]) -> bool {
        match self.route_hint(head) {
            Ok(Route::Admin {
                resource:
                    Resource::ImageManifest { .. }
                    | Resource::ImageBytes { .. }
                    | Resource::ImageCancel { .. }
                    | Resource::ImageMedia { .. },
                ..
            }) => true,
            Ok(Route::Admin {
                resource: Resource::ProfileContent { .. },
                write: true,
            }) => wire::Mutation::decode(body).is_ok_and(|input| input.action == "publishImages"),
            _ => false,
        }
    }
    pub(crate) fn profile_images_in_job(
        &self,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        body: &[u8],
        life: &TransferLifetime,
        context: &JobContext,
    ) -> WebResponse {
        if !life.is_active() || context.checkpoint().is_err() {
            return unavailable();
        }
        let fence = life.job_fence(context);
        let app = self.for_execution(fence.clone());
        let route = match app.boundary.preflight(head) {
            Ok(route) => route,
            Err(error) => return app.request_boundary_error(head, error),
        };
        if body.len() as u64 != head.content_length.unwrap_or(0)
            || body.len() as u64 > route.body_limit()
        {
            return invalid();
        }
        let session = {
            let Ok(backend) = backend.lock() else {
                return unavailable();
            };
            if !app.service.same_instance(&backend.web_service()) {
                return unavailable();
            };
            match app.authenticate_route(head, route) {
                Ok(Some(session)) if session.account.principal.role == Role::GlobalAdmin => session,
                Ok(_) => return failure(403, responses::ErrorCode::Forbidden),
                Err(error) => return error,
            }
        };
        let Some(page) = head
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case("x-fc-page"))
        else {
            return failure(401, responses::ErrorCode::Unauthorized);
        };
        let page = fc_security::sha256(&page.value);
        let lease = session.lease();
        let principal = session.account.principal.id;
        let Some(files) = app
            .profile_images
            .lock()
            .ok()
            .and_then(|files| files.clone())
        else {
            return unavailable();
        };
        let expired = {
            let Ok(mut batches) = files.batches.lock() else {
                return unavailable();
            };
            let mut expired = Vec::new();
            let mut index = 0;
            while index < batches.len() {
                if Instant::now() >= batches[index].expires {
                    expired.push(batches.remove(index));
                } else {
                    index += 1
                }
            }
            expired
        };
        for batch in expired {
            files.retire(batch);
        }
        let result = match route {
            Route::Admin {
                resource: Resource::ImageManifest { slot },
                write: true,
            } => {
                let (expected, items) = match images::manifest(body) {
                    Ok(value) => value,
                    Err(_) => return invalid(),
                };
                if items.iter().any(|item| format(&item.name).is_none()) {
                    return invalid();
                }
                let Some(permit) = PhysicalUploadPermit::take() else {
                    return failure(429, responses::ErrorCode::Capacity);
                };
                let mut token = [0; 32];
                if fc_windows::SystemRandom.fill(&mut token).is_err() {
                    return unavailable();
                }
                let sizes = items.iter().map(|item| item.bytes).collect::<Vec<_>>();
                let (issued, admission, owner, reservation) = {
                    let Ok(mut locked) = backend.lock() else {
                        return unavailable();
                    };
                    let mut scoped = locked.begin_execution_scope(fence.clone());
                    match scoped.begin_web_profile_images(lease, slot, expected, token, &sizes) {
                        Ok(value) => value,
                        Err(error) => return backend_error(error),
                    }
                };
                let mut batch = Batch {
                    id: issued.command_id,
                    slot,
                    principal,
                    page,
                    token,
                    issued,
                    admission,
                    owner,
                    reservation,
                    backend: Arc::downgrade(backend),
                    expires: Instant::now() + Duration::from_secs(120),
                    stages: Vec::new(),
                    permit: Some(permit),
                };
                let ids = batch
                    .reservation
                    .as_ref()
                    .map(|reservation| reservation.ids.clone())
                    .unwrap_or_default();
                for (item, id) in items.into_iter().zip(ids) {
                    let path = files.stages.join(format!("image-{}.part", id.get()));
                    let file = match files.files.create_new(&path) {
                        Ok(file) => file,
                        Err(_) => {
                            files.retire(batch);
                            return unavailable();
                        }
                    };
                    let identity = file.identity().ok();
                    batch.stages.push(Stage {
                        id,
                        name: item.name.clone(),
                        format: format(&item.name).expect("checked image kind"),
                        bytes: item.bytes,
                        received: 0,
                        path,
                        identity,
                        file: Some(file),
                        last: None,
                    });
                    if identity.is_none() {
                        files.retire(batch);
                        return unavailable();
                    }
                }
                let reply = images::created(
                    batch.id,
                    &batch
                        .stages
                        .iter()
                        .map(|s| (s.id, s.name.clone(), s.bytes))
                        .collect::<Vec<_>>(),
                );
                let Ok(mut batches) = files.batches.lock() else {
                    files.retire(batch);
                    return unavailable();
                };
                batches.push(batch);
                response(201, reply, None)
            }
            Route::Admin {
                resource:
                    Resource::ImageBytes {
                        slot,
                        batch,
                        upload,
                    },
                write: true,
            } => {
                let values = head
                    .headers
                    .iter()
                    .filter(|h| h.name.eq_ignore_ascii_case("x-fc-upload-offset"))
                    .collect::<Vec<_>>();
                if values.len() != 1 {
                    return invalid();
                }
                let offset = match std::str::from_utf8(&values[0].value)
                    .ok()
                    .and_then(|value| wire::revision(value).ok())
                    .map(|v| v.get())
                {
                    Some(value) => value,
                    None => return invalid(),
                };
                let Ok(mut batches) = files.batches.lock() else {
                    return unavailable();
                };
                let Some(batch) = batches.iter_mut().find(|candidate| {
                    candidate.id == batch
                        && candidate.slot == slot
                        && candidate.principal == principal
                        && candidate.page == page
                }) else {
                    return failure(404, responses::ErrorCode::NotFound);
                };
                let Some(stage) = batch.stages.iter_mut().find(|stage| stage.id == upload) else {
                    return invalid();
                };
                let hash = fc_security::sha256(body);
                if stage.last == Some((offset, body.len(), hash)) {
                    response(200, images::accepted(), None)
                } else {
                    if stage.received != offset
                        || body.is_empty()
                        || body.len() > images::CHUNK_BYTES
                        || offset
                            .checked_add(body.len() as u64)
                            .is_none_or(|end| end > stage.bytes)
                    {
                        return failure(409, responses::ErrorCode::RevisionConflict);
                    }
                    if !fence() || !app.service.web_session_current(lease) {
                        return failure(401, responses::ErrorCode::Unauthorized);
                    }
                    if stage
                        .file
                        .as_mut()
                        .is_none_or(|file| file.file_mut().write_all(body).is_err())
                    {
                        return unavailable();
                    }
                    stage.received += body.len() as u64;
                    stage.last = Some((offset, body.len(), hash));
                    response(200, images::accepted(), None)
                }
            }
            Route::Admin {
                resource: Resource::ImageCancel { slot, batch },
                write: true,
            } => {
                if images::empty(body).is_err() {
                    return invalid();
                }
                let batch = {
                    let Ok(mut batches) = files.batches.lock() else {
                        return unavailable();
                    };
                    let Some(index) = batches.iter().position(|candidate| {
                        candidate.id == batch
                            && candidate.slot == slot
                            && candidate.principal == principal
                            && candidate.page == page
                    }) else {
                        return failure(404, responses::ErrorCode::NotFound);
                    };
                    batches.remove(index)
                };
                if files.retire(batch) {
                    response(200, images::cancelled(), None)
                } else {
                    unavailable()
                }
            }
            Route::Admin {
                resource: Resource::ProfileContent { slot },
                write: true,
            } => app.publish_images(backend, &files, lease, principal, page, slot, body, &fence),
            Route::Admin {
                resource: Resource::ImageMedia { slot, media },
                write: false,
            } => app.image_media(
                backend, &files, lease, principal, page, slot, media, head, &fence,
            ),
            _ => invalid(),
        };
        if !fence() {
            failure(503, responses::ErrorCode::ResultUnknown)
        } else {
            result
        }
    }
    #[allow(clippy::too_many_arguments)] // Exact request, page and physical-owner facts are kept together through commit.
    fn publish_images(
        &self,
        backend: &Arc<Mutex<Backend>>,
        files: &Arc<ImageFiles>,
        lease: SessionLease,
        principal: PrincipalId,
        page: [u8; 32],
        slot: ContentSlotKey,
        body: &[u8],
        fence: &ExecutionFence,
    ) -> WebResponse {
        let (expected, input) = match images::publication(body) {
            Ok(value) => value,
            Err(_) => return invalid(),
        };
        let mut batch = if let Some(id) = input
            .batch_id
            .as_ref()
            .and_then(|id| wire::id(id).ok())
            .and_then(|id| CommandId::new(id).ok())
        {
            let Ok(mut batches) = files.batches.lock() else {
                return unavailable();
            };
            let Some(index) = batches.iter().position(|b| {
                b.id == id && b.slot == slot && b.principal == principal && b.page == page
            }) else {
                return failure(404, responses::ErrorCode::NotFound);
            };
            batches.remove(index)
        } else {
            let Some(permit) = PhysicalUploadPermit::take() else {
                return failure(429, responses::ErrorCode::Capacity);
            };
            let mut token = [0; 32];
            if fc_windows::SystemRandom.fill(&mut token).is_err() {
                return unavailable();
            }
            let (issued, admission, owner, reservation) = {
                let Ok(mut locked) = backend.lock() else {
                    return unavailable();
                };
                let mut scoped = locked.begin_execution_scope(fence.clone());
                match scoped.begin_web_profile_images(lease, slot, expected, token, &[]) {
                    Ok(value) => value,
                    Err(error) => return backend_error(error),
                }
            };
            Batch {
                id: issued.command_id,
                slot,
                principal,
                page,
                token,
                issued,
                admission,
                owner,
                reservation,
                backend: Arc::downgrade(backend),
                expires: Instant::now() + Duration::from_secs(120),
                stages: Vec::new(),
                permit: Some(permit),
            }
        };
        let result = (|| {
            if batch.admission.request.expected_revision != expected {
                return failure(409, responses::ErrorCode::RevisionConflict);
            }
            let mut sources = Vec::new();
            let mut ids = Vec::new();
            let mut sizes = Vec::new();
            for item in input.pages {
                match item {
                    images::Page::Existing { media_version_id } => {
                        let id =
                            MediaVersionId::new(wire::id(&media_version_id).expect("validated id"))
                                .expect("validated id");
                        let reference = {
                            let Ok(locked) = backend.lock() else {
                                return unavailable();
                            };
                            match ImageFiles::reference(&locked, lease, slot, id) {
                                Ok(reference) => reference,
                                Err(error) => return error,
                            }
                        };
                        sources.push(WebImageInput::Existing(reference));
                    }
                    images::Page::Upload { upload_id } => {
                        let id = MediaVersionId::new(wire::id(&upload_id).expect("validated id"))
                            .expect("validated id");
                        let Some(stage) = batch.stages.iter_mut().find(|stage| stage.id == id)
                        else {
                            return invalid();
                        };
                        if stage.received != stage.bytes {
                            return invalid();
                        }
                        let Some(file) = stage.file.take() else {
                            return invalid();
                        };
                        let source = match file.seal() {
                            Ok(file) => file,
                            Err(_) => return unavailable(),
                        };
                        ids.push(id);
                        sizes.push(stage.bytes);
                        sources.push(WebImageInput::Uploaded {
                            source,
                            format: stage.format,
                        });
                    }
                }
            }
            let reservation = if ids.is_empty() {
                None
            } else {
                Some(MediaReservation {
                    admission: batch.admission.clone(),
                    ids,
                    bytes: sizes,
                })
            };
            let sources = match ContentSources::from_web_images(sources, batch.token) {
                Ok(sources) => sources,
                Err(_) => return invalid(),
            };
            let guard = {
                let Ok(locked) = backend.lock() else {
                    return unavailable();
                };
                match locked.profile_content_preparation_guard(lease) {
                    Ok(guard) => guard,
                    Err(error) => return backend_error(error),
                }
            };
            let state = || {
                use crate::{
                    backend::ProfileContentPreparationState as B,
                    profile_content_files::PreparationState as P,
                };
                if !fence() {
                    return P::Revoked;
                }
                match guard.state() {
                    B::Ready => P::Ready,
                    B::AuthorityUpdating => P::AuthorityUpdating,
                    B::Revoked => P::Revoked,
                }
            };
            let mut preparation = files
                .content
                .preparation_while(reservation.as_ref(), &state);
            let prepared =
                match prepare_profile_content(&mut preparation, batch.admission.clone(), sources) {
                    Ok(value) => value,
                    Err(_) => return invalid(),
                };
            let result = {
                let Ok(mut locked) = backend.lock() else {
                    return unavailable();
                };
                let mut scoped = locked.begin_execution_scope(fence.clone());
                let mut command = match scoped.begin_command_scope(batch.issued) {
                    Ok(command) => command,
                    Err(error) => return backend_error(error),
                };
                match command.commit_profile_content_while(lease, prepared, fence.clone()) {
                    Ok(_) => match command.read_profile_content(lease, slot) {
                        Ok(document) => admin::data_response(wire::profile_content::document(
                            &document,
                            ContentPayloadKind::Images,
                            command.web_profile_content_can_page(lease, &document),
                        )),
                        Err(error) => backend_error(error),
                    },
                    Err(error) => backend_error(error),
                }
            };
            drop(preparation);
            result
        })();
        if !files.retire(batch) {
            return failure(503, responses::ErrorCode::ResultUnknown);
        }
        result
    }
    #[allow(clippy::too_many_arguments)] // Read authority and immutable media identity are explicit at this boundary.
    fn image_media(
        &self,
        backend: &Arc<Mutex<Backend>>,
        files: &Arc<ImageFiles>,
        lease: SessionLease,
        principal: PrincipalId,
        page: [u8; 32],
        slot: ContentSlotKey,
        media: MediaVersionId,
        head: &RequestHead,
        fence: &ExecutionFence,
    ) -> WebResponse {
        let reference = {
            let Ok(backend) = backend.lock() else {
                return unavailable();
            };
            match ImageFiles::reference(&backend, lease, slot, media) {
                Ok(reference) => reference,
                Err(error) => return error,
            }
        };
        let ranges = head
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("range"))
            .collect::<Vec<_>>();
        if ranges.len() != 1 {
            return failure(416, responses::ErrorCode::InvalidRequest);
        }
        let Some((start, end)) = std::str::from_utf8(&ranges[0].value)
            .ok()
            .and_then(|range| range.strip_prefix("bytes="))
            .and_then(|range| range.split_once('-'))
            .and_then(|(start, end)| Some((start.parse::<u64>().ok()?, end.parse::<u64>().ok()?)))
        else {
            return invalid();
        };
        if end < start || end >= reference.bytes || end - start >= images::CHUNK_BYTES as u64 {
            return failure(416, responses::ErrorCode::InvalidRequest);
        }
        let mut readers = match files.readers.lock() {
            Ok(readers) => readers,
            Err(_) => return unavailable(),
        };
        let index = match readers.iter().position(|reader| {
            reader.principal == principal && reader.page == page && reader.reference == reference
        }) {
            Some(index) => index,
            None => {
                let file = match files.content.open_web_media(&reference, &|| {
                    fence() && self.service.web_session_current(lease)
                }) {
                    Ok(file) => file,
                    Err(_) => return unavailable(),
                };
                if readers.len() >= 2 {
                    readers.remove(0);
                }
                readers.push(MediaReader {
                    principal,
                    page,
                    reference: reference.clone(),
                    file,
                });
                readers.len() - 1
            }
        };
        if !fence() || !self.service.web_session_current(lease) {
            return failure(401, responses::ErrorCode::Unauthorized);
        }
        let mut body = vec![0; (end - start + 1) as usize];
        let file = readers[index].file.file_mut();
        if file.seek(SeekFrom::Start(start)).is_err() || file.read_exact(&mut body).is_err() {
            return unavailable();
        }
        let mime = match reference.format {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Bmp => "image/bmp",
        };
        WebResponse {
            head: ResponseHead {
                status: 206,
                content_length: body.len() as u64,
                headers: vec![
                    Header {
                        name: "Content-Type".into(),
                        value: mime.as_bytes().to_vec(),
                    },
                    Header {
                        name: "Cache-Control".into(),
                        value: b"no-store".to_vec(),
                    },
                    Header {
                        name: "Accept-Ranges".into(),
                        value: b"bytes".to_vec(),
                    },
                    Header {
                        name: "Content-Range".into(),
                        value: format!("bytes {start}-{end}/{}", reference.bytes).into_bytes(),
                    },
                ],
            },
            body,
        }
    }
}
