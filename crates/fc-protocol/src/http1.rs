//! One bounded HTTP/1.1 request, independent of a listener or TLS implementation.
//! The trusted transport supplies correlation and TLS facts. A parsed head is
//! neither authorization nor permission to consume its body: callers must admit
//! it explicitly after their own routing, authentication and quota reservation.
use fc_ports::http::{
    Header, RequestHead, RequestToken, CHUNK_BYTES, MAX_HEADERS, MAX_HEADER_BYTES,
    MAX_TARGET_BYTES, MAX_TRANSFER_BYTES,
};

pub mod response;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidHead,
    HeadTooLarge,
    TooManyHeaders,
    InvalidLength,
    UnsupportedFraming,
    BodyTooLarge,
    AdmissionRequired,
    InvalidState,
    UnexpectedEof,
    ExtraBytes,
    Failed,
}

/// At most one event is emitted for each call. Body bytes borrow the caller's
/// existing plaintext buffer; this parser never allocates a body or a pipeline.
pub enum Event<'a> {
    NeedHead,
    Head(RequestHead),
    AwaitBodyAdmission,
    Body { bytes: &'a [u8], eof: bool },
    Complete,
}

impl std::fmt::Debug for Event<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedHead => f.write_str("NeedHead"),
            Self::Head(head) => f.debug_tuple("Head").field(head).finish(),
            Self::AwaitBodyAdmission => f.write_str("AwaitBodyAdmission"),
            Self::Body { bytes, eof } => f
                .debug_struct("Body")
                .field("byte_length", &bytes.len())
                .field("eof", eof)
                .finish(),
            Self::Complete => f.write_str("Complete"),
        }
    }
}

/// Only `input[..consumed]` was consumed. In particular, a head ending halfway
/// through a TLS frame leaves its entire coalesced body in that original frame.
/// A body event acknowledges parsing only, never a file worker's completion.
#[derive(Debug)]
pub struct Progress<'a> {
    pub consumed: usize,
    pub event: Event<'a>,
}

#[derive(Clone, Copy)]
enum State {
    Head,
    Admission(u64),
    Body(u64),
    Complete,
    Failed,
}

/// A connection-close request parser. It cannot reset or create a new token.
/// The caller retains its frame until consumers finish with each borrowed body
/// slice, then asks for another slice; no eager body prefetch is performed here.
pub struct Parser {
    token: RequestToken,
    tls: bool,
    head: Vec<u8>,
    state: State,
}

impl Parser {
    pub fn new(token: RequestToken, tls: bool) -> Self {
        Self {
            token,
            tls,
            head: Vec::with_capacity(MAX_HEADER_BYTES),
            state: State::Head,
        }
    }

    pub fn is_complete(&self) -> bool {
        matches!(self.state, State::Complete)
    }

    /// Includes the retained fixed-capacity header arena even after parsing.
    /// RequestHead allocations delivered to the caller must be charged too.
    pub fn header_capacity(&self) -> usize {
        self.head.capacity()
    }

    fn fail<T>(&mut self, error: Error) -> Result<T, Error> {
        self.head.fill(0);
        self.head.clear();
        self.state = State::Failed;
        Err(error)
    }

    pub fn feed<'a>(&mut self, input: &'a [u8]) -> Result<Progress<'a>, Error> {
        match self.state {
            State::Failed => Err(Error::Failed),
            State::Admission(_) => Ok(Progress {
                consumed: 0,
                event: Event::AwaitBodyAdmission,
            }),
            State::Complete if input.is_empty() => Ok(Progress {
                consumed: 0,
                event: Event::Complete,
            }),
            State::Complete => self.fail(Error::ExtraBytes),
            State::Body(remaining) => {
                if input.len() as u64 > remaining {
                    return self.fail(Error::ExtraBytes);
                }
                let consumed = input.len().min(CHUNK_BYTES);
                let remaining = remaining - consumed as u64;
                let eof = remaining == 0;
                self.state = if eof {
                    State::Complete
                } else {
                    State::Body(remaining)
                };
                Ok(Progress {
                    consumed,
                    event: Event::Body {
                        bytes: &input[..consumed],
                        eof,
                    },
                })
            }
            State::Head => {
                for (index, &byte) in input.iter().enumerate() {
                    if self.head.len() == MAX_HEADER_BYTES {
                        return self.fail(Error::HeadTooLarge);
                    }
                    let previous = self.head.last().copied();
                    if (previous == Some(b'\r') && byte != b'\n')
                        || (byte == b'\n' && previous != Some(b'\r'))
                    {
                        return self.fail(Error::InvalidHead);
                    }
                    self.head.push(byte);
                    if self.head.ends_with(b"\r\n\r\n") {
                        let parsed = parse_head(&self.head, self.token, self.tls);
                        self.head.fill(0);
                        self.head.clear();
                        return match parsed {
                            Ok(head) => {
                                self.state = State::Admission(head.content_length.unwrap_or(0));
                                Ok(Progress {
                                    consumed: index + 1,
                                    event: Event::Head(head),
                                })
                            }
                            Err(error) => self.fail(error),
                        };
                    }
                }
                Ok(Progress {
                    consumed: input.len(),
                    event: Event::NeedHead,
                })
            }
        }
    }

    /// Explicit admission after the head's business/authority and quota checks.
    /// The route limit can only tighten the existing global transfer bound.
    /// Even requests with no body require `admit_body(0)` (or another limit).
    pub fn admit_body(&mut self, max_bytes: u64) -> Result<(), Error> {
        let State::Admission(length) = self.state else {
            return self.fail(Error::InvalidState);
        };
        if max_bytes > MAX_TRANSFER_BYTES || length > max_bytes {
            return self.fail(Error::BodyTooLarge);
        }
        self.state = if length == 0 {
            State::Complete
        } else {
            State::Body(length)
        };
        Ok(())
    }

    /// Call on a genuine plaintext-stream EOF. It never treats disconnect as a
    /// substitute for Content-Length, or for the explicit business admission.
    pub fn finish_eof(&mut self) -> Result<(), Error> {
        match self.state {
            State::Complete => Ok(()),
            State::Admission(_) => self.fail(Error::AdmissionRequired),
            State::Failed => Err(Error::Failed),
            _ => self.fail(Error::UnexpectedEof),
        }
    }
}

fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn trim_ows(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        value = &value[1..];
    }
    while value.last().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        value = &value[..value.len() - 1];
    }
    value
}

fn length(value: &[u8]) -> Result<u64, Error> {
    if value.is_empty()
        || value.len() > 20
        || (value.len() > 1 && value[0] == b'0')
        || !value.iter().all(u8::is_ascii_digit)
    {
        return Err(Error::InvalidLength);
    }
    let mut result = 0u64;
    for &digit in value {
        result = result
            .checked_mul(10)
            .and_then(|n| n.checked_add(u64::from(digit - b'0')))
            .ok_or(Error::InvalidLength)?;
    }
    if result > MAX_TRANSFER_BYTES {
        return Err(Error::BodyTooLarge);
    }
    Ok(result)
}

fn parse_head(bytes: &[u8], token: RequestToken, tls: bool) -> Result<RequestHead, Error> {
    let head = bytes.strip_suffix(b"\r\n\r\n").ok_or(Error::InvalidHead)?;
    let mut lines = head.split(|&b| b == b'\n');
    let request = lines.next().ok_or(Error::InvalidHead)?;
    let request = request.strip_suffix(b"\r").unwrap_or(request);
    let mut words = request.split(|&b| b == b' ');
    let method = words.next().ok_or(Error::InvalidHead)?;
    let target = words.next().ok_or(Error::InvalidHead)?;
    if words.next() != Some(b"HTTP/1.1")
        || words.next().is_some()
        || method.is_empty()
        || !method.iter().copied().all(token_byte)
        || target.is_empty()
        || target.len() > MAX_TARGET_BYTES
        || target[0] != b'/'
        || !target.iter().all(|&b| (33..=126).contains(&b))
    {
        return Err(Error::InvalidHead);
    }
    let mut headers = Vec::new();
    let mut content_length = None;
    for line in lines {
        if headers.len() == MAX_HEADERS {
            return Err(Error::TooManyHeaders);
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let colon = line
            .iter()
            .position(|&b| b == b':')
            .ok_or(Error::InvalidHead)?;
        let name = &line[..colon];
        if name.is_empty() || !name.iter().copied().all(token_byte) {
            return Err(Error::InvalidHead);
        }
        let name = std::str::from_utf8(name).map_err(|_| Error::InvalidHead)?;
        let value = trim_ows(&line[colon + 1..]);
        if value.iter().any(|&b| (b < 32 && b != b'\t') || b == 127) {
            return Err(Error::InvalidHead);
        }
        if [
            "transfer-encoding",
            "te",
            "expect",
            "trailer",
            "upgrade",
            "content-encoding",
        ]
        .iter()
        .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
        {
            return Err(Error::UnsupportedFraming);
        }
        if name.eq_ignore_ascii_case("connection")
            && !value.eq_ignore_ascii_case(b"close")
            && !value.eq_ignore_ascii_case(b"keep-alive")
        {
            return Err(Error::UnsupportedFraming);
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(Error::InvalidLength);
            }
            content_length = Some(length(value)?);
        }
        headers.push(Header {
            name: name.into(),
            value: value.to_vec(),
        });
    }
    Ok(RequestHead {
        token,
        method: std::str::from_utf8(method)
            .map_err(|_| Error::InvalidHead)?
            .into(),
        target: std::str::from_utf8(target)
            .map_err(|_| Error::InvalidHead)?
            .into(),
        headers,
        tls,
        content_length,
        has_body: content_length.is_some_and(|n| n != 0),
    })
}
