//! Uses the existing real-credential Web/SQLite fixture without reducing KDF
//! strength, introducing fake accounts, or creating a second execution pool.
use super::*;
use fc_domain::https_settings::*;
use fc_ports::{HttpsSettingsRequest, HttpsSettingsState};
fn configuration() -> HttpsConfiguration {
    HttpsConfiguration::new(
        HttpsHost::parse("Board.School").unwrap(),
        8443,
        HttpsCertificateRef::parse(&"ab".repeat(20)).unwrap(),
    )
    .unwrap()
}
fn request(id: u64, base: &HttpsSettingsState) -> HttpsSettingsRequest {
    HttpsSettingsRequest {
        command_id: CommandId::new(id).unwrap(),
        expected_revision: base.settings.revision,
        expected_identity_revision: base.identity_revision,
        configuration: Some(configuration()),
    }
}
#[test]
fn local_configuration_only_save_replay_and_physical_reopen_leave_web_disabled() {
    let mut f = Fixture::new();
    let base = f.backend.read_https_settings(f.admin).unwrap();
    assert!(base.settings.configuration.is_none());
    let saved = f
        .backend
        .save_https_settings(f.admin, request(100, &base))
        .unwrap();
    assert_eq!(
        f.backend
            .save_https_settings(f.admin, request(100, &base))
            .unwrap(),
        saved
    );
    assert!(!f.service().management_snapshot(f.admin).unwrap().enabled);
    assert!(!f.service().configuration_in_progress());
    f.reopen();
    assert_eq!(f.backend.read_https_settings(f.admin).unwrap(), saved);
    assert!(!f.service().management_snapshot(f.admin).unwrap().enabled);
}
#[test]
fn real_web_global_session_active_web_and_canceled_scope_cannot_save_https_configuration() {
    let mut f = Fixture::new();
    let base = f.backend.read_https_settings(f.admin).unwrap();
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    assert!(f
        .backend
        .save_https_settings(f.admin, request(100, &base))
        .is_err());
    let issued = f.grant("admin", "Synthetic HTTPS configuration tester");
    let web = service.authenticate(&issued.cookie).unwrap().lease();
    assert!(f.backend.read_https_settings(web).is_err());
    assert!(f
        .backend
        .save_https_settings(web, request(101, &base))
        .is_err());
    service.set_enabled(f.admin, false).unwrap();
    {
        let mut scope = f.backend.begin_execution_scope(Arc::new(|| false));
        assert!(scope
            .save_https_settings(f.admin, request(102, &base))
            .is_err());
    }
    assert_eq!(f.backend.read_https_settings(f.admin).unwrap(), base);
    assert!(!service.configuration_in_progress());
    f.backend.session_control().lock_local();
    assert!(f.backend.read_https_settings(f.admin).is_err());
    f.login_admin();
    assert!(f
        .backend
        .save_https_settings(f.admin, request(103, &base))
        .is_ok());
}
#[test]
fn configuration_owner_and_actual_last_writer_guard_exclude_enable_and_next_owner() {
    let f = Fixture::new();
    let service = f.service();
    let owner = service.configuration_edit(f.admin).unwrap();
    let guard = owner.guard();
    assert!(guard());
    assert_eq!(
        service.set_enabled(f.admin, true),
        Err(WebServiceError::Busy)
    );
    drop(owner);
    assert!(!guard());
    assert!(service.configuration_in_progress());
    assert_eq!(
        service.set_enabled(f.admin, true),
        Err(WebServiceError::Busy)
    );
    assert!(matches!(
        service.configuration_edit(f.admin),
        Err(WebServiceError::Busy)
    ));
    drop(guard);
    assert!(!service.configuration_in_progress());
    let next = service.configuration_edit(f.admin).unwrap();
    assert!(next.guard()());
    drop(next);
    service.set_enabled(f.admin, true).unwrap();
    assert!(matches!(
        service.configuration_edit(f.admin),
        Err(WebServiceError::Busy)
    ));
    service.set_enabled(f.admin, false).unwrap();
}
struct Held {
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
impl fc_storage::TransactionTask<Snapshot> for Held {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        _: &dyn fc_storage::TransactionDocuments,
    ) -> std::result::Result<fc_storage::TransactionDecision<Snapshot>, StorageError> {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(5)).unwrap();
        Ok(fc_storage::TransactionDecision {
            outcome: vec![],
            commit: None,
            precommit: None,
        })
    }
}
#[test]
fn queued_real_writer_save_retains_disabled_owner_until_old_local_session_cancellation_settles() {
    let mut f = Fixture::new();
    let base = f.backend.read_https_settings(f.admin).unwrap();
    let service = f.service();
    let control = f.backend.session_control();
    let before = f.backend.handle.try_revision().unwrap().wait().unwrap();
    let (entered, arrived) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let held = f
        .backend
        .handle
        .try_transaction(Held {
            entered,
            release: wait,
        })
        .unwrap();
    arrived.recv_timeout(Duration::from_secs(5)).unwrap();
    let admin = f.admin;
    std::thread::scope(|scope| {
        let job = scope.spawn(|| f.backend.save_https_settings(admin, request(100, &base)));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !service.configuration_in_progress() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        // Always release the held writer even if setup fails; never strand a
        // physical task behind a test assertion or manufacture a second owner.
        let pending = service.configuration_in_progress();
        let enable = service.set_enabled(admin, true);
        control.lock_local();
        release.send(()).unwrap();
        held.wait().unwrap();
        let result = job.join().unwrap();
        assert!(pending);
        assert_eq!(enable, Err(WebServiceError::Busy));
        assert!(result.is_err());
    });
    assert!(!service.configuration_in_progress());
    assert_eq!(
        f.backend.handle.try_revision().unwrap().wait().unwrap(),
        before
    );
    f.login_admin();
    assert_eq!(f.backend.read_https_settings(f.admin).unwrap(), base);
    assert!(f
        .backend
        .save_https_settings(f.admin, request(101, &base))
        .is_ok());
}
struct LaterClock;
impl Clock for LaterClock {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 36_002).unwrap()
    }
}
impl MonotonicClock for LaterClock {
    fn monotonic_millis(&self) -> u64 {
        2000
    }
}
#[test]
fn same_committed_command_on_real_new_local_lease_returns_original_outcome_without_expiry_in_fingerprint(
) {
    let mut f = Fixture::new();
    let original_lease = f.admin;
    let base = f.backend.read_https_settings(f.admin).unwrap();
    let original = request(100, &base);
    let saved = f
        .backend
        .save_https_settings(f.admin, original.clone())
        .unwrap();
    f.backend.close().unwrap();
    f.backend = Backend::open(
        f.directory.database(),
        CredentialRecordCodec::from_key([39; 32]).unwrap(),
        Arc::new(LaterClock),
        Arc::new(LaterClock),
    )
    .unwrap();
    f.login_admin();
    assert_ne!(f.admin, original_lease);
    assert!(f
        .backend
        .save_https_settings(original_lease, original.clone())
        .is_err());
    assert_eq!(
        f.backend
            .save_https_settings(f.admin, original.clone())
            .unwrap(),
        saved
    );
    let mut different = original;
    different.configuration.as_mut().unwrap().port = 9443;
    assert!(matches!(
        f.backend.save_https_settings(f.admin, different),
        Err(BackendError::HttpsSettings(
            fc_ports::HttpsSettingsRepositoryError::IdempotencyConflict
        ))
    ));
    assert!(!f.service().management_snapshot(f.admin).unwrap().enabled);
}
#[test]
fn preexisting_enable_snapshot_cannot_survive_configuration_publication() {
    let mut f = Fixture::new();
    let service = f.service();
    let old = service.snapshot().unwrap();
    let generation = f.backend.session_control().generation();
    let base = f.backend.read_https_settings(f.admin).unwrap();
    f.backend
        .save_https_settings(f.admin, request(100, &base))
        .unwrap();
    let mut ledger = service.inner.ledger.lock().unwrap();
    assert_eq!(
        service.admin_current(&old, &mut ledger, f.admin, generation),
        Err(WebServiceError::Busy)
    );
    drop(ledger);
    assert!(!service.management_snapshot(f.admin).unwrap().enabled);
}
