use std::net::IpAddr;
fn tlv(bytes: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = bytes.split_first()?;
    let (&first, mut rest) = rest.split_first()?;
    let length = if first < 128 {
        first as usize
    } else {
        let count = (first & 127) as usize;
        if count == 0 || count > 4 || rest.len() < count || rest[0] == 0 {
            return None;
        }
        let mut length = 0usize;
        for byte in &rest[..count] {
            length = length.checked_mul(256)?.checked_add(*byte as usize)?;
        }
        if length < 128 {
            return None;
        }
        rest = &rest[count..];
        length
    };
    if rest.len() < length {
        return None;
    }
    Some((tag, &rest[..length], &rest[length..]))
}
fn dns_matches(name: &[u8], host: &str) -> bool {
    if name.is_empty() || name.len() > 253 || !name.is_ascii() {
        return false;
    }
    let Ok(name) = std::str::from_utf8(name) else {
        return false;
    };
    if name.eq_ignore_ascii_case(host) {
        return true;
    }
    let Some(suffix) = name.strip_prefix("*.") else {
        return false;
    };
    // One whole leftmost label only, with at least two explicit suffix labels.
    if suffix.contains('*') || !suffix.contains('.') {
        return false;
    }
    host.split_once('.')
        .is_some_and(|(first, rest)| !first.is_empty() && rest.eq_ignore_ascii_case(suffix))
}
fn matches_san(bytes: &[u8], host: &str) -> bool {
    let Some((0x30, mut names, [])) = tlv(bytes) else {
        return false;
    };
    let address = host.parse::<IpAddr>().ok();
    let mut found = false;
    let mut count = 0;
    while !names.is_empty() {
        count += 1;
        if count > 128 {
            return false;
        }
        let Some((tag, value, rest)) = tlv(names) else {
            return false;
        };
        names = rest;
        // A supported matching name cannot hide an unvalidated later name.
        if !matches!(tag, 0x82 | 0x87)
            || tag == 0x87 && value.len() != 4 && value.len() != 16
            || tag == 0x82
                && (value.is_empty()
                    || value.len() > 253
                    || !value.is_ascii()
                    || value.contains(&0))
        {
            return false;
        }
        found |= match (tag, address) {
            (0x82, None) => dns_matches(value, host),
            (0x87, Some(IpAddr::V4(ip))) => value == ip.octets(),
            (0x87, Some(IpAddr::V6(ip))) => value == ip.octets(),
            _ => false,
        };
    }
    found
}
#[cfg(test)]
mod tests {
    use super::*;
    fn san(tag: u8, bytes: &[u8]) -> Vec<u8> {
        let mut v = vec![0x30, (bytes.len() + 2) as u8, tag, bytes.len() as u8];
        v.extend_from_slice(bytes);
        v
    }
    #[test]
    fn configured_certificate_san_matches_exact_ip_or_one_dns_label() {
        assert!(matches_san(&san(0x87, &[127, 0, 0, 1]), "127.0.0.1"));
        assert!(!matches_san(&san(0x82, b"127.0.0.1"), "127.0.0.1"));
        assert!(!matches_san(&san(0x87, &[127, 0, 0, 2]), "127.0.0.1"));
        assert!(matches_san(
            &san(0x87, &std::net::Ipv6Addr::LOCALHOST.octets()),
            "::1"
        ));
        assert!(matches_san(
            &san(0x82, b"BOARD.school.example"),
            "board.school.example"
        ));
        assert!(matches_san(
            &san(0x82, b"*.school.example"),
            "board.school.example"
        ));
        for host in [
            "school.example",
            "one.board.school.example",
            "board.other.example",
        ] {
            assert!(!matches_san(&san(0x82, b"*.school.example"), host));
        }
        assert!(!matches_san(&san(0x82, b"*.example"), "board.example"));
        assert!(!matches_san(
            &san(0x82, b"board.school.example\0evil"),
            "board.school.example"
        ));
    }
    #[test]
    fn configured_certificate_san_rejects_malformed_trailing_and_oversized_encoding() {
        for bytes in [
            &[0x30, 0x80, 0, 0][..],
            &[0x30, 0x81, 3, 0x82, 1, b'a'],
            &[0x30, 3, 0x82, 2, b'a'],
            &[0x30, 3, 0x82, 1, b'a', 0],
        ] {
            assert!(!matches_san(bytes, "a"));
        }
    }
    #[test]
    fn configured_certificate_san_rejects_other_general_names_after_a_matching_ip() {
        let malformed_directory = [0x30, 9, 0x87, 4, 127, 0, 0, 1, 0xa4, 1, 0xff];
        assert!(!matches_san(&malformed_directory, "127.0.0.1"));
        // Even well-formed unsupported names must not become an accidental
        // promise that this bounded matcher validates all GeneralName types.
        for (tag, value) in [
            (0xa4, &[0x30, 0][..]),
            (0x86, &b"https://school.example"[..]),
            (0x81, &b"admin@school.example"[..]),
            (0x88, &[0x2a, 3, 4][..]),
        ] {
            let mut content = vec![0x87, 4, 127, 0, 0, 1, tag, value.len() as u8];
            content.extend_from_slice(value);
            let mut outer = vec![0x30, content.len() as u8];
            outer.extend(content);
            assert!(!matches_san(&outer, "127.0.0.1"));
        }
    }
}
