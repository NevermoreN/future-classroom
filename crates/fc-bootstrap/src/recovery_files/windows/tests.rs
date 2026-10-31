use super::*;
use fc_domain::{ids::*, recovery::BACKUP_FORMAT};
pub(super) struct Fixture {
    pub(super) paths: DataPaths,
    guard: Option<fc_windows::InstanceGuard>,
    files: Option<RecoveryFiles>,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-RecoveryFiles-{}",
            nonce().unwrap()
        ));
        let paths = DataPaths::isolated_test(root).unwrap();
        paths
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        let guard = fc_windows::InstanceGuard::acquire(paths.root()).unwrap();
        crate::profile_content_files::ControlledContentFiles::new(paths.root()).unwrap();
        let files = RecoveryFiles::new(&paths).unwrap();
        Self {
            paths,
            guard: Some(guard),
            files: Some(files),
        }
    }
    pub(super) fn f(&self) -> &RecoveryFiles {
        self.files.as_ref().unwrap()
    }
    pub(super) fn write(&self, path: &Path, bytes: &[u8]) {
        create_parent(&self.f().files, path).unwrap();
        write_new(&self.f().files, path, bytes).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.files.take();
        self.guard.take();
        std::fs::remove_dir_all(self.paths.root()).unwrap();
    }
}
pub(super) fn manifest(database: &[u8]) -> BackupManifest {
    BackupManifest {
        format: BACKUP_FORMAT,
        installation_id: [1; 32],
        school_id: SchoolId::new(1).unwrap(),
        revision: Revision::new(7),
        entries: vec![BackupEntry::Database {
            bytes: database.len() as u64,
            sha256: fc_security::sha256(database),
        }],
    }
}
#[test]
fn complete_package_roundtrip_pins_ordinary_and_removes_only_cancelled_work_area() {
    let f = Fixture::new();
    let workspace = f.f().workspace().unwrap();
    let database = b"synthetic framing database; SQLite validation belongs to storage";
    f.write(&workspace.ordinary_database(), database);
    let text = "合成课堂资料，安全备份。".as_bytes();
    let mut manifest = manifest(database);
    let entry = BackupEntry::Material {
        teacher_id: PrincipalId::new(2).unwrap(),
        subject_id: SubjectId::new(3).unwrap(),
        material_id: MaterialId::new(4).unwrap(),
        version_id: ResourceVersionId::new(5).unwrap(),
        kind: MaterialKind::Txt,
        bytes: text.len() as u64,
        sha256: fc_security::sha256(text),
    };
    f.write(&f.paths.root().join(entry_relative_path(&entry)), text);
    manifest.entries.push(entry);
    let receipt = f
        .f()
        .export_package(&manifest, &workspace.ordinary_database(), &|| true)
        .unwrap();
    let pin = LockedFile::open(&receipt.path, MAX_PACKAGE_BYTES).unwrap();
    let prepared = f.f().preflight_pinned(pin, &|| true).unwrap();
    assert_eq!(prepared.manifest(), &manifest);
    assert_eq!(prepared.manifest_sha256(), receipt.manifest_sha256);
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(prepared.ordinary_database())
        .is_err());
    let candidate_operation = prepared.workspace.operation.clone();
    assert!(candidate_operation.exists());
    drop(prepared);
    assert!(!candidate_operation.exists());
    assert!(receipt.path.exists());
    assert!(workspace.ordinary_database().exists());
    let operation = workspace.operation.clone();
    drop(workspace);
    assert!(!operation.exists());
}
#[test]
fn hash_valid_malformed_image_and_cancelled_package_never_leave_candidate_files() {
    let f = Fixture::new();
    let workspace = f.f().workspace().unwrap();
    let database = b"synthetic DB";
    f.write(&workspace.ordinary_database(), database);
    let mut manifest = manifest(database);
    let fake = b"not a decoded image";
    let entry = BackupEntry::ProfileMedia {
        version_id: MediaVersionId::new(9).unwrap(),
        format: ImageFormat::Png,
        bytes: fake.len() as u64,
        sha256: fc_security::sha256(fake),
        width: 1,
        height: 1,
    };
    f.write(&f.paths.root().join(entry_relative_path(&entry)), fake);
    manifest.entries.push(entry);
    let receipt = f
        .f()
        .export_package(&manifest, &workspace.ordinary_database(), &|| true)
        .unwrap();
    let before = f
        .f()
        .files
        .entries(&f.paths.root().join("recovery"), 34)
        .unwrap();
    assert!(matches!(
        f.f().preflight_pinned(
            LockedFile::open(&receipt.path, MAX_PACKAGE_BYTES).unwrap(),
            &|| true
        ),
        Err(RecoveryFileError::InvalidPackage)
    ));
    assert_eq!(
        f.f()
            .files
            .entries(&f.paths.root().join("recovery"), 34)
            .unwrap(),
        before
    );
    assert!(matches!(
        f.f().preflight_pinned(
            LockedFile::open(&receipt.path, MAX_PACKAGE_BYTES).unwrap(),
            &|| false
        ),
        Err(RecoveryFileError::Cancelled)
    ));
    assert_eq!(
        f.f()
            .files
            .entries(&f.paths.root().join("recovery"), 34)
            .unwrap(),
        before
    );
}
#[test]
fn directory_pin_prevents_cold_move_and_candidate_acl_is_protected() {
    let f = Fixture::new();
    let workspace = f.f().workspace().unwrap();
    f.paths
        .prepare_recovery_candidate(&workspace.new_root())
        .unwrap()
        .read_back()
        .unwrap();
    let db = f.paths.database();
    f.write(&db, b"old");
    let pin = LockedFile::open(&db, 3).unwrap();
    let old = workspace.operation.join("old");
    drop(f.f().files.create_directory_new(&old).unwrap());
    assert!(f
        .f()
        .files
        .rename_directory(&f.paths.root().join("state"), &old.join("state"))
        .is_err());
    drop(pin);
    f.f()
        .files
        .rename_directory(&f.paths.root().join("state"), &old.join("state"))
        .unwrap();
    f.f()
        .files
        .rename_directory(&old.join("state"), &f.paths.root().join("state"))
        .unwrap();
    assert_eq!(std::fs::read(db).unwrap(), b"old");
}

#[test]
fn cancelled_sealed_candidate_releases_its_pin_before_workspace_cleanup() {
    // This fixture isolates file ownership and cancellation. Storage/Backend
    // tests separately prove how a real reviewed preparation earns this type.
    for reject_digest in [false, true] {
        let f = Fixture::new();
        let source = f.f().workspace().unwrap();
        let database = b"synthetic ordinary framing bytes";
        f.write(&source.ordinary_database(), database);
        let manifest = manifest(database);
        let exported = f
            .f()
            .export_package(&manifest, &source.ordinary_database(), &|| true)
            .unwrap();
        let prepared = f
            .f()
            .preflight_pinned(
                LockedFile::open(&exported.path, MAX_PACKAGE_BYTES).unwrap(),
                &|| true,
            )
            .unwrap();
        let operation = prepared.workspace.operation.clone();
        let candidate_bytes = b"synthetic current-security candidate bytes";
        f.write(&prepared.candidate_database(), candidate_bytes);
        let mut expected_hash = fc_security::sha256(candidate_bytes);
        if reject_digest {
            expected_hash[0] ^= 1;
        }
        let proof = crate::backend::PreparedRecoveryDatabase {
            live_database: f.paths.database(),
            candidate_database: prepared.candidate_database(),
            manifest_sha256: prepared.manifest_sha256(),
            receipt: fc_storage::recovery::RecoveryDatabaseReceipt {
                current_revision: Revision::new(7),
                revision: Revision::new(8),
                source_revision: manifest.revision,
                school_id: manifest.school_id,
                installation_id: manifest.installation_id,
                recovery_generation: 2,
                document_count: 1,
                source_database_bytes: database.len() as u64,
                source_database_sha256: fc_security::sha256(database),
                database_bytes: candidate_bytes.len() as u64,
                database_sha256: expected_hash,
                media: vec![],
            },
        };
        let result = prepared.seal_candidate(&proof, &|| true);
        if reject_digest {
            assert!(matches!(result, Err(RecoveryFileError::InvalidPackage)));
        } else {
            let cold = result.unwrap();
            assert!(std::fs::OpenOptions::new()
                .write(true)
                .open(cold.workspace.candidate_database())
                .is_err());
            drop(cold);
        }
        assert!(
            !operation.exists(),
            "all cancelled candidate files must be removed"
        );
        assert!(exported.path.exists());
        assert!(source.ordinary_database().exists());
    }
}
