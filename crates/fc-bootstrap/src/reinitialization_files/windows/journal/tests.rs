use super::super::tests::Fixture;
use super::*;
use std::fs;
fn moved(prepared: &PreparedReinitialization, meta: &Metadata, count: usize, receipts: bool) {
    for step in 0..count {
        move_one(&prepared.service, &prepared.operation, meta, step).unwrap();
        if receipts {
            phase(
                &prepared.service.files,
                &prepared.operation,
                meta.owner,
                step as u8 + 1,
            )
            .unwrap();
        }
    }
}
fn committed(fixture: &Fixture) -> (PreparedReinitialization, Metadata) {
    let prepared = fixture.prepared();
    let meta = arm(&prepared, &|| true).unwrap();
    moved(&prepared, &meta, OWNED_TREES.len() * 2, true);
    verify_installed(&prepared.service, &prepared.operation, &meta, &|| true).unwrap();
    phase(
        &prepared.service.files,
        &prepared.operation,
        meta.owner,
        COMMITTED,
    )
    .unwrap();
    (prepared, meta)
}
#[test]
fn every_rename_cut_with_or_without_receipt_rolls_back_all_thirteen_trees() {
    for receipts in [false, true] {
        for count in 0..=OWNED_TREES.len() * 2 {
            let fixture = Fixture::new();
            let prepared = fixture.prepared();
            let meta = arm(&prepared, &|| true).unwrap();
            moved(&prepared, &meta, count, receipts);
            let operation = prepared.operation.clone();
            drop(prepared);
            assert_eq!(
                fixture
                    .files()
                    .reconcile_startup(&|| true)
                    .unwrap()
                    .rolled_back,
                1,
                "cut {count} receipts {receipts}"
            );
            for name in OWNED_TREES {
                assert_eq!(
                    fs::read(fixture.paths.root().join(name).join("own.bin")).unwrap(),
                    b"old opaque bytes"
                );
            }
            assert_eq!(fs::read(fixture.paths.database()).unwrap(), b"");
            assert_eq!(
                fs::read(
                    fixture
                        .paths
                        .root()
                        .join("config")
                        .join("credential-key.dpapi")
                )
                .unwrap(),
                b""
            );
            assert!(!operation.exists());
            fixture.assert_runtime();
            assert_eq!(
                fixture.files().reconcile_startup(&|| true).unwrap(),
                ResetStartup::default()
            );
        }
    }
}
#[test]
fn originally_missing_trees_are_restored_to_absence_at_every_cut() {
    for count in 0..=OWNED_TREES.len() * 2 {
        let fixture = Fixture::new();
        for n in (0..OWNED_TREES.len()).step_by(2) {
            fs::remove_dir_all(fixture.paths.root().join(OWNED_TREES[n])).unwrap();
        }
        let prepared = fixture.prepared();
        let meta = arm(&prepared, &|| true).unwrap();
        moved(&prepared, &meta, count, false);
        drop(prepared);
        fixture.files().reconcile_startup(&|| true).unwrap();
        for (n, name) in OWNED_TREES.iter().enumerate() {
            assert_eq!(
                fixture.paths.root().join(name).exists(),
                n % 2 != 0,
                "cut {count} tree {name}"
            );
        }
        fixture.assert_runtime();
    }
}
#[test]
fn rollback_itself_is_reentrant_after_each_reverse_move() {
    for reverse_moves in 0..=OWNED_TREES.len() * 2 {
        let fixture = Fixture::new();
        let prepared = fixture.prepared();
        let meta = arm(&prepared, &|| true).unwrap();
        moved(&prepared, &meta, OWNED_TREES.len() * 2, true);
        let mut done = 0;
        for n in (0..OWNED_TREES.len()).rev() {
            if done == reverse_moves {
                break;
            }
            let name = OWNED_TREES[n];
            move_exact(
                &prepared.service,
                &fixture.paths.root().join(name),
                &prepared.operation.join("new").join(name),
                meta.new[n],
            )
            .unwrap();
            done += 1;
            if done == reverse_moves {
                break;
            }
            move_exact(
                &prepared.service,
                &prepared.operation.join("old").join(name),
                &fixture.paths.root().join(name),
                meta.old[n].unwrap(),
            )
            .unwrap();
            done += 1;
        }
        drop(prepared);
        fixture.files().reconcile_startup(&|| true).unwrap();
        for name in OWNED_TREES {
            assert_eq!(
                fs::read(fixture.paths.root().join(name).join("own.bin")).unwrap(),
                b"old opaque bytes"
            );
        }
        fixture.assert_runtime();
    }
}
#[test]
fn commit_removes_old_accounts_audio_exports_keys_and_does_not_reject_later_fresh_writes() {
    let fixture = Fixture::new();
    let (prepared, _) = committed(&fixture);
    let operation = prepared.operation.clone();
    drop(prepared);
    assert_eq!(
        fixture
            .files()
            .reconcile_startup(&|| true)
            .unwrap()
            .installed,
        1
    );
    for name in OWNED_TREES {
        assert_eq!(
            fs::read_dir(fixture.paths.root().join(name))
                .unwrap()
                .count(),
            0
        );
    }
    assert!(!operation.exists());
    fixture.assert_runtime();
    fixture.write(
        &fixture
            .paths
            .root()
            .join("config")
            .join("new-key-placeholder"),
        b"subsequent fresh deployment write",
    );
    assert_eq!(
        fixture.files().reconcile_startup(&|| true).unwrap(),
        ResetStartup::default()
    );
}
#[test]
fn cleanup_resumes_after_some_exact_recorded_objects_have_already_disappeared() {
    let fixture = Fixture::new();
    let (prepared, meta) = committed(&fixture);
    write_new(
        &prepared.service.files,
        &prepared.operation.join("cleanup"),
        &phase_bytes(meta.owner, CLEAN_INSTALLED),
    )
    .unwrap();
    let manifest = read_manifest(&prepared.operation, &meta, 0)
        .unwrap()
        .unwrap();
    let first = manifest.records.iter().find(|r| !r.directory).unwrap();
    let old = prepared.operation.join("old").join(OWNED_TREES[0]);
    prepared
        .service
        .files
        .remove_object_expected(&old.join(&first.relative), false, first.identity, |_| true)
        .unwrap();
    let operation = prepared.operation.clone();
    drop(prepared);
    fixture.files().reconcile_startup(&|| true).unwrap();
    assert!(!operation.exists());
    fixture.assert_runtime();
}
#[test]
fn cleanup_missing_or_corrupt_manifest_preserves_every_remaining_old_object() {
    for corrupt in [false, true] {
        let fixture = Fixture::new();
        let (prepared, meta) = committed(&fixture);
        write_new(
            &prepared.service.files,
            &prepared.operation.join("cleanup"),
            &phase_bytes(meta.owner, CLEAN_INSTALLED),
        )
        .unwrap();
        let manifest = manifest_path(&prepared.operation, 0);
        prepared.service.files.remove(&manifest).unwrap();
        if corrupt {
            fixture.write(&manifest, b"damaged manifest");
        }
        let operation = prepared.operation.clone();
        drop(prepared);
        assert_eq!(
            fixture.files().reconcile_startup(&|| true),
            Err(ReinitializationFileError::RepairRequired)
        );
        assert!(operation
            .join("old")
            .join("state")
            .join("classroom.sqlite3")
            .exists());
        fixture.assert_runtime();
    }
}
#[test]
fn cleanup_rejects_new_or_replaced_old_objects_instead_of_deleting_them() {
    for replace in [false, true] {
        let fixture = Fixture::new();
        let (prepared, meta) = committed(&fixture);
        write_new(
            &prepared.service.files,
            &prepared.operation.join("cleanup"),
            &phase_bytes(meta.owner, CLEAN_INSTALLED),
        )
        .unwrap();
        let path = prepared
            .operation
            .join("old")
            .join("state")
            .join(if replace { "own.bin" } else { "unexpected.bin" });
        if replace {
            prepared.service.files.remove(&path).unwrap();
        }
        fixture.write(&path, b"new unexpected object");
        let operation = prepared.operation.clone();
        drop(prepared);
        assert_eq!(
            fixture.files().reconcile_startup(&|| true),
            Err(ReinitializationFileError::RepairRequired)
        );
        assert_eq!(fs::read(path).unwrap(), b"new unexpected object");
        assert!(operation
            .join("old")
            .join("state")
            .join("classroom.sqlite3")
            .exists());
        fixture.assert_runtime();
    }
}
#[test]
fn damaged_or_missing_journal_and_ambiguous_layout_preserve_both_sets() {
    for fault in 0..3 {
        let fixture = Fixture::new();
        let prepared = fixture.prepared();
        let meta = arm(&prepared, &|| true).unwrap();
        moved(&prepared, &meta, 1, true);
        if fault < 2 {
            prepared
                .service
                .files
                .remove(&prepared.operation.join("journal"))
                .unwrap();
            if fault == 0 {
                fixture.write(&prepared.operation.join("journal"), b"corrupt");
            }
        } else {
            fs::create_dir(fixture.paths.root().join("state")).unwrap();
        }
        let operation = prepared.operation.clone();
        drop(prepared);
        assert!(fixture.files().reconcile_startup(&|| true).is_err());
        assert!(operation.join("old").join("state").exists());
        assert!(operation.join("new").join("state").exists());
        fixture.assert_runtime();
    }
}
#[test]
fn cancellation_inside_rollback_preserves_recoverable_journal_for_next_start() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let meta = arm(&prepared, &|| true).unwrap();
    moved(&prepared, &meta, 26, true);
    drop(prepared);
    let result = fixture.files().reconcile_startup(&|| {
        !fixture
            .paths
            .root()
            .join("recovery")
            .join("own.bin")
            .exists()
    });
    assert_eq!(result, Err(ReinitializationFileError::Cancelled));
    fixture.files().reconcile_startup(&|| true).unwrap();
    fixture.assert_runtime();
}

#[test]
fn metadata_cleanup_restarts_after_journal_and_owner_are_removed_without_touching_fresh_writes() {
    let fixture = Fixture::new();
    let (mut prepared, meta) = committed(&fixture);
    write_new(
        &prepared.service.files,
        &prepared.operation.join("cleanup"),
        &phase_bytes(meta.owner, CLEAN_INSTALLED),
    )
    .unwrap();
    prepared.operation_pin.take();
    let operation = prepared.operation.clone();
    let result = cleanup(&prepared.service, &operation, &|| {
        !(operation.join("cleanup-complete").exists() && !operation.join("journal").exists())
    });
    assert_eq!(result, Err(ReinitializationFileError::Cancelled));
    assert!(operation.join("cleanup-complete").exists());
    drop(prepared);
    fixture.write(
        &fixture.paths.root().join("config").join("fresh.bin"),
        b"fresh generation survives metadata cleanup",
    );
    fixture.files().reconcile_startup(&|| true).unwrap();
    assert!(!operation.exists());
    assert_eq!(
        fs::read(fixture.paths.root().join("config").join("fresh.bin")).unwrap(),
        b"fresh generation survives metadata cleanup"
    );
    fixture.assert_runtime();
}
#[test]
fn old_content_changed_after_move_prevents_the_commit_receipt() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let meta = arm(&prepared, &|| true).unwrap();
    moved(&prepared, &meta, 26, true);
    let changed = prepared.operation.join("old").join("state").join("own.bin");
    prepared.service.files.remove(&changed).unwrap();
    fixture.write(&changed, b"unexpected change before commit");
    assert_eq!(
        verify_installed(&prepared.service, &prepared.operation, &meta, &|| true),
        Err(ReinitializationFileError::RepairRequired)
    );
    assert!(!prepared
        .operation
        .join(format!("step-{COMMITTED}"))
        .exists());
    assert_eq!(
        fs::read(changed).unwrap(),
        b"unexpected change before commit"
    );
    fixture.assert_runtime();
}
