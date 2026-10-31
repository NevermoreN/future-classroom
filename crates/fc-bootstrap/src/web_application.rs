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
use fc_security::web::{PairingCode, WebSecret};

mod content;
#[cfg(windows)]
mod material_files;
mod materials;
#[cfg(windows)]
mod shared_upload;
mod voice;
#[cfg(windows)]
mod voice_files;
#[cfg(windows)]
pub use material_files::{
    MaterialDownload, MaterialTransferBinding, PreparedWebMaterial, ReceivingMaterialUpload,
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
    boundary: std::sync::Arc<WebBoundary>,
    service: WebService,
    #[cfg(windows)]
    upload_binding: std::sync::Arc<()>,
    #[cfg(windows)]
    material_files: std::sync::Arc<std::sync::Mutex<Option<material_files::MaterialFiles>>>,
    #[cfg(windows)]
    voice_files: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<voice_files::VoiceFiles>>>>,
}
impl WebApplication {
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
    #[cfg(windows)]
    pub(crate) fn route_hint(&self, head: &RequestHead) -> Result<Route, WebError> {
        self.boundary.preflight(head)
    }
    pub fn new(prefix: &ListenPrefix, service: WebService) -> Result<Self, WebError> {
        Ok(Self {
            boundary: std::sync::Arc::new(WebBoundary::new(prefix)?),
            service,
            #[cfg(windows)]
            upload_binding: std::sync::Arc::new(()),
            #[cfg(windows)]
            material_files: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(windows)]
            voice_files: std::sync::Arc::new(std::sync::Mutex::new(None)),
        })
    }
    /// A short-lived view for one ordinary JSON dispatch. Stream owners use the
    /// original application and a current context on each call; retaining this
    /// job-bound view across jobs would incorrectly cancel their next chunk.
    fn for_execution(&self, fence: crate::web_transfer::ExecutionFence) -> Self {
        Self {
            boundary: self.boundary.clone(),
            service: self.service.with_execution_fence(fence),
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
        } else {
            None
        };
        self.boundary
            .check(
                head,
                authenticated.as_ref().map(|session| session.csrf.as_str()),
            )
            .map_err(|error| self.request_boundary_error(head, error))?;
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
            Route::Status => response(200, responses::public_status(), None),
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
            Route::SessionLogout => {
                if pairing::logout(body).is_err() {
                    return failure(400, responses::ErrorCode::InvalidRequest);
                }
                let Some(session) = authenticated else {
                    return failure(401, responses::ErrorCode::Unauthorized);
                };
                match backend.logout_account(session.lease()) {
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
            | Route::ContentRead { .. }
            | Route::ContentSave { .. }
            | Route::ContentCommand { .. } => {
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
fn failure(status: u16, code: responses::ErrorCode) -> WebResponse {
    response(status, responses::error(code), None)
}
fn boundary_error(error: WebError) -> WebResponse {
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
        _ => failure(503, responses::ErrorCode::ServiceUnavailable),
    }
}

#[cfg(test)]
mod tests;
