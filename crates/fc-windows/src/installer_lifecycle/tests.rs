use super::*;
use std::{
    fs::OpenOptions,
    io::Write,
    os::windows::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

const CHILD: &str = "installer_lifecycle::tests::native_child";
const ENV_LEAF: &str = "FC_LIFECYCLE_TEST_LEAF";
const ENV_MODE: &str = "FC_LIFECYCLE_TEST_MODE";

struct Fixture {
    identity: InstallIdentity,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        use fc_ports::RandomSource;
        let mut identity =
            InstallIdentity::for_current_user().expect("actual current user identity");
        let mut random = [0; 16];
        crate::SystemRandom.fill(&mut random).unwrap();
        let leaf = format!(
            "FutureClassroom-Installer-Test-{}",
            random
                .iter()
                .map(|x| format!("{x:02x}"))
                .collect::<String>()
        );
        let root = identity.local.join(&leaf);
        assert!(!root.exists(), "fresh cryptographically unique test root");
        identity.leaf = Some(leaf);
        // Force real protected creation and checked normal return before the
        // test takes any cleanup authority over this exact fresh namespace.
        let mut initial = LifecycleGate::acquire_startup(&identity).unwrap();
        assert_eq!(initial.resources.root(), root);
        initial.try_release().unwrap();
        Self { identity, root }
    }
    fn child(&self, mode: &str) -> OriginalChild {
        spawn_child(&self.identity, mode)
    }
    fn maintenance(&self) -> MaintenanceOwner {
        LifecycleGate::try_begin_maintenance(
            &self.identity,
            MaintenanceOperation::Repair,
            Instant::now() + Duration::from_secs(20),
        )
        .unwrap()
    }
}
impl Fixture {
    fn cleanup(&self) -> std::io::Result<()> {
        // Exact private test namespace only. No recursive deletion; if cleanup
        // cannot complete while unwinding, preserve the original test failure.
        for leaf in ["ready", "pending.v1", "gate.lock"] {
            let path = self.root.join(leaf);
            match std::fs::symlink_metadata(&path) {
                Ok(meta) => {
                    if !meta.is_file() || meta.file_type().is_symlink() {
                        return Err(std::io::Error::other("unexpected owned test file type"));
                    }
                    std::fs::remove_file(path)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
        }
        std::fs::remove_dir(&self.root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            if std::thread::panicking() {
                eprintln!(
                    "owned fixture cleanup failed; retained {:?}: {error}",
                    self.root
                );
            } else {
                panic!("exact owned fixture cleanup failed: {error}");
            }
        }
    }
}
struct OwnedAlias(PathBuf);
impl Drop for OwnedAlias {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.0) {
            if std::thread::panicking() {
                eprintln!("owned alias cleanup failed: {error}");
            } else {
                panic!("owned alias cleanup failed: {error}");
            }
        }
    }
}
fn spawn_child(identity: &InstallIdentity, mode: &str) -> OriginalChild {
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD, "--nocapture", "--test-threads=1"])
        .env(ENV_LEAF, identity.leaf.as_ref().unwrap())
        .env(ENV_MODE, mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .creation_flags(0x0800_0000)
        .spawn()
        .unwrap();
    OriginalChild {
        child,
        returned: false,
    }
}
struct OriginalChild {
    child: Child,
    returned: bool,
}
impl OriginalChild {
    fn ready(&mut self, root: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !root.join("ready").exists() {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "original child exited before ready"
            );
            assert!(Instant::now() < deadline, "original child ready deadline");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn close_and_wait(&mut self) {
        self.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.returned = true;
                assert!(status.success(), "original child status {status}");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "original child actual-return deadline"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for OriginalChild {
    fn drop(&mut self) {
        if !self.returned {
            self.child.stdin.take();
            // Test failure cleanup only this retained Child, never image/name
            // search. Termination is not test success or lifecycle settlement.
            if self.child.try_wait().expect("owned child poll").is_none() {
                self.child.kill().expect("owned child failure cleanup");
            }
            self.child.wait().expect("actual owned child reap");
        }
    }
}
fn ready(root: &Path) {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("ready"))
        .unwrap();
    f.write_all(b"test-only-ready").unwrap();
    f.sync_all().unwrap();
}
fn wait_stdin() {
    let mut byte = [0];
    use std::io::Read;
    assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
}

#[test]
fn native_child() {
    let Some(leaf) = std::env::var_os(ENV_LEAF) else {
        return;
    };
    let leaf = leaf.into_string().unwrap();
    assert!(
        leaf.starts_with("FutureClassroom-Installer-Test-")
            && leaf.len() == 63
            && leaf[31..].bytes().all(|b| b.is_ascii_hexdigit())
    );
    let mut identity = InstallIdentity::for_current_user().unwrap();
    identity.leaf = Some(leaf);
    let root = identity.local.join(identity.leaf.as_ref().unwrap());
    match std::env::var(ENV_MODE).unwrap().as_str() {
        "shared" => {
            let mut lease = LifecycleGate::acquire_startup(&identity).unwrap();
            ready(&root);
            wait_stdin();
            lease.try_release().unwrap();
            assert!(lease.is_released());
        }
        "blocked" => {
            assert!(matches!(
                LifecycleGate::acquire_startup(&identity),
                Err(LifecycleError::Busy | LifecycleError::Pending)
            ));
        }
        "guardian" => {
            ready(&root);
            wait_stdin();
        }
        "helper-success" => {
            let result = with_startup_lease::<()>(&identity, || {
                ready(&root);
                wait_stdin();
                Ok(())
            });
            assert!(matches!(result, Ok(Ok(()))));
            assert_eq!(native::retained_count(), 0);
        }
        "helper-error" => {
            let result = with_startup_lease(&identity, || Err::<(), _>("synthetic body error"));
            assert!(matches!(result, Ok(Err("synthetic body error"))));
            assert!(native::retained_count() > 0);
            ready(&root); // body already returned, process deliberately alive
            wait_stdin();
        }
        "helper-panic" => {
            // Only this isolated child changes its panic hook. The real unwind
            // must traverse StartupHold and retain the original native lease.
            let old_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {}));
            let result = std::panic::catch_unwind(|| {
                with_startup_lease::<()>(&identity, || panic!("synthetic startup panic"))
            });
            std::panic::set_hook(old_hook);
            assert!(result.is_err());
            assert!(native::retained_count() > 0);
            ready(&root); // caught unwind is not physical process return
            wait_stdin();
        }
        "maintenance-loss" => {
            let _owner = LifecycleGate::try_begin_maintenance(
                &identity,
                MaintenanceOperation::Install,
                Instant::now() + Duration::from_secs(20),
            )
            .unwrap();
            ready(&root);
            wait_stdin();
        }
        "close-fault" => {
            let mut lease = LifecycleGate::acquire_startup(&identity).unwrap();
            lease.resources.inject_close_failure();
            assert_eq!(lease.try_release(), Err(LifecycleError::Faulted));
            assert!(!lease.is_released());
            drop(lease);
            assert!(native::retained_count() > 0);
            assert!(matches!(
                LifecycleGate::acquire_startup(&identity),
                Err(LifecycleError::Faulted)
            ));
            // The process will exit with original native handles retained.
            // No parent cleanup starts before actual child process return.
        }
        "draft-loss" | "draft-close-fault" => {
            let close_fault = std::env::var(ENV_MODE).unwrap() == "draft-close-fault";
            let mut draft = LifecycleGate::try_draft_maintenance(
                &identity,
                Instant::now() + Duration::from_secs(20),
            )
            .unwrap();
            assert!(!root.join("pending.v1").exists());
            let mut guardian = spawn_child(&identity, "guardian");
            guardian.ready(&root);
            let original = draft.watch_guardian(&guardian.child).unwrap();
            if close_fault {
                guardian.close_and_wait();
                draft.inject_close_failure_for_test("guardian");
                let failed_begin =
                    native::DraftAbortObservation::for_test(original, Some((1618, 0)), None, 0);
                assert_eq!(
                    draft.try_abort(Some(failed_begin)),
                    Err(LifecycleError::Faulted)
                );
            } else {
                drop(draft);
                assert!(native::retained_count() > 0);
                assert!(matches!(
                    LifecycleGate::acquire_startup(&identity),
                    Err(LifecycleError::Faulted)
                ));
                // The nested guardian has already made the exact ready file.
                // Keep this original broker process alive for the parent probe.
                wait_stdin();
                guardian.close_and_wait();
                return;
            }
            drop(draft);
            assert!(native::retained_count() > 0);
            assert!(!root.join("pending.v1").exists());
        }
        mode if mode.starts_with("maintenance-fault-") => {
            let role = &mode["maintenance-fault-".len()..];
            assert!(["guardian", "pending", "parent", "gate"].contains(&role));
            let mut owner = LifecycleGate::try_begin_maintenance(
                &identity,
                MaintenanceOperation::Repair,
                Instant::now() + Duration::from_secs(20),
            )
            .unwrap();
            let mut guardian = spawn_child(&identity, "guardian");
            guardian.ready(&root);
            let original = owner.watch_guardian(&guardian.child).unwrap();
            guardian.close_and_wait();
            owner
                .record_settlement(Settlement {
                    operation_id: owner.operation_id(),
                    guardian: original,
                    outcome: SettledOutcome::Commit,
                    join_return: 0,
                    end_return: 0,
                    effects_reconciled: true,
                })
                .unwrap();
            owner.resources.inject_close_failure_at(role);
            assert_eq!(owner.try_release(), Err(LifecycleError::Faulted));
            assert_eq!(owner.resources.producers_returned(), role != "guardian");
            assert!(!owner.is_released());
            assert!(matches!(
                LifecycleGate::acquire_startup(&identity),
                Err(LifecycleError::Faulted)
            ));
            drop(owner);
            assert!(native::retained_count() > 0);
            // No Released receipt. Only this actual process exit returns the
            // remaining original kernel controls. All producer cases differ:
            // a failed producer close leaves Pending not delete-pending.
        }
        _ => panic!("unknown fixed child role"),
    }
}

#[test]
fn physical_shared_child_blocks_maintenance_until_actual_return() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut child = fixture.child("shared");
    child.ready(&fixture.root);
    let mut second = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
    assert!(matches!(
        LifecycleGate::try_begin_maintenance(
            &fixture.identity,
            MaintenanceOperation::Upgrade,
            Instant::now() + Duration::from_secs(20)
        ),
        Err(LifecycleError::Busy)
    ));
    second.try_release().unwrap();
    child.close_and_wait();
    let owner = fixture.maintenance();
    drop(owner); // leaves negative Pending
    assert!(matches!(
        LifecycleGate::acquire_startup(&fixture.identity),
        Err(LifecycleError::Pending)
    ));
}

#[test]
fn physical_maintenance_blocks_child_and_owner_loss_keeps_pending() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut owner = fixture.child("maintenance-loss");
    owner.ready(&fixture.root);
    let mut blocked = fixture.child("blocked");
    blocked.close_and_wait();
    owner.close_and_wait();
    // Original process truly returned; share handle is gone but persistent
    // Pending still rejects startup and a fresh maintenance owner.
    assert!(matches!(
        LifecycleGate::acquire_startup(&fixture.identity),
        Err(LifecycleError::Pending)
    ));
    assert!(matches!(
        LifecycleGate::try_begin_maintenance(
            &fixture.identity,
            MaintenanceOperation::Repair,
            Instant::now() + Duration::from_secs(20)
        ),
        Err(LifecycleError::Pending)
    ));
}

#[test]
fn receipt_before_actual_guardian_exit_cannot_clear_pending() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut owner = fixture.maintenance();
    let mut guardian = fixture.child("guardian");
    guardian.ready(&fixture.root);
    let actual = owner.watch_guardian(&guardian.child).unwrap();
    let receipt = Settlement {
        operation_id: owner.operation_id(),
        guardian: actual,
        outcome: SettledOutcome::Commit,
        join_return: 0,
        end_return: 0,
        effects_reconciled: true,
    };
    assert_eq!(
        owner.record_settlement(receipt),
        Err(LifecycleError::GuardianRunning)
    );
    assert_eq!(owner.try_release(), Err(LifecycleError::Unsettled));
    assert!(fixture.root.join("pending.v1").exists());
    guardian.close_and_wait();
    let mut wrong = receipt;
    wrong.guardian.creation_filetime += 1;
    assert_eq!(
        owner.record_settlement(wrong),
        Err(LifecycleError::Unsettled)
    );
    wrong = receipt;
    wrong.operation_id = OperationId([0; 16]);
    assert_eq!(
        owner.record_settlement(wrong),
        Err(LifecycleError::Unsettled)
    );
    wrong = receipt;
    wrong.effects_reconciled = false;
    assert_eq!(
        owner.record_settlement(wrong),
        Err(LifecycleError::Unsettled)
    );
    owner.record_settlement(receipt).unwrap();
    owner.try_release().unwrap();
    assert!(owner.is_released());
    assert!(!fixture.root.join("pending.v1").exists());
    let mut startup = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
    startup.try_release().unwrap();
}

#[test]
fn controlled_draft_blocks_startup_without_creating_pending_and_checked_aborts() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut draft = LifecycleGate::try_draft_maintenance(
        &fixture.identity,
        Instant::now() + Duration::from_secs(20),
    )
    .unwrap();
    assert!(!fixture.root.join("pending.v1").exists());
    assert!(matches!(
        LifecycleGate::acquire_startup(&fixture.identity),
        Err(LifecycleError::Busy)
    ));
    assert!(matches!(
        draft.arm(&fixture.identity),
        Err(LifecycleError::Unsettled)
    ));
    draft.expire_for_test();
    assert!(matches!(
        draft.arm(&fixture.identity),
        Err(LifecycleError::Expired)
    ));
    // Negative cleanup does not renew or issue an installation permit.
    draft.try_abort(None).unwrap();
    let mut startup = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
    startup.try_release().unwrap();
}

#[test]
fn controlled_failed_begin_requires_original_guardian_exit_before_draft_abort() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut draft = LifecycleGate::try_draft_maintenance(
        &fixture.identity,
        Instant::now() + Duration::from_secs(20),
    )
    .unwrap();
    let mut guardian = fixture.child("guardian");
    guardian.ready(&fixture.root);
    let actual = draft.watch_guardian(&guardian.child).unwrap();
    // These are explicitly synthetic native-return observations; actual OS gate
    // and original process objects are exercised. No MSI success is claimed.
    assert_eq!(
        draft.observe_begin(native::BeginObservation::for_test(actual, 1618, 0)),
        Err(LifecycleError::Unsettled)
    );
    assert!(matches!(
        draft.arm(&fixture.identity),
        Err(LifecycleError::Unsettled)
    ));
    assert_eq!(draft.try_abort(None), Err(LifecycleError::Unsettled));
    let abort = native::DraftAbortObservation::for_test(actual, Some((1618, 0)), None, 0);
    assert_eq!(
        draft.try_abort(Some(abort)),
        Err(LifecycleError::GuardianRunning)
    );
    assert!(!fixture.root.join("pending.v1").exists());
    guardian.close_and_wait();
    draft.try_abort(Some(abort)).unwrap();
    let mut startup = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
    startup.try_release().unwrap();
}

#[test]
fn controlled_begin_draft_expiry_requires_real_empty_end_observation_and_actual_exit() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut draft = LifecycleGate::try_draft_maintenance(
        &fixture.identity,
        Instant::now() + Duration::from_secs(20),
    )
    .unwrap();
    let mut guardian = fixture.child("guardian");
    guardian.ready(&fixture.root);
    let actual = draft.watch_guardian(&guardian.child).unwrap();
    let mut wrong = actual;
    wrong.creation_filetime += 1;
    assert_eq!(
        draft.observe_begin(native::BeginObservation::for_test(wrong, 0, 31)),
        Err(LifecycleError::Unsettled)
    );
    draft
        .observe_begin(native::BeginObservation::for_test(actual, 0, 31))
        .unwrap();
    draft.expire_for_test();
    assert!(matches!(
        draft.arm(&fixture.identity),
        Err(LifecycleError::Expired)
    ));
    guardian.close_and_wait();
    for receipt in [
        native::DraftAbortObservation::for_test(actual, None, None, 0),
        native::DraftAbortObservation::for_test(actual, Some((0, 31)), None, 0),
        native::DraftAbortObservation::for_test(actual, Some((0, 31)), Some(1603), 0),
        native::DraftAbortObservation::for_test(actual, Some((0, 31)), Some(0), 6),
        native::DraftAbortObservation::for_test(actual, Some((0, 32)), Some(0), 0),
    ] {
        assert_eq!(
            draft.try_abort(Some(receipt)),
            Err(LifecycleError::Unsettled)
        );
    }
    assert!(!fixture.root.join("pending.v1").exists());
    draft
        .try_abort(Some(native::DraftAbortObservation::for_test(
            actual,
            Some((0, 31)),
            Some(0),
            0,
        )))
        .unwrap();
    let mut startup = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
    startup.try_release().unwrap();
}

#[test]
fn controlled_arm_uses_original_tag5_and_cannot_settle_with_synthetic_join0() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut draft = LifecycleGate::try_draft_maintenance(
        &fixture.identity,
        Instant::now() + Duration::from_secs(20),
    )
    .unwrap();
    let mut guardian = fixture.child("guardian");
    guardian.ready(&fixture.root);
    let actual = draft.watch_guardian(&guardian.child).unwrap();
    draft
        .observe_begin(native::BeginObservation::for_test(actual, 0, 41))
        .unwrap();
    assert!(!fixture.root.join("pending.v1").exists());
    let mut owner = draft.arm(&fixture.identity).unwrap();
    assert_eq!(
        owner.transaction_origin,
        TransactionOrigin::OwnedByBegin { transaction_id: 41 }
    );
    assert!(fixture.root.join("pending.v1").exists());
    guardian.close_and_wait();
    let legacy = Settlement {
        operation_id: owner.operation_id(),
        guardian: actual,
        outcome: SettledOutcome::Rollback,
        join_return: 0,
        end_return: 0,
        effects_reconciled: true,
    };
    assert_eq!(
        owner.record_settlement(legacy),
        Err(LifecycleError::Unsettled)
    );
    let origin = native::TransactionOwnership::OwnedByBegin {
        begin_return: 0,
        transaction_id: 41,
    };
    for receipt in [
        native::TransactionObservation::for_test(
            owner.operation_id(),
            actual,
            native::TransactionOwnership::OwnedByJoin { join_return: 0 },
            SettledOutcome::Rollback,
            1603,
            0,
            0,
            true,
        ),
        native::TransactionObservation::for_test(
            owner.operation_id(),
            actual,
            native::TransactionOwnership::OwnedByBegin {
                begin_return: 0,
                transaction_id: 42,
            },
            SettledOutcome::Rollback,
            1603,
            0,
            0,
            true,
        ),
        native::TransactionObservation::for_test(
            owner.operation_id(),
            actual,
            origin,
            SettledOutcome::Rollback,
            1603,
            1603,
            0,
            true,
        ),
        native::TransactionObservation::for_test(
            owner.operation_id(),
            actual,
            origin,
            SettledOutcome::Rollback,
            1603,
            0,
            6,
            true,
        ),
        native::TransactionObservation::for_test(
            owner.operation_id(),
            actual,
            origin,
            SettledOutcome::Commit,
            1603,
            0,
            0,
            true,
        ),
        native::TransactionObservation::for_test(
            owner.operation_id(),
            actual,
            origin,
            SettledOutcome::Rollback,
            1603,
            0,
            0,
            false,
        ),
    ] {
        assert_eq!(
            owner.record_controlled_settlement(receipt),
            Err(LifecycleError::Unsettled)
        );
    }
    assert_eq!(owner.try_release(), Err(LifecycleError::Unsettled));
    owner
        .record_controlled_settlement(native::TransactionObservation::for_test(
            owner.operation_id(),
            actual,
            origin,
            SettledOutcome::Rollback,
            1603,
            0,
            0,
            true,
        ))
        .unwrap();
    owner.try_release().unwrap();
    assert!(!fixture.root.join("pending.v1").exists());
    let mut startup = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
    startup.try_release().unwrap();
}

#[test]
fn controlled_unknown_original_draft_owner_retains_gate_until_actual_process_exit() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut child = fixture.child("draft-loss");
    child.ready(&fixture.root);
    assert!(!fixture.root.join("pending.v1").exists());
    assert!(matches!(
        LifecycleGate::acquire_startup(&fixture.identity),
        Err(LifecycleError::Busy)
    ));
    child.close_and_wait();
    let mut startup = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
    startup.try_release().unwrap();
}

#[test]
fn controlled_draft_checked_close_failure_has_no_pending_and_no_release_until_exit() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut child = fixture.child("draft-close-fault");
    child.close_and_wait();
    assert!(!fixture.root.join("pending.v1").exists());
    let mut startup = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
    startup.try_release().unwrap();
}

#[test]
fn malformed_or_torn_pending_is_negative_not_dead_owner_recovery() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    for bytes in [b"".as_slice(), b"FC-INSTALL-PENDING", b"not a valid record"] {
        std::fs::write(fixture.root.join("pending.v1"), bytes).unwrap();
        assert!(matches!(
            LifecycleGate::acquire_startup(&fixture.identity),
            Err(LifecycleError::Pending)
        ));
        assert!(matches!(
            LifecycleGate::try_begin_maintenance(
                &fixture.identity,
                MaintenanceOperation::Repair,
                Instant::now() + Duration::from_secs(20)
            ),
            Err(LifecycleError::Pending)
        ));
        std::fs::remove_file(fixture.root.join("pending.v1")).unwrap(); // private synthetic test reset only
    }
}

#[test]
fn original_expiry_never_renews_or_clears_negative_state() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut owner = fixture.maintenance();
    owner.deadline = Instant::now();
    assert_eq!(owner.try_release(), Err(LifecycleError::Expired));
    drop(owner);
    assert!(matches!(
        LifecycleGate::acquire_startup(&fixture.identity),
        Err(LifecycleError::Pending)
    ));
}

#[test]
fn close_failure_retains_physical_owner_in_child_without_replacement() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut child = fixture.child("close-fault");
    child.close_and_wait();
    // Only actual process exit returns the intentionally retained test handles.
    let mut lease = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
    lease.try_release().unwrap();
}

#[test]
fn coordination_rejects_gate_hardlink_alias_before_pending_or_school_data() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let alias = fixture.root.join("gate-alias");
    std::fs::hard_link(fixture.root.join("gate.lock"), &alias).unwrap();
    let _owned_alias = OwnedAlias(alias);
    assert!(LifecycleGate::acquire_startup(&fixture.identity).is_err());
    assert!(LifecycleGate::try_begin_maintenance(
        &fixture.identity,
        MaintenanceOperation::Install,
        Instant::now() + Duration::from_secs(20)
    )
    .is_err());
    assert!(!fixture.root.join("pending.v1").exists());
}

#[test]
fn existing_broad_gate_dacl_is_rejected_without_repair_or_pending() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    native::make_unprotected_for_test(&fixture.root.join("gate.lock")).unwrap();
    assert!(LifecycleGate::acquire_startup(&fixture.identity).is_err());
    assert!(LifecycleGate::try_begin_maintenance(
        &fixture.identity,
        MaintenanceOperation::Install,
        Instant::now() + Duration::from_secs(20),
    )
    .is_err());
    assert!(!fixture.root.join("pending.v1").exists());
}

#[test]
fn isolated_parent_delete_child_grant_is_rejected() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    assert!(native::isolated_delete_child_acl_is_rejected(&fixture.root).unwrap());
    // This modifies only the fresh private test root, never real LocalAppData.
    assert!(!fixture.root.join("pending.v1").exists());
}

#[test]
fn producer_close_failure_remains_negative_after_original_process_exit() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut child = fixture.child("maintenance-fault-guardian");
    child.close_and_wait();
    assert!(fixture.root.join("pending.v1").exists());
    assert!(matches!(
        LifecycleGate::acquire_startup(&fixture.identity),
        Err(LifecycleError::Pending)
    ));
}

#[test]
fn settled_control_close_failure_never_releases_but_actual_exit_returns_same_gate() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    for role in ["pending", "parent", "gate"] {
        let fixture = Fixture::new();
        let mut child = fixture.child(&format!("maintenance-fault-{role}"));
        child.close_and_wait();
        // Child first proved exact settlement, actual guardian exit0 and checked
        // return of all producers. It stayed FAULTED/not Released while alive.
        // Only actual original process death returned the SAME kernel gate and
        // terminal Pending controls; this is not permission after unknown End.
        let mut lease = LifecycleGate::acquire_startup(&fixture.identity).unwrap();
        lease.try_release().unwrap();
        assert!(!fixture.root.join("pending.v1").exists());
    }
}

#[test]
fn os_parent_owner_trust_does_not_relax_exact_private_owner() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    native::owner_boundary_for_test().unwrap();
}

#[test]
fn common_startup_helper_does_not_enter_body_under_maintenance() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let _maintenance = fixture.maintenance();
    let entered = std::cell::Cell::new(false);
    let result = with_startup_lease(&fixture.identity, || {
        entered.set(true);
        std::fs::create_dir(fixture.root.join("simulated-data"))
    });
    // Remove only this exact private synthetic directory if the regression
    // unexpectedly entered the body; preserve the actual admission failure.
    let wrong_entry = entered.get();
    let created = fixture.root.join("simulated-data").exists();
    if created {
        std::fs::remove_dir(fixture.root.join("simulated-data")).unwrap();
    }
    assert!(!wrong_entry);
    assert!(!created);
    assert!(matches!(result, Err(LifecycleError::Busy)));
}

#[test]
fn common_startup_helper_holds_during_success_body_until_checked_return() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut child = fixture.child("helper-success");
    child.ready(&fixture.root);
    assert!(matches!(
        LifecycleGate::try_begin_maintenance(
            &fixture.identity,
            MaintenanceOperation::Upgrade,
            Instant::now() + Duration::from_secs(20),
        ),
        Err(LifecycleError::Busy)
    ));
    child.close_and_wait();
    let owner = fixture.maintenance();
    drop(owner);
}

#[test]
fn common_startup_helper_retains_after_error_until_actual_process_exit() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut child = fixture.child("helper-error");
    child.ready(&fixture.root);
    assert!(matches!(
        LifecycleGate::try_begin_maintenance(
            &fixture.identity,
            MaintenanceOperation::Repair,
            Instant::now() + Duration::from_secs(20),
        ),
        Err(LifecycleError::Busy)
    ));
    child.close_and_wait();
    let owner = fixture.maintenance();
    drop(owner);
}

#[test]
fn common_startup_helper_retains_after_caught_panic_until_actual_process_exit() {
    let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let mut child = fixture.child("helper-panic");
    child.ready(&fixture.root);
    assert!(matches!(
        LifecycleGate::try_begin_maintenance(
            &fixture.identity,
            MaintenanceOperation::Uninstall,
            Instant::now() + Duration::from_secs(20),
        ),
        Err(LifecycleError::Busy)
    ));
    child.close_and_wait();
    let owner = fixture.maintenance();
    drop(owner);
}
