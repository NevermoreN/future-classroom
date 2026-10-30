//! Isolated Winsock admission candidate; never enabled by application startup.
//!
//! This is not an HTTPS server. The diagnostic binds only loopback, admits one
//! finite batch, and never dispatches plaintext to application ports. Fixed
//! application allocations do not bound SYN/backlog, kernel or SSPI memory.
use std::time::Duration;
#[cfg(any(windows, test))]
use std::time::Instant;

pub const CONNECTION_SLOTS: usize = 4;
pub const HEADER_BYTES: usize = 16 * 1024;
pub const FIRST_HEADER_BUDGET: Duration = Duration::from_secs(5);

/// A deadline is fixed before posting accept, never extended by incoming bytes.
/// It bounds the admitted candidate batch, not an inferred TCP establishment
/// timestamp. SO_CONNECT_TIME has no documented rounding/error guarantee.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug)]
struct Deadline(Instant);
#[cfg(any(windows, test))]
impl Deadline {
    fn posted(now: Instant) -> Self {
        Self(now + FIRST_HEADER_BUDGET)
    }
    fn expired(self, now: Instant) -> bool {
        now >= self.0
    }
}

#[cfg(any(windows, test))]
#[derive(Debug)]
struct HeaderBuffer {
    bytes: [u8; HEADER_BYTES],
    len: usize,
    complete: bool,
}
#[cfg(any(windows, test))]
impl HeaderBuffer {
    fn new() -> Self {
        Self {
            bytes: [0; HEADER_BYTES],
            len: 0,
            complete: false,
        }
    }
    fn append(&mut self, bytes: &[u8]) -> Result<bool, ()> {
        if self.complete || bytes.len() > HEADER_BYTES - self.len {
            return Err(());
        }
        let old = self.len;
        self.bytes[old..old + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        self.complete = self.bytes[old.saturating_sub(3)..self.len]
            .windows(4)
            .any(|w| w == b"\r\n\r\n");
        if !self.complete && self.len == HEADER_BYTES {
            return Err(());
        }
        Ok(self.complete)
    }
}

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{probe, FrontdoorProbeReport};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytes_do_not_renew_absolute_deadline() {
        let start = Instant::now();
        let deadline = Deadline::posted(start);
        let mut header = HeaderBuffer::new();
        for second in 0..5 {
            assert!(!header.append(b"x").unwrap());
            assert!(!deadline.expired(start + Duration::from_secs(second)));
        }
        assert!(deadline.expired(start + FIRST_HEADER_BUDGET));
    }
    #[test]
    fn split_terminator_and_exact_bound_are_checked() {
        let mut header = HeaderBuffer::new();
        header
            .append(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r")
            .unwrap();
        assert!(header.append(b"\n").unwrap());
        assert!(header.append(b"late").is_err());
        let mut full = HeaderBuffer::new();
        assert!(full.append(&[b'x'; HEADER_BYTES]).is_err());
        let mut exact = HeaderBuffer::new();
        exact.append(&[b'x'; HEADER_BYTES - 4]).unwrap();
        assert!(exact.append(b"\r\n\r\n").unwrap());
    }
}
