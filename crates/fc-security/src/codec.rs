//! Fixed-size authenticated record envelope. The independent installation MAC key
//! must be DPAPI-protected by the deployment user; a valid MAC is integrity, not encryption.
//! Database read/write permissions and DPAPI confidentiality remain required.
use crate::{
    PasswordRecord, RandomPort, RecoveryRecord, SecurityError, Zeroizing, PASSWORD_FORMAT,
    PBKDF2_ITERATIONS, RECOVERY_FORMAT,
};
use fc_domain::ids::PrincipalId;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::fmt;

const MAGIC: &[u8; 8] = b"FCCRED01";
const KEY_BYTES: usize = 32;
const PAYLOAD_BYTES: usize = 97;
pub const CREDENTIAL_RECORD_BYTES: usize = PAYLOAD_BYTES + 32;
const PASSWORD_KIND: u8 = 1;
const RECOVERY_KIND: u8 = 2;

/// Owned codec output is wiped unless explicitly handed into the protected store.
pub struct EncodedCredential(Zeroizing<Vec<u8>>);
impl EncodedCredential {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Ownership handoff: the storage adapter must wipe temporary copies when done.
    pub fn into_storage_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut *self.0)
    }
}
impl fmt::Debug for EncodedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EncodedCredential([REDACTED])")
    }
}
pub struct CredentialRecordCodec {
    key: Zeroizing<[u8; KEY_BYTES]>,
}
impl fmt::Debug for CredentialRecordCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CredentialRecordCodec([REDACTED])")
    }
}
impl CredentialRecordCodec {
    pub fn from_key(key: [u8; KEY_BYTES]) -> Result<Self, SecurityError> {
        let key = Zeroizing::new(key);
        if key.iter().all(|b| *b == 0) {
            return Err(SecurityError::InvalidCodecKey);
        }
        Ok(Self { key })
    }
    pub fn generate<R: RandomPort>(random: &mut R) -> Result<Self, SecurityError> {
        let mut key = Zeroizing::new([0u8; KEY_BYTES]);
        random
            .fill(&mut key[..])
            .map_err(|_| SecurityError::RandomUnavailable)?;
        Self::from_key(*key)
    }
    /// The only key export path is an explicit callback for the platform protector.
    /// Do not log/copy the bytes or put them in an ordinary application snapshot.
    pub fn protect_key<T>(&self, protect: impl FnOnce(&[u8]) -> T) -> T {
        protect(&self.key[..])
    }
    fn encode(
        &self,
        kind: u8,
        id: PrincipalId,
        version: u64,
        parameters: (u32, u32),
        salt: &[u8; 32],
        verifier: &[u8; 32],
    ) -> Result<EncodedCredential, SecurityError> {
        if version == 0 {
            return Err(SecurityError::InvalidBinding);
        }
        let mut bytes = Zeroizing::new(Vec::with_capacity(CREDENTIAL_RECORD_BYTES));
        bytes.extend_from_slice(MAGIC);
        bytes.push(kind);
        bytes.extend_from_slice(&parameters.0.to_le_bytes());
        bytes.extend_from_slice(&parameters.1.to_le_bytes());
        bytes.extend_from_slice(&id.get().to_le_bytes());
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(salt);
        bytes.extend_from_slice(verifier);
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key[..])
            .map_err(|_| SecurityError::InvalidCodecKey)?;
        mac.update(&bytes);
        let tag = mac.finalize().into_bytes();
        bytes.extend_from_slice(&tag);
        Ok(EncodedCredential(bytes))
    }
    fn payload<'a>(
        &self,
        kind: u8,
        id: PrincipalId,
        version: u64,
        bytes: &'a [u8],
    ) -> Result<&'a [u8], SecurityError> {
        if bytes.len() != CREDENTIAL_RECORD_BYTES || version == 0 {
            return Err(SecurityError::CorruptCredential);
        }
        let (payload, tag) = bytes.split_at(PAYLOAD_BYTES);
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key[..])
            .map_err(|_| SecurityError::InvalidCodecKey)?;
        mac.update(payload);
        mac.verify_slice(tag)
            .map_err(|_| SecurityError::CorruptCredential)?;
        if &payload[..8] != MAGIC || payload[8] != kind {
            return Err(SecurityError::UnsupportedCredential);
        }
        if payload[17..25] != id.get().to_le_bytes() || payload[25..33] != version.to_le_bytes() {
            return Err(SecurityError::InvalidBinding);
        }
        Ok(payload)
    }
    pub fn encode_password(
        &self,
        id: PrincipalId,
        version: u64,
        record: &PasswordRecord,
    ) -> Result<EncodedCredential, SecurityError> {
        if record.format() != PASSWORD_FORMAT || record.iterations() != PBKDF2_ITERATIONS {
            return Err(SecurityError::UnsupportedCredential);
        }
        self.encode(
            PASSWORD_KIND,
            id,
            version,
            (record.format(), record.iterations()),
            record.salt(),
            record.verifier(),
        )
    }
    pub fn decode_password(
        &self,
        id: PrincipalId,
        version: u64,
        bytes: &[u8],
    ) -> Result<PasswordRecord, SecurityError> {
        let p = self.payload(PASSWORD_KIND, id, version, bytes)?;
        PasswordRecord::from_parts(
            u32_at(p, 9)?,
            u32_at(p, 13)?,
            array_at(p, 33)?,
            array_at(p, 65)?,
        )
    }
    pub fn encode_recovery(
        &self,
        id: PrincipalId,
        version: u64,
        record: &RecoveryRecord,
    ) -> Result<EncodedCredential, SecurityError> {
        if record.format != RECOVERY_FORMAT {
            return Err(SecurityError::UnsupportedCredential);
        }
        self.encode(
            RECOVERY_KIND,
            id,
            version,
            (record.format, 0),
            &record.salt,
            &record.verifier,
        )
    }
    pub fn decode_recovery(
        &self,
        id: PrincipalId,
        version: u64,
        bytes: &[u8],
    ) -> Result<RecoveryRecord, SecurityError> {
        let p = self.payload(RECOVERY_KIND, id, version, bytes)?;
        if u32_at(p, 13)? != 0 {
            return Err(SecurityError::UnsupportedCredential);
        }
        RecoveryRecord::from_parts(u32_at(p, 9)?, array_at(p, 33)?, array_at(p, 65)?)
    }
}
fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], SecurityError> {
    let end = offset
        .checked_add(N)
        .ok_or(SecurityError::CorruptCredential)?;
    bytes
        .get(offset..end)
        .ok_or(SecurityError::CorruptCredential)?
        .try_into()
        .map_err(|_| SecurityError::CorruptCredential)
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, SecurityError> {
    Ok(u32::from_le_bytes(array_at(bytes, offset)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id() -> PrincipalId {
        PrincipalId::new(7).unwrap()
    }
    fn password() -> PasswordRecord {
        PasswordRecord::from_parts(PASSWORD_FORMAT, PBKDF2_ITERATIONS, [2; 32], [3; 32]).unwrap()
    }
    #[test]
    fn fixed_record_round_trip_and_independent_hmac_vector() {
        let codec = CredentialRecordCodec::from_key([5; 32]).unwrap();
        let record = password();
        let encoded = codec.encode_password(id(), 9, &record).unwrap();
        assert_eq!(encoded.as_bytes().len(), CREDENTIAL_RECORD_BYTES);
        let expected = [
            0x7a, 0xed, 0x5c, 0x5d, 0x91, 0xbc, 0xf3, 0x1f, 0x82, 0xf1, 0x63, 0x06, 0xec, 0xfa,
            0xa3, 0xf4, 0xd1, 0x54, 0x4b, 0xe0, 0xeb, 0xed, 0x5d, 0x0c, 0xb5, 0x95, 0xd9, 0xcf,
            0xed, 0x7b, 0xea, 0x7e,
        ];
        assert_eq!(&encoded.as_bytes()[PAYLOAD_BYTES..], expected);
        let decoded = codec.decode_password(id(), 9, encoded.as_bytes()).unwrap();
        assert_eq!(decoded.salt(), record.salt());
        assert_eq!(decoded.verifier(), record.verifier());
        assert!(!format!("{codec:?} {encoded:?}").contains("[5, 5"));
        let transferred = encoded.into_storage_bytes();
        assert_eq!(transferred.len(), CREDENTIAL_RECORD_BYTES);
    }
    #[test]
    fn each_modified_byte_truncation_extension_wrong_key_kind_and_binding_are_rejected() {
        let codec = CredentialRecordCodec::from_key([5; 32]).unwrap();
        let encoded = codec.encode_password(id(), 9, &password()).unwrap();
        for index in 0..CREDENTIAL_RECORD_BYTES {
            let mut corrupt = encoded.as_bytes().to_vec();
            corrupt[index] ^= 1;
            assert!(
                codec.decode_password(id(), 9, &corrupt).is_err(),
                "byte {index}"
            );
        }
        for length in 0..CREDENTIAL_RECORD_BYTES {
            assert!(codec
                .decode_password(id(), 9, &encoded.as_bytes()[..length])
                .is_err());
        }
        let mut extended = encoded.as_bytes().to_vec();
        extended.push(0);
        assert!(codec.decode_password(id(), 9, &extended).is_err());
        assert!(CredentialRecordCodec::from_key([6; 32])
            .unwrap()
            .decode_password(id(), 9, encoded.as_bytes())
            .is_err());
        assert!(codec
            .decode_password(PrincipalId::new(8).unwrap(), 9, encoded.as_bytes())
            .is_err());
        assert!(codec.decode_password(id(), 8, encoded.as_bytes()).is_err());
        assert!(codec.decode_recovery(id(), 9, encoded.as_bytes()).is_err());
        assert!(codec.encode_password(id(), 0, &password()).is_err());
    }
    #[test]
    fn authenticated_unsupported_formats_and_weak_work_factors_are_rejected_before_kdf() {
        let codec = CredentialRecordCodec::from_key([5; 32]).unwrap();
        for (format, iterations) in [(2, PBKDF2_ITERATIONS), (1, 1), (1, u32::MAX)] {
            let encoded = codec
                .encode(
                    PASSWORD_KIND,
                    id(),
                    9,
                    (format, iterations),
                    &[2; 32],
                    &[3; 32],
                )
                .unwrap();
            assert_eq!(
                codec
                    .decode_password(id(), 9, encoded.as_bytes())
                    .unwrap_err(),
                SecurityError::UnsupportedCredential
            );
        }
        let encoded = codec
            .encode(
                RECOVERY_KIND,
                id(),
                9,
                (1, PBKDF2_ITERATIONS),
                &[2; 32],
                &[3; 32],
            )
            .unwrap();
        assert!(codec.decode_recovery(id(), 9, encoded.as_bytes()).is_err());
        assert!(CredentialRecordCodec::from_key([0; 32]).is_err());
    }
    #[test]
    fn recovery_record_codec_keeps_only_verifier_and_rejects_cross_account_copy() {
        let codec = CredentialRecordCodec::from_key([5; 32]).unwrap();
        let record = RecoveryRecord::from_parts(RECOVERY_FORMAT, [2; 32], [3; 32]).unwrap();
        let encoded = codec.encode_recovery(id(), 9, &record).unwrap();
        let decoded = codec.decode_recovery(id(), 9, encoded.as_bytes()).unwrap();
        assert_eq!(&decoded.verifier[..], &record.verifier[..]);
        assert!(codec
            .decode_recovery(PrincipalId::new(8).unwrap(), 9, encoded.as_bytes())
            .is_err());
    }
}
