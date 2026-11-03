//! Real credentials/SQLite and shared two-worker composition. The injected
//! query is explicitly a model; no CryptoAPI/TLS/device evidence is claimed.
use super::*;
use fc_ports::{command_window::CommandFamily, HttpsSettingsRequest};
use fc_runtime::worker::{JobContext, Rejection, Task, WorkerPool};
use std::sync::{atomic::AtomicUsize, mpsc};

struct Time;
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 36000).unwrap()
    }
}
impl MonotonicClock for Time {
    fn monotonic_millis(&self) -> u64 {
        1000
    }
}
struct Fixture {
    backend: Backend,
    directory: std::path::PathBuf,
    admin: SessionLease,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "FutureClassroom-Https-Query-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let mut backend = Backend::open(
            directory.join("state.sqlite3"),
            CredentialRecordCodec::from_key([49; 32]).unwrap(),
            Arc::new(Time),
            Arc::new(Time),
        )
        .unwrap();
        drop(
            backend
                .initialize(
                    Initialize {
                        school_name: "Synthetic certificate query".into(),
                        login_name: "admin".into(),
                        display_name: "Synthetic admin".into(),
                    },
                    password(),
                )
                .unwrap(),
        );
        let admin = backend.login("admin", password()).unwrap();
        Self {
            backend,
            directory,
            admin,
        }
    }
    fn configured(&mut self) -> HttpsSettingsState {
        let old = self.backend.read_https_settings(self.admin).unwrap();
        let issued = self
            .backend
            .issue_native_command(
                CommandFamily::HttpsSettings,
                Some(self.admin),
                SessionPurpose::Manage,
            )
            .unwrap();
        self.backend
            .begin_command_scope(issued)
            .unwrap()
            .save_https_settings(
                self.admin,
                HttpsSettingsRequest {
                    command_id: issued.command_id,
                    expected_revision: old.settings.revision,
                    expected_identity_revision: old.identity_revision,
                    configuration: Some(
                        HttpsConfiguration::new(
                            HttpsHost::parse("board.school").unwrap(),
                            8443,
                            HttpsCertificateRef::parse(&"ab".repeat(20)).unwrap(),
                        )
                        .unwrap(),
                    ),
                },
            )
            .unwrap()
    }
    fn service(&self) -> HttpsCertificateService {
        self.backend
            .https_certificate_service()
            .unwrap()
            .with_execution_fence(Arc::new(|| true))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.backend
            .close()
            .expect("certificate fixture actual writer join");
        std::fs::remove_dir_all(&self.directory).expect("certificate fixture cleanup");
    }
}
fn password() -> Password {
    Password::new("Synthetic-certificate-password-2026".into()).unwrap()
}
fn model(
    _: &CertificateQueryRequest,
    active: &dyn Fn() -> bool,
) -> std::result::Result<CertificateQueryReport, HttpsCertificateError> {
    assert!(active());
    Ok(CertificateQueryReport::default())
}

fn provision_model(reused: bool) -> ProvisionedCertificate {
    ProvisionedCertificate {
        thumbprint: "ab".repeat(20),
        public_der: vec![0x30, 3, 1, 2, 3],
        reused,
    }
}

#[test]
fn ip_provision_exports_only_public_bytes_without_saving_settings_and_preserves_conflicting_file() {
    let f = Fixture::new();
    let before = f.backend.read_https_settings(f.admin).unwrap();
    let service = f.service();
    let first = service
        .provision_with(f.admin, "192.168.10.212".into(), |host, active| {
            assert_eq!(host, "192.168.10.212");
            assert!(active());
            assert!(
                service.ledger.try_lock().is_ok(),
                "ledger released for native IO"
            );
            Ok(provision_model(false))
        })
        .unwrap();
    assert_eq!(
        first.export_path.parent(),
        Some(f.directory.join("exports").as_path())
    );
    assert_eq!(
        std::fs::read(&first.export_path).unwrap(),
        provision_model(false).public_der
    );
    let again = service
        .provision_with(f.admin, first.host.clone(), |_, _| {
            Ok(provision_model(true))
        })
        .unwrap();
    assert!(again.reused);
    assert_eq!(again.export_path, first.export_path);
    assert_eq!(f.backend.read_https_settings(f.admin).unwrap(), before);
    std::fs::write(&first.export_path, b"leave this file unchanged").unwrap();
    assert_eq!(
        service.provision_with(f.admin, first.host, |_, _| Ok(provision_model(true))),
        Err(HttpsCertificateError::ExportFailed)
    );
    assert_eq!(
        std::fs::read(&first.export_path).unwrap(),
        b"leave this file unchanged"
    );
}

#[test]
fn ip_provision_requires_original_fence_local_admin_and_ip_without_calling_native() {
    let f = Fixture::new();
    let service = f.service();
    assert_eq!(
        service.provision_with(f.admin, "school.example".into(), |_, _| panic!(
            "DNS is not an IP"
        )),
        Err(HttpsCertificateError::InvalidIpHost)
    );
    let unfenced = f.backend.https_certificate_service().unwrap();
    assert_eq!(
        unfenced.provision_with(f.admin, "192.168.10.212".into(), |_, _| panic!(
            "missing execution fence"
        )),
        Err(HttpsCertificateError::MissingExecutionFence)
    );
    f.backend.session_control().lock_local();
    assert!(service
        .provision_with(f.admin, "192.168.10.212".into(), |_, _| panic!(
            "revoked lease"
        ))
        .is_err());
    assert!(!f.directory.join("exports").exists());
}

#[test]
fn ip_provision_reports_unknown_after_native_success_and_revocation_without_exporting_or_saving() {
    let f = Fixture::new();
    let service = f.service();
    assert_eq!(
        service.provision_with(f.admin, "192.168.10.212".into(), |_, active| {
            assert!(active());
            f.backend.session_control().lock_local();
            assert!(!active());
            Ok(provision_model(false))
        }),
        Err(HttpsCertificateError::ProvisioningOutcomeUnknown)
    );
    assert!(!f.directory.join("exports").exists());
}

#[test]
fn https_certificate_held_ledger_is_recoverable_uncertainty_and_explicit_retry_works() {
    let mut f = Fixture::new();
    let expected = f.configured();
    let service = f.service();
    let before = f.backend.session_control().health_gate().status();
    let ledger = service.ledger.lock().unwrap();
    let result = service.inspect_with(f.admin, expected.clone(), |_, _| {
        panic!("contended ledger must not enter native query")
    });
    assert_eq!(
        result,
        Err(HttpsCertificateError::Backend(BackendError::Storage(
            StorageError::AuthorityUnavailable
        )))
    );
    assert_eq!(before.health, Health::Healthy);
    assert_eq!(f.backend.session_control().health_gate().status(), before);
    drop(ledger);
    let observation = service
        .inspect_with(f.admin, expected.clone(), model)
        .unwrap();
    assert!(observation.is_current_for(&expected, f.admin, &f.backend.session_control()));
}

#[test]
fn https_certificate_requires_configured_exact_state_and_original_nonrelaxable_fence() {
    let mut f = Fixture::new();
    let empty = f.backend.read_https_settings(f.admin).unwrap();
    assert_eq!(
        f.service()
            .inspect_with(f.admin, empty.clone(), |_, _| panic!(
                "unconfigured must not query"
            )),
        Err(HttpsCertificateError::NotConfigured)
    );
    let saved = f.configured();
    // A real paired Web GlobalAdmin remains a Web lease, never the exact local
    // management session required by this inspection.
    let web = f.backend.web_service();
    web.set_enabled(f.admin, true).unwrap();
    let window = web.begin_pairing(f.admin).unwrap();
    let pairing = web
        .request_pairing(window.code, "admin", "Synthetic certificate observer")
        .unwrap();
    let approval = web
        .pending(f.admin)
        .unwrap()
        .into_iter()
        .find(|item| item.request_id == pairing.request_id)
        .unwrap();
    web.approve(f.admin, pairing.request_id, approval.identity_revision)
        .unwrap();
    let issued = match web.poll(pairing.request_id, &pairing.poll_secret).unwrap() {
        web::PairingPoll::Granted(issued) => issued,
        web::PairingPoll::Pending => panic!("explicit approval must grant"),
    };
    let web_lease = web.authenticate(&issued.cookie).unwrap().lease();
    assert!(f
        .service()
        .inspect_with(web_lease, saved.clone(), |_, _| panic!(
            "Web lease must not query"
        ))
        .is_err());
    web.set_enabled(f.admin, false).unwrap();
    assert_eq!(
        f.service()
            .inspect_with(f.admin, empty, |_, _| panic!("stale must not query")),
        Err(HttpsCertificateError::ConfigurationChanged)
    );
    let unbound = f.backend.https_certificate_service().unwrap();
    assert_eq!(
        unbound.inspect_with(f.admin, saved.clone(), |_, _| panic!(
            "unbound must not query"
        )),
        Err(HttpsCertificateError::MissingExecutionFence)
    );
    let cancelled = f
        .service()
        .with_execution_fence(Arc::new(|| false))
        .with_execution_fence(Arc::new(|| true));
    assert!(cancelled
        .inspect_with(f.admin, saved.clone(), |_, _| panic!(
            "cancelled must not query"
        ))
        .is_err());
    let scoped;
    {
        let scope = f.backend.begin_execution_scope(Arc::new(|| false));
        assert!(scope.https_certificate_service().is_err());
    }
    {
        let scope = f.backend.begin_execution_scope(Arc::new(|| true));
        scoped = scope.https_certificate_service().unwrap();
    }
    let result = scoped.inspect_with(f.admin, saved.clone(), model).unwrap();
    assert!(result.is_current_for(&saved, f.admin, &f.backend.session_control()));
    assert_eq!(result.identity_revision, saved.identity_revision);
    assert_eq!(
        result.publication_revision,
        f.backend.global_revision().unwrap()
    );
    assert_eq!(
        result.configuration,
        saved.settings.configuration.clone().unwrap()
    );
    f.backend.session_control().lock_local();
    assert!(!result.is_current_for(&saved, f.admin, &f.backend.session_control()));
    assert!(scoped
        .inspect_with(f.admin, saved, |_, _| panic!("old lease must not query"))
        .is_err());
}

#[test]
fn https_certificate_discards_query_after_configuration_publication_or_local_lock_and_keeps_native_error(
) {
    let mut f = Fixture::new();
    let saved = f.configured();
    let service = f.service();
    let admin = f.admin;
    assert!(service
        .inspect_with(admin, saved.clone(), |_, active| {
            assert!(active());
            f.configured(); // even explicit same-value save advances settings/pub
            assert!(!active());
            Ok(CertificateQueryReport::default())
        })
        .is_err());
    let current = f.backend.read_https_settings(admin).unwrap();
    for error in [
        CertificateQueryError::Busy,
        CertificateQueryError::Faulted,
        CertificateQueryError::Cleanup(-55),
    ] {
        assert_eq!(
            f.service().inspect_with(admin, current.clone(), |_, _| Err(
                HttpsCertificateError::Native(error)
            )),
            Err(HttpsCertificateError::Native(error))
        );
    }
    let control = f.backend.session_control();
    assert!(f
        .service()
        .inspect_with(admin, current, |_, active| {
            assert!(active());
            control.lock_local();
            assert!(!active());
            Ok(CertificateQueryReport::default())
        })
        .is_err());
}

struct QueryWork {
    fixture: Arc<Mutex<Fixture>>,
    expected: HttpsSettingsState,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    discarded: Arc<AtomicUsize>,
}
impl Task for QueryWork {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(
            self.expected
                .settings
                .configuration
                .as_ref()
                .map_or(0, HttpsConfiguration::allocation_bytes),
        )
    }
    fn run(self, context: &JobContext) {
        let (service, admin) = {
            let f = self.fixture.lock().unwrap();
            (
                f.backend
                    .https_certificate_service()
                    .unwrap()
                    .with_execution_fence(Arc::new(context.execution_fence())),
                f.admin,
            )
        };
        let result = service.inspect_with(admin, self.expected, |_, active| {
            assert!(
                self.fixture.try_lock().is_ok(),
                "actual Backend lock released before query"
            );
            assert!(
                service.ledger.try_lock().is_ok(),
                "ledger lock released before query"
            );
            assert!(active());
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(!active());
            Ok(CertificateQueryReport::default())
        });
        if result.is_err() {
            self.discarded.fetch_add(1, Ordering::SeqCst);
        }
    }
}
#[test]
fn https_certificate_actual_shared_worker_cancel_retains_job_until_query_returns_without_backend_lock(
) {
    let fixture = Arc::new(Mutex::new(Fixture::new()));
    let expected = fixture.lock().unwrap().configured();
    let pool = WorkerPool::start().unwrap();
    let (entered, seen) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let discarded = Arc::new(AtomicUsize::new(0));
    let mut task = QueryWork {
        fixture: fixture.clone(),
        expected,
        entered,
        release: held,
        discarded: discarded.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let ticket = loop {
        match pool.handle().try_submit(task, deadline) {
            Ok(ticket) => break ticket,
            Err(error) if error.kind == Rejection::Busy && Instant::now() < deadline => {
                task = error.task;
                std::thread::yield_now();
            }
            Err(error) => panic!("certificate worker admission: {:?}", error.kind),
        }
    };
    seen.recv_timeout(Duration::from_secs(5)).unwrap();
    ticket.cancel();
    assert!(!ticket.is_finished());
    assert!(fixture.try_lock().is_ok());
    release.send(()).unwrap();
    assert!(ticket.wait().is_err());
    pool.request_stop();
    pool.join().unwrap();
    assert_eq!(discarded.load(Ordering::SeqCst), 1);
    drop(fixture); // original writer closes and DB files clean only after join
}
