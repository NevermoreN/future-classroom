use super::*;
use fc_ports::http::{Header, TokenIssuer, MAX_HEADERS, MAX_TRANSFER_BYTES};

fn head(bytes: u64) -> ResponseHead {
    ResponseHead {
        status: 200,
        headers: Vec::new(),
        content_length: bytes,
    }
}
fn token() -> RequestToken {
    TokenIssuer::new().unwrap().issue(0).unwrap()
}
fn field(name: &str, value: &[u8]) -> Header {
    Header {
        name: name.into(),
        value: value.to_vec(),
    }
}
// Test-only duplicate delivery simulates a transport bug; public receipts have
// private fields and deliberately do not implement Clone or Copy.
fn duplicate(receipt: &PendingChunk) -> PendingChunk {
    PendingChunk {
        token: receipt.token,
        sender: receipt.sender,
        sequence: receipt.sequence,
        bytes: receipt.bytes,
    }
}

#[test]
fn complete_head_has_one_transport_owned_length_and_close_without_secret_debug() {
    let mut input = head(17);
    input.headers = vec![
        field("Content-Type", b"application/json"),
        field("Set-Cookie", b"a=one; Secure"),
        field("Set-Cookie", b"b=two; Secure"),
    ];
    assert_eq!(
        encode_head(&input).unwrap(),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: a=one; Secure\r\nSet-Cookie: b=two; Secure\r\nContent-Length: 17\r\nConnection: close\r\n\r\n"
    );
}

#[test]
fn forbids_all_connection_negotiation_framing_and_nominated_field_tricks() {
    for name in [
        "Content-Length",
        "tRaNsFeR-EnCoDiNg",
        "Connection",
        "KEEP-ALIVE",
        "Proxy-Connection",
        "Proxy-Authenticate",
        "Proxy-Authorization",
        "TE",
        "Trailer",
        "Upgrade",
        "HTTP2-Settings",
    ] {
        let mut input = head(0);
        input.headers = vec![field(name, b"X-Hidden"), field("X-Hidden", b"smuggled")];
        assert_eq!(
            encode_head(&input),
            Err(ResponseError::InvalidHead),
            "{name}"
        );
        assert!(matches!(
            BodySender::new(token(), &input),
            Err(ResponseError::InvalidHead)
        ));
        assert!(matches!(
            BodySender::new_for_head(token(), &input),
            Err(ResponseError::InvalidHead)
        ));
    }
}

#[test]
fn forbids_crlf_controls_invalid_names_and_header_count_overflow() {
    for header in [
        field("X-Value", b"ok\r\nContent-Length: 9"),
        field("X-Value", b"bad\nvalue"),
        field("X-Value", b"bad\0value"),
        field("X-Value", b"bad\x7fvalue"),
        field("X-Name: injected", b"value"),
        field("X-Name\r\nX-Other", b"value"),
        field("", b"value"),
    ] {
        let mut input = head(0);
        input.headers.push(header);
        assert_eq!(encode_head(&input), Err(ResponseError::InvalidHead));
        assert!(matches!(
            BodySender::new_for_head(token(), &input),
            Err(ResponseError::InvalidHead)
        ));
    }
    let mut input = head(0);
    input.headers = (0..=MAX_HEADERS).map(|_| field("X", b"a")).collect();
    assert_eq!(encode_head(&input), Err(ResponseError::TooLarge));
    assert!(matches!(
        BodySender::new_for_head(token(), &input),
        Err(ResponseError::TooLarge)
    ));
}

#[test]
fn automatic_fields_status_and_final_crlf_all_share_the_exact_sixteen_kib_budget() {
    for status in [200, 204, 205, 304] {
        let mut input = head(0);
        input.status = status;
        let overhead = encode_head(&input).unwrap().len();
        input.headers = vec![field("X", &vec![b'a'; MAX_HEADER_BYTES - overhead - 5])];
        assert!(input.validate().is_ok());
        assert_eq!(encode_head(&input).unwrap().len(), MAX_HEADER_BYTES);
        input.headers[0].value.push(b'a');
        assert!(
            input.validate().is_ok(),
            "application-only header budget still fits"
        );
        assert_eq!(encode_head(&input), Err(ResponseError::TooLarge));
        assert!(matches!(
            BodySender::new(token(), &input),
            Err(ResponseError::TooLarge)
        ));
    }
}

#[test]
fn status_and_no_body_rules_do_not_emit_forbidden_framing() {
    for status in [0, 100, 101, 199, 600, u16::MAX] {
        let mut input = head(0);
        input.status = status;
        assert_eq!(encode_head(&input), Err(ResponseError::InvalidHead));
        assert!(matches!(
            BodySender::new_for_head(token(), &input),
            Err(ResponseError::InvalidHead)
        ));
    }
    for status in [204, 205, 304] {
        let mut input = head(0);
        input.status = status;
        let encoded = encode_head(&input).unwrap();
        let encoded = std::str::from_utf8(&encoded).unwrap();
        assert!(encoded.ends_with("Connection: close\r\n\r\n"));
        assert_eq!(encoded.contains("Content-Length:"), status == 205);
        if status == 205 {
            assert!(encoded.contains("Content-Length: 0\r\n"));
        }
        input.content_length = 1;
        assert_eq!(encode_head(&input), Err(ResponseError::InvalidHead));
        assert!(matches!(
            BodySender::new(token(), &input),
            Err(ResponseError::InvalidHead)
        ));
        assert!(matches!(
            BodySender::new_for_head(token(), &input),
            Err(ResponseError::InvalidHead)
        ));
    }
    for status in 200..=599 {
        let mut input = head(0);
        input.status = status;
        let encoded = encode_head(&input).unwrap();
        let first = encoded.split(|byte| *byte == b'\r').next().unwrap();
        assert!(first.starts_with(format!("HTTP/1.1 {status} ").as_bytes()));
        assert!(first
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' '));
    }
}

#[test]
fn not_modified_omits_selected_representation_length_and_preserves_validator() {
    let mut input = head(0);
    input.status = 304;
    input.headers = vec![field("ETag", b"\"selected-asset\"")];
    assert_eq!(
        encode_head(&input).unwrap(),
        b"HTTP/1.1 304 Not Modified\r\nETag: \"selected-asset\"\r\nConnection: close\r\n\r\n"
    );
}

#[test]
fn head_request_responses_omit_length_and_never_admit_body_or_infer_eof() {
    for (status, bytes) in [
        (200, 17),
        (405, 33),
        (404, 41),
        (403, 14),
        (304, 0),
        (205, 0),
    ] {
        let request = token();
        let mut input = head(bytes);
        input.status = status;
        input.headers = vec![field("Content-Type", b"application/json")];
        let mut sender = BodySender::new_for_head(request, &input).unwrap();
        let frozen = sender.head_bytes().to_vec();
        let encoded = std::str::from_utf8(&frozen).unwrap();
        assert!(encoded.starts_with(&format!("HTTP/1.1 {status} ")));
        assert!(encoded.contains("Content-Type: application/json\r\n"));
        assert!(!encoded.contains("Content-Length:"));
        assert!(encoded.ends_with("Connection: close\r\n\r\n"));
        assert_eq!(sender.remaining_bytes(), 0);
        assert_eq!(sender.acknowledged_bytes(), 0);
        for bytes in [0, 1, CHUNK_BYTES, CHUNK_BYTES + 1, usize::MAX] {
            assert!(sender.begin_chunk(request, bytes).is_err());
            assert!(!sender.has_in_flight());
            assert!(!sender.is_complete());
        }
        input.headers.push(field("X-Later", b"ignored"));
        assert_eq!(sender.head_bytes(), frozen);
        sender.finish(request).unwrap();
        assert!(sender.is_complete());
        assert_eq!(sender.finish(request), Err(ResponseError::Closed));
    }
    assert!(matches!(
        BodySender::new_for_head(token(), &head(MAX_TRANSFER_BYTES + 1)),
        Err(ResponseError::InvalidHead)
    ));
}

#[test]
fn head_request_preserves_token_checks_and_rejects_unrelated_body_receipts() {
    let mut issuer = TokenIssuer::new().unwrap();
    let request = issuer.issue(0).unwrap();
    let next = issuer.issue(0).unwrap();
    let mut sender = BodySender::new_for_head(request, &head(4)).unwrap();
    assert_eq!(
        sender.begin_chunk(next, 1).err(),
        Some(ResponseError::StaleRequest)
    );
    assert_eq!(sender.finish(next), Err(ResponseError::StaleRequest));
    let mut normal = BodySender::new(request, &head(4)).unwrap();
    let receipt = normal.begin_chunk(request, 4).unwrap();
    assert_eq!(
        sender.acknowledge_chunk(receipt, 4),
        Err(ResponseError::StaleAcknowledgement)
    );
    assert!(!sender.is_complete());
    assert!(!sender.has_in_flight());
    assert_eq!(sender.acknowledged_bytes(), 0);
    sender.finish(request).unwrap();
    assert!(sender.is_complete());
}

#[test]
fn head_request_uses_its_actual_encoded_head_for_the_exact_sixteen_kib_limit() {
    for status in [200, 405, 404, 403, 304] {
        let mut input = head(if status == 304 { 0 } else { 17 });
        input.status = status;
        let overhead = BodySender::new_for_head(token(), &input)
            .unwrap()
            .head_bytes()
            .len();
        input.headers = vec![field("X", &vec![b'a'; MAX_HEADER_BYTES - overhead - 5])];
        let sender = BodySender::new_for_head(token(), &input).unwrap();
        assert_eq!(sender.head_bytes().len(), MAX_HEADER_BYTES);
        input.headers[0].value.push(b'a');
        assert!(input.validate().is_ok());
        assert!(matches!(
            BodySender::new_for_head(token(), &input),
            Err(ResponseError::TooLarge)
        ));
    }
}

#[test]
fn transport_maximum_and_frozen_head_cannot_be_changed_after_admission() {
    assert!(encode_head(&head(MAX_TRANSFER_BYTES)).is_ok());
    assert_eq!(
        encode_head(&head(MAX_TRANSFER_BYTES + 1)),
        Err(ResponseError::InvalidHead)
    );
    let mut input = head(1);
    let mut sender = BodySender::new(token(), &input).unwrap();
    let frozen = sender.head_bytes().to_vec();
    input.content_length = 10;
    input.headers.push(field("X-Later", b"different"));
    assert_eq!(sender.head_bytes(), frozen);
    assert_eq!(sender.remaining_bytes(), 1);
    assert!(matches!(
        sender.begin_chunk(sender.token, 2),
        Err(ResponseError::TooLong)
    ));
}

#[test]
fn one_inflight_chunk_blocks_progress_until_its_whole_acknowledgement() {
    let request = token();
    let mut sender = BodySender::new(request, &head(CHUNK_BYTES as u64 + 3)).unwrap();
    let first = sender.begin_chunk(request, CHUNK_BYTES).unwrap();
    assert_eq!(first.token(), request);
    assert_eq!(first.bytes(), CHUNK_BYTES);
    assert_eq!(sender.acknowledged_bytes(), 0);
    assert!(sender.has_in_flight());
    assert!(matches!(
        sender.begin_chunk(request, 3),
        Err(ResponseError::Busy)
    ));
    sender.acknowledge_chunk(first, CHUNK_BYTES).unwrap();
    assert_eq!(sender.acknowledged_bytes(), CHUNK_BYTES as u64);
    assert_eq!(sender.remaining_bytes(), 3);
    assert!(!sender.has_in_flight());
    let last = sender.begin_chunk(request, 3).unwrap();
    sender.acknowledge_chunk(last, 3).unwrap();
    assert!(
        !sender.is_complete(),
        "acknowledgement does not manufacture source EOF"
    );
    sender.finish(request).unwrap();
    assert!(sender.is_complete());
    assert!(matches!(
        sender.begin_chunk(request, 1),
        Err(ResponseError::Closed)
    ));
    assert_eq!(sender.finish(request), Err(ResponseError::Closed));
}

#[test]
fn empty_oversize_and_overlong_chunk_admission_never_advances_the_counter() {
    let request = token();
    let mut sender = BodySender::new(request, &head(10)).unwrap();
    for (bytes, error) in [
        (0, ResponseError::InvalidChunk),
        (CHUNK_BYTES + 1, ResponseError::InvalidChunk),
        (11, ResponseError::TooLong),
    ] {
        // Explicitly inspect the negative result without creating any receipt.
        assert_eq!(sender.begin_chunk(request, bytes).err(), Some(error));
        assert_eq!(sender.acknowledged_bytes(), 0);
        assert!(!sender.has_in_flight());
    }
    let receipt = sender.begin_chunk(request, 10).unwrap();
    sender.acknowledge_chunk(receipt, 10).unwrap();
    assert!(matches!(
        sender.begin_chunk(request, 1),
        Err(ResponseError::TooLong)
    ));
    sender.finish(request).unwrap();
}

#[test]
fn dropping_a_linear_receipt_does_not_fake_a_send_or_release_an_inflight_slot() {
    let request = token();
    let mut sender = BodySender::new(request, &head(4)).unwrap();
    {
        let _unacknowledged = sender.begin_chunk(request, 4).unwrap();
    }
    assert!(sender.has_in_flight());
    assert_eq!(sender.acknowledged_bytes(), 0);
    assert!(matches!(
        sender.begin_chunk(request, 4),
        Err(ResponseError::Busy)
    ));
    assert_eq!(sender.finish(request), Err(ResponseError::Incomplete));
    assert!(!sender.is_complete());
}

#[test]
fn early_source_eof_with_or_without_inflight_work_is_permanently_negative() {
    let request = token();
    let mut short = BodySender::new(request, &head(5)).unwrap();
    let sent = short.begin_chunk(request, 4).unwrap();
    short.acknowledge_chunk(sent, 4).unwrap();
    assert_eq!(short.finish(request), Err(ResponseError::Incomplete));
    assert!(matches!(
        short.begin_chunk(request, 1),
        Err(ResponseError::Closed)
    ));
    let mut pending = BodySender::new(request, &head(5)).unwrap();
    let receipt = pending.begin_chunk(request, 5).unwrap();
    assert_eq!(pending.finish(request), Err(ResponseError::Incomplete));
    assert_eq!(
        pending.acknowledge_chunk(receipt, 5),
        Err(ResponseError::Closed)
    );
    assert_eq!(pending.acknowledged_bytes(), 0);
    assert!(
        pending.has_in_flight(),
        "no physical completion was inferred"
    );
}

#[test]
fn zero_short_or_overreported_native_ack_is_not_logical_completion() {
    for reported in [0, 3, 5, usize::MAX] {
        let request = token();
        let mut sender = BodySender::new(request, &head(4)).unwrap();
        let receipt = sender.begin_chunk(request, 4).unwrap();
        assert_eq!(
            sender.acknowledge_chunk(receipt, reported),
            Err(ResponseError::SendFailed)
        );
        assert_eq!(sender.acknowledged_bytes(), 0);
        assert!(sender.has_in_flight());
        assert_eq!(sender.finish(request), Err(ResponseError::Closed));
        assert!(!sender.is_complete());
    }
}

#[test]
fn foreign_tokens_and_other_senders_receipts_cannot_ack_current_work() {
    let mut issuer = TokenIssuer::new().unwrap();
    let request = issuer.issue(0).unwrap();
    let same_slot_new_serial = issuer.issue(0).unwrap();
    let other_slot = issuer.issue(1).unwrap();
    let other_generation = token();
    let mut current = BodySender::new(request, &head(4)).unwrap();
    for foreign in [same_slot_new_serial, other_slot, other_generation] {
        assert!(matches!(
            current.begin_chunk(foreign, 4),
            Err(ResponseError::StaleRequest)
        ));
        assert_eq!(current.finish(foreign), Err(ResponseError::StaleRequest));
        let mut unrelated = BodySender::new(foreign, &head(4)).unwrap();
        let receipt = unrelated.begin_chunk(foreign, 4).unwrap();
        assert_eq!(
            current.acknowledge_chunk(receipt, 4),
            Err(ResponseError::StaleRequest)
        );
    }
    let original = current.begin_chunk(request, 4).unwrap();
    let mut other_sender_same_token = BodySender::new(request, &head(4)).unwrap();
    let foreign = other_sender_same_token.begin_chunk(request, 4).unwrap();
    assert_eq!(
        current.acknowledge_chunk(foreign, 4),
        Err(ResponseError::StaleAcknowledgement)
    );
    assert_eq!(current.acknowledged_bytes(), 0);
    current.acknowledge_chunk(original, 4).unwrap();
    current.finish(request).unwrap();
}

#[test]
fn replayed_old_chunk_ack_cannot_advance_or_retire_the_next_chunk() {
    let request = token();
    let mut sender = BodySender::new(request, &head(8)).unwrap();
    let first = sender.begin_chunk(request, 4).unwrap();
    let stale = duplicate(&first);
    sender.acknowledge_chunk(first, 4).unwrap();
    let second = sender.begin_chunk(request, 4).unwrap();
    assert_eq!(
        sender.acknowledge_chunk(stale, 4),
        Err(ResponseError::StaleAcknowledgement)
    );
    assert_eq!(sender.acknowledged_bytes(), 4);
    assert!(sender.has_in_flight());
    sender.acknowledge_chunk(second, 4).unwrap();
    sender.finish(request).unwrap();
}

#[test]
fn zero_length_body_needs_no_empty_chunk_and_fifty_mib_stream_needs_no_body_allocation() {
    let request = token();
    for status in [200, 204, 205, 304] {
        let mut input = head(0);
        input.status = status;
        let mut empty = BodySender::new(request, &input).unwrap();
        assert!(matches!(
            empty.begin_chunk(request, 0),
            Err(ResponseError::InvalidChunk)
        ));
        assert!(matches!(
            empty.begin_chunk(request, 1),
            Err(ResponseError::TooLong)
        ));
        assert!(!empty.has_in_flight());
        assert_eq!(empty.acknowledged_bytes(), 0);
        assert!(!empty.is_complete());
        empty.finish(request).unwrap();
        assert!(empty.is_complete());
    }
    let mut download = BodySender::new(request, &head(MAX_TRANSFER_BYTES)).unwrap();
    while download.remaining_bytes() != 0 {
        let bytes = download.remaining_bytes().min(CHUNK_BYTES as u64) as usize;
        let receipt = download.begin_chunk(request, bytes).unwrap();
        download.acknowledge_chunk(receipt, bytes).unwrap();
    }
    download.finish(request).unwrap();
    assert!(download.is_complete());
    assert_eq!(download.acknowledged_bytes(), MAX_TRANSFER_BYTES);
    assert!(download.head_bytes().len() < MAX_HEADER_BYTES);
}
