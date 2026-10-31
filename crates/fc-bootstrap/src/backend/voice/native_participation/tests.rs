//! Participation is a bounded metadata fence, never a native Released receipt
//! or evidence that an OS thread/TLS destructor has physically terminated.
use super::super::tests as fixtures;
use super::*;

fn save(service: &VoiceService, local: SessionLease) -> Result<VoiceSnapshot> {
    let review = service.management_snapshot(local)?;
    service.set_limits_reviewed(
        local,
        VoiceLimits {
            max_repeat_total: 2,
            ..VoiceLimits::default()
        },
        review.revision,
        review.identity_revision,
        review.runtime_generation,
    )
}

#[test]
fn only_one_native_domain_can_participate_and_registration_has_no_business_effects() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    let guard = service.register_native_domain().unwrap();
    assert!(service.has_native_domain());
    assert!(matches!(
        service.register_native_domain(),
        Err(BackendError::Storage(StorageError::Busy))
    ));
    assert_eq!(service.generation(), epoch);
    assert!(service.is_admitting());
    assert_eq!(service.inventory().unwrap(), before);
    drop(guard);
    assert!(!service.has_native_domain());
    let replacement = service.register_native_domain().unwrap();
    assert!(service.has_native_domain());
    drop(replacement);
    assert!(!service.has_native_domain());
    assert_eq!(service.inventory().unwrap(), before);
}

#[test]
fn terminal_database_jobs_do_not_authorize_helper_resume_while_a_domain_still_participates() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let guard = service.register_native_domain().unwrap();
    let from = service.generation();
    save(&service, local).unwrap();
    let receipt = service.policy_handoff(from).unwrap().unwrap();
    assert_eq!(receipt.phase, VoicePolicyPhase::Committed);
    assert!(receipt.enabled);
    assert!(service
        .inventory()
        .unwrap()
        .jobs
        .iter()
        .all(|job| job.state.terminal()));
    assert!(
        !service.is_admitting(),
        "old domain has not entered its supervisor's retirement section"
    );
    assert!(!service.management_snapshot(local).unwrap().admitting);
    drop(guard);
    assert!(!service.has_native_domain());
    assert!(
        !service.is_admitting(),
        "guard drop alone is not release evidence or fresh consent"
    );
    // No native device/thread was constructed by this host test. A new local
    // explicit save can now use the ordinary no-domain path and exact epoch.
    save(&service, local).unwrap();
    assert!(service.is_admitting());
    assert_eq!(service.generation(), receipt.generation + 1);
}

#[test]
fn participation_never_blocks_atomic_independent_stop_or_extends_old_policy_authority() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, _web, _class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let local = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Local);
    let guard = service.register_native_domain().unwrap();
    let from = service.generation();
    save(&service, local).unwrap();
    let old = service.generation();
    let independent = service.invalidate_runtime();
    assert_eq!(independent, old + 1);
    assert!(service.has_native_domain());
    assert!(service.policy_handoff(from).unwrap().is_none());
    assert!(service.resume_after_stop(old).is_err());
    assert!(!service.is_admitting());
    drop(guard);
    assert!(!service.is_admitting());
}
