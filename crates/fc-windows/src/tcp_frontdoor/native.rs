use super::{Deadline, HeaderBuffer, CONNECTION_SLOTS, FIRST_HEADER_BUDGET};
use std::{
    cell::UnsafeCell,
    io::{Read, Write},
    marker::PhantomPinned,
    mem::{size_of, ManuallyDrop},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
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

// One physical owner per process. A retained native fault permanently prevents
// replacement, so repeated failures cannot accumulate arbitrary allocations.
static OWNER_ACTIVE: AtomicBool = AtomicBool::new(false);
static OWNER_FAULTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Default)]
pub struct FrontdoorProbeReport {
    pub cycles: usize,
    pub accept_submissions: usize,
    pub accept_completions: usize,
    pub canceled_accepts: usize,
    pub accepted: usize,
    pub headers: usize,
    pub deadline_closes: usize,
    pub header_limit_closes: usize,
    pub capacity_listener_closes: usize,
    pub connect_age_seconds: Vec<u32>,
    pub wire_close_nanos: Vec<u128>,
    pub delayed_poll_millis: u128,
    pub fixed_owner_bytes: usize,
    pub diagnostic_vector_payload_capacity_bytes: usize,
    pub cleanup_complete: bool,
}

// Native APIs can write only these two UnsafeCell allocations while in flight.
// No Rust references into them are created until their matching completion.
// The surrounding allocation is pinned before posting and never moves/reuses.
struct AcceptMemory {
    operation: UnsafeCell<OVERLAPPED>,
    addresses: UnsafeCell<[u8; 64]>,
    _pinned: PhantomPinned,
}
struct Slot {
    memory: Pin<Box<AcceptMemory>>,
    socket: SOCKET,
    pending: bool,
    accepted: bool,
    deadline: Deadline,
    header: HeaderBuffer,
}
struct Owner {
    slots: ManuallyDrop<[Slot; CONNECTION_SLOTS]>,
    listener: SOCKET,
    port: HANDLE,
    address: SocketAddr,
    started_winsock: bool,
    stopping: bool,
    report: FrontdoorProbeReport,
}
fn error(stage: &str) -> String {
    // SAFETY: this is the current thread's Winsock error, read immediately.
    format!("{stage}: Winsock {}", unsafe { WSAGetLastError().0 })
}
fn check(value: i32, stage: &str) -> Result<(), String> {
    if value == 0 {
        Ok(())
    } else {
        Err(error(stage))
    }
}
fn socket_handle(socket: SOCKET) -> HANDLE {
    HANDLE(socket.0 as *mut _)
}
fn abortive_close(socket: &mut SOCKET) -> Result<(), String> {
    if *socket == INVALID_SOCKET {
        return Ok(());
    }
    // SAFETY: single owner thread; no concurrent call uses this descriptor.
    // Linger(1,0) requests reset without waiting for peer acknowledgement.
    unsafe {
        let value = [1u8, 0, 0, 0];
        // Closing still proceeds if the provider refuses the optional linger.
        let _ = setsockopt(*socket, SOL_SOCKET, SO_LINGER, Some(&value));
        check(closesocket(*socket), "closesocket")?;
    }
    *socket = INVALID_SOCKET;
    Ok(())
}
impl Owner {
    fn new() -> Result<Self, String> {
        if OWNER_FAULTED.load(Ordering::Acquire)
            || OWNER_ACTIVE
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err("frontdoor owner busy or permanently faulted".into());
        }
        let now = Instant::now();
        let mut owner = Self {
            slots: ManuallyDrop::new(std::array::from_fn(|_| Slot {
                memory: Box::pin(AcceptMemory {
                    operation: UnsafeCell::new(OVERLAPPED::default()),
                    addresses: UnsafeCell::new([0; 64]),
                    _pinned: PhantomPinned,
                }),
                socket: INVALID_SOCKET,
                pending: false,
                accepted: false,
                deadline: Deadline::posted(now),
                header: HeaderBuffer::new(),
            })),
            listener: INVALID_SOCKET,
            port: HANDLE::default(),
            address: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
            started_winsock: false,
            stopping: false,
            report: FrontdoorProbeReport {
                connect_age_seconds: Vec::with_capacity(CONNECTION_SLOTS),
                wire_close_nanos: Vec::with_capacity(CONNECTION_SLOTS),
                fixed_owner_bytes: size_of::<Self>() + CONNECTION_SLOTS * size_of::<AcceptMemory>(),
                diagnostic_vector_payload_capacity_bytes: CONNECTION_SLOTS
                    * (size_of::<u32>() + size_of::<u128>()),
                ..Default::default()
            },
        };
        owner.start()?;
        Ok(owner)
    }
    fn start(&mut self) -> Result<(), String> {
        // SAFETY: all addresses point to live local/owned, adequately sized data.
        // No handles escape this owner and no global Winsock configuration changes.
        unsafe {
            let mut data = WSADATA::default();
            let code = WSAStartup(0x202, &mut data);
            if code != 0 {
                return Err(format!("WSAStartup: {code}"));
            }
            self.started_winsock = true;
            self.listener = WSASocketW(
                AF_INET.0 as i32,
                SOCK_STREAM.0,
                IPPROTO_TCP.0,
                None,
                0,
                WSA_FLAG_OVERLAPPED,
            )
            .map_err(|e| format!("listener socket: {e}"))?;
            check(
                setsockopt(
                    self.listener,
                    SOL_SOCKET,
                    SO_EXCLUSIVEADDRUSE,
                    Some(&1i32.to_ne_bytes()),
                ),
                "exclusive",
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
                    self.listener,
                    ptr::from_ref(&address).cast(),
                    size_of::<SOCKADDR_IN>() as i32,
                ),
                "bind",
            )?;
            check(listen(self.listener, CONNECTION_SLOTS as i32), "listen")?;
            let mut length = size_of::<SOCKADDR_IN>() as i32;
            check(
                getsockname(
                    self.listener,
                    ptr::from_mut(&mut address).cast(),
                    &mut length,
                ),
                "getsockname",
            )?;
            self.address.set_port(u16::from_be(address.sin_port));
            self.port = CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1)
                .map_err(|e| format!("IOCP: {e}"))?;
            CreateIoCompletionPort(socket_handle(self.listener), Some(self.port), 1, 1)
                .map_err(|e| format!("associate: {e}"))?;
            let mut accept: LPFN_ACCEPTEX = None;
            let mut bytes = 0;
            check(
                WSAIoctl(
                    self.listener,
                    SIO_GET_EXTENSION_FUNCTION_POINTER,
                    Some(ptr::from_ref(&WSAID_ACCEPTEX).cast()),
                    size_of::<windows::core::GUID>() as u32,
                    Some(ptr::from_mut(&mut accept).cast()),
                    size_of::<LPFN_ACCEPTEX>() as u32,
                    &mut bytes,
                    None,
                    None,
                ),
                "get AcceptEx",
            )?;
            let accept = accept.ok_or("missing AcceptEx function")?;
            for slot in self.slots.iter_mut() {
                slot.socket = WSASocketW(
                    AF_INET.0 as i32,
                    SOCK_STREAM.0,
                    IPPROTO_TCP.0,
                    None,
                    0,
                    WSA_FLAG_OVERLAPPED,
                )
                .map_err(|e| format!("accept socket: {e}"))?;
                slot.deadline = Deadline::posted(Instant::now());
                let completed = accept(
                    self.listener,
                    slot.socket,
                    slot.memory.addresses.get().cast(),
                    0,
                    32,
                    32,
                    &mut bytes,
                    slot.memory.operation.get(),
                )
                .as_bool();
                if !completed && WSAGetLastError() != WSA_IO_PENDING {
                    return Err(error("AcceptEx"));
                }
                // Synchronous success also queues IOCP (no skip-on-success flag).
                slot.pending = true;
                self.report.accept_submissions += 1;
            }
        }
        Ok(())
    }
    fn pending(&self) -> usize {
        self.slots.iter().filter(|s| s.pending).count()
    }
    fn connected(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.accepted && s.socket != INVALID_SOCKET)
            .count()
    }
    fn stop(&mut self) -> Result<(), String> {
        self.stopping = true;
        // SAFETY: exact pinned OVERLAPPED belongs to this listener. Cancellation
        // is merely requested; pending stays true until its IOCP packet arrives.
        if self.listener != INVALID_SOCKET {
            for slot in self.slots.iter() {
                if slot.pending {
                    unsafe {
                        let _ = CancelIoEx(
                            socket_handle(self.listener),
                            Some(slot.memory.operation.get()),
                        );
                    }
                }
            }
        }
        let mut failure = None;
        for slot in self.slots.iter_mut() {
            if let Err(e) = abortive_close(&mut slot.socket) {
                failure = Some(e);
            }
        }
        if let Err(e) = abortive_close(&mut self.listener) {
            failure = Some(e);
        }
        failure.map_or(Ok(()), Err)
    }
    fn poll(&mut self) -> Result<(), String> {
        // Drain at most four packets per poll. No allocation or worker per event.
        for _ in 0..CONNECTION_SLOTS {
            let (mut bytes, mut key, mut operation) = (0, 0, ptr::null_mut());
            // SAFETY: output variables live through the call; IOCP stays open
            // until every owned overlapped packet has actually been dequeued.
            let result = unsafe {
                GetQueuedCompletionStatus(self.port, &mut bytes, &mut key, &mut operation, 0)
            };
            if operation.is_null() {
                if let Err(e) = result {
                    if e.code().0 as u32 & 0xffff != WAIT_TIMEOUT.0 {
                        return Err(format!("IOCP poll: {e}"));
                    }
                }
                break;
            }
            let slot = self
                .slots
                .iter_mut()
                .find(|s| s.memory.operation.get() == operation)
                .ok_or("foreign completion pointer")?;
            if key != 1 || !slot.pending {
                return Err("unexpected completion ownership".into());
            }
            slot.pending = false;
            self.report.accept_completions += 1;
            if result.is_err() || self.stopping {
                self.report.canceled_accepts += 1;
                abortive_close(&mut slot.socket)?;
                continue;
            }
            // SAFETY: AcceptEx has actually completed. Update context before
            // getsockopt/ioctlsocket. No overlapped receive is posted afterwards.
            unsafe {
                check(
                    setsockopt(
                        slot.socket,
                        SOL_SOCKET,
                        SO_UPDATE_ACCEPT_CONTEXT,
                        Some(&self.listener.0.to_ne_bytes()),
                    ),
                    "accept context",
                )?;
                let (mut age, mut length) = (0u32, size_of::<u32>() as i32);
                check(
                    getsockopt(
                        slot.socket,
                        SOL_SOCKET,
                        SO_CONNECT_TIME,
                        PSTR(ptr::from_mut(&mut age).cast()),
                        &mut length,
                    ),
                    "connect age",
                )?;
                if length != size_of::<u32>() as i32 || age == u32::MAX {
                    return Err("invalid connect age".into());
                }
                self.report.connect_age_seconds.push(age);
                check(ioctlsocket(slot.socket, FIONBIO, &mut 1), "nonblocking")?;
            }
            slot.accepted = true;
            self.report.accepted += 1;
        }
        // This finite diagnostic does not silently keep a listener with no
        // acceptance capacity. No claims about preexisting SYN/backlog follow.
        if !self.stopping
            && self.report.accepted == CONNECTION_SLOTS
            && self.listener != INVALID_SOCKET
        {
            abortive_close(&mut self.listener)?;
            self.report.capacity_listener_closes += 1;
        }
        for slot in self.slots.iter_mut() {
            if slot.socket == INVALID_SOCKET || !slot.accepted {
                continue;
            }
            if self.stopping || slot.deadline.expired(Instant::now()) {
                abortive_close(&mut slot.socket)?;
                self.report.deadline_closes += 1;
                continue;
            }
            // At most 1KiB per slot/poll. A slow client cannot monopolize owner.
            let mut bytes = [0u8; 1024];
            // SAFETY: exclusive descriptor and bounded output slice; nonblocking.
            let count = unsafe { recv(slot.socket, &mut bytes, SEND_RECV_FLAGS(0)) };
            if count > 0 {
                match slot.header.append(&bytes[..count as usize]) {
                    Ok(true) => {
                        self.report.headers += 1;
                        abortive_close(&mut slot.socket)?;
                    }
                    Err(()) => {
                        self.report.header_limit_closes += 1;
                        abortive_close(&mut slot.socket)?;
                    }
                    Ok(false) => {}
                }
            } else if count == 0 || unsafe { WSAGetLastError() } != WSAEWOULDBLOCK {
                abortive_close(&mut slot.socket)?;
            }
        }
        Ok(())
    }
    fn finish(mut self) -> Result<FrontdoorProbeReport, String> {
        self.stop()?;
        let end = Instant::now() + FIRST_HEADER_BUDGET;
        while self.pending() != 0 && Instant::now() < end {
            self.poll()?;
            std::thread::sleep(Duration::from_millis(1));
        }
        if self.pending() != 0 {
            return Err("accept drain timeout; owner retained and replacement forbidden".into());
        }
        self.report.cleanup_complete = true;
        Ok(std::mem::take(&mut self.report))
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let close_failed = self.stop().is_err();
        if close_failed || self.pending() != 0 {
            OWNER_FAULTED.store(true, Ordering::Release);
            // The entire ManuallyDrop slot array (including its pinned boxes)
            // stays retained without any fault-path replacement allocation.
            // Keep IOCP/Winsock alive; no pending address can be freed. This
            // failure path does not reset OWNER_ACTIVE or start replacement.
            return;
        }
        // SAFETY: every pending operation has completed; no native writes can
        // reference these slots. No Drop action is performed on the fault path.
        unsafe {
            ManuallyDrop::drop(&mut self.slots);
        }
        if !self.port.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.port);
            }
        }
        if self.started_winsock {
            unsafe {
                WSACleanup();
            }
        }
        OWNER_ACTIVE.store(false, Ordering::Release);
    }
}

/// Actual loopback sockets and IOCP; plaintext is never routed to business APIs.
/// A finite five-second admission batch is diagnostic evidence, not proof of
/// a persistent TLS listener, hard kernel resource bounds or scheduler deadlines.
pub fn probe() -> Result<FrontdoorProbeReport, String> {
    let mut total = FrontdoorProbeReport::default();
    for cycle in 0..3 {
        let mut owner = Owner::new()?;
        let address = owner.address;
        let start = Instant::now();
        let mut clients: Vec<(TcpStream, Instant, Option<u128>)> =
            Vec::with_capacity(CONNECTION_SLOTS);
        if cycle != 0 {
            for _ in 0..CONNECTION_SLOTS {
                let client = TcpStream::connect_timeout(&address, Duration::from_secs(1))
                    .map_err(|e| format!("connect: {e}"))?;
                let connected = Instant::now();
                client.set_nonblocking(true).map_err(|e| e.to_string())?;
                clients.push((client, connected, None));
            }
            // Deliberately leave completion packets queued. No clock starts at
            // dequeue, so this 1.2s consumes the original deadline.
            std::thread::sleep(Duration::from_millis(1200));
            owner.report.delayed_poll_millis = start.elapsed().as_millis();
            if cycle == 2 {
                clients[0]
                    .0
                    .write_all(b"GET /fc/ HTTP/1.1\r\nHost: localhost\r\n\r\n")
                    .map_err(|e| e.to_string())?;
                clients[2]
                    .0
                    .write_all(&[b'x'; super::HEADER_BYTES + 1])
                    .map_err(|e| e.to_string())?;
            }
            let mut next_byte = Instant::now();
            while start.elapsed() < Duration::from_secs(7) {
                owner.poll()?;
                for (client, connected, closed) in &mut clients {
                    if closed.is_some() {
                        continue;
                    }
                    let mut byte = [0];
                    match client.read(&mut byte) {
                        Ok(0) => *closed = Some(connected.elapsed().as_nanos()),
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::ConnectionReset
                                    | std::io::ErrorKind::ConnectionAborted
                                    | std::io::ErrorKind::BrokenPipe
                            ) =>
                        {
                            *closed = Some(connected.elapsed().as_nanos());
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        other => return Err(format!("unexpected client read: {other:?}")),
                    }
                }
                if clients.iter().all(|(_, _, closed)| closed.is_some()) {
                    break;
                }
                if Instant::now() >= next_byte {
                    // One incomplete header drips indefinitely unless the fixed
                    // deadline closes it. Other connections remain zero-byte.
                    let _ = clients[1].0.write_all(b"x");
                    next_byte += Duration::from_millis(200);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            if owner.connected() != 0 || owner.pending() != 0 {
                return Err("finite batch failed to close".into());
            }
            for (_, _, closed) in clients {
                owner
                    .report
                    .wire_close_nanos
                    .push(closed.ok_or("client did not observe close within diagnostic window")?);
            }
        }
        let report = owner.finish()?;
        if report.accept_completions != report.accept_submissions {
            return Err("completion ledger mismatch".into());
        }
        total.cycles += 1;
        total.accept_submissions += report.accept_submissions;
        total.accept_completions += report.accept_completions;
        total.canceled_accepts += report.canceled_accepts;
        total.accepted += report.accepted;
        total.headers += report.headers;
        total.deadline_closes += report.deadline_closes;
        total.header_limit_closes += report.header_limit_closes;
        total.capacity_listener_closes += report.capacity_listener_closes;
        total.connect_age_seconds.extend(report.connect_age_seconds);
        total.wire_close_nanos.extend(report.wire_close_nanos);
        total.delayed_poll_millis = total.delayed_poll_millis.max(report.delayed_poll_millis);
        total.fixed_owner_bytes = report.fixed_owner_bytes;
        total.diagnostic_vector_payload_capacity_bytes =
            report.diagnostic_vector_payload_capacity_bytes;
        total.cleanup_complete = report.cleanup_complete;
    }
    if total.accepted != 8
        || total.headers != 1
        || total.deadline_closes != 6
        || total.header_limit_closes != 1
        || total.canceled_accepts != 4
    {
        return Err(format!("unexpected diagnostic metrics: {total:?}"));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_finite_admission_deadline_and_drain() {
        let _serial = crate::NATIVE_TEST_SERIAL.lock().unwrap();
        let report = super::probe().expect("actual loopback candidate");
        println!("TCP_FRONTDOOR_CANDIDATE_PASS {report:?} TLS_NOT_RUN hard_kernel_bound_UNPROVEN full_TCP_to_header_5s_UNPROVEN");
        assert!(report.cleanup_complete);
        assert_eq!(report.accept_submissions, report.accept_completions);
    }
}
