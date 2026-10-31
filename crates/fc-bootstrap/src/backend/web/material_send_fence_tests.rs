fn setup() -> (Fixture, WebService, IssuedWebSession, MaterialScope) {
    let mut fixture = Fixture::new();
    fixture
        .backend
        .identity(
            fixture.admin,
            IdentityCommand::PutSubject {
                id: SubjectId::new(1).unwrap(),
                name: "Synthetic download subject".into(),
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
    let service = fixture.service();
    service.set_enabled(fixture.admin, true).unwrap();
    let session = fixture.grant("admin", "download send fixture");
    let scope = MaterialScope {
        teacher_id: fixture.admin.principal_id(),
        subject_id: SubjectId::new(1).unwrap(),
    };
    (fixture, service, session, scope)
}
fn mint(
    service: &WebService,
    session: &IssuedWebSession,
    scope: MaterialScope,
) -> TransactionExecutionFence {
    let lease = service.authenticate(&session.cookie).unwrap().lease();
    service
        .material_send_fence(lease, scope, &[], Arc::new(|| true))
        .unwrap()
        .1
}
#[test]
fn material_send_fence_try_locks_never_wait_and_local_lock_does_not_revoke_web() {
    let (fixture, service, session, scope) = setup();
    let fence = mint(&service, &session, scope);
    assert!(fence());
    // Same-thread ownership would deadlock a waiting lock, so these are actual
    // nonwaiting lock-contention checks without a generous timing threshold.
    {
        let _state = service.inner.state.lock().unwrap();
        assert!(!fence());
    }
    {
        let _ledger = service.inner.ledger.lock().unwrap();
        assert!(!fence());
    }
    assert!(fence());
    fixture.backend.session_control().lock_local();
    assert!(fence());
    assert!(service.authenticate(&session.cookie).is_ok());
    assert!(
        fence(),
        "synchronizing a local lock must retain this exact Web lease"
    );
}
#[test]
fn material_send_fence_rechecks_exact_device_enablement_and_every_publication() {
    let (mut fixture, service, session, scope) = setup();
    let fence = mint(&service, &session, scope);
    service
        .revoke_device(fixture.admin, session.device.device_id)
        .unwrap();
    assert!(!fence());
    let next = fixture.grant("admin", "next exact download device");
    let fence = mint(&service, &next, scope);
    service.set_enabled(fixture.admin, false).unwrap();
    assert!(!fence());
    service.set_enabled(fixture.admin, true).unwrap();
    assert!(!fence(), "reenabling cannot revive the revoked exact lease");
    let next = fixture.grant("admin", "publication fixture");
    let fence = mint(&service, &next, scope);
    fixture.create_class(1);
    assert!(
        !fence(),
        "even unrelated current publication cancels the old send"
    );
    assert!(mint(&service, &next, scope)());
}
#[test]
fn material_send_fence_uses_original_session_expiry_and_latches_clock_regression() {
    let (fixture, service, session, scope) = setup();
    let fence = mint(&service, &session, scope);
    fixture.clock.advance(WEB_SESSION_MILLIS);
    assert!(
        !fence(),
        "reads never renew the original absolute Web session"
    );
    drop(fixture);
    let (fixture, service, session, scope) = setup();
    let fence = mint(&service, &session, scope);
    fixture.clock.tick.store(999, Ordering::Release);
    assert!(!fence());
    fixture.clock.tick.store(1000, Ordering::Release);
    assert!(
        !fence(),
        "a later tick cannot revive a detected clock regression"
    );
}
#[test]
fn material_send_fence_preserves_original_transfer_cancellation_and_closed_state() {
    let (fixture, service, session, scope) = setup();
    let active = Arc::new(AtomicBool::new(true));
    let lifetime = active.clone();
    let lease = service.authenticate(&session.cookie).unwrap().lease();
    let fence = service
        .material_send_fence(
            lease,
            scope,
            &[],
            Arc::new(move || lifetime.load(Ordering::Acquire)),
        )
        .unwrap()
        .1;
    assert!(fence());
    active.store(false, Ordering::Release);
    assert!(!fence());
    let fence = mint(&service, &session, scope);
    fixture
        .backend
        .session_control()
        .0
        .closed
        .store(true, Ordering::Release);
    assert!(!fence());
}
