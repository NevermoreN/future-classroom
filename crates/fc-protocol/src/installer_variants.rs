//! Exact current-release source variants. COPY ranges refer only to the current
//! base source, never installed files. Reconstructed bytes retain native seals.
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const MAGIC: &[u8; 16] = b"FC-MSI-VARIANTS1";
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_SOURCE: usize = 128 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidVariants;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub bytes: usize,
    pub sha256: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub package_code: String,
    pub predecessor_version: String,
    pub predecessor_product: String,
    pub predecessor_msi_sha256: String,
    pub setup: Source,
    pub msi: Source,
    pub setup_delta_bytes: usize,
    pub msi_delta_bytes: usize,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub base_package_code: String,
    pub base_predecessor_version: String,
    pub base_predecessor_product: String,
    pub base_predecessor_msi_sha256: String,
    pub base_setup: Source,
    pub base_msi: Source,
    pub variants: Vec<Variant>,
}
pub struct Package<'a> {
    pub catalog: Catalog,
    patches: &'a [u8],
}
fn hash_valid(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn guid_valid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(n, c)| {
            if [8, 13, 18, 23].contains(&n) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'A'..=b'F').contains(&c)
            }
        })
}
fn source_valid(s: &Source, limit: usize) -> bool {
    s.bytes > 0 && s.bytes <= limit && hash_valid(&s.sha256)
}
fn matches(source: &Source, bytes: &[u8]) -> bool {
    source.bytes == bytes.len() && format!("{:x}", Sha256::digest(bytes)) == source.sha256
}
impl<'a> Package<'a> {
    pub fn parse(raw: &'a [u8]) -> Result<Self, InvalidVariants> {
        if raw.len() < 20 || raw.len() > MAX_BYTES || &raw[..16] != MAGIC {
            return Err(InvalidVariants);
        }
        let length =
            u32::from_le_bytes(raw[16..20].try_into().map_err(|_| InvalidVariants)?) as usize;
        if length == 0 || length > 65536 || length > raw.len() - 20 {
            return Err(InvalidVariants);
        }
        let catalog: Catalog =
            serde_json::from_slice(&raw[20..20 + length]).map_err(|_| InvalidVariants)?;
        if !guid_valid(&catalog.base_package_code)
            || !guid_valid(&catalog.base_predecessor_product)
            || !hash_valid(&catalog.base_predecessor_msi_sha256)
            || catalog.base_predecessor_version.is_empty()
            || catalog.base_predecessor_version.len() > 32
            || !catalog
                .base_predecessor_version
                .bytes()
                .all(|c| c.is_ascii_digit() || c == b'.')
            || !source_valid(&catalog.base_setup, 32 << 20)
            || !source_valid(&catalog.base_msi, MAX_SOURCE)
            || catalog.variants.is_empty()
            || catalog.variants.len() > 16
        {
            return Err(InvalidVariants);
        }
        let mut size = 0usize;
        for (index, v) in catalog.variants.iter().enumerate() {
            if !guid_valid(&v.package_code)
                || v.package_code == catalog.base_package_code
                || !guid_valid(&v.predecessor_product)
                || !hash_valid(&v.predecessor_msi_sha256)
                || v.predecessor_version.len() > 32
                || v.predecessor_version.is_empty()
                || !v
                    .predecessor_version
                    .bytes()
                    .all(|c| c.is_ascii_digit() || c == b'.')
                || !source_valid(&v.setup, 32 << 20)
                || !source_valid(&v.msi, MAX_SOURCE)
                || v.setup_delta_bytes < 4
                || v.msi_delta_bytes < 4
                || catalog.variants[..index].iter().any(|p| {
                    p.package_code == v.package_code
                        || (p.predecessor_product == v.predecessor_product
                            && p.predecessor_msi_sha256 == v.predecessor_msi_sha256)
                })
                || (v.predecessor_product == catalog.base_predecessor_product
                    && v.predecessor_msi_sha256 == catalog.base_predecessor_msi_sha256)
            {
                return Err(InvalidVariants);
            }
            size = size
                .checked_add(v.setup_delta_bytes)
                .and_then(|s| s.checked_add(v.msi_delta_bytes))
                .ok_or(InvalidVariants)?;
        }
        let patches = &raw[20 + length..];
        if size != patches.len() {
            return Err(InvalidVariants);
        }
        Ok(Self { catalog, patches })
    }
    pub fn validate_base(&self, setup: &[u8], msi: &[u8]) -> Result<(), InvalidVariants> {
        if !matches(&self.catalog.base_setup, setup) || !matches(&self.catalog.base_msi, msi) {
            return Err(InvalidVariants);
        }
        Ok(())
    }
    pub fn materialize(
        &self,
        index: usize,
        setup: &[u8],
        msi: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), InvalidVariants> {
        self.validate_base(setup, msi)?;
        let v = self.catalog.variants.get(index).ok_or(InvalidVariants)?;
        let start: usize = self.catalog.variants[..index]
            .iter()
            .map(|p| p.setup_delta_bytes + p.msi_delta_bytes)
            .sum();
        let split = start + v.setup_delta_bytes;
        Ok((
            apply(setup, &self.patches[start..split], &v.setup)?,
            apply(msi, &self.patches[split..split + v.msi_delta_bytes], &v.msi)?,
        ))
    }
}
fn number(raw: &[u8], offset: &mut usize) -> Result<usize, InvalidVariants> {
    let end = offset.checked_add(4).ok_or(InvalidVariants)?;
    let value = u32::from_le_bytes(
        raw.get(*offset..end)
            .ok_or(InvalidVariants)?
            .try_into()
            .map_err(|_| InvalidVariants)?,
    ) as usize;
    *offset = end;
    Ok(value)
}
fn apply(base: &[u8], patch: &[u8], target: &Source) -> Result<Vec<u8>, InvalidVariants> {
    let mut offset = 0;
    let count = number(patch, &mut offset)?;
    if count == 0 || count > 65536 {
        return Err(InvalidVariants);
    }
    let mut output = Vec::with_capacity(target.bytes);
    for _ in 0..count {
        let tag = *patch.get(offset).ok_or(InvalidVariants)?;
        offset += 1;
        let start = if tag == 0 {
            number(patch, &mut offset)?
        } else {
            0
        };
        let length = number(patch, &mut offset)?;
        if length == 0 || length > target.bytes - output.len() {
            return Err(InvalidVariants);
        }
        let data = match tag {
            0 => base
                .get(start..start.checked_add(length).ok_or(InvalidVariants)?)
                .ok_or(InvalidVariants)?,
            1 => {
                let end = offset.checked_add(length).ok_or(InvalidVariants)?;
                let data = patch.get(offset..end).ok_or(InvalidVariants)?;
                offset = end;
                data
            }
            _ => return Err(InvalidVariants),
        };
        output.extend_from_slice(data);
    }
    if offset != patch.len() || !matches(target, &output) {
        return Err(InvalidVariants);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(raw: &[u8]) -> serde_json::Value {
        serde_json::json!({"bytes":raw.len(),"sha256":format!("{:x}",Sha256::digest(raw))})
    }
    fn fixture() -> Vec<u8> {
        // COPY the original two bytes, then insert a new byte. MSI and Setup
        // have independent exact target digests, not only a container checksum.
        let patch = [2, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1, 1, 0, 0, 0, b'!'];
        let metadata = serde_json::to_vec(&serde_json::json!({
        "base_package_code":"11111111-1111-1111-1111-111111111111",
            "base_predecessor_version":"1.1.0",
            "base_predecessor_product":"44444444-4444-4444-4444-444444444444",
            "base_predecessor_msi_sha256":"2".repeat(64),
        "base_setup":source(b"ab"),"base_msi":source(b"ab"),"variants":[{
            "package_code":"22222222-2222-2222-2222-222222222222",
            "predecessor_version":"1.0.2",
            "predecessor_product":"33333333-3333-3333-3333-333333333333",
            "predecessor_msi_sha256":"1".repeat(64),
            "setup":source(b"ab!"),"msi":source(b"ab!"),
            "setup_delta_bytes":patch.len(),"msi_delta_bytes":patch.len()
        }]}))
        .unwrap();
        let mut raw = MAGIC.to_vec();
        raw.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        raw.extend_from_slice(&metadata);
        raw.extend_from_slice(&patch);
        raw.extend_from_slice(&patch);
        raw
    }
    #[test]
    fn reconstruct_exact_pair_and_reject_wrong_source_tamper_and_truncation() {
        let raw = fixture();
        let package = Package::parse(&raw).unwrap();
        assert_eq!(
            package.materialize(0, b"ab", b"ab").unwrap(),
            (b"ab!".to_vec(), b"ab!".to_vec())
        );
        assert!(package.materialize(0, b"ac", b"ab").is_err());
        assert!(package.materialize(1, b"ab", b"ab").is_err());
        let mut changed = raw.clone();
        *changed.last_mut().unwrap() = b'?';
        assert!(Package::parse(&changed)
            .unwrap()
            .materialize(0, b"ab", b"ab")
            .is_err());
        assert!(Package::parse(&raw[..raw.len() - 1]).is_err());
        let mut trailing = raw;
        trailing.push(0);
        assert!(Package::parse(&trailing).is_err());
    }
    #[test]
    fn copied_ranges_and_operation_budget_are_bounded() {
        let target = Source {
            bytes: 3,
            sha256: format!("{:x}", Sha256::digest(b"abc")),
        };
        assert!(apply(
            b"abc",
            &[1, 0, 0, 0, 0, 255, 255, 255, 255, 3, 0, 0, 0],
            &target
        )
        .is_err());
        assert!(apply(b"abc", &[255, 255, 255, 255], &target).is_err());
        assert!(apply(
            b"abc",
            &[1, 0, 0, 0, 1, 4, 0, 0, 0, b'a', b'b', b'c', b'd'],
            &target
        )
        .is_err());
    }
}
