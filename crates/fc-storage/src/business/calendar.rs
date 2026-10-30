//! Durable non-active cache, single-writer fetch reservations and narrow adoption.
//! No network, filesystem or session fabrication occurs inside a transaction.
use super::{calendar_codec as dto, *};
use fc_domain::{calendar::*, schedule::CalendarMode, time::*};
use fc_ports::calendar::*;
use std::collections::BTreeSet;
const META: &str = "business/calendar/meta/v1";
const PREFIX: &str = "business/calendar/candidate/v1/";
fn key(year: i32) -> String {
    format!("{PREFIX}{year:04}")
}
#[derive(Clone, Default)]
pub(super) struct Cache {
    pub sequence: u64,
    pub adoption_count: u64,
    pub watermark: Option<LocalDateTime>,
    pub policy: CalendarAutoPolicy,
    pub years: Vec<Year>,
}
#[derive(Clone)]
pub(super) struct Year {
    pub summary: CalendarYearSummary,
    pub ticket: Option<CalendarFetchTicket>,
    pub candidate_digest: Option<[u8; 32]>,
}
impl Cache {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        if self.years.len() > MAX_PUBLIC_YEARS || self.adoption_count > 65536 {
            return Err(StorageError::Database);
        }
        let mut last = 0;
        for y in &self.years {
            let s = &y.summary;
            if !(1..=9999).contains(&s.year)
                || s.year <= last
                || s.pending != y.ticket.is_some()
                || (s.candidate_revision.get() > 0) != y.candidate_digest.is_some()
                || s.last_attempt.is_none() != s.next_allowed.is_none()
                || s.last_success.is_some() != y.candidate_digest.is_some()
                || s.last_attempt
                    .zip(s.next_allowed)
                    .is_some_and(|(a, n)| tomorrow(a).ok() != Some(n))
                || (s.pending && s.last_error.is_some())
                || [s.last_attempt, s.last_success]
                    .into_iter()
                    .flatten()
                    .any(|t| self.watermark.is_none_or(|w| t > w))
            {
                return Err(StorageError::Database);
            }
            if let Some(t) = &y.ticket {
                if t.year != s.year
                    || t.attempt_sequence > self.sequence
                    || t.expected_candidate_revision != s.candidate_revision
                    || s.last_attempt.is_none()
                {
                    return Err(StorageError::Database);
                }
            }
            last = s.year;
        }
        Ok(())
    }
    fn observed(&mut self, now: LocalDateTime) -> LocalDateTime {
        let now = self.watermark.map_or(now, |w| w.max(now));
        self.watermark = Some(now);
        now
    }
    fn row(&self, year: i32) -> Option<&Year> {
        self.years.iter().find(|r| r.summary.year == year)
    }
    fn row_mut(&mut self, year: i32) -> Result<&mut Year, StorageError> {
        if self.row(year).is_none() {
            if self.years.len() >= MAX_PUBLIC_YEARS {
                return Err(StorageError::ResourceQuota);
            }
            self.years.push(Year {
                summary: CalendarYearSummary {
                    year,
                    candidate_revision: Revision::default(),
                    last_attempt: None,
                    next_allowed: None,
                    last_success: None,
                    last_error: None,
                    pending: false,
                },
                ticket: None,
                candidate_digest: None,
            });
            self.years.sort_by_key(|y| y.summary.year);
        }
        self.years
            .iter_mut()
            .find(|y| y.summary.year == year)
            .ok_or(StorageError::Database)
    }
}
fn valid_year(year: i32) -> Result<(), StorageError> {
    if !(1..=9999).contains(&year) {
        Err(StorageError::InvalidCommand)
    } else {
        Ok(())
    }
}
fn load(d: &dyn TransactionDocuments) -> Result<Cache, StorageError> {
    d.get(META)?.map_or(Ok(Cache::default()), |r| {
        dto::read_cache(&codec::value(&r.value)?)
    })
}
fn load_candidate(
    d: &dyn TransactionDocuments,
    c: &Cache,
    year: i32,
) -> Result<Option<CalendarCandidate>, StorageError> {
    let row = d.get(&key(year))?;
    match (c.row(year).and_then(|r| r.candidate_digest), row) {
        (None, None) => Ok(None),
        (Some(expected), Some(r)) if Sha256::digest(&r.value).as_slice() == expected => {
            let candidate = dto::read_candidate(&codec::value(&r.value)?)?;
            if candidate.source_year != year
                || Some(candidate.fetched_at) != c.row(year).and_then(|r| r.summary.last_success)
            {
                return Err(StorageError::Database);
            }
            Ok(Some(candidate))
        }
        _ => Err(StorageError::Database),
    }
}
pub(super) fn startup_check(d: &dyn TransactionDocuments) -> Result<(), StorageError> {
    let c = load(d)?;
    if d.get(META)?.is_some() && load_identity(d)?.installation.is_none() {
        return Err(StorageError::Database);
    }
    let page = d.scan_prefix(PREFIX, None, MAX_PUBLIC_YEARS + 1)?;
    if page.next_key.is_some() || page.entries.len() > MAX_PUBLIC_YEARS {
        return Err(StorageError::Database);
    }
    let expected: BTreeSet<_> = c
        .years
        .iter()
        .filter(|r| r.candidate_digest.is_some())
        .map(|r| key(r.summary.year))
        .collect();
    let actual: BTreeSet<_> = page.entries.iter().map(|(k, _)| k.clone()).collect();
    if expected != actual {
        return Err(StorageError::Database);
    }
    for y in &c.years {
        let candidate = load_candidate(d, &c, y.summary.year)?;
        if y.ticket.as_ref().is_some_and(|ticket| {
            ticket.etag
                != candidate
                    .as_ref()
                    .and_then(|candidate| candidate.etag.clone())
                || ticket.last_modified
                    != candidate
                        .as_ref()
                        .and_then(|candidate| candidate.last_modified.clone())
        }) {
            return Err(StorageError::Database);
        }
    }
    Ok(())
}
fn snapshot(
    c: &Cache,
    d: &dyn TransactionDocuments,
    year: i32,
) -> Result<CalendarStatus, StorageError> {
    Ok(CalendarStatus {
        policy: c.policy.clone(),
        years: c.years.iter().map(|y| y.summary.clone()).collect(),
        selected_year: year,
        candidate: load_candidate(d, c, year)?,
    })
}
fn bytes(value: &Value) -> Result<Vec<u8>, StorageError> {
    let b = codec::bytes(value)?;
    if b.len() > MAX_VALUE_BYTES {
        return Err(StorageError::DocumentTooLarge);
    }
    Ok(b)
}
fn meta(c: &Cache) -> Result<Mutation, StorageError> {
    c.validate()?;
    Ok(Mutation::Put {
        key: META.into(),
        value: bytes(&dto::cache(c))?,
    })
}
fn error(e: StorageError) -> CalendarRepositoryError {
    match e {
        StorageError::CalendarCancelled => CalendarRepositoryError::Cancelled,
        StorageError::Schedule(fc_domain::schedule_document::ScheduleDocumentError {
            kind: fc_domain::schedule_document::DocumentErrorKind::RevisionConflict { .. },
            ..
        }) => CalendarRepositoryError::Stale,
        StorageError::Schedule(fc_domain::schedule_document::ScheduleDocumentError {
            kind: fc_domain::schedule_document::DocumentErrorKind::Identity(_),
            ..
        }) => CalendarRepositoryError::Denied,
        StorageError::InvalidCommand | StorageError::Schedule(_) => {
            CalendarRepositoryError::Invalid
        }
        StorageError::Conflict { .. } | StorageError::CommandIdReused => {
            CalendarRepositoryError::Stale
        }
        StorageError::Identity(_) => CalendarRepositoryError::Denied,
        StorageError::ResourceQuota | StorageError::DocumentTooLarge => {
            CalendarRepositoryError::Capacity
        }
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::AuthorityUnavailable
        | StorageError::WorkerPanicked => CalendarRepositoryError::Faulted,
        _ => CalendarRepositoryError::Unavailable,
    }
}
fn execution_check(execution: &CalendarExecutionGuard) -> Result<(), StorageError> {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execution())).unwrap_or(false) {
        Ok(())
    } else {
        Err(StorageError::CalendarCancelled)
    }
}
fn denied() -> StorageError {
    StorageError::Identity(IdentityError::Permission(
        fc_domain::auth::PermissionError::RoleDenied,
    ))
}
fn authorize(
    identity: &IdentityState,
    actor: &ActorContext,
    clock: &dyn Clock,
    session: &SessionGuard,
) -> Result<(), StorageError> {
    session(identity, actor)?;
    identity
        .authorize_management(actor, clock.now())
        .map_err(StorageError::Identity)
}
fn grant_valid(c: &Cache, identity: &IdentityState) -> bool {
    c.policy.enabled
        && c.policy.grant.as_ref().is_some_and(|g| {
            identity.accounts.iter().any(|a| {
                a.principal.id == g.principal_id
                    && a.principal.auth_epoch == g.auth_epoch
                    && a.principal.enabled
                    && a.principal.role == Role::GlobalAdmin
                    && !a.must_change_password
                    && identity
                        .installation
                        .as_ref()
                        .is_some_and(|i| i.school_id == a.principal.school_id)
            })
        })
}
fn tomorrow(now: LocalDateTime) -> Result<LocalDateTime, StorageError> {
    let d = now.date;
    let next = LocalDate::new(d.year(), d.month(), d.day() + 1)
        .or_else(|_| LocalDate::new(d.year(), d.month() + 1, 1))
        .or_else(|_| LocalDate::new(d.year() + 1, 1, 1))
        .map_err(|_| StorageError::InvalidCommand)?;
    LocalDateTime::new(next, now.second_of_day()).map_err(|_| StorageError::InvalidCommand)
}
fn reserve(
    d: &dyn TransactionDocuments,
    c: &mut Cache,
    schedule: &fc_domain::schedule_document::ScheduleDocument,
    year: i32,
    now: LocalDateTime,
) -> Result<CalendarReservation, StorageError> {
    valid_year(year)?;
    let now = c.observed(now);
    if let Some(next_allowed) = c
        .row(year)
        .and_then(|r| r.summary.next_allowed)
        .filter(|n| now < *n)
    {
        return Ok(CalendarReservation::RateLimited { next_allowed });
    }
    let candidate = load_candidate(d, c, year)?;
    c.sequence = c
        .sequence
        .checked_add(1)
        .ok_or(StorageError::ResourceQuota)?;
    let ticket = CalendarFetchTicket {
        year,
        attempt_sequence: c.sequence,
        expected_candidate_revision: c
            .row(year)
            .map_or(Revision::default(), |r| r.summary.candidate_revision),
        captured_mode_revision: schedule.calendar_mode_revision,
        captured_policy_revision: c.policy.revision,
        etag: candidate.as_ref().and_then(|c| c.etag.clone()),
        last_modified: candidate.as_ref().and_then(|c| c.last_modified.clone()),
    };
    let y = c.row_mut(year)?;
    y.summary.last_attempt = Some(now);
    y.summary.next_allowed = Some(tomorrow(now)?);
    y.summary.pending = true;
    y.summary.last_error = None;
    y.ticket = Some(ticket.clone());
    Ok(CalendarReservation::Reserved(ticket))
}
impl<S: Send + 'static> CalendarRepository for BusinessRepository<S> {
    fn calendar_status(
        &self,
        year: i32,
        actor: &ActorContext,
    ) -> Result<CalendarStatus, CalendarRepositoryError> {
        let b = self
            .handle
            .try_transaction(Read {
                year,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_status(&b).map_err(error)
    }
    fn reserve_calendar_fetch(
        &mut self,
        year: i32,
        actor: &ActorContext,
    ) -> Result<CalendarReservation, CalendarRepositoryError> {
        let b = self
            .handle
            .try_transaction(Reserve {
                year: Some(year),
                actor: Some(actor.clone()),
                clock: self.clock.clone(),
                session: self.session.clone(),
                execution: self.calendar_execution.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        let v = codec::value(&b).map_err(error)?;
        match v["kind"].as_str() {
            Some("reserved") => Ok(CalendarReservation::Reserved(
                dto::read_ticket(&v["ticket"]).map_err(error)?,
            )),
            Some("limited") => Ok(CalendarReservation::RateLimited {
                next_allowed: codec::read_time(&v["next_allowed"]).map_err(error)?,
            }),
            _ => Err(CalendarRepositoryError::Unavailable),
        }
    }
    fn calendar_background_fetch(
        &mut self,
    ) -> Result<Option<CalendarFetchTicket>, CalendarRepositoryError> {
        let b = self
            .handle
            .try_transaction(Reserve {
                year: None,
                actor: None,
                clock: self.clock.clone(),
                session: self.session.clone(),
                execution: self.calendar_execution.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        let v = codec::value(&b).map_err(error)?;
        if v["kind"] == "none" {
            Ok(None)
        } else {
            Ok(Some(dto::read_ticket(&v["ticket"]).map_err(error)?))
        }
    }
    fn finish_calendar_fetch(
        &mut self,
        ticket: CalendarFetchTicket,
        outcome: CalendarFetchOutcome,
    ) -> Result<CalendarStatus, CalendarRepositoryError> {
        let b = self
            .handle
            .try_transaction(Finish {
                ticket,
                outcome,
                clock: self.clock.clone(),
                execution: self.calendar_execution.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_status(&b).map_err(error)
    }
    fn adopt_calendar(
        &mut self,
        request: CalendarAdoptionRequest,
        actor: &ActorContext,
    ) -> Result<fc_domain::schedule_document::ScheduleDocument, CalendarRepositoryError> {
        let b = self
            .handle
            .try_transaction(Adopt {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                execution: self.calendar_execution.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        super::schedule::read_document(&b).map_err(error)
    }
    fn set_calendar_auto_policy(
        &mut self,
        expected_revision: Revision,
        enabled: bool,
        actor: &ActorContext,
    ) -> Result<CalendarAutoPolicy, CalendarRepositoryError> {
        let b = self
            .handle
            .try_transaction(Policy {
                expected_revision,
                enabled,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                execution: self.calendar_execution.clone(),
                factory: self.factory.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        dto::read_policy(&codec::value(&b).map_err(error)?).map_err(error)
    }
}
struct Read {
    year: i32,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S> TransactionTask<S> for Read {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        valid_year(self.year)?;
        let identity = load_identity(d)?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        let c = load(d)?;
        let outcome = dto::encode_status(&snapshot(&c, d, self.year)?)?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: None,
        })
    }
}
struct Reserve<S> {
    year: Option<i32>,
    actor: Option<ActorContext>,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
    execution: CalendarExecutionGuard,
}
impl<S: Send + 'static> TransactionTask<S> for Reserve<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        execution_check(&self.execution)?;
        let identity = load_identity(d)?;
        let mut c = load(d)?;
        let schedule = super::schedule::load(d, &identity)?;
        if let Some(actor) = &self.actor {
            authorize(&identity, actor, self.clock.as_ref(), &self.session)?;
        } else if !grant_valid(&c, &identity) || schedule.calendar_mode != CalendarMode::Public {
            return Ok(TransactionDecision {
                outcome: codec::bytes(&json!({"kind":"none"}))?,
                commit: None,
                precommit: None,
            });
        }
        let year = if let Some(year) = self.year {
            year
        } else {
            let mut years = BTreeSet::new();
            for plan in &schedule.semesters {
                if !plan.semester.enabled {
                    continue;
                }
                for year in plan.semester.start.year()..=plan.semester.end.year() {
                    years.insert(year);
                    if years.len() > MAX_PUBLIC_YEARS {
                        return Err(StorageError::ResourceQuota);
                    }
                }
            }
            if let Some(last) = years.last().copied().filter(|y| *y < 9999) {
                years.insert(last + 1);
            }
            if years.len() > MAX_PUBLIC_YEARS {
                return Err(StorageError::ResourceQuota);
            }
            let now = c
                .watermark
                .map_or(self.clock.now(), |w| w.max(self.clock.now()));
            let Some(year) = years.into_iter().find(|year| {
                c.row(*year)
                    .and_then(|r| r.summary.next_allowed)
                    .is_none_or(|n| now >= n)
            }) else {
                return Ok(TransactionDecision {
                    outcome: codec::bytes(&json!({"kind":"none"}))?,
                    commit: None,
                    precommit: None,
                });
            };
            year
        };
        let reservation = reserve(d, &mut c, &schedule, year, self.clock.now())?;
        let outcome = match reservation {
            CalendarReservation::Reserved(t) => json!({"kind":"reserved","ticket":dto::ticket(&t)}),
            CalendarReservation::RateLimited { next_allowed } => {
                json!({"kind":"limited","next_allowed":codec::time(next_allowed)})
            }
        };
        let mutation = meta(&c)?;
        let snapshot = (self.factory)(
            &identity,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        Ok(TransactionDecision {
            outcome: codec::bytes(&outcome)?,
            commit: Some((vec![mutation], snapshot)),
            precommit: Some(Box::new(move || {
                execution_check(&self.execution)?;
                if let Some(actor) = &self.actor {
                    authorize(&identity, actor, self.clock.as_ref(), &self.session)
                } else if grant_valid(&c, &identity)
                    && schedule.calendar_mode == CalendarMode::Public
                {
                    Ok(())
                } else {
                    Err(denied())
                }
            })),
        })
    }
}
struct Policy<S> {
    expected_revision: Revision,
    enabled: bool,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
    execution: CalendarExecutionGuard,
}
impl<S: Send + 'static> TransactionTask<S> for Policy<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        execution_check(&self.execution)?;
        let identity = load_identity(d)?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        let mut c = load(d)?;
        if c.policy.revision != self.expected_revision {
            return Err(StorageError::Conflict {
                current: c.policy.revision,
            });
        }
        c.policy = CalendarAutoPolicy {
            revision: c
                .policy
                .revision
                .next()
                .map_err(|_| StorageError::ResourceQuota)?,
            enabled: self.enabled,
            grant: self.enabled.then_some(CalendarPolicyGrant {
                principal_id: self.actor.principal_id,
                auth_epoch: self.actor.auth_epoch,
            }),
        };
        let outcome = codec::bytes(&dto::policy(&c.policy))?;
        let mutation = meta(&c)?;
        let snapshot = (self.factory)(
            &identity,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        Ok(TransactionDecision {
            outcome,
            commit: Some((vec![mutation], snapshot)),
            precommit: Some(Box::new(move || {
                execution_check(&self.execution)?;
                authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)
            })),
        })
    }
}
struct Finish<S> {
    ticket: CalendarFetchTicket,
    outcome: CalendarFetchOutcome,
    clock: Arc<dyn Clock + Send + Sync>,
    factory: Factory<S>,
    execution: CalendarExecutionGuard,
}
impl<S: Send + 'static> TransactionTask<S> for Finish<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(match &self.outcome {
                CalendarFetchOutcome::Downloaded(c) => c.allocation_bytes().unwrap_or(usize::MAX),
                _ => 0,
            })
            .saturating_add(self.ticket.etag.as_ref().map_or(0, String::capacity))
            .saturating_add(
                self.ticket
                    .last_modified
                    .as_ref()
                    .map_or(0, String::capacity),
            )
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let positive = !matches!(&self.outcome, CalendarFetchOutcome::Failed(_));
        if positive {
            execution_check(&self.execution)?;
        }
        let identity = load_identity(d)?;
        let mut c = load(d)?;
        let Some(row) = c.row(self.ticket.year) else {
            return Err(StorageError::Conflict {
                current: Revision::default(),
            });
        };
        if row.ticket.as_ref() != Some(&self.ticket)
            || row.summary.candidate_revision != self.ticket.expected_candidate_revision
        {
            return Err(StorageError::Conflict {
                current: row.summary.candidate_revision,
            });
        }
        let old_candidate = load_candidate(d, &c, self.ticket.year)?;
        let now = c.observed(self.clock.now());
        let captured_ticket = self.ticket.clone();
        let execution = self.execution.clone();
        let mut failure = None;
        let mut candidate = match self.outcome {
            CalendarFetchOutcome::Downloaded(candidate) => {
                if candidate.source_year != self.ticket.year || candidate.validate().is_err() {
                    failure = Some(CalendarFetchFailure::InvalidData);
                    None
                } else {
                    Some(candidate)
                }
            }
            CalendarFetchOutcome::NotModified => {
                if self.ticket.etag.is_none() && self.ticket.last_modified.is_none() {
                    failure = Some(CalendarFetchFailure::InvalidData);
                    None
                } else if let Some(candidate) = old_candidate.as_ref().filter(|c| {
                    c.etag == self.ticket.etag && c.last_modified == self.ticket.last_modified
                }) {
                    Some(candidate.clone())
                } else {
                    failure = Some(CalendarFetchFailure::InvalidData);
                    None
                }
            }
            CalendarFetchOutcome::Failed(f) => {
                failure = Some(f);
                None
            }
        };
        let mut candidate_blob = None;
        if let Some(candidate) = candidate.as_mut() {
            candidate.fetched_at = now;
            match bytes(&dto::candidate(candidate)) {
                Ok(blob) => candidate_blob = Some(blob),
                Err(StorageError::DocumentTooLarge) => {
                    failure = Some(CalendarFetchFailure::TooLarge);
                }
                Err(e) => return Err(e),
            }
        }
        let mut extra = Vec::new();
        let row = c.row_mut(self.ticket.year)?;
        row.ticket = None;
        row.summary.pending = false;
        row.summary.last_error = failure;
        if let Some(blob) = candidate_blob {
            row.summary.candidate_revision = row
                .summary
                .candidate_revision
                .next()
                .map_err(|_| StorageError::ResourceQuota)?;
            row.summary.last_success = Some(now);
            row.candidate_digest = Some(Sha256::digest(&blob).into());
            extra.push(Mutation::Put {
                key: key(self.ticket.year),
                value: blob,
            });
        } else {
            candidate = None;
        }
        let current = super::schedule::load(d, &identity)?;
        let authorized =
            grant_valid(&c, &identity) && c.policy.revision == self.ticket.captured_policy_revision;
        let proposed = if let Some(candidate) = candidate.as_ref() {
            let proposal = current
                .public_calendar_data
                .proposal(
                    std::slice::from_ref(candidate),
                    &current.public_calendar.entries,
                )
                .ok();
            if let Some(proposal) = proposal.filter(|proposal| {
                proposal.auto_eligible(
                    &current.public_calendar_data,
                    &current.public_calendar.entries,
                    AutoAdoptionContext {
                        current_mode: current.calendar_mode,
                        captured_mode_revision: self.ticket.captured_mode_revision,
                        current_mode_revision: current.calendar_mode_revision,
                        policy_authorized: authorized,
                    },
                )
            }) {
                current
                    .transition_calendar_data(proposal.data, current.revision, &identity)
                    .ok()
            } else {
                None
            }
        } else {
            None
        };
        // A candidate success is never rolled back merely because a large aggregate
        // cannot accept it. Such a year remains available for explicit review.
        let proposed = proposed
            .filter(|next| super::schedule::encode(&super::schedule_codec::document(next)).is_ok());
        let (mut mutations, snapshot) = if let Some(next) = proposed.as_ref() {
            super::schedule::publication(d, &identity, next, now, &self.factory)?
        } else {
            (
                Vec::new(),
                (self.factory)(
                    &identity,
                    d.revision()?
                        .next()
                        .map_err(|_| StorageError::InvalidCommand)?,
                )?,
            )
        };
        mutations.extend(extra);
        mutations.push(meta(&c)?);
        let status = CalendarStatus {
            policy: c.policy.clone(),
            years: c.years.iter().map(|y| y.summary.clone()).collect(),
            selected_year: self.ticket.year,
            candidate: candidate.or(old_candidate),
        };
        let outcome = dto::encode_status(&status)?;
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                if positive {
                    execution_check(&execution)?;
                }
                if proposed.is_some()
                    && (!grant_valid(&c, &identity)
                        || c.policy.revision != captured_ticket.captured_policy_revision
                        || current.calendar_mode != CalendarMode::Public
                        || current.calendar_mode_revision != captured_ticket.captured_mode_revision)
                {
                    return Err(denied());
                }
                Ok(())
            })),
        })
    }
}
struct Adopt<S> {
    request: CalendarAdoptionRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
    execution: CalendarExecutionGuard,
}
impl<S: Send + 'static> TransactionTask<S> for Adopt<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>().saturating_add(
            self.request
                .candidates
                .capacity()
                .saturating_mul(std::mem::size_of::<CalendarCandidateRef>()),
        )
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        execution_check(&self.execution)?;
        if self.request.candidates.is_empty() || self.request.candidates.len() > MAX_PUBLIC_YEARS {
            return Err(StorageError::InvalidCommand);
        }
        let identity = load_identity(d)?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        let mut sha = Sha256::new();
        sha.update(b"future-classroom/calendar-adopt/v1\0");
        sha.update(self.request.expected_schedule_revision.get().to_be_bytes());
        sha.update(codec::bytes(&codec::actor(&self.actor))?);
        for r in &self.request.candidates {
            sha.update(r.year.to_be_bytes());
            sha.update(r.revision.get().to_be_bytes());
        }
        let fingerprint: Vec<u8> = sha.finalize().to_vec();
        let receipt_key = format!(
            "business/receipt/calendar/{}",
            self.request.command_id.get()
        );
        let result_key = format!("business/result/calendar/{}", self.request.command_id.get());
        if let Some(receipt) = d.get(&receipt_key)? {
            let value = codec::value(&receipt.value)?;
            codec::object_fields(&value, &["fingerprint", "revision", "result_digest"])?;
            let old: Vec<u8> = serde_json::from_value(value["fingerprint"].clone())
                .map_err(|_| StorageError::Database)?;
            if old.len() != 32 {
                return Err(StorageError::Database);
            }
            if old != fingerprint {
                return Err(StorageError::CommandIdReused);
            }
            let result = d.get(&result_key)?.ok_or(StorageError::Database)?;
            let digest: Vec<u8> = serde_json::from_value(value["result_digest"].clone())
                .map_err(|_| StorageError::Database)?;
            if digest.len() != 32 || digest.as_slice() != Sha256::digest(&result.value).as_slice() {
                return Err(StorageError::Database);
            }
            let doc = super::schedule::read_document(&result.value)?;
            if doc.revision.get() != codec::number(&value, "revision")?
                || doc.revision
                    != self
                        .request
                        .expected_schedule_revision
                        .next()
                        .map_err(|_| StorageError::Database)?
            {
                return Err(StorageError::Database);
            }
            authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
            return Ok(TransactionDecision {
                outcome: result.value,
                commit: None,
                precommit: None,
            });
        }
        let mut c = load(d)?;
        let current = super::schedule::load(d, &identity)?;
        if c.adoption_count >= 65536 {
            return Err(StorageError::ResourceQuota);
        }
        if current.calendar_mode != CalendarMode::Public {
            return Err(StorageError::InvalidCommand);
        }
        let mut candidates = Vec::new();
        let mut selected = BTreeSet::new();
        for r in &self.request.candidates {
            valid_year(r.year)?;
            if !selected.insert(r.year) {
                return Err(StorageError::InvalidCommand);
            }
            let row = c.row(r.year).ok_or(StorageError::InvalidCommand)?;
            if row.summary.candidate_revision != r.revision || r.revision.get() == 0 {
                return Err(StorageError::Conflict {
                    current: row.summary.candidate_revision,
                });
            }
            candidates.push(load_candidate(d, &c, r.year)?.ok_or(StorageError::Database)?);
        }
        let proposal = current
            .public_calendar_data
            .proposal(&candidates, &current.public_calendar.entries)
            .map_err(|_| StorageError::InvalidCommand)?;
        let data = proposal.data;
        let next = current
            .transition_calendar_data(
                data.clone(),
                self.request.expected_schedule_revision,
                &identity,
            )
            .map_err(StorageError::Schedule)?;
        let outcome = super::schedule::encode(&super::schedule_codec::document(&next))?;
        let (mut mutations, snapshot) =
            super::schedule::publication(d, &identity, &next, self.clock.now(), &self.factory)?;
        c.adoption_count += 1;
        mutations.push(meta(&c)?);
        mutations.push(Mutation::Put {
            key: result_key,
            value: outcome.clone(),
        });
        mutations.push(Mutation::Put{key:receipt_key,value:bytes(&json!({"fingerprint":fingerprint,"revision":next.revision.get(),"result_digest":Sha256::digest(&outcome).to_vec()}))?});
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                execution_check(&self.execution)?;
                authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
                current
                    .transition_calendar_data(
                        data,
                        self.request.expected_schedule_revision,
                        &identity,
                    )
                    .map(|_| ())
                    .map_err(StorageError::Schedule)
            })),
        })
    }
}
#[cfg(test)]
#[path = "tests/calendar.rs"]
mod tests;
