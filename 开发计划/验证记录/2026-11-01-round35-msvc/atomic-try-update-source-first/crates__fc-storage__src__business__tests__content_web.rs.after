//! Real SQLite/sole-writer tests; credential bytes are synthetic protected records.
//! The session callback captures an exact fixture channel, never a production bypass.
use super::*;
use fc_domain::time::{LocalDate, LocalDateTime};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize},
    Mutex,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "FutureClassroom-ContentWeb-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("content web fixture cleanup");
    }
}
fn date(day: u8) -> LocalDate {
    LocalDate::new(2026, 10, day).unwrap()
}
fn time(day: u8) -> LocalDateTime {
    LocalDateTime::new(date(day), 8 * 3600).unwrap()
}
struct Time(Mutex<LocalDateTime>);
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        *self.0.lock().unwrap()
    }
}
fn principal(id: u64) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn key(day: u8, owner: u64) -> ContentKey {
    ContentKey::Homework(HomeworkKey {
        class_id: ClassId::new(1).unwrap(),
        actual_date: date(day),
        subject_id: SubjectId::new(1).unwrap(),
        owner_teacher_id: principal(owner),
    })
}
fn command(id: u64, day: u8, owner: u64, expected: u64, text: &str) -> ContentCommand {
    ContentCommand {
        command_id: CommandId::new(id).unwrap(),
        key: key(day, owner),
        expected_revision: Revision::new(expected),
        text: text.into(),
    }
}
fn actor(id: u64, origin: TrustedOrigin) -> ActorContext {
    ActorContext {
        principal_id: principal(id),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(LocalDate::new(2026, 11, 5).unwrap(), 0).unwrap(),
        origin,
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
struct Publisher {
    clock: Arc<Time>,
    web: Arc<AtomicBool>,
    midnight: Arc<AtomicBool>,
    revoke_begin: Arc<AtomicBool>,
    revoke_publish: Arc<AtomicBool>,
}
impl fc_ports::AuthorityPublisher for Publisher {
    type Snapshot = Revision;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, revision: &Revision) -> Revision {
        *revision
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        if self.revoke_begin.swap(false, Ordering::AcqRel) {
            self.web.store(false, Ordering::Release);
        }
        Ok(())
    }
    fn publish(&mut self, _: (), _: Revision) -> Result<(), ()> {
        if self.midnight.swap(false, Ordering::AcqRel) {
            *self.clock.0.lock().unwrap() = time(31);
        }
        if self.revoke_publish.swap(false, Ordering::AcqRel) {
            self.web.store(false, Ordering::Release);
        }
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn fault(&mut self) {}
}
struct Fixture {
    writer: Option<StorageWriter<Revision>>,
    clock: Arc<Time>,
    web: Arc<AtomicBool>,
    local: Arc<AtomicBool>,
    midnight: Arc<AtomicBool>,
    revoke_begin: Arc<AtomicBool>,
    revoke_publish: Arc<AtomicBool>,
    budget: Arc<AtomicUsize>,
    directory: Directory,
}
impl Fixture {
    fn new() -> Self {
        let mut f = Self {
            writer: None,
            clock: Arc::new(Time(Mutex::new(time(30)))),
            web: Arc::new(AtomicBool::new(true)),
            local: Arc::new(AtomicBool::new(true)),
            midnight: Arc::new(AtomicBool::new(false)),
            revoke_begin: Arc::new(AtomicBool::new(false)),
            revoke_publish: Arc::new(AtomicBool::new(false)),
            budget: Arc::new(AtomicUsize::new(usize::MAX)),
            directory: Directory::new(),
        };
        f.open();
        let identity = state();
        identity.validate().unwrap();
        let mut mutations = vec![Mutation::Put {
            key: IDENTITY.into(),
            value: codec::bytes(&codec::identity(&identity)).unwrap(),
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
                self.directory.path(),
                8,
                Publisher {
                    clock: self.clock.clone(),
                    web: self.web.clone(),
                    midnight: self.midnight.clone(),
                    revoke_begin: self.revoke_begin.clone(),
                    revoke_publish: self.revoke_publish.clone(),
                },
            )
            .unwrap(),
        );
    }
    fn reopen(&mut self) {
        self.writer.take().unwrap().shutdown().unwrap();
        self.open();
    }
    fn handle(&self) -> StorageHandle<Revision> {
        self.writer.as_ref().unwrap().handle()
    }
    fn revision(&self) -> Revision {
        self.handle().try_revision().unwrap().wait().unwrap()
    }
    fn commit(&self, mutations: Vec<Mutation>) {
        let revision = self.revision();
        self.handle()
            .try_commit(
                CommitCommand::new(format!("fixture-{}", revision.get()), revision, mutations)
                    .unwrap(),
                revision.next().unwrap(),
            )
            .unwrap()
            .wait()
            .unwrap();
    }
    fn replace(&self, key: &str, value: Vec<u8>) {
        self.commit(vec![Mutation::Put {
            key: key.into(),
            value,
        }]);
    }
    fn repository(&self, captured: ActorContext) -> BusinessRepository<Revision> {
        let active = if captured.origin == TrustedOrigin::Web {
            self.web.clone()
        } else {
            self.local.clone()
        };
        let budget = self.budget.clone();
        let clock = self.clock.clone();
        BusinessRepository::new(
            self.handle(),
            self.clock.clone(),
            Arc::new(|_, revision| Ok(revision)),
        )
        .with_session_guard(Arc::new(move |identity, current| {
            let remaining = budget
                .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                    if n == usize::MAX {
                        None
                    } else {
                        Some(n.saturating_sub(1))
                    }
                })
                .unwrap_or(usize::MAX);
            if current != &captured || !active.load(Ordering::Acquire) || remaining == 0 {
                return Err(StorageError::Content(ContentError::Permission(
                    PermissionError::StaleAuthorization,
                )));
            }
            identity
                .authority(current.principal_id)
                .map_err(StorageError::Identity)?
                .validate_actor(current, clock.now())
                .map_err(|e| StorageError::Content(ContentError::Permission(e)))
        }))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            writer.shutdown().expect("content web writer shutdown");
        }
    }
}
fn denied(error: RepositoryError) -> bool {
    matches!(error, RepositoryError::Domain(ContentError::Permission(_)))
}
fn wrong_day() -> RepositoryError {
    RepositoryError::Domain(ContentError::WrongDate)
}

#[test]
fn web_cleared_slot_retains_cas_and_receipts_are_historical_sender_only_without_recommit() {
    let f = Fixture::new();
    let teacher = actor(2, TrustedOrigin::Web);
    let mut repo = f.repository(teacher.clone());
    assert_eq!(repo.content_for_web(key(30, 2), &teacher).unwrap(), None);
    let empty = repo
        .execute_content_for_web(command(100, 30, 2, 0, "  "), &teacher)
        .unwrap();
    assert_eq!(empty.revision, Revision::new(1));
    assert!(empty.text.is_empty());
    assert_eq!(
        repo.content_for_web(key(30, 2), &teacher).unwrap(),
        Some(empty.clone())
    );
    let local = actor(2, TrustedOrigin::Local);
    assert_eq!(
        f.repository(local.clone())
            .content(key(30, 2), &local)
            .unwrap(),
        Some(empty.clone())
    );
    let latest = repo
        .execute_content_for_web(command(101, 30, 2, 1, "new visible text"), &teacher)
        .unwrap();
    assert_eq!(latest.revision, Revision::new(2));
    let committed = f.revision();
    assert_eq!(
        repo.content_outcome(CommandId::new(100).unwrap(), &teacher)
            .unwrap(),
        Some(empty.clone())
    );
    assert_eq!(
        repo.execute_content_for_web(command(100, 30, 2, 0, "  "), &teacher)
            .unwrap(),
        empty
    );
    assert_eq!(f.revision(), committed);
    assert_eq!(
        repo.content_for_web(key(30, 2), &teacher).unwrap(),
        Some(latest)
    );
    assert_eq!(
        repo.execute_content_for_web(command(100, 30, 2, 0, "changed payload"), &teacher),
        Err(RepositoryError::Domain(ContentError::IdempotencyConflict))
    );
    assert_eq!(f.revision(), committed);
    for id in [1, 3] {
        let foreign = actor(id, TrustedOrigin::Web);
        let other = f.repository(foreign.clone());
        for command in [100, 9999] {
            assert_eq!(
                other
                    .content_outcome(CommandId::new(command).unwrap(), &foreign)
                    .unwrap(),
                None
            );
        }
    }
}

#[test]
fn web_receipts_recheck_exact_channel_after_read_and_local_lock_does_not_revoke_web() {
    let f = Fixture::new();
    let web = actor(2, TrustedOrigin::Web);
    let mut repo = f.repository(web.clone());
    repo.execute_content_for_web(command(110, 30, 2, 0, "mine"), &web)
        .unwrap();
    f.local.store(false, Ordering::Release);
    let local = actor(2, TrustedOrigin::Local);
    assert!(denied(
        f.repository(local.clone())
            .content_outcome(CommandId::new(110).unwrap(), &local)
            .unwrap_err()
    ));
    assert!(repo
        .content_outcome(CommandId::new(110).unwrap(), &web)
        .unwrap()
        .is_some());
    f.budget.store(3, Ordering::Release);
    assert!(
        denied(
            repo.content_outcome(CommandId::new(110).unwrap(), &web)
                .unwrap_err()
        ),
        "the second exact-lease check must gate loaded bytes"
    );
    f.budget.store(usize::MAX, Ordering::Release);
    f.web.store(false, Ordering::Release);
    for id in [110, 9999] {
        assert!(denied(
            repo.content_outcome(CommandId::new(id).unwrap(), &web)
                .unwrap_err()
        ));
    }
}

#[test]
fn web_global_target_requires_fresh_assignment_but_native_global_semantics_stay_compatible() {
    let f = Fixture::new();
    let global = actor(1, TrustedOrigin::Web);
    let mut repo = f.repository(global.clone());
    repo.execute_content_for_web(command(120, 30, 2, 0, "global authored"), &global)
        .unwrap();
    let mut changed = state();
    changed
        .accounts
        .iter_mut()
        .find(|a| a.principal.id == principal(2))
        .unwrap()
        .principal
        .enabled = false;
    changed.validate().unwrap();
    changed.revision = Revision::new(2);
    f.replace(IDENTITY, codec::bytes(&codec::identity(&changed)).unwrap());
    assert!(denied(
        repo.content_for_web(key(30, 2), &global).unwrap_err()
    ));
    assert!(denied(
        repo.content_outcome(CommandId::new(120).unwrap(), &global)
            .unwrap_err()
    ));
    assert!(denied(
        repo.execute_content_for_web(command(121, 30, 2, 1, "invalid now"), &global)
            .unwrap_err()
    ));
    // Existing native global-domain authorization still allows a managed slot.
    assert!(repo
        .execute(
            command(122, 30, 2, 1, "native global override"),
            &global,
            &*f.clock
        )
        .is_ok());
    let teacher = actor(3, TrustedOrigin::Web);
    let other = f.repository(teacher.clone());
    assert_eq!(
        other
            .content_outcome(CommandId::new(120).unwrap(), &teacher)
            .unwrap(),
        None
    );
}

#[test]
fn web_midnight_and_persistent_day_floor_never_release_expired_or_cleared_text() {
    let mut f = Fixture::new();
    let teacher = actor(2, TrustedOrigin::Web);
    let mut repo = f.repository(teacher.clone());
    repo.execute_content_for_web(command(130, 30, 2, 0, "yesterday"), &teacher)
        .unwrap();
    repo.execute_content_for_web(command(131, 30, 2, 1, ""), &teacher)
        .unwrap();
    *f.clock.0.lock().unwrap() = time(31);
    for id in [130, 131] {
        assert_eq!(
            repo.content_outcome(CommandId::new(id).unwrap(), &teacher),
            Err(wrong_day())
        );
    }
    assert!(repo.content(key(30, 2), &teacher).unwrap().is_none());
    assert_eq!(repo.content_for_web(key(30, 2), &teacher), Err(wrong_day()));
    repo.execute_content_for_web(command(132, 31, 2, 0, "today"), &teacher)
        .unwrap();
    drop(repo);
    f.reopen();
    *f.clock.0.lock().unwrap() = time(30);
    let mut repo = f.repository(teacher.clone());
    assert!(repo.content(key(30, 2), &teacher).unwrap().is_none());
    assert_eq!(repo.content_for_web(key(30, 2), &teacher), Err(wrong_day()));
    assert_eq!(
        repo.content_outcome(CommandId::new(130).unwrap(), &teacher),
        Err(wrong_day())
    );
    assert_eq!(
        repo.execute_content_for_web(command(133, 30, 2, 2, "revive"), &teacher),
        Err(wrong_day())
    );
    assert_eq!(
        repo.execute(command(130, 30, 2, 0, "yesterday"), &teacher, &*f.clock),
        Err(wrong_day())
    );
}

#[test]
fn web_reminder_receipts_require_web_origin_and_do_not_grant_other_authors_access() {
    let f = Fixture::new();
    let sender = actor(2, TrustedOrigin::Web);
    let mut repo = f.repository(sender.clone());
    let command = ContentCommand {
        command_id: CommandId::new(140).unwrap(),
        key: ContentKey::SharedReminder(ReminderKey {
            class_id: ClassId::new(1).unwrap(),
            actual_date: date(30),
        }),
        expected_revision: Revision::default(),
        text: "shared reminder".into(),
    };
    repo.execute_content_for_web(command.clone(), &sender)
        .unwrap();
    let local = actor(2, TrustedOrigin::Local);
    assert_eq!(
        f.repository(local.clone())
            .content_outcome(command.command_id, &local),
        Err(RepositoryError::Domain(ContentError::Permission(
            PermissionError::WebOnly
        )))
    );
    for id in [1, 3] {
        let other = actor(id, TrustedOrigin::Web);
        assert!(f
            .repository(other.clone())
            .content_outcome(command.command_id, &other)
            .unwrap()
            .is_none());
    }
    let mut changed = state();
    changed.classes[0].enabled = false;
    changed.installation.as_mut().unwrap().current_class = None;
    changed.validate().unwrap();
    f.replace(IDENTITY, codec::bytes(&codec::identity(&changed)).unwrap());
    assert!(denied(
        repo.content_outcome(command.command_id, &sender)
            .unwrap_err()
    ));
}

#[test]
fn web_read_only_new_day_observation_survives_restart_and_wall_rollback_without_desktop() {
    let mut f = Fixture::new();
    let teacher = actor(2, TrustedOrigin::Web);
    let mut repo = f.repository(teacher.clone());
    repo.execute_content_for_web(command(145, 30, 2, 0, "old day"), &teacher)
        .unwrap();
    let old_revision = f.revision();
    *f.clock.0.lock().unwrap() = time(31);
    // No desktop facts or new-day content write: this authenticated read alone
    // must establish the day floor even though its requested old slot is stale.
    assert_eq!(repo.content_for_web(key(30, 2), &teacher), Err(wrong_day()));
    assert_eq!(f.revision(), old_revision.next().unwrap());
    assert_eq!(repo.observe_content_day(&teacher).unwrap().date, date(31));
    let observed_revision = f.revision();
    assert!(repo
        .content_for_web(key(31, 2), &teacher)
        .unwrap()
        .is_none());
    assert!(repo
        .content_outcome(CommandId::new(9999).unwrap(), &teacher)
        .unwrap()
        .is_none());
    assert_eq!(
        f.revision(),
        observed_revision,
        "ordinary same-day reads never write"
    );
    drop(repo);
    f.reopen();
    *f.clock.0.lock().unwrap() = time(30);
    let repo = f.repository(teacher.clone());
    assert_eq!(repo.observe_content_day(&teacher), Err(wrong_day()));
    assert_eq!(repo.content_for_web(key(30, 2), &teacher), Err(wrong_day()));
    assert_eq!(
        repo.content_outcome(CommandId::new(145).unwrap(), &teacher),
        Err(wrong_day())
    );
    assert_eq!(f.revision(), observed_revision);
}

#[test]
fn web_committed_but_midnight_or_revoked_result_release_is_unknown_not_prewrite_denial() {
    for midnight in [true, false] {
        let f = Fixture::new();
        let teacher = actor(2, TrustedOrigin::Web);
        let mut repo = f.repository(teacher.clone());
        assert!(repo
            .content_for_web(key(30, 2), &teacher)
            .unwrap()
            .is_none());
        let before = f.revision();
        if midnight {
            f.midnight.store(true, Ordering::Release);
        } else {
            f.revoke_publish.store(true, Ordering::Release);
        }
        assert_eq!(
            repo.execute_content_for_web(command(150, 30, 2, 0, "committed result"), &teacher),
            Err(RepositoryError::Unavailable)
        );
        assert_eq!(
            f.revision(),
            if midnight {
                before.next().unwrap().next().unwrap()
            } else {
                before.next().unwrap()
            }
        );
        assert!(f
            .handle()
            .try_get("business/receipt/content/150".into())
            .unwrap()
            .wait()
            .unwrap()
            .is_some());
        *f.clock.0.lock().unwrap() = time(30);
        f.web.store(true, Ordering::Release);
        if midnight {
            assert_eq!(
                repo.content_outcome(CommandId::new(150).unwrap(), &teacher),
                Err(wrong_day())
            );
        } else {
            assert_eq!(
                repo.content_outcome(CommandId::new(150).unwrap(), &teacher)
                    .unwrap()
                    .unwrap()
                    .text,
                "committed result"
            );
        }
        let stored = f
            .handle()
            .try_get(content_key(&key(30, 2)))
            .unwrap()
            .wait()
            .unwrap()
            .unwrap();
        assert_eq!(
            codec::read_record(&codec::value(&stored.value).unwrap())
                .unwrap()
                .revision,
            Revision::new(1)
        );
    }
}

#[test]
fn web_precommit_lease_revocation_rolls_back_receipt_and_slot() {
    let f = Fixture::new();
    let teacher = actor(2, TrustedOrigin::Web);
    let mut repo = f.repository(teacher.clone());
    let before = f.revision();
    f.revoke_begin.store(true, Ordering::Release);
    assert!(denied(
        repo.execute_content_for_web(command(160, 30, 2, 0, "not committed"), &teacher)
            .unwrap_err()
    ));
    assert_eq!(f.revision(), before);
    assert!(f
        .handle()
        .try_get("business/receipt/content/160".into())
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
    assert!(f
        .handle()
        .try_get(content_key(&key(30, 2)))
        .unwrap()
        .wait()
        .unwrap()
        .is_none());
}

#[test]
fn web_receipt_shape_digest_length_duplicates_and_record_corruption_are_rejected() {
    let f = Fixture::new();
    let teacher = actor(2, TrustedOrigin::Web);
    let mut repo = f.repository(teacher.clone());
    repo.execute_content_for_web(command(170, 30, 2, 0, "valid"), &teacher)
        .unwrap();
    let journal = "business/receipt/content/170";
    let original = f
        .handle()
        .try_get(journal.into())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap()
        .value;
    let parsed = codec::value(&original).unwrap();
    let mut cases = Vec::new();
    let mut unknown = parsed.clone();
    unknown["unknown"] = json!(0);
    cases.push(codec::bytes(&unknown).unwrap());
    let mut short = parsed.clone();
    short["fingerprint"] = json!(vec![0; 31]);
    cases.push(codec::bytes(&short).unwrap());
    let mut bad_record = parsed;
    bad_record["record"]["revision"] = json!(0);
    cases.push(codec::bytes(&bad_record).unwrap());
    let mut duplicate = b"{\"fingerprint\":[],".to_vec();
    duplicate.extend_from_slice(&original[1..]);
    cases.push(duplicate);
    for damaged in cases {
        f.replace(journal, damaged);
        assert_eq!(
            repo.content_outcome(CommandId::new(170).unwrap(), &teacher),
            Err(RepositoryError::Unavailable)
        );
        assert_eq!(
            repo.execute_content_for_web(command(170, 30, 2, 0, "valid"), &teacher),
            Err(RepositoryError::Unavailable)
        );
    }
}

#[test]
fn web_receipt_requires_current_account_epoch_but_new_own_session_can_query_original_commit() {
    let f = Fixture::new();
    let original = actor(2, TrustedOrigin::Web);
    let mut repo = f.repository(original.clone());
    let saved = repo
        .execute_content_for_web(command(180, 30, 2, 0, "original commit"), &original)
        .unwrap();
    let mut changed = state();
    changed
        .accounts
        .iter_mut()
        .find(|a| a.principal.id == principal(2))
        .unwrap()
        .principal
        .auth_epoch = AuthEpoch::new(2);
    changed.revision = Revision::new(2);
    changed.validate().unwrap();
    f.replace(IDENTITY, codec::bytes(&codec::identity(&changed)).unwrap());
    assert!(denied(
        repo.content_outcome(CommandId::new(180).unwrap(), &original)
            .unwrap_err()
    ));
    let mut fresh = original;
    fresh.auth_epoch = AuthEpoch::new(2);
    let mut current = f.repository(fresh.clone());
    assert_eq!(
        current
            .content_outcome(CommandId::new(180).unwrap(), &fresh)
            .unwrap(),
        Some(saved)
    );
    assert_eq!(
        current.execute_content_for_web(command(180, 30, 2, 0, "original commit"), &fresh),
        Err(RepositoryError::Domain(ContentError::IdempotencyConflict))
    );
    let user = actor(4, TrustedOrigin::Web);
    let before = f.revision();
    assert!(denied(
        f.repository(user.clone())
            .observe_content_day(&user)
            .unwrap_err()
    ));
    assert_eq!(f.revision(), before);
}
