use super::*;
use crate::recovery::test_support::{password, Fixture};

#[test]
fn captured_backup_rejects_revision_drift_and_lock_without_renewing_session() {
    let f = Fixture::new();
    let revision = f.revision();
    let access = f
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(f.lease, revision)
        .unwrap();
    f.backend
        .lock()
        .unwrap()
        .identity(
            f.lease,
            IdentityCommand::CreateClass {
                id: ClassId::new(1).unwrap(),
                name: "Synthetic".into(),
            },
        )
        .unwrap();
    assert!(matches!(
        access.check_revision(),
        Err(BackendError::Storage(StorageError::Conflict { .. }))
    ));
    let revision = f.revision();
    let access = f
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(f.lease, revision)
        .unwrap();
    f.backend.lock().unwrap().session_control().lock_local();
    assert!(access.check_revision().is_err());
    assert!(access
        .export_database(f.root.join("denied.sqlite3"), Arc::new(|| true))
        .is_err());
    assert!(!f.root.join("denied.sqlite3").exists());
}
#[test]
fn restore_review_digest_school_and_stale_lease_cannot_authorize_barrier() {
    let f = Fixture::new();
    let (source, manifest) = f.export();
    let revision = f.revision();
    assert!(matches!(
        f.backend.lock().unwrap().review_ordinary_restore(
            f.lease,
            revision,
            manifest.clone(),
            [0; 32],
            source.clone()
        ),
        Err(BackendError::InvalidInput)
    ));
    let mut other = manifest.clone();
    other.school_id = SchoolId::new(999).unwrap();
    let digest = fc_security::sha256(&fc_protocol::recovery::encode_manifest(&other).unwrap());
    assert!(matches!(
        f.backend.lock().unwrap().review_ordinary_restore(
            f.lease,
            revision,
            other,
            digest,
            source.clone()
        ),
        Err(BackendError::Storage(StorageError::RecoveryConflict))
    ));
    let digest = fc_security::sha256(&fc_protocol::recovery::encode_manifest(&manifest).unwrap());
    let review = f
        .backend
        .lock()
        .unwrap()
        .review_ordinary_restore(f.lease, revision, manifest, digest, source.clone())
        .unwrap();
    f.backend.lock().unwrap().session_control().lock_local();
    assert!(review.confirm(Arc::new(|| true)).is_err());
    assert_eq!(
        f.backend.lock().unwrap().global_revision().unwrap(),
        revision
    );
    assert!(!f.backend.lock().unwrap().session_control().is_recovering());
}
#[test]
fn canceled_barrier_leaves_live_data_and_authorization_floor_unchanged() {
    let f = Fixture::new();
    let (source, manifest) = f.export();
    let revision = f.revision();
    let state = f.backend.lock().unwrap().state().unwrap();
    let digest = fc_security::sha256(&fc_protocol::recovery::encode_manifest(&manifest).unwrap());
    let review = f
        .backend
        .lock()
        .unwrap()
        .review_ordinary_restore(f.lease, revision, manifest, digest, source.clone())
        .unwrap();
    assert!(matches!(
        review.confirm(Arc::new(|| false)),
        Err(BackendError::Storage(StorageError::RecoveryCancelled))
    ));
    assert_eq!(f.backend.lock().unwrap().state().unwrap(), state);
    assert_eq!(f.revision(), revision);
    assert_eq!(
        f.backend.lock().unwrap().role(f.lease).unwrap(),
        Role::GlobalAdmin
    );
}
#[test]
fn full_preflight_is_inert_and_its_exact_target_revision_cannot_be_reblessed_after_drift() {
    let f = Fixture::new();
    let (source, manifest) = f.export();
    let revision = f.revision();
    let before = f.backend.lock().unwrap().state().unwrap();
    let digest = fc_security::sha256(&fc_protocol::recovery::encode_manifest(&manifest).unwrap());
    let access = f
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(f.lease, revision)
        .unwrap();
    let preview = access
        .preflight_restore(
            source,
            f.root.join("preview.sqlite3"),
            manifest,
            digest,
            Arc::new(|| true),
        )
        .unwrap();
    assert_eq!(preview.current_revision(), revision);
    assert_eq!(f.revision(), revision);
    assert_eq!(f.backend.lock().unwrap().state().unwrap(), before);
    f.backend
        .lock()
        .unwrap()
        .identity(
            f.lease,
            IdentityCommand::CreateClass {
                id: ClassId::new(1).unwrap(),
                name: "Changed after preflight".into(),
            },
        )
        .unwrap();
    assert!(matches!(
        f.backend
            .lock()
            .unwrap()
            .review_preflighted_restore(f.lease, preview),
        Err(BackendError::Storage(StorageError::Conflict { .. }))
    ));
    assert!(!f.backend.lock().unwrap().session_control().is_recovering());
}
#[test]
fn same_numeric_school_and_principal_ids_do_not_allow_a_different_installation() {
    let f = Fixture::new();
    let (source, mut manifest) = f.export();
    let revision = f.revision();
    manifest.installation_id = [88; 32];
    let digest = fc_security::sha256(&fc_protocol::recovery::encode_manifest(&manifest).unwrap());
    let access = f
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(f.lease, revision)
        .unwrap();
    assert!(matches!(
        access.preflight_restore(
            source,
            f.root.join("foreign-preview.sqlite3"),
            manifest,
            digest,
            Arc::new(|| true)
        ),
        Err(BackendError::Storage(StorageError::RecoveryConflict))
    ));
    assert!(!f.root.join("foreign-preview.sqlite3").exists());
    assert_eq!(f.revision(), revision);
}
#[test]
fn full_preflight_binds_the_actual_database_bytes_not_only_its_revision_and_media() {
    let f = Fixture::new();
    let (source, mut manifest) = f.export();
    let revision = f.revision();
    if let BackupEntry::Database { sha256, .. } = &mut manifest.entries[0] {
        *sha256 = [55; 32];
    }
    let digest = fc_security::sha256(&fc_protocol::recovery::encode_manifest(&manifest).unwrap());
    let access = f
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(f.lease, revision)
        .unwrap();
    assert!(matches!(
        access.preflight_restore(
            source,
            f.root.join("mismatched-preview.sqlite3"),
            manifest,
            digest,
            Arc::new(|| true)
        ),
        Err(BackendError::Storage(StorageError::RecoveryInvalidPackage))
    ));
    assert_eq!(f.revision(), revision);
    assert!(!f.backend.lock().unwrap().session_control().is_recovering());
    assert_eq!(
        f.backend.lock().unwrap().role(f.lease).unwrap(),
        Role::GlobalAdmin
    );
}
#[test]
fn confirmed_candidate_blocks_new_login_keeps_current_password_and_advances_both_floors() {
    let f = Fixture::new();
    let old = f.backend.lock().unwrap().state().unwrap();
    let before = f.revision();
    let permit = f.prepare();
    let candidate = permit.prepared_database();
    let control = f.backend.lock().unwrap().session_control();
    assert!(control.is_recovering());
    assert!(control.is_local_locked());
    assert!(matches!(
        f.backend.lock().unwrap().login("admin", password()),
        Err(BackendError::Closed)
    ));
    assert!(f.backend.lock().unwrap().role(f.lease).is_err());
    assert!(candidate.receipt().revision > before);
    assert!(candidate.receipt().recovery_generation >= 2);
    let state = fc_storage::business::startup_state(candidate.candidate_database()).unwrap();
    assert!(state.identity.accounts[0].principal.auth_epoch > old.accounts[0].principal.auth_epoch);
    assert_eq!(
        state.identity.accounts[0].credential_version,
        old.accounts[0].credential_version
    );
    f.backend.lock().unwrap().close().unwrap();
    let current =
        fc_storage::business::startup_state(f.root.join("state/classroom.sqlite3")).unwrap();
    assert!(
        current.identity.accounts[0].principal.auth_epoch > old.accounts[0].principal.auth_epoch
    );
    assert!(
        state.identity.accounts[0].principal.auth_epoch
            >= current.identity.accounts[0].principal.auth_epoch
    );
    // Open with the existing protected-record key and actually verify the
    // current password. This is not a fake issued lease or old backup credential.
    struct ClockAgain;
    impl Clock for ClockAgain {
        fn now(&self) -> LocalDateTime {
            LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 10 * 3600).unwrap()
        }
    }
    impl MonotonicClock for ClockAgain {
        fn monotonic_millis(&self) -> u64 {
            2000
        }
    }
    let clock = Arc::new(ClockAgain);
    let mut reopened = Backend::open(
        candidate.candidate_database(),
        CredentialRecordCodec::from_key([119; 32]).unwrap(),
        clock.clone(),
        clock,
    )
    .unwrap();
    let login = reopened.login("admin", password()).unwrap();
    assert_eq!(reopened.role(login).unwrap(), Role::GlobalAdmin);
    reopened.close().unwrap();
}
#[test]
fn manifest_media_comparison_is_order_independent_and_includes_ownership_hash_and_namespace() {
    use fc_domain::{materials::MaterialKind, profile_content::ImageFormat};
    let material = BackupEntry::Material {
        teacher_id: PrincipalId::new(1).unwrap(),
        subject_id: SubjectId::new(2).unwrap(),
        material_id: MaterialId::new(3).unwrap(),
        version_id: ResourceVersionId::new(1).unwrap(),
        kind: MaterialKind::Txt,
        bytes: 3,
        sha256: [1; 32],
    };
    let image = BackupEntry::ProfileMedia {
        version_id: MediaVersionId::new(1).unwrap(),
        format: ImageFormat::Png,
        bytes: 4,
        sha256: [2; 32],
        width: 1,
        height: 1,
    };
    assert!(media_match(
        &[material.clone(), image.clone()],
        &[image.clone(), material.clone()]
    ));
    assert!(!media_match(std::slice::from_ref(&material), &[image]));
    let mut changed = material.clone();
    if let BackupEntry::Material { sha256, .. } = &mut changed {
        *sha256 = [3; 32];
    }
    assert!(!media_match(std::slice::from_ref(&material), &[changed]));
    assert!(!media_match(
        &[material.clone(), material.clone()],
        &[material.clone(), material]
    ));
}
