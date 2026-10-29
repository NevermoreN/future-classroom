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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasureError {
    Unmeasurable,
}
pub trait TextMeasure {
    fn height(&self, text: &str, width_dip: f32, font_dip: f32) -> Result<f32, MeasureError>;
}
