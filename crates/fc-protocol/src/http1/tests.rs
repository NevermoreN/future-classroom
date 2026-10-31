use super::*;
use crate::web::{WebBoundary, WebError};
use fc_ports::http::{ListenPrefix, TokenIssuer};

fn parser() -> Parser {
    Parser::new(TokenIssuer::new().unwrap().issue(0).unwrap(), true)
}
fn request_head(parser: &mut Parser, bytes: &[u8]) -> RequestHead {
    match parser.feed(bytes).unwrap().event {
        Event::Head(head) => head,
        event => panic!("expected head, got {event:?}"),
    }
}
fn boundary() -> WebBoundary {
    WebBoundary::new(&ListenPrefix::https("https://school:443/classroom/").unwrap()).unwrap()
}

#[test]
fn head_precedes_admission_and_borrowed_body_never_consumes_the_coalesced_tail_early() {
    let mut p = parser();
    assert!(!p.is_complete());
    let input = b"POST /classroom/api/v1/pairing/requests HTTP/1.1\r\nHost: school\r\nOrigin:\thttps://school \t\r\nContent-Type: application/json\r\nContent-Length: 3\r\n\r\nabc";
    let output = p.feed(input).unwrap();
    let Event::Head(head) = output.event else {
        panic!("head first")
    };
    assert_eq!(
        boundary().preflight(&head).unwrap(),
        crate::web::Route::PairingCreate
    );
    assert_eq!(output.consumed, input.len() - 3);
    assert_eq!(head.content_length, Some(3));
    assert!(head.has_body);
    let tail = &input[output.consumed..];
    let stopped = p.feed(tail).unwrap();
    assert_eq!(stopped.consumed, 0);
    assert!(matches!(stopped.event, Event::AwaitBodyAdmission));
    assert!(p.head.is_empty(), "only the caller keeps the body tail");
    p.admit_body(3).unwrap();
    let body = p.feed(tail).unwrap();
    let Event::Body { bytes, eof } = body.event else {
        panic!("body after admission")
    };
    assert_eq!(body.consumed, 3);
    assert_eq!(bytes, b"abc");
    assert_eq!(bytes.as_ptr(), tail.as_ptr(), "no body allocation or copy");
    assert!(eof);
    assert!(p.is_complete());
    assert!(matches!(p.feed(b"").unwrap().event, Event::Complete));
    p.finish_eof().unwrap();
}

fn fragmented(chunks: &[&[u8]]) -> (RequestHead, Vec<u8>) {
    let mut p = parser();
    let mut head = None;
    let mut body = Vec::new();
    for chunk in chunks {
        let mut offset = 0;
        while offset < chunk.len() {
            let result = p.feed(&chunk[offset..]).unwrap();
            assert!(result.consumed > 0);
            offset += result.consumed;
            match result.event {
                Event::Head(value) => {
                    assert!(head.replace(value).is_none());
                    p.admit_body(MAX_TRANSFER_BYTES).unwrap();
                }
                Event::Body { bytes, .. } => body.extend_from_slice(bytes),
                Event::NeedHead => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }
    p.finish_eof().unwrap();
    (head.unwrap(), body)
}

#[test]
fn every_single_split_and_one_byte_fragments_preserve_crlf_head_and_exact_body() {
    let bytes = b"POST /x?key=1 HTTP/1.1\r\nHost: school\r\nX-Raw:\t a:b \t\r\nContent-Length: 7\r\n\r\n12\r\nabc";
    for split in 0..=bytes.len() {
        let (head, body) = fragmented(&[&bytes[..split], &bytes[split..]]);
        assert_eq!(head.method, "POST");
        assert_eq!(head.target, "/x?key=1");
        assert_eq!(head.headers[1].name, "X-Raw");
        assert_eq!(head.headers[1].value, b"a:b");
        assert_eq!(body, b"12\r\nabc");
    }
    let one_byte: Vec<_> = bytes.chunks(1).collect();
    assert_eq!(fragmented(&one_byte).1, b"12\r\nabc");
}

#[test]
fn preserves_sensitive_duplicate_headers_for_the_existing_boundary_to_reject() {
    for duplicated in [
        "Host: school\r\nhOsT: school",
        "Cookie: a\r\ncOoKiE: b",
        "Origin: https://school\r\noRiGiN: https://school",
    ] {
        let mut p = parser();
        let bytes = format!(
            "GET /classroom/api/v1/status HTTP/1.1\r\nHost: school\r\n{duplicated}\r\n\r\n"
        );
        let head = request_head(&mut p, bytes.as_bytes());
        assert_eq!(head.headers.len(), 3);
        assert!(matches!(
            boundary().preflight(&head),
            Err(WebError::Headers)
        ));
    }
}

#[test]
fn transport_token_and_tls_cannot_be_overridden_by_headers() {
    let token = TokenIssuer::new().unwrap().issue(3).unwrap();
    let mut p = Parser::new(token, false);
    let head = request_head(&mut p, b"GET /classroom/api/v1/status HTTP/1.1\r\nHost: school\r\nX-Forwarded-Proto: https\r\nX-Request-Token: 999\r\n\r\n");
    assert_eq!(head.token, token);
    assert!(!head.tls);
    assert_eq!(boundary().preflight(&head), Err(WebError::Transport));
}

#[test]
fn empty_and_zero_length_requests_still_require_explicit_business_admission() {
    for header in ["", "Content-Length: 0\r\n"] {
        let mut p = parser();
        let bytes = format!("GET / HTTP/1.1\r\n{header}\r\n");
        let head = request_head(&mut p, bytes.as_bytes());
        assert!(!head.has_body);
        assert!(matches!(
            p.feed(b"").unwrap().event,
            Event::AwaitBodyAdmission
        ));
        assert_eq!(p.finish_eof(), Err(Error::AdmissionRequired));
        let mut p = parser();
        request_head(&mut p, bytes.as_bytes());
        p.admit_body(0).unwrap();
        assert!(matches!(p.feed(b"").unwrap().event, Event::Complete));
        p.finish_eof().unwrap();
        assert_eq!(
            p.feed(b"GET /next HTTP/1.1\r\n\r\n").unwrap_err(),
            Error::ExtraBytes
        );
    }
}

#[test]
fn exact_content_length_rejects_smuggling_variants_and_transfer_extensions() {
    for value in [
        "",
        "+1",
        "-1",
        "1,1",
        "1, 2",
        "01",
        "00",
        "0x1",
        "1 2",
        "1\t2",
        "18446744073709551616",
        "52428801",
    ] {
        let mut p = parser();
        let bytes = format!("POST / HTTP/1.1\r\nContent-Length: {value}\r\n\r\n");
        assert!(p.feed(bytes.as_bytes()).is_err(), "{value:?}");
        assert_eq!(p.feed(b"").unwrap_err(), Error::Failed);
    }
    for extra in [
        "Content-Length: 1\r\ncontent-length: 1",
        "Content-Length: 1\r\ncontent-length: 2",
        "Transfer-Encoding: chunked",
        "Transfer-Encoding: identity",
        "TE: trailers",
        "Expect: 100-continue",
        "Trailer: X",
        "Upgrade: websocket",
        "Content-Encoding: gzip",
        "Connection: upgrade",
        "Connection: close, content-length",
    ] {
        let mut p = parser();
        let bytes = format!("POST / HTTP/1.1\r\n{extra}\r\n\r\n");
        assert!(p.feed(bytes.as_bytes()).is_err(), "{extra}");
    }
    let mut p = parser();
    assert_eq!(
        request_head(
            &mut p,
            b"POST / HTTP/1.1\r\nContent-Length:\t 52428800 \t\r\n\r\n"
        )
        .content_length,
        Some(MAX_TRANSFER_BYTES)
    );
    assert_eq!(p.admit_body(1), Err(Error::BodyTooLarge));
}

#[test]
fn invalid_request_lines_obs_fold_controls_and_bare_newlines_are_not_normalized() {
    for bad in [
        b"GET / HTTP/1.0\r\n\r\n".as_slice(),
        b"GET  / HTTP/1.1\r\n\r\n",
        b"GET /  HTTP/1.1\r\n\r\n",
        b"GET\t/ HTTP/1.1\r\n\r\n",
        b"GET / HTTP/1.1 \r\n\r\n",
        b"GET https://school/ HTTP/1.1\r\n\r\n",
        b"CONNECT school:443 HTTP/1.1\r\n\r\n",
        b"OPTIONS * HTTP/1.1\r\n\r\n",
        b"GET / HTTP/1.1\n\n",
        b"GET / HTTP/1.1\r\nX: a\r\n b\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost : school\r\n\r\n",
        b"GET / HTTP/1.1\r\nX: a\0b\r\n\r\n",
        b"GET / HTTP/1.1\r\nX: a\x7fb\r\n\r\n",
        b"GET / HTTP/1.1\rX: b\r\n\r\n",
        b"GET / HTTP/1.1\r\nX: b\n\r\n",
    ] {
        let mut p = parser();
        assert!(p.feed(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn exact_head_field_and_target_limits_are_enforced_before_growth() {
    let prefix = "GET / HTTP/1.1\r\nX: ";
    let exact = format!(
        "{prefix}{}\r\n\r\n",
        "a".repeat(MAX_HEADER_BYTES - prefix.len() - 4)
    );
    let mut p = parser();
    request_head(&mut p, exact.as_bytes());
    assert_eq!(p.header_capacity(), MAX_HEADER_BYTES);
    let too_large = exact.replacen("X: ", "X: a", 1);
    assert_eq!(
        parser().feed(too_large.as_bytes()).unwrap_err(),
        Error::HeadTooLarge
    );
    for count in [MAX_HEADERS, MAX_HEADERS + 1] {
        let bytes = format!("GET / HTTP/1.1\r\n{}\r\n", "X: a\r\n".repeat(count));
        let result = parser().feed(bytes.as_bytes());
        if count == MAX_HEADERS {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err(), Error::TooManyHeaders);
        }
    }
    for count in [MAX_TARGET_BYTES, MAX_TARGET_BYTES + 1] {
        let bytes = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(count - 1));
        assert_eq!(
            parser().feed(bytes.as_bytes()).is_ok(),
            count == MAX_TARGET_BYTES
        );
    }
}

#[test]
fn bodies_are_borrowed_bounded_chunks_with_no_prefetch_and_no_extra_bytes() {
    let mut p = parser();
    let data = vec![b'x'; CHUNK_BYTES * 2 + 7];
    let head = format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", data.len());
    request_head(&mut p, head.as_bytes());
    p.admit_body(data.len() as u64).unwrap();
    let mut offset = 0;
    let mut sizes = Vec::new();
    while offset < data.len() {
        let result = p.feed(&data[offset..]).unwrap();
        let Event::Body { bytes, eof } = result.event else {
            panic!("body")
        };
        assert_eq!(bytes.as_ptr(), data[offset..].as_ptr());
        assert_eq!(eof, offset + bytes.len() == data.len());
        sizes.push(bytes.len());
        offset += result.consumed;
    }
    assert_eq!(sizes, [CHUNK_BYTES, CHUNK_BYTES, 7]);
    assert_eq!(p.feed(b"extra").unwrap_err(), Error::ExtraBytes);
    let mut p = parser();
    request_head(&mut p, b"POST / HTTP/1.1\r\nContent-Length: 3\r\n\r\n");
    p.admit_body(3).unwrap();
    assert_eq!(
        p.feed(b"abcGET / HTTP/1.1\r\n\r\n").unwrap_err(),
        Error::ExtraBytes
    );
}

#[test]
fn truncated_heads_and_bodies_fail_at_eof_and_errors_cannot_be_reset() {
    for truncated in [
        b"".as_slice(),
        b"GET / HTTP/1.1\r",
        b"GET / HTTP/1.1\r\nHost: school\r\n",
    ] {
        let mut p = parser();
        assert!(matches!(p.feed(truncated).unwrap().event, Event::NeedHead));
        assert_eq!(p.finish_eof(), Err(Error::UnexpectedEof));
        assert_eq!(p.admit_body(0), Err(Error::InvalidState));
        assert_eq!(p.feed(b"\r\n").unwrap_err(), Error::Failed);
    }
    let mut p = parser();
    request_head(&mut p, b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\n");
    p.admit_body(4).unwrap();
    assert!(matches!(
        p.feed(b"abc").unwrap().event,
        Event::Body { eof: false, .. }
    ));
    assert_eq!(p.finish_eof(), Err(Error::UnexpectedEof));
}

#[test]
fn debug_never_prints_header_or_body_secrets() {
    let mut p = parser();
    let head = p
        .feed(b"POST /SECRET HTTP/1.1\r\nCookie: SECRET\r\nContent-Length: 6\r\n\r\n")
        .unwrap();
    assert!(!format!("{head:?}").contains("SECRET"));
    p.admit_body(6).unwrap();
    assert!(!format!("{:?}", p.feed(b"SECRET").unwrap()).contains("SECRET"));
}
