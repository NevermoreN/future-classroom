//! Versioned complete-backup framing. Bytes are uncompressed and paths absent.
//! Successful framing/hash verification is not WIC/PDF/Office validation. Every
//! reader destination is isolated staging until the whole package is accepted.
use fc_domain::{ids::*, materials::MaterialKind, profile_content::ImageFormat, recovery::*};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fmt,
    io::{Read, Write},
};

pub const STREAM_BLOCK_BYTES: usize = 32 * 1024;
pub const PACKAGE_MAGIC: &[u8; 16] = b"FCBACKUP-V1\r\n\x1a\n\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupFormatError {
    Io,
    InvalidJson,
    InvalidManifest(BackupValidationError),
    ManifestTooLarge,
    InvalidContainer,
    Incomplete,
    LengthMismatch,
    HashMismatch,
    TrailingData,
    Canceled,
}
impl fmt::Display for BackupFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BACKUP_{self:?}")
    }
}
impl std::error::Error for BackupFormatError {}
impl From<BackupValidationError> for BackupFormatError {
    fn from(error: BackupValidationError) -> Self {
        Self::InvalidManifest(error)
    }
}
type Result<T> = std::result::Result<T, BackupFormatError>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ManifestDto {
    format: u16,
    installation_id: String,
    school_id: u64,
    revision: u64,
    entries: Vec<EntryDto>,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "type",
    deny_unknown_fields,
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum EntryDto {
    Database {
        bytes: u64,
        sha256: String,
    },
    Material {
        teacher_id: u64,
        subject_id: u64,
        material_id: u64,
        version_id: u64,
        kind: String,
        bytes: u64,
        sha256: String,
    },
    ProfileMedia {
        version_id: u64,
        format: String,
        bytes: u64,
        sha256: String,
        width: u32,
        height: u32,
    },
}
fn hex(hash: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in hash {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}
fn hash(text: &str) -> Result<[u8; 32]> {
    fn nibble(c: u8) -> Result<u8> {
        match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            _ => Err(BackupFormatError::InvalidJson),
        }
    }
    if text.len() != 64 {
        return Err(BackupFormatError::InvalidJson);
    }
    let mut out = [0; 32];
    for (index, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        out[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Ok(out)
}
fn kind_name(kind: MaterialKind) -> &'static str {
    match kind {
        MaterialKind::Png => "png",
        MaterialKind::Jpeg => "jpeg",
        MaterialKind::Bmp => "bmp",
        MaterialKind::Txt => "txt",
        MaterialKind::Pdf => "pdf",
        MaterialKind::Pptx => "pptx",
        MaterialKind::Docx => "docx",
        MaterialKind::Xlsx => "xlsx",
    }
}
fn read_kind(kind: &str) -> Result<MaterialKind> {
    Ok(match kind {
        "png" => MaterialKind::Png,
        "jpeg" => MaterialKind::Jpeg,
        "bmp" => MaterialKind::Bmp,
        "txt" => MaterialKind::Txt,
        "pdf" => MaterialKind::Pdf,
        "pptx" => MaterialKind::Pptx,
        "docx" => MaterialKind::Docx,
        "xlsx" => MaterialKind::Xlsx,
        _ => return Err(BackupFormatError::InvalidJson),
    })
}
fn image_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Bmp => "bmp",
    }
}
fn read_image(format: &str) -> Result<ImageFormat> {
    match format {
        "png" => Ok(ImageFormat::Png),
        "jpeg" => Ok(ImageFormat::Jpeg),
        "bmp" => Ok(ImageFormat::Bmp),
        _ => Err(BackupFormatError::InvalidJson),
    }
}
fn entry_dto(entry: &BackupEntry) -> EntryDto {
    match entry {
        BackupEntry::Database { bytes, sha256 } => EntryDto::Database {
            bytes: *bytes,
            sha256: hex(sha256),
        },
        BackupEntry::Material {
            teacher_id,
            subject_id,
            material_id,
            version_id,
            kind,
            bytes,
            sha256,
        } => EntryDto::Material {
            teacher_id: teacher_id.get(),
            subject_id: subject_id.get(),
            material_id: material_id.get(),
            version_id: version_id.get(),
            kind: kind_name(*kind).into(),
            bytes: *bytes,
            sha256: hex(sha256),
        },
        BackupEntry::ProfileMedia {
            version_id,
            format,
            bytes,
            sha256,
            width,
            height,
        } => EntryDto::ProfileMedia {
            version_id: version_id.get(),
            format: image_name(*format).into(),
            bytes: *bytes,
            sha256: hex(sha256),
            width: *width,
            height: *height,
        },
    }
}
fn read_entry(entry: EntryDto) -> Result<BackupEntry> {
    let invalid = |_| BackupFormatError::InvalidJson;
    Ok(match entry {
        EntryDto::Database { bytes, sha256 } => BackupEntry::Database {
            bytes,
            sha256: hash(&sha256)?,
        },
        EntryDto::Material {
            teacher_id,
            subject_id,
            material_id,
            version_id,
            kind,
            bytes,
            sha256,
        } => BackupEntry::Material {
            teacher_id: PrincipalId::new(teacher_id).map_err(invalid)?,
            subject_id: SubjectId::new(subject_id).map_err(invalid)?,
            material_id: MaterialId::new(material_id).map_err(invalid)?,
            version_id: ResourceVersionId::new(version_id).map_err(invalid)?,
            kind: read_kind(&kind)?,
            bytes,
            sha256: hash(&sha256)?,
        },
        EntryDto::ProfileMedia {
            version_id,
            format,
            bytes,
            sha256,
            width,
            height,
        } => BackupEntry::ProfileMedia {
            version_id: MediaVersionId::new(version_id).map_err(invalid)?,
            format: read_image(&format)?,
            bytes,
            sha256: hash(&sha256)?,
            width,
            height,
        },
    })
}

/// Preflight retains duplicate (including escaped-equivalent) keys. Different
/// limits from ordinary HTTP JSON are intentional: 16,385 complete backup entries.
struct Bounded(u8);
impl<'de> DeserializeSeed<'de> for Bounded {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        if self.0 > 8 {
            return Err(de::Error::custom("backup depth"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Bounded {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded backup manifest")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        let mut count = 0;
        while seq.next_element_seed(Bounded(self.0 + 1))?.is_some() {
            count += 1;
            if count > MAX_BACKUP_ENTRIES {
                return Err(de::Error::custom("backup entry count"));
            }
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) || keys.len() > 16 {
                return Err(de::Error::custom("backup object keys"));
            }
            map.next_value_seed(Bounded(self.0 + 1))?;
        }
        Ok(())
    }
}
pub fn decode_manifest(bytes: &[u8]) -> Result<BackupManifest> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(BackupFormatError::ManifestTooLarge);
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    Bounded(0)
        .deserialize(&mut decoder)
        .map_err(|_| BackupFormatError::InvalidJson)?;
    decoder.end().map_err(|_| BackupFormatError::InvalidJson)?;
    let dto: ManifestDto =
        serde_json::from_slice(bytes).map_err(|_| BackupFormatError::InvalidJson)?;
    let manifest = BackupManifest {
        format: dto.format,
        installation_id: hash(&dto.installation_id)?,
        school_id: SchoolId::new(dto.school_id).map_err(|_| BackupFormatError::InvalidJson)?,
        revision: Revision::new(dto.revision),
        entries: dto
            .entries
            .into_iter()
            .map(read_entry)
            .collect::<Result<_>>()?,
    };
    manifest.validate()?;
    Ok(manifest)
}
pub fn encode_manifest(manifest: &BackupManifest) -> Result<Vec<u8>> {
    manifest.validate()?;
    let dto = ManifestDto {
        format: manifest.format,
        installation_id: hex(&manifest.installation_id),
        school_id: manifest.school_id.get(),
        revision: manifest.revision.get(),
        entries: manifest.entries.iter().map(entry_dto).collect(),
    };
    let bytes = serde_json::to_vec(&dto).map_err(|_| BackupFormatError::InvalidJson)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(BackupFormatError::ManifestTooLarge);
    }
    Ok(bytes)
}
fn active(check: &mut impl FnMut() -> bool) -> Result<()> {
    if check() {
        Ok(())
    } else {
        Err(BackupFormatError::Canceled)
    }
}
fn read_exact(
    reader: &mut impl Read,
    mut bytes: &mut [u8],
    check: &mut impl FnMut() -> bool,
) -> Result<()> {
    while !bytes.is_empty() {
        active(check)?;
        let count = bytes.len().min(STREAM_BLOCK_BYTES);
        match reader.read(&mut bytes[..count]) {
            Ok(0) => return Err(BackupFormatError::LengthMismatch),
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(BackupFormatError::Io),
        }
    }
    Ok(())
}
fn write_all(
    writer: &mut impl Write,
    mut bytes: &[u8],
    check: &mut impl FnMut() -> bool,
) -> Result<()> {
    while !bytes.is_empty() {
        active(check)?;
        let count = bytes.len().min(STREAM_BLOCK_BYTES);
        match writer.write(&bytes[..count]) {
            Ok(0) => return Err(BackupFormatError::Io),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(BackupFormatError::Io),
        }
    }
    Ok(())
}
fn eof(reader: &mut impl Read, check: &mut impl FnMut() -> bool) -> Result<()> {
    loop {
        active(check)?;
        match reader.read(&mut [0; 1]) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err(BackupFormatError::TrailingData),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(BackupFormatError::Io),
        }
    }
}
fn payload(
    reader: &mut impl Read,
    writer: &mut impl Write,
    entry: &BackupEntry,
    check: &mut impl FnMut() -> bool,
) -> Result<()> {
    let mut remaining = entry.bytes();
    let mut block = [0u8; STREAM_BLOCK_BYTES];
    let mut digest = Sha256::new();
    while remaining != 0 {
        let count = remaining.min(STREAM_BLOCK_BYTES as u64) as usize;
        read_exact(reader, &mut block[..count], check)?;
        digest.update(&block[..count]);
        write_all(writer, &block[..count], check)?;
        remaining -= count as u64;
    }
    active(check)?;
    if <[u8; 32]>::from(digest.finalize()) != entry.sha256() {
        return Err(BackupFormatError::HashMismatch);
    }
    Ok(())
}

pub struct PackageReader<R> {
    reader: R,
    manifest: BackupManifest,
    next: usize,
    poisoned: bool,
}
impl<R: Read> PackageReader<R> {
    pub fn new(reader: R) -> Result<Self> {
        Self::new_while(reader, || true)
    }
    pub fn new_while(mut reader: R, mut check: impl FnMut() -> bool) -> Result<Self> {
        let mut magic = [0; 16];
        read_exact(&mut reader, &mut magic, &mut check)?;
        if &magic != PACKAGE_MAGIC {
            return Err(BackupFormatError::InvalidContainer);
        }
        let mut length = [0; 4];
        read_exact(&mut reader, &mut length, &mut check)?;
        let length = u32::from_le_bytes(length) as usize;
        if length == 0 || length > MAX_MANIFEST_BYTES {
            return Err(BackupFormatError::ManifestTooLarge);
        }
        let mut bytes = vec![0; length];
        read_exact(&mut reader, &mut bytes, &mut check)?;
        let manifest = decode_manifest(&bytes)?;
        active(&mut check)?;
        Ok(Self {
            reader,
            manifest,
            next: 0,
            poisoned: false,
        })
    }
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }
    pub fn copy_next<W: Write>(&mut self, output: &mut W) -> Result<Option<BackupEntry>> {
        self.copy_next_while(output, || true)
    }
    /// Partial output remains untrusted on every error. The reader is poisoned
    /// after an interrupted entry; callers must discard the entire staged package.
    pub fn copy_next_while<W: Write>(
        &mut self,
        output: &mut W,
        mut check: impl FnMut() -> bool,
    ) -> Result<Option<BackupEntry>> {
        if self.poisoned {
            return Err(BackupFormatError::InvalidContainer);
        }
        let Some(entry) = self.manifest.entries.get(self.next).cloned() else {
            return Ok(None);
        };
        self.poisoned = true;
        payload(&mut self.reader, output, &entry, &mut check)?;
        self.next += 1;
        self.poisoned = false;
        Ok(Some(entry))
    }
    pub fn finish(self) -> Result<R> {
        self.finish_while(|| true)
    }
    pub fn finish_while(mut self, mut check: impl FnMut() -> bool) -> Result<R> {
        if self.poisoned {
            return Err(BackupFormatError::InvalidContainer);
        }
        if self.next != self.manifest.entries.len() {
            return Err(BackupFormatError::Incomplete);
        }
        eof(&mut self.reader, &mut check)?;
        Ok(self.reader)
    }
}
pub struct PackageWriter<W> {
    writer: W,
    manifest: BackupManifest,
    next: usize,
    poisoned: bool,
}
impl<W: Write> PackageWriter<W> {
    pub fn new(writer: W, manifest: &BackupManifest) -> Result<Self> {
        Self::new_while(writer, manifest, || true)
    }
    pub fn new_while(
        mut writer: W,
        manifest: &BackupManifest,
        mut check: impl FnMut() -> bool,
    ) -> Result<Self> {
        active(&mut check)?;
        let bytes = encode_manifest(manifest)?;
        active(&mut check)?;
        write_all(&mut writer, PACKAGE_MAGIC, &mut check)?;
        write_all(&mut writer, &(bytes.len() as u32).to_le_bytes(), &mut check)?;
        write_all(&mut writer, &bytes, &mut check)?;
        Ok(Self {
            writer,
            manifest: manifest.clone(),
            next: 0,
            poisoned: false,
        })
    }
    pub fn write_next<R: Read>(&mut self, input: &mut R) -> Result<BackupEntry> {
        self.write_next_while(input, || true)
    }
    /// Each input is one independently bounded resource, not a shared container
    /// stream: an extra source byte is rejected rather than silently truncated.
    pub fn write_next_while<R: Read>(
        &mut self,
        input: &mut R,
        mut check: impl FnMut() -> bool,
    ) -> Result<BackupEntry> {
        if self.poisoned {
            return Err(BackupFormatError::InvalidContainer);
        }
        self.poisoned = true;
        let entry = self
            .manifest
            .entries
            .get(self.next)
            .cloned()
            .ok_or(BackupFormatError::InvalidContainer)?;
        payload(input, &mut self.writer, &entry, &mut check)?;
        eof(input, &mut check)?;
        self.next += 1;
        self.poisoned = false;
        Ok(entry)
    }
    pub fn finish(self) -> Result<W> {
        self.finish_while(|| true)
    }
    pub fn finish_while(mut self, mut check: impl FnMut() -> bool) -> Result<W> {
        active(&mut check)?;
        if self.poisoned {
            return Err(BackupFormatError::InvalidContainer);
        }
        if self.next != self.manifest.entries.len() {
            return Err(BackupFormatError::Incomplete);
        }
        self.writer.flush().map_err(|_| BackupFormatError::Io)?;
        Ok(self.writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn digest(bytes: &[u8]) -> [u8; 32] {
        Sha256::digest(bytes).into()
    }
    fn manifest() -> BackupManifest {
        BackupManifest {
            format: BACKUP_FORMAT,
            installation_id: [7; 32],
            school_id: SchoolId::new(7).unwrap(),
            revision: Revision::new(9),
            entries: vec![BackupEntry::Database {
                bytes: 3,
                sha256: digest(b"db!"),
            }],
        }
    }
    fn package() -> Vec<u8> {
        let mut writer = PackageWriter::new(Vec::new(), &manifest()).unwrap();
        writer.write_next(&mut Cursor::new(b"db!")).unwrap();
        writer.finish().unwrap()
    }
    fn material(id: u64, bytes: &[u8]) -> BackupEntry {
        BackupEntry::Material {
            teacher_id: PrincipalId::new(1).unwrap(),
            subject_id: SubjectId::new(2).unwrap(),
            material_id: MaterialId::new(id).unwrap(),
            version_id: ResourceVersionId::new(id).unwrap(),
            kind: MaterialKind::Pdf,
            bytes: bytes.len() as u64,
            sha256: digest(bytes),
        }
    }
    #[test]
    fn manifest_roundtrip_and_strict_pathless_schema() {
        let mut m = manifest();
        m.entries.push(material(1, b"pdf"));
        m.entries.push(BackupEntry::ProfileMedia {
            version_id: MediaVersionId::new(1).unwrap(),
            format: ImageFormat::Png,
            bytes: 3,
            sha256: digest(b"png"),
            width: 2,
            height: 2,
        });
        let encoded = encode_manifest(&m).unwrap();
        assert_eq!(decode_manifest(&encoded).unwrap(), m);
        let text = String::from_utf8(encoded).unwrap();
        for bad in [
            text.replace("\"schoolId\":7", "\"schoolId\":7,\"schoolId\":7"),
            text.replace("\"schoolId\":7", "\"schoolId\":7,\"school\\u0049d\":7"),
            text.replace("\"width\":2", "\"width\":2,\"width\":2"),
            text.replace("\"format\":1", "\"format\":1,\"path\":\"C:\\\\secret\""),
            text.replace(
                "\"type\":\"database\"",
                "\"type\":\"database\",\"credential\":\"opaque\"",
            ),
            text.replace(
                "\"type\":\"material\"",
                "\"type\":\"material\",\"path\":\"../other\"",
            ),
            text.replace("\"revision\":9", "\"revision\":18446744073709551616"),
            text.replace("\"schoolId\":7", "\"schoolId\":0"),
            text.replace("\"width\":2", "\"width\":4294967296"),
            text.replace("\"bytes\":3", "\"bytes\":-1"),
            text.replace("\"format\":1", "\"format\":2"),
            text.replace("\"type\":\"material\"", "\"type\":\"executable\""),
            format!("{text} {{}}"),
        ] {
            assert!(decode_manifest(bad.as_bytes()).is_err(), "{bad}");
        }
        assert_eq!(
            decode_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1]),
            Err(BackupFormatError::ManifestTooLarge)
        );
    }
    #[test]
    fn canonical_hash_strings_and_required_database() {
        let bytes = encode_manifest(&manifest()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let original = hex(&digest(b"db!"));
        for bad_hash in [
            original[..63].to_owned(),
            format!("{original}0"),
            original.to_ascii_uppercase(),
            "g".repeat(64),
        ] {
            assert!(decode_manifest(text.replace(&original, &bad_hash).as_bytes()).is_err());
        }
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["entries"] = serde_json::json!([]);
        assert!(decode_manifest(&serde_json::to_vec(&value).unwrap()).is_err());
        let nested = format!("{}0{}", "[".repeat(10), "]".repeat(10));
        assert_eq!(
            decode_manifest(nested.as_bytes()),
            Err(BackupFormatError::InvalidJson)
        );
    }
    #[test]
    fn installation_identity_is_required_exact_nonzero_and_public() {
        let mut m = manifest();
        m.installation_id = [0xab; 32];
        let encoded = encode_manifest(&m).unwrap();
        assert_eq!(
            decode_manifest(&encoded).unwrap().installation_id,
            [0xab; 32]
        );
        let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        for invalid in [
            serde_json::Value::Null,
            serde_json::json!(7),
            serde_json::json!("ab"),
            serde_json::json!("AB".repeat(32)),
            serde_json::json!("00".repeat(32)),
            serde_json::json!("zz".repeat(32)),
        ] {
            let mut corrupt = value.clone();
            corrupt["installationId"] = invalid;
            assert!(decode_manifest(&serde_json::to_vec(&corrupt).unwrap()).is_err());
        }
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("installationId");
        assert!(decode_manifest(&serde_json::to_vec(&missing).unwrap()).is_err());
        let text = String::from_utf8(encoded).unwrap();
        let key = format!("\"installationId\":\"{}\"", "ab".repeat(32));
        let duplicate = text.replace(&key, &format!("{key},{key}"));
        assert_eq!(
            decode_manifest(duplicate.as_bytes()),
            Err(BackupFormatError::InvalidJson)
        );
        let escaped = text.replace(
            &key,
            &format!("{key},\"installation\\u0049d\":\"{}\"", "ab".repeat(32)),
        );
        assert_eq!(
            decode_manifest(escaped.as_bytes()),
            Err(BackupFormatError::InvalidJson)
        );
        let before = encode_manifest(&m).unwrap();
        m.installation_id = [0xcd; 32];
        assert_ne!(before, encode_manifest(&m).unwrap());
    }
    #[test]
    fn maximum_complete_entry_count_is_supported_without_http_limit() {
        let mut m = manifest();
        for id in 1..=MAX_MATERIAL_ENTRIES as u64 {
            m.entries.push(material(id, b"x"));
        }
        for id in 1..=MAX_PROFILE_MEDIA_ENTRIES as u64 {
            m.entries.push(BackupEntry::ProfileMedia {
                version_id: MediaVersionId::new(id).unwrap(),
                format: ImageFormat::Jpeg,
                bytes: 1,
                sha256: digest(b"x"),
                width: 1,
                height: 1,
            });
        }
        let encoded = encode_manifest(&m).unwrap();
        assert!(encoded.len() > crate::MAX_JSON_BYTES && encoded.len() < MAX_MANIFEST_BYTES);
        assert_eq!(decode_manifest(&encoded).unwrap(), m);
        let extra = entry_dto(&material(8193, b"x"));
        let mut value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        value["entries"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::to_value(extra).unwrap());
        assert!(decode_manifest(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    struct Chunks<R> {
        inner: R,
        reads: usize,
    }
    impl<R: Read> Read for Chunks<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            assert!(buf.len() <= STREAM_BLOCK_BYTES);
            self.reads += 1;
            let max = buf.len().min(719);
            self.inner.read(&mut buf[..max])
        }
    }
    struct ChunkSink {
        bytes: Vec<u8>,
        writes: usize,
    }
    impl Write for ChunkSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            assert!(buf.len() <= STREAM_BLOCK_BYTES);
            self.writes += 1;
            let n = buf.len().min(997);
            self.bytes.extend_from_slice(&buf[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn multiple_resources_stream_through_short_reads_and_writes() {
        let a = vec![0x5a; 3 * STREAM_BLOCK_BYTES + 19];
        let b = vec![0x31; 2 * STREAM_BLOCK_BYTES + 7];
        let mut m = manifest();
        m.entries.push(material(1, &a));
        m.entries.push(BackupEntry::ProfileMedia {
            version_id: MediaVersionId::new(1).unwrap(),
            format: ImageFormat::Bmp,
            bytes: b.len() as u64,
            sha256: digest(&b),
            width: 100,
            height: 100,
        });
        let mut writer = PackageWriter::new(
            ChunkSink {
                bytes: Vec::new(),
                writes: 0,
            },
            &m,
        )
        .unwrap();
        for resource in [b"db!".as_slice(), &a, &b] {
            let mut source = Chunks {
                inner: Cursor::new(resource),
                reads: 0,
            };
            writer.write_next(&mut source).unwrap();
            assert!(source.reads > 0);
        }
        let sink = writer.finish().unwrap();
        assert!(sink.writes > 150);
        let mut reader = PackageReader::new(Chunks {
            inner: Cursor::new(sink.bytes),
            reads: 0,
        })
        .unwrap();
        assert_eq!(reader.manifest(), &m);
        for (index, expected) in [b"db!".as_slice(), &a, &b].into_iter().enumerate() {
            let mut output = ChunkSink {
                bytes: Vec::new(),
                writes: 0,
            };
            assert_eq!(
                reader.copy_next(&mut output).unwrap(),
                Some(m.entries[index].clone())
            );
            assert_eq!(output.bytes, expected);
        }
        assert_eq!(reader.copy_next(&mut Vec::new()).unwrap(), None);
        assert!(reader.finish().unwrap().reads > 100);
    }
    #[test]
    fn corruption_truncation_trailing_bytes_and_poison_prevent_partial_success() {
        let original = package();
        for length in [0, 15, 19, original.len() - 1] {
            let result = PackageReader::new(Cursor::new(&original[..length]));
            if let Ok(mut reader) = result {
                assert!(reader.copy_next(&mut Vec::new()).is_err());
            }
        }
        let mut corrupt = original.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        let mut reader = PackageReader::new(Cursor::new(corrupt)).unwrap();
        let mut staged = Vec::new();
        assert_eq!(
            reader.copy_next(&mut staged),
            Err(BackupFormatError::HashMismatch)
        );
        assert!(!staged.is_empty());
        assert_eq!(
            reader.copy_next(&mut staged),
            Err(BackupFormatError::InvalidContainer)
        );
        assert!(matches!(
            reader.finish(),
            Err(BackupFormatError::InvalidContainer)
        ));
        let mut extra = original.clone();
        extra.push(0);
        let mut reader = PackageReader::new(Cursor::new(extra)).unwrap();
        reader.copy_next(&mut Vec::new()).unwrap();
        assert!(matches!(
            reader.finish(),
            Err(BackupFormatError::TrailingData)
        ));
        let reader = PackageReader::new(Cursor::new(original)).unwrap();
        assert!(matches!(
            reader.finish(),
            Err(BackupFormatError::Incomplete)
        ));
        let mut bad_magic = package();
        bad_magic[0] ^= 1;
        assert!(matches!(
            PackageReader::new(Cursor::new(bad_magic)),
            Err(BackupFormatError::InvalidContainer)
        ));
        let mut bad_length = PACKAGE_MAGIC.to_vec();
        bad_length.extend_from_slice(&((MAX_MANIFEST_BYTES + 1) as u32).to_le_bytes());
        assert!(matches!(
            PackageReader::new(Cursor::new(bad_length)),
            Err(BackupFormatError::ManifestTooLarge)
        ));
    }
    #[test]
    fn writer_rejects_wrong_length_hash_and_unfinished_package() {
        for (bytes, error) in [
            (b"db!extra".as_slice(), BackupFormatError::TrailingData),
            (b"db", BackupFormatError::LengthMismatch),
            (b"bad", BackupFormatError::HashMismatch),
        ] {
            let mut writer = PackageWriter::new(Vec::new(), &manifest()).unwrap();
            assert_eq!(writer.write_next(&mut Cursor::new(bytes)), Err(error));
            assert_eq!(
                writer.write_next(&mut Cursor::new(b"db!")),
                Err(BackupFormatError::InvalidContainer)
            );
            assert_eq!(writer.finish(), Err(BackupFormatError::InvalidContainer));
        }
        assert_eq!(
            PackageWriter::new(Vec::new(), &manifest())
                .unwrap()
                .finish(),
            Err(BackupFormatError::Incomplete)
        );
        let mut writer = PackageWriter::new(Vec::new(), &manifest()).unwrap();
        writer.write_next(&mut Cursor::new(b"db!")).unwrap();
        assert_eq!(
            writer.write_next(&mut Cursor::new(b"extra")),
            Err(BackupFormatError::InvalidContainer)
        );
        assert_eq!(writer.finish(), Err(BackupFormatError::InvalidContainer));
    }
    #[test]
    fn database_lengths_above_u32_are_stream_counters_not_allocations() {
        let mut m = manifest();
        m.entries = vec![BackupEntry::Database {
            bytes: MAX_DATABASE_BYTES,
            sha256: [0; 32],
        }];
        let mut writer = PackageWriter::new(std::io::sink(), &m).unwrap();
        let mut checks = 0;
        assert_eq!(
            writer.write_next_while(&mut std::io::repeat(0), || {
                checks += 1;
                checks <= 3
            }),
            Err(BackupFormatError::Canceled)
        );
        assert!(matches!(
            writer.finish(),
            Err(BackupFormatError::InvalidContainer)
        ));
        m.entries[0] = BackupEntry::Database {
            bytes: MAX_DATABASE_BYTES + 1,
            sha256: [0; 32],
        };
        assert!(matches!(
            PackageWriter::new(std::io::sink(), &m),
            Err(BackupFormatError::InvalidManifest(
                BackupValidationError::InvalidDatabase
            ))
        ));
    }
    #[test]
    fn per_block_cancellation_fences_both_headers_and_payloads() {
        assert!(matches!(
            PackageReader::new_while(Cursor::new(package()), || false),
            Err(BackupFormatError::Canceled)
        ));
        assert!(matches!(
            PackageWriter::new_while(Vec::new(), &manifest(), || false),
            Err(BackupFormatError::Canceled)
        ));
        let data = vec![7; 3 * STREAM_BLOCK_BYTES];
        let mut m = manifest();
        m.entries = vec![BackupEntry::Database {
            bytes: data.len() as u64,
            sha256: digest(&data),
        }];
        let mut writer = PackageWriter::new(Vec::new(), &m).unwrap();
        let mut checks = 0;
        assert_eq!(
            writer.write_next_while(&mut Cursor::new(&data), || {
                checks += 1;
                checks <= 3
            }),
            Err(BackupFormatError::Canceled)
        );
        assert_eq!(writer.finish(), Err(BackupFormatError::InvalidContainer));
        let mut writer = PackageWriter::new(Vec::new(), &m).unwrap();
        writer.write_next(&mut Cursor::new(&data)).unwrap();
        let mut reader = PackageReader::new(Cursor::new(writer.finish().unwrap())).unwrap();
        let mut checks = 0;
        assert_eq!(
            reader.copy_next_while(&mut Vec::new(), || {
                checks += 1;
                checks <= 3
            }),
            Err(BackupFormatError::Canceled)
        );
        assert!(matches!(
            reader.finish(),
            Err(BackupFormatError::InvalidContainer)
        ));
    }
    #[test]
    fn io_failures_and_interrupted_operations_do_not_forge_success() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::Other.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut reader = PackageReader::new(Cursor::new(package())).unwrap();
        assert_eq!(reader.copy_next(&mut Broken), Err(BackupFormatError::Io));
        assert!(matches!(
            reader.finish(),
            Err(BackupFormatError::InvalidContainer)
        ));
        struct Interrupted {
            inner: Cursor<Vec<u8>>,
            interrupted: bool,
        }
        impl Read for Interrupted {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                self.interrupted = !self.interrupted;
                if self.interrupted {
                    Err(std::io::ErrorKind::Interrupted.into())
                } else {
                    self.inner.read(b)
                }
            }
        }
        let mut reader = PackageReader::new(Interrupted {
            inner: Cursor::new(package()),
            interrupted: false,
        })
        .unwrap();
        let mut out = Vec::new();
        reader.copy_next(&mut out).unwrap();
        reader.finish().unwrap();
        assert_eq!(out, b"db!");
    }
}
