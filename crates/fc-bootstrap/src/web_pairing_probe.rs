//! Owned Windows CNG/DPAPI/SQLite and authenticated request-port diagnostic.
//! It neither opens a listener nor claims wire TLS/browser acceptance.
use fc_application::identity::SessionPurpose;
use fc_bootstrap::{
    backend::{
        web::{session_cookie, PairingPoll, WebServiceError},
        Backend, Initialize,
    },
    web_application::WebApplication,
};
use fc_domain::{
    auth::Role,
    identity::{AssignmentScope, IdentityCommand, Rgb},
    ids::{ClassId, SubjectId},
};
use fc_ports::http::*;
use fc_security::{web::PairingCode, CredentialRecordCodec, Password};
use fc_windows::{
    system::{CurrentClock, CurrentUserDpapi, DataPaths, SecretBytes},
    SystemRandom,
};
use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
mod content_probe;
mod materials_probe;

fn check<T, E: std::fmt::Debug>(r: Result<T, E>) -> Result<T, String> {
    r.map_err(|e| format!("web-pairing: {e:?}"))
}
fn require(value: bool, message: &str) -> Result<(), String> {
    if value {
        Ok(())
    } else {
        Err(format!("web-pairing: {message}"))
    }
}
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn head(
    method: &str,
    route: &str,
    body: &[u8],
    cookie: Option<&str>,
    csrf: Option<&str>,
) -> Result<RequestHead, String> {
    let mut issuer = check(TokenIssuer::new())?;
    let mut headers = vec![
        Header {
            name: "Host".into(),
            value: b"board.synthetic".to_vec(),
        },
        Header {
            name: "Origin".into(),
            value: b"https://board.synthetic".to_vec(),
        },
        Header {
            name: "Sec-Fetch-Site".into(),
            value: b"same-origin".to_vec(),
        },
    ];
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
    let content_length = if method == "POST" {
        headers.push(Header {
            name: "Content-Length".into(),
            value: body.len().to_string().into_bytes(),
        });
        headers.push(Header {
            name: "Content-Type".into(),
            value: b"application/json".to_vec(),
        });
        Some(body.len() as u64)
    } else {
        None
    };
    Ok(RequestHead {
        token: check(issuer.issue(0))?,
        method: method.into(),
        target: format!("/fc/api/v1/{route}"),
        headers,
        tls: true,
        content_length,
        has_body: !body.is_empty(),
    })
}
pub fn run() -> Result<(), String> {
    std::thread::Builder::new()
        .name("fc-web-pairing-probe".into())
        .spawn(run_owned)
        .map_err(|_| "web pairing probe thread unavailable")?
        .join()
        .map_err(|_| "web pairing probe panicked")?
}
fn run_owned() -> Result<(), String> {
    let stamp = check(SystemTime::now().duration_since(UNIX_EPOCH))?.as_nanos();
    let root = Cleanup(std::env::temp_dir().join(format!(
        "FutureClassroom-Test-WebPairing-{}-{stamp}",
        std::process::id()
    )));
    let deployment =
        check(check(DataPaths::isolated_test(root.0.clone()))?.prepare_for_current_user())?;
    let codec = check(CredentialRecordCodec::generate(&mut SystemRandom))?;
    let dpapi = CurrentUserDpapi::default();
    let encrypted =
        check(codec.protect_key(|key| dpapi.protect(&SecretBytes::new(key.to_vec())?)))?;
    drop(codec);
    let key = check(dpapi.unprotect(&encrypted))?;
    let bytes: [u8; 32] = check(key.expose().try_into())?;
    let clock = Arc::new(CurrentClock::new());
    let path = root.0.join("state/web-pairing.sqlite");
    let mut backend = check(Backend::open(
        &path,
        check(CredentialRecordCodec::from_key(bytes))?,
        clock.clone(),
        clock.clone(),
    ))?;
    drop(key);
    const PASSWORD: &str = "Synthetic Web pairing password 2026!";
    drop(check(backend.initialize(
        Initialize {
            school_name: "Synthetic pairing school".into(),
            login_name: "probe".into(),
            display_name: "Synthetic pairing administrator".into(),
        },
        check(Password::new(PASSWORD.into()))?,
    ))?);
    let local = check(backend.login("probe", check(Password::new(PASSWORD.into()))?))?;
    let class = check(ClassId::new(7))?;
    let subject = check(SubjectId::new(9))?;
    check(backend.identity(
        local,
        IdentityCommand::CreateClass {
            id: class,
            name: "Synthetic content class".into(),
        },
    ))?;
    check(backend.identity(local, IdentityCommand::BindCurrentClass { id: class }))?;
    check(backend.identity(
        local,
        IdentityCommand::PutSubject {
            id: subject,
            name: "Synthetic content subject".into(),
            enabled: true,
            color: Rgb {
                red: 30,
                green: 80,
                blue: 150,
            },
            expected_revision: None,
        },
    ))?;
    check(backend.identity(
        local,
        IdentityCommand::UpdateAccount {
            id: local.principal_id(),
            display_name: "Synthetic pairing administrator".into(),
            role: Role::GlobalAdmin,
            enabled: true,
            scopes: vec![AssignmentScope {
                class_id: class,
                subject_id: subject,
            }],
        },
    ))?;
    // Updating assignments revokes the original local epoch. Do not transfer it
    // to the new pairing window or bypass the ordinary password login.
    let local = check(backend.login("probe", check(Password::new(PASSWORD.into()))?))?;
    let service = backend.web_service();
    require(
        matches!(service.begin_pairing(local), Err(WebServiceError::Disabled)),
        "Web admission was enabled by default",
    )?;
    check(service.set_enabled(local, true))?;
    let window = check(service.begin_pairing(local))?;
    let code = check(PairingCode::parse(
        window.code.expose_text().as_str().to_owned(),
    ))?;
    let receipt = check(service.request_pairing(code, "probe", "Synthetic browser"))?;
    let unrelated_proof = check(fc_security::web::WebSecret::generate(&mut SystemRandom))?;
    require(
        matches!(
            service.poll(receipt.request_id, &unrelated_proof),
            Err(WebServiceError::Denied)
        ),
        "unrelated poll proof was accepted",
    )?;
    require(
        matches!(
            check(service.poll(receipt.request_id, &receipt.poll_secret))?,
            PairingPoll::Pending
        ),
        "short code authorized before local approval",
    )?;
    let review = check(service.pending(local))?;
    require(
        review.len() == 1 && review[0].request_id == receipt.request_id,
        "local review request mismatch",
    )?;
    check(service.approve(local, receipt.request_id, review[0].identity_revision))?;
    std::thread::sleep(Duration::from_millis(550));
    let PairingPoll::Granted(issued) =
        check(service.poll(receipt.request_id, &receipt.poll_secret))?
    else {
        return Err("approved request was not collectable".into());
    };
    require(
        service
            .poll(receipt.request_id, &receipt.poll_secret)
            .is_err(),
        "pairing collection was replayable",
    )?;
    let authentication = check(service.authenticate(&issued.cookie))?;
    let lease = authentication.lease();
    require(
        !issued
            .cookie
            .verifier()
            .verify(&check(fc_security::web::WebSecret::parse_hex(
                authentication.csrf.as_str().into(),
            ))?),
        "CSRF was the bearer cookie",
    )?;
    let mut application = check(WebApplication::new(
        &check(ListenPrefix::https("https://board.synthetic:443/fc/"))?,
        service.clone(),
    ))?;
    let set_cookie = session_cookie(&issued.cookie);
    for flag in ["; Path=/", "; Secure", "; HttpOnly", "; SameSite=Strict"] {
        require(
            set_cookie.as_str().contains(flag),
            "session cookie flag missing",
        )?;
    }
    let cookie = set_cookie
        .as_str()
        .split(';')
        .next()
        .ok_or("missing generated cookie")?;
    let anonymous =
        application.dispatch(&mut backend, &head("GET", "session", &[], None, None)?, &[]);
    require(anonymous.head.status == 401, "private session was public")?;
    backend.session_control().lock_local();
    let session_head = head("GET", "session", &[], Some(cookie), None)?;
    let session = application.dispatch(&mut backend, &session_head, &[]);
    require(
        session.head.status == 200,
        "local lock revoked independent Web channel",
    )?;
    require(
        session
            .head
            .headers
            .iter()
            .any(|h| h.name == "Cache-Control" && h.value == b"no-store"),
        "session response could be cached",
    )?;
    let mut cross_origin = head("GET", "session", &[], Some(cookie), None)?;
    cross_origin
        .headers
        .iter_mut()
        .find(|h| h.name == "Origin")
        .ok_or("missing origin")?
        .value = b"https://other.synthetic".to_vec();
    require(
        application
            .dispatch(&mut backend, &cross_origin, &[])
            .head
            .status
            == 403,
        "cross-origin session disclosure",
    )?;
    const BODY: &[u8] = br#"{"schemaVersion":1}"#;
    let mut wrong_csrf = authentication.csrf.as_str().as_bytes().to_vec();
    wrong_csrf[0] = if wrong_csrf[0] == b'a' { b'b' } else { b'a' };
    let wrong_csrf = check(std::str::from_utf8(&wrong_csrf))?;
    let bad_logout = head(
        "POST",
        "session/logout",
        BODY,
        Some(cookie),
        Some(wrong_csrf),
    )?;
    require(
        application
            .dispatch(&mut backend, &bad_logout, BODY)
            .head
            .status
            == 403,
        "wrong CSRF logged out account",
    )?;
    check(backend.check_session(lease, SessionPurpose::Manage))?;
    materials_probe::run(
        &mut backend,
        &mut application,
        &root.0,
        cookie,
        authentication.csrf.as_str(),
        lease,
    )?;
    content_probe::run(
        &mut backend,
        &application,
        cookie,
        authentication.csrf.as_str(),
        lease,
    )?;
    let before = check(backend.state())?
        .account(lease.principal_id())
        .map_err(|_| "missing account")?
        .principal
        .auth_epoch;
    let logout = head(
        "POST",
        "session/logout",
        BODY,
        Some(cookie),
        Some(authentication.csrf.as_str()),
    )?;
    let result = application.dispatch(&mut backend, &logout, BODY);
    require(result.head.status == 200, "durable logout failed")?;
    require(
        result
            .head
            .headers
            .iter()
            .any(|h| h.name == "Set-Cookie" && h.value.ends_with(b"Max-Age=0")),
        "logout did not clear cookie",
    )?;
    require(
        backend
            .check_session(lease, SessionPurpose::Manage)
            .is_err()
            && service.authenticate(&issued.cookie).is_err(),
        "logout left original channel authorized",
    )?;
    let epoch = check(backend.state())?
        .account(lease.principal_id())
        .map_err(|_| "missing account")?
        .principal
        .auth_epoch;
    require(
        epoch.get() == before.get().checked_add(1).ok_or("epoch exhausted")?,
        "logout did not advance epoch",
    )?;
    check(backend.close())?;
    drop(backend);
    let key = check(dpapi.unprotect(&encrypted))?;
    let key_bytes: [u8; 32] = check(key.expose().try_into())?;
    let mut reopened = check(Backend::open(
        &path,
        check(CredentialRecordCodec::from_key(key_bytes))?,
        clock.clone(),
        clock,
    ))?;
    require(
        check(reopened.state())?
            .account(lease.principal_id())
            .map_err(|_| "missing persisted account")?
            .principal
            .auth_epoch
            == epoch,
        "logout epoch did not survive reopen",
    )?;
    require(
        matches!(
            reopened.web_service().authenticate(&issued.cookie),
            Err(WebServiceError::Disabled)
        ),
        "old browser authorization survived restart",
    )?;
    check(reopened.close())?;
    drop(reopened);
    drop(deployment);
    check(fs::remove_dir_all(&root.0))?;
    println!("WEB_PAIRING_PASS CNG_DPAPI_SQLite_exact_local_approval_original_poll_secret_Secure_HttpOnly_CSRF_local_lock_independent_durable_logout_restart_cleanup=PASS wire_TLS_browser_listener=NOT_RUN");
    Ok(())
}
