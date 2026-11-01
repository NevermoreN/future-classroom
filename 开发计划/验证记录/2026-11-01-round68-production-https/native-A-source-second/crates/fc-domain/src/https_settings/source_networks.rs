//! Explicit permitted peer CIDRs; a network range never grants application authority.
use super::HttpsSettingsError;
use std::net::IpAddr;

pub const MAX_SOURCE_NETWORKS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsSourceNetwork {
    network: IpAddr,
    prefix: u8,
    text: String,
}
impl HttpsSourceNetwork {
    /// Host bits must already be zero. /0, scoped/mapped IPv6, multicast,
    /// unspecified and broadcast ranges are unsupported rather than widened.
    pub fn parse(value: &str) -> Result<Self, HttpsSettingsError> {
        let invalid = || HttpsSettingsError::InvalidSourceNetworks;
        if value.is_empty() || value.len() > 64 || !value.is_ascii() {
            return Err(invalid());
        }
        let (address, prefix) = value.split_once('/').ok_or_else(invalid)?;
        let network = address.parse::<IpAddr>().map_err(|_| invalid())?;
        if prefix.is_empty() || !prefix.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        let prefix = prefix.parse::<u8>().map_err(|_| invalid())?;
        let canonical = match network {
            IpAddr::V4(v) => {
                if prefix == 0
                    || prefix > 32
                    || v.is_unspecified()
                    || v.is_multicast()
                    || v.is_broadcast()
                {
                    return Err(invalid());
                }
                let mask = u32::MAX << (32 - prefix);
                u32::from(v) & !mask == 0
            }
            IpAddr::V6(v) => {
                if prefix == 0
                    || prefix > 128
                    || v.is_unspecified()
                    || v.is_multicast()
                    || v.to_ipv4_mapped().is_some()
                {
                    return Err(invalid());
                }
                let mask = u128::MAX << (128 - prefix);
                u128::from(v) & !mask == 0
            }
        };
        if !canonical {
            return Err(invalid());
        }
        Ok(Self {
            network,
            prefix,
            text: format!("{network}/{prefix}"),
        })
    }
    pub fn as_str(&self) -> &str {
        &self.text
    }
    pub fn contains(&self, peer: IpAddr) -> bool {
        // Never reinterpret mapped IPv6 as IPv4 or accept unusable peers.
        if peer.is_unspecified()
            || peer.is_multicast()
            || matches!(peer, IpAddr::V4(v) if v.is_broadcast())
            || matches!(peer, IpAddr::V6(v) if v.to_ipv4_mapped().is_some())
        {
            return false;
        }
        match (self.network, peer) {
            (IpAddr::V4(network), IpAddr::V4(peer)) => {
                u32::from(network) == u32::from(peer) & (u32::MAX << (32 - self.prefix))
            }
            (IpAddr::V6(network), IpAddr::V6(peer)) => {
                u128::from(network) == u128::from(peer) & (u128::MAX << (128 - self.prefix))
            }
            _ => false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsSourceNetworks(Vec<HttpsSourceNetwork>);
impl HttpsSourceNetworks {
    pub const MAX_NETWORKS: usize = MAX_SOURCE_NETWORKS;
    pub fn new(networks: Vec<HttpsSourceNetwork>) -> Result<Self, HttpsSettingsError> {
        if networks.is_empty()
            || networks.len() > MAX_SOURCE_NETWORKS
            || networks
                .iter()
                .enumerate()
                .any(|(n, v)| networks[..n].contains(v))
        {
            return Err(HttpsSettingsError::InvalidSourceNetworks);
        }
        // Do not retain an arbitrarily over-reserved caller Vec in this
        // bounded pure value; the owned capacity is the validated length.
        Ok(Self(networks.into_boxed_slice().into_vec()))
    }
    /// UI/storage text is bounded before parsing; tokens are comma or ASCII
    /// whitespace separated. No empty input or implicit default is accepted.
    pub fn parse(value: &str) -> Result<Self, HttpsSettingsError> {
        if value.len() > MAX_SOURCE_NETWORKS * 65 || !value.is_ascii() {
            return Err(HttpsSettingsError::InvalidSourceNetworks);
        }
        let mut networks = Vec::new();
        for part in value
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|p| !p.is_empty())
        {
            if networks.len() == MAX_SOURCE_NETWORKS {
                return Err(HttpsSettingsError::InvalidSourceNetworks);
            }
            networks.push(HttpsSourceNetwork::parse(part)?);
        }
        Self::new(networks)
    }
    pub fn networks(&self) -> &[HttpsSourceNetwork] {
        &self.0
    }
    pub fn contains(&self, peer: IpAddr) -> bool {
        self.0.iter().any(|network| network.contains(peer))
    }
    pub fn to_text(&self) -> String {
        self.0
            .iter()
            .map(HttpsSourceNetwork::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn allocation_bytes(&self) -> usize {
        self.0.capacity() * std::mem::size_of::<HttpsSourceNetwork>()
            + self.0.iter().map(|n| n.text.capacity()).sum::<usize>()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_sources_are_bounded_canonical_and_family_specific() {
        let p =
            HttpsSourceNetworks::parse("192.168.40.0/24, 2001:db8:1::/48\n127.0.0.1/32 ::1/128")
                .unwrap();
        for v in ["192.168.40.7", "2001:db8:1::7", "127.0.0.1", "::1"] {
            assert!(p.contains(v.parse().unwrap()));
        }
        for v in [
            "192.168.41.7",
            "2001:db8:2::7",
            "::ffff:192.168.40.7",
            "224.0.0.1",
            "::",
            "ff02::1",
        ] {
            assert!(!p.contains(v.parse().unwrap()));
        }
        assert_eq!(HttpsSourceNetworks::parse(&p.to_text()).unwrap(), p);
        for v in [
            "",
            "0.0.0.0/0",
            "::/0",
            "192.168.40.7/24",
            "2001:db8:1::1/48",
            "::ffff:192.168.40.0/120",
            "fe80::1%2/128",
            "224.0.0.0/4",
            "127.0.0.1/33",
            "::1/129",
            "127.0.0.1/32,127.0.0.1/32",
        ] {
            assert!(HttpsSourceNetworks::parse(v).is_err(), "{v}");
        }
        assert!(HttpsSourceNetworks::new(
            (1..=17)
                .map(|n| HttpsSourceNetwork::parse(&format!("10.0.{n}.0/24")).unwrap())
                .collect()
        )
        .is_err());
    }
}
