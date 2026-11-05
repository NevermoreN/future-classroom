//! Synchronous authenticated Web application dispatch. Invoke only on the shared
//! bounded application worker, never the native HTTP/IOCP or UI event thread.
//! Listener/TLS provisioning and browser delivery remain separate adapters.
use crate::backend::{
    web::{
        cookie_secret, session_cookie, AuthenticatedWeb, PairingPoll, WebService, WebServiceError,
        CLEAR_SESSION_COOKIE,
    },
    Backend,
};
use fc_ports::http::{Header, ListenPrefix, RequestHead, ResponseHead};
use fc_protocol::{
    web::{pairing, responses, Route, WebBoundary, WebError},
    ProtocolError,
};
use fc_security::web::{page::PageToken, PairingCode, WebSecret};

mod account_security;
pub mod admin;
pub(crate) mod admin_platform;
#[cfg(windows)]
pub(crate) mod admin_preview;
pub(crate) mod admin_voice;
mod content;
#[cfg(windows)]
pub mod maintenance;
#[cfg(windows)]
mod material_files;
mod materials;
#[cfg(windows)]
mod profile_images;
#[cfg(windows)]
mod shared_upload;
mod voice;
mod voice_capabilities;
#[cfg(windows)]
mod voice_files;
#[cfg(windows)]
pub use material_files::{
    MaterialDownload, MaterialTransferBinding, PreparedWebMaterial, ReceivingMaterialBatchUpload,
    ReceivingMaterialUpload,
};
#[cfg(all(windows, test))]
pub(crate) use shared_upload::test_guard as upload_test_guard;
#[cfg(windows)]
pub use shared_upload::{quarantined_uploads, retry_quarantined_uploads, UploadCleanupStatus};
#[cfg(windows)]
pub use voice_files::{PreparedWebVoiceUpload, ReceivingWebVoiceUpload};

/// Contains explicitly disclosed session headers/body. Deliberately no Debug or
/// Clone: transport ownership must not turn credentials into routine diagnostics.
pub struct WebResponse {
    pub head: ResponseHead,
    pub body: Vec<u8>,
}
pub struct WebApplication {
    #[cfg(windows)]
    profile_images:
        std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<profile_images::ImageFiles>>>>,
    boundary: std::sync::Arc<WebBoundary>,
    service: WebService,
    account_receipts: std::sync::Arc<std::sync::Mutex<Vec<account_security::Receipt>>>,
    local_control: bool,
    quick_connect_url: Option<std::sync::Arc<dyn Fn() -> Option<String> + Send + Sync>>,
    classrooms: Option<std::sync::Arc<dyn Fn() -> responses::Classrooms + Send + Sync>>,
    admin_platform: Option<admin::PlatformHook>,
    #[cfg(windows)]
    pub(crate) maintenance: Option<std::sync::Arc<crate::maintenance_web::MaintenanceService>>,
    #[cfg(windows)]
    upload_binding: std::sync::Arc<()>,
    #[cfg(windows)]
    material_files: std::sync::Arc<std::sync::Mutex<Option<material_files::MaterialFiles>>>,
    #[cfg(windows)]
    voice_files: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<voice_files::VoiceFiles>>>>,
}
impl WebApplication {
    fn page_session_response(&self, proof: &PageToken) -> WebResponse {
        let session = match self.service.authenticate_page(proof) {
            Ok(session) => session,
            Err(error) => return authentication_error(error),
        };
        let token = proof.expose_hex();
        response(
            200,
            responses::page_session(
                &session.account,
                session.current_class,
                &session.assignments,
                session.csrf.as_str(),
                Some(token.as_str()),
            ),
            None,
        )
    }
    fn request_boundary_error(&self, head: &RequestHead, error: WebError) -> WebResponse {
        if error == WebError::Method {
            // Reuse strict target recognition only after the original boundary
            // has rejected the method. This lookup grants no authorization.
            return match self.boundary.allowed_methods(&head.target) {
                Ok(methods) => response(
                    405,
                    responses::error(responses::ErrorCode::MethodNotAllowed),
                    Some(Header {
                        name: "Allow".into(),
                        value: methods.header_value().as_bytes().to_vec(),
                    }),
                ),
                Err(_) => failure(404, responses::ErrorCode::NotFound),
            };
        }
        boundary_error(error)
    }
    #[cfg(windows)]
    pub(crate) fn binding(&self) -> WebService {
        self.service.clone()
    }
    #[cfg(windows)]
    pub(crate) fn upload_instance_key(&self) -> std::sync::Weak<()> {
        std::sync::Arc::downgrade(&self.upload_binding)
    }
    /// Routing/length hint only, never admission or an authorization lease.
    pub(crate) fn route_hint(&self, head: &RequestHead) -> Result<Route, WebError> {
        self.boundary.preflight(head)
    }
    pub fn new(prefix: &ListenPrefix, service: WebService) -> Result<Self, WebError> {
        Ok(Self {
            boundary: std::sync::Arc::new(WebBoundary::new(prefix)?),
            service,
            account_receipts: Default::default(),
            local_control: false,
            quick_connect_url: None,
            classrooms: None,
            #[cfg(windows)]
            profile_images: std::sync::Arc::new(std::sync::Mutex::new(None)),
            admin_platform: None,
            #[cfg(windows)]
            maintenance: None,
            #[cfg(windows)]
            upload_binding: std::sync::Arc::new(()),
            #[cfg(windows)]
            material_files: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(windows)]
            voice_files: std::sync::Arc::new(std::sync::Mutex::new(None)),
        })
    }
    pub fn new_local(prefix: &ListenPrefix, service: WebService) -> Result<Self, WebError> {
        let mut app = Self::new(prefix, service.for_local_control())?;
        app.local_control = true;
        Ok(app)
    }
    #[cfg(windows)]
    pub fn with_maintenance_service(
        mut self,
        service: std::sync::Arc<crate::maintenance_web::MaintenanceService>,
    ) -> Self {
        self.maintenance = Some(service);
        self
    }
    pub fn with_admin_platform_hook(mut self, hook: admin::PlatformHook) -> Self {
        self.admin_platform = Some(hook);
        self
    }
    /// Trusted native composition supplies only the currently confirmed LAN URL.
    /// The provider must be a bounded memory-only snapshot, not a backend call.
    pub fn with_quick_connect_url(
        mut self,
        provider: std::sync::Arc<dyn Fn() -> Option<String> + Send + Sync>,
    ) -> Self {
        self.quick_connect_url = Some(provider);
        self
    }
    pub fn with_classrooms(
        mut self,
        provider: std::sync::Arc<dyn Fn() -> responses::Classrooms + Send + Sync>,
    ) -> Self {
        self.classrooms = Some(provider);
        self
    }
    pub fn local_variant(&self, prefix: &ListenPrefix) -> Result<Self, WebError> {
        let mut app = self.for_execution(std::sync::Arc::new(|| true));
        app.boundary = std::sync::Arc::new(WebBoundary::new(prefix)?);
        app.service = self.service.for_local_control();
        app.local_control = true;
        Ok(app)
    }
    pub fn lan_variant(&self, prefix: &ListenPrefix) -> Result<Self, WebError> {
        let mut app = self.for_execution(std::sync::Arc::new(|| true));
        app.boundary = std::sync::Arc::new(WebBoundary::new(prefix)?);
        app.service = self.service.for_lan();
        app.local_control = false;
        Ok(app)
    }
    /// A short-lived view for one ordinary JSON dispatch. Stream owners use the
    /// original application and a current context on each call; retaining this
    /// job-bound view across jobs would incorrectly cancel their next chunk.
    fn for_execution(&self, fence: crate::web_transfer::ExecutionFence) -> Self {
        Self {
            boundary: self.boundary.clone(),
            service: self.service.with_execution_fence(fence),
            account_receipts: self.account_receipts.clone(),
            local_control: self.local_control,
            quick_connect_url: self.quick_connect_url.clone(),
            classrooms: self.classrooms.clone(),
            #[cfg(windows)]
            profile_images: self.profile_images.clone(),
            admin_platform: self.admin_platform.clone(),
            #[cfg(windows)]
            maintenance: self.maintenance.clone(),
            #[cfg(windows)]
            upload_binding: self.upload_binding.clone(),
            #[cfg(windows)]
            material_files: self.material_files.clone(),
            #[cfg(windows)]
            voice_files: self.voice_files.clone(),
        }
    }
    pub fn dispatch_in_job(
        &self,
        backend: &mut Backend,
        head: &RequestHead,
        body: &[u8],
        life: &crate::web_transfer::TransferLifetime,
        context: &fc_runtime::worker::JobContext,
    ) -> WebResponse {
        if head.token != life.token() || !life.is_active() || context.checkpoint().is_err() {
            return failure(503, responses::ErrorCode::ServiceUnavailable);
        }
        let fence = life.job_fence(context);
        let app = self.for_execution(fence.clone());
        let mut scope = backend.begin_execution_scope(fence.clone());
        let result = app.dispatch(&mut scope, head, body);
        if !fence() {
            // SQL may already have committed. Suppress secrets/data but retain
            // the original idempotent receipt/ResultUnknown reconciliation.
            return failure(503, responses::ErrorCode::ResultUnknown);
        }
        result
    }
    /// Worker-only head admission before a transport consumes any body. The
    /// route is a size/dispatch hint, never an authorization capability: final
    /// dispatch and every stream owner authenticate their exact live session
    /// again. No stage, SQL reservation or pairing mutation is created here.
    pub fn admit_request(
        &self,
        backend: &Backend,
        head: &RequestHead,
    ) -> Result<Route, WebResponse> {
        if !self.service.same_instance(&backend.web_service()) {
            return Err(failure(503, responses::ErrorCode::ServiceUnavailable));
        }
        let route = self
            .boundary
            .preflight(head)
            .map_err(|error| self.request_boundary_error(head, error))?;
        self.authenticate_route(head, route)?;
        #[cfg(windows)]
        if matches!(
            route,
            Route::Maintenance {
                resource: fc_protocol::web::maintenance::Resource::Upload(_)
                    | fc_protocol::web::maintenance::Resource::File(_),
                ..
            }
        ) {
            self.prepare_maintenance_transfer(backend, head)?;
        }
        Ok(route)
    }
    pub fn admit_request_in_job(
        &self,
        backend: &mut Backend,
        head: &RequestHead,
        life: &crate::web_transfer::TransferLifetime,
        context: &fc_runtime::worker::JobContext,
    ) -> Result<Route, WebResponse> {
        if head.token != life.token() || !life.is_active() || context.checkpoint().is_err() {
            return Err(failure(503, responses::ErrorCode::ServiceUnavailable));
        }
        let fence = life.job_fence(context);
        let app = self.for_execution(fence.clone());
        let scope = backend.begin_execution_scope(fence.clone());
        let result = app.admit_request(&scope, head);
        if !fence() {
            return Err(failure(503, responses::ErrorCode::ServiceUnavailable));
        }
        result
    }
    fn authenticate_route(
        &self,
        head: &RequestHead,
        route: Route,
    ) -> Result<Option<AuthenticatedWeb>, WebResponse> {
        let authenticated = if route.requires_session() {
            let mut pages = head
                .headers
                .iter()
                .filter(|h| h.name.eq_ignore_ascii_case("x-fc-page"));
            if let Some(header) = pages.next() {
                if pages.next().is_some() {
                    return Err(boundary_error(WebError::Headers));
                }
                let proof = std::str::from_utf8(&header.value)
                    .ok()
                    .and_then(|value| PageToken::parse_hex(value.to_owned()).ok())
                    .ok_or_else(|| failure(401, responses::ErrorCode::Unauthorized))?;
                Some(
                    self.service
                        .authenticate_page(&proof)
                        .map_err(authentication_error)?,
                )
            } else {
                let mut cookies = head
                    .headers
                    .iter()
                    .filter(|h| h.name.eq_ignore_ascii_case("cookie"));
                let header = match (cookies.next(), cookies.next()) {
                    (Some(header), None) => header,
                    (None, _) => return Err(failure(401, responses::ErrorCode::Unauthorized)),
                    _ => return Err(boundary_error(WebError::Headers)),
                };
                let cookie = cookie_secret(&header.value).map_err(authentication_error)?;
                Some(
                    self.service
                        .authenticate(&cookie)
                        .map_err(authentication_error)?,
                )
            }
        } else {
            None
        };
        self.boundary
            .check(
                head,
                authenticated.as_ref().map(|session| session.csrf.as_str()),
            )
            .map_err(|error| self.request_boundary_error(head, error))?;
        if authenticated
            .as_ref()
            .is_some_and(|session| session.account.must_change_password)
            && !matches!(
                route,
                Route::Session | Route::SessionPassword | Route::SessionLogout
            )
        {
            return Err(failure(403, responses::ErrorCode::Forbidden));
        }
        Ok(authenticated)
    }
    pub fn dispatch(&self, backend: &mut Backend, head: &RequestHead, body: &[u8]) -> WebResponse {
        // A lease from another backend can have identical numeric fields.
        // Bind the trusted composition before authenticating or mutating either.
        if !self.service.same_instance(&backend.web_service()) {
            return failure(503, responses::ErrorCode::ServiceUnavailable);
        }
        let route = match self.boundary.preflight(head) {
            Ok(route) => route,
            Err(error) => return self.request_boundary_error(head, error),
        };
        // Binary bodies are accepted only through the typed stream port; the
        // ordinary JSON dispatcher must never retain a 50MiB request Vec.
        if matches!(
            route,
            Route::Materials {
                route: fc_protocol::web::materials::MaterialRoute::UploadBytes { .. }
                    | fc_protocol::web::materials::MaterialRoute::BatchUploadBytes { .. }
            } | Route::VoiceUpload { .. }
        ) {
            return failure(400, responses::ErrorCode::InvalidRequest);
        }
        // The transport's complete body must agree with both its parsed metadata
        // and the application route budget before any pairing/session mutation.
        if body.len() as u64 != head.content_length.unwrap_or(0)
            || body.len() as u64 > route.body_limit()
        {
            return boundary_error(WebError::Framing);
        }
        let authenticated = match self.authenticate_route(head, route) {
            Ok(session) => session,
            Err(response) => return response,
        };
        match route {
            Route::About => {
                if !self.local_control {
                    return failure(403, responses::ErrorCode::Forbidden);
                }
                response(200, responses::about(&responses::About {
                    schema_version: 1,
                    version: env!("CARGO_PKG_VERSION"),
                    automatic_updates: fc_domain::update_state::UpdatePolicy::default().enabled,
                    update_available: false,
                    update_message: "自动更新默认开启；下载和安装组件尚未就绪，当前还不能执行内网更新。功能就绪并加入学校更新群组后，将自动校验新版、退出安装并立即重启。",
                }), None)
            }
            Route::Classrooms => {
                let directory = self
                    .classrooms
                    .as_ref()
                    .map(|provider| provider())
                    .unwrap_or_else(|| responses::Classrooms {
                        schema_version: 1,
                        discovery_available: false,
                        rooms: Vec::new(),
                    });
                response(200, responses::classrooms(&directory), None)
            }
            Route::QuickConnect => {
                if !self.local_control {
                    return failure(403, responses::ErrorCode::Forbidden);
                }
                let state = match backend.state() {
                    Ok(state) => state,
                    Err(_) => return failure(503, responses::ErrorCode::ServiceUnavailable),
                };
                let installation = state.installation.as_ref();
                let class_name = installation
                    .and_then(|value| value.current_class)
                    .and_then(|id| {
                        state
                            .classes
                            .iter()
                            .find(|class| class.id == id && class.enabled)
                    })
                    .map(|class| class.name.as_str());
                let url = self
                    .quick_connect_url
                    .as_ref()
                    .and_then(|provider| provider());
                response(200, responses::quick_connect(&responses::QuickConnect {
                    schema_version: 1,
                    school_name: installation.map_or("未来教室", |value| value.school_name.as_str()),
                    class_name,
                    entry_url: url.as_deref(),
                    unavailable_reason: url.is_none().then_some("尚无可用的校内 HTTPS 连接地址，请管理员在网络设置中完成配置并启动连接。"),
                }), None)
            }
            Route::Maintenance { .. } | Route::Repair { .. } => {
                failure(503, responses::ErrorCode::ServiceUnavailable)
            }
            Route::Admin { resource, write } => {
                let Some(session) = authenticated else {
                    return failure(401, responses::ErrorCode::Unauthorized);
                };
                if resource == fc_protocol::web::admin::Resource::AccountSecurity {
                    return account_security::dispatch(self, backend, session.lease(), body);
                }
                admin::dispatch(
                    backend,
                    session.lease(),
                    resource,
                    write.then_some(body),
                    self.admin_platform.as_ref(),
                )
            }
            Route::AccountRecover => account_security::recover(self, backend, body),
            Route::SessionLogin => {
                let input = match fc_protocol::web::session::login(body) {
                    Ok(input) => input,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                let password = match fc_security::Password::new(input.password) {
                    Ok(password) => password,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                let proof = match backend.login_web_page(&input.login_name, password, &self.service)
                {
                    Ok(proof) => proof,
                    Err(crate::backend::BackendError::InvalidCredentials) => {
                        return failure(401, responses::ErrorCode::Unauthorized)
                    }
                    Err(crate::backend::BackendError::Session(
                        fc_application::identity::SessionError::RateLimited { .. },
                    )) => return failure(429, responses::ErrorCode::RateLimited),
                    Err(crate::backend::BackendError::Storage(
                        fc_storage::StorageError::LoginRateLimited { .. },
                    )) => return failure(429, responses::ErrorCode::RateLimited),
                    Err(_) => return failure(503, responses::ErrorCode::ServiceUnavailable),
                };
                self.page_session_response(&proof)
            }
            Route::SessionPassword => {
                let input = match fc_protocol::web::session::password(body) {
                    Ok(input) => input,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                let Some(session) = authenticated else {
                    return failure(401, responses::ErrorCode::Unauthorized);
                };
                let old = match fc_security::Password::new(input.current_password) {
                    Ok(value) => value,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                let new = match fc_security::Password::new(input.new_password) {
                    Ok(value) => value,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                match backend.change_web_page_password(session.lease(), old, new, &self.service) {
                    Ok(proof) => self.page_session_response(&proof),
                    Err(crate::backend::BackendError::InvalidCredentials) => {
                        failure(403, responses::ErrorCode::Forbidden)
                    }
                    Err(crate::backend::BackendError::Storage(
                        fc_storage::StorageError::LoginRateLimited { .. },
                    )) => failure(429, responses::ErrorCode::RateLimited),
                    Err(_) => failure(503, responses::ErrorCode::ServiceUnavailable),
                }
            }
            Route::SessionResume => {
                let input = match fc_protocol::web::session::resume(body) {
                    Ok(input) => input,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                let proof = match PageToken::parse_hex(input.page_token) {
                    Ok(proof) => proof,
                    Err(_) => return failure(401, responses::ErrorCode::Unauthorized),
                };
                self.page_session_response(&proof)
            }
            Route::Status => match backend.state() {
                Ok(state) => response(
                    200,
                    responses::initialization_status(state.installation.is_some()),
                    None,
                ),
                Err(_) => failure(503, responses::ErrorCode::ServiceUnavailable),
            },
            Route::Setup => {
                if !self.local_control {
                    return failure(403, responses::ErrorCode::Forbidden);
                }
                let input = match fc_protocol::web::session::setup(body) {
                    Ok(input) => input,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                let password = match fc_security::Password::new(input.password) {
                    Ok(value) => value,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                match backend.initialize_web_page(
                    crate::backend::Initialize {
                        school_name: input.school_name,
                        login_name: input.login_name,
                        display_name: input.display_name,
                    },
                    password,
                    &self.service,
                ) {
                    Ok((proof, recovery)) => {
                        account_security::setup_response(self, &proof, recovery)
                    }
                    Err(crate::backend::BackendError::Identity(
                        fc_ports::IdentityRepositoryError::Domain(
                            fc_domain::identity::IdentityError::AlreadyInitialized,
                        ),
                    )) => failure(409, responses::ErrorCode::RevisionConflict),
                    Err(_) => failure(503, responses::ErrorCode::ServiceUnavailable),
                }
            }
            Route::PairingCreate => {
                let input = match pairing::create(body) {
                    Ok(input) => input,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                let code = match PairingCode::parse(input.code) {
                    Ok(code) => code,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                match self
                    .service
                    .request_pairing(code, &input.login_name, &input.device_name)
                {
                    Ok(receipt) => {
                        let secret = receipt.poll_secret.expose_hex();
                        response(
                            202,
                            responses::pairing_requested(receipt.request_id, secret.as_str()),
                            None,
                        )
                    }
                    Err(error) => service_error(error),
                }
            }
            Route::PairingPoll { request_id } => {
                let input = match pairing::poll(body) {
                    Ok(input) => input,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                let proof = match WebSecret::parse_hex(input.poll_secret) {
                    Ok(proof) => proof,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                match self.service.poll(request_id, &proof) {
                    Ok(PairingPoll::Pending) => response(202, responses::pairing_pending(), None),
                    Ok(PairingPoll::Granted(session)) => {
                        let cookie = session_cookie(&session.cookie);
                        let header = Header {
                            name: "Set-Cookie".into(),
                            value: cookie.as_str().as_bytes().to_vec(),
                        };
                        response(200, responses::authorized(), Some(header))
                    }
                    Err(error) => service_error(error),
                }
            }
            Route::Session => {
                let Some(session) = authenticated else {
                    return failure(401, responses::ErrorCode::Unauthorized);
                };
                response(
                    200,
                    responses::session(
                        &session.account,
                        session.current_class,
                        &session.assignments,
                        session.csrf.as_str(),
                    ),
                    None,
                )
            }
            Route::CommandIssue => {
                let Some(session) = authenticated else {
                    return failure(401, responses::ErrorCode::Unauthorized);
                };
                let family = match fc_protocol::web::command_window::issue(body) {
                    Ok(family) => family,
                    Err(_) => return failure(400, responses::ErrorCode::InvalidRequest),
                };
                match backend.issue_web_command(session.lease(), family) {
                    Ok(issued) => {
                        response(201, fc_protocol::web::command_window::issued(issued), None)
                    }
                    Err(error) => command_failure(&error)
                        .unwrap_or_else(|| failure(403, responses::ErrorCode::Forbidden)),
                }
            }
            Route::SessionLogout => {
                if pairing::logout(body).is_err() {
                    return failure(400, responses::ErrorCode::InvalidRequest);
                }
                let Some(session) = authenticated else {
                    return failure(401, responses::ErrorCode::Unauthorized);
                };
                match backend.logout_web_page(session.lease()) {
                    Ok(()) => response(
                        200,
                        responses::logged_out(),
                        Some(Header {
                            name: "Set-Cookie".into(),
                            value: CLEAR_SESSION_COOKIE.as_bytes().to_vec(),
                        }),
                    ),
                    Err(_) => failure(503, responses::ErrorCode::ResultUnknown),
                }
            }
            Route::VoiceCapabilities
            | Route::VoiceList
            | Route::VoiceSubmit
            | Route::VoiceStatus { .. }
            | Route::VoiceBoundStatus { .. }
            | Route::VoiceWindowCommand { .. }
            | Route::VoiceCancel { .. } => {
                let Some(session) = authenticated else {
                    return failure(401, responses::ErrorCode::Unauthorized);
                };
                voice::dispatch(&backend.voice_service(), session.lease(), route, body)
            }
            Route::Materials { route } => {
                let Some(session) = authenticated else {
                    return failure(401, responses::ErrorCode::Unauthorized);
                };
                materials::dispatch(self, backend, session.lease(), route, body)
            }
            Route::ContentScopes { .. }
            | Route::SubjectVisual { .. }
            | Route::ContentRead { .. }
            | Route::ContentSave { .. }
            | Route::ContentCommand { .. }
            | Route::ContentPresentation { .. }
            | Route::ContentWindowCommand { .. } => {
                let Some(session) = authenticated else {
                    return failure(401, responses::ErrorCode::Unauthorized);
                };
                content::dispatch(backend, session.lease(), route, body)
            }
            _ => failure(404, responses::ErrorCode::NotFound),
        }
    }
}
fn response(
    status: u16,
    body: Result<Vec<u8>, ProtocolError>,
    extra: Option<Header>,
) -> WebResponse {
    let (status, body, extra) = match body {
        Ok(body) => (status, body, extra),
        Err(_) => (
            503,
            br#"{"schemaVersion":1,"code":"SERVICE_UNAVAILABLE"}"#.to_vec(),
            None,
        ),
    };
    let mut headers = vec![
        Header {
            name: "Cache-Control".into(),
            value: b"no-store".to_vec(),
        },
        Header {
            name: "Content-Type".into(),
            value: b"application/json".to_vec(),
        },
        Header {
            name: "X-Content-Type-Options".into(),
            value: b"nosniff".to_vec(),
        },
    ];
    if let Some(header) = extra {
        headers.push(header);
    }
    WebResponse {
        head: ResponseHead {
            status,
            headers,
            content_length: body.len() as u64,
        },
        body,
    }
}
pub(crate) fn failure(status: u16, code: responses::ErrorCode) -> WebResponse {
    response(status, responses::error(code), None)
}
fn command_failure(error: &crate::backend::BackendError) -> Option<WebResponse> {
    use fc_ports::command_window::CommandWindowError as E;
    Some(match error.command_window_error()? {
        E::Expired => failure(410, responses::ErrorCode::CommandExpired),
        E::NotIssued => failure(404, responses::ErrorCode::CommandNotIssued),
        E::Capacity => failure(503, responses::ErrorCode::CommandWindowFull),
        E::Clock => failure(503, responses::ErrorCode::CommandWindowUnavailable),
    })
}
pub(crate) fn boundary_error(error: WebError) -> WebResponse {
    match error {
        WebError::NotFound => failure(404, responses::ErrorCode::NotFound),
        WebError::Method => failure(405, responses::ErrorCode::MethodNotAllowed),
        WebError::Transport | WebError::Host | WebError::Origin | WebError::Csrf => {
            failure(403, responses::ErrorCode::Forbidden)
        }
        _ => failure(400, responses::ErrorCode::InvalidRequest),
    }
}
fn authentication_error(error: WebServiceError) -> WebResponse {
    match error {
        WebServiceError::Denied | WebServiceError::Invalid => {
            failure(401, responses::ErrorCode::Unauthorized)
        }
        _ => service_error(error),
    }
}
fn service_error(error: WebServiceError) -> WebResponse {
    match error {
        WebServiceError::Invalid => failure(400, responses::ErrorCode::InvalidRequest),
        WebServiceError::Denied => failure(403, responses::ErrorCode::Forbidden),
        WebServiceError::RateLimited => failure(429, responses::ErrorCode::RateLimited),
        WebServiceError::Capacity => failure(429, responses::ErrorCode::Capacity),
        _ => failure(503, responses::ErrorCode::ServiceUnavailable),
    }
}

#[cfg(test)]
mod tests;
