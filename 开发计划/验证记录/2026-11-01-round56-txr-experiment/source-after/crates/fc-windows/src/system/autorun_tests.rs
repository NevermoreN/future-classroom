use super::*;
use std::{
    cell::Cell,
    sync::{Mutex, MutexGuard},
};

static TEST_LOCK: Mutex<()> = Mutex::new(());
fn test_lock() -> MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap()
}

struct Scratch {
    scope: RunScope,
    _guard: MutexGuard<'static, ()>,
}
impl Scratch {
    fn new() -> Self {
        let lock = test_lock();
        Self {
            scope: RunScope::scratch_for(PathBuf::from(r"C:\合成 测试\future-classroom.exe"))
                .unwrap(),
            _guard: lock,
        }
    }
    fn run(&self) -> RunAtLogin {
        self.scope.open().unwrap()
    }
    fn raw(&self, kind: REG_VALUE_TYPE, bytes: &[u8]) {
        let run = self.run();
        let key = run.open_key(true, false).unwrap().unwrap();
        // SAFETY: malformed synthetic bytes only in this generated scratch leaf.
        unsafe {
            RegSetValueExW(key.0, w!("FutureClassroom"), None, kind, Some(bytes))
                .ok()
                .unwrap();
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        self.scope.cleanup().unwrap();
    }
}
fn encoded(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[test]
fn quoted_absolute_command_and_isolated_registry_roundtrip() {
    let scratch = Scratch::new();
    let mut run = scratch.run();
    assert_eq!(
        run.expected_command(),
        "\"C:\\合成 测试\\future-classroom.exe\" --autostart"
    );
    assert_eq!(run.observe(), AutostartObservation::Absent);
    let enabled = run.apply(&AutostartObservation::Absent, true, &|| true);
    assert!(enabled.failure.is_none());
    assert!(enabled.mutated);
    assert_eq!(
        enabled.observation,
        AutostartObservation::Command(run.expected_command().into())
    );
    assert!(run.read_back().unwrap().matches_expected);
    let disabled = run.apply(&enabled.observation, false, &|| true);
    assert!(disabled.failure.is_none());
    assert_eq!(disabled.observation, AutostartObservation::Absent);
    assert!(!run.read_back().unwrap().present);
}
#[test]
fn generated_scope_can_open_new_worker_without_moving_a_registry_handle() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RunScope>();
    let scratch = Scratch::new();
    let scope = scratch.scope.clone();
    let result = std::thread::spawn(move || {
        let mut run = scope.open().unwrap();
        run.apply(&AutostartObservation::Absent, true, &|| true)
    })
    .join()
    .unwrap();
    assert!(result.failure.is_none());
    assert!(scratch.run().read_back().unwrap().matches_expected);
}
#[test]
fn production_scope_cleanup_is_refused() {
    let production =
        RunScope::build(RUN_KEY.into(), PathBuf::from(r"C:\test\app.exe"), false).unwrap();
    assert!(!production.is_scratch());
    assert!(production.cleanup().is_err()); // no production registry call made
}
#[test]
fn native_wrong_type_and_oversize_values_are_opaque_and_preserved() {
    let scratch = Scratch::new();
    let values = [
        (REG_BINARY, vec![1, 2, 3, 4]),
        (REG_DWORD, vec![1, 0, 0, 0]),
        (REG_EXPAND_SZ, encoded("%TEMP%\\synthetic.exe")),
        (REG_SZ, encoded(&"x".repeat(MAX_COMMAND_UNITS))),
    ];
    for (kind, bytes) in values {
        scratch.raw(kind, &bytes);
        let mut run = scratch.run();
        let expected = AutostartObservation::Opaque {
            kind: kind.0,
            bytes: bytes.len() as u32,
        };
        assert_eq!(run.observe(), expected);
        let outcome = run.apply(&expected, false, &|| true);
        assert_eq!(outcome.failure, Some(AutostartFailure::InvalidValue));
        assert!(!outcome.mutated);
        assert_eq!(outcome.observation, expected);
        assert_eq!(run.observe(), expected);
    }
}
#[test]
fn registry_decoder_rejects_exact_untrusted_string_bytes_without_normalizing() {
    // RegSetValueExW can normalize an unterminated REG_SZ on the real system.
    // Test the decoder's contract against the exact bytes a query could return,
    // rather than assuming a malformed write survives that native API unchanged.
    for raw in [
        vec![],
        vec![65],                       // odd byte count
        vec![65, 0],                    // missing UTF-16 NUL terminator
        vec![65, 0, 0, 0, 66, 0, 0, 0], // embedded NUL before trailing content
        vec![0, 216, 0, 0],             // unpaired high surrogate
        vec![0, 220, 0, 0],             // unpaired low surrogate
    ] {
        let bytes = raw.len() as u32;
        assert_eq!(
            decode_value(REG_SZ.0, bytes, &raw),
            AutostartObservation::Opaque {
                kind: REG_SZ.0,
                bytes
            },
        );
    }
    // A terminator outside the reported length cannot repair an invalid value.
    assert_eq!(
        decode_value(REG_SZ.0, 2, &[65, 0, 0, 0]),
        AutostartObservation::Opaque {
            kind: REG_SZ.0,
            bytes: 2
        },
    );
    let valid = encoded("合成𠀀");
    assert_eq!(
        decode_value(REG_SZ.0, valid.len() as u32, &valid),
        AutostartObservation::Command("合成𠀀".into()),
    );
}
#[test]
fn moved_executable_requires_reviewed_exact_preimage() {
    let scratch = Scratch::new();
    let mut original = scratch.run();
    let enabled = original.apply(&AutostartObservation::Absent, true, &|| true);
    let moved_scope = RunScope::build(
        scratch.scope.0.key.clone(),
        PathBuf::from(r"C:\moved\future-classroom.exe"),
        true,
    )
    .unwrap();
    let mut moved = moved_scope.open().unwrap();
    assert!(!moved.read_back().unwrap().matches_expected);
    assert_eq!(
        moved
            .apply(&AutostartObservation::Absent, true, &|| true)
            .failure,
        Some(AutostartFailure::PreimageChanged)
    );
    assert_eq!(original.observe(), enabled.observation);
    let repaired = moved.apply(&enabled.observation, true, &|| true);
    assert!(repaired.failure.is_none());
    assert!(moved.read_back().unwrap().matches_expected);
    assert!(!original.read_back().unwrap().matches_expected);
}
#[test]
fn cancellation_before_write_preserves_observed_value() {
    let scratch = Scratch::new();
    let mut run = scratch.run();
    let calls = Cell::new(0);
    let outcome = run.apply(&AutostartObservation::Absent, true, &|| {
        let n = calls.get() + 1;
        calls.set(n);
        n < 2
    });
    assert_eq!(calls.get(), 2);
    assert_eq!(outcome.failure, Some(AutostartFailure::Cancelled));
    assert!(!outcome.mutated);
    assert_eq!(run.observe(), AutostartObservation::Absent);
}
#[test]
fn cancellation_after_write_reports_actual_state_without_rollback() {
    let scratch = Scratch::new();
    let mut run = scratch.run();
    let calls = Cell::new(0);
    let outcome = run.apply(&AutostartObservation::Absent, true, &|| {
        let n = calls.get() + 1;
        calls.set(n);
        n < 4
    });
    assert_eq!(outcome.failure, Some(AutostartFailure::Cancelled));
    assert!(outcome.mutated);
    assert_eq!(
        outcome.observation,
        AutostartObservation::Command(run.expected_command().into())
    );
    assert!(run.read_back().unwrap().matches_expected);
}
#[test]
fn external_change_before_compare_is_not_overwritten() {
    let scratch = Scratch::new();
    let mut run = scratch.run();
    let outcome = run.apply(&AutostartObservation::Absent, true, &|| {
        scratch.raw(REG_SZ, &encoded("different command"));
        true
    });
    assert_eq!(outcome.failure, Some(AutostartFailure::PreimageChanged));
    assert!(!outcome.mutated);
    assert_eq!(
        outcome.observation,
        AutostartObservation::Command("different command".into())
    );
}
#[test]
fn external_change_after_write_reports_mismatch_without_compensating_write() {
    let scratch = Scratch::new();
    let mut run = scratch.run();
    let calls = Cell::new(0);
    let outcome = run.apply(&AutostartObservation::Absent, true, &|| {
        let n = calls.get() + 1;
        calls.set(n);
        if n == 4 {
            scratch.raw(REG_SZ, &encoded("external writer"));
        }
        true
    });
    assert_eq!(outcome.failure, Some(AutostartFailure::ReadbackMismatch));
    assert!(outcome.mutated);
    assert_eq!(
        run.observe(),
        AutostartObservation::Command("external writer".into())
    );
}
#[test]
fn command_validation_rejects_ambiguous_and_overlength_targets() {
    for invalid in [
        r"app.exe",
        r"C:app.exe",
        r"\\server\share\app.exe",
        r"\\?\C:\app.exe",
        r"C:\bad.exe:stream",
        "C:\\bad\".exe",
        r"C:\a\..\b.exe",
        r"C:\a\.\b.exe",
        r"C:\a\\b.exe",
        "C:/app.exe",
        r"C:\app.cmd",
    ] {
        assert!(command_for(Path::new(invalid)).is_err(), "{invalid}");
    }
    let exact = format!("C:\\{}.exe", "a".repeat(238));
    let command = command_for(Path::new(&exact)).unwrap();
    assert_eq!(command.encode_utf16().count(), 259);
    let too_long = format!("C:\\{}.exe", "a".repeat(239));
    assert!(command_for(Path::new(&too_long)).is_err());
    let surrogate_long = format!("C:\\{}.exe", "𠀀".repeat(120));
    assert!(command_for(Path::new(&surrogate_long)).is_err());
}
#[test]
fn access_denial_has_specific_failure_without_leaking_os_error_text() {
    assert_eq!(
        read_failure(ERROR_ACCESS_DENIED),
        AutostartFailure::AccessDenied
    );
    assert_eq!(
        write_failure(ERROR_ACCESS_DENIED),
        AutostartFailure::AccessDenied
    );
    assert_eq!(
        read_failure(ERROR_INVALID_HANDLE),
        AutostartFailure::ReadFailed
    );
    assert_eq!(
        write_failure(ERROR_INVALID_HANDLE),
        AutostartFailure::WriteFailed
    );
}

#[test]
fn unavailable_preimage_never_requests_a_write_guard() {
    let scratch = Scratch::new();
    let mut run = scratch.run();
    let calls = Cell::new(0);
    let outcome = run.apply(
        &AutostartObservation::Unavailable(AutostartFailure::ReadFailed),
        true,
        &|| {
            calls.set(calls.get() + 1);
            true
        },
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(outcome.failure, Some(AutostartFailure::InvalidValue));
    assert!(!outcome.mutated);
    assert_eq!(run.observe(), AutostartObservation::Absent);
}
#[test]
fn explicit_scratch_cleanup_is_idempotent_after_workers_return() {
    let _lock = test_lock();
    let scope = RunScope::scratch_for(PathBuf::from(r"C:\test\app.exe")).unwrap();
    {
        let mut run = scope.open().unwrap();
        assert!(run
            .apply(&AutostartObservation::Absent, true, &|| true)
            .failure
            .is_none());
    }
    scope.cleanup().unwrap();
    scope.cleanup().unwrap();
    assert_eq!(
        scope.open().unwrap().observe(),
        AutostartObservation::Absent
    );
}

#[test]
fn applying_fixed_project_value_preserves_other_values_in_the_same_key() {
    let scratch = Scratch::new();
    let mut run = scratch.run();
    let key = run.open_key(true, false).unwrap().unwrap();
    let sentinel = encoded("unrelated synthetic startup");
    // SAFETY: unrelated synthetic value resides only in our generated test leaf.
    unsafe {
        RegSetValueExW(
            key.0,
            w!("OtherSyntheticStartup"),
            None,
            REG_SZ,
            Some(&sentinel),
        )
        .ok()
        .unwrap();
    }
    let enabled = run.apply(&AutostartObservation::Absent, true, &|| true);
    assert!(enabled.failure.is_none());
    assert!(run
        .apply(&enabled.observation, false, &|| true)
        .failure
        .is_none());
    let mut actual = [0u8; 128];
    let mut bytes = actual.len() as u32;
    // SAFETY: bounded live stack buffer; only the synthetic sentinel is queried.
    unsafe {
        RegQueryValueExW(
            key.0,
            w!("OtherSyntheticStartup"),
            None,
            None,
            Some(actual.as_mut_ptr()),
            Some(&mut bytes),
        )
        .ok()
        .unwrap();
    }
    assert_eq!(&actual[..bytes as usize], sentinel);
}

#[test]
fn cancellation_after_staging_before_commit_rolls_back_enable_and_disable() {
    let scratch = Scratch::new();
    for enabled in [true, false] {
        let mut run = scratch.run();
        let previous = if enabled {
            AutostartObservation::Absent
        } else {
            let saved = run.apply(&AutostartObservation::Absent, true, &|| true);
            assert!(saved.failure.is_none());
            saved.observation
        };
        let calls = Cell::new(0);
        let outcome = run.apply(&previous, enabled, &|| {
            calls.set(calls.get() + 1);
            calls.get() < 3
        });
        assert_eq!(calls.get(), 3);
        assert_eq!(outcome.failure, Some(AutostartFailure::Cancelled));
        assert!(
            !outcome.mutated,
            "a staged write is not a committed mutation"
        );
        assert_eq!(outcome.observation, previous);
        assert_eq!(run.observe(), previous);
    }
}

#[test]
fn original_adapter_deadline_is_not_restarted_by_apply_or_transaction() {
    let scratch = Scratch::new();
    assert!(scratch
        .scope
        .open_until(Instant::now() - Duration::from_secs(1))
        .is_err());
    assert_eq!(transaction::remaining_millis(Instant::now()), None);
    let deadline = Instant::now() + Duration::from_millis(150);
    let mut run = scratch.scope.open_until(deadline).unwrap();
    let calls = Cell::new(0);
    let outcome = run.apply(&AutostartObservation::Absent, true, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            std::thread::sleep(
                (deadline + Duration::from_millis(10)).saturating_duration_since(Instant::now()),
            );
        }
        true
    });
    assert_eq!(run.deadline, deadline);
    assert!(Instant::now() >= deadline);
    assert_eq!(outcome.failure, Some(AutostartFailure::Cancelled));
    assert!(!outcome.mutated);
    assert_eq!(outcome.observation, AutostartObservation::Absent);
    let again = run.apply(&AutostartObservation::Absent, true, &|| true);
    assert_eq!(again.failure, Some(AutostartFailure::Cancelled));
    assert!(!again.mutated);
    assert_eq!(run.observe(), AutostartObservation::Absent);
}

fn ordinary_replacement(scope: &RunScope, command: &str) -> bool {
    let name = HSTRING::from(scope.0.key.as_str());
    let mut key = HKEY::default();
    // SAFETY: another ordinary writer of the same owned synthetic leaf/value.
    let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &name, None, KEY_SET_VALUE, &mut key) };
    if opened != ERROR_SUCCESS {
        return false;
    }
    let changed = unsafe {
        RegSetValueExW(
            key,
            w!("FutureClassroom"),
            None,
            REG_SZ,
            Some(&encoded(command)),
        )
    };
    // SAFETY: close the exact successful ordinary open, even on write conflict.
    assert_eq!(unsafe { RegCloseKey(key) }, ERROR_SUCCESS);
    changed == ERROR_SUCCESS
}

fn transacted_replacement(scope: &RunScope, command: &str, deadline: Instant) -> bool {
    use windows::Win32::Storage::FileSystem::*;
    struct Foreign {
        key: Option<HKEY>,
        transaction: HANDLE,
    }
    impl Drop for Foreign {
        fn drop(&mut self) {
            // SAFETY: only this synthetic competitor's exclusive handles. Rollback
            // is negative and cannot undo an already committed transaction.
            unsafe {
                if let Some(key) = self.key.take() {
                    assert_eq!(RegCloseKey(key), ERROR_SUCCESS);
                }
                let _ = RollbackTransaction(self.transaction);
                CloseHandle(self.transaction).unwrap();
            }
        }
    }
    let Some(timeout) = transaction::remaining_millis(deadline) else {
        return false;
    };
    // SAFETY: independent finite transaction, no inherited handles/extra threads.
    let transaction = unsafe {
        CreateTransaction(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            0,
            0,
            timeout,
            w!("FutureClassroom synthetic competing transaction"),
        )
    }
    .unwrap();
    let mut foreign = Foreign {
        key: None,
        transaction,
    };
    let mut key = HKEY::default();
    let name = HSTRING::from(scope.0.key.as_str());
    let status = unsafe {
        RegOpenKeyTransactedW(
            HKEY_CURRENT_USER,
            &name,
            None,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut key,
            transaction,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return false;
    }
    foreign.key = Some(key);
    let status = unsafe {
        RegSetValueExW(
            key,
            w!("FutureClassroom"),
            None,
            REG_SZ,
            Some(&encoded(command)),
        )
    };
    if status != ERROR_SUCCESS {
        return false;
    }
    assert_eq!(unsafe { RegCloseKey(key) }, ERROR_SUCCESS);
    foreign.key = None;
    if unsafe { CommitTransaction(transaction) }.is_ok() {
        return true;
    }
    let (mut outcome, mut isolation, mut flags, mut timeout) = (0, 0, 0, 0);
    unsafe {
        GetTransactionInformation(
            transaction,
            &mut outcome,
            &mut isolation,
            &mut flags,
            &mut timeout,
            None,
        )
    }
    .unwrap();
    if outcome == TransactionOutcomeUndetermined.0 as u32 {
        // A failed commit is not proof of abort; actually resolve the competitor
        // before deciding what this scratch interleaving demonstrated.
        unsafe { RollbackTransaction(transaction) }.unwrap();
        unsafe {
            GetTransactionInformation(
                transaction,
                &mut outcome,
                &mut isolation,
                &mut flags,
                &mut timeout,
                None,
            )
        }
        .unwrap();
    }
    assert!(
        outcome == TransactionOutcomeCommitted.0 as u32
            || outcome == TransactionOutcomeAborted.0 as u32
    );
    outcome == TransactionOutcomeCommitted.0 as u32
}

#[test]
fn ordinary_competing_replacements_cannot_be_stale_overwritten_at_either_window() {
    competing_replacements(false);
}
#[test]
fn transacted_competing_replacements_cannot_be_stale_overwritten_at_either_window() {
    competing_replacements(true);
}
fn competing_replacements(transacted: bool) {
    for enabled in [false, true] {
        for phase in [2, 3] {
            let scratch = Scratch::new();
            let previous = AutostartObservation::Command("reviewed synthetic previous".into());
            scratch.raw(REG_SZ, &encoded("reviewed synthetic previous"));
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut run = scratch.scope.open_until(deadline).unwrap();
            let calls = Cell::new(0);
            let attempted = Cell::new(false);
            let foreign_committed = Cell::new(false);
            let outcome = run.apply(&previous, enabled, &|| {
                calls.set(calls.get() + 1);
                if calls.get() == phase {
                    attempted.set(true);
                    foreign_committed.set(if transacted {
                        transacted_replacement(
                            &scratch.scope,
                            "external reviewed replacement",
                            deadline,
                        )
                    } else {
                        ordinary_replacement(&scratch.scope, "external reviewed replacement")
                    });
                }
                true
            });
            assert!(attempted.get());
            let after = scratch.run().observe();
            assert_eq!(after, outcome.observation);
            if foreign_committed.get() {
                assert!(
                    outcome.failure.is_some(),
                    "stale preimage must not commit over the replacement"
                );
                assert_eq!(
                    after,
                    AutostartObservation::Command("external reviewed replacement".into())
                );
            } else {
                let desired = if enabled {
                    AutostartObservation::Command(run.expected_command().into())
                } else {
                    AutostartObservation::Absent
                };
                assert!(after == previous || after == desired);
                if outcome.failure.is_none() {
                    assert_eq!(after, desired);
                }
            }
            eprintln!("TXR_SCRATCH_INTERLEAVING transacted={transacted} enabled={enabled} phase={phase} foreign_committed={} own_mutated={} own_failure={:?}",
                foreign_committed.get(), outcome.mutated, outcome.failure);
        }
    }
}

#[test]
fn transacted_staging_never_uses_the_ordinary_observer_before_commit() {
    let scratch = Scratch::new();
    let mut run = scratch.run();
    let calls = Cell::new(0);
    let outcome = run.apply(&AutostartObservation::Absent, true, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == 3 {
            assert_eq!(
                scratch.run().observe(),
                AutostartObservation::Unavailable(AutostartFailure::Unavailable)
            );
        }
        true
    });
    assert_eq!(calls.get(), 4);
    assert!(outcome.failure.is_none());
    assert!(outcome.mutated);
    assert_eq!(
        outcome.observation,
        AutostartObservation::Command(run.expected_command().into())
    );
}

#[test]
fn failed_bound_key_close_retains_exact_owner_and_permanently_blocks_replacement() {
    const CHILD: &str = "FUTURE_CLASSROOM_TXR_CLOSE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let name = std::thread::current().name().unwrap().to_owned();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture", "--test-threads=1"])
            .env(CHILD, "owned-scratch-only")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("owned child exceeded its original 15s deadline");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    }
    let scratch = Scratch::new();
    let permit = transaction::MutationPermit::acquire().unwrap();
    let mut native =
        transaction::NativeTransaction::begin(&permit, Instant::now() + Duration::from_secs(5))
            .unwrap();
    native.open_key(&scratch.scope.0.key, false).unwrap();
    assert!(native.key().is_some());
    native.fail_key_close_for_test();
    assert_eq!(native.close_key(), Err(AutostartFailure::Interrupted));
    let settled = native.settle();
    assert!(!settled.released);
    assert_eq!(settled.outcome, transaction::Outcome::Aborted);
    drop(permit);
    assert_eq!(transaction::retained_for_test(), (true, true, true));
    assert!(transaction::MutationPermit::acquire().is_none());
    let mut run = scratch.run();
    let outcome = run.apply(&AutostartObservation::Absent, true, &|| true);
    assert_eq!(outcome.failure, Some(AutostartFailure::Unavailable));
    assert!(!outcome.mutated);
    assert_eq!(transaction::retained_for_test(), (true, true, true));
    // Actually close the exact retained test handles before deleting this owned
    // registry leaf. This test-only cleanup does not clear the permanent fault.
    assert!(transaction::release_retained_for_test());
    assert_eq!(transaction::retained_for_test(), (true, false, false));
    assert!(transaction::MutationPermit::acquire().is_none());
}

#[test]
fn missing_scratch_key_disable_is_noop_and_enable_creates_only_owned_leaf() {
    let scratch = Scratch::new();
    scratch.scope.cleanup().unwrap();
    let mut run = scratch.run();
    let disabled = run.apply(&AutostartObservation::Absent, false, &|| true);
    assert!(disabled.failure.is_none());
    assert!(!disabled.mutated);
    assert_eq!(disabled.observation, AutostartObservation::Absent);
    let enabled = run.apply(&AutostartObservation::Absent, true, &|| true);
    assert!(enabled.failure.is_none());
    assert!(enabled.mutated);
    assert!(run.read_back().unwrap().matches_expected);
}
