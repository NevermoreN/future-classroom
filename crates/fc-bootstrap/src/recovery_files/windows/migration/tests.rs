use super::super::tests::{manifest, Fixture};
use super::*;
use fc_domain::{
    identity::Rgb,
    ids::*,
    recovery::{MigrationClassLabel, MigrationPrincipalLabel, MigrationSubjectLabel},
};

const SOURCE_DB: &[u8] =
    b"isolated source ordinary framing; typed SQL is validated by writer tests";
const MAPPED_DB: &[u8] = b"isolated newly mapped ordinary framing";
const TEXT: &[u8] = b"synthetic class material";

fn picture() -> Vec<u8> {
    let mut b = vec![0; 58];
    b[..2].copy_from_slice(b"BM");
    b[2..6].copy_from_slice(&58u32.to_le_bytes());
    b[10..14].copy_from_slice(&54u32.to_le_bytes());
    b[14..18].copy_from_slice(&40u32.to_le_bytes());
    b[18..22].copy_from_slice(&1u32.to_le_bytes());
    b[22..26].copy_from_slice(&1u32.to_le_bytes());
    b[26..28].copy_from_slice(&1u16.to_le_bytes());
    b[28..30].copy_from_slice(&24u16.to_le_bytes());
    b[34..38].copy_from_slice(&4u32.to_le_bytes());
    b[54] = 255;
    b
}
fn material() -> BackupEntry {
    BackupEntry::Material {
        teacher_id: PrincipalId::new(2).unwrap(),
        subject_id: SubjectId::new(3).unwrap(),
        material_id: MaterialId::new(4).unwrap(),
        version_id: ResourceVersionId::new(5).unwrap(),
        kind: MaterialKind::Txt,
        bytes: TEXT.len() as u64,
        sha256: fc_security::sha256(TEXT),
    }
}
fn source(f: &Fixture) -> PreparedRestore {
    let work = f.f().workspace().unwrap();
    f.write(&work.ordinary_database(), SOURCE_DB);
    let mut m = manifest(SOURCE_DB);
    let material = material();
    f.write(&f.paths.root().join(entry_relative_path(&material)), TEXT);
    m.entries.push(material);
    let bmp = picture();
    let image = BackupEntry::ProfileMedia {
        version_id: MediaVersionId::new(6).unwrap(),
        format: ImageFormat::Bmp,
        bytes: bmp.len() as u64,
        sha256: fc_security::sha256(&bmp),
        width: 1,
        height: 1,
    };
    f.write(&f.paths.root().join(entry_relative_path(&image)), &bmp);
    m.entries.push(image);
    let export = f
        .f()
        .export_package(&m, &work.ordinary_database(), &|| true)
        .unwrap();
    f.f()
        .preflight_pinned(
            LockedFile::open(&export.path, MAX_PACKAGE_BYTES).unwrap(),
            &|| true,
        )
        .unwrap()
}
fn mapping() -> MigrationMapping {
    MigrationMapping {
        source_school_id: SchoolId::new(1).unwrap(),
        target_school_id: SchoolId::new(9).unwrap(),
        classes: vec![(ClassId::new(1).unwrap(), ClassId::new(11).unwrap())],
        subjects: vec![(SubjectId::new(3).unwrap(), SubjectId::new(33).unwrap())],
        principals: vec![(PrincipalId::new(2).unwrap(), PrincipalId::new(22).unwrap())],
    }
}
fn binding() -> SourceBinding {
    SourceBinding {
        current_revision: Revision::new(12),
        source_revision: Revision::new(7),
        school_id: SchoolId::new(1).unwrap(),
        installation_id: [1; 32],
        database_bytes: SOURCE_DB.len() as u64,
        database_sha256: fc_security::sha256(SOURCE_DB),
        catalog_sha256: [5; 32],
        catalog: MigrationCatalog {
            school_id: SchoolId::new(1).unwrap(),
            school_name: "源学校".into(),
            current_class: Some(ClassId::new(1).unwrap()),
            classes: vec![MigrationClassLabel {
                id: ClassId::new(1).unwrap(),
                name: "源班级".into(),
            }],
            subjects: vec![MigrationSubjectLabel {
                id: SubjectId::new(3).unwrap(),
                name: "源科目".into(),
                color: Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
            }],
            principals: vec![MigrationPrincipalLabel {
                id: PrincipalId::new(2).unwrap(),
                display_name: "源教师".into(),
            }],
            default_teachers: vec![],
        },
    }
}
fn stage(f: &Fixture) -> MappingStage {
    source(f)
        .prepare_mapping_bound(binding(), mapping(), &|| true)
        .unwrap()
}
fn mapped_media(stage: &MappingStage) -> Vec<BackupEntry> {
    stage.source.manifest.entries[1..]
        .iter()
        .map(|e| mapped_entry(e, &stage.mapping).unwrap())
        .collect()
}
fn receipt<'a>(stage: &MappingStage, media: &'a [BackupEntry]) -> MappedBinding<'a> {
    MappedBinding {
        source: stage.expected.clone(),
        mapping_sha256: stage.mapping_sha256,
        current_revision: stage.expected.current_revision,
        school_id: stage.mapping.target_school_id,
        installation_id: [9; 32],
        revision: stage.expected.source_revision,
        database_bytes: MAPPED_DB.len() as u64,
        database_sha256: fc_security::sha256(MAPPED_DB),
        media,
    }
}

#[test]
fn mapped_files_use_target_typed_directories_and_keep_every_source_byte_pinned() {
    let f = Fixture::new();
    let stage = stage(&f);
    let source_db = stage.source_database().to_owned();
    let source_material = stage
        .source
        .workspace
        .new_root()
        .join(entry_relative_path(&material()));
    let original_manifest = stage.source_manifest().clone();
    let original_sha = stage.source_manifest_sha256();
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(&source_material)
        .is_err());
    f.write(&stage.mapped_database_destination(), MAPPED_DB);
    let media = mapped_media(&stage);
    let proof = receipt(&stage, &media);
    let mapped = stage.finish_bound(proof, &|| true).unwrap();
    assert_eq!(mapped.source_manifest(), &original_manifest);
    assert_eq!(mapped.source_manifest_sha256(), original_sha);
    assert_eq!(std::fs::read(&source_db).unwrap(), SOURCE_DB);
    assert_eq!(std::fs::read(&source_material).unwrap(), TEXT);
    assert_eq!(mapped.manifest().installation_id, [9; 32]);
    assert_eq!(mapped.manifest().school_id, SchoolId::new(9).unwrap());
    assert_eq!(mapped.manifest().revision, original_manifest.revision);
    assert_eq!(mapped.manifest().entries[1..], media);
    let target = mapped.mapped.workspace.new_root();
    let remapped = target.join(entry_relative_path(&media[0]));
    assert_eq!(std::fs::read(&remapped).unwrap(), TEXT);
    assert!(remapped.ends_with("teachers/22/subjects/33/4/5.txt"));
    assert!(!target.join(entry_relative_path(&material())).exists());
    assert_eq!(
        std::fs::read(target.join(entry_relative_path(&media[1]))).unwrap(),
        picture()
    );
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(&remapped)
        .is_err());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(mapped.ordinary_database())
        .is_err());
    let source_operation = mapped.source.workspace.operation.clone();
    let mapped_operation = mapped.mapped.workspace.operation.clone();
    mapped.discard(&|| true).unwrap();
    assert!(!source_operation.exists());
    assert!(!mapped_operation.exists());
    assert_eq!(
        std::fs::read(f.paths.root().join(entry_relative_path(&material()))).unwrap(),
        TEXT
    );
}

#[test]
fn incomplete_duplicate_or_wrong_source_mapping_never_allocates_an_operation() {
    for fault in 0..5 {
        let f = Fixture::new();
        let source = source(&f);
        let original = source.workspace.operation.clone();
        let mut expected = binding();
        let mut map = mapping();
        match fault {
            0 => {
                map.principals.clear();
            }
            1 => {
                map.subjects.push(map.subjects[0]);
            }
            2 => {
                map.classes[0].0 = ClassId::new(99).unwrap();
            }
            3 => {
                expected.database_sha256[0] ^= 1;
            }
            _ => {
                expected.installation_id[0] ^= 1;
            }
        }
        assert!(matches!(
            source.prepare_mapping_bound(expected, map, &|| true),
            Err(RecoveryFileError::InvalidPackage)
        ));
        assert!(!original.exists());
        assert_eq!(
            f.f()
                .files
                .entries(&f.paths.root().join("recovery"), 34)
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn mismatched_writer_binding_or_database_digest_retires_both_owned_workspaces() {
    // These private file-layer proof views cannot be constructed by production
    // callers. Storage tests independently exercise the actual opaque receipts.
    for fault in 0..10 {
        let f = Fixture::new();
        let stage = stage(&f);
        let source = stage.source.workspace.operation.clone();
        let target = stage.target.operation.clone();
        f.write(&stage.mapped_database_destination(), MAPPED_DB);
        let mut media = mapped_media(&stage);
        if fault == 8 {
            media.pop();
        }
        if fault == 9 {
            if let BackupEntry::Material { teacher_id, .. } = &mut media[0] {
                *teacher_id = PrincipalId::new(99).unwrap();
            }
        }
        let mut proof = receipt(&stage, &media);
        match fault {
            0 => {
                proof.source.database_sha256[0] ^= 1;
            }
            1 => {
                proof.mapping_sha256[0] ^= 1;
            }
            2 => {
                proof.current_revision = Revision::new(13);
            }
            3 => {
                proof.school_id = SchoolId::new(99).unwrap();
            }
            4 => {
                proof.installation_id = [0; 32];
            }
            5 => {
                proof.revision = Revision::new(8);
            }
            6 => {
                proof.database_bytes += 1;
            }
            7 => {
                proof.database_sha256[0] ^= 1;
            }
            _ => {}
        }
        assert!(matches!(
            stage.finish_bound(proof, &|| true),
            Err(RecoveryFileError::InvalidPackage)
        ));
        assert!(!source.exists(), "source fault {fault}");
        assert!(!target.exists(), "target fault {fault}");
        assert_eq!(
            std::fs::read(f.paths.root().join(entry_relative_path(&material()))).unwrap(),
            TEXT
        );
    }
}

#[test]
fn cancel_after_first_media_copy_retires_both_workspaces_on_the_file_worker() {
    let f = Fixture::new();
    let stage = stage(&f);
    let source = stage.source.workspace.operation.clone();
    let target = stage.target.operation.clone();
    f.write(&stage.mapped_database_destination(), MAPPED_DB);
    let media = mapped_media(&stage);
    let first = stage.target.new_root().join(entry_relative_path(&media[0]));
    let proof = receipt(&stage, &media);
    assert!(matches!(
        stage.finish_bound(proof, &|| !first.exists()),
        Err(RecoveryFileError::Cancelled)
    ));
    assert!(!source.exists());
    assert!(!target.exists());
    assert_eq!(
        std::fs::read(f.paths.root().join(entry_relative_path(&material()))).unwrap(),
        TEXT
    );
}

#[test]
fn preexisting_target_file_is_never_adopted_or_overwritten() {
    let f = Fixture::new();
    let stage = stage(&f);
    f.write(&stage.mapped_database_destination(), MAPPED_DB);
    let media = mapped_media(&stage);
    let target = stage.target.new_root().join(entry_relative_path(&media[0]));
    f.write(&target, b"unrelated injected candidate bytes");
    let proof = receipt(&stage, &media);
    assert!(matches!(
        stage.finish_bound(proof, &|| true),
        Err(RecoveryFileError::Unavailable)
    ));
    assert_eq!(
        std::fs::read(f.paths.root().join(entry_relative_path(&material()))).unwrap(),
        TEXT
    );
}

#[test]
fn sealing_retires_source_then_keeps_mapped_media_pinned_until_cold_retirement() {
    for reject_candidate in [false, true] {
        let f = Fixture::new();
        let stage = stage(&f);
        let source_operation = stage.source.workspace.operation.clone();
        let mapped_operation = stage.target.operation.clone();
        let mapping_sha256 = stage.mapping_sha256();
        f.write(&stage.mapped_database_destination(), MAPPED_DB);
        let media = mapped_media(&stage);
        let proof = receipt(&stage, &media);
        let mapped = stage.finish_bound(proof, &|| true).unwrap();
        assert_eq!(mapped.mapping_sha256(), mapping_sha256);
        let candidate = b"isolated current-security candidate";
        f.write(&mapped.candidate_database(), candidate);
        let mut candidate_sha256 = fc_security::sha256(candidate);
        if reject_candidate {
            candidate_sha256[0] ^= 1;
        }
        let proof = crate::backend::PreparedRecoveryDatabase {
            live_database: f.paths.database(),
            candidate_database: mapped.candidate_database(),
            manifest_sha256: mapped.manifest_sha256(),
            receipt: fc_storage::recovery::RecoveryDatabaseReceipt {
                current_revision: Revision::new(12),
                revision: Revision::new(13),
                source_revision: Revision::new(7),
                school_id: SchoolId::new(9).unwrap(),
                installation_id: [9; 32],
                recovery_generation: 2,
                document_count: 1,
                database_bytes: candidate.len() as u64,
                database_sha256: candidate_sha256,
                source_database_bytes: MAPPED_DB.len() as u64,
                source_database_sha256: fc_security::sha256(MAPPED_DB),
                media: media.clone(),
            },
        };
        let result = mapped.seal_candidate(&proof, &|| true);
        if reject_candidate {
            assert!(matches!(result, Err(RecoveryFileError::InvalidPackage)));
        } else {
            let cold = result.unwrap();
            assert!(!source_operation.exists());
            assert!(mapped_operation.exists());
            let path = cold
                .workspace
                .new_root()
                .join(entry_relative_path(&media[0]));
            assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
            assert!(cold
                .workspace
                .files
                .rename_directory(
                    &cold.workspace.new_root().join("teachers"),
                    &cold.workspace.operation.join("blocked-move")
                )
                .is_err());
            drop(cold);
        }
        assert!(!source_operation.exists());
        assert!(!mapped_operation.exists());
    }
}
