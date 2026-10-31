//! Bounded HTTP/1.1 response framing. Native owners retain buffers and receipts
//! until actual send completion; this state machine does not observe IOCP.
use fc_ports::http::{HttpError, RequestToken, ResponseHead, CHUNK_BYTES, MAX_HEADER_BYTES};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseError {
    InvalidHead,
    TooLarge,
    StaleRequest,
    Busy,
    InvalidChunk,
    TooLong,
    Incomplete,
    StaleAcknowledgement,
    SendFailed,
    Closed,
    Exhausted,
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => "Response",
    }
}

/// Encodes one complete head, including its final CRLF. No caller-supplied
/// connection/framing or upgrade negotiation field is accepted. All automatic
/// fields and the status line count towards the same 16KiB limit.
pub fn encode_head(head: &ResponseHead) -> Result<Vec<u8>, ResponseError> {
    head.validate().map_err(|error| match error {
        HttpError::TooLarge => ResponseError::TooLarge,
        _ => ResponseError::InvalidHead,
    })?;
    if matches!(head.status, 204 | 205 | 304) && head.content_length != 0 {
        return Err(ResponseError::InvalidHead);
    }
    for header in &head.headers {
        // Reject Connection itself rather than interpreting its nominations.
        // Consequently no named field can acquire hidden hop-by-hop semantics.
        if [
            "content-length",
            "transfer-encoding",
            "connection",
            "keep-alive",
            "proxy-connection",
            "proxy-authenticate",
            "proxy-authorization",
            "te",
            "trailer",
            "upgrade",
            "http2-settings",
        ]
        .iter()
        .any(|name| header.name.eq_ignore_ascii_case(name))
        {
            return Err(ResponseError::InvalidHead);
        }
    }
    let status = format!("HTTP/1.1 {} {}\r\n", head.status, reason(head.status));
    // RFC framing forbids Content-Length on a 204. The existing port restricts
    // 304 to length zero; this adapter does not infer a selected representation.
    let length = if head.status == 204 {
        String::new()
    } else {
        format!("Content-Length: {}\r\n", head.content_length)
    };
    const CLOSE: &[u8] = b"Connection: close\r\n";
    let mut size = status.len() + length.len() + CLOSE.len() + 2;
    for header in &head.headers {
        size = size
            .checked_add(header.name.len())
            .and_then(|size| size.checked_add(header.value.len()))
            .and_then(|size| size.checked_add(4))
            .ok_or(ResponseError::TooLarge)?;
    }
    if size > MAX_HEADER_BYTES {
        return Err(ResponseError::TooLarge);
    }
    let mut encoded = Vec::with_capacity(size);
    encoded.extend_from_slice(status.as_bytes());
    for header in &head.headers {
        encoded.extend_from_slice(header.name.as_bytes());
        encoded.extend_from_slice(b": ");
        encoded.extend_from_slice(&header.value);
        encoded.extend_from_slice(b"\r\n");
    }
    encoded.extend_from_slice(length.as_bytes());
    encoded.extend_from_slice(CLOSE);
    encoded.extend_from_slice(b"\r\n");
    debug_assert_eq!(encoded.len(), size);
    Ok(encoded)
}

static NEXT_SENDER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct Flight {
    sequence: u64,
    bytes: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Sending,
    Complete,
    Failed,
}

/// Linear logical-body receipt. It contains no bytes and cannot free native
/// memory. Dropping it does not acknowledge a send or release an in-flight slot.
/// A transport keeps this receipt alongside the exact native operation owner.
pub struct PendingChunk {
    token: RequestToken,
    sender: u64,
    sequence: u64,
    bytes: usize,
}
impl PendingChunk {
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn token(&self) -> RequestToken {
        self.token
    }
}

/// One frozen encoded head and a zero-body-copy counter, owned by one transport
/// request. Neither this owner nor its chunk receipt implements Clone.
pub struct BodySender {
    token: RequestToken,
    sender: u64,
    head: Vec<u8>,
    total: u64,
    acknowledged: u64,
    sequence: u64,
    flight: Option<Flight>,
    phase: Phase,
}
impl BodySender {
    /// A successfully encoded, bounded head is frozen before any body admission.
    pub fn new(token: RequestToken, head: &ResponseHead) -> Result<Self, ResponseError> {
        let encoded = encode_head(head)?;
        let sender = NEXT_SENDER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map_err(|_| ResponseError::Exhausted)?
            + 1;
        Ok(Self {
            token,
            sender,
            head: encoded,
            total: head.content_length,
            acknowledged: 0,
            sequence: 0,
            flight: None,
            phase: Phase::Sending,
        })
    }
    pub fn head_bytes(&self) -> &[u8] {
        &self.head
    }
    /// Retained Vec capacity for the native owner's allocation report; excludes
    /// allocator metadata and the inline size of this state machine.
    pub fn head_capacity(&self) -> usize {
        self.head.capacity()
    }
    pub fn acknowledged_bytes(&self) -> u64 {
        self.acknowledged
    }
    pub fn remaining_bytes(&self) -> u64 {
        self.total - self.acknowledged
    }
    pub fn has_in_flight(&self) -> bool {
        self.flight.is_some()
    }
    /// Logical completion only. It proves neither head/TLS shutdown transmission
    /// nor socket closure nor retirement of pinned native allocations.
    pub fn is_complete(&self) -> bool {
        self.phase == Phase::Complete
    }

    fn check(&self, token: RequestToken) -> Result<(), ResponseError> {
        if token != self.token {
            Err(ResponseError::StaleRequest)
        } else if self.phase != Phase::Sending {
            Err(ResponseError::Closed)
        } else {
            Ok(())
        }
    }

    /// Call before submitting a body chunk. No empty progress markers, chunks
    /// over 64KiB, overlapping submissions or bytes beyond Content-Length.
    pub fn begin_chunk(
        &mut self,
        token: RequestToken,
        bytes: usize,
    ) -> Result<PendingChunk, ResponseError> {
        self.check(token)?;
        if self.flight.is_some() {
            return Err(ResponseError::Busy);
        }
        if bytes == 0 || bytes > CHUNK_BYTES {
            return Err(ResponseError::InvalidChunk);
        }
        if bytes as u64 > self.remaining_bytes() {
            return Err(ResponseError::TooLong);
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(ResponseError::Exhausted)?;
        self.sequence = sequence;
        self.flight = Some(Flight { sequence, bytes });
        Ok(PendingChunk {
            token,
            sender: self.sender,
            sequence,
            bytes,
        })
    }

    /// Acknowledge one whole logical plaintext chunk only AFTER its complete
    /// ciphertext/native sends have actually completed. A short socket send
    /// must first be accumulated by its native owner; it is not a chunk ACK.
    /// A mismatched byte count permanently fails this response. Receipt checks
    /// reject stale/foreign callbacks without advancing any current counters.
    pub fn acknowledge_chunk(
        &mut self,
        pending: PendingChunk,
        plaintext_bytes_sent: usize,
    ) -> Result<(), ResponseError> {
        self.check(pending.token)?;
        let matching = self.flight.is_some_and(|flight| {
            pending.sender == self.sender
                && pending.sequence == flight.sequence
                && pending.bytes == flight.bytes
        });
        if !matching {
            return Err(ResponseError::StaleAcknowledgement);
        }
        if plaintext_bytes_sent != pending.bytes {
            self.phase = Phase::Failed;
            return Err(ResponseError::SendFailed);
        }
        // begin_chunk proved this addition is within the fixed declared total.
        self.acknowledged += pending.bytes as u64;
        self.flight = None;
        Ok(())
    }

    /// Explicit source EOF. An outstanding/short body cannot become complete;
    /// an early EOF fails closed and later callbacks cannot revive it.
    pub fn finish(&mut self, token: RequestToken) -> Result<(), ResponseError> {
        self.check(token)?;
        if self.flight.is_some() || self.acknowledged != self.total {
            self.phase = Phase::Failed;
            return Err(ResponseError::Incomplete);
        }
        self.phase = Phase::Complete;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
