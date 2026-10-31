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
fn initiator(source: LessonInitiator) -> Value {
    match source {
        LessonInitiator::Authenticated(id) => json!({"kind":"authenticated","principal":id.get()}),
        LessonInitiator::LocalDesktop => json!({"kind":"local-desktop"}),
    }
}
fn read_initiator(v: &Value) -> Result<LessonInitiator, StorageError> {
    match v["kind"].as_str() {
        Some("authenticated") => {
            object_fields(v, &["kind", "principal"])?;
            Ok(LessonInitiator::Authenticated(id(
                v,
                "principal",
                PrincipalId::new,
            )?))
        }
        Some("local-desktop") => {
            object_fields(v, &["kind"])?;
            Ok(LessonInitiator::LocalDesktop)
        }
        _ => Err(StorageError::Database),
    }
}
pub(super) fn snapshot(s: &LessonContextSnapshot) -> Value {
    json!({"format":3,"revision":s.revision.get(),"identity_revision":s.identity_revision.get(),"observed_at":s.observed_at.map(time),"class":s.class_id.map(|c|c.get()),"blocked_until":s.blocked_until.map(time),"current":s.current.as_ref().map(context),"override":s.active_override.as_ref().map(|o|json!({"id":o.id.get(),"key":materials_codec::lesson(&o.key),"target":cell(&o.target),"initiator":initiator(o.initiator),"created":time(o.created_at)})),"history":s.history.iter().map(|h|json!({"context":context(&h.context),"ended":time(h.ended_at)})).collect::<Vec<_>>(),"base_trust":s.base_trust.as_ref().map(|p|json!({"schedule_revision":p.schedule_revision.get(),"identity_revision":p.identity_revision.get(),"class":p.class_id.get(),"since":time(p.trusted_since)}))})
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
    if format == 2 || format == 3 {
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
            if format == 3 {
                object_fields(o, &["id", "key", "target", "initiator", "created"])?;
            } else {
                object_fields(o, &["id", "key", "target", "actor", "created"])?;
            }
            Some(LessonOverrideContext {
                id: id(o, "id", OverrideId::new)?,
                key: materials_codec::read_lesson(&o["key"])?,
                target: read_cell(&o["target"])?,
                initiator: if format == 3 {
                    read_initiator(&o["initiator"])?
                } else {
                    LessonInitiator::Authenticated(id(o, "actor", PrincipalId::new)?)
                },
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
        assert_eq!(snapshot(&decoded)["format"], 3);
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

#[cfg(test)]
mod initiator_tests {
    use super::*;
    fn example(initiator: LessonInitiator) -> LessonContextSnapshot {
        use fc_domain::time::*;
        let date = LocalDate::new(2026, 10, 30).unwrap();
        let at = LocalDateTime::new(date, 8 * 3600 + 30 * 60).unwrap();
        let key = LessonInstanceKey {
            actual_date: date,
            class_id: ClassId::new(1).unwrap(),
            range: MinuteRange::new(Minute::new(480).unwrap(), Minute::new(540).unwrap()).unwrap(),
            version: ResolvedScheduleVersion {
                actual_semester_id: SemesterId::new(1).unwrap(),
                actual_revision: Revision::new(1),
                source_semester_id: SemesterId::new(1).unwrap(),
                source_revision: Revision::new(1),
                calendar_mode: CalendarMode::School,
                calendar_mode_revision: Revision::new(1),
                applied_calendar_revision: Revision::new(1),
                makeup_mapping_revision: Revision::new(1),
                source_table_id: TableId::new(1).unwrap(),
                source_row_id: RowId::new(1).unwrap(),
                rules_revision: Revision::new(1),
            },
        };
        let cell = CourseCell {
            subject_id: SubjectId::new(1).unwrap(),
            teacher: TeacherBinding::Unassigned,
        };
        LessonContextSnapshot {
            revision: Revision::new(3),
            identity_revision: Revision::new(5),
            observed_at: Some(at),
            class_id: Some(key.class_id),
            current: Some(EffectiveLessonContext {
                key: key.clone(),
                base: cell.clone(),
                effective: cell.clone(),
                teacher_id: None,
                effective_from: at,
                effective_until: LocalDateTime::new(date, 9 * 3600).unwrap(),
            }),
            active_override: Some(LessonOverrideContext {
                id: OverrideId::new(9).unwrap(),
                key,
                target: cell,
                initiator,
                created_at: at,
            }),
            ..Default::default()
        }
    }
    #[test]
    fn strict_format_three_roundtrips_both_honest_initiators_and_old_formats_keep_admin() {
        for initiator in [
            LessonInitiator::Authenticated(PrincipalId::new(7).unwrap()),
            LessonInitiator::LocalDesktop,
        ] {
            let s = example(initiator);
            s.validate().unwrap();
            let encoded = snapshot(&s);
            assert_eq!(encoded["format"], 3);
            assert_eq!(read_snapshot(&encoded).unwrap(), s);
            assert!(encoded["override"].get("actor").is_none());
        }
        for format in [1, 2] {
            let mut legacy = snapshot(&example(LessonInitiator::Authenticated(
                PrincipalId::new(7).unwrap(),
            )));
            legacy["format"] = json!(format);
            let object = legacy["override"].as_object_mut().unwrap();
            object.remove("initiator");
            object.insert("actor".into(), json!(7));
            if format == 1 {
                legacy.as_object_mut().unwrap().remove("base_trust");
            }
            assert_eq!(
                read_snapshot(&legacy)
                    .unwrap()
                    .active_override
                    .unwrap()
                    .initiator,
                LessonInitiator::Authenticated(PrincipalId::new(7).unwrap())
            );
        }
    }
    #[test]
    fn malformed_mixed_or_guessed_initiators_never_become_desktop_authority() {
        let good = snapshot(&example(LessonInitiator::LocalDesktop));
        for source in [
            Value::Null,
            json!({}),
            json!({"kind":"unknown"}),
            json!({"kind":"authenticated"}),
            json!({"kind":"authenticated","principal":0}),
            json!({"kind":"local-desktop","principal":7}),
            json!({"kind":"local-desktop","extra":true}),
        ] {
            let mut bad = good.clone();
            bad["override"]["initiator"] = source;
            assert!(read_snapshot(&bad).is_err());
        }
        let mut mixed = good.clone();
        mixed["override"]["actor"] = json!(7);
        assert!(read_snapshot(&mixed).is_err());
        let mut missing = good.clone();
        missing["override"]
            .as_object_mut()
            .unwrap()
            .remove("initiator");
        assert!(read_snapshot(&missing).is_err());
        let mut legacy = good.clone();
        legacy["format"] = json!(2);
        assert!(read_snapshot(&legacy).is_err());
    }
}
