//! Typed business repositories on the existing SQLite writer. The synchronous
//! facade is for application workers; native/HTTP event loops poll tickets.
pub(crate) mod codec;
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
    transaction.rollback().map_err(|_| StorageError::Database)?;
    Ok(StartupState { identity, revision })
}

const IDENTITY: &str = "business/identity/v1";
/// Bind an exact authenticated channel lease in this trusted memory-only callback.
/// It must revalidate monotonic idle/expiry/local lock without refreshing the lease.
pub type SessionGuard =
    Arc<dyn Fn(&IdentityState, &ActorContext) -> Result<(), StorageError> + Send + Sync>;
type Factory<S> = Arc<dyn Fn(&IdentityState, Revision) -> Result<S, StorageError> + Send + Sync>;
/// Factory must only prepare an owned immutable authority snapshot: no writer
/// reentry, file access, KDF, channel effects, or externally visible publication.
pub struct BusinessRepository<S> {
    handle: StorageHandle<S>,
    clock: Arc<dyn Clock + Send + Sync>,
    factory: Factory<S>,
    session: SessionGuard,
}
impl<S> Clone for BusinessRepository<S> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            clock: self.clock.clone(),
            factory: self.factory.clone(),
            session: self.session.clone(),
        }
    }
}
impl<S: Send + 'static> BusinessRepository<S> {
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
        }
    }
    /// Authenticated operations fail closed until composition binds their exact lease.
    pub fn with_session_guard(mut self, guard: SessionGuard) -> Self {
        self.session = guard;
        self
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
        let actual_date = match &self.key {
            ContentKey::Homework(k) => {
                a.authorize_owned_content(
                    &self.actor,
                    now,
                    k.class_id,
                    k.subject_id,
                    k.owner_teacher_id,
                )
                .map_err(|e| StorageError::Content(ContentError::Permission(e)))?;
                k.actual_date
            }
            ContentKey::SharedReminder(k) => {
                a.authorize_shared_reminder(&self.actor, now, k.class_id)
                    .map_err(|e| StorageError::Content(ContentError::Permission(e)))?;
                k.actual_date
            }
        };
        if actual_date != now.date {
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
                (self.session)(&state, &self.actor)?;
                a.validate_actor(&self.actor, self.clock.now())
                    .map_err(|e| StorageError::Content(ContentError::Permission(e)))?;
                if record.visible_text(self.clock.now().date).is_some() {
                    v
                } else {
                    Value::Null
                }
            }
            None => Value::Null,
        };
        Ok(TransactionDecision {
            precommit: None,
            outcome: codec::bytes(&value)?,
            commit: None,
        })
    }
}
struct ContentTask<S> {
    command: ContentCommand,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    factory: Factory<S>,
    session: SessionGuard,
}
impl<S: Send> TransactionTask<S> for ContentTask<S> {
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
            let record = codec::read_record(&v["record"])?;
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
            self.command
                .validate(&a, &self.actor, self.clock.now())
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
        let session = self.session.clone();
        let actor = self.actor.clone();
        let clock = self.clock.clone();
        let command = self.command.clone();
        Ok(TransactionDecision {
            precommit: Some(Box::new(move || {
                session(&state, &actor)?;
                command
                    .validate(&a, &actor, clock.now())
                    .map_err(StorageError::Content)
            })),
            outcome: outcome.clone(),
            commit: Some((
                vec![
                    Mutation::Put {
                        key,
                        value: outcome,
                    },
                    Mutation::Put {
                        key: journal,
                        value: receipt,
                    },
                ],
                snapshot,
            )),
        })
    }
}
fn repository_error(e: StorageError) -> RepositoryError {
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
    match e {
        StorageError::Identity(e) => IdentityRepositoryError::Domain(e),
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
struct IdentityTask<S> {
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
        if let Some(actor) = &self.actor {
            (self.session)(&state, actor)?;
        }
        let request = &self.request;
        let journal = format!("business/receipt/identity/{}", request.command_id.get());
        let mut digest = Sha256::new();
        digest.update(format!("{:?}", request));
        digest.update(codec::bytes(
            &self.actor.as_ref().map(codec::actor).unwrap_or(Value::Null),
        )?);
        if let Some(c) = &self.credentials {
            digest.update(c.principal_id.get().to_be_bytes());
            for pair in [&c.password, &c.recovery] {
                if let Some((version, record)) = pair {
                    digest.update(version.to_be_bytes());
                    digest.update((record.len() as u64).to_be_bytes());
                    digest.update(record);
                } else {
                    digest.update([0]);
                }
            }
        }
        let fingerprint = digest.finalize().to_vec();
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
        let mut mutations = Vec::new();
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
        mutations.push(Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&transition.next))?,
        });
        mutations.push(Mutation::Put {
            key: journal,
            value: codec::bytes(&json!({"fingerprint":fingerprint,"outcome":outcome}))?,
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
