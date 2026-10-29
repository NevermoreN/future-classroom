use crate::{
    auth::{ActorContext, AuthorityContext, PermissionError},
    ids::*,
    time::*,
};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableRole {
    Single,
    Odd,
    Even,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestPurpose {
    Recess,
    Lunch,
    Other,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayGroup {
    Morning,
    Afternoon,
    Other(u16),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Course,
    Rest(RestPurpose),
    Activity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeacherBinding {
    Explicit(PrincipalId),
    Default,
    Unassigned,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CourseCell {
    pub subject_id: SubjectId,
    pub teacher: TeacherBinding,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimetableRow {
    pub id: RowId,
    pub logical_key: LogicalRowKey,
    pub group: DayGroup,
    pub kind: RowKind,
    pub range: MinuteRange,
    pub enabled_days: [bool; 7],
    pub courses: [Option<CourseCell>; 7],
}
impl TimetableRow {
    fn course(&self, day: Weekday) -> Option<&CourseCell> {
        if self.kind == RowKind::Course && self.enabled_days[day.index()] {
            self.courses[day.index()].as_ref()
        } else {
            None
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeekTable {
    pub id: TableId,
    pub role: TableRole,
    pub rows: Vec<TimetableRow>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Semester {
    pub id: SemesterId,
    pub class_id: ClassId,
    pub revision: Revision,
    pub enabled: bool,
    pub start: LocalDate,
    pub end: LocalDate,
    pub first_week_monday: LocalDate,
    pub tables: Vec<WeekTable>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    InvalidTerm,
    AnchorNotMonday,
    InvalidRoles,
    DuplicateIdentity,
    InvalidRow,
    Overlap {
        table_id: TableId,
        day: Weekday,
        first: RowId,
        second: RowId,
    },
    AmbiguousTerm,
    MissingTerm,
    BeforeAnchor,
    MissingSource,
    InvalidMapping,
    InvalidOverride,
    Permission(PermissionError),
}
impl Semester {
    pub fn contains(&self, date: LocalDate) -> bool {
        self.enabled && date >= self.start && date <= self.end
    }
    pub fn teaching_week(&self, date: LocalDate) -> Result<u32, ScheduleError> {
        if !self.contains(date) {
            return Err(ScheduleError::MissingTerm);
        }
        let days = date.days_since(self.first_week_monday);
        if days < 0 {
            return Err(ScheduleError::BeforeAnchor);
        }
        Ok((days / 7 + 1) as u32)
    }
    pub fn table_for(&self, date: LocalDate) -> Result<&WeekTable, ScheduleError> {
        let week = self.teaching_week(date)?;
        self.tables
            .iter()
            .find(|t| {
                t.role == TableRole::Single
                    || t.role
                        == if week % 2 == 1 {
                            TableRole::Odd
                        } else {
                            TableRole::Even
                        }
            })
            .ok_or(ScheduleError::InvalidRoles)
    }
    pub fn validate(&self) -> Result<(), ScheduleError> {
        if self.start > self.end || self.first_week_monday > self.start {
            return Err(ScheduleError::InvalidTerm);
        }
        if self.first_week_monday.weekday() != Weekday::Monday {
            return Err(ScheduleError::AnchorNotMonday);
        }
        let roles: Vec<_> = self.tables.iter().map(|t| t.role).collect();
        if !(roles == [TableRole::Single]
            || (roles.len() == 2
                && roles.contains(&TableRole::Odd)
                && roles.contains(&TableRole::Even)))
        {
            return Err(ScheduleError::InvalidRoles);
        }
        let mut ids = HashSet::new();
        let mut row_ids = HashSet::new();
        for table in &self.tables {
            if !ids.insert(table.id) {
                return Err(ScheduleError::DuplicateIdentity);
            }
            let mut logical = HashSet::new();
            for row in &table.rows {
                if !row_ids.insert(row.id) || !logical.insert(row.logical_key) {
                    return Err(ScheduleError::DuplicateIdentity);
                }
                if row.range.begin >= row.range.end
                    || (row.kind != RowKind::Course && row.courses.iter().any(Option::is_some))
                {
                    return Err(ScheduleError::InvalidRow);
                }
            }
            for index in 0..7 {
                let day = Weekday::from_monday_index(index as u8).unwrap();
                let mut rows: Vec<_> = table
                    .rows
                    .iter()
                    .filter(|r| r.enabled_days[index])
                    .collect();
                rows.sort_by_key(|r| r.range.begin);
                for pair in rows.windows(2) {
                    if pair[0].range.overlaps(pair[1].range) {
                        return Err(ScheduleError::Overlap {
                            table_id: table.id,
                            day,
                            first: pair[0].id,
                            second: pair[1].id,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}
pub fn validate_semesters(semesters: &[Semester]) -> Result<(), ScheduleError> {
    let mut ids = HashSet::new();
    for term in semesters {
        term.validate()?;
        if !ids.insert(term.id) {
            return Err(ScheduleError::DuplicateIdentity);
        }
    }
    for (i, a) in semesters.iter().enumerate() {
        for b in &semesters[i + 1..] {
            if a.enabled
                && b.enabled
                && a.class_id == b.class_id
                && a.start <= b.end
                && b.start <= a.end
            {
                return Err(ScheduleError::AmbiguousTerm);
            }
        }
    }
    Ok(())
}

/// Caller allocates new physical IDs; cells and notes remain owned independent copies.
pub fn split_week_tables(
    base: &WeekTable,
    odd_id: TableId,
    even_id: TableId,
    odd_rows: &[RowId],
    even_rows: &[RowId],
) -> Result<Vec<WeekTable>, ScheduleError> {
    if odd_id == even_id || odd_rows.len() != base.rows.len() || even_rows.len() != base.rows.len()
    {
        return Err(ScheduleError::DuplicateIdentity);
    }
    let all: HashSet<_> = odd_rows.iter().chain(even_rows).copied().collect();
    if all.len() != base.rows.len() * 2
        || odd_rows
            .iter()
            .chain(even_rows)
            .any(|id| base.rows.iter().any(|r| r.id == *id))
        || odd_id == base.id
        || even_id == base.id
    {
        return Err(ScheduleError::DuplicateIdentity);
    }
    let mut odd = base.clone();
    odd.id = odd_id;
    odd.role = TableRole::Odd;
    let mut even = base.clone();
    even.id = even_id;
    even.role = TableRole::Even;
    for (row, id) in odd.rows.iter_mut().zip(odd_rows) {
        row.id = *id;
    }
    for (row, id) in even.rows.iter_mut().zip(even_rows) {
        row.id = *id;
    }
    Ok(vec![odd, even])
}

/// Validates a complete candidate before returning it; an error leaves caller's draft intact.
/// Synchronizes only logical row timing/structure, never course cells or physical row IDs.
pub fn synchronize_timing(semester: &Semester, source: TableId) -> Result<Semester, ScheduleError> {
    let mut next = semester.clone();
    let source = semester
        .tables
        .iter()
        .find(|t| t.id == source)
        .ok_or(ScheduleError::InvalidRoles)?;
    for table in &mut next.tables {
        if table.id == source.id {
            continue;
        }
        if table.rows.len() != source.rows.len() {
            return Err(ScheduleError::InvalidRow);
        }
        for target in &mut table.rows {
            let from = source
                .rows
                .iter()
                .find(|r| r.logical_key == target.logical_key)
                .ok_or(ScheduleError::InvalidRow)?;
            if from.kind != target.kind {
                return Err(ScheduleError::InvalidRow);
            }
            target.range = from.range;
            target.group = from.group;
            target.enabled_days = from.enabled_days;
        }
        table.rows.sort_by_key(|r| r.range.begin);
    }
    next.validate()?;
    Ok(next)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CalendarMode {
    School,
    Public,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DayMapping {
    Rest,
    SourceDate(LocalDate),
    Unconfirmed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarEntry {
    pub actual_semester_id: SemesterId,
    pub date: LocalDate,
    pub mapping: DayMapping,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarConfig {
    pub mode: CalendarMode,
    pub mode_revision: Revision,
    pub applied_revision: Revision,
    pub mapping_revision: Revision,
    pub school_coverage: Option<(LocalDate, LocalDate)>,
    pub public_verified_years: Vec<i32>,
    pub entries: Vec<CalendarEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolvedScheduleVersion {
    pub actual_semester_id: SemesterId,
    pub actual_revision: Revision,
    pub source_semester_id: SemesterId,
    pub source_revision: Revision,
    pub calendar_mode: CalendarMode,
    pub calendar_mode_revision: Revision,
    pub applied_calendar_revision: Revision,
    pub makeup_mapping_revision: Revision,
    pub source_table_id: TableId,
    pub source_row_id: RowId,
    pub rules_revision: Revision,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LessonInstanceKey {
    pub actual_date: LocalDate,
    pub class_id: ClassId,
    pub version: ResolvedScheduleVersion,
    pub range: MinuteRange,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDay {
    pub actual_date: LocalDate,
    pub class_id: ClassId,
    pub source_date: LocalDate,
    pub actual_semester_id: SemesterId,
    pub actual_revision: Revision,
    pub source_semester_id: SemesterId,
    pub source_revision: Revision,
    pub table: WeekTable,
    pub calendar: CalendarConfig,
    pub public_data_unverified: bool,
}
impl ResolvedDay {
    pub fn lesson_key(&self, row: &TimetableRow) -> LessonInstanceKey {
        LessonInstanceKey {
            actual_date: self.actual_date,
            class_id: self.class_id,
            range: row.range,
            version: ResolvedScheduleVersion {
                actual_semester_id: self.actual_semester_id,
                actual_revision: self.actual_revision,
                source_semester_id: self.source_semester_id,
                source_revision: self.source_revision,
                calendar_mode: self.calendar.mode,
                calendar_mode_revision: self.calendar.mode_revision,
                applied_calendar_revision: self.calendar.applied_revision,
                makeup_mapping_revision: self.calendar.mapping_revision,
                source_table_id: self.table.id,
                source_row_id: row.id,
                rules_revision: Revision::new(1),
            },
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcademicDay {
    Scheduled(ResolvedDay),
    Rest,
    Unconfirmed,
    OutOfTerm,
}

/// Call with one transactionally consistent repository snapshot. A source date resolves
/// directly against its base table: its own calendar entries are deliberately ignored.
pub fn resolve_day(
    semesters: &[Semester],
    class_id: ClassId,
    date: LocalDate,
    calendar: &CalendarConfig,
) -> Result<AcademicDay, ScheduleError> {
    validate_semesters(semesters)?;
    let actual = match semesters
        .iter()
        .find(|s| s.class_id == class_id && s.contains(date))
    {
        Some(s) => s,
        None => return Ok(AcademicDay::OutOfTerm),
    };
    if calendar.mode == CalendarMode::School {
        match calendar.school_coverage {
            Some((start, end)) if start <= end && date >= start && date <= end => {}
            _ => return Ok(AcademicDay::Unconfirmed),
        }
    }
    let entries: Vec<_> = calendar
        .entries
        .iter()
        .filter(|e| e.actual_semester_id == actual.id && e.date == date)
        .collect();
    if entries.len() > 1 {
        return Err(ScheduleError::InvalidMapping);
    }
    let source_date = match entries.first().map(|e| &e.mapping) {
        Some(DayMapping::Rest) => return Ok(AcademicDay::Rest),
        Some(DayMapping::Unconfirmed) => return Ok(AcademicDay::Unconfirmed),
        Some(DayMapping::SourceDate(source)) => {
            if *source == date {
                return Err(ScheduleError::InvalidMapping);
            }
            *source
        }
        None => date,
    };
    let source = match semesters
        .iter()
        .find(|s| s.class_id == class_id && s.contains(source_date))
    {
        Some(s) => s,
        None => return Ok(AcademicDay::Unconfirmed),
    };
    let table = source.table_for(source_date)?.clone();
    Ok(AcademicDay::Scheduled(ResolvedDay {
        actual_date: date,
        class_id,
        source_date,
        actual_semester_id: actual.id,
        actual_revision: actual.revision,
        source_semester_id: source.id,
        source_revision: source.revision,
        table,
        calendar: calendar.clone(),
        public_data_unverified: calendar.mode == CalendarMode::Public
            && !calendar.public_verified_years.contains(&date.year()),
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LessonState {
    Course {
        key: LessonInstanceKey,
        cell: CourseCell,
    },
    Recess {
        deadline: LocalDateTime,
        remaining_seconds: u32,
        until_next_course: bool,
    },
    Lunch,
    Activity,
    Rest,
    Idle,
    Unconfirmed,
    OutOfTerm,
}
pub fn resolve_lesson(day: &AcademicDay, now: LocalDateTime, automatic_gaps: bool) -> LessonState {
    let day = match day {
        AcademicDay::Scheduled(d) => d,
        AcademicDay::Rest => return LessonState::Rest,
        AcademicDay::Unconfirmed => return LessonState::Unconfirmed,
        AcademicDay::OutOfTerm => return LessonState::OutOfTerm,
    };
    if day.actual_date != now.date {
        return LessonState::OutOfTerm;
    }
    let weekday = day.source_date.weekday();
    let mut rows: Vec<_> = day
        .table
        .rows
        .iter()
        .filter(|r| r.enabled_days[weekday.index()])
        .collect();
    rows.sort_by_key(|r| r.range.begin);
    let second = now.second_of_day();
    let recess = |end: Minute, next: bool| LessonState::Recess {
        deadline: LocalDateTime::new(now.date, u32::from(end.get()) * 60).unwrap(),
        remaining_seconds: u32::from(end.get()) * 60 - second,
        until_next_course: next,
    };
    for (index, row) in rows.iter().enumerate() {
        if !row.range.contains_second(second) {
            continue;
        }
        return match row.kind {
            RowKind::Course => match row.course(weekday) {
                Some(cell) => LessonState::Course {
                    key: day.lesson_key(row),
                    cell: cell.clone(),
                },
                None => LessonState::Idle,
            },
            RowKind::Activity => LessonState::Activity,
            RowKind::Rest(RestPurpose::Lunch) => LessonState::Lunch,
            RowKind::Rest(RestPurpose::Recess) => {
                let group = row.group;
                let allowed = matches!(group, DayGroup::Morning | DayGroup::Afternoon);
                // A lunch/group boundary forbids borrowing an earlier/later lesson.
                let before = rows[..index]
                    .iter()
                    .rev()
                    .take_while(|r| r.group == group && r.kind != RowKind::Rest(RestPurpose::Lunch))
                    .any(|r| r.course(weekday).is_some());
                let after = rows[index + 1..]
                    .iter()
                    .take_while(|r| r.group == group && r.kind != RowKind::Rest(RestPurpose::Lunch))
                    .any(|r| r.course(weekday).is_some());
                if allowed && before && after {
                    let next = rows.get(index + 1).is_some_and(|r| {
                        r.range.begin == row.range.end && r.course(weekday).is_some()
                    });
                    recess(row.range.end, next)
                } else {
                    LessonState::Rest
                }
            }
            RowKind::Rest(_) => LessonState::Rest,
        };
    }
    if automatic_gaps {
        for pair in rows.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if matches!(a.group, DayGroup::Morning | DayGroup::Afternoon)
                && a.group == b.group
                && a.course(weekday).is_some()
                && b.course(weekday).is_some()
                && second >= u32::from(a.range.end.get()) * 60
                && second < u32::from(b.range.begin.get()) * 60
            {
                return recess(b.range.begin, true);
            }
        }
    }
    LessonState::Idle
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverrideStatus {
    Active,
    Restored,
    Expired,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LessonOverride {
    pub id: OverrideId,
    pub key: LessonInstanceKey,
    pub target: CourseCell,
    pub actor_id: PrincipalId,
    pub revision: Revision,
    pub created_at: LocalDateTime,
    pub status: OverrideStatus,
    last_observed: LocalDateTime,
}
impl LessonOverride {
    pub fn create_for_current_lesson(
        id: OverrideId,
        current: &LessonState,
        target: CourseCell,
        actor: &ActorContext,
        authority: &AuthorityContext,
        now: LocalDateTime,
    ) -> Result<Self, ScheduleError> {
        let key = match current {
            LessonState::Course { key, .. } => key.clone(),
            _ => return Err(ScheduleError::InvalidOverride),
        };
        Self::create(id, key, target, actor, authority, now)
    }
    fn create(
        id: OverrideId,
        key: LessonInstanceKey,
        target: CourseCell,
        actor: &ActorContext,
        authority: &AuthorityContext,
        now: LocalDateTime,
    ) -> Result<Self, ScheduleError> {
        authority
            .authorize_global_local(actor, now)
            .map_err(ScheduleError::Permission)?;
        authority
            .require_class(key.class_id)
            .map_err(ScheduleError::Permission)?;
        if now.date != key.actual_date
            || !key.range.contains_second(now.second_of_day())
            || !matches!(target.teacher, TeacherBinding::Explicit(_))
        {
            return Err(ScheduleError::InvalidOverride);
        }
        if !authority.assignments.iter().any(|a| {
            a.enabled
                && a.class_id == key.class_id
                && a.subject_id == target.subject_id
                && target.teacher == TeacherBinding::Explicit(a.teacher_id)
        }) {
            return Err(ScheduleError::InvalidOverride);
        }
        Ok(Self {
            id,
            key,
            target,
            actor_id: actor.principal_id,
            revision: Revision::new(1),
            created_at: now,
            status: OverrideStatus::Active,
            last_observed: now,
        })
    }
    /// Observe every schedule/time/authority change. Once terminated it never revives,
    /// including after a backward wall-clock jump. Target scope is checked by caller.
    pub fn observe(
        &mut self,
        key: Option<&LessonInstanceKey>,
        now: LocalDateTime,
        target_valid: bool,
    ) -> Option<&CourseCell> {
        if self.status != OverrideStatus::Active {
            return None;
        }
        if key != Some(&self.key)
            || now < self.last_observed
            || now.date != self.key.actual_date
            || !self.key.range.contains_second(now.second_of_day())
            || !target_valid
        {
            self.status = OverrideStatus::Expired;
            return None;
        }
        self.last_observed = now;
        Some(&self.target)
    }
    pub fn restore(&mut self) {
        if self.status == OverrideStatus::Active {
            self.status = OverrideStatus::Restored;
        }
    }
}

/// Persisted evidence, not reconstructed by reading a later edited source timetable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastLessonMaterialContext {
    pub source_lesson_key: LessonInstanceKey,
    pub actual_teacher: PrincipalId,
    pub actual_subject: SubjectId,
    pub folder_id: FolderId,
    pub effective_from: LocalDateTime,
    pub effective_until: LocalDateTime,
}
impl LastLessonMaterialContext {
    pub fn usable_for_recess(
        &self,
        class: ClassId,
        now: LocalDateTime,
        authority_valid: bool,
    ) -> bool {
        authority_valid
            && self.source_lesson_key.class_id == class
            && self.source_lesson_key.actual_date == now.date
            && self.effective_from.date == now.date
            && self.effective_until.date == now.date
            && self.effective_from < self.effective_until
            && self.effective_from.second_of_day()
                >= u32::from(self.source_lesson_key.range.begin.get()) * 60
            && self.effective_until.second_of_day()
                == u32::from(self.source_lesson_key.range.end.get()) * 60
            && now >= self.effective_until
    }
}

#[cfg(test)]
mod tests;
