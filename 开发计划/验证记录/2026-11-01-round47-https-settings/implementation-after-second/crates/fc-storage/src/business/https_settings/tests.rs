//! Real SQLite and original sole-writer fences, not a model of configuration IO.
use super::*;
use fc_ports::AuthorityPublisher;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn now() -> fc_domain::time::LocalDateTime {
    fc_domain::time::LocalDateTime::new(fc_domain::time::LocalDate::new(2026, 11, 1).unwrap(), 3600)
        .unwrap()
}
struct Time;
impl Clock for Time {
    fn now(&self) -> fc_domain::time::LocalDateTime {
        now()
    }
}
fn actor(id: u64, origin: TrustedOrigin) -> ActorContext {
    ActorContext {
        principal_id: PrincipalId::new(id).unwrap(),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: fc_domain::time::LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 11, 2).unwrap(),
            0,
        )
        .unwrap(),
        origin,
    }
}
struct Publisher {
    active: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
    published: Arc<AtomicU64>,
    late_cancel: Arc<std::sync::Mutex<Option<Arc<AtomicBool>>>>,
}
impl AuthorityPublisher for Publisher {
    type Snapshot = u64;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, value: &u64) -> Revision {
        Revision::new(*value)
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        if self.revoke.swap(false, Ordering::AcqRel) {
            self.active.store(false, Ordering::Release);
        }
        Ok(())
    }
    fn publish(&mut self, _: (), _: u64) -> Result<(), ()> {
        self.published.fetch_add(1, Ordering::AcqRel);
        if let Some(cancel) = self.late_cancel.lock().unwrap().take() {
            cancel.store(false, Ordering::Release);
        }
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn fault(&mut self) {}
}
struct Fixture {
    root: PathBuf,
    writer: Option<StorageWriter<u64>>,
    active: Arc<AtomicBool>,
    permit: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
    published: Arc<AtomicU64>,
    late_cancel: Arc<std::sync::Mutex<Option<Arc<AtomicBool>>>>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-HttpsSettings-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut value = Self {
            root,
            writer: None,
            active: Arc::new(AtomicBool::new(true)),
            permit: Arc::new(AtomicBool::new(true)),
            revoke: Arc::new(AtomicBool::new(false)),
            published: Arc::new(AtomicU64::new(0)),
            late_cancel: Arc::new(std::sync::Mutex::new(None)),
        };
        value.open();
        let mut mutations = vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&identity())).unwrap(),
        }];
        for id in 1..=4 {
            for kind in ["password", "recovery"] {
                mutations.push(Mutation::Put {
                    key: credential_key(PrincipalId::new(id).unwrap(), kind, 1),
                    value: vec![id as u8],
                });
            }
        }
        value.commit(mutations);
        value
    }
    fn open(&mut self) {
        self.writer = Some(
            StorageWriter::open(
                self.root.join("state.sqlite3"),
                8,
                Publisher {
                    active: self.active.clone(),
                    revoke: self.revoke.clone(),
                    published: self.published.clone(),
                    late_cancel: self.late_cancel.clone(),
                },
            )
            .unwrap(),
        );
    }
    fn reopen(&mut self) {
        self.writer.take().unwrap().shutdown().unwrap();
        self.open();
    }
    fn handle(&self) -> StorageHandle<u64> {
        self.writer.as_ref().unwrap().handle()
    }
    fn revision(&self) -> Revision {
        self.handle().try_revision().unwrap().wait().unwrap()
    }
    fn raw(&self, key: &str) -> Option<Document> {
        self.handle().try_get(key.into()).unwrap().wait().unwrap()
    }
    fn commit(&self, mutations: Vec<Mutation>) {
        let rev = self.revision();
        self.handle()
            .try_commit(
                CommitCommand::new(format!("https-fixture-{}", rev.get()), rev, mutations).unwrap(),
                rev.get() + 1,
            )
            .unwrap()
            .wait()
            .unwrap();
    }
    fn base_repository(&self, captured: ActorContext) -> BusinessRepository<u64> {
        let active = self.active.clone();
        BusinessRepository::new(
            self.handle(),
            Arc::new(Time),
            Arc::new(|_, rev| Ok(rev.get())),
        )
        .with_session_guard(Arc::new(move |_, actual| {
            if active.load(Ordering::Acquire) && actual == &captured {
                Ok(())
            } else {
                Err(StorageError::ExecutionCancelled)
            }
        }))
    }
    fn repository(&self, captured: ActorContext) -> BusinessRepository<u64> {
        let permit = self.permit.clone();
        self.base_repository(captured)
            .with_https_configuration_guard(Arc::new(move || permit.load(Ordering::Acquire)))
    }
    fn read(&self) -> HttpsSettingsState {
        self.repository(actor(1, TrustedOrigin::Local))
            .https_settings(&actor(1, TrustedOrigin::Local))
            .unwrap()
    }
    fn save(
        &self,
        request: HttpsSettingsRequest,
    ) -> Result<HttpsSettingsState, HttpsSettingsRepositoryError> {
        self.repository(actor(1, TrustedOrigin::Local))
            .execute_https_settings(request, &actor(1, TrustedOrigin::Local))
    }
    fn replace_identity(&self, value: IdentityState) {
        self.commit(vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&value)).unwrap(),
        }]);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(w) = self.writer.take() {
            w.shutdown().unwrap();
        }
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn identity() -> IdentityState {
    let school = SchoolId::new(1).unwrap();
    IdentityState {
        revision: Revision::new(1),
        installation: Some(Installation {
            school_id: school,
            school_name: "Synthetic HTTPS school".into(),
            current_class: Some(ClassId::new(1).unwrap()),
        }),
        accounts: (1..=4)
            .map(|id| UserAccount {
                principal: Principal {
                    id: PrincipalId::new(id).unwrap(),
                    school_id: school,
                    role: match id {
                        1 | 4 => Role::GlobalAdmin,
                        2 => Role::TeacherAdmin,
                        _ => Role::User,
                    },
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("user{id}"),
                display_name: format!("User{id}"),
                must_change_password: id == 4,
                credential_version: 1,
                recovery_version: 1,
            })
            .collect(),
        classes: vec![SchoolClass {
            id: ClassId::new(1).unwrap(),
            school_id: school,
            name: "Class1".into(),
            enabled: true,
        }],
        subjects: vec![SchoolSubject {
            id: SubjectId::new(1).unwrap(),
            school_id: school,
            name: "Subject1".into(),
            enabled: true,
            color: Rgb {
                red: 70,
                green: 100,
                blue: 150,
            },
            revision: Revision::new(1),
        }],
        assignments: vec![TeachingAssignment {
            teacher_id: PrincipalId::new(2).unwrap(),
            class_id: ClassId::new(1).unwrap(),
            subject_id: SubjectId::new(1).unwrap(),
            enabled: true,
        }],
        default_teachers: vec![],
    }
}
fn config(host: &str, port: u16) -> HttpsConfiguration {
    HttpsConfiguration::new(
        HttpsHost::parse(host).unwrap(),
        port,
        HttpsCertificateRef::parse(&"ab".repeat(20)).unwrap(),
    )
    .unwrap()
}
fn request(
    id: u64,
    base: &HttpsSettingsState,
    configuration: Option<HttpsConfiguration>,
) -> HttpsSettingsRequest {
    HttpsSettingsRequest {
        command_id: CommandId::new(id).unwrap(),
        expected_revision: base.settings.revision,
        expected_identity_revision: base.identity_revision,
        configuration,
    }
}
#[test]
fn canonical_save_clear_replay_and_cold_reopen_preserve_identity_and_never_create_voice_or_autostart(
) {
    let mut f = Fixture::new();
    let base = f.read();
    assert_eq!(base.settings, HttpsSettings::default());
    let before = f
        .repository(actor(1, TrustedOrigin::Local))
        .load_identity()
        .unwrap();
    let saved = f
        .save(request(1, &base, Some(config("Board.School.", 8443))))
        .unwrap();
    assert_eq!(
        saved.settings.configuration.as_ref().unwrap().host.as_str(),
        "board.school"
    );
    let rev = f.revision();
    assert_eq!(
        f.save(request(1, &base, Some(config("BOARD.school", 8443))))
            .unwrap(),
        saved
    );
    assert_eq!(f.revision(), rev);
    assert_eq!(
        f.repository(actor(1, TrustedOrigin::Local))
            .load_identity()
            .unwrap(),
        before
    );
    for key in ["business/voice/v1", "business/autostart/v1"] {
        assert!(f.raw(key).is_none());
    }
    f.reopen();
    assert_eq!(f.read(), saved);
    assert!(matches!(
        f.save(request(1, &base, Some(config("board.school", 443)))),
        Err(HttpsSettingsRepositoryError::IdempotencyConflict)
    ));
    assert!(matches!(
        f.save(request(2, &base, None)),
        Err(HttpsSettingsRepositoryError::Domain(
            HttpsSettingsError::RevisionConflict { .. }
        ))
    ));
    let cleared = f.save(request(3, &saved, None)).unwrap();
    assert!(cleared.settings.configuration.is_none());
    assert_eq!(cleared.settings.revision.get(), 2);
}
#[test]
fn local_global_exact_session_must_change_and_default_deny_configuration_permit() {
    let f = Fixture::new();
    let base = f.read();
    let before = f.revision();
    for a in [
        actor(1, TrustedOrigin::Web),
        actor(2, TrustedOrigin::Local),
        actor(3, TrustedOrigin::Local),
        actor(4, TrustedOrigin::Local),
    ] {
        let mut r = f.repository(a.clone());
        assert!(r.https_settings(&a).is_err());
        assert!(r
            .execute_https_settings(request(1, &base, Some(config("board", 8443))), &a)
            .is_err());
    }
    let mut wrong = actor(1, TrustedOrigin::Local);
    wrong.expires_at = now();
    assert!(f
        .repository(actor(1, TrustedOrigin::Local))
        .https_settings(&wrong)
        .is_err());
    assert!(f
        .base_repository(actor(1, TrustedOrigin::Local))
        .execute_https_settings(request(2, &base, None), &actor(1, TrustedOrigin::Local))
        .is_err());
    let mut tightened = f
        .repository(actor(1, TrustedOrigin::Local))
        .with_https_configuration_guard(Arc::new(|| false))
        .with_https_configuration_guard(Arc::new(|| true));
    assert!(tightened
        .execute_https_settings(request(3, &base, None), &actor(1, TrustedOrigin::Local))
        .is_err());
    assert_eq!(f.revision(), before);
}
#[test]
fn identity_cas_and_config_cas_are_independent_even_when_host_is_unchanged() {
    let f = Fixture::new();
    let base = f.read();
    let mut identity = identity();
    identity.revision = identity.revision.next().unwrap();
    identity.installation.as_mut().unwrap().school_name = "Changed".into();
    f.replace_identity(identity);
    let before = f.revision();
    assert!(matches!(
        f.save(request(1, &base, Some(config("school", 443)))),
        Err(HttpsSettingsRepositoryError::Domain(
            HttpsSettingsError::IdentityRevisionConflict { .. }
        ))
    ));
    assert_eq!(f.revision(), before);
    let next = f.read();
    assert_eq!(next.settings, base.settings);
    assert!(f
        .save(request(2, &next, Some(config("school", 443))))
        .is_ok());
}
#[test]
fn precommit_session_revocation_or_disabled_permit_loss_rolls_back_without_publication() {
    let f = Fixture::new();
    let base = f.read();
    let before = f.revision();
    let publications = f.published.load(Ordering::Acquire);
    f.revoke.store(true, Ordering::Release);
    assert!(f
        .save(request(1, &base, Some(config("school", 443))))
        .is_err());
    f.active.store(true, Ordering::Release);
    assert_eq!(f.read(), base);
    assert_eq!(f.revision(), before);
    assert_eq!(f.published.load(Ordering::Acquire), publications);
    let permit = f.permit.clone();
    let mut r = f.repository(actor(1, TrustedOrigin::Local));
    r.factory = Arc::new(move |_, rev| {
        permit.store(false, Ordering::Release);
        Ok(rev.get())
    });
    assert!(r
        .execute_https_settings(request(2, &base, None), &actor(1, TrustedOrigin::Local))
        .is_err());
    f.permit.store(true, Ordering::Release);
    assert_eq!(f.read(), base);
    assert_eq!(f.revision(), before);
    assert!(f.raw("business/result/https-settings/2").is_none());
    assert!(f.save(request(3, &base, None)).is_ok());
}
struct Held {
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
impl TransactionTask<u64> for Held {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        _: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<u64>, StorageError> {
        self.entered.send(()).unwrap();
        self.release
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Ok(TransactionDecision {
            outcome: vec![],
            commit: None,
            precommit: None,
        })
    }
}
#[test]
fn queued_execution_cancel_and_precommit_cancel_use_original_writer_fence_without_faulting() {
    let f = Fixture::new();
    let base = f.read();
    let before = f.revision();
    let (enter, arrived) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let held = f
        .handle()
        .try_transaction(Held {
            entered: enter,
            release: wait,
        })
        .unwrap();
    arrived
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let active = Arc::new(AtomicBool::new(true));
    let check = active.clone();
    let r = f
        .repository(actor(1, TrustedOrigin::Local))
        .with_execution_fence(Arc::new(move || check.load(Ordering::Acquire)));
    let queued = r
        .handle
        .try_transaction(SaveSettings {
            request: request(1, &base, Some(config("school", 443))),
            actor: actor(1, TrustedOrigin::Local),
            clock: r.clock.clone(),
            session: r.session.clone(),
            factory: r.factory.clone(),
            guard: r.https_configuration_guard.clone().unwrap(),
        })
        .unwrap();
    active.store(false, Ordering::Release);
    release.send(()).unwrap();
    held.wait().unwrap();
    assert_eq!(queued.wait(), Err(StorageError::ExecutionCancelled));
    assert_eq!(f.revision(), before);
    let active = Arc::new(AtomicBool::new(true));
    let check = active.clone();
    let cancel = active.clone();
    let mut r = f
        .repository(actor(1, TrustedOrigin::Local))
        .with_execution_fence(Arc::new(move || check.load(Ordering::Acquire)));
    r.factory = Arc::new(move |_, rev| {
        cancel.store(false, Ordering::Release);
        Ok(rev.get())
    });
    assert!(r
        .execute_https_settings(request(2, &base, None), &actor(1, TrustedOrigin::Local))
        .is_err());
    assert_eq!(f.read(), base);
    assert_eq!(f.revision(), before);
    assert!(f.save(request(3, &base, None)).is_ok());
}
#[test]
fn strict_protected_document_rejects_bad_shape_store_noncanonical_values_and_corruption_on_startup()
{
    let f = Fixture::new();
    let base = f.read();
    let saved = f
        .save(request(1, &base, Some(config("board", 443))))
        .unwrap();
    let valid = settings(&saved.settings);
    let mut bad = valid.clone();
    bad["configuration"]["port"] = json!(0);
    let mut other_store = valid.clone();
    other_store["configuration"]["certificate_store"] = json!("LOCAL_MACHINE_MY");
    let mut old = valid.clone();
    old["format"] = json!(2);
    let mut extra = valid.clone();
    extra["enabled"] = json!(true);
    let mut upper = valid.clone();
    upper["configuration"]["host"] = json!("BOARD");
    for malformed in [bad, other_store, old, extra, upper, json!({"format":1})] {
        f.commit(vec![Mutation::Put {
            key: DOCUMENT.into(),
            value: codec::bytes(&malformed).unwrap(),
        }]);
        assert!(f
            .repository(actor(1, TrustedOrigin::Local))
            .https_settings(&actor(1, TrustedOrigin::Local))
            .is_err());
        assert!(startup_state(f.root.join("state.sqlite3")).is_err());
    }
}

fn recovered_config(path: &Path) -> Option<Document> {
    // Actual independent SQLite reopen and full startup validation. A recovery
    // candidate never supplies a live local management lease to these assertions.
    startup_state(path).unwrap();
    let db = open_connection(path).unwrap();
    TransactionView(&db).get(DOCUMENT).unwrap()
}
#[test]
fn ordinary_restore_keeps_target_security_row_exact_and_export_omits_configuration() {
    let f = Fixture::new();
    let admin = actor(1, TrustedOrigin::Local);
    let r = f.repository(admin.clone());
    r.ensure_recovery_installation([7; 32]).unwrap();
    f.save(request(1, &f.read(), Some(config("old.school", 8443))))
        .unwrap();
    let backup = f.root.join("ordinary.sqlite3");
    r.try_export_ordinary(backup.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let db = open_connection(&backup).unwrap();
    assert!(TransactionView(&db).get(DOCUMENT).unwrap().is_none());
    drop(db);
    f.save(request(2, &f.read(), Some(config("target.school", 443))))
        .unwrap();
    let exact = f.raw(DOCUMENT).unwrap();
    let preview = f.root.join("preview.sqlite3");
    let receipt = r
        .try_preflight_ordinary_restore(backup.clone(), preview.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(recovered_config(&preview), Some(exact.clone()));
    let barrier = r
        .try_advance_recovery_barrier(receipt.current_revision, &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let candidate = f.root.join("candidate.sqlite3");
    r.try_prepare_ordinary_restore(backup, candidate.clone(), barrier, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(recovered_config(&candidate), Some(exact));
}
fn mapped_case(target_has_config: bool) {
    let source = Fixture::new();
    let admin = actor(1, TrustedOrigin::Local);
    let s = source.repository(admin.clone());
    s.ensure_recovery_installation([7; 32]).unwrap();
    source
        .save(request(
            1,
            &source.read(),
            Some(config("source.school", 8443)),
        ))
        .unwrap();
    let backup = source.root.join("ordinary.sqlite3");
    s.try_export_ordinary(backup.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let target = Fixture::new();
    let t = target.repository(admin.clone());
    t.ensure_recovery_installation([9; 32]).unwrap();
    if target_has_config {
        target
            .save(request(
                1,
                &target.read(),
                Some(config("target.school", 9443)),
            ))
            .unwrap();
    }
    let exact = target.raw(DOCUMENT);
    let inspected = t
        .try_inspect_migration_source(backup.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let mapped = target.root.join("mapped.sqlite3");
    let mapping = fc_domain::recovery::MigrationMapping {
        source_school_id: SchoolId::new(1).unwrap(),
        target_school_id: SchoolId::new(1).unwrap(),
        classes: vec![(ClassId::new(1).unwrap(), ClassId::new(1).unwrap())],
        subjects: vec![(SubjectId::new(1).unwrap(), SubjectId::new(1).unwrap())],
        principals: (1..=4)
            .map(|id| (PrincipalId::new(id).unwrap(), PrincipalId::new(id).unwrap()))
            .collect(),
    };
    t.try_map_ordinary(
        backup,
        mapped.clone(),
        inspected,
        mapping,
        &admin,
        Arc::new(|| true),
    )
    .unwrap()
    .wait()
    .unwrap();
    let preview = target.root.join("preview.sqlite3");
    let receipt = t
        .try_preflight_ordinary_restore(mapped.clone(), preview.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(recovered_config(&preview), exact);
    let barrier = t
        .try_advance_recovery_barrier(receipt.current_revision, &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let candidate = target.root.join("candidate.sqlite3");
    t.try_prepare_ordinary_restore(mapped, candidate.clone(), barrier, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(recovered_config(&candidate), exact);
}
#[test]
fn mapped_restore_keeps_current_target_configuration_exact() {
    mapped_case(true);
}
#[test]
fn mapped_restore_keeps_missing_target_unconfigured_not_source_configuration() {
    mapped_case(false);
}
#[test]
fn ordinary_input_injected_protected_https_configuration_is_rejected() {
    let f = Fixture::new();
    let admin = actor(1, TrustedOrigin::Local);
    let r = f.repository(admin.clone());
    r.ensure_recovery_installation([7; 32]).unwrap();
    let backup = f.root.join("ordinary.sqlite3");
    r.try_export_ordinary(backup.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let row = HttpsSettings {
        revision: Revision::new(1),
        configuration: Some(config("injected.school", 443)),
    };
    let db = open_connection(&backup).unwrap();
    db.execute(
        "INSERT INTO documents(key,value,revision) VALUES (?1,?2,1)",
        rusqlite::params![DOCUMENT, codec::bytes(&settings(&row)).unwrap()],
    )
    .unwrap();
    drop(db);
    let before = f.revision();
    assert!(r
        .try_preflight_ordinary_restore(
            backup,
            f.root.join("rejected.sqlite3"),
            &admin,
            Arc::new(|| true)
        )
        .unwrap()
        .wait()
        .is_err());
    assert_eq!(f.revision(), before);
    assert!(f.raw(DOCUMENT).is_none());
}
#[test]
fn actual_commit_then_canceled_delivery_is_unknown_not_rollback_and_original_command_reconciles() {
    let f = Fixture::new();
    let base = f.read();
    let original = request(1, &base, Some(config("school", 443)));
    let active = Arc::new(AtomicBool::new(true));
    *f.late_cancel.lock().unwrap() = Some(active.clone());
    let execution = active.clone();
    let mut r = f
        .repository(actor(1, TrustedOrigin::Local))
        .with_execution_fence(Arc::new(move || execution.load(Ordering::Acquire)));
    assert_eq!(
        r.execute_https_settings(original.clone(), &actor(1, TrustedOrigin::Local)),
        Err(HttpsSettingsRepositoryError::Unavailable)
    );
    assert!(!active.load(Ordering::Acquire));
    let committed = f.read();
    assert_eq!(committed.settings.revision.get(), 1);
    assert!(committed.settings.configuration.is_some());
    assert!(f.raw("business/receipt/https-settings/1").is_some());
    let revision = f.revision();
    let count = f.published.load(Ordering::Acquire);
    assert_eq!(f.save(original).unwrap(), committed);
    assert_eq!(f.revision(), revision);
    assert_eq!(f.published.load(Ordering::Acquire), count);
}
#[test]
fn read_rechecks_original_exact_session_at_final_writer_guard_without_mutation() {
    let f = Fixture::new();
    let before = f.revision();
    let mut r = f.repository(actor(1, TrustedOrigin::Local));
    let count = Arc::new(AtomicU64::new(0));
    let checks = count.clone();
    let original = r.session.clone();
    r.session = Arc::new(move |identity, actor| {
        original(identity, actor)?;
        if checks.fetch_add(1, Ordering::AcqRel) == 2 {
            Err(StorageError::ExecutionCancelled)
        } else {
            Ok(())
        }
    });
    assert_eq!(
        r.https_settings(&actor(1, TrustedOrigin::Local)),
        Err(HttpsSettingsRepositoryError::Unavailable)
    );
    assert_eq!(count.load(Ordering::Acquire), 3);
    assert_eq!(f.revision(), before);
    assert!(f.read().settings.configuration.is_none());
}
#[test]
fn committed_command_uses_stable_actor_scope_but_every_new_lease_is_still_authorized() {
    let f = Fixture::new();
    let base = f.read();
    let original = request(1, &base, Some(config("school", 443)));
    let saved = f.save(original.clone()).unwrap();
    let mut renewed = actor(1, TrustedOrigin::Local);
    renewed.expires_at = fc_domain::time::LocalDateTime::new(
        fc_domain::time::LocalDate::new(2026, 11, 3).unwrap(),
        0,
    )
    .unwrap();
    let revision = f.revision();
    assert_eq!(
        f.repository(renewed.clone())
            .execute_https_settings(original.clone(), &renewed)
            .unwrap(),
        saved
    );
    let mut old = f.repository(actor(1, TrustedOrigin::Local));
    f.active.store(false, Ordering::Release);
    assert!(old
        .execute_https_settings(original.clone(), &actor(1, TrustedOrigin::Local))
        .is_err());
    f.active.store(true, Ordering::Release);
    let mut new_identity = identity();
    new_identity.accounts[3].must_change_password = false;
    new_identity.revision = Revision::new(2);
    f.replace_identity(new_identity);
    let other = actor(4, TrustedOrigin::Local);
    assert_eq!(
        f.repository(other.clone())
            .execute_https_settings(original.clone(), &other),
        Err(HttpsSettingsRepositoryError::IdempotencyConflict)
    );
    for changed in [
        ActorContext {
            school_id: SchoolId::new(2).unwrap(),
            ..renewed.clone()
        },
        ActorContext {
            auth_epoch: AuthEpoch::new(2),
            ..renewed.clone()
        },
        actor(1, TrustedOrigin::Web),
    ] {
        assert!(f
            .repository(changed.clone())
            .execute_https_settings(original.clone(), &changed)
            .is_err());
    }
    assert!(f.revision() > revision);
    assert_eq!(f.read().settings, saved.settings);
}
