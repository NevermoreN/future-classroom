//! Persistent timetable aggregate and atomic native-grid draft operations.
//! Serialization belongs to the storage/protocol adapters, never this crate.
use crate::{
    auth::ActorContext,
    identity::{IdentityError, IdentityState},
    ids::*,
    schedule::*,
    time::*,
};
use std::collections::{HashMap, HashSet};

pub const MAX_SEMESTERS: usize = 32;
pub const MAX_ROWS_PER_TABLE: usize = 128;
pub const MAX_TOTAL_ROWS: usize = 4096;
pub const MAX_CALENDAR_ENTRIES: usize = 4096;
pub const MAX_VERIFIED_YEARS: usize = 32;
pub const MAX_CELL_DETAILS: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowDetails {
    pub row_id: RowId,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellDetails {
    pub row_id: RowId,
    pub day: Weekday,
    pub room: String,
    pub note: String,
}
/// Only the most recently disabled pair is retained. Restoring consumes this
/// history; rebuilding explicitly replaces it. No hidden unbounded undo log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DualTableHistory {
    pub tables: Vec<WeekTable>,
    pub row_details: Vec<RowDetails>,
    pub cell_details: Vec<CellDetails>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemesterPlan {
    pub semester: Semester,
    pub name: String,
    pub timing_sync: bool,
    pub dual_history: Option<DualTableHistory>,
    pub row_details: Vec<RowDetails>,
    pub cell_details: Vec<CellDetails>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleDocument {
    pub revision: Revision,
    /// Published identities remain retired after deletion and process restart.
    pub id_high_water: u64,
    pub semesters: Vec<SemesterPlan>,
    pub calendar_mode: CalendarMode,
    pub calendar_mode_revision: Revision,
    pub school_calendar: CalendarConfig,
    pub public_calendar: CalendarConfig,
    /// Adopted public provenance; non-active candidates live in their own repository.
    pub public_calendar_data: crate::calendar::PublicCalendarData,
    pub automatic_gaps: bool,
    pub rules_revision: Revision,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ErrorLocation {
    pub semester_id: Option<SemesterId>,
    pub table_id: Option<TableId>,
    pub row_id: Option<RowId>,
    pub day: Option<Weekday>,
    pub calendar_mode: Option<CalendarMode>,
    pub date: Option<LocalDate>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentErrorKind {
    Schedule(ScheduleError),
    Identity(IdentityError),
    Capacity,
    InvalidText,
    InvalidReference,
    InvalidHistory,
    InvalidMetadata,
    InvalidCalendar,
    MappingCycle,
    RevisionConflict { current: Revision },
    VersionOverflow,
    NotFound,
    InvalidDraft,
    RetiredIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleDocumentError {
    pub kind: DocumentErrorKind,
    pub location: ErrorLocation,
}
impl ScheduleDocumentError {
    fn new(kind: DocumentErrorKind) -> Self {
        Self {
            kind,
            location: ErrorLocation::default(),
        }
    }
}
impl Default for ScheduleDocument {
    fn default() -> Self {
        let calendar = |mode| CalendarConfig {
            mode,
            mode_revision: Revision::default(),
            applied_revision: Revision::default(),
            mapping_revision: Revision::default(),
            school_coverage: None,
            public_verified_years: Vec::new(),
            entries: Vec::new(),
        };
        Self {
            revision: Revision::default(),
            id_high_water: 0,
            semesters: Vec::new(),
            calendar_mode: CalendarMode::Public,
            calendar_mode_revision: Revision::default(),
            school_calendar: calendar(CalendarMode::School),
            public_calendar: calendar(CalendarMode::Public),
            public_calendar_data: crate::calendar::PublicCalendarData::default(),
            automatic_gaps: true,
            rules_revision: Revision::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnableDual {
    RestoreHistory,
    Rebuild {
        odd_id: TableId,
        even_id: TableId,
        odd_rows: Vec<RowId>,
        even_rows: Vec<RowId>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftEdit {
    SetCell {
        semester_id: SemesterId,
        table_id: TableId,
        row_id: RowId,
        day: Weekday,
        cell: Option<CourseCell>,
    },
    SetRow {
        semester_id: SemesterId,
        table_id: TableId,
        row: TimetableRow,
    },
    InsertRow {
        semester_id: SemesterId,
        table_id: TableId,
        index: usize,
        row: TimetableRow,
    },
    RemoveRow {
        semester_id: SemesterId,
        table_id: TableId,
        row_id: RowId,
    },
    MoveRow {
        semester_id: SemesterId,
        table_id: TableId,
        row_id: RowId,
        index: usize,
    },
    CopyDay {
        semester_id: SemesterId,
        table_id: TableId,
        from: Weekday,
        to: Weekday,
    },
}

fn error(kind: DocumentErrorKind, location: ErrorLocation) -> ScheduleDocumentError {
    ScheduleDocumentError { kind, location }
}
fn next(revision: Revision) -> Result<Revision, ScheduleDocumentError> {
    revision
        .next()
        .map_err(|_| ScheduleDocumentError::new(DocumentErrorKind::VersionOverflow))
}
fn text_valid(value: &str, max: usize, multiline: bool, required: bool) -> bool {
    value.len() <= max * 4
        && value.chars().count() <= max
        && (!required || !value.trim().is_empty())
        && !value
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')))
}
fn located_schedule(value: ScheduleError, semester: SemesterId) -> ScheduleDocumentError {
    let mut location = ErrorLocation {
        semester_id: Some(semester),
        ..ErrorLocation::default()
    };
    if let ScheduleError::Overlap {
        table_id,
        day,
        second,
        ..
    } = &value
    {
        location.table_id = Some(*table_id);
        location.row_id = Some(*second);
        location.day = Some(*day);
    }
    error(DocumentErrorKind::Schedule(value), location)
}
fn metadata(
    tables: &[WeekTable],
    rows: &[RowDetails],
    cells: &[CellDetails],
    location: ErrorLocation,
) -> Result<(), ScheduleDocumentError> {
    let known: HashMap<_, _> = tables
        .iter()
        .flat_map(|t| t.rows.iter().map(move |r| (r.id, (t.id, r))))
        .collect();
    let mut seen = HashSet::new();
    for row in rows {
        let mut at = location;
        at.row_id = Some(row.row_id);
        if !known.contains_key(&row.row_id) || !seen.insert(row.row_id) {
            return Err(error(DocumentErrorKind::InvalidMetadata, at));
        }
        if !text_valid(&row.name, 64, false, false) {
            return Err(error(DocumentErrorKind::InvalidText, at));
        }
    }
    let mut seen = HashSet::new();
    for cell in cells {
        let mut at = location;
        at.row_id = Some(cell.row_id);
        at.day = Some(cell.day);
        let Some((table, row)) = known.get(&cell.row_id) else {
            return Err(error(DocumentErrorKind::InvalidMetadata, at));
        };
        at.table_id = Some(*table);
        if row.kind != RowKind::Course
            || row.courses[cell.day.index()].is_none()
            || !seen.insert((cell.row_id, cell.day))
        {
            return Err(error(DocumentErrorKind::InvalidMetadata, at));
        }
        if !text_valid(&cell.room, 64, false, false) || !text_valid(&cell.note, 512, true, false) {
            return Err(error(DocumentErrorKind::InvalidText, at));
        }
    }
    Ok(())
}
impl ScheduleDocument {
    /// Only metadata and capacity: unfinished timing, temporary overlaps and
    /// calendar drafts do not stop a teacher fixing a row name or cell note.
    pub fn validate_draft_metadata(&self) -> Result<(), ScheduleDocumentError> {
        self.capacity()?;
        for plan in &self.semesters {
            let at = ErrorLocation {
                semester_id: Some(plan.semester.id),
                ..ErrorLocation::default()
            };
            metadata(
                &plan.semester.tables,
                &plan.row_details,
                &plan.cell_details,
                at,
            )?;
            if let Some(history) = &plan.dual_history {
                metadata(
                    &history.tables,
                    &history.row_details,
                    &history.cell_details,
                    at,
                )?;
            }
        }
        Ok(())
    }
    pub fn maximum_id(&self) -> u64 {
        let mut maximum = 0;
        for plan in &self.semesters {
            maximum = maximum.max(plan.semester.id.get());
            for collection in std::iter::once(&plan.semester.tables)
                .chain(plan.dual_history.iter().map(|h| &h.tables))
            {
                for table in collection {
                    maximum = maximum.max(table.id.get());
                    for row in &table.rows {
                        maximum = maximum.max(row.id.get()).max(row.logical_key.get());
                    }
                }
            }
        }
        maximum
    }
    pub fn update_id_high_water(&mut self) {
        self.id_high_water = self.id_high_water.max(self.maximum_id());
    }
    /// Reserve one native draft identity, checked before any mutation. The writer
    /// persists only IDs present in the candidate plus the old published watermark.
    pub fn allocate_id(&mut self) -> Result<u64, ScheduleDocumentError> {
        let next = self
            .id_high_water
            .max(self.maximum_id())
            .checked_add(1)
            .ok_or_else(|| ScheduleDocumentError::new(DocumentErrorKind::VersionOverflow))?;
        self.id_high_water = next;
        Ok(next)
    }
    /// Complete draft check for previews: revision fields are untrusted draft data,
    /// while structure, ranges, mappings and references obey the normal rules.
    pub fn validate_draft_structure(&self) -> Result<(), ScheduleDocumentError> {
        self.capacity()?;
        let mut draft = self.clone();
        draft.revision = Revision::default();
        draft.validate_structure()
    }
    pub fn validate_draft(&self, identity: &IdentityState) -> Result<(), ScheduleDocumentError> {
        self.capacity()?;
        let mut draft = self.clone();
        draft.revision = Revision::default();
        draft.validate(identity)
    }
    fn capacity(&self) -> Result<(), ScheduleDocumentError> {
        if self.semesters.len() > MAX_SEMESTERS {
            return Err(ScheduleDocumentError::new(DocumentErrorKind::Capacity));
        }
        let mut total_rows = 0usize;
        let mut total_cells = 0usize;
        for plan in &self.semesters {
            let at = ErrorLocation {
                semester_id: Some(plan.semester.id),
                ..ErrorLocation::default()
            };
            if plan.semester.tables.len() > 2
                || plan.row_details.len() > MAX_ROWS_PER_TABLE * 2
                || plan.cell_details.len() > MAX_CELL_DETAILS
            {
                return Err(error(DocumentErrorKind::Capacity, at));
            }
            if !text_valid(&plan.name, 128, false, false) {
                return Err(error(DocumentErrorKind::InvalidText, at));
            }
            for tables in std::iter::once(&plan.semester.tables)
                .chain(plan.dual_history.iter().map(|h| &h.tables))
            {
                if tables.len() > 2 {
                    return Err(error(DocumentErrorKind::Capacity, at));
                }
                for table in tables {
                    if table.rows.len() > MAX_ROWS_PER_TABLE {
                        return Err(error(
                            DocumentErrorKind::Capacity,
                            ErrorLocation {
                                table_id: Some(table.id),
                                ..at
                            },
                        ));
                    }
                    total_rows = total_rows
                        .checked_add(table.rows.len())
                        .ok_or_else(|| error(DocumentErrorKind::Capacity, at))?;
                }
            }
            if let Some(h) = &plan.dual_history {
                if h.row_details.len() > MAX_ROWS_PER_TABLE * 2
                    || h.cell_details.len() > MAX_CELL_DETAILS
                {
                    return Err(error(DocumentErrorKind::Capacity, at));
                }
            }
            // Reject oversized untrusted strings before any draft/transition clone.
            for (rows, cells) in std::iter::once((&plan.row_details, &plan.cell_details)).chain(
                plan.dual_history
                    .iter()
                    .map(|h| (&h.row_details, &h.cell_details)),
            ) {
                for row in rows {
                    if !text_valid(&row.name, 64, false, false) {
                        return Err(error(
                            DocumentErrorKind::InvalidText,
                            ErrorLocation {
                                row_id: Some(row.row_id),
                                ..at
                            },
                        ));
                    }
                }
                for cell in cells {
                    if !text_valid(&cell.room, 64, false, false)
                        || !text_valid(&cell.note, 512, true, false)
                    {
                        return Err(error(
                            DocumentErrorKind::InvalidText,
                            ErrorLocation {
                                row_id: Some(cell.row_id),
                                day: Some(cell.day),
                                ..at
                            },
                        ));
                    }
                }
            }
            total_cells = total_cells
                .checked_add(plan.cell_details.len())
                .and_then(|n| {
                    n.checked_add(
                        plan.dual_history
                            .as_ref()
                            .map_or(0, |h| h.cell_details.len()),
                    )
                })
                .ok_or_else(|| error(DocumentErrorKind::Capacity, at))?;
        }
        if total_rows > MAX_TOTAL_ROWS || total_cells > MAX_CELL_DETAILS {
            return Err(ScheduleDocumentError::new(DocumentErrorKind::Capacity));
        }
        for calendar in [&self.school_calendar, &self.public_calendar] {
            if calendar.entries.len() > MAX_CALENDAR_ENTRIES
                || calendar.public_verified_years.len() > MAX_VERIFIED_YEARS
            {
                return Err(error(
                    DocumentErrorKind::Capacity,
                    ErrorLocation {
                        calendar_mode: Some(calendar.mode),
                        ..ErrorLocation::default()
                    },
                ));
            }
        }
        Ok(())
    }
    /// Pure syntax/structure check, including retained histories. Suitable for old
    /// immutable command receipts whose identity references may since have changed.
    pub fn validate_structure(&self) -> Result<(), ScheduleDocumentError> {
        self.capacity()?;
        self.public_calendar_data
            .validate()
            .map_err(|_| ScheduleDocumentError::new(DocumentErrorKind::InvalidCalendar))?;
        if self.revision.get() > 0 && self.id_high_water < self.maximum_id() {
            return Err(ScheduleDocumentError::new(DocumentErrorKind::InvalidDraft));
        }
        if self.school_calendar.mode != CalendarMode::School
            || self.public_calendar.mode != CalendarMode::Public
        {
            return Err(ScheduleDocumentError::new(
                DocumentErrorKind::InvalidCalendar,
            ));
        }
        if self.revision.get() > 0
            && (self.calendar_mode_revision.get() == 0 || self.rules_revision.get() == 0)
        {
            return Err(ScheduleDocumentError::new(DocumentErrorKind::InvalidDraft));
        }
        let mut term_ids = HashSet::new();
        let mut table_ids = HashMap::new();
        let mut row_ids = HashMap::new();
        for plan in &self.semesters {
            let term = &plan.semester;
            let at = ErrorLocation {
                semester_id: Some(term.id),
                ..ErrorLocation::default()
            };
            if !text_valid(&plan.name, 128, false, true) {
                return Err(error(DocumentErrorKind::InvalidText, at));
            }
            if !term_ids.insert(term.id) || (self.revision.get() > 0 && term.revision.get() == 0) {
                return Err(error(DocumentErrorKind::InvalidDraft, at));
            }
            term.validate().map_err(|e| located_schedule(e, term.id))?;
            if plan.timing_sync && term.tables.len() == 2 {
                let source = &term.tables[0];
                let target = &term.tables[1];
                if source.rows.len() != target.rows.len() {
                    return Err(error(
                        DocumentErrorKind::InvalidDraft,
                        ErrorLocation {
                            table_id: Some(target.id),
                            ..at
                        },
                    ));
                }
                for row in &source.rows {
                    let aligned = target
                        .rows
                        .iter()
                        .find(|r| r.logical_key == row.logical_key)
                        .is_some_and(|r| {
                            r.range == row.range
                                && r.group == row.group
                                && r.kind == row.kind
                                && r.enabled_days == row.enabled_days
                        });
                    if !aligned {
                        return Err(error(
                            DocumentErrorKind::InvalidDraft,
                            ErrorLocation {
                                table_id: Some(target.id),
                                row_id: target
                                    .rows
                                    .iter()
                                    .find(|r| r.logical_key == row.logical_key)
                                    .map(|r| r.id),
                                ..at
                            },
                        ));
                    }
                }
            }
            metadata(&term.tables, &plan.row_details, &plan.cell_details, at)?;
            if let Some(history) = &plan.dual_history {
                if term.tables.len() != 1
                    || !history.tables.iter().any(|t| t.id == term.tables[0].id)
                {
                    return Err(error(DocumentErrorKind::InvalidHistory, at));
                }
                let mut archived = term.clone();
                archived.tables = history.tables.clone();
                if archived.tables.len() != 2 {
                    return Err(error(DocumentErrorKind::InvalidHistory, at));
                }
                archived
                    .validate()
                    .map_err(|e| located_schedule(e, term.id))?;
                metadata(
                    &history.tables,
                    &history.row_details,
                    &history.cell_details,
                    at,
                )?;
            }
            for tables in
                std::iter::once(&term.tables).chain(plan.dual_history.iter().map(|h| &h.tables))
            {
                for table in tables {
                    if table_ids
                        .insert(table.id, term.id)
                        .is_some_and(|old| old != term.id)
                    {
                        return Err(error(
                            DocumentErrorKind::Schedule(ScheduleError::DuplicateIdentity),
                            ErrorLocation {
                                table_id: Some(table.id),
                                ..at
                            },
                        ));
                    }
                    for row in &table.rows {
                        if row_ids
                            .insert(row.id, (term.id, table.id))
                            .is_some_and(|old| old != (term.id, table.id))
                        {
                            return Err(error(
                                DocumentErrorKind::Schedule(ScheduleError::DuplicateIdentity),
                                ErrorLocation {
                                    table_id: Some(table.id),
                                    row_id: Some(row.id),
                                    ..at
                                },
                            ));
                        }
                    }
                }
            }
        }
        for (index, a) in self.semesters.iter().enumerate() {
            for b in &self.semesters[index + 1..] {
                let (a, b) = (&a.semester, &b.semester);
                if a.enabled
                    && b.enabled
                    && a.class_id == b.class_id
                    && a.start <= b.end
                    && b.start <= a.end
                {
                    return Err(located_schedule(ScheduleError::AmbiguousTerm, b.id));
                }
            }
        }
        self.validate_calendar(&self.school_calendar)?;
        self.validate_calendar(&self.public_calendar)
    }
    fn validate_calendar(&self, calendar: &CalendarConfig) -> Result<(), ScheduleDocumentError> {
        let at = ErrorLocation {
            calendar_mode: Some(calendar.mode),
            ..ErrorLocation::default()
        };
        if self.revision.get() > 0
            && (calendar.mode_revision != self.calendar_mode_revision
                || calendar.applied_revision.get() == 0
                || calendar.mapping_revision.get() == 0)
        {
            return Err(error(DocumentErrorKind::InvalidCalendar, at));
        }
        if calendar.mode == CalendarMode::School && !calendar.public_verified_years.is_empty()
            || calendar.mode == CalendarMode::Public && calendar.school_coverage.is_some()
            || calendar
                .school_coverage
                .is_some_and(|(start, end)| start > end)
        {
            return Err(error(DocumentErrorKind::InvalidCalendar, at));
        }
        let mut years = HashSet::new();
        for year in &calendar.public_verified_years {
            if !(1..=9999).contains(year) || !years.insert(*year) {
                return Err(error(DocumentErrorKind::InvalidCalendar, at));
            }
        }
        let mut entries = HashSet::new();
        let mut edges = HashMap::new();
        for entry in &calendar.entries {
            let at = ErrorLocation {
                semester_id: Some(entry.actual_semester_id),
                date: Some(entry.date),
                ..at
            };
            let actual = self
                .semesters
                .iter()
                .find(|p| p.semester.id == entry.actual_semester_id)
                .map(|p| &p.semester)
                .ok_or_else(|| error(DocumentErrorKind::InvalidCalendar, at))?;
            if entry.date < actual.start
                || entry.date > actual.end
                || !entries.insert((actual.id, entry.date))
            {
                return Err(error(DocumentErrorKind::InvalidCalendar, at));
            }
            if calendar.mode == CalendarMode::School
                && !calendar
                    .school_coverage
                    .is_some_and(|(start, end)| entry.date >= start && entry.date <= end)
            {
                return Err(error(DocumentErrorKind::InvalidCalendar, at));
            }
            if let DayMapping::SourceDate(source) = entry.mapping {
                if source == entry.date {
                    return Err(error(DocumentErrorKind::InvalidCalendar, at));
                }
                let targets: Vec<_> = self
                    .semesters
                    .iter()
                    .filter(|p| {
                        p.semester.class_id == actual.class_id && p.semester.contains(source)
                    })
                    .collect();
                if targets.len() != 1 {
                    return Err(error(DocumentErrorKind::InvalidReference, at));
                }
                edges.insert((actual.id, entry.date), (targets[0].semester.id, source));
            }
        }
        let mut complete = HashSet::new();
        for start in edges.keys() {
            let mut visiting = HashSet::new();
            let mut node = *start;
            while !complete.contains(&node) {
                if !visiting.insert(node) {
                    return Err(error(
                        DocumentErrorKind::MappingCycle,
                        ErrorLocation {
                            semester_id: Some(node.0),
                            date: Some(node.1),
                            ..at
                        },
                    ));
                }
                let Some(target) = edges.get(&node) else {
                    break;
                };
                node = *target;
            }
            complete.extend(visiting);
        }
        Ok(())
    }
    /// Validate the current aggregate against current, same-transaction identity.
    /// Explicit invalid teachers never silently resolve to a different teacher.
    pub fn validate(&self, identity: &IdentityState) -> Result<(), ScheduleDocumentError> {
        self.validate_structure()?;
        identity
            .validate()
            .map_err(|e| ScheduleDocumentError::new(DocumentErrorKind::Identity(e)))?;
        for plan in &self.semesters {
            let term = &plan.semester;
            let at = ErrorLocation {
                semester_id: Some(term.id),
                ..ErrorLocation::default()
            };
            if !identity
                .classes
                .iter()
                .any(|c| c.id == term.class_id && c.enabled)
            {
                return Err(error(DocumentErrorKind::InvalidReference, at));
            }
            for tables in
                std::iter::once(&term.tables).chain(plan.dual_history.iter().map(|h| &h.tables))
            {
                for table in tables {
                    for row in &table.rows {
                        for (day, cell) in row.courses.iter().enumerate() {
                            if let Some(cell) = cell {
                                self.validate_cell(
                                    identity,
                                    term.class_id,
                                    cell,
                                    ErrorLocation {
                                        table_id: Some(table.id),
                                        row_id: Some(row.id),
                                        day: Weekday::from_monday_index(day as u8),
                                        ..at
                                    },
                                )?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn validate_cell(
        &self,
        identity: &IdentityState,
        class: ClassId,
        cell: &CourseCell,
        at: ErrorLocation,
    ) -> Result<(), ScheduleDocumentError> {
        if !identity
            .subjects
            .iter()
            .any(|s| s.id == cell.subject_id && s.enabled)
            || !identity.classes.iter().any(|c| c.id == class && c.enabled)
        {
            return Err(error(DocumentErrorKind::InvalidReference, at));
        }
        let valid = match cell.teacher {
            TeacherBinding::Explicit(id) => identity.is_teacher(id, class, cell.subject_id),
            TeacherBinding::Default => identity
                .resolve_teacher(class, cell.subject_id, None)
                .is_ok(),
            TeacherBinding::Unassigned => true,
        };
        if valid {
            Ok(())
        } else {
            Err(error(DocumentErrorKind::InvalidReference, at))
        }
    }
    pub fn active_calendar(&self) -> &CalendarConfig {
        match self.calendar_mode {
            CalendarMode::School => &self.school_calendar,
            CalendarMode::Public => &self.public_calendar,
        }
    }
    pub fn resolve_day(
        &self,
        class: ClassId,
        date: LocalDate,
    ) -> Result<AcademicDay, ScheduleDocumentError> {
        self.validate_structure()?;
        let semesters: Vec<_> = self.semesters.iter().map(|p| p.semester.clone()).collect();
        let mut calendar = self.active_calendar().clone();
        if self.calendar_mode == CalendarMode::Public {
            // Legacy year flags carry no provider provenance and cannot verify a year.
            calendar.public_verified_years = self.public_calendar_data.verified_years();
            if let Some(actual) = semesters
                .iter()
                .find(|s| s.class_id == class && s.contains(date))
            {
                let manual = calendar
                    .entries
                    .iter()
                    .any(|e| e.actual_semester_id == actual.id && e.date == date);
                if !manual {
                    if let Some(day) = self.public_calendar_data.merged_day(date) {
                        calendar.entries.push(CalendarEntry {
                            actual_semester_id: actual.id,
                            date,
                            mapping: match day.kind {
                                Some(crate::calendar::PublicDayKind::Rest) => DayMapping::Rest,
                                Some(crate::calendar::PublicDayKind::Makeup) | None => {
                                    DayMapping::Unconfirmed
                                }
                            },
                        });
                    }
                }
            }
        }
        let mut day = crate::schedule::resolve_day(&semesters, class, date, &calendar)
            .map_err(|e| ScheduleDocumentError::new(DocumentErrorKind::Schedule(e)))?;
        if let AcademicDay::Scheduled(day) = &mut day {
            day.rules_revision = self.rules_revision;
        }
        Ok(day)
    }
    /// CAS and every mutable identity check are repeated by the sole writer. All
    /// caller-supplied revision fields are ignored and re-derived from real changes.
    pub fn transition(
        &self,
        candidate: &Self,
        expected_revision: Revision,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<Self, ScheduleDocumentError> {
        if self.revision == Revision::default() && self != &Self::default() {
            return Err(ScheduleDocumentError::new(DocumentErrorKind::InvalidDraft));
        }
        if self.revision != expected_revision {
            return Err(ScheduleDocumentError::new(
                DocumentErrorKind::RevisionConflict {
                    current: self.revision,
                },
            ));
        }
        self.validate(identity)?;
        candidate.capacity()?;
        identity
            .authorize_management(actor, now)
            .map_err(|e| ScheduleDocumentError::new(DocumentErrorKind::Identity(e)))?;
        self.derive_transition(candidate, identity)
    }
    /// Restricted pure adoption transition. The caller must first authorize an exact
    /// local administrator or a durable automatic-adoption grant on the sole writer.
    /// This cannot change school data, mode, manual mappings, or timetable contents.
    pub fn transition_calendar_data(
        &self,
        data: crate::calendar::PublicCalendarData,
        expected_revision: Revision,
        identity: &IdentityState,
    ) -> Result<Self, ScheduleDocumentError> {
        if self.revision == Revision::default() && self != &Self::default() {
            return Err(ScheduleDocumentError::new(DocumentErrorKind::InvalidDraft));
        }
        if self.revision != expected_revision {
            return Err(ScheduleDocumentError::new(
                DocumentErrorKind::RevisionConflict {
                    current: self.revision,
                },
            ));
        }
        self.validate(identity)?;
        data.validate()
            .map_err(|_| ScheduleDocumentError::new(DocumentErrorKind::InvalidCalendar))?;
        let mut candidate = self.clone();
        candidate.public_calendar.public_verified_years = data.verified_years();
        candidate.public_calendar_data = data;
        self.derive_transition(&candidate, identity)
    }
    fn derive_transition(
        &self,
        candidate: &Self,
        identity: &IdentityState,
    ) -> Result<Self, ScheduleDocumentError> {
        self.reject_retired_ids(candidate)?;
        let mut result = candidate.clone();
        // Like version fields, caller-supplied allocation counters cannot silently
        // exhaust the domain. Only real IDs and old committed evidence advance it.
        result.id_high_water = self
            .id_high_water
            .max(self.maximum_id())
            .max(candidate.maximum_id());
        result.revision = next(self.revision)?;
        result.calendar_mode_revision = if self.calendar_mode_revision.get() == 0 {
            Revision::new(1)
        } else if self.calendar_mode != result.calendar_mode {
            next(self.calendar_mode_revision)?
        } else {
            self.calendar_mode_revision
        };
        result.rules_revision = if self.rules_revision.get() == 0 {
            Revision::new(1)
        } else if self.automatic_gaps != result.automatic_gaps {
            next(self.rules_revision)?
        } else {
            self.rules_revision
        };
        for plan in &mut result.semesters {
            plan.semester.revision = if let Some(old) = self
                .semesters
                .iter()
                .find(|p| p.semester.id == plan.semester.id)
            {
                let mut normalized = plan.clone();
                normalized.semester.revision = old.semester.revision;
                if normalized == *old {
                    old.semester.revision
                } else {
                    next(old.semester.revision)?
                }
            } else {
                Revision::new(1)
            };
        }
        for (new, old) in [
            (&mut result.school_calendar, &self.school_calendar),
            (&mut result.public_calendar, &self.public_calendar),
        ] {
            let content_changed = new.school_coverage != old.school_coverage
                || new.public_verified_years != old.public_verified_years
                || new.entries != old.entries
                || (new.mode == CalendarMode::Public
                    && self.public_calendar_data != result.public_calendar_data);
            new.applied_revision = if old.applied_revision.get() == 0 {
                Revision::new(1)
            } else if content_changed {
                next(old.applied_revision)?
            } else {
                old.applied_revision
            };
            new.mapping_revision = if old.mapping_revision.get() == 0 {
                Revision::new(1)
            } else if new.entries != old.entries {
                next(old.mapping_revision)?
            } else {
                old.mapping_revision
            };
            new.mode_revision = result.calendar_mode_revision;
        }
        result.validate(identity)?;
        Ok(result)
    }
    fn reject_retired_ids(&self, candidate: &Self) -> Result<(), ScheduleDocumentError> {
        let mut semesters = HashSet::new();
        let mut tables = HashSet::new();
        let mut rows = HashSet::new();
        let mut logical = HashSet::new();
        for plan in &self.semesters {
            semesters.insert(plan.semester.id);
            for collection in std::iter::once(&plan.semester.tables)
                .chain(plan.dual_history.iter().map(|h| &h.tables))
            {
                for table in collection {
                    tables.insert(table.id);
                    for row in &table.rows {
                        rows.insert(row.id);
                        logical.insert((plan.semester.id, row.logical_key));
                    }
                }
            }
        }
        let watermark = self.id_high_water.max(self.maximum_id());
        for plan in &candidate.semesters {
            let at = ErrorLocation {
                semester_id: Some(plan.semester.id),
                ..ErrorLocation::default()
            };
            if plan.semester.id.get() <= watermark && !semesters.contains(&plan.semester.id) {
                return Err(error(DocumentErrorKind::RetiredIdentity, at));
            }
            for collection in std::iter::once(&plan.semester.tables)
                .chain(plan.dual_history.iter().map(|h| &h.tables))
            {
                for table in collection {
                    let at = ErrorLocation {
                        table_id: Some(table.id),
                        ..at
                    };
                    if table.id.get() <= watermark && !tables.contains(&table.id) {
                        return Err(error(DocumentErrorKind::RetiredIdentity, at));
                    }
                    for row in &table.rows {
                        if row.id.get() <= watermark && !rows.contains(&row.id)
                            || row.logical_key.get() <= watermark
                                && !logical.contains(&(plan.semester.id, row.logical_key))
                        {
                            return Err(error(
                                DocumentErrorKind::RetiredIdentity,
                                ErrorLocation {
                                    row_id: Some(row.id),
                                    ..at
                                },
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
    /// Includes actual Vec/String capacities, not lengths or a fixed estimate.
    pub fn allocation_bytes(&self) -> Option<usize> {
        let mut bytes = self
            .semesters
            .capacity()
            .checked_mul(std::mem::size_of::<SemesterPlan>())?;
        for plan in &self.semesters {
            bytes = bytes
                .checked_add(plan.name.capacity())?
                .checked_add(table_bytes(&plan.semester.tables)?)?
                .checked_add(detail_bytes(&plan.row_details, &plan.cell_details)?)?;
            if let Some(history) = &plan.dual_history {
                bytes = bytes
                    .checked_add(table_bytes(&history.tables)?)?
                    .checked_add(detail_bytes(&history.row_details, &history.cell_details)?)?;
            }
        }
        for calendar in [&self.school_calendar, &self.public_calendar] {
            bytes = bytes
                .checked_add(
                    calendar
                        .entries
                        .capacity()
                        .checked_mul(std::mem::size_of::<CalendarEntry>())?,
                )?
                .checked_add(
                    calendar
                        .public_verified_years
                        .capacity()
                        .checked_mul(std::mem::size_of::<i32>())?,
                )?;
        }
        bytes = bytes.checked_add(self.public_calendar_data.allocation_bytes()?)?;
        Some(bytes)
    }
}
fn table_bytes(tables: &Vec<WeekTable>) -> Option<usize> {
    let mut bytes = tables
        .capacity()
        .checked_mul(std::mem::size_of::<WeekTable>())?;
    for table in tables {
        bytes = bytes.checked_add(
            table
                .rows
                .capacity()
                .checked_mul(std::mem::size_of::<TimetableRow>())?,
        )?;
    }
    Some(bytes)
}
fn detail_bytes(rows: &Vec<RowDetails>, cells: &Vec<CellDetails>) -> Option<usize> {
    let mut bytes = rows
        .capacity()
        .checked_mul(std::mem::size_of::<RowDetails>())?
        .checked_add(
            cells
                .capacity()
                .checked_mul(std::mem::size_of::<CellDetails>())?,
        )?;
    for row in rows {
        bytes = bytes.checked_add(row.name.capacity())?;
    }
    for cell in cells {
        bytes = bytes
            .checked_add(cell.room.capacity())?
            .checked_add(cell.note.capacity())?;
    }
    Some(bytes)
}

impl SemesterPlan {
    fn operation_bounds(&self) -> Result<(), ScheduleDocumentError> {
        let at = ErrorLocation {
            semester_id: Some(self.semester.id),
            ..ErrorLocation::default()
        };
        if self.name.len() > 512 {
            return Err(error(DocumentErrorKind::InvalidText, at));
        }
        for (tables, rows, cells) in
            std::iter::once((&self.semester.tables, &self.row_details, &self.cell_details)).chain(
                self.dual_history
                    .iter()
                    .map(|h| (&h.tables, &h.row_details, &h.cell_details)),
            )
        {
            if tables.len() > 2
                || tables.iter().any(|t| t.rows.len() > MAX_ROWS_PER_TABLE)
                || rows.len() > MAX_ROWS_PER_TABLE * 2
                || cells.len() > MAX_CELL_DETAILS
            {
                return Err(error(DocumentErrorKind::Capacity, at));
            }
            if rows.iter().any(|r| r.name.len() > 256)
                || cells
                    .iter()
                    .any(|c| c.room.len() > 256 || c.note.len() > 2048)
            {
                return Err(error(DocumentErrorKind::InvalidText, at));
            }
        }
        Ok(())
    }
    pub fn new(semester: Semester, name: String) -> Self {
        Self {
            semester,
            name,
            timing_sync: true,
            dual_history: None,
            row_details: Vec::new(),
            cell_details: Vec::new(),
        }
    }
    /// Explicitly select which table survives as Single; archive BOTH originals.
    pub fn disable_dual(&self, keep: TableId) -> Result<Self, ScheduleDocumentError> {
        self.operation_bounds()?;
        self.semester
            .validate()
            .map_err(|e| located_schedule(e, self.semester.id))?;
        if self.semester.tables.len() != 2 {
            return Err(ScheduleDocumentError::new(
                DocumentErrorKind::InvalidHistory,
            ));
        }
        let mut selected = self
            .semester
            .tables
            .iter()
            .find(|t| t.id == keep)
            .cloned()
            .ok_or_else(|| ScheduleDocumentError::new(DocumentErrorKind::NotFound))?;
        selected.role = TableRole::Single;
        let rows: HashSet<_> = selected.rows.iter().map(|r| r.id).collect();
        let mut result = self.clone();
        result.dual_history = Some(DualTableHistory {
            tables: self.semester.tables.clone(),
            row_details: self.row_details.clone(),
            cell_details: self.cell_details.clone(),
        });
        result.semester.tables = vec![selected];
        result.row_details.retain(|r| rows.contains(&r.row_id));
        result.cell_details.retain(|r| rows.contains(&r.row_id));
        Ok(result)
    }
    pub fn enable_dual(&self, choice: EnableDual) -> Result<Self, ScheduleDocumentError> {
        self.operation_bounds()?;
        self.semester
            .validate()
            .map_err(|e| located_schedule(e, self.semester.id))?;
        if self.semester.tables.len() != 1 {
            return Err(ScheduleDocumentError::new(
                DocumentErrorKind::InvalidHistory,
            ));
        }
        let mut result = self.clone();
        match choice {
            EnableDual::RestoreHistory => {
                let history = self
                    .dual_history
                    .as_ref()
                    .ok_or_else(|| ScheduleDocumentError::new(DocumentErrorKind::InvalidHistory))?;
                result.semester.tables = history.tables.clone();
                result.row_details = history.row_details.clone();
                result.cell_details = history.cell_details.clone();
            }
            EnableDual::Rebuild {
                odd_id,
                even_id,
                odd_rows,
                even_rows,
            } => {
                if let Some(history) = &self.dual_history {
                    if history.tables.iter().any(|t| {
                        t.id == odd_id
                            || t.id == even_id
                            || t.rows
                                .iter()
                                .any(|r| odd_rows.contains(&r.id) || even_rows.contains(&r.id))
                    }) {
                        return Err(ScheduleDocumentError::new(
                            DocumentErrorKind::InvalidHistory,
                        ));
                    }
                }
                let base = &self.semester.tables[0];
                result.semester.tables =
                    split_week_tables(base, odd_id, even_id, &odd_rows, &even_rows)
                        .map_err(|e| located_schedule(e, self.semester.id))?;
                result.row_details.clear();
                result.cell_details.clear();
                for target in [&odd_rows, &even_rows] {
                    for (old, new) in base.rows.iter().zip(target) {
                        for details in self.row_details.iter().filter(|d| d.row_id == old.id) {
                            let mut d = details.clone();
                            d.row_id = *new;
                            result.row_details.push(d);
                        }
                        for details in self.cell_details.iter().filter(|d| d.row_id == old.id) {
                            let mut d = details.clone();
                            d.row_id = *new;
                            result.cell_details.push(d);
                        }
                    }
                }
            }
        }
        result.dual_history = None;
        if result.semester.tables.len() != 2 {
            return Err(ScheduleDocumentError::new(
                DocumentErrorKind::InvalidHistory,
            ));
        }
        result
            .semester
            .validate()
            .map_err(|e| located_schedule(e, self.semester.id))?;
        Ok(result)
    }
    pub fn synchronize_timing(&self, source: TableId) -> Result<Self, ScheduleDocumentError> {
        self.operation_bounds()?;
        let mut result = self.clone();
        result.semester = crate::schedule::synchronize_timing(&self.semester, source)
            .map_err(|e| located_schedule(e, self.semester.id))?;
        Ok(result)
    }
}
impl DraftEdit {
    fn target(&self) -> (SemesterId, TableId) {
        match self {
            Self::SetCell {
                semester_id,
                table_id,
                ..
            }
            | Self::SetRow {
                semester_id,
                table_id,
                ..
            }
            | Self::InsertRow {
                semester_id,
                table_id,
                ..
            }
            | Self::RemoveRow {
                semester_id,
                table_id,
                ..
            }
            | Self::MoveRow {
                semester_id,
                table_id,
                ..
            }
            | Self::CopyDay {
                semester_id,
                table_id,
                ..
            } => (*semester_id, *table_id),
        }
    }
}
impl ScheduleDocument {
    fn fresh_row_id(&self) -> Result<RowId, ScheduleDocumentError> {
        let max = self.id_high_water.max(self.maximum_id());
        RowId::new(
            max.checked_add(1)
                .ok_or_else(|| ScheduleDocumentError::new(DocumentErrorKind::VersionOverflow))?,
        )
        .map_err(|_| ScheduleDocumentError::new(DocumentErrorKind::VersionOverflow))
    }
    /// Grid edits preserve revisions and permit incomplete timing while drafting.
    /// Every operation is all-or-nothing and bounded; full save runs transition.
    pub fn edit(
        &self,
        edit: &DraftEdit,
        identity: &IdentityState,
    ) -> Result<Self, ScheduleDocumentError> {
        self.capacity()?;
        let (semester_id, table_id) = edit.target();
        let at = ErrorLocation {
            semester_id: Some(semester_id),
            table_id: Some(table_id),
            ..ErrorLocation::default()
        };
        let pi = self
            .semesters
            .iter()
            .position(|p| p.semester.id == semester_id)
            .ok_or_else(|| error(DocumentErrorKind::NotFound, at))?;
        let ti = self.semesters[pi]
            .semester
            .tables
            .iter()
            .position(|t| t.id == table_id)
            .ok_or_else(|| error(DocumentErrorKind::NotFound, at))?;
        let mut result = self.clone();
        // Reserve a physical ID above every active/retained row. It is consumed only
        // for a second table, and never changes an existing row's logical identity.
        let fresh = if matches!(edit, DraftEdit::InsertRow { .. })
            && self.semesters[pi].timing_sync
            && self.semesters[pi].semester.tables.len() == 2
        {
            let mut max = self.fresh_row_id()?.get();
            if let DraftEdit::InsertRow { row, .. } = edit {
                max = max.max(
                    row.id
                        .get()
                        .max(row.logical_key.get())
                        .checked_add(1)
                        .ok_or_else(|| error(DocumentErrorKind::VersionOverflow, at))?,
                );
            }
            Some(RowId::new(max).map_err(|_| error(DocumentErrorKind::VersionOverflow, at))?)
        } else {
            None
        };
        let plan = &mut result.semesters[pi];
        match edit {
            DraftEdit::SetCell {
                row_id, day, cell, ..
            } => {
                let row = plan.semester.tables[ti]
                    .rows
                    .iter_mut()
                    .find(|r| r.id == *row_id)
                    .ok_or_else(|| error(DocumentErrorKind::NotFound, at))?;
                let at = ErrorLocation {
                    row_id: Some(*row_id),
                    day: Some(*day),
                    ..at
                };
                if row.kind != RowKind::Course {
                    return Err(error(
                        DocumentErrorKind::Schedule(ScheduleError::InvalidRow),
                        at,
                    ));
                }
                if let Some(cell) = cell {
                    self.validate_cell(identity, plan.semester.class_id, cell, at)?;
                }
                row.courses[day.index()] = cell.clone();
                if cell.is_none() {
                    plan.cell_details
                        .retain(|d| d.row_id != *row_id || d.day != *day);
                }
            }
            DraftEdit::SetRow { row, .. } => {
                if row.kind != RowKind::Course && row.courses.iter().any(Option::is_some) {
                    return Err(error(
                        DocumentErrorKind::Schedule(ScheduleError::InvalidRow),
                        ErrorLocation {
                            row_id: Some(row.id),
                            ..at
                        },
                    ));
                }
                for cell in row.courses.iter().flatten() {
                    self.validate_cell(identity, plan.semester.class_id, cell, at)?;
                }
                let index = plan.semester.tables[ti]
                    .rows
                    .iter()
                    .position(|r| r.id == row.id)
                    .ok_or_else(|| error(DocumentErrorKind::NotFound, at))?;
                let old_key = plan.semester.tables[ti].rows[index].logical_key;
                if plan.timing_sync {
                    for (other, table) in plan.semester.tables.iter_mut().enumerate() {
                        if other != ti {
                            let target =
                                table
                                    .rows
                                    .iter_mut()
                                    .find(|r| r.logical_key == old_key)
                                    .ok_or_else(|| error(DocumentErrorKind::InvalidDraft, at))?;
                            if row.kind != RowKind::Course
                                && target.courses.iter().any(Option::is_some)
                            {
                                return Err(error(
                                    DocumentErrorKind::InvalidDraft,
                                    ErrorLocation {
                                        table_id: Some(table.id),
                                        row_id: Some(target.id),
                                        ..at
                                    },
                                ));
                            }
                            target.logical_key = row.logical_key;
                            target.range = row.range;
                            target.group = row.group;
                            target.kind = row.kind;
                            target.enabled_days = row.enabled_days;
                        }
                    }
                }
                plan.semester.tables[ti].rows[index] = row.clone();
                if row.kind != RowKind::Course {
                    plan.cell_details.retain(|d| d.row_id != row.id);
                }
            }
            DraftEdit::InsertRow { index, row, .. } => {
                if row.kind != RowKind::Course && row.courses.iter().any(Option::is_some) {
                    return Err(error(DocumentErrorKind::InvalidDraft, at));
                }
                for cell in row.courses.iter().flatten() {
                    self.validate_cell(identity, plan.semester.class_id, cell, at)?;
                }
                // A row identity must be new throughout the aggregate, including history.
                if self
                    .semesters
                    .iter()
                    .flat_map(|p| {
                        std::iter::once(&p.semester.tables)
                            .chain(p.dual_history.iter().map(|h| &h.tables))
                    })
                    .flatten()
                    .flat_map(|t| &t.rows)
                    .any(|r| r.id == row.id)
                {
                    return Err(error(
                        DocumentErrorKind::Schedule(ScheduleError::DuplicateIdentity),
                        at,
                    ));
                }
                for (other, table) in plan.semester.tables.iter_mut().enumerate() {
                    if other == ti || plan.timing_sync {
                        if *index > table.rows.len()
                            || table.rows.iter().any(|r| r.logical_key == row.logical_key)
                        {
                            return Err(error(DocumentErrorKind::InvalidDraft, at));
                        }
                        let mut added = row.clone();
                        if other != ti {
                            added.id =
                                fresh.ok_or_else(|| error(DocumentErrorKind::InvalidDraft, at))?;
                            added.courses = std::array::from_fn(|_| None);
                        }
                        table.rows.insert(*index, added);
                    }
                }
            }
            DraftEdit::RemoveRow { row_id, .. } => {
                let key = plan.semester.tables[ti]
                    .rows
                    .iter()
                    .find(|r| r.id == *row_id)
                    .ok_or_else(|| error(DocumentErrorKind::NotFound, at))?
                    .logical_key;
                let mut removed = HashSet::new();
                for (other, table) in plan.semester.tables.iter_mut().enumerate() {
                    if other == ti || plan.timing_sync {
                        let index = table
                            .rows
                            .iter()
                            .position(|r| r.logical_key == key)
                            .ok_or_else(|| error(DocumentErrorKind::InvalidDraft, at))?;
                        removed.insert(table.rows.remove(index).id);
                    }
                }
                plan.row_details.retain(|d| !removed.contains(&d.row_id));
                plan.cell_details.retain(|d| !removed.contains(&d.row_id));
            }
            DraftEdit::MoveRow { row_id, index, .. } => {
                let key = plan.semester.tables[ti]
                    .rows
                    .iter()
                    .find(|r| r.id == *row_id)
                    .ok_or_else(|| error(DocumentErrorKind::NotFound, at))?
                    .logical_key;
                for (other, table) in plan.semester.tables.iter_mut().enumerate() {
                    if other == ti || plan.timing_sync {
                        if *index >= table.rows.len() {
                            return Err(error(DocumentErrorKind::InvalidDraft, at));
                        }
                        let old = table
                            .rows
                            .iter()
                            .position(|r| r.logical_key == key)
                            .ok_or_else(|| error(DocumentErrorKind::InvalidDraft, at))?;
                        let row = table.rows.remove(old);
                        table.rows.insert(*index, row);
                    }
                }
            }
            DraftEdit::CopyDay { from, to, .. } => {
                if from != to {
                    for row in &mut plan.semester.tables[ti].rows {
                        row.enabled_days[to.index()] = row.enabled_days[from.index()];
                        row.courses[to.index()] = row.courses[from.index()].clone();
                    }
                    if plan.timing_sync {
                        let flags: HashMap<_, _> = plan.semester.tables[ti]
                            .rows
                            .iter()
                            .map(|r| (r.logical_key, r.enabled_days[to.index()]))
                            .collect();
                        for (other, table) in plan.semester.tables.iter_mut().enumerate() {
                            if other != ti {
                                for row in &mut table.rows {
                                    row.enabled_days[to.index()] =
                                        *flags.get(&row.logical_key).ok_or_else(|| {
                                            error(DocumentErrorKind::InvalidDraft, at)
                                        })?;
                                }
                            }
                        }
                    }
                    let ids: HashSet<_> =
                        plan.semester.tables[ti].rows.iter().map(|r| r.id).collect();
                    let copied: Vec<_> = plan
                        .cell_details
                        .iter()
                        .filter(|d| ids.contains(&d.row_id) && d.day == *from)
                        .cloned()
                        .map(|mut d| {
                            d.day = *to;
                            d
                        })
                        .collect();
                    plan.cell_details
                        .retain(|d| !ids.contains(&d.row_id) || d.day != *to);
                    plan.cell_details.extend(copied);
                }
            }
        }
        // Detect logical collisions introduced by a draft row edit without forcing
        // users to finish unrelated date/time cells before moving to the next cell.
        for table in &plan.semester.tables {
            let mut ids = HashSet::new();
            let mut logical = HashSet::new();
            for row in &table.rows {
                if !ids.insert(row.id) || !logical.insert(row.logical_key) {
                    return Err(error(
                        DocumentErrorKind::Schedule(ScheduleError::DuplicateIdentity),
                        at,
                    ));
                }
            }
        }
        result.capacity()?;
        result.update_id_high_water();
        Ok(result)
    }
}

mod row_operations;
#[cfg(test)]
mod row_operations_tests;
#[cfg(test)]
mod tests;
