use crate::StorageError;
use fc_domain::{auth::*, content::*, ids::*, time::*};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;
use serde_json::{json, Value};
use std::{collections::BTreeSet, fmt};

const MAX_DEPTH: u8 = 32;
const MAX_CONTAINER_ITEMS: usize = 4096;

struct Bounded(u8);
impl<'de> DeserializeSeed<'de> for Bounded {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        if self.0 > MAX_DEPTH {
            return Err(de::Error::custom("storage JSON depth limit"));
        }
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Bounded {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded storage JSON")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        let mut count = 0;
        while sequence.next_element_seed(Bounded(self.0 + 1))?.is_some() {
            count += 1;
            if count > MAX_CONTAINER_ITEMS {
                return Err(de::Error::custom("storage JSON array limit"));
            }
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(de::Error::custom("storage JSON duplicate key"));
            }
            if keys.len() > MAX_CONTAINER_ITEMS {
                return Err(de::Error::custom("storage JSON object limit"));
            }
            map.next_value_seed(Bounded(self.0 + 1))?;
        }
        Ok(())
    }
}

pub(super) fn bytes(value: &Value) -> Result<Vec<u8>, StorageError> {
    serde_json::to_vec(value).map_err(|_| StorageError::Database)
}
/// Keep duplicate keys observable during a bounded first pass, before Value
/// could silently overwrite them. The caller must also bound its BLOB read.
pub(crate) fn value(bytes: &[u8]) -> Result<Value, StorageError> {
    if bytes.len() > crate::MAX_VALUE_BYTES {
        return Err(StorageError::Database);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    Bounded(0)
        .deserialize(&mut deserializer)
        .map_err(|_| StorageError::Database)?;
    deserializer.end().map_err(|_| StorageError::Database)?;
    serde_json::from_slice(bytes).map_err(|_| StorageError::Database)
}
/// Exact required field set. Nullable fields remain mandatory: absence must not
/// be confused with an explicitly stored null by Value's indexing fallback.
pub(crate) fn object_fields(value: &Value, fields: &[&str]) -> Result<(), StorageError> {
    let object = value.as_object().ok_or(StorageError::Database)?;
    if object.len() != fields.len() || fields.iter().any(|name| !object.contains_key(*name)) {
        return Err(StorageError::Database);
    }
    Ok(())
}
pub(super) fn number(value: &Value, name: &str) -> Result<u64, StorageError> {
    value[name].as_u64().ok_or(StorageError::Database)
}
pub(super) fn text(value: &Value, name: &str) -> Result<String, StorageError> {
    value[name]
        .as_str()
        .map(str::to_owned)
        .ok_or(StorageError::Database)
}
pub(super) fn boolean(value: &Value, name: &str) -> Result<bool, StorageError> {
    value[name].as_bool().ok_or(StorageError::Database)
}
pub(super) fn array<'a>(value: &'a Value, name: &str) -> Result<&'a Vec<Value>, StorageError> {
    value[name].as_array().ok_or(StorageError::Database)
}
macro_rules! id {
    ($v:expr,$n:expr,$t:ty) => {
        <$t>::new(number($v, $n)?).map_err(|_| StorageError::Database)?
    };
}
pub(super) fn date(value: LocalDate) -> Value {
    json!([value.year(), value.month(), value.day()])
}
pub(super) fn read_date(value: &Value) -> Result<LocalDate, StorageError> {
    let a = value.as_array().ok_or(StorageError::Database)?;
    if a.len() != 3 {
        return Err(StorageError::Database);
    }
    LocalDate::new(
        i32::try_from(a[0].as_i64().ok_or(StorageError::Database)?)
            .map_err(|_| StorageError::Database)?,
        u8::try_from(a[1].as_u64().ok_or(StorageError::Database)?)
            .map_err(|_| StorageError::Database)?,
        u8::try_from(a[2].as_u64().ok_or(StorageError::Database)?)
            .map_err(|_| StorageError::Database)?,
    )
    .map_err(|_| StorageError::Database)
}
pub(super) fn time(value: LocalDateTime) -> Value {
    json!({"date":date(value.date),"second":value.second_of_day()})
}
pub(super) fn read_time(v: &Value) -> Result<LocalDateTime, StorageError> {
    object_fields(v, &["date", "second"])?;
    LocalDateTime::new(
        read_date(&v["date"])?,
        u32::try_from(number(v, "second")?).map_err(|_| StorageError::Database)?,
    )
    .map_err(|_| StorageError::Database)
}
pub(super) fn key(k: &ContentKey) -> Value {
    match k {
        ContentKey::Homework(k) => {
            json!({"kind":"homework","class":k.class_id.get(),"date":date(k.actual_date),"subject":k.subject_id.get(),"owner":k.owner_teacher_id.get()})
        }
        ContentKey::SharedReminder(k) => {
            json!({"kind":"reminder","class":k.class_id.get(),"date":date(k.actual_date)})
        }
    }
}
pub(super) fn read_key(v: &Value) -> Result<ContentKey, StorageError> {
    match v["kind"].as_str() {
        Some("homework") => object_fields(v, &["kind", "class", "date", "subject", "owner"]),
        Some("reminder") => object_fields(v, &["kind", "class", "date"]),
        _ => Err(StorageError::Database),
    }?;
    Ok(match v["kind"].as_str() {
        Some("homework") => ContentKey::Homework(HomeworkKey {
            class_id: id!(v, "class", ClassId),
            actual_date: read_date(&v["date"])?,
            subject_id: id!(v, "subject", SubjectId),
            owner_teacher_id: id!(v, "owner", PrincipalId),
        }),
        Some("reminder") => ContentKey::SharedReminder(ReminderKey {
            class_id: id!(v, "class", ClassId),
            actual_date: read_date(&v["date"])?,
        }),
        _ => return Err(StorageError::Database),
    })
}
pub(super) fn record(r: &ContentRecord) -> Value {
    json!({"key":key(&r.key),"text":r.text,"revision":r.revision.get(),"actor":r.last_actor_id.get(),"at":time(r.updated_at)})
}
pub(super) fn read_record(v: &Value) -> Result<ContentRecord, StorageError> {
    object_fields(v, &["key", "text", "revision", "actor", "at"])?;
    let record = ContentRecord {
        key: read_key(&v["key"])?,
        text: text(v, "text")?,
        revision: Revision::new(number(v, "revision")?),
        last_actor_id: id!(v, "actor", PrincipalId),
        updated_at: read_time(&v["at"])?,
    };
    let date = match &record.key {
        ContentKey::Homework(k) => k.actual_date,
        ContentKey::SharedReminder(k) => k.actual_date,
    };
    if record.revision == Revision::default()
        || record.text.len() > 16 * 1024
        || record.text.trim() != record.text
        || record.updated_at.date != date
    {
        return Err(StorageError::Database);
    }
    Ok(record)
}

pub(super) fn actor(a: &ActorContext) -> Value {
    json!({"principal":a.principal_id.get(),"school":a.school_id.get(),"epoch":a.auth_epoch.get(),"origin":match a.origin{TrustedOrigin::Local=>"local",TrustedOrigin::Web=>"web"}})
}
pub(super) fn role(role: Role) -> &'static str {
    match role {
        Role::GlobalAdmin => "global",
        Role::TeacherAdmin => "teacher",
        Role::User => "user",
    }
}
pub(super) fn read_role(v: &Value) -> Result<Role, StorageError> {
    match v.as_str() {
        Some("global") => Ok(Role::GlobalAdmin),
        Some("teacher") => Ok(Role::TeacherAdmin),
        Some("user") => Ok(Role::User),
        _ => Err(StorageError::Database),
    }
}
pub(super) fn principal(p: &Principal) -> Value {
    json!({"id":p.id.get(),"school":p.school_id.get(),"role":role(p.role),"enabled":p.enabled,"epoch":p.auth_epoch.get()})
}
pub(super) fn read_principal(v: &Value) -> Result<Principal, StorageError> {
    object_fields(v, &["id", "school", "role", "enabled", "epoch"])?;
    Ok(Principal {
        id: id!(v, "id", PrincipalId),
        school_id: id!(v, "school", SchoolId),
        role: read_role(&v["role"])?,
        enabled: boolean(v, "enabled")?,
        auth_epoch: AuthEpoch::new(number(v, "epoch")?),
    })
}
pub(super) fn assignment(a: &TeachingAssignment) -> Value {
    json!({"teacher":a.teacher_id.get(),"class":a.class_id.get(),"subject":a.subject_id.get(),"enabled":a.enabled})
}
pub(super) fn read_assignment(v: &Value) -> Result<TeachingAssignment, StorageError> {
    object_fields(v, &["teacher", "class", "subject", "enabled"])?;
    Ok(TeachingAssignment {
        teacher_id: id!(v, "teacher", PrincipalId),
        class_id: id!(v, "class", ClassId),
        subject_id: id!(v, "subject", SubjectId),
        enabled: boolean(v, "enabled")?,
    })
}
use fc_domain::identity::*;
pub(super) fn identity(s: &IdentityState) -> Value {
    json!({"format":1,"revision":s.revision.get(),"installation":s.installation.as_ref().map(|i|json!({"school":i.school_id.get(),"name":i.school_name,"class":i.current_class.map(ClassId::get)})),
      "accounts":s.accounts.iter().map(|a|json!({"principal":principal(&a.principal),"login":a.login_name,"display":a.display_name,"change":a.must_change_password,"credential":a.credential_version,"recovery":a.recovery_version})).collect::<Vec<_>>(),
      "classes":s.classes.iter().map(|c|json!({"id":c.id.get(),"school":c.school_id.get(),"name":c.name,"enabled":c.enabled})).collect::<Vec<_>>(),
      "subjects":s.subjects.iter().map(|c|json!({"id":c.id.get(),"school":c.school_id.get(),"name":c.name,"enabled":c.enabled,"color":[c.color.red,c.color.green,c.color.blue],"revision":c.revision.get()})).collect::<Vec<_>>(),
      "assignments":s.assignments.iter().map(assignment).collect::<Vec<_>>(),
      "defaults":s.default_teachers.iter().map(|d|json!({"class":d.class_id.get(),"subject":d.subject_id.get(),"teacher":d.teacher_id.get()})).collect::<Vec<_>>()})
}
pub(super) fn read_identity(v: &Value) -> Result<IdentityState, StorageError> {
    object_fields(
        v,
        &[
            "format",
            "revision",
            "installation",
            "accounts",
            "classes",
            "subjects",
            "assignments",
            "defaults",
        ],
    )?;
    if number(v, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let installation = if v["installation"].is_null() {
        None
    } else {
        let i = &v["installation"];
        object_fields(i, &["school", "name", "class"])?;
        Some(Installation {
            school_id: id!(i, "school", SchoolId),
            school_name: text(i, "name")?,
            current_class: if i["class"].is_null() {
                None
            } else {
                Some(ClassId::new(number(i, "class")?).map_err(|_| StorageError::Database)?)
            },
        })
    };
    let accounts = array(v, "accounts")?
        .iter()
        .map(|a| {
            object_fields(
                a,
                &[
                    "principal",
                    "login",
                    "display",
                    "change",
                    "credential",
                    "recovery",
                ],
            )?;
            Ok(UserAccount {
                principal: read_principal(&a["principal"])?,
                login_name: text(a, "login")?,
                display_name: text(a, "display")?,
                must_change_password: boolean(a, "change")?,
                credential_version: number(a, "credential")?,
                recovery_version: number(a, "recovery")?,
            })
        })
        .collect::<Result<_, StorageError>>()?;
    let classes = array(v, "classes")?
        .iter()
        .map(|c| {
            object_fields(c, &["id", "school", "name", "enabled"])?;
            Ok(SchoolClass {
                id: id!(c, "id", ClassId),
                school_id: id!(c, "school", SchoolId),
                name: text(c, "name")?,
                enabled: boolean(c, "enabled")?,
            })
        })
        .collect::<Result<_, StorageError>>()?;
    let subjects = array(v, "subjects")?
        .iter()
        .map(|c| {
            object_fields(c, &["id", "school", "name", "enabled", "color", "revision"])?;
            let color = c["color"].as_array().ok_or(StorageError::Database)?;
            if color.len() != 3 {
                return Err(StorageError::Database);
            }
            let channel = |i: usize| {
                u8::try_from(color[i].as_u64().ok_or(StorageError::Database)?)
                    .map_err(|_| StorageError::Database)
            };
            Ok(SchoolSubject {
                id: id!(c, "id", SubjectId),
                school_id: id!(c, "school", SchoolId),
                name: text(c, "name")?,
                enabled: boolean(c, "enabled")?,
                revision: Revision::new(number(c, "revision")?),
                color: Rgb {
                    red: channel(0)?,
                    green: channel(1)?,
                    blue: channel(2)?,
                },
            })
        })
        .collect::<Result<_, StorageError>>()?;
    let assignments = array(v, "assignments")?
        .iter()
        .map(read_assignment)
        .collect::<Result<_, StorageError>>()?;
    let default_teachers = array(v, "defaults")?
        .iter()
        .map(|d| {
            object_fields(d, &["class", "subject", "teacher"])?;
            Ok(DefaultTeacher {
                class_id: id!(d, "class", ClassId),
                subject_id: id!(d, "subject", SubjectId),
                teacher_id: id!(d, "teacher", PrincipalId),
            })
        })
        .collect::<Result<_, StorageError>>()?;
    let state = IdentityState {
        revision: Revision::new(number(v, "revision")?),
        installation,
        accounts,
        classes,
        subjects,
        assignments,
        default_teachers,
    };
    state.validate().map_err(|_| StorageError::Database)?;
    Ok(state)
}

#[cfg(test)]
#[path = "tests/json.rs"]
mod json_tests;
