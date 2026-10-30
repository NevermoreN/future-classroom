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
    pub fn reserve_calendar_fetch(
        &mut self,
        lease: SessionLease,
        year: i32,
    ) -> Result<CalendarReservation> {
        self.reserve_calendar_fetch_while(lease, year, Arc::new(|| true))
    }
    pub fn reserve_calendar_fetch_while(
        &mut self,
        lease: SessionLease,
        year: i32,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<CalendarReservation> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .with_calendar_execution_guard(fence)
            .reserve_calendar_fetch(year, &actor)?)
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
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .with_calendar_execution_guard(fence)
            .set_calendar_auto_policy(expected, enabled, &actor)?)
    }
}
