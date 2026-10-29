//! Current-user DPAPI. No machine-wide flag, prompting, logging, or secret cloning.
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{compiler_fence, Ordering},
};
use windows::{
    core::{w, Error, Result},
    Win32::{Foundation::*, Security::Cryptography::*},
};

const MAX_SECRET: usize = 64 * 1024;
const ENTROPY: &[u8] = b"FutureClassroom.CurrentUserCredential.v1";

/// Plaintext is intentionally neither Clone nor Debug. Access is explicit and borrowed.
/// This buffer and the adapter remain on the execution thread which owns the operation.
pub struct SecretBytes {
    bytes: Vec<u8>,
    _thread: PhantomData<Rc<()>>,
}
impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_SECRET {
            // Wipe caller-transferred plaintext even when length validation fails.
            let mut bytes = bytes;
            wipe(&mut bytes);
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        Ok(Self {
            bytes,
            _thread: PhantomData,
        })
    }
    pub fn expose(&self) -> &[u8] {
        &self.bytes
    }
}
impl Drop for SecretBytes {
    fn drop(&mut self) {
        wipe(&mut self.bytes);
    }
}
fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        // SAFETY: exclusive live slice; volatile stores prevent removal of the wipe.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

struct DpapiAllocation(CRYPT_INTEGER_BLOB);
impl Drop for DpapiAllocation {
    fn drop(&mut self) {
        if !self.0.pbData.is_null() {
            // SAFETY: DPAPI transfers a LocalAlloc buffer with exactly cbData bytes.
            // This owner is unique, no slice survives here, and LocalFree matches it.
            unsafe {
                wipe(std::slice::from_raw_parts_mut(
                    self.0.pbData,
                    self.0.cbData as usize,
                ));
                let _ = LocalFree(Some(HLOCAL(self.0.pbData.cast())));
            }
        }
    }
}

#[derive(Default)]
pub struct CurrentUserDpapi(PhantomData<Rc<()>>);
impl CurrentUserDpapi {
    pub fn protect(&self, secret: &SecretBytes) -> Result<Vec<u8>> {
        self.transform(secret.expose(), true)
    }
    pub fn unprotect(&self, ciphertext: &[u8]) -> Result<SecretBytes> {
        SecretBytes::new(self.transform(ciphertext, false)?)
    }
    fn transform(&self, input: &[u8], protect: bool) -> Result<Vec<u8>> {
        let limit = if protect {
            MAX_SECRET
        } else {
            MAX_SECRET + 4096
        };
        if input.is_empty() || input.len() > limit {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let source = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_ptr().cast_mut(),
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr().cast_mut(),
        };
        let mut output = DpapiAllocation(CRYPT_INTEGER_BLOB::default());
        // SAFETY: input/entropy are borrowed for the synchronous API call; APIs do
        // not mutate input despite legacy mutable ABI fields. No pointer escapes.
        // Output has a unique LocalFree owner on both success and failure paths.
        unsafe {
            if protect {
                CryptProtectData(
                    &source,
                    w!("Future Classroom credential v1"),
                    Some(&entropy),
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )?;
            } else {
                CryptUnprotectData(
                    &source,
                    None,
                    Some(&entropy),
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )?;
            }
            let output_limit = if protect {
                MAX_SECRET + 4096
            } else {
                MAX_SECRET
            };
            if output.0.pbData.is_null()
                || output.0.cbData == 0
                || output.0.cbData as usize > output_limit
            {
                return Err(Error::from_hresult(E_UNEXPECTED));
            }
            Ok(std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize).to_vec())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_user_dpapi_roundtrip_tamper_and_bounds() {
        let dpapi = CurrentUserDpapi::default();
        let secret = SecretBytes::new("合成口令 · 不是真实凭据".as_bytes().to_vec()).unwrap();
        let mut protected = dpapi.protect(&secret).unwrap();
        assert_ne!(protected, secret.expose());
        assert_eq!(
            dpapi.unprotect(&protected).unwrap().expose(),
            secret.expose()
        );
        let position = protected.len() / 2;
        protected[position] ^= 0x80;
        assert!(dpapi.unprotect(&protected).is_err());
        assert!(dpapi.unprotect(&[]).is_err());
        assert!(SecretBytes::new(vec![0; MAX_SECRET + 1]).is_err());
    }
}
