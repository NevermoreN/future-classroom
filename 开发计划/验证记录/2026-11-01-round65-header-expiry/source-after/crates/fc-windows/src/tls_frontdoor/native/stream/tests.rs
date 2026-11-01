//! Real Winsock/IOCP and the actual SSPI mailbox/thread with a fake record
//! Driver. These tests prove scheduling/ownership, never real TLS encryption.
use super::*;
use crate::tls_frontdoor::{worker::tests::SERIAL, Driver};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    sync::{atomic::AtomicUsize, Arc, Mutex},
};
#[derive(Default)]
struct Calls {
    encrypt: Vec<usize>,
    retire: Vec<RequestToken>,
    handshake: Vec<RequestToken>,
}
struct RecordDriver {
    calls: Arc<Mutex<Calls>>,
    closed: Arc<AtomicUsize>,
    encrypt_hook: Option<Arc<dyn Fn(usize) + Send + Sync>>,
    decrypt_hook: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Driver for RecordDriver {
    fn call(&mut self, frame: &mut Frame) -> Result<Step, TlsError> {
        match frame.call {
            Call::Handshake => {
                if frame.input_len == 0 {
                    return Ok(Step::NeedInput);
                }
                if frame.input[0] != b'H' {
                    return Err(TlsError::Protocol);
                }
                self.calls.lock().unwrap().handshake.push(frame.token);
                Ok(Step::Handshake {
                    consumed: 1,
                    complete: true,
                })
            }
            Call::Decrypt => {
                if frame.input_len < 2 {
                    return Ok(Step::NeedInput);
                }
                let len = u16::from_be_bytes([frame.input[0], frame.input[1]]) as usize;
                if len > PLAINTEXT_BYTES {
                    return Err(TlsError::Capacity);
                }
                if frame.input_len < len + 2 {
                    return Ok(Step::NeedInput);
                }
                frame.output[..len].copy_from_slice(&frame.input[2..len + 2]);
                frame.output_len = len;
                if let Some(hook) = &self.decrypt_hook {
                    hook();
                }
                Ok(Step::Plaintext { consumed: len + 2 })
            }
            Call::Encrypt => {
                if frame.input_len == 0 || frame.input_len > PLAINTEXT_BYTES {
                    return Err(TlsError::Capacity);
                }
                let ordinal = {
                    let mut calls = self.calls.lock().unwrap();
                    calls.encrypt.push(frame.input_len);
                    calls.encrypt.len()
                };
                frame.output[..frame.input_len].copy_from_slice(&frame.input[..frame.input_len]);
                frame.output_len = frame.input_len;
                if let Some(hook) = &self.encrypt_hook {
                    hook(ordinal);
                }
                Ok(Step::Encrypted)
            }
            Call::Shutdown => {
                frame.output[0] = b'!';
                frame.output_len = 1;
                Ok(Step::Shutdown { complete: true })
            }
            Call::Retire => {
                self.calls.lock().unwrap().retire.push(frame.token);
                Ok(Step::Retired)
            }
        }
    }
    fn close(&mut self) -> Result<(), TlsError> {
        self.closed.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}
struct Fixture {
    host: TlsHttpHost,
    client: TcpStream,
    client_connected_at: Instant,
    posted_header_deadlines: [Instant; CONNECTION_SLOTS],
    calls: Arc<Mutex<Calls>>,
    closed: Arc<AtomicUsize>,
    retain_faulted_owner_on_drop: bool,
}
impl Fixture {
    fn new() -> Self {
        Self::with_encrypt_hook(None)
    }
    fn with_encrypt_hook(encrypt_hook: Option<Arc<dyn Fn(usize) + Send + Sync>>) -> Self {
        Self::with_hooks(encrypt_hook, None)
    }
    fn with_hooks(
        encrypt_hook: Option<Arc<dyn Fn(usize) + Send + Sync>>,
        decrypt_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        let mut owner = Owner::new("FutureClassroom-Test-TLS-native-stream-fixture").unwrap();
        owner.http_mode = true;
        for slot in &mut owner.resources.slots {
            slot.stream = Some(Stream::new());
        }
        let calls = Arc::new(Mutex::new(Calls::default()));
        let closed = Arc::new(AtomicUsize::new(0));
        let driver = RecordDriver {
            calls: calls.clone(),
            closed: closed.clone(),
            encrypt_hook,
            decrypt_hook,
        };
        let worker =
            Worker::start_with_factory(owner.generation, move || Ok(Box::new(driver))).unwrap();
        owner.start_with_worker(worker).unwrap();
        // There is one initial native AcceptEx post, not four Ready clocks.
        // This fixture's first connection inherits that actual bare post.
        let posted_header_deadlines =
            [owner.resources.accept.as_ref().unwrap().deadline().unwrap(); CONNECTION_SLOTS];
        let client = TcpStream::connect_timeout(
            &SocketAddrV4::new(Ipv4Addr::LOCALHOST, owner.port_number).into(),
            Duration::from_secs(1),
        )
        .unwrap();
        let client_connected_at = Instant::now();
        client.set_nonblocking(true).unwrap();
        Self {
            host: TlsHttpHost {
                owner,
                drain_deadline: None,
            },
            client,
            client_connected_at,
            posted_header_deadlines,
            calls,
            closed,
            retain_faulted_owner_on_drop: false,
        }
    }
    fn input(&mut self, bytes: &[u8]) {
        self.client.set_nonblocking(false).unwrap();
        self.client.write_all(bytes).unwrap();
        self.client.set_nonblocking(true).unwrap();
    }
    fn until(&mut self, mut accept: impl FnMut(TlsHttpEvent) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut done = false;
        while !done {
            for event in self.host.poll().unwrap() {
                done = accept(event) || done;
            }
            assert!(
                Instant::now() < deadline,
                "actual IOCP fake-record fixture deadline"
            );
            if !done {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    fn head(&mut self) -> (RequestToken, Instant) {
        let mut result = None;
        self.until(|event| {
            if let TlsHttpEvent::Head { head, admitted_at } = event {
                assert!(head.tls);
                result = Some((head.token, admitted_at));
                true
            } else {
                false
            }
        });
        result.unwrap()
    }
    fn body(&mut self) -> (RequestToken, u64, u64, Vec<u8>, bool) {
        let mut result = None;
        self.until(|event| {
            if let TlsHttpEvent::Body {
                token,
                sequence,
                offset,
                bytes,
                eof,
            } = event
            {
                result = Some((token, sequence, offset, bytes, eof));
                true
            } else {
                false
            }
        });
        result.unwrap()
    }
    fn admitted_get(&mut self) -> RequestToken {
        let mut wire = vec![b'H'];
        wire.extend(record(
            b"GET /fc/api/v1/materials/download HTTP/1.1\r\nHost: localhost\r\n\r\n",
        ));
        self.input(&wire);
        let (token, at) = self.head();
        self.host
            .admit_body(token, 0, at + Duration::from_secs(120))
            .unwrap();
        let (body_token, sequence, offset, bytes, eof) = self.body();
        assert_eq!(
            (body_token, sequence, offset, bytes.len(), eof),
            (token, 1, 0, 0, true)
        );
        self.host.ack_body(token, 1, 0).unwrap();
        token
    }
    fn revoked_retirement(&mut self, token: RequestToken) {
        let mut closed = false;
        self.until(|event| match event {
            TlsHttpEvent::ResponseComplete(t) if t == token => {
                panic!("revoked response cannot produce physical ResponseComplete")
            }
            TlsHttpEvent::Closed { token: t } if t == token => {
                closed = true;
                false
            }
            TlsHttpEvent::Retired(t) if t == token => {
                assert!(
                    closed,
                    "logical cancellation must precede physical retirement"
                );
                true
            }
            _ => false,
        });
        let slot = &self.host.owner.resources.slots[token.slot()];
        assert!(slot.retired && slot.pending.is_none() && slot.frame.is_some());
        let stream = slot.stream.as_ref().unwrap();
        assert!(stream.response_fence.is_none());
        assert!(stream.tx.iter().all(|byte| *byte == 0));
        assert!(slot
            .frame
            .as_ref()
            .unwrap()
            .input
            .iter()
            .all(|byte| *byte == 0));
        assert!(slot
            .frame
            .as_ref()
            .unwrap()
            .output
            .iter()
            .all(|byte| *byte == 0));
        // SAFETY: the assertions above witness actual IOCP and SSPI retirement.
        assert!(unsafe { (&*slot.memory.bytes.get()).iter().all(|byte| *byte == 0) });
    }
    fn stop(&mut self) {
        self.host.request_stop().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.host.poll_shutdown().unwrap() {
            assert!(
                Instant::now() < deadline,
                "actual native candidate shutdown deadline"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(self.closed.load(Ordering::Acquire), 1);
        assert!(self.host.owner.drained);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.retain_faulted_owner_on_drop {
            // Only the independent controlled-failure child selects this path.
            // Leave the original Host/Owner destructors to retain their exact
            // native resource domain; do not claim normal physical drain.
            assert!(OWNER_FAULTED.load(Ordering::Acquire));
            assert!(!self.host.owner.drained && !self.host.owner.bare_released());
            return;
        }
        if !self.host.owner.drained {
            self.host
                .owner
                .finish()
                .expect("actual fake-record fixture teardown");
        }
    }
}
fn record(bytes: &[u8]) -> Vec<u8> {
    assert!(bytes.len() <= PLAINTEXT_BYTES);
    let mut output = (bytes.len() as u16).to_be_bytes().to_vec();
    output.extend_from_slice(bytes);
    output
}
fn post(bytes: usize) -> Vec<u8> {
    format!("POST /fc/api/v1/materials/uploads/7/bytes HTTP/1.1\r\nHost: localhost\r\nContent-Length: {bytes}\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes()
}
fn response(bytes: usize) -> ResponseHead {
    ResponseHead {
        status: 200,
        headers: Vec::new(),
        content_length: bytes as u64,
    }
}

#[test]
fn real_iocp_fake_records_keep_head_tail_and_pause_until_exact_body_ack_then_send_all_records() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut first = post(7);
    first.extend_from_slice(b"abc");
    let mut wire = vec![b'H'];
    wire.extend(record(&first));
    wire.extend(record(b"defg"));
    f.input(&wire);
    let (token, admitted) = f.head();
    assert_eq!(f.host.tokens()[token.slot()], token);
    let slot = &f.host.owner.resources.slots[token.slot()];
    let stream = slot.stream.as_ref().unwrap();
    assert!(
        stream.plain_pos < stream.plain_len,
        "coalesced head tail remains in original Frame"
    );
    let before = (
        f.host.owner.report.submissions,
        f.host.owner.report.sspi_submissions,
    );
    for _ in 0..8 {
        assert!(f.host.poll().unwrap().is_empty());
    }
    assert_eq!(
        before,
        (
            f.host.owner.report.submissions,
            f.host.owner.report.sspi_submissions
        )
    );
    assert!(f
        .host
        .admit_body(token, 8, admitted + Duration::from_secs(120))
        .is_err());
    f.host
        .admit_body(token, 7, admitted + Duration::from_secs(120))
        .unwrap();
    let (_, seq, offset, bytes, eof) = f.body();
    assert_eq!((seq, offset, bytes, eof), (1, 0, b"abc".to_vec(), false));
    let before = (
        f.host.owner.report.submissions,
        f.host.owner.report.sspi_submissions,
    );
    assert!(f.host.ack_body(token, 2, 3).is_err());
    assert!(f.host.ack_body(token, 1, 4).is_err());
    for _ in 0..8 {
        assert!(f.host.poll().unwrap().is_empty());
    }
    assert_eq!(
        before,
        (
            f.host.owner.report.submissions,
            f.host.owner.report.sspi_submissions
        )
    );
    f.host.ack_body(token, 1, 3).unwrap();
    let (_, seq, offset, bytes, eof) = f.body();
    assert_eq!((seq, offset, bytes, eof), (2, 3, b"defg".to_vec(), true));
    assert!(f.host.respond(token, response(0), b"", true).is_err());
    f.host.ack_body(token, 2, 7).unwrap();
    // The old finite batch clock must not truncate an admitted business budget.
    f.host.owner.started = Instant::now() - Duration::from_secs(10);
    let mut forged = response(CHUNK_BYTES);
    forged.headers.push(fc_ports::http::Header {
        name: "dAtE".into(),
        value: b"Sun, 06 Nov 1994 08:49:37 GMT".to_vec(),
    });
    let before = (
        f.host.owner.report.submissions,
        f.host.owner.report.sspi_submissions,
    );
    assert!(f.host.respond(token, forged, b"", false).is_err());
    assert!(f.host.owner.resources.slots[token.slot()]
        .response_body
        .is_none());
    assert_eq!(
        before,
        (
            f.host.owner.report.submissions,
            f.host.owner.report.sspi_submissions
        )
    );
    f.host
        .respond(token, response(CHUNK_BYTES), b"", false)
        .unwrap();
    f.until(|event| matches!(event,TlsHttpEvent::ResponseReady(t) if t==token));
    assert_eq!(
        f.calls.lock().unwrap().encrypt.len(),
        1,
        "real header sent before response-ready"
    );
    let payload = vec![b'z'; CHUNK_BYTES];
    f.host.send_response_body(token, &payload, true).unwrap();
    assert!(f.host.send_response_body(token, b"x", false).is_err());
    let mut response_complete = false;
    let mut retired = false;
    let mut closed = false;
    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !retired
        || !received
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .is_some_and(|pos| received.len() > pos + 4 + CHUNK_BYTES)
    {
        for event in f.host.poll().unwrap() {
            match event {
                TlsHttpEvent::ResponseComplete(t) if t == token => response_complete = true,
                TlsHttpEvent::Closed { token: t } if t == token => closed = true,
                TlsHttpEvent::Retired(t) if t == token => retired = true,
                _ => {}
            }
        }
        let mut buf = [0; 8192];
        loop {
            match f.client.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => received.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                other => panic!("fake-record read: {other:?}"),
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(response_complete && closed);
    let head = received.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let headers = std::str::from_utf8(&received[..head]).unwrap();
    assert_eq!(headers.matches("\r\nDate: ").count(), 1);
    let date = headers
        .lines()
        .find_map(|line| line.strip_prefix("Date: "))
        .unwrap();
    assert_eq!(date.len(), 29);
    assert!(date.ends_with(" GMT"));
    assert_eq!(&received[head..head + CHUNK_BYTES], payload);
    assert_eq!(&received[head + CHUNK_BYTES..], b"!");
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls.encrypt.len(), 5);
    assert!(calls.encrypt.iter().all(|len| *len <= PLAINTEXT_BYTES));
    assert!(calls.retire.contains(&token));
    drop(calls);
    let next = f.host.rearm(token).unwrap();
    assert_eq!(next.slot(), token.slot());
    assert_eq!(next.generation(), token.generation());
    assert!(next.serial() > token.serial());
    assert!(f.host.ack_body(token, 2, 7).is_err());
    assert!(f.host.cancel(token).is_err());
    f.host.cancel(next).unwrap();
    f.until(|event| matches!(event,TlsHttpEvent::Retired(t) if t==next));
    f.stop();
}

#[test]
fn real_iocp_fake_records_zero_body_extra_pipeline_and_early_eof_never_become_ready() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut wire = vec![b'H'];
    wire.extend(record(b"GET /fc/ HTTP/1.1\r\nHost: localhost\r\n\r\n"));
    f.input(&wire);
    let (token, at) = f.head();
    assert!(f.host.respond(token, response(0), b"", true).is_err());
    f.host
        .admit_body(token, 0, at + Duration::from_secs(120))
        .unwrap();
    let (_, sequence, offset, bytes, eof) = f.body();
    assert_eq!((sequence, offset, bytes.len(), eof), (1, 0, 0, true));
    assert!(f
        .host
        .admit_body(token, 0, at + Duration::from_secs(120))
        .is_err());
    f.host.ack_body(token, 1, 0).unwrap();
    f.host.respond(token, response(0), b"", true).unwrap();
    f.until(|event| matches!(event,TlsHttpEvent::Retired(t) if t==token));
    f.stop();
    drop(f);

    let mut f = Fixture::new();
    let mut request = post(3);
    request.extend_from_slice(b"abc");
    let mut wire = vec![b'H'];
    wire.extend(record(&request));
    wire.extend(record(b"GET /pipeline HTTP/1.1\r\n\r\n"));
    f.input(&wire);
    let (token, at) = f.head();
    f.host
        .admit_body(token, 3, at + Duration::from_secs(120))
        .unwrap();
    let mut final_body = false;
    f.until(|event| match event {
        TlsHttpEvent::Body { eof: true, .. } => {
            final_body = true;
            false
        }
        TlsHttpEvent::Closed { token: t } if t == token => true,
        _ => false,
    });
    assert!(
        !final_body,
        "already-buffered extra record is rejected before final body admission"
    );
    f.stop();
    drop(f);

    let mut f = Fixture::new();
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let (token, at) = f.head();
    f.host
        .admit_body(token, 3, at + Duration::from_secs(120))
        .unwrap();
    f.client.shutdown(std::net::Shutdown::Write).unwrap();
    f.until(|event| matches!(event,TlsHttpEvent::Closed {token:t} if t==token));
    assert!(f.host.respond(token, response(0), b"", true).is_err());
    f.stop();
}

#[test]
fn real_iocp_fake_records_deadline_is_absolute_and_closed_event_precedes_retirement_reuse() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let (token, at) = f.head();
    assert!(f
        .host
        .admit_body(token, 3, at + Duration::from_secs(121))
        .is_err());
    let deadline = Instant::now() + Duration::from_millis(50);
    f.host.admit_body(token, 3, deadline).unwrap();
    assert_eq!(
        f.host.owner.resources.slots[token.slot()]
            .stream
            .as_ref()
            .unwrap()
            .deadline,
        deadline
    );
    assert!(f.host.rearm(token).is_err());
    let mut closed = false;
    let mut retired = false;
    f.until(|event| match event {
        TlsHttpEvent::Closed { token: t } if t == token => {
            closed = true;
            false
        }
        TlsHttpEvent::Retired(t) if t == token => {
            assert!(closed);
            retired = true;
            true
        }
        _ => false,
    });
    assert!(retired);
    let next = f.host.rearm(token).unwrap();
    assert_ne!(next, token);
    f.stop();
}

#[test]
fn header_completion_transfers_one_absolute_budget_and_admission_only_tightens_it() {
    // Inject absolute instants into the private state transition. This is a
    // boundary test, not evidence of a real five-second socket wait.
    let mut stream = Stream::new();
    stream.posted_header_at(Instant::now());
    let header_deadline = stream.deadline;
    let completed_at = header_close_alarm(header_deadline) - Duration::from_nanos(1);
    assert!(stream.complete_head_at(completed_at));
    let original = completed_at + REQUEST_BUDGET;
    assert_eq!(stream.request_started_at(), Some(completed_at));
    assert_eq!(stream.deadline, original);
    assert!(!stream.expired_at(header_deadline + Duration::from_nanos(1)));
    assert!(!stream.complete_head_at(completed_at + Duration::from_secs(1)));
    assert_eq!(
        stream.deadline, original,
        "a second head cannot renew the request"
    );
    assert!(!stream.admission_deadline_valid(original + Duration::from_nanos(1), completed_at));
    assert!(!stream.admission_deadline_valid(completed_at, completed_at));
    assert!(!stream.admission_deadline_valid(original, original));
    let shorter = completed_at + Duration::from_secs(30);
    assert!(stream.admission_deadline_valid(shorter, completed_at));
    stream.admit_until(shorter);
    assert_eq!(stream.deadline, shorter);
    assert_eq!(stream.request_started_at(), Some(completed_at));
    assert!(stream.expired_at(shorter));
    assert!(!stream.admission_deadline_valid(original, completed_at));
    stream.reset();
    assert_eq!(stream.deadline_phase, DeadlinePhase::AwaitingConnection);
    assert_eq!(stream.request_started_at(), None);
    assert!(!stream.admitted);
}

#[test]
fn a_head_completed_at_or_after_the_original_header_deadline_never_starts_a_request() {
    for lateness in [Duration::ZERO, Duration::from_nanos(1)] {
        let mut stream = Stream::new();
        stream.posted_header_at(Instant::now());
        let deadline = stream.deadline;
        assert!(!stream.complete_head_at(deadline + lateness));
        assert_eq!(stream.deadline, deadline);
        assert_eq!(stream.deadline_phase, DeadlinePhase::Header);
        assert_eq!(stream.request_started_at(), None);
        assert!(!stream.admitted);
    }
}

#[test]
fn real_iocp_complete_head_ends_header_budget_before_body_admission_and_keeps_one_start() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let (token, at) = f.head();
    let index = token.slot();
    let original = at + REQUEST_BUDGET;
    let stream = f.host.owner.resources.slots[index].stream.as_ref().unwrap();
    assert_eq!(
        stream.deadline_phase,
        DeadlinePhase::Request { admitted_at: at }
    );
    assert_eq!(stream.deadline, original);
    assert!(!stream.admitted);
    assert!(stream.flight.is_none());
    // Scenario time is injected only into the predicate. No claim that the
    // native fixture physically waited past the old five-second window.
    assert!(!stream.expired_at(at + BUDGET + Duration::from_nanos(1)));
    for _ in 0..4 {
        assert!(f.host.poll().unwrap().is_empty());
    }
    assert!(f
        .host
        .admit_body(token, 3, original + Duration::from_nanos(1))
        .is_err());
    assert!(f
        .host
        .admit_body(token, 3, Instant::now() - Duration::from_nanos(1))
        .is_err());
    assert!(f.host.admit_body(token, 4, original).is_err());
    let foreign = TokenIssuer::new().unwrap().issue(index).unwrap();
    assert!(f.host.admit_body(foreign, 3, original).is_err());
    assert_eq!(
        f.host.owner.resources.slots[index]
            .stream
            .as_ref()
            .unwrap()
            .deadline,
        original
    );
    // Inject a prior absolute clamp to verify admission cannot replace an
    // already shorter deadline with an otherwise valid original+120 limit.
    let shorter = at + Duration::from_secs(30);
    f.host.owner.resources.slots[index]
        .stream
        .as_mut()
        .unwrap()
        .deadline = shorter;
    assert!(f.host.admit_body(token, 3, original).is_err());
    assert!(
        !f.host.owner.resources.slots[index]
            .stream
            .as_ref()
            .unwrap()
            .admitted
    );
    f.host.admit_body(token, 3, shorter).unwrap();
    f.input(&record(b"abc"));
    let (body_token, sequence, offset, bytes, eof) = f.body();
    assert_eq!(
        (body_token, sequence, offset, bytes, eof),
        (token, 1, 0, b"abc".to_vec(), true)
    );
    f.host.ack_body(token, sequence, 3).unwrap();
    let stream = f.host.owner.resources.slots[index].stream.as_ref().unwrap();
    assert_eq!(stream.request_started_at(), Some(at));
    assert_eq!(
        stream.deadline, shorter,
        "body IO/ACK cannot renew the budget"
    );
    f.host.respond(token, response(0), b"", true).unwrap();
    complete_response_wire(&mut f, token);
    let next = f.host.rearm(token).unwrap();
    assert_eq!(next.slot(), index);
    let stream = f.host.owner.resources.slots[index].stream.as_ref().unwrap();
    assert_eq!(stream.deadline_phase, DeadlinePhase::AwaitingConnection);
    assert_eq!(stream.request_started_at(), None);
    assert!(!stream.admitted);
    assert!(f.host.admit_body(token, 3, original).is_err());
    f.stop();
}

#[test]
fn real_iocp_wait_past_original_post_header_deadline_then_admit_without_renewing_request() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let (token, at) = f.head();
    let index = token.slot();
    let posted_header_deadline = f.posted_header_deadlines[index];
    let original_request_deadline = at + REQUEST_BUDGET;
    assert!(at < posted_header_deadline);
    assert!(
        !f.host.owner.resources.slots[index]
            .stream
            .as_ref()
            .unwrap()
            .admitted
    );
    // This deliberately exercises real elapsed time past this exact slot's
    // former post+5s deadline. It is fake record protection, not Schannel or a
    // server ESTABLISHED timestamp / strict first-TCP deadline proof.
    std::thread::sleep(
        posted_header_deadline.saturating_duration_since(Instant::now())
            + Duration::from_millis(50),
    );
    assert!(Instant::now() >= posted_header_deadline);
    for event in f.host.poll().unwrap() {
        match event {
            TlsHttpEvent::Closed { token: t } if t == token => {
                panic!("complete Head remained subject to old post+5s deadline")
            }
            TlsHttpEvent::Body { token: t, .. } if t == token => {
                panic!("body published before admission")
            }
            _ => {}
        }
    }
    let slot = &f.host.owner.resources.slots[index];
    assert!(slot.phase == Phase::Http && !slot.retired);
    let stream = slot.stream.as_ref().unwrap();
    assert_eq!(stream.deadline, original_request_deadline);
    assert_eq!(stream.request_started_at(), Some(at));
    f.host
        .admit_body(token, 3, original_request_deadline)
        .unwrap();
    f.input(&record(b"abc"));
    let (body_token, sequence, offset, bytes, eof) = f.body();
    assert_eq!(
        (body_token, sequence, offset, bytes, eof),
        (token, 1, 0, b"abc".to_vec(), true)
    );
    f.host.ack_body(token, sequence, 3).unwrap();
    let stream = f.host.owner.resources.slots[index].stream.as_ref().unwrap();
    assert_eq!(stream.deadline, original_request_deadline);
    assert_eq!(stream.request_started_at(), Some(at));
    f.stop();
}

#[test]
fn real_iocp_unadmitted_request_expiry_discards_a_queued_head_and_cannot_be_renewed() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    // Poll the real IOCP/mailbox owner without taking the queued event, so
    // expiry must discard a Head that has never reached the caller.
    let until = Instant::now() + Duration::from_secs(3);
    let token = loop {
        f.host.owner.poll().unwrap();
        if let Some(token) = f.host.owner.resources.slots.iter().find_map(|slot| {
            matches!(
                slot.stream.as_ref().unwrap().event,
                Some(TlsHttpEvent::Head { .. })
            )
            .then_some(slot.token)
        }) {
            break token;
        }
        assert!(Instant::now() < until, "actual native queued Head");
        std::thread::sleep(Duration::from_millis(1));
    };
    let stream = f.host.owner.resources.slots[token.slot()]
        .stream
        .as_mut()
        .unwrap();
    let at = stream.request_started_at().unwrap();
    assert_eq!(stream.deadline, at + REQUEST_BUDGET);
    assert!(!stream.admitted);
    assert!(!stream.expired_at(at + BUDGET + Duration::from_nanos(1)));
    assert!(matches!(stream.event, Some(TlsHttpEvent::Head { .. })));
    // Absolute deadline injection substitutes for a 120-second test sleep.
    stream.deadline = Instant::now() - Duration::from_nanos(1);
    assert!(f.host.admit_body(token, 3, at + REQUEST_BUDGET).is_err());
    let mut closed = false;
    f.until(|event| match event {
        TlsHttpEvent::Head { head, .. } if head.token == token => {
            panic!("expired queued Head escaped")
        }
        TlsHttpEvent::Body { token: t, .. } if t == token => {
            panic!("unadmitted expired body escaped")
        }
        TlsHttpEvent::Closed { token: t } if t == token => {
            closed = true;
            false
        }
        TlsHttpEvent::Retired(t) if t == token => {
            assert!(closed);
            true
        }
        _ => false,
    });
    assert!(f
        .host
        .admit_body(token, 3, Instant::now() + REQUEST_BUDGET)
        .is_err());
    f.stop();
}

#[test]
fn real_iocp_late_decrypt_cannot_publish_a_head_or_release_its_slot_before_actual_return() {
    let _serial = SERIAL.lock().unwrap();
    let (entered, started) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let hook = Arc::new(move || {
        entered.send(()).unwrap();
        released
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
    });
    let mut f = Fixture::with_hooks(None, Some(hook));
    // Declared after Fixture so unwind releases the fake driver before native
    // fixture teardown tries to join its original physical worker.
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(send) = self.0.take() {
                let _ = send.send(());
            }
        }
    }
    let mut release = Release(Some(release));
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        for event in f.host.poll().unwrap() {
            assert!(!matches!(event, TlsHttpEvent::Head { .. }));
        }
        if started.try_recv().is_ok() {
            break;
        }
        assert!(Instant::now() < until, "actual fake decrypt started");
        std::thread::sleep(Duration::from_millis(1));
    }
    let token = *f.calls.lock().unwrap().handshake.first().unwrap();
    let slot = &mut f.host.owner.resources.slots[token.slot()];
    assert!(slot.frame.is_none());
    let stream = slot.stream.as_mut().unwrap();
    assert_eq!(stream.deadline_phase, DeadlinePhase::Header);
    stream.deadline = Instant::now() - Duration::from_nanos(1);
    let mut closed = false;
    for event in f.host.poll().unwrap() {
        match event {
            TlsHttpEvent::Closed { token: t } if t == token => closed = true,
            TlsHttpEvent::Head { head, .. } if head.token == token => panic!("late Head escaped"),
            TlsHttpEvent::Retired(t) if t == token => panic!("in-flight driver was not returned"),
            _ => {}
        }
    }
    assert!(closed);
    assert_eq!(f.host.owner.report.parsed_requests, 0);
    assert!(f.host.rearm(token).is_err());
    let slot = &f.host.owner.resources.slots[token.slot()];
    assert!(slot.frame.is_none() && !slot.retired);
    release.0.take().unwrap().send(()).unwrap();
    f.until(|event| match event {
        TlsHttpEvent::Head { head, .. } if head.token == token => {
            panic!("late returned plaintext escaped")
        }
        TlsHttpEvent::Retired(t) if t == token => true,
        _ => false,
    });
    assert_eq!(f.host.owner.report.parsed_requests, 0);
    assert_eq!(f.host.owner.report.late_sspi_completions, 1);
    let next = f.host.rearm(token).unwrap();
    assert_eq!(next.slot(), token.slot());
    assert_ne!(next, token);
    f.stop();
}

#[test]
fn real_iocp_response_fence_rejects_false_or_panicking_guard_before_response_admission() {
    let _serial = SERIAL.lock().unwrap();
    for panics in [false, true] {
        let mut f = Fixture::new();
        let token = f.admitted_get();
        let foreign = TokenIssuer::new().unwrap().issue(token.slot()).unwrap();
        assert!(f
            .host
            .set_response_fence(foreign, Arc::new(|| true))
            .is_err());
        let fence: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            assert!(!panics, "synthetic response authorization panic");
            false
        });
        let weak = Arc::downgrade(&fence);
        f.host.set_response_fence(token, fence).unwrap();
        assert!(f.host.set_response_fence(token, Arc::new(|| true)).is_err());
        let submissions = f.host.owner.report.submissions;
        assert!(f.host.respond(token, response(3), b"abc", true).is_err());
        assert!(
            weak.upgrade().is_some(),
            "logical close retains guard until actual retirement"
        );
        f.revoked_retirement(token);
        assert!(weak.upgrade().is_none());
        assert!(f.calls.lock().unwrap().encrypt.is_empty());
        assert_eq!(f.host.owner.report.submissions, submissions);
        let next = f.host.rearm(token).unwrap();
        assert!(f.host.set_response_fence(token, Arc::new(|| true)).is_err());
        assert_ne!(next, token);
        f.stop();
    }
}

#[test]
fn real_iocp_response_fence_rechecks_after_sspi_before_first_and_next_record_send() {
    let _serial = SERIAL.lock().unwrap();
    for revoke_encrypt in [1, 2] {
        let allowed = Arc::new(AtomicBool::new(true));
        let worker_allowed = allowed.clone();
        let hook = Arc::new(move |ordinal| {
            if ordinal == revoke_encrypt {
                worker_allowed.store(false, Ordering::Release);
            }
        });
        let mut f = Fixture::with_encrypt_hook(Some(hook));
        let token = f.admitted_get();
        f.host
            .set_response_fence(token, Arc::new(move || allowed.load(Ordering::Acquire)))
            .unwrap();
        let submissions = f.host.owner.report.submissions;
        // Acceptance succeeds while allowed; the real SSPI mailbox/thread then
        // revokes before returning an encrypted record to the Net owner.
        f.host
            .respond(token, response(CHUNK_BYTES), &vec![b'z'; CHUNK_BYTES], true)
            .unwrap();
        f.revoked_retirement(token);
        assert_eq!(f.calls.lock().unwrap().encrypt.len(), revoke_encrypt);
        assert_eq!(
            f.host.owner.report.submissions - submissions,
            revoke_encrypt - 1,
            "only records authorized at the actual WSASend boundary were submitted"
        );
        assert!(f.calls.lock().unwrap().retire.contains(&token));
        f.stop();
    }
}

#[test]
fn real_iocp_response_fence_rejects_next_chunk_after_physical_header_ack() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let token = f.admitted_get();
    let allowed = Arc::new(AtomicBool::new(true));
    let check = allowed.clone();
    f.host
        .set_response_fence(token, Arc::new(move || check.load(Ordering::Acquire)))
        .unwrap();
    f.host.respond(token, response(3), b"", false).unwrap();
    f.until(|event| matches!(event, TlsHttpEvent::ResponseReady(t) if t == token));
    assert!(f.host.set_response_fence(token, Arc::new(|| true)).is_err());
    let submissions = f.host.owner.report.submissions;
    allowed.store(false, Ordering::Release);
    assert!(f.host.send_response_body(token, b"abc", true).is_err());
    f.revoked_retirement(token);
    assert_eq!(f.host.owner.report.submissions, submissions);
    assert_eq!(f.calls.lock().unwrap().encrypt.len(), 1);
    f.stop();
}

#[test]
fn real_iocp_response_fence_allows_complete_response_and_does_not_gate_close_notify() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let token = f.admitted_get();
    let checks = Arc::new(AtomicUsize::new(0));
    let observed = checks.clone();
    f.host
        .set_response_fence(
            token,
            Arc::new(move || {
                // Only response admission and its sole physical WSASend are allowed.
                // A mistaken third check for close_notify would return false.
                observed.fetch_add(1, Ordering::AcqRel) < 2
            }),
        )
        .unwrap();
    f.host.respond(token, response(3), b"abc", true).unwrap();
    let mut complete = false;
    f.until(|event| match event {
        TlsHttpEvent::ResponseComplete(t) if t == token => {
            complete = true;
            false
        }
        TlsHttpEvent::Retired(t) if t == token => true,
        _ => false,
    });
    assert!(complete);
    assert_eq!(checks.load(Ordering::Acquire), 2);
    assert_eq!(f.host.owner.report.shutdown_tokens_sent, 1);
    f.stop();
}

fn parsed_head_request(f: &mut Fixture) -> RequestToken {
    let mut wire = vec![b'H'];
    wire.extend(record(
        b"HEAD /fc/missing HTTP/1.1\r\nHost: localhost\r\n\r\n",
    ));
    f.input(&wire);
    let (token, _) = f.head();
    assert!(
        f.host.owner.resources.slots[token.slot()]
            .stream
            .as_ref()
            .unwrap()
            .head_only
    );
    token
}

fn complete_response_wire(f: &mut Fixture, token: RequestToken) -> Vec<u8> {
    let mut complete = false;
    f.until(|event| match event {
        TlsHttpEvent::ResponseReady(t) if t == token => {
            panic!("one-shot response cannot request another source chunk")
        }
        TlsHttpEvent::ResponseComplete(t) if t == token => {
            complete = true;
            false
        }
        TlsHttpEvent::Retired(t) if t == token => {
            assert!(complete, "physical response ACK must precede retirement");
            true
        }
        _ => false,
    });
    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut bytes = [0; 4096];
        match f.client.read(&mut bytes) {
            Ok(0) => return received,
            Ok(count) => received.extend_from_slice(&bytes[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "actual fake-record client EOF");
                std::thread::sleep(Duration::from_millis(1));
            }
            other => panic!("actual fake-record response read: {other:?}"),
        }
    }
}

#[test]
fn real_iocp_head_errors_send_only_date_headers_and_wait_for_actual_header_ack() {
    let _serial = SERIAL.lock().unwrap();
    for status in [405, 404, 403] {
        let mut f = Fixture::new();
        let token = parsed_head_request(&mut f);
        let mut response = response(77);
        response.status = status;
        if status == 405 {
            response.headers.push(fc_ports::http::Header {
                name: "Allow".into(),
                value: b"GET".to_vec(),
            });
        }
        let mut forged = response.clone();
        forged.headers.push(fc_ports::http::Header {
            name: "dAtE".into(),
            value: b"forged".to_vec(),
        });
        let before = f.host.owner.report.sspi_submissions;
        assert!(f
            .host
            .respond(token, forged, b"secret-error-body", true)
            .is_err());
        assert_eq!(f.host.owner.report.sspi_submissions, before);
        let foreign = TokenIssuer::new().unwrap().issue(token.slot()).unwrap();
        assert!(f
            .host
            .respond(foreign, response.clone(), b"secret-error-body", true)
            .is_err());
        // Neither the supplied first bytes, their mismatch with the selected
        // representation length, nor a non-final source may create a HEAD body.
        f.host
            .respond(token, response, b"secret-error-body", status == 405)
            .unwrap();
        let slot = &f.host.owner.resources.slots[token.slot()];
        let stream = slot.stream.as_ref().unwrap();
        assert_eq!(stream.tx_len, 0);
        assert!(stream.tx_final && !stream.awaiting_response);
        assert!(stream.tx.iter().all(|byte| *byte == 0));
        assert!(slot.response_chunk.is_none());
        assert_eq!(slot.response_body.as_ref().unwrap().remaining_bytes(), 0);
        assert_eq!(f.host.owner.report.responses, 0);
        assert!(f
            .host
            .send_response_body(token, b"late-secret", true)
            .is_err());
        assert!(f.host.send_response_body(token, b"", true).is_err());
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            // poll_io runs before the SSPI mailbox. The poll that obtains the
            // encrypted head submits WSASend but cannot also dequeue its ACK.
            for event in f.host.poll().unwrap() {
                assert!(!matches!(event,
                    TlsHttpEvent::ResponseComplete(t) | TlsHttpEvent::ResponseReady(t)
                        if t == token));
            }
            if f.host.owner.resources.slots[token.slot()].pending == Some(Operation::Send) {
                break;
            }
            assert!(Instant::now() < deadline, "actual HEAD WSASend submission");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            f.host.owner.report.responses, 0,
            "unconsumed IOCP is not an ACK"
        );
        assert!(f.host.owner.resources.slots[token.slot()].phase == Phase::Response);
        let received = complete_response_wire(&mut f, token);
        let end = received
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let headers = std::str::from_utf8(&received[..end]).unwrap();
        assert!(headers.starts_with(&format!("HTTP/1.1 {status} ")));
        assert!(!headers.contains("Content-Length:"));
        assert_eq!(headers.matches("\r\nDate: ").count(), 1);
        if status == 405 {
            assert!(headers.contains("\r\nAllow: GET\r\n"));
        }
        assert_eq!(
            &received[end..],
            b"!",
            "only fake close_notify follows HEAD headers"
        );
        assert_eq!(f.host.owner.report.responses, 1);
        f.stop();
    }
}

#[test]
fn real_iocp_head_marker_clears_on_exact_slot_reuse_so_get_keeps_its_body() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let token = parsed_head_request(&mut f);
    let mut error = response(7);
    error.status = 404;
    f.host.respond(token, error, b"ignored", false).unwrap();
    let received = complete_response_wire(&mut f, token);
    assert!(received.ends_with(b"\r\n\r\n!"));
    // Retire the other Ready business slots so the new bare acceptance must
    // use the exact rearmed HEAD slot; Ready slots have no accept I/O of their own.
    for other in f.host.tokens().into_iter().filter(|other| *other != token) {
        f.host.cancel(other).unwrap();
    }
    let mut other_retired = 0;
    f.until(|event| {
        if matches!(event, TlsHttpEvent::Retired(t) if t != token) {
            other_retired += 1;
        }
        other_retired == CONNECTION_SLOTS - 1
    });
    let next = f.host.rearm(token).unwrap();
    assert_eq!(next.slot(), token.slot());
    assert!(
        !f.host.owner.resources.slots[next.slot()]
            .stream
            .as_ref()
            .unwrap()
            .head_only
    );
    f.client = TcpStream::connect_timeout(
        &SocketAddrV4::new(Ipv4Addr::LOCALHOST, f.host.owner.port_number).into(),
        Duration::from_secs(1),
    )
    .unwrap();
    f.client.set_nonblocking(true).unwrap();
    let actual = f.admitted_get();
    assert_eq!(actual, next);
    let payload = b"GET-keeps-its-body";
    f.host
        .respond(next, response(payload.len()), payload, true)
        .unwrap();
    let received = complete_response_wire(&mut f, next);
    let end = received
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    let headers = std::str::from_utf8(&received[..end]).unwrap();
    assert!(headers.contains(&format!("\r\nContent-Length: {}\r\n", payload.len())));
    assert_eq!(headers.matches("\r\nDate: ").count(), 1);
    assert_eq!(&received[end..end + payload.len()], payload);
    assert_eq!(&received[end + payload.len()..], b"!");
    assert!(f.host.send_response_body(token, b"stale", true).is_err());
    f.stop();
}

#[test]
fn real_iocp_head_does_not_bypass_a_revoked_response_fence() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let token = parsed_head_request(&mut f);
    f.host
        .set_response_fence(token, Arc::new(|| false))
        .unwrap();
    let before = f.host.owner.report.sspi_submissions;
    let mut error = response(23);
    error.status = 403;
    assert!(f.host.respond(token, error, b"ignored", false).is_err());
    assert_eq!(f.host.owner.report.sspi_submissions, before);
    assert!(f.calls.lock().unwrap().encrypt.is_empty());
    f.revoked_retirement(token);
    assert!(f.host.send_response_body(token, b"late", true).is_err());
    f.stop();
}

fn extra_client(f: &Fixture) -> TcpStream {
    let client = TcpStream::connect_timeout(
        &SocketAddrV4::new(Ipv4Addr::LOCALHOST, f.host.port_number()).into(),
        Duration::from_secs(1),
    )
    .unwrap();
    client.set_nonblocking(true).unwrap();
    client
}
fn rejected_client(f: &mut Fixture, client: &mut TcpStream) {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        for event in f.host.poll().unwrap() {
            assert!(!matches!(
                event,
                TlsHttpEvent::Head { .. } | TlsHttpEvent::Body { .. }
            ));
        }
        let mut byte = [0];
        match client.read(&mut byte) {
            Ok(0) => break,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ) =>
            {
                break
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("capacity/expired rejection must close, not send HTTP/TLS: {other:?}"),
        }
        assert!(Instant::now() < until, "actual rejected TCP close");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn ready_business_slots_have_no_native_operation_or_active_header_clock() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    assert_eq!(f.host.owner.report.submissions, 1);
    assert_eq!(f.host.owner.report.accept_posts, 1);
    let bare = f.host.owner.resources.accept.as_ref().unwrap();
    assert!(bare.pending());
    assert_eq!(bare.serial, 1);
    for slot in &f.host.owner.resources.slots {
        assert!(
            slot.phase == Phase::Ready && slot.socket == INVALID_SOCKET && slot.pending.is_none()
        );
        assert!(slot.frame.is_some() && !slot.context_submitted && !slot.retired);
        let stream = slot.stream.as_ref().unwrap();
        assert!(stream.waiting_for_connection());
        assert!(!stream.expired_at(Instant::now() + REQUEST_BUDGET));
    }
    for token in f.host.tokens() {
        assert!(f
            .host
            .admit_body(token, 0, Instant::now() + REQUEST_BUDGET)
            .is_err());
    }
    let expected = size_of::<Owner>()
        + size_of::<super::super::accept::AcceptMemory>()
        + CONNECTION_SLOTS * (size_of::<IoMemory>() + size_of::<Frame>())
        + f.host.owner.target.capacity()
        + f.host.owner.marker.capacity()
        + f.host
            .owner
            .resources
            .slots
            .iter()
            .map(|slot| {
                slot.parser.header_capacity() + slot.stream.as_ref().unwrap().allocation_bytes()
            })
            .sum::<usize>();
    assert_eq!(f.host.owner.app_owned_bytes(), expected);
    f.stop();
}

#[test]
fn real_iocp_bare_handoff_near_original_post_deadline_never_gets_handoff_plus_five() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let original = f
        .host
        .owner
        .resources
        .accept
        .as_ref()
        .unwrap()
        .deadline()
        .unwrap();
    // This is a real elapsed wait against the actual posted bare operation,
    // not Stream::new or a claimed exact TCP ESTABLISHED timestamp.
    std::thread::sleep(
        original
            .saturating_duration_since(Instant::now())
            .saturating_sub(Duration::from_millis(500)),
    );
    f.host.owner.poll_io().unwrap();
    assert_eq!(f.host.owner.report.accept_handoffs, 1);
    let slot = f
        .host
        .owner
        .resources
        .slots
        .iter()
        .find(|slot| slot.phase == Phase::Handshake)
        .unwrap();
    assert_eq!(slot.stream.as_ref().unwrap().deadline, original);
    assert!(original.saturating_duration_since(Instant::now()) < Duration::from_secs(1));
    assert!(f.host.owner.resources.accept.as_ref().unwrap().serial > 1);
    f.stop();
}

#[test]
fn real_iocp_expired_queued_accept_is_discarded_before_any_business_handoff() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    // TCP connect completed but its actual AcceptEx packet has not been read.
    // Inject an expired original post deadline; this does not claim real5s wait.
    let bare = f.host.owner.resources.accept.as_mut().unwrap();
    let serial = bare.serial;
    bare.state = super::super::accept::AcceptState::Pending {
        serial,
        deadline: Instant::now() - Duration::from_nanos(1),
    };
    let until = Instant::now() + Duration::from_secs(3);
    while f.host.owner.report.accept_expired_discards == 0 {
        assert!(f.host.poll().unwrap().is_empty());
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(f.host.owner.report.accepted, 0);
    assert_eq!(f.host.owner.report.sspi_submissions, 0);
    assert_eq!(f.host.owner.report.accept_completions, 1);
    assert_eq!(
        f.host.owner.resources.accept.as_ref().unwrap().serial,
        serial + 1
    );
    assert!(f
        .host
        .owner
        .resources
        .slots
        .iter()
        .all(|slot| slot.phase == Phase::Ready));
    f.stop();
}

#[test]
fn real_iocp_idle_bare_cancel_keeps_debt_until_packet_then_reposts_same_memory() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let _ = f.head();
    let bare = f.host.owner.resources.accept.as_ref().unwrap();
    let pointer = bare.memory.operation.get();
    let serial = bare.serial;
    let completions = f.host.owner.report.accept_completions;
    f.host.owner.close_bare_accept().unwrap();
    let bare = f.host.owner.resources.accept.as_ref().unwrap();
    assert!(matches!(
        bare.state,
        super::super::accept::AcceptState::Closing { .. }
    ));
    assert!(bare.socket == INVALID_SOCKET && bare.pending());
    assert!(f.host.owner.post_bare_accept().is_err());
    assert_eq!(f.host.owner.report.accept_completions, completions);
    let until = Instant::now() + Duration::from_secs(3);
    while f.host.owner.resources.accept.as_ref().unwrap().serial == serial {
        f.host.poll().unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    let bare = f.host.owner.resources.accept.as_ref().unwrap();
    assert_eq!(bare.memory.operation.get(), pointer);
    assert_eq!(bare.serial, serial + 1);
    assert!(bare.pending());
    assert_eq!(f.host.owner.report.accept_completions, completions + 1);
    assert_eq!(f.host.owner.report.accepted, 1);
    f.stop();
}

#[test]
fn real_iocp_four_busy_business_requests_reject_excess_without_touching_old_requests() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut clients = Vec::new();
    let mut tokens = Vec::new();
    for index in 0..CONNECTION_SLOTS {
        if index != 0 {
            let client = extra_client(&f);
            clients.push(std::mem::replace(&mut f.client, client));
        }
        let mut wire = vec![b'H'];
        wire.extend(record(&post(3)));
        f.input(&wire);
        let (token, at) = f.head();
        f.host.admit_body(token, 3, at + REQUEST_BUDGET).unwrap();
        tokens.push(token);
    }
    let preserved: Vec<_> = tokens
        .iter()
        .map(|token| {
            let slot = &f.host.owner.resources.slots[token.slot()];
            (
                slot.token,
                slot.socket,
                slot.stream.as_ref().unwrap().deadline,
            )
        })
        .collect();
    let sspi = f.host.owner.report.sspi_submissions;
    for expected_rejections in 1..=2 {
        let mut client = extra_client(&f);
        rejected_client(&mut f, &mut client);
        assert_eq!(
            f.host.owner.report.accept_capacity_rejections,
            expected_rejections
        );
        assert_eq!(f.host.owner.report.sspi_submissions, sspi);
        assert_eq!(f.host.owner.report.accepted, CONNECTION_SLOTS);
        for &(token, socket, deadline) in &preserved {
            let slot = &f.host.owner.resources.slots[token.slot()];
            assert_eq!(slot.token, token);
            assert!(slot.socket == socket);
            assert_eq!(slot.stream.as_ref().unwrap().deadline, deadline);
            assert!(slot.phase == Phase::Http && slot.pending == Some(Operation::Receive));
        }
    }
    let old = tokens[0];
    f.host.cancel(old).unwrap();
    f.until(|event| matches!(event, TlsHttpEvent::Retired(t) if t == old));
    let next = f.host.rearm(old).unwrap();
    assert!(f.host.owner.resources.slots[next.slot()].phase == Phase::Ready);
    let client = extra_client(&f);
    clients.push(std::mem::replace(&mut f.client, client));
    let mut wire = vec![b'H'];
    wire.extend(record(&post(0)));
    f.input(&wire);
    assert_eq!(f.head().0, next);
    f.stop();
}

#[test]
fn real_iocp_stop_after_actual_success_packet_does_not_handoff_or_repost() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let (mut bytes, mut key, mut operation) = (0, 0, ptr::null_mut());
    // Obtain a genuine successful AcceptEx completion, then interpose stop
    // before dispatching that exact packet. This does not invent a kernel serial.
    // SAFETY: this fixture owns the IOCP and all pinned operations; the exact
    // returned packet is dispatched once below before any operation reuse.
    unsafe {
        GetQueuedCompletionStatus(
            f.host.owner.resources.port,
            &mut bytes,
            &mut key,
            &mut operation,
            1000,
        )
    }
    .unwrap();
    let bare = f.host.owner.resources.accept.as_ref().unwrap();
    assert_eq!(operation, bare.memory.operation.get());
    assert_eq!(key, 1);
    let serial = bare.serial;
    f.host.request_stop().unwrap();
    assert!(f.host.owner.resources.accept.as_ref().unwrap().pending());
    f.host.owner.complete_bare_accept(key, true).unwrap();
    assert_eq!(f.host.owner.report.accepted, 0);
    assert_eq!(f.host.owner.report.sspi_submissions, 0);
    assert_eq!(f.host.owner.report.accept_stop_discards, 1);
    assert_eq!(
        f.host.owner.resources.accept.as_ref().unwrap().serial,
        serial
    );
    assert!(f.host.owner.bare_released());
    f.stop();
}

#[test]
fn real_iocp_accept_cannot_steal_held_frame_or_unconsumed_retired_business_slots() {
    let _serial = SERIAL.lock().unwrap();
    let (entered, started) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let hook = Arc::new(move || {
        entered.send(()).unwrap();
        released
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
    });
    let mut f = Fixture::with_hooks(None, Some(hook));
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(send) = self.0.take() {
                let _ = send.send(());
            }
        }
    }
    let mut release = Release(Some(release));
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        f.host.poll().unwrap();
        if started.try_recv().is_ok() {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    let held = f.calls.lock().unwrap().handshake[0];
    for token in f.host.tokens() {
        f.host.cancel(token).unwrap();
    }
    f.host.owner.poll().unwrap(); // preserve Closed/Retired events inside original slots
    assert!(f.host.owner.resources.slots[held.slot()].frame.is_none());
    assert!(!f.host.owner.resources.slots[held.slot()].retired);
    for token in f.host.tokens() {
        assert!(f.host.rearm(token).is_err());
    }
    let mut extra = extra_client(&f);
    rejected_client(&mut f, &mut extra);
    assert_eq!(f.host.acceptance_snapshot().capacity_rejects, 1);
    assert_eq!(f.host.acceptance_snapshot().ready_slots, 0);
    // Even once empty-slot retirement events were consumed, an explicit rearm
    // and caller descriptor reservation are required; none is inferred here.
    let mut extra = extra_client(&f);
    rejected_client(&mut f, &mut extra);
    assert_eq!(f.host.acceptance_snapshot().capacity_rejects, 2);
    assert!(f.host.owner.resources.slots[held.slot()].frame.is_none());
    assert_eq!(f.calls.lock().unwrap().handshake.len(), 1);
    release.0.take().unwrap().send(()).unwrap();
    f.until(|event| matches!(event, TlsHttpEvent::Retired(t) if t == held));
    let next = f.host.rearm(held).unwrap();
    assert_eq!(next.slot(), held.slot());
    assert_ne!(next, held);
    assert_eq!(f.host.acceptance_snapshot().ready_slots, 1);
    f.stop();
}

#[test]
fn real_iocp_expired_idle_post_snapshot_counts_request_and_actual_return_separately() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let _ = f.head();
    let before = f.host.acceptance_snapshot();
    assert_eq!(before.transfers, 1);
    assert_eq!(before.native_pending, 1);
    let bare = f.host.owner.resources.accept.as_mut().unwrap();
    bare.state = super::super::accept::AcceptState::Pending {
        serial: bare.serial,
        deadline: Instant::now() - Duration::from_nanos(1),
    };
    f.host.owner.expire_bare_accept(Instant::now()).unwrap();
    let closing = f.host.acceptance_snapshot();
    assert_eq!(closing.idle_cancellations, before.idle_cancellations + 1);
    assert_eq!(closing.idle_returns, before.idle_returns);
    assert_eq!(closing.posts, before.posts);
    assert_eq!(
        closing.native_pending, 1,
        "CancelIoEx/close is not completion"
    );
    let until = Instant::now() + Duration::from_secs(3);
    while f.host.acceptance_snapshot().idle_returns == before.idle_returns {
        f.host.poll().unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    let after = f.host.acceptance_snapshot();
    assert_eq!(after.idle_returns, before.idle_returns + 1);
    assert_eq!(after.posts, before.posts + 1);
    assert_eq!(after.transfers, before.transfers);
    assert!(after.native_pending <= 5 && after.owned_sockets <= 6);
    f.stop();
    let stopped = f.host.acceptance_snapshot();
    assert_eq!(stopped.native_pending, 0);
    assert_eq!(stopped.owned_sockets, 0);
}

#[test]
fn bare_close_failure_retains_exact_owner_and_forbids_replacement_in_child() {
    let _serial = SERIAL.lock().unwrap();
    const CHILD: &str = "FC_TLS_BARE_CLOSE_FAILURE_CHILD";
    const NAME: &str = "tls_frontdoor::native::stream::tests::bare_close_failure_retains_exact_owner_and_forbids_replacement_in_child";
    const MARKER: &str = "BARE_CLOSE_FAILURE_CHILD retainedOwner=true replaced=false cleanupComplete=false controlledCloseSeam=true";
    if std::env::var(CHILD).as_deref() == Ok("1") {
        let mut f = Fixture::new();
        let bare = f.host.owner.resources.accept.as_mut().unwrap();
        let socket = bare.socket;
        let pointer = bare.memory.operation.get();
        let serial = bare.serial;
        bare.fail_close_once = true;
        let error = f.host.owner.close_bare_accept().unwrap_err();
        f.retain_faulted_owner_on_drop = true;
        assert!(error.contains("controlled"));
        let bare = f.host.owner.resources.accept.as_ref().unwrap();
        assert!(bare.pending() && bare.socket == socket);
        assert_eq!(bare.memory.operation.get(), pointer);
        assert_eq!(bare.serial, serial);
        assert!(f.host.owner.post_bare_accept().is_err());
        assert!(!f.host.owner.bare_released());
        assert!(OWNER_FAULTED.load(Ordering::Acquire));
        assert!(Owner::new("forbidden-bare-replacement").is_err());
        // Fault retention is intentional, not a claim that physical drain
        // completed. This independent child is the bounded OS teardown scope.
        drop(f);
        assert!(OWNER_ACTIVE.load(Ordering::Acquire));
        assert!(Owner::new("still-forbidden-bare-replacement").is_err());
        println!("{MARKER}");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", NAME, "--nocapture", "--test-threads=2"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(MARKER));
    assert!(!OWNER_FAULTED.load(Ordering::Acquire));
}

#[test]
fn header_negative_alarm_is_distinct_from_hard_deadline_and_never_shortens_request_stage() {
    let posted = Instant::now();
    let hard = posted + BUDGET;
    let alarm = hard - HEADER_CLOSE_RESERVE;
    for instant in [alarm, alarm + Duration::from_nanos(1), hard] {
        let mut stream = Stream::new();
        stream.posted_header_at(posted);
        assert_eq!(stream.deadline, hard);
        assert!(!stream.negative_close_due_at(alarm - Duration::from_nanos(1)));
        assert!(stream.negative_close_due_at(instant));
        assert_eq!(stream.expired_at(instant), instant >= hard);
        assert!(!stream.complete_head_at(instant));
        assert_eq!(
            stream.deadline, hard,
            "a negative alarm cannot rewrite the hard deadline"
        );
        assert_eq!(stream.request_started_at(), None);
    }
    let mut stream = Stream::new();
    stream.posted_header_at(posted);
    let at = alarm - Duration::from_nanos(1);
    assert!(stream.complete_head_at(at));
    assert_eq!(stream.deadline, at + REQUEST_BUDGET);
    assert!(!stream.negative_close_due_at(stream.deadline - Duration::from_nanos(1)));
    assert!(stream.negative_close_due_at(stream.deadline));
    let shorter = at + Duration::from_secs(30);
    stream.admit_until(shorter);
    assert!(!stream.negative_close_due_at(shorter - Duration::from_nanos(1)));
    assert!(stream.negative_close_due_at(shorter));
    stream.reset();
    assert!(!stream.negative_close_due_at(hard + REQUEST_BUDGET));
}

fn incomplete_header_receive(f: &mut Fixture) -> RequestToken {
    let mut wire = vec![b'H'];
    wire.extend(record(b"GET /fc/ HTTP/1.1\r\nHost: localhost\r\n"));
    f.input(&wire);
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        for event in f.host.poll().unwrap() {
            assert!(
                !matches!(event, TlsHttpEvent::Head { .. }),
                "incomplete head must not publish"
            );
        }
        if let Some(slot) = f
            .host
            .owner
            .resources
            .slots
            .iter()
            .find(|s| s.phase == Phase::Http && s.pending == Some(Operation::Receive))
        {
            assert_eq!(
                slot.stream.as_ref().unwrap().deadline_phase,
                DeadlinePhase::Header
            );
            return slot.token;
        }
        assert!(Instant::now() < until, "actual incomplete-head WSARecv");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn real_iocp_early_header_close_reports_socket_return_without_discharge_of_receive_debt() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let token = incomplete_header_receive(&mut f);
    // Inject a short remaining header interval to exercise the alarm, not a
    // claim that this case physically waited five seconds or established TLS.
    let hard = Instant::now() + HEADER_CLOSE_RESERVE;
    f.host.owner.resources.slots[token.slot()]
        .stream
        .as_mut()
        .unwrap()
        .deadline = hard;
    f.host.owner.close_slot(token.slot(), true).unwrap();
    let snapshot = f.host.close_snapshot();
    let record = snapshot.headers[token.slot()].unwrap();
    assert_eq!(record.token, token);
    assert_eq!(record.hard_deadline, hard);
    assert_eq!(record.alarm + HEADER_CLOSE_RESERVE, hard);
    assert!(record.began_at >= record.alarm);
    assert!(record.socket_returned_at.unwrap() >= record.began_at);
    assert!(!record.failed);
    assert!(record.io_pending_at_close && record.io_returned_at.is_none());
    assert!(!record.frame_pending_at_close && record.frame_returned_at.is_none());
    assert!(record.retirement_confirmed_at.is_none());
    assert_eq!(snapshot.header_begins, 1);
    assert_eq!(snapshot.header_socket_returns, 1);
    assert_eq!(snapshot.header_failures, 0);
    assert!(f.host.owner.resources.slots[token.slot()].pending == Some(Operation::Receive));
    assert!(f.host.rearm(token).is_err());
    f.revoked_retirement(token);
    let record = f.host.close_snapshot().headers[token.slot()].unwrap();
    assert!(record.io_returned_at.unwrap() >= record.socket_returned_at.unwrap());
    assert!(record.retirement_confirmed_at.unwrap() >= record.io_returned_at.unwrap());
    let next = f.host.rearm(token).unwrap();
    assert_ne!(next, token);
    // A later serial cannot rewrite this diagnostic history before its own
    // deadline close. No retained observation is itself an owner/permit.
    assert_eq!(
        f.host.close_snapshot().headers[token.slot()].unwrap().token,
        token
    );
    f.stop();
}

#[test]
fn real_iocp_bare_alarm_preserves_original_deadline_and_requires_actual_packet_before_repost() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    // First handoff leaves the unique bare operation actually pending again.
    let _ = incomplete_header_receive(&mut f);
    let bare = f.host.owner.resources.accept.as_mut().unwrap();
    let serial = bare.serial;
    let pointer = bare.memory.operation.get();
    let hard = Instant::now() + HEADER_CLOSE_RESERVE;
    bare.state = super::super::accept::AcceptState::Pending {
        serial,
        deadline: hard,
    };
    let before = f.host.acceptance_snapshot();
    f.host.owner.expire_bare_accept(Instant::now()).unwrap();
    let closing = f.host.close_snapshot().accept.unwrap();
    assert_eq!(closing.serial, serial);
    assert_eq!(closing.hard_deadline, hard);
    assert_eq!(closing.alarm + HEADER_CLOSE_RESERVE, hard);
    assert!(closing.socket_returned_at.unwrap() >= closing.began_at);
    assert!(closing.completion_returned_at.is_none());
    assert!(!closing.failed);
    assert_eq!(f.host.acceptance_snapshot().completions, before.completions);
    assert!(f.host.owner.resources.accept.as_ref().unwrap().pending());
    assert!(f.host.owner.post_bare_accept().is_err());
    let until = Instant::now() + Duration::from_secs(3);
    while f
        .host
        .close_snapshot()
        .accept
        .unwrap()
        .completion_returned_at
        .is_none()
    {
        f.host.poll().unwrap();
        assert!(Instant::now() < until, "actual original bare IOCP return");
        std::thread::sleep(Duration::from_millis(1));
    }
    let after = f.host.close_snapshot().accept.unwrap();
    assert!(after.completion_returned_at.unwrap() >= after.socket_returned_at.unwrap());
    let bare = f.host.owner.resources.accept.as_ref().unwrap();
    assert_eq!(bare.memory.operation.get(), pointer);
    assert_eq!(bare.serial, serial + 1);
    assert!(bare.pending());
    let capacity = f.host.acceptance_snapshot();
    assert!(capacity.native_pending <= 5 && capacity.owned_sockets <= 6);
    f.stop();
}

#[test]
fn real_iocp_header_alarm_keeps_a_held_decrypt_frame_and_suppresses_its_late_head() {
    let _serial = SERIAL.lock().unwrap();
    let (entered, started) = std::sync::mpsc::channel();
    let (send, wait) = std::sync::mpsc::channel();
    let wait = Mutex::new(wait);
    let hook = Arc::new(move || {
        entered.send(()).unwrap();
        wait.lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
    });
    let mut f = Fixture::with_hooks(None, Some(hook));
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(send) = self.0.take() {
                let _ = send.send(());
            }
        }
    }
    let mut release = Release(Some(send));
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        for event in f.host.poll().unwrap() {
            assert!(!matches!(event, TlsHttpEvent::Head { .. }));
        }
        if started.try_recv().is_ok() {
            break;
        }
        assert!(Instant::now() < until, "actual held fake Decrypt");
        std::thread::sleep(Duration::from_millis(1));
    }
    let token = *f.calls.lock().unwrap().handshake.first().unwrap();
    let hard = Instant::now() + HEADER_CLOSE_RESERVE;
    f.host.owner.resources.slots[token.slot()]
        .stream
        .as_mut()
        .unwrap()
        .deadline = hard;
    let mut closed = false;
    for event in f.host.poll().unwrap() {
        match event {
            TlsHttpEvent::Closed { token: t } if t == token => closed = true,
            TlsHttpEvent::Head { head, .. } if head.token == token => {
                panic!("late Head escaped alarm")
            }
            TlsHttpEvent::Retired(t) if t == token => panic!("held Frame falsely retired"),
            _ => {}
        }
    }
    assert!(closed);
    let record = f.host.close_snapshot().headers[token.slot()].unwrap();
    assert_eq!(record.hard_deadline, hard);
    assert!(record.frame_pending_at_close && record.frame_returned_at.is_none());
    assert!(!record.io_pending_at_close && record.io_returned_at.is_none());
    assert!(record.retirement_confirmed_at.is_none());
    assert!(f.host.owner.resources.slots[token.slot()].frame.is_none());
    assert!(f.host.rearm(token).is_err());
    release.0.take().unwrap().send(()).unwrap();
    f.until(|event| match event {
        TlsHttpEvent::Head { head, .. } if head.token == token => {
            panic!("late plaintext published")
        }
        TlsHttpEvent::Retired(t) if t == token => true,
        _ => false,
    });
    let after = f.host.close_snapshot().headers[token.slot()].unwrap();
    assert!(after.frame_returned_at.unwrap() >= after.socket_returned_at.unwrap());
    assert!(after.retirement_confirmed_at.unwrap() >= after.frame_returned_at.unwrap());
    assert_eq!(f.host.owner.report.parsed_requests, 0);
    assert_eq!(f.host.owner.report.late_sspi_completions, 1);
    f.stop();
}

#[test]
fn real_iocp_first_header_reserve_keeps_original_five_second_wire_sample_strict() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let token = incomplete_header_receive(&mut f);
    let hard = f.host.owner.resources.slots[token.slot()]
        .stream
        .as_ref()
        .unwrap()
        .deadline;
    let until = f.client_connected_at + Duration::from_secs(6);
    let wire_closed_at = loop {
        for event in f.host.poll().unwrap() {
            assert!(!matches!(event, TlsHttpEvent::Head { .. }));
        }
        let mut byte = [0];
        match f.client.read(&mut byte) {
            Ok(0) => break Instant::now(),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                break Instant::now()
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("unexpected incomplete-head wire result {other:?}"),
        }
        assert!(
            Instant::now() < until,
            "six seconds only collects a failure, not a passing threshold"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    let elapsed = wire_closed_at
        .duration_since(f.client_connected_at)
        .as_nanos();
    let record = f.host.close_snapshot().headers[token.slot()].unwrap();
    assert_eq!(record.hard_deadline, hard);
    assert_eq!(record.alarm + HEADER_CLOSE_RESERVE, hard);
    assert!(record.socket_returned_at.unwrap() >= record.began_at);
    // Print before the strict assertion so a genuine overshoot remains visible.
    println!("TLS_HEADER_CLOSE_SAMPLE {{\"wireCloseNs\":{elapsed},\"thresholdNs\":5000000000,\"reserveNs\":{},\"socketReturnObserved\":true,\"peerCloseObserved\":true,\"recordProtection\":\"FAKE\",\"tcpEpoch\":\"CLIENT_CONNECT_RETURN_ONLY\"}}", HEADER_CLOSE_RESERVE.as_nanos());
    assert!(
        elapsed <= 5_000_000_000,
        "strict five-second wire sample {elapsed}ns"
    );
    f.stop();
}

#[test]
fn abortive_option_failure_preserves_original_socket_and_faults_replacement_in_child() {
    let _serial = SERIAL.lock().unwrap();
    const CHILD: &str = "FC_TLS_LINGER_FAILURE_CHILD";
    const NAME: &str = "tls_frontdoor::native::stream::tests::abortive_option_failure_preserves_original_socket_and_faults_replacement_in_child";
    const MARKER: &str = "LINGER_FAILURE_CHILD retainedOwner=true released=false replaced=false cleanupComplete=false controlledOptionFailure=true";
    if std::env::var(CHILD).as_deref() == Ok("1") {
        let mut f = Fixture::new();
        let bare = f.host.owner.resources.accept.as_mut().unwrap();
        let socket = bare.socket;
        let pointer = bare.memory.operation.get();
        let serial = bare.serial;
        let result = close_with(
            &mut bare.socket,
            true,
            |candidate, abortive| {
                assert_eq!(candidate, socket);
                assert!(abortive);
                Err("controlled SO_LINGER abortive close failure".into())
            },
            |_| panic!("release cannot run after abortive-option failure"),
        );
        assert!(result.unwrap_err().contains("SO_LINGER"));
        f.retain_faulted_owner_on_drop = true;
        let bare = f.host.owner.resources.accept.as_ref().unwrap();
        assert_eq!(bare.socket, socket);
        assert!(bare.pending());
        assert_eq!(bare.serial, serial);
        assert_eq!(bare.memory.operation.get(), pointer);
        assert!(OWNER_FAULTED.load(Ordering::Acquire));
        assert!(Owner::new("forbidden-after-option-failure").is_err());
        drop(f);
        assert!(OWNER_ACTIVE.load(Ordering::Acquire));
        assert!(Owner::new("still-forbidden-after-option-failure").is_err());
        println!("{MARKER}");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", NAME, "--nocapture", "--test-threads=2"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(MARKER));
    assert!(!OWNER_FAULTED.load(Ordering::Acquire));
}

#[test]
fn real_iocp_queued_accept_after_negative_alarm_is_rejected_without_new_hard_deadline() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    // The actual first connection is already queued, but no packet has been
    // dequeued by this fixture. Only the remaining interval is injected.
    let bare = f.host.owner.resources.accept.as_mut().unwrap();
    let serial = bare.serial;
    let hard = Instant::now() + HEADER_CLOSE_RESERVE;
    bare.state = super::super::accept::AcceptState::Pending {
        serial,
        deadline: hard,
    };
    let before = f.host.acceptance_snapshot();
    let until = Instant::now() + Duration::from_secs(3);
    while f.host.owner.report.accept_expired_discards == 0 {
        // Deliberately use actual completion handling without the poll's
        // earlier alarm cancellation: late success must recheck on its own.
        f.host.owner.poll_io().unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    let snapshot = f.host.close_snapshot();
    let record = snapshot.accept.unwrap();
    assert_eq!(record.serial, serial);
    assert_eq!(record.hard_deadline, hard);
    assert_eq!(record.alarm + HEADER_CLOSE_RESERVE, hard);
    assert!(record.began_at >= record.alarm);
    assert!(record.completion_returned_at.unwrap() <= record.began_at);
    assert!(record.socket_close_began_at.unwrap() >= record.began_at);
    assert!(record.socket_returned_at.unwrap() >= record.socket_close_began_at.unwrap());
    assert!(!record.failed);
    let after = f.host.acceptance_snapshot();
    assert_eq!(after.transfers, before.transfers);
    assert_eq!(after.ready_slots, CONNECTION_SLOTS);
    assert_eq!(f.host.owner.report.handshake_calls, 0);
    assert_eq!(after.posts, before.posts + 1);
    assert!(after.native_pending <= 5 && after.owned_sockets <= 6);
    let mut client = f.client.try_clone().unwrap();
    rejected_client(&mut f, &mut client);
    f.stop();
}

// Stop/release only this fixture's original bare operation without rearming.
// The queued real connection may be rejected; no business Frame was admitted.
fn release_fixture_bare_for_post_test(f: &mut Fixture) {
    f.host.owner.stopping = true;
    f.host.owner.close_bare_accept().unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while f.host.owner.resources.accept.as_ref().unwrap().pending() {
        f.host.owner.poll_io().unwrap();
        assert!(Instant::now() < until, "original bare IOCP return");
        std::thread::yield_now();
    }
    assert!(f.host.owner.bare_released());
    f.host.owner.stopping = false;
}

#[test]
fn real_bare_post_charges_preparation_against_one_captured_origin() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    release_fixture_bare_for_post_test(&mut f);
    // A controlled earlier origin represents preparation time. AcceptEx is
    // real; this is not a measured ESTABLISHED epoch or real five-second wait.
    let began_at = Instant::now() - Duration::from_secs(1);
    let posts = f.host.owner.report.accept_posts;
    f.host
        .owner
        .post_bare_accept_with_clock(began_at, Instant::now)
        .unwrap();
    let bare = f.host.owner.resources.accept.as_ref().unwrap();
    assert_eq!(bare.deadline(), Some(began_at + BUDGET));
    assert!(bare.pending() && bare.socket != INVALID_SOCKET);
    assert_eq!(f.host.owner.report.accept_posts, posts + 1);
    f.stop();
}

#[test]
fn real_bare_expired_preparation_closes_socket_without_posting_io() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    release_fixture_bare_for_post_test(&mut f);
    let posts = f.host.owner.report.accept_posts;
    let completions = f.host.owner.report.accept_completions;
    let began_at = Instant::now() - BUDGET;
    let result = f
        .host
        .owner
        .post_bare_accept_with_clock(began_at, Instant::now);
    assert_eq!(
        result.unwrap_err(),
        "bare accept preparation consumed original deadline"
    );
    assert!(f.host.owner.bare_released());
    assert_eq!(f.host.owner.report.accept_posts, posts);
    assert_eq!(f.host.owner.report.accept_completions, completions);
    f.stop();
}

#[test]
fn real_bare_late_native_return_retains_charged_io_until_original_packet() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    release_fixture_bare_for_post_test(&mut f);
    let began_at = Instant::now();
    let mut before_native = true;
    let completions = f.host.owner.report.accept_completions;
    // Inject only the passage of monotonic time across a REAL AcceptEx call.
    // No fake IOCP packet or claimed five-second wire observation is supplied.
    f.host
        .owner
        .post_bare_accept_with_clock(began_at, || {
            if std::mem::take(&mut before_native) {
                began_at
            } else {
                began_at + BUDGET
            }
        })
        .unwrap();
    let bare = f.host.owner.resources.accept.as_ref().unwrap();
    assert_eq!(bare.deadline(), Some(began_at + BUDGET));
    assert!(matches!(
        bare.state,
        super::super::accept::AcceptState::Closing { expired: true, .. }
    ));
    assert!(bare.pending() && bare.socket == INVALID_SOCKET);
    assert_eq!(f.host.owner.report.accept_completions, completions);
    assert!(f.host.owner.post_bare_accept().is_err());
    f.stop();
    assert_eq!(f.host.owner.report.accept_completions, completions + 1);
}

#[test]
fn real_iocp_batch_services_due_receive_before_another_top_level_poll() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let token = incomplete_header_receive(&mut f);
    let hard = Instant::now() + HEADER_CLOSE_RESERVE;
    f.host.owner.resources.slots[token.slot()]
        .stream
        .as_mut()
        .unwrap()
        .deadline = hard;
    // Old poll_io did no deadline sweep: even an empty completion batch left
    // this socket live until an outer poll. Exercise this actual stage directly.
    f.host.owner.poll_io().unwrap();
    let slot = &f.host.owner.resources.slots[token.slot()];
    assert!(slot.phase == Phase::Closed && slot.socket == INVALID_SOCKET);
    assert!(
        !slot.retired,
        "IOCP service cannot fabricate SSPI retirement"
    );
    let closed = f.host.close_snapshot().headers[token.slot()].unwrap();
    assert_eq!(closed.hard_deadline, hard);
    assert!(closed.socket_returned_at.is_some());
    assert!(closed.retirement_confirmed_at.is_none());
    assert!(f.host.rearm(token).is_err());
    f.revoked_retirement(token);
    f.stop();
}

#[test]
fn caller_deadline_service_keeps_original_head_request_budget_and_admission() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let mut wire = vec![b'H'];
    wire.extend(record(&post(3)));
    f.input(&wire);
    let (token, admitted_at) = f.head();
    let request_deadline = admitted_at + REQUEST_BUDGET;
    let bare = f.host.owner.resources.accept.as_mut().unwrap();
    bare.state = super::super::accept::AcceptState::Pending {
        serial: bare.serial,
        deadline: Instant::now() - Duration::from_nanos(1),
    };
    // A new negative boundary services the due bare operation, while a valid
    // queued Head retains its one original120s interval and no native work
    // completion or replacement owner is invented by this API.
    let completions = f.host.owner.report.accept_completions;
    f.host.service_deadlines().unwrap();
    assert_eq!(f.host.owner.report.accept_completions, completions);
    assert!(f.host.owner.resources.accept.as_ref().unwrap().pending());
    let slot = &f.host.owner.resources.slots[token.slot()];
    assert!(slot.phase == Phase::Http && slot.socket != INVALID_SOCKET);
    assert_eq!(slot.stream.as_ref().unwrap().deadline, request_deadline);
    f.host.admit_body(token, 3, request_deadline).unwrap();
    assert_eq!(
        f.host.owner.resources.slots[token.slot()]
            .stream
            .as_ref()
            .unwrap()
            .deadline,
        request_deadline
    );
    f.input(&record(b"abc"));
    let (body_token, sequence, offset, bytes, eof) = f.body();
    assert_eq!(
        (body_token, offset, bytes.as_slice(), eof),
        (token, 0, b"abc".as_slice(), true)
    );
    f.host.ack_body(token, sequence, 3).unwrap();
    f.stop();
}
