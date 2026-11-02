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
fn installed_terminal_survives_reopen_and_only_retires_metadata_after_initialization() {
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
    assert_eq!(fs::read_dir(&operation).unwrap().count(), 2);
    assert!(operation.join("journal").exists());
    assert!(operation.join("cleanup-complete").exists());
    fixture.assert_runtime();
    // A new service reconstructs authority solely from the persisted original
    // journal and positive terminal marker, never ResetStartup.installed.
    let files = fixture.files();
    assert_eq!(
        files.reconcile_startup(&|| true).unwrap(),
        ResetStartup::default()
    );
    let repeat = files.review(43, &|| true).unwrap();
    let deferred = files.prepare(repeat, &|| true).unwrap();
    assert!(deferred.deferred.is_some());
    assert_eq!(deferred.sequence(), 43);
    deferred.discard(&|| true).unwrap();
    assert_eq!(
        fs::read_dir(operation.parent().unwrap()).unwrap().count(),
        2
    );
    let handoff = files.initialization_handoff(&|| true).unwrap().unwrap();
    let permit = handoff.authorize_initialization(&files, &|| true).unwrap();
    permit
        .verify_before_initialization(fixture.paths.root(), &|| true)
        .unwrap();
    drop(permit);
    fixture.write(
        &fixture
            .paths
            .root()
            .join("config")
            .join("new-key-placeholder"),
        b"subsequent fresh deployment write",
    );
    let handoff = files.initialization_handoff(&|| true).unwrap().unwrap();
    assert!(handoff.authorize_initialization(&files, &|| true).is_err());
    let handoff = files.initialization_handoff(&|| true).unwrap().unwrap();
    // This filesystem-unit test exercises metadata retirement only. Actual
    // marker/DPAPI/SQLite success is the normal bootstrap caller's gate.
    handoff
        .complete_after_backend_open(&files, &|| true)
        .unwrap();
    assert!(!operation.exists());
    assert_eq!(
        fs::read(fixture.paths.root().join("config/new-key-placeholder")).unwrap(),
        b"subsequent fresh deployment write"
    );
    assert!(files.initialization_handoff(&|| true).unwrap().is_none());
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
    assert!(operation.join("cleanup-complete").exists());
    assert!(operation.join("journal").exists());
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
fn interrupted_retirement_cannot_reauthorize_initialization_or_touch_later_data() {
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
    cleanup(&prepared.service, &operation, &|| true).unwrap();
    drop(prepared);
    let files = fixture.files();
    let handoff = files.initialization_handoff(&|| true).unwrap().unwrap();
    let result =
        handoff.complete_after_backend_open(&files, &|| operation.join("journal").exists());
    assert_eq!(result, Err(ReinitializationFileError::Cancelled));
    assert!(operation.join("cleanup-complete").exists());
    assert!(!operation.join("journal").exists());
    files.reconcile_startup(&|| true).unwrap();
    let handoff = files.initialization_handoff(&|| true).unwrap().unwrap();
    assert!(handoff.authorize_initialization(&files, &|| true).is_err());
    fixture.write(
        &fixture.paths.root().join("config").join("fresh.bin"),
        b"fresh generation survives metadata cleanup",
    );
    let handoff = files.initialization_handoff(&|| true).unwrap().unwrap();
    handoff
        .complete_after_backend_open(&files, &|| true)
        .unwrap();
    assert!(!operation.exists());
    assert_eq!(
        fs::read(fixture.paths.root().join("config").join("fresh.bin")).unwrap(),
        b"fresh generation survives metadata cleanup"
    );
    fixture.assert_runtime();
}
#[test]
fn invalid_terminal_conflict_or_replaced_empty_tree_never_mints_permission() {
    for fault in 0..3 {
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
        cleanup(&prepared.service, &operation, &|| true).unwrap();
        drop(prepared);
        if fault == 0 {
            let terminal = operation.join("cleanup-complete");
            fs::remove_file(&terminal).unwrap();
            fixture.write(&terminal, b"damaged positive receipt");
        } else if fault == 1 {
            fs::create_dir(
                operation
                    .parent()
                    .unwrap()
                    .join("op-00000000000000000000000000000000"),
            )
            .unwrap();
        } else {
            fs::rename(
                fixture.paths.root().join("state"),
                fixture.outside.join("preserved-state"),
            )
            .unwrap();
            fixture.paths.prepare_for_current_user().unwrap();
            fixture.write(
                &fixture.paths.root().join("state/replacement"),
                b"replacement preserved",
            );
        }
        let files = fixture.files();
        assert!(files.initialization_handoff(&|| true).is_err());
        assert!(operation.join("journal").exists());
        assert!(operation.join("cleanup-complete").exists());
        if fault < 2 {
            assert!(files.reconcile_startup(&|| true).is_err());
            let review = files.review(43, &|| true).unwrap();
            assert!(files.prepare(review, &|| true).is_err());
        } else {
            assert_eq!(
                fs::read(fixture.paths.root().join("state/replacement")).unwrap(),
                b"replacement preserved"
            );
        }
        fixture.assert_runtime();
    }
}
fn actual_cold_permit(
    fixture: &Fixture,
    sequence: u64,
) -> crate::reinitialization::ColdReinitializationPermit {
    use crate::reinitialization::{
        ColdReinitialization, NeverOpenedProof, ReinitializationBackend,
    };
    let proof = NeverOpenedProof::claim_repair_startup(&fixture.paths, sequence).unwrap();
    let mut drain = ColdReinitialization::begin(
        fixture.paths.root().to_owned(),
        sequence,
        ReinitializationBackend::NeverOpened(proof),
        fc_runtime::worker::WorkerPool::<crate::recovery::test_support::QuietTask>::start()
            .unwrap(),
        None,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Some(permit) = drain.poll().unwrap() {
            return permit;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
#[test]
fn repeated_explicit_reset_waits_for_actual_cold_permit_then_clears_partial_initialization() {
    let fixture = Fixture::new();
    let (prepared, _) = committed(&fixture);
    let old_operation = prepared.operation.clone();
    drop(prepared);
    let files = fixture.files();
    // An active journal is not completed metadata, even if all moves finished.
    let active_review = files.review(43, &|| true).unwrap();
    assert!(files.prepare(active_review, &|| true).is_err());
    files.reconcile_startup(&|| true).unwrap();
    fixture.write(
        &fixture.paths.root().join("config/credential-key.dpapi"),
        b"partial key survives until clear",
    );
    fs::remove_dir(fixture.paths.root().join("state")).unwrap();
    fs::remove_dir(fixture.paths.root().join("teachers")).unwrap();
    let review = files.review(43, &|| true).unwrap();
    let deferred = files.prepare(review, &|| true).unwrap();
    assert!(deferred.deferred.is_some());
    assert_eq!(deferred.root(), fixture.paths.root());
    assert_eq!(deferred.sequence(), 43);
    assert_eq!(
        fs::read_dir(old_operation.parent().unwrap())
            .unwrap()
            .count(),
        2
    );
    assert_eq!(
        fs::read(fixture.paths.root().join("config/credential-key.dpapi")).unwrap(),
        b"partial key survives until clear"
    );
    let permit = actual_cold_permit(&fixture, 43);
    let receipt = deferred.commit_cold(permit, &|| true).unwrap();
    assert_eq!(receipt.sequence, 43);
    assert!(!old_operation.exists());
    for name in OWNED_TREES {
        assert_eq!(
            fs::read_dir(fixture.paths.root().join(name))
                .unwrap()
                .count(),
            0
        );
    }
    assert_eq!(
        fs::read_dir(fixture.paths.root().join(NAMESPACE))
            .unwrap()
            .count(),
        2
    );
    assert!(files.initialization_handoff(&|| true).unwrap().is_some());
    fixture.assert_runtime();
}
#[test]
fn interrupted_explicit_metadata_retirement_preserves_partial_data_and_can_prepare_again() {
    let fixture = Fixture::new();
    let (prepared, _) = committed(&fixture);
    let operation = prepared.operation.clone();
    drop(prepared);
    let files = fixture.files();
    files.reconcile_startup(&|| true).unwrap();
    fixture.write(
        &fixture.paths.root().join("config/partial.bin"),
        b"not yet cleared",
    );
    let terminal = replacement_terminal(&files, &|| true).unwrap().unwrap();
    // Exercise the private metadata port cut; public production access is only
    // through commit_cold, whose actual permit is covered above.
    assert_eq!(
        retire_for_repeat(&files, terminal, &|| operation.join("journal").exists()),
        Err(ReinitializationFileError::Cancelled)
    );
    assert!(!operation.join("journal").exists());
    assert!(operation.join("cleanup-complete").exists());
    assert_eq!(
        fs::read(fixture.paths.root().join("config/partial.bin")).unwrap(),
        b"not yet cleared"
    );
    files.reconcile_startup(&|| true).unwrap();
    let review = files.review(44, &|| true).unwrap();
    let deferred = files.prepare(review, &|| true).unwrap();
    assert!(deferred.deferred.is_some());
    deferred.discard(&|| true).unwrap();
    assert!(operation.join("cleanup-complete").exists());
    assert_eq!(
        fs::read(fixture.paths.root().join("config/partial.bin")).unwrap(),
        b"not yet cleared"
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
