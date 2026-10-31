use crate::{RandomPort, SecurityError, Sha256, Zeroizing, SALT_BYTES, VERIFIER_BYTES};
use sha2::Digest;
use std::fmt;
use subtle::ConstantTimeEq;

pub const RECOVERY_FORMAT: u32 = 1;
const RECOVERY_BYTES: usize = 32;
const CODE_PREFIX: &str = "FC1-";
const CODE_CHARACTERS: usize = CODE_PREFIX.len() + RECOVERY_BYTES * 2;
const HASH_DOMAIN: &[u8] = b"future-classroom/recovery/v1\0";

/// 256 unpredictable bits. No Clone, Display, serialization, or raw byte accessor.
/// Reveal consumes the secret; the UI must show it only after a successful commit.
pub struct RecoveryCode(Zeroizing<[u8; RECOVERY_BYTES]>);
impl RecoveryCode {
    /// Own and wipe untrusted input even on parse failure. Canonical output is ASCII;
    /// uppercase hex is accepted when a teacher pastes a printed recovery code.
    pub fn parse(input: String) -> Result<Self, SecurityError> {
        let input = Zeroizing::new(input);
        if input.len() != CODE_CHARACTERS || !input.starts_with(CODE_PREFIX) {
            return Err(SecurityError::InvalidRecoveryCode);
        }
        let mut bytes = Zeroizing::new([0u8; RECOVERY_BYTES]);
        for (dst, pair) in bytes
            .iter_mut()
            .zip(input.as_bytes()[CODE_PREFIX.len()..].chunks_exact(2))
        {
            let high = hex(pair[0]).ok_or(SecurityError::InvalidRecoveryCode)?;
            let low = hex(pair[1]).ok_or(SecurityError::InvalidRecoveryCode)?;
            *dst = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
    pub fn reveal(self) -> Zeroizing<String> {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = Zeroizing::new(String::with_capacity(CODE_CHARACTERS));
        text.push_str(CODE_PREFIX);
        for byte in self.0.iter() {
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 15)]));
        }
        text
    }
}
fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode([REDACTED])")
    }
}
pub struct RecoveryRecord {
    pub(crate) format: u32,
    pub(crate) salt: Zeroizing<[u8; SALT_BYTES]>,
    pub(crate) verifier: Zeroizing<[u8; VERIFIER_BYTES]>,
}
impl fmt::Debug for RecoveryRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryRecord([REDACTED])")
    }
}
impl RecoveryRecord {
    pub fn format(&self) -> u32 {
        self.format
    }
    pub(crate) fn from_parts(
        format: u32,
        salt: [u8; SALT_BYTES],
        verifier: [u8; VERIFIER_BYTES],
    ) -> Result<Self, SecurityError> {
        let salt = Zeroizing::new(salt);
        let verifier = Zeroizing::new(verifier);
        if format != RECOVERY_FORMAT {
            return Err(SecurityError::UnsupportedCredential);
        }
        Ok(Self {
            format,
            salt,
            verifier,
        })
    }
}
fn digest(code: &RecoveryCode, salt: &[u8; SALT_BYTES]) -> Zeroizing<[u8; VERIFIER_BYTES]> {
    let mut hash = Sha256::new();
    hash.update(HASH_DOMAIN);
    hash.update(salt);
    hash.update(code.0.as_ref());
    let mut output = hash.finalize();
    let mut verifier = Zeroizing::new([0u8; VERIFIER_BYTES]);
    verifier.copy_from_slice(&output);
    use zeroize::Zeroize;
    output.as_mut_slice().zeroize();
    verifier
}
pub fn create_recovery_record<R: RandomPort>(
    random: &mut R,
) -> Result<(RecoveryCode, RecoveryRecord), SecurityError> {
    let mut code = Zeroizing::new([0u8; RECOVERY_BYTES]);
    let mut salt = Zeroizing::new([0u8; SALT_BYTES]);
    random
        .fill(&mut code[..])
        .map_err(|_| SecurityError::RandomUnavailable)?;
    random
        .fill(&mut salt[..])
        .map_err(|_| SecurityError::RandomUnavailable)?;
    let code = RecoveryCode(code);
    let verifier = digest(&code, &salt);
    Ok((
        code,
        RecoveryRecord {
            format: RECOVERY_FORMAT,
            salt,
            verifier,
        },
    ))
}
pub fn verify_recovery(
    code: &RecoveryCode,
    record: &RecoveryRecord,
) -> Result<bool, SecurityError> {
    if record.format != RECOVERY_FORMAT {
        return Err(SecurityError::UnsupportedCredential);
    }
    let candidate = digest(code, &record.salt);
    Ok(bool::from(candidate[..].ct_eq(&record.verifier[..])))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Deterministic(u8);
    impl RandomPort for Deterministic {
        type Error = ();
        fn fill(&mut self, out: &mut [u8]) -> Result<(), ()> {
            self.0 += 1;
            out.fill(self.0);
            Ok(())
        }
    }
    struct Failure;
    impl RandomPort for Failure {
        type Error = ();
        fn fill(&mut self, _: &mut [u8]) -> Result<(), ()> {
            Err(())
        }
    }
    #[test]
    fn random_recovery_has_independent_salt_and_reference_verifier() {
        let (code, record) = create_recovery_record(&mut Deterministic(0)).unwrap();
        assert!(verify_recovery(&code, &record).unwrap());
        // Independently generated with Python hashlib, not this implementation.
        let expected = [
            0x9e, 0x3a, 0xea, 0x0a, 0xc2, 0x15, 0x45, 0x6c, 0x18, 0x3d, 0x98, 0x84, 0x9b, 0x35,
            0xd4, 0xde, 0x8f, 0x56, 0xfd, 0x17, 0xa7, 0xf3, 0x24, 0x1a, 0xce, 0x06, 0x1d, 0xd8,
            0x6d, 0xc5, 0xd0, 0x64,
        ];
        assert_eq!(&record.verifier[..], expected);
        assert!(!format!("{code:?} {record:?}").contains("01010101"));
        let shown = code.reveal();
        assert_eq!(shown.len(), 68);
        let parsed = RecoveryCode::parse(shown.to_uppercase()).unwrap();
        assert!(verify_recovery(&parsed, &record).unwrap());
        let (other, other_record) = create_recovery_record(&mut Deterministic(2)).unwrap();
        assert!(!verify_recovery(&other, &record).unwrap());
        assert!(!verify_recovery(&parsed, &other_record).unwrap());
    }
    #[test]
    fn malformed_recovery_codes_and_rng_failure_fail_closed() {
        for input in [
            "".into(),
            "FC1-".into(),
            format!("FC1-{}", "0".repeat(63)),
            format!("FC1-{}g", "0".repeat(63)),
            format!("FC2-{}", "0".repeat(64)),
            format!("FC1-{}", "🦀".repeat(64)),
        ] {
            assert_eq!(
                RecoveryCode::parse(input).unwrap_err(),
                SecurityError::InvalidRecoveryCode
            );
        }
        assert_eq!(
            create_recovery_record(&mut Failure).unwrap_err(),
            SecurityError::RandomUnavailable
        );
        assert!(RecoveryRecord::from_parts(2, [0; 32], [0; 32]).is_err());
    }
    #[test]
    fn credential_port_rejects_wrong_recovery_before_preparing_password_or_new_recovery() {
        use crate::{CredentialCryptoAdapter, Password};
        use fc_domain::{
            identity::IdentityCommand,
            ids::{CommandId, PrincipalId, Revision},
        };
        use fc_ports::{prepare_recovery, CredentialCrypto, CredentialError, IdentityRequest};
        let (code, record) = create_recovery_record(&mut Deterministic(0)).unwrap();
        let (wrong, _) = create_recovery_record(&mut Deterministic(2)).unwrap();
        let mut crypto = CredentialCryptoAdapter::new(Failure);
        assert!(crypto.verify_recovery(&code, &record).unwrap());
        let request = IdentityRequest {
            command_id: CommandId::new(1).unwrap(),
            expected_revision: Revision::new(1),
            command: IdentityCommand::RecoveryConsumed {
                id: PrincipalId::new(1).unwrap(),
                expected_recovery_version: 1,
            },
        };
        let password = Password::new("new-long-password".into()).unwrap();
        assert!(matches!(
            prepare_recovery(&mut crypto, &wrong, &password, &record, request),
            Err(CredentialError::VerificationFailed)
        ));
        assert_eq!(
            crypto.recovery_record().unwrap_err(),
            CredentialError::Unavailable
        );
        assert_eq!(
            crypto.password_record(&password).unwrap_err(),
            CredentialError::Unavailable
        );
    }
}
