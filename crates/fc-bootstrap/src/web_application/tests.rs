//! Port-level application evidence with real Backend, credentials and SQLite.
//! Synthetic TLS metadata is not evidence of a real listener/browser/TLS setup.
use super::*;
use fc_application::identity::{SessionLease, SessionPurpose};
use fc_domain::time::*;
use fc_ports::{http::TokenIssuer, Clock, MonotonicClock};
use fc_security::{CredentialRecordCodec, Password};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

const PASSWORD: &str = "Synthetic-Web-Application-Password-2026";
struct Time(AtomicU64, AtomicU64);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        let seconds = self.1.load(Ordering::Acquire);
        LocalDateTime::new(
            LocalDate::new(2026, 10, if seconds < 86_400 { 30 } else { 31 }).unwrap(),
            (seconds % 86_400) as u32,
        )
        .unwrap()
    }
}
impl MonotonicClock for Time {
    fn monotonic_millis(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "FutureClassroom-Web-Application-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn database(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("web application fixture cleanup");
    }
}
struct BackendFixture {
    backend: Backend,
    app: WebApplication,
    service: WebService,
    admin: SessionLease,
    clock: Arc<Time>,
    directory: Directory,
}
impl BackendFixture {
    fn open(directory: &Directory, clock: Arc<Time>) -> Backend {
        Backend::open(
            directory.database(),
            CredentialRecordCodec::from_key([67; 32]).unwrap(),
            clock.clone(),
            clock,
        )
        .unwrap()
    }
    fn new() -> Self {
        let directory = Directory::new();
        let clock = Arc::new(Time(AtomicU64::new(1000), AtomicU64::new(36_000)));
        let mut backend = Self::open(&directory, clock.clone());
        drop(
            backend
                .initialize(
                    crate::backend::Initialize {
                        school_name: "Synthetic private school".into(),
                        login_name: "admin".into(),
                        display_name: "Synthetic private administrator".into(),
                    },
                    Password::new(PASSWORD.to_owned()).unwrap(),
                )
                .unwrap(),
        );
        let admin = backend
            .login("admin", Password::new(PASSWORD.to_owned()).unwrap())
            .unwrap();
        let service = backend.web_service();
        service.set_enabled(admin, true).unwrap();
        let app = WebApplication::new(
            &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
            service.clone(),
        )
        .unwrap();
        Self {
            backend,
            app,
            service,
            admin,
            clock,
            directory,
        }
    }
    fn dispatch(&mut self, head: &RequestHead, body: &[u8]) -> WebResponse {
        let response = self.app.dispatch(&mut self.backend, head, body);
        response.head.validate().unwrap();
        assert_eq!(response.head.content_length, response.body.len() as u64);
        response
    }
    fn pair(&mut self, device: &str, pending_first: bool) -> String {
        self.pair_login("admin", device, pending_first)
    }
    fn pair_login(&mut self, login: &str, device: &str, pending_first: bool) -> String {
        let window = self.service.begin_pairing(self.admin).unwrap();
        let code = window.code.expose_text();
        let body = format!(
            r#"{{"schemaVersion":1,"code":"{}","loginName":"{login}","deviceName":"{device}"}}"#,
            code.as_str()
        );
        let created = self.dispatch(
            &head("POST", "pairing/requests", body.len(), None, None),
            body.as_bytes(),
        );
        assert_eq!(created.head.status, 202);
        let id: u64 = field(&created.body, "requestId").parse().unwrap();
        let secret = field(&created.body, "pollSecret");
        let poll = format!(r#"{{"schemaVersion":1,"pollSecret":"{secret}"}}"#);
        let route = format!("pairing/requests/{id}/poll");
        if pending_first {
            let response = self.dispatch(
                &head("POST", &route, poll.len(), None, None),
                poll.as_bytes(),
            );
            assert_eq!(response.head.status, 202);
            assert!(text(&response.body).contains("PENDING"));
            assert!(!text(&response.body).contains(&secret));
            self.clock.0.fetch_add(500, Ordering::AcqRel);
        }
        let review = self
            .service
            .pending(self.admin)
            .unwrap()
            .into_iter()
            .find(|r| r.request_id == id)
            .unwrap();
        self.service
            .approve(self.admin, id, review.identity_revision)
            .unwrap();
        let authorized = self.dispatch(
            &head("POST", &route, poll.len(), None, None),
            poll.as_bytes(),
        );
        assert_eq!(authorized.head.status, 200);
        assert_eq!(
            text(&authorized.body),
            r#"{"schemaVersion":1,"status":"AUTHORIZED"}"#
        );
        assert!(!text(&authorized.body).contains(&secret));
        let cookie = authorized
            .head
            .headers
            .iter()
            .find(|h| h.name == "Set-Cookie")
            .unwrap();
        let cookie = text(&cookie.value);
        for flag in ["Path=/", "Secure", "HttpOnly", "SameSite=Strict"] {
            assert!(cookie.contains(flag));
        }
        cookie.split(';').next().unwrap().to_owned()
    }
}
impl Drop for BackendFixture {
    fn drop(&mut self) {
        self.backend
            .close()
            .expect("web application backend shutdown");
    }
}
fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}
// Only fixed response keys containing unescaped synthetic decimal/hex values.
fn field(bytes: &[u8], key: &str) -> String {
    text(bytes)
        .split_once(&format!("\"{key}\":\""))
        .unwrap()
        .1
        .split('"')
        .next()
        .unwrap()
        .to_owned()
}
fn head(
    method: &str,
    route: &str,
    len: usize,
    cookie: Option<&str>,
    csrf: Option<&str>,
) -> RequestHead {
    let mut headers = vec![Header {
        name: "Host".into(),
        value: b"board.school".to_vec(),
    }];
    if method == "POST" {
        headers.extend([
            Header {
                name: "Origin".into(),
                value: b"https://board.school".to_vec(),
            },
            Header {
                name: "Content-Type".into(),
                value: b"application/json".to_vec(),
            },
            Header {
                name: "Content-Length".into(),
                value: len.to_string().into_bytes(),
            },
        ]);
    }
    if let Some(cookie) = cookie {
        headers.push(Header {
            name: "Cookie".into(),
            value: cookie.as_bytes().to_vec(),
        });
    }
    if let Some(csrf) = csrf {
        headers.push(Header {
            name: "X-FC-CSRF".into(),
            value: csrf.as_bytes().to_vec(),
        });
    }
    RequestHead {
        token: TokenIssuer::new().unwrap().issue(0).unwrap(),
        method: method.into(),
        target: format!("/fc/api/v1/{route}"),
        headers,
        tls: true,
        content_length: (method == "POST").then_some(len as u64),
        has_body: method == "POST" && len != 0,
    }
}

#[test]
fn dispatcher_rejects_another_backend_even_when_exact_lease_numbers_collide() {
    let mut first = BackendFixture::new();
    let mut second = BackendFixture::new();
    let first_cookie = first.pair("first-installation", false);
    let second_cookie = second.pair("second-installation", false);
    let first_session = first
        .service
        .authenticate(&cookie_secret(first_cookie.as_bytes()).unwrap())
        .unwrap();
    let second_session = second
        .service
        .authenticate(&cookie_secret(second_cookie.as_bytes()).unwrap())
        .unwrap();
    assert_eq!(
        first_session.lease(),
        second_session.lease(),
        "fixture must demonstrate numeric collision"
    );
    let body = br#"{"schemaVersion":1}"#;
    let logout = head(
        "POST",
        "session/logout",
        body.len(),
        Some(&first_cookie),
        Some(first_session.csrf.as_str()),
    );
    let before = second.backend.state().unwrap();
    let response = first.app.dispatch(&mut second.backend, &logout, body);
    assert_eq!(response.head.status, 503);
    assert_eq!(second.backend.state().unwrap(), before);
    for (service, cookie) in [
        (&first.service, &first_cookie),
        (&second.service, &second_cookie),
    ] {
        assert!(service
            .authenticate(&cookie_secret(cookie.as_bytes()).unwrap())
            .is_ok());
    }
    assert_eq!(
        first.dispatch(&logout, body).head.status,
        200,
        "same-instance dispatch remains usable"
    );
    assert!(second
        .service
        .authenticate(&cookie_secret(second_cookie.as_bytes()).unwrap())
        .is_ok());
}

#[test]
fn pairing_session_csrf_and_account_logout_cross_the_real_backend_and_persist_epoch() {
    let mut fixture = BackendFixture::new();
    let first = fixture.pair("first-browser", true);
    let second = fixture.pair("second-browser", false);
    let first_secret = cookie_secret(first.as_bytes()).unwrap();
    let second_secret = cookie_secret(second.as_bytes()).unwrap();
    let first_lease = fixture.service.authenticate(&first_secret).unwrap().lease();
    let second_lease = fixture
        .service
        .authenticate(&second_secret)
        .unwrap()
        .lease();
    let before = fixture.backend.state().unwrap();
    let session = fixture.dispatch(&head("GET", "session", 0, Some(&first), None), b"");
    assert_eq!(session.head.status, 200);
    let body = text(&session.body);
    for expected in [
        "AUTHENTICATED",
        "Synthetic private administrator",
        "GLOBAL_ADMIN",
        "\"currentClassId\":null",
        "\"assignments\":[]",
    ] {
        assert!(body.contains(expected));
    }
    for denied in [
        "credential",
        "recovery",
        "password",
        "loginName",
        "authEpoch",
        "pollSecret",
    ] {
        assert!(!body.contains(denied));
    }
    let csrf = field(&session.body, "csrf");
    assert_eq!(
        fixture
            .dispatch(
                &head("GET", "voice/capabilities", 0, Some(&first), None),
                b""
            )
            .head
            .status,
        404
    );
    let logout = br#"{"schemaVersion":1}"#;
    let denied = fixture.dispatch(
        &head("POST", "session/logout", logout.len(), Some(&first), None),
        logout,
    );
    assert_eq!(denied.head.status, 403);
    assert_eq!(fixture.backend.state().unwrap(), before);
    let bad = "b".repeat(64);
    let denied = fixture.dispatch(
        &head(
            "POST",
            "session/logout",
            logout.len(),
            Some(&first),
            Some(&bad),
        ),
        logout,
    );
    assert_eq!(denied.head.status, 403);
    let logged_out = fixture.dispatch(
        &head(
            "POST",
            "session/logout",
            logout.len(),
            Some(&first),
            Some(&csrf),
        ),
        logout,
    );
    assert_eq!(logged_out.head.status, 200);
    assert!(text(&logged_out.body).contains("LOGGED_OUT"));
    assert!(logged_out
        .head
        .headers
        .iter()
        .any(|h| h.name == "Set-Cookie" && h.value == CLEAR_SESSION_COOKIE.as_bytes()));
    let after = fixture.backend.state().unwrap();
    assert_eq!(
        after.accounts[0].principal.auth_epoch,
        before.accounts[0].principal.auth_epoch.next().unwrap()
    );
    for proof in [fixture.admin, first_lease, second_lease] {
        assert!(fixture
            .backend
            .check_session(proof, SessionPurpose::Manage)
            .is_err());
    }
    for cookie in [&first, &second] {
        assert_eq!(
            fixture
                .dispatch(&head("GET", "session", 0, Some(cookie), None), b"")
                .head
                .status,
            401
        );
    }
    fixture.backend.close().unwrap();
    fixture.backend = BackendFixture::open(&fixture.directory, fixture.clock.clone());
    assert_eq!(
        fixture.backend.state().unwrap().accounts[0]
            .principal
            .auth_epoch,
        after.accounts[0].principal.auth_epoch
    );
    for proof in [fixture.admin, first_lease, second_lease] {
        assert!(fixture
            .backend
            .check_session(proof, SessionPurpose::Manage)
            .is_err());
    }
}

#[test]
fn public_status_is_non_sensitive_and_frame_origin_cookie_fail_before_mutation() {
    let mut fixture = BackendFixture::new();
    let status = fixture.dispatch(&head("GET", "status", 0, None, None), b"");
    assert_eq!(status.head.status, 200);
    assert_eq!(
        text(&status.body),
        r#"{"schemaVersion":1,"apiVersion":1,"uiCompatibilityVersion":1,"online":true}"#
    );
    for (name, value) in [
        ("Cache-Control", "no-store"),
        ("Content-Type", "application/json"),
        ("X-Content-Type-Options", "nosniff"),
    ] {
        assert!(status
            .head
            .headers
            .iter()
            .any(|h| h.name == name && h.value == value.as_bytes()));
    }
    assert!(!status
        .head
        .headers
        .iter()
        .any(|h| h.name.to_ascii_lowercase().starts_with("access-control-")));
    let before = fixture.backend.state().unwrap();
    let window = fixture.service.begin_pairing(fixture.admin).unwrap();
    let body = format!(
        r#"{{"schemaVersion":1,"code":"{}","loginName":"admin","deviceName":"browser"}}"#,
        window.code.expose_text().as_str()
    );
    let mismatch = fixture.dispatch(
        &head("POST", "pairing/requests", body.len() - 1, None, None),
        body.as_bytes(),
    );
    assert_eq!(mismatch.head.status, 400);
    assert!(fixture.service.pending(fixture.admin).unwrap().is_empty());
    let mut cross_origin = head("POST", "pairing/requests", body.len(), None, None);
    cross_origin
        .headers
        .iter_mut()
        .find(|h| h.name == "Origin")
        .unwrap()
        .value = b"https://evil.school".to_vec();
    assert_eq!(
        fixture.dispatch(&cross_origin, body.as_bytes()).head.status,
        403
    );
    assert!(fixture.service.pending(fixture.admin).unwrap().is_empty());
    let valid = fixture.dispatch(
        &head("POST", "pairing/requests", body.len(), None, None),
        body.as_bytes(),
    );
    assert_eq!(
        valid.head.status, 202,
        "rejected frames/origins never consumed the pairing code"
    );
    let mut duplicate = head(
        "GET",
        "session",
        0,
        Some("__Host-FutureClassroom=invalid"),
        None,
    );
    duplicate.headers.push(Header {
        name: "cookie".into(),
        value: b"other=invalid".to_vec(),
    });
    assert_eq!(fixture.dispatch(&duplicate, b"").head.status, 400);
    let mut cross_private = head("GET", "session", 0, None, None);
    cross_private.headers.push(Header {
        name: "Origin".into(),
        value: b"https://evil.school".to_vec(),
    });
    assert_eq!(
        fixture.dispatch(&cross_private, b"").head.status,
        403,
        "preflight rejects cross-origin before missing-cookie authentication"
    );
    assert_eq!(fixture.backend.state().unwrap(), before);
    assert_eq!(
        fixture
            .dispatch(&head("GET", "session", 0, None, None), b"")
            .head
            .status,
        401
    );
}

#[path = "content/tests.rs"]
mod content_tests;
