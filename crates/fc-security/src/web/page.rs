//! Process-bound, signed browser page grants. Only verifiers enter host caches.
use super::{SecretText, SecretVerifier, WebSecret, WebSecretError};
use fc_ports::RandomPort;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

type Hmac256 = Hmac<Sha256>;
pub const PAGE_TOKEN_HEX_BYTES: usize = 128;
pub struct PageKey(WebSecret);
pub struct PageToken(Zeroizing<[u8; 64]>);
impl PageKey {
    pub fn generate<R: RandomPort>(random: &mut R) -> Result<Self, WebSecretError> {
        WebSecret::generate(random).map(Self)
    }
    fn mac(&self, domain: &[u8], input: &[u8]) -> Result<[u8; 32], WebSecretError> {
        let mut mac =
            Hmac256::new_from_slice(&self.0 .0[..]).map_err(|_| WebSecretError::InvalidSecret)?;
        mac.update(domain);
        mac.update(input);
        Ok(mac.finalize().into_bytes().into())
    }
    pub fn issue<R: RandomPort>(
        &self,
        principal: u64,
        epoch: u64,
        random: &mut R,
    ) -> Result<PageToken, WebSecretError> {
        let nonce = WebSecret::generate(random)?;
        let mut bytes = Zeroizing::new([0; 64]);
        bytes[..8].copy_from_slice(&principal.to_le_bytes());
        bytes[8..16].copy_from_slice(&epoch.to_le_bytes());
        bytes[16..32].copy_from_slice(&nonce.0[..16]);
        let mac = self.mac(b"fc-page-grant-v1", &bytes[..32])?;
        bytes[32..].copy_from_slice(&mac);
        Ok(PageToken(bytes))
    }
    pub fn verify(&self, token: &PageToken) -> Result<(u64, u64), WebSecretError> {
        let mut mac =
            Hmac256::new_from_slice(&self.0 .0[..]).map_err(|_| WebSecretError::InvalidSecret)?;
        mac.update(b"fc-page-grant-v1");
        mac.update(&token.0[..32]);
        mac.verify_slice(&token.0[32..])
            .map_err(|_| WebSecretError::InvalidSecret)?;
        Ok((
            u64::from_le_bytes(
                token.0[..8]
                    .try_into()
                    .map_err(|_| WebSecretError::InvalidSecret)?,
            ),
            u64::from_le_bytes(
                token.0[8..16]
                    .try_into()
                    .map_err(|_| WebSecretError::InvalidSecret)?,
            ),
        ))
    }
    /// A stable, independent CSRF token survives cold lease-cache eviction.
    pub fn csrf(&self, token: &PageToken) -> Result<WebSecret, WebSecretError> {
        self.verify(token)?;
        Ok(WebSecret(Zeroizing::new(
            self.mac(b"fc-page-csrf-v1", &token.0[..])?,
        )))
    }
}
impl PageToken {
    pub fn parse_hex(input: String) -> Result<Self, WebSecretError> {
        let input = Zeroizing::new(input);
        if input.len() != PAGE_TOKEN_HEX_BYTES {
            return Err(WebSecretError::InvalidSecret);
        }
        let mut bytes = Zeroizing::new([0; 64]);
        for (byte, pair) in bytes.iter_mut().zip(input.as_bytes().as_chunks::<2>().0) {
            *byte = (super::hex(pair[0]).ok_or(WebSecretError::InvalidSecret)? << 4)
                | super::hex(pair[1]).ok_or(WebSecretError::InvalidSecret)?;
        }
        Ok(Self(bytes))
    }
    pub fn expose_hex(&self) -> SecretText {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::with_capacity(PAGE_TOKEN_HEX_BYTES);
        for byte in self.0.iter() {
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 15)]));
        }
        SecretText::new(text)
    }
    pub fn verifier(&self) -> SecretVerifier {
        let mut hash = Sha256::new();
        hash.update(b"fc-page-cache-v1");
        hash.update(&self.0[..]);
        let mut bytes = Zeroizing::new([0; 32]);
        bytes.copy_from_slice(&hash.finalize());
        SecretVerifier(bytes)
    }
    pub fn matches(&self, verifier: &SecretVerifier) -> bool {
        use subtle::ConstantTimeEq;
        bool::from(self.verifier().0[..].ct_eq(&verifier.0[..]))
    }
}
