//! Package identity in a CA observation, never an operation role or permit.
//! Native callers must obtain these facts from the borrowed session's active DB.
use super::{LifecycleError, Nonce, Result};
use crate::installer_lifecycle::{broker::manifest, INSTALL_FAMILY};

pub(crate) const INFO_PREFIX: &str = "FCGI2:";
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedPackage {
    product: String,
    package: String,
    version: String,
}
impl ObservedPackage {
    pub(crate) fn new(product: &str, package: &str, version: &str) -> Result<Self> {
        let guid = |s: &str| manifest::uuid(s) && s.bytes().any(|b| b != b'0' && b != b'-');
        let mut parts = version.split('.');
        let valid_version = [255u32, 255, 65535].into_iter().all(|max| {
            parts.next().is_some_and(|p| {
                !p.is_empty()
                    && p.len() <= 5
                    && (p.len() == 1 || !p.starts_with('0'))
                    && p.bytes().all(|b| b.is_ascii_digit())
                    && p.parse::<u32>().is_ok_and(|n| n <= max)
            })
        }) && parts.next().is_none();
        if !guid(product) || !guid(package) || !valid_version {
            return Err(LifecycleError::Invalid);
        }
        Ok(Self {
            product: product.into(),
            package: package.into(),
            version: version.into(),
        })
    }
    pub(crate) fn info(&self, nonce: Nonce) -> String {
        format!(
            "{INFO_PREFIX}{}:{}:{}:{INSTALL_FAMILY}:{}:2",
            nonce.hex(),
            self.product,
            self.package,
            self.version
        )
    }
    // This parses observations only. Future private Plan matching supplies roles.
    pub(crate) fn parse_info(value: &str) -> Result<(Self, Nonce)> {
        if value.len() > 170 || !value.is_ascii() {
            return Err(LifecycleError::Invalid);
        }
        let mut parts = value
            .strip_prefix(INFO_PREFIX)
            .ok_or(LifecycleError::Invalid)?
            .split(':');
        let hex = parts.next().ok_or(LifecycleError::Invalid)?;
        let nonce = Nonce::parse_info(&format!("FCGI1:{hex}"))?;
        let product = parts.next().ok_or(LifecycleError::Invalid)?;
        let package = parts.next().ok_or(LifecycleError::Invalid)?;
        let family = parts.next().ok_or(LifecycleError::Invalid)?;
        let version = parts.next().ok_or(LifecycleError::Invalid)?;
        if family != INSTALL_FAMILY || parts.next() != Some("2") || parts.next().is_some() {
            return Err(LifecycleError::Invalid);
        }
        Ok((Self::new(product, package, version)?, nonce))
    }
}
// Identity bytes are format values. Only the private B/G flow constructs a
// Fresh association after native admission; parsing this body grants nothing.
pub(crate) const FRAME_BYTES: usize = 160;
const IDENTITY_BYTES: usize = 116;
impl ObservedPackage {
    fn encode_identity(&self) -> [u8; IDENTITY_BYTES] {
        let mut b = [0; IDENTITY_BYTES];
        b[..36].copy_from_slice(self.product.as_bytes());
        b[36..72].copy_from_slice(self.package.as_bytes());
        b[72..108].copy_from_slice(INSTALL_FAMILY.as_bytes());
        let parts: Vec<u32> = self
            .version
            .split('.')
            .map(|s| s.parse().unwrap())
            .collect();
        b[108] = parts[0] as u8;
        b[109] = parts[1] as u8;
        b[110..112].copy_from_slice(&(parts[2] as u16).to_le_bytes());
        b[112..114].copy_from_slice(&2u16.to_le_bytes());
        b
    }
    fn parse_identity(b: &[u8]) -> Result<Self> {
        if b.len() != IDENTITY_BYTES
            || &b[72..108] != INSTALL_FAMILY.as_bytes()
            || b[112..114] != 2u16.to_le_bytes()
            || b[114..116] != [0; 2]
        {
            return Err(LifecycleError::Invalid);
        }
        fn ascii(s: &[u8]) -> Result<&str> {
            std::str::from_utf8(s).map_err(|_| LifecycleError::Invalid)
        }
        let version = format!(
            "{}.{}.{}",
            b[108],
            b[109],
            u16::from_le_bytes(b[110..112].try_into().unwrap())
        );
        Self::new(ascii(&b[..36])?, ascii(&b[36..72])?, &version)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Grant {
    package: ObservedPackage,
    nonce: Nonce,
    deadline: u64,
    transaction: u32,
}
impl Grant {
    pub(crate) fn new(
        package: ObservedPackage,
        nonce: Nonce,
        deadline: u64,
        transaction: u32,
    ) -> Result<Self> {
        if deadline == 0 || transaction == 0 {
            return Err(LifecycleError::Invalid);
        }
        Ok(Self {
            package,
            nonce,
            deadline,
            transaction,
        })
    }
    pub(crate) fn deadline_ticks(&self) -> u64 {
        self.deadline
    }
    pub(crate) fn encode(&self) -> [u8; FRAME_BYTES] {
        let mut b = [0; FRAME_BYTES];
        b[..4].copy_from_slice(b"FCG2");
        b[4] = 2;
        b[5] = 1;
        b[8..24].copy_from_slice(&self.nonce.0);
        b[24..32].copy_from_slice(&self.deadline.to_le_bytes());
        b[32..36].copy_from_slice(&self.transaction.to_le_bytes());
        b[40..156].copy_from_slice(&self.package.encode_identity());
        b
    }
    pub(crate) fn parse(b: &[u8]) -> Result<Self> {
        if b.len() != FRAME_BYTES
            || &b[..4] != b"FCG2"
            || b[4] != 2
            || b[5] != 1
            || b[6..8] != [0; 2]
            || b[36..40] != [0; 4]
            || b[156..160] != [0; 4]
        {
            return Err(LifecycleError::Invalid);
        }
        Self::new(
            ObservedPackage::parse_identity(&b[40..156])?,
            Nonce::from_bytes(b[8..24].try_into().unwrap())?,
            u64::from_le_bytes(b[24..32].try_into().unwrap()),
            u32::from_le_bytes(b[32..36].try_into().unwrap()),
        )
    }
    pub(crate) fn validate(&self, package: &ObservedPackage, nonce: Nonce, now: u64) -> Result<()> {
        if &self.package != package
            || self.nonce != nonce
            || self.deadline <= now
            || self.deadline - now > super::MAX_GRANT_MILLIS
        {
            return Err(LifecycleError::Invalid);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bridge {
    package: ObservedPackage,
    kind: super::BridgeKind,
    transaction: u32,
    nonce: Nonce,
}
impl Bridge {
    pub(crate) fn new(
        kind: super::BridgeKind,
        transaction: u32,
        nonce: Nonce,
        package: ObservedPackage,
    ) -> Result<Self> {
        if transaction == 0 {
            return Err(LifecycleError::Invalid);
        }
        Ok(Self {
            package,
            kind,
            transaction,
            nonce,
        })
    }
    pub(crate) fn kind(&self) -> super::BridgeKind {
        self.kind
    }
    pub(crate) fn transaction_id(&self) -> u32 {
        self.transaction
    }
    pub(crate) fn nonce(&self) -> Nonce {
        self.nonce
    }
    pub(crate) fn package(&self) -> &ObservedPackage {
        &self.package
    }
    pub(crate) fn encode(&self) -> [u8; FRAME_BYTES] {
        let mut b = [0; FRAME_BYTES];
        b[..4].copy_from_slice(b"FCB2");
        b[4] = 2;
        b[5] = self.kind.byte();
        b[8..12].copy_from_slice(&self.transaction.to_le_bytes());
        b[12..28].copy_from_slice(&self.nonce.0);
        b[32..148].copy_from_slice(&self.package.encode_identity());
        b
    }
    pub(crate) fn parse(b: &[u8]) -> Result<Self> {
        if b.len() != FRAME_BYTES
            || &b[..4] != b"FCB2"
            || b[4] != 2
            || b[6..8] != [0; 2]
            || b[28..32] != [0; 4]
            || b[148..160] != [0; 12]
        {
            return Err(LifecycleError::Invalid);
        }
        let kind = match b[5] {
            1 => super::BridgeKind::Request,
            2 => super::BridgeKind::Ack,
            3 => super::BridgeKind::Returned,
            _ => return Err(LifecycleError::Invalid),
        };
        Self::new(
            kind,
            u32::from_le_bytes(b[8..12].try_into().unwrap()),
            Nonce::from_bytes(b[12..28].try_into().unwrap())?,
            ObservedPackage::parse_identity(&b[32..148])?,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const PRODUCT: &str = "99378807-098D-5F04-8D4D-F181DFA218D6";
    const PACKAGE: &str = "99378807-098D-5F04-8D4D-F181DFA218D7";
    #[test]
    fn v2_observation_roundtrip_and_legacy_rejection() {
        let p = ObservedPackage::new(PRODUCT, PACKAGE, "1.2.3").unwrap();
        let n = Nonce::from_bytes([1; 16]).unwrap();
        let text = p.info(n);
        assert_eq!(ObservedPackage::parse_info(&text).unwrap(), (p, n));
        assert!(Nonce::parse_info(&text).is_err());
        assert!(ObservedPackage::parse_info(&n.info()).is_err());
    }
    #[test]
    fn v2_observation_rejects_role_tail_family_abi_and_noncanonical_identity() {
        let text = ObservedPackage::new(PRODUCT, PACKAGE, "1.2.3")
            .unwrap()
            .info(Nonce::from_bytes([1; 16]).unwrap());
        for bad in [
            format!("{text}:old"),
            text.replace(INSTALL_FAMILY, PRODUCT),
            text.strip_suffix('2').unwrap().to_owned() + "1",
            text.replace(":1.2.3:", ":01.2.3:"),
            text.replace(PACKAGE, "00000000-0000-0000-0000-000000000000"),
        ] {
            assert!(ObservedPackage::parse_info(&bad).is_err());
        }
    }
    #[test]
    fn v2_ticket_and_bridge_roundtrip_never_accept_legacy_or_other_package() {
        let p = ObservedPackage::new(PRODUCT, PACKAGE, "255.255.65535").unwrap();
        let n = Nonce::from_bytes([7; 16]).unwrap();
        let grant = Grant::new(p.clone(), n, 2000, 42).unwrap();
        assert_eq!(Grant::parse(&grant.encode()).unwrap(), grant);
        grant.validate(&p, n, 1000).unwrap();
        assert!(super::super::Grant::parse(&grant.encode()).is_err());
        assert!(Grant::parse(&super::super::Grant::new(n, 2000, 42).unwrap().encode()).is_err());
        let other = ObservedPackage::new(PACKAGE, PRODUCT, "255.255.65535").unwrap();
        assert!(grant.validate(&other, n, 1000).is_err());
        assert!(grant
            .validate(&p, Nonce::from_bytes([8; 16]).unwrap(), 1000)
            .is_err());
        assert!(grant.validate(&p, n, 2000).is_err());
        let distant = Grant::new(p.clone(), n, 121001, 42).unwrap();
        assert!(distant.validate(&p, n, 1000).is_err());
        for kind in [
            super::super::BridgeKind::Request,
            super::super::BridgeKind::Ack,
            super::super::BridgeKind::Returned,
        ] {
            let bridge = Bridge::new(kind, 42, n, p.clone()).unwrap();
            assert_eq!(Bridge::parse(&bridge.encode()).unwrap(), bridge);
            assert!(super::super::Bridge::parse(&bridge.encode()).is_err());
        }
    }
    #[test]
    fn v2_ticket_exact_headers_identity_and_reserved_regions_are_closed() {
        let p = ObservedPackage::new(PRODUCT, PACKAGE, "1.2.3").unwrap();
        let n = Nonce::from_bytes([7; 16]).unwrap();
        let grant = Grant::new(p.clone(), n, 2000, 42).unwrap().encode();
        for at in [
            0, 4, 5, 6, 7, 36, 37, 38, 39, 112, 152, 153, 154, 155, 156, 157, 158, 159,
        ] {
            let mut bad = grant;
            bad[at] ^= 0x80;
            assert!(Grant::parse(&bad).is_err(), "grant {at}");
        }
        let bridge = Bridge::new(super::super::BridgeKind::Request, 42, n, p)
            .unwrap()
            .encode();
        for at in [
            0, 4, 5, 6, 7, 28, 29, 30, 31, 104, 144, 145, 146, 147, 148, 159,
        ] {
            let mut bad = bridge;
            bad[at] ^= 0x80;
            assert!(Bridge::parse(&bad).is_err(), "bridge {at}");
        }
        for length in 0..FRAME_BYTES {
            assert!(Grant::parse(&grant[..length]).is_err());
            assert!(Bridge::parse(&bridge[..length]).is_err());
        }
    }
}
