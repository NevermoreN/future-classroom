//! Exact existing CurrentUser\\MY selection shared by Schannel credentials and
//! the explicit read-only inspection. No credential/SSPI acquisition here.
use super::{CertificateThumbprint, TlsError};
use std::{marker::PhantomData, rc::Rc};
use windows::Win32::{
    Foundation::{GetLastError, CRYPT_E_NOT_FOUND, E_UNEXPECTED},
    Security::Cryptography::*,
};

pub(super) struct SelectedCertificate {
    certificate: *mut CERT_CONTEXT,
    store: Option<HCERTSTORE>,
    key: Option<(HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, CERT_KEY_SPEC)>,
    // All native ownership stays on the synchronous creating worker.
    _thread_bound: PhantomData<Rc<()>>,
}

impl SelectedCertificate {
    pub fn open() -> windows::core::Result<Self> {
        // SAFETY: constant existing current-user store; no write or fallback.
        let store = unsafe {
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                CERT_QUERY_ENCODING_TYPE(0),
                None,
                CERT_OPEN_STORE_FLAGS(CERT_SYSTEM_STORE_CURRENT_USER)
                    | CERT_STORE_OPEN_EXISTING_FLAG
                    | CERT_STORE_READONLY_FLAG,
                Some(windows::core::w!("MY").as_ptr().cast()),
            )
        }?;
        Ok(Self {
            certificate: std::ptr::null_mut(),
            store: Some(store),
            key: None,
            _thread_bound: PhantomData,
        })
    }

    pub fn find(&mut self, thumbprint: CertificateThumbprint) -> windows::core::Result<bool> {
        // This is a one-shot owner, never an enumeration cursor.
        if !self.certificate.is_null() || self.key.is_some() {
            return Err(E_UNEXPECTED.into());
        }
        let store = self
            .store
            .ok_or_else(|| windows::core::Error::from(E_UNEXPECTED))?;
        let hash = CRYPT_INTEGER_BLOB {
            cbData: 20,
            pbData: thumbprint.bytes().as_ptr().cast_mut(),
        };
        // SAFETY: exactly twenty live public hash bytes, no previous context.
        self.certificate = unsafe {
            CertFindCertificateInStore(
                store,
                X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                0,
                CERT_FIND_HASH,
                Some((&hash as *const CRYPT_INTEGER_BLOB).cast()),
                None,
            )
        };
        if self.certificate.is_null() {
            // SAFETY: immediately captures the failed native lookup's status.
            let error = unsafe { GetLastError() };
            return if error.0 == CRYPT_E_NOT_FOUND.0 as u32 {
                Ok(false)
            } else {
                Err(windows::core::Error::from_hresult(if error.0 == 0 {
                    E_UNEXPECTED
                } else {
                    windows::core::HRESULT::from_win32(error.0)
                }))
            };
        }
        let mut actual = [0u8; 20];
        let mut actual_len = 20;
        // SAFETY: owned selected context and exactly-sized writable buffer.
        unsafe {
            CertGetCertificateContextProperty(
                self.certificate,
                CERT_SHA1_HASH_PROP_ID,
                Some(actual.as_mut_ptr().cast()),
                &mut actual_len,
            )
        }?;
        if actual_len != 20 || &actual != thumbprint.bytes() {
            return Err(E_UNEXPECTED.into());
        }
        Ok(true)
    }

    pub fn certificate(&self) -> *mut CERT_CONTEXT {
        self.certificate
    }

    pub fn store(&self) -> Option<HCERTSTORE> {
        self.store
    }

    pub fn acquire_private_key(&mut self) -> windows::core::Result<()> {
        if self.certificate.is_null() || self.key.is_some() {
            return Err(E_UNEXPECTED.into());
        }
        let mut key = HCRYPTPROV_OR_NCRYPT_KEY_HANDLE::default();
        let mut spec = CERT_KEY_SPEC::default();
        let mut caller_free = windows::core::BOOL(0);
        // SAFETY: same existing-key flags as historical Credentials::new. No
        // UI/cache creation/healing flag. Borrowed handles live with the cert.
        unsafe {
            CryptAcquireCertificatePrivateKey(
                self.certificate,
                CRYPT_ACQUIRE_SILENT_FLAG
                    | CRYPT_ACQUIRE_ALLOW_NCRYPT_KEY_FLAG
                    | CRYPT_ACQUIRE_NO_HEALING
                    | CRYPT_ACQUIRE_COMPARE_KEY_FLAG,
                None,
                &mut key,
                Some(&mut spec),
                Some(&mut caller_free),
            )
        }?;
        if caller_free.as_bool() {
            self.key = Some((key, spec));
        }
        if key.0 == 0 {
            return Err(E_UNEXPECTED.into());
        }
        Ok(())
    }

    pub fn close(&mut self, check_store: bool) -> Result<(), TlsError> {
        if check_store {
            return self.close_checked();
        }
        let mut failure = None;
        let mut remember = |result: windows::core::Result<()>| {
            if let Err(error) = result {
                failure.get_or_insert(TlsError::Cleanup(error.code().0));
            }
        };
        // SAFETY: unique thread-bound owner; consume each original reference
        // once and attempt every release even after a preceding failure.
        unsafe {
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
                remember(CertCloseStore(
                    Some(store),
                    if check_store {
                        CERT_CLOSE_STORE_CHECK_FLAG
                    } else {
                        0
                    },
                ));
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// Inspection keeps the exact failed reference and all dependencies for
    /// terminal quarantine. It never retries a release whose failure leaves
    /// ownership uncertain. The legacy Credentials path above stays unchanged.
    fn close_checked(&mut self) -> Result<(), TlsError> {
        let cleanup = |error: windows::core::Error| TlsError::Cleanup(error.code().0);
        if let Some((key, spec)) = self.key {
            // SAFETY: caller-owned key from the one completed synchronous
            // acquisition. On failure retain it and its certificate/store.
            unsafe {
                if spec == CERT_NCRYPT_KEY_SPEC {
                    NCryptFreeObject(NCRYPT_HANDLE(key.0))
                } else {
                    CryptReleaseContext(key.0, 0)
                }
            }
            .map_err(cleanup)?;
            self.key = None;
        }
        if !self.certificate.is_null() {
            // SAFETY: sole selected context reference, after own key release.
            unsafe { CertFreeCertificateContext(Some(self.certificate)) }
                .ok()
                .map_err(cleanup)?;
            self.certificate = std::ptr::null_mut();
        }
        if let Some(store) = self.store.take() {
            // SAFETY: chain and our certificate references already returned.
            // CertCloseStore ALWAYS closes the handle, including FALSE with
            // CHECK_FLAG, so consume this reference before the single call.
            unsafe { CertCloseStore(Some(store), CERT_CLOSE_STORE_CHECK_FLAG) }.map_err(cleanup)?;
        }
        Ok(())
    }
}

impl Drop for SelectedCertificate {
    fn drop(&mut self) {
        // Explicit callers observe close failures; unwind cleanup is fallback.
        let _ = self.close(false);
    }
}
