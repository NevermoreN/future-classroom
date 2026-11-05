//! Bounded official update transport. Trust anchors are installed, never imported.
//! Call on an existing worker, in a private directory owned by the sole instance.
use fc_protocol::update_release::ReleaseClaims;
use fc_security::updates::{UpdateMetadata, UpdateTrust, VerifiedUpdateTarget};
use fc_windows::update_cache::{CacheError, PackageReader, UpdateCache, VerifiedTarget};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use zip::{CompressionMethod, ZipArchive};
const MAX: u64 = 128 * 1024 * 1024;
const META: usize = 256 * 1024;
const TOTAL_META: usize = 2 * 1024 * 1024;
#[derive(Debug)]
pub enum PackageError {
    Io(std::io::Error),
    Invalid,
    Trust,
    Checkpoint,
    Cache(CacheError),
}
impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for PackageError {}
impl From<std::io::Error> for PackageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<zip::result::ZipError> for PackageError {
    fn from(_: zip::result::ZipError) -> Self {
        Self::Invalid
    }
}
impl From<CacheError> for PackageError {
    fn from(e: CacheError) -> Self {
        Self::Cache(e)
    }
}
type Result<T> = std::result::Result<T, PackageError>;
/// Protect the 32-byte checkpoint digest with current-user DPAPI. No school key
/// or publisher key may substitute for this local integrity binding.
pub trait CheckpointProtection {
    fn protect(&self, digest: &[u8; 32]) -> Result<Vec<u8>>;
    fn unprotect(&self, protected: &[u8]) -> Result<[u8; 32]>;
}
#[derive(Clone, Debug)]
pub struct OwnedMetadata {
    pub roots: Vec<(u64, Vec<u8>)>,
    pub timestamp: Vec<u8>,
    pub snapshot: Vec<u8>,
    pub targets: Vec<u8>,
}
impl OwnedMetadata {
    fn borrowed<'a>(&'a self, roots: &'a [&'a [u8]]) -> UpdateMetadata<'a> {
        UpdateMetadata {
            roots,
            timestamp: &self.timestamp,
            snapshot: &self.snapshot,
            targets: &self.targets,
        }
    }
    fn validate(&self) -> Result<()> {
        if self.roots.len() > 32 || self.roots.windows(2).any(|w| w[1].0 != w[0].0 + 1) {
            return Err(PackageError::Invalid);
        }
        let mut total = 0;
        for bytes in self.roots.iter().map(|(_, b)| b).chain([
            &self.timestamp,
            &self.snapshot,
            &self.targets,
        ]) {
            if bytes.is_empty() || bytes.len() > META {
                return Err(PackageError::Invalid);
            }
            total += bytes.len();
        }
        if total > TOTAL_META {
            return Err(PackageError::Invalid);
        }
        Ok(())
    }
}
pub struct PreparedUpdate {
    pub claims: ReleaseClaims,
    pub target: VerifiedTarget,
    pub target_path: String,
    pub metadata: OwnedMetadata,
}
pub struct TrustedCatalog {
    pub targets: Vec<VerifiedUpdateTarget>,
    pub metadata: OwnedMetadata,
    pub accepted_time: i64,
}
fn json_version(raw: &[u8]) -> Result<u64> {
    serde_json::from_slice::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| v["signed"]["version"].as_u64())
        .filter(|v| *v > 0)
        .ok_or(PackageError::Invalid)
}
fn bytes_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2)
        || s.len() > 2 * TOTAL_META
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(PackageError::Checkpoint);
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| PackageError::Checkpoint))
        .collect()
}
fn checkpoint_bytes(m: &OwnedMetadata, time: i64) -> Result<Vec<u8>> {
    serde_json::to_vec(&serde_json::json!({"time":time,"roots":m.roots.iter().map(|(v,b)|serde_json::json!([v,bytes_hex(b)])).collect::<Vec<_>>(),"timestamp":bytes_hex(&m.timestamp),"snapshot":bytes_hex(&m.snapshot),"targets":bytes_hex(&m.targets)})).map_err(|_|PackageError::Checkpoint)
}
fn decode_checkpoint(raw: &[u8]) -> Result<(OwnedMetadata, i64)> {
    let v: serde_json::Value = serde_json::from_slice(raw).map_err(|_| PackageError::Checkpoint)?;
    let get = |name: &str| unhex(v[name].as_str().ok_or(PackageError::Checkpoint)?);
    let roots = v["roots"]
        .as_array()
        .ok_or(PackageError::Checkpoint)?
        .iter()
        .map(|r| {
            Ok((
                r[0].as_u64().ok_or(PackageError::Checkpoint)?,
                unhex(r[1].as_str().ok_or(PackageError::Checkpoint)?)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let m = OwnedMetadata {
        roots,
        timestamp: get("timestamp")?,
        snapshot: get("snapshot")?,
        targets: get("targets")?,
    };
    m.validate()?;
    Ok((m, v["time"].as_i64().ok_or(PackageError::Checkpoint)?))
}
fn read_bound(path: &Path, max: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > max as u64 {
        return Err(PackageError::Invalid);
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(PackageError::Invalid);
    }
    Ok(bytes)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
impl TrustedCatalog {
    /// Recover the last locally integrity-bound catalog after a normal restart.
    /// The caller still refreshes signatures and expiry against the current time.
    pub fn restore_metadata(
        checkpoint_dir: &Path,
        protection: &dyn CheckpointProtection,
    ) -> Result<Option<OwnedMetadata>> {
        let mut generations = Vec::new();
        for entry in fs::read_dir(checkpoint_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(number) = name
                .strip_prefix("checkpoint-")
                .filter(|v| v.len() == 16)
                .and_then(|v| u64::from_str_radix(v, 16).ok())
            {
                generations.push((number, entry.path()));
            }
            if generations.len() > 64 {
                return Err(PackageError::Checkpoint);
            }
        }
        generations.sort_by_key(|(n, _)| *n);
        let Some((_, path)) = generations.last() else {
            return Ok(None);
        };
        let raw = read_bound(&path.join("metadata.json"), 2 * TOTAL_META + 4096)?;
        let pin = read_bound(&path.join("integrity.dpapi"), 8192)?;
        if protection.unprotect(&pin)? != fc_security::sha256(&raw) {
            return Err(PackageError::Checkpoint);
        }
        let (metadata, _) = decode_checkpoint(&raw)?;
        Ok(Some(metadata))
    }
    /// `checkpoint_dir` has a current-user-only ACL and an exclusive product owner.
    /// Final checkpoint directories are immutable and monotonically numbered;
    /// incomplete staging directories cannot lower the selected checkpoint.
    pub fn refresh(
        anchor: &[u8],
        checkpoint_dir: &Path,
        incoming: OwnedMetadata,
        now: i64,
        protection: &dyn CheckpointProtection,
    ) -> Result<Self> {
        incoming.validate()?;
        fs::create_dir_all(checkpoint_dir)?;
        let mut generations = Vec::new();
        for entry in fs::read_dir(checkpoint_dir)? {
            let e = entry?;
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(number) = name
                .strip_prefix("checkpoint-")
                .filter(|s| s.len() == 16)
                .and_then(|s| u64::from_str_radix(s, 16).ok())
            {
                generations.push((number, e.path()));
            }
            if generations.len() > 64 {
                return Err(PackageError::Checkpoint);
            }
        }
        generations.sort_by_key(|(n, _)| *n);
        let (mut trust, mut chain) = if let Some((_, path)) = generations.last() {
            let raw = read_bound(&path.join("metadata.json"), 2 * TOTAL_META + 4096)?;
            let pin = read_bound(&path.join("integrity.dpapi"), 8192)?;
            if protection.unprotect(&pin)? != fc_security::sha256(&raw) {
                return Err(PackageError::Checkpoint);
            }
            let (prior, time) = decode_checkpoint(&raw)?;
            let roots = prior
                .roots
                .iter()
                .map(|(_, b)| b.as_slice())
                .collect::<Vec<_>>();
            (
                UpdateTrust::restore_from_checkpoint(anchor, prior.borrowed(&roots), time)
                    .map_err(|_| PackageError::Trust)?,
                prior.roots,
            )
        } else {
            (
                UpdateTrust::from_installed_root(anchor).map_err(|_| PackageError::Trust)?,
                Vec::new(),
            )
        };
        let current_version = json_version(trust.root_bytes())?;
        let fresh = incoming
            .roots
            .iter()
            .filter(|(v, _)| *v > current_version)
            .collect::<Vec<_>>();
        let root_refs = fresh.iter().map(|(_, b)| b.as_slice()).collect::<Vec<_>>();
        let targets = trust
            .refresh(incoming.borrowed(&root_refs), now)
            .map_err(|_| PackageError::Trust)?;
        chain.extend(fresh.into_iter().map(|(v, b)| (*v, b.clone())));
        let metadata = OwnedMetadata {
            roots: chain,
            ..incoming
        };
        metadata.validate()?;
        let raw = checkpoint_bytes(&metadata, now)?;
        let pin = protection.protect(&fc_security::sha256(&raw))?;
        let number = generations.last().map_or(Ok(1), |(n, _)| {
            n.checked_add(1).ok_or(PackageError::Checkpoint)
        })?;
        let stage = checkpoint_dir.join(format!("pending-{number:016x}"));
        if stage.exists() {
            fs::remove_dir_all(&stage)?;
        }
        fs::create_dir(&stage)?;
        let saved: Result<()> = (|| {
            write_new(&stage.join("metadata.json"), &raw)?;
            write_new(&stage.join("integrity.dpapi"), &pin)?;
            fs::rename(
                &stage,
                checkpoint_dir.join(format!("checkpoint-{number:016x}")),
            )?;
            Ok(())
        })();
        if saved.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        saved?;
        // Retain previous complete checkpoint until the successor is durable.
        for (_, old) in generations.iter().take(generations.len().saturating_sub(1)) {
            fs::remove_dir_all(old)?;
        }
        Ok(Self {
            targets,
            metadata,
            accepted_time: now,
        })
    }
}
/// Inspect the small ZIP end record before the library allocates directory data.
fn archive<R: Read + Seek>(mut source: R, max_entries: u16) -> Result<ZipArchive<R>> {
    let length = source.seek(SeekFrom::End(0))?;
    if !(22..=MAX).contains(&length) {
        return Err(PackageError::Invalid);
    }
    source.seek(SeekFrom::End(-22))?;
    let mut end = [0; 22];
    source.read_exact(&mut end)?;
    let u16at = |n| u16::from_le_bytes(end[n..n + 2].try_into().unwrap());
    let u32at = |n| u32::from_le_bytes(end[n..n + 4].try_into().unwrap());
    if &end[..4] != b"PK\x05\x06"
        || u16at(4) != 0
        || u16at(6) != 0
        || u16at(8) != u16at(10)
        || u16at(10) == 0
        || u16at(10) > max_entries
        || u16at(20) != 0
        || u32at(12) > 65536
        || u64::from(u32at(16)) + u64::from(u32at(12)) != length - 22
    {
        return Err(PackageError::Invalid);
    }
    source.seek(SeekFrom::Start(0))?;
    let mut zip = ZipArchive::new(source)?;
    if zip.len() != usize::from(u16at(10)) {
        return Err(PackageError::Invalid);
    }
    let mut names = BTreeSet::new();
    let mut previous = 0;
    for i in 0..zip.len() {
        let f = zip.by_index(i)?;
        if f.header_start() != previous
            || f.encrypted()
            || !names.insert(f.name().to_owned())
            || f.is_dir()
            || f.unix_mode()
                .is_some_and(|m| m & 0o170000 != 0 && m & 0o170000 != 0o100000)
            || f.extra_data().is_some_and(|b| !b.is_empty())
            || !f.comment().is_empty()
            || f.size() == 0
            || f.size() > MAX
            || f.compressed_size() > MAX
            || !matches!(
                f.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
        {
            return Err(PackageError::Invalid);
        }
        previous = f
            .data_start()
            .checked_add(f.compressed_size())
            .ok_or(PackageError::Invalid)?;
    }
    if zip.offset() != 0 || previous != zip.central_directory_start() || !zip.comment().is_empty() {
        return Err(PackageError::Invalid);
    }
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        let f = zip.by_index(i)?;
        if f.name_raw() != f.name().as_bytes() || !f.name().is_ascii() {
            return Err(PackageError::Invalid);
        }
        entries.push((
            f.header_start(),
            f.name().as_bytes().to_vec(),
            f.crc32(),
            f.compressed_size(),
            f.size(),
            f.compression(),
        ));
    }
    let mut source = zip.into_inner();
    for (start, name, crc, compressed, size, method) in entries {
        source.seek(SeekFrom::Start(start))?;
        let mut h = [0; 30];
        source.read_exact(&mut h)?;
        let u16at = |n| u16::from_le_bytes(h[n..n + 2].try_into().unwrap());
        let u32at = |n| u32::from_le_bytes(h[n..n + 4].try_into().unwrap());
        let compression = if method == CompressionMethod::Stored {
            0
        } else {
            8
        };
        if &h[..4] != b"PK\x03\x04"
            || u16at(6) & !0x800 != 0
            || u16at(8) != compression
            || u32at(14) != crc
            || u64::from(u32at(18)) != compressed
            || u64::from(u32at(22)) != size
            || usize::from(u16at(26)) != name.len()
            || u16at(28) != 0
        {
            return Err(PackageError::Invalid);
        }
        let mut local_name = vec![0; name.len()];
        source.read_exact(&mut local_name)?;
        if local_name != name {
            return Err(PackageError::Invalid);
        }
    }
    source.seek(SeekFrom::Start(0))?;
    Ok(ZipArchive::new(source)?)
}
fn read_member<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str, max: usize) -> Result<Vec<u8>> {
    let mut member = zip.by_name(name)?;
    if member.size() > max as u64 {
        return Err(PackageError::Invalid);
    }
    let mut out = Vec::new();
    member.by_ref().take(max as u64 + 1).read_to_end(&mut out)?;
    if out.len() > max || out.len() as u64 != member.size() {
        return Err(PackageError::Invalid);
    }
    Ok(out)
}
fn metadata_from_archive<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<OwnedMetadata> {
    let mut roots = Vec::new();
    let mut metadata_total = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index(i)?;
        let n = f.name();
        if n != "payload.zip" {
            if f.size() > META as u64 {
                return Err(PackageError::Invalid);
            }
            metadata_total += f.size();
            if metadata_total > TOTAL_META as u64 {
                return Err(PackageError::Invalid);
            }
        }
        if f.compression() != CompressionMethod::Stored {
            return Err(PackageError::Invalid);
        }
        if matches!(
            n,
            "payload.zip"
                | "metadata/timestamp.json"
                | "metadata/snapshot.json"
                | "metadata/targets.json"
        ) {
            continue;
        }
        let v = n
            .strip_prefix("metadata/")
            .and_then(|n| n.strip_suffix(".root.json"))
            .and_then(|n| n.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .ok_or(PackageError::Invalid)?;
        if n != format!("metadata/{v}.root.json") {
            return Err(PackageError::Invalid);
        }
        roots.push((v, n.to_owned()));
    }
    roots.sort_by_key(|(v, _)| *v);
    let m = OwnedMetadata {
        roots: roots
            .into_iter()
            .map(|(v, n)| Ok((v, read_member(zip, &n, META)?)))
            .collect::<Result<_>>()?,
        timestamp: read_member(zip, "metadata/timestamp.json", META)?,
        snapshot: read_member(zip, "metadata/snapshot.json", META)?,
        targets: read_member(zip, "metadata/targets.json", META)?,
    };
    m.validate()?;
    Ok(m)
}
/// Copies into cache at its resumed offset. Only freshly TUF-verified targets
/// with valid product claims and complete formal source binding become ready.
pub fn import_envelope(
    path: &Path,
    anchor: &[u8],
    checkpoint_dir: &Path,
    now: i64,
    protection: &dyn CheckpointProtection,
    cache: &UpdateCache,
) -> Result<PreparedUpdate> {
    import_envelope_cancellable(
        path,
        anchor,
        checkpoint_dir,
        now,
        protection,
        cache,
        &|| false,
    )
}
/// Background orchestration supplies its existing worker cancellation token.
pub fn import_envelope_cancellable(
    path: &Path,
    anchor: &[u8],
    checkpoint_dir: &Path,
    now: i64,
    protection: &dyn CheckpointProtection,
    cache: &UpdateCache,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedUpdate> {
    if cancelled() {
        return Err(CacheError::Cancelled.into());
    }
    let mut zip = archive(File::open(path)?, 36)?;
    let metadata = metadata_from_archive(&mut zip)?;
    let catalog = TrustedCatalog::refresh(anchor, checkpoint_dir, metadata, now, protection)?;
    let length = zip.by_name("payload.zip")?.size();
    // Always hash the entire imported payload, even if a same-size blob exists.
    let mut member = zip.by_name("payload.zip")?;
    let mut h = fc_security::Sha256Stream::default();
    let mut buf = [0; 65536];
    loop {
        if cancelled() {
            return Err(CacheError::Cancelled.into());
        }
        let n = member.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let payload_sha256 = h.finish();
    drop(member);
    let mut candidates = Vec::new();
    for target in &catalog.targets {
        if target.length() != length || target.sha256() != payload_sha256 {
            continue;
        }
        let custom = serde_json::to_vec(target.custom().ok_or(PackageError::Invalid)?)
            .map_err(|_| PackageError::Invalid)?;
        let claims = ReleaseClaims::from_verified_custom(&custom, target.sha256(), length)
            .map_err(|_| PackageError::Invalid)?;
        candidates.push((target, claims));
    }
    if candidates.len() != 1 {
        return Err(PackageError::Invalid);
    }
    let (t, claims) = candidates.remove(0);
    let target = VerifiedTarget {
        sha256: t.sha256(),
        length,
    };
    match cache.acquire_cancellable(target, cancelled) {
        Ok(_) => {}
        Err(CacheError::NotReady) => {
            let mut download = cache.begin_cancellable(target, cancelled)?;
            let offset = download.offset();
            let mut member = zip.by_name("payload.zip")?;
            let mut skipped = 0;
            loop {
                if cancelled() {
                    return Err(CacheError::Cancelled.into());
                }
                let n = member.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                let start = offset.saturating_sub(skipped).min(n as u64) as usize;
                if start < n {
                    download.append(&buf[start..n])?;
                }
                skipped += n as u64;
            }
            download.finish(|reader| preflight_payload_cancellable(reader, cancelled))?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(PreparedUpdate {
        claims,
        target,
        target_path: t.path().into(),
        metadata: catalog.metadata,
    })
}
/// Full member streaming checks CRC and size. Only the fixed carrier trailer is
/// buffered; GNU/unsealed showcase executables are rejected by construction.
pub fn preflight_payload(reader: &mut dyn PackageReader) -> std::result::Result<(), CacheError> {
    preflight_payload_cancellable(reader, &|| false)
}
struct CancelReader<'a> {
    inner: &'a mut dyn PackageReader,
    cancelled: &'a dyn Fn() -> bool,
}
impl Read for CancelReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if (self.cancelled)() {
            return Err(std::io::Error::other("update cancelled"));
        }
        self.inner.read(bytes)
    }
}
impl Seek for CancelReader<'_> {
    fn seek(&mut self, at: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(at)
    }
}
pub fn preflight_payload_cancellable(
    reader: &mut dyn PackageReader,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<(), CacheError> {
    let result = inspect_payload(
        &mut CancelReader {
            inner: reader,
            cancelled,
        },
        None,
    );
    if cancelled() {
        return Err(CacheError::Cancelled);
    }
    result.map_err(|_| CacheError::InvalidPackage)
}
pub struct SourceFiles {
    pub setup: PathBuf,
    pub msi: PathBuf,
}
pub fn extract_install_sources(
    reader: &mut dyn PackageReader,
    directory: &Path,
) -> Result<SourceFiles> {
    extract_install_sources_cancellable(reader, directory, &|| false)
}
pub fn extract_install_sources_cancellable(
    reader: &mut dyn PackageReader,
    directory: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<SourceFiles> {
    let mut reader = CancelReader {
        inner: reader,
        cancelled,
    };
    inspect_payload(&mut reader, None)?;
    reader.seek(SeekFrom::Start(0))?;
    inspect_payload(&mut reader, Some(directory))?;
    Ok(SourceFiles {
        setup: directory.join("Setup.exe"),
        msi: directory.join("FutureClassroom.msi"),
    })
}
fn inspect_payload(reader: &mut dyn PackageReader, destination: Option<&Path>) -> Result<()> {
    let mut zip = archive(reader, 5)?;
    let mut setup_tail = Vec::new();
    let mut msi_hash = None;
    let mut required = BTreeSet::new();
    let mut total = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let name = file.name().to_owned();
        let max = match name.as_str() {
            "Setup.exe" | "FutureClassroom.msi" => 64 * 1024 * 1024,
            "LICENSE" | "THIRD-PARTY-NOTICES.txt" => 30 * 1024 * 1024,
            "manifest.json" => 32768,
            _ => return Err(PackageError::Invalid),
        };
        if file.size() > max {
            return Err(PackageError::Invalid);
        }
        total += file.size();
        if total > MAX {
            return Err(PackageError::Invalid);
        }
        let is_source = matches!(name.as_str(), "Setup.exe" | "FutureClassroom.msi");
        let mut output = if is_source {
            if let Some(dir) = destination {
                Some(
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(dir.join(&name))?,
                )
            } else {
                None
            }
        } else {
            None
        };
        let mut hash = fc_security::Sha256Stream::default();
        let mut actual = 0;
        let mut buffer = [0; 65536];
        let mut head = Vec::new();
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            actual += n as u64;
            if actual > max || actual > file.size() {
                return Err(PackageError::Invalid);
            }
            hash.update(&buffer[..n]);
            if let Some(out) = &mut output {
                out.write_all(&buffer[..n])?;
            }
            if head.len() < 64 {
                head.extend_from_slice(&buffer[..n.min(64 - head.len())]);
            }
            if name == "Setup.exe" {
                if n >= 132 {
                    setup_tail.clear();
                    setup_tail.extend_from_slice(&buffer[n - 132..n]);
                } else {
                    setup_tail.extend_from_slice(&buffer[..n]);
                    if setup_tail.len() > 132 {
                        setup_tail.drain(..setup_tail.len() - 132);
                    }
                }
            }
        }
        if actual != file.size() {
            return Err(PackageError::Invalid);
        }
        if let Some(out) = output {
            out.sync_all()?;
        }
        if name == "FutureClassroom.msi" {
            if !head.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
                return Err(PackageError::Invalid);
            }
            msi_hash = Some(hash.finish());
            required.insert(name);
        } else if name == "Setup.exe" {
            if !head.starts_with(b"MZ") || actual < 196 {
                return Err(PackageError::Invalid);
            }
            required.insert(name);
        }
    }
    if required.len() != 2 || setup_tail.len() != 132 {
        return Err(PackageError::Invalid);
    }
    let t = &setup_tail;
    let mut descriptor = Vec::new();
    descriptor.extend_from_slice(b"FCCAR2\0\0");
    descriptor.extend_from_slice(&2u16.to_le_bytes());
    descriptor.extend_from_slice(&64u16.to_le_bytes());
    descriptor.extend_from_slice(b"99378807-098D-5F04-8D4D-F181DFA218D5");
    for n in [120u16, 64, 8, 72] {
        descriptor.extend_from_slice(&n.to_le_bytes());
    }
    descriptor.extend_from_slice(&(30u32 * 1024 * 1024).to_le_bytes());
    descriptor.extend_from_slice(&3u32.to_le_bytes());
    if &t[..16] != b"FC-SETUP-MSI\0v1\0"
        || t[16..48] != msi_hash.ok_or(PackageError::Invalid)?
        || t[48..112] != descriptor
        || t[112..116] != 64u32.to_le_bytes()
        || &t[116..] != b"FCCR2-END-v2\0\0\0\0"
    {
        return Err(PackageError::Invalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestIntegrity;
    impl CheckpointProtection for TestIntegrity {
        fn protect(&self, d: &[u8; 32]) -> Result<Vec<u8>> {
            Ok(d.to_vec())
        }
        fn unprotect(&self, b: &[u8]) -> Result<[u8; 32]> {
            b.try_into().map_err(|_| PackageError::Checkpoint)
        }
    }
    const ROOT: &[u8] = include_bytes!("../tests/fixtures/updates/test-root.json");
    const ENVELOPE: &[u8] = include_bytes!("../tests/fixtures/updates/test.fcupdate");
    const NOW: i64 = 1793923200;
    struct Stage(PathBuf);
    impl std::ops::Deref for Stage {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Stage {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn stage() -> Stage {
        let p = std::env::temp_dir().join(format!(
            "fc-update-packages-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        Stage(p)
    }
    #[test]
    fn python_tuf_fixture_imports_replays_and_detects_checkpoint_tampering() {
        let path = stage();
        {
            let source = path.join("test.fcupdate");
            fs::write(&source, ENVELOPE).unwrap();
            fs::create_dir(path.join("cache")).unwrap();
            let cache = UpdateCache::open(&path.join("cache"), &[]).unwrap();
            let cp = path.join("trust");
            let prepared =
                import_envelope(&source, ROOT, &cp, NOW, &TestIntegrity, &cache).unwrap();
            assert_eq!(prepared.claims.release_id, "TEST-ONLY-NOT-INSTALLABLE");
            assert_eq!(
                TrustedCatalog::restore_metadata(&cp, &TestIntegrity)
                    .unwrap()
                    .unwrap()
                    .targets,
                prepared.metadata.targets
            );
            let mut blob = cache.acquire(prepared.target).unwrap();
            preflight_payload(&mut blob).unwrap();
            drop(blob);
            import_envelope(&source, ROOT, &cp, NOW + 1, &TestIntegrity, &cache).unwrap();
            assert!(matches!(
                import_envelope(&source, ROOT, &cp, NOW, &TestIntegrity, &cache),
                Err(PackageError::Trust)
            ));
            let latest = cp.join("checkpoint-0000000000000002").join("metadata.json");
            let mut bytes = fs::read(&latest).unwrap();
            bytes[5] ^= 1;
            fs::write(latest, bytes).unwrap();
            assert!(matches!(
                import_envelope(&source, ROOT, &cp, NOW + 2, &TestIntegrity, &cache),
                Err(PackageError::Checkpoint)
            ));
        }
        fs::remove_dir_all(&path.0).unwrap();
    }
    #[test]
    fn payload_binding_and_transport_tampering_never_become_ready() {
        let mut zip = archive(std::io::Cursor::new(ENVELOPE), 36).unwrap();
        let payload = read_member(&mut zip, "payload.zip", MAX as usize).unwrap();
        let mut archive = ZipArchive::new(std::io::Cursor::new(&payload)).unwrap();
        let mut setup = read_member(&mut archive, "Setup.exe", 64 * 1024 * 1024).unwrap();
        let msi = read_member(&mut archive, "FutureClassroom.msi", 64 * 1024 * 1024).unwrap();
        setup[80] ^= 1;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options =
            zip::write::SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file("Setup.exe", options).unwrap();
        writer.write_all(&setup).unwrap();
        writer.start_file("FutureClassroom.msi", options).unwrap();
        writer.write_all(&msi).unwrap();
        let changed = writer.finish().unwrap().into_inner();
        assert!(preflight_payload(&mut std::io::Cursor::new(changed)).is_err());
        assert!(matches!(
            preflight_payload_cancellable(&mut std::io::Cursor::new(&payload), &|| true),
            Err(CacheError::Cancelled)
        ));
        let mut trailing = ENVELOPE.to_vec();
        trailing.push(0);
        assert!(super::archive(std::io::Cursor::new(trailing), 36).is_err());
        // Local names disagreeing with the central directory are rejected.
        let mut ambiguous = ENVELOPE.to_vec();
        ambiguous[30] ^= 1;
        assert!(super::archive(std::io::Cursor::new(ambiguous), 36).is_err());
    }
}
