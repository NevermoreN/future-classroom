//! Strict request boundary, not a session authenticator or an enabled listener.
//! Composition must authenticate the cookie against its exact live Web lease
//! and supply that session's CSRF token before dispatching any private route.
use crate::{parse, ProtocolError, SCHEMA_VERSION};
use fc_domain::{ids::*, voice::*};
use fc_ports::{http::*, voice::*};
use serde::Deserialize;

pub mod assets;
pub mod pairing;
pub mod responses;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebError {
    Transport,
    Host,
    Origin,
    Csrf,
    Headers,
    Framing,
    NotFound,
    Method,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Status,
    PairingCreate,
    PairingPoll { request_id: u64 },
    Session,
    SessionLogout,
    VoiceCapabilities,
    VoiceUpload { class_id: ClassId },
    VoiceSubmit,
    VoiceStatus { job_id: VoiceJobId },
    VoiceCancel { job_id: VoiceJobId },
}
impl Route {
    pub fn requires_session(self) -> bool {
        !matches!(
            self,
            Self::Status | Self::PairingCreate | Self::PairingPoll { .. }
        )
    }
    pub fn body_limit(self) -> u64 {
        match self {
            Self::PairingCreate | Self::PairingPoll { .. } | Self::SessionLogout => {
                pairing::MAX_PAIRING_JSON_BYTES as u64
            }
            Self::VoiceUpload { .. } => 2 * 1024 * 1024,
            Self::VoiceSubmit | Self::VoiceCancel { .. } => crate::MAX_JSON_BYTES as u64,
            _ => 0,
        }
    }
    fn requires_csrf(self) -> bool {
        self.post() && !matches!(self, Self::PairingCreate | Self::PairingPoll { .. })
    }
    fn post(self) -> bool {
        matches!(
            self,
            Self::PairingCreate
                | Self::PairingPoll { .. }
                | Self::SessionLogout
                | Self::VoiceUpload { .. }
                | Self::VoiceSubmit
                | Self::VoiceCancel { .. }
        )
    }
}

/// One explicit administrator-configured HTTPS prefix. No forwarded header,
/// Referer, request Host, or JSON field can change this trusted origin.
pub struct WebBoundary {
    authority: String,
    origin: String,
    base: String,
}
impl WebBoundary {
    pub fn new(prefix: &ListenPrefix) -> Result<Self, WebError> {
        if prefix.is_diagnostic() {
            return Err(WebError::Transport);
        }
        let tail = prefix
            .as_str()
            .strip_prefix("https://")
            .ok_or(WebError::Transport)?;
        let (authority, path) = tail.split_once('/').ok_or(WebError::Transport)?;
        // Reject aliases and noncanonical spellings rather than normalizing
        // request routing differently from HTTP.sys or a browser.
        if path
            .bytes()
            .any(|b| !b.is_ascii_alphanumeric() && !b"/-_".contains(&b))
        {
            return Err(WebError::Transport);
        }
        let authority = authority.to_ascii_lowercase();
        // Browsers omit the default port from serialized Origin/Host.
        let authority = authority
            .strip_suffix(":443")
            .unwrap_or(&authority)
            .to_owned();
        Ok(Self {
            origin: format!("https://{authority}"),
            authority,
            base: format!("/{path}api/v1/"),
        })
    }
    pub fn route(&self, method: &str, target: &str) -> Result<Route, WebError> {
        if target.len() > MAX_TARGET_BYTES
            || !target.is_ascii()
            || target
                .bytes()
                .any(|b| b <= b' ' || b == 127 || b"%\\#".contains(&b))
        {
            return Err(WebError::NotFound);
        }
        let path = target.strip_prefix(&self.base).ok_or(WebError::NotFound)?;
        let route = match path {
            "status" => Route::Status,
            "pairing/requests" => Route::PairingCreate,
            "session" => Route::Session,
            "session/logout" => Route::SessionLogout,
            "voice/capabilities" => Route::VoiceCapabilities,
            "voice/broadcasts" => Route::VoiceSubmit,
            _ => {
                if let Some(id) = path
                    .strip_prefix("pairing/requests/")
                    .and_then(|s| s.strip_suffix("/poll"))
                {
                    Route::PairingPoll {
                        request_id: decimal(id, false).ok_or(WebError::NotFound)?,
                    }
                } else if let Some(id) = path.strip_prefix("voice/uploads?classId=") {
                    Route::VoiceUpload {
                        class_id: ClassId::new(decimal(id, false).ok_or(WebError::NotFound)?)
                            .map_err(|_| WebError::NotFound)?,
                    }
                } else if let Some(job) = path.strip_prefix("voice/broadcasts/") {
                    let (id, cancel) = match job.strip_suffix("/cancel") {
                        Some(id) => (id, true),
                        None => (job, false),
                    };
                    let job_id = VoiceJobId::new(decimal(id, false).ok_or(WebError::NotFound)?)
                        .map_err(|_| WebError::NotFound)?;
                    if cancel {
                        Route::VoiceCancel { job_id }
                    } else {
                        Route::VoiceStatus { job_id }
                    }
                } else {
                    return Err(WebError::NotFound);
                }
            }
        };
        if method != if route.post() { "POST" } else { "GET" } {
            return Err(WebError::Method);
        }
        Ok(route)
    }
    /// expected_csrf is obtained from the authenticated server session, never
    /// from a cookie echo or a JSON claim. This check does not authenticate it.
    pub fn check(
        &self,
        head: &RequestHead,
        expected_csrf: Option<&str>,
    ) -> Result<Route, WebError> {
        let route = self.preflight(head)?;
        let supplied_csrf = unique(head, "x-fc-csrf")?;
        if route.requires_csrf() {
            let supplied = supplied_csrf.ok_or(WebError::Csrf)?;
            let expected = expected_csrf
                .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or(WebError::Csrf)?;
            if !equal_token(supplied, expected.as_bytes()) {
                return Err(WebError::Csrf);
            }
        }
        Ok(route)
    }
    /// Reject invalid transport, origin and framing before looking up a bearer
    /// secret or touching a session ledger. Success does not authenticate a
    /// session and does not authorize a private request without `check`.
    pub fn preflight(&self, head: &RequestHead) -> Result<Route, WebError> {
        self.check_host(head)?;
        let route = self.route(&head.method, &head.target)?;
        self.finish_preflight(head, route)
    }
    fn check_host(&self, head: &RequestHead) -> Result<(), WebError> {
        if !head.tls {
            return Err(WebError::Transport);
        }
        validate_headers(&head.headers).map_err(|_| WebError::Headers)?;
        if unique(head, "host")?.is_none_or(|v| !v.eq_ignore_ascii_case(self.authority.as_bytes()))
        {
            return Err(WebError::Host);
        }
        Ok(())
    }
    /// Canonical public static paths share the transport/origin/framing gate.
    /// API URLs can never fall through to the HTML entry document.
    pub fn static_path<'a>(&self, head: &'a RequestHead) -> Result<&'a str, WebError> {
        self.check_host(head)?;
        if head.method != "GET" {
            return Err(WebError::Method);
        }
        if head.target.len() > MAX_TARGET_BYTES {
            return Err(WebError::NotFound);
        }
        let base = self
            .base
            .strip_suffix("api/v1/")
            .ok_or(WebError::NotFound)?;
        let path = head.target.strip_prefix(base).ok_or(WebError::NotFound)?;
        let path = if path.is_empty() { "index.html" } else { path };
        if !assets::valid_path(path) {
            return Err(WebError::NotFound);
        }
        self.finish_preflight(head, Route::Status)?;
        Ok(path)
    }
    fn finish_preflight(&self, head: &RequestHead, route: Route) -> Result<Route, WebError> {
        let origin = unique(head, "origin")?;
        if origin.is_some_and(|v| v != self.origin.as_bytes()) || (route.post() && origin.is_none())
        {
            return Err(WebError::Origin);
        }
        if let Some(site) = unique(head, "sec-fetch-site")? {
            if site != b"same-origin" && !(site == b"none" && !route.post()) {
                return Err(WebError::Origin);
            }
        }
        // Only the two pairing bootstrap routes omit a pre-existing session
        // CSRF token. They still require same-origin JSON requests; polling
        // additionally proves the original unpredictable secret in its body.
        unique(head, "x-fc-csrf")?;
        // Cookie and Authorization must not be ambiguously combined by layers.
        unique(head, "cookie")?;
        if unique(head, "authorization")?.is_some()
            || unique(head, "transfer-encoding")?.is_some()
            || unique(head, "expect")?.is_some()
            || unique(head, "content-encoding")?.is_some()
            || unique(head, "trailer")?.is_some()
        {
            return Err(WebError::Framing);
        }
        let length = unique(head, "content-length")?
            .map(|v| {
                std::str::from_utf8(v)
                    .ok()
                    .and_then(|s| decimal(s, true))
                    .ok_or(WebError::Framing)
            })
            .transpose()?;
        if length != head.content_length {
            return Err(WebError::Framing);
        }
        if route.post() {
            let len = length
                .filter(|n| *n > 0 && *n <= route.body_limit())
                .ok_or(WebError::Body)?;
            if !head.has_body || len == 0 {
                return Err(WebError::Body);
            }
            let content_type = unique(head, "content-type")?.ok_or(WebError::Body)?;
            let allowed = if matches!(route, Route::VoiceUpload { .. }) {
                b"audio/wav".as_slice()
            } else {
                b"application/json".as_slice()
            };
            if content_type != allowed {
                return Err(WebError::Body);
            }
        } else if head.has_body || length.is_some_and(|n| n != 0) {
            return Err(WebError::Body);
        }
        Ok(route)
    }
}
fn unique<'a>(head: &'a RequestHead, name: &str) -> Result<Option<&'a [u8]>, WebError> {
    let mut matches = head
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case(name));
    let result = matches.next().map(|h| h.value.as_slice());
    if matches.next().is_some() {
        return Err(WebError::Headers);
    }
    Ok(result)
}
fn equal_token(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}
fn decimal(s: &str, zero: bool) -> Option<u64> {
    if s.is_empty()
        || s.len() > 20
        || !s.bytes().all(|b| b.is_ascii_digit())
        || (s.len() > 1 && s.starts_with('0'))
    {
        return None;
    }
    s.parse().ok().filter(|n| zero || *n != 0)
}

/// Decimal strings preserve all 64 bits in JavaScript. They cannot inject an
/// actor, owner, origin, file path, hash, playback state or mutable target later.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VoiceSubmitDto {
    schema_version: u16,
    command_id: String,
    asset_id: String,
    expected_asset_revision: String,
    class_id: String,
    repeat_total: u8,
}
pub fn voice_submit(bytes: &[u8]) -> Result<VoiceSubmitRequest, ProtocolError> {
    let d: VoiceSubmitDto = parse(bytes)?;
    if d.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    if !(1..=10).contains(&d.repeat_total) {
        return Err(ProtocolError::InvalidCommand);
    }
    let bad = || ProtocolError::InvalidCommand;
    Ok(VoiceSubmitRequest {
        command_id: CommandId::new(decimal(&d.command_id, false).ok_or_else(bad)?)
            .map_err(|_| bad())?,
        asset_id: VoiceAssetId::new(decimal(&d.asset_id, false).ok_or_else(bad)?)
            .map_err(|_| bad())?,
        expected_asset_revision: Revision::new(
            decimal(&d.expected_asset_revision, true).ok_or_else(bad)?,
        ),
        class_id: ClassId::new(decimal(&d.class_id, false).ok_or_else(bad)?).map_err(|_| bad())?,
        repeat_total: d.repeat_total,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct VoiceCancelDto {
    schema_version: u16,
    command_id: String,
    expected_revision: String,
}
pub fn voice_cancel(job_id: VoiceJobId, bytes: &[u8]) -> Result<VoiceCancelRequest, ProtocolError> {
    let d: VoiceCancelDto = parse(bytes)?;
    if d.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    Ok(VoiceCancelRequest {
        job_id,
        command_id: CommandId::new(
            decimal(&d.command_id, false).ok_or(ProtocolError::InvalidCommand)?,
        )
        .map_err(|_| ProtocolError::InvalidCommand)?,
        expected_revision: Revision::new(
            decimal(&d.expected_revision, true).ok_or(ProtocolError::InvalidCommand)?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn boundary() -> WebBoundary {
        WebBoundary::new(&ListenPrefix::https("https://board.school:443/fc/").unwrap()).unwrap()
    }
    fn head() -> RequestHead {
        let token = TokenIssuer::new().unwrap().issue(0).unwrap();
        RequestHead {
            token,
            method: "POST".into(),
            target: "/fc/api/v1/voice/broadcasts".into(),
            headers: vec![
                Header {
                    name: "Host".into(),
                    value: b"board.school".to_vec(),
                },
                Header {
                    name: "Origin".into(),
                    value: b"https://board.school".to_vec(),
                },
                Header {
                    name: "X-FC-CSRF".into(),
                    value: vec![b'a'; 64],
                },
                Header {
                    name: "Content-Length".into(),
                    value: b"2".to_vec(),
                },
                Header {
                    name: "Content-Type".into(),
                    value: b"application/json".to_vec(),
                },
            ],
            tls: true,
            content_length: Some(2),
            has_body: true,
        }
    }
    #[test]
    fn origin_host_csrf_and_transport_are_independent() {
        let boundary = boundary();
        let csrf = "a".repeat(64);
        assert_eq!(boundary.check(&head(), Some(&csrf)), Ok(Route::VoiceSubmit));
        for (name, value, expected) in [
            ("Host", "evil.school", WebError::Host),
            ("Origin", "null", WebError::Origin),
            ("Origin", "https://board.school.evil", WebError::Origin),
            ("X-FC-CSRF", "b", WebError::Csrf),
        ] {
            let mut h = head();
            h.headers
                .iter_mut()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .unwrap()
                .value = value.as_bytes().into();
            assert_eq!(boundary.check(&h, Some(&csrf)), Err(expected));
        }
        let mut h = head();
        h.tls = false;
        assert_eq!(boundary.check(&h, Some(&csrf)), Err(WebError::Transport));
        assert_eq!(boundary.check(&head(), None), Err(WebError::Csrf));
        let mut h = head();
        h.headers.push(h.headers[0].clone());
        assert_eq!(boundary.check(&h, Some(&csrf)), Err(WebError::Headers));
        assert!(WebBoundary::new(
            &ListenPrefix::isolated_loopback(12345, "FutureClassroom-Test-io").unwrap()
        )
        .is_err());
    }
    #[test]
    fn preflight_is_only_transport_admission_and_never_private_csrf_authorization() {
        let boundary = boundary();
        let mut request = head();
        request
            .headers
            .retain(|h| !h.name.eq_ignore_ascii_case("x-fc-csrf"));
        assert_eq!(boundary.preflight(&request), Ok(Route::VoiceSubmit));
        assert_eq!(boundary.check(&request, None), Err(WebError::Csrf));
        assert_eq!(
            boundary.check(&request, Some(&"a".repeat(64))),
            Err(WebError::Csrf)
        );
        request.headers.push(Header {
            name: "X-FC-CSRF".into(),
            value: vec![b'a'; 64],
        });
        assert_eq!(boundary.preflight(&request), Ok(Route::VoiceSubmit));
        assert_eq!(boundary.check(&request, None), Err(WebError::Csrf));
        assert_eq!(
            boundary.check(&request, Some(&"a".repeat(64))),
            Ok(Route::VoiceSubmit)
        );
        request.headers.push(Header {
            name: "Cookie".into(),
            value: b"token=x".to_vec(),
        });
        request.headers.push(Header {
            name: "cookie".into(),
            value: b"token=y".to_vec(),
        });
        assert_eq!(boundary.preflight(&request), Err(WebError::Headers));
    }
    #[test]
    fn routing_has_no_remote_launch_admin_or_static_fallback() {
        let b = boundary();
        for path in [
            "../status",
            "voice/broadcasts/01",
            "voice/broadcasts/1/",
            "voice/broadcasts/1?x=1",
            "voice/%62roadcasts/1",
            "materials/1/open",
            "settings",
            "commands",
            "voice/uploads?classId=1&role=global",
            "status?token=secret",
        ] {
            assert_eq!(
                b.route("GET", &format!("/fc/api/v1/{path}")),
                Err(WebError::NotFound),
                "{path}"
            );
        }
        assert_eq!(
            b.route("POST", "/fc/api/v1/voice/uploads?classId=1"),
            Ok(Route::VoiceUpload {
                class_id: ClassId::new(1).unwrap()
            })
        );
        assert_eq!(
            b.route("GET", "/fc/api/v1/voice/broadcasts"),
            Err(WebError::Method)
        );
        assert_eq!(
            b.route("GET", "/fc/api/v1/voice/broadcasts/18446744073709551615"),
            Ok(Route::VoiceStatus {
                job_id: VoiceJobId::new(u64::MAX).unwrap()
            })
        );
    }
    #[test]
    fn ambiguous_framing_and_cross_site_reads_are_rejected() {
        let b = boundary();
        let csrf = "a".repeat(64);
        for name in [
            "Content-Length",
            "Transfer-Encoding",
            "Content-Encoding",
            "Expect",
            "Cookie",
            "Origin",
        ] {
            let mut h = head();
            if name == "Cookie" {
                h.headers.push(Header {
                    name: name.into(),
                    value: b"a=1".to_vec(),
                });
            }
            h.headers.push(Header {
                name: name.into(),
                value: b"2".to_vec(),
            });
            assert!(b.check(&h, Some(&csrf)).is_err(), "{name}");
        }
        let mut h = head();
        h.content_length = Some(3);
        assert_eq!(b.check(&h, Some(&csrf)), Err(WebError::Framing));
        let mut h = head();
        h.headers.push(Header {
            name: "Sec-Fetch-Site".into(),
            value: b"cross-site".to_vec(),
        });
        assert_eq!(b.check(&h, Some(&csrf)), Err(WebError::Origin));
    }
    #[test]
    fn voice_dto_exact_ids_and_injection_rejection() {
        let input = r#"{"schemaVersion":1,"commandId":"18446744073709551615","assetId":"2","expectedAssetRevision":"0","classId":"1","repeatTotal":3}"#;
        assert_eq!(
            voice_submit(input.as_bytes()).unwrap().command_id.get(),
            u64::MAX
        );
        for field in [
            r#""role":"GLOBAL_ADMIN""#,
            r#""owner":1"#,
            r#""path":"C:/secret.wav""#,
            r#""actor":{}"#,
            r#""repeatTotal":1"#,
        ] {
            let injected = format!("{},{} }}", &input[..input.len() - 1], field);
            assert!(voice_submit(injected.as_bytes()).is_err());
        }
        for id in ["0", "01", "+1", "1e2", "18446744073709551616"] {
            assert!(voice_submit(input.replace("18446744073709551615", id).as_bytes()).is_err());
        }
        assert!(voice_cancel(
            VoiceJobId::new(1).unwrap(),
            br#"{"schemaVersion":1,"commandId":"2","expectedRevision":"0","jobId":"3"}"#
        )
        .is_err());
    }
    #[test]
    fn pairing_alone_bootstraps_without_existing_session_csrf() {
        let b = boundary();
        for (target, route) in [
            ("/fc/api/v1/pairing/requests", Route::PairingCreate),
            (
                "/fc/api/v1/pairing/requests/1/poll",
                Route::PairingPoll { request_id: 1 },
            ),
        ] {
            let mut h = head();
            h.target = target.into();
            h.headers
                .retain(|h| !h.name.eq_ignore_ascii_case("x-fc-csrf"));
            assert_eq!(b.check(&h, None), Ok(route));
            assert!(!route.requires_session());
            assert_eq!(route.body_limit(), 4096);
            let mut missing = h.clone();
            missing
                .headers
                .retain(|h| !h.name.eq_ignore_ascii_case("origin"));
            assert_eq!(b.check(&missing, None), Err(WebError::Origin));
            for (name, value, error) in [
                ("Origin", "https://evil.school", WebError::Origin),
                ("Host", "evil.school", WebError::Host),
                ("Sec-Fetch-Site", "cross-site", WebError::Origin),
                ("Authorization", "Bearer x", WebError::Framing),
                ("Transfer-Encoding", "chunked", WebError::Framing),
            ] {
                let mut bad = h.clone();
                bad.headers.retain(|h| !h.name.eq_ignore_ascii_case(name));
                bad.headers.push(Header {
                    name: name.into(),
                    value: value.as_bytes().into(),
                });
                assert_eq!(b.check(&bad, None), Err(error));
            }
            h.headers.push(Header {
                name: "Cookie".into(),
                value: b"fc-session=old".into(),
            });
            assert_eq!(b.check(&h, None), Ok(route));
            h.content_length = Some(4097);
            h.headers
                .iter_mut()
                .find(|h| h.name.eq_ignore_ascii_case("content-length"))
                .unwrap()
                .value = b"4097".into();
            assert_eq!(b.check(&h, None), Err(WebError::Body));
        }
        let mut logout = head();
        logout.target = "/fc/api/v1/session/logout".into();
        assert_eq!(b.check(&logout, None), Err(WebError::Csrf));
        assert_eq!(
            b.check(&logout, Some(&"a".repeat(64))),
            Ok(Route::SessionLogout)
        );
        assert!(Route::SessionLogout.requires_session());
        assert!(Route::Session.requires_session());
        assert_eq!(b.check(&head(), None), Err(WebError::Csrf));
    }
    #[test]
    fn private_session_get_forbids_body_and_pairing_urls_forbid_secrets() {
        let b = boundary();
        let mut h = head();
        h.method = "GET".into();
        h.target = "/fc/api/v1/session".into();
        assert_eq!(b.check(&h, None), Err(WebError::Body));
        h.has_body = false;
        h.content_length = None;
        h.headers
            .retain(|h| !h.name.eq_ignore_ascii_case("content-length"));
        assert_eq!(b.check(&h, None), Ok(Route::Session));
        for path in [
            "pairing/requests/01/poll",
            "pairing/requests/0/poll",
            "pairing/requests/18446744073709551616/poll",
            "pairing/requests/%31/poll",
            "pairing/requests/1/poll?pollSecret=secret",
            "pairing/requests?code=01234567",
            "pairing/requests/1",
            "session?token=secret",
            "session/logout/",
        ] {
            assert_eq!(
                b.route("POST", &format!("/fc/api/v1/{path}")),
                Err(WebError::NotFound),
                "{path}"
            );
        }
        assert_eq!(
            b.route(
                "POST",
                "/fc/api/v1/pairing/requests/18446744073709551615/poll"
            ),
            Ok(Route::PairingPoll {
                request_id: u64::MAX
            })
        );
        assert_eq!(
            b.route("GET", "/fc/api/v1/pairing/requests/1/poll"),
            Err(WebError::Method)
        );
        assert_eq!(b.route("POST", "/fc/api/v1/session"), Err(WebError::Method));
    }
}
