//! Tray navigation never unlocks or opens a product-native settings window.
use super::*;
use fc_bootstrap::{
    local_https::{LocalRuntime, PreparedLocal},
    web_application::WebApplication,
};

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
    LanStarted,
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
                let app = Arc::new(
                    app.with_maintenance_service(maintenance)
                        .with_quick_connect_url(Arc::new(move || {
                            quick_connect_control.quick_connect_url()
                        }))
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
                use fc_domain::https_settings::{
                    HttpsConfiguration, HttpsHost, HttpsSourceNetworks,
                };
                let saved = backend
                    .lock()
                    .map_err(|_| Failure::Stopped)?
                    .startup_https_configuration()
                    .map_err(|_| invalid("无法读取已保存的教师HTTPS配置。"))?
                    .filter(|configuration| {
                        !configuration
                            .host
                            .as_str()
                            .parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())
                    });
                let mut configuration = match saved {
                    Some(configuration) => configuration,
                    None => {
                        let hosts = fc_windows::tls_frontdoor::certificate_provision::local_certificate_hosts()
                            .map_err(|_| invalid("无法读取本机局域网地址。"))?;
                        let host = hosts
                            .iter()
                            .find(|host| {
                                host.parse::<std::net::Ipv4Addr>()
                                    .is_ok_and(|ip| ip.is_private())
                            })
                            .ok_or_else(|| {
                                invalid("未找到可用的局域网IPv4地址，请连接校园网络后重启程序。")
                            })?;
                        HttpsConfiguration::new(
                            HttpsHost::parse(host).map_err(|_| invalid("局域网地址无效。"))?,
                            8443,
                            control.certificate(),
                        )
                        .map_err(|_| invalid("教师HTTPS默认配置无效。"))?
                        .with_source_networks(
                            HttpsSourceNetworks::parse("10.0.0.0/8\n172.16.0.0/12\n192.168.0.0/16")
                                .map_err(|_| invalid("局域网来源范围无效。"))?,
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
                            .map_err(|_| invalid("局域网来源范围无效。"))?,
                    );
                }
                control.remember_startup_configuration(configuration.clone());
                let prefix =
                    fc_ports::http::ListenPrefix::https(&format!("{}/fc/", configuration.origin()))
                        .map_err(|_| invalid("教师HTTPS地址无效。"))?;
                let lan = Arc::new(
                    app.lan_variant(&prefix)
                        .map_err(|_| invalid("教师网页入口未能准备。"))?,
                );
                let assets = control
                    .assets_for_prefix(&prefix)
                    .map_err(|_| invalid("教师网页资源未能准备。"))?;
                control
                    .configure_lan(
                        Some(configuration),
                        Some(lan),
                        Some(assets),
                        context,
                        Arc::new(|| true),
                    )
                    .map_err(|error| invalid(error.message()))?
                    .wait_in_job(context)
                    .map_err(|error| invalid(error.message()))?;
                Completed::LanStarted
            }
        };
        Ok(Outcome::LocalWeb(completed))
    }
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
    attached: bool,
    lan_app: Option<Arc<WebApplication>>,
    lan_attempted: bool,
    failed: bool,
    pub(super) failure: Option<String>,
}

impl App {
    pub(super) fn start_local_web(&mut self) {
        if self.offscreen_probe
            || self.local_web.pending.is_some()
            || self.local_web.runtime.is_some()
            || self.local_web.failed
        {
            return;
        }
        match self.pool.handle().try_submit(
            Work::LocalWeb(Box::new(Job::Prepare)),
            Instant::now() + Duration::from_secs(90),
        ) {
            Ok(ticket) => self.local_web.pending = Some(ticket),
            Err(_) => {
                self.local_web.failure = Some("本机Web准备暂未接纳，稍后重试。".into());
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
        if let Some(completed) = completed {
            self.local_web.pending = None;
            match completed {
                Ok(Outcome::LocalWeb(Completed::Prepared(prepared))) if !self.stopping => {
                    self.local_web.prefix = Some(prepared.prefix.clone());
                    match LocalRuntime::start(
                        *prepared,
                        http_executor::SharedHttpExecutor::new(self.pool.handle()),
                    ) {
                        Ok(runtime) => self.local_web.runtime = Some(runtime),
                        Err(error) => {
                            self.local_web.failure = Some(error.message().into());
                            self.local_web.failed = true;
                        }
                    }
                }
                Ok(Outcome::LocalWeb(Completed::Attached(backend, app))) if !self.stopping => {
                    if let Some(runtime) = &self.local_web.runtime {
                        match runtime.attach(backend, app.clone()) {
                            Ok(()) => {
                                self.local_web.attached = true;
                                self.local_web.lan_app = Some(app);
                            }
                            Err(error) => {
                                self.local_web.failure = Some(error.message().into());
                                self.local_web.failed = true;
                            }
                        }
                    }
                }
                Ok(Outcome::LocalWeb(Completed::LanStarted)) => {}
                Err(error) if !self.stopping => {
                    self.local_web.failure = Some(error_text(&error));
                    if !self.local_web.lan_attempted {
                        self.local_web.failed = true;
                    }
                }
                _ => {}
            }
        }
        if let Some(runtime) = &mut self.local_web.runtime {
            let state = runtime.poll();
            if let Some(error) = state.error {
                self.local_web.failure = Some(error.message().into());
                self.local_web.failed = true;
            }
        }
        // One startup attempt only: a later administrator Stop must stay stopped.
        // Attachment is acknowledged by the network owner before LAN work begins.
        if !self.stopping
            && !self.local_web.failed
            && !self.local_web.lan_attempted
            && self.local_web.attached
            && self.local_web.pending.is_none()
        {
            if let (Some(backend), Some(app), Some(runtime)) = (
                &self.backend,
                &self.local_web.lan_app,
                &self.local_web.runtime,
            ) {
                let control = runtime.control();
                if control.backend_attached() {
                    let job = Job::StartLan {
                        backend: backend.clone(),
                        app: app.clone(),
                        control,
                    };
                    if let Ok(ticket) = self.pool.handle().try_submit(
                        Work::LocalWeb(Box::new(job)),
                        Instant::now() + Duration::from_secs(60),
                    ) {
                        self.local_web.lan_attempted = true;
                        self.local_web.pending = Some(ticket);
                    }
                }
            }
        }
        if self.local_web.open_when_ready
            && self.local_web.attached
            && !self.stopping
            && fc_bootstrap::local_management::url(true).is_some()
        {
            self.local_web.open_when_ready = false;
            if let Err(error) = self.open_local_web(true) {
                self.local_web.failure = Some(error);
            }
        }
        if !self.stopping
            && !self.local_web.failed
            && !self.local_web.attached
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
                    self.local_web.pending = Some(ticket);
                }
            }
        }
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
            .ok_or_else(|| "本机Web入口尚未就绪，请稍后重试；程序仍在运行。".to_owned())?;
        fc_windows::startup::open_local_management(&url)
            .map_err(|_| "无法打开系统浏览器，请检查默认浏览器设置。".to_owned())
    }
    pub(super) fn open_quick_connect(&self) -> std::result::Result<(), String> {
        let url = fc_bootstrap::local_management::quick_connect_url()
            .ok_or_else(|| "本机Web入口尚未就绪，请稍后重试；程序仍在运行。".to_owned())?;
        fc_windows::startup::open_local_management(&url)
            .map_err(|_| "无法打开系统浏览器，请检查默认浏览器设置。".to_owned())
    }
}
