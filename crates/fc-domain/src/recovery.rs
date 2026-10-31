//! Complete ordinary backup descriptions. No credentials, grants or paths.
use crate::{ids::*, materials::*, profile_content::ImageFormat};
use std::collections::BTreeSet;

pub const BACKUP_FORMAT: u16 = 1;
pub const MAX_MEDIA_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Independent streaming database allowance, not the writer command budget.
/// Existing teaching history has no global retention bound; larger databases
/// must be rejected explicitly, never truncated to manufacture a valid backup.
pub const MAX_DATABASE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_MATERIAL_ENTRIES: usize = 8192;
pub const MAX_PROFILE_MEDIA_ENTRIES: usize = 8192;
pub const MAX_BACKUP_ENTRIES: usize = 1 + MAX_MATERIAL_ENTRIES + MAX_PROFILE_MEDIA_ENTRIES;
pub const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupEntry {
    Database {
        bytes: u64,
        sha256: [u8; 32],
    },
    Material {
        teacher_id: PrincipalId,
        subject_id: SubjectId,
        material_id: MaterialId,
        version_id: ResourceVersionId,
        kind: MaterialKind,
        bytes: u64,
        sha256: [u8; 32],
    },
    ProfileMedia {
        version_id: MediaVersionId,
        format: ImageFormat,
        bytes: u64,
        sha256: [u8; 32],
        width: u32,
        height: u32,
    },
}
impl BackupEntry {
    pub fn bytes(&self) -> u64 {
        match self {
            Self::Database { bytes, .. }
            | Self::Material { bytes, .. }
            | Self::ProfileMedia { bytes, .. } => *bytes,
        }
    }
    pub fn sha256(&self) -> [u8; 32] {
        match self {
            Self::Database { sha256, .. }
            | Self::Material { sha256, .. }
            | Self::ProfileMedia { sha256, .. } => *sha256,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupManifest {
    pub format: u16,
    /// Public, CSPRNG installation identity. It conveys no authorization.
    /// School and principal numeric IDs alone are not globally unique.
    pub installation_id: [u8; 32],
    pub school_id: SchoolId,
    pub revision: Revision,
    /// Database first, then complete immutable media objects in payload order.
    pub entries: Vec<BackupEntry>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupValidationError {
    UnsupportedFormat,
    InvalidInstallationId,
    InvalidRevision,
    InvalidDatabase,
    InvalidMedia,
    DuplicateResource,
    Capacity,
    Overflow,
}
impl BackupManifest {
    pub fn validate(&self) -> Result<(), BackupValidationError> {
        use BackupValidationError as E;
        if self.format != BACKUP_FORMAT {
            return Err(E::UnsupportedFormat);
        }
        if self.installation_id == [0; 32] {
            return Err(E::InvalidInstallationId);
        }
        if self.revision == Revision::default() {
            return Err(E::InvalidRevision);
        }
        if self.entries.is_empty() || !matches!(self.entries[0], BackupEntry::Database { .. }) {
            return Err(E::InvalidDatabase);
        }
        if self.entries.len() > MAX_BACKUP_ENTRIES {
            return Err(E::Capacity);
        }
        let mut materials = BTreeSet::new();
        let mut media = BTreeSet::new();
        let mut bytes = 0u64;
        for (index, entry) in self.entries.iter().enumerate() {
            match entry {
                BackupEntry::Database { bytes, .. } => {
                    if index != 0 || *bytes == 0 || *bytes > MAX_DATABASE_BYTES {
                        return Err(E::InvalidDatabase);
                    }
                }
                BackupEntry::Material {
                    version_id,
                    bytes: n,
                    kind,
                    ..
                } => {
                    if !materials.insert(*version_id) {
                        return Err(E::DuplicateResource);
                    }
                    if materials.len() > MAX_MATERIAL_ENTRIES {
                        return Err(E::Capacity);
                    }
                    MaterialResource {
                        version_id: *version_id,
                        bytes: *n,
                        kind: *kind,
                        sha256: entry.sha256(),
                    }
                    .validate()
                    .map_err(|_| E::InvalidMedia)?;
                    bytes = bytes.checked_add(*n).ok_or(E::Overflow)?;
                }
                BackupEntry::ProfileMedia {
                    version_id,
                    bytes: n,
                    width,
                    height,
                    ..
                } => {
                    if !media.insert(*version_id) {
                        return Err(E::DuplicateResource);
                    }
                    if media.len() > MAX_PROFILE_MEDIA_ENTRIES {
                        return Err(E::Capacity);
                    }
                    if *n == 0
                        || *n > crate::profile_content::MAX_CONTENT_IMAGE_BYTES
                        || *width == 0
                        || *height == 0
                        || u64::from(*width) * u64::from(*height)
                            > crate::profile_content::MAX_CONTENT_IMAGE_PIXELS
                    {
                        return Err(E::InvalidMedia);
                    }
                    bytes = bytes.checked_add(*n).ok_or(E::Overflow)?;
                }
            }
            if bytes > MAX_MEDIA_BYTES {
                return Err(E::Capacity);
            }
        }
        Ok(())
    }
    pub fn media_bytes(&self) -> Result<u64, BackupValidationError> {
        self.entries
            .iter()
            .filter(|entry| !matches!(entry, BackupEntry::Database { .. }))
            .try_fold(0u64, |total, entry| {
                total
                    .checked_add(entry.bytes())
                    .ok_or(BackupValidationError::Overflow)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> BackupManifest {
        BackupManifest {
            format: BACKUP_FORMAT,
            installation_id: [7; 32],
            school_id: SchoolId::new(1).unwrap(),
            revision: Revision::new(1),
            entries: vec![BackupEntry::Database {
                bytes: 1,
                sha256: [0; 32],
            }],
        }
    }
    fn material(id: u64, bytes: u64) -> BackupEntry {
        BackupEntry::Material {
            teacher_id: PrincipalId::new(1).unwrap(),
            subject_id: SubjectId::new(1).unwrap(),
            material_id: MaterialId::new(id).unwrap(),
            version_id: ResourceVersionId::new(id).unwrap(),
            kind: MaterialKind::Pdf,
            bytes,
            sha256: [0; 32],
        }
    }
    #[test]
    fn database_is_unique_first_and_separate_from_media_quota() {
        let mut m = manifest();
        m.entries[0] = BackupEntry::Database {
            bytes: MAX_DATABASE_BYTES,
            sha256: [0; 32],
        };
        assert_eq!(m.validate(), Ok(()));
        m.entries.push(material(1, MAX_MATERIAL_BYTES));
        assert_eq!(m.validate(), Ok(()));
        m.entries.push(m.entries[0].clone());
        assert_eq!(m.validate(), Err(BackupValidationError::InvalidDatabase));
        m.entries.swap(0, 1);
        assert_eq!(m.validate(), Err(BackupValidationError::InvalidDatabase));
    }
    #[test]
    fn installation_identity_cannot_be_an_absent_default() {
        let mut m = manifest();
        m.installation_id = [0; 32];
        assert_eq!(
            m.validate(),
            Err(BackupValidationError::InvalidInstallationId)
        );
        m.installation_id[31] = 1;
        assert_eq!(m.validate(), Ok(()));
    }
    #[test]
    fn resource_namespaces_are_distinct_but_versions_cannot_alias() {
        let mut m = manifest();
        m.entries.push(material(1, 1));
        m.entries.push(BackupEntry::ProfileMedia {
            version_id: MediaVersionId::new(1).unwrap(),
            format: ImageFormat::Png,
            bytes: 1,
            sha256: [0; 32],
            width: 1,
            height: 1,
        });
        assert_eq!(m.validate(), Ok(()));
        m.entries.push(material(1, 1));
        assert_eq!(m.validate(), Err(BackupValidationError::DuplicateResource));
    }
    #[test]
    fn media_total_and_image_dimension_boundaries() {
        let mut m = manifest();
        for n in 1..=40 {
            m.entries.push(material(n, MAX_MATERIAL_BYTES));
        }
        m.entries
            .push(material(41, MAX_MEDIA_BYTES - 40 * MAX_MATERIAL_BYTES));
        assert_eq!(m.validate(), Ok(()));
        m.entries.push(material(42, 1));
        assert_eq!(m.validate(), Err(BackupValidationError::Capacity));
        m.entries.truncate(1);
        m.entries.push(BackupEntry::ProfileMedia {
            version_id: MediaVersionId::new(1).unwrap(),
            format: ImageFormat::Png,
            bytes: 1,
            sha256: [0; 32],
            width: 4000,
            height: 2000,
        });
        assert_eq!(m.validate(), Ok(()));
        if let BackupEntry::ProfileMedia { height, .. } = &mut m.entries[1] {
            *height += 1;
        }
        assert_eq!(m.validate(), Err(BackupValidationError::InvalidMedia));
        if let BackupEntry::ProfileMedia { width, height, .. } = &mut m.entries[1] {
            *width = u32::MAX;
            *height = u32::MAX;
        }
        assert_eq!(m.validate(), Err(BackupValidationError::InvalidMedia));
    }
}
