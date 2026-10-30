//! Stable, path-free classroom material metadata and scoped publication rules.
use crate::{
    auth::{ActorContext, PermissionError, Role},
    identity::{IdentityError, IdentityState},
    ids::*,
    schedule::{
        resolve_lesson, AcademicDay, LessonInstanceKey, LessonState, RestPurpose, RowKind,
        TeacherBinding,
    },
    schedule_document::ScheduleDocument,
    time::LocalDateTime,
};
use std::collections::HashSet;
pub const MAX_FOLDER_RECORDS: usize = 2048;
pub const MAX_MATERIAL_BYTES: u64 = 50 * 1024 * 1024;
pub const DESKTOP_MATERIAL_PAGE: usize = 5;
pub const MANAGEMENT_MATERIAL_PAGE: usize = 20;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaterialKind {
    Png,
    Jpeg,
    Bmp,
    Txt,
    Pdf,
    Pptx,
    Docx,
    Xlsx,
}
impl MaterialKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(
            match name.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
                "png" => Self::Png,
                "jpg" | "jpeg" => Self::Jpeg,
                "bmp" => Self::Bmp,
                "txt" => Self::Txt,
                "pdf" => Self::Pdf,
                "pptx" => Self::Pptx,
                "docx" => Self::Docx,
                "xlsx" => Self::Xlsx,
                _ => return None,
            },
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialResource {
    pub version_id: ResourceVersionId,
    pub sha256: [u8; 32],
    pub kind: MaterialKind,
    pub bytes: u64,
}
impl MaterialResource {
    pub fn validate(&self) -> Result<(), MaterialError> {
        if self.bytes == 0
            || self.bytes > MAX_MATERIAL_BYTES
            || matches!(
                self.kind,
                MaterialKind::Png | MaterialKind::Jpeg | MaterialKind::Bmp
            ) && self.bytes > 10 * 1024 * 1024
        {
            return Err(MaterialError::InvalidResource);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialScope {
    pub teacher_id: PrincipalId,
    pub subject_id: SubjectId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeacherMaterialFolder {
    pub id: FolderId,
    pub scope: MaterialScope,
    pub revision: Revision,
    pub last_added_sequence: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialStatus {
    Published,
    Deleted,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialRecord {
    pub id: MaterialId,
    pub folder_id: FolderId,
    pub display_name: String,
    pub resource: MaterialResource,
    pub allowed_class_ids: Vec<ClassId>,
    pub added_at: LocalDateTime,
    pub added_sequence: u64,
    pub status: MaterialStatus,
    pub revision: Revision,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialAppend {
    pub material_id: MaterialId,
    pub display_name: String,
    pub resource: MaterialResource,
    pub allowed_class_ids: Vec<ClassId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialFolderState {
    pub folder: TeacherMaterialFolder,
    pub records: Vec<MaterialRecord>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterialError {
    Identity(IdentityError),
    Permission(PermissionError),
    InvalidName,
    InvalidResource,
    InvalidScope,
    InvalidState,
    InvalidContext,
    Capacity,
    NotFound,
    AlreadyExists,
    RevisionConflict { current: Revision },
    StaleCursor,
    VersionOverflow,
}
impl From<IdentityError> for MaterialError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}
impl From<PermissionError> for MaterialError {
    fn from(error: PermissionError) -> Self {
        Self::Permission(error)
    }
}
pub fn validate_material_name(name: &str) -> Result<(), MaterialError> {
    if name.is_empty()
        || name.len() > 720
        || name.chars().count() > 180
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || MaterialKind::from_name(name).is_none()
    {
        return Err(MaterialError::InvalidName);
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|v| {
                matches!(
                    v,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
    {
        return Err(MaterialError::InvalidName);
    }
    Ok(())
}
fn validate_classes(classes: &[ClassId]) -> Result<(), MaterialError> {
    if classes.is_empty() || classes.len() > crate::identity::MAX_CLASSES {
        return Err(MaterialError::Capacity);
    }
    let mut unique = HashSet::new();
    if classes.iter().any(|id| !unique.insert(*id)) {
        return Err(MaterialError::InvalidScope);
    }
    Ok(())
}
impl MaterialAppend {
    pub fn validate(&self) -> Result<(), MaterialError> {
        validate_material_name(&self.display_name)?;
        self.resource.validate()?;
        validate_classes(&self.allowed_class_ids)?;
        if MaterialKind::from_name(&self.display_name) != Some(self.resource.kind) {
            return Err(MaterialError::InvalidResource);
        }
        Ok(())
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.display_name.capacity().checked_add(
            self.allowed_class_ids
                .capacity()
                .checked_mul(std::mem::size_of::<ClassId>())?,
        )
    }
}
impl MaterialRecord {
    pub fn validate(&self) -> Result<(), MaterialError> {
        validate_material_name(&self.display_name)?;
        self.resource.validate()?;
        validate_classes(&self.allowed_class_ids)?;
        if self.revision.get() == 0
            || self.added_sequence == 0
            || MaterialKind::from_name(&self.display_name) != Some(self.resource.kind)
        {
            return Err(MaterialError::InvalidState);
        }
        Ok(())
    }
}
/// Management uses a real authenticated actor. A visitor is never forged into one.
pub fn authorize_material_folder(
    identity: &IdentityState,
    actor: &ActorContext,
    scope: MaterialScope,
    now: LocalDateTime,
) -> Result<(), MaterialError> {
    identity.validate()?;
    let authority = identity.authority(actor.principal_id)?;
    authority.validate_actor(actor, now)?;
    let account = identity.account(actor.principal_id)?;
    if account.must_change_password {
        return Err(MaterialError::Identity(
            IdentityError::PasswordChangeRequired,
        ));
    }
    if !identity
        .accounts
        .iter()
        .any(|a| a.principal.id == scope.teacher_id)
        || !identity.subjects.iter().any(|s| s.id == scope.subject_id)
    {
        return Err(MaterialError::InvalidScope);
    }
    match authority.principal.role {
        Role::GlobalAdmin => Ok(()),
        Role::User => Err(MaterialError::Permission(PermissionError::RoleDenied)),
        Role::TeacherAdmin => {
            if actor.principal_id != scope.teacher_id {
                return Err(MaterialError::Permission(PermissionError::WrongOwner));
            }
            if !identity.assignments.iter().any(|a| {
                a.teacher_id == scope.teacher_id
                    && a.subject_id == scope.subject_id
                    && identity.is_teacher(scope.teacher_id, a.class_id, scope.subject_id)
            }) {
                return Err(MaterialError::InvalidScope);
            }
            Ok(())
        }
    }
}
impl MaterialFolderState {
    pub fn empty(id: FolderId, scope: MaterialScope) -> Self {
        Self {
            folder: TeacherMaterialFolder {
                id,
                scope,
                revision: Revision::default(),
                last_added_sequence: 0,
            },
            records: vec![],
        }
    }
    pub fn validate(&self) -> Result<(), MaterialError> {
        if self.records.len() > MAX_FOLDER_RECORDS {
            return Err(MaterialError::Capacity);
        }
        if self.folder.last_added_sequence > self.folder.revision.get()
            || self.folder.last_added_sequence == 0
                && (!self.records.is_empty() || self.folder.revision.get() > 1)
            || self.folder.revision.get() == 0
                && (!self.records.is_empty() || self.folder.last_added_sequence != 0)
        {
            return Err(MaterialError::InvalidState);
        }
        let mut ids = HashSet::new();
        let mut sequences = HashSet::new();
        for record in &self.records {
            record.validate()?;
            if record.folder_id != self.folder.id
                || record.revision > self.folder.revision
                || record.added_sequence > self.folder.last_added_sequence
                || !ids.insert(record.id)
                || !sequences.insert(record.added_sequence)
            {
                return Err(MaterialError::InvalidState);
            }
        }
        Ok(())
    }
    pub fn append(
        &self,
        append: &MaterialAppend,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<Self, MaterialError> {
        self.validate()?;
        append.validate()?;
        authorize_material_folder(identity, actor, self.folder.scope, now)?;
        if self.records.len() >= MAX_FOLDER_RECORDS {
            return Err(MaterialError::Capacity);
        }
        if self.records.iter().any(|r| r.id == append.material_id) {
            return Err(MaterialError::AlreadyExists);
        }
        if append.allowed_class_ids.iter().any(|class| {
            !identity.is_teacher(
                self.folder.scope.teacher_id,
                *class,
                self.folder.scope.subject_id,
            )
        }) {
            return Err(MaterialError::InvalidScope);
        }
        let sequence = self
            .folder
            .last_added_sequence
            .checked_add(1)
            .ok_or(MaterialError::VersionOverflow)?;
        let revision = self
            .folder
            .revision
            .next()
            .map_err(|_| MaterialError::VersionOverflow)?;
        let mut next = self.clone();
        next.folder.last_added_sequence = sequence;
        next.folder.revision = revision;
        next.records.push(MaterialRecord {
            id: append.material_id,
            folder_id: self.folder.id,
            display_name: append.display_name.clone(),
            resource: append.resource.clone(),
            allowed_class_ids: append.allowed_class_ids.clone(),
            added_at: now,
            added_sequence: sequence,
            status: MaterialStatus::Published,
            revision: Revision::new(1),
        });
        Ok(next)
    }
    pub fn delete(
        &self,
        id: MaterialId,
        expected_material_revision: Revision,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<Self, MaterialError> {
        self.validate()?;
        authorize_material_folder(identity, actor, self.folder.scope, now)?;
        let index = self
            .records
            .iter()
            .position(|r| r.id == id)
            .ok_or(MaterialError::NotFound)?;
        let record = &self.records[index];
        if record.status != MaterialStatus::Published {
            return Err(MaterialError::NotFound);
        }
        if record.revision != expected_material_revision {
            return Err(MaterialError::RevisionConflict {
                current: record.revision,
            });
        }
        if actor.principal_id != self.folder.scope.teacher_id
            && identity.account(actor.principal_id)?.principal.role != Role::GlobalAdmin
        {
            return Err(MaterialError::Permission(PermissionError::WrongOwner));
        }
        if identity.account(actor.principal_id)?.principal.role == Role::TeacherAdmin
            && record.allowed_class_ids.iter().any(|class| {
                !identity.is_teacher(actor.principal_id, *class, self.folder.scope.subject_id)
            })
        {
            return Err(MaterialError::InvalidScope);
        }
        let folder_revision = self
            .folder
            .revision
            .next()
            .map_err(|_| MaterialError::VersionOverflow)?;
        let record_revision = record
            .revision
            .next()
            .map_err(|_| MaterialError::VersionOverflow)?;
        let mut next = self.clone();
        next.folder.revision = folder_revision;
        next.records[index].revision = record_revision;
        next.records[index].status = MaterialStatus::Deleted;
        Ok(next)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaterialSourceKind {
    CurrentLesson,
    RetainedBreak,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaterialSourceEvidence {
    BaseTimetableOnly,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MaterialSource {
    pub kind: MaterialSourceKind,
    pub lesson: LessonInstanceKey,
    pub subject_id: SubjectId,
    pub teacher_id: PrincipalId,
    pub evidence: MaterialSourceEvidence,
}
/// This milestone has no temporary-override history. The explicit evidence tag
/// identifies a base-timetable source, never fabricated final override evidence.
pub fn resolve_material_source(
    identity: &IdentityState,
    schedule: &ScheduleDocument,
    class: ClassId,
    now: LocalDateTime,
) -> Result<Option<MaterialSource>, MaterialError> {
    identity.validate()?;
    schedule
        .validate(identity)
        .map_err(|_| MaterialError::InvalidContext)?;
    let day = schedule
        .resolve_day(class, now.date)
        .map_err(|_| MaterialError::InvalidContext)?;
    let state = resolve_lesson(&day, now, schedule.automatic_gaps);
    let selected = match &state {
        LessonState::Course { key, cell } => {
            Some((MaterialSourceKind::CurrentLesson, key.clone(), cell.clone()))
        }
        LessonState::Recess { .. } => {
            let AcademicDay::Scheduled(day) = &day else {
                return Ok(None);
            };
            let weekday = day.source_date.weekday();
            let current_group = day
                .table
                .rows
                .iter()
                .filter(|r| {
                    r.enabled_days[weekday.index()]
                        && r.range.begin.get() as u32 * 60 <= now.second_of_day()
                })
                .max_by_key(|r| r.range.begin)
                .map(|r| r.group);
            let mut completed: Vec<_> = day
                .table
                .rows
                .iter()
                .filter(|r| {
                    r.enabled_days[weekday.index()]
                        && Some(r.group) == current_group
                        && u32::from(r.range.end.get()) * 60 <= now.second_of_day()
                })
                .collect();
            completed.sort_by_key(|r| std::cmp::Reverse(r.range.end));
            // A preceding activity/lunch/unknown row clears teacher provenance;
            // an explicitly contiguous recess may retain the completed course.
            completed
                .into_iter()
                .find(|r| r.kind != RowKind::Rest(RestPurpose::Recess))
                .filter(|r| r.kind == RowKind::Course)
                .and_then(|r| {
                    r.courses[weekday.index()].as_ref().map(|cell| {
                        (
                            MaterialSourceKind::RetainedBreak,
                            day.lesson_key(r),
                            cell.clone(),
                        )
                    })
                })
        }
        _ => None,
    };
    let Some((kind, lesson, cell)) = selected else {
        return Ok(None);
    };
    let teacher = match cell.teacher {
        TeacherBinding::Explicit(id) => Some(id),
        TeacherBinding::Default => None,
        TeacherBinding::Unassigned => return Ok(None),
    };
    let teacher_id = identity
        .resolve_teacher(class, cell.subject_id, teacher)
        .map_err(|_| MaterialError::InvalidScope)?;
    Ok(Some(MaterialSource {
        kind,
        lesson,
        subject_id: cell.subject_id,
        teacher_id,
        evidence: MaterialSourceEvidence::BaseTimetableOnly,
    }))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialCursor {
    pub folder_id: FolderId,
    pub folder_revision: Revision,
    pub scope: MaterialScope,
    pub class_id: Option<ClassId>,
    pub source: Option<MaterialSource>,
    pub offset: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialPage {
    pub folder: TeacherMaterialFolder,
    pub source: Option<MaterialSource>,
    pub class_id: Option<ClassId>,
    pub offset: u64,
    pub total: u64,
    pub items: Vec<MaterialRecord>,
    pub next_cursor: Option<MaterialCursor>,
}
impl MaterialPage {
    pub fn validate(
        &self,
        identity: &IdentityState,
        max_items: usize,
    ) -> Result<(), MaterialError> {
        if self.items.len() > max_items
            || max_items > MANAGEMENT_MATERIAL_PAGE
            || self.total > MAX_FOLDER_RECORDS as u64
        {
            return Err(MaterialError::Capacity);
        }
        if self.folder.revision.get() == 0
            || self
                .offset
                .checked_add(self.items.len() as u64)
                .is_none_or(|end| end > self.total)
        {
            return Err(MaterialError::InvalidState);
        }
        if let Some(source) = &self.source {
            if self.class_id != Some(source.lesson.class_id)
                || self.folder.scope.teacher_id != source.teacher_id
                || self.folder.scope.subject_id != source.subject_id
                || !identity.is_teacher(
                    source.teacher_id,
                    source.lesson.class_id,
                    source.subject_id,
                )
            {
                return Err(MaterialError::InvalidContext);
            }
        } else if self.class_id.is_some() {
            return Err(MaterialError::InvalidContext);
        }
        identity.validate()?;
        if !identity
            .accounts
            .iter()
            .any(|a| a.principal.id == self.folder.scope.teacher_id)
            || !identity
                .subjects
                .iter()
                .any(|s| s.id == self.folder.scope.subject_id)
            || self.items.is_empty() && self.offset < self.total
        {
            return Err(MaterialError::InvalidState);
        }
        let mut previous = None;
        let mut ids = HashSet::new();
        for record in &self.items {
            record.validate()?;
            if record.folder_id != self.folder.id
                || record.status != MaterialStatus::Published
                || record.added_sequence > self.folder.last_added_sequence
                || record.revision > self.folder.revision
                || record
                    .allowed_class_ids
                    .iter()
                    .any(|id| !identity.classes.iter().any(|class| class.id == *id))
                || previous.is_some_and(|sequence| record.added_sequence >= sequence)
                || !ids.insert(record.id)
                || self
                    .class_id
                    .is_some_and(|id| !record.allowed_class_ids.contains(&id))
            {
                return Err(MaterialError::InvalidState);
            }
            previous = Some(record.added_sequence);
        }
        let end = self.offset + self.items.len() as u64;
        match &self.next_cursor {
            Some(cursor)
                if end < self.total
                    && cursor.folder_id == self.folder.id
                    && cursor.folder_revision == self.folder.revision
                    && cursor.scope == self.folder.scope
                    && cursor.class_id == self.class_id
                    && cursor.source == self.source
                    && cursor.offset == end => {}
            None if end == self.total => {}
            _ => return Err(MaterialError::StaleCursor),
        }
        Ok(())
    }
}

impl MaterialCursor {
    pub fn validate(
        &self,
        folder: &TeacherMaterialFolder,
        source: Option<&MaterialSource>,
        class_id: Option<ClassId>,
    ) -> Result<(), MaterialError> {
        if self.folder_id != folder.id
            || self.folder_revision != folder.revision
            || self.scope != folder.scope
            || self.class_id != class_id
            || self.source.as_ref() != source
            || self.offset > MAX_FOLDER_RECORDS as u64
        {
            return Err(MaterialError::StaleCursor);
        }
        Ok(())
    }
}
impl MaterialFolderState {
    pub fn management_page(
        &self,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
        cursor: Option<&MaterialCursor>,
    ) -> Result<MaterialPage, MaterialError> {
        self.validate()?;
        authorize_material_folder(identity, actor, self.folder.scope, now)?;
        self.page(identity, None, cursor, MANAGEMENT_MATERIAL_PAGE)
    }
    /// The caller first re-resolves source from the current committed schedule;
    /// this helper checks scope/class publication, never manufactures an actor.
    pub fn published_page(
        &self,
        identity: &IdentityState,
        source: &MaterialSource,
        cursor: Option<&MaterialCursor>,
    ) -> Result<MaterialPage, MaterialError> {
        self.validate()?;
        if source.teacher_id != self.folder.scope.teacher_id
            || source.subject_id != self.folder.scope.subject_id
            || !identity.is_teacher(source.teacher_id, source.lesson.class_id, source.subject_id)
        {
            return Err(MaterialError::InvalidContext);
        }
        self.page(identity, Some(source), cursor, DESKTOP_MATERIAL_PAGE)
    }
    fn page(
        &self,
        identity: &IdentityState,
        source: Option<&MaterialSource>,
        cursor: Option<&MaterialCursor>,
        limit: usize,
    ) -> Result<MaterialPage, MaterialError> {
        let class_id = source.map(|s| s.lesson.class_id);
        if let Some(cursor) = cursor {
            cursor.validate(&self.folder, source, class_id)?;
        }
        let offset = cursor.map_or(0, |c| c.offset);
        let mut records: Vec<_> = self
            .records
            .iter()
            .filter(|r| {
                r.status == MaterialStatus::Published
                    && class_id.is_none_or(|class| r.allowed_class_ids.contains(&class))
            })
            .collect();
        records.sort_by_key(|r| std::cmp::Reverse(r.added_sequence));
        let total = records.len() as u64;
        if offset > total {
            return Err(MaterialError::StaleCursor);
        }
        let items: Vec<_> = records
            .into_iter()
            .skip(offset as usize)
            .take(limit)
            .cloned()
            .collect();
        let end = offset + items.len() as u64;
        let next_cursor = (end < total).then(|| MaterialCursor {
            folder_id: self.folder.id,
            folder_revision: self.folder.revision,
            scope: self.folder.scope,
            class_id,
            source: source.cloned(),
            offset: end,
        });
        let page = MaterialPage {
            folder: self.folder.clone(),
            source: source.cloned(),
            class_id,
            offset,
            total,
            items,
            next_cursor,
        };
        page.validate(identity, limit)?;
        Ok(page)
    }
}

#[cfg(test)]
mod tests;
