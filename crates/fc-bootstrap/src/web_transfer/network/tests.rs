//! Pure coordinator-state regressions. No listener, TLS, IOCP, file worker or
//! physical-cleanup evidence is inferred from these memory-only ticket doubles.
use super::*;
use fc_ports::http::TokenIssuer;
use fc_runtime::worker::JobError;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct State {
    done: AtomicBool,
    cancelled: AtomicUsize,
    dropped: AtomicUsize,
}
struct Ticket(Arc<State>);
impl HttpTicket for Ticket {
    fn cancel(&self) {
        self.0.cancelled.fetch_add(1, Ordering::AcqRel);
    }
    fn is_finished(&self) -> bool {
        self.0.done.load(Ordering::Acquire)
    }
    fn try_result(&mut self) -> Result<Option<HttpCompletion>, JobError> {
        if self.is_finished() {
            Err(JobError::Cancelled)
        } else {
            Ok(None)
        }
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::AcqRel);
    }
}
fn token() -> RequestToken {
    TokenIssuer::new().unwrap().issue(0).unwrap()
}
fn json_record(bytes: usize) -> Record<Ticket> {
    let token = token();
    let mut record = Record::new(token);
    record.head = Some(RequestHead {
        token,
        method: "POST".into(),
        target: "/fc/api/v1/status".into(),
        headers: Vec::new(),
        tls: true,
        content_length: Some(bytes as u64),
        has_body: bytes != 0,
    });
    record.json = Vec::with_capacity(bytes.min(MAX_JSON_BYTES));
    record
}

#[test]
fn closed_retired_request_keeps_original_ticket_until_terminal_and_then_still_requires_cleanup() {
    let state = Arc::new(State::default());
    let mut record = Record::<Ticket>::new(token());
    record.hub_touched = true;
    record.cleanup_done = false;
    record.native_retired = true;
    record.work = Some(Work {
        purpose: Purpose::Begin,
        state: JobState::Running(Ticket(state.clone())),
    });
    record.cancel();
    record.cancel();
    assert_eq!(state.cancelled.load(Ordering::Acquire), 1);
    assert_eq!(state.dropped.load(Ordering::Acquire), 0);
    assert!(!record.may_release());
    state.done.store(true, Ordering::Release);
    assert!(
        !record.may_release(),
        "a terminal-but-uncollected ticket is still owned"
    );
    drop(record.work.take());
    assert_eq!(state.dropped.load(Ordering::Acquire), 1);
    assert!(
        !record.may_release(),
        "actual job completion does not fabricate negative stage cleanup"
    );
    record.cleanup_done = true;
    assert!(record.may_release());
}

#[test]
fn clean_material_response_preserves_hub_control_but_lost_response_cancels_exact_original_source() {
    for completed in [false, true] {
        let token = token();
        let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
        let mut record = Record::<Ticket>::new(token);
        record.life = Some(life.clone());
        record.cancellation = Some(control.cancellation_handle());
        // Begin transferred the sole strong control to Hub; Net owns weak only.
        record.material_ready = true;
        record.response_complete = completed;
        record.hub_touched = true;
        record.cleanup_done = completed;
        record.native_retired = true;
        record.cancel();
        assert_eq!(life.is_active(), completed);
        assert_eq!(record.may_release(), completed);
        drop(record);
        assert_eq!(
            life.is_active(),
            completed,
            "dropping weak Net metadata cannot cancel a clean Ready"
        );
        drop(control);
        assert!(!life.is_active());
    }
}

#[test]
fn eight_records_include_retired_worker_pending_generations_and_never_alias_same_native_slot() {
    let mut issuer = TokenIssuer::new().unwrap();
    let mut records: [Option<Record<Ticket>>; MAX_REQUESTS] = std::array::from_fn(|_| None);
    let state = Arc::new(State::default());
    let mut tokens = Vec::new();
    for (index, entry) in records.iter_mut().enumerate() {
        let token = issuer.issue(index % 4).unwrap();
        tokens.push(token);
        let mut record = Record::new(token);
        record.native_retired = index < 4;
        record.closed = index < 4;
        record.work = Some(Work {
            purpose: Purpose::Begin,
            state: JobState::Running(Ticket(state.clone())),
        });
        *entry = Some(record);
    }
    assert_eq!(records.iter().flatten().count(), MAX_REQUESTS);
    assert!(records.iter().position(Option::is_none).is_none());
    assert_eq!(tokens[0].slot(), tokens[4].slot());
    assert_ne!(tokens[0].serial(), tokens[4].serial());
    for (index, token) in tokens.into_iter().enumerate() {
        assert_eq!(find_record(&records, token), Some(index));
    }
    let foreign = TokenIssuer::new().unwrap().issue(0).unwrap();
    assert_eq!(find_record(&records, foreign), None);
    assert!(records[..4]
        .iter()
        .flatten()
        .all(|record| !record.may_release()));
}

#[test]
fn zero_body_requires_unique_eof_and_json_malformed_offsets_lengths_or_sequences_do_not_advance() {
    let mut zero = json_record(0);
    assert!(zero.buffer_json(1, 0, b"", false).is_err());
    assert_eq!(zero.buffer_json(1, 0, b"", true), Ok(0));
    assert!(zero.buffer_json(1, 0, b"", true).is_err());

    let mut record = json_record(3);
    assert_eq!(record.buffer_json(1, 0, b"a", false), Ok(1));
    for (sequence, offset, bytes, eof) in [
        (3, 1, b"bc".as_slice(), true),
        (2, 0, b"bc".as_slice(), true),
        (2, 1, b"b".as_slice(), true),
        (2, u64::MAX, b"bc".as_slice(), true),
    ] {
        assert!(record.buffer_json(sequence, offset, bytes, eof).is_err());
        assert_eq!(record.json, b"a");
        assert_eq!(record.json_sequence, 2);
    }
    assert_eq!(record.buffer_json(2, 1, b"bc", true), Ok(3));
    assert_eq!(record.json, b"abc");
    let mut too_large = json_record(MAX_JSON_BYTES + 1);
    assert!(too_large.buffer_json(1, 0, b"a", false).is_err());
    assert!(too_large.json.is_empty());
}

#[test]
fn negative_cleanup_ticket_is_not_cancelled_by_repeated_original_connection_close() {
    let state = Arc::new(State::default());
    let mut record = Record::<Ticket>::new(token());
    record.hub_touched = true;
    record.cleanup_done = false;
    record.closed = true;
    record.work = Some(Work {
        purpose: Purpose::Cleanup,
        state: JobState::Running(Ticket(state.clone())),
    });
    record.cancel();
    assert_eq!(state.cancelled.load(Ordering::Acquire), 0);
    assert_eq!(state.dropped.load(Ordering::Acquire), 0);
    assert!(!record.may_release());
}

#[test]
fn download_progress_requires_physical_ack_before_next_read_and_exact_final_length() {
    let mut cursor = DownloadCursor::new(CHUNK_BYTES as u64 + 3);
    assert_eq!(cursor.position(), Ok((1, 0)));
    for (sequence, offset, bytes, eof) in [
        (2, 0, CHUNK_BYTES, false),
        (1, 1, CHUNK_BYTES, false),
        (1, 0, CHUNK_BYTES + 1, false),
        (1, 0, 0, false),
        (1, 0, CHUNK_BYTES, true),
    ] {
        assert!(cursor.accept(sequence, offset, bytes, eof).is_err());
        assert_eq!(cursor.position(), Ok((1, 0)));
    }
    cursor.accept(1, 0, CHUNK_BYTES, false).unwrap();
    assert!(
        cursor.position().is_err(),
        "worker read completion cannot permit another read"
    );
    assert_eq!(cursor.offset, 0, "physical send not yet acknowledged");
    assert!(cursor.acknowledge(true).is_err());
    cursor.acknowledge(false).unwrap();
    assert_eq!(cursor.position(), Ok((2, CHUNK_BYTES as u64)));
    assert!(cursor.accept(2, CHUNK_BYTES as u64, 3, false).is_err());
    assert!(cursor.accept(2, CHUNK_BYTES as u64, 2, true).is_err());
    assert!(cursor.accept(2, u64::MAX, 3, true).is_err());
    cursor.accept(2, CHUNK_BYTES as u64, 3, true).unwrap();
    assert!(cursor.acknowledge(false).is_err());
    cursor.acknowledge(true).unwrap();
    assert!(cursor.complete);
    assert_eq!(cursor.offset, CHUNK_BYTES as u64 + 3);
    assert!(cursor.position().is_err());
}

#[test]
fn empty_download_still_requires_one_exact_worker_eof_and_physical_complete() {
    let mut cursor = DownloadCursor::new(0);
    assert!(cursor.acknowledge(true).is_err());
    assert!(cursor.accept(1, 0, 0, false).is_err());
    cursor.accept(1, 0, 0, true).unwrap();
    assert!(!cursor.complete);
    assert!(cursor.position().is_err());
    cursor.acknowledge(true).unwrap();
    assert!(cursor.complete);
    assert!(cursor.acknowledge(true).is_err());
}

#[test]
fn download_disconnect_keeps_actual_read_ticket_and_cleanup_charge_after_native_retirement() {
    let token = token();
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let state = Arc::new(State::default());
    let mut record = Record::<Ticket>::new(token);
    record.control = Some(control);
    record.life = Some(life.clone());
    record.download_request = true;
    record.hub_touched = true;
    record.cleanup_done = false;
    record.download_cursor = Some(DownloadCursor::new(7));
    record.work = Some(Work {
        purpose: Purpose::DownloadRead {
            sequence: 1,
            offset: 0,
        },
        state: JobState::Running(Ticket(state.clone())),
    });
    record.cancel();
    record.native_retired = true;
    assert!(!life.is_active());
    assert_eq!(state.cancelled.load(Ordering::Acquire), 1);
    assert_eq!(state.dropped.load(Ordering::Acquire), 0);
    assert!(!record.may_release());
    state.done.store(true, Ordering::Release);
    assert!(
        !record.may_release(),
        "native retirement does not return the shared worker ticket"
    );
    drop(record.work.take());
    assert_eq!(state.dropped.load(Ordering::Acquire), 1);
    assert!(
        !record.may_release(),
        "late reply loss still requires precise negative Hub cleanup"
    );
    record.cleanup_failed = true;
    assert!(
        !record.may_release(),
        "failed cleanup retains the descriptor"
    );
}

fn static_fixture() -> Arc<StaticAssets> {
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const LARGE: &str = "e3c33f1a7c00a23610a13fa6b862df2882931be5b0262aeada7a47e282f1c679";
    let manifest = format!(
        r#"{{"schemaVersion":1,"apiVersion":1,"uiCompatibilityVersion":1,"assets":[{{"path":"index.html","sizeBytes":3,"sha256":"{ABC}"}},{{"path":"assets/app-01234567.js","sizeBytes":131089,"sha256":"{LARGE}"}}]}}"#
    );
    Arc::new(
        StaticAssets::new(
            &fc_ports::http::ListenPrefix::https("https://board.synthetic:443/classroom/").unwrap(),
            manifest.as_bytes(),
            [
                ("index.html".into(), b"abc".to_vec()),
                (
                    "assets/app-01234567.js".into(),
                    vec![b'x'; 2 * CHUNK_BYTES + 17],
                ),
            ],
        )
        .unwrap(),
    )
}
fn static_request(target: &str) -> RequestHead {
    RequestHead {
        token: token(),
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
fn static_source_progress_does_not_advance_without_real_send_ack_and_large_body_is_not_json() {
    let assets = static_fixture();
    let mut response = assets
        .dispatch(&static_request("/classroom/assets/app-01234567.js"), b"")
        .unwrap();
    assert!(response.head.content_length > MAX_JSON_BYTES as u64);
    let mut cursor = DownloadCursor::new(response.head.content_length);
    let mut sizes = Vec::new();
    loop {
        let (bytes, eof) = static_chunk(&mut cursor, &mut response.body).unwrap();
        assert!(bytes.iter().all(|byte| *byte == b'x'));
        sizes.push(bytes.len());
        let remaining = response.body.remaining_bytes();
        assert!(static_chunk(&mut cursor, &mut response.body).is_err());
        assert_eq!(
            response.body.remaining_bytes(),
            remaining,
            "an unacknowledged physical chunk cannot advance StaticBody"
        );
        assert!(
            !cursor.complete,
            "source EOF does not fabricate physical completion"
        );
        cursor.acknowledge(eof).unwrap();
        if eof {
            break;
        }
    }
    assert_eq!(sizes, [CHUNK_BYTES, CHUNK_BYTES, 17]);
    assert_eq!(cursor.offset, response.head.content_length);
    assert!(cursor.complete);
}

#[test]
fn static_cached_empty_response_still_waits_for_physical_complete_and_has_no_business_stage() {
    let assets = static_fixture();
    let mut head = static_request("/classroom/");
    head.headers.push(Header {
        name: "If-None-Match".into(),
        value: b"*".to_vec(),
    });
    let mut response = assets.dispatch(&head, b"").unwrap();
    assert_eq!(
        (response.head.status, response.head.content_length),
        (304, 0)
    );
    let mut cursor = DownloadCursor::new(0);
    let (bytes, eof) = static_chunk(&mut cursor, &mut response.body).unwrap();
    assert!(bytes.is_empty() && eof);
    assert!(!cursor.complete);
    assert!(cursor.acknowledge(false).is_err());
    cursor.acknowledge(true).unwrap();
    assert!(cursor.complete);
    let (control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let mut record = Record::<Ticket>::new(head.token);
    record.static_body = Some(response.body);
    record.static_cursor = Some(cursor);
    record.control = Some(control);
    record.life = Some(life.clone());
    record.native_retired = true;
    record.cancel();
    assert!(!life.is_active());
    assert!(!record.hub_touched);
    assert!(
        record.may_release(),
        "public static has no upload or download Hub cleanup"
    );
}

#[test]
fn static_routing_uses_configured_prefix_and_preserves_boundary_error_statuses() {
    let assets = static_fixture();
    for target in [
        "/classroom/api",
        "/classroom/api?bad=1",
        "/classroom/api/v1/unknown",
    ] {
        assert!(assets.is_api_target(target));
        assert!(
            assets.dispatch(&static_request(target), b"").is_err(),
            "unknown private API must never resolve to a public entry document"
        );
    }
    assert!(!assets.is_api_target("/fc/api/v1/status"));
    for (target, status) in [
        ("/classroom/missing.js", 404),
        ("/classroom/index.html?x=1", 404),
    ] {
        let response = assets.dispatch(&static_request(target), b"");
        assert_eq!(static_error(response.err().unwrap()).head.status, status);
    }
    let mut request = static_request("/classroom/");
    request.tls = false;
    assert_eq!(
        static_error(assets.dispatch(&request, b"").err().unwrap())
            .head
            .status,
        403
    );
    request.tls = true;
    request.method = "POST".into();
    let rejected = static_error(assets.dispatch(&request, b"").err().unwrap());
    let allowed: Vec<_> = rejected
        .head
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("allow"))
        .collect();
    assert_eq!(allowed.len(), 1);
    assert_eq!(allowed[0].value, b"GET");
    assert_eq!(
        static_error(assets.dispatch(&request, b"").err().unwrap())
            .head
            .status,
        405
    );
    request.method = "GET".into();
    request.content_length = Some(1);
    request.has_body = true;
    assert_eq!(
        static_error(assets.dispatch(&request, b"").err().unwrap())
            .head
            .status,
        400
    );
}
