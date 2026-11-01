// Real credential/session and unique SQLite-writer regressions. Native-ready
// metadata is injected only to test the service boundary, not TLS readiness.
#[test]
fn runtime_start_reservation_excludes_configuration_and_legacy_enable_until_return() {
    let f = Fixture::new();
    let service = f.service();
    let permit = service.reserve_runtime(f.admin).unwrap();
    assert!(matches!(
        service.configuration_edit(f.admin),
        Err(WebServiceError::Busy)
    ));
    assert_eq!(
        service.set_enabled(f.admin, true),
        Err(WebServiceError::Busy)
    );
    let held = permit.clone();
    drop(permit);
    assert!(matches!(
        service.reserve_runtime(f.admin),
        Err(WebServiceError::Busy)
    ));
    drop(held);
    assert!(service.configuration_edit(f.admin).is_ok());
}
#[test]
fn independent_stop_cannot_be_adopted_by_late_native_ready_cas() {
    let f = Fixture::new();
    let service = f.service();
    let permit = service.reserve_runtime(f.admin).unwrap();
    assert!(permit.current());
    service.negative_runtime_stop();
    assert_eq!(permit.activate(7), Err(WebServiceError::Disabled));
    assert!(!service.runtime_execution_current());
    assert_eq!(service.runtime_generation(), None);
    assert!(!service.management_snapshot(f.admin).unwrap().enabled);
}
#[test]
fn runtime_readiness_requires_original_exact_local_lease_after_lock() {
    let mut f = Fixture::new();
    let service = f.service();
    let permit = service.reserve_runtime(f.admin).unwrap();
    f.backend.session_control().lock_local();
    f.login_admin();
    assert!(!permit.current());
    assert!(permit.activate(8).is_err());
    assert!(!service.runtime_execution_current());
    drop(permit);
    let fresh = service.reserve_runtime(f.admin).unwrap();
    fresh.activate(9).unwrap();
    assert_eq!(service.runtime_generation(), Some(9));
    assert!(service.begin_pairing_for_runtime(f.admin, 8).is_err());
    assert!(service.begin_pairing_for_runtime(f.admin, 9).is_ok());
    service.negative_runtime_stop();
    service.settle_runtime_stop();
    fresh.physical_returned();
}
#[test]
fn restarting_runtime_revokes_old_exact_web_lease_before_new_epoch_is_positive() {
    let f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let old = f.grant("admin", "old synthetic generation");
    let old_lease = service.authenticate(&old.cookie).unwrap().lease();
    let actor = f
        .backend
        .check_session(old_lease, SessionPurpose::Manage)
        .unwrap();
    let guard = f.backend.session_guard(
        old_lease,
        f.backend.control.generation(),
        SessionPurpose::Manage,
    );
    let identity = f.backend.state().unwrap();
    assert!(guard(&identity, &actor).is_ok());
    service.negative_runtime_stop();
    service.settle_runtime_stop();
    let permit = service.reserve_runtime(f.admin).unwrap();
    permit.activate(11).unwrap();
    assert!(service.runtime_execution_current());
    assert!(guard(&identity, &actor).is_err());
    assert!(service.authenticate(&old.cookie).is_err());
    service.negative_runtime_stop();
    service.settle_runtime_stop();
    permit.physical_returned();
}
#[test]
fn negative_stop_while_ledger_held_rejects_original_sql_write_at_real_precommit() {
    use fc_storage::{Mutation, TransactionDecision, TransactionDocuments, TransactionTask};
    use std::sync::mpsc;
    struct Write {
        guard: fc_storage::business::SessionGuard,
        identity: IdentityState,
        actor: ActorContext,
        snapshot: Snapshot,
        arrived: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }
    impl TransactionTask<Snapshot> for Write {
        fn allocation_bytes(&self) -> usize {
            1024
        }
        fn evaluate(
            self: Box<Self>,
            _: &dyn TransactionDocuments,
        ) -> std::result::Result<TransactionDecision<Snapshot>, StorageError> {
            let Write {
                guard,
                identity,
                actor,
                snapshot,
                arrived,
                release,
            } = *self;
            guard(&identity, &actor)?;
            Ok(TransactionDecision {
                outcome: vec![],
                commit: Some((
                    vec![Mutation::Put {
                        key: "synthetic/runtime-negative-write".into(),
                        value: b"MUST_NOT_COMMIT".to_vec(),
                    }],
                    snapshot,
                )),
                precommit: Some(Box::new(move || {
                    arrived
                        .send(())
                        .map_err(|_| StorageError::AuthorityUnavailable)?;
                    release
                        .recv_timeout(Duration::from_secs(3))
                        .map_err(|_| StorageError::AuthorityUnavailable)?;
                    guard(&identity, &actor)
                })),
            })
        }
    }
    let f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let issued = f.grant("admin", "original writer epoch");
    let lease = service.authenticate(&issued.cookie).unwrap().lease();
    let actor = f
        .backend
        .check_session(lease, SessionPurpose::Manage)
        .unwrap();
    let current = f.backend.authority.snapshot().unwrap();
    let identity = current.data.clone();
    let next = current.revision.next().unwrap();
    let candidate = f
        .backend
        .authority
        .prepare_update(identity.clone(), next, current.auth_epoch)
        .unwrap();
    let (arrived, wait) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    let ticket = f
        .backend
        .handle
        .try_transaction(Write {
            guard: f.backend.session_guard(
                lease,
                f.backend.control.generation(),
                SessionPurpose::Manage,
            ),
            identity,
            actor,
            snapshot: candidate,
            arrived,
            release: resume,
        })
        .unwrap();
    wait.recv_timeout(Duration::from_secs(3)).unwrap();
    {
        // A waiting negative stop or precommit guard would deadlock against this
        // exact held ledger. The atomic negative path must return without it.
        let _ledger = f.backend.ledger.lock().unwrap();
        service.negative_runtime_stop();
        release.send(()).unwrap();
        assert!(matches!(
            ticket.wait(),
            Err(StorageError::AuthorityUnavailable)
        ));
    }
    assert_eq!(
        f.backend.authority.snapshot().unwrap().revision,
        current.revision
    );
    assert_eq!(
        f.backend.control.health_gate().status().health,
        Health::Healthy
    );
    service.settle_runtime_stop();
}

#[test]
fn queued_sql_write_is_negative_before_ledger_after_original_runtime_stop() {
    use fc_storage::{Mutation, TransactionDecision, TransactionDocuments, TransactionTask};
    use std::sync::mpsc;
    struct Block {
        arrived: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }
    impl TransactionTask<Snapshot> for Block {
        fn allocation_bytes(&self) -> usize {
            256
        }
        fn evaluate(
            self: Box<Self>,
            _: &dyn TransactionDocuments,
        ) -> std::result::Result<TransactionDecision<Snapshot>, StorageError> {
            self.arrived.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(TransactionDecision {
                outcome: vec![],
                commit: None,
                precommit: None,
            })
        }
    }
    struct Queued {
        guard: fc_storage::business::SessionGuard,
        identity: IdentityState,
        actor: ActorContext,
        candidate: Snapshot,
    }
    impl TransactionTask<Snapshot> for Queued {
        fn allocation_bytes(&self) -> usize {
            1024
        }
        fn evaluate(
            self: Box<Self>,
            _: &dyn TransactionDocuments,
        ) -> std::result::Result<TransactionDecision<Snapshot>, StorageError> {
            (self.guard)(&self.identity, &self.actor)?;
            Ok(TransactionDecision {
                outcome: vec![],
                commit: Some((
                    vec![Mutation::Put {
                        key: "synthetic/queued-runtime-write".into(),
                        value: b"MUST_NOT_COMMIT".to_vec(),
                    }],
                    self.candidate,
                )),
                precommit: None,
            })
        }
    }
    let f = Fixture::new();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    let issued = f.grant("admin", "queued original epoch");
    let lease = service.authenticate(&issued.cookie).unwrap().lease();
    let actor = f
        .backend
        .check_session(lease, SessionPurpose::Manage)
        .unwrap();
    let current = f.backend.authority.snapshot().unwrap();
    let candidate = f
        .backend
        .authority
        .prepare_update(
            current.data.clone(),
            current.revision.next().unwrap(),
            current.auth_epoch,
        )
        .unwrap();
    let (arrived, wait) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    let first = f
        .backend
        .handle
        .try_transaction(Block {
            arrived,
            release: resume,
        })
        .unwrap();
    wait.recv_timeout(Duration::from_secs(3)).unwrap();
    let queued = f
        .backend
        .handle
        .try_transaction(Queued {
            guard: f.backend.session_guard(
                lease,
                f.backend.control.generation(),
                SessionPurpose::Manage,
            ),
            identity: current.data.clone(),
            actor,
            candidate,
        })
        .unwrap();
    {
        let _ledger = f.backend.ledger.lock().unwrap();
        service.negative_runtime_stop();
        assert!(
            !service.settle_runtime_stop(),
            "negative settle never waits for held ledger"
        );
        release.send(()).unwrap();
        first.wait().unwrap();
        assert!(matches!(
            queued.wait(),
            Err(StorageError::AuthorityUnavailable)
        ));
    }
    assert_eq!(
        f.backend.authority.snapshot().unwrap().revision,
        current.revision
    );
    assert!(service.settle_runtime_stop());
    assert_eq!(
        f.backend.control.health_gate().status().health,
        Health::Healthy
    );
}

#[test]
fn returned_old_reservation_cannot_clear_new_starting_or_stopping_exact_claim() {
    for stopping in [false, true] {
        let f = Fixture::new();
        let service = f.service();
        let old = service.reserve_runtime(f.admin).unwrap();
        old.mark_started();
        old.activate(31).unwrap();
        assert!(old.negative_stop());
        assert!(service.settle_runtime_stop());
        old.physical_returned();
        let delayed = old.clone();
        drop(old);
        let current = service.reserve_runtime(f.admin).unwrap();
        current.mark_started();
        if stopping {
            current.activate(32).unwrap();
            assert!(current.negative_stop());
            assert!(service.settle_runtime_stop());
        }
        // Both Starting and Stopping have logical enabled=false. A bool-only
        // test therefore cannot distinguish this exact active physical owner.
        assert!(!service.management_snapshot(f.admin).unwrap().enabled);
        let original_claim = service.inner.runtime_claim.load(Ordering::Acquire);
        let original_epoch = service.inner.runtime_epoch.load(Ordering::Acquire);
        assert_ne!(original_claim, 0);
        delayed.physical_returned();
        delayed.physical_returned();
        assert!(!delayed.negative_stop());
        drop(delayed);
        assert_eq!(
            service.inner.runtime_claim.load(Ordering::Acquire),
            original_claim
        );
        assert_eq!(
            service.inner.runtime_epoch.load(Ordering::Acquire),
            original_epoch
        );
        assert!(matches!(
            service.configuration_edit(f.admin),
            Err(WebServiceError::Busy)
        ));
        assert!(matches!(
            service.reserve_runtime(f.admin),
            Err(WebServiceError::Busy)
        ));
        if !stopping {
            current.activate(33).unwrap();
            assert_eq!(service.runtime_generation(), Some(33));
            assert!(current.negative_stop());
            assert!(service.settle_runtime_stop());
        }
        current.physical_returned();
        assert!(service.configuration_edit(f.admin).is_ok());
    }
}

#[test]
fn delayed_old_negative_stop_cannot_revoke_new_running_generation() {
    let f = Fixture::new();
    let service = f.service();
    let old = service.reserve_runtime(f.admin).unwrap();
    old.mark_started();
    old.activate(41).unwrap();
    assert!(old.negative_stop());
    assert!(service.settle_runtime_stop());
    old.physical_returned();
    let current = service.reserve_runtime(f.admin).unwrap();
    current.mark_started();
    current.activate(42).unwrap();
    assert!(!old.negative_stop());
    old.physical_returned();
    drop(old);
    assert_eq!(service.runtime_generation(), Some(42));
    assert!(service.runtime_execution_current());
    assert!(service.management_snapshot(f.admin).unwrap().enabled);
    assert!(service.begin_pairing_for_runtime(f.admin, 42).is_ok());
    assert!(matches!(
        service.configuration_edit(f.admin),
        Err(WebServiceError::Busy)
    ));
    assert!(current.negative_stop());
    assert!(service.settle_runtime_stop());
    current.physical_returned();
}
