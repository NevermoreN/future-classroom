//! Real SQLite transactions with synthetic identity and an exact fixture lease.
use super::*;
use fc_ports::{AuthorityPublisher, ScheduleRepository, ScheduleRequest};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "FutureClassroom-Calendar-R20-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn at(day: u8) -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 10, day).unwrap(), 28800).unwrap()
}
struct Time(Mutex<LocalDateTime>);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        *self.0.lock().unwrap()
    }
}
fn actor() -> ActorContext {
    ActorContext {
        principal_id: PrincipalId::new(1).unwrap(),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(LocalDate::new(2027, 12, 31).unwrap(), 0).unwrap(),
        origin: TrustedOrigin::Local,
    }
}
fn identity() -> IdentityState {
    let school = SchoolId::new(1).unwrap();
    IdentityState {
        revision: Revision::new(1),
        installation: Some(Installation {
            school_id: school,
            school_name: "calendar synthetic".into(),
            current_class: None,
        }),
        accounts: (1..=2)
            .map(|id| UserAccount {
                principal: Principal {
                    id: PrincipalId::new(id).unwrap(),
                    school_id: school,
                    role: Role::GlobalAdmin,
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("admin{id}"),
                display_name: format!("Admin {id}"),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            })
            .collect(),
        ..Default::default()
    }
}
struct Publisher {
    active: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
}
impl AuthorityPublisher for Publisher {
    type Snapshot = Revision;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, s: &Revision) -> Revision {
        *s
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        if self.revoke.swap(false, Ordering::AcqRel) {
            self.active.store(false, Ordering::Release)
        }
        Ok(())
    }
    fn publish(&mut self, _: (), _: Revision) -> Result<(), ()> {
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn fault(&mut self) {}
}
struct Fixture {
    writer: Option<StorageWriter<Revision>>,
    directory: Directory,
    clock: Arc<Time>,
    active: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(w) = self.writer.take() {
            w.shutdown().unwrap()
        }
    }
}
impl Fixture {
    fn new() -> Self {
        let mut f = Self {
            writer: None,
            directory: Directory::new(),
            clock: Arc::new(Time(Mutex::new(at(10)))),
            active: Arc::new(AtomicBool::new(true)),
            revoke: Arc::new(AtomicBool::new(false)),
        };
        f.open();
        let mut m = vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&identity())).unwrap(),
        }];
        for id in 1..=2 {
            for kind in ["password", "recovery"] {
                m.push(Mutation::Put {
                    key: credential_key(PrincipalId::new(id).unwrap(), kind, 1),
                    value: vec![id as u8],
                });
            }
        }
        f.commit(m);
        f
    }
    fn open(&mut self) {
        self.writer = Some(
            StorageWriter::open(
                self.directory.0.join("state.sqlite3"),
                8,
                Publisher {
                    active: self.active.clone(),
                    revoke: self.revoke.clone(),
                },
            )
            .unwrap(),
        )
    }
    fn reopen(&mut self) {
        self.writer.take().unwrap().shutdown().unwrap();
        self.open()
    }
    fn handle(&self) -> StorageHandle<Revision> {
        self.writer.as_ref().unwrap().handle()
    }
    fn commit(&self, m: Vec<Mutation>) {
        let r = self.handle().try_revision().unwrap().wait().unwrap();
        self.handle()
            .try_commit(
                CommitCommand::new(format!("fixture{}", r.get()), r, m).unwrap(),
                r.next().unwrap(),
            )
            .unwrap()
            .wait()
            .unwrap();
    }
    fn repo(&self) -> BusinessRepository<Revision> {
        let active = self.active.clone();
        BusinessRepository::new(self.handle(), self.clock.clone(), Arc::new(|_, r| Ok(r)))
            .with_session_guard(Arc::new(move |_, a| {
                if a == &actor() && active.load(Ordering::Acquire) {
                    Ok(())
                } else {
                    Err(denied())
                }
            }))
    }
    fn next(&self, day: u8) {
        *self.clock.0.lock().unwrap() = at(day)
    }
    fn fetch(&self, year: i32) -> CalendarFetchTicket {
        match self.repo().reserve_calendar_fetch(year, &actor()).unwrap() {
            CalendarReservation::Reserved(t) => t,
            _ => panic!("unexpected throttle"),
        }
    }
    fn cache(&self, c: CalendarCandidate) -> CalendarStatus {
        let t = self.fetch(c.source_year);
        self.repo()
            .finish_calendar_fetch(t, CalendarFetchOutcome::Downloaded(c))
            .unwrap()
    }
    fn adopt(&self, id: u64, year: i32) -> fc_domain::schedule_document::ScheduleDocument {
        let mut r = self.repo();
        let current = r.schedule(&actor()).unwrap();
        let status = r.calendar_status(year, &actor()).unwrap();
        r.adopt_calendar(
            CalendarAdoptionRequest {
                command_id: CommandId::new(id).unwrap(),
                expected_schedule_revision: current.revision,
                candidates: vec![CalendarCandidateRef {
                    year,
                    revision: status
                        .years
                        .iter()
                        .find(|y| y.year == year)
                        .unwrap()
                        .candidate_revision,
                }],
            },
            &actor(),
        )
        .unwrap()
    }
    fn update_identity(&self, change: impl FnOnce(&mut IdentityState)) {
        let mut i = identity();
        change(&mut i);
        i.validate().unwrap();
        self.commit(vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&i)).unwrap(),
        }]);
    }
}
fn candidate(year: i32, day: u8) -> CalendarCandidate {
    CalendarCandidate {
        source_year: year,
        fetched_at: at(1),
        source_sha256: [day; 32],
        papers: vec!["https://www.gov.cn/zhengce/content/2025-11/01/content_1234567.htm".into()],
        days: vec![PublicCalendarDay {
            date: LocalDate::new(year, 10, day).unwrap(),
            kind: PublicDayKind::Rest,
            name: "synthetic holiday".into(),
        }],
        etag: Some(format!("\"{year}-{day}\"")),
        last_modified: None,
    }
}
#[test]
fn reservation_failure_and_clock_rollback_survive_restart() {
    let mut f = Fixture::new();
    let t = f.fetch(2026);
    assert_eq!(t.attempt_sequence, 1);
    f.repo()
        .finish_calendar_fetch(
            t,
            CalendarFetchOutcome::Failed(CalendarFetchFailure::Network),
        )
        .unwrap();
    f.reopen();
    f.next(9);
    assert!(
        matches!(f.repo().reserve_calendar_fetch(2026,&actor()).unwrap(),CalendarReservation::RateLimited{next_allowed}if next_allowed==at(11))
    );
    let s = f.repo().calendar_status(2026, &actor()).unwrap();
    assert_eq!(s.years[0].last_error, Some(CalendarFetchFailure::Network));
    assert!(s.candidate.is_none());
    f.next(11);
    assert_eq!(f.fetch(2026).attempt_sequence, 2);
}
#[test]
fn cache_is_not_active_and_only_explicit_adoption_verifies() {
    let f = Fixture::new();
    let s = f.cache(candidate(2026, 1));
    assert_eq!(s.years[0].candidate_revision, Revision::new(1));
    assert!(f
        .repo()
        .schedule(&actor())
        .unwrap()
        .public_calendar_data
        .years
        .is_empty());
    let adopted = f.adopt(10, 2026);
    assert_eq!(adopted.public_calendar_data.years.len(), 1);
    assert_eq!(adopted.public_calendar.public_verified_years, vec![2026]);
    assert_eq!(adopted.public_calendar_data.years[0].fetched_at, at(10));
}
#[test]
fn ordinary_schedule_draft_cannot_forge_sourced_data_or_verified_years() {
    let f = Fixture::new();
    let mut forged = fc_domain::schedule_document::ScheduleDocument::default();
    forged.public_calendar_data.years.push(candidate(2026, 1));
    let request = ScheduleRequest {
        command_id: CommandId::new(1).unwrap(),
        expected_revision: Revision::default(),
        candidate: forged,
    };
    assert!(f
        .repo()
        .execute_schedule(request, &actor(), f.clock.as_ref())
        .is_err());
    let mut forged = fc_domain::schedule_document::ScheduleDocument::default();
    forged.public_calendar.public_verified_years.push(2026);
    assert!(f
        .repo()
        .execute_schedule(
            ScheduleRequest {
                command_id: CommandId::new(2).unwrap(),
                expected_revision: Revision::default(),
                candidate: forged
            },
            &actor(),
            f.clock.as_ref()
        )
        .is_err());
    assert_eq!(
        f.repo().schedule(&actor()).unwrap().revision,
        Revision::default()
    );
}
#[test]
fn late_completion_cannot_overwrite_new_attempt_after_restart() {
    let mut f = Fixture::new();
    let old = f.fetch(2026);
    f.reopen();
    f.next(11);
    let new = f.fetch(2026);
    assert!(matches!(
        f.repo()
            .finish_calendar_fetch(old, CalendarFetchOutcome::Downloaded(candidate(2026, 1))),
        Err(CalendarRepositoryError::Stale)
    ));
    f.repo()
        .finish_calendar_fetch(new, CalendarFetchOutcome::Downloaded(candidate(2026, 2)))
        .unwrap();
    assert_eq!(
        f.repo()
            .calendar_status(2026, &actor())
            .unwrap()
            .candidate
            .unwrap()
            .source_sha256,
        [2; 32]
    );
}
#[test]
fn not_modified_requires_existing_exact_validators_and_preserves_source_hash() {
    let f = Fixture::new();
    let t = f.fetch(2026);
    let s = f
        .repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::NotModified)
        .unwrap();
    assert_eq!(
        s.years[0].last_error,
        Some(CalendarFetchFailure::InvalidData)
    );
    assert!(s.candidate.is_none());
    f.next(11);
    f.cache(candidate(2026, 1));
    f.next(12);
    let t = f.fetch(2026);
    let s = f
        .repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::NotModified)
        .unwrap();
    assert_eq!(s.candidate.unwrap().source_sha256, [1; 32]);
    assert_eq!(s.years[0].last_success, Some(at(12)));
    assert_eq!(s.years[0].candidate_revision, Revision::new(2));
}
#[test]
fn failure_keeps_previous_candidate_and_adopted_schedule() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    let before = f.adopt(1, 2026);
    f.next(11);
    let t = f.fetch(2026);
    let status = f
        .repo()
        .finish_calendar_fetch(
            t,
            CalendarFetchOutcome::Failed(CalendarFetchFailure::Cancelled),
        )
        .unwrap();
    assert_eq!(status.candidate.unwrap().source_sha256, [1; 32]);
    assert_eq!(status.years[0].last_success, Some(at(10)));
    assert_eq!(f.repo().schedule(&actor()).unwrap(), before);
}
#[test]
fn exact_local_lease_is_checked_again_at_real_commit() {
    let f = Fixture::new();
    f.revoke.store(true, Ordering::Release);
    assert!(matches!(
        f.repo().reserve_calendar_fetch(2026, &actor()),
        Err(CalendarRepositoryError::Denied)
    ));
    f.active.store(true, Ordering::Release);
    assert!(f
        .repo()
        .calendar_status(2026, &actor())
        .unwrap()
        .years
        .is_empty());
    assert_eq!(f.fetch(2026).attempt_sequence, 1);
}
#[test]
fn adoption_candidate_cas_schedule_cas_and_idempotent_result() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    let request = CalendarAdoptionRequest {
        command_id: CommandId::new(5).unwrap(),
        expected_schedule_revision: Revision::default(),
        candidates: vec![CalendarCandidateRef {
            year: 2026,
            revision: Revision::new(1),
        }],
    };
    let next = f.repo().adopt_calendar(request.clone(), &actor()).unwrap();
    assert_eq!(
        f.repo().adopt_calendar(request.clone(), &actor()).unwrap(),
        next
    );
    let mut stale = request.clone();
    stale.command_id = CommandId::new(6).unwrap();
    assert!(f.repo().adopt_calendar(stale, &actor()).is_err());
    f.next(11);
    f.cache(candidate(2026, 2));
    assert_eq!(f.repo().adopt_calendar(request, &actor()).unwrap(), next);
    let stale = CalendarAdoptionRequest {
        command_id: CommandId::new(7).unwrap(),
        expected_schedule_revision: next.revision,
        candidates: vec![CalendarCandidateRef {
            year: 2026,
            revision: Revision::new(1),
        }],
    };
    assert_eq!(
        f.repo().adopt_calendar(stale, &actor()),
        Err(CalendarRepositoryError::Stale)
    );
}
#[test]
fn invalid_empty_or_wrong_year_download_does_not_verify_or_erase() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    f.next(11);
    let t = f.fetch(2026);
    let s = f
        .repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::Downloaded(candidate(2027, 1)))
        .unwrap();
    assert_eq!(
        s.years[0].last_error,
        Some(CalendarFetchFailure::InvalidData)
    );
    assert_eq!(s.years[0].candidate_revision, Revision::new(1));
    f.next(12);
    let t = f.fetch(2026);
    let mut empty = candidate(2026, 2);
    empty.days.clear();
    let s = f
        .repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::Downloaded(empty))
        .unwrap();
    assert_eq!(s.candidate.unwrap().source_sha256, [1; 32]);
    assert_eq!(s.years[0].last_success, Some(at(10)));
}
#[test]
fn auto_policy_default_cas_and_revoked_grant_fail_closed() {
    let f = Fixture::new();
    let s = f.repo().calendar_status(2026, &actor()).unwrap();
    assert!(!s.policy.enabled);
    let p = f
        .repo()
        .set_calendar_auto_policy(Revision::default(), true, &actor())
        .unwrap();
    assert_eq!(p.grant.unwrap().principal_id, actor().principal_id);
    assert_eq!(
        f.repo()
            .set_calendar_auto_policy(Revision::default(), false, &actor()),
        Err(CalendarRepositoryError::Stale)
    );
    f.update_identity(|i| i.accounts[0].principal.auth_epoch = AuthEpoch::new(2));
    assert!(f.repo().calendar_background_fetch().unwrap().is_none());
}
#[test]
fn auto_pure_rest_update_works_without_live_local_lease() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    let adopted = f.adopt(1, 2026);
    f.repo()
        .set_calendar_auto_policy(Revision::default(), true, &actor())
        .unwrap();
    f.next(11);
    let t = f.fetch(2026);
    let mut changed = candidate(2026, 1);
    changed.source_sha256 = [9; 32];
    changed.days.push(PublicCalendarDay {
        date: LocalDate::new(2026, 10, 2).unwrap(),
        kind: PublicDayKind::Rest,
        name: "new synthetic holiday".into(),
    });
    f.active.store(false, Ordering::Release);
    f.repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::Downloaded(changed))
        .unwrap();
    f.active.store(true, Ordering::Release);
    let next = f.repo().schedule(&actor()).unwrap();
    assert_eq!(next.revision, adopted.revision.next().unwrap());
    assert_eq!(next.public_calendar_data.years[0].days.len(), 2);
    assert_eq!(
        next.public_calendar.mapping_revision,
        adopted.public_calendar.mapping_revision
    );
}
#[test]
fn auto_revoked_epoch_keeps_successful_cache_but_does_not_adopt() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    let adopted = f.adopt(1, 2026);
    f.repo()
        .set_calendar_auto_policy(Revision::default(), true, &actor())
        .unwrap();
    f.next(11);
    let t = f.fetch(2026);
    f.update_identity(|i| i.accounts[0].principal.auth_epoch = AuthEpoch::new(2));
    let s = f
        .repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::Downloaded(candidate(2026, 2)))
        .unwrap();
    assert_eq!(s.candidate.unwrap().source_sha256, [2; 32]);
    f.update_identity(|_| {});
    assert_eq!(f.repo().schedule(&actor()).unwrap(), adopted);
}
#[test]
fn switch_to_school_while_fetch_in_flight_only_updates_cache() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    f.adopt(1, 2026);
    f.repo()
        .set_calendar_auto_policy(Revision::default(), true, &actor())
        .unwrap();
    f.next(11);
    let t = f.fetch(2026);
    let mut current = f.repo().schedule(&actor()).unwrap();
    current.calendar_mode = CalendarMode::School;
    let before = f
        .repo()
        .execute_schedule(
            ScheduleRequest {
                command_id: CommandId::new(2).unwrap(),
                expected_revision: current.revision,
                candidate: current,
            },
            &actor(),
            f.clock.as_ref(),
        )
        .unwrap();
    f.repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::Downloaded(candidate(2026, 2)))
        .unwrap();
    assert_eq!(f.repo().schedule(&actor()).unwrap(), before);
    assert_eq!(
        f.repo()
            .calendar_status(2026, &actor())
            .unwrap()
            .candidate
            .unwrap()
            .source_sha256,
        [2; 32]
    );
}
#[test]
fn startup_rejects_candidate_hash_corruption() {
    let mut f = Fixture::new();
    f.cache(candidate(2026, 1));
    f.writer.take().unwrap().shutdown().unwrap();
    assert!(startup_state(f.directory.0.join("state.sqlite3")).is_ok());
    f.open();
    f.commit(vec![Mutation::Put {
        key: key(2026),
        value: bytes(&dto::candidate(&candidate(2026, 2))).unwrap(),
    }]);
    f.writer.take().unwrap().shutdown().unwrap();
    assert_eq!(
        startup_state(f.directory.0.join("state.sqlite3")),
        Err(StorageError::Database)
    );
}
#[test]
fn bounded_cache_rejects_thirty_third_year_without_losing_existing() {
    let f = Fixture::new();
    for year in 2000..2032 {
        f.fetch(year);
    }
    assert_eq!(
        f.repo().reserve_calendar_fetch(2032, &actor()),
        Err(CalendarRepositoryError::Capacity)
    );
    assert_eq!(
        f.repo()
            .calendar_status(2000, &actor())
            .unwrap()
            .years
            .len(),
        32
    );
}
#[test]
fn legacy_schedule_without_public_data_is_read_strictly_as_empty() {
    let d = fc_domain::schedule_document::ScheduleDocument::default();
    let mut value = super::super::schedule_codec::document(&d);
    value
        .as_object_mut()
        .unwrap()
        .remove("public_calendar_data");
    let decoded = super::super::schedule_codec::read_document(&value).unwrap();
    assert!(decoded.public_calendar_data.years.is_empty());
    value["unknown"] = json!(true);
    assert!(super::super::schedule_codec::read_document(&value).is_err());
}
fn add_term(f: &Fixture) -> fc_domain::schedule_document::ScheduleDocument {
    use fc_domain::{schedule::*, schedule_document::SemesterPlan};
    f.update_identity(|i| {
        i.classes.push(SchoolClass {
            id: ClassId::new(1).unwrap(),
            school_id: actor().school_id,
            name: "synthetic class".into(),
            enabled: true,
        });
        i.installation.as_mut().unwrap().current_class = Some(ClassId::new(1).unwrap());
    });
    let mut d = f.repo().schedule(&actor()).unwrap();
    d.semesters.push(SemesterPlan {
        semester: Semester {
            id: SemesterId::new(1).unwrap(),
            class_id: ClassId::new(1).unwrap(),
            revision: Revision::default(),
            enabled: true,
            start: LocalDate::new(2026, 9, 1).unwrap(),
            end: LocalDate::new(2027, 7, 1).unwrap(),
            first_week_monday: LocalDate::new(2026, 8, 31).unwrap(),
            tables: vec![WeekTable {
                id: TableId::new(2).unwrap(),
                role: TableRole::Single,
                rows: vec![],
            }],
        },
        name: "synthetic term".into(),
        timing_sync: true,
        dual_history: None,
        row_details: vec![],
        cell_details: vec![],
    });
    f.repo()
        .execute_schedule(
            ScheduleRequest {
                command_id: CommandId::new(500).unwrap(),
                expected_revision: d.revision,
                candidate: d,
            },
            &actor(),
            f.clock.as_ref(),
        )
        .unwrap()
}
#[test]
fn background_reserves_term_years_and_next_year_without_local_actor() {
    let f = Fixture::new();
    add_term(&f);
    assert!(f.repo().calendar_background_fetch().unwrap().is_none());
    f.repo()
        .set_calendar_auto_policy(Revision::default(), true, &actor())
        .unwrap();
    f.active.store(false, Ordering::Release);
    for year in [2026, 2027, 2028] {
        let t = f.repo().calendar_background_fetch().unwrap().unwrap();
        assert_eq!(t.year, year);
        f.repo()
            .finish_calendar_fetch(
                t,
                CalendarFetchOutcome::Failed(CalendarFetchFailure::Network),
            )
            .unwrap();
    }
    assert!(f.repo().calendar_background_fetch().unwrap().is_none());
}
#[test]
fn policy_revision_change_after_reserve_blocks_auto_without_discarding_cache() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    let before = f.adopt(1, 2026);
    let p = f
        .repo()
        .set_calendar_auto_policy(Revision::default(), true, &actor())
        .unwrap();
    f.next(11);
    let t = f.fetch(2026);
    let p = f
        .repo()
        .set_calendar_auto_policy(p.revision, false, &actor())
        .unwrap();
    f.repo()
        .set_calendar_auto_policy(p.revision, true, &actor())
        .unwrap();
    f.repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::Downloaded(candidate(2026, 2)))
        .unwrap();
    assert_eq!(f.repo().schedule(&actor()).unwrap(), before);
    assert_eq!(
        f.repo()
            .calendar_status(2026, &actor())
            .unwrap()
            .candidate
            .unwrap()
            .source_sha256,
        [2; 32]
    );
}
#[test]
fn mode_changed_away_and_back_invalidates_auto_capture() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    f.adopt(1, 2026);
    f.repo()
        .set_calendar_auto_policy(Revision::default(), true, &actor())
        .unwrap();
    f.next(11);
    let t = f.fetch(2026);
    for (id, mode) in [(20, CalendarMode::School), (21, CalendarMode::Public)] {
        let mut d = f.repo().schedule(&actor()).unwrap();
        d.calendar_mode = mode;
        f.repo()
            .execute_schedule(
                ScheduleRequest {
                    command_id: CommandId::new(id).unwrap(),
                    expected_revision: d.revision,
                    candidate: d,
                },
                &actor(),
                f.clock.as_ref(),
            )
            .unwrap();
    }
    let before = f.repo().schedule(&actor()).unwrap();
    f.repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::Downloaded(candidate(2026, 2)))
        .unwrap();
    assert_eq!(f.repo().schedule(&actor()).unwrap(), before);
}
#[test]
fn adoption_commit_revocation_rolls_back_document_receipt_and_change_ledger() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    let request = CalendarAdoptionRequest {
        command_id: CommandId::new(11).unwrap(),
        expected_schedule_revision: Revision::default(),
        candidates: vec![CalendarCandidateRef {
            year: 2026,
            revision: Revision::new(1),
        }],
    };
    f.revoke.store(true, Ordering::Release);
    assert_eq!(
        f.repo().adopt_calendar(request.clone(), &actor()),
        Err(CalendarRepositoryError::Denied)
    );
    assert!(f
        .handle()
        .try_get("business/receipt/calendar/11".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    assert!(f
        .handle()
        .try_get("business/schedule/last-changed/v1".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    f.active.store(true, Ordering::Release);
    f.repo().adopt_calendar(request, &actor()).unwrap();
    assert!(f
        .handle()
        .try_get("business/schedule/last-changed/v1".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_some());
}
#[test]
fn manual_override_and_makeup_changes_require_explicit_adoption() {
    use fc_domain::schedule::*;
    let f = Fixture::new();
    add_term(&f);
    f.cache(candidate(2026, 1));
    f.adopt(1, 2026);
    let mut d = f.repo().schedule(&actor()).unwrap();
    d.public_calendar.entries.push(CalendarEntry {
        actual_semester_id: SemesterId::new(1).unwrap(),
        date: LocalDate::new(2026, 10, 2).unwrap(),
        mapping: DayMapping::Unconfirmed,
    });
    let d = f
        .repo()
        .execute_schedule(
            ScheduleRequest {
                command_id: CommandId::new(22).unwrap(),
                expected_revision: d.revision,
                candidate: d,
            },
            &actor(),
            f.clock.as_ref(),
        )
        .unwrap();
    f.repo()
        .set_calendar_auto_policy(Revision::default(), true, &actor())
        .unwrap();
    f.next(11);
    f.cache(candidate(2026, 2));
    assert_eq!(f.repo().schedule(&actor()).unwrap(), d);
    let adopted = f.adopt(23, 2026);
    assert_eq!(adopted.public_calendar.entries, d.public_calendar.entries);
    f.next(12);
    let mut makeup = candidate(2026, 2);
    makeup.days[0].kind = PublicDayKind::Makeup;
    f.cache(makeup);
    assert_eq!(f.repo().schedule(&actor()).unwrap(), adopted);
}
#[test]
fn impossible_ticket_headers_fail_startup() {
    let mut f = Fixture::new();
    let t = f.fetch(2026);
    let meta = f
        .handle()
        .try_get(META.into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    let mut value = codec::value(&meta.value).unwrap();
    value["years"][0]["ticket"]["etag"] = json!("bad\r\nheader");
    f.commit(vec![Mutation::Put {
        key: META.into(),
        value: codec::bytes(&value).unwrap(),
    }]);
    f.writer.take().unwrap().shutdown().unwrap();
    assert_eq!(
        startup_state(f.directory.0.join("state.sqlite3")),
        Err(StorageError::Database)
    );
    assert_eq!(t.attempt_sequence, 1);
}
#[test]
fn startup_rejects_unindexed_candidate_namespace() {
    let mut f = Fixture::new();
    f.commit(vec![Mutation::Put {
        key: key(2026),
        value: bytes(&dto::candidate(&candidate(2026, 1))).unwrap(),
    }]);
    f.writer.take().unwrap().shutdown().unwrap();
    assert_eq!(
        startup_state(f.directory.0.join("state.sqlite3")),
        Err(StorageError::Database)
    );
}
#[test]
fn oversize_candidate_is_failed_attempt_and_preserves_previous_cache() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    f.next(11);
    let t = f.fetch(2026);
    let mut large = candidate(2026, 1);
    large.days = (0..365)
        .map(|n| {
            let mut date = LocalDate::new(2026, 1, 1).unwrap();
            for _ in 0..n {
                date = tomorrow(LocalDateTime::new(date, 0).unwrap()).unwrap().date;
            }
            PublicCalendarDay {
                date,
                kind: PublicDayKind::Rest,
                name: "字".repeat(64),
            }
        })
        .collect();
    large.validate().unwrap();
    let s = f
        .repo()
        .finish_calendar_fetch(t, CalendarFetchOutcome::Downloaded(large))
        .unwrap();
    assert_eq!(s.years[0].last_error, Some(CalendarFetchFailure::TooLarge));
    assert_eq!(s.candidate.unwrap().source_sha256, [1; 32]);
    assert_eq!(s.years[0].candidate_revision, Revision::new(1));
}
#[test]
fn durable_adoption_receipt_capacity_fails_without_touching_schedule() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    let stored = f
        .handle()
        .try_get(META.into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    let mut c = dto::read_cache(&codec::value(&stored.value).unwrap()).unwrap();
    c.adoption_count = 65536;
    f.commit(vec![meta(&c).unwrap()]);
    let request = CalendarAdoptionRequest {
        command_id: CommandId::new(55).unwrap(),
        expected_schedule_revision: Revision::default(),
        candidates: vec![CalendarCandidateRef {
            year: 2026,
            revision: Revision::new(1),
        }],
    };
    assert_eq!(
        f.repo().adopt_calendar(request, &actor()),
        Err(CalendarRepositoryError::Capacity)
    );
    assert_eq!(
        f.repo().schedule(&actor()).unwrap().revision,
        Revision::default()
    );
}
#[test]
fn identical_pre_upgrade_schedule_receipt_replays_without_sourced_downgrade() {
    let f = Fixture::new();
    let draft = fc_domain::schedule_document::ScheduleDocument::default();
    let result = draft
        .transition(&draft, Revision::default(), &identity(), &actor(), at(10))
        .unwrap();
    let mut legacy_draft = super::super::schedule_codec::document(&draft);
    legacy_draft
        .as_object_mut()
        .unwrap()
        .remove("public_calendar_data");
    let mut legacy_result = super::super::schedule_codec::document(&result);
    legacy_result
        .as_object_mut()
        .unwrap()
        .remove("public_calendar_data");
    let result_blob = super::super::schedule::encode(&legacy_result).unwrap();
    let mut hash = Sha256::new();
    hash.update(b"future-classroom/schedule/v1\0");
    hash.update(0u64.to_be_bytes());
    hash.update(codec::bytes(&codec::actor(&actor())).unwrap());
    hash.update(super::super::schedule::encode(&legacy_draft).unwrap());
    let fingerprint = hash.finalize().to_vec();
    f.commit(vec![Mutation::Put{key:"business/schedule/v1".into(),value:result_blob.clone()},Mutation::Put{key:"business/result/schedule/99".into(),value:result_blob.clone()},Mutation::Put{key:"business/receipt/schedule/99".into(),value:codec::bytes(&json!({"fingerprint":fingerprint,"revision":result.revision.get(),"result_digest":Sha256::digest(&result_blob).to_vec()})).unwrap()}]);
    let mut request = ScheduleRequest {
        command_id: CommandId::new(99).unwrap(),
        expected_revision: Revision::default(),
        candidate: draft,
    };
    assert_eq!(
        f.repo()
            .execute_schedule(request.clone(), &actor(), f.clock.as_ref())
            .unwrap(),
        result
    );
    request
        .candidate
        .public_calendar_data
        .years
        .push(candidate(2026, 1));
    assert!(f
        .repo()
        .execute_schedule(request, &actor(), f.clock.as_ref())
        .is_err());
}
#[test]
fn largest_bounded_status_uses_separate_response_budget_without_relaxing_blob_cap() {
    let f = Fixture::new();
    let mut selected = None;
    for year in 2000..2032 {
        let t = f.fetch(year);
        if year == 2026 {
            selected = Some(t)
        }
    }
    let mut c = candidate(2026, 1);
    c.days.clear();
    let mut date = LocalDate::new(2026, 1, 1).unwrap();
    loop {
        c.days.push(PublicCalendarDay {
            date,
            kind: PublicDayKind::Rest,
            name: "字".repeat(64),
        });
        if codec::bytes(&dto::candidate(&c)).unwrap().len() > 61 * 1024 {
            c.days.pop();
            break;
        }
        date = tomorrow(LocalDateTime::new(date, 0).unwrap()).unwrap().date;
    }
    let s = f
        .repo()
        .finish_calendar_fetch(selected.unwrap(), CalendarFetchOutcome::Downloaded(c))
        .unwrap();
    let response = codec::bytes(&dto::status(&s)).unwrap();
    assert!(response.len() > MAX_VALUE_BYTES);
    assert!(response.len() <= 2 * MAX_VALUE_BYTES);
    assert_eq!(dto::read_status(&response).unwrap(), s);
    let blob = f
        .handle()
        .try_get(key(2026))
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    assert!(blob.value.len() <= MAX_VALUE_BYTES);
    assert_eq!(f.repo().calendar_status(2026, &actor()).unwrap(), s);
}
fn until_precommit() -> CalendarExecutionGuard {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    Arc::new(move || calls.fetch_add(1, Ordering::AcqRel) == 0)
}
#[test]
fn execution_guard_rejects_before_evaluation_without_faulting_authority() {
    let f = Fixture::new();
    let mut r = f
        .repo()
        .with_calendar_execution_guard(Arc::new(|| false))
        .clone();
    assert_eq!(
        r.reserve_calendar_fetch(2026, &actor()),
        Err(CalendarRepositoryError::Cancelled)
    );
    assert_eq!(
        r.set_calendar_auto_policy(Revision::default(), true, &actor()),
        Err(CalendarRepositoryError::Cancelled)
    );
    assert_eq!(
        r.calendar_background_fetch(),
        Err(CalendarRepositoryError::Cancelled)
    );
    assert!(f
        .repo()
        .calendar_status(2026, &actor())
        .unwrap()
        .years
        .is_empty());
    assert_eq!(f.fetch(2026).attempt_sequence, 1);
}
#[test]
fn reservation_and_policy_precommit_cancellation_roll_back_without_consuming_reservation() {
    let f = Fixture::new();
    assert_eq!(
        f.repo()
            .with_calendar_execution_guard(until_precommit())
            .reserve_calendar_fetch(2026, &actor()),
        Err(CalendarRepositoryError::Cancelled)
    );
    assert!(f
        .repo()
        .calendar_status(2026, &actor())
        .unwrap()
        .years
        .is_empty());
    assert_eq!(f.fetch(2026).attempt_sequence, 1);
    assert_eq!(
        f.repo()
            .with_calendar_execution_guard(until_precommit())
            .set_calendar_auto_policy(Revision::default(), true, &actor()),
        Err(CalendarRepositoryError::Cancelled)
    );
    assert_eq!(
        f.repo().calendar_status(2026, &actor()).unwrap().policy,
        CalendarAutoPolicy::default()
    );
}
#[test]
fn positive_finish_precommit_cancel_preserves_cache_then_failure_settles_same_ticket() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    f.next(11);
    let ticket = f.fetch(2026);
    assert_eq!(
        f.repo()
            .with_calendar_execution_guard(until_precommit())
            .finish_calendar_fetch(
                ticket.clone(),
                CalendarFetchOutcome::Downloaded(candidate(2026, 2))
            ),
        Err(CalendarRepositoryError::Cancelled)
    );
    let before = f.repo().calendar_status(2026, &actor()).unwrap();
    assert_eq!(before.candidate.unwrap().source_sha256, [1; 32]);
    assert!(before.years[0].pending);
    assert_eq!(before.years[0].candidate_revision, Revision::new(1));
    let settled = f
        .repo()
        .with_calendar_execution_guard(Arc::new(|| {
            panic!("failure settlement must not consult execution")
        }))
        .finish_calendar_fetch(
            ticket,
            CalendarFetchOutcome::Failed(CalendarFetchFailure::Cancelled),
        )
        .unwrap();
    assert!(!settled.years[0].pending);
    assert_eq!(
        settled.years[0].last_error,
        Some(CalendarFetchFailure::Cancelled)
    );
    assert_eq!(settled.years[0].next_allowed, Some(at(12)));
    assert_eq!(settled.candidate.unwrap().source_sha256, [1; 32]);
}
#[test]
fn adoption_precommit_execution_cancellation_rolls_back_receipt_and_schedule() {
    let f = Fixture::new();
    f.cache(candidate(2026, 1));
    let request = CalendarAdoptionRequest {
        command_id: CommandId::new(200).unwrap(),
        expected_schedule_revision: Revision::default(),
        candidates: vec![CalendarCandidateRef {
            year: 2026,
            revision: Revision::new(1),
        }],
    };
    assert_eq!(
        f.repo()
            .with_calendar_execution_guard(until_precommit())
            .adopt_calendar(request.clone(), &actor()),
        Err(CalendarRepositoryError::Cancelled)
    );
    assert_eq!(
        f.repo().schedule(&actor()).unwrap().revision,
        Revision::default()
    );
    assert!(f
        .handle()
        .try_get("business/receipt/calendar/200".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    f.repo().adopt_calendar(request, &actor()).unwrap();
}
#[test]
fn panicked_execution_callback_is_cancelled_not_an_authority_fault() {
    let f = Fixture::new();
    assert_eq!(
        f.repo()
            .with_calendar_execution_guard(Arc::new(|| panic!("synthetic ownership fence panic")))
            .reserve_calendar_fetch(2026, &actor()),
        Err(CalendarRepositoryError::Cancelled)
    );
    assert_eq!(f.fetch(2026).attempt_sequence, 1);
}
