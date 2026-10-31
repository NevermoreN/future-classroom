//! Integration tests use two real SQLite deployments and the sole writer.
use super::*;
use fc_domain::recovery::*;
fn target() -> IdentityState {
    let mut s = identity();
    let school = SchoolId::new(9).unwrap();
    s.installation = Some(Installation {
        school_id: school,
        school_name: "current target school".into(),
        current_class: Some(ClassId::new(20).unwrap()),
    });
    s.accounts[0].principal.id = PrincipalId::new(40).unwrap();
    s.accounts[0].principal.school_id = school;
    s.accounts[0].display_name = "current target label".into();
    s.accounts[0].login_name = "target-login".into();
    s.accounts[0].must_change_password = false;
    s.classes[0].id = ClassId::new(20).unwrap();
    s.classes[0].school_id = school;
    s.classes[0].name = "current target class".into();
    s.subjects[0].id = SubjectId::new(30).unwrap();
    s.subjects[0].school_id = school;
    s.subjects[0].name = "current target subject".into();
    s.assignments[0].teacher_id = PrincipalId::new(40).unwrap();
    s.assignments[0].class_id = ClassId::new(20).unwrap();
    s.assignments[0].subject_id = SubjectId::new(30).unwrap();
    let mut extra = s.classes[0].clone();
    extra.id = ClassId::new(21).unwrap();
    extra.name = "unmapped target class".into();
    s.classes.push(extra);
    s.validate().unwrap();
    s
}
fn target_actor() -> ActorContext {
    ActorContext {
        principal_id: PrincipalId::new(40).unwrap(),
        school_id: SchoolId::new(9).unwrap(),
        ..actor()
    }
}
fn seed_target(path: &Path) {
    let db = open_connection(path).unwrap();
    db.execute("UPDATE state_meta SET revision=500", [])
        .unwrap();
    write_row(
        &db,
        "business/identity/v1",
        &business::codec::bytes(&business::codec::identity(&target())).unwrap(),
        Revision::new(500),
    )
    .unwrap();
    write_row(
        &db,
        model::INSTALLATION,
        &model::installation_bytes([9; 32]).unwrap(),
        Revision::new(500),
    )
    .unwrap();
    for (k, v) in [
        (
            "business/credential/40/password/1",
            b"CURRENT-TARGET-PASSWORD".as_slice(),
        ),
        (
            "business/credential/40/recovery/1",
            b"CURRENT-TARGET-RECOVERY".as_slice(),
        ),
        (
            "business/security/login/40",
            b"{\"format\":1,\"failures\":4,\"wait_seconds\":4,\"pending\":false,\"repair\":[]}"
                .as_slice(),
        ),
    ] {
        write_row(&db, k, v, Revision::new(500)).unwrap();
    }
    drop(db);
    business::startup_state(path).unwrap();
}
fn mapping() -> MigrationMapping {
    MigrationMapping {
        source_school_id: SchoolId::new(1).unwrap(),
        target_school_id: SchoolId::new(9).unwrap(),
        classes: vec![(ClassId::new(1).unwrap(), ClassId::new(20).unwrap())],
        subjects: vec![(SubjectId::new(1).unwrap(), SubjectId::new(30).unwrap())],
        principals: vec![(PrincipalId::new(1).unwrap(), PrincipalId::new(40).unwrap())],
    }
}
fn inspect(r: &BusinessRepository<u64>, path: PathBuf) -> MigrationSourceReceipt {
    r.try_inspect_migration_source(path, &target_actor(), fence())
        .unwrap()
        .wait()
        .unwrap()
}
fn subject_image(r: &mut BusinessRepository<u64>) -> ContentSlotKey {
    let key = SubjectProfileKey {
        class_id: ClassId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
        teacher_id: PrincipalId::new(1).unwrap(),
    };
    let mut p = SubjectProfileDocument::new(ProfileId::new(1).unwrap(), key);
    let node = p.allocate_id().unwrap();
    p.tree.nodes[0].children.push(node);
    p.tree.nodes.push(LayoutNode {
        id: node,
        kind: NodeKind::Widget(WidgetKind::SubjectImage),
        children: vec![],
    });
    p.content_styles
        .push(ContentNodeStyle::new(node, ContentPayloadKind::Images));
    r.execute_subject_profile(
        fc_ports::ProfileRequest {
            command_id: CommandId::new(95).unwrap(),
            key,
            expected_revision: Revision::default(),
            candidate: p,
        },
        &actor(),
        &TestClock,
    )
    .unwrap();
    ContentSlotKey {
        scope: LayoutScope::Subject(key),
        node_id: node,
    }
}
#[test]
fn mapped_ordinary_rewrites_complete_teaching_references_and_preserves_target_security() {
    let t = Temp::new();
    seed(&t.path("source.sqlite3"), 1);
    let (sw, mut source) = open(&t.path("source.sqlite3"));
    let slot = subject_image(&mut source);
    publish_image(&mut source, slot, 101, 0);
    publish_image(&mut source, slot, 102, 1);
    let scope = MaterialScope {
        teacher_id: PrincipalId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
    };
    let append = fc_ports::prepare_material_append(
        &mut MaterialPrepared,
        (),
        fc_ports::MaterialAppendMetadata {
            command_id: CommandId::new(103).unwrap(),
            scope,
            material_id: MaterialId::new(1).unwrap(),
            display_name: "source material.txt".into(),
            allowed_class_ids: vec![ClassId::new(1).unwrap()],
        },
    )
    .unwrap();
    source
        .append_material(append, &actor(), &TestClock)
        .unwrap();
    let old = source
        .try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    sw.shutdown().unwrap();
    seed_target(&t.path("target.sqlite3"));
    let (tw, r) = open(&t.path("target.sqlite3"));
    assert!(r
        .try_preflight_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("refused.sqlite3"),
            &target_actor(),
            fence()
        )
        .unwrap()
        .wait()
        .is_err());
    let expected = inspect(&r, t.path("ordinary.sqlite3"));
    let mapped = r
        .try_map_ordinary(
            t.path("ordinary.sqlite3"),
            t.path("mapped.sqlite3"),
            expected.clone(),
            mapping(),
            &target_actor(),
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(mapped.source(), &expected);
    assert_eq!(mapped.installation_id(), [9; 32]);
    assert_eq!(mapped.media().len(), 3);
    assert_eq!(mapped.revision(), old.revision);
    assert_eq!(
        tw.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(500)
    );
    let preview = r
        .try_preflight_ordinary_restore(
            t.path("mapped.sqlite3"),
            t.path("preview.sqlite3"),
            &target_actor(),
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(preview.installation_id, [9; 32]);
    let barrier = r
        .try_advance_recovery_barrier(preview.current_revision, &target_actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    r.try_prepare_ordinary_restore(
        t.path("mapped.sqlite3"),
        t.path("candidate.sqlite3"),
        barrier,
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    tw.shutdown().unwrap();
    let state = business::startup_state(t.path("candidate.sqlite3")).unwrap();
    let mut original = target();
    original.revision = state.identity.revision;
    for a in &mut original.accounts {
        a.principal.auth_epoch = AuthEpoch::new(9);
    }
    for s in &mut original.subjects {
        s.revision = s.revision.next().unwrap();
    }
    assert_eq!(state.identity, original);
    let db = Connection::open(t.path("candidate.sqlite3")).unwrap();
    for (key, bytes) in [
        (
            "business/credential/40/password/1",
            b"CURRENT-TARGET-PASSWORD".as_slice(),
        ),
        (
            "business/credential/40/recovery/1",
            b"CURRENT-TARGET-RECOVERY".as_slice(),
        ),
    ] {
        assert_eq!(TransactionView(&db).get(key).unwrap().unwrap().value, bytes);
    }
    assert!(TransactionView(&db)
        .get("business/homework/20/2000-01-01/30/40")
        .unwrap()
        .is_some());
    assert!(TransactionView(&db)
        .get("business/homework/1/2000-01-01/1/1")
        .unwrap()
        .is_none());
    let entries = TransactionView(&db)
        .scan_prefix("business/profile-media/v1/entries/", None, 128)
        .unwrap();
    for (_, row) in entries.entries {
        let v = business::codec::value(&row.value).unwrap();
        assert_eq!(
            v["owner"]["scope"],
            json!({"class":20,"subject":30,"teacher":40})
        );
    }
    drop(db);
}
#[test]
fn stale_review_and_incomplete_mapping_never_create_an_output() {
    let t = Temp::new();
    seed(&t.path("source.sqlite3"), 0);
    let (sw, s) = open(&t.path("source.sqlite3"));
    s.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    sw.shutdown().unwrap();
    seed_target(&t.path("target.sqlite3"));
    let (tw, r) = open(&t.path("target.sqlite3"));
    let expected = inspect(&r, t.path("ordinary.sqlite3"));
    let mut missing = mapping();
    missing.principals.clear();
    assert!(r
        .try_map_ordinary(
            t.path("ordinary.sqlite3"),
            t.path("missing.sqlite3"),
            expected.clone(),
            missing,
            &target_actor(),
            fence()
        )
        .unwrap()
        .wait()
        .is_err());
    assert!(!t.path("missing.sqlite3").exists());
    r.try_advance_recovery_barrier(Revision::new(500), &target_actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    assert!(r
        .try_map_ordinary(
            t.path("ordinary.sqlite3"),
            t.path("stale.sqlite3"),
            expected,
            mapping(),
            &ActorContext {
                auth_epoch: AuthEpoch::new(8),
                ..target_actor()
            },
            fence()
        )
        .unwrap()
        .wait()
        .is_err());
    assert!(!t.path("stale.sqlite3").exists());
    tw.shutdown().unwrap();
}
#[test]
fn canonical_mapping_digest_is_order_independent_but_not_lossy() {
    let mut m = mapping();
    m.classes
        .push((ClassId::new(2).unwrap(), ClassId::new(21).unwrap()));
    let digest = migration_mapping_sha256(&m).unwrap();
    m.classes.reverse();
    assert_eq!(migration_mapping_sha256(&m).unwrap(), digest);
    m.classes[0].1 = m.classes[1].1;
    assert!(migration_mapping_sha256(&m).is_err());
    m.classes = vec![m.classes[0]; MAX_CLASSES + 1];
    assert_eq!(
        migration_mapping_sha256(&m),
        Err(StorageError::ResourceQuota)
    );
}
#[test]
fn malformed_source_rows_are_rejected_before_mapping_can_normalize_them() {
    let t = Temp::new();
    seed(&t.path("source.sqlite3"), 1);
    let (sw, s) = open(&t.path("source.sqlite3"));
    s.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    sw.shutdown().unwrap();
    seed_target(&t.path("target.sqlite3"));
    let (tw, r) = open(&t.path("target.sqlite3"));
    for case in 0..4 {
        let input = t.path(&format!("bad-{case}.sqlite3"));
        std::fs::copy(t.path("ordinary.sqlite3"), &input).unwrap();
        let db = Connection::open(&input).unwrap();
        let key = "business/homework/1/2000-01-01/1/1";
        match case {
            0 => {
                let row = TransactionView(&db).get(key).unwrap().unwrap();
                let mut v = business::codec::value(&row.value).unwrap();
                v["role"] = json!("global_admin");
                db.execute(
                    "UPDATE documents SET value=?1 WHERE key=?2",
                    params![business::codec::bytes(&v).unwrap(), key],
                )
                .unwrap();
            }
            1 => {
                db.execute(
                    "UPDATE documents SET key='business/homework/2/2000-01-01/1/1' WHERE key=?1",
                    [key],
                )
                .unwrap();
            }
            2 => {
                db.execute("UPDATE documents SET revision=999999 WHERE key=?1", [key])
                    .unwrap();
            }
            3 => {
                let bytes: Vec<u8> = db
                    .query_row("SELECT catalog FROM backup_meta", [], |r| r.get(0))
                    .unwrap();
                let mut v = business::codec::value(&bytes).unwrap();
                v["accounts"] = json!([]);
                db.execute(
                    "UPDATE backup_meta SET catalog=?1",
                    [business::codec::bytes(&v).unwrap()],
                )
                .unwrap();
            }
            _ => unreachable!(),
        };
        drop(db);
        let expected = r
            .try_inspect_migration_source(input.clone(), &target_actor(), fence())
            .unwrap()
            .wait();
        let output = t.path(&format!("bad-output-{case}.sqlite3"));
        if case >= 2 {
            assert!(expected.is_err(), "case {case}");
        } else {
            assert!(
                r.try_map_ordinary(
                    input,
                    output.clone(),
                    expected.unwrap(),
                    mapping(),
                    &target_actor(),
                    fence()
                )
                .unwrap()
                .wait()
                .is_err(),
                "case {case}"
            );
        }
        assert!(!output.exists());
        assert_eq!(
            tw.handle().try_revision().unwrap().wait().unwrap(),
            Revision::new(500)
        );
    }
    tw.shutdown().unwrap();
}
#[test]
fn changed_source_hash_cancellation_and_existing_destination_preserve_live_data() {
    let t = Temp::new();
    seed(&t.path("source.sqlite3"), 1);
    let (sw, s) = open(&t.path("source.sqlite3"));
    s.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    sw.shutdown().unwrap();
    seed_target(&t.path("target.sqlite3"));
    let (tw, r) = open(&t.path("target.sqlite3"));
    let expected = inspect(&r, t.path("ordinary.sqlite3"));
    std::fs::write(t.path("existing.sqlite3"), b"EXTERNAL-EXISTING-SENTINEL").unwrap();
    assert_eq!(
        r.try_map_ordinary(
            t.path("ordinary.sqlite3"),
            t.path("existing.sqlite3"),
            expected.clone(),
            mapping(),
            &target_actor(),
            fence()
        )
        .unwrap()
        .wait()
        .unwrap_err(),
        StorageError::BackupDestinationExists
    );
    assert_eq!(
        std::fs::read(t.path("existing.sqlite3")).unwrap(),
        b"EXTERNAL-EXISTING-SENTINEL"
    );
    assert!(r
        .try_map_ordinary(
            t.path("ordinary.sqlite3"),
            t.path("cancelled.sqlite3"),
            expected.clone(),
            mapping(),
            &target_actor(),
            Arc::new(|| false)
        )
        .unwrap()
        .wait()
        .is_err());
    assert!(!t.path("cancelled.sqlite3").exists());
    let db = Connection::open(t.path("ordinary.sqlite3")).unwrap();
    let key = "business/homework/1/2000-01-01/1/1";
    let row = TransactionView(&db).get(key).unwrap().unwrap();
    let mut v = business::codec::value(&row.value).unwrap();
    v["text"] = json!("valid but changed since review");
    db.execute(
        "UPDATE documents SET value=?1 WHERE key=?2",
        params![business::codec::bytes(&v).unwrap(), key],
    )
    .unwrap();
    drop(db);
    assert!(r
        .try_map_ordinary(
            t.path("ordinary.sqlite3"),
            t.path("changed.sqlite3"),
            expected,
            mapping(),
            &target_actor(),
            fence()
        )
        .unwrap()
        .wait()
        .is_err());
    assert!(!t.path("changed.sqlite3").exists());
    assert_eq!(
        tw.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(500)
    );
    tw.shutdown().unwrap();
}
#[test]
fn mapped_published_history_does_not_reenable_a_disabled_target_account() {
    let t = Temp::new();
    seed(&t.path("source.sqlite3"), 1);
    let (sw, mut source) = open(&t.path("source.sqlite3"));
    let scope = MaterialScope {
        teacher_id: PrincipalId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
    };
    let prepared = fc_ports::prepare_material_append(
        &mut MaterialPrepared,
        (),
        fc_ports::MaterialAppendMetadata {
            command_id: CommandId::new(200).unwrap(),
            scope,
            material_id: MaterialId::new(1).unwrap(),
            display_name: "history.txt".into(),
            allowed_class_ids: vec![ClassId::new(1).unwrap()],
        },
    )
    .unwrap();
    source
        .append_material(prepared, &actor(), &TestClock)
        .unwrap();
    source
        .try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    sw.shutdown().unwrap();
    seed_target(&t.path("target.sqlite3"));
    let db = Connection::open(t.path("target.sqlite3")).unwrap();
    let mut i = target();
    let mut disabled = i.accounts[0].clone();
    disabled.principal.id = PrincipalId::new(41).unwrap();
    disabled.principal.role = Role::User;
    disabled.principal.enabled = false;
    disabled.login_name = "disabled-target".into();
    disabled.display_name = "disabled current label".into();
    i.accounts.push(disabled);
    i.validate().unwrap();
    write_row(
        &db,
        "business/identity/v1",
        &business::codec::bytes(&business::codec::identity(&i)).unwrap(),
        Revision::new(500),
    )
    .unwrap();
    // Even disabled accounts retain their independently provisioned current
    // credentials; the source teaching package never supplies these records.
    for (key, value) in [
        (
            "business/credential/41/password/1",
            b"CURRENT-DISABLED-TARGET-PASSWORD".as_slice(),
        ),
        (
            "business/credential/41/recovery/1",
            b"CURRENT-DISABLED-TARGET-RECOVERY".as_slice(),
        ),
    ] {
        write_row(&db, key, value, Revision::new(500)).unwrap();
    }
    drop(db);
    assert_eq!(
        business::startup_state(t.path("target.sqlite3"))
            .unwrap()
            .identity,
        i
    );
    let (tw, r) = open(&t.path("target.sqlite3"));
    let expected = inspect(&r, t.path("ordinary.sqlite3"));
    let mut m = mapping();
    m.principals[0].1 = PrincipalId::new(41).unwrap();
    r.try_map_ordinary(
        t.path("ordinary.sqlite3"),
        t.path("mapped.sqlite3"),
        expected,
        m,
        &target_actor(),
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    r.try_preflight_ordinary_restore(
        t.path("mapped.sqlite3"),
        t.path("preview.sqlite3"),
        &target_actor(),
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    tw.shutdown().unwrap();
    let db = Connection::open(t.path("preview.sqlite3")).unwrap();
    let restored = business::load_identity(&TransactionView(&db)).unwrap();
    assert!(
        !restored
            .account(PrincipalId::new(41).unwrap())
            .unwrap()
            .principal
            .enabled
    );
    let folder = TransactionView(&db)
        .get("business/material/v1/folders/00000000000000000001")
        .unwrap()
        .unwrap();
    assert_eq!(
        business::codec::value(&folder.value).unwrap()["scope"]["teacher"],
        41
    );
    drop(db);
    let (w, r) = open(&t.path("preview.sqlite3"));
    let disabled_actor = ActorContext {
        principal_id: PrincipalId::new(41).unwrap(),
        auth_epoch: AuthEpoch::new(8),
        ..target_actor()
    };
    assert!(r
        .materials(
            fc_ports::MaterialListRequest {
                scope: MaterialScope {
                    teacher_id: PrincipalId::new(41).unwrap(),
                    subject_id: SubjectId::new(30).unwrap()
                },
                cursor: None
            },
            &disabled_actor,
            &TestClock
        )
        .is_err());
    w.shutdown().unwrap();
}
