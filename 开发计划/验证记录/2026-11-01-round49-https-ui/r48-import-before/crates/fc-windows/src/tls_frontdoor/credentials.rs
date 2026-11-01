//! Read-only certificate selection. Provisioning belongs to the administrator;
//! this module never imports certificates or creates/deletes key containers.
use super::{CertificateThumbprint, TlsError};
use std::{marker::PhantomData, rc::Rc};
use windows::Win32::Security::{
    Authentication::Identity::*, Credentials::SecHandle, Cryptography::*,
};

pub(super) struct Credentials {
    pub handle: SecHandle,
    acquired: bool,
    certificate: *mut CERT_CONTEXT,
    store: Option<HCERTSTORE>,
    key: Option<(HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, CERT_KEY_SPEC)>,
    // Construction, use and destruction all belong to the fixed SSPI thread.
    _thread_bound: PhantomData<Rc<()>>,
}

impl Credentials {
    pub fn new(thumbprint: CertificateThumbprint) -> Result<Self, TlsError> {
        let name = windows::core::w!("MY");
        // SAFETY: constant provider/name, read-only existing current-user store.
        let store = unsafe {
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                CERT_QUERY_ENCODING_TYPE(0),
                None,
                CERT_OPEN_STORE_FLAGS(CERT_SYSTEM_STORE_CURRENT_USER)
                    | CERT_STORE_OPEN_EXISTING_FLAG
                    | CERT_STORE_READONLY_FLAG,
                Some(name.as_ptr().cast()),
            )
        }
        .map_err(|_| TlsError::Certificate)?;
        let mut result = Self {
            handle: SecHandle::default(),
            acquired: false,
            certificate: std::ptr::null_mut(),
            store: Some(store),
            key: None,
            _thread_bound: PhantomData,
        };
        let initialized = (|| -> Result<(), TlsError> {
            let hash = CRYPT_INTEGER_BLOB {
                cbData: 20,
                pbData: thumbprint.bytes().as_ptr().cast_mut(),
            };
            // SAFETY: hash remains live and points to exactly twenty public bytes.
            result.certificate = unsafe {
                CertFindCertificateInStore(
                    store,
                    X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                    0,
                    CERT_FIND_HASH,
                    Some((&hash as *const CRYPT_INTEGER_BLOB).cast()),
                    None,
                )
            };
            if result.certificate.is_null() {
                return Err(TlsError::Certificate);
            }
            let mut actual = [0u8; 20];
            let mut actual_len = 20;
            // SAFETY: owned live certificate and exactly-sized writable hash buffer.
            unsafe {
                CertGetCertificateContextProperty(
                    result.certificate,
                    CERT_SHA1_HASH_PROP_ID,
                    Some(actual.as_mut_ptr().cast()),
                    &mut actual_len,
                )
            }
            .map_err(|_| TlsError::Certificate)?;
            if actual_len != 20 || &actual != thumbprint.bytes() {
                return Err(TlsError::Certificate);
            }
            // SAFETY: CertFindCertificateInStore returned a live context owned above.
            if unsafe { CertVerifyTimeValidity(None, (*result.certificate).pCertInfo) } != 0
                || !server_auth_usage(result.certificate)?
            {
                return Err(TlsError::Certificate);
            }
            let mut key = HCRYPTPROV_OR_NCRYPT_KEY_HANDLE::default();
            let mut spec = CERT_KEY_SPEC::default();
            let mut free = windows::core::BOOL(0);
            // SAFETY: only opens the selected existing private key, never prompts.
            unsafe {
                CryptAcquireCertificatePrivateKey(
                    result.certificate,
                    CRYPT_ACQUIRE_SILENT_FLAG
                        | CRYPT_ACQUIRE_ALLOW_NCRYPT_KEY_FLAG
                        | CRYPT_ACQUIRE_NO_HEALING
                        | CRYPT_ACQUIRE_COMPARE_KEY_FLAG,
                    None,
                    &mut key,
                    Some(&mut spec),
                    Some(&mut free),
                )
            }
            .map_err(|_| TlsError::Certificate)?;
            if free.as_bool() {
                result.key = Some((key, spec));
            }
            if key.0 == 0 {
                return Err(TlsError::Certificate);
            }
            let mut certificate = result.certificate;
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
            if let Some((key, spec)) = self.key.take() {
                if spec == CERT_NCRYPT_KEY_SPEC {
                    remember(NCryptFreeObject(NCRYPT_HANDLE(key.0)));
                } else {
                    remember(CryptReleaseContext(key.0, 0));
                }
            }
            let certificate = std::mem::replace(&mut self.certificate, std::ptr::null_mut());
            if !certificate.is_null() {
                remember(CertFreeCertificateContext(Some(certificate)).ok());
            }
            if let Some(store) = self.store.take() {
                remember(CertCloseStore(Some(store), 0));
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

fn server_auth_usage(certificate: *const CERT_CONTEXT) -> Result<bool, TlsError> {
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
    unsafe { CertGetEnhancedKeyUsage(certificate, 0, Some(base.cast()), &mut bytes) }
        .map_err(|_| TlsError::Certificate)?;
    if bytes as usize > capacity || bytes < std::mem::size_of::<CTL_USAGE>() as u32 {
        return Err(TlsError::Certificate);
    }
    // SAFETY: the first call required at least a CTL_USAGE and second succeeded.
    let usage = unsafe { &*base.cast::<CTL_USAGE>() };
    let count = usage.cUsageIdentifier as usize;
    if count == 0 || count > 128 {
        return Ok(false);
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
    Ok(found)
}

impl Drop for Credentials {
    fn drop(&mut self) {
        // Explicit worker shutdown observes close(); unwinding is best effort.
        let _ = self.close();
    }
}
