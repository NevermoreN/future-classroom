use super::*;
use crate::backend::voice::tests as fixtures;
use fc_domain::identity::{AssignmentScope, NewAccount};
use fc_runtime::worker::{JobError, JobTicket, Rejection, Task, WorkerHandle, WorkerPool};
use std::sync::{atomic::AtomicUsize, mpsc};

type Check = Box<dyn FnOnce(&JobContext) -> Result<VoiceManagementSnapshot> + Send>;
struct Work(Check);
impl Task for Work {
    type Output = Result<VoiceManagementSnapshot>;
    fn allocation_bytes(&self) -> Option<usize> {
        Some(std::mem::size_of::<Check>())
    }
    fn run(self, context: &JobContext) -> Self::Output {
        (self.0)(context)
    }
}
fn submit(
    handle: &WorkerHandle<Work>,
    mut work: Work,
    deadline: Instant,
) -> JobTicket<Result<VoiceManagementSnapshot>> {
    loop {
        match handle.try_submit(work, deadline) {
            Ok(ticket) => return ticket,
            Err(error) if error.kind == Rejection::Busy && Instant::now() < deadline => {
                work = error.task;
                std::thread::yield_now();
            }
            Err(error) => panic!("device-check fixture admission {:?}", error.kind),
        }
    }
}
fn run(
    check: impl FnOnce(&JobContext) -> Result<VoiceManagementSnapshot> + Send + 'static,
) -> Result<VoiceManagementSnapshot> {
    let pool = WorkerPool::<Work>::start().unwrap();
    let ticket = submit(
        &pool.handle(),
        Work(Box::new(check)),
        Instant::now() + Duration::from_secs(15),
    );
    let result = ticket.wait().unwrap();
    pool.join().unwrap();
    result
}
fn check(
    service: VoiceService,
    lease: SessionLease,
    review: VoiceManagementSnapshot,
    query: impl FnOnce() -> std::result::Result<DefaultOutputSupport, AudioOutputError> + Send + 'static,
) -> Result<VoiceManagementSnapshot> {
    run(move |context| {
        service.check_default_output_reviewed(
            lease,
            review.publication_revision,
            review.revision,
            review.identity_revision,
            review.runtime_generation,
            context,
            query,
        )
    })
}

#[test]
fn device_observation_is_local_historical_and_never_changes_voice_or_opens_admission() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let lease = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let original = service.management_snapshot(lease).unwrap();
    service
        .set_enabled(lease, false, original.revision)
        .unwrap();
    let review = service.management_snapshot(lease).unwrap();
    let inventory = service.inventory().unwrap();
    let epoch = service.generation();
    for (answer, expected) in [
        (
            Ok(DefaultOutputSupport::Supported),
            VoiceDeviceCheckResult::Supported,
        ),
        (
            Ok(DefaultOutputSupport::FormatUnsupported),
            VoiceDeviceCheckResult::FormatUnsupported,
        ),
        (
            Ok(DefaultOutputSupport::Unavailable),
            VoiceDeviceCheckResult::Unavailable,
        ),
        (
            Err(AudioOutputError::Driver(0xdead_beef)),
            VoiceDeviceCheckResult::Failed,
        ),
        (
            Err(AudioOutputError::CleanupPending),
            VoiceDeviceCheckResult::Failed,
        ),
    ] {
        let query_backend = backend.0.clone();
        let query_service = service.clone();
        let view = check(service.clone(), lease, review.clone(), move || {
            // A native adapter must not inherit a Backend or ledger mutex.
            assert!(query_backend.try_lock().is_ok());
            assert!(query_service.inner.ledger.try_lock().is_ok());
            answer
        })
        .unwrap();
        let observation = view.current_device_observation(lease).unwrap();
        assert_eq!(observation.result, expected);
        assert!(observation.is_fresh());
        assert!(!observation.is_current_for(&view, web));
        assert!(observation.remaining_validity() <= OBSERVATION_LIFETIME);
        assert_eq!(observation.observed_at, service.inner.clock.now());
        assert!(!view.enabled && !view.admitting);
        assert!(observation.current_at(
            &view,
            lease,
            observation.observed + Duration::from_millis(29_999)
        ));
        assert!(!observation.current_at(&view, lease, observation.observed + OBSERVATION_LIFETIME));
        assert!(!observation.current_at(
            &view,
            lease,
            observation.observed - Duration::from_millis(1)
        ));
        let refreshed = service.management_snapshot(lease).unwrap();
        assert!(refreshed.device_observation.is_none());
        assert!(observation.is_current_for(&refreshed, lease));
        for field in ["publication", "identity", "voice", "runtime"] {
            let mut changed = refreshed.clone();
            match field {
                "publication" => changed.publication_revision = Revision::new(999),
                "identity" => changed.identity_revision = Revision::new(999),
                "voice" => changed.revision = Revision::new(999),
                "runtime" => changed.runtime_generation += 1,
                _ => unreachable!(),
            }
            assert!(!observation.is_current_for(&changed, lease));
        }
        assert_eq!(service.inventory().unwrap(), inventory);
        assert_eq!(service.generation(), epoch);
    }
    let fresh = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let next = service.management_snapshot(fresh).unwrap();
    assert!(next.device_observation.is_none());
}

#[test]
fn stale_reviews_and_unauthorized_leases_never_invoke_the_native_query() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let review = service.management_snapshot(local).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let probe = |lease, review| {
        let called = calls.clone();
        assert!(check(service.clone(), lease, review, move || {
            called.fetch_add(1, Ordering::AcqRel);
            Ok(DefaultOutputSupport::Supported)
        })
        .is_err());
    };
    probe(web, review.clone());
    for kind in ["publication", "identity", "voice", "runtime"] {
        let mut stale = review.clone();
        match kind {
            "publication" => stale.publication_revision = Revision::new(999),
            "identity" => stale.identity_revision = Revision::new(999),
            "voice" => stale.revision = Revision::new(999),
            "runtime" => stale.runtime_generation += 1,
            _ => unreachable!(),
        }
        probe(local, stale);
    }
    let replacement = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    probe(local, review.clone());
    backend
        .0
        .lock()
        .unwrap()
        .ledger
        .lock()
        .unwrap()
        .sessions
        .revoke_exact(replacement);
    probe(replacement, review.clone());
    let expired = super::super::tests::issue_account(
        &backend.0.lock().unwrap(),
        1,
        TrustedOrigin::Local,
        true,
    );
    probe(expired, review);
    assert_eq!(calls.load(Ordering::Acquire), 0);
}

#[test]
fn device_checks_require_global_role_and_password_change_without_querying() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let admin = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let review = service.management_snapshot(admin).unwrap();
    let password = "Synthetic device manager 2026!";
    backend
        .0
        .lock()
        .unwrap()
        .create_account(
            admin,
            NewAccount {
                id: PrincipalId::new(2).unwrap(),
                login_name: "device-manager".into(),
                display_name: "Device manager".into(),
                role: Role::GlobalAdmin,
                scopes: vec![],
            },
            Password::new(password.into()).unwrap(),
        )
        .unwrap();
    let must_change = super::super::tests::issue_account(
        &backend.0.lock().unwrap(),
        2,
        TrustedOrigin::Local,
        false,
    );
    assert!(
        check(service.clone(), must_change, review.clone(), || panic!(
            "must-change query"
        ))
        .is_err()
    );
    backend
        .0
        .lock()
        .unwrap()
        .change_password(
            must_change,
            Password::new(password.into()).unwrap(),
            Password::new("Changed synthetic device manager 2026!".into()).unwrap(),
        )
        .unwrap();
    let admin = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let subject = SubjectId::new(1).unwrap();
    backend
        .0
        .lock()
        .unwrap()
        .identity(
            admin,
            IdentityCommand::PutSubject {
                id: subject,
                name: "Synthetic device subject".into(),
                enabled: true,
                color: fc_domain::identity::Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                expected_revision: None,
            },
        )
        .unwrap();
    for (role, scopes) in [
        (Role::User, vec![]),
        (
            Role::TeacherAdmin,
            vec![AssignmentScope {
                class_id: class,
                subject_id: subject,
            }],
        ),
    ] {
        backend
            .0
            .lock()
            .unwrap()
            .identity(
                admin,
                IdentityCommand::UpdateAccount {
                    id: PrincipalId::new(2).unwrap(),
                    display_name: "Limited device account".into(),
                    role,
                    enabled: true,
                    scopes,
                },
            )
            .unwrap();
        let review = service.management_snapshot(admin).unwrap();
        let limited = super::super::tests::issue_account(
            &backend.0.lock().unwrap(),
            2,
            TrustedOrigin::Local,
            false,
        );
        assert!(check(service.clone(), limited, review, || panic!("limited query")).is_err());
        // Restore the original active administrator without issuing an old lease.
        service
            .inner
            .control
            .0
            .active_local
            .store(admin.id(), Ordering::Release);
        service
            .inner
            .control
            .0
            .active_principal
            .store(admin.principal_id().get(), Ordering::Release);
    }
}

#[test]
fn native_results_after_lock_revoke_publication_voice_change_or_epoch_change_are_discarded() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    for kind in [
        "lock", "revoke", "identity", "voice", "runtime", "fault", "request",
    ] {
        let (_directory, backend, web, class) = fixtures::shared_fixture();
        let service = backend.0.lock().unwrap().voice_service();
        let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
        let review = service.management_snapshot(local).unwrap();
        let original = service.inventory().unwrap();
        let action_service = service.clone();
        let action_backend = backend.0.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let current = alive.clone();
        let scoped =
            service.with_execution_fence(Arc::new(move || current.load(Ordering::Acquire)));
        let result = check(scoped, local, review, move || {
            match kind {
                "lock" => action_service.inner.control.lock_local(),
                "revoke" => {
                    assert!(action_service
                        .inner
                        .ledger
                        .lock()
                        .unwrap()
                        .sessions
                        .revoke_exact(local));
                }
                "identity" => {
                    action_backend
                        .lock()
                        .unwrap()
                        .identity(
                            local,
                            IdentityCommand::PutSubject {
                                id: SubjectId::new(1).unwrap(),
                                name: "Changed device observation authority".into(),
                                enabled: true,
                                color: fc_domain::identity::Rgb {
                                    red: 1,
                                    green: 2,
                                    blue: 3,
                                },
                                expected_revision: None,
                            },
                        )
                        .unwrap();
                }
                "voice" => {
                    let request = fixtures::independent_request(&action_service, web, class, 987);
                    action_service.submit(web, request).unwrap();
                }
                "runtime" => {
                    action_service.invalidate_runtime();
                }
                "fault" => action_service.inner.control.health_gate().fault(),
                "request" => alive.store(false, Ordering::Release),
                _ => unreachable!(),
            }
            Ok(DefaultOutputSupport::Supported)
        });
        assert!(result.is_err(), "late {kind} must not publish");
        if matches!(kind, "request" | "revoke" | "lock") {
            assert_eq!(service.inventory().unwrap(), original);
        }
    }
}

#[test]
fn cancellation_and_original_deadline_suppress_late_query_result_but_keep_worker_until_return() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let review = service.management_snapshot(local).unwrap();
    let original = service.inventory().unwrap();
    let pool = WorkerPool::<Work>::start().unwrap();
    let (entered, begun) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let action_service = service.clone();
    let action_review = review.clone();
    let rejected = Arc::new(AtomicBool::new(false));
    let observed_rejection = rejected.clone();
    let ticket = submit(
        &pool.handle(),
        Work(Box::new(move |context| {
            let result = action_service.check_default_output_reviewed(
                local,
                action_review.publication_revision,
                action_review.revision,
                action_review.identity_revision,
                action_review.runtime_generation,
                context,
                || {
                    entered.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(DefaultOutputSupport::Supported)
                },
            );
            observed_rejection.store(
                matches!(
                    result,
                    Err(BackendError::Storage(StorageError::ExecutionCancelled))
                ),
                Ordering::Release,
            );
            result
        })),
        Instant::now() + Duration::from_secs(15),
    );
    begun.recv_timeout(Duration::from_secs(5)).unwrap();
    ticket.cancel();
    assert!(
        !ticket.is_finished(),
        "native query still owns the running worker"
    );
    release.send(()).unwrap();
    assert_eq!(ticket.wait(), Err(JobError::Cancelled));
    pool.join().unwrap();
    assert!(rejected.load(Ordering::Acquire));
    let pool = WorkerPool::<Work>::start().unwrap();
    let ticket = submit(
        &pool.handle(),
        Work(Box::new(move |context| {
            service.check_default_output_reviewed(
                local,
                review.publication_revision,
                review.revision,
                review.identity_revision,
                review.runtime_generation,
                context,
                || {
                    while Instant::now() < context.deadline() {
                        std::thread::yield_now();
                    }
                    Ok(DefaultOutputSupport::Supported)
                },
            )
        })),
        Instant::now() + Duration::from_millis(500),
    );
    assert_eq!(ticket.wait(), Err(JobError::Deadline));
    pool.join().unwrap();
    assert_eq!(
        backend
            .0
            .lock()
            .unwrap()
            .voice_service()
            .inventory()
            .unwrap(),
        original
    );
}

struct Tick(AtomicU64);
impl MonotonicClock for Tick {
    fn monotonic_millis(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}

#[test]
fn monotonic_regression_during_the_query_cannot_publish_an_observation() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let original = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let clock = Arc::new(Tick(AtomicU64::new(100)));
    let service = VoiceService::new(
        original.inner.repository.clone(),
        original.inner.ledger.clone(),
        original.inner.control.clone(),
        original.inner.authority.clone(),
        original.inner.clock.clone(),
        clock.clone(),
    );
    let review = service.management_snapshot(local).unwrap();
    let epoch = service.generation();
    assert!(check(service.clone(), local, review, move || {
        clock.0.store(99, Ordering::Release);
        Ok(DefaultOutputSupport::Supported)
    })
    .is_err());
    assert_eq!(service.generation(), epoch);
    assert!(service.inner.enabled.load(Ordering::Acquire));
}

#[test]
fn saving_policy_invalidates_an_observation_and_lock_cannot_rebind_it_to_a_new_lease() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let review = service.management_snapshot(local).unwrap();
    let checked = check(service.clone(), local, review, || {
        Ok(DefaultOutputSupport::Supported)
    })
    .unwrap();
    let observation = checked.device_observation.clone().unwrap();
    service
        .set_limits_reviewed(
            local,
            VoiceLimits::default(),
            checked.revision,
            checked.identity_revision,
            checked.runtime_generation,
        )
        .unwrap();
    let saved = service.management_snapshot(local).unwrap();
    assert!(!observation.is_current_for(&saved, local));
    assert!(saved.device_observation.is_none());
    service.inner.control.lock_local();
    assert!(service.management_snapshot(local).is_err());
    // Re-enter through real login: synchronizing the lock generation and
    // installing the new input lease must precede validating the replacement.
    // The generic issue() helper deliberately bypasses that production path.
    let replacement = backend
        .0
        .lock()
        .unwrap()
        .login(
            "admin",
            Password::new("Independent voice password 2026!".into()).unwrap(),
        )
        .unwrap();
    assert_ne!(replacement, local);
    assert!(service.management_snapshot(local).is_err());
    let fresh = service.management_snapshot(replacement).unwrap();
    assert!(!observation.is_current_for(&fresh, replacement));
    assert!(!observation.is_current_for(&checked, replacement));
}
