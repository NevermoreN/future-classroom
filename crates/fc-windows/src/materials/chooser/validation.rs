//! Pure lexical and streaming checks shared by the Windows destination adapter.
use std::io::{self, Read, Write};

pub(super) const COPY_BLOCK_BYTES: usize = 32 * 1024;

pub(super) fn safe_display_path(path: &str) -> bool {
    !path.is_empty()
        && path.encode_utf16().count() <= 32767
        && !path.chars().any(|c| {
            c.is_control()
                || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

pub(super) fn backup_name(name: &str) -> bool {
    if name.is_empty()
        || name.encode_utf16().count() > 180
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
        || !name.to_ascii_lowercase().ends_with(".fcbak")
    {
        return false;
    }
    let base = name.split('.').next().unwrap_or("").trim_end_matches(' ');
    let upper = base.to_ascii_uppercase();
    !base.is_empty()
        && !matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
        && !["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"].contains(&upper.as_str())
}

#[derive(Debug)]
pub(super) enum CopyError {
    Cancelled,
    InvalidLength,
    UnexpectedEof,
    TrailingData,
    Io(io::Error),
}
impl From<io::Error> for CopyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
fn active(check: &mut impl FnMut() -> bool) -> Result<(), CopyError> {
    if check() {
        Ok(())
    } else {
        Err(CopyError::Cancelled)
    }
}
/// Hash only bytes actually accepted by Write, including partial writes. The
/// caller discards every output on error, and validates the final digest itself.
pub(super) fn copy_exact(
    reader: &mut impl Read,
    writer: &mut impl Write,
    expected_bytes: u64,
    max_bytes: u64,
    check: &mut impl FnMut() -> bool,
    written: &mut impl FnMut(&[u8]) -> io::Result<()>,
) -> Result<(), CopyError> {
    if expected_bytes == 0 || expected_bytes > max_bytes {
        return Err(CopyError::InvalidLength);
    }
    let mut remaining = expected_bytes;
    let mut block = [0; COPY_BLOCK_BYTES];
    while remaining != 0 {
        active(check)?;
        let count = remaining.min(COPY_BLOCK_BYTES as u64) as usize;
        let count = loop {
            match reader.read(&mut block[..count]) {
                Ok(0) => return Err(CopyError::UnexpectedEof),
                Ok(count) => break count,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => active(check)?,
                Err(e) => return Err(e.into()),
            }
        };
        let mut offset = 0;
        while offset < count {
            active(check)?;
            match writer.write(&block[offset..count]) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero).into()),
                Ok(n) => {
                    written(&block[offset..offset + n])?;
                    offset += n;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                Err(e) => return Err(e.into()),
            }
        }
        remaining -= count as u64;
    }
    loop {
        active(check)?;
        match reader.read(&mut [0; 1]) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err(CopyError::TrailingData),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_display_paths_preserve_chinese_and_emoji_but_reject_bidi_and_controls() {
        assert!(safe_display_path("C:\\课堂资料😀\\完整备份.fcbak"));
        for c in [
            '\0', '\n', '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}',
            '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ] {
            assert!(!safe_display_path(&format!("C:\\资料{c}\\backup.fcbak")));
        }
        let boundary = format!("{}a", "😀".repeat(16383));
        assert!(safe_display_path(&boundary));
        assert!(!safe_display_path(&format!("{boundary}b")));
    }
    #[test]
    fn backup_names_reject_devices_ads_navigation_and_wrong_extensions() {
        for name in ["课堂备份.fcbak", "backup.FCBAK", "class.room.fcbak"] {
            assert!(backup_name(name), "{name}");
        }
        for name in [
            "",
            ".fcbak",
            "CON.fcbak",
            "com1.fcbak",
            "LPT².fcbak",
            "NUL.txt.fcbak",
            "../a.fcbak",
            "a:b.fcbak",
            "a.fcbak ",
            "a.txt",
            "a\0.fcbak",
        ] {
            assert!(!backup_name(name), "{name}");
        }
        assert!(!backup_name(&format!("{}.fcbak", "中".repeat(180))));
    }
    struct Partial(Vec<u8>);
    impl Write for Partial {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let n = bytes.len().min(3);
            self.0.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn short_writes_hash_exactly_the_written_bytes() {
        let input = vec![7u8; COPY_BLOCK_BYTES * 2 + 17];
        let mut output = Partial(Vec::new());
        let mut hash_input = Vec::new();
        copy_exact(
            &mut input.as_slice(),
            &mut output,
            input.len() as u64,
            100_000,
            &mut || true,
            &mut |b| {
                hash_input.extend_from_slice(b);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(output.0, input);
        assert_eq!(hash_input, input);
    }
    #[test]
    fn cancellation_lengths_truncation_and_trailing_bytes_are_rejected() {
        let mut output = Vec::new();
        for (input, expected, limit, variant) in [
            (&b"abc"[..], 0, 9, 0),
            (&b"abc"[..], 10, 9, 0),
            (&b"ab"[..], 3, 9, 1),
            (&b"abcd"[..], 3, 9, 2),
        ] {
            let mut source = input;
            let result = copy_exact(
                &mut source,
                &mut output,
                expected,
                limit,
                &mut || true,
                &mut |_| Ok(()),
            );
            assert!(matches!(
                (variant, result),
                (0, Err(CopyError::InvalidLength))
                    | (1, Err(CopyError::UnexpectedEof))
                    | (2, Err(CopyError::TrailingData))
            ));
        }
        assert!(matches!(
            copy_exact(
                &mut &b"abc"[..],
                &mut output,
                3,
                9,
                &mut || false,
                &mut |_| Ok(())
            ),
            Err(CopyError::Cancelled)
        ));
        // Reading CopyError's owned I/O error also verifies it is retained.
        let error = CopyError::from(io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(matches!(error, CopyError::Io(e) if e.kind() == io::ErrorKind::PermissionDenied));
    }
}
