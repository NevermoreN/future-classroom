use super::*;
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const ADMIN_PASSWORD: &str = "Web-fixture-admin-password-2026";

struct TestClock {
    tick: AtomicU64,
    step: AtomicU64,
    input_hook: Mutex<Option<InputHook>>,
    input_hook_fired: AtomicBool,
}
struct InputHook {
    owner: std::thread::ThreadId,
    remaining_reads: usize,
    control: SessionControl,
    lease: Option<SessionLease>,
    tick: u64,
}
impl TestClock {
    fn new() -> Self {
        Self {
            tick: AtomicU64::new(1_000),
            step: AtomicU64::new(0),
            input_hook: Mutex::new(None),
            input_hook_fired: AtomicBool::new(false),
        }
    }
    fn advance(&self, millis: u64) {
        self.tick.fetch_add(millis, Ordering::AcqRel);
    }
    fn local_input_on_second_read(&self, control: SessionControl, lease: SessionLease) {
        let mut hook = self.input_hook.lock().unwrap();
        assert!(hook.is_none());
        self.input_hook_fired.store(false, Ordering::Release);
        *hook = Some(InputHook {
            owner: std::thread::current().id(),
            remaining_reads: 2,
            control,
            lease: Some(lease),
            tick: self.tick.load(Ordering::Acquire).checked_add(1).unwrap(),
        });
    }
    fn local_lock_on_read(&self, control: SessionControl, remaining_reads: usize) {
        let mut hook = self.input_hook.lock().unwrap();
        assert!(hook.is_none() && remaining_reads > 0);
        self.input_hook_fired.store(false, Ordering::Release);
        *hook = Some(InputHook {
            owner: std::thread::current().id(),
            remaining_reads,
            control,
            lease: None,
            tick: 0,
        });
    }
}
impl Clock for TestClock {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 36_000).unwrap()
    }
}
impl MonotonicClock for TestClock {
    fn monotonic_millis(&self) -> u64 {
        let fire = {
            let mut hook = self.input_hook.lock().unwrap();
            match hook.as_mut() {
                Some(pending) if pending.owner == std::thread::current().id() => {
                    pending.remaining_reads -= 1;
                    if pending.remaining_reads == 0 {
                        hook.take()
                    } else {
                        None
                    }
                }
                _ => None,
            }
        };
        if let Some(input) = fire {
            if let Some(lease) = input.lease {
                let before = self.tick.swap(input.tick, Ordering::AcqRel);
                assert!(input.tick > before);
                assert!(input.control.note_input(lease, input.tick));
            } else {
                input.control.lock_local();
            }
            self.input_hook_fired.store(true, Ordering::Release);
        }
        self.tick
            .fetch_add(self.step.load(Ordering::Acquire), Ordering::AcqRel)
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "FutureClassroom-Web-{}-{}",
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
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
struct Fixture {
    backend: Backend,
    directory: Directory,
    clock: Arc<TestClock>,
    admin: SessionLease,
}
impl Fixture {
    fn open(directory: &Directory, clock: Arc<TestClock>) -> Backend {
        Backend::open(
            directory.database(),
            CredentialRecordCodec::from_key([39; 32]).unwrap(),
            clock.clone(),
            clock,
        )
        .unwrap()
    }
    fn new() -> Self {
        let directory = Directory::new();
        let clock = Arc::new(TestClock::new());
        let mut backend = Self::open(&directory, clock.clone());
        drop(
            backend
                .initialize(
                    Initialize {
                        school_name: "Synthetic Web School".into(),
                        login_name: "admin".into(),
                        display_name: "Synthetic Administrator".into(),
                    },
                    password(ADMIN_PASSWORD),
                )
                .unwrap(),
        );
        let admin = backend.login("admin", password(ADMIN_PASSWORD)).unwrap();
        Self {
            backend,
            directory,
            clock,
            admin,
        }
    }
    fn service(&self) -> WebService {
        self.backend.web_service()
    }
    fn login_admin(&mut self) {
        self.admin = self
            .backend
            .login("admin", password(ADMIN_PASSWORD))
            .unwrap();
    }
    fn request(&self, login: &str, device: &str) -> PairingReceipt {
        let service = self.service();
        let window = service.begin_pairing(self.admin).unwrap();
        assert_eq!(window.expires_in_millis, PAIR_WINDOW_MILLIS);
        service.request_pairing(window.code, login, device).unwrap()
    }
    fn approve(&self, receipt: &PairingReceipt) {
        let service = self.service();
        let review = service
            .pending(self.admin)
            .unwrap()
            .into_iter()
            .find(|r| r.request_id == receipt.request_id)
            .unwrap();
        assert!(!review.approved);
        service
            .approve(self.admin, receipt.request_id, review.identity_revision)
            .unwrap();
    }
    fn grant(&self, login: &str, device: &str) -> IssuedWebSession {
        let receipt = self.request(login, device);
        self.approve(&receipt);
        granted(
            self.service()
                .poll(receipt.request_id, &receipt.poll_secret)
                .unwrap(),
        )
    }
    fn create_class(&mut self, id: u64) -> ClassId {
        let id = ClassId::new(id).unwrap();
        self.backend
            .identity(
                self.admin,
                IdentityCommand::CreateClass {
                    id,
                    name: format!("Synthetic class {}", id.get()),
                },
            )
            .unwrap();
        id
    }
    fn reopen(&mut self) {
        self.backend.close().unwrap();
        self.backend = Self::open(&self.directory, self.clock.clone());
        self.login_admin();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let result = self.backend.close();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn password(value: &str) -> Password {
    Password::new(value.to_owned()).unwrap()
}
fn granted(poll: PairingPoll) -> IssuedWebSession {
    match poll {
        PairingPoll::Granted(session) => session,
        PairingPoll::Pending => panic!("expected explicit approval grant"),
    }
}
fn code(value: &str) -> PairingCode {
    PairingCode::parse(value.to_owned()).unwrap()
}
fn wrong_code(original: &PairingCode) -> PairingCode {
    let mut text = original.expose_text().as_str().as_bytes().to_vec();
    text[0] = if text[0] == b'1' { b'2' } else { b'1' };
    code(std::str::from_utf8(&text).unwrap())
}

#[test]
fn request_cancellation_while_waiting_for_pairing_lock_preserves_original_code_and_global_service()
{
    use std::sync::mpsc;
    let fixture = Fixture::new();
    let service = fixture.service();
    service.set_enabled(fixture.admin, true).unwrap();
    let window = service.begin_pairing(fixture.admin).unwrap();
    let text = window.code.expose_text().as_str().to_owned();
    let active = Arc::new(AtomicBool::new(true));
    let check = active.clone();
    let (entered, seen) = mpsc::channel();
    let notified = AtomicBool::new(false);
    let scoped = service.with_execution_fence(Arc::new(move || {
        if !notified.swap(true, Ordering::AcqRel) {
            let _ = entered.send(());
        }
        check.load(Ordering::Acquire)
    }));
    let original_state = service.inner.state.lock().unwrap();
    let thread =
        std::thread::spawn(move || scoped.request_pairing(code(&text), "admin", "late-browser"));
    seen.recv_timeout(Duration::from_secs(3)).unwrap();
    active.store(false, Ordering::Release);
    drop(original_state);
    assert!(matches!(
        thread.join().unwrap(),
        Err(WebServiceError::Unavailable)
    ));
    assert!(service.pending(fixture.admin).unwrap().is_empty());
    assert!(fixture
        .backend
        .control
        .health_gate()
        .check_sensitive()
        .is_ok());
    // The canceled view did not consume a code, disable the base service or
    // mint a lease. The same original code is still valid for a fresh request.
    let receipt = service
        .request_pairing(window.code, "admin", "fresh-browser")
        .unwrap();
    assert!(receipt.request_id > 0);
    let inner = service
        .with_execution_fence(Arc::new(|| false))
        .with_execution_fence(Arc::new(|| true));
    assert_eq!(
        inner.execution_checkpoint(),
        Err(WebServiceError::Unavailable)
    );
    let panicking = service.with_execution_fence(Arc::new(|| panic!("diagnostic request fence")));
    assert_eq!(
        panicking.execution_checkpoint(),
        Err(WebServiceError::Unavailable)
    );
    assert!(service.execution_checkpoint().is_ok());
}

#[test]
fn disabled_default_and_only_exact_local_global_administrator_manages_pairing() {
    let mut f = Fixture::new();
    let service = f.service();
    assert!(matches!(
        service.begin_pairing(f.admin),
        Err(WebServiceError::Disabled)
    ));
    assert!(matches!(
        service.request_pairing(code("12345678"), "admin", "device"),
        Err(WebServiceError::Disabled)
    ));
    let unknown = WebSecret::parse_hex("11".repeat(32)).unwrap();
    assert!(matches!(
        service.authenticate(&unknown),
        Err(WebServiceError::Disabled)
    ));
    service.set_enabled(f.admin, true).unwrap();
    let issued = f.grant("admin", "browser");
    let web = service.authenticate(&issued.cookie).unwrap().lease();
    assert_eq!(web.origin(), TrustedOrigin::Web);
    assert!(f.backend.check_session(web, SessionPurpose::Manage).is_ok());
    assert!(f.backend.voice_service().check_web_session(web).is_ok());
    assert_eq!(
        service.set_enabled(web, false),
        Err(WebServiceError::Denied)
    );
    assert!(matches!(
        service.begin_pairing(web),
        Err(WebServiceError::Denied)
    ));
    assert!(f
        .backend
        .identity(
            web,
            IdentityCommand::CreateClass {
                id: ClassId::new(1).unwrap(),
                name: "cannot create from browser".into(),
            }
        )
        .is_err());
    let receipt = f.request("admin", "another browser");
    let revision = f.backend.state().unwrap().revision;
    assert_eq!(
        service.approve(web, receipt.request_id, revision),
        Err(WebServiceError::Denied)
    );
    assert!(matches!(
        service.poll(receipt.request_id, &receipt.poll_secret),
        Ok(PairingPoll::Pending)
    ));
}

#[test]
fn eight_digit_code_only_creates_pending_and_poll_secret_approval_are_one_use() {
    let f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let window = service.begin_pairing(f.admin).unwrap();
    let replay = code(window.code.expose_text().as_str());
    assert_eq!(window.code.expose_text().as_str().len(), 8);
    let receipt = service
        .request_pairing(window.code, "admin", "my tablet")
        .unwrap();
    assert!(matches!(
        service.request_pairing(replay, "admin", "replay"),
        Err(WebServiceError::Denied)
    ));
    assert!(service.devices(f.admin).unwrap().is_empty());
    let wrong = WebSecret::parse_hex("22".repeat(32)).unwrap();
    assert!(matches!(
        service.poll(receipt.request_id, &wrong),
        Err(WebServiceError::Denied)
    ));
    assert!(matches!(
        service.poll(receipt.request_id, &receipt.poll_secret),
        Ok(PairingPoll::Pending)
    ));
    assert!(matches!(
        service.poll(receipt.request_id, &receipt.poll_secret),
        Err(WebServiceError::RateLimited)
    ));
    f.approve(&receipt);
    f.clock.advance(500);
    let issued = granted(
        service
            .poll(receipt.request_id, &receipt.poll_secret)
            .unwrap(),
    );
    assert!(matches!(
        service.poll(receipt.request_id, &receipt.poll_secret),
        Err(WebServiceError::Denied)
    ));
    let auth = service.authenticate(&issued.cookie).unwrap();
    assert_eq!(auth.csrf.as_str(), issued.csrf.as_str());
    assert_ne!(issued.cookie.expose_hex().as_str(), issued.csrf.as_str());
    let csrf_as_cookie = WebSecret::parse_hex(issued.csrf.as_str().to_owned()).unwrap();
    assert!(matches!(
        service.authenticate(&csrf_as_cookie),
        Err(WebServiceError::Denied)
    ));
    assert_eq!(format!("{:?}", issued.csrf), "SecretText([REDACTED])");
    assert_eq!(format!("{:?}", issued.cookie), "WebSecret([REDACTED])");
}

#[test]
fn input_absorbed_during_poll_uses_fresh_issue_time_without_revoking_other_sessions() {
    let f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let existing = f.grant("admin", "already established device");
    let existing_lease = service.authenticate(&existing.cookie).unwrap().lease();
    let receipt = f.request("admin", "new device approved before input");
    f.approve(&receipt);
    // poll samples t0 for sweep; its admin validation then samples t1 and
    // synchronizes this exact local-input stream into the approver's last_input.
    // Issuing a lease at t0 would incorrectly fail with ClockRegression.
    // The hook is restricted to this thread and this exact second clock read;
    // background writer observations cannot consume it. No global UI input.
    let stale_tick = f.clock.tick.load(Ordering::Acquire);
    f.clock
        .local_input_on_second_read(f.backend.session_control(), f.admin);
    let result = service.poll(receipt.request_id, &receipt.poll_secret);
    assert!(f.clock.input_hook_fired.load(Ordering::Acquire));
    let identity = f.backend.state().unwrap();
    assert_eq!(
        service.inner.ledger.lock().unwrap().sessions.actor(
            &identity,
            f.admin,
            f.clock.now(),
            stale_tick,
            SessionPurpose::Manage,
        ),
        Err(SessionError::ClockRegression)
    );
    assert!(f
        .backend
        .check_session(f.admin, SessionPurpose::Manage)
        .is_ok());
    assert_eq!(
        service.authenticate(&existing.cookie).unwrap().lease(),
        existing_lease
    );
    let issued = granted(result.unwrap());
    let issued_lease = service.authenticate(&issued.cookie).unwrap().lease();
    assert_ne!(issued_lease, existing_lease);
    assert!(f
        .backend
        .voice_service()
        .check_web_session(existing_lease)
        .is_ok());
    assert!(f
        .backend
        .voice_service()
        .check_web_session(issued_lease)
        .is_ok());
    assert_eq!(service.devices(f.admin).unwrap().len(), 2);
}

#[test]
fn locking_and_relogging_cannot_transfer_pending_or_approved_local_authority() {
    let mut f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let pending = f.request("admin", "pending before lock");
    f.backend.session_control().lock_local();
    assert_eq!(
        service.approve(
            f.admin,
            pending.request_id,
            f.backend.state().unwrap().revision
        ),
        Err(WebServiceError::Denied)
    );
    f.login_admin();
    assert!(matches!(
        service.poll(pending.request_id, &pending.poll_secret),
        Err(WebServiceError::Denied)
    ));
    assert!(service.pending(f.admin).unwrap().is_empty());
    let approved = f.request("admin", "approved before relogin");
    f.approve(&approved);
    let old_admin = f.admin;
    f.login_admin();
    assert_ne!(old_admin, f.admin);
    assert!(matches!(
        service.poll(approved.request_id, &approved.poll_secret),
        Err(WebServiceError::Denied)
    ));
    assert!(service.devices(f.admin).unwrap().is_empty());
    f.clock.advance(fc_application::identity::LOCAL_IDLE_MILLIS);
    assert!(matches!(
        service.begin_pairing(f.admin),
        Err(WebServiceError::Denied)
    ));
}

#[test]
fn approval_captures_current_binding_and_rejects_a_different_class_before_grant() {
    let mut f = Fixture::new();
    let a = f.create_class(1);
    let b = f.create_class(2);
    f.backend
        .identity(f.admin, IdentityCommand::BindCurrentClass { id: a })
        .unwrap();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let receipt = f.request("admin", "class display");
    f.approve(&receipt);
    f.backend
        .identity(f.admin, IdentityCommand::BindCurrentClass { id: b })
        .unwrap();
    assert!(matches!(
        service.poll(receipt.request_id, &receipt.poll_secret),
        Err(WebServiceError::Denied)
    ));
    assert!(service.devices(f.admin).unwrap().is_empty());
    let review = service.pending(f.admin).unwrap().pop().unwrap();
    assert_eq!(review.current_class, Some(b));
    service
        .approve(f.admin, receipt.request_id, review.identity_revision)
        .unwrap();
    f.clock.advance(500);
    let issued = granted(
        service
            .poll(receipt.request_id, &receipt.poll_secret)
            .unwrap(),
    );
    assert_eq!(
        service.authenticate(&issued.cookie).unwrap().current_class,
        Some(b)
    );
}

#[test]
fn device_revocation_is_exact_and_local_lock_does_not_revoke_existing_web_leases() {
    let mut f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let first = f.grant("admin", "tablet one");
    let second = f.grant("admin", "tablet two");
    let first_lease = service.authenticate(&first.cookie).unwrap().lease();
    let second_lease = service.authenticate(&second.cookie).unwrap().lease();
    assert_ne!(first_lease, second_lease);
    assert_eq!(service.devices(f.admin).unwrap().len(), 2);
    // Concurrent reads use genuinely increasing samples: a tick captured before
    // contending for a service lock must not invalidate a healthy later reader.
    f.clock.step.store(1, Ordering::Release);
    std::thread::scope(|scope| {
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let mut readers = Vec::new();
        for _ in 0..8 {
            let service = service.clone();
            let barrier = barrier.clone();
            let cookie = &second.cookie;
            let voice = f.backend.voice_service();
            let admin = f.admin;
            readers.push(scope.spawn(move || {
                barrier.wait();
                for _ in 0..32 {
                    assert_eq!(service.authenticate(cookie).unwrap().lease(), second_lease);
                    assert_eq!(service.devices(admin).unwrap().len(), 2);
                    voice.check_web_session(second_lease).unwrap();
                }
            }));
        }
        for reader in readers {
            reader.join().unwrap();
        }
    });
    f.clock.step.store(0, Ordering::Release);
    f.backend.session_control().lock_local();
    assert!(service.authenticate(&first.cookie).is_ok());
    assert!(service.authenticate(&second.cookie).is_ok());
    assert!(f
        .backend
        .voice_service()
        .check_web_session(first_lease)
        .is_ok());
    f.login_admin();
    service
        .revoke_device(f.admin, first.device.device_id)
        .unwrap();
    assert!(matches!(
        service.authenticate(&first.cookie),
        Err(WebServiceError::Denied)
    ));
    assert!(f
        .backend
        .check_session(first_lease, SessionPurpose::Manage)
        .is_err());
    assert!(f
        .backend
        .voice_service()
        .check_web_session(first_lease)
        .is_err());
    assert_eq!(
        service.authenticate(&second.cookie).unwrap().lease(),
        second_lease
    );
    assert!(f
        .backend
        .voice_service()
        .check_web_session(second_lease)
        .is_ok());
    assert_eq!(service.devices(f.admin).unwrap(), vec![second.device]);
}

#[test]
fn disabling_revokes_all_captured_leases_and_restart_never_restores_web_secrets() {
    let mut f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let first = f.grant("admin", "one");
    let second = f.grant("admin", "two");
    let leases = [
        service.authenticate(&first.cookie).unwrap().lease(),
        service.authenticate(&second.cookie).unwrap().lease(),
    ];
    let pending = f.request("admin", "pending");
    service.set_enabled(f.admin, false).unwrap();
    for cookie in [&first.cookie, &second.cookie] {
        assert!(matches!(
            service.authenticate(cookie),
            Err(WebServiceError::Disabled)
        ));
    }
    for lease in leases {
        assert!(f
            .backend
            .check_session(lease, SessionPurpose::Manage)
            .is_err());
        assert!(f.backend.voice_service().check_web_session(lease).is_err());
    }
    service.set_enabled(f.admin, true).unwrap();
    assert!(matches!(
        service.authenticate(&first.cookie),
        Err(WebServiceError::Denied)
    ));
    assert!(matches!(
        service.poll(pending.request_id, &pending.poll_secret),
        Err(WebServiceError::Denied)
    ));
    let before_restart = f.grant("admin", "restart victim");
    f.reopen();
    assert!(matches!(
        service.authenticate(&before_restart.cookie),
        Err(WebServiceError::Disabled)
    ));
    let reopened = f.service();
    assert!(matches!(
        reopened.begin_pairing(f.admin),
        Err(WebServiceError::Disabled)
    ));
    reopened.set_enabled(f.admin, true).unwrap();
    assert!(matches!(
        reopened.authenticate(&before_restart.cookie),
        Err(WebServiceError::Denied)
    ));
    assert!(reopened.devices(f.admin).unwrap().is_empty());
}

#[test]
fn pairing_deadline_is_exact_and_clock_regression_disables_all_admission() {
    let f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let existing = f.grant("admin", "existing browser");
    let lease = service.authenticate(&existing.cookie).unwrap().lease();
    let receipt = f.request("admin", "expires at two minutes");
    f.approve(&receipt);
    f.clock.advance(PAIR_WINDOW_MILLIS);
    assert!(matches!(
        service.poll(receipt.request_id, &receipt.poll_secret),
        Err(WebServiceError::Denied)
    ));
    assert!(service.authenticate(&existing.cookie).is_ok());
    service.begin_pairing(f.admin).unwrap();
    f.clock.tick.fetch_sub(1, Ordering::AcqRel);
    assert!(matches!(
        service.authenticate(&existing.cookie),
        Err(WebServiceError::Denied)
    ));
    assert!(f
        .backend
        .check_session(lease, SessionPurpose::Manage)
        .is_err());
    // Returning to a nonregressing tick does not reopen admission implicitly.
    f.clock.advance(1);
    assert!(matches!(
        service.begin_pairing(f.admin),
        Err(WebServiceError::Disabled)
    ));
}

#[test]
fn ten_code_attempts_are_shared_across_windows_and_reset_only_after_the_period() {
    let f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let window = service.begin_pairing(f.admin).unwrap();
    for _ in 0..10 {
        assert!(matches!(
            service.request_pairing(wrong_code(&window.code), "admin", "guess"),
            Err(WebServiceError::Denied)
        ));
    }
    assert!(matches!(
        service.request_pairing(window.code, "admin", "eleventh attempt"),
        Err(WebServiceError::RateLimited)
    ));
    let next = service.begin_pairing(f.admin).unwrap();
    assert!(matches!(
        service.request_pairing(next.code, "admin", "new window cannot reset limit"),
        Err(WebServiceError::RateLimited)
    ));
    f.clock.advance(PAIR_WINDOW_MILLIS);
    let next = service.begin_pairing(f.admin).unwrap();
    assert!(service
        .request_pairing(next.code, "admin", "after fixed period")
        .is_ok());
}

#[test]
fn cookie_has_one_fixed_host_name_canonical_secret_and_strict_security_attributes() {
    let secret = WebSecret::parse_hex("ab".repeat(32)).unwrap();
    let header = session_cookie(&secret);
    assert_eq!(
        header.as_str(),
        format!(
            "__Host-FutureClassroom={}; Path=/; Secure; HttpOnly; SameSite=Strict",
            secret.expose_hex().as_str()
        )
    );
    assert!(!header.as_str().contains("Domain="));
    let incoming = format!(
        "unrelated=ok; __Host-FutureClassroom={}",
        secret.expose_hex().as_str()
    );
    assert!(secret
        .verifier()
        .verify(&cookie_secret(incoming.as_bytes()).unwrap()));
    for invalid in [
        format!(
            "{incoming}; __Host-FutureClassroom={}",
            secret.expose_hex().as_str()
        ),
        format!("__Host-FutureClassroom={}", "AB".repeat(32)),
        "__Host-FutureClassroom=0000".into(),
        format!(
            "__Host-FutureClassroom={}\r\nx: injected",
            secret.expose_hex().as_str()
        ),
        format!("__Host-FutureClassroom={}", "0".repeat(64)),
        "x=y;".into(),
        "x=y;".repeat(33),
        "x".repeat(4097),
    ] {
        assert!(matches!(
            cookie_secret(invalid.as_bytes()),
            Err(WebServiceError::Invalid)
        ));
    }
    assert!(matches!(
        cookie_secret(format!("attacker-name={}", secret.expose_hex().as_str()).as_bytes()),
        Err(WebServiceError::Denied)
    ));
    assert!(CLEAR_SESSION_COOKIE.contains("Max-Age=0"));
    assert!(CLEAR_SESSION_COOKIE.contains("Secure; HttpOnly; SameSite=Strict"));
}

#[test]
fn management_snapshot_tracks_consumed_code_original_revision_and_exact_devices() {
    let mut f = Fixture::new();
    let service = f.service();
    let disabled = service.management_snapshot(f.admin).unwrap();
    assert!(!disabled.enabled);
    assert!(
        disabled.window.is_none() && disabled.pending.is_empty() && disabled.devices.is_empty()
    );
    service.set_enabled(f.admin, true).unwrap();
    let window = service.begin_pairing(f.admin).unwrap();
    let shown_code = window.code.expose_text();
    let available = service.management_snapshot(f.admin).unwrap();
    assert_eq!(
        available.window,
        Some(WindowStatus {
            id: window.id,
            remaining_millis: PAIR_WINDOW_MILLIS,
            code_available: true,
        })
    );
    f.clock.advance(17);
    assert_eq!(
        service
            .management_snapshot(f.admin)
            .unwrap()
            .window
            .unwrap()
            .remaining_millis,
        PAIR_WINDOW_MILLIS - 17
    );
    let receipt = service
        .request_pairing(window.code, "admin", "first managed device")
        .unwrap();
    let captured = service.management_snapshot(f.admin).unwrap();
    assert!(!captured.window.as_ref().unwrap().code_available);
    assert_eq!(captured.pending.len(), 1);
    let original = &captured.pending[0];
    assert_eq!(original.request_id, receipt.request_id);
    assert_eq!(original.principal_id, f.admin.principal_id());
    assert_eq!(original.role, Role::GlobalAdmin);
    assert_eq!(
        original.identity_revision,
        f.backend.state().unwrap().revision
    );
    assert!(!original.approved && captured.devices.is_empty());
    let diagnostic = format!("{:?}", captured.clone());
    assert!(!diagnostic.contains(shown_code.as_str()));
    assert!(!diagnostic.contains(receipt.poll_secret.expose_hex().as_str()));

    f.create_class(1);
    assert_eq!(
        service.approve(f.admin, receipt.request_id, original.identity_revision),
        Err(WebServiceError::Busy)
    );
    let fresh = service.management_snapshot(f.admin).unwrap();
    assert_eq!(fresh.pending[0].request_id, original.request_id);
    assert_ne!(
        fresh.pending[0].identity_revision,
        original.identity_revision
    );
    service
        .approve(
            f.admin,
            receipt.request_id,
            fresh.pending[0].identity_revision,
        )
        .unwrap();
    assert!(service.management_snapshot(f.admin).unwrap().pending[0].approved);
    let first = granted(
        service
            .poll(receipt.request_id, &receipt.poll_secret)
            .unwrap(),
    );
    let first_view = service.management_snapshot(f.admin).unwrap();
    assert!(first_view.pending.is_empty());
    assert_eq!(first_view.devices, vec![first.device.clone()]);
    assert!(!first_view.window.unwrap().code_available);

    let second = f.grant("admin", "second managed device");
    let both = service.management_snapshot(f.admin).unwrap();
    assert_eq!(both.devices.len(), 2);
    service
        .revoke_device(f.admin, first.device.device_id)
        .unwrap();
    assert_eq!(
        service.management_snapshot(f.admin).unwrap().devices,
        vec![second.device]
    );
    assert_eq!(both.devices.len(), 2);
    assert!(matches!(
        service.authenticate(&first.cookie),
        Err(WebServiceError::Denied)
    ));
    assert!(service.authenticate(&second.cookie).is_ok());
}

#[test]
fn management_snapshot_expires_at_deadline_and_never_renews_the_admin_or_web_admission() {
    let f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let existing = f.grant("admin", "independent existing device");
    let lease = service.authenticate(&existing.cookie).unwrap().lease();
    let receipt = f.request("admin", "request at expiry");
    f.clock.advance(PAIR_WINDOW_MILLIS - 1);
    for _ in 0..3 {
        let view = service.management_snapshot(f.admin).unwrap();
        assert_eq!(view.window.unwrap().remaining_millis, 1);
        assert_eq!(view.pending[0].request_id, receipt.request_id);
        assert_eq!(view.devices, vec![existing.device.clone()]);
    }
    f.clock.advance(1);
    let expired = service.management_snapshot(f.admin).unwrap();
    assert!(expired.window.is_none() && expired.pending.is_empty());
    assert_eq!(expired.devices, vec![existing.device]);
    assert!(service.authenticate(&existing.cookie).is_ok());
    service.set_enabled(f.admin, false).unwrap();
    let disabled = service.management_snapshot(f.admin).unwrap();
    assert!(!disabled.enabled);
    assert!(
        disabled.window.is_none() && disabled.pending.is_empty() && disabled.devices.is_empty()
    );
    assert!(f
        .backend
        .check_session(lease, SessionPurpose::Manage)
        .is_err());
    service.set_enabled(f.admin, true).unwrap();
    assert!(service
        .management_snapshot(f.admin)
        .unwrap()
        .devices
        .is_empty());
    f.clock
        .advance(fc_application::identity::LOCAL_IDLE_MILLIS - PAIR_WINDOW_MILLIS);
    assert!(matches!(
        service.management_snapshot(f.admin),
        Err(WebServiceError::Denied)
    ));
}

#[test]
fn management_snapshot_denies_web_stale_locked_and_logged_out_local_leases() {
    let mut f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let existing = f.grant("admin", "device survives local lock");
    let web = service.authenticate(&existing.cookie).unwrap().lease();
    assert!(matches!(
        service.management_snapshot(web),
        Err(WebServiceError::Denied)
    ));
    let old_admin = f.admin;
    f.login_admin();
    assert!(matches!(
        service.management_snapshot(old_admin),
        Err(WebServiceError::Denied)
    ));
    let valid = service.management_snapshot(f.admin).unwrap();
    assert_eq!(valid.devices, vec![existing.device.clone()]);
    assert!(valid.window.is_none());
    // With no pairing window, samples 1..3 cover sweep and the initial admin
    // validation. Sample 4 occurs only after the view has been built, during
    // its final admin validation. Revoke there to prove no stale view escapes.
    f.clock.local_lock_on_read(f.backend.session_control(), 4);
    assert!(matches!(
        service.management_snapshot(f.admin),
        Err(WebServiceError::Denied)
    ));
    assert!(f.clock.input_hook_fired.load(Ordering::Acquire));
    assert_eq!(service.authenticate(&existing.cookie).unwrap().lease(), web);
    assert!(f.backend.voice_service().check_web_session(web).is_ok());
    f.login_admin();
    assert_eq!(
        service.management_snapshot(f.admin).unwrap().devices,
        vec![existing.device]
    );
    f.backend.logout_account(f.admin).unwrap();
    assert!(matches!(
        service.management_snapshot(f.admin),
        Err(WebServiceError::Denied)
    ));
    assert!(matches!(
        service.authenticate(&existing.cookie),
        Err(WebServiceError::Denied)
    ));
}

#[test]
fn teacher_must_change_initial_password_and_review_cannot_cross_account_epoch_change() {
    let mut f = Fixture::new();
    let class = f.create_class(1);
    let subject = SubjectId::new(1).unwrap();
    f.backend
        .identity(
            f.admin,
            IdentityCommand::PutSubject {
                id: subject,
                name: "Synthetic subject".into(),
                enabled: true,
                color: Rgb {
                    red: 20,
                    green: 40,
                    blue: 80,
                },
                expected_revision: None,
            },
        )
        .unwrap();
    let teacher = PrincipalId::new(2).unwrap();
    let scopes = vec![AssignmentScope {
        class_id: class,
        subject_id: subject,
    }];
    drop(
        f.backend
            .create_account(
                f.admin,
                NewAccount {
                    id: teacher,
                    login_name: "teacher".into(),
                    display_name: "Synthetic Teacher".into(),
                    role: Role::TeacherAdmin,
                    scopes: scopes.clone(),
                },
                password("Web-teacher-initial-password-2026"),
            )
            .unwrap(),
    );
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let window = service.begin_pairing(f.admin).unwrap();
    assert!(matches!(
        service.request_pairing(window.code, "teacher", "before password change"),
        Err(WebServiceError::Denied)
    ));
    let initial = f
        .backend
        .login("teacher", password("Web-teacher-initial-password-2026"))
        .unwrap();
    assert_eq!(
        f.backend.check_session(initial, SessionPurpose::Manage),
        Err(BackendError::Session(SessionError::PasswordChangeRequired))
    );
    assert!(matches!(
        service.begin_pairing(initial),
        Err(WebServiceError::Denied)
    ));
    f.backend
        .change_password(
            initial,
            password("Web-teacher-initial-password-2026"),
            password("Web-teacher-changed-password-2026"),
        )
        .unwrap();
    assert!(f
        .backend
        .check_session(initial, SessionPurpose::ChangeInitialPassword)
        .is_err());
    f.login_admin();
    let receipt = f.request("teacher", "reviewed teacher device");
    f.approve(&receipt);
    f.backend
        .identity(
            f.admin,
            IdentityCommand::UpdateAccount {
                id: teacher,
                display_name: "Teacher name changed after approval".into(),
                role: Role::TeacherAdmin,
                enabled: true,
                scopes,
            },
        )
        .unwrap();
    assert!(matches!(
        service.poll(receipt.request_id, &receipt.poll_secret),
        Err(WebServiceError::Denied)
    ));
    assert!(service.devices(f.admin).unwrap().is_empty());
    let issued = f.grant("teacher", "fresh review teacher device");
    let auth = service.authenticate(&issued.cookie).unwrap();
    assert_eq!(auth.account.principal.id, teacher);
    assert_eq!(
        auth.account.display_name,
        "Teacher name changed after approval"
    );
    assert_eq!(auth.assignments.len(), 1);
    assert!(f
        .backend
        .voice_service()
        .check_web_session(auth.lease())
        .is_ok());
    assert_eq!(
        service.set_enabled(auth.lease(), false),
        Err(WebServiceError::Denied)
    );
    let local_teacher = f
        .backend
        .login("teacher", password("Web-teacher-changed-password-2026"))
        .unwrap();
    assert!(f
        .backend
        .check_session(local_teacher, SessionPurpose::Manage)
        .is_ok());
    assert!(matches!(
        service.management_snapshot(local_teacher),
        Err(WebServiceError::Denied)
    ));
}
