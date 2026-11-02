use super::super::tests::{drain, lose_worker_reply, ready_management, wait_for, SERIAL};
use super::super::*;
use super::*;

#[test]
fn lost_policy_reply_current_year_read_is_separate_from_original_typed_history() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app) = ready_management();
    let lease = app.lease.unwrap();
    let backend = app.backend.as_ref().unwrap().clone();
    let year = app.clock.try_now().unwrap().date.year();
    let before = backend
        .lock()
        .unwrap()
        .calendar_status(lease, year)
        .unwrap();
    app.submit_calendar(Operation::Policy {
        year,
        expected: before.policy.revision,
        enabled: false,
    })
    .unwrap();
    let original = app.native_command.as_ref().unwrap().clone();
    lose_worker_reply(&mut app);
    let pair = original.snapshot().unwrap();
    let committed = backend
        .lock()
        .unwrap()
        .calendar_status(lease, year)
        .unwrap();
    assert_ne!(committed.policy.revision, before.policy.revision);
    app.submit(Work::Calendar(Box::new(Job {
        backend: backend.clone(),
        lease: Some(lease),
        operation: Operation::Load(year),
    })))
    .unwrap();
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(
        app.native_unknown,
        "current status is not the original policy result"
    );
    app.submit_calendar(Operation::Load(year)).unwrap();
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(!app.native_unknown);
    assert_eq!(app.native_command.as_ref(), Some(&original));
    assert_eq!(original.snapshot(), Some(pair));
    let after = backend
        .lock()
        .unwrap()
        .calendar_status(lease, year)
        .unwrap();
    assert_eq!(
        after.policy, committed.policy,
        "history query does not apply policy twice"
    );
    drain(app);
}
