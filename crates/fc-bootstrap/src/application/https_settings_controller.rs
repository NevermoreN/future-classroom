//! All HTTPS reads, saves and original-outcome queries use the existing workers.
//! No native or SQL owner is returned to the window pump.
use super::*;
use fc_application::identity::SessionPurpose;
use fc_ports::{HttpsSettingsRequest, HttpsSettingsState};
use https_settings_editor as editor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Load,
    Save,
    Query,
    Check,
}
#[derive(Clone, PartialEq, Eq)]
pub(super) enum Operation {
    Load,
    Save(HttpsSettingsRequest),
    Query(HttpsSettingsRequest),
    Check(HttpsSettingsState),
}
impl Operation {
    fn kind(&self) -> Kind {
        match self {
            Self::Load => Kind::Load,
            Self::Save(_) => Kind::Save,
            Self::Query(_) => Kind::Query,
            Self::Check(_) => Kind::Check,
        }
    }
    pub(super) fn allocation_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>()
            .checked_add(match self {
                Self::Save(r) | Self::Query(r) => {
                    r.configuration.as_ref().map_or(0, |c| c.allocation_bytes())
                }
                Self::Check(s) => s
                    .settings
                    .configuration
                    .as_ref()
                    .map_or(0, |c| c.allocation_bytes()),
                Self::Load => 0,
            })?
            .checked_add(1024)
    }
}
pub(super) struct Pending {
    kind: Kind,
    lease: SessionLease,
}
pub(super) struct Completion {
    lease: SessionLease,
    publication: Revision,
    value: Value,
}
enum Value {
    Loaded(HttpsSettingsState),
    Saved(HttpsSettingsState),
    Queried(Option<HttpsSettingsState>),
    Checked(Box<fc_bootstrap::backend::HttpsCertificateObservation>),
}
pub(super) fn run(
    backend: Arc<Mutex<Backend>>,
    lease: SessionLease,
    operation: Operation,
    context: &JobContext,
) -> Result<Outcome> {
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    let operation = match operation {
        Operation::Check(expected) => {
            let service = {
                let mut owner = backend.lock().map_err(|_| Failure::Stopped)?;
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                let scoped = owner.begin_execution_scope(Arc::new(context.execution_fence()));
                scoped.https_certificate_service()?
            };
            // The Backend guard AND scope are both gone. No KDF/ledger/writer
            // mutex is carried into this actual synchronous native query.
            let observation = service
                .with_execution_fence(Arc::new(context.execution_fence()))
                .inspect_certificate(lease, expected)
                .map_err(|error| match error {
                    fc_bootstrap::backend::HttpsCertificateError::Backend(error) => {
                        Failure::Backend(error)
                    }
                    _ => invalid("证书只读检查未完成，旧结果已失效；可核对配置后重新检查。"),
                })?;
            context.checkpoint().map_err(|_| Failure::Stopped)?;
            return Ok(Outcome::Https(Box::new(Completion {
                lease,
                publication: observation.publication_revision,
                value: Value::Checked(Box::new(observation)),
            })));
        }
        other => other,
    };
    let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    let control = backend.session_control();
    let mut scoped = backend.begin_execution_scope(Arc::new(context.execution_fence()));
    let value = match operation {
        Operation::Load => Value::Loaded(scoped.read_https_settings(lease)?),
        Operation::Save(request) => Value::Saved(scoped.save_https_settings(lease, request)?),
        Operation::Check(_) => return Err(invalid("检查请求已变化。")),
        Operation::Query(request) => {
            Value::Queried(scoped.query_https_settings_outcome(lease, request)?)
        }
    };
    // Capture the actual publication AFTER a possible own COMMIT. The identity
    // revision in a historical receipt is deliberately not a publication id.
    let status = control
        .health_gate()
        .check_sensitive()
        .map_err(|_| Failure::Stopped)?;
    scoped.check_session(lease, SessionPurpose::Manage)?;
    if scoped.role(lease)? != Role::GlobalAdmin {
        return Err(invalid("配置管理权限已变化。"));
    }
    // A new publication cannot be adopted between checking the returned
    // metadata and minting its UI handoff. Historical query results deliberately
    // retain their original identity revision and are never used as current CAS.
    if let Value::Loaded(state) | Value::Saved(state) = &value {
        if scoped.state()?.revision != state.identity_revision {
            return Err(Failure::Stopped);
        }
    }
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    if control.health_gate().status() != status {
        return Err(Failure::Stopped);
    }
    Ok(Outcome::Https(Box::new(Completion {
        lease,
        publication: status.revision,
        value,
    })))
}

impl App {
    fn https_owner(&self) -> Option<editor::Owner> {
        let lease = self.lease?;
        if !self.https_allowed(lease) {
            return None;
        }
        Some(editor::Owner {
            school: self.state.installation.as_ref()?.school_id,
            principal: lease.principal_id(),
        })
    }
    pub(super) fn https_allowed(&self, lease: SessionLease) -> bool {
        lease.origin() == TrustedOrigin::Local
            && self.current_global_owner() == Some(lease.principal_id())
            && self.profiles_memory_gate()
            && self
                .state
                .account(lease.principal_id())
                .is_ok_and(|a| a.principal.enabled && !a.must_change_password)
    }
    pub(super) fn open_https_settings(&mut self) -> std::result::Result<(), String> {
        if self.pending.is_some() || self.stopping {
            return Ok(());
        }
        let Some(owner) = self.https_owner() else {
            self.lock();
            return self.render();
        };
        self.leave_web();
        self.screen = Screen::HttpsSettings;
        self.status.clear();
        if self.https_editor.is_none() {
            self.https_editor = Some(editor::Editor::new(owner));
            self.submit_https(Operation::Load)?;
        } else {
            // Same-owner draft/capsule is retained; another owner sees only the
            // explicit discard warning, never the previous form or receipt.
            self.render()?;
        }
        Ok(())
    }
    pub(super) fn https_view(&self, status: &str, busy: bool) -> Option<View> {
        let owner = self.https_owner()?;
        self.https_editor
            .as_ref()
            .map(|e| e.view(owner, status, busy))
    }
    pub(super) fn https_capture(&mut self, data: FormData) {
        let owner = self.https_owner();
        if let Some(e) = &mut self.https_editor {
            if owner.is_some_and(|o| e.owns(o)) {
                if let Err(message) = e.capture(data) {
                    self.status = message;
                }
            }
        }
    }
    pub(super) fn suspend_https(&mut self) {
        self.https_unadmitted = None;
        if self.screen == Screen::HttpsSettings {
            if self.pending.is_none() {
                if let (Ok(data), Some(lease), Some(installation)) = (
                    self.window.draft_nonsecret(),
                    self.lease,
                    self.state.installation.as_ref(),
                ) {
                    let owner = editor::Owner {
                        school: installation.school_id,
                        principal: lease.principal_id(),
                    };
                    // Negative-only retention while locking must not depend on
                    // the authority still being healthy. It grants no action.
                    if let Some(e) = &mut self.https_editor {
                        if e.owns(owner) {
                            let _ = e.capture(data);
                        }
                    }
                }
            }
            if let Some(e) = &mut self.https_editor {
                e.suspended();
            }
        }
    }
    fn submit_https(&mut self, operation: Operation) -> std::result::Result<(), String> {
        if self.pending.is_some() || self.stopping {
            return Ok(());
        }
        let Some(lease) = self.lease.filter(|l| self.https_allowed(*l)) else {
            self.lock();
            return self.render();
        };
        let Some(backend) = self.backend.clone() else {
            return Ok(());
        };
        let kind = operation.kind();
        // Save retry after an UNADMITTED Busy rejection retains the first
        // absolute deadline. No accepted write is ever submitted a second time.
        let deadline = if kind == Kind::Save {
            self.https_editor
                .as_ref()
                .and_then(editor::Editor::save_deadline)
                .ok_or("原保存记录不可用，请重新读取。")?
        } else {
            self.https_unadmitted
                .as_ref()
                .and_then(|(work, deadline)| match work {
                    Work::Https(prior_backend, prior_lease, prior)
                        if *prior_lease == lease
                            && *prior == operation
                            && Arc::ptr_eq(prior_backend, &backend) =>
                    {
                        Some(*deadline)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| Instant::now() + Duration::from_secs(90))
        };
        let work = match self.https_unadmitted.take() {
            Some((Work::Https(prior_backend, prior_lease, prior), prior_deadline))
                if prior_lease == lease
                    && prior == operation
                    && prior_deadline == deadline
                    && Arc::ptr_eq(&prior_backend, &backend) =>
            {
                Work::Https(prior_backend, prior_lease, prior)
            }
            _ => Work::Https(backend, lease, operation),
        };
        match self.pool.handle().try_submit(work, deadline) {
            Ok(ticket) => {
                if kind == Kind::Save {
                    if let Some(e) = &mut self.https_editor {
                        e.accepted();
                    }
                }
                self.https_pending = Some(Pending { kind, lease });
                self.pending = Some(super::Pending {
                    ticket,
                    generation: self.generation,
                    self_revokes: false,
                    public_lesson: false,
                });
                self.status = match kind {
                    Kind::Save => "正在保存配置；取消等待不等于撤销保存。",
                    Kind::Query => "正在只读查询原操作结果。",
                    Kind::Load => "正在读取当前配置。",
                    Kind::Check => "正在只读检查已保存的证书；这不会启用教师连接。",
                }
                .into();
            }
            Err(rejected) => {
                // The rejected Work has never run. Keep the original request in
                // the editor; dropping this metadata-only task releases no IO.
                let rejection = rejected.kind;
                let returned_ok = matches!(&rejected.task, Work::Https(_, returned_lease, returned) if *returned_lease == lease && returned.kind() == kind);
                if !returned_ok {
                    return Err("未提交的配置任务不匹配，请重新打开。".into());
                }
                if rejection == fc_runtime::worker::Rejection::Busy {
                    self.https_unadmitted = Some((rejected.task, deadline));
                }
                self.status = match rejection {
                    fc_runtime::worker::Rejection::Busy => {
                        "后台繁忙，本次操作尚未提交。原保存记录和处理期限已保留。"
                    }
                    fc_runtime::worker::Rejection::Deadline => {
                        "本次操作已过处理期限，尚未提交。请明确重新读取后再确认。"
                    }
                    fc_runtime::worker::Rejection::Stopped => "后台正在停止，本次操作尚未提交。",
                    fc_runtime::worker::Rejection::Faulted => "后台状态异常，本次操作尚未提交。",
                    fc_runtime::worker::Rejection::TooLarge => {
                        "配置任务超过容量，本次操作尚未提交。"
                    }
                }
                .into();
            }
        }
        self.render()
    }
    /// Finite diagnostic admission helper. Only a returned Busy task is kept;
    /// it has never executed. submit_https reuses that exact Work and deadline.
    pub(super) fn retry_https_unadmitted(&mut self) -> std::result::Result<(), String> {
        let operation = self
            .https_unadmitted
            .as_ref()
            .and_then(|(work, _)| match work {
                Work::Https(_, _, operation) => Some(operation.clone()),
                _ => None,
            });
        if let Some(operation) = operation {
            self.submit_https(operation)?;
        }
        Ok(())
    }
    pub(super) fn https_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        let Some(owner) = self.https_owner() else {
            self.lock();
            return self.render();
        };
        if self.pending.is_some() {
            if action == editor::CANCEL_WAIT {
                if let Some(p) = &self.pending {
                    p.ticket.cancel();
                }
                if let Some(e) = &mut self.https_editor {
                    e.clear_certificate();
                }
                self.status = if self
                    .https_pending
                    .as_ref()
                    .is_some_and(|p| p.kind == Kind::Save)
                {
                    editor::UNCONFIRMED
                } else {
                    "已取消等待；旧检查结果已失效。"
                }
                .into();
                return self.render();
            }
            return Ok(());
        }
        let command = self
            .https_editor
            .as_mut()
            .ok_or("配置页面已关闭。")?
            .handle(owner, action, data);
        match command {
            Ok(editor::Command::Stay) => {
                self.status.clear();
            }
            Ok(editor::Command::Close) => {
                self.screen = Screen::Home;
                self.status.clear();
                return self.open_web_editor();
            }
            Ok(editor::Command::Reload) => {
                return self.submit_https(Operation::Load);
            }
            Ok(editor::Command::Save) => {
                let id = command_id().map_err(|e| error_text(&e))?;
                let request = self
                    .https_editor
                    .as_mut()
                    .ok_or("配置页面已关闭。")?
                    .prepare_save(id, Instant::now());
                match request {
                    Ok(r) => return self.submit_https(Operation::Save(r)),
                    Err(message) => self.status = message,
                }
            }
            Ok(editor::Command::CheckCertificate) => {
                if let Some(expected) = self
                    .https_editor
                    .as_ref()
                    .and_then(editor::Editor::certificate_state)
                {
                    return self.submit_https(Operation::Check(expected));
                }
                self.status = "请先保存并核对当前配置。".into();
            }
            Ok(editor::Command::Query) => {
                if let Some(request) = self
                    .https_editor
                    .as_ref()
                    .and_then(editor::Editor::query_request)
                {
                    return self.submit_https(Operation::Query(request));
                }
                self.status = "当前没有可查询的原保存操作。".into();
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
    pub(super) fn poll_https_settings(&mut self) -> std::result::Result<(), String> {
        if self.screen != Screen::HttpsSettings {
            return Ok(());
        }
        let changed = match (&mut self.https_editor, self.lease, self.control.as_ref()) {
            (Some(e), Some(lease), Some(control)) => e.invalidate_certificate(lease, control),
            (Some(e), _, _) => {
                e.clear_certificate();
                false
            }
            _ => false,
        };
        if changed {
            self.render()?;
        }
        Ok(())
    }
    pub(super) fn https_completed(
        &mut self,
        pending: Pending,
        generation: u64,
        outcome: Result<Outcome>,
    ) -> std::result::Result<(), String> {
        let same_interval = generation == self.generation
            && self.lease == Some(pending.lease)
            && self.screen == Screen::HttpsSettings
            && self.https_allowed(pending.lease);
        match outcome {
            Ok(Outcome::Https(done)) => {
                let healthy = self.control.as_ref().is_some_and(|c| {
                    let s = c.health_gate().status();
                    s.health == fc_runtime::health::Health::Healthy
                        && s.revision == done.publication
                });
                if same_interval && done.lease == pending.lease && healthy {
                    let e = self.https_editor.as_mut().ok_or("配置页面已关闭。")?;
                    match done.value {
                        Value::Loaded(state) if pending.kind == Kind::Load => {
                            e.loaded(state);
                            self.status = "已读取当前配置；教师连接尚未启用。".into();
                        }
                        Value::Saved(state) if pending.kind == Kind::Save => {
                            e.loaded(state);
                            self.status = editor::SAVED.into();
                        }
                        Value::Queried(result) if pending.kind == Kind::Query => {
                            e.queried(result);
                            self.status = "已完成原操作结果查询；当前配置须另行读取。".into();
                        }
                        Value::Checked(observation) if pending.kind == Kind::Check => {
                            let current = e.certificate_state().is_some_and(|s| {
                                self.control.as_ref().is_some_and(|c| {
                                    observation.is_current_for(&s, pending.lease, c)
                                })
                            });
                            if current {
                                e.certificate_loaded(*observation);
                                self.status = "已完成一次只读检查；不表示教师连接可启用。".into();
                            } else {
                                e.clear_certificate();
                                self.status = "配置或会话已变化，请重新检查。".into();
                            }
                        }
                        _ => self.status = "本次结果不匹配，请重新核对。".into(),
                    }
                } else if self.screen == Screen::HttpsSettings {
                    self.status = editor::UNCONFIRMED.into();
                }
            }
            Err(error) => {
                if fatal(&error) {
                    if let Some(c) = &self.control {
                        c.fault();
                    }
                    self.begin_shutdown();
                }
                if same_interval {
                    self.status = match pending.kind {
                        Kind::Save => format!("{} {}", error_text(&error), editor::UNCONFIRMED),
                        Kind::Check => {
                            "证书只读检查未完成，旧结果已失效；可核对配置后重新检查。".into()
                        }
                        _ => "读取未完成，请核对权限后再明确重试。".into(),
                    };
                }
            }
            _ => {
                if same_interval {
                    self.status = "本次结果不匹配，请重新核对。".into();
                }
            }
        }
        self.render()
    }
}

#[cfg(test)]
mod tests;
