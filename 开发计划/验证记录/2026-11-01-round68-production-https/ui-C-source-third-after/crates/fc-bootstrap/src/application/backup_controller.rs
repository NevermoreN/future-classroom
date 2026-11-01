use super::*;
use fc_bootstrap::{
    recovery_files::ExportReceipt,
    recovery_workflow::{
        ConfirmationState, MigrationBundle, PreparedCoordinator, RecoveryWorkflow,
        RestoreOperation, RestorePhase, ReviewBundle, WorkflowError,
    },
};
use fc_windows::materials::{
    chooser::{BackupDestination, SelectedFile},
    files::LockedFile,
};

const CANCEL_RESTORE: ActionId = ActionId(678);
const CLOSE_RESTORE: ActionId = ActionId(679);

#[derive(Default)]
pub(super) struct State {
    pub workflow: Option<RecoveryWorkflow>,
    pub(super) editor: Option<backup_editor::Editor>,
    pub(super) job: Option<Pending>,
    pub(super) cleanup: Option<JobTicket<Result<Outcome>>>,
    pub(super) sequence: u64,
    pub(super) job_sequence: u64,
    pub(super) job_owner: Option<PrincipalId>,
    pub(super) selected_name: String,
    pub(super) review: Option<ReviewBundle>,
    pub(super) migration: Option<migration_editor::Editor>,
    pub(super) mapping: Option<MigrationBundle>,
    pub(super) mapping_draft_revision: u64,
    pub(super) mapping_target_revision: Option<Revision>,
    pub(super) migration_inspection_sequence: Option<u64>,
    pub(super) exported: Option<ExportReceipt>,
    pub(super) irreversible: bool,
    pub(super) coordinator: Option<RestoreOperation<Work>>,
    pub(super) last_phase: Option<RestorePhase>,
    pub(super) finished: bool,
    pub(super) acknowledged: bool,
    start_failed: bool,
}
pub(super) enum Job {
    InspectMigration(
        RecoveryWorkflow,
        Arc<Mutex<Backend>>,
        SessionLease,
        SelectedFile,
    ),
    MapMigration(
        RecoveryWorkflow,
        Arc<Mutex<Backend>>,
        SessionLease,
        MigrationBundle,
        fc_domain::recovery::MigrationMapping,
        u64,
    ),
    Export(RecoveryWorkflow, Arc<Mutex<Backend>>, SessionLease),
    Preflight(
        RecoveryWorkflow,
        Arc<Mutex<Backend>>,
        SessionLease,
        SelectedFile,
    ),
    Confirm(
        RecoveryWorkflow,
        Arc<Mutex<Backend>>,
        SessionLease,
        ReviewBundle,
    ),
    Copy(
        Arc<Mutex<Backend>>,
        SessionLease,
        ExportReceipt,
        BackupDestination,
    ),
    Retire(RecoveryWorkflow),
}
pub(super) enum Completed {
    MigrationInspected(MigrationBundle),
    MigrationPreflighted {
        review: ReviewBundle,
        draft_revision: u64,
    },
    Exported(ExportReceipt),
    Preflighted(ReviewBundle),
    Confirmed,
    Copied {
        path: String,
        bytes: u64,
    },
    Retired,
}
fn failure(error: WorkflowError) -> Failure {
    match error {
        WorkflowError::Backend(error) => Failure::Backend(error),
        WorkflowError::Cancelled => Failure::Stopped,
        WorkflowError::Busy => Failure::Deployment("上一次备份工作仍在结束，请稍后重试。"),
        WorkflowError::Files(_) => {
            Failure::Deployment("备份预检或文件操作失败；正式资料未被本次预检修改。")
        }
        WorkflowError::Shutdown(_) => {
            Failure::Deployment("恢复未能完成，请保留数据并重新启动核对。")
        }
    }
}
fn access(
    backend: &Arc<Mutex<Backend>>,
    lease: SessionLease,
) -> Result<fc_bootstrap::backend::OrdinaryBackupAccess> {
    let backend = backend.lock().map_err(|_| Failure::Stopped)?;
    let revision = backend.global_revision()?;
    Ok(backend.ordinary_backup_access(lease, revision)?)
}
impl Job {
    pub fn allocation_bytes(&self) -> Option<usize> {
        match self {
            Self::InspectMigration(_, _, _, selected) => {
                selected.allocation_bytes().checked_add(4096)
            }
            Self::MapMigration(_, _, _, bundle, mapping, _) => {
                let pairs = mapping
                    .classes
                    .capacity()
                    .checked_add(mapping.subjects.capacity())?
                    .checked_add(mapping.principals.capacity())?;
                bundle
                    .allocation_bytes()?
                    .checked_add(pairs.checked_mul(16)?)?
                    .checked_add(4096)
            }
            Self::Preflight(_, _, _, selected) => selected.allocation_bytes().checked_add(4096),
            Self::Copy(_, _, receipt, destination) => destination
                .allocation_bytes()
                .checked_add(receipt.path.as_os_str().len().checked_mul(2)?)?
                .checked_add(4096),
            _ => Some(4096),
        }
    }
    pub fn run(self, context: &JobContext) -> Result<Outcome> {
        let fence: fc_storage::recovery::ExecutionFence = Arc::new(context.execution_fence());
        let result = match self {
            Self::InspectMigration(workflow, backend, lease, selected) => {
                workflow.retire_pending_on_worker();
                let access = access(&backend, lease)?;
                Completed::MigrationInspected(
                    workflow
                        .inspect_migration(selected, access, fence)
                        .map_err(failure)?,
                )
            }
            Self::MapMigration(workflow, backend, lease, bundle, mapping, draft_revision) => {
                let access = access(&backend, lease)?;
                Completed::MigrationPreflighted {
                    review: workflow
                        .preflight_mapping(bundle, mapping, access, fence)
                        .map_err(failure)?,
                    draft_revision,
                }
            }
            Self::Export(workflow, backend, lease) => {
                let access = access(&backend, lease)?;
                Completed::Exported(workflow.export(access, fence).map_err(failure)?)
            }
            Self::Preflight(workflow, backend, lease, selected) => {
                workflow.retire_pending_on_worker();
                let access = access(&backend, lease)?;
                Completed::Preflighted(
                    workflow
                        .preflight(selected, access, fence)
                        .map_err(failure)?,
                )
            }
            Self::Confirm(workflow, backend, lease, review) => {
                // The workflow retains the durable capability even if the
                // generic worker discards this result after cancellation.
                workflow
                    .confirm(review, backend, lease, fence)
                    .map_err(failure)?;
                Completed::Confirmed
            }
            Self::Copy(backend, lease, receipt, destination) => {
                let access = Arc::new(access(&backend, lease)?);
                let check = || fence() && access.check_revision().is_ok();
                if !check() {
                    return Err(Failure::Stopped);
                }
                let source = LockedFile::open(&receipt.path, receipt.bytes)
                    .map_err(|_| Failure::Deployment("已导出的备份不可读取，请重新导出。"))?;
                let copied = destination
                    .copy_from(source, receipt.bytes, receipt.package_sha256, check)
                    .map_err(|_| {
                        Failure::Deployment(
                            "备份另存未完成；请选择本地可写位置和新的文件名后重试。",
                        )
                    })?;
                Completed::Copied {
                    path: copied.path_display,
                    bytes: copied.bytes,
                }
            }
            Self::Retire(workflow) => {
                workflow.retire_pending_on_worker();
                Completed::Retired
            }
        };
        Ok(Outcome::Backup(result))
    }
}
impl State {
    pub fn irreversible(&self) -> bool {
        self.irreversible
    }
    pub fn exit_ready(&self) -> bool {
        !self.irreversible || self.finished && self.acknowledged
    }
    pub fn retirement_complete(&self) -> bool {
        self.job.is_none()
            && self.cleanup.is_none()
            && self.workflow.as_ref().is_none_or(RecoveryWorkflow::is_idle)
    }
    pub fn has_review(&self, owner: PrincipalId) -> bool {
        self.migration
            .as_ref()
            .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
            || self
                .editor
                .as_ref()
                .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
    }
    pub fn view(&self, status: &str) -> Option<View> {
        if self.irreversible {
            return Some(View { title: "正在恢复教学资料".into(), description: "旧会话已失效。当前密码、账号权限和登录限速保留。请等待真实后台任务、声音和文件操作结束；完成后关闭程序，再重新启动并登录。".into(), status: status.into(), fields: vec![], actions: if self.finished {
                vec![Action { id: CLOSE_RESTORE, label: "关闭程序，稍后重新启动".into(), enabled: true }]
            } else { vec![Action { id: CANCEL_RESTORE, label: "请求安全中止并关闭".into(), enabled: self.coordinator.is_some() }] }, busy: false, grid: None });
        }
        if let Some(editor) = &self.migration {
            let mut view = editor.view(status, self.job.is_some() || self.cleanup.is_some());
            view.actions.push(Action {
                id: LOCK,
                label: "锁定".into(),
                enabled: true,
            });
            return Some(view);
        }
        self.editor.as_ref().map(|e| {
            let mut view = e.view(status, self.job.is_some() || self.cleanup.is_some());
            // The editor disables every ordinary action while a file job runs.
            // Keep the explicitly enabled cancellation and lock controls usable.
            view.busy = false;
            view.actions.push(Action {
                id: LOCK,
                label: "锁定".into(),
                enabled: true,
            });
            view
        })
    }
}
impl App {
    pub(super) fn open_backup(&mut self) -> std::result::Result<(), String> {
        let Some(owner) = self.current_global_owner() else {
            self.lock();
            return self.render();
        };
        if self
            .control
            .as_ref()
            .is_none_or(SessionControl::is_local_locked)
        {
            self.lock();
            return self.render();
        }
        if self.backup.job.is_some() || self.backup.cleanup.is_some() {
            self.status = "上一项备份任务仍在结束，请稍后重试。".into();
            return self.render();
        }
        self.backup.review = None;
        self.backup.mapping = None;
        self.backup.migration = None;
        self.backup.mapping_target_revision = None;
        self.backup.migration_inspection_sequence = None;
        self.backup.exported = None;
        if self.backup.workflow.is_none() {
            return Err("备份服务尚未准备完成。".into());
        }
        self.backup.editor = Some(backup_editor::Editor::new(owner));
        self.screen = Screen::Backup;
        self.status.clear();
        self.render()
    }
    pub(super) fn lock_backup(&mut self) {
        if self.backup.irreversible {
            return;
        }
        self.backup.review = None;
        self.backup.mapping = None;
        if let Some(editor) = &mut self.backup.migration {
            editor.invalidate();
        }
        self.backup.migration = None;
        self.backup.mapping_target_revision = None;
        self.backup.migration_inspection_sequence = None;
        self.backup.exported = None;
        if let Some(editor) = &mut self.backup.editor {
            editor.invalidate();
        }
        self.backup.editor = None;
        if let Some(job) = &self.backup.job {
            job.ticket.cancel();
        }
    }
    pub(super) fn close_backup(&mut self) {
        if let Some(coordinator) = &self.backup.coordinator {
            coordinator.cancel();
        }
        if self.backup.irreversible {
            self.backup.acknowledged = true;
            return;
        }
        self.lock_backup();
        if let Some(workflow) = &self.backup.workflow {
            workflow.request_close();
        }
    }
    pub(super) fn submit_backup(
        &mut self,
        job: Job,
        owner: PrincipalId,
        sequence: u64,
    ) -> std::result::Result<(), String> {
        if self.backup.job.is_some() || self.backup.cleanup.is_some() || self.stopping {
            return Ok(());
        }
        match self.pool.handle().try_submit(
            Work::Backup(Box::new(job)),
            Instant::now() + Duration::from_secs(30 * 60),
        ) {
            Ok(ticket) => {
                self.backup.job = Some(Pending {
                    ticket,
                    generation: self.generation,
                    self_revokes: false,
                    public_lesson: false,
                });
                self.backup.job_owner = Some(owner);
                self.backup.job_sequence = sequence;
                self.status = "正在处理备份；可取消或锁定，后台会在实际结束后清理。".into();
            }
            Err(_) => {
                if let Some(editor) = &mut self.backup.editor {
                    editor.cancel_review();
                }
                self.status = "后台繁忙，请稍后重试。".into();
            }
        }
        self.render()
    }
    pub(super) fn preflight_selected_backup(
        &mut self,
        selected: SelectedFile,
    ) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        if self.current_global_owner() != Some(lease.principal_id())
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
        {
            self.lock();
            return self.render();
        }
        let workflow = self
            .backup
            .workflow
            .clone()
            .ok_or("备份服务尚未准备完成。")?;
        if !workflow.is_idle() && !workflow.retirement_pending() {
            self.status = "上一份备份仍在结束或清理，请稍后重新选择。".into();
            return self.render();
        }
        self.backup.sequence = self
            .backup
            .sequence
            .checked_add(1)
            .ok_or("备份复核序号已用尽，请重新启动。")?;
        let sequence = self.backup.sequence;
        self.backup.selected_name = selected.display_path().map_err(|_| "文件名不可读取。")?;
        self.backup
            .editor
            .as_mut()
            .ok_or("备份页面已失效。")?
            .begin_review(sequence)?;
        let backend = self.backend.clone().ok_or("数据服务尚未准备完成。")?;
        self.submit_backup(
            Job::Preflight(workflow, backend, lease, selected),
            lease.principal_id(),
            sequence,
        )
    }
    pub(super) fn save_backup_to(
        &mut self,
        destination: BackupDestination,
    ) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        if self.current_global_owner() != Some(lease.principal_id())
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
        {
            self.lock();
            return self.render();
        }
        let receipt = self.backup.exported.clone().ok_or("请先导出完整备份。")?;
        let backend = self.backend.clone().ok_or("数据服务尚未准备完成。")?;
        self.submit_backup(
            Job::Copy(backend, lease, receipt, destination),
            lease.principal_id(),
            0,
        )
    }
    pub(super) fn backup_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        if self.backup.irreversible {
            if action == CANCEL_RESTORE {
                if let Some(coordinator) = &self.backup.coordinator {
                    coordinator.cancel();
                }
                self.backup.acknowledged = true;
                self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(30));
                self.status =
                    "已请求安全中止，正在等待实际后台结束；原数据与恢复核对记录会保留。".into();
            } else if action == CLOSE_RESTORE && self.backup.finished {
                self.backup.acknowledged = true;
            }
            return self.render();
        }
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        let owner = lease.principal_id();
        if self.current_global_owner() != Some(owner)
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
            || self
                .backup
                .editor
                .as_ref()
                .is_none_or(|e| e.owner() != owner)
        {
            self.lock();
            return self.render();
        }
        if self.backup.migration.is_some() {
            return self.migration_event(action, data, lease);
        }
        if (self.backup.job.is_some() || self.backup.cleanup.is_some())
            && action != backup_editor::CANCEL
        {
            return Ok(());
        }
        let command = self
            .backup
            .editor
            .as_mut()
            .ok_or("备份页面已失效。")?
            .event(action, data);
        match command {
            Ok(backup_editor::Command::Stay) => {}
            Ok(backup_editor::Command::Cancel) => {
                self.backup.migration_inspection_sequence = None;
                self.backup.review = None;
                if let Some(job) = &self.backup.job {
                    job.ticket.cancel();
                }
                self.status = "已取消本次备份操作；后台仍在结束时请稍候。".into();
            }
            Ok(backup_editor::Command::Close) => {
                self.backup.migration_inspection_sequence = None;
                self.backup.review = None;
                self.backup.editor = None;
                self.screen = Screen::Home;
                self.status.clear();
            }
            Ok(backup_editor::Command::Export) => {
                let workflow = self
                    .backup
                    .workflow
                    .clone()
                    .ok_or("备份服务尚未准备完成。")?;
                let backend = self.backend.clone().ok_or("数据服务尚未准备完成。")?;
                return self.submit_backup(Job::Export(workflow, backend, lease), owner, 0);
            }
            Ok(backup_editor::Command::SelectRestore) => {
                self.backup.review = None;
                match fc_windows::materials::chooser::select_backup(None) {
                    Ok(Some(selected)) => return self.preflight_selected_backup(selected),
                    Ok(None) => self.status = "未选择备份。".into(),
                    Err(_) => self.status = "无法选择备份，请选择本地完整 .fcbak 文件。".into(),
                }
            }
            Ok(backup_editor::Command::SelectMigration) => {
                self.backup.review = None;
                match fc_windows::materials::chooser::select_backup(None) {
                    Ok(Some(selected)) => return self.inspect_selected_migration(selected),
                    Ok(None) => self.status = "未选择映射恢复包。".into(),
                    Err(_) => self.status = "无法选择备份，请选择本地完整 .fcbak 文件。".into(),
                }
            }
            Ok(backup_editor::Command::SaveCopy) => {
                if self.backup.exported.is_none() {
                    return Err("请先导出完整备份。".into());
                }
                match fc_windows::materials::chooser::save_backup(None, "未来教室教学备份.fcbak")
                {
                    Ok(Some(destination)) => {
                        // A modal WTS callback may already have revoked this
                        // lease; only the worker's fresh access grants copying.
                        return self.save_backup_to(destination);
                    }
                    Ok(None) => self.status = "未另存备份，原导出文件仍保留。".into(),
                    Err(_) => self.status = "无法选择保存位置，请选择本地目录。".into(),
                }
            }
            Ok(backup_editor::Command::Confirm { sequence }) => {
                if sequence != self.backup.job_sequence {
                    return Err("此备份复核已失效。".into());
                }
                let Some(review) = self.backup.review.take() else {
                    return Err("请重新选择备份并预检。".into());
                };
                let workflow = self
                    .backup
                    .workflow
                    .clone()
                    .ok_or("备份服务尚未准备完成。")?;
                let backend = self.backend.clone().ok_or("数据服务尚未准备完成。")?;
                return self.submit_backup(
                    Job::Confirm(workflow, backend, lease, review),
                    owner,
                    sequence,
                );
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
    pub(super) fn poll_backup(&mut self) -> std::result::Result<(), String> {
        if self.poll_migration_revision() {
            self.render()?;
        }
        let confirmed = self
            .backup
            .workflow
            .as_ref()
            .and_then(RecoveryWorkflow::confirmed_bundle);
        if !self.backup.irreversible
            && (confirmed.is_some()
                || self
                    .control
                    .as_ref()
                    .is_some_and(SessionControl::is_recovering))
        {
            self.stop_teacher_https();
            self.backup.irreversible = true;
            self.stopping = true;
            self.screen = Screen::Restoring;
            self.lease = None;
            self.desktop.stop();
            self.shell.stop();
            self.clear_profile_preview();
            self.backup.editor = None;
            self.backup.migration = None;
            self.backup.mapping = None;
            self.backup.mapping_target_revision = None;
            self.backup.migration_inspection_sequence = None;
            if let Some(pending) = self.pending.take() {
                pending.ticket.cancel();
            }
            if let Some(job) = self.autostart_job.take() {
                job.ticket.cancel();
            }
            if let Some(job) = self.calendar_job.take() {
                job.ticket.cancel();
            }
            self.content_maintenance = None;
            self.voice_maintenance = None;
            if let Some(audio) = &self.audio {
                audio.shutdown();
            }
            self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(30 * 60));
            self.status = "正在关闭接纳并排空实际任务，请稍候。".into();
            self.render()?;
        }
        let confirmation_failed = self
            .backup
            .workflow
            .as_ref()
            .is_some_and(|w| w.confirmation_state() == ConfirmationState::Failed);
        if self.backup.irreversible
            && self.backup.coordinator.is_none()
            && !self.backup.finished
            && !self.backup.start_failed
            && self.shell.is_finished()
            && self.teacher_https_quiescent()
            && (confirmed.is_some() || confirmation_failed)
        {
            // Validate every remaining owner before moving any out of the App.
            let backend = self.backend.clone();
            if backend.is_none() || self.audio.is_none() {
                self.backup.start_failed = true;
                self.terminate_required = true;
                self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(5));
                self.status =
                    "无法确认恢复关闭所有权，正在结束当前进程；原数据保留供重启核对。".into();
                return self.render();
            }
            let prepared = match PreparedCoordinator::<Work>::prepare() {
                Ok(value) => value,
                Err(_) => {
                    self.backup.start_failed = true;
                    self.terminate_required = true;
                    self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(5));
                    self.status =
                        "无法启动恢复关闭协调器，正在结束当前进程；原数据保留供重启核对。".into();
                    return self.render();
                }
            };
            let pool = self
                .pool
                .take_for_restore()
                .ok_or("恢复执行池所有权缺失。")?;
            let audio = self.audio.take().expect("validated audio owner");
            let backend = backend.expect("validated backend owner");
            self.backup.coordinator = Some(if let Some(confirmed) = confirmed {
                prepared.start(confirmed, backend, pool, audio)
            } else {
                if let Some(workflow) = &self.backup.workflow {
                    workflow.request_close();
                }
                prepared.start_abort_original(backend, pool, audio)
            });
        }
        if let Some(coordinator) = &mut self.backup.coordinator {
            let status = coordinator.status();
            if self.backup.last_phase != Some(status.phase) {
                self.backup.last_phase = Some(status.phase);
                self.status = match status.phase {
                    RestorePhase::Waiting | RestorePhase::Draining => {
                        "正在等待后台任务和声音实际结束。"
                    }
                    RestorePhase::PreparingDatabase => "正在构造恢复数据库，并保留当前安全资料。",
                    RestorePhase::Sealing => "正在核对完整候选与文件摘要。",
                    RestorePhase::Committing => "正在切换教学数据库及完整原件。",
                    RestorePhase::Retained => "实际后台结束尚未确认；保留所有权并等待安全核对。",
                    RestorePhase::Finished => "恢复任务已结束。",
                }
                .into();
                self.render()?;
            }
            let coordinator = self
                .backup
                .coordinator
                .as_mut()
                .expect("coordinator retained");
            if coordinator.reap_nonblocking() && !self.backup.finished {
                let status = coordinator.status();
                if let Some(result) = status.result {
                    self.backup.finished = true;
                    self.shutdown_deadline = None;
                    self.status=if result.is_ok() { "教学资料恢复完成，当前安全资料已保留。请关闭程序后重新启动并登录。" } else { "本次恢复未完成或已中止。原数据与核对记录已保留，请关闭程序并重新启动，先核对恢复状态。" }.into();
                    self.render()?;
                }
            }
        }
        if let Some(ticket) = &mut self.backup.cleanup {
            if !matches!(ticket.try_result(), Ok(None)) {
                self.backup.cleanup = None;
            }
        }
        let completed = self
            .backup
            .job
            .as_mut()
            .and_then(|p| match p.ticket.try_result() {
                Ok(None) => None,
                Ok(Some(value)) => Some(value),
                Err(_) => Some(Err(Failure::Stopped)),
            });
        if let Some(completed) = completed {
            let generation = self.backup.job.take().map(|p| p.generation);
            let current = generation == Some(self.generation)
                && self.backup.job_owner == self.current_global_owner()
                && self.screen == Screen::Backup
                && !self.stopping;
            if current {
                match completed {
                    Ok(Outcome::Backup(Completed::MigrationInspected(bundle))) => {
                        if self.backup.migration_inspection_sequence
                            == Some(self.backup.job_sequence)
                        {
                            self.backup.migration_inspection_sequence = None;
                            self.accept_migration_inspection(bundle)?;
                        }
                    }
                    Ok(Outcome::Backup(Completed::MigrationPreflighted {
                        review,
                        draft_revision,
                    })) => {
                        self.accept_migration_review(review, draft_revision)?;
                    }
                    Ok(Outcome::Backup(Completed::Exported(receipt))) => {
                        self.backup
                            .editor
                            .as_mut()
                            .ok_or("备份页面已失效。")?
                            .exported(receipt.path.to_string_lossy().into_owned(), receipt.bytes)?;
                        self.backup.exported = Some(receipt);
                        self.status = "完整备份已导出，可另存到本地位置。".into();
                    }
                    Ok(Outcome::Backup(Completed::Copied { path, bytes })) => {
                        self.backup
                            .editor
                            .as_mut()
                            .ok_or("备份页面已失效。")?
                            .exported(path, bytes)?;
                        self.status = "备份已完整另存，字节与摘要已核对。".into();
                    }
                    Ok(Outcome::Backup(Completed::Preflighted(review))) => {
                        let s = review.summary();
                        let summary = backup_editor::ReviewSummary {
                            current_revision: s.current_revision,
                            source_revision: s.source_revision,
                            school_id: s.school_id,
                            document_count: s.document_count,
                            material_count: s.material_count as u64,
                            material_bytes: s.material_bytes,
                            profile_media_count: s.profile_media_count as u64,
                            profile_media_bytes: s.profile_media_bytes,
                            database_bytes: s.database_bytes,
                            selected_file: self.backup.selected_name.clone(),
                        };
                        match self
                            .backup
                            .editor
                            .as_mut()
                            .ok_or("备份页面已失效。")?
                            .set_review(
                                self.backup.job_owner.ok_or("备份身份已失效。")?,
                                self.backup.job_sequence,
                                summary,
                            ) {
                            Ok(()) => {
                                self.backup.review = Some(review);
                                self.status = "完整预检已通过，请核对恢复影响并明确确认。".into();
                            }
                            Err(error) => self.status = error,
                        }
                    }
                    Err(error) => {
                        self.backup.migration_inspection_sequence = None;
                        if self.backup.migration.is_some() {
                            self.migration_inspection_lost();
                        }
                        self.backup.review = None;
                        self.backup.editor = self.backup.job_owner.map(backup_editor::Editor::new);
                        if fatal(&error) {
                            if let Some(control) = &self.control {
                                control.fault();
                            }
                            self.begin_shutdown();
                        }
                        self.status = error_text(&error);
                    }
                    _ => {}
                }
                self.render()?;
            }
        }
        if !self.backup.irreversible && self.backup.cleanup.is_none() {
            if let Some(workflow) = &self.backup.workflow {
                if workflow.retirement_pending() {
                    if let Ok(ticket) = self.pool.handle().try_submit(
                        Work::Backup(Box::new(Job::Retire(workflow.clone()))),
                        Instant::now() + Duration::from_secs(30 * 60),
                    ) {
                        self.backup.cleanup = Some(ticket);
                    }
                }
            }
        }
        Ok(())
    }
}
