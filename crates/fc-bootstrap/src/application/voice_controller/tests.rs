use super::super::tests::{drain, lose_worker_reply, ready_management, wait_for, SERIAL};
use super::*;

#[test]
fn lost_policy_reply_queries_original_pair_while_current_read_cannot_confirm_it() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app) = ready_management();
    let lease = app.lease.unwrap();
    let service = app.voice_service.as_ref().unwrap().clone();
    let before = service.management_snapshot(lease).unwrap();
    app.submit_voice_management(Operation::SetEnabled {
        expected: before.revision,
        expected_identity: before.identity_revision,
        expected_runtime: before.runtime_generation,
        enabled: false,
    })
    .unwrap();
    let original = app.native_command.as_ref().unwrap().clone();
    let intent = original.intent().cloned();
    assert!(matches!(intent, Some(NativeIntent::VoicePolicy(_))));
    lose_worker_reply(&mut app);
    let pair = original.snapshot().unwrap();
    let committed = service.management_snapshot(lease).unwrap();
    assert_ne!(committed.revision, before.revision);
    // Direct current read succeeds but is not a historical receipt.
    app.submit(Work::Voice(service.clone(), lease, Operation::Refresh))
        .unwrap();
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(app.native_unknown);
    assert_eq!(original.snapshot(), Some(pair));
    assert_eq!(original.intent(), intent.as_ref());
    // The user's explicit refresh now follows the historical query path.
    app.submit_voice_management(Operation::Refresh).unwrap();
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(!app.native_unknown);
    assert_eq!(app.native_command.as_ref(), Some(&original));
    assert_eq!(original.snapshot(), Some(pair));
    let after = service.management_snapshot(lease).unwrap();
    assert_eq!(after.revision, committed.revision);
    assert_eq!(
        after.runtime_generation, committed.runtime_generation,
        "query cannot stop again"
    );
    drain(app);
}

#[test]
fn actual_policy_precommit_publication_keeps_original_ui_lease_and_worker() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Condvar, Mutex,
    };

    struct ReleaseOnDrop(Arc<(Mutex<bool>, Condvar)>);
    impl ReleaseOnDrop {
        fn release(&self) {
            let (released, wake) = &*self.0;
            *released.lock().unwrap_or_else(|poison| poison.into_inner()) = true;
            wake.notify_all();
        }
    }
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            self.release();
        }
    }

    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app) = ready_management();
    let lease = app.lease.unwrap();
    let generation = app.generation;
    let service = app.voice_service.as_ref().unwrap().clone();
    app.submit_voice_management(Operation::Refresh).unwrap();
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(matches!(app.screen, Screen::Voice));
    let before = service.management_snapshot(lease).unwrap();
    let control = app.control.as_ref().unwrap().clone();
    let gate = control.health_gate();
    let released = Arc::new((Mutex::new(false), Condvar::new()));
    // Declared after App: every panic releases the real Writer before App drops.
    let release = ReleaseOnDrop(released.clone());
    let notified = AtomicBool::new(false);
    let (entered, arrived) = mpsc::channel();
    app.voice_service = Some(service.with_execution_fence(Arc::new(move || {
        if gate.status().health == fc_runtime::health::Health::AuthorityUpdating
            && !notified.swap(true, Ordering::AcqRel)
        {
            if entered.send(()).is_err() {
                return false;
            }
            let (released, wake) = &*released;
            let Ok(released) = released.lock() else {
                return false;
            };
            return wake
                .wait_timeout_while(released, Duration::from_secs(2), |released| !*released)
                .is_ok_and(|(released, _)| *released);
        }
        true
    })));
    let operation = || Operation::SetEnabled {
        expected: before.revision,
        expected_identity: before.identity_revision,
        expected_runtime: before.runtime_generation,
        enabled: !before.enabled,
    };
    app.submit_voice_management(operation()).unwrap();
    let original = app.native_command.as_ref().unwrap().clone();
    // Do not pump: the actual Writer reaches its original precommit fence first.
    arrived
        .recv_timeout(Duration::from_secs(5))
        .expect("actual policy Writer did not reach AuthorityUpdating precommit");
    assert_eq!(
        control.health_gate().status().health,
        fc_runtime::health::Health::AuthorityUpdating
    );
    let pair = original
        .snapshot()
        .expect("issued before real policy write");
    let Some(NativeIntent::VoicePolicy(intent)) = original.intent().cloned() else {
        panic!("original policy intent absent")
    };
    assert!(!app.voice_management_allowed(lease));
    app.poll().unwrap();
    assert_eq!(app.lease, Some(lease));
    assert_eq!(app.generation, generation);
    assert!(!control.is_local_locked());
    assert!(app
        .pending
        .as_ref()
        .is_some_and(|p| !p.ticket.is_finished()));
    assert_eq!(app.native_command.as_ref(), Some(&original));
    assert_eq!(original.snapshot(), Some(pair));
    for action in [
        voice_editor::ENABLE,
        voice_editor::DISABLE,
        voice_editor::SAVE_LIMITS,
    ] {
        assert!(
            !app.view()
                .actions
                .iter()
                .find(|candidate| candidate.id == action)
                .unwrap()
                .enabled
        );
    }
    app.submit_voice_management(operation()).unwrap();
    assert_eq!(app.native_command.as_ref(), Some(&original));
    assert_eq!(original.snapshot(), Some(pair));
    assert!(app
        .pending
        .as_ref()
        .is_some_and(|p| !p.ticket.is_finished()));
    release.release();
    wait_for(&mut app, |app| app.pending.is_none());
    assert_eq!(app.lease, Some(lease));
    assert_eq!(app.generation, generation);
    assert!(matches!(app.screen, Screen::Voice));
    assert!(!app.native_unknown);
    assert_eq!(original.snapshot(), Some(pair));
    let committed = service.management_snapshot(lease).unwrap();
    assert_eq!(committed.enabled, !before.enabled);
    assert_eq!(committed.revision, before.revision.next().unwrap());
    assert_eq!(
        committed.publication_revision,
        before.publication_revision.next().unwrap(),
        "one business COMMIT; durable intent acceptance does not advance publication"
    );
    let outcome = service
        .query_policy_outcome_native(lease, pair, intent)
        .unwrap();
    let fc_ports::command_window::TypedCommandOutcome::Completed(
        fc_ports::command_window::TypedCommandResult::VoicePolicy(saved),
    ) = outcome
    else {
        panic!("original typed policy result was not durably completed")
    };
    assert_eq!(saved.enabled, committed.enabled);
    assert_eq!(saved.revision, committed.revision);
    let after_query = service.management_snapshot(lease).unwrap();
    assert_eq!(after_query.revision, committed.revision);
    assert_eq!(after_query.runtime_generation, committed.runtime_generation);
    control.lock_local();
    app.poll_voice_management().unwrap();
    assert!(app.lease.is_none());
    assert!(matches!(app.screen, Screen::Login));
    assert!(service.issue_native_command(lease).is_err());
    drain(app);
}
