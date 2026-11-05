//! Exact bounded wire shapes; these membership proofs never grant install trust.
use super::*;
pub(super) fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(H[(b >> 4) as usize] as char);
        out.push(H[(b & 15) as usize] as char);
    }
    out
}
pub(super) fn unhex<const N: usize>(value: &[u8]) -> Option<[u8; N]> {
    if value.len() != N * 2 {
        return None;
    }
    let mut b = [0; N];
    for (i, p) in value.chunks_exact(2).enumerate() {
        let digit = |b| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        };
        b[i] = digit(p[0])? * 16 + digit(p[1])?;
    }
    Some(b)
}
fn proof_message(
    node: [u8; 16],
    nonce: [u8; 16],
    challenge: [u8; 16],
    path: &str,
    range: &str,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(128 + path.len() + range.len());
    bytes.extend_from_slice(b"FutureClassroom.UpdateHttp.v1\0");
    bytes.extend_from_slice(&node);
    bytes.extend_from_slice(&nonce);
    bytes.extend_from_slice(&challenge);
    bytes.extend_from_slice(&(path.len() as u16).to_be_bytes());
    bytes.extend_from_slice(path.as_bytes());
    bytes.extend_from_slice(&(range.len() as u16).to_be_bytes());
    bytes.extend_from_slice(range.as_bytes());
    bytes
}
pub(super) fn request(
    config: &NetworkConfiguration,
    peer: Peer,
    nonce: [u8; 16],
    challenge: [u8; 16],
    path: &str,
    range: &str,
) -> Vec<u8> {
    let tag = proof(
        config.auth.as_ref(),
        config.node,
        nonce,
        challenge,
        path,
        range,
    );
    format!("GET {path} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nAccept-Encoding: identity\r\nX-FC-Node: {}\r\nX-FC-Nonce: {}\r\nX-FC-Challenge: {}\r\nX-FC-Proof: {}\r\n{}\r\n",peer.address,peer.port,hex(&config.node),hex(&nonce),hex(&challenge),hex(&tag),if range.is_empty(){String::new()}else{format!("Range: {range}\r\n")}).into_bytes()
}
pub(super) fn header<'a>(head: &'a RequestHead, name: &str) -> Result<Option<&'a [u8]>, String> {
    let mut found = None;
    for h in &head.headers {
        if h.name.eq_ignore_ascii_case(name) {
            if found.is_some() {
                return Err("duplicate update request header".into());
            }
            found = Some(h.value.as_slice());
        }
    }
    Ok(found)
}
pub(super) fn challenge_response(
    config: &NetworkConfiguration,
    client: [u8; 16],
    nonce: [u8; 16],
    challenge: [u8; 16],
) -> Vec<u8> {
    let mut out = b"FCUPCH01".to_vec();
    out.extend_from_slice(&config.node);
    out.extend_from_slice(&client);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&challenge);
    let tag = config.auth.authenticate(&out);
    out.extend_from_slice(&tag);
    out
}
pub(super) fn parse_challenge(
    config: &NetworkConfiguration,
    peer: Peer,
    nonce: [u8; 16],
    bytes: &[u8],
) -> Result<[u8; 16], String> {
    if bytes.len() != 104
        || &bytes[..8] != b"FCUPCH01"
        || bytes[8..24] != peer.node
        || bytes[24..40] != config.node
        || bytes[40..56] != nonce
        || !config.auth.verify(
            &bytes[..72],
            bytes[72..].try_into().map_err(|_| "challenge tag")?,
        )
    {
        return Err("invalid update challenge proof".into());
    }
    let challenge = bytes[56..72].try_into().map_err(|_| "challenge nonce")?;
    if challenge == [0; 16] {
        return Err("zero update challenge".into());
    }
    Ok(challenge)
}
pub(super) fn metadata_name(name: &str) -> bool {
    matches!(name, "timestamp.json" | "snapshot.json" | "targets.json")
        || name.strip_suffix(".root.json").is_some_and(|v| {
            !v.is_empty()
                && v.len() <= 20
                && !v.starts_with('0')
                && v.bytes().all(|b| b.is_ascii_digit())
                && v.parse::<u64>().is_ok_and(|n| n > 0)
        })
}
pub(super) fn root_versions(bytes: &[u8]) -> Result<Vec<u64>, String> {
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err("catalog bound".into());
    }
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| "catalog JSON")?;
    let object = v.as_object().ok_or("catalog object")?;
    if object.len() != 2 || v["schemaVersion"] != 1 {
        return Err("catalog schema".into());
    }
    let a = v["roots"].as_array().ok_or("catalog roots")?;
    if a.len() > 32 {
        return Err("root chain bound".into());
    }
    let roots = a
        .iter()
        .map(|n| {
            n.as_u64()
                .filter(|n| *n > 0)
                .ok_or("root version".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if roots.windows(2).any(|w| w[0].checked_add(1) != Some(w[1])) {
        return Err("root chain order".into());
    }
    Ok(roots)
}
pub(super) fn range(value: &[u8], length: u64) -> Option<u64> {
    let s = std::str::from_utf8(value).ok()?;
    let n = s.strip_prefix("bytes=")?.strip_suffix('-')?;
    if n.is_empty()
        || n.len() > 20
        || n.len() > 1 && n.starts_with('0')
        || !n.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let offset = n.parse().ok()?;
    if offset >= length {
        None
    } else {
        Some(offset)
    }
}
#[derive(Clone, Copy)]
pub(super) enum Expected {
    Challenge,
    Catalog,
    Metadata,
    Blob { target: VerifiedTarget, offset: u64 },
}
pub(super) struct ResponseParser {
    head: Vec<u8>,
    pub head_complete: bool,
    pub length: u64,
    pub received: u64,
    expected: Expected,
}
impl ResponseParser {
    pub fn new(expected: Expected) -> Self {
        Self {
            head: Vec::new(),
            head_complete: false,
            length: 0,
            received: 0,
            expected,
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        let body = if !self.head_complete {
            let end = self.head.len() + bytes.len();
            if end > 8192 + 16384 {
                return Err("update response head bound".into());
            }
            self.head.extend_from_slice(bytes);
            let Some(pos) = self.head.windows(4).position(|v| v == b"\r\n\r\n") else {
                if self.head.len() > 8192 {
                    return Err("update head too large".into());
                }
                return Ok(Vec::new());
            };
            if pos + 4 > 8192 {
                return Err("update head too large".into());
            }
            self.validate(pos)?;
            self.head_complete = true;
            let body = self.head[pos + 4..].to_vec();
            self.head.clear();
            body
        } else {
            bytes.to_vec()
        };
        self.received = self
            .received
            .checked_add(body.len() as u64)
            .ok_or("update body overflow")?;
        if self.received > self.length {
            return Err("update response exceeds content length".into());
        }
        Ok(body)
    }
    fn validate(&mut self, end: usize) -> Result<(), String> {
        let text = std::str::from_utf8(&self.head[..end]).map_err(|_| "non-ASCII response head")?;
        if !text.is_ascii() {
            return Err("non-ASCII response head".into());
        }
        let mut lines = text.split("\r\n");
        let status = lines.next().ok_or("missing update status")?;
        let status = status
            .strip_prefix("HTTP/1.1 ")
            .ok_or("update HTTP version")?
            .split(' ')
            .next()
            .ok_or("update status")?
            .parse::<u16>()
            .map_err(|_| "update status")?;
        let mut headers = BTreeMap::new();
        for line in lines {
            let (name, value) = line.split_once(':').ok_or("invalid update header")?;
            if name.is_empty()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                || value.bytes().any(|b| b < 32 && b != b'\t' || b == 127)
            {
                return Err("invalid update header".into());
            }
            if headers
                .insert(name.to_ascii_lowercase(), value.trim().to_owned())
                .is_some()
            {
                return Err("duplicate update response header".into());
            }
        }
        if headers.contains_key("transfer-encoding")
            || headers.contains_key("location")
            || headers
                .get("content-encoding")
                .is_some_and(|v| v != "identity")
        {
            return Err("update encoding or redirect refused".into());
        }
        let value = headers
            .get("content-length")
            .ok_or("update length required")?;
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err("invalid update length".into());
        }
        self.length = value.parse().map_err(|_| "invalid update length")?;
        match self.expected {
            Expected::Blob { target, offset } => {
                if self.length != target.length - offset
                    || headers.get("etag").map(String::as_str)
                        != Some(format!("\"{}\"", hex(&target.sha256)).as_str())
                {
                    return Err("update blob identity or length mismatch".into());
                }
                if status == 206 {
                    let expected =
                        format!("bytes {}-{}/{}", offset, target.length - 1, target.length);
                    if headers.get("content-range") != Some(&expected) {
                        return Err("update content range mismatch".into());
                    }
                } else if status != 200 || offset != 0 || headers.contains_key("content-range") {
                    return Err("update range response refused".into());
                }
            }
            expected => {
                let maximum = match expected {
                    Expected::Challenge => 104,
                    Expected::Catalog => 4096,
                    Expected::Metadata => META as u64,
                    _ => unreachable!(),
                };
                if status != 200
                    || self.length == 0
                    || self.length > maximum
                    || headers.contains_key("content-range")
                {
                    return Err("update metadata response refused".into());
                }
            }
        }
        Ok(())
    }
    pub fn complete(&self) -> bool {
        self.head_complete && self.received == self.length
    }
}
pub(super) fn proof(
    auth: &dyn UpdateGroupAuthenticator,
    node: [u8; 16],
    nonce: [u8; 16],
    challenge: [u8; 16],
    path: &str,
    range: &str,
) -> [u8; 32] {
    auth.authenticate(&proof_message(node, nonce, challenge, path, range))
}
pub(super) fn verify_proof(
    auth: &dyn UpdateGroupAuthenticator,
    node: [u8; 16],
    nonce: [u8; 16],
    challenge: [u8; 16],
    path: &str,
    range: &str,
    tag: &[u8; 32],
) -> bool {
    auth.verify(&proof_message(node, nonce, challenge, path, range), tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_rejects_redirect_encoding_and_range_aliases() {
        let target = VerifiedTarget {
            sha256: [3; 32],
            length: 10,
        };
        let head=format!("HTTP/1.1 206 Partial Content\r\nContent-Length: 6\r\nETag: \"{}\"\r\nContent-Range: bytes 4-9/10\r\n\r\nabcdef",hex(&target.sha256));
        let mut p = ResponseParser::new(Expected::Blob { target, offset: 4 });
        assert_eq!(p.feed(head.as_bytes()).unwrap(), b"abcdef");
        assert!(p.complete());
        for altered in [
            head.replace("206 Partial Content", "302 Redirect"),
            head.replace("bytes 4-9/10", "bytes 0-5/10"),
            head.replace(
                "Content-Length: 6",
                "Content-Length: 6\r\nContent-Encoding: gzip",
            ),
            head.replace(
                "Content-Length: 6",
                "Content-Length: 6\r\nContent-Length: 6",
            ),
        ] {
            assert!(ResponseParser::new(Expected::Blob { target, offset: 4 })
                .feed(altered.as_bytes())
                .is_err());
        }
        assert_eq!(range(b"bytes=4-", 10), Some(4));
        for bad in [
            b"bytes=04-".as_slice(),
            b"bytes=4-8",
            b"bytes=4-,8-",
            b"bytes=10-",
        ] {
            assert_eq!(range(bad, 10), None);
        }
    }
}
