use super::*;
use fc_domain::{auth::*, content::*, ids::*, time::*};
use std::{cell::Cell, collections::HashMap};

fn date(day: u8) -> LocalDate {
    LocalDate::new(2026, 10, day).unwrap()
}
fn at(day: u8, hour: u32) -> LocalDateTime {
    LocalDateTime::new(date(day), hour * 3600).unwrap()
}
struct TestClock(Cell<LocalDateTime>);
impl Clock for TestClock {
    fn now(&self) -> LocalDateTime {
        self.0.get()
    }
}
struct Store {
    authority: AuthorityContext,
    rows: HashMap<ContentKey, ContentRecord>,
    journal: HashMap<CommandId, (PrincipalId, ContentCommand, ContentRecord)>,
    revoke_at_commit: bool,
}
impl ContentRepository for Store {
    fn authority(&self, _: &ActorContext) -> Result<AuthorityContext, RepositoryError> {
        Ok(self.authority.clone())
    }
    fn execute(
        &mut self,
        c: ContentCommand,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<ContentRecord, RepositoryError> {
        if self.revoke_at_commit {
            self.authority.principal.auth_epoch =
                self.authority.principal.auth_epoch.next().unwrap();
        }
        let now = clock.now();
        c.validate(&self.authority, actor, now)
            .map_err(RepositoryError::Domain)?;
        if let Some((who, original, result)) = self.journal.get(&c.command_id) {
            if *who == actor.principal_id && original == &c {
                return Ok(result.clone());
            }
            return Err(RepositoryError::Domain(ContentError::IdempotencyConflict));
        }
        let row = c
            .apply(&self.authority, actor, now, self.rows.get(&c.key))
            .map_err(RepositoryError::Domain)?;
        self.rows.insert(c.key.clone(), row.clone());
        self.journal
            .insert(c.command_id, (actor.principal_id, c, row.clone()));
        Ok(row)
    }
}
fn fixture(role: Role, origin: TrustedOrigin) -> (Store, ActorContext, TestClock) {
    let principal = Principal {
        id: PrincipalId::new(1).unwrap(),
        school_id: SchoolId::new(1).unwrap(),
        role,
        enabled: true,
        auth_epoch: AuthEpoch::new(1),
    };
    let actor = ActorContext {
        principal_id: principal.id,
        school_id: principal.school_id,
        auth_epoch: principal.auth_epoch,
        expires_at: at(28, 0),
        origin,
    };
    let assignment = TeachingAssignment {
        teacher_id: principal.id,
        class_id: ClassId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
        enabled: true,
    };
    (
        Store {
            authority: AuthorityContext {
                installation_school: principal.school_id,
                principal,
                managed_classes: vec![ClassId::new(1).unwrap(), ClassId::new(2).unwrap()],
                assignments: vec![assignment],
            },
            rows: HashMap::new(),
            journal: HashMap::new(),
            revoke_at_commit: false,
        },
        actor,
        TestClock(Cell::new(at(26, 12))),
    )
}
fn homework(owner: u64) -> ContentCommand {
    ContentCommand {
        command_id: CommandId::new(1).unwrap(),
        key: ContentKey::Homework(HomeworkKey {
            class_id: ClassId::new(1).unwrap(),
            actual_date: date(26),
            subject_id: SubjectId::new(1).unwrap(),
            owner_teacher_id: PrincipalId::new(owner).unwrap(),
        }),
        expected_revision: Revision::default(),
        text: "  阅读三页  ".into(),
    }
}
fn reminder(class: u64) -> ContentCommand {
    ContentCommand {
        command_id: CommandId::new(2).unwrap(),
        key: ContentKey::SharedReminder(ReminderKey {
            class_id: ClassId::new(class).unwrap(),
            actual_date: date(26),
        }),
        expected_revision: Revision::default(),
        text: "放学带伞".into(),
    }
}

#[test]
fn own_homework_is_independent_of_other_teacher_same_subject() {
    let (mut repo, actor, clock) = fixture(Role::TeacherAdmin, TrustedOrigin::Web);
    assert_eq!(
        execute_content(&mut repo, &clock, &actor, homework(2)),
        Err(RepositoryError::Domain(ContentError::Permission(
            PermissionError::WrongOwner
        )))
    );
    let saved = execute_content(&mut repo, &clock, &actor, homework(1)).unwrap();
    assert_eq!(saved.text, "阅读三页");
    assert_eq!(saved.revision, Revision::new(1));
    assert_eq!(repo.rows.len(), 1);
}
#[test]
fn role_is_independent_of_residual_teaching_assignments() {
    let (mut repo, actor, clock) = fixture(Role::User, TrustedOrigin::Web);
    assert!(execute_content(&mut repo, &clock, &actor, homework(1)).is_err());
    assert!(execute_content(&mut repo, &clock, &actor, reminder(1)).is_err());
    assert!(repo.rows.is_empty());
}
#[test]
fn shared_reminders_cover_managed_classes_without_teaching_scope() {
    let (mut repo, actor, clock) = fixture(Role::TeacherAdmin, TrustedOrigin::Web);
    assert!(execute_content(&mut repo, &clock, &actor, reminder(2)).is_ok());
    let mut changed = reminder(2);
    changed.command_id = CommandId::new(3).unwrap();
    changed.expected_revision = Revision::new(1);
    changed.text = "共同编辑".into();
    assert_eq!(
        execute_content(&mut repo, &clock, &actor, changed)
            .unwrap()
            .revision,
        Revision::new(2)
    );
    assert!(execute_content(&mut repo, &clock, &actor, reminder(3)).is_err());
}
#[test]
fn local_reminder_write_is_denied_even_for_global_admin() {
    for role in [Role::GlobalAdmin, Role::TeacherAdmin] {
        let (mut repo, actor, clock) = fixture(role, TrustedOrigin::Local);
        assert_eq!(
            execute_content(&mut repo, &clock, &actor, reminder(1)),
            Err(RepositoryError::Domain(ContentError::Permission(
                PermissionError::WebOnly
            )))
        );
    }
}
#[test]
fn idempotency_does_not_bypass_cas_or_allow_changed_payload() {
    let (mut repo, actor, clock) = fixture(Role::TeacherAdmin, TrustedOrigin::Web);
    let first = execute_content(&mut repo, &clock, &actor, homework(1)).unwrap();
    assert_eq!(
        execute_content(&mut repo, &clock, &actor, homework(1)),
        Ok(first)
    );
    let mut changed = homework(1);
    changed.text = "different".into();
    assert_eq!(
        execute_content(&mut repo, &clock, &actor, changed),
        Err(RepositoryError::Domain(ContentError::IdempotencyConflict))
    );
    let mut stale = homework(1);
    stale.command_id = CommandId::new(4).unwrap();
    assert_eq!(
        execute_content(&mut repo, &clock, &actor, stale),
        Err(RepositoryError::Domain(ContentError::RevisionConflict {
            current: Revision::new(1)
        }))
    );
    assert_eq!(repo.journal.len(), 1);
}
#[test]
fn writer_rechecks_revocation_after_admission() {
    let (mut repo, actor, clock) = fixture(Role::TeacherAdmin, TrustedOrigin::Web);
    repo.revoke_at_commit = true;
    assert_eq!(
        execute_content(&mut repo, &clock, &actor, homework(1)),
        Err(RepositoryError::Domain(ContentError::Permission(
            PermissionError::StaleAuthorization
        )))
    );
    assert!(repo.rows.is_empty());
    assert!(repo.journal.is_empty());
}
#[test]
fn midnight_commit_and_reads_expire_without_waiting_for_cleanup() {
    let (mut repo, actor, clock) = fixture(Role::TeacherAdmin, TrustedOrigin::Web);
    let saved = execute_content(&mut repo, &clock, &actor, reminder(1)).unwrap();
    assert_eq!(saved.visible_text(date(27)), None);
    clock.0.set(at(27, 0));
    assert_eq!(
        execute_content(&mut repo, &clock, &actor, reminder(1)),
        Err(RepositoryError::Domain(ContentError::WrongDate))
    );
    struct MidnightClock(Cell<bool>);
    impl Clock for MidnightClock {
        fn now(&self) -> LocalDateTime {
            if self.0.replace(true) {
                at(27, 0)
            } else {
                at(26, 23)
            }
        }
    }
    let (mut repo, actor, _) = fixture(Role::TeacherAdmin, TrustedOrigin::Web);
    assert_eq!(
        execute_content(
            &mut repo,
            &MidnightClock(Cell::new(false)),
            &actor,
            reminder(1)
        ),
        Err(RepositoryError::Domain(ContentError::WrongDate))
    );
    assert!(repo.rows.is_empty());
}
#[test]
fn school_scope_disabled_assignment_and_session_expiry_are_enforced() {
    let (mut repo, mut actor, clock) = fixture(Role::TeacherAdmin, TrustedOrigin::Web);
    actor.school_id = SchoolId::new(2).unwrap();
    assert!(execute_content(&mut repo, &clock, &actor, homework(1)).is_err());
    actor.school_id = SchoolId::new(1).unwrap();
    repo.authority.assignments[0].enabled = false;
    assert!(execute_content(&mut repo, &clock, &actor, homework(1)).is_err());
    actor.expires_at = clock.now();
    assert_eq!(
        execute_content(&mut repo, &clock, &actor, reminder(1)),
        Err(RepositoryError::Domain(ContentError::Permission(
            PermissionError::Expired
        )))
    );
}
