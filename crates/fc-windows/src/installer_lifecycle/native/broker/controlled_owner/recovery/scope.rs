//! Build-bound owned-fixture authorization data. No runtime argument/file/env
//! supplies it. Its provenance is the trusted compiler's separately frozen input,
//! not a hash or caller-selected success bit. Missing input remains inert.
use super::*;

const MAGIC: &[u8] = b"FC-OWNED-REMOVAL\0v1\0";
const MAX_SCOPE: usize = 40 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ObjectId {
    pub(super) volume: u32,
    pub(super) index: u64,
    pub(super) creation: u64,
}
impl ObjectId {
    pub(super) fn matches(self, handle: HANDLE, directory: bool) -> Result<()> {
        let (volume, index) = verify_info(handle, directory)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe {
            GetFileInformationByHandle(handle, &mut info)?;
        }
        // Creation is only additional held-object consistency. NTFS tunneling
        // can retain it; it never identifies an old owner or proves same boot.
        let creation = u64::from(info.ftCreationTime.dwLowDateTime)
            | (u64::from(info.ftCreationTime.dwHighDateTime) << 32);
        if self
            == (Self {
                volume,
                index,
                creation,
            })
        {
            Ok(())
        } else {
            Err(LifecycleError::Unsettled)
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Terminated {
    pub(super) pid: u32,
    pub(super) creation: u64,
}
pub(super) struct CompileOwnedFixtureScope {
    pub(super) runtime_report: [u8; 32],
    pub(super) package_report: [u8; 32],
    pub(super) pending_report: [u8; 32],
    pub(super) old_image: [u8; 32],
    pub(super) sid: Vec<u8>,
    pub(super) root: ObjectId,
    pub(super) gate: ObjectId,
    pub(super) pending_id: ObjectId,
    pub(super) pending: Vec<u8>,
    pub(super) old_b: Terminated,
    pub(super) old_g: Terminated,
    pub(super) manifest: Manifest,
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        if self.0.len() < count {
            return Err(LifecycleError::Invalid);
        }
        let (value, rest) = self.0.split_at(count);
        self.0 = rest;
        Ok(value)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn digest(&mut self) -> Result<[u8; 32]> {
        let value = self.take(32)?.try_into().unwrap();
        if value == [0; 32] {
            return Err(LifecycleError::Invalid);
        }
        Ok(value)
    }
    fn object(&mut self) -> Result<ObjectId> {
        let value = ObjectId {
            volume: self.u32()?,
            index: self.u64()?,
            creation: self.u64()?,
        };
        if value.volume == 0 || value.index == 0 || value.creation == 0 {
            return Err(LifecycleError::Invalid);
        }
        Ok(value)
    }
    fn terminated(&mut self) -> Result<Terminated> {
        let value = Terminated {
            pid: self.u32()?,
            creation: self.u64()?,
        };
        // These are exact, trusted original-object receipts from the owned
        // failed attempt, not a present-day PID lookup or proof of old End.
        if value.pid == 0
            || value.creation == 0
            || self.u32()? != 0
            || self.u32()? != 1603
            || self.take(1)? != [1]
        {
            return Err(LifecycleError::Invalid);
        }
        Ok(value)
    }
}
impl CompileOwnedFixtureScope {
    pub(super) fn compiled() -> Result<Self> {
        let hex = option_env!("FC_OWNED_REMOVAL_SCOPE_HEX").ok_or(LifecycleError::Invalid)?;
        Self::parse_hex(hex)
    }
    fn parse_hex(hex: &str) -> Result<Self> {
        if hex.is_empty() || hex.len() > MAX_SCOPE * 2 || !hex.len().is_multiple_of(2) {
            return Err(LifecycleError::Invalid);
        }
        fn nibble(value: u8) -> Result<u8> {
            match value {
                b'0'..=b'9' => Ok(value - b'0'),
                b'a'..=b'f' => Ok(value - b'a' + 10),
                _ => Err(LifecycleError::Invalid),
            }
        }
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        for pair in hex.as_bytes().chunks_exact(2) {
            bytes.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
        }
        Self::parse(&bytes)
    }
    fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_SCOPE {
            return Err(LifecycleError::Invalid);
        }
        let mut r = Reader(bytes);
        if r.take(MAGIC.len())? != MAGIC {
            return Err(LifecycleError::Invalid);
        }
        let runtime_report = r.digest()?;
        let package_report = r.digest()?;
        let pending_report = r.digest()?;
        let old_image = r.digest()?;
        let n = usize::from(r.u16()?);
        if !(12..=68).contains(&n) {
            return Err(LifecycleError::Invalid);
        }
        let sid = r.take(n)?.to_vec();
        if sid[0] != 1 || sid[1] > 15 || sid.len() != 8 + usize::from(sid[1]) * 4 {
            return Err(LifecycleError::Invalid);
        }
        let root = r.object()?;
        let gate = r.object()?;
        let pending_id = r.object()?;
        if root.volume != gate.volume
            || root.volume != pending_id.volume
            || root.index == gate.index
            || root.index == pending_id.index
            || gate.index == pending_id.index
        {
            return Err(LifecycleError::Invalid);
        }
        let n = usize::from(r.u16()?);
        if !(1..=MAX_PENDING_BYTES).contains(&n) {
            return Err(LifecycleError::Invalid);
        }
        let pending = r.take(n)?.to_vec();
        validate_pending(&pending, &sid)?;
        let old_b = r.terminated()?;
        let old_g = r.terminated()?;
        if old_b.pid == old_g.pid || old_g.creation < old_b.creation {
            return Err(LifecycleError::Invalid);
        }
        let n = r.u32()? as usize;
        let manifest = manifest::parse(r.take(n)?)?;
        if manifest.entries.len() != 14 || !r.0.is_empty() {
            return Err(LifecycleError::Invalid);
        }
        Ok(Self {
            runtime_report,
            package_report,
            pending_report,
            old_image,
            sid,
            root,
            gate,
            pending_id,
            pending,
            old_b,
            old_g,
            manifest,
        })
    }
    pub(super) fn matches_target(&self, target: &Manifest) -> Result<()> {
        // First bounded cleanup supports the original c508 complete14 only.
        // A changed payload needs separately reviewed dual-manifest migration;
        // silently treating that as SameManifest is forbidden here.
        let same = self.manifest.product == target.product
            && self.manifest.version == target.version
            && self.manifest.entries.len() == target.entries.len()
            && self
                .manifest
                .entries
                .iter()
                .zip(&target.entries)
                .all(|(a, b)| {
                    a.path == b.path
                        && a.bytes == b.bytes
                        && a.sha256 == b.sha256
                        && a.component == b.component
                });
        if same {
            Ok(())
        } else {
            Err(LifecycleError::Invalid)
        }
    }
}
pub(super) fn validate_pending(bytes: &[u8], sid: &[u8]) -> Result<()> {
    let mut r = Reader(bytes);
    let header = b"FC-INSTALL-PENDING\0v1\0";
    if r.take(header.len())? != header
        || r.take(INSTALL_FAMILY.len())? != INSTALL_FAMILY.as_bytes()
        || r.take(1)? != [0]
        || r.take(16)?.iter().all(|&x| x == 0)
        || r.take(1)? != [5]
    {
        return Err(LifecycleError::Invalid);
    }
    let n = usize::from(r.u16()?);
    if r.take(n)? != sid || !r.0.is_empty() {
        return Err(LifecycleError::Invalid);
    }
    Ok(())
}
pub(super) fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(H[usize::from(byte >> 4)]));
        out.push(char::from(H[usize::from(byte & 15)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_removal_scope_never_accepts_unbound_or_runtime_authority_text() {
        for value in ["", "UNBOUND", "00", "AA", "0", "--pid=1", "true"] {
            assert!(CompileOwnedFixtureScope::parse_hex(value).is_err());
        }
        assert!(CompileOwnedFixtureScope::parse_hex(&"00".repeat(MAX_SCOPE + 1)).is_err());
    }
    #[test]
    fn owned_removal_pending_requires_exact_original_schema_sid_and_tag5() {
        let sid = [1, 1, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0];
        let mut p = b"FC-INSTALL-PENDING\0v1\0".to_vec();
        p.extend(INSTALL_FAMILY.as_bytes());
        p.push(0);
        p.extend([7; 16]);
        p.push(5);
        p.extend((sid.len() as u16).to_le_bytes());
        p.extend(sid);
        validate_pending(&p, &sid).unwrap();
        let mut altered = p.clone();
        altered.push(0);
        assert!(validate_pending(&altered, &sid).is_err());
        let tag = p.len() - sid.len() - 3;
        altered = p.clone();
        altered[tag] = 1;
        assert!(validate_pending(&altered, &sid).is_err());
        altered = p.clone();
        *altered.last_mut().unwrap() ^= 1;
        assert!(validate_pending(&altered, &sid).is_err());
        for n in 0..p.len() {
            assert!(validate_pending(&p[..n], &sid).is_err());
        }
    }
    fn complete_scope() -> Vec<u8> {
        let sid = [1, 1, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0];
        let mut pending = b"FC-INSTALL-PENDING\0v1\0".to_vec();
        pending.extend(INSTALL_FAMILY.as_bytes());
        pending.push(0);
        pending.extend([7; 16]);
        pending.push(5);
        pending.extend((sid.len() as u16).to_le_bytes());
        pending.extend(sid);
        let mut names = [
            "LICENSE",
            "MSI-CANDIDATE.txt",
            "PROBE-README.txt",
            "THIRD-PARTY-NOTICES.txt",
            "fc-bootstrap.exe",
            "manifest.json",
            "web/THIRD-PARTY-NOTICES.txt",
            "web/assets/a.js",
            "web/assets/b.js",
            "web/assets/c.css",
            "web/assets/d.js",
            "web/assets/e.svg",
            "web/fc-assets.json",
            "web/index.html",
        ];
        names.sort();
        let mut manifest = manifest::MAGIC.to_vec();
        manifest.extend(1u16.to_le_bytes());
        manifest.extend(14u16.to_le_bytes());
        manifest.extend(b"12345678-1234-1234-1234-123456789ABC");
        manifest.push(5);
        manifest.extend(b"0.1.0");
        for (i, name) in names.iter().enumerate() {
            manifest.extend((name.len() as u16).to_le_bytes());
            manifest.extend(name.as_bytes());
            manifest.extend(1u32.to_le_bytes());
            manifest.extend([1; 32]);
            manifest.extend(format!("00000000-0000-0000-0000-{:012X}", i + 1).as_bytes());
        }
        let mut bytes = MAGIC.to_vec();
        bytes.extend([1; 128]);
        bytes.extend((sid.len() as u16).to_le_bytes());
        bytes.extend(sid);
        for index in 1..=3u64 {
            bytes.extend(1u32.to_le_bytes());
            bytes.extend(index.to_le_bytes());
            bytes.extend(2u64.to_le_bytes());
        }
        bytes.extend((pending.len() as u16).to_le_bytes());
        bytes.extend(pending);
        for (pid, time) in [(4u32, 5u64), (6, 7)] {
            bytes.extend(pid.to_le_bytes());
            bytes.extend(time.to_le_bytes());
            bytes.extend(0u32.to_le_bytes());
            bytes.extend(1603u32.to_le_bytes());
            bytes.push(1);
        }
        bytes.extend((manifest.len() as u32).to_le_bytes());
        bytes.extend(manifest);
        bytes
    }
    #[test]
    fn owned_removal_scope_closes_full_manifest_receipts_and_object_identity() {
        let bytes = complete_scope();
        let scope = CompileOwnedFixtureScope::parse(&bytes).unwrap();
        scope.matches_target(&scope.manifest).unwrap();
        assert_eq!(scope.manifest.entries.len(), 14);
        CompileOwnedFixtureScope::parse_hex(&hex(&bytes)).unwrap();
        for n in 0..bytes.len() {
            assert!(
                CompileOwnedFixtureScope::parse(&bytes[..n]).is_err(),
                "truncated {n}"
            );
        }
        let mut altered = bytes.clone();
        altered.push(0);
        assert!(CompileOwnedFixtureScope::parse(&altered).is_err());
        let objects = MAGIC.len() + 128 + 2 + scope.sid.len();
        altered = bytes.clone();
        let duplicate = altered[objects..objects + 20].to_vec();
        altered[objects + 20..objects + 40].copy_from_slice(&duplicate);
        assert!(CompileOwnedFixtureScope::parse(&altered).is_err());
        let receipt = objects + 60 + 2 + scope.pending.len();
        for offset in [12, 16, 20] {
            altered = bytes.clone();
            altered[receipt + offset] ^= 1;
            assert!(CompileOwnedFixtureScope::parse(&altered).is_err());
        }
        let mut changed = CompileOwnedFixtureScope::parse(&bytes).unwrap();
        changed.manifest.entries[0].sha256[0] ^= 1;
        assert!(scope.matches_target(&changed.manifest).is_err());
    }
}
