//! Non-active public-calendar cache and narrowly authorized adoption capabilities.
use fc_domain::{
    auth::ActorContext,
    calendar::CalendarCandidate,
    ids::{AuthEpoch, CommandId, PrincipalId, Revision},
    schedule_document::ScheduleDocument,
    time::LocalDateTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarPolicyGrant {
    pub principal_id: PrincipalId,
    pub auth_epoch: AuthEpoch,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CalendarAutoPolicy {
    pub revision: Revision,
    pub enabled: bool,
    pub grant: Option<CalendarPolicyGrant>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarFetchFailure {
    Cancelled,
    Network,
    Tls,
    Http,
    InvalidData,
    TooLarge,
    Deadline,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarYearSummary {
    pub year: i32,
    pub candidate_revision: Revision,
    pub last_attempt: Option<LocalDateTime>,
    pub next_allowed: Option<LocalDateTime>,
    pub last_success: Option<LocalDateTime>,
    pub last_error: Option<CalendarFetchFailure>,
    pub pending: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarStatus {
    pub policy: CalendarAutoPolicy,
    pub years: Vec<CalendarYearSummary>,
    pub selected_year: i32,
    pub candidate: Option<CalendarCandidate>,
}
/// Exact persisted attempt. This is trusted composition data, never an HTTP DTO.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarFetchTicket {
    pub year: i32,
    pub attempt_sequence: u64,
    pub expected_candidate_revision: Revision,
    pub captured_mode_revision: Revision,
    pub captured_policy_revision: Revision,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalendarReservation {
    Reserved(CalendarFetchTicket),
    RateLimited { next_allowed: LocalDateTime },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalendarFetchOutcome {
    Downloaded(CalendarCandidate),
    NotModified,
    Failed(CalendarFetchFailure),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarCandidateRef {
    pub year: i32,
    pub revision: Revision,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarAdoptionRequest {
    pub command_id: CommandId,
    pub expected_schedule_revision: Revision,
    pub candidates: Vec<CalendarCandidateRef>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalendarRepositoryError {
    Cancelled,
    Invalid,
    Denied,
    Capacity,
    Stale,
    Unavailable,
    Faulted,
}
/// All operations run on the existing sole writer. Only finish/background are
/// capability/grant based; UI reads, reserve, adoption and policy require an exact
/// current local global-administrator session through the real COMMIT boundary.
pub trait CalendarRepository {
    fn calendar_status(
        &self,
        year: i32,
        actor: &ActorContext,
    ) -> Result<CalendarStatus, CalendarRepositoryError>;
    fn reserve_calendar_fetch(
        &mut self,
        year: i32,
        actor: &ActorContext,
    ) -> Result<CalendarReservation, CalendarRepositoryError>;
    fn calendar_background_fetch(
        &mut self,
    ) -> Result<Option<CalendarFetchTicket>, CalendarRepositoryError>;
    fn finish_calendar_fetch(
        &mut self,
        ticket: CalendarFetchTicket,
        outcome: CalendarFetchOutcome,
    ) -> Result<CalendarStatus, CalendarRepositoryError>;
    fn adopt_calendar(
        &mut self,
        request: CalendarAdoptionRequest,
        actor: &ActorContext,
    ) -> Result<ScheduleDocument, CalendarRepositoryError>;
    fn set_calendar_auto_policy(
        &mut self,
        expected_revision: Revision,
        enabled: bool,
        actor: &ActorContext,
    ) -> Result<CalendarAutoPolicy, CalendarRepositoryError>;
}

/// Conditional request hints from the exact persisted candidate revision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CalendarFetchValidators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalendarProviderResponse {
    Downloaded(CalendarCandidate),
    NotModified,
}
/// Replaceable, allowlisted candidate-source adapter. Implementations bound the
/// complete response and physical request count, validate TLS and source schema,
/// and observe cancellation. This capability never adopts or edits a schedule.
pub trait CalendarProvider: Send + Sync {
    fn fetch(
        &self,
        year: i32,
        validators: Option<&CalendarFetchValidators>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<CalendarProviderResponse, CalendarFetchFailure>;
}
