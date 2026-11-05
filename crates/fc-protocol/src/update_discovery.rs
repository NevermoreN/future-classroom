//! Fixed binary update discovery on the existing UDP socket. No public secrets.
//! Authentication establishes school membership, never official release trust.
use fc_ports::update_discovery::UpdateGroupAuthenticator;
use std::net::Ipv4Addr;

const MAGIC: &[u8; 8] = b"FCUPDISC";
const HEADER: usize = 58;
pub const QUERY_BYTES: usize = HEADER + 32;
pub const REPLY_BYTES: usize = HEADER + 4 + 2 + 32 + 32;
pub const MAX_DATAGRAM_BYTES: usize = REPLY_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Query {
    pub node: [u8; 16],
    pub nonce: [u8; 16],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub node: [u8; 16],
    pub nonce: [u8; 16],
    pub address: Ipv4Addr,
    pub port: u16,
    /// Exact DER certificate SHA-256, authenticated before native TLS pinning.
    pub certificate_sha256: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryError {
    Invalid,
    Unauthenticated,
    WrongChallenge,
}

fn header(
    out: &mut [u8],
    kind: u8,
    node: [u8; 16],
    nonce: [u8; 16],
    auth: &dyn UpdateGroupAuthenticator,
) -> Result<(), DiscoveryError> {
    if node == [0; 16] || nonce == [0; 16] || auth.group_id() == [0; 16] {
        return Err(DiscoveryError::Invalid);
    }
    out[..8].copy_from_slice(MAGIC);
    out[8] = 1;
    out[9] = kind;
    out[10..26].copy_from_slice(&auth.group_id());
    out[26..42].copy_from_slice(&node);
    out[42..58].copy_from_slice(&nonce);
    Ok(())
}
fn sign(out: &mut [u8], auth: &dyn UpdateGroupAuthenticator) {
    let end = out.len() - 32;
    let tag = auth.authenticate(&out[..end]);
    out[end..].copy_from_slice(&tag);
}
fn parse(
    input: &[u8],
    kind: u8,
    length: usize,
    auth: &dyn UpdateGroupAuthenticator,
) -> Result<Query, DiscoveryError> {
    if input.len() != length || &input[..8] != MAGIC || input[8] != 1 || input[9] != kind {
        return Err(DiscoveryError::Invalid);
    }
    let end = length - 32;
    if input[10..26] != auth.group_id()
        || !auth.verify(&input[..end], input[end..].try_into().unwrap())
    {
        return Err(DiscoveryError::Unauthenticated);
    }
    let node = input[26..42].try_into().unwrap();
    let nonce = input[42..58].try_into().unwrap();
    if node == [0; 16] || nonce == [0; 16] {
        return Err(DiscoveryError::Invalid);
    }
    Ok(Query { node, nonce })
}
pub fn encode_query(
    query: Query,
    auth: &dyn UpdateGroupAuthenticator,
) -> Result<[u8; QUERY_BYTES], DiscoveryError> {
    let mut out = [0; QUERY_BYTES];
    header(&mut out, 1, query.node, query.nonce, auth)?;
    sign(&mut out, auth);
    Ok(out)
}
pub fn decode_query(
    input: &[u8],
    auth: &dyn UpdateGroupAuthenticator,
) -> Result<Query, DiscoveryError> {
    parse(input, 1, QUERY_BYTES, auth)
}
pub fn encode_reply(
    reply: Reply,
    auth: &dyn UpdateGroupAuthenticator,
) -> Result<[u8; REPLY_BYTES], DiscoveryError> {
    if !reply.address.is_private() || reply.port == 0 || reply.certificate_sha256 == [0; 32] {
        return Err(DiscoveryError::Invalid);
    }
    let mut out = [0; REPLY_BYTES];
    header(&mut out, 2, reply.node, reply.nonce, auth)?;
    out[58..62].copy_from_slice(&reply.address.octets());
    out[62..64].copy_from_slice(&reply.port.to_be_bytes());
    out[64..96].copy_from_slice(&reply.certificate_sha256);
    sign(&mut out, auth);
    Ok(out)
}
/// The caller supplies its currently outstanding unpredictable nonce and the
/// actual UDP source; replayed challenges and third-party address referrals fail.
pub fn decode_reply(
    input: &[u8],
    nonce: [u8; 16],
    source: Ipv4Addr,
    auth: &dyn UpdateGroupAuthenticator,
) -> Result<Reply, DiscoveryError> {
    let query = parse(input, 2, REPLY_BYTES, auth)?;
    if nonce == [0; 16] || query.nonce != nonce {
        return Err(DiscoveryError::WrongChallenge);
    }
    let address = Ipv4Addr::new(input[58], input[59], input[60], input[61]);
    let port = u16::from_be_bytes(input[62..64].try_into().unwrap());
    let certificate_sha256 = input[64..96].try_into().unwrap();
    if !address.is_private() || address != source || port == 0 || certificate_sha256 == [0; 32] {
        return Err(DiscoveryError::Invalid);
    }
    Ok(Reply {
        node: query.node,
        nonce: query.nonce,
        address,
        port,
        certificate_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    // Codec fixture only; production cryptography lives in fc-security.
    struct Auth;
    impl UpdateGroupAuthenticator for Auth {
        fn group_id(&self) -> [u8; 16] {
            [1; 16]
        }
        fn authenticate(&self, message: &[u8]) -> [u8; 32] {
            Sha256::digest(message).into()
        }
        fn verify(&self, message: &[u8], tag: &[u8; 32]) -> bool {
            &self.authenticate(message) == tag
        }
    }
    #[test]
    fn exact_wire_bounds_and_all_bytes_are_bound() {
        let query = Query {
            node: [2; 16],
            nonce: [3; 16],
        };
        let encoded = encode_query(query, &Auth).unwrap();
        assert_eq!(decode_query(&encoded, &Auth), Ok(query));
        for size in 0..encoded.len() {
            assert!(decode_query(&encoded[..size], &Auth).is_err());
        }
        for position in 0..encoded.len() {
            let mut bad = encoded;
            bad[position] ^= 1;
            assert!(decode_query(&bad, &Auth).is_err());
        }
        let mut tail = encoded.to_vec();
        tail.push(0);
        assert!(decode_query(&tail, &Auth).is_err());
        assert!(QUERY_BYTES <= 256 && REPLY_BYTES <= 1024);
    }
    #[test]
    fn reply_rejects_replay_referrals_and_invalid_endpoints() {
        let reply = Reply {
            node: [2; 16],
            nonce: [3; 16],
            address: Ipv4Addr::new(192, 168, 10, 212),
            port: 8445,
            certificate_sha256: [4; 32],
        };
        let encoded = encode_reply(reply, &Auth).unwrap();
        assert_eq!(
            decode_reply(&encoded, reply.nonce, reply.address, &Auth),
            Ok(reply)
        );
        assert_eq!(
            decode_reply(&encoded, [5; 16], reply.address, &Auth),
            Err(DiscoveryError::WrongChallenge)
        );
        assert!(decode_reply(
            &encoded,
            reply.nonce,
            Ipv4Addr::new(192, 168, 10, 213),
            &Auth
        )
        .is_err());
        assert!(decode_query(&encoded, &Auth).is_err());
        for address in [
            Ipv4Addr::LOCALHOST,
            Ipv4Addr::BROADCAST,
            Ipv4Addr::new(8, 8, 8, 8),
        ] {
            assert!(encode_reply(Reply { address, ..reply }, &Auth).is_err());
        }
        assert!(encode_reply(Reply { port: 0, ..reply }, &Auth).is_err());
        assert!(encode_reply(
            Reply {
                certificate_sha256: [0; 32],
                ..reply
            },
            &Auth
        )
        .is_err());
        let mut tampered = encoded;
        tampered[64] ^= 1;
        assert_eq!(
            decode_reply(&tampered, reply.nonce, reply.address, &Auth),
            Err(DiscoveryError::Unauthenticated)
        );
    }
}
