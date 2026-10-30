//! Autostart intent transactions use synthetic identity; no real system writes.
use super::*;
use fc_domain::time::{LocalDate, LocalDateTime};
use fc_ports::AuthorityPublisher;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "FutureClassroom-Autostart-R21-{}-{}",
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
            school_name: "autostart synthetic".into(),
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
}
fn request(id: u64, revision: Revision, desired: bool) -> AutostartRequest {
    AutostartRequest {
        command_id: CommandId::new(id).unwrap(),
        expected_revision: revision,
        desired,
        target_command: "\"C:\\教室 空间\\FutureClassroom.exe\" --autostart".into(),
        previous_observed: AutostartObservation::Absent,
    }
}
fn success(op: &AutostartOperation) -> AutostartSystemOutcome {
    AutostartSystemOutcome {
        observation: if op.desired {
            AutostartObservation::Command(op.target_command.clone())
        } else {
            AutostartObservation::Absent
        },
        failure: None,
        mutated: true,
    }
}
#[test]
fn default_has_no_intent_and_no_os_action() {
    let f = Fixture::new();
    let s = f.repo().autostart_status(&actor()).unwrap();
    assert_eq!(s, AutostartState::default());
}
#[test]
fn prepare_apply_finish_is_durable_and_keeps_exact_intent() {
    let mut f = Fixture::new();
    let mut r = f.repo();
    let op = r
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    assert_eq!(op.phase, AutostartPhase::Prepared);
    assert_eq!(op.ticket.prepared_revision.get(), 1);
    f.reopen();
    let mut r = f.repo();
    assert_eq!(r.autostart_status(&actor()).unwrap().current(), Some(&op));
    let applying = r
        .mark_autostart_applying(op.ticket.clone(), &actor())
        .unwrap();
    assert_eq!(applying.phase, AutostartPhase::Applying);
    let s = r.finish_autostart(op.ticket.clone(), success(&op)).unwrap();
    assert_eq!(s.current().unwrap().phase, AutostartPhase::Succeeded);
    assert!(!s.drifted());
    assert_eq!(s.revision.get(), 3);
    f.reopen();
    assert_eq!(f.repo().autostart_status(&actor()).unwrap(), s);
}
#[test]
fn read_and_preparation_require_local_current_global_lease() {
    let f = Fixture::new();
    let mut r = f.repo();
    let mut web = actor();
    web.origin = TrustedOrigin::Web;
    assert_eq!(
        r.autostart_status(&web),
        Err(AutostartRepositoryError::Denied)
    );
    assert_eq!(
        r.begin_autostart(request(1, Revision::default(), true), &web),
        Err(AutostartRepositoryError::Denied)
    );
    f.active.store(false, Ordering::Release);
    assert_eq!(
        r.autostart_status(&actor()),
        Err(AutostartRepositoryError::Denied)
    );
}
#[test]
fn committed_preparation_is_fenced_at_real_precommit() {
    let f = Fixture::new();
    f.revoke.store(true, Ordering::Release);
    assert_eq!(
        f.repo()
            .begin_autostart(request(1, Revision::default(), true), &actor()),
        Err(AutostartRepositoryError::Denied)
    );
    f.active.store(true, Ordering::Release);
    assert_eq!(
        f.repo().autostart_status(&actor()).unwrap(),
        AutostartState::default()
    );
}
#[test]
fn execution_cancel_is_not_health_fault_and_no_intent_is_committed() {
    let f = Fixture::new();
    let mut r = f.repo().with_autostart_execution_guard(Arc::new(|| false));
    assert_eq!(
        r.begin_autostart(request(1, Revision::default(), true), &actor()),
        Err(AutostartRepositoryError::Cancelled)
    );
    assert_eq!(
        f.repo().autostart_status(&actor()).unwrap(),
        AutostartState::default()
    );
}
#[test]
fn execution_guard_is_checked_after_factory_at_precommit() {
    let f = Fixture::new();
    let active = Arc::new(AtomicBool::new(true));
    let flip = active.clone();
    let mut r = f.repo();
    r.factory = Arc::new(move |_, rev| {
        flip.store(false, Ordering::Release);
        Ok(rev)
    });
    r = r.with_autostart_execution_guard(Arc::new(move || active.load(Ordering::Acquire)));
    assert_eq!(
        r.begin_autostart(request(1, Revision::default(), true), &actor()),
        Err(AutostartRepositoryError::Cancelled)
    );
    assert_eq!(
        f.repo().autostart_status(&actor()).unwrap(),
        AutostartState::default()
    );
}
#[test]
fn applying_rechecks_authorization_and_exact_owner() {
    let f = Fixture::new();
    let op = f
        .repo()
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    f.revoke.store(true, Ordering::Release);
    assert_eq!(
        f.repo()
            .mark_autostart_applying(op.ticket.clone(), &actor()),
        Err(AutostartRepositoryError::Denied)
    );
    f.active.store(true, Ordering::Release);
    assert_eq!(
        f.repo()
            .autostart_status(&actor())
            .unwrap()
            .current()
            .unwrap()
            .phase,
        AutostartPhase::Prepared
    );
}
#[test]
fn finish_can_record_physical_success_after_lock_and_cancellation() {
    let f = Fixture::new();
    let mut r = f.repo();
    let op = r
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    r.mark_autostart_applying(op.ticket.clone(), &actor())
        .unwrap();
    f.active.store(false, Ordering::Release);
    let s = r
        .with_autostart_execution_guard(Arc::new(|| false))
        .finish_autostart(op.ticket.clone(), success(&op))
        .unwrap();
    assert_eq!(s.current().unwrap().phase, AutostartPhase::Succeeded);
}
#[test]
fn prepared_cancellation_settles_but_cannot_claim_mutation() {
    let f = Fixture::new();
    let mut r = f.repo();
    let op = r
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    assert_eq!(
        r.finish_autostart(op.ticket.clone(), success(&op)),
        Err(AutostartRepositoryError::Invalid)
    );
    let s = r
        .finish_autostart(
            op.ticket,
            AutostartSystemOutcome {
                observation: AutostartObservation::Absent,
                failure: Some(AutostartFailure::Cancelled),
                mutated: false,
            },
        )
        .unwrap();
    assert_eq!(s.current().unwrap().phase, AutostartPhase::Failed);
    assert!(s.desired);
    assert!(s.drifted());
}
#[test]
fn stale_ticket_cannot_settle_new_operation() {
    let f = Fixture::new();
    let mut r = f.repo();
    let first = r
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    let s = r
        .finish_autostart(
            first.ticket.clone(),
            AutostartSystemOutcome {
                observation: AutostartObservation::Absent,
                failure: Some(AutostartFailure::Cancelled),
                mutated: false,
            },
        )
        .unwrap();
    let second = r
        .begin_autostart(request(2, s.revision, false), &actor())
        .unwrap();
    assert!(second.ticket.sequence > first.ticket.sequence);
    assert_eq!(
        r.finish_autostart(first.ticket, success(&second)),
        Err(AutostartRepositoryError::Stale)
    );
}
#[test]
fn pending_operation_and_wrong_revision_prevent_replacement() {
    let f = Fixture::new();
    let mut r = f.repo();
    r.begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    assert_eq!(
        r.begin_autostart(request(2, Revision::new(1), false), &actor()),
        Err(AutostartRepositoryError::Stale)
    );
    assert_eq!(
        r.begin_autostart(request(3, Revision::default(), false), &actor()),
        Err(AutostartRepositoryError::Stale)
    );
}
#[test]
fn exact_begin_replay_never_performs_second_operation() {
    let f = Fixture::new();
    let mut r = f.repo();
    let req = request(1, Revision::default(), true);
    let first = r.begin_autostart(req.clone(), &actor()).unwrap();
    let global = f.handle().try_revision().unwrap().wait().unwrap();
    assert_eq!(r.begin_autostart(req, &actor()).unwrap(), first);
    assert_eq!(f.handle().try_revision().unwrap().wait().unwrap(), global);
    let mut altered = request(1, Revision::default(), false);
    altered.previous_observed = AutostartObservation::Command("other".into());
    assert_eq!(
        r.begin_autostart(altered, &actor()),
        Err(AutostartRepositoryError::Stale)
    );
}
#[test]
fn restart_prepared_is_never_mistaken_for_proven_system_success() {
    let mut f = Fixture::new();
    let op = f
        .repo()
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    f.reopen();
    let mut r = f.repo();
    let s = r.autostart_reconciliation_status().unwrap();
    let s = r
        .reconcile_autostart(s.revision, AutostartObservation::Command(op.target_command))
        .unwrap();
    assert_eq!(s.current().unwrap().phase, AutostartPhase::NeedsReconcile);
    assert_eq!(
        s.current().unwrap().failure,
        Some(AutostartFailure::Interrupted)
    );
}
#[test]
fn restart_applying_exact_readback_proves_fact_but_mismatch_needs_reconcile() {
    for matching in [false, true] {
        let mut f = Fixture::new();
        let mut r = f.repo();
        let op = r
            .begin_autostart(request(1, Revision::default(), true), &actor())
            .unwrap();
        r.mark_autostart_applying(op.ticket.clone(), &actor())
            .unwrap();
        f.reopen();
        let mut r = f.repo();
        let s = r.autostart_reconciliation_status().unwrap();
        let observed = if matching {
            AutostartObservation::Command(op.target_command.clone())
        } else {
            AutostartObservation::Absent
        };
        let s = r.reconcile_autostart(s.revision, observed).unwrap();
        assert_eq!(
            s.current().unwrap().phase,
            if matching {
                AutostartPhase::Succeeded
            } else {
                AutostartPhase::NeedsReconcile
            }
        );
        assert_eq!(
            r.finish_autostart(op.ticket, success(s.current().unwrap())),
            Err(AutostartRepositoryError::Stale)
        );
    }
}
#[test]
fn external_removal_records_drift_without_rebuilding_intent() {
    let f = Fixture::new();
    let mut r = f.repo();
    let op = r
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    r.mark_autostart_applying(op.ticket.clone(), &actor())
        .unwrap();
    let s = r.finish_autostart(op.ticket.clone(), success(&op)).unwrap();
    let history = s.history.clone();
    let s = r
        .reconcile_autostart(s.revision, AutostartObservation::Absent)
        .unwrap();
    assert!(s.drifted());
    assert!(s.desired);
    assert_eq!(s.history, history);
    assert_eq!(s.target_command, Some(op.target_command));
}
#[test]
fn malformed_or_unavailable_preimage_and_nonfixed_command_are_rejected() {
    let f = Fixture::new();
    for previous in [
        AutostartObservation::Opaque { kind: 3, bytes: 42 },
        AutostartObservation::default(),
        AutostartObservation::Command("x".repeat(260)),
    ] {
        let mut req = request(1, Revision::default(), true);
        req.previous_observed = previous;
        assert_eq!(
            f.repo().begin_autostart(req, &actor()),
            Err(AutostartRepositoryError::Invalid)
        );
    }
    let mut req = request(2, Revision::default(), true);
    req.target_command = "cmd.exe /c anything".into();
    assert_eq!(
        f.repo().begin_autostart(req, &actor()),
        Err(AutostartRepositoryError::Invalid)
    );
}
#[test]
fn history_is_bounded_and_sequence_does_not_reset_when_trimmed() {
    let f = Fixture::new();
    let mut r = f.repo();
    let mut rev = Revision::default();
    for id in 1..=40 {
        let op = r.begin_autostart(request(id, rev, true), &actor()).unwrap();
        let s = r
            .finish_autostart(
                op.ticket,
                AutostartSystemOutcome {
                    observation: AutostartObservation::Absent,
                    failure: Some(AutostartFailure::Cancelled),
                    mutated: false,
                },
            )
            .unwrap();
        rev = s.revision;
        assert!(s.history.len() <= 32);
    }
    let s = r.autostart_status(&actor()).unwrap();
    assert_eq!(s.history.len(), 32);
    assert_eq!(s.sequence, 40);
    assert_eq!(s.history[0].ticket.sequence, 9);
    assert!(encode(&s).unwrap().len() <= MAX_VALUE_BYTES);
}
#[test]
fn strict_codec_and_startup_detect_malformed_or_uninstalled_data() {
    let f = Fixture::new();
    let s = AutostartState::default();
    let mut v = dto::state(&s);
    v["extra"] = json!(true);
    assert!(dto::read_state(&v).is_err());
    assert!(codec::value(br#"{"version":1,"version":1}"#).is_err());
    f.commit(vec![Mutation::Put {
        key: DOCUMENT.into(),
        value: br#"{"wrong":true}"#.to_vec(),
    }]);
    assert!(f.repo().autostart_reconciliation_status().is_err());
    assert!(startup_state(f.directory.0.join("state.sqlite3")).is_err());
}
#[test]
fn actual_role_epoch_password_and_expiry_are_rechecked_not_cached_buttons() {
    for variant in 0..5 {
        let f = Fixture::new();
        let mut identity = identity();
        let account = &mut identity.accounts[0];
        match variant {
            0 => account.principal.role = Role::TeacherAdmin,
            1 => account.principal.role = Role::User,
            2 => account.principal.auth_epoch = AuthEpoch::new(2),
            3 => account.must_change_password = true,
            _ => account.principal.enabled = false,
        };
        if variant == 0 {
            identity.classes.push(SchoolClass {
                id: ClassId::new(1).unwrap(),
                school_id: actor().school_id,
                name: "synthetic".into(),
                enabled: true,
            });
            identity.subjects.push(SchoolSubject {
                id: SubjectId::new(1).unwrap(),
                school_id: actor().school_id,
                name: "synthetic".into(),
                enabled: true,
                color: Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                revision: Revision::new(1),
            });
            identity.assignments.push(TeachingAssignment {
                teacher_id: actor().principal_id,
                class_id: ClassId::new(1).unwrap(),
                subject_id: SubjectId::new(1).unwrap(),
                enabled: true,
            });
        }
        identity.validate().unwrap();
        f.commit(vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&identity)).unwrap(),
        }]);
        assert_eq!(
            f.repo()
                .begin_autostart(request(1, Revision::default(), true), &actor()),
            Err(AutostartRepositoryError::Denied)
        );
        assert_eq!(
            f.repo().autostart_status(&actor()),
            Err(AutostartRepositoryError::Denied)
        );
    }
    let f = Fixture::new();
    *f.clock.0.lock().unwrap() = actor().expires_at;
    assert_eq!(
        f.repo()
            .begin_autostart(request(1, Revision::default(), true), &actor()),
        Err(AutostartRepositoryError::Denied)
    );
}
#[test]
fn forged_ticket_owner_sequence_or_revision_is_never_a_finish_capability() {
    let f = Fixture::new();
    let mut r = f.repo();
    let op = r
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    r.mark_autostart_applying(op.ticket.clone(), &actor())
        .unwrap();
    for variant in 0..5 {
        let mut ticket = op.ticket.clone();
        match variant {
            0 => ticket.command_id = CommandId::new(2).unwrap(),
            1 => ticket.sequence += 1,
            2 => ticket.prepared_revision = Revision::new(3),
            3 => ticket.principal_id = PrincipalId::new(2).unwrap(),
            _ => ticket.auth_epoch = AuthEpoch::new(2),
        };
        assert_eq!(
            r.finish_autostart(ticket, success(&op)),
            Err(AutostartRepositoryError::Stale)
        );
    }
    assert_eq!(
        r.autostart_status(&actor())
            .unwrap()
            .current()
            .unwrap()
            .phase,
        AutostartPhase::Applying
    );
}
#[test]
fn each_system_failure_preserves_desired_and_factual_readback() {
    for failure in [
        AutostartFailure::AccessDenied,
        AutostartFailure::InvalidValue,
        AutostartFailure::PathUnavailable,
        AutostartFailure::ReadFailed,
        AutostartFailure::WriteFailed,
        AutostartFailure::ReadbackMismatch,
        AutostartFailure::PreimageChanged,
        AutostartFailure::Cancelled,
        AutostartFailure::Interrupted,
        AutostartFailure::Unavailable,
    ] {
        for mutated in [false, true] {
            let f = Fixture::new();
            let mut r = f.repo();
            let op = r
                .begin_autostart(request(1, Revision::default(), true), &actor())
                .unwrap();
            r.mark_autostart_applying(op.ticket.clone(), &actor())
                .unwrap();
            let s = r
                .finish_autostart(
                    op.ticket,
                    AutostartSystemOutcome {
                        observation: AutostartObservation::Absent,
                        failure: Some(failure),
                        mutated,
                    },
                )
                .unwrap();
            assert!(s.desired);
            let o = s.current().unwrap();
            assert_eq!(o.failure, Some(failure));
            assert_eq!(o.observed, Some(AutostartObservation::Absent));
            assert_eq!(
                o.phase,
                if mutated {
                    AutostartPhase::NeedsReconcile
                } else {
                    AutostartPhase::Failed
                }
            );
        }
    }
}
#[test]
fn untrusted_observation_cannot_be_reported_as_success() {
    for observed in [
        AutostartObservation::Opaque { kind: 3, bytes: 42 },
        AutostartObservation::Unavailable(AutostartFailure::ReadFailed),
        AutostartObservation::Command("another app".into()),
        AutostartObservation::Absent,
    ] {
        let f = Fixture::new();
        let mut r = f.repo();
        let op = r
            .begin_autostart(request(1, Revision::default(), true), &actor())
            .unwrap();
        r.mark_autostart_applying(op.ticket.clone(), &actor())
            .unwrap();
        let s = r
            .finish_autostart(
                op.ticket,
                AutostartSystemOutcome {
                    observation: observed.clone(),
                    failure: None,
                    mutated: true,
                },
            )
            .unwrap();
        assert_eq!(s.observed, observed);
        assert_eq!(s.current().unwrap().phase, AutostartPhase::NeedsReconcile);
        assert_eq!(
            s.current().unwrap().failure,
            Some(AutostartFailure::ReadbackMismatch)
        );
    }
}
#[test]
fn reconcile_is_revision_cas_and_repeated_unchanged_read_has_no_commit() {
    let f = Fixture::new();
    let mut r = f.repo();
    let s = r
        .reconcile_autostart(Revision::default(), AutostartObservation::Absent)
        .unwrap();
    let global = f.handle().try_revision().unwrap().wait().unwrap();
    assert_eq!(
        r.reconcile_autostart(
            Revision::default(),
            AutostartObservation::Command("external".into())
        ),
        Err(AutostartRepositoryError::Stale)
    );
    assert_eq!(
        r.reconcile_autostart(s.revision, AutostartObservation::Absent)
            .unwrap(),
        s
    );
    assert_eq!(f.handle().try_revision().unwrap().wait().unwrap(), global);
}
#[test]
fn encoded_budget_rejects_whole_operation_without_truncating_history_or_intent() {
    let f = Fixture::new();
    let mut r = f.repo();
    let target = format!("\"C:\\{}.exe\" --autostart", "中".repeat(220));
    assert!(valid_autostart_target(&target));
    let previous = AutostartObservation::Command("汉".repeat(259));
    let mut state = r.autostart_status(&actor()).unwrap();
    let mut capacity = false;
    for id in 1..=40 {
        let mut req = request(id, state.revision, true);
        req.target_command = target.clone();
        req.previous_observed = previous.clone();
        match r.begin_autostart(req, &actor()) {
            Err(AutostartRepositoryError::Capacity) => {
                capacity = true;
                assert_eq!(r.autostart_status(&actor()).unwrap(), state);
                break;
            }
            Ok(op) => {
                r.mark_autostart_applying(op.ticket.clone(), &actor())
                    .unwrap();
                state = r.finish_autostart(op.ticket.clone(), success(&op)).unwrap();
                assert!(encode(&state).unwrap().len() <= MAX_VALUE_BYTES);
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(
        capacity,
        "fixture must reach 64KiB without modifying a partial intent"
    );
}
#[test]
fn startup_rejects_autostart_metadata_before_installation_initialization() {
    let f = Fixture::new();
    f.commit(vec![
        Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&IdentityState::default())).unwrap(),
        },
        Mutation::Put {
            key: DOCUMENT.into(),
            value: encode(&AutostartState::default()).unwrap(),
        },
    ]);
    assert!(startup_state(f.directory.0.join("state.sqlite3")).is_err());
}
#[test]
fn codec_rejects_phase_sequence_duplicate_command_and_unknown_fields() {
    let f = Fixture::new();
    let mut r = f.repo();
    let op = r
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    let state = r.autostart_status(&actor()).unwrap();
    for variant in 0..5 {
        let mut v = dto::state(&state);
        match variant {
            0 => v["history"][0]["ticket"]["sequence"] = json!(0),
            1 => v["history"][0]["phase"] = json!("succeeded"),
            2 => v["history"][0]["ticket"]["extra"] = json!(1),
            3 => v["history"][0]["failure"] = json!("anything"),
            _ => v["history"][0]["previous"] = json!({"kind":"absent","extra":true}),
        };
        assert!(dto::read_state(&v).is_err());
    }
    let s = r
        .finish_autostart(
            op.ticket,
            AutostartSystemOutcome {
                observation: AutostartObservation::Absent,
                failure: Some(AutostartFailure::Cancelled),
                mutated: false,
            },
        )
        .unwrap();
    let mut v = dto::state(&s);
    let mut duplicate = v["history"][0].clone();
    duplicate["ticket"]["sequence"] = json!(2);
    duplicate["ticket"]["prepared_revision"] = json!(3);
    v["history"].as_array_mut().unwrap().push(duplicate);
    v["sequence"] = json!(2);
    v["revision"] = json!(3);
    assert!(dto::read_state(&v).is_err());
}
#[test]
fn final_database_failure_keeps_applying_for_factual_startup_reconciliation() {
    let mut f = Fixture::new();
    let mut r = f.repo();
    let op = r
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    r.mark_autostart_applying(op.ticket.clone(), &actor())
        .unwrap();
    r.factory = Arc::new(|_, _| Err(StorageError::Database));
    assert_eq!(
        r.finish_autostart(op.ticket.clone(), success(&op)),
        Err(AutostartRepositoryError::Unavailable)
    );
    assert_eq!(
        f.repo()
            .autostart_status(&actor())
            .unwrap()
            .current()
            .unwrap()
            .phase,
        AutostartPhase::Applying
    );
    f.reopen();
    let mut r = f.repo();
    let current = r.autostart_reconciliation_status().unwrap();
    let s = r
        .reconcile_autostart(
            current.revision,
            AutostartObservation::Command(op.target_command),
        )
        .unwrap();
    assert_eq!(s.current().unwrap().phase, AutostartPhase::Succeeded);
}
#[test]
fn applying_execution_fence_at_precommit_does_not_authorize_effect() {
    let f = Fixture::new();
    let op = f
        .repo()
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    let active = Arc::new(AtomicBool::new(true));
    let flip = active.clone();
    let mut r = f.repo();
    r.factory = Arc::new(move |_, revision| {
        flip.store(false, Ordering::Release);
        Ok(revision)
    });
    let mut r = r.with_autostart_execution_guard(Arc::new(move || active.load(Ordering::Acquire)));
    assert_eq!(
        r.mark_autostart_applying(op.ticket, &actor()),
        Err(AutostartRepositoryError::Cancelled)
    );
    assert_eq!(
        f.repo()
            .autostart_status(&actor())
            .unwrap()
            .current()
            .unwrap()
            .phase,
        AutostartPhase::Prepared
    );
}
#[test]
fn exact_success_finish_replay_is_read_only_but_altered_outcome_is_stale() {
    let f = Fixture::new();
    let mut r = f.repo();
    let op = r
        .begin_autostart(request(1, Revision::default(), true), &actor())
        .unwrap();
    r.mark_autostart_applying(op.ticket.clone(), &actor())
        .unwrap();
    let outcome = success(&op);
    let state = r
        .finish_autostart(op.ticket.clone(), outcome.clone())
        .unwrap();
    let global = f.handle().try_revision().unwrap().wait().unwrap();
    assert_eq!(
        r.finish_autostart(op.ticket.clone(), outcome).unwrap(),
        state
    );
    assert_eq!(f.handle().try_revision().unwrap().wait().unwrap(), global);
    assert_eq!(
        r.finish_autostart(
            op.ticket,
            AutostartSystemOutcome {
                observation: AutostartObservation::Absent,
                failure: None,
                mutated: false
            }
        ),
        Err(AutostartRepositoryError::Stale)
    );
}
