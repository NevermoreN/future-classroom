//! Explicit historical inspection, not an HTTPS listener/start permission.
//! No native handle, certificate bytes or private-key metadata leaves this API.
use super::CertificateThumbprint;

/// A fact may be negative or unknown independently of all other facts. Even a
/// set of Satisfied facts is NOT a production TLS readiness/authority token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateCheck {
    Satisfied,
    Rejected,
    Unverified,
    NotChecked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CertificateFact {
    pub check: CertificateCheck,
    /// Sanitized native status only, never provider/container names or paths.
    pub native_error: Option<i32>,
}

impl CertificateFact {
    const fn new(check: CertificateCheck) -> Self {
        Self {
            check,
            native_error: None,
        }
    }
    #[cfg(any(windows, test))]
    fn unknown(error: i32) -> Self {
        Self {
            check: CertificateCheck::Unverified,
            native_error: Some(error),
        }
    }
}

/// These facts concern this Windows logon user's existing MY store and local
/// cached trust/revocation data. They do not prove another client's trust,
/// fresh online revocation, successful TLS, or the right to start listening.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertificateQueryReport {
    pub scope: CertificateQueryScope,
    pub found: CertificateFact,
    pub current_time: CertificateFact,
    /// Silent existing-key acquisition with compare/no-healing flags; NOT a
    /// proof that a future signature, handshake or provider operation succeeds.
    pub private_key: CertificateFact,
    /// Effective extension/property intersection, including documented
    /// unrestricted usage, independently of legacy credential admission.
    pub server_auth_eku: CertificateFact,
    /// DNS SSL policy only; IP/IPv6 SAN matching remains Unverified.
    pub server_name: CertificateFact,
    pub ssl_chain_policy: CertificateFact,
    /// Always cache-only, chain excluding root. Satisfied is not an online
    /// freshness claim: it means the engine reported no trust/revocation
    /// errors under those flags (the root is not checked). Missing/offline
    /// cached revocation is Unverified, never permission to ignore it.
    pub cached_revocation: CertificateFact,
    pub chain_trust_errors: Option<u32>,
    pub ssl_policy_error: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateQueryScope {
    /// Windows logon user MY; local trust and cache-only revocation excluding
    /// root. Not online freshness, client trust, TLS readiness or permission.
    CurrentUserMyCacheOnly,
}

impl Default for CertificateQueryReport {
    fn default() -> Self {
        let unchecked = CertificateFact::new(CertificateCheck::NotChecked);
        Self {
            scope: CertificateQueryScope::CurrentUserMyCacheOnly,
            found: unchecked,
            current_time: unchecked,
            private_key: unchecked,
            server_auth_eku: unchecked,
            server_name: unchecked,
            ssl_chain_policy: unchecked,
            cached_revocation: unchecked,
            chain_trust_errors: None,
            ssl_policy_error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateQueryError {
    InvalidInput,
    Busy,
    Faulted,
    Cancelled,
    Native(i32),
    /// A release API failed. Callers must not turn this into a successful
    /// reusable query flight merely because the logical work finished.
    Cleanup(i32),
}

#[derive(Clone, PartialEq, Eq)]
pub struct CertificateQueryRequest {
    thumbprint: CertificateThumbprint,
    host: String,
    dns_name: bool,
}

impl CertificateQueryRequest {
    /// Canonical ASCII DNS/A-label or std canonical IP, without brackets,
    /// scheme, port, trailing dot, whitespace, NUL or wildcard. No DNS lookup.
    pub fn new(thumbprint: &str, host: &str) -> Result<Self, CertificateQueryError> {
        let thumbprint = CertificateThumbprint::parse(thumbprint)
            .map_err(|_| CertificateQueryError::InvalidInput)?;
        if host.is_empty() || host.len() > 253 || !host.is_ascii() {
            return Err(CertificateQueryError::InvalidInput);
        }
        let dns_name = if let Ok(address) = host.parse::<std::net::IpAddr>() {
            if address.to_string() != host {
                return Err(CertificateQueryError::InvalidInput);
            }
            false
        } else {
            let lower_alphanumeric = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
            if host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                || host.split('.').any(|label| {
                    label.is_empty()
                        || label.len() > 63
                        || !lower_alphanumeric(label.as_bytes()[0])
                        || !lower_alphanumeric(label.as_bytes()[label.len() - 1])
                        || !label
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
            {
                return Err(CertificateQueryError::InvalidInput);
            }
            true
        };
        Ok(Self {
            thumbprint,
            host: host.to_owned(),
            dns_name,
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }
}

/// Run synchronously on the already-owned shared worker. The caller supplies
/// exact authority/lifetime checks; no new thread, SSPI or listener is created.
/// Cancellation is cooperative between native calls, not provider interruption.
#[cfg(windows)]
pub fn inspect_certificate(
    request: &CertificateQueryRequest,
    active: &dyn Fn() -> bool,
) -> Result<CertificateQueryReport, CertificateQueryError> {
    checkpoint(active)?;
    native::inspect(request, active)
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy)]
enum Usage {
    Allowed,
    Rejected,
    Unverified,
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy)]
struct ChainObservation {
    trust_errors: u32,
    /// Err = API BOOL false. Ok(error) = API BOOL true; error MUST be checked.
    policy: Result<u32, i32>,
}

#[cfg(any(windows, test))]
trait QueryDriver {
    fn select(&mut self, thumbprint: CertificateThumbprint) -> Result<bool, i32>;
    fn time_valid(&mut self) -> Result<bool, i32>;
    fn usage(&mut self) -> Result<Usage, i32>;
    fn private_key(&mut self) -> Result<(), i32>;
    fn chain(&mut self, host: &str) -> Result<ChainObservation, i32>;
    /// Always attempted, including cancellation/partial failure; all resources
    /// must have actually returned before a successful report is delivered.
    fn close(&mut self) -> Result<(), i32>;
}

#[cfg(any(windows, test))]
fn checkpoint(active: &dyn Fn() -> bool) -> Result<(), CertificateQueryError> {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(active)).unwrap_or(false) {
        Ok(())
    } else {
        Err(CertificateQueryError::Cancelled)
    }
}

#[cfg(any(windows, test))]
fn run_query(
    driver: &mut impl QueryDriver,
    request: &CertificateQueryRequest,
    active: &dyn Fn() -> bool,
) -> Result<CertificateQueryReport, CertificateQueryError> {
    let result = (|| {
        let mut report = CertificateQueryReport::default();
        checkpoint(active)?;
        let selected = driver.select(request.thumbprint);
        checkpoint(active)?;
        report.found = match selected {
            Ok(true) => CertificateFact::new(CertificateCheck::Satisfied),
            Ok(false) => {
                report.found = CertificateFact::new(CertificateCheck::Rejected);
                return Ok(report);
            }
            Err(error) => {
                report.found = CertificateFact::unknown(error);
                return Ok(report);
            }
        };
        report.current_time = match driver.time_valid() {
            Ok(true) => CertificateFact::new(CertificateCheck::Satisfied),
            Ok(false) => CertificateFact::new(CertificateCheck::Rejected),
            Err(error) => CertificateFact::unknown(error),
        };
        checkpoint(active)?;
        report.server_auth_eku = match driver.usage() {
            Ok(Usage::Allowed) => CertificateFact::new(CertificateCheck::Satisfied),
            Ok(Usage::Rejected) => CertificateFact::new(CertificateCheck::Rejected),
            Ok(Usage::Unverified) => CertificateFact::new(CertificateCheck::Unverified),
            Err(error) => CertificateFact::unknown(error),
        };
        checkpoint(active)?;
        report.private_key = match driver.private_key() {
            Ok(()) => CertificateFact::new(CertificateCheck::Satisfied),
            Err(error) => CertificateFact::unknown(error),
        };
        checkpoint(active)?;
        match driver.chain(&request.host) {
            Ok(observation) => apply_chain(&mut report, observation, request.dns_name),
            Err(error) => {
                report.ssl_chain_policy = CertificateFact::unknown(error);
                report.server_name = CertificateFact::unknown(error);
                report.cached_revocation = CertificateFact::unknown(error);
            }
        }
        checkpoint(active)?;
        Ok(report)
    })();
    // Cleanup errors dominate both success and cancellation. Never report a
    // successful assessment while any release API has reported failure.
    driver.close().map_err(CertificateQueryError::Cleanup)?;
    checkpoint(active)?;
    result
}

#[cfg(any(windows, test))]
fn apply_chain(report: &mut CertificateQueryReport, chain: ChainObservation, dns_name: bool) {
    const REVOKED: u32 = 0x4;
    const UNKNOWN_REVOCATION: u32 = 0x40 | 0x0100_0000;
    const CN_NO_MATCH: u32 = 0x800b_010f;
    report.chain_trust_errors = Some(chain.trust_errors);
    report.cached_revocation = CertificateFact::new(if chain.trust_errors & REVOKED != 0 {
        CertificateCheck::Rejected
    } else if chain.trust_errors != 0 {
        CertificateCheck::Unverified
    } else {
        CertificateCheck::Satisfied
    });
    match chain.policy {
        Err(error) => {
            report.ssl_chain_policy = CertificateFact::unknown(error);
            report.server_name = CertificateFact::unknown(error);
        }
        Ok(error) => {
            report.ssl_policy_error = Some(error);
            report.ssl_chain_policy = CertificateFact {
                check: if error == 0 && chain.trust_errors == 0 {
                    CertificateCheck::Satisfied
                } else if chain.trust_errors & !UNKNOWN_REVOCATION == 0
                    && matches!(error, 0 | 0x8009_2012 | 0x8009_2013 | 0x800b_010e)
                {
                    CertificateCheck::Unverified
                } else {
                    CertificateCheck::Rejected
                },
                native_error: (error != 0).then_some(error as i32),
            };
            report.server_name = CertificateFact {
                check: if !dns_name {
                    CertificateCheck::Unverified
                } else if error == CN_NO_MATCH {
                    CertificateCheck::Rejected
                } else if error == 0 && chain.trust_errors == 0 {
                    CertificateCheck::Satisfied
                } else {
                    CertificateCheck::Unverified
                },
                native_error: (dns_name && error == CN_NO_MATCH).then_some(error as i32),
            };
        }
    }
}

#[cfg(any(windows, test))]
mod flight;
#[cfg(windows)]
mod native;
#[cfg(test)]
mod tests;
