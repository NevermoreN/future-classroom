//! Original-resource, sealed-source qualified predecessor; never a public role.
use super::*;
use crate::installer_lifecycle::guard_contract::v2::ObservedPackage;
#[derive(Debug)]
pub(super) struct VerifiedUpgradePlan {
    pub(super) old: Manifest,
    pub(super) current_auxiliary: super::super::super::broker::carrier::AuxiliaryIdentity,
    pub(super) predecessor_auxiliary: super::super::super::broker::carrier::AuxiliaryIdentity,
    pub(super) union: Manifest,
    pub(super) registration: maintenance::Registration,
    pub(super) cache_object: (u32, u64),
    pub(super) cache_bytes: u64,
    pub(super) cache_sha: [u8; 32],
    pub(super) source_sha: [u8; 32],
    pub(super) old_package: String,
    pub(super) checked_msi_closes: u32,
}
impl VerifiedUpgradePlan {
    pub(super) fn matches(&self, s: &Session) -> bool {
        s.captured_source.as_ref().is_some_and(|v| {
            v.source_sha256 == self.source_sha
                && self.old.product != v.manifest.product
                && version(&self.old.version)
                    .zip(version(&v.manifest.version))
                    .is_some_and(|(a, b)| a < b)
                && self.checked_msi_closes != 0
                && v.auxiliary().ok() == Some(self.current_auxiliary)
                && v.predecessor.as_ref().and_then(|p| p.auxiliary().ok())
                    == Some(self.predecessor_auxiliary)
        })
    }
    pub(super) fn packages(&self, s: &Session) -> Result<[ObservedPackage; 2]> {
        if !self.matches(s) {
            return Err(LifecycleError::Invalid);
        }
        let new = s.captured_source.as_ref().ok_or(LifecycleError::Invalid)?;
        Ok([
            ObservedPackage::new(&new.manifest.product, &new.package, &new.manifest.version)?,
            ObservedPackage::new(&self.old.product, &self.old_package, &self.old.version)?,
        ])
    }
}
fn version(v: &str) -> Option<[u32; 3]> {
    let a: Vec<_> = v
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    a.try_into().ok()
}
pub(super) fn clone_manifest(m: &Manifest) -> Manifest {
    Manifest {
        product: m.product.clone(),
        version: m.version.clone(),
        entries: m
            .entries
            .iter()
            .map(|e| manifest::Entry {
                path: e.path.clone(),
                bytes: e.bytes,
                sha256: e.sha256,
                component: e.component.clone(),
            })
            .collect(),
    }
}
pub(super) fn union(old: &Manifest, new: &Manifest) -> Result<Manifest> {
    let mut entries = std::collections::BTreeMap::new();
    for e in old.entries.iter().chain(&new.entries) {
        let item = entries
            .entry(e.path.clone())
            .or_insert_with(|| manifest::Entry {
                path: e.path.clone(),
                bytes: e.bytes,
                sha256: e.sha256,
                component: e.component.clone(),
            });
        item.bytes = item.bytes.max(e.bytes);
    }
    if entries.len() > 72
        || entries
            .values()
            .try_fold(0u64, |a, e| a.checked_add(u64::from(e.bytes)))
            .is_none_or(|v| v > u64::from(manifest::MAX_PAYLOAD))
    {
        return Err(LifecycleError::Invalid);
    }
    // Case-folded aliases were rejected by each manifest; reject cross-release
    // collisions too, without reinterpreting case on the Windows filesystem.
    let mut folded = BTreeSet::new();
    for n in entries.keys() {
        if !folded.insert(n.to_ascii_lowercase()) {
            return Err(LifecycleError::Invalid);
        }
    }
    Ok(Manifest {
        product: old.product.clone(),
        version: old.version.clone(),
        entries: entries.into_values().collect(),
    })
}
// Closed ABI2 grammar: an initial three-stream or published four-stream old
// package, and exactly four streams for the new package. The old fourth stream
// is parsed as bounded data by the same Reader, never executed recursively.
pub(super) fn binary_names(new: bool, names: &BTreeSet<String>) -> bool {
    let mut expected: BTreeSet<String> = [
        "FcControlledGuardDll",
        "FcGuardClientTemplate",
        "FcPayloadManifest",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if !new && names == &expected {
        return true;
    }
    expected.insert("FcPublishedPredecessor".into());
    names == &expected
}
pub(super) fn revalidate(s: &mut Session) -> Result<()> {
    let Some(proof) = &s.verified_upgrade else {
        return Ok(());
    };
    if !proof.matches(s) {
        return Err(LifecycleError::Invalid);
    }
    maintenance::revalidate_upgrade(s)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn m(product: &str, path: &str, size: u32) -> Manifest {
        Manifest {
            product: product.into(),
            version: "0.2.0".into(),
            entries: vec![manifest::Entry {
                path: path.into(),
                bytes: size,
                sha256: [1; 32],
                component: "99378807-098D-5F04-8D4D-F181DFA218D6".into(),
            }],
        }
    }
    #[test]
    fn predecessor_and_successor_binary_sets_reject_unknown_or_cross_schema() {
        let old: BTreeSet<String> = [
            "FcControlledGuardDll",
            "FcGuardClientTemplate",
            "FcPayloadManifest",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert!(binary_names(false, &old));
        assert!(!binary_names(true, &old));
        let mut new = old.clone();
        new.insert("FcPublishedPredecessor".into());
        assert!(binary_names(true, &new));
        assert!(binary_names(false, &new));
        new.insert("Other".into());
        assert!(!binary_names(true, &new));
        assert!(!binary_names(false, &new));
        let mut missing = old;
        missing.remove("FcControlledGuardDll");
        assert!(!binary_names(false, &missing));
    }
    #[test]
    fn upgrade_union_counts_shared_path_max_and_rejects_cross_alias_or_overbudget() {
        let a = m("old", "LICENSE", 10);
        let b = m("new", "LICENSE", 20);
        assert_eq!(union(&a, &b).unwrap().entries[0].bytes, 20);
        assert!(union(&a, &m("new", "license", 10)).is_err());
        assert!(union(&a, &m("new", "web/assets/a.js", manifest::MAX_PAYLOAD)).is_err());
    }
}
