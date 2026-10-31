//! Process-local teacher pairing and browser bearer/session ownership. All
//! critical sections contain bounded metadata only, never SQL/KDF/native work.
//! A short code creates a request; only exact local administrator approval can
//! issue a Web lease, and only the original polling secret can collect it.
use super::*;
use fc_runtime::{
    authority::AuthoritySnapshot,
    health::{GateError, Health},
};
use fc_security::web::{PairingCode, SecretText, SecretVerifier, WebSecret};

pub const PAIR_WINDOW_MILLIS: u64 = 120_000;
pub const MAX_PAIR_REQUESTS: usize = 8;
pub const MAX_WEB_DEVICES: usize = 64;
pub const WEB_SESSION_MILLIS: u64 = 28_800_000;
const PAIR_ATTEMPTS: u32 = 10;
const COOKIE_NAME: &str = "__Host-FutureClassroom";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebServiceError {
    Disabled,
    Denied,
    Busy,
    RateLimited,
    Capacity,
    Invalid,
    Unavailable,
    Overflow,
}
type WebResult<T> = std::result::Result<T, WebServiceError>;

pub struct PairingWindow {
    pub id: u64,
    pub code: PairingCode,
    pub expires_in_millis: u64,
}
pub struct PairingReceipt {
    pub request_id: u64,
    pub poll_secret: WebSecret,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingReview {
    pub request_id: u64,
    pub identity_revision: Revision,
    pub principal_id: PrincipalId,
    pub login_name: String,
    pub display_name: String,
    pub device_name: String,
    pub role: Role,
    pub assignments: Vec<TeachingAssignment>,
    pub current_class: Option<ClassId>,
    pub approved: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebDevice {
    pub device_id: u64,
    pub principal_id: PrincipalId,
    pub device_name: String,
}
/// Non-secret pairing-window metadata. The short code is disclosed only by
/// begin_pairing, never by management reads or Debug output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowStatus {
    pub id: u64,
    pub remaining_millis: u64,
    pub code_available: bool,
}
/// One coherent, non-secret view for the exact local global administrator.
#[derive(Clone, Debug)]
pub struct ManagementSnapshot {
    pub enabled: bool,
    pub window: Option<WindowStatus>,
    pub pending: Vec<PairingReview>,
    pub devices: Vec<WebDevice>,
}
pub struct IssuedWebSession {
    pub cookie: WebSecret,
    pub csrf: SecretText,
    pub device: WebDevice,
}
pub enum PairingPoll {
    Pending,
    Granted(IssuedWebSession),
}
pub struct AuthenticatedWeb {
    lease: SessionLease,
    pub csrf: SecretText,
    pub account: UserAccount,
    pub assignments: Vec<TeachingAssignment>,
    pub current_class: Option<ClassId>,
}
impl AuthenticatedWeb {
    pub fn lease(&self) -> SessionLease {
        self.lease
    }
}
struct Window {
    id: u64,
    approver: SessionLease,
    local_generation: u64,
    deadline: u64,
    code: Option<PairingCode>,
}
#[derive(Clone, PartialEq, Eq)]
struct Target {
    account: UserAccount,
    authority: AuthorityContext,
    current_class: Option<ClassId>,
}
struct Request {
    id: u64,
    window: u64,
    target: PrincipalId,
    device_name: String,
    verifier: SecretVerifier,
    next_poll: u64,
    approved: Option<Target>,
}
struct Device {
    cookie: SecretVerifier,
    csrf: WebSecret,
    lease: SessionLease,
    name: String,
}
#[derive(Default)]
struct State {
    enabled: bool,
    sequence: u64,
    last_tick: Option<u64>,
    attempt_start: u64,
    attempts: u32,
    window: Option<Window>,
    requests: Vec<Request>,
    devices: Vec<Device>,
}
impl State {
    fn next(&mut self) -> WebResult<u64> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(WebServiceError::Overflow)?;
        Ok(self.sequence)
    }
    fn clear_pairing(&mut self) {
        self.window = None;
        self.requests.clear();
    }
    fn tick(&mut self, tick: u64) -> WebResult<()> {
        if self.last_tick.is_some_and(|last| tick < last) {
            self.enabled = false;
            self.clear_pairing();
            self.devices.clear();
            return Err(WebServiceError::Denied);
        }
        self.last_tick = Some(tick);
        if self.window.as_ref().is_some_and(|w| tick >= w.deadline) {
            self.clear_pairing();
        }
        Ok(())
    }
}
struct Inner {
    state: Mutex<State>,
    ledger: Arc<Mutex<Ledger>>,
    control: SessionControl,
    authority: fc_runtime::authority::AuthorityReader<IdentityState>,
    clock: Arc<dyn Clock + Send + Sync>,
    monotonic: Arc<dyn MonotonicClock + Send + Sync>,
}
#[derive(Clone)]
pub struct WebService {
    inner: Arc<Inner>,
}
impl WebService {
    /// Session lease numbers are local to one backend, never installation IDs.
    pub(crate) fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub(super) fn new(
        ledger: Arc<Mutex<Ledger>>,
        control: SessionControl,
        authority: fc_runtime::authority::AuthorityReader<IdentityState>,
        clock: Arc<dyn Clock + Send + Sync>,
        monotonic: Arc<dyn MonotonicClock + Send + Sync>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State::default()),
                ledger,
                control,
                authority,
                clock,
                monotonic,
            }),
        }
    }
    fn snapshot(&self) -> WebResult<Arc<AuthoritySnapshot<IdentityState>>> {
        let deadline = Instant::now() + Duration::from_millis(100);
        loop {
            if self.inner.control.0.closed.load(Ordering::Acquire)
                || self.inner.control.is_recovering()
            {
                return Err(WebServiceError::Disabled);
            }
            match self.inner.authority.snapshot() {
                Ok(snapshot) => {
                    let status = self.inner.control.health_gate().status();
                    if status.health == Health::Healthy
                        && status.revision == snapshot.revision
                        && status.auth_epoch == snapshot.auth_epoch
                    {
                        return Ok(snapshot);
                    }
                    if !matches!(status.health, Health::Healthy | Health::AuthorityUpdating) {
                        return Err(WebServiceError::Disabled);
                    }
                }
                Err(GateError::Updating) => {}
                Err(_) => return Err(WebServiceError::Disabled),
            }
            if Instant::now() >= deadline {
                return Err(WebServiceError::Busy);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn current(&self, snapshot: &AuthoritySnapshot<IdentityState>) -> WebResult<()> {
        let status = self.inner.control.health_gate().status();
        if self.inner.control.0.closed.load(Ordering::Acquire) || self.inner.control.is_recovering()
        {
            return Err(WebServiceError::Disabled);
        }
        if status.health == Health::Healthy
            && status.revision == snapshot.revision
            && status.auth_epoch == snapshot.auth_epoch
        {
            Ok(())
        } else {
            Err(WebServiceError::Busy)
        }
    }
    fn admin(
        &self,
        ledger: &mut Ledger,
        state: &IdentityState,
        lease: SessionLease,
    ) -> WebResult<u64> {
        let generation = self.inner.control.generation();
        self.inner
            .control
            .check(lease, generation)
            .map_err(|_| WebServiceError::Denied)?;
        let now = self.inner.clock.now();
        ledger.synchronize(
            &self.inner.control,
            state,
            now,
            self.inner.monotonic.monotonic_millis(),
        );
        let actor = ledger
            .sessions
            .actor(
                state,
                lease,
                now,
                self.inner.monotonic.monotonic_millis(),
                SessionPurpose::Manage,
            )
            .map_err(|_| WebServiceError::Denied)?;
        state
            .authority(actor.principal_id)
            .map_err(|_| WebServiceError::Denied)?
            .authorize_global_local(&actor, now)
            .map_err(|_| WebServiceError::Denied)?;
        self.inner
            .control
            .check(lease, generation)
            .map_err(|_| WebServiceError::Denied)?;
        Ok(generation)
    }
    fn admin_current(
        &self,
        snapshot: &AuthoritySnapshot<IdentityState>,
        ledger: &mut Ledger,
        lease: SessionLease,
        generation: u64,
    ) -> WebResult<()> {
        if self.admin(ledger, &snapshot.data, lease)? != generation {
            return Err(WebServiceError::Denied);
        }
        self.current(snapshot)?;
        self.inner
            .control
            .check(lease, generation)
            .map_err(|_| WebServiceError::Denied)
    }
    fn sweep(
        &self,
        state: &mut State,
        ledger: &mut Ledger,
        identity: &IdentityState,
        tick: u64,
    ) -> WebResult<()> {
        if let Err(error) = state.tick(tick) {
            ledger.sessions.revoke_web();
            return Err(error);
        }
        let now = self.inner.clock.now();
        ledger.synchronize(&self.inner.control, identity, now, tick);
        ledger.sessions.prune(identity, now, tick);
        state.devices.retain(|d| {
            ledger
                .sessions
                .actor(identity, d.lease, now, tick, SessionPurpose::Manage)
                .is_ok()
        });
        state
            .requests
            .retain(|r| Self::target(identity, r.target).is_ok());
        if state.window.as_ref().is_some_and(|w| {
            self.inner.control.generation() != w.local_generation
                || self.admin(ledger, identity, w.approver).is_err()
        }) {
            state.clear_pairing();
        }
        Ok(())
    }
    fn target(identity: &IdentityState, id: PrincipalId) -> WebResult<Target> {
        let account = identity.account(id).map_err(|_| WebServiceError::Denied)?;
        if !account.principal.enabled
            || account.must_change_password
            || Some(account.principal.school_id)
                != identity.installation.as_ref().map(|i| i.school_id)
        {
            return Err(WebServiceError::Denied);
        }
        Ok(Target {
            account: account.clone(),
            authority: identity
                .authority(id)
                .map_err(|_| WebServiceError::Denied)?,
            current_class: identity.installation.as_ref().and_then(|i| i.current_class),
        })
    }
    /// Logical Web admission only. HTTP/TLS deployment separately owns actual
    /// listening; this service never creates a URL reservation or opens a port.
    pub fn set_enabled(&self, admin: SessionLease, enabled: bool) -> WebResult<()> {
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        self.sweep(
            &mut state,
            &mut ledger,
            &snapshot.data,
            self.inner.monotonic.monotonic_millis(),
        )?;
        let generation = self.admin(&mut ledger, &snapshot.data, admin)?;
        self.admin_current(&snapshot, &mut ledger, admin, generation)?;
        state.enabled = enabled;
        if !enabled {
            state.clear_pairing();
            state.devices.clear();
            ledger.sessions.revoke_web();
        }
        Ok(())
    }
    pub(super) fn close(&self) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.enabled = false;
            state.clear_pairing();
            state.devices.clear();
            if let Ok(mut ledger) = self.inner.ledger.lock() {
                ledger.sessions.revoke_web();
            }
        } else {
            self.inner.control.fault();
        }
    }
    pub fn begin_pairing(&self, admin: SessionLease) -> WebResult<PairingWindow> {
        let code =
            PairingCode::generate(&mut PlatformRandom).map_err(|_| WebServiceError::Unavailable)?;
        let shown = PairingCode::parse(code.expose_text().as_str().to_owned())
            .map_err(|_| WebServiceError::Unavailable)?;
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        let tick = self.inner.monotonic.monotonic_millis();
        let deadline = tick
            .checked_add(PAIR_WINDOW_MILLIS)
            .ok_or(WebServiceError::Overflow)?;
        self.sweep(&mut state, &mut ledger, &snapshot.data, tick)?;
        let local_generation = self.admin(&mut ledger, &snapshot.data, admin)?;
        if !state.enabled {
            return Err(WebServiceError::Disabled);
        }
        self.admin_current(&snapshot, &mut ledger, admin, local_generation)?;
        state.clear_pairing();
        let id = state.next()?;
        state.window = Some(Window {
            id,
            approver: admin,
            local_generation,
            deadline,
            code: Some(code),
        });
        Ok(PairingWindow {
            id,
            code: shown,
            expires_in_millis: PAIR_WINDOW_MILLIS,
        })
    }
    pub fn close_pairing(&self, admin: SessionLease) -> WebResult<()> {
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        let generation = self.admin(&mut ledger, &snapshot.data, admin)?;
        self.admin_current(&snapshot, &mut ledger, admin, generation)?;
        state.clear_pairing();
        Ok(())
    }
    pub fn request_pairing(
        &self,
        code: PairingCode,
        login: &str,
        device_name: &str,
    ) -> WebResult<PairingReceipt> {
        normalized_name(login, 64).map_err(|_| WebServiceError::Invalid)?;
        if device_name.is_empty()
            || device_name.len() > 256
            || device_name.chars().count() > 64
            || device_name.chars().any(char::is_control)
            || device_name.trim().is_empty()
        {
            return Err(WebServiceError::Invalid);
        }
        let secret =
            WebSecret::generate(&mut PlatformRandom).map_err(|_| WebServiceError::Unavailable)?;
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        let tick = self.inner.monotonic.monotonic_millis();
        self.sweep(&mut state, &mut ledger, &snapshot.data, tick)?;
        if !state.enabled {
            return Err(WebServiceError::Disabled);
        }
        if tick.saturating_sub(state.attempt_start) >= PAIR_WINDOW_MILLIS {
            state.attempt_start = tick;
            state.attempts = 0;
        }
        if state.attempts >= PAIR_ATTEMPTS {
            state.clear_pairing();
            return Err(WebServiceError::RateLimited);
        }
        state.attempts += 1;
        let window = state.window.as_mut().ok_or(WebServiceError::Denied)?;
        if !window
            .code
            .as_ref()
            .is_some_and(|expected| expected.matches(&code))
        {
            return Err(WebServiceError::Denied);
        }
        // Consume even when a supplied account is unavailable: the code cannot
        // be reused as an account-enumeration oracle or to create many requests.
        window.code = None;
        let (window_id, approver, generation, window_deadline) = (
            window.id,
            window.approver,
            window.local_generation,
            window.deadline,
        );
        let account = snapshot
            .data
            .account_by_login(login)
            .map_err(|_| WebServiceError::Denied)?;
        Self::target(&snapshot.data, account.principal.id)?;
        if state.requests.len() >= MAX_PAIR_REQUESTS {
            return Err(WebServiceError::Capacity);
        }
        self.admin_current(&snapshot, &mut ledger, approver, generation)?;
        if self.inner.monotonic.monotonic_millis() >= window_deadline {
            return Err(WebServiceError::Denied);
        }
        let id = state.next()?;
        state.requests.push(Request {
            id,
            window: window_id,
            target: account.principal.id,
            device_name: device_name.into(),
            verifier: secret.verifier(),
            next_poll: tick,
            approved: None,
        });
        Ok(PairingReceipt {
            request_id: id,
            poll_secret: secret,
        })
    }
    pub fn pending(&self, admin: SessionLease) -> WebResult<Vec<PairingReview>> {
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        self.sweep(
            &mut state,
            &mut ledger,
            &snapshot.data,
            self.inner.monotonic.monotonic_millis(),
        )?;
        let generation = self.admin(&mut ledger, &snapshot.data, admin)?;
        self.admin_current(&snapshot, &mut ledger, admin, generation)?;
        state
            .requests
            .iter()
            .map(|r| {
                let target = Self::target(&snapshot.data, r.target)?;
                Ok(PairingReview {
                    request_id: r.id,
                    identity_revision: snapshot.data.revision,
                    principal_id: r.target,
                    login_name: target.account.login_name,
                    display_name: target.account.display_name,
                    device_name: r.device_name.clone(),
                    role: target.account.principal.role,
                    assignments: target.authority.assignments,
                    current_class: target.current_class,
                    approved: r.approved.is_some(),
                })
            })
            .collect()
    }
    /// Reads one authority revision and one state/ledger critical section.
    /// This neither reveals credentials nor renews any local or Web lease.
    pub fn management_snapshot(&self, admin: SessionLease) -> WebResult<ManagementSnapshot> {
        // Authority publication may wait briefly; keep that wait outside both
        // business locks and reject a superseded snapshot after acquiring them.
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        let tick = self.inner.monotonic.monotonic_millis();
        self.sweep(&mut state, &mut ledger, &snapshot.data, tick)?;
        let generation = self.admin(&mut ledger, &snapshot.data, admin)?;
        let view = if state.enabled {
            let pending = state
                .requests
                .iter()
                .map(|r| {
                    let target = Self::target(&snapshot.data, r.target)?;
                    Ok(PairingReview {
                        request_id: r.id,
                        identity_revision: snapshot.data.revision,
                        principal_id: r.target,
                        login_name: target.account.login_name,
                        display_name: target.account.display_name,
                        device_name: r.device_name.clone(),
                        role: target.account.principal.role,
                        assignments: target.authority.assignments,
                        current_class: target.current_class,
                        approved: r.approved.is_some(),
                    })
                })
                .collect::<WebResult<Vec<_>>>()?;
            ManagementSnapshot {
                enabled: true,
                window: state.window.as_ref().map(|w| WindowStatus {
                    id: w.id,
                    remaining_millis: w.deadline.saturating_sub(tick),
                    code_available: w.code.is_some(),
                }),
                pending,
                devices: state
                    .devices
                    .iter()
                    .map(|d| WebDevice {
                        device_id: d.lease.id(),
                        principal_id: d.lease.principal_id(),
                        device_name: d.name.clone(),
                    })
                    .collect(),
            }
        } else {
            ManagementSnapshot {
                enabled: false,
                window: None,
                pending: Vec::new(),
                devices: Vec::new(),
            }
        };
        // Building a view grants no permission: the original exact local lease,
        // generation and authority snapshot must still pass at publication.
        self.admin_current(&snapshot, &mut ledger, admin, generation)?;
        Ok(view)
    }
    pub fn approve(
        &self,
        admin: SessionLease,
        id: u64,
        expected_identity_revision: Revision,
    ) -> WebResult<()> {
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        self.sweep(
            &mut state,
            &mut ledger,
            &snapshot.data,
            self.inner.monotonic.monotonic_millis(),
        )?;
        let generation = self.admin(&mut ledger, &snapshot.data, admin)?;
        if snapshot.data.revision != expected_identity_revision {
            return Err(WebServiceError::Busy);
        }
        let r = state
            .requests
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or(WebServiceError::Denied)?;
        let approved = Self::target(&snapshot.data, r.target)?;
        self.admin_current(&snapshot, &mut ledger, admin, generation)?;
        r.approved = Some(approved);
        Ok(())
    }
    pub fn reject(&self, admin: SessionLease, id: u64) -> WebResult<()> {
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        let generation = self.admin(&mut ledger, &snapshot.data, admin)?;
        self.admin_current(&snapshot, &mut ledger, admin, generation)?;
        state.requests.retain(|r| r.id != id);
        Ok(())
    }
    pub fn poll(&self, id: u64, proof: &WebSecret) -> WebResult<PairingPoll> {
        // Generate outside either lock. Even a broken CSPRNG cannot make the
        // browser CSRF secret equal to its HttpOnly bearer cookie.
        let cookie =
            WebSecret::generate(&mut PlatformRandom).map_err(|_| WebServiceError::Unavailable)?;
        let csrf =
            WebSecret::generate(&mut PlatformRandom).map_err(|_| WebServiceError::Unavailable)?;
        if cookie.verifier().verify(&csrf) {
            return Err(WebServiceError::Unavailable);
        }
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        let tick = self.inner.monotonic.monotonic_millis();
        self.sweep(&mut state, &mut ledger, &snapshot.data, tick)?;
        if !state.enabled {
            return Err(WebServiceError::Disabled);
        }
        let window = state.window.as_ref().ok_or(WebServiceError::Denied)?;
        let (window_id, approver, generation, deadline) = (
            window.id,
            window.approver,
            window.local_generation,
            window.deadline,
        );
        let index = state
            .requests
            .iter()
            .position(|r| r.id == id && r.window == window_id && r.verifier.verify(proof))
            .ok_or(WebServiceError::Denied)?;
        let request = &mut state.requests[index];
        if tick < request.next_poll {
            return Err(WebServiceError::RateLimited);
        }
        request.next_poll = tick.checked_add(500).ok_or(WebServiceError::Overflow)?;
        let Some(approved) = &request.approved else {
            return Ok(PairingPoll::Pending);
        };
        if *approved != Self::target(&snapshot.data, request.target)? {
            return Err(WebServiceError::Denied);
        }
        if state.devices.len() >= MAX_WEB_DEVICES {
            return Err(WebServiceError::Capacity);
        }
        if state.devices.iter().any(|d| d.cookie.verify(&cookie)) {
            return Err(WebServiceError::Unavailable);
        }
        let request = &state.requests[index];
        // sweep's administrator check can absorb later local input. Validate
        // and prune at a fresh instant, never at the earlier polling timestamp.
        let now = self.inner.clock.now();
        let issue_tick = self.inner.monotonic.monotonic_millis();
        let lease = ledger
            .sessions
            .issue_approved_web(
                &snapshot.data,
                approver,
                request.target,
                now,
                issue_tick,
                WEB_SESSION_MILLIS,
                next_day(now).map_err(|_| WebServiceError::Unavailable)?,
            )
            .map_err(|_| WebServiceError::Denied)?;
        if let Err(error) = self.admin_current(&snapshot, &mut ledger, approver, generation) {
            ledger.sessions.revoke_exact(lease);
            return Err(error);
        }
        if self.inner.monotonic.monotonic_millis() >= deadline {
            ledger.sessions.revoke_exact(lease);
            return Err(WebServiceError::Denied);
        }
        let request = state.requests.remove(index);
        let device = WebDevice {
            device_id: lease.id(),
            principal_id: lease.principal_id(),
            device_name: request.device_name.clone(),
        };
        let csrf_text = csrf.expose_hex();
        state.devices.push(Device {
            cookie: cookie.verifier(),
            csrf,
            lease,
            name: request.device_name,
        });
        Ok(PairingPoll::Granted(IssuedWebSession {
            cookie,
            csrf: csrf_text,
            device,
        }))
    }
    /// File-worker checkpoint using the latest published identity and exact
    /// original device lease. No Backend mutex, SQL or activity renewal.
    pub fn check_material_lease(
        &self,
        lease: SessionLease,
        scope: MaterialScope,
        classes: &[ClassId],
    ) -> WebResult<()> {
        let deadline = Instant::now() + Duration::from_millis(100);
        loop {
            match self.check_material_lease_once(lease, scope, classes) {
                Err(WebServiceError::Busy) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                result => return result,
            }
        }
    }
    fn check_material_lease_once(
        &self,
        lease: SessionLease,
        scope: MaterialScope,
        classes: &[ClassId],
    ) -> WebResult<()> {
        if lease.origin() != TrustedOrigin::Web {
            return Err(WebServiceError::Denied);
        }
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        let tick = self.inner.monotonic.monotonic_millis();
        self.sweep(&mut state, &mut ledger, &snapshot.data, tick)?;
        if !state.enabled || !state.devices.iter().any(|d| d.lease == lease) {
            return Err(WebServiceError::Denied);
        }
        let actor = ledger
            .sessions
            .actor(
                &snapshot.data,
                lease,
                self.inner.clock.now(),
                tick,
                SessionPurpose::Manage,
            )
            .map_err(|_| WebServiceError::Denied)?;
        // Authority publication already validated this immutable identity.
        // Repeating full IdentityState::validate for each 32KiB hash block is
        // quadratic in a school's roster; check the same authorization rules
        // against its current trusted fields without revalidating the catalog.
        let identity = &snapshot.data;
        let authority = identity
            .authority(actor.principal_id)
            .map_err(|_| WebServiceError::Denied)?;
        authority
            .validate_actor(&actor, self.inner.clock.now())
            .map_err(|_| WebServiceError::Denied)?;
        let account = identity
            .account(actor.principal_id)
            .map_err(|_| WebServiceError::Denied)?;
        if account.must_change_password
            || identity.account(scope.teacher_id).is_err()
            || !identity.subjects.iter().any(|s| s.id == scope.subject_id)
        {
            return Err(WebServiceError::Denied);
        }
        match account.principal.role {
            Role::GlobalAdmin => {}
            Role::User => return Err(WebServiceError::Denied),
            Role::TeacherAdmin => {
                if actor.principal_id != scope.teacher_id
                    || !identity.assignments.iter().any(|a| {
                        a.teacher_id == scope.teacher_id
                            && a.subject_id == scope.subject_id
                            && identity.is_teacher(scope.teacher_id, a.class_id, scope.subject_id)
                    })
                {
                    return Err(WebServiceError::Denied);
                }
            }
        }
        if classes.len() > MAX_CLASSES
            || classes.iter().enumerate().any(|(i, c)| {
                classes[..i].contains(c)
                    || !snapshot
                        .data
                        .is_teacher(scope.teacher_id, *c, scope.subject_id)
            })
        {
            return Err(WebServiceError::Denied);
        }
        self.current(&snapshot)
    }

    pub fn authenticate(&self, cookie: &WebSecret) -> WebResult<AuthenticatedWeb> {
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        let tick = self.inner.monotonic.monotonic_millis();
        self.sweep(&mut state, &mut ledger, &snapshot.data, tick)?;
        if !state.enabled {
            return Err(WebServiceError::Disabled);
        }
        let device = state
            .devices
            .iter()
            .find(|d| d.cookie.verify(cookie))
            .ok_or(WebServiceError::Denied)?;
        ledger
            .sessions
            .actor(
                &snapshot.data,
                device.lease,
                self.inner.clock.now(),
                tick,
                SessionPurpose::Manage,
            )
            .map_err(|_| WebServiceError::Denied)?;
        let target = Self::target(&snapshot.data, device.lease.principal_id())?;
        self.current(&snapshot)?;
        Ok(AuthenticatedWeb {
            lease: device.lease,
            csrf: device.csrf.expose_hex(),
            account: target.account,
            assignments: target.authority.assignments,
            current_class: target.current_class,
        })
    }
    pub fn devices(&self, admin: SessionLease) -> WebResult<Vec<WebDevice>> {
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        self.sweep(
            &mut state,
            &mut ledger,
            &snapshot.data,
            self.inner.monotonic.monotonic_millis(),
        )?;
        let generation = self.admin(&mut ledger, &snapshot.data, admin)?;
        self.admin_current(&snapshot, &mut ledger, admin, generation)?;
        Ok(state
            .devices
            .iter()
            .map(|d| WebDevice {
                device_id: d.lease.id(),
                principal_id: d.lease.principal_id(),
                device_name: d.name.clone(),
            })
            .collect())
    }
    pub fn revoke_device(&self, admin: SessionLease, device_id: u64) -> WebResult<()> {
        let snapshot = self.snapshot()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        let mut ledger = self
            .inner
            .ledger
            .lock()
            .map_err(|_| WebServiceError::Unavailable)?;
        self.current(&snapshot)?;
        let generation = self.admin(&mut ledger, &snapshot.data, admin)?;
        self.admin_current(&snapshot, &mut ledger, admin, generation)?;
        if let Some(i) = state.devices.iter().position(|d| d.lease.id() == device_id) {
            let device = state.devices.remove(i);
            ledger.sessions.revoke_exact(device.lease);
        }
        Ok(())
    }
}
/// Accept bearer credentials only in this one host-only cookie, never in a
/// query, JSON actor, Authorization header, or a client-selected cookie name.
pub fn cookie_secret(header: &[u8]) -> WebResult<WebSecret> {
    if header.len() > 4096 || !header.iter().all(|b| *b >= 32 && *b < 127) {
        return Err(WebServiceError::Invalid);
    }
    let text = std::str::from_utf8(header).map_err(|_| WebServiceError::Invalid)?;
    let mut found = None;
    for (index, part) in text.split(';').enumerate() {
        if index >= 32 {
            return Err(WebServiceError::Invalid);
        }
        let (name, value) = part
            .trim()
            .split_once('=')
            .ok_or(WebServiceError::Invalid)?;
        if name == COOKIE_NAME {
            if found.is_some() {
                return Err(WebServiceError::Invalid);
            }
            found =
                Some(WebSecret::parse_hex(value.to_owned()).map_err(|_| WebServiceError::Invalid)?);
        }
    }
    found.ok_or(WebServiceError::Denied)
}
pub fn session_cookie(secret: &WebSecret) -> SecretText {
    SecretText::new(format!(
        "{COOKIE_NAME}={}; Path=/; Secure; HttpOnly; SameSite=Strict",
        secret.expose_hex().as_str()
    ))
}
pub const CLEAR_SESSION_COOKIE: &str =
    "__Host-FutureClassroom=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0";

#[cfg(test)]
mod tests;
