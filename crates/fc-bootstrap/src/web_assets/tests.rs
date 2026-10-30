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
