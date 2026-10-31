//! Trusted service policy changes on the real sole writer. No native output is
//! constructed; a Released receipt is explicit rather than inferred from Stop.
use super::tests as fixtures;
use super::*;

fn limited() -> VoiceLimits {
    VoiceLimits {
        max_duration_millis: 1_000,
        max_pending_jobs: 2,
        max_pending_per_owner: 1,
        max_repeat_total: 2,
    }
}
fn save(
    service: &VoiceService,
    lease: SessionLease,
    limits: VoiceLimits,
    review: &VoiceManagementSnapshot,
) -> Result<VoiceSnapshot> {
    service.set_limits_reviewed(
        lease,
        limits,
        review.revision,
        review.identity_revision,
        review.runtime_generation,
    )
}

#[test]
fn invalid_limits_cannot_stop_wake_or_persist_before_the_real_writer() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let before = service.management_snapshot(local).unwrap();
    let inventory = service.inventory().unwrap();
    let wakes = Arc::new(AtomicU64::new(0));
    let observed = wakes.clone();
    service
        .set_waker(Arc::new(move || {
            observed.fetch_add(1, Ordering::AcqRel);
        }))
        .unwrap();
    let base = VoiceLimits::default();
    for limits in [
        VoiceLimits {
            max_duration_millis: 499,
            ..base
        },
        VoiceLimits {
            max_duration_millis: 60_001,
            ..base
        },
        VoiceLimits {
            max_pending_jobs: 0,
            ..base
        },
        VoiceLimits {
            max_pending_jobs: 5,
            ..base
        },
        VoiceLimits {
            max_pending_per_owner: 0,
            ..base
        },
        VoiceLimits {
            max_pending_per_owner: 3,
            ..base
        },
        VoiceLimits {
            max_pending_jobs: 1,
            max_pending_per_owner: 2,
            ..base
        },
        VoiceLimits {
            max_repeat_total: 0,
            ..base
        },
        VoiceLimits {
            max_repeat_total: 11,
            ..base
        },
    ] {
        assert!(matches!(
            save(&service, local, limits, &before),
            Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::InvalidState
            )))
        ));
        assert_eq!(service.management_snapshot(local).unwrap(), before);
        assert_eq!(service.inventory().unwrap(), inventory);
    }
    service.clear_waker().unwrap();
    assert_eq!(wakes.load(Ordering::Acquire), 0);
}

#[test]
fn limits_require_exact_local_manager_and_all_three_captured_versions_before_invalidation() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let before = service.management_snapshot(local).unwrap();
    let inventory = service.inventory().unwrap();
    assert!(save(&service, web, limited(), &before).is_err());
    assert_eq!(service.inventory().unwrap(), inventory);
    for (voice, identity, epoch) in [
        (
            before.revision.next().unwrap(),
            before.identity_revision,
            before.runtime_generation,
        ),
        (
            before.revision,
            before.identity_revision.next().unwrap(),
            before.runtime_generation,
        ),
        (
            before.revision,
            before.identity_revision,
            before.runtime_generation + 1,
        ),
    ] {
        assert!(service
            .set_limits_reviewed(local, limited(), voice, identity, epoch)
            .is_err());
        assert_eq!(service.management_snapshot(local).unwrap(), before);
    }
    let replacement = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    assert!(save(&service, local, limited(), &before).is_err());
    service
        .inner
        .ledger
        .lock()
        .unwrap()
        .sessions
        .revoke_exact(replacement);
    assert!(save(&service, replacement, limited(), &before).is_err());
    assert_eq!(service.inventory().unwrap(), inventory);
    assert_eq!(service.generation(), before.runtime_generation);
    assert!(service.is_admitting());
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    backend
        .0
        .lock()
        .unwrap()
        .identity(
            local,
            IdentityCommand::RenameSchool {
                name: "New reviewed policy identity".into(),
            },
        )
        .unwrap();
    assert!(save(&service, local, limited(), &before).is_err());
    assert_eq!(service.inventory().unwrap(), inventory);
    let fresh = service.management_snapshot(local).unwrap();
    service.invalidate_runtime();
    let stopped = service.generation();
    assert!(save(&service, local, limited(), &fresh).is_err());
    assert_eq!(service.generation(), stopped);
    assert_eq!(service.inventory().unwrap(), inventory);
}

#[test]
fn saved_limits_reach_current_caps_retire_old_upload_epoch_and_survive_database_reopen() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (directory, backend, web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let old_upload = service.reserve(web, class).unwrap();
    let before = service.management_snapshot(local).unwrap();
    let saved = save(&service, local, limited(), &before).unwrap();
    assert_eq!(saved.limits, limited());
    assert!(saved.enabled);
    assert!(saved
        .assets
        .iter()
        .all(|asset| asset.state == VoiceAssetState::DeletePending));
    let view = service.management_snapshot(local).unwrap();
    assert_eq!(view.limits, limited());
    assert!(view.enabled && view.admitting);
    assert!(view.runtime_generation > before.runtime_generation);
    assert!(service
        .check_web_upload(web, &old_upload, before.runtime_generation)
        .is_err());
    let caps = service.web_capabilities(web).unwrap();
    assert_eq!(caps.limits, limited());
    assert_eq!(caps.limits.max_wav_bytes(), 32_044);
    assert_eq!(caps.identity_revision, view.identity_revision);
    assert_eq!(caps.publication_revision, view.publication_revision);
    assert!(caps.publication_revision > caps.identity_revision);
    assert!(caps.enabled && caps.admitting);
    assert_eq!(caps.runtime_generation, view.runtime_generation);
    let fresh = service.reserve(web, class).unwrap();
    assert_eq!(fresh.charged_bytes, MAX_WAV_BYTES);
    assert!(service
        .check_web_upload(web, &fresh, caps.runtime_generation)
        .is_ok());
    backend.0.lock().unwrap().close().unwrap();
    let mut reopened = Backend::open(
        directory.0.join("state.sqlite3"),
        fc_security::CredentialRecordCodec::from_key([92; 32]).unwrap(),
        service.inner.clock.clone(),
        service.inner.monotonic.clone(),
    )
    .unwrap();
    let local = fixtures::issue(&reopened, TrustedOrigin::Local);
    let restored = reopened.voice_service().management_snapshot(local).unwrap();
    assert_eq!(restored.limits, limited());
    assert!(restored.enabled);
    assert_eq!(restored.active_jobs, 0);
    reopened.close().unwrap();
}

#[test]
fn saving_limits_while_disabled_does_not_reenable_the_persisted_or_runtime_switch() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let before = service.management_snapshot(local).unwrap();
    service
        .set_enabled_reviewed(
            local,
            false,
            before.revision,
            before.identity_revision,
            before.runtime_generation,
        )
        .unwrap();
    let disabled = service.management_snapshot(local).unwrap();
    save(&service, local, limited(), &disabled).unwrap();
    let view = service.management_snapshot(local).unwrap();
    assert_eq!(view.limits, limited());
    assert!(!view.enabled && !view.admitting);
    let caps = service.web_capabilities(web).unwrap();
    assert_eq!(caps.limits, limited());
    assert!(!caps.enabled && !caps.admitting);
}

#[test]
fn changed_limits_cancel_queued_and_playing_jobs_without_claiming_the_old_device_released() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let mut first_request = fixtures::independent_request(&service, web, class, 2_801);
    first_request.repeat_total = 3;
    let first = service.submit(web, first_request).unwrap();
    assert_eq!(first.repeat_total, 3);
    let old_runtime = service.runtime_handle();
    let starting = old_runtime
        .drive(VoiceDriverUpdate {
            job_id: first.id,
            expected_revision: first.revision,
            generation: first.generation,
            action: VoiceDriverAction::Start,
        })
        .unwrap();
    let playing = old_runtime
        .drive(VoiceDriverUpdate {
            job_id: first.id,
            expected_revision: starting.revision,
            generation: starting.generation,
            action: VoiceDriverAction::FirstWrite,
        })
        .unwrap();
    let permission = old_runtime.guard(playing.id).unwrap();
    let queued = service
        .submit(
            web,
            fixtures::independent_request(&service, web, class, 2_802),
        )
        .unwrap();
    let before = service.management_snapshot(local).unwrap();
    let saved = save(&service, local, limited(), &before).unwrap();
    let stopping = saved.jobs.iter().find(|job| job.id == playing.id).unwrap();
    assert_eq!(stopping.state, VoiceState::Stopping);
    assert_eq!(stopping.release_generation, Some(playing.generation));
    assert_eq!(
        stopping.repeat_total, 3,
        "historical job is not rewritten to a new repeat limit"
    );
    assert!(saved
        .jobs
        .iter()
        .find(|job| job.id == queued.id)
        .unwrap()
        .state
        .terminal());
    assert!(!permission.guard.is_valid());
    assert!(!old_runtime.is_current());
    let view = service.management_snapshot(local).unwrap();
    assert!(view.enabled && !view.admitting);
    assert_eq!(
        (view.active_jobs, view.queued_jobs, view.playing_jobs),
        (1, 0, 0)
    );
    assert!(service.resume_after_stop(view.runtime_generation).is_err());
    service
        .set_enabled_reviewed(
            local,
            true,
            view.revision,
            view.identity_revision,
            view.runtime_generation,
        )
        .unwrap();
    assert!(
        !service.is_admitting(),
        "setting true cannot manufacture a Released receipt"
    );
    old_runtime
        .drive(VoiceDriverUpdate {
            job_id: stopping.id,
            expected_revision: stopping.revision,
            generation: stopping.release_generation.unwrap(),
            action: VoiceDriverAction::Released,
        })
        .unwrap();
    assert_eq!(service.management_snapshot(local).unwrap().active_jobs, 0);
    service.resume_after_stop(view.runtime_generation).unwrap();
    assert!(service.is_admitting());
    assert!(!old_runtime.is_current());
}

#[test]
fn rejected_limits_persistence_keeps_the_original_policy_and_fails_runtime_closed() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let before = service.management_snapshot(local).unwrap();
    let ledger = service.inner.ledger.clone();
    let wakes = Arc::new(AtomicU64::new(0));
    let observed = wakes.clone();
    service
        .set_waker(Arc::new(move || {
            if observed.fetch_add(1, Ordering::AcqRel) == 0 {
                assert!(ledger.lock().unwrap().sessions.revoke_exact(local));
            }
        }))
        .unwrap();
    assert!(save(&service, local, limited(), &before).is_err());
    service.clear_waker().unwrap();
    assert!(wakes.load(Ordering::Acquire) >= 2);
    let fresh = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let after = service.management_snapshot(fresh).unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.limits, before.limits);
    assert!(after.enabled && !after.admitting);
    assert!(!service.inner.enabled.load(Ordering::Acquire));
    assert!(after.runtime_generation > before.runtime_generation);
}

#[test]
fn committed_limits_cannot_resume_a_later_stop_or_replay_the_old_review() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let before = service.management_snapshot(local).unwrap();
    let changed = service.clone();
    service.after_next_commit(Arc::new(move || {
        changed.invalidate_runtime();
    }));
    assert!(matches!(
        save(&service, local, limited(), &before),
        Err(BackendError::Voice(VoiceRepositoryError::Domain(
            VoiceError::StaleGeneration
        )))
    ));
    let after = service.management_snapshot(local).unwrap();
    assert_eq!(
        after.limits,
        limited(),
        "the actual completed commit remains factual"
    );
    assert!(after.enabled && !after.admitting);
    assert!(after.runtime_generation > before.runtime_generation + 1);
    assert!(save(&service, local, limited(), &before).is_err());
    assert_eq!(service.management_snapshot(local).unwrap(), after);
}

struct FenceClock {
    source: Arc<dyn MonotonicClock + Send + Sync>,
    reads: AtomicU64,
    fire_at: u64,
    action: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl MonotonicClock for FenceClock {
    fn monotonic_millis(&self) -> u64 {
        if self.reads.fetch_add(1, Ordering::AcqRel) + 1 == self.fire_at {
            let action = self.action.lock().unwrap().take();
            if let Some(action) = action {
                action();
            }
        }
        self.source.monotonic_millis()
    }
}

#[test]
fn limits_writer_checks_the_captured_epoch_during_evaluation_and_before_commit() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let original = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let caller = std::thread::current().id();
    for fire_at in [3, 4] {
        let clock = Arc::new(FenceClock {
            source: original.inner.monotonic.clone(),
            reads: AtomicU64::new(0),
            fire_at,
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
        let stopped = service.clone();
        *clock.action.lock().unwrap() = Some(Box::new(move || {
            assert_ne!(std::thread::current().id(), caller);
            stopped.invalidate_runtime();
        }));
        assert!(matches!(
            save(&service, local, limited(), &before),
            Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration
            )))
        ));
        assert_eq!(clock.reads.load(Ordering::Acquire), fire_at);
        assert!(clock.action.lock().unwrap().is_none());
        let current = service.inventory().unwrap();
        assert_eq!(current.revision, before.revision);
        assert_eq!(current.limits, before.limits);
        assert!(current.enabled && !service.is_admitting());
    }
}
