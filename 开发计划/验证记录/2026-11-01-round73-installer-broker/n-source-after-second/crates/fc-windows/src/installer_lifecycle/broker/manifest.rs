use super::super::{LifecycleError, Result};
use std::collections::BTreeSet;

pub(crate) const MAGIC: &[u8; 8] = b"FCLM73\0\0";
pub(crate) const FOOTER: &[u8; 16] = b"FCLIFE73-END-v1\0";
pub(crate) const MAX_MANIFEST: usize = 32768;
pub(crate) const MAX_PAYLOAD: u32 = 30 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct Entry {
    pub path: String,
    pub bytes: u32,
    pub sha256: [u8; 32],
    pub component: String,
}
#[derive(Debug)]
pub(crate) struct Manifest {
    pub product: String,
    pub version: String,
    pub entries: Vec<Entry>,
}

fn invalid<T>() -> Result<T> {
    Err(LifecycleError::Invalid)
}
fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    if bytes.len() < n {
        return invalid();
    }
    let (value, rest) = bytes.split_at(n);
    *bytes = rest;
    Ok(value)
}
fn u16le(bytes: &mut &[u8]) -> Result<u16> {
    Ok(u16::from_le_bytes(take(bytes, 2)?.try_into().unwrap()))
}
fn u32le(bytes: &mut &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()))
}
fn ascii(bytes: &mut &[u8], n: usize) -> Result<String> {
    let value = take(bytes, n)?;
    if !value.is_ascii() {
        return invalid();
    }
    String::from_utf8(value.to_vec()).map_err(|_| LifecycleError::Invalid)
}
pub(crate) fn uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'A'..=b'F').contains(&b)
            }
        })
}
fn version(text: &str) -> bool {
    let fields: Vec<_> = text.split('.').collect();
    fields.len() == 3
        && fields.iter().enumerate().all(|(i, field)| {
            !field.is_empty()
                && field.bytes().all(|b| b.is_ascii_digit())
                && field
                    .parse::<u32>()
                    .is_ok_and(|n| n <= if i == 2 { 65535 } else { 255 })
        })
}
pub(crate) fn path(text: &str) -> bool {
    if text.is_empty() || text.len() > 240 || !text.is_ascii() {
        return false;
    }
    if matches!(
        text,
        "fc-bootstrap.exe"
            | "LICENSE"
            | "PROBE-README.txt"
            | "THIRD-PARTY-NOTICES.txt"
            | "manifest.json"
            | "MSI-CANDIDATE.txt"
            | "web/index.html"
            | "web/fc-assets.json"
            | "web/THIRD-PARTY-NOTICES.txt"
    ) {
        return true;
    }
    let Some(name) = text.strip_prefix("web/assets/") else {
        return false;
    };
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        && name.rsplit_once('.').is_some_and(|(_, extension)| {
            matches!(
                extension,
                "js" | "css" | "svg" | "png" | "jpg" | "jpeg" | "webp"
            )
        })
}

pub(crate) fn parse(mut bytes: &[u8]) -> Result<Manifest> {
    if bytes.len() > MAX_MANIFEST || take(&mut bytes, 8)? != MAGIC || u16le(&mut bytes)? != 1 {
        return invalid();
    }
    let count = usize::from(u16le(&mut bytes)?);
    if !(1..=72).contains(&count) {
        return invalid();
    }
    let product = ascii(&mut bytes, 36)?;
    let length = usize::from(take(&mut bytes, 1)?[0]);
    if !(1..=24).contains(&length) {
        return invalid();
    }
    let product_version = ascii(&mut bytes, length)?;
    if !uuid(&product) || !version(&product_version) {
        return invalid();
    }
    let mut entries = Vec::with_capacity(count);
    let mut paths = BTreeSet::new();
    let mut components = BTreeSet::new();
    let mut total = 0u32;
    let mut previous = String::new();
    for _ in 0..count {
        let length = usize::from(u16le(&mut bytes)?);
        if !(1..=240).contains(&length) {
            return invalid();
        }
        let name = ascii(&mut bytes, length)?;
        let size = u32le(&mut bytes)?;
        let digest: [u8; 32] = take(&mut bytes, 32)?.try_into().unwrap();
        let component = ascii(&mut bytes, 36)?;
        total = total.checked_add(size).ok_or(LifecycleError::Invalid)?;
        if !path(&name)
            || name <= previous
            || !paths.insert(name.to_ascii_lowercase())
            || size == 0
            || total > MAX_PAYLOAD
            || !uuid(&component)
            || !components.insert(component.clone())
        {
            return invalid();
        }
        previous = name.clone();
        entries.push(Entry {
            path: name,
            bytes: size,
            sha256: digest,
            component,
        });
    }
    for required in [
        "fc-bootstrap.exe",
        "LICENSE",
        "PROBE-README.txt",
        "THIRD-PARTY-NOTICES.txt",
        "manifest.json",
        "MSI-CANDIDATE.txt",
        "web/index.html",
        "web/fc-assets.json",
        "web/THIRD-PARTY-NOTICES.txt",
    ] {
        if !paths.contains(&required.to_ascii_lowercase()) {
            return invalid();
        }
    }
    // A file cannot simultaneously be the ancestor of another declared file.
    if !bytes.is_empty()
        || entries.iter().any(|a| {
            entries
                .iter()
                .any(|b| b.path.starts_with(&(a.path.clone() + "/")))
        })
    {
        return invalid();
    }
    Ok(Manifest {
        product,
        version: product_version,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_cannot_escape_or_create_win32_aliases() {
        for value in [
            "web/../a",
            "web/a:ads",
            "web/a.",
            "web/NUL.txt",
            "web/a\\b",
            "C:/x",
            "web//a",
            "web/.hidden",
        ] {
            assert!(!path(value), "{value}");
        }
        assert!(!path("web/资料.js"));
        assert!(path("web/assets/main-1.js"));
    }

    fn fixture() -> Vec<u8> {
        let mut names = [
            "fc-bootstrap.exe",
            "LICENSE",
            "PROBE-README.txt",
            "THIRD-PARTY-NOTICES.txt",
            "manifest.json",
            "MSI-CANDIDATE.txt",
            "web/index.html",
            "web/fc-assets.json",
            "web/THIRD-PARTY-NOTICES.txt",
        ];
        names.sort();
        let mut b = MAGIC.to_vec();
        b.extend(1u16.to_le_bytes());
        b.extend((names.len() as u16).to_le_bytes());
        b.extend(b"12345678-1234-1234-1234-123456789ABC");
        b.push(5);
        b.extend(b"0.1.0");
        for (i, name) in names.iter().enumerate() {
            b.extend((name.len() as u16).to_le_bytes());
            b.extend(name.as_bytes());
            b.extend(1u32.to_le_bytes());
            b.extend([i as u8; 32]);
            b.extend(format!("00000000-0000-0000-0000-{:012X}", i + 1).as_bytes());
        }
        b
    }
    #[test]
    fn actual_closed_format_accepts_full_set_and_rejects_truncation_tail_and_aliases() {
        let b = fixture();
        let value = parse(&b).unwrap();
        assert_eq!(value.entries.len(), 9);
        assert_eq!(value.version, "0.1.0");
        for end in 0..b.len() {
            assert!(parse(&b[..end]).is_err(), "truncation {end}");
        }
        let mut tail = b.clone();
        tail.push(0);
        assert!(parse(&tail).is_err());
        let mut magic = b.clone();
        magic[0] ^= 1;
        assert!(parse(&magic).is_err());
        let mut duplicate = b.clone();
        let positions: Vec<_> = duplicate
            .windows(36)
            .enumerate()
            .filter(|(_, v)| v.starts_with(b"00000000-0000-0000-0000-"))
            .map(|(i, _)| i)
            .collect();
        let first: Vec<_> = duplicate[positions[0]..positions[0] + 36].to_vec();
        duplicate[positions[1]..positions[1] + 36].copy_from_slice(&first);
        assert!(parse(&duplicate).is_err());
    }
}
