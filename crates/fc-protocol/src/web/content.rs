//! Canonical teacher-content paths, strict mutation input and bounded disclosures.
//! A decoded URL is a requested key; authority still belongs to the live server lease.
use crate::{parse, ProtocolError, MAX_JSON_BYTES, SCHEMA_VERSION};
use fc_domain::{content::*, ids::*, time::*};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

pub const PAGE_SIZE: usize = 20;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
pub const MAX_OFFSET: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Homework,
    Reminder,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Homework {
        class_id: ClassId,
        subject_id: SubjectId,
        owner_teacher_id: PrincipalId,
        date: LocalDate,
    },
    Reminder {
        class_id: ClassId,
        date: LocalDate,
    },
}
impl Key {
    pub fn into_domain(self) -> ContentKey {
        match self {
            Self::Homework {
                class_id,
                subject_id,
                owner_teacher_id,
                date,
            } => ContentKey::Homework(HomeworkKey {
                class_id,
                subject_id,
                owner_teacher_id,
                actual_date: date,
            }),
            Self::Reminder { class_id, date } => ContentKey::SharedReminder(ReminderKey {
                class_id,
                actual_date: date,
            }),
        }
    }
}
pub(super) fn route_offset(value: &str) -> Option<usize> {
    usize::try_from(super::decimal(value, true)?)
        .ok()
        .filter(|n| *n <= MAX_OFFSET)
}
pub(super) fn route_key(kind: Kind, value: &str) -> Option<Key> {
    let fields: Vec<_> = value.split('/').collect();
    match (kind, fields.as_slice()) {
        (Kind::Homework, [class, subject, owner, date]) => Some(Key::Homework {
            class_id: ClassId::new(super::decimal(class, false)?).ok()?,
            subject_id: SubjectId::new(super::decimal(subject, false)?).ok()?,
            owner_teacher_id: PrincipalId::new(super::decimal(owner, false)?).ok()?,
            date: parse_date(date)?,
        }),
        (Kind::Reminder, [class, date]) => Some(Key::Reminder {
            class_id: ClassId::new(super::decimal(class, false)?).ok()?,
            date: parse_date(date)?,
        }),
        _ => None,
    }
}
fn parse_date(value: &str) -> Option<LocalDate> {
    let b = value.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return None;
    }
    LocalDate::new(
        value[..4].parse().ok()?,
        value[5..7].parse().ok()?,
        value[8..].parse().ok()?,
    )
    .ok()
}
pub fn date(value: LocalDate) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        value.month(),
        value.day()
    )
}
pub fn datetime(value: LocalDateTime) -> String {
    let seconds = value.second_of_day();
    format!(
        "{}T{:02}:{:02}:{:02}",
        date(value.date),
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Private classroom text is not a diagnostic payload or an authority claim.
pub struct SaveInput {
    pub command_id: CommandId,
    pub expected_revision: Revision,
    pub text: String,
}
pub fn valid_text(text: &str) -> bool {
    text.len() <= MAX_TEXT_BYTES
        && !text.chars().any(|c| {
            (c <= '\u{1f}' && !matches!(c, '\t' | '\n' | '\r'))
                || ('\u{7f}'..='\u{9f}').contains(&c)
        })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SaveDto {
    schema_version: u16,
    command_id: String,
    expected_revision: String,
    text: String,
}
pub fn save(bytes: &[u8]) -> Result<SaveInput, ProtocolError> {
    let dto: SaveDto = parse(bytes)?;
    if dto.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::UnsupportedSchema);
    }
    if dto.text.len() > MAX_TEXT_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    if !valid_text(&dto.text) {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(SaveInput {
        command_id: CommandId::new(
            super::decimal(&dto.command_id, false).ok_or(ProtocolError::InvalidCommand)?,
        )
        .map_err(|_| ProtocolError::InvalidCommand)?,
        expected_revision: Revision::new(
            super::decimal(&dto.expected_revision, true).ok_or(ProtocolError::InvalidCommand)?,
        ),
        text: dto.text,
    })
}

#[derive(Default)]
struct BoundedJson(Vec<u8>);
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_JSON_BYTES - self.0.len() {
            return Err(io::Error::other("content JSON byte limit"));
        }
        let len = self.0.len() + bytes.len();
        if len > self.0.capacity() {
            self.0
                .try_reserve_exact(
                    len.next_power_of_two().clamp(256, MAX_JSON_BYTES) - self.0.len(),
                )
                .map_err(|_| io::Error::other("content JSON allocation"))?;
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    let mut output = BoundedJson::default();
    serde_json::to_writer(&mut output, value).map_err(|_| ProtocolError::TooLarge)?;
    Ok(output.0)
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
enum KeyDto {
    #[serde(rename_all = "camelCase")]
    Homework {
        class_id: String,
        subject_id: String,
        owner_teacher_id: String,
        date: String,
    },
    #[serde(rename_all = "camelCase")]
    Reminder { class_id: String, date: String },
}
fn key_dto(key: &ContentKey) -> KeyDto {
    match key {
        ContentKey::Homework(k) => KeyDto::Homework {
            class_id: k.class_id.get().to_string(),
            subject_id: k.subject_id.get().to_string(),
            owner_teacher_id: k.owner_teacher_id.get().to_string(),
            date: date(k.actual_date),
        },
        ContentKey::SharedReminder(k) => KeyDto::Reminder {
            class_id: k.class_id.get().to_string(),
            date: date(k.actual_date),
        },
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Document<'a> {
    schema_version: u16,
    status: &'static str,
    today: String,
    server_now: String,
    expires_at: String,
    key: KeyDto,
    revision: String,
    text: &'a str,
    last_actor_id: Option<String>,
    updated_at: Option<String>,
}
fn document_dto<'a>(
    key: &ContentKey,
    now: LocalDateTime,
    expires_at: LocalDateTime,
    record: Option<&'a ContentRecord>,
) -> Result<Document<'a>, ProtocolError> {
    let key_date = match key {
        ContentKey::Homework(k) => k.actual_date,
        ContentKey::SharedReminder(k) => k.actual_date,
    };
    if key_date != now.date
        || expires_at.second_of_day() != 0
        || expires_at.date.days_since(now.date) != 1
        || record.is_some_and(|r| {
            r.key != *key
                || r.revision.get() == 0
                || !valid_text(&r.text)
                || r.updated_at.date != now.date
        })
    {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(Document {
        schema_version: SCHEMA_VERSION,
        status: "CONTENT",
        today: date(now.date),
        server_now: datetime(now),
        expires_at: datetime(expires_at),
        key: key_dto(key),
        revision: record.map_or_else(|| "0".into(), |r| r.revision.get().to_string()),
        text: record.map_or("", |r| r.text.as_str()),
        last_actor_id: record.map(|r| r.last_actor_id.get().to_string()),
        updated_at: record.map(|r| datetime(r.updated_at)),
    })
}
pub fn document(
    key: &ContentKey,
    now: LocalDateTime,
    expires_at: LocalDateTime,
    record: Option<&ContentRecord>,
) -> Result<Vec<u8>, ProtocolError> {
    encode(&document_dto(key, now, expires_at, record)?)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Committed<'a> {
    schema_version: u16,
    status: &'static str,
    command_id: String,
    content: Document<'a>,
}
pub fn committed(
    command_id: CommandId,
    key: &ContentKey,
    now: LocalDateTime,
    expires_at: LocalDateTime,
    record: &ContentRecord,
) -> Result<Vec<u8>, ProtocolError> {
    encode(&Committed {
        schema_version: SCHEMA_VERSION,
        status: "COMMITTED",
        command_id: command_id.get().to_string(),
        content: document_dto(key, now, expires_at, Some(record))?,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandResult<'a> {
    schema_version: u16,
    status: &'static str,
    command_id: String,
    state: &'static str,
    content: Option<Document<'a>>,
}
pub fn command_result(
    command_id: CommandId,
    content: Option<(&ContentKey, LocalDateTime, LocalDateTime, &ContentRecord)>,
) -> Result<Vec<u8>, ProtocolError> {
    let document = content
        .map(|(key, now, expires_at, record)| document_dto(key, now, expires_at, Some(record)))
        .transpose()?;
    encode(&CommandResult {
        schema_version: SCHEMA_VERSION,
        status: "COMMAND_RESULT",
        command_id: command_id.get().to_string(),
        state: if document.is_some() {
            "COMMITTED"
        } else {
            "NOT_FOUND"
        },
        content: document,
    })
}

pub enum Scope<'a> {
    Homework {
        class_id: ClassId,
        class_name: &'a str,
        subject_id: SubjectId,
        subject_name: &'a str,
        owner_teacher_id: PrincipalId,
        owner_display_name: &'a str,
    },
    Reminder {
        class_id: ClassId,
        class_name: &'a str,
    },
}
#[derive(Serialize)]
#[serde(untagged)]
enum ScopeDto<'a> {
    Homework {
        #[serde(rename = "classId")]
        class_id: String,
        #[serde(rename = "className")]
        class_name: &'a str,
        #[serde(rename = "subjectId")]
        subject_id: String,
        #[serde(rename = "subjectName")]
        subject_name: &'a str,
        #[serde(rename = "ownerTeacherId")]
        owner_teacher_id: String,
        #[serde(rename = "ownerDisplayName")]
        owner_display_name: &'a str,
    },
    Reminder {
        #[serde(rename = "classId")]
        class_id: String,
        #[serde(rename = "className")]
        class_name: &'a str,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Scopes<'a> {
    schema_version: u16,
    status: &'static str,
    kind: &'static str,
    today: String,
    identity_revision: String,
    offset: usize,
    total: usize,
    next_offset: Option<usize>,
    items: Vec<ScopeDto<'a>>,
}
pub fn scopes(
    kind: Kind,
    today: LocalDate,
    identity_revision: Revision,
    offset: usize,
    total: usize,
    items: &[Scope<'_>],
) -> Result<Vec<u8>, ProtocolError> {
    let max_total = match kind {
        Kind::Homework => 4096,
        Kind::Reminder => 128,
    };
    if offset > MAX_OFFSET
        || total > max_total
        || offset > total
        || items.len() != (total - offset).min(PAGE_SIZE)
    {
        return Err(ProtocolError::InvalidCommand);
    }
    let mut output = Vec::new();
    for item in items {
        output.push(match (kind, item) {
            (
                Kind::Homework,
                Scope::Homework {
                    class_id,
                    class_name,
                    subject_id,
                    subject_name,
                    owner_teacher_id,
                    owner_display_name,
                },
            ) => ScopeDto::Homework {
                class_id: class_id.get().to_string(),
                class_name,
                subject_id: subject_id.get().to_string(),
                subject_name,
                owner_teacher_id: owner_teacher_id.get().to_string(),
                owner_display_name,
            },
            (
                Kind::Reminder,
                Scope::Reminder {
                    class_id,
                    class_name,
                },
            ) => ScopeDto::Reminder {
                class_id: class_id.get().to_string(),
                class_name,
            },
            _ => return Err(ProtocolError::InvalidCommand),
        });
    }
    let next = offset + items.len();
    encode(&Scopes {
        schema_version: SCHEMA_VERSION,
        status: "SCOPES",
        kind: match kind {
            Kind::Homework => "HOMEWORK",
            Kind::Reminder => "REMINDER",
        },
        today: date(today),
        identity_revision: identity_revision.get().to_string(),
        offset,
        total,
        next_offset: (next < total).then_some(next),
        items: output,
    })
}

#[cfg(test)]
mod tests;
