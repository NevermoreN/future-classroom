//! Streaming OOXML scanner. ZIP metadata is parsed with fixed buffers BEFORE any
//! decompressor/XML parser is created, so forged entry counts cannot allocate a
//! library-sized central directory. Nothing is extracted or executed.
use crc32fast::Hasher;
use flate2::bufread::DeflateDecoder;
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::BTreeSet,
    fmt,
    io::{self, BufReader, Read, Seek, SeekFrom},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

mod xml;
static ACTIVE: AtomicBool = AtomicBool::new(false);
const BLOCK: usize = 32 * 1024;
const MAX_METADATA: usize = 4 * 1024 * 1024;
const MAX_ENTRY: u64 = 32 * 1024 * 1024;
const MAX_EXPANDED: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeKind {
    Docx,
    Pptx,
    Xlsx,
}
impl OfficeKind {
    fn root(self) -> &'static str {
        match self {
            Self::Docx => "word/",
            Self::Pptx => "ppt/",
            Self::Xlsx => "xl/",
        }
    }
    fn main(self) -> &'static str {
        match self {
            Self::Docx => "word/document.xml",
            Self::Pptx => "ppt/presentation.xml",
            Self::Xlsx => "xl/workbook.xml",
        }
    }
    fn content_type(self) -> &'static str {
        match self {
            Self::Docx => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
            }
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"
            }
            Self::Xlsx => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerError {
    Busy,
    Io,
    InvalidArchive,
    UnsupportedStructure,
    UnsafePath,
    DuplicatePath,
    TooLarge,
    InvalidXml,
    ForbiddenContent,
    Canceled,
    Deadline,
    CpuBudget,
    ClockUnavailable,
}
impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ContainerError {}
impl From<io::Error> for ContainerError {
    fn from(e: io::Error) -> Self {
        e.get_ref()
            .and_then(|e| e.downcast_ref::<Self>())
            .copied()
            .unwrap_or(Self::Io)
    }
}
fn io_error(e: ContainerError) -> io::Error {
    io::Error::other(e)
}

/// Must be supplied by the execution domain: CPU is actual current-thread CPU,
/// not process-wide CPU or a guessed number. The scanner adds its own wall deadline.
pub trait ValidationControl {
    fn canceled(&self) -> bool;
    fn thread_cpu_time(&self) -> Result<Duration, ContainerError>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerReport {
    pub kind: OfficeKind,
    pub compressed_bytes: u64,
    pub expanded_bytes: u64,
    pub entries: u32,
    pub xml_events: u64,
    pub sha256: [u8; 32],
}
struct Exclusive;
impl Exclusive {
    fn acquire() -> Result<Self, ContainerError> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| ContainerError::Busy)
    }
}
impl Drop for Exclusive {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
struct Budget<'a> {
    control: &'a dyn ValidationControl,
    start: Instant,
    cpu_start: Duration,
    expanded: Cell<u64>,
    xml_events: Cell<u64>,
}
impl<'a> Budget<'a> {
    fn new(control: &'a dyn ValidationControl) -> Result<Self, ContainerError> {
        Ok(Self {
            control,
            start: Instant::now(),
            cpu_start: control.thread_cpu_time()?,
            expanded: Cell::new(0),
            xml_events: Cell::new(0),
        })
    }
    fn check(&self) -> Result<(), ContainerError> {
        if self.control.canceled() {
            return Err(ContainerError::Canceled);
        }
        if self.start.elapsed() >= Duration::from_secs(30) {
            return Err(ContainerError::Deadline);
        }
        let cpu = self
            .control
            .thread_cpu_time()?
            .checked_sub(self.cpu_start)
            .ok_or(ContainerError::ClockUnavailable)?;
        if cpu >= Duration::from_secs(10) {
            return Err(ContainerError::CpuBudget);
        }
        Ok(())
    }
    fn output(&self, n: usize) -> Result<(), ContainerError> {
        self.check()?;
        let next = self
            .expanded
            .get()
            .checked_add(n as u64)
            .ok_or(ContainerError::TooLarge)?;
        if next > MAX_EXPANDED {
            return Err(ContainerError::TooLarge);
        }
        self.expanded.set(next);
        Ok(())
    }
    fn node(&self) -> Result<(), ContainerError> {
        let n = self.xml_events.get() + 1;
        if n > 250_000 {
            return Err(ContainerError::TooLarge);
        }
        self.xml_events.set(n);
        Ok(())
    }
}
#[derive(Debug)]
struct Entry {
    name: String,
    flags: u16,
    method: u16,
    crc: u32,
    compressed: u32,
    expanded: u32,
    offset: u32,
}
fn u16le(b: &[u8], n: usize) -> u16 {
    u16::from_le_bytes([b[n], b[n + 1]])
}
fn u32le(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes([b[n], b[n + 1], b[n + 2], b[n + 3]])
}
fn read_array<const N: usize>(r: &mut impl Read) -> Result<[u8; N], ContainerError> {
    let mut b = [0; N];
    r.read_exact(&mut b)?;
    Ok(b)
}
fn seek(r: &mut impl Seek, p: u64) -> Result<(), ContainerError> {
    r.seek(SeekFrom::Start(p))?;
    Ok(())
}
fn checked_end(start: u64, len: u64, bound: u64) -> Result<u64, ContainerError> {
    let end = start
        .checked_add(len)
        .ok_or(ContainerError::InvalidArchive)?;
    if end > bound {
        return Err(ContainerError::InvalidArchive);
    }
    Ok(end)
}

fn path_key(raw: &[u8], flags: u16) -> Result<(String, String), ContainerError> {
    if raw.is_empty() || raw.len() > 2048 || (!raw.is_ascii() && flags & 0x800 == 0) {
        return Err(ContainerError::UnsafePath);
    }
    let name = std::str::from_utf8(raw).map_err(|_| ContainerError::UnsafePath)?;
    if name.chars().count() > 512
        || name.starts_with('/')
        || name.contains('\\')
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"|?*".contains(c))
    {
        return Err(ContainerError::UnsafePath);
    }
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.ends_with([' ', '.']) {
            return Err(ContainerError::UnsafePath);
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "NUL" | "PRN" | "AUX")
            || ["COM", "LPT"].iter().any(|p| {
                stem.strip_prefix(p).is_some_and(|s| {
                    matches!(
                        s,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            })
        {
            return Err(ContainerError::UnsafePath);
        }
    }
    Ok((name.to_owned(), trimmed.to_lowercase()))
}
fn allowed_part(name: &str, kind: OfficeKind) -> Result<(), ContainerError> {
    let n = name.to_lowercase();
    if n.contains("vba")
        || n.contains("macro")
        || n.contains("activex")
        || n.contains("embeddings/")
        || n.starts_with("customui/")
    {
        return Err(ContainerError::ForbiddenContent);
    }
    if !(n == "[content_types].xml"
        || n.starts_with("_rels/")
        || n.starts_with("docprops/")
        || n.starts_with(kind.root()))
    {
        return Err(ContainerError::UnsupportedStructure);
    }
    if n.ends_with('/') {
        return Ok(());
    }
    let ext = n
        .rsplit_once('.')
        .map(|(_, s)| s)
        .ok_or(ContainerError::UnsupportedStructure)?;
    if !matches!(
        ext,
        "xml"
            | "rels"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "bmp"
            | "emf"
            | "wmf"
            | "tif"
            | "tiff"
            | "svg"
            | "vml"
            | "ttf"
            | "otf"
            | "odttf"
            | "bin"
    ) {
        return Err(ContainerError::ForbiddenContent);
    }
    // The only opaque .bin parts enabled are Office printer settings; OLE/embedded
    // objects and arbitrary executable containers are not accepted as printer data.
    if ext == "bin" && !n.contains("/printersettings/") {
        return Err(ContainerError::ForbiddenContent);
    }
    Ok(())
}
fn extra(r: &mut (impl Read + Seek), len: u16) -> Result<(), ContainerError> {
    let mut left = u64::from(len);
    while left > 0 {
        if left < 4 {
            return Err(ContainerError::InvalidArchive);
        }
        let h = read_array::<4>(r)?;
        let id = u16le(&h, 0);
        let n = u64::from(u16le(&h, 2));
        left -= 4;
        if n > left {
            return Err(ContainerError::InvalidArchive);
        }
        if matches!(id, 1 | 0x9901 | 0x7075) {
            return Err(ContainerError::UnsupportedStructure);
        }
        r.seek(SeekFrom::Current(n as i64))?;
        left -= n;
    }
    Ok(())
}
fn directory<R: Read + Seek>(
    r: &mut R,
    len: u64,
    kind: OfficeKind,
    b: &Budget<'_>,
) -> Result<(Vec<Entry>, u64), ContainerError> {
    if !(22..=super::MAX_MATERIAL_BYTES).contains(&len) {
        return Err(ContainerError::TooLarge);
    }
    let tail_len = len.min(65_557) as usize;
    seek(r, len - tail_len as u64)?;
    let mut tail = vec![0u8; tail_len];
    r.read_exact(&mut tail)?;
    let offset = (0..=tail_len - 22)
        .rev()
        .find(|&i| {
            tail[i..i + 4] == [0x50, 0x4b, 5, 6]
                && i + 22 + usize::from(u16le(&tail, i + 20)) == tail_len
        })
        .ok_or(ContainerError::InvalidArchive)?;
    let e = &tail[offset..];
    let count = usize::from(u16le(e, 10));
    if u16le(e, 4) != 0
        || u16le(e, 6) != 0
        || usize::from(u16le(e, 8)) != count
        || count == 0
        || count > MAX_ENTRIES
    {
        return Err(ContainerError::UnsupportedStructure);
    }
    let central = u64::from(u32le(e, 16));
    let central_len = u64::from(u32le(e, 12));
    let end_record = len - tail_len as u64 + offset as u64;
    if checked_end(central, central_len, len)? != end_record {
        return Err(ContainerError::InvalidArchive);
    }
    drop(tail);
    seek(r, central)?;
    let mut entries = Vec::with_capacity(count);
    let mut names = BTreeSet::new();
    let mut metadata = count * (std::mem::size_of::<Entry>() + 256);
    for _ in 0..count {
        b.check()?;
        let h = read_array::<46>(r)?;
        if h[..4] != [0x50, 0x4b, 1, 2] {
            return Err(ContainerError::InvalidArchive);
        }
        let flags = u16le(&h, 8);
        let method = u16le(&h, 10);
        let name_len = usize::from(u16le(&h, 28));
        if flags & !0x80e != 0
            || !matches!(method, 0 | 8)
            || u16le(&h, 34) != 0
            || u16le(&h, 6) > 20
        {
            return Err(ContainerError::UnsupportedStructure);
        }
        if name_len == 0 || name_len > 2048 {
            return Err(ContainerError::UnsafePath);
        }
        let mut raw = vec![0; name_len];
        r.read_exact(&mut raw)?;
        let (name, key) = path_key(&raw, flags)?;
        allowed_part(&name, kind)?;
        metadata = metadata
            .checked_add(name.capacity() + key.capacity())
            .ok_or(ContainerError::TooLarge)?;
        if metadata > MAX_METADATA {
            return Err(ContainerError::TooLarge);
        }
        if !names.insert(key) {
            return Err(ContainerError::DuplicatePath);
        }
        let mode = u32le(&h, 38) >> 16;
        if mode & 0xf000 == 0xa000 {
            return Err(ContainerError::ForbiddenContent);
        }
        let expanded = u32le(&h, 24);
        let compressed = u32le(&h, 20);
        if u64::from(expanded) > MAX_ENTRY || compressed == u32::MAX || u32le(&h, 42) == u32::MAX {
            return Err(ContainerError::TooLarge);
        }
        if name.ends_with('/') && (expanded != 0 || compressed != 0) {
            return Err(ContainerError::InvalidArchive);
        }
        extra(r, u16le(&h, 30))?;
        r.seek(SeekFrom::Current(i64::from(u16le(&h, 32))))?;
        if r.stream_position()? > end_record {
            return Err(ContainerError::InvalidArchive);
        }
        entries.push(Entry {
            name,
            flags,
            method,
            crc: u32le(&h, 16),
            compressed,
            expanded,
            offset: u32le(&h, 42),
        });
    }
    if r.stream_position()? != end_record {
        return Err(ContainerError::InvalidArchive);
    }
    if !entries.iter().any(|e| e.name == "[Content_Types].xml")
        || !entries.iter().any(|e| e.name == "_rels/.rels")
        || !entries.iter().any(|e| e.name == kind.main())
    {
        return Err(ContainerError::UnsupportedStructure);
    }
    entries.sort_by_key(|e| e.offset);
    Ok((entries, central))
}

enum Data<'a, R: Read> {
    Stored(io::Take<&'a mut R>),
    Deflated(DeflateDecoder<BufReader<io::Take<&'a mut R>>>),
}
impl<R: Read> Read for Data<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Stored(r) => r.read(out),
            Self::Deflated(r) => r.read(out),
        }
    }
}
impl<R: Read> Data<'_, R> {
    fn consumed(&self, declared: u32) -> u64 {
        match self {
            Self::Stored(r) => u64::from(declared) - r.limit(),
            Self::Deflated(r) => r.total_in(),
        }
    }
}
struct Accounted<'a, R: Read> {
    data: Data<'a, R>,
    budget: &'a Budget<'a>,
    bytes: u64,
    crc: Hasher,
}
impl<R: Read> Read for Accounted<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.budget.check().map_err(io_error)?;
        let cap = out.len().min(BLOCK);
        let n = self.data.read(&mut out[..cap])?;
        self.budget.output(n).map_err(io_error)?;
        self.bytes = self
            .bytes
            .checked_add(n as u64)
            .ok_or_else(|| io_error(ContainerError::TooLarge))?;
        if self.bytes > MAX_ENTRY {
            return Err(io_error(ContainerError::TooLarge));
        }
        self.crc.update(&out[..n]);
        Ok(n)
    }
}

pub fn validate_office<R: Read + Seek>(
    reader: &mut R,
    kind: OfficeKind,
    control: &dyn ValidationControl,
) -> Result<ContainerReport, ContainerError> {
    let _exclusive = Exclusive::acquire()?;
    let budget = Budget::new(control)?;
    let len = reader.seek(SeekFrom::End(0))?;
    let (entries, central) = directory(reader, len, kind, &budget)?;
    let parts: BTreeSet<&str> = entries
        .iter()
        .filter(|e| !e.name.ends_with('/'))
        .map(|e| e.name.as_str())
        .collect();
    let mut expected_offset = 0u64;
    let mut types_ok = false;
    let mut relationship_ok = false;
    for e in &entries {
        budget.check()?;
        if u64::from(e.offset) != expected_offset {
            return Err(ContainerError::InvalidArchive);
        }
        seek(reader, expected_offset)?;
        let h = read_array::<30>(reader)?;
        if h[..4] != [0x50, 0x4b, 3, 4]
            || u16le(&h, 6) != e.flags
            || u16le(&h, 8) != e.method
            || usize::from(u16le(&h, 26)) != e.name.len()
        {
            return Err(ContainerError::InvalidArchive);
        }
        if e.flags & 8 == 0
            && (u32le(&h, 14) != e.crc
                || u32le(&h, 18) != e.compressed
                || u32le(&h, 22) != e.expanded)
        {
            return Err(ContainerError::InvalidArchive);
        }
        if e.flags & 8 != 0
            && ((u32le(&h, 14) != 0 && u32le(&h, 14) != e.crc)
                || (u32le(&h, 18) != 0 && u32le(&h, 18) != e.compressed)
                || (u32le(&h, 22) != 0 && u32le(&h, 22) != e.expanded))
        {
            return Err(ContainerError::InvalidArchive);
        }
        let mut local = vec![0u8; e.name.len()];
        reader.read_exact(&mut local)?;
        if local != e.name.as_bytes() {
            return Err(ContainerError::InvalidArchive);
        }
        extra(reader, u16le(&h, 28))?;
        let data_start = reader.stream_position()?;
        let data_end = checked_end(data_start, u64::from(e.compressed), central)?;
        let source = reader.take(u64::from(e.compressed));
        let data = if e.method == 0 {
            if e.compressed != e.expanded {
                return Err(ContainerError::InvalidArchive);
            }
            Data::Stored(source)
        } else {
            Data::Deflated(DeflateDecoder::new(BufReader::with_capacity(BLOCK, source)))
        };
        let mut body = Accounted {
            data,
            budget: &budget,
            bytes: 0,
            crc: Hasher::new(),
        };
        if e.name.rsplit_once('.').is_some_and(|(_, ext)| {
            ["xml", "rels", "svg", "vml"]
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        }) {
            let semantic = xml::validate(&mut body, &e.name, kind, &budget, &parts)?;
            types_ok |= semantic.0;
            relationship_ok |= semantic.1;
        } else {
            let mut chunk = [0u8; BLOCK];
            while body.read(&mut chunk)? > 0 {}
        }
        if body.bytes != u64::from(e.expanded)
            || body.data.consumed(e.compressed) != u64::from(e.compressed)
            || body.crc.finalize() != e.crc
        {
            return Err(ContainerError::InvalidArchive);
        }
        seek(reader, data_end)?;
        expected_offset = data_end;
        if e.flags & 8 != 0 {
            let first = read_array::<4>(reader)?;
            let crc = if first == [0x50, 0x4b, 7, 8] {
                u32le(&read_array::<4>(reader)?, 0)
            } else {
                u32le(&first, 0)
            };
            let sizes = read_array::<8>(reader)?;
            if crc != e.crc || u32le(&sizes, 0) != e.compressed || u32le(&sizes, 4) != e.expanded {
                return Err(ContainerError::InvalidArchive);
            }
            expected_offset = reader.stream_position()?;
        }
    }
    if expected_offset != central || !types_ok || !relationship_ok {
        return Err(ContainerError::UnsupportedStructure);
    }
    seek(reader, 0)?;
    let mut hash = Sha256::new();
    let mut chunk = [0u8; BLOCK];
    let mut read_bytes = 0u64;
    loop {
        budget.check()?;
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        read_bytes += n as u64;
        if read_bytes > len {
            return Err(ContainerError::InvalidArchive);
        }
        hash.update(&chunk[..n]);
    }
    if read_bytes != len {
        return Err(ContainerError::InvalidArchive);
    }
    Ok(ContainerReport {
        kind,
        compressed_bytes: len,
        expanded_bytes: budget.expanded.get(),
        entries: entries.len() as u32,
        xml_events: budget.xml_events.get(),
        sha256: hash.finalize().into(),
    })
}

#[cfg(test)]
mod tests;
