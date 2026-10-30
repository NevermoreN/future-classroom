use super::*;
use fc_bootstrap::autostart::{Job, Operation};

impl App {
    pub(super) fn submit_autostart(
        &mut self,
        operation: Operation,
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
        if self.autostart_job.is_some() {
            self.status = "上一次启动项任务仍在核对，完成后才能继续。".into();
            return self.render();
        }
        let Some(backend) = self.backend.clone() else {
            return Ok(());
        };
        let job = Job {
            backend,
            lease: Some(lease),
            operation,
        };
        let work = Work::Autostart(Box::new(job), self.autostart_scope.clone());
        match self
            .pool
            .handle()
            .try_submit(work, Instant::now() + Duration::from_secs(40))
        {
            Ok(ticket) => {
                self.autostart_job = Some(Pending {
                    ticket,
                    generation: self.generation,
                    self_revokes: false,
                });
                self.status = "正在核对启动项；锁定可取消后续操作。".into();
            }
            Err(_) => self.status = "后台繁忙，请稍后重新读取。".into(),
        }
        self.render()
    }
    pub(super) fn autostart_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
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
        let Some(editor) = &mut self.autostart else {
            self.screen = Screen::Home;
            return self.render();
        };
        if editor.owner() != owner {
            self.lock();
            return self.render();
        }
        match editor.event(action, data) {
            Ok(autostart_editor::Command::Stay) => self.status.clear(),
            Ok(autostart_editor::Command::Close) => {
                self.autostart = None;
                self.screen = Screen::Home;
                self.status.clear();
            }
            Ok(autostart_editor::Command::Load) => return self.submit_autostart(Operation::Load),
            Ok(autostart_editor::Command::Change {
                expected,
                desired,
                previous_observed,
                reviewed_target,
            }) => {
                return self.submit_autostart(Operation::Change {
                    expected,
                    desired,
                    previous_observed,
                    reviewed_target,
                })
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
    pub(super) fn poll_autostart(&mut self) -> std::result::Result<(), String> {
        let completed = self
            .autostart_job
            .as_mut()
            .and_then(|job| match job.ticket.try_result() {
                Ok(None) => None,
                Ok(Some(result)) => Some(result),
                Err(_) => Some(Err(Failure::Stopped)),
            });
        if let Some(result) = completed {
            let generation = self.autostart_job.take().map(|job| job.generation);
            match result {
                Err(error) if fatal(&error) => {
                    if let Some(control) = &self.control {
                        control.fault();
                    }
                    self.begin_shutdown();
                }
                Ok(Outcome::Autostart(owner, loaded))
                    if generation == Some(self.generation)
                        && !self.stopping
                        && self.current_global_owner() == owner
                        && owner.is_some() =>
                {
                    self.status = loaded
                        .state
                        .as_ref()
                        .and_then(fc_domain::autostart::AutostartState::current)
                        .map_or("已重新读取实际启动状态。".into(), |op| {
                            format!("最近结果：{}。", autostart_editor::phase(op.phase))
                        });
                    self.autostart = Some(autostart_editor::Editor::new(
                        owner.expect("checked owner"),
                        *loaded,
                    ));
                    self.screen = Screen::Autostart;
                    self.render()?;
                }
                Err(error) if generation == Some(self.generation) && !self.stopping => {
                    if let Some(editor) = &mut self.autostart {
                        editor.invalidate();
                        self.screen = Screen::Autostart;
                    } else {
                        self.screen = Screen::Home;
                    }
                    self.status = error_text(&error);
                    self.render()?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}
