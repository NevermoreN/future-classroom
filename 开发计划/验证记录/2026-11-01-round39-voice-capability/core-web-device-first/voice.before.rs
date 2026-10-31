//! Cloneable trusted voice composition. Audio never takes the Backend/KDF lock.
//! Persisted ActorContext never substitutes for the original exact Web session.
use super::*;
use fc_domain::voice::*;
use fc_ports::{audio::AudioAuthorization, voice::*};
use fc_runtime::health::{GateError, Health, HealthGate};
use std::collections::BTreeMap;
#[cfg(test)]
mod limits_tests;
mod management;
mod policy_handoff;
#[cfg(test)]
mod request_fence_tests;
pub use policy_handoff::{VoicePolicyHandoff, VoicePolicyPhase};
mod native_participation;
pub use native_participation::VoiceNativeParticipation;
#[cfg(test)]
mod upload_authorization_tests;
pub use management::{VoiceDeviceCheckResult, VoiceDeviceObservation, VoiceManagementSnapshot};

#[derive(Default)]
struct VoiceSessions {
    version: u64,
    uploads: BTreeMap<VoiceAssetId, SessionLease>,
    jobs: BTreeMap<VoiceJobId, VoiceSession>,
}
impl VoiceSessions {
    fn changed(&mut self) -> Result<()> {
        self.version = self.version.checked_add(1).ok_or(BackendError::Overflow)?;
        Ok(())
    }
}
#[derive(Clone)]
struct VoiceSession {
    lease: SessionLease,
    // The monotonic durable sequence distinguishes an expired numeric job ID
    // reused later, even after terminal audit metadata has released its asset.
    sequence: u64,
    actor: ActorContext,
    stamp: Arc<Mutex<Stamp>>,
    alive: Arc<AtomicBool>,
}
#[derive(Clone, PartialEq, Eq)]
struct Stamp(VoiceJob);
impl From<&VoiceJob> for Stamp {
    fn from(job: &VoiceJob) -> Self {
        let mut job = job.clone();
        // Repository Web responses redact authorization, while internal driver
        // results retain it. The immutable exact actor is kept separately.
        job.authorization = None;
        Self(job)
    }
}
struct Inner {
    repository: Repository,
    ledger: Arc<Mutex<Ledger>>,
    control: SessionControl,
    authority: fc_runtime::authority::AuthorityReader<IdentityState>,
    clock: Arc<dyn Clock + Send + Sync>,
    monotonic: Arc<dyn MonotonicClock + Send + Sync>,
    // These critical sections are bounded by 32 asset/4 active-job metadata.
    // No writer ticket, KDF, filesystem, native call or callback runs inside them.
    sessions: Mutex<VoiceSessions>,
    // One atomic state prevents a stale stop acknowledgement reopening a newer
    // stop generation: high bits epoch, low bit admission, terminal epoch closed.
    runtime_state: AtomicU64,
    // One bounded metadata receipt. Never held across a writer wait or waker.
    policy: Mutex<policy_handoff::PolicySlot>,
    // Domain participation only. OS-thread/TLS exit is proved by the runtime.
    native_participating: AtomicBool,
    enabled: AtomicBool,
    waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    after_commit: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
#[derive(Clone)]
pub struct VoiceService {
    inner: Arc<Inner>,
    execution: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}
#[derive(Clone)]
pub struct VoiceRuntimeHandle {
    service: VoiceService,
    generation: u64,
}
#[derive(Clone)]
pub struct VoiceGuard {
    check: Arc<dyn Fn() -> AudioAuthorization + Send + Sync>,
}
impl VoiceGuard {
    pub fn decision(&self) -> AudioAuthorization {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.check)()))
            .unwrap_or(AudioAuthorization::Revoked)
    }
    pub fn is_valid(&self) -> bool {
        self.decision() == AudioAuthorization::Authorized
    }
}
// Updating is uncertainty, never permission. A completed exchange can also
// invalidate a permit obtained moments earlier; retry against its new authority.
fn gate_decision(gate: &HealthGate, error: GateError) -> AudioAuthorization {
    match (error, gate.status().health) {
        (GateError::Updating, Health::AuthorityUpdating | Health::Healthy)
        | (GateError::StaleLease | GateError::VersionConflict, Health::Healthy)
        | (_, Health::AuthorityUpdating) => AudioAuthorization::Deferred,
        _ => AudioAuthorization::Revoked,
    }
}
pub struct VoicePlaybackAuthorization {
    pub job: VoiceJob,
    pub guard: VoiceGuard,
}
fn denied() -> BackendError {
    BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::Permission(
        PermissionError::StaleAuthorization,
    )))
}
/// Browser capability metadata, containing no device/native claim.
pub struct WebVoiceCapabilities {
    pub publication_revision: Revision,
    pub identity_revision: Revision,
    pub limits: VoiceLimits,
    pub runtime_generation: u64,
    pub now: LocalDateTime,
    pub current_class: Option<(ClassId, String)>,
    pub enabled: bool,
    pub admitting: bool,
    pub target_authorized: bool,
}
fn result_unknown() -> BackendError {
    BackendError::Voice(VoiceRepositoryError::Unavailable)
}
impl VoiceService {
    pub(super) fn new(
        repository: Repository,
        ledger: Arc<Mutex<Ledger>>,
        control: SessionControl,
        authority: fc_runtime::authority::AuthorityReader<IdentityState>,
        clock: Arc<dyn Clock + Send + Sync>,
        monotonic: Arc<dyn MonotonicClock + Send + Sync>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                repository,
                ledger,
                control,
                authority,
                clock,
                monotonic,
                sessions: Mutex::new(VoiceSessions::default()),
                runtime_state: AtomicU64::new(3),
                policy: Mutex::new(policy_handoff::PolicySlot::default()),
                native_participating: AtomicBool::new(false),
                enabled: AtomicBool::new(true),
                waker: Mutex::new(None),
                #[cfg(test)]
                after_commit: Mutex::new(None),
            }),
            execution: None,
        }
    }
    /// A request-local clone. Never installs a request lifetime in the shared
    /// audio service or changes the authority of an independent cleanup owner.
    pub fn with_execution_fence(&self, fence: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        let execution = match &self.execution {
            Some(previous) => {
                let previous = previous.clone();
                Arc::new(move || previous() && fence()) as Arc<dyn Fn() -> bool + Send + Sync>
            }
            None => fence,
        };
        Self {
            inner: self.inner.clone(),
            execution: Some(execution),
        }
    }
    fn execution_current(&self) -> bool {
        self.execution.as_ref().is_none_or(|fence| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fence())).unwrap_or(false)
        })
    }
    fn execution_checkpoint(&self) -> Result<()> {
        if self.execution_current() {
            Ok(())
        } else {
            Err(BackendError::Storage(StorageError::ExecutionCancelled))
        }
    }
    #[cfg(test)]
    pub(crate) fn after_next_commit(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        let mut slot = self.inner.after_commit.lock().unwrap();
        assert!(slot.is_none());
        *slot = Some(hook);
    }
    #[cfg(test)]
    fn after_committed(&self) {
        let hook = self.inner.after_commit.lock().unwrap().take();
        if let Some(hook) = hook {
            hook()
        }
    }
    fn check_open(&self) -> Result<()> {
        self.execution_checkpoint()?;
        if self.inner.control.0.closed.load(Ordering::Acquire) {
            return Err(BackendError::Closed);
        }
        // Updating only permits entering the same bounded writer queue. Native
        // decisions remain Deferred until the current identity is published.
        if !matches!(
            self.inner.control.health_gate().status().health,
            Health::Healthy | Health::AuthorityUpdating
        ) {
            return Err(BackendError::Closed);
        }
        Ok(())
    }
    pub fn generation(&self) -> u64 {
        self.inner.runtime_state.load(Ordering::Acquire) >> 1
    }
    pub fn is_admitting(&self) -> bool {
        self.inner.runtime_state.load(Ordering::Acquire) & 1 == 1
    }
    fn current(&self, generation: u64) -> bool {
        self.execution_current()
            && generation < (u64::MAX >> 1)
            && self.inner.runtime_state.load(Ordering::Acquire) == (generation << 1 | 1)
    }
    fn epoch(&self) -> Result<u64> {
        // Request cancellation is distinct from an independent runtime stop.
        // Do not import check_open's health rules into the original epoch gate.
        self.execution_checkpoint()?;
        let generation = self.generation();
        if self.current(generation) {
            Ok(generation)
        } else {
            // A cancellation may have raced the first check/current() read.
            self.execution_checkpoint()?;
            Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            )))
        }
    }
    /// IMM/WTS/control callback safe: atomic invalidation only. Never acquires a
    /// mutex, calls a user waker, allocates, joins, or waits on SQLite.
    pub fn invalidate_runtime(&self) -> u64 {
        let old = self
            .inner
            .runtime_state
            .try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
                Some(((word >> 1).saturating_add(1).min(u64::MAX >> 1)) << 1)
            })
            .unwrap_or(u64::MAX - 1);
        (old >> 1).saturating_add(1).min(u64::MAX >> 1)
    }
    pub fn runtime_handle(&self) -> VoiceRuntimeHandle {
        VoiceRuntimeHandle {
            service: self.clone(),
            generation: self.generation(),
        }
    }
    pub fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) -> Result<()> {
        *self.inner.waker.lock().map_err(|_| BackendError::Closed)? = Some(waker);
        Ok(())
    }
    pub fn clear_waker(&self) -> Result<()> {
        *self.inner.waker.lock().map_err(|_| BackendError::Closed)? = None;
        Ok(())
    }
    fn wake(&self) {
        let waker = self.inner.waker.lock().ok().and_then(|w| w.clone());
        if let Some(waker) = waker {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker()));
        }
    }
    fn conclude<T>(&self, result: std::result::Result<T, VoiceRepositoryError>) -> Result<T> {
        if matches!(&result, Err(VoiceRepositoryError::Faulted)) {
            self.invalidate_runtime();
        }
        result.map_err(BackendError::Voice)
    }
    fn actor(&self, lease: SessionLease, web: bool) -> Result<ActorContext> {
        self.check_open()?;
        if web && lease.origin() != TrustedOrigin::Web {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::Permission(PermissionError::WebOnly),
            )));
        }
        // A publication barrier is transient uncertainty, not device closure.
        // Retry only memory snapshots; never replay SQL or renew this exact lease.
        let deadline = Instant::now() + Duration::from_millis(100);
        let gate = self.inner.control.health_gate();
        loop {
            self.check_open()?;
            if Instant::now() >= deadline {
                return Err(BackendError::Storage(StorageError::Busy));
            }
            match self.inner.authority.snapshot() {
                Ok(snapshot) => {
                    let before = gate.status();
                    match before.health {
                        Health::Healthy
                            if snapshot.revision == before.revision
                                && snapshot.auth_epoch == before.auth_epoch =>
                        {
                            let actor = self.actor_from_state(lease, &snapshot.data);
                            let after = gate.status();
                            if self.inner.control.0.closed.load(Ordering::Acquire) {
                                return Err(BackendError::Closed);
                            }
                            if after == before {
                                if Instant::now() >= deadline {
                                    return Err(BackendError::Storage(StorageError::Busy));
                                }
                                return actor;
                            }
                            if !matches!(after.health, Health::Healthy | Health::AuthorityUpdating)
                                || after.revision < before.revision
                                || after.auth_epoch < before.auth_epoch
                            {
                                return Err(BackendError::Closed);
                            }
                        }
                        Health::AuthorityUpdating => {}
                        // A new exchange overtook this immutable snapshot. The
                        // next iteration must read its newly published identity.
                        Health::Healthy
                            if before.revision > snapshot.revision
                                && before.auth_epoch >= snapshot.auth_epoch => {}
                        _ => return Err(BackendError::Closed),
                    }
                }
                Err(GateError::Updating) => {}
                Err(_) => return Err(BackendError::Closed),
            }
            self.check_open()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(BackendError::Storage(StorageError::Busy));
            }
            // No ledger/map/gate lock survives this bounded wait. Fault/stop is
            // observed on the next iteration, at most one short sleep later.
            std::thread::sleep(remaining.min(Duration::from_millis(1)));
        }
    }
    /// Memory-only preparation checkpoint for the original Web session. A
    /// transient publication may wait briefly; it never renews the lease or lets
    /// an in-flight upload cross a stop into a replacement runtime generation.
    /// Read/cancel result freshness does not require positive runtime admission.
    pub fn check_web_actor(&self, lease: SessionLease) -> Result<()> {
        let actor = self.actor(lease, true)?;
        let snapshot = self
            .inner
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        let account = snapshot
            .data
            .account(actor.principal_id)
            .map_err(|_| denied())?;
        if !matches!(
            account.principal.role,
            Role::GlobalAdmin | Role::TeacherAdmin
        ) {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::Permission(PermissionError::RoleDenied),
            )));
        }
        if account.must_change_password {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::Permission(PermissionError::PasswordChangeRequired),
            )));
        }
        self.actor_from_state(lease, &snapshot.data)?;
        let status = self.inner.control.health_gate().status();
        if status.health != Health::Healthy
            || status.revision != snapshot.revision
            || status.auth_epoch != snapshot.auth_epoch
        {
            return Err(BackendError::Storage(StorageError::Busy));
        }
        Ok(())
    }
    pub fn check_web_session(&self, lease: SessionLease) -> Result<()> {
        let epoch = self.epoch()?;
        self.actor(lease, true)?;
        if !self.current(epoch) {
            return Err(denied());
        }
        Ok(())
    }
    fn actor_from_state(&self, lease: SessionLease, state: &IdentityState) -> Result<ActorContext> {
        let now = self.inner.clock.now();
        let tick = self.inner.monotonic.monotonic_millis();
        let mut ledger = self.inner.ledger.lock().map_err(|_| BackendError::Closed)?;
        ledger.synchronize(&self.inner.control, state, now, tick);
        Ok(ledger
            .sessions
            .actor(state, lease, now, tick, SessionPurpose::Manage)?)
    }
    fn repository(&self, epoch: u64, lease: Option<SessionLease>) -> Repository {
        self.repository_mode(epoch, lease, true)
    }
    fn repository_mode(
        &self,
        epoch: u64,
        lease: Option<SessionLease>,
        require_admission: bool,
    ) -> Repository {
        let service = self.clone();
        let mut repository = self.inner.repository.clone().with_voice_timeout(
            Duration::from_millis(100),
            Arc::new(move || {
                let word = service.inner.runtime_state.load(Ordering::Acquire);
                word >> 1 == epoch
                    && epoch < (u64::MAX >> 1)
                    && !service.inner.control.0.closed.load(Ordering::Acquire)
                    && (!require_admission || word & 1 == 1)
            }),
        );
        let driver_service = self.clone();
        repository = repository.with_voice_driver_guard(Arc::new(move || {
            driver_service.policy_drivers_allowed().unwrap_or(false)
        }));
        if let Some(lease) = lease {
            repository = repository.with_session_guard(self.exact_session_guard(
                lease,
                self.inner.control.generation(),
                None,
            ));
        }
        if let Some(fence) = &self.execution {
            repository = repository.with_execution_fence(fence.clone());
        }
        repository
    }
    fn exact_session_guard(
        &self,
        lease: SessionLease,
        local_generation: u64,
        review: Option<(Revision, u64)>,
    ) -> fc_storage::business::SessionGuard {
        let service = self.clone();
        Arc::new(move |state, expected| {
            let reviewed_current = || {
                if let Some((identity_revision, epoch)) = review {
                    if state.revision != identity_revision {
                        return Err(StorageError::Voice(VoiceError::RevisionConflict));
                    }
                    if epoch >= (u64::MAX >> 1) || service.generation() != epoch {
                        return Err(StorageError::Voice(VoiceError::StaleGeneration));
                    }
                }
                Ok(())
            };
            reviewed_current()?;
            service
                .inner
                .control
                .check(lease, local_generation)
                .map_err(|_| StorageError::AuthorityUnavailable)?;
            let now = service.inner.clock.now();
            let tick = service.inner.monotonic.monotonic_millis();
            let mut ledger = service
                .inner
                .ledger
                .lock()
                .map_err(|_| StorageError::AuthorityUnavailable)?;
            ledger.synchronize(&service.inner.control, state, now, tick);
            let actual = ledger
                .sessions
                .actor(state, lease, now, tick, SessionPurpose::Manage)
                .map_err(|_| StorageError::AuthorityUnavailable)?;
            service
                .inner
                .control
                .check(lease, local_generation)
                .map_err(|_| StorageError::AuthorityUnavailable)?;
            reviewed_current()?;
            if actual == *expected {
                Ok(())
            } else {
                Err(StorageError::AuthorityUnavailable)
            }
        })
    }
    /// Bound to the original runtime generation. This does not renew a lease,
    /// take Backend's mutex, query SQLite or permit stop/resume retargeting.
    pub fn check_web_target(
        &self,
        lease: SessionLease,
        class: ClassId,
        generation: u64,
    ) -> Result<()> {
        self.check_web_target_until(
            lease,
            class,
            generation,
            Instant::now() + Duration::from_millis(100),
            None,
        )
    }
    fn check_upload_runtime(&self, generation: u64) -> Result<()> {
        self.check_open()?;
        if !self.inner.enabled.load(Ordering::Acquire) {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::Disabled,
            )));
        }
        if !self.current(generation) {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            )));
        }
        Ok(())
    }
    fn check_web_target_until(
        &self,
        lease: SessionLease,
        class: ClassId,
        generation: u64,
        deadline: Instant,
        upload: Option<&VoiceAsset>,
    ) -> Result<()> {
        let gate = self.inner.control.health_gate();
        loop {
            // The caller's original generation and lease remain fixed even
            // when unrelated commits overtake this immutable memory snapshot.
            self.check_upload_runtime(generation)?;
            if Instant::now() >= deadline {
                return Err(BackendError::Storage(StorageError::Busy));
            }
            match self.inner.authority.snapshot() {
                Ok(snapshot) => {
                    let before = gate.status();
                    match before.health {
                        Health::Healthy
                            if before.revision == snapshot.revision
                                && before.auth_epoch == snapshot.auth_epoch =>
                        {
                            let decision =
                                self.actor_from_state(lease, &snapshot.data)
                                    .and_then(|actor| {
                                        let now = self.inner.clock.now();
                                        authorize_submit(&snapshot.data, &actor, class, now)
                                            .map_err(|error| {
                                                BackendError::Voice(VoiceRepositoryError::Domain(
                                                    error,
                                                ))
                                            })?;
                                        if let Some(asset) = upload {
                                            let now = civil_seconds(now);
                                            if now < asset.created_at || now >= asset.expires_at {
                                                return Err(BackendError::Voice(
                                                    VoiceRepositoryError::Domain(
                                                        VoiceError::Expired,
                                                    ),
                                                ));
                                            }
                                            if self
                                                .inner
                                                .sessions
                                                .lock()
                                                .map_err(|_| BackendError::Closed)?
                                                .uploads
                                                .get(&asset.id)
                                                != Some(&lease)
                                            {
                                                return Err(denied());
                                            }
                                        }
                                        if Instant::now() >= deadline {
                                            return Err(BackendError::Storage(StorageError::Busy));
                                        }
                                        // Exact session revocation does not
                                        // necessarily publish a new identity.
                                        // Recheck the same proof after target
                                        // and reservation validation as well.
                                        if self.actor_from_state(lease, &snapshot.data)? != actor {
                                            return Err(denied());
                                        }
                                        Ok(())
                                    });
                            let after = gate.status();
                            self.check_upload_runtime(generation)?;
                            if after == before {
                                // Even a fully coherent successful check must
                                // not escape its original 100 ms deadline.
                                if Instant::now() >= deadline {
                                    return Err(BackendError::Storage(StorageError::Busy));
                                }
                                return decision;
                            }
                            if !matches!(after.health, Health::Healthy | Health::AuthorityUpdating)
                                || after.revision < before.revision
                                || after.auth_epoch < before.auth_epoch
                            {
                                return Err(BackendError::Closed);
                            }
                        }
                        Health::AuthorityUpdating => {}
                        Health::Healthy
                            if before.revision > snapshot.revision
                                && before.auth_epoch >= snapshot.auth_epoch => {}
                        _ => return Err(BackendError::Closed),
                    }
                }
                Err(GateError::Updating) => {}
                Err(_) => return Err(BackendError::Closed),
            }
            self.check_upload_runtime(generation)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(BackendError::Storage(StorageError::Busy));
            }
            // No map, ledger or gate lock survives this bounded wait. This
            // retries memory authorization only, never body I/O or a SQL task.
            std::thread::sleep(remaining.min(Duration::from_millis(1)));
        }
    }
    pub fn check_web_upload(
        &self,
        lease: SessionLease,
        asset: &VoiceAsset,
        generation: u64,
    ) -> Result<()> {
        let deadline = Instant::now() + Duration::from_millis(100);
        if asset.owner != lease.principal_id()
            || self
                .inner
                .sessions
                .lock()
                .map_err(|_| BackendError::Closed)?
                .uploads
                .get(&asset.id)
                != Some(&lease)
        {
            return Err(denied());
        }
        self.check_web_target_until(lease, asset.class_id, generation, deadline, Some(asset))
    }
    pub fn web_capabilities(&self, lease: SessionLease) -> Result<WebVoiceCapabilities> {
        self.check_web_actor(lease)?;
        let snapshot = self
            .inner
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        let actor = self.actor_from_state(lease, &snapshot.data)?;
        let runtime = self.inner.runtime_state.load(Ordering::Acquire);
        let enabled = self.inner.enabled.load(Ordering::Acquire);
        let inventory = self.inventory()?;
        let now = self.inner.clock.now();
        let class = snapshot
            .data
            .installation
            .as_ref()
            .and_then(|i| i.current_class)
            .and_then(|id| {
                snapshot
                    .data
                    .classes
                    .iter()
                    .find(|c| c.id == id && c.enabled)
            })
            .map(|c| (c.id, c.name.clone()));
        let authorized = class
            .as_ref()
            .is_some_and(|(id, _)| authorize_submit(&snapshot.data, &actor, *id, now).is_ok());
        self.actor_from_state(lease, &snapshot.data)?;
        let status = self.inner.control.health_gate().status();
        if status.health != Health::Healthy
            || status.revision != snapshot.revision
            || status.auth_epoch != snapshot.auth_epoch
            || self.inner.runtime_state.load(Ordering::Acquire) != runtime
            || self.inner.enabled.load(Ordering::Acquire) != enabled
        {
            return Err(BackendError::Storage(StorageError::Busy));
        }
        Ok(WebVoiceCapabilities {
            publication_revision: snapshot.revision,
            identity_revision: snapshot.data.revision,
            limits: inventory.limits,
            runtime_generation: runtime >> 1,
            now,
            current_class: class,
            enabled: inventory.enabled,
            admitting: inventory.enabled && enabled && runtime & 1 == 1,
            target_authorized: authorized,
        })
    }
    pub fn web_jobs(&self, lease: SessionLease) -> Result<Vec<VoiceJob>> {
        self.check_web_actor(lease)?;
        let snapshot = self
            .inner
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        let actor = self.actor_from_state(lease, &snapshot.data)?;
        let inventory = self.inventory()?;
        let now = self.inner.clock.now();
        let cutoff = civil_seconds(now).max(inventory.observed_at.unwrap_or(i64::MIN));
        let mut jobs: Vec<_> = inventory
            .jobs
            .into_iter()
            .filter(|j| {
                authorize_job(&snapshot.data, &actor, j, now).is_ok()
                    && (!j.state.terminal()
                        || j.ended_at
                            .is_some_and(|ended| cutoff.saturating_sub(ended) < AUDIT_TTL_SECONDS))
            })
            .collect();
        jobs.sort_by_key(|j| std::cmp::Reverse(j.sequence));
        let mut terminal = 0;
        let mut active = 0;
        jobs.retain(|j| {
            if j.state.terminal() {
                terminal += 1;
                terminal <= 20
            } else {
                active += 1;
                active <= 4
            }
        });
        for job in &mut jobs {
            job.authorization = None;
        }
        self.actor_from_state(lease, &snapshot.data)?;
        let status = self.inner.control.health_gate().status();
        if status.health != Health::Healthy
            || status.revision != snapshot.revision
            || status.auth_epoch != snapshot.auth_epoch
        {
            return Err(BackendError::Storage(StorageError::Busy));
        }
        Ok(jobs)
    }
    pub fn reserve(&self, lease: SessionLease, class: ClassId) -> Result<VoiceAsset> {
        let epoch = self.epoch()?;
        let actor = self.actor(lease, true)?;
        if !self.current(epoch) {
            return Err(denied());
        }
        if self
            .inner
            .sessions
            .lock()
            .map_err(|_| BackendError::Closed)?
            .uploads
            .len()
            >= MAX_VOICE_ASSETS
        {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::Capacity,
            )));
        }
        let mut bytes = [0; 8];
        let id = (0..8)
            .find_map(|_| {
                PlatformRandom.fill(&mut bytes).ok()?;
                VoiceAssetId::new(u64::from_le_bytes(bytes)).ok()
            })
            .ok_or(BackendError::CryptoUnavailable)?;
        let asset = self.conclude(
            self.repository(epoch, Some(lease))
                .reserve_voice_upload(id, class, &actor),
        )?;
        #[cfg(test)]
        self.after_committed();
        if !self.current(epoch) {
            return Err(result_unknown());
        }
        {
            let mut sessions = self.inner.sessions.lock().map_err(|_| result_unknown())?;
            sessions.changed().map_err(|_| result_unknown())?;
            sessions.uploads.insert(asset.id, lease);
        }
        Ok(asset)
    }
    pub fn publish(
        &self,
        lease: SessionLease,
        prepared: PreparedVoiceUpload,
    ) -> Result<VoiceAsset> {
        let epoch = self.epoch()?;
        if self
            .inner
            .sessions
            .lock()
            .map_err(|_| BackendError::Closed)?
            .uploads
            .get(&prepared.reservation().id)
            != Some(&lease)
        {
            return Err(denied());
        }
        let actor = self.actor(lease, true)?;
        if !self.current(epoch) {
            return Err(denied());
        }
        let asset = self.conclude(
            self.repository(epoch, Some(lease))
                .publish_voice_upload(prepared, &actor),
        )?;
        #[cfg(test)]
        self.after_committed();
        self.check_web_upload(lease, &asset, epoch)
            .map_err(|_| result_unknown())?;
        Ok(asset)
    }
    pub fn submit(&self, lease: SessionLease, request: VoiceSubmitRequest) -> Result<VoiceJob> {
        let epoch = self.epoch()?;
        let actor = self.actor(lease, true)?;
        if !self.current(epoch) {
            return Err(denied());
        }
        let id = VoiceJobId::new(request.command_id.get())
            .map_err(|e| BackendError::Voice(VoiceRepositoryError::Domain(e)))?;
        let existing = self.inventory()?.jobs.into_iter().find(|j| j.id == id);
        {
            let sessions = self
                .inner
                .sessions
                .lock()
                .map_err(|_| BackendError::Closed)?;
            if let Some(original) = sessions.jobs.get(&id) {
                if !existing.as_ref().is_some_and(|j| j.state.terminal()) && original.lease != lease
                {
                    return Err(denied());
                }
            } else if existing.as_ref().is_none_or(|j| !j.state.terminal())
                && (existing.is_some() || sessions.uploads.get(&request.asset_id) != Some(&lease))
            {
                return Err(denied());
            }
        }
        if existing.as_ref().is_some_and(|j| j.state.terminal()) {
            self.status(lease, id)?;
        }
        let asset = request.asset_id;
        let job = self.conclude(
            self.repository(epoch, Some(lease))
                .submit_voice(request, &actor),
        )?;
        #[cfg(test)]
        self.after_committed();
        if !self.current(epoch) {
            return Err(result_unknown());
        }
        {
            let mut sessions = self.inner.sessions.lock().map_err(|_| result_unknown())?;
            sessions.changed().map_err(|_| result_unknown())?;
            sessions.uploads.remove(&asset);
            if existing.is_none() && !job.state.terminal() {
                sessions.jobs.entry(job.id).or_insert_with(|| VoiceSession {
                    lease,
                    sequence: job.sequence,
                    actor: actor.clone(),
                    stamp: Arc::new(Mutex::new(Stamp::from(&job))),
                    alive: Arc::new(AtomicBool::new(true)),
                });
            }
        }
        self.wake();
        Ok(job)
    }
    pub fn status(&self, lease: SessionLease, id: VoiceJobId) -> Result<VoiceJob> {
        let actor = self.actor(lease, true)?;
        let mut job = self.conclude(
            self.repository(self.generation(), Some(lease))
                .voice_job(id, &actor),
        )?;
        job.authorization = None;
        Ok(job)
    }
    pub fn cancel(&self, lease: SessionLease, request: VoiceCancelRequest) -> Result<VoiceJob> {
        let actor = self.actor(lease, true)?;
        let current = self.status(lease, request.job_id)?;
        if current.revision != request.expected_revision
            && current.cancel_command != Some(request.command_id)
        {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::RevisionConflict,
            )));
        }
        // After exact ownership/CAS admission, stop intent closes this job's old
        // native capabilities before waiting for COMMIT. A racing completion or
        // uncertain result can never revive that same capability.
        if let Some(entry) = self
            .inner
            .sessions
            .lock()
            .map_err(|_| BackendError::Closed)?
            .jobs
            .get(&request.job_id)
            .filter(|entry| entry.sequence == current.sequence)
        {
            entry.alive.store(false, Ordering::Release);
        }
        let job = self.conclude(
            self.repository(self.generation(), Some(lease))
                .cancel_voice(request, &actor),
        )?;
        #[cfg(test)]
        self.after_committed();
        self.update(&job).map_err(|_| result_unknown())?;
        self.wake();
        Ok(job)
    }
    fn update(&self, job: &VoiceJob) -> Result<()> {
        let entry = {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .map_err(|_| BackendError::Closed)?;
            if sessions
                .jobs
                .get(&job.id)
                .is_some_and(|entry| entry.sequence != job.sequence)
            {
                return Ok(());
            }
            if job.state.terminal() {
                if sessions.jobs.contains_key(&job.id) {
                    sessions.changed()?;
                }
                sessions.jobs.remove(&job.id)
            } else {
                sessions.jobs.get(&job.id).cloned()
            }
        };
        if let Some(entry) = entry {
            let mut stamp = entry.stamp.lock().map_err(|_| BackendError::Closed)?;
            if job.revision >= stamp.0.revision {
                *stamp = Stamp::from(job);
            }
            if job.state.terminal() || job.state == VoiceState::Stopping {
                entry.alive.store(false, Ordering::Release);
            }
        }
        Ok(())
    }
    pub fn inventory(&self) -> Result<VoiceSnapshot> {
        self.check_open()?;
        self.conclude(self.repository(self.generation(), None).voice_inventory())
    }
    pub fn recover(&self) -> Result<VoiceSnapshot> {
        self.check_open()?;
        let generation = self.invalidate_runtime();
        let snapshot = self.conclude(self.repository(generation, None).recover_voice())?;
        {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .map_err(|_| BackendError::Closed)?;
            for entry in sessions.jobs.values() {
                entry.alive.store(false, Ordering::Release);
            }
            sessions.changed()?;
            sessions.jobs.clear();
            sessions.uploads.clear();
        }
        self.inner
            .enabled
            .store(snapshot.enabled, Ordering::Release);
        if generation < (u64::MAX >> 1) {
            self.inner
                .runtime_state
                .compare_exchange(
                    generation << 1,
                    generation << 1 | 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .map_err(|_| denied())?;
        }
        Ok(snapshot)
    }
    pub fn drive(&self, update: VoiceDriverUpdate) -> Result<VoiceJob> {
        self.drive_epoch(update, self.generation())
    }
    fn drive_epoch(&self, update: VoiceDriverUpdate, epoch: u64) -> Result<VoiceJob> {
        self.check_open()?;
        if !self.policy_drivers_allowed()? {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            )));
        }
        let positive = matches!(
            update.action,
            VoiceDriverAction::Start
                | VoiceDriverAction::FirstWrite
                | VoiceDriverAction::NaturalComplete
        );
        if positive && !self.current(epoch) {
            return Err(denied());
        }
        let lease = if matches!(
            update.action,
            VoiceDriverAction::Start | VoiceDriverAction::FirstWrite
        ) {
            Some(
                self.inner
                    .sessions
                    .lock()
                    .map_err(|_| BackendError::Closed)?
                    .jobs
                    .get(&update.job_id)
                    .ok_or_else(denied)?
                    .lease,
            )
        } else {
            None
        };
        if let Some(lease) = lease {
            match self.inner.authority.snapshot() {
                Ok(snapshot) => {
                    self.actor_from_state(lease, &snapshot.data)?;
                }
                // A known exact lease enters the same bounded writer queue. Its
                // evaluation and precommit still check the newly current state.
                Err(GateError::Updating) => {}
                Err(_) => return Err(BackendError::Closed),
            }
        }
        let job = self.conclude(self.repository(epoch, lease).drive_voice(update))?;
        self.update(&job)?;
        Ok(job)
    }
    pub fn guard(&self, id: VoiceJobId) -> Result<VoicePlaybackAuthorization> {
        self.guard_epoch(id, self.epoch()?)
    }
    fn guard_epoch(&self, id: VoiceJobId, epoch: u64) -> Result<VoicePlaybackAuthorization> {
        if !self.current(epoch) {
            return Err(denied());
        }
        let inventory = self.inventory()?;
        self.inner
            .enabled
            .store(inventory.enabled, Ordering::Release);
        if !inventory.enabled {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::Disabled,
            )));
        }
        let job = inventory
            .jobs
            .into_iter()
            .find(|j| {
                j.id == id
                    && matches!(
                        j.state,
                        VoiceState::Queued
                            | VoiceState::Starting
                            | VoiceState::Playing
                            | VoiceState::Gap
                    )
            })
            .ok_or_else(denied)?;
        self.guard_for_epoch(&job, epoch)
    }
    fn guard_for_epoch(&self, job: &VoiceJob, epoch: u64) -> Result<VoicePlaybackAuthorization> {
        if !self.current(epoch)
            || !matches!(
                job.state,
                VoiceState::Queued | VoiceState::Starting | VoiceState::Playing | VoiceState::Gap
            )
        {
            return Err(denied());
        }
        let job = job.clone();
        let id = job.id;
        let entry = self
            .inner
            .sessions
            .lock()
            .map_err(|_| BackendError::Closed)?
            .jobs
            .get(&id)
            .cloned()
            .ok_or_else(denied)?;
        let expected = Stamp::from(&job);
        if entry.sequence != job.sequence
            || *entry.stamp.lock().map_err(|_| BackendError::Closed)? != expected
            || !entry.alive.load(Ordering::Acquire)
        {
            return Err(denied());
        }
        let actor = entry.actor.clone();
        if job.authorization.as_ref() != Some(&actor) {
            return Err(denied());
        }
        let issued_at = self.inner.clock.now();
        let latest_start = job
            .first_started_at
            .is_none()
            .then_some(job.latest_start_at);
        let class = job.class_id;
        let service = self.clone();
        let guard = VoiceGuard {
            check: Arc::new(move || {
                use AudioAuthorization::{Authorized, Deferred, Revoked};
                if !service.current(epoch)
                    || !service.inner.enabled.load(Ordering::Acquire)
                    || !entry.alive.load(Ordering::Acquire)
                    || entry.stamp.lock().map_or(true, |s| *s != expected)
                    || service.inner.control.0.closed.load(Ordering::Acquire)
                {
                    return Revoked;
                }
                let now = service.inner.clock.now();
                if now < issued_at
                    || latest_start.is_some_and(|deadline| civil_seconds(now) >= deadline)
                {
                    return Revoked;
                }
                let gate = service.inner.control.health_gate();
                // CURRENT authority only. Deferred never supplies a buffer, resets
                // the device, counts a repeat, or turns an old snapshot into proof.
                let snapshot = match service.inner.authority.snapshot() {
                    Ok(snapshot) => snapshot,
                    Err(error) => return gate_decision(&gate, error),
                };
                let status = match gate.check_sensitive() {
                    Ok(status) => status,
                    Err(error) => return gate_decision(&gate, error),
                };
                if snapshot.revision != status.revision {
                    return Deferred;
                }
                let Ok(mut ledger) = service.inner.ledger.lock() else {
                    return Revoked;
                };
                let tick = service.inner.monotonic.monotonic_millis();
                ledger.synchronize(&service.inner.control, &snapshot.data, now, tick);
                let Ok(actual) = ledger.sessions.actor(
                    &snapshot.data,
                    entry.lease,
                    now,
                    tick,
                    SessionPurpose::Manage,
                ) else {
                    return Revoked;
                };
                if actual != actor || authorize_submit(&snapshot.data, &actual, class, now).is_err()
                {
                    return Revoked;
                }
                drop(ledger);
                let effect = match gate.lease_for_authorized_scope(
                    id.get(),
                    Instant::now() + Duration::from_millis(50),
                ) {
                    Ok(effect) => effect,
                    Err(error) => return gate_decision(&gate, error),
                };
                let permit = match gate.try_start(
                    effect,
                    fc_runtime::health::EffectIntent {
                        scope: id.get(),
                        job_id: id.get(),
                    },
                ) {
                    Ok(permit) => permit,
                    Err(error) => return gate_decision(&gate, error),
                };
                if !service.current(epoch) || !entry.alive.load(Ordering::Acquire) {
                    return Revoked;
                }
                if let Err(error) = gate.check_permit(&permit) {
                    return gate_decision(&gate, error);
                }
                let after = gate.status();
                if after != status {
                    return if matches!(after.health, Health::Healthy | Health::AuthorityUpdating) {
                        Deferred
                    } else {
                        Revoked
                    };
                }
                Authorized
            }),
        };
        Ok(VoicePlaybackAuthorization { job, guard })
    }
    pub fn interrupt_all(&self) -> Result<VoiceSnapshot> {
        self.check_open()?;
        if self.is_admitting() {
            self.invalidate_runtime();
        }
        let snapshot = self.conclude(
            self.repository(self.generation(), None)
                .emergency_stop_voice(),
        )?;
        for job in &snapshot.jobs {
            self.update(job)?;
        }
        self.wake();
        Ok(snapshot)
    }
    /// Caller must already have confirmed no native ownership. The writer query
    /// is also a fence behind any old, timed-out transaction; old jobs stay terminal.
    pub fn resume_after_stop(&self, expected_generation: u64) -> Result<()> {
        let snapshot = self.inventory()?;
        if snapshot.jobs.iter().any(|j| !j.state.terminal())
            || expected_generation >= (u64::MAX >> 1)
        {
            return Err(denied());
        }
        if self
            .policy_handoff(expected_generation)?
            .is_some_and(|receipt| receipt.phase != VoicePolicyPhase::Committed || !receipt.enabled)
        {
            return Err(denied());
        }
        self.inner
            .runtime_state
            .compare_exchange(
                expected_generation << 1,
                expected_generation << 1 | 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| denied())?;
        Ok(())
    }
    pub fn set_enabled(
        &self,
        lease: SessionLease,
        enabled: bool,
        expected: Revision,
    ) -> Result<VoiceSnapshot> {
        // Preserve the bounded authority-publication wait of the original API.
        // The reviewed entry point still revalidates the exact lease itself.
        self.actor(lease, false)?;
        let state = self
            .inner
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        self.set_enabled_reviewed(
            lease,
            enabled,
            expected,
            state.data.revision,
            self.generation(),
        )
    }
    pub fn maintain(&self) -> Result<VoiceSnapshot> {
        self.check_open()?;
        let version = self
            .inner
            .sessions
            .lock()
            .map_err(|_| BackendError::Closed)?
            .version;
        let snapshot = self.conclude(self.repository(self.generation(), None).maintain_voice())?;
        {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .map_err(|_| BackendError::Closed)?;
            // A concurrently committed new upload/job may publish its lease
            // after this writer snapshot. Never prune it from an older result.
            if sessions.version == version {
                sessions.jobs.retain(|id, _| {
                    snapshot
                        .jobs
                        .iter()
                        .any(|j| j.id == *id && !j.state.terminal())
                });
                sessions.uploads.retain(|id, _| {
                    snapshot.assets.iter().any(|a| {
                        a.id == *id
                            && matches!(a.state, VoiceAssetState::Reserved | VoiceAssetState::Ready)
                    })
                });
                sessions.changed()?;
            }
        }
        for job in &snapshot.jobs {
            self.update(job)?;
        }
        Ok(snapshot)
    }
    pub fn finalize(&self, id: VoiceAssetId, expected: Revision) -> Result<()> {
        self.check_open()?;
        self.conclude(
            self.repository(self.generation(), None)
                .finalize_voice_delete(id, expected),
        )?;
        {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .map_err(|_| BackendError::Closed)?;
            if sessions.uploads.remove(&id).is_some() {
                sessions.changed()?;
            }
        }
        Ok(())
    }
}
impl VoiceRuntimeHandle {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn is_current(&self) -> bool {
        self.service.current(self.generation)
    }
    /// Availability is only a triage for constructing a fresh job capability.
    /// Authorized here never grants a native Write without its exact job guard.
    pub fn availability_decision(&self) -> AudioAuthorization {
        if !self.is_current()
            || self.service.inner.control.0.closed.load(Ordering::Acquire)
            || !self.service.inner.enabled.load(Ordering::Acquire)
        {
            return AudioAuthorization::Revoked;
        }
        match self.service.inner.control.health_gate().status().health {
            Health::AuthorityUpdating => AudioAuthorization::Deferred,
            Health::Healthy => AudioAuthorization::Authorized,
            _ => AudioAuthorization::Revoked,
        }
    }
    pub fn inventory(&self) -> Result<VoiceSnapshot> {
        self.service.inventory()
    }
    pub fn drive(&self, update: VoiceDriverUpdate) -> Result<VoiceJob> {
        self.service.drive_epoch(update, self.generation)
    }
    pub fn guard(&self, id: VoiceJobId) -> Result<VoicePlaybackAuthorization> {
        self.service.guard_epoch(id, self.generation)
    }
    /// Builds only from an exact current durable job stamp and its original
    /// session mapping. No SQL or authority read occurs until decision().
    pub fn guard_for(&self, job: &VoiceJob) -> Result<VoicePlaybackAuthorization> {
        self.service.guard_for_epoch(job, self.generation)
    }
    pub fn interrupt_all(&self) -> Result<VoiceSnapshot> {
        self.service.interrupt_all()
    }
}
impl Backend {
    pub fn voice_service(&self) -> VoiceService {
        match &self.request_execution_fence {
            Some(fence) => self.voice.with_execution_fence(fence.clone()),
            None => self.voice.clone(),
        }
    }
    pub fn voice_reserve(&mut self, l: SessionLease, c: ClassId) -> Result<VoiceAsset> {
        self.voice_service().reserve(l, c)
    }
    pub fn voice_publish(&mut self, l: SessionLease, p: PreparedVoiceUpload) -> Result<VoiceAsset> {
        self.voice_service().publish(l, p)
    }
    pub fn voice_submit(&mut self, l: SessionLease, r: VoiceSubmitRequest) -> Result<VoiceJob> {
        self.voice_service().submit(l, r)
    }
    pub fn voice_status(&self, l: SessionLease, id: VoiceJobId) -> Result<VoiceJob> {
        self.voice_service().status(l, id)
    }
    pub fn voice_cancel(&mut self, l: SessionLease, r: VoiceCancelRequest) -> Result<VoiceJob> {
        self.voice_service().cancel(l, r)
    }
    pub fn voice_inventory(&self) -> Result<VoiceSnapshot> {
        self.voice.inventory()
    }
    pub fn voice_recover(&mut self) -> Result<VoiceSnapshot> {
        self.voice.recover()
    }
    pub fn voice_drive(&mut self, u: VoiceDriverUpdate) -> Result<VoiceJob> {
        self.voice.drive(u)
    }
    pub fn voice_guard(&self, id: VoiceJobId) -> Result<VoicePlaybackAuthorization> {
        self.voice.guard(id)
    }
    pub fn voice_emergency_stop(&mut self) -> Result<VoiceSnapshot> {
        self.voice.interrupt_all()
    }
    pub fn voice_enabled(
        &mut self,
        l: SessionLease,
        e: bool,
        r: Revision,
    ) -> Result<VoiceSnapshot> {
        self.voice_service().set_enabled(l, e, r)
    }
    pub fn voice_maintain(&mut self) -> Result<VoiceSnapshot> {
        self.voice.maintain()
    }
    pub fn voice_finalize_delete(&mut self, id: VoiceAssetId, r: Revision) -> Result<()> {
        self.voice.finalize(id, r)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    // Production deadlines are wall-clock safety bounds. Avoid four synthetic
    // password KDF fixtures starving each other's real SQLite writer in tests.
    pub(super) static VOICE_FIXTURE: Mutex<()> = Mutex::new(());
    struct ClockFixture(Mutex<LocalDateTime>);
    impl Clock for ClockFixture {
        fn now(&self) -> LocalDateTime {
            *self.0.lock().unwrap()
        }
    }
    impl MonotonicClock for ClockFixture {
        fn monotonic_millis(&self) -> u64 {
            0
        }
    }
    // TEST ONLY: these host fixtures exercise the real SQLite writer protocol,
    // authorization and 100 ms control bounds, not disk durability/fsync latency.
    // NAS btrfs /tmp showed intermittent Faulted even with one test thread;
    // Linux uses a private tmpfs fixture when available. Windows still uses its
    // ordinary TEMP disk. Disk persistence/restart coverage remains in storage
    // integration tests and native Windows validation, with production unchanged.
    fn fixture_root() -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            let shared_memory = PathBuf::from("/dev/shm");
            if shared_memory.is_dir() {
                return shared_memory;
            }
        }
        std::env::temp_dir()
    }
    pub(super) struct Directory(pub(super) PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let r = std::fs::remove_dir_all(&self.0);
            if !std::thread::panicking() {
                r.unwrap();
            }
        }
    }
    struct Service(Backend);
    impl Drop for Service {
        fn drop(&mut self) {
            let r = self.0.close();
            if !std::thread::panicking() {
                r.unwrap();
            }
        }
    }
    pub(super) fn issue(backend: &Backend, origin: TrustedOrigin) -> SessionLease {
        let state = backend.state().unwrap();
        let account = state.account(PrincipalId::new(1).unwrap()).unwrap();
        let lease = backend
            .ledger
            .lock()
            .unwrap()
            .sessions
            .issue_verified(
                account,
                origin,
                0,
                SESSION_MILLIS,
                LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 3600).unwrap(),
            )
            .unwrap();
        if origin == TrustedOrigin::Local {
            backend
                .control
                .0
                .active_local
                .store(lease.id(), Ordering::Release);
            backend
                .control
                .0
                .active_principal
                .store(lease.principal_id().get(), Ordering::Release);
        }
        lease
    }
    struct MetadataPreparation;
    impl VoicePreparation for MetadataPreparation {
        type Input = ();
        fn prepare(
            &mut self,
            _: (),
            _: &VoiceAsset,
        ) -> std::result::Result<VoicePcmMetadata, VoiceRepositoryError> {
            Ok(VoicePcmMetadata {
                byte_length: 16_044,
                frame_count: 8_000,
                sha256: [1; 32],
            })
        }
    }
    fn prepared(asset: VoiceAsset) -> PreparedVoiceUpload {
        prepare_voice_upload(&mut MetadataPreparation, (), asset).unwrap()
    }
    fn submit(
        backend: &mut Backend,
        lease: SessionLease,
        class: ClassId,
        id: u64,
    ) -> VoiceSubmitRequest {
        let reservation = backend.voice_reserve(lease, class).unwrap();
        let asset = backend.voice_publish(lease, prepared(reservation)).unwrap();
        VoiceSubmitRequest {
            command_id: CommandId::new(id).unwrap(),
            asset_id: asset.id,
            expected_asset_revision: asset.revision,
            class_id: class,
            repeat_total: 3,
        }
    }
    #[test]
    fn exact_original_voice_session_guards_cancel_disable_and_terminal_retry_are_real() {
        let _fixture = VOICE_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
        let directory = Directory(fixture_root().join(format!(
                "FutureClassroom-Voice-Backend-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&directory.0).unwrap();
        let clock = Arc::new(ClockFixture(Mutex::new(
            LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 3600).unwrap(),
        )));
        let mut service = Service(
            Backend::open(
                directory.0.join("state.sqlite3"),
                CredentialRecordCodec::from_key([91; 32]).unwrap(),
                clock.clone(),
                clock.clone(),
            )
            .unwrap(),
        );
        let backend = &mut service.0;
        drop(
            backend
                .initialize(
                    Initialize {
                        school_name: "Synthetic voice".into(),
                        login_name: "admin".into(),
                        display_name: "Synthetic administrator".into(),
                    },
                    Password::new("Synthetic voice credential 2026!".into()).unwrap(),
                )
                .unwrap(),
        );
        let local = issue(backend, TrustedOrigin::Local);
        let class = ClassId::new(1).unwrap();
        backend
            .identity(
                local,
                IdentityCommand::CreateClass {
                    id: class,
                    name: "Synthetic class".into(),
                },
            )
            .unwrap();
        backend
            .identity(local, IdentityCommand::BindCurrentClass { id: class })
            .unwrap();
        let old = issue(backend, TrustedOrigin::Web);
        let request = submit(backend, old, class, 71);
        let job = backend.voice_submit(old, request.clone()).unwrap();
        assert!(job.authorization.is_none());
        let queued = backend.voice_guard(job.id).unwrap();
        assert!(queued.guard.is_valid());
        let newer = issue(backend, TrustedOrigin::Web);
        assert!(
            !queued.guard.is_valid(),
            "same principal's new session cannot revive old queued permit"
        );
        assert!(backend.voice_submit(newer, request.clone()).is_err());
        assert!(backend
            .voice_drive(VoiceDriverUpdate {
                job_id: job.id,
                expected_revision: job.revision,
                generation: job.generation,
                action: VoiceDriverAction::Start
            })
            .is_err());
        // Current authenticated owner may cancel the old job, but cannot replace
        // the original session to play it. Terminal retry returns current state.
        let canceled = backend
            .voice_cancel(
                newer,
                VoiceCancelRequest {
                    command_id: CommandId::new(72).unwrap(),
                    job_id: job.id,
                    expected_revision: job.revision,
                },
            )
            .unwrap();
        assert_eq!(canceled.state, VoiceState::Canceled);
        let replay = backend.voice_submit(newer, request).unwrap();
        assert_eq!(replay.state, VoiceState::Canceled);
        assert!(replay.authorization.is_none());
        assert!(backend.voice_guard(job.id).is_err());
        assert!(!backend
            .voice
            .inner
            .sessions
            .lock()
            .unwrap()
            .jobs
            .contains_key(&job.id));
        let request = submit(backend, newer, class, 73);
        let job = backend.voice_submit(newer, request).unwrap();
        let queued = backend.voice_guard(job.id).unwrap();
        let started = backend
            .voice_drive(VoiceDriverUpdate {
                job_id: job.id,
                expected_revision: job.revision,
                generation: 0,
                action: VoiceDriverAction::Start,
            })
            .unwrap();
        assert!(
            !queued.guard.is_valid(),
            "a COMMIT invalidates the previously captured guard"
        );
        let active = backend.voice_guard(job.id).unwrap();
        assert!(active.guard.is_valid());
        let stopped = backend
            .voice_cancel(
                newer,
                VoiceCancelRequest {
                    command_id: CommandId::new(74).unwrap(),
                    job_id: job.id,
                    expected_revision: started.revision,
                },
            )
            .unwrap();
        assert_eq!(stopped.state, VoiceState::Stopping);
        assert!(!active.guard.is_valid());
        assert!(backend.voice_guard(job.id).is_err());
        backend
            .voice_drive(VoiceDriverUpdate {
                job_id: stopped.id,
                expected_revision: stopped.revision,
                generation: started.release_generation.unwrap(),
                action: VoiceDriverAction::Released,
            })
            .unwrap();
        let request = submit(backend, newer, class, 75);
        let job = backend.voice_submit(newer, request).unwrap();
        let permit = backend.voice_guard(job.id).unwrap();
        let revision = backend.voice_inventory().unwrap().revision;
        backend.voice_enabled(local, false, revision).unwrap();
        assert!(!permit.guard.is_valid());
        assert!(backend.voice_guard(job.id).is_err());
        assert_eq!(
            backend.voice_status(newer, job.id).unwrap().state,
            VoiceState::Failed
        );
        backend.voice_maintain().unwrap();
        assert!(backend.voice.inner.sessions.lock().unwrap().jobs.is_empty());
        let revision = backend.voice_inventory().unwrap().revision;
        backend.voice_enabled(local, true, revision).unwrap();
        let request = submit(backend, newer, class, 76);
        let queued = backend.voice_submit(newer, request).unwrap();
        let starting = backend
            .voice_drive(VoiceDriverUpdate {
                job_id: queued.id,
                expected_revision: queued.revision,
                generation: 0,
                action: VoiceDriverAction::Start,
            })
            .unwrap();
        let before_deadline = backend.voice_guard(starting.id).unwrap();
        assert!(before_deadline.guard.is_valid());
        let day = clock.now().date;
        *clock.0.lock().unwrap() = LocalDateTime::new(day, 3900).unwrap();
        assert!(
            !before_deadline.guard.is_valid(),
            "source or device open crossing latest start cannot write a first buffer"
        );
    }
    pub(super) struct SharedService(pub(super) Arc<Mutex<Backend>>);
    impl Drop for SharedService {
        fn drop(&mut self) {
            let result = self.0.lock().unwrap_or_else(|p| p.into_inner()).close();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
    pub(super) fn shared_fixture() -> (Directory, SharedService, SessionLease, ClassId) {
        let directory = Directory(fixture_root().join(format!(
                "FutureClassroom-Voice-Service-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&directory.0).unwrap();
        let clock = Arc::new(ClockFixture(Mutex::new(
            LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 3600).unwrap(),
        )));
        let mut backend = Backend::open(
            directory.0.join("state.sqlite3"),
            CredentialRecordCodec::from_key([92; 32]).unwrap(),
            clock.clone(),
            clock,
        )
        .unwrap();
        drop(
            backend
                .initialize(
                    Initialize {
                        school_name: "Synthetic independent voice".into(),
                        login_name: "admin".into(),
                        display_name: "Administrator".into(),
                    },
                    Password::new("Independent voice password 2026!".into()).unwrap(),
                )
                .unwrap(),
        );
        let local = issue(&backend, TrustedOrigin::Local);
        let class = ClassId::new(1).unwrap();
        backend
            .identity(
                local,
                IdentityCommand::CreateClass {
                    id: class,
                    name: "Synthetic class".into(),
                },
            )
            .unwrap();
        backend
            .identity(local, IdentityCommand::BindCurrentClass { id: class })
            .unwrap();
        let web = issue(&backend, TrustedOrigin::Web);
        (
            directory,
            SharedService(Arc::new(Mutex::new(backend))),
            web,
            class,
        )
    }
    pub(super) fn independent_request(
        service: &VoiceService,
        lease: SessionLease,
        class: ClassId,
        id: u64,
    ) -> VoiceSubmitRequest {
        let reservation = service.reserve(lease, class).unwrap();
        let asset = service.publish(lease, prepared(reservation)).unwrap();
        VoiceSubmitRequest {
            command_id: CommandId::new(id).unwrap(),
            asset_id: asset.id,
            expected_asset_revision: asset.revision,
            class_id: class,
            repeat_total: 1,
        }
    }
    struct Release(Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    #[test]
    fn independent_voice_service_progresses_during_actual_backend_mutex_and_kdf() {
        let _fixture = VOICE_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
        let (_directory, backend, web, class) = shared_fixture();
        let service = backend.0.lock().unwrap().voice_service();
        let request = independent_request(&service, web, class, 301);
        let queued = service.submit(web, request).unwrap();
        let runtime = service.runtime_handle();
        let queued_guard = runtime.guard(queued.id).unwrap();
        assert!(queued_guard.guard.is_valid());
        // An expired task may share this numeric ID, but its distinct durable
        // sequence cannot retire the new task's exact session mapping.
        let mut old_terminal = queued.clone();
        old_terminal.sequence = queued.sequence + 1;
        old_terminal.state = VoiceState::Completed;
        service.update(&old_terminal).unwrap();
        assert!(queued_guard.guard.is_valid());
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let kdf_done = Arc::new(AtomicBool::new(false));
        std::thread::scope(|scope| {
            let held_backend = backend.0.clone();
            let started = entered.clone();
            let released = release.clone();
            let finished = kdf_done.clone();
            let thread = scope.spawn(move || {
                let mut backend = held_backend.lock().unwrap();
                started.store(true, Ordering::Release);
                // Real login KDF owns the global Backend mutex, while its CPU
                // work stays off the SQLite writer and the voice service.
                backend
                    .login_origin(
                        "admin",
                        Password::new("Independent voice password 2026!".into()).unwrap(),
                        TrustedOrigin::Local,
                    )
                    .unwrap();
                finished.store(true, Ordering::Release);
                let deadline = Instant::now() + Duration::from_secs(10);
                while !released.load(Ordering::Acquire) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
            let _release = Release(release.clone());
            let deadline = Instant::now() + Duration::from_secs(10);
            while !entered.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(
                !kdf_done.load(Ordering::Acquire),
                "the real Backend KDF must overlap the original memory-only permission"
            );
            assert!(backend.0.try_lock().is_err());
            assert!(queued_guard.guard.is_valid());
            let begin = Instant::now();
            let starting = runtime
                .drive(VoiceDriverUpdate {
                    job_id: queued.id,
                    expected_revision: queued.revision,
                    generation: 0,
                    action: VoiceDriverAction::Start,
                })
                .unwrap();
            assert!(
                begin.elapsed() < Duration::from_secs(1),
                "voice must not wait for the Backend/KDF mutex"
            );
            assert!(backend.0.try_lock().is_err());
            let starting_guard = runtime.guard(starting.id).unwrap();
            assert!(starting_guard.guard.is_valid());
            let playing = runtime
                .drive(VoiceDriverUpdate {
                    job_id: starting.id,
                    expected_revision: starting.revision,
                    generation: starting.generation,
                    action: VoiceDriverAction::FirstWrite,
                })
                .unwrap();
            let playing_guard = runtime.guard(playing.id).unwrap();
            assert!(playing_guard.guard.is_valid());
            let stopped = service
                .cancel(
                    web,
                    VoiceCancelRequest {
                        command_id: CommandId::new(302).unwrap(),
                        job_id: playing.id,
                        expected_revision: playing.revision,
                    },
                )
                .unwrap();
            assert!(!playing_guard.guard.is_valid());
            let terminal = runtime
                .drive(VoiceDriverUpdate {
                    job_id: stopped.id,
                    expected_revision: stopped.revision,
                    generation: playing.release_generation.unwrap(),
                    action: VoiceDriverAction::Released,
                })
                .unwrap();
            assert_eq!(terminal.state, VoiceState::Canceled);
            release.store(true, Ordering::Release);
            thread.join().unwrap();
        });
        assert!(kdf_done.load(Ordering::Acquire));
        let old = runtime.clone();
        let generation = service.invalidate_runtime();
        assert!(!old.is_current());
        assert!(!service.is_admitting());
        assert!(service.reserve(web, class).is_err());
        service.interrupt_all().unwrap();
        // A stale acknowledgement cannot reopen the new stop generation.
        let newer = service.invalidate_runtime();
        assert_ne!(newer, generation);
        assert!(service.resume_after_stop(generation).is_err());
        assert!(!service.is_admitting());
        service.resume_after_stop(newer).unwrap();
        let request = independent_request(&service, web, class, 303);
        let queued = service.submit(web, request).unwrap();
        assert!(old
            .drive(VoiceDriverUpdate {
                job_id: queued.id,
                expected_revision: queued.revision,
                generation: 0,
                action: VoiceDriverAction::Start
            })
            .is_err());
        let fresh = service.runtime_handle();
        assert!(fresh.is_current());
        let starting = fresh
            .drive(VoiceDriverUpdate {
                job_id: queued.id,
                expected_revision: queued.revision,
                generation: 0,
                action: VoiceDriverAction::Start,
            })
            .expect(
                "fresh runtime Start must revalidate the original exact lease on the real writer",
            );
        assert_eq!(starting.state, VoiceState::Starting);
        service.recover().unwrap();
    }
    struct HoldPublication {
        snapshot: Snapshot,
        entered: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
    }
    impl fc_storage::TransactionTask<Snapshot> for HoldPublication {
        fn allocation_bytes(&self) -> usize {
            std::mem::size_of::<Self>() + 256
        }
        fn evaluate(
            self: Box<Self>,
            _: &dyn fc_storage::TransactionDocuments,
        ) -> std::result::Result<fc_storage::TransactionDecision<Snapshot>, StorageError> {
            let Self {
                snapshot,
                entered,
                release,
            } = *self;
            Ok(fc_storage::TransactionDecision {
                outcome: Vec::new(),
                commit: Some((
                    vec![fc_storage::Mutation::Put {
                        key: "synthetic/voice/unrelated-page".into(),
                        value: vec![1],
                    }],
                    snapshot,
                )),
                // The real writer has opened its authority barrier and written
                // SQL here, but neither COMMIT nor publication has completed.
                precommit: Some(Box::new(move || {
                    entered.store(true, Ordering::Release);
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while !release.load(Ordering::Acquire) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Ok(())
                })),
            })
        }
    }
    #[test]
    fn actual_authority_publication_defers_without_revoking_then_rechecks_current_binding() {
        let _fixture = VOICE_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
        let (_directory, backend, web, class) = shared_fixture();
        let service = backend.0.lock().unwrap().voice_service();
        let queued = service
            .submit(web, independent_request(&service, web, class, 351))
            .unwrap();
        let permission = service.guard(queued.id).unwrap();
        let durable_job = service
            .inventory()
            .unwrap()
            .jobs
            .into_iter()
            .find(|j| j.id == queued.id)
            .unwrap();
        let runtime = service.runtime_handle();
        assert_eq!(
            runtime.availability_decision(),
            AudioAuthorization::Authorized
        );
        assert_eq!(permission.guard.decision(), AudioAuthorization::Authorized);
        let (handle, state, local) = {
            let backend = backend.0.lock().unwrap();
            (
                backend.handle.clone(),
                backend.state().unwrap(),
                issue(&backend, TrustedOrigin::Local),
            )
        };
        let status = service.inner.authority.gate().status();
        let snapshot = service
            .inner
            .authority
            .prepare_update(
                state,
                Revision::new(status.revision.get() + 1),
                status.auth_epoch,
            )
            .unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let _release = Release(release.clone());
        let pending = handle
            .try_transaction(HoldPublication {
                snapshot,
                entered: entered.clone(),
                release: release.clone(),
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            service.inner.authority.gate().status().health,
            Health::AuthorityUpdating
        );
        let build_started = Instant::now();
        let during_update = runtime.guard_for(&durable_job).unwrap();
        assert!(build_started.elapsed() < Duration::from_millis(100));
        assert_eq!(during_update.guard.decision(), AudioAuthorization::Deferred);
        let mut forged_deadline = durable_job.clone();
        forged_deadline.first_started_at = Some(forged_deadline.submitted_at);
        assert!(runtime.guard_for(&forged_deadline).is_err());
        for _ in 0..16 {
            assert_eq!(
                runtime.availability_decision(),
                AudioAuthorization::Deferred
            );
            assert_eq!(permission.guard.decision(), AudioAuthorization::Deferred);
            assert!(
                !permission.guard.is_valid(),
                "Deferred never grants a native Write"
            );
        }
        release.store(true, Ordering::Release);
        pending.wait().unwrap();
        assert_eq!(
            permission.guard.decision(),
            AudioAuthorization::Authorized,
            "an unrelated committed global revision must not permanently revoke the exact session"
        );
        let inventory = service.inventory().unwrap();
        assert_eq!(inventory.jobs[0].state, VoiceState::Queued);
        assert_eq!(inventory.jobs[0].completed_repeats, 0);
        assert_eq!(inventory.jobs[0].generation, 0);
        // Start admission races a real unrelated publication too: it must join
        // the bounded writer queue and revalidate there, not report revocation.
        let current = service.inner.authority.snapshot().unwrap();
        let status = service.inner.authority.gate().status();
        let snapshot = service
            .inner
            .authority
            .prepare_update(
                current.data.clone(),
                Revision::new(status.revision.get() + 1),
                status.auth_epoch,
            )
            .unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let _release = Release(release.clone());
        let pending = handle
            .try_transaction(HoldPublication {
                snapshot,
                entered: entered.clone(),
                release: release.clone(),
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            service.inner.authority.gate().status().health,
            Health::AuthorityUpdating
        );
        assert_eq!(handle.queued_bytes(), 0);
        let (observer_ready, ready) = std::sync::mpsc::sync_channel(1);
        let starting = std::thread::scope(|scope| {
            let observer = scope.spawn(|| {
                let _release = Release(release.clone());
                observer_ready.send(()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                // This private writer is held and Start is its only producer.
                // Budget reservation precedes try_send: this observes Start's
                // queue reservation, not a successful-send acknowledgement.
                while handle.queued_bytes() == 0 {
                    assert!(
                        Instant::now() < deadline,
                        "Start did not reserve queue budget"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                release.store(true, Ordering::Release);
            });
            ready.recv_timeout(Duration::from_secs(10)).unwrap();
            let starting = runtime.drive(VoiceDriverUpdate {
                job_id: queued.id,
                expected_revision: queued.revision,
                generation: 0,
                action: VoiceDriverAction::Start,
            });
            observer.join().unwrap();
            starting.unwrap()
        });
        pending.wait().unwrap();
        assert_eq!(starting.state, VoiceState::Starting);
        assert_eq!(starting.completed_repeats, 0);
        let permission = runtime.guard_for(&starting).unwrap();
        assert_eq!(permission.guard.decision(), AudioAuthorization::Authorized);
        {
            let mut backend = backend.0.lock().unwrap();
            let other = ClassId::new(2).unwrap();
            backend
                .identity(
                    local,
                    IdentityCommand::CreateClass {
                        id: other,
                        name: "Other synthetic class".into(),
                    },
                )
                .unwrap();
            assert_eq!(permission.guard.decision(), AudioAuthorization::Authorized);
            backend
                .identity(local, IdentityCommand::BindCurrentClass { id: other })
                .unwrap();
        }
        assert_eq!(
            permission.guard.decision(),
            AudioAuthorization::Revoked,
            "Deferred must recheck the newly committed real binding before allowing any buffer"
        );
        service.interrupt_all().unwrap();
    }
    fn hold_publication(
        service: &VoiceService,
        handle: &StorageHandle<Snapshot>,
    ) -> (fc_storage::Ticket<Vec<u8>>, Arc<AtomicBool>) {
        let current = service.inner.authority.snapshot().unwrap();
        let status = service.inner.authority.gate().status();
        let snapshot = service
            .inner
            .authority
            .prepare_update(
                current.data.clone(),
                Revision::new(status.revision.get() + 1),
                status.auth_epoch,
            )
            .unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let pending = handle
            .try_transaction(HoldPublication {
                snapshot,
                entered: entered.clone(),
                release: release.clone(),
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        (pending, release)
    }
    #[test]
    fn actor_waits_for_real_publication_and_busy_preserves_exact_voice_runtime() {
        let _fixture = VOICE_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
        let (_directory, backend, web, class) = shared_fixture();
        let service = backend.0.lock().unwrap().voice_service();
        let queued = service
            .submit(web, independent_request(&service, web, class, 451))
            .unwrap();
        let runtime = service.runtime_handle();
        let permission = runtime.guard(queued.id).unwrap();
        let handle = backend.0.lock().unwrap().handle.clone();
        let (pending, release) = hold_publication(&service, &handle);
        let _release = Release(release.clone());
        let status = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(10));
                release.store(true, Ordering::Release);
            });
            service.status(web, queued.id).unwrap()
        });
        pending.wait().unwrap();
        assert_eq!(status.id, queued.id);
        assert!(status.authorization.is_none());
        assert!(runtime.is_current());
        assert!(permission.guard.is_valid());

        // A prepared proof is consumed once after a short publication pulse;
        // no SQL command or file preparation is automatically retried.
        let proof = prepared(service.reserve(web, class).unwrap());
        let (pending, release) = hold_publication(&service, &handle);
        let _release = Release(release.clone());
        let published = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(10));
                release.store(true, Ordering::Release);
            });
            service.publish(web, proof).unwrap()
        });
        pending.wait().unwrap();
        assert_eq!(published.state, VoiceAssetState::Ready);
        assert!(permission.guard.is_valid());

        let (pending, release) = hold_publication(&service, &handle);
        let _release = Release(release.clone());
        let generation = service.generation();
        let begin = Instant::now();
        let error = std::thread::scope(|scope| {
            let waiting = scope.spawn(|| service.status(web, queued.id));
            // The waiting actor does not own the ledger while sleeping, and
            // cannot block independent memory-only cancellation/session control.
            std::thread::sleep(Duration::from_millis(10));
            assert!(service.inner.ledger.try_lock().is_ok());
            assert_eq!(permission.guard.decision(), AudioAuthorization::Deferred);
            waiting.join().unwrap().unwrap_err()
        });
        assert!(
            matches!(error, BackendError::Storage(StorageError::Busy)),
            "{error:?}"
        );
        assert!(begin.elapsed() < Duration::from_millis(500));
        assert_eq!(service.generation(), generation);
        assert!(runtime.is_current());
        assert!(service.is_admitting());
        release.store(true, Ordering::Release);
        pending.wait().unwrap();
        assert!(service.status(web, queued.id).is_ok());
        assert!(permission.guard.is_valid());
        let asset_count = service.inventory().unwrap().assets.len();
        let (pending, release) = hold_publication(&service, &handle);
        let _release = Release(release.clone());
        let generation = service.generation();
        let error = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(10));
                service.invalidate_runtime();
                release.store(true, Ordering::Release);
            });
            service.reserve(web, class).unwrap_err()
        });
        pending.wait().unwrap();
        assert!(
            matches!(
                error,
                BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::Permission(
                    PermissionError::StaleAuthorization
                )))
            ),
            "{error:?}"
        );
        assert_eq!(
            service.generation(),
            generation + 1,
            "a pre-stop actor waiter must not invalidate or adopt the next runtime epoch"
        );
        assert_eq!(service.inventory().unwrap().assets.len(), asset_count);
        assert!(!runtime.is_current());
        service.interrupt_all().unwrap();
    }
    #[test]
    fn actor_wait_then_publish_rechecks_actual_new_binding_without_replaying_sql() {
        let _fixture = VOICE_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
        let (_directory, backend, web, class) = shared_fixture();
        let service = backend.0.lock().unwrap().voice_service();
        let other = ClassId::new(2).unwrap();
        let (local, mut repository, request, actor, clock) = {
            let mut backend = backend.0.lock().unwrap();
            let local = issue(&backend, TrustedOrigin::Local);
            backend
                .identity(
                    local,
                    IdentityCommand::CreateClass {
                        id: other,
                        name: "Actual new binding".into(),
                    },
                )
                .unwrap();
            let state = backend.state().unwrap();
            let actor = backend
                .actor(&state, local, SessionPurpose::Manage)
                .unwrap()
                .0;
            let request = backend
                .request(&state, IdentityCommand::BindCurrentClass { id: other })
                .unwrap();
            (
                local,
                backend.repository.clone(),
                request,
                actor,
                backend.clock.clone(),
            )
        };
        let reservation = service.reserve(web, class).unwrap();
        let prepared = prepared(reservation);
        let generation = service.generation();
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let _release = Release(release.clone());
        let writer_entered = entered.clone();
        let writer_release = release.clone();
        let exact = service.clone();
        repository = repository.with_session_guard(Arc::new(move |state, expected| {
            let actual = exact
                .actor_from_state(local, state)
                .map_err(|_| StorageError::AuthorityUnavailable)?;
            if actual != *expected {
                return Err(StorageError::AuthorityUnavailable);
            }
            if exact.inner.control.health_gate().status().health == Health::AuthorityUpdating {
                writer_entered.store(true, Ordering::Release);
                let deadline = Instant::now() + Duration::from_secs(10);
                while !writer_release.load(Ordering::Acquire) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            Ok(())
        }));
        std::thread::scope(|scope| {
            let identity = scope.spawn(move || {
                fc_ports::IdentityRepository::execute_identity(
                    &mut repository,
                    request,
                    Some(&actor),
                    clock.as_ref(),
                )
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while !entered.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(10));
                release.store(true, Ordering::Release);
            });
            let error = service.publish(web, prepared).unwrap_err();
            assert!(
                matches!(
                    error,
                    BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::TargetMismatch))
                ),
                "{error:?}"
            );
            identity.join().unwrap().unwrap();
        });
        assert_eq!(service.generation(), generation);
        assert!(service.is_admitting());
        // The consumed proof was never retried or written: it remains Reserved.
        let state = service.inventory().unwrap();
        assert_eq!(state.assets.len(), 1);
        assert_eq!(state.assets[0].state, VoiceAssetState::Reserved);
        let current = backend.0.lock().unwrap().state().unwrap();
        assert_eq!(current.installation.unwrap().current_class, Some(other));
    }
    struct HoldWriter {
        entered: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
    }
    impl fc_storage::TransactionTask<Snapshot> for HoldWriter {
        fn allocation_bytes(&self) -> usize {
            std::mem::size_of::<Self>()
        }
        fn evaluate(
            self: Box<Self>,
            _: &dyn fc_storage::TransactionDocuments,
        ) -> std::result::Result<fc_storage::TransactionDecision<Snapshot>, StorageError> {
            self.entered.store(true, Ordering::Release);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !self.release.load(Ordering::Acquire) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(fc_storage::TransactionDecision {
                outcome: Vec::new(),
                commit: None,
                precommit: None,
            })
        }
    }
    #[test]
    fn voice_timeout_invalidates_runtime_and_late_submit_cannot_commit_or_reopen_admission() {
        let _fixture = VOICE_FIXTURE.lock().unwrap_or_else(|p| p.into_inner());
        let (_directory, backend, web, class) = shared_fixture();
        let service = backend.0.lock().unwrap().voice_service();
        let first = service
            .submit(web, independent_request(&service, web, class, 401))
            .unwrap();
        let runtime = service.runtime_handle();
        let permission = runtime.guard(first.id).unwrap();
        assert!(permission.guard.is_valid());
        let late = independent_request(&service, web, class, 402);
        let handle = backend.0.lock().unwrap().handle.clone();
        let before = handle.try_revision().unwrap().wait().unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let _release = Release(release.clone());
        let blocker = handle
            .try_transaction(HoldWriter {
                entered: entered.clone(),
                release: release.clone(),
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        // Enqueue the actual positive Submit behind the blocked writer. The
        // normal public method first reads inventory; bypass only that already
        // satisfied read here so this test covers a late mutating ticket, not
        // merely a timed-out query. Exact actor/session/generation hooks are the
        // same repository and conclude path used by VoiceService::submit.
        let actor = service.actor(web, true).unwrap();
        let repository = service.repository(runtime.generation(), Some(web));
        let begin = Instant::now();
        let error = service
            .conclude(repository.submit_voice(late, &actor))
            .unwrap_err();
        assert!(matches!(
            error,
            BackendError::Voice(VoiceRepositoryError::Faulted)
        ));
        assert!(begin.elapsed() < Duration::from_secs(1));
        assert!(!permission.guard.is_valid());
        assert!(!runtime.is_current());
        assert!(!service.is_admitting());
        assert!(service.reserve(web, class).is_err());
        release.store(true, Ordering::Release);
        blocker.wait().unwrap();
        let state = service.inventory().unwrap();
        assert_eq!(state.jobs.len(), 1);
        assert_eq!(state.jobs[0].id, first.id);
        assert_eq!(
            handle.try_revision().unwrap().wait().unwrap(),
            before,
            "expired queued voice operation may not COMMIT later"
        );
        let interrupted = runtime.interrupt_all().unwrap();
        assert_eq!(interrupted.jobs[0].state, VoiceState::Interrupted);
        // Cleanup remains available to an invalidated runtime, but no positive
        // update or native permission may cross into the next runtime epoch.
        assert!(runtime
            .drive(VoiceDriverUpdate {
                job_id: first.id,
                expected_revision: first.revision,
                generation: 0,
                action: VoiceDriverAction::Start
            })
            .is_err());
        let generation = service.generation();
        service.resume_after_stop(generation).unwrap();
        assert!(service.is_admitting());
        assert!(!permission.guard.is_valid());
        assert!(!runtime.is_current());
        let recovered = service.recover().unwrap();
        assert!(recovered.jobs.iter().all(|j| j.state.terminal()));
    }
}
