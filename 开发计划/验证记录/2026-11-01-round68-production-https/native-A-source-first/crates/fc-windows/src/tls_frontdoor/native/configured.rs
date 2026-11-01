//! Exact one-address binding and bounded explicit source admission. No wildcard,
//! interface guessing, trust-store/firewall mutation or HTTP.sys configuration.
use super::*;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};

fn supported(address: IpAddr) -> bool {
    !address.is_unspecified()
        && !address.is_multicast()
        && !matches!(address, IpAddr::V4(v) if v.is_broadcast())
        && !matches!(address, IpAddr::V6(v) if v.to_ipv4_mapped().is_some() || v.is_unicast_link_local())
}
fn unique_endpoint(
    addresses: impl IntoIterator<Item = SocketAddr>,
) -> Result<SocketAddr, ConfiguredTlsError> {
    let mut selected = None;
    for (count, address) in addresses.into_iter().enumerate() {
        if count >= 16
            || !supported(address.ip())
            || matches!(address, SocketAddr::V6(v) if v.scope_id() != 0)
        {
            return Err(ConfiguredTlsError::UnsupportedAddress);
        }
        match selected {
            None => selected = Some(address),
            Some(previous) if previous == address => {}
            Some(_) => return Err(ConfiguredTlsError::AmbiguousAddress),
        }
    }
    selected.ok_or(ConfiguredTlsError::AddressUnavailable)
}
pub(super) fn endpoint(
    configuration: &fc_domain::https_settings::HttpsConfiguration,
) -> Result<SocketAddr, ConfiguredTlsError> {
    configuration
        .validate()
        .map_err(|_| ConfiguredTlsError::InvalidConfiguration)?;
    if configuration.source_networks.is_none() {
        return Err(ConfiguredTlsError::MissingSourceNetworks);
    }
    if let Ok(ip) = configuration.host.as_str().parse::<IpAddr>() {
        return unique_endpoint([SocketAddr::new(ip, configuration.port)]);
    }
    let addresses = (configuration.host.as_str(), configuration.port)
        .to_socket_addrs()
        .map_err(|_| ConfiguredTlsError::AddressUnavailable)?;
    unique_endpoint(addresses)
}
pub(super) fn bind_exact(socket: SOCKET, address: SocketAddr) -> Result<(), String> {
    // SAFETY: synchronous bind copies the corresponding complete stack address.
    unsafe {
        match address {
            SocketAddr::V4(v) => {
                let address = SOCKADDR_IN {
                    sin_family: AF_INET,
                    sin_port: v.port().to_be(),
                    sin_addr: IN_ADDR {
                        S_un: IN_ADDR_0 {
                            S_addr: u32::from_ne_bytes(v.ip().octets()),
                        },
                    },
                    ..Default::default()
                };
                check(
                    bind(
                        socket,
                        ptr::from_ref(&address).cast(),
                        size_of::<SOCKADDR_IN>() as i32,
                    ),
                    "configured IPv4 bind",
                )
            }
            SocketAddr::V6(v) => {
                check(
                    setsockopt(
                        socket,
                        IPPROTO_IPV6.0,
                        IPV6_V6ONLY,
                        Some(&1i32.to_ne_bytes()),
                    ),
                    "IPv6-only listener",
                )?;
                let address = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    sin6_port: v.port().to_be(),
                    sin6_addr: IN6_ADDR {
                        u: IN6_ADDR_0 {
                            Byte: v.ip().octets(),
                        },
                    },
                    ..Default::default()
                };
                check(
                    bind(
                        socket,
                        ptr::from_ref(&address).cast(),
                        size_of::<SOCKADDR_IN6>() as i32,
                    ),
                    "configured IPv6 bind",
                )
            }
        }
    }
}
fn socket_address(socket: SOCKET, peer: bool) -> Result<(IpAddr, u16), String> {
    let mut storage = [0u64; 16];
    let mut length = std::mem::size_of_val(&storage) as i32;
    // SAFETY: aligned writable SOCKADDR_STORAGE-sized memory; the result is
    // interpreted only after checking its exact family and returned length.
    unsafe {
        let pointer = storage.as_mut_ptr().cast();
        check(
            if peer {
                getpeername(socket, pointer, &mut length)
            } else {
                getsockname(socket, pointer, &mut length)
            },
            "TLS socket address",
        )?;
        let family = *storage.as_ptr().cast::<ADDRESS_FAMILY>();
        if family == AF_INET && length == size_of::<SOCKADDR_IN>() as i32 {
            let v = &*storage.as_ptr().cast::<SOCKADDR_IN>();
            Ok((
                Ipv4Addr::from(v.sin_addr.S_un.S_addr.to_ne_bytes()).into(),
                u16::from_be(v.sin_port),
            ))
        } else if family == AF_INET6 && length == size_of::<SOCKADDR_IN6>() as i32 {
            let v = &*storage.as_ptr().cast::<SOCKADDR_IN6>();
            let ip = Ipv6Addr::from(v.sin6_addr.u.Byte);
            if v.Anonymous.sin6_scope_id != 0 || ip.to_ipv4_mapped().is_some() {
                return Err("TLS mapped or scoped peer unsupported".into());
            }
            Ok((ip.into(), u16::from_be(v.sin6_port)))
        } else {
            Err("TLS socket address family or length unsupported".into())
        }
    }
}
pub(super) fn peer_address(socket: SOCKET) -> Result<IpAddr, String> {
    socket_address(socket, true).map(|v| v.0)
}
pub(super) fn local_port(socket: SOCKET) -> Result<u16, String> {
    socket_address(socket, false).map(|v| v.1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_endpoint_never_selects_first_ambiguous_or_unsupported_address() {
        let one: SocketAddr = "127.0.0.1:9443".parse().unwrap();
        assert_eq!(unique_endpoint([one, one]).unwrap(), one);
        assert!(unique_endpoint([one, "[::1]:9443".parse().unwrap()]).is_err());
        for s in [
            "0.0.0.0:9443",
            "[::]:9443",
            "[::ffff:127.0.0.1]:9443",
            "[fe80::1]:9443",
            "224.0.0.1:9443",
        ] {
            assert!(unique_endpoint([s.parse().unwrap()]).is_err(), "{s}");
        }
        assert_eq!(size_of::<SOCKADDR_IN6>(), 28);
        assert!(ACCEPT_ADDRESS_LENGTH as usize >= size_of::<SOCKADDR_IN6>() + 16);
        assert_eq!(ACCEPT_ADDRESS_BYTES, 2 * ACCEPT_ADDRESS_LENGTH as usize);
    }
}
