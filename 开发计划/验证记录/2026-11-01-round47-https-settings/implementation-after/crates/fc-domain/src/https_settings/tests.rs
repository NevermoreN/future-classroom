use super::*;
#[test]
fn canonical_dns_ip_and_certificate_identity_do_not_claim_readiness() {
    for (input, expected) in [
        ("Board.School.", "board.school"),
        ("BOARD", "board"),
        ("xn--fiqs8s.example", "xn--fiqs8s.example"),
        ("192.168.10.212", "192.168.10.212"),
        ("[2001:0db8::1]", "2001:db8::1"),
    ] {
        assert_eq!(HttpsHost::parse(input).unwrap().as_str(), expected);
    }
    let cert = HttpsCertificateRef::parse(&"Ab".repeat(20)).unwrap();
    assert_eq!(cert.to_hex(), "ab".repeat(20));
    let c = HttpsConfiguration::new(HttpsHost::parse("::1").unwrap(), 8443, cert).unwrap();
    assert_eq!(c.origin(), "https://[::1]:8443");
    assert_eq!(HttpsSettings::default().configuration, None);
}
#[test]
fn host_path_wildcard_unspecified_ambiguous_ip_and_invalid_port_reject() {
    for host in [
        "",
        "https://school",
        "user@school",
        "school/path",
        "school:443",
        "*.school",
        " school",
        "school ",
        "a..b",
        "-a",
        "a-",
        "0.0.0.0",
        "::",
        "[::]",
        "255.255.255.255",
        "224.0.0.1",
        "ff02::1",
        "127.1",
        "0127.0.0.1",
        "123",
        "中文",
        "a\\b",
        "a\0b",
        "[127.0.0.1]x",
    ] {
        assert!(HttpsHost::parse(host).is_err(), "{host:?}");
    }
    assert!(HttpsHost::parse(&"a".repeat(64)).is_err());
    assert!(HttpsCertificateRef::parse(&"a".repeat(39)).is_err());
    assert!(HttpsCertificateRef::parse(&"g".repeat(40)).is_err());
    assert!(HttpsConfiguration::new(
        HttpsHost::parse("school").unwrap(),
        0,
        HttpsCertificateRef::parse(&"a".repeat(40)).unwrap()
    )
    .is_err());
}
#[test]
fn configured_save_and_clear_have_independent_cas_no_default_port() {
    let old = HttpsSettings::default();
    let config = HttpsConfiguration::new(
        HttpsHost::parse("school").unwrap(),
        65535,
        HttpsCertificateRef::parse(&"a".repeat(40)).unwrap(),
    )
    .unwrap();
    let first = old.transition(Some(config), old.revision).unwrap();
    assert_eq!(first.revision.get(), 1);
    assert!(matches!(
        first.transition(None, old.revision),
        Err(HttpsSettingsError::RevisionConflict { .. })
    ));
    assert_eq!(
        first.transition(None, first.revision).unwrap(),
        HttpsSettings {
            revision: Revision::new(2),
            configuration: None
        }
    );
}
