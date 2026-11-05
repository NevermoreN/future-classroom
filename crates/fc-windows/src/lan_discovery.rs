//! Bounded, native IPv4 LAN discovery. Browsers cannot send these UDP queries.
//!
//! The caller polls from its existing network loop only while a LAN HTTPS
//! listener is active. Discovery advertises that listener; it grants no access.
use std::io;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

pub const DISCOVERY_PORT: u16 = 38443;
pub const DISCOVERY_QUERY: &[u8] = b"who is FutureClassroom? v1";
const MAX_QUERY_BYTES: usize = 128;
const MAX_REPLY_BYTES: usize = 1024;
const MAX_PACKETS_PER_POLL: usize = 8;
const MAX_REPLIES_PER_SECOND: usize = 8;

/// Owns one nonblocking UDP socket, without creating threads or peer maps.
pub struct DiscoveryResponder {
    socket: UdpSocket,
    window_start: Instant,
    replies: usize,
}

impl DiscoveryResponder {
    /// Bind the product's own discovery port (not Emby's port or protocol).
    pub fn bind() -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT))?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            window_start: Instant::now(),
            replies: 0,
        })
    }

    /// Inspect at most eight queued datagrams and reply directly to their sender.
    ///
    /// `public_json` must be caller-serialized public discovery JSON containing
    /// only the actual active HTTPS endpoint and public identity/labels. Never
    /// include credentials, tokens, or local administration/loopback URLs.
    /// Empty, oversized, or non-object-shaped payloads are rejected.
    ///
    /// `allow_peer` must restrict senders to the active listener's configured
    /// source CIDRs, excluding those networks' directed broadcast addresses.
    /// The responder additionally rejects non-RFC1918 IPv4 addresses and all
    /// IPv6 senders. A discovered endpoint must still undergo normal TLS trust
    /// and authentication; a UDP reply is not proof of server identity.
    pub fn poll(
        &mut self,
        public_json: &[u8],
        allow_peer: impl Fn(IpAddr) -> bool,
    ) -> io::Result<usize> {
        if public_json.len() > MAX_REPLY_BYTES
            || public_json.first() != Some(&b'{')
            || public_json.last() != Some(&b'}')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LAN discovery requires a public JSON object of at most 1024 bytes",
            ));
        }
        let now = Instant::now();
        if now.duration_since(self.window_start) >= Duration::from_secs(1) {
            self.window_start = now;
            self.replies = 0;
        }
        // One extra byte distinguishes oversized packets even when recv_from
        // truncates them to the provided buffer on the host platform.
        let mut buffer = [0_u8; MAX_QUERY_BYTES + 1];
        let mut sent = 0;
        for _ in 0..MAX_PACKETS_PER_POLL {
            let (length, peer) = match self.socket.recv_from(&mut buffer) {
                Ok(packet) => packet,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                // Windows returns WSAEMSGSIZE for oversized UDP datagrams.
                Err(error) if error.raw_os_error() == Some(10040) => continue,
                Err(error) => return Err(error),
            };
            if self.replies >= MAX_REPLIES_PER_SECOND
                || !valid_query(&buffer[..length], peer.ip())
                || !allow_peer(peer.ip())
            {
                continue;
            }
            // Charge attempts as well as successes to keep work bounded when
            // the network refuses outgoing packets.
            self.replies += 1;
            match self.socket.send_to(public_json, peer) {
                Ok(length) if length == public_json.len() => sent += 1,
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(sent)
    }
}

fn valid_query(query: &[u8], sender: IpAddr) -> bool {
    query == DISCOVERY_QUERY && matches!(sender, IpAddr::V4(address) if address.is_private())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_product_query_from_private_ipv4_is_eligible() {
        for address in ["10.2.3.4", "172.16.2.3", "192.168.10.212"] {
            assert!(valid_query(DISCOVERY_QUERY, address.parse().unwrap()));
        }
        for address in [
            "127.0.0.1",
            "8.8.8.8",
            "169.254.1.2",
            "224.0.0.1",
            "255.255.255.255",
            "0.0.0.0",
            "::1",
            "fd00::1",
        ] {
            assert!(!valid_query(DISCOVERY_QUERY, address.parse().unwrap()));
        }
        let sender = "192.168.10.2".parse().unwrap();
        assert!(!valid_query(b"who is EmbyServer?", sender));
        assert!(!valid_query(b"who is FutureClassroom? v1\n", sender));
        assert!(!valid_query(&[b'x'; MAX_QUERY_BYTES + 1], sender));
    }

    #[test]
    fn each_poll_drains_at_most_eight_packets_without_replying_to_loopback() {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket.set_nonblocking(true).unwrap();
        let target = socket.local_addr().unwrap();
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut responder = DiscoveryResponder {
            socket,
            window_start: Instant::now(),
            replies: 0,
        };
        for _ in 0..MAX_PACKETS_PER_POLL + 1 {
            sender.send_to(DISCOVERY_QUERY, target).unwrap();
        }
        assert_eq!(responder.poll(b"{}", |_| true).unwrap(), 0);
        let mut buffer = [0_u8; MAX_QUERY_BYTES + 1];
        assert_eq!(
            responder.socket.recv_from(&mut buffer).unwrap().0,
            DISCOVERY_QUERY.len()
        );
        assert_eq!(
            responder.socket.recv_from(&mut buffer).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(responder.replies, 0);
        assert_eq!(
            responder
                .poll(&[b'x'; MAX_REPLY_BYTES + 1], |_| true)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
