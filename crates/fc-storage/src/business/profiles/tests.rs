//! Real sole-writer SQLite tests. Credential bytes and channel guards are explicit
//! synthetic fixtures; these tests do not claim KDF/DPAPI/password authentication.
use super::*;
use fc_domain::{layout::*, time::*};
use fc_ports::{
    DisplayRepository, IdentityRepository, ProfileRepository, ProfileRepositoryError as E,
    ProfileRequest, PublicLayoutRequest,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "FutureClassroom-Profiles-R16-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(p)
    }
    fn db(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("profiles private test directory cleanup")
    }
}
fn day() -> LocalDate {
    LocalDate::new(2026, 10, 31).unwrap()
}
fn at(hour: u32, minute: u32) -> LocalDateTime {
    LocalDateTime::new(day(), hour * 3600 + minute * 60).unwrap()
}
struct Time(Mutex<LocalDateTime>);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        *self.0.lock().unwrap()
    }
}
fn principal(n: u64) -> PrincipalId {
    PrincipalId::new(n).unwrap()
}
fn key(class: u64, owner: u64) -> SubjectProfileKey {
    SubjectProfileKey {
        class_id: ClassId::new(class).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
        teacher_id: principal(owner),
    }
}
fn actor(n: u64, origin: TrustedOrigin) -> ActorContext {
    ActorContext {
        principal_id: principal(n),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(LocalDate::new(2026, 11, 1).unwrap(), 0).unwrap(),
        origin,
    }
}
fn state() -> IdentityState {
    let school = SchoolId::new(1).unwrap();
    IdentityState {
        revision: Revision::new(1),
        installation: Some(Installation {
            school_id: school,
            school_name: "profile school".into(),
            current_class: Some(ClassId::new(1).unwrap()),
        }),
        accounts: (1..=4)
            .map(|n| UserAccount {
                principal: Principal {
                    id: principal(n),
                    school_id: school,
                    role: match n {
                        1 => Role::GlobalAdmin,
                        4 => Role::User,
                        _ => Role::TeacherAdmin,
                    },
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("person{n}"),
                display_name: format!("Person {n}"),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            })
            .collect(),
        classes: (1..=70)
            .map(|n| SchoolClass {
                id: ClassId::new(n).unwrap(),
                school_id: school,
                name: format!("Class {n}"),
                enabled: true,
            })
            .collect(),
        subjects: vec![SchoolSubject {
            id: SubjectId::new(1).unwrap(),
            school_id: school,
            name: "Math".into(),
            enabled: true,
            color: Rgb {
                red: 30,
                green: 50,
                blue: 70,
            },
            revision: Revision::new(1),
        }],
        assignments: (1..=70)
            .flat_map(|c| {
                [2, 3].map(move |t| TeachingAssignment {
                    teacher_id: principal(t),
                    class_id: ClassId::new(c).unwrap(),
                    subject_id: SubjectId::new(1).unwrap(),
                    enabled: true,
                })
            })
            .collect(),
        default_teachers: vec![],
    }
}
struct Publisher {
    active: Arc<AtomicBool>,
    lock_begin: Arc<AtomicBool>,
    publish_fail: Arc<AtomicBool>,
    published: Arc<AtomicU64>,
    rolled_back: Arc<AtomicU64>,
}
impl fc_ports::AuthorityPublisher for Publisher {
    type Snapshot = Revision;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, s: &Revision) -> Revision {
        *s
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        if self.lock_begin.swap(false, Ordering::AcqRel) {
            self.active.store(false, Ordering::Release)
        }
        Ok(())
    }
    fn publish(&mut self, _: (), _: Revision) -> Result<(), ()> {
        if self.publish_fail.swap(false, Ordering::AcqRel) {
            return Err(());
        }
        self.published.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        self.rolled_back.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn fault(&mut self) {}
}
struct Fixture {
    writer: Option<StorageWriter<Revision>>,
    clock: Arc<Time>,
    active: Arc<AtomicBool>,
    lock_begin: Arc<AtomicBool>,
    publish_fail: Arc<AtomicBool>,
    published: Arc<AtomicU64>,
    rolled_back: Arc<AtomicU64>,
    directory: Directory,
}
impl Fixture {
    fn new() -> Self {
        let mut f = Self {
            writer: None,
            clock: Arc::new(Time(Mutex::new(at(8, 30)))),
            active: Arc::new(AtomicBool::new(true)),
            lock_begin: Arc::new(AtomicBool::new(false)),
            publish_fail: Arc::new(AtomicBool::new(false)),
            published: Arc::new(AtomicU64::new(0)),
            rolled_back: Arc::new(AtomicU64::new(0)),
            directory: Directory::new(),
        };
        f.open();
        let s = state();
        s.validate().unwrap();
        let mut mutations = vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&s)).unwrap(),
        }];
        for n in 1..=4 {
            for kind in ["password", "recovery"] {
                mutations.push(Mutation::Put {
                    key: credential_key(principal(n), kind, 1),
                    value: vec![n as u8],
                })
            }
        }
        f.commit(mutations);
        f
    }
    fn open(&mut self) {
        self.writer = Some(
            StorageWriter::open(
                self.directory.db(),
                8,
                Publisher {
                    active: self.active.clone(),
                    lock_begin: self.lock_begin.clone(),
                    publish_fail: self.publish_fail.clone(),
                    published: self.published.clone(),
                    rolled_back: self.rolled_back.clone(),
                },
            )
            .unwrap(),
        )
    }
    fn stop(&mut self) {
        if let Some(w) = self.writer.take() {
            w.shutdown().unwrap()
        }
    }
    fn reopen(&mut self) {
        self.stop();
        startup_state(self.directory.db()).unwrap();
        self.open()
    }
    fn handle(&self) -> StorageHandle<Revision> {
        self.writer.as_ref().unwrap().handle()
    }
    fn revision(&self) -> Revision {
        self.handle().try_revision().unwrap().wait().unwrap()
    }
    fn commit(&self, mutations: Vec<Mutation>) {
        let rev = self.revision();
        self.handle()
            .try_commit(
                CommitCommand::new(format!("seed-{}", rev.get()), rev, mutations).unwrap(),
                rev.next().unwrap(),
            )
            .unwrap()
            .wait()
            .unwrap();
    }
    fn repo(&self, captured: ActorContext) -> BusinessRepository<Revision> {
        let active = self.active.clone();
        let clock = self.clock.clone();
        BusinessRepository::new(
            self.handle(),
            self.clock.clone(),
            Arc::new(|_, rev| Ok(rev)),
        )
        .with_session_guard(Arc::new(move |s, current| {
            if current != &captured || !active.load(Ordering::Acquire) {
                return Err(StorageError::Identity(IdentityError::Permission(
                    PermissionError::StaleAuthorization,
                )));
            }
            s.authority(current.principal_id)
                .map_err(StorageError::Identity)?
                .validate_actor(current, clock.now())
                .map_err(|e| StorageError::Identity(IdentityError::Permission(e)))?;
            Ok(())
        }))
    }
    fn admin(&self) -> BusinessRepository<Revision> {
        self.repo(actor(1, TrustedOrigin::Local))
    }
    fn request(&self, id: u64, k: SubjectProfileKey) -> ProfileRequest {
        ProfileRequest {
            command_id: CommandId::new(id).unwrap(),
            key: k,
            expected_revision: Revision::default(),
            candidate: SubjectProfileDocument::new(ProfileId::new(999).unwrap(), k),
        }
    }
    fn save(&self, request: ProfileRequest) -> Result<SubjectProfileDocument, E> {
        self.admin().execute_subject_profile(
            request,
            &actor(1, TrustedOrigin::Local),
            self.clock.as_ref(),
        )
    }
    fn sql(&self, sql: &str) {
        Connection::open(self.directory.db())
            .unwrap()
            .execute_batch(sql)
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop()
    }
}
fn public_request(id: u64, s: fc_ports::PublicLayoutState) -> PublicLayoutRequest {
    PublicLayoutRequest {
        command_id: CommandId::new(id).unwrap(),
        expected_layout_revision: s.layout.revision,
        expected_display_revision: s.display.revision,
        layout: s.layout,
        display: s.display,
    }
}
fn gap(doc: &mut LayoutDocument, n: u16) {
    let root = doc
        .tree
        .nodes
        .iter_mut()
        .find(|n| n.id == doc.tree.root)
        .unwrap();
    if let NodeKind::Root { gap, .. } = &mut root.kind {
        *gap = n
    } else {
        panic!("root")
    }
}
#[test]
fn profiles_independent_cas_joint_display_receipts_and_same_id_payload_binding() {
    let f = Fixture::new();
    let a = actor(1, TrustedOrigin::Local);
    let mut r = f.admin();
    let initial = r.public_layout(&a).unwrap();
    let first = public_request(10, initial);
    let saved = r
        .execute_public_layout(first.clone(), &a, f.clock.as_ref())
        .unwrap();
    assert_eq!(saved.layout.revision, Revision::new(1));
    assert_eq!(saved.display.revision, Revision::default());
    let rev = f.revision();
    assert_eq!(
        r.execute_public_layout(first.clone(), &a, f.clock.as_ref())
            .unwrap(),
        saved
    );
    assert_eq!(f.revision(), rev);
    let mut conflict = first;
    conflict.display.clock_seconds = true;
    assert_eq!(
        r.execute_public_layout(conflict, &a, f.clock.as_ref()),
        Err(E::IdempotencyConflict)
    );
    let mut d = saved.display.clone();
    d.clock_seconds = true;
    let updated = r
        .execute_display(
            fc_ports::DisplayRequest {
                command_id: CommandId::new(11).unwrap(),
                expected_revision: d.revision,
                candidate: d,
            },
            &a,
            f.clock.as_ref(),
        )
        .unwrap();
    let before = f.revision();
    assert!(
        matches!(r.execute_public_layout(public_request(12,saved.clone()),&a,f.clock.as_ref()),Err(E::Display(fc_domain::desktop::DisplayError::RevisionConflict{current}))if current==updated.revision)
    );
    assert_eq!(f.revision(), before);
    assert_eq!(r.public_layout(&a).unwrap().layout, saved.layout);
    let mut joint = r.public_layout(&a).unwrap();
    gap(&mut joint.layout, 14);
    joint.display.clock_seconds = false;
    let next = r
        .execute_public_layout(public_request(13, joint), &a, f.clock.as_ref())
        .unwrap();
    assert_eq!(next.layout.revision, Revision::new(2));
    assert_eq!(next.display.revision, Revision::new(2));
    let noop = r
        .execute_public_layout(public_request(14, next.clone()), &a, f.clock.as_ref())
        .unwrap();
    assert_eq!(noop, next);
    let req = f.request(20, key(1, 2));
    let one = f.save(req.clone()).unwrap();
    let two = f.save(f.request(21, key(1, 3))).unwrap();
    assert_eq!(one.id.get(), 1);
    assert_eq!(two.id.get(), 2);
    let mut wrong_id = one.clone();
    wrong_id.id = two.id;
    let before = f.revision();
    assert_eq!(
        f.save(ProfileRequest {
            command_id: CommandId::new(24).unwrap(),
            key: one.key,
            expected_revision: one.revision,
            candidate: wrong_id
        }),
        Err(E::Domain(ProfileError::IdMismatch))
    );
    assert_eq!(f.revision(), before);

    let mut change = ProfileRequest {
        command_id: CommandId::new(22).unwrap(),
        key: one.key,
        expected_revision: one.revision,
        candidate: one.clone(),
    };
    change.candidate.materials.visible_rows = 2;
    let newer = f.save(change.clone()).unwrap();
    assert_eq!(newer.revision, Revision::new(2));
    assert_eq!(newer.layout_revision, Revision::new(1));
    let before = f.revision();
    assert!(
        matches!(f.save(ProfileRequest{command_id:CommandId::new(23).unwrap(),..change}),Err(E::Domain(ProfileError::RevisionConflict{current}))if current==newer.revision)
    );
    assert_eq!(f.revision(), before);
    assert_eq!(f.save(req).unwrap(), one);
    assert_eq!(f.revision(), before);
    assert_eq!(r.subject_profile(two.key, &a).unwrap(), Some(two));
}
#[test]
fn profiles_teacher_web_user_denied_and_local_lock_at_precommit_rolls_back() {
    let f = Fixture::new();
    for a in [
        actor(2, TrustedOrigin::Local),
        actor(1, TrustedOrigin::Web),
        actor(4, TrustedOrigin::Local),
    ] {
        let mut r = f.repo(a.clone());
        assert!(r.public_layout(&a).is_err());
        assert!(r.subject_profile(key(1, 2), &a).is_err());
        assert!(r.list_subject_profiles(None, &a).is_err());
        assert!(r
            .execute_subject_profile(f.request(1, key(1, 2)), &a, f.clock.as_ref())
            .is_err())
    }
    let before = f.revision();
    f.lock_begin.store(true, Ordering::Release);
    assert!(f.save(f.request(30, key(1, 2))).is_err());
    assert_eq!(f.revision(), before);
    assert_eq!(f.rolled_back.load(Ordering::Acquire), 1);
    f.active.store(true, Ordering::Release);
    assert!(f
        .admin()
        .subject_profile(key(1, 2), &actor(1, TrustedOrigin::Local))
        .unwrap()
        .is_none());
    f.save(f.request(30, key(1, 2))).unwrap();
    assert_eq!(
        f.admin()
            .list_subject_profiles(None, &actor(1, TrustedOrigin::Local))
            .unwrap()
            .total,
        1
    );
}
#[test]
fn profiles_paged_catalog_history_disable_and_restart_do_not_block_revocation() {
    let mut f = Fixture::new();
    for c in 1..=65 {
        f.save(f.request(100 + c, key(c, 2))).unwrap();
    }
    let a = actor(1, TrustedOrigin::Local);
    let r = f.admin();
    let first = r.list_subject_profiles(None, &a).unwrap();
    assert_eq!(first.items.len(), 20);
    assert_eq!(first.next_after.unwrap().get(), 20);
    let last = r
        .list_subject_profiles(Some(ProfileId::new(60).unwrap()), &a)
        .unwrap();
    assert_eq!(last.items.len(), 5);
    assert!(last.next_after.is_none());
    assert_eq!(last.items.last().unwrap().id.get(), 65);
    let s = r.state().unwrap();
    let scopes = s
        .assignments
        .iter()
        .filter(|v| v.teacher_id == principal(2))
        .map(|v| AssignmentScope {
            class_id: v.class_id,
            subject_id: v.subject_id,
        })
        .collect();
    f.admin()
        .execute_identity(
            IdentityRequest {
                command_id: CommandId::new(500).unwrap(),
                expected_revision: s.revision,
                command: IdentityCommand::UpdateAccount {
                    id: principal(2),
                    display_name: "Former teacher".into(),
                    role: Role::TeacherAdmin,
                    enabled: false,
                    scopes,
                },
            },
            Some(&a),
            f.clock.as_ref(),
        )
        .unwrap();
    let old = f.admin().subject_profile(key(1, 2), &a).unwrap().unwrap();
    assert!(old.enabled);
    let mut off = old.clone();
    off.enabled = false;
    let disabled = f
        .save(ProfileRequest {
            command_id: CommandId::new(501).unwrap(),
            key: old.key,
            expected_revision: old.revision,
            candidate: off,
        })
        .unwrap();
    assert_eq!(disabled.id, old.id);
    assert_eq!(disabled.revision, Revision::new(2));
    let mut on = disabled.clone();
    on.enabled = true;
    let before = f.revision();
    assert_eq!(
        f.save(ProfileRequest {
            command_id: CommandId::new(502).unwrap(),
            key: old.key,
            expected_revision: disabled.revision,
            candidate: on
        }),
        Err(E::Domain(ProfileError::InvalidScope))
    );
    assert_eq!(f.revision(), before);
    f.reopen();
    let page = f.admin().list_subject_profiles(None, &a).unwrap();
    assert_eq!(page.total, 65);
    assert!(!page.items[0].enabled);
    assert_eq!(
        f.admin().subject_profile(old.key, &a).unwrap(),
        Some(disabled)
    );
}
#[test]
fn profiles_declared_missing_page_document_and_cross_owner_are_startup_corruption() {
    for mode in [
        "page",
        "document",
        "scope",
        "duplicate",
        "unknown",
        "version",
    ] {
        let mut f = Fixture::new();
        let p = f.save(f.request(600, key(1, 2))).unwrap();
        f.stop();
        match mode{"page"=>f.sql("DELETE FROM documents WHERE key='business/profiles/v1/pages/00000000000000000000'"),"document"=>f.sql("DELETE FROM documents WHERE key='business/profiles/v1/documents/00000000000000000001'"),_=>{let mut v=dto::subject(&p);match mode{"scope"=>v["key"]["teacher"]=json!(3),"unknown"=>v["unexpected"]=json!(true),"version"=>v["format"]=json!(99),_=>{}}let bytes=if mode=="duplicate"{let text=String::from_utf8(codec::bytes(&v).unwrap()).unwrap();text.replacen('{',"{\"format\":1,",1).into_bytes()}else{codec::bytes(&v).unwrap()};Connection::open(f.directory.db()).unwrap().execute("UPDATE documents SET value=?1 WHERE key='business/profiles/v1/documents/00000000000000000001'",[bytes]).unwrap();}}
        assert_eq!(
            startup_state(f.directory.db()),
            Err(StorageError::Database),
            "{mode}"
        );
    }
}

fn wrap_materials(p: &mut SubjectProfileDocument, node: u64) {
    let root = p.tree.root;
    let old = p.tree.node(root).unwrap().children.clone();
    let id = LayoutNodeId::new(node).unwrap();
    p.tree
        .nodes
        .iter_mut()
        .find(|n| n.id == root)
        .unwrap()
        .children = vec![id];
    p.tree.nodes.push(LayoutNode {
        id,
        kind: NodeKind::Container(ContainerSettings {
            flow: Flow::Column,
            padding: 4,
            gap: 4,
            width: AxisSize::Content {
                min: 120,
                preferred: 300,
                max: 800,
            },
            height: AxisSize::Content {
                min: 32,
                preferred: 100,
                max: 900,
            },
            group_opacity_percent: 100,
            material: None,
        }),
        children: old,
    });
}
#[test]
fn profiles_canonical_node_water_survives_delete_restart_and_rejects_retired_ids() {
    let mut f = Fixture::new();
    let mut request = f.request(700, key(1, 2));
    wrap_materials(&mut request.candidate, 100);
    request.candidate.node_id_high_water = u64::MAX;
    let first = f.save(request).unwrap();
    assert_eq!(first.node_id_high_water, 100);
    let mut bare = first.clone();
    bare.tree.nodes.retain(|n| n.id.get() != 100);
    bare.tree
        .nodes
        .iter_mut()
        .find(|n| n.id == bare.tree.root)
        .unwrap()
        .children = vec![LayoutNodeId::new(2).unwrap()];
    bare.node_id_high_water = 0;
    let second = f
        .save(ProfileRequest {
            command_id: CommandId::new(701).unwrap(),
            key: bare.key,
            expected_revision: first.revision,
            candidate: bare,
        })
        .unwrap();
    assert_eq!(second.node_id_high_water, 100);
    f.reopen();
    let a = actor(1, TrustedOrigin::Local);
    let loaded = f.admin().subject_profile(second.key, &a).unwrap().unwrap();
    assert_eq!(loaded, second);
    let mut reuse = loaded.clone();
    wrap_materials(&mut reuse, 50);
    let before = f.revision();
    assert_eq!(
        f.save(ProfileRequest {
            command_id: CommandId::new(702).unwrap(),
            key: reuse.key,
            expected_revision: loaded.revision,
            candidate: reuse
        }),
        Err(E::Domain(ProfileError::Layout(LayoutError::RetiredNodeId)))
    );
    assert_eq!(f.revision(), before);
    let mut next = loaded.clone();
    let id = next.allocate_id().unwrap();
    assert_eq!(id.get(), 101);
    wrap_materials(&mut next, id.get());
    assert_eq!(
        f.save(ProfileRequest {
            command_id: CommandId::new(703).unwrap(),
            key: next.key,
            expected_revision: loaded.revision,
            candidate: next
        })
        .unwrap()
        .node_id_high_water,
        101
    );
}
struct MetadataPreparation(u64);
impl fc_ports::MaterialPreparation for MetadataPreparation {
    type Input = ();
    fn prepare(
        &mut self,
        _: (),
    ) -> Result<fc_domain::materials::MaterialResource, fc_ports::MaterialRepositoryError> {
        Ok(fc_domain::materials::MaterialResource {
            version_id: ResourceVersionId::new(self.0).unwrap(),
            sha256: [self.0 as u8; 32],
            kind: fc_domain::materials::MaterialKind::Txt,
            bytes: 10,
        })
    }
}
fn add_material(f: &Fixture, owner: u64, n: u64) -> fc_domain::materials::MaterialRecord {
    use fc_ports::MaterialRepository;
    let prepared = fc_ports::prepare_material_append(
        &mut MetadataPreparation(n),
        (),
        fc_ports::MaterialAppendMetadata {
            command_id: CommandId::new(10_000 + n).unwrap(),
            scope: MaterialScope {
                teacher_id: principal(owner),
                subject_id: SubjectId::new(1).unwrap(),
            },
            material_id: MaterialId::new(n).unwrap(),
            display_name: format!("material-{n}.txt"),
            allowed_class_ids: vec![ClassId::new(1).unwrap()],
        },
    )
    .unwrap();
    f.admin()
        .append_material(prepared, &actor(1, TrustedOrigin::Local), f.clock.as_ref())
        .unwrap()
}
#[test]
fn profiles_exact_material_folder_owner_and_corrupt_receipt_are_not_silent_defaults() {
    let mut f = Fixture::new();
    let own = add_material(&f, 2, 800);
    let foreign = add_material(&f, 3, 801);
    let mut req = f.request(800, key(1, 2));
    req.candidate.folder_id = Some(foreign.folder_id);
    let before = f.revision();
    assert_eq!(
        f.save(req.clone()),
        Err(E::Domain(ProfileError::FolderMismatch))
    );
    assert_eq!(f.revision(), before);
    req.candidate.folder_id = Some(own.folder_id);
    let saved = f.save(req.clone()).unwrap();
    assert_eq!(saved.folder_id, Some(own.folder_id));
    f.stop();
    let r = receipt_keys(req.command_id).0;
    let db = Connection::open(f.directory.db()).unwrap();
    let bytes: Vec<u8> = db
        .query_row("SELECT value FROM documents WHERE key=?1", [&r], |row| {
            row.get(0)
        })
        .unwrap();
    let mut v = codec::value(&bytes).unwrap();
    v["result_digest"] = json!([1, 2]);
    db.execute(
        "UPDATE documents SET value=?1 WHERE key=?2",
        params![codec::bytes(&v).unwrap(), r],
    )
    .unwrap();
    drop(db);
    f.open();
    assert_eq!(f.save(req), Err(E::Unavailable));
    assert_eq!(
        f.admin()
            .subject_profile(saved.key, &actor(1, TrustedOrigin::Local))
            .unwrap(),
        Some(saved)
    );
}
fn schedule() -> fc_domain::schedule_document::ScheduleDocument {
    use fc_domain::{schedule::*, schedule_document::*};
    let course = TimetableRow {
        id: RowId::new(30).unwrap(),
        logical_key: LogicalRowKey::new(40).unwrap(),
        group: DayGroup::Morning,
        kind: RowKind::Course,
        range: MinuteRange::new(
            Minute::from_hm(8, 20).unwrap(),
            Minute::from_hm(8, 40).unwrap(),
        )
        .unwrap(),
        enabled_days: [true; 7],
        courses: std::array::from_fn(|_| {
            Some(CourseCell {
                subject_id: SubjectId::new(1).unwrap(),
                teacher: TeacherBinding::Explicit(principal(2)),
            })
        }),
    };
    let recess = TimetableRow {
        id: RowId::new(31).unwrap(),
        logical_key: LogicalRowKey::new(41).unwrap(),
        group: DayGroup::Morning,
        kind: RowKind::Rest(RestPurpose::Recess),
        range: MinuteRange::new(
            Minute::from_hm(8, 40).unwrap(),
            Minute::from_hm(8, 50).unwrap(),
        )
        .unwrap(),
        enabled_days: [true; 7],
        courses: std::array::from_fn(|_| None),
    };
    let mut second = course.clone();
    second.id = RowId::new(32).unwrap();
    second.logical_key = LogicalRowKey::new(42).unwrap();
    second.range = MinuteRange::new(
        Minute::from_hm(8, 50).unwrap(),
        Minute::from_hm(9, 10).unwrap(),
    )
    .unwrap();
    let term = Semester {
        id: SemesterId::new(10).unwrap(),
        class_id: ClassId::new(1).unwrap(),
        revision: Revision::default(),
        enabled: true,
        start: LocalDate::new(2026, 10, 1).unwrap(),
        end: LocalDate::new(2026, 11, 30).unwrap(),
        first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
        tables: vec![WeekTable {
            id: TableId::new(20).unwrap(),
            role: TableRole::Single,
            rows: vec![course, recess, second],
        }],
    };
    ScheduleDocument {
        semesters: vec![SemesterPlan {
            semester: term,
            name: "Autumn".into(),
            timing_sync: false,
            dual_history: None,
            row_details: vec![],
            cell_details: vec![],
        }],
        ..Default::default()
    }
}
#[test]
fn profiles_facts_current_exact_teacher_override_break_and_real_material_pagination() {
    use fc_domain::{
        lesson_context::LessonAction,
        schedule::{CourseCell, TeacherBinding},
    };
    use fc_ports::{LessonRepository, ScheduleRepository};
    let f = Fixture::new();
    let a = actor(1, TrustedOrigin::Local);
    let mut r = f.admin();
    r.execute_schedule(
        fc_ports::ScheduleRequest {
            command_id: CommandId::new(900).unwrap(),
            expected_revision: Revision::default(),
            candidate: schedule(),
        },
        &a,
        f.clock.as_ref(),
    )
    .unwrap();
    let missing = r.desktop_facts(0, f.clock.as_ref()).unwrap();
    assert_eq!(missing.profiles.reason, ProfileSelectionReason::Missing);
    let one = add_material(&f, 2, 901);
    add_material(&f, 2, 902);
    add_material(&f, 2, 903);
    let mut req = f.request(901, key(1, 2));
    req.candidate.folder_id = Some(one.folder_id);
    req.candidate.materials.visible_rows = 2;
    let p = f.save(req).unwrap();
    let q = f.save(f.request(902, key(1, 3))).unwrap();
    let facts = r.desktop_facts(0, f.clock.as_ref()).unwrap();
    assert_eq!(facts.profiles.subject_profile.as_ref(), Some(&p));
    assert_eq!(facts.materials_page.as_ref().unwrap().items.len(), 2);
    let cursor = facts.materials_page.unwrap().next_cursor.unwrap();
    assert_eq!(cursor.offset, 2);
    let last = r
        .desktop_facts_with_material_cursor(0, Some(cursor), f.clock.as_ref())
        .unwrap();
    assert_eq!(last.materials_page.unwrap().items.len(), 1);
    let context = r.lesson_context(&a, f.clock.as_ref()).unwrap();
    r.execute_lesson(
        fc_ports::LessonRequest {
            command_id: CommandId::new(903).unwrap(),
            expected_revision: context.revision,
            lesson: context.current.unwrap().key,
            action: LessonAction::Replace(CourseCell {
                subject_id: SubjectId::new(1).unwrap(),
                teacher: TeacherBinding::Explicit(principal(3)),
            }),
        },
        &a,
        f.clock.as_ref(),
    )
    .unwrap();
    let replaced = r.desktop_facts(0, f.clock.as_ref()).unwrap();
    assert_eq!(replaced.profiles.subject_profile.as_ref(), Some(&q));
    assert_eq!(
        replaced.materials_source.as_ref().unwrap().teacher_id,
        principal(3)
    );
    assert!(replaced.materials_page.is_none());
    *f.clock.0.lock().unwrap() = at(8, 40);
    let after = r.desktop_facts(0, f.clock.as_ref()).unwrap();
    assert_eq!(after.profiles.reason, ProfileSelectionReason::Public);
    assert!(after.profiles.subject_profile.is_none());
    assert_eq!(
        after.materials_source.as_ref().unwrap().teacher_id,
        principal(3)
    );
    assert_eq!(
        after.materials_source.as_ref().unwrap().kind,
        fc_domain::materials::MaterialSourceKind::RetainedBreak
    );
}
#[test]
fn profiles_postcommit_publication_failure_faults_new_operations_without_losing_document() {
    let mut f = Fixture::new();
    f.publish_fail.store(true, Ordering::Release);
    assert_eq!(f.save(f.request(1000, key(1, 2))), Err(E::Faulted));
    assert!(f
        .admin()
        .public_layout(&actor(1, TrustedOrigin::Local))
        .is_err());
    f.stop();
    let restored = startup_state(f.directory.db()).unwrap();
    assert!(restored.identity.installation.is_some());
    f.open();
    assert!(f
        .admin()
        .subject_profile(key(1, 2), &actor(1, TrustedOrigin::Local))
        .unwrap()
        .is_some());
}

#[test]
fn profiles_public_change_incompatible_current_profile_is_neutral_not_lost_or_global_failure() {
    use fc_ports::ScheduleRepository;
    let f = Fixture::new();
    let a = actor(1, TrustedOrigin::Local);
    let mut r = f.admin();
    let mut public = r.public_layout(&a).unwrap();
    for n in &mut public.layout.tree.nodes {
        if let NodeKind::Outlet { height, .. } = &mut n.kind {
            *height = AxisSize::Fixed(600)
        }
    }
    let public = r
        .execute_public_layout(public_request(1100, public), &a, f.clock.as_ref())
        .unwrap();
    let mut req = f.request(1101, key(1, 2));
    for n in &mut req.candidate.tree.nodes {
        if let NodeKind::Root { height, .. } = &mut n.kind {
            *height = AxisSize::Percent {
                basis_points: 10000,
                min: 0,
                max: 2160,
            }
        }
    }
    let p = f.save(req).unwrap();
    r.execute_schedule(
        fc_ports::ScheduleRequest {
            command_id: CommandId::new(1102).unwrap(),
            expected_revision: Revision::default(),
            candidate: schedule(),
        },
        &a,
        f.clock.as_ref(),
    )
    .unwrap();
    assert_eq!(
        r.desktop_facts(0, f.clock.as_ref())
            .unwrap()
            .profiles
            .subject_profile,
        Some(p.clone())
    );
    let mut altered = public;
    for n in &mut altered.layout.tree.nodes {
        if let NodeKind::Outlet { height, .. } = &mut n.kind {
            *height = AxisSize::Content {
                min: 0,
                preferred: 0,
                max: 2160,
            }
        }
    }
    let changed = r
        .execute_public_layout(public_request(1103, altered), &a, f.clock.as_ref())
        .unwrap();
    let facts = r.desktop_facts(0, f.clock.as_ref()).unwrap();
    assert_eq!(
        facts.profiles.reason,
        ProfileSelectionReason::LayoutMismatch
    );
    assert!(facts.profiles.subject_profile.is_none());
    assert_eq!(facts.profiles.public_layout, changed.layout);
    assert_eq!(r.subject_profile(p.key, &a).unwrap(), Some(p.clone()));
    let mut edit = p.clone();
    edit.materials.visible_rows = 3;
    let before = f.revision();
    assert_eq!(
        f.save(ProfileRequest {
            command_id: CommandId::new(1104).unwrap(),
            key: edit.key,
            expected_revision: p.revision,
            candidate: edit
        }),
        Err(E::Domain(ProfileError::Layout(LayoutError::SizeCycle)))
    );
    assert_eq!(f.revision(), before);
    let mut off = p.clone();
    off.enabled = false;
    assert!(
        !f.save(ProfileRequest {
            command_id: CommandId::new(1105).unwrap(),
            key: off.key,
            expected_revision: p.revision,
            candidate: off
        })
        .unwrap()
        .enabled
    );
}
#[test]
fn profiles_facts_codec_accepts_only_exact_legacy_shape_and_rejects_partial_new_data() {
    let f = Fixture::new();
    let facts = f.admin().desktop_facts(0, f.clock.as_ref()).unwrap();
    let current = display_codec::facts(&facts);
    assert_eq!(current["format"], json!(2));
    assert_eq!(display_codec::read_facts(&current).unwrap(), facts);
    let mut legacy = current.clone();
    legacy["format"] = json!(1);
    legacy.as_object_mut().unwrap().remove("profiles");
    assert_eq!(display_codec::read_facts(&legacy).unwrap(), facts);
    let mut partial = current.clone();
    partial.as_object_mut().unwrap().remove("profiles");
    assert_eq!(
        display_codec::read_facts(&partial),
        Err(StorageError::Database)
    );
    let mut mixed = current.clone();
    mixed["format"] = json!(1);
    assert_eq!(
        display_codec::read_facts(&mixed),
        Err(StorageError::Database)
    );
    let mut unknown = current;
    unknown["profiles"]["stray"] = json!(0);
    assert_eq!(
        display_codec::read_facts(&unknown),
        Err(StorageError::Database)
    );
    let mut too_many = f.request(1200, key(1, 2));
    let node = too_many.candidate.tree.nodes[1].clone();
    too_many
        .candidate
        .tree
        .nodes
        .resize(MAX_LAYOUT_NODES + 1, node);
    let before = f.revision();
    assert!(f.save(too_many).is_err());
    assert_eq!(f.revision(), before);
}
