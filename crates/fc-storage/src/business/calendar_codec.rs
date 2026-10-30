//! Strict, separately bounded candidate and durable attempt DTOs.
use super::{
    calendar::{Cache, Year},
    codec::*,
};
use crate::{StorageError, MAX_VALUE_BYTES};
use fc_domain::{calendar::*, ids::*, time::*};
use fc_ports::calendar::*;
use serde_json::{json, Value};

pub(super) fn candidate(c: &CalendarCandidate) -> Value {
    json!({"year":c.source_year,"fetched_at":time(c.fetched_at),"sha256":c.source_sha256.to_vec(),"papers":c.papers,"days":c.days.iter().map(|d|json!({"date":date(d.date),"kind":match d.kind{PublicDayKind::Rest=>"rest",PublicDayKind::Makeup=>"makeup"},"name":d.name})).collect::<Vec<_>>(),"etag":c.etag,"last_modified":c.last_modified})
}
fn optional_text(v: &Value, n: &str) -> Result<Option<String>, StorageError> {
    if v[n].is_null() {
        Ok(None)
    } else {
        Ok(Some(text(v, n)?))
    }
}
fn year(v: &Value, n: &str) -> Result<i32, StorageError> {
    let n = i32::try_from(number(v, n)?).map_err(|_| StorageError::Database)?;
    if !(1..=9999).contains(&n) {
        return Err(StorageError::Database);
    }
    Ok(n)
}
pub(super) fn read_candidate(v: &Value) -> Result<CalendarCandidate, StorageError> {
    object_fields(
        v,
        &[
            "year",
            "fetched_at",
            "sha256",
            "papers",
            "days",
            "etag",
            "last_modified",
        ],
    )?;
    let sha: Vec<u8> =
        serde_json::from_value(v["sha256"].clone()).map_err(|_| StorageError::Database)?;
    let papers = array(v, "papers")?;
    let days = array(v, "days")?;
    if papers.len() > MAX_CANDIDATE_PAPERS || days.len() > MAX_CANDIDATE_DAYS {
        return Err(StorageError::Database);
    }
    let c = CalendarCandidate {
        source_year: year(v, "year")?,
        fetched_at: read_time(&v["fetched_at"])?,
        source_sha256: sha.try_into().map_err(|_| StorageError::Database)?,
        papers: papers
            .iter()
            .map(|p| p.as_str().map(str::to_owned).ok_or(StorageError::Database))
            .collect::<Result<_, _>>()?,
        days: days
            .iter()
            .map(|d| {
                object_fields(d, &["date", "kind", "name"])?;
                Ok(PublicCalendarDay {
                    date: read_date(&d["date"])?,
                    kind: match d["kind"].as_str() {
                        Some("rest") => PublicDayKind::Rest,
                        Some("makeup") => PublicDayKind::Makeup,
                        _ => return Err(StorageError::Database),
                    },
                    name: text(d, "name")?,
                })
            })
            .collect::<Result<_, _>>()?,
        etag: optional_text(v, "etag")?,
        last_modified: optional_text(v, "last_modified")?,
    };
    c.validate().map_err(|_| StorageError::Database)?;
    Ok(c)
}
pub(super) fn public_data(data: &PublicCalendarData) -> Value {
    json!({"years":data.years.iter().map(candidate).collect::<Vec<_>>()})
}
pub(super) fn read_public_data(v: &Value) -> Result<PublicCalendarData, StorageError> {
    object_fields(v, &["years"])?;
    let years = array(v, "years")?;
    if years.len() > MAX_PUBLIC_YEARS {
        return Err(StorageError::Database);
    }
    let data = PublicCalendarData {
        years: years.iter().map(read_candidate).collect::<Result<_, _>>()?,
    };
    data.validate().map_err(|_| StorageError::Database)?;
    Ok(data)
}
pub(super) fn policy(p: &CalendarAutoPolicy) -> Value {
    json!({"revision":p.revision.get(),"enabled":p.enabled,"grant":p.grant.as_ref().map(|g|json!({"principal":g.principal_id.get(),"epoch":g.auth_epoch.get()}))})
}
pub(super) fn read_policy(v: &Value) -> Result<CalendarAutoPolicy, StorageError> {
    object_fields(v, &["revision", "enabled", "grant"])?;
    let enabled = boolean(v, "enabled")?;
    let grant = if v["grant"].is_null() {
        None
    } else {
        let g = &v["grant"];
        object_fields(g, &["principal", "epoch"])?;
        let epoch = number(g, "epoch")?;
        if epoch == 0 {
            return Err(StorageError::Database);
        }
        Some(CalendarPolicyGrant {
            principal_id: PrincipalId::new(number(g, "principal")?)
                .map_err(|_| StorageError::Database)?,
            auth_epoch: AuthEpoch::new(epoch),
        })
    };
    let p = CalendarAutoPolicy {
        revision: Revision::new(number(v, "revision")?),
        enabled,
        grant,
    };
    if enabled != p.grant.is_some() || (enabled && p.revision.get() == 0) {
        return Err(StorageError::Database);
    }
    Ok(p)
}
pub(super) fn ticket(t: &CalendarFetchTicket) -> Value {
    json!({"year":t.year,"sequence":t.attempt_sequence,"candidate_revision":t.expected_candidate_revision.get(),"mode_revision":t.captured_mode_revision.get(),"policy_revision":t.captured_policy_revision.get(),"etag":t.etag,"last_modified":t.last_modified})
}
pub(super) fn read_ticket(v: &Value) -> Result<CalendarFetchTicket, StorageError> {
    object_fields(
        v,
        &[
            "year",
            "sequence",
            "candidate_revision",
            "mode_revision",
            "policy_revision",
            "etag",
            "last_modified",
        ],
    )?;
    let t = CalendarFetchTicket {
        year: year(v, "year")?,
        attempt_sequence: number(v, "sequence")?,
        expected_candidate_revision: Revision::new(number(v, "candidate_revision")?),
        captured_mode_revision: Revision::new(number(v, "mode_revision")?),
        captured_policy_revision: Revision::new(number(v, "policy_revision")?),
        etag: optional_text(v, "etag")?,
        last_modified: optional_text(v, "last_modified")?,
    };
    if t.attempt_sequence == 0
        || [
            (&t.etag, MAX_ETAG_BYTES),
            (&t.last_modified, MAX_LAST_MODIFIED_BYTES),
        ]
        .into_iter()
        .any(|(header, limit)| {
            header.as_ref().is_some_and(|value| {
                value.is_empty()
                    || value.len() > limit
                    || value.bytes().any(|b| !(0x20..=0x7e).contains(&b))
            })
        })
    {
        return Err(StorageError::Database);
    }
    Ok(t)
}
fn failure(f: CalendarFetchFailure) -> &'static str {
    match f {
        CalendarFetchFailure::Cancelled => "cancelled",
        CalendarFetchFailure::Network => "network",
        CalendarFetchFailure::Tls => "tls",
        CalendarFetchFailure::Http => "http",
        CalendarFetchFailure::InvalidData => "invalid-data",
        CalendarFetchFailure::TooLarge => "too-large",
        CalendarFetchFailure::Deadline => "deadline",
    }
}
fn read_failure(v: &Value) -> Result<Option<CalendarFetchFailure>, StorageError> {
    Ok(match v.as_str() {
        None if v.is_null() => None,
        Some("cancelled") => Some(CalendarFetchFailure::Cancelled),
        Some("network") => Some(CalendarFetchFailure::Network),
        Some("tls") => Some(CalendarFetchFailure::Tls),
        Some("http") => Some(CalendarFetchFailure::Http),
        Some("invalid-data") => Some(CalendarFetchFailure::InvalidData),
        Some("too-large") => Some(CalendarFetchFailure::TooLarge),
        Some("deadline") => Some(CalendarFetchFailure::Deadline),
        _ => return Err(StorageError::Database),
    })
}
fn optional_time(v: &Value) -> Result<Option<LocalDateTime>, StorageError> {
    if v.is_null() {
        Ok(None)
    } else {
        Ok(Some(read_time(v)?))
    }
}
fn summary(y: &CalendarYearSummary) -> Value {
    json!({"year":y.year,"revision":y.candidate_revision.get(),"last_attempt":y.last_attempt.map(time),"next_allowed":y.next_allowed.map(time),"last_success":y.last_success.map(time),"error":y.last_error.map(failure),"pending":y.pending})
}
fn read_summary(v: &Value) -> Result<CalendarYearSummary, StorageError> {
    object_fields(
        v,
        &[
            "year",
            "revision",
            "last_attempt",
            "next_allowed",
            "last_success",
            "error",
            "pending",
        ],
    )?;
    Ok(CalendarYearSummary {
        year: year(v, "year")?,
        candidate_revision: Revision::new(number(v, "revision")?),
        last_attempt: optional_time(&v["last_attempt"])?,
        next_allowed: optional_time(&v["next_allowed"])?,
        last_success: optional_time(&v["last_success"])?,
        last_error: read_failure(&v["error"])?,
        pending: boolean(v, "pending")?,
    })
}
pub(super) fn cache(c: &Cache) -> Value {
    json!({"format":1,"provider":PROVIDER_VERSION,"sequence":c.sequence,"adoption_count":c.adoption_count,"watermark":c.watermark.map(time),"policy":policy(&c.policy),"years":c.years.iter().map(|y|json!({"summary":summary(&y.summary),"ticket":y.ticket.as_ref().map(ticket),"candidate_digest":y.candidate_digest.map(|d|d.to_vec())})).collect::<Vec<_>>()})
}
pub(super) fn read_cache(v: &Value) -> Result<Cache, StorageError> {
    object_fields(
        v,
        &[
            "format",
            "provider",
            "sequence",
            "adoption_count",
            "watermark",
            "policy",
            "years",
        ],
    )?;
    if number(v, "format")? != 1 || text(v, "provider")? != PROVIDER_VERSION {
        return Err(StorageError::Database);
    }
    let values = array(v, "years")?;
    if values.len() > MAX_PUBLIC_YEARS {
        return Err(StorageError::Database);
    }
    let years = values
        .iter()
        .map(|v| {
            object_fields(v, &["summary", "ticket", "candidate_digest"])?;
            Ok(Year {
                candidate_digest: if v["candidate_digest"].is_null() {
                    None
                } else {
                    let digest: Vec<u8> = serde_json::from_value(v["candidate_digest"].clone())
                        .map_err(|_| StorageError::Database)?;
                    Some(digest.try_into().map_err(|_| StorageError::Database)?)
                },
                summary: read_summary(&v["summary"])?,
                ticket: if v["ticket"].is_null() {
                    None
                } else {
                    Some(read_ticket(&v["ticket"])?)
                },
            })
        })
        .collect::<Result<_, StorageError>>()?;
    let c = Cache {
        adoption_count: number(v, "adoption_count")?,
        sequence: number(v, "sequence")?,
        watermark: optional_time(&v["watermark"])?,
        policy: read_policy(&v["policy"])?,
        years,
    };
    c.validate()?;
    Ok(c)
}
pub(super) fn status(c: &CalendarStatus) -> Value {
    json!({"policy":policy(&c.policy),"years":c.years.iter().map(summary).collect::<Vec<_>>(),"selected_year":c.selected_year,"candidate":c.candidate.as_ref().map(candidate)})
}
pub(super) fn encode_status(status_value: &CalendarStatus) -> Result<Vec<u8>, StorageError> {
    let output = bytes(&status(status_value))?;
    if output.len() > 2 * MAX_VALUE_BYTES {
        return Err(StorageError::DocumentTooLarge);
    }
    Ok(output)
}
pub(super) fn read_status(bytes: &[u8]) -> Result<CalendarStatus, StorageError> {
    let v = value_with_limit(bytes, MAX_VALUE_BYTES * 2)?;
    object_fields(&v, &["policy", "years", "selected_year", "candidate"])?;
    if array(&v, "years")?.len() > MAX_PUBLIC_YEARS {
        return Err(StorageError::Database);
    }
    Ok(CalendarStatus {
        policy: read_policy(&v["policy"])?,
        years: array(&v, "years")?
            .iter()
            .map(read_summary)
            .collect::<Result<_, _>>()?,
        selected_year: year(&v, "selected_year")?,
        candidate: if v["candidate"].is_null() {
            None
        } else {
            Some(read_candidate(&v["candidate"])?)
        },
    })
}
