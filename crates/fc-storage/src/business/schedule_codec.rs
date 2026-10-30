//! Explicit bounded storage DTOs. Domain types remain independent of serde/SQL.
use super::codec::*;
use crate::StorageError;
use fc_domain::{ids::*, schedule::*, time::*};
use serde_json::{json, Value};
fn uid<T>(
    v: &Value,
    n: &str,
    make: impl FnOnce(u64) -> Result<T, InvalidId>,
) -> Result<T, StorageError> {
    make(number(v, n)?).map_err(|_| StorageError::Database)
}
fn row(r: &TimetableRow) -> Value {
    let group = match r.group {
        DayGroup::Morning => json!({"kind":"morning","number":null}),
        DayGroup::Afternoon => json!({"kind":"afternoon","number":null}),
        DayGroup::Other(n) => json!({"kind":"other","number":n}),
    };
    let kind = match r.kind {
        RowKind::Course => "course",
        RowKind::Activity => "activity",
        RowKind::Rest(RestPurpose::Recess) => "recess",
        RowKind::Rest(RestPurpose::Lunch) => "lunch",
        RowKind::Rest(RestPurpose::Other) => "rest-other",
        RowKind::Rest(RestPurpose::Unknown) => "rest-unknown",
    };
    let courses=r.courses.iter().map(|cell|cell.as_ref().map(|c|json!({"subject":c.subject_id.get(),"binding":match c.teacher{TeacherBinding::Explicit(_)=>"explicit",TeacherBinding::Default=>"default",TeacherBinding::Unassigned=>"unassigned"},"teacher":match c.teacher{TeacherBinding::Explicit(id)=>Some(id.get()),_=>None}}))).collect::<Vec<_>>();
    json!({"id":r.id.get(),"logical":r.logical_key.get(),"group":group,"kind":kind,"begin":r.range.begin.get(),"end":r.range.end.get(),"days":r.enabled_days,"courses":courses})
}
fn read_row(v: &Value) -> Result<TimetableRow, StorageError> {
    object_fields(
        v,
        &[
            "id", "logical", "group", "kind", "begin", "end", "days", "courses",
        ],
    )?;
    object_fields(&v["group"], &["kind", "number"])?;
    let group = match v["group"]["kind"].as_str() {
        Some("morning") if v["group"]["number"].is_null() => DayGroup::Morning,
        Some("afternoon") if v["group"]["number"].is_null() => DayGroup::Afternoon,
        Some("other") => DayGroup::Other(
            u16::try_from(number(&v["group"], "number")?).map_err(|_| StorageError::Database)?,
        ),
        _ => return Err(StorageError::Database),
    };
    let kind = match v["kind"].as_str() {
        Some("course") => RowKind::Course,
        Some("activity") => RowKind::Activity,
        Some("recess") => RowKind::Rest(RestPurpose::Recess),
        Some("lunch") => RowKind::Rest(RestPurpose::Lunch),
        Some("rest-other") => RowKind::Rest(RestPurpose::Other),
        Some("rest-unknown") => RowKind::Rest(RestPurpose::Unknown),
        _ => return Err(StorageError::Database),
    };
    let days = array(v, "days")?;
    let cells = array(v, "courses")?;
    if days.len() != 7 || cells.len() != 7 {
        return Err(StorageError::Database);
    }
    let mut enabled_days = [false; 7];
    let mut courses: [Option<CourseCell>; 7] = std::array::from_fn(|_| None);
    for i in 0..7 {
        enabled_days[i] = days[i].as_bool().ok_or(StorageError::Database)?;
        if !cells[i].is_null() {
            let c = &cells[i];
            object_fields(c, &["subject", "binding", "teacher"])?;
            let teacher = match c["binding"].as_str() {
                Some("explicit") => TeacherBinding::Explicit(uid(c, "teacher", PrincipalId::new)?),
                Some("default") if c["teacher"].is_null() => TeacherBinding::Default,
                Some("unassigned") if c["teacher"].is_null() => TeacherBinding::Unassigned,
                _ => return Err(StorageError::Database),
            };
            courses[i] = Some(CourseCell {
                subject_id: uid(c, "subject", SubjectId::new)?,
                teacher,
            });
        }
    }
    let minute = |n| {
        Minute::new(u16::try_from(number(v, n)?).map_err(|_| StorageError::Database)?)
            .map_err(|_| StorageError::Database)
    };
    Ok(TimetableRow {
        id: uid(v, "id", RowId::new)?,
        logical_key: uid(v, "logical", LogicalRowKey::new)?,
        group,
        kind,
        range: MinuteRange::new(minute("begin")?, minute("end")?)
            .map_err(|_| StorageError::Database)?,
        enabled_days,
        courses,
    })
}
fn table(t: &WeekTable) -> Value {
    json!({"id":t.id.get(),"role":match t.role{TableRole::Single=>"single",TableRole::Odd=>"odd",TableRole::Even=>"even"},"rows":t.rows.iter().map(row).collect::<Vec<_>>()})
}
fn read_table(t: &Value) -> Result<WeekTable, StorageError> {
    object_fields(t, &["id", "role", "rows"])?;
    Ok(WeekTable {
        id: uid(t, "id", TableId::new)?,
        role: match t["role"].as_str() {
            Some("single") => TableRole::Single,
            Some("odd") => TableRole::Odd,
            Some("even") => TableRole::Even,
            _ => return Err(StorageError::Database),
        },
        rows: array(t, "rows")?
            .iter()
            .map(read_row)
            .collect::<Result<_, StorageError>>()?,
    })
}
fn semester(s: &Semester) -> Value {
    json!({"id":s.id.get(),"class":s.class_id.get(),"revision":s.revision.get(),"enabled":s.enabled,"start":date(s.start),"end":date(s.end),"anchor":date(s.first_week_monday),"tables":s.tables.iter().map(table).collect::<Vec<_>>()})
}
fn read_semester(v: &Value) -> Result<Semester, StorageError> {
    object_fields(
        v,
        &[
            "id", "class", "revision", "enabled", "start", "end", "anchor", "tables",
        ],
    )?;
    let tables = array(v, "tables")?
        .iter()
        .map(read_table)
        .collect::<Result<_, StorageError>>()?;
    Ok(Semester {
        id: uid(v, "id", SemesterId::new)?,
        class_id: uid(v, "class", ClassId::new)?,
        revision: Revision::new(number(v, "revision")?),
        enabled: boolean(v, "enabled")?,
        start: read_date(&v["start"])?,
        end: read_date(&v["end"])?,
        first_week_monday: read_date(&v["anchor"])?,
        tables,
    })
}
fn calendar(c: &CalendarConfig) -> Value {
    json!({"mode":match c.mode{CalendarMode::School=>"school",CalendarMode::Public=>"public"},"mode_revision":c.mode_revision.get(),"applied_revision":c.applied_revision.get(),"mapping_revision":c.mapping_revision.get(),"coverage":c.school_coverage.map(|(a,b)|vec![date(a),date(b)]),"years":c.public_verified_years,"entries":c.entries.iter().map(|e|json!({"term":e.actual_semester_id.get(),"date":date(e.date),"mapping":match e.mapping{DayMapping::Rest=>"rest",DayMapping::Unconfirmed=>"unconfirmed",DayMapping::SourceDate(_)=>"source"},"source":match e.mapping{DayMapping::SourceDate(d)=>Some(date(d)),_=>None}})).collect::<Vec<_>>()})
}
fn read_calendar(v: &Value) -> Result<CalendarConfig, StorageError> {
    object_fields(
        v,
        &[
            "mode",
            "mode_revision",
            "applied_revision",
            "mapping_revision",
            "coverage",
            "years",
            "entries",
        ],
    )?;
    let school_coverage = if v["coverage"].is_null() {
        None
    } else {
        let a = array(v, "coverage")?;
        if a.len() != 2 {
            return Err(StorageError::Database);
        }
        Some((read_date(&a[0])?, read_date(&a[1])?))
    };
    let entries = array(v, "entries")?
        .iter()
        .map(|e| {
            object_fields(e, &["term", "date", "mapping", "source"])?;
            let mapping = match e["mapping"].as_str() {
                Some("rest") if e["source"].is_null() => DayMapping::Rest,
                Some("unconfirmed") if e["source"].is_null() => DayMapping::Unconfirmed,
                Some("source") => DayMapping::SourceDate(read_date(&e["source"])?),
                _ => return Err(StorageError::Database),
            };
            Ok(CalendarEntry {
                actual_semester_id: uid(e, "term", SemesterId::new)?,
                date: read_date(&e["date"])?,
                mapping,
            })
        })
        .collect::<Result<_, StorageError>>()?;
    Ok(CalendarConfig {
        mode: match v["mode"].as_str() {
            Some("school") => CalendarMode::School,
            Some("public") => CalendarMode::Public,
            _ => return Err(StorageError::Database),
        },
        mode_revision: Revision::new(number(v, "mode_revision")?),
        applied_revision: Revision::new(number(v, "applied_revision")?),
        mapping_revision: Revision::new(number(v, "mapping_revision")?),
        school_coverage,
        public_verified_years: array(v, "years")?
            .iter()
            .map(|n| {
                i32::try_from(n.as_i64().ok_or(StorageError::Database)?)
                    .map_err(|_| StorageError::Database)
            })
            .collect::<Result<_, _>>()?,
        entries,
    })
}
use fc_domain::schedule_document::*;
fn rows(items: &[RowDetails]) -> Value {
    json!(items
        .iter()
        .map(|r| json!({"row":r.row_id.get(),"name":r.name}))
        .collect::<Vec<_>>())
}
fn cells(items: &[CellDetails]) -> Value {
    json!(items
        .iter()
        .map(|c| json!({"row":c.row_id.get(),"day":c.day.index(),"room":c.room,"note":c.note}))
        .collect::<Vec<_>>())
}
fn read_rows(v: &Value) -> Result<Vec<RowDetails>, StorageError> {
    array(v, "row_details")?
        .iter()
        .map(|r| {
            object_fields(r, &["row", "name"])?;
            Ok(RowDetails {
                row_id: uid(r, "row", RowId::new)?,
                name: text(r, "name")?,
            })
        })
        .collect()
}
fn read_cells(v: &Value) -> Result<Vec<CellDetails>, StorageError> {
    array(v, "cell_details")?
        .iter()
        .map(|c| {
            object_fields(c, &["row", "day", "room", "note"])?;
            Ok(CellDetails {
                row_id: uid(c, "row", RowId::new)?,
                day: Weekday::from_monday_index(
                    u8::try_from(number(c, "day")?).map_err(|_| StorageError::Database)?,
                )
                .ok_or(StorageError::Database)?,
                room: text(c, "room")?,
                note: text(c, "note")?,
            })
        })
        .collect()
}

pub(super) fn document(d: &ScheduleDocument) -> Value {
    json!({"format":1,"revision":d.revision.get(),"id_high_water":d.id_high_water,"semesters":d.semesters.iter().map(|p|json!({"semester":semester(&p.semester),"name":p.name,"timing_sync":p.timing_sync,"dual_history":p.dual_history.as_ref().map(|h|json!({"tables":h.tables.iter().map(table).collect::<Vec<_>>(),"row_details":rows(&h.row_details),"cell_details":cells(&h.cell_details)})),"row_details":p.row_details.iter().map(|r|json!({"row":r.row_id.get(),"name":r.name})).collect::<Vec<_>>(),"cell_details":p.cell_details.iter().map(|c|json!({"row":c.row_id.get(),"day":c.day.index(),"room":c.room,"note":c.note})).collect::<Vec<_>>()})).collect::<Vec<_>>(),"calendar_mode":match d.calendar_mode{CalendarMode::School=>"school",CalendarMode::Public=>"public"},"calendar_mode_revision":d.calendar_mode_revision.get(),"school_calendar":calendar(&d.school_calendar),"public_calendar":calendar(&d.public_calendar),"public_calendar_data":super::calendar_codec::public_data(&d.public_calendar_data),"automatic_gaps":d.automatic_gaps,"rules_revision":d.rules_revision.get()})
}
pub(super) fn read_document(v: &Value) -> Result<ScheduleDocument, StorageError> {
    let legacy = !v
        .as_object()
        .ok_or(StorageError::Database)?
        .contains_key("public_calendar_data");
    let mut fields = vec![
        "format",
        "revision",
        "id_high_water",
        "semesters",
        "calendar_mode",
        "calendar_mode_revision",
        "school_calendar",
        "public_calendar",
        "automatic_gaps",
        "rules_revision",
    ];
    if !legacy {
        fields.push("public_calendar_data");
    }
    object_fields(v, &fields)?;
    if number(v, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let semesters = array(v, "semesters")?
        .iter()
        .map(|p| {
            object_fields(
                p,
                &[
                    "semester",
                    "name",
                    "timing_sync",
                    "dual_history",
                    "row_details",
                    "cell_details",
                ],
            )?;
            let dual_history = if p["dual_history"].is_null() {
                None
            } else {
                let h = &p["dual_history"];
                object_fields(h, &["tables", "row_details", "cell_details"])?;
                Some(DualTableHistory {
                    tables: array(h, "tables")?
                        .iter()
                        .map(read_table)
                        .collect::<Result<_, StorageError>>()?,
                    row_details: read_rows(h)?,
                    cell_details: read_cells(h)?,
                })
            };
            let row_details = read_rows(p)?;
            let cell_details = read_cells(p)?;
            Ok(SemesterPlan {
                semester: read_semester(&p["semester"])?,
                name: text(p, "name")?,
                timing_sync: boolean(p, "timing_sync")?,
                dual_history,
                row_details,
                cell_details,
            })
        })
        .collect::<Result<_, StorageError>>()?;
    Ok(ScheduleDocument {
        revision: Revision::new(number(v, "revision")?),
        id_high_water: number(v, "id_high_water")?,
        semesters,
        calendar_mode: match v["calendar_mode"].as_str() {
            Some("school") => CalendarMode::School,
            Some("public") => CalendarMode::Public,
            _ => return Err(StorageError::Database),
        },
        calendar_mode_revision: Revision::new(number(v, "calendar_mode_revision")?),
        school_calendar: read_calendar(&v["school_calendar"])?,
        public_calendar: read_calendar(&v["public_calendar"])?,
        public_calendar_data: if legacy {
            Default::default()
        } else {
            super::calendar_codec::read_public_data(&v["public_calendar_data"])?
        },
        automatic_gaps: boolean(v, "automatic_gaps")?,
        rules_revision: Revision::new(number(v, "rules_revision")?),
    })
}
