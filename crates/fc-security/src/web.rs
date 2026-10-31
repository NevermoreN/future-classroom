//! Owned secrets for pairing and Web sessions. Expiry, attempt limits and session
//! authority belong to the caller; these primitives perform no I/O or persistence.
use fc_ports::RandomPort;
use sha2::{Digest, Sha256};
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const SECRET_BYTES: usize = 32;
const SECRET_HEX_BYTES: usize = SECRET_BYTES * 2;
const CODE_BYTES: usize = 8;
const RANDOM_ATTEMPTS: usize = 16;
const CODE_SPACE: u32 = 100_000_000;
// The inclusive interval 1..=42*CODE_SPACE gives every decimal code exactly
// 42 preimages. Excluding zero detects a stuck all-zero source without bias.
const CODE_RANDOM_MAX: u32 = 4_200_000_000;
const HASH_DOMAIN: &[u8] = b"fc-web-secret-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSecretError {
    InvalidSecret,
    InvalidPairingCode,
    RandomUnavailable,
}

/// 256-bit bearer secret. Deliberately no Clone, Display, serialization or raw
/// byte accessor; persistence must store only its domain-separated verifier.
pub struct WebSecret(Zeroizing<[u8; SECRET_BYTES]>);

impl WebSecret {
    pub fn generate<R: RandomPort>(random: &mut R) -> Result<Self, WebSecretError> {
        let mut bytes = Zeroizing::new([0; SECRET_BYTES]);
        for _ in 0..RANDOM_ATTEMPTS {
            random
                .fill(&mut bytes[..])
                .map_err(|_| WebSecretError::RandomUnavailable)?;
            if !bool::from(bytes[..].ct_eq(&[0; SECRET_BYTES])) {
                return Ok(Self(bytes));
            }
        }
        Err(WebSecretError::RandomUnavailable)
    }

    /// Consumes and wipes the supplied allocation, including on invalid input.
    /// Accepts exactly 64 lowercase ASCII hex digits and rejects the zero key.
    pub fn parse_hex(input: String) -> Result<Self, WebSecretError> {
        let input = Zeroizing::new(input);
        if input.len() != SECRET_HEX_BYTES {
            return Err(WebSecretError::InvalidSecret);
        }
        let mut bytes = Zeroizing::new([0; SECRET_BYTES]);
        for (output, pair) in bytes
            .iter_mut()
            .zip(input.as_bytes().as_chunks::<2>().0.iter())
        {
            let high = hex(pair[0]).ok_or(WebSecretError::InvalidSecret)?;
            let low = hex(pair[1]).ok_or(WebSecretError::InvalidSecret)?;
            *output = high << 4 | low;
        }
        if bool::from(bytes[..].ct_eq(&[0; SECRET_BYTES])) {
            return Err(WebSecretError::InvalidSecret);
        }
        Ok(Self(bytes))
    }

    /// Explicit disclosure for the Set-Cookie/CSRF/pairing response boundary.
    /// The returned allocation is wiped on drop and redacted in diagnostics.
    pub fn expose_hex(&self) -> SecretText {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = Zeroizing::new(String::with_capacity(SECRET_HEX_BYTES));
        for byte in self.0.iter() {
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 15)]));
        }
        SecretText(text)
    }

    /// SHA-256("fc-web-secret-v1" || secret32), with no textual encoding or
    /// separator appended to the fixed domain prefix.
    pub fn verifier(&self) -> SecretVerifier {
        let mut hash = Sha256::new();
        hash.update(HASH_DOMAIN);
        hash.update(&self.0[..]);
        let mut verifier = Zeroizing::new([0; SECRET_BYTES]);
        verifier.copy_from_slice(&hash.finalize());
        SecretVerifier(verifier)
    }
}

impl fmt::Debug for WebSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WebSecret([REDACTED])")
    }
}

/// An explicitly exposed text allocation, wiped on drop. Borrow only for the
/// response lifetime; do not copy it into ordinary DTOs, state dumps or logs.
pub struct SecretText(Zeroizing<String>);

impl SecretText {
    /// Takes ownership immediately so the allocation is wiped on drop and never
    /// exposes its contents through this wrapper's diagnostics.
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText([REDACTED])")
    }
}

/// The server may keep this hash instead of a bearer secret. No ordinary
/// PartialEq/Eq implementation: all candidate verification takes the same
/// fixed-length constant-time comparison path.
#[derive(Clone)]
pub struct SecretVerifier(Zeroizing<[u8; SECRET_BYTES]>);

impl SecretVerifier {
    pub fn from_bytes(bytes: [u8; SECRET_BYTES]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; SECRET_BYTES] {
        &self.0
    }

    pub fn verify(&self, candidate: &WebSecret) -> bool {
        let candidate = candidate.verifier();
        bool::from(self.0[..].ct_eq(&candidate.0[..]))
    }
}

impl fmt::Debug for SecretVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretVerifier([REDACTED])")
    }
}

/// Short-lived, in-memory eight-digit pairing code. Callers must enforce a
/// bounded lifetime and attempt limit; its entropy does not permit offline use.
pub struct PairingCode(Zeroizing<[u8; CODE_BYTES]>);

impl PairingCode {
    pub fn generate<R: RandomPort>(random: &mut R) -> Result<Self, WebSecretError> {
        let mut bytes = Zeroizing::new([0; 4]);
        for _ in 0..RANDOM_ATTEMPTS {
            random
                .fill(&mut bytes[..])
                .map_err(|_| WebSecretError::RandomUnavailable)?;
            let mut value = Zeroizing::new(u32::from_be_bytes(*bytes));
            if *value == 0 || *value > CODE_RANDOM_MAX {
                continue;
            }
            *value %= CODE_SPACE;
            let mut digits = Zeroizing::new([b'0'; CODE_BYTES]);
            for digit in digits.iter_mut().rev() {
                *digit += (*value % 10) as u8;
                *value /= 10;
            }
            return Ok(Self(digits));
        }
        Err(WebSecretError::RandomUnavailable)
    }

    /// Consumes and wipes the input, including on parse failure. Leading zeros
    /// are significant and valid; whitespace and non-ASCII digits are invalid.
    pub fn parse(input: String) -> Result<Self, WebSecretError> {
        let input = Zeroizing::new(input);
        if input.len() != CODE_BYTES || !input.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(WebSecretError::InvalidPairingCode);
        }
        let mut digits = Zeroizing::new([0; CODE_BYTES]);
        digits.copy_from_slice(input.as_bytes());
        Ok(Self(digits))
    }

    pub fn expose_text(&self) -> SecretText {
        let mut text = Zeroizing::new(String::with_capacity(CODE_BYTES));
        for byte in self.0.iter() {
            text.push(char::from(*byte));
        }
        SecretText(text)
    }

    pub fn matches(&self, candidate: &Self) -> bool {
        bool::from(self.0[..].ct_eq(&candidate.0[..]))
    }
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingCode([REDACTED])")
    }
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SequenceRandom {
        values: Vec<Vec<u8>>,
        calls: usize,
    }

    impl SequenceRandom {
        fn new(values: Vec<Vec<u8>>) -> Self {
            Self { values, calls: 0 }
        }
    }

    impl RandomPort for SequenceRandom {
        type Error = ();

        fn fill(&mut self, output: &mut [u8]) -> Result<(), ()> {
            let value = self.values.get(self.calls).ok_or(())?;
            self.calls += 1;
            if value.len() != output.len() {
                return Err(());
            }
            output.copy_from_slice(value);
            Ok(())
        }
    }

    struct ConstantRandom {
        value: u8,
        calls: usize,
    }

    impl RandomPort for ConstantRandom {
        type Error = ();

        fn fill(&mut self, output: &mut [u8]) -> Result<(), ()> {
            self.calls += 1;
            output.fill(self.value);
            Ok(())
        }
    }

    const SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn independent_sha256_vector_and_verifier_round_trip() {
        // Independently computed using Python hashlib.sha256(domain+bytes(range(32))).
        let expected = [
            0x22, 0x99, 0x18, 0x5b, 0xe4, 0x1d, 0xed, 0x32, 0x9f, 0x86, 0x69, 0x4c, 0xf4, 0xa0,
            0x88, 0x49, 0x26, 0xe6, 0x5d, 0x84, 0xbb, 0xab, 0x55, 0xba, 0xb9, 0xa5, 0x4d, 0x16,
            0xc7, 0x52, 0x41, 0x59,
        ];
        let secret = WebSecret::parse_hex(SECRET.to_owned()).unwrap();
        assert_eq!(secret.expose_hex().as_str(), SECRET);
        assert_eq!(secret.verifier().as_bytes(), &expected);
        let restored = SecretVerifier::from_bytes(expected);
        assert!(restored.verify(&secret));
        assert!(restored.clone().verify(&secret));
    }

    #[test]
    fn canonical_secret_validation_and_redaction() {
        for invalid in [
            String::new(),
            "0".repeat(64),
            "1".repeat(63),
            "1".repeat(65),
            SECRET.to_uppercase(),
            format!(" {SECRET}"),
            "é".repeat(32),
            format!("g{}", &SECRET[1..]),
        ] {
            assert!(matches!(
                WebSecret::parse_hex(invalid),
                Err(WebSecretError::InvalidSecret)
            ));
        }
        let secret = WebSecret::parse_hex(SECRET.to_owned()).unwrap();
        assert_eq!(format!("{secret:?}"), "WebSecret([REDACTED])");
        assert_eq!(
            format!("{:?}", secret.expose_hex()),
            "SecretText([REDACTED])"
        );
        assert_eq!(
            format!("{:?}", secret.verifier()),
            "SecretVerifier([REDACTED])"
        );
    }

    #[test]
    fn owned_text_constructor_redacts_diagnostics() {
        let text = SecretText::new("synthetic-sensitive-header".to_owned());
        assert_eq!(text.as_str(), "synthetic-sensitive-header");
        assert_eq!(format!("{text:?}"), "SecretText([REDACTED])");
    }

    #[test]
    fn secret_generation_has_bounded_zero_rejection_and_propagates_failure() {
        let mut random = SequenceRandom::new(vec![vec![0; 32], (0..32).collect()]);
        let secret = WebSecret::generate(&mut random).unwrap();
        assert_eq!(secret.expose_hex().as_str(), SECRET);
        assert_eq!(random.calls, 2);
        let mut zeros = ConstantRandom { value: 0, calls: 0 };
        assert!(matches!(
            WebSecret::generate(&mut zeros),
            Err(WebSecretError::RandomUnavailable)
        ));
        assert_eq!(zeros.calls, RANDOM_ATTEMPTS);
        assert!(matches!(
            WebSecret::generate(&mut SequenceRandom::new(Vec::new())),
            Err(WebSecretError::RandomUnavailable)
        ));
    }

    #[test]
    fn pairing_sampling_rejects_zero_and_out_of_range_without_modulo_bias() {
        for (sample, expected) in [
            (1, "00000001"),
            (99_999_999, "99999999"),
            (100_000_000, "00000000"),
            (CODE_RANDOM_MAX, "00000000"),
        ] {
            let mut random = SequenceRandom::new(vec![
                0_u32.to_be_bytes().to_vec(),
                (CODE_RANDOM_MAX + 1).to_be_bytes().to_vec(),
                sample.to_be_bytes().to_vec(),
            ]);
            assert_eq!(
                PairingCode::generate(&mut random)
                    .unwrap()
                    .expose_text()
                    .as_str(),
                expected
            );
            assert_eq!(random.calls, 3);
        }
        for value in [0, 255] {
            let mut random = ConstantRandom { value, calls: 0 };
            assert!(matches!(
                PairingCode::generate(&mut random),
                Err(WebSecretError::RandomUnavailable)
            ));
            assert_eq!(random.calls, RANDOM_ATTEMPTS);
        }
        assert!(matches!(
            PairingCode::generate(&mut SequenceRandom::new(Vec::new())),
            Err(WebSecretError::RandomUnavailable)
        ));
    }

    #[test]
    fn pairing_canonical_validation_and_redaction() {
        for invalid in [
            "",
            "1234567",
            "123456789",
            " 1234567",
            "1234567\n",
            "１２３４",
        ] {
            assert!(matches!(
                PairingCode::parse(invalid.to_owned()),
                Err(WebSecretError::InvalidPairingCode)
            ));
        }
        let code = PairingCode::parse("00000000".to_owned()).unwrap();
        assert_eq!(code.expose_text().as_str(), "00000000");
        assert_eq!(format!("{code:?}"), "PairingCode([REDACTED])");
        assert_eq!(
            format!("{:?}", code.expose_text()),
            "SecretText([REDACTED])"
        );
    }

    #[test]
    fn fixed_length_comparisons_reject_first_middle_and_last_mismatches() {
        let secret = WebSecret::parse_hex(SECRET.to_owned()).unwrap();
        let verifier = secret.verifier();
        for index in [0, 32, 63] {
            let mut changed = SECRET.as_bytes().to_vec();
            changed[index] = if changed[index] == b'1' { b'2' } else { b'1' };
            let candidate = WebSecret::parse_hex(String::from_utf8(changed).unwrap()).unwrap();
            assert!(!verifier.verify(&candidate));
        }
        let code = PairingCode::parse("12345678".to_owned()).unwrap();
        assert!(code.matches(&PairingCode::parse("12345678".to_owned()).unwrap()));
        for candidate in ["22345678", "12355678", "12345679"] {
            assert!(!code.matches(&PairingCode::parse(candidate.to_owned()).unwrap()));
        }
    }
}
