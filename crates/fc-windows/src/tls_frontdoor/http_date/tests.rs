use super::*;
use fc_ports::http::{TokenIssuer, MAX_HEADERS, MAX_HEADER_BYTES};
use fc_protocol::http1::response::encode_head;

fn sample() -> UtcFields {
    UtcFields {
        year: 1994,
        month: 11,
        weekday: 0,
        day: 6,
        hour: 8,
        minute: 49,
        second: 37,
        millis: 999,
    }
}
fn head(status: u16) -> ResponseHead {
    ResponseHead {
        status,
        headers: Vec::new(),
        content_length: 0,
    }
}
fn token() -> RequestToken {
    TokenIssuer::new().unwrap().issue(0).unwrap()
}

#[test]
fn imf_fixdate_is_fixed_english_utc_with_verified_weekday_and_leap_days() {
    assert_eq!(
        format_utc(sample()).unwrap(),
        *b"Sun, 06 Nov 1994 08:49:37 GMT"
    );
    for (year, weekday, expected) in [
        (2000, 2, *b"Tue, 29 Feb 2000 00:00:00 GMT"),
        (2024, 4, *b"Thu, 29 Feb 2024 00:00:00 GMT"),
    ] {
        assert_eq!(
            format_utc(UtcFields {
                year,
                month: 2,
                weekday,
                day: 29,
                hour: 0,
                minute: 0,
                second: 0,
                millis: 0,
            })
            .unwrap(),
            expected
        );
    }
    for (value, expected) in [
        (
            UtcFields {
                year: 1,
                month: 1,
                day: 1,
                weekday: 1,
                ..sample()
            },
            *b"Mon, 01 Jan 0001 08:49:37 GMT",
        ),
        (
            UtcFields {
                year: 9999,
                month: 12,
                day: 31,
                weekday: 5,
                hour: 23,
                minute: 59,
                second: 59,
                ..sample()
            },
            *b"Fri, 31 Dec 9999 23:59:59 GMT",
        ),
    ] {
        assert_eq!(format_utc(value).unwrap(), expected);
    }
    for (weekday, day_name) in ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
        .into_iter()
        .enumerate()
    {
        let formatted = format_utc(UtcFields {
            year: 2026,
            day: weekday as u16 + 1,
            weekday: weekday as u16,
            ..sample()
        })
        .unwrap();
        assert_eq!(&formatted[..3], day_name.as_bytes());
    }
}

#[test]
fn invalid_windows_utc_structures_are_rejected_without_fabricating_a_date() {
    for bad in [
        UtcFields {
            year: 0,
            ..sample()
        },
        UtcFields {
            year: 10000,
            ..sample()
        },
        UtcFields {
            month: 0,
            ..sample()
        },
        UtcFields {
            month: 13,
            ..sample()
        },
        UtcFields {
            month: 257,
            ..sample()
        },
        UtcFields { day: 0, ..sample() },
        UtcFields {
            day: 31,
            ..sample()
        }, // November has only 30 days.
        UtcFields {
            day: 262,
            ..sample()
        },
        UtcFields {
            weekday: 7,
            ..sample()
        },
        UtcFields {
            weekday: 1,
            ..sample()
        },
        UtcFields {
            hour: 24,
            ..sample()
        },
        UtcFields {
            minute: 60,
            ..sample()
        },
        UtcFields {
            second: 60,
            ..sample()
        },
        UtcFields {
            millis: 1000,
            ..sample()
        },
        UtcFields {
            year: 1900,
            month: 2,
            day: 29,
            ..sample()
        },
        UtcFields {
            year: 2023,
            month: 2,
            day: 29,
            ..sample()
        },
        UtcFields {
            year: 2024,
            month: 2,
            day: 30,
            ..sample()
        },
    ] {
        assert_eq!(format_utc(bad), Err(DateError::InvalidUtc));
    }
}

#[test]
fn all_native_response_status_classes_get_one_date_in_the_frozen_head() {
    for status in [200, 204, 205, 304, 400, 405, 503] {
        let sender = body_sender_at(token(), head(status), sample(), false).unwrap();
        let wire = std::str::from_utf8(sender.head_bytes()).unwrap();
        assert_eq!(wire.matches("\r\nDate: ").count(), 1);
        assert!(wire.contains("\r\nDate: Sun, 06 Nov 1994 08:49:37 GMT\r\n"));
        assert!(wire.ends_with("Connection: close\r\n\r\n"));
        assert_eq!(sender.acknowledged_bytes(), 0);
    }
}

#[test]
fn caller_date_is_rejected_case_insensitively_instead_of_preserved_or_duplicated() {
    for (head_only, names) in [false, true].into_iter().flat_map(|head_only| {
        [vec!["Date"], vec!["dAtE"], vec!["date", "Date"]]
            .into_iter()
            .map(move |names| (head_only, names))
    }) {
        let mut response = head(200);
        response.headers = names
            .into_iter()
            .map(|name| Header {
                name: name.into(),
                value: b"forged".to_vec(),
            })
            .collect();
        assert!(matches!(
            body_sender_at(token(), response, sample(), head_only),
            Err(DateError::CallerDate)
        ));
    }
}

#[test]
fn head_date_uses_the_same_validation_and_budget_without_cl_or_logical_body() {
    for status in [200, 304, 403, 404, 405, 503] {
        let mut response = head(status);
        if status != 304 {
            response.content_length = 71;
        }
        let sender = body_sender_at(token(), response, sample(), true).unwrap();
        let wire = std::str::from_utf8(sender.head_bytes()).unwrap();
        assert!(!wire.contains("Content-Length:"));
        assert_eq!(wire.matches("\r\nDate: ").count(), 1);
        assert_eq!(sender.remaining_bytes(), 0);
        assert_eq!(sender.acknowledged_bytes(), 0);
    }
    let mut response = head(405);
    response.content_length = 71;
    response.headers.push(Header {
        name: "X-Pad".into(),
        value: Vec::new(),
    });
    let base = BodySender::new_for_head(token(), &response)
        .unwrap()
        .head_bytes()
        .len();
    const DATE_LINE_BYTES: usize = 6 + 29 + 2;
    response.headers[0]
        .value
        .resize(MAX_HEADER_BYTES - base - DATE_LINE_BYTES, b'x');
    let sender = body_sender_at(token(), response.clone(), sample(), true).unwrap();
    assert_eq!(sender.head_bytes().len(), MAX_HEADER_BYTES);
    response.headers[0].value.push(b'x');
    assert!(matches!(
        body_sender_at(token(), response, sample(), true),
        Err(DateError::Response(ResponseError::TooLarge))
    ));
    let mut response = head(200);
    response.headers.push(Header {
        name: "Connection".into(),
        value: b"close".to_vec(),
    });
    assert!(matches!(
        body_sender_at(token(), response, sample(), true),
        Err(DateError::Response(ResponseError::InvalidHead))
    ));
}

#[test]
fn generated_date_counts_towards_the_original_header_byte_and_field_limits() {
    let mut response = head(200);
    response.headers.push(Header {
        name: "X-Pad".into(),
        value: Vec::new(),
    });
    let base = encode_head(&response).unwrap().len();
    const DATE_LINE_BYTES: usize = 6 + 29 + 2;
    response.headers[0]
        .value
        .resize(MAX_HEADER_BYTES - base - DATE_LINE_BYTES, b'x');
    let sender = body_sender_at(token(), response.clone(), sample(), false).unwrap();
    assert_eq!(sender.head_bytes().len(), MAX_HEADER_BYTES);
    response.headers[0].value.push(b'x');
    assert!(
        encode_head(&response).is_ok(),
        "only the generated field exceeds the budget"
    );
    assert!(matches!(
        body_sender_at(token(), response, sample(), false),
        Err(DateError::Response(ResponseError::TooLarge))
    ));
    let mut response = head(200);
    response.headers = (0..MAX_HEADERS)
        .map(|_| Header {
            name: "X-Small".into(),
            value: Vec::new(),
        })
        .collect();
    assert!(response.validate().is_ok());
    assert!(matches!(
        body_sender_at(token(), response, sample(), false),
        Err(DateError::Response(ResponseError::TooLarge))
    ));
}

#[cfg(windows)]
#[test]
fn native_get_system_time_produces_one_valid_utc_date_without_local_clock_conversion() {
    let sender = body_sender(token(), head(200), false).unwrap();
    let wire = std::str::from_utf8(sender.head_bytes()).unwrap();
    let date = wire
        .lines()
        .find_map(|line| line.strip_prefix("Date: "))
        .unwrap();
    assert_eq!(date.len(), 29);
    assert_eq!(&date[25..], " GMT");
    assert_eq!(wire.matches("\r\nDate: ").count(), 1);
}
