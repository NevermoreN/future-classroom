//! Bounded preparation and Office container validation. Native image decode remains
//! the WIC adapter's responsibility; filenames alone never establish media validity.
#![forbid(unsafe_code)]
pub mod office;
pub mod pdf_validation;
pub mod validation;

use std::fmt;
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

pub const MAX_MATERIAL_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_WAV_BYTES: u32 = 1_920_044;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceError {
    InvalidName,
    UnsupportedType,
    TooLarge,
    InvalidWav,
    InvalidArchiveState,
    Canceled,
    Deadline,
    Io,
}
impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ResourceError {}
impl From<io::Error> for ResourceError {
    fn from(_: io::Error) -> Self {
        Self::Io
    }
}

/// A display name, never a path or a Shell argument. The persisted resource path uses IDs.
pub fn validate_display_name(name: &str) -> Result<(), ResourceError> {
    if name.is_empty()
        || name.chars().count() > 180
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || name == "."
        || name == ".."
    {
        return Err(ResourceError::InvalidName);
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| {
            stem.strip_prefix(p).is_some_and(|s| {
                matches!(
                    s,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
    {
        return Err(ResourceError::InvalidName);
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .ok_or(ResourceError::UnsupportedType)?;
    // Legacy Office binary formats require their own validators before being enabled.
    if !matches!(
        ext.as_str(),
        "txt" | "pdf" | "pptx" | "docx" | "xlsx" | "png" | "jpg" | "jpeg" | "bmp"
    ) {
        return Err(ResourceError::UnsupportedType);
    }
    Ok(())
}

/// Bounded 32KiB copy; cancellation/deadline checks precede every read and write.
/// Caller owns staging creation and deletion, and only publishes after sync + full validation.
pub fn copy_bounded<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    max: u64,
    timeout: Duration,
    canceled: impl Fn() -> bool,
) -> Result<u64, ResourceError> {
    let started = Instant::now();
    let mut total = 0u64;
    let mut buf = [0u8; 32 * 1024];
    loop {
        if canceled() {
            return Err(ResourceError::Canceled);
        }
        if started.elapsed() >= timeout {
            return Err(ResourceError::Deadline);
        }
        let n = reader.read(&mut buf)?;
        if canceled() {
            return Err(ResourceError::Canceled);
        }
        if started.elapsed() >= timeout {
            return Err(ResourceError::Deadline);
        }
        if n == 0 {
            return Ok(total);
        }
        total = total.checked_add(n as u64).ok_or(ResourceError::TooLarge)?;
        if total > max {
            return Err(ResourceError::TooLarge);
        }
        let mut written = 0;
        while written < n {
            if canceled() {
                return Err(ResourceError::Canceled);
            }
            if started.elapsed() >= timeout {
                return Err(ResourceError::Deadline);
            }
            let count = writer.write(&buf[written..n])?;
            if count == 0 {
                return Err(ResourceError::Io);
            }
            written += count;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub frames: u32,
    pub data_bytes: u32,
}
impl WavInfo {
    pub fn duration_ms(self) -> u32 {
        self.frames / 16
    }
}
/// The product accepts only the canonical 44-byte PCM16/16kHz/mono header.
/// Checking a header alone does not validate the body: `file_bytes` must be the actual size.
pub fn validate_wav_header(header: &[u8; 44], file_bytes: u64) -> Result<WavInfo, ResourceError> {
    let bad = ResourceError::InvalidWav;
    let u16_at = |i| u16::from_le_bytes([header[i], header[i + 1]]);
    let u32_at = |i| u32::from_le_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]]);
    if &header[0..4] != b"RIFF"
        || &header[8..12] != b"WAVE"
        || &header[12..16] != b"fmt "
        || &header[36..40] != b"data"
        || u32_at(16) != 16
        || u16_at(20) != 1
        || u16_at(22) != 1
        || u32_at(24) != 16_000
        || u32_at(28) != 32_000
        || u16_at(32) != 2
        || u16_at(34) != 16
    {
        return Err(bad);
    }
    let bytes = u32_at(40);
    if bytes % 2 != 0
        || !(16_000..=1_920_000).contains(&bytes)
        || file_bytes != u64::from(bytes) + 44
        || u64::from(u32_at(4)) + 8 != file_bytes
    {
        return Err(bad);
    }
    Ok(WavInfo {
        frames: bytes / 2,
        data_bytes: bytes,
    })
}

#[derive(Debug)]
pub struct ArchiveBudget {
    entries: u32,
    entry_bytes: u64,
    total_bytes: u64,
    started: Instant,
    failure: Option<ResourceError>,
}
impl Default for ArchiveBudget {
    fn default() -> Self {
        Self::new()
    }
}
impl ArchiveBudget {
    pub fn new() -> Self {
        Self {
            entries: 0,
            entry_bytes: 0,
            total_bytes: 0,
            started: Instant::now(),
            failure: None,
        }
    }
    fn check(&mut self) -> Result<(), ResourceError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.started.elapsed() >= Duration::from_secs(30) {
            self.failure = Some(ResourceError::Deadline);
            return Err(ResourceError::Deadline);
        }
        Ok(())
    }
    fn reject(&mut self, error: ResourceError) -> Result<(), ResourceError> {
        self.failure = Some(error);
        Err(error)
    }
    pub fn begin_entry(&mut self) -> Result<(), ResourceError> {
        self.check()?;
        if self.entries >= 2048 {
            return self.reject(ResourceError::TooLarge);
        }
        self.entries += 1;
        self.entry_bytes = 0;
        Ok(())
    }
    /// Call with actual decompressed bytes, never the ZIP directory's declared size.
    pub fn account_output(&mut self, bytes: usize) -> Result<(), ResourceError> {
        self.check()?;
        if self.entries == 0 {
            return self.reject(ResourceError::InvalidArchiveState);
        }
        let Some(entry_bytes) = self.entry_bytes.checked_add(bytes as u64) else {
            return self.reject(ResourceError::TooLarge);
        };
        let Some(total_bytes) = self.total_bytes.checked_add(bytes as u64) else {
            return self.reject(ResourceError::TooLarge);
        };
        if entry_bytes > 32 * 1024 * 1024 || total_bytes > 128 * 1024 * 1024 {
            return self.reject(ResourceError::TooLarge);
        }
        self.entry_bytes = entry_bytes;
        self.total_bytes = total_bytes;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_never_become_shell_paths() {
        for n in [
            "../x.txt",
            "C:\\x.txt",
            "CON.txt",
            "COM1.pdf",
            "x.pptx.exe",
            "x.txt.",
            "a:stream.txt",
        ] {
            assert!(validate_display_name(n).is_err(), "{n}");
        }
        validate_display_name("第一课 讲义.pptx").unwrap();
    }
    fn header(bytes: u32) -> [u8; 44] {
        let mut h = [0; 44];
        h[0..4].copy_from_slice(b"RIFF");
        h[4..8].copy_from_slice(&(bytes + 36).to_le_bytes());
        h[8..16].copy_from_slice(b"WAVEfmt ");
        h[16..20].copy_from_slice(&16u32.to_le_bytes());
        h[20..22].copy_from_slice(&1u16.to_le_bytes());
        h[22..24].copy_from_slice(&1u16.to_le_bytes());
        h[24..28].copy_from_slice(&16000u32.to_le_bytes());
        h[28..32].copy_from_slice(&32000u32.to_le_bytes());
        h[32..34].copy_from_slice(&2u16.to_le_bytes());
        h[34..36].copy_from_slice(&16u16.to_le_bytes());
        h[36..40].copy_from_slice(b"data");
        h[40..44].copy_from_slice(&bytes.to_le_bytes());
        h
    }
    #[test]
    fn exact_voice_limits_and_truncated_or_extended_body() {
        for bytes in [16_000u32, 1_920_000] {
            assert!(validate_wav_header(&header(bytes), u64::from(bytes) + 44).is_ok());
        }
        for bytes in [15_998u32, 1_920_002, 16_001] {
            assert!(validate_wav_header(&header(bytes), u64::from(bytes) + 44).is_err());
        }
        assert!(validate_wav_header(&header(16_000), 16_045).is_err());
        let mut h = header(16_000);
        h[22] = 2;
        assert!(validate_wav_header(&h, 16_044).is_err());
    }
    #[test]
    fn copy_limits_and_cancellation_preserve_destination_budget() {
        let mut out = Vec::new();
        assert_eq!(
            copy_bounded(
                &mut &b"hello"[..],
                &mut out,
                4,
                Duration::from_secs(1),
                || false
            ),
            Err(ResourceError::TooLarge)
        );
        assert!(out.is_empty());
        assert_eq!(
            copy_bounded(
                &mut &b"hello"[..],
                &mut out,
                5,
                Duration::from_secs(1),
                || true
            ),
            Err(ResourceError::Canceled)
        );
    }
    #[test]
    fn actual_output_not_compressed_size_counts_toward_budget() {
        let mut b = ArchiveBudget::new();
        b.begin_entry().unwrap();
        assert_eq!(
            b.account_output(32 * 1024 * 1024 + 1),
            Err(ResourceError::TooLarge)
        );
        let mut b = ArchiveBudget::new();
        for _ in 0..2048 {
            b.begin_entry().unwrap();
        }
        assert_eq!(b.begin_entry(), Err(ResourceError::TooLarge));
    }
    #[test]
    fn legacy_office_is_not_enabled_by_filename_alone() {
        for name in ["lesson.ppt", "lesson.doc", "lesson.xls"] {
            assert_eq!(
                validate_display_name(name),
                Err(ResourceError::UnsupportedType)
            );
        }
    }
    #[test]
    fn archive_failure_cannot_be_cleared_by_starting_another_entry() {
        let mut b = ArchiveBudget::new();
        b.begin_entry().unwrap();
        assert_eq!(
            b.account_output(32 * 1024 * 1024 + 1),
            Err(ResourceError::TooLarge)
        );
        assert_eq!(b.begin_entry(), Err(ResourceError::TooLarge));
        assert_eq!(b.account_output(0), Err(ResourceError::TooLarge));
        let mut b = ArchiveBudget::new();
        assert!(b.account_output(1).is_err());
        assert!(b.begin_entry().is_err());
    }
    #[test]
    fn cancellation_between_partial_writes_stops_the_copy() {
        use std::cell::Cell;
        struct Partial<'a> {
            writes: &'a Cell<usize>,
            bytes: Vec<u8>,
        }
        impl Write for Partial<'_> {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.writes.set(self.writes.get() + 1);
                self.bytes.push(buf[0]);
                Ok(1)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let writes = Cell::new(0);
        let mut out = Partial {
            writes: &writes,
            bytes: vec![],
        };
        assert_eq!(
            copy_bounded(
                &mut &b"hello"[..],
                &mut out,
                5,
                Duration::from_secs(1),
                || writes.get() > 0
            ),
            Err(ResourceError::Canceled)
        );
        assert_eq!(out.bytes, b"h");
    }
    #[test]
    fn deadline_is_checked_again_after_a_slow_read_before_writing() {
        struct Slow;
        impl Read for Slow {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                std::thread::sleep(Duration::from_millis(5));
                buf[0] = 1;
                Ok(1)
            }
        }
        let mut out = vec![];
        assert_eq!(
            copy_bounded(&mut Slow, &mut out, 10, Duration::from_millis(1), || false),
            Err(ResourceError::Deadline)
        );
        assert!(out.is_empty());
    }
}
