//! Ordinary HTTPS composition. The UI owns only bounded control/status; the
//! native host is constructed and retired on the runtime's HTTP domain.
use super::*;
use fc_bootstrap::https_runtime::{
    self as runtime, RuntimeOwner, RuntimePhase, StartPlan, StartRequest,
};

pub(super) struct Job {
    backend: Arc<Mutex<Backend>>,
    request: StartRequest,
    // Only an explicit hidden fixture can supply a protected deployment root.
    package: Option<PathBuf>,
}
impl Job {
    pub(super) fn allocation_bytes(&self) -> Option<usize> {
        self.request
            .allocation_bytes()
            .checked_add(1024)?
            .checked_add(
                self.package
                    .as_ref()
                    .map_or(0, |p| p.as_os_str().len().saturating_mul(2)),
            )
    }
    pub(super) fn run(self, context: &JobContext) -> Result<Outcome> {
        let value = match self.package {
            Some(package) => {
                runtime::prepare_start_from_package(self.backend, self.request, &package, context)
            }
            None => runtime::prepare_start(self.backend, self.request, context),
        };
        Ok(Outcome::HttpsRuntime(Box::new(Prepared(value))))
    }
}
pub(super) struct Prepared(std::result::Result<StartPlan, runtime::RuntimeError>);
enum Preparing {
    Unadmitted(Box<Job>),
    Running(JobTicket<Result<Outcome>>),
}
#[derive(Default)]
pub(super) struct State {
    pub(super) owner: Option<RuntimeOwner>,
    request: Option<StartRequest>,
    preparing: Option<Preparing>,
    lease: Option<SessionLease>,
    prepare_deadline: Option<Instant>,
    stop_deadline: Option<Instant>,
    failure: Option<runtime::RuntimeError>,
    // Remains private to the explicit hidden probe; never comes from a form.
    pub(super) probe_package: Option<PathBuf>,
}
impl State {
    pub(super) fn failure_message(&self) -> Option<&'static str> {
        self.failure.map(runtime::RuntimeError::message)
    }
    pub(super) fn quiescent(&self) -> bool {
        self.preparing.is_none() && self.owner.as_ref().is_none_or(RuntimeOwner::is_quiescent)
    }
    fn stop(&mut self) {
        let began_at = Instant::now();
        if let Some(request) = &self.request {
            request.cancel();
        }
        if let Some(Preparing::Running(ticket)) = &self.preparing {
            ticket.cancel();
        }
        if matches!(self.preparing, Some(Preparing::Unadmitted(_))) {
            self.preparing = None;
        }
        if let Some(owner) = &self.owner {
            owner.request_stop_at(began_at);
        }
        if !self.quiescent() {
            self.stop_deadline
                .get_or_insert(began_at + Duration::from_secs(5));
        }
    }
}
impl App {
    pub(super) fn stop_teacher_https(&mut self) {
        self.teacher_https.stop();
        if !self.teacher_https.quiescent() {
            self.set_web_readiness(web_editor::Readiness::Stopping);
        } else if matches!(
            self.web_readiness,
            web_editor::Readiness::Starting | web_editor::Readiness::Stopping
        ) {
            self.set_web_readiness(web_editor::Readiness::Disabled);
        }
    }
    pub(super) fn teacher_https_quiescent(&self) -> bool {
        self.teacher_https.quiescent()
    }
    pub(super) fn set_web_readiness(&mut self, readiness: web_editor::Readiness) {
        self.web_readiness = readiness;
        if let Some(editor) = &mut self.web_editor {
            editor.set_readiness(readiness);
        }
    }
    pub(super) fn start_teacher_https(
        &mut self,
        lease: SessionLease,
    ) -> std::result::Result<(), String> {
        if !self.teacher_https.quiescent() || self.web_readiness != web_editor::Readiness::Disabled
        {
            self.status = "教师连接仍在启动或清理，不能重复启用。".into();
            return self.render();
        }
        if !self.https_allowed(lease) || self.stopping {
            return Err("启用需要当前已解锁的本机主管理员。".into());
        }
        let backend = self.backend.clone().ok_or("后台尚未就绪。")?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let request = StartRequest::new(lease, deadline);
        self.teacher_https.owner = None; // prior owner already physically quiescent
        self.teacher_https.request = Some(request.clone());
        self.teacher_https.lease = Some(lease);
        self.teacher_https.prepare_deadline = Some(deadline);
        self.teacher_https.stop_deadline = None;
        self.teacher_https.failure = None;
        let package = if self.offscreen_probe {
            self.teacher_https.probe_package.clone()
        } else {
            None
        };
        self.teacher_https.preparing = Some(Preparing::Unadmitted(Box::new(Job {
            backend,
            request,
            package,
        })));
        self.set_web_readiness(web_editor::Readiness::Starting);
        self.status = "正在核对配置并启动教师连接，尚不能配对。".into();
        self.render()
    }
    fn force_teacher_https_exit(&mut self, expired: Instant) {
        self.set_web_readiness(web_editor::Readiness::Faulted);
        self.terminate_required = true;
        self.begin_shutdown();
        // begin_shutdown also coordinates unrelated owners, but may not renew
        // the already expired network budget. Actual exit retains all owners.
        self.shutdown_deadline = Some(self.shutdown_deadline.map_or(expired, |d| d.min(expired)));
    }
    pub(super) fn poll_teacher_https(&mut self) -> std::result::Result<(), String> {
        let before = self.web_readiness;
        // Lock/cancel during preparation cannot deliver a newly activated host.
        // An established Running domain does not depend on a management page.
        if self.web_readiness == web_editor::Readiness::Starting
            && (self.stopping
                || self
                    .teacher_https
                    .prepare_deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                || self
                    .teacher_https
                    .lease
                    .is_none_or(|lease| self.lease != Some(lease) || !self.https_allowed(lease)))
        {
            self.stop_teacher_https();
        }
        if let Some(preparing) = self.teacher_https.preparing.take() {
            match preparing {
                Preparing::Unadmitted(job) => {
                    let deadline = self
                        .teacher_https
                        .prepare_deadline
                        .ok_or("缺少原启动期限。")?;
                    if Instant::now() >= deadline || self.teacher_https.stop_deadline.is_some() {
                        self.teacher_https.request.take().map(|r| r.cancel());
                        self.set_web_readiness(web_editor::Readiness::Disabled);
                        self.status = "本次教师连接启动未完成，可核对配置后明确重试。".into();
                    } else {
                        match self
                            .pool
                            .handle()
                            .try_submit(Work::HttpsRuntime(job), deadline)
                        {
                            Ok(ticket) => {
                                self.teacher_https.preparing = Some(Preparing::Running(ticket))
                            }
                            Err(rejected)
                                if rejected.kind == fc_runtime::worker::Rejection::Busy =>
                            {
                                let Work::HttpsRuntime(job) = rejected.task else {
                                    unreachable!("returned start input")
                                };
                                self.teacher_https.preparing = Some(Preparing::Unadmitted(job));
                            }
                            Err(_) => {
                                self.teacher_https.request.take().map(|r| r.cancel());
                                self.set_web_readiness(web_editor::Readiness::Disabled);
                                self.status = "后台未接纳本次启动，请核对后明确重试。".into();
                            }
                        }
                    }
                }
                Preparing::Running(mut ticket) => {
                    // A canceled/lost reply is not actual worker completion.
                    if !ticket.is_finished() {
                        self.teacher_https.preparing = Some(Preparing::Running(ticket));
                    } else {
                        match ticket.try_result() {
                            Ok(Some(Ok(Outcome::HttpsRuntime(prepared)))) => match prepared.0 {
                                Ok(plan)
                                    if self.teacher_https.stop_deadline.is_none()
                                        && self.teacher_https.lease.is_some_and(|lease| {
                                            self.lease == Some(lease) && self.https_allowed(lease)
                                        })
                                        && !self.stopping =>
                                {
                                    match RuntimeOwner::start(
                                        plan,
                                        http_executor::SharedHttpExecutor::new(self.pool.handle()),
                                    ) {
                                        Ok(owner) => self.teacher_https.owner = Some(owner),
                                        Err(error) => {
                                            self.teacher_https.failure = Some(error);
                                            self.status = runtime_message(&error);
                                            self.set_web_readiness(web_editor::Readiness::Disabled);
                                        }
                                    }
                                }
                                Ok(_) => self.set_web_readiness(web_editor::Readiness::Disabled),
                                Err(error) => {
                                    self.teacher_https.failure = Some(error);
                                    self.status = runtime_message(&error);
                                    self.set_web_readiness(web_editor::Readiness::Disabled);
                                }
                            },
                            Ok(None) => {
                                self.teacher_https.preparing = Some(Preparing::Running(ticket))
                            }
                            _ => {
                                self.set_web_readiness(web_editor::Readiness::Disabled);
                                self.status = "教师连接启动已取消或结果未交付；未开启配对。".into();
                            }
                        }
                    }
                }
            }
        }
        if let Some(owner) = self.teacher_https.owner.as_mut() {
            let status = owner.poll();
            let readiness = match status.phase {
                RuntimePhase::Disabled => web_editor::Readiness::Disabled,
                RuntimePhase::Starting => web_editor::Readiness::Starting,
                RuntimePhase::Running if status.generation != 0 => web_editor::Readiness::Running {
                    generation: status.generation,
                },
                RuntimePhase::Running | RuntimePhase::Faulted => web_editor::Readiness::Faulted,
                RuntimePhase::Stopping => web_editor::Readiness::Stopping,
            };
            if let Some(error) = status.error {
                self.teacher_https.failure = Some(error);
                self.status = runtime_message(&error);
            }
            self.set_web_readiness(readiness);
            if readiness == web_editor::Readiness::Faulted && !self.teacher_https.quiescent() {
                self.teacher_https.stop();
            }
            if status.error == Some(runtime::RuntimeError::StopTimeout) {
                // The HTTP domain's original stop interval already elapsed.
                // Do not grant another interval when the UI observes Faulted.
                self.force_teacher_https_exit(Instant::now());
            }
        }
        if self.teacher_https.quiescent() {
            self.teacher_https.stop_deadline = None;
            if self.web_readiness == web_editor::Readiness::Stopping {
                self.set_web_readiness(web_editor::Readiness::Disabled);
            }
        } else if self
            .teacher_https
            .stop_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            // Retain the exact owner/tickets; a timeout never permits replacement.
            let expired = self
                .teacher_https
                .stop_deadline
                .expect("observed original stop deadline");
            self.force_teacher_https_exit(expired);
        }
        if self.web_readiness != before {
            if self.web_readiness.ready() {
                self.status = "教师连接已启动；其他设备仍须能够访问此地址并信任证书。".into();
            }
            if self.web_readiness == web_editor::Readiness::Disabled {
                self.next_web_refresh = Instant::now();
            }
            if self.screen == Screen::Web && !self.stopping {
                self.render()?;
            }
        }
        Ok(())
    }
}
fn runtime_message(error: &runtime::RuntimeError) -> String {
    // Stable typed messages are supplied by the runtime; never render an
    // arbitrary provider response, file path or certificate store enumeration.
    error.message().into()
}
