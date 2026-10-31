//! Bounded owned HTTP transport messages. No native pointers, file paths, actors,
//! credentials or business authorization are manufactured by this transport.
pub const MAX_REQUESTS: usize = 8;
pub const MAX_POLL_EVENTS: usize = 2 * MAX_REQUESTS;
pub const MAX_HEADERS: usize = 64;
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
pub const MAX_TARGET_BYTES: usize = 2048;
pub const CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_TRANSFER_BYTES: u64 = 50 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpError {
    InvalidPrefix,
    InvalidGeneration,
    InvalidRequest,
    InvalidResponse,
    TooLarge,
    Busy,
    StaleRequest,
    Stopping,
    Native(u32),
    Invariant,
    Retained,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenPrefix {
    value: String,
    diagnostic: bool,
}
impl ListenPrefix {
    pub fn https(value: &str) -> Result<Self, HttpError> {
        Self::validate(value, false)
    }
    /// Explicit transport-only diagnostic. Never expose credentials or business
    /// routes on this prefix; this capability does not constitute HTTPS evidence.
    pub fn isolated_loopback(port: u16, marker: &str) -> Result<Self, HttpError> {
        if port == 0
            || !marker.starts_with("FutureClassroom-Test-")
            || marker.len() > 96
            || !marker
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(HttpError::InvalidPrefix);
        }
        Self::validate(&format!("http://127.0.0.1:{port}/{marker}/"), true)
    }
    fn validate(value: &str, diagnostic: bool) -> Result<Self, HttpError> {
        if value.len() > 512
            || !value.is_ascii()
            || value.bytes().any(|b| {
                b <= b' '
                    || b == 127
                    || matches!(b, b'*' | b'+' | b'?' | b'#' | b'@' | b'%' | b'\\')
            })
            || !value.ends_with('/')
        {
            return Err(HttpError::InvalidPrefix);
        }
        let authority = value
            .strip_prefix(if diagnostic { "http://" } else { "https://" })
            .ok_or(HttpError::InvalidPrefix)?;
        let (host, path) = authority.split_once('/').ok_or(HttpError::InvalidPrefix)?;
        let (host, port) = if let Some(tail) = host.strip_prefix('[') {
            let (host, port) = tail.split_once("]:").ok_or(HttpError::InvalidPrefix)?;
            host.parse::<std::net::Ipv6Addr>()
                .map_err(|_| HttpError::InvalidPrefix)?;
            (host, port)
        } else {
            host.rsplit_once(':').ok_or(HttpError::InvalidPrefix)?
        };
        if host.is_empty()
            || port.parse::<u16>().ok().filter(|p| *p != 0).is_none()
            || (!host.contains(':')
                && !host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')))
            || path.split('/').any(|s| matches!(s, "." | ".."))
        {
            return Err(HttpError::InvalidPrefix);
        }
        if diagnostic && host != "127.0.0.1" {
            return Err(HttpError::InvalidPrefix);
        }
        Ok(Self {
            value: value.into(),
            diagnostic,
        })
    }
    pub fn as_str(&self) -> &str {
        &self.value
    }
    pub fn is_diagnostic(&self) -> bool {
        self.diagnostic
    }
}
/// Issued only by a trusted transport. Values are response correlation, never
/// authentication or permission evidence. HTTP body decoders must not create it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestToken {
    generation: u64,
    slot: u8,
    serial: u64,
}
impl RequestToken {
    pub fn generation(self) -> u64 {
        self.generation
    }
    pub fn slot(self) -> usize {
        usize::from(self.slot)
    }
    pub fn serial(self) -> u64 {
        self.serial
    }
}
static NEXT_TRANSPORT_GENERATION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
/// One trusted transport owns one issuer. It cannot select, reset or clone an
/// existing generation. Creating another issuer cannot forge an existing token.
/// Issuers do not own native resources or bypass native poison/admission gates.
pub struct TokenIssuer {
    generation: u64,
    serial: u64,
}
impl TokenIssuer {
    pub fn new() -> Result<Self, HttpError> {
        use std::sync::atomic::Ordering;
        let generation = NEXT_TRANSPORT_GENERATION
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(1).filter(|v| *v <= usize::MAX as u64)
            })
            .map_err(|_| HttpError::InvalidGeneration)?
            + 1;
        Ok(Self {
            generation,
            serial: 0,
        })
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn issue(&mut self, slot: usize) -> Result<RequestToken, HttpError> {
        if slot >= MAX_REQUESTS {
            return Err(HttpError::InvalidGeneration);
        }
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or(HttpError::InvalidGeneration)?;
        Ok(RequestToken {
            generation: self.generation,
            slot: slot as u8,
            serial: self.serial,
        })
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: Vec<u8>,
}
impl Header {
    pub fn validate(&self) -> Result<(), HttpError> {
        if self.name.is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || self
                .value
                .iter()
                .any(|b| (*b < 32 && *b != b'\t') || *b == 127)
        {
            return Err(HttpError::InvalidRequest);
        }
        Ok(())
    }
}
pub fn validate_headers(headers: &[Header]) -> Result<(), HttpError> {
    if headers.len() > MAX_HEADERS {
        return Err(HttpError::TooLarge);
    }
    let mut count = 0usize;
    for h in headers {
        h.validate()?;
        count = count
            .checked_add(h.name.len())
            .and_then(|v| v.checked_add(h.value.len() + 4))
            .ok_or(HttpError::TooLarge)?;
    }
    if count > MAX_HEADER_BYTES {
        return Err(HttpError::TooLarge);
    }
    Ok(())
}
#[derive(Clone, PartialEq, Eq)]
pub struct RequestHead {
    pub token: RequestToken,
    pub method: String,
    pub target: String,
    pub headers: Vec<Header>,
    pub tls: bool,
    pub content_length: Option<u64>,
    pub has_body: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: u16,
    pub headers: Vec<Header>,
    pub content_length: u64,
}
impl ResponseHead {
    pub fn validate(&self) -> Result<(), HttpError> {
        if !(200..=599).contains(&self.status)
            || self.content_length > MAX_TRANSFER_BYTES
            || (self.status == 304 && self.content_length != 0)
        {
            return Err(HttpError::InvalidResponse);
        }
        validate_headers(&self.headers)?;
        if self.headers.iter().any(|h| {
            [
                "content-length",
                "transfer-encoding",
                "connection",
                "trailer",
            ]
            .iter()
            .any(|n| h.name.eq_ignore_ascii_case(n))
        }) {
            return Err(HttpError::InvalidResponse);
        }
        Ok(())
    }
}
#[derive(Clone, PartialEq, Eq)]
pub enum HttpEvent {
    Request(RequestHead),
    BodyChunk {
        token: RequestToken,
        bytes: Vec<u8>,
        eof: bool,
    },
    ResponseReady(RequestToken),
    ResponseComplete(RequestToken),
    Closed {
        token: RequestToken,
        error: u32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpState {
    Running,
    Quiescing,
    Canceling,
    Draining,
    Closed,
    Retained,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HttpMetrics {
    pub generation: u64,
    pub submitted: u64,
    pub completions: u64,
    pub immediate_success: u64,
    pub pending: u64,
    pub immediate_errors: u64,
    pub canceled: u64,
    pub active_slots: usize,
    pub in_flight: usize,
    pub peak_in_flight: usize,
    pub native_allocation_bytes: usize,
}

impl std::fmt::Debug for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Header")
            .field("name_bytes", &self.name.len())
            .field("value_bytes", &self.value.len())
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for RequestHead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestHead")
            .field("token", &self.token)
            .field("method_bytes", &self.method.len())
            .field("target_bytes", &self.target.len())
            .field("header_count", &self.headers.len())
            .field("tls", &self.tls)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for HttpEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(h) => f.debug_tuple("Request").field(h).finish(),
            Self::BodyChunk { token, bytes, eof } => f
                .debug_struct("BodyChunk")
                .field("token", token)
                .field("byte_length", &bytes.len())
                .field("eof", eof)
                .finish(),
            Self::ResponseReady(t) => f.debug_tuple("ResponseReady").field(t).finish(),
            Self::ResponseComplete(t) => f.debug_tuple("ResponseComplete").field(t).finish(),
            Self::Closed { token, error } => f
                .debug_struct("Closed")
                .field("token", token)
                .field("error", error)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefixes_never_infer_plaintext_or_wildcard() {
        for p in [
            "http://192.168.1.1:80/",
            "https://+:443/",
            "https://*:443/",
            "https://school:443/../",
            "https://school:443/?x",
            "https://user@school:443/",
            "https://school/",
            "https://school:0/",
        ] {
            assert_eq!(ListenPrefix::https(p), Err(HttpError::InvalidPrefix));
        }
        assert!(ListenPrefix::https("https://school.local:443/classroom/").is_ok());
        assert!(ListenPrefix::https("https://[::1]:443/classroom/").is_ok());
        assert!(
            ListenPrefix::isolated_loopback(54321, "FutureClassroom-Test-iocp")
                .unwrap()
                .is_diagnostic()
        );
    }
    #[test]
    fn tokens_cannot_be_reconstructed_for_an_existing_generation_or_wrap() {
        let mut first = TokenIssuer::new().unwrap();
        let old = first.issue(0).unwrap();
        let next = first.issue(0).unwrap();
        assert_eq!(old.generation(), next.generation());
        assert_ne!(old.serial(), next.serial());
        let mut second = TokenIssuer::new().unwrap();
        assert_ne!(old.generation(), second.issue(0).unwrap().generation());
        let mut exhausted = TokenIssuer {
            generation: 1,
            serial: u64::MAX,
        };
        assert_eq!(exhausted.issue(0), Err(HttpError::InvalidGeneration));
        assert_eq!(exhausted.serial, u64::MAX);
    }
    #[test]
    fn debug_redacts_target_header_values_and_body() {
        let mut issuer = TokenIssuer::new().unwrap();
        let token = issuer.issue(0).unwrap();
        let head = RequestHead {
            token,
            method: "SECRET".into(),
            target: "/SECRET".into(),
            headers: vec![Header {
                name: "SECRET".into(),
                value: b"SECRET".to_vec(),
            }],
            tls: true,
            content_length: Some(6),
            has_body: true,
        };
        assert!(!format!("{:?}", HttpEvent::Request(head)).contains("SECRET"));
        assert!(!format!(
            "{:?}",
            HttpEvent::BodyChunk {
                token,
                bytes: b"SECRET".to_vec(),
                eof: true
            }
        )
        .contains("SECRET"));
    }
    #[test]
    fn bounded_headers_and_response_framing() {
        assert!(ResponseHead {
            status: 304,
            headers: vec![],
            content_length: 0
        }
        .validate()
        .is_ok());
        assert!(ResponseHead {
            status: 304,
            headers: vec![],
            content_length: 3
        }
        .validate()
        .is_err());
        let h = Header {
            name: "x-token".into(),
            value: b"a\r\nb".to_vec(),
        };
        assert!(h.validate().is_err());
        assert!(ResponseHead {
            status: 200,
            headers: vec![Header {
                name: "Content-Length".into(),
                value: b"1".to_vec()
            }],
            content_length: 1
        }
        .validate()
        .is_err());
        assert!(validate_headers(&[Header {
            name: "x".into(),
            value: vec![b'x'; MAX_HEADER_BYTES]
        }])
        .is_err());
    }
}
