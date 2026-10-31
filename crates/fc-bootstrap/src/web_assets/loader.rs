//! Read a caller-selected trusted package once on its existing startup worker.
//! Requests never supply a filename and no native file survives this method.
use super::*;
use fc_runtime::worker::JobContext;
use fc_windows::materials::files::LockedFile;
use std::{
    io::Read,
    path::{Component, Path},
};

const READ_BLOCK: usize = 32 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticLoadError {
    InvalidRoot,
    Cancelled,
    FileUnavailable,
    LengthMismatch,
    Allocation,
    Assets(StaticError),
}
fn checkpoint(context: &JobContext) -> Result<(), StaticLoadError> {
    context.checkpoint().map_err(|_| StaticLoadError::Cancelled)
}
fn read_pin(
    pin: &mut LockedFile,
    limit: usize,
    expected: Option<usize>,
    context: &JobContext,
) -> Result<Vec<u8>, StaticLoadError> {
    read_pin_checked(pin, limit, expected, &|| checkpoint(context))
}
fn read_pin_checked(
    pin: &mut LockedFile,
    limit: usize,
    expected: Option<usize>,
    checkpoint: &dyn Fn() -> Result<(), StaticLoadError>,
) -> Result<Vec<u8>, StaticLoadError> {
    checkpoint()?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected.unwrap_or(limit))
        .map_err(|_| StaticLoadError::Allocation)?;
    let bounded = limit
        .checked_add(1)
        .ok_or(StaticLoadError::LengthMismatch)?;
    let mut block = [0; READ_BLOCK];
    loop {
        checkpoint()?;
        let room = bounded
            .checked_sub(bytes.len())
            .ok_or(StaticLoadError::LengthMismatch)?;
        if room == 0 {
            return Err(StaticLoadError::LengthMismatch);
        }
        let count = pin
            .file_mut()
            .read(&mut block[..room.min(READ_BLOCK)])
            .map_err(|_| StaticLoadError::FileUnavailable)?;
        checkpoint()?;
        if count == 0 {
            break;
        }
        let end = bytes
            .len()
            .checked_add(count)
            .ok_or(StaticLoadError::LengthMismatch)?;
        if end > limit {
            return Err(StaticLoadError::LengthMismatch);
        }
        bytes
            .try_reserve_exact(count)
            .map_err(|_| StaticLoadError::Allocation)?;
        bytes.extend_from_slice(&block[..count]);
    }
    if expected.is_some_and(|expected| bytes.len() != expected) {
        return Err(StaticLoadError::LengthMismatch);
    }
    checkpoint()?;
    Ok(bytes)
}
impl StaticAssets {
    /// Use on the application's original startup worker before READY/listening.
    /// The root is trusted deployment input, never a decoded request path. The
    /// manifest and every payload are read from the same protected handle used
    /// for size/regular/reparse/link checks; parent pins prevent replacement.
    /// Undeclared files are not enumerated, read or exposed. Missing, duplicate,
    /// extra supplied payloads and every declared SHA are checked by new().
    pub fn load_in_job(
        prefix: &ListenPrefix,
        trusted_absolute_package_root: &Path,
        context: &JobContext,
    ) -> Result<Self, StaticLoadError> {
        checkpoint(context)?;
        let root = trusted_absolute_package_root;
        if !root.is_absolute()
            || root
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(StaticLoadError::InvalidRoot);
        }
        let mut manifest_pin = LockedFile::open(
            &root.join("fc-assets.json"),
            fc_protocol::MAX_JSON_BYTES as u64,
        )
        .map_err(|_| StaticLoadError::FileUnavailable)?;
        let manifest_bytes = read_pin(
            &mut manifest_pin,
            fc_protocol::MAX_JSON_BYTES,
            None,
            context,
        )?;
        let manifest = Manifest::parse(&manifest_bytes)
            .map_err(|_| StaticLoadError::Assets(StaticError::InvalidManifest))?;
        checkpoint(context)?;
        let mut payloads = Vec::with_capacity(manifest.assets.len());
        for asset in manifest.assets {
            checkpoint(context)?;
            let size =
                usize::try_from(asset.size_bytes).map_err(|_| StaticLoadError::LengthMismatch)?;
            let mut pin = LockedFile::open(&root.join(&asset.path), asset.size_bytes)
                .map_err(|_| StaticLoadError::FileUnavailable)?;
            let bytes = read_pin(&mut pin, size, Some(size), context)?;
            // Verification is bounded to the declared <=5MiB bundle and does
            // not retain a JobContext or native pin in the resulting bytes.
            if !asset.verify(&bytes) {
                return Err(StaticLoadError::Assets(StaticError::PayloadMismatch));
            }
            checkpoint(context)?;
            payloads.push((asset.path, bytes));
        }
        let assets =
            Self::new(prefix, &manifest_bytes, payloads).map_err(StaticLoadError::Assets)?;
        checkpoint(context)?;
        // All original pins, including the manifest/root pins, close here on
        // this worker before immutable bytes may be handed to Net.
        drop(manifest_pin);
        Ok(assets)
    }
}
#[cfg(test)]
mod tests;
