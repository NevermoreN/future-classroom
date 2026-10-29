#![forbid(unsafe_code)]
use fc_domain::ids::Revision;
use fc_domain::{
    auth::{ActorContext, AuthorityContext},
    content::{ContentCommand, ContentError, ContentRecord},
    time::LocalDateTime,
};

pub trait Clock {
    fn now(&self) -> LocalDateTime;
}
/// Process-local monotonic milliseconds. Never reconstruct live leases after restart.
pub trait MonotonicClock {
    fn monotonic_millis(&self) -> u64;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityRequest {
    pub command_id: fc_domain::ids::CommandId,
    pub expected_revision: Revision,
    pub command: fc_domain::identity::IdentityCommand,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityRepositoryError {
    Domain(fc_domain::identity::IdentityError),
    Unavailable,
    Faulted,
    IdempotencyConflict,
}
/// Implementations execute on the sole writer, re-read state/authentication lease at
/// commit, compare expected_revision, and persist outcome and state atomically. Credential
/// commands MUST use a separate verified-credential path; this plain interface must refuse
/// bootstrap/account creation without prepared credentials and all password/recovery changes.
pub trait IdentityRepository {
    fn state(&self) -> Result<fc_domain::identity::IdentityState, IdentityRepositoryError>;
    fn execute_identity(
        &mut self,
        request: IdentityRequest,
        actor: Option<&ActorContext>,
        clock: &dyn Clock,
    ) -> Result<fc_domain::identity::IdentityTransition, IdentityRepositoryError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialError {
    InvalidSecret,
    VerificationFailed,
    Unavailable,
    StaleCredential,
}
/// Cryptographic operations run on the bounded worker pool, never on the UI/writer.
/// Associated records and secrets should be owned, redacted, wiped on drop and non-Clone.
/// Recovery codes must contain at least 128 unpredictable bits; records store only a
/// one-way verifier, and verify must use constant-time comparison.
pub trait CredentialCrypto {
    type Secret;
    type PasswordRecord;
    type RecoveryCode;
    type RecoveryRecord;
    fn password_record(
        &mut self,
        secret: &Self::Secret,
    ) -> Result<Self::PasswordRecord, CredentialError>;
    fn verify_password(
        &mut self,
        secret: &Self::Secret,
        record: &Self::PasswordRecord,
    ) -> Result<bool, CredentialError>;
    fn recovery_record(
        &mut self,
    ) -> Result<(Self::RecoveryCode, Self::RecoveryRecord), CredentialError>;
    fn verify_recovery(
        &mut self,
        code: &Self::RecoveryCode,
        record: &Self::RecoveryRecord,
    ) -> Result<bool, CredentialError>;
}

/// Created only by the verification helpers below using a trusted cryptographic port.
/// No public constructor or mutation permits replacing its version-bound request after
/// password/recovery verification. It is never Clone, Debug, or a published snapshot.
pub struct PreparedCredentialChange<P, R> {
    request: IdentityRequest,
    password: Option<P>,
    recovery: Option<R>,
}
impl<P, R> PreparedCredentialChange<P, R> {
    pub fn request(&self) -> &IdentityRequest {
        &self.request
    }
    pub fn into_parts(self) -> (IdentityRequest, Option<P>, Option<R>) {
        (self.request, self.password, self.recovery)
    }
    /// Trusted composition may encode owned protected records for the storage adapter;
    /// the immutable command/version binding is preserved through the conversion.
    pub fn try_map<P2, R2, E>(
        self,
        password: impl FnOnce(P) -> Result<P2, E>,
        recovery: impl FnOnce(R) -> Result<R2, E>,
    ) -> Result<PreparedCredentialChange<P2, R2>, E> {
        Ok(PreparedCredentialChange {
            request: self.request,
            password: self.password.map(password).transpose()?,
            recovery: self.recovery.map(recovery).transpose()?,
        })
    }
}
pub type CredentialPreparation<C> = (
    PreparedCredentialChange<
        <C as CredentialCrypto>::PasswordRecord,
        <C as CredentialCrypto>::RecoveryRecord,
    >,
    <C as CredentialCrypto>::RecoveryCode,
);
pub fn prepare_initial_credentials<C: CredentialCrypto>(
    crypto: &mut C,
    secret: &C::Secret,
    request: IdentityRequest,
) -> Result<CredentialPreparation<C>, CredentialError> {
    if !matches!(
        request.command,
        fc_domain::identity::IdentityCommand::FirstAdministrator { .. }
            | fc_domain::identity::IdentityCommand::CreateAccount(_)
    ) {
        return Err(CredentialError::InvalidSecret);
    }
    let password = crypto.password_record(secret)?;
    let (code, recovery) = crypto.recovery_record()?;
    Ok((
        PreparedCredentialChange {
            request,
            password: Some(password),
            recovery: Some(recovery),
        },
        code,
    ))
}
pub fn prepare_password_change<C: CredentialCrypto>(
    crypto: &mut C,
    old: &C::Secret,
    new: &C::Secret,
    existing: &C::PasswordRecord,
    request: IdentityRequest,
) -> Result<PreparedCredentialChange<C::PasswordRecord, C::RecoveryRecord>, CredentialError> {
    if !matches!(
        request.command,
        fc_domain::identity::IdentityCommand::PasswordChanged { .. }
    ) {
        return Err(CredentialError::InvalidSecret);
    }
    if !crypto.verify_password(old, existing)? {
        return Err(CredentialError::VerificationFailed);
    }
    Ok(PreparedCredentialChange {
        request,
        password: Some(crypto.password_record(new)?),
        recovery: None,
    })
}
pub fn prepare_recovery<C: CredentialCrypto>(
    crypto: &mut C,
    code: &C::RecoveryCode,
    new: &C::Secret,
    existing: &C::RecoveryRecord,
    request: IdentityRequest,
) -> Result<CredentialPreparation<C>, CredentialError> {
    if !matches!(
        request.command,
        fc_domain::identity::IdentityCommand::RecoveryConsumed { .. }
    ) {
        return Err(CredentialError::InvalidSecret);
    }
    if !crypto.verify_recovery(code, existing)? {
        return Err(CredentialError::VerificationFailed);
    }
    let password = crypto.password_record(new)?;
    let (code, recovery) = crypto.recovery_record()?;
    Ok((
        PreparedCredentialChange {
            request,
            password: Some(password),
            recovery: Some(recovery),
        },
        code,
    ))
}
pub fn prepare_recovery_rotation<C: CredentialCrypto>(
    crypto: &mut C,
    password: &C::Secret,
    existing: &C::PasswordRecord,
    request: IdentityRequest,
) -> Result<CredentialPreparation<C>, CredentialError> {
    if !matches!(
        request.command,
        fc_domain::identity::IdentityCommand::RecoveryRotated { .. }
    ) {
        return Err(CredentialError::InvalidSecret);
    }
    if !crypto.verify_password(password, existing)? {
        return Err(CredentialError::VerificationFailed);
    }
    let (code, recovery) = crypto.recovery_record()?;
    Ok((
        PreparedCredentialChange {
            request,
            password: None,
            recovery: Some(recovery),
        },
        code,
    ))
}
/// Implement alongside IdentityRepository. Read the existing protected record at the
/// captured version before KDF preparation. At commit revalidate lease/epoch/CAS and both
/// old record versions, then atomically store new credential records, state and outcome.
/// Reveal a new recovery code only after success; never include it in ordinary snapshots.
pub trait CredentialIdentityRepository<P, R>: IdentityRepository {
    fn execute_credentials(
        &mut self,
        change: PreparedCredentialChange<P, R>,
        actor: Option<&ActorContext>,
        clock: &dyn Clock,
    ) -> Result<fc_domain::identity::IdentityTransition, IdentityRepositoryError>;
}
/// Production implementation must use the operating system CSPRNG.
pub trait RandomSource {
    type Error;
    fn fill(&mut self, output: &mut [u8]) -> Result<(), Self::Error>;
}
pub use RandomSource as RandomPort;

/// Writer-only publication capability. Construct and validate Snapshot before begin_update.
/// Never hold the arbitration lock across database or OS calls. A failed/panicked publish
/// after COMMIT, or an unknown commit outcome, MUST call fault before serving any old state.
pub trait AuthorityPublisher {
    type Snapshot;
    type Update;
    type Error;
    fn snapshot_revision(&self, snapshot: &Self::Snapshot) -> Revision;
    fn begin_update(&mut self) -> Result<Self::Update, Self::Error>;
    fn publish(
        &mut self,
        update: Self::Update,
        snapshot: Self::Snapshot,
    ) -> Result<(), Self::Error>;
    /// Only call after proving database and authority are still the old committed state.
    fn rollback(&mut self, update: Self::Update) -> Result<(), Self::Error>;
    fn fault(&mut self);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepositoryError {
    Domain(ContentError),
    Unavailable,
    Faulted,
}

/// execute must atomically re-read authority, check health/auth/expiry/date/CAS and persist
/// the row and command outcome. clock.now() must be read at the transaction commit check.
/// Same command ID + actor + payload returns the original outcome; a changed payload rejects.
/// No callback may reenter the single writer. This trait does not imply durable storage.
pub trait ContentRepository {
    fn authority(&self, actor: &ActorContext) -> Result<AuthorityContext, RepositoryError>;
    fn execute(
        &mut self,
        command: ContentCommand,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<ContentRecord, RepositoryError>;
}
pub use ContentRepository as Repository;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleRequest {
    pub command_id: fc_domain::ids::CommandId,
    pub expected_revision: Revision,
    pub candidate: fc_domain::schedule_document::ScheduleDocument,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleRepositoryError {
    Domain(fc_domain::schedule_document::ScheduleDocumentError),
    Unavailable,
    Faulted,
    IdempotencyConflict,
    DocumentTooLarge,
}
/// Complete native-grid draft publication. Re-read the exact local/global lease,
/// identity references and independent document CAS on the sole SQLite writer;
/// execute domain transition to derive revisions, persist state/outcome atomically,
/// and publish authority under the shared health gate. The explicit 64 KiB encoded
/// document limit is separate from domain element limits; never truncate a draft.
pub trait ScheduleRepository {
    fn schedule(
        &self,
        actor: &ActorContext,
    ) -> Result<fc_domain::schedule_document::ScheduleDocument, ScheduleRepositoryError>;
    fn execute_schedule(
        &mut self,
        request: ScheduleRequest,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<fc_domain::schedule_document::ScheduleDocument, ScheduleRepositoryError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasureError {
    Unmeasurable,
}
pub trait TextMeasure {
    fn height(&self, text: &str, width_dip: f32, font_dip: f32) -> Result<f32, MeasureError>;
}
