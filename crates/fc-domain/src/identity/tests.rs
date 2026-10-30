use super::*;
use crate::time::LocalDate;

fn now() -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 36000).unwrap()
}
fn admin() -> PrincipalId {
    PrincipalId::new(1).unwrap()
}
fn class() -> ClassId {
    ClassId::new(1).unwrap()
}
fn subject() -> SubjectId {
    SubjectId::new(1).unwrap()
}
fn actor(state: &IdentityState) -> ActorContext {
    ActorContext {
        principal_id: admin(),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: state.account(admin()).unwrap().principal.auth_epoch,
        expires_at: LocalDateTime::new(now().date, 40000).unwrap(),
        origin: TrustedOrigin::Local,
    }
}
fn initial() -> IdentityState {
    IdentityState::default()
        .transition(
            &IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "未来学校".into(),
                id: admin(),
                login_name: "Admin".into(),
                display_name: "主管理员".into(),
            },
            None,
            now(),
        )
        .unwrap()
        .next
}
fn apply(state: IdentityState, command: IdentityCommand) -> IdentityState {
    let a = actor(&state);
    state.transition(&command, Some(&a), now()).unwrap().next
}
fn catalog() -> IdentityState {
    let state = apply(
        initial(),
        IdentityCommand::CreateClass {
            id: class(),
            name: "一年一班".into(),
        },
    );
    apply(
        state,
        IdentityCommand::PutSubject {
            id: subject(),
            name: "数学".into(),
            enabled: true,
            color: Rgb {
                red: 10,
                green: 20,
                blue: 30,
            },
            expected_revision: None,
        },
    )
}
fn teacher(id: u64) -> NewAccount {
    NewAccount {
        id: PrincipalId::new(id).unwrap(),
        login_name: format!("teacher{id}"),
        display_name: "任课老师".into(),
        role: Role::TeacherAdmin,
        scopes: vec![AssignmentScope {
            class_id: class(),
            subject_id: subject(),
        }],
    }
}
#[test]
fn initialization_is_atomic_and_cannot_reopen_after_interrupted_wizard() {
    let state = initial();
    assert_eq!(state.accounts.len(), 1);
    assert!(state.installation.as_ref().unwrap().current_class.is_none());
    assert_eq!(
        state.transition(
            &IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(2).unwrap(),
                school_name: "另校".into(),
                id: PrincipalId::new(2).unwrap(),
                login_name: "x".into(),
                display_name: "x".into()
            },
            None,
            now()
        ),
        Err(IdentityError::AlreadyInitialized)
    );
    let mut corrupt = state.clone();
    corrupt.installation = None;
    assert_eq!(corrupt.validate(), Err(IdentityError::InvalidState));
    let mut corrupt = state.clone();
    corrupt.accounts[0].credential_version = 0;
    assert_eq!(corrupt.validate(), Err(IdentityError::InvalidState));
}
#[test]
fn teacher_requires_actual_active_class_subject_pairs_and_unique_names() {
    let state = catalog();
    let a = actor(&state);
    let mut spec = teacher(2);
    spec.scopes.clear();
    assert_eq!(
        state.transition(
            &IdentityCommand::CreateAccount(spec.clone()),
            Some(&a),
            now()
        ),
        Err(IdentityError::ScopeRequired)
    );
    spec.scopes.push(AssignmentScope {
        class_id: ClassId::new(99).unwrap(),
        subject_id: subject(),
    });
    assert_eq!(
        state.transition(&IdentityCommand::CreateAccount(spec), Some(&a), now()),
        Err(IdentityError::InvalidScope)
    );
    let mut spec = teacher(2);
    spec.login_name = " ADMIN ".into();
    assert_eq!(
        state.transition(&IdentityCommand::CreateAccount(spec), Some(&a), now()),
        Err(IdentityError::DuplicateName)
    );
    let state = apply(state, IdentityCommand::CreateAccount(teacher(2)));
    assert!(
        state
            .account(PrincipalId::new(2).unwrap())
            .unwrap()
            .must_change_password
    );
    assert!(state.account_by_login(" TEACHER2 ").is_ok());
}
#[test]
fn roles_are_separate_from_teaching_scope_and_only_local_global_can_manage() {
    let state = apply(catalog(), IdentityCommand::CreateAccount(teacher(2)));
    let mut a = actor(&state);
    a.origin = TrustedOrigin::Web;
    assert_eq!(
        state.transition(
            &IdentityCommand::CreateClass {
                id: ClassId::new(2).unwrap(),
                name: "二班".into()
            },
            Some(&a),
            now()
        ),
        Err(IdentityError::Permission(PermissionError::LocalOnly))
    );
    a.principal_id = PrincipalId::new(2).unwrap();
    a.auth_epoch = AuthEpoch::new(1);
    a.origin = TrustedOrigin::Local;
    assert_eq!(
        state.transition(
            &IdentityCommand::BindCurrentClass { id: class() },
            Some(&a),
            now()
        ),
        Err(IdentityError::Permission(PermissionError::RoleDenied))
    );
    let user = NewAccount {
        id: PrincipalId::new(3).unwrap(),
        login_name: "reader".into(),
        display_name: "用户".into(),
        role: Role::User,
        scopes: vec![],
    };
    let state = apply(state, IdentityCommand::CreateAccount(user));
    assert!(!state.is_teacher(PrincipalId::new(3).unwrap(), class(), subject()));
    assert!(!state.is_teacher(admin(), class(), subject()));
}
#[test]
fn last_admin_is_protected_and_handover_needs_existing_admin() {
    let state = initial();
    let a = actor(&state);
    let demote = IdentityCommand::UpdateAccount {
        id: admin(),
        display_name: "x".into(),
        role: Role::User,
        enabled: true,
        scopes: vec![],
    };
    assert_eq!(
        state.transition(&demote, Some(&a), now()),
        Err(IdentityError::LastAdministrator)
    );
    let state = apply(
        state,
        IdentityCommand::CreateAccount(NewAccount {
            id: PrincipalId::new(2).unwrap(),
            login_name: "nextadmin".into(),
            display_name: "新管理员".into(),
            role: Role::GlobalAdmin,
            scopes: vec![],
        }),
    );
    let state = apply(state, demote);
    assert_eq!(state.account(admin()).unwrap().principal.role, Role::User);
    assert_eq!(
        state.account(admin()).unwrap().principal.auth_epoch,
        AuthEpoch::new(2)
    );
}
#[test]
fn rename_preserves_subject_id_and_cas_disabling_revokes_all_affected_teachers() {
    let state = apply(catalog(), IdentityCommand::CreateAccount(teacher(2)));
    let old = state.clone();
    let command = |name: &str, enabled, revision| IdentityCommand::PutSubject {
        id: subject(),
        name: name.into(),
        enabled,
        color: Rgb {
            red: 5,
            green: 6,
            blue: 7,
        },
        expected_revision: Some(Revision::new(revision)),
    };
    let state = apply(state, command("数学提高", true, 1));
    assert_eq!(state.subjects[0].id, subject());
    assert_eq!(state.assignments, old.assignments);
    assert_eq!(
        state.transition(&command("不该覆盖", true, 1), Some(&actor(&state)), now()),
        Err(IdentityError::RevisionConflict)
    );
    let state = apply(state, command("数学提高", false, 2));
    assert_eq!(
        state
            .account(PrincipalId::new(2).unwrap())
            .unwrap()
            .principal
            .auth_epoch,
        AuthEpoch::new(2)
    );
    assert!(!state.is_teacher(PrincipalId::new(2).unwrap(), class(), subject()));
    assert_eq!(state.validate(), Ok(()));
}
#[test]
fn multiple_teachers_require_explicit_selection_and_defaults_validate_relationships() {
    let state = apply(
        apply(catalog(), IdentityCommand::CreateAccount(teacher(2))),
        IdentityCommand::CreateAccount(teacher(3)),
    );
    assert_eq!(
        state.resolve_teacher(class(), subject(), None),
        Err(IdentityError::AmbiguousTeacher)
    );
    assert_eq!(
        state.resolve_teacher(class(), subject(), Some(admin())),
        Err(IdentityError::InvalidScope)
    );
    let state = apply(
        state,
        IdentityCommand::SetDefaultTeacher(DefaultTeacher {
            class_id: class(),
            subject_id: subject(),
            teacher_id: PrincipalId::new(2).unwrap(),
        }),
    );
    assert_eq!(
        state.resolve_teacher(class(), subject(), None),
        Ok(PrincipalId::new(2).unwrap())
    );
    let state = apply(
        state,
        IdentityCommand::UpdateAccount {
            id: PrincipalId::new(2).unwrap(),
            display_name: "离任".into(),
            role: Role::TeacherAdmin,
            enabled: false,
            scopes: teacher(2).scopes,
        },
    );
    assert!(state.default_teachers.is_empty());
    assert_eq!(
        state.resolve_teacher(class(), subject(), None),
        Ok(PrincipalId::new(3).unwrap())
    );
}
#[test]
fn first_password_change_and_scope_change_revoke_queued_actors() {
    let state = apply(catalog(), IdentityCommand::CreateAccount(teacher(2)));
    let mut a = actor(&state);
    a.principal_id = PrincipalId::new(2).unwrap();
    a.auth_epoch = AuthEpoch::new(1);
    let change = IdentityCommand::PasswordChanged {
        id: a.principal_id,
        expected_credential_version: 1,
    };
    let result = state.transition(&change, Some(&a), now()).unwrap();
    assert!(
        !result
            .next
            .account(a.principal_id)
            .unwrap()
            .must_change_password
    );
    assert_eq!(
        result
            .next
            .account(a.principal_id)
            .unwrap()
            .credential_version,
        2
    );
    assert_eq!(
        result
            .next
            .authority(a.principal_id)
            .unwrap()
            .validate_actor(&a, now()),
        Err(PermissionError::StaleAuthorization)
    );
    assert_eq!(
        result.next.transition(&change, Some(&a), now()),
        Err(IdentityError::Permission(
            PermissionError::StaleAuthorization
        ))
    );
}
#[test]
fn recovery_rotates_once_and_cannot_revive_any_old_account_actor() {
    let state = apply(catalog(), IdentityCommand::CreateAccount(teacher(2)));
    let a = actor(&state);
    let id = PrincipalId::new(2).unwrap();
    let command = IdentityCommand::RecoveryConsumed {
        id,
        expected_recovery_version: 1,
    };
    let result = state.transition(&command, None, now()).unwrap();
    assert_eq!(result.next.account(id).unwrap().recovery_version, 2);
    assert_eq!(result.next.account(id).unwrap().credential_version, 2);
    assert_eq!(
        result
            .next
            .authority(admin())
            .unwrap()
            .validate_actor(&a, now()),
        Err(PermissionError::StaleAuthorization)
    );
    assert_eq!(
        result.next.transition(&command, None, now()),
        Err(IdentityError::RevisionConflict)
    );
    assert!(result
        .events
        .contains(&IdentityEvent::AllAuthorizationRevoked));
}
#[test]
fn binding_and_names_are_bounded_and_reject_wrong_or_disabled_classes() {
    let state = apply(catalog(), IdentityCommand::BindCurrentClass { id: class() });
    assert_eq!(
        state.transition(
            &IdentityCommand::UpdateClass {
                id: class(),
                name: "新班名".into(),
                enabled: false
            },
            Some(&actor(&state)),
            now()
        ),
        Err(IdentityError::InvalidScope)
    );
    assert_eq!(
        state.transition(
            &IdentityCommand::RenameSchool {
                name: "学校".repeat(129)
            },
            Some(&actor(&state)),
            now()
        ),
        Err(IdentityError::InvalidText)
    );
    assert_eq!(
        normalized_name("\nadmin", 64),
        Err(IdentityError::InvalidText)
    );
    assert_eq!(normalized_name("  数学  提高  ", 64).unwrap(), "数学 提高");
}
#[test]
fn overflow_is_atomic_and_invalid_reconstruction_fails_closed() {
    let mut state = initial();
    state.revision = Revision::new(u64::MAX);
    let before = state.clone();
    assert_eq!(
        state.transition(
            &IdentityCommand::RenameSchool {
                name: "学校".into()
            },
            Some(&actor(&state)),
            now()
        ),
        Err(IdentityError::VersionOverflow)
    );
    assert_eq!(state, before);
    state.revision = Revision::new(1);
    state.accounts[0].principal.enabled = false;
    assert_eq!(state.validate(), Err(IdentityError::LastAdministrator));
}

#[test]
fn self_logout_accepts_local_and_web_and_persists_only_the_accounts_epoch() {
    let mut state = apply(
        catalog(),
        IdentityCommand::CreateAccount(NewAccount {
            id: PrincipalId::new(2).unwrap(),
            login_name: "reader".into(),
            display_name: "用户".into(),
            role: Role::User,
            scopes: vec![],
        }),
    );
    state.accounts[1].must_change_password = false;
    for origin in [TrustedOrigin::Local, TrustedOrigin::Web] {
        let mut a = actor(&state);
        a.principal_id = state.accounts[1].principal.id;
        a.auth_epoch = state.accounts[1].principal.auth_epoch;
        a.origin = origin;
        let transition = state
            .transition(
                &IdentityCommand::LogoutAccount {
                    id: a.principal_id,
                    expected_auth_epoch: a.auth_epoch,
                },
                Some(&a),
                now(),
            )
            .unwrap();
        assert_eq!(
            transition.events,
            vec![IdentityEvent::AuthorizationRevoked(a.principal_id)]
        );
        assert_eq!(transition.next.revision, state.revision.next().unwrap());
        assert_eq!(transition.next.accounts[0], state.accounts[0]);
        assert_eq!(
            transition.next.accounts[1].principal.auth_epoch,
            a.auth_epoch.next().unwrap()
        );
        assert_eq!(
            transition.next.accounts[1].credential_version,
            state.accounts[1].credential_version
        );
        assert_eq!(
            transition.next.accounts[1].recovery_version,
            state.accounts[1].recovery_version
        );
        assert_eq!(
            transition
                .next
                .authority(a.principal_id)
                .unwrap()
                .validate_actor(&a, now()),
            Err(PermissionError::StaleAuthorization)
        );
    }
}

#[test]
fn logout_requires_current_self_actor_and_expected_epoch_without_mutating_on_failure() {
    let state = apply(catalog(), IdentityCommand::CreateAccount(teacher(2)));
    let a = actor(&state);
    let command = IdentityCommand::LogoutAccount {
        id: admin(),
        expected_auth_epoch: a.auth_epoch,
    };
    assert_eq!(
        state.transition(&command, None, now()),
        Err(IdentityError::Permission(PermissionError::RoleDenied))
    );
    assert_eq!(
        state.transition(
            &IdentityCommand::LogoutAccount {
                id: PrincipalId::new(2).unwrap(),
                expected_auth_epoch: AuthEpoch::new(1)
            },
            Some(&a),
            now()
        ),
        Err(IdentityError::Permission(PermissionError::WrongOwner))
    );
    assert_eq!(
        state.transition(
            &IdentityCommand::LogoutAccount {
                id: admin(),
                expected_auth_epoch: AuthEpoch::new(2)
            },
            Some(&a),
            now()
        ),
        Err(IdentityError::RevisionConflict)
    );
    let mut expired = a.clone();
    expired.expires_at = now();
    assert_eq!(
        state.transition(&command, Some(&expired), now()),
        Err(IdentityError::Permission(PermissionError::Expired))
    );
    let mut stale = a;
    stale.auth_epoch = AuthEpoch::new(2);
    assert_eq!(
        state.transition(&command, Some(&stale), now()),
        Err(IdentityError::Permission(
            PermissionError::StaleAuthorization
        ))
    );
    let mut initial_actor = actor(&state);
    initial_actor.principal_id = PrincipalId::new(2).unwrap();
    initial_actor.auth_epoch = AuthEpoch::new(1);
    assert_eq!(
        state.transition(
            &IdentityCommand::LogoutAccount {
                id: initial_actor.principal_id,
                expected_auth_epoch: initial_actor.auth_epoch
            },
            Some(&initial_actor),
            now()
        ),
        Err(IdentityError::PasswordChangeRequired)
    );
}

#[test]
fn logout_epoch_and_identity_revision_overflow_fail_atomically() {
    let mut state = initial();
    for (epoch, revision) in [(u64::MAX, 1), (1, u64::MAX)] {
        state.accounts[0].principal.auth_epoch = AuthEpoch::new(epoch);
        state.revision = Revision::new(revision);
        let before = state.clone();
        let a = actor(&state);
        assert_eq!(
            state.transition(
                &IdentityCommand::LogoutAccount {
                    id: admin(),
                    expected_auth_epoch: a.auth_epoch
                },
                Some(&a),
                now()
            ),
            Err(IdentityError::VersionOverflow)
        );
        assert_eq!(state, before);
    }
}
