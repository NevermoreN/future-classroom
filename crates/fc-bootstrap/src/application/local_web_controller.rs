//! Tray navigation never unlocks or opens a product-native settings window.
use super::*;
use fc_bootstrap::{
    https_runtime::{RuntimeError, RuntimePhase},
    local_https::{LocalControlHandle, LocalRuntime, PreparedLocal},
    web_application::WebApplication,
};
use fc_domain::https_settings::{
    HttpsCertificateRef, HttpsConfiguration, HttpsHost, HttpsSourceNetworks,
};
use fc_windows::tls_frontdoor::{self, certificate_provision};

const START_ATTEMPTS: u8 = 3;
const START_RETRY_DELAY: Duration = Duration::from_secs(3);
const HEALTHY_RETRY_RESET: Duration = Duration::from_secs(60);
const LAN_CHECK_INTERVAL: Duration = Duration::from_secs(10);

pub(super) enum Job {
    Prepare,
    Attach {
        backend: Arc<Mutex<Backend>>,
        prefix: fc_ports::http::ListenPrefix,
        root: PathBuf,
        control: fc_bootstrap::local_https::LocalControlHandle,
        maintenance: Arc<fc_bootstrap::maintenance_web::MaintenanceService>,
        diagnostics: Arc<Mutex<Diagnostics>>,
        workers: fc_runtime::worker::WorkerHandle<Work>,
    },
    StartLan {
        backend: Arc<Mutex<Backend>>,
        app: Arc<WebApplication>,
        control: fc_bootstrap::local_https::LocalControlHandle,
    },
}
pub(super) enum Completed {
    Prepared(Box<PreparedLocal>),
    Attached(Arc<Mutex<Backend>>, Arc<WebApplication>),
    LanChecked,
}
impl Job {
    pub(super) fn allocation_bytes(&self) -> Option<usize> {
        Some(
            std::mem::size_of::<Self>()
                + match self {
                    Self::Prepare => 0,
                    Self::Attach { root, .. } => root.as_os_str().len() * 2 + 1024,
                    Self::StartLan { .. } => 1024,
                },
        )
    }
    pub(super) fn run(self, context: &JobContext) -> Result<Outcome> {
        let completed = match self {
            Self::Prepare => Completed::Prepared(Box::new(
                PreparedLocal::prepare(context).map_err(|e| invalid(e.message()))?,
            )),
            Self::Attach {
                backend,
                prefix,
                root,
                control,
                maintenance,
                diagnostics,
                workers,
            } => {
                let web = {
                    let value = backend.lock().map_err(|_| Failure::Stopped)?;
                    value.web_service()
                };
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                let mut app = WebApplication::new_local(&prefix, web.clone())
                    .map_err(|_| invalid("本机Web配置无效。"))?;
                app.configure_material_files(&root)
                    .map_err(|_| invalid("本机资料入口未能准备。"))?;
                {
                    let value = backend.lock().map_err(|_| Failure::Stopped)?;
                    app.configure_voice_files(&value, &root)
                        .map_err(|_| invalid("本机录音入口未能准备。"))?;
                }
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                let binding = Arc::new(OnceLock::new());
                let quick_connect_control = control.clone();
                let classrooms_control = control.clone();
                let app = Arc::new(
                    app.with_maintenance_service(maintenance)
                        .with_quick_connect_url(Arc::new(move || {
                            quick_connect_control.quick_connect_url()
                        }))
                        .with_classrooms(Arc::new(move || classrooms_control.classrooms()))
                        .with_admin_platform_hook(web_platform::hook(
                            &backend,
                            control,
                            binding.clone(),
                            diagnostics,
                            workers,
                        )),
                );
                binding
                    .set(Arc::downgrade(&app))
                    .map_err(|_| Failure::Stopped)?;
                Completed::Attached(backend, app)
            }
            Self::StartLan {
                backend,
                app,
                control,
            } => {
                if control.automatic_lan() {
                    match control.begin_lan_edit() {
                        Ok(mut edit) if control.automatic_lan() => {
                            if let Err(message) =
                                check_automatic_lan(&backend, &app, &control, &mut edit, context)
                            {
                                control.set_lan_notice(message);
                            }
                        }
                        Ok(_) | Err(RuntimeError::Busy | RuntimeError::Cancelled) => {}
                        Err(_) => control.set_lan_notice("校园入口未能检查，请查看运行诊断"),
                    }
                }
                Completed::LanChecked
            }
        };
        Ok(Outcome::LocalWeb(completed))
    }
}

fn check_automatic_lan(
    backend: &Arc<Mutex<Backend>>,
    app: &Arc<WebApplication>,
    control: &LocalControlHandle,
    edit: &mut fc_bootstrap::local_https::LanEdit<'_>,
    context: &JobContext,
) -> std::result::Result<(), &'static str> {
    let saved = backend
        .try_lock()
        .map_err(|_| "无法读取已保存的教师HTTPS配置")?
        .startup_https_configuration()
        .map_err(|_| "无法读取已保存的教师HTTPS配置")?
        .filter(|configuration| {
            !configuration
                .host
                .as_str()
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        });
    let previous = control.live_lan_configuration();
    let mut configuration = match saved {
        Some(configuration) => configuration,
        None => {
            let address = certificate_provision::preferred_private_ipv4()
                .map_err(|_| "无法读取本机校园网络地址")?;
            let Some(address) = address else {
                if previous.is_some() {
                    control
                        .configure_lan(None, None, None, context, Arc::new(|| true))
                        .and_then(|ticket| ticket.wait_in_job(context))
                        .map_err(RuntimeError::message)?;
                }
                return Err("等待校园网络分配可用的IPv4地址");
            };
            let host = address.to_string();
            if let Some(prior) = previous.as_ref().filter(|c| c.host.as_str() == host) {
                prepare_teacher_firewall(control, prior.port);
                return Ok(());
            }
            let certificate =
                if tls_frontdoor::verify_listener_certificate(control.certificate(), &host).is_ok()
                {
                    control.certificate()
                } else {
                    provision_teacher_certificate(&host, context, || true)
                        .map_err(RuntimeError::message)?
                };
            HttpsConfiguration::new(
                HttpsHost::parse(&host).map_err(|_| "校园地址无效")?,
                8443,
                certificate,
            )
            .map_err(|_| "教师HTTPS默认配置无效")?
            .with_source_networks(
                HttpsSourceNetworks::parse("10.0.0.0/8\n172.16.0.0/12\n192.168.0.0/16")
                    .map_err(|_| "校园来源范围无效")?,
            )
        }
    };
    // Earlier local-only deployment drafts permitted this computer
    // alone. The new LAN default replaces that obsolete draft in
    // memory, while retaining other administrator-defined policies.
    let self_source = configuration
        .host
        .as_str()
        .parse::<std::net::IpAddr>()
        .ok()
        .map(|ip| format!("{ip}/{}", if ip.is_ipv4() { 32 } else { 128 }));
    if configuration.source_networks.as_ref().is_none_or(|policy| {
        policy.networks().iter().all(|network| {
            matches!(network.as_str(), "127.0.0.1/32" | "::1/128")
                || self_source.as_deref() == Some(network.as_str())
        })
    }) {
        configuration.source_networks = Some(
            HttpsSourceNetworks::parse("10.0.0.0/8\n172.16.0.0/12\n192.168.0.0/16")
                .map_err(|_| "校园来源范围无效")?,
        );
    }
    if previous.as_ref() == Some(&configuration) {
        prepare_teacher_firewall(control, configuration.port);
        return Ok(());
    }
    tls_frontdoor::TlsHttpHost::preflight_lan(&configuration, previous.as_ref())
        .map_err(RuntimeError::from)
        .map_err(RuntimeError::message)?;
    let (lan, assets) =
        lan_resources(app, control, &configuration).map_err(RuntimeError::message)?;
    let rollback = previous
        .map(|prior| {
            let (app, assets) = lan_resources(app, control, &prior)?;
            Ok((prior, app, assets))
        })
        .transpose()
        .map_err(RuntimeError::message)?;
    edit.preview(
        configuration.clone(),
        lan,
        assets,
        rollback,
        context,
        Arc::new(|| true),
    )
    .and_then(|ticket| ticket.wait_in_job(context))
    .map_err(RuntimeError::message)?;
    edit.activate(&configuration)
        .map_err(RuntimeError::message)?;
    prepare_teacher_firewall(control, configuration.port);
    control.set_lan_notice("校园HTTPS监听已就绪");
    Ok(())
}

pub(super) fn lan_resources(
    app: &Arc<WebApplication>,
    control: &LocalControlHandle,
    configuration: &HttpsConfiguration,
) -> std::result::Result<
    (
        Arc<WebApplication>,
        Arc<fc_bootstrap::web_assets::StaticAssets>,
    ),
    RuntimeError,
> {
    let prefix = fc_ports::http::ListenPrefix::https(&format!("{}/fc/", configuration.origin()))
        .map_err(|_| RuntimeError::InvalidConfiguration)?;
    let lan = Arc::new(
        app.lan_variant(&prefix)
            .map_err(|_| RuntimeError::InvalidConfiguration)?,
    );
    Ok((lan, control.assets_for_prefix(&prefix)?))
}

pub(super) fn provision_teacher_certificate(
    host: &str,
    context: &JobContext,
    authority: impl Fn() -> bool,
) -> std::result::Result<HttpsCertificateRef, RuntimeError> {
    let active = || authority() && context.checkpoint().is_ok();
    let certificate = certificate_provision::provision_ip_certificate(host, active)
        .map_err(|_| RuntimeError::Certificate)?;
    if !active() {
        return Err(RuntimeError::Cancelled);
    }
    certificate_provision::trust_owned_certificate(&certificate.thumbprint)
        .map_err(|_| RuntimeError::Certificate)?;
    HttpsCertificateRef::parse(&certificate.thumbprint).map_err(|_| RuntimeError::Certificate)
}

pub(super) fn prepare_teacher_firewall(control: &LocalControlHandle, port: u16) {
    let (https, discovery) = fc_windows::system::prepare_teacher_firewall(port);
    use fc_windows::system::UpdateFirewallStatus::*;
    control.set_firewall_notice(match (https, discovery) {
        (Ready, Ready) => "教师HTTPS与发现的防火墙规则已准备",
        (NeedsAdministrator, _) | (_, NeedsAdministrator) => {
            "防火墙规则需要管理员权限。请让学校管理员运行部署规则或检查组策略"
        }
        (PrivateNetworkRequired, _) | (_, PrivateNetworkRequired) => {
            "当前网络不是专用或域网络，请让学校管理员核对网络配置"
        }
        _ => "学校防火墙策略或现有规则可能限制手机登录或教室发现，请让学校管理员检查",
    });
}

#[derive(Default)]
pub(super) struct Diagnostics {
    pub(super) desktop: Option<fc_windows::desktop::DesktopMetrics>,
    pub(super) desktop_notice: String,
    pub(super) events: VecDeque<DiagnosticEvent>,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) open_when_ready: bool,
    diagnostics: Arc<Mutex<Diagnostics>>,
    next_diagnostics: Option<Instant>,
    pending: Option<JobTicket<Result<Outcome>>>,
    pub(super) runtime: Option<LocalRuntime>,
    prefix: Option<fc_ports::http::ListenPrefix>,
    pub(super) attached: bool,
    pub(super) lan_app: Option<Arc<WebApplication>>,
    next_lan_check: Option<Instant>,
    prepare_attempts: u8,
    attach_attempts: u8,
    next_attempt: Option<Instant>,
    healthy_since: Option<Instant>,
    restart_automatic_lan: Option<bool>,
    failure_reason: Option<&'static str>,
    failed: bool,
    pub(super) failure: Option<String>,
}

impl App {
    pub(super) fn local_web_runtime_reason(&self) -> Option<&'static str> {
        self.local_web.failure_reason
    }

    pub(super) fn local_web_runtime_state(&self) -> &'static str {
        if self.local_web.failed {
            "local-web-failed"
        } else if self.local_web.attached {
            "local-web-ready"
        } else if self.local_web.failure.is_some() {
            "local-web-retrying"
        } else {
            "local-web-starting"
        }
    }

    pub(super) fn start_local_web(&mut self) {
        if self.offscreen_probe
            || self.exiting
            || self.local_web.pending.is_some()
            || self.local_web.runtime.is_some()
            || self.local_web.failed
            || self
                .local_web
                .next_attempt
                .is_some_and(|at| Instant::now() < at)
        {
            return;
        }
        match self.pool.handle().try_submit(
            Work::LocalWeb(Box::new(Job::Prepare)),
            Instant::now() + Duration::from_secs(90),
        ) {
            Ok(ticket) => {
                self.local_web.prepare_attempts += 1;
                self.local_web.pending = Some(ticket);
            }
            Err(_) => {
                self.local_web.failure = Some("本机Web准备暂未接纳，稍后重试。".into());
                self.local_web.failure_reason = Some("local-web-worker-capacity");
                self.local_web.next_attempt = Some(Instant::now() + START_RETRY_DELAY);
            }
        }
    }
    pub(super) fn poll_local_web(&mut self) {
        if self.offscreen_probe {
            return;
        }
        if self
            .local_web
            .next_diagnostics
            .is_none_or(|next| Instant::now() >= next)
        {
            if let Ok(mut snapshot) = self.local_web.diagnostics.try_lock() {
                snapshot.desktop = Some(self.desktop.last_metrics());
                snapshot.desktop_notice.clone_from(&self.desktop_notice);
                snapshot.events.clone_from(&self.diagnostic_events);
            }
            self.local_web.next_diagnostics = Some(Instant::now() + Duration::from_secs(1));
        }
        if self.exiting {
            if let Some(ticket) = &self.local_web.pending {
                ticket.cancel();
            }
            if let Some(runtime) = &self.local_web.runtime {
                runtime.request_stop();
            }
        }
        let completed = match self
            .local_web
            .pending
            .as_mut()
            .filter(|ticket| ticket.is_finished())
            .map(JobTicket::try_result)
        {
            Some(Ok(Some(value))) => Some(value),
            Some(Err(_)) => Some(Err(Failure::Stopped)),
            _ => None,
        };
        let mut start_error = None;
        if let Some(completed) = completed {
            self.local_web.pending = None;
            match completed {
                Ok(Outcome::LocalWeb(Completed::Prepared(prepared))) if !self.exiting => {
                    self.local_web.prefix = Some(prepared.prefix.clone());
                    match LocalRuntime::start(
                        *prepared,
                        http_executor::SharedHttpExecutor::new(self.pool.handle()),
                    ) {
                        Ok(runtime) => {
                            if let Some(enabled) = self.local_web.restart_automatic_lan.take() {
                                runtime.control().set_automatic_lan(enabled);
                            }
                            self.local_web.runtime = Some(runtime);
                        }
                        Err(error) => {
                            start_error = Some(error.message().to_owned());
                            self.local_web.failure_reason = Some(runtime_reason(error));
                        }
                    }
                }
                Ok(Outcome::LocalWeb(Completed::Attached(backend, app))) if !self.exiting => {
                    if let Some(runtime) = &self.local_web.runtime {
                        match runtime.attach(backend, app.clone()) {
                            Ok(()) => {
                                self.local_web.lan_app = Some(app);
                            }
                            Err(error) => {
                                start_error = Some(error.message().to_owned());
                                self.local_web.failure_reason = Some(runtime_reason(error));
                            }
                        }
                    }
                }
                Ok(Outcome::LocalWeb(Completed::LanChecked)) => {}
                Err(error) if !self.exiting && !self.local_web.attached => {
                    start_error = Some(error_text(&error));
                    self.local_web.failure_reason = Some(if self.local_web.runtime.is_some() {
                        "local-web-backend-preparation"
                    } else {
                        "local-web-listener-preparation"
                    });
                }
                _ => {}
            }
        }
        let mut listening = false;
        if let Some(runtime) = &mut self.local_web.runtime {
            let state = runtime.poll();
            listening = state.phase == RuntimePhase::Running;
            if let Some(error) = state.error.filter(|_| !self.exiting) {
                self.local_web.failure = Some(error.message().into());
                self.local_web.failure_reason = Some(runtime_reason(error));
                self.local_web.healthy_since = None;
                self.local_web.attached = false;
                let control = runtime.control();
                if control.updates_bound() {
                    self.updates.recover_network = true;
                }
                if runtime.is_quiescent() && self.local_web.pending.is_none() {
                    self.local_web.restart_automatic_lan = Some(control.automatic_lan());
                    self.local_web.runtime = None;
                    self.local_web.prefix = None;
                    self.local_web.lan_app = None;
                    self.local_web.attach_attempts = 0;
                    start_error = Some(error.message().to_owned());
                } else {
                    self.local_web.failed =
                        self.local_web.prepare_attempts >= START_ATTEMPTS || !runtime.can_retry();
                }
            } else if listening
                && !self.local_web.attached
                && self.local_web.lan_app.is_some()
                && runtime.control().backend_attached()
            {
                self.local_web.attached = true;
                self.local_web.failure = None;
                self.local_web.failure_reason = None;
                self.local_web.healthy_since = Some(Instant::now());
            }
        }
        if self
            .local_web
            .healthy_since
            .is_some_and(|since| since.elapsed() >= HEALTHY_RETRY_RESET)
        {
            self.local_web.healthy_since = None;
            self.local_web.prepare_attempts = 0;
            self.local_web.attach_attempts = 0;
        }
        if let Some(error) = start_error {
            self.local_web.failure = Some(error);
            self.local_web.failed = if let Some(runtime) = &self.local_web.runtime {
                let failed = self.local_web.attach_attempts >= START_ATTEMPTS;
                if failed {
                    runtime.abandon_backend_startup();
                }
                failed
            } else {
                self.local_web.prepare_attempts >= START_ATTEMPTS
            };
            self.local_web.next_attempt = Some(Instant::now() + START_RETRY_DELAY);
        }
        // The administrator's Stop switches off automatic LAN for this run.
        if !self.exiting
            && !self.local_web.failed
            && self
                .local_web
                .next_lan_check
                .is_none_or(|at| Instant::now() >= at)
            && self.local_web.attached
            && self.local_web.pending.is_none()
        {
            if let (Some(backend), Some(app), Some(runtime)) = (
                &self.backend,
                &self.local_web.lan_app,
                &self.local_web.runtime,
            ) {
                let control = runtime.control();
                if control.backend_attached() && control.automatic_lan() {
                    let job = Job::StartLan {
                        backend: backend.clone(),
                        app: app.clone(),
                        control,
                    };
                    if let Ok(ticket) = self.pool.handle().try_submit(
                        Work::LocalWeb(Box::new(job)),
                        Instant::now() + Duration::from_secs(60),
                    ) {
                        self.local_web.next_lan_check = Some(Instant::now() + LAN_CHECK_INTERVAL);
                        self.local_web.pending = Some(ticket);
                    }
                }
            }
        }
        if self.local_web.open_when_ready
            && self.local_web.attached
            && !self.exiting
            && fc_bootstrap::local_management::url(true).is_some()
        {
            self.local_web.open_when_ready = false;
            if let Err(error) = self.open_local_web(true) {
                self.local_web.failure = Some(error);
            }
        }
        if !self.exiting
            && !self.local_web.failed
            && !self.local_web.attached
            && self.local_web.lan_app.is_none()
            && listening
            && self
                .local_web
                .next_attempt
                .is_none_or(|at| Instant::now() >= at)
            && self.local_web.pending.is_none()
        {
            if let (Some(backend), Some(prefix), Some(runtime)) = (
                &self.backend,
                &self.local_web.prefix,
                &self.local_web.runtime,
            ) {
                let job = Job::Attach {
                    backend: backend.clone(),
                    prefix: prefix.clone(),
                    root: self.root.clone(),
                    control: runtime.control(),
                    maintenance: self
                        .maintenance
                        .service
                        .clone()
                        .expect("normal runtime maintenance owner"),
                    diagnostics: self.local_web.diagnostics.clone(),
                    workers: self.pool.handle(),
                };
                if let Ok(ticket) = self.pool.handle().try_submit(
                    Work::LocalWeb(Box::new(job)),
                    Instant::now() + Duration::from_secs(40),
                ) {
                    self.local_web.attach_attempts += 1;
                    self.local_web.pending = Some(ticket);
                }
            }
        }
        self.start_local_web();
    }
    pub(super) fn local_web_quiescent(&self) -> bool {
        self.local_web.pending.is_none()
            && self
                .local_web
                .runtime
                .as_ref()
                .is_none_or(LocalRuntime::is_quiescent)
    }
    pub(super) fn open_local_web(&self, settings: bool) -> std::result::Result<(), String> {
        let url = fc_bootstrap::local_management::url(settings)
            .ok_or_else(|| self.local_web_unavailable())?;
        fc_windows::startup::open_local_management(&url)
            .map_err(|_| "无法打开系统浏览器，请检查默认浏览器设置。".to_owned())
    }
    pub(super) fn open_quick_connect(&self) -> std::result::Result<(), String> {
        let url = fc_bootstrap::local_management::quick_connect_url()
            .ok_or_else(|| self.local_web_unavailable())?;
        fc_windows::startup::open_local_management(&url)
            .map_err(|_| "无法打开系统浏览器，请检查默认浏览器设置。".to_owned())
    }
    pub(super) fn open_about(&self) -> std::result::Result<(), String> {
        let url = fc_bootstrap::local_management::about_url()
            .ok_or_else(|| self.local_web_unavailable())?;
        fc_windows::startup::open_local_management(&url)
            .map_err(|_| "无法打开系统浏览器，请检查默认浏览器设置。".to_owned())
    }
    fn local_web_unavailable(&self) -> String {
        if self.local_web.failed {
            format!(
                "本机网页入口未能启动。{} 请查看运行诊断。",
                self.local_web.failure.as_deref().unwrap_or("")
            )
        } else {
            "未来教室正在准备本机网页，请稍后重试。".into()
        }
    }
}

fn runtime_reason(error: RuntimeError) -> &'static str {
    match error {
        RuntimeError::Busy => "local-web-owner-busy",
        RuntimeError::StaticAssets => "local-web-assets-unavailable",
        RuntimeError::BackendUnavailable => "local-web-backend-attachment",
        RuntimeError::AddressUnavailable => "local-web-address-unavailable",
        RuntimeError::Certificate => "local-web-certificate-unavailable",
        RuntimeError::Native => "local-web-native-listener",
        RuntimeError::Cleanup | RuntimeError::StopTimeout => "local-web-owner-drain",
        _ => "local-web-listener-unavailable",
    }
}
