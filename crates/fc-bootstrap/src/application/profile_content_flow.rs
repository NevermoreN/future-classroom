//! Existing shared-worker composition for local independent content. Job inputs
//! are bounded text or selection/scene descriptors; pixels stay in one shared
//! physical 32 MiB cache and never become ordinary queue payloads.
use super::{invalid, Failure, Outcome, Result};
use fc_application::identity::SessionLease;
use fc_bootstrap::{
    backend::{Backend, MediaState},
    profile_content_files::{
        ContentSources, ControlledContentFiles, LocalContentInput, PreparationState,
    },
};
use fc_domain::{ids::*, profile_content::*, time::LocalDateTime};
use fc_ports::{prepare_profile_content, ProfileContentOutcome, ProfileContentStatus};
use fc_runtime::{health::Health, worker::JobContext};
use fc_windows::{
    desktop::ImageRenderRequest,
    materials::raster::{RasterCache, RasterFrame},
    system::CurrentClock,
    SystemRandom,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

pub(super) fn raster_cache() -> Arc<RasterCache> {
    static CACHE: OnceLock<Arc<RasterCache>> = OnceLock::new();
    CACHE.get_or_init(|| Arc::new(RasterCache::new())).clone()
}

pub(super) fn problem(error: &fc_ports::ProfileContentRepositoryError) -> &'static str {
    use fc_ports::ProfileContentRepositoryError as R;
    match error {
        R::Domain(
            ProfileContentError::RevisionConflict { .. }
            | ProfileContentError::ViewRevisionConflict { .. }
            | ProfileContentError::Superseded,
        ) => "节点内容已有更新，请重新加载并核对草稿后再发布。",
        R::Domain(ProfileContentError::InvalidSlot | ProfileContentError::KindMismatch) => {
            "节点已删除、类型或任教范围已变化，请返回布局重新选择。"
        }
        R::Domain(ProfileContentError::TextTooLarge | ProfileContentError::InvalidText) => {
            "文字须为有效 UTF-8 或带 BOM 的 UTF-16，最多 10,000 字；请核对文件编码和正文。"
        }
        R::Domain(ProfileContentError::InvalidMedia) => {
            "图片校验未通过；请使用 PNG、JPEG 或 BMP，每张最多 10MiB、800 万像素。"
        }
        R::Domain(ProfileContentError::UndoUnavailable) => "可撤销版本已有变化，请重新加载后核对。",
        R::Domain(ProfileContentError::Capacity | ProfileContentError::VersionOverflow)
        | R::DocumentTooLarge => "内容容量或版本数量已达本阶段上限，请保留数据并检查。",
        R::QuotaExceeded => "内容原件总容量已达上限，请释放不需要的内容并稍后重新加载。",
        R::Cancelled | R::Domain(ProfileContentError::Expired) => {
            "任务已取消或超过处理期限，请重新加载并核对保存结果。"
        }
        R::Faulted => "内容数据状态需要修复，已停止本次运行。",
        _ => "尚未确认节点内容的保存结果，请重新加载核对后再操作。",
    }
}

pub(super) struct ImageJob {
    pub root: PathBuf,
    pub backend: Arc<Mutex<Backend>>,
    pub generation: SceneGeneration,
    pub source_revision: Revision,
    pub captured_at: LocalDateTime,
    pub valid_until: Option<LocalDateTime>,
    pub requests: Vec<ImageRenderRequest>,
    pub cache: Arc<RasterCache>,
}
pub(super) struct ImageCompletion {
    pub generation: SceneGeneration,
    pub source_revision: Revision,
    pub frames: Vec<Arc<RasterFrame>>,
}
impl ImageJob {
    pub fn allocation_bytes(&self) -> Option<usize> {
        if self.requests.len() > fc_domain::layout::MAX_VISIBLE_LEAVES {
            return None;
        }
        self.root
            .as_os_str()
            .len()
            .checked_mul(2)?
            .checked_add(
                self.requests
                    .capacity()
                    .checked_mul(std::mem::size_of::<ImageRenderRequest>())?,
            )?
            .checked_add(std::mem::size_of::<Self>())
    }
    pub fn run(self, context: &JobContext) -> Result<Outcome> {
        if self.requests.len() > fc_domain::layout::MAX_VISIBLE_LEAVES {
            return Err(invalid("图片准备数量超过当前场景上限。"));
        }
        let descriptors: Vec<_> = self
            .requests
            .iter()
            .map(|request| {
                (
                    request.identity.slot,
                    request.identity.revision,
                    request.identity.view_revision,
                    request.reference.clone(),
                )
            })
            .collect();
        let gate = {
            let backend = self
                .backend
                .try_lock()
                .map_err(|_| invalid("内容状态正在更新，请稍后刷新。"))?;
            backend.verify_profile_content_images(
                self.source_revision,
                self.captured_at,
                self.valid_until,
                &descriptors,
            )?;
            backend.session_control().health_gate()
        };
        let files = ControlledContentFiles::new(&self.root)
            .map_err(fc_bootstrap::backend::BackendError::ProfileContent)?;
        let current = || {
            let status = gate.status();
            context.checkpoint().is_ok()
                && status.health == Health::Healthy
                && status.revision == self.source_revision
        };
        let mut frames = Vec::with_capacity(self.requests.len());
        for request in &self.requests {
            if !current() {
                return Err(Failure::Stopped);
            }
            // Keep every frame in this batch alive until the receiving native
            // cards own them. Cache eviction cannot erase an earlier ready node.
            frames.push(
                files
                    .decode_current(
                        &self.cache,
                        &request.reference,
                        request.max_pixel_width,
                        request.max_pixel_height,
                        &current,
                    )
                    .map_err(fc_bootstrap::backend::BackendError::ProfileContent)?,
            );
        }
        if !current() {
            return Err(Failure::Stopped);
        }
        self.backend
            .try_lock()
            .map_err(|_| invalid("内容状态正在更新，请稍后刷新。"))?
            .verify_profile_content_images(
                self.source_revision,
                self.captured_at,
                self.valid_until,
                &descriptors,
            )?;
        Ok(Outcome::ContentImages(ImageCompletion {
            generation: self.generation,
            source_revision: self.source_revision,
            frames,
        }))
    }
}

pub(super) enum Operation {
    Load {
        slot: ContentSlotKey,
    },
    Publish {
        slot: ContentSlotKey,
        expected_revision: Revision,
        command_id: CommandId,
        operation: ContentOperation,
        input: LocalContentInput,
    },
}
pub(super) struct ContentJob {
    pub root: PathBuf,
    pub backend: Arc<Mutex<Backend>>,
    pub lease: SessionLease,
    pub operation: Operation,
}
pub(super) struct Loaded {
    pub document: ProfileContentDocument,
    pub outcome: Option<ProfileContentOutcome>,
    pub cleanup_pending: bool,
}

/// One deployment's bounded scan position, independent of a completion that
/// might be discarded after cancellation or a deadline.
#[derive(Default)]
pub(super) struct GcCursor {
    after: Option<MediaVersionId>,
}
impl ContentJob {
    pub fn allocation_bytes(&self) -> Option<usize> {
        let payload = match &self.operation {
            Operation::Load { .. } => 1024,
            Operation::Publish { input, .. } => input.allocation_bytes()?.checked_add(2048)?,
        };
        self.root
            .as_os_str()
            .len()
            .checked_mul(2)?
            .checked_add(payload)
    }
    pub fn run(self, context: &JobContext) -> Result<Outcome> {
        match self.operation {
            Operation::Load { slot } => {
                let document = self
                    .backend
                    .lock()
                    .map_err(|_| Failure::Stopped)?
                    .read_profile_content(self.lease, slot)?;
                Ok(Outcome::ProfileContent(Box::new(Loaded {
                    document,
                    outcome: None,
                    cleanup_pending: false,
                })))
            }
            Operation::Publish {
                slot,
                expected_revision,
                command_id,
                operation,
                input,
            } => {
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                let sources = ContentSources::open(input, &mut SystemRandom)
                    .map_err(fc_bootstrap::backend::BackendError::ProfileContent)?;
                let sizes = sources.image_sizes().filter(|sizes| !sizes.is_empty());
                let now = CurrentClock::new()
                    .try_now()
                    .map_err(|_| invalid("本机日期不可用，请重新加载。"))?;
                let expires_at =
                    LocalDateTime::new(now.date, (now.second_of_day() + 300).min(86_399))
                        .map_err(|_| invalid("本机日期不可用，请重新加载。"))?;
                if expires_at <= now {
                    return Err(invalid("日期即将变化，请稍后重新加载内容。"));
                }
                let request = BeginContentRequest {
                    command_id,
                    slot,
                    expected_revision,
                    operation,
                    payload_fingerprint: sources
                        .intent_fingerprint(slot, operation)
                        .map_err(fc_bootstrap::backend::BackendError::ProfileContent)?,
                    expires_at,
                };
                let admission = self
                    .backend
                    .lock()
                    .map_err(|_| Failure::Stopped)?
                    .begin_profile_content(self.lease, request)?;
                let reservation = if let Some(sizes) = sizes {
                    let reserved = self
                        .backend
                        .lock()
                        .map_err(|_| Failure::Stopped)?
                        .reserve_profile_media(self.lease, &admission, &sizes);
                    match reserved {
                        Ok(reservation) => Some(reservation),
                        Err(error) => {
                            let _ = self
                                .backend
                                .lock()
                                .map_err(|_| Failure::Stopped)?
                                .fail_profile_content(self.lease, admission);
                            return Err(error.into());
                        }
                    }
                } else {
                    None
                };
                let files = ControlledContentFiles::new(&self.root)
                    .map_err(fc_bootstrap::backend::BackendError::ProfileContent);
                let prepared = (|| {
                    let files = files.as_ref().map_err(Clone::clone)?;
                    let guard = self
                        .backend
                        .lock()
                        .map_err(|_| Failure::Stopped)?
                        .profile_content_preparation_guard(self.lease)?;
                    let state = || {
                        use fc_bootstrap::backend::ProfileContentPreparationState as GuardState;
                        if context.checkpoint().is_err() {
                            return PreparationState::Revoked;
                        }
                        match guard.state() {
                            GuardState::Ready => PreparationState::Ready,
                            GuardState::AuthorityUpdating => PreparationState::AuthorityUpdating,
                            GuardState::Revoked => PreparationState::Revoked,
                        }
                    };
                    let mut preparation = files.preparation_while(reservation.as_ref(), &state);
                    let proof =
                        prepare_profile_content(&mut preparation, admission.clone(), sources)
                            .map_err(fc_bootstrap::backend::BackendError::ProfileContent)?;
                    if context.checkpoint().is_err() || !guard.is_context_valid() {
                        return Err(Failure::Stopped);
                    }
                    // No backend mutex is held during byte copying/hash/WIC.
                    // The sealed original pins remain alive until COMMIT returns.
                    self.backend
                        .lock()
                        .map_err(|_| Failure::Stopped)?
                        .commit_profile_content_while(
                            self.lease,
                            proof,
                            Arc::new(context.execution_fence()),
                        )?;
                    Ok(())
                })(); // every physical preparation/source pin has now really exited
                if let Some(reservation) = reservation {
                    self.backend
                        .lock()
                        .map_err(|_| Failure::Stopped)?
                        .settle_profile_media(reservation)?;
                }
                let mut cleanup_pending = false;
                if let Ok(files) = &files {
                    cleanup_pending = collect_garbage_while(
                        files,
                        &self.backend,
                        &|| context.checkpoint().is_ok(),
                        &Mutex::new(GcCursor::default()),
                    )?;
                }
                if let Err(error) = prepared {
                    let failed = self
                        .backend
                        .lock()
                        .map_err(|_| Failure::Stopped)?
                        .fail_profile_content(self.lease, admission);
                    if let Err(failure) = failed {
                        if super::fatal(&Failure::Backend(failure.clone())) {
                            return Err(failure.into());
                        }
                    }
                    return Err(error);
                }
                let backend = self.backend.lock().map_err(|_| Failure::Stopped)?;
                let outcome = backend.profile_content_outcome(self.lease, command_id)?;
                if !outcome
                    .as_ref()
                    .is_some_and(|outcome| outcome.status == ProfileContentStatus::Committed)
                {
                    return Err(invalid("尚未确认保存结果，请重新打开核对，勿重复提交。"));
                }
                let document = backend.read_profile_content(self.lease, slot)?;
                Ok(Outcome::ProfileContent(Box::new(Loaded {
                    document,
                    outcome,
                    cleanup_pending,
                })))
            }
        }
    }
}
/// Runtime GC never settles a Reserved admission by age. Only the worker which
/// physically exited above can settle its reservation; startup uses a separate
/// exclusive handoff. Busy file owners keep their bytes charged for a later pass.
pub(super) fn maintain(
    root: PathBuf,
    backend: Arc<Mutex<Backend>>,
    cursor: Arc<Mutex<GcCursor>>,
    context: &JobContext,
) -> Result<Outcome> {
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    let files = ControlledContentFiles::new(&root)
        .map_err(fc_bootstrap::backend::BackendError::ProfileContent)?;
    let pending =
        collect_garbage_while(&files, &backend, &|| context.checkpoint().is_ok(), &cursor)?;
    Ok(Outcome::ContentMaintained { pending })
}

#[cfg(test)]
fn collect_garbage(files: &ControlledContentFiles, backend: &Arc<Mutex<Backend>>) -> Result<bool> {
    collect_garbage_while(files, backend, &|| true, &Mutex::new(GcCursor::default()))
}

fn collect_garbage_while(
    files: &ControlledContentFiles,
    backend: &Arc<Mutex<Backend>>,
    current: &dyn Fn() -> bool,
    cursor: &Mutex<GcCursor>,
) -> Result<bool> {
    // One page per physical job. A busy original or a repeated budget expiry
    // cannot force every later job to restart at the registry's prefix.
    let after = cursor.try_lock().map_err(|_| Failure::Stopped)?.after;
    let mut pending = false;
    if !current() {
        return Ok(true);
    }
    let read = match backend.try_lock() {
        Ok(backend) => backend.inventory_profile_media(after, 128),
        Err(std::sync::TryLockError::WouldBlock) => return Ok(true),
        Err(std::sync::TryLockError::Poisoned(_)) => return Err(Failure::Stopped),
    };
    let Some(page) = cleanup_result(read)? else {
        return Ok(true);
    };
    for entry in page.items {
        if !current() {
            return Ok(true);
        }
        let retry = (|| -> Result<bool> {
            if !matches!(entry.state, MediaState::Unreferenced | MediaState::Retiring) {
                return Ok(false);
            }
            let retired = match backend.try_lock() {
                Ok(backend) => backend.retire_profile_media(entry.id, entry.revision),
                Err(std::sync::TryLockError::WouldBlock) => return Ok(true),
                Err(std::sync::TryLockError::Poisoned(_)) => return Err(Failure::Stopped),
            };
            let Some(ticket) = cleanup_result(retired)? else {
                return Ok(true);
            };
            if !current() {
                return Ok(true);
            }
            if files.remove_retired(&ticket).is_err() {
                return Ok(true);
            }
            if !current() {
                return Ok(true);
            }
            let finalized = match backend.try_lock() {
                Ok(backend) => backend.finalize_profile_media_removal(ticket),
                Err(std::sync::TryLockError::WouldBlock) => return Ok(true),
                Err(std::sync::TryLockError::Poisoned(_)) => return Err(Failure::Stopped),
            };
            Ok(cleanup_result(finalized)?.is_none())
        })()?;
        pending |= retry;
        if !current() {
            return Ok(true);
        }
        // No cursor lock spans IO/SQL. A cancelled result cannot erase this
        // progress; failed resources are revisited after wraparound.
        cursor.try_lock().map_err(|_| Failure::Stopped)?.after = Some(entry.id);
    }
    if page.next_after.is_none() {
        cursor.try_lock().map_err(|_| Failure::Stopped)?.after = None;
    }
    Ok(pending || page.next_after.is_some())
}

/// A concurrent cleanup may advance this resource CAS after the inventory read.
/// That retry cannot turn a known committed publication into a reported failure.
/// Actual authority faults still close the application.
fn cleanup_result<T>(result: fc_bootstrap::backend::Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            let failure = Failure::Backend(error);
            if super::fatal(&failure) {
                Err(failure)
            } else {
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
#[path = "profile_content_flow_tests.rs"]
mod tests;
