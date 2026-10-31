use super::*;

#[test]
fn request_voice_fences_only_tighten_the_clone_and_never_close_the_shared_runtime() {
    let _scenario = tests::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, class) = tests::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    for fence in [
        Arc::new(|| false) as Arc<dyn Fn() -> bool + Send + Sync>,
        Arc::new(|| panic!("request fence panic")),
    ] {
        let scoped = service
            .with_execution_fence(fence)
            .with_execution_fence(Arc::new(|| true));
        assert!(matches!(
            scoped.reserve(web, class),
            Err(BackendError::Storage(StorageError::ExecutionCancelled))
        ));
        assert!(matches!(
            scoped.epoch(),
            Err(BackendError::Storage(StorageError::ExecutionCancelled))
        ));
        assert_eq!(service.inventory().unwrap(), before);
        assert_eq!(service.generation(), epoch);
        assert!(service.is_admitting());
    }
    let keep = Arc::new(AtomicBool::new(true));
    let observed = keep.clone();
    let scoped = service.with_execution_fence(Arc::new(move || observed.load(Ordering::Acquire)));
    assert!(scoped.check_open().is_ok());
    keep.store(false, Ordering::Release);
    assert!(matches!(
        scoped.check_open(),
        Err(BackendError::Storage(StorageError::ExecutionCancelled))
    ));
    assert!(service.check_open().is_ok());
    assert_eq!(service.inventory().unwrap(), before);
    let reads = Arc::new(AtomicU64::new(0));
    let seen = reads.clone();
    let racing =
        service.with_execution_fence(Arc::new(move || seen.fetch_add(1, Ordering::AcqRel) == 0));
    assert!(matches!(
        racing.epoch(),
        Err(BackendError::Storage(StorageError::ExecutionCancelled))
    ));
    assert_eq!(service.generation(), epoch);
    let stopped = service.invalidate_runtime();
    for current in [
        service.clone(),
        service.with_execution_fence(Arc::new(|| true)),
    ] {
        assert!(matches!(
            current.epoch(),
            Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration
            )))
        ));
    }
    assert!(matches!(
        scoped.epoch(),
        Err(BackendError::Storage(StorageError::ExecutionCancelled))
    ));
    assert_eq!(service.generation(), stopped);
    assert!(!service.is_admitting());
}

#[test]
fn request_voice_fence_reaches_the_actual_writer_without_faulting_or_late_reservation() {
    let _scenario = tests::VOICE_FIXTURE
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let (_directory, backend, web, class) = tests::shared_fixture();
    let service = backend.0.lock().unwrap().voice_service();
    let before = service.inventory().unwrap();
    let epoch = service.generation();
    let reached = Arc::new(AtomicBool::new(false));
    let observed = reached.clone();
    let scoped = service.with_execution_fence(Arc::new(move || {
        let writer = std::thread::current().name() == Some("fc-sqlite-writer");
        if writer {
            observed.store(true, Ordering::Release);
        }
        !writer
    }));
    assert!(scoped.reserve(web, class).is_err());
    assert!(
        reached.load(Ordering::Acquire),
        "entry checks must not mask missing writer propagation"
    );
    assert_eq!(service.inventory().unwrap(), before);
    assert_eq!(service.generation(), epoch);
    assert!(service.is_admitting());
    assert_eq!(
        service.inner.control.health_gate().status().health,
        Health::Healthy
    );
}
