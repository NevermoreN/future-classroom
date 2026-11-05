//! Bounded, hash-addressed update payload cache. This is not a TUF verifier.
//!
//! The bootstrap must supply a private current-user directory, hold its single
//! instance lease, and pass only freshly verified target metadata. Callers must
//! perform the package whitelist/source preflight in `Download::finish` before
//! any blob becomes shareable. No network, database, thread, or install access.
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub const MAX_PAYLOAD_BYTES: u64 = 128 * 1024 * 1024;
pub const CACHE_QUOTA_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 256;
const CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub enum CacheError {
    Io(io::Error),
    InvalidTarget,
    UnsafePath,
    Busy,
    Full,
    NotReady,
    LengthMismatch,
    HashMismatch,
    InvalidPackage,
    Cancelled,
}
impl From<io::Error> for CacheError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
/// Trust is supplied by the caller's verified metadata, never by a peer name.
#[derive(Clone, Copy, Debug)]
pub struct VerifiedTarget {
    pub sha256: [u8; 32],
    pub length: u64,
}
impl VerifiedTarget {
    fn validate(self) -> Result<Self, CacheError> {
        if self.length == 0 || self.length > MAX_PAYLOAD_BYTES {
            return Err(CacheError::InvalidTarget);
        }
        Ok(self)
    }
}
struct Entry {
    ready: bool,
    length: u64,
    used: u64,
    owners: usize,
}
struct State {
    entries: BTreeMap<[u8; 32], Entry>,
    protected: BTreeSet<[u8; 32]>,
    clock: u64,
    downloading: bool,
    reserved: u64,
    external_reserved: u64,
}
#[derive(Clone)]
pub struct UpdateCache {
    root: PathBuf,
    state: Arc<Mutex<State>>,
    quota: u64,
}
fn name(hash: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(64);
    for b in hash {
        write!(out, "{b:02x}").expect("String write");
    }
    out
}
fn parse_name(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut hash = [0; 32];
    for (i, b) in hash.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(hash)
}
fn checked(path: &Path, directory: bool) -> Result<(), CacheError> {
    let m = fs::symlink_metadata(path)?;
    if m.file_type().is_symlink() || (directory && !m.is_dir()) || (!directory && !m.is_file()) {
        return Err(CacheError::UnsafePath);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(CacheError::UnsafePath);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if !directory && m.nlink() != 1 {
            return Err(CacheError::UnsafePath);
        }
    }
    Ok(())
}
fn open_file(path: &Path, write: bool, create: bool) -> Result<File, CacheError> {
    if !create {
        checked(path, false)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(write).create_new(create);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000).share_mode(1); // OPEN_REPARSE_POINT; share read only.
    }
    let file = options.open(path)?;
    checked(path, false)?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the synchronous call borrows this owned live file handle.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| CacheError::UnsafePath)?;
        if info.nNumberOfLinks != 1 || info.dwFileAttributes & (0x400 | 0x10) != 0 {
            return Err(CacheError::UnsafePath);
        }
    }
    Ok(file)
}
impl UpdateCache {
    /// Temporary envelopes and extracted installer sources share the cache's
    /// physical quota. Drop only after their file owners and files are gone.
    pub fn reserve_external(&self, bytes: u64) -> Result<ExternalReservation, CacheError> {
        let mut state = self.state.lock().map_err(|_| CacheError::Busy)?;
        self.make_room(&mut state, bytes, None)?;
        state.external_reserved = state
            .external_reserved
            .checked_add(bytes)
            .ok_or(CacheError::Full)?;
        Ok(ExternalReservation {
            cache: self.clone(),
            bytes,
        })
    }
    /// `protected` is the current and previous complete maintenance-source hash,
    /// read from authoritative local maintenance state before any reclamation.
    /// The directory must already exist with the application's private ACL.
    pub fn open(root: &Path, protected: &[[u8; 32]]) -> Result<Self, CacheError> {
        Self::open_with_quota(root, protected, CACHE_QUOTA_BYTES)
    }
    fn open_with_quota(
        root: &Path,
        protected: &[[u8; 32]],
        quota: u64,
    ) -> Result<Self, CacheError> {
        if !root.is_absolute() || protected.len() > 2 {
            return Err(CacheError::UnsafePath);
        }
        for ancestor in root.ancestors() {
            checked(ancestor, true)?;
        }
        let mut entries = BTreeMap::new();
        let mut clock = 0;
        let mut files = Vec::new();
        for (dir, ready) in [("ready", true), ("partial", false)] {
            let path = root.join(dir);
            match fs::create_dir(&path) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e.into()),
            }
            checked(&path, true)?;
            for file in fs::read_dir(&path)? {
                if files.len() >= MAX_ENTRIES {
                    return Err(CacheError::Full);
                }
                let file = file?;
                checked(&file.path(), false)?;
                let hash = file
                    .file_name()
                    .to_str()
                    .and_then(parse_name)
                    .ok_or(CacheError::UnsafePath)?;
                let metadata = file.metadata()?;
                if metadata.len() > MAX_PAYLOAD_BYTES {
                    return Err(CacheError::LengthMismatch);
                }
                files.push((metadata.modified()?, hash, metadata.len(), ready));
            }
        }
        files.sort_by_key(|f| f.0);
        for (_, hash, length, ready) in files {
            clock += 1;
            if entries
                .insert(
                    hash,
                    Entry {
                        ready,
                        length,
                        used: clock,
                        owners: 0,
                    },
                )
                .is_some()
            {
                return Err(CacheError::UnsafePath);
            }
        }
        if entries.len() > MAX_ENTRIES {
            return Err(CacheError::Full);
        }
        let cache = Self {
            root: root.to_owned(),
            quota,
            state: Arc::new(Mutex::new(State {
                entries,
                protected: protected.iter().copied().collect(),
                clock,
                downloading: false,
                reserved: 0,
                external_reserved: 0,
            })),
        };
        {
            let mut state = cache.state.lock().map_err(|_| CacheError::Busy)?;
            cache.make_room(&mut state, 0, None)?;
        }
        Ok(cache)
    }
    fn path(&self, hash: &[u8; 32], ready: bool) -> PathBuf {
        self.root
            .join(if ready { "ready" } else { "partial" })
            .join(name(hash))
    }
    /// Update this pair only after authoritative maintenance state commits.
    pub fn protect_sources(
        &self,
        current: Option<[u8; 32]>,
        previous: Option<[u8; 32]>,
    ) -> Result<(), CacheError> {
        let mut state = self.state.lock().map_err(|_| CacheError::Busy)?;
        state.protected = [current, previous].into_iter().flatten().collect();
        Ok(())
    }
    /// An invalid/incompatible prefix may be explicitly abandoned and retried.
    /// This never removes complete maintenance sources or live owners.
    pub fn discard_partial(&self, hash: [u8; 32]) -> Result<(), CacheError> {
        let mut state = self.state.lock().map_err(|_| CacheError::Busy)?;
        let entry = state.entries.get(&hash).ok_or(CacheError::NotReady)?;
        if entry.ready || entry.owners != 0 {
            return Err(CacheError::Busy);
        }
        let path = self.path(&hash, false);
        checked(&path, false)?;
        fs::remove_file(path)?;
        state.entries.remove(&hash);
        Ok(())
    }
    fn make_room(
        &self,
        state: &mut State,
        additional: u64,
        keep: Option<[u8; 32]>,
    ) -> Result<(), CacheError> {
        let mut total: u64 = state.entries.values().map(|e| e.length).sum();
        while total
            .saturating_add(state.reserved)
            .saturating_add(state.external_reserved)
            .saturating_add(additional)
            > self.quota
        {
            let candidate = state
                .entries
                .iter()
                .filter(|(h, e)| {
                    Some(**h) != keep && !state.protected.contains(*h) && e.owners == 0
                })
                .min_by_key(|(_, e)| e.used)
                .map(|(h, _)| *h)
                .ok_or(CacheError::Full)?;
            let entry = &state.entries[&candidate];
            let path = self.path(&candidate, entry.ready);
            checked(&path, false)?;
            fs::remove_file(path)?;
            total -= entry.length;
            state.entries.remove(&candidate);
        }
        Ok(())
    }
    /// Rehashes the persisted prefix in 64 KiB chunks before resuming. The
    /// caller must first revalidate metadata and bind HTTP Range/ETag to target.
    /// Non-cancellable convenience for explicitly synchronous local callers.
    /// Production background work must use `begin_cancellable`.
    pub fn begin(&self, target: VerifiedTarget) -> Result<Download, CacheError> {
        self.begin_cancellable(target, &|| false)
    }
    /// Worker cancellation is checked between prefix chunks, without returning
    /// the worker/physical file owner early.
    pub fn begin_cancellable(
        &self,
        target: VerifiedTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Download, CacheError> {
        let target = target.validate()?;
        if cancelled() {
            return Err(CacheError::Cancelled);
        }
        let mut state = self.state.lock().map_err(|_| CacheError::Busy)?;
        if state.downloading {
            return Err(CacheError::Busy);
        }
        let existing = state.entries.get(&target.sha256);
        if existing.is_some_and(|e| e.ready) {
            return Err(CacheError::Busy);
        }
        let offset = existing.map_or(0, |e| e.length);
        if offset > target.length {
            return Err(CacheError::LengthMismatch);
        }
        self.make_room(&mut state, target.length - offset, Some(target.sha256))?;
        if !state.entries.contains_key(&target.sha256) && state.entries.len() >= MAX_ENTRIES {
            return Err(CacheError::Full);
        }
        let mut file = open_file(
            &self.path(&target.sha256, false),
            true,
            !state.entries.contains_key(&target.sha256),
        )?;
        let mut hasher = Sha256::new();
        let read = hash_into(&mut file, &mut hasher, cancelled)?;
        if read != offset {
            return Err(CacheError::LengthMismatch);
        }
        state.clock += 1;
        let used = state.clock;
        state.entries.insert(
            target.sha256,
            Entry {
                ready: false,
                length: offset,
                used,
                owners: 1,
            },
        );
        state.downloading = true;
        state.reserved = target.length - offset;
        Ok(Download {
            cache: self.clone(),
            target,
            file: Some(file),
            hasher,
            offset,
            complete: false,
        })
    }
    /// Ready files are rehashed before handing out an owner. A damaged blob is
    /// never served; explicit recovery/re-download is needed for protected data.
    /// Non-cancellable convenience; background work uses `acquire_cancellable`.
    pub fn acquire(&self, target: VerifiedTarget) -> Result<BlobLease, CacheError> {
        self.acquire_cancellable(target, &|| false)
    }
    pub fn acquire_cancellable(
        &self,
        target: VerifiedTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<BlobLease, CacheError> {
        let target = target.validate()?;
        if cancelled() {
            return Err(CacheError::Cancelled);
        }
        let mut state = self.state.lock().map_err(|_| CacheError::Busy)?;
        let entry = state
            .entries
            .get(&target.sha256)
            .ok_or(CacheError::NotReady)?;
        if !entry.ready {
            return Err(CacheError::NotReady);
        }
        let mut file = open_file(&self.path(&target.sha256, true), false, false)?;
        let mut hash = Sha256::new();
        if hash_into(&mut file, &mut hash, cancelled)? != target.length {
            return Err(CacheError::LengthMismatch);
        }
        if <[u8; 32]>::from(hash.finalize()) != target.sha256 {
            return Err(CacheError::HashMismatch);
        }
        file.seek(SeekFrom::Start(0))?;
        state.clock += 1;
        let used = state.clock;
        let entry = state
            .entries
            .get_mut(&target.sha256)
            .expect("checked entry");
        entry.owners += 1;
        entry.used = used;
        Ok(BlobLease {
            cache: self.clone(),
            hash: target.sha256,
            file: Some(file),
        })
    }
}
fn hash_into(
    file: &mut File,
    hash: &mut Sha256,
    cancelled: &dyn Fn() -> bool,
) -> Result<u64, CacheError> {
    let mut buffer = [0; CHUNK_BYTES];
    let mut length = 0;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            return Ok(length);
        }
        length += n as u64;
        if length > MAX_PAYLOAD_BYTES {
            return Err(CacheError::LengthMismatch);
        }
        hash.update(&buffer[..n]);
        if cancelled() {
            return Err(CacheError::Cancelled);
        }
    }
}
pub trait PackageReader: Read + Seek {}
pub struct ExternalReservation {
    cache: UpdateCache,
    bytes: u64,
}
impl Drop for ExternalReservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.cache.state.lock() {
            state.external_reserved -= self.bytes;
        }
    }
}
impl<T: Read + Seek> PackageReader for T {}
pub struct Download {
    cache: UpdateCache,
    target: VerifiedTarget,
    file: Option<File>,
    hasher: Sha256,
    offset: u64,
    complete: bool,
}
impl Download {
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), CacheError> {
        if bytes.len() > CHUNK_BYTES || bytes.len() as u64 > self.target.length - self.offset {
            return Err(CacheError::LengthMismatch);
        }
        self.file
            .as_mut()
            .expect("live download")
            .write_all(bytes)?;
        self.hasher.update(bytes);
        self.offset += bytes.len() as u64;
        let mut state = self.cache.state.lock().map_err(|_| CacheError::Busy)?;
        state
            .entries
            .get_mut(&self.target.sha256)
            .expect("owned entry")
            .length = self.offset;
        state.reserved = self.target.length - self.offset;
        Ok(())
    }
    /// A source whitelist, ZIP bounds and formal source-seal inspection must
    /// succeed here. Hash equality alone never grants Ready status.
    pub fn finish(
        mut self,
        preflight: impl FnOnce(&mut dyn PackageReader) -> Result<(), CacheError>,
    ) -> Result<(), CacheError> {
        if self.offset != self.target.length {
            return Err(CacheError::LengthMismatch);
        }
        if <[u8; 32]>::from(self.hasher.clone().finalize()) != self.target.sha256 {
            return Err(CacheError::HashMismatch);
        }
        let file = self.file.as_mut().expect("live download");
        file.sync_all()?;
        file.seek(SeekFrom::Start(0))?;
        preflight(file)?;
        // Windows rename requires releasing the write handle. The cache's
        // sole instance owner still excludes another downloader/installer.
        drop(self.file.take());
        let mut state = self.cache.state.lock().map_err(|_| CacheError::Busy)?;
        let destination = self.cache.path(&self.target.sha256, true);
        if destination.try_exists()? {
            return Err(CacheError::UnsafePath);
        }
        fs::rename(self.cache.path(&self.target.sha256, false), destination)?;
        let entry = state
            .entries
            .get_mut(&self.target.sha256)
            .expect("owned entry");
        entry.ready = true;
        entry.owners = 0;
        state.downloading = false;
        state.reserved = 0;
        self.complete = true;
        Ok(())
    }
}
impl Drop for Download {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        // Keep only the bytes actually persisted; a failed write can have made
        // partial progress. Next begin rehashes everything rather than trusting
        // this in-memory hasher. Owners return only after the handle is closed.
        drop(self.file.take());
        if let Ok(mut state) = self.cache.state.lock() {
            if let Some(entry) = state.entries.get_mut(&self.target.sha256) {
                if let Ok(m) = fs::metadata(self.cache.path(&self.target.sha256, false)) {
                    entry.length = m.len();
                }
                entry.owners = 0;
            }
            state.downloading = false;
            state.reserved = 0;
        }
    }
}
pub struct BlobLease {
    cache: UpdateCache,
    hash: [u8; 32],
    file: Option<File>,
}
impl Read for BlobLease {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.file.as_mut().expect("live lease").read(bytes)
    }
}
impl Seek for BlobLease {
    fn seek(&mut self, at: SeekFrom) -> io::Result<u64> {
        self.file.as_mut().expect("live lease").seek(at)
    }
}
impl Drop for BlobLease {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Ok(mut state) = self.cache.state.lock() {
            if let Some(entry) = state.entries.get_mut(&self.hash) {
                entry.owners -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "fc-update-cache-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn target(bytes: &[u8]) -> VerifiedTarget {
        VerifiedTarget {
            sha256: Sha256::digest(bytes).into(),
            length: bytes.len() as u64,
        }
    }
    fn ready(cache: &UpdateCache, bytes: &[u8]) {
        let mut download = cache.begin(target(bytes)).unwrap();
        download.append(bytes).unwrap();
        download
            .finish(|reader| {
                let mut actual = Vec::new();
                reader.read_to_end(&mut actual)?;
                if actual == bytes {
                    Ok(())
                } else {
                    Err(CacheError::InvalidPackage)
                }
            })
            .unwrap();
    }
    #[test]
    fn interrupted_prefix_resumes_but_is_not_shareable_before_preflight() {
        let fixture = Fixture::new();
        let bytes = b"a complete signed payload";
        let t = target(bytes);
        {
            let cache = UpdateCache::open(&fixture.0, &[]).unwrap();
            let mut download = cache.begin(t).unwrap();
            download.append(&bytes[..5]).unwrap();
            assert!(matches!(cache.acquire(t), Err(CacheError::NotReady)));
        }
        let cache = UpdateCache::open(&fixture.0, &[]).unwrap();
        let mut download = cache.begin(t).unwrap();
        assert_eq!(download.offset(), 5);
        download.append(&bytes[5..]).unwrap();
        assert!(matches!(
            download.finish(|_| Err(CacheError::InvalidPackage)),
            Err(CacheError::InvalidPackage)
        ));
        assert!(matches!(cache.acquire(t), Err(CacheError::NotReady)));
        cache.begin(t).unwrap().finish(|_| Ok(())).unwrap();
        let mut actual = Vec::new();
        cache.acquire(t).unwrap().read_to_end(&mut actual).unwrap();
        assert_eq!(actual, bytes);
    }
    #[test]
    fn prefix_and_ready_tampering_are_rejected() {
        let fixture = Fixture::new();
        let cache = UpdateCache::open(&fixture.0, &[]).unwrap();
        let bytes = b"official bytes";
        let t = target(bytes);
        let mut download = cache.begin(t).unwrap();
        download.append(&bytes[..4]).unwrap();
        drop(download);
        fs::write(cache.path(&t.sha256, false), b"fake").unwrap();
        let mut download = cache.begin(t).unwrap();
        download.append(&bytes[4..]).unwrap();
        assert!(matches!(
            download.finish(|_| Ok(())),
            Err(CacheError::HashMismatch)
        ));
        cache.discard_partial(t.sha256).unwrap();
        ready(&cache, bytes);
        fs::write(cache.path(&t.sha256, true), b"modified bytes").unwrap();
        assert!(matches!(cache.acquire(t), Err(CacheError::HashMismatch)));
    }
    #[test]
    fn quota_never_evicts_protected_sources_or_live_readers() {
        let fixture = Fixture::new();
        let a = b"aaaa";
        let b = b"bbbb";
        let c = b"cccc";
        let cache = UpdateCache::open_with_quota(&fixture.0, &[target(a).sha256], 8).unwrap();
        ready(&cache, a);
        ready(&cache, b);
        let owner = cache.acquire(target(b)).unwrap();
        assert!(matches!(cache.begin(target(c)), Err(CacheError::Full)));
        drop(owner);
        ready(&cache, c);
        assert!(cache.acquire(target(a)).is_ok());
        assert!(matches!(
            cache.acquire(target(b)),
            Err(CacheError::NotReady)
        ));
        assert!(cache.acquire(target(c)).is_ok());
    }
    #[test]
    fn staging_reservation_shares_quota_and_returns_after_owner_drop() {
        let fixture = Fixture::new();
        let cache =
            UpdateCache::open_with_quota(&fixture.0, &[target(b"keep").sha256], 12).unwrap();
        ready(&cache, b"keep");
        let staging = cache.reserve_external(8).unwrap();
        assert!(matches!(
            cache.begin(target(b"next")),
            Err(CacheError::Full)
        ));
        assert!(cache.acquire(target(b"keep")).is_ok());
        drop(staging);
        ready(&cache, b"next");
        assert!(cache.acquire(target(b"keep")).is_ok());
        assert!(cache.acquire(target(b"next")).is_ok());
    }
    #[test]
    fn invalid_length_and_busy_download_do_not_publish() {
        let fixture = Fixture::new();
        let cache = UpdateCache::open(&fixture.0, &[]).unwrap();
        assert!(matches!(
            cache.begin(VerifiedTarget {
                sha256: [0; 32],
                length: MAX_PAYLOAD_BYTES + 1
            }),
            Err(CacheError::InvalidTarget)
        ));
        let mut download = cache.begin(target(b"test")).unwrap();
        assert!(matches!(
            cache.begin(target(b"next")),
            Err(CacheError::Busy)
        ));
        assert!(matches!(
            download.append(b"too long"),
            Err(CacheError::LengthMismatch)
        ));
        assert!(matches!(
            download.finish(|_| Ok(())),
            Err(CacheError::LengthMismatch)
        ));
    }
    #[test]
    fn cancellation_during_prefix_rehash_retains_partial_without_ready() {
        use std::cell::Cell;
        let fixture = Fixture::new();
        let cache = UpdateCache::open(&fixture.0, &[]).unwrap();
        let bytes = vec![7; CHUNK_BYTES * 2];
        let t = target(&bytes);
        let mut download = cache.begin(t).unwrap();
        download.append(&bytes[..CHUNK_BYTES]).unwrap();
        drop(download);
        let checked = Cell::new(false);
        assert!(matches!(
            cache.begin_cancellable(t, &|| checked.replace(true)),
            Err(CacheError::Cancelled)
        ));
        assert!(matches!(cache.acquire(t), Err(CacheError::NotReady)));
        let mut resumed = cache.begin_cancellable(t, &|| false).unwrap();
        assert_eq!(resumed.offset(), CHUNK_BYTES as u64);
        resumed.append(&bytes[CHUNK_BYTES..]).unwrap();
        resumed.finish(|_| Ok(())).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn cache_does_not_follow_external_symlink_or_hardlink() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let outside = Fixture::new();
        symlink(&outside.0, fixture.0.join("ready")).unwrap();
        assert!(matches!(
            UpdateCache::open(&fixture.0, &[]),
            Err(CacheError::UnsafePath)
        ));
        fs::remove_file(fixture.0.join("ready")).unwrap();
        let cache = UpdateCache::open(&fixture.0, &[]).unwrap();
        ready(&cache, b"test");
        fs::hard_link(
            cache.path(&target(b"test").sha256, true),
            outside.0.join("alias"),
        )
        .unwrap();
        assert!(matches!(
            cache.acquire(target(b"test")),
            Err(CacheError::UnsafePath)
        ));
    }
}
