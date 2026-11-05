//! One pinned, bare AcceptEx owner. Its clock is the original post-derived
//! candidate clock, never a TCP ESTABLISHED or accept-observed epoch.
use super::*;

pub(super) struct AcceptMemory {
    pub(super) operation: UnsafeCell<OVERLAPPED>,
    addresses: UnsafeCell<[u8; ACCEPT_ADDRESS_BYTES]>,
    _pinned: PhantomPinned,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AcceptState {
    Idle,
    Pending {
        serial: u64,
        deadline: Instant,
    },
    Closing {
        serial: u64,
        deadline: Instant,
        expired: bool,
    },
}
pub(super) struct AcceptSlot {
    pub(super) memory: Pin<Box<AcceptMemory>>,
    pub(super) socket: SOCKET,
    pub(super) serial: u64,
    pub(super) state: AcceptState,
    accept: LPFN_ACCEPTEX,
    failed: bool,
    #[cfg(test)]
    pub(super) fail_close_once: bool,
}
impl AcceptSlot {
    pub(super) fn new(accept: LPFN_ACCEPTEX) -> Self {
        Self {
            memory: Box::pin(AcceptMemory {
                operation: UnsafeCell::new(OVERLAPPED::default()),
                addresses: UnsafeCell::new([0; ACCEPT_ADDRESS_BYTES]),
                _pinned: PhantomPinned,
            }),
            socket: INVALID_SOCKET,
            serial: 0,
            state: AcceptState::Idle,
            accept,
            failed: false,
            #[cfg(test)]
            fail_close_once: false,
        }
    }
    pub(super) fn pending(&self) -> bool {
        self.state != AcceptState::Idle
    }
    pub(super) fn deadline(&self) -> Option<Instant> {
        match self.state {
            AcceptState::Pending { deadline, .. } | AcceptState::Closing { deadline, .. } => {
                Some(deadline)
            }
            AcceptState::Idle => None,
        }
    }
    pub(super) fn released(&self) -> bool {
        !self.pending() && self.socket == INVALID_SOCKET && !self.failed
    }
}
impl Owner {
    pub(super) fn bare_released(&self) -> bool {
        self.resources
            .accept
            .as_ref()
            .is_none_or(AcceptSlot::released)
            && self
                .resources
                .lan_accept
                .as_ref()
                .is_none_or(AcceptSlot::released)
            && self
                .resources
                .update_accept
                .as_ref()
                .is_none_or(AcceptSlot::released)
    }
    fn close_bare_socket_for(&mut self, listener_index: usize) -> Result<(), String> {
        let Some(bare) = self.resources.bare_accept_mut(listener_index) else {
            return Ok(());
        };
        let had_socket = bare.socket != INVALID_SOCKET;
        if had_socket {
            if let Some(observation) = self
                .report
                .close_observations
                .accept
                .as_mut()
                .filter(|o| o.serial == bare.serial)
            {
                observation
                    .socket_close_began_at
                    .get_or_insert_with(Instant::now);
            }
        }
        #[cfg(test)]
        let result = if std::mem::take(&mut bare.fail_close_once) {
            Err("controlled bare close failure".to_owned())
        } else {
            close(&mut bare.socket, true)
        };
        #[cfg(not(test))]
        let result = close(&mut bare.socket, true);
        if had_socket {
            if let Some(observation) = self
                .report
                .close_observations
                .accept
                .as_mut()
                .filter(|o| o.serial == bare.serial)
            {
                if result.is_ok() {
                    observation
                        .socket_returned_at
                        .get_or_insert_with(Instant::now);
                } else {
                    observation.failed = true;
                }
            }
        }
        if let Err(error) = result {
            // Keep this exact descriptor and allocation; never overwrite it or
            // create a replacement even if later shutdown happens to succeed.
            bare.failed = true;
            OWNER_FAULTED.store(true, Ordering::Release);
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn close_bare_accept(&mut self) -> Result<(), String> {
        let primary = self.cancel_bare_accept_for(0, false);
        let lan = self.cancel_bare_accept_for(1, false);
        primary.and(lan)
    }
    pub(super) fn cancel_bare_accept_for(
        &mut self,
        listener_index: usize,
        expired: bool,
    ) -> Result<(), String> {
        let listener = self.listener(listener_index);
        let Some(bare) = self.resources.bare_accept_mut(listener_index) else {
            return Ok(());
        };
        if let AcceptState::Pending { serial, deadline } = bare.state {
            bare.state = AcceptState::Closing {
                serial,
                deadline,
                expired,
            };
            if expired {
                self.report.accept_idle_cancellations += 1;
                self.report.close_observations.accept = Some(TlsAcceptClose {
                    serial,
                    hard_deadline: deadline,
                    alarm: header_close_alarm(deadline),
                    began_at: Instant::now(),
                    socket_close_began_at: None,
                    socket_returned_at: None,
                    failed: false,
                    completion_returned_at: None,
                });
            }
        }
        if bare.pending() && listener != INVALID_SOCKET {
            // SAFETY: exact original listener operation remains pinned and
            // charged. Even ERROR_NOT_FOUND does not discharge its IOCP debt.
            unsafe {
                let _ = CancelIoEx(socket_handle(listener), Some(bare.memory.operation.get()));
            }
        }
        self.close_bare_socket_for(listener_index)
    }
    pub(super) fn expire_bare_accept(&mut self, now: Instant) -> Result<(), String> {
        for listener_index in 0..3 {
            if self.bare_accept(listener_index).is_some_and(
            |bare| matches!(bare.state, AcceptState::Pending { deadline, .. } if now >= header_close_alarm(deadline)),
        ) {
            self.cancel_bare_accept_for(listener_index, true)?;
        }
        }
        Ok(())
    }
    pub(super) fn post_bare_accept(&mut self) -> Result<(), String> {
        self.post_bare_accept_with_clock(Instant::now(), Instant::now)
    }
    pub(super) fn post_bare_accept_for(&mut self, listener_index: usize) -> Result<(), String> {
        if self.listener(listener_index) == INVALID_SOCKET || self.stopping {
            return Ok(());
        }
        self.post_bare_accept_for_with_clock(listener_index, Instant::now(), Instant::now)
    }
    /// One captured origin covers preparation and the actual native post.
    /// Production always supplies the monotonic clock above; a private test
    /// clock can cross the boundary around real native IO without long sleeps.
    pub(super) fn post_bare_accept_with_clock(
        &mut self,
        began_at: Instant,
        now: impl FnMut() -> Instant,
    ) -> Result<(), String> {
        self.post_bare_accept_for_with_clock(0, began_at, now)
    }
    fn post_bare_accept_for_with_clock(
        &mut self,
        listener_index: usize,
        began_at: Instant,
        mut now: impl FnMut() -> Instant,
    ) -> Result<(), String> {
        let deadline = began_at
            .checked_add(BUDGET)
            .ok_or("bare accept original deadline overflow")?;
        if self.stopping
            || OWNER_FAULTED.load(Ordering::Acquire)
            || !self.http_mode
            || self.listener(listener_index) == INVALID_SOCKET
        {
            return Err("bare AcceptEx listener stopped".into());
        }
        let listener = self.listener(listener_index);
        let family = self.listener_family(listener_index);
        let bare = self
            .resources
            .bare_accept_mut(listener_index)
            .ok_or("missing bare accept owner")?;
        if !bare.released() {
            return Err("bare AcceptEx still owns native debt or socket".into());
        }
        let serial = bare
            .serial
            .checked_add(1)
            .ok_or("bare accept serial exhausted")?;
        let accept = bare.accept.ok_or("bare AcceptEx unavailable")?;
        // SAFETY: previous IOCP packet actually returned and the old descriptor
        // was transferred/closed; only now may this pinned operation be reused.
        unsafe {
            *bare.memory.operation.get() = OVERLAPPED::default();
            *bare.memory.addresses.get() = [0; ACCEPT_ADDRESS_BYTES];
            bare.socket = WSASocketW(
                family.0 as i32,
                SOCK_STREAM.0,
                IPPROTO_TCP.0,
                None,
                0,
                WSA_FLAG_OVERLAPPED,
            )
            .map_err(|e| format!("bare accept socket: {e}"))?;
        }
        // A slow preparation must not issue an already-expired positive post.
        // There is no IOCP debt yet, so only the exact new socket is closed.
        if now() >= header_close_alarm(deadline) {
            self.close_bare_socket_for(listener_index)?;
            return Err("bare accept preparation consumed original deadline".into());
        }
        // SAFETY: the same unused pinned operation/socket remains exclusively
        // owned. Native post latency consumes the already-captured interval.
        let success = unsafe {
            let bare = self.resources.bare_accept_mut(listener_index).unwrap();
            let mut received = 0;
            accept(
                listener,
                bare.socket,
                bare.memory.addresses.get().cast(),
                0,
                ACCEPT_ADDRESS_LENGTH,
                ACCEPT_ADDRESS_LENGTH,
                &mut received,
                bare.memory.operation.get(),
            )
            .as_bool()
        };
        if !success && unsafe { WSAGetLastError() } != WSA_IO_PENDING {
            let failure = error("bare AcceptEx post");
            self.close_bare_socket_for(listener_index)?;
            return Err(failure);
        }
        // A successful/PENDING call owes a real IOCP packet even if the call
        // returned too late. Publish that exact debt BEFORE negative cleanup;
        // never return an uncharged operation or mint another five seconds.
        let bare = self.resources.bare_accept_mut(listener_index).unwrap();
        bare.serial = serial;
        bare.state = AcceptState::Pending { serial, deadline };
        self.report.submissions += 1;
        self.report.accept_posts += 1;
        if now() >= header_close_alarm(deadline) {
            self.cancel_bare_accept_for(listener_index, true)?;
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn complete_bare_accept(
        &mut self,
        key: usize,
        succeeded: bool,
    ) -> Result<(), String> {
        self.complete_bare_accept_for(0, key, succeeded)
    }
    pub(super) fn complete_bare_accept_for(
        &mut self,
        listener_index: usize,
        key: usize,
        succeeded: bool,
    ) -> Result<(), String> {
        let bare = self
            .resources
            .bare_accept_mut(listener_index)
            .ok_or("foreign bare completion")?;
        if key != Self::listener_key(listener_index) || !bare.pending() {
            return Err("bare accept completion key or unsubmitted operation".into());
        }
        let state = bare.state;
        let deadline = bare.deadline().unwrap();
        let completed_at = Instant::now();
        // Serial is a local ledger, not information present in a kernel packet.
        // Safety comes from never reusing memory until the actual packet returns.
        match state {
            AcceptState::Pending { serial, .. } | AcceptState::Closing { serial, .. }
                if serial == bare.serial => {}
            _ => return Err("bare accept serial ledger mismatch".into()),
        }
        bare.state = AcceptState::Idle;
        self.report.completions += 1;
        self.report.accept_completions += 1;
        if matches!(state, AcceptState::Closing { expired: true, .. }) {
            self.report.accept_idle_returns += 1;
            if let Some(observation) = self
                .report
                .close_observations
                .accept
                .as_mut()
                .filter(|o| o.serial == bare.serial)
            {
                observation.completion_returned_at = Some(Instant::now());
            }
        }
        let expired = Instant::now() >= header_close_alarm(deadline)
            || matches!(state, AcceptState::Closing { expired: true, .. });
        let canceled = matches!(state, AcceptState::Closing { .. });
        if self.stopping || expired || canceled || !succeeded {
            if expired
                && self
                    .report
                    .close_observations
                    .accept
                    .is_none_or(|o| o.serial != bare.serial)
            {
                self.report.close_observations.accept = Some(TlsAcceptClose {
                    serial: bare.serial,
                    hard_deadline: deadline,
                    alarm: header_close_alarm(deadline),
                    began_at: Instant::now(),
                    socket_close_began_at: None,
                    socket_returned_at: None,
                    failed: false,
                    completion_returned_at: Some(completed_at),
                });
            }
            if self.stopping {
                self.report.accept_stop_discards += 1;
            } else if expired {
                self.report.accept_expired_discards += 1;
            } else {
                self.report.accept_error_discards += 1;
            }
            self.close_bare_socket_for(listener_index)?;
        } else if let Some(index) =
            self.resources
                .slots
                .iter()
                .enumerate()
                .position(|(index, slot)| {
                    slot.phase == Phase::Ready
                        && !(self.control_mode && listener_index != 0 && index == 0)
                        && !(listener_index == 2 && index < 2)
                })
        {
            let slot = &self.resources.slots[index];
            if slot.socket != INVALID_SOCKET
                || slot.pending.is_some()
                || slot.context_submitted
                || slot.retired
                || slot
                    .frame
                    .as_ref()
                    .is_none_or(|frame| frame.token != slot.token)
                || !slot
                    .stream
                    .as_ref()
                    .is_some_and(stream::Stream::waiting_for_connection)
            {
                return Err("Ready business slot retains previous ownership".into());
            }
            // All fallible association operations occur while the bare owner
            // still owns its exact socket. No business I/O has been submitted.
            let association = unsafe {
                check(
                    setsockopt(
                        self.bare_accept(listener_index).unwrap().socket,
                        SOL_SOCKET,
                        SO_UPDATE_ACCEPT_CONTEXT,
                        Some(&self.listener(listener_index).0.to_ne_bytes()),
                    ),
                    "bare accept context",
                )
                .and_then(|()| {
                    let peer =
                        configured::peer_address(self.bare_accept(listener_index).unwrap().socket)?;
                    if self
                        .source_policy(listener_index)
                        .is_some_and(|policy| !policy.contains(peer))
                    {
                        return Ok(false);
                    }
                    CreateIoCompletionPort(
                        socket_handle(self.bare_accept(listener_index).unwrap().socket),
                        Some(self.resources.port),
                        index + 2,
                        1,
                    )
                    .map(|_| true)
                    .map_err(|e| format!("bare socket association: {e}"))
                })
            };
            match association {
                Err(error) => {
                    self.report.accept_error_discards += 1;
                    self.close_bare_socket_for(listener_index)?;
                    return Err(error);
                }
                Ok(false) => {
                    self.report.accept_source_rejections += 1;
                    self.close_bare_socket_for(listener_index)?;
                    // This actual packet was consumed; replenish only after the
                    // rejected socket really closed. No SSPI permit was used.
                    if !self.stopping {
                        self.post_bare_accept_for(listener_index)?;
                    }
                    return Ok(());
                }
                Ok(true) => {}
            }
            // Recheck after association; no late success can gain a new clock.
            if Instant::now() >= header_close_alarm(deadline) {
                self.report.accept_expired_discards += 1;
                self.report.close_observations.accept = Some(TlsAcceptClose {
                    serial: self.bare_accept(listener_index).unwrap().serial,
                    hard_deadline: deadline,
                    alarm: header_close_alarm(deadline),
                    began_at: Instant::now(),
                    socket_close_began_at: None,
                    socket_returned_at: None,
                    failed: false,
                    completion_returned_at: Some(completed_at),
                });
                self.close_bare_socket_for(listener_index)?;
            } else {
                let socket = std::mem::replace(
                    &mut self
                        .resources
                        .bare_accept_mut(listener_index)
                        .unwrap()
                        .socket,
                    INVALID_SOCKET,
                );
                let slot = &mut self.resources.slots[index];
                slot.socket = socket;
                slot.listener_index = listener_index;
                slot.listener_scope = if listener_index == 2 {
                    TlsListenerScope::UpdateServe
                } else if self.control_mode && listener_index == 0 {
                    TlsListenerScope::LocalControl
                } else {
                    TlsListenerScope::Lan
                };
                slot.stream.as_mut().unwrap().bind_header_deadline(deadline);
                slot.phase = Phase::Handshake;
                self.report.accepted += 1;
                self.report.accept_handoffs += 1;
                // Replenish the same single bare owner before business driving.
                self.post_bare_accept_for(listener_index)?;
                return self.drive(index);
            }
        } else {
            self.close_bare_socket_for(listener_index)?;
            self.report.accept_capacity_rejections += 1;
        }
        if !self.stopping {
            self.post_bare_accept_for(listener_index)?;
        }
        Ok(())
    }
    pub(super) fn accept_ledger_matches(&self) -> bool {
        self.report.accept_posts == self.report.accept_completions
            && self.report.accept_completions
                == self.report.accept_handoffs
                    + self.report.accept_capacity_rejections
                    + self.report.accept_source_rejections
                    + self.report.accept_expired_discards
                    + self.report.accept_error_discards
                    + self.report.accept_stop_discards
    }
}
