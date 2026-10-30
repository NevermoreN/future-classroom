//! Bounded public calendar transport. No school data, credentials, or caller URLs.
//! The caller supplies a worker; this module never creates a request thread. The
//! physical gate is held until synchronous WinHTTP and all handle drops return.
//! Stage timeouts and cooperative 30 s checks do not prove native hard preemption.
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

pub const MAX_BODY_BYTES: usize = 65_536;
pub const MAX_HEADER_BYTES: usize = 8192;
pub const MAX_VALIDATOR_BYTES: usize = 256;
#[cfg(any(windows, test))]
const CHUNK_BYTES: usize = 32_768;
static OWNER: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Download {
    Modified {
        bytes: Vec<u8>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
    /// The service must independently prove an existing valid cache before use.
    NotModified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadError {
    Unavailable,
    Busy,
    Cancelled,
    Timeout,
    InvalidYear,
    InvalidValidator,
    InvalidResponse,
    TooLarge,
    Forbidden,
    NotFound,
    HttpStatus,
    Tls,
    Network,
}
fn validator(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VALIDATOR_BYTES
        && value.bytes().all(|b| (32..=126).contains(&b))
}
fn validate_input(year: i32, validators: Option<&Validators>) -> Result<(), DownloadError> {
    if !(1..=9999).contains(&year) {
        return Err(DownloadError::InvalidYear);
    }
    if validators.is_some_and(|v| {
        [&v.etag, &v.last_modified]
            .into_iter()
            .flatten()
            .any(|v| !validator(v))
    }) {
        return Err(DownloadError::InvalidValidator);
    }
    Ok(())
}
struct Budget<'a> {
    started: Instant,
    cancelled: &'a dyn Fn() -> bool,
}
impl<'a> Budget<'a> {
    fn new(cancelled: &'a dyn Fn() -> bool) -> Self {
        Self {
            started: Instant::now(),
            cancelled,
        }
    }
    fn check(&self) -> Result<(), DownloadError> {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(self.cancelled)).unwrap_or(true) {
            Err(DownloadError::Cancelled)
        } else if self.started.elapsed() >= Duration::from_secs(30) {
            Err(DownloadError::Timeout)
        } else {
            Ok(())
        }
    }
    #[cfg(windows)]
    fn stage_millis(&self) -> Result<i32, DownloadError> {
        self.check()?;
        let left = Duration::from_secs(30).saturating_sub(self.started.elapsed());
        Ok(left.as_millis().clamp(1, 5000) as i32)
    }
    fn acquire(&self, owner: &'a Mutex<()>) -> Result<MutexGuard<'a, ()>, DownloadError> {
        self.check()?;
        match owner.try_lock() {
            Ok(g) => {
                self.check()?;
                Ok(g)
            }
            Err(TryLockError::WouldBlock) => Err(DownloadError::Busy),
            Err(TryLockError::Poisoned(_)) => Err(DownloadError::Unavailable),
        }
    }
}
#[cfg(any(windows, test))]
fn response_status(status: u32, validators: Option<&Validators>) -> Result<bool, DownloadError> {
    match status {
        200 => Ok(true),
        304 if validators.is_some_and(|v| v.etag.is_some() || v.last_modified.is_some()) => {
            Ok(false)
        }
        304 => Err(DownloadError::InvalidResponse),
        401 | 403 => Err(DownloadError::Forbidden),
        404 => Err(DownloadError::NotFound),
        _ => Err(DownloadError::HttpStatus),
    }
}
#[cfg(any(windows, test))]
fn append(bytes: &mut Vec<u8>, chunk: &[u8]) -> Result<(), DownloadError> {
    if chunk.len() > CHUNK_BYTES || bytes.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
        return Err(DownloadError::TooLarge);
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

/// Fixed provider: raw.githubusercontent.com/NateScarlet/holiday-cn/master/{year}.json.
/// Call on an existing bounded worker, never from a window procedure.
pub fn fetch(
    year: i32,
    validators: Option<&Validators>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Download, DownloadError> {
    let budget = Budget::new(cancelled);
    budget.check()?;
    validate_input(year, validators)?;
    let _owner = budget.acquire(&OWNER)?;
    #[cfg(windows)]
    {
        native::fetch(year, validators, &budget)
    }
    #[cfg(not(windows))]
    {
        Err(DownloadError::Unavailable)
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::ffi::c_void;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::Networking::WinHttp::*;

    struct Handle(*mut c_void);
    impl Handle {
        fn new(raw: *mut c_void) -> Result<Self, DownloadError> {
            if raw.is_null() {
                Err(native_error(unsafe { GetLastError().0 }))
            } else {
                Ok(Self(raw))
            }
        }
    }
    impl Drop for Handle {
        fn drop(&mut self) {
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
    fn native_error(code: u32) -> DownloadError {
        match code {
            ERROR_WINHTTP_TIMEOUT => DownloadError::Timeout,
            ERROR_WINHTTP_SECURE_FAILURE
            | ERROR_WINHTTP_SECURE_CERT_CN_INVALID
            | ERROR_WINHTTP_SECURE_CERT_DATE_INVALID
            | ERROR_WINHTTP_SECURE_CERT_REVOKED
            | ERROR_WINHTTP_SECURE_CERT_REV_FAILED
            | ERROR_WINHTTP_SECURE_CERT_WRONG_USAGE
            | ERROR_WINHTTP_SECURE_CHANNEL_ERROR
            | ERROR_WINHTTP_SECURE_INVALID_CA
            | ERROR_WINHTTP_SECURE_INVALID_CERT => DownloadError::Tls,
            ERROR_WINHTTP_HEADER_SIZE_OVERFLOW => DownloadError::TooLarge,
            _ => DownloadError::Network,
        }
    }
    fn result(r: windows::core::Result<()>) -> Result<(), DownloadError> {
        r.map_err(|e| native_error(e.code().0 as u32 & 0xffff))
    }
    fn option(handle: &Handle, name: u32, value: u32) -> Result<(), DownloadError> {
        result(unsafe { WinHttpSetOption(Some(handle.0), name, Some(&value.to_ne_bytes())) })
    }
    fn stage(handle: &Handle, budget: &Budget<'_>) -> Result<(), DownloadError> {
        let timeout = budget.stage_millis()?;
        result(unsafe { WinHttpSetTimeouts(handle.0, timeout, timeout, timeout, timeout) })
    }
    fn headers(
        handle: &Handle,
        query: u32,
        capacity: usize,
        optional: bool,
    ) -> Result<Option<String>, DownloadError> {
        let mut buffer = vec![0u16; capacity + 1];
        let mut size = (buffer.len() * 2) as u32;
        let mut index = 0;
        let r = unsafe {
            WinHttpQueryHeaders(
                handle.0,
                query,
                PCWSTR::null(),
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
                &mut index,
            )
        };
        if let Err(e) = r {
            return if optional && (e.code().0 as u32 & 0xffff) == ERROR_WINHTTP_HEADER_NOT_FOUND {
                Ok(None)
            } else if (e.code().0 as u32 & 0xffff) == 122 {
                Err(DownloadError::TooLarge)
            } else {
                Err(native_error(e.code().0 as u32 & 0xffff))
            };
        }
        if !size.is_multiple_of(2) || size as usize > buffer.len() * 2 {
            return Err(DownloadError::InvalidResponse);
        }
        buffer.truncate(size as usize / 2);
        if buffer.last() == Some(&0) {
            buffer.pop();
        }
        let s = String::from_utf16(&buffer).map_err(|_| DownloadError::InvalidResponse)?;
        if s.len() > capacity || s.contains('\0') {
            return Err(DownloadError::TooLarge);
        }
        Ok(Some(s))
    }
    pub(super) fn fetch(
        year: i32,
        validators: Option<&Validators>,
        budget: &Budget<'_>,
    ) -> Result<Download, DownloadError> {
        budget.check()?;
        let session = Handle::new(unsafe {
            WinHttpOpen(
                w!("FutureClassroom/PublicCalendar"),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        })?;
        budget.check()?;
        result(unsafe { WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 5000) })?;
        option(
            &session,
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2,
        )?;
        option(&session, WINHTTP_OPTION_CONNECT_RETRIES, 1)?;
        budget.check()?;
        let connection = Handle::new(unsafe {
            WinHttpConnect(session.0, w!("raw.githubusercontent.com"), 443, 0)
        })?;
        budget.check()?;
        let path: Vec<u16> = format!("/NateScarlet/holiday-cn/master/{year}.json\0")
            .encode_utf16()
            .collect();
        let request = Handle::new(unsafe {
            WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                PCWSTR(path.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        budget.check()?;
        option(
            &request,
            WINHTTP_OPTION_MAX_RESPONSE_HEADER_SIZE,
            MAX_HEADER_BYTES as u32,
        )?;
        option(
            &request,
            WINHTTP_OPTION_DISABLE_FEATURE,
            WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION,
        )?;
        option(
            &request,
            WINHTTP_OPTION_AUTOLOGON_POLICY,
            WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH,
        )?;
        option(
            &request,
            WINHTTP_OPTION_ENABLE_FEATURE,
            WINHTTP_ENABLE_SSL_REVOCATION,
        )?;
        option(
            &request,
            WINHTTP_OPTION_REDIRECT_POLICY,
            WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
        )?;
        let mut request_headers =
            String::from("Accept: application/json\r\nAccept-Encoding: identity\r\n");
        if let Some(v) = validators {
            if let Some(etag) = &v.etag {
                request_headers.push_str(&format!("If-None-Match: {etag}\r\n"));
            }
            if let Some(modified) = &v.last_modified {
                request_headers.push_str(&format!("If-Modified-Since: {modified}\r\n"));
            }
        }
        let request_headers: Vec<u16> = request_headers.encode_utf16().collect();
        budget.check()?;
        stage(&request, budget)?;
        let sent = unsafe { WinHttpSendRequest(request.0, Some(&request_headers), None, 0, 0, 0) };
        budget.check()?;
        result(sent)?;
        stage(&request, budget)?;
        let received = unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) };
        budget.check()?;
        result(received)?;
        headers(
            &request,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            MAX_HEADER_BYTES,
            false,
        )?;
        budget.check()?;
        let status = headers(&request, WINHTTP_QUERY_STATUS_CODE, 3, false)?
            .ok_or(DownloadError::InvalidResponse)?
            .parse()
            .map_err(|_| DownloadError::InvalidResponse)?;
        if !response_status(status, validators)? {
            return Ok(Download::NotModified);
        }
        let etag = headers(&request, WINHTTP_QUERY_ETAG, MAX_VALIDATOR_BYTES, true)?;
        budget.check()?;
        let last_modified = headers(
            &request,
            WINHTTP_QUERY_LAST_MODIFIED,
            MAX_VALIDATOR_BYTES,
            true,
        )?;
        budget.check()?;
        if [&etag, &last_modified]
            .into_iter()
            .flatten()
            .any(|v| !validator(v))
        {
            return Err(DownloadError::InvalidValidator);
        }
        let encoding = headers(&request, WINHTTP_QUERY_CONTENT_ENCODING, 32, true)?;
        if encoding.is_some_and(|v| !v.eq_ignore_ascii_case("identity")) {
            return Err(DownloadError::InvalidResponse);
        }
        budget.check()?;
        let length = headers(&request, WINHTTP_QUERY_CONTENT_LENGTH, 20, true)?;
        let length = length
            .map(|v| {
                if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(DownloadError::InvalidResponse);
                }
                v.parse::<usize>().map_err(|_| DownloadError::TooLarge)
            })
            .transpose()?;
        if length.is_some_and(|v| v > MAX_BODY_BYTES) {
            return Err(DownloadError::TooLarge);
        }
        let mut bytes = Vec::with_capacity(length.unwrap_or(CHUNK_BYTES));
        let mut chunk = [0u8; CHUNK_BYTES];
        loop {
            budget.check()?;
            stage(&request, budget)?;
            let mut read = 0;
            let r = unsafe {
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    CHUNK_BYTES as u32,
                    &mut read,
                )
            };
            budget.check()?;
            result(r)?;
            if read == 0 {
                break;
            }
            if read as usize > chunk.len() {
                return Err(DownloadError::InvalidResponse);
            }
            append(&mut bytes, &chunk[..read as usize])?;
        }
        if bytes.is_empty() || length.is_some_and(|v| v != bytes.len()) {
            return Err(DownloadError::InvalidResponse);
        }
        budget.check()?;
        Ok(Download::Modified {
            bytes,
            etag,
            last_modified,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validators_are_bounded_ascii_and_never_header_injection() {
        for v in ["", "a\r\nHost: attacker", "汉", "\t", "\u{7f}"] {
            assert!(!validator(v));
        }
        assert!(validator("W/\"safe-tag\""));
        assert!(validator(&"x".repeat(256)));
        assert!(!validator(&"x".repeat(257)));
        assert_eq!(
            validate_input(
                2026,
                Some(&Validators {
                    etag: Some("x\n".into()),
                    last_modified: None
                })
            ),
            Err(DownloadError::InvalidValidator)
        );
        assert_eq!(validate_input(0, None), Err(DownloadError::InvalidYear));
    }
    #[test]
    fn response_bounds_and_status_do_not_accept_redirect_or_uncached_304() {
        let mut b = Vec::new();
        append(&mut b, &[0; CHUNK_BYTES]).unwrap();
        append(&mut b, &[0; CHUNK_BYTES]).unwrap();
        assert_eq!(append(&mut b, &[0]), Err(DownloadError::TooLarge));
        assert_eq!(b.len(), MAX_BODY_BYTES);
        assert_eq!(
            append(&mut Vec::new(), &[0; CHUNK_BYTES + 1]),
            Err(DownloadError::TooLarge)
        );
        assert_eq!(
            response_status(304, None),
            Err(DownloadError::InvalidResponse)
        );
        assert_eq!(
            response_status(304, Some(&Validators::default())),
            Err(DownloadError::InvalidResponse)
        );
        assert_eq!(
            response_status(
                304,
                Some(&Validators {
                    etag: Some("\"x\"".into()),
                    last_modified: None
                })
            ),
            Ok(false)
        );
        for code in [301, 302, 307, 500, 204] {
            assert_eq!(response_status(code, None), Err(DownloadError::HttpStatus));
        }
        assert_eq!(response_status(403, None), Err(DownloadError::Forbidden));
        assert_eq!(response_status(404, None), Err(DownloadError::NotFound));
    }
    #[test]
    fn cancellation_panics_fail_closed_and_busy_never_releases_physical_owner() {
        let owner = Mutex::new(());
        let held = owner.lock().unwrap();
        let budget = Budget::new(&|| false);
        assert!(matches!(budget.acquire(&owner), Err(DownloadError::Busy)));
        assert!(owner.try_lock().is_err());
        assert_eq!(Budget::new(&|| true).check(), Err(DownloadError::Cancelled));
        assert_eq!(
            Budget::new(&|| panic!("cancel hook")).check(),
            Err(DownloadError::Cancelled)
        );
        let expired = Budget {
            started: Instant::now() - Duration::from_secs(31),
            cancelled: &|| false,
        };
        assert_eq!(expired.check(), Err(DownloadError::Timeout));
        assert_eq!(fetch(2026, None, &|| true), Err(DownloadError::Cancelled));
        assert_eq!(
            fetch(2026, None, &|| panic!("entry hook")),
            Err(DownloadError::Cancelled)
        );
        drop(held);
        assert!(budget.acquire(&owner).is_ok());
    }
}
