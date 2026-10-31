//! Real SQLite/writer regressions: public desktop capability is not a session.
use super::*;
use fc_domain::{auth::*, schedule::*, schedule_document::SemesterPlan, time::*};
use fc_ports::{AuthorityPublisher, LessonRepository};
use std::sync::{atomic::AtomicU64, Mutex};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn principal(id: u64) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn now() -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 11, 1).unwrap(), 8 * 3600 + 30 * 60).unwrap()
}
fn admin() -> ActorContext {
    ActorContext {
        principal_id: principal(1),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(LocalDate::new(2026, 11, 5).unwrap(), 0).unwrap(),
        origin: TrustedOrigin::Local,
    }
}
struct Time(Mutex<LocalDateTime>);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        *self.0.lock().unwrap()
    }
}
struct Publisher {
    active: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
    clock: Arc<Time>,
    expire: Arc<AtomicBool>,
    published: Arc<AtomicUsize>,
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
            self.active.store(false, Ordering::Release);
        }
        if self.expire.swap(false, Ordering::AcqRel) {
            *self.clock.0.lock().unwrap() = LocalDateTime::new(now().date, 9 * 3600).unwrap();
        }
        Ok(())
    }
    fn publish(&mut self, _: (), _: Revision) -> Result<(), ()> {
        self.published.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn fault(&mut self) {}
}
struct Fixture {
    writer: Option<StorageWriter<Revision>>,
    root: PathBuf,
    active: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
    expire: Arc<AtomicBool>,
    clock: Arc<Time>,
    published: Arc<AtomicUsize>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-PublicDesktopLesson-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut f = Self {
            writer: None,
            root,
            active: Arc::new(AtomicBool::new(true)),
            revoke: Arc::new(AtomicBool::new(false)),
            expire: Arc::new(AtomicBool::new(false)),
            clock: Arc::new(Time(Mutex::new(now()))),
            published: Arc::new(AtomicUsize::new(0)),
        };
        f.open();
        let mut identity = state();
        let mut second = identity.subjects[0].clone();
        second.id = SubjectId::new(2).unwrap();
        second.name = "数学".into();
        identity.subjects.push(second);
        let draft = ScheduleDocument {
            semesters: vec![SemesterPlan::new(
                Semester {
                    id: SemesterId::new(1).unwrap(),
                    class_id: ClassId::new(1).unwrap(),
                    revision: Revision::default(),
                    enabled: true,
                    start: LocalDate::new(2026, 10, 1).unwrap(),
                    end: LocalDate::new(2026, 11, 30).unwrap(),
                    first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
                    tables: vec![WeekTable {
                        id: TableId::new(1).unwrap(),
                        role: TableRole::Single,
                        rows: vec![TimetableRow {
                            id: RowId::new(1).unwrap(),
                            logical_key: LogicalRowKey::new(1).unwrap(),
                            group: DayGroup::Morning,
                            kind: RowKind::Course,
                            range: MinuteRange::new(
                                Minute::new(480).unwrap(),
                                Minute::new(540).unwrap(),
                            )
                            .unwrap(),
                            enabled_days: [true; 7],
                            courses: std::array::from_fn(|_| {
                                Some(CourseCell {
                                    subject_id: SubjectId::new(1).unwrap(),
                                    teacher: TeacherBinding::Explicit(principal(2)),
                                })
                            }),
                        }],
                    }],
                },
                "Synthetic classroom timetable".into(),
            )],
            ..Default::default()
        };
        let schedule = ScheduleDocument::default()
            .transition(&draft, Revision::default(), &identity, &admin(), now())
            .unwrap();
        let context = LessonContextSnapshot::default()
            .observe(&identity, &schedule, now())
            .unwrap();
        assert!(context.current.is_some());
        let mut mutations = vec![
            Mutation::Put {
                key: IDENTITY.into(),
                value: codec::bytes(&codec::identity(&identity)).unwrap(),
            },
            Mutation::Put {
                key: "business/schedule/v1".into(),
                value: schedule::encode(&schedule_codec::document(&schedule)).unwrap(),
            },
            lesson::mutation(&context).unwrap(),
        ];
        for id in 1..=4 {
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
                self.root.join("state.sqlite3"),
                8,
                Publisher {
                    active: self.active.clone(),
                    revoke: self.revoke.clone(),
                    clock: self.clock.clone(),
                    expire: self.expire.clone(),
                    published: self.published.clone(),
                },
            )
            .unwrap(),
        );
    }
    fn reopen(&mut self) {
        self.writer.take().unwrap().shutdown().unwrap();
        startup_state(self.root.join("state.sqlite3")).unwrap();
        self.open();
    }
    fn handle(&self) -> StorageHandle<Revision> {
        self.writer.as_ref().unwrap().handle()
    }
    fn revision(&self) -> Revision {
        self.handle().try_revision().unwrap().wait().unwrap()
    }
    fn base(&self) -> BusinessRepository<Revision> {
        BusinessRepository::new(
            self.handle(),
            self.clock.clone(),
            Arc::new(|_, revision| Ok(revision)),
        )
    }
    fn repository(&self) -> BusinessRepository<Revision> {
        let active = self.active.clone();
        self.base()
            .with_desktop_lesson_guard(Arc::new(move || active.load(Ordering::Acquire)))
    }
    fn managed(&self) -> BusinessRepository<Revision> {
        let clock = self.clock.clone();
        self.base().with_session_guard(Arc::new(move |i, a| {
            i.authorize_management(a, clock.now())
                .map_err(StorageError::Identity)
        }))
    }
    fn get(&self, key: &str) -> Document {
        self.handle()
            .try_get(key.into())
            .unwrap()
            .wait()
            .unwrap()
            .unwrap()
    }
    fn commit(&self, mutations: Vec<Mutation>) {
        let revision = self.revision();
        self.handle()
            .try_commit(
                CommitCommand::new(format!("fixture{}", revision.get()), revision, mutations)
                    .unwrap(),
                revision.next().unwrap(),
            )
            .unwrap()
            .wait()
            .unwrap();
    }
    fn identity(&self) -> IdentityState {
        codec::read_identity(&codec::value(&self.get(IDENTITY).value).unwrap()).unwrap()
    }
    fn schedule(&self) -> ScheduleDocument {
        schedule::read_document(&self.get("business/schedule/v1").value).unwrap()
    }
    fn context(&self) -> LessonContextSnapshot {
        lesson_codec::read_snapshot(
            &codec::value(&self.get("business/lesson-context/v1").value).unwrap(),
        )
        .unwrap()
    }
    fn binding(&self) -> DesktopLessonBinding {
        DesktopLessonBinding {
            expected_revision: self.context().revision,
            expected_identity_revision: self.identity().revision,
            expected_schedule_revision: self.schedule().revision,
            lesson: self.context().current.unwrap().key,
        }
    }
    fn change(
        &self,
        id: u64,
        binding: DesktopLessonBinding,
        action: LessonAction,
    ) -> Result<DesktopLessonSource, LessonRepositoryError> {
        self.repository()
            .execute_desktop_lesson(DesktopLessonRequest {
                command_id: CommandId::new(id).unwrap(),
                binding,
                action,
            })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            writer
                .shutdown()
                .expect("desktop lesson actual writer join");
        }
        std::fs::remove_dir_all(&self.root).expect("desktop lesson fixture cleanup");
    }
}
fn replacement() -> LessonAction {
    LessonAction::Replace(CourseCell {
        subject_id: SubjectId::new(2).unwrap(),
        teacher: TeacherBinding::Unassigned,
    })
}
fn state() -> IdentityState {
    let school = SchoolId::new(1).unwrap();
    IdentityState {
        revision: Revision::new(1),
        installation: Some(Installation {
            school_id: school,
            school_name: "测试学校".into(),
            current_class: Some(ClassId::new(1).unwrap()),
        }),
        accounts: (1..=4)
            .map(|id| UserAccount {
                principal: Principal {
                    id: principal(id),
                    school_id: school,
                    role: match id {
                        1 => Role::GlobalAdmin,
                        4 => Role::User,
                        _ => Role::TeacherAdmin,
                    },
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("user{id}"),
                display_name: format!("账号{id}"),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            })
            .collect(),
        classes: vec![SchoolClass {
            id: ClassId::new(1).unwrap(),
            school_id: school,
            name: "一班".into(),
            enabled: true,
        }],
        subjects: vec![SchoolSubject {
            id: SubjectId::new(1).unwrap(),
            school_id: school,
            name: "语文".into(),
            enabled: true,
            color: Rgb {
                red: 20,
                green: 40,
                blue: 60,
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
#[test]
fn public_commands_need_no_account_session_and_preserve_only_this_lesson() {
    let f = Fixture::new();
    let before = f.identity();
    let schedule = f.schedule();
    let original = f.binding();
    assert!(f.base().desktop_lesson(original.clone()).is_err());
    let mut repository = f.repository().with_session_guard(Arc::new(|_, _| {
        panic!("public desktop must never ask for a session")
    }));
    let source = repository.desktop_lesson(original.clone()).unwrap();
    assert_eq!(source.catalog_revision, before.revision);
    assert_eq!(source.teachers.len(), 2);
    assert!(source
        .teachers
        .iter()
        .all(|t| t.subject_ids == [SubjectId::new(1).unwrap()]));
    let saved = repository
        .execute_desktop_lesson(DesktopLessonRequest {
            command_id: CommandId::new(1).unwrap(),
            binding: original,
            action: replacement(),
        })
        .unwrap();
    assert_eq!(
        saved.context.active_override.as_ref().unwrap().initiator,
        LessonInitiator::LocalDesktop
    );
    assert_eq!(
        saved.context.current.as_ref().unwrap().key,
        f.context().current.unwrap().key
    );
    assert_eq!(f.identity(), before);
    assert_eq!(f.schedule(), schedule);
    let restored = f.change(2, f.binding(), LessonAction::Restore).unwrap();
    assert!(restored.context.active_override.is_none());
    assert_eq!(
        restored.context.current.as_ref().unwrap().effective,
        restored.context.current.as_ref().unwrap().base
    );
}
#[test]
fn desktop_receipts_are_idempotent_and_never_impersonate_managed_receipts() {
    let f = Fixture::new();
    let binding = f.binding();
    let saved = f.change(1, binding.clone(), replacement()).unwrap();
    let revision = f.revision();
    assert_eq!(f.change(1, binding.clone(), replacement()).unwrap(), saved);
    assert_eq!(f.revision(), revision);
    assert_eq!(
        f.change(1, binding, LessonAction::Restore),
        Err(LessonRepositoryError::IdempotencyConflict)
    );
    assert!(f
        .handle()
        .try_get("business/receipt/lesson/1".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    let record = codec::value(&f.get("business/receipt/desktop-lesson/1").value).unwrap();
    assert_eq!(
        record["result"]["override"]["initiator"],
        json!({"kind":"local-desktop"})
    );
    assert!(record["result"]["override"].get("actor").is_none());
    let admin_request = fc_ports::LessonRequest {
        command_id: CommandId::new(10).unwrap(),
        expected_revision: f.context().revision,
        lesson: f.binding().lesson,
        action: LessonAction::Restore,
    };
    assert!(f
        .repository()
        .execute_lesson(admin_request, &admin(), f.clock.as_ref())
        .is_err());
}
#[test]
fn exact_catalog_schedule_lesson_key_and_class_cas_never_rebinds_a_stale_click() {
    let f = Fixture::new();
    let binding = f.binding();
    let before = f.revision();
    for case in 0..7 {
        let mut bad = binding.clone();
        match case {
            0 => bad.expected_identity_revision = Revision::new(99),
            1 => bad.expected_schedule_revision = Revision::new(99),
            2 => bad.expected_revision = Revision::new(99),
            3 => bad.lesson.class_id = ClassId::new(99).unwrap(),
            4 => bad.lesson.actual_date = LocalDate::new(2026, 11, 2).unwrap(),
            5 => bad.lesson.version.rules_revision = Revision::new(99),
            _ => {
                bad.lesson.range =
                    MinuteRange::new(Minute::new(480).unwrap(), Minute::new(541).unwrap()).unwrap()
            }
        }
        assert!(f.repository().desktop_lesson(bad.clone()).is_err());
        assert!(f.change(1, bad, replacement()).is_err());
    }
    assert_eq!(f.revision(), before);
    assert!(f.context().active_override.is_none());
    let mut identity = f.identity();
    identity.revision = identity.revision.next().unwrap();
    identity.installation.as_mut().unwrap().current_class = None;
    f.commit(vec![Mutation::Put {
        key: IDENTITY.into(),
        value: codec::bytes(&codec::identity(&identity)).unwrap(),
    }]);
    assert!(f.change(1, binding, replacement()).is_err());
}
#[test]
fn scope_targets_and_end_of_lesson_or_rollback_cannot_be_extended() {
    let f = Fixture::new();
    let binding = f.binding();
    let before = f.revision();
    for cell in [
        CourseCell {
            subject_id: SubjectId::new(1).unwrap(),
            teacher: TeacherBinding::Explicit(principal(4)),
        },
        CourseCell {
            subject_id: SubjectId::new(2).unwrap(),
            teacher: TeacherBinding::Default,
        },
        CourseCell {
            subject_id: SubjectId::new(999).unwrap(),
            teacher: TeacherBinding::Unassigned,
        },
    ] {
        assert!(f
            .change(1, binding.clone(), LessonAction::Replace(cell))
            .is_err());
    }
    *f.clock.0.lock().unwrap() = LocalDateTime::new(now().date, 9 * 3600).unwrap();
    assert!(f.change(1, binding.clone(), replacement()).is_err());
    *f.clock.0.lock().unwrap() = LocalDateTime::new(now().date, 8 * 3600 + 29 * 60).unwrap();
    assert!(f.change(1, binding, replacement()).is_err());
    assert_eq!(f.revision(), before);
}
#[test]
fn default_deny_panic_cancel_and_combined_guards_do_not_grant_or_fault() {
    let f = Fixture::new();
    let binding = f.binding();
    let before = f.revision();
    assert_eq!(
        f.base().desktop_lesson(binding.clone()),
        Err(LessonRepositoryError::Unavailable)
    );
    assert_eq!(
        f.base()
            .with_desktop_lesson_guard(Arc::new(|| panic!("synthetic guard")))
            .desktop_lesson(binding.clone()),
        Err(LessonRepositoryError::Unavailable)
    );
    let r = f
        .base()
        .with_desktop_lesson_guard(Arc::new(|| false))
        .with_desktop_lesson_guard(Arc::new(|| true));
    assert_eq!(
        r.desktop_lesson(binding.clone()),
        Err(LessonRepositoryError::Unavailable)
    );
    f.active.store(false, Ordering::Release);
    assert!(f.change(1, binding.clone(), replacement()).is_err());
    assert_eq!(f.revision(), before);
    f.active.store(true, Ordering::Release);
    assert!(f.change(2, binding, replacement()).is_ok());
}
#[test]
fn guard_revocation_and_course_expiry_at_precommit_roll_back_without_publication() {
    for expire in [false, true] {
        let f = Fixture::new();
        let binding = f.binding();
        let before = f.revision();
        let published = f.published.load(Ordering::Acquire);
        if expire {
            f.expire.store(true, Ordering::Release);
        } else {
            f.revoke.store(true, Ordering::Release);
        }
        assert!(f.change(1, binding, replacement()).is_err());
        assert_eq!(f.revision(), before);
        assert!(f.context().active_override.is_none());
        assert_eq!(f.published.load(Ordering::Acquire), published);
        assert!(f
            .handle()
            .try_get("business/receipt/desktop-lesson/1".into())
            .unwrap()
            .wait()
            .unwrap()
            .is_none());
    }
}
#[test]
fn queued_public_task_cancellation_keeps_sql_state_and_receipt_unchanged() {
    struct Held {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl<S> TransactionTask<S> for Held {
        fn allocation_bytes(&self) -> usize {
            std::mem::size_of::<Self>()
        }
        fn evaluate(
            self: Box<Self>,
            _: &dyn TransactionDocuments,
        ) -> Result<TransactionDecision<S>, StorageError> {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(TransactionDecision {
                outcome: vec![],
                commit: None,
                precommit: None,
            })
        }
    }
    let f = Fixture::new();
    let binding = f.binding();
    let before = f.revision();
    let publications = f.published.load(Ordering::Acquire);
    let (entered, arrived) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let held = f
        .handle()
        .try_transaction(Held {
            entered,
            release: wait,
        })
        .unwrap();
    arrived.recv_timeout(Duration::from_secs(5)).unwrap();
    let r = f.repository();
    let request = DesktopLessonRequest {
        command_id: CommandId::new(1).unwrap(),
        binding: binding.clone(),
        action: replacement(),
    };
    let ticket = r
        .handle
        .try_transaction(Task {
            binding,
            request: Some(request),
            guard: r.desktop_lesson_guard.clone(),
            observations: r.lesson_observations.clone(),
            clock: r.clock.clone(),
            factory: r.factory.clone(),
        })
        .unwrap();
    f.active.store(false, Ordering::Release);
    release.send(()).unwrap();
    held.wait().unwrap();
    assert_eq!(ticket.wait(), Err(StorageError::ExecutionCancelled));
    assert_eq!(f.revision(), before);
    assert!(f.context().active_override.is_none());
    assert_eq!(f.published.load(Ordering::Acquire), publications);
    assert!(f
        .handle()
        .try_get("business/receipt/desktop-lesson/1".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
}
#[test]
fn scoped_execution_fence_still_cancels_before_commit_and_base_remains_healthy() {
    let f = Fixture::new();
    let binding = f.binding();
    let before = f.revision();
    let flag = Arc::new(AtomicBool::new(true));
    let observed = flag.clone();
    let mut r = f
        .repository()
        .with_execution_fence(Arc::new(move || observed.load(Ordering::Acquire)));
    r.factory = Arc::new(move |_, revision| {
        flag.store(false, Ordering::Release);
        Ok(revision)
    });
    assert_eq!(
        r.execute_desktop_lesson(DesktopLessonRequest {
            command_id: CommandId::new(1).unwrap(),
            binding: binding.clone(),
            action: replacement()
        }),
        Err(LessonRepositoryError::Unavailable)
    );
    assert_eq!(f.revision(), before);
    assert!(f.context().active_override.is_none());
    assert!(f.change(2, binding, replacement()).is_ok());
}
#[test]
fn legacy_read_is_not_rewritten_until_mutation_and_reopen_has_no_implicit_capability() {
    let mut f = Fixture::new();
    let mut old = lesson_codec::snapshot(&f.context());
    old["format"] = json!(2);
    f.commit(vec![Mutation::Put {
        key: "business/lesson-context/v1".into(),
        value: codec::bytes(&old).unwrap(),
    }]);
    let before = f.revision();
    let stored = f.get("business/lesson-context/v1");
    f.repository().desktop_lesson(f.binding()).unwrap();
    assert_eq!(f.revision(), before);
    assert_eq!(f.get("business/lesson-context/v1"), stored);
    let saved = f.change(1, f.binding(), replacement()).unwrap();
    assert_eq!(
        codec::value(&f.get("business/lesson-context/v1").value).unwrap()["format"],
        3
    );
    f.reopen();
    assert!(f.base().desktop_lesson(f.binding()).is_err());
    assert_eq!(
        f.repository().desktop_lesson(f.binding()).unwrap().context,
        saved.context
    );
}
#[test]
fn persistent_day_floor_and_directory_invalidation_reject_without_partial_writes() {
    let f = Fixture::new();
    let binding = f.binding();
    f.commit(vec![Mutation::Put {
        key: "business/desktop/max-observed-day/v1".into(),
        value: codec::bytes(
            &json!({"format":1,"date":codec::date(LocalDate::new(2026,11,2).unwrap())}),
        )
        .unwrap(),
    }]);
    let before = f.revision();
    assert!(f.change(1, binding, replacement()).is_err());
    assert_eq!(f.revision(), before);
    assert!(f.context().active_override.is_none());
}
#[test]
fn projected_backup_and_restore_never_revive_live_desktop_override_or_capability() {
    let f = Fixture::new();
    let r = f.managed();
    r.ensure_recovery_installation([7; 32]).unwrap();
    *f.clock.0.lock().unwrap() = LocalDateTime::new(now().date, 8 * 3600 + 31 * 60).unwrap();
    let saved = f.change(1, f.binding(), replacement()).unwrap();
    assert!(saved.context.active_override.is_some());
    let backup = f.root.join("ordinary.sqlite3");
    r.try_export_ordinary(backup.clone(), &admin(), Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let ordinary = Connection::open(&backup).unwrap();
    let row = TransactionView(&ordinary)
        .get("business/lesson-context/v1")
        .unwrap()
        .unwrap();
    let projected = lesson_codec::read_snapshot(&codec::value(&row.value).unwrap()).unwrap();
    assert!(projected.current.is_none());
    assert!(projected.active_override.is_none());
    assert!(!projected.history.is_empty());
    assert!(TransactionView(&ordinary)
        .get("business/receipt/desktop-lesson/1")
        .unwrap()
        .is_none());
    drop(ordinary);
    let candidate = f.root.join("preview.sqlite3");
    r.try_preflight_ordinary_restore(backup, candidate.clone(), &admin(), Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let restored = startup_state(&candidate).unwrap();
    let db = open_connection(&candidate).unwrap();
    let context = lesson::load(&TransactionView(&db)).unwrap();
    assert!(context.active_override.is_none());
    assert!(context.base_trust.is_none());
    assert!(restored
        .identity
        .accounts
        .iter()
        .all(|a| a.principal.auth_epoch.get() > 1));
    assert_eq!(f.context().active_override, saved.context.active_override);
}
#[test]
fn public_source_is_bounded_and_contains_no_authentication_fields() {
    let f = Fixture::new();
    let source = f.repository().desktop_lesson(f.binding()).unwrap();
    let encoded = value(&source);
    let bytes = display::encode(&encoded, MAX_VALUE_BYTES).unwrap();
    assert!(bytes.len() < MAX_VALUE_BYTES);
    let text = String::from_utf8(bytes).unwrap();
    for field in ["login", "password", "credential", "auth_epoch", "role"] {
        assert!(!text.contains(field), "unexpected auth field {field}");
    }
    assert_eq!(read(&encoded).unwrap(), source);
    let mut bad = encoded.clone();
    bad["teachers"][0]["login"] = json!("synthetic");
    assert!(read(&bad).is_err());
    let mut bad = encoded.clone();
    bad["subjects"][0]["rgb"] = json!([256, 0, 0]);
    assert!(read(&bad).is_err());
    assert_eq!(
        display::encode(
            &json!({"large":"x".repeat(MAX_VALUE_BYTES)}),
            MAX_VALUE_BYTES
        ),
        Err(StorageError::DocumentTooLarge)
    );
}
#[test]
fn mapped_format_three_audit_remains_inert_and_preserves_complete_history() {
    use fc_domain::recovery::MigrationMapping;
    let source = Fixture::new();
    let s = source.managed();
    s.ensure_recovery_installation([7; 32]).unwrap();
    *source.clock.0.lock().unwrap() = LocalDateTime::new(now().date, 8 * 3600 + 31 * 60).unwrap();
    let saved = source.change(1, source.binding(), replacement()).unwrap();
    assert_eq!(
        saved.context.active_override.as_ref().unwrap().initiator,
        LessonInitiator::LocalDesktop
    );
    assert!(!saved.context.history.is_empty());
    assert!(saved
        .context
        .history
        .iter()
        .all(|h| h.ended_at > h.context.effective_from));
    let backup = source.root.join("ordinary.sqlite3");
    s.try_export_ordinary(backup.clone(), &admin(), Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let read_context = |db: &Connection, key: &str| {
        let row = TransactionView(db).get(key).unwrap().unwrap();
        let value = codec::value(&row.value).unwrap();
        assert_eq!(value["format"], 3);
        lesson_codec::read_snapshot(&value).unwrap()
    };
    let ordinary = Connection::open(&backup).unwrap();
    let projected = read_context(&ordinary, "business/lesson-context/v1");
    assert!(projected.current.is_none());
    assert!(projected.active_override.is_none());
    assert!(projected.base_trust.is_none());
    assert_eq!(projected.history, saved.context.history);
    drop(ordinary);
    let target = Fixture::new();
    *target.clock.0.lock().unwrap() = LocalDateTime::new(now().date, 8 * 3600 + 31 * 60).unwrap();
    let t = target.managed();
    t.ensure_recovery_installation([9; 32]).unwrap();
    let inspected = t
        .try_inspect_migration_source(backup.clone(), &admin(), Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let mapping = MigrationMapping {
        source_school_id: SchoolId::new(1).unwrap(),
        target_school_id: SchoolId::new(1).unwrap(),
        classes: vec![(ClassId::new(1).unwrap(), ClassId::new(1).unwrap())],
        subjects: (1..=2)
            .map(|id| (SubjectId::new(id).unwrap(), SubjectId::new(id).unwrap()))
            .collect(),
        principals: (1..=4).map(|id| (principal(id), principal(id))).collect(),
    };
    let mapped = target.root.join("mapped.sqlite3");
    t.try_map_ordinary(
        backup,
        mapped.clone(),
        inspected,
        mapping,
        &admin(),
        Arc::new(|| true),
    )
    .unwrap()
    .wait()
    .unwrap();
    let mapped_db = Connection::open(&mapped).unwrap();
    let mapped_context = read_context(&mapped_db, "business/lesson-context/v1");
    assert_eq!(mapped_context.history, projected.history);
    assert!(mapped_context.current.is_none());
    assert!(mapped_context.active_override.is_none());
    assert!(mapped_context.base_trust.is_none());
    drop(mapped_db);
    let candidate = target.root.join("candidate.sqlite3");
    t.try_preflight_ordinary_restore(mapped, candidate.clone(), &admin(), Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    startup_state(&candidate).unwrap();
    let db = open_connection(&candidate).unwrap();
    let restored = lesson::load(&TransactionView(&db)).unwrap();
    assert!(restored.current.is_none());
    assert!(restored.active_override.is_none());
    assert!(restored.base_trust.is_none());
    assert!(
        restored.history.is_empty(),
        "live lesson is deliberately reset"
    );
    let audit = read_context(&db, "business/recovery/lesson-audit/v1");
    assert!(audit.current.is_none());
    assert!(audit.active_override.is_none());
    assert!(audit.base_trust.is_none());
    assert!(!audit.history.is_empty());
    assert_eq!(audit.history, mapped_context.history);
    assert_eq!(audit.history, projected.history);
    assert!(TransactionView(&db)
        .get("business/receipt/desktop-lesson/1")
        .unwrap()
        .is_none());
    assert!(target.context().active_override.is_none());
}
