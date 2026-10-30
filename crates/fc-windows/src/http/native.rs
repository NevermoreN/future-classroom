//! Native ownership policy: NO_ERROR and ERROR_IO_PENDING both retain their
//! OVERLAPPED until IOCP. Synchronous failure is not an accepted operation.
//! No skip-completion notification modes or event low bits are enabled.
use super::lifecycle::{submission_accepted, Ledger, Operation};
use fc_ports::http::*;
use std::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::{align_of, size_of, ManuallyDrop},
    ptr,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows::{
    core::{PCSTR, PCWSTR},
    Win32::{Foundation::*, Networking::HttpServer::*, System::IO::*},
};
const HEADER_BUFFER_BYTES: usize = 32 * 1024;
const RESPONSE_HEADERS: usize = 32;
const BODY_TIMEOUT: Duration = Duration::from_secs(120);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static POISONED: AtomicBool = AtomicBool::new(false);
pub(super) fn request_disconnected(code: u32) -> bool {
    [
        ERROR_CONNECTION_INVALID.0,
        ERROR_NOT_FOUND.0,
        ERROR_OPERATION_ABORTED.0,
        ERROR_NETNAME_DELETED.0,
        ERROR_CONNECTION_ABORTED.0,
        ERROR_BROKEN_PIPE.0,
    ]
    .contains(&code)
}
fn status(code: u32) -> Result<(), HttpError> {
    if code == 0 {
        Ok(())
    } else {
        Err(HttpError::Native(code))
    }
}
// BOOL-returning windows bindings capture the Win32 error in Error::code().
// Decode only HRESULT_FROM_WIN32 values; preserve other facilities rather than
// mistaking their low word for an unrelated Win32 status. Never reread the
// thread's mutable last-error slot after the binding has constructed its Error.
fn win32_hresult_code(hresult: u32) -> u32 {
    if hresult & 0xffff_0000 == 0x8007_0000 {
        hresult & 0xffff
    } else {
        hresult
    }
}
// This evidence is created only by the synchronous rejection of cancellation
// for this exact native request. Accepted/PENDING cancellation is not proof that
// the peer had already disconnected, nor is an absent/already-pending cancel.
#[derive(Clone, Copy, Debug)]
enum CancelWitness {
    Unconfirmed,
    Disconnected { request_id: u64, code: u32 },
}
fn unaccepted_body_error(code: u32, request_id: u64, witness: CancelWitness) -> u32 {
    if code == ERROR_INVALID_FUNCTION.0 && request_id != 0 {
        if let CancelWitness::Disconnected {
            request_id: canceled_id,
            code: cancel_code,
        } = witness
        {
            if request_id == canceled_id && request_disconnected(cancel_code) {
                return cancel_code;
            }
        }
    }
    code
}
#[repr(C, align(16))]
struct RequestBuffer([u8; HEADER_BUFFER_BYTES]);
#[repr(C, align(16))]
struct NativeMemory {
    operation: UnsafeCell<OVERLAPPED>,
    cancel: UnsafeCell<OVERLAPPED>,
    request: UnsafeCell<RequestBuffer>,
    body: UnsafeCell<[u8; CHUNK_BYTES]>,
    response: UnsafeCell<HTTP_RESPONSE_V2>,
    chunk: UnsafeCell<HTTP_DATA_CHUNK>,
    send: UnsafeCell<[u8; CHUNK_BYTES]>,
    header_bytes: UnsafeCell<[u8; MAX_HEADER_BYTES]>,
    headers: UnsafeCell<[HTTP_UNKNOWN_HEADER; RESPONSE_HEADERS]>,
}
impl NativeMemory {
    fn new() -> Box<Self> {
        // SAFETY: C structs contain only nullable pointers, scalar integers and
        // byte arrays. Zero is valid for every field. Allocate directly on heap,
        // avoiding a large temporary on the Win7 x86 default thread stack.
        unsafe {
            let layout = std::alloc::Layout::new::<Self>();
            let p = std::alloc::alloc_zeroed(layout).cast::<Self>();
            if p.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            Box::from_raw(p)
        }
    }
    fn request(&self) -> *mut HTTP_REQUEST_V2 {
        self.request.get().cast()
    }
}
struct Slot {
    memory: ManuallyDrop<Box<NativeMemory>>,
    retained: bool,
    request_id: u64,
    deadline: Option<Instant>,
    body_limit: Option<u64>,
    body_read: u64,
    body_eof: bool,
    response_length: Option<u64>,
    response_sent: u64,
    canceled: bool,
    terminal_error: u32,
}
impl Slot {
    fn new() -> Self {
        Self {
            memory: ManuallyDrop::new(NativeMemory::new()),
            retained: false,
            request_id: 0,
            deadline: None,
            body_limit: None,
            body_read: 0,
            body_eof: false,
            response_length: None,
            response_sent: 0,
            canceled: false,
            terminal_error: 0,
        }
    }
    fn reset(&mut self) {
        self.request_id = 0;
        self.deadline = None;
        self.body_limit = None;
        self.body_read = 0;
        self.body_eof = false;
        self.response_length = None;
        self.response_sent = 0;
        self.canceled = false;
        self.terminal_error = 0;
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        if !self.retained {
            // SAFETY: HttpHost drops normally only after every IOCP ownership record has
            // returned. On uncertain cleanup it marks retained and never invokes this.
            unsafe {
                ManuallyDrop::drop(&mut self.memory);
            }
        }
    }
}
struct Resources {
    queue: HANDLE,
    iocp: HANDLE,
    session: u64,
    group: u64,
    initialized: bool,
}
impl Resources {
    fn empty() -> Self {
        Self {
            queue: HANDLE::default(),
            iocp: HANDLE::default(),
            session: 0,
            group: 0,
            initialized: false,
        }
    }
    // SAFETY for native calls: each resource is an owned live handle/ID recorded
    // by initialize; this function is reachable only with zero outstanding I/O.
    // A successful close clears that value before any later fallible close.
    fn close(&mut self) -> Result<(), HttpError> {
        // Called only after the complete ownership ledger is empty. Closing URL
        // configuration is not evidence that a submitted native pointer returned.
        if self.group != 0 {
            status(unsafe { HttpCloseUrlGroup(self.group) })?;
            self.group = 0;
        }
        if !self.queue.is_invalid() {
            status(unsafe { HttpCloseRequestQueue(self.queue) })?;
            self.queue = HANDLE::default();
        }
        if self.session != 0 {
            status(unsafe { HttpCloseServerSession(self.session) })?;
            self.session = 0;
        }
        if !self.iocp.is_invalid() {
            unsafe { CloseHandle(self.iocp) }.map_err(|e| HttpError::Native(e.code().0 as u32))?;
            self.iocp = HANDLE::default();
        }
        if self.initialized {
            status(unsafe { HttpTerminate(HTTP_INITIALIZE_SERVER, None) })?;
            self.initialized = false;
        }
        Ok(())
    }
}
/// Create and drive entirely on the single HTTP execution domain. !Send/!Sync
/// keeps native completion ownership on that domain; application workers receive
/// only owned messages and return RequestToken-correlated bounded responses.
pub struct HttpHost {
    resources: Resources,
    slots: [Slot; MAX_REQUESTS],
    ledger: Ledger,
    prefix: ListenPrefix,
    stopping: Option<Instant>,
    metrics: HttpMetrics,
    _domain: PhantomData<Rc<()>>,
}
impl HttpHost {
    pub fn open(prefix: ListenPrefix) -> Result<Self, HttpError> {
        if POISONED.load(Ordering::Acquire) {
            return Err(HttpError::Retained);
        }
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| HttpError::Busy)?;
        let ledger = match Ledger::new() {
            Ok(ledger) => ledger,
            Err(e) => {
                ACTIVE.store(false, Ordering::Release);
                return Err(e);
            }
        };
        let generation = ledger.generation;
        let mut host = Self {
            resources: Resources::empty(),
            slots: std::array::from_fn(|_| Slot::new()),
            ledger,
            prefix,
            stopping: None,
            metrics: HttpMetrics {
                generation,
                native_allocation_bytes: MAX_REQUESTS * size_of::<NativeMemory>(),
                ..HttpMetrics::default()
            },
            _domain: PhantomData,
        };
        host.initialize()?;
        host.replenish()?;
        Ok(host)
    }
    // SAFETY for native calls in this function: All configuration pointers here are valid stack values for synchronous calls; no OVERLAPPED or borrowed argument is retained. Resources record each acquired handle before the next fallible call.
    fn initialize(&mut self) -> Result<(), HttpError> {
        assert!(align_of::<RequestBuffer>() >= align_of::<HTTP_REQUEST_V2>());
        let version = HTTPAPI_VERSION {
            HttpApiMajorVersion: 2,
            HttpApiMinorVersion: 0,
        };
        status(unsafe { HttpInitialize(version, HTTP_INITIALIZE_SERVER, None) })?;
        self.resources.initialized = true;
        status(unsafe { HttpCreateServerSession(version, &mut self.resources.session, None) })?;
        status(unsafe {
            HttpCreateUrlGroup(self.resources.session, &mut self.resources.group, None)
        })?;
        status(unsafe {
            HttpCreateRequestQueue(
                version,
                PCWSTR::null(),
                None,
                None,
                &mut self.resources.queue,
            )
        })?;
        self.resources.iocp = unsafe {
            CreateIoCompletionPort(
                self.resources.queue,
                None,
                self.ledger.generation as usize,
                1,
            )
        }
        .map_err(|e| HttpError::Native(e.code().0 as u32))?;
        let queue_len = MAX_REQUESTS as u32;
        status(unsafe {
            HttpSetRequestQueueProperty(
                self.resources.queue,
                HttpServerQueueLengthProperty,
                ptr::from_ref(&queue_len).cast(),
                size_of::<u32>() as u32,
                None,
                None,
            )
        })?;
        let binding = HTTP_BINDING_INFO {
            Flags: HTTP_PROPERTY_FLAGS { _bitfield: 1 },
            RequestQueueHandle: self.resources.queue,
        };
        status(unsafe {
            HttpSetUrlGroupProperty(
                self.resources.group,
                HttpServerBindingProperty,
                ptr::from_ref(&binding).cast(),
                size_of::<HTTP_BINDING_INFO>() as u32,
            )
        })?;
        // HTTP_TIMEOUT_LIMIT_INFO documents that application HeaderWait and
        // IdleConnection apply only AFTER a first request is routed (subsequent
        // keep-alive requests). The first incomplete request uses driver defaults;
        // neither URL-group nor server-session configuration changes that. We
        // deliberately do not modify persistent, driver-wide HTTP settings. A
        // first-connection 5s guarantee remains an unvalidated P6-11 release gate.
        let timeout = HTTP_TIMEOUT_LIMIT_INFO {
            Flags: HTTP_PROPERTY_FLAGS { _bitfield: 1 },
            EntityBody: 120,
            DrainEntityBody: 5,
            RequestQueue: 5,
            IdleConnection: 5,
            HeaderWait: 5,
            MinSendRate: 0,
        };
        status(unsafe {
            HttpSetUrlGroupProperty(
                self.resources.group,
                HttpServerTimeoutsProperty,
                ptr::from_ref(&timeout).cast(),
                size_of::<HTTP_TIMEOUT_LIMIT_INFO>() as u32,
            )
        })?;
        let prefix: Vec<u16> = self.prefix.as_str().encode_utf16().chain(Some(0)).collect();
        status(unsafe {
            HttpAddUrlToUrlGroup(
                self.resources.group,
                PCWSTR(prefix.as_ptr()),
                self.ledger.generation,
                None,
            )
        })
    }
    pub fn generation(&self) -> u64 {
        self.ledger.generation
    }
    pub fn state(&self) -> HttpState {
        self.ledger.state
    }
    pub fn metrics(&self) -> HttpMetrics {
        HttpMetrics {
            active_slots: self.ledger.active(),
            in_flight: self.ledger.in_flight(),
            ..self.metrics
        }
    }
    // SAFETY for native calls in this function: Each reserved slot owns aligned heap storage. The ledger registers ownership before submission; no reference into a cell is retained across the call, and success/PENDING keep it alive until IOCP.
    fn replenish(&mut self) -> Result<(), HttpError> {
        while self.ledger.state == HttpState::Running
            && self.ledger.heads() < 2
            && self.ledger.active() < MAX_REQUESTS
        {
            let t = self.ledger.reserve()?;
            let n = t.slot();
            self.slots[n].reset();
            // No references into these cells survive the native operation.
            unsafe {
                ptr::write_bytes(self.slots[n].memory.request.get(), 0, 1);
                *self.slots[n].memory.operation.get() = OVERLAPPED::default();
            }
            self.ledger.begin(t, Operation::Head)?;
            self.submitted();
            let code = unsafe {
                HttpReceiveHttpRequest(
                    self.resources.queue,
                    0,
                    HTTP_RECEIVE_HTTP_REQUEST_FLAGS(0),
                    self.slots[n].memory.request(),
                    HEADER_BUFFER_BYTES as u32,
                    None,
                    Some(self.slots[n].memory.operation.get()),
                )
            };
            if !self.accepted(code) {
                self.trace_rejected("receive-head", n, code);
                self.ledger.complete(n, self.ledger.generation, false)?;
                self.slots[n].request_id =
                    unsafe { (*self.slots[n].memory.request()).Base.RequestId };
                if self.slots[n].request_id != 0 {
                    self.cancel(t, code)?;
                } else {
                    self.ledger.release(t)?;
                    return Err(HttpError::Native(code));
                }
            }
        }
        Ok(())
    }
    fn submitted(&mut self) {
        self.metrics.submitted += 1;
        self.metrics.peak_in_flight = self.metrics.peak_in_flight.max(self.ledger.in_flight());
    }
    fn accepted(&mut self, code: u32) -> bool {
        if submission_accepted(code) {
            if code == 0 {
                self.metrics.immediate_success += 1;
            } else {
                self.metrics.pending += 1;
            }
            true
        } else {
            self.metrics.immediate_errors += 1;
            false
        }
    }
    fn trace_rejected(&self, stage: &'static str, slot: usize, code: u32) {
        // Static operation labels and counters only; no client-controlled data.
        eprintln!(
            "HTTP_IOCP_REQUEST_ERROR stage={stage} slot={slot} code={code} state={:?} in_flight={} body_read={} canceled={}",
            self.ledger.state,
            self.ledger.in_flight(),
            self.slots[slot].body_read,
            self.slots[slot].canceled
        );
    }
    fn request(&self, t: RequestToken) -> Result<usize, HttpError> {
        self.ledger.check(t)?;
        if self.ledger.state != HttpState::Running {
            return Err(HttpError::Stopping);
        }
        let n = t.slot();
        if self.slots[n].canceled || self.slots[n].request_id == 0 {
            return Err(HttpError::StaleRequest);
        }
        Ok(n)
    }
    /// Explicitly request one bounded body chunk after routing/authentication and
    /// upload reservation. Repeated calls retain the original absolute deadline.
    // SAFETY for native calls in this function: The exact token and vacant operation are checked before using its fixed body/OVERLAPPED addresses. A body pointer is not read or overwritten until completion, including cancellation.
    pub fn receive_body(&mut self, t: RequestToken, limit: u64) -> Result<(), HttpError> {
        let n = self.request(t)?;
        if limit > MAX_TRANSFER_BYTES {
            return Err(HttpError::TooLarge);
        }
        if self.ledger.bodies() >= 2
            || (self.slots[n].body_limit.is_none()
                && self
                    .slots
                    .iter()
                    .filter(|s| s.body_limit.is_some() && !s.body_eof)
                    .count()
                    >= 2)
        {
            return Err(HttpError::Busy);
        }
        let s = &mut self.slots[n];
        if s.response_length.is_some() || s.body_eof {
            return Err(HttpError::InvalidRequest);
        }
        if let Some(old) = s.body_limit {
            if old != limit {
                return Err(HttpError::InvalidRequest);
            }
        } else {
            s.body_limit = Some(limit);
            s.deadline = Some(Instant::now() + BODY_TIMEOUT);
        }
        // One extra byte distinguishes exact limit + EOF from truncated input.
        let remaining = limit.checked_sub(s.body_read).ok_or(HttpError::TooLarge)?;
        let read = (remaining.saturating_add(1).min(CHUNK_BYTES as u64)) as u32;
        self.ledger.begin(t, Operation::Body)?;
        unsafe {
            *self.slots[n].memory.operation.get() = OVERLAPPED::default();
        }
        self.submitted();
        let code = unsafe {
            HttpReceiveRequestEntityBody(
                self.resources.queue,
                self.slots[n].request_id,
                0,
                self.slots[n].memory.body.get().cast(),
                read,
                None,
                Some(self.slots[n].memory.operation.get()),
            )
        };
        if !self.accepted(code) {
            self.trace_rejected("receive-body", n, code);
            self.ledger.complete(n, self.ledger.generation, false)?;
            if code == ERROR_HANDLE_EOF.0 {
                self.slots[n].body_eof = true;
                return Ok(());
            }
            let request_id = self.slots[n].request_id;
            let witness = self.cancel_with_witness(t, code)?;
            // A truncated peer can make HTTP.sys reject an unaccepted read with
            // INVALID_FUNCTION. Do not classify that status alone as a peer
            // disconnect: require cancellation of the SAME RequestId to return
            // an already-disconnected error synchronously. The rejected read
            // and rejected cancel each withdrew only their own ledger entry;
            // any independently accepted I/O still needs its actual IOCP packet.
            let classified = unaccepted_body_error(code, request_id, witness);
            if classified != code {
                eprintln!(
                    "HTTP_IOCP_REQUEST_ERROR stage=receive-body-confirmed-disconnect slot={n} original_code={code} cancel_code={classified} in_flight={}",
                    self.ledger.in_flight()
                );
            }
            return Err(HttpError::Native(classified));
        }
        Ok(())
    }
    /// The response framing is supplied by trusted routing. Native memory contains
    /// only this fixed 64KiB piece; subsequent pieces wait for ResponseReady.
    // SAFETY for native calls in this function: Only the vacant exact-token slot is populated. All response fields, header tables, strings, chunk descriptor and payload stay in fixed allocations through its IOCP completion.
    pub fn respond(
        &mut self,
        t: RequestToken,
        head: ResponseHead,
        body: &[u8],
        more: bool,
    ) -> Result<(), HttpError> {
        let n = self.request(t)?;
        head.validate()?;
        if head.headers.len() + 2 > RESPONSE_HEADERS
            || body.len() > CHUNK_BYTES
            || body.len() as u64 > head.content_length
            || (!more && body.len() as u64 != head.content_length)
        {
            return Err(HttpError::InvalidResponse);
        }
        if self.slots[n].response_length.is_some() {
            return Err(HttpError::InvalidResponse);
        }
        self.ledger.begin(t, Operation::SendHead { last: !more })?;
        self.slots[n].response_length = Some(head.content_length);
        self.slots[n].response_sent = body.len() as u64;
        self.slots[n].deadline = Some(Instant::now() + REQUEST_TIMEOUT);
        if let Err(e) = self.prepare_response(n, head, body) {
            self.ledger.complete(n, self.ledger.generation, false)?;
            self.slots[n].response_length = None;
            return Err(e);
        }
        self.submitted();
        let flags = if more {
            HTTP_SEND_RESPONSE_FLAG_MORE_DATA
        } else {
            HTTP_SEND_RESPONSE_FLAG_DISCONNECT
        };
        let code = unsafe {
            HttpSendHttpResponse(
                self.resources.queue,
                self.slots[n].request_id,
                flags,
                self.slots[n].memory.response.get(),
                None,
                None,
                None,
                None,
                Some(self.slots[n].memory.operation.get()),
                None,
            )
        };
        if !self.accepted(code) {
            self.trace_rejected("send-head", n, code);
            self.ledger.complete(n, self.ledger.generation, false)?;
            self.cancel(t, code)?;
            return Err(HttpError::Native(code));
        }
        Ok(())
    }
    // SAFETY for native calls in this function: The previous send completion must have vacated this exact-token slot. Both chunk descriptor and payload stay fixed until the next completion.
    pub fn send_response_body(
        &mut self,
        t: RequestToken,
        body: &[u8],
        more: bool,
    ) -> Result<(), HttpError> {
        let n = self.request(t)?;
        let expected = self.slots[n]
            .response_length
            .ok_or(HttpError::InvalidResponse)?;
        let sent = self.slots[n]
            .response_sent
            .checked_add(body.len() as u64)
            .ok_or(HttpError::TooLarge)?;
        if body.len() > CHUNK_BYTES || sent > expected || (!more && sent != expected) {
            return Err(HttpError::InvalidResponse);
        }
        self.ledger.begin(t, Operation::SendBody { last: !more })?;
        self.prepare_chunk(n, body);
        self.slots[n].response_sent = sent;
        self.slots[n].deadline = Some(Instant::now() + REQUEST_TIMEOUT);
        self.submitted();
        let flags = if more {
            HTTP_SEND_RESPONSE_FLAG_MORE_DATA
        } else {
            HTTP_SEND_RESPONSE_FLAG_DISCONNECT
        };
        let chunk = unsafe {
            std::slice::from_raw_parts(
                self.slots[n].memory.chunk.get(),
                usize::from(!body.is_empty()),
            )
        };
        let code = unsafe {
            HttpSendResponseEntityBody(
                self.resources.queue,
                self.slots[n].request_id,
                flags,
                Some(chunk),
                None,
                None,
                None,
                Some(self.slots[n].memory.operation.get()),
                None,
            )
        };
        if !self.accepted(code) {
            self.trace_rejected("send-body", n, code);
            self.ledger.complete(n, self.ledger.generation, false)?;
            self.cancel(t, code)?;
            return Err(HttpError::Native(code));
        }
        Ok(())
    }
    fn prepare_chunk(&mut self, n: usize, body: &[u8]) {
        let m = &self.slots[n].memory;
        // SAFETY: ledger.begin proved no operation owns these response pointers.
        unsafe {
            *m.operation.get() = OVERLAPPED::default();
            (&mut *m.send.get())[..body.len()].copy_from_slice(body);
            *m.chunk.get() = HTTP_DATA_CHUNK {
                DataChunkType: HttpDataChunkFromMemory,
                Anonymous: HTTP_DATA_CHUNK_0 {
                    FromMemory: HTTP_DATA_CHUNK_0_0 {
                        pBuffer: m.send.get().cast(),
                        BufferLength: body.len() as u32,
                    },
                },
            };
        }
    }
    fn prepare_response(
        &mut self,
        n: usize,
        mut head: ResponseHead,
        body: &[u8],
    ) -> Result<(), HttpError> {
        // The port length is the wire body length. A 304 has no body, but
        // Content-Length: 0 would falsely describe the cached representation
        // (RFC 9110 §8.6). Omit this optional field for conditional responses.
        if head.status != 304 {
            head.headers.push(Header {
                name: "Content-Length".into(),
                value: head.content_length.to_string().into_bytes(),
            });
        }
        head.headers.push(Header {
            name: "Connection".into(),
            value: b"close".to_vec(),
        });
        validate_headers(&head.headers)?;
        self.prepare_chunk(n, body);
        let m = &self.slots[n].memory;
        let mut cursor = 0;
        // SAFETY: fixed arrays never move and no overlapping operation owns them.
        unsafe {
            *m.response.get() = HTTP_RESPONSE_V2::default();
            let response = &mut (*m.response.get()).Base;
            response.Version = HTTP_VERSION {
                MajorVersion: 1,
                MinorVersion: 1,
            };
            response.StatusCode = head.status;
            response.Headers.UnknownHeaderCount = 0;
            let known = [
                "cache-control",
                "connection",
                "date",
                "keep-alive",
                "pragma",
                "trailer",
                "transfer-encoding",
                "upgrade",
                "via",
                "warning",
                "allow",
                "content-length",
                "content-type",
                "content-encoding",
                "content-language",
                "content-location",
                "content-md5",
                "content-range",
                "expires",
                "last-modified",
                "accept-ranges",
                "age",
                "etag",
                "location",
                "proxy-authenticate",
                "retry-after",
                "server",
                "set-cookie",
                "vary",
                "www-authenticate",
            ];
            let mut seen = [false; 30];
            response.Headers.pUnknownHeaders = (*m.headers.get()).as_mut_ptr();
            for h in &head.headers {
                let start = cursor;
                let end = start + h.name.len();
                (&mut *m.header_bytes.get())[start..end].copy_from_slice(h.name.as_bytes());
                cursor = end;
                let value = cursor;
                let end = value + h.value.len();
                if end > MAX_HEADER_BYTES {
                    return Err(HttpError::TooLarge);
                }
                (&mut *m.header_bytes.get())[value..end].copy_from_slice(&h.value);
                cursor = end;
                let name = PCSTR((*m.header_bytes.get()).as_ptr().add(start));
                let value_ptr = PCSTR((*m.header_bytes.get()).as_ptr().add(value));
                if let Some(index) = known
                    .iter()
                    .position(|name| h.name.eq_ignore_ascii_case(name))
                {
                    if seen[index] {
                        return Err(HttpError::InvalidResponse);
                    }
                    seen[index] = true;
                    response.Headers.KnownHeaders[index] = HTTP_KNOWN_HEADER {
                        RawValueLength: h.value.len() as u16,
                        pRawValue: value_ptr,
                    };
                } else {
                    let index = usize::from(response.Headers.UnknownHeaderCount);
                    (*m.headers.get())[index] = HTTP_UNKNOWN_HEADER {
                        NameLength: h.name.len() as u16,
                        RawValueLength: h.value.len() as u16,
                        pName: name,
                        pRawValue: value_ptr,
                    };
                    response.Headers.UnknownHeaderCount += 1;
                }
            }
            response.EntityChunkCount = u16::from(!body.is_empty());
            response.pEntityChunks = if body.is_empty() {
                ptr::null_mut()
            } else {
                m.chunk.get()
            };
        }
        Ok(())
    }
    // SAFETY for native calls in this function: Cancellation has a separate fixed OVERLAPPED and independent ledger entry; it cannot overwrite or release any original receive/send storage.
    pub fn cancel(&mut self, t: RequestToken, error: u32) -> Result<(), HttpError> {
        self.cancel_with_witness(t, error).map(|_| ())
    }
    // SAFETY for native calls: the exact token is checked before capturing its
    // RequestId. A distinct registered cancel record remains fixed through IOCP
    // when accepted. Only synchronous rejection withdraws that cancel record.
    fn cancel_with_witness(
        &mut self,
        t: RequestToken,
        error: u32,
    ) -> Result<CancelWitness, HttpError> {
        self.ledger.check(t)?;
        let n = t.slot();
        let request_id = self.slots[n].request_id;
        self.slots[n].canceled = true;
        self.slots[n].terminal_error = error;
        self.slots[n].deadline = None;
        if request_id == 0 {
            return Ok(CancelWitness::Unconfirmed);
        }
        if !self.ledger.begin_cancel(t)? {
            return Ok(CancelWitness::Unconfirmed);
        }
        unsafe {
            *self.slots[n].memory.cancel.get() = OVERLAPPED::default();
        }
        self.submitted();
        self.metrics.canceled += 1;
        let code = unsafe {
            HttpCancelHttpRequest(
                self.resources.queue,
                request_id,
                Some(self.slots[n].memory.cancel.get()),
            )
        };
        if !self.accepted(code) {
            self.trace_rejected("cancel-request", n, code);
            self.ledger.complete(n, self.ledger.generation, true)?;
            if !request_disconnected(code) {
                return Err(HttpError::Native(code));
            }
            return Ok(CancelWitness::Disconnected { request_id, code });
        }
        Ok(CancelWitness::Unconfirmed)
    }
    // SAFETY for native calls in this function: URL strings live for synchronous removal. Queue shutdown does not release slots; all original and cancellation OVERLAPPED records remain owned while the ledger drains.
    pub fn request_stop(&mut self) -> Result<(), HttpError> {
        if self.ledger.state != HttpState::Running {
            return Ok(());
        }
        self.ledger.state = HttpState::Quiescing;
        self.stopping = Some(Instant::now());
        // First detach URL routing; do not remove any URLACL/certificate config.
        let prefix: Vec<u16> = self.prefix.as_str().encode_utf16().chain(Some(0)).collect();
        let removed =
            unsafe { HttpRemoveUrlFromUrlGroup(self.resources.group, PCWSTR(prefix.as_ptr()), 0) };
        self.ledger.state = HttpState::Canceling;
        let shutdown = unsafe { HttpShutdownRequestQueue(self.resources.queue) };
        let tokens: Vec<_> = (0..MAX_REQUESTS)
            .filter_map(|n| self.ledger.token(n).ok())
            .collect();
        for t in tokens {
            self.cancel(t, ERROR_OPERATION_ABORTED.0)?;
        }
        self.ledger.state = HttpState::Draining;
        status(removed)?;
        status(shutdown)
    }
    /// Bounded IOCP service; no callback is invoked and no business I/O occurs.
    pub fn poll(&mut self, wait_ms: u32) -> Result<Vec<HttpEvent>, HttpError> {
        if self.ledger.state == HttpState::Retained {
            return Err(HttpError::Retained);
        }
        if self.ledger.state == HttpState::Closed {
            return Ok(Vec::new());
        }
        let result = self.poll_inner(wait_ms.min(50));
        if let Err(error) = &result {
            // Fixed internal fields only: never include request headers, URL,
            // body, or credentials in native lifecycle diagnostics.
            eprintln!(
                "HTTP_IOCP_FAILURE stage=poll error={error:?} state={:?} in_flight={} completions={}",
                self.ledger.state,
                self.ledger.in_flight(),
                self.metrics.completions
            );
        }
        if result.is_err() && self.ledger.state == HttpState::Running {
            let _ = self.request_stop();
        }
        result
    }
    // SAFETY for native calls in this function: IOCP returns only opaque addresses; they are matched against our exact fixed records before access. Null-pointer failure is distinguished from a failed completed operation. Only matched completions vacate ownership.
    fn poll_inner(&mut self, wait_ms: u32) -> Result<Vec<HttpEvent>, HttpError> {
        let mut events = Vec::with_capacity(MAX_POLL_EVENTS);
        let timed: Vec<_> = (0..MAX_REQUESTS)
            .filter(|n| self.slots[*n].deadline.is_some_and(|d| Instant::now() >= d))
            .filter_map(|n| self.ledger.token(n).ok())
            .collect();
        for t in timed {
            self.cancel(t, ERROR_TIMEOUT.0)?;
        }
        for batch in 0..MAX_REQUESTS {
            let (mut bytes, mut key, mut overlapped) = (0, 0usize, ptr::null_mut());
            let result = unsafe {
                GetQueuedCompletionStatus(
                    self.resources.iocp,
                    &mut bytes,
                    &mut key,
                    &mut overlapped,
                    if batch == 0 { wait_ms } else { 0 },
                )
            };
            let hresult = result
                .as_ref()
                .err()
                .map_or(0, |error| error.code().0 as u32);
            let code = win32_hresult_code(hresult);
            if overlapped.is_null() {
                if result.is_err() && code != WAIT_TIMEOUT.0 {
                    eprintln!(
                        "HTTP_IOCP_FAILURE stage=completion-wait code={code} hresult=0x{hresult:08x} in_flight={}",
                        self.ledger.in_flight()
                    );
                    return Err(HttpError::Native(code));
                }
                break;
            }
            let (n, cancel) = (0..MAX_REQUESTS)
                .find_map(|n| {
                    let m = &self.slots[n].memory;
                    if m.operation.get() == overlapped {
                        Some((n, false))
                    } else if m.cancel.get() == overlapped {
                        Some((n, true))
                    } else {
                        None
                    }
                })
                .ok_or(HttpError::Invariant)?;
            let operation = self.ledger.complete(n, key as u64, cancel)?;
            self.metrics.completions += 1;
            if let Some(op) = operation {
                self.completed(n, op, code, bytes, &mut events)
                    .inspect_err(|error| {
                        eprintln!(
                            "HTTP_IOCP_FAILURE stage=operation-completion operation={op:?} slot={n} code={code} hresult=0x{hresult:08x} error={error:?}"
                        );
                    })?;
            }
            self.collect(n, &mut events)?;
        }
        // Synchronous EOF is not an accepted operation and therefore has no IOCP
        // packet; publish it once from this same bounded completion domain.
        for n in 0..MAX_REQUESTS {
            if self.slots[n].body_eof
                && !self.slots[n].canceled
                && self.slots[n].body_limit.is_some()
                && self.ledger.operation(n).is_none()
            {
                if let Ok(token) = self.ledger.token(n) {
                    events.push(HttpEvent::BodyChunk {
                        token,
                        bytes: Vec::new(),
                        eof: true,
                    });
                    self.slots[n].body_limit = None;
                }
            }
        }
        // A synchronously rejected read followed by a synchronously rejected
        // cancel has NO future IOCP packet. Collect such canceled, vacant slots
        // on every poll, including Running. collect still requires BOTH native
        // records to be vacant before returning storage or emitting Closed.
        for n in 0..MAX_REQUESTS {
            self.collect(n, &mut events)?;
        }
        if self.ledger.state == HttpState::Running {
            self.replenish()?;
        } else {
            if self.ledger.active() == 0 && self.ledger.in_flight() == 0 {
                self.resources.close()?;
                self.ledger.state = HttpState::Closed;
                ACTIVE.store(false, Ordering::Release);
            } else if self
                .stopping
                .is_some_and(|s| s.elapsed() >= REQUEST_TIMEOUT)
            {
                self.ledger.state = HttpState::Retained;
                POISONED.store(true, Ordering::Release);
                return Err(HttpError::Retained);
            }
        }
        if events.len() > MAX_POLL_EVENTS {
            return Err(HttpError::Invariant);
        }
        Ok(events)
    }
    // SAFETY for native calls in this function: The ledger has consumed exactly this original operation before any native result buffer is read; all slot allocations remain live, and cancellation is tracked independently.
    fn completed(
        &mut self,
        n: usize,
        op: Operation,
        code: u32,
        bytes: u32,
        events: &mut Vec<HttpEvent>,
    ) -> Result<(), HttpError> {
        let t = self.ledger.token(n)?;
        if self.ledger.state != HttpState::Running || self.slots[n].canceled {
            return Ok(());
        }
        if code != 0 && !(op == Operation::Body && code == ERROR_HANDLE_EOF.0) {
            // MORE_DATA means the fixed, bounded header was insufficient. Do not
            // resize/retry while native pointers might still belong to the OS.
            if op == Operation::Head {
                self.slots[n].request_id =
                    unsafe { (*self.slots[n].memory.request()).Base.RequestId };
            }
            self.cancel(t, code)?;
            return Ok(());
        }
        match op {
            Operation::Head => {
                self.slots[n].request_id =
                    unsafe { (*self.slots[n].memory.request()).Base.RequestId };
                if self.slots[n].request_id == 0 {
                    return Err(HttpError::Invariant);
                }
                let head = match Self::parse_head(&self.slots[n].memory, &self.prefix, t) {
                    Ok(head) => head,
                    Err(_) => {
                        self.cancel(t, ERROR_INVALID_DATA.0)?;
                        return Ok(());
                    }
                };
                self.slots[n].deadline = Some(Instant::now() + REQUEST_TIMEOUT);
                self.slots[n].body_eof = !head.has_body;
                events.push(HttpEvent::Request(head));
            }
            Operation::Body => {
                if code == ERROR_HANDLE_EOF.0 {
                    self.slots[n].body_eof = true;
                    self.slots[n].body_limit = None;
                    events.push(HttpEvent::BodyChunk {
                        token: t,
                        bytes: Vec::new(),
                        eof: true,
                    });
                } else {
                    let count = bytes as usize;
                    if count > CHUNK_BYTES {
                        return Err(HttpError::Invariant);
                    }
                    let s = &mut self.slots[n];
                    s.body_read = s
                        .body_read
                        .checked_add(count as u64)
                        .ok_or(HttpError::TooLarge)?;
                    if s.body_read > s.body_limit.ok_or(HttpError::Invariant)? {
                        self.cancel(t, ERROR_FILE_TOO_LARGE.0)?;
                    } else {
                        let body = unsafe { (&*s.memory.body.get())[..count].to_vec() };
                        events.push(HttpEvent::BodyChunk {
                            token: t,
                            bytes: body,
                            eof: false,
                        });
                    }
                }
            }
            Operation::SendHead { last } | Operation::SendBody { last } => {
                if last {
                    events.push(HttpEvent::ResponseComplete(t));
                    self.ledger.release(t)?;
                    self.slots[n].reset();
                } else {
                    events.push(HttpEvent::ResponseReady(t));
                }
            }
        }
        Ok(())
    }
    fn collect(&mut self, n: usize, events: &mut Vec<HttpEvent>) -> Result<(), HttpError> {
        if self.ledger.operation(n).is_none() && !self.ledger.cancel_pending(n) {
            if let Ok(t) = self.ledger.token(n) {
                if self.slots[n].canceled || self.ledger.state != HttpState::Running {
                    events.push(HttpEvent::Closed {
                        token: t,
                        error: self.slots[n].terminal_error,
                    });
                    self.ledger.release(t)?;
                    self.slots[n].reset();
                }
            }
        }
        Ok(())
    }
    fn parse_head(
        memory: &NativeMemory,
        prefix: &ListenPrefix,
        token: RequestToken,
    ) -> Result<RequestHead, HttpError> {
        let base = memory.request.get().cast::<u8>() as usize;
        let end = base + HEADER_BUFFER_BYTES;
        let bounded = |p: *const u8, len: usize| -> Result<&[u8], HttpError> {
            let start = p as usize;
            if len == 0 {
                return Ok(&[]);
            }
            if start < base || start.checked_add(len).is_none_or(|to| to > end) {
                return Err(HttpError::Invariant);
            }
            Ok(unsafe { std::slice::from_raw_parts(p, len) })
        };
        // SAFETY: successful receive completed and the allocation meets HTTP_REQUEST
        // alignment. Internal pointers are range/alignment checked before reads.
        let r = unsafe { &(*memory.request()).Base };
        if r.EntityChunkCount != 0 || r.Headers.TrailerCount != 0 {
            return Err(HttpError::InvalidRequest);
        }
        let tls = !r.pSslInfo.is_null();
        if tls {
            bounded(r.pSslInfo.cast(), size_of::<HTTP_SSL_INFO>())?;
        }
        if !prefix.is_diagnostic() && !tls {
            return Err(HttpError::InvalidRequest);
        }
        let target = bounded(r.pRawUrl.0, usize::from(r.RawUrlLength))?;
        if target.len() > MAX_TARGET_BYTES || target.iter().any(|b| *b < 33 || *b == 127) {
            return Err(HttpError::InvalidRequest);
        }
        let target = std::str::from_utf8(target)
            .map_err(|_| HttpError::InvalidRequest)?
            .to_owned();
        let method = match r.Verb.0 {
            4 => "GET",
            5 => "HEAD",
            6 => "POST",
            7 => "PUT",
            8 => "DELETE",
            3 => "OPTIONS",
            1 => {
                let bytes = bounded(r.pUnknownVerb.0, usize::from(r.UnknownVerbLength))?;
                if bytes.len() > 16 || !bytes.iter().all(u8::is_ascii_uppercase) {
                    return Err(HttpError::InvalidRequest);
                }
                std::str::from_utf8(bytes).map_err(|_| HttpError::InvalidRequest)?
            }
            _ => return Err(HttpError::InvalidRequest),
        }
        .to_owned();
        let names = [
            "cache-control",
            "connection",
            "date",
            "keep-alive",
            "pragma",
            "trailer",
            "transfer-encoding",
            "upgrade",
            "via",
            "warning",
            "allow",
            "content-length",
            "content-type",
            "content-encoding",
            "content-language",
            "content-location",
            "content-md5",
            "content-range",
            "expires",
            "last-modified",
            "accept",
            "accept-charset",
            "accept-encoding",
            "accept-language",
            "authorization",
            "cookie",
            "expect",
            "from",
            "host",
            "if-match",
            "if-modified-since",
            "if-none-match",
            "if-range",
            "if-unmodified-since",
            "max-forwards",
            "proxy-authorization",
            "referer",
            "range",
            "te",
            "translate",
            "user-agent",
        ];
        let mut headers = Vec::new();
        let mut header_bytes = 0usize;
        let mut append = |name: &str, value: &[u8]| -> Result<(), HttpError> {
            if headers.len() >= MAX_HEADERS {
                return Err(HttpError::TooLarge);
            }
            header_bytes = header_bytes
                .checked_add(name.len())
                .and_then(|n| n.checked_add(value.len() + 4))
                .ok_or(HttpError::TooLarge)?;
            if header_bytes > MAX_HEADER_BYTES {
                return Err(HttpError::TooLarge);
            }
            let h = Header {
                name: name.to_ascii_lowercase(),
                value: value.to_vec(),
            };
            h.validate()?;
            headers.push(h);
            Ok(())
        };
        for (index, h) in r.Headers.KnownHeaders.iter().enumerate() {
            if h.RawValueLength != 0 {
                append(
                    names[index],
                    bounded(h.pRawValue.0, usize::from(h.RawValueLength))?,
                )?;
            }
        }
        let count = usize::from(r.Headers.UnknownHeaderCount);
        if count > MAX_HEADERS {
            return Err(HttpError::TooLarge);
        }
        let p = r.Headers.pUnknownHeaders;
        if count > 0 {
            bounded(p.cast(), count * size_of::<HTTP_UNKNOWN_HEADER>())?;
            if !(p as usize).is_multiple_of(align_of::<HTTP_UNKNOWN_HEADER>()) {
                return Err(HttpError::Invariant);
            }
            for h in unsafe { std::slice::from_raw_parts(p, count) } {
                append(
                    std::str::from_utf8(bounded(h.pName.0, usize::from(h.NameLength))?)
                        .map_err(|_| HttpError::InvalidRequest)?,
                    bounded(h.pRawValue.0, usize::from(h.RawValueLength))?,
                )?;
            }
        }
        validate_headers(&headers)?;
        let lengths: Vec<_> = headers
            .iter()
            .filter(|h| h.name == "content-length")
            .collect();
        if lengths.len() > 1 {
            return Err(HttpError::InvalidRequest);
        }
        let content_length = lengths
            .first()
            .map(|h| {
                std::str::from_utf8(&h.value)
                    .ok()
                    .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or(HttpError::InvalidRequest)
            })
            .transpose()?;
        let chunked = headers.iter().any(|h| h.name == "transfer-encoding");
        if chunked && content_length.is_some() {
            return Err(HttpError::InvalidRequest);
        }
        let has_body = r.Flags & HTTP_REQUEST_FLAG_MORE_ENTITY_BODY_EXISTS != 0
            || content_length.is_some_and(|n| n > 0)
            || chunked;
        Ok(RequestHead {
            token,
            method,
            target,
            headers,
            tls,
            content_length,
            has_body,
        })
    }
}
impl Drop for HttpHost {
    fn drop(&mut self) {
        if self.ledger.state == HttpState::Closed {
            return;
        }
        let _ = self.request_stop();
        // Drop never blocks UI/shutdown for the drain budget. The execution owner
        // normally calls poll until Closed. If it did not, retain every pointer
        // and queue handle and forbid replacement listeners until process exit.
        if self.ledger.in_flight() != 0 {
            POISONED.store(true, Ordering::Release);
            for slot in &mut self.slots {
                slot.retained = true;
            }
            self.resources.queue = HANDLE::default();
            self.resources.iocp = HANDLE::default();
            self.resources.group = 0;
            self.resources.session = 0;
            self.resources.initialized = false;
        } else if self.resources.close().is_ok() {
            ACTIVE.store(false, Ordering::Release);
        } else {
            POISONED.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_function_requires_exact_synchronously_confirmed_disconnection() {
        let exact = CancelWitness::Disconnected {
            request_id: 42,
            code: ERROR_CONNECTION_INVALID.0,
        };
        assert_eq!(
            unaccepted_body_error(ERROR_INVALID_FUNCTION.0, 42, exact),
            ERROR_CONNECTION_INVALID.0
        );
        // No proof, a different request, or a zero request identifier cannot
        // convert INVALID_FUNCTION into an expected peer disconnect.
        for (request_id, witness) in [
            (42, CancelWitness::Unconfirmed),
            (43, exact),
            (0, exact),
            (
                42,
                CancelWitness::Disconnected {
                    request_id: 42,
                    code: ERROR_INVALID_FUNCTION.0,
                },
            ),
        ] {
            assert_eq!(
                unaccepted_body_error(ERROR_INVALID_FUNCTION.0, request_id, witness),
                ERROR_INVALID_FUNCTION.0
            );
        }
        // Unrelated API failures remain visible even when cancellation happens
        // to discover that the peer is also gone. Success/PENDING is unchanged.
        for code in [ERROR_INVALID_HANDLE.0, ERROR_INVALID_PARAMETER.0, 0, 997] {
            assert_eq!(unaccepted_body_error(code, 42, exact), code);
        }
    }
    #[test]
    fn rejected_body_and_rejected_cancel_vacate_only_their_own_records() {
        let mut ledger = Ledger::new().unwrap();
        let body = ledger.reserve().unwrap();
        let unrelated = ledger.reserve().unwrap();
        ledger.begin(body, Operation::Body).unwrap();
        ledger.begin(unrelated, Operation::Head).unwrap();
        assert!(!submission_accepted(ERROR_INVALID_FUNCTION.0));
        assert_eq!(
            ledger
                .complete(body.slot(), ledger.generation, false)
                .unwrap(),
            Some(Operation::Body)
        );
        assert!(ledger.begin_cancel(body).unwrap());
        assert_eq!(ledger.release(body), Err(HttpError::Busy));
        assert_eq!(
            unaccepted_body_error(ERROR_INVALID_FUNCTION.0, 42, CancelWitness::Unconfirmed),
            ERROR_INVALID_FUNCTION.0
        );
        assert!(!submission_accepted(ERROR_CONNECTION_INVALID.0));
        ledger
            .complete(body.slot(), ledger.generation, true)
            .unwrap();
        let witness = CancelWitness::Disconnected {
            request_id: 42,
            code: ERROR_CONNECTION_INVALID.0,
        };
        assert_eq!(
            unaccepted_body_error(ERROR_INVALID_FUNCTION.0, 42, witness),
            ERROR_CONNECTION_INVALID.0
        );
        ledger.release(body).unwrap();
        // Rejected operations have no IOCP to await. The independently accepted
        // head is still owned and cannot be freed by this request's disconnect.
        assert_eq!(ledger.in_flight(), 1);
        assert_eq!(ledger.operation(unrelated.slot()), Some(Operation::Head));
        assert_eq!(ledger.check(body), Err(HttpError::StaleRequest));
        ledger
            .complete(unrelated.slot(), ledger.generation, false)
            .unwrap();
        ledger.release(unrelated).unwrap();
        assert_eq!(ledger.in_flight(), 0);
    }
    #[test]
    fn captured_completion_hresult_is_decoded_without_thread_last_error() {
        assert_eq!(win32_hresult_code(0), 0);
        assert_eq!(win32_hresult_code(0x8007_0102), WAIT_TIMEOUT.0);
        assert_eq!(win32_hresult_code(0x8007_03e3), ERROR_OPERATION_ABORTED.0);
        assert_eq!(win32_hresult_code(0x8007_0040), ERROR_NETNAME_DELETED.0);
        assert_eq!(win32_hresult_code(0x8007_00ea), ERROR_MORE_DATA.0);
        assert_eq!(win32_hresult_code(0x8000_4005), 0x8000_4005);
        let captured =
            windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_0102u32 as i32));
        // SAFETY: this test only changes its own thread's error slot. The
        // operation result above already owns the original timeout HRESULT.
        unsafe { SetLastError(ERROR_INVALID_FUNCTION) };
        assert_eq!(win32_hresult_code(captured.code().0 as u32), WAIT_TIMEOUT.0);
    }
    fn input() -> (Box<NativeMemory>, ListenPrefix, RequestToken) {
        let memory = NativeMemory::new();
        let prefix = ListenPrefix::isolated_loopback(54321, "FutureClassroom-Test-bounds").unwrap();
        let mut issuer = TokenIssuer::new().unwrap();
        let token = issuer.issue(0).unwrap();
        // SAFETY: this synthetic allocation is not submitted to any native API.
        unsafe {
            let base = memory.request.get().cast::<u8>();
            let target = b"/FutureClassroom-Test-bounds/health";
            ptr::copy_nonoverlapping(target.as_ptr(), base.add(4096), target.len());
            let r = &mut (*memory.request()).Base;
            r.RequestId = 1;
            r.Verb = HttpVerbGET;
            r.RawUrlLength = target.len() as u16;
            r.pRawUrl = PCSTR(base.add(4096));
            let host = b"127.0.0.1:54321";
            ptr::copy_nonoverlapping(host.as_ptr(), base.add(5000), host.len());
            r.Headers.KnownHeaders[28] = HTTP_KNOWN_HEADER {
                RawValueLength: host.len() as u16,
                pRawValue: PCSTR(base.add(5000)),
            };
        }
        (memory, prefix, token)
    }
    #[test]
    fn fixed_header_is_aligned_and_pointer_tables_cannot_escape_allocation() {
        let (m, p, t) = input();
        assert!((m.request() as usize).is_multiple_of(align_of::<HTTP_REQUEST_V2>()));
        let head = HttpHost::parse_head(&m, &p, t).unwrap();
        assert_eq!(head.method, "GET");
        assert!(!head.tls);
        unsafe {
            (*m.request()).Base.Headers.UnknownHeaderCount = 1;
            (*m.request()).Base.Headers.pUnknownHeaders =
                ptr::dangling_mut::<HTTP_UNKNOWN_HEADER>();
        }
        assert_eq!(HttpHost::parse_head(&m, &p, t), Err(HttpError::Invariant));
    }
    #[test]
    fn raw_target_and_ssl_pointer_are_range_checked_before_use() {
        let (m, p, t) = input();
        unsafe {
            (*m.request()).Base.pRawUrl = PCSTR(ptr::dangling());
        }
        assert_eq!(HttpHost::parse_head(&m, &p, t), Err(HttpError::Invariant));
        let (m, _, t) = input();
        let p = ListenPrefix::https("https://school.example:443/classroom/").unwrap();
        assert_eq!(
            HttpHost::parse_head(&m, &p, t),
            Err(HttpError::InvalidRequest)
        );
        unsafe {
            (*m.request()).Base.pSslInfo = ptr::dangling_mut();
        }
        assert_eq!(HttpHost::parse_head(&m, &p, t), Err(HttpError::Invariant));
    }
    #[test]
    fn native_slack_does_not_expand_wire_header_budget() {
        let (m, p, t) = input();
        unsafe {
            let base = m.request.get().cast::<u8>();
            ptr::write_bytes(base.add(8192), b'Z', MAX_HEADER_BYTES);
            ptr::copy_nonoverlapping(b"x-filler".as_ptr(), base.add(7000), 8);
            let headers = base.add(6000).cast::<HTTP_UNKNOWN_HEADER>();
            ptr::write(
                headers,
                HTTP_UNKNOWN_HEADER {
                    NameLength: 8,
                    RawValueLength: MAX_HEADER_BYTES as u16,
                    pName: PCSTR(base.add(7000)),
                    pRawValue: PCSTR(base.add(8192)),
                },
            );
            (*m.request()).Base.Headers.UnknownHeaderCount = 1;
            (*m.request()).Base.Headers.pUnknownHeaders = headers;
        }
        assert_eq!(HttpHost::parse_head(&m, &p, t), Err(HttpError::TooLarge));
    }
}
