//! Complete ordinary backup files. Paths come only from typed, validated IDs;
//! Windows effects are isolated below and run on the existing bounded workers.
use fc_domain::{materials::MaterialKind, profile_content::ImageFormat, recovery::BackupEntry};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryFileError {
    InvalidPackage,
    Unavailable,
    Cancelled,
    RepairRequired,
}
impl std::fmt::Display for RecoveryFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RecoveryFileError {}
pub type Result<T> = std::result::Result<T, RecoveryFileError>;

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TreeLayout {
    Original,
    BetweenRenames,
    Installed,
}
/// A path observation is useful only together with exact old/new byte digests.
/// All other combinations are ambiguous and must preserve every tree.
#[cfg(any(windows, test))]
fn tree_layout(live: bool, old: bool, new: bool) -> Result<TreeLayout> {
    match (live, old, new) {
        (true, false, true) => Ok(TreeLayout::Original),
        (false, true, true) => Ok(TreeLayout::BetweenRenames),
        (true, true, false) => Ok(TreeLayout::Installed),
        _ => Err(RecoveryFileError::RepairRequired),
    }
}

pub fn material_extension(kind: MaterialKind) -> &'static str {
    match kind {
        MaterialKind::Png => "png",
        MaterialKind::Jpeg => "jpg",
        MaterialKind::Bmp => "bmp",
        MaterialKind::Txt => "txt",
        MaterialKind::Pdf => "pdf",
        MaterialKind::Pptx => "pptx",
        MaterialKind::Docx => "docx",
        MaterialKind::Xlsx => "xlsx",
    }
}
fn image_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Bmp => "bmp",
    }
}
/// No externally supplied component, separator or display name enters this path.
/// The database path is a package-relative name, not the live database source.
pub fn entry_relative_path(entry: &BackupEntry) -> PathBuf {
    match entry {
        BackupEntry::Database { .. } => PathBuf::from("state").join("classroom.sqlite3"),
        BackupEntry::Material {
            teacher_id,
            subject_id,
            material_id,
            version_id,
            kind,
            ..
        } => PathBuf::from("teachers")
            .join(teacher_id.get().to_string())
            .join("subjects")
            .join(subject_id.get().to_string())
            .join(material_id.get().to_string())
            .join(format!(
                "{}.{}",
                version_id.get(),
                material_extension(*kind)
            )),
        BackupEntry::ProfileMedia {
            version_id, format, ..
        } => PathBuf::from("profile-content")
            .join("originals")
            .join(format!(
                "{:020}.{}",
                version_id.get(),
                image_extension(*format)
            )),
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::ids::*;
    #[test]
    fn cold_switch_accepts_only_three_recoverable_locations() {
        let mut accepted = 0;
        for live in [false, true] {
            for old in [false, true] {
                for new in [false, true] {
                    let layout = tree_layout(live, old, new);
                    if let Ok(layout) = layout {
                        accepted += 1;
                        assert_eq!(
                            (live, old, new),
                            match layout {
                                TreeLayout::Original => (true, false, true),
                                TreeLayout::BetweenRenames => (false, true, true),
                                TreeLayout::Installed => (true, true, false),
                            }
                        );
                    } else {
                        assert_eq!(layout, Err(RecoveryFileError::RepairRequired));
                    }
                }
            }
        }
        assert_eq!(accepted, 3);
    }
    #[test]
    fn typed_paths_are_canonical_and_cannot_contain_teacher_text() {
        let entry = BackupEntry::Material {
            teacher_id: PrincipalId::new(7).unwrap(),
            subject_id: SubjectId::new(8).unwrap(),
            material_id: MaterialId::new(9).unwrap(),
            version_id: ResourceVersionId::new(10).unwrap(),
            kind: MaterialKind::Docx,
            bytes: 1,
            sha256: [1; 32],
        };
        assert_eq!(
            entry_relative_path(&entry),
            PathBuf::from("teachers")
                .join("7")
                .join("subjects")
                .join("8")
                .join("9")
                .join("10.docx")
        );
        let image = BackupEntry::ProfileMedia {
            version_id: MediaVersionId::new(17).unwrap(),
            format: ImageFormat::Png,
            bytes: 1,
            sha256: [1; 32],
            width: 1,
            height: 1,
        };
        assert_eq!(
            entry_relative_path(&image),
            PathBuf::from("profile-content")
                .join("originals")
                .join("00000000000000000017.png")
        );
        assert_eq!(
            entry_relative_path(&BackupEntry::Database {
                bytes: 1,
                sha256: [1; 32]
            }),
            PathBuf::from("state").join("classroom.sqlite3")
        );
    }
}
