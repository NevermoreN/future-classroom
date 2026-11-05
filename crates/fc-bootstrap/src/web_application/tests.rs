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
/// Synthetic labels stay in local variable names only. The command identity
/// and owner/family binding below come from the actual authenticated HTTP issuer.
#[derive(Clone, Copy)]
struct TestCommand {
    ticket: fc_ports::command_window::CommandTicket,
    id: fc_domain::ids::CommandId,
}
impl TestCommand {
    fn fields(self) -> String {
        format!(
            r#""schemaVersion":2,"commandTicket":"{}","commandId":"{}""#,
            self.ticket,
            self.id.get()
        )
    }
    fn history(self, family: &str) -> String {
        format!("{family}/commands/{}/{}", self.ticket, self.id.get())
    }
    fn job(self) -> String {
        format!(
            "voice/broadcasts/{}/commands/{}",
            self.id.get(),
            self.ticket
        )
    }
    fn cancel(self) -> String {
        format!("voice/broadcasts/{}/cancel", self.id.get())
    }
}
impl BackendFixture {
    fn issue(&mut self, cookie: &str, family: &str) -> TestCommand {
        let session = self.dispatch(&head("GET", "session", 0, Some(cookie), None), b"");
        assert_eq!(session.head.status, 200);
        let csrf = field(&session.body, "csrf");
        let body = format!(r#"{{"schemaVersion":2,"family":"{family}"}}"#);
        let issued = self.dispatch(
            &head("POST", "commands", body.len(), Some(cookie), Some(&csrf)),
            body.as_bytes(),
        );
        assert_eq!(issued.head.status, 201, "{}", text(&issued.body));
        assert!(text(&issued.body).contains("\"status\":\"ISSUED\""));
        TestCommand {
            ticket: fc_ports::command_window::CommandTicket::parse(&field(
                &issued.body,
                "commandTicket",
            ))
            .unwrap(),
            id: fc_domain::ids::CommandId::new(field(&issued.body, "commandId").parse().unwrap())
                .unwrap(),
        }
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
fn classrooms_are_public_memory_candidates_and_keep_business_authentication() {
    let mut fixture = BackendFixture::new();
    let directory = Arc::new(std::sync::Mutex::new(responses::Classrooms {
        schema_version: 1,
        discovery_available: true,
        rooms: vec![responses::ClassroomEntry {
            address: "https://192.168.10.212:8443/fc/".into(),
            school_name: "校园".into(),
            class_name: Some("七年级一班".into()),
            is_current: true,
        }],
    }));
    let provider = directory.clone();
    fixture.app = fixture
        .app
        .lan_variant(&ListenPrefix::https("https://board.school:443/fc/").unwrap())
        .unwrap()
        .with_classrooms(Arc::new(move || provider.lock().unwrap().clone()));
    let request = head("GET", "classrooms", 0, None, None);
    let response = fixture.dispatch(&request, b"");
    assert_eq!(response.head.status, 200);
    assert!(text(&response.body).contains("\"discoveryAvailable\":true"));
    assert!(text(&response.body).contains("七年级一班"));
    assert!(!text(&response.body).contains(PASSWORD));
    directory.lock().unwrap().rooms.clear();
    assert!(text(&fixture.dispatch(&request, b"").body).contains("\"rooms\":[]"));
    assert_eq!(
        fixture
            .dispatch(&head("POST", "classrooms", 0, None, None), b"")
            .head
            .status,
        405
    );
    assert_eq!(
        fixture
            .dispatch(&head("GET", "session", 0, None, None), b"")
            .head
            .status,
        401
    );
    fixture.backend.close().unwrap();
}

#[test]
fn quick_connect_is_local_public_and_reads_live_url_without_disclosing_accounts() {
    let mut fixture = BackendFixture::new();
    let request = head("GET", "quick-connect", 0, None, None);
    assert_eq!(fixture.dispatch(&request, b"").head.status, 403);
    fixture.app = fixture
        .app
        .local_variant(&ListenPrefix::https("https://board.school:443/fc/").unwrap())
        .unwrap();
    let unavailable = fixture.dispatch(&request, b"");
    assert_eq!(unavailable.head.status, 200);
    assert_eq!(
        field(&unavailable.body, "schoolName"),
        "Synthetic private school"
    );
    assert!(text(&unavailable.body).contains("\"entryUrl\":null"));
    assert!(text(&unavailable.body).contains("\"unavailableReason\":\""));
    let live = Arc::new(std::sync::Mutex::new(Some(
        "https://192.168.10.212:8443/fc/".to_owned(),
    )));
    let provider = live.clone();
    fixture.app = fixture
        .app
        .local_variant(&ListenPrefix::https("https://board.school:443/fc/").unwrap())
        .unwrap()
        .with_quick_connect_url(Arc::new(move || provider.lock().unwrap().clone()));
    let ready = fixture.dispatch(&request, b"");
    assert_eq!(
        field(&ready.body, "entryUrl"),
        "https://192.168.10.212:8443/fc/"
    );
    assert!(text(&ready.body).contains("\"unavailableReason\":null"));
    assert!(!text(&ready.body).contains("admin"));
    assert!(!text(&ready.body).contains(PASSWORD));
    *live.lock().unwrap() = None;
    let stopped = fixture.dispatch(&request, b"");
    assert!(text(&stopped.body).contains("\"entryUrl\":null"));
    let post = head("POST", "quick-connect", 0, None, None);
    assert_eq!(fixture.dispatch(&post, b"").head.status, 405);
    fixture.backend.close().unwrap();
}

#[test]
fn method_rejections_report_exact_allowed_methods_without_changing_other_boundary_errors() {
    let mut fixture = BackendFixture::new();
    for (route, allowed) in [
        ("status", "GET"),
        ("pairing/requests", "POST"),
        ("voice/broadcasts", "GET, POST"),
        ("content/reminders/1/2026-10-31", "GET, POST"),
        ("materials/uploads/1/2", "POST"),
    ] {
        let request = head("DELETE", route, 0, None, None);
        for response in [
            fixture
                .app
                .admit_request(&fixture.backend, &request)
                .err()
                .unwrap(),
            fixture.dispatch(&request, b""),
        ] {
            assert_eq!(response.head.status, 405, "{route}");
            let fields: Vec<_> = response
                .head
                .headers
                .iter()
                .filter(|header| header.name.eq_ignore_ascii_case("allow"))
                .collect();
            assert_eq!(fields.len(), 1, "{route}");
            assert_eq!(fields[0].value, allowed.as_bytes(), "{route}");
        }
    }
    for (mut request, status) in [
        (head("DELETE", "missing", 0, None, None), 404),
        (head("DELETE", "status", 0, None, None), 403),
    ] {
        if status == 403 {
            request.headers[0].value = b"other.school".to_vec();
        }
        let response = fixture.dispatch(&request, b"");
        assert_eq!(response.head.status, status);
        assert!(!response
            .head
            .headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case("allow")));
    }
}

#[test]
fn head_admission_authenticates_before_body_without_creating_pairing_or_sql_work() {
    let mut fixture = BackendFixture::new();
    let revision = fixture.backend.global_revision().unwrap();
    let requested = head("POST", "pairing/requests", 120, None, None);
    assert_eq!(
        fixture.app.admit_request(&fixture.backend, &requested).ok(),
        Some(Route::PairingCreate)
    );
    assert!(fixture.service.pending(fixture.admin).unwrap().is_empty());
    assert_eq!(fixture.backend.global_revision().unwrap(), revision);
    let private = head("POST", "session/logout", 19, None, None);
    assert_eq!(
        fixture
            .app
            .admit_request(&fixture.backend, &private)
            .err()
            .unwrap()
            .head
            .status,
        401
    );
    let cookie = fixture.pair("head-admission", false);
    let session = fixture.dispatch(&head("GET", "session", 0, Some(&cookie), None), b"");
    let csrf = field(&session.body, "csrf");
    let mut private = head("POST", "session/logout", 19, Some(&cookie), Some("wrong"));
    assert_eq!(
        fixture
            .app
            .admit_request(&fixture.backend, &private)
            .err()
            .unwrap()
            .head
            .status,
        403
    );
    private
        .headers
        .iter_mut()
        .find(|h| h.name == "X-FC-CSRF")
        .unwrap()
        .value = csrf.into_bytes();
    assert_eq!(
        fixture.app.admit_request(&fixture.backend, &private).ok(),
        Some(Route::SessionLogout)
    );
    // Admission did not consume the logout or grant a lasting capability.
    assert_eq!(
        fixture
            .dispatch(&head("GET", "session", 0, Some(&cookie), None), b"")
            .head
            .status,
        200
    );
    fixture.service.set_enabled(fixture.admin, false).unwrap();
    assert_eq!(
        fixture
            .dispatch(&private, br#"{"schemaVersion":1}"#)
            .head
            .status,
        503
    );
}

#[test]
fn head_admission_rejects_another_backend_and_rechecks_sensitive_headers() {
    let mut first = BackendFixture::new();
    let second = BackendFixture::new();
    let cookie = first.pair("admission-installation", false);
    let mut request = head("GET", "session", 0, Some(&cookie), None);
    assert_eq!(
        first
            .app
            .admit_request(&second.backend, &request)
            .err()
            .unwrap()
            .head
            .status,
        503
    );
    request.headers.push(Header {
        name: "Cookie".into(),
        value: cookie.as_bytes().to_vec(),
    });
    assert_eq!(
        first
            .app
            .admit_request(&first.backend, &request)
            .err()
            .unwrap()
            .head
            .status,
        400
    );
    request.headers.pop();
    request.tls = false;
    assert_eq!(
        first
            .app
            .admit_request(&first.backend, &request)
            .err()
            .unwrap()
            .head
            .status,
        403
    );
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
    let voice = fixture.dispatch(
        &head("GET", "voice/capabilities", 0, Some(&first), None),
        b"",
    );
    assert_eq!(voice.head.status, 200);
    let voice_body = text(&voice.body);
    for expected in [
        "\"status\":\"VOICE_CAPABILITIES\"",
        "\"currentClass\":null",
        "\"targetAuthorized\":false",
        "\"deviceAvailability\":\"UNVERIFIED\"",
    ] {
        assert!(voice_body.contains(expected));
    }
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
        r#"{"schemaVersion":1,"apiVersion":1,"uiCompatibilityVersion":1,"online":true,"initialized":true}"#
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

#[path = "materials_tests.rs"]
mod material_tests;

#[path = "voice_tests.rs"]
mod voice_tests;

fn page_head(method: &str, route: &str, len: usize, page: &str, csrf: Option<&str>) -> RequestHead {
    let mut request = head(method, route, len, None, csrf);
    request.headers.push(Header {
        name: "X-FC-Page".into(),
        value: page.as_bytes().to_vec(),
    });
    request
}
fn direct_page_login(fixture: &mut BackendFixture) -> (String, String) {
    let body = format!(r#"{{"schemaVersion":1,"loginName":"admin","password":"{PASSWORD}"}}"#);
    let response = fixture.dispatch(
        &head("POST", "session/login", body.len(), None, None),
        body.as_bytes(),
    );
    assert_eq!(response.head.status, 200);
    assert!(response
        .head
        .headers
        .iter()
        .all(|h| !h.name.eq_ignore_ascii_case("set-cookie")));
    (
        field(&response.body, "pageToken"),
        field(&response.body, "csrf"),
    )
}
#[test]
fn direct_page_login_needs_no_pairing_and_cookie_cannot_restore_a_closed_page() {
    let mut fixture = BackendFixture::new();
    let (page, _) = direct_page_login(&mut fixture);
    let response = fixture.dispatch(&page_head("GET", "session", 0, &page, None), b"");
    assert_eq!(response.head.status, 200);
    assert_eq!(field(&response.body, "role"), "GLOBAL_ADMIN");
    let cookie = format!("__Host-FutureClassroom={page}");
    assert_eq!(
        fixture
            .dispatch(&head("GET", "session", 0, Some(&cookie), None), b"")
            .head
            .status,
        401
    );
    assert_eq!(
        fixture
            .dispatch(&head("GET", "session", 0, None, None), b"")
            .head
            .status,
        401
    );
}
#[test]
fn page_resume_survives_idle_access_expiry_and_midnight_without_reviving_revoked_identity() {
    let mut fixture = BackendFixture::new();
    let (first, _) = direct_page_login(&mut fixture);
    let (second, _) = direct_page_login(&mut fixture);
    fixture.clock.0.fetch_add(86_401_000, Ordering::AcqRel);
    fixture.clock.1.store(90_000, Ordering::Release);
    for token in [&first, &second] {
        let body = format!(r#"{{"schemaVersion":1,"pageToken":"{token}"}}"#);
        assert_eq!(
            fixture
                .dispatch(
                    &head("POST", "session/resume", body.len(), None, None),
                    body.as_bytes()
                )
                .head
                .status,
            200
        );
    }
    let session = fixture.dispatch(&page_head("GET", "session", 0, &first, None), b"");
    let csrf = field(&session.body, "csrf");
    let body = br#"{"schemaVersion":1}"#;
    assert_eq!(
        fixture
            .dispatch(
                &page_head("POST", "session/logout", body.len(), &first, Some(&csrf)),
                body
            )
            .head
            .status,
        200
    );
    assert_eq!(
        fixture
            .dispatch(&page_head("GET", "session", 0, &second, None), b"")
            .head
            .status,
        401
    );
}
#[test]
fn page_login_rejects_wrong_password_and_private_writes_still_require_csrf() {
    let mut fixture = BackendFixture::new();
    let wrong = br#"{"schemaVersion":1,"loginName":"admin","password":"Incorrect-password-2026"}"#;
    assert_eq!(
        fixture
            .dispatch(
                &head("POST", "session/login", wrong.len(), None, None),
                wrong
            )
            .head
            .status,
        401
    );
    let (page, _) = direct_page_login(&mut fixture);
    let body = br#"{"schemaVersion":1}"#;
    assert_eq!(
        fixture
            .dispatch(
                &page_head("POST", "session/logout", body.len(), &page, None),
                body
            )
            .head
            .status,
        403
    );
}

#[test]
fn page_password_initial_login_is_restricted_and_change_returns_new_revoking_session() {
    use fc_domain::{
        auth::Role,
        identity::{AssignmentScope, IdentityCommand, NewAccount, Rgb},
        ids::{ClassId, PrincipalId, SubjectId},
    };
    let mut fixture = BackendFixture::new();
    let initial = "Initial-teacher-password-2026";
    let changed = "Changed-teacher-password-2026";
    let admin = fixture.admin;
    for command in [
        IdentityCommand::CreateClass {
            id: ClassId::new(1).unwrap(),
            name: "Test class".into(),
        },
        IdentityCommand::PutSubject {
            id: SubjectId::new(1).unwrap(),
            name: "Math".into(),
            enabled: true,
            color: Rgb {
                red: 20,
                green: 80,
                blue: 180,
            },
            expected_revision: None,
        },
    ] {
        fixture
            .backend
            .test_native_command(
                fc_ports::command_window::CommandFamily::Identity,
                Some(admin),
                SessionPurpose::Manage,
                |backend| backend.identity(admin, command),
            )
            .unwrap();
    }

    drop(
        fixture
            .backend
            .test_native_command(
                fc_ports::command_window::CommandFamily::Identity,
                Some(admin),
                SessionPurpose::Manage,
                |backend| {
                    backend.create_account(
                        admin,
                        NewAccount {
                            id: PrincipalId::new(2).unwrap(),
                            login_name: "newteacher".into(),
                            display_name: "New teacher".into(),
                            role: Role::TeacherAdmin,
                            scopes: vec![AssignmentScope {
                                class_id: ClassId::new(1).unwrap(),
                                subject_id: SubjectId::new(1).unwrap(),
                            }],
                        },
                        Password::new(initial.into()).unwrap(),
                    )
                },
            )
            .unwrap(),
    );
    let body = format!(r#"{{"schemaVersion":1,"loginName":"newteacher","password":"{initial}"}}"#);
    let login = fixture.dispatch(
        &head("POST", "session/login", body.len(), None, None),
        body.as_bytes(),
    );
    assert_eq!(login.head.status, 200);
    assert!(text(&login.body).contains(r#""mustChangePassword":true"#));
    let page = field(&login.body, "pageToken");
    let csrf = field(&login.body, "csrf");
    assert_eq!(
        fixture
            .dispatch(&page_head("GET", "voice/capabilities", 0, &page, None), b"")
            .head
            .status,
        403
    );
    let wrong = format!(
        r#"{{"schemaVersion":1,"currentPassword":"Incorrect-password-2026","newPassword":"{changed}"}}"#
    );
    assert_eq!(
        fixture
            .dispatch(
                &page_head("POST", "session/password", wrong.len(), &page, Some(&csrf)),
                wrong.as_bytes()
            )
            .head
            .status,
        403
    );
    let change =
        format!(r#"{{"schemaVersion":1,"currentPassword":"{initial}","newPassword":"{changed}"}}"#);
    let response = fixture.dispatch(
        &page_head("POST", "session/password", change.len(), &page, Some(&csrf)),
        change.as_bytes(),
    );
    assert_eq!(response.head.status, 200);
    assert!(text(&response.body).contains(r#""mustChangePassword":false"#));
    assert_eq!(field(&response.body, "role"), "TEACHER_ADMIN");
    let fresh = field(&response.body, "pageToken");
    assert_ne!(fresh, page);
    assert_eq!(
        fixture
            .dispatch(&page_head("GET", "session", 0, &page, None), b"")
            .head
            .status,
        401
    );
    assert_eq!(
        fixture
            .dispatch(&page_head("GET", "session", 0, &fresh, None), b"")
            .head
            .status,
        200
    );
}

#[test]
fn local_setup_initializes_once_and_returns_recovery_code_only_once() {
    let directory = Directory::new();
    let clock = Arc::new(Time(AtomicU64::new(1000), AtomicU64::new(36000)));
    let mut backend = BackendFixture::open(&directory, clock);
    let prefix = ListenPrefix::https("https://board.school:443/fc/").unwrap();
    let app = WebApplication::new_local(&prefix, backend.web_service()).unwrap();
    let normal = WebApplication::new(&prefix, backend.web_service()).unwrap();
    let status = app.dispatch(&mut backend, &head("GET", "status", 0, None, None), b"");
    assert!(text(&status.body).contains(r#""initialized":false"#));
    let body = format!(
        r#"{{"schemaVersion":1,"schoolName":"Test setup","loginName":"admin","displayName":"Administrator","password":"{PASSWORD}"}}"#
    );
    assert_eq!(
        normal
            .dispatch(
                &mut backend,
                &head("POST", "setup", body.len(), None, None),
                body.as_bytes()
            )
            .head
            .status,
        403
    );
    let created = app.dispatch(
        &mut backend,
        &head("POST", "setup", body.len(), None, None),
        body.as_bytes(),
    );
    assert_eq!(created.head.status, 200);
    assert!(field(&created.body, "recoveryCode").starts_with("FC1-"));
    let page = field(&created.body, "pageToken");
    assert_eq!(
        app.dispatch(
            &mut backend,
            &page_head("GET", "session", 0, &page, None),
            b""
        )
        .head
        .status,
        200
    );
    assert_eq!(
        app.dispatch(
            &mut backend,
            &head("POST", "setup", body.len(), None, None),
            body.as_bytes()
        )
        .head
        .status,
        409
    );
    backend.close().unwrap();
}
#[test]
fn web_global_admin_updates_display_with_cas_and_identity_excludes_credential_metadata() {
    let mut fixture = BackendFixture::new();
    let (page, csrf) = direct_page_login(&mut fixture);
    let identity = fixture.dispatch(&page_head("GET", "admin/identity", 0, &page, None), b"");
    assert_eq!(identity.head.status, 200);
    for forbidden in [
        "credentialVersion",
        "recoveryVersion",
        "protectedRecord",
        "authEpoch",
    ] {
        assert!(!text(&identity.body).contains(forbidden));
    }
    let original = fixture.dispatch(&page_head("GET", "admin/display", 0, &page, None), b"");
    assert_eq!(original.head.status, 200);
    let json = text(&original.body);
    let revision = field(&original.body, "revision");
    let start = json.find("\"data\":").unwrap() + 7;
    let candidate = &json[start..json.len() - 1];
    let changed = candidate.replace("\"glassSurfacePercent\":3", "\"glassSurfacePercent\":9");
    let body = format!(
        r#"{{"schemaVersion":1,"expectedRevision":"{revision}","action":"save","candidate":{changed}}}"#
    );
    let saved = fixture.dispatch(
        &page_head("POST", "admin/display", body.len(), &page, Some(&csrf)),
        body.as_bytes(),
    );
    assert_eq!(saved.head.status, 200);
    assert!(text(&saved.body).contains("\"glassSurfacePercent\":9"));
    assert_eq!(
        fixture
            .dispatch(
                &page_head("POST", "admin/display", body.len(), &page, Some(&csrf)),
                body.as_bytes()
            )
            .head
            .status,
        409
    );
}

#[test]
fn web_content_text_publish_clear_and_exact_undo_use_existing_public_node_writer() {
    use fc_domain::{
        ids::LayoutNodeId,
        layout::{LayoutNode, NodeKind, WidgetKind},
        profile_content::{ContentNodeStyle, ContentPayloadKind},
    };
    let mut fixture = BackendFixture::new();
    let admin = fixture.admin;
    let current = fixture.backend.read_public_layout(admin).unwrap();
    let mut candidate = current.layout.clone();
    let node = LayoutNodeId::new(candidate.node_id_high_water + 1).unwrap();
    let group = LayoutNodeId::new(node.get() + 1).unwrap();
    candidate.node_id_high_water = group.get();
    let outlet = candidate
        .tree
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
        .unwrap();
    let mut children = std::mem::replace(&mut outlet.children, vec![group]);
    children.push(node);
    candidate.tree.nodes.push(LayoutNode {
        id: group,
        kind: NodeKind::Container(fc_domain::layout::ContainerSettings {
            flow: fc_domain::layout::Flow::Column,
            padding: 8,
            gap: 8,
            width: fc_domain::layout::AxisSize::Fixed(500),
            height: fc_domain::layout::AxisSize::Fixed(800),
            group_opacity_percent: 100,
            material: Some(fc_domain::layout::GlassMaterial::default()),
        }),
        children,
    });
    candidate.tree.nodes.push(LayoutNode {
        id: node,
        kind: NodeKind::Widget(WidgetKind::SubjectText),
        children: vec![],
    });
    candidate
        .content_styles
        .push(ContentNodeStyle::new(node, ContentPayloadKind::Text));
    fixture
        .backend
        .test_native_command(
            fc_ports::command_window::CommandFamily::Profiles,
            Some(admin),
            SessionPurpose::Manage,
            |backend| {
                backend.save_public_layout(
                    admin,
                    current.layout.revision,
                    current.display.revision,
                    candidate,
                    current.display,
                )
            },
        )
        .unwrap();
    let (page, csrf) = direct_page_login(&mut fixture);
    let path = format!("admin/content/public/{}", node.get());
    let body=br#"{"schemaVersion":1,"expectedRevision":"0","action":"publishText","candidate":{"text":"Structured text\nsecond line"}}"#;
    let published = fixture.dispatch(
        &page_head("POST", &path, body.len(), &page, Some(&csrf)),
        body,
    );
    assert_eq!(published.head.status, 200);
    assert!(text(&published.body).contains("Structured text\\nsecond line"));
    let revision = field(&published.body, "revision");
    let clear = format!(
        r#"{{"schemaVersion":1,"expectedRevision":"{revision}","action":"clear","candidate":{{}}}}"#
    );
    let cleared = fixture.dispatch(
        &page_head("POST", &path, clear.len(), &page, Some(&csrf)),
        clear.as_bytes(),
    );
    assert_eq!(cleared.head.status, 200);
    let next = field(&cleared.body, "revision");
    let undo = format!(
        r#"{{"schemaVersion":1,"expectedRevision":"{next}","action":"undo","candidate":{{"targetRevision":"{revision}"}}}}"#
    );
    let restored = fixture.dispatch(
        &page_head("POST", &path, undo.len(), &page, Some(&csrf)),
        undo.as_bytes(),
    );
    assert_eq!(restored.head.status, 200);
    assert!(text(&restored.body).contains("Structured text\\nsecond line"));
}

#[test]
fn web_global_voice_policy_keeps_real_origin_and_writer_review_cas() {
    use fc_domain::{auth::TrustedOrigin, voice::VoiceLimits};
    let mut fixture = BackendFixture::new();
    let (page, _csrf) = direct_page_login(&mut fixture);
    let proof = PageToken::parse_hex(page).unwrap();
    let lease = fixture.service.authenticate_page(&proof).unwrap().lease();
    assert_eq!(lease.origin(), TrustedOrigin::Web);
    let service = fixture.backend.voice_service();
    let before = service.management_snapshot(lease).unwrap();
    let issued = fixture
        .backend
        .issue_for_web_admin(lease, fc_ports::command_window::CommandFamily::Voice)
        .unwrap();
    let scoped = service.with_command_window(lease, issued).unwrap();
    let limits = VoiceLimits {
        max_duration_millis: 30_000,
        max_pending_jobs: 3,
        max_pending_per_owner: 1,
        max_repeat_total: 4,
    };
    scoped
        .set_limits_reviewed(
            lease,
            limits,
            before.revision,
            before.identity_revision,
            before.runtime_generation,
        )
        .unwrap();
    let after = service.management_snapshot(lease).unwrap();
    assert_eq!(after.limits, limits);
    assert!(after.revision > before.revision);
    let issued = fixture
        .backend
        .issue_for_web_admin(lease, fc_ports::command_window::CommandFamily::Voice)
        .unwrap();
    let scoped = service.with_command_window(lease, issued).unwrap();
    assert!(scoped
        .set_limits_reviewed(
            lease,
            VoiceLimits::default(),
            before.revision,
            before.identity_revision,
            before.runtime_generation
        )
        .is_err());
    assert_eq!(service.management_snapshot(lease).unwrap().limits, limits);
}

#[cfg(windows)]
#[test]
fn web_images_real_wic_publish_range_and_cancel_retire_controlled_owners() {
    use crate::web_transfer::{
        uploads::tests::{Fixture, Pool},
        TransferControl, WebCompletion, WebTask,
    };
    use fc_domain::{
        ids::LayoutNodeId,
        layout::{LayoutNode, NodeKind, WidgetKind},
        profile_content::{ContentNodeStyle, ContentPayloadKind},
    };
    use fc_runtime::worker::Task;
    use std::time::Instant;
    let _physical = super::upload_test_guard();
    let fixture = Fixture::new();
    let mut pool = Pool::new();
    let password = "Synthetic-Upload-Hub-Password-2026";
    let login = format!(r#"{{"schemaVersion":1,"loginName":"admin","password":"{password}"}}"#);
    let logged = fixture.app.dispatch(
        &mut fixture.backend.lock().unwrap(),
        &head("POST", "session/login", login.len(), None, None),
        login.as_bytes(),
    );
    assert_eq!(logged.head.status, 200, "{}", text(&logged.body));
    let (page, csrf) = (
        field(&logged.body, "pageToken"),
        field(&logged.body, "csrf"),
    );
    let lease = fixture
        .app
        .service
        .authenticate_page(&PageToken::parse_hex(page.clone()).unwrap())
        .unwrap()
        .lease();
    let node = {
        let mut backend = fixture.backend.lock().unwrap();
        let current = backend.read_public_layout(lease).unwrap();
        let mut candidate = current.layout.clone();
        let node = LayoutNodeId::new(candidate.node_id_high_water + 1).unwrap();
        let group = LayoutNodeId::new(node.get() + 1).unwrap();
        candidate.node_id_high_water = group.get();
        let outlet = candidate
            .tree
            .nodes
            .iter_mut()
            .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
            .unwrap();
        let mut children = std::mem::replace(&mut outlet.children, vec![group]);
        children.push(node);
        candidate.tree.nodes.push(LayoutNode {
            id: group,
            kind: NodeKind::Container(fc_domain::layout::ContainerSettings {
                flow: fc_domain::layout::Flow::Column,
                padding: 8,
                gap: 8,
                width: fc_domain::layout::AxisSize::Fixed(500),
                height: fc_domain::layout::AxisSize::Fixed(800),
                group_opacity_percent: 100,
                material: Some(fc_domain::layout::GlassMaterial::default()),
            }),
            children,
        });
        candidate.tree.nodes.push(LayoutNode {
            id: node,
            kind: NodeKind::Widget(WidgetKind::SubjectImage),
            children: vec![],
        });
        candidate
            .content_styles
            .push(ContentNodeStyle::new(node, ContentPayloadKind::Images));
        let issued = backend
            .issue_for_web_admin(lease, fc_ports::command_window::CommandFamily::Profiles)
            .unwrap();
        backend
            .begin_command_scope(issued)
            .unwrap()
            .save_public_layout(
                lease,
                current.layout.revision,
                current.display.revision,
                candidate,
                current.display,
            )
            .unwrap();
        node
    };
    let request = |request: RequestHead, body: Vec<u8>| {
        let (control, life) = TransferControl::new(request.token, Instant::now()).unwrap();
        let task = WebTask::json(&fixture.backend, &fixture.app, request, body, life).unwrap();
        let response = pool.call(move |context| match task.run(context) {
            WebCompletion::Response { response, .. } => response,
            _ => panic!("expected image response"),
        });
        drop(control);
        response
    };
    let expect = |response: WebResponse, status| {
        assert_eq!(response.head.status, status, "{}", text(&response.body));
        response
    };
    let png: Vec<u8> = vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240,
        31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let slot = format!("admin/content/public/{}", node.get());
    let manifest = format!(
        r#"{{"schemaVersion":1,"expectedRevision":"0","items":[{{"name":"pixel.png","bytes":{}}}]}}"#,
        png.len()
    );
    let created = expect(
        request(
            page_head(
                "POST",
                &format!("{slot}/uploads"),
                manifest.len(),
                &page,
                Some(&csrf),
            ),
            manifest.into_bytes(),
        ),
        201,
    );
    let batch = field(&created.body, "batchId");
    let upload = field(&created.body, "id");
    for (offset, part) in [
        (0, png[..png.len() / 2].to_vec()),
        (png.len() / 2, png[png.len() / 2..].to_vec()),
    ] {
        let mut h = page_head(
            "POST",
            &format!("{slot}/uploads/{batch}/{upload}/bytes"),
            part.len(),
            &page,
            Some(&csrf),
        );
        h.headers
            .iter_mut()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .unwrap()
            .value = b"application/octet-stream".to_vec();
        h.headers.push(Header {
            name: "X-FC-Upload-Offset".into(),
            value: offset.to_string().into_bytes(),
        });
        expect(request(h, part), 200);
    }
    let publish = format!(
        r#"{{"schemaVersion":1,"expectedRevision":"0","action":"publishImages","candidate":{{"batchId":"{batch}","pages":[{{"source":"upload","uploadId":"{upload}"}}]}}}}"#
    );
    let saved = expect(
        request(
            page_head("POST", &slot, publish.len(), &page, Some(&csrf)),
            publish.into_bytes(),
        ),
        200,
    );
    assert!(text(&saved.body).contains("\"format\":\"png\""));
    assert!(text(&saved.body).contains("\"width\":1"));
    let revision = field(&saved.body, "revision");
    let media = field(&saved.body, "mediaVersionId");
    let mut downloaded = Vec::new();
    for (start, end) in [(0, png.len() / 2 - 1), (png.len() / 2, png.len() - 1)] {
        let mut h = page_head("GET", &format!("{slot}/media/{media}"), 0, &page, None);
        h.headers.push(Header {
            name: "Range".into(),
            value: format!("bytes={start}-{end}").into_bytes(),
        });
        let read = expect(request(h, vec![]), 206);
        assert_eq!(read.head.content_length, (end - start + 1) as u64);
        assert!(read
            .head
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value == b"image/png"));
        downloaded.extend(read.body);
    }
    assert_eq!(downloaded, png);
    assert_eq!(fc_security::sha256(&downloaded), fc_security::sha256(&png));
    let manifest = format!(
        r#"{{"schemaVersion":1,"expectedRevision":"{revision}","items":[{{"name":"cancelled.png","bytes":{}}}]}}"#,
        png.len()
    );
    let created = expect(
        request(
            page_head(
                "POST",
                &format!("{slot}/uploads"),
                manifest.len(),
                &page,
                Some(&csrf),
            ),
            manifest.into_bytes(),
        ),
        201,
    );
    let batch = field(&created.body, "batchId");
    let cancel = br#"{"schemaVersion":1}"#;
    expect(
        request(
            page_head(
                "POST",
                &format!("{slot}/uploads/{batch}/cancel"),
                cancel.len(),
                &page,
                Some(&csrf),
            ),
            cancel.to_vec(),
        ),
        200,
    );
    let app = fixture.app.clone();
    assert!(pool.call(move |_| app.cleanup_profile_images()));
    assert_eq!(super::quarantined_uploads(), 0);
    pool.join().unwrap();
    // Fixture Drop closes the real writer and removes its isolated controlled directory.
}

#[test]
fn web_account_security_preserves_recovery_and_current_owner_transfer_receipt() {
    let mut fixture = BackendFixture::new();
    let prefix = ListenPrefix::https("https://board.school:443/fc/").unwrap();
    fixture.app = WebApplication::new_local(&prefix, fixture.service.clone()).unwrap();
    let (page, csrf) = direct_page_login(&mut fixture);
    let revision = fixture.backend.state().unwrap().revision.get();
    let create = format!(
        r#"{{"schemaVersion":1,"expectedRevision":"{revision}","action":"createAccount","candidate":{{"loginName":"recipient","displayName":"Recipient","role":"USER","password":"Initial-recipient-password-2026","scopes":[]}}}}"#
    );
    let result = fixture.dispatch(
        &page_head("POST", "admin/identity", create.len(), &page, Some(&csrf)),
        create.as_bytes(),
    );
    assert_eq!(result.head.status, 200, "{}", text(&result.body));
    assert!(field(&result.body, "recoveryCode").starts_with("FC1-"));
    let recipient_id = fixture
        .backend
        .state()
        .unwrap()
        .account_by_login("recipient")
        .unwrap()
        .principal
        .id
        .get();
    let login = r#"{"schemaVersion":1,"loginName":"recipient","password":"Initial-recipient-password-2026"}"#;
    let response = fixture.dispatch(
        &head("POST", "session/login", login.len(), None, None),
        login.as_bytes(),
    );
    let recipient_page = field(&response.body, "pageToken");
    let recipient_csrf = field(&response.body, "csrf");
    let change = r#"{"schemaVersion":1,"currentPassword":"Initial-recipient-password-2026","newPassword":"Changed-recipient-password-2026"}"#;
    assert_eq!(
        fixture
            .dispatch(
                &page_head(
                    "POST",
                    "session/password",
                    change.len(),
                    &recipient_page,
                    Some(&recipient_csrf)
                ),
                change.as_bytes()
            )
            .head
            .status,
        200
    );
    let revision = fixture.backend.state().unwrap().revision.get();
    let prepare = format!(
        r#"{{"schemaVersion":1,"action":"prepareTransfer","expectedRevision":"{revision}","targetId":"{recipient_id}"}}"#
    );
    let remote = WebApplication::new(&prefix, fixture.service.clone()).unwrap();
    assert_eq!(
        remote
            .dispatch(
                &mut fixture.backend,
                &page_head(
                    "POST",
                    "admin/account-security",
                    prepare.len(),
                    &page,
                    Some(&csrf)
                ),
                prepare.as_bytes()
            )
            .head
            .status,
        403
    );
    let response = fixture.dispatch(
        &page_head(
            "POST",
            "admin/account-security",
            prepare.len(),
            &page,
            Some(&csrf),
        ),
        prepare.as_bytes(),
    );
    assert_eq!(response.head.status, 200, "{}", text(&response.body));
    let ticket = field(&response.body, "ticket");
    let transfer = format!(
        r#"{{"schemaVersion":1,"action":"transfer","ticket":"{ticket}","currentPassword":"{PASSWORD}","reviewedTarget":true,"understoodRevocation":true}}"#
    );
    let response = fixture.dispatch(
        &page_head(
            "POST",
            "admin/account-security",
            transfer.len(),
            &page,
            Some(&csrf),
        ),
        transfer.as_bytes(),
    );
    assert_eq!(response.head.status, 200, "{}", text(&response.body));
    assert!(text(&response.body).contains("COMMITTED"));
    assert_eq!(
        fixture
            .dispatch(&page_head("GET", "session", 0, &page, None), b"")
            .head
            .status,
        401
    );
    let (fresh, fresh_csrf) = direct_page_login(&mut fixture);
    assert_eq!(
        fixture
            .dispatch(&page_head("GET", "admin/identity", 0, &fresh, None), b"")
            .head
            .status,
        403
    );
    let query = format!(r#"{{"schemaVersion":1,"action":"outcome","ticket":"{ticket}"}}"#);
    let original = fixture.dispatch(
        &page_head(
            "POST",
            "admin/account-security",
            query.len(),
            &fresh,
            Some(&fresh_csrf),
        ),
        query.as_bytes(),
    );
    assert_eq!(original.head.status, 200, "{}", text(&original.body));
    assert!(text(&original.body).contains("COMMITTED"));
    assert!(!text(&original.body).contains("Recipient"));
    fixture.backend.close().unwrap();
}

#[test]
fn web_recovery_rotation_and_forgot_password_use_original_single_use_credential_flow() {
    let mut fixture = BackendFixture::new();
    let prefix = ListenPrefix::https("https://board.school:443/fc/").unwrap();
    fixture.app = WebApplication::new_local(&prefix, fixture.service.clone()).unwrap();
    let (page, csrf) = direct_page_login(&mut fixture);
    let rotate = format!(
        r#"{{"schemaVersion":1,"action":"rotateRecovery","currentPassword":"{PASSWORD}"}}"#
    );
    let response = fixture.dispatch(
        &page_head(
            "POST",
            "admin/account-security",
            rotate.len(),
            &page,
            Some(&csrf),
        ),
        rotate.as_bytes(),
    );
    assert_eq!(response.head.status, 200, "{}", text(&response.body));
    let code = field(&response.body, "recoveryCode");
    assert_eq!(
        fixture
            .dispatch(&page_head("GET", "session", 0, &page, None), b"")
            .head
            .status,
        401
    );
    let recover = format!(
        r#"{{"schemaVersion":1,"loginName":"admin","recoveryCode":"{code}","newPassword":"Recovered-admin-password-2026"}}"#
    );
    let remote = WebApplication::new(&prefix, fixture.service.clone()).unwrap();
    assert_eq!(
        remote
            .dispatch(
                &mut fixture.backend,
                &head("POST", "session/recover", recover.len(), None, None),
                recover.as_bytes()
            )
            .head
            .status,
        403
    );
    let response = fixture.dispatch(
        &head("POST", "session/recover", recover.len(), None, None),
        recover.as_bytes(),
    );
    assert_eq!(response.head.status, 200, "{}", text(&response.body));
    assert_ne!(field(&response.body, "recoveryCode"), code);
    assert_eq!(
        fixture
            .dispatch(
                &head("POST", "session/recover", recover.len(), None, None),
                recover.as_bytes()
            )
            .head
            .status,
        403
    );
    let login =
        r#"{"schemaVersion":1,"loginName":"admin","password":"Recovered-admin-password-2026"}"#;
    assert_eq!(
        fixture
            .dispatch(
                &head("POST", "session/login", login.len(), None, None),
                login.as_bytes()
            )
            .head
            .status,
        200
    );
    fixture.backend.close().unwrap();
}
