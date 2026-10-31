use super::*;
use crate::business::{BusinessRepository, SessionGuard};
use fc_domain::profile::*;
use fc_domain::{auth::*, identity::*, ids::*};
use fc_domain::{layout::*, materials::*, profile_content::*};
use fc_ports::voice::VoiceRepository;
use fc_ports::{MaterialRepository, ProfileContentRepository, ProfileRepository};
use std::sync::atomic::AtomicU64;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "fc-round22-storage-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
struct TestClock;
impl Clock for TestClock {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 10 * 3600).unwrap()
    }
}
struct Publisher;
impl AuthorityPublisher for Publisher {
    type Snapshot = u64;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, s: &u64) -> Revision {
        Revision::new(*s)
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        Ok(())
    }
    fn publish(&mut self, _: (), _: u64) -> Result<(), ()> {
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn fault(&mut self) {}
}
fn identity() -> IdentityState {
    IdentityState {
        revision: Revision::new(1),
        installation: Some(Installation {
            school_id: SchoolId::new(1).unwrap(),
            school_name: "恢复测试学校".into(),
            current_class: Some(ClassId::new(1).unwrap()),
        }),
        accounts: vec![UserAccount {
            principal: Principal {
                id: PrincipalId::new(1).unwrap(),
                school_id: SchoolId::new(1).unwrap(),
                role: Role::GlobalAdmin,
                enabled: true,
                auth_epoch: AuthEpoch::new(7),
            },
            login_name: "admin".into(),
            display_name: "主管理员".into(),
            must_change_password: false,
            credential_version: 1,
            recovery_version: 1,
        }],
        classes: vec![SchoolClass {
            id: ClassId::new(1).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            name: "一班".into(),
            enabled: true,
        }],
        subjects: vec![SchoolSubject {
            id: SubjectId::new(1).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            name: "语文".into(),
            enabled: true,
            color: Rgb {
                red: 1,
                green: 2,
                blue: 3,
            },
            revision: Revision::new(1),
        }],
        assignments: vec![TeachingAssignment {
            teacher_id: PrincipalId::new(1).unwrap(),
            class_id: ClassId::new(1).unwrap(),
            subject_id: SubjectId::new(1).unwrap(),
            enabled: true,
        }],
        default_teachers: vec![],
    }
}
fn actor() -> ActorContext {
    ActorContext {
        principal_id: PrincipalId::new(1).unwrap(),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(7),
        expires_at: LocalDateTime::new(LocalDate::new(2026, 11, 1).unwrap(), 0).unwrap(),
        origin: TrustedOrigin::Local,
    }
}
fn guard() -> SessionGuard {
    Arc::new(|i, a| {
        i.authority(a.principal_id)
            .and_then(|x| {
                x.validate_actor(a, TestClock.now())
                    .map_err(IdentityError::Permission)
            })
            .map(|_| ())
            .map_err(StorageError::Identity)
    })
}
fn seed(path: &Path, rows: usize) {
    let mut db = open_connection(path).unwrap();
    let tx = db.transaction().unwrap();
    tx.execute("UPDATE state_meta SET revision=1", []).unwrap();
    write_row(
        &tx,
        "business/identity/v1",
        &business::codec::bytes(&business::codec::identity(&identity())).unwrap(),
        Revision::new(1),
    )
    .unwrap();
    for (k, v) in [
        (
            "business/credential/1/password/1",
            b"SECRET-PASSWORD-RECORD-ROUND22".as_slice(),
        ),
        (
            "business/credential/1/recovery/1",
            b"SECRET-RECOVERY-RECORD-ROUND22".as_slice(),
        ),
        (
            "business/receipt/identity/42",
            b"SECRET-IDENTITY-RECEIPT-ROUND22".as_slice(),
        ),
        (
            "business/security/login/1",
            b"{\"format\":1,\"failures\":8,\"wait_seconds\":8,\"pending\":false,\"repair\":[]}"
                .as_slice(),
        ),
    ] {
        write_row(&tx, k, v, Revision::new(1)).unwrap();
    }
    write_row(
        &tx,
        model::INSTALLATION,
        &model::installation_bytes([7; 32]).unwrap(),
        Revision::new(1),
    )
    .unwrap();
    for n in 0..rows {
        let year = 2000 + n as i32;
        let key = format!("business/homework/1/{year:04}-01-01/1/1");
        let date = json!([year, 1, 1]);
        let v = json!({"key":{"kind":"homework","class":1,"date":date,"subject":1,"owner":1},"text":"x".repeat(2300),"revision":1,"actor":1,"at":{"date":date,"second":36000}});
        write_row(
            &tx,
            &key,
            &serde_json::to_vec(&v).unwrap(),
            Revision::new(1),
        )
        .unwrap();
    }
    tx.commit().unwrap();
    drop(db);
    business::startup_state(path).unwrap();
}
fn open(path: &Path) -> (StorageWriter<u64>, BusinessRepository<u64>) {
    let w = StorageWriter::open(path, 8, Publisher).unwrap();
    let r = BusinessRepository::new(
        w.handle(),
        Arc::new(TestClock),
        Arc::new(|_, r| Ok(r.get())),
    )
    .with_session_guard(guard());
    (w, r)
}
fn fence() -> ExecutionFence {
    Arc::new(|| true)
}
fn image_slot(r: &mut BusinessRepository<u64>) -> ContentSlotKey {
    image_slot_for(r, &actor(), 100)
}
fn image_slot_for(
    r: &mut BusinessRepository<u64>,
    who: &ActorContext,
    command: u64,
) -> ContentSlotKey {
    let state = r.public_layout(who).unwrap();
    let mut layout = state.layout.clone();
    let id = layout.allocate_id().unwrap();
    let group = layout.allocate_id().unwrap();
    let outlet = layout
        .tree
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
        .unwrap();
    let mut children = std::mem::replace(&mut outlet.children, vec![group]);
    children.push(id);
    layout.tree.nodes.push(LayoutNode {
        id: group,
        kind: NodeKind::Container(ContainerSettings {
            flow: Flow::Column,
            padding: 0,
            gap: 8,
            width: AxisSize::Fixed(300),
            height: AxisSize::Fixed(400),
            group_opacity_percent: 100,
            material: Some(GlassMaterial {
                surface_percent: 3,
                edge_percent: 50,
            }),
        }),
        children,
    });
    layout.tree.nodes.push(LayoutNode {
        id,
        kind: NodeKind::Widget(WidgetKind::SubjectImage),
        children: vec![],
    });
    let mut component = state.display.components[0].clone();
    component.kind = fc_domain::desktop::ComponentKind::SubjectImage;
    layout.content_styles.push(ContentNodeStyle {
        node_id: id,
        component,
        image_fit: ImageFit::Contain,
    });
    r.execute_public_layout(
        fc_ports::PublicLayoutRequest {
            command_id: CommandId::new(command).unwrap(),
            expected_layout_revision: state.layout.revision,
            expected_display_revision: state.display.revision,
            layout,
            display: state.display,
        },
        who,
        &TestClock,
    )
    .unwrap();
    ContentSlotKey {
        scope: LayoutScope::Public,
        node_id: id,
    }
}

#[test]
fn restoring_an_older_empty_package_keeps_profile_media_folder_and_node_allocator_floors() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, mut r) = open(&t.path("live.sqlite3"));
    r.try_export_ordinary(t.path("old.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let slot = image_slot(&mut r);
    publish_image(&mut r, slot, 101, 0);
    let key = SubjectProfileKey {
        class_id: ClassId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
        teacher_id: PrincipalId::new(1).unwrap(),
    };
    r.execute_subject_profile(
        fc_ports::ProfileRequest {
            command_id: CommandId::new(110).unwrap(),
            key,
            expected_revision: Revision::default(),
            candidate: SubjectProfileDocument::new(ProfileId::new(1).unwrap(), key),
        },
        &actor(),
        &TestClock,
    )
    .unwrap();
    let scope = MaterialScope {
        teacher_id: key.teacher_id,
        subject_id: key.subject_id,
    };
    let append = fc_ports::prepare_material_append(
        &mut MaterialPrepared,
        (),
        fc_ports::MaterialAppendMetadata {
            command_id: CommandId::new(111).unwrap(),
            scope,
            material_id: MaterialId::new(1).unwrap(),
            display_name: "后来新增.txt".into(),
            allowed_class_ids: vec![ClassId::new(1).unwrap()],
        },
    )
    .unwrap();
    r.append_material(append, &actor(), &TestClock).unwrap();
    let revision = w.handle().try_revision().unwrap().wait().unwrap();
    let high = r.public_layout(&actor()).unwrap().layout.node_id_high_water;
    let barrier = r
        .try_advance_recovery_barrier(revision, &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let receipt = r
        .try_prepare_ordinary_restore(
            t.path("old.sqlite3"),
            t.path("candidate.sqlite3"),
            barrier,
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert!(receipt.media.is_empty());
    w.shutdown().unwrap();
    let (cw, mut restored) = open(&t.path("candidate.sqlite3"));
    let mut fresh = actor();
    fresh.auth_epoch = AuthEpoch::new(9);
    let layout = restored.public_layout(&fresh).unwrap();
    assert!(layout.layout.node_id_high_water >= high);
    let p = restored.subject_profile(key, &fresh).unwrap().unwrap();
    assert_eq!(p.id.get(), 1);
    assert!(!p.enabled);
    let media = restored.inventory_profile_media(None, 128).unwrap();
    assert_eq!(media.items.len(), 1);
    assert_eq!(
        media.items[0].state,
        business::profile_content::MediaState::Removed
    );
    let material = restored
        .materials(
            fc_ports::MaterialListRequest {
                scope,
                cursor: None,
            },
            &fresh,
            &TestClock,
        )
        .unwrap();
    assert_eq!(material.folder.id.get(), 1);
    assert!(material.items.is_empty());
    let next = image_slot_for(&mut restored, &fresh, 200);
    assert!(next.node_id.get() > slot.node_id.get());
    let ad = begin_image(&mut restored, next, 112, 0, &fresh);
    let res = restored.reserve_profile_media(&ad, &[4], &fresh).unwrap();
    assert_eq!(res.ids[0].get(), 2);
    restored.settle_profile_media(res).unwrap();
    cw.shutdown().unwrap();
    business::startup_state(t.path("candidate.sqlite3")).unwrap();
}
fn begin_image(
    r: &mut BusinessRepository<u64>,
    slot: ContentSlotKey,
    command: u64,
    rev: u64,
    who: &ActorContext,
) -> ContentAdmission {
    r.begin_profile_content(
        BeginContentRequest {
            command_id: CommandId::new(command).unwrap(),
            slot,
            expected_revision: Revision::new(rev),
            operation: ContentOperation::Publish(ContentPayloadKind::Images),
            payload_fingerprint: [command as u8; 32],
            expires_at: LocalDateTime::new(TestClock.now().date, 12 * 3600).unwrap(),
        },
        who,
        &TestClock,
    )
    .unwrap()
}
struct Prepared(ContentPayload);
impl fc_ports::ProfileContentPreparation for Prepared {
    type Input = ();
    fn prepare(
        &mut self,
        _: &ContentAdmission,
        _: (),
    ) -> Result<ContentPayload, fc_ports::ProfileContentRepositoryError> {
        Ok(self.0.clone())
    }
    fn sha256(&self, b: &[u8]) -> Result<[u8; 32], fc_ports::ProfileContentRepositoryError> {
        Ok(Sha256::digest(b).into())
    }
}
fn publish_image(r: &mut BusinessRepository<u64>, slot: ContentSlotKey, command: u64, rev: u64) {
    let ad = begin_image(r, slot, command, rev, &actor());
    let res = r.reserve_profile_media(&ad, &[4], &actor()).unwrap();
    let media = MediaReference {
        version_id: res.ids[0],
        owner: slot,
        sha256: Sha256::digest(b"test").into(),
        bytes: 4,
        width: 1,
        height: 1,
        format: ImageFormat::Png,
    };
    let proof = fc_ports::prepare_profile_content(
        &mut Prepared(ContentPayload::Images(vec![media])),
        ad,
        (),
    )
    .unwrap();
    r.commit_profile_content(proof, &actor(), &TestClock)
        .unwrap();
    r.settle_profile_media(res).unwrap();
}
struct MaterialPrepared;
impl fc_ports::MaterialPreparation for MaterialPrepared {
    type Input = ();
    fn prepare(&mut self, _: ()) -> Result<MaterialResource, fc_ports::MaterialRepositoryError> {
        Ok(MaterialResource {
            version_id: ResourceVersionId::new(1).unwrap(),
            sha256: Sha256::digest(b"test").into(),
            kind: MaterialKind::Txt,
            bytes: 4,
        })
    }
}

#[test]
fn projection_covers_two_resource_namespaces_previous_images_and_pending_media_without_replay() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, mut r) = open(&t.path("live.sqlite3"));
    let slot = image_slot(&mut r);
    publish_image(&mut r, slot, 101, 0);
    publish_image(&mut r, slot, 102, 1);
    let pending = begin_image(&mut r, slot, 103, 2, &actor());
    let reservation = r.reserve_profile_media(&pending, &[4], &actor()).unwrap();
    assert_eq!(reservation.ids[0].get(), 3);
    let scope = MaterialScope {
        teacher_id: PrincipalId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
    };
    let append = fc_ports::prepare_material_append(
        &mut MaterialPrepared,
        (),
        fc_ports::MaterialAppendMetadata {
            command_id: CommandId::new(101).unwrap(),
            scope,
            material_id: MaterialId::new(1).unwrap(),
            display_name: "教学资料.txt".into(),
            allowed_class_ids: vec![ClassId::new(1).unwrap()],
        },
    )
    .unwrap();
    r.append_material(append, &actor(), &TestClock).unwrap();
    let receipt = r
        .try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(receipt.media.len(), 3);
    assert!(matches!(receipt.media[0],BackupEntry::Material{version_id,..}if version_id.get()==1));
    assert!(receipt
        .media
        .iter()
        .any(|e| matches!(e,BackupEntry::ProfileMedia{version_id,..}if version_id.get()==1)));
    let ordinary = open_ordinary(&t.path("ordinary.sqlite3")).unwrap();
    assert!(TransactionView(&ordinary)
        .scan_prefix("business/profile-media/v1/reservations/", None, 128)
        .unwrap()
        .entries
        .is_empty());
    drop(ordinary);
    let revision = w.handle().try_revision().unwrap().wait().unwrap();
    let preview = r
        .try_preflight_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("preview.sqlite3"),
            &actor(),
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(preview.current_revision, revision);
    assert_eq!(w.handle().try_revision().unwrap().wait().unwrap(), revision);
    let barrier = r
        .try_advance_recovery_barrier(revision, &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let receipt = r
        .try_prepare_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("candidate.sqlite3"),
            barrier,
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(receipt.media.len(), 3);
    let candidate = Connection::open(t.path("candidate.sqlite3")).unwrap();
    assert_eq!(
        TransactionView(&candidate)
            .get("business/profile-content/v1/receipts/00000000000000000103"),
        Err(StorageError::CommandIdReused)
    );
    assert_eq!(
        TransactionView(&candidate).get("business/material/v1/receipts/00000000000000000101"),
        Err(StorageError::CommandIdReused)
    );
    let row = TransactionView(&candidate)
        .get("business/profile-media/v1/entries/00000000000000000003")
        .unwrap()
        .unwrap();
    assert_eq!(
        business::codec::value(&row.value).unwrap()["state"],
        "removed"
    );
    drop(candidate);
    w.shutdown().unwrap();
    let (cw, mut restored) = open(&t.path("candidate.sqlite3"));
    let mut fresh = actor();
    fresh.auth_epoch = AuthEpoch::new(9);
    let ad = begin_image(&mut restored, slot, 104, 2, &fresh);
    let next = restored.reserve_profile_media(&ad, &[4], &fresh).unwrap();
    assert_eq!(next.ids[0].get(), 4);
    restored.settle_profile_media(next).unwrap();
    cw.shutdown().unwrap();
}

#[test]
fn ordinary_projection_never_writes_credentials_security_or_authorization_pages() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 2);
    let (w, r) = open(&t.path("live.sqlite3"));
    let receipt = r
        .try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(receipt.revision, Revision::new(1));
    assert_eq!(receipt.document_count, 4);
    assert!(receipt.media.is_empty());
    let bytes = std::fs::read(t.path("ordinary.sqlite3")).unwrap();
    for secret in [
        b"SECRET-PASSWORD-RECORD-ROUND22".as_slice(),
        b"SECRET-RECOVERY-RECORD-ROUND22",
        b"SECRET-IDENTITY-RECEIPT-ROUND22",
        b"auth_epoch",
        b"credential_version",
        b"business/security/login",
    ] {
        assert!(!bytes.windows(secret.len()).any(|w| w == secret));
    }
    let db = open_ordinary(&t.path("ordinary.sqlite3")).unwrap();
    assert!(TransactionView(&db)
        .get("business/identity/v1")
        .unwrap()
        .is_none());
    assert_eq!(
        r.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
            .unwrap()
            .wait(),
        Err(StorageError::BackupDestinationExists)
    );
    w.shutdown().unwrap();
}

#[test]
fn streaming_restore_exceeds_normal_mutation_and_queue_limits_and_keeps_current_security() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 2200);
    let (w, r) = open(&t.path("live.sqlite3"));
    let export = r
        .try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    assert!(
        std::fs::metadata(t.path("ordinary.sqlite3")).unwrap().len() > MAX_COMMAND_BYTES as u64
    );
    assert!(export.document_count > 1024);
    assert_eq!(w.handle().queued_bytes(), 0);
    let barrier = r
        .try_advance_recovery_barrier(Revision::new(1), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(barrier.revision(), Revision::new(2));
    assert_eq!(barrier.generation(), 1);
    let candidate = r
        .try_prepare_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("candidate.sqlite3"),
            barrier,
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(candidate.revision, Revision::new(3));
    assert_eq!(candidate.recovery_generation, 2);
    assert!(candidate.document_count > 2200);
    let live = business::startup_state(t.path("live.sqlite3")).unwrap();
    let restored = business::startup_state(t.path("candidate.sqlite3")).unwrap();
    assert_eq!(live.revision, Revision::new(2));
    assert_eq!(
        live.identity.accounts[0].principal.auth_epoch,
        AuthEpoch::new(8)
    );
    assert_eq!(
        restored.identity.accounts[0].principal.auth_epoch,
        AuthEpoch::new(9)
    );
    assert_eq!(restored.identity.accounts[0].credential_version, 1);
    let live = Connection::open(t.path("live.sqlite3")).unwrap();
    let restored = Connection::open(t.path("candidate.sqlite3")).unwrap();
    for key in [
        "business/credential/1/password/1",
        "business/credential/1/recovery/1",
        "business/security/login/1",
    ] {
        assert_eq!(
            TransactionView(&live).get(key).unwrap().unwrap().value,
            TransactionView(&restored).get(key).unwrap().unwrap().value
        );
    }
    assert!(r
        .try_export_ordinary(t.path("stale.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .is_err());
    w.shutdown().unwrap();
}

#[test]
fn cancelled_and_wrong_origin_barriers_leave_live_data_and_security_unchanged() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, r) = open(&t.path("live.sqlite3"));
    assert!(matches!(
        r.try_advance_recovery_barrier(Revision::new(1), &actor(), Arc::new(|| false))
            .unwrap()
            .wait(),
        Err(StorageError::RecoveryCancelled)
    ));
    let mut web = actor();
    web.origin = TrustedOrigin::Web;
    assert!(r
        .try_advance_recovery_barrier(Revision::new(1), &web, fence())
        .unwrap()
        .wait()
        .is_err());
    assert_eq!(r.load_identity().unwrap(), identity());
    assert_eq!(
        w.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(1)
    );
    w.shutdown().unwrap();
}

#[test]
fn stale_barrier_and_malicious_schema_do_not_create_a_candidate() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 1);
    let (w, r) = open(&t.path("live.sqlite3"));
    r.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let db = Connection::open(t.path("ordinary.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE injected_secret (value BLOB);")
        .unwrap();
    drop(db);
    let barrier = r
        .try_advance_recovery_barrier(Revision::new(1), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        r.try_prepare_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("candidate.sqlite3"),
            barrier,
            fence()
        )
        .unwrap()
        .wait(),
        Err(StorageError::RecoveryInvalidPackage)
    );
    assert!(!t.path("candidate.sqlite3").exists());
    assert_eq!(
        w.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(2)
    );
    w.shutdown().unwrap();
}

#[test]
fn ordinary_cannot_hide_unknown_rows_outside_the_program_namespace() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, r) = open(&t.path("live.sqlite3"));
    r.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let db = Connection::open(t.path("ordinary.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO documents VALUES('outside/secret',?1,1)",
        [b"{}".as_slice()],
    )
    .unwrap();
    drop(db);
    assert!(matches!(
        open_ordinary(&t.path("ordinary.sqlite3")),
        Err(StorageError::RecoveryInvalidPackage)
    ));
    w.shutdown().unwrap();
}

#[test]
fn preview_rejects_a_different_installation_even_when_numeric_school_and_teacher_ids_match() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, r) = open(&t.path("live.sqlite3"));
    r.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let source = Connection::open(t.path("ordinary.sqlite3")).unwrap();
    source
        .execute(
            "UPDATE backup_meta SET installation_id=?1",
            [[8u8; 32].as_slice()],
        )
        .unwrap();
    drop(source);
    assert_eq!(
        r.try_preflight_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("preview.sqlite3"),
            &actor(),
            fence()
        )
        .unwrap()
        .wait(),
        Err(StorageError::RecoveryConflict)
    );
    assert!(!t.path("preview.sqlite3").exists());
    assert_eq!(
        w.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(1)
    );
    w.shutdown().unwrap();
}

#[test]
fn barrier_precommit_revocation_rolls_back_and_later_cleanup_can_advance_revision() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, r) = open(&t.path("live.sqlite3"));
    r.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    assert!(matches!(
        r.try_advance_recovery_barrier(
            Revision::new(1),
            &actor(),
            Arc::new(move || c.fetch_add(1, Ordering::SeqCst) == 0)
        )
        .unwrap()
        .wait(),
        Err(StorageError::RecoveryCancelled)
    ));
    assert_eq!(r.load_identity().unwrap(), identity());
    let barrier = r
        .try_advance_recovery_barrier(Revision::new(1), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let penalty =
        b"{\"format\":1,\"failures\":9,\"wait_seconds\":16,\"pending\":false,\"repair\":[]}";
    w.handle()
        .try_commit(
            CommitCommand::new(
                "trusted-drain-cleanup".into(),
                Revision::new(2),
                vec![Mutation::Put {
                    key: "business/security/login/1".into(),
                    value: penalty.to_vec(),
                }],
            )
            .unwrap(),
            3,
        )
        .unwrap()
        .wait()
        .unwrap();
    let receipt = r
        .try_prepare_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("candidate.sqlite3"),
            barrier,
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(receipt.current_revision, Revision::new(3));
    assert_eq!(receipt.revision, Revision::new(4));
    let db = Connection::open(t.path("candidate.sqlite3")).unwrap();
    assert_eq!(
        TransactionView(&db)
            .get("business/security/login/1")
            .unwrap()
            .unwrap()
            .value,
        penalty
    );
    drop(db);
    w.shutdown().unwrap();
}

#[test]
fn migration_keeps_an_existing_installation_identifier_and_rejects_corruption() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, r) = open(&t.path("live.sqlite3"));
    assert_eq!(r.ensure_recovery_installation([8; 32]).unwrap(), [7; 32]);
    assert_eq!(r.recovery_installation().unwrap(), [7; 32]);
    w.handle()
        .try_commit(
            CommitCommand::new(
                "corrupt-test-binding".into(),
                Revision::new(1),
                vec![Mutation::Put {
                    key: model::INSTALLATION.into(),
                    value: b"{}".to_vec(),
                }],
            )
            .unwrap(),
            2,
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        r.ensure_recovery_installation([9; 32]),
        Err(StorageError::Database)
    );
    w.shutdown().unwrap();
    assert!(business::startup_state(t.path("live.sqlite3")).is_err());
}

struct VoicePrepared;
impl fc_ports::voice::VoicePreparation for VoicePrepared {
    type Input = ();
    fn prepare(
        &mut self,
        _: (),
        _: &fc_domain::voice::VoiceAsset,
    ) -> Result<fc_domain::voice::VoicePcmMetadata, fc_ports::voice::VoiceRepositoryError> {
        Ok(fc_domain::voice::VoicePcmMetadata {
            byte_length: 16044,
            frame_count: 8000,
            sha256: [4; 32],
        })
    }
}
#[test]
fn ordinary_voice_is_replay_inert_and_restore_keeps_current_voice_sequence_floors() {
    use fc_domain::voice::*;
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, r) = open(&t.path("live.sqlite3"));
    let mut web = actor();
    web.origin = TrustedOrigin::Web;
    let asset = r
        .reserve_voice_upload(
            VoiceAssetId::new(1).unwrap(),
            ClassId::new(1).unwrap(),
            &web,
        )
        .unwrap();
    let prepared = fc_ports::voice::prepare_voice_upload(&mut VoicePrepared, (), asset).unwrap();
    let asset = r.publish_voice_upload(prepared, &web).unwrap();
    let job = r
        .submit_voice(
            fc_ports::voice::VoiceSubmitRequest {
                command_id: CommandId::new(500).unwrap(),
                asset_id: asset.id,
                expected_asset_revision: asset.revision,
                class_id: ClassId::new(1).unwrap(),
                repeat_total: 3,
            },
            &web,
        )
        .unwrap();
    assert_eq!(job.state, VoiceState::Queued);
    let export = r
        .try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    assert!(export.media.is_empty());
    let ordinary = open_ordinary(&t.path("ordinary.sqlite3")).unwrap();
    let voice = business::codec::value(
        &TransactionView(&ordinary)
            .get("business/voice/v1")
            .unwrap()
            .unwrap()
            .value,
    )
    .unwrap();
    assert!(voice["assets"].as_array().unwrap().is_empty());
    assert!(voice["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|j| !j.to_string().contains("queued")));
    drop(ordinary);
    let current = r.emergency_stop_voice().unwrap();
    let revision = w.handle().try_revision().unwrap().wait().unwrap();
    let barrier = r
        .try_advance_recovery_barrier(revision, &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    r.try_prepare_ordinary_restore(
        t.path("ordinary.sqlite3"),
        t.path("candidate.sqlite3"),
        barrier,
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    w.shutdown().unwrap();
    let (cw, restored) = open(&t.path("candidate.sqlite3"));
    let s = restored.voice_inventory().unwrap();
    assert!(s.assets.is_empty());
    assert!(s
        .jobs
        .iter()
        .all(|j| j.state.terminal() && j.authorization.is_none() && j.asset_id.is_none()));
    assert!(s.next_sequence > current.next_sequence);
    assert!(s.next_generation > current.next_generation);
    assert!(s.observed_at >= current.observed_at);
    cw.shutdown().unwrap();
}

#[test]
fn account_display_labels_restore_but_current_login_security_and_grants_do_not_revert() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, r) = open(&t.path("live.sqlite3"));
    r.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let mut changed = identity();
    changed.accounts[0].display_name = "后来改过的名字".into();
    changed.accounts[0].login_name = "current-login".into();
    changed.accounts[0].principal.auth_epoch = AuthEpoch::new(20);
    changed.accounts[0].must_change_password = false;
    changed.accounts[0].credential_version = 2;
    changed.accounts[0].recovery_version = 2;
    changed.revision = Revision::new(2);
    changed.assignments.clear();
    changed.validate().unwrap();
    let changes = vec![
        Mutation::Put {
            key: "business/identity/v1".into(),
            value: business::codec::bytes(&business::codec::identity(&changed)).unwrap(),
        },
        Mutation::Put {
            key: "business/credential/1/password/2".into(),
            value: b"CURRENT-PASSWORD".to_vec(),
        },
        Mutation::Put {
            key: "business/credential/1/recovery/2".into(),
            value: b"CURRENT-RECOVERY".to_vec(),
        },
    ];
    w.handle()
        .try_commit(
            CommitCommand::new("current-security-change".into(), Revision::new(1), changes)
                .unwrap(),
            2,
        )
        .unwrap()
        .wait()
        .unwrap();
    let mut fresh = actor();
    fresh.auth_epoch = AuthEpoch::new(20);
    let barrier = r
        .try_advance_recovery_barrier(Revision::new(2), &fresh, fence())
        .unwrap()
        .wait()
        .unwrap();
    r.try_prepare_ordinary_restore(
        t.path("ordinary.sqlite3"),
        t.path("candidate.sqlite3"),
        barrier,
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    let got = business::startup_state(t.path("candidate.sqlite3"))
        .unwrap()
        .identity;
    assert_eq!(
        got.accounts[0].display_name,
        identity().accounts[0].display_name
    );
    assert_eq!(got.accounts[0].login_name, "current-login");
    assert_eq!(got.accounts[0].credential_version, 2);
    assert_eq!(got.accounts[0].recovery_version, 2);
    assert_eq!(got.accounts[0].principal.auth_epoch, AuthEpoch::new(22));
    assert!(got.assignments.is_empty());
    let db = Connection::open(t.path("candidate.sqlite3")).unwrap();
    assert_eq!(
        TransactionView(&db)
            .get("business/credential/1/password/2")
            .unwrap()
            .unwrap()
            .value,
        b"CURRENT-PASSWORD"
    );
    drop(db);
    w.shutdown().unwrap();
}

#[test]
fn retained_lesson_audit_survives_a_second_complete_restore_with_an_empty_live_lesson() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let time = |second: u64| json!({"date":[2026,10,31],"second":second});
    let cell = json!({"subject":1,"teacher":{"id":1}});
    let key = json!({"date":[2026,10,31],"class":1,"begin":540,"end":585,"version":{"actual_semester":1,"actual_revision":1,"source_semester":1,"source_revision":1,"mode":"school","mode_revision":1,"applied_revision":1,"mapping_revision":1,"table":1,"row":1,"rules_revision":1}});
    let context = json!({"key":key,"base":cell,"effective":cell,"teacher":1,"from":time(540*60),"until":time(585*60)});
    let audit = json!({"format":2,"revision":1,"identity_revision":1,"observed_at":time(10*3600),"class":1,"blocked_until":null,"current":null,"override":null,"history":[{"context":context,"ended":time(585*60)}],"base_trust":null});
    let db = Connection::open(t.path("live.sqlite3")).unwrap();
    write_row(
        &db,
        "business/recovery/lesson-audit/v1",
        &serde_json::to_vec(&audit).unwrap(),
        Revision::new(1),
    )
    .unwrap();
    drop(db);
    let (w, r) = open(&t.path("live.sqlite3"));
    r.try_export_ordinary(t.path("first.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let barrier = r
        .try_advance_recovery_barrier(Revision::new(1), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    r.try_prepare_ordinary_restore(
        t.path("first.sqlite3"),
        t.path("candidate1.sqlite3"),
        barrier,
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    w.shutdown().unwrap();
    let (w, r) = open(&t.path("candidate1.sqlite3"));
    let mut fresh = actor();
    fresh.auth_epoch = AuthEpoch::new(9);
    let export = r
        .try_export_ordinary(t.path("second.sqlite3"), &fresh, fence())
        .unwrap()
        .wait()
        .unwrap();
    let barrier = r
        .try_advance_recovery_barrier(export.current_revision, &fresh, fence())
        .unwrap()
        .wait()
        .unwrap();
    r.try_prepare_ordinary_restore(
        t.path("second.sqlite3"),
        t.path("candidate2.sqlite3"),
        barrier,
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    w.shutdown().unwrap();
    let db = Connection::open(t.path("candidate2.sqlite3")).unwrap();
    let retained = business::codec::value(
        &TransactionView(&db)
            .get("business/recovery/lesson-audit/v1")
            .unwrap()
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(retained["history"], audit["history"]);
    assert!(retained["current"].is_null());
    assert!(retained["override"].is_null());
    let live = business::codec::value(
        &TransactionView(&db)
            .get("business/lesson-context/v1")
            .unwrap()
            .unwrap()
            .value,
    )
    .unwrap();
    assert!(live["history"].as_array().unwrap().is_empty());
    assert!(live["current"].is_null());
    drop(db);
    business::startup_state(t.path("candidate2.sqlite3")).unwrap();
}

#[test]
fn wal_header_large_values_and_cancellation_fail_closed_without_sidecars_or_candidate() {
    use std::io::{Seek, Write};
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (w, r) = open(&t.path("live.sqlite3"));
    r.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    std::fs::copy(t.path("ordinary.sqlite3"), t.path("wal.sqlite3")).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(t.path("wal.sqlite3"))
        .unwrap();
    file.seek(std::io::SeekFrom::Start(18)).unwrap();
    file.write_all(&[2, 2]).unwrap();
    drop(file);
    assert!(matches!(
        open_ordinary(&t.path("wal.sqlite3")),
        Err(StorageError::RecoveryInvalidPackage)
    ));
    assert!(!t.path("wal.sqlite3-shm").exists());
    assert!(!t.path("wal.sqlite3-wal").exists());
    let db = Connection::open(t.path("ordinary.sqlite3")).unwrap();
    db.execute("UPDATE documents SET value=zeroblob(1048576)", [])
        .unwrap();
    drop(db);
    assert!(open_ordinary(&t.path("ordinary.sqlite3")).is_err());
    assert!(matches!(
        r.try_preflight_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("preview.sqlite3"),
            &actor(),
            Arc::new(|| false)
        )
        .unwrap()
        .wait(),
        Err(StorageError::RecoveryCancelled)
    ));
    assert!(!t.path("preview.sqlite3").exists());
    assert_eq!(
        w.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(1)
    );
    w.shutdown().unwrap();
}

struct CredentialFixture;
impl fc_ports::CredentialCrypto for CredentialFixture {
    type Secret = ();
    type PasswordRecord = Vec<u8>;
    type RecoveryCode = ();
    type RecoveryRecord = Vec<u8>;
    fn password_record(&mut self, _: &()) -> Result<Vec<u8>, fc_ports::CredentialError> {
        Ok(b"synthetic-protected-password".to_vec())
    }
    fn verify_password(&mut self, _: &(), _: &Vec<u8>) -> Result<bool, fc_ports::CredentialError> {
        Ok(true)
    }
    fn recovery_record(&mut self) -> Result<((), Vec<u8>), fc_ports::CredentialError> {
        Ok(((), b"synthetic-protected-recovery".to_vec()))
    }
    fn verify_recovery(&mut self, _: &(), _: &Vec<u8>) -> Result<bool, fc_ports::CredentialError> {
        Ok(true)
    }
}
#[test]
fn first_registration_commits_installation_binding_and_both_credential_records_atomically() {
    use fc_ports::CredentialIdentityRepository;
    let t = Temp::new();
    let (w, r) = open(&t.path("live.sqlite3"));
    let request = fc_ports::IdentityRequest {
        command_id: CommandId::new(1).unwrap(),
        expected_revision: Revision::default(),
        command: IdentityCommand::FirstAdministrator {
            school_id: SchoolId::new(1).unwrap(),
            school_name: "新部署".into(),
            id: PrincipalId::new(1).unwrap(),
            login_name: "admin".into(),
            display_name: "主管理员".into(),
        },
    };
    let (mut bad, mut good) = (
        r.clone().with_initial_installation_identity([0; 32]),
        r.clone().with_initial_installation_identity([7; 32]),
    );
    let (change, _) =
        fc_ports::prepare_initial_credentials(&mut CredentialFixture, &(), request.clone())
            .unwrap();
    assert!(bad.execute_credentials(change, None, &TestClock).is_err());
    assert!(r.load_identity().unwrap().installation.is_none());
    assert_eq!(
        w.handle().try_revision().unwrap().wait().unwrap(),
        Revision::default()
    );
    assert!(w
        .handle()
        .try_get("business/credential/1/password/1".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    let (change, _) =
        fc_ports::prepare_initial_credentials(&mut CredentialFixture, &(), request).unwrap();
    good.execute_credentials(change, None, &TestClock).unwrap();
    assert_eq!(r.recovery_installation().unwrap(), [7; 32]);
    assert_eq!(
        w.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(1)
    );
    for kind in ["password", "recovery"] {
        assert!(w
            .handle()
            .try_get(format!("business/credential/1/{kind}/1"))
            .unwrap()
            .wait()
            .unwrap()
            .is_some());
    }
    w.shutdown().unwrap();
    business::startup_state(t.path("live.sqlite3")).unwrap();
}

#[test]
fn cancelled_in_progress_export_removes_its_partial_file_and_never_changes_live_revision() {
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 20);
    let (w, r) = open(&t.path("live.sqlite3"));
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    assert_eq!(
        r.try_export_ordinary(
            t.path("cancelled.sqlite3"),
            &actor(),
            Arc::new(move || c.fetch_add(1, Ordering::SeqCst) < 3)
        )
        .unwrap()
        .wait(),
        Err(StorageError::RecoveryCancelled)
    );
    assert!(!t.path("cancelled.sqlite3").exists());
    assert_eq!(
        w.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(1)
    );
    assert_eq!(r.load_identity().unwrap(), identity());
    w.shutdown().unwrap();
}
