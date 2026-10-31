use super::super::tests as fixtures;
use super::*;
use fc_domain::identity::{AssignmentScope, NewAccount, Rgb};

fn assert_limits_denied(service: &VoiceService, lease: SessionLease) {
    let before = service.inventory().unwrap();
    let identity = service.inner.authority.snapshot().unwrap().data.revision;
    let epoch = service.generation();
    assert!(service
        .set_limits_reviewed(
            lease,
            VoiceLimits::default(),
            before.revision,
            identity,
            epoch
        )
        .is_err());
    assert_eq!(service.inventory().unwrap(), before);
    assert_eq!(service.generation(), epoch);
}

pub(super) fn issue_account(
    backend: &Backend,
    principal: u64,
    origin: TrustedOrigin,
    expired: bool,
) -> SessionLease {
    let state = backend.state().unwrap();
    let now = backend.clock.now();
    let expires = if expired {
        now
    } else {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 3600).unwrap()
    };
    let lease = backend
        .ledger
        .lock()
        .unwrap()
        .sessions
        .issue_verified(
            state.account(PrincipalId::new(principal).unwrap()).unwrap(),
            origin,
            0,
            SESSION_MILLIS,
            expires,
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
            .store(principal, Ordering::Release);
    }
    lease
}

#[test]
fn management_metadata_counts_jobs_and_disabled_setting_survives_reopen_and_cas_conflict() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (directory, backend, web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let initial = service.management_snapshot(local).unwrap();
    assert!(initial.enabled && initial.admitting);
    assert_eq!(
        (
            initial.active_jobs,
            initial.queued_jobs,
            initial.playing_jobs
        ),
        (0, 0, 0)
    );
    let request = fixtures::independent_request(&service, web, class, 901);
    let job = service.submit(web, request).unwrap();
    let queued = service.management_snapshot(local).unwrap();
    assert_eq!(
        (queued.active_jobs, queued.queued_jobs, queued.playing_jobs),
        (1, 1, 0)
    );
    assert_eq!(
        queued.identity_revision,
        backend.0.lock().unwrap().state().unwrap().revision
    );
    assert_eq!(queued.revision, service.inventory().unwrap().revision);
    let generation = service.generation();
    assert!(matches!(
        service.set_enabled(local, false, initial.revision),
        Err(BackendError::Voice(VoiceRepositoryError::Domain(
            VoiceError::RevisionConflict
        )))
    ));
    assert_eq!(service.generation(), generation);
    assert!(service.management_snapshot(local).unwrap().admitting);
    let guard = service.guard(job.id).unwrap();
    let disabled = service.set_enabled(local, false, queued.revision).unwrap();
    assert!(!disabled.enabled);
    assert!(!guard.guard.is_valid());
    let view = service.management_snapshot(local).unwrap();
    assert!(!view.enabled && !view.admitting);
    assert_eq!(
        (view.active_jobs, view.queued_jobs, view.playing_jobs),
        (0, 0, 0)
    );
    assert!(view.runtime_generation > queued.runtime_generation);
    assert!(matches!(
        service.set_enabled(local, true, queued.revision),
        Err(BackendError::Voice(VoiceRepositoryError::Domain(
            VoiceError::RevisionConflict
        )))
    ));
    assert_eq!(service.management_snapshot(local).unwrap(), view);
    backend.0.lock().unwrap().close().unwrap();
    let clock = service.inner.clock.clone();
    let monotonic = service.inner.monotonic.clone();
    let mut reopened = Backend::open(
        directory.0.join("state.sqlite3"),
        fc_security::CredentialRecordCodec::from_key([92; 32]).unwrap(),
        clock,
        monotonic,
    )
    .unwrap();
    let fresh = fixtures::issue(&reopened, TrustedOrigin::Local);
    let restored = reopened.voice_service().management_snapshot(fresh).unwrap();
    assert!(!restored.enabled && !restored.admitting);
    assert_eq!(restored.active_jobs, 0);
    reopened.close().unwrap();
}

#[test]
fn management_rejects_web_stale_revoked_expired_and_lock_or_faulted_local_leases() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    assert!(service.management_snapshot(web).is_err());
    assert!(service
        .set_enabled(web, false, service.inventory().unwrap().revision)
        .is_err());
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    service.management_snapshot(local).unwrap();
    let replacement = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    assert!(service.management_snapshot(local).is_err());
    service.management_snapshot(replacement).unwrap();
    assert!(service
        .inner
        .ledger
        .lock()
        .unwrap()
        .sessions
        .revoke_exact(replacement));
    assert!(service.management_snapshot(replacement).is_err());
    let expired = issue_account(&backend.0.lock().unwrap(), 1, TrustedOrigin::Local, true);
    assert!(service.management_snapshot(expired).is_err());
    assert_limits_denied(&service, expired);
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    service.inner.control.lock_local();
    assert!(service.management_snapshot(local).is_err());
    assert_limits_denied(&service, local);
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    service.inner.control.health_gate().fault();
    assert!(service.management_snapshot(local).is_err());
}

#[test]
fn management_requires_global_admin_and_completed_initial_password_change() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, class) = fixtures::shared_fixture();
    let mut backend = backend.0.lock().unwrap();
    let admin = fixtures::issue(&backend, TrustedOrigin::Local);
    let password = "Synthetic managed voice account 2026!";
    drop(
        backend
            .create_account(
                admin,
                NewAccount {
                    id: PrincipalId::new(2).unwrap(),
                    login_name: "voice-manager".into(),
                    display_name: "Voice manager".into(),
                    role: Role::GlobalAdmin,
                    scopes: vec![],
                },
                Password::new(password.into()).unwrap(),
            )
            .unwrap(),
    );
    let candidate = issue_account(&backend, 2, TrustedOrigin::Local, false);
    let service = backend.voice_service();
    assert!(service.management_snapshot(candidate).is_err());
    assert_limits_denied(&service, candidate);
    backend
        .change_password(
            candidate,
            Password::new(password.into()).unwrap(),
            Password::new("Changed synthetic managed voice 2026!".into()).unwrap(),
        )
        .unwrap();
    let candidate = issue_account(&backend, 2, TrustedOrigin::Local, false);
    service.management_snapshot(candidate).unwrap();
    let admin = fixtures::issue(&backend, TrustedOrigin::Local);
    let subject = SubjectId::new(1).unwrap();
    backend
        .identity(
            admin,
            IdentityCommand::PutSubject {
                id: subject,
                name: "Synthetic voice subject".into(),
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
            .identity(
                admin,
                IdentityCommand::UpdateAccount {
                    id: PrincipalId::new(2).unwrap(),
                    display_name: "Synthetic limited account".into(),
                    role,
                    enabled: true,
                    scopes,
                },
            )
            .unwrap();
        assert!(service.management_snapshot(candidate).is_err());
        let limited = issue_account(&backend, 2, TrustedOrigin::Local, false);
        assert!(
            !backend
                .state()
                .unwrap()
                .account(limited.principal_id())
                .unwrap()
                .must_change_password
        );
        assert!(service.management_snapshot(limited).is_err());
        assert_limits_denied(&service, limited);
        assert!(service
            .set_enabled(limited, false, service.inventory().unwrap().revision)
            .is_err());
        // Selecting another local account is not a renewal of the old admin.
        backend
            .control
            .0
            .active_local
            .store(admin.id(), Ordering::Release);
        backend
            .control
            .0
            .active_principal
            .store(1, Ordering::Release);
    }
}

#[test]
fn disabling_wakes_before_writer_rejection_and_keeps_runtime_closed_without_faking_completion() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let service = backend.0.lock().unwrap().voice_service();
    let before = service.management_snapshot(local).unwrap();
    let wakes = Arc::new(AtomicU64::new(0));
    let observed = wakes.clone();
    let ledger = service.inner.ledger.clone();
    service
        .set_waker(Arc::new(move || {
            if observed.fetch_add(1, Ordering::AcqRel) == 0 {
                assert!(ledger.lock().unwrap().sessions.revoke_exact(local));
            }
        }))
        .unwrap();
    assert!(service.set_enabled(local, false, before.revision).is_err());
    service.clear_waker().unwrap();
    assert_eq!(wakes.load(Ordering::Acquire), 2);
    assert!(!service.inner.enabled.load(Ordering::Acquire));
    assert!(!service.is_admitting());
    assert!(service.generation() > before.runtime_generation);
    let fresh = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let after = service.management_snapshot(fresh).unwrap();
    assert!(
        after.enabled,
        "rejected transaction did not persist the proposed setting"
    );
    assert!(
        !after.admitting,
        "a rejected disable cannot reopen the old runtime"
    );
    assert_eq!(after.revision, before.revision);
}

struct PublicationClock {
    reads: AtomicU64,
    fire_at: u64,
    tick: AtomicU64,
    action: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl MonotonicClock for PublicationClock {
    fn monotonic_millis(&self) -> u64 {
        if self.reads.fetch_add(1, Ordering::AcqRel) + 1 == self.fire_at {
            let action = self.action.lock().unwrap().take();
            if let Some(action) = action {
                action();
            }
        }
        self.tick.load(Ordering::Acquire)
    }
}

#[test]
fn management_revalidates_original_lease_health_and_runtime_after_the_writer_read() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let original = backend.0.lock().unwrap().voice_service();
    for kind in ["revoke", "lock", "runtime", "fault"] {
        let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
        let clock = Arc::new(PublicationClock {
            reads: AtomicU64::new(0),
            fire_at: 2,
            tick: AtomicU64::new(0),
            action: Mutex::new(None),
        });
        let service = VoiceService::new(
            original.inner.repository.clone(),
            original.inner.ledger.clone(),
            original.inner.control.clone(),
            original.inner.authority.clone(),
            original.inner.clock.clone(),
            clock.clone(),
        );
        let action_service = service.clone();
        *clock.action.lock().unwrap() = Some(Box::new(move || match kind {
            "revoke" => assert!(action_service
                .inner
                .ledger
                .lock()
                .unwrap()
                .sessions
                .revoke_exact(local)),
            "lock" => action_service.inner.control.lock_local(),
            "runtime" => {
                action_service.invalidate_runtime();
            }
            "fault" => action_service.inner.control.health_gate().fault(),
            _ => unreachable!(),
        }));
        assert!(
            service.management_snapshot(local).is_err(),
            "post-read {kind} must reject the view"
        );
        assert_eq!(
            clock.reads.load(Ordering::Acquire),
            2,
            "{kind}: both authorization checkpoints ran"
        );
        assert!(clock.action.lock().unwrap().is_none());
    }
}

#[test]
fn management_reads_never_renew_the_original_local_idle_deadline() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let original = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let clock = Arc::new(PublicationClock {
        reads: AtomicU64::new(0),
        fire_at: 2,
        tick: AtomicU64::new(0),
        action: Mutex::new(None),
    });
    let service = VoiceService::new(
        original.inner.repository.clone(),
        original.inner.ledger.clone(),
        original.inner.control.clone(),
        original.inner.authority.clone(),
        original.inner.clock.clone(),
        clock.clone(),
    );
    service.management_snapshot(local).unwrap();
    clock.tick.store(
        fc_application::identity::LOCAL_IDLE_MILLIS - 1,
        Ordering::Release,
    );
    service.management_snapshot(local).unwrap();
    clock.tick.store(
        fc_application::identity::LOCAL_IDLE_MILLIS,
        Ordering::Release,
    );
    assert!(service.management_snapshot(local).is_err());
}

#[test]
fn management_distinguishes_starting_playing_and_stopping_until_exact_release() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let request = fixtures::independent_request(&service, web, class, 902);
    let queued = service.submit(web, request).unwrap();
    let starting = service
        .drive(VoiceDriverUpdate {
            job_id: queued.id,
            expected_revision: queued.revision,
            generation: queued.generation,
            action: VoiceDriverAction::Start,
        })
        .unwrap();
    let view = service.management_snapshot(local).unwrap();
    assert_eq!(
        (view.active_jobs, view.queued_jobs, view.playing_jobs),
        (1, 0, 0)
    );
    let playing = service
        .drive(VoiceDriverUpdate {
            job_id: starting.id,
            expected_revision: starting.revision,
            generation: starting.generation,
            action: VoiceDriverAction::FirstWrite,
        })
        .unwrap();
    assert_eq!(playing.state, VoiceState::Playing);
    let view = service.management_snapshot(local).unwrap();
    assert_eq!(
        (view.active_jobs, view.queued_jobs, view.playing_jobs),
        (1, 0, 1)
    );
    let disabled = service.set_enabled(local, false, view.revision).unwrap();
    let stopping = disabled
        .jobs
        .iter()
        .find(|job| job.id == queued.id)
        .unwrap();
    assert_eq!(stopping.state, VoiceState::Stopping);
    let view = service.management_snapshot(local).unwrap();
    assert!(!view.enabled && !view.admitting);
    assert_eq!(
        (view.active_jobs, view.queued_jobs, view.playing_jobs),
        (1, 0, 0)
    );
    service
        .drive(VoiceDriverUpdate {
            job_id: stopping.id,
            expected_revision: stopping.revision,
            generation: stopping.release_generation.unwrap(),
            action: VoiceDriverAction::Released,
        })
        .unwrap();
    let view = service.management_snapshot(local).unwrap();
    assert_eq!(
        (view.active_jobs, view.queued_jobs, view.playing_jobs),
        (0, 0, 0)
    );
}

fn apply_review(
    service: &VoiceService,
    lease: SessionLease,
    enabled: bool,
    view: &VoiceManagementSnapshot,
) -> Result<VoiceSnapshot> {
    service.set_enabled_reviewed(
        lease,
        enabled,
        view.revision,
        view.identity_revision,
        view.runtime_generation,
    )
}

#[test]
fn reviewed_change_rejects_identity_only_or_runtime_only_changes_with_unchanged_voice_revision() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let before = service.management_snapshot(local).unwrap();
    backend
        .0
        .lock()
        .unwrap()
        .identity(
            local,
            IdentityCommand::RenameSchool {
                name: "Changed review identity".into(),
            },
        )
        .unwrap();
    assert_eq!(service.inventory().unwrap().revision, before.revision);
    assert!(matches!(
        apply_review(&service, local, false, &before),
        Err(BackendError::Voice(VoiceRepositoryError::Domain(
            VoiceError::RevisionConflict
        )))
    ));
    let fresh = service.management_snapshot(local).unwrap();
    assert_eq!(fresh.runtime_generation, before.runtime_generation);
    assert!(fresh.enabled && fresh.admitting);
    service.invalidate_runtime();
    let stopped = service.generation();
    assert!(matches!(
        apply_review(&service, local, false, &fresh),
        Err(BackendError::Voice(VoiceRepositoryError::Domain(
            VoiceError::StaleGeneration
        )))
    ));
    assert_eq!(service.generation(), stopped);
    assert_eq!(service.inventory().unwrap().revision, fresh.revision);
}

#[test]
fn reviewed_writer_guard_rejects_identity_or_runtime_changed_after_disable_cas_before_enqueue() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    for kind in ["identity", "runtime"] {
        let before = service.management_snapshot(local).unwrap();
        let changed = Arc::new(AtomicBool::new(false));
        let fired = changed.clone();
        let backend_handle = backend.0.clone();
        let changed_service = service.clone();
        service
            .set_waker(Arc::new(move || {
                if !fired.swap(true, Ordering::AcqRel) {
                    if kind == "identity" {
                        backend_handle
                            .lock()
                            .unwrap()
                            .identity(
                                local,
                                IdentityCommand::RenameSchool {
                                    name: "Changed after stop CAS".into(),
                                },
                            )
                            .unwrap();
                    } else {
                        changed_service.invalidate_runtime();
                    }
                }
            }))
            .unwrap();
        let outcome = apply_review(&service, local, false, &before);
        service.clear_waker().unwrap();
        assert!(changed.load(Ordering::Acquire));
        assert!(
            matches!(
                outcome,
                Err(BackendError::Voice(VoiceRepositoryError::Domain(
                    VoiceError::RevisionConflict | VoiceError::StaleGeneration
                )))
            ),
            "{kind}: {outcome:?}"
        );
        let after = service.management_snapshot(local).unwrap();
        assert_eq!(
            after.revision, before.revision,
            "{kind}: rejected writer did not commit"
        );
        assert!(after.enabled);
        assert!(!after.admitting);
        assert!(after.runtime_generation > before.runtime_generation);
    }
}

#[test]
fn reviewed_enable_does_not_adopt_a_later_stop_after_its_commit() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let before = service.management_snapshot(local).unwrap();
    apply_review(&service, local, false, &before).unwrap();
    let disabled = service.management_snapshot(local).unwrap();
    let stopped_service = service.clone();
    service.after_next_commit(Arc::new(move || {
        stopped_service.invalidate_runtime();
    }));
    assert!(matches!(
        apply_review(&service, local, true, &disabled),
        Err(BackendError::Voice(VoiceRepositoryError::Domain(
            VoiceError::StaleGeneration
        )))
    ));
    let after = service.management_snapshot(local).unwrap();
    assert!(after.enabled, "the real completed commit remains factual");
    assert_ne!(after.revision, disabled.revision);
    assert!(
        !after.admitting,
        "old consent cannot resume the later stop epoch"
    );
    assert!(!service.inner.enabled.load(Ordering::Acquire));
    assert!(after.runtime_generation > disabled.runtime_generation);
    assert!(
        apply_review(&service, local, true, &disabled).is_err(),
        "old review is never replayed"
    );
}

#[test]
fn reviewed_writer_rechecks_exact_epoch_during_evaluation_and_immediately_before_commit() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let original = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let caller = std::thread::current().id();
    for enabled in [false, true] {
        for fire_at in [3, 4] {
            let clock = Arc::new(PublicationClock {
                reads: AtomicU64::new(0),
                fire_at,
                tick: AtomicU64::new(0),
                action: Mutex::new(None),
            });
            let service = VoiceService::new(
                original.inner.repository.clone(),
                original.inner.ledger.clone(),
                original.inner.control.clone(),
                original.inner.authority.clone(),
                original.inner.clock.clone(),
                clock.clone(),
            );
            let before = service.management_snapshot(local).unwrap();
            clock.reads.store(0, Ordering::Release);
            let changed_service = service.clone();
            *clock.action.lock().unwrap() = Some(Box::new(move || {
                assert_ne!(
                    std::thread::current().id(),
                    caller,
                    "fence executes on real SQLite writer"
                );
                changed_service.invalidate_runtime();
            }));
            assert!(
                matches!(
                    apply_review(&service, local, enabled, &before),
                    Err(BackendError::Voice(VoiceRepositoryError::Domain(
                        VoiceError::StaleGeneration
                    )))
                ),
                "enabled={enabled} at writer checkpoint={fire_at}"
            );
            assert_eq!(clock.reads.load(Ordering::Acquire), fire_at);
            assert!(clock.action.lock().unwrap().is_none());
            let actual = service.inventory().unwrap();
            assert_eq!(
                actual.revision, before.revision,
                "no late COMMIT for rejected review"
            );
            assert!(actual.enabled);
            assert!(!service.is_admitting());
        }
    }
}
