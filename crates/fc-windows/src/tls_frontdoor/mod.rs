//! Owned TLS execution for the admission candidate. Native contexts stay on one
//! fixed SSPI thread; bounded linear frames cross its mailbox. Production
//! listening and the original strict first-connection deadline remain separate
//! platform gates, rather than being inferred from a successful TLS handshake.
#[cfg(any(windows, test))]
use fc_ports::http::RequestToken;
#[cfg(any(windows, test))]
use std::sync::atomic::{compiler_fence, Ordering};

/// Read-only candidate owner accounting, not a kernel backlog/TCB bound or
/// evidence of an exact TCP establishment epoch. No request/user data included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TlsAcceptSnapshot {
    pub generation: u64,
    pub posts: u64,
    pub completions: u64,
    /// Actual expired-post cancellation requests (not completion proof).
    pub idle_cancellations: u64,
    /// Actual IOCP returns for those expired-post Closing operations.
    pub idle_returns: u64,
    pub capacity_rejects: u64,
    pub transfers: u64,
    pub ready_slots: usize,
    pub bound_slots: usize,
    pub native_pending: usize,
    pub owned_sockets: usize,
}

pub const CONNECTION_SLOTS: usize = 4;
pub const BUFFER_BYTES: usize = 64 * 1024;
pub const PLAINTEXT_BYTES: usize = 16 * 1024;

/// Public certificate identity, never a private key or authorization token.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CertificateThumbprint([u8; 20]);
impl CertificateThumbprint {
    pub fn parse(value: &str) -> Result<Self, TlsError> {
        if value.len() != 40 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(TlsError::InvalidInput);
        }
        let mut bytes = [0; 20];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let digit = |b: u8| match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => unreachable!(),
            };
            bytes[index] = digit(pair[0]) * 16 + digit(pair[1]);
        }
        Ok(Self(bytes))
    }
    #[cfg(windows)]
    pub(super) fn bytes(&self) -> &[u8; 20] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsError {
    InvalidInput,
    Busy,
    Stopped,
    Native(i32),
    Cleanup(i32),
    Protocol,
    Capacity,
    Faulted,
    ThreadPanic,
    UnexpectedCompletion,
    Certificate,
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))] // Native SSPI steps have no host implementation.
pub(super) enum Call {
    Handshake,
    Decrypt,
    Encrypt,
    /// Generate local close_notify on the existing context; does not delete it.
    Shutdown,
    /// Delete only this exact connection context, on its owning SSPI thread.
    Retire,
}
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))] // Native SSPI steps have no host implementation.
pub(super) enum Step {
    NeedInput,
    Handshake {
        consumed: usize,
        complete: bool,
    },
    Plaintext {
        consumed: usize,
    },
    Encrypted,
    PeerClosed,
    Shutdown {
        complete: bool,
    },
    /// Actual context deletion (or proof this exact context was already absent).
    Retired,
}

/// Exactly one allocation per connection; no Clone/Debug and no borrowed native
/// pointer crosses the thread boundary. Input/output may contain future HTTP
/// secrets and are wiped when this actual owner is finally retired.
#[cfg(any(windows, test))]
pub(super) struct Frame {
    pub token: RequestToken,
    pub call: Call,
    pub input: [u8; BUFFER_BYTES],
    pub input_len: usize,
    pub output: [u8; BUFFER_BYTES],
    pub output_len: usize,
    pub result: Option<Result<Step, TlsError>>,
}
#[cfg(any(windows, test))]
impl Frame {
    pub fn new(token: RequestToken) -> Box<Self> {
        Box::new(Self {
            token,
            call: Call::Handshake,
            input: [0; BUFFER_BYTES],
            input_len: 0,
            output: [0; BUFFER_BYTES],
            output_len: 0,
            result: None,
        })
    }
}
#[cfg(any(windows, test))]
impl Frame {
    /// Call only with the actual unique frame owner, after all native calls
    /// returned. This also erases previous connection contents before reuse.
    pub fn wipe(&mut self) {
        for byte in self.input.iter_mut().chain(self.output.iter_mut()) {
            // SAFETY: this final unique owner has no native call in flight and
            // no aliases to either byte array; volatile stores retain the wipe.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        compiler_fence(Ordering::SeqCst);
        self.input_len = 0;
        self.output_len = 0;
    }
}
#[cfg(any(windows, test))]
impl Drop for Frame {
    fn drop(&mut self) {
        self.wipe();
    }
}

#[cfg(any(windows, test))]
pub(super) trait Driver {
    fn call(&mut self, frame: &mut Frame) -> Result<Step, TlsError>;
    fn close(&mut self) -> Result<(), TlsError> {
        Ok(())
    }
}
#[cfg(windows)]
mod credentials;
#[cfg(any(windows, test))]
mod http_date;
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{TlsHttpEvent, TlsHttpHost};
#[cfg(windows)]
mod schannel;
#[cfg(any(windows, test))]
mod worker;

#[cfg(any(windows, test))]
fn validate_marker(marker: &str) -> Result<(), TlsError> {
    let nonce = marker
        .strip_prefix("FutureClassroom-Test-TLS-")
        .ok_or(TlsError::InvalidInput)?;
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(TlsError::InvalidInput);
    }
    Ok(())
}

/// Explicit finite loopback diagnostic. Never called by ordinary startup.
/// The certificate is selected read-only from the current user's MY store;
/// certificate provisioning and cleanup belong to the external probe runner.
#[cfg(windows)]
pub fn probe(thumbprint: &str, marker: &str) -> Result<(), String> {
    validate_marker(marker).map_err(|e| format!("TLS probe marker: {e:?}"))?;
    let thumbprint = CertificateThumbprint::parse(thumbprint)
        .map_err(|e| format!("TLS probe certificate identity: {e:?}"))?;
    native::run(thumbprint, marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_marker_cannot_inject_http_or_control_output() {
        let marker = "FutureClassroom-Test-TLS-0123456789abcdef0123456789abcdef";
        assert_eq!(validate_marker(marker), Ok(()));
        for bad in [
            "",
            "FutureClassroom-Test-TLS-a",
            "FutureClassroom-Test-TLS-0123456789abcdef0123456789abcdeg",
            "FutureClassroom-Test-TLS-0123456789abcdef0123456789abcde/",
            "FutureClassroom-Test-TLS-0123456789abcdef0123456789abcde\n",
            "FutureClassroom-Test-TLS-0123456789abcdef0123456789abcdef\r\n",
        ] {
            assert_eq!(validate_marker(bad), Err(TlsError::InvalidInput));
        }
    }
    #[test]
    fn certificate_identity_is_exact_ascii_hex_without_aliases() {
        let a = CertificateThumbprint::parse("aBcD0123456789abcdef0123456789abcdef0123").unwrap();
        assert!(
            a == CertificateThumbprint::parse("abcd0123456789abcdef0123456789abcdef0123").unwrap()
        );
        for bad in [
            "",
            "00",
            " abcd0123456789abcdef0123456789abcdef0123",
            "abcd0123456789abcdef0123456789abcdef01234g",
            "ａｂｃｄ0123456789abcdef0123456789abcdef012345",
        ] {
            assert!(CertificateThumbprint::parse(bad).is_err());
        }
    }
}
