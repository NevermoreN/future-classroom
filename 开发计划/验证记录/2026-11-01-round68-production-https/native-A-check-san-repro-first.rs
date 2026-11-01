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

fn main() { let malformed=[0x30,9,0x87,4,127,0,0,1,0xa4,1,0xff]; println!("malformed_directory_name_accepted={}",matches_san(&malformed,"127.0.0.1")); assert!(matches_san(&malformed,"127.0.0.1"),"reproduce first implementation defect"); }
