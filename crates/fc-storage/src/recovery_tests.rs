use super::*;
use crate::business::{BusinessRepository, SessionGuard};
use fc_domain::profile::*;
use fc_domain::{auth::*, identity::*, ids::*};
use fc_domain::{layout::*, materials::*, profile_content::*};
use fc_ports::voice::VoiceRepository;
use fc_ports::{
    MaterialOutcomeRepository, MaterialRepository, ProfileContentRepository, ProfileRepository,
};
use std::sync::atomic::AtomicU64;
#[path = "migration_tests.rs"]
mod migration_tests;
#[path = "recovery_voice_policy_tests.rs"]
mod recovery_voice_policy_tests;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf, crate::fixture_commands::FixtureCommands);
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
        Self(p, Default::default())
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
// This is a caller fixture adapter, not an alternate Writer or business gate.
// All payload/CAS/authentication checks below still run in BusinessRepository.
struct TestRepository {
    repository: BusinessRepository<u64>,
    handle: StorageHandle<u64>,
    commands: crate::fixture_commands::FixtureCommands,
}
impl std::ops::Deref for TestRepository {
    type Target = BusinessRepository<u64>;
    fn deref(&self) -> &Self::Target {
        &self.repository
    }
}
impl std::ops::DerefMut for TestRepository {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.repository
    }
}
impl TestRepository {
    fn issue(
        &self,
        family: fc_ports::command_window::CommandFamily,
        label: &str,
        who: Option<&ActorContext>,
    ) -> fc_ports::command_window::IssuedCommand {
        let owner = who.map_or(
            fc_ports::command_window::CommandOwner::PublicDesktop,
            fc_ports::command_window::CommandOwner::authenticated,
        );
        self.commands
            .issue(&self.handle, family, label, owner)
            .unwrap()
    }
    fn original(&self, id: CommandId) -> BusinessRepository<u64> {
        self.repository
            .with_command_window(self.commands.original(id).unwrap())
    }
    fn execute_public_layout(
        &mut self,
        mut request: fc_ports::PublicLayoutRequest,
        who: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<fc_ports::PublicLayoutState, fc_ports::ProfileRepositoryError> {
        let issued = self.issue(
            fc_ports::command_window::CommandFamily::Profiles,
            &request.command_id.get().to_string(),
            Some(who),
        );
        request.command_id = issued.command_id;
        self.repository
            .with_command_window(issued)
            .execute_public_layout(request, who, clock)
    }
    fn execute_subject_profile(
        &mut self,
        mut request: fc_ports::ProfileRequest,
        who: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<SubjectProfileDocument, fc_ports::ProfileRepositoryError> {
        let issued = self.issue(
            fc_ports::command_window::CommandFamily::Profiles,
            &request.command_id.get().to_string(),
            Some(who),
        );
        request.command_id = issued.command_id;
        self.repository
            .with_command_window(issued)
            .execute_subject_profile(request, who, clock)
    }
    fn begin_profile_content(
        &mut self,
        mut request: BeginContentRequest,
        who: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<ContentAdmission, fc_ports::ProfileContentRepositoryError> {
        let issued = self.issue(
            fc_ports::command_window::CommandFamily::ProfileContent,
            &request.command_id.get().to_string(),
            Some(who),
        );
        request.command_id = issued.command_id;
        self.repository
            .with_command_window(issued)
            .begin_profile_content(request, who, clock)
    }
    fn reserve_profile_media(
        &self,
        admission: &ContentAdmission,
        bytes: &[u64],
        who: &ActorContext,
    ) -> Result<business::profile_content::MediaReservation, fc_ports::ProfileContentRepositoryError>
    {
        self.original(admission.request.command_id)
            .reserve_profile_media(admission, bytes, who)
    }
    fn commit_profile_content(
        &mut self,
        prepared: fc_ports::PreparedProfileContent,
        who: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<PublishedContentSnapshot, fc_ports::ProfileContentRepositoryError> {
        self.original(prepared.admission().request.command_id)
            .commit_profile_content(prepared, who, clock)
    }
    fn append_material(
        &mut self,
        prepared: fc_ports::PreparedMaterialAppend,
        who: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<MaterialRecord, fc_ports::MaterialRepositoryError> {
        self.original(prepared.request().command_id)
            .append_material(prepared, who, clock)
    }
    fn submit_voice(
        &self,
        mut request: fc_ports::voice::VoiceSubmitRequest,
        who: &ActorContext,
    ) -> Result<fc_domain::voice::VoiceJob, fc_ports::voice::VoiceRepositoryError> {
        let issued = self.issue(
            fc_ports::command_window::CommandFamily::Voice,
            &request.command_id.get().to_string(),
            Some(who),
        );
        request.command_id = issued.command_id;
        self.repository
            .with_command_window(issued)
            .submit_voice(request, who)
    }
    fn set_voice_limits(
        &self,
        limits: fc_domain::voice::VoiceLimits,
        expected: Revision,
        who: &ActorContext,
    ) -> Result<fc_domain::voice::VoiceSnapshot, fc_ports::voice::VoiceRepositoryError> {
        let issued = self.issue(
            fc_ports::command_window::CommandFamily::Voice,
            &format!("limits-{}", expected.get()),
            Some(who),
        );
        self.repository
            .with_command_window(issued)
            .set_voice_limits(limits, expected, who)
    }
    fn set_voice_enabled(
        &self,
        enabled: bool,
        expected: Revision,
        who: &ActorContext,
    ) -> Result<fc_domain::voice::VoiceSnapshot, fc_ports::voice::VoiceRepositoryError> {
        let issued = self.issue(
            fc_ports::command_window::CommandFamily::Voice,
            &format!("enabled-{}", expected.get()),
            Some(who),
        );
        self.repository
            .with_command_window(issued)
            .set_voice_enabled(enabled, expected, who)
    }
    fn try_advance_recovery_barrier(
        &self,
        expected: Revision,
        who: &ActorContext,
        fence: ExecutionFence,
    ) -> Result<Ticket<RecoveryBarrier>, StorageError> {
        let issued = self.issue(
            fc_ports::command_window::CommandFamily::Generic,
            &format!("barrier-{}", expected.get()),
            Some(who),
        );
        self.repository
            .with_command_window(issued)
            .try_advance_recovery_barrier(expected, who, fence)
    }
}
fn open(path: &Path) -> (StorageWriter<u64>, TestRepository) {
    let w = StorageWriter::open(path, 8, Publisher).unwrap();
    let r = BusinessRepository::new(
        w.handle(),
        Arc::new(TestClock),
        Arc::new(|_, r| Ok(r.get())),
    )
    .with_session_guard(guard());
    let handle = w.handle();
    (
        w,
        TestRepository {
            repository: r,
            handle,
            commands: Default::default(),
        },
    )
}
fn fence() -> ExecutionFence {
    Arc::new(|| true)
}
fn image_slot(r: &mut TestRepository) -> ContentSlotKey {
    image_slot_for(r, &actor(), 100)
}
fn image_slot_for(r: &mut TestRepository, who: &ActorContext, command: u64) -> ContentSlotKey {
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
            command_id: r
                .issue(
                    fc_ports::command_window::CommandFamily::Materials,
                    "material-111",
                    Some(&actor()),
                )
                .command_id,
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
    r: &mut TestRepository,
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
fn publish_image(r: &mut TestRepository, slot: ContentSlotKey, command: u64, rev: u64) {
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
            command_id: r
                .issue(
                    fc_ports::command_window::CommandFamily::Materials,
                    "material-101",
                    Some(&actor()),
                )
                .command_id,
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
    // The source backup captured the pending metadata above. Its synchronous
    // trusted preparation owner has now returned; it created no native file.
    r.original(pending.request.command_id)
        .fail_profile_content(pending, &actor(), &TestClock)
        .unwrap();
    r.settle_profile_media(reservation).unwrap();
    let revision = w.handle().try_revision().unwrap().wait().unwrap();
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
    // Physical C2 history may be removed. The persisted epoch/watermark must
    // still reject the full original pairs through the real public read gates.
    let retired = [
        (
            "business/profile-content/v1/receipts/00000000000000000103",
            r.commands
                .labelled(
                    fc_ports::command_window::CommandFamily::ProfileContent,
                    "103",
                )
                .unwrap(),
        ),
        (
            "business/material/v1/receipts/00000000000000000101",
            r.commands
                .labelled(
                    fc_ports::command_window::CommandFamily::Materials,
                    "material-101",
                )
                .unwrap(),
        ),
    ];
    for (legacy, issued) in retired {
        let (slot, _) = business::command_window::legacy(legacy).unwrap();
        let key = business::command_window::namespaced(slot, issued.ticket);
        assert_eq!(TransactionView(&candidate).get(&key), Ok(None));
    }
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
    for (_, issued) in retired {
        assert_eq!(
            cw.handle().try_check_command(issued).unwrap().wait(),
            Err(StorageError::CommandExpired)
        );
    }
    let profile_issued = retired[0].1;
    assert_eq!(
        restored
            .repository
            .with_command_window(profile_issued)
            .profile_content_outcome(profile_issued.command_id, &fresh, &TestClock),
        Err(fc_ports::ProfileContentRepositoryError::CommandWindow(
            fc_ports::command_window::CommandWindowError::Expired
        ))
    );
    let material_issued = retired[1].1;
    assert_eq!(
        restored
            .repository
            .with_command_window(material_issued)
            .material_outcome(material_issued.command_id, &fresh),
        Err(fc_ports::MaterialRepositoryError::CommandWindow(
            fc_ports::command_window::CommandWindowError::Expired
        ))
    );
    let ad = begin_image(&mut restored, slot, 104, 2, &fresh);
    let next = restored.reserve_profile_media(&ad, &[4], &fresh).unwrap();
    assert_eq!(next.ids[0].get(), 4);
    // A real new C2 commit must update the restored current/previous entries.
    // They have recovery-owner markers, intentionally no old reservations.
    let resource = MediaReference {
        version_id: next.ids[0],
        owner: slot,
        sha256: Sha256::digest(b"test").into(),
        bytes: 4,
        width: 1,
        height: 1,
        format: ImageFormat::Png,
    };
    let original = ad.request.command_id;
    let proof = fc_ports::prepare_profile_content(
        &mut Prepared(ContentPayload::Images(vec![resource.clone()])),
        ad,
        (),
    )
    .unwrap();
    let committed = restored
        .commit_profile_content(proof, &fresh, &TestClock)
        .unwrap();
    assert_eq!(committed.revision, Revision::new(3));
    let document = restored.profile_content(slot, &fresh).unwrap();
    assert_eq!(
        document.current.as_ref().unwrap().payload,
        ContentPayload::Images(vec![resource])
    );
    assert_eq!(
        document.previous.as_ref().unwrap().revision,
        Revision::new(2)
    );
    assert!(document.pending.is_none());
    assert_eq!(
        restored
            .original(original)
            .profile_content_outcome(original, &fresh, &TestClock)
            .unwrap()
            .unwrap()
            .status,
        fc_ports::ProfileContentStatus::Committed
    );
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
    let web_issued = r.issue(
        fc_ports::command_window::CommandFamily::Generic,
        "wrong-origin-barrier",
        Some(&web),
    );
    assert!(r
        .repository
        .with_command_window(web_issued)
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
    t.1.commit(
        &w.handle(),
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
    t.1.commit(
        &w.handle(),
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
    let stopped = r.emergency_stop_voice().unwrap();
    let limits = VoiceLimits {
        max_duration_millis: 1_000,
        max_pending_jobs: 1,
        max_pending_per_owner: 1,
        max_repeat_total: 1,
    };
    let changed = r
        .set_voice_limits(limits, stopped.revision, &actor())
        .unwrap();
    let current = r
        .set_voice_enabled(false, changed.revision, &actor())
        .unwrap();
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
    assert!(
        !s.enabled,
        "an older teaching backup cannot reenable target voice"
    );
    assert_eq!(
        s.limits, limits,
        "current target policy cannot be widened by restore"
    );
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
    t.1.commit(
        &w.handle(),
        CommitCommand::new("current-security-change".into(), Revision::new(1), changes).unwrap(),
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
    let (w, mut r) = open(&t.path("live.sqlite3"));
    let registration = r.issue(
        fc_ports::command_window::CommandFamily::Identity,
        "registration",
        None,
    );
    let request = fc_ports::IdentityRequest {
        command_id: registration.command_id,
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
        r.repository
            .with_command_window(registration)
            .with_initial_installation_identity([0; 32]),
        r.repository
            .with_command_window(registration)
            .with_initial_installation_identity([7; 32]),
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
    for key in [
        "business/display/v1",
        "business/profiles/v1/public",
        "business/profiles/v1/manifest",
    ] {
        assert!(w
            .handle()
            .try_get(key.into())
            .unwrap()
            .wait()
            .unwrap()
            .is_none());
    }
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
    let registered = r.load_identity().unwrap();
    assert!(registered.classes.is_empty());
    assert!(registered.subjects.is_empty());
    let who = ActorContext {
        auth_epoch: AuthEpoch::new(1),
        ..actor()
    };
    let preset = r.public_layout(&who).unwrap();
    let mut expected = fc_domain::desktop::first_install_display_settings();
    expected.revision = Revision::new(1);
    assert_eq!(preset.display, expected);
    assert_eq!(preset.layout.tree, first_install_public_layout());
    assert_eq!(preset.layout.revision, Revision::new(1));
    assert_eq!(
        preset.layout.node_id_high_water,
        preset.layout.tree.maximum_id()
    );
    assert!(preset.layout.content_styles.is_empty());
    assert!(preset.display.target_monitor.is_none());
    assert!(preset.display.countdown.target.is_none());
    let mut custom = preset.clone();
    custom.display.course_fill_percent = 31;
    custom.layout.tree.canvas_frames[0].x = 0;
    let saved = r
        .execute_public_layout(
            fc_ports::PublicLayoutRequest {
                command_id: CommandId::new(9).unwrap(),
                expected_layout_revision: preset.layout.revision,
                expected_display_revision: preset.display.revision,
                layout: custom.layout,
                display: custom.display,
            },
            &who,
            &TestClock,
        )
        .unwrap();
    w.shutdown().unwrap();
    business::startup_state(t.path("live.sqlite3")).unwrap();
    let (reopened, repository) = open(&t.path("live.sqlite3"));
    assert_eq!(
        repository
            .repository
            .with_initial_installation_identity([7; 32])
            .public_layout(&who)
            .unwrap(),
        saved
    );
    reopened.shutdown().unwrap();
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

#[test]
fn command_window_repeated_real_restore_does_not_revive_old_tickets_after_failure_or_reopen() {
    use fc_domain::content::*;
    use fc_ports::{command_window::*, ContentRepository};
    fn issue(
        w: &StorageWriter<u64>,
        family: CommandFamily,
        actor: &ActorContext,
        raw: u64,
    ) -> IssuedCommand {
        let mut entropy = [11; 24];
        entropy[16..].copy_from_slice(&raw.to_be_bytes());
        w.handle()
            .try_issue_command(crate::command_window::IssueCommand {
                family,
                owner: CommandOwner::authenticated(actor),
                entropy,
            })
            .unwrap()
            .wait()
            .unwrap()
    }
    fn fresh_actor(r: &BusinessRepository<u64>) -> ActorContext {
        let mut a = actor();
        a.auth_epoch = r.load_identity().unwrap().accounts[0].principal.auth_epoch;
        a
    }
    fn add_current(
        w: &StorageWriter<u64>,
        r: &BusinessRepository<u64>,
        raw: u64,
        text: &str,
    ) -> IssuedCommand {
        let actor = fresh_actor(r);
        let issued = issue(w, CommandFamily::Content, &actor, raw);
        r.with_command_window(issued)
            .execute(
                ContentCommand {
                    command_id: issued.command_id,
                    key: ContentKey::Homework(HomeworkKey {
                        class_id: ClassId::new(1).unwrap(),
                        actual_date: TestClock.now().date,
                        subject_id: SubjectId::new(1).unwrap(),
                        owner_teacher_id: PrincipalId::new(1).unwrap(),
                    }),
                    expected_revision: Revision::default(),
                    text: text.into(),
                },
                &actor,
                &TestClock,
            )
            .unwrap();
        issued
    }
    fn barrier(w: &StorageWriter<u64>, r: &BusinessRepository<u64>, raw: u64) -> RecoveryBarrier {
        let actor = fresh_actor(r);
        let issued = issue(w, CommandFamily::Generic, &actor, raw);
        let revision = w.handle().try_revision().unwrap().wait().unwrap();
        r.with_command_window(issued)
            .try_advance_recovery_barrier(revision, &actor, fence())
            .unwrap()
            .wait()
            .unwrap()
    }
    let t = Temp::new();
    let live = t.path("live.sqlite3");
    seed(&live, 1);
    let (w, r) = open(&live);
    r.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let first = add_current(&w, &r, 101, "written after the backup");
    let failed_barrier = barrier(&w, &r, 102);
    assert_eq!(
        w.handle().try_check_command(first).unwrap().wait(),
        Err(StorageError::CommandExpired)
    );
    assert!(r
        .try_prepare_ordinary_restore(
            t.path("missing.sqlite3"),
            t.path("must-not-exist.sqlite3"),
            failed_barrier,
            fence()
        )
        .unwrap()
        .wait()
        .is_err());
    assert!(!t.path("must-not-exist.sqlite3").exists());
    // Failed candidate preparation cannot roll back the original live barrier.
    w.shutdown().unwrap();
    let (w, r) = open(&live);
    assert_eq!(
        w.handle().try_check_command(first).unwrap().wait(),
        Err(StorageError::CommandExpired)
    );
    let current = w
        .handle()
        .try_get("business/homework/1/2026-10-31/1/1".into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8(current.value)
        .unwrap()
        .contains("written after the backup"));
    let retry = barrier(&w, &r, 103);
    r.try_prepare_ordinary_restore(
        t.path("ordinary.sqlite3"),
        t.path("restored-first.sqlite3"),
        retry,
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    w.shutdown().unwrap();
    // Open through the actual Writer/startup path. This exercises storage
    // restore activation in an owned host fixture, not Win32 cold-switch/audio.
    business::startup_state(t.path("restored-first.sqlite3")).unwrap();
    let (w, r) = open(&t.path("restored-first.sqlite3"));
    assert_eq!(
        w.handle().try_check_command(first).unwrap().wait(),
        Err(StorageError::CommandExpired)
    );
    assert!(w
        .handle()
        .try_get("business/homework/1/2026-10-31/1/1".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    let original = w
        .handle()
        .try_get("business/homework/1/2000-01-01/1/1".into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8(original.value)
        .unwrap()
        .contains(&"x".repeat(2300)));
    let second = add_current(&w, &r, 104, "issued and written after the first restore");
    assert_ne!(first.ticket.epoch(), second.ticket.epoch());
    let again = barrier(&w, &r, 105);
    r.try_prepare_ordinary_restore(
        t.path("ordinary.sqlite3"),
        t.path("restored-second.sqlite3"),
        again,
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    w.shutdown().unwrap();
    business::startup_state(t.path("restored-second.sqlite3")).unwrap();
    let (w, _r) = open(&t.path("restored-second.sqlite3"));
    for old in [first, second] {
        assert_eq!(
            w.handle().try_check_command(old).unwrap().wait(),
            Err(StorageError::CommandExpired)
        );
    }
    assert!(w
        .handle()
        .try_get("business/homework/1/2026-10-31/1/1".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    assert!(w
        .handle()
        .try_get("business/homework/1/2000-01-01/1/1".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_some());
    // No fixture ever declared an OS/audio owner returned; there were none.
    w.shutdown().unwrap();
}

#[test]
fn command_window_preview_keeps_live_owner_and_confirmed_restore_waits_for_original_settlement() {
    use fc_ports::command_window::*;
    fn issue(
        w: &StorageWriter<u64>,
        family: CommandFamily,
        actor: &ActorContext,
        raw: u64,
    ) -> IssuedCommand {
        let mut entropy = [13; 24];
        entropy[16..].copy_from_slice(&raw.to_be_bytes());
        w.handle()
            .try_issue_command(crate::command_window::IssueCommand {
                family,
                owner: CommandOwner::authenticated(actor),
                entropy,
            })
            .unwrap()
            .wait()
            .unwrap()
    }
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 1);
    let (w, mut r) = open(&t.path("live.sqlite3"));
    r.try_export_ordinary(t.path("ordinary.sqlite3"), &actor(), fence())
        .unwrap()
        .wait()
        .unwrap();
    let slot = image_slot_for(&mut r, &actor(), 201);
    let content = issue(&w, CommandFamily::ProfileContent, &actor(), 202);
    let mut scoped = r.with_command_window(content);
    let (admission, owner) = scoped
        .begin_profile_content_owned(
            BeginContentRequest {
                command_id: content.command_id,
                slot,
                expected_revision: Revision::default(),
                operation: ContentOperation::Publish(ContentPayloadKind::Images),
                payload_fingerprint: [27; 32],
                expires_at: LocalDateTime::new(TestClock.now().date, 12 * 3600).unwrap(),
            },
            &actor(),
            &TestClock,
        )
        .unwrap();
    let reservation = scoped
        .reserve_profile_media(&admission, &[7], &actor())
        .unwrap();
    // This fixture owns metadata reservations only; no OS file/audio owner is
    // fabricated. Its original full AD/id/size identity remains authoritative.
    r.try_preflight_ordinary_restore(
        t.path("ordinary.sqlite3"),
        t.path("preview.sqlite3"),
        &actor(),
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    let binding = crate::business::command_window::slots()
        .iter()
        .find(|s| s.prefix == "business/profile-content/v1/receipts/")
        .unwrap();
    let key = crate::business::command_window::namespaced(*binding, content.ticket);
    let before = w
        .handle()
        .try_get(key.clone())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(
        business::codec::value(&before).unwrap()["status"],
        "accepted"
    );
    let issued = issue(&w, CommandFamily::Generic, &actor(), 203);
    let barrier = r
        .with_command_window(issued)
        .try_advance_recovery_barrier(
            w.handle().try_revision().unwrap().wait().unwrap(),
            &actor(),
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        r.try_prepare_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("denied.sqlite3"),
            barrier,
            fence()
        )
        .unwrap()
        .wait()
        .unwrap_err(),
        StorageError::RecoveryConflict
    );
    assert!(!t.path("denied.sqlite3").exists());
    assert_eq!(
        w.handle()
            .try_get(key.clone())
            .unwrap()
            .wait()
            .unwrap()
            .unwrap()
            .value,
        before
    );
    // Losing permission/client lifetime cannot suppress this exact negative
    // receipt, and failing intent does not release a reserved media owner.
    assert_eq!(
        r.fail_profile_content_owned(owner).unwrap().status,
        fc_ports::ProfileContentStatus::Failed
    );
    let mut actor = actor();
    actor.auth_epoch = r.load_identity().unwrap().accounts[0].principal.auth_epoch;
    let issued = issue(&w, CommandFamily::Generic, &actor, 204);
    let barrier = r
        .with_command_window(issued)
        .try_advance_recovery_barrier(
            w.handle().try_revision().unwrap().wait().unwrap(),
            &actor,
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        r.try_prepare_ordinary_restore(
            t.path("ordinary.sqlite3"),
            t.path("still-denied.sqlite3"),
            barrier,
            fence()
        )
        .unwrap()
        .wait()
        .unwrap_err(),
        StorageError::RecoveryConflict
    );
    r.settle_profile_media(reservation).unwrap();
    // Settle records worker return only. Resource quota remains charged and the
    // original entry is Unreferenced until physical-delete/finalize, not Removed.
    let inventory = r.inventory_profile_media(None, 16).unwrap();
    assert_eq!(
        inventory.items[0].state,
        business::profile_content::MediaState::Unreferenced
    );
    assert_eq!(inventory.items[0].charged_bytes, 7);
    actor.auth_epoch = r.load_identity().unwrap().accounts[0].principal.auth_epoch;
    let issued = issue(&w, CommandFamily::Generic, &actor, 205);
    let barrier = r
        .with_command_window(issued)
        .try_advance_recovery_barrier(
            w.handle().try_revision().unwrap().wait().unwrap(),
            &actor,
            fence(),
        )
        .unwrap()
        .wait()
        .unwrap();
    r.try_prepare_ordinary_restore(
        t.path("ordinary.sqlite3"),
        t.path("allowed.sqlite3"),
        barrier,
        fence(),
    )
    .unwrap()
    .wait()
    .unwrap();
    business::startup_state(t.path("allowed.sqlite3")).unwrap();
    w.shutdown().unwrap();
}

#[test]
fn command_window_autostart_original_pair_and_owner_return_survive_expiry_and_unavailable_clock() {
    use fc_ports::{autostart::*, command_window::*};
    use std::sync::atomic::AtomicBool;
    struct WindowTime {
        now: AtomicU64,
        unavailable: AtomicBool,
    }
    impl fc_ports::MonotonicClock for WindowTime {
        fn monotonic_millis(&self) -> u64 {
            self.now.load(Ordering::SeqCst)
        }
    }
    impl crate::command_window::WindowClock for WindowTime {
        fn wall_millis(&self) -> Result<u64, StorageError> {
            if self.unavailable.load(Ordering::SeqCst) {
                Err(StorageError::CommandWindowClock)
            } else {
                Ok(self.now.load(Ordering::SeqCst))
            }
        }
        fn monotonic_millis(&self) -> u64 {
            self.now.load(Ordering::SeqCst)
        }
    }
    fn issue(w: &StorageWriter<u64>) -> IssuedCommand {
        let mut entropy = [19; 24];
        entropy[16..].copy_from_slice(&301u64.to_be_bytes());
        w.handle()
            .try_issue_command(crate::command_window::IssueCommand {
                family: CommandFamily::Autostart,
                owner: CommandOwner::authenticated(&actor()),
                entropy,
            })
            .unwrap()
            .wait()
            .unwrap()
    }
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let clock = Arc::new(WindowTime {
        now: AtomicU64::new(1000),
        unavailable: AtomicBool::new(false),
    });
    let w = StorageWriter::open_with_clocks(
        t.path("live.sqlite3"),
        8,
        Publisher,
        clock.clone(),
        clock.clone(),
    )
    .unwrap();
    let r = BusinessRepository::new(
        w.handle(),
        Arc::new(TestClock),
        Arc::new(|_, revision| Ok(revision.get())),
    )
    .with_session_guard(guard());
    let original = issue(&w);
    let request = AutostartRequest {
        command_id: original.command_id,
        expected_revision: Revision::default(),
        desired: true,
        target_command: r#""C:\FutureClassroom\fc-bootstrap.exe" --autostart"#.into(),
        previous_observed: AutostartObservation::Absent,
    };
    let operation = r
        .with_command_window(original)
        .begin_autostart(request.clone(), &actor())
        .unwrap();
    clock.now.fetch_add(COMMAND_WINDOW_MILLIS, Ordering::SeqCst);
    assert_eq!(
        w.handle().try_check_command(original).unwrap().wait(),
        Err(StorageError::CommandExpired)
    );
    clock.unavailable.store(true, Ordering::SeqCst);
    // No native write was started in this fixture. An original factual failure
    // may still settle Prepared after both TTL and clock availability are lost.
    let settled = r
        .clone()
        .finish_autostart(
            operation.ticket.clone(),
            AutostartSystemOutcome {
                observation: AutostartObservation::Absent,
                mutated: false,
                failure: Some(AutostartFailure::ReadbackMismatch),
            },
        )
        .unwrap();
    assert_eq!(settled.current().unwrap().phase, AutostartPhase::Failed);
    clock.unavailable.store(false, Ordering::SeqCst);
    let next = issue(&w); // Deliberately same raw id; production uses SystemRandom.
    assert_eq!(next.command_id, original.command_id);
    assert_ne!(next.ticket, original.ticket);
    assert_eq!(
        r.with_command_window(next)
            .begin_autostart(request, &actor()),
        Err(AutostartRepositoryError::CommandWindow(
            CommandWindowError::NotIssued
        ))
    );
    assert_eq!(
        r.with_command_window(next)
            .mark_autostart_applying(operation.ticket, &actor()),
        Err(AutostartRepositoryError::CommandWindow(
            CommandWindowError::NotIssued
        ))
    );
    assert_eq!(
        r.autostart_status(&actor())
            .unwrap()
            .current()
            .unwrap()
            .phase,
        AutostartPhase::Failed
    );
    w.shutdown().unwrap();
}

#[test]
fn profile_window_owner_migration_preserves_full_c2_and_two_slot_local_sequences() {
    use fc_ports::command_window::{CommandFamily, CommandOwner};
    use fc_ports::ProfileContentRepositoryError;
    let t = Temp::new();
    seed(&t.path("live.sqlite3"), 0);
    let (writer, mut repo) = open(&t.path("live.sqlite3"));
    let first_slot = image_slot_for(&mut repo, &actor(), 310);
    // Add a sibling image, not another nested glass container.
    let state = repo.public_layout(&actor()).unwrap();
    let mut layout = state.layout;
    let node = layout.allocate_id().unwrap();
    layout
        .tree
        .nodes
        .iter_mut()
        .find(|n| n.children.contains(&first_slot.node_id))
        .unwrap()
        .children
        .push(node);
    layout.tree.nodes.push(LayoutNode {
        id: node,
        kind: NodeKind::Widget(WidgetKind::SubjectImage),
        children: vec![],
    });
    layout
        .content_styles
        .push(ContentNodeStyle::new(node, ContentPayloadKind::Images));
    repo.execute_public_layout(
        fc_ports::PublicLayoutRequest {
            command_id: CommandId::new(311).unwrap(),
            expected_layout_revision: layout.revision,
            expected_display_revision: state.display.revision,
            layout,
            display: state.display,
        },
        &actor(),
        &TestClock,
    )
    .unwrap();
    let second_slot = ContentSlotKey {
        scope: LayoutScope::Public,
        node_id: node,
    };
    let issued = repo
        .commands
        .issue(
            &writer.handle(),
            CommandFamily::ProfileContent,
            "first-original",
            CommandOwner::authenticated(&actor()),
        )
        .unwrap();
    let request = BeginContentRequest {
        command_id: issued.command_id,
        slot: first_slot,
        expected_revision: Revision::default(),
        operation: ContentOperation::Publish(ContentPayloadKind::Images),
        payload_fingerprint: [31; 32],
        expires_at: LocalDateTime::new(TestClock.now().date, 12 * 3600).unwrap(),
    };
    let (first_ad, first_owner) = repo
        .with_command_window(issued)
        .begin_profile_content_owned(request, &actor(), &TestClock)
        .unwrap();
    let first_reservation = repo
        .with_command_window(issued)
        .reserve_profile_media(&first_ad, &[7], &actor())
        .unwrap();
    assert_eq!(first_ad.sequence, 1);
    writer.shutdown().unwrap();
    // Metadata-only original preparation; no OS file/audio owner is created.
    // Rebuild the exact previous unpublished schema with its held row intact.
    let row_before: (u64, u64, Vec<u8>, String);
    let entry_before: (bool, Vec<u8>, String, Vec<u8>);
    {
        let db = Connection::open(t.path("live.sqlite3")).unwrap();
        row_before = db
            .query_row(
                "SELECT sequence,serial,admission,receipt_key FROM command_window_profile_owners",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        entry_before = db
            .query_row(
                "SELECT held,payload_id,family,owner FROM command_window_entries WHERE serial=?1",
                [issued.ticket.serial()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert!(entry_before.0);
        db.execute_batch("BEGIN IMMEDIATE;ALTER TABLE command_window_profile_owners RENAME TO previous_owner_rows;CREATE TABLE command_window_profile_owners(sequence INTEGER PRIMARY KEY,serial INTEGER UNIQUE NOT NULL REFERENCES command_window_entries(serial),admission BLOB NOT NULL,receipt_key TEXT UNIQUE NOT NULL);INSERT INTO command_window_profile_owners SELECT sequence,serial,admission,receipt_key FROM previous_owner_rows;DROP TABLE previous_owner_rows;COMMIT;").unwrap();
    }
    // Unknown candidate structure fails closed; the original held row survives.
    std::fs::copy(t.path("live.sqlite3"), t.path("unknown.sqlite3")).unwrap();
    {
        let db = Connection::open(t.path("unknown.sqlite3")).unwrap();
        db.execute_batch(
            "ALTER TABLE command_window_profile_owners ADD COLUMN unexpected INTEGER;",
        )
        .unwrap();
    }
    assert!(matches!(
        StorageWriter::open(t.path("unknown.sqlite3"), 8, Publisher),
        Err(StorageError::Database)
    ));
    {
        let db = Connection::open(t.path("unknown.sqlite3")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT sequence,serial,admission,receipt_key FROM command_window_profile_owners",
                [],
                |r| Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, String>(3)?
                ))
            )
            .unwrap(),
            row_before
        );
        assert!(db
            .query_row(
                "SELECT held FROM command_window_entries WHERE serial=?1",
                [issued.ticket.serial()],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
    }
    let (writer, repo) = open(&t.path("live.sqlite3"));
    {
        let db = Connection::open(t.path("live.sqlite3")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT sequence,serial,admission,receipt_key FROM command_window_profile_owners",
                [],
                |r| Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, String>(3)?
                ))
            )
            .unwrap(),
            row_before
        );
        assert_eq!(
            db.query_row(
                "SELECT held,payload_id,family,owner FROM command_window_entries WHERE serial=?1",
                [issued.ticket.serial()],
                |r| Ok((
                    r.get::<_, bool>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Vec<u8>>(3)?
                ))
            )
            .unwrap(),
            entry_before
        );
    }
    let second = repo
        .commands
        .issue(
            &writer.handle(),
            CommandFamily::ProfileContent,
            "second-original",
            CommandOwner::authenticated(&actor()),
        )
        .unwrap();
    let (second_ad, second_owner) = repo
        .with_command_window(second)
        .begin_profile_content_owned(
            BeginContentRequest {
                command_id: second.command_id,
                slot: second_slot,
                expected_revision: Revision::default(),
                operation: ContentOperation::Publish(ContentPayloadKind::Images),
                payload_fingerprint: [32; 32],
                expires_at: LocalDateTime::new(TestClock.now().date, 12 * 3600).unwrap(),
            },
            &actor(),
            &TestClock,
        )
        .unwrap();
    assert_eq!(second_ad.sequence, 1, "sequence belongs to this slot");
    assert_ne!(first_ad.request.slot, second_ad.request.slot);
    assert_ne!(issued.ticket, second.ticket);
    let mut swapped = first_ad.clone();
    swapped.request.slot = second_slot;
    assert!(repo
        .with_command_window(issued)
        .reserve_profile_media(&swapped, &[7], &actor())
        .is_err());
    let first = repo.profile_content(first_slot, &actor()).unwrap();
    let second_head = repo.profile_content(second_slot, &actor()).unwrap();
    assert_eq!(first.pending.as_ref(), Some(&first_ad));
    assert_eq!(second_head.pending.as_ref(), Some(&second_ad));
    // Original closed negative capability still identifies only its original C2
    // receipt after migration; it cannot release the other slot's preparation.
    repo.fail_profile_content_owned(first_owner.clone())
        .unwrap();
    assert_eq!(
        repo.profile_content(second_slot, &actor())
            .unwrap()
            .pending
            .as_ref(),
        Some(&second_ad)
    );
    assert!(repo
        .profile_content(first_slot, &actor())
        .unwrap()
        .pending
        .is_none());
    {
        let db = Connection::open(t.path("live.sqlite3")).unwrap();
        assert!(
            db.query_row(
                "SELECT held FROM command_window_entries WHERE serial=?1",
                [issued.ticket.serial()],
                |r| r.get::<_, bool>(0)
            )
            .unwrap(),
            "original Reserved media remains charged after intent failure"
        );
        assert!(db
            .query_row(
                "SELECT held FROM command_window_entries WHERE serial=?1",
                [second.ticket.serial()],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
    }
    repo.settle_profile_media(first_reservation).unwrap();
    writer.shutdown().unwrap();
    let (writer, repo) = open(&t.path("live.sqlite3"));
    repo.fail_profile_content_owned(first_owner).unwrap();
    repo.fail_profile_content_owned(second_owner).unwrap();
    assert!(repo
        .profile_content(second_slot, &actor())
        .unwrap()
        .pending
        .is_none());
    {
        let db = Connection::open(t.path("live.sqlite3")).unwrap();
        for serial in [issued.ticket.serial(), second.ticket.serial()] {
            assert!(!db
                .query_row(
                    "SELECT held FROM command_window_entries WHERE serial=?1",
                    [serial],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap());
        }
    }
    // A fabricated admission does not become an original owner capability.
    assert!(matches!(
        repo.with_command_window(second)
            .reserve_profile_media(&swapped, &[7], &actor()),
        Err(ProfileContentRepositoryError::CommandWindow(_))
            | Err(ProfileContentRepositoryError::Domain(_))
    ));
    writer.shutdown().unwrap();
}

// G02 typed receipts use the real SQLite writer and the same synthetic local
// administrator as the existing recovery fixtures. No native effect is mocked
// as physical return; the manual provider case reports a real Failed attempt.
struct TypedTestClock(std::sync::atomic::AtomicU64);
impl fc_ports::MonotonicClock for TypedTestClock {
    fn monotonic_millis(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}
impl crate::command_window::WindowClock for TypedTestClock {
    fn wall_millis(&self) -> Result<u64, StorageError> {
        Ok(self.0.load(Ordering::Acquire))
    }
    fn monotonic_millis(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}
fn typed_test_open(
    path: &Path,
    clock: Arc<TypedTestClock>,
) -> (StorageWriter<u64>, BusinessRepository<u64>) {
    let writer = StorageWriter::open_with_clocks(path, 8, Publisher, clock.clone(), clock).unwrap();
    let repo = BusinessRepository::new(
        writer.handle(),
        Arc::new(TestClock),
        Arc::new(|_, revision| Ok(revision.get())),
    )
    .with_session_guard(guard());
    (writer, repo)
}
struct RawTypedFixture {
    issued: fc_ports::command_window::IssuedCommand,
    write: bool,
}
impl TransactionTask<u64> for RawTypedFixture {
    fn command_requirement(&self) -> Option<(fc_ports::command_window::CommandFamily, CommandId)> {
        Some((self.issued.family, self.issued.command_id))
    }
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<u64>, StorageError> {
        Ok(TransactionDecision {
            precommit: None,
            outcome: Vec::new(),
            commit: if self.write {
                Some((
                    vec![Mutation::Put {
                        key: "fixture/typed/raw".into(),
                        value: vec![1],
                    }],
                    d.revision()?.get() + 1,
                ))
            } else {
                None
            },
        })
    }
}
#[test]
fn typed_voice_acceptance_survives_reopen_and_raw_typed_consumption_is_bidirectional() {
    use crate::typed_command::TypedAdmission;
    use fc_ports::command_window::*;
    let t = Temp::new();
    seed(&t.path("typed.sqlite3"), 0);
    let clock = Arc::new(TypedTestClock(std::sync::atomic::AtomicU64::new(1000)));
    let (writer, repo) = typed_test_open(&t.path("typed.sqlite3"), clock.clone());
    let commands = crate::fixture_commands::FixtureCommands::default();
    let issue = |label| {
        commands
            .issue(
                &writer.handle(),
                CommandFamily::Voice,
                label,
                CommandOwner::authenticated(&actor()),
            )
            .unwrap()
    };
    let issued = issue("voice-policy");
    let scoped = repo.with_command_window(issued);
    let intent = VoicePolicyIntent::Enabled {
        enabled: false,
        expected_voice: Revision::default(),
        expected_identity: Revision::new(1),
        expected_runtime: 7,
    };
    // A query is not consumption of an unused issuance.
    writer
        .handle()
        .with_command_window(issued)
        .try_transaction(RawTypedFixture {
            issued,
            write: false,
        })
        .unwrap()
        .wait()
        .unwrap();
    let permit = match scoped
        .accept_voice_policy(intent.clone(), &actor(), guard())
        .unwrap()
    {
        TypedAdmission::Fresh(p) => p,
        _ => panic!("first durable acceptance must return the only permit"),
    };
    assert_eq!(
        writer.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(1)
    );
    assert!(matches!(
        scoped
            .accept_voice_policy(intent.clone(), &actor(), guard())
            .unwrap(),
        TypedAdmission::History(TypedCommandOutcome::Pending)
    ));
    assert_eq!(
        writer
            .handle()
            .with_command_window(issued)
            .try_transaction(RawTypedFixture {
                issued,
                write: true
            })
            .unwrap()
            .wait(),
        Err(StorageError::CommandIdReused)
    );
    let raw = issue("raw-first");
    writer
        .handle()
        .with_command_window(raw)
        .try_transaction(RawTypedFixture {
            issued: raw,
            write: true,
        })
        .unwrap()
        .wait()
        .unwrap();
    assert!(matches!(
        repo.with_command_window(raw)
            .accept_voice_policy(intent.clone(), &actor(), guard()),
        Err(StorageError::CommandIdReused)
    ));
    writer.shutdown().unwrap();
    let (writer, repo) = typed_test_open(&t.path("typed.sqlite3"), clock);
    let scoped = repo.with_command_window(issued);
    assert!(matches!(
        scoped
            .accept_voice_policy(intent.clone(), &actor(), guard())
            .unwrap(),
        TypedAdmission::History(TypedCommandOutcome::Pending)
    ));
    let completed = scoped.complete_voice_policy(permit, &actor()).unwrap();
    let TypedCommandOutcome::Completed(TypedCommandResult::VoicePolicy(snapshot)) = &completed
    else {
        panic!("original snapshot required")
    };
    assert_eq!(snapshot.revision, Revision::new(1));
    assert!(snapshot.jobs.iter().all(|j| j.authorization.is_none()));
    assert_eq!(
        scoped
            .query_typed_command(TypedCommandIntent::VoicePolicy(intent.clone()), &actor())
            .unwrap(),
        completed
    );
    let revision = writer.handle().try_revision().unwrap().wait().unwrap();
    assert!(
        matches!(scoped.accept_voice_policy(intent,&actor(),guard()).unwrap(),TypedAdmission::History(outcome) if outcome==completed)
    );
    assert_eq!(
        writer.handle().try_revision().unwrap().wait().unwrap(),
        revision
    );
    let changed = VoicePolicyIntent::Limits {
        limits: Default::default(),
        expected_voice: Revision::default(),
        expected_identity: Revision::new(1),
        expected_runtime: 7,
    };
    assert!(matches!(
        scoped.accept_voice_policy(changed, &actor(), guard()),
        Err(StorageError::CommandIdReused)
    ));
    let failed = commands
        .issue(
            &writer.handle(),
            CommandFamily::Voice,
            "accepted-conflict",
            CommandOwner::authenticated(&actor()),
        )
        .unwrap();
    let failed_intent = VoicePolicyIntent::Enabled {
        enabled: true,
        expected_voice: Revision::new(1),
        expected_identity: Revision::new(1),
        expected_runtime: 7,
    };
    let failed_permit = match repo
        .with_command_window(failed)
        .accept_voice_policy(failed_intent.clone(), &actor(), guard())
        .unwrap()
    {
        TypedAdmission::Fresh(p) => p,
        _ => panic!("first accepted conflict"),
    };
    let competing = commands
        .issue(
            &writer.handle(),
            CommandFamily::Voice,
            "competing-policy",
            CommandOwner::authenticated(&actor()),
        )
        .unwrap();
    repo.with_command_window(competing)
        .set_voice_enabled(false, Revision::new(1), &actor())
        .unwrap();
    assert_eq!(
        repo.with_command_window(failed)
            .complete_voice_policy(failed_permit, &actor()),
        Err(StorageError::Voice(
            fc_domain::voice::VoiceError::RevisionConflict
        ))
    );
    assert_eq!(
        repo.with_command_window(failed)
            .query_typed_command(TypedCommandIntent::VoicePolicy(failed_intent), &actor())
            .unwrap(),
        TypedCommandOutcome::Failed(TypedCommandFailure::Conflict)
    );
    writer.shutdown().unwrap();
}
#[test]
fn typed_calendar_policy_returns_original_result_after_current_state_changes() {
    use fc_ports::command_window::*;
    let t = Temp::new();
    seed(&t.path("typed.sqlite3"), 0);
    let clock = Arc::new(TypedTestClock(std::sync::atomic::AtomicU64::new(1000)));
    let (writer, repo) = typed_test_open(&t.path("typed.sqlite3"), clock);
    let commands = crate::fixture_commands::FixtureCommands::default();
    let first = commands
        .issue(
            &writer.handle(),
            CommandFamily::Calendar,
            "policy-first",
            CommandOwner::authenticated(&actor()),
        )
        .unwrap();
    let original = repo
        .with_command_window(first)
        .set_calendar_policy_typed(Revision::default(), true, &actor())
        .unwrap();
    let second = commands
        .issue(
            &writer.handle(),
            CommandFamily::Calendar,
            "policy-second",
            CommandOwner::authenticated(&actor()),
        )
        .unwrap();
    let current = repo
        .with_command_window(second)
        .set_calendar_policy_typed(Revision::new(1), false, &actor())
        .unwrap();
    assert_ne!(original, current);
    let revision = writer.handle().try_revision().unwrap().wait().unwrap();
    assert_eq!(
        repo.with_command_window(first)
            .set_calendar_policy_typed(Revision::default(), true, &actor())
            .unwrap(),
        original
    );
    assert_eq!(
        repo.with_command_window(first)
            .query_typed_command(
                TypedCommandIntent::CalendarPolicy {
                    enabled: true,
                    expected_policy: Revision::default()
                },
                &actor()
            )
            .unwrap(),
        original
    );
    assert!(matches!(
        repo.with_command_window(first).set_calendar_policy_typed(
            Revision::default(),
            false,
            &actor()
        ),
        Err(StorageError::CommandIdReused)
    ));
    assert!(matches!(
        repo.with_command_window(first)
            .begin_calendar_manual_typed(2026, &actor()),
        Err(StorageError::CommandIdReused)
    ));
    assert_eq!(
        writer.handle().try_revision().unwrap().wait().unwrap(),
        revision
    );
    writer.shutdown().unwrap();
}
#[test]
fn typed_manual_history_never_recreates_provider_and_original_owner_settles_after_expiry() {
    use crate::business::ManualFetchStart;
    use fc_ports::{calendar::*, command_window::*};
    let t = Temp::new();
    seed(&t.path("typed.sqlite3"), 0);
    let clock = Arc::new(TypedTestClock(std::sync::atomic::AtomicU64::new(1000)));
    let (writer, repo) = typed_test_open(&t.path("typed.sqlite3"), clock.clone());
    let commands = crate::fixture_commands::FixtureCommands::default();
    let issued = commands
        .issue(
            &writer.handle(),
            CommandFamily::Calendar,
            "manual",
            CommandOwner::authenticated(&actor()),
        )
        .unwrap();
    let permit = match repo
        .with_command_window(issued)
        .begin_calendar_manual_typed(2026, &actor())
        .unwrap()
    {
        ManualFetchStart::Fresh(p) => p,
        _ => panic!("fresh provider owner expected"),
    };
    assert_eq!(permit.year(), 2026);
    let revision = writer.handle().try_revision().unwrap().wait().unwrap();
    assert!(matches!(
        repo.with_command_window(issued)
            .begin_calendar_manual_typed(2026, &actor())
            .unwrap(),
        ManualFetchStart::History(TypedCommandOutcome::Pending)
    ));
    assert_eq!(
        writer.handle().try_revision().unwrap().wait().unwrap(),
        revision
    );
    clock.0.fetch_add(COMMAND_WINDOW_MILLIS, Ordering::AcqRel);
    assert_eq!(
        repo.with_command_window(issued).query_typed_command(
            TypedCommandIntent::CalendarManualFetch { year: 2026 },
            &actor()
        ),
        Err(StorageError::CommandExpired)
    );
    let settled = repo
        .clone()
        .with_execution_fence(Arc::new(|| false))
        .with_calendar_execution_guard(Arc::new(|| false))
        .finish_calendar_manual_typed(
            permit,
            CalendarFetchOutcome::Failed(CalendarFetchFailure::Network),
        )
        .unwrap();
    assert!(matches!(
        settled,
        TypedCommandOutcome::Completed(TypedCommandResult::CalendarManualFetch(
            CalendarManualResult {
                kind: CalendarManualResultKind::Failed(CalendarFetchFailure::Network),
                ..
            }
        ))
    ));
    writer.shutdown().unwrap();
    let db = Connection::open(t.path("typed.sqlite3")).unwrap();
    let (held, bytes): (bool, u64) = db
        .query_row(
            "SELECT held,charged_bytes FROM command_window_entries WHERE serial=?1",
            [issued.ticket.serial()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(!held);
    assert!(
        bytes < MAX_VALUE_BYTES as u64,
        "completion must refund the unused reserved result allowance"
    );
}

#[test]
fn typed_manual_late_positive_uses_only_the_original_negative_settlement() {
    use fc_ports::{calendar::*, command_window::*};
    let t = Temp::new();
    seed(&t.path("typed.sqlite3"), 0);
    let clock = Arc::new(TypedTestClock(std::sync::atomic::AtomicU64::new(1000)));
    let (writer, repo) = typed_test_open(&t.path("typed.sqlite3"), clock);
    let commands = crate::fixture_commands::FixtureCommands::default();
    let issued = commands
        .issue(
            &writer.handle(),
            CommandFamily::Calendar,
            "late-positive",
            CommandOwner::authenticated(&actor()),
        )
        .unwrap();
    let permit = match repo
        .with_command_window(issued)
        .begin_calendar_manual_typed(2026, &actor())
        .unwrap()
    {
        business::ManualFetchStart::Fresh(p) => p,
        _ => panic!("first provider required"),
    };
    let candidate = fc_domain::calendar::CalendarCandidate {
        source_year: 2026,
        fetched_at: TestClock.now(),
        source_sha256: [9; 32],
        papers: vec!["https://www.gov.cn/zhengce/content/2025-11/01/content_1234567.htm".into()],
        days: vec![fc_domain::calendar::PublicCalendarDay {
            date: TestClock.now().date,
            kind: fc_domain::calendar::PublicDayKind::Rest,
            name: "synthetic test".into(),
        }],
        etag: None,
        last_modified: None,
    };
    candidate.validate().unwrap();
    let outcome = repo
        .clone()
        .with_execution_fence(Arc::new(|| false))
        .finish_calendar_manual_typed(permit, CalendarFetchOutcome::Downloaded(candidate))
        .unwrap();
    assert!(matches!(
        outcome,
        TypedCommandOutcome::Completed(TypedCommandResult::CalendarManualFetch(
            CalendarManualResult {
                kind: CalendarManualResultKind::Failed(CalendarFetchFailure::Cancelled),
                ..
            }
        ))
    ));
    let status = repo.calendar_status(2026, &actor()).unwrap();
    assert!(status.candidate.is_none());
    assert!(!status.years[0].pending);
    assert_eq!(
        status.years[0].last_error,
        Some(CalendarFetchFailure::Cancelled)
    );
    assert_eq!(
        repo.with_command_window(issued)
            .query_typed_command(
                TypedCommandIntent::CalendarManualFetch { year: 2026 },
                &actor()
            )
            .unwrap(),
        outcome
    );
    writer.shutdown().unwrap();
}

#[test]
fn missing_v2_consumption_retires_old_clients_but_preserves_original_private_owner() {
    use fc_ports::command_window::{CommandFamily, CommandWindowError};
    let t = Temp::new();
    seed(&t.path("legacy-v2.sqlite3"), 0);
    let (writer, mut repo) = open(&t.path("legacy-v2.sqlite3"));
    let slot = image_slot(&mut repo);
    let issued = repo.issue(
        CommandFamily::ProfileContent,
        "old-v2-owner",
        Some(&actor()),
    );
    let (ad, owner) = repo
        .with_command_window(issued)
        .begin_profile_content_owned(
            BeginContentRequest {
                command_id: issued.command_id,
                slot,
                expected_revision: Revision::default(),
                operation: ContentOperation::Publish(ContentPayloadKind::Images),
                payload_fingerprint: [37; 32],
                expires_at: LocalDateTime::new(TestClock.now().date, 12 * 3600).unwrap(),
            },
            &actor(),
            &TestClock,
        )
        .unwrap();
    writer.shutdown().unwrap();
    let db = Connection::open(t.path("legacy-v2.sqlite3")).unwrap();
    let owner_before:(u64,u64,Vec<u8>,String)=db.query_row("SELECT sequence,serial,admission,receipt_key FROM command_window_profile_owners WHERE serial=?1",[issued.ticket.serial()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    let high_before: u64 = db
        .query_row("SELECT issued FROM command_window_meta", [], |r| r.get(0))
        .unwrap();
    // Synthetic exact earlier-v2 schema; the real AD/held owner and all business
    // bytes stay intact. No OS file/preparation owner was created by this test.
    db.execute_batch("DROP TABLE command_window_consumption")
        .unwrap();
    drop(db);
    let (writer, repo) = open(&t.path("legacy-v2.sqlite3"));
    assert_eq!(
        writer.handle().try_check_command(issued).unwrap().wait(),
        Err(StorageError::CommandExpired)
    );
    assert_eq!(
        repo.with_command_window(issued).profile_content_outcome(
            issued.command_id,
            &actor(),
            &TestClock
        ),
        Err(fc_ports::ProfileContentRepositoryError::CommandWindow(
            CommandWindowError::Expired
        ))
    );
    assert_eq!(
        repo.profile_content(slot, &actor())
            .unwrap()
            .pending
            .as_ref(),
        Some(&ad)
    );
    let db = Connection::open(t.path("legacy-v2.sqlite3")).unwrap();
    let owner_after:(u64,u64,Vec<u8>,String)=db.query_row("SELECT sequence,serial,admission,receipt_key FROM command_window_profile_owners WHERE serial=?1",[issued.ticket.serial()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(owner_after, owner_before);
    assert!(db
        .query_row(
            "SELECT held FROM command_window_entries WHERE serial=?1",
            [issued.ticket.serial()],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
    drop(db);
    repo.fail_profile_content_owned(owner).unwrap();
    assert!(repo
        .profile_content(slot, &actor())
        .unwrap()
        .pending
        .is_none());
    let next = repo.issue(
        CommandFamily::ProfileContent,
        "new-v2-owner",
        Some(&actor()),
    );
    assert!(next.ticket.serial() > high_before);
    assert_ne!(next.ticket, issued.ticket);
    assert_eq!(
        writer.handle().try_check_command(next).unwrap().wait(),
        Ok(())
    );
    assert_eq!(
        writer.handle().try_check_command(issued).unwrap().wait(),
        Err(StorageError::CommandExpired)
    );
    writer.shutdown().unwrap();
}
