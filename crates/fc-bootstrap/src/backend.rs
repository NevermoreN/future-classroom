//! Synchronous product business service. Invoke only on the shared bounded worker
//! pool: every SQLite wait and every password KDF is deliberately off the UI thread.
//! UI session controls use atomics and a private atomic coalescing input stream instead.
mod voice;
pub mod web;
use fc_application::identity::{IdentitySessions, SessionError, SessionLease, SessionPurpose};
use fc_domain::{
    auth::*,
    content::*,
    desktop::{DesktopFacts, DisplaySettings},
    identity::*,
    ids::*,
    lesson_context::{LessonAction, LessonContextSnapshot},
    materials::*,
    schedule_document::ScheduleDocument,
    time::*,
};
use fc_ports::{
    prepare_initial_credentials, prepare_password_change, prepare_recovery,
    prepare_recovery_rotation, Clock, CredentialCrypto, CredentialIdentityRepository,
    DisplayRepository, DisplayRepositoryError, DisplayRequest, IdentityRepositoryError,
    IdentityRequest, LessonRepository, LessonRepositoryError, LessonRequest, MaterialDeleteRequest,
    MaterialListRequest, MaterialRepository, MaterialRepositoryError, MonotonicClock,
    PreparedCredentialChange, PreparedMaterialAppend, RandomSource, RepositoryError,
    ScheduleRepository, ScheduleRepositoryError, ScheduleRequest,
};
use fc_runtime::{
    authority::{AuthorityStore, PreparedAuthority},
    health::HealthGate,
};
use fc_security::{
    CredentialCryptoAdapter, CredentialRecordCodec, Password, PasswordRecord, RecoveryCode,
    RecoveryRecord,
};
pub use fc_storage::business::materials::{
    ResourceInventory, ResourceInventoryPage, ResourceRetirement, ResourceState,
};
use fc_storage::{
    business::{startup_state, BusinessRepository},
    login::*,
    StorageError, StorageHandle, StorageWriter,
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
pub use voice::{VoiceGuard, VoicePlaybackAuthorization, VoiceRuntimeHandle, VoiceService};

pub type Result<T> = std::result::Result<T, BackendError>;
type Snapshot = PreparedAuthority<IdentityState>;
type Repository = BusinessRepository<Snapshot>;
type Change = PreparedCredentialChange<PasswordRecord, RecoveryRecord>;
const SESSION_MILLIS: u64 = 86_400_000;

/// Contains status codes only, never submitted passwords, recovery codes or records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendError {
    Closed,
    Session(SessionError),
    Identity(IdentityRepositoryError),
    Content(RepositoryError),
    Schedule(ScheduleRepositoryError),
    Lesson(LessonRepositoryError),
    Display(DisplayRepositoryError),
    Material(MaterialRepositoryError),
    Voice(fc_ports::voice::VoiceRepositoryError),
    Storage(StorageError),
    InvalidCredentials,
    CryptoUnavailable,
    CorruptCredential,
    InvalidInput,
    Overflow,
}
impl From<fc_ports::voice::VoiceRepositoryError> for BackendError {
    fn from(e: fc_ports::voice::VoiceRepositoryError) -> Self {
        Self::Voice(e)
    }
}
impl From<StorageError> for BackendError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}
impl From<IdentityRepositoryError> for BackendError {
    fn from(e: IdentityRepositoryError) -> Self {
        Self::Identity(e)
    }
}
impl From<SessionError> for BackendError {
    fn from(e: SessionError) -> Self {
        Self::Session(e)
    }
}
impl From<ScheduleRepositoryError> for BackendError {
    fn from(error: ScheduleRepositoryError) -> Self {
        Self::Schedule(error)
    }
}
impl From<LessonRepositoryError> for BackendError {
    fn from(error: LessonRepositoryError) -> Self {
        Self::Lesson(error)
    }
}
impl From<DisplayRepositoryError> for BackendError {
    fn from(error: DisplayRepositoryError) -> Self {
        Self::Display(error)
    }
}
impl From<MaterialRepositoryError> for BackendError {
    fn from(error: MaterialRepositoryError) -> Self {
        Self::Material(error)
    }
}
impl From<RepositoryError> for BackendError {
    fn from(e: RepositoryError) -> Self {
        Self::Content(e)
    }
}

#[derive(Clone, Debug)]
pub struct Initialize {
    pub school_name: String,
    pub login_name: String,
    pub display_name: String,
}
struct Control {
    health: HealthGate,
    generation: AtomicU64,
    active_local: AtomicU64,
    active_principal: AtomicU64,
    closed: AtomicBool,
    input_sequence: AtomicU64,
    input_lease: AtomicU64,
    input_generation: AtomicU64,
    input_first: AtomicU64,
    input_last: AtomicU64,
    material_scene: Mutex<Option<PublishedMaterialScene>>,
}
#[derive(Clone)]
struct PublishedMaterialScene {
    source: MaterialSource,
    generation: SceneGeneration,
    revision: Revision,
}
/// Fixed metadata plus a short, memory-only final execution permission. Preparing
/// a launch copy does not extend this permission; fetch a fresh authorization
/// after preparation, immediately before entering the Shell execution domain.
pub struct MaterialOpenAuthorization {
    pub record: MaterialRecord,
    pub guard: MaterialOpenGuard,
}
#[derive(Clone)]
pub struct MaterialOpenGuard {
    check: Arc<dyn Fn() -> bool + Send + Sync>,
    pub expires_at: Instant,
}
impl MaterialOpenGuard {
    /// Preparation-only cancellation gate. Keeps the original management/session
    /// or scene validity without renewing it and without the dispatch-only 2s cap.
    pub fn is_context_valid(&self) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.check)())).unwrap_or(false)
    }
    pub fn is_valid(&self) -> bool {
        Instant::now() < self.expires_at && self.is_context_valid()
    }
}
/// Clone into the UI. These operations never acquire the backend/session mutex or
/// wait for SQLite, KDFs, a worker slot, or other UI input producers.
#[derive(Clone)]
pub struct SessionControl(Arc<Control>);
impl SessionControl {
    pub fn publish_material_scene(
        &self,
        source: &MaterialSource,
        generation: SceneGeneration,
        revision: Revision,
    ) -> bool {
        let status = self.0.health.status();
        if self.0.closed.load(Ordering::Acquire)
            || status.health != fc_runtime::health::Health::Healthy
            || status.revision != revision
        {
            return false;
        }
        let Ok(mut scene) = self.0.material_scene.lock() else {
            self.0.health.fault();
            return false;
        };
        let status = self.0.health.status();
        if status.health != fc_runtime::health::Health::Healthy || status.revision != revision {
            return false;
        }
        *scene = Some(PublishedMaterialScene {
            source: source.clone(),
            generation,
            revision,
        });
        true
    }
    pub fn clear_material_scene(&self) {
        match self.0.material_scene.lock() {
            Ok(mut scene) => {
                *scene = None;
            }
            Err(poisoned) => {
                *poisoned.into_inner() = None;
                self.0.health.fault();
            }
        }
    }
    /// Pure memory gate for a CURRENT_SCENE launch: never waits for Backend/KDF/SQL.
    pub fn material_open_guard(
        &self,
        source: &MaterialSource,
        generation: SceneGeneration,
        revision: Revision,
    ) -> bool {
        let status = self.0.health.status();
        if self.0.closed.load(Ordering::Acquire)
            || status.health != fc_runtime::health::Health::Healthy
            || status.revision != revision
        {
            return false;
        }
        self.0.material_scene.lock().is_ok_and(|scene| {
            scene.as_ref().is_some_and(|s| {
                s.source == *source && s.generation == generation && s.revision == revision
            })
        })
    }
    /// Read-only render gate; the UI can reject stale facts without taking the
    /// backend or session ledger mutex and without refreshing a management lease.
    pub fn health_gate(&self) -> HealthGate {
        self.0.health.clone()
    }
    /// Immediately fail the real authority gate without waiting for a backend
    /// mutex/KDF. This irreversible fault also cancels old effect permissions.
    pub fn fault(&self) {
        self.0.health.fault();
        self.0.closed.store(true, Ordering::Release);
        self.lock_local();
    }
    pub fn lock_local(&self) {
        self.clear_material_scene();
        // A saturated generation remains permanently closed rather than wrap old leases.
        if self
            .0
            .generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .is_err()
        {
            self.0.closed.store(true, Ordering::Release);
        }
        self.0.active_local.store(0, Ordering::Release);
        self.0.active_principal.store(0, Ordering::Release);
    }
    pub fn note_input(&self, lease: SessionLease, tick: u64) -> bool {
        if self.0.closed.load(Ordering::Acquire)
            || lease.origin() != TrustedOrigin::Local
            || self.0.active_local.load(Ordering::Acquire) != lease.id()
        {
            return false;
        }
        let sequence = self.0.input_sequence.load(Ordering::Acquire);
        if sequence > u64::MAX - 2
            || !sequence.is_multiple_of(2)
            || self
                .0
                .input_sequence
                .compare_exchange(sequence, sequence + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return false;
        }
        let generation = self.generation();
        let matches = self.0.input_lease.load(Ordering::Relaxed) == lease.id()
            && self.0.input_generation.load(Ordering::Relaxed) == generation;
        let previous = self.0.input_last.load(Ordering::Relaxed);
        let continuous = matches
            && tick >= previous
            && tick - previous < fc_application::identity::LOCAL_IDLE_MILLIS;
        if continuous {
            self.0.input_last.store(tick, Ordering::Relaxed);
        }
        self.0.input_sequence.store(sequence + 2, Ordering::Release);
        if matches && !continuous {
            self.lock_local();
        }
        continuous && self.generation() == generation
    }
    fn install_input(&self, lease: SessionLease, generation: u64, tick: u64) -> Result<()> {
        let sequence = self.0.input_sequence.load(Ordering::Acquire);
        if sequence > u64::MAX - 2
            || !sequence.is_multiple_of(2)
            || self
                .0
                .input_sequence
                .compare_exchange(sequence, sequence + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(BackendError::Session(SessionError::Revoked));
        }
        self.0.input_lease.store(lease.id(), Ordering::Relaxed);
        self.0.input_generation.store(generation, Ordering::Relaxed);
        self.0.input_first.store(tick, Ordering::Relaxed);
        self.0.input_last.store(tick, Ordering::Relaxed);
        self.0.input_sequence.store(sequence + 2, Ordering::Release);
        Ok(())
    }
    fn input_snapshot(&self) -> Option<(u64, u64, u64, u64)> {
        for _ in 0..3 {
            let sequence = self.0.input_sequence.load(Ordering::Acquire);
            if sequence > u64::MAX - 2 || !sequence.is_multiple_of(2) {
                continue;
            }
            let value = (
                self.0.input_lease.load(Ordering::Relaxed),
                self.0.input_generation.load(Ordering::Relaxed),
                self.0.input_first.load(Ordering::Relaxed),
                self.0.input_last.load(Ordering::Relaxed),
            );
            std::sync::atomic::fence(Ordering::Acquire);
            if self.0.input_sequence.load(Ordering::Acquire) == sequence {
                return Some(value);
            }
        }
        None
    }
    pub fn is_local_locked(&self) -> bool {
        self.0.active_local.load(Ordering::Acquire) == 0 || self.0.closed.load(Ordering::Acquire)
    }
    fn generation(&self) -> u64 {
        self.0.generation.load(Ordering::Acquire)
    }
    fn check(&self, lease: SessionLease, generation: u64) -> Result<()> {
        if self.0.closed.load(Ordering::Acquire) {
            return Err(BackendError::Closed);
        }
        if lease.origin() == TrustedOrigin::Local
            && (generation != self.generation()
                || self.0.active_local.load(Ordering::Acquire) != lease.id())
        {
            return Err(BackendError::Session(SessionError::Revoked));
        }
        Ok(())
    }
}
struct Ledger {
    sessions: IdentitySessions,
    observed_generation: u64,
    local: Option<SessionLease>,
}
impl Ledger {
    fn synchronize(
        &mut self,
        control: &SessionControl,
        state: &IdentityState,
        wall: LocalDateTime,
        tick: u64,
    ) {
        let generation = control.generation();
        if generation != self.observed_generation {
            self.sessions.lock_local();
            self.local = None;
            self.observed_generation = generation;
        }
        if let (Some(lease), Some((id, input_generation, first, last))) =
            (self.local, control.input_snapshot())
        {
            if lease.id() == id
                && id == control.0.active_local.load(Ordering::Acquire)
                && generation == input_generation
            {
                let _ = self
                    .sessions
                    .local_input_stream(state, lease, wall, tick, first, last);
            }
        }
    }
}
struct PlatformRandom;
impl RandomSource for PlatformRandom {
    type Error = ();
    fn fill(&mut self, output: &mut [u8]) -> std::result::Result<(), ()> {
        #[cfg(windows)]
        {
            fc_windows::SystemRandom.fill(output).map_err(|_| ())
        }
        #[cfg(not(windows))]
        {
            use std::io::Read;
            std::fs::File::open("/dev/urandom")
                .and_then(|mut file| file.read_exact(output))
                .map_err(|_| ())
        }
    }
}

pub struct Backend {
    web: web::WebService,
    voice: voice::VoiceService,
    writer: Option<StorageWriter<Snapshot>>,
    handle: StorageHandle<Snapshot>,
    repository: Repository,
    codec: CredentialRecordCodec,
    crypto: CredentialCryptoAdapter<PlatformRandom>,
    clock: Arc<dyn Clock + Send + Sync>,
    monotonic: Arc<dyn MonotonicClock + Send + Sync>,
    ledger: Arc<Mutex<Ledger>>,
    control: SessionControl,
}
impl Backend {
    /// Caller already owns the deployment instance lock and has restored the codec
    /// key through the deployment account's protected key store.
    pub fn open(
        path: impl AsRef<Path>,
        codec: CredentialRecordCodec,
        clock: Arc<dyn Clock + Send + Sync>,
        monotonic: Arc<dyn MonotonicClock + Send + Sync>,
    ) -> Result<Self> {
        let facts = startup_state(path.as_ref())?;
        let (mut publisher, reader) =
            AuthorityStore::new(facts.identity, facts.revision, AuthEpoch::new(1));
        publisher
            .initialize()
            .map_err(|_| BackendError::Storage(StorageError::AuthorityUnavailable))?;
        let writer = StorageWriter::open_with_login_clock(path, 16, publisher, monotonic.clone())?;
        let handle = writer.handle();
        let health = reader.gate();
        let authority = reader.clone();
        let factory = Arc::new(move |state: &IdentityState, revision: Revision| {
            reader
                .prepare_update(state.clone(), revision, AuthEpoch::new(1))
                .map_err(|_| StorageError::AuthorityUnavailable)
        });
        let repository = BusinessRepository::new(handle.clone(), clock.clone(), factory);
        // Exclusive deployment ownership is already held. Recovered voice work
        // becomes Interrupted before any new output executor can be constructed.
        repository.recover_voice()?;
        let control = SessionControl(Arc::new(Control {
            health,
            generation: AtomicU64::new(1),
            active_local: AtomicU64::new(0),
            active_principal: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            input_sequence: AtomicU64::new(0),
            input_lease: AtomicU64::new(0),
            input_generation: AtomicU64::new(0),
            input_first: AtomicU64::new(0),
            input_last: AtomicU64::new(0),
            material_scene: Mutex::new(None),
        }));
        let scene_control = control.clone();
        let repository =
            repository.with_material_scene_guard(Arc::new(move |source, generation| {
                let revision = scene_control.health_gate().status().revision;
                if scene_control.material_open_guard(source, generation, revision) {
                    Ok(())
                } else {
                    Err(StorageError::Material(MaterialError::InvalidContext))
                }
            }));
        let ledger = Arc::new(Mutex::new(Ledger {
            sessions: IdentitySessions::default(),
            observed_generation: 1,
            local: None,
        }));
        let voice = voice::VoiceService::new(
            repository.clone(),
            ledger.clone(),
            control.clone(),
            authority.clone(),
            clock.clone(),
            monotonic.clone(),
        );
        let web = web::WebService::new(
            ledger.clone(),
            control.clone(),
            authority,
            clock.clone(),
            monotonic.clone(),
        );
        Ok(Self {
            web,
            voice,
            writer: Some(writer),
            handle,
            repository,
            codec,
            crypto: CredentialCryptoAdapter::new(PlatformRandom),
            clock,
            monotonic,
            ledger,
            control,
        })
    }
    pub fn session_control(&self) -> SessionControl {
        self.control.clone()
    }
    pub fn web_service(&self) -> web::WebService {
        self.web.clone()
    }
    fn check_open(&self) -> Result<()> {
        if self.writer.is_none() || self.control.0.closed.load(Ordering::Acquire) {
            Err(BackendError::Closed)
        } else {
            self.control
                .0
                .health
                .check_sensitive()
                .map(|_| ())
                .map_err(|_| BackendError::Storage(StorageError::AuthorityUnavailable))
        }
    }
    pub fn state(&self) -> Result<IdentityState> {
        self.check_open()?;
        let state = self.repository.load_identity()?;
        self.check_open()?;
        Ok(state)
    }
    pub fn global_revision(&self) -> Result<Revision> {
        self.check_open()?;
        let revision = self.handle.try_revision()?.wait()?;
        self.check_open()?;
        Ok(revision)
    }
    /// Authenticated native exit. Consume the exact local lease immediately
    /// before closing; no database/KDF lookup occurs after this final admission.
    pub fn close_authorized(&mut self, lease: SessionLease) -> Result<()> {
        self.authorize_exit(lease)?;
        self.close()
    }
    /// Consume the exact exit authorization without closing the writer. Normal
    /// composition first drains the audio domain, then closes SQLite on a worker.
    pub fn authorize_exit(&mut self, lease: SessionLease) -> Result<()> {
        if lease.origin() != TrustedOrigin::Local {
            return Err(BackendError::Identity(IdentityRepositoryError::Domain(
                IdentityError::Permission(PermissionError::LocalOnly),
            )));
        }
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        state
            .authority(actor.principal_id)
            .map_err(|e| BackendError::Identity(IdentityRepositoryError::Domain(e)))?
            .authorize_global_local(&actor, self.clock.now())
            .map_err(|e| {
                BackendError::Identity(IdentityRepositoryError::Domain(IdentityError::Permission(
                    e,
                )))
            })?;
        self.control.check(lease, generation)?;
        self.control
            .0
            .active_local
            .compare_exchange(lease.id(), 0, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| BackendError::Session(SessionError::Revoked))?;
        // A lock which was already advancing its generation must win. Once
        // this admission settles, later lock events cannot undo started shutdown.
        if self.control.generation() != generation {
            return Err(BackendError::Session(SessionError::Revoked));
        }
        Ok(())
    }
    /// Explicit joining close belongs on a shutdown worker/coordinator, never the UI.
    pub fn close(&mut self) -> Result<()> {
        self.web.close();
        self.voice.invalidate_runtime();
        self.control.0.health.stop();
        self.control.0.closed.store(true, Ordering::Release);
        self.control.lock_local();
        if let Some(writer) = self.writer.take() {
            writer.shutdown()?;
        }
        Ok(())
    }
    fn command_id(&mut self) -> Result<CommandId> {
        for _ in 0..4 {
            let mut bytes = [0u8; 8];
            self.crypto_random(&mut bytes)?;
            if let Ok(id) = CommandId::new(u64::from_le_bytes(bytes)) {
                return Ok(id);
            }
        }
        Err(BackendError::CryptoUnavailable)
    }
    fn crypto_random(&self, bytes: &mut [u8]) -> Result<()> {
        PlatformRandom
            .fill(bytes)
            .map_err(|_| BackendError::CryptoUnavailable)
    }
    fn request(
        &mut self,
        state: &IdentityState,
        command: IdentityCommand,
    ) -> Result<IdentityRequest> {
        Ok(IdentityRequest {
            command_id: self.command_id()?,
            expected_revision: state.revision,
            command,
        })
    }
    fn validate_identity(
        &self,
        state: &IdentityState,
        request: &IdentityRequest,
        actor: Option<&ActorContext>,
    ) -> Result<()> {
        if state.revision != request.expected_revision {
            return Err(BackendError::Identity(IdentityRepositoryError::Domain(
                IdentityError::RevisionConflict,
            )));
        }
        state
            .transition(&request.command, actor, self.clock.now())
            .map(|_| ())
            .map_err(|e| BackendError::Identity(IdentityRepositoryError::Domain(e)))
    }
    fn encode(
        &self,
        change: Change,
        id: PrincipalId,
        password_version: u64,
        recovery_version: u64,
    ) -> Result<PreparedCredentialChange<Vec<u8>, Vec<u8>>> {
        change
            .try_map(
                |record| {
                    self.codec
                        .encode_password(id, password_version, &record)
                        .map(|e| e.into_storage_bytes())
                },
                |record| {
                    self.codec
                        .encode_recovery(id, recovery_version, &record)
                        .map(|e| e.into_storage_bytes())
                },
            )
            .map_err(|_| BackendError::CryptoUnavailable)
    }
    fn apply(&self, transition: &IdentityTransition) -> Result<()> {
        let mut ledger = self.ledger.lock().map_err(|_| BackendError::Closed)?;
        ledger.sessions.apply_events(&transition.events);
        // Every affected local lease must stop UI admission immediately as well.
        if transition.events.iter().any(|e| match e {
            IdentityEvent::AllAuthorizationRevoked => true,
            IdentityEvent::AuthorizationRevoked(id) => {
                id.get() == self.control.0.active_principal.load(Ordering::Acquire)
            }
            _ => false,
        }) {
            self.control.lock_local();
        }
        ledger.synchronize(
            &self.control,
            &transition.next,
            self.clock.now(),
            self.monotonic.monotonic_millis(),
        );
        Ok(())
    }
    fn actor(
        &self,
        state: &IdentityState,
        lease: SessionLease,
        purpose: SessionPurpose,
    ) -> Result<(ActorContext, u64)> {
        let generation = self.control.generation();
        self.control.check(lease, generation)?;
        let mut ledger = self.ledger.lock().map_err(|_| BackendError::Closed)?;
        ledger.synchronize(
            &self.control,
            state,
            self.clock.now(),
            self.monotonic.monotonic_millis(),
        );
        let actor = ledger.sessions.actor(
            state,
            lease,
            self.clock.now(),
            self.monotonic.monotonic_millis(),
            purpose,
        )?;
        self.control.check(lease, generation)?;
        Ok((actor, generation))
    }
    fn guarded(&self, lease: SessionLease, generation: u64, purpose: SessionPurpose) -> Repository {
        let ledger = self.ledger.clone();
        let control = self.control.clone();
        let wall = self.clock.clone();
        let monotonic = self.monotonic.clone();
        self.repository
            .clone()
            .with_session_guard(Arc::new(move |state, expected| {
                control
                    .check(lease, generation)
                    .map_err(|_| StorageError::AuthorityUnavailable)?;
                let mut ledger = ledger
                    .lock()
                    .map_err(|_| StorageError::AuthorityUnavailable)?;
                ledger.synchronize(&control, state, wall.now(), monotonic.monotonic_millis());
                let actual = ledger
                    .sessions
                    .actor(
                        state,
                        lease,
                        wall.now(),
                        monotonic.monotonic_millis(),
                        purpose,
                    )
                    .map_err(|_| StorageError::AuthorityUnavailable)?;
                control
                    .check(lease, generation)
                    .map_err(|_| StorageError::AuthorityUnavailable)?;
                if actual == *expected {
                    Ok(())
                } else {
                    Err(StorageError::AuthorityUnavailable)
                }
            }))
    }
    pub fn check_session(
        &self,
        lease: SessionLease,
        purpose: SessionPurpose,
    ) -> Result<ActorContext> {
        let state = self.state()?;
        self.actor(&state, lease, purpose).map(|(actor, _)| actor)
    }
    /// Explicit account logout persists a new authorization epoch. All local
    /// and Web channels for this account then fail even before event delivery.
    /// Locking the local workbench remains a separate local-only operation.
    pub fn logout_account(&mut self, lease: SessionLease) -> Result<()> {
        let actor = self.check_session(lease, SessionPurpose::Manage)?;
        self.identity(
            lease,
            IdentityCommand::LogoutAccount {
                id: actor.principal_id,
                expected_auth_epoch: actor.auth_epoch,
            },
        )?;
        Ok(())
    }
    pub fn role(&self, lease: SessionLease) -> Result<Role> {
        let state = self.state()?;
        self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(state
            .account(lease.principal_id())
            .map_err(|_| BackendError::InvalidCredentials)?
            .principal
            .role)
    }
    fn admission(&self, id: PrincipalId, kind: CredentialKind) -> Result<Admission> {
        match self.handle.try_login_admission(id, kind)?.wait()? {
            Response::Admitted(a) => Ok(a),
            _ => Err(BackendError::InvalidInput),
        }
    }
    fn finish(&self, reservation: Reservation, verification: Verification) -> Result<Completion> {
        match self
            .handle
            .try_finish_login(reservation, verification)?
            .wait()?
        {
            Response::Completed(c) => Ok(c),
            _ => Err(BackendError::InvalidInput),
        }
    }
    fn authenticated(
        &self,
        completion: Completion,
        state: &IdentityState,
        id: PrincipalId,
    ) -> Result<()> {
        let account = state
            .account(id)
            .map_err(|_| BackendError::InvalidCredentials)?;
        if matches!(completion,Completion::Authenticated{principal_id,auth_epoch,credential_version,recovery_version}if principal_id==id&&auth_epoch==account.principal.auth_epoch&&credential_version==account.credential_version&&recovery_version==account.recovery_version)
        {
            Ok(())
        } else {
            Err(BackendError::InvalidCredentials)
        }
    }
    pub fn initialize(&mut self, options: Initialize, password: Password) -> Result<RecoveryCode> {
        let state = self.state()?;
        if state.installation.is_some() {
            return Err(BackendError::Identity(IdentityRepositoryError::Domain(
                IdentityError::AlreadyInitialized,
            )));
        }
        let id = PrincipalId::new(1).map_err(|_| BackendError::Overflow)?;
        let request = self.request(
            &state,
            IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).map_err(|_| BackendError::Overflow)?,
                school_name: options.school_name,
                id,
                login_name: options.login_name,
                display_name: options.display_name,
            },
        )?;
        self.validate_identity(&state, &request, None)?;
        let (change, code) = prepare_initial_credentials(&mut self.crypto, &password, request)
            .map_err(|_| BackendError::CryptoUnavailable)?;
        let transition = self.repository.execute_credentials(
            self.encode(change, id, 1, 1)?,
            None,
            self.clock.as_ref(),
        )?;
        self.apply(&transition)?;
        Ok(code)
    }
    pub fn login(&mut self, login: &str, password: Password) -> Result<SessionLease> {
        self.login_origin(login, password, TrustedOrigin::Local)
    }
    pub fn login_origin(
        &mut self,
        login: &str,
        password: Password,
        origin: TrustedOrigin,
    ) -> Result<SessionLease> {
        let state = self.state()?;
        let id = state
            .account_by_login(login)
            .map_err(|_| BackendError::InvalidCredentials)?
            .principal
            .id;
        let generation = self.control.generation();
        let admission = self.admission(id, CredentialKind::Password)?;
        let record = match self.codec.decode_password(
            id,
            admission.credential_version,
            &admission.protected_record,
        ) {
            Ok(record) => record,
            Err(_) => {
                let _ = self.finish(admission.reservation, Verification::CorruptRecord);
                return Err(BackendError::CorruptCredential);
            }
        };
        let verified = self
            .crypto
            .verify_password(&password, &record)
            .map_err(|_| BackendError::CryptoUnavailable)?;
        let completion = self.finish(
            admission.reservation,
            if verified {
                Verification::Verified
            } else {
                Verification::Rejected
            },
        )?;
        let state = self.state()?;
        self.authenticated(completion, &state, id)?;
        if origin == TrustedOrigin::Local && self.control.generation() != generation {
            return Err(BackendError::Session(SessionError::Revoked));
        }
        let expires = next_day(self.clock.now())?;
        let mut ledger = self.ledger.lock().map_err(|_| BackendError::Closed)?;
        ledger.synchronize(
            &self.control,
            &state,
            self.clock.now(),
            self.monotonic.monotonic_millis(),
        );
        if origin == TrustedOrigin::Local {
            ledger.sessions.lock_local();
        }
        let issued_tick = self.monotonic.monotonic_millis();
        let lease = ledger.sessions.issue_verified(
            state
                .account(id)
                .map_err(|_| BackendError::InvalidCredentials)?,
            origin,
            issued_tick,
            SESSION_MILLIS,
            expires,
        )?;
        if origin == TrustedOrigin::Local {
            self.control.install_input(lease, generation, issued_tick)?;
            ledger.local = Some(lease);
            self.control
                .0
                .active_principal
                .store(id.get(), Ordering::Release);
            self.control
                .0
                .active_local
                .store(lease.id(), Ordering::Release);
            if self.control.generation() != generation {
                self.control
                    .0
                    .active_local
                    .compare_exchange(lease.id(), 0, Ordering::AcqRel, Ordering::Acquire)
                    .ok();
                ledger.sessions.lock_local();
                return Err(BackendError::Session(SessionError::Revoked));
            }
        }
        Ok(lease)
    }
    pub fn identity(
        &mut self,
        lease: SessionLease,
        command: IdentityCommand,
    ) -> Result<IdentityState> {
        let revision = self.state()?.revision;
        self.identity_at(lease, revision, command)
    }
    pub fn identity_at(
        &mut self,
        lease: SessionLease,
        expected_revision: Revision,
        command: IdentityCommand,
    ) -> Result<IdentityState> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let mut request = self.request(&state, command)?;
        request.expected_revision = expected_revision;
        let mut repository = self.guarded(lease, generation, SessionPurpose::Manage);
        let transition = fc_application::identity::execute_identity(
            &mut repository,
            self.clock.as_ref(),
            Some(&actor),
            request,
        )?;
        self.apply(&transition)?;
        Ok(transition.next)
    }
    pub fn create_account(
        &mut self,
        lease: SessionLease,
        account: NewAccount,
        password: Password,
    ) -> Result<RecoveryCode> {
        let revision = self.state()?.revision;
        self.create_account_at(lease, revision, account, password)
    }
    pub fn create_account_at(
        &mut self,
        lease: SessionLease,
        expected_revision: Revision,
        account: NewAccount,
        password: Password,
    ) -> Result<RecoveryCode> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let id = account.id;
        let mut request = self.request(&state, IdentityCommand::CreateAccount(account))?;
        request.expected_revision = expected_revision;
        self.validate_identity(&state, &request, Some(&actor))?;
        let (change, code) = prepare_initial_credentials(&mut self.crypto, &password, request)
            .map_err(|_| BackendError::CryptoUnavailable)?;
        let mut repository = self.guarded(lease, generation, SessionPurpose::Manage);
        let transition = repository.execute_credentials(
            self.encode(change, id, 1, 1)?,
            Some(&actor),
            self.clock.as_ref(),
        )?;
        self.apply(&transition)?;
        Ok(code)
    }
    pub fn change_password(
        &mut self,
        lease: SessionLease,
        old: Password,
        new: Password,
    ) -> Result<()> {
        let state = self.state()?;
        let (actor, generation) =
            self.actor(&state, lease, SessionPurpose::ChangeInitialPassword)?;
        let id = lease.principal_id();
        let admission = self.admission(id, CredentialKind::Password)?;
        let record = match self.codec.decode_password(
            id,
            admission.credential_version,
            &admission.protected_record,
        ) {
            Ok(record) => record,
            Err(_) => {
                let _ = self.finish(admission.reservation, Verification::CorruptRecord);
                return Err(BackendError::CorruptCredential);
            }
        };
        let request = self.request(
            &state,
            IdentityCommand::PasswordChanged {
                id,
                expected_credential_version: admission.credential_version,
            },
        )?;
        let change = match prepare_password_change(&mut self.crypto, &old, &new, &record, request) {
            Ok(change) => change,
            Err(fc_ports::CredentialError::VerificationFailed) => {
                self.finish(admission.reservation, Verification::Rejected)?;
                return Err(BackendError::InvalidCredentials);
            }
            Err(_) => return Err(BackendError::CryptoUnavailable),
        };
        let completion = self.finish(admission.reservation, Verification::Verified)?;
        self.authenticated(completion, &self.state()?, id)?;
        let mut repository = self.guarded(lease, generation, SessionPurpose::ChangeInitialPassword);
        let transition = repository.execute_credentials(
            self.encode(
                change,
                id,
                admission
                    .credential_version
                    .checked_add(1)
                    .ok_or(BackendError::Overflow)?,
                admission.recovery_version,
            )?,
            Some(&actor),
            self.clock.as_ref(),
        )?;
        self.apply(&transition)?;
        Ok(())
    }
    pub fn rotate_recovery(
        &mut self,
        lease: SessionLease,
        password: Password,
    ) -> Result<RecoveryCode> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let id = lease.principal_id();
        let admission = self.admission(id, CredentialKind::Password)?;
        let record = match self.codec.decode_password(
            id,
            admission.credential_version,
            &admission.protected_record,
        ) {
            Ok(record) => record,
            Err(_) => {
                let _ = self.finish(admission.reservation, Verification::CorruptRecord);
                return Err(BackendError::CorruptCredential);
            }
        };
        let request = self.request(
            &state,
            IdentityCommand::RecoveryRotated {
                id,
                expected_recovery_version: admission.recovery_version,
            },
        )?;
        let (change, code) =
            match prepare_recovery_rotation(&mut self.crypto, &password, &record, request) {
                Ok(prepared) => prepared,
                Err(fc_ports::CredentialError::VerificationFailed) => {
                    self.finish(admission.reservation, Verification::Rejected)?;
                    return Err(BackendError::InvalidCredentials);
                }
                Err(_) => return Err(BackendError::CryptoUnavailable),
            };
        let completion = self.finish(admission.reservation, Verification::Verified)?;
        self.authenticated(completion, &self.state()?, id)?;
        let mut repository = self.guarded(lease, generation, SessionPurpose::Manage);
        let transition = repository.execute_credentials(
            self.encode(
                change,
                id,
                admission.credential_version,
                admission
                    .recovery_version
                    .checked_add(1)
                    .ok_or(BackendError::Overflow)?,
            )?,
            Some(&actor),
            self.clock.as_ref(),
        )?;
        self.apply(&transition)?;
        Ok(code)
    }
    pub fn recover(
        &mut self,
        login: &str,
        code: RecoveryCode,
        new: Password,
    ) -> Result<RecoveryCode> {
        let state = self.state()?;
        let id = state
            .account_by_login(login)
            .map_err(|_| BackendError::InvalidCredentials)?
            .principal
            .id;
        let admission = self.admission(id, CredentialKind::Recovery)?;
        let record = match self.codec.decode_recovery(
            id,
            admission.recovery_version,
            &admission.protected_record,
        ) {
            Ok(record) => record,
            Err(_) => {
                let _ = self.finish(admission.reservation, Verification::CorruptRecord);
                return Err(BackendError::CorruptCredential);
            }
        };
        let request = self.request(
            &state,
            IdentityCommand::RecoveryConsumed {
                id,
                expected_recovery_version: admission.recovery_version,
            },
        )?;
        let (change, next) = match prepare_recovery(&mut self.crypto, &code, &new, &record, request)
        {
            Ok(change) => change,
            Err(fc_ports::CredentialError::VerificationFailed) => {
                self.finish(admission.reservation, Verification::Rejected)?;
                return Err(BackendError::InvalidCredentials);
            }
            Err(_) => return Err(BackendError::CryptoUnavailable),
        };
        let completion = self.finish(admission.reservation, Verification::Verified)?;
        self.authenticated(completion, &self.state()?, id)?;
        let transition = self.repository.execute_credentials(
            self.encode(
                change,
                id,
                admission
                    .credential_version
                    .checked_add(1)
                    .ok_or(BackendError::Overflow)?,
                admission
                    .recovery_version
                    .checked_add(1)
                    .ok_or(BackendError::Overflow)?,
            )?,
            None,
            self.clock.as_ref(),
        )?;
        self.apply(&transition)?;
        Ok(next)
    }
    pub fn read_display(&self, lease: SessionLease) -> Result<DisplaySettings> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .display(&actor)?)
    }
    /// Publish the UI's captured independent display revision, preserving stale drafts.
    pub fn save_display(
        &mut self,
        lease: SessionLease,
        expected_revision: Revision,
        settings: DisplaySettings,
    ) -> Result<DisplaySettings> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let request = DisplayRequest {
            command_id: self.command_id()?,
            expected_revision,
            candidate: settings,
        };
        let mut repository = self.guarded(lease, generation, SessionPurpose::Manage);
        Ok(repository.execute_display(request, &actor, self.clock.as_ref())?)
    }
    /// Trusted local presentation read. No management lease is fabricated or renewed.
    pub fn desktop_facts(&self, homework_offset: usize) -> Result<DesktopFacts> {
        self.desktop_facts_with_material_cursor(homework_offset, None)
    }
    pub fn desktop_facts_with_material_cursor(
        &self,
        homework_offset: usize,
        cursor: Option<MaterialCursor>,
    ) -> Result<DesktopFacts> {
        self.check_open()?;
        let offset = u64::try_from(homework_offset).map_err(|_| BackendError::InvalidInput)?;
        let facts = self.repository.desktop_facts_with_material_cursor(
            offset,
            cursor,
            self.clock.as_ref(),
        )?;
        self.check_open()?;
        if self.control.0.health.status().revision != facts.global_revision {
            return Err(BackendError::Display(DisplayRepositoryError::Unavailable));
        }
        Ok(facts)
    }
    pub fn materials(
        &self,
        lease: SessionLease,
        request: MaterialListRequest,
    ) -> Result<MaterialPage> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .materials(request, &actor, self.clock.as_ref())?)
    }
    pub fn append_material(
        &mut self,
        lease: SessionLease,
        prepared: PreparedMaterialAppend,
    ) -> Result<MaterialRecord> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .append_material(prepared, &actor, self.clock.as_ref())?)
    }
    pub fn delete_material(
        &mut self,
        lease: SessionLease,
        request: MaterialDeleteRequest,
    ) -> Result<MaterialRecord> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .delete_material(request, &actor, self.clock.as_ref())?)
    }
    pub fn resource_inventory(
        &self,
        after: Option<ResourceVersionId>,
    ) -> Result<ResourceInventoryPage> {
        self.check_open()?;
        let result = self.repository.resource_inventory(after)?;
        self.check_open()?;
        Ok(result)
    }
    pub fn retire_resource(
        &self,
        id: ResourceVersionId,
        revision: Revision,
    ) -> Result<ResourceRetirement> {
        self.check_open()?;
        Ok(self.repository.retire_resource(id, revision)?)
    }
    pub fn finalize_resource_removal(&self, ticket: ResourceRetirement) -> Result<()> {
        self.check_open()?;
        Ok(self.repository.finalize_resource_removal(ticket)?)
    }
    /// Shared launch copies may contain edits by different teachers. Only the
    /// local global administrator can inspect or request their expired cleanup.
    pub fn launch_copy_guard(&self, lease: SessionLease) -> Result<MaterialOpenGuard> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        state
            .authority(actor.principal_id)
            .and_then(|a| {
                a.authorize_global_local(&actor, self.clock.now())
                    .map_err(IdentityError::Permission)
            })
            .map_err(|_| BackendError::InvalidCredentials)?;
        let status = self
            .control
            .health_gate()
            .check_sensitive()
            .map_err(|_| BackendError::Closed)?;
        let ledger = self.ledger.clone();
        let control = self.control.clone();
        let clock = self.clock.clone();
        let monotonic = self.monotonic.clone();
        let expires_at = Instant::now() + Duration::from_secs(2);
        let check = Arc::new(move || {
            let current = control.health_gate().status();
            if current.health != fc_runtime::health::Health::Healthy
                || current.revision != status.revision
                || current.health_epoch != status.health_epoch
                || current.cancellation_generation != status.cancellation_generation
                || control.check(lease, generation).is_err()
            {
                return false;
            }
            let Ok(mut ledger) = ledger.lock() else {
                return false;
            };
            ledger.synchronize(&control, &state, clock.now(), monotonic.monotonic_millis());
            ledger
                .sessions
                .actor(
                    &state,
                    lease,
                    clock.now(),
                    monotonic.monotonic_millis(),
                    SessionPurpose::Manage,
                )
                .is_ok()
        });
        Ok(MaterialOpenGuard { check, expires_at })
    }
    pub fn material_for_owner(
        &self,
        lease: SessionLease,
        scope: MaterialScope,
        id: MaterialId,
        version: ResourceVersionId,
    ) -> Result<MaterialOpenAuthorization> {
        if lease.origin() != TrustedOrigin::Local {
            return Err(BackendError::Material(MaterialRepositoryError::Domain(
                MaterialError::Permission(PermissionError::LocalOnly),
            )));
        }
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let record = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .managed_material(scope, id, version, &actor)?;
        let status = self
            .control
            .health_gate()
            .check_sensitive()
            .map_err(|_| BackendError::Closed)?;
        let ledger = self.ledger.clone();
        let control = self.control.clone();
        let clock = self.clock.clone();
        let monotonic = self.monotonic.clone();
        let expires_at = Instant::now() + Duration::from_secs(2);
        let check = Arc::new(move || {
            let current = control.health_gate().status();
            if current.health != fc_runtime::health::Health::Healthy
                || current.revision != status.revision
                || current.health_epoch != status.health_epoch
                || current.cancellation_generation != status.cancellation_generation
                || control.check(lease, generation).is_err()
            {
                return false;
            }
            let Ok(mut ledger) = ledger.lock() else {
                return false;
            };
            ledger.synchronize(&control, &state, clock.now(), monotonic.monotonic_millis());
            ledger
                .sessions
                .actor(
                    &state,
                    lease,
                    clock.now(),
                    monotonic.monotonic_millis(),
                    SessionPurpose::Manage,
                )
                .is_ok()
        });
        Ok(MaterialOpenAuthorization {
            record,
            guard: MaterialOpenGuard { check, expires_at },
        })
    }
    pub fn material_for_scene(
        &self,
        source: &MaterialSource,
        generation: SceneGeneration,
        id: MaterialId,
        version: ResourceVersionId,
    ) -> Result<MaterialOpenAuthorization> {
        self.check_open()?;
        let record = self.repository.published_material(
            source,
            generation,
            id,
            version,
            self.clock.as_ref(),
        )?;
        let facts = self.desktop_facts(0)?;
        if facts.materials_source.as_ref() != Some(source) {
            return Err(BackendError::Material(MaterialRepositoryError::Domain(
                MaterialError::InvalidContext,
            )));
        }
        let status = self
            .control
            .health_gate()
            .check_sensitive()
            .map_err(|_| BackendError::Closed)?;
        if status.revision != facts.global_revision {
            return Err(BackendError::Material(MaterialRepositoryError::Unavailable));
        }
        let control = self.control.clone();
        let clock = self.clock.clone();
        let source = source.clone();
        let authorized_at = clock.now();
        // Facts already contain the writer's durable actual-teacher resolution.
        // Never resolve a temporary lesson back through today's base timetable.
        // Its time window starts at the captured trusted clock, so crossing a
        // boundary between the read and this dispatch cannot extend the grant.
        if authorized_at < facts.captured_at
            || facts
                .lesson_context
                .material_source(
                    &facts.identity,
                    &facts.schedule,
                    source.lesson.class_id,
                    authorized_at,
                )
                .ok()
                .flatten()
                != Some(source.clone())
        {
            return Err(BackendError::Material(MaterialRepositoryError::Domain(
                MaterialError::InvalidContext,
            )));
        }
        let mut context_until = LocalDateTime::new(next_day(facts.captured_at)?.date, 0)
            .map_err(|_| BackendError::Overflow)?;
        if let Ok(fc_domain::schedule::AcademicDay::Scheduled(day)) = facts
            .schedule
            .resolve_day(source.lesson.class_id, facts.captured_at.date)
        {
            for row in &day.table.rows {
                if row.enabled_days[day.source_date.weekday().index()] {
                    for minute in [row.range.begin, row.range.end] {
                        let second = u32::from(minute.get()) * 60;
                        if second > facts.captured_at.second_of_day() {
                            let boundary = LocalDateTime::new(facts.captured_at.date, second)
                                .map_err(|_| BackendError::InvalidInput)?;
                            context_until = context_until.min(boundary);
                        }
                    }
                }
            }
        }
        if authorized_at >= context_until {
            return Err(BackendError::Material(MaterialRepositoryError::Domain(
                MaterialError::InvalidContext,
            )));
        }
        let expires_at = Instant::now() + Duration::from_secs(2);
        let check = Arc::new(move || {
            let current = control.health_gate().status();
            let now = clock.now();
            current.health == fc_runtime::health::Health::Healthy
                && current.revision == status.revision
                && current.health_epoch == status.health_epoch
                && current.cancellation_generation == status.cancellation_generation
                && now >= authorized_at
                && now < context_until
                && control.material_open_guard(&source, generation, status.revision)
        });
        Ok(MaterialOpenAuthorization {
            record,
            guard: MaterialOpenGuard { check, expires_at },
        })
    }
    pub fn read_schedule(&self, lease: SessionLease) -> Result<ScheduleDocument> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .schedule(&actor)?)
    }
    /// Load a trusted captured lesson context on the worker. This operation never
    /// fabricates an actor for an unsigned-in desktop entry.
    pub fn read_lesson(&self, lease: SessionLease) -> Result<LessonContextSnapshot> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .lesson_context(&actor, self.clock.as_ref())?)
    }
    pub fn change_lesson(
        &mut self,
        lease: SessionLease,
        expected_revision: Revision,
        lesson: fc_domain::schedule::LessonInstanceKey,
        action: LessonAction,
    ) -> Result<LessonContextSnapshot> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let request = LessonRequest {
            command_id: self.command_id()?,
            expected_revision,
            lesson,
            action,
        };
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .execute_lesson(request, &actor, self.clock.as_ref())?)
    }
    /// The revision comes from the UI's original complete draft, never a new
    /// identity/global revision sampled during save. Failed CAS preserves it.
    pub fn save_schedule(
        &mut self,
        lease: SessionLease,
        expected_revision: Revision,
        document: ScheduleDocument,
    ) -> Result<ScheduleDocument> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let request = ScheduleRequest {
            command_id: self.command_id()?,
            expected_revision,
            candidate: document,
        };
        let mut repository = self.guarded(lease, generation, SessionPurpose::Manage);
        Ok(repository.execute_schedule(request, &actor, self.clock.as_ref())?)
    }
    pub fn read_content(
        &self,
        lease: SessionLease,
        key: ContentKey,
    ) -> Result<Option<ContentRecord>> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .content(key, &actor)?)
    }
    pub fn content(
        &mut self,
        lease: SessionLease,
        command: ContentCommand,
    ) -> Result<ContentRecord> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let mut repository = self.guarded(lease, generation, SessionPurpose::Manage);
        Ok(fc_application::execute_content(
            &mut repository,
            self.clock.as_ref(),
            &actor,
            command,
        )?)
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.control.0.health.stop();
        self.control.0.closed.store(true, Ordering::Release);
        self.control.lock_local();
        if let Some(writer) = &self.writer {
            writer.handle().request_stop();
        }
    }
}
fn next_day(now: LocalDateTime) -> Result<LocalDateTime> {
    let d = now.date;
    let next = LocalDate::new(d.year(), d.month(), d.day() + 1)
        .or_else(|_| {
            if d.month() < 12 {
                LocalDate::new(d.year(), d.month() + 1, 1)
            } else {
                LocalDate::new(d.year() + 1, 1, 1)
            }
        })
        .map_err(|_| BackendError::Overflow)?;
    LocalDateTime::new(next, now.second_of_day()).map_err(|_| BackendError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct TestClock {
        tick: AtomicU64,
        wall: LocalDateTime,
    }
    impl TestClock {
        fn new() -> Self {
            Self {
                tick: AtomicU64::new(0),
                wall: LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 3600).unwrap(),
            }
        }
    }
    impl Clock for TestClock {
        fn now(&self) -> LocalDateTime {
            self.wall
        }
    }
    impl MonotonicClock for TestClock {
        fn monotonic_millis(&self) -> u64 {
            self.tick.load(Ordering::Acquire)
        }
    }
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "FutureClassroom-Backend-{}-{}",
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
    struct Joined(Backend);
    impl Drop for Joined {
        fn drop(&mut self) {
            let result = self.0.close();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
    fn password(value: &str) -> Password {
        Password::new(value.into()).unwrap()
    }
    fn open(directory: &Directory, clock: Arc<TestClock>) -> Joined {
        Joined(
            Backend::open(
                directory.database(),
                CredentialRecordCodec::from_key([27u8; 32]).unwrap(),
                clock.clone(),
                clock,
            )
            .unwrap(),
        )
    }
    #[test]
    fn real_backend_accounts_catalog_homework_recovery_and_exact_local_input_survive_reopen() {
        let directory = Directory::new();
        let clock = Arc::new(TestClock::new());
        let mut service = open(&directory, clock.clone());
        assert!(service.0.state().unwrap().installation.is_none());
        let first_code = service
            .0
            .initialize(
                Initialize {
                    school_name: "Synthetic Backend School".into(),
                    login_name: "admin".into(),
                    display_name: "Administrator".into(),
                },
                password("Backend-initial-admin-2026"),
            )
            .unwrap();
        let admin = service
            .0
            .login("admin", password("Backend-initial-admin-2026"))
            .unwrap();
        assert_eq!(service.0.role(admin).unwrap(), Role::GlobalAdmin);
        let stale = service.0.state().unwrap().revision;
        service
            .0
            .identity(
                admin,
                IdentityCommand::CreateClass {
                    id: ClassId::new(1).unwrap(),
                    name: "Synthetic Class".into(),
                },
            )
            .unwrap();
        assert!(matches!(
            service.0.identity_at(
                admin,
                stale,
                IdentityCommand::RenameSchool {
                    name: "stale must not overwrite".into()
                }
            ),
            Err(BackendError::Identity(IdentityRepositoryError::Domain(
                IdentityError::RevisionConflict
            )))
        ));
        service
            .0
            .identity(
                admin,
                IdentityCommand::BindCurrentClass {
                    id: ClassId::new(1).unwrap(),
                },
            )
            .unwrap();
        service
            .0
            .identity(
                admin,
                IdentityCommand::PutSubject {
                    id: SubjectId::new(1).unwrap(),
                    name: "Math".into(),
                    enabled: true,
                    color: Rgb {
                        red: 40,
                        green: 80,
                        blue: 160,
                    },
                    expected_revision: None,
                },
            )
            .unwrap();
        let teacher = PrincipalId::new(2).unwrap();
        let revision = service.0.state().unwrap().revision;
        let teacher_recovery = service
            .0
            .create_account_at(
                admin,
                revision,
                NewAccount {
                    id: teacher,
                    login_name: "teacher".into(),
                    display_name: "Synthetic Teacher".into(),
                    role: Role::TeacherAdmin,
                    scopes: vec![AssignmentScope {
                        class_id: ClassId::new(1).unwrap(),
                        subject_id: SubjectId::new(1).unwrap(),
                    }],
                },
                password("Backend-teacher-initial-2026"),
            )
            .unwrap();
        drop(teacher_recovery);
        let initial = service
            .0
            .login("teacher", password("Backend-teacher-initial-2026"))
            .unwrap();
        assert_eq!(
            service.0.check_session(initial, SessionPurpose::Manage),
            Err(BackendError::Session(SessionError::PasswordChangeRequired))
        );
        service
            .0
            .change_password(
                initial,
                password("Backend-teacher-initial-2026"),
                password("Backend-teacher-changed-2026"),
            )
            .unwrap();
        assert!(service
            .0
            .check_session(initial, SessionPurpose::ChangeInitialPassword)
            .is_err());
        let lease = service
            .0
            .login("teacher", password("Backend-teacher-changed-2026"))
            .unwrap();
        let key = ContentKey::Homework(HomeworkKey {
            class_id: ClassId::new(1).unwrap(),
            actual_date: clock.wall.date,
            subject_id: SubjectId::new(1).unwrap(),
            owner_teacher_id: teacher,
        });
        let command = ContentCommand {
            command_id: CommandId::new(7001).unwrap(),
            key: key.clone(),
            expected_revision: Revision::default(),
            text: "Synthetic persistent assignment".into(),
        };
        let record = service.0.content(lease, command.clone()).unwrap();
        assert_eq!(
            service.0.read_content(lease, key.clone()).unwrap(),
            Some(record.clone())
        );
        let identity_revision = service.0.state().unwrap().revision;
        let global = service.0.global_revision().unwrap();
        assert!(global > identity_revision);
        service.0.close().unwrap();
        drop(service);
        let facts = startup_state(directory.database()).unwrap();
        assert_eq!(facts.revision, global);
        assert_eq!(facts.identity.revision, identity_revision);
        let mut service = open(&directory, clock.clone());
        assert_eq!(service.0.global_revision().unwrap(), global);
        let lease = service
            .0
            .login("teacher", password("Backend-teacher-changed-2026"))
            .unwrap();
        assert_eq!(service.0.read_content(lease, key).unwrap(), Some(record));
        let control = service.0.session_control();
        // Twenty minutes of continuous real input are coalesced without any worker
        // drain; neither a finite mailbox nor an old ledger timestamp drops activity.
        for minute in 1..=20 {
            clock.tick.store(minute * 60_000, Ordering::Release);
            assert!(control.note_input(lease, minute * 60_000));
        }
        service
            .0
            .check_session(lease, SessionPurpose::Manage)
            .unwrap();
        clock.tick.fetch_add(300_000, Ordering::AcqRel);
        assert!(service
            .0
            .check_session(lease, SessionPurpose::Manage)
            .is_err());
        assert!(
            !control.note_input(lease, clock.monotonic_millis()),
            "a true idle gap cannot revive the old lease"
        );
        let new_lease = service
            .0
            .login("teacher", password("Backend-teacher-changed-2026"))
            .unwrap();
        assert!(
            !control.note_input(lease, clock.monotonic_millis()),
            "queued old-lease input cannot touch a replacement lease"
        );
        let web = service
            .0
            .login_origin(
                "teacher",
                password("Backend-teacher-changed-2026"),
                TrustedOrigin::Web,
            )
            .unwrap();
        {
            let _ledger = service.0.ledger.lock().unwrap();
            let immediate = control.clone();
            std::thread::spawn(move || immediate.lock_local())
                .join()
                .unwrap();
            assert!(control.is_local_locked());
        }
        assert!(service
            .0
            .check_session(new_lease, SessionPurpose::Manage)
            .is_err());
        service
            .0
            .check_session(web, SessionPurpose::Manage)
            .unwrap();
        let next_code = service
            .0
            .recover(
                "admin",
                first_code,
                password("Backend-admin-recovered-2026"),
            )
            .unwrap();
        assert!(service
            .0
            .check_session(web, SessionPurpose::Manage)
            .is_err());
        let admin = service
            .0
            .login("admin", password("Backend-admin-recovered-2026"))
            .unwrap();
        let rotated = service
            .0
            .rotate_recovery(admin, password("Backend-admin-recovered-2026"))
            .unwrap();
        assert!(service
            .0
            .check_session(admin, SessionPurpose::Manage)
            .is_err());
        assert_eq!(
            service
                .0
                .recover("admin", next_code, password("Backend-admin-final-2026"))
                .unwrap_err(),
            BackendError::InvalidCredentials
        );
        drop(rotated);
        service.0.close().unwrap();
        assert_eq!(service.0.state(), Err(BackendError::Closed));
    }
    #[test]
    fn session_control_faults_actual_authority_while_backend_mutex_is_unavailable() {
        use fc_runtime::health::{GateError, Health};
        let directory = Directory::new();
        let clock = Arc::new(TestClock::new());
        let service = open(&directory, clock);
        let control = service.0.session_control();
        let health = control.0.health.clone();
        let cancelled = health.cancellation();
        let shared = Arc::new(Mutex::new(service));
        let mut locked = shared.lock().unwrap();
        let immediate = control.clone();
        std::thread::spawn(move || immediate.fault())
            .join()
            .unwrap();
        assert_eq!(health.status().health, Health::Faulted);
        assert!(cancelled.is_cancelled());
        assert_eq!(health.check_sensitive(), Err(GateError::Faulted));
        assert_eq!(locked.0.state(), Err(BackendError::Closed));
        locked.0.close().unwrap();
        assert_eq!(
            health.status().health,
            Health::Faulted,
            "normal stop never revives a faulted gate"
        );
    }
    /// Policy fixture records deliberately bypass cryptography; these tests do
    /// no password authentication. The full backend lifecycle test covers KDFs.
    struct PolicyCrypto;
    impl fc_ports::CredentialCrypto for PolicyCrypto {
        type Secret = ();
        type PasswordRecord = Vec<u8>;
        type RecoveryCode = ();
        type RecoveryRecord = Vec<u8>;
        fn password_record(
            &mut self,
            _: &(),
        ) -> std::result::Result<Vec<u8>, fc_ports::CredentialError> {
            Ok(vec![1])
        }
        fn verify_password(
            &mut self,
            _: &(),
            _: &Vec<u8>,
        ) -> std::result::Result<bool, fc_ports::CredentialError> {
            Ok(true)
        }
        fn recovery_record(
            &mut self,
        ) -> std::result::Result<((), Vec<u8>), fc_ports::CredentialError> {
            Ok(((), vec![2]))
        }
        fn verify_recovery(
            &mut self,
            _: &(),
            _: &Vec<u8>,
        ) -> std::result::Result<bool, fc_ports::CredentialError> {
            Ok(true)
        }
    }
    fn policy_accounts(service: &mut Backend) {
        use fc_ports::IdentityRepository;
        let mut repo = service
            .repository
            .clone()
            .with_session_guard(Arc::new(|_, _| Ok(())));
        let admin = PrincipalId::new(1).unwrap();
        let teacher = PrincipalId::new(2).unwrap();
        let request = IdentityRequest {
            command_id: CommandId::new(1).unwrap(),
            expected_revision: Revision::default(),
            command: IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "Exit policy fixture".into(),
                id: admin,
                login_name: "admin".into(),
                display_name: "Admin".into(),
            },
        };
        let (change, _) = prepare_initial_credentials(&mut PolicyCrypto, &(), request).unwrap();
        repo.execute_credentials(change, None, service.clock.as_ref())
            .unwrap();
        let admin_actor = ActorContext {
            principal_id: admin,
            school_id: SchoolId::new(1).unwrap(),
            auth_epoch: AuthEpoch::new(1),
            expires_at: next_day(service.clock.now()).unwrap(),
            origin: TrustedOrigin::Local,
        };
        for (id, command) in [
            (
                2,
                IdentityCommand::CreateClass {
                    id: ClassId::new(1).unwrap(),
                    name: "Class".into(),
                },
            ),
            (
                3,
                IdentityCommand::PutSubject {
                    id: SubjectId::new(1).unwrap(),
                    name: "Math".into(),
                    enabled: true,
                    color: Rgb {
                        red: 40,
                        green: 80,
                        blue: 160,
                    },
                    expected_revision: None,
                },
            ),
        ] {
            let state = repo.load_identity().unwrap();
            repo.execute_identity(
                IdentityRequest {
                    command_id: CommandId::new(id).unwrap(),
                    expected_revision: state.revision,
                    command,
                },
                Some(&admin_actor),
                service.clock.as_ref(),
            )
            .unwrap();
        }
        let state = repo.load_identity().unwrap();
        let request = IdentityRequest {
            command_id: CommandId::new(4).unwrap(),
            expected_revision: state.revision,
            command: IdentityCommand::CreateAccount(NewAccount {
                id: teacher,
                login_name: "teacher".into(),
                display_name: "Teacher".into(),
                role: Role::TeacherAdmin,
                scopes: vec![AssignmentScope {
                    class_id: ClassId::new(1).unwrap(),
                    subject_id: SubjectId::new(1).unwrap(),
                }],
            }),
        };
        let (change, _) = prepare_initial_credentials(&mut PolicyCrypto, &(), request).unwrap();
        repo.execute_credentials(change, Some(&admin_actor), service.clock.as_ref())
            .unwrap();
        let state = repo.load_identity().unwrap();
        let request = IdentityRequest {
            command_id: CommandId::new(5).unwrap(),
            expected_revision: state.revision,
            command: IdentityCommand::PasswordChanged {
                id: teacher,
                expected_credential_version: 1,
            },
        };
        let change =
            prepare_password_change(&mut PolicyCrypto, &(), &(), &vec![1], request).unwrap();
        let teacher_actor = ActorContext {
            principal_id: teacher,
            ..admin_actor
        };
        repo.execute_credentials(change, Some(&teacher_actor), service.clock.as_ref())
            .unwrap();
    }
    fn policy_lease(service: &Backend, id: u64, origin: TrustedOrigin) -> SessionLease {
        let state = service.state().unwrap();
        let generation = service.control.generation();
        let tick = service.monotonic.monotonic_millis();
        let mut ledger = service.ledger.lock().unwrap();
        ledger.synchronize(&service.control, &state, service.clock.now(), tick);
        if origin == TrustedOrigin::Local {
            ledger.sessions.lock_local();
        }
        let lease = ledger
            .sessions
            .issue_verified(
                state.account(PrincipalId::new(id).unwrap()).unwrap(),
                origin,
                tick,
                SESSION_MILLIS,
                next_day(service.clock.now()).unwrap(),
            )
            .unwrap();
        if origin == TrustedOrigin::Local {
            service
                .control
                .install_input(lease, generation, tick)
                .unwrap();
            service
                .control
                .0
                .active_principal
                .store(id, Ordering::Release);
            service
                .control
                .0
                .active_local
                .store(lease.id(), Ordering::Release);
            ledger.local = Some(lease);
        }
        lease
    }
    #[test]
    fn authorized_close_requires_fresh_local_admin_and_consumes_exact_permission() {
        let directory = Directory::new();
        let clock = Arc::new(TestClock::new());
        let mut service = open(&directory, clock.clone());
        policy_accounts(&mut service.0);
        let teacher = policy_lease(&service.0, 2, TrustedOrigin::Local);
        assert!(matches!(
            service.0.close_authorized(teacher),
            Err(BackendError::Identity(IdentityRepositoryError::Domain(
                IdentityError::Permission(PermissionError::RoleDenied)
            )))
        ));
        service.0.state().unwrap();
        let web = policy_lease(&service.0, 1, TrustedOrigin::Web);
        assert!(matches!(
            service.0.close_authorized(web),
            Err(BackendError::Identity(IdentityRepositoryError::Domain(
                IdentityError::Permission(PermissionError::LocalOnly)
            )))
        ));
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        clock.tick.store(300_000, Ordering::Release);
        assert_eq!(
            service.0.close_authorized(admin),
            Err(BackendError::Session(SessionError::Expired))
        );
        service.0.state().unwrap();
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        service.0.control.lock_local();
        assert_eq!(
            service.0.close_authorized(admin),
            Err(BackendError::Session(SessionError::Revoked))
        );
        service.0.state().unwrap();
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        assert_eq!(service.0.close_authorized(admin), Ok(()));
        assert_eq!(service.0.control.0.active_local.load(Ordering::Acquire), 0);
        assert_eq!(service.0.state(), Err(BackendError::Closed));
    }
    #[test]
    fn timetable_backend_uses_ui_revision_and_recovers_independent_schedule_state() {
        use fc_domain::{schedule::*, schedule_document::*};
        let directory = Directory::new();
        let clock = Arc::new(TestClock::new());
        let mut service = open(&directory, clock.clone());
        policy_accounts(&mut service.0);
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        assert_eq!(
            service.0.read_schedule(admin).unwrap(),
            ScheduleDocument::default()
        );
        let row = TimetableRow {
            id: RowId::new(1).unwrap(),
            logical_key: LogicalRowKey::new(1).unwrap(),
            group: DayGroup::Morning,
            kind: RowKind::Course,
            range: MinuteRange::new(
                Minute::from_hm(8, 20).unwrap(),
                Minute::from_hm(9, 0).unwrap(),
            )
            .unwrap(),
            enabled_days: [true, false, false, false, false, false, false],
            courses: std::array::from_fn(|day| {
                if day == 0 {
                    Some(CourseCell {
                        subject_id: SubjectId::new(1).unwrap(),
                        teacher: TeacherBinding::Explicit(PrincipalId::new(2).unwrap()),
                    })
                } else {
                    None
                }
            }),
        };
        let doc = ScheduleDocument {
            semesters: vec![SemesterPlan {
                semester: Semester {
                    id: SemesterId::new(1).unwrap(),
                    class_id: ClassId::new(1).unwrap(),
                    revision: Revision::default(),
                    enabled: true,
                    start: LocalDate::new(2026, 10, 1).unwrap(),
                    end: LocalDate::new(2026, 10, 31).unwrap(),
                    first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
                    tables: vec![WeekTable {
                        id: TableId::new(1).unwrap(),
                        role: TableRole::Single,
                        rows: vec![row],
                    }],
                },
                name: "Backend timetable".into(),
                timing_sync: false,
                dual_history: None,
                row_details: Vec::new(),
                cell_details: Vec::new(),
            }],
            ..ScheduleDocument::default()
        };
        let saved = service
            .0
            .save_schedule(admin, Revision::default(), doc)
            .unwrap();
        assert_eq!(saved.revision, Revision::new(1));
        let identity_revision = service.0.state().unwrap().revision;
        let mut draft = saved.clone();
        draft.automatic_gaps = false;
        service
            .0
            .content(
                admin,
                ContentCommand {
                    command_id: CommandId::new(7600).unwrap(),
                    key: ContentKey::Homework(HomeworkKey {
                        class_id: ClassId::new(1).unwrap(),
                        actual_date: clock.wall.date,
                        subject_id: SubjectId::new(1).unwrap(),
                        owner_teacher_id: PrincipalId::new(2).unwrap(),
                    }),
                    expected_revision: Revision::default(),
                    text: "independent content revision".into(),
                },
            )
            .unwrap();
        let saved = service
            .0
            .save_schedule(admin, saved.revision, draft.clone())
            .unwrap();
        assert_eq!(saved.revision, Revision::new(2));
        assert_eq!(service.0.state().unwrap().revision, identity_revision);
        assert!(
            matches!(service.0.save_schedule(admin,Revision::new(1),draft),Err(BackendError::Schedule(ScheduleRepositoryError::Domain(e)))if matches!(e.kind,DocumentErrorKind::RevisionConflict{current}if current==Revision::new(2)))
        );
        let teacher = policy_lease(&service.0, 2, TrustedOrigin::Local);
        assert!(service.0.read_schedule(teacher).is_err());
        assert!(service
            .0
            .save_schedule(teacher, saved.revision, saved.clone())
            .is_err());
        let global = service.0.global_revision().unwrap();
        service.0.close().unwrap();
        drop(service);
        let mut restored = open(&directory, clock);
        assert_eq!(restored.0.global_revision().unwrap(), global);
        let admin = policy_lease(&restored.0, 1, TrustedOrigin::Local);
        assert_eq!(restored.0.read_schedule(admin).unwrap(), saved);
        restored.0.close().unwrap();
    }
    #[test]
    fn lesson_backend_requires_exact_local_admin_and_persists_override_restore_cas() {
        use fc_domain::{schedule::*, schedule_document::*};
        let directory = Directory::new();
        let clock = Arc::new(TestClock::new());
        let mut service = open(&directory, clock.clone());
        policy_accounts(&mut service.0);
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        service
            .0
            .identity(
                admin,
                IdentityCommand::BindCurrentClass {
                    id: ClassId::new(1).unwrap(),
                },
            )
            .unwrap();
        let row = TimetableRow {
            id: RowId::new(1).unwrap(),
            logical_key: LogicalRowKey::new(1).unwrap(),
            group: DayGroup::Morning,
            kind: RowKind::Course,
            range: MinuteRange::new(Minute::new(0).unwrap(), Minute::new(120).unwrap()).unwrap(),
            enabled_days: [true; 7],
            courses: std::array::from_fn(|_| {
                Some(CourseCell {
                    subject_id: SubjectId::new(1).unwrap(),
                    teacher: TeacherBinding::Explicit(PrincipalId::new(2).unwrap()),
                })
            }),
        };
        let doc = ScheduleDocument {
            semesters: vec![SemesterPlan::new(
                Semester {
                    id: SemesterId::new(1).unwrap(),
                    class_id: ClassId::new(1).unwrap(),
                    revision: Revision::default(),
                    enabled: true,
                    start: LocalDate::new(2026, 10, 1).unwrap(),
                    end: LocalDate::new(2026, 10, 31).unwrap(),
                    first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
                    tables: vec![WeekTable {
                        id: TableId::new(1).unwrap(),
                        role: TableRole::Single,
                        rows: vec![row],
                    }],
                },
                "Lesson backend".into(),
            )],
            ..ScheduleDocument::default()
        };
        service
            .0
            .save_schedule(admin, Revision::default(), doc)
            .unwrap();
        let before = service.0.read_lesson(admin).unwrap();
        let current = before.current.as_ref().unwrap();
        let key = current.key.clone();
        let target = current.base.clone();
        let replaced = service
            .0
            .change_lesson(
                admin,
                before.revision,
                key.clone(),
                LessonAction::Replace(target.clone()),
            )
            .unwrap();
        assert!(replaced.active_override.is_some());
        assert_eq!(replaced.current.as_ref().unwrap().effective, target);
        assert!(matches!(
            service
                .0
                .change_lesson(admin, before.revision, key.clone(), LessonAction::Restore),
            Err(BackendError::Lesson(LessonRepositoryError::Domain(
                fc_domain::lesson_context::LessonContextError::RevisionConflict { .. }
            )))
        ));
        service.0.close().unwrap();
        drop(service);
        let mut service = open(&directory, clock);
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        let loaded = service.0.read_lesson(admin).unwrap();
        assert_eq!(loaded.active_override, replaced.active_override);
        let restored = service
            .0
            .change_lesson(admin, loaded.revision, key.clone(), LessonAction::Restore)
            .unwrap();
        assert!(restored.active_override.is_none());
        let teacher = policy_lease(&service.0, 2, TrustedOrigin::Local);
        assert!(service.0.read_lesson(teacher).is_err());
        assert!(service
            .0
            .change_lesson(
                teacher,
                restored.revision,
                key.clone(),
                LessonAction::Replace(target.clone())
            )
            .is_err());
        let web = policy_lease(&service.0, 1, TrustedOrigin::Web);
        assert!(service.0.read_lesson(web).is_err());
        assert!(service
            .0
            .change_lesson(
                web,
                restored.revision,
                key.clone(),
                LessonAction::Replace(target.clone())
            )
            .is_err());
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        service.0.session_control().lock_local();
        assert!(matches!(
            service
                .0
                .change_lesson(admin, restored.revision, key, LessonAction::Replace(target)),
            Err(BackendError::Session(SessionError::Revoked))
        ));
    }
    #[test]
    fn display_backend_uses_captured_cas_and_public_facts_do_not_renew_management() {
        use fc_domain::desktop::{ClockMode, DisplayError};
        let directory = Directory::new();
        let clock = Arc::new(TestClock::new());
        let mut service = open(&directory, clock.clone());
        policy_accounts(&mut service.0);
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        service
            .0
            .identity(
                admin,
                IdentityCommand::BindCurrentClass {
                    id: ClassId::new(1).unwrap(),
                },
            )
            .unwrap();
        let mut draft = service.0.read_display(admin).unwrap();
        draft.clock_mode = ClockMode::Digital12;
        let saved = service
            .0
            .save_display(admin, Revision::default(), draft.clone())
            .unwrap();
        assert_eq!(saved.revision, Revision::new(1));
        assert!(
            matches!(service.0.save_display(admin,Revision::default(),draft),Err(BackendError::Display(DisplayRepositoryError::Domain(DisplayError::RevisionConflict{current})))if current==Revision::new(1))
        );
        service
            .0
            .content(
                admin,
                ContentCommand {
                    command_id: CommandId::new(7800).unwrap(),
                    key: ContentKey::Homework(HomeworkKey {
                        class_id: ClassId::new(1).unwrap(),
                        actual_date: clock.wall.date,
                        subject_id: SubjectId::new(1).unwrap(),
                        owner_teacher_id: PrincipalId::new(2).unwrap(),
                    }),
                    expected_revision: Revision::default(),
                    text: "public desktop homework".into(),
                },
            )
            .unwrap();
        clock.tick.store(299_999, Ordering::Release);
        let facts = service.0.desktop_facts(0).unwrap();
        assert_eq!(facts.display, saved);
        assert_eq!(facts.homework[0].text, "public desktop homework");
        assert_eq!(
            service.0.session_control().health_gate().status().revision,
            facts.global_revision
        );
        clock.tick.store(300_001, Ordering::Release);
        assert!(service
            .0
            .check_session(admin, SessionPurpose::Manage)
            .is_err());
        service.0.session_control().lock_local();
        assert_eq!(
            service.0.desktop_facts(0).unwrap().homework[0].text,
            "public desktop homework"
        );
        let teacher = policy_lease(&service.0, 2, TrustedOrigin::Local);
        assert!(service.0.read_display(teacher).is_err());
        assert!(service
            .0
            .save_display(teacher, saved.revision, saved.clone())
            .is_err());
        let global = service.0.global_revision().unwrap();
        service.0.close().unwrap();
        drop(service);
        let mut restored = open(&directory, clock);
        assert_eq!(restored.0.global_revision().unwrap(), global);
        assert_eq!(restored.0.desktop_facts(0).unwrap().display, saved);
        restored.0.session_control().fault();
        assert!(restored.0.desktop_facts(0).is_err());
        restored.0.close().unwrap();
    }
    #[test]
    fn material_backend_owner_and_guest_permissions_are_fresh_memory_guards() {
        struct Fixture;
        impl fc_ports::MaterialPreparation for Fixture {
            type Input = MaterialResource;
            fn prepare(
                &mut self,
                input: Self::Input,
            ) -> std::result::Result<Self::Input, MaterialRepositoryError> {
                Ok(input)
            }
        }
        let directory = Directory::new();
        let clock = Arc::new(TestClock {
            tick: AtomicU64::new(0),
            wall: LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 8 * 3600 + 30 * 60)
                .unwrap(),
        });
        let mut service = open(&directory, clock.clone());
        policy_accounts(&mut service.0);
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        service
            .0
            .identity(
                admin,
                IdentityCommand::BindCurrentClass {
                    id: ClassId::new(1).unwrap(),
                },
            )
            .unwrap();
        use fc_domain::{schedule::*, schedule_document::*};
        let doc = ScheduleDocument {
            semesters: vec![SemesterPlan {
                semester: Semester {
                    id: SemesterId::new(1).unwrap(),
                    class_id: ClassId::new(1).unwrap(),
                    revision: Revision::default(),
                    enabled: true,
                    start: LocalDate::new(2026, 10, 1).unwrap(),
                    end: LocalDate::new(2026, 10, 31).unwrap(),
                    first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
                    tables: vec![WeekTable {
                        id: TableId::new(1).unwrap(),
                        role: TableRole::Single,
                        rows: vec![TimetableRow {
                            id: RowId::new(1).unwrap(),
                            logical_key: LogicalRowKey::new(1).unwrap(),
                            group: DayGroup::Morning,
                            kind: RowKind::Course,
                            range: MinuteRange::new(
                                Minute::from_hm(8, 20).unwrap(),
                                Minute::from_hm(9, 0).unwrap(),
                            )
                            .unwrap(),
                            enabled_days: [true; 7],
                            courses: std::array::from_fn(|_| {
                                Some(CourseCell {
                                    subject_id: SubjectId::new(1).unwrap(),
                                    teacher: TeacherBinding::Explicit(PrincipalId::new(2).unwrap()),
                                })
                            }),
                        }],
                    }],
                },
                name: "Material guard fixture".into(),
                timing_sync: false,
                dual_history: None,
                row_details: Vec::new(),
                cell_details: Vec::new(),
            }],
            ..ScheduleDocument::default()
        };
        service
            .0
            .save_schedule(admin, Revision::default(), doc)
            .unwrap();
        let scope = MaterialScope {
            teacher_id: PrincipalId::new(2).unwrap(),
            subject_id: SubjectId::new(1).unwrap(),
        };
        let version = ResourceVersionId::new(7900).unwrap();
        let id = MaterialId::new(7900).unwrap();
        let prepared = fc_ports::prepare_material_append(
            &mut Fixture,
            MaterialResource {
                version_id: version,
                sha256: [8; 32],
                kind: MaterialKind::Txt,
                bytes: 10,
            },
            fc_ports::MaterialAppendMetadata {
                command_id: CommandId::new(7900).unwrap(),
                scope,
                material_id: id,
                display_name: "lesson.txt".into(),
                allowed_class_ids: vec![ClassId::new(1).unwrap()],
            },
        )
        .unwrap();
        service.0.append_material(admin, prepared).unwrap();
        let facts = service.0.desktop_facts(0).unwrap();
        let source = facts.materials_source.unwrap();
        let generation = SceneGeneration::new(7);
        assert!(service
            .0
            .material_for_scene(&source, generation, id, version)
            .is_err());
        assert!(service.0.control.publish_material_scene(
            &source,
            generation,
            facts.global_revision
        ));
        let guest = service
            .0
            .material_for_scene(&source, generation, id, version)
            .unwrap();
        assert!(guest.guard.is_valid());
        assert!(guest.guard.is_context_valid());
        let owner = service
            .0
            .material_for_owner(admin, scope, id, version)
            .unwrap();
        assert!(owner.guard.is_valid());
        let mut expired_dispatch = owner.guard.clone();
        expired_dispatch.expires_at = Instant::now() - Duration::from_secs(1);
        assert!(!expired_dispatch.is_valid());
        assert!(expired_dispatch.is_context_valid());
        service.0.control.clear_material_scene();
        assert!(!guest.guard.is_context_valid());
        assert!(owner.guard.is_context_valid());
        let web = policy_lease(&service.0, 2, TrustedOrigin::Web);
        assert!(service
            .0
            .material_for_owner(web, scope, id, version)
            .is_err());
        service.0.control.lock_local();
        assert!(!owner.guard.is_context_valid());
        let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
        let owner = service
            .0
            .material_for_owner(admin, scope, id, version)
            .unwrap();
        service
            .0
            .delete_material(
                admin,
                MaterialDeleteRequest {
                    command_id: CommandId::new(7901).unwrap(),
                    scope,
                    material_id: id,
                    expected_material_revision: Revision::new(1),
                },
            )
            .unwrap();
        assert!(!owner.guard.is_context_valid());
        assert!(service
            .0
            .material_for_owner(admin, scope, id, version)
            .is_err());
        service.0.close().unwrap();
    }
}
