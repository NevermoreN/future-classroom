//! Transport-owned Date for both native TLS response entry points. Clock reads
//! stay in this Windows adapter; the protocol encoder still checks the entire
//! final header block before any ciphertext is submitted.
use fc_domain::time::LocalDate;
use fc_ports::http::{Header, HttpError, RequestToken, ResponseHead};
use fc_protocol::http1::response::{BodySender, ResponseError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DateError {
    InvalidUtc,
    CallerDate,
    Response(ResponseError),
}

#[derive(Clone, Copy)]
struct UtcFields {
    year: u16,
    month: u16,
    weekday: u16, // Windows SYSTEMTIME: Sunday is zero.
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    millis: u16,
}

fn format_utc(value: UtcFields) -> Result<[u8; 29], DateError> {
    let month = u8::try_from(value.month).map_err(|_| DateError::InvalidUtc)?;
    let day = u8::try_from(value.day).map_err(|_| DateError::InvalidUtc)?;
    // LocalDate is used only for Gregorian validation, not timezone conversion.
    let date =
        LocalDate::new(i32::from(value.year), month, day).map_err(|_| DateError::InvalidUtc)?;
    let weekday = ((date.weekday().index() + 1) % 7) as u16;
    if value.weekday != weekday
        || value.hour >= 24
        || value.minute >= 60
        || value.second >= 60
        || value.millis >= 1000
    {
        return Err(DateError::InvalidUtc);
    }
    const DAYS: [&[u8; 3]; 7] = [b"Sun", b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat"];
    const MONTHS: [&[u8; 3]; 12] = [
        b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov",
        b"Dec",
    ];
    let mut bytes = *b"Sun, 00 Jan 0000 00:00:00 GMT";
    bytes[..3].copy_from_slice(DAYS[usize::from(value.weekday)]);
    bytes[8..11].copy_from_slice(MONTHS[usize::from(month - 1)]);
    let pair = |number: u16| [b'0' + (number / 10) as u8, b'0' + (number % 10) as u8];
    bytes[5..7].copy_from_slice(&pair(value.day));
    bytes[12..14].copy_from_slice(&pair(value.year / 100));
    bytes[14..16].copy_from_slice(&pair(value.year % 100));
    bytes[17..19].copy_from_slice(&pair(value.hour));
    bytes[20..22].copy_from_slice(&pair(value.minute));
    bytes[23..25].copy_from_slice(&pair(value.second));
    Ok(bytes)
}

fn body_sender_at(
    token: RequestToken,
    mut head: ResponseHead,
    utc: UtcFields,
    head_only: bool,
) -> Result<BodySender, DateError> {
    head.validate().map_err(|error| {
        DateError::Response(match error {
            HttpError::TooLarge => ResponseError::TooLarge,
            _ => ResponseError::InvalidHead,
        })
    })?;
    // Date is owned by the transport. Even one caller value must not survive
    // as a spoofed timestamp or become a duplicate of the generated field.
    if head
        .headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("date"))
    {
        return Err(DateError::CallerDate);
    }
    head.headers.push(Header {
        name: "Date".into(),
        value: format_utc(utc)?.to_vec(),
    });
    if head_only {
        BodySender::new_for_head(token, &head)
    } else {
        BodySender::new(token, &head)
    }
    .map_err(DateError::Response)
}

#[cfg(windows)]
pub(super) fn body_sender(
    token: RequestToken,
    head: ResponseHead,
    head_only: bool,
) -> Result<BodySender, DateError> {
    // GetSystemTime is available before Win7 and returns UTC, unlike the local
    // teaching-calendar Clock. It retains no pointer or native owner. No newer
    // GetSystemTimePreciseAsFileTime import or timezone conversion is needed.
    // SAFETY: the binding returns an owned SYSTEMTIME value.
    let utc = unsafe { windows::Win32::System::SystemInformation::GetSystemTime() };
    body_sender_at(
        token,
        head,
        UtcFields {
            year: utc.wYear,
            month: utc.wMonth,
            weekday: utc.wDayOfWeek,
            day: utc.wDay,
            hour: utc.wHour,
            minute: utc.wMinute,
            second: utc.wSecond,
            millis: utc.wMilliseconds,
        },
        head_only,
    )
}

#[cfg(test)]
mod tests;
