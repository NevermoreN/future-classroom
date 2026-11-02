//! Actual SQLite, sole-writer publication and exact fixture-channel regressions.
use super::*;
use fc_domain::{
    recovery::MigrationMapping,
    time::{LocalDate, LocalDateTime},
};
use fc_ports::command_window::{CommandFamily, CommandOwner};
use fc_ports::{AuthorityPublisher, DisplayRepository, DisplayRequest};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn principal(id: u64) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn date(day: u8) -> LocalDate {
    LocalDate::new(2026, 11, day).unwrap()
}
fn now() -> LocalDateTime {
    LocalDateTime::new(date(1), 3600).unwrap()
}
fn actor(id: u64, origin: TrustedOrigin) -> ActorContext {
    ActorContext {
        principal_id: principal(id),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(date(5), 0).unwrap(),
        origin,
    }
}
struct Time(Mutex<LocalDateTime>);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        *self.0.lock().unwrap()
    }
}
#[derive(Clone)]
struct Snapshot {
    identity: IdentityState,
    revision: Revision,
}
struct Publisher {
    active: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
    clock: Arc<Time>,
    midnight: Arc<AtomicBool>,
    published: Arc<Mutex<Vec<Snapshot>>>,
}
impl AuthorityPublisher for Publisher {
    type Snapshot = Snapshot;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, s: &Snapshot) -> Revision {
        s.revision
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        if self.revoke.swap(false, Ordering::AcqRel) {
            self.active.store(false, Ordering::Release);
        }
        if self.midnight.swap(false, Ordering::AcqRel) {
            *self.clock.0.lock().unwrap() = LocalDateTime::new(date(2), 0).unwrap();
        }
        Ok(())
    }
    fn publish(&mut self, _: (), s: Snapshot) -> Result<(), ()> {
        self.published.lock().unwrap().push(s);
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn fault(&mut self) {}
}
struct Fixture {
    commands: crate::fixture_commands::FixtureCommands,
    writer: Option<StorageWriter<Snapshot>>,
    clock: Arc<Time>,
    active: Arc<AtomicBool>,
    revoke: Arc<AtomicBool>,
    midnight: Arc<AtomicBool>,
    published: Arc<Mutex<Vec<Snapshot>>>,
    root: PathBuf,
}
impl Fixture {
    fn receipt_key(&self, legacy: &str) -> String {
        let (slot, id) =
            crate::business::command_window::legacy(legacy).expect("exact fixture receipt slot");
        let original = self
            .commands
            .original(id)
            .expect("original fixture receipt pair");
        crate::business::command_window::namespaced(slot, original.ticket)
    }

    // Labels name fixture intents only. Request ids come from the real Writer;
    // this cache keeps the exact original pair through retries and reopen.
    fn command_id(&self, family: CommandFamily, label: u64) -> CommandId {
        if let Some(original) = self.commands.labelled(family, &label.to_string()) {
            return original.command_id;
        }
        self.commands
            .issue(
                &self.handle(),
                family,
                &label.to_string(),
                CommandOwner::authenticated(&actor(1, TrustedOrigin::Local)),
            )
            .unwrap()
            .command_id
    }

    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-SubjectVisual-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut f = Self {
            commands: crate::fixture_commands::FixtureCommands::default(),
            writer: None,
            clock: Arc::new(Time(Mutex::new(now()))),
            active: Arc::new(AtomicBool::new(true)),
            revoke: Arc::new(AtomicBool::new(false)),
            midnight: Arc::new(AtomicBool::new(false)),
            published: Arc::new(Mutex::new(vec![])),
            root,
        };
        f.open();
        let state = state();
        let mut mutations = vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&state)).unwrap(),
        }];
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
                    midnight: self.midnight.clone(),
                    published: self.published.clone(),
                },
            )
            .unwrap(),
        );
    }
    fn reopen(&mut self) {
        self.writer.take().unwrap().shutdown().unwrap();
        self.open();
    }
    fn handle(&self) -> StorageHandle<Snapshot> {
        self.writer.as_ref().unwrap().handle()
    }
    fn repository(&self, captured: ActorContext) -> BusinessRepository<Snapshot> {
        let active = self.active.clone();
        let clock = self.clock.clone();
        BusinessRepository::new(
            self.handle(),
            self.clock.clone(),
            Arc::new(|identity, revision| {
                Ok(Snapshot {
                    identity: identity.clone(),
                    revision,
                })
            }),
        )
        .with_session_guard(Arc::new(move |state, current| {
            if current != &captured || !active.load(Ordering::Acquire) {
                return Err(StorageError::SubjectVisual(SubjectVisualError::Identity(
                    IdentityError::Permission(PermissionError::StaleAuthorization),
                )));
            }
            state
                .authority(current.principal_id)
                .map_err(StorageError::Identity)?
                .validate_actor(current, clock.now())
                .map_err(|e| {
                    StorageError::SubjectVisual(SubjectVisualError::Identity(
                        IdentityError::Permission(e),
                    ))
                })
        }))
    }
    fn revision(&self) -> Revision {
        self.handle().try_revision().unwrap().wait().unwrap()
    }
    fn identity(&self) -> IdentityState {
        self.repository(actor(1, TrustedOrigin::Local))
            .load_identity()
            .unwrap()
    }
    fn commit(&self, mutations: Vec<Mutation>) {
        let revision = self.revision();
        let identity = mutations
            .iter()
            .find_map(|m| {
                if let Mutation::Put { key, value } = m {
                    (key == IDENTITY)
                        .then(|| codec::read_identity(&codec::value(value).unwrap()).unwrap())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| self.identity());
        {
            let (fixture_handle, fixture_command) = self
                .commands
                .prepare_generic(
                    &self.handle(),
                    CommitCommand::new(format!("fixture-{}", revision.get()), revision, mutations)
                        .unwrap(),
                )
                .unwrap();
            fixture_handle.try_commit(
                fixture_command,
                Snapshot {
                    identity,
                    revision: revision.next().unwrap(),
                },
            )
        }
        .unwrap()
        .wait()
        .unwrap();
    }
    fn replace_identity(&self, state: &IdentityState) {
        self.commit(vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(state)).unwrap(),
        }]);
    }
    fn read(&self) -> SubjectVisualState {
        self.repository(actor(1, TrustedOrigin::Local))
            .subject_visuals(&actor(1, TrustedOrigin::Local))
            .unwrap()
    }
    fn save(
        &self,
        id: u64,
        base: &SubjectVisualState,
        red: u8,
    ) -> Result<SubjectVisualState, SubjectVisualRepositoryError> {
        let r = self.repository(actor(1, TrustedOrigin::Local));
        {
            let fixture_request = request(self, id, base, red);
            let original_pair = self.commands.original(fixture_request.command_id).unwrap();
            let mut issued_repository = r.with_command_window(original_pair);
            issued_repository.execute_subject_visuals(
                fixture_request,
                &actor(1, TrustedOrigin::Local),
                self.clock.as_ref(),
            )
        }
    }
    fn legacy_subject(&self, id: u64, name: &str, color: Rgb, expected: Option<Revision>) {
        let r = self.repository(actor(1, TrustedOrigin::Local));
        let current = r.load_identity().unwrap();
        {
            let fixture_request = IdentityRequest {
                command_id: self.command_id(CommandFamily::Identity, 1000 + self.revision().get()),
                expected_revision: current.revision,
                command: IdentityCommand::PutSubject {
                    id: SubjectId::new(id).unwrap(),
                    name: name.into(),
                    enabled: true,
                    color,
                    expected_revision: expected,
                },
            };
            let original_pair = self.commands.original(fixture_request.command_id).unwrap();
            let mut issued_repository = r.with_command_window(original_pair);
            issued_repository.execute_identity(
                fixture_request,
                Some(&actor(1, TrustedOrigin::Local)),
                self.clock.as_ref(),
            )
        }
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            writer
                .shutdown()
                .expect("subject visual writer actual join");
        }
        std::fs::remove_dir_all(&self.root).expect("subject visual owned fixture cleanup");
    }
}
fn request(f: &Fixture, id: u64, base: &SubjectVisualState, red: u8) -> SubjectVisualRequest {
    let mut colors: Vec<_> = base
        .palette
        .subjects
        .iter()
        .map(|s| SubjectColor {
            subject_id: s.id,
            color: s.color,
        })
        .collect();
    colors[0].color.red = red;
    let mut display = base.display.clone();
    display.subject_fill = !display.subject_fill;
    SubjectVisualRequest {
        command_id: f.command_id(CommandFamily::SubjectVisual, id),
        expected_palette_revision: base.palette.revision.clone(),
        expected_display_revision: base.display.revision,
        colors,
        display,
    }
}
fn homework(owner: u64, day: u8) -> HomeworkKey {
    HomeworkKey {
        class_id: ClassId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
        owner_teacher_id: principal(owner),
        actual_date: date(day),
    }
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
fn joint_save_is_atomic_published_from_updated_identity_and_durable_idempotent() {
    let mut f = Fixture::new();
    let base = f.read();
    let before = f.identity();
    let saved = f.save(1, &base, 170).unwrap();
    assert_eq!(
        saved.display.revision,
        base.display.revision.next().unwrap()
    );
    assert_eq!(
        saved.palette.subjects[0].revision,
        before.subjects[0].revision.next().unwrap()
    );
    assert_eq!(saved.palette.subjects[0].color.red, 170);
    assert_eq!(
        f.published.lock().unwrap().last().unwrap().identity,
        f.identity()
    );
    let after = f.revision();
    assert_eq!(f.save(1, &base, 170).unwrap(), saved);
    assert_eq!(f.revision(), after);
    assert_eq!(
        f.save(1, &base, 171),
        Err(SubjectVisualRepositoryError::IdempotencyConflict)
    );
    f.reopen();
    assert_eq!(f.read(), saved);
    assert_eq!(f.save(1, &base, 170).unwrap(), saved);
    let no_color_change = f.save(2, &saved, 170).unwrap();
    assert_eq!(no_color_change.palette, saved.palette);
    assert_eq!(
        no_color_change.display.revision,
        saved.display.revision.next().unwrap()
    );
    assert_eq!(f.identity().revision, before.revision.next().unwrap());
}
#[test]
fn monitor_settings_keep_legacy_receipts_and_persist_opaque_selection_after_reopen() {
    let mut f = Fixture::new();
    let admin = actor(1, TrustedOrigin::Local);
    let base = f.read();
    let legacy = display_codec::settings(&base.display);
    assert_eq!(legacy["format"], json!(1));
    assert!(legacy.get("target_monitor").is_none());
    assert_eq!(display_codec::read_settings(&legacy).unwrap(), base.display);
    let display_request = DisplayRequest {
        command_id: f.command_id(CommandFamily::Display, 81),
        expected_revision: base.display.revision,
        candidate: base.display.clone(),
    };
    let save_display = |f: &Fixture, request: DisplayRequest| {
        f.repository(admin.clone())
            .with_command_window(f.commands.original(request.command_id).unwrap())
            .execute_display(request, &admin, f.clock.as_ref())
            .unwrap()
    };
    let legacy_display = save_display(&f, display_request.clone());
    let before_visual = f.read();
    let legacy_visual = f.save(82, &before_visual, 170).unwrap();
    let mut selected_request = request(&f, 83, &legacy_visual, 171);
    selected_request.display.target_monitor = Some(
        r"\\?\DISPLAY#SYNTHETIC#classroom-screen#{00000000-0000-0000-0000-000000000000}".into(),
    );
    let save_visual = |f: &Fixture, request: SubjectVisualRequest| {
        f.repository(admin.clone())
            .with_command_window(f.commands.original(request.command_id).unwrap())
            .execute_subject_visuals(request, &admin, f.clock.as_ref())
            .unwrap()
    };
    let selected = save_visual(&f, selected_request.clone());
    let wire = display_codec::settings(&selected.display);
    assert_eq!(wire["format"], json!(2));
    assert_eq!(
        display_codec::read_settings(&wire).unwrap(),
        selected.display
    );
    let mut mixed = wire.clone();
    mixed["format"] = json!(1);
    assert!(display_codec::read_settings(&mixed).is_err());
    let mut partial = wire.clone();
    partial.as_object_mut().unwrap().remove("target_monitor");
    assert!(display_codec::read_settings(&partial).is_err());
    for target in [
        "".into(),
        " padded".into(),
        "bad\nidentity".into(),
        "x".repeat(513),
    ] {
        let mut bad = wire.clone();
        bad["target_monitor"] = json!(target);
        assert!(display_codec::read_settings(&bad).is_err());
    }
    let mut unknown = wire.clone();
    unknown["target_monitor"] = Value::Null;
    assert!(display_codec::read_settings(&unknown).is_err());
    f.reopen();
    let revision = f.revision();
    assert_eq!(f.read(), selected);
    assert_eq!(save_display(&f, display_request), legacy_display);
    assert_eq!(f.save(82, &before_visual, 170).unwrap(), legacy_visual);
    assert_eq!(save_visual(&f, selected_request), selected);
    assert_eq!(f.revision(), revision);
    assert_eq!(f.read(), selected); // Historical retries must not restore the old primary-screen policy.
    let follow_primary = DisplayRequest {
        command_id: f.command_id(CommandFamily::Display, 84),
        expected_revision: selected.display.revision,
        candidate: DisplaySettings {
            target_monitor: None,
            ..selected.display
        },
    };
    let following = save_display(&f, follow_primary);
    assert_eq!(display_codec::settings(&following)["format"], json!(1));
    f.reopen();
    assert_eq!(f.read().display, following);
}
#[test]
fn each_cas_conflict_leaves_the_other_document_and_receipt_untouched() {
    let f = Fixture::new();
    let base = f.read();
    let color = base.palette.subjects[0].color;
    f.legacy_subject(1, "改名", color, Some(base.palette.subjects[0].revision));
    let current = f.read();
    let revision = f.revision();
    assert!(matches!(
        f.save(1, &base, 180),
        Err(SubjectVisualRepositoryError::Domain(
            SubjectVisualError::RevisionConflict { .. }
        ))
    ));
    assert_eq!(f.read(), current);
    assert_eq!(f.revision(), revision);
    let r = f.repository(actor(1, TrustedOrigin::Local));
    {
        let fixture_request = DisplayRequest {
            command_id: f.command_id(CommandFamily::Display, 80),
            expected_revision: current.display.revision,
            candidate: current.display.clone(),
        };
        let original_pair = f.commands.original(fixture_request.command_id).unwrap();
        let mut issued_repository = r.with_command_window(original_pair);
        issued_repository.execute_display(
            fixture_request,
            &actor(1, TrustedOrigin::Local),
            f.clock.as_ref(),
        )
    }
    .unwrap();
    let before = f.read();
    let revision = f.revision();
    assert!(matches!(
        f.save(2, &current, 180),
        Err(SubjectVisualRepositoryError::Display(
            fc_domain::desktop::DisplayError::RevisionConflict { .. }
        ))
    ));
    assert_eq!(f.read(), before);
    assert_eq!(f.revision(), revision);
    assert!(f
        .handle()
        .try_get(f.receipt_key(&format!(
            "business/receipt/subject-visual/{}",
            f.command_id(CommandFamily::SubjectVisual, 2).get()
        )))
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
}
#[test]
fn legacy_color_aba_and_new_subject_conflict_but_unrelated_catalog_does_not() {
    let f = Fixture::new();
    let base = f.read();
    let color = base.palette.subjects[0].color;
    let mut changed = color;
    changed.red += 1;
    f.legacy_subject(1, "语文", changed, Some(base.palette.subjects[0].revision));
    f.legacy_subject(
        1,
        "语文",
        color,
        Some(base.palette.subjects[0].revision.next().unwrap()),
    );
    assert!(matches!(
        f.save(1, &base, 180),
        Err(SubjectVisualRepositoryError::Domain(
            SubjectVisualError::RevisionConflict { .. }
        ))
    ));
    let base = f.read();
    f.legacy_subject(2, "数学", color, None);
    assert!(matches!(
        f.save(2, &base, 180),
        Err(SubjectVisualRepositoryError::Domain(
            SubjectVisualError::RevisionConflict { .. }
        ))
    ));
    let base = f.read();
    let r = f.repository(actor(1, TrustedOrigin::Local));
    let current = r.load_identity().unwrap();
    {
        let fixture_request = IdentityRequest {
            command_id: f.command_id(CommandFamily::Identity, 90),
            expected_revision: current.revision,
            command: IdentityCommand::RenameSchool {
                name: "新学校名".into(),
            },
        };
        let original_pair = f.commands.original(fixture_request.command_id).unwrap();
        let mut issued_repository = r.with_command_window(original_pair);
        issued_repository.execute_identity(
            fixture_request,
            Some(&actor(1, TrustedOrigin::Local)),
            f.clock.as_ref(),
        )
    }
    .unwrap();
    f.commit(vec![Mutation::Put {
        key: "fixture/material-noise".into(),
        value: vec![1],
    }]);
    let saved = f.save(3, &base, 180).unwrap();
    assert_eq!(saved.palette.subjects.len(), 2);
    assert_eq!(f.identity().installation.unwrap().school_name, "新学校名");
}
#[test]
fn exact_local_global_session_invalid_palette_and_settings_fail_without_writes() {
    let f = Fixture::new();
    let base = f.read();
    let revision = f.revision();
    for a in [
        actor(1, TrustedOrigin::Web),
        actor(2, TrustedOrigin::Local),
        actor(2, TrustedOrigin::Web),
    ] {
        let r = f.repository(a.clone());
        assert!(r.subject_visuals(&a).is_err());
        assert!({
            let fixture_request = request(&f, 1, &base, 180);
            let original_pair = f.commands.original(fixture_request.command_id).unwrap();
            let mut issued_repository = r.with_command_window(original_pair);
            issued_repository.execute_subject_visuals(fixture_request, &a, f.clock.as_ref())
        }
        .is_err());
    }
    let captured = actor(1, TrustedOrigin::Local);
    let mut other = captured.clone();
    other.expires_at = LocalDateTime::new(date(4), 0).unwrap();
    assert!(f.repository(captured).subject_visuals(&other).is_err());
    let r = f.repository(actor(1, TrustedOrigin::Local));
    for mut bad in [
        request(&f, 2, &base, 180),
        request(&f, 3, &base, 180),
        request(&f, 4, &base, 180),
    ] {
        match bad.command_id.get() {
            2 => bad.colors.clear(),
            3 => bad.colors[0].subject_id = SubjectId::new(999).unwrap(),
            _ => bad.display.course_fill_percent = 101,
        }
        assert!({
            let fixture_request = bad;
            let original_pair = f.commands.original(fixture_request.command_id).unwrap();
            let mut issued_repository = r.with_command_window(original_pair);
            issued_repository.execute_subject_visuals(
                fixture_request,
                &actor(1, TrustedOrigin::Local),
                f.clock.as_ref(),
            )
        }
        .is_err());
    }
    assert_eq!(f.revision(), revision);
    assert_eq!(f.read(), base);
    let mut identity = f.identity();
    identity.accounts[0].must_change_password = true;
    f.replace_identity(&identity);
    let revision = f.revision();
    assert!(f.save(5, &base, 180).is_err());
    assert_eq!(f.revision(), revision);
}
#[test]
fn precommit_revocation_rolls_back_both_and_does_not_publish_a_snapshot() {
    let f = Fixture::new();
    let base = f.read();
    let revision = f.revision();
    let publications = f.published.lock().unwrap().len();
    f.revoke.store(true, Ordering::Release);
    assert!(f.save(1, &base, 180).is_err());
    f.active.store(true, Ordering::Release);
    assert_eq!(f.revision(), revision);
    assert_eq!(f.read(), base);
    assert_eq!(f.published.lock().unwrap().len(), publications);
    assert!(f
        .handle()
        .try_get(f.receipt_key(&format!(
            "business/result/subject-visual/{}",
            f.command_id(CommandFamily::SubjectVisual, 1).get()
        )))
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
}
#[test]
fn web_preview_is_same_version_single_subject_and_enforces_scope_day_and_session() {
    let f = Fixture::new();
    let base = f.read();
    let saved = f.save(1, &base, 180).unwrap();
    let teacher = actor(2, TrustedOrigin::Web);
    let r = f.repository(teacher.clone());
    let preview = r.subject_visual_preview(homework(2, 1), &teacher).unwrap();
    assert_eq!(preview.subject, saved.palette.subjects[0]);
    assert_eq!(preview.display, saved.display);
    assert!(r.subject_visuals(&teacher).is_err());
    assert!(matches!(
        r.subject_visual_preview(homework(3, 1), &teacher),
        Err(SubjectVisualRepositoryError::Content(
            ContentError::Permission(PermissionError::WrongOwner)
        ))
    ));
    assert!(matches!(
        r.subject_visual_preview(homework(2, 2), &teacher),
        Err(SubjectVisualRepositoryError::Content(
            ContentError::WrongDate
        ))
    ));
    assert!(f
        .repository(actor(2, TrustedOrigin::Local))
        .subject_visual_preview(homework(2, 1), &actor(2, TrustedOrigin::Local))
        .is_err());
    f.commit(vec![Mutation::Put {
        key: "business/desktop/max-observed-day/v1".into(),
        value: codec::bytes(&json!({"format":1,"date":codec::date(date(2))})).unwrap(),
    }]);
    assert!(matches!(
        r.subject_visual_preview(homework(2, 1), &teacher),
        Err(SubjectVisualRepositoryError::Content(
            ContentError::WrongDate
        ))
    ));
    f.active.store(false, Ordering::Release);
    assert!(r.subject_visual_preview(homework(2, 1), &teacher).is_err());
}
#[test]
fn equivalent_color_order_replays_the_same_durable_command() {
    let f = Fixture::new();
    let initial = f.read();
    f.legacy_subject(2, "数学", initial.palette.subjects[0].color, None);
    let base = f.read();
    let original = request(&f, 1, &base, 180);
    let mut reversed = original.clone();
    reversed.colors.reverse();
    let r = f.repository(actor(1, TrustedOrigin::Local));
    let saved = {
        let fixture_request = original;
        let original_pair = f.commands.original(fixture_request.command_id).unwrap();
        let mut issued_repository = r.with_command_window(original_pair);
        issued_repository.execute_subject_visuals(
            fixture_request,
            &actor(1, TrustedOrigin::Local),
            f.clock.as_ref(),
        )
    }
    .unwrap();
    let revision = f.revision();
    assert_eq!(
        {
            let fixture_request = reversed;
            let original_pair = f.commands.original(fixture_request.command_id).unwrap();
            let mut issued_repository = r.with_command_window(original_pair);
            issued_repository.execute_subject_visuals(
                fixture_request,
                &actor(1, TrustedOrigin::Local),
                f.clock.as_ref(),
            )
        }
        .unwrap(),
        saved
    );
    assert_eq!(f.revision(), revision);
}
#[test]
fn queued_request_execution_cancellation_never_writes_or_publishes() {
    struct Held {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
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
    let f = Fixture::new();
    let base = f.read();
    let before = f.revision();
    let publications = f.published.lock().unwrap().len();
    let (entered, arrived) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let queued_id = f.command_id(CommandFamily::SubjectVisual, 1);
    let queued_pair = f.commands.original(queued_id).unwrap();
    let held = f
        .handle()
        .try_transaction(Held {
            entered,
            release: wait,
        })
        .unwrap();
    arrived
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let active = Arc::new(AtomicBool::new(true));
    let observed = active.clone();
    let r = f
        .repository(actor(1, TrustedOrigin::Local))
        .with_execution_fence(Arc::new(move || observed.load(Ordering::Acquire)))
        .with_command_window(queued_pair);
    let task = SaveVisuals {
        request: request(&f, 1, &base, 180),
        actor: actor(1, TrustedOrigin::Local),
        clock: r.clock.clone(),
        session: r.session.clone(),
        factory: r.factory.clone(),
    };
    let queued = r.handle.try_transaction(task).unwrap();
    active.store(false, Ordering::Release);
    release.send(()).unwrap();
    held.wait().unwrap();
    assert_eq!(queued.wait(), Err(StorageError::ExecutionCancelled));
    assert_eq!(f.revision(), before);
    assert_eq!(f.read(), base);
    assert_eq!(f.published.lock().unwrap().len(), publications);
}
#[test]
fn precommit_execution_cancellation_rolls_back_and_keeps_writer_healthy() {
    let f = Fixture::new();
    let base = f.read();
    let before = f.revision();
    let active = Arc::new(AtomicBool::new(true));
    let observed = active.clone();
    let mut r = f
        .repository(actor(1, TrustedOrigin::Local))
        .with_execution_fence(Arc::new(move || observed.load(Ordering::Acquire)));
    let active_after_factory = active.clone();
    r.factory = Arc::new(move |identity, revision| {
        active_after_factory.store(false, Ordering::Release);
        Ok(Snapshot {
            identity: identity.clone(),
            revision,
        })
    });
    assert_eq!(
        {
            let fixture_request = request(&f, 1, &base, 180);
            let original_pair = f.commands.original(fixture_request.command_id).unwrap();
            let mut issued_repository = r.with_command_window(original_pair);
            issued_repository.execute_subject_visuals(
                fixture_request,
                &actor(1, TrustedOrigin::Local),
                f.clock.as_ref(),
            )
        },
        Err(SubjectVisualRepositoryError::Unavailable)
    );
    assert_eq!(f.revision(), before);
    assert_eq!(f.read(), base);
    assert!(f.save(2, &base, 180).is_ok());
}
#[test]
fn bounded_result_and_strict_palette_codec_refuse_malformed_data() {
    let f = Fixture::new();
    let base = f.read();
    let valid = state_value(&base.palette, &base.display);
    let mut bad = valid.clone();
    bad["palette"]["subjects"][0]["color"] = json!([256, 0, 0]);
    assert!(read_state(&bad).is_err());
    let mut bad = valid.clone();
    bad["palette"]["revision"] = json!([[1, 0]]);
    assert!(read_state(&bad).is_err());
    let mut bad = valid.clone();
    bad["palette"]["revision"] = json!([[1, 1], [1, 1]]);
    assert!(read_state(&bad).is_err());
    let mut bad = valid.clone();
    bad["palette"]["unknown"] = json!(true);
    assert!(read_state(&bad).is_err());
    let revision = f.revision();
    let mut request = request(&f, 1, &base, 180);
    request.display.countdown.label = "x".repeat(MAX_VALUE_BYTES + 1);
    assert!({
        let fixture_request = request;
        let original_pair = f.commands.original(fixture_request.command_id).unwrap();
        let mut issued_repository = f
            .repository(actor(1, TrustedOrigin::Local))
            .with_command_window(original_pair);
        issued_repository.execute_subject_visuals(
            fixture_request,
            &actor(1, TrustedOrigin::Local),
            f.clock.as_ref(),
        )
    }
    .is_err());
    assert_eq!(f.revision(), revision);
    assert_eq!(f.read(), base);
    assert_eq!(
        display::encode(
            &json!({"large":"x".repeat(MAX_VALUE_BYTES)}),
            MAX_VALUE_BYTES
        ),
        Err(StorageError::DocumentTooLarge)
    );
}
#[test]
fn ordinary_restore_rebases_current_subject_versions_and_restores_original_colors_and_display() {
    let f = Fixture::new();
    let admin = actor(1, TrustedOrigin::Local);
    let r = f.repository(admin.clone());
    r.ensure_recovery_installation([7; 32]).unwrap();
    let base = f.read();
    let old = f
        .save(1, &base, base.palette.subjects[0].color.red)
        .unwrap();
    let backup = f.root.join("ordinary.sqlite3");
    r.try_export_ordinary(backup.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let changed = f.save(2, &old, 180).unwrap();
    let current_identity = f.identity();
    let preview = f.root.join("preview.sqlite3");
    let receipt = r
        .try_preflight_ordinary_restore(backup.clone(), preview.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(f.read(), changed);
    let barrier = {
        let original_pair = f
            .commands
            .issue(
                &f.handle(),
                CommandFamily::Generic,
                "restore-barrier",
                CommandOwner::authenticated(&admin),
            )
            .unwrap();
        let issued_repository = r.with_command_window(original_pair);
        issued_repository.try_advance_recovery_barrier(
            receipt.current_revision,
            &admin,
            Arc::new(|| true),
        )
    }
    .unwrap()
    .wait()
    .unwrap();
    let candidate = f.root.join("candidate.sqlite3");
    r.try_prepare_ordinary_restore(backup, candidate.clone(), barrier, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    for path in [preview, candidate] {
        let restored = startup_state(&path).unwrap();
        let palette = SubjectVisualPalette::from_identity(&restored.identity).unwrap();
        assert_eq!(palette.subjects[0].color, old.palette.subjects[0].color);
        assert_eq!(
            palette.subjects[0].revision,
            current_identity.subjects[0].revision.next().unwrap()
        );
        assert_ne!(palette.revision, old.palette.revision);
        assert_ne!(palette.revision, changed.palette.revision);
        let db = open_connection(&path).unwrap();
        let display = display::load(&TransactionView(&db), &restored.identity).unwrap();
        assert_eq!(display.subject_fill, old.display.subject_fill);
        assert!(display.revision > changed.display.revision);
        assert!(TransactionView(&db)
            .get(&f.receipt_key(&format!(
                "business/result/subject-visual/{}",
                f.command_id(CommandFamily::SubjectVisual, 1).get()
            )))
            .unwrap()
            .is_none());
    }
}
#[test]
fn mapped_restore_keeps_target_colors_and_rebases_its_complete_palette_vector() {
    let source = Fixture::new();
    let admin = actor(1, TrustedOrigin::Local);
    let s = source.repository(admin.clone());
    s.ensure_recovery_installation([7; 32]).unwrap();
    let source_old = source.read();
    source.save(1, &source_old, 80).unwrap();
    let backup = source.root.join("ordinary.sqlite3");
    s.try_export_ordinary(backup.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let target = Fixture::new();
    let t = target.repository(admin.clone());
    t.ensure_recovery_installation([9; 32]).unwrap();
    let base = target.read();
    let expected = target.save(1, &base, 200).unwrap();
    let inspected = t
        .try_inspect_migration_source(backup.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let mapping = MigrationMapping {
        source_school_id: SchoolId::new(1).unwrap(),
        target_school_id: SchoolId::new(1).unwrap(),
        classes: vec![(ClassId::new(1).unwrap(), ClassId::new(1).unwrap())],
        subjects: vec![(SubjectId::new(1).unwrap(), SubjectId::new(1).unwrap())],
        principals: (1..=4).map(|id| (principal(id), principal(id))).collect(),
    };
    let mapped = target.root.join("mapped.sqlite3");
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
    let candidate = target.root.join("mapped-preview.sqlite3");
    t.try_preflight_ordinary_restore(mapped, candidate.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let restored = startup_state(&candidate).unwrap();
    let palette = SubjectVisualPalette::from_identity(&restored.identity).unwrap();
    assert_eq!(
        palette.subjects[0].color,
        expected.palette.subjects[0].color
    );
    assert_eq!(
        palette.subjects[0].revision,
        expected.palette.subjects[0].revision.next().unwrap()
    );
    assert_ne!(palette.revision, expected.palette.revision);
    assert_eq!(target.read(), expected);
}
#[test]
fn web_preview_rechecks_exact_session_and_midnight_before_releasing_output() {
    let f = Fixture::new();
    let teacher = actor(2, TrustedOrigin::Web);
    let mut r = f.repository(teacher.clone());
    let original = r.session.clone();
    let calls = Arc::new(AtomicU64::new(0));
    let counted = calls.clone();
    r.session = Arc::new(move |identity, actor| {
        original(identity, actor)?;
        if counted.fetch_add(1, Ordering::AcqRel) == 2 {
            return Err(StorageError::Content(ContentError::Permission(
                PermissionError::StaleAuthorization,
            )));
        }
        Ok(())
    });
    assert!(matches!(
        r.subject_visual_preview(homework(2, 1), &teacher),
        Err(SubjectVisualRepositoryError::Content(
            ContentError::Permission(PermissionError::StaleAuthorization)
        ))
    ));
    assert_eq!(calls.load(Ordering::Acquire), 3);
    let mut r = f.repository(teacher.clone());
    let original = r.session.clone();
    let clock = f.clock.clone();
    let calls = Arc::new(AtomicU64::new(0));
    r.session = Arc::new(move |identity, actor| {
        original(identity, actor)?;
        if calls.fetch_add(1, Ordering::AcqRel) == 1 {
            *clock.0.lock().unwrap() = LocalDateTime::new(date(2), 0).unwrap();
        }
        Ok(())
    });
    assert_eq!(
        r.subject_visual_preview(homework(2, 1), &teacher),
        Err(SubjectVisualRepositoryError::Content(
            ContentError::WrongDate
        ))
    );
}
#[test]
fn visual_only_new_day_is_durable_against_wall_rollback_and_physical_reopen() {
    let mut f = Fixture::new();
    let teacher = actor(2, TrustedOrigin::Web);
    let initial = f.read();
    let first = f
        .repository(teacher.clone())
        .subject_visual_preview(homework(2, 1), &teacher)
        .unwrap();
    let first_revision = f.revision();
    assert_eq!(first.subject, initial.palette.subjects[0]);
    assert_eq!(first.display, initial.display);
    f.repository(teacher.clone())
        .subject_visual_preview(homework(2, 1), &teacher)
        .unwrap();
    assert_eq!(
        f.revision(),
        first_revision,
        "same-day preview must not repeatedly publish a floor"
    );
    *f.clock.0.lock().unwrap() = LocalDateTime::new(date(2), 3600).unwrap();
    let second = f
        .repository(teacher.clone())
        .subject_visual_preview(homework(2, 2), &teacher)
        .unwrap();
    assert_eq!(second, first);
    let later_revision = f.revision();
    assert_eq!(later_revision, first_revision.next().unwrap());
    *f.clock.0.lock().unwrap() = LocalDateTime::new(date(1), 7200).unwrap();
    assert_eq!(
        f.repository(teacher.clone())
            .subject_visual_preview(homework(2, 1), &teacher),
        Err(SubjectVisualRepositoryError::Content(
            ContentError::WrongDate
        ))
    );
    assert_eq!(f.revision(), later_revision);
    f.reopen(); // actual writer exit, SQLite close and a fresh writer on the same deployment
    assert_eq!(
        f.repository(teacher.clone())
            .subject_visual_preview(homework(2, 1), &teacher),
        Err(SubjectVisualRepositoryError::Content(
            ContentError::WrongDate
        ))
    );
    assert_eq!(f.revision(), later_revision);
    let floor = f
        .handle()
        .try_get("business/desktop/max-observed-day/v1".into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    assert_eq!(
        codec::read_date(&codec::value(&floor.value).unwrap()["date"]).unwrap(),
        date(2)
    );
    assert!(
        f.handle()
            .try_get(content_key(&ContentKey::Homework(homework(2, 2))))
            .unwrap()
            .wait()
            .unwrap()
            .is_none(),
        "no content read/save was needed to advance this floor"
    );
    *f.clock.0.lock().unwrap() = LocalDateTime::new(date(2), 7200).unwrap();
    assert_eq!(
        f.repository(teacher.clone())
            .subject_visual_preview(homework(2, 2), &teacher)
            .unwrap(),
        second
    );
    assert_eq!(f.revision(), later_revision);
    assert_eq!(
        f.read(),
        initial,
        "date observations never modify the palette or display document"
    );
}
