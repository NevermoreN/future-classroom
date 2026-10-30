//! Streaming non-PDF preparation checks. Images still require native WIC decode;
//! PDF parsing belongs to the separately isolated PDF helper, never this worker.
use crate::office::{self, ContainerError, OfficeKind, ValidationControl};
use fc_domain::materials::{validate_material_name, MaterialKind};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom},
    time::{Duration, Instant},
};
pub const BLOCK: usize = 32 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    Io,
    InvalidName,
    UnsupportedType,
    InvalidText,
    InvalidImage,
    InvalidPdf,
    EncryptedPdf,
    ForbiddenContent,
    TooLarge,
    PdfRequiresHelper,
    Canceled,
    Deadline,
    CpuBudget,
    ClockUnavailable,
    Office(ContainerError),
}
impl From<std::io::Error> for ValidationError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileValidationReport {
    pub kind: MaterialKind,
    pub bytes: u64,
    pub sha256: [u8; 32],
    pub image: Option<ImageInfo>,
    pub text_characters: Option<u32>,
}
impl FileValidationReport {
    pub fn requires_native_image_decode(&self) -> bool {
        self.image.is_some()
    }
}
pub(crate) struct Budget<'a> {
    control: &'a dyn ValidationControl,
    start: Instant,
    cpu: Duration,
}
impl<'a> Budget<'a> {
    pub(crate) fn new(control: &'a dyn ValidationControl) -> Result<Self, ValidationError> {
        let cpu = control.thread_cpu_time().map_err(control_error)?;
        let budget = Self {
            control,
            start: Instant::now(),
            cpu,
        };
        budget.check()?;
        Ok(budget)
    }
    pub(crate) fn check(&self) -> Result<(), ValidationError> {
        if self.control.canceled() {
            return Err(ValidationError::Canceled);
        }
        if self.start.elapsed() >= Duration::from_secs(30) {
            return Err(ValidationError::Deadline);
        }
        let elapsed = self
            .control
            .thread_cpu_time()
            .map_err(control_error)?
            .checked_sub(self.cpu)
            .ok_or(ValidationError::ClockUnavailable)?;
        if elapsed >= Duration::from_secs(10) {
            return Err(ValidationError::CpuBudget);
        }
        Ok(())
    }
}
fn control_error(error: ContainerError) -> ValidationError {
    match error {
        ContainerError::Canceled => ValidationError::Canceled,
        ContainerError::Deadline => ValidationError::Deadline,
        ContainerError::CpuBudget => ValidationError::CpuBudget,
        _ => ValidationError::ClockUnavailable,
    }
}
fn image_info(width: u32, height: u32) -> Result<ImageInfo, ValidationError> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 8_000_000 {
        return Err(ValidationError::TooLarge);
    }
    Ok(ImageInfo { width, height })
}
fn be32(s: &[u8]) -> Result<u32, ValidationError> {
    Ok(u32::from_be_bytes(
        s.try_into().map_err(|_| ValidationError::InvalidImage)?,
    ))
}
fn le32(s: &[u8]) -> Result<u32, ValidationError> {
    Ok(u32::from_le_bytes(
        s.try_into().map_err(|_| ValidationError::InvalidImage)?,
    ))
}
/// Header/metadata only: a successful report never claims pixels were decoded.
pub fn inspect_image_header(
    kind: MaterialKind,
    data: &[u8],
    file_bytes: u64,
) -> Result<ImageInfo, ValidationError> {
    if file_bytes == 0 || file_bytes > 10 * 1024 * 1024 {
        return Err(ValidationError::TooLarge);
    }
    match kind {
        MaterialKind::Png => {
            if data.len() < 33
                || &data[..8] != b"\x89PNG\r\n\x1a\n"
                || be32(&data[8..12])? != 13
                || &data[12..16] != b"IHDR"
            {
                return Err(ValidationError::InvalidImage);
            }
            let depth = data[24];
            let color = data[25];
            if !matches!(
                (color, depth),
                (0, 1 | 2 | 4 | 8 | 16)
                    | (2, 8 | 16)
                    | (3, 1 | 2 | 4 | 8)
                    | (4, 8 | 16)
                    | (6, 8 | 16)
            ) || data[26] != 0
                || data[27] != 0
                || data[28] > 1
            {
                return Err(ValidationError::InvalidImage);
            }
            let mut crc = crc32fast::Hasher::new();
            crc.update(&data[12..29]);
            if crc.finalize() != be32(&data[29..33])? {
                return Err(ValidationError::InvalidImage);
            }
            image_info(be32(&data[16..20])?, be32(&data[20..24])?)
        }
        MaterialKind::Bmp => {
            if data.len() < 26 || &data[..2] != b"BM" || u64::from(le32(&data[2..6])?) != file_bytes
            {
                return Err(ValidationError::InvalidImage);
            }
            let offset = le32(&data[10..14])?;
            let dib = le32(&data[14..18])?;
            let (width, height, planes, bits) = if dib == 12 {
                (
                    u32::from(u16::from_le_bytes([data[18], data[19]])),
                    u32::from(u16::from_le_bytes([data[20], data[21]])),
                    u16::from_le_bytes([data[22], data[23]]),
                    u16::from_le_bytes([data[24], data[25]]),
                )
            } else if matches!(dib, 40 | 52 | 56 | 108 | 124) && data.len() >= 54 {
                let width = i32::from_le_bytes(
                    data[18..22]
                        .try_into()
                        .map_err(|_| ValidationError::InvalidImage)?,
                );
                let height = i32::from_le_bytes(
                    data[22..26]
                        .try_into()
                        .map_err(|_| ValidationError::InvalidImage)?,
                );
                if width <= 0 || height == 0 || height == i32::MIN {
                    return Err(ValidationError::InvalidImage);
                }
                if !matches!(le32(&data[30..34])?, 0 | 1 | 2 | 3 | 6) {
                    return Err(ValidationError::InvalidImage);
                }
                (
                    width as u32,
                    height.unsigned_abs(),
                    u16::from_le_bytes([data[26], data[27]]),
                    u16::from_le_bytes([data[28], data[29]]),
                )
            } else {
                return Err(ValidationError::InvalidImage);
            };
            if planes != 1
                || !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32)
                || offset < 14 + dib
                || u64::from(offset) >= file_bytes
            {
                return Err(ValidationError::InvalidImage);
            }
            image_info(width, height)
        }
        MaterialKind::Jpeg => {
            if data.len() < 4 || data[..2] != [0xff, 0xd8] {
                return Err(ValidationError::InvalidImage);
            }
            let mut at = 2usize;
            while at < data.len() {
                if data[at] != 0xff {
                    return Err(ValidationError::InvalidImage);
                }
                while at < data.len() && data[at] == 0xff {
                    at += 1
                }
                let marker = *data.get(at).ok_or(ValidationError::InvalidImage)?;
                at += 1;
                if matches!(marker, 0x00 | 0xd8 | 0xd9 | 0xda) {
                    return Err(ValidationError::InvalidImage);
                }
                if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                    continue;
                }
                let len_data = data.get(at..at + 2).ok_or(ValidationError::InvalidImage)?;
                let len = usize::from(u16::from_be_bytes([len_data[0], len_data[1]]));
                if len < 2 || at.checked_add(len).is_none_or(|end| end > data.len()) {
                    return Err(ValidationError::InvalidImage);
                }
                if matches!(marker, 0xc0..=0xc2) {
                    if len < 8 || data[at + 2] != 8 {
                        return Err(ValidationError::InvalidImage);
                    }
                    let components = usize::from(data[at + 7]);
                    if !matches!(components, 1 | 3 | 4) || len != 8 + components * 3 {
                        return Err(ValidationError::InvalidImage);
                    }
                    return image_info(
                        u32::from(u16::from_be_bytes([data[at + 5], data[at + 6]])),
                        u32::from(u16::from_be_bytes([data[at + 3], data[at + 4]])),
                    );
                }
                at += len;
            }
            Err(ValidationError::InvalidImage)
        }
        _ => Err(ValidationError::UnsupportedType),
    }
}
fn valid_character(c: char) -> bool {
    (!c.is_control() || matches!(c, '\r' | '\n' | '\t')) && c != '\u{feff}'
}
#[derive(Clone, Copy)]
enum TextEncoding {
    Unknown,
    Utf8,
    Utf16(bool),
}
struct TextValidator {
    encoding: TextEncoding,
    prefix: [u8; 3],
    prefix_len: usize,
    utf8: [u8; 4],
    utf8_len: usize,
    utf8_need: usize,
    byte: Option<u8>,
    high: Option<u16>,
    count: u32,
    content: Option<String>,
}
impl TextValidator {
    fn new() -> Self {
        Self {
            encoding: TextEncoding::Unknown,
            prefix: [0; 3],
            prefix_len: 0,
            utf8: [0; 4],
            utf8_len: 0,
            utf8_need: 0,
            byte: None,
            high: None,
            count: 0,
            content: None,
        }
    }
    fn character(&mut self, c: char) -> Result<(), ValidationError> {
        if !valid_character(c) && !(c == '\u{feff}' && self.content.is_some()) {
            return Err(ValidationError::InvalidText);
        }
        self.count = self.count.checked_add(1).ok_or(ValidationError::TooLarge)?;
        if let Some(text) = &mut self.content {
            if self.count as usize > fc_domain::profile_content::MAX_CONTENT_CHARACTERS
                || text.len() + c.len_utf8() > fc_domain::profile_content::MAX_CONTENT_TEXT_BYTES
            {
                return Err(ValidationError::TooLarge);
            }
            text.push(c);
        }
        Ok(())
    }
    fn data_byte(&mut self, byte: u8) -> Result<(), ValidationError> {
        match self.encoding {
            TextEncoding::Unknown => Err(ValidationError::InvalidText),
            TextEncoding::Utf8 => {
                if self.utf8_len == 0 {
                    if byte < 128 {
                        return self.character(char::from(byte));
                    }
                    self.utf8_need = match byte {
                        0xc2..=0xdf => 2,
                        0xe0..=0xef => 3,
                        0xf0..=0xf4 => 4,
                        _ => return Err(ValidationError::InvalidText),
                    };
                } else if !(0x80..=0xbf).contains(&byte) {
                    return Err(ValidationError::InvalidText);
                }
                self.utf8[self.utf8_len] = byte;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_need {
                    let c = std::str::from_utf8(&self.utf8[..self.utf8_len])
                        .map_err(|_| ValidationError::InvalidText)?
                        .chars()
                        .next()
                        .ok_or(ValidationError::InvalidText)?;
                    self.utf8_len = 0;
                    self.character(c)?;
                }
                Ok(())
            }
            TextEncoding::Utf16(little) => {
                let Some(first) = self.byte.take() else {
                    self.byte = Some(byte);
                    return Ok(());
                };
                let word = if little {
                    u16::from_le_bytes([first, byte])
                } else {
                    u16::from_be_bytes([first, byte])
                };
                if let Some(high) = self.high.take() {
                    if !(0xdc00..=0xdfff).contains(&word) {
                        return Err(ValidationError::InvalidText);
                    }
                    let scalar =
                        0x10000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(word) - 0xdc00);
                    self.character(char::from_u32(scalar).ok_or(ValidationError::InvalidText)?)
                } else if (0xd800..=0xdbff).contains(&word) {
                    self.high = Some(word);
                    Ok(())
                } else if (0xdc00..=0xdfff).contains(&word) {
                    Err(ValidationError::InvalidText)
                } else {
                    self.character(
                        char::from_u32(u32::from(word)).ok_or(ValidationError::InvalidText)?,
                    )
                }
            }
        }
    }
    fn feed(&mut self, bytes: &[u8]) -> Result<(), ValidationError> {
        for byte in bytes.iter().copied() {
            if matches!(self.encoding, TextEncoding::Unknown) {
                self.prefix[self.prefix_len] = byte;
                self.prefix_len += 1;
                if self.prefix_len == 2 && matches!(&self.prefix[..2], [0xff, 0xfe] | [0xfe, 0xff])
                {
                    self.encoding = TextEncoding::Utf16(self.prefix[0] == 0xff);
                } else if self.prefix_len == 3 {
                    self.encoding = TextEncoding::Utf8;
                    if self.prefix != [0xef, 0xbb, 0xbf] {
                        for b in self.prefix {
                            self.data_byte(b)?;
                        }
                    }
                }
            } else {
                self.data_byte(byte)?;
            }
        }
        Ok(())
    }
    fn finalize(&mut self) -> Result<(), ValidationError> {
        if matches!(self.encoding, TextEncoding::Unknown) {
            self.encoding = TextEncoding::Utf8;
            for n in 0..self.prefix_len {
                self.data_byte(self.prefix[n])?;
            }
        }
        if self.utf8_len != 0 || self.byte.is_some() || self.high.is_some() {
            return Err(ValidationError::InvalidText);
        }
        Ok(())
    }
    fn finish(mut self) -> Result<u32, ValidationError> {
        self.finalize()?;
        Ok(self.count)
    }
}
/// Independent text-node import, with the same strict incremental UTF-8/BOM or
/// BOM-marked UTF-16 decoder used for material validation. Preserve formatting;
/// never guess a legacy code page or replace malformed Unicode. Only this
/// content path collects text and applies its 10,000-scalar/40,000-byte policy;
/// ordinary material TXT files retain their existing larger-original policy.
pub fn decode_content_text<R: Read + Seek>(
    reader: &mut R,
    control: &dyn ValidationControl,
) -> Result<String, ValidationError> {
    let budget = Budget::new(control)?;
    let bytes = reader.seek(SeekFrom::End(0))?;
    if bytes > 64 * 1024 {
        return Err(ValidationError::TooLarge);
    }
    reader.seek(SeekFrom::Start(0))?;
    let mut text = TextValidator::new();
    text.content = Some(String::new());
    let mut block = [0u8; BLOCK];
    let mut total = 0u64;
    loop {
        budget.check()?;
        let n = reader.read(&mut block)?;
        budget.check()?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(ValidationError::TooLarge)?;
        if total > bytes || total > 64 * 1024 {
            return Err(ValidationError::TooLarge);
        }
        text.feed(&block[..n])?;
    }
    if total != bytes {
        return Err(ValidationError::Io);
    }
    text.finalize()?;
    budget.check()?;
    text.content.ok_or(ValidationError::InvalidText)
}
/// Read in bounded blocks; PDF always fails with PdfRequiresHelper. Calling code
/// must run validate_pdf on a read-only mapping in the OS-limited helper instead.
pub fn validate_file<R: Read + Seek>(
    reader: &mut R,
    display_name: &str,
    control: &dyn ValidationControl,
) -> Result<FileValidationReport, ValidationError> {
    validate_material_name(display_name).map_err(|_| ValidationError::InvalidName)?;
    let kind = MaterialKind::from_name(display_name).ok_or(ValidationError::UnsupportedType)?;
    let budget = Budget::new(control)?;
    let bytes = reader.seek(SeekFrom::End(0))?;
    if bytes == 0 || bytes > 50 * 1024 * 1024 {
        return Err(ValidationError::TooLarge);
    }
    reader.seek(SeekFrom::Start(0))?;
    if kind == MaterialKind::Pdf {
        return Err(ValidationError::PdfRequiresHelper);
    }
    if let Some(office_kind) = match kind {
        MaterialKind::Docx => Some(OfficeKind::Docx),
        MaterialKind::Pptx => Some(OfficeKind::Pptx),
        MaterialKind::Xlsx => Some(OfficeKind::Xlsx),
        _ => None,
    } {
        let report = office::validate_office(reader, office_kind, control)
            .map_err(ValidationError::Office)?;
        budget.check()?;
        return Ok(FileValidationReport {
            kind,
            bytes: report.compressed_bytes,
            sha256: report.sha256,
            image: None,
            text_characters: None,
        });
    }
    if matches!(
        kind,
        MaterialKind::Png | MaterialKind::Jpeg | MaterialKind::Bmp
    ) && bytes > 10 * 1024 * 1024
    {
        return Err(ValidationError::TooLarge);
    }
    let mut hash = Sha256::new();
    let mut block = [0u8; BLOCK];
    let mut sample = Vec::new();
    let mut total = 0u64;
    let mut text = TextValidator::new();
    loop {
        budget.check()?;
        let n = reader.read(&mut block)?;
        budget.check()?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(ValidationError::TooLarge)?;
        if total > bytes {
            return Err(ValidationError::Io);
        }
        hash.update(&block[..n]);
        if kind == MaterialKind::Txt {
            text.feed(&block[..n])?;
        } else {
            let take = n.min((256 * 1024) - sample.len());
            sample.extend_from_slice(&block[..take]);
        }
    }
    if total != bytes {
        return Err(ValidationError::Io);
    }
    let (image, text_characters) = if kind == MaterialKind::Txt {
        (None, Some(text.finish()?))
    } else {
        (Some(inspect_image_header(kind, &sample, bytes)?), None)
    };
    budget.check()?;
    Ok(FileValidationReport {
        kind,
        bytes,
        sha256: hash.finalize().into(),
        image,
        text_characters,
    })
}

/// Hash a fixed-length material/copy with one 32KiB buffer. This never parses or
/// renders the file and retains no body bytes after return. Reader length changes
/// during hashing are an IO failure; the native caller must also keep its lock.
pub fn hash_file<R: Read + Seek>(
    reader: &mut R,
    control: &dyn ValidationControl,
) -> Result<(u64, [u8; 32]), ValidationError> {
    let budget = Budget::new(control)?;
    let expected = reader.seek(SeekFrom::End(0))?;
    if expected > 50 * 1024 * 1024 {
        return Err(ValidationError::TooLarge);
    }
    reader.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut block = [0u8; BLOCK];
    let mut total = 0u64;
    loop {
        budget.check()?;
        let n = reader.read(&mut block)?;
        budget.check()?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(ValidationError::TooLarge)?;
        if total > expected {
            return Err(ValidationError::Io);
        }
        hash.update(&block[..n]);
    }
    if total != expected {
        return Err(ValidationError::Io);
    }
    budget.check()?;
    Ok((total, hash.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        io::{Cursor, SeekFrom},
    };
    struct Control;
    impl ValidationControl for Control {
        fn canceled(&self) -> bool {
            false
        }
        fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
            Ok(Duration::ZERO)
        }
    }
    fn validate(bytes: Vec<u8>, name: &str) -> Result<FileValidationReport, ValidationError> {
        validate_file(&mut Cursor::new(bytes), name, &Control)
    }
    fn utf16(text: &str, little: bool) -> Vec<u8> {
        let mut data = if little {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        };
        for word in text.encode_utf16() {
            data.extend_from_slice(&if little {
                word.to_le_bytes()
            } else {
                word.to_be_bytes()
            });
        }
        data
    }
    struct Chunked {
        inner: Cursor<Vec<u8>>,
        max: usize,
    }
    impl Read for Chunked {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            assert!(buf.len() <= BLOCK);
            let n = buf.len().min(self.max);
            self.inner.read(&mut buf[..n])
        }
    }
    impl Seek for Chunked {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }
    #[test]
    fn utf8_utf16_boms_and_surrogates_are_validated_across_every_byte_and_block_boundary() {
        for little in [true, false] {
            let data = utf16("你好🙂\r\n", little);
            let mut reader = Chunked {
                inner: Cursor::new(data),
                max: 1,
            };
            assert_eq!(
                validate_file(&mut reader, "文件.txt", &Control)
                    .unwrap()
                    .text_characters,
                Some(5)
            );
        }
        for data in [utf16(&"🙂".repeat(10_000), true), {
            let mut data = vec![0xef, 0xbb, 0xbf];
            data.extend_from_slice("🙂".repeat(10_000).as_bytes());
            data
        }] {
            let report = validate(data, "数据.txt").unwrap();
            assert_eq!(report.text_characters, Some(10_000));
            assert!(!report.requires_native_image_decode());
        }
    }
    #[test]
    fn binary_controls_bad_sequences_and_truncated_utf16_fail_while_long_originals_are_allowed() {
        for bytes in [
            vec![0],
            vec![0xc0, 0xaf],
            vec![0xe2, 0x82],
            vec![0xed, 0xa0, 0x80],
            vec![0xff, 0xfe, 0x41],
            vec![0xff, 0xfe, 0, 0xd8],
            vec![0xfe, 0xff, 0xdc, 0],
            b"hello\x1b[31m".to_vec(),
        ] {
            assert_eq!(
                validate(bytes, "文件.txt"),
                Err(ValidationError::InvalidText)
            );
        }
        assert_eq!(
            validate(vec![b'a'; 50_001], "文件.txt")
                .unwrap()
                .text_characters,
            Some(50_001)
        );
    }
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.extend_from_slice(&13u32.to_be_bytes());
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut crc = crc32fast::Hasher::new();
        crc.update(&data[12..]);
        data.extend_from_slice(&crc.finalize().to_be_bytes());
        data
    }
    #[test]
    fn image_header_reports_dimensions_and_requires_native_full_decode() {
        let data = png(200, 100);
        let report = validate(data.clone(), "图片.png").unwrap();
        assert_eq!(
            report.image,
            Some(ImageInfo {
                width: 200,
                height: 100
            })
        );
        assert!(report.requires_native_image_decode());
        assert_eq!(report.sha256, <Sha256 as Digest>::digest(&data).as_slice());
        let mut damaged = data;
        damaged[29] ^= 1;
        assert_eq!(
            validate(damaged, "图片.png"),
            Err(ValidationError::InvalidImage)
        );
        assert_eq!(
            validate(png(9000, 1000), "图片.png"),
            Err(ValidationError::TooLarge)
        );
    }
    #[test]
    fn bmp_and_jpeg_metadata_reject_forged_geometry_and_lengths() {
        let mut bmp = vec![0u8; 58];
        bmp[..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&58u32.to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&1u32.to_le_bytes());
        bmp[22..26].copy_from_slice(&1u32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        assert_eq!(
            validate(bmp.clone(), "图片.bmp").unwrap().image,
            Some(ImageInfo {
                width: 1,
                height: 1
            })
        );
        bmp[2] = 57;
        assert_eq!(
            validate(bmp, "图片.bmp"),
            Err(ValidationError::InvalidImage)
        );
        let jpeg = vec![
            0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 1, 0, 2, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0,
            0xff, 0xd9,
        ];
        assert_eq!(
            validate(jpeg, "图片.jpg").unwrap().image,
            Some(ImageInfo {
                width: 2,
                height: 1
            })
        );
        assert_eq!(
            validate(vec![0xff, 0xd8, 0xff, 0xc0, 0, 1], "图片.jpg"),
            Err(ValidationError::InvalidImage)
        );
    }
    #[test]
    fn pdf_is_never_accepted_by_magic_and_unsafe_names_are_rejected() {
        assert_eq!(
            validate(b"%PDF-1.7\n".to_vec(), "课件.pdf"),
            Err(ValidationError::PdfRequiresHelper)
        );
        assert_eq!(
            validate(b"hello".to_vec(), "../课件.txt"),
            Err(ValidationError::InvalidName)
        );
        assert_eq!(
            validate(b"hello".to_vec(), "课件.exe"),
            Err(ValidationError::InvalidName)
        );
    }
    #[test]
    fn cancellation_cpu_limit_and_cpu_rollback_fail_before_unbounded_work() {
        struct Canceled;
        impl ValidationControl for Canceled {
            fn canceled(&self) -> bool {
                true
            }
            fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
                Ok(Duration::ZERO)
            }
        }
        assert_eq!(
            validate_file(&mut Cursor::new(b"hello"), "文件.txt", &Canceled),
            Err(ValidationError::Canceled)
        );
        struct Cpu(Cell<u32>, bool);
        impl ValidationControl for Cpu {
            fn canceled(&self) -> bool {
                false
            }
            fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
                let n = self.0.get();
                self.0.set(n + 1);
                Ok(Duration::from_secs(if self.1 {
                    if n == 0 {
                        20
                    } else {
                        19
                    }
                } else {
                    u64::from(n) * 11
                }))
            }
        }
        assert_eq!(
            validate_file(
                &mut Cursor::new(b"hello"),
                "文件.txt",
                &Cpu(Cell::new(0), false)
            ),
            Err(ValidationError::CpuBudget)
        );
        assert_eq!(
            validate_file(
                &mut Cursor::new(b"hello"),
                "文件.txt",
                &Cpu(Cell::new(0), true)
            ),
            Err(ValidationError::ClockUnavailable)
        );
    }
    #[test]
    fn content_txt_preserves_unicode_and_formatting_across_every_decoder_boundary() {
        let expected = "\t中文😀\r\n第二行\n\u{feff}正文";
        let mut forms = vec![
            expected.as_bytes().to_vec(),
            utf16(expected, true),
            utf16(expected, false),
        ];
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend_from_slice(expected.as_bytes());
        forms.push(bom);
        for form in forms {
            for max in 1..=7 {
                let mut input = Chunked {
                    inner: Cursor::new(form.clone()),
                    max,
                };
                assert_eq!(decode_content_text(&mut input, &Control).unwrap(), expected);
            }
        }
    }
    #[test]
    fn content_txt_counts_scalars_and_bytes_without_restricting_material_originals() {
        let maximum = "😀".repeat(10_000);
        for bytes in [
            maximum.as_bytes().to_vec(),
            utf16(&maximum, true),
            utf16(&maximum, false),
        ] {
            assert_eq!(
                decode_content_text(&mut Cursor::new(bytes), &Control).unwrap(),
                maximum
            );
        }
        for invalid in ["😀".repeat(10_001), "a".repeat(10_001)] {
            assert_eq!(
                decode_content_text(&mut Cursor::new(invalid.as_bytes()), &Control),
                Err(ValidationError::TooLarge)
            );
        }
        let large_original = vec![b'a'; 50_001];
        assert!(validate_file(&mut Cursor::new(&large_original), "课堂资料.txt", &Control).is_ok());
        assert_eq!(
            decode_content_text(&mut Cursor::new(large_original), &Control),
            Err(ValidationError::TooLarge)
        );
        assert_eq!(
            decode_content_text(&mut Cursor::new(vec![b'a'; 65_537]), &Control),
            Err(ValidationError::TooLarge)
        );
    }
    #[test]
    fn content_txt_rejects_invalid_unicode_and_binary_instead_of_lossy_decoding() {
        for invalid in [
            vec![0xc0, 0x80],
            vec![0xff],
            vec![0xef, 0xbb],
            vec![0xff, 0xfe, 0],
            vec![0xfe, 0xff, 0xd8, 0],
            vec![0xff, 0xfe, 0, 0xdc],
            vec![0xff, 0xfe, 0, 0xd8, 0x61, 0],
            vec![0xf4, 0x90, 0x80, 0x80],
            b"text\0binary".to_vec(),
            vec![b'a', 0x1b, b'b'],
        ] {
            for max in 1..=3 {
                let mut input = Chunked {
                    inner: Cursor::new(invalid.clone()),
                    max,
                };
                assert_eq!(
                    decode_content_text(&mut input, &Control),
                    Err(ValidationError::InvalidText)
                );
            }
        }
    }
    #[test]
    fn content_txt_rechecks_cancellation_while_streaming_and_rejects_cpu_exhaustion() {
        struct Stop(Cell<u32>, bool);
        impl ValidationControl for Stop {
            fn canceled(&self) -> bool {
                let n = self.0.get();
                self.0.set(n + 1);
                !self.1 && n >= 8
            }
            fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
                Ok(if self.1 && self.0.get() > 1 {
                    Duration::from_secs(10)
                } else {
                    Duration::ZERO
                })
            }
        }
        for (cpu, expected) in [
            (false, ValidationError::Canceled),
            (true, ValidationError::CpuBudget),
        ] {
            let mut input = Chunked {
                inner: Cursor::new(vec![b'a'; 500]),
                max: 1,
            };
            assert_eq!(
                decode_content_text(&mut input, &Stop(Cell::new(0), cpu)),
                Err(expected)
            );
            assert!(input.inner.position() < 500);
        }
    }
    #[test]
    fn hash_file_is_streaming_and_does_not_trigger_text_or_pdf_parsing() {
        let data = vec![0xff; BLOCK + 3];
        let mut input = Chunked {
            inner: Cursor::new(data.clone()),
            max: 7,
        };
        let (bytes, hash) = hash_file(&mut input, &Control).unwrap();
        assert_eq!(bytes, data.len() as u64);
        assert_eq!(hash, <Sha256 as Digest>::digest(data).as_slice());
    }
}
