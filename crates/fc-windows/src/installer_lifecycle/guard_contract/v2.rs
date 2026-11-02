//! Package identity in a CA observation, never an operation role or permit.
//! Native callers must obtain these facts from the borrowed session's active DB.
use super::{LifecycleError, Nonce, Result};
use crate::installer_lifecycle::{broker::manifest, INSTALL_FAMILY};

pub(crate) const INFO_PREFIX: &str = "FCGI2:";
#[derive(Debug, PartialEq, Eq)]
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
}
