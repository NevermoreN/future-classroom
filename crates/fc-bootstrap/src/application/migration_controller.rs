//! Native mapping events carry choices only; physical source and mapped owners
//! remain in the existing recovery slot until a worker retires them.
use super::*;
use backup_controller::Job;
use fc_bootstrap::recovery_workflow::{MigrationBundle, ReviewBundle};

impl App {
    pub(super) fn inspect_selected_migration(
        &mut self,
        selected: fc_windows::materials::chooser::SelectedFile,
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
        if self.backup.job.is_some()
            || self.backup.cleanup.is_some()
            || !workflow.is_idle() && !workflow.retirement_pending()
        {
            self.status = "上一份备份仍在结束，请稍后重新选择。".into();
            return self.render();
        }
        self.backup.sequence = self
            .backup
            .sequence
            .checked_add(1)
            .ok_or("复核序号已用尽，请重新启动。")?;
        self.backup.review = None;
        self.backup.mapping = None;
        self.backup.migration = None;
        self.backup.mapping_target_revision = None;
        self.backup.migration_inspection_sequence = Some(self.backup.sequence);
        self.backup.selected_name = selected.display_path().map_err(|_| "文件位置不可读取。")?;
        let backend = self.backend.clone().ok_or("数据服务尚未准备完成。")?;
        self.submit_backup(
            Job::InspectMigration(workflow, backend, lease, selected),
            lease.principal_id(),
            self.backup.sequence,
        )?;
        if self.backup.job.is_none() {
            self.backup.migration_inspection_sequence = None;
        }
        Ok(())
    }

    pub(super) fn accept_migration_inspection(
        &mut self,
        bundle: MigrationBundle,
    ) -> std::result::Result<(), String> {
        let owner = self.backup.job_owner.ok_or("映射身份已失效。")?;
        let d = bundle.details();
        let editor = match migration_editor::Editor::new(
            owner,
            self.backup.job_sequence,
            migration_editor::Inspection {
                source_catalog: d.source_catalog.clone(),
                source_installation: d.source_installation,
                source_revision: d.source_revision,
                target_identity: d.target_identity.clone(),
                target_installation: d.target_installation,
                target_global_revision: d.target_global_revision,
                selected_file: self.backup.selected_name.clone(),
            },
        ) {
            Ok(editor) => editor,
            Err(message) => {
                self.status = message;
                return Ok(());
            }
        };
        self.backup.mapping_target_revision = Some(d.target_global_revision);
        self.backup.mapping_draft_revision = 0;
        self.backup.migration = Some(editor);
        self.backup.mapping = Some(bundle);
        self.status = "目录检查已完成；请选择全部目标后运行完整预检。".into();
        Ok(())
    }

    pub(super) fn accept_migration_review(
        &mut self,
        review: ReviewBundle,
        draft_revision: u64,
    ) -> std::result::Result<(), String> {
        let Some(editor) = &mut self.backup.migration else {
            return Ok(());
        };
        let s = review.summary();
        let result = editor.set_review(
            self.backup.job_owner.ok_or("映射身份已失效。")?,
            self.backup.job_sequence,
            draft_revision,
            migration_editor::ReviewSummary {
                source_revision: s.source_revision,
                current_revision: s.current_revision,
                document_count: s.document_count,
                material_count: s.material_count as u64,
                material_bytes: s.material_bytes,
                profile_media_count: s.profile_media_count as u64,
                profile_media_bytes: s.profile_media_bytes,
                database_bytes: s.database_bytes,
            },
        );
        match result {
            Ok(()) => {
                self.backup.review = Some(review);
                self.status = "映射与完整预检已通过，请核对全量替换和当前安全边界。".into();
            }
            Err(message) => self.status = message,
        }
        Ok(())
    }

    pub(super) fn migration_inspection_lost(&mut self) {
        self.backup.mapping = None;
        self.backup.review = None;
        self.backup.mapping_target_revision = None;
        self.backup.migration_inspection_sequence = None;
        if let (Some(editor), Some(owner)) = (&mut self.backup.migration, self.backup.job_owner) {
            let _ = editor.inspection_lost(owner, self.backup.job_sequence);
        }
    }

    pub(super) fn poll_migration_revision(&mut self) -> bool {
        if self.backup.irreversible {
            return false;
        }
        let Some(expected) = self.backup.mapping_target_revision else {
            return false;
        };
        // A slow worker may own Backend. The window never waits for that lock;
        // every writer operation also repeats the mandatory exact revision check.
        let revision = self
            .backend
            .as_ref()
            .and_then(|b| b.try_lock().ok())
            .and_then(|b| b.published_revision().ok());
        if revision.is_some_and(|r| r != expected) {
            self.migration_inspection_lost();
            if let Some(job) = &self.backup.job {
                job.ticket.cancel();
            }
            self.status = "目标资料或账号目录已变化，请退出并重新选择备份与映射。".into();
            return true;
        }
        false
    }

    pub(super) fn migration_event(
        &mut self,
        action: ActionId,
        data: FormData,
        lease: SessionLease,
    ) -> std::result::Result<(), String> {
        let owner = lease.principal_id();
        if self
            .backup
            .migration
            .as_ref()
            .is_none_or(|e| e.owner() != owner || e.sequence() != self.backup.job_sequence)
        {
            self.lock();
            return self.render();
        }
        if (self.backup.job.is_some() || self.backup.cleanup.is_some())
            && action != migration_editor::CANCEL
        {
            return Ok(());
        }
        let command = self
            .backup
            .migration
            .as_mut()
            .ok_or("映射页面已失效。")?
            .event(action, data);
        match command {
            Ok(migration_editor::Command::Stay) => {}
            Ok(
                migration_editor::Command::Cancel
                | migration_editor::Command::Close
                | migration_editor::Command::ManageTargets,
            ) => {
                self.backup.mapping = None;
                self.backup.review = None;
                self.backup.migration = None;
                self.backup.mapping_target_revision = None;
                self.backup.editor = Some(backup_editor::Editor::new(owner));
                if let Some(job) = &self.backup.job {
                    job.ticket.cancel();
                }
                if action == migration_editor::CANCEL {
                    self.status = "已取消映射；后台结束后可重新选择，正式资料保持当前状态。".into();
                } else {
                    self.screen = Screen::Home;
                    self.status = if action == migration_editor::MANAGE_TARGETS {
                        "请在班级、科目和账号管理中创建目标并发放新的初始口令/配置任教，再重新选择备份。"
                    } else { "已退出映射，正式资料保持当前状态。" }.into();
                }
            }
            Ok(migration_editor::Command::Preflight {
                sequence,
                draft_revision,
                mapping,
            }) => {
                if sequence != self.backup.job_sequence {
                    return Err("映射来源已失效。".into());
                }
                let bundle = self
                    .backup
                    .mapping
                    .take()
                    .ok_or("请重新选择备份并检查来源目录。")?;
                let workflow = self
                    .backup
                    .workflow
                    .clone()
                    .ok_or("备份服务尚未准备完成。")?;
                let backend = self.backend.clone().ok_or("数据服务尚未准备完成。")?;
                self.backup.mapping_draft_revision = draft_revision;
                self.submit_backup(
                    Job::MapMigration(workflow, backend, lease, bundle, mapping, draft_revision),
                    owner,
                    sequence,
                )?;
                if self.backup.job.is_none() {
                    self.migration_inspection_lost();
                    self.render()?;
                }
                return Ok(());
            }
            Ok(migration_editor::Command::Confirm {
                sequence,
                draft_revision,
            }) => {
                if sequence != self.backup.job_sequence
                    || draft_revision != self.backup.mapping_draft_revision
                {
                    return Err("映射复核已失效，请重新选择。".into());
                }
                let review = self.backup.review.take().ok_or("请重新预检完整映射。")?;
                let workflow = self
                    .backup
                    .workflow
                    .clone()
                    .ok_or("备份服务尚未准备完成。")?;
                let backend = self.backend.clone().ok_or("数据服务尚未准备完成。")?;
                self.submit_backup(
                    Job::Confirm(workflow, backend, lease, review),
                    owner,
                    sequence,
                )?;
                if self.backup.job.is_none() {
                    self.migration_inspection_lost();
                    self.render()?;
                }
                return Ok(());
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
}
