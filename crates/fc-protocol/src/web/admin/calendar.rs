//! Public calendar candidates can be reviewed, but only the existing writer can adopt them.
use crate::{ProtocolError, SCHEMA_VERSION};
use fc_domain::{
    calendar::{PublicDayKind, PublicDaySummary},
    identity::IdentityState,
    ids::Revision,
    schedule_document::ScheduleDocument,
    time::{LocalDate, LocalDateTime},
};
use fc_ports::calendar::{CalendarFetchFailure, CalendarStatus};
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) fn year(value: &str) -> Option<i32> {
    if value.len() != 4 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().filter(|year| (1..=9999).contains(year))
}

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Request {
    Download {
        schema_version: u16,
    },
    Adopt {
        schema_version: u16,
        expected_revision: String,
        candidate_revision: String,
        reviewed: bool,
    },
    SetAutomatic {
        schema_version: u16,
        expected_policy: String,
        enabled: bool,
    },
}
pub enum Command {
    Download,
    Adopt {
        expected: Revision,
        candidate: Revision,
    },
    SetAutomatic {
        expected: Revision,
        enabled: bool,
    },
}
pub fn command(bytes: &[u8]) -> Result<Command, ProtocolError> {
    match super::parse(bytes)? {
        Request::Download {
            schema_version: SCHEMA_VERSION,
        } => Ok(Command::Download),
        Request::Adopt {
            schema_version: SCHEMA_VERSION,
            expected_revision,
            candidate_revision,
            reviewed: true,
        } => {
            let candidate = super::revision(&candidate_revision)?;
            if candidate == Revision::default() {
                return Err(ProtocolError::InvalidCommand);
            }
            Ok(Command::Adopt {
                expected: super::revision(&expected_revision)?,
                candidate,
            })
        }
        Request::SetAutomatic {
            schema_version: SCHEMA_VERSION,
            expected_policy,
            enabled,
        } => Ok(Command::SetAutomatic {
            expected: super::revision(&expected_policy)?,
            enabled,
        }),
        _ => Err(ProtocolError::InvalidCommand),
    }
}

fn date(value: LocalDate) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        value.month(),
        value.day()
    )
}
fn stamp(value: LocalDateTime) -> String {
    let seconds = value.second_of_day();
    format!(
        "{} {:02}:{:02}",
        date(value.date),
        seconds / 3600,
        seconds / 60 % 60
    )
}
fn day(value: &PublicDaySummary) -> Value {
    json!({
        "kind": match value.kind {
            Some(PublicDayKind::Rest) => "rest",
            Some(PublicDayKind::Makeup) => "makeup",
            None => "conflict",
        },
        "names": value.names,
    })
}
pub fn document(
    schedule: &ScheduleDocument,
    status: &CalendarStatus,
    identity: &IdentityState,
) -> Result<Vec<u8>, ProtocolError> {
    let summary = status
        .years
        .iter()
        .find(|entry| entry.year == status.selected_year);
    let error = summary
        .and_then(|entry| entry.last_error)
        .map(|error| match error {
            CalendarFetchFailure::Cancelled => "cancelled",
            CalendarFetchFailure::Network => "network",
            CalendarFetchFailure::Tls => "tls",
            CalendarFetchFailure::Http => "source",
            CalendarFetchFailure::InvalidData => "invalid",
            CalendarFetchFailure::TooLarge => "size",
            CalendarFetchFailure::Deadline => "timeout",
        });
    let candidate = status.candidate.as_ref().map(|candidate| {
        let changes = schedule
            .public_calendar_data
            .proposal(
                std::slice::from_ref(candidate),
                &schedule.public_calendar.entries,
            )
            .ok()
            .map(|proposal| {
                proposal
                    .differences
                    .iter()
                    .map(|change| {
                        json!({
                            "date": date(change.date),
                            "before": change.before.as_ref().map(day),
                            "after": change.after.as_ref().map(day),
                            "manualOverride": !change.manual_overrides.is_empty(),
                        })
                    })
                    .collect::<Vec<_>>()
            });
        json!({
            "revision": summary.map(|v| v.candidate_revision.get()).unwrap_or_default().to_string(),
            "fetchedAt": stamp(candidate.fetched_at),
            "sources": candidate.papers,
            "changes": changes,
            "adopted": schedule.public_calendar_data.years.iter().any(|year| year == candidate),
        })
    });
    super::encode(
        schedule.revision,
        &json!({
            "year": status.selected_year,
            "policyRevision": status.policy.revision.get().to_string(),
            "automatic": status.policy.enabled,
            "automaticAuthorized": status.policy.authorized(identity),
            "pending": summary.is_some_and(|entry| entry.pending),
            "error": error,
            "nextAllowed": summary.and_then(|entry| entry.next_allowed).map(stamp),
            "candidate": candidate,
        }),
    )
}
