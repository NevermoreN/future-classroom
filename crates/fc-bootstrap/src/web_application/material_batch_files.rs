//! A group owns the original physical permit and immutable manifest until all
//! members AND their actual worker calls retire. Hub keeps its original slot.
use super::*;
use fc_ports::prepare_material_batch_append;
use fc_protocol::web::command_window::CommandReference;

pub(super) struct BatchGroup {
    permit: Option<PhysicalUploadPermit>,
    id: u64,
    lease: SessionLease,
    scope: MaterialScope,
    classes: Vec<ClassId>,
    members: Vec<Arc<Mutex<Slot>>>,
    expires: Instant,
    next: usize,
    serial: u64,
    active: Option<(usize, RequestToken, u64)>,
    last_life: Option<TransferLifetime>,
    cancelled: bool,
    attempted: bool,
    original: Option<CommandReference>,
    temporary_bytes: u64,
}
impl BatchGroup {
    fn check(&self, service: &WebService) -> Result<(), WebResponse> {
        if self.cancelled
            || Instant::now() >= self.expires
            || self.temporary_bytes > MAX_MATERIAL_BATCH_FILES as u64 * MAX_MATERIAL_BYTES
        {
            return Err(failure(409, responses::ErrorCode::UploadExpired));
        }
        service
            .check_material_lease(self.lease, self.scope, &self.classes)
            .map_err(authentication_error)
    }
    fn cleanup(&mut self) -> bool {
        self.cancelled = true;
        // An executing helper/receiver or attempted SQL transaction cannot be
        // retired by a flag, TTL, NOT_FOUND, or a competing cancel request.
        if self.active.is_some() || self.attempted {
            return false;
        }
        cleanup_members(&self.members)
    }
    fn keep_published(&mut self) {
        for member in &self.members {
            let mut member = member
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            member.path = None;
            member.phase = Phase::Consuming;
        }
        self.attempted = false;
        self.cancelled = true;
    }
}
fn cleanup_members(members: &[Arc<Mutex<Slot>>]) -> bool {
    // First close every real member pin. No cleanup can return the aggregate
    // permit while another member of this same group still owns a live handle.
    for member in members {
        let Ok(mut member) = member.try_lock() else {
            return false;
        };
        member.phase = Phase::Consuming;
    }
    let mut clean = true;
    for member in members {
        let Ok(mut member) = member.try_lock() else {
            clean = false;
            continue;
        };
        if let Some(path) = &member.path {
            if member
                .files
                .remove_object_expected(path, false, member.identity, |_| true)
                .is_ok()
            {
                member.path = None;
            } else {
                clean = false;
            }
        }
    }
    clean
}
impl Drop for BatchGroup {
    fn drop(&mut self) {
        let Some(permit) = self.permit.take() else {
            return;
        };
        let members = std::mem::take(&mut self.members);
        if self.attempted {
            // No receipt established commit truth. Retain this exact original
            // charge and bounded member set, including pins, until process exit.
            quarantine_cleanup(permit, move || {
                let _ = &members;
                false
            });
        } else if !cleanup_members(&members) {
            quarantine_cleanup(permit, move || cleanup_members(&members));
        } else {
            drop(members);
            drop(permit);
        }
    }
}
pub(super) fn valid_stage_name(name: &str) -> bool {
    let canonical = |s: &str| {
        s.parse::<u64>()
            .ok()
            .filter(|id| *id != 0)
            .is_some_and(|id| s == id.to_string())
    };
    let Some(inner) = name
        .strip_prefix("web-material-")
        .and_then(|s| s.strip_suffix(".part"))
    else {
        return false;
    };
    if let Some(batch) = inner.strip_prefix("batch-") {
        batch
            .split_once('-')
            .is_some_and(|(group, member)| canonical(group) && canonical(member))
    } else {
        canonical(inner)
    }
}
pub(super) fn valid_stage_set<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    let mut groups = std::collections::BTreeMap::<String, usize>::new();
    let mut total = 0usize;
    for name in names {
        if !valid_stage_name(name) {
            return false;
        }
        let body = name
            .strip_prefix("web-material-")
            .and_then(|s| s.strip_suffix(".part"))
            .expect("validated name");
        let (owner, limit) = if let Some(batch) = body.strip_prefix("batch-") {
            let (group, _) = batch.split_once('-').expect("validated batch");
            (format!("b{group}"), MAX_MATERIAL_BATCH_FILES)
        } else {
            (format!("s{body}"), 1)
        };
        let count = groups.entry(owner).or_default();
        *count += 1;
        total += 1;
        if *count > limit || groups.len() > 2 || total > 2 * MAX_MATERIAL_BATCH_FILES {
            return false;
        }
    }
    true
}

fn remove_group(
    registry: &std::sync::Weak<Mutex<Option<MaterialFiles>>>,
    id: u64,
    owner: &Arc<Mutex<BatchGroup>>,
) {
    if let Some(registry) = registry.upgrade() {
        if let Ok(mut registry) = registry.lock() {
            if let Some(files) = registry.as_mut() {
                files
                    .batches
                    .retain(|(saved, group)| *saved != id || !Arc::ptr_eq(group, owner));
            }
        }
    }
}
impl WebApplication {
    fn batch_group(&self, id: u64) -> Result<Arc<Mutex<BatchGroup>>, WebResponse> {
        self.material_files
            .try_lock()
            .map_err(|_| unavailable())?
            .as_ref()
            .and_then(|files| {
                files
                    .batches
                    .iter()
                    .find(|(saved, _)| *saved == id)
                    .map(|(_, g)| g.clone())
            })
            .ok_or_else(|| failure(409, responses::ErrorCode::UploadExpired))
    }
    pub(crate) fn material_batch_create(
        &self,
        _backend: &mut Backend,
        _lease: SessionLease,
        _scope: MaterialScope,
        _input: wire::BatchUploadInput,
    ) -> WebResponse {
        unavailable()
    }
    pub(crate) fn material_batch_create_in_job(
        &self,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        body: &[u8],
        life: &TransferLifetime,
        context: &JobContext,
    ) -> Result<(u64, WebResponse), WebResponse> {
        if head.token != life.token()
            || !life.is_active()
            || context.checkpoint().is_err()
            || context.deadline() > life.deadline()
        {
            return Err(unavailable());
        }
        let (lease, route) = self.material_authenticate(head)?;
        let MaterialRoute::BatchUploadCreate { scope } = route else {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        };
        let input = wire::batch_upload(body)
            .map_err(|_| failure(400, responses::ErrorCode::InvalidRequest))?;
        let ids = {
            let mut backend = backend.try_lock().map_err(|_| unavailable())?;
            if !self.service.same_instance(&backend.web_service()) {
                return Err(unavailable());
            }
            backend
                .web_material_authorize(lease, scope, &input.allowed_class_ids)
                .map_err(|e| materials::error(e, false))?;
            let mut ids = Vec::with_capacity(input.files.len() + 1);
            for _ in 0..=input.files.len() {
                ids.push(
                    backend
                        .web_material_identifiers(lease, scope)
                        .map_err(|e| materials::error(e, false))?,
                );
            }
            ids
        };
        let id = ids[0].0;
        // Original Backend mutex is gone before any real stage creation.
        let response =
            self.open_material_batch(lease, scope, input, ids, life, &context.execution_fence());
        if response.head.status == 201 {
            Ok((id, response))
        } else {
            Err(response)
        }
    }
    fn open_material_batch(
        &self,
        lease: SessionLease,
        scope: MaterialScope,
        input: wire::BatchUploadInput,
        ids: Vec<(u64, MaterialId, ResourceVersionId)>,
        life: &TransferLifetime,
        current: &dyn Fn() -> bool,
    ) -> WebResponse {
        let result = (|| -> Result<_, WebResponse> {
            self.service
                .check_material_lease(lease, scope, &input.allowed_class_ids)
                .map_err(authentication_error)?;
            if !current() || !life.is_active() {
                return Err(unavailable());
            }
            let mut identifiers = ids.into_iter();
            if input.files.is_empty()
                || input.files.len() > MAX_MATERIAL_BATCH_FILES
                || input.allowed_class_ids.is_empty()
            {
                return Err(failure(400, responses::ErrorCode::InvalidRequest));
            }
            let mut bytes = 0u64;
            for file in &input.files {
                validate_material_name(&file.display_name)
                    .map_err(|_| failure(400, responses::ErrorCode::InvalidRequest))?;
                let kind = MaterialKind::from_name(&file.display_name).ok_or_else(unavailable)?;
                if file.bytes == 0
                    || file.bytes > MAX_MATERIAL_BYTES
                    || matches!(
                        kind,
                        MaterialKind::Png | MaterialKind::Jpeg | MaterialKind::Bmp
                    ) && file.bytes > 10 * 1024 * 1024
                {
                    return Err(failure(413, responses::ErrorCode::InvalidResource));
                }
                bytes = bytes
                    .checked_add(file.bytes)
                    .filter(|n| *n <= MAX_MATERIAL_BATCH_FILES as u64 * MAX_MATERIAL_BYTES)
                    .ok_or_else(unavailable)?;
            }
            self.sweep_material_uploads().map_err(|_| unavailable())?;
            let files = {
                let registry = self.material_files.try_lock().map_err(|_| unavailable())?;
                let files = registry.as_ref().ok_or_else(unavailable)?;
                if files.slots.len() + files.batches.len() >= 2 {
                    return Err(failure(429, responses::ErrorCode::Capacity));
                }
                files.files.clone()
            };
            let permit = PhysicalUploadPermit::take()
                .ok_or_else(|| failure(429, responses::ErrorCode::Capacity))?;
            let (id, _, _) = identifiers.next().ok_or_else(unavailable)?;
            let group = Arc::new(Mutex::new(BatchGroup {
                permit: Some(permit),
                id,
                lease,
                scope,
                classes: input.allowed_class_ids,
                members: Vec::with_capacity(input.files.len()),
                expires: life
                    .admitted_at()
                    .checked_add(TTL)
                    .ok_or_else(unavailable)?,
                next: 0,
                serial: 0,
                active: None,
                last_life: Some(life.clone()),
                cancelled: false,
                attempted: false,
                original: None,
                temporary_bytes: bytes,
            }));
            let mut ids = std::collections::HashSet::new();
            let expires = group_deadline(&group)?;
            for input in input.files {
                if !current() || !life.is_active() {
                    return Err(unavailable());
                }
                let (_, material_id, version) = identifiers.next().ok_or_else(unavailable)?;
                if !ids.insert(version.get()) {
                    return Err(unavailable());
                }
                let stage = files
                    .directory(&["web-material-staging".into()])
                    .map_err(|_| unavailable())?
                    .join(format!("web-material-batch-{id}-{}.part", version.get()));
                let file = files.create_new(&stage).map_err(|_| unavailable())?;
                // Register the original object before any later fallible work.
                // If identity capture fails, retain its real file and aggregate
                // charge by quarantining the whole bounded group.
                let identity = match file.identity() {
                    Ok(value) => value,
                    Err(_) => {
                        let mut file = Some(file);
                        let mut captured = None;
                        let held = group.clone();
                        // This group is not published in any registry. Its permit
                        // stays with the captured group until exact disposal succeeds.
                        let permit = held
                            .lock()
                            .map_err(|_| unavailable())?
                            .permit
                            .take()
                            .ok_or_else(unavailable)?;
                        let files = files.clone();
                        quarantine_cleanup(permit, move || {
                            if captured.is_none() {
                                captured = file.as_ref().and_then(|f| f.identity().ok());
                                if captured.is_none() {
                                    return false;
                                }
                                drop(file.take());
                            }
                            if files
                                .remove_object_expected(&stage, false, captured.unwrap(), |_| true)
                                .is_err()
                            {
                                return false;
                            }
                            held.lock().map(|mut g| g.cleanup()).unwrap_or(false)
                        });
                        return Err(unavailable());
                    }
                };
                group
                    .lock()
                    .map_err(|_| unavailable())?
                    .members
                    .push(Arc::new(Mutex::new(Slot {
                        _permit: None,
                        identity,
                        lease,
                        scope,
                        material_id,
                        version,
                        name: input.display_name,
                        bytes: input.bytes,
                        expires,
                        lifetime: None,
                        phase: Phase::Waiting(file),
                        path: Some(stage),
                        files: files.clone(),
                    })));
            }
            let encoded = {
                let g = group.lock().map_err(|_| unavailable())?;
                let mut members = Vec::with_capacity(g.members.len());
                let mut facts = Vec::with_capacity(g.members.len());
                for m in &g.members {
                    let m = m.lock().map_err(|_| unavailable())?;
                    facts.push((m.version.get(), m.name.clone(), m.bytes));
                }
                for (id, name, bytes) in &facts {
                    members.push(wire::BatchReadyMember {
                        upload_id: *id,
                        display_name: name,
                        bytes: *bytes,
                    });
                }
                wire::batch_ready(g.id, &members).map_err(|_| unavailable())?
            };
            self.service
                .check_material_lease(
                    lease,
                    scope,
                    &group.lock().map_err(|_| unavailable())?.classes,
                )
                .map_err(authentication_error)?;
            {
                let mut registry = self.material_files.try_lock().map_err(|_| unavailable())?;
                let registry = registry.as_mut().ok_or_else(unavailable)?;
                if registry.slots.len() + registry.batches.len() >= 2
                    || registry.batches.iter().any(|(saved, _)| *saved == id)
                {
                    return Err(failure(429, responses::ErrorCode::Capacity));
                }
                if !current() || !life.is_active() {
                    return Err(unavailable());
                }
                registry.batches.push((id, group));
            }
            Ok(encoded)
        })();
        match result {
            Ok(body) => response(201, Ok(body), None),
            Err(error) => error,
        }
    }
    pub(crate) fn sweep_material_batches(&self) -> io::Result<()> {
        let groups = {
            let registry = self.material_files.lock().map_err(io_error)?;
            registry
                .as_ref()
                .map(|f| f.batches.clone())
                .unwrap_or_default()
        };
        for (id, owner) in groups {
            let remove = {
                let Ok(mut group) = owner.try_lock() else {
                    continue;
                };
                if !group.cancelled
                    && Instant::now() < group.expires
                    && matches!(
                        self.service
                            .check_material_lease(group.lease, group.scope, &group.classes),
                        Ok(()) | Err(WebServiceError::Busy | WebServiceError::Unavailable)
                    )
                {
                    false
                } else {
                    group.cleanup()
                }
            };
            if remove {
                remove_group(&Arc::downgrade(&self.material_files), id, &owner);
            }
        }
        Ok(())
    }
    pub(crate) fn material_batch_cancel(&self, lease: SessionLease, id: u64) -> WebResponse {
        let result = (|| {
            let group = self.batch_group(id)?;
            {
                let mut g = group.try_lock().map_err(|_| unavailable())?;
                if g.lease != lease {
                    return Err(failure(403, responses::ErrorCode::Forbidden));
                }
                self.service
                    .check_material_lease(lease, g.scope, &g.classes)
                    .map_err(authentication_error)?;
                if !g.cleanup() {
                    return Err(failure(503, responses::ErrorCode::ResultUnknown));
                }
            }
            remove_group(&Arc::downgrade(&self.material_files), id, &group);
            Ok(response(200, wire::batch_cancelled(id), None))
        })();
        result.unwrap_or_else(|e| e)
    }
    /// Correlation-only negative witness. An unbound ID grants no cleanup right.
    pub(crate) fn material_batch_transfer_binding(
        &self,
        id: u64,
        life: &TransferLifetime,
    ) -> MaterialTransferBinding {
        let owner = {
            let Ok(registry) = self.material_files.try_lock() else {
                return MaterialTransferBinding::Busy;
            };
            registry.as_ref().and_then(|files| {
                files
                    .batches
                    .iter()
                    .find(|(saved, _)| *saved == id)
                    .map(|(_, owner)| owner.clone())
            })
        };
        let Some(owner) = owner else {
            return MaterialTransferBinding::Absent;
        };
        let Ok(g) = owner.try_lock() else {
            return MaterialTransferBinding::Busy;
        };
        match &g.last_life {
            None => MaterialTransferBinding::Unbound,
            Some(saved) if saved.same_owner(life) => MaterialTransferBinding::Exact,
            Some(_) => MaterialTransferBinding::BoundElsewhere,
        }
    }
    pub(crate) fn material_batch_live(&self, id: u64) -> bool {
        let Ok(owner) = self.batch_group(id) else {
            return false;
        };
        let Ok(g) = owner.try_lock() else { return true };
        g.attempted
            || (!g.cancelled
                && Instant::now() < g.expires
                && self
                    .service
                    .check_material_lease(g.lease, g.scope, &g.classes)
                    .is_ok())
    }
    pub(crate) fn discard_material_batch_in_cleanup(
        &self,
        id: u64,
        life: &TransferLifetime,
    ) -> bool {
        let owner = match self.batch_group(id) {
            Ok(g) => g,
            Err(_) => {
                return self.material_batch_transfer_binding(id, life)
                    == MaterialTransferBinding::Absent
            }
        };
        {
            let Ok(mut g) = owner.try_lock() else {
                return false;
            };
            if g.last_life
                .as_ref()
                .is_none_or(|saved| !saved.same_owner(life))
            {
                return false;
            }
            if !g.cleanup() {
                return false;
            }
        }
        remove_group(&Arc::downgrade(&self.material_files), id, &owner);
        true
    }
    pub(crate) fn authorize_material_batch_stream(
        &self,
        head: &RequestHead,
    ) -> Result<(u64, u64), WebResponse> {
        let (lease, route) = self.material_authenticate(head)?;
        let MaterialRoute::BatchUploadBytes {
            batch_id,
            upload_id,
        } = route
        else {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        };
        let owner = self.batch_group(batch_id)?;
        let g = owner
            .try_lock()
            .map_err(|_| failure(429, responses::ErrorCode::Capacity))?;
        g.check(&self.service)?;
        if g.lease != lease || g.attempted || g.active.is_some() {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        let member = g
            .members
            .get(g.next)
            .ok_or_else(|| failure(409, responses::ErrorCode::UploadNotReady))?
            .lock()
            .map_err(|_| unavailable())?;
        if member.version.get() != upload_id || head.content_length != Some(member.bytes) {
            return Err(failure(409, responses::ErrorCode::UploadNotReady));
        }
        Ok((batch_id, upload_id))
    }
    pub(crate) fn begin_material_batch_upload(
        &self,
        head: &RequestHead,
        life: TransferLifetime,
        context: &JobContext,
    ) -> Result<ReceivingMaterialBatchUpload, WebResponse> {
        let (id, upload_id) = self.authorize_material_batch_stream(head)?;
        if head.token != life.token() || !life.is_active() || context.checkpoint().is_err() {
            return Err(unavailable());
        }
        let owner = self.batch_group(id)?;
        let (file, index, serial, expected, expires, lease, scope) = {
            let mut g = owner.try_lock().map_err(|_| unavailable())?;
            g.check(&self.service)?;
            if g.active.is_some() || g.attempted {
                return Err(failure(429, responses::ErrorCode::Capacity));
            }
            let index = g.next;
            let member = g.members.get(index).ok_or_else(unavailable)?.clone();
            let mut member = member.try_lock().map_err(|_| unavailable())?;
            if member.version.get() != upload_id {
                return Err(failure(409, responses::ErrorCode::UploadNotReady));
            }
            let serial = g.serial.checked_add(1).ok_or_else(unavailable)?;
            let Phase::Waiting(file) = std::mem::replace(&mut member.phase, Phase::Receiving)
            else {
                return Err(failure(409, responses::ErrorCode::UploadNotReady));
            };
            g.serial = serial;
            g.active = Some((index, head.token, serial));
            g.last_life = Some(life.clone());
            member.lifetime = Some(life.clone());
            (
                file,
                index,
                serial,
                member.bytes,
                g.expires.min(life.deadline()),
                g.lease,
                g.scope,
            )
        };
        Ok(ReceivingMaterialBatchUpload {
            file: Some(file),
            owner,
            registry: Arc::downgrade(&self.material_files),
            service: self.service.clone(),
            life,
            index,
            serial,
            upload_id,
            expected,
            expires,
            lease,
            scope,
            written: 0,
            next_sequence: 1,
            finished: false,
            failed: false,
        })
    }
    pub(crate) fn material_batch_append(
        &self,
        _backend: &mut Backend,
        _lease: SessionLease,
        _scope: MaterialScope,
        _input: wire::BatchAppendInput,
    ) -> WebResponse {
        // Long validation/owner retirement must use the actual worker interceptor.
        unavailable()
    }
}
fn group_deadline(group: &Arc<Mutex<BatchGroup>>) -> Result<Instant, WebResponse> {
    Ok(group.lock().map_err(|_| unavailable())?.expires)
}

/// Same-worker receiver with one exact group/member/token/serial capability.
/// No positive reply transfers this owner or its original aggregate permit.
pub struct ReceivingMaterialBatchUpload {
    file: Option<NewFile>,
    owner: Arc<Mutex<BatchGroup>>,
    registry: std::sync::Weak<Mutex<Option<MaterialFiles>>>,
    service: WebService,
    life: TransferLifetime,
    index: usize,
    serial: u64,
    upload_id: u64,
    expected: u64,
    expires: Instant,
    lease: SessionLease,
    scope: MaterialScope,
    written: u64,
    next_sequence: u64,
    finished: bool,
    failed: bool,
}
impl ReceivingMaterialBatchUpload {
    pub fn upload_id(&self) -> u64 {
        self.upload_id
    }
    pub fn batch_id(&self) -> u64 {
        self.owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .id
    }
    pub fn deadline(&self) -> Instant {
        self.expires
    }
    fn check(&self, current: &dyn Fn() -> bool) -> Result<(), WebResponse> {
        if self.finished || !current() || !self.life.is_active() || Instant::now() >= self.expires {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        let g = self.owner.try_lock().map_err(|_| unavailable())?;
        g.check(&self.service)?;
        if g.active != Some((self.index, self.life.token(), self.serial)) {
            return Err(unavailable());
        }
        Ok(())
    }
    pub fn write(
        &mut self,
        token: RequestToken,
        sequence: u64,
        offset: u64,
        bytes: &[u8],
        context: &JobContext,
    ) -> Result<UploadAck, WebResponse> {
        let result = (|| {
            let current = context.execution_fence();
            self.check(&current)?;
            let end = offset
                .checked_add(bytes.len() as u64)
                .filter(|n| *n <= self.expected)
                .ok_or_else(unavailable)?;
            if token != self.life.token()
                || sequence != self.next_sequence
                || offset != self.written
                || bytes.is_empty()
                || bytes.len() > CHUNK
            {
                return Err(failure(400, responses::ErrorCode::InvalidRequest));
            }
            let next = sequence.checked_add(1).ok_or_else(unavailable)?;
            self.file
                .as_mut()
                .ok_or_else(unavailable)?
                .file_mut()
                .write_all(bytes)
                .map_err(|_| unavailable())?;
            self.check(&current)?;
            self.written = end;
            self.next_sequence = next;
            Ok(UploadAck::written(
                token,
                UploadKind::Material,
                self.upload_id,
                sequence,
                end,
            ))
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn finish(mut self, token: RequestToken, context: &JobContext) -> WebResponse {
        let result = (|| -> Result<WebResponse, WebResponse> {
            let execution = context.execution_fence();
            let active_owner = self.owner.clone();
            let active = (self.index, self.life.token(), self.serial);
            let current = || {
                execution()
                    && active_owner
                        .try_lock()
                        .is_ok_and(|g| !g.cancelled && g.active == Some(active))
            };
            self.check(&current)?;
            if token != self.life.token() || self.written != self.expected {
                return Err(failure(400, responses::ErrorCode::InvalidResource));
            }
            let file = self
                .file
                .take()
                .ok_or_else(unavailable)?
                .seal()
                .map_err(|_| unavailable())?;
            let (member, files, id, version, name, classes) = {
                let g = self.owner.try_lock().map_err(|_| unavailable())?;
                let member = g.members[self.index].clone();
                let m = member.try_lock().map_err(|_| unavailable())?;
                (
                    member.clone(),
                    m.files.clone(),
                    m.material_id,
                    m.version,
                    m.name.clone(),
                    g.classes.clone(),
                )
            };
            let stage = file.path().to_owned();
            let mut preparation = Preparation {
                cleanup_owner: member.clone(),
                files,
                scope: self.scope,
                id,
                version,
                name,
                expected: self.expected,
                control: Control {
                    service: &self.service,
                    lease: self.lease,
                    scope: self.scope,
                    classes: &classes,
                    start: Instant::now(),
                    expires: self.expires,
                    revision: None,
                    lifetime: Some(&self.life),
                    current: Some(&current),
                },
                pin: None,
                path: stage,
                keep: false,
            };
            let resource = preparation
                .prepare(file)
                .map_err(|e| materials::error(BackendError::Material(e), false))?;
            self.check(&current)?;
            {
                let mut g = self.owner.try_lock().map_err(|_| unavailable())?;
                if g.active != Some((self.index, token, self.serial)) || g.next != self.index {
                    return Err(unavailable());
                }
                let mut member = member.try_lock().map_err(|_| unavailable())?;
                member.path = Some(preparation.path.clone());
                member.phase =
                    Phase::BatchPrepared(resource, preparation.pin.take().ok_or_else(unavailable)?);
                preparation.keep = true;
                g.next += 1;
                g.active = None;
            }
            self.finished = true;
            Ok(response(
                200,
                wire::batch_member_uploaded(self.batch_id(), self.upload_id, self.written),
                None,
            ))
        })();
        result.unwrap_or_else(|e| e)
    }
    pub fn abort(&mut self) -> bool {
        drop(self.file.take());
        let id = {
            let mut g = self
                .owner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if g.active == Some((self.index, self.life.token(), self.serial)) {
                g.active = None;
            }
            if !g.cleanup() {
                return false;
            }
            g.id
        };
        remove_group(&self.registry, id, &self.owner);
        self.finished = true;
        true
    }
}
impl Drop for ReceivingMaterialBatchUpload {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.finished {
            let _ = self.abort();
        }
    }
}

struct ValidatedMember(MaterialResource);
impl MaterialPreparation for ValidatedMember {
    type Input = ();
    fn prepare(&mut self, _: ()) -> Result<MaterialResource, MaterialRepositoryError> {
        Ok(self.0.clone())
    }
}
impl WebApplication {
    pub(crate) fn authorize_material_batch_append(
        &self,
        head: &RequestHead,
        body: &[u8],
        life: &TransferLifetime,
    ) -> Result<u64, WebResponse> {
        if head.token != life.token() || !life.is_active() {
            return Err(unavailable());
        }
        let (lease, route) = self.material_authenticate(head)?;
        let MaterialRoute::BatchAppend { scope } = route else {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        };
        let input = wire::batch_append(body)
            .map_err(|_| failure(400, responses::ErrorCode::InvalidRequest))?;
        let owner = self.batch_group(input.batch_id)?;
        let g = owner.try_lock().map_err(|_| unavailable())?;
        g.check(&self.service)?;
        if g.lease != lease
            || g.scope != scope
            || g.attempted
            || g.active.is_some()
            || g.next != g.members.len()
            || input.allowed_class_ids != g.classes
            || input.upload_ids.len() != g.members.len()
        {
            return Err(failure(409, responses::ErrorCode::UploadNotReady));
        }
        for (id, member) in input.upload_ids.iter().zip(&g.members) {
            let member = member.try_lock().map_err(|_| unavailable())?;
            if *id != member.version.get() || !matches!(member.phase, Phase::BatchPrepared(..)) {
                return Err(failure(409, responses::ErrorCode::UploadNotReady));
            }
        }
        Ok(input.batch_id)
    }
    pub(crate) fn material_batch_receipt_in_job(
        &self,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        body: &[u8],
        life: &TransferLifetime,
        context: &JobContext,
    ) -> Result<Option<WebResponse>, WebResponse> {
        if head.token != life.token()
            || !life.is_active()
            || context.checkpoint().is_err()
            || context.deadline() > life.deadline()
        {
            return Err(unavailable());
        }
        let (lease, route) = self.material_authenticate(head)?;
        let MaterialRoute::BatchAppend { scope } = route else {
            return Err(failure(400, responses::ErrorCode::InvalidRequest));
        };
        let input = wire::batch_append(body)
            .map_err(|_| failure(400, responses::ErrorCode::InvalidRequest))?;
        let reference = CommandReference {
            command_id: input.command_id,
            ticket: input.command_ticket,
        };
        let result = {
            let mut backend = backend.try_lock().map_err(|_| unavailable())?;
            if !self.service.same_instance(&backend.web_service()) {
                return Err(unavailable());
            }
            let scoped = backend.begin_execution_scope(life.job_fence(context));
            scoped
                .web_material_batch_outcome_window(
                    lease,
                    scope,
                    input.command_id,
                    input.command_ticket,
                )
                .map_err(|e| materials::error(e, false))?
        };
        if !life.is_active() || context.checkpoint().is_err() {
            return Err(unavailable());
        }
        let Some(result) = result else {
            return Ok(None);
        };
        if result.scope != scope
            || result.records.len() != input.upload_ids.len()
            || result.records.iter().zip(&input.upload_ids).any(|(r, id)| {
                r.resource.version_id.get() != *id || r.allowed_class_ids != input.allowed_class_ids
            })
        {
            return Err(failure(409, responses::ErrorCode::IdempotencyConflict));
        }
        // A busy physical retirement never erases authoritative saved history.
        let _ = self.retire_material_batch_receipt(reference, scope, &result.records);
        Ok(Some(response(
            200,
            wire::batch_committed(reference, scope, &result.records),
            None,
        )))
    }
    pub(crate) fn material_batch_append_in_job(
        &self,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        body: &[u8],
        life: &TransferLifetime,
        context: &JobContext,
    ) -> Result<WebResponse, WebResponse> {
        let id = self.authorize_material_batch_append(head, body, life)?;
        if context.checkpoint().is_err() || context.deadline() > life.deadline() {
            return Err(unavailable());
        }
        let input = wire::batch_append(body).map_err(|_| unavailable())?;
        let reference = CommandReference {
            command_id: input.command_id,
            ticket: input.command_ticket,
        };
        let owner = self.batch_group(id)?;
        let (lease, scope, expires, serial, prepared) = {
            let mut g = owner.try_lock().map_err(|_| unavailable())?;
            g.check(&self.service)?;
            if g.active.is_some() || g.attempted {
                return Err(failure(429, responses::ErrorCode::Capacity));
            }
            let mut members = Vec::with_capacity(g.members.len());
            for m in &g.members {
                let m = m.try_lock().map_err(|_| unavailable())?;
                let Phase::BatchPrepared(ref resource, ref pin) = m.phase else {
                    return Err(failure(409, responses::ErrorCode::UploadNotReady));
                };
                // Reading this same held object is a metadata sanity check;
                // full hash/format was established by the prior real worker.
                if pin.len().map_err(|_| unavailable())? != m.bytes {
                    return Err(unavailable());
                }
                let mut proof = ValidatedMember(resource.clone());
                members.push(
                    prepare_material_append(
                        &mut proof,
                        (),
                        MaterialAppendMetadata {
                            command_id: input.command_id,
                            scope: g.scope,
                            material_id: m.material_id,
                            display_name: m.name.clone(),
                            allowed_class_ids: g.classes.clone(),
                        },
                    )
                    .map_err(|e| materials::error(BackendError::Material(e), false))?,
                );
            }
            let prepared = prepare_material_batch_append(members)
                .map_err(|e| materials::error(BackendError::Material(e), false))?;
            wire::batch_result_fits(g.scope, &prepared.request().appends)
                .map_err(|_| failure(413, responses::ErrorCode::InvalidRequest))?;
            let serial = g.serial.checked_add(1).ok_or_else(unavailable)?;
            g.serial = serial;
            g.original = Some(reference);
            g.last_life = Some(life.clone());
            g.active = Some((g.members.len(), head.token, serial));
            (g.lease, g.scope, g.expires, serial, prepared)
        };
        let weak = Arc::downgrade(&owner);
        let request = life.clone();
        let job = context.execution_fence();
        // Writer checks this fence while its own authority publication is
        // Updating. Do not request a published snapshot here: the original
        // guarded SessionGuard and domain checks validate the transaction's
        // actual identity/lease before COMMIT. This fence only tightens the
        // original physical request and exact aggregate operation.
        let fence: crate::web_transfer::ExecutionFence = Arc::new(move || {
            job()
                && request.is_active()
                && Instant::now() < expires
                && weak.upgrade().is_some_and(|owner| {
                    owner.try_lock().is_ok_and(|g| {
                        !g.cancelled
                            && g.active == Some((g.members.len(), request.token(), serial))
                            && g.original == Some(reference)
                            && g.lease == lease
                            && g.scope == scope
                    })
                })
        });
        let result = (|| {
            let mut backend = backend
                .try_lock()
                .map_err(|_| BackendError::Material(MaterialRepositoryError::Cancelled))?;
            if !self.service.same_instance(&backend.web_service()) {
                return Err(BackendError::Material(MaterialRepositoryError::Cancelled));
            }
            let mut backend = backend.begin_execution_scope(fence);
            let issued = backend.resolve_web_command(
                lease,
                fc_ports::command_window::CommandFamily::Materials,
                input.command_ticket,
                input.command_id,
            )?;
            let mut backend = backend.begin_command_scope(issued)?;
            owner
                .lock()
                .map_err(|_| BackendError::Material(MaterialRepositoryError::Faulted))?
                .attempted = true;
            backend.web_material_append_batch(lease, prepared)
        })();
        let definite = result.as_ref().is_err_and(|e| {
            e.command_window_error().is_some()
                || matches!(
                    e,
                    BackendError::Material(
                        MaterialRepositoryError::Domain(_)
                            | MaterialRepositoryError::IdempotencyConflict
                            | MaterialRepositoryError::QuotaExceeded
                            | MaterialRepositoryError::DocumentTooLarge
                            | MaterialRepositoryError::Cancelled
                    )
                )
        });
        let remove = {
            let mut g = owner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            g.active = None;
            if result.is_ok() {
                g.keep_published();
                true
            } else if definite || !g.attempted {
                g.attempted = false;
                g.cleanup()
            } else {
                false
            }
        };
        if remove {
            remove_group(&Arc::downgrade(&self.material_files), id, &owner);
        }
        match result {
            Ok(records) if life.is_active() && context.checkpoint().is_ok() => Ok(response(
                200,
                wire::batch_committed(reference, scope, &records),
                None,
            )),
            Ok(_) => Ok(failure(503, responses::ErrorCode::ResultUnknown)),
            Err(e) => Ok(materials::error(e, true)),
        }
    }
}
impl WebApplication {
    pub(crate) fn material_batch_owner_witness(&self) -> Arc<dyn Fn() -> usize + Send + Sync> {
        let registry = Arc::downgrade(&self.material_files);
        Arc::new(move || match registry.upgrade() {
            None => 0,
            Some(registry) => match registry.try_lock() {
                Ok(files) => files.as_ref().map_or(0, |f| f.batches.len()),
                Err(_) => 2,
            },
        })
    }
    pub(crate) fn cancel_all_material_batches_in_cleanup(&self) {
        let groups = {
            let Ok(registry) = self.material_files.try_lock() else {
                return;
            };
            registry
                .as_ref()
                .map(|f| f.batches.clone())
                .unwrap_or_default()
        };
        for (id, owner) in groups {
            let clean = {
                let Ok(mut g) = owner.try_lock() else {
                    continue;
                };
                g.cleanup()
            };
            if clean {
                remove_group(&Arc::downgrade(&self.material_files), id, &owner);
            }
        }
    }
}
impl WebApplication {
    /// Called only after Backend returned the original authenticated durable
    /// receipt. Public IDs or NOT_FOUND never enter this negative retirement.
    pub(crate) fn retire_material_batch_receipt(
        &self,
        command: CommandReference,
        scope: MaterialScope,
        records: &[MaterialRecord],
    ) -> Result<(), WebResponse> {
        let groups = {
            let registry = self.material_files.try_lock().map_err(|_| unavailable())?;
            registry
                .as_ref()
                .map(|f| f.batches.clone())
                .unwrap_or_default()
        };
        for (id, owner) in groups {
            let retire = {
                let mut g = owner.try_lock().map_err(|_| unavailable())?;
                if g.original != Some(command) || g.scope != scope {
                    false
                } else {
                    if g.active.is_some() || g.members.len() != records.len() {
                        return Err(unavailable());
                    }
                    for (member, record) in g.members.iter().zip(records) {
                        let member = member.try_lock().map_err(|_| unavailable())?;
                        let Phase::BatchPrepared(ref resource, _) = member.phase else {
                            return Err(unavailable());
                        };
                        if member.material_id != record.id
                            || member.version != record.resource.version_id
                            || member.name != record.display_name
                            || *resource != record.resource
                            || g.classes != record.allowed_class_ids
                        {
                            return Err(unavailable());
                        }
                    }
                    g.keep_published();
                    true
                }
            };
            if retire {
                remove_group(&Arc::downgrade(&self.material_files), id, &owner);
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod namespace_tests {
    use super::*;
    #[test]
    fn startup_accepts_only_two_owned_groups_and_forty_canonical_members() {
        let names = (1..=2)
            .flat_map(|group| {
                (1..=20).map(move |member| format!("web-material-batch-{group}-{member}.part"))
            })
            .collect::<Vec<_>>();
        assert!(valid_stage_set(names.iter().map(String::as_str)));
        for bad in [
            "web-material-batch-01-1.part",
            "web-material-batch-1-0.part",
            "web-material-batch-1-1.part.exe",
            "web-material-batch-1-1-2.part",
            "foreign.txt",
        ] {
            assert!(!valid_stage_set([bad]));
        }
        assert!(!valid_stage_set([
            "web-material-batch-1-1.part",
            "web-material-batch-2-1.part",
            "web-material-batch-3-1.part"
        ]));
        let excess = (1..=21)
            .map(|member| format!("web-material-batch-1-{member}.part"))
            .collect::<Vec<_>>();
        assert!(!valid_stage_set(excess.iter().map(String::as_str)));
        assert!(valid_stage_set([
            "web-material-1.part",
            "web-material-batch-2-1.part"
        ]));
        assert!(!valid_stage_set([
            "web-material-1.part",
            "web-material-2.part",
            "web-material-batch-3-1.part"
        ]));
    }
}
