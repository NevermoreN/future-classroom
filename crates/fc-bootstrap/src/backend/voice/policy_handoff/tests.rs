use super::super::tests as fixtures;
use super::*;

fn limits() -> VoiceLimits {
    VoiceLimits {
        max_duration_millis: 1_000,
        max_pending_jobs: 2,
        max_pending_per_owner: 1,
        max_repeat_total: 2,
    }
}
fn save(service: &VoiceService, local: SessionLease) -> Result<VoiceSnapshot> {
    let review = service.management_snapshot(local)?;
    service.set_limits_reviewed(
        local,
        limits(),
        review.revision,
        review.identity_revision,
        review.runtime_generation,
    )
}

#[test]
fn pending_is_published_before_wake_and_rejects_concurrent_policy_without_another_cas() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let from = service.generation();
    let seen = Arc::new(AtomicU64::new(0));
    let observed = seen.clone();
    let verified = Arc::new(AtomicBool::new(false));
    let checked = verified.clone();
    let exact = service.clone();
    service
        .set_waker(Arc::new(move || {
            if observed.fetch_add(1, Ordering::AcqRel) == 0 {
                let receipt = exact.policy_handoff(from).unwrap().unwrap();
                assert_eq!(receipt.phase, VoicePolicyPhase::Pending);
                assert_eq!(receipt.generation, from + 1);
                assert_eq!(exact.policy_handoff(from + 1).unwrap(), Some(receipt));
                assert!(!receipt.enabled && !exact.policy_drivers_allowed().unwrap());
                assert!(matches!(
                    save(&exact, local),
                    Err(BackendError::Storage(StorageError::Busy))
                ));
                assert_eq!(exact.generation(), from + 1);
                assert_eq!(exact.policy_handoff(from).unwrap(), Some(receipt));
                checked.store(true, Ordering::Release);
            }
        }))
        .unwrap();
    let saved = save(&service, local);
    service.clear_waker().unwrap();
    saved.unwrap();
    assert!(
        verified.load(Ordering::Acquire),
        "waker assertions all completed"
    );
    assert!(seen.load(Ordering::Acquire) >= 2);
    let receipt = service.policy_handoff(from).unwrap().unwrap();
    assert_eq!(receipt.phase, VoicePolicyPhase::Committed);
    assert!(receipt.enabled && service.policy_drivers_allowed().unwrap());
}

#[test]
fn failed_writer_keeps_the_known_endpoint_closed_and_allows_only_negative_retirement() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let from = service.generation();
    let control = service.inner.control.clone();
    let fired = Arc::new(AtomicBool::new(false));
    let once = fired.clone();
    service
        .set_waker(Arc::new(move || {
            if !once.swap(true, Ordering::AcqRel) {
                control.lock_local();
            }
        }))
        .unwrap();
    assert!(save(&service, local).is_err());
    service.clear_waker().unwrap();
    let receipt = service.policy_handoff(from).unwrap().unwrap();
    assert_eq!(receipt.phase, VoicePolicyPhase::Failed);
    assert_eq!(
        receipt.generation,
        from + 1,
        "failure cannot invent another stop epoch"
    );
    assert_eq!(service.generation(), receipt.generation);
    assert!(!receipt.enabled && !service.is_admitting());
    assert!(service.policy_drivers_allowed().unwrap());
    assert_eq!(service.inventory().unwrap().limits, VoiceLimits::default());
    assert!(!service.inner.enabled.load(Ordering::Acquire));
    assert!(
        service.resume_after_stop(receipt.generation).is_err(),
        "Failed is never admission authority"
    );
}

#[test]
fn committed_disable_retires_normally_and_explicit_enable_updates_the_same_stop_serial() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
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
    let receipt = service
        .policy_handoff(before.runtime_generation)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, VoicePolicyPhase::Committed);
    assert!(!receipt.enabled && !service.is_admitting());
    assert!(service.policy_drivers_allowed().unwrap());
    assert!(
        service.resume_after_stop(receipt.generation).is_err(),
        "disabled Committed is not enable consent"
    );
    let disabled = service.management_snapshot(local).unwrap();
    service
        .set_enabled_reviewed(
            local,
            true,
            disabled.revision,
            disabled.identity_revision,
            disabled.runtime_generation,
        )
        .unwrap();
    let enabled = service
        .policy_handoff(before.runtime_generation)
        .unwrap()
        .unwrap();
    assert_eq!(
        enabled.serial, receipt.serial,
        "enable creates no new native stop"
    );
    assert_eq!(enabled.generation, receipt.generation);
    assert!(enabled.enabled && service.is_admitting());
}

#[test]
fn bounded_receipt_merges_contiguous_policy_epochs_but_never_an_independent_stop_gap() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let from = service.generation();
    let mut serial = 0;
    for index in 1..=8 {
        save(&service, local).unwrap();
        let receipt = service.policy_handoff(from).unwrap().unwrap();
        assert_eq!(receipt.from_generation, from);
        assert_eq!(receipt.generation, from + index);
        assert_eq!(receipt.phase, VoicePolicyPhase::Committed);
        assert!(receipt.serial > serial);
        serial = receipt.serial;
        let slot = service.inner.policy.lock().unwrap();
        assert!(slot.active.is_none());
        assert!(slot.receipt.is_some());
        assert!(
            std::mem::size_of::<PolicySlot>() <= 128,
            "one bounded receipt/work slot"
        );
    }
    let previous = service.generation();
    let independent = service.invalidate_runtime();
    assert_eq!(independent, previous + 1);
    assert!(service.policy_handoff(from).unwrap().is_none());
    save(&service, local).unwrap();
    assert!(service.policy_handoff(from).unwrap().is_none());
    let receipt = service.policy_handoff(independent).unwrap().unwrap();
    assert_eq!(receipt.from_generation, independent);
    assert_eq!(receipt.generation, independent + 1);
    assert!(service.resume_after_stop(previous).is_err());
}

#[test]
fn enable_of_a_running_policy_does_not_create_a_new_native_stop_receipt() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let queued = service
        .submit(
            web,
            fixtures::independent_request(&service, web, class, 2_881),
        )
        .unwrap();
    let starting = service
        .drive(VoiceDriverUpdate {
            job_id: queued.id,
            expected_revision: queued.revision,
            generation: queued.generation,
            action: VoiceDriverAction::Start,
        })
        .unwrap();
    let playing = service
        .drive(VoiceDriverUpdate {
            job_id: queued.id,
            expected_revision: starting.revision,
            generation: starting.generation,
            action: VoiceDriverAction::FirstWrite,
        })
        .unwrap();
    let guard = service.guard(playing.id).unwrap();
    let before = service.management_snapshot(local).unwrap();
    service
        .set_enabled_reviewed(
            local,
            true,
            before.revision,
            before.identity_revision,
            before.runtime_generation,
        )
        .unwrap();
    assert_eq!(service.generation(), before.runtime_generation);
    assert!(service.is_admitting());
    assert!(guard.guard.is_valid());
    assert!(service
        .policy_handoff(before.runtime_generation)
        .unwrap()
        .is_none());
    assert_eq!(
        service.inventory().unwrap().jobs[0].state,
        VoiceState::Playing
    );
}

#[test]
fn an_abandoned_post_commit_owner_never_publishes_committed_or_reopens_admission() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let from = service.generation();
    service.after_next_commit(Arc::new(|| panic!("synthetic abandoned policy owner")));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| save(&service, local))).is_err()
    );
    let receipt = service.policy_handoff(from).unwrap().unwrap();
    assert_eq!(receipt.phase, VoicePolicyPhase::Failed);
    assert!(!receipt.enabled && !service.is_admitting());
    assert_eq!(
        service.inventory().unwrap().limits,
        limits(),
        "actual SQL commit stays factual"
    );
    assert!(service.inner.policy.lock().unwrap().active.is_none());
}

#[test]
fn real_negative_driver_precommit_rolls_back_on_pending_and_can_later_settle_the_same_release() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let queued = service
        .submit(
            web,
            fixtures::independent_request(&service, web, class, 2_882),
        )
        .unwrap();
    let starting = service
        .drive(VoiceDriverUpdate {
            job_id: queued.id,
            expected_revision: queued.revision,
            generation: queued.generation,
            action: VoiceDriverAction::Start,
        })
        .unwrap();
    let stopping = service
        .cancel(
            web,
            VoiceCancelRequest {
                command_id: CommandId::new(2_883).unwrap(),
                job_id: queued.id,
                expected_revision: starting.revision,
            },
        )
        .unwrap();
    assert_eq!(stopping.state, VoiceState::Stopping);
    let update = VoiceDriverUpdate {
        job_id: stopping.id,
        expected_revision: stopping.revision,
        generation: stopping.release_generation.unwrap(),
        action: VoiceDriverAction::Released,
    };
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    let handle = backend.0.lock().unwrap().handle.clone();
    let revision = handle.try_revision().unwrap().wait().unwrap();
    let caller = std::thread::current().id();
    for fire_at in [2, 3] {
        let count = Arc::new(AtomicU64::new(0));
        let pending = Arc::new(Mutex::new(None));
        let stored = pending.clone();
        let exact = service.clone();
        let checks = count.clone();
        let repository = service
            .repository(epoch, None)
            .with_voice_driver_guard(Arc::new(move || {
                if checks.fetch_add(1, Ordering::AcqRel) + 1 == fire_at {
                    assert_ne!(
                        std::thread::current().id(),
                        caller,
                        "real writer evaluation/precommit"
                    );
                    *stored.lock().unwrap() =
                        Some(exact.begin_policy_change(epoch, epoch + 1, true).unwrap());
                }
                exact.policy_drivers_allowed().unwrap()
            }));
        let result = service.conclude(repository.drive_voice(update.clone()));
        assert!(matches!(
            result,
            Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration
            )))
        ));
        assert_eq!(count.load(Ordering::Acquire), fire_at);
        assert!(
            pending.lock().unwrap().is_some(),
            "Pending was actually published on the writer"
        );
        assert_eq!(service.inventory().unwrap(), before);
        assert_eq!(handle.try_revision().unwrap().wait().unwrap(), revision);
        assert_eq!(
            service.generation(),
            epoch,
            "Pending driver refusal never invalidates the policy endpoint"
        );
        let abandoned = pending.lock().unwrap().take();
        drop(abandoned);
        assert!(service.policy_drivers_allowed().unwrap());
    }
    let released = service.drive(update).unwrap();
    assert!(
        released.state.terminal(),
        "the exact unchanged release can settle after Pending"
    );
}
