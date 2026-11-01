use super::*;
use std::cell::Cell;

const THUMBPRINT: &str = "0123456789abcdef0123456789abcdef01234567";

struct Model {
    trace: Vec<&'static str>,
    selected: Result<bool, i32>,
    time: Result<bool, i32>,
    usage: Result<Usage, i32>,
    key: Result<(), i32>,
    chain: Result<ChainObservation, i32>,
    cleanup: Result<(), i32>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            trace: vec![],
            selected: Ok(true),
            time: Ok(true),
            usage: Ok(Usage::Allowed),
            key: Ok(()),
            chain: Ok(ChainObservation {
                trust_errors: 0,
                policy: Ok(0),
            }),
            cleanup: Ok(()),
        }
    }
}

impl QueryDriver for Model {
    fn select(&mut self, _: CertificateThumbprint) -> Result<bool, i32> {
        self.trace.push("select");
        self.selected
    }
    fn time_valid(&mut self) -> Result<bool, i32> {
        self.trace.push("time");
        self.time
    }
    fn usage(&mut self) -> Result<Usage, i32> {
        self.trace.push("usage");
        self.usage
    }
    fn private_key(&mut self) -> Result<(), i32> {
        self.trace.push("key");
        self.key
    }
    fn chain(&mut self, host: &str) -> Result<ChainObservation, i32> {
        assert!(!host.contains('\0'));
        self.trace.push("chain");
        self.chain
    }
    fn close(&mut self) -> Result<(), i32> {
        self.trace.push("close");
        self.cleanup
    }
}

fn request(host: &str) -> CertificateQueryRequest {
    CertificateQueryRequest::new(THUMBPRINT, host).unwrap()
}

#[test]
fn certificate_query_rejects_noncanonical_or_unbounded_input_before_native_work() {
    for host in [
        "",
        "https://board",
        "board:443",
        "Board",
        "board.",
        "a..b",
        "-a",
        "a-",
        "*.example",
        "a/b",
        "a@b",
        "a b",
        "a\0b",
        "白板",
        "[::1]",
        "0:0:0:0:0:0:0:1",
        "127.000.0.1",
        "256.1.1.1",
        "1.2.3.4.",
        "a_b",
        "fe80::1%1",
    ] {
        assert!(
            matches!(
                CertificateQueryRequest::new(THUMBPRINT, host),
                Err(CertificateQueryError::InvalidInput)
            ),
            "{host:?}"
        );
    }
    for host in [
        "board",
        "classroom.local",
        "xn--fiqs8s.example",
        "127.0.0.1",
        "::1",
        "2001:db8::1",
    ] {
        assert_eq!(request(host).host(), host);
    }
    for thumbprint in [
        "",
        "0123",
        "0123456789abcdef0123456789abcdef0123456g",
        " 0123456789abcdef0123456789abcdef01234567",
    ] {
        assert!(CertificateQueryRequest::new(thumbprint, "board").is_err());
    }
    assert!(CertificateQueryRequest::new(THUMBPRINT, &"a".repeat(64)).is_err());
    assert!(CertificateQueryRequest::new(
        THUMBPRINT,
        &format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(62)
        )
    )
    .is_err());
    assert!(CertificateQueryRequest::new(&THUMBPRINT.to_uppercase(), "board").is_ok());
}

#[test]
fn certificate_query_delivers_only_after_original_driver_cleanup_and_keeps_scope() {
    let mut driver = Model::default();
    let report = run_query(&mut driver, &request("board"), &|| true).unwrap();
    assert_eq!(
        driver.trace,
        ["select", "time", "usage", "key", "chain", "close"]
    );
    assert_eq!(report.found.check, CertificateCheck::Satisfied);
    assert_eq!(report.ssl_chain_policy.check, CertificateCheck::Satisfied);
    assert_eq!(report.server_name.check, CertificateCheck::Satisfied);
    assert_eq!(report.scope, CertificateQueryScope::CurrentUserMyCacheOnly);
    // This is a deterministic pipeline model, not Windows/private-key/TLS proof.
}

#[test]
fn certificate_query_missing_or_failed_lookup_never_inspects_another_certificate() {
    for result in [Ok(false), Err(-123)] {
        let mut driver = Model {
            selected: result,
            ..Model::default()
        };
        let report = run_query(&mut driver, &request("board"), &|| true).unwrap();
        assert_eq!(driver.trace, ["select", "close"]);
        assert_eq!(
            report.found.check,
            if result.is_ok() {
                CertificateCheck::Rejected
            } else {
                CertificateCheck::Unverified
            }
        );
        assert_eq!(report.private_key.check, CertificateCheck::NotChecked);
        assert_eq!(report.server_name.check, CertificateCheck::NotChecked);
    }
}

#[test]
fn certificate_query_policy_true_with_error_is_not_policy_or_name_success() {
    for error in [0x800b_010f, 0x800b_0109, 0x800b_0101] {
        let mut driver = Model {
            chain: Ok(ChainObservation {
                trust_errors: 0,
                policy: Ok(error),
            }),
            ..Model::default()
        };
        let report = run_query(&mut driver, &request("board"), &|| true).unwrap();
        assert_eq!(report.ssl_policy_error, Some(error));
        assert_eq!(report.ssl_chain_policy.check, CertificateCheck::Rejected);
        assert_ne!(report.server_name.check, CertificateCheck::Satisfied);
    }
}

#[test]
fn certificate_query_api_failure_cannot_be_treated_like_zero_policy_status() {
    let mut driver = Model {
        chain: Ok(ChainObservation {
            trust_errors: 0,
            policy: Err(-321),
        }),
        ..Model::default()
    };
    let report = run_query(&mut driver, &request("board"), &|| true).unwrap();
    assert_eq!(report.ssl_policy_error, None);
    assert_eq!(report.ssl_chain_policy, CertificateFact::unknown(-321));
    assert_eq!(report.server_name, CertificateFact::unknown(-321));
}

#[test]
fn certificate_query_ip_and_ipv6_name_stay_unverified_even_with_zero_policy_error() {
    for host in ["192.0.2.1", "2001:db8::1", "::1"] {
        let report = run_query(&mut Model::default(), &request(host), &|| true).unwrap();
        assert_eq!(report.server_name.check, CertificateCheck::Unverified);
        assert_eq!(report.ssl_chain_policy.check, CertificateCheck::Satisfied);
    }
}

#[test]
fn certificate_query_cached_revocation_unknown_and_revoked_remain_distinct() {
    for trust_errors in [0x40, 0x0100_0000, 0x0100_0040] {
        let mut driver = Model {
            chain: Ok(ChainObservation {
                trust_errors,
                policy: Ok(0x8009_2013),
            }),
            ..Model::default()
        };
        let report = run_query(&mut driver, &request("board"), &|| true).unwrap();
        assert_eq!(report.cached_revocation.check, CertificateCheck::Unverified);
        assert_eq!(report.ssl_chain_policy.check, CertificateCheck::Unverified);
        assert_ne!(report.server_name.check, CertificateCheck::Satisfied);
    }
    let mut driver = Model {
        chain: Ok(ChainObservation {
            trust_errors: 0x0100_0044,
            policy: Ok(0x800b_010c),
        }),
        ..Model::default()
    };
    let report = run_query(&mut driver, &request("board"), &|| true).unwrap();
    assert_eq!(report.cached_revocation.check, CertificateCheck::Rejected);
    assert_eq!(report.ssl_chain_policy.check, CertificateCheck::Rejected);
    // A revocation-unknown policy error cannot mask the stronger revoked bit.
    let mut driver = Model {
        chain: Ok(ChainObservation {
            trust_errors: 0x44,
            policy: Ok(0x8009_2013),
        }),
        ..Model::default()
    };
    assert_eq!(
        run_query(&mut driver, &request("board"), &|| true)
            .unwrap()
            .ssl_chain_policy
            .check,
        CertificateCheck::Rejected
    );
}

#[test]
fn certificate_query_incomplete_chain_never_promises_cached_revocation_success() {
    let mut driver = Model {
        chain: Ok(ChainObservation {
            trust_errors: 0x10000,
            policy: Ok(0x800b_010a),
        }),
        ..Model::default()
    };
    let report = run_query(&mut driver, &request("board"), &|| true).unwrap();
    assert_eq!(report.cached_revocation.check, CertificateCheck::Unverified);
    assert_eq!(report.ssl_chain_policy.check, CertificateCheck::Rejected);
}

#[test]
fn certificate_query_independent_negative_facts_are_retained_without_skipping_cleanup() {
    let mut driver = Model {
        time: Ok(false),
        usage: Ok(Usage::Rejected),
        key: Err(-42),
        chain: Err(-43),
        ..Model::default()
    };
    let report = run_query(&mut driver, &request("board"), &|| true).unwrap();
    assert_eq!(report.current_time.check, CertificateCheck::Rejected);
    assert_eq!(report.server_auth_eku.check, CertificateCheck::Rejected);
    assert_eq!(report.private_key, CertificateFact::unknown(-42));
    assert_eq!(report.ssl_chain_policy, CertificateFact::unknown(-43));
    assert_eq!(driver.trace.last(), Some(&"close"));
    let mut driver = Model {
        usage: Ok(Usage::Unverified),
        ..Model::default()
    };
    assert_eq!(
        run_query(&mut driver, &request("board"), &|| true)
            .unwrap()
            .server_auth_eku
            .check,
        CertificateCheck::Unverified
    );
}

#[test]
fn certificate_query_cancellation_at_each_checkpoint_closes_exact_original_driver() {
    for cancelled_at in 1..=7 {
        let calls = Cell::new(0);
        let mut driver = Model::default();
        let result = run_query(&mut driver, &request("board"), &|| {
            calls.set(calls.get() + 1);
            calls.get() < cancelled_at
        });
        assert_eq!(result, Err(CertificateQueryError::Cancelled));
        assert_eq!(
            driver
                .trace
                .iter()
                .filter(|event| **event == "close")
                .count(),
            1
        );
        assert_eq!(driver.trace.last(), Some(&"close"));
        if cancelled_at == 1 {
            assert_eq!(driver.trace, ["close"]);
        }
    }
}

#[test]
fn certificate_query_panicking_authority_is_cancelled_and_still_closes() {
    let mut driver = Model::default();
    assert_eq!(
        run_query(&mut driver, &request("board"), &|| panic!(
            "synthetic revoked authority"
        )),
        Err(CertificateQueryError::Cancelled)
    );
    assert_eq!(driver.trace, ["close"]);
}

#[test]
fn certificate_query_release_failure_overrides_success_cancel_and_missing_certificate() {
    for (selected, active) in [(Ok(true), true), (Ok(true), false), (Ok(false), true)] {
        let mut driver = Model {
            selected,
            cleanup: Err(-99),
            ..Model::default()
        };
        assert_eq!(
            run_query(&mut driver, &request("board"), &|| active),
            Err(CertificateQueryError::Cleanup(-99))
        );
        assert_eq!(
            driver
                .trace
                .iter()
                .filter(|event| **event == "close")
                .count(),
            1
        );
    }
}
