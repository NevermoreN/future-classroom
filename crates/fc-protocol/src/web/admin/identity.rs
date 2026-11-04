use super::*;
use fc_domain::{auth::Role, identity::*, ids::*};
use serde_json::json;
pub fn role(role: Role) -> &'static str {
    match role {
        Role::GlobalAdmin => "GLOBAL_ADMIN",
        Role::TeacherAdmin => "TEACHER_ADMIN",
        Role::User => "USER",
    }
}
pub fn read_role(role: &str) -> Result<Role, ProtocolError> {
    Ok(match role {
        "GLOBAL_ADMIN" => Role::GlobalAdmin,
        "TEACHER_ADMIN" => Role::TeacherAdmin,
        "USER" => Role::User,
        _ => return Err(ProtocolError::InvalidCommand),
    })
}
pub fn document(state: &IdentityState) -> Result<Vec<u8>, ProtocolError> {
    let school = state
        .installation
        .as_ref()
        .ok_or(ProtocolError::InvalidCommand)?;
    let data = json!({"schoolName":school.school_name,"currentClassId":school.current_class.map(|v|v.get().to_string()),
 "accounts":state.accounts.iter().map(|a|json!({"id":a.principal.id.get().to_string(),"loginName":a.login_name,"displayName":a.display_name,"role":role(a.principal.role),"enabled":a.principal.enabled,"mustChangePassword":a.must_change_password,"scopes":state.assignments.iter().filter(|s|s.teacher_id==a.principal.id&&s.enabled).map(|s|json!({"classId":s.class_id.get().to_string(),"subjectId":s.subject_id.get().to_string()})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
 "classes":state.classes.iter().map(|c|json!({"id":c.id.get().to_string(),"name":c.name,"enabled":c.enabled})).collect::<Vec<_>>(),
 "subjects":state.subjects.iter().map(|s|json!({"id":s.id.get().to_string(),"name":s.name,"color":format!("#{:02x}{:02x}{:02x}",s.color.red,s.color.green,s.color.blue),"enabled":s.enabled,"revision":s.revision.get().to_string()})).collect::<Vec<_>>(),
 "assignments":state.assignments.iter().map(|s|json!({"teacherId":s.teacher_id.get().to_string(),"classId":s.class_id.get().to_string(),"subjectId":s.subject_id.get().to_string(),"enabled":s.enabled})).collect::<Vec<_>>(),
 "defaultTeachers":state.default_teachers.iter().map(|s|json!({"teacherId":s.teacher_id.get().to_string(),"classId":s.class_id.get().to_string(),"subjectId":s.subject_id.get().to_string()})).collect::<Vec<_>>()});
    encode(state.revision, &data)
}
pub enum Change {
    Identity(IdentityCommand),
    Create {
        account: NewAccount,
        password: String,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Class {
    id: String,
    name: String,
    enabled: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Subject {
    id: Option<String>,
    name: String,
    color: String,
    enabled: bool,
    expected_revision: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Scope {
    class_id: String,
    subject_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Create {
    login_name: String,
    display_name: String,
    role: String,
    password: String,
    scopes: Vec<Scope>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Update {
    id: String,
    display_name: String,
    role: String,
    enabled: bool,
    scopes: Vec<Scope>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DefaultTeacherDto {
    teacher_id: String,
    class_id: String,
    subject_id: String,
}
fn scopes(values: Vec<Scope>) -> Result<Vec<AssignmentScope>, ProtocolError> {
    if values.len() > MAX_ASSIGNMENTS {
        return Err(ProtocolError::TooLarge);
    }
    values
        .into_iter()
        .map(|v| {
            Ok(AssignmentScope {
                class_id: ClassId::new(id(&v.class_id)?)
                    .map_err(|_| ProtocolError::InvalidCommand)?,
                subject_id: SubjectId::new(id(&v.subject_id)?)
                    .map_err(|_| ProtocolError::InvalidCommand)?,
            })
        })
        .collect()
}
fn next(values: impl Iterator<Item = u64>) -> Result<u64, ProtocolError> {
    values
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(ProtocolError::InvalidCommand)
}
pub fn mutation(bytes: &[u8], state: &IdentityState) -> Result<(Revision, Change), ProtocolError> {
    let request = Mutation::decode(bytes)?;
    let expected = revision(&request.expected_revision)?;
    let change = match request.action.as_str() {
        "renameSchool" => {
            let (_, c) = request.candidate::<Name>()?;
            Change::Identity(IdentityCommand::RenameSchool { name: c.name })
        }
        "createClass" => {
            let (_, c) = request.candidate::<Name>()?;
            Change::Identity(IdentityCommand::CreateClass {
                id: ClassId::new(next(state.classes.iter().map(|v| v.id.get()))?)
                    .map_err(|_| ProtocolError::InvalidCommand)?,
                name: c.name,
            })
        }
        "updateClass" => {
            let (_, c) = request.candidate::<Class>()?;
            Change::Identity(IdentityCommand::UpdateClass {
                id: ClassId::new(id(&c.id)?).map_err(|_| ProtocolError::InvalidCommand)?,
                name: c.name,
                enabled: c.enabled,
            })
        }
        "bindClass" => {
            let (_, c) = request.candidate::<Id>()?;
            Change::Identity(IdentityCommand::BindCurrentClass {
                id: ClassId::new(id(&c.id)?).map_err(|_| ProtocolError::InvalidCommand)?,
            })
        }
        "putSubject" => {
            let (_, c) = request.candidate::<Subject>()?;
            if c.color.len() != 7
                || !c.color.starts_with('#')
                || !c.color[1..].bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(ProtocolError::InvalidCommand);
            }
            let color = u32::from_str_radix(&c.color[1..], 16)
                .map_err(|_| ProtocolError::InvalidCommand)?;
            Change::Identity(IdentityCommand::PutSubject {
                id: SubjectId::new(match c.id {
                    Some(v) => id(&v)?,
                    None => next(state.subjects.iter().map(|v| v.id.get()))?,
                })
                .map_err(|_| ProtocolError::InvalidCommand)?,
                name: c.name,
                enabled: c.enabled,
                color: Rgb {
                    red: (color >> 16) as u8,
                    green: (color >> 8) as u8,
                    blue: color as u8,
                },
                expected_revision: c.expected_revision.map(|v| revision(&v)).transpose()?,
            })
        }
        "createAccount" => {
            let (_, c) = request.candidate::<Create>()?;
            Change::Create {
                account: NewAccount {
                    id: PrincipalId::new(next(
                        state.accounts.iter().map(|v| v.principal.id.get()),
                    )?)
                    .map_err(|_| ProtocolError::InvalidCommand)?,
                    login_name: c.login_name,
                    display_name: c.display_name,
                    role: read_role(&c.role)?,
                    scopes: scopes(c.scopes)?,
                },
                password: c.password,
            }
        }
        "updateAccount" => {
            let (_, c) = request.candidate::<Update>()?;
            Change::Identity(IdentityCommand::UpdateAccount {
                id: PrincipalId::new(id(&c.id)?).map_err(|_| ProtocolError::InvalidCommand)?,
                display_name: c.display_name,
                role: read_role(&c.role)?,
                enabled: c.enabled,
                scopes: scopes(c.scopes)?,
            })
        }
        "setDefaultTeacher" => {
            let (_, c) = request.candidate::<DefaultTeacherDto>()?;
            Change::Identity(IdentityCommand::SetDefaultTeacher(DefaultTeacher {
                teacher_id: PrincipalId::new(id(&c.teacher_id)?)
                    .map_err(|_| ProtocolError::InvalidCommand)?,
                class_id: ClassId::new(id(&c.class_id)?)
                    .map_err(|_| ProtocolError::InvalidCommand)?,
                subject_id: SubjectId::new(id(&c.subject_id)?)
                    .map_err(|_| ProtocolError::InvalidCommand)?,
            }))
        }
        _ => return Err(ProtocolError::InvalidCommand),
    };
    Ok((expected, change))
}
