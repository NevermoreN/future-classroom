//! Nonblocking update sockets and client frames borrow the original four-slot
//! owner. Caller drives HTTP parsing and bounded file chunks on the network loop.
use super::super::UpdatePeerIdentity;
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateTlsCall {
    Handshake,
    Decrypt,
    Encrypt,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateTlsStep {
    NeedInput,
    Handshake { complete: bool },
    Plaintext,
    Encrypted,
    PeerClosed,
}
pub struct UpdateTlsCompletion {
    pub step: UpdateTlsStep,
    pub consumed: usize,
    pub output: Vec<u8>,
}
impl TlsHttpHost {
    pub fn update_fetch_available(&self) -> bool {
        !self.owner.stopping
            && !self.owner.resources.slots.iter().any(|s| s.external_update)
            && (2..CONNECTION_SLOTS).any(|i| self.owner.resources.slots[i].phase == Phase::Ready)
    }
    pub fn begin_update_fetch(
        &mut self,
        peer: UpdatePeerIdentity,
        port: u16,
    ) -> Result<RequestToken, String> {
        if self.owner.stopping
            || !peer.address.is_private()
            || port == 0
            || peer.certificate_sha256 == [0; 32]
            || self.owner.resources.slots.iter().any(|s| s.external_update)
        {
            return Err("update fetch unavailable or invalid peer".into());
        }
        let index = (2..CONNECTION_SLOTS)
            .find(|&i| self.owner.resources.slots[i].phase == Phase::Ready)
            .ok_or("update fetch waits for shared connection slot")?;
        // SAFETY: exact nonblocking socket remains under the existing Net owner;
        // no overlapped operation is posted for this client socket.
        let socket = unsafe {
            WSASocketW(
                AF_INET.0 as i32,
                SOCK_STREAM.0,
                IPPROTO_TCP.0,
                None,
                0,
                WSA_FLAG_OVERLAPPED,
            )
        }
        .map_err(|e| format!("update socket: {e}"))?;
        let mut nonblocking = 1;
        let configured = unsafe {
            if ioctlsocket(socket, FIONBIO, &mut nonblocking) != 0 {
                Err(error("update nonblocking socket"))
            } else {
                let address = SOCKADDR_IN {
                    sin_family: AF_INET,
                    sin_port: port.to_be(),
                    sin_addr: IN_ADDR {
                        S_un: IN_ADDR_0 {
                            S_addr: u32::from_ne_bytes(peer.address.octets()),
                        },
                    },
                    ..Default::default()
                };
                let result = connect(
                    socket,
                    (&address as *const SOCKADDR_IN).cast(),
                    size_of::<SOCKADDR_IN>() as i32,
                );
                if result == 0 || matches!(WSAGetLastError(), WSAEWOULDBLOCK | WSAEINPROGRESS) {
                    Ok(())
                } else {
                    Err(error("update connect"))
                }
            }
        };
        if let Err(e) = configured {
            let mut socket = socket;
            close(&mut socket, true)?;
            return Err(e);
        }
        let slot = &mut self.owner.resources.slots[index];
        slot.external_update = true;
        slot.update_result_ready = false;
        slot.listener_scope = TlsListenerScope::UpdateFetch;
        slot.socket = socket;
        slot.phase = Phase::Handshake;
        slot.context_submitted = false;
        slot.retired = false;
        slot.frame.as_mut().ok_or("update slot frame missing")?.peer = Some(peer);
        Ok(slot.token)
    }
    fn update_fetch_index(&self, token: RequestToken) -> Result<usize, String> {
        let index = token.slot();
        if index >= CONNECTION_SLOTS
            || self.owner.resources.slots[index].token != token
            || !self.owner.resources.slots[index].external_update
        {
            return Err("stale update fetch owner".into());
        }
        Ok(index)
    }
    pub fn update_fetch_connected(&self, token: RequestToken) -> Result<bool, String> {
        let index = self.update_fetch_index(token)?;
        let socket = self.owner.resources.slots[index].socket;
        if socket == INVALID_SOCKET {
            return Err("update socket closed".into());
        }
        let mut code = [0u8; 4];
        let mut length = 4;
        // SAFETY: fixed writable socket option/result buffers.
        unsafe {
            if getsockopt(
                socket,
                SOL_SOCKET,
                SO_ERROR,
                PSTR(code.as_mut_ptr()),
                &mut length,
            ) != 0
            {
                return Err(error("update connect status"));
            }
            if i32::from_ne_bytes(code) != 0 {
                return Err("update connection refused".into());
            }
            let mut address = SOCKADDR_IN::default();
            let mut len = size_of::<SOCKADDR_IN>() as i32;
            if getpeername(socket, (&mut address as *mut SOCKADDR_IN).cast(), &mut len) == 0 {
                return Ok(true);
            }
            if WSAGetLastError() == WSAENOTCONN {
                Ok(false)
            } else {
                Err(error("update peer status"))
            }
        }
    }
    pub fn update_fetch_read(
        &mut self,
        token: RequestToken,
        bytes: &mut [u8],
    ) -> std::io::Result<usize> {
        let index = self
            .update_fetch_index(token)
            .map_err(std::io::Error::other)?;
        if bytes.is_empty() || bytes.len() > BUFFER_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "update receive bound",
            ));
        }
        let result = unsafe {
            recv(
                self.owner.resources.slots[index].socket,
                bytes,
                SEND_RECV_FLAGS(0),
            )
        };
        socket_result(result)
    }
    pub fn update_fetch_write(
        &mut self,
        token: RequestToken,
        bytes: &[u8],
    ) -> std::io::Result<usize> {
        let index = self
            .update_fetch_index(token)
            .map_err(std::io::Error::other)?;
        if bytes.is_empty() || bytes.len() > BUFFER_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "update send bound",
            ));
        }
        let result = unsafe {
            send(
                self.owner.resources.slots[index].socket,
                bytes,
                SEND_RECV_FLAGS(0),
            )
        };
        socket_result(result)
    }
    pub fn submit_update_tls(
        &mut self,
        token: RequestToken,
        call: UpdateTlsCall,
        bytes: &[u8],
    ) -> Result<(), String> {
        let index = self.update_fetch_index(token)?;
        let slot = &mut self.owner.resources.slots[index];
        if self.owner.stopping
            || slot.phase == Phase::Closed
            || slot.update_result_ready
            || bytes.len() > BUFFER_BYTES
            || call == UpdateTlsCall::Encrypt && bytes.len() > PLAINTEXT_BYTES
        {
            return Err("update TLS submission state or bounds".into());
        }
        let mut frame = slot.frame.take().ok_or("update TLS frame busy")?;
        frame.scope = TlsListenerScope::UpdateFetch;
        frame.call = match call {
            UpdateTlsCall::Handshake => Call::Handshake,
            UpdateTlsCall::Decrypt => Call::Decrypt,
            UpdateTlsCall::Encrypt => Call::Encrypt,
        };
        frame.input[..bytes.len()].copy_from_slice(bytes);
        frame.input_len = bytes.len();
        match self
            .owner
            .resources
            .worker
            .as_mut()
            .ok_or("missing TLS worker")?
            .try_submit(frame)
        {
            Ok(()) => {
                self.owner.resources.slots[index].context_submitted = true;
                self.owner.report.sspi_submissions += 1;
                Ok(())
            }
            Err((e, frame)) => {
                self.owner.resources.slots[index].frame = Some(frame);
                Err(format!("update TLS mailbox: {e:?}"))
            }
        }
    }
    pub fn poll_update_tls(
        &mut self,
        token: RequestToken,
    ) -> Result<Option<UpdateTlsCompletion>, String> {
        let index = self.update_fetch_index(token)?;
        let slot = &mut self.owner.resources.slots[index];
        if !slot.update_result_ready {
            return Ok(None);
        }
        slot.update_result_ready = false;
        let frame = slot.frame.as_mut().ok_or("update result frame missing")?;
        let result = frame
            .result
            .take()
            .ok_or("update result missing")?
            .map_err(|e| format!("update TLS: {e:?}"))?;
        let (step, consumed) = match result {
            Step::NeedInput => (UpdateTlsStep::NeedInput, 0),
            Step::Handshake { consumed, complete } => {
                (UpdateTlsStep::Handshake { complete }, consumed)
            }
            Step::Plaintext { consumed } => (UpdateTlsStep::Plaintext, consumed),
            Step::Encrypted => (UpdateTlsStep::Encrypted, frame.input_len),
            Step::PeerClosed => (UpdateTlsStep::PeerClosed, 0),
            _ => return Err("unexpected update TLS completion".into()),
        };
        let output = frame.output[..frame.output_len].to_vec();
        Ok(Some(UpdateTlsCompletion {
            step,
            consumed,
            output,
        }))
    }
    pub fn close_update_fetch(&mut self, token: RequestToken) -> Result<(), String> {
        let index = self.update_fetch_index(token)?;
        self.owner.resources.slots[index].update_result_ready = false;
        self.owner.close_slot(index, true)
    }
    pub fn update_fetch_retired(&self, token: RequestToken) -> Result<bool, String> {
        let index = self.update_fetch_index(token)?;
        let slot = &self.owner.resources.slots[index];
        Ok(slot.phase == Phase::Closed
            && slot.socket == INVALID_SOCKET
            && slot.frame.is_some()
            && slot.retired)
    }
    pub fn release_update_fetch(&mut self, token: RequestToken) -> Result<(), String> {
        if !self.update_fetch_retired(token)? {
            return Err("update TLS owner not physically retired".into());
        }
        let index = token.slot();
        let next = self
            .owner
            .issuer
            .issue(index)
            .map_err(|e| format!("update token: {e:?}"))?;
        let slot = &mut self.owner.resources.slots[index];
        slot.frame.as_mut().unwrap().wipe();
        slot.frame.as_mut().unwrap().token = next;
        slot.frame.as_mut().unwrap().peer = None;
        slot.token = next;
        slot.phase = Phase::Ready;
        slot.external_update = false;
        slot.update_result_ready = false;
        slot.context_submitted = false;
        slot.retired = false;
        slot.parser = Parser::new(next, true);
        slot.stream.as_mut().unwrap().reset();
        Ok(())
    }
}
fn socket_result(result: i32) -> std::io::Result<usize> {
    if result >= 0 {
        return Ok(result as usize);
    }
    let code = unsafe { WSAGetLastError() };
    if code == WSAEWOULDBLOCK {
        Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
    } else {
        Err(std::io::Error::from_raw_os_error(code.0))
    }
}
