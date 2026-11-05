//! Worker-owned recovery payloads; the UI receives only lightweight job and
//! confirmed-operation capabilities. No HTTP body supplies a filesystem path.
use super::*;
use crate::{
    backend::{Backend, OrdinaryBackupAccess},
    recovery_files::ExportReceipt,
    recovery_workflow::{ConfirmedBundle, MigrationBundle, RecoveryWorkflow, ReviewBundle},
};
use fc_application::identity::SessionLease;
use fc_domain::recovery::MigrationMapping;
use fc_protocol::web::maintenance::{self as wire, Mode};
use fc_runtime::worker::JobContext as WorkerContext;
use fc_storage::recovery::ExecutionFence;
use fc_windows::{materials::files::ControlledFiles, system::DataPaths};

mod files;
pub use files::{MaintenanceDownload, MaintenanceUpload, UploadedBackup};
pub const MAINTENANCE_TRANSFER_BUDGET: std::time::Duration =
    std::time::Duration::from_secs(30 * 60);

struct Active {
    reset_review: Option<crate::reinitialization_files::ResetReview>,
    stage_name: String,
    workflow: RecoveryWorkflow,
    job: JobContext,
    lease: SessionLease,
    revision: Revision,
    kind: Kind,
    mode: Option<Mode>,
    expected_bytes: Option<u64>,
    uploaded: Option<UploadedBackup>,
    exported: Option<ExportReceipt>,
    review: Option<ReviewBundle>,
    mapping: Option<MigrationBundle>,
}
pub struct MaintenanceService {
    paths: DataPaths,
    closing: AtomicBool,
    pub control: MaintenanceControl,
    active: Mutex<Option<Active>>,
    work: Mutex<Option<MaintenanceWork>>,
    confirmed: Mutex<Option<ConfirmedMaintenance>>,
}
pub enum ConfirmedMaintenance {
    Reset {
        job: JobContext,
        prepared: Box<crate::reinitialization_files::PreparedReinitialization>,
    },
    Restore {
        job: JobContext,
        bundle: ConfirmedBundle,
    },
    AbortOriginal {
        job: JobContext,
        workflow: RecoveryWorkflow,
    },
}
pub enum MaintenanceWorkKind {
    ResetReview,
    Export,
    Preflight,
    Mapping(MigrationMapping),
    Confirm,
    Retire,
}
pub struct MaintenanceWork {
    service: Arc<MaintenanceService>,
    id: u64,
    kind: MaintenanceWorkKind,
}
impl MaintenanceService {
    pub fn new(paths: DataPaths) -> Arc<Self> {
        Arc::new(Self {
            paths,
            closing: AtomicBool::new(false),
            control: MaintenanceControl::default(),
            active: Mutex::new(None),
            work: Mutex::new(None),
            confirmed: Mutex::new(None),
        })
    }
    pub fn create(
        self: &Arc<Self>,
        lease: SessionLease,
        request: wire::Create,
        random: &mut impl RandomPort,
    ) -> Result<CreatedJob> {
        if self.closing.load(Ordering::Acquire) {
            return Err(MaintenanceError::Unavailable);
        }
        let mut active = self.active.try_lock().map_err(|_| MaintenanceError::Busy)?;
        if let Some(previous) = active.as_mut() {
            if !previous
                .job
                .control
                .0
                .lock()
                .map_err(|_| MaintenanceError::Retained)?
                .current
                .as_ref()
                .is_some_and(|v| v.snapshot.phase.terminal())
            {
                return Err(MaintenanceError::Busy);
            }
            self.retire(previous)?;
        }
        let revision = request.revision().map_err(|_| MaintenanceError::Invalid)?;
        let expected_bytes = if request.kind == Kind::Import {
            Some(
                request
                    .expected_bytes()
                    .map_err(|_| MaintenanceError::Invalid)?,
            )
        } else {
            None
        };
        let mut stage_nonce = [0u8; 16];
        random
            .fill(&mut stage_nonce)
            .map_err(|_| MaintenanceError::Unavailable)?;
        let stage_name = hex(&stage_nonce);
        let (mut created, job) =
            self.control
                .begin(lease.principal_id(), revision, request.kind, random)?;
        job.update(
            if request.kind == Kind::Import {
                Phase::ReadyForUpload
            } else {
                Phase::Waiting
            },
            |s| {
                s.file_name = request.file_name;
                s.bytes = request.bytes;
            },
        )?;
        created.snapshot = job.snapshot()?;
        *active = Some(Active {
            stage_name,
            reset_review: None,
            workflow: RecoveryWorkflow::new(self.paths.clone()),
            job,
            lease,
            revision,
            kind: request.kind,
            mode: request.mode,
            expected_bytes,
            uploaded: None,
            exported: None,
            review: None,
            mapping: None,
        });
        if self.closing.load(Ordering::Acquire) {
            if let Some(active) = active.as_ref() {
                active.job.cancelled.store(true, Ordering::Release);
            }
            return Err(MaintenanceError::Unavailable);
        }
        drop(active);
        if request.kind != Kind::Import {
            self.offer(
                created.id,
                if request.kind == Kind::Export {
                    MaintenanceWorkKind::Export
                } else {
                    MaintenanceWorkKind::ResetReview
                },
            )?;
            created.snapshot.phase = Phase::Working;
        }
        Ok(created)
    }
    fn offer(self: &Arc<Self>, id: u64, kind: MaintenanceWorkKind) -> Result<()> {
        let mut queue = self.work.lock().map_err(|_| MaintenanceError::Retained)?;
        if queue.is_some() {
            return Err(MaintenanceError::Busy);
        }
        let expected = match &kind {
            MaintenanceWorkKind::Export | MaintenanceWorkKind::ResetReview => Phase::Waiting,
            MaintenanceWorkKind::Preflight => Phase::Uploaded,
            MaintenanceWorkKind::Mapping(_) => Phase::MappingRequired,
            MaintenanceWorkKind::Confirm => Phase::ReviewReady,
            MaintenanceWorkKind::Retire => return Err(MaintenanceError::Invalid),
        };
        let mut state = self
            .control
            .0
            .lock()
            .map_err(|_| MaintenanceError::Retained)?;
        let current = state
            .current
            .as_mut()
            .filter(|v| v.id == id)
            .ok_or(MaintenanceError::Stale)?;
        if current.snapshot.phase != expected || current.cancelled.load(Ordering::Acquire) {
            return Err(MaintenanceError::Conflict);
        }
        current.snapshot.phase = Phase::Working;
        *queue = Some(MaintenanceWork {
            service: self.clone(),
            id,
            kind,
        });
        Ok(())
    }
    /// Return the exact task rejected by the shared pool. No capability or
    /// filesystem ownership is transferred to an unbounded retry queue.
    pub fn return_work(&self, work: MaintenanceWork) {
        let mut queue = self.work.lock().unwrap_or_else(|error| error.into_inner());
        if queue.is_none() {
            *queue = Some(work);
        } else {
            // The current phase reservation prevents a second normal offer.
            // An inconsistent queued request grants no cold-commit capability.
            if let Ok(active) = self.active.try_lock() {
                if let Some(active) = active.as_ref().filter(|v| v.job.id() == work.id) {
                    let _ = active.job.fail("维护任务队列发生冲突，请重新开始。");
                }
            }
        }
    }
    pub fn retirement_complete(&self) -> bool {
        let Ok(active) = self.active.try_lock() else {
            return false;
        };
        let Ok(work) = self.work.try_lock() else {
            return false;
        };
        let Ok(confirmed) = self.confirmed.try_lock() else {
            return false;
        };
        active.is_none() && work.is_none() && confirmed.is_none()
    }
    pub fn request_close(&self) {
        self.closing.store(true, Ordering::Release);
        if let Ok(state) = self.control.0.lock() {
            if let Some(current) = state.current.as_ref() {
                if !matches!(
                    current.snapshot.phase,
                    Phase::Draining
                        | Phase::PreparingDatabase
                        | Phase::Sealing
                        | Phase::Committing
                        | Phase::Retained
                ) {
                    current.cancelled.store(true, Ordering::Release);
                }
            }
        }
    }
    pub fn take_work(self: &Arc<Self>) -> Option<MaintenanceWork> {
        if let Ok(mut queue) = self.work.try_lock() {
            if let Some(work) = queue.take() {
                return Some(work);
            }
        }
        let active = self.active.try_lock().ok()?;
        let active = active.as_ref()?;
        if active.job.cancelled.load(Ordering::Acquire)
            && !active.workflow.has_confirmed()
            && active.job.snapshot().is_ok_and(|s| {
                !matches!(
                    s.phase,
                    Phase::Draining
                        | Phase::PreparingDatabase
                        | Phase::Sealing
                        | Phase::Committing
                        | Phase::Retained
                )
            })
        {
            Some(MaintenanceWork {
                service: self.clone(),
                id: active.job.id(),
                kind: MaintenanceWorkKind::Retire,
            })
        } else {
            None
        }
    }
    pub fn take_confirmed(&self) -> Option<ConfirmedMaintenance> {
        self.confirmed.try_lock().ok()?.take()
    }
    pub fn context(&self, id: u64, proof: &WebSecret, lease: SessionLease) -> Result<JobContext> {
        self.control.authorize(id, proof, lease.principal_id())
    }
    pub fn request_preflight(
        self: &Arc<Self>,
        id: u64,
        proof: &WebSecret,
        lease: SessionLease,
    ) -> Result<()> {
        self.context(id, proof, lease)?;
        let active = self.active.try_lock().map_err(|_| MaintenanceError::Busy)?;
        if active
            .as_ref()
            .filter(|v| v.job.id() == id)
            .is_none_or(|v| v.uploaded.is_none())
        {
            return Err(MaintenanceError::Conflict);
        }
        drop(active);
        self.offer(id, MaintenanceWorkKind::Preflight)
    }
    pub fn request_mapping(
        self: &Arc<Self>,
        id: u64,
        proof: &WebSecret,
        lease: SessionLease,
        mapping: wire::Mapping,
    ) -> Result<()> {
        self.context(id, proof, lease)?;
        let active = self.active.try_lock().map_err(|_| MaintenanceError::Busy)?;
        let value = active
            .as_ref()
            .filter(|v| v.job.id() == id)
            .ok_or(MaintenanceError::Stale)?;
        if value.mapping.is_none() || mapping.expected_revision != value.revision.get().to_string()
        {
            return Err(MaintenanceError::Conflict);
        }
        drop(active);
        self.offer(
            id,
            MaintenanceWorkKind::Mapping(mapping.domain().map_err(|_| MaintenanceError::Invalid)?),
        )
    }
    pub fn request_confirm(
        self: &Arc<Self>,
        id: u64,
        proof: &WebSecret,
        lease: SessionLease,
        confirm: wire::Confirm,
    ) -> Result<()> {
        self.context(id, proof, lease)?;
        let active = self.active.try_lock().map_err(|_| MaintenanceError::Busy)?;
        let value = active
            .as_ref()
            .filter(|v| v.job.id() == id)
            .ok_or(MaintenanceError::Stale)?;
        if (value.review.is_none() && value.reset_review.is_none())
            || confirm.expected_revision != value.revision.get().to_string()
            || !confirm.reviewed_data
            || !confirm.understood_replacement
        {
            return Err(MaintenanceError::Conflict);
        }
        drop(active);
        self.offer(id, MaintenanceWorkKind::Confirm)
    }
    fn retire(&self, active: &mut Active) -> Result<()> {
        active.reset_review = None;
        active.review = None;
        active.mapping = None;
        if let Some(receipt) = active.exported.as_ref() {
            ControlledFiles::new(self.paths.root())
                .map_err(|_| MaintenanceError::Unavailable)?
                .remove(&receipt.path)
                .map_err(|_| MaintenanceError::Unavailable)?;
        }
        active.exported = None;
        active.workflow.request_close();
        if !active.workflow.retire_pending_on_worker() {
            return Err(MaintenanceError::Retained);
        }
        if let Some(upload) = active.uploaded.as_mut() {
            upload.retire()?;
        }
        active.uploaded = None;
        Ok(())
    }
}
impl MaintenanceWork {
    pub fn allocation_bytes(&self) -> Option<usize> {
        Some(
            4096 + match &self.kind {
                MaintenanceWorkKind::Mapping(v) => {
                    (v.classes.capacity() + v.subjects.capacity() + v.principals.capacity()) * 16
                }
                _ => 0,
            },
        )
    }
    pub fn run(self, backend: Arc<Mutex<Backend>>, context: &WorkerContext) -> Result<()> {
        let service = &self.service;
        let mut guard = service
            .active
            .try_lock()
            .map_err(|_| MaintenanceError::Busy)?;
        let active = guard
            .as_mut()
            .filter(|v| v.job.id() == self.id)
            .ok_or(MaintenanceError::Stale)?;
        if matches!(self.kind, MaintenanceWorkKind::Retire) {
            service.retire(active)?;
            active.job.update(Phase::Cancelled, |_| {})?;
            *guard = None;
            return Ok(());
        }
        let access: OrdinaryBackupAccess = match backend
            .lock()
            .map_err(|_| MaintenanceError::Unavailable)
            .and_then(|backend| {
                backend
                    .ordinary_backup_access(active.lease, active.revision)
                    .map_err(|_| MaintenanceError::Denied)
            }) {
            Ok(access) => access,
            Err(error) => {
                active.job.fail("当前登录或数据版本已变化，请重新预检。")?;
                return Err(error);
            }
        };
        let job = active.job.clone();
        let execution = context.execution_fence();
        let fence: ExecutionFence = Arc::new(move || job.active() && execution());
        active.job.update(Phase::Working, |s| s.error = None)?;
        let mut failure_hint = None;
        let result = (|| {
            match self.kind {
                MaintenanceWorkKind::ResetReview => {
                    let files =
                        crate::reinitialization_files::ReinitializationFiles::new(&service.paths)
                            .map_err(|_| MaintenanceError::Unavailable)?;
                    let reviewed = files
                        .review(active.job.id(), &*fence)
                        .map_err(|_| MaintenanceError::Unavailable)?;
                    let scope = wire::ResetScope {
                        present_tree_count: reviewed.present_tree_count(),
                        scope: vec![
                            "账号与凭据",
                            "班级、学科与任教关系",
                            "课程表与设置布局",
                            "资料原件、缩略图与索引",
                            "提醒、录音与命令历史",
                        ],
                    };
                    active.reset_review = Some(reviewed);
                    active
                        .job
                        .update(Phase::ReviewReady, |s| s.reset_review = Some(scope))?;
                }
                MaintenanceWorkKind::Export => {
                    let receipt = active
                        .workflow
                        .export(access, fence.clone())
                        .map_err(|_| MaintenanceError::Unavailable)?;
                    let bytes = receipt.bytes;
                    let sha = hex(&receipt.package_sha256);
                    active.exported = Some(receipt);
                    active.job.update(Phase::Finished, |s| {
                        s.bytes = Some(bytes.to_string());
                        s.sha256 = Some(sha);
                        s.file_name = Some("future-classroom-backup.fcbackup".into());
                    })?;
                }
                MaintenanceWorkKind::Preflight => {
                    let mut uploaded = active.uploaded.take().ok_or(MaintenanceError::Conflict)?;
                    let pin = uploaded.take_pin()?;
                    active.uploaded = Some(uploaded);
                    let outcome = if active.mode == Some(Mode::Mapped) {
                        active
                            .workflow
                            .inspect_migration_pinned(pin, access, fence.clone())
                            .map(|bundle| {
                                let mapping = mapping_review(bundle.details());
                                active.mapping = Some(bundle);
                                active
                                    .job
                                    .update(Phase::MappingRequired, |s| s.mapping = Some(mapping))
                            })
                    } else {
                        active
                            .workflow
                            .preflight_pinned(pin, access, fence.clone())
                            .map(|bundle| {
                                let review = review(bundle.summary());
                                active.review = Some(bundle);
                                active
                                    .job
                                    .update(Phase::ReviewReady, |s| s.review = Some(review))
                            })
                    };
                    // RecoveryFiles::preflight_pinned has extracted and pinned
                    // its own workspace and released the original package before
                    // either workflow returns. Retire our upload on this worker
                    // before confirmation can stop the shared pool.
                    if let Err(error) = &outcome {
                        use crate::{
                            recovery_files::RecoveryFileError as F,
                            recovery_workflow::WorkflowError as W,
                        };
                        failure_hint = Some(match error {
                            W::Files(F::InvalidPackage) => {
                                "备份包格式或完整性预检未通过，正式数据未修改。"
                            }
                            W::Files(F::Unavailable) => {
                                "无法读取受控备份或创建预检工作区，正式数据未修改。"
                            }
                            W::Files(F::RepairRequired) => {
                                "预检工作区需要修复或清理，正式数据未修改。"
                            }
                            W::Backend(_) => "备份数据或恢复权限预检未通过，正式数据未修改。",
                            W::Busy => "恢复工作区仍被占用，请先结束上一项维护。",
                            W::Cancelled | W::Files(F::Cancelled) => {
                                "备份预检已取消，正式数据未修改。"
                            }
                            W::Shutdown(_) => "恢复协调器未完成准备，正式数据未修改。",
                        });
                    }
                    if let Some(upload) = active.uploaded.as_mut() {
                        upload.retire().inspect_err(|_| {
                            failure_hint = Some("备份上传暂存文件尚未完成清理，正式数据未修改。");
                        })?;
                    }
                    active.uploaded = None;
                    outcome.map_err(|_| MaintenanceError::Unavailable)??;
                }
                MaintenanceWorkKind::Mapping(mapping) => {
                    let bundle = active.mapping.take().ok_or(MaintenanceError::Conflict)?;
                    let review_bundle = active
                        .workflow
                        .preflight_mapping(bundle, mapping, access, fence.clone())
                        .map_err(|_| MaintenanceError::Unavailable)?;
                    let summary = review(review_bundle.summary());
                    active.review = Some(review_bundle);
                    active.job.update(Phase::ReviewReady, |s| {
                        s.review = Some(summary);
                        s.mapping = None;
                    })?;
                }
                MaintenanceWorkKind::Confirm if active.kind == Kind::Reset => {
                    let review = active
                        .reset_review
                        .take()
                        .ok_or(MaintenanceError::Conflict)?;
                    active.job.update(Phase::Confirming, |_| {})?;
                    let files =
                        crate::reinitialization_files::ReinitializationFiles::new(&service.paths)
                            .map_err(|_| MaintenanceError::Unavailable)?;
                    let prepared = files
                        .prepare(review, &*fence)
                        .map_err(|_| MaintenanceError::Unavailable)?;
                    // Close ordinary admission only after the exact live page,
                    // global revision and physical prepared scope still agree.
                    let admitted = (|| {
                        let backend = backend.lock().map_err(|_| MaintenanceError::Unavailable)?;
                        if !fence() {
                            return Err(MaintenanceError::Cancelled);
                        }
                        backend
                            .ordinary_backup_access(active.lease, active.revision)
                            .map_err(|_| MaintenanceError::Denied)?
                            .check_revision()
                            .map_err(|_| MaintenanceError::Denied)?;
                        backend.close_reinitialization_admission().map_err(|error| {
                            if matches!(
                                error,
                                crate::backend::BackendError::Storage(
                                    fc_storage::StorageError::Busy
                                )
                            ) {
                                MaintenanceError::Busy
                            } else {
                                MaintenanceError::Retained
                            }
                        })
                    })();
                    if let Err(error) = admitted {
                        if error != MaintenanceError::Retained {
                            prepared
                                .discard(&|| true)
                                .map_err(|_| MaintenanceError::Retained)?;
                            return Err(error);
                        }
                    }
                    active
                        .job
                        .update(Phase::Draining, |s| s.restart_required = true)?;
                    *service
                        .confirmed
                        .lock()
                        .map_err(|_| MaintenanceError::Retained)? =
                        Some(ConfirmedMaintenance::Reset {
                            job: active.job.clone(),
                            prepared: Box::new(prepared),
                        });
                }
                MaintenanceWorkKind::Confirm => {
                    let review = active.review.take().ok_or(MaintenanceError::Conflict)?;
                    active.job.update(Phase::Confirming, |_| {})?;
                    let confirmed = active.workflow.confirm(
                        review,
                        backend.clone(),
                        active.lease,
                        fence.clone(),
                    );
                    if let Err(error) = &confirmed {
                        use crate::recovery_workflow::WorkflowError as W;
                        let stage = match error {
                            W::Busy => "busy",
                            W::Cancelled => "cancelled",
                            W::Backend(crate::backend::BackendError::Storage(
                                fc_storage::StorageError::AuthorityUnavailable,
                            )) => "writer-authority-unavailable",
                            W::Backend(crate::backend::BackendError::Storage(
                                fc_storage::StorageError::Conflict { .. },
                            )) => "writer-revision-conflict",
                            W::Backend(crate::backend::BackendError::Storage(_)) => "writer",
                            W::Backend(_) => "backend",
                            W::Files(_) => "files",
                            W::Shutdown(_) => "shutdown",
                        };
                        eprintln!("未来教室恢复确认未完成：{stage}。");
                    }
                    if let Some(bundle) = active.workflow.confirmed_bundle() {
                        active
                            .job
                            .update(Phase::Draining, |s| s.restart_required = true)?;
                        *service
                            .confirmed
                            .lock()
                            .map_err(|_| MaintenanceError::Retained)? =
                            Some(ConfirmedMaintenance::Restore {
                                job: active.job.clone(),
                                bundle,
                            });
                    } else if backend
                        .lock()
                        .is_ok_and(|b| b.session_control().is_recovering())
                    {
                        active.job.update(Phase::Retained, |s| {
                            s.restart_required = true;
                            s.error = Some("恢复确认未能完成，正在保留原有数据并关闭后台。".into());
                        })?;
                        *service
                            .confirmed
                            .lock()
                            .map_err(|_| MaintenanceError::Retained)? =
                            Some(ConfirmedMaintenance::AbortOriginal {
                                job: active.job.clone(),
                                workflow: active.workflow.clone(),
                            });
                    } else {
                        confirmed.map_err(|_| MaintenanceError::Unavailable)?;
                    }
                }
                MaintenanceWorkKind::Retire => unreachable!(),
            }
            Ok(())
        })();
        if let Err(error) = result {
            if active.workflow.has_confirmed() {
                active
                    .job
                    .update(Phase::Retained, |s| s.restart_required = true)?;
            } else {
                active
                    .job
                    .fail(failure_hint.unwrap_or("维护操作未完成，正式数据未由本次预检修改。"))?;
            }
            return Err(error);
        }
        Ok(())
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn review(value: &crate::recovery_workflow::ReviewSummary) -> wire::Review {
    wire::Review {
        school_id: value.school_id.get().to_string(),
        source_revision: value.source_revision.get().to_string(),
        current_revision: value.current_revision.get().to_string(),
        document_count: value.document_count.to_string(),
        material_count: value.material_count,
        profile_media_count: value.profile_media_count,
        payload_bytes: value.payload_bytes.to_string(),
        database_bytes: value.database_bytes.to_string(),
        material_bytes: value.material_bytes.to_string(),
        profile_media_bytes: value.profile_media_bytes.to_string(),
    }
}
fn mapping_review(v: &crate::recovery_workflow::MigrationDetails) -> wire::MappingReview {
    let source = &v.source_catalog;
    let target = &v.target_identity;
    let label = |id: u64, name: String| wire::Label {
        id: id.to_string(),
        name,
    };
    wire::MappingReview {
        source: wire::Catalog {
            school_id: source.school_id.get().to_string(),
            school_name: source.school_name.clone(),
            classes: source
                .classes
                .iter()
                .map(|v| label(v.id.get(), v.name.clone()))
                .collect(),
            subjects: source
                .subjects
                .iter()
                .map(|v| label(v.id.get(), v.name.clone()))
                .collect(),
            principals: source
                .principals
                .iter()
                .map(|v| label(v.id.get(), v.display_name.clone()))
                .collect(),
        },
        target: wire::Catalog {
            school_id: target
                .installation
                .as_ref()
                .map_or(String::new(), |v| v.school_id.get().to_string()),
            school_name: target
                .installation
                .as_ref()
                .map_or(String::new(), |v| v.school_name.clone()),
            classes: target
                .classes
                .iter()
                .map(|v| label(v.id.get(), v.name.clone()))
                .collect(),
            subjects: target
                .subjects
                .iter()
                .map(|v| label(v.id.get(), v.name.clone()))
                .collect(),
            principals: target
                .accounts
                .iter()
                .map(|v| label(v.principal.id.get(), v.display_name.clone()))
                .collect(),
        },
        current_revision: v.target_global_revision.get().to_string(),
    }
}
