//! Public timetable DTOs. Private dual history and provenance never come from Web input.
use super::parse;
use crate::ProtocolError;
use fc_domain::{ids::*, schedule::*, schedule_document::*, time::*};
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, ProtocolError>;
fn invalid<T>() -> Result<T> {
    Err(ProtocolError::InvalidCommand)
}
fn number(s: &str) -> Result<u64> {
    if s.is_empty()
        || s.len() > 20
        || !s.bytes().all(|b| b.is_ascii_digit())
        || s.len() > 1 && s.starts_with('0')
    {
        return invalid();
    }
    s.parse().map_err(|_| ProtocolError::InvalidCommand)
}
fn id<T>(s: &str, make: impl FnOnce(u64) -> std::result::Result<T, InvalidId>) -> Result<T> {
    make(number(s)?).map_err(|_| ProtocolError::InvalidCommand)
}
fn date(s: &str) -> Result<LocalDate> {
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return invalid();
    }
    LocalDate::new(
        s[..4].parse().map_err(|_| ProtocolError::InvalidCommand)?,
        s[5..7].parse().map_err(|_| ProtocolError::InvalidCommand)?,
        s[8..].parse().map_err(|_| ProtocolError::InvalidCommand)?,
    )
    .map_err(|_| ProtocolError::InvalidCommand)
}
fn date_text(d: LocalDate) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}
fn role(s: &str) -> Result<TableRole> {
    match s {
        "single" => Ok(TableRole::Single),
        "odd" => Ok(TableRole::Odd),
        "even" => Ok(TableRole::Even),
        _ => invalid(),
    }
}
fn role_text(r: TableRole) -> &'static str {
    match r {
        TableRole::Single => "single",
        TableRole::Odd => "odd",
        TableRole::Even => "even",
    }
}
fn row_kind(s: &str) -> Result<RowKind> {
    match s {
        "course" => Ok(RowKind::Course),
        "activity" => Ok(RowKind::Activity),
        "recess" => Ok(RowKind::Rest(RestPurpose::Recess)),
        "lunch" => Ok(RowKind::Rest(RestPurpose::Lunch)),
        "rest-other" => Ok(RowKind::Rest(RestPurpose::Other)),
        "rest-unknown" => Ok(RowKind::Rest(RestPurpose::Unknown)),
        _ => invalid(),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cell {
    subject_id: String,
    binding: String,
    teacher_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Row {
    id: String,
    logical_key: String,
    group: String,
    group_number: Option<u16>,
    kind: String,
    begin: u16,
    end: u16,
    enabled_days: [bool; 7],
    courses: [Option<Cell>; 7],
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Table {
    id: String,
    role: String,
    rows: Vec<Row>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RowDetail {
    row_id: String,
    name: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellDetail {
    row_id: String,
    day: u8,
    room: String,
    note: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Term {
    id: String,
    revision: String,
    class_id: String,
    name: String,
    enabled: bool,
    start: String,
    end: String,
    first_week_monday: String,
    timing_sync: bool,
    tables: Vec<Table>,
    row_details: Vec<RowDetail>,
    cell_details: Vec<CellDetail>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    semester_id: String,
    date: String,
    mapping: String,
    source_date: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Calendar {
    coverage: Option<[String; 2]>,
    verified_years: Vec<i32>,
    entries: Vec<Entry>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    semesters: Vec<Term>,
    calendar_mode: &'static str,
    automatic_gaps: bool,
    school_calendar: Calendar,
    public_calendar: Calendar,
}

impl Row {
    fn domain(self) -> Result<TimetableRow> {
        let group = match (self.group.as_str(), self.group_number) {
            ("morning", None) => DayGroup::Morning,
            ("afternoon", None) => DayGroup::Afternoon,
            ("other", Some(n)) => DayGroup::Other(n),
            _ => return invalid(),
        };
        let mut courses: [Option<CourseCell>; 7] = std::array::from_fn(|_| None);
        for (target, c) in courses.iter_mut().zip(self.courses) {
            if let Some(c) = c {
                let teacher = match (c.binding.as_str(), c.teacher_id) {
                    ("default", None) => TeacherBinding::Default,
                    ("unassigned", None) => TeacherBinding::Unassigned,
                    ("explicit", Some(s)) => TeacherBinding::Explicit(id(&s, PrincipalId::new)?),
                    _ => return invalid(),
                };
                *target = Some(CourseCell {
                    subject_id: id(&c.subject_id, SubjectId::new)?,
                    teacher,
                });
            }
        }
        Ok(TimetableRow {
            id: id(&self.id, RowId::new)?,
            logical_key: id(&self.logical_key, LogicalRowKey::new)?,
            group,
            kind: row_kind(&self.kind)?,
            range: MinuteRange::new(
                Minute::new(self.begin).map_err(|_| ProtocolError::InvalidCommand)?,
                Minute::new(self.end).map_err(|_| ProtocolError::InvalidCommand)?,
            )
            .map_err(|_| ProtocolError::InvalidCommand)?,
            enabled_days: self.enabled_days,
            courses,
        })
    }
}
impl Term {
    fn domain(self, previous: &SemesterPlan) -> Result<SemesterPlan> {
        let sid = id(&self.id, SemesterId::new)?;
        if sid != previous.semester.id
            || number(&self.revision)? != previous.semester.revision.get()
            || id(&self.class_id, ClassId::new)? != previous.semester.class_id
        {
            return invalid();
        }
        if self.tables.len() > 2
            || self
                .tables
                .iter()
                .any(|t| t.rows.len() > MAX_ROWS_PER_TABLE)
            || self.row_details.len() > MAX_ROWS_PER_TABLE * 2
            || self.cell_details.len() > MAX_CELL_DETAILS
        {
            return Err(ProtocolError::TooLarge);
        }
        let tables = self
            .tables
            .into_iter()
            .map(|t| {
                Ok(WeekTable {
                    id: id(&t.id, TableId::new)?,
                    role: role(&t.role)?,
                    rows: t.rows.into_iter().map(Row::domain).collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(SemesterPlan {
            semester: Semester {
                id: sid,
                class_id: previous.semester.class_id,
                revision: previous.semester.revision,
                enabled: self.enabled,
                start: date(&self.start)?,
                end: date(&self.end)?,
                first_week_monday: date(&self.first_week_monday)?,
                tables,
            },
            name: self.name,
            timing_sync: self.timing_sync,
            dual_history: previous.dual_history.clone(),
            row_details: self
                .row_details
                .into_iter()
                .map(|d| {
                    Ok(RowDetails {
                        row_id: id(&d.row_id, RowId::new)?,
                        name: d.name,
                    })
                })
                .collect::<Result<_>>()?,
            cell_details: self
                .cell_details
                .into_iter()
                .map(|d| {
                    Ok(CellDetails {
                        row_id: id(&d.row_id, RowId::new)?,
                        day: Weekday::from_monday_index(d.day)
                            .ok_or(ProtocolError::InvalidCommand)?,
                        room: d.room,
                        note: d.note,
                    })
                })
                .collect::<Result<_>>()?,
        })
    }
}
fn term(p: &SemesterPlan) -> Term {
    let s = &p.semester;
    Term {
        id: s.id.get().to_string(),
        revision: s.revision.get().to_string(),
        class_id: s.class_id.get().to_string(),
        name: p.name.clone(),
        enabled: s.enabled,
        start: date_text(s.start),
        end: date_text(s.end),
        first_week_monday: date_text(s.first_week_monday),
        timing_sync: p.timing_sync,
        tables: s
            .tables
            .iter()
            .map(|t| Table {
                id: t.id.get().to_string(),
                role: role_text(t.role).into(),
                rows: t
                    .rows
                    .iter()
                    .map(|r| Row {
                        id: r.id.get().to_string(),
                        logical_key: r.logical_key.get().to_string(),
                        group: match r.group {
                            DayGroup::Morning => "morning",
                            DayGroup::Afternoon => "afternoon",
                            DayGroup::Other(_) => "other",
                        }
                        .into(),
                        group_number: match r.group {
                            DayGroup::Other(n) => Some(n),
                            _ => None,
                        },
                        kind: match r.kind {
                            RowKind::Course => "course",
                            RowKind::Activity => "activity",
                            RowKind::Rest(RestPurpose::Recess) => "recess",
                            RowKind::Rest(RestPurpose::Lunch) => "lunch",
                            RowKind::Rest(RestPurpose::Other) => "rest-other",
                            RowKind::Rest(RestPurpose::Unknown) => "rest-unknown",
                        }
                        .into(),
                        begin: r.range.begin.get(),
                        end: r.range.end.get(),
                        enabled_days: r.enabled_days,
                        courses: std::array::from_fn(|i| {
                            r.courses[i].as_ref().map(|c| Cell {
                                subject_id: c.subject_id.get().to_string(),
                                binding: match c.teacher {
                                    TeacherBinding::Default => "default",
                                    TeacherBinding::Unassigned => "unassigned",
                                    TeacherBinding::Explicit(_) => "explicit",
                                }
                                .into(),
                                teacher_id: match c.teacher {
                                    TeacherBinding::Explicit(id) => Some(id.get().to_string()),
                                    _ => None,
                                },
                            })
                        }),
                    })
                    .collect(),
            })
            .collect(),
        row_details: p
            .row_details
            .iter()
            .map(|d| RowDetail {
                row_id: d.row_id.get().to_string(),
                name: d.name.clone(),
            })
            .collect(),
        cell_details: p
            .cell_details
            .iter()
            .map(|d| CellDetail {
                row_id: d.row_id.get().to_string(),
                day: d.day.index() as u8,
                room: d.room.clone(),
                note: d.note.clone(),
            })
            .collect(),
    }
}
fn calendar(c: &CalendarConfig) -> Calendar {
    Calendar {
        coverage: c.school_coverage.map(|(a, b)| [date_text(a), date_text(b)]),
        verified_years: c.public_verified_years.clone(),
        entries: c
            .entries
            .iter()
            .map(|e| Entry {
                semester_id: e.actual_semester_id.get().to_string(),
                date: date_text(e.date),
                mapping: match e.mapping {
                    DayMapping::Rest => "rest",
                    DayMapping::Unconfirmed => "unconfirmed",
                    DayMapping::SourceDate(_) => "source",
                }
                .into(),
                source_date: match e.mapping {
                    DayMapping::SourceDate(d) => Some(date_text(d)),
                    _ => None,
                },
            })
            .collect(),
    }
}
pub fn document(d: &ScheduleDocument) -> Result<Vec<u8>> {
    super::encode(
        d.revision,
        &Data {
            semesters: d.semesters.iter().map(term).collect(),
            calendar_mode: match d.calendar_mode {
                CalendarMode::School => "school",
                CalendarMode::Public => "public",
            },
            automatic_gaps: d.automatic_gaps,
            school_calendar: calendar(&d.school_calendar),
            public_calendar: calendar(&d.public_calendar),
        },
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    schema_version: u16,
    expected_revision: String,
    action: String,
    candidate: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewTerm {
    class_id: String,
    name: String,
    start: String,
    end: String,
    first_week_monday: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteTerm {
    id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Dual {
    semester_id: String,
    enabled: bool,
    source_role: String,
    restore_previous: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddRow {
    semester_id: String,
    table_id: String,
    kind: String,
    begin: u16,
    end: u16,
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveCalendar {
    calendar_mode: String,
    automatic_gaps: bool,
    school_calendar: Calendar,
    public_calendar: Calendar,
}
fn candidate<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| ProtocolError::InvalidCommand)
}
fn calendar_domain(dto: Calendar, old: &CalendarConfig) -> Result<CalendarConfig> {
    if dto.entries.len() > MAX_CALENDAR_ENTRIES || dto.verified_years.len() > MAX_VERIFIED_YEARS {
        return Err(ProtocolError::TooLarge);
    }
    let mut c = old.clone();
    c.school_coverage = dto
        .coverage
        .map(|[a, b]| Ok((date(&a)?, date(&b)?)))
        .transpose()?;
    // Verified public-year provenance is adopted through the calendar usecase,
    // never fabricated by a general calendar form.
    if dto.verified_years != old.public_verified_years {
        return invalid();
    }
    c.entries = dto
        .entries
        .into_iter()
        .map(|e| {
            let mapping = match (e.mapping.as_str(), e.source_date) {
                ("rest", None) => DayMapping::Rest,
                ("unconfirmed", None) => DayMapping::Unconfirmed,
                ("source", Some(s)) => DayMapping::SourceDate(date(&s)?),
                _ => return invalid(),
            };
            Ok(CalendarEntry {
                actual_semester_id: id(&e.semester_id, SemesterId::new)?,
                date: date(&e.date)?,
                mapping,
            })
        })
        .collect::<Result<_>>()?;
    Ok(c)
}
pub fn mutation(
    bytes: &[u8],
    current: &ScheduleDocument,
    identity: &fc_domain::identity::IdentityState,
) -> Result<(Revision, ScheduleDocument)> {
    let r: Request = parse(bytes)?;
    if r.schema_version != 1 {
        return Err(ProtocolError::UnsupportedSchema);
    }
    let expected = super::revision(&r.expected_revision)?;
    let mut d = current.clone();
    match r.action.as_str() {
        "createSemester" => {
            let c: NewTerm = candidate(r.candidate)?;
            if d.semesters.len() >= MAX_SEMESTERS {
                return Err(ProtocolError::TooLarge);
            }
            let sid = SemesterId::new(d.allocate_id().map_err(|_| ProtocolError::InvalidCommand)?)
                .map_err(|_| ProtocolError::InvalidCommand)?;
            let tid = TableId::new(d.allocate_id().map_err(|_| ProtocolError::InvalidCommand)?)
                .map_err(|_| ProtocolError::InvalidCommand)?;
            d.semesters.push(SemesterPlan::new(
                Semester {
                    id: sid,
                    class_id: id(&c.class_id, ClassId::new)?,
                    revision: Revision::default(),
                    enabled: true,
                    start: date(&c.start)?,
                    end: date(&c.end)?,
                    first_week_monday: date(&c.first_week_monday)?,
                    tables: vec![WeekTable {
                        id: tid,
                        role: TableRole::Single,
                        rows: vec![],
                    }],
                },
                c.name,
            ));
        }
        "saveSemester" => {
            let c: Term = candidate(r.candidate)?;
            let index = d
                .semesters
                .iter()
                .position(|p| p.semester.id.get().to_string() == c.id)
                .ok_or(ProtocolError::InvalidCommand)?;
            d.semesters[index] = c.domain(&d.semesters[index])?;
        }
        "deleteSemester" => {
            let c: DeleteTerm = candidate(r.candidate)?;
            let sid = id(&c.id, SemesterId::new)?;
            let i = d
                .semesters
                .iter()
                .position(|p| p.semester.id == sid)
                .ok_or(ProtocolError::InvalidCommand)?;
            d.semesters.remove(i);
            d.school_calendar
                .entries
                .retain(|e| e.actual_semester_id != sid);
            d.public_calendar
                .entries
                .retain(|e| e.actual_semester_id != sid);
        }
        "setDualMode" => {
            let c: Dual = candidate(r.candidate)?;
            let sid = id(&c.semester_id, SemesterId::new)?;
            let index = d
                .semesters
                .iter()
                .position(|p| p.semester.id == sid)
                .ok_or(ProtocolError::InvalidCommand)?;
            let old = d.semesters[index].clone();
            let wanted = role(&c.source_role)?;
            let source = old
                .semester
                .tables
                .iter()
                .find(|t| t.role == wanted)
                .ok_or(ProtocolError::InvalidCommand)?
                .id;
            let next = if !c.enabled {
                old.disable_dual(source)
            } else if c.restore_previous {
                old.enable_dual(EnableDual::RestoreHistory)
            } else {
                let odd_id =
                    TableId::new(d.allocate_id().map_err(|_| ProtocolError::InvalidCommand)?)
                        .map_err(|_| ProtocolError::InvalidCommand)?;
                let even_id =
                    TableId::new(d.allocate_id().map_err(|_| ProtocolError::InvalidCommand)?)
                        .map_err(|_| ProtocolError::InvalidCommand)?;
                let mut odd_rows = Vec::new();
                let mut even_rows = Vec::new();
                for _ in &old.semester.tables[0].rows {
                    odd_rows.push(
                        RowId::new(d.allocate_id().map_err(|_| ProtocolError::InvalidCommand)?)
                            .map_err(|_| ProtocolError::InvalidCommand)?,
                    );
                    even_rows.push(
                        RowId::new(d.allocate_id().map_err(|_| ProtocolError::InvalidCommand)?)
                            .map_err(|_| ProtocolError::InvalidCommand)?,
                    );
                }
                old.enable_dual(EnableDual::Rebuild {
                    odd_id,
                    even_id,
                    odd_rows,
                    even_rows,
                })
            };
            d.semesters[index] = next.map_err(|_| ProtocolError::InvalidCommand)?;
        }
        "addRow" => {
            let c: AddRow = candidate(r.candidate)?;
            let sid = id(&c.semester_id, SemesterId::new)?;
            let tid = id(&c.table_id, TableId::new)?;
            let pi = d
                .semesters
                .iter()
                .position(|p| p.semester.id == sid)
                .ok_or(ProtocolError::InvalidCommand)?;
            let row_id = RowId::new(d.allocate_id().map_err(|_| ProtocolError::InvalidCommand)?)
                .map_err(|_| ProtocolError::InvalidCommand)?;
            let logical =
                LogicalRowKey::new(d.allocate_id().map_err(|_| ProtocolError::InvalidCommand)?)
                    .map_err(|_| ProtocolError::InvalidCommand)?;
            let table = d.semesters[pi]
                .semester
                .tables
                .iter()
                .find(|t| t.id == tid)
                .ok_or(ProtocolError::InvalidCommand)?;
            if table.rows.len() >= MAX_ROWS_PER_TABLE {
                return Err(ProtocolError::TooLarge);
            }
            let index = table.rows.len();
            let row = TimetableRow {
                id: row_id,
                logical_key: logical,
                group: DayGroup::Morning,
                kind: row_kind(&c.kind)?,
                range: MinuteRange::new(
                    Minute::new(c.begin).map_err(|_| ProtocolError::InvalidCommand)?,
                    Minute::new(c.end).map_err(|_| ProtocolError::InvalidCommand)?,
                )
                .map_err(|_| ProtocolError::InvalidCommand)?,
                enabled_days: [true, true, true, true, true, false, false],
                courses: std::array::from_fn(|_| None),
            };
            d = d
                .edit(
                    &DraftEdit::InsertRow {
                        semester_id: sid,
                        table_id: tid,
                        index,
                        row,
                    },
                    identity,
                )
                .map_err(|_| ProtocolError::InvalidCommand)?;
            if !c.name.is_empty() {
                let p = &mut d.semesters[pi];
                for row in p
                    .semester
                    .tables
                    .iter()
                    .flat_map(|table| &table.rows)
                    .filter(|row| row.logical_key == logical)
                {
                    p.row_details.push(RowDetails {
                        row_id: row.id,
                        name: c.name.clone(),
                    });
                }
            }
        }
        "saveCalendar" => {
            let c: SaveCalendar = candidate(r.candidate)?;
            d.calendar_mode = match c.calendar_mode.as_str() {
                "school" => CalendarMode::School,
                "public" => CalendarMode::Public,
                _ => return invalid(),
            };
            d.automatic_gaps = c.automatic_gaps;
            d.school_calendar = calendar_domain(c.school_calendar, &d.school_calendar)?;
            d.public_calendar = calendar_domain(c.public_calendar, &d.public_calendar)?;
        }
        _ => return invalid(),
    }
    d.update_id_high_water();
    d.validate_draft_structure()
        .map_err(|_| ProtocolError::InvalidCommand)?;
    Ok((expected, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adding_a_synchronized_dual_row_keeps_shared_timing_and_distinct_physical_ids() {
        let identity = fc_domain::identity::IdentityState::default();
        let apply = |document: &ScheduleDocument, action: &str, candidate: serde_json::Value| {
            let request = serde_json::json!({"schemaVersion":1,"expectedRevision":"0","action":action,"candidate":candidate});
            mutation(&serde_json::to_vec(&request).unwrap(), document, &identity)
                .unwrap()
                .1
        };
        let document = apply(
            &ScheduleDocument::default(),
            "createSemester",
            serde_json::json!({
                "classId":"1","name":"秋季","start":"2026-09-01","end":"2027-01-31","firstWeekMonday":"2026-08-31"
            }),
        );
        let semester_id = document.semesters[0].semester.id.get().to_string();
        let mut document = apply(
            &document,
            "setDualMode",
            serde_json::json!({
                "semesterId":semester_id,"enabled":true,"sourceRole":"single","restorePrevious":false
            }),
        );
        document.semesters[0].timing_sync = true;
        let table_id = document.semesters[0].semester.tables[0]
            .id
            .get()
            .to_string();
        let result = apply(
            &document,
            "addRow",
            serde_json::json!({
                "semesterId":semester_id,"tableId":table_id,"kind":"course","begin":480,"end":525,"name":"第一节"
            }),
        );
        let plan = &result.semesters[0];
        let odd = &plan.semester.tables[0].rows[0];
        let even = &plan.semester.tables[1].rows[0];
        assert_ne!(odd.id, even.id);
        assert_eq!(odd.logical_key, even.logical_key);
        assert_eq!(odd.range, even.range);
        assert_eq!(plan.row_details.len(), 2);
        assert!(result.id_high_water >= even.id.get());
        assert!(document.semesters[0]
            .semester
            .tables
            .iter()
            .all(|table| table.rows.is_empty()));
    }
}
