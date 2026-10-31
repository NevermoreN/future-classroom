//! A picker-created destination is an untrusted name until its worker pins the
//! local parent chain. Publication never replaces an existing user object.
use super::validation::{backup_name, copy_exact, safe_display_path, CopyError};
use crate::{
    materials::files::{ControlledFiles, LockedFile, PinnedDirectory},
    SystemRandom,
};
use fc_ports::RandomSource;
use std::{
    fs::File,
    io::{Seek, SeekFrom},
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::{Path, PathBuf},
};
use windows::{
    core::{Error, Result},
    Win32::{Foundation::*, Security::Cryptography::*, Storage::FileSystem::*},
};

const MAX_PACKAGE_BYTES: u64 = fc_domain::recovery::MAX_DATABASE_BYTES
    + fc_domain::recovery::MAX_MEDIA_BYTES
    + fc_domain::recovery::MAX_MANIFEST_BYTES as u64
    + 20;
fn denied() -> Error {
    Error::from_hresult(E_ACCESSDENIED)
}
fn invalid() -> Error {
    Error::from_hresult(E_INVALIDARG)
}
fn active(check: &mut impl FnMut() -> bool) -> Result<()> {
    if check() {
        Ok(())
    } else {
        Err(Error::from_hresult(ERROR_CANCELLED.to_hresult()))
    }
}

/// No public path constructor: selections originate only in the native picker
/// or the explicitly marked synthetic diagnostic deployment.
pub struct BackupDestination {
    path: PathBuf,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupCopyReceipt {
    /// Display text only. Never reuse as a trusted filesystem capability.
    pub path_display: String,
    pub bytes: u64,
    pub sha256: [u8; 32],
}
impl BackupDestination {
    pub(super) fn from_picker(path: PathBuf) -> Result<Self> {
        if !path.to_str().is_some_and(safe_display_path)
            || !path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(backup_name)
        {
            return Err(invalid());
        }
        Ok(Self { path })
    }
    pub fn allocation_bytes(&self) -> usize {
        self.path.as_os_str().len().saturating_mul(2)
    }
    /// Hidden diagnostics may write only beneath a synthetic deployment's
    /// quarantine. The worker still applies every real local/reparse check.
    pub fn isolated_probe(root: &Path, name: &str) -> Result<Self> {
        if root
            .file_name()
            .and_then(|n| n.to_str())
            .is_none_or(|n| !n.starts_with("FutureClassroom-Test-"))
            || !backup_name(name)
        {
            return Err(denied());
        }
        Self::from_picker(root.join("quarantine").join(name))
    }
    /// Run only on the bounded worker. Source is the immutable program-produced
    /// package pin; expected bytes/hash are its trusted export receipt. This
    /// does not infer validity or permissions from a caller-supplied path.
    pub fn copy_from(
        self,
        mut source: LockedFile,
        expected_bytes: u64,
        expected_sha256: [u8; 32],
        mut can_continue: impl FnMut() -> bool,
    ) -> Result<BackupCopyReceipt> {
        active(&mut can_continue)?;
        if expected_bytes == 0
            || expected_bytes > MAX_PACKAGE_BYTES
            || source.len()? != expected_bytes
        {
            return Err(invalid());
        }
        let parent = self.path.parent().ok_or_else(invalid)?;
        // Pins deny deleting/replacing every checked directory through the final
        // handle rename. No network drive, ADS or reparse component is accepted.
        let _parent_pin = PinnedDirectory::open(parent)?;
        let files = ControlledFiles::new(parent)?;
        files.check(&self.path)?;
        // This is only an early UX check. The final kernel rename below is the
        // authoritative no-overwrite decision under a concurrent target creation.
        match std::fs::symlink_metadata(&self.path) {
            Ok(_) => return Err(Error::from_hresult(ERROR_FILE_EXISTS.to_hresult())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        let mut random = [0u8; 16];
        SystemRandom.fill(&mut random)?;
        let mut leaf = String::from(".future-classroom-backup-");
        use std::fmt::Write as _;
        for byte in random {
            let _ = write!(&mut leaf, "{byte:02x}");
        }
        leaf.push_str(".partial");
        let mut staging = files.create_new(&parent.join(leaf))?;
        source.file_mut().seek(SeekFrom::Start(0))?;
        let mut digest = Sha256::new()?;
        copy_exact(
            source.file_mut(),
            staging.file_mut(),
            expected_bytes,
            MAX_PACKAGE_BYTES,
            &mut can_continue,
            &mut |bytes| {
                digest
                    .update(bytes)
                    .map_err(|_| std::io::Error::other("backup digest unavailable"))
            },
        )
        .map_err(|error| match error {
            CopyError::Cancelled => Error::from_hresult(ERROR_CANCELLED.to_hresult()),
            CopyError::Io(e) => e.into(),
            _ => invalid(),
        })?;
        if digest.finish()? != expected_sha256 {
            return Err(invalid());
        }
        // seal flushes and retains the exact inode with DELETE rights, without
        // closing/reopening an untrusted staging name. Drop is exact-handle-only.
        let mut owned = OwnedPublication {
            file: staging.seal()?,
            keep: false,
        };
        active(&mut can_continue)?;
        rename(owned.file.file(), &self.path)?;
        // A late cancellation retires this same published inode, never a path
        // which might now name someone else's file.
        active(&mut can_continue)?;
        owned.keep = true;
        Ok(BackupCopyReceipt {
            path_display: self.path.to_string_lossy().into_owned(),
            bytes: expected_bytes,
            sha256: expected_sha256,
        })
    }
}
struct OwnedPublication {
    file: LockedFile,
    keep: bool,
}
impl Drop for OwnedPublication {
    fn drop(&mut self) {
        if !self.keep {
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: our live sealed inode owns DELETE rights and share-0 lock.
            // Neither cancellation nor an attacker can redirect this disposition.
            unsafe {
                let _ = SetFileInformationByHandle(
                    HANDLE(self.file.file().as_raw_handle()),
                    FileDispositionInfo,
                    &disposition as *const _ as _,
                    std::mem::size_of_val(&disposition) as u32,
                );
            }
        }
    }
}
fn rename(file: &File, target: &Path) -> Result<()> {
    let name: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let bytes = offset.checked_add(name.len() * 2).ok_or_else(invalid)?;
    let mut buffer = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: aligned header+UTF16 buffer covers all writes, directory ancestors
    // remain pinned, and ReplaceIfExists=false is enforced by the kernel. Only
    // Win7's FileRenameInfo is used; FileRenameInfoEx is deliberately absent.
    unsafe {
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = HANDLE::default();
        (*info).FileNameLength = ((name.len() - 1) * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            buffer.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
            name.len(),
        );
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileRenameInfo,
            buffer.as_ptr().cast(),
            bytes as u32,
        )?;
    }
    Ok(())
}

/// Win7-compatible provider with explicit caller-owned hash-object memory.
/// No BCrypt pseudo-handles, reusable hash flags or newer OS helpers are used.
struct Sha256 {
    algorithm: BCRYPT_ALG_HANDLE,
    hash: BCRYPT_HASH_HANDLE,
    _object: Vec<u8>,
}
impl Sha256 {
    fn new() -> Result<Self> {
        let mut value = Self {
            algorithm: BCRYPT_ALG_HANDLE::default(),
            hash: BCRYPT_HASH_HANDLE::default(),
            _object: Vec::new(),
        };
        // SAFETY: synchronous CNG calls retain only the stable Vec allocation
        // until DestroyHash in Drop; all provider/hash handles are solely owned.
        unsafe {
            BCryptOpenAlgorithmProvider(
                &mut value.algorithm,
                BCRYPT_SHA256_ALGORITHM,
                None,
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
            .ok()?;
            let mut bytes = [0u8; 4];
            let mut returned = 0;
            BCryptGetProperty(
                BCRYPT_HANDLE(value.algorithm.0),
                BCRYPT_OBJECT_LENGTH,
                Some(&mut bytes),
                &mut returned,
                0,
            )
            .ok()?;
            let length = u32::from_ne_bytes(bytes) as usize;
            if returned != 4 || length == 0 || length > 1024 * 1024 {
                return Err(invalid());
            }
            value._object = vec![0; length];
            BCryptCreateHash(
                value.algorithm,
                &mut value.hash,
                Some(&mut value._object),
                None,
                0,
            )
            .ok()?;
        }
        Ok(value)
    }
    fn update(&mut self, bytes: &[u8]) -> Result<()> {
        // SAFETY: live hash handle and bounded immutable block, retained nowhere.
        unsafe { BCryptHashData(self.hash, bytes, 0).ok() }
    }
    fn finish(&mut self) -> Result<[u8; 32]> {
        let mut hash = [0; 32];
        // SAFETY: SHA256 provider writes exactly 32 bytes to the owned buffer.
        unsafe {
            BCryptFinishHash(self.hash, &mut hash, 0).ok()?;
        }
        Ok(hash)
    }
}
impl Drop for Sha256 {
    fn drop(&mut self) {
        // SAFETY: each nonnull handle is destroyed exactly once, before the
        // backing object allocation and provider go away, including early errors.
        unsafe {
            if !self.hash.0.is_null() {
                let _ = BCryptDestroyHash(self.hash);
            }
            if !self.algorithm.0.is_null() {
                let _ = BCryptCloseAlgorithmProvider(self.algorithm, 0);
            }
        }
    }
}
#[cfg(test)]
#[path = "backup_destination_tests.rs"]
mod tests;
