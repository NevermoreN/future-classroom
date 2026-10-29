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
