use super::*;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{ErrorKind, Seek, SeekFrom},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
};
const MAX_OBJECTS: usize = 100_000;
pub(super) const MAX_CLEANUP_MANIFEST: usize = 32 * 1024 * 1024;
const MANIFEST_MAGIC: &[u8; 16] = b"FCRESETFILES1\0\0\0";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    pub identity: Identity,
    pub digest: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ObjectRecord {
    pub relative: PathBuf,
    pub directory: bool,
    pub identity: Identity,
    pub length: u64,
    pub hash: [u8; 32],
}
impl ObjectRecord {
    fn encode(&self, out: &mut Vec<u8>) {
        let name: Vec<u16> = self.relative.as_os_str().encode_wide().collect();
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        for unit in name {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.push(u8::from(self.directory));
        out.extend_from_slice(&self.identity.0.to_le_bytes());
        out.extend_from_slice(&self.identity.1.to_le_bytes());
        out.extend_from_slice(&self.length.to_le_bytes());
        out.extend_from_slice(&self.hash);
    }
}
pub(super) struct Snapshot {
    pub stamp: Stamp,
    pub records: Vec<ObjectRecord>,
}
impl Snapshot {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = MANIFEST_MAGIC.to_vec();
        out.extend_from_slice(&(self.records.len() as u32).to_le_bytes());
        for record in &self.records {
            record.encode(&mut out);
            if out.len() > MAX_CLEANUP_MANIFEST - 32 {
                return Err(ReinitializationFileError::RepairRequired);
            }
        }
        let hash = fc_security::sha256(&out);
        out.extend_from_slice(&hash);
        Ok(out)
    }
    pub fn decode(bytes: &[u8], expected: Stamp) -> Result<Self> {
        fn take<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
            if input.len() < n {
                return Err(ReinitializationFileError::RepairRequired);
            }
            let (head, tail) = input.split_at(n);
            *input = tail;
            Ok(head)
        }
        if bytes.len() < 52
            || bytes.len() > MAX_CLEANUP_MANIFEST
            || &bytes[..16] != MANIFEST_MAGIC
            || fc_security::sha256(&bytes[..bytes.len() - 32]) != bytes[bytes.len() - 32..]
        {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let mut input = &bytes[16..bytes.len() - 32];
        let count = u32::from_le_bytes(take(&mut input, 4)?.try_into().map_err(repair)?) as usize;
        if count == 0 || count > MAX_OBJECTS {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let mut records = Vec::with_capacity(count);
        for n in 0..count {
            let units =
                u32::from_le_bytes(take(&mut input, 4)?.try_into().map_err(repair)?) as usize;
            if units > 16000 {
                return Err(ReinitializationFileError::RepairRequired);
            }
            let name: Vec<u16> = take(&mut input, units * 2)?
                .chunks_exact(2)
                .map(|v| u16::from_le_bytes([v[0], v[1]]))
                .collect();
            if name.iter().any(|v| matches!(*v, 0 | 58)) {
                return Err(ReinitializationFileError::RepairRequired);
            }
            let relative = PathBuf::from(std::ffi::OsString::from_wide(&name));
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                || relative.as_os_str().encode_wide().collect::<Vec<_>>() != name
            {
                return Err(ReinitializationFileError::RepairRequired);
            }
            let directory = match take(&mut input, 1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(ReinitializationFileError::RepairRequired),
            };
            let identity = (
                u32::from_le_bytes(take(&mut input, 4)?.try_into().map_err(repair)?),
                u64::from_le_bytes(take(&mut input, 8)?.try_into().map_err(repair)?),
            );
            let length = u64::from_le_bytes(take(&mut input, 8)?.try_into().map_err(repair)?);
            let hash = take(&mut input, 32)?.try_into().map_err(repair)?;
            if (directory && (length != 0 || hash != [0; 32]))
                || (relative.as_os_str().is_empty() != (n == count - 1))
                || (n == count - 1 && (!directory || identity != expected.identity))
            {
                return Err(ReinitializationFileError::RepairRequired);
            }
            records.push(ObjectRecord {
                relative,
                directory,
                identity,
                length,
                hash,
            });
        }
        if !input.is_empty() {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let mut names = BTreeSet::new();
        let mut identities = BTreeSet::new();
        for record in &records {
            if !names.insert(&record.relative) || !identities.insert(record.identity) {
                return Err(ReinitializationFileError::RepairRequired);
            }
        }
        if chain(&records) != expected.digest {
            return Err(ReinitializationFileError::RepairRequired);
        }
        Ok(Self {
            stamp: expected,
            records,
        })
    }
}
fn chain(records: &[ObjectRecord]) -> [u8; 32] {
    let mut digest = fc_security::sha256(b"FC_REINITIALIZATION_TREE_V1");
    for record in records {
        let mut item = digest.to_vec();
        record.encode(&mut item);
        digest = fc_security::sha256(&item);
    }
    digest
}
pub(super) fn entries(files: &ControlledFiles, path: &Path, limit: usize) -> Result<Vec<PathBuf>> {
    if path != files.root() {
        files.check(path).map_err(repair)?;
    }
    let _pin = PinnedDirectory::open(path).map_err(repair)?;
    let mut result = Vec::new();
    let mut allocation = 0usize;
    for entry in std::fs::read_dir(path).map_err(repair)? {
        if result.len() == limit {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let path = entry.map_err(repair)?.path();
        files.check(&path).map_err(repair)?;
        allocation = allocation
            .checked_add(path.as_os_str().len().saturating_mul(2))
            .ok_or(ReinitializationFileError::RepairRequired)?;
        if allocation > MAX_CLEANUP_MANIFEST
            || std::fs::symlink_metadata(&path)
                .map_err(repair)?
                .file_attributes()
                & 0x400
                != 0
        {
            return Err(ReinitializationFileError::RepairRequired);
        }
        result.push(path);
    }
    Ok(result)
}

fn raw_digest(file: &mut File, check: &dyn Fn() -> bool) -> Result<(u64, [u8; 32])> {
    let expected = file.metadata().map_err(repair)?.len();
    file.seek(SeekFrom::Start(0)).map_err(repair)?;
    let mut read = 0u64;
    let mut block = [0; 32 * 1024];
    let mut hash = fc_security::Sha256Stream::default();
    loop {
        ensure(check)?;
        let n = match file.read(&mut block) {
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(repair(e)),
        };
        if n == 0 {
            break;
        }
        read = read
            .checked_add(n as u64)
            .ok_or(ReinitializationFileError::RepairRequired)?;
        if read > expected {
            return Err(ReinitializationFileError::RepairRequired);
        }
        hash.update(&block[..n]);
    }
    if read != expected {
        return Err(ReinitializationFileError::RepairRequired);
    }
    Ok((read, hash.finish()))
}
/// Postorder records include every original inode and exact raw bytes. Empty
/// corrupt credentials/database files require no SQL or DPAPI interpretation.
pub(super) fn snapshot(
    files: &ControlledFiles,
    path: &Path,
    check: &dyn Fn() -> bool,
) -> Result<Option<Snapshot>> {
    if !exists(path)? {
        return Ok(None);
    }
    struct Limits {
        remaining: usize,
        encoded: usize,
    }
    fn walk(
        files: &ControlledFiles,
        base: &Path,
        path: &Path,
        records: &mut Vec<ObjectRecord>,
        limits: &mut Limits,
        depth: usize,
        check: &dyn Fn() -> bool,
    ) -> Result<()> {
        ensure(check)?;
        if limits.remaining == 0 || depth > 64 {
            return Err(ReinitializationFileError::RepairRequired);
        }
        limits.remaining -= 1;
        let pin = OwnedObjectPin::directory(path).map_err(repair)?;
        let mut children = entries(files, path, 32768)?;
        children.sort();
        for child in children {
            ensure(check)?;
            if std::fs::symlink_metadata(&child).map_err(repair)?.is_dir() {
                walk(files, base, &child, records, limits, depth + 1, check)?;
            } else {
                if limits.remaining == 0 {
                    return Err(ReinitializationFileError::RepairRequired);
                }
                limits.remaining -= 1;
                let mut file = OwnedObjectPin::file_allow_empty(&child).map_err(repair)?;
                let (length, hash) = raw_digest(file.file_mut(), check)?;
                push(
                    records,
                    limits,
                    ObjectRecord {
                        relative: child.strip_prefix(base).map_err(repair)?.to_owned(),
                        directory: false,
                        identity: file.identity(),
                        length,
                        hash,
                    },
                )?;
            }
        }
        push(
            records,
            limits,
            ObjectRecord {
                relative: path.strip_prefix(base).map_err(repair)?.to_owned(),
                directory: true,
                identity: pin.identity(),
                length: 0,
                hash: [0; 32],
            },
        )
    }
    fn push(
        records: &mut Vec<ObjectRecord>,
        limits: &mut Limits,
        record: ObjectRecord,
    ) -> Result<()> {
        let units = record.relative.as_os_str().encode_wide().count();
        if units > 16000 {
            return Err(ReinitializationFileError::RepairRequired);
        }
        limits.encoded = limits
            .encoded
            .checked_add(57 + units * 2)
            .ok_or(ReinitializationFileError::RepairRequired)?;
        if limits.encoded > MAX_CLEANUP_MANIFEST - 52 {
            return Err(ReinitializationFileError::RepairRequired);
        }
        records.push(record);
        Ok(())
    }
    let mut records = Vec::new();
    walk(
        files,
        path,
        path,
        &mut records,
        &mut Limits {
            remaining: MAX_OBJECTS,
            encoded: 0,
        },
        0,
        check,
    )?;
    let mut identities = BTreeSet::new();
    if records.iter().any(|r| !identities.insert(r.identity)) {
        return Err(ReinitializationFileError::RepairRequired);
    }
    let stamp = Stamp {
        identity: records
            .last()
            .ok_or(ReinitializationFileError::RepairRequired)?
            .identity,
        digest: chain(&records),
    };
    Ok(Some(Snapshot { stamp, records }))
}
pub(super) fn stamp(
    files: &ControlledFiles,
    path: &Path,
    check: &dyn Fn() -> bool,
) -> Result<Option<Stamp>> {
    Ok(snapshot(files, path, check)?.map(|v| v.stamp))
}
/// Verify remaining objects are a subset of the originally journaled objects,
/// allowing an earlier cleanup's deletions, and refusing additions/replacements.
pub(super) fn remove_recorded(
    files: &ControlledFiles,
    path: &Path,
    expected: &Snapshot,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    let Some(current) = snapshot(files, path, check)? else {
        return Ok(());
    };
    if current.stamp.identity != expected.stamp.identity {
        return Err(ReinitializationFileError::RepairRequired);
    }
    let mut original: Vec<_> = expected.records.iter().collect();
    original.sort_by(|a, b| a.relative.cmp(&b.relative));
    for record in &current.records {
        let Ok(index) = original.binary_search_by(|r| r.relative.cmp(&record.relative)) else {
            return Err(ReinitializationFileError::RepairRequired);
        };
        if *original[index] != *record {
            return Err(ReinitializationFileError::RepairRequired);
        }
    }
    for record in &expected.records {
        ensure(check)?;
        let target = path.join(&record.relative);
        if !exists(&target)? {
            continue;
        }
        let mut failure = None;
        let _ = files
            .remove_object_expected(&target, record.directory, record.identity, |file| {
                if record.directory {
                    return check();
                }
                match raw_digest(file, check) {
                    Ok(actual) if actual == (record.length, record.hash) => true,
                    Ok(_) => {
                        failure = Some(ReinitializationFileError::RepairRequired);
                        false
                    }
                    Err(e) => {
                        failure = Some(e);
                        false
                    }
                }
            })
            .map_err(repair)?;
        if let Some(error) = failure {
            return Err(error);
        }
        ensure(check)?;
    }
    if exists(path)? {
        return Err(ReinitializationFileError::RepairRequired);
    }
    Ok(())
}
pub(super) fn empty_snapshot(expected: Stamp) -> Result<Snapshot> {
    let records = vec![ObjectRecord {
        relative: PathBuf::new(),
        directory: true,
        identity: expected.identity,
        length: 0,
        hash: [0; 32],
    }];
    if chain(&records) != expected.digest {
        return Err(ReinitializationFileError::RepairRequired);
    }
    Ok(Snapshot {
        stamp: expected,
        records,
    })
}

pub(super) fn manifest_checksum(bytes: &[u8]) -> bool {
    bytes.len() >= 52
        && bytes.len() <= MAX_CLEANUP_MANIFEST
        && &bytes[..16] == MANIFEST_MAGIC
        && fc_security::sha256(&bytes[..bytes.len() - 32]) == bytes[bytes.len() - 32..]
}
pub(super) fn remove_metadata(
    files: &ControlledFiles,
    path: &Path,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    if !exists(path)? {
        return Ok(());
    }
    let mut pin = OwnedObjectPin::file_allow_empty(path).map_err(repair)?;
    let expected = raw_digest(pin.file_mut(), check)?;
    let identity = pin.identity();
    drop(pin);
    let mut failure = None;
    let deleted = files
        .remove_object_expected(path, false, identity, |file| {
            match raw_digest(file, check) {
                Ok(actual) if actual == expected => true,
                Ok(_) => {
                    failure = Some(ReinitializationFileError::RepairRequired);
                    false
                }
                Err(error) => {
                    failure = Some(error);
                    false
                }
            }
        })
        .map_err(repair)?;
    if let Some(error) = failure {
        return Err(error);
    }
    if !deleted && exists(path)? {
        return Err(ReinitializationFileError::RepairRequired);
    }
    Ok(())
}
