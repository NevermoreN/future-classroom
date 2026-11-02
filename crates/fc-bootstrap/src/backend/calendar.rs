//! Exact local management and independently constrained durable-calendar grants.
//! Network I/O belongs to the shared worker after this facade releases its mutex.
use super::*;
use fc_ports::calendar::*;

impl Backend {
    pub fn calendar_status(&self, lease: SessionLease, year: i32) -> Result<CalendarStatus> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .calendar_status(year, &actor)?)
    }
    pub fn calendar_background_fetch(&mut self) -> Result<Option<CalendarFetchTicket>> {
        self.calendar_background_fetch_while(Arc::new(|| true))
    }
    pub fn calendar_background_fetch_while(
        &mut self,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Option<CalendarFetchTicket>> {
        self.check_open()?;
        // The writer validates the durable grant; no fabricated local actor/lease.
        Ok(self
            .repository
            .clone()
            .with_calendar_execution_guard(fence)
            .calendar_background_fetch()?)
    }
    pub fn finish_calendar_fetch(
        &mut self,
        ticket: CalendarFetchTicket,
        outcome: CalendarFetchOutcome,
    ) -> Result<CalendarStatus> {
        self.finish_calendar_fetch_while(ticket, outcome, Arc::new(|| true))
    }
    pub fn finish_calendar_fetch_while(
        &mut self,
        ticket: CalendarFetchTicket,
        outcome: CalendarFetchOutcome,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<CalendarStatus> {
        self.check_open()?;
        let result = self
            .repository
            .clone()
            .with_calendar_execution_guard(fence)
            .finish_calendar_fetch(ticket.clone(), outcome);
        match result {
            // The guarded transaction has rolled back. Settle the exact old
            // attempt without publishing a late body or releasing its interval.
            Err(CalendarRepositoryError::Cancelled) => Ok(self.repository.finish_calendar_fetch(
                ticket,
                CalendarFetchOutcome::Failed(CalendarFetchFailure::Cancelled),
            )?),
            result => Ok(result?),
        }
    }
    pub fn adopt_calendar(
        &mut self,
        lease: SessionLease,
        expected: Revision,
        candidates: Vec<CalendarCandidateRef>,
    ) -> Result<ScheduleDocument> {
        self.adopt_calendar_while(lease, expected, candidates, Arc::new(|| true))
    }
    pub fn adopt_calendar_while(
        &mut self,
        lease: SessionLease,
        expected: Revision,
        candidates: Vec<CalendarCandidateRef>,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ScheduleDocument> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let request = CalendarAdoptionRequest {
            command_id: self.command_id()?,
            expected_schedule_revision: expected,
            candidates,
        };
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .with_calendar_execution_guard(fence)
            .adopt_calendar(request, &actor)?)
    }
    pub fn set_calendar_auto_policy(
        &mut self,
        lease: SessionLease,
        expected: Revision,
        enabled: bool,
    ) -> Result<CalendarAutoPolicy> {
        self.set_calendar_auto_policy_while(lease, expected, enabled, Arc::new(|| true))
    }
    pub fn set_calendar_auto_policy_while(
        &mut self,
        lease: SessionLease,
        expected: Revision,
        enabled: bool,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<CalendarAutoPolicy> {
        match self.set_calendar_auto_policy_window_while(lease, expected, enabled, fence)? {
            fc_ports::command_window::TypedCommandOutcome::Completed(
                fc_ports::command_window::TypedCommandResult::CalendarPolicy(policy),
            ) => Ok(policy),
            _ => Err(StorageError::Indeterminate.into()),
        }
    }
}

/// A first manual provider attempt; its original owner cannot be cloned from
/// a historical receipt. Only transport validators are exposed to the worker.
pub struct ManualCalendarFetch(fc_storage::business::ManualFetchPermit);
impl ManualCalendarFetch {
    pub fn year(&self) -> i32 {
        self.0.year()
    }
    pub fn validators(&self) -> CalendarFetchValidators {
        self.0.validators()
    }
}
pub enum CalendarManualStart {
    Fresh(ManualCalendarFetch),
    History(fc_ports::command_window::TypedCommandOutcome),
}
impl Backend {
    pub fn set_calendar_auto_policy_window_while(
        &mut self,
        lease: SessionLease,
        expected_policy: Revision,
        enabled: bool,
        fence: TransactionExecutionFence,
    ) -> Result<fc_ports::command_window::TypedCommandOutcome> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        self.guarded(lease, generation, SessionPurpose::Manage)
            .with_calendar_execution_guard(fence)
            .set_calendar_policy_typed(expected_policy, enabled, &actor)
            .map_err(calendar_window_error)
    }
    pub fn begin_calendar_manual_fetch_while(
        &mut self,
        lease: SessionLease,
        year: i32,
        fence: TransactionExecutionFence,
    ) -> Result<CalendarManualStart> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        match self
            .guarded(lease, generation, SessionPurpose::Manage)
            .with_calendar_execution_guard(fence)
            .begin_calendar_manual_typed(year, &actor)
            .map_err(calendar_window_error)?
        {
            fc_storage::business::ManualFetchStart::Fresh(permit) => {
                Ok(CalendarManualStart::Fresh(ManualCalendarFetch(permit)))
            }
            fc_storage::business::ManualFetchStart::History(outcome) => {
                Ok(CalendarManualStart::History(outcome))
            }
        }
    }
    pub fn finish_calendar_manual_fetch_while(
        &mut self,
        permit: ManualCalendarFetch,
        outcome: CalendarFetchOutcome,
        fence: TransactionExecutionFence,
    ) -> Result<fc_ports::command_window::TypedCommandOutcome> {
        self.check_open()?;
        self.repository
            .clone()
            .with_calendar_execution_guard(fence)
            .finish_calendar_manual_typed(permit.0, outcome)
            .map_err(calendar_window_error)
    }
    pub fn query_typed_outcome_native(
        &self,
        lease: SessionLease,
        issued: fc_ports::command_window::IssuedCommand,
        intent: fc_ports::command_window::TypedCommandIntent,
    ) -> Result<fc_ports::command_window::TypedCommandOutcome> {
        use fc_ports::command_window::{CommandFamily, CommandOwner, TypedCommandIntent};
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let family = match &intent {
            TypedCommandIntent::CalendarPolicy { .. }
            | TypedCommandIntent::CalendarManualFetch { .. } => CommandFamily::Calendar,
            TypedCommandIntent::VoicePolicy(_) => CommandFamily::Voice,
            TypedCommandIntent::RestoreBarrier { .. } => CommandFamily::Generic,
        };
        if issued.family != family
            || issued.owner != CommandOwner::authenticated(&actor)
            || actor.origin != TrustedOrigin::Local
        {
            return Err(StorageError::CommandNotIssued.into());
        }
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .with_command_window(issued)
            .query_typed_command(intent, &actor)?)
    }
}

fn calendar_window_error(error: StorageError) -> BackendError {
    match error {
        StorageError::CalendarCancelled => {
            BackendError::Calendar(CalendarRepositoryError::Cancelled)
        }
        other => BackendError::Storage(other),
    }
}
