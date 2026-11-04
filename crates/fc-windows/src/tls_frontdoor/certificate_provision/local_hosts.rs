//! Win7-compatible local adapter query; no DNS, probe, shell or firewall change.
use super::CertificateProvisionError;
use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};
use windows::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA},
    NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp},
    Networking::WinSock::*,
};

pub(super) fn enumerate() -> Result<Vec<String>, CertificateProvisionError> {
    let mut size = 16 * 1024u32;
    for _ in 0..3 {
        if size > 256 * 1024 {
            return Err(CertificateProvisionError::Unavailable);
        }
        // Vec<u64> supplies the adapter structure's documented alignment on
        // both x86 and x64; all returned list/address storage stays in this call.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let result = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                GAA_FLAG_SKIP_ANYCAST
                    | GAA_FLAG_SKIP_MULTICAST
                    | GAA_FLAG_SKIP_DNS_SERVER
                    | GAA_FLAG_SKIP_FRIENDLY_NAME,
                None,
                Some(first),
                &mut size,
            )
        };
        if result == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if result == ERROR_NO_DATA.0 {
            return Ok(vec!["127.0.0.1".into()]);
        }
        if result != 0 {
            return Err(CertificateProvisionError::Native(result as i32));
        }
        let mut hosts = BTreeSet::from([IpAddr::V4(Ipv4Addr::LOCALHOST)]);
        let mut adapter = first;
        // Bounds cap traversal as well as retained SAN names. API lists are
        // native outputs, and no pointer escapes the owned backing allocation.
        for _ in 0..128 {
            if adapter.is_null() {
                break;
            }
            let current = unsafe { &*adapter };
            if current.OperStatus == IfOperStatusUp {
                let mut unicast = current.FirstUnicastAddress;
                for _ in 0..128 {
                    if unicast.is_null() {
                        break;
                    }
                    let value = unsafe { &*unicast };
                    let address = value.Address.lpSockaddr;
                    let length = value.Address.iSockaddrLength as usize;
                    if !address.is_null()
                        && length >= std::mem::size_of::<ADDRESS_FAMILY>()
                        && value.DadState == IpDadStatePreferred
                    {
                        let family = unsafe { (*address).sa_family };
                        let ip =
                            if family == AF_INET && length >= std::mem::size_of::<SOCKADDR_IN>() {
                                let address = unsafe { &*address.cast::<SOCKADDR_IN>() };
                                Some(IpAddr::V4(Ipv4Addr::from(
                                    unsafe { address.sin_addr.S_un.S_addr }.to_ne_bytes(),
                                )))
                            } else if family == AF_INET6
                                && length >= std::mem::size_of::<SOCKADDR_IN6>()
                            {
                                let address = unsafe { &*address.cast::<SOCKADDR_IN6>() };
                                let ip = Ipv6Addr::from(unsafe { address.sin6_addr.u.Byte });
                                (!ip.is_unicast_link_local() && ip.to_ipv4_mapped().is_none())
                                    .then_some(IpAddr::V6(ip))
                            } else {
                                None
                            };
                        if let Some(ip) = ip.filter(|ip| {
                            !ip.is_unspecified()
                                && !ip.is_multicast()
                                && !matches!(ip, IpAddr::V4(v) if v.is_broadcast())
                                && hosts.len() < 16
                        }) {
                            hosts.insert(ip);
                        }
                    }
                    unicast = value.Next;
                }
            }
            adapter = current.Next;
        }
        return Ok(hosts.into_iter().map(|ip| ip.to_string()).collect());
    }
    Err(CertificateProvisionError::Unavailable)
}
