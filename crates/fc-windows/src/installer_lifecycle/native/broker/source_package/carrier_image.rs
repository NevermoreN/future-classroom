//! Bounded seek adapter for the frozen carrier grammar. No whole-image copy.
use crate::installer_lifecycle::{
    broker::{carrier, manifest},
    LifecycleError, Result,
};
fn invalid<T>() -> Result<T> {
    Err(LifecycleError::Invalid)
}
pub(super) fn hole(
    length: usize,
    mut read: impl FnMut(usize, &mut [u8]) -> Result<()>,
) -> Result<usize> {
    if !(carrier::MIN_IMAGE_BYTES..=carrier::MAX_IMAGE_BYTES).contains(&length) {
        return invalid();
    }
    let prefix = length - 132;
    let mut tail = [0u8; 132];
    read(prefix, &mut tail)?;
    if &tail[..16] != carrier::SEAL_MAGIC
        || tail[16..48] == [0; 32]
        || tail[48..112] != carrier::descriptor()
        || tail[112..116] != 64u32.to_le_bytes()
        || &tail[116..] != carrier::FOOTER
    {
        return invalid();
    }
    let mut boundary = [0; 48];
    if prefix < 64 {
        return invalid();
    }
    read(prefix - 48, &mut boundary)?;
    if boundary.ends_with(manifest::FOOTER)
        || boundary.ends_with(carrier::FOOTER)
        || &boundary[..16] == carrier::SEAL_MAGIC
    {
        return invalid();
    }
    let mut get = |at: usize, bytes: &mut [u8]| -> Result<()> {
        if at.checked_add(bytes.len()).is_none_or(|end| end > prefix) {
            return invalid();
        }
        read(at, bytes)
    };
    let mut dos = [0; 64];
    get(0, &mut dos)?;
    if &dos[..2] != b"MZ" {
        return invalid();
    }
    let pe = u32::from_le_bytes(dos[60..64].try_into().unwrap()) as usize;
    let mut coff = [0; 24];
    get(pe, &mut coff)?;
    let u16at = |b: &[u8], at: usize| u16::from_le_bytes(b[at..at + 2].try_into().unwrap());
    let u32at =
        |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
    if pe < 64 || &coff[..4] != b"PE\0\0" || u16at(&coff, 4) != 0x14c {
        return invalid();
    }
    let count = usize::from(u16at(&coff, 6));
    let optional_size = usize::from(u16at(&coff, 20));
    let optional_at = pe.checked_add(24).ok_or(LifecycleError::Invalid)?;
    let mut optional = [0; 224];
    get(optional_at, &mut optional)?;
    if !(1..=96).contains(&count)
        || optional_size < 224
        || u16at(&optional, 0) != 0x10b
        || u16at(&optional, 68) != 2
        || u16at(&optional, 48) != 6
        || u16at(&optional, 50) != 1
    {
        return invalid();
    }
    let table = optional_at
        .checked_add(optional_size)
        .ok_or(LifecycleError::Invalid)?;
    let table_end = table
        .checked_add(count * 40)
        .ok_or(LifecycleError::Invalid)?;
    let headers = u32at(&optional, 60);
    if headers < table_end || headers > prefix {
        return invalid();
    }
    let mut ranges = [(0usize, 0usize); 96];
    let mut end = headers;
    for i in 0..count {
        let mut section = [0; 40];
        get(table + i * 40, &mut section)?;
        let size = u32at(&section, 16);
        let offset = u32at(&section, 20);
        if size == 0 {
            continue;
        }
        let next = offset.checked_add(size).ok_or(LifecycleError::Invalid)?;
        if offset < headers
            || next > prefix
            || ranges[..i].iter().any(|&(a, z)| offset < z && next > a)
        {
            return invalid();
        }
        ranges[i] = (offset, next);
        end = end.max(next);
    }
    let directories = u32at(&optional, 92);
    if directories > 16 {
        return invalid();
    }
    if directories > 4 {
        let certificate = u32at(&optional, 128);
        let size = u32at(&optional, 132);
        if certificate != 0 || size != 0 {
            if certificate < end
                || !certificate.is_multiple_of(8)
                || size < 8
                || certificate.checked_add(size) != Some(prefix)
            {
                return invalid();
            }
            end = prefix;
        }
    }
    if end != prefix {
        return invalid();
    }
    Ok(prefix + 16)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pe_fixture() -> Vec<u8> {
        let mut b = vec![0u8; 1024];
        b[..2].copy_from_slice(b"MZ");
        b[60..64].copy_from_slice(&64u32.to_le_bytes());
        b[64..68].copy_from_slice(b"PE\0\0");
        b[68..70].copy_from_slice(&0x14cu16.to_le_bytes());
        b[70..72].copy_from_slice(&1u16.to_le_bytes());
        b[84..86].copy_from_slice(&224u16.to_le_bytes());
        b[86..88].copy_from_slice(&0x102u16.to_le_bytes());
        b[88..90].copy_from_slice(&0x10bu16.to_le_bytes());
        for (at, value) in [
            (92, 512u32),
            (104, 0x1000),
            (108, 0x1000),
            (116, 0x400000),
            (120, 0x1000),
            (124, 512),
            (144, 0x2000),
            (148, 512),
            (160, 0x100000),
            (164, 0x1000),
            (168, 0x100000),
            (172, 0x1000),
            (180, 16),
        ] {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        b[136..138].copy_from_slice(&6u16.to_le_bytes());
        b[138..140].copy_from_slice(&1u16.to_le_bytes());
        b[156..158].copy_from_slice(&2u16.to_le_bytes());
        b[312..317].copy_from_slice(b".text");
        for (at, value) in [
            (320, 3u32),
            (324, 0x1000),
            (328, 512),
            (332, 512),
            (348, 0x60000020),
        ] {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        b[512..515].copy_from_slice(&[0x31, 0xc0, 0xc3]);
        b
    }
    fn check(b: &[u8]) -> Result<usize> {
        hole(b.len(), |at, out| {
            out.copy_from_slice(b.get(at..at + out.len()).ok_or(LifecycleError::Invalid)?);
            Ok(())
        })
    }
    #[test]
    fn streaming_carrier_checks_same_full_prefix_and_only_digest_hole() {
        let pe = pe_fixture();
        let template = carrier::make_template(&pe).unwrap();
        let sealed = carrier::fill_digest_format_value(&template, [1; 32]).unwrap();
        assert_eq!(
            check(&sealed).unwrap(),
            carrier::parse(&sealed).unwrap().digest_offset()
        );
        assert!(check(&template).is_err());
        let hole = check(&sealed).unwrap();
        for at in hole..hole + 32 {
            let mut changed = sealed.clone();
            changed[at] ^= 0x80;
            assert_eq!(check(&changed).unwrap(), hole);
            assert!(carrier::matches_template_format(&template, &changed).unwrap());
        }
        let mut changed = sealed.clone();
        changed[512] ^= 1;
        assert!(!carrier::matches_template_format(&template, &changed).unwrap());
        for offset in [
            0,
            60,
            64 + 4,
            64 + 24 + 48,
            64 + 24 + 60,
            sealed.len() - 20,
            sealed.len() - 80,
        ] {
            let mut bad = sealed.clone();
            bad[offset] ^= 0x80;
            assert!(check(&bad).is_err(), "offset {offset}");
            assert!(carrier::parse(&bad).is_err());
        }
    }
}
