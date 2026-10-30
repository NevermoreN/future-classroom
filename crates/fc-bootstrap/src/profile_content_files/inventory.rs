//! Startup-only physical namespace inspection. Complete validation precedes any
//! deletion; unknown filenames/links are never silently adopted or removed.
use super::*;
use std::collections::HashSet;
const MAX_OWNED_ENTRIES: usize = 32768;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentFileLocation {
    Original(ImageFormat),
    Quarantine,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedContentEntry {
    pub id: MediaVersionId,
    pub location: ContentFileLocation,
    pub bytes: u64,
}
fn corrupt() -> Error {
    Error::Unavailable
}
impl ControlledContentFiles {
    /// Requires deployment instance ownership. Returns only canonical metadata,
    /// never external paths; both subdirectories together are bounded to32768.
    pub fn owned_inventory(&self) -> Result<Vec<OwnedContentEntry>, Error> {
        let originals = self
            .files
            .entries(&self.originals, MAX_OWNED_ENTRIES)
            .map_err(unavailable)?;
        let remaining = MAX_OWNED_ENTRIES - originals.len();
        let quarantine = self
            .files
            .entries(&self.quarantine, remaining.max(1))
            .map_err(unavailable)?;
        if originals.len() + quarantine.len() > MAX_OWNED_ENTRIES {
            return Err(corrupt());
        }
        let mut entries = Vec::with_capacity(originals.len() + quarantine.len());
        let mut names = HashSet::new();
        for (path, is_original) in originals
            .into_iter()
            .map(|p| (p, true))
            .chain(quarantine.into_iter().map(|p| (p, false)))
        {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(corrupt)?;
            let (stem, ext) = name.split_once('.').ok_or_else(corrupt)?;
            if stem.len() != 20 || !stem.bytes().all(|b| b.is_ascii_digit()) {
                return Err(corrupt());
            }
            let id = stem
                .parse::<u64>()
                .ok()
                .and_then(|n| MediaVersionId::new(n).ok())
                .ok_or_else(corrupt)?;
            let location = if is_original {
                ContentFileLocation::Original(match ext {
                    "png" => ImageFormat::Png,
                    "jpg" => ImageFormat::Jpeg,
                    "bmp" => ImageFormat::Bmp,
                    _ => return Err(corrupt()),
                })
            } else {
                if ext != "part" {
                    return Err(corrupt());
                }
                ContentFileLocation::Quarantine
            };
            let expected = self.owned_path(id, location);
            if expected != path || !names.insert(path.clone()) {
                return Err(corrupt());
            }
            let bytes = self
                .files
                .inspect_regular(&path, MAX_CONTENT_IMAGE_BYTES, !is_original)
                .map_err(unavailable)?;
            entries.push(OwnedContentEntry {
                id,
                location,
                bytes,
            });
        }
        entries.sort_by_key(|e| {
            (
                e.id.get(),
                match e.location {
                    ContentFileLocation::Original(ImageFormat::Png) => 0,
                    ContentFileLocation::Original(ImageFormat::Jpeg) => 1,
                    ContentFileLocation::Original(ImageFormat::Bmp) => 2,
                    ContentFileLocation::Quarantine => 3,
                },
            )
        });
        Ok(entries)
    }
    fn owned_path(&self, id: MediaVersionId, location: ContentFileLocation) -> PathBuf {
        match location {
            ContentFileLocation::Original(format) => self.original(id, format),
            ContentFileLocation::Quarantine => self.staging(id),
        }
    }
    /// Trusted startup only, after the COMPLETE namespace and registry have been
    /// validated under exclusive deployment ownership/no active workers.
    pub(crate) fn remove_orphan_after_restart(
        &self,
        entry: &OwnedContentEntry,
    ) -> Result<(), Error> {
        let path = self.owned_path(entry.id, entry.location);
        let bytes = self
            .files
            .inspect_regular(
                &path,
                MAX_CONTENT_IMAGE_BYTES,
                entry.location == ContentFileLocation::Quarantine,
            )
            .map_err(unavailable)?;
        if bytes != entry.bytes {
            return Err(corrupt());
        }
        self.files.remove(&path).map_err(unavailable)?;
        Ok(())
    }
    /// Same immutable inode supplies both length and actual streaming digest.
    /// Startup verification intentionally does not redo expensive WIC decoding.
    pub(crate) fn verify_original(&self, reference: &MediaReference) -> Result<(), Error> {
        reference.validate().map_err(Error::Domain)?;
        let mut pinned = LockedFile::open(
            &self.original(reference.version_id, reference.format),
            MAX_CONTENT_IMAGE_BYTES,
        )
        .map_err(unavailable)?;
        let control = Control::new(&|| true)?;
        let (bytes, sha) = hash_file(pinned.file_mut(), &control).map_err(validation)?;
        if bytes != reference.bytes || sha != reference.sha256 {
            return Err(corrupt());
        }
        Ok(())
    }
}
