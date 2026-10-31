use super::*;
use fc_ports::http::{TokenIssuer, CHUNK_BYTES};
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const LARGE: &str = "e3c33f1a7c00a23610a13fa6b862df2882931be5b0262aeada7a47e282f1c679";
fn manifest() -> Vec<u8> {
    format!(r#"{{"schemaVersion":1,"apiVersion":1,"uiCompatibilityVersion":1,"assets":[{{"path":"index.html","sizeBytes":3,"sha256":"{ABC}"}},{{"path":"assets/app-01234567.js","sizeBytes":131089,"sha256":"{LARGE}"}}]}}"#).into_bytes()
}
fn payloads() -> Vec<(String, Vec<u8>)> {
    vec![
        ("index.html".into(), b"abc".to_vec()),
        (
            "assets/app-01234567.js".into(),
            vec![b'x'; 2 * CHUNK_BYTES + 17],
        ),
    ]
}
fn assets() -> StaticAssets {
    StaticAssets::new(
        &ListenPrefix::https("https://board.synthetic:443/fc/").unwrap(),
        &manifest(),
        payloads(),
    )
    .unwrap()
}
fn request(target: &str) -> RequestHead {
    RequestHead {
        token: TokenIssuer::new().unwrap().issue(0).unwrap(),
        method: "GET".into(),
        target: target.into(),
        headers: vec![Header {
            name: "Host".into(),
            value: b"board.synthetic".to_vec(),
        }],
        tls: true,
        content_length: None,
        has_body: false,
    }
}
#[test]
fn verified_payload_mapping_etag_and_chunk_ownership() {
    let assets = assets();
    let mut first = assets.dispatch(&request("/fc/"), b"").unwrap();
    assert_eq!(first.head.status, 200);
    first.head.validate().unwrap();
    assert_eq!(first.body.next_chunk(), Some(b"abc".as_slice()));
    assert_eq!(first.body.next_chunk(), None);
    assert!(first
        .head
        .headers
        .iter()
        .any(|h| h.name == "Cache-Control" && h.value == b"no-cache"));
    assert!(first
        .head
        .headers
        .iter()
        .any(|h| h.name == "Content-Security-Policy"
            && h.value == CONTENT_SECURITY_POLICY.as_bytes()));
    let mut head = request("/fc/index.html");
    head.headers.push(Header {
        name: "If-None-Match".into(),
        value: format!("\"{ABC}\"").into_bytes(),
    });
    let mut cached = assets.dispatch(&head, b"").unwrap();
    assert_eq!(cached.head.status, 304);
    assert_eq!(cached.head.content_length, 0);
    assert!(cached.body.next_chunk().is_none());
    let mut left = assets
        .dispatch(&request("/fc/assets/app-01234567.js"), b"")
        .unwrap();
    let mut right = assets
        .dispatch(&request("/fc/assets/app-01234567.js"), b"")
        .unwrap();
    assert!(Arc::ptr_eq(&left.body.bytes, &right.body.bytes));
    let mut sizes = vec![];
    while let Some(chunk) = left.body.next_chunk() {
        assert!(chunk.iter().all(|b| *b == b'x'));
        sizes.push(chunk.len());
    }
    assert_eq!(sizes, vec![CHUNK_BYTES, CHUNK_BYTES, 17]);
    assert_eq!(right.body.next_chunk().unwrap().len(), CHUNK_BYTES);
}
#[test]
fn assets_never_fall_back_for_api_traversal_bad_headers_or_wrong_transport() {
    let assets = assets();
    for target in [
        "/fc/api/v1/status",
        "/fc/assets/missing.js",
        "/fc/../index.html",
        "/fc/%69ndex.html",
        "/fc/index.html?secret=abc",
        "/fc//index.html",
        "/fc",
        "/other/index.html",
    ] {
        assert!(assets.dispatch(&request(target), b"").is_err(), "{target}");
    }
    let mut head = request("/fc/");
    head.tls = false;
    assert!(matches!(
        assets.dispatch(&head, b""),
        Err(StaticError::Boundary(WebError::Transport))
    ));
    head.tls = true;
    head.method = "POST".into();
    assert!(matches!(
        assets.dispatch(&head, b""),
        Err(StaticError::Boundary(WebError::Method))
    ));
    head.method = "GET".into();
    head.headers.push(Header {
        name: "Origin".into(),
        value: b"https://evil.synthetic".to_vec(),
    });
    assert!(matches!(
        assets.dispatch(&head, b""),
        Err(StaticError::Boundary(WebError::Origin))
    ));
    assert!(matches!(
        assets.dispatch(&request("/fc/"), b"unclaimed-body"),
        Err(StaticError::Boundary(WebError::Body))
    ));
    let mut duplicate = request("/fc/");
    duplicate.headers.extend([
        Header {
            name: "If-None-Match".into(),
            value: vec![],
        },
        Header {
            name: "if-none-match".into(),
            value: vec![],
        },
    ]);
    assert!(matches!(
        assets.dispatch(&duplicate, b""),
        Err(StaticError::Boundary(WebError::Headers))
    ));
}
#[test]
fn missing_extra_duplicate_and_corrupt_payloads_fail_closed_at_load() {
    let prefix = ListenPrefix::https("https://board.synthetic:443/fc/").unwrap();
    let mut missing = payloads();
    missing.pop();
    let mut extra = payloads();
    extra.push(("secret.db".into(), b"abc".to_vec()));
    let mut duplicate = payloads();
    duplicate.push(("index.html".into(), b"abc".to_vec()));
    let mut corrupt = payloads();
    corrupt[0].1 = b"abd".to_vec();
    for payload in [missing, extra, duplicate, corrupt] {
        assert!(matches!(
            StaticAssets::new(&prefix, &manifest(), payload),
            Err(StaticError::PayloadMismatch)
        ));
    }
}

#[test]
fn conditional_get_uses_weak_comparison_lists_and_wildcard() {
    let assets = assets();
    for value in [
        format!("W/\"{ABC}\""),
        format!("\"other,tag\", W/\"{ABC}\""),
        format!(" , , \"{ABC}\" , "),
        "*".into(),
        " \t* \t".into(),
    ] {
        let mut head = request("/fc/");
        head.headers.push(Header {
            name: "If-None-Match".into(),
            value: value.into_bytes(),
        });
        let mut response = assets.dispatch(&head, b"").unwrap();
        assert_eq!(response.head.status, 304);
        assert_eq!(response.head.content_length, 0); // wire body length, not representation length
        assert!(response.body.next_chunk().is_none());
    }
    for (value, valid) in [
        ("\"other,tag\"", true),
        ("W/\"unknown\"", true),
        ("\"unterminated", false),
        ("*, \"tag\"", false),
        ("\"tag\"junk", false),
        ("bare", false),
    ] {
        let mut head = request("/fc/");
        head.headers.push(Header {
            name: "If-None-Match".into(),
            value: value.as_bytes().to_vec(),
        });
        match assets.dispatch(&head, b"") {
            Ok(response) => {
                assert!(valid);
                assert_eq!(response.head.status, 200);
            }
            Err(StaticError::Boundary(WebError::Headers)) => assert!(!valid),
            _ => panic!("unexpected conditional GET result"),
        }
    }
}

#[test]
fn allocation_counts_the_shared_bundle_once_and_each_response_only_owns_a_cursor() {
    let assets = assets();
    assert_eq!(assets.asset_count(), 2);
    let measured = assets.allocation_bytes();
    let payload = 3 + 2 * CHUNK_BYTES + 17;
    assert!(measured >= payload + size_of::<StaticAssets>());
    assert!(
        measured < payload + 16 * 1024,
        "metadata must not duplicate the large payload"
    );
    let mut first = assets
        .dispatch(&request("/fc/assets/app-01234567.js"), b"")
        .unwrap();
    let second = assets
        .dispatch(&request("/fc/assets/app-01234567.js"), b"")
        .unwrap();
    assert!(Arc::ptr_eq(&first.body.bytes, &second.body.bytes));
    assert_eq!(first.body.owned_bytes(), size_of::<StaticBody>());
    assert_eq!(first.body.remaining_bytes(), 2 * CHUNK_BYTES + 17);
    first.body.next_chunk().unwrap();
    assert_eq!(first.body.remaining_bytes(), CHUNK_BYTES + 17);
    assert_eq!(second.body.remaining_bytes(), 2 * CHUNK_BYTES + 17);
    assert_eq!(assets.allocation_bytes(), measured);
    let mut head = request("/fc/assets/app-01234567.js");
    head.headers.push(Header {
        name: "If-None-Match".into(),
        value: b"*".to_vec(),
    });
    let mut cached = assets.dispatch(&head, b"").unwrap();
    assert_eq!(cached.head.status, 304);
    assert!(Arc::ptr_eq(&cached.body.bytes, &first.body.bytes));
    assert_eq!(cached.body.remaining_bytes(), 0);
    assert_eq!(cached.body.owned_bytes(), size_of::<StaticBody>());
    assert!(cached.body.next_chunk().is_none());
}
#[test]
fn configured_static_root_and_real_css_hash_keep_api_separate_and_cache_consistent() {
    let prefix = ListenPrefix::https("https://board.synthetic:443/classroom/teacher/").unwrap();
    let manifest = format!(
        r#"{{"schemaVersion":1,"apiVersion":1,"uiCompatibilityVersion":1,"assets":[{{"path":"index.html","sizeBytes":3,"sha256":"{ABC}"}},{{"path":"assets/index-DFFVdc8-.css","sizeBytes":3,"sha256":"{ABC}"}}]}}"#
    );
    let assets = StaticAssets::new(
        &prefix,
        manifest.as_bytes(),
        vec![
            ("index.html".into(), b"abc".to_vec()),
            ("assets/index-DFFVdc8-.css".into(), b"abc".to_vec()),
        ],
    )
    .unwrap();
    assert!(assets.is_api_target("/classroom/teacher/api/v1/status"));
    assert!(assets.is_api_target("/classroom/teacher/api?bad"));
    assert!(!assets.is_api_target("/fc/api/v1/status"));
    assert!(!assets.is_api_target("/classroom/teacher/assets/index-DFFVdc8-.css"));
    let target = "/classroom/teacher/assets/index-DFFVdc8-.css";
    let head = request(target);
    let first = assets.dispatch(&head, b"").unwrap();
    assert_eq!(first.head.status, 200);
    assert!(first
        .head
        .headers
        .iter()
        .any(|h| h.name == "Cache-Control" && h.value == b"public, max-age=31536000, immutable"));
    assert!(first
        .head
        .headers
        .iter()
        .any(|h| h.name == "Content-Security-Policy"
            && h.value == CONTENT_SECURITY_POLICY.as_bytes()));
    let mut cached = head;
    cached.headers.push(Header {
        name: "If-None-Match".into(),
        value: format!("W/\"{ABC}\"").into_bytes(),
    });
    let response = assets.dispatch(&cached, b"").unwrap();
    assert_eq!(response.head.status, 304);
    assert_eq!(response.head.content_length, 0);
    assert_eq!(response.body.remaining_bytes(), 0);
    assert!(matches!(
        assets.dispatch(&request("/classroom/teacher/api/v1/unknown"), b""),
        Err(StaticError::Boundary(WebError::NotFound))
    ));
}

#[test]
fn trusted_startup_rebinds_host_and_origin_without_copying_verified_payloads() {
    let assets = assets();
    let original = assets
        .dispatch(&request("/fc/assets/app-01234567.js"), b"")
        .unwrap();
    let count = assets.asset_count();
    let retained_bytes = assets.allocation_bytes() - assets.boundary.heap_bytes();
    let actual = ListenPrefix::https("https://localhost:49152/fc/").unwrap();
    let assets = assets.with_prefix(&actual).unwrap();
    assert_eq!(assets.asset_count(), count);
    assert_eq!(
        assets.allocation_bytes() - assets.boundary.heap_bytes(),
        retained_bytes
    );
    assert!(matches!(
        assets.dispatch(&request("/fc/assets/app-01234567.js"), b""),
        Err(StaticError::Boundary(WebError::Host))
    ));
    let mut current = request("/fc/assets/app-01234567.js");
    current.headers[0].value = b"localhost:49152".to_vec();
    current.headers.push(Header {
        name: "Origin".into(),
        value: b"https://localhost:49152".to_vec(),
    });
    let response = assets.dispatch(&current, b"").unwrap();
    assert_eq!(response.head.status, 200);
    assert_eq!(response.head.content_length, original.head.content_length);
    assert_eq!(
        response.body.remaining_bytes(),
        original.body.remaining_bytes()
    );
    assert!(Arc::ptr_eq(&response.body.bytes, &original.body.bytes));
    let etag = |head: &ResponseHead| {
        head.headers
            .iter()
            .find(|h| h.name == "ETag")
            .unwrap()
            .value
            .clone()
    };
    assert_eq!(etag(&response.head), etag(&original.head));
    current.headers.last_mut().unwrap().value = b"https://board.synthetic".to_vec();
    assert!(matches!(
        assets.dispatch(&current, b""),
        Err(StaticError::Boundary(WebError::Origin))
    ));
    assert!(assets.is_api_target("/fc/api/v1/status"));
}
#[test]
fn trusted_startup_rebind_still_rejects_diagnostic_http_transport() {
    let diagnostic =
        ListenPrefix::isolated_loopback(49153, "FutureClassroom-Test-static-rebind").unwrap();
    assert!(matches!(
        assets().with_prefix(&diagnostic),
        Err(StaticError::Boundary(WebError::Transport))
    ));
}
