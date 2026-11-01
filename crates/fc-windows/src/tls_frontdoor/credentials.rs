//! Read-only certificate selection. Provisioning belongs to the administrator;
//! this module never imports certificates or creates/deletes key containers.
use super::certificate_selection::SelectedCertificate;
use super::{CertificateThumbprint, TlsError};
use windows::Win32::Security::{
    Authentication::Identity::*, Credentials::SecHandle, Cryptography::*,
};

pub(super) struct Credentials {
    pub handle: SecHandle,
    acquired: bool,
    selected: SelectedCertificate,
}

impl Credentials {
    pub fn new(thumbprint: CertificateThumbprint) -> Result<Self, TlsError> {
        let selected = SelectedCertificate::open().map_err(|_| TlsError::Certificate)?;
        let mut result = Self {
            handle: SecHandle::default(),
            acquired: false,
            selected,
        };
        let initialized = (|| -> Result<(), TlsError> {
            if !result
                .selected
                .find(thumbprint)
                .map_err(|_| TlsError::Certificate)?
            {
                return Err(TlsError::Certificate);
            }
            // SAFETY: CertFindCertificateInStore returned a live context owned above.
            if unsafe { CertVerifyTimeValidity(None, (*result.selected.certificate()).pCertInfo) }
                != 0
                || server_auth_usage(result.selected.certificate())? != ServerAuthUsage::Explicit
            {
                return Err(TlsError::Certificate);
            }
            result
                .selected
                .acquire_private_key()
                .map_err(|_| TlsError::Certificate)?;
            let mut certificate = result.selected.certificate();
            let config = SCHANNEL_CRED {
                dwVersion: SCHANNEL_CRED_VERSION,
                cCreds: 1,
                paCred: &mut certificate,
                grbitEnabledProtocols: SP_PROT_TLS1_2_SERVER,
                dwMinimumCipherStrength: 128,
                dwFlags: SCH_USE_STRONG_CRYPTO | SCH_CRED_NO_SYSTEM_MAPPER,
                ..Default::default()
            };
            // SAFETY: all input pointers remain live through the synchronous call;
            // the certificate and any caller-owned private-key handle outlive creds.
            unsafe {
                AcquireCredentialsHandleW(
                    windows::core::PCWSTR::null(),
                    UNISP_NAME_W,
                    SECPKG_CRED_INBOUND,
                    None,
                    Some((&config as *const SCHANNEL_CRED).cast()),
                    None,
                    None,
                    &mut result.handle,
                    None,
                )
            }
            .map_err(|e| TlsError::Native(e.code().0))?;
            result.acquired = true;
            Ok(())
        })();
        if let Err(error) = initialized {
            // Readiness failures must report native cleanup failure distinctly:
            // a caller may retry only after every partially owned handle closed.
            return match result.close() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(cleanup),
            };
        }
        Ok(result)
    }

    pub fn close(&mut self) -> Result<(), TlsError> {
        let mut failure = None;
        let mut remember = |result: windows::core::Result<()>| {
            if let Err(error) = result {
                failure.get_or_insert(TlsError::Cleanup(error.code().0));
            }
        };
        // SAFETY: this !Send/!Sync owner stays on its creating SSPI thread.
        // Each ownership flag is consumed before its one release attempt. All
        // releases are attempted even if an earlier native release fails.
        unsafe {
            if std::mem::replace(&mut self.acquired, false) {
                remember(FreeCredentialsHandle(&self.handle));
            }
        }
        if let Err(error) = self.selected.close(false) {
            failure.get_or_insert(error);
        }
        failure.map_or(Ok(()), Err)
    }
}

/// Explicit is the historical Schannel admission requirement. Inspection also
/// distinguishes unrestricted usage (the documented zero-count/not-found case).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ServerAuthUsage {
    Explicit,
    Unrestricted,
    Rejected,
    Unverified,
}

pub(super) fn server_auth_usage(
    certificate: *const CERT_CONTEXT,
) -> Result<ServerAuthUsage, TlsError> {
    let mut bytes = 0;
    // SAFETY: live caller-owned certificate; first call only obtains buffer size.
    unsafe { CertGetEnhancedKeyUsage(certificate, 0, None, &mut bytes) }
        .map_err(|_| TlsError::Certificate)?;
    if bytes < std::mem::size_of::<CTL_USAGE>() as u32 || bytes > 8192 {
        return Err(TlsError::Certificate);
    }
    // u64 backing supplies sufficient alignment for native pointer-containing CTL_USAGE.
    let mut backing = vec![0u64; (bytes as usize).div_ceil(8)];
    let capacity = backing.len() * 8;
    let base = backing.as_mut_ptr().cast::<u8>();
    // SAFETY: aligned writable region of at least the requested bounded size.
    unsafe { windows::Win32::Foundation::SetLastError(windows::Win32::Foundation::WIN32_ERROR(0)) };
    let usage_result =
        unsafe { CertGetEnhancedKeyUsage(certificate, 0, Some(base.cast()), &mut bytes) };
    // GetLastError distinguishes unrestricted zero usage from an empty allowed
    // intersection; capture it before any other native call.
    let usage_error = unsafe { windows::Win32::Foundation::GetLastError() }.0;
    usage_result.map_err(|_| TlsError::Certificate)?;
    if bytes as usize > capacity || bytes < std::mem::size_of::<CTL_USAGE>() as u32 {
        return Err(TlsError::Certificate);
    }
    // SAFETY: the first call required at least a CTL_USAGE and second succeeded.
    let usage = unsafe { &*base.cast::<CTL_USAGE>() };
    let count = usage.cUsageIdentifier as usize;
    if count == 0 {
        return Ok(empty_usage(usage_error));
    }
    if count > 128 {
        return Ok(ServerAuthUsage::Unverified);
    }
    let start = base as usize;
    let end = start
        .checked_add(bytes as usize)
        .ok_or(TlsError::Certificate)?;
    let pointers = usage.rgpszUsageIdentifier as usize;
    let pointer_bytes = count
        .checked_mul(std::mem::size_of::<windows::core::PSTR>())
        .ok_or(TlsError::Certificate)?;
    if pointers < start
        || pointers.checked_add(pointer_bytes).is_none_or(|n| n > end)
        || !pointers.is_multiple_of(std::mem::align_of::<windows::core::PSTR>())
    {
        return Err(TlsError::Certificate);
    }
    const SERVER_AUTH: &[u8] = b"1.3.6.1.5.5.7.3.1\0";
    let mut found = false;
    for index in 0..count {
        // SAFETY: checked complete aligned native pointer array above.
        let oid = unsafe { *usage.rgpszUsageIdentifier.add(index) }.as_ptr() as usize;
        if oid < start || oid >= end {
            return Err(TlsError::Certificate);
        }
        let length = (end - oid).min(256);
        // SAFETY: this bounded slice lies wholly inside the native result owner.
        let value = unsafe { std::slice::from_raw_parts(oid as *const u8, length) };
        let nul = value
            .iter()
            .position(|b| *b == 0)
            .ok_or(TlsError::Certificate)?;
        found |= &value[..=nul] == SERVER_AUTH;
    }
    Ok(if found {
        ServerAuthUsage::Explicit
    } else {
        ServerAuthUsage::Rejected
    })
}

fn empty_usage(last_error: u32) -> ServerAuthUsage {
    match last_error {
        0x80092004 => ServerAuthUsage::Unrestricted, // CRYPT_E_NOT_FOUND
        0 => ServerAuthUsage::Rejected,
        _ => ServerAuthUsage::Unverified,
    }
}

impl Drop for Credentials {
    fn drop(&mut self) {
        // Explicit worker shutdown observes close(); unwinding is best effort.
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_query_empty_eku_preserves_unrestricted_empty_and_unknown_distinction() {
        assert_eq!(empty_usage(0x80092004), ServerAuthUsage::Unrestricted);
        assert_eq!(empty_usage(0), ServerAuthUsage::Rejected);
        assert_eq!(empty_usage(5), ServerAuthUsage::Unverified);
        // None of those becomes Explicit, so legacy Credentials admission
        // still rejects every zero-count case exactly as before extraction.
        for error in [0x80092004, 0, 5] {
            assert_ne!(empty_usage(error), ServerAuthUsage::Explicit);
        }
    }
}
