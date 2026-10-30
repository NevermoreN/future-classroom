//! Synchronous authenticated Web application dispatch. Invoke only on the shared
//! bounded application worker, never the native HTTP/IOCP or UI event thread.
//! Listener/TLS provisioning and browser delivery remain separate adapters.
use crate::backend::{
    web::{
        cookie_secret, session_cookie, PairingPoll, WebService, WebServiceError,
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

/// Contains explicitly disclosed session headers/body. Deliberately no Debug or
/// Clone: transport ownership must not turn credentials into routine diagnostics.
pub struct WebResponse {
    pub head: ResponseHead,
    pub body: Vec<u8>,
}
pub struct WebApplication {
    boundary: WebBoundary,
    service: WebService,
}
impl WebApplication {
    pub fn new(prefix: &ListenPrefix, service: WebService) -> Result<Self, WebError> {
        Ok(Self {
            boundary: WebBoundary::new(prefix)?,
            service,
        })
    }
    pub fn dispatch(&self, backend: &mut Backend, head: &RequestHead, body: &[u8]) -> WebResponse {
        // A lease from another backend can have identical numeric fields.
        // Bind the trusted composition before authenticating or mutating either.
        if !self.service.same_instance(&backend.web_service()) {
            return failure(503, responses::ErrorCode::ServiceUnavailable);
        }
        let route = match self.boundary.preflight(head) {
            Ok(route) => route,
            Err(error) => return boundary_error(error),
        };
        // The transport's complete body must agree with both its parsed metadata
        // and the application route budget before any pairing/session mutation.
        if body.len() as u64 != head.content_length.unwrap_or(0)
            || body.len() as u64 > route.body_limit()
        {
            return boundary_error(WebError::Framing);
        }
        let authenticated = if route.requires_session() {
            let mut cookies = head
                .headers
                .iter()
                .filter(|h| h.name.eq_ignore_ascii_case("cookie"));
            let header = match (cookies.next(), cookies.next()) {
                (Some(header), None) => header,
                (None, _) => return failure(401, responses::ErrorCode::Unauthorized),
                _ => return boundary_error(WebError::Headers),
            };
            let cookie = match cookie_secret(&header.value) {
                Ok(cookie) => cookie,
                Err(error) => return authentication_error(error),
            };
            match self.service.authenticate(&cookie) {
                Ok(session) => Some(session),
                Err(error) => return authentication_error(error),
            }
        } else {
            None
        };
        if let Err(error) = self.boundary.check(
            head,
            authenticated.as_ref().map(|session| session.csrf.as_str()),
        ) {
            return boundary_error(error);
        }
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
