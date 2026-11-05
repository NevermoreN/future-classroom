//! The sole process-owned local HTTPS/IOCP domain. Teaching backend attachment
//! is later and never creates another writer, worker pool or network owner.
use crate::{
    backend::Backend,
    https_runtime::{
        poll_cleanup, CleanupWork, RuntimeError, RuntimePhase, RuntimeStatus, STOP_BUDGET,
    },
    web_application::WebApplication,
    web_assets::StaticAssets,
    web_transfer::{
        network::{NetCoordinator, NetworkReport},
        DownloadHubOwner, HttpExecutor, HttpJob, UploadHubOwner,
    },
};
use fc_domain::https_settings::{
    HttpsCertificateRef, HttpsConfiguration, HttpsHost, HttpsSourceNetworks,
};
use fc_ports::http::ListenPrefix;
use fc_runtime::worker::JobContext;
use fc_windows::tls_frontdoor::{
    certificate_provision::{
        local_certificate_hosts, provision_hosts_certificate, trust_owned_certificate,
    },
    ConfiguredTlsError, LanListenerPhase, TlsHttpHost,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub struct PreparedLocal {
    pub port: u16,
    pub prefix: ListenPrefix,
    configuration: HttpsConfiguration,
    assets: Arc<StaticAssets>,
}
impl PreparedLocal {
    pub fn prepare(context: &JobContext) -> Result<Self, RuntimeError> {
        context.checkpoint().map_err(|_| RuntimeError::Cancelled)?;
        let port = (18443..=18452)
            .find(|port| {
                std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, *port)).is_ok()
            })
            .ok_or(RuntimeError::AddressUnavailable)?;
        let prefix = ListenPrefix::https(&format!("https://127.0.0.1:{port}/fc/"))
            .map_err(|_| RuntimeError::InvalidConfiguration)?;
        let exe = std::env::current_exe().map_err(|_| RuntimeError::StaticAssets)?;
        let root = exe.parent().ok_or(RuntimeError::StaticAssets)?.join("web");
        let assets = Arc::new(
            StaticAssets::load_in_job(&prefix, &root, context)
                .map_err(|_| RuntimeError::StaticAssets)?,
        );
        let hosts = local_certificate_hosts().map_err(|_| RuntimeError::AddressUnavailable)?;
        if !hosts.iter().any(|host| host == "127.0.0.1") {
            return Err(RuntimeError::AddressUnavailable);
        }
        let hosts = hosts.iter().map(String::as_str).collect::<Vec<_>>();
        let certificate = provision_hosts_certificate(&hosts, || context.checkpoint().is_ok())
            .map_err(|_| RuntimeError::Certificate)?;
        context.checkpoint().map_err(|_| RuntimeError::Cancelled)?;
        trust_owned_certificate(&certificate.thumbprint).map_err(|_| RuntimeError::Certificate)?;
        context.checkpoint().map_err(|_| RuntimeError::Cancelled)?;
        let configuration = HttpsConfiguration::new(
            HttpsHost::parse("127.0.0.1").map_err(|_| RuntimeError::InvalidConfiguration)?,
            port,
            HttpsCertificateRef::parse(&certificate.thumbprint)
                .map_err(|_| RuntimeError::Certificate)?,
        )
        .map_err(|_| RuntimeError::InvalidConfiguration)?
        .with_source_networks(
            HttpsSourceNetworks::parse("127.0.0.1/32")
                .map_err(|_| RuntimeError::InvalidConfiguration)?,
        );
        Ok(Self {
            port,
            prefix,
            configuration,
            assets,
        })
    }
    pub fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.assets.allocation_bytes()
            + self.configuration.allocation_bytes()
    }
    pub fn assets(&self) -> Arc<StaticAssets> {
        self.assets.clone()
    }
}
struct Attachment {
    backend: Arc<Mutex<Backend>>,
    app: Arc<WebApplication>,
}
struct Shared {
    status: RuntimeStatus,
    stop_at: Option<Instant>,
    report: Option<NetworkReport>,
    returned: bool,
    attached: bool,
    pending: Option<Attachment>,
    pending_repair: Option<Arc<crate::repair_web::RepairService>>,
    pending_updates: Option<crate::update_network::NetworkControl>,
    update_control: Option<crate::update_network::NetworkControl>,
    service: Option<crate::backend::web::WebService>,
    lan_commands: std::collections::VecDeque<LanCommand>,
    lan_changing: bool,
    negative_lan_stop: bool,
    business_drain_requested: bool,
    business_quiescent: bool,
    lan_quiescent: bool,
    lan_status: LanStatus,
    lan_configuration: Option<HttpsConfiguration>,
    startup_configuration: Option<HttpsConfiguration>,
    discovery_status: &'static str,
    classrooms: fc_protocol::web::responses::Classrooms,
}
static CLAIMED: AtomicBool = AtomicBool::new(false);
pub struct LocalRuntime {
    shared: Arc<Mutex<Shared>>,
    thread: Option<JoinHandle<()>>,
    port: u16,
    quiescent: bool,
    certificate: HttpsCertificateRef,
    assets: Arc<StaticAssets>,
}
impl LocalRuntime {
    pub fn start<E>(prepared: PreparedLocal, executor: E) -> Result<Self, RuntimeError>
    where
        E: HttpExecutor + Send + 'static,
        E::Ticket: Send + 'static,
    {
        CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| RuntimeError::Busy)?;
        let port = prepared.port;
        let certificate = prepared.configuration.certificate;
        let assets = prepared.assets.clone();
        let shared = Arc::new(Mutex::new(Shared {
            status: RuntimeStatus {
                phase: RuntimePhase::Starting,
                generation: 0,
                error: None,
            },
            stop_at: None,
            report: None,
            returned: false,
            attached: false,
            pending: None,
            pending_repair: None,
            pending_updates: None,
            update_control: None,
            service: None,
            lan_commands: std::collections::VecDeque::new(),
            lan_changing: false,
            negative_lan_stop: false,
            business_drain_requested: false,
            business_quiescent: false,
            lan_quiescent: true,
            lan_status: LanStatus {
                phase: LanListenerPhase::Disabled,
                error: None,
            },
            lan_configuration: None,
            startup_configuration: None,
            discovery_status: "未启动",
            classrooms: fc_protocol::web::responses::Classrooms {
                schema_version: 1,
                discovery_available: false,
                rooms: Vec::new(),
            },
        }));
        let state = shared.clone();
        let thread = match std::thread::Builder::new()
            .name("fc-local-https".into())
            .spawn(move || run(prepared, executor, state))
        {
            Ok(thread) => thread,
            Err(_) => {
                CLAIMED.store(false, Ordering::Release);
                return Err(RuntimeError::Native);
            }
        };
        Ok(Self {
            shared,
            thread: Some(thread),
            port,
            quiescent: false,
            certificate,
            assets,
        })
    }
    pub fn control(&self) -> LocalControlHandle {
        LocalControlHandle {
            shared: self.shared.clone(),
            certificate: self.certificate,
            assets: self.assets.clone(),
            port: self.port,
        }
    }
    pub fn certificate(&self) -> HttpsCertificateRef {
        self.certificate
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn attach(
        &self,
        backend: Arc<Mutex<Backend>>,
        app: Arc<WebApplication>,
    ) -> Result<(), RuntimeError> {
        let mut shared = self.shared.lock().map_err(|_| RuntimeError::Faulted)?;
        if shared.attached
            || shared.pending.is_some()
            || shared.pending_repair.is_some()
            || shared.stop_at.is_some()
            || !shared.lan_commands.is_empty()
            || shared.lan_changing
        {
            return Err(RuntimeError::Busy);
        }
        shared.pending = Some(Attachment { backend, app });
        Ok(())
    }
    pub fn attach_repair(
        &self,
        service: Arc<crate::repair_web::RepairService>,
    ) -> Result<(), RuntimeError> {
        let mut shared = self.shared.lock().map_err(|_| RuntimeError::Faulted)?;
        if shared.attached
            || shared.pending.is_some()
            || shared.pending_repair.is_some()
            || shared.stop_at.is_some()
        {
            return Err(RuntimeError::Busy);
        }
        shared.pending_repair = Some(service);
        Ok(())
    }
    pub fn request_stop(&self) {
        self.request_stop_at(Instant::now())
    }
    pub fn request_stop_at(&self, at: Instant) {
        crate::local_management::publish(0);
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(service) = &shared.service {
            service.set_local_control_ready(false);
        }
        shared.stop_at = Some(shared.stop_at.map_or(at, |old| old.min(at)));
        if shared.status.phase != RuntimePhase::Faulted {
            shared.status.phase = RuntimePhase::Stopping;
        }
    }
    pub fn network_report(&self) -> Option<NetworkReport> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).report
    }
    pub fn is_quiescent(&self) -> bool {
        self.quiescent
    }
    pub fn poll(&mut self) -> RuntimeStatus {
        if self
            .thread
            .as_ref()
            .is_some_and(crate::physical_thread::is_finished)
        {
            let joined = self
                .thread
                .take()
                .expect("observed original local HTTP thread")
                .join();
            let mut state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            self.quiescent = state.returned && joined.is_ok();
            if self.quiescent && state.status.error.is_none() {
                state.status.phase = RuntimePhase::Disabled;
                CLAIMED.store(false, Ordering::Release);
            } else if !self.quiescent {
                state.status.phase = RuntimePhase::Faulted;
                state.status.error = Some(RuntimeError::Faulted);
            }
        }
        let mut state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if !self.quiescent
            && state
                .stop_at
                .is_some_and(|at| Instant::now() >= at + STOP_BUDGET)
        {
            state.status.phase = RuntimePhase::Faulted;
            state.status.error = Some(RuntimeError::StopTimeout);
        }
        state.status
    }
    pub fn recover_retained() -> Option<Self> {
        RETAINED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|r| Self {
                shared: r.shared,
                thread: r.thread,
                port: r.port,
                quiescent: false,
                certificate: r.certificate,
                assets: r.assets,
            })
    }
}
struct Retained {
    shared: Arc<Mutex<Shared>>,
    thread: Option<JoinHandle<()>>,
    port: u16,
    certificate: HttpsCertificateRef,
    assets: Arc<StaticAssets>,
}
static RETAINED: Mutex<Option<Retained>> = Mutex::new(None);
impl Drop for LocalRuntime {
    fn drop(&mut self) {
        if !self.quiescent && self.thread.is_some() {
            self.request_stop();
            let mut slot = RETAINED.lock().unwrap_or_else(|e| e.into_inner());
            assert!(slot.is_none(), "one retained local HTTPS owner");
            *slot = Some(Retained {
                shared: self.shared.clone(),
                thread: self.thread.take(),
                port: self.port,
                certificate: self.certificate,
                assets: self.assets.clone(),
            });
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct LanStatus {
    pub phase: LanListenerPhase,
    pub error: Option<RuntimeError>,
}
#[derive(Clone)]
pub struct LocalControlHandle {
    shared: Arc<Mutex<Shared>>,
    certificate: HttpsCertificateRef,
    port: u16,
    assets: Arc<StaticAssets>,
}
struct LanCommand {
    configuration: Option<HttpsConfiguration>,
    app: Option<Arc<WebApplication>>,
    assets: Option<Arc<StaticAssets>>,
    active: crate::web_transfer::ExecutionFence,
    reply: mpsc::SyncSender<Result<(), RuntimeError>>,
}
pub struct ConfigureTicket {
    result: mpsc::Receiver<Result<(), RuntimeError>>,
}
impl ConfigureTicket {
    pub fn wait_in_job(self, context: &JobContext) -> Result<(), RuntimeError> {
        loop {
            context.checkpoint().map_err(|_| RuntimeError::Cancelled)?;
            match self.result.recv_timeout(Duration::from_millis(5)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(RuntimeError::Faulted),
            }
        }
    }
    pub fn try_result(&self) -> Result<Option<Result<(), RuntimeError>>, RuntimeError> {
        match self.result.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(_) => Err(RuntimeError::Faulted),
        }
    }
}
impl LocalControlHandle {
    pub fn attach_updates(
        &self,
        control: crate::update_network::NetworkControl,
    ) -> Result<(), RuntimeError> {
        let mut state = self.shared.lock().map_err(|_| RuntimeError::Faulted)?;
        if state.stop_at.is_some()
            || state.pending_updates.is_some()
            || state.update_control.is_some()
        {
            return Err(RuntimeError::Busy);
        }
        state.pending_updates = Some(control.clone());
        state.update_control = Some(control);
        Ok(())
    }

    pub fn discovery_state(&self) -> &'static str {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .discovery_status
    }
    pub fn remember_startup_configuration(&self, configuration: HttpsConfiguration) {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .startup_configuration = Some(configuration);
    }
    pub fn startup_configuration(&self) -> Option<HttpsConfiguration> {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .startup_configuration
            .clone()
    }
    /// Irreversible business shutdown while the local native control listener
    /// stays alive. Complete this on the original pool before cold handoff.
    pub fn request_business_drain(&self) {
        let mut state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if !state.business_drain_requested {
            state.business_drain_requested = true;
            state.business_quiescent = false;
            state.negative_lan_stop = true;
            state.lan_quiescent = false;
        }
    }
    pub fn business_quiescent(&self) -> bool {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .business_quiescent
    }
    /// Negative-only recovery control; grants no authority and cannot start LAN.
    pub fn request_lan_stop(&self) {
        let mut state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        state.negative_lan_stop = true;
        state.lan_quiescent = false;
        if let Some(service) = &state.service {
            let _ = service.set_lan_ready(false);
        }
    }
    pub fn lan_quiescent(&self) -> bool {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .lan_quiescent
    }

    pub fn lan_status(&self) -> LanStatus {
        self.lan_state()
    }
    /// Native composition readiness; does not grant browser administration.
    pub fn backend_attached(&self) -> bool {
        let state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        state.attached
            && state.pending.is_none()
            && state.stop_at.is_none()
            && !state.business_drain_requested
    }
    /// No saved drafts or loopback maintenance URLs are published to phones.
    pub fn quick_connect_url(&self) -> Option<String> {
        let state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if state.lan_status.phase != LanListenerPhase::Listening
            || state.lan_status.error.is_some()
            || state.lan_changing
            || !state.lan_commands.is_empty()
            || state.negative_lan_stop
            || state.business_drain_requested
            || state.stop_at.is_some()
        {
            return None;
        }
        let config = state.lan_configuration.as_ref()?;
        let address: std::net::IpAddr = config.host.as_str().parse().ok()?;
        if address.is_loopback()
            || address.is_unspecified()
            || address.is_multicast()
            || matches!(address, std::net::IpAddr::V4(ip) if ip.is_broadcast())
            || matches!(address, std::net::IpAddr::V6(ip) if ip.is_unicast_link_local() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast()))
        {
            return None;
        }
        Some(format!("{}/fc/", config.origin()))
    }
    pub fn classrooms(&self) -> fc_protocol::web::responses::Classrooms {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .classrooms
            .clone()
    }
    pub fn assets_for_prefix(
        &self,
        prefix: &ListenPrefix,
    ) -> Result<Arc<StaticAssets>, RuntimeError> {
        self.assets
            .shared_for_prefix(prefix)
            .map(Arc::new)
            .map_err(|_| RuntimeError::StaticAssets)
    }

    pub fn lan_state(&self) -> LanStatus {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .lan_status
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn certificate(&self) -> HttpsCertificateRef {
        self.certificate
    }
    pub fn configure_lan(
        &self,
        configuration: Option<HttpsConfiguration>,
        app: Option<Arc<WebApplication>>,
        assets: Option<Arc<StaticAssets>>,
        context: &JobContext,
        authority: crate::web_transfer::ExecutionFence,
    ) -> Result<ConfigureTicket, RuntimeError> {
        context.checkpoint().map_err(|_| RuntimeError::Cancelled)?;
        if configuration.is_some() != app.is_some() || configuration.is_some() != assets.is_some() {
            return Err(RuntimeError::InvalidConfiguration);
        }
        let mut shared = self.shared.lock().map_err(|_| RuntimeError::Faulted)?;
        if !shared.attached
            || shared.pending.is_some()
            || shared.stop_at.is_some()
            || shared.lan_commands.len() + usize::from(shared.lan_changing) >= 2
        {
            return Err(RuntimeError::Busy);
        }
        let (reply, result) = mpsc::sync_channel(1);
        let job = context.execution_fence();
        shared.lan_commands.push_back(LanCommand {
            configuration,
            app,
            assets,
            active: Arc::new(move || job() && authority()),
            reply,
        });
        Ok(ConfigureTicket { result })
    }
}
fn fail(shared: &Arc<Mutex<Shared>>, error: RuntimeError) {
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    state.status.phase = RuntimePhase::Faulted;
    state.status.error = Some(error);
    state.stop_at.get_or_insert_with(Instant::now);
}
fn early(shared: &Arc<Mutex<Shared>>, error: RuntimeError, returned: bool) {
    fail(shared, error);
    shared.lock().unwrap_or_else(|e| e.into_inner()).returned = returned;
}
fn run<E>(prepared: PreparedLocal, executor: E, shared: Arc<Mutex<Shared>>)
where
    E: HttpExecutor + 'static,
    E::Ticket: 'static,
{
    let (mut uploads, hub) = match UploadHubOwner::new() {
        Ok(value) => value,
        Err(_) => {
            early(&shared, RuntimeError::Busy, true);
            return;
        }
    };
    let (mut downloads, download) = match DownloadHubOwner::new() {
        Ok(value) => value,
        Err(_) => {
            hub.request_domain_stop();
            let done = uploads.confirm_live_drained();
            drop(uploads);
            early(&shared, RuntimeError::Busy, done);
            return;
        }
    };
    let host = match TlsHttpHost::open_local_and_lan(&prepared.configuration, None) {
        Ok(host) => host,
        Err(error) => {
            hub.request_domain_stop();
            download.request_domain_stop();
            let done = uploads.confirm_live_drained() && downloads.confirm_live_drained();
            drop(uploads);
            drop(downloads);
            early(
                &shared,
                error.into(),
                done && error != ConfiguredTlsError::Cleanup,
            );
            return;
        }
    };
    let mut net = NetCoordinator::new_unattached(host, executor, hub.clone(), download.clone());
    if net.set_local_assets(prepared.assets.clone()).is_err() {
        fail(&shared, RuntimeError::StaticAssets);
    }
    {
        let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
        state.status.generation = net.generation();
        if state.status.error.is_none() {
            state.status.phase = RuntimePhase::Running;
            crate::local_management::publish(prepared.port);
        }
    }
    let mut attachment: Option<Attachment> = None;
    let mut lan_owner: Option<(Arc<WebApplication>, Arc<StaticAssets>)> = None;
    let mut retired_lan = Vec::new();
    let mut changing: Option<(LanCommand, Instant, Option<RuntimeError>)> = None;
    let mut stopping = false;
    let mut cleanup = None;
    let mut cleaned = false;
    let mut business_draining = false;
    let discovery_control = LocalControlHandle {
        shared: shared.clone(),
        certificate: prepared.configuration.certificate,
        port: prepared.port,
        assets: prepared.assets.clone(),
    };
    let mut discovery = None;
    let mut discovery_attempted = false;
    let mut browser = None;
    let mut discovery_poll_at = Instant::now();
    let mut update_discovery_at = Instant::now();
    loop {
        if !stopping {
            let pending_updates = shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending_updates
                .take();
            if let Some(control) = pending_updates {
                if net.set_update_network(control).is_err() {
                    fail(&shared, RuntimeError::Busy);
                }
            }

            let repair = shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending_repair
                .take();
            if let Some(repair) = repair {
                if net.attach_repair(repair).is_ok() {
                    shared.lock().unwrap_or_else(|e| e.into_inner()).attached = true;
                } else {
                    fail(&shared, RuntimeError::BackendUnavailable);
                }
            }
            let pending = shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending
                .take();
            if let Some(pending) = pending {
                match net.attach_backend(&pending.backend, &pending.app) {
                    Ok(()) => {
                        pending.app.binding().set_local_control_ready(true);
                        shared.lock().unwrap_or_else(|e| e.into_inner()).service =
                            Some(pending.app.binding());
                        attachment = Some(pending);
                        shared.lock().unwrap_or_else(|e| e.into_inner()).attached = true;
                    }
                    Err(_) => {
                        fail(&shared, RuntimeError::BackendUnavailable);
                    }
                }
            }
        }
        let negative_lan_stop = {
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            let requested = std::mem::take(&mut state.negative_lan_stop);
            if requested {
                for command in state.lan_commands.drain(..) {
                    let _ = command.reply.send(Err(RuntimeError::Cancelled));
                }
            }
            requested
        };
        if negative_lan_stop {
            if let Some((command, _, _)) = changing.take() {
                let _ = command.reply.send(Err(RuntimeError::Cancelled));
            }
            if let Some(previous) = lan_owner.take() {
                retired_lan.push(previous);
            }
            if net.configure_lan(None, None, None).is_err() {
                fail(&shared, RuntimeError::Cleanup);
            }
            shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .lan_changing = false;
        }
        if !stopping && changing.is_none() {
            let command = {
                let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
                let command = state.lan_commands.pop_front();
                state.lan_changing = command.is_some();
                command
            };
            if let Some(command) = command {
                let result = if !(command.active)() {
                    Err(RuntimeError::Cancelled)
                } else if retired_lan.len() >= 2 {
                    Err(RuntimeError::Busy)
                } else {
                    if let Some(attached) = &attachment {
                        let _ = attached.app.binding().set_lan_ready(false);
                    }
                    net.configure_lan(
                        command.configuration.as_ref(),
                        command.app.as_ref(),
                        command.assets.clone(),
                    )
                    .map_err(|_| RuntimeError::Native)
                };
                match result {
                    Ok(()) => {
                        if let Some(previous) = lan_owner.take() {
                            retired_lan.push(previous);
                        }
                        lan_owner = command.app.clone().zip(command.assets.clone());
                        changing = Some((command, Instant::now(), None));
                    }
                    Err(error) => {
                        let _ = command.reply.send(Err(error));
                        let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
                        state.lan_changing = false;
                        state.lan_status = LanStatus {
                            phase: net.lan_phase(),
                            error: Some(error),
                        };
                    }
                }
            }
        }
        let requested = shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .business_drain_requested;
        if requested && !business_draining {
            business_draining = true;
            hub.request_domain_stop();
            download.request_domain_stop();
        }
        let stop_at = shared.lock().unwrap_or_else(|e| e.into_inner()).stop_at;
        if stop_at.is_some() && !stopping {
            stopping = true;
            if let Some((command, _, _)) = changing.take() {
                let _ = command.reply.send(Err(RuntimeError::Cancelled));
            }
            {
                let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
                for command in state.lan_commands.drain(..) {
                    let _ = command.reply.send(Err(RuntimeError::Cancelled));
                }
                state.pending.take();
            }
            crate::local_management::publish(0);
            if let Some(attached) = &attachment {
                attached.app.binding().set_local_control_ready(false);
            }
            hub.request_domain_stop();
            download.request_domain_stop();
            if net.request_stop().is_err() {
                fail(&shared, RuntimeError::Cleanup);
            }
        }
        if net.poll().is_err() {
            fail(&shared, RuntimeError::Native);
        }
        if let Some((command, started, mut error)) = changing.take() {
            if error.is_none() {
                error = if !(command.active)() {
                    Some(RuntimeError::Cancelled)
                } else if Instant::now() >= started + STOP_BUDGET {
                    Some(RuntimeError::StopTimeout)
                } else {
                    net.lan_error().map(RuntimeError::from)
                };
                if error.is_some() && net.configure_lan(None, None, None).is_err() {
                    fail(&shared, RuntimeError::Cleanup);
                }
            }
            let phase = net.lan_phase();
            let complete = if error.is_some() || command.configuration.is_none() {
                phase == LanListenerPhase::Disabled
            } else {
                phase == LanListenerPhase::Listening && net.lan_error().is_none()
            };
            {
                let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
                state.lan_status = LanStatus { phase, error };
                state.lan_changing = !complete;
                if complete {
                    state.lan_configuration = if error.is_none() {
                        command.configuration.clone()
                    } else {
                        None
                    };
                }
            }
            if complete {
                if let Some(attached) = &attachment {
                    let _ = attached
                        .app
                        .binding()
                        .set_lan_ready(error.is_none() && command.configuration.is_some());
                }
                let _ = command.reply.send(error.map_or(Ok(()), Err));
            } else {
                changing = Some((command, started, error));
            }
        }
        let report = net.report();
        if report.pending_jobs == 0 && report.cleanup_pending == 0 && report.retained_cleanup == 0 {
            retired_lan.clear();
        }
        {
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            state.lan_status.phase = net.lan_phase();
            state.lan_quiescent = net.lan_phase() == LanListenerPhase::Disabled
                && changing.is_none()
                && state.lan_commands.is_empty()
                && retired_lan.is_empty();
            state.report = Some(report);
        }
        if !stopping && !business_draining && Instant::now() >= discovery_poll_at {
            discovery_poll_at = Instant::now() + Duration::from_millis(100);
            if let Some(address) = discovery_control.quick_connect_url() {
                if !discovery_attempted {
                    discovery_attempted = true;
                    if discovery.is_none() {
                        discovery = fc_windows::lan_discovery::DiscoveryResponder::bind().ok();
                    }
                    let host = shared
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .lan_configuration
                        .as_ref()
                        .and_then(|config| config.host.as_str().parse::<std::net::Ipv4Addr>().ok());
                    browser = host.and_then(|host| {
                        fc_windows::lan_discovery::DiscoveryBrowser::bind(host).ok()
                    });
                    shared
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .discovery_status = if discovery.is_some() {
                        "UDP 38443 已启用"
                    } else {
                        "UDP 38443 不可用，仍可扫码连接"
                    };
                }
                let available = browser
                    .as_mut()
                    .is_some_and(|browser| browser.poll().is_ok());
                let mut rooms = browser
                    .as_ref()
                    .map_or_else(Vec::new, |browser| browser.rooms());
                if let Some(attached) = &attachment {
                    if let Ok(current) = attached.app.binding().current_classroom(&address) {
                        rooms.retain(|room| room.address != current.address);
                        rooms.insert(0, current);
                        rooms.truncate(64);
                    }
                }
                shared.lock().unwrap_or_else(|e| e.into_inner()).classrooms =
                    fc_protocol::web::responses::Classrooms {
                        schema_version: 1,
                        discovery_available: available,
                        rooms,
                    };
                let configuration = shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .lan_configuration
                    .clone();
                if let (Some(responder), Some(configuration), Some(attached)) =
                    (&mut discovery, configuration, &attachment)
                {
                    {
                        let payload = attached.app.binding().lan_discovery_payload(&address).ok();
                        let update_control = shared
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .update_control
                            .clone();
                        let _ = responder.poll_services_filtered(
                            payload.as_deref(),
                            |peer| {
                                configuration
                                    .source_networks
                                    .as_ref()
                                    .is_some_and(|policy| {
                                        policy.contains(peer)
                                            && !policy.networks().iter().any(|network| {
                                                let Some((address, prefix)) =
                                                    network.as_str().split_once('/')
                                                else {
                                                    return false;
                                                };
                                                match (
                                                    address.parse::<std::net::Ipv4Addr>(),
                                                    prefix.parse::<u32>(),
                                                    peer,
                                                ) {
                                                    (
                                                        Ok(network),
                                                        Ok(prefix @ 1..=30),
                                                        std::net::IpAddr::V4(peer),
                                                    ) => {
                                                        u32::from(peer)
                                                            == u32::from(network)
                                                                | (u32::MAX >> prefix)
                                                    }
                                                    _ => false,
                                                }
                                            })
                                    })
                            },
                            |peer| {
                                update_control
                                    .as_ref()
                                    .is_some_and(|u| u.discovery_enabled())
                                    && matches!(peer,std::net::IpAddr::V4(ip) if ip.is_private())
                            },
                            |bytes, source| {
                                update_control
                                    .as_ref()
                                    .and_then(|u| u.discovery_message(bytes, source))
                            },
                        );
                    }
                }
            } else {
                if !shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .update_control
                    .as_ref()
                    .is_some_and(|u| u.discovery_enabled())
                {
                    discovery = None;
                }
                browser = None;
                discovery_attempted = false;
                shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .discovery_status = "未启动";
                shared.lock().unwrap_or_else(|e| e.into_inner()).classrooms =
                    fc_protocol::web::responses::Classrooms {
                        schema_version: 1,
                        discovery_available: false,
                        rooms: Vec::new(),
                    };
            }
        }
        if !stopping && !business_draining && Instant::now() >= update_discovery_at {
            update_discovery_at = Instant::now() + Duration::from_millis(100);
            net.update_discovery(&mut discovery);
            if discovery_control.quick_connect_url().is_none() {
                let control = shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .update_control
                    .clone();
                if let (Some(responder), Some(control)) = (&mut discovery, control) {
                    let enabled = control.discovery_enabled();
                    let _ = responder.poll_services_filtered(
                        None,
                        |_| false,
                        |peer| {
                            enabled && matches!(peer,std::net::IpAddr::V4(ip) if ip.is_private())
                        },
                        |bytes, source| control.discovery_message(bytes, source),
                    );
                }
            }
        }
        if stopping || business_draining {
            if cleanup.is_none() && !cleaned {
                if let Some(attached) = &attachment {
                    cleanup = Some(CleanupWork::Pending(HttpJob::Cleanup(Box::new(
                        crate::web_transfer::live_cleanup::CleanupTask::new(
                            hub.clone(),
                            download.clone(),
                            attached.app.clone(),
                            Instant::now() + Duration::from_millis(250),
                        ),
                    ))));
                } else {
                    cleaned = true;
                }
            }
            if let Some(work) = cleanup.take() {
                match poll_cleanup(work, net.executor()) {
                    Ok((next, done)) => {
                        cleanup = next;
                        cleaned = done
                    }
                    Err(error) => fail(&shared, error),
                }
            }
            let previously_confirmed = shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .business_quiescent;
            if !previously_confirmed
                && cleaned
                && (report.pending_jobs != 0
                    || report.cleanup_pending != 0
                    || report.retained_cleanup != 0)
            {
                cleaned = false;
            }
            let business_done = net.updates_idle()
                && cleaned
                && cleanup.is_none()
                && report.pending_jobs == 0
                && report.cleanup_pending == 0
                && report.retained_cleanup == 0
                && uploads.confirm_live_drained()
                && downloads.confirm_live_drained();
            if business_done {
                shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .business_quiescent = true;
            }
            // Hub admission is permanently closed once draining starts. Later
            // memory-only control/static traffic cannot recreate a file owner,
            // so a confirmed cleanup is reused after the shared pool stops.
            if stopping && net.is_idle() && business_done {
                break;
            }
            if stopping && stop_at.is_some_and(|at| Instant::now() >= at + STOP_BUDGET) {
                fail(&shared, RuntimeError::StopTimeout);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(net);
    drop(uploads);
    drop(downloads);
    drop(lan_owner);
    drop(retired_lan);
    drop(attachment);
    drop(prepared);
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    if state
        .stop_at
        .is_some_and(|at| Instant::now() >= at + STOP_BUDGET)
    {
        state.status.phase = RuntimePhase::Faulted;
        state.status.error = Some(RuntimeError::StopTimeout);
    }
    state.returned = true;
}
