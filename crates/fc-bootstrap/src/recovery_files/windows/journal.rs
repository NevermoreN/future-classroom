//! A three-tree cold switch is deliberately NOT called atomic. Every completed
//! rename is discoverable from exact old/new tree digests even if its following
//! receipt was not flushed. Until the commit receipt exists, restart rolls back
//! the complete old set; ambiguity retains every tree and fails closed.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
const TREES: [&str; 3] = ["state", "teachers", "profile-content"];
const MAGIC: &[u8; 16] = b"FCRESTOR-V1\r\n\x1a\n\0";
const MAX_META: usize = MAX_MANIFEST_BYTES + 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColdCommitReceipt {
    pub generation: u64,
    pub operation_id: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartupRecovery {
    pub discarded_unarmed: u32,
    pub rolled_back: u32,
    pub installed: u32,
}
struct Metadata {
    generation: u64,
    candidate_revision: fc_domain::ids::Revision,
    candidate_bytes: u64,
    candidate_sha256: [u8; 32],
    manifest: BackupManifest,
    old: [[u8; 32]; 3],
    new: [[u8; 32]; 3],
}
fn repair<T>(_: T) -> RecoveryFileError {
    RecoveryFileError::RepairRequired
}
fn phase_path(operation: &Path, phase: u8) -> PathBuf {
    operation.join(format!("step-{phase}"))
}
fn phase_bytes(generation: u64, phase: u8) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&generation.to_le_bytes());
    bytes.push(phase);
    bytes
}
fn phase(files: &ControlledFiles, operation: &Path, generation: u64, n: u8) -> Result<()> {
    write_new(
        files,
        &phase_path(operation, n),
        &phase_bytes(generation, n),
    )
}
fn has_phase(operation: &Path, generation: u64, n: u8) -> Result<bool> {
    let path = phase_path(operation, n);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(repair(e)),
        Ok(_) => {
            if read_small(&path, 25).map_err(repair)? != phase_bytes(generation, n) {
                return Err(RecoveryFileError::RepairRequired);
            }
            Ok(true)
        }
    }
}
fn cleanup_marker(operation: &Path) -> Result<bool> {
    let path = operation.join("cleanup");
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(repair(e)),
        Ok(_) => {
            let bytes = read_small(&path, 25).map_err(repair)?;
            if bytes.len() != 25
                || &bytes[..16] != MAGIC
                || !matches!(bytes[24], 9 | 10)
                || u64::from_le_bytes(bytes[16..24].try_into().map_err(repair)?) == 0
            {
                return Err(RecoveryFileError::RepairRequired);
            }
            Ok(true)
        }
    }
}
fn cleanup_operation(files: &ControlledFiles, operation: &Path) -> Result<()> {
    // A completed cleanup receipt survives until all confidential/large trees and
    // other receipts have gone. Restart can finish any interrupted deletion.
    let pin = PinnedDirectory::open(operation).map_err(repair)?;
    let cleanup = operation.join("cleanup");
    for child in files.entries(operation, 32768).map_err(repair)? {
        if child == cleanup {
            continue;
        }
        if std::fs::symlink_metadata(&child).map_err(repair)?.is_dir() {
            remove_owned_tree(files, &child, &mut 100_000)?;
        } else {
            files.remove(&child).map_err(repair)?;
        }
    }
    files.remove(&cleanup).map_err(repair)?;
    drop(pin);
    files.remove_empty_directory(operation).map_err(repair)
}
fn validate_unarmed_layout(files: &RecoveryFiles, operation: &Path) -> Result<()> {
    // Missing receipts alone are not proof that a switch never started. In
    // particular, never discard an old subtree after metadata loss/reordering.
    // Only an absent/empty rollback container and a complete live directory set
    // permit cleanup of an unarmed operation.
    let old = operation.join("old");
    if checked_directory(&files.files, &old).map_err(repair)?
        && !files.files.entries(&old, 4).map_err(repair)?.is_empty()
    {
        return Err(RecoveryFileError::RepairRequired);
    }
    for name in TREES {
        if !checked_directory(&files.files, &files.paths.root().join(name)).map_err(repair)? {
            return Err(RecoveryFileError::RepairRequired);
        }
    }
    Ok(())
}
fn encode(meta: &Metadata) -> Result<Vec<u8>> {
    let manifest = encode_manifest(&meta.manifest).map_err(invalid)?;
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&meta.generation.to_le_bytes());
    out.extend_from_slice(&meta.candidate_bytes.to_le_bytes());
    out.extend_from_slice(&meta.candidate_sha256);
    out.extend_from_slice(&meta.candidate_revision.get().to_le_bytes());
    for h in meta.old.iter().chain(meta.new.iter()) {
        out.extend_from_slice(h)
    }
    out.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
    out.extend_from_slice(&manifest);
    Ok(out)
}
fn decode(bytes: &[u8]) -> Result<Metadata> {
    if bytes.len() < 268 || &bytes[..16] != MAGIC {
        return Err(RecoveryFileError::RepairRequired);
    }
    let generation = u64::from_le_bytes(bytes[16..24].try_into().map_err(repair)?);
    let candidate_bytes = u64::from_le_bytes(bytes[24..32].try_into().map_err(repair)?);
    let candidate_sha256 = bytes[32..64].try_into().map_err(repair)?;
    let candidate_revision = fc_domain::ids::Revision::new(u64::from_le_bytes(
        bytes[64..72].try_into().map_err(repair)?,
    ));
    let mut old = [[0; 32]; 3];
    let mut new = [[0; 32]; 3];
    for (n, h) in old.iter_mut().chain(new.iter_mut()).enumerate() {
        h.copy_from_slice(&bytes[72 + n * 32..104 + n * 32]);
    }
    let length = u32::from_le_bytes(bytes[264..268].try_into().map_err(repair)?) as usize;
    if generation == 0
        || candidate_bytes == 0
        || candidate_bytes > MAX_DATABASE_BYTES
        || length > MAX_MANIFEST_BYTES
        || candidate_revision == fc_domain::ids::Revision::default()
        || bytes.len() != 268 + length
    {
        return Err(RecoveryFileError::RepairRequired);
    }
    let manifest = fc_protocol::recovery::decode_manifest(&bytes[268..]).map_err(repair)?;
    Ok(Metadata {
        generation,
        candidate_revision,
        candidate_bytes,
        candidate_sha256,
        manifest,
        old,
        new,
    })
}

/// Deterministic, bounded chain hash of directory names and exact immutable file
/// bytes. It includes empty directories and rejects links/hardlinks throughout.
/// Directory pins live until all children have been inspected.
fn tree_digest(files: &ControlledFiles, path: &Path, check: &dyn Fn() -> bool) -> Result<[u8; 32]> {
    fn walk(
        files: &ControlledFiles,
        base: &Path,
        path: &Path,
        state: &mut [u8; 32],
        remaining: &mut usize,
        check: &dyn Fn() -> bool,
    ) -> Result<()> {
        ensure(check)?;
        if *remaining == 0 {
            return Err(RecoveryFileError::RepairRequired);
        }
        *remaining -= 1;
        let pin = PinnedDirectory::open(path).map_err(repair)?;
        let mut entries = files.entries(path, 32768).map_err(repair)?;
        entries.sort();
        for entry in entries {
            ensure(check)?;
            if *remaining == 0 {
                return Err(RecoveryFileError::RepairRequired);
            }
            let relative = entry.strip_prefix(base).map_err(repair)?;
            let name = relative.to_str().ok_or(RecoveryFileError::RepairRequired)?;
            if name.len() > 16000 {
                return Err(RecoveryFileError::RepairRequired);
            }
            let metadata = std::fs::symlink_metadata(&entry).map_err(repair)?;
            let mut bytes = state.to_vec();
            bytes.extend_from_slice(&(name.len() as u64).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            if metadata.is_dir() {
                bytes.push(0);
                *state = fc_security::sha256(&bytes);
                walk(files, base, &entry, state, remaining, check)?
            } else {
                *remaining -= 1;
                bytes.push(1);
                let mut source = LockedFile::open(&entry, MAX_DATABASE_BYTES).map_err(repair)?;
                let (length, hash) = digest(&mut source, check)?;
                bytes.extend_from_slice(&length.to_le_bytes());
                bytes.extend_from_slice(&hash);
                *state = fc_security::sha256(&bytes);
            }
        }
        drop(pin);
        Ok(())
    }
    let mut state = fc_security::sha256(b"FC_RECOVERY_TREE_V1");
    walk(files, path, path, &mut state, &mut 100_000, check)?;
    Ok(state)
}
/// Expected chain uses typed names and approved digests, never a newly observed
/// directory member. Comparing the capture to this value prevents an object
/// inserted after layout validation from being adopted into the cold journal.
fn expected_candidate_digests(
    manifest: &BackupManifest,
    bytes: u64,
    sha256: [u8; 32],
) -> Result<[[u8; 32]; 3]> {
    type Object = Option<(u64, [u8; 32])>;
    let mut objects: BTreeMap<PathBuf, Object> = BTreeMap::new();
    for tree in TREES {
        objects.insert(PathBuf::from(tree), None);
    }
    objects.insert(PathBuf::from("profile-content").join("originals"), None);
    objects.insert(PathBuf::from("profile-content").join("quarantine"), None);
    let marker = b"FUTURE_CLASSROOM_PROFILE_CONTENT_V1\n";
    objects.insert(
        PathBuf::from("profile-content").join(".future-classroom-owner"),
        Some((marker.len() as u64, fc_security::sha256(marker))),
    );
    for entry in &manifest.entries {
        let path = entry_relative_path(entry);
        let expected = if matches!(entry, BackupEntry::Database { .. }) {
            (bytes, sha256)
        } else {
            (entry.bytes(), entry.sha256())
        };
        if objects.insert(path.clone(), Some(expected)).is_some() {
            return Err(RecoveryFileError::InvalidPackage);
        }
        let mut parent = path.parent();
        while let Some(path) = parent.filter(|p| !p.as_os_str().is_empty()) {
            if objects.get(path).is_some_and(Option::is_some) {
                return Err(RecoveryFileError::InvalidPackage);
            }
            objects.entry(path.to_owned()).or_insert(None);
            parent = path.parent();
        }
    }
    let mut result = [[0; 32]; 3];
    for (index, tree) in TREES.iter().enumerate() {
        let base = Path::new(tree);
        let mut state = fc_security::sha256(b"FC_RECOVERY_TREE_V1");
        // Path component ordering is the same parent-first depth-first order as
        // the native walker: a directory precedes all its sorted descendants.
        for (path, object) in &objects {
            let Ok(relative) = path.strip_prefix(base) else {
                continue;
            };
            if relative.as_os_str().is_empty() {
                continue;
            }
            let name = relative.to_str().ok_or(RecoveryFileError::InvalidPackage)?;
            let mut next = state.to_vec();
            next.extend_from_slice(&(name.len() as u64).to_le_bytes());
            next.extend_from_slice(name.as_bytes());
            match object {
                None => next.push(0),
                Some((bytes, sha256)) => {
                    next.push(1);
                    next.extend_from_slice(&bytes.to_le_bytes());
                    next.extend_from_slice(sha256);
                }
            }
            state = fc_security::sha256(&next);
        }
        result[index] = state;
    }
    Ok(result)
}
fn capture_candidate_digests(
    workspace: &RecoveryWorkspace,
    manifest: &BackupManifest,
    bytes: u64,
    sha256: [u8; 32],
    check: &dyn Fn() -> bool,
) -> Result<[[u8; 32]; 3]> {
    let expected = expected_candidate_digests(manifest, bytes, sha256)?;
    for (index, name) in TREES.iter().enumerate() {
        if tree_digest(&workspace.files, &workspace.new_root().join(name), check)?
            != expected[index]
        {
            return Err(RecoveryFileError::RepairRequired);
        }
    }
    Ok(expected)
}
fn verify_candidate(
    workspace: &RecoveryWorkspace,
    manifest: &BackupManifest,
    bytes: u64,
    sha256: [u8; 32],
    check: &dyn Fn() -> bool,
) -> Result<()> {
    let base = workspace.new_root();
    let mut allowed_files = BTreeSet::new();
    let mut allowed_dirs = BTreeSet::new();
    for name in TREES {
        allowed_dirs.insert(base.join(name));
    }
    allowed_dirs.insert(base.join("profile-content").join("originals"));
    allowed_dirs.insert(base.join("profile-content").join("quarantine"));
    let marker = base.join("profile-content").join(".future-classroom-owner");
    allowed_files.insert(marker.clone());
    if read_small(&marker, 64)? != b"FUTURE_CLASSROOM_PROFILE_CONTENT_V1\n" {
        return Err(RecoveryFileError::RepairRequired);
    }
    for entry in &manifest.entries {
        let path = base.join(entry_relative_path(entry));
        allowed_files.insert(path.clone());
        let mut parent = path.parent();
        while let Some(p) = parent {
            if p == base {
                break;
            }
            allowed_dirs.insert(p.to_owned());
            parent = p.parent();
        }
        let mut source = LockedFile::open(&path, MAX_DATABASE_BYTES).map_err(repair)?;
        let actual = digest(&mut source, check)?;
        let expected = if matches!(entry, BackupEntry::Database { .. }) {
            (bytes, sha256)
        } else {
            (entry.bytes(), entry.sha256())
        };
        if actual != expected {
            return Err(RecoveryFileError::RepairRequired);
        }
    }
    fn scan(
        files: &ControlledFiles,
        path: &Path,
        allowed_files: &BTreeSet<PathBuf>,
        allowed_dirs: &BTreeSet<PathBuf>,
        seen: &mut BTreeSet<PathBuf>,
        check: &dyn Fn() -> bool,
    ) -> Result<()> {
        ensure(check)?;
        let pin = PinnedDirectory::open(path).map_err(repair)?;
        for child in files.entries(path, 32768).map_err(repair)? {
            if !seen.insert(child.clone()) {
                return Err(RecoveryFileError::RepairRequired);
            }
            if std::fs::symlink_metadata(&child).map_err(repair)?.is_dir() {
                if !allowed_dirs.contains(&child) {
                    return Err(RecoveryFileError::RepairRequired);
                }
                scan(files, &child, allowed_files, allowed_dirs, seen, check)?;
            } else if !allowed_files.contains(&child) {
                return Err(RecoveryFileError::RepairRequired);
            }
        }
        drop(pin);
        Ok(())
    }
    let mut seen = BTreeSet::new();
    for name in TREES {
        scan(
            &workspace.files,
            &base.join(name),
            &allowed_files,
            &allowed_dirs,
            &mut seen,
            check,
        )?;
    }
    if allowed_files.iter().any(|p| !seen.contains(p)) {
        return Err(RecoveryFileError::RepairRequired);
    }
    Ok(())
}
pub(super) fn seal(
    workspace: &RecoveryWorkspace,
    manifest: &BackupManifest,
    generation: u64,
    candidate_revision: fc_domain::ids::Revision,
    bytes: u64,
    sha256: [u8; 32],
) -> Result<()> {
    // This marker is not an armed operation. Until an actual drain permit is
    // consumed, interruption leaves the live deployment untouched.
    let meta = Metadata {
        generation,
        candidate_revision,
        candidate_bytes: bytes,
        candidate_sha256: sha256,
        manifest: manifest.clone(),
        old: [[0; 32]; 3],
        new: [[0; 32]; 3],
    };
    write_new(
        &workspace.files,
        &workspace.operation.join("sealed"),
        &encode(&meta)?,
    )
}
pub(super) fn commit(
    mut restore: ColdRestore,
    permit: crate::recovery::ColdRestorePermit,
    check: &dyn Fn() -> bool,
) -> Result<ColdCommitReceipt> {
    // Keep the complete capability intact on every fallible validation path:
    // its field order releases DB/media pins before worker-owned cleanup.
    let workspace = &restore.workspace;
    let manifest = &restore.manifest;
    let generation = restore.generation;
    let candidate_bytes = restore.candidate_bytes;
    let candidate_sha256 = restore.candidate_sha256;
    let candidate_revision = restore.candidate_revision;
    if permit.live_database()
        != workspace
            .files
            .root()
            .join("state")
            .join("classroom.sqlite3")
        || permit.candidate_database() != workspace.candidate_database()
        || permit.generation() != generation
        || permit.revision() != candidate_revision
        || permit.manifest_sha256()
            != fc_security::sha256(&encode_manifest(manifest).map_err(invalid)?)
    {
        return Err(RecoveryFileError::InvalidPackage);
    }
    // Actual join/drain capability is consumed. Exact immutable candidate pins
    // remain held through allowed-layout validation and journal digest capture.
    ensure(check)?;
    verify_candidate(
        workspace,
        manifest,
        candidate_bytes,
        candidate_sha256,
        check,
    )?;
    let old_root = workspace.operation.join("old");
    let old_pin = workspace
        .files
        .create_directory_new(&old_root)
        .map_err(unavailable)?;
    drop(old_pin);
    let mut meta = Metadata {
        generation,
        candidate_revision,
        candidate_bytes,
        candidate_sha256,
        manifest: manifest.clone(),
        old: [[0; 32]; 3],
        new: capture_candidate_digests(
            workspace,
            manifest,
            candidate_bytes,
            candidate_sha256,
            check,
        )?,
    };
    for (n, name) in TREES.iter().enumerate() {
        meta.old[n] = tree_digest(&workspace.files, &workspace.files.root().join(name), check)?;
    }
    write_new(
        &workspace.files,
        &workspace.operation.join("journal"),
        &encode(&meta)?,
    )?;
    phase(&workspace.files, &workspace.operation, generation, 0)?;
    restore.workspace.armed = true;
    let ColdRestore {
        workspace,
        _candidate,
        _media,
        ..
    } = restore;
    // Directory ancestor pins would prevent our own rename. Release them only
    // after the durable journal captures the verified exact pinned bytes; the
    // installed digest check below refuses any subsequent mutation.
    drop(_candidate);
    drop(_media);
    for (n, name) in TREES.iter().enumerate() {
        ensure(check)?;
        workspace
            .files
            .rename_directory(&workspace.files.root().join(name), &old_root.join(name))
            .map_err(repair)?;
        phase(
            &workspace.files,
            &workspace.operation,
            generation,
            1 + (n as u8) * 2,
        )?;
        ensure(check)?;
        workspace
            .files
            .rename_directory(
                &workspace.new_root().join(name),
                &workspace.files.root().join(name),
            )
            .map_err(repair)?;
        phase(
            &workspace.files,
            &workspace.operation,
            generation,
            2 + (n as u8) * 2,
        )?;
    }
    // All three installed trees must still match the exact pre-rename digests.
    for (n, name) in TREES.iter().enumerate() {
        if tree_digest(&workspace.files, &workspace.files.root().join(name), check)? != meta.new[n]
        {
            return Err(RecoveryFileError::RepairRequired);
        }
    }
    phase(&workspace.files, &workspace.operation, generation, 7)?;
    Ok(ColdCommitReceipt {
        generation,
        operation_id: workspace
            .operation
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(RecoveryFileError::RepairRequired)?
            .to_owned(),
    })
}

pub(super) fn reconcile(
    files: &RecoveryFiles,
    check: &dyn Fn() -> bool,
) -> Result<StartupRecovery> {
    let namespace = files.namespace()?;
    let marker = namespace.join(".future-classroom-owner");
    let mut operations = files.files.entries(&namespace, 34).map_err(repair)?;
    operations.sort();
    let mut report = StartupRecovery::default();
    let mut armed_count = 0;
    // Validate all operation owners before deleting even unarmed residue.
    for operation in operations.iter().filter(|p| **p != marker) {
        if files
            .files
            .entries(operation, 32768)
            .map_err(repair)?
            .is_empty()
            || cleanup_marker(operation)?
        {
            continue;
        }
        if read_small(&operation.join("owner"), OWNER.len()).map_err(repair)? != OWNER {
            return Err(RecoveryFileError::RepairRequired);
        }
        if phase_path(operation, 0).exists() || operation.join("journal").exists() {
            armed_count += 1;
        }
    }
    if armed_count > 1 {
        return Err(RecoveryFileError::RepairRequired);
    }
    for operation in operations.into_iter().filter(|p| *p != marker) {
        ensure(check)?;
        if files
            .files
            .entries(&operation, 32768)
            .map_err(repair)?
            .is_empty()
        {
            files
                .files
                .remove_empty_directory(&operation)
                .map_err(repair)?;
            continue;
        }
        if cleanup_marker(&operation)? {
            cleanup_operation(&files.files, &operation)?;
            continue;
        }
        if !phase_path(&operation, 0).exists() && !operation.join("journal").exists() {
            validate_unarmed_layout(files, &operation)?;
            remove_owned_tree(&files.files, &operation, &mut 100_000)?;
            report.discarded_unarmed += 1;
            continue;
        }
        let meta = decode(&read_small(&operation.join("journal"), MAX_META).map_err(repair)?)?;
        for n in 0..=8 {
            let _ = has_phase(&operation, meta.generation, n)?;
        }
        // A complete journal alone is enough to require whole-set recovery.
        // Never discard old/new trees merely because a following receipt is
        // absent: interrupted flush/metadata observation must fail safely too.
        if has_phase(&operation, meta.generation, 7)? {
            for (n, name) in TREES.iter().enumerate() {
                if tree_digest(&files.files, &files.paths.root().join(name), check)? != meta.new[n]
                {
                    return Err(RecoveryFileError::RepairRequired);
                }
            }
            write_new(
                &files.files,
                &operation.join("cleanup"),
                &phase_bytes(meta.generation, 9),
            )?;
            cleanup_operation(&files.files, &operation)?;
            report.installed += 1;
            continue;
        }
        // Validate every observed combination before moving a single tree.
        for (n, name) in TREES.iter().enumerate() {
            let live = files.paths.root().join(name);
            let old = operation.join("old").join(name);
            let new = operation.join("new").join(name);
            let (l, o, p) = (
                checked_directory(&files.files, &live)?,
                checked_directory(&files.files, &old)?,
                checked_directory(&files.files, &new)?,
            );
            match tree_layout(l, o, p)? {
                TreeLayout::Original => {
                    if tree_digest(&files.files, &live, check)? != meta.old[n]
                        || tree_digest(&files.files, &new, check)? != meta.new[n]
                    {
                        return Err(RecoveryFileError::RepairRequired);
                    }
                }
                TreeLayout::BetweenRenames => {
                    if tree_digest(&files.files, &old, check)? != meta.old[n]
                        || tree_digest(&files.files, &new, check)? != meta.new[n]
                    {
                        return Err(RecoveryFileError::RepairRequired);
                    }
                }
                TreeLayout::Installed => {
                    if tree_digest(&files.files, &old, check)? != meta.old[n]
                        || tree_digest(&files.files, &live, check)? != meta.new[n]
                    {
                        return Err(RecoveryFileError::RepairRequired);
                    }
                }
            }
        }
        for name in TREES.iter().rev() {
            ensure(check)?;
            let live = files.paths.root().join(name);
            let old = operation.join("old").join(name);
            let new = operation.join("new").join(name);
            if checked_directory(&files.files, &old)? {
                if checked_directory(&files.files, &live)? {
                    files.files.rename_directory(&live, &new).map_err(repair)?;
                }
                files.files.rename_directory(&old, &live).map_err(repair)?;
            }
        }
        for (n, name) in TREES.iter().enumerate() {
            if tree_digest(&files.files, &files.paths.root().join(name), check)? != meta.old[n] {
                return Err(RecoveryFileError::RepairRequired);
            }
        }
        if !has_phase(&operation, meta.generation, 8)? {
            phase(&files.files, &operation, meta.generation, 8)?;
        }
        write_new(
            &files.files,
            &operation.join("cleanup"),
            &phase_bytes(meta.generation, 10),
        )?;
        cleanup_operation(&files.files, &operation)?;
        report.rolled_back += 1;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{manifest, Fixture};
    use super::*;
    #[test]
    fn candidate_capture_refuses_unlisted_files_or_empty_directories() {
        for directory in [false, true] {
            let f = Fixture::new();
            let workspace = f.f().workspace().unwrap();
            let database = b"approved current-security candidate";
            f.write(&workspace.candidate_database(), database);
            let manifest = manifest(b"source ordinary");
            let capture = || {
                capture_candidate_digests(
                    &workspace,
                    &manifest,
                    database.len() as u64,
                    fc_security::sha256(database),
                    &|| true,
                )
            };
            capture().unwrap();
            let injected = workspace.new_root().join("teachers").join("unlisted");
            if directory {
                drop(workspace.files.create_directory_new(&injected).unwrap());
            } else {
                f.write(&injected, b"unlisted bytes");
            }
            assert_eq!(capture(), Err(RecoveryFileError::RepairRequired));
        }
    }
    #[test]
    fn candidate_capture_refuses_changed_owner_marker_or_database() {
        for owner in [false, true] {
            let f = Fixture::new();
            let workspace = f.f().workspace().unwrap();
            let database = b"approved current-security candidate";
            f.write(&workspace.candidate_database(), database);
            let manifest = manifest(b"source ordinary");
            let capture = || {
                capture_candidate_digests(
                    &workspace,
                    &manifest,
                    database.len() as u64,
                    fc_security::sha256(database),
                    &|| true,
                )
            };
            capture().unwrap();
            let path = if owner {
                workspace
                    .new_root()
                    .join("profile-content")
                    .join(".future-classroom-owner")
            } else {
                workspace.candidate_database()
            };
            std::fs::write(path, b"unexpected changed bytes").unwrap();
            assert_eq!(capture(), Err(RecoveryFileError::RepairRequired));
        }
    }
    fn armed(f: &Fixture) -> (RecoveryWorkspace, Metadata) {
        f.write(&f.paths.database(), b"old-state");
        f.write(
            &f.paths.root().join("teachers").join("old.txt"),
            b"old-teacher",
        );
        let mut workspace = f.f().workspace().unwrap();
        f.write(&workspace.candidate_database(), b"new-state");
        f.write(
            &workspace.new_root().join("teachers").join("new.txt"),
            b"new-teacher",
        );
        let mut meta = Metadata {
            generation: 3,
            candidate_revision: fc_domain::ids::Revision::new(8),
            candidate_bytes: 9,
            candidate_sha256: fc_security::sha256(b"new-state"),
            manifest: manifest(b"ordinary"),
            old: [[0; 32]; 3],
            new: [[0; 32]; 3],
        };
        drop(
            workspace
                .files
                .create_directory_new(&workspace.operation.join("old"))
                .unwrap(),
        );
        for (n, name) in TREES.iter().enumerate() {
            meta.old[n] =
                tree_digest(&workspace.files, &f.paths.root().join(name), &|| true).unwrap();
            meta.new[n] =
                tree_digest(&workspace.files, &workspace.new_root().join(name), &|| true).unwrap();
        }
        write_new(
            &workspace.files,
            &workspace.operation.join("journal"),
            &encode(&meta).unwrap(),
        )
        .unwrap();
        phase(&workspace.files, &workspace.operation, 3, 0).unwrap();
        workspace.armed = true;
        (workspace, meta)
    }
    fn move_n(workspace: &RecoveryWorkspace, count: usize, receipts: bool) {
        for n in 0..count {
            let name = TREES[n / 2];
            let (source, target) = if n % 2 == 0 {
                (
                    workspace.files.root().join(name),
                    workspace.operation.join("old").join(name),
                )
            } else {
                (
                    workspace.new_root().join(name),
                    workspace.files.root().join(name),
                )
            };
            workspace.files.rename_directory(&source, &target).unwrap();
            if receipts {
                phase(&workspace.files, &workspace.operation, 3, (n + 1) as u8).unwrap();
            }
        }
    }
    #[test]
    fn every_rename_cut_with_and_without_following_receipt_restores_complete_old_set() {
        for receipts in [true, false] {
            for count in 0..=6 {
                let f = Fixture::new();
                let (workspace, _) = armed(&f);
                move_n(&workspace, count, receipts);
                let operation = workspace.operation.clone();
                drop(workspace);
                let report = f.f().reconcile_startup(&|| true).unwrap();
                assert_eq!(report.rolled_back, 1, "cut {count}, receipts {receipts}");
                assert_eq!(std::fs::read(f.paths.database()).unwrap(), b"old-state");
                assert!(f.paths.root().join("teachers").join("old.txt").exists());
                assert!(!f.paths.root().join("teachers").join("new.txt").exists());
                assert!(!operation.exists());
                assert_eq!(
                    f.f().reconcile_startup(&|| true).unwrap(),
                    StartupRecovery::default()
                );
            }
        }
    }
    #[test]
    fn committed_set_is_verified_before_rollback_copy_cleanup_and_later_writes_are_allowed() {
        let f = Fixture::new();
        let (workspace, _) = armed(&f);
        move_n(&workspace, 6, true);
        phase(&workspace.files, &workspace.operation, 3, 7).unwrap();
        let operation = workspace.operation.clone();
        drop(workspace);
        assert_eq!(f.f().reconcile_startup(&|| true).unwrap().installed, 1);
        assert_eq!(std::fs::read(f.paths.database()).unwrap(), b"new-state");
        assert!(!operation.exists());
        f.f().files.remove(&f.paths.database()).unwrap();
        f.write(&f.paths.database(), b"subsequent real writer changes");
        assert_eq!(
            f.f().reconcile_startup(&|| true).unwrap(),
            StartupRecovery::default()
        );
    }
    #[test]
    fn ambiguous_extra_tree_corrupt_journal_and_changed_old_bytes_preserve_all_evidence() {
        for fault in 0..3 {
            let f = Fixture::new();
            let (workspace, _) = armed(&f);
            move_n(&workspace, 1, true);
            match fault {
                0 => {
                    drop(
                        workspace
                            .files
                            .create_directory_new(&f.paths.root().join("state"))
                            .unwrap(),
                    );
                }
                1 => {
                    workspace
                        .files
                        .remove(&workspace.operation.join("journal"))
                        .unwrap();
                    write_new(
                        &workspace.files,
                        &workspace.operation.join("journal"),
                        b"truncated",
                    )
                    .unwrap();
                }
                _ => {
                    let old = workspace
                        .operation
                        .join("old")
                        .join("state")
                        .join("classroom.sqlite3");
                    workspace.files.remove(&old).unwrap();
                    write_new(&workspace.files, &old, b"tampered").unwrap();
                }
            }
            let operation = workspace.operation.clone();
            drop(workspace);
            assert!(matches!(
                f.f().reconcile_startup(&|| true),
                Err(RecoveryFileError::RepairRequired)
            ));
            assert!(operation.join("old").join("state").exists());
            assert!(operation.join("new").join("state").exists());
        }
    }
    #[test]
    fn interrupted_terminal_cleanup_is_idempotent_and_does_not_revisit_live_database() {
        let f = Fixture::new();
        let (workspace, _) = armed(&f);
        move_n(&workspace, 6, true);
        write_new(
            &workspace.files,
            &workspace.operation.join("cleanup"),
            &phase_bytes(3, 9),
        )
        .unwrap();
        workspace
            .files
            .remove(&workspace.operation.join("journal"))
            .unwrap();
        workspace
            .files
            .remove(&workspace.operation.join("owner"))
            .unwrap();
        let operation = workspace.operation.clone();
        drop(workspace);
        f.f().reconcile_startup(&|| true).unwrap();
        assert!(!operation.exists());
        assert_eq!(std::fs::read(f.paths.database()).unwrap(), b"new-state");
    }
    #[test]
    fn missing_arm_receipt_still_recovers_from_complete_journal() {
        let f = Fixture::new();
        let (workspace, _) = armed(&f);
        move_n(&workspace, 3, false);
        workspace
            .files
            .remove(&phase_path(&workspace.operation, 0))
            .unwrap();
        drop(workspace);
        assert_eq!(f.f().reconcile_startup(&|| true).unwrap().rolled_back, 1);
        assert_eq!(std::fs::read(f.paths.database()).unwrap(), b"old-state");
    }
    #[test]
    fn hardlinked_old_original_is_not_adopted_or_deleted_during_recovery() {
        let f = Fixture::new();
        let (workspace, _) = armed(&f);
        std::fs::hard_link(
            f.paths.database(),
            f.paths.root().join("quarantine").join("same-inode"),
        )
        .unwrap();
        let operation = workspace.operation.clone();
        drop(workspace);
        assert!(matches!(
            f.f().reconcile_startup(&|| true),
            Err(RecoveryFileError::RepairRequired)
        ));
        assert!(f.paths.database().exists());
        assert!(operation.join("new").join("state").exists());
    }
    #[test]
    fn missing_both_receipts_never_discards_nonempty_or_unknown_rollback_data() {
        for fault in 0..3 {
            let f = Fixture::new();
            let (workspace, _) = armed(&f);
            if fault == 0 {
                move_n(&workspace, 1, false);
            } else {
                let name = if fault == 1 {
                    "unknown-original.bin"
                } else {
                    "unknown-tree"
                };
                let path = workspace.operation.join("old").join(name);
                if fault == 1 {
                    f.write(&path, b"unknown rollback bytes must survive");
                } else {
                    drop(workspace.files.create_directory_new(&path).unwrap());
                    f.write(
                        &path.join("original.bin"),
                        b"unknown subtree bytes must survive",
                    );
                }
            }
            workspace
                .files
                .remove(&workspace.operation.join("journal"))
                .unwrap();
            workspace
                .files
                .remove(&phase_path(&workspace.operation, 0))
                .unwrap();
            let operation = workspace.operation.clone();
            let before = tree_digest(&workspace.files, &operation, &|| true).unwrap();
            drop(workspace);
            assert!(matches!(
                f.f().reconcile_startup(&|| true),
                Err(RecoveryFileError::RepairRequired)
            ));
            assert_eq!(
                tree_digest(&f.f().files, &operation, &|| true).unwrap(),
                before
            );
            if fault == 0 {
                assert!(!f.paths.database().exists());
                assert_eq!(
                    std::fs::read(operation.join("old/state/classroom.sqlite3")).unwrap(),
                    b"old-state"
                );
            } else {
                assert_eq!(std::fs::read(f.paths.database()).unwrap(), b"old-state");
            }
        }
    }
    #[test]
    fn genuinely_unarmed_workspace_is_removed_only_with_complete_live_directories() {
        for empty_old in [false, true] {
            let f = Fixture::new();
            let mut workspace = f.f().workspace().unwrap();
            if empty_old {
                drop(
                    workspace
                        .files
                        .create_directory_new(&workspace.operation.join("old"))
                        .unwrap(),
                );
            }
            f.write(&f.paths.database(), b"unchanged live bytes");
            f.write(&workspace.ordinary_database(), b"partial ordinary export");
            let operation = workspace.operation.clone();
            workspace.armed = true;
            drop(workspace); // Simulate lost process ownership.
            assert_eq!(
                f.f().reconcile_startup(&|| true).unwrap().discarded_unarmed,
                1
            );
            assert!(!operation.exists());
            assert_eq!(
                std::fs::read(f.paths.database()).unwrap(),
                b"unchanged live bytes"
            );
        }
        let f = Fixture::new();
        let mut workspace = f.f().workspace().unwrap();
        let operation = workspace.operation.clone();
        workspace.armed = true;
        drop(workspace);
        f.f()
            .files
            .rename_directory(
                &f.paths.root().join("state"),
                &f.paths.root().join("quarantine").join("moved-state"),
            )
            .unwrap();
        assert!(matches!(
            f.f().reconcile_startup(&|| true),
            Err(RecoveryFileError::RepairRequired)
        ));
        assert!(operation.exists());
    }
}
