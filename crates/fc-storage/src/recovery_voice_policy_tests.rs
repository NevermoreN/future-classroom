//! Cross-installation ordinary restore retains the target's local audio policy,
//! including the implicit Default when this target has no voice document.
use super::*;
use fc_domain::{recovery::MigrationMapping, voice::*};

fn narrow() -> VoiceLimits {
    VoiceLimits {
        max_duration_millis: 500,
        max_pending_jobs: 1,
        max_pending_per_owner: 1,
        max_repeat_total: 1,
    }
}

fn cross_installation_case(target_has_policy: bool) {
    let t = Temp::new();
    seed(&t.path("source.sqlite3"), 0);
    let (sw, source) = open(&t.path("source.sqlite3"));
    let source_limits = if target_has_policy {
        VoiceLimits::default()
    } else {
        narrow()
    };
    let original = source.voice_inventory().unwrap();
    let changed = source
        .set_voice_limits(source_limits, original.revision, &actor())
        .unwrap();
    if !target_has_policy {
        source
            .set_voice_enabled(false, changed.revision, &actor())
            .unwrap();
    }
    source
        .try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    sw.shutdown().unwrap();

    seed(&t.path("target.sqlite3"), 0);
    let db = open_connection(&t.path("target.sqlite3")).unwrap();
    // Numeric school/catalog IDs may coincide across independently identified
    // deployments. The source is [7;32]; the target has a distinct [9;32] ID.
    write_row(
        &db,
        model::INSTALLATION,
        &model::installation_bytes([9; 32]).unwrap(),
        Revision::new(1),
    )
    .unwrap();
    drop(db);
    let (tw, target) = open(&t.path("target.sqlite3"));
    if target_has_policy {
        let old = target.voice_inventory().unwrap();
        let changed = target
            .set_voice_limits(narrow(), old.revision, &actor())
            .unwrap();
        target
            .set_voice_enabled(false, changed.revision, &actor())
            .unwrap();
    } else {
        let db = Connection::open(t.path("target.sqlite3")).unwrap();
        assert!(TransactionView(&db)
            .get("business/voice/v1")
            .unwrap()
            .is_none());
    }
    let expected_policy = target.voice_inventory().unwrap();
    let inspected = target
        .try_inspect_migration_source(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let mapping = MigrationMapping {
        source_school_id: SchoolId::new(1).unwrap(),
        target_school_id: SchoolId::new(1).unwrap(),
        classes: vec![(ClassId::new(1).unwrap(), ClassId::new(1).unwrap())],
        subjects: vec![(SubjectId::new(1).unwrap(), SubjectId::new(1).unwrap())],
        principals: vec![(PrincipalId::new(1).unwrap(), PrincipalId::new(1).unwrap())],
    };
    let mapped = target
        .try_map_ordinary(
            t.path("ordinary.sqlite3"),
            t.path("mapped.sqlite3"),
            inspected,
            mapping,
            &actor(),
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(mapped.installation_id(), [9; 32]);
    let preview = target
        .try_preflight_ordinary_restore(
            t.path("mapped.sqlite3"),
            t.path("preview.sqlite3"),
            &actor(),
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(preview.installation_id, [9; 32]);
    let barrier = target
        .try_advance_recovery_barrier(preview.current_revision, &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    target
        .try_prepare_ordinary_restore(
            t.path("mapped.sqlite3"),
            t.path("candidate.sqlite3"),
            barrier,
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    tw.shutdown().unwrap();
    for name in ["preview.sqlite3", "candidate.sqlite3"] {
        let (writer, restored) = open(&t.path(name));
        let voice = restored.voice_inventory().unwrap();
        assert_eq!(
            voice.enabled, expected_policy.enabled,
            "{name}: source switch is not target policy"
        );
        assert_eq!(
            voice.limits, expected_policy.limits,
            "{name}: target local limits survive mapping restore"
        );
        assert!(voice.assets.is_empty());
        assert!(voice.jobs.iter().all(|job| job.state.terminal()
            && job.asset_id.is_none()
            && job.authorization.is_none()));
        assert!(voice.next_sequence > expected_policy.next_sequence);
        assert!(voice.next_generation > expected_policy.next_generation);
        assert!(voice.observed_at >= expected_policy.observed_at);
        writer.shutdown().unwrap();
    }
}

#[test]
fn mapped_voice_restore_keeps_current_target_disabled_and_tight_limits() {
    cross_installation_case(true);
}

#[test]
fn mapped_voice_restore_uses_target_defaults_when_no_local_voice_document_exists() {
    cross_installation_case(false);
}
