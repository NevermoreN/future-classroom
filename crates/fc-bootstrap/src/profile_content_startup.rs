//! Trusted startup physical ownership handoff. Invoke only while owning the
//! deployment instance lock and BEFORE this run admits any preparation worker.
//! Neither local login nor a management lease grants this lifecycle authority.
use crate::{
    backend::{Backend, BackendError, MediaEntry, MediaState},
    profile_content_files::{ContentFileLocation, ControlledContentFiles, OwnedContentEntry},
};
use fc_domain::ids::MediaVersionId;
use std::collections::BTreeMap;
const MAX_REGISTRY: usize = 8192;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartupContentReport {
    pub physical_entries: u64,
    pub registry_entries: u64,
    pub verified_originals: u64,
    pub settled_reservations: u64,
    pub retired_resources: u64,
    pub finalized_resources: u64,
    pub removed_orphans: u64,
}
fn repair() -> BackendError {
    BackendError::Storage(fc_storage::StorageError::Database)
}
fn registry(backend: &Backend) -> Result<BTreeMap<MediaVersionId, MediaEntry>, BackendError> {
    let mut entries = BTreeMap::new();
    let mut after = None;
    loop {
        let page = backend.inventory_profile_media(after, 128)?;
        if page.items.len() > 128 || page.items.is_empty() && page.next_after.is_some() {
            return Err(repair());
        }
        let mut last = after;
        for entry in page.items {
            if entries.len() >= MAX_REGISTRY
                || last.is_some_and(|n| entry.id <= n)
                || entries.insert(entry.id, entry.clone()).is_some()
            {
                return Err(repair());
            }
            last = Some(entry.id);
        }
        if let Some(next) = page.next_after {
            if Some(next) != last || after.is_some_and(|previous| next <= previous) {
                return Err(repair());
            }
            after = Some(next);
        } else {
            break;
        }
    }
    Ok(entries)
}
/// First performs a read-only, whole-namespace/whole-registry validation. Any
/// unknown name/link, active missing/mismatched original, or writer fault stops
/// recovery before deleting ANY file. Only then may orphan removal and the
/// retire -> physical exclusive delete -> finalize protocol return quota.
pub fn reconcile_after_restart(
    backend: &Backend,
    files: &ControlledContentFiles,
) -> Result<StartupContentReport, BackendError> {
    let physical = files.owned_inventory().map_err(|_| repair())?;
    let mut records = registry(backend)?;
    if backend.state()?.installation.is_none() && (!records.is_empty() || !physical.is_empty()) {
        return Err(repair());
    }
    let mut report = StartupContentReport {
        physical_entries: physical.len() as u64,
        registry_entries: records.len() as u64,
        ..Default::default()
    };
    let mut by_id: BTreeMap<MediaVersionId, Vec<&OwnedContentEntry>> = BTreeMap::new();
    for entry in &physical {
        by_id.entry(entry.id).or_default().push(entry);
    }
    for entry in records.values() {
        match entry.state {
            MediaState::Referenced => {
                let resource = entry.resource.as_ref().ok_or_else(repair)?;
                let actual = by_id.get(&entry.id).ok_or_else(repair)?;
                if entry.refs == 0
                    || resource.version_id != entry.id
                    || resource.owner != entry.owner
                    || actual.len() != 1
                    || actual[0].location != ContentFileLocation::Original(resource.format)
                    || actual[0].bytes != resource.bytes
                {
                    return Err(repair());
                }
                files.verify_original(resource).map_err(|_| repair())?;
                report.verified_originals += 1;
            }
            MediaState::Reserved => {
                if entry.refs != 0 || entry.resource.is_some() {
                    return Err(repair());
                }
            }
            MediaState::Unreferenced | MediaState::Retiring | MediaState::Removed => {
                if entry.refs != 0 {
                    return Err(repair());
                }
            }
        }
    }
    // No physical destruction occurs before all referenced bytes have passed.
    backend.state()?;
    for entry in &physical {
        if !records.contains_key(&entry.id) {
            backend.state()?;
            files
                .remove_orphan_after_restart(entry)
                .map_err(|_| repair())?;
            report.removed_orphans += 1;
        }
    }
    for entry in records.values() {
        if entry.state == MediaState::Reserved {
            backend.settle_profile_media_after_restart(entry.id, entry.revision)?;
            report.settled_reservations += 1;
        }
    }
    // Settlement advances exact registry revisions and can cancel a pending
    // admission; never retire using the stale pre-settlement revision snapshot.
    if report.settled_reservations > 0 {
        records = registry(backend)?;
    }
    for entry in records.values() {
        match entry.state {
            MediaState::Referenced => {}
            MediaState::Reserved => return Err(repair()),
            MediaState::Removed => {
                if let Some(leftovers) = by_id.get(&entry.id) {
                    for item in leftovers {
                        backend.state()?;
                        files
                            .remove_orphan_after_restart(item)
                            .map_err(|_| repair())?;
                        report.removed_orphans += 1;
                    }
                }
            }
            MediaState::Unreferenced | MediaState::Retiring => {
                let ticket = backend.retire_profile_media(entry.id, entry.revision)?;
                report.retired_resources += 1;
                backend.state()?;
                files.remove_retired(&ticket).map_err(|_| repair())?;
                backend.finalize_profile_media_removal(ticket)?;
                report.finalized_resources += 1;
            }
        }
    }
    // Validate the final physical/registry correspondence before allowing any new
    // reservation. In particular a partial physical deletion never frees quota.
    let remaining = files.owned_inventory().map_err(|_| repair())?;
    let final_records = registry(backend)?;
    if remaining.len()
        != final_records
            .values()
            .filter(|e| e.state == MediaState::Referenced)
            .count()
    {
        return Err(repair());
    }
    for item in remaining {
        let e = final_records.get(&item.id).ok_or_else(repair)?;
        let r = e.resource.as_ref().ok_or_else(repair)?;
        if e.state != MediaState::Referenced
            || e.refs == 0
            || item.location != ContentFileLocation::Original(r.format)
            || item.bytes != r.bytes
        {
            return Err(repair());
        }
    }
    Ok(report)
}
#[cfg(test)]
#[path = "profile_content_startup_tests.rs"]
mod tests;
