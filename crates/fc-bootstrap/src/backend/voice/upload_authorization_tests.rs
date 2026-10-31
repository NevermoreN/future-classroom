//! Regression contracts for memory-only upload checkpoints. These tests use
//! the real sole writer's publication barrier, not a simulated GateError.
//! No PCM/file operation or reserve/publish command is retried by a checkpoint.
use super::tests as fixtures;
use super::*;

struct Release(Arc<AtomicBool>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

struct HoldPublication {
    snapshot: Snapshot,
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}
impl fc_storage::TransactionTask<Snapshot> for HoldPublication {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + 256
    }
    fn evaluate(
        self: Box<Self>,
        _: &dyn fc_storage::TransactionDocuments,
    ) -> std::result::Result<fc_storage::TransactionDecision<Snapshot>, StorageError> {
        let Self {
            snapshot,
            entered,
            release,
        } = *self;
        Ok(fc_storage::TransactionDecision {
            outcome: Vec::new(),
            commit: Some((
                vec![fc_storage::Mutation::Put {
                    key: "synthetic/voice/upload-publication".into(),
                    value: vec![1],
                }],
                snapshot,
            )),
            precommit: Some(Box::new(move || {
                entered.store(true, Ordering::Release);
                let deadline = Instant::now() + Duration::from_secs(10);
                while !release.load(Ordering::Acquire) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            })),
        })
    }
}

fn entered(ready: &AtomicBool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "real writer publication did not start"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn hold_publication(
    service: &VoiceService,
    handle: &StorageHandle<Snapshot>,
) -> (fc_storage::Ticket<Vec<u8>>, Arc<AtomicBool>) {
    let current = service.inner.authority.snapshot().unwrap();
    let status = service.inner.control.health_gate().status();
    let snapshot = service
        .inner
        .authority
        .prepare_update(
            current.data.clone(),
            status.revision.next().unwrap(),
            status.auth_epoch,
        )
        .unwrap();
    let ready = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let ticket = handle
        .try_transaction(HoldPublication {
            snapshot,
            entered: ready.clone(),
            release: release.clone(),
        })
        .unwrap();
    entered(&ready);
    assert_eq!(
        service.inner.control.health_gate().status().health,
        Health::AuthorityUpdating
    );
    (ticket, release)
}

fn check(
    service: &VoiceService,
    lease: SessionLease,
    class: ClassId,
    asset: &VoiceAsset,
    epoch: u64,
    upload: bool,
) -> Result<()> {
    if upload {
        service.check_web_upload(lease, asset, epoch)
    } else {
        service.check_web_target(lease, class, epoch)
    }
}

#[test]
fn upload_checkpoints_wait_for_short_unrelated_publication_in_the_same_exact_epoch() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, lease, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let handle = backend.0.lock().unwrap().handle.clone();
    let asset = service.reserve(lease, class).unwrap();
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    for upload in [false, true] {
        let (ticket, release) = hold_publication(&service, &handle);
        let _release = Release(release.clone());
        let outcome = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(10));
                release.store(true, Ordering::Release);
            });
            check(&service, lease, class, &asset, epoch, upload)
        });
        ticket.wait().unwrap();
        assert!(
            outcome.is_ok(),
            "upload={upload}: transient publication is uncertainty, not revocation: {outcome:?}"
        );
        assert_eq!(service.generation(), epoch);
        assert!(service.is_admitting());
        assert_eq!(
            service.inventory().unwrap(),
            before,
            "memory checkpoint never replays SQL"
        );
    }
}

#[test]
fn upload_checkpoints_bound_publication_wait_and_leave_runtime_and_reservation_unchanged() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, lease, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let handle = backend.0.lock().unwrap().handle.clone();
    let asset = service.reserve(lease, class).unwrap();
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    for upload in [false, true] {
        let (ticket, release) = hold_publication(&service, &handle);
        let _release = Release(release.clone());
        let begin = Instant::now();
        let outcome = std::thread::scope(|scope| {
            let waiting = scope.spawn(|| check(&service, lease, class, &asset, epoch, upload));
            std::thread::sleep(Duration::from_millis(10));
            assert!(
                service.inner.ledger.try_lock().is_ok(),
                "no ledger lock across publication wait"
            );
            waiting.join().unwrap()
        });
        let elapsed = begin.elapsed();
        release.store(true, Ordering::Release);
        ticket.wait().unwrap();
        assert!(
            matches!(outcome, Err(BackendError::Storage(StorageError::Busy))),
            "upload={upload}: {outcome:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(75) && elapsed < Duration::from_millis(500),
            "upload={upload}: existing 100 ms bound, elapsed={elapsed:?}"
        );
        assert_eq!(service.generation(), epoch);
        assert!(service.is_admitting());
        assert_eq!(service.inventory().unwrap(), before);
        check(&service, lease, class, &asset, epoch, upload).unwrap();
    }
}

#[test]
fn upload_publication_wait_never_adopts_a_replacement_lease_or_runtime_epoch() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    for kind in ["replacement", "revocation", "epoch"] {
        for upload in [false, true] {
            // Each case owns one real reservation in an independent database.
            // Do not accumulate unused reserved quota across unrelated cases.
            let (_directory, backend, _original, class) = fixtures::shared_fixture();
            let service = backend.0.lock().unwrap().voice_service();
            let handle = backend.0.lock().unwrap().handle.clone();
            let lease = fixtures::issue(&backend.0.lock().unwrap(), TrustedOrigin::Web);
            let asset = service.reserve(lease, class).unwrap();
            let before = service.inventory().unwrap();
            let epoch = service.generation();
            let identity = before_identity(&service);
            let (ticket, release) = hold_publication(&service, &handle);
            let _release = Release(release.clone());
            let outcome = std::thread::scope(|scope| {
                scope.spawn(|| {
                    std::thread::sleep(Duration::from_millis(10));
                    match kind {
                        "replacement" => {
                            // Same principal and origin, new exact lease; read
                            // the immutable already-held identity, not SQLite.
                            service
                                .inner
                                .ledger
                                .lock()
                                .unwrap()
                                .sessions
                                .issue_verified(
                                    identity.account(lease.principal_id()).unwrap(),
                                    TrustedOrigin::Web,
                                    0,
                                    SESSION_MILLIS,
                                    LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 3600)
                                        .unwrap(),
                                )
                                .unwrap();
                        }
                        "revocation" => {
                            assert!(service
                                .inner
                                .ledger
                                .lock()
                                .unwrap()
                                .sessions
                                .revoke_exact(lease));
                        }
                        "epoch" => {
                            service.invalidate_runtime();
                        }
                        _ => unreachable!(),
                    }
                    release.store(true, Ordering::Release);
                });
                check(&service, lease, class, &asset, epoch, upload)
            });
            ticket.wait().unwrap();
            assert!(matches!(outcome,
                Err(BackendError::Session(SessionError::Revoked))
                | Err(BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::StaleGeneration)))
                | Err(BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::Permission(PermissionError::StaleAuthorization))))),
                "{kind} upload={upload}: old checkpoint must observe actual revocation, not a transient Closed: {outcome:?}");
            assert_eq!(service.inventory().unwrap(), before);
            if kind == "epoch" {
                assert_eq!(service.generation(), epoch + 1);
                assert!(!service.is_admitting());
                service.resume_after_stop(epoch + 1).unwrap();
                assert!(
                    check(&service, lease, class, &asset, epoch, upload).is_err(),
                    "resumption cannot retarget the original checkpoint to the new epoch"
                );
            } else {
                assert_eq!(service.generation(), epoch);
            }
        }
    }
}

fn before_identity(service: &VoiceService) -> IdentityState {
    // This helper is called only before entering a publication barrier.
    service.inner.authority.snapshot().unwrap().data.clone()
}

#[test]
fn upload_wait_rechecks_the_actual_new_class_binding_after_publication_without_sql_replay() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, lease, class) = fixtures::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let other = ClassId::new(2).unwrap();
    let (local, mut repository, request, actor, clock) = {
        let mut backend = backend.0.lock().unwrap();
        let local = fixtures::issue(&backend, TrustedOrigin::Local);
        backend
            .identity(
                local,
                IdentityCommand::CreateClass {
                    id: other,
                    name: "New upload binding".into(),
                },
            )
            .unwrap();
        let state = backend.state().unwrap();
        let actor = backend
            .actor(&state, local, SessionPurpose::Manage)
            .unwrap()
            .0;
        let request = backend
            .request(&state, IdentityCommand::BindCurrentClass { id: other })
            .unwrap();
        (
            local,
            backend.repository.clone(),
            request,
            actor,
            backend.clock.clone(),
        )
    };
    let asset = service.reserve(lease, class).unwrap();
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    let ready = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let _release = Release(release.clone());
    let writer_ready = ready.clone();
    let writer_release = release.clone();
    let exact = service.clone();
    repository = repository.with_session_guard(Arc::new(move |state, expected| {
        if exact
            .actor_from_state(local, state)
            .map_err(|_| StorageError::AuthorityUnavailable)?
            != *expected
        {
            return Err(StorageError::AuthorityUnavailable);
        }
        if exact.inner.control.health_gate().status().health == Health::AuthorityUpdating {
            writer_ready.store(true, Ordering::Release);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !writer_release.load(Ordering::Acquire) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }));
    let (target, upload) = std::thread::scope(|scope| {
        let identity = scope.spawn(move || {
            fc_ports::IdentityRepository::execute_identity(
                &mut repository,
                request,
                Some(&actor),
                clock.as_ref(),
            )
        });
        entered(&ready);
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(10));
            release.store(true, Ordering::Release);
        });
        let target = scope.spawn(|| service.check_web_target(lease, class, epoch));
        let upload = scope.spawn(|| service.check_web_upload(lease, &asset, epoch));
        let outcomes = (target.join().unwrap(), upload.join().unwrap());
        identity.join().unwrap().unwrap();
        outcomes
    });
    for outcome in [target, upload] {
        assert!(
            matches!(
                outcome,
                Err(BackendError::Voice(VoiceRepositoryError::Domain(
                    VoiceError::TargetMismatch
                )))
            ),
            "waiter must inspect the newly committed real target: {outcome:?}"
        );
    }
    assert_eq!(service.generation(), epoch);
    assert_eq!(service.inventory().unwrap(), before);
    assert_eq!(
        backend
            .0
            .lock()
            .unwrap()
            .state()
            .unwrap()
            .installation
            .unwrap()
            .current_class,
        Some(other)
    );
}

struct SlowOnce {
    source: Arc<dyn MonotonicClock + Send + Sync>,
    armed: AtomicBool,
}
impl MonotonicClock for SlowOnce {
    fn monotonic_millis(&self) -> u64 {
        if self.armed.swap(false, Ordering::AcqRel) {
            std::thread::sleep(Duration::from_millis(125));
        }
        self.source.monotonic_millis()
    }
}

#[test]
fn upload_checkpoint_checks_original_deadline_even_after_a_coherent_successful_authorization() {
    let _fixture = fixtures::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, lease, class) = fixtures::shared_fixture();
    let original = backend.0.lock().unwrap().voice_service();
    let clock = Arc::new(SlowOnce {
        source: original.inner.monotonic.clone(),
        armed: AtomicBool::new(false),
    });
    let service = VoiceService::new(
        original.inner.repository.clone(),
        original.inner.ledger.clone(),
        original.inner.control.clone(),
        original.inner.authority.clone(),
        original.inner.clock.clone(),
        clock.clone(),
    );
    let asset = service.reserve(lease, class).unwrap();
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    for upload in [false, true] {
        clock.armed.store(true, Ordering::Release);
        let outcome = check(&service, lease, class, &asset, epoch, upload);
        assert!(
            !clock.armed.load(Ordering::Acquire),
            "actual authorization passed through the slow test clock"
        );
        assert!(matches!(outcome, Err(BackendError::Storage(StorageError::Busy))),
            "upload={upload}: a valid actor is not permission after the original deadline: {outcome:?}");
        assert_eq!(service.generation(), epoch);
        assert!(service.is_admitting());
        assert_eq!(service.inventory().unwrap(), before);
        check(&service, lease, class, &asset, epoch, upload).unwrap();
    }
}
