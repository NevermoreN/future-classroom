//! Explicit isolated loopback HTTP/TLS candidate. Business work stays outside
//! this owner. No ordinary-startup, trust-store or strict-five-second claim.
use super::*;
use fc_ports::http::{RequestHead, CHUNK_BYTES, MAX_POLL_EVENTS};
use std::sync::Arc;

/// Secrets are deliberately neither Clone nor Debug. Closed is a logical
/// cancellation signal; only Retired witnesses this native slot's retirement.
pub enum TlsHttpEvent {
    Head {
        head: RequestHead,
        admitted_at: Instant,
    },
    Body {
        token: RequestToken,
        sequence: u64,
        offset: u64,
        bytes: Vec<u8>,
        eof: bool,
    },
    ResponseReady(RequestToken),
    ResponseComplete(RequestToken),
    Closed {
        token: RequestToken,
    },
    Retired(RequestToken),
}
struct BodyFlight {
    sequence: u64,
    end: u64,
    eof: bool,
}
const REQUEST_BUDGET: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeadlinePhase {
    AwaitingConnection,
    Header,
    Request { admitted_at: Instant },
}

pub(super) struct Stream {
    // Inert while Ready. Bound exactly once to the bare owner's original
    // post-derived deadline, never an ESTABLISHED or handoff+5 timestamp.
    pub(super) deadline: Instant,
    deadline_phase: DeadlinePhase,
    admitted: bool,
    expected_body: Option<u64>,
    // Captured from the original parsed request before publishing its Head.
    // The response caller cannot opt out of HEAD semantics.
    head_only: bool,
    plain_pos: usize,
    plain_len: usize,
    body_offset: u64,
    next_sequence: u64,
    flight: Option<BodyFlight>,
    request_done: bool,
    pub(super) deferred_final: Option<TlsHttpEvent>,
    pub(super) event: Option<TlsHttpEvent>,
    pub(super) closed_event: bool,
    pub(super) retired_event: bool,
    closed_delivered: bool,
    retired_delivered: bool,
    // One actual owned logical response chunk; never an asynchronous caller slice.
    tx: Box<[u8; CHUNK_BYTES]>,
    tx_len: usize,
    tx_pos: usize,
    tx_final: bool,
    head_pos: usize,
    record_head: usize,
    record_body: usize,
    awaiting_response: bool,
    response_fence: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}
impl Stream {
    fn new() -> Self {
        Self {
            deadline: Instant::now(),
            deadline_phase: DeadlinePhase::AwaitingConnection,
            admitted: false,
            expected_body: None,
            head_only: false,
            plain_pos: 0,
            plain_len: 0,
            body_offset: 0,
            next_sequence: 1,
            flight: None,
            request_done: false,
            deferred_final: None,
            event: None,
            closed_event: false,
            retired_event: false,
            closed_delivered: false,
            retired_delivered: false,
            tx: Box::new([0; CHUNK_BYTES]),
            tx_len: 0,
            tx_pos: 0,
            tx_final: false,
            head_pos: 0,
            record_head: 0,
            record_body: 0,
            awaiting_response: false,
            response_fence: None,
        }
    }
    pub(super) fn allocation_bytes(&self) -> usize {
        CHUNK_BYTES
            + self.event.as_ref().map_or(0, event_bytes)
            + self.deferred_final.as_ref().map_or(0, event_bytes)
    }
    fn reset(&mut self) {
        self.wipe_tx();
        self.deadline = Instant::now();
        self.deadline_phase = DeadlinePhase::AwaitingConnection;
        self.admitted = false;
        self.expected_body = None;
        self.head_only = false;
        self.plain_pos = 0;
        self.plain_len = 0;
        self.body_offset = 0;
        self.next_sequence = 1;
        self.flight = None;
        self.request_done = false;
        self.deferred_final = None;
        self.event = None;
        self.closed_event = false;
        self.retired_event = false;
        self.closed_delivered = false;
        self.retired_delivered = false;
        self.tx_len = 0;
        self.tx_pos = 0;
        self.tx_final = false;
        self.head_pos = 0;
        self.record_head = 0;
        self.record_body = 0;
        self.awaiting_response = false;
        self.response_fence = None;
    }
    fn expired_at(&self, now: Instant) -> bool {
        self.deadline_phase != DeadlinePhase::AwaitingConnection && now >= self.deadline
    }
    pub(super) fn header_deadline(&self) -> Option<Instant> {
        (self.deadline_phase == DeadlinePhase::Header).then_some(self.deadline)
    }
    fn negative_close_due_at(&self, now: Instant) -> bool {
        match self.deadline_phase {
            DeadlinePhase::AwaitingConnection => false,
            DeadlinePhase::Header => now >= header_close_alarm(self.deadline),
            DeadlinePhase::Request { .. } => self.expired_at(now),
        }
    }
    pub(super) fn waiting_for_connection(&self) -> bool {
        self.deadline_phase == DeadlinePhase::AwaitingConnection
    }
    pub(super) fn bind_header_deadline(&mut self, deadline: Instant) {
        assert!(self.waiting_for_connection(), "header clock already bound");
        self.deadline_phase = DeadlinePhase::Header;
        self.deadline = deadline;
    }
    #[cfg(test)]
    fn posted_header_at(&mut self, now: Instant) {
        self.bind_header_deadline(now + BUDGET);
    }
    fn request_started_at(&self) -> Option<Instant> {
        match self.deadline_phase {
            DeadlinePhase::AwaitingConnection | DeadlinePhase::Header => None,
            DeadlinePhase::Request { admitted_at } => Some(admitted_at),
        }
    }
    /// Called only after the parser returns a complete valid head. The final
    /// time check must be here too: parsing can cross the old header deadline.
    fn complete_head_at(&mut self, admitted_at: Instant) -> bool {
        if self.deadline_phase != DeadlinePhase::Header
            || self.expired_at(admitted_at)
            || self.negative_close_due_at(admitted_at)
        {
            return false;
        }
        let Some(deadline) = admitted_at.checked_add(REQUEST_BUDGET) else {
            return false;
        };
        self.deadline_phase = DeadlinePhase::Request { admitted_at };
        self.deadline = deadline;
        true
    }
    fn admission_deadline_valid(&self, deadline: Instant, now: Instant) -> bool {
        self.request_started_at().is_some()
            && !self.admitted
            && !self.expired_at(now)
            && deadline > now
            && deadline <= self.deadline
    }
    /// The caller has checked the exact body length/state and original request
    /// lifetime before admitting the parser. This never establishes a new epoch.
    fn admit_until(&mut self, deadline: Instant) {
        debug_assert!(self.request_started_at().is_some());
        debug_assert!(!self.admitted && deadline <= self.deadline);
        self.deadline = deadline;
        self.admitted = true;
    }
    pub(super) fn response_allowed(&self) -> bool {
        self.response_fence.as_ref().is_none_or(|fence| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fence())).unwrap_or(false)
        })
    }
    pub(super) fn retire_response(&mut self) {
        self.wipe_tx();
        self.response_fence = None;
        self.tx_len = 0;
        self.tx_pos = 0;
    }
    fn wipe_tx(&mut self) {
        for byte in self.tx.iter_mut() {
            // SAFETY: this owner is unique and native/SSPI never borrows tx.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(Ordering::SeqCst);
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        self.wipe_tx();
    }
}
fn event_bytes(event: &TlsHttpEvent) -> usize {
    match event {
        TlsHttpEvent::Body { bytes, .. } => bytes.capacity(),
        TlsHttpEvent::Head { head, .. } => {
            head.method.capacity()
                + head.target.capacity()
                + head.headers.capacity() * size_of::<fc_ports::http::Header>()
                + head
                    .headers
                    .iter()
                    .map(|h| h.name.capacity() + h.value.capacity())
                    .sum::<usize>()
        }
        _ => 0,
    }
}

/// One Net-domain owner, sharing the diagnostic's exact native/SSPI lifecycle.
/// The caller must explicitly stop and poll_shutdown until true before Drop.
pub struct TlsHttpHost {
    owner: Owner,
    drain_deadline: Option<Instant>,
}
impl TlsHttpHost {
    /// Certificate selection is read-only. Only an explicit isolated loopback
    /// capability can enter this candidate; normal listening remains gated.
    pub fn open(thumbprint: &str, marker: &str) -> Result<Self, String> {
        super::super::validate_marker(marker).map_err(|e| format!("TLS HTTP marker: {e:?}"))?;
        let thumbprint = CertificateThumbprint::parse(thumbprint)
            .map_err(|e| format!("TLS HTTP certificate: {e:?}"))?;
        let mut owner = Owner::new(marker)?;
        owner.http_mode = true;
        for slot in &mut owner.resources.slots {
            slot.stream = Some(Stream::new());
        }
        if let Err(error) = owner.start(thumbprint) {
            let cleanup = owner.finish();
            return Err(match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; cleanup: {cleanup}"),
            });
        }
        Ok(Self {
            owner,
            drain_deadline: None,
        })
    }
    pub fn port_number(&self) -> u16 {
        self.owner.port_number
    }
    pub fn generation(&self) -> u64 {
        self.owner.generation
    }
    /// Pure owner metadata only. Socket descriptors count this owner's handles,
    /// not kernel TCB/backlog/SSPI allocations or any deadline guarantee.
    pub fn acceptance_snapshot(&self) -> TlsAcceptSnapshot {
        let resources = &self.owner.resources;
        let bare = resources.accept.as_ref();
        TlsAcceptSnapshot {
            generation: self.owner.generation,
            posts: self.owner.report.accept_posts,
            completions: self.owner.report.accept_completions,
            idle_cancellations: self.owner.report.accept_idle_cancellations,
            idle_returns: self.owner.report.accept_idle_returns,
            capacity_rejects: self.owner.report.accept_capacity_rejections,
            transfers: self.owner.report.accept_handoffs,
            ready_slots: resources
                .slots
                .iter()
                .filter(|slot| slot.phase == Phase::Ready)
                .count(),
            bound_slots: resources
                .slots
                .iter()
                .filter(|slot| slot.phase != Phase::Ready && slot.socket != INVALID_SOCKET)
                .count(),
            native_pending: resources
                .slots
                .iter()
                .filter(|slot| slot.pending.is_some())
                .count()
                + usize::from(bare.is_some_and(|bare| bare.pending())),
            owned_sockets: usize::from(resources.listener != INVALID_SOCKET)
                + resources
                    .slots
                    .iter()
                    .filter(|slot| slot.socket != INVALID_SOCKET)
                    .count()
                + usize::from(bare.is_some_and(|bare| bare.socket != INVALID_SOCKET)),
        }
    }
    /// Latest fixed-slot observations only, not peer wire-close evidence or
    /// proof of a precise kernel ESTABLISHED epoch. No native work or locks.
    pub fn close_snapshot(&self) -> TlsCloseSnapshot {
        let mut snapshot = self.owner.report.close_observations;
        snapshot.generation = self.owner.generation;
        snapshot
    }
    pub fn tokens(&self) -> [RequestToken; CONNECTION_SLOTS] {
        std::array::from_fn(|n| self.owner.resources.slots[n].token)
    }
    /// All fixed allocations plus retained DTO/chunk capacities. Excludes
    /// provider/kernel/thread runtime, caller-supplied fence closure storage,
    /// and caller-owned events already returned.
    pub fn app_owned_bytes(&self) -> usize {
        self.owner.app_owned_bytes() + size_of::<Self>() - size_of::<Owner>()
    }
    /// Service only already-due native deadlines between bounded caller work.
    /// Closing remains negative: the original IOCP/Frame debts and exact token
    /// are retained until later poll/shutdown observes their physical returns.
    pub fn service_deadlines(&mut self) -> Result<(), String> {
        self.owner.service_deadlines()
    }
    pub fn poll(&mut self) -> Result<Vec<TlsHttpEvent>, String> {
        self.owner.poll()?;
        Ok(self.owner.take_http_events())
    }
    pub fn admit_body(
        &mut self,
        token: RequestToken,
        exact_limit: u64,
        absolute_deadline: Instant,
    ) -> Result<(), String> {
        let index = self.owner.http_index(token)?;
        let slot = &mut self.owner.resources.slots[index];
        let stream = slot.stream.as_mut().unwrap();
        if slot.phase != Phase::Http
            || stream.expected_body != Some(exact_limit)
            || stream.event.is_some()
            || !stream.admission_deadline_valid(absolute_deadline, Instant::now())
        {
            return Err("HTTP body admission state or absolute deadline".into());
        }
        slot.parser
            .admit_body(exact_limit)
            .map_err(|e| format!("HTTP body admission: {e:?}"))?;
        stream.admit_until(absolute_deadline);
        self.owner.drive(index)
    }
    /// For uploads the coordinator may call this ONLY after the matching real
    /// worker UploadAck. For buffered JSON it means the bounded bytes are owned.
    pub fn ack_body(
        &mut self,
        token: RequestToken,
        sequence: u64,
        next_offset: u64,
    ) -> Result<(), String> {
        let index = self.owner.http_index(token)?;
        let slot = &mut self.owner.resources.slots[index];
        let stream = slot.stream.as_mut().unwrap();
        let flight = stream
            .flight
            .as_ref()
            .ok_or("HTTP body has no pending receipt")?;
        if slot.phase != Phase::Http
            || stream.event.is_some()
            || flight.sequence != sequence
            || flight.end != next_offset
        {
            return Err("HTTP body receipt mismatch".into());
        }
        let eof = flight.eof;
        stream.flight = None;
        stream.body_offset = next_offset;
        stream.next_sequence = sequence
            .checked_add(1)
            .ok_or("HTTP body sequence exhausted")?;
        if eof {
            stream.request_done = true;
        }
        self.owner.drive(index)
    }
    /// Install once on the exact live request before its response starts. The
    /// callback must be bounded memory-only work (atomics/try-locks), never wait
    /// for a worker, SQL, file IO or ordinary mutex acquisition. Its allocation
    /// remains caller-budgeted; this owner retains only one Arc per native slot.
    /// Each positive check linearizes immediately before chunk acceptance or
    /// actual WSASend submission; already submitted IO cannot be retracted.
    pub fn set_response_fence(
        &mut self,
        token: RequestToken,
        fence: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<(), String> {
        let index = self.owner.http_index(token)?;
        let slot = &mut self.owner.resources.slots[index];
        let stream = slot.stream.as_mut().unwrap();
        if slot.phase != Phase::Http
            || stream.request_started_at().is_none()
            || slot.response_body.is_some()
            || stream.response_fence.is_some()
        {
            return Err("HTTP response fence requires original live request".into());
        }
        stream.response_fence = Some(fence);
        Ok(())
    }
    pub fn respond(
        &mut self,
        token: RequestToken,
        head: ResponseHead,
        first: &[u8],
        eof: bool,
    ) -> Result<(), String> {
        let index = self.owner.http_index(token)?;
        let slot = &mut self.owner.resources.slots[index];
        let stream = slot.stream.as_mut().unwrap();
        if slot.phase != Phase::Http
            || slot.pending.is_some()
            || slot.frame.is_none()
            || stream.event.is_some()
            || stream.flight.is_some()
            || (stream.admitted && !stream.request_done)
            || (!stream.admitted && stream.request_started_at().is_none())
            || (!stream.admitted && head.status < 400)
        {
            return Err("HTTP response before request retirement or while busy".into());
        }
        if !stream.response_allowed() {
            self.owner.close_slot(index, true)?;
            return Err("HTTP response authorization revoked".into());
        }
        if first.len() > CHUNK_BYTES
            || (!stream.head_only && eof && head.content_length != first.len() as u64)
        {
            return Err("HTTP initial response bounds".into());
        }
        let mut sender = super::super::http_date::body_sender(token, head, stream.head_only)
            .map_err(|e| format!("HTTP response head: {e:?}"))?;
        let first = if stream.head_only { &[][..] } else { first };
        let receipt = if first.is_empty() {
            None
        } else {
            Some(
                sender
                    .begin_chunk(token, first.len())
                    .map_err(|e| format!("HTTP response chunk: {e:?}"))?,
            )
        };
        // A rejected request is deliberately not admitted or drained. This
        // unique Frame is discarded before its connection-close error response.
        slot.frame.as_mut().unwrap().wipe();
        stream.plain_pos = 0;
        stream.plain_len = 0;
        stream.deferred_final = None;
        stream.tx[..first.len()].copy_from_slice(first);
        stream.tx_len = first.len();
        stream.tx_pos = 0;
        stream.tx_final = stream.head_only || eof;
        stream.head_pos = 0;
        stream.awaiting_response = false;
        slot.response_body = Some(sender);
        slot.response_chunk = receipt;
        slot.phase = Phase::Response;
        self.owner.drive(index)
    }
    pub fn send_response_body(
        &mut self,
        token: RequestToken,
        bytes: &[u8],
        eof: bool,
    ) -> Result<(), String> {
        let index = self.owner.http_index(token)?;
        let slot = &mut self.owner.resources.slots[index];
        let stream = slot.stream.as_mut().unwrap();
        if stream.head_only {
            // HEAD is terminal after its actual header ACK; no later source
            // chunk, even empty, can reopen it or create ResponseReady.
            return Err("HTTP HEAD has no response body".into());
        }
        let sender = slot.response_body.as_mut().ok_or("HTTP missing response")?;
        if bytes.len() > CHUNK_BYTES
            || slot.phase != Phase::Response
            || !stream.awaiting_response
            || stream.event.is_some()
            || slot.pending.is_some()
            || slot.frame.is_none()
            || (eof && bytes.len() as u64 != sender.remaining_bytes())
            || (!eof && bytes.is_empty())
        {
            return Err("HTTP response source state or size".into());
        }
        if !stream.response_allowed() {
            self.owner.close_slot(index, true)?;
            return Err("HTTP response authorization revoked".into());
        }
        slot.response_chunk = if bytes.is_empty() {
            None
        } else {
            Some(
                sender
                    .begin_chunk(token, bytes.len())
                    .map_err(|e| format!("HTTP response chunk: {e:?}"))?,
            )
        };
        stream.tx[..bytes.len()].copy_from_slice(bytes);
        stream.tx_len = bytes.len();
        stream.tx_pos = 0;
        stream.tx_final = eof;
        stream.awaiting_response = false;
        self.owner.drive(index)
    }
    pub fn cancel(&mut self, token: RequestToken) -> Result<(), String> {
        let index = token.slot();
        if index >= CONNECTION_SLOTS || self.owner.resources.slots[index].token != token {
            return Err("stale TLS HTTP cancellation".into());
        }
        if self.owner.resources.slots[index].phase == Phase::Closed {
            return Ok(());
        }
        self.owner.close_slot(index, true)
    }
    pub fn rearm(&mut self, token: RequestToken) -> Result<RequestToken, String> {
        self.owner.rearm_http(token)
    }
    pub fn request_stop(&mut self) -> Result<(), String> {
        self.drain_deadline
            .get_or_insert_with(|| Instant::now() + BUDGET);
        self.owner.stop()
    }
    pub fn poll_shutdown(&mut self) -> Result<bool, String> {
        if !self.owner.stopping {
            return Err("TLS HTTP stop not requested".into());
        }
        let done = self.owner.poll_http_drain()?;
        if !done
            && self
                .drain_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err("TLS HTTP drain deadline; owner retained and replacement forbidden".into());
        }
        Ok(done)
    }
}

#[cfg(test)]
mod tests;

impl Owner {
    pub(super) fn expired(&self, index: usize) -> bool {
        if self.http_mode {
            self.resources.slots[index]
                .stream
                .as_ref()
                .is_some_and(|stream| stream.negative_close_due_at(Instant::now()))
        } else {
            self.started.elapsed() >= BUDGET
        }
    }
    fn http_index(&self, token: RequestToken) -> Result<usize, String> {
        let index = token.slot();
        if !self.http_mode
            || self.stopping
            || index >= CONNECTION_SLOTS
            || self.resources.slots[index].token != token
            || matches!(
                self.resources.slots[index].phase,
                Phase::Ready | Phase::Closed
            )
            || self.expired(index)
        {
            return Err("stale or closed TLS HTTP request".into());
        }
        Ok(index)
    }
    fn take_http_events(&mut self) -> Vec<TlsHttpEvent> {
        let mut events = Vec::with_capacity(MAX_POLL_EVENTS);
        for slot in &mut self.resources.slots {
            let stream = slot.stream.as_mut().unwrap();
            if let Some(event) = stream.event.take() {
                events.push(event);
            }
            if stream.closed_event {
                stream.closed_event = false;
                stream.closed_delivered = true;
                events.push(TlsHttpEvent::Closed { token: slot.token });
            }
            if stream.retired_event {
                stream.retired_event = false;
                stream.retired_delivered = true;
                events.push(TlsHttpEvent::Retired(slot.token));
            }
        }
        debug_assert!(events.len() <= MAX_POLL_EVENTS);
        events
    }
    fn submit_http_call(&mut self, index: usize, call: Call) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        let mut frame = slot.frame.take().ok_or("HTTP TLS frame busy")?;
        frame.call = call;
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
                Err(format!("TLS HTTP mailbox: {error:?}"))
            }
        }
    }
    pub(super) fn drive_http(&mut self, index: usize) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        if slot.pending.is_some() || slot.frame.is_none() {
            return Ok(());
        }
        match slot.phase {
            Phase::Handshake => {
                if slot.frame.as_ref().unwrap().input_len == 0 {
                    self.receive(index)
                } else {
                    self.submit_http_call(index, Call::Handshake)
                }
            }
            Phase::Http => {
                let stream = slot.stream.as_ref().unwrap();
                if stream.event.is_some()
                    || stream.flight.is_some()
                    || stream.request_done
                    || stream.request_started_at().is_some() && !stream.admitted
                {
                    return Ok(());
                }
                if stream.plain_pos < stream.plain_len {
                    return self.parse_http_plain(index);
                }
                if stream.deferred_final.is_some() && slot.frame.as_ref().unwrap().input_len == 0 {
                    return self.publish_http_final(index);
                }
                if slot.parser.is_complete() && stream.deferred_final.is_none() {
                    let token = slot.token;
                    let sequence = stream.next_sequence;
                    let offset = stream.body_offset;
                    slot.stream.as_mut().unwrap().deferred_final = Some(TlsHttpEvent::Body {
                        token,
                        sequence,
                        offset,
                        bytes: Vec::new(),
                        eof: true,
                    });
                    return self.drive_http(index);
                }
                if slot.frame.as_ref().unwrap().input_len == 0 {
                    self.receive(index)
                } else {
                    self.submit_http_call(index, Call::Decrypt)
                }
            }
            Phase::Response => {
                if slot.stream.as_ref().unwrap().awaiting_response {
                    return Ok(());
                }
                self.prepare_http_record(index)
            }
            Phase::Shutdown => {
                slot.frame.as_mut().unwrap().input_len = 0;
                self.submit_http_call(index, Call::Shutdown)
            }
            Phase::Ready | Phase::Closed => Ok(()),
        }
    }
    fn parse_http_plain(&mut self, index: usize) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        let frame = slot.frame.as_ref().unwrap();
        let stream = slot.stream.as_mut().unwrap();
        let progress = match slot
            .parser
            .feed(&frame.output[stream.plain_pos..stream.plain_len])
        {
            Ok(progress) => progress,
            Err(_) => return self.close_slot(index, true),
        };
        stream.plain_pos += progress.consumed;
        match progress.event {
            Event::NeedHead => self.drive(index),
            Event::Head(head) => {
                let admitted_at = Instant::now();
                if !stream.complete_head_at(admitted_at) {
                    return self.close_slot(index, true);
                }
                stream.head_only = head.method == "HEAD";
                stream.expected_body = Some(head.content_length.unwrap_or(0));
                stream.event = Some(TlsHttpEvent::Head { head, admitted_at });
                self.report.parsed_requests += 1;
                Ok(())
            }
            Event::Body { bytes, eof } => {
                let event = TlsHttpEvent::Body {
                    token: slot.token,
                    sequence: stream.next_sequence,
                    offset: stream.body_offset,
                    bytes: bytes.to_vec(),
                    eof,
                };
                if eof {
                    stream.deferred_final = Some(event);
                    // No more plaintext is permitted after the declared body.
                    if stream.plain_pos != stream.plain_len {
                        return self.close_slot(index, true);
                    }
                    self.drive(index)
                } else {
                    stream.flight = Some(BodyFlight {
                        sequence: stream.next_sequence,
                        end: stream.body_offset + bytes.len() as u64,
                        eof: false,
                    });
                    stream.event = Some(event);
                    Ok(())
                }
            }
            _ => self.close_slot(index, true),
        }
    }
    fn publish_http_final(&mut self, index: usize) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        let stream = slot.stream.as_mut().unwrap();
        let event = stream
            .deferred_final
            .take()
            .ok_or("HTTP missing final body")?;
        let TlsHttpEvent::Body {
            sequence,
            offset,
            ref bytes,
            eof: true,
            ..
        } = event
        else {
            return Err("HTTP invalid deferred EOF".into());
        };
        stream.flight = Some(BodyFlight {
            sequence,
            end: offset + bytes.len() as u64,
            eof: true,
        });
        stream.event = Some(event);
        Ok(())
    }
    pub(super) fn complete_http_step(&mut self, index: usize, step: Step) -> Result<(), String> {
        match step {
            Step::Handshake { consumed, complete } => {
                let slot = &mut self.resources.slots[index];
                let frame = slot.frame.as_mut().unwrap();
                if consumed == 0 || consumed > frame.input_len || slot.phase != Phase::Handshake {
                    return Err("HTTP handshake bounds".into());
                }
                frame.input.copy_within(consumed..frame.input_len, 0);
                frame.input_len -= consumed;
                if complete {
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
                    return Err("HTTP plaintext bounds".into());
                }
                frame.input.copy_within(consumed..frame.input_len, 0);
                frame.input_len -= consumed;
                let stream = slot.stream.as_mut().unwrap();
                stream.plain_pos = 0;
                stream.plain_len = frame.output_len;
                if frame.output_len == 0 {
                    self.drive(index)
                } else {
                    self.parse_http_plain(index)
                }
            }
            Step::NeedInput => {
                if self.resources.slots[index]
                    .stream
                    .as_ref()
                    .unwrap()
                    .deferred_final
                    .is_some()
                {
                    // An incomplete extra record is not an excuse to read beyond
                    // the completed request before its final worker ACK.
                    self.close_slot(index, true)
                } else {
                    self.receive(index)
                }
            }
            Step::PeerClosed => {
                let slot = &mut self.resources.slots[index];
                slot.frame.as_mut().unwrap().input_len = 0;
                if slot.stream.as_ref().unwrap().deferred_final.is_some() {
                    self.publish_http_final(index)
                } else {
                    let _ = slot.parser.finish_eof();
                    self.close_slot(index, true)
                }
            }
            Step::Encrypted => {
                if self.resources.slots[index].phase != Phase::Response {
                    return Err("HTTP unexpected encryption".into());
                }
                self.start_send(index, false)
            }
            Step::Shutdown { complete } => {
                let slot = &mut self.resources.slots[index];
                if slot.phase != Phase::Shutdown {
                    return Err("HTTP unexpected shutdown".into());
                }
                slot.shutdown_complete = complete;
                if slot.frame.as_ref().unwrap().output_len != 0 {
                    self.start_send(index, true)
                } else {
                    self.close_slot(index, !complete)
                }
            }
            Step::Retired => Err("HTTP unsolicited retirement".into()),
        }
    }
    fn prepare_http_record(&mut self, index: usize) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        let stream = slot.stream.as_mut().unwrap();
        let sender = slot
            .response_body
            .as_ref()
            .ok_or("HTTP response owner missing")?;
        let frame = slot.frame.as_mut().unwrap();
        let head = &sender.head_bytes()[stream.head_pos..];
        let head_count = head.len().min(PLAINTEXT_BYTES);
        let body_count = (PLAINTEXT_BYTES - head_count).min(stream.tx_len - stream.tx_pos);
        if head_count + body_count == 0 {
            return self.finish_http_chunk(index);
        }
        frame.input[..head_count].copy_from_slice(&head[..head_count]);
        frame.input[head_count..head_count + body_count]
            .copy_from_slice(&stream.tx[stream.tx_pos..stream.tx_pos + body_count]);
        frame.input_len = head_count + body_count;
        stream.record_head = head_count;
        stream.record_body = body_count;
        self.submit_http_call(index, Call::Encrypt)
    }
    pub(super) fn complete_http_send(&mut self, index: usize) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        match slot.phase {
            Phase::Response => {
                let stream = slot.stream.as_mut().unwrap();
                stream.head_pos += stream.record_head;
                stream.tx_pos += stream.record_body;
                stream.record_head = 0;
                stream.record_body = 0;
                if stream.head_pos == slot.response_body.as_ref().unwrap().head_bytes().len()
                    && stream.tx_pos == stream.tx_len
                {
                    self.finish_http_chunk(index)
                } else {
                    self.drive(index)
                }
            }
            Phase::Shutdown => {
                self.report.shutdown_tokens_sent += 1;
                if slot.shutdown_complete {
                    self.close_slot(index, false)
                } else {
                    self.drive(index)
                }
            }
            Phase::Handshake | Phase::Http => self.drive(index),
            Phase::Ready => Err("send completion for unbound Ready business slot".into()),
            Phase::Closed => Ok(()),
        }
    }
    fn finish_http_chunk(&mut self, index: usize) -> Result<(), String> {
        let slot = &mut self.resources.slots[index];
        let stream = slot.stream.as_mut().unwrap();
        let sender = slot.response_body.as_mut().ok_or("HTTP response missing")?;
        if let Some(chunk) = slot.response_chunk.take() {
            sender
                .acknowledge_chunk(chunk, stream.tx_len)
                .map_err(|e| format!("HTTP real send ACK: {e:?}"))?;
        }
        if stream.tx_final {
            sender
                .finish(slot.token)
                .map_err(|e| format!("HTTP response source EOF: {e:?}"))?;
            stream.event = Some(TlsHttpEvent::ResponseComplete(slot.token));
            self.report.responses += 1;
            slot.phase = Phase::Shutdown;
            slot.frame.as_mut().unwrap().input_len = 0;
            self.drive(index)
        } else {
            stream.awaiting_response = true;
            stream.event = Some(TlsHttpEvent::ResponseReady(slot.token));
            Ok(())
        }
    }
    fn rearm_http(&mut self, token: RequestToken) -> Result<RequestToken, String> {
        let index = token.slot();
        if self.stopping || index >= CONNECTION_SLOTS {
            return Err("TLS HTTP stopping or invalid slot".into());
        }
        let slot = &mut self.resources.slots[index];
        let stream = slot.stream.as_ref().ok_or("not HTTP owner")?;
        if slot.token != token
            || slot.phase != Phase::Closed
            || slot.socket != INVALID_SOCKET
            || slot.pending.is_some()
            || !slot.retired
            || slot.frame.is_none()
            || stream.event.is_some()
            || stream.closed_event
            || stream.retired_event
            || !stream.closed_delivered
            || !stream.retired_delivered
        {
            return Err("TLS HTTP slot not physically retired/delivered".into());
        }
        let next = self
            .issuer
            .issue(index)
            .map_err(|e| format!("TLS HTTP serial: {e:?}"))?;
        slot.frame.as_mut().unwrap().wipe();
        slot.frame.as_mut().unwrap().token = next;
        slot.token = next;
        slot.phase = Phase::Ready;
        slot.parser = Parser::new(next, true);
        slot.response_body = None;
        slot.response_chunk = None;
        slot.stream.as_mut().unwrap().reset();
        slot.context_submitted = false;
        slot.retired = false;
        slot.shutdown_complete = false;
        slot.send_len = 0;
        slot.sent = 0;
        slot.close_after_send = false;
        Ok(next)
    }
    fn poll_http_drain(&mut self) -> Result<bool, String> {
        if self.drained {
            return Ok(true);
        }
        self.poll()?;
        if self
            .resources
            .slots
            .iter()
            .any(|slot| slot.pending.is_some() || slot.socket != INVALID_SOCKET)
            || self.resources.listener != INVALID_SOCKET
            || !self.bare_released()
        {
            return Ok(false);
        }
        if let Some(worker) = self.resources.worker.as_mut() {
            if worker.has_in_flight()
                || !worker
                    .reap_nonblocking()
                    .map_err(|e| format!("TLS HTTP actual worker reap: {e:?}"))?
            {
                return Ok(false);
            }
        }
        if self.report.submissions != self.report.completions
            || !self.accept_ledger_matches()
            || self.resources.slots.iter().any(|slot| slot.frame.is_none())
            || self.report.sspi_submissions != self.report.sspi_completions
            || self.report.sspi_completions != self.report.returned_frames
        {
            return Err("TLS HTTP final native ledger mismatch".into());
        }
        for (index, slot) in self.resources.slots.iter_mut().enumerate() {
            if !slot.retired {
                slot.retired = true;
                slot.wipe_retired_http();
                slot.stream.as_mut().unwrap().retired_event = true;
            }
            if let Some(observation) = self.report.close_observations.headers[index]
                .as_mut()
                .filter(|o| o.token == slot.token)
            {
                observation
                    .retirement_confirmed_at
                    .get_or_insert_with(Instant::now);
            }
        }
        self.report.slot_frames_at_drain = CONNECTION_SLOTS;
        // SAFETY: all native packets returned and the actual OS SSPI thread
        // terminated. No in-flight operation can reference these resources.
        unsafe {
            if !self.resources.port.is_invalid() {
                CloseHandle(self.resources.port)
                    .map_err(|e| format!("TLS HTTP IOCP close: {e}"))?;
                self.resources.port = HANDLE::default();
            }
            if self.resources.winsock {
                check(WSACleanup(), "TLS HTTP WSACleanup")?;
                self.resources.winsock = false;
            }
        }
        self.drained = true;
        Ok(true)
    }
}
