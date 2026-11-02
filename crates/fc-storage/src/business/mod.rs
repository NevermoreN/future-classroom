//! Typed business repositories on the existing SQLite writer. The synchronous
//! facade is for application workers; native/HTTP event loops poll tickets.
pub(crate) mod autostart;
mod autostart_codec;
mod calendar;
pub use calendar::{ManualFetchPermit, ManualFetchStart};
mod calendar_codec;
pub(crate) mod codec;
pub(crate) mod command_window;
mod content_web;
#[cfg(test)]
#[path = "tests/content_web.rs"]
mod content_web_tests;
mod desktop;
mod desktop_lesson;
mod display;
mod display_codec;
mod https_settings;
mod lesson;
mod lesson_codec;
#[cfg(test)]
#[path = "tests/logout.rs"]
mod logout_tests;
pub mod materials;
mod materials_codec;
pub(crate) mod migration;
pub mod profile_content;
mod profile_content_codec;
mod profiles;
mod profiles_codec;
pub(crate) mod recovery;
mod schedule;
mod schedule_codec;
mod subject_visual;
pub(crate) mod typed_commands;
pub mod voice;
mod voice_codec;
use crate::*;
use fc_domain::{auth::*, content::*, identity::*, ids::*};
use fc_ports::{
    Clock, ContentRepository, CredentialIdentityRepository, IdentityRepository,
    IdentityRepositoryError, IdentityRequest, PreparedCredentialChange, RepositoryError,
};
use serde_json::{json, Value};

/// Startup-only recovery facts. The deployment must already own its exclusive
/// instance lock; this helper closes SQLite before the sole writer is created.
/// No healthy/fake authority is constructed to read persisted initialization facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupState {
    pub identity: IdentityState,
    pub revision: Revision,
}
pub fn startup_state(path: impl AsRef<Path>) -> Result<StartupState, StorageError> {
    let db = open_connection(path.as_ref())?;
    let transaction = db
        .unchecked_transaction()
        .map_err(|_| StorageError::Database)?;
    let revision = read_revision(&transaction)?;
    let identity = load_identity(&TransactionView(&transaction))?;
    validate_documents(&TransactionView(&transaction), &identity)?;
    transaction.rollback().map_err(|_| StorageError::Database)?;
    Ok(StartupState { identity, revision })
}
pub(crate) fn validate_documents(
    d: &dyn TransactionDocuments,
    identity: &IdentityState,
) -> Result<(), StorageError> {
    schedule::load(d, identity)?;
    autostart::startup_check(d)?;
    calendar::startup_check(d)?;
    schedule::change_proof(d)?;
    lesson::load(d)?;
    display::load(d, identity)?;
    https_settings::load(d, identity)?;
    profiles::startup_check(d, identity)?;
    desktop::read_watermark(d, identity)?;
    materials::startup_check(d, identity)?;
    voice::startup_check(d, identity)?;
    profile_content::startup_check(d, identity)?;
    recovery::startup_check(d)?;
    Ok(())
}

const IDENTITY: &str = "business/identity/v1";
/// Bind an exact authenticated channel lease in this trusted memory-only callback.
/// It must revalidate monotonic idle/expiry/local lock without refreshing the lease.
pub type SessionGuard =
    Arc<dyn Fn(&IdentityState, &ActorContext) -> Result<(), StorageError> + Send + Sync>;
/// Memory-only lifetime fence for preparing an intent and marking it Applying.
/// This callback grants no OS write permission; factual settlement remains allowed.
pub type AutostartExecutionGuard = Arc<dyn Fn() -> bool + Send + Sync>;
/// Memory-only worker ownership fence for calendar writes. It grants no actor
/// or durable policy authority; false/panic is an ordinary cancellation.
pub type CalendarExecutionGuard = Arc<dyn Fn() -> bool + Send + Sync>;
/// Bounded memory-only policy handoff fence for native driver transactions.
pub type VoiceDriverGuard = Arc<dyn Fn() -> bool + Send + Sync>;
/// Exact logical-Web-disabled configuration permit; default denied.
pub type HttpsConfigurationGuard = Arc<dyn Fn() -> bool + Send + Sync>;
/// A short memory-only native-flow capability. Never grants account authority.
/// False/panic cancels; no SQL, native waiting or new lease is allowed inside.
pub type DesktopLessonGuard = Arc<dyn Fn() -> bool + Send + Sync>;
type Factory<S> = Arc<dyn Fn(&IdentityState, Revision) -> Result<S, StorageError> + Send + Sync>;
pub type MaterialSceneGuard = Arc<
    dyn Fn(&fc_domain::materials::MaterialSource, SceneGeneration) -> Result<(), StorageError>
        + Send
        + Sync,
>;
/// Factory must only prepare an owned immutable authority snapshot: no writer
/// reentry, file access, KDF, channel effects, or externally visible publication.
pub struct BusinessRepository<S> {
    handle: StorageHandle<S>,
    clock: Arc<dyn Clock + Send + Sync>,
    factory: Factory<S>,
    session: SessionGuard,
    material_scene: MaterialSceneGuard,
    profile_content_scene: profile_content::ProfileContentSceneGuard,
    lesson_observations: lesson::Observations,
    desktop_lesson_guard: Option<DesktopLessonGuard>,
    calendar_execution: CalendarExecutionGuard,
    autostart_execution: AutostartExecutionGuard,
    voice_timeout: Option<(std::time::Duration, Arc<dyn Fn() -> bool + Send + Sync>)>,
    voice_driver_guard: VoiceDriverGuard,
    initial_installation_identity: Option<[u8; 32]>,
    https_configuration_guard: Option<HttpsConfigurationGuard>,
}
impl<S> Clone for BusinessRepository<S> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            clock: self.clock.clone(),
            factory: self.factory.clone(),
            session: self.session.clone(),
            material_scene: self.material_scene.clone(),
            profile_content_scene: self.profile_content_scene.clone(),
            lesson_observations: self.lesson_observations.clone(),
            desktop_lesson_guard: self.desktop_lesson_guard.clone(),
            calendar_execution: self.calendar_execution.clone(),
            autostart_execution: self.autostart_execution.clone(),
            voice_timeout: self.voice_timeout.clone(),
            voice_driver_guard: self.voice_driver_guard.clone(),
            initial_installation_identity: self.initial_installation_identity,
            https_configuration_guard: self.https_configuration_guard.clone(),
        }
    }
}
impl<S: Send + 'static> BusinessRepository<S> {
    /// Request-scoped lifetime binding. The base repository and all existing
    /// session/scene/policy guards remain intact; rebinding only tightens it.
    pub fn with_execution_fence(mut self, fence: TransactionExecutionFence) -> Self {
        self.handle = self.handle.with_transaction_execution_fence(fence);
        self
    }
    pub fn new(
        handle: StorageHandle<S>,
        clock: Arc<dyn Clock + Send + Sync>,
        factory: Factory<S>,
    ) -> Self {
        Self {
            handle,
            clock,
            factory,
            session: Arc::new(|_, _| Err(StorageError::AuthorityUnavailable)),
            material_scene: Arc::new(|_, _| Err(StorageError::AuthorityUnavailable)),
            profile_content_scene: Arc::new(|_, _, _, _| Err(StorageError::AuthorityUnavailable)),
            lesson_observations: Arc::new(std::sync::Mutex::new(None)),
            desktop_lesson_guard: None,
            calendar_execution: Arc::new(|| true),
            autostart_execution: Arc::new(|| true),
            voice_timeout: None,
            voice_driver_guard: Arc::new(|| true),
            initial_installation_identity: None,
            https_configuration_guard: None,
        }
    }
    /// Scoped native desktop capability. An unbound repository denies access;
    /// rebinding an existing capability can only tighten it.
    pub fn with_desktop_lesson_guard(mut self, guard: DesktopLessonGuard) -> Self {
        self.desktop_lesson_guard = Some(match self.desktop_lesson_guard.take() {
            Some(previous) => Arc::new(move || previous() && guard()),
            None => guard,
        });
        self
    }
    /// Trusted short metadata permit. Rebinding can only tighten; callbacks must
    /// not wait on native resources, SQL or another execution queue.
    pub fn with_https_configuration_guard(mut self, guard: HttpsConfigurationGuard) -> Self {
        self.https_configuration_guard = Some(match self.https_configuration_guard.take() {
            Some(previous) => Arc::new(move || previous() && guard()),
            None => guard,
        });
        self
    }
    /// Authenticated operations fail closed until composition binds their exact lease.
    pub fn with_session_guard(mut self, guard: SessionGuard) -> Self {
        self.session = match self.handle.command_window {
            Some(issued) => Arc::new(move |state, actor| {
                if fc_ports::command_window::CommandOwner::authenticated(actor) != issued.owner {
                    return Err(StorageError::CommandNotIssued);
                }
                guard(state, actor)
            }),
            None => guard,
        };
        self
    }
    pub fn with_voice_driver_guard(mut self, guard: VoiceDriverGuard) -> Self {
        self.voice_driver_guard = guard;
        self
    }
    /// Bind a CSPRNG installation identifier to the same initial credential
    /// transaction. This value is public identity, never an authentication key.
    pub fn with_initial_installation_identity(mut self, id: [u8; 32]) -> Self {
        self.initial_installation_identity = Some(id);
        self
    }
    /// Bind the shared-worker lifetime through intent/Applying writer COMMIT.
    /// Cancellation never grants permission or suppresses factual settlement.
    pub fn with_autostart_execution_guard(mut self, guard: AutostartExecutionGuard) -> Self {
        self.autostart_execution = guard;
        self
    }
    /// Bind the exact shared-worker lifetime through calendar writer COMMIT.
    /// Failure settlement remains permitted after cancellation; no lease or grant
    /// is created or extended by this memory-only callback.
    pub fn with_calendar_execution_guard(mut self, guard: CalendarExecutionGuard) -> Self {
        self.calendar_execution = guard;
        self
    }
    pub fn with_material_scene_guard(mut self, guard: MaterialSceneGuard) -> Self {
        self.material_scene = guard;
        self
    }
    /// Audio-domain clone only. Positive voice effects require the captured
    /// runtime generation through COMMIT. Cleanup can still settle old tokens
    /// after invalidation, and every operation has a bounded caller wait.
    pub fn with_voice_timeout(
        mut self,
        timeout: std::time::Duration,
        active: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        self.voice_timeout = Some((timeout, active));
        self
    }
    /// Immutable command context; actor/lease validation still occurs in each
    /// original business task under the same write transaction.
    /// Trusted worker issuance. No UI/event thread should wait here.
    pub fn issue_command_window(
        &self,
        request: crate::command_window::IssueCommand,
    ) -> Result<fc_ports::command_window::IssuedCommand, StorageError> {
        self.handle.try_issue_command(request)?.wait()
    }
    pub fn with_command_window(&self, issued: fc_ports::command_window::IssuedCommand) -> Self {
        let mut scoped = self.clone();
        scoped.handle = self.handle.with_command_window(issued);
        let guard = scoped.session.clone();
        scoped = scoped.with_session_guard(guard);
        scoped
    }
    pub fn load_identity(&self) -> Result<IdentityState, StorageError> {
        let bytes = self.handle.try_transaction(ReadIdentity)?.wait()?;
        codec::read_identity(&codec::value(&bytes)?)
    }
    pub fn content(
        &self,
        key: ContentKey,
        actor: &ActorContext,
    ) -> Result<Option<ContentRecord>, StorageError> {
        let bytes = self
            .handle
            .try_transaction(ReadContent {
                web_strict: false,
                key,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })?
            .wait()?;
        let v = codec::value(&bytes)?;
        if v.is_null() {
            Ok(None)
        } else {
            codec::read_record(&v).map(Some)
        }
    }
    pub fn try_content(
        &self,
        command: ContentCommand,
        actor: ActorContext,
    ) -> Result<Ticket<Vec<u8>>, StorageError> {
        self.handle.try_transaction(ContentTask {
            web_strict: false,
            command,
            actor,
            clock: self.clock.clone(),
            factory: self.factory.clone(),
            session: self.session.clone(),
        })
    }
}
pub(crate) fn load_identity(d: &dyn TransactionDocuments) -> Result<IdentityState, StorageError> {
    match d.get(IDENTITY)? {
        Some(row) => {
            let state = codec::read_identity(&codec::value(&row.value)?)?;
            for a in &state.accounts {
                for (kind, version) in [
                    ("password", a.credential_version),
                    ("recovery", a.recovery_version),
                ] {
                    if d.get(&credential_key(a.principal.id, kind, version))?
                        .is_none()
                    {
                        return Err(StorageError::Database);
                    }
                }
            }
            Ok(state)
        }
        // An existing committed database cannot silently become first registration.
        None if d.revision()? == Revision::default() => Ok(IdentityState::default()),
        None => Err(StorageError::Database),
    }
}
struct ReadIdentity;
impl<S> TransactionTask<S> for ReadIdentity {
    fn allocation_bytes(&self) -> usize {
        0
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        Ok(TransactionDecision {
            precommit: None,
            outcome: codec::bytes(&codec::identity(&load_identity(d)?))?,
            commit: None,
        })
    }
}
fn content_key(key: &ContentKey) -> String {
    match key {
        ContentKey::Homework(k) => format!(
            "business/homework/{}/{:04}-{:02}-{:02}/{}/{}",
            k.class_id.get(),
            k.actual_date.year(),
            k.actual_date.month(),
            k.actual_date.day(),
            k.subject_id.get(),
            k.owner_teacher_id.get()
        ),
        ContentKey::SharedReminder(k) => format!(
            "business/reminder/{}/{:04}-{:02}-{:02}",
            k.class_id.get(),
            k.actual_date.year(),
            k.actual_date.month(),
            k.actual_date.day()
        ),
    }
}
fn authority(s: &IdentityState, a: &ActorContext) -> Result<AuthorityContext, StorageError> {
    let account = s.account(a.principal_id).map_err(|_| {
        StorageError::Content(ContentError::Permission(
            PermissionError::StaleAuthorization,
        ))
    })?;
    if account.must_change_password {
        return Err(StorageError::Content(ContentError::Permission(
            PermissionError::PasswordChangeRequired,
        )));
    }
    s.authority(a.principal_id).map_err(|_| {
        StorageError::Content(ContentError::Permission(
            PermissionError::StaleAuthorization,
        ))
    })
}
struct ReadContent {
    web_strict: bool,
    key: ContentKey,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for ReadContent {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        (self.session)(&state, &self.actor)?;
        let now = self.clock.now();
        let a = authority(&state, &self.actor)?;
        content_web::authorize_key(&self.key, &a, &self.actor, now)?;
        if self.web_strict {
            content_web::validate_web_target(&state, &self.key, &self.actor)?;
            content_web::require_current_day(d, &state, &self.key, now)?;
        }
        if !content_web::date_is_current(d, &state, &self.key, now)? {
            (self.session)(&state, &self.actor)?;
            a.validate_actor(&self.actor, self.clock.now())
                .map_err(|e| StorageError::Content(ContentError::Permission(e)))?;
            return Ok(TransactionDecision {
                precommit: None,
                outcome: b"null".to_vec(),
                commit: None,
            });
        }
        let value = d
            .get(&content_key(&self.key))?
            .map(|v| codec::value(&v.value))
            .transpose()?;
        let value = match value {
            Some(v) => {
                let record = codec::read_record(&v)?;
                if record.key != self.key {
                    return Err(StorageError::Database);
                }
                v
            }
            None => Value::Null,
        };
        (self.session)(&state, &self.actor)?;
        let now = self.clock.now();
        content_web::authorize_key(&self.key, &a, &self.actor, now)?;
        if self.web_strict {
            content_web::validate_web_target(&state, &self.key, &self.actor)?;
            content_web::require_current_day(d, &state, &self.key, now)?;
        }
        // Empty is a real committed slot with a CAS revision, not an absent row.
        let value = if content_web::date_is_current(d, &state, &self.key, now)? {
            value
        } else {
            Value::Null
        };
        Ok(TransactionDecision {
            precommit: None,
            outcome: codec::bytes(&value)?,
            commit: None,
        })
    }
}
struct ContentTask<S> {
    web_strict: bool,
    command: ContentCommand,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    factory: Factory<S>,
    session: SessionGuard,
}
impl<S: Send> TransactionTask<S> for ContentTask<S> {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((
            fc_ports::command_window::CommandFamily::Content,
            self.command.command_id,
        ))
    }

    fn allocation_bytes(&self) -> usize {
        self.command.text.capacity() + std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        (self.session)(&state, &self.actor)?;
        let a = authority(&state, &self.actor)?;
        content_web::require_current_day(d, &state, &self.command.key, self.clock.now())?;
        if self.web_strict {
            content_web::validate_web_target(&state, &self.command.key, &self.actor)?;
        }
        let journal = format!("business/receipt/content/{}", self.command.command_id.get());
        let payload = codec::bytes(
            &json!({"actor":codec::actor(&self.actor),"key":codec::key(&self.command.key),"expected":self.command.expected_revision.get(),"text":self.command.text}),
        )?;
        let digest = Sha256::digest(&payload).to_vec();
        if let Some(receipt) = d.get(&journal)? {
            let v = codec::value(&receipt.value)?;
            codec::object_fields(&v, &["fingerprint", "record"])?;
            let expected: Vec<u8> = serde_json::from_value(v["fingerprint"].clone())
                .map_err(|_| StorageError::Database)?;
            if expected.len() != 32 {
                return Err(StorageError::Database);
            }
            if expected != digest {
                return Err(StorageError::Content(ContentError::IdempotencyConflict));
            }
            let record = content_web::receipt(&v)?;
            if record.key != self.command.key
                || record.last_actor_id != self.actor.principal_id
                || record.text != self.command.normalized_text()
                || record.revision
                    != self
                        .command
                        .expected_revision
                        .next()
                        .map_err(|_| StorageError::Database)?
            {
                return Err(StorageError::Database);
            }
            // Replays never mutate; current authority still gates release of original result.
            (self.session)(&state, &self.actor)?;
            let now = self.clock.now();
            content_web::require_current_day(d, &state, &self.command.key, now)?;
            if self.web_strict {
                content_web::validate_web_target(&state, &self.command.key, &self.actor)?;
            }
            self.command
                .validate(&a, &self.actor, now)
                .map_err(StorageError::Content)?;
            return Ok(TransactionDecision {
                precommit: None,
                outcome: codec::bytes(&v["record"])?,
                commit: None,
            });
        }
        let key = content_key(&self.command.key);
        let current = d
            .get(&key)?
            .map(|v| codec::value(&v.value).and_then(|v| codec::read_record(&v)))
            .transpose()?;
        if current
            .as_ref()
            .is_some_and(|record| record.key != self.command.key)
        {
            return Err(StorageError::Database);
        }
        let now = self.clock.now();
        content_web::require_current_day(d, &state, &self.command.key, now)?;
        let record = self
            .command
            .apply(&a, &self.actor, now, current.as_ref())
            .map_err(StorageError::Content)?;
        let snapshot = (self.factory)(
            &state,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        (self.session)(&state, &self.actor)?;
        // Recheck time after preparation immediately before returning the commit plan.
        self.command
            .validate(&a, &self.actor, self.clock.now())
            .map_err(StorageError::Content)?;
        let value = codec::record(&record);
        let outcome = codec::bytes(&value)?;
        let receipt = codec::bytes(&json!({"fingerprint":digest,"record":value}))?;
        let mut mutations = vec![
            Mutation::Put {
                key,
                value: outcome.clone(),
            },
            Mutation::Put {
                key: journal,
                value: receipt,
            },
        ];
        if let Some(watermark) = desktop::advance_watermark(d, &state, now.date)? {
            mutations.push(watermark);
        }
        let session = self.session.clone();
        let actor = self.actor.clone();
        let clock = self.clock.clone();
        let command = self.command.clone();
        let web_strict = self.web_strict;
        Ok(TransactionDecision {
            precommit: Some(Box::new(move || {
                session(&state, &actor)?;
                if web_strict {
                    content_web::validate_web_target(&state, &command.key, &actor)?;
                }
                command
                    .validate(&a, &actor, clock.now())
                    .map_err(StorageError::Content)
            })),
            outcome: outcome.clone(),
            commit: Some((mutations, snapshot)),
        })
    }
}
fn repository_error(e: StorageError) -> RepositoryError {
    if let Some(error) = e.command_window_error() {
        return RepositoryError::CommandWindow(error);
    }
    match e {
        StorageError::Content(e) => RepositoryError::Domain(e),
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => RepositoryError::Faulted,
        _ => RepositoryError::Unavailable,
    }
}
impl<S: Send + 'static> ContentRepository for BusinessRepository<S> {
    fn authority(&self, actor: &ActorContext) -> Result<AuthorityContext, RepositoryError> {
        authority(&self.load_identity().map_err(repository_error)?, actor).map_err(repository_error)
    }
    fn execute(
        &mut self,
        command: ContentCommand,
        actor: &ActorContext,
        _clock: &dyn Clock,
    ) -> Result<ContentRecord, RepositoryError> {
        let bytes = self
            .try_content(command, actor.clone())
            .map_err(repository_error)?
            .wait()
            .map_err(repository_error)?;
        codec::read_record(&codec::value(&bytes).map_err(repository_error)?)
            .map_err(repository_error)
    }
}

/// Already protected, version-bound cryptographic records prepared by the trusted
/// credential worker. Deliberately not Deserialize, Debug, or Clone. Never accept
/// these bytes as a request body or use this API as proof of password verification.
struct PreparedCredentials {
    principal_id: PrincipalId,
    password: Option<(u64, Vec<u8>)>,
    recovery: Option<(u64, Vec<u8>)>,
}
pub(crate) fn credential_key(id: PrincipalId, kind: &str, version: u64) -> String {
    format!("business/credential/{}/{}/{}", id.get(), kind, version)
}
fn identity_error(e: StorageError) -> IdentityRepositoryError {
    if let Some(error) = e.command_window_error() {
        return IdentityRepositoryError::CommandWindow(error);
    }
    match e {
        StorageError::Identity(e) => IdentityRepositoryError::Domain(e),
        StorageError::DocumentTooLarge => IdentityRepositoryError::Domain(IdentityError::Capacity),
        StorageError::CommandIdReused => IdentityRepositoryError::IdempotencyConflict,
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => IdentityRepositoryError::Faulted,
        _ => IdentityRepositoryError::Unavailable,
    }
}
impl<S: Send + 'static> BusinessRepository<S> {
    /// Trusted composition boundary: caller MUST verify a password/recovery proof
    /// against the same expected version before invoking security commands.
    fn execute_identity_with_credentials(
        &mut self,
        request: IdentityRequest,
        actor: Option<&ActorContext>,
        credentials: PreparedCredentials,
    ) -> Result<IdentityTransition, IdentityRepositoryError> {
        self.execute_identity_inner(request, actor, Some(credentials))
    }
    fn execute_identity_inner(
        &mut self,
        request: IdentityRequest,
        actor: Option<&ActorContext>,
        credentials: Option<PreparedCredentials>,
    ) -> Result<IdentityTransition, IdentityRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(IdentityTask {
                request,
                actor: actor.cloned(),
                credentials,
                clock: self.clock.clone(),
                factory: self.factory.clone(),
                session: self.session.clone(),
                initial_installation_identity: self.initial_installation_identity,
            })
            .map_err(identity_error)?
            .wait()
            .map_err(identity_error)?;
        let v = codec::value(&bytes).map_err(identity_error)?;
        codec::object_fields(&v, &["state", "events"]).map_err(identity_error)?;
        let state = codec::read_identity(&v["state"]).map_err(identity_error)?;
        let events = decode_events(&v["events"]).map_err(identity_error)?;
        Ok(IdentityTransition {
            next: state,
            events,
        })
    }
    /// Read only the original default-teacher receipt. Absence never executes
    /// the request, and the caller's current identity is not the saved result.
    pub fn default_teacher_outcome(
        &self,
        request: IdentityRequest,
        actor: &ActorContext,
    ) -> Result<Option<(Revision, DefaultTeacher)>, IdentityRepositoryError> {
        if !matches!(request.command, IdentityCommand::SetDefaultTeacher(_)) {
            return Err(IdentityRepositoryError::Domain(IdentityError::InvalidState));
        }
        let bytes = self
            .handle
            .try_transaction(DefaultTeacherOutcomeTask {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(identity_error)?
            .wait()
            .map_err(identity_error)?;
        let value = codec::value(&bytes).map_err(identity_error)?;
        if value.is_null() {
            return Ok(None);
        }
        codec::object_fields(&value, &["revision", "class", "subject", "teacher"])
            .map_err(identity_error)?;
        let number = |name| codec::number(&value, name).map_err(identity_error);
        Ok(Some((
            Revision::new(number("revision")?),
            DefaultTeacher {
                class_id: ClassId::new(number("class")?)
                    .map_err(|_| IdentityRepositoryError::Unavailable)?,
                subject_id: SubjectId::new(number("subject")?)
                    .map_err(|_| IdentityRepositoryError::Unavailable)?,
                teacher_id: PrincipalId::new(number("teacher")?)
                    .map_err(|_| IdentityRepositoryError::Unavailable)?,
            },
        )))
    }
    pub fn subject_outcome(
        &self,
        request: IdentityRequest,
        actor: &ActorContext,
    ) -> Result<Option<(Revision, SchoolSubject)>, IdentityRepositoryError> {
        if !matches!(request.command, IdentityCommand::PutSubject { .. }) {
            return Err(IdentityRepositoryError::Domain(IdentityError::InvalidState));
        }
        let bytes = self
            .handle
            .try_transaction(SubjectOutcomeTask {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(identity_error)?
            .wait()
            .map_err(identity_error)?;
        let value = codec::value(&bytes).map_err(identity_error)?;
        if value.is_null() {
            return Ok(None);
        }
        codec::object_fields(
            &value,
            &[
                "state_revision",
                "id",
                "school",
                "name",
                "enabled",
                "red",
                "green",
                "blue",
                "revision",
            ],
        )
        .map_err(identity_error)?;
        let number = |name| codec::number(&value, name).map_err(identity_error);
        let color =
            |name| u8::try_from(number(name)?).map_err(|_| IdentityRepositoryError::Unavailable);
        Ok(Some((
            Revision::new(number("state_revision")?),
            SchoolSubject {
                id: SubjectId::new(number("id")?)
                    .map_err(|_| IdentityRepositoryError::Unavailable)?,
                school_id: SchoolId::new(number("school")?)
                    .map_err(|_| IdentityRepositoryError::Unavailable)?,
                name: codec::text(&value, "name").map_err(identity_error)?,
                enabled: codec::boolean(&value, "enabled").map_err(identity_error)?,
                color: Rgb {
                    red: color("red")?,
                    green: color("green")?,
                    blue: color("blue")?,
                },
                revision: Revision::new(number("revision")?),
            },
        )))
    }
    /// Trusted credential worker only; records are never authority/UI snapshots.
    pub fn credential_record(
        &self,
        id: PrincipalId,
        recovery: bool,
    ) -> Result<(u64, Vec<u8>), StorageError> {
        let bytes = self
            .handle
            .try_transaction(ReadCredential { id, recovery })?
            .wait()?;
        let v = codec::value(&bytes)?;
        codec::object_fields(&v, &["version", "record"])?;
        let version = codec::number(&v, "version")?;
        let bytes =
            serde_json::from_value(v["record"].clone()).map_err(|_| StorageError::Database)?;
        Ok((version, bytes))
    }
}
impl<S: Send + 'static> IdentityRepository for BusinessRepository<S> {
    fn state(&self) -> Result<IdentityState, IdentityRepositoryError> {
        self.load_identity().map_err(identity_error)
    }
    fn execute_identity(
        &mut self,
        request: IdentityRequest,
        actor: Option<&ActorContext>,
        _clock: &dyn Clock,
    ) -> Result<IdentityTransition, IdentityRepositoryError> {
        self.execute_identity_inner(request, actor, None)
    }
}
struct ReadCredential {
    id: PrincipalId,
    recovery: bool,
}
impl<S> TransactionTask<S> for ReadCredential {
    fn allocation_bytes(&self) -> usize {
        0
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let s = load_identity(d)?;
        let a = s.account(self.id).map_err(StorageError::Identity)?;
        let version = if self.recovery {
            a.recovery_version
        } else {
            a.credential_version
        };
        let r = d
            .get(&credential_key(
                self.id,
                if self.recovery {
                    "recovery"
                } else {
                    "password"
                },
                version,
            ))?
            .ok_or(StorageError::Database)?;
        Ok(TransactionDecision {
            precommit: None,
            outcome: codec::bytes(&json!({"version":version,"record":r.value}))?,
            commit: None,
        })
    }
}
struct DefaultTeacherOutcomeTask {
    request: IdentityRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for DefaultTeacherOutcomeTask {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((
            fc_ports::command_window::CommandFamily::Identity,
            self.request.command_id,
        ))
    }
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        let check = || -> Result<(), StorageError> {
            (self.session)(&state, &self.actor)?;
            state
                .authorize_management(&self.actor, self.clock.now())
                .map_err(StorageError::Identity)
        };
        check()?;
        let IdentityCommand::SetDefaultTeacher(expected) = &self.request.command else {
            return Err(StorageError::InvalidCommand);
        };
        let mut digest = Sha256::new();
        digest.update(format!("{:?}", self.request));
        digest.update(codec::bytes(&codec::actor(&self.actor))?);
        let fingerprint = digest.finalize().to_vec();
        let value = match d.get(&format!(
            "business/receipt/identity/{}",
            self.request.command_id.get()
        ))? {
            None => Value::Null,
            Some(receipt) => {
                let value = codec::value(&receipt.value)?;
                codec::object_fields(&value, &["fingerprint", "outcome"])?;
                codec::object_fields(&value["outcome"], &["state", "events"])?;
                let original: Vec<u8> = serde_json::from_value(value["fingerprint"].clone())
                    .map_err(|_| StorageError::Database)?;
                if original.len() != 32 || original != fingerprint {
                    return Err(StorageError::CommandIdReused);
                }
                let historical = codec::read_identity(&value["outcome"]["state"])?;
                let events = decode_events(&value["outcome"]["events"])?;
                if historical.revision
                    != self
                        .request
                        .expected_revision
                        .next()
                        .map_err(|_| StorageError::Database)?
                    || events != [IdentityEvent::CatalogChanged]
                    || !historical.default_teachers.contains(expected)
                {
                    return Err(StorageError::Database);
                }
                json!({"revision":historical.revision.get(),"class":expected.class_id.get(),"subject":expected.subject_id.get(),"teacher":expected.teacher_id.get()})
            }
        };
        check()?;
        Ok(TransactionDecision {
            outcome: codec::bytes(&value)?,
            commit: None,
            precommit: None,
        })
    }
}
struct SubjectOutcomeTask {
    request: IdentityRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for SubjectOutcomeTask {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((
            fc_ports::command_window::CommandFamily::Identity,
            self.request.command_id,
        ))
    }
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>().saturating_add(match &self.request.command {
            IdentityCommand::PutSubject { name, .. } => name.capacity(),
            _ => 0,
        })
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        let check = || -> Result<(), StorageError> {
            (self.session)(&state, &self.actor)?;
            state
                .authorize_management(&self.actor, self.clock.now())
                .map_err(StorageError::Identity)
        };
        check()?;
        let IdentityCommand::PutSubject {
            id,
            name,
            enabled,
            color,
            expected_revision,
        } = &self.request.command
        else {
            return Err(StorageError::InvalidCommand);
        };
        let mut digest = Sha256::new();
        digest.update(format!("{:?}", self.request));
        digest.update(codec::bytes(&codec::actor(&self.actor))?);
        let fingerprint = digest.finalize().to_vec();
        let value = match d.get(&format!(
            "business/receipt/identity/{}",
            self.request.command_id.get()
        ))? {
            None => Value::Null,
            Some(receipt) => {
                let value = codec::value(&receipt.value)?;
                codec::object_fields(&value, &["fingerprint", "outcome"])?;
                codec::object_fields(&value["outcome"], &["state", "events"])?;
                let original: Vec<u8> = serde_json::from_value(value["fingerprint"].clone())
                    .map_err(|_| StorageError::Database)?;
                if original.len() != 32 || original != fingerprint {
                    return Err(StorageError::CommandIdReused);
                }
                let historical = codec::read_identity(&value["outcome"]["state"])?;
                let events = decode_events(&value["outcome"]["events"])?;
                let subject = historical
                    .subjects
                    .iter()
                    .find(|s| s.id == *id)
                    .ok_or(StorageError::Database)?;
                let subject_revision = expected_revision
                    .unwrap_or_default()
                    .next()
                    .map_err(|_| StorageError::Database)?;
                if historical.revision
                    != self
                        .request
                        .expected_revision
                        .next()
                        .map_err(|_| StorageError::Database)?
                    || events.last() != Some(&IdentityEvent::CatalogChanged)
                    || events.iter().any(|e| {
                        !matches!(
                            e,
                            IdentityEvent::CatalogChanged | IdentityEvent::AuthorizationRevoked(_)
                        )
                    })
                    || subject.name != *name
                    || subject.enabled != *enabled
                    || subject.color != *color
                    || subject.revision != subject_revision
                {
                    return Err(StorageError::Database);
                }
                json!({"state_revision":historical.revision.get(),"id":subject.id.get(),"school":subject.school_id.get(),
                    "name":subject.name,"enabled":subject.enabled,"red":subject.color.red,"green":subject.color.green,
                    "blue":subject.color.blue,"revision":subject.revision.get()})
            }
        };
        check()?;
        let outcome = codec::bytes(&value)?;
        if outcome.len() > 1024 {
            return Err(StorageError::Database);
        }
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}
struct IdentityTask<S> {
    initial_installation_identity: Option<[u8; 32]>,
    request: IdentityRequest,
    actor: Option<ActorContext>,
    credentials: Option<PreparedCredentials>,
    clock: Arc<dyn Clock + Send + Sync>,
    factory: Factory<S>,
    session: SessionGuard,
}
fn events(e: &[IdentityEvent]) -> Value {
    Value::Array(
        e.iter()
            .map(|e| match e {
                IdentityEvent::Initialized => json!(["initialized"]),
                IdentityEvent::CatalogChanged => json!(["catalog"]),
                IdentityEvent::AccountChanged(id) => json!(["account", id.get()]),
                IdentityEvent::AuthorizationRevoked(id) => json!(["revoked", id.get()]),
                IdentityEvent::AllAuthorizationRevoked => json!(["all-revoked"]),
            })
            .collect(),
    )
}
fn decode_events(v: &Value) -> Result<Vec<IdentityEvent>, StorageError> {
    v.as_array()
        .ok_or(StorageError::Database)?
        .iter()
        .map(|v| {
            let entry = v.as_array().ok_or(StorageError::Database)?;
            let expected = match entry.first().and_then(Value::as_str) {
                Some("account" | "revoked") => 2,
                Some("initialized" | "catalog" | "all-revoked") => 1,
                _ => return Err(StorageError::Database),
            };
            if entry.len() != expected {
                return Err(StorageError::Database);
            }
            Ok(match v[0].as_str() {
                Some("initialized") => IdentityEvent::Initialized,
                Some("catalog") => IdentityEvent::CatalogChanged,
                Some("account") => IdentityEvent::AccountChanged(
                    PrincipalId::new(v[1].as_u64().ok_or(StorageError::Database)?)
                        .map_err(|_| StorageError::Database)?,
                ),
                Some("revoked") => IdentityEvent::AuthorizationRevoked(
                    PrincipalId::new(v[1].as_u64().ok_or(StorageError::Database)?)
                        .map_err(|_| StorageError::Database)?,
                ),
                Some("all-revoked") => IdentityEvent::AllAuthorizationRevoked,
                _ => return Err(StorageError::Database),
            })
        })
        .collect()
}
impl<S: Send> TransactionTask<S> for IdentityTask<S> {
    fn retirement(&self) -> CommandRetirement {
        if matches!(
            self.request.command,
            IdentityCommand::RecoveryConsumed { .. }
        ) {
            CommandRetirement::IdentityRecovery
        } else {
            CommandRetirement::None
        }
    }

    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((
            fc_ports::command_window::CommandFamily::Identity,
            self.request.command_id,
        ))
    }

    fn allocation_bytes(&self) -> usize {
        let command = match &self.request.command {
            IdentityCommand::FirstAdministrator {
                school_name,
                login_name,
                display_name,
                ..
            } => school_name
                .capacity()
                .saturating_add(login_name.capacity())
                .saturating_add(display_name.capacity()),
            IdentityCommand::CreateClass { name, .. }
            | IdentityCommand::RenameSchool { name }
            | IdentityCommand::UpdateClass { name, .. }
            | IdentityCommand::PutSubject { name, .. } => name.capacity(),
            IdentityCommand::CreateAccount(a) => a
                .login_name
                .capacity()
                .saturating_add(a.display_name.capacity())
                .saturating_add(
                    a.scopes
                        .capacity()
                        .saturating_mul(std::mem::size_of::<AssignmentScope>()),
                ),
            IdentityCommand::UpdateAccount {
                display_name,
                scopes,
                ..
            } => display_name.capacity().saturating_add(
                scopes
                    .capacity()
                    .saturating_mul(std::mem::size_of::<AssignmentScope>()),
            ),
            _ => 0,
        };
        std::mem::size_of::<Self>()
            .saturating_add(command)
            .saturating_add(self.credentials.as_ref().map_or(0, |c| {
                c.password
                    .as_ref()
                    .map_or(0, |p| p.1.capacity())
                    .saturating_add(c.recovery.as_ref().map_or(0, |p| p.1.capacity()))
            }))
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        if self.actor.is_none() {
            if let Some(issued) = d.active_command() {
                use fc_ports::command_window::CommandOwner;
                let owner = match self.request.command {
                    IdentityCommand::FirstAdministrator { .. } => CommandOwner::PublicDesktop,
                    IdentityCommand::RecoveryConsumed { id, .. } => {
                        let account = state.account(id).map_err(StorageError::Identity)?;
                        CommandOwner::Principal {
                            principal: id,
                            school: account.principal.school_id,
                            epoch: account.principal.auth_epoch,
                            origin: TrustedOrigin::Local,
                        }
                    }
                    _ => return Err(StorageError::CommandNotIssued),
                };
                if issued.owner != owner {
                    return Err(StorageError::CommandNotIssued);
                }
            }
        }

        if let Some(actor) = &self.actor {
            (self.session)(&state, actor)?;
        }
        let request = &self.request;
        let journal = format!("business/receipt/identity/{}", request.command_id.get());
        let fingerprint = identity_fingerprint(
            request,
            self.actor.as_ref(),
            self.initial_installation_identity,
            self.credentials.as_ref(),
        )?;
        if let Some(receipt) = d.get(&journal)? {
            let v = codec::value(&receipt.value)?;
            codec::object_fields(&v, &["fingerprint", "outcome"])?;
            codec::object_fields(&v["outcome"], &["state", "events"])?;
            let original: Vec<u8> = serde_json::from_value(v["fingerprint"].clone())
                .map_err(|_| StorageError::Database)?;
            if original.len() != 32 {
                return Err(StorageError::Database);
            }
            if original != fingerprint {
                return Err(StorageError::CommandIdReused);
            }
            if let Some(a) = &self.actor {
                state
                    .authority(a.principal_id)
                    .map_err(StorageError::Identity)?
                    .validate_actor(a, self.clock.now())
                    .map_err(|e| StorageError::Identity(IdentityError::Permission(e)))?;
            }
            return Ok(TransactionDecision {
                precommit: None,
                outcome: codec::bytes(&v["outcome"])?,
                commit: None,
            });
        }
        if state.revision != request.expected_revision {
            return Err(StorageError::Identity(IdentityError::RevisionConflict));
        }
        let required = match &request.command {
            IdentityCommand::FirstAdministrator { id, .. } => Some((*id, true, true)),
            IdentityCommand::CreateAccount(a) => Some((a.id, true, true)),
            IdentityCommand::PasswordChanged { id, .. } => Some((*id, true, false)),
            IdentityCommand::RecoveryConsumed { id, .. } => Some((*id, true, true)),
            IdentityCommand::RecoveryRotated { id, .. } => Some((*id, false, true)),
            IdentityCommand::TransferAdministrator { from, .. } => Some((*from, false, false)),
            _ => None,
        };
        if required.is_some() && self.credentials.is_none() {
            return Err(StorageError::Identity(IdentityError::Permission(
                PermissionError::RoleDenied,
            )));
        }
        if required.is_none() && self.credentials.is_some() {
            return Err(StorageError::InvalidCommand);
        }
        let transition = state
            .transition(&request.command, self.actor.as_ref(), self.clock.now())
            .map_err(StorageError::Identity)?;
        schedule::check_identity_change(d, &state, &transition.next)?;
        let mut mutations = Vec::new();
        if matches!(request.command, IdentityCommand::FirstAdministrator { .. }) {
            if let Some(id) = self.initial_installation_identity {
                recovery::validate_installation_id(id)?;
                if recovery::installation(d)?.is_some() {
                    return Err(StorageError::RecoveryConflict);
                }
                mutations.push(Mutation::Put {
                    key: recovery::INSTALLATION.into(),
                    value: recovery::installation_bytes(id)?,
                });
            }
        }
        if transition.next.installation.is_some() {
            let document = schedule::load(d, &state)?;
            let (_, change) = lesson::observe(d, &transition.next, &document, self.clock.now())?;
            if let Some(change) = change {
                mutations.push(change);
            }
        }
        if let Some((id, password, recovery)) = required {
            let c = self
                .credentials
                .as_ref()
                .ok_or(StorageError::InvalidCommand)?;
            let a = transition
                .next
                .account(id)
                .map_err(StorageError::Identity)?;
            if c.principal_id != id
                || c.password.is_some() != password
                || c.recovery.is_some() != recovery
            {
                return Err(StorageError::InvalidCommand);
            }
            for (kind, pair, expected) in [
                ("password", &c.password, a.credential_version),
                ("recovery", &c.recovery, a.recovery_version),
            ] {
                if let Some((version, record)) = pair {
                    if *version != expected || record.is_empty() || record.len() > 4096 {
                        return Err(StorageError::InvalidCommand);
                    }
                    mutations.push(Mutation::Put {
                        key: credential_key(id, kind, *version),
                        value: record.clone(),
                    });
                    // Replaced records no longer authenticate and must not grow without bound.
                    if let Ok(old) = state.account(id) {
                        let previous = if kind == "password" {
                            old.credential_version
                        } else {
                            old.recovery_version
                        };
                        if previous != *version {
                            mutations.push(Mutation::Delete {
                                key: credential_key(id, kind, previous),
                            });
                        }
                    }
                }
            }
        }
        let snapshot = (self.factory)(
            &transition.next,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        if let Some(actor) = &self.actor {
            (self.session)(&state, actor)?;
        }
        // Validate mutable date/expiry once more after preparation.
        state
            .transition(&request.command, self.actor.as_ref(), self.clock.now())
            .map_err(StorageError::Identity)?;
        let outcome =
            json!({"state":codec::identity(&transition.next),"events":events(&transition.events)});
        let identity_value = codec::bytes(&codec::identity(&transition.next))?;
        let receipt_value = codec::bytes(&json!({"fingerprint":fingerprint,"outcome":outcome}))?;
        // Domain count ceilings do not imply this release's complete identity
        // and receipt fit their independent 64KiB persisted value budgets.
        // Reject before authority update/COMMIT, without truncating assignments.
        if identity_value.len() > MAX_VALUE_BYTES || receipt_value.len() > MAX_VALUE_BYTES {
            return Err(StorageError::DocumentTooLarge);
        }
        mutations.push(Mutation::Put {
            key: IDENTITY.into(),
            value: identity_value,
        });
        mutations.push(Mutation::Put {
            key: journal,
            value: receipt_value,
        });
        let session = self.session.clone();
        let actor = self.actor.clone();
        let clock = self.clock.clone();
        let command = self.request.command.clone();
        Ok(TransactionDecision {
            precommit: Some(Box::new(move || {
                if let Some(a) = &actor {
                    session(&state, a)?;
                }
                state
                    .transition(&command, actor.as_ref(), clock.now())
                    .map(|_| ())
                    .map_err(StorageError::Identity)
            })),
            outcome: codec::bytes(&outcome)?,
            commit: Some((mutations, snapshot)),
        })
    }
}

impl<S: Send + 'static> CredentialIdentityRepository<Vec<u8>, Vec<u8>> for BusinessRepository<S> {
    fn execute_credentials(
        &mut self,
        change: PreparedCredentialChange<Vec<u8>, Vec<u8>>,
        actor: Option<&ActorContext>,
        _clock: &dyn Clock,
    ) -> Result<IdentityTransition, IdentityRepositoryError> {
        let (request, password, recovery) = change.into_parts();
        let state = self.load_identity().map_err(identity_error)?;
        let (id, pv, rv) =
            match &request.command {
                IdentityCommand::FirstAdministrator { id, .. } => (*id, 1, 1),
                IdentityCommand::CreateAccount(a) => (a.id, 1, 1),
                IdentityCommand::PasswordChanged {
                    id,
                    expected_credential_version,
                } => (
                    *id,
                    expected_credential_version.checked_add(1).ok_or(
                        IdentityRepositoryError::Domain(IdentityError::VersionOverflow),
                    )?,
                    0,
                ),
                IdentityCommand::RecoveryConsumed {
                    id,
                    expected_recovery_version,
                } => {
                    let a = state
                        .account(*id)
                        .map_err(IdentityRepositoryError::Domain)?;
                    (
                        *id,
                        a.credential_version.checked_add(1).ok_or(
                            IdentityRepositoryError::Domain(IdentityError::VersionOverflow),
                        )?,
                        expected_recovery_version.checked_add(1).ok_or(
                            IdentityRepositoryError::Domain(IdentityError::VersionOverflow),
                        )?,
                    )
                }
                IdentityCommand::RecoveryRotated {
                    id,
                    expected_recovery_version,
                } => (
                    *id,
                    0,
                    expected_recovery_version.checked_add(1).ok_or(
                        IdentityRepositoryError::Domain(IdentityError::VersionOverflow),
                    )?,
                ),
                IdentityCommand::TransferAdministrator { from, .. } => (*from, 0, 0),
                _ => return Err(IdentityRepositoryError::Domain(IdentityError::InvalidState)),
            };
        self.execute_identity_with_credentials(
            request,
            actor,
            PreparedCredentials {
                principal_id: id,
                password: password.map(|v| (pv, v)),
                recovery: recovery.map(|v| (rv, v)),
            },
        )
    }
}

// Account result lookup deliberately shares the existing identity receipt and
// fingerprint grammar. No credential schema, command-window permission or TTL
// changes. Keep here to avoid another module/file in the bounded source tree.
fn identity_fingerprint_seed(
    request: &IdentityRequest,
    actor: Option<&ActorContext>,
    installation: Option<[u8; 32]>,
) -> Result<Sha256, StorageError> {
    let mut digest = Sha256::new();
    digest.update(format!("{:?}", request));
    if matches!(request.command, IdentityCommand::FirstAdministrator { .. }) {
        if let Some(id) = installation {
            digest.update(b"installation/v1\0");
            digest.update(id);
        }
    }
    digest.update(codec::bytes(
        &actor.map(codec::actor).unwrap_or(Value::Null),
    )?);
    Ok(digest)
}
fn fingerprint_record(digest: &mut Sha256, version: u64, record: &[u8]) {
    digest.update(version.to_be_bytes());
    digest.update((record.len() as u64).to_be_bytes());
    digest.update(record);
}
fn identity_fingerprint(
    request: &IdentityRequest,
    actor: Option<&ActorContext>,
    installation: Option<[u8; 32]>,
    credentials: Option<&PreparedCredentials>,
) -> Result<Vec<u8>, StorageError> {
    let mut digest = identity_fingerprint_seed(request, actor, installation)?;
    if let Some(c) = credentials {
        digest.update(c.principal_id.get().to_be_bytes());
        for pair in [&c.password, &c.recovery] {
            if let Some((version, record)) = pair {
                fingerprint_record(&mut digest, *version, record);
            } else {
                digest.update([0]);
            }
        }
    }
    Ok(digest.finalize().to_vec())
}
fn account_request_bytes(request: &IdentityRequest) -> usize {
    match &request.command {
        IdentityCommand::CreateAccount(a) => a
            .login_name
            .capacity()
            .saturating_add(a.display_name.capacity())
            .saturating_add(
                a.scopes
                    .capacity()
                    .saturating_mul(std::mem::size_of::<AssignmentScope>()),
            ),
        IdentityCommand::UpdateAccount {
            display_name,
            scopes,
            ..
        } => display_name.capacity().saturating_add(
            scopes
                .capacity()
                .saturating_mul(std::mem::size_of::<AssignmentScope>()),
        ),
        _ => 0,
    }
}
/// Non-secret evidence from the original prepared request, before Writer admission.
/// No Deserialize/public constructor: callers cannot manufacture or replace the
/// digest. It cannot authorize a write or regenerate an initial recovery code.
#[derive(PartialEq, Eq)]
pub struct AccountReceiptProof {
    issued: fc_ports::command_window::IssuedCommand,
    request: IdentityRequest,
    fingerprint: [u8; 32],
}
/// The unchanged prepared credential payload and its original result proof.
pub type PreparedAccountWithReceipt = (
    PreparedCredentialChange<Vec<u8>, Vec<u8>>,
    AccountReceiptProof,
);
impl std::fmt::Debug for AccountReceiptProof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountReceiptProof")
            .field("issued", &self.issued)
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}
impl AccountReceiptProof {
    pub fn issued(&self) -> fc_ports::command_window::IssuedCommand {
        self.issued
    }
    pub fn request(&self) -> &IdentityRequest {
        &self.request
    }
}
/// Historical public data only. Never feed this result to authority publication,
/// login, session renewal or Backend::apply. Current identity is read separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalAccountResult {
    pub revision: Revision,
    pub account: UserAccount,
    pub scopes: Vec<AssignmentScope>,
}
impl OriginalAccountResult {
    pub fn matches(&self, expected: Revision, command: &IdentityCommand) -> bool {
        if expected.next().ok() != Some(self.revision) {
            return false;
        }
        let a = &self.account;
        match command {
            IdentityCommand::CreateAccount(spec) => {
                a.principal.id == spec.id
                    && a.login_name == spec.login_name
                    && a.display_name == spec.display_name
                    && a.principal.role == spec.role
                    && a.principal.enabled
                    && a.principal.auth_epoch == AuthEpoch::new(1)
                    && a.must_change_password
                    && a.credential_version == 1
                    && a.recovery_version == 1
                    && self.scopes == spec.scopes
            }
            IdentityCommand::UpdateAccount {
                id,
                display_name,
                role,
                enabled,
                scopes,
            } => {
                a.principal.id == *id
                    && a.display_name == *display_name
                    && a.principal.role == *role
                    && a.principal.enabled == *enabled
                    && self.scopes == *scopes
            }
            IdentityCommand::TransferAdministrator { from, target, .. } => {
                from != target
                    && a.principal.id == *target
                    && a.principal.role == Role::GlobalAdmin
                    && a.principal.enabled
                    && !a.must_change_password
            }
            _ => false,
        }
    }
}
fn account_result_value(result: &OriginalAccountResult) -> Value {
    let a = &result.account;
    json!({"revision":result.revision.get(),"account":{
        "principal":codec::principal(&a.principal),"login":a.login_name,"display":a.display_name,
        "change":a.must_change_password,"credential":a.credential_version,"recovery":a.recovery_version},
        "scopes":result.scopes.iter().map(|s|json!([s.class_id.get(),s.subject_id.get()])).collect::<Vec<_>>()})
}
fn read_account_result(value: &Value) -> Result<OriginalAccountResult, StorageError> {
    codec::object_fields(value, &["revision", "account", "scopes"])?;
    let a = &value["account"];
    codec::object_fields(
        a,
        &[
            "principal",
            "login",
            "display",
            "change",
            "credential",
            "recovery",
        ],
    )?;
    let scopes = value["scopes"].as_array().ok_or(StorageError::Database)?;
    if scopes.len() > MAX_ASSIGNMENTS {
        return Err(StorageError::Database);
    }
    Ok(OriginalAccountResult {
        revision: Revision::new(codec::number(value, "revision")?),
        account: UserAccount {
            principal: codec::read_principal(&a["principal"])?,
            login_name: codec::text(a, "login")?,
            display_name: codec::text(a, "display")?,
            must_change_password: codec::boolean(a, "change")?,
            credential_version: codec::number(a, "credential")?,
            recovery_version: codec::number(a, "recovery")?,
        },
        scopes: scopes
            .iter()
            .map(|v| {
                let pair = v.as_array().ok_or(StorageError::Database)?;
                if pair.len() != 2 {
                    return Err(StorageError::Database);
                }
                Ok(AssignmentScope {
                    class_id: ClassId::new(pair[0].as_u64().ok_or(StorageError::Database)?)
                        .map_err(|_| StorageError::Database)?,
                    subject_id: SubjectId::new(pair[1].as_u64().ok_or(StorageError::Database)?)
                        .map_err(|_| StorageError::Database)?,
                })
            })
            .collect::<Result<_, StorageError>>()?,
    })
}
impl<S: Send + 'static> BusinessRepository<S> {
    fn account_proof_context(
        &self,
        request: &IdentityRequest,
        actor: &ActorContext,
    ) -> Result<fc_ports::command_window::IssuedCommand, StorageError> {
        let issued = self
            .handle
            .command_window
            .ok_or(StorageError::CommandNotIssued)?;
        check_account_proof_binding(issued, request, actor)?;
        Ok(issued)
    }
    /// Consume/map the exact encoded prepared records back into the same
    /// immutable payload. No clone, lookup of current credentials, or second KDF.
    /// The caller must bind the returned proof before submitting returned change.
    pub fn prepare_account_creation_receipt(
        &self,
        change: PreparedCredentialChange<Vec<u8>, Vec<u8>>,
        actor: &ActorContext,
    ) -> Result<PreparedAccountWithReceipt, StorageError> {
        let request = change.request().clone();
        if !matches!(request.command, IdentityCommand::CreateAccount(_)) {
            return Err(StorageError::InvalidCommand);
        }
        let issued = self.account_proof_context(&request, actor)?;
        prepared_account_creation_proof(change, request, actor, issued)
    }
    pub fn prepare_account_update_receipt(
        &self,
        request: &IdentityRequest,
        actor: &ActorContext,
    ) -> Result<AccountReceiptProof, StorageError> {
        if !matches!(request.command, IdentityCommand::UpdateAccount { .. }) {
            return Err(StorageError::InvalidCommand);
        }
        let issued = self.account_proof_context(request, actor)?;
        let fingerprint = identity_fingerprint_seed(request, Some(actor), None)?
            .finalize()
            .into();
        Ok(AccountReceiptProof {
            issued,
            request: request.clone(),
            fingerprint,
        })
    }
    /// Bind the original verified transfer without changing either credential
    /// record. The empty prepared payload is consumed once by execute_credentials.
    pub fn prepare_account_transfer_receipt(
        &self,
        change: PreparedCredentialChange<Vec<u8>, Vec<u8>>,
        actor: &ActorContext,
    ) -> Result<PreparedAccountWithReceipt, StorageError> {
        let request = change.request().clone();
        let IdentityCommand::TransferAdministrator { from, .. } = request.command else {
            return Err(StorageError::InvalidCommand);
        };
        let issued = self.account_proof_context(&request, actor)?;
        let change = change.try_map(
            |_| -> Result<Vec<u8>, StorageError> { Err(StorageError::InvalidCommand) },
            |_| -> Result<Vec<u8>, StorageError> { Err(StorageError::InvalidCommand) },
        )?;
        let mut digest = identity_fingerprint_seed(&request, Some(actor), None)?;
        digest.update(from.get().to_be_bytes());
        digest.update([0, 0]);
        Ok((
            change,
            AccountReceiptProof {
                issued,
                request,
                fingerprint: digest.finalize().into(),
            },
        ))
    }
    /// Only query the original identity receipt. Missing means unknown, never
    /// execute/write. The exact lease and latest management authority are checked
    /// in this read-only Writer transaction and again by the composition.
    pub fn account_outcome(
        &self,
        proof: Arc<AccountReceiptProof>,
        actor: &ActorContext,
    ) -> Result<Option<OriginalAccountResult>, IdentityRepositoryError> {
        self.read_account_outcome(proof, actor, self.session.clone())
    }
    /// Only the original handover's read-only task may use a fresh lease after
    /// self-revocation. Keep the ordinary owner-bound mutation guard unchanged.
    pub fn administrator_transfer_outcome(
        &self,
        proof: Arc<AccountReceiptProof>,
        actor: &ActorContext,
        fresh_session: SessionGuard,
    ) -> Result<Option<OriginalAccountResult>, IdentityRepositoryError> {
        if !matches!(
            proof.request.command,
            IdentityCommand::TransferAdministrator { .. }
        ) {
            return Err(identity_error(StorageError::InvalidCommand));
        }
        self.read_account_outcome(proof, actor, fresh_session)
    }
    fn read_account_outcome(
        &self,
        proof: Arc<AccountReceiptProof>,
        actor: &ActorContext,
        session: SessionGuard,
    ) -> Result<Option<OriginalAccountResult>, IdentityRepositoryError> {
        let issued = if matches!(
            proof.request.command,
            IdentityCommand::TransferAdministrator { .. }
        ) {
            self.handle
                .command_window
                .ok_or_else(|| identity_error(StorageError::CommandNotIssued))?
        } else {
            self.account_proof_context(&proof.request, actor)
                .map_err(identity_error)?
        };
        if issued != proof.issued {
            return Err(identity_error(StorageError::CommandNotIssued));
        }
        let bytes = self
            .handle
            .try_transaction(AccountOutcomeTask {
                proof,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session,
            })
            .map_err(identity_error)?
            .wait()
            .map_err(identity_error)?;
        let value = codec::value(&bytes).map_err(identity_error)?;
        if value.is_null() {
            Ok(None)
        } else {
            read_account_result(&value)
                .map(Some)
                .map_err(identity_error)
        }
    }
}
fn check_account_proof_binding(
    issued: fc_ports::command_window::IssuedCommand,
    request: &IdentityRequest,
    actor: &ActorContext,
) -> Result<(), StorageError> {
    use fc_ports::command_window::{CommandFamily, CommandOwner};
    if issued.family != CommandFamily::Identity
        || issued.command_id != request.command_id
        || issued.owner != CommandOwner::authenticated(actor)
        || actor.origin != TrustedOrigin::Local
        || !matches!(
            request.command,
            IdentityCommand::CreateAccount(_)
                | IdentityCommand::UpdateAccount { .. }
                | IdentityCommand::TransferAdministrator { .. }
        )
    {
        return Err(StorageError::CommandNotIssued);
    }
    Ok(())
}
fn prepared_account_creation_proof(
    change: PreparedCredentialChange<Vec<u8>, Vec<u8>>,
    request: IdentityRequest,
    actor: &ActorContext,
    issued: fc_ports::command_window::IssuedCommand,
) -> Result<PreparedAccountWithReceipt, StorageError> {
    check_account_proof_binding(issued, &request, actor)?;
    if change.request() != &request {
        return Err(StorageError::InvalidCommand);
    }
    let IdentityCommand::CreateAccount(spec) = &request.command else {
        return Err(StorageError::InvalidCommand);
    };
    let mut seed = identity_fingerprint_seed(&request, Some(actor), None)?;
    seed.update(spec.id.get().to_be_bytes());
    let digest = std::cell::RefCell::new(seed);
    let password_seen = std::cell::Cell::new(false);
    let recovery_seen = std::cell::Cell::new(false);
    let change = change.try_map(
        |record| -> Result<_, StorageError> {
            fingerprint_record(&mut digest.borrow_mut(), 1, &record);
            password_seen.set(true);
            Ok(record)
        },
        |record| -> Result<_, StorageError> {
            fingerprint_record(&mut digest.borrow_mut(), 1, &record);
            recovery_seen.set(true);
            Ok(record)
        },
    )?;
    if !password_seen.get() || !recovery_seen.get() {
        return Err(StorageError::InvalidCommand);
    }
    let fingerprint = digest.into_inner().finalize().into();
    Ok((
        change,
        AccountReceiptProof {
            issued,
            request,
            fingerprint,
        },
    ))
}
struct AccountOutcomeTask {
    proof: Arc<AccountReceiptProof>,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for AccountOutcomeTask {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((
            fc_ports::command_window::CommandFamily::Identity,
            self.proof.request.command_id,
        ))
    }
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>().saturating_add(account_request_bytes(&self.proof.request))
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let state = load_identity(d)?;
        let check = || -> Result<(), StorageError> {
            if let IdentityCommand::TransferAdministrator {
                from,
                expected_credential_version,
                ..
            } = &self.proof.request.command
            {
                use fc_ports::command_window::{CommandFamily, CommandOwner};
                let CommandOwner::Principal {
                    principal,
                    school,
                    epoch,
                    origin,
                } = self.proof.issued.owner
                else {
                    return Err(StorageError::CommandNotIssued);
                };
                let account = state.account(*from).map_err(StorageError::Identity)?;
                if self.proof.issued.family != CommandFamily::Identity
                    || self.proof.issued.command_id != self.proof.request.command_id
                    || principal != *from
                    || principal != self.actor.principal_id
                    || school != self.actor.school_id
                    || origin != TrustedOrigin::Local
                    || self.actor.origin != TrustedOrigin::Local
                    || (self.actor.auth_epoch != epoch
                        && epoch.next().ok() != Some(self.actor.auth_epoch))
                    || account.must_change_password
                    || account.credential_version != *expected_credential_version
                {
                    return Err(StorageError::CommandNotIssued);
                }
                (self.session)(&state, &self.actor)?;
                return state
                    .authority(*from)
                    .map_err(StorageError::Identity)?
                    .validate_actor(&self.actor, self.clock.now())
                    .map_err(|e| StorageError::Identity(IdentityError::Permission(e)));
            }
            if self.proof.issued.owner
                != fc_ports::command_window::CommandOwner::authenticated(&self.actor)
            {
                return Err(StorageError::CommandNotIssued);
            }
            (self.session)(&state, &self.actor)?;
            state
                .authorize_management(&self.actor, self.clock.now())
                .map_err(StorageError::Identity)
        };
        check()?;
        let request = &self.proof.request;
        let value = match d.get(&format!(
            "business/receipt/identity/{}",
            request.command_id.get()
        ))? {
            None => Value::Null,
            Some(receipt) => {
                let v = codec::value(&receipt.value)?;
                codec::object_fields(&v, &["fingerprint", "outcome"])?;
                codec::object_fields(&v["outcome"], &["state", "events"])?;
                let original: Vec<u8> = serde_json::from_value(v["fingerprint"].clone())
                    .map_err(|_| StorageError::Database)?;
                if original.len() != 32 {
                    return Err(StorageError::Database);
                }
                if original.as_slice() != self.proof.fingerprint.as_slice() {
                    return Err(StorageError::CommandIdReused);
                }
                let historical = codec::read_identity(&v["outcome"]["state"])?;
                let (id, expected_events) = match &request.command {
                    IdentityCommand::CreateAccount(a) => {
                        (a.id, vec![IdentityEvent::AccountChanged(a.id)])
                    }
                    IdentityCommand::UpdateAccount { id, .. } => (
                        *id,
                        vec![
                            IdentityEvent::AuthorizationRevoked(*id),
                            IdentityEvent::AccountChanged(*id),
                        ],
                    ),
                    IdentityCommand::TransferAdministrator {
                        from,
                        target,
                        expected_credential_version,
                    } => {
                        let original = historical
                            .account(*from)
                            .map_err(|_| StorageError::Database)?;
                        let fc_ports::command_window::CommandOwner::Principal { epoch, .. } =
                            self.proof.issued.owner
                        else {
                            return Err(StorageError::Database);
                        };
                        let teaching = historical.assignments.iter().any(|a| a.teacher_id == *from);
                        if original.principal.role
                            != if teaching {
                                Role::TeacherAdmin
                            } else {
                                Role::User
                            }
                            || !original.principal.enabled
                            || original.must_change_password
                            || original.credential_version != *expected_credential_version
                            || epoch.next().ok() != Some(original.principal.auth_epoch)
                        {
                            return Err(StorageError::Database);
                        }
                        (
                            *target,
                            vec![
                                IdentityEvent::AuthorizationRevoked(*from),
                                IdentityEvent::AuthorizationRevoked(*target),
                                IdentityEvent::AccountChanged(*from),
                                IdentityEvent::AccountChanged(*target),
                            ],
                        )
                    }
                    _ => return Err(StorageError::InvalidCommand),
                };
                if decode_events(&v["outcome"]["events"])? != expected_events {
                    return Err(StorageError::Database);
                }
                let account = historical
                    .account(id)
                    .map_err(|_| StorageError::Database)?
                    .clone();
                let scopes = historical
                    .assignments
                    .iter()
                    .filter(|a| a.teacher_id == id)
                    .map(|a| {
                        if !a.enabled
                            && !matches!(
                                request.command,
                                IdentityCommand::TransferAdministrator { .. }
                            )
                        {
                            return Err(StorageError::Database);
                        }
                        Ok(AssignmentScope {
                            class_id: a.class_id,
                            subject_id: a.subject_id,
                        })
                    })
                    .collect::<Result<Vec<_>, StorageError>>()?;
                let result = OriginalAccountResult {
                    revision: historical.revision,
                    account,
                    scopes,
                };
                if result.account.principal.school_id != self.actor.school_id
                    || !result.matches(request.expected_revision, &request.command)
                {
                    return Err(StorageError::Database);
                }
                account_result_value(&result)
            }
        };
        check()?;
        let outcome = codec::bytes(&value)?;
        if outcome.len() > 64 * 1024 {
            return Err(StorageError::DocumentTooLarge);
        }
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}

#[cfg(test)]
mod account_receipt_proof_tests {
    use super::*;
    use fc_ports::command_window::{CommandFamily, CommandOwner, CommandTicket, IssuedCommand};
    // Verify original credential-payload ownership and receipt fingerprints.
    struct FakeCrypto(u8);
    impl fc_ports::CredentialCrypto for FakeCrypto {
        type Secret = ();
        type PasswordRecord = Vec<u8>;
        type RecoveryCode = ();
        type RecoveryRecord = Vec<u8>;
        fn password_record(&mut self, _: &()) -> Result<Vec<u8>, fc_ports::CredentialError> {
            self.0 += 1;
            Ok(vec![self.0; 17])
        }
        fn verify_password(
            &mut self,
            _: &(),
            _: &Vec<u8>,
        ) -> Result<bool, fc_ports::CredentialError> {
            Ok(false)
        }
        fn recovery_record(&mut self) -> Result<((), Vec<u8>), fc_ports::CredentialError> {
            Ok(((), vec![self.0 + 100; 19]))
        }
        fn verify_recovery(
            &mut self,
            _: &(),
            _: &Vec<u8>,
        ) -> Result<bool, fc_ports::CredentialError> {
            Ok(false)
        }
    }
    fn request() -> IdentityRequest {
        IdentityRequest {
            command_id: CommandId::new(9).unwrap(),
            expected_revision: Revision::new(12),
            command: IdentityCommand::CreateAccount(NewAccount {
                id: PrincipalId::new(4).unwrap(),
                login_name: "teacher4".into(),
                display_name: "原姓名".into(),
                role: Role::TeacherAdmin,
                scopes: vec![AssignmentScope {
                    class_id: ClassId::new(2).unwrap(),
                    subject_id: SubjectId::new(3).unwrap(),
                }],
            }),
        }
    }
    fn actor() -> ActorContext {
        ActorContext {
            principal_id: PrincipalId::new(1).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            auth_epoch: AuthEpoch::new(7),
            origin: TrustedOrigin::Local,
            expires_at: fc_domain::time::LocalDateTime::new(
                fc_domain::time::LocalDate::new(2026, 11, 3).unwrap(),
                3600,
            )
            .unwrap(),
        }
    }
    fn issued(request: &IdentityRequest, actor: &ActorContext) -> IssuedCommand {
        IssuedCommand {
            ticket: CommandTicket::new([7; 16], 4).unwrap(),
            command_id: request.command_id,
            family: CommandFamily::Identity,
            owner: CommandOwner::authenticated(actor),
            window_millis: 86_400_000,
        }
    }
    #[test]
    fn proof_hashes_the_same_owned_records_handed_to_writer_without_preparing_again() {
        let request = request();
        let actor = actor();
        let issued = issued(&request, &actor);
        let mut crypto = FakeCrypto(0);
        let (change, _) =
            fc_ports::prepare_initial_credentials(&mut crypto, &(), request.clone()).unwrap();
        let password_ptr = std::cell::Cell::new(std::ptr::null());
        let recovery_ptr = std::cell::Cell::new(std::ptr::null());
        let change = change
            .try_map(
                |v| {
                    password_ptr.set(v.as_ptr());
                    Ok::<_, StorageError>(v)
                },
                |v| {
                    recovery_ptr.set(v.as_ptr());
                    Ok::<_, StorageError>(v)
                },
            )
            .unwrap();
        let (change, proof) =
            prepared_account_creation_proof(change, request.clone(), &actor, issued).unwrap();
        assert_eq!(crypto.0, 1, "metadata handoff must not rerun preparation");
        let (actual, password, recovery) = change.into_parts();
        let password = password.unwrap();
        let recovery = recovery.unwrap();
        assert_eq!(password.as_ptr(), password_ptr.get());
        assert_eq!(recovery.as_ptr(), recovery_ptr.get());
        assert_eq!(actual, request);
        assert_eq!(proof.issued(), issued);
        let credentials = PreparedCredentials {
            principal_id: PrincipalId::new(4).unwrap(),
            password: Some((1, password)),
            recovery: Some((1, recovery)),
        };
        assert_eq!(
            proof.fingerprint.as_slice(),
            identity_fingerprint(&actual, Some(&actor), None, Some(&credentials))
                .unwrap()
                .as_slice()
        );
        let (other, _) =
            fc_ports::prepare_initial_credentials(&mut crypto, &(), request.clone()).unwrap();
        let (_, other_proof) =
            prepared_account_creation_proof(other, request, &actor, issued).unwrap();
        assert_ne!(
            proof.fingerprint, other_proof.fingerprint,
            "new randomized records cannot replace the original proof"
        );
    }
    #[test]
    fn proof_owner_binding_ignores_expiry_metadata_but_rejects_epoch_or_family_changes() {
        let request = request();
        let actor = actor();
        let original = issued(&request, &actor);
        let mut later_lease = actor.clone();
        later_lease.expires_at =
            fc_domain::time::LocalDateTime::new(actor.expires_at.date, 7200).unwrap();
        assert!(check_account_proof_binding(original, &request, &later_lease).is_ok());
        later_lease.auth_epoch = AuthEpoch::new(8);
        assert_eq!(
            check_account_proof_binding(original, &request, &later_lease),
            Err(StorageError::CommandNotIssued)
        );
        let mut wrong = original;
        wrong.family = CommandFamily::Display;
        assert_eq!(
            check_account_proof_binding(wrong, &request, &actor),
            Err(StorageError::CommandNotIssued)
        );
        let mut request2 = request;
        request2.command_id = CommandId::new(10).unwrap();
        assert_eq!(
            check_account_proof_binding(original, &request2, &actor),
            Err(StorageError::CommandNotIssued)
        );
    }
    #[test]
    fn extracted_historical_account_is_compared_to_original_not_current_directory() {
        let request = request();
        let IdentityCommand::CreateAccount(spec) = &request.command else {
            unreachable!()
        };
        let original = OriginalAccountResult {
            revision: Revision::new(13),
            account: UserAccount {
                principal: Principal {
                    id: spec.id,
                    school_id: SchoolId::new(1).unwrap(),
                    role: spec.role,
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: spec.login_name.clone(),
                display_name: spec.display_name.clone(),
                must_change_password: true,
                credential_version: 1,
                recovery_version: 1,
            },
            scopes: spec.scopes.clone(),
        };
        assert!(original.matches(request.expected_revision, &request.command));
        let mut later = original.clone();
        later.account.display_name = "后来修改".into();
        later.account.credential_version = 2;
        assert!(!later.matches(request.expected_revision, &request.command));
        assert_eq!(
            read_account_result(&account_result_value(&original)).unwrap(),
            original
        );
    }
    #[test]
    fn shared_fingerprint_preserves_legacy_byte_order_and_initial_installation_binding() {
        let request = request();
        let actor = actor();
        let records = PreparedCredentials {
            principal_id: PrincipalId::new(4).unwrap(),
            password: Some((1, vec![1, 2, 3])),
            recovery: Some((1, vec![4, 5, 6])),
        };
        // Literal old grammar, independent of the new seed/record helper.
        let mut old = Sha256::new();
        old.update(format!("{:?}", request));
        old.update(codec::bytes(&codec::actor(&actor)).unwrap());
        old.update(4_u64.to_be_bytes());
        for record in [[1_u8, 2, 3], [4_u8, 5, 6]] {
            old.update(1_u64.to_be_bytes());
            old.update(3_u64.to_be_bytes());
            old.update(record);
        }
        assert_eq!(
            old.finalize().to_vec(),
            identity_fingerprint(&request, Some(&actor), None, Some(&records)).unwrap()
        );
        let first = IdentityRequest {
            command_id: request.command_id,
            expected_revision: Revision::default(),
            command: IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "学校".into(),
                id: PrincipalId::new(1).unwrap(),
                login_name: "admin".into(),
                display_name: "管理员".into(),
            },
        };
        let mut old = Sha256::new();
        old.update(format!("{:?}", first));
        old.update(b"installation/v1\0");
        old.update([9; 32]);
        old.update(codec::bytes(&Value::Null).unwrap());
        assert_eq!(
            old.finalize().to_vec(),
            identity_fingerprint(&first, None, Some([9; 32]), None).unwrap()
        );
    }
}
