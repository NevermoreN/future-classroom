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
/// Seek is relative to the bounded official archive inside the single EXE.
/// ZIP offsets never address the executable prefix or the outer trailer.
struct EnvelopeReader {
    file: File,
    start: u64,
    length: u64,
    position: u64,
}
impl Read for EnvelopeReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = bytes.len().min((self.length - self.position) as usize);
        let count = self.file.read(&mut bytes[..count])?;
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for EnvelopeReader {
    fn seek(&mut self, seek: SeekFrom) -> std::io::Result<u64> {
        let position = match seek {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::End(offset) => i128::from(self.length) + i128::from(offset),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
        };
        if position < 0 || position > i128::from(self.length) {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        self.position = position as u64;
        self.file
            .seek(SeekFrom::Start(self.start + self.position))?;
        Ok(self.position)
    }
}
fn open_envelope(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<EnvelopeReader> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX + (32 << 20) + 64 {
        return Err(PackageError::Invalid);
    }
    let mut head = [0; 4];
    file.read_exact(&mut head)?;
    if head == *b"PK\x03\x04" {
        file.seek(SeekFrom::Start(0))?;
        return Ok(EnvelopeReader {
            file,
            start: 0,
            length: size,
            position: 0,
        });
    }
    if &head[..2] != b"MZ" || size < 64 {
        return Err(PackageError::Invalid);
    }
    file.seek(SeekFrom::End(-64))?;
    let mut trailer = [0; 64];
    file.read_exact(&mut trailer)?;
    let prefix = u64::from_le_bytes(trailer[16..24].try_into().unwrap());
    let length = u64::from_le_bytes(trailer[24..32].try_into().unwrap());
    if &trailer[..16] != b"FC-SFX-PACK\0v4\0\0"
        || !(64..=32 << 20).contains(&prefix)
        || !(22..=MAX).contains(&length)
        || prefix.checked_add(length).and_then(|n| n.checked_add(64)) != Some(size)
    {
        return Err(PackageError::Invalid);
    }
    file.seek(SeekFrom::Start(prefix))?;
    let mut reader = EnvelopeReader {
        file,
        start: prefix,
        length,
        position: 0,
    };
    let mut hash = fc_security::Sha256Stream::default();
    let mut bytes = [0; 65536];
    loop {
        if cancelled() {
            return Err(CacheError::Cancelled.into());
        }
        let count = reader.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    if hash.finish() != trailer[32..64] {
        return Err(PackageError::Invalid);
    }
    reader.seek(SeekFrom::Start(0))?;
    Ok(reader)
}
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
/// Official signatures and source membership for the public installation entry.
/// `maintenance_version` is read from Windows registration, never UI/argv. Only
/// exactly that signed target may use historical signature time for maintenance;
/// the native same-source/cache admission still runs before any MSI operation.
pub fn verify_installer_distribution(
    path: &Path,
    anchor: &[u8],
    now: i64,
    maintenance_version: Option<&str>,
    selected_sources: Option<(&Path, &Path)>,
) -> Result<ReleaseClaims> {
    let reader = open_envelope(path, &|| false)?;
    if reader.start == 0 {
        return Err(PackageError::Invalid);
    }
    let prefix_length = reader.start;
    let mut zip = archive(reader, 36)?;
    let metadata = metadata_from_archive(&mut zip)?;
    let payload = read_member(&mut zip, "payload.zip", MAX as usize)?;
    preflight_payload(&mut std::io::Cursor::new(&payload))?;
    let roots: Vec<_> = metadata
        .roots
        .iter()
        .map(|(_, raw)| raw.as_slice())
        .collect();
    let verify = |time| {
        UpdateTrust::from_installed_root(anchor)
            .and_then(|mut trust| trust.refresh(metadata.borrowed(&roots), time))
    };
    let targets = match verify(now) {
        Ok(targets) => targets,
        Err(_) if maintenance_version.is_some() => {
            verify(historical_time(anchor, &metadata)?).map_err(|_| PackageError::Trust)?
        }
        Err(_) => return Err(PackageError::Trust),
    };
    let digest = fc_security::sha256(&payload);
    let mut matching = targets
        .iter()
        .filter(|t| t.sha256() == digest && t.length() == payload.len() as u64);
    let target = matching.next().ok_or(PackageError::Trust)?;
    if matching.next().is_some() {
        return Err(PackageError::Trust);
    }
    let claims = ReleaseClaims::from_verified_custom(
        &serde_json::to_vec(target.custom().ok_or(PackageError::Trust)?)
            .map_err(|_| PackageError::Trust)?,
        digest,
        payload.len() as u64,
    )
    .map_err(|_| PackageError::Trust)?;
    // Historical expiry is maintenance-only; never allow it to introduce a
    // different release into a fresh machine or an older registered product.
    if verify(now).is_err()
        && maintenance_version.and_then(|v| fc_domain::updates::ReleaseVersion::parse(v).ok())
            != Some(claims.version)
    {
        return Err(PackageError::Trust);
    }
    let mut sources = archive(std::io::Cursor::new(&payload), 6)?;
    let setup = read_member(&mut sources, "Setup.exe", 32 << 20)?;
    let msi = read_member(&mut sources, "FutureClassroom.msi", 64 << 20)?;
    let prefix = read_prefix(path, prefix_length as usize)?;
    if prefix != setup[..setup.len() - 132] {
        return Err(PackageError::Trust);
    }
    if let Some((selected_setup, selected_msi)) = selected_sources {
        let actual_setup = read_bound(selected_setup, 32 << 20)?;
        let actual_msi = read_bound(selected_msi, 64 << 20)?;
        let mut matched = actual_setup == setup && actual_msi == msi;
        if !matched && sources.file_names().any(|n| n == "installer-variants.bin") {
            let raw = read_member(
                &mut sources,
                "installer-variants.bin",
                fc_protocol::installer_variants::MAX_BYTES,
            )?;
            let variants = fc_protocol::installer_variants::Package::parse(&raw)
                .map_err(|_| PackageError::Invalid)?;
            for index in 0..variants.catalog.variants.len() {
                let pair = variants
                    .materialize(index, &setup, &msi)
                    .map_err(|_| PackageError::Invalid)?;
                if pair.0 == actual_setup && pair.1 == actual_msi {
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            return Err(PackageError::Trust);
        }
    }
    Ok(claims)
}
fn read_prefix(path: &Path, length: usize) -> Result<Vec<u8>> {
    let mut prefix = vec![0; length];
    File::open(path)?.read_exact(&mut prefix)?;
    Ok(prefix)
}
fn historical_time(anchor: &[u8], metadata: &OwnedMetadata) -> Result<i64> {
    let mut minimum = i64::MAX;
    for raw in std::iter::once(anchor)
        .chain(metadata.roots.iter().map(|(_, raw)| raw.as_slice()))
        .chain([
            metadata.timestamp.as_slice(),
            metadata.snapshot.as_slice(),
            metadata.targets.as_slice(),
        ])
    {
        let value: serde_json::Value =
            serde_json::from_slice(raw).map_err(|_| PackageError::Invalid)?;
        let expiry = value["signed"]["expires"]
            .as_str()
            .ok_or(PackageError::Invalid)?;
        if !expiry.is_ascii()
            || expiry.len() != 20
            || &expiry[4..5] != "-"
            || &expiry[7..8] != "-"
            || &expiry[10..11] != "T"
            || &expiry[13..14] != ":"
            || &expiry[16..17] != ":"
            || &expiry[19..] != "Z"
        {
            return Err(PackageError::Invalid);
        }
        let number = |a: usize, b: usize| {
            expiry[a..b]
                .parse::<u32>()
                .map_err(|_| PackageError::Invalid)
        };
        let date = fc_domain::time::LocalDate::new(
            number(0, 4)? as i32,
            number(5, 7)? as u8,
            number(8, 10)? as u8,
        )
        .map_err(|_| PackageError::Invalid)?;
        let hour = number(11, 13)?;
        let minute = number(14, 16)?;
        let second = number(17, 19)?;
        if hour > 23 || minute > 59 || second > 59 {
            return Err(PackageError::Invalid);
        }
        let epoch = fc_domain::time::LocalDate::new(1970, 1, 1).unwrap();
        minimum = minimum.min(
            date.days_since(epoch) * 86400 + i64::from(hour * 3600 + minute * 60 + second) - 1,
        );
    }
    Ok(minimum)
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
    let mut zip = archive(open_envelope(path, cancelled)?, 36)?;
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
/// Recreate the public single installer from the signed, hash-verified payload
/// and original metadata bytes. ZIP fields match package-update-envelope.py;
/// no peer executable, publisher key or unsigned executable prefix is accepted.
pub fn reconstruct_installer(
    reader: &mut dyn PackageReader,
    metadata: &OwnedMetadata,
    output: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    metadata.validate()?;
    preflight_payload_cancellable(reader, cancelled)?;
    reader.seek(SeekFrom::Start(0))?;
    let mut zip = archive(&mut *reader, 6)?;
    let setup = read_member(&mut zip, "Setup.exe", 32 << 20)?;
    let helper = &setup[..setup.len() - 132];
    drop(zip);
    reader.seek(SeekFrom::Start(0))?;
    let length = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut crc = flate2::Crc::new();
    let mut buffer = [0; 65536];
    loop {
        if cancelled() {
            return Err(CacheError::Cancelled.into());
        }
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        crc.update(&buffer[..n]);
    }
    reader.seek(SeekFrom::Start(0))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let result = (|| {
        file.write_all(helper)?;
        let mut writer = ArchiveWriter {
            file: &mut file,
            hash: fc_security::Sha256Stream::default(),
            count: 0,
        };
        let mut central = Vec::new();
        let mut count = 0u16;
        for (name, raw) in [
            ("snapshot.json", &metadata.snapshot),
            ("targets.json", &metadata.targets),
            ("timestamp.json", &metadata.timestamp),
        ] {
            write_archive_member(
                &mut writer,
                &mut central,
                &format!("metadata/{name}"),
                raw.len() as u64,
                checksum(raw),
                &mut std::io::Cursor::new(raw),
                cancelled,
            )?;
            count += 1;
        }
        for (version, raw) in &metadata.roots {
            write_archive_member(
                &mut writer,
                &mut central,
                &format!("metadata/{version}.root.json"),
                raw.len() as u64,
                checksum(raw),
                &mut std::io::Cursor::new(raw),
                cancelled,
            )?;
            count += 1;
        }
        write_archive_member(
            &mut writer,
            &mut central,
            "payload.zip",
            length,
            crc.sum(),
            reader,
            cancelled,
        )?;
        count += 1;
        let offset = writer.count as u32;
        writer.write_all(&central)?;
        writer.write_all(b"PK\x05\x06")?;
        for number in [0u16, 0, count, count] {
            writer.write_all(&number.to_le_bytes())?;
        }
        writer.write_all(&(central.len() as u32).to_le_bytes())?;
        writer.write_all(&offset.to_le_bytes())?;
        writer.write_all(&0u16.to_le_bytes())?;
        let archive_length = writer.count;
        let hash = writer.hash.finish();
        file.write_all(b"FC-SFX-PACK\0v4\0\0")?;
        file.write_all(&(helper.len() as u64).to_le_bytes())?;
        file.write_all(&archive_length.to_le_bytes())?;
        file.write_all(&hash)?;
        file.sync_all()?;
        Ok(())
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(output);
    }
    result
}
struct ArchiveWriter<'a, W: Write + ?Sized> {
    file: &'a mut W,
    hash: fc_security::Sha256Stream,
    count: u64,
}
impl<W: Write + ?Sized> Write for ArchiveWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let count = self.file.write(bytes)?;
        self.hash.update(&bytes[..count]);
        self.count += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    crc.sum()
}
fn write_archive_member(
    writer: &mut ArchiveWriter<'_, impl Write>,
    central: &mut Vec<u8>,
    name: &str,
    length: u64,
    crc: u32,
    source: &mut dyn Read,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    if length == 0 || length > MAX || writer.count + length > MAX {
        return Err(PackageError::Invalid);
    }
    let offset = writer.count as u32;
    writer.write_all(b"PK\x03\x04")?;
    for n in [20u16, 0, 0, 0, 33] {
        writer.write_all(&n.to_le_bytes())?;
    }
    for n in [crc, length as u32, length as u32] {
        writer.write_all(&n.to_le_bytes())?;
    }
    writer.write_all(&(name.len() as u16).to_le_bytes())?;
    writer.write_all(&0u16.to_le_bytes())?;
    writer.write_all(name.as_bytes())?;
    let mut remaining = length;
    let mut buffer = [0; 65536];
    while remaining > 0 {
        if cancelled() {
            return Err(CacheError::Cancelled.into());
        }
        let count = (remaining as usize).min(buffer.len());
        source.read_exact(&mut buffer[..count])?;
        writer.write_all(&buffer[..count])?;
        remaining -= count as u64;
    }
    central.extend_from_slice(b"PK\x01\x02");
    for n in [0x314u16, 20, 0, 0, 0, 33] {
        central.extend_from_slice(&n.to_le_bytes());
    }
    for n in [crc, length as u32, length as u32] {
        central.extend_from_slice(&n.to_le_bytes());
    }
    for n in [name.len() as u16, 0, 0, 0, 0] {
        central.extend_from_slice(&n.to_le_bytes());
    }
    central.extend_from_slice(&0x81800000u32.to_le_bytes());
    central.extend_from_slice(&offset.to_le_bytes());
    central.extend_from_slice(name.as_bytes());
    Ok(())
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
/// Derive the latest old coordinator's four-member representation from the
/// current base. Its independent signed target digest remains mandatory.
pub fn derive_legacy_payload(
    reader: &mut dyn PackageReader,
    target: VerifiedTarget,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut zip = archive(reader, 6)?;
    let mut output = Vec::new();
    let mut writer = ArchiveWriter {
        file: &mut output,
        hash: fc_security::Sha256Stream::default(),
        count: 0,
    };
    let mut central = Vec::new();
    for name in [
        "Setup.exe",
        "FutureClassroom.msi",
        "LICENSE",
        "THIRD-PARTY-NOTICES.txt",
    ] {
        let mut member = zip.by_name(name)?;
        let size = member.size();
        let crc = member.crc32();
        write_archive_member(
            &mut writer,
            &mut central,
            name,
            size,
            crc,
            &mut member,
            cancelled,
        )?;
    }
    let offset = writer.count as u32;
    writer.write_all(&central)?;
    writer.write_all(b"PK\x05\x06")?;
    for n in [0u16, 0, 4, 4] {
        writer.write_all(&n.to_le_bytes())?;
    }
    writer.write_all(&(central.len() as u32).to_le_bytes())?;
    writer.write_all(&offset.to_le_bytes())?;
    writer.write_all(&0u16.to_le_bytes())?;
    if writer.count != target.length || writer.hash.finish() != target.sha256 {
        return Err(PackageError::Trust);
    }
    Ok(output)
}
fn inspect_payload(reader: &mut dyn PackageReader, destination: Option<&Path>) -> Result<()> {
    let mut zip = archive(reader, 6)?;
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
            "installer-variants.bin" => fc_protocol::installer_variants::MAX_BYTES as u64,
            _ => return Err(PackageError::Invalid),
        };
        if file.size() > max {
            return Err(PackageError::Invalid);
        }
        total += file.size();
        if total > MAX {
            return Err(PackageError::Invalid);
        }
        let is_source = matches!(
            name.as_str(),
            "Setup.exe" | "FutureClassroom.msi" | "installer-variants.bin"
        );
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
    if zip
        .file_names()
        .any(|name| name == "installer-variants.bin")
    {
        let raw = read_member(
            &mut zip,
            "installer-variants.bin",
            fc_protocol::installer_variants::MAX_BYTES,
        )?;
        let variants = fc_protocol::installer_variants::Package::parse(&raw)
            .map_err(|_| PackageError::Invalid)?;
        let setup = read_member(&mut zip, "Setup.exe", 32 << 20)?;
        let msi = read_member(&mut zip, "FutureClassroom.msi", 64 << 20)?;
        variants
            .validate_base(&setup, &msi)
            .map_err(|_| PackageError::Invalid)?;
        for index in 0..variants.catalog.variants.len() {
            variants
                .materialize(index, &setup, &msi)
                .map_err(|_| PackageError::Invalid)?;
        }
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
    #[test]
    fn reconstructed_single_installer_preserves_python_archive_and_official_source() {
        let path = stage();
        let mut archive = super::archive(std::io::Cursor::new(ENVELOPE), 36).unwrap();
        let metadata = metadata_from_archive(&mut archive).unwrap();
        let payload = read_member(&mut archive, "payload.zip", MAX as usize).unwrap();
        let output = path.join("installer.exe");
        reconstruct_installer(
            &mut std::io::Cursor::new(&payload),
            &metadata,
            &output,
            &|| false,
        )
        .unwrap();
        let mut reader = open_envelope(&output, &|| false).unwrap();
        let mut original = Vec::new();
        reader.read_to_end(&mut original).unwrap();
        assert_eq!(original, ENVELOPE);
        let claims = verify_installer_distribution(&output, ROOT, NOW, None, None).unwrap();
        assert_eq!(claims.release_id, "TEST-ONLY-NOT-INSTALLABLE");
        // Historical signatures cannot authorize fresh or cross-version installs.
        let expired = 2_000_000_000;
        assert!(verify_installer_distribution(&output, ROOT, expired, None, None).is_err());
        assert!(
            verify_installer_distribution(&output, ROOT, expired, Some("99.0.0"), None).is_err()
        );
        assert!(verify_installer_distribution(&output, ROOT, expired, Some("0.2.0"), None).is_ok());
        let mut damaged = fs::read(&output).unwrap();
        damaged[2] ^= 1;
        fs::write(&output, &damaged).unwrap();
        assert!(verify_installer_distribution(&output, ROOT, NOW, None, None).is_err());
    }
}
