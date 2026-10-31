use super::*;
use crate::recovery::test_support::Fixture;

fn access(f: &Fixture) -> OrdinaryBackupAccess {
    let revision = f.revision();
    f.backend
        .lock()
        .unwrap()
        .ordinary_backup_access(f.lease, revision)
        .unwrap()
}
fn hash(manifest: &BackupManifest) -> [u8; 32] {
    fc_security::sha256(&fc_protocol::recovery::encode_manifest(manifest).unwrap())
}
fn mapping(inspection: &MigrationInspection) -> MigrationMapping {
    let source = inspection.source_catalog();
    MigrationMapping {
        source_school_id: source.school_id,
        target_school_id: inspection
            .target_identity()
            .installation
            .as_ref()
            .unwrap()
            .school_id,
        classes: source
            .classes
            .iter()
            .map(|c| (c.id, ClassId::new(11).unwrap()))
            .collect(),
        subjects: vec![],
        principals: source
            .principals
            .iter()
            .map(|p| (p.id, PrincipalId::new(1).unwrap()))
            .collect(),
    }
}
#[test]
fn mapped_projection_is_inert_and_ordinary_guard_still_rejects_original_foreign_package() {
    let source = Fixture::new();
    let target = Fixture::new();
    source
        .backend
        .lock()
        .unwrap()
        .identity(
            source.lease,
            IdentityCommand::CreateClass {
                id: ClassId::new(1).unwrap(),
                name: "Source class".into(),
            },
        )
        .unwrap();
    target
        .backend
        .lock()
        .unwrap()
        .identity(
            target.lease,
            IdentityCommand::CreateClass {
                id: ClassId::new(11).unwrap(),
                name: "Target class preserved".into(),
            },
        )
        .unwrap();
    let (path, manifest) = source.export();
    let original = std::fs::read(&path).unwrap();
    let before = target.backend.lock().unwrap().state().unwrap();
    let revision = target.revision();
    let access = access(&target);
    assert!(matches!(
        access.preflight_restore(
            path.clone(),
            target.root.join("foreign-preview.sqlite3"),
            manifest.clone(),
            hash(&manifest),
            Arc::new(|| true)
        ),
        Err(BackendError::Storage(StorageError::RecoveryConflict))
    ));
    assert!(!target.root.join("foreign-preview.sqlite3").exists());
    let inspection = access
        .inspect_migration_source(
            path.clone(),
            manifest.clone(),
            hash(&manifest),
            Arc::new(|| true),
        )
        .unwrap();
    assert_ne!(inspection.target_installation(), manifest.installation_id);
    let mapped = target.root.join("mapped-ordinary.sqlite3");
    let receipt = access
        .map_migration_database(
            &inspection,
            mapping(&inspection),
            mapped.clone(),
            Arc::new(|| true),
        )
        .unwrap();
    let mut entries = vec![BackupEntry::Database {
        bytes: receipt.database_bytes(),
        sha256: receipt.database_sha256(),
    }];
    entries.extend_from_slice(receipt.media());
    let effective = BackupManifest {
        format: manifest.format,
        installation_id: receipt.installation_id(),
        school_id: receipt.school_id(),
        revision: receipt.revision(),
        entries,
    };
    let preview = access
        .preflight_restore(
            mapped,
            target.root.join("mapped-preview.sqlite3"),
            effective.clone(),
            hash(&effective),
            Arc::new(|| true),
        )
        .unwrap();
    assert_eq!(preview.current_revision(), revision);
    assert_eq!(target.revision(), revision);
    assert_eq!(target.backend.lock().unwrap().state().unwrap(), before);
    assert_eq!(
        target.backend.lock().unwrap().role(target.lease).unwrap(),
        Role::GlobalAdmin
    );
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn source_manifest_and_source_database_must_match_before_any_mapping_review() {
    let source = Fixture::new();
    let target = Fixture::new();
    let (path, manifest) = source.export();
    let access = access(&target);
    assert!(matches!(
        access.inspect_migration_source(path.clone(), manifest.clone(), [0; 32], Arc::new(|| true)),
        Err(BackendError::InvalidInput)
    ));
    let mut wrong = manifest.clone();
    wrong.installation_id = [77; 32];
    assert!(matches!(
        access.inspect_migration_source(
            path.clone(),
            wrong.clone(),
            hash(&wrong),
            Arc::new(|| true)
        ),
        Err(BackendError::Storage(StorageError::RecoveryConflict))
    ));
    wrong = manifest;
    if let BackupEntry::Database { bytes, .. } = &mut wrong.entries[0] {
        *bytes += 1;
    }
    assert!(matches!(
        access.inspect_migration_source(path, wrong.clone(), hash(&wrong), Arc::new(|| true)),
        Err(BackendError::Storage(StorageError::RecoveryConflict))
    ));
    assert!(!target
        .backend
        .lock()
        .unwrap()
        .session_control()
        .is_recovering());
}

#[test]
fn fresh_target_access_cannot_rebless_old_inspection_and_lock_closes_mapping_admission() {
    let source = Fixture::new();
    let target = Fixture::new();
    let (path, manifest) = source.export();
    let initial = access(&target);
    let inspection = initial
        .inspect_migration_source(path, manifest.clone(), hash(&manifest), Arc::new(|| true))
        .unwrap();
    target
        .backend
        .lock()
        .unwrap()
        .identity(
            target.lease,
            IdentityCommand::CreateClass {
                id: ClassId::new(11).unwrap(),
                name: "Changed target".into(),
            },
        )
        .unwrap();
    let fresh = access(&target);
    assert!(matches!(
        fresh.map_migration_database(
            &inspection,
            mapping(&inspection),
            target.root.join("stale.sqlite3"),
            Arc::new(|| true)
        ),
        Err(BackendError::Storage(StorageError::RecoveryConflict))
    ));
    assert!(!target.root.join("stale.sqlite3").exists());
    let inspection = fresh
        .inspect_migration_source(
            inspection.source_database.clone(),
            manifest.clone(),
            hash(&manifest),
            Arc::new(|| true),
        )
        .unwrap();
    target
        .backend
        .lock()
        .unwrap()
        .session_control()
        .lock_local();
    assert!(fresh
        .map_migration_database(
            &inspection,
            mapping(&inspection),
            target.root.join("locked.sqlite3"),
            Arc::new(|| true)
        )
        .is_err());
    assert!(!target.root.join("locked.sqlite3").exists());
}
