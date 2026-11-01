//! Fixed manifest-indexed, read-only damaged file preimage. This is volatile
//! broker memory, not a backup, an engine settlement, or crash recovery.
use super::*;
use std::collections::BTreeMap;

type Object = (u32, u64);
#[derive(Debug, PartialEq, Eq)]
enum Leaf {
    KnownAbsent,
    Regular {
        object: Object,
        bytes: u64,
        sha256: [u8; 32],
    },
}
#[derive(Debug)]
pub(super) struct Snapshot {
    // All expected directories, including those actually missing. Programs is
    // mandatory and pinned: no missing ancestor above the owned product root.
    directories: BTreeMap<PathBuf, Option<Object>>,
    leaves: Vec<Leaf>,
}
impl Snapshot {
    pub(super) fn same_objects(&self, other: &Self) -> bool {
        self.directories == other.directories && self.leaves == other.leaves
    }
    pub(super) fn same_effects(&self, other: &Self) -> bool {
        // MSI rollback may replace an original file/directory object. Compare
        // the actual original damaged bytes and missing namespace, not IDs or
        // the healthy manifest. ACL/reparse/link policy is rechecked natively.
        self.directories.len() == other.directories.len()
            && self
                .directories
                .iter()
                .zip(&other.directories)
                .all(|((a, x), (b, y))| a == b && x.is_some() == y.is_some())
            && self.leaves.len() == other.leaves.len()
            && self
                .leaves
                .iter()
                .zip(&other.leaves)
                .all(|(a, b)| match (a, b) {
                    (Leaf::KnownAbsent, Leaf::KnownAbsent) => true,
                    (
                        Leaf::Regular {
                            bytes: x,
                            sha256: h,
                            ..
                        },
                        Leaf::Regular {
                            bytes: y,
                            sha256: k,
                            ..
                        },
                    ) => x == y && h == k,
                    _ => false,
                })
    }
}
fn missing(error: &windows::core::Error) -> bool {
    error.code() == ERROR_FILE_NOT_FOUND.to_hresult()
        || error.code() == ERROR_PATH_NOT_FOUND.to_hresult()
}
// This structural predicate grants traversal only through directory objects
// already captured and pinned by this same proof, up to mandatory Programs.
fn parents_pinned(
    path: &Path,
    boundary: &Path,
    directories: &BTreeMap<PathBuf, Option<Object>>,
) -> bool {
    let mut next = path.parent();
    while let Some(parent) = next.filter(|p| p.starts_with(boundary)) {
        if !directories.get(parent).is_some_and(Option::is_some) {
            return false;
        }
        next = parent.parent();
    }
    true
}
pub(super) fn capture(
    proof: &mut Proof,
    base: &Path,
    manifest: &Manifest,
    sid: &[u8],
) -> Result<Snapshot> {
    #[cfg(test)]
    {
        capture_inner(proof, base, manifest, sid, None)
    }
    #[cfg(not(test))]
    {
        capture_inner(proof, base, manifest, sid)
    }
}
// No hook or caller state is compiled into the production entrypoint.
#[cfg(test)]
pub(super) fn capture_with_directory_hook(
    proof: &mut Proof,
    base: &Path,
    manifest: &Manifest,
    sid: &[u8],
    mut hook: impl FnMut(),
) -> Result<Snapshot> {
    capture_inner(proof, base, manifest, sid, Some(&mut hook))
}
fn capture_inner(
    proof: &mut Proof,
    base: &Path,
    manifest: &Manifest,
    sid: &[u8],
    #[cfg(test)] after_directories: Option<&mut dyn FnMut()>,
) -> Result<Snapshot> {
    if manifest.entries.is_empty() || manifest.entries.len() > 72 {
        return Err(LifecycleError::Invalid);
    }
    let parent = base.parent().ok_or(LifecycleError::Invalid)?.to_owned();
    let mut expected = BTreeSet::from([parent.clone(), base.to_owned()]);
    for entry in &manifest.entries {
        let path = manifest_path(base, &entry.path)?;
        let mut ancestor = path.parent();
        while let Some(dir) = ancestor.filter(|p| p.starts_with(base)) {
            expected.insert(dir.to_owned());
            ancestor = dir.parent();
        }
    }
    let mut directories = BTreeMap::new();
    // BTreeSet's parent-before-child ordering, together with no DELETE sharing,
    // keeps every existing ancestor pinned while any child is resolved.
    for dir in &expected {
        valid_path(dir)?;
        if dir != &parent && !parents_pinned(dir, &parent, &directories) {
            // Do not resolve through an absent, unpinned intermediate ancestor.
            // Its original absence is checked again after all leaf observations.
            directories.insert(dir.clone(), None);
            continue;
        }
        let owner = proof.owner()?;
        let result = owner.open_file(
            dir,
            FILE_READ_ATTRIBUTES.0 | FILE_LIST_DIRECTORY.0 | READ_CONTROL.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            true,
            false,
        );
        let object = match result {
            Ok(handle) => {
                owner.add(handle, Role::Parent);
                let object = verify_info(handle, true)?;
                verify_acl(handle, sid, false)?;
                Some(object)
            }
            Err(error) if dir != &parent && missing(&error) => None,
            Err(error) => return Err(error.into()),
        };
        directories.insert(dir.clone(), object);
    }
    // All expected names remain closed even when some expected directories or
    // leaves are missing. Enumerate only actually existing owned directories.
    let present_dirs = directories
        .iter()
        .filter_map(|(p, id)| id.map(|_| p.clone()))
        .collect();
    exact_names(proof, base, &present_dirs, manifest)?;
    #[cfg(test)]
    if let Some(hook) = after_directories {
        hook();
    }
    let mut leaves = Vec::with_capacity(manifest.entries.len());
    let mut total = 0u64;
    for entry in &manifest.entries {
        let path = manifest_path(base, &entry.path)?;
        if !parents_pinned(&path, &parent, &directories) {
            // A regular leaf cannot be observed without every original
            // ancestor pinned. Never follow an ancestor that appeared later.
            leaves.push(Leaf::KnownAbsent);
            continue;
        }
        let deadline = proof.deadline;
        let owner = proof.owner()?;
        let handle = match owner.open_file(
            &path,
            GENERIC_READ.0 | READ_CONTROL.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        ) {
            Ok(handle) => handle,
            Err(error) if missing(&error) => {
                leaves.push(Leaf::KnownAbsent);
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        owner.add(handle, Role::Parent);
        let object = verify_info(handle, false)?;
        verify_acl(handle, sid, false)?;
        let mut bytes = 0i64;
        unsafe {
            GetFileSizeEx(handle, &mut bytes)?;
        }
        let bytes = u64::try_from(bytes).map_err(|_| LifecycleError::Unsettled)?;
        total = total.checked_add(bytes).ok_or(LifecycleError::Invalid)?;
        if total > u64::from(manifest::MAX_PAYLOAD) {
            return Err(LifecycleError::Unsettled);
        }
        let sha256 = owner.digest_preimage(handle, u64::from(manifest::MAX_PAYLOAD), deadline)?;
        leaves.push(Leaf::Regular {
            object,
            bytes,
            sha256,
        });
    }
    // Original missing directories must still be known absent. In particular
    // this rejects a new empty directory, even when all its leaves are absent.
    for (dir, object) in &directories {
        if object.is_some() {
            continue;
        }
        let owner = proof.owner()?;
        match owner.open_file(
            dir,
            FILE_READ_ATTRIBUTES.0 | FILE_LIST_DIRECTORY.0 | READ_CONTROL.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            true,
            false,
        ) {
            Ok(handle) => {
                // Record even an unexpected/reparse object before rejecting:
                // the same Proof must perform its actual checked return.
                owner.add(handle, Role::Parent);
                return Err(LifecycleError::Unsettled);
            }
            Err(error) if missing(&error) => {}
            Err(error) => return Err(error.into()),
        }
    }
    proof.check()?;
    Ok(Snapshot {
        directories,
        leaves,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(object: Object, bytes: u64, hash: u8) -> Snapshot {
        Snapshot {
            directories: BTreeMap::from([(PathBuf::from("closed"), Some(object))]),
            leaves: vec![
                Leaf::KnownAbsent,
                Leaf::Regular {
                    object,
                    bytes,
                    sha256: [hash; 32],
                },
            ],
        }
    }
    #[test]
    fn traversal_requires_every_expected_original_ancestor_pin() {
        let parent = PathBuf::from(r"C:\fixed\Programs");
        let base = parent.join("FutureClassroom");
        let web = base.join("web");
        let leaf = web.join("index.html");
        let mut dirs = BTreeMap::from([
            (parent.clone(), Some((1, 1))),
            (base.clone(), Some((1, 2))),
            (web.clone(), Some((1, 3))),
        ]);
        assert!(parents_pinned(&leaf, &parent, &dirs));
        dirs.insert(web.clone(), None);
        assert!(!parents_pinned(&leaf, &parent, &dirs));
        dirs.insert(web, Some((1, 3)));
        dirs.insert(base.clone(), None);
        assert!(!parents_pinned(&leaf, &parent, &dirs));
        dirs.insert(base, Some((1, 2)));
        dirs.remove(&parent);
        assert!(!parents_pinned(&leaf, &parent, &dirs));
    }
    #[test]
    fn rollback_requires_original_bad_bytes_and_absence_but_not_old_object_ids() {
        let original = snapshot((1, 2), 3, 4);
        let replaced = snapshot((1, 99), 3, 4);
        assert!(!original.same_objects(&replaced));
        assert!(original.same_effects(&replaced));
        assert!(!original.same_effects(&snapshot((1, 99), 2, 4)));
        assert!(!original.same_effects(&snapshot((1, 99), 3, 5)));
        let mut filled = snapshot((1, 2), 3, 4);
        filled.leaves[0] = Leaf::Regular {
            object: (1, 3),
            bytes: 0,
            sha256: [0; 32],
        };
        assert!(!original.same_effects(&filled));
        let mut missing_directory = snapshot((1, 2), 3, 4);
        *missing_directory.directories.values_mut().next().unwrap() = None;
        assert!(!original.same_effects(&missing_directory));
    }
}
