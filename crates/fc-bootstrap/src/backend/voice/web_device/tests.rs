//! Real SQLite/session service and original two-worker ownership; no device opens.
use super::*;
use crate::backend::voice::tests as fixtures;
use fc_runtime::worker::{
    JobContext, JobError, JobTicket, Rejection, Task, WorkerHandle, WorkerPool,
};
use std::sync::{atomic::AtomicUsize, mpsc};

fn due() -> Instant {
    Instant::now() + Duration::from_secs(15)
}
fn clear(service: &VoiceService) {
    // Independent classification cases do not pretend to be cache invalidation.
    *service.inner.output_observation.observation.lock().unwrap() = None;
}
fn empty(service: &VoiceService) -> bool {
    service
        .inner
        .output_observation
        .observation
        .lock()
        .unwrap()
        .is_none()
}

#[test]
fn negative_output_fact_is_shared_but_every_request_still_needs_its_exact_web_session() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    let answer = service
        .web_capabilities_with_output(web, due(), || Ok(DefaultOutputSupport::FormatUnsupported))
        .unwrap();
    assert_eq!(
        answer.device_availability,
        WebVoiceDeviceAvailability::Unavailable
    );
    let original = service
        .inner
        .output_observation
        .observation
        .lock()
        .unwrap()
        .unwrap();
    assert_eq!(
        service.web_capabilities(web).unwrap().device_availability,
        WebVoiceDeviceAvailability::Unverified,
        "old read API never inherits a device claim"
    );
    let next = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Web);
    assert!(service
        .web_capabilities_with_output(web, due(), || panic!("old lease query"))
        .is_err());
    let hit = service
        .web_capabilities_with_output(next, due(), || panic!("cache hit query"))
        .unwrap();
    assert_eq!(
        hit.device_availability,
        WebVoiceDeviceAvailability::Unavailable
    );
    assert_eq!(
        *service.inner.output_observation.observation.lock().unwrap(),
        Some(original)
    );
    assert_eq!(service.inventory().unwrap(), before);
    assert_eq!(service.generation(), epoch);
    assert!(service.is_admitting());
    assert!(!service.inner.native_participating.load(Ordering::Acquire));
}

#[test]
fn supported_driver_error_and_query_panic_remain_unverified_without_fault_or_sql_mutation() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    for mode in 0..4 {
        clear(&service);
        let result = service
            .web_capabilities_with_output(web, due(), || match mode {
                0 => Ok(DefaultOutputSupport::Supported),
                1 => Err(AudioOutputError::Driver(0xdead_beef)),
                2 => panic!("synthetic query panic, no native owner"),
                _ => Ok(DefaultOutputSupport::Unavailable),
            })
            .unwrap();
        let expected = if mode == 3 {
            WebVoiceDeviceAvailability::Unavailable
        } else {
            WebVoiceDeviceAvailability::Unverified
        };
        assert_eq!(result.device_availability, expected);
        assert_eq!(
            service
                .web_capabilities_with_output(web, due(), || panic!("same fact reused"))
                .unwrap()
                .device_availability,
            expected
        );
        assert!(!service
            .inner
            .output_observation
            .in_flight
            .load(Ordering::Acquire));
        assert_eq!(service.inventory().unwrap(), before);
        assert_eq!(service.generation(), epoch);
        assert_eq!(
            service.inner.control.health_gate().status().health,
            Health::Healthy
        );
    }
}

#[test]
fn native_query_runs_without_backend_ledger_cache_or_writer_locks() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let proof = service.clone();
    let result = service
        .web_capabilities_with_output(web, due(), || {
            drop(backend.0.try_lock().expect("Backend free across driver"));
            drop(
                proof
                    .inner
                    .ledger
                    .try_lock()
                    .expect("ledger free across driver"),
            );
            drop(
                proof
                    .inner
                    .sessions
                    .try_lock()
                    .expect("session map free across driver"),
            );
            drop(
                proof
                    .inner
                    .output_observation
                    .observation
                    .try_lock()
                    .expect("cache free across driver"),
            );
            // A real writer roundtrip must remain possible while this query is live.
            assert!(proof.inventory().unwrap().jobs.is_empty());
            Ok(DefaultOutputSupport::Unavailable)
        })
        .unwrap();
    assert_eq!(
        result.device_availability,
        WebVoiceDeviceAvailability::Unavailable
    );
}

#[test]
fn disabled_closed_wrong_origin_and_cancelled_requests_never_query_or_reuse_negative_fact() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    assert!(service
        .web_capabilities_with_output(local, due(), || panic!("Local is not Web"))
        .is_err());
    assert!(service
        .web_capabilities_with_output(web, Instant::now(), || panic!("expired entry"))
        .is_err());
    for fence in [
        Arc::new(|| false) as Arc<dyn Fn() -> bool + Send + Sync>,
        Arc::new(|| panic!("fence")),
    ] {
        assert!(service
            .with_execution_fence(fence)
            .web_capabilities_with_output(web, due(), || panic!("cancelled query"))
            .is_err());
    }
    let epoch = service.invalidate_runtime();
    let closed = service
        .web_capabilities_with_output(web, due(), || panic!("closed query"))
        .unwrap();
    assert!(!closed.admitting);
    assert_eq!(
        closed.device_availability,
        WebVoiceDeviceAvailability::Unverified
    );
    service.resume_after_stop(epoch).unwrap();
    let inventory = service.inventory().unwrap();
    service
        .set_enabled(local, false, inventory.revision)
        .unwrap();
    let disabled = service
        .web_capabilities_with_output(web, due(), || panic!("disabled query"))
        .unwrap();
    assert!(!disabled.enabled);
    assert_eq!(
        disabled.device_availability,
        WebVoiceDeviceAvailability::Unverified
    );
    assert!(empty(&service));
}

#[test]
fn completed_queries_cannot_cache_or_deliver_after_revocation_request_cancel_or_authority_change() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    for mode in [
        "revoke", "request", "identity", "voice", "class", "enabled", "runtime", "fault",
    ] {
        // One real credential/SQLite fixture. Each independent request receives
        // its current exact verified lease; no slow KDF is repeated per mode.
        let web = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Web);
        let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
        let active = Arc::new(AtomicBool::new(true));
        let observed = active.clone();
        let scoped =
            service.with_execution_fence(Arc::new(move || observed.load(Ordering::Acquire)));
        let result = scoped.web_capabilities_with_output(web, due(), || {
            match mode {
                "revoke" => {
                    assert!(service
                        .inner
                        .ledger
                        .lock()
                        .unwrap()
                        .sessions
                        .revoke_exact(web));
                }
                "request" => active.store(false, Ordering::Release),
                "identity" => {
                    backend
                        .0
                        .lock()
                        .unwrap()
                        .identity(
                            local,
                            IdentityCommand::PutSubject {
                                id: SubjectId::new(1).unwrap(),
                                name: "Changed observation catalog".into(),
                                enabled: true,
                                color: Rgb {
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
                    let request = fixtures::independent_request(&service, web, class, 40901);
                    service.submit(web, request).unwrap();
                }
                "class" => {
                    let id = ClassId::new(2).unwrap();
                    let mut backend = backend.0.lock().unwrap();
                    backend
                        .identity(
                            local,
                            IdentityCommand::CreateClass {
                                id,
                                name: "Changed target".into(),
                            },
                        )
                        .unwrap();
                    backend
                        .identity(local, IdentityCommand::BindCurrentClass { id })
                        .unwrap();
                }
                "enabled" => {
                    service
                        .set_enabled(local, false, service.inventory().unwrap().revision)
                        .unwrap();
                }
                "runtime" => {
                    service.invalidate_runtime();
                }
                "fault" => service.inner.control.health_gate().fault(),
                _ => unreachable!(),
            }
            Ok(DefaultOutputSupport::Unavailable)
        });
        assert!(result.is_err(), "late {mode} must not publish a fact");
        assert!(empty(&service), "late {mode} must not populate the cache");
        assert!(!service
            .inner
            .output_observation
            .in_flight
            .load(Ordering::Acquire));
        if mode != "fault" {
            // Negative SQL settlement of never-started jobs; no device owner
            // exists in this fixture. Restore admission for the next new case.
            let state = service.interrupt_all().unwrap();
            if !state.enabled {
                service.set_enabled(local, true, state.revision).unwrap();
            }
            if !service.is_admitting() {
                service.resume_after_stop(service.generation()).unwrap();
            }
        }
    }
}

struct Tick(AtomicU64);
impl fc_ports::MonotonicClock for Tick {
    fn monotonic_millis(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}
#[test]
fn both_ttl_clocks_expire_exactly_and_cache_hits_never_renew_completion() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let base = backend.0.lock().unwrap().voice_service();
    let ticks = Arc::new(Tick(AtomicU64::new(1_000)));
    // Same real SQLite/authority/ledger, with a deterministic monotonic source;
    // this is not a 30-second physical wallclock or hardware timing claim.
    let service = VoiceService::new(
        base.inner.repository.clone(),
        base.inner.ledger.clone(),
        base.inner.control.clone(),
        base.inner.authority.clone(),
        base.inner.clock.clone(),
        ticks.clone(),
    );
    service
        .web_capabilities_with_output(web, due(), || {
            ticks.0.store(2_000, Ordering::Release);
            Ok(DefaultOutputSupport::Unavailable)
        })
        .unwrap();
    let original = service
        .inner
        .output_observation
        .observation
        .lock()
        .unwrap()
        .unwrap();
    assert_eq!(
        original.completed_tick, 2_000,
        "TTL starts at query completion"
    );
    ticks.0.store(31_999, Ordering::Release);
    assert_eq!(
        service
            .web_capabilities_with_output(web, due(), || panic!("age29999 cache hit"))
            .unwrap()
            .device_availability,
        WebVoiceDeviceAvailability::Unavailable
    );
    assert_eq!(
        *service.inner.output_observation.observation.lock().unwrap(),
        Some(original)
    );
    ticks.0.store(32_000, Ordering::Release);
    let queried = Arc::new(AtomicBool::new(false));
    let observed = queried.clone();
    assert_eq!(
        service
            .web_capabilities_with_output(web, due(), move || {
                observed.store(true, Ordering::Release);
                Ok(DefaultOutputSupport::Supported)
            })
            .unwrap()
            .device_availability,
        WebVoiceDeviceAvailability::Unverified
    );
    assert!(queried.load(Ordering::Acquire));
    // Tighten only the private cached Instant: exercise the second real lookup
    // clock independently, without sleeping30s or fabricating a JobContext.
    service
        .inner
        .output_observation
        .observation
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .completed = Instant::now().checked_sub(LIFETIME).unwrap();
    let queried = Arc::new(AtomicBool::new(false));
    let observed = queried.clone();
    service
        .web_capabilities_with_output(web, due(), move || {
            observed.store(true, Ordering::Release);
            Ok(DefaultOutputSupport::Unavailable)
        })
        .unwrap();
    assert!(queried.load(Ordering::Acquire));
    let last = service
        .inner
        .output_observation
        .observation
        .lock()
        .unwrap()
        .unwrap();
    assert!(matches!(
        service
            .inner
            .output_observation
            .lookup(last.stamp, last.completed_tick - 1),
        Lookup::Missing
    ));
    assert!(empty(&service));
    assert!(
        matches!(
            service
                .inner
                .output_observation
                .lookup(last.stamp, last.completed_tick),
            Lookup::Missing
        ),
        "clock recovery cannot resurrect a discarded observation"
    );
    assert!(
        service
            .web_capabilities_with_output(web, due(), || {
                ticks.0.store(31_000, Ordering::Release);
                Ok(DefaultOutputSupport::Unavailable)
            })
            .is_err(),
        "query-time monotonic regression cannot publish"
    );
    assert!(empty(&service));
}

type Call = Box<dyn FnOnce(&JobContext) -> Result<WebVoiceCapabilities> + Send>;
struct Work(Call);
impl Task for Work {
    type Output = Result<WebVoiceCapabilities>;
    fn allocation_bytes(&self) -> Option<usize> {
        Some(std::mem::size_of_val(self.0.as_ref()))
    }
    fn run(self, context: &JobContext) -> Self::Output {
        (self.0)(context)
    }
}
fn submit(
    handle: &WorkerHandle<Work>,
    mut task: Work,
    deadline: Instant,
) -> JobTicket<Result<WebVoiceCapabilities>> {
    loop {
        match handle.try_submit(task, deadline) {
            Ok(ticket) => return ticket,
            Err(error) if error.kind == Rejection::Busy && Instant::now() < deadline => {
                task = error.task;
                std::thread::yield_now();
            }
            Err(error) => panic!("web device fixture admission {:?}", error.kind),
        }
    }
}
struct Release(mpsc::Sender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[test]
fn concurrent_cancelled_query_keeps_physical_flight_and_worker_until_its_actual_return() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let pool = WorkerPool::<Work>::start().unwrap();
    let (started, entered) = mpsc::sync_channel(1);
    let (release, held) = mpsc::channel();
    let release = Release(release);
    let worker = service.clone();
    let deadline = due();
    let mut first = submit(
        &pool.handle(),
        Work(Box::new(move |context| {
            let scoped = worker.with_execution_fence(Arc::new(context.execution_fence()));
            scoped.web_capabilities_with_output(web, deadline, || {
                started.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(DefaultOutputSupport::Unavailable)
            })
        })),
        deadline,
    );
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    first.cancel();
    assert!(service
        .inner
        .output_observation
        .in_flight
        .load(Ordering::Acquire));
    assert!(!first.is_finished());
    assert!(first.try_result().unwrap().is_none());
    let second_calls = Arc::new(AtomicUsize::new(0));
    let observed = second_calls.clone();
    let worker = service.clone();
    let second = submit(
        &pool.handle(),
        Work(Box::new(move |context| {
            worker
                .with_execution_fence(Arc::new(context.execution_fence()))
                .web_capabilities_with_output(web, deadline, || {
                    observed.fetch_add(1, Ordering::AcqRel);
                    Ok(DefaultOutputSupport::Unavailable)
                })
        })),
        deadline,
    );
    assert_eq!(
        second.wait().unwrap().unwrap().device_availability,
        WebVoiceDeviceAvailability::Unverified
    );
    assert_eq!(second_calls.load(Ordering::Acquire), 0);
    assert!(!first.is_finished());
    assert!(service
        .inner
        .output_observation
        .in_flight
        .load(Ordering::Acquire));
    let epoch = service.invalidate_runtime();
    let another_query = Arc::new(AtomicBool::new(false));
    let observed = another_query.clone();
    let _ = service.web_capabilities_with_output(web, due(), move || {
        observed.store(true, Ordering::Release);
        Ok(DefaultOutputSupport::Unavailable)
    });
    assert!(
        !another_query.load(Ordering::Acquire),
        "new epoch cannot replace a blocked physical query"
    );
    assert!(service
        .inner
        .output_observation
        .in_flight
        .load(Ordering::Acquire));
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(stats) = pool.handle().try_stats() {
            assert!(stats.running >= 1);
            break;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    drop(release);
    assert!(matches!(first.wait(), Err(JobError::Cancelled)));
    pool.join().unwrap();
    assert!(empty(&service));
    assert!(!service
        .inner
        .output_observation
        .in_flight
        .load(Ordering::Acquire));
    service.resume_after_stop(epoch).unwrap();
    assert_eq!(
        service
            .web_capabilities_with_output(web, due(), || Ok(DefaultOutputSupport::Unavailable))
            .unwrap()
            .device_availability,
        WebVoiceDeviceAvailability::Unavailable
    );
}

#[test]
fn actual_original_deadline_does_not_retire_a_blocked_query_or_admit_its_late_fact() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let pool = WorkerPool::<Work>::start().unwrap();
    let (started, entered) = mpsc::sync_channel(1);
    let (release, held) = mpsc::channel();
    let release = Release(release);
    let deadline = Instant::now() + Duration::from_secs(1);
    let worker = service.clone();
    let mut ticket = submit(
        &pool.handle(),
        Work(Box::new(move |context| {
            worker
                .with_execution_fence(Arc::new(context.execution_fence()))
                .web_capabilities_with_output(web, deadline, || {
                    started.send(()).unwrap();
                    held.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(DefaultOutputSupport::Unavailable)
                })
        })),
        deadline,
    );
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!ticket.is_finished());
    assert!(ticket.try_result().unwrap().is_none());
    assert!(service
        .inner
        .output_observation
        .in_flight
        .load(Ordering::Acquire));
    assert!(empty(&service));
    drop(release);
    assert!(matches!(ticket.wait(), Err(JobError::Deadline)));
    pool.join().unwrap();
    assert!(empty(&service));
    assert!(!service
        .inner
        .output_observation
        .in_flight
        .load(Ordering::Acquire));
}

fn account(backend: &Backend, id: u64, origin: TrustedOrigin) -> SessionLease {
    // Trusted test admission after real account/credential mutations, identical
    // to existing service fixtures; no caller-controlled actor enters production.
    let state = backend.state().unwrap();
    let lease = backend
        .ledger
        .lock()
        .unwrap()
        .sessions
        .issue_verified(
            state.account(PrincipalId::new(id).unwrap()).unwrap(),
            origin,
            0,
            SESSION_MILLIS,
            LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 3600).unwrap(),
        )
        .unwrap();
    if origin == TrustedOrigin::Local {
        backend
            .control
            .0
            .active_local
            .store(lease.id(), Ordering::Release);
        backend
            .control
            .0
            .active_principal
            .store(id, Ordering::Release);
    }
    lease
}
#[test]
fn only_authorized_teacher_or_global_web_can_query_and_no_password_change_or_user_bypasses_it() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let admin = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let subject = SubjectId::new(1).unwrap();
    let other = ClassId::new(2).unwrap();
    {
        let mut backend = backend.0.lock().unwrap();
        backend
            .identity(
                admin,
                IdentityCommand::CreateClass {
                    id: other,
                    name: "Other class".into(),
                },
            )
            .unwrap();
        backend
            .identity(
                admin,
                IdentityCommand::PutSubject {
                    id: subject,
                    name: "Subject".into(),
                    enabled: true,
                    color: Rgb {
                        red: 1,
                        green: 2,
                        blue: 3,
                    },
                    expected_revision: None,
                },
            )
            .unwrap();
        drop(
            backend
                .create_account(
                    admin,
                    NewAccount {
                        id: PrincipalId::new(2).unwrap(),
                        login_name: "output-teacher".into(),
                        display_name: "Teacher".into(),
                        role: Role::TeacherAdmin,
                        scopes: vec![AssignmentScope {
                            class_id: other,
                            subject_id: subject,
                        }],
                    },
                    Password::new("Synthetic initial output credential!".into()).unwrap(),
                )
                .unwrap(),
        );
    }
    let first = account(&backend.0.lock().unwrap(), 2, TrustedOrigin::Web);
    assert!(service
        .web_capabilities_with_output(first, due(), || panic!("must-change query"))
        .is_err());
    let first_local = account(&backend.0.lock().unwrap(), 2, TrustedOrigin::Local);
    backend
        .0
        .lock()
        .unwrap()
        .change_password(
            first_local,
            Password::new("Synthetic initial output credential!".into()).unwrap(),
            Password::new("Changed synthetic output credential!".into()).unwrap(),
        )
        .unwrap();
    let admin = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    for (role, scopes, allowed) in [
        (Role::User, vec![], false),
        (
            Role::TeacherAdmin,
            vec![AssignmentScope {
                class_id: other,
                subject_id: subject,
            }],
            false,
        ),
        (
            Role::TeacherAdmin,
            vec![AssignmentScope {
                class_id: class,
                subject_id: subject,
            }],
            true,
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
                    display_name: "Teacher".into(),
                    role,
                    enabled: true,
                    scopes,
                },
            )
            .unwrap();
        let web = account(&backend.0.lock().unwrap(), 2, TrustedOrigin::Web);
        let calls = AtomicUsize::new(0);
        let result = service.web_capabilities_with_output(web, due(), || {
            calls.fetch_add(1, Ordering::AcqRel);
            Ok(DefaultOutputSupport::Unavailable)
        });
        if role == Role::User {
            assert!(result.is_err());
        } else {
            let view = result.unwrap();
            assert_eq!(view.target_authorized, allowed);
            assert_eq!(
                view.device_availability,
                if allowed {
                    WebVoiceDeviceAvailability::Unavailable
                } else {
                    WebVoiceDeviceAvailability::Unverified
                }
            );
        }
        assert_eq!(calls.load(Ordering::Acquire), usize::from(allowed));
    }
}

#[test]
fn final_delivery_check_rejects_expired_missing_or_changed_negative_facts_and_old_versions() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let negative = service
        .web_capabilities_with_output(web, due(), || Ok(DefaultOutputSupport::Unavailable))
        .unwrap();
    service
        .check_web_capabilities_output(web, &negative)
        .unwrap();
    let before = service
        .inner
        .output_observation
        .observation
        .lock()
        .unwrap()
        .unwrap();
    service
        .check_web_capabilities_output(web, &negative)
        .unwrap();
    assert_eq!(
        *service.inner.output_observation.observation.lock().unwrap(),
        Some(before)
    );
    service
        .inner
        .output_observation
        .observation
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .completed = Instant::now().checked_sub(LIFETIME).unwrap();
    assert!(service
        .check_web_capabilities_output(web, &negative)
        .is_err());
    clear(&service);
    assert!(service
        .check_web_capabilities_output(web, &negative)
        .is_err());
    let unverified = service
        .web_capabilities_with_output(web, due(), || Ok(DefaultOutputSupport::Supported))
        .unwrap();
    assert!(
        service
            .check_web_capabilities_output(web, &negative)
            .is_err(),
        "new unverified fact cannot justify old negative response"
    );
    service
        .check_web_capabilities_output(web, &unverified)
        .unwrap();
    let alive = Arc::new(AtomicBool::new(false));
    let active = alive.clone();
    assert!(service
        .with_execution_fence(Arc::new(move || active.load(Ordering::Acquire)))
        .check_web_capabilities_output(web, &unverified)
        .is_err());
    service.invalidate_runtime();
    assert!(service
        .check_web_capabilities_output(web, &unverified)
        .is_err());
}

#[test]
fn withdrawing_a_cancelled_publication_does_not_wait_for_a_contended_cache_mutex() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let pool = WorkerPool::<Work>::start().unwrap();
    let (reached, at_publication) = mpsc::sync_channel(1);
    let (proceed, may_cancel) = mpsc::channel();
    let proceed = Release(proceed);
    let waited = Arc::new(AtomicBool::new(false));
    let held = Arc::new(Mutex::new(may_cancel));
    let query_service = service.clone();
    let witness = service.clone();
    let scoped = query_service.with_execution_fence(Arc::new(move || {
        if witness
            .inner
            .output_observation
            .valid
            .load(Ordering::Acquire)
            && !waited.swap(true, Ordering::AcqRel)
        {
            reached.send(()).unwrap();
            held.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            false
        } else {
            true
        }
    }));
    let deadline = due();
    let ticket = submit(
        &pool.handle(),
        Work(Box::new(move |_context| {
            scoped.web_capabilities_with_output(web, deadline, || {
                Ok(DefaultOutputSupport::Unavailable)
            })
        })),
        deadline,
    );
    at_publication.recv_timeout(Duration::from_secs(3)).unwrap();
    let cache_lock = service.inner.output_observation.observation.lock().unwrap();
    assert!(cache_lock.is_some());
    drop(proceed);
    assert!(matches!(
        ticket.wait().unwrap(),
        Err(BackendError::Storage(StorageError::ExecutionCancelled))
    ));
    assert!(!service
        .inner
        .output_observation
        .valid
        .load(Ordering::Acquire));
    assert!(!service
        .inner
        .output_observation
        .in_flight
        .load(Ordering::Acquire));
    drop(cache_lock);
    pool.join().unwrap();
    assert_eq!(
        service
            .web_capabilities_with_output(web, due(), || Ok(DefaultOutputSupport::Supported))
            .unwrap()
            .device_availability,
        WebVoiceDeviceAvailability::Unverified
    );
}

#[test]
fn real_catalog_voice_policy_runtime_and_class_changes_never_reuse_old_cache_stamp() {
    let _scenario = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let calls = AtomicUsize::new(0);
    let query = || {
        calls.fetch_add(1, Ordering::AcqRel);
        Ok(DefaultOutputSupport::Unavailable)
    };
    service
        .web_capabilities_with_output(web, due(), query)
        .unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 1);
    backend
        .0
        .lock()
        .unwrap()
        .identity(
            local,
            IdentityCommand::PutSubject {
                id: SubjectId::new(1).unwrap(),
                name: "Changed cache catalog".into(),
                enabled: true,
                color: Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                expected_revision: None,
            },
        )
        .unwrap();
    service
        .web_capabilities_with_output(web, due(), query)
        .unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 2);
    service.reserve(web, class).unwrap();
    service
        .web_capabilities_with_output(web, due(), query)
        .unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 3);
    let review = service.management_snapshot(local).unwrap();
    service
        .set_limits_reviewed(
            local,
            VoiceLimits {
                max_duration_millis: 1_000,
                ..VoiceLimits::default()
            },
            review.revision,
            review.identity_revision,
            review.runtime_generation,
        )
        .unwrap();
    let changed = service
        .web_capabilities_with_output(web, due(), query)
        .unwrap();
    assert_eq!(changed.limits.max_duration_millis, 1_000);
    assert_eq!(calls.load(Ordering::Acquire), 4);
    let epoch = service.invalidate_runtime();
    service.resume_after_stop(epoch).unwrap();
    service
        .web_capabilities_with_output(web, due(), query)
        .unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 5);
    let id = ClassId::new(2).unwrap();
    {
        let mut backend = backend.0.lock().unwrap();
        backend
            .identity(
                local,
                IdentityCommand::CreateClass {
                    id,
                    name: "Current cache target".into(),
                },
            )
            .unwrap();
        backend
            .identity(local, IdentityCommand::BindCurrentClass { id })
            .unwrap();
    }
    let changed = service
        .web_capabilities_with_output(web, due(), query)
        .unwrap();
    assert_eq!(changed.current_class.unwrap().0, id);
    assert_eq!(calls.load(Ordering::Acquire), 6);
}
