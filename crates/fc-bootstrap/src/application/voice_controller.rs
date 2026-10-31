//! Local voice policy uses the existing shared workers and exact local lease.
//! A saved switch is not proof that a native output has physically stopped.
use super::*;

pub(super) enum Operation {
    Refresh,
    CheckOutput {
        expected_publication: Revision,
        expected: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
    },
    SetEnabled {
        expected: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
        enabled: bool,
    },
    SetLimits {
        expected: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
        limits: fc_domain::voice::VoiceLimits,
    },
}

pub(super) fn run(
    service: VoiceService,
    lease: SessionLease,
    operation: Operation,
    context: &JobContext,
) -> Result<Outcome> {
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    // Even refresh is a privileged read. Form data supplies neither the lease
    // nor a captured revision, and no automatic retry replays a change.
    service.management_snapshot(lease)?;
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    match operation {
        Operation::Refresh => {}
        Operation::CheckOutput {
            expected_publication,
            expected,
            expected_identity,
            expected_runtime,
        } => {
            let loaded = service.check_default_output_reviewed(
                lease,
                expected_publication,
                expected,
                expected_identity,
                expected_runtime,
                context,
                fc_windows::audio::query_default_output_format,
            )?;
            context.checkpoint().map_err(|_| Failure::Stopped)?;
            return Ok(Outcome::VoiceLoaded(lease, Box::new(loaded)));
        }
        Operation::SetEnabled {
            expected,
            expected_identity,
            expected_runtime,
            enabled,
        } => {
            service.set_enabled_reviewed(
                lease,
                enabled,
                expected,
                expected_identity,
                expected_runtime,
            )?;
        }
        Operation::SetLimits {
            expected,
            expected_identity,
            expected_runtime,
            limits,
        } => {
            service.set_limits_reviewed(
                lease,
                limits,
                expected,
                expected_identity,
                expected_runtime,
            )?;
        }
    }
    let loaded = service.management_snapshot(lease)?;
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    Ok(Outcome::VoiceLoaded(lease, Box::new(loaded)))
}

impl App {
    fn voice_management_allowed(&self, lease: SessionLease) -> bool {
        lease.origin() == TrustedOrigin::Local
            && self.current_global_owner() == Some(lease.principal_id())
            && self.profiles_memory_gate()
            && self
                .state
                .account(lease.principal_id())
                .is_ok_and(|account| !account.must_change_password && account.principal.enabled)
    }

    pub(super) fn submit_voice_management(
        &mut self,
        operation: Operation,
    ) -> std::result::Result<(), String> {
        if self.pending.is_some() || self.stopping {
            return Ok(());
        }
        let Some(lease) = self
            .lease
            .filter(|lease| self.voice_management_allowed(*lease))
        else {
            self.lock();
            return self.render();
        };
        let Some(service) = self.voice_service.clone() else {
            self.status = "语音服务尚未就绪，请稍后重新打开。".into();
            return self.render();
        };
        self.screen = Screen::Voice;
        self.submit(Work::Voice(service, lease, operation))
    }

    pub(super) fn voice_management_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        let Some(lease) = self
            .lease
            .filter(|lease| self.voice_management_allowed(*lease))
        else {
            self.lock();
            return self.render();
        };
        let Some(editor) = self.voice_editor.as_mut() else {
            if !data.is_empty() {
                self.status = "请重新读取远程语音设置。".into();
                return self.render();
            }
            return match action {
                voice_editor::REFRESH => self.submit_voice_management(Operation::Refresh),
                voice_editor::CLOSE => {
                    self.screen = Screen::Home;
                    self.status.clear();
                    self.render()
                }
                _ => {
                    self.status = "请重新读取远程语音设置，或返回工作台。".into();
                    self.render()
                }
            };
        };
        if editor.owner() != lease.principal_id() {
            self.lock();
            return self.render();
        }
        match editor.handle(action, data) {
            Ok(voice_editor::Command::Stay) => self.status.clear(),
            Ok(voice_editor::Command::Close) => {
                self.voice_editor = None;
                self.screen = Screen::Home;
                self.status.clear();
            }
            Ok(voice_editor::Command::Refresh) => {
                return self.submit_voice_management(Operation::Refresh);
            }
            Ok(voice_editor::Command::CheckOutput {
                expected_publication,
                expected,
                expected_identity,
                expected_runtime,
            }) => {
                return self.submit_voice_management(Operation::CheckOutput {
                    expected_publication,
                    expected,
                    expected_identity,
                    expected_runtime,
                });
            }
            Ok(voice_editor::Command::SetEnabled {
                expected,
                expected_identity,
                expected_runtime,
                enabled,
            }) => {
                return self.submit_voice_management(Operation::SetEnabled {
                    expected,
                    expected_identity,
                    expected_runtime,
                    enabled,
                });
            }
            Ok(voice_editor::Command::SetLimits {
                expected,
                expected_identity,
                expected_runtime,
                limits,
            }) => {
                return self.submit_voice_management(Operation::SetLimits {
                    expected,
                    expected_identity,
                    expected_runtime,
                    limits,
                });
            }
            Err(message) => self.status = message,
        }
        self.render()
    }

    pub(super) fn voice_management_loaded(
        &mut self,
        lease: SessionLease,
        mut loaded: fc_bootstrap::backend::VoiceManagementSnapshot,
    ) -> std::result::Result<(), String> {
        if self.screen != Screen::Voice
            || self.lease != Some(lease)
            || !self.voice_management_allowed(lease)
        {
            return Ok(());
        }
        // Completion delivery can lag another authority publication or a stop.
        // This is a memory-only freshness check, never UI-side SQLite or KDF.
        let fresh = self.control.as_ref().is_some_and(|control| {
            let status = control.health_gate().status();
            status.health == fc_runtime::health::Health::Healthy
                && status.revision == loaded.publication_revision
        }) && self
            .voice_service
            .as_ref()
            .is_some_and(|service| service.generation() == loaded.runtime_generation);
        if self.offscreen_probe {
            let status = self
                .control
                .as_ref()
                .map(|control| control.health_gate().status());
            let runtime = self.voice_service.as_ref().map(VoiceService::generation);
            println!(
                "VOICE_SETTINGS_FRESH fresh={fresh} loaded_publication={} current_publication={:?} health={:?} loaded_runtime={} current_runtime={runtime:?} loaded_enabled={}",
                loaded.publication_revision.get(),
                status.as_ref().map(|status| status.revision.get()),
                status.as_ref().map(|status| status.health),
                loaded.runtime_generation,
                loaded.enabled,
            );
        }
        if !fresh {
            if let Some(editor) = self.voice_editor.as_mut() {
                editor.invalidate();
            }
            self.status = "语音状态已更新，请重新读取后核对。".into();
            return Ok(());
        }
        let had_check = loaded.device_observation.is_some();
        let check_expired = had_check && loaded.current_device_observation(lease).is_none();
        if check_expired {
            // Receipt delivery itself may arrive after the original 30 seconds.
            // Never start a new lifetime or substitute an older successful one.
            loaded.device_observation = None;
        } else if !had_check {
            loaded.device_observation = self
                .voice_editor
                .as_ref()
                .and_then(|editor| editor.retained_device_observation(&loaded, lease));
        }
        if let Some(editor) = self.voice_editor.as_mut() {
            if editor.owner() != lease.principal_id() {
                self.lock();
                return Ok(());
            }
            editor.loaded(loaded);
        } else {
            self.voice_editor = Some(voice_editor::Editor::new(lease.principal_id(), loaded));
        }
        self.status = if check_expired {
            "检查结果已过期，请重新检查默认输出。".into()
        } else if had_check {
            "已完成默认输出格式检查；实际是否有声须另行确认。".into()
        } else {
            "已重新读取已保存的语音开关和任务状态；设备释放与实际听到须另行确认。".into()
        };
        Ok(())
    }

    /// Use the existing pump only for local memory invalidation and repaint.
    /// No query, worker submission, SQLite read or additional timer occurs here.
    pub(super) fn poll_voice_management(&mut self) -> std::result::Result<(), String> {
        if self.screen != Screen::Voice || self.stopping {
            return Ok(());
        }
        let Some(lease) = self
            .lease
            .filter(|lease| self.voice_management_allowed(*lease))
        else {
            self.lock();
            return self.render();
        };
        let Some(editor) = self.voice_editor.as_ref() else {
            return Ok(());
        };
        if editor.owner() != lease.principal_id() {
            self.lock();
            return self.render();
        }
        let (publication, runtime) = editor.loaded_versions();
        let current = self.control.as_ref().is_some_and(|control| {
            let status = control.health_gate().status();
            status.health == fc_runtime::health::Health::Healthy && status.revision == publication
        }) && self
            .voice_service
            .as_ref()
            .is_some_and(|service| service.generation() == runtime);
        let now = Instant::now();
        if !editor.needs_tick(now, current) {
            return Ok(());
        }
        // EN_CHANGE already captures normal editing. Also preserve any native
        // edit awaiting its Input event immediately before an expiry repaint.
        let capture = self.pending.is_none() && !editor.is_reviewing();
        let draft = capture.then(|| self.window.draft_nonsecret());
        if let Some(editor) = self.voice_editor.as_mut() {
            if let Some(Ok(draft)) = draft {
                let _ = editor.capture(draft);
            }
            editor.tick(now, current);
        }
        self.render()
    }
}
