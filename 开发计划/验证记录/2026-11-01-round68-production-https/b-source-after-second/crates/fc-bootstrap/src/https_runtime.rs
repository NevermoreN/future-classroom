//! The ordinary HTTPS domain: one HTTP thread owns the original native TLS/Net
//! domain, while all business/file work uses the application's existing pool.
//! UI methods only signal/poll fixed metadata. No helper starts another pool.
use crate::{
    backend::{
        web::{WebRuntimeReservation, WebService},
        Backend, SessionLease,
    },
    web_application::WebApplication,
    web_assets::StaticAssets,
    web_transfer::network::{NetCoordinator, NetworkReport},
    web_transfer::{
        DownloadHubOwner, HttpCompletion, HttpExecutor, HttpJob, HttpTicket, UploadHubOwner,
    },
};
use fc_domain::https_settings::HttpsConfiguration;
use fc_ports::http::ListenPrefix;
use fc_runtime::worker::{JobContext, Rejection};
use fc_windows::tls_frontdoor::{ConfiguredTlsError, TlsHttpHost};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub const STOP_BUDGET: Duration = Duration::from_secs(5);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    Busy,
    Cancelled,
    MissingConfiguration,
    MissingSourceNetworks,
    InvalidConfiguration,
    StaticAssets,
    BackendUnavailable,
    AddressUnavailable,
    AmbiguousAddress,
    UnsupportedAddress,
    Certificate,
    Native,
    Cleanup,
    Faulted,
    StopTimeout,
}
impl RuntimeError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Busy => "网络服务正在处理其他操作，请稍后重试",
            Self::Cancelled => "操作已取消",
            Self::MissingConfiguration => "请先保存 HTTPS 地址、端口和证书",
            Self::MissingSourceNetworks => "请先明确填写允许访问的网络范围",
            Self::InvalidConfiguration => "HTTPS 配置无效",
            Self::StaticAssets => "网页资源缺失或校验失败",
            Self::BackendUnavailable => "当前无法核对设置与管理权限",
            Self::AddressUnavailable => "配置的地址暂时不可用",
            Self::AmbiguousAddress => "域名对应多个地址，请明确配置服务地址",
            Self::UnsupportedAddress => "配置的地址暂不受支持",
            Self::Certificate => "证书无法用于本次 HTTPS 服务",
            Self::Native => "无法建立 HTTPS 监听，请检查地址、端口和证书",
            Self::Cleanup => "网络资源尚未完成清理",
            Self::Faulted => "网络服务出现错误，尚不能重新启动",
            Self::StopTimeout => "网络服务未在预定时间内完成停止，仍在清理原资源",
        }
    }
}
impl From<ConfiguredTlsError> for RuntimeError {
    fn from(e: ConfiguredTlsError) -> Self {
        match e {
            ConfiguredTlsError::InvalidConfiguration => Self::InvalidConfiguration,
            ConfiguredTlsError::MissingSourceNetworks => Self::MissingSourceNetworks,
            ConfiguredTlsError::UnsupportedAddress => Self::UnsupportedAddress,
            ConfiguredTlsError::AmbiguousAddress => Self::AmbiguousAddress,
            ConfiguredTlsError::AddressUnavailable => Self::AddressUnavailable,
            ConfiguredTlsError::Certificate => Self::Certificate,
            ConfiguredTlsError::Busy => Self::Busy,
            ConfiguredTlsError::Native => Self::Native,
            ConfiguredTlsError::Cleanup => Self::Cleanup,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePhase {
    Disabled,
    Starting,
    Running,
    Stopping,
    Faulted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub phase: RuntimePhase,
    pub generation: u64,
    pub error: Option<RuntimeError>,
}
#[derive(Clone)]
pub struct StartRequest {
    lease: SessionLease,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}
impl StartRequest {
    pub fn new(lease: SessionLease, deadline: Instant) -> Self {
        Self {
            lease,
            deadline: deadline.min(Instant::now() + Duration::from_secs(90)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + std::mem::size_of::<AtomicBool>()
            + 2 * std::mem::size_of::<usize>()
    }
    fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire) && Instant::now() < self.deadline
    }
}
pub struct StartPlan {
    backend: Arc<Mutex<Backend>>,
    app: Arc<WebApplication>,
    assets: Arc<StaticAssets>,
    service: WebService,
    configuration: HttpsConfiguration,
    request: StartRequest,
    reservation: Arc<WebRuntimeReservation>,
}
impl StartPlan {
    /// Includes the immutable package payload once; the worker input descriptor
    /// separately charges StartRequest/path metadata. Package load has its own
    /// fixed5MiB/MAX64 limit rather than silently expanding the pool's4MiB queue.
    pub fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.assets.allocation_bytes()
            + self.configuration.allocation_bytes()
            + self.request.allocation_bytes()
    }
}
/// Original shared worker only. The package path is derived from the running
/// installed executable, never a form, URL, current directory or temp fallback.
pub fn prepare_start(
    backend: Arc<Mutex<Backend>>,
    request: StartRequest,
    context: &JobContext,
) -> Result<StartPlan, RuntimeError> {
    let exe = std::env::current_exe().map_err(|_| RuntimeError::StaticAssets)?;
    let root = exe.parent().ok_or(RuntimeError::StaticAssets)?.join("web");
    prepare_start_from_package(backend, request, &root, context)
}
/// Trusted finite probe composition only may supply its separately verified
/// owned package. Ordinary product callers use prepare_start, not a form path.
pub fn prepare_start_from_package(
    backend: Arc<Mutex<Backend>>,
    request: StartRequest,
    package_root: &Path,
    context: &JobContext,
) -> Result<StartPlan, RuntimeError> {
    context.checkpoint().map_err(|_| RuntimeError::Cancelled)?;
    if !request.current() {
        return Err(RuntimeError::Cancelled);
    }
    let (service, voice, reservation, state, data_root) = {
        let mut locked = backend.try_lock().map_err(|_| RuntimeError::Busy)?;
        let service = locked.web_service();
        let voice = locked.voice_service();
        let reservation = service
            .reserve_runtime(request.lease)
            .map_err(|_| RuntimeError::Busy)?;
        let data_root = locked
            .https_data_root()
            .map_err(|_| RuntimeError::BackendUnavailable)?;
        let stable = request.clone();
        let job = context.execution_fence();
        let scope = locked.begin_execution_scope(Arc::new(move || stable.current() && job()));
        let state = scope
            .read_https_settings(request.lease)
            .map_err(|_| RuntimeError::BackendUnavailable)?;
        if state.identity_revision != reservation.identity_revision() {
            return Err(RuntimeError::Cancelled);
        }
        (service, voice, reservation, state, data_root)
    }; // Backend/ledger/writer locks are all gone before filesystem/native IO.
    let configuration = state
        .settings
        .configuration
        .ok_or(RuntimeError::MissingConfiguration)?;
    configuration
        .validate()
        .map_err(|_| RuntimeError::InvalidConfiguration)?;
    if configuration.source_networks.is_none() {
        return Err(RuntimeError::MissingSourceNetworks);
    }
    let prefix = ListenPrefix::https(&format!("{}/fc/", configuration.origin()))
        .map_err(|_| RuntimeError::InvalidConfiguration)?;
    let assets = Arc::new(
        StaticAssets::load_in_job(&prefix, package_root, context)
            .map_err(|_| RuntimeError::StaticAssets)?,
    );
    context.checkpoint().map_err(|_| RuntimeError::Cancelled)?;
    if !request.current() || !reservation.current() {
        return Err(RuntimeError::Cancelled);
    }
    let mut application = WebApplication::new(&prefix, service.clone())
        .map_err(|_| RuntimeError::InvalidConfiguration)?;
    application
        .configure_material_files(&data_root)
        .map_err(|_| RuntimeError::BackendUnavailable)?;
    application
        .configure_voice_files_from_services(&service, voice, &data_root)
        .map_err(|_| RuntimeError::BackendUnavailable)?;
    context.checkpoint().map_err(|_| RuntimeError::Cancelled)?;
    if !request.current() || !reservation.current() {
        return Err(RuntimeError::Cancelled);
    }
    Ok(StartPlan {
        backend,
        app: Arc::new(application),
        assets,
        service,
        configuration,
        request,
        reservation,
    })
}
struct Shared {
    status: RuntimeStatus,
    stop_at: Option<Instant>,
    report: Option<NetworkReport>,
    returned: bool,
    faulted: bool,
    retain_claim: bool,
}
pub struct RuntimeOwner {
    shared: Arc<Mutex<Shared>>,
    thread: Option<JoinHandle<()>>,
    service: WebService,
    request: StartRequest,
    reservation: Arc<WebRuntimeReservation>,
    quiescent: bool,
}
fn stop(shared: &Arc<Mutex<Shared>>, service: &WebService, at: Instant) {
    service.negative_runtime_stop();
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    state.stop_at = Some(state.stop_at.map_or(at, |original| original.min(at)));
    if !state.faulted {
        state.status.phase = RuntimePhase::Stopping;
    }
}
fn fault(shared: &Arc<Mutex<Shared>>, error: RuntimeError) {
    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
    s.faulted = true;
    s.retain_claim |= matches!(
        error,
        RuntimeError::StopTimeout
            | RuntimeError::Cleanup
            | RuntimeError::Native
            | RuntimeError::Faulted
    );
    s.status.phase = RuntimePhase::Faulted;
    s.status.error = Some(error);
}
impl RuntimeOwner {
    pub fn start<E>(plan: StartPlan, executor: E) -> Result<Self, RuntimeError>
    where
        E: HttpExecutor + Send + 'static,
        E::Ticket: Send + 'static,
    {
        if !plan.request.current() || !plan.reservation.current() {
            return Err(RuntimeError::Cancelled);
        }
        let shared = Arc::new(Mutex::new(Shared {
            status: RuntimeStatus {
                phase: RuntimePhase::Starting,
                generation: 0,
                error: None,
            },
            stop_at: None,
            report: None,
            returned: false,
            faulted: false,
            retain_claim: false,
        }));
        let state = shared.clone();
        let service = plan.service.clone();
        let request = plan.request.clone();
        let reservation = plan.reservation.clone();
        let thread = std::thread::Builder::new()
            .name("fc-https".into())
            .spawn(move || run(plan, executor, state))
            .map_err(|_| RuntimeError::Native)?;
        Ok(Self {
            shared,
            thread: Some(thread),
            service,
            request,
            reservation,
            quiescent: false,
        })
    }
    pub fn request_stop(&self) {
        self.request_stop_at(Instant::now());
    }
    /// The UI/coordinator passes its original stop observation, before any
    /// cancellation work. Repeated stop requests can only tighten that origin.
    pub fn request_stop_at(&self, at: Instant) {
        let at = at.min(Instant::now());
        self.request.cancel();
        stop(&self.shared, &self.service, at);
    }
    pub fn network_report(&self) -> Option<NetworkReport> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).report
    }
    pub fn is_quiescent(&self) -> bool {
        self.quiescent
    }
    /// Check the original Windows thread HANDLE with zero timeout; Rust's
    /// JoinHandle::is_finished alone does not prove TLS destructors returned.
    pub fn poll(&mut self) -> RuntimeStatus {
        if self
            .thread
            .as_ref()
            .is_some_and(crate::physical_thread::is_finished)
        {
            let joined = self
                .thread
                .take()
                .expect("observed original HTTP thread")
                .join();
            let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            self.quiescent = s.returned && joined.is_ok();
            if self.quiescent && !s.retain_claim {
                self.reservation.physical_returned();
            }
            if self.quiescent && !s.faulted {
                s.status.phase = RuntimePhase::Disabled;
            } else if !self.quiescent {
                s.faulted = true;
                s.status.phase = RuntimePhase::Faulted;
                s.status.error = Some(RuntimeError::Faulted);
            }
        }
        let mut state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if !self.quiescent
            && state
                .stop_at
                .is_some_and(|at| Instant::now() >= at + STOP_BUDGET)
        {
            state.faulted = true;
            state.retain_claim = true;
            state.status.phase = RuntimePhase::Faulted;
            state.status.error = Some(RuntimeError::StopTimeout);
        }
        if state.stop_at.is_some() && !state.faulted && !self.quiescent {
            state.status.phase = RuntimePhase::Stopping;
        }
        state.status
    }
}
impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        if self.quiescent {
            return;
        }
        self.request_stop();
        // Dropping a live owner must not detach its original physical thread.
        // The bounded singleton is retained for the final coordinator/recovery.
        if self.thread.is_some() {
            let retained = Retained {
                thread: self.thread.take(),
                shared: self.shared.clone(),
                service: self.service.clone(),
                request: self.request.clone(),
                reservation: self.reservation.clone(),
            };
            let mut slot = RETAINED.lock().unwrap_or_else(|e| e.into_inner());
            assert!(slot.is_none(), "one original retained HTTPS domain");
            *slot = Some(retained);
        }
    }
}
struct Retained {
    thread: Option<JoinHandle<()>>,
    shared: Arc<Mutex<Shared>>,
    service: WebService,
    request: StartRequest,
    reservation: Arc<WebRuntimeReservation>,
}
static RETAINED: Mutex<Option<Retained>> = Mutex::new(None);
impl RuntimeOwner {
    pub fn recover_retained() -> Option<Self> {
        RETAINED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|r| Self {
                shared: r.shared,
                thread: r.thread,
                service: r.service,
                request: r.request,
                reservation: r.reservation,
                quiescent: false,
            })
    }
}
fn run<E>(plan: StartPlan, executor: E, shared: Arc<Mutex<Shared>>)
where
    E: HttpExecutor + 'static,
    E::Ticket: 'static,
{
    plan.reservation.mark_started();
    if !plan.request.current() || !plan.reservation.current() {
        early_return(&shared, RuntimeError::Cancelled, true);
        return;
    }
    let (mut uploads, hub) = match UploadHubOwner::new() {
        Ok(v) => v,
        Err(_) => {
            early_return(&shared, RuntimeError::Busy, true);
            return;
        }
    };
    let (mut downloads, download) = match DownloadHubOwner::new() {
        Ok(v) => v,
        Err(_) => {
            hub.request_domain_stop();
            let _ = uploads.confirm_live_drained();
            drop(uploads);
            early_return(&shared, RuntimeError::Busy, true);
            return;
        }
    };
    let host = match TlsHttpHost::open_configured(&plan.configuration) {
        Ok(host) => host,
        Err(e) => {
            hub.request_domain_stop();
            download.request_domain_stop();
            let _ = uploads.confirm_live_drained();
            let _ = downloads.confirm_live_drained();
            drop(uploads);
            drop(downloads);
            early_return(&shared, e.into(), e != ConfiguredTlsError::Cleanup);
            return;
        }
    };
    let mut net = NetCoordinator::new(
        host,
        executor,
        &plan.backend,
        &plan.app,
        hub.clone(),
        download.clone(),
    );
    let mut error = net
        .set_execution_fence(plan.reservation.execution_fence())
        .err()
        .map(|_| RuntimeError::Faulted);
    let assets_error = net
        .set_static_assets(plan.assets.clone())
        .err()
        .map(|_| RuntimeError::StaticAssets);
    error = error.or(assets_error);
    {
        shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .generation = net.generation();
    }
    if error.is_none() && plan.request.current() && plan.reservation.current() {
        if plan.reservation.activate(net.generation()).is_err() {
            error = Some(RuntimeError::Cancelled);
        }
    } else if error.is_none() {
        error = Some(RuntimeError::Cancelled);
    }
    if let Some(e) = error {
        fault(&shared, e);
        stop(&shared, &plan.service, Instant::now());
    } else {
        shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .phase = RuntimePhase::Running;
    }
    let mut stopping = false;
    let mut sessions_settled = false;
    let mut cleanup: Option<CleanupWork<E::Ticket>> = None;
    let mut cleaned = false;
    loop {
        let stop_at = shared.lock().unwrap_or_else(|e| e.into_inner()).stop_at;
        if !stopping && (stop_at.is_some() || !plan.service.runtime_domain_current()) {
            stop(&shared, &plan.service, Instant::now());
            stopping = true;
            hub.request_domain_stop();
            download.request_domain_stop();
            if net.request_stop().is_err() {
                fault(&shared, RuntimeError::Cleanup);
            }
        }
        if net.poll().is_err() {
            fault(&shared, RuntimeError::Native);
            stop(&shared, &plan.service, Instant::now());
        }
        {
            shared.lock().unwrap_or_else(|e| e.into_inner()).report = Some(net.report());
        }
        if stopping {
            if !sessions_settled {
                sessions_settled = plan.service.settle_runtime_stop();
            }
            let original = shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .stop_at
                .expect("original stop origin");
            if Instant::now() >= original + STOP_BUDGET {
                fault(&shared, RuntimeError::StopTimeout);
            }
            if cleanup.is_none() && !cleaned {
                // After a missed stop deadline only negative cleanup continues;
                // the original status remains Faulted and no replacement starts.
                cleanup = Some(CleanupWork::Pending(HttpJob::Cleanup(Box::new(
                    crate::web_transfer::live_cleanup::CleanupTask::new(
                        hub.clone(),
                        download.clone(),
                        plan.app.clone(),
                        Instant::now() + Duration::from_millis(250),
                    ),
                ))));
            }
            if let Some(work) = cleanup.take() {
                match poll_cleanup(work, net.executor()) {
                    Ok((next, done)) => {
                        cleanup = next;
                        cleaned = done;
                    }
                    Err(_) => {
                        fault(&shared, RuntimeError::Cleanup);
                    }
                }
            }
            if net.is_idle()
                && sessions_settled
                && cleaned
                && cleanup.is_none()
                && uploads.confirm_live_drained()
                && downloads.confirm_live_drained()
            {
                break;
            }
            if cleaned && !net.is_idle() {
                cleaned = false;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // Both Hub registries have no Busy/Owned lease, quarantine is empty, and
    // all original Net tickets/native IO returned. Native SSPI Drop/join is
    // completed on this HTTP thread before publishing returned.
    drop(net);
    drop(uploads);
    drop(downloads);
    drop(plan);
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    // Native/SSPI destruction is part of this same original budget too.
    if state
        .stop_at
        .is_some_and(|at| Instant::now() >= at + STOP_BUDGET)
    {
        state.faulted = true;
        state.retain_claim = true;
        state.status.phase = RuntimePhase::Faulted;
        state.status.error = Some(RuntimeError::StopTimeout);
    }
    state.returned = true;
}
fn early_return(shared: &Arc<Mutex<Shared>>, error: RuntimeError, physical: bool) {
    fault(shared, error);
    shared.lock().unwrap_or_else(|e| e.into_inner()).returned = physical;
}
enum CleanupWork<T> {
    Pending(HttpJob),
    Running(T),
}
fn poll_cleanup<E: HttpExecutor>(
    work: CleanupWork<E::Ticket>,
    executor: &E,
) -> Result<(Option<CleanupWork<E::Ticket>>, bool), RuntimeError> {
    match work {
        CleanupWork::Pending(job) => match executor.try_submit_http(job) {
            Ok(ticket) => Ok((Some(CleanupWork::Running(ticket)), false)),
            Err(e) if e.kind == Rejection::Busy => Ok((Some(CleanupWork::Pending(e.task)), false)),
            Err(_) => Ok((None, false)),
        },
        CleanupWork::Running(mut ticket) => {
            if !ticket.is_finished() {
                return Ok((Some(CleanupWork::Running(ticket)), false));
            }
            match ticket.try_result() {
                Ok(Some(HttpCompletion::Cleanup(status))) => Ok((None, status.is_drained())),
                Ok(None) => Ok((Some(CleanupWork::Running(ticket)), false)),
                _ => Err(RuntimeError::Cleanup),
            }
        }
    }
}
