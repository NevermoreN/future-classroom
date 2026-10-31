//! UI routing and one physical calendar task retained through native completion.
use super::*;
use calendar_flow::{Job, Operation};

impl App {
    pub(super) fn open_calendar(&mut self) -> std::result::Result<(), String> {
        if self
            .schedule
            .as_ref()
            .is_some_and(|editor| editor.has_unsaved_changes())
        {
            self.status = "请先保存或取消课表草稿，再打开公共校历候选。".into();
            return self.render();
        }
        let year = self
            .clock
            .try_now()
            .map_err(|_| "当前日期不可用")?
            .date
            .year();
        self.submit_calendar(Operation::Load(year))
    }
    fn submit_calendar(&mut self, operation: Operation) -> std::result::Result<(), String> {
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
        let Some(backend) = self.backend.clone() else {
            return Ok(());
        };
        let job = Job {
            backend,
            lease: Some(lease),
            operation,
        };
        if !job.is_download() {
            return self.submit(Work::Calendar(Box::new(job)));
        }
        if self.calendar_job.is_some() {
            self.status = "上一次校历任务仍在处理，完成后才允许再次下载。".into();
            return self.render();
        }
        match self.pool.handle().try_submit(
            Work::Calendar(Box::new(job)),
            Instant::now() + Duration::from_secs(40),
        ) {
            Ok(ticket) => {
                self.calendar_job = Some(Pending {
                    ticket,
                    generation: self.generation,
                    self_revokes: false,
                    public_lesson: false,
                });
                self.status = "正在下载公共校历候选；可以锁定工作台取消本次请求。".into();
            }
            Err(_) => self.status = "后台繁忙，请稍后重试。".into(),
        }
        self.render()
    }
    pub(super) fn calendar_event(
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
        if self.calendar_job.is_some() {
            return Ok(());
        }
        let Some(editor) = &mut self.calendar else {
            self.screen = Screen::Home;
            return self.render();
        };
        if editor.owner() != owner {
            self.lock();
            return self.render();
        }
        // Read the currently displayed source year from the immutable editor,
        // never a forged confirmation form. Its captured candidate CAS stays in
        // the command and is repeated by the sole writer.
        let year = editor.selected_year();
        match editor.event(action, data) {
            Ok(calendar_editor::Command::Stay) => self.status.clear(),
            Ok(calendar_editor::Command::Close) => {
                self.calendar = None;
                if let (Some(backend), Some(lease)) = (&self.backend, self.lease) {
                    return self.submit(Work::LoadSchedule(backend.clone(), lease));
                }
                self.screen = Screen::Home;
            }
            Ok(calendar_editor::Command::Load(year)) => {
                return self.submit_calendar(Operation::Load(year))
            }
            Ok(calendar_editor::Command::Download(year)) => {
                return self.submit_calendar(Operation::Download(year))
            }
            Ok(calendar_editor::Command::Adopt {
                expected,
                candidates,
            }) => {
                return self.submit_calendar(Operation::Adopt {
                    year,
                    expected,
                    candidates,
                })
            }
            Ok(calendar_editor::Command::Policy { expected, enabled }) => {
                return self.submit_calendar(Operation::Policy {
                    year,
                    expected,
                    enabled,
                })
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
    pub(super) fn calendar_loaded(&mut self, loaded: calendar_flow::Loaded) {
        if self.current_global_owner() != Some(loaded.owner) {
            return;
        }
        // Entry required a clean schedule draft. Another foreground edit cannot
        // run during this command; later stale drafts are protected by writer CAS.
        self.schedule = Some(ScheduleEditor::new(loaded.document.clone(), loaded.owner));
        if let Some(editor) = self
            .calendar
            .as_mut()
            .filter(|editor| editor.owner() == loaded.owner)
        {
            editor.replace(loaded.document, loaded.status);
        } else {
            self.calendar = Some(calendar_editor::Editor::new(
                loaded.owner,
                loaded.document,
                loaded.status,
            ));
        }
        self.screen = Screen::Calendar;
        self.status = loaded.message;
    }
    pub(super) fn poll_calendar(&mut self) -> std::result::Result<(), String> {
        let completed = self
            .calendar_job
            .as_mut()
            .and_then(|job| match job.ticket.try_result() {
                Ok(None) => None,
                Ok(Some(result)) => Some(result),
                Err(_) => Some(Err(Failure::Stopped)),
            });
        if let Some(result) = completed {
            let generation = self.calendar_job.take().map(|job| job.generation);
            self.next_calendar_maintenance = Instant::now() + Duration::from_secs(60);
            match result {
                Err(error) if fatal(&error) => {
                    if let Some(control) = &self.control {
                        control.fault();
                    }
                    self.begin_shutdown();
                }
                Ok(Outcome::Calendar(loaded))
                    if generation == Some(self.generation) && !self.stopping =>
                {
                    self.calendar_loaded(*loaded);
                    self.render()?;
                }
                Ok(Outcome::CalendarMaintained { attempted: true }) if !self.stopping => {
                    if self.screen == Screen::Calendar && self.pending.is_none() {
                        if let Some(editor) = &self.calendar {
                            if !editor.has_unsaved_changes() {
                                let year = editor.selected_year();
                                self.submit_calendar(Operation::Load(year))?;
                            } else {
                                self.status = "后台校历检查已结束；当前复核保留原版本，请取消并重新读取后确认。".into();
                                self.render()?;
                            }
                        }
                    } else if self.screen == Screen::Schedule {
                        self.status =
                            "后台校历检查已结束；课表草稿仍保留，保存前请核对最新版本。".into();
                        self.render()?;
                    }
                }
                Err(error)
                    if generation == Some(self.generation)
                        && self.screen == Screen::Calendar
                        && !self.stopping =>
                {
                    self.status = error_text(&error);
                    self.render()?;
                }
                _ => {}
            }
        }
        if !self.stopping
            && !self.offscreen_probe
            && self.pending.is_none()
            && self.calendar_job.is_none()
            && self.state.installation.is_some()
            && Instant::now() >= self.next_calendar_maintenance
        {
            self.next_calendar_maintenance = Instant::now() + Duration::from_secs(60);
            if let Some(backend) = &self.backend {
                // The persisted policy is default-off. The background writer
                // checks mode, due years and its real grant before reserving.
                if let Ok(ticket) = self.pool.handle().try_submit(
                    Work::Calendar(Box::new(Job {
                        backend: backend.clone(),
                        lease: None,
                        operation: Operation::Background,
                    })),
                    Instant::now() + Duration::from_secs(40),
                ) {
                    self.calendar_job = Some(Pending {
                        ticket,
                        generation: self.generation,
                        self_revokes: false,
                        public_lesson: false,
                    });
                }
            }
        }
        Ok(())
    }
}
