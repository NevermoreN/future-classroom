//! Path-free, strictly decoded material metadata. Every persisted value is 64 KiB.
use super::codec::*;
use crate::StorageError;
use fc_domain::{ids::*, materials::*, schedule::*, time::*};
use serde_json::{json, Value};
fn id<T>(
    v: &Value,
    n: &str,
    make: impl FnOnce(u64) -> Result<T, InvalidId>,
) -> Result<T, StorageError> {
    make(number(v, n)?).map_err(|_| StorageError::Database)
}
pub(super) fn scope(s: MaterialScope) -> Value {
    json!({"teacher":s.teacher_id.get(),"subject":s.subject_id.get()})
}
pub(super) fn read_scope(v: &Value) -> Result<MaterialScope, StorageError> {
    object_fields(v, &["teacher", "subject"])?;
    Ok(MaterialScope {
        teacher_id: id(v, "teacher", PrincipalId::new)?,
        subject_id: id(v, "subject", SubjectId::new)?,
    })
}
pub(super) fn resource(r: &MaterialResource) -> Value {
    json!({"version":r.version_id.get(),"sha256":r.sha256,"kind":match r.kind{MaterialKind::Png=>"png",MaterialKind::Jpeg=>"jpeg",MaterialKind::Bmp=>"bmp",MaterialKind::Txt=>"txt",MaterialKind::Pdf=>"pdf",MaterialKind::Pptx=>"pptx",MaterialKind::Docx=>"docx",MaterialKind::Xlsx=>"xlsx"},"bytes":r.bytes})
}
pub(super) fn read_resource(v: &Value) -> Result<MaterialResource, StorageError> {
    object_fields(v, &["version", "sha256", "kind", "bytes"])?;
    let sha256: [u8; 32] =
        serde_json::from_value(v["sha256"].clone()).map_err(|_| StorageError::Database)?;
    let kind = match v["kind"].as_str() {
        Some("png") => MaterialKind::Png,
        Some("jpeg") => MaterialKind::Jpeg,
        Some("bmp") => MaterialKind::Bmp,
        Some("txt") => MaterialKind::Txt,
        Some("pdf") => MaterialKind::Pdf,
        Some("pptx") => MaterialKind::Pptx,
        Some("docx") => MaterialKind::Docx,
        Some("xlsx") => MaterialKind::Xlsx,
        _ => return Err(StorageError::Database),
    };
    let r = MaterialResource {
        version_id: id(v, "version", ResourceVersionId::new)?,
        sha256,
        kind,
        bytes: number(v, "bytes")?,
    };
    r.validate().map_err(|_| StorageError::Database)?;
    Ok(r)
}
pub(super) fn folder(f: &TeacherMaterialFolder) -> Value {
    json!({"id":f.id.get(),"scope":scope(f.scope),"revision":f.revision.get(),"sequence":f.last_added_sequence})
}
pub(super) fn read_folder(v: &Value) -> Result<TeacherMaterialFolder, StorageError> {
    object_fields(v, &["id", "scope", "revision", "sequence"])?;
    Ok(TeacherMaterialFolder {
        id: id(v, "id", FolderId::new)?,
        scope: read_scope(&v["scope"])?,
        revision: Revision::new(number(v, "revision")?),
        last_added_sequence: number(v, "sequence")?,
    })
}
pub(super) fn record(r: &MaterialRecord) -> Value {
    json!({"id":r.id.get(),"folder":r.folder_id.get(),"name":r.display_name,"resource":resource(&r.resource),"classes":r.allowed_class_ids.iter().map(|c|c.get()).collect::<Vec<_>>(),"added_at":time(r.added_at),"sequence":r.added_sequence,"status":match r.status{MaterialStatus::Published=>"published",MaterialStatus::Deleted=>"deleted"},"revision":r.revision.get()})
}
pub(super) fn read_record(v: &Value) -> Result<MaterialRecord, StorageError> {
    object_fields(
        v,
        &[
            "id", "folder", "name", "resource", "classes", "added_at", "sequence", "status",
            "revision",
        ],
    )?;
    let classes = array(v, "classes")?;
    if classes.len() > fc_domain::identity::MAX_CLASSES {
        return Err(StorageError::Database);
    }
    let r = MaterialRecord {
        id: id(v, "id", MaterialId::new)?,
        folder_id: id(v, "folder", FolderId::new)?,
        display_name: text(v, "name")?,
        resource: read_resource(&v["resource"])?,
        allowed_class_ids: classes
            .iter()
            .map(|c| {
                ClassId::new(c.as_u64().ok_or(StorageError::Database)?)
                    .map_err(|_| StorageError::Database)
            })
            .collect::<Result<Vec<_>, _>>()?,
        added_at: read_time(&v["added_at"])?,
        added_sequence: number(v, "sequence")?,
        status: match v["status"].as_str() {
            Some("published") => MaterialStatus::Published,
            Some("deleted") => MaterialStatus::Deleted,
            _ => return Err(StorageError::Database),
        },
        revision: Revision::new(number(v, "revision")?),
    };
    r.validate().map_err(|_| StorageError::Database)?;
    Ok(r)
}
pub(super) fn lesson(k: &LessonInstanceKey) -> Value {
    let v = &k.version;
    json!({"date":date(k.actual_date),"class":k.class_id.get(),"begin":k.range.begin.get(),"end":k.range.end.get(),"version":{"actual_semester":v.actual_semester_id.get(),"actual_revision":v.actual_revision.get(),"source_semester":v.source_semester_id.get(),"source_revision":v.source_revision.get(),"mode":match v.calendar_mode{CalendarMode::School=>"school",CalendarMode::Public=>"public"},"mode_revision":v.calendar_mode_revision.get(),"applied_revision":v.applied_calendar_revision.get(),"mapping_revision":v.makeup_mapping_revision.get(),"table":v.source_table_id.get(),"row":v.source_row_id.get(),"rules_revision":v.rules_revision.get()}})
}
pub(super) fn read_lesson(v: &Value) -> Result<LessonInstanceKey, StorageError> {
    object_fields(v, &["date", "class", "begin", "end", "version"])?;
    let s = &v["version"];
    object_fields(
        s,
        &[
            "actual_semester",
            "actual_revision",
            "source_semester",
            "source_revision",
            "mode",
            "mode_revision",
            "applied_revision",
            "mapping_revision",
            "table",
            "row",
            "rules_revision",
        ],
    )?;
    let minute = |name| {
        Minute::new(u16::try_from(number(v, name)?).map_err(|_| StorageError::Database)?)
            .map_err(|_| StorageError::Database)
    };
    Ok(LessonInstanceKey {
        actual_date: read_date(&v["date"])?,
        class_id: id(v, "class", ClassId::new)?,
        range: MinuteRange::new(minute("begin")?, minute("end")?)
            .map_err(|_| StorageError::Database)?,
        version: ResolvedScheduleVersion {
            actual_semester_id: id(s, "actual_semester", SemesterId::new)?,
            actual_revision: Revision::new(number(s, "actual_revision")?),
            source_semester_id: id(s, "source_semester", SemesterId::new)?,
            source_revision: Revision::new(number(s, "source_revision")?),
            calendar_mode: match s["mode"].as_str() {
                Some("school") => CalendarMode::School,
                Some("public") => CalendarMode::Public,
                _ => return Err(StorageError::Database),
            },
            calendar_mode_revision: Revision::new(number(s, "mode_revision")?),
            applied_calendar_revision: Revision::new(number(s, "applied_revision")?),
            makeup_mapping_revision: Revision::new(number(s, "mapping_revision")?),
            source_table_id: id(s, "table", TableId::new)?,
            source_row_id: id(s, "row", RowId::new)?,
            rules_revision: Revision::new(number(s, "rules_revision")?),
        },
    })
}
pub(super) fn source(s: &MaterialSource) -> Value {
    json!({"kind":match s.kind{MaterialSourceKind::CurrentLesson=>"current",MaterialSourceKind::RetainedBreak=>"break"},"lesson":lesson(&s.lesson),"subject":s.subject_id.get(),"teacher":s.teacher_id.get(),"evidence":match s.evidence{MaterialSourceEvidence::BaseTimetableOnly=>json!("base_timetable_only"),MaterialSourceEvidence::PersistedLesson{revision}=>json!({"lesson_revision":revision.get()})}})
}
pub(super) fn read_source(v: &Value) -> Result<MaterialSource, StorageError> {
    object_fields(v, &["kind", "lesson", "subject", "teacher", "evidence"])?;
    let evidence = if v["evidence"].as_str() == Some("base_timetable_only") {
        MaterialSourceEvidence::BaseTimetableOnly
    } else {
        object_fields(&v["evidence"], &["lesson_revision"])?;
        MaterialSourceEvidence::PersistedLesson {
            revision: Revision::new(number(&v["evidence"], "lesson_revision")?),
        }
    };
    Ok(MaterialSource {
        kind: match v["kind"].as_str() {
            Some("current") => MaterialSourceKind::CurrentLesson,
            Some("break") => MaterialSourceKind::RetainedBreak,
            _ => return Err(StorageError::Database),
        },
        lesson: read_lesson(&v["lesson"])?,
        subject_id: id(v, "subject", SubjectId::new)?,
        teacher_id: id(v, "teacher", PrincipalId::new)?,
        evidence,
    })
}
pub(super) fn cursor(c: &MaterialCursor) -> Value {
    json!({"folder":c.folder_id.get(),"revision":c.folder_revision.get(),"scope":scope(c.scope),"class":c.class_id.map(|c|c.get()),"source":c.source.as_ref().map(source),"offset":c.offset})
}
pub(super) fn read_cursor(v: &Value) -> Result<MaterialCursor, StorageError> {
    object_fields(
        v,
        &["folder", "revision", "scope", "class", "source", "offset"],
    )?;
    Ok(MaterialCursor {
        folder_id: id(v, "folder", FolderId::new)?,
        folder_revision: Revision::new(number(v, "revision")?),
        scope: read_scope(&v["scope"])?,
        class_id: if v["class"].is_null() {
            None
        } else {
            Some(id(v, "class", ClassId::new)?)
        },
        source: if v["source"].is_null() {
            None
        } else {
            Some(read_source(&v["source"])?)
        },
        offset: number(v, "offset")?,
    })
}
pub(super) fn page(p: &MaterialPage) -> Value {
    json!({"folder":folder(&p.folder),"source":p.source.as_ref().map(source),"class":p.class_id.map(|c|c.get()),"offset":p.offset,"total":p.total,"items":p.items.iter().map(record).collect::<Vec<_>>(),"next":p.next_cursor.as_ref().map(cursor)})
}
pub(super) fn read_page(v: &Value) -> Result<MaterialPage, StorageError> {
    object_fields(
        v,
        &[
            "folder", "source", "class", "offset", "total", "items", "next",
        ],
    )?;
    let items = array(v, "items")?;
    if items.len() > MANAGEMENT_MATERIAL_PAGE {
        return Err(StorageError::Database);
    }
    Ok(MaterialPage {
        folder: read_folder(&v["folder"])?,
        source: if v["source"].is_null() {
            None
        } else {
            Some(read_source(&v["source"])?)
        },
        class_id: if v["class"].is_null() {
            None
        } else {
            Some(id(v, "class", ClassId::new)?)
        },
        offset: number(v, "offset")?,
        total: number(v, "total")?,
        items: items
            .iter()
            .map(read_record)
            .collect::<Result<Vec<_>, _>>()?,
        next_cursor: if v["next"].is_null() {
            None
        } else {
            Some(read_cursor(&v["next"])?)
        },
    })
}
