//! Finite loopback evidence for owned TLS execution. This is not a production
//! listener and does not prove the strict first-connection scheduler deadline.
use super::{
    worker::Worker, Call, CertificateThumbprint, Frame, Step, TlsError, BUFFER_BYTES,
    CONNECTION_SLOTS, PLAINTEXT_BYTES,
};
use fc_ports::http::{RequestToken, ResponseHead, TokenIssuer};
use fc_protocol::http1::{
    response::{BodySender, PendingChunk},
    Event, Parser,
};
use std::{
    cell::UnsafeCell,
    io::Write,
    marker::PhantomPinned,
    mem::{size_of, ManuallyDrop},
    pin::Pin,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows::{
    core::PSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT},
        Networking::WinSock::*,
        System::IO::{CancelIoEx, CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED},
    },
};
const BUDGET: Duration = Duration::from_secs(5);
static OWNER_ACTIVE: AtomicBool = AtomicBool::new(false);
static OWNER_FAULTED: AtomicBool = AtomicBool::new(false);

struct IoMemory {
    operation: UnsafeCell<OVERLAPPED>,
    addresses: UnsafeCell<[u8; 64]>,
    bytes: UnsafeCell<[u8; BUFFER_BYTES]>,
    flags: UnsafeCell<u32>,
    _pinned: PhantomPinned,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Accept,
    Receive,
    Send,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Handshake,
    Http,
    Response,
    Shutdown,
    Closed,
}
struct Slot {
    memory: Pin<Box<IoMemory>>,
    socket: SOCKET,
    pending: Option<Operation>,
    token: RequestToken,
    frame: Option<Box<Frame>>,
    phase: Phase,
    parser: Parser,
    response_body: Option<BodySender>,
    response_chunk: Option<PendingChunk>,
    shutdown_complete: bool,
    context_submitted: bool,
    retired: bool,
    send_len: usize,
    sent: usize,
    close_after_send: bool,
}
struct Resources {
    slots: [Slot; CONNECTION_SLOTS],
    listener: SOCKET,
    port: HANDLE,
    winsock: bool,
    worker: Option<Worker>,
}
#[derive(Debug, Default)]
struct Report {
    accepted: usize,
    handshakes: usize,
    responses: usize,
    submissions: usize,
    completions: usize,
    sspi_submissions: usize,
    sspi_completions: usize,
    returned_frames: usize,
    slot_frames_at_drain: usize,
    deadline_closes: usize,
    closed_with_sspi_in_flight: usize,
    late_sspi_completions: usize,
    tls_failures: usize,
    first_tls_error: Option<TlsError>,
    first_tls_call: Option<Call>,
    handshake_calls: usize,
    need_input_count: usize,
    consumed_bytes: u64,
    extra_events: usize,
    extra_bytes: u64,
    parsed_requests: usize,
    shutdown_tokens_sent: usize,
    explicit_retirements: usize,
}
struct Owner {
    resources: ManuallyDrop<Resources>,
    started: Instant,
    generation: u64,
    port_number: u16,
    target: String,
    marker: String,
    stopping: bool,
    drained: bool,
    report: Report,
}
fn socket_handle(socket: SOCKET) -> HANDLE {
    HANDLE(socket.0 as *mut _)
}
fn error(stage: &str) -> String {
    // SAFETY: read the calling owner's last Winsock error immediately.
    format!("{stage}: Winsock {}", unsafe { WSAGetLastError().0 })
}
fn check(result: i32, stage: &str) -> Result<(), String> {
    if result == 0 {
        Ok(())
    } else {
        Err(error(stage))
    }
}
fn close(socket: &mut SOCKET, abortive: bool) -> Result<(), String> {
    if *socket == INVALID_SOCKET {
        return Ok(());
    }
    // SAFETY: the single owner closes its descriptor, without freeing any
    // associated OVERLAPPED memory or treating this as cancellation completion.
    unsafe {
        if abortive {
            let _ = setsockopt(*socket, SOL_SOCKET, SO_LINGER, Some(&[1, 0, 0, 0]));
        } else {
            let _ = shutdown(*socket, SD_SEND);
        }
        check(closesocket(*socket), "closesocket")?;
    }
    *socket = INVALID_SOCKET;
    Ok(())
}
impl Owner {
    fn new(marker: &str) -> Result<Self, String> {
        let started = Instant::now();
        if OWNER_FAULTED.load(Ordering::Acquire)
            || OWNER_ACTIVE
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err("TLS native owner busy or permanently faulted".into());
        }
        let mut issuer = match TokenIssuer::new() {
            Ok(issuer) => issuer,
            Err(_) => {
                OWNER_ACTIVE.store(false, Ordering::Release);
                return Err("TLS token generation exhausted".into());
            }
        };
        let generation = issuer.generation();
        Ok(Self {
            resources: ManuallyDrop::new(Resources {
                slots: std::array::from_fn(|index| {
                    let token = issuer
                        .issue(index)
                        .expect("four bounded initial token slots");
                    Slot {
                        memory: Box::pin(IoMemory {
                            operation: UnsafeCell::new(OVERLAPPED::default()),
                            addresses: UnsafeCell::new([0; 64]),
                            bytes: UnsafeCell::new([0; BUFFER_BYTES]),
                            flags: UnsafeCell::new(0),
                            _pinned: PhantomPinned,
                        }),
                        socket: INVALID_SOCKET,
                        pending: None,
                        token,
                        frame: Some(Frame::new(token)),
                        phase: Phase::Handshake,
                        parser: Parser::new(token, true),
                        response_body: None,
                        response_chunk: None,
                        shutdown_complete: false,
                        context_submitted: false,
                        retired: false,
                        send_len: 0,
                        sent: 0,
                        close_after_send: false,
                    }
                }),
                listener: INVALID_SOCKET,
                port: HANDLE::default(),
                winsock: false,
                worker: None,
            }),
            started,
            generation,
            port_number: 0,
            target: format!("/fc/tls-probe/{marker}"),
            marker: marker.into(),
            stopping: false,
            drained: false,
            report: Report::default(),
        })
    }
    fn start(&mut self, thumbprint: CertificateThumbprint) -> Result<(), String> {
        let worker = Worker::start(self.generation, thumbprint)
            .map_err(|e| format!("TLS worker start: {e:?}"))?;
        self.start_with_worker(worker)
    }
    fn start_with_worker(&mut self, worker: Worker) -> Result<(), String> {
        self.resources.worker = Some(worker);
        loop {
            if let Some(result) = self.resources.worker.as_mut().unwrap().poll_ready() {
                result.map_err(|e| format!("TLS credential readiness: {e:?}"))?;
                break;
            }
            if self.started.elapsed() >= BUDGET {
                return Err("TLS initialization deadline".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if self.started.elapsed() >= BUDGET {
            return Err("TLS initialization consumed admission deadline".into());
        }
        // SAFETY: all pointers are local fixed values or pinned owner memory.
        // No listener is opened until worker readiness has actually been taken.
        unsafe {
            let mut data = WSADATA::default();
            let code = WSAStartup(0x202, &mut data);
            if code != 0 {
                return Err(format!("WSAStartup: {code}"));
            }
            self.resources.winsock = true;
            self.resources.listener = WSASocketW(
                AF_INET.0 as i32,
                SOCK_STREAM.0,
                IPPROTO_TCP.0,
                None,
                0,
                WSA_FLAG_OVERLAPPED,
            )
            .map_err(|e| format!("TLS listener: {e}"))?;
            check(
                setsockopt(
                    self.resources.listener,
                    SOL_SOCKET,
                    SO_EXCLUSIVEADDRUSE,
                    Some(&1i32.to_ne_bytes()),
                ),
                "exclusive listener",
            )?;
            let mut address = SOCKADDR_IN {
                sin_family: AF_INET,
                sin_port: 0,
                sin_addr: IN_ADDR {
                    S_un: IN_ADDR_0 {
                        S_addr: u32::from_ne_bytes([127, 0, 0, 1]),
                    },
                },
                ..Default::default()
            };
            check(
                bind(
                    self.resources.listener,
                    ptr::from_ref(&address).cast(),
                    size_of::<SOCKADDR_IN>() as i32,
                ),
                "loopback bind",
            )?;
            if self.started.elapsed() >= BUDGET {
                return Err("TLS admission deadline before listen".into());
            }
            check(
                listen(self.resources.listener, CONNECTION_SLOTS as i32),
                "listen",
            )?;
            let mut length = size_of::<SOCKADDR_IN>() as i32;
            check(
                getsockname(
                    self.resources.listener,
                    ptr::from_mut(&mut address).cast(),
                    &mut length,
                ),
                "getsockname",
            )?;
            self.port_number = u16::from_be(address.sin_port);
            self.resources.port = CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1)
                .map_err(|e| format!("TLS IOCP: {e}"))?;
            CreateIoCompletionPort(
                socket_handle(self.resources.listener),
                Some(self.resources.port),
                1,
                1,
            )
            .map_err(|e| format!("listener IOCP: {e}"))?;
            let mut accept: LPFN_ACCEPTEX = None;
            let mut bytes = 0;
            check(
                WSAIoctl(
                    self.resources.listener,
                    SIO_GET_EXTENSION_FUNCTION_POINTER,
                    Some(ptr::from_ref(&WSAID_ACCEPTEX).cast()),
                    size_of::<windows::core::GUID>() as u32,
                    Some(ptr::from_mut(&mut accept).cast()),
                    size_of::<LPFN_ACCEPTEX>() as u32,
                    &mut bytes,
                    None,
                    None,
                ),
                "AcceptEx lookup",
            )?;
            let accept = accept.ok_or("AcceptEx unavailable")?;
            let listener = self.resources.listener;
            for slot in &mut self.resources.slots {
                slot.socket = WSASocketW(
                    AF_INET.0 as i32,
                    SOCK_STREAM.0,
                    IPPROTO_TCP.0,
                    None,
                    0,
                    WSA_FLAG_OVERLAPPED,
                )
                .map_err(|e| format!("TLS accept socket: {e}"))?;
                let success = accept(
                    listener,
                    slot.socket,
                    slot.memory.addresses.get().cast(),
                    0,
                    32,
                    32,
                    &mut bytes,
                    slot.memory.operation.get(),
                )
                .as_bool();
                if !success && WSAGetLastError() != WSA_IO_PENDING {
                    return Err(error("AcceptEx post"));
                }
                // Immediate success queues an IOCP packet as well.
                slot.pending = Some(Operation::Accept);
                self.report.submissions += 1;
            }
        }
        Ok(())
    }
    fn close_slot(&mut self, index: usize, abortive: bool) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        if slot.phase != Phase::Closed && slot.socket != INVALID_SOCKET {
            if self.started.elapsed() >= BUDGET {
                self.report.deadline_closes += 1;
            }
            if slot.frame.is_none() {
                self.report.closed_with_sspi_in_flight += 1;
            }
        }
        slot.phase = Phase::Closed;
        if slot.socket != INVALID_SOCKET
            && matches!(slot.pending, Some(Operation::Receive | Operation::Send))
        {
            // SAFETY: cancellation targets this exact pinned operation; its
            // pending ledger remains occupied until its actual packet is read.
            unsafe {
                let _ = CancelIoEx(
                    socket_handle(slot.socket),
                    Some(slot.memory.operation.get()),
                );
            }
        }
        close(&mut slot.socket, abortive)
    }
    fn prepare_io(&mut self, index: usize) -> Result<(), String> {
        let slot = &self.resources.slots[index];
        if slot.pending.is_some() || slot.socket == INVALID_SOCKET || slot.phase == Phase::Closed {
            return Err("TLS IO overlap or closed socket".into());
        }
        // SAFETY: the previous packet has been dequeued; no native writer can
        // still reference this operation. The pinned allocation never moves.
        unsafe {
            *slot.memory.operation.get() = OVERLAPPED::default();
        }
        Ok(())
    }
    fn receive(&mut self, index: usize) -> Result<(), String> {
        if self.stopping || self.started.elapsed() >= BUDGET {
            return self.close_slot(index, true);
        }
        self.prepare_io(index)?;
        let slot = &mut self.resources.slots[index];
        let frame = slot
            .frame
            .as_ref()
            .ok_or("recv while TLS frame belongs to worker")?;
        let capacity = BUFFER_BYTES
            .checked_sub(frame.input_len)
            .ok_or("TLS input length")?;
        if capacity == 0 {
            return self.close_slot(index, true);
        }
        // SAFETY: buffer and flags are pinned and remain exclusively native
        // until this operation's own completion is dequeued.
        unsafe {
            *slot.memory.flags.get() = 0;
            let buffer = WSABUF {
                len: capacity as u32,
                buf: PSTR(slot.memory.bytes.get().cast()),
            };
            let result = WSARecv(
                slot.socket,
                &[buffer],
                None,
                slot.memory.flags.get(),
                Some(slot.memory.operation.get()),
                None,
            );
            if result != 0 && WSAGetLastError() != WSA_IO_PENDING {
                return self.close_slot(index, true);
            }
        }
        slot.pending = Some(Operation::Receive);
        self.report.submissions += 1;
        Ok(())
    }
    fn send(&mut self, index: usize) -> Result<(), String> {
        if self.stopping || self.started.elapsed() >= BUDGET {
            return self.close_slot(index, true);
        }
        self.prepare_io(index)?;
        let slot = &mut self.resources.slots[index];
        let remaining = slot
            .send_len
            .checked_sub(slot.sent)
            .filter(|n| *n > 0)
            .ok_or("TLS empty send")?;
        // SAFETY: the immutable suffix belongs to this pin until its matching
        // completion; the stack WSABUF descriptor itself is captured by Winsock.
        unsafe {
            let buffer = WSABUF {
                len: remaining as u32,
                buf: PSTR(slot.memory.bytes.get().cast::<u8>().add(slot.sent)),
            };
            let result = WSASend(
                slot.socket,
                &[buffer],
                None,
                0,
                Some(slot.memory.operation.get()),
                None,
            );
            if result != 0 && WSAGetLastError() != WSA_IO_PENDING {
                return self.close_slot(index, true);
            }
        }
        slot.pending = Some(Operation::Send);
        self.report.submissions += 1;
        Ok(())
    }
    fn start_send(&mut self, index: usize, close_after: bool) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        let frame = slot
            .frame
            .as_ref()
            .ok_or("send without returned TLS frame")?;
        if frame.output_len == 0 || frame.output_len > BUFFER_BYTES {
            return Err("TLS output bounds".into());
        }
        // SAFETY: there is no pending native IO and this owner alone accesses
        // the pinned send buffer; the worker owns no alias of either array.
        unsafe {
            (&mut *slot.memory.bytes.get())[..frame.output_len]
                .copy_from_slice(&frame.output[..frame.output_len]);
        }
        slot.send_len = frame.output_len;
        slot.sent = 0;
        slot.close_after_send = close_after;
        self.send(index)
    }
    fn drive(&mut self, index: usize) -> Result<(), String> {
        if self.stopping || self.resources.slots[index].phase == Phase::Closed {
            return Ok(());
        }
        if self.started.elapsed() >= BUDGET {
            return self.close_slot(index, true);
        }
        let slot = &mut self.resources.slots[index];
        if slot.pending.is_some() || slot.frame.is_none() {
            return Ok(());
        }
        if slot.frame.as_ref().unwrap().input_len == 0
            && matches!(slot.phase, Phase::Handshake | Phase::Http)
        {
            return self.receive(index);
        }
        let mut frame = slot.frame.take().unwrap();
        frame.call = match slot.phase {
            Phase::Handshake => Call::Handshake,
            Phase::Http => Call::Decrypt,
            Phase::Response => Call::Encrypt,
            Phase::Shutdown => Call::Shutdown,
            Phase::Closed => unreachable!(),
        };
        match self
            .resources
            .worker
            .as_mut()
            .ok_or("missing TLS worker")?
            .try_submit(frame)
        {
            Ok(()) => {
                self.resources.slots[index].context_submitted = true;
                self.report.sspi_submissions += 1;
                Ok(())
            }
            Err((error, frame)) => {
                self.resources.slots[index].frame = Some(frame);
                Err(format!("TLS mailbox rejected admitted slot: {error:?}"))
            }
        }
    }
    fn complete_frame(&mut self, mut frame: Box<Frame>) -> Result<(), String> {
        let index = frame.token.slot();
        if index >= CONNECTION_SLOTS
            || self.resources.slots[index].token != frame.token
            || self.resources.slots[index].frame.is_some()
        {
            return Err("TLS completion identity mismatch".into());
        }
        let result = frame.result.take().ok_or("TLS result missing")?;
        let call = frame.call;
        if let Err(error) = result {
            self.report.tls_failures += 1;
            if self.report.first_tls_error.is_none() {
                self.report.first_tls_error = Some(error);
                self.report.first_tls_call = Some(call);
            }
        }
        // Stopped frames were returned without invoking Driver::call. Other
        // handshake completions include the actual guarded native call attempt.
        if call == Call::Handshake && result != Err(TlsError::Stopped) {
            self.report.handshake_calls += 1;
        }
        if result == Ok(Step::NeedInput) {
            self.report.need_input_count += 1;
        }
        if let Ok(Step::Handshake { consumed, .. } | Step::Plaintext { consumed }) = result {
            if consumed != 0 && consumed <= frame.input_len {
                self.report.consumed_bytes += consumed as u64;
                let extra = frame.input_len - consumed;
                if extra != 0 {
                    self.report.extra_events += 1;
                    self.report.extra_bytes += extra as u64;
                }
            }
        }
        self.resources.slots[index].frame = Some(frame);
        if call == Call::Retire {
            if self.stopping && result == Err(TlsError::Stopped) {
                // A queued retirement can be negatively returned on stop.
                // Actual worker close/reap, not this frame, must clear it.
                return Ok(());
            }
            if result != Ok(Step::Retired) {
                return Err(format!("TLS explicit retirement failed: {result:?}"));
            }
            let slot = &mut self.resources.slots[index];
            if slot.phase != Phase::Closed || slot.pending.is_some() || slot.retired {
                return Err("TLS retirement before native IO returned".into());
            }
            slot.retired = true;
            self.report.explicit_retirements += 1;
            return Ok(());
        }
        if self.stopping || self.resources.slots[index].phase == Phase::Closed {
            return Ok(());
        }
        if self.started.elapsed() >= BUDGET {
            return self.close_slot(index, true);
        }
        let step = match result {
            Ok(step) => step,
            Err(_) => return self.close_slot(index, true),
        };
        match step {
            Step::NeedInput => self.receive(index),
            Step::PeerClosed => {
                let slot = &mut self.resources.slots[index];
                slot.frame.as_mut().unwrap().input_len = 0;
                slot.phase = Phase::Shutdown;
                self.drive(index)
            }
            Step::Handshake { consumed, complete } => {
                let slot = &mut self.resources.slots[index];
                let frame = slot.frame.as_mut().unwrap();
                if consumed == 0 || consumed > frame.input_len {
                    return Err("TLS handshake consumption".into());
                }
                frame.input.copy_within(consumed..frame.input_len, 0);
                frame.input_len -= consumed;
                if complete {
                    if slot.phase != Phase::Handshake {
                        return Err("TLS duplicate handshake".into());
                    }
                    slot.phase = Phase::Http;
                    self.report.handshakes += 1;
                }
                if frame.output_len != 0 {
                    self.start_send(index, false)
                } else {
                    self.drive(index)
                }
            }
            Step::Plaintext { consumed } => {
                let slot = &mut self.resources.slots[index];
                let frame = slot.frame.as_mut().unwrap();
                if slot.phase != Phase::Http
                    || consumed == 0
                    || consumed > frame.input_len
                    || frame.output_len > PLAINTEXT_BYTES
                {
                    return Err("TLS plaintext bounds".into());
                }
                let progress = match slot.parser.feed(&frame.output[..frame.output_len]) {
                    Ok(progress) => progress,
                    Err(_) => return self.close_slot(index, true),
                };
                let head = match progress.event {
                    Event::NeedHead => None,
                    Event::Head(head) if progress.consumed == frame.output_len => Some(head),
                    _ => return self.close_slot(index, true),
                };
                frame.input.copy_within(consumed..frame.input_len, 0);
                frame.input_len -= consumed;
                if let Some(head) = head {
                    // This remains one isolated diagnostic route. Parsed HTTP
                    // metadata is not session authentication or authorization.
                    if frame.input_len != 0
                        || head.method != "GET"
                        || head.target != self.target
                        || head.has_body
                        || head.content_length.is_some_and(|length| length != 0)
                        || head
                            .headers
                            .iter()
                            .filter(|h| h.name.eq_ignore_ascii_case("host"))
                            .count()
                            != 1
                        || !head.headers.iter().any(|h| {
                            h.name.eq_ignore_ascii_case("host")
                                && h.value.eq_ignore_ascii_case(b"localhost")
                        })
                    {
                        return self.close_slot(index, true);
                    }
                    slot.parser
                        .admit_body(0)
                        .map_err(|e| format!("TLS diagnostic body admission: {e:?}"))?;
                    if !slot.parser.is_complete() {
                        return Err(
                            "TLS diagnostic framing incomplete after zero-body admission".into(),
                        );
                    }
                    let response = ResponseHead {
                        status: 200,
                        headers: Vec::new(),
                        content_length: self.marker.len() as u64,
                    };
                    let mut sender = BodySender::new(slot.token, &response)
                        .map_err(|e| format!("TLS response head: {e:?}"))?;
                    let header = sender.head_bytes();
                    let length = header.len() + self.marker.len();
                    if length > PLAINTEXT_BYTES {
                        return Err(
                            "TLS diagnostic response exceeds single plaintext record".into()
                        );
                    }
                    frame.input[..header.len()].copy_from_slice(header);
                    frame.input[header.len()..length].copy_from_slice(self.marker.as_bytes());
                    frame.input_len = length;
                    slot.response_chunk = Some(
                        sender
                            .begin_chunk(slot.token, self.marker.len())
                            .map_err(|e| format!("TLS response body admission: {e:?}"))?,
                    );
                    slot.response_body = Some(sender);
                    slot.phase = Phase::Response;
                    self.report.parsed_requests += 1;
                }
                self.drive(index)
            }
            Step::Encrypted => {
                if self.resources.slots[index].phase != Phase::Response {
                    return Err("unexpected encrypted response".into());
                }
                self.start_send(index, true)
            }
            Step::Shutdown { complete } => {
                let slot = &mut self.resources.slots[index];
                if slot.phase != Phase::Shutdown {
                    return Err("unexpected TLS shutdown result".into());
                }
                slot.shutdown_complete = complete;
                if slot.frame.as_ref().unwrap().output_len != 0 {
                    self.start_send(index, true)
                } else if complete {
                    self.close_slot(index, false)
                } else {
                    // A provider producing neither output nor completion must
                    // not create an unbounded self-waking native call loop.
                    self.close_slot(index, true)
                }
            }
            Step::Retired => Err("unsolicited TLS retirement result".into()),
        }
    }
    fn poll_io(&mut self) -> Result<(), String> {
        if self.resources.port.is_invalid() {
            return Ok(());
        }
        for _ in 0..CONNECTION_SLOTS {
            let (mut bytes, mut key, mut operation) = (0, 0, ptr::null_mut());
            // SAFETY: IOCP and all operation allocations remain owned and pinned.
            let result = unsafe {
                GetQueuedCompletionStatus(
                    self.resources.port,
                    &mut bytes,
                    &mut key,
                    &mut operation,
                    0,
                )
            };
            if operation.is_null() {
                if let Err(error) = result {
                    if error.code().0 as u32 & 0xffff != WAIT_TIMEOUT.0 {
                        return Err(format!("TLS IOCP poll: {error}"));
                    }
                }
                break;
            }
            let index = self
                .resources
                .slots
                .iter()
                .position(|slot| slot.memory.operation.get() == operation)
                .ok_or("foreign TLS completion pointer")?;
            let pending = self.resources.slots[index]
                .pending
                .ok_or("unsubmitted TLS completion")?;
            let expected_key = if pending == Operation::Accept {
                1
            } else {
                index + 2
            };
            if key != expected_key {
                return Err("TLS completion key mismatch".into());
            }
            self.resources.slots[index].pending = None;
            self.report.completions += 1;
            if self.stopping || result.is_err() {
                self.close_slot(index, true)?;
                continue;
            }
            match pending {
                Operation::Accept => {
                    let listener = self.resources.listener;
                    let port = self.resources.port;
                    let slot = &mut self.resources.slots[index];
                    // SAFETY: AcceptEx is complete before socket context update
                    // and association; receives are posted only afterwards.
                    unsafe {
                        check(
                            setsockopt(
                                slot.socket,
                                SOL_SOCKET,
                                SO_UPDATE_ACCEPT_CONTEXT,
                                Some(&listener.0.to_ne_bytes()),
                            ),
                            "TLS accept context",
                        )?;
                        CreateIoCompletionPort(
                            socket_handle(slot.socket),
                            Some(port),
                            index + 2,
                            1,
                        )
                        .map_err(|e| format!("TLS socket association: {e}"))?;
                    }
                    self.report.accepted += 1;
                    self.drive(index)?;
                    if self.report.accepted == CONNECTION_SLOTS {
                        close(&mut self.resources.listener, true)?;
                    }
                }
                Operation::Receive => {
                    if bytes == 0 {
                        self.close_slot(index, true)?;
                        continue;
                    }
                    let slot = &mut self.resources.slots[index];
                    let frame = slot
                        .frame
                        .as_mut()
                        .ok_or("recv completed without frame owner")?;
                    let end = frame
                        .input_len
                        .checked_add(bytes as usize)
                        .filter(|end| *end <= BUFFER_BYTES)
                        .ok_or("TLS recv bounds")?;
                    // SAFETY: this exact receive completed; native has released
                    // the buffer, and no new operation is posted during copying.
                    unsafe {
                        frame.input[frame.input_len..end]
                            .copy_from_slice(&(&*slot.memory.bytes.get())[..bytes as usize]);
                    }
                    frame.input_len = end;
                    self.drive(index)?;
                }
                Operation::Send => {
                    let slot = &mut self.resources.slots[index];
                    let sent = slot
                        .sent
                        .checked_add(bytes as usize)
                        .filter(|sent| *sent <= slot.send_len)
                        .ok_or("TLS send bounds")?;
                    if bytes == 0 {
                        self.close_slot(index, true)?;
                        continue;
                    }
                    slot.sent = sent;
                    if sent != slot.send_len {
                        self.send(index)?;
                    } else if slot.close_after_send {
                        match slot.phase {
                            Phase::Response => {
                                let sender = slot
                                    .response_body
                                    .as_mut()
                                    .ok_or("missing HTTP response sender")?;
                                let chunk = slot
                                    .response_chunk
                                    .take()
                                    .ok_or("missing HTTP send receipt")?;
                                // Every ciphertext byte of this response has
                                // returned through its actual IOCP completion.
                                sender
                                    .acknowledge_chunk(chunk, self.marker.len())
                                    .map_err(|e| format!("HTTP response acknowledgement: {e:?}"))?;
                                sender
                                    .finish(slot.token)
                                    .map_err(|e| format!("HTTP response incomplete: {e:?}"))?;
                                self.report.responses += 1;
                                slot.frame.as_mut().unwrap().input_len = 0;
                                slot.phase = Phase::Shutdown;
                                self.drive(index)?;
                            }
                            Phase::Shutdown => {
                                self.report.shutdown_tokens_sent += 1;
                                if slot.shutdown_complete {
                                    self.close_slot(index, false)?;
                                } else {
                                    self.drive(index)?;
                                }
                            }
                            _ => return Err("unexpected close-after-send phase".into()),
                        }
                    } else {
                        self.drive(index)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn poll(&mut self) -> Result<(), String> {
        self.poll_io()?;
        for _ in 0..CONNECTION_SLOTS {
            let frame = match self.resources.worker.as_mut() {
                Some(worker) => worker
                    .poll()
                    .map_err(|e| format!("TLS result mailbox: {e:?}"))?,
                None => None,
            };
            match frame {
                Some(frame) => {
                    self.report.returned_frames += 1;
                    if frame.result.is_some() {
                        self.report.sspi_completions += 1;
                    }
                    let index = frame.token.slot();
                    if frame.call != Call::Retire
                        && (self.stopping
                            || (index < CONNECTION_SLOTS
                                && self.resources.slots[index].phase == Phase::Closed))
                    {
                        self.report.late_sspi_completions += 1;
                    }
                    self.complete_frame(frame)?;
                }
                None => break,
            }
        }
        if !self.stopping {
            for index in 0..CONNECTION_SLOTS {
                self.retire_closed_slot(index)?;
            }
        }
        Ok(())
    }
    fn retire_closed_slot(&mut self, index: usize) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        if slot.phase != Phase::Closed
            || slot.socket != INVALID_SOCKET
            || slot.pending.is_some()
            || !slot.context_submitted
            || slot.retired
            || slot.frame.is_none()
        {
            return Ok(());
        }
        let mut frame = slot.frame.take().unwrap();
        frame.call = Call::Retire;
        frame.input_len = 0;
        match self
            .resources
            .worker
            .as_mut()
            .ok_or("missing retiring TLS worker")?
            .try_submit(frame)
        {
            Ok(()) => {
                self.report.sspi_submissions += 1;
                Ok(())
            }
            Err((error, frame)) => {
                self.resources.slots[index].frame = Some(frame);
                Err(format!(
                    "TLS retirement mailbox rejected exact token: {error:?}"
                ))
            }
        }
    }
    fn app_owned_bytes(&self) -> usize {
        size_of::<Owner>()
            + CONNECTION_SLOTS * (size_of::<IoMemory>() + size_of::<Frame>())
            + self.target.capacity()
            + self.marker.capacity()
            + self
                .resources
                .slots
                .iter()
                .map(|slot| {
                    slot.parser.header_capacity()
                        + slot
                            .response_body
                            .as_ref()
                            .map_or(0, BodySender::head_capacity)
                })
                .sum::<usize>()
    }
    fn stop(&mut self) -> Result<(), String> {
        self.stopping = true;
        if let Some(worker) = &self.resources.worker {
            worker.request_stop();
        }
        let mut failure = None;
        if self.resources.listener != INVALID_SOCKET {
            for slot in &self.resources.slots {
                if slot.pending == Some(Operation::Accept) {
                    // SAFETY: request only; actual IOCP packets remain owed.
                    unsafe {
                        let _ = CancelIoEx(
                            socket_handle(self.resources.listener),
                            Some(slot.memory.operation.get()),
                        );
                    }
                }
            }
        }
        for index in 0..CONNECTION_SLOTS {
            if let Err(error) = self.close_slot(index, true) {
                failure = Some(error);
            }
        }
        if let Err(error) = close(&mut self.resources.listener, true) {
            failure = Some(error);
        }
        failure.map_or(Ok(()), Err)
    }
    fn finish(&mut self) -> Result<(), String> {
        let stop_result = self.stop();
        let deadline = Instant::now() + BUDGET;
        loop {
            self.poll()?;
            let io_done = self
                .resources
                .slots
                .iter()
                .all(|slot| slot.pending.is_none());
            let worker_done = match self.resources.worker.as_mut() {
                Some(worker) => {
                    if worker.has_in_flight() {
                        false
                    } else {
                        worker
                            .reap_nonblocking()
                            .map_err(|e| format!("TLS physical worker reap: {e:?}"))?
                    }
                }
                None => true,
            };
            if io_done && worker_done {
                stop_result?;
                if self.report.submissions != self.report.completions {
                    return Err("TLS IO ledger mismatch".into());
                }
                if self.resources.slots.iter().any(|slot| slot.frame.is_none()) {
                    return Err("TLS linear frame missing after drain".into());
                }
                if self.report.sspi_submissions != self.report.sspi_completions
                    || self.report.sspi_completions != self.report.returned_frames
                {
                    return Err("TLS SSPI completion ledger mismatch".into());
                }
                self.report.slot_frames_at_drain = self
                    .resources
                    .slots
                    .iter()
                    .filter(|slot| slot.frame.is_some())
                    .count();
                // SAFETY: all IOCP packets and the actual SSPI thread have
                // terminated. Check real resource release before reporting a
                // completed cleanup; failures retain the bounded owner.
                unsafe {
                    if !self.resources.port.is_invalid() {
                        CloseHandle(self.resources.port)
                            .map_err(|e| format!("TLS IOCP close: {e}"))?;
                        self.resources.port = HANDLE::default();
                    }
                    if self.resources.winsock {
                        check(WSACleanup(), "TLS WSACleanup")?;
                        self.resources.winsock = false;
                    }
                }
                self.drained = true;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(
                    "TLS drain deadline; native owner retained and replacement forbidden".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if !self.drained {
            let _ = self.stop();
            OWNER_FAULTED.store(true, Ordering::Release);
            // Keep all IOCP/Winsock handles, pinned arrays, linear frames and
            // worker owner together. A sticky fault prohibits replacement.
            return;
        }
        // SAFETY: every native operation and SSPI thread has physically ended;
        // no external writer can still reference any field in Resources.
        debug_assert!(self.resources.port.is_invalid() && !self.resources.winsock);
        unsafe {
            ManuallyDrop::drop(&mut self.resources);
        }
        OWNER_ACTIVE.store(false, Ordering::Release);
    }
}

pub(super) fn run(thumbprint: CertificateThumbprint, marker: &str) -> Result<(), String> {
    if marker.is_empty()
        || marker.len() > 64
        || !marker
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("TLS probe marker must be 1..64 ASCII identifier bytes".into());
    }
    let mut total = Report::default();
    let mut app_owned_bytes = 0;
    for cycle in 1..=2 {
        let mut owner = Owner::new(marker)?;
        app_owned_bytes = app_owned_bytes.max(owner.app_owned_bytes());
        let result = (|| {
            owner.start(thumbprint)?;
            println!("TLS_FRONTDOOR_READY {{\"port\":{},\"marker\":\"{}\",\"generation\":{},\"cycle\":{cycle}}}", owner.port_number, marker, owner.generation);
            std::io::stdout().flush().map_err(|e| e.to_string())?;
            while owner.started.elapsed() < BUDGET {
                owner.poll()?;
                app_owned_bytes = app_owned_bytes.max(owner.app_owned_bytes());
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok::<(), String>(())
        })();
        let cleanup = owner.finish();
        app_owned_bytes = app_owned_bytes.max(owner.app_owned_bytes());
        // Bounded metadata only: never emit TLS/HTTP bytes, key material or
        // native pointers. Closed enums contain no user-controlled strings.
        let first_error = owner
            .report
            .first_tls_error
            .map_or_else(|| "null".into(), |error| format!("\"{error:?}\""));
        let first_call = owner
            .report
            .first_tls_call
            .map_or_else(|| "null".into(), |call| format!("\"{call:?}\""));
        eprintln!("TLS_FRONTDOOR_CYCLE {{\"cycle\":{cycle},\"accepted\":{},\"handshakes\":{},\"responses\":{},\"parsedRequests\":{},\"shutdownTokensSent\":{},\"explicitRetirements\":{},\"tlsFailures\":{},\"firstTlsError\":{},\"firstTlsCall\":{},\"handshakeCalls\":{},\"needInputCount\":{},\"consumedBytes\":{},\"extraEvents\":{},\"extraBytes\":{},\"ioSubmissions\":{},\"ioCompletions\":{},\"sspiSubmissions\":{},\"sspiCompletions\":{},\"cleanupComplete\":{}}}", owner.report.accepted, owner.report.handshakes, owner.report.responses, owner.report.parsed_requests, owner.report.shutdown_tokens_sent, owner.report.explicit_retirements, owner.report.tls_failures, first_error, first_call, owner.report.handshake_calls, owner.report.need_input_count, owner.report.consumed_bytes, owner.report.extra_events, owner.report.extra_bytes, owner.report.submissions, owner.report.completions, owner.report.sspi_submissions, owner.report.sspi_completions, owner.drained);
        match (result, cleanup) {
            (Err(error), Err(cleanup)) => return Err(format!("{error}; cleanup: {cleanup}")),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => return Err(error),
            (Ok(()), Ok(())) => {}
        }
        let minimum_handshakes = if cycle == 1 { 3 } else { 1 };
        if owner.report.accepted != CONNECTION_SLOTS
            || owner.report.handshakes < minimum_handshakes
            || owner.report.responses != 1
        {
            return Err(format!(
                "TLS diagnostic batch {cycle} mismatch: {:?}",
                owner.report
            ));
        }
        total.accepted += owner.report.accepted;
        total.handshakes += owner.report.handshakes;
        total.responses += owner.report.responses;
        total.parsed_requests += owner.report.parsed_requests;
        total.shutdown_tokens_sent += owner.report.shutdown_tokens_sent;
        total.explicit_retirements += owner.report.explicit_retirements;
        total.submissions += owner.report.submissions;
        total.completions += owner.report.completions;
        total.sspi_submissions += owner.report.sspi_submissions;
        total.sspi_completions += owner.report.sspi_completions;
        total.returned_frames += owner.report.returned_frames;
        total.slot_frames_at_drain += owner.report.slot_frames_at_drain;
        total.deadline_closes += owner.report.deadline_closes;
        total.closed_with_sspi_in_flight += owner.report.closed_with_sspi_in_flight;
        total.late_sspi_completions += owner.report.late_sspi_completions;
        total.tls_failures += owner.report.tls_failures;
        if total.first_tls_error.is_none() {
            total.first_tls_error = owner.report.first_tls_error;
            total.first_tls_call = owner.report.first_tls_call;
        }
        total.handshake_calls += owner.report.handshake_calls;
        total.need_input_count += owner.report.need_input_count;
        total.consumed_bytes += owner.report.consumed_bytes;
        total.extra_events += owner.report.extra_events;
        total.extra_bytes += owner.report.extra_bytes;
        // Drop proves complete cleanup and releases both native/worker leases
        // before the next cycle can construct an independent new generation.
        drop(owner);
    }
    let first_error = total
        .first_tls_error
        .map_or_else(|| "null".into(), |error| format!("\"{error:?}\""));
    let first_call = total
        .first_tls_call
        .map_or_else(|| "null".into(), |call| format!("\"{call:?}\""));
    println!("TLS_FRONTDOOR_PASS {{\"cycles\":2,\"accepted\":{},\"handshakes\":{},\"responses\":{},\"parsedRequests\":{},\"shutdownTokensSent\":{},\"explicitRetirements\":{},\"ioSubmissions\":{},\"ioCompletions\":{},\"sspiSubmissions\":{},\"sspiCompletions\":{},\"returnedFrames\":{},\"slotFramesAtDrain\":{},\"deadlineCloses\":{},\"closedWithSspiInFlight\":{},\"lateSspiCompletions\":{},\"tlsFailures\":{},\"firstTlsError\":{},\"firstTlsCall\":{},\"handshakeCalls\":{},\"needInputCount\":{},\"consumedBytes\":{},\"extraEvents\":{},\"extraBytes\":{},\"appOwnedBufferAndStructBytes\":{},\"sspiThreadStackBytes\":1048576,\"memoryScope\":\"native owner inline structs, four IO/Frame allocations, parser header arenas, retained response head capacities and target/marker String capacities; stack is requested size; excludes transient decoded RequestHead allocations, worker shared/channel/runtime metadata, allocator overhead and SSPI/provider/kernel allocations\",\"cleanupComplete\":true,\"strictP0\":\"NOT_VALIDATED\"}}", total.accepted, total.handshakes, total.responses, total.parsed_requests, total.shutdown_tokens_sent, total.explicit_retirements, total.submissions, total.completions, total.sspi_submissions, total.sspi_completions, total.returned_frames, total.slot_frames_at_drain, total.deadline_closes, total.closed_with_sspi_in_flight, total.late_sspi_completions, total.tls_failures, first_error, first_call, total.handshake_calls, total.need_input_count, total.consumed_bytes, total.extra_events, total.extra_bytes, app_owned_bytes);
    std::io::stdout().flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
