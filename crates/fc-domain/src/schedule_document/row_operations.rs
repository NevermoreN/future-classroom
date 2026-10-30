//! Lossless activity restructuring and explicit row-kind changes. These pure
//! operations own draft IDs but do not authenticate or publish a timetable.
use super::*;

type RowOperationTargets = Vec<(usize, Vec<usize>)>;

fn location(semester: SemesterId, table: Option<TableId>, row: Option<RowId>) -> ErrorLocation {
    ErrorLocation {
        semester_id: Some(semester),
        table_id: table,
        row_id: row,
        ..ErrorLocation::default()
    }
}
fn invalid(at: ErrorLocation) -> ScheduleDocumentError {
    error(DocumentErrorKind::InvalidDraft, at)
}
fn valid_roles(tables: &[WeekTable]) -> bool {
    matches!(tables, [table] if table.role == TableRole::Single)
        || matches!(tables, [a, b] if a.role != b.role
            && matches!(a.role, TableRole::Odd | TableRole::Even)
            && matches!(b.role, TableRole::Odd | TableRole::Even))
}
fn same_shape(a: &TimetableRow, b: &TimetableRow) -> bool {
    a.logical_key == b.logical_key
        && a.range == b.range
        && a.group == b.group
        && a.kind == b.kind
        && a.enabled_days == b.enabled_days
}
fn row_name(plan: &SemesterPlan, id: RowId) -> &str {
    plan.row_details
        .iter()
        .find(|detail| detail.row_id == id)
        .map_or("", |detail| detail.name.as_str())
}

impl ScheduleDocument {
    /// Check bounded metadata and identities before cloning. Unlike a Save, this
    /// deliberately permits unrelated incomplete dates, timing and calendar fields.
    fn row_operation_plan(&self, semester: SemesterId) -> Result<usize, ScheduleDocumentError> {
        self.validate_draft_metadata()?;
        let mut terms = HashSet::new();
        let mut table_ids = HashMap::new();
        let mut row_ids = HashMap::new();
        for plan in &self.semesters {
            let sid = plan.semester.id;
            if !terms.insert(sid) {
                return Err(invalid(location(sid, None, None)));
            }
            for tables in std::iter::once(&plan.semester.tables)
                .chain(plan.dual_history.iter().map(|history| &history.tables))
            {
                let mut local_tables = HashSet::new();
                for table in tables {
                    let at = location(sid, Some(table.id), None);
                    if !local_tables.insert(table.id)
                        || table_ids
                            .insert(table.id, sid)
                            .is_some_and(|old| old != sid)
                    {
                        return Err(error(
                            DocumentErrorKind::Schedule(ScheduleError::DuplicateIdentity),
                            at,
                        ));
                    }
                    let mut local_rows = HashSet::new();
                    let mut logical = HashSet::new();
                    for row in &table.rows {
                        if !local_rows.insert(row.id)
                            || !logical.insert(row.logical_key)
                            || row_ids
                                .insert(row.id, (sid, table.id))
                                .is_some_and(|old| old != (sid, table.id))
                        {
                            return Err(error(
                                DocumentErrorKind::Schedule(ScheduleError::DuplicateIdentity),
                                location(sid, Some(table.id), Some(row.id)),
                            ));
                        }
                    }
                }
            }
        }
        let index = self
            .semesters
            .iter()
            .position(|plan| plan.semester.id == semester)
            .ok_or_else(|| error(DocumentErrorKind::NotFound, location(semester, None, None)))?;
        if !valid_roles(&self.semesters[index].semester.tables) {
            return Err(error(
                DocumentErrorKind::Schedule(ScheduleError::InvalidRoles),
                location(semester, None, None),
            ));
        }
        Ok(index)
    }

    fn row_operation_source(
        &self,
        semester: SemesterId,
        table: TableId,
        row_ids: &[RowId],
    ) -> Result<(usize, RowOperationTargets), ScheduleDocumentError> {
        let pi = self.row_operation_plan(semester)?;
        let plan = &self.semesters[pi];
        let ti = plan
            .semester
            .tables
            .iter()
            .position(|candidate| candidate.id == table)
            .ok_or_else(|| {
                error(
                    DocumentErrorKind::NotFound,
                    location(semester, Some(table), None),
                )
            })?;
        let source = &plan.semester.tables[ti];
        let mut unique = HashSet::new();
        let mut indices = Vec::with_capacity(row_ids.len());
        for id in row_ids {
            let at = location(semester, Some(table), Some(*id));
            if !unique.insert(*id) {
                return Err(invalid(at));
            }
            indices.push(
                source
                    .rows
                    .iter()
                    .position(|row| row.id == *id)
                    .ok_or_else(|| error(DocumentErrorKind::NotFound, at))?,
            );
        }
        indices.sort_unstable();
        let mut targets = Vec::with_capacity(2);
        for (other, target) in plan.semester.tables.iter().enumerate() {
            if other != ti && !plan.timing_sync {
                continue;
            }
            let mut selected = Vec::with_capacity(indices.len());
            for index in &indices {
                let original = &source.rows[*index];
                let target_index = target
                    .rows
                    .iter()
                    .position(|row| row.logical_key == original.logical_key)
                    .ok_or_else(|| {
                        invalid(location(semester, Some(target.id), Some(original.id)))
                    })?;
                let row = &target.rows[target_index];
                let at = location(semester, Some(target.id), Some(row.id));
                if !same_shape(original, row)
                    || row.range.begin >= row.range.end
                    || row.kind != RowKind::Course && row.courses.iter().any(Option::is_some)
                {
                    return Err(invalid(at));
                }
                selected.push(target_index);
            }
            targets.push((other, selected));
        }
        Ok((pi, targets))
    }

    fn row_operation_id(&mut self, at: ErrorLocation) -> Result<u64, ScheduleDocumentError> {
        self.allocate_id().map_err(|mut failure| {
            failure.location = at;
            failure
        })
    }

    /// Swap the semantic week roles only. All tables, rows, content and ordering
    /// keep their identities; a second swap restores the exact original draft.
    pub fn swap_table_roles(&self, semester: SemesterId) -> Result<Self, ScheduleDocumentError> {
        let pi = self.row_operation_plan(semester)?;
        if self.semesters[pi].semester.tables.len() != 2 {
            return Err(error(
                DocumentErrorKind::Schedule(ScheduleError::InvalidRoles),
                location(semester, None, None),
            ));
        }
        let mut next = self.clone();
        for table in &mut next.semesters[pi].semester.tables {
            table.role = match table.role {
                TableRole::Odd => TableRole::Even,
                TableRole::Even => TableRole::Odd,
                TableRole::Single => unreachable!("roles checked before cloning"),
            };
        }
        Ok(next)
    }

    /// Replace one shared activity with one independent activity for each enabled
    /// weekday. Every result receives fresh physical and logical identities.
    pub fn split_activity(
        &self,
        semester: SemesterId,
        table: TableId,
        row_id: RowId,
    ) -> Result<Self, ScheduleDocumentError> {
        let (pi, targets) = self.row_operation_source(semester, table, &[row_id])?;
        let at = location(semester, Some(table), Some(row_id));
        let source = self.semesters[pi]
            .semester
            .tables
            .iter()
            .find(|candidate| candidate.id == table)
            .and_then(|source| source.rows.iter().find(|row| row.id == row_id))
            .expect("source identity checked before cloning");
        let days: Vec<_> = source
            .enabled_days
            .iter()
            .enumerate()
            .filter_map(|(day, enabled)| enabled.then_some(day))
            .collect();
        if source.kind != RowKind::Activity || days.len() < 2 {
            return Err(invalid(at));
        }
        for (ti, _) in &targets {
            let target = &self.semesters[pi].semester.tables[*ti];
            if target.rows.len() + days.len() - 1 > MAX_ROWS_PER_TABLE {
                return Err(error(
                    DocumentErrorKind::Capacity,
                    location(semester, Some(target.id), None),
                ));
            }
        }
        let mut next = self.clone();
        let keys: Vec<_> = days
            .iter()
            .map(|_| {
                next.row_operation_id(at)
                    .map(|id| LogicalRowKey::new(id).unwrap())
            })
            .collect::<Result<_, _>>()?;
        for (ti, indices) in targets {
            let index = indices[0];
            let original = next.semesters[pi].semester.tables[ti].rows[index].clone();
            let name = next.semesters[pi]
                .row_details
                .iter()
                .find(|detail| detail.row_id == original.id)
                .cloned();
            let mut replacements = Vec::with_capacity(days.len());
            let mut names = Vec::new();
            for (day, key) in days.iter().zip(&keys) {
                let mut row = original.clone();
                row.id = RowId::new(next.row_operation_id(at)?).unwrap();
                row.logical_key = *key;
                row.enabled_days = [false; 7];
                row.enabled_days[*day] = true;
                if let Some(name) = &name {
                    names.push(RowDetails {
                        row_id: row.id,
                        name: name.name.clone(),
                    });
                }
                replacements.push(row);
            }
            let plan = &mut next.semesters[pi];
            plan.semester.tables[ti]
                .rows
                .splice(index..=index, replacements);
            plan.row_details
                .retain(|detail| detail.row_id != original.id);
            plan.row_details.extend(names);
        }
        next.validate_draft_metadata()?;
        Ok(next)
    }

    /// Merge two to seven compatible activities without losing any weekday or
    /// label. Caller ordering is immaterial; the earliest selected row is replaced.
    pub fn merge_activities(
        &self,
        semester: SemesterId,
        table: TableId,
        row_ids: &[RowId],
    ) -> Result<Self, ScheduleDocumentError> {
        let at = location(semester, Some(table), row_ids.first().copied());
        if !(2..=7).contains(&row_ids.len()) {
            return Err(invalid(at));
        }
        let (pi, targets) = self.row_operation_source(semester, table, row_ids)?;
        let plan = &self.semesters[pi];
        for (ti, indices) in &targets {
            let target = &plan.semester.tables[*ti];
            let first = &target.rows[indices[0]];
            let mut days = [false; 7];
            for index in indices {
                let row = &target.rows[*index];
                let at = location(semester, Some(target.id), Some(row.id));
                if row.kind != RowKind::Activity
                    || row.range != first.range
                    || row.group != first.group
                    || row_name(plan, row.id) != row_name(plan, first.id)
                    || !row.enabled_days.iter().any(|enabled| *enabled)
                    || (0..7).any(|day| days[day] && row.enabled_days[day])
                {
                    return Err(invalid(at));
                }
                for (day, enabled) in row.enabled_days.iter().enumerate() {
                    days[day] |= *enabled;
                }
            }
        }
        let mut next = self.clone();
        let key = LogicalRowKey::new(next.row_operation_id(at)?).unwrap();
        for (ti, indices) in targets {
            let target = &next.semesters[pi].semester.tables[ti];
            let insertion = *indices.iter().min().unwrap();
            let mut row = target.rows[insertion].clone();
            let removed: HashSet<_> = indices.iter().map(|index| target.rows[*index].id).collect();
            let name = next.semesters[pi]
                .row_details
                .iter()
                .find(|detail| detail.row_id == row.id)
                .cloned();
            row.enabled_days = std::array::from_fn(|day| {
                indices
                    .iter()
                    .any(|index| target.rows[*index].enabled_days[day])
            });
            row.logical_key = key;
            row.id = RowId::new(next.row_operation_id(at)?).unwrap();
            let plan = &mut next.semesters[pi];
            plan.row_details
                .retain(|detail| !removed.contains(&detail.row_id));
            if let Some(name) = name {
                plan.row_details.push(RowDetails {
                    row_id: row.id,
                    name: name.name,
                });
            }
            let rows = &mut plan.semester.tables[ti].rows;
            rows.retain(|row| !removed.contains(&row.id));
            rows.insert(insertion, row);
        }
        next.validate_draft_metadata()?;
        Ok(next)
    }

    /// Apply an explicitly confirmed kind change. Physical row IDs and names stay;
    /// a new logical identity prevents a later sync from confusing old structure.
    /// The native editor is responsible for presenting and confirming data loss.
    pub fn convert_row_kind(
        &self,
        semester: SemesterId,
        table: TableId,
        row_id: RowId,
        kind: RowKind,
    ) -> Result<Self, ScheduleDocumentError> {
        let (pi, targets) = self.row_operation_source(semester, table, &[row_id])?;
        let at = location(semester, Some(table), Some(row_id));
        if targets.iter().any(|(ti, indices)| {
            self.semesters[pi].semester.tables[*ti].rows[indices[0]].kind == kind
        }) {
            return Err(invalid(at));
        }
        let mut next = self.clone();
        let key = LogicalRowKey::new(next.row_operation_id(at)?).unwrap();
        for (ti, indices) in targets {
            let plan = &mut next.semesters[pi];
            let row = &mut plan.semester.tables[ti].rows[indices[0]];
            row.kind = kind;
            row.logical_key = key;
            if kind != RowKind::Course {
                row.courses = std::array::from_fn(|_| None);
                plan.cell_details.retain(|detail| detail.row_id != row.id);
            }
        }
        next.validate_draft_metadata()?;
        Ok(next)
    }
}
