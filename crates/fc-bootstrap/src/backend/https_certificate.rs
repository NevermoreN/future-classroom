//! Clone-only trusted composition. Call outside the Backend mutex, on the
//! original shared worker. No KDF/SQL/ledger lock spans the provider query.
use super::*;
use fc_domain::https_settings::{HttpsCertificateRef, HttpsConfiguration, HttpsHost};
use fc_ports::{HttpsSettingsRepository, HttpsSettingsState};
use fc_runtime::{
    authority::AuthoritySnapshot,
    health::{Health, HealthStatus},
};
use fc_windows::tls_frontdoor::certificate_provision::{
    CertificateProvisionError, ProvisionedCertificate,
};
use fc_windows::tls_frontdoor::certificate_query::{
    CertificateQueryError, CertificateQueryReport, CertificateQueryRequest,
};
use std::{
    io::{Read, Write},
    net::IpAddr,
    path::PathBuf,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpsCertificateError {
    Backend(BackendError),
    NotConfigured,
    ConfigurationChanged,
    MissingExecutionFence,
    Native(CertificateQueryError),
    Provision(CertificateProvisionError),
    InvalidIpHost,
    ExportFailed,
    /// Native creation may already have completed. Nothing is saved or enabled;
    /// a later explicit retry can rediscover the certificate in the local store.
    ProvisioningOutcomeUnknown,
    UnsupportedPlatform,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvisionedHttpsCertificate {
    pub publication_revision: Revision,
    pub host: String,
    pub thumbprint: String,
    pub export_path: PathBuf,
    pub reused: bool,
}
impl From<BackendError> for HttpsCertificateError {
    fn from(error: BackendError) -> Self {
        Self::Backend(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsCertificateObservation {
    pub publication_revision: Revision,
    pub identity_revision: Revision,
    pub settings_revision: Revision,
    pub configuration: HttpsConfiguration,
    pub observed_at: LocalDateTime,
    pub observed_monotonic_millis: u64,
    pub report: CertificateQueryReport,
    lease: SessionLease,
    generation: u64,
}
impl HttpsCertificateObservation {
    /// Historical facts only. Does not renew a check or authorize listening.
    /// The UI still uses its usual exact local-session delivery gate.
    pub fn is_current_for(
        &self,
        state: &HttpsSettingsState,
        lease: SessionLease,
        control: &SessionControl,
    ) -> bool {
        let health = control.health_gate().status();
        lease == self.lease
            && control.check(lease, self.generation).is_ok()
            && health.health == Health::Healthy
            && health.revision == self.publication_revision
            && state.identity_revision == self.identity_revision
            && state.settings.revision == self.settings_revision
            && state.settings.configuration.as_ref() == Some(&self.configuration)
            && control.check(lease, self.generation).is_ok()
            && control.health_gate().status() == health
    }
}

#[derive(Clone)]
pub struct HttpsCertificateService {
    repository: Repository,
    ledger: Arc<Mutex<Ledger>>,
    control: SessionControl,
    authority: AuthorityReader<IdentityState>,
    clock: Arc<dyn Clock + Send + Sync>,
    monotonic: Arc<dyn MonotonicClock + Send + Sync>,
    execution: Option<TransactionExecutionFence>,
    exports_root: Option<PathBuf>,
}
struct Review {
    snapshot: Arc<AuthoritySnapshot<IdentityState>>,
    health: HealthStatus,
    generation: u64,
    lease: SessionLease,
    actor: Option<ActorContext>,
}
impl Backend {
    /// Short memory-only factory. Clone this under the Backend mutex, then
    /// RELEASE that mutex before invoking the service on the same worker.
    pub fn https_certificate_service(&self) -> Result<HttpsCertificateService> {
        self.check_open()?;
        Ok(HttpsCertificateService {
            repository: self.repository.clone(),
            ledger: self.ledger.clone(),
            control: self.control.clone(),
            authority: self.authority.clone(),
            clock: self.clock.clone(),
            monotonic: self.monotonic.clone(),
            execution: self.request_execution_fence.clone(),
            exports_root: self.database_path.parent().and_then(|parent| {
                if !self.database_path.is_absolute() {
                    return None;
                }
                let production = self.database_path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .eq_ignore_ascii_case("classroom.sqlite3")
                }) && parent
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("state"));
                (if production {
                    parent.parent()
                } else {
                    Some(parent)
                })
                .map(|root| root.join("exports"))
            }),
        })
    }
}
impl HttpsCertificateService {
    /// Explicit local-management action, on the shared worker outside Backend.
    /// Generates/reuses only a local IP certificate and exports its public DER.
    /// Does not persist HTTPS configuration or start a listener.
    pub fn provision_ip_certificate(
        &self,
        lease: SessionLease,
        host: String,
    ) -> std::result::Result<ProvisionedHttpsCertificate, HttpsCertificateError> {
        self.provision_with(lease, host, |host, active| {
            fc_windows::tls_frontdoor::certificate_provision::provision_ip_certificate(host, active)
                .map_err(HttpsCertificateError::Provision)
        })
    }

    fn provision_with(
        &self,
        lease: SessionLease,
        host: String,
        provision: impl FnOnce(
            &str,
            &dyn Fn() -> bool,
        )
            -> std::result::Result<ProvisionedCertificate, HttpsCertificateError>,
    ) -> std::result::Result<ProvisionedHttpsCertificate, HttpsCertificateError> {
        if self.execution.is_none() {
            return Err(HttpsCertificateError::MissingExecutionFence);
        }
        let snapshot = self
            .authority
            .snapshot()
            .map_err(|_| BackendError::Storage(StorageError::AuthorityUnavailable))?;
        let mut review = Review {
            snapshot,
            health: self.control.health_gate().status(),
            generation: self.control.generation(),
            lease,
            actor: None,
        };
        review.actor = Some(self.memory_actor(&review)?);
        let parsed = HttpsHost::parse(&host).map_err(|_| HttpsCertificateError::InvalidIpHost)?;
        let ip: IpAddr = parsed
            .as_str()
            .parse()
            .map_err(|_| HttpsCertificateError::InvalidIpHost)?;
        let host = ip.to_string();
        let exports = self
            .exports_root
            .as_ref()
            .ok_or(HttpsCertificateError::ExportFailed)?;
        self.current(&review)?;
        let certificate = provision(&host, &|| self.current(&review).is_ok())?;
        self.current(&review)
            .map_err(|_| HttpsCertificateError::ProvisioningOutcomeUnknown)?;
        let thumbprint = HttpsCertificateRef::parse(&certificate.thumbprint)
            .map_err(|_| HttpsCertificateError::ProvisioningOutcomeUnknown)?
            .to_hex();
        if certificate.public_der.is_empty() || certificate.public_der.len() > 64 * 1024 {
            return Err(HttpsCertificateError::ProvisioningOutcomeUnknown);
        }
        std::fs::create_dir_all(exports).map_err(|_| HttpsCertificateError::ExportFailed)?;
        self.current(&review)
            .map_err(|_| HttpsCertificateError::ProvisioningOutcomeUnknown)?;
        let export_path = exports.join(format!(
            "FutureClassroom-HTTPS-{}-{thumbprint}.cer",
            host.replace(':', "_")
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&export_path)
        {
            Ok(mut file) => {
                let result = file
                    .write_all(&certificate.public_der)
                    .and_then(|()| file.sync_all());
                drop(file);
                if result.is_err() {
                    let _ = std::fs::remove_file(&export_path);
                    return Err(HttpsCertificateError::ExportFailed);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = std::fs::symlink_metadata(&export_path)
                    .map_err(|_| HttpsCertificateError::ExportFailed)?;
                if !metadata.file_type().is_file()
                    || metadata.len() != certificate.public_der.len() as u64
                {
                    return Err(HttpsCertificateError::ExportFailed);
                }
                let mut existing = Vec::new();
                std::fs::File::open(&export_path)
                    .and_then(|file| file.take(64 * 1024 + 1).read_to_end(&mut existing))
                    .map_err(|_| HttpsCertificateError::ExportFailed)?;
                if existing != certificate.public_der {
                    return Err(HttpsCertificateError::ExportFailed);
                }
            }
            Err(_) => return Err(HttpsCertificateError::ExportFailed),
        }
        self.current(&review)
            .map_err(|_| HttpsCertificateError::ProvisioningOutcomeUnknown)?;
        Ok(ProvisionedHttpsCertificate {
            publication_revision: review.health.revision,
            host,
            thumbprint,
            export_path,
            reused: certificate.reused,
        })
    }
    pub fn with_execution_fence(&self, fence: TransactionExecutionFence) -> Self {
        let mut next = self.clone();
        next.execution = Some(match &self.execution {
            None => fence,
            Some(previous) => {
                let previous = previous.clone();
                Arc::new(move || checked(&previous) && checked(&fence))
            }
        });
        next
    }
    fn execution_current(&self) -> bool {
        self.execution.as_ref().is_some_and(checked)
    }
    fn memory_actor(&self, review: &Review) -> Result<ActorContext> {
        if !self.execution_current() {
            return Err(BackendError::Storage(StorageError::ExecutionCancelled));
        }
        if review.lease.origin() != TrustedOrigin::Local {
            return Err(BackendError::Session(SessionError::Revoked));
        }
        let gate = self.control.health_gate();
        if gate.status() != review.health
            || review.health.health != Health::Healthy
            || review.health.revision != review.snapshot.revision
            || review.health.auth_epoch != review.snapshot.auth_epoch
        {
            return Err(BackendError::Storage(StorageError::AuthorityUnavailable));
        }
        self.control.check(review.lease, review.generation)?;
        let now = self.clock.now();
        let tick = self.monotonic.monotonic_millis();
        let actor = {
            // Contention means uncertainty. This callback never waits behind a
            // Backend/KDF owner and never holds metadata across a native call.
            let mut ledger = self
                .ledger
                .try_lock()
                .map_err(|_| BackendError::Storage(StorageError::AuthorityUnavailable))?;
            ledger.synchronize(&self.control, &review.snapshot.data, now, tick);
            ledger.sessions.actor(
                &review.snapshot.data,
                review.lease,
                now,
                tick,
                SessionPurpose::Manage,
            )?
        };
        review
            .snapshot
            .data
            .authority(actor.principal_id)
            .map_err(|e| BackendError::Identity(IdentityRepositoryError::Domain(e)))?
            .authorize_global_local(&actor, now)
            .map_err(|e| {
                BackendError::Identity(IdentityRepositoryError::Domain(IdentityError::Permission(
                    e,
                )))
            })?;
        review
            .snapshot
            .data
            .authorize_management(&actor, now)
            .map_err(|e| BackendError::Identity(IdentityRepositoryError::Domain(e)))?;
        self.control.check(review.lease, review.generation)?;
        if gate.status() != review.health || !self.execution_current() {
            return Err(BackendError::Storage(StorageError::AuthorityUnavailable));
        }
        Ok(actor)
    }
    fn current(&self, review: &Review) -> Result<()> {
        if review.actor.as_ref() == Some(&self.memory_actor(review)?) {
            Ok(())
        } else {
            Err(BackendError::Session(SessionError::Revoked))
        }
    }
    fn repository(&self, review: Arc<Review>) -> Repository {
        let service = self.clone();
        let session = Arc::new(move |state: &IdentityState, expected: &ActorContext| {
            if state.revision != review.snapshot.data.revision
                || review.actor.as_ref() != Some(expected)
            {
                return Err(StorageError::AuthorityUnavailable);
            }
            service
                .current(&review)
                .map_err(|_| StorageError::AuthorityUnavailable)
        });
        let mut repository = self.repository.clone().with_session_guard(session);
        if let Some(fence) = &self.execution {
            repository = repository.with_execution_fence(fence.clone());
        }
        repository
    }

    pub fn inspect_certificate(
        &self,
        lease: SessionLease,
        expected: HttpsSettingsState,
    ) -> std::result::Result<HttpsCertificateObservation, HttpsCertificateError> {
        self.inspect_with(lease, expected, |request, active| {
            #[cfg(windows)]
            {
                fc_windows::tls_frontdoor::certificate_query::inspect_certificate(request, active)
                    .map_err(HttpsCertificateError::Native)
            }
            #[cfg(not(windows))]
            {
                let _ = (request, active);
                Err(HttpsCertificateError::UnsupportedPlatform)
            }
        })
    }
    fn inspect_with(
        &self,
        lease: SessionLease,
        expected: HttpsSettingsState,
        query: impl FnOnce(
            &CertificateQueryRequest,
            &dyn Fn() -> bool,
        )
            -> std::result::Result<CertificateQueryReport, HttpsCertificateError>,
    ) -> std::result::Result<HttpsCertificateObservation, HttpsCertificateError> {
        if self.execution.is_none() {
            return Err(HttpsCertificateError::MissingExecutionFence);
        }
        if !self.execution_current() {
            return Err(BackendError::Storage(StorageError::ExecutionCancelled).into());
        }
        let snapshot = self
            .authority
            .snapshot()
            .map_err(|_| BackendError::Storage(StorageError::AuthorityUnavailable))?;
        if snapshot.data.revision != expected.identity_revision {
            return Err(HttpsCertificateError::ConfigurationChanged);
        }
        let health = self.control.health_gate().status();
        // Actor is established from the original exact lease, never form data.
        let mut review = Review {
            snapshot,
            health,
            generation: self.control.generation(),
            lease,
            actor: None,
        };
        review.actor = Some(self.memory_actor(&review)?);
        let review = Arc::new(review);
        let repository = self.repository(review.clone());
        let actor = review.actor.as_ref().ok_or(BackendError::Closed)?;
        let persisted = repository
            .https_settings(actor)
            .map_err(BackendError::HttpsSettings)?;
        if persisted != expected {
            return Err(HttpsCertificateError::ConfigurationChanged);
        }
        let configuration = persisted
            .settings
            .configuration
            .clone()
            .ok_or(HttpsCertificateError::NotConfigured)?;
        self.current(&review)?;
        let request = CertificateQueryRequest::new(
            &configuration.certificate.to_hex(),
            configuration.host.as_str(),
        )
        .map_err(HttpsCertificateError::Native)?;
        let report = query(&request, &|| self.current(&review).is_ok())?;
        let observed_at = self.clock.now();
        let observed_monotonic_millis = self.monotonic.monotonic_millis();
        // The query returned only after native ownership was actually settled
        // (or its precise fatal failure). No old positive survives either read.
        let after = repository
            .https_settings(actor)
            .map_err(BackendError::HttpsSettings)?;
        if after != expected {
            return Err(HttpsCertificateError::ConfigurationChanged);
        }
        self.current(&review)?;
        let observation = HttpsCertificateObservation {
            publication_revision: review.snapshot.revision,
            identity_revision: expected.identity_revision,
            settings_revision: expected.settings.revision,
            configuration,
            observed_at,
            observed_monotonic_millis,
            report,
            lease,
            generation: review.generation,
        };
        self.current(&review)?;
        Ok(observation)
    }
}
fn checked(fence: &TransactionExecutionFence) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fence())).unwrap_or(false)
}

#[cfg(test)]
mod tests;
