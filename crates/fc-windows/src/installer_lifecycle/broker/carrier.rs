//! ABI2 carrier byte format only; not yet wired to Session/CA/Setup execution.
//! Parsing or filling a digest grants no host provenance or installation right.
//! Native callers must hash the complete pinned MSI with the existing owner and
//! independently match a compiled PublishedInstallerHost; no client digest input.
use super::super::{LifecycleError, Result, INSTALL_FAMILY};
use super::manifest;

pub(crate) const MAGIC: &[u8; 8] = b"FCCAR2\0\0";
pub(crate) const FOOTER: &[u8; 16] = b"FCCR2-END-v2\0\0\0\0";
pub(crate) const SEAL_MAGIC: &[u8; 16] = b"FC-SETUP-MSI\0v1\0";
pub(crate) const ABI: u16 = 2;
pub(crate) const DESCRIPTOR_BYTES: usize = 64;
pub(crate) const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const TRAILER_BYTES: usize = 48 + DESCRIPTOR_BYTES + 4 + FOOTER.len();
pub(crate) const MIN_IMAGE_BYTES: usize = TRAILER_BYTES + 64;

pub(crate) struct CapturedCarrier<'a> {
    prefix: &'a [u8],
    digest: [u8; 32],
}
impl CapturedCarrier<'_> {
    pub(crate) fn normal_helper(&self) -> &[u8] {
        self.prefix
    }
    pub(crate) fn digest_format_value(&self) -> [u8; 32] {
        self.digest
    }
    pub(crate) fn digest_offset(&self) -> usize {
        self.prefix.len() + SEAL_MAGIC.len()
    }
}
fn invalid<T>() -> Result<T> {
    Err(LifecycleError::Invalid)
}
fn u16_at(b: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        b.get(at..at.checked_add(2).ok_or(LifecycleError::Invalid)?)
            .ok_or(LifecycleError::Invalid)?
            .try_into()
            .map_err(|_| LifecycleError::Invalid)?,
    ))
}
fn u32_at(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(at..at.checked_add(4).ok_or(LifecycleError::Invalid)?)
            .ok_or(LifecycleError::Invalid)?
            .try_into()
            .map_err(|_| LifecycleError::Invalid)?,
    ))
}
/// All64 descriptor bytes are fixed within ABI2; no product/version or digest.
pub(crate) fn descriptor() -> [u8; DESCRIPTOR_BYTES] {
    let mut b = [0u8; DESCRIPTOR_BYTES];
    b[..8].copy_from_slice(MAGIC);
    b[8..10].copy_from_slice(&ABI.to_le_bytes());
    b[10..12].copy_from_slice(&(DESCRIPTOR_BYTES as u16).to_le_bytes());
    b[12..48].copy_from_slice(INSTALL_FAMILY.as_bytes());
    for (offset, value) in [(48, 120u16), (50, 64), (52, 8), (54, 72)] {
        b[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    b[56..60].copy_from_slice(&(30u32 * 1024 * 1024).to_le_bytes());
    b
}
/// Structural PE32/I386 GUI closure, not publisher/compiler provenance. The
/// complete prefix including headers, sections and any certificate is retained.
/// No existing lifecycle overlay, nested carrier or arbitrary extra tail fits.
fn normal_prefix(b: &[u8]) -> Result<()> {
    if b.len() < 64 || b.len() > MAX_IMAGE_BYTES || &b[..2] != b"MZ" {
        return invalid();
    }
    if b.ends_with(manifest::FOOTER)
        || b.ends_with(FOOTER)
        || (b.len() >= 48 && &b[b.len() - 48..b.len() - 32] == SEAL_MAGIC)
    {
        return invalid();
    }
    let pe = u32_at(b, 60)? as usize;
    if pe < 64
        || b.get(pe..pe.checked_add(4).ok_or(LifecycleError::Invalid)?) != Some(b"PE\0\0")
        || u16_at(b, pe + 4)? != 0x14c
    {
        return invalid();
    }
    let count = usize::from(u16_at(b, pe + 6)?);
    let optional = pe.checked_add(24).ok_or(LifecycleError::Invalid)?;
    let optional_bytes = usize::from(u16_at(b, pe + 20)?);
    if !(1..=96).contains(&count)
        || optional_bytes < 224
        || u16_at(b, optional)? != 0x10b
        || u16_at(b, optional + 68)? != 2
        || u16_at(b, optional + 48)? != 6
        || u16_at(b, optional + 50)? != 1
    {
        return invalid();
    }
    let sections = optional
        .checked_add(optional_bytes)
        .ok_or(LifecycleError::Invalid)?;
    let table_end = sections
        .checked_add(count * 40)
        .ok_or(LifecycleError::Invalid)?;
    let headers = u32_at(b, optional + 60)? as usize;
    if headers < table_end || headers > b.len() {
        return invalid();
    }
    let mut ranges = Vec::with_capacity(count);
    let mut end = headers;
    for i in 0..count {
        let at = sections + i * 40;
        let size = u32_at(b, at + 16)? as usize;
        let offset = u32_at(b, at + 20)? as usize;
        if size == 0 {
            continue;
        }
        let next = offset.checked_add(size).ok_or(LifecycleError::Invalid)?;
        if offset < headers || next > b.len() || ranges.iter().any(|&(a, z)| offset < z && next > a)
        {
            return invalid();
        }
        ranges.push((offset, next));
        end = end.max(next);
    }
    // PE security directory uses file offsets, not RVAs. It is compared as
    // ordinary immutable bytes; signing is optional and never new authority.
    let directories = u32_at(b, optional + 92)?;
    if directories > 16 {
        return invalid();
    }
    if directories > 4 {
        let certificate = u32_at(b, optional + 128)? as usize;
        let size = u32_at(b, optional + 132)? as usize;
        if certificate != 0 || size != 0 {
            if certificate < end
                || !certificate.is_multiple_of(8)
                || size < 8
                || certificate.checked_add(size) != Some(b.len())
            {
                return invalid();
            }
            end = b.len();
        }
    }
    if end != b.len() {
        return invalid();
    }
    Ok(())
}
pub(crate) fn parse(image: &[u8]) -> Result<CapturedCarrier<'_>> {
    if image.len() > MAX_IMAGE_BYTES || image.len() < MIN_IMAGE_BYTES {
        return invalid();
    }
    let trailer = image.len() - 20;
    if &image[trailer + 4..] != FOOTER || u32_at(image, trailer)? != DESCRIPTOR_BYTES as u32 {
        return invalid();
    }
    let descriptor_start = trailer
        .checked_sub(DESCRIPTOR_BYTES)
        .ok_or(LifecycleError::Invalid)?;
    let seal = descriptor_start
        .checked_sub(48)
        .ok_or(LifecycleError::Invalid)?;
    if image[descriptor_start..trailer] != descriptor() || &image[seal..seal + 16] != SEAL_MAGIC {
        return invalid();
    }
    normal_prefix(&image[..seal])?;
    let digest = image[seal + 16..descriptor_start]
        .try_into()
        .map_err(|_| LifecycleError::Invalid)?;
    Ok(CapturedCarrier {
        prefix: &image[..seal],
        digest,
    })
}
pub(crate) fn make_template(normal_helper: &[u8]) -> Result<Vec<u8>> {
    normal_prefix(normal_helper)?;
    let length = normal_helper
        .len()
        .checked_add(TRAILER_BYTES)
        .ok_or(LifecycleError::Invalid)?;
    if length > MAX_IMAGE_BYTES {
        return invalid();
    }
    let mut image = Vec::with_capacity(length);
    image.extend(normal_helper);
    image.extend(SEAL_MAGIC);
    image.extend([0; 32]);
    image.extend(descriptor());
    image.extend((DESCRIPTOR_BYTES as u32).to_le_bytes());
    image.extend(FOOTER);
    Ok(image)
}
/// Pure packaging operation on a captured digest FORMAT VALUE. It neither
/// computes nor authenticates that digest, and does not construct Published or
/// Verified types. Production use remains unconnected; only owner-hashed whole
/// MSI bytes / root builder's full-byte SHA may supply this fixed32-byte slot.
pub(crate) fn fill_digest_format_value(
    template: &[u8],
    whole_msi_digest: [u8; 32],
) -> Result<Vec<u8>> {
    let parsed = parse(template)?;
    if parsed.digest != [0; 32] || whole_msi_digest == [0; 32] {
        return invalid();
    }
    let at = parsed.digest_offset();
    let mut out = template.to_vec();
    out[at..at + 32].copy_from_slice(&whole_msi_digest);
    Ok(out)
}
/// Compare EVERY image byte except the original fixed zero32 seal slot. This is
/// necessary structural evidence, never sufficient Published-host authority.
pub(crate) fn matches_template_format(template: &[u8], sealed: &[u8]) -> Result<bool> {
    let a = parse(template)?;
    let b = parse(sealed)?;
    if a.digest != [0; 32] || b.digest == [0; 32] || template.len() != sealed.len() {
        return invalid();
    }
    let at = a.digest_offset();
    Ok(at == b.digest_offset()
        && template[..at] == sealed[..at]
        && template[at + 32..] == sealed[at + 32..])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(in super::super) fn pe_fixture() -> Vec<u8> {
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
    #[test]
    fn abi2_carrier_roundtrip_fills_only_original_zero32_slot() {
        let prefix = pe_fixture();
        let template = make_template(&prefix).unwrap();
        let parsed = parse(&template).unwrap();
        assert_eq!(parsed.normal_helper(), prefix);
        assert_eq!(parsed.digest_format_value(), [0; 32]);
        let sealed = fill_digest_format_value(&template, [7; 32]).unwrap();
        assert!(matches_template_format(&template, &sealed).unwrap());
        let hole = parsed.digest_offset();
        assert_eq!(
            template.iter().zip(&sealed).filter(|(a, b)| a != b).count(),
            32
        );
        assert_eq!(&sealed[hole..hole + 32], &[7; 32]);
        assert!(fill_digest_format_value(&sealed, [8; 32]).is_err());
        assert!(fill_digest_format_value(&template, [0; 32]).is_err());
    }
    #[test]
    fn abi2_carrier_does_not_normalize_pe_or_descriptor_bytes() {
        let template = make_template(&pe_fixture()).unwrap();
        let mut sealed = fill_digest_format_value(&template, [1; 32]).unwrap();
        sealed[600] = 9;
        assert!(!matches_template_format(&template, &sealed).unwrap());
        for at in [0, 8, 10, 12, 48, 50, 52, 54, 56, 60] {
            let mut b = template.clone();
            b[1024 + 48 + at] ^= 1;
            assert!(parse(&b).is_err());
        }
    }
    #[test]
    fn abi2_carrier_rejects_every_truncation_extra_tail_and_nested_overlay() {
        let template = make_template(&pe_fixture()).unwrap();
        for n in 0..template.len() {
            assert!(parse(&template[..n]).is_err());
        }
        let mut extra = template.clone();
        extra.push(0);
        assert!(parse(&extra).is_err());
        assert!(make_template(&template).is_err());
        let mut old = pe_fixture();
        old.extend(b"FCLIFE73-END-v1\0");
        assert!(make_template(&old).is_err());
        assert!(parse(&old).is_err());
    }
    #[test]
    fn abi2_carrier_pe_bounds_machine_subsystem_and_section_range_are_closed() {
        for at in [60, 68, 70, 84, 88, 148, 156, 328, 332] {
            let mut b = pe_fixture();
            b[at] = 255;
            assert!(make_template(&b).is_err());
        }
        let mut b = pe_fixture();
        b[332..336].copy_from_slice(&500u32.to_le_bytes());
        assert!(make_template(&b).is_err());
    }
}
