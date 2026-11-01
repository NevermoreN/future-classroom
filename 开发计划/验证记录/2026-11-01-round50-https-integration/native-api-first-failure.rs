use super::{ChainObservation, QueryDriver, Usage};
use crate::tls_frontdoor::{
    certificate_selection::SelectedCertificate,
    credentials::{server_auth_usage, ServerAuthUsage},
    CertificateThumbprint, TlsError,
};
use windows::Win32::{Foundation::E_UNEXPECTED, Security::Cryptography::*};

pub(super) struct NativeQuery {
    selected: SelectedCertificate,
    chain: *mut CERT_CHAIN_CONTEXT,
    cleanup_failed: bool,
}

struct RetainedQuery {
    _owner: std::mem::ManuallyDrop<Box<NativeQuery>>,
}

// SAFETY: this wrapper is created only AFTER every synchronous provider/chain/
// release call has actually returned. It never exposes NativeQuery, never
// invokes another native method, and its ManuallyDrop owner is never destroyed
// or reused, including at worker exit. Moving the inert exact owner into the
// one terminal process ledger is not permission to move/use NativeQuery on a
// different thread. NativeQuery itself remains !Send/!Sync via its selection.
unsafe impl Send for RetainedQuery {}

static FLIGHT: super::flight::FlightLedger<RetainedQuery> = super::flight::FlightLedger::new();

pub(super) fn inspect(
    request: &super::CertificateQueryRequest,
    active: &dyn Fn() -> bool,
) -> Result<super::CertificateQueryReport, super::CertificateQueryError> {
    let flight = FLIGHT.claim()?;
    if let Err(error) = super::checkpoint(active) {
        // No native owner exists yet.
        flight.released()?;
        return Err(error);
    }
    // Construction failure owns no native resources: store-open either returned
    // a valid handle encapsulated below, or failed without one.
    let mut owner = match NativeQuery::new() {
        Ok(owner) => Box::new(owner),
        Err(error) => {
            flight.released()?;
            return Err(error);
        }
    };
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::run_query(owner.as_mut(), request, active)
    }));
    let result = match attempt {
        Ok(result) => result,
        Err(_) => match owner.close() {
            Ok(()) => Err(super::CertificateQueryError::Native(E_UNEXPECTED.0)),
            Err(code) => Err(super::CertificateQueryError::Cleanup(code)),
        },
    };
    if owner.cleanup_failed {
        flight.retain(RetainedQuery {
            _owner: std::mem::ManuallyDrop::new(owner),
        });
        return result;
    }
    // Successful explicit close discharged every owned reference. Drop contains
    // no native debt, then and only then publish Idle.
    drop(owner);
    flight.released()?;
    result
}

impl NativeQuery {
    pub fn new() -> Result<Self, super::CertificateQueryError> {
        Ok(Self {
            selected: SelectedCertificate::open()
                .map_err(|error| super::CertificateQueryError::Native(error.code().0))?,
            chain: std::ptr::null_mut(),
            cleanup_failed: false,
        })
    }

    fn free_chain(&mut self) {
        let chain = std::mem::replace(&mut self.chain, std::ptr::null_mut());
        if !chain.is_null() {
            // SAFETY: exactly this context was returned to this sole owner.
            // The native API is void: return discharges our one reference,
            // not a claim about unrelated references in the OS chain cache.
            unsafe { CertFreeCertificateChain(chain) };
        }
    }
}

impl QueryDriver for NativeQuery {
    fn select(&mut self, thumbprint: CertificateThumbprint) -> Result<bool, i32> {
        self.selected
            .find(thumbprint)
            .map_err(|error| error.code().0)
    }

    fn time_valid(&mut self) -> Result<bool, i32> {
        let certificate = self.selected.certificate();
        if certificate.is_null() {
            return Err(E_UNEXPECTED.0);
        }
        // SAFETY: live exact selected certificate; null time uses system time.
        Ok(unsafe { CertVerifyTimeValidity(None, (*certificate).pCertInfo) } == 0)
    }

    fn usage(&mut self) -> Result<Usage, i32> {
        match server_auth_usage(self.selected.certificate()) {
            Ok(ServerAuthUsage::Explicit | ServerAuthUsage::Unrestricted) => Ok(Usage::Allowed),
            Ok(ServerAuthUsage::Rejected) => Ok(Usage::Rejected),
            Ok(ServerAuthUsage::Unverified) => Ok(Usage::Unverified),
            Err(_) => Err(E_UNEXPECTED.0),
        }
    }

    fn private_key(&mut self) -> Result<(), i32> {
        self.selected
            .acquire_private_key()
            .map_err(|error| error.code().0)
    }

    fn chain(&mut self, host: &str) -> Result<ChainObservation, i32> {
        if !self.chain.is_null() || self.selected.certificate().is_null() {
            return Err(E_UNEXPECTED.0);
        }
        // No runtime-supplied OID, allocation or unconstrained string here.
        let mut server_auth =
            windows::core::PSTR(windows::core::s!("1.3.6.1.5.5.7.3.1").0.cast_mut());
        let parameters = CERT_CHAIN_PARA {
            // Use the documented base structure (cbSize + RequestedUsage),
            // avoiding Windows 8 strong-sign extra fields on Windows 7.
            cbSize: std::mem::offset_of!(CERT_CHAIN_PARA, RequestedIssuancePolicy) as u32,
            RequestedUsage: CERT_USAGE_MATCH {
                dwType: USAGE_MATCH_TYPE_AND,
                Usage: CTL_USAGE {
                    cUsageIdentifier: 1,
                    rgpszUsageIdentifier: &mut server_auth,
                },
            },
            ..Default::default()
        };
        let flags = CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL
            | CERT_CHAIN_REVOCATION_CHECK_CACHE_ONLY
            | CERT_CHAIN_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT
            | CERT_CHAIN_DISABLE_AUTH_ROOT_AUTO_UPDATE;
        // SAFETY: current-user default engine, selected live cert/store;
        // synchronous borrowed OID/parameters, owned returned chain. Both URL
        // and revocation retrieval are cache-only; no ignore/peer-trust flags.
        let built = unsafe {
            CertGetCertificateChain(
                None,
                self.selected.certificate(),
                None,
                self.selected.store(),
                &parameters,
                flags,
                None,
                &mut self.chain,
            )
        };
        built.map_err(|error| error.code().0)?;
        if self.chain.is_null() {
            return Err(E_UNEXPECTED.0);
        }
        // SAFETY: successful chain API returned live context held until close.
        let trust_errors = unsafe { (*self.chain).TrustStatus.dwErrorStatus };
        let mut name = [0u16; 254];
        // Request construction already enforced ASCII <=253 and no NUL.
        for (target, byte) in name.iter_mut().zip(host.bytes()) {
            *target = byte as u16;
        }
        let mut ssl = HTTPSPolicyCallbackData {
            Anonymous: HTTPSPolicyCallbackData_0 {
                cbSize: std::mem::size_of::<HTTPSPolicyCallbackData>() as u32,
            },
            dwAuthType: AUTHTYPE_SERVER,
            fdwChecks: 0,
            pwszServerName: windows::core::PWSTR(name.as_mut_ptr()),
        };
        let policy = CERT_CHAIN_POLICY_PARA {
            cbSize: std::mem::size_of::<CERT_CHAIN_POLICY_PARA>() as u32,
            dwFlags: CERT_CHAIN_POLICY_FLAGS(0),
            pvExtraPolicyPara: (&mut ssl as *mut HTTPSPolicyCallbackData).cast(),
        };
        let mut status = CERT_CHAIN_POLICY_STATUS {
            cbSize: std::mem::size_of::<CERT_CHAIN_POLICY_STATUS>() as u32,
            ..Default::default()
        };
        // SAFETY: chain and bounded terminated server name remain live for this
        // call; no ignored date/name/usage/trust/revocation errors.
        let checked = unsafe {
            CertVerifyCertificateChainPolicy(
                CERT_CHAIN_POLICY_SSL,
                self.chain,
                &policy,
                &mut status,
            )
        };
        let policy = if checked.as_bool() {
            // TRUE means the policy was evaluated, NOT that it passed.
            Ok(status.dwError)
        } else {
            Err(windows::core::Error::from_win32().code().0)
        };
        Ok(ChainObservation {
            trust_errors,
            policy,
        })
    }

    fn close(&mut self) -> Result<(), i32> {
        if self.cleanup_failed {
            return Err(E_UNEXPECTED.0);
        }
        // Chain retains certificate references; discharge it before selected
        // key/certificate/store. Store close additionally checks outstanding
        // context debt for this explicit diagnostic (not the legacy TLS path).
        self.free_chain();
        let result = self.selected.close(true).map_err(|error| match error {
            TlsError::Cleanup(code) => code,
            _ => E_UNEXPECTED.0,
        });
        self.cleanup_failed = result.is_err();
        result
    }
}

impl Drop for NativeQuery {
    fn drop(&mut self) {
        self.free_chain();
        // SelectedCertificate's Drop is fallback; successful delivery always
        // observed QueryDriver::close before reaching this destructor.
    }
}
