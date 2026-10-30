use super::{codec::*, materials_codec, *};
use fc_domain::{lesson_context::*, schedule::*};
fn cell(c: &CourseCell) -> Value {
    json!({"subject":c.subject_id.get(),"teacher":match c.teacher {TeacherBinding::Explicit(t)=>json!({"id":t.get()}),TeacherBinding::Default=>json!("default"),TeacherBinding::Unassigned=>json!("unassigned")}})
}
fn read_cell(v: &Value) -> Result<CourseCell, StorageError> {
    object_fields(v, &["subject", "teacher"])?;
    let teacher = match v["teacher"].as_str() {
        Some("default") => TeacherBinding::Default,
        Some("unassigned") => TeacherBinding::Unassigned,
        None => {
            object_fields(&v["teacher"], &["id"])?;
            TeacherBinding::Explicit(id(&v["teacher"], "id", PrincipalId::new)?)
        }
        _ => return Err(StorageError::Database),
    };
    Ok(CourseCell {
        subject_id: id(v, "subject", SubjectId::new)?,
        teacher,
    })
}
fn context(c: &EffectiveLessonContext) -> Value {
    json!({"key":materials_codec::lesson(&c.key),"base":cell(&c.base),"effective":cell(&c.effective),"teacher":c.teacher_id.map(|t|t.get()),"from":time(c.effective_from),"until":time(c.effective_until)})
}
fn read_context(v: &Value) -> Result<EffectiveLessonContext, StorageError> {
    object_fields(v, &["key", "base", "effective", "teacher", "from", "until"])?;
    Ok(EffectiveLessonContext {
        key: materials_codec::read_lesson(&v["key"])?,
        base: read_cell(&v["base"])?,
        effective: read_cell(&v["effective"])?,
        teacher_id: if v["teacher"].is_null() {
            None
        } else {
            Some(id(v, "teacher", PrincipalId::new)?)
        },
        effective_from: read_time(&v["from"])?,
        effective_until: read_time(&v["until"])?,
    })
}
pub(super) fn snapshot(s: &LessonContextSnapshot) -> Value {
    json!({"format":2,"revision":s.revision.get(),"identity_revision":s.identity_revision.get(),"observed_at":s.observed_at.map(time),"class":s.class_id.map(|c|c.get()),"blocked_until":s.blocked_until.map(time),"current":s.current.as_ref().map(context),"override":s.active_override.as_ref().map(|o|json!({"id":o.id.get(),"key":materials_codec::lesson(&o.key),"target":cell(&o.target),"actor":o.actor_id.get(),"created":time(o.created_at)})),"history":s.history.iter().map(|h|json!({"context":context(&h.context),"ended":time(h.ended_at)})).collect::<Vec<_>>(),"base_trust":s.base_trust.as_ref().map(|p|json!({"schedule_revision":p.schedule_revision.get(),"identity_revision":p.identity_revision.get(),"class":p.class_id.get(),"since":time(p.trusted_since)}))})
}
pub(super) fn read_snapshot(v: &Value) -> Result<LessonContextSnapshot, StorageError> {
    let format = number(v, "format")?;
    let mut fields = vec![
        "format",
        "revision",
        "identity_revision",
        "observed_at",
        "class",
        "blocked_until",
        "current",
        "override",
        "history",
    ];
    if format == 2 {
        fields.push("base_trust");
    } else if format != 1 {
        return Err(StorageError::Database);
    }
    object_fields(v, &fields)?;
    let history = array(v, "history")?;
    if history.len() > MAX_LESSON_INTERVALS {
        return Err(StorageError::Database);
    };
    let s = LessonContextSnapshot {
        revision: Revision::new(number(v, "revision")?),
        identity_revision: Revision::new(number(v, "identity_revision")?),
        observed_at: if v["observed_at"].is_null() {
            None
        } else {
            Some(read_time(&v["observed_at"])?)
        },
        class_id: if v["class"].is_null() {
            None
        } else {
            Some(id(v, "class", ClassId::new)?)
        },
        blocked_until: if v["blocked_until"].is_null() {
            None
        } else {
            Some(read_time(&v["blocked_until"])?)
        },
        current: if v["current"].is_null() {
            None
        } else {
            Some(read_context(&v["current"])?)
        },
        active_override: if v["override"].is_null() {
            None
        } else {
            let o = &v["override"];
            object_fields(o, &["id", "key", "target", "actor", "created"])?;
            Some(LessonOverrideContext {
                id: id(o, "id", OverrideId::new)?,
                key: materials_codec::read_lesson(&o["key"])?,
                target: read_cell(&o["target"])?,
                actor_id: id(o, "actor", PrincipalId::new)?,
                created_at: read_time(&o["created"])?,
            })
        },
        history: history
            .iter()
            .map(|h| {
                object_fields(h, &["context", "ended"])?;
                Ok(EffectiveLessonInterval {
                    context: read_context(&h["context"])?,
                    ended_at: read_time(&h["ended"])?,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?,
        // Version 1 deliberately has no continuous baseline proof. An old
        // observed_at timestamp must never be upgraded into inferred history.
        base_trust: if format == 1 || v["base_trust"].is_null() {
            None
        } else {
            let p = &v["base_trust"];
            object_fields(
                p,
                &["schedule_revision", "identity_revision", "class", "since"],
            )?;
            Some(BaseLessonTrust {
                schedule_revision: Revision::new(number(p, "schedule_revision")?),
                identity_revision: Revision::new(number(p, "identity_revision")?),
                class_id: id(p, "class", ClassId::new)?,
                trusted_since: read_time(&p["since"])?,
            })
        },
    };
    s.validate().map_err(|_| StorageError::Database)?;
    Ok(s)
}
pub(super) fn action(a: &LessonAction) -> Value {
    match a {
        LessonAction::Replace(c) => json!({"replace":cell(c)}),
        LessonAction::Restore => json!("restore"),
    }
}

fn id<T>(
    v: &Value,
    n: &str,
    make: impl FnOnce(u64) -> Result<T, InvalidId>,
) -> Result<T, StorageError> {
    make(number(v, n)?).map_err(|_| StorageError::Database)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn format_one_remains_unproved_and_format_two_proof_is_strict() {
        let now = fc_domain::time::LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 30).unwrap(),
            3600,
        )
        .unwrap();
        let s = LessonContextSnapshot {
            revision: Revision::new(1),
            identity_revision: Revision::new(3),
            class_id: Some(ClassId::new(1).unwrap()),
            observed_at: Some(now),
            base_trust: Some(BaseLessonTrust {
                schedule_revision: Revision::new(2),
                identity_revision: Revision::new(3),
                class_id: ClassId::new(1).unwrap(),
                trusted_since: now,
            }),
            ..Default::default()
        };
        let encoded = snapshot(&s);
        assert_eq!(read_snapshot(&encoded).unwrap(), s);
        let mut legacy = encoded.clone();
        legacy["format"] = json!(1);
        legacy.as_object_mut().unwrap().remove("base_trust");
        let decoded = read_snapshot(&legacy).unwrap();
        assert!(decoded.base_trust.is_none());
        assert_eq!(decoded.observed_at, Some(now));
        assert_eq!(snapshot(&decoded)["format"], 2);
        let mut smuggled = encoded.clone();
        smuggled["format"] = json!(1);
        assert!(read_snapshot(&smuggled).is_err());
        for (field, value) in [
            ("identity_revision", json!(4)),
            ("class", json!(2)),
            (
                "since",
                time(fc_domain::time::LocalDateTime::new(now.date, 3601).unwrap()),
            ),
        ] {
            let mut corrupt = encoded.clone();
            corrupt["base_trust"][field] = value;
            assert!(
                read_snapshot(&corrupt).is_err(),
                "accepted malformed {field}"
            );
        }
        let mut missing = encoded.clone();
        missing.as_object_mut().unwrap().remove("base_trust");
        assert!(read_snapshot(&missing).is_err());
    }
}
