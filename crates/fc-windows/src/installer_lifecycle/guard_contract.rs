//! Closed, pure byte contracts for the original controlled guard association.
//! The CA creates a fresh nonzero nonce using SystemRandom before FIRST_INSTANCE
//! pipe creation. Parsing alone proves neither entropy nor native identity.
use super::{LifecycleError, Result};

pub(crate) mod v2;

pub(crate) const INFO_PREFIX: &str = "FCGI1:";
pub(crate) const PIPE_PREFIX: &str = r"\\.\pipe\FutureClassroom-ControlledGuard-";
pub(crate) const GUARD_TEMPLATE_QUERY: &str =
    "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcGuardClientTemplate'";
pub(crate) const GRANT_BYTES: usize = 48;
pub(crate) const BRIDGE_BYTES: usize = 32;
pub(crate) const MAX_GRANT_MILLIS: u64 = 120_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Nonce([u8; 16]);
impl Nonce {
    pub(crate) fn from_bytes(bytes: [u8; 16]) -> Result<Self> {
        if bytes == [0; 16] {
            return Err(LifecycleError::Invalid);
        }
        Ok(Self(bytes))
    }
    pub(crate) fn parse_info(info: &str) -> Result<Self> {
        let hex = info
            .strip_prefix(INFO_PREFIX)
            .filter(|s| s.len() == 32)
            .ok_or(LifecycleError::Invalid)?;
        fn nibble(b: u8) -> Result<u8> {
            match b {
                b'0'..=b'9' => Ok(b - b'0'),
                b'a'..=b'f' => Ok(b - b'a' + 10),
                _ => Err(LifecycleError::Invalid),
            }
        }
        let mut bytes = [0; 16];
        for (i, pair) in hex.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            bytes[i] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Self::from_bytes(bytes)
    }
    fn hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut result = String::with_capacity(32);
        for byte in self.0 {
            result.push(char::from(HEX[usize::from(byte >> 4)]));
            result.push(char::from(HEX[usize::from(byte & 15)]));
        }
        result
    }
    pub(crate) fn info(self) -> String {
        format!("{INFO_PREFIX}{}", self.hex())
    }
    pub(crate) fn pipe_name(self) -> String {
        format!("{PIPE_PREFIX}{}", self.hex())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Grant {
    nonce: Nonce,
    deadline_ticks: u64,
    transaction_id: u32,
}
impl Grant {
    pub(crate) fn new(nonce: Nonce, deadline_ticks: u64, transaction_id: u32) -> Result<Self> {
        if deadline_ticks == 0 || transaction_id == 0 {
            return Err(LifecycleError::Invalid);
        }
        Ok(Self {
            nonce,
            deadline_ticks,
            transaction_id,
        })
    }
    pub(crate) fn deadline_ticks(self) -> u64 {
        self.deadline_ticks
    }
    pub(crate) fn encode(self) -> [u8; GRANT_BYTES] {
        let mut b = [0; GRANT_BYTES];
        b[..4].copy_from_slice(b"FCGA");
        b[4] = 1;
        b[5] = 1;
        b[8..24].copy_from_slice(&self.nonce.0);
        b[24..32].copy_from_slice(&self.deadline_ticks.to_le_bytes());
        b[32..36].copy_from_slice(&self.transaction_id.to_le_bytes());
        b
    }
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let b: &[u8; GRANT_BYTES] = bytes.try_into().map_err(|_| LifecycleError::Invalid)?;
        if &b[..4] != b"FCGA" || b[4] != 1 || b[5] != 1 || b[6..8] != [0; 2] || b[36..48] != [0; 12]
        {
            return Err(LifecycleError::Invalid);
        }
        Self::new(
            Nonce::from_bytes(b[8..24].try_into().unwrap())?,
            u64::from_le_bytes(b[24..32].try_into().unwrap()),
            u32::from_le_bytes(b[32..36].try_into().unwrap()),
        )
    }
    pub(crate) fn validate(self, expected_nonce: Nonce, now_ticks: u64) -> Result<()> {
        if self.nonce != expected_nonce
            || self.deadline_ticks <= now_ticks
            || self.deadline_ticks - now_ticks > MAX_GRANT_MILLIS
        {
            return Err(LifecycleError::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BridgeKind {
    Request,
    Ack,
    Returned,
}
impl BridgeKind {
    fn byte(self) -> u8 {
        match self {
            Self::Request => 1,
            Self::Ack => 2,
            Self::Returned => 3,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bridge {
    kind: BridgeKind,
    transaction_id: u32,
    nonce: Nonce,
    code: u32,
}
impl Bridge {
    pub(crate) fn new(
        kind: BridgeKind,
        transaction_id: u32,
        nonce: Nonce,
        code: u32,
    ) -> Result<Self> {
        if transaction_id == 0 || code == u32::MAX || (kind != BridgeKind::Returned && code != 0) {
            return Err(LifecycleError::Invalid);
        }
        Ok(Self {
            kind,
            transaction_id,
            nonce,
            code,
        })
    }
    pub(crate) fn kind(self) -> BridgeKind {
        self.kind
    }
    pub(crate) fn transaction_id(self) -> u32 {
        self.transaction_id
    }
    pub(crate) fn nonce(self) -> Nonce {
        self.nonce
    }
    pub(crate) fn code(self) -> u32 {
        self.code
    }
    pub(crate) fn encode(self) -> [u8; BRIDGE_BYTES] {
        let mut b = [0; BRIDGE_BYTES];
        b[..4].copy_from_slice(b"FCGB");
        b[4] = 1;
        b[5] = self.kind.byte();
        b[8..12].copy_from_slice(&self.transaction_id.to_le_bytes());
        b[12..28].copy_from_slice(&self.nonce.0);
        b[28..32].copy_from_slice(&self.code.to_le_bytes());
        b
    }
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let b: &[u8; BRIDGE_BYTES] = bytes.try_into().map_err(|_| LifecycleError::Invalid)?;
        if &b[..4] != b"FCGB" || b[4] != 1 || b[6..8] != [0; 2] {
            return Err(LifecycleError::Invalid);
        }
        let kind = match b[5] {
            1 => BridgeKind::Request,
            2 => BridgeKind::Ack,
            3 => BridgeKind::Returned,
            _ => return Err(LifecycleError::Invalid),
        };
        Self::new(
            kind,
            u32::from_le_bytes(b[8..12].try_into().unwrap()),
            Nonce::from_bytes(b[12..28].try_into().unwrap())?,
            u32::from_le_bytes(b[28..32].try_into().unwrap()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn info_is_exact_lower_hex_with_no_zero_or_path_authority() {
        let n = Nonce::parse_info("FCGI1:00112233445566778899aabbccddeeff").unwrap();
        assert_eq!(
            n.0,
            [0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255]
        );
        assert_eq!(n.info(), "FCGI1:00112233445566778899aabbccddeeff");
        assert_eq!(
            n.pipe_name(),
            format!("{PIPE_PREFIX}00112233445566778899aabbccddeeff")
        );
        for info in [
            "FCGI1:00000000000000000000000000000000",
            "FCGI1:00112233445566778899AABBCCDDEEFF",
            "FCGI1:00112233445566778899aabbccddeeff ",
            "FCGI1:../pipe",
            "FCGI1:00112233445566778899aabbccddee",
            " FCGI1:00112233445566778899aabbccddeeff",
        ] {
            assert!(Nonce::parse_info(info).is_err());
        }
    }
    #[test]
    fn grant_has_fixed_offsets_and_original_absolute_deadline() {
        let n = Nonce::from_bytes([7; 16]).unwrap();
        let g = Grant::new(n, 0x0102030405060708, 0x11223344).unwrap();
        let bytes = g.encode();
        assert_eq!(&bytes[..8], b"FCGA\x01\x01\0\0");
        assert_eq!(&bytes[8..24], &[7; 16]);
        assert_eq!(
            &bytes[24..36],
            &[8, 7, 6, 5, 4, 3, 2, 1, 0x44, 0x33, 0x22, 0x11]
        );
        assert_eq!(&bytes[36..], &[0; 12]);
        assert_eq!(Grant::parse(&bytes).unwrap(), g);
        for i in [0, 4, 5, 6, 7, 36, 39, 40, 47] {
            let mut bad = bytes;
            bad[i] ^= 0x80;
            assert!(Grant::parse(&bad).is_err());
        }
        for len in [0, 1, 47] {
            assert!(Grant::parse(&bytes[..len]).is_err());
        }
        let mut tail = bytes.to_vec();
        tail.push(0);
        assert!(Grant::parse(&tail).is_err());
        let g = Grant::new(n, 120_001, 1).unwrap();
        assert!(g.validate(n, 1).is_ok());
        assert!(g.validate(n, 0).is_err());
        assert!(g.validate(n, 120_001).is_err());
        assert!(g.validate(Nonce::from_bytes([8; 16]).unwrap(), 1).is_err());
        assert!(Grant::new(n, 0, 1).is_err());
        assert!(Grant::new(n, 1, 0).is_err());
    }
    #[test]
    fn bridge_is_closed_and_failure_return_cannot_be_acknowledgement() {
        let n = Nonce::from_bytes([9; 16]).unwrap();
        let b = Bridge::new(BridgeKind::Returned, 0x11223344, n, 1603).unwrap();
        let raw = b.encode();
        assert_eq!(&raw[..12], b"FCGB\x01\x03\0\0\x44\x33\x22\x11");
        assert_eq!(&raw[12..28], &[9; 16]);
        assert_eq!(&raw[28..], &[0x43, 6, 0, 0]);
        assert_eq!(Bridge::parse(&raw).unwrap(), b);
        for kind in [BridgeKind::Request, BridgeKind::Ack] {
            assert!(Bridge::new(kind, 1, n, 1603).is_err());
            assert!(Bridge::new(kind, 1, n, 0).is_ok());
        }
        assert!(Bridge::new(BridgeKind::Returned, 0, n, 0).is_err());
        assert!(Bridge::new(BridgeKind::Returned, 1, n, u32::MAX).is_err());
        for i in [0, 4, 5, 6, 7] {
            let mut bad = raw;
            bad[i] = 255;
            assert!(Bridge::parse(&bad).is_err());
        }
        let mut bad = raw;
        bad[12..28].fill(0);
        assert!(Bridge::parse(&bad).is_err());
        assert!(Bridge::parse(&raw[..31]).is_err());
        let mut bad = raw.to_vec();
        bad.push(0);
        assert!(Bridge::parse(&bad).is_err());
    }
}
