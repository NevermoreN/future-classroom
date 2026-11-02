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
