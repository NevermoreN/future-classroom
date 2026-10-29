//! Password/recovery cryptography only. Callers must schedule KDF work on the bounded worker
//! pool and enforce authentication, persistence and rate limits separately.
#![forbid(unsafe_code)]
use fc_ports::RandomPort;
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

mod codec;
mod recovery;
pub use codec::{CredentialRecordCodec, EncodedCredential, CREDENTIAL_RECORD_BYTES};
pub use recovery::{
    create_recovery_record, verify_recovery, RecoveryCode, RecoveryRecord, RECOVERY_FORMAT,
};

pub const PASSWORD_FORMAT: u32 = 1;
pub const PBKDF2_ITERATIONS: u32 = 600_000;
pub const SALT_BYTES: usize = 32;
pub const VERIFIER_BYTES: usize = 32;
/// 128 Unicode scalar values, each at most four UTF-8 bytes.
pub const MAX_PASSWORD_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityError {
    PasswordLength,
    RandomUnavailable,
    UnsupportedCredential,
    InvalidRecoveryCode,
    CorruptCredential,
    InvalidCodecKey,
    InvalidBinding,
}

/// Inject the platform CSPRNG at composition (Windows uses SystemRandom / BCrypt).
/// This adapter owns neither threads nor persistence. KDF scheduling/rate limits and
/// atomic credential-version checks remain the application's responsibility.
pub struct CredentialCryptoAdapter<R> {
    random: R,
}
impl<R: RandomPort> CredentialCryptoAdapter<R> {
    pub fn new(random: R) -> Self {
        Self { random }
    }
}
fn port_error(error: SecurityError) -> fc_ports::CredentialError {
    match error {
        SecurityError::RandomUnavailable => fc_ports::CredentialError::Unavailable,
        SecurityError::PasswordLength | SecurityError::InvalidRecoveryCode => {
            fc_ports::CredentialError::InvalidSecret
        }
        _ => fc_ports::CredentialError::StaleCredential,
    }
}
impl<R: RandomPort> fc_ports::CredentialCrypto for CredentialCryptoAdapter<R> {
    type Secret = Password;
    type PasswordRecord = PasswordRecord;
    type RecoveryCode = RecoveryCode;
    type RecoveryRecord = RecoveryRecord;
    fn password_record(
        &mut self,
        secret: &Password,
    ) -> Result<PasswordRecord, fc_ports::CredentialError> {
        create_password_record(secret, &mut self.random).map_err(port_error)
    }
    fn verify_password(
        &mut self,
        secret: &Password,
        record: &PasswordRecord,
    ) -> Result<bool, fc_ports::CredentialError> {
        verify_password(secret, record).map_err(port_error)
    }
    fn recovery_record(
        &mut self,
    ) -> Result<(RecoveryCode, RecoveryRecord), fc_ports::CredentialError> {
        create_recovery_record(&mut self.random).map_err(port_error)
    }
    fn verify_recovery(
        &mut self,
        code: &RecoveryCode,
        record: &RecoveryRecord,
    ) -> Result<bool, fc_ports::CredentialError> {
        verify_recovery(code, record).map_err(port_error)
    }
}

/// Owned and wiped on drop. Deliberately neither Clone nor Display.
pub struct Password(Zeroizing<String>);

impl Password {
    /// Consumes the allocation so it is wiped even when validation fails.
    pub fn new(value: String) -> Result<Self, SecurityError> {
        let secret = Zeroizing::new(value);
        if secret.len() > MAX_PASSWORD_BYTES {
            return Err(SecurityError::PasswordLength);
        }
        let characters = secret.chars().count();
        if !(12..=128).contains(&characters) {
            return Err(SecurityError::PasswordLength);
        }
        Ok(Self(secret))
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password([REDACTED])")
    }
}

/// Persist only through the credential store, never in ordinary snapshots or logs.
/// The verifier and salt are wiped when this owned record leaves scope.
pub struct PasswordRecord {
    format: u32,
    iterations: u32,
    salt: Zeroizing<[u8; SALT_BYTES]>,
    verifier: Zeroizing<[u8; VERIFIER_BYTES]>,
}

impl fmt::Debug for PasswordRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PasswordRecord([REDACTED])")
    }
}

impl PasswordRecord {
    /// An untrusted record cannot select an unbounded or weakened work factor.
    /// Legacy formats require an explicit, separately tested migration adapter.
    pub fn from_parts(
        format: u32,
        iterations: u32,
        salt: [u8; SALT_BYTES],
        verifier: [u8; VERIFIER_BYTES],
    ) -> Result<Self, SecurityError> {
        let salt = Zeroizing::new(salt);
        let verifier = Zeroizing::new(verifier);
        if format != PASSWORD_FORMAT || iterations != PBKDF2_ITERATIONS {
            return Err(SecurityError::UnsupportedCredential);
        }
        Ok(Self {
            format,
            iterations,
            salt,
            verifier,
        })
    }

    pub fn format(&self) -> u32 {
        self.format
    }
    pub fn iterations(&self) -> u32 {
        self.iterations
    }
    pub fn salt(&self) -> &[u8; SALT_BYTES] {
        &self.salt
    }
    pub fn verifier(&self) -> &[u8; VERIFIER_BYTES] {
        &self.verifier
    }
}

pub fn create_password_record<R: RandomPort>(
    password: &Password,
    random: &mut R,
) -> Result<PasswordRecord, SecurityError> {
    let mut salt = Zeroizing::new([0u8; SALT_BYTES]);
    random
        .fill(&mut salt[..])
        .map_err(|_| SecurityError::RandomUnavailable)?;
    let mut verifier = Zeroizing::new([0u8; VERIFIER_BYTES]);
    pbkdf2_hmac::<Sha256>(
        password.0.as_bytes(),
        &salt[..],
        PBKDF2_ITERATIONS,
        &mut verifier[..],
    );
    Ok(PasswordRecord {
        format: PASSWORD_FORMAT,
        iterations: PBKDF2_ITERATIONS,
        salt,
        verifier,
    })
}

pub fn verify_password(
    password: &Password,
    record: &PasswordRecord,
) -> Result<bool, SecurityError> {
    if record.format != PASSWORD_FORMAT || record.iterations != PBKDF2_ITERATIONS {
        return Err(SecurityError::UnsupportedCredential);
    }
    let mut candidate = Zeroizing::new([0u8; VERIFIER_BYTES]);
    pbkdf2_hmac::<Sha256>(
        password.0.as_bytes(),
        &record.salt[..],
        record.iterations,
        &mut candidate[..],
    );
    Ok(bool::from(candidate[..].ct_eq(&record.verifier[..])))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRandom(u8);
    impl RandomPort for TestRandom {
        type Error = ();
        fn fill(&mut self, output: &mut [u8]) -> Result<(), ()> {
            self.0 = self.0.wrapping_add(1);
            output.fill(self.0);
            Ok(())
        }
    }
    struct FailedRandom;
    impl RandomPort for FailedRandom {
        type Error = ();
        fn fill(&mut self, _: &mut [u8]) -> Result<(), ()> {
            Err(())
        }
    }

    #[test]
    fn unicode_bounds_and_redaction() {
        assert!(Password::new("密".repeat(11)).is_err());
        assert!(Password::new("密".repeat(12)).is_ok());
        assert!(Password::new("🦀".repeat(128)).is_ok());
        assert!(Password::new("🦀".repeat(129)).is_err());
        let password = Password::new("secret-password".to_owned()).unwrap();
        assert!(!format!("{password:?}").contains("secret-password"));
        assert_eq!(
            create_password_record(&password, &mut FailedRandom).unwrap_err(),
            SecurityError::RandomUnavailable
        );
    }

    #[test]
    fn independent_salts_and_constant_time_verification() {
        let password = Password::new("中文口令至少十二个汉字用以测试".to_owned()).unwrap();
        let wrong = Password::new("不同口令至少十二个汉字用以测试".to_owned()).unwrap();
        let mut random = TestRandom(0);
        let first = create_password_record(&password, &mut random).unwrap();
        let mut adapter = CredentialCryptoAdapter::new(random);
        let second = fc_ports::CredentialCrypto::password_record(&mut adapter, &password).unwrap();
        // Independently generated by Python hashlib/OpenSSL at the production
        // work factor, with this UTF-8 password and 32 bytes of 0x01 salt.
        assert_eq!(
            first.verifier(),
            &[
                0x44, 0xcc, 0xa2, 0x63, 0x4e, 0x31, 0x4a, 0x69, 0x0a, 0x71, 0x3b, 0x4a, 0xa3, 0x22,
                0x9b, 0x1b, 0xac, 0xb7, 0x49, 0x41, 0xe6, 0xe4, 0x4c, 0x54, 0x86, 0x72, 0xe7, 0xb1,
                0x99, 0xe6, 0x04, 0xe4
            ]
        );
        assert_ne!(first.salt(), second.salt());
        assert_ne!(first.verifier(), second.verifier());
        assert!(verify_password(&password, &first).unwrap());
        assert!(!verify_password(&wrong, &first).unwrap());
        assert!(PasswordRecord::from_parts(1, 1, [0; 32], [0; 32]).is_err());
    }

    #[test]
    fn standard_pbkdf2_sha256_vector() {
        // Widely published PBKDF2-HMAC-SHA256 password/salt, c=1 vector.
        let mut derived = Zeroizing::new([0u8; 32]);
        pbkdf2_hmac::<Sha256>(b"password", b"salt", 1, &mut derived[..]);
        assert_eq!(
            &derived[..],
            &[
                0x12, 0x0f, 0xb6, 0xcf, 0xfc, 0xf8, 0xb3, 0x2c, 0x43, 0xe7, 0x22, 0x52, 0x56, 0xc4,
                0xf8, 0x37, 0xa8, 0x65, 0x48, 0xc9, 0x2c, 0xcc, 0x35, 0x48, 0x08, 0x05, 0x98, 0x7c,
                0xb7, 0x0b, 0xe1, 0x7b
            ]
        );
    }
}
