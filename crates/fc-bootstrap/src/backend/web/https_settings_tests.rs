//! Uses the existing real-credential Web/SQLite fixture without reducing KDF
//! strength, introducing fake accounts, or creating a second execution pool.
use super::*;
use fc_domain::https_settings::*;
use fc_ports::{
    command_window::{CommandFamily, IssuedCommand},
    HttpsSettingsRequest, HttpsSettingsState,
};
fn configuration() -> HttpsConfiguration {
    HttpsConfiguration::new(
        HttpsHost::parse("Board.School").unwrap(),
        8443,
        HttpsCertificateRef::parse(&"ab".repeat(20)).unwrap(),
    )
    .unwrap()
}
fn request(issued: IssuedCommand, base: &HttpsSettingsState) -> HttpsSettingsRequest {
    HttpsSettingsRequest {
        command_id: issued.command_id,
        expected_revision: base.settings.revision,
        expected_identity_revision: base.identity_revision,
        configuration: Some(configuration()),
    }
}
fn issue_https(backend: &Backend, lease: SessionLease) -> IssuedCommand {
    backend
        .issue_native_command(
            CommandFamily::HttpsSettings,
            Some(lease),
            SessionPurpose::Manage,
        )
        .unwrap()
}
fn save_with_ticket(
    backend: &mut Backend,
    issued: IssuedCommand,
    lease: SessionLease,
    request: HttpsSettingsRequest,
) -> Result<HttpsSettingsState> {
    backend
        .begin_command_scope(issued)?
        .save_https_settings(lease, request)
}
fn query_with_ticket(
    backend: &mut Backend,
    issued: IssuedCommand,
    lease: SessionLease,
    request: HttpsSettingsRequest,
) -> Result<Option<HttpsSettingsState>> {
    backend
        .begin_command_scope(issued)?
        .query_https_settings_outcome(lease, request)
}
#[test]
fn local_configuration_only_save_replay_and_physical_reopen_leave_web_disabled() {
    let mut f = Fixture::new();
    let base = f.backend.read_https_settings(f.admin).unwrap();
    assert!(base.settings.configuration.is_none());
    let issued = issue_https(&f.backend, f.admin);
    let original = request(issued, &base);
    let saved = save_with_ticket(&mut f.backend, issued, f.admin, original.clone()).unwrap();
    assert_eq!(
        save_with_ticket(&mut f.backend, issued, f.admin, original.clone()).unwrap(),
        saved
    );
    assert!(!f.service().management_snapshot(f.admin).unwrap().enabled);
    assert!(!f.service().configuration_in_progress());
    f.reopen();
    assert_eq!(f.backend.read_https_settings(f.admin).unwrap(), saved);
    assert_eq!(
        query_with_ticket(&mut f.backend, issued, f.admin, original).unwrap(),
        Some(saved)
    );
    assert!(!f.service().management_snapshot(f.admin).unwrap().enabled);
}
#[test]
fn real_web_global_session_active_web_and_canceled_scope_cannot_save_https_configuration() {
    let mut f = Fixture::new();
    let base = f.backend.read_https_settings(f.admin).unwrap();
    let local = issue_https(&f.backend, f.admin);
    let canceled = issue_https(&f.backend, f.admin);
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    assert!(save_with_ticket(&mut f.backend, local, f.admin, request(local, &base)).is_err());
    let issued = f.grant("admin", "Synthetic HTTPS configuration tester");
    let web = service.authenticate(&issued.cookie).unwrap().lease();
    assert!(f.backend.read_https_settings(web).is_err());
    assert!(save_with_ticket(&mut f.backend, local, web, request(local, &base)).is_err());
    service.set_enabled(f.admin, false).unwrap();
    {
        let mut scope = f.backend.begin_execution_scope(Arc::new(|| false));
        assert!(save_with_ticket(&mut scope, canceled, f.admin, request(canceled, &base)).is_err());
    }
    assert_eq!(f.backend.read_https_settings(f.admin).unwrap(), base);
    assert!(!service.configuration_in_progress());
    f.backend.session_control().lock_local();
    assert!(f.backend.read_https_settings(f.admin).is_err());
    f.login_admin();
    let after_login = issue_https(&f.backend, f.admin);
    assert!(save_with_ticket(
        &mut f.backend,
        after_login,
        f.admin,
        request(after_login, &base)
    )
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
    let issued = issue_https(&f.backend, f.admin);
    let original = request(issued, &base);
    let before = f.backend.handle.try_revision().unwrap().wait().unwrap();
    let mut command = f.backend.begin_command_scope(issued).unwrap();
    let (entered, arrived) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let held = command
        .handle
        .try_transaction(Held {
            entered,
            release: wait,
        })
        .unwrap();
    arrived.recv_timeout(Duration::from_secs(5)).unwrap();
    let admin = f.admin;
    std::thread::scope(|scope| {
        let job = scope.spawn(|| command.save_https_settings(admin, original));
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
    drop(command);
    assert!(!service.configuration_in_progress());
    assert_eq!(
        f.backend.handle.try_revision().unwrap().wait().unwrap(),
        before
    );
    f.login_admin();
    let next = issue_https(&f.backend, f.admin);
    assert_eq!(f.backend.read_https_settings(f.admin).unwrap(), base);
    assert!(save_with_ticket(&mut f.backend, next, f.admin, request(next, &base)).is_ok());
}
struct LaterClock {
    later: AtomicBool,
}
impl Clock for LaterClock {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(
            LocalDate::new(2026, 10, 30).unwrap(),
            if self.later.load(Ordering::Acquire) {
                36_002
            } else {
                36_000
            },
        )
        .unwrap()
    }
}
impl MonotonicClock for LaterClock {
    fn monotonic_millis(&self) -> u64 {
        if self.later.load(Ordering::Acquire) {
            2000
        } else {
            1000
        }
    }
}
#[test]
fn same_committed_command_on_real_new_local_lease_returns_original_outcome_without_expiry_in_fingerprint(
) {
    let mut f = Fixture::new();
    f.backend.close().unwrap();
    let clock = Arc::new(LaterClock {
        later: AtomicBool::new(false),
    });
    f.backend = Backend::open(
        f.directory.database(),
        CredentialRecordCodec::from_key([39; 32]).unwrap(),
        clock.clone(),
        clock.clone(),
    )
    .unwrap();
    f.login_admin();
    let original_lease = f.admin;
    let base = f.backend.read_https_settings(f.admin).unwrap();
    let issued = issue_https(&f.backend, f.admin);
    let original = request(issued, &base);
    let saved = save_with_ticket(&mut f.backend, issued, f.admin, original.clone()).unwrap();
    f.backend.session_control().lock_local();
    clock.later.store(true, Ordering::Release);
    f.login_admin();
    assert_ne!(f.admin, original_lease);
    assert!(save_with_ticket(&mut f.backend, issued, original_lease, original.clone()).is_err());
    assert_eq!(
        save_with_ticket(&mut f.backend, issued, f.admin, original.clone()).unwrap(),
        saved
    );
    // Pure query works while admission is enabled, never takes configuration
    // ownership, and never interprets a missing command as permission to save.
    let service = f.service();
    service.set_enabled(f.admin, true).unwrap();
    assert_eq!(
        query_with_ticket(&mut f.backend, issued, f.admin, original.clone()).unwrap(),
        Some(saved.clone())
    );
    assert!(query_with_ticket(&mut f.backend, issued, original_lease, original.clone()).is_err());
    let missing_issued = issue_https(&f.backend, f.admin);
    let missing = request(
        missing_issued,
        &f.backend.read_https_settings(f.admin).unwrap(),
    );
    let before = f.backend.handle.try_revision().unwrap().wait().unwrap();
    assert_eq!(
        query_with_ticket(&mut f.backend, missing_issued, f.admin, missing).unwrap(),
        None
    );
    assert_eq!(
        f.backend.handle.try_revision().unwrap().wait().unwrap(),
        before
    );
    assert!(!service.configuration_in_progress());
    service.set_enabled(f.admin, false).unwrap();
    let mut different = original;
    different.configuration.as_mut().unwrap().port = 9443;
    assert!(matches!(
        save_with_ticket(&mut f.backend, issued, f.admin, different),
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
    let issued = issue_https(&f.backend, f.admin);
    save_with_ticket(&mut f.backend, issued, f.admin, request(issued, &base)).unwrap();
    let mut ledger = service.inner.ledger.lock().unwrap();
    assert_eq!(
        service.admin_current(&old, &mut ledger, f.admin, generation),
        Err(WebServiceError::Busy)
    );
    drop(ledger);
    assert!(!service.management_snapshot(f.admin).unwrap().enabled);
}
