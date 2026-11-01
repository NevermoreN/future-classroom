//! Configured listener SAN identity on its exact selected credential context.
//! No cached inspection report, CN fallback, trust-store mutation or claim of
//! a remote browser's trust. Administrator deployment supplies that trust.
use super::*;
use std::net::IpAddr;

pub(super) fn verify(certificate: *const CERT_CONTEXT, host: &str) -> Result<(), TlsError> {
    // SAFETY: selected live CERT_CONTEXT/CERT_INFO owns all extension pointers.
    let info = unsafe { &*(*certificate).pCertInfo };
    if info.cExtension == 0 || info.cExtension > 128 || info.rgExtension.is_null() {
        return Err(TlsError::Certificate);
    }
    let extensions =
        unsafe { std::slice::from_raw_parts(info.rgExtension, info.cExtension as usize) };
    let mut san = None;
    for extension in extensions {
        // Windows' certificate parser supplies a terminated OID per extension.
        if !extension.pszObjId.is_null() && unsafe { extension.pszObjId.as_bytes() } == b"2.5.29.17"
        {
            if san.is_some()
                || extension.Value.cbData == 0
                || extension.Value.cbData > 65536
                || extension.Value.pbData.is_null()
            {
                return Err(TlsError::Certificate);
            }
            san = Some(unsafe {
                std::slice::from_raw_parts(extension.Value.pbData, extension.Value.cbData as usize)
            });
        }
    }
    if san.is_some_and(|bytes| matches_san(bytes, host)) {
        Ok(())
    } else {
        Err(TlsError::Certificate)
    }
}
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
        if !matches!(
            tag,
            0xa0 | 0x81 | 0x82 | 0xa3 | 0xa4 | 0xa5 | 0x86 | 0x87 | 0x88
        ) || tag == 0x87 && value.len() != 4 && value.len() != 16
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
}
