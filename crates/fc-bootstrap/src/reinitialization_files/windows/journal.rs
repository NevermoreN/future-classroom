//! A restartable fixed-tree switch, never described as filesystem-atomic.
//! Only a final commit receipt chooses the new set; otherwise startup restores
//! the complete old set, including trees which were originally absent.
use super::*;
const OWNER_MAGIC: &[u8; 16] = b"FCRESETOWNER1\0\0\0";
const JOURNAL_MAGIC: &[u8; 16] = b"FCRESETJOURN1\0\0\0";
const PHASE_MAGIC: &[u8; 16] = b"FCRESETPHASE1\0\0\0";
const COMMITTED: u8 = 27;
const ROLLED_BACK: u8 = 28;
const CLEAN_INSTALLED: u8 = 29;
const CLEAN_ORIGINAL: u8 = 30;
const CLEAN_DONE_INSTALLED: u8 = 31;
const CLEAN_DONE_ORIGINAL: u8 = 32;
const MAX_META: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OperationOwner {
    pub sequence: u64,
    pub root: Identity,
    pub runtime: Identity,
    pub presence: u16,
}
impl OperationOwner {
    const LEN: usize = 82;
    pub fn encode(self) -> Vec<u8> {
        let mut out = OWNER_MAGIC.to_vec();
        out.extend_from_slice(&self.sequence.to_le_bytes());
        for identity in [self.root, self.runtime] {
            put_identity(&mut out, identity);
        }
        out.extend_from_slice(&self.presence.to_le_bytes());
        let hash = fc_security::sha256(&out);
        out.extend_from_slice(&hash);
        out
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::LEN
            || &bytes[..16] != OWNER_MAGIC
            || fc_security::sha256(&bytes[..50]) != bytes[50..]
        {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let mut input = &bytes[16..50];
        let sequence = read_u64(&mut input)?;
        let root = read_identity(&mut input)?;
        let runtime = read_identity(&mut input)?;
        let presence = read_u16(&mut input)?;
        if sequence == 0 || presence & !((1 << OWNED_TREES.len()) - 1) != 0 {
            return Err(ReinitializationFileError::RepairRequired);
        }
        Ok(Self {
            sequence,
            root,
            runtime,
            presence,
        })
    }
    fn verify(self, files: &ReinitializationFiles) -> Result<()> {
        if (self.root, self.runtime) != files.identities()? {
            return Err(ReinitializationFileError::RepairRequired);
        }
        Ok(())
    }
}
fn read_bytes<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    if input.len() < n {
        return Err(ReinitializationFileError::RepairRequired);
    }
    let (head, tail) = input.split_at(n);
    *input = tail;
    Ok(head)
}
fn read_u16(input: &mut &[u8]) -> Result<u16> {
    Ok(u16::from_le_bytes(
        read_bytes(input, 2)?.try_into().map_err(repair)?,
    ))
}
fn read_u32(input: &mut &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(
        read_bytes(input, 4)?.try_into().map_err(repair)?,
    ))
}
fn read_u64(input: &mut &[u8]) -> Result<u64> {
    Ok(u64::from_le_bytes(
        read_bytes(input, 8)?.try_into().map_err(repair)?,
    ))
}
fn put_identity(out: &mut Vec<u8>, identity: Identity) {
    out.extend_from_slice(&identity.0.to_le_bytes());
    out.extend_from_slice(&identity.1.to_le_bytes());
}
fn read_identity(input: &mut &[u8]) -> Result<Identity> {
    Ok((read_u32(input)?, read_u64(input)?))
}
fn put_stamp(out: &mut Vec<u8>, stamp: Stamp) {
    put_identity(out, stamp.identity);
    out.extend_from_slice(&stamp.digest);
}
fn read_stamp(input: &mut &[u8]) -> Result<Stamp> {
    Ok(Stamp {
        identity: read_identity(input)?,
        digest: read_bytes(input, 32)?.try_into().map_err(repair)?,
    })
}
#[derive(Clone, Debug)]
pub(super) struct Metadata {
    owner: OperationOwner,
    old: [Option<Stamp>; 13],
    new: [Stamp; 13],
    manifests: [Option<(u64, [u8; 32])>; 13],
    containers: [Identity; 3],
}
impl Metadata {
    fn encode(&self) -> Vec<u8> {
        let mut out = JOURNAL_MAGIC.to_vec();
        out.extend_from_slice(&self.owner.encode());
        for id in self.containers {
            put_identity(&mut out, id);
        }
        for (n, (old, new)) in self.old.iter().zip(self.new).enumerate() {
            out.push(u8::from(old.is_some()));
            match old {
                Some(value) => put_stamp(&mut out, *value),
                None => out.extend_from_slice(&[0; 44]),
            }
            let (len, hash) = self.manifests[n].unwrap_or((0, [0; 32]));
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&hash);
            put_stamp(&mut out, new);
        }
        let hash = fc_security::sha256(&out);
        out.extend_from_slice(&hash);
        out
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 16 + OperationOwner::LEN + 36 + OWNED_TREES.len() * 129 + 32
            || &bytes[..16] != JOURNAL_MAGIC
            || fc_security::sha256(&bytes[..bytes.len() - 32]) != bytes[bytes.len() - 32..]
        {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let mut input = &bytes[16..bytes.len() - 32];
        let owner = OperationOwner::decode(read_bytes(&mut input, OperationOwner::LEN)?)?;
        let containers = [
            read_identity(&mut input)?,
            read_identity(&mut input)?,
            read_identity(&mut input)?,
        ];
        let mut manifests = [None; 13];
        let mut old = [None; 13];
        let mut new = [Stamp {
            identity: (0, 0),
            digest: [0; 32],
        }; 13];
        for n in 0..OWNED_TREES.len() {
            let flag = read_bytes(&mut input, 1)?[0];
            let value = read_stamp(&mut input)?;
            old[n] = match flag {
                0 if value.identity == (0, 0) && value.digest == [0; 32] => None,
                1 => Some(value),
                _ => return Err(ReinitializationFileError::RepairRequired),
            };
            let length = read_u64(&mut input)?;
            let hash: [u8; 32] = read_bytes(&mut input, 32)?.try_into().map_err(repair)?;
            if old[n].is_some() {
                if length == 0 || length > MAX_CLEANUP_MANIFEST as u64 {
                    return Err(ReinitializationFileError::RepairRequired);
                }
                manifests[n] = Some((length, hash));
            } else if length != 0 || hash != [0; 32] {
                return Err(ReinitializationFileError::RepairRequired);
            }
            new[n] = read_stamp(&mut input)?;
        }
        if !input.is_empty() {
            return Err(ReinitializationFileError::RepairRequired);
        }
        Ok(Self {
            owner,
            old,
            new,
            manifests,
            containers,
        })
    }
}
fn phase_bytes(owner: OperationOwner, phase: u8) -> Vec<u8> {
    let mut out = PHASE_MAGIC.to_vec();
    out.extend_from_slice(&owner.encode());
    out.push(phase);
    let hash = fc_security::sha256(&out);
    out.extend_from_slice(&hash);
    out
}
fn decode_phase(bytes: &[u8]) -> Result<(OperationOwner, u8)> {
    let offset = 16 + OperationOwner::LEN;
    if bytes.len() != offset + 33
        || &bytes[..16] != PHASE_MAGIC
        || fc_security::sha256(&bytes[..offset + 1]) != bytes[offset + 1..]
    {
        return Err(ReinitializationFileError::RepairRequired);
    }
    Ok((OperationOwner::decode(&bytes[16..offset])?, bytes[offset]))
}
fn phase(files: &ControlledFiles, operation: &Path, owner: OperationOwner, n: u8) -> Result<()> {
    write_new(
        files,
        &operation.join(format!("step-{n}")),
        &phase_bytes(owner, n),
    )
}
fn has_phase(operation: &Path, owner: OperationOwner, n: u8) -> Result<bool> {
    let path = operation.join(format!("step-{n}"));
    if !exists(&path)? {
        return Ok(false);
    }
    if read_small(&path, MAX_META)? != phase_bytes(owner, n) {
        return Err(ReinitializationFileError::RepairRequired);
    }
    Ok(true)
}
fn allowed_children(files: &ControlledFiles, operation: &Path) -> Result<()> {
    for entry in entries(files, operation, 34)? {
        let name = entry
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(ReinitializationFileError::RepairRequired)?;
        if matches!(name, "new" | "old" | "manifests") {
            let _ = PinnedDirectory::open(&entry).map_err(repair)?;
        } else if matches!(name, "owner" | "journal" | "cleanup" | "cleanup-complete") {
            let _ = OwnedObjectPin::file_allow_empty(&entry).map_err(repair)?;
        } else if let Some(number) = name
            .strip_prefix("step-")
            .and_then(|v| v.parse::<u8>().ok())
        {
            if number > ROLLED_BACK || name != format!("step-{number}") {
                return Err(ReinitializationFileError::RepairRequired);
            }
            let _ = OwnedObjectPin::file_allow_empty(&entry).map_err(repair)?;
        } else {
            return Err(ReinitializationFileError::RepairRequired);
        }
    }
    Ok(())
}
fn cleanup_record(
    files: &ReinitializationFiles,
    operation: &Path,
) -> Result<Option<OperationOwner>> {
    let first = operation.join("cleanup");
    let complete = operation.join("cleanup-complete");
    let mut found = None;
    for (path, completed) in [(&first, false), (&complete, true)] {
        if !exists(path)? {
            continue;
        }
        let (owner, phase) = decode_phase(&read_small(path, MAX_META)?)?;
        owner.verify(files)?;
        if if completed {
            !matches!(phase, CLEAN_DONE_INSTALLED | CLEAN_DONE_ORIGINAL)
        } else {
            !matches!(phase, CLEAN_INSTALLED | CLEAN_ORIGINAL)
        } {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let mode = if completed { phase - 2 } else { phase };
        if found.is_some_and(|previous| previous != (owner, mode)) {
            return Err(ReinitializationFileError::RepairRequired);
        }
        found = Some((owner, mode));
    }
    Ok(found.map(|v| v.0))
}

/// Marker survives removal of every other receipt and confidential subtree.
/// After marker publication, startup must not compare legitimate later live
/// changes against the historical empty candidate digest.
fn manifest_path(operation: &Path, n: usize) -> PathBuf {
    operation.join("manifests").join(format!("old-{n}.bin"))
}
fn read_manifest(operation: &Path, meta: &Metadata, n: usize) -> Result<Option<Snapshot>> {
    let Some(expected) = meta.old[n] else {
        return Ok(None);
    };
    let (length, hash) = meta.manifests[n].ok_or(ReinitializationFileError::RepairRequired)?;
    let bytes = read_small(&manifest_path(operation, n), MAX_CLEANUP_MANIFEST)?;
    if bytes.len() as u64 != length || fc_security::sha256(&bytes) != hash {
        return Err(ReinitializationFileError::RepairRequired);
    }
    Ok(Some(Snapshot::decode(&bytes, expected)?))
}
fn validate_manifest_names(
    files: &ControlledFiles,
    operation: &Path,
    meta: Option<&Metadata>,
) -> Result<()> {
    let dir = operation.join("manifests");
    if !exists(&dir)? {
        return if meta.is_some() {
            Err(ReinitializationFileError::RepairRequired)
        } else {
            Ok(())
        };
    }
    for file in entries(files, &dir, 14)? {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(ReinitializationFileError::RepairRequired)?;
        let Some(n) = name
            .strip_prefix("old-")
            .and_then(|s| s.strip_suffix(".bin"))
            .and_then(|s| s.parse::<usize>().ok())
        else {
            return Err(ReinitializationFileError::RepairRequired);
        };
        if n >= OWNED_TREES.len()
            || name != format!("old-{n}.bin")
            || meta.is_some_and(|m| m.old[n].is_none())
        {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let _ = OwnedObjectPin::file_allow_empty(&file).map_err(repair)?;
    }
    Ok(())
}
fn remove_empty_expected(files: &ControlledFiles, path: &Path, expected: Identity) -> Result<()> {
    if !exists(path)? {
        return Ok(());
    }
    if !files
        .remove_object_expected(path, true, expected, |_| true)
        .map_err(repair)?
    {
        return Err(ReinitializationFileError::RepairRequired);
    }
    Ok(())
}
fn cleanup(
    files: &ReinitializationFiles,
    operation: &Path,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    allowed_children(&files.files, operation)?;
    let owner =
        cleanup_record(files, operation)?.ok_or(ReinitializationFileError::RepairRequired)?;
    let cleanup_path = operation.join("cleanup");
    let (_, mode) = if exists(&cleanup_path)? {
        decode_phase(&read_small(&cleanup_path, MAX_META)?)?
    } else {
        let (owner, phase) =
            decode_phase(&read_small(&operation.join("cleanup-complete"), MAX_META)?)?;
        (owner, phase - 2)
    };
    let complete = operation.join("cleanup-complete");
    if !exists(&complete)? {
        let meta = Metadata::decode(&read_small(&operation.join("journal"), MAX_META)?)?;
        if meta.owner != owner {
            return Err(ReinitializationFileError::RepairRequired);
        }
        validate_manifest_names(&files.files, operation, Some(&meta))?;
        let manifest_pin =
            OwnedObjectPin::directory(&operation.join("manifests")).map_err(repair)?;
        if manifest_pin.identity() != meta.containers[2] {
            return Err(ReinitializationFileError::RepairRequired);
        }
        drop(manifest_pin);
        // Validate every manifest before removing a single remaining old object.
        // A missing/corrupt manifest never licenses an opportunistic tree delete.
        for n in 0..OWNED_TREES.len() {
            let _ = read_manifest(operation, &meta, n)?;
        }
        for (container_n, label) in ["old", "new"].iter().enumerate() {
            let container = operation.join(label);
            if exists(&container)? {
                let pin = OwnedObjectPin::directory(&container).map_err(repair)?;
                if pin.identity() != meta.containers[container_n] {
                    return Err(ReinitializationFileError::RepairRequired);
                }
                for child in entries(&files.files, &container, 14)? {
                    let name = child
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or(ReinitializationFileError::RepairRequired)?;
                    if !OWNED_TREES.contains(&name) {
                        return Err(ReinitializationFileError::RepairRequired);
                    }
                }
                drop(pin);
            }
        }
        for (n, name) in OWNED_TREES.iter().enumerate() {
            ensure(check)?;
            let old = operation.join("old").join(name);
            let new = operation.join("new").join(name);
            if mode == CLEAN_INSTALLED {
                if let Some(manifest) = read_manifest(operation, &meta, n)? {
                    remove_recorded(&files.files, &old, &manifest, check)?;
                } else if exists(&old)? {
                    return Err(ReinitializationFileError::RepairRequired);
                }
                if exists(&new)? {
                    return Err(ReinitializationFileError::RepairRequired);
                }
            } else {
                if exists(&old)? {
                    return Err(ReinitializationFileError::RepairRequired);
                }
                remove_recorded(&files.files, &new, &empty_snapshot(meta.new[n])?, check)?;
            }
        }
        for (n, label) in ["old", "new"].iter().enumerate() {
            remove_empty_expected(&files.files, &operation.join(label), meta.containers[n])?;
        }
        ensure(check)?;
        write_new(
            &files.files,
            &complete,
            &phase_bytes(
                owner,
                if mode == CLEAN_INSTALLED {
                    CLEAN_DONE_INSTALLED
                } else {
                    CLEAN_DONE_ORIGINAL
                },
            ),
        )?;
    }
    let (done_owner, done_mode) = decode_phase(&read_small(&complete, MAX_META)?)?;
    if done_owner != owner
        || done_mode
            != if mode == CLEAN_INSTALLED {
                CLEAN_DONE_INSTALLED
            } else {
                CLEAN_DONE_ORIGINAL
            }
        || exists(&operation.join("old"))?
        || exists(&operation.join("new"))?
    {
        return Err(ReinitializationFileError::RepairRequired);
    }
    // Every confidential/data object is gone. Keep both terminal receipts until
    // metadata-only cleanup is complete, and never traverse an unexpected tree.
    validate_manifest_names(&files.files, operation, None)?;
    if exists(&operation.join("manifests"))? {
        let meta = Metadata::decode(&read_small(&operation.join("journal"), MAX_META)?)?;
        if meta.owner != owner {
            return Err(ReinitializationFileError::RepairRequired);
        }
        let pin = OwnedObjectPin::directory(&operation.join("manifests")).map_err(repair)?;
        if pin.identity() != meta.containers[2] {
            return Err(ReinitializationFileError::RepairRequired);
        }
        drop(pin);
        for file in entries(&files.files, &operation.join("manifests"), 14)? {
            ensure(check)?;
            let n = file
                .file_name()
                .and_then(|v| v.to_str())
                .and_then(|v| v.strip_prefix("old-"))
                .and_then(|v| v.strip_suffix(".bin"))
                .and_then(|v| v.parse::<usize>().ok())
                .ok_or(ReinitializationFileError::RepairRequired)?;
            let _ = read_manifest(operation, &meta, n)?
                .ok_or(ReinitializationFileError::RepairRequired)?;
            remove_metadata(&files.files, &file, check)?;
        }
        remove_empty_expected(
            &files.files,
            &operation.join("manifests"),
            meta.containers[2],
        )?;
    }
    let pin = PinnedDirectory::open(operation).map_err(repair)?;
    for child in entries(&files.files, operation, 34)? {
        ensure(check)?;
        if child == complete {
            continue;
        }
        if child == operation.join("owner") {
            if OperationOwner::decode(&read_small(&child, MAX_META)?)? != owner {
                return Err(ReinitializationFileError::RepairRequired);
            }
        } else if child == operation.join("journal") {
            if Metadata::decode(&read_small(&child, MAX_META)?)?.owner != owner {
                return Err(ReinitializationFileError::RepairRequired);
            }
        } else if child == operation.join("cleanup") {
            let _ = cleanup_record(files, operation)?
                .ok_or(ReinitializationFileError::RepairRequired)?;
        } else if let Some(n) = child
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("step-"))
            .and_then(|s| s.parse::<u8>().ok())
        {
            if read_small(&child, MAX_META)? != phase_bytes(owner, n) {
                return Err(ReinitializationFileError::RepairRequired);
            }
        } else {
            return Err(ReinitializationFileError::RepairRequired);
        }
        remove_metadata(&files.files, &child, check)?;
    }
    ensure(check)?;
    // This independent terminal marker is always the last receipt removed.
    remove_metadata(&files.files, &complete, check)?;
    drop(pin);
    files
        .files
        .remove_empty_directory(operation)
        .map_err(repair)
}

fn owner(files: &ReinitializationFiles, operation: &Path) -> Result<OperationOwner> {
    let owner =
        OperationOwner::decode(&read_small(&operation.join("owner"), OperationOwner::LEN)?)?;
    owner.verify(files)?;
    Ok(owner)
}
fn validate_unarmed(
    files: &ReinitializationFiles,
    operation: &Path,
    check: &dyn Fn() -> bool,
) -> Result<OperationOwner> {
    allowed_children(&files.files, operation)?;
    let owner = owner(files, operation)?;
    if exists(&operation.join("journal"))?
        || exists(&operation.join("cleanup"))?
        || exists(&operation.join("cleanup-complete"))?
    {
        return Err(ReinitializationFileError::RepairRequired);
    }
    for n in 0..=ROLLED_BACK {
        if exists(&operation.join(format!("step-{n}")))? {
            return Err(ReinitializationFileError::RepairRequired);
        }
    }
    if files.check_scope(check)? != owner.presence {
        return Err(ReinitializationFileError::RepairRequired);
    }
    let old = operation.join("old");
    if exists(&old)? && !entries(&files.files, &old, 1)?.is_empty() {
        return Err(ReinitializationFileError::RepairRequired);
    }
    let new = operation.join("new");
    if exists(&new)? {
        for child in entries(&files.files, &new, 14)? {
            let name = child
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(ReinitializationFileError::RepairRequired)?;
            if !OWNED_TREES.contains(&name) {
                return Err(ReinitializationFileError::RepairRequired);
            }
            let _pin = PinnedDirectory::open(&child).map_err(repair)?;
            if !entries(&files.files, &child, 1)?.is_empty() {
                return Err(ReinitializationFileError::RepairRequired);
            }
        }
    }
    validate_manifest_names(&files.files, operation, None)?;
    if exists(&operation.join("manifests"))? {
        for file in entries(&files.files, &operation.join("manifests"), 14)? {
            if !manifest_checksum(&read_small(&file, MAX_CLEANUP_MANIFEST)?) {
                return Err(ReinitializationFileError::RepairRequired);
            }
        }
    }
    Ok(owner)
}
fn cleanup_unarmed(
    files: &ReinitializationFiles,
    operation: &Path,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    let pin = PinnedDirectory::open(operation).map_err(repair)?;
    for label in ["new", "old", "manifests"] {
        let directory = operation.join(label);
        if !exists(&directory)? {
            continue;
        }
        let parent = OwnedObjectPin::directory(&directory).map_err(repair)?;
        let identity = parent.identity();
        drop(parent);
        for child in entries(&files.files, &directory, 14)? {
            ensure(check)?;
            if label == "new" {
                let child_pin = OwnedObjectPin::directory(&child).map_err(repair)?;
                let id = child_pin.identity();
                drop(child_pin);
                remove_empty_expected(&files.files, &child, id)?;
            } else if label == "manifests" {
                remove_metadata(&files.files, &child, check)?;
            } else {
                return Err(ReinitializationFileError::RepairRequired);
            }
        }
        remove_empty_expected(&files.files, &directory, identity)?;
    }
    remove_metadata(&files.files, &operation.join("owner"), check)?;
    drop(pin);
    files
        .files
        .remove_empty_directory(operation)
        .map_err(repair)
}
pub(super) fn discard_unarmed(
    files: &ReinitializationFiles,
    operation: &Path,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    let _ = validate_unarmed(files, operation, check)?;
    cleanup_unarmed(files, operation, check)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetCommitReceipt {
    pub sequence: u64,
    pub operation_id: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResetStartup {
    pub discarded_unarmed: u32,
    pub rolled_back: u32,
    pub installed: u32,
}
fn candidate(
    files: &ReinitializationFiles,
    operation: &Path,
    check: &dyn Fn() -> bool,
) -> Result<[Stamp; 13]> {
    let root = operation.join("new");
    let children = entries(&files.files, &root, 14)?;
    if children.len() != OWNED_TREES.len() {
        return Err(ReinitializationFileError::RepairRequired);
    }
    let mut stamps = [Stamp {
        identity: (0, 0),
        digest: [0; 32],
    }; 13];
    for (n, name) in OWNED_TREES.iter().enumerate() {
        ensure(check)?;
        let path = root.join(name);
        if !children.contains(&path) || !entries(&files.files, &path, 1)?.is_empty() {
            return Err(ReinitializationFileError::RepairRequired);
        }
        stamps[n] =
            stamp(&files.files, &path, check)?.ok_or(ReinitializationFileError::RepairRequired)?;
    }
    Ok(stamps)
}
fn arm(prepared: &PreparedReinitialization, check: &dyn Fn() -> bool) -> Result<Metadata> {
    let files = &prepared.service;
    prepared.owner.verify(files)?;
    // This is called only AFTER the real drain capability was consumed. These
    // final hashes intentionally need not equal the earlier read-only UI review.
    let _ = files.check_scope(check)?;
    allowed_children(&files.files, &prepared.operation)?;
    if owner(files, &prepared.operation)? != prepared.owner {
        return Err(ReinitializationFileError::RepairRequired);
    }
    for n in 0..=ROLLED_BACK {
        if exists(&prepared.operation.join(format!("step-{n}")))? {
            return Err(ReinitializationFileError::RepairRequired);
        }
    }
    if exists(&prepared.operation.join("journal"))?
        || exists(&prepared.operation.join("cleanup"))?
        || !entries(&files.files, &prepared.operation.join("old"), 1)?.is_empty()
    {
        return Err(ReinitializationFileError::RepairRequired);
    }
    let mut old = [None; 13];
    let mut manifests = [None; 13];
    let mut total = 0usize;
    for (n, name) in OWNED_TREES.iter().enumerate() {
        if let Some(snapshot) = snapshot(&files.files, &files.paths.root().join(name), check)? {
            old[n] = Some(snapshot.stamp);
            let bytes = snapshot.encode()?;
            total = total
                .checked_add(bytes.len())
                .ok_or(ReinitializationFileError::RepairRequired)?;
            if total > MAX_CLEANUP_MANIFEST {
                return Err(ReinitializationFileError::RepairRequired);
            }
            manifests[n] = Some((bytes.len() as u64, fc_security::sha256(&bytes)));
            write_new(&files.files, &manifest_path(&prepared.operation, n), &bytes)?;
        }
    }
    let new = candidate(files, &prepared.operation, check)?;
    let containers = [
        OwnedObjectPin::directory(&prepared.operation.join("old"))
            .map_err(repair)?
            .identity(),
        OwnedObjectPin::directory(&prepared.operation.join("new"))
            .map_err(repair)?
            .identity(),
        OwnedObjectPin::directory(&prepared.operation.join("manifests"))
            .map_err(repair)?
            .identity(),
    ];
    let meta = Metadata {
        owner: prepared.owner,
        old,
        new,
        manifests,
        containers,
    };
    ensure(check)?;
    write_new(
        &files.files,
        &prepared.operation.join("journal"),
        &meta.encode(),
    )?;
    phase(&files.files, &prepared.operation, meta.owner, 0)?;
    Ok(meta)
}
fn move_exact(
    files: &ReinitializationFiles,
    from: &Path,
    to: &Path,
    expected: Stamp,
) -> Result<()> {
    files
        .files
        .rename_directory_expected(from, to, expected.identity)
        .map_err(repair)
}
fn move_one(
    files: &ReinitializationFiles,
    operation: &Path,
    meta: &Metadata,
    step: usize,
) -> Result<()> {
    let n = step / 2;
    let name = OWNED_TREES[n];
    if step.is_multiple_of(2) {
        if let Some(old) = meta.old[n] {
            move_exact(
                files,
                &files.paths.root().join(name),
                &operation.join("old").join(name),
                old,
            )?;
        }
    } else {
        move_exact(
            files,
            &operation.join("new").join(name),
            &files.paths.root().join(name),
            meta.new[n],
        )?;
    }
    Ok(())
}
fn verify_installed(
    files: &ReinitializationFiles,
    operation: &Path,
    meta: &Metadata,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    let _ = files.check_scope(check)?;
    for (n, label) in ["old", "new", "manifests"].iter().enumerate() {
        let pin = OwnedObjectPin::directory(&operation.join(label)).map_err(repair)?;
        if pin.identity() != meta.containers[n] {
            return Err(ReinitializationFileError::RepairRequired);
        }
    }
    let old_entries = entries(&files.files, &operation.join("old"), 14)?;
    if old_entries.len() != meta.old.iter().filter(|v| v.is_some()).count()
        || !entries(&files.files, &operation.join("new"), 1)?.is_empty()
    {
        return Err(ReinitializationFileError::RepairRequired);
    }
    for path in old_entries {
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or(ReinitializationFileError::RepairRequired)?;
        if !OWNED_TREES
            .iter()
            .position(|v| *v == name)
            .is_some_and(|n| meta.old[n].is_some())
        {
            return Err(ReinitializationFileError::RepairRequired);
        }
    }
    for (n, name) in OWNED_TREES.iter().enumerate() {
        if stamp(&files.files, &files.paths.root().join(name), check)? != Some(meta.new[n])
            || stamp(&files.files, &operation.join("old").join(name), check)? != meta.old[n]
            || exists(&operation.join("new").join(name))?
        {
            return Err(ReinitializationFileError::RepairRequired);
        }
    }
    Ok(())
}
pub(super) fn commit(
    mut prepared: PreparedReinitialization,
    check: &dyn Fn() -> bool,
) -> Result<ResetCommitReceipt> {
    let meta = arm(&prepared, check)?;
    for step in 0..OWNED_TREES.len() * 2 {
        ensure(check)?;
        move_one(&prepared.service, &prepared.operation, &meta, step)?;
        phase(
            &prepared.service.files,
            &prepared.operation,
            meta.owner,
            step as u8 + 1,
        )?;
    }
    ensure(check)?;
    verify_installed(&prepared.service, &prepared.operation, &meta, check)?;
    phase(
        &prepared.service.files,
        &prepared.operation,
        meta.owner,
        COMMITTED,
    )?;
    write_new(
        &prepared.service.files,
        &prepared.operation.join("cleanup"),
        &phase_bytes(meta.owner, CLEAN_INSTALLED),
    )?;
    let receipt = ResetCommitReceipt {
        sequence: meta.owner.sequence,
        operation_id: prepared
            .operation
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or(ReinitializationFileError::RepairRequired)?
            .to_owned(),
    };
    prepared.operation_pin.take();
    cleanup(&prepared.service, &prepared.operation, check)?;
    Ok(receipt)
}
#[derive(Clone, Copy)]
enum Layout {
    Original,
    Between,
    Installed,
}
fn layout(
    files: &ReinitializationFiles,
    operation: &Path,
    meta: &Metadata,
    n: usize,
    check: &dyn Fn() -> bool,
) -> Result<Layout> {
    let name = OWNED_TREES[n];
    let live = stamp(&files.files, &files.paths.root().join(name), check)?;
    let old = stamp(&files.files, &operation.join("old").join(name), check)?;
    let new = stamp(&files.files, &operation.join("new").join(name), check)?;
    match meta.old[n] {
        Some(expected) if live == Some(expected) && old.is_none() && new == Some(meta.new[n]) => {
            Ok(Layout::Original)
        }
        Some(expected) if live.is_none() && old == Some(expected) && new == Some(meta.new[n]) => {
            Ok(Layout::Between)
        }
        Some(expected) if live == Some(meta.new[n]) && old == Some(expected) && new.is_none() => {
            Ok(Layout::Installed)
        }
        None if live.is_none() && old.is_none() && new == Some(meta.new[n]) => Ok(Layout::Original),
        None if live == Some(meta.new[n]) && old.is_none() && new.is_none() => {
            Ok(Layout::Installed)
        }
        _ => Err(ReinitializationFileError::RepairRequired),
    }
}
fn rollback(
    files: &ReinitializationFiles,
    operation: &Path,
    meta: &Metadata,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    let mut layouts = [Layout::Original; 13];
    for (n, value) in layouts.iter_mut().enumerate() {
        *value = layout(files, operation, meta, n, check)?;
    }
    // Every layout is proven before the first rollback move. Receipts may lag
    // physical renames; original identity/digest selects the object to move.
    for n in (0..OWNED_TREES.len()).rev() {
        ensure(check)?;
        let name = OWNED_TREES[n];
        if matches!(layouts[n], Layout::Installed) {
            move_exact(
                files,
                &files.paths.root().join(name),
                &operation.join("new").join(name),
                meta.new[n],
            )?;
        }
        if !matches!(layouts[n], Layout::Original) {
            if let Some(old) = meta.old[n] {
                move_exact(
                    files,
                    &operation.join("old").join(name),
                    &files.paths.root().join(name),
                    old,
                )?;
            }
        }
    }
    for (n, name) in OWNED_TREES.iter().enumerate() {
        if stamp(&files.files, &files.paths.root().join(name), check)? != meta.old[n] {
            return Err(ReinitializationFileError::RepairRequired);
        }
    }
    if !has_phase(operation, meta.owner, ROLLED_BACK)? {
        phase(&files.files, operation, meta.owner, ROLLED_BACK)?;
    }
    write_new(
        &files.files,
        &operation.join("cleanup"),
        &phase_bytes(meta.owner, CLEAN_ORIGINAL),
    )?;
    cleanup(files, operation, check)
}
pub(super) fn reconcile(
    files: &ReinitializationFiles,
    check: &dyn Fn() -> bool,
) -> Result<ResetStartup> {
    let Some(namespace) = files.namespace(false)? else {
        return Ok(ResetStartup::default());
    };
    let marker = namespace.join(".future-classroom-owner");
    let mut operations = entries(&files.files, &namespace, 34)?;
    operations.sort();
    if operations.iter().all(|p| *p == marker) {
        return Ok(ResetStartup::default());
    }
    let _ = files.check_scope(check)?;
    let mut armed = 0;
    for operation in operations.iter().filter(|p| **p != marker) {
        ensure(check)?;
        if entries(&files.files, operation, 34)?.is_empty() {
            continue;
        }
        allowed_children(&files.files, operation)?;
        if cleanup_record(files, operation)?.is_some() {
            continue;
        }
        let _ = owner(files, operation)?;
        if exists(&operation.join("journal"))? || exists(&operation.join("step-0"))? {
            armed += 1;
        }
    }
    if armed > 1 {
        return Err(ReinitializationFileError::RepairRequired);
    }
    let mut report = ResetStartup::default();
    for operation in operations.into_iter().filter(|p| *p != marker) {
        ensure(check)?;
        if entries(&files.files, &operation, 34)?.is_empty() {
            files
                .files
                .remove_empty_directory(&operation)
                .map_err(repair)?;
            report.discarded_unarmed += 1;
            continue;
        }
        if cleanup_record(files, &operation)?.is_some() {
            cleanup(files, &operation, check)?;
            continue;
        }
        if !exists(&operation.join("journal"))? {
            discard_unarmed(files, &operation, check)?;
            report.discarded_unarmed += 1;
            continue;
        }
        let meta = Metadata::decode(&read_small(&operation.join("journal"), MAX_META)?)?;
        if owner(files, &operation)? != meta.owner {
            return Err(ReinitializationFileError::RepairRequired);
        }
        for n in 0..=ROLLED_BACK {
            let _ = has_phase(&operation, meta.owner, n)?;
        }
        if has_phase(&operation, meta.owner, COMMITTED)? {
            verify_installed(files, &operation, &meta, check)?;
            write_new(
                &files.files,
                &operation.join("cleanup"),
                &phase_bytes(meta.owner, CLEAN_INSTALLED),
            )?;
            cleanup(files, &operation, check)?;
            report.installed += 1;
        } else {
            rollback(files, &operation, &meta, check)?;
            report.rolled_back += 1;
        }
    }
    Ok(report)
}
#[cfg(test)]
pub(super) fn arm_for_test(
    prepared: &PreparedReinitialization,
    check: &dyn Fn() -> bool,
) -> Result<Metadata> {
    arm(prepared, check)
}
#[cfg(test)]
mod tests;
