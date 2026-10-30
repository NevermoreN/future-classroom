//! Windows handle-pinned local files. No network/ADS/reparse/hard-link input.
//! Byte inspection and copying belong to the bounded material worker.
use std::{
    fs::{File, OpenOptions},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use windows::{
    core::{Error, Result, HSTRING},
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{
            Memory::*,
            WindowsProgramming::{DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOVABLE},
        },
    },
};

fn denied() -> Error {
    Error::from_hresult(E_ACCESSDENIED)
}
fn handle(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}
fn local(path: &Path) -> Result<()> {
    let s = path.as_os_str().to_string_lossy();
    if !path.is_absolute()
        || s.len() > 16_000
        || s.as_bytes().get(1) != Some(&b':')
        || !s.as_bytes()[0].is_ascii_alphabetic()
        || s[2..].contains([':', '\0'])
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err(denied());
    }
    let drive = HSTRING::from(format!("{}\\", &s[..2]));
    // Drive-letter UNC mappings are network inputs too.
    let kind = unsafe { GetDriveTypeW(&drive) };
    if !matches!(
        kind,
        DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_CDROM | DRIVE_RAMDISK
    ) {
        return Err(denied());
    }
    Ok(())
}
fn info(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: file owns a live kernel handle, output is synchronous and exclusive.
    unsafe {
        GetFileInformationByHandle(handle(file), &mut info)?;
    }
    Ok(info)
}
fn regular(file: &File) -> Result<()> {
    let i = info(file)?;
    if i.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0) != 0
        || i.nNumberOfLinks != 1
    {
        return Err(denied());
    }
    Ok(())
}
fn pins(path: &Path) -> Result<Vec<File>> {
    local(path)?;
    let mut out = Vec::new();
    let mut ancestors: Vec<_> = path.ancestors().skip(1).collect();
    ancestors.reverse();
    if ancestors.len() > 64 {
        return Err(denied());
    }
    for dir in ancestors {
        let f = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(dir)?;
        let i = info(&f)?;
        if i.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || i.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
        {
            return Err(denied());
        }
        out.push(f);
    }
    Ok(out)
}
/// An immutable source, including directory pins. File sharing refuses mutation
/// or replacement until this capability is released; never reopen an untrusted path.
pub struct LockedFile {
    file: File,
    path: PathBuf,
    _parents: Vec<File>,
    publish_access: bool,
}
impl LockedFile {
    pub fn open(path: &Path, max: u64) -> Result<Self> {
        Self::open_mode(path, max, false, false)
    }
    /// Owned staging: immutable bytes plus DELETE rights on this very handle.
    /// WIC reads the same handle; the PDF child explicitly shares DELETE.
    pub fn open_prepared(path: &Path, max: u64) -> Result<Self> {
        Self::open_mode(path, max, true, false)
    }
    pub fn open_helper(path: &Path, max: u64) -> Result<Self> {
        Self::open_mode(path, max, false, true)
    }
    fn open_mode(path: &Path, max: u64, publish_access: bool, helper: bool) -> Result<Self> {
        let parents = pins(path)?;
        let file = OpenOptions::new()
            .read(true)
            .access_mode(GENERIC_READ.0 | if publish_access { DELETE.0 } else { 0 })
            .share_mode(FILE_SHARE_READ.0 | if helper { FILE_SHARE_DELETE.0 } else { 0 })
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_SEQUENTIAL_SCAN.0)
            .open(path)?;
        regular(&file)?;
        let size = file.metadata()?.len();
        if size == 0 || size > max {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        Ok(Self {
            file,
            path: path.to_owned(),
            _parents: parents,
            publish_access,
        })
    }
    pub fn file(&self) -> &File {
        &self.file
    }
    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn len(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }
    pub fn publish(mut self, files: &ControlledFiles, target: &Path) -> Result<Self> {
        if !self.publish_access {
            return Err(denied());
        }
        files.check(&self.path)?;
        files.check(target)?;
        let parents = pins(target)?;
        rename_handle(&self.file, target)?;
        self.path = target.to_owned();
        self._parents = parents;
        Ok(self)
    }
    pub fn map(self) -> Result<Arc<ReadOnlyMap>> {
        let len = usize::try_from(self.len()?).map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        // SAFETY: immutable locked file remains alive through the mapping's lifetime.
        let mapping =
            unsafe { CreateFileMappingW(handle(&self.file), None, PAGE_READONLY, 0, 0, None)? };
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, len) };
        if view.Value.is_null() {
            unsafe {
                let _ = CloseHandle(mapping);
            }
            return Err(Error::from_thread());
        }
        Ok(Arc::new(ReadOnlyMap {
            view,
            mapping,
            len,
            _source: self,
        }))
    }
}
/// A read-only mapping of a pinned file, without a 50MiB input Vec allocation.
pub struct ReadOnlyMap {
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    mapping: HANDLE,
    len: usize,
    _source: LockedFile,
}
// SAFETY: the view is immutable, File sharing denies writes/truncation; all handles
// remain owned until the final Arc is dropped. There are no thread-affine objects.
unsafe impl Send for ReadOnlyMap {}
unsafe impl Sync for ReadOnlyMap {}
impl AsRef<[u8]> for ReadOnlyMap {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: live file mapping covers exactly len readable immutable bytes.
        unsafe { std::slice::from_raw_parts(self.view.Value.cast(), self.len) }
    }
}
impl Drop for ReadOnlyMap {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.mapping);
        }
    }
}

fn rename_handle(file: &File, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    // The Win32 wrapper resolves this DOS path before the kernel rename.
    // Keep an explicit terminator even though FileNameLength excludes it; never
    // let that normalization inspect the next heap allocation on exact alignment.
    let name: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let bytes = offset.checked_add(name.len() * 2).ok_or_else(denied)?;
    let mut buffer = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: usize-aligned allocation covers the header plus the full UTF16
    // tail, no aliasing pointer survives this synchronous call. No replacement.
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
            handle(file),
            FileRenameInfo,
            buffer.as_ptr().cast(),
            bytes as u32,
        )?;
    }
    Ok(())
}

/// Private deployment tree. New subdirectories inherit its protected DACL.
/// Call only after DataPaths::prepare_for_current_user/read_back and hold the
/// deployment's instance lock. Every component is checked before returning.
pub struct ControlledFiles {
    root: PathBuf,
}
impl ControlledFiles {
    pub fn new(root: &Path) -> Result<Self> {
        local(root)?;
        let _ = pins(&root.join(".pin"))?;
        let metadata = std::fs::symlink_metadata(root)?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(denied());
        }
        Ok(Self {
            root: root.to_owned(),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn directory(&self, components: &[String]) -> Result<PathBuf> {
        let mut path = self.root.clone();
        for part in components {
            if part.is_empty()
                || part.len() > 80
                || !part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            {
                return Err(denied());
            }
            path.push(part);
            match std::fs::create_dir(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.is_dir()
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            {
                return Err(denied());
            }
            let _ = pins(&path.join(".pin"))?;
        }
        Ok(path)
    }
    pub fn create_new(&self, path: &Path) -> Result<NewFile> {
        self.check(path)?;
        let parents = pins(path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .access_mode(GENERIC_READ.0 | GENERIC_WRITE.0 | DELETE.0)
            .create_new(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_WRITE_THROUGH.0)
            .open(path)?;
        regular(&file)?;
        Ok(NewFile {
            file: Some(file),
            path: path.to_owned(),
            _parents: parents,
            keep: false,
        })
    }
    pub fn check(&self, path: &Path) -> Result<()> {
        local(path)?;
        if !path.starts_with(&self.root) || path == self.root {
            return Err(denied());
        }
        Ok(())
    }
    /// Bounded one-directory enumeration; refuses excess entries and every link.
    /// Returned paths are names only: callers open/pin again before byte access.
    pub fn entries(&self, path: &Path, limit: usize) -> Result<Vec<PathBuf>> {
        self.check(path)?;
        let _parents = pins(&path.join(".pin"))?;
        if limit == 0 || limit > 32768 {
            return Err(denied());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if out.len() == limit {
                return Err(denied());
            }
            let path = entry.path();
            self.check(&path)?;
            let m = std::fs::symlink_metadata(&path)?;
            if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                return Err(denied());
            }
            out.push(path);
        }
        Ok(out)
    }
    /// Same-volume, non-replacing rename of the exact exclusively held inode.
    /// Only the Win7 FileRenameInfo class is used, never FileRenameInfoEx.
    pub fn publish(&self, source: &Path, target: &Path) -> Result<()> {
        self.check(source)?;
        self.check(target)?;
        let _source_parents = pins(source)?;
        let _target_parents = pins(target)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .access_mode(GENERIC_READ.0 | GENERIC_WRITE.0 | DELETE.0)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(source)?;
        regular(&file)?;
        file.sync_all()?;
        rename_handle(&file, target)?;
        Ok(())
    }
    pub fn remove(&self, path: &Path) -> Result<bool> {
        self.check(path)?;
        let _parents = pins(path)?;
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
            Ok(m) => {
                if !m.is_file() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                    return Err(denied());
                }
            }
        }
        // An exclusive handle proves no external reader/writer currently holds it.
        let file = OpenOptions::new()
            .read(true)
            .access_mode(GENERIC_READ.0 | DELETE.0)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)?;
        regular(&file)?;
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        // Delete this pinned inode, not whatever might later occupy its name.
        unsafe {
            SetFileInformationByHandle(
                handle(&file),
                FileDispositionInfo,
                &disposition as *const _ as _,
                std::mem::size_of_val(&disposition) as u32,
            )?;
        }
        drop(file);
        Ok(true)
    }
}
pub struct NewFile {
    file: Option<File>,
    path: PathBuf,
    _parents: Vec<File>,
    keep: bool,
}
impl NewFile {
    pub fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("new file remains open until finish")
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Flush durable bytes before the caller may publish a database reference.
    /// The destination is a new immutable resource version, never an overwrite.
    pub fn finish(mut self) -> Result<PathBuf> {
        self.file.as_ref().expect("new file open").sync_all()?;
        self.keep = true;
        self.file.take();
        Ok(self.path.clone())
    }
}
impl Drop for NewFile {
    fn drop(&mut self) {
        if !self.keep {
            if let Some(file) = &self.file {
                let d = FILE_DISPOSITION_INFO { DeleteFile: true };
                // SAFETY: uniquely created, still exclusively held file with
                // DELETE access. Cancellation cannot target a replacement path.
                unsafe {
                    let _ = SetFileInformationByHandle(
                        handle(file),
                        FileDispositionInfo,
                        &d as *const _ as _,
                        std::mem::size_of_val(&d) as u32,
                    );
                }
            }
        }
        self.file.take();
    }
}

/// Launch copies may be edited by their associated application. Pins prevent
/// name substitution while Shell is dispatching, without blocking Office's first
/// read/write open. This type cannot create a read-only byte mapping.
pub struct LaunchPath {
    path: PathBuf,
    _file: File,
    _parents: Vec<File>,
}
impl LaunchPath {
    /// Open the editable copy while the verified read handle still denies all
    /// writes/deletion. No path substitution or in-place change can enter this gap.
    pub fn from_verified(source: LockedFile, max: u64) -> Result<Self> {
        let launch = Self::open(source.path(), max)?;
        let before = info(source.file())?;
        let after = info(&launch._file)?;
        if (
            before.dwVolumeSerialNumber,
            before.nFileIndexHigh,
            before.nFileIndexLow,
        ) != (
            after.dwVolumeSerialNumber,
            after.nFileIndexHigh,
            after.nFileIndexLow,
        ) {
            return Err(denied());
        }
        drop(source);
        Ok(launch)
    }

    pub fn open(path: &Path, max: u64) -> Result<Self> {
        let parents = pins(path)?;
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)?;
        regular(&file)?;
        let size = file.metadata()?.len();
        if size == 0 || size > max {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        Ok(Self {
            path: path.to_owned(),
            _file: file,
            _parents: parents,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "FutureClassroom-Test-Files-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("owned native files test cleanup");
        }
    }
    #[test]
    fn private_file_publish_cancel_mapping_and_exclusive_cleanup() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let tree = Tree::new();
        let files = ControlledFiles::new(&tree.0).unwrap();
        let dir = files.directory(&["staging".into()]).unwrap();
        let stage = dir.join("one.txt");
        {
            let mut f = files.create_new(&stage).unwrap();
            f.file_mut().write_all(b"canceled").unwrap();
        }
        assert!(!stage.exists());
        let mut f = files.create_new(&stage).unwrap();
        f.file_mut().write_all(b"immutable bytes").unwrap();
        f.finish().unwrap();
        let dest = files
            .directory(&["teachers".into(), "1".into()])
            .unwrap()
            .join("2.txt");
        files.publish(&stage, &dest).unwrap();
        assert!(!stage.exists());
        let locked = LockedFile::open(&dest, 100).unwrap();
        assert!(OpenOptions::new().write(true).open(&dest).is_err());
        assert!(files.remove(&dest).is_err());
        assert!(std::fs::rename(dest.parent().unwrap(), tree.0.join("moved")).is_err());
        let map = locked.map().unwrap();
        assert_eq!(AsRef::<[u8]>::as_ref(&*map), b"immutable bytes");
        drop(map);
        let mut f = files.create_new(&stage).unwrap();
        f.file_mut().write_all(b"replacement").unwrap();
        f.finish().unwrap();
        assert!(files.publish(&stage, &dest).is_err());
        let mut actual = String::new();
        File::open(&dest)
            .unwrap()
            .read_to_string(&mut actual)
            .unwrap();
        assert_eq!(actual, "immutable bytes");
        let launch = LaunchPath::open(&dest, 100).unwrap();
        assert!(OpenOptions::new().write(true).open(&dest).is_ok());
        assert!(files.remove(&dest).is_err());
        drop(launch);
        assert!(files.remove(&dest).unwrap());
        assert!(!files.remove(&dest).unwrap());
        assert!(files.remove(&stage).unwrap());
        // Alternate UTF16 path parity, and publish the very handle used for
        // validation. This catches wrapper normalization reading beyond the tail.
        for index in 0..32 {
            let stage = dir.join(format!("stage-{index}.part"));
            let mut out = files.create_new(&stage).unwrap();
            out.file_mut().write_all(b"verified").unwrap();
            out.finish().unwrap();
            let verified = LockedFile::open_prepared(&stage, 100).unwrap();
            let target = dir.join(format!(
                "资料{}{}.txt",
                index,
                if index % 2 == 0 { "a" } else { "ab" }
            ));
            let verified = verified.publish(&files, &target).unwrap();
            assert!(target.is_file());
            assert!(!stage.exists());
            assert!(files.remove(&target).is_err());
            drop(verified);
            assert!(files.remove(&target).unwrap());
        }
    }
    #[test]
    fn native_file_capabilities_reject_aliases_and_oversized_enumeration() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let tree = Tree::new();
        let files = ControlledFiles::new(&tree.0).unwrap();
        assert!(files.directory(&["..".into()]).is_err());
        assert!(LockedFile::open(Path::new("\\\\server\\share\\file.txt"), 100).is_err());
        assert!(LockedFile::open(&tree.0.join("a.txt:stream"), 100).is_err());
        let dir = files.directory(&["items".into()]).unwrap();
        let a = dir.join("a.txt");
        std::fs::write(&a, b"abc").unwrap();
        assert!(LockedFile::open(&a, 2).is_err());
        let b = dir.join("b.txt");
        std::fs::hard_link(&a, &b).unwrap();
        assert!(LockedFile::open(&a, 10).is_err());
        assert!(files.remove(&a).is_err());
        assert!(files.entries(&dir, 1).is_err());
        assert_eq!(files.entries(&dir, 2).unwrap().len(), 2);
    }
}
