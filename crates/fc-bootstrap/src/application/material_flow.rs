//! Owned material preparation on the existing shared worker pool. No file bytes
//! or native handles enter SQLite transactions; publication follows full validation.
use super::{command_id, fatal, invalid, Failure, Outcome, Result};
use fc_application::identity::SessionLease;
use fc_bootstrap::backend::{
    Backend, MaterialOpenAuthorization, MaterialOpenGuard, ResourceState, SessionControl,
};
use fc_domain::{ids::*, materials::*};
use fc_materials::office::{ContainerError, ValidationControl};
use fc_ports::{
    prepare_material_append, MaterialAppendMetadata, MaterialDeleteRequest, MaterialListRequest,
    MaterialPreparation, MaterialRepositoryError,
};
use fc_runtime::worker::JobContext;
use fc_windows::materials::{
    chooser::SelectedFile,
    files::{ControlledFiles, LaunchPath, LockedFile},
    helper, image,
};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

static LAUNCH_PREPARING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct LaunchPreparation;
impl LaunchPreparation {
    fn take() -> Result<Self> {
        LAUNCH_PREPARING
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .map_err(|_| invalid("另一个打开副本正在准备，请稍后重试。"))?;
        Ok(Self)
    }
}
impl Drop for LaunchPreparation {
    fn drop(&mut self) {
        LAUNCH_PREPARING.store(false, std::sync::atomic::Ordering::Release);
    }
}
pub(super) enum Job {
    Load {
        lease: SessionLease,
        request: MaterialListRequest,
    },
    Import {
        lease: SessionLease,
        scope: MaterialScope,
        classes: Vec<ClassId>,
        selected: SelectedFile,
    },
    Delete {
        lease: SessionLease,
        request: MaterialDeleteRequest,
    },
    Open(OpenRequest),
    Copies {
        lease: SessionLease,
        clean: bool,
    },
}
#[derive(Clone)]
pub(super) enum OpenRequest {
    Owner {
        lease: SessionLease,
        scope: MaterialScope,
        id: MaterialId,
        version: ResourceVersionId,
    },
    Scene {
        source: MaterialSource,
        generation: SceneGeneration,
        id: MaterialId,
        version: ResourceVersionId,
    },
}
impl OpenRequest {
    fn authorize(&self, backend: &Backend) -> Result<MaterialOpenAuthorization> {
        Ok(match self {
            Self::Owner {
                lease,
                scope,
                id,
                version,
            } => backend.material_for_owner(*lease, *scope, *id, *version)?,
            Self::Scene {
                source,
                generation,
                id,
                version,
            } => backend.material_for_scene(source, *generation, *id, *version)?,
        })
    }
}
pub(super) struct PreparedLaunch {
    pub path: LaunchPath,
    pub guard: MaterialOpenGuard,
    pub request_id: u64,
    pub resource_version: ResourceVersionId,
    pub from_scene: bool,
}
pub(super) struct MaterialJob {
    pub root: PathBuf,
    pub backend: Arc<Mutex<Backend>>,
    pub control: SessionControl,
    pub job: Job,
}
struct Control<'a> {
    job: &'a JobContext,
    check: &'a dyn Fn() -> bool,
    started: Instant,
}
impl ValidationControl for Control<'_> {
    fn canceled(&self) -> bool {
        self.started.elapsed() >= Duration::from_secs(30)
            || self.job.checkpoint().is_err()
            || !(self.check)()
    }
    fn thread_cpu_time(&self) -> std::result::Result<Duration, ContainerError> {
        fc_windows::system::current_thread_cpu_time().map_err(|_| ContainerError::ClockUnavailable)
    }
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
fn resource_path(
    files: &ControlledFiles,
    scope: MaterialScope,
    id: MaterialId,
    resource: &MaterialResource,
) -> Result<PathBuf> {
    let dir = files
        .directory(&[
            "teachers".into(),
            scope.teacher_id.get().to_string(),
            "subjects".into(),
            scope.subject_id.get().to_string(),
            id.get().to_string(),
        ])
        .map_err(|_| invalid("资料目录不可用。"))?;
    Ok(dir.join(format!(
        "{}.{}",
        resource.version_id.get(),
        ext(resource.kind)
    )))
}
fn io_error() -> Failure {
    invalid("文件不可读、被占用或不在允许的本机目录中。")
}
fn verify_pdf(file: &mut LockedFile, control: &Control<'_>) -> Result<(u64, [u8; 32])> {
    let (bytes, sha) = fc_materials::validation::hash_file(file.file_mut(), control)
        .map_err(|_| invalid("PDF读取或哈希校验未完成。"))?;
    let report = helper::validate_pdf(file.path(), || control.canceled())
        .map_err(|_| invalid("PDF隔离校验取消、超时或超出资源上限。"))?;
    let line = std::str::from_utf8(&report.stdout).map_err(|_| invalid("PDF校验结果不完整。"))?;
    let parts: Vec<_> = line.split_whitespace().collect();
    let expected: String = sha.iter().map(|v| format!("{v:02x}")).collect();
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
        return Err(invalid("PDF格式、加密或主动内容校验未通过。"));
    }
    Ok((bytes, sha))
}
struct PendingBlob<'a> {
    files: &'a ControlledFiles,
    path: PathBuf,
    retain: bool,
    pinned: Option<LockedFile>,
}
impl Drop for PendingBlob<'_> {
    fn drop(&mut self) {
        self.pinned.take();
        if !self.retain && self.files.remove(&self.path).is_ok() {
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }
}
struct Preparation<'a> {
    files: &'a ControlledFiles,
    scope: MaterialScope,
    id: MaterialId,
    version: ResourceVersionId,
    name: String,
    control: Control<'a>,
    pending: Option<PendingBlob<'a>>,
    failure: Option<Failure>,
}
impl MaterialPreparation for Preparation<'_> {
    type Input = SelectedFile;
    fn prepare(
        &mut self,
        selected: SelectedFile,
    ) -> std::result::Result<MaterialResource, MaterialRepositoryError> {
        match self.prepare_file(selected) {
            Ok(resource) => Ok(resource),
            Err(error) => {
                self.failure = Some(error);
                Err(MaterialRepositoryError::Cancelled)
            }
        }
    }
}
impl Preparation<'_> {
    fn prepare_file(&mut self, selected: SelectedFile) -> Result<MaterialResource> {
        if self.control.canceled() {
            return Err(Failure::Stopped);
        }
        let mut source = selected.open(MAX_MATERIAL_BYTES).map_err(|_| io_error())?;
        let kind =
            MaterialKind::from_name(&self.name).ok_or_else(|| invalid("不支持此资料类型。"))?;
        let stage = self
            .files
            .directory(&["staging".into()])
            .map_err(|_| io_error())?
            .join(format!("{}.part", self.version.get()));
        let mut target = self.files.create_new(&stage).map_err(|_| io_error())?;
        let bytes = fc_materials::copy_bounded(
            source.file_mut(),
            target.file_mut(),
            MAX_MATERIAL_BYTES,
            Duration::from_secs(30),
            || self.control.canceled(),
        )
        .map_err(|_| invalid("资料复制取消、超时或超过50MiB。"))?;
        drop(source);
        let stage = target.finish().map_err(|_| io_error())?;
        self.pending = Some(PendingBlob {
            files: self.files,
            path: stage.clone(),
            retain: false,
            pinned: None,
        });
        let mut copied =
            LockedFile::open_prepared(&stage, MAX_MATERIAL_BYTES).map_err(|_| io_error())?;
        let (validated, sha) = if kind == MaterialKind::Pdf {
            verify_pdf(&mut copied, &self.control)?
        } else {
            let report = fc_materials::validation::validate_file(
                copied.file_mut(),
                &self.name,
                &self.control,
            )
            .map_err(|_| invalid("文件实际格式或内容校验未通过。"))?;
            if let Some(expected) = report.image {
                let decoded = image::validate_image(&copied, kind, || self.control.canceled())
                    .map_err(|_| invalid("图片无法完整解码或超过尺寸限制。"))?;
                if (decoded.width, decoded.height) != (expected.width, expected.height) {
                    return Err(invalid("图片尺寸信息不一致。"));
                }
            }
            (report.bytes, report.sha256)
        };
        if validated != bytes || self.control.canceled() {
            return Err(Failure::Stopped);
        }
        let resource = MaterialResource {
            version_id: self.version,
            sha256: sha,
            kind,
            bytes,
        };
        resource
            .validate()
            .map_err(|_| invalid("资料超过类型或大小限制。"))?;
        let final_path = resource_path(self.files, self.scope, self.id, &resource)?;
        let copied = copied
            .publish(self.files, &final_path)
            .map_err(|_| invalid("已验证资料无法原子发布。"))?;
        let pending = self.pending.as_mut().expect("stage tracked");
        pending.path = final_path;
        pending.pinned = Some(copied);
        Ok(resource)
    }
}
/// Mutex occupancy is a retryable scheduling condition, not a revoked lease.
/// Poisoning still faults the product rather than masquerading as contention.
fn backend_lock_error<T>(error: std::sync::TryLockError<T>) -> Failure {
    match error {
        std::sync::TryLockError::WouldBlock => invalid("后台繁忙，请稍后重试。"),
        std::sync::TryLockError::Poisoned(_) => Failure::Backend(
            fc_bootstrap::backend::BackendError::Storage(fc_storage::StorageError::WorkerPanicked),
        ),
    }
}
fn after_committed_material(error: Failure) -> Failure {
    match error {
        Failure::Input("后台繁忙，请稍后重试。") => {
            invalid("资料操作已保存。后续更新暂时繁忙，请刷新目录核对，勿重复导入。")
        }
        other => other,
    }
}
impl MaterialJob {
    pub fn allocation_bytes(&self) -> Option<usize> {
        Some(
            self.root.as_os_str().len().saturating_mul(2)
                + match &self.job {
                    Job::Import {
                        classes, selected, ..
                    } => {
                        classes.capacity() * std::mem::size_of::<ClassId>()
                            + selected.allocation_bytes()
                    }
                    Job::Open(OpenRequest::Scene { .. }) => 1024,
                    _ => 512,
                },
        )
    }
    pub fn run(self, context: &JobContext) -> Result<Outcome> {
        let files = ControlledFiles::new(&self.root).map_err(|_| io_error())?;
        match self.job {
            Job::Copies { lease, clean } => {
                let guard = self
                    .backend
                    .try_lock()
                    .map_err(backend_lock_error)?
                    .launch_copy_guard(lease)?;
                let check = || context.checkpoint().is_ok() && guard.is_context_valid();
                let report = launch_inventory(&files, clean, &check)?;
                if !check() {
                    return Err(Failure::Stopped);
                }
                Ok(Outcome::LaunchCopies(report))
            }
            Job::Load { lease, request } => Ok(Outcome::Materials(Box::new(
                self.backend
                    .try_lock()
                    .map_err(backend_lock_error)?
                    .materials(lease, request)?,
            ))),
            Job::Delete { lease, request } => {
                let scope = request.scope;
                self.backend
                    .try_lock()
                    .map_err(backend_lock_error)?
                    .delete_material(lease, request)?;
                reconcile(&files, &self.backend, context).map_err(after_committed_material)?;
                Ok(Outcome::Materials(Box::new(
                    self.backend
                        .try_lock()
                        .map_err(backend_lock_error)
                        .map_err(after_committed_material)?
                        .materials(
                            lease,
                            MaterialListRequest {
                                scope,
                                cursor: None,
                            },
                        )?,
                )))
            }
            Job::Import {
                lease,
                scope,
                classes,
                selected,
            } => {
                // Preflight before any filesystem copy; the writer checks this
                // same exact lease/scope again at the actual commit boundary.
                self.backend
                    .try_lock()
                    .map_err(backend_lock_error)?
                    .materials(
                        lease,
                        MaterialListRequest {
                            scope,
                            cursor: None,
                        },
                    )?;
                let name = selected.display_name().map_err(|_| io_error())?;
                let id = MaterialId::new(command_id()?.get()).map_err(|_| Failure::Stopped)?;
                let version =
                    ResourceVersionId::new(command_id()?.get()).map_err(|_| Failure::Stopped)?;
                let alive = || {
                    !self.control.is_local_locked()
                        && self.control.health_gate().check_sensitive().is_ok()
                };
                let mut preparation = Preparation {
                    files: &files,
                    scope,
                    id,
                    version,
                    name: name.clone(),
                    control: Control {
                        job: context,
                        check: &alive,
                        started: Instant::now(),
                    },
                    pending: None,
                    failure: None,
                };
                let prepared = prepare_material_append(
                    &mut preparation,
                    selected,
                    MaterialAppendMetadata {
                        command_id: command_id()?,
                        scope,
                        material_id: id,
                        display_name: name,
                        allowed_class_ids: classes,
                    },
                )
                .map_err(|_| {
                    preparation
                        .failure
                        .take()
                        .unwrap_or_else(|| invalid("资料校验未通过或已取消；原文件未改变。"))
                })?;
                if preparation.control.canceled() {
                    return Err(Failure::Stopped);
                }
                let mut result = self
                    .backend
                    .try_lock()
                    .map_err(backend_lock_error)?
                    .append_material(lease, prepared)
                    .map_err(Failure::from);
                // Unavailable also covers decoding a completion after COMMIT.
                // Without a definite rejection, retain the validated inode and
                // fail closed; a fresh startup inventory resolves DB truth.
                if matches!(
                    result,
                    Err(Failure::Backend(
                        fc_bootstrap::backend::BackendError::Material(
                            MaterialRepositoryError::Unavailable
                        )
                    ))
                ) {
                    self.control.fault();
                    result = Err(Failure::Backend(
                        fc_bootstrap::backend::BackendError::Material(
                            MaterialRepositoryError::Faulted,
                        ),
                    ));
                }
                // A failed post-COMMIT publication must retain the blob until the
                // next successful startup inventory resolves commit truth.
                if result.is_ok() || result.as_ref().is_err_and(fatal) {
                    if let Some(p) = &mut preparation.pending {
                        p.retain = true;
                    }
                }
                result?;
                drop(preparation);
                Ok(Outcome::Materials(Box::new(
                    self.backend
                        .try_lock()
                        .map_err(backend_lock_error)
                        .map_err(after_committed_material)?
                        .materials(
                            lease,
                            MaterialListRequest {
                                scope,
                                cursor: None,
                            },
                        )?,
                )))
            }
            Job::Open(request) => {
                // A physical reservation spans inventory/admission through the
                // complete copy. A second worker cannot admit against stale quota.
                let _preparation = LaunchPreparation::take()?;
                let initial =
                    request.authorize(&*self.backend.try_lock().map_err(backend_lock_error)?)?;
                let record = initial.record;
                let alive = || initial.guard.is_context_valid();
                let control = Control {
                    job: context,
                    check: &alive,
                    started: Instant::now(),
                };
                let scope = match &request {
                    OpenRequest::Owner { scope, .. } => *scope,
                    OpenRequest::Scene { source, .. } => MaterialScope {
                        teacher_id: source.teacher_id,
                        subject_id: source.subject_id,
                    },
                };
                let mut original = LockedFile::open(
                    &resource_path(&files, scope, record.id, &record.resource)?,
                    MAX_MATERIAL_BYTES,
                )
                .map_err(|_| io_error())?;
                let dir = launch_admission(&files, record.resource.bytes, &|| !control.canceled())?;
                let copy = dir.join(&record.display_name);
                let mut out = files.create_new(&copy).map_err(|_| io_error())?;
                let bytes = fc_materials::copy_bounded(
                    original.file_mut(),
                    out.file_mut(),
                    MAX_MATERIAL_BYTES,
                    Duration::from_secs(30),
                    || control.canceled(),
                )
                .map_err(|_| invalid("打开副本准备未完成。"))?;
                let copy = out.finish().map_err(|_| io_error())?;
                let mut pending = PendingBlob {
                    files: &files,
                    path: copy.clone(),
                    retain: false,
                    pinned: None,
                };
                let mut locked =
                    LockedFile::open(&copy, MAX_MATERIAL_BYTES).map_err(|_| io_error())?;
                let actual = fc_materials::validation::hash_file(locked.file_mut(), &control)
                    .map_err(|_| invalid("打开副本哈希校验未完成。"))?;
                if bytes != record.resource.bytes
                    || actual != (record.resource.bytes, record.resource.sha256)
                    || control.canceled()
                {
                    return Err(invalid("资料内容已改变或打开权限已撤销。"));
                }
                drop(original);
                let fresh =
                    request.authorize(&*self.backend.try_lock().map_err(backend_lock_error)?)?;
                if fresh.record != record || !fresh.guard.is_valid() {
                    return Err(invalid("资料版本或权限已改变，请重新选择。"));
                }
                let path = LaunchPath::from_verified(locked, MAX_MATERIAL_BYTES)
                    .map_err(|_| io_error())?;
                pending.retain = true;
                Ok(Outcome::MaterialLaunch(Box::new(PreparedLaunch {
                    path,
                    guard: fresh.guard,
                    request_id: command_id()?.get(),
                    resource_version: record.resource.version_id,
                    from_scene: matches!(request, OpenRequest::Scene { .. }),
                })))
            }
        }
    }
}
const LAUNCH_BYTES: u64 = 200 * 1024 * 1024;
const LAUNCH_FILES: usize = 128;
const LAUNCH_GRACE: Duration = Duration::from_secs(24 * 60 * 60);
#[derive(Clone, Copy, Default)]
pub(super) struct LaunchCopyReport {
    pub bytes: u64,
    pub count: usize,
    pub expired: usize,
    pub removed: usize,
    pub retained: usize,
}
fn expired_copy(metadata: &std::fs::Metadata) -> bool {
    metadata
        .modified()
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age >= LAUNCH_GRACE)
}
fn launch_inventory(
    files: &ControlledFiles,
    clean: bool,
    authorized: &dyn Fn() -> bool,
) -> Result<LaunchCopyReport> {
    let root = files
        .directory(&["launch-copies".into()])
        .map_err(|_| io_error())?;
    let mut report = LaunchCopyReport::default();
    for dir in files.entries(&root, LAUNCH_FILES).map_err(|_| io_error())? {
        if !authorized() {
            return Err(Failure::Stopped);
        }
        if !dir.is_dir() {
            return Err(io_error());
        }
        for path in files.entries(&dir, 1).map_err(|_| io_error())? {
            if !authorized() {
                return Err(Failure::Stopped);
            }
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| io_error())?;
            if !metadata.is_file() {
                return Err(io_error());
            }
            if expired_copy(&metadata) {
                report.expired += 1;
                // Age and live permission are checked again while the exact file
                // is held exclusively. No external writes can race disposition.
                if clean {
                    match files.remove_when(&path, |pinned| authorized() && expired_copy(pinned)) {
                        Ok(true) => {
                            report.removed += 1;
                            continue;
                        }
                        Ok(false) => {}
                        Err(error) if matches!(error.code().0 as u32, 0x80070020 | 0x80070021) => {
                            report.retained += 1;
                        }
                        Err(_) => return Err(io_error()),
                    }
                }
            }
            report.bytes = report
                .bytes
                .checked_add(metadata.len())
                .ok_or_else(io_error)?;
            report.count += 1;
        }
        if clean {
            let _ = std::fs::remove_dir(&dir);
        }
    }
    Ok(report)
}
fn launch_admission(
    files: &ControlledFiles,
    required: u64,
    authorized: &dyn Fn() -> bool,
) -> Result<PathBuf> {
    let report = launch_inventory(files, true, authorized)?;
    if report.count >= LAUNCH_FILES
        || report
            .bytes
            .checked_add(required)
            .is_none_or(|v| v > LAUNCH_BYTES)
    {
        return Err(invalid(
            "打开副本空间已满（200MiB/128份）。请关闭不用的文件，由全局管理员在资料管理中清理已到期副本。",
        ));
    }
    files
        .directory(&["launch-copies".into(), command_id()?.get().to_string()])
        .map_err(|_| io_error())
}
/// Resolve durable DB truth before deleting any original. Inventory is paged;
/// retirement is committed first, filesystem deletion second, quota release last.
pub(super) fn reconcile(
    files: &ControlledFiles,
    backend: &Arc<Mutex<Backend>>,
    context: &JobContext,
) -> Result<()> {
    let mut after = None;
    let mut known = HashSet::new();
    loop {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        let page = backend
            .try_lock()
            .map_err(backend_lock_error)?
            .resource_inventory(after)?;
        for item in page.entries {
            context.checkpoint().map_err(|_| Failure::Stopped)?;
            if item.state == ResourceState::Removed {
                continue;
            }
            let path = resource_path(files, item.scope, item.material_id, &item.resource)?;
            match item.state {
                ResourceState::Referenced => {
                    let original = LockedFile::open(&path, MAX_MATERIAL_BYTES)
                        .map_err(|_| invalid("已发布资料文件缺失或状态异常，需要核对数据。"))?;
                    if original.len().map_err(|_| io_error())? != item.resource.bytes {
                        return Err(invalid("已发布资料大小异常，需要核对数据。"));
                    }
                    drop(original);
                    known.insert(path);
                }
                ResourceState::Unreferenced | ResourceState::Retiring => {
                    let ticket = backend
                        .try_lock()
                        .map_err(backend_lock_error)?
                        .retire_resource(item.resource.version_id, item.revision)?;
                    if files.remove(&path).is_ok() {
                        if let Some(parent) = path.parent() {
                            let _ = std::fs::remove_dir(parent);
                        }
                        backend
                            .try_lock()
                            .map_err(backend_lock_error)?
                            .finalize_resource_removal(ticket)?;
                    } else {
                        known.insert(path);
                    }
                }
                ResourceState::Removed => {}
            }
        }
        after = page.next_cursor;
        if after.is_none() {
            break;
        }
    }
    let staging = files
        .directory(&["staging".into()])
        .map_err(|_| io_error())?;
    for p in files.entries(&staging, 128).map_err(|_| io_error())? {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        files.remove(&p).map_err(|_| io_error())?;
    }
    // Traverse only our fixed ID hierarchy, with global and per-directory bounds;
    // no school files or arbitrary filesystem descendants are enumerated.
    let teachers = files
        .directory(&["teachers".into()])
        .map_err(|_| io_error())?;
    let mut directories = vec![(teachers, 0usize)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = directories.pop() {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        for path in files.entries(&dir, 16384).map_err(|_| io_error())? {
            visited += 1;
            if visited > 32768 {
                return Err(invalid("资料目录超过本阶段核对上限。"));
            }
            if path.is_dir() {
                if depth >= 4 {
                    return Err(io_error());
                }
                directories.push((path, depth + 1));
            } else if depth == 4 {
                if !known.contains(&path) {
                    files.remove(&path).map_err(|_| io_error())?;
                }
            } else {
                return Err(io_error());
            }
        }
        if depth == 4 {
            let _ = std::fs::remove_dir(&dir);
        }
    }
    Ok(())
}
