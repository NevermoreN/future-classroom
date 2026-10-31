//! Sole-writer SQLite receipts; the exact-channel guard is an isolated fixture.
use super::*;
use fc_domain::auth::Principal;
use fc_ports::{AuthorityPublisher, MaterialAppendMetadata, MaterialPreparation};
use std::sync::atomic::{AtomicU64, AtomicUsize};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Time;
fn now() -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 3600).unwrap()
}
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        now()
    }
}
fn principal(id: u64) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn actor(id: u64, origin: TrustedOrigin) -> ActorContext {
    ActorContext {
        principal_id: principal(id),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(now().date, 7200).unwrap(),
        origin,
    }
}
fn scope() -> MaterialScope {
    MaterialScope {
        teacher_id: principal(2),
        subject_id: SubjectId::new(1).unwrap(),
    }
}
fn state() -> IdentityState {
    IdentityState {
        revision: Revision::new(1),
        installation: Some(Installation {
            school_id: SchoolId::new(1).unwrap(),
            school_name: "Synthetic school".into(),
            current_class: Some(ClassId::new(1).unwrap()),
        }),
        accounts: (1..=3)
            .map(|id| UserAccount {
                principal: Principal {
                    id: principal(id),
                    school_id: SchoolId::new(1).unwrap(),
                    role: if id == 1 {
                        Role::GlobalAdmin
                    } else {
                        Role::TeacherAdmin
                    },
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("user{id}"),
                display_name: format!("User {id}"),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            })
            .collect(),
        classes: vec![SchoolClass {
            id: ClassId::new(1).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            name: "Class".into(),
            enabled: true,
        }],
        subjects: vec![SchoolSubject {
            id: SubjectId::new(1).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            name: "Subject".into(),
            enabled: true,
            color: Rgb {
                red: 1,
                green: 2,
                blue: 3,
            },
            revision: Revision::new(1),
        }],
        assignments: [2, 3]
            .into_iter()
            .map(|id| TeachingAssignment {
                teacher_id: principal(id),
                class_id: ClassId::new(1).unwrap(),
                subject_id: SubjectId::new(1).unwrap(),
                enabled: true,
            })
            .collect(),
        default_teachers: vec![],
    }
}
struct Publisher {
    fail: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
    live: Arc<AtomicBool>,
}
impl AuthorityPublisher for Publisher {
    type Snapshot = Revision;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, s: &Revision) -> Revision {
        *s
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        if self.revoke.swap(false, Ordering::AcqRel) {
            self.live.store(false, Ordering::Release);
        }
        Ok(())
    }
    fn publish(&mut self, _: (), _: Revision) -> Result<(), ()> {
        if self.fail.swap(false, Ordering::AcqRel) {
            Err(())
        } else {
            Ok(())
        }
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn fault(&mut self) {}
}
struct Fixture {
    writer: Option<StorageWriter<Revision>>,
    directory: PathBuf,
    live: Arc<AtomicBool>,
    checks: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "FutureClassroom-MaterialOutcome-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut f = Self {
            writer: None,
            directory,
            live: Arc::new(AtomicBool::new(true)),
            checks: Arc::new(AtomicUsize::new(usize::MAX)),
            fail: Arc::new(AtomicBool::new(false)),
            revoke: Arc::new(AtomicBool::new(false)),
        };
        f.open();
        let identity = state();
        identity.validate().unwrap();
        let mut mutations = vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&identity)).unwrap(),
        }];
        for id in 1..=3 {
            for kind in ["password", "recovery"] {
                mutations.push(Mutation::Put {
                    key: credential_key(principal(id), kind, 1),
                    value: vec![id as u8],
                });
            }
        }
        f.commit(mutations);
        f
    }
    fn open(&mut self) {
        self.writer = Some(
            StorageWriter::open(
                self.directory.join("state.sqlite3"),
                8,
                Publisher {
                    fail: self.fail.clone(),
                    revoke: self.revoke.clone(),
                    live: self.live.clone(),
                },
            )
            .unwrap(),
        );
    }
    fn reopen(&mut self) {
        let _ = self.writer.take().unwrap().shutdown();
        self.open();
    }
    fn handle(&self) -> StorageHandle<Revision> {
        self.writer.as_ref().unwrap().handle()
    }
    fn revision(&self) -> Revision {
        self.handle().try_revision().unwrap().wait().unwrap()
    }
    fn commit(&self, mutations: Vec<Mutation>) {
        let r = self.revision();
        self.handle()
            .try_commit(
                CommitCommand::new(format!("fixture-{}", r.get()), r, mutations).unwrap(),
                r.next().unwrap(),
            )
            .unwrap()
            .wait()
            .unwrap();
    }
    fn replace(&self, key: String, value: Value) {
        self.commit(vec![Mutation::Put {
            key,
            value: codec::bytes(&value).unwrap(),
        }]);
    }
    fn identity(&self, edit: impl FnOnce(&mut IdentityState)) {
        let mut s = state();
        edit(&mut s);
        s.validate().unwrap();
        self.replace(IDENTITY.into(), codec::identity(&s));
    }
    fn repo(&self, exact: ActorContext) -> BusinessRepository<Revision> {
        let live = self.live.clone();
        let checks = self.checks.clone();
        BusinessRepository::new(self.handle(), Arc::new(Time), Arc::new(|_, r| Ok(r)))
            .with_session_guard(Arc::new(move |state, a| {
                let remaining = checks
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                        if n == usize::MAX {
                            None
                        } else {
                            Some(n.saturating_sub(1))
                        }
                    })
                    .unwrap_or(usize::MAX);
                if a != &exact || !live.load(Ordering::Acquire) || remaining == 0 {
                    return Err(material_error(MaterialError::Permission(
                        PermissionError::StaleAuthorization,
                    )));
                }
                state
                    .authority(a.principal_id)
                    .map_err(StorageError::Identity)?
                    .validate_actor(a, now())
                    .map_err(|e| material_error(MaterialError::Permission(e)))
            }))
    }
    fn value(&self, k: String) -> Option<Value> {
        self.handle()
            .try_get(k)
            .unwrap()
            .wait()
            .unwrap()
            .map(|r| codec::value(&r.value).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(w) = self.writer.take() {
            let _ = w.shutdown();
        }
        std::fs::remove_dir_all(&self.directory).expect("material outcome cleanup");
    }
}
struct Preparation;
impl MaterialPreparation for Preparation {
    type Input = ();
    fn prepare(&mut self, _: ()) -> Result<MaterialResource, MaterialRepositoryError> {
        Ok(MaterialResource {
            version_id: ResourceVersionId::new(10).unwrap(),
            sha256: [7; 32],
            kind: MaterialKind::Txt,
            bytes: 7,
        })
    }
}
fn prepared(id: u64) -> PreparedMaterialAppend {
    fc_ports::prepare_material_append(
        &mut Preparation,
        (),
        MaterialAppendMetadata {
            command_id: CommandId::new(id).unwrap(),
            scope: scope(),
            material_id: MaterialId::new(20).unwrap(),
            display_name: "synthetic.txt".into(),
            allowed_class_ids: vec![ClassId::new(1).unwrap()],
        },
    )
    .unwrap()
}
fn append(f: &Fixture, id: u64) -> MaterialRecord {
    let a = actor(2, TrustedOrigin::Web);
    f.repo(a.clone())
        .append_material(prepared(id), &a, &Time)
        .unwrap()
}
fn outcome(
    f: &Fixture,
    id: u64,
) -> Result<Option<MaterialCommandOutcome>, MaterialRepositoryError> {
    let a = actor(2, TrustedOrigin::Web);
    f.repo(a.clone())
        .material_outcome(CommandId::new(id).unwrap(), &a)
}

#[test]
fn web_append_delete_receipts_are_historical_original_sender_only_and_survive_restart() {
    let mut f = Fixture::new();
    let a = actor(2, TrustedOrigin::Web);
    let record = append(&f, 100);
    let receipt = f.value(key("receipts", 100)).unwrap();
    let before = f.revision();
    let first = outcome(&f, 100).unwrap().unwrap();
    assert_eq!(first.kind, MaterialCommandKind::Append);
    assert_eq!(first.record, record);
    assert_eq!(f.revision(), before);
    assert_eq!(
        f.repo(a.clone())
            .append_material(prepared(100), &a, &Time)
            .unwrap(),
        record
    );
    assert_eq!(f.value(key("receipts", 100)), Some(receipt));
    assert_eq!(f.revision(), before);
    for id in [1, 3] {
        let other = actor(id, TrustedOrigin::Web);
        assert_eq!(
            f.repo(other.clone())
                .material_outcome(CommandId::new(100).unwrap(), &other)
                .unwrap(),
            None
        );
    }
    assert_eq!(outcome(&f, 999).unwrap(), None);
    let deleted = f
        .repo(a.clone())
        .delete_material(
            MaterialDeleteRequest {
                command_id: CommandId::new(101).unwrap(),
                scope: scope(),
                material_id: record.id,
                expected_material_revision: record.revision,
            },
            &a,
            &Time,
        )
        .unwrap();
    assert_eq!(outcome(&f, 100).unwrap().unwrap().record, record);
    let result = outcome(&f, 101).unwrap().unwrap();
    assert_eq!(result.kind, MaterialCommandKind::Delete);
    assert_eq!(result.record, deleted);
    f.reopen();
    assert_eq!(outcome(&f, 100).unwrap(), Some(first));
    assert_eq!(outcome(&f, 101).unwrap(), Some(result));
}
#[test]
fn outcome_requires_exact_live_web_and_current_authority_even_for_unknown() {
    let f = Fixture::new();
    append(&f, 110);
    let a = actor(2, TrustedOrigin::Web);
    let r = f.repo(a.clone());
    let local = actor(2, TrustedOrigin::Local);
    assert!(f
        .repo(local.clone())
        .material_outcome(CommandId::new(110).unwrap(), &local)
        .is_err());
    let foreign = actor(3, TrustedOrigin::Web);
    assert!(r
        .material_outcome(CommandId::new(110).unwrap(), &foreign)
        .is_err());
    f.live.store(false, Ordering::Release);
    assert!(outcome(&f, 110).is_err());
    assert!(outcome(&f, 999).is_err());
    f.live.store(true, Ordering::Release);
    f.identity(|s| {
        let mut other_subject = s.subjects[0].clone();
        other_subject.id = SubjectId::new(2).unwrap();
        other_subject.name = "Other subject".into();
        s.subjects.push(other_subject);
        for assignment in &mut s.assignments {
            if assignment.teacher_id == principal(2) {
                assignment.subject_id = SubjectId::new(2).unwrap();
            }
        }
    });
    assert!(outcome(&f, 110).is_err());
    f.identity(|s| s.accounts[1].principal.auth_epoch = AuthEpoch::new(2));
    assert!(outcome(&f, 110).is_err());
    f.identity(|s| s.accounts[1].principal.enabled = false);
    assert!(outcome(&f, 110).is_err());
    f.identity(|s| s.accounts[1].must_change_password = true);
    assert!(outcome(&f, 110).is_err());
}
#[test]
fn query_final_guard_gates_owned_foreign_and_unknown_without_writes() {
    let f = Fixture::new();
    append(&f, 120);
    let before = f.revision();
    for (id, principal_id) in [(120, 2), (120, 1), (999, 2)] {
        f.checks.store(1, Ordering::Release);
        let a = actor(principal_id, TrustedOrigin::Web);
        assert!(f
            .repo(a.clone())
            .material_outcome(CommandId::new(id).unwrap(), &a)
            .is_err());
        f.checks.store(usize::MAX, Ordering::Release);
    }
    assert_eq!(f.revision(), before);
}
#[test]
fn native_legacy_receipt_keeps_schema_and_never_gets_fabricated_web_sender() {
    let f = Fixture::new();
    let local = actor(1, TrustedOrigin::Local);
    let r = f
        .repo(local.clone())
        .append_material(prepared(130), &local, &Time)
        .unwrap();
    assert_eq!(r.id.get(), 20);
    let v = f.value(key("receipts", 130)).unwrap();
    codec::object_fields(&v, &["fingerprint", "record", "result_digest"]).unwrap();
    assert_eq!(f.value(key("web-outcomes", 130)), None);
    let web = actor(1, TrustedOrigin::Web);
    assert_eq!(
        f.repo(web.clone())
            .material_outcome(CommandId::new(130).unwrap(), &web)
            .unwrap(),
        None
    );
}
#[test]
fn receipt_and_sidecar_digest_shape_and_kind_mismatches_fail_closed() {
    let f = Fixture::new();
    append(&f, 140);
    let meta = f.value(key("web-outcomes", 140)).unwrap();
    let receipt = f.value(key("receipts", 140)).unwrap();
    for changed in [
        {
            let mut v = meta.clone();
            v["unexpected"] = json!(true);
            v
        },
        {
            let mut v = meta.clone();
            v["fingerprint"] = json!([1]);
            v
        },
        {
            let mut v = meta.clone();
            v["result_digest"] = json!(vec![1; 32]);
            v
        },
        {
            let mut v = meta.clone();
            v["operation"] = json!("delete");
            v
        },
    ] {
        f.replace(key("web-outcomes", 140), changed);
        assert!(outcome(&f, 140).is_err());
    }
    f.replace(key("web-outcomes", 140), meta);
    let mut changed = receipt;
    changed["record"]["name"] = json!("changed.txt");
    f.replace(key("receipts", 140), changed);
    assert!(outcome(&f, 140).is_err());
}
#[test]
fn precommit_revocation_rolls_back_both_docs_but_postcommit_fault_keeps_queryable_history() {
    let mut f = Fixture::new();
    let a = actor(2, TrustedOrigin::Web);
    let before = f.revision();
    f.revoke.store(true, Ordering::Release);
    assert!(f
        .repo(a.clone())
        .append_material(prepared(150), &a, &Time)
        .is_err());
    assert_eq!(f.revision(), before);
    assert!(f.value(key("receipts", 150)).is_none());
    assert!(f.value(key("web-outcomes", 150)).is_none());
    f.live.store(true, Ordering::Release);
    f.fail.store(true, Ordering::Release);
    assert_eq!(
        f.repo(a.clone()).append_material(prepared(151), &a, &Time),
        Err(MaterialRepositoryError::Faulted)
    );
    assert!(outcome(&f, 151).is_err());
    f.reopen();
    let result = outcome(&f, 151).unwrap().unwrap();
    assert_eq!(result.kind, MaterialCommandKind::Append);
    assert_eq!(result.record.id.get(), 20);
}

#[test]
fn a_new_exact_session_can_read_its_old_commit_but_old_epoch_cannot() {
    let f = Fixture::new();
    let original = append(&f, 160);
    f.identity(|s| s.accounts[1].principal.auth_epoch = AuthEpoch::new(2));
    assert!(outcome(&f, 160).is_err());
    let mut fresh = actor(2, TrustedOrigin::Web);
    fresh.auth_epoch = AuthEpoch::new(2);
    assert_eq!(
        f.repo(fresh.clone())
            .material_outcome(CommandId::new(160).unwrap(), &fresh)
            .unwrap()
            .unwrap()
            .record,
        original
    );
    let mut meta = f.value(key("web-outcomes", 160)).unwrap();
    meta["principal"] = json!(1);
    meta["actor"]["principal"] = json!(1);
    f.replace(key("web-outcomes", 160), meta);
    let global = actor(1, TrustedOrigin::Web);
    assert!(f
        .repo(global.clone())
        .material_outcome(CommandId::new(160).unwrap(), &global)
        .is_err());
}

#[test]
fn stale_delete_cas_never_creates_outcome_and_committed_delete_fault_is_recoverable() {
    let mut f = Fixture::new();
    let original = append(&f, 170);
    let a = actor(2, TrustedOrigin::Web);
    let request = |id, revision| MaterialDeleteRequest {
        command_id: CommandId::new(id).unwrap(),
        scope: scope(),
        material_id: original.id,
        expected_material_revision: revision,
    };
    assert!(matches!(
        f.repo(a.clone())
            .delete_material(request(171, Revision::default()), &a, &Time),
        Err(MaterialRepositoryError::Domain(
            MaterialError::RevisionConflict { .. }
        ))
    ));
    assert_eq!(outcome(&f, 171).unwrap(), None);
    assert_eq!(f.value(key("receipts", 171)), None);
    f.fail.store(true, Ordering::Release);
    assert_eq!(
        f.repo(a.clone())
            .delete_material(request(172, original.revision), &a, &Time),
        Err(MaterialRepositoryError::Faulted)
    );
    f.reopen();
    let result = outcome(&f, 172).unwrap().unwrap();
    assert_eq!(result.kind, MaterialCommandKind::Delete);
    assert_eq!(result.record.status, MaterialStatus::Deleted);
    assert_eq!(result.record.revision, original.revision.next().unwrap());
    assert_eq!(outcome(&f, 170).unwrap().unwrap().record, original);
}

#[test]
fn managed_download_missing_version_or_deleted_record_is_not_found_without_mutation() {
    let f = Fixture::new();
    let record = append(&f, 180);
    let a = actor(2, TrustedOrigin::Web);
    let mut repo = f.repo(a.clone());
    let before = f.revision();
    assert_eq!(
        repo.managed_material(scope(), record.id, ResourceVersionId::new(999).unwrap(), &a),
        Err(MaterialRepositoryError::Domain(MaterialError::NotFound))
    );
    assert_eq!(f.revision(), before);
    assert_eq!(
        repo.managed_material(scope(), record.id, record.resource.version_id, &a)
            .unwrap(),
        record
    );
    repo.delete_material(
        MaterialDeleteRequest {
            command_id: CommandId::new(181).unwrap(),
            scope: scope(),
            material_id: record.id,
            expected_material_revision: record.revision,
        },
        &a,
        &Time,
    )
    .unwrap();
    let deleted_revision = f.revision();
    assert_eq!(
        repo.managed_material(scope(), record.id, record.resource.version_id, &a),
        Err(MaterialRepositoryError::Domain(MaterialError::NotFound))
    );
    assert_eq!(f.revision(), deleted_revision);
}
