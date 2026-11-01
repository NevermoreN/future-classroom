use super::*;
use std::cell::Cell;

struct Scratch(RunScope);
impl Scratch {
    fn new() -> Self {
        Self(RunScope::scratch_for(PathBuf::from(r"C:\合成 测试\future-classroom.exe")).unwrap())
    }
    fn run(&self) -> RunAtLogin {
        self.0.open().unwrap()
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
        self.0.cleanup().unwrap();
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
    let scope = scratch.0.clone();
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
        scratch.0 .0.key.clone(),
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
        n < 3
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
        if n == 3 {
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
