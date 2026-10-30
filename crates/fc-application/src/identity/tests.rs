use super::*;
use fc_domain::time::LocalDate;
fn time(seconds: u32) -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), seconds).unwrap()
}
fn state() -> IdentityState {
    IdentityState::default()
        .transition(
            &IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "学校".into(),
                id: PrincipalId::new(1).unwrap(),
                login_name: "admin".into(),
                display_name: "管理员".into(),
            },
            None,
            time(0),
        )
        .unwrap()
        .next
}
fn principal() -> PrincipalId {
    PrincipalId::new(1).unwrap()
}
fn issue(
    sessions: &mut IdentitySessions,
    state: &IdentityState,
    origin: TrustedOrigin,
    tick: u64,
) -> SessionLease {
    sessions
        .issue_verified(
            state.account(principal()).unwrap(),
            origin,
            tick,
            3_600_000,
            time(3600),
        )
        .unwrap()
}
#[test]
fn lock_local_does_not_revoke_independent_web_channel() {
    let state = state();
    let mut sessions = IdentitySessions::default();
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    let web = issue(&mut sessions, &state, TrustedOrigin::Web, 0);
    sessions.lock_local();
    assert_eq!(
        sessions.actor(&state, local, time(1), 1000, SessionPurpose::Manage),
        Err(SessionError::Revoked)
    );
    assert!(sessions
        .actor(&state, web, time(1), 1000, SessionPurpose::Manage)
        .is_ok());
}
#[test]
fn snapshot_reads_do_not_renew_five_minute_local_idle_and_wall_rollback_does_not_help() {
    let state = state();
    let mut sessions = IdentitySessions::default();
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    for tick in [1000, 100_000, 299_999] {
        assert!(sessions
            .actor(&state, local, time(10), tick, SessionPurpose::Manage)
            .is_ok());
    }
    assert_eq!(
        sessions.actor(&state, local, time(0), 300_000, SessionPurpose::Manage),
        Err(SessionError::Expired)
    );
    assert_eq!(
        sessions.local_input(&state, local, time(0), 300_001),
        Err(SessionError::Expired)
    );
}
#[test]
fn real_input_extends_idle_only_up_to_absolute_session_deadline() {
    let state = state();
    let mut sessions = IdentitySessions::default();
    let local = sessions
        .issue_verified(
            state.account(principal()).unwrap(),
            TrustedOrigin::Local,
            0,
            600_000,
            time(3600),
        )
        .unwrap();
    sessions
        .local_input(&state, local, time(100), 299_999)
        .unwrap();
    assert!(sessions
        .actor(&state, local, time(100), 500_000, SessionPurpose::Manage)
        .is_ok());
    sessions
        .local_input(&state, local, time(100), 599_998)
        .unwrap();
    assert_eq!(
        sessions.actor(&state, local, time(100), 600_000, SessionPurpose::Manage),
        Err(SessionError::Expired)
    );
    let web = issue(&mut sessions, &state, TrustedOrigin::Web, 0);
    assert_eq!(
        sessions.local_input(&state, web, time(1), 1000),
        Err(SessionError::Revoked)
    );
}
#[test]
fn stale_queued_logout_cannot_revoke_replacement_lease_but_current_logout_revokes_all_channels() {
    let state = state();
    let mut sessions = IdentitySessions::default();
    let old = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    let web = issue(&mut sessions, &state, TrustedOrigin::Web, 0);
    let new = issue(&mut sessions, &state, TrustedOrigin::Local, 1000);
    assert_ne!(old.id(), new.id());
    assert_eq!(
        sessions.logout_account(&state, old, time(2), 2000),
        Err(SessionError::Revoked)
    );
    assert!(sessions
        .actor(&state, new, time(2), 2000, SessionPurpose::Manage)
        .is_ok());
    sessions.logout_account(&state, new, time(2), 2000).unwrap();
    assert_eq!(
        sessions.actor(&state, web, time(2), 2000, SessionPurpose::Manage),
        Err(SessionError::Revoked)
    );
}
#[test]
fn domain_epoch_change_revokes_even_before_event_delivery_and_new_lease_is_not_old_lease() {
    let old_state = state();
    let mut sessions = IdentitySessions::default();
    let old = issue(&mut sessions, &old_state, TrustedOrigin::Web, 0);
    let actor = sessions
        .actor(&old_state, old, time(1), 1000, SessionPurpose::Manage)
        .unwrap();
    let transition = old_state
        .transition(
            &IdentityCommand::PasswordChanged {
                id: principal(),
                expected_credential_version: 1,
            },
            Some(&actor),
            time(1),
        )
        .unwrap();
    assert_eq!(
        sessions.actor(&transition.next, old, time(2), 2000, SessionPurpose::Manage),
        Err(SessionError::Revoked)
    );
    sessions.apply_events(&transition.events);
    let new = issue(&mut sessions, &transition.next, TrustedOrigin::Web, 2000);
    assert_ne!(old.id(), new.id());
    assert!(sessions
        .actor(&transition.next, new, time(2), 2000, SessionPurpose::Manage)
        .is_ok());
}
#[test]
fn first_login_is_password_change_only_and_monotonic_regression_fails_closed() {
    let mut state = state();
    state.accounts[0].must_change_password = true;
    let mut sessions = IdentitySessions::default();
    let lease = issue(&mut sessions, &state, TrustedOrigin::Local, 1000);
    assert_eq!(
        sessions.actor(&state, lease, time(2), 2000, SessionPurpose::Manage),
        Err(SessionError::PasswordChangeRequired)
    );
    assert!(sessions
        .actor(
            &state,
            lease,
            time(2),
            2000,
            SessionPurpose::ChangeInitialPassword
        )
        .is_ok());
    sessions.local_input(&state, lease, time(2), 2000).unwrap();
    assert_eq!(
        sessions.actor(
            &state,
            lease,
            time(2),
            1999,
            SessionPurpose::ChangeInitialPassword
        ),
        Err(SessionError::ClockRegression)
    );
}
#[test]
fn login_penalty_survives_restart_and_cannot_be_skipped_by_wall_clock_changes() {
    let mut backoff = AuthenticationBackoff::restore(LoginPenalty::default(), 0).unwrap();
    for i in 1..=4 {
        assert_eq!(backoff.failed(0).unwrap().failures, i);
    }
    let penalty = backoff.failed(0).unwrap();
    assert_eq!(penalty.wait_seconds, 1);
    assert_eq!(
        backoff.admit(999),
        Err(SessionError::RateLimited {
            retry_after_millis: 1
        })
    );
    let mut reopened = AuthenticationBackoff::restore(penalty, 10_000).unwrap();
    assert_eq!(
        reopened.admit(10_999),
        Err(SessionError::RateLimited {
            retry_after_millis: 1
        })
    );
    assert_eq!(reopened.failed(11_000).unwrap().wait_seconds, 2);
    assert_eq!(reopened.admit(10_999), Err(SessionError::ClockRegression));
    assert_eq!(reopened.succeeded(13_000).unwrap(), LoginPenalty::default());
    assert!(AuthenticationBackoff::restore(
        LoginPenalty {
            failures: 10,
            wait_seconds: 0
        },
        0
    )
    .is_err());
    assert_eq!(
        LoginPenalty {
            failures: u32::MAX,
            wait_seconds: 900
        }
        .failure()
        .wait_seconds,
        900
    );
}
#[test]
fn invalid_lifetimes_and_sequence_overflow_do_not_mutate_live_sessions() {
    let state = state();
    let mut sessions = IdentitySessions::default();
    assert_eq!(
        sessions.issue_verified(
            state.account(principal()).unwrap(),
            TrustedOrigin::Local,
            0,
            0,
            time(3600)
        ),
        Err(SessionError::InvalidDuration)
    );
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    sessions.sequence = u64::MAX;
    assert_eq!(
        sessions.issue_verified(
            state.account(principal()).unwrap(),
            TrustedOrigin::Local,
            0,
            1000,
            time(3600)
        ),
        Err(SessionError::Overflow)
    );
    assert!(sessions
        .actor(&state, local, time(1), 1000, SessionPurpose::Manage)
        .is_ok());
}

#[test]
fn verified_continuous_input_stream_preserves_exact_lease_idle_and_absolute_limits() {
    let state = state();
    let mut sessions = IdentitySessions::default();
    let lease = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    sessions
        .local_input_stream(&state, lease, time(100), 1_200_000, 0, 1_200_000)
        .unwrap();
    sessions
        .actor(&state, lease, time(100), 1_200_000, SessionPurpose::Manage)
        .unwrap();
    sessions
        .local_input_stream(&state, lease, time(100), 1_250_000, 0, 1_250_000)
        .unwrap();
    assert_eq!(
        sessions.local_input_stream(&state, lease, time(100), 1_250_000, 0, 1_250_001),
        Err(SessionError::ClockRegression)
    );
    assert_eq!(
        sessions.local_input_stream(&state, lease, time(100), 1_250_000, 0, 1_200_000),
        Err(SessionError::ClockRegression)
    );
    assert_eq!(
        sessions.local_input_stream(&state, lease, time(100), 3_600_000, 0, 3_600_000),
        Err(SessionError::Expired)
    );
    assert_eq!(
        sessions.actor(&state, lease, time(100), 1_550_000, SessionPurpose::Manage),
        Err(SessionError::Expired),
        "failed updates never refreshed the last real input"
    );
    let replacement = issue(&mut sessions, &state, TrustedOrigin::Local, 1_550_000);
    assert_eq!(
        sessions.local_input_stream(&state, lease, time(100), 1_550_000, 0, 1_550_000),
        Err(SessionError::Revoked)
    );
    sessions
        .actor(
            &state,
            replacement,
            time(100),
            1_550_000,
            SessionPurpose::Manage,
        )
        .unwrap();
    let web = issue(&mut sessions, &state, TrustedOrigin::Web, 1_550_000);
    assert!(sessions
        .local_input_stream(&state, web, time(100), 1_550_000, 1_550_000, 1_550_000)
        .is_err());
}

fn pairing_state() -> IdentityState {
    let mut state = state();
    let mut account = state.accounts[0].clone();
    account.principal.id = PrincipalId::new(2).unwrap();
    account.principal.role = Role::TeacherAdmin;
    account.login_name = "teacher".into();
    account.display_name = "教师".into();
    account.must_change_password = false;
    state.accounts.push(account);
    let school_id = SchoolId::new(1).unwrap();
    let class_id = ClassId::new(1).unwrap();
    let subject_id = SubjectId::new(1).unwrap();
    state.classes.push(SchoolClass {
        id: class_id,
        school_id,
        name: "一班".into(),
        enabled: true,
    });
    state.subjects.push(SchoolSubject {
        id: subject_id,
        school_id,
        name: "语文".into(),
        enabled: true,
        color: Rgb {
            red: 0,
            green: 0,
            blue: 0,
        },
        revision: Revision::new(1),
    });
    state.assignments.push(TeachingAssignment {
        teacher_id: teacher(),
        class_id,
        subject_id,
        enabled: true,
    });
    state.validate().unwrap();
    state
}
fn teacher() -> PrincipalId {
    PrincipalId::new(2).unwrap()
}
fn approve(
    sessions: &mut IdentitySessions,
    state: &IdentityState,
    approver: SessionLease,
) -> Result<SessionLease, SessionError> {
    sessions.issue_approved_web(state, approver, teacher(), time(1), 1000, 60_000, time(61))
}

#[test]
fn local_global_approval_adds_independent_devices_and_exact_revoke_is_scoped() {
    let state = pairing_state();
    let mut sessions = IdentitySessions::default();
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    let first = approve(&mut sessions, &state, local).unwrap();
    let second = approve(&mut sessions, &state, local).unwrap();
    assert_ne!(first.id(), second.id());
    assert_eq!(first.origin(), TrustedOrigin::Web);
    assert_eq!(first.principal_id(), teacher());
    for proof in [local, first, second] {
        sessions
            .actor(&state, proof, time(2), 2000, SessionPurpose::Manage)
            .unwrap();
    }
    let mut fabricated = first;
    fabricated.origin = TrustedOrigin::Local;
    assert!(!sessions.revoke_exact(fabricated));
    fabricated = first;
    fabricated.auth_epoch = AuthEpoch::new(2);
    assert!(!sessions.revoke_exact(fabricated));
    assert!(sessions.revoke_exact(first));
    assert!(!sessions.revoke_exact(first));
    assert_eq!(
        sessions.actor(&state, first, time(2), 2000, SessionPurpose::Manage),
        Err(SessionError::Revoked)
    );
    for proof in [local, second] {
        sessions
            .actor(&state, proof, time(2), 2000, SessionPurpose::Manage)
            .unwrap();
    }
    sessions.lock_local();
    sessions
        .actor(&state, second, time(2), 2000, SessionPurpose::Manage)
        .unwrap();
}

#[test]
fn approval_requires_the_live_local_global_manage_lease_from_this_store() {
    let state = pairing_state();
    let mut sessions = IdentitySessions::default();
    let web = issue(&mut sessions, &state, TrustedOrigin::Web, 0);
    assert_eq!(
        approve(&mut sessions, &state, web),
        Err(SessionError::Revoked)
    );
    let teacher_local = sessions
        .issue_verified(
            state.account(teacher()).unwrap(),
            TrustedOrigin::Local,
            0,
            3_600_000,
            time(3600),
        )
        .unwrap();
    assert_eq!(
        approve(&mut sessions, &state, teacher_local),
        Err(SessionError::Revoked)
    );
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    assert_eq!(
        sessions.issue_approved_web(
            &state,
            local,
            teacher(),
            time(300),
            LOCAL_IDLE_MILLIS,
            60_000,
            time(360)
        ),
        Err(SessionError::Expired)
    );
    let mut other_store = IdentitySessions::default();
    assert_eq!(
        approve(&mut other_store, &state, local),
        Err(SessionError::Revoked)
    );
    let replacement = issue(&mut sessions, &state, TrustedOrigin::Local, 1000);
    assert_eq!(
        approve(&mut sessions, &state, local),
        Err(SessionError::Revoked)
    );
    assert!(approve(&mut sessions, &state, replacement).is_ok());
    let mut initial = state.clone();
    initial.accounts[0].must_change_password = true;
    let initial_local = issue(&mut sessions, &initial, TrustedOrigin::Local, 1000);
    assert_eq!(
        approve(&mut sessions, &initial, initial_local),
        Err(SessionError::PasswordChangeRequired)
    );
}

#[test]
fn approval_rejects_unknown_disabled_initial_password_and_foreign_school_targets() {
    let state = pairing_state();
    let mut sessions = IdentitySessions::default();
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    assert_eq!(
        sessions.issue_approved_web(
            &state,
            local,
            PrincipalId::new(99).unwrap(),
            time(1),
            1000,
            60_000,
            time(61)
        ),
        Err(SessionError::Identity)
    );
    let mut changed = state.clone();
    changed.accounts[1].must_change_password = true;
    assert_eq!(
        approve(&mut sessions, &changed, local),
        Err(SessionError::PasswordChangeRequired)
    );
    changed = state.clone();
    changed.accounts[1].principal.enabled = false;
    assert_eq!(
        approve(&mut sessions, &changed, local),
        Err(SessionError::Revoked)
    );
    changed = state.clone();
    changed.accounts[1].principal.school_id = SchoolId::new(2).unwrap();
    assert_eq!(
        approve(&mut sessions, &changed, local),
        Err(SessionError::Revoked)
    );
    changed = state.clone();
    changed.installation = None;
    assert_eq!(
        approve(&mut sessions, &changed, local),
        Err(SessionError::Identity)
    );
    assert_eq!(sessions.sessions.len(), 1);
}

#[test]
fn password_web_login_still_replaces_all_older_web_leases_for_that_account() {
    let state = pairing_state();
    let mut sessions = IdentitySessions::default();
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    let first = approve(&mut sessions, &state, local).unwrap();
    let second = approve(&mut sessions, &state, local).unwrap();
    let new = sessions
        .issue_verified(
            state.account(teacher()).unwrap(),
            TrustedOrigin::Web,
            2000,
            60_000,
            time(62),
        )
        .unwrap();
    assert!(!sessions.revoke_exact(first));
    assert!(!sessions.revoke_exact(second));
    for proof in [local, new] {
        sessions
            .actor(&state, proof, time(2), 2000, SessionPurpose::Manage)
            .unwrap();
    }
}

#[test]
fn recovery_and_epoch_changes_reject_old_approvers_and_approved_devices_before_events() {
    let state = pairing_state();
    let mut sessions = IdentitySessions::default();
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    let web = approve(&mut sessions, &state, local).unwrap();
    let mut changed = state.clone();
    changed.accounts[1].principal.auth_epoch = AuthEpoch::new(2);
    assert_eq!(
        sessions.actor(&changed, web, time(2), 2000, SessionPurpose::Manage),
        Err(SessionError::Revoked)
    );
    let recovered = state
        .transition(
            &IdentityCommand::RecoveryConsumed {
                id: principal(),
                expected_recovery_version: 1,
            },
            None,
            time(2),
        )
        .unwrap();
    assert_eq!(
        approve(&mut sessions, &recovered.next, local),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        sessions.actor(&recovered.next, web, time(2), 2000, SessionPurpose::Manage),
        Err(SessionError::Revoked)
    );
    sessions.apply_events(&recovered.events);
    assert_eq!(sessions.sessions.len(), 0);
}

#[test]
fn approved_sessions_are_bounded_and_pruning_recovers_expired_capacity() {
    let state = pairing_state();
    let mut sessions = IdentitySessions::default();
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    for _ in 1..MAX_LIVE_LEASES {
        approve(&mut sessions, &state, local).unwrap();
    }
    assert_eq!(sessions.sessions.len(), MAX_LIVE_LEASES);
    let sequence = sessions.sequence;
    assert_eq!(
        approve(&mut sessions, &state, local),
        Err(SessionError::Capacity)
    );
    assert_eq!(sessions.sequence, sequence);
    let new = sessions
        .issue_approved_web(&state, local, teacher(), time(61), 61_000, 1000, time(62))
        .unwrap();
    assert_eq!(sessions.sessions.len(), 2);
    assert!(new.id() > sequence);
    assert_eq!(sessions.prune(&state, time(62), 62_000), 1);
    assert_eq!(sessions.prune(&state, time(62), 62_000), 0);
    sessions
        .actor(&state, local, time(62), 62_000, SessionPurpose::Manage)
        .unwrap();
    assert_eq!(sessions.prune(&state, time(300), LOCAL_IDLE_MILLIS), 1);
}

#[test]
fn approved_admission_bounds_fail_without_replacing_or_consuming_sequence() {
    let state = pairing_state();
    let mut sessions = IdentitySessions::default();
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 1000);
    let tomorrow = LocalDateTime::new(LocalDate::new(2026, 11, 1).unwrap(), 1).unwrap();
    for (duration, expiry) in [
        (0, time(61)),
        (MAX_SESSION_MILLIS + 1, time(61)),
        (1000, time(1)),
        (1000, time(0)),
        (1000, tomorrow),
    ] {
        assert_eq!(
            sessions.issue_approved_web(&state, local, teacher(), time(1), 1000, duration, expiry),
            Err(SessionError::InvalidDuration)
        );
    }
    assert_eq!(
        sessions.issue_approved_web(&state, local, teacher(), time(1), 999, 1000, time(2)),
        Err(SessionError::ClockRegression)
    );
    assert_eq!(sessions.sequence, 1);
    assert_eq!(sessions.sessions.len(), 1);
    sessions.sequence = u64::MAX;
    assert_eq!(
        approve(&mut sessions, &state, local),
        Err(SessionError::Overflow)
    );
    assert_eq!(sessions.sessions.len(), 1);
    sessions.sequence = 1;
    let max = sessions
        .issue_verified(
            state.account(principal()).unwrap(),
            TrustedOrigin::Local,
            u64::MAX - 100,
            100,
            time(3600),
        )
        .unwrap();
    assert_eq!(
        sessions.issue_approved_web(&state, max, teacher(), time(1), u64::MAX - 50, 51, time(2)),
        Err(SessionError::Overflow)
    );
    assert_eq!(sessions.sessions.len(), 1);
}

#[test]
fn approved_maximum_lifetime_and_independent_wall_deadlines_are_exclusive() {
    let state = pairing_state();
    let mut sessions = IdentitySessions::default();
    let local = issue(&mut sessions, &state, TrustedOrigin::Local, 0);
    let tomorrow = LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 1).unwrap();
    let web = sessions
        .issue_approved_web(
            &state,
            local,
            teacher(),
            time(1),
            1000,
            MAX_SESSION_MILLIS,
            tomorrow,
        )
        .unwrap();
    sessions
        .actor(
            &state,
            web,
            time(0),
            MAX_SESSION_MILLIS + 999,
            SessionPurpose::Manage,
        )
        .unwrap();
    assert_eq!(
        sessions.actor(
            &state,
            web,
            time(0),
            MAX_SESSION_MILLIS + 1000,
            SessionPurpose::Manage
        ),
        Err(SessionError::Expired),
        "wall-clock rollback cannot extend the monotonic lifetime"
    );
    assert_eq!(
        sessions.actor(&state, web, tomorrow, 2000, SessionPurpose::Manage),
        Err(SessionError::Expired),
        "the wall deadline independently bounds the lease"
    );
}
