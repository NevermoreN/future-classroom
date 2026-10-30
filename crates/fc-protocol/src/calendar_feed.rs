//! Strict, bounded decoding of the selected public holiday feed. Upstream schema
//! is descriptive, not trusted: duplicate/unknown keys and empty years fail.
use fc_domain::calendar::{CalendarCandidate, PublicCalendarDay, PublicDayKind};
use fc_domain::time::{LocalDate, LocalDateTime};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedError {
    TooLarge,
    InvalidJson,
    InvalidSource,
    InvalidCalendar,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Feed {
    #[serde(rename = "$schema", default, deserialize_with = "present_string")]
    schema: Option<String>,
    #[serde(rename = "$id", default, deserialize_with = "present_string")]
    id: Option<String>,
    year: i32,
    papers: Vec<String>,
    days: Vec<Day>,
}
fn present_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    String::deserialize(d).map(Some)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Day {
    name: String,
    date: String,
    is_off_day: bool,
}
fn date(s: &str) -> Result<LocalDate, FeedError> {
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return Err(FeedError::InvalidCalendar);
    }
    let year = s[..4].parse().map_err(|_| FeedError::InvalidCalendar)?;
    let month = s[5..7].parse().map_err(|_| FeedError::InvalidCalendar)?;
    let day = s[8..].parse().map_err(|_| FeedError::InvalidCalendar)?;
    LocalDate::new(year, month, day).map_err(|_| FeedError::InvalidCalendar)
}
/// Hashes the exact downloaded bytes. The timestamp and validators are trusted
/// adapter metadata, while all feed data receives independent domain validation.
pub fn parse(
    year: i32,
    bytes: &[u8],
    fetched_at: LocalDateTime,
    etag: Option<String>,
    last_modified: Option<String>,
) -> Result<CalendarCandidate, FeedError> {
    let feed: Feed = crate::parse(bytes).map_err(|e| match e {
        crate::ProtocolError::TooLarge => FeedError::TooLarge,
        _ => FeedError::InvalidJson,
    })?;
    if feed.year != year
        || feed.schema.as_deref().is_some_and(|v| {
            v != "https://raw.githubusercontent.com/NateScarlet/holiday-cn/master/schema.json"
        })
        || feed.id.as_deref().is_some_and(|v| {
            v != format!(
                "https://raw.githubusercontent.com/NateScarlet/holiday-cn/master/{year}.json"
            )
        })
    {
        return Err(FeedError::InvalidSource);
    }
    let days = feed
        .days
        .into_iter()
        .map(|d| {
            Ok(PublicCalendarDay {
                date: date(&d.date)?,
                kind: if d.is_off_day {
                    PublicDayKind::Rest
                } else {
                    PublicDayKind::Makeup
                },
                name: d.name,
            })
        })
        .collect::<Result<Vec<_>, FeedError>>()?;
    let candidate = CalendarCandidate {
        source_year: year,
        fetched_at,
        source_sha256: Sha256::digest(bytes).into(),
        papers: feed.papers,
        days,
        etag,
        last_modified,
    };
    candidate
        .validate()
        .map_err(|_| FeedError::InvalidCalendar)?;
    Ok(candidate)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn now() -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 1).unwrap()
    }
    // Synthetic dates and names; no embedded upstream holiday calendar.
    fn fixture() -> serde_json::Value {
        serde_json::json!({
            "year":2026,
            "papers":["https://www.gov.cn/zhengce/zhengceku/202511/content_1000000.htm"],
            "days":[{"date":"2026-06-01","name":"合成休息","isOffDay":true},
                    {"date":"2026-06-02","name":"合成补课","isOffDay":false}]
        })
    }
    fn decode(v: &serde_json::Value) -> Result<CalendarCandidate, FeedError> {
        parse(2026, &serde_json::to_vec(v).unwrap(), now(), None, None)
    }
    #[test]
    fn exact_byte_digest_kind_and_optional_official_identifiers() {
        let mut f = fixture();
        let bytes = serde_json::to_vec(&f).unwrap();
        let c = parse(
            2026,
            &bytes,
            now(),
            Some("\"tag\"".into()),
            Some("Thu, 01 Oct 2026 00:00:00 GMT".into()),
        )
        .unwrap();
        assert_eq!(c.source_sha256, <[u8; 32]>::from(Sha256::digest(&bytes)));
        assert_eq!(c.days[0].kind, PublicDayKind::Rest);
        assert_eq!(c.days[1].kind, PublicDayKind::Makeup);
        assert_eq!(c.fetched_at, now());
        f["$schema"] =
            "https://raw.githubusercontent.com/NateScarlet/holiday-cn/master/schema.json".into();
        f["$id"] =
            "https://raw.githubusercontent.com/NateScarlet/holiday-cn/master/2026.json".into();
        assert!(decode(&f).is_ok());
        f["$id"] = serde_json::Value::Null;
        assert_eq!(decode(&f), Err(FeedError::InvalidJson));
        f["$id"] = "https://example.invalid/2026.json".into();
        assert_eq!(decode(&f), Err(FeedError::InvalidSource));
    }
    #[test]
    fn duplicate_unknown_keys_numeric_year_and_date_grammar_fail_closed() {
        for bytes in [
            br#"{"year":2026,"year":2026,"papers":[],"days":[]}"#.as_slice(),
            br#"{"year":2026.0,"papers":[],"days":[]}"#.as_slice(),
            br#"{"year":2026,"papers":[],"days":[],"actor":"admin"}"#.as_slice(),
            br#"{"year":2026,"papers":[],"days":[{"date":"2026-01-01","name":"x","name":"y","isOffDay":true}]}"#.as_slice(),
        ] { assert!(parse(2026,bytes,now(),None,None).is_err()); }
        for invalid in [
            "2026-6-01",
            "2026-02-29",
            "2026-00-01",
            "2026-06-1 ",
            "２０２６-06-01",
            "2026-06-01Z",
        ] {
            let mut f = fixture();
            f["days"][0]["date"] = invalid.into();
            assert!(decode(&f).is_err(), "{invalid}");
        }
        let mut f = fixture();
        f["days"][0]["extra"] = true.into();
        assert!(decode(&f).is_err());
    }
    #[test]
    fn empty_unknown_year_duplicate_dates_and_spill_only_are_not_calendars() {
        let mut f = fixture();
        f["year"] = 2027.into();
        assert_eq!(decode(&f), Err(FeedError::InvalidSource));
        f = fixture();
        f["days"] = serde_json::json!([]);
        assert!(decode(&f).is_err());
        f = fixture();
        f["papers"] = serde_json::json!([]);
        assert!(decode(&f).is_err());
        f = fixture();
        f["days"][1]["date"] = "2026-06-01".into();
        assert!(decode(&f).is_err());
        f = fixture();
        f["days"][0]["date"] = "2025-12-31".into();
        assert!(decode(&f).is_ok());
        f["days"][1]["date"] = "2025-12-30".into();
        assert!(decode(&f).is_err());
        for invalid in ["2025-11-30", "2027-01-01"] {
            f = fixture();
            f["days"][0]["date"] = invalid.into();
            assert!(decode(&f).is_err());
        }
    }
    #[test]
    fn untrusted_notice_names_validators_and_size_are_rejected() {
        for notice in [
            "http://www.gov.cn/zhengce/content/a.htm",
            "https://www.gov.cn.evil/zhengce/content/a.htm",
            "https://www.gov.cn/zhengce/content/../a.htm",
            "https://www.gov.cn/zhengce/content/a.htm?x=1",
            "https://www.gov.cn/zhengce/content/a.htm#x",
        ] {
            let mut f = fixture();
            f["papers"][0] = notice.into();
            assert!(decode(&f).is_err(), "{notice}");
        }
        for name in [String::new(), "x".repeat(65), "a\nb".into()] {
            let mut f = fixture();
            f["days"][0]["name"] = name.into();
            assert!(decode(&f).is_err());
        }
        let bytes = serde_json::to_vec(&fixture()).unwrap();
        assert!(parse(2026, &bytes, now(), Some("a\r\nInjected: x".into()), None).is_err());
        assert_eq!(
            parse(2026, &vec![b' '; 65537], now(), None, None),
            Err(FeedError::TooLarge)
        );
        let depth = format!("{}0{}", "[".repeat(34), "]".repeat(34));
        assert_eq!(
            parse(2026, depth.as_bytes(), now(), None, None),
            Err(FeedError::InvalidJson)
        );
    }
}
