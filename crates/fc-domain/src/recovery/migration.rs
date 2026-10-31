//! Cross-installation references are explicit choices, never authority.
use crate::{identity::*, ids::*};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationClassLabel {
    pub id: ClassId,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationSubjectLabel {
    pub id: SubjectId,
    pub name: String,
    pub color: Rgb,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationPrincipalLabel {
    pub id: PrincipalId,
    pub display_name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationCatalog {
    pub school_id: SchoolId,
    pub school_name: String,
    pub current_class: Option<ClassId>,
    pub classes: Vec<MigrationClassLabel>,
    pub subjects: Vec<MigrationSubjectLabel>,
    pub principals: Vec<MigrationPrincipalLabel>,
    pub default_teachers: Vec<DefaultTeacher>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationMapping {
    pub source_school_id: SchoolId,
    pub target_school_id: SchoolId,
    pub classes: Vec<(ClassId, ClassId)>,
    pub subjects: Vec<(SubjectId, SubjectId)>,
    pub principals: Vec<(PrincipalId, PrincipalId)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationError {
    InvalidCatalog,
    Capacity,
    WrongSchool,
    IncompleteMapping,
    DuplicateMapping,
    MissingTarget,
    CurrentAuthorization,
}
fn text(s: &str, max: usize) -> bool {
    normalized_name(s, max).is_ok()
}
impl MigrationCatalog {
    pub fn validate(&self) -> Result<(), MigrationError> {
        use MigrationError as E;
        if self.classes.len() > MAX_CLASSES
            || self.subjects.len() > MAX_SUBJECTS
            || self.principals.len() > MAX_ACCOUNTS
            || self.default_teachers.len() > MAX_ASSIGNMENTS
        {
            return Err(E::Capacity);
        }
        if !text(&self.school_name, 128) || self.principals.is_empty() {
            return Err(E::InvalidCatalog);
        }
        let classes: BTreeSet<_> = self.classes.iter().map(|x| x.id).collect();
        let subjects: BTreeSet<_> = self.subjects.iter().map(|x| x.id).collect();
        let principals: BTreeSet<_> = self.principals.iter().map(|x| x.id).collect();
        if classes.len() != self.classes.len()
            || subjects.len() != self.subjects.len()
            || principals.len() != self.principals.len()
            || self.classes.iter().any(|x| !text(&x.name, 128))
            || self.subjects.iter().any(|x| !text(&x.name, 64))
            || self.principals.iter().any(|x| !text(&x.display_name, 128))
            || self.current_class.is_some_and(|c| !classes.contains(&c))
        {
            return Err(E::InvalidCatalog);
        }
        let names: BTreeSet<_> = self
            .classes
            .iter()
            .map(|x| normalized_name(&x.name, 128).unwrap_or_default())
            .collect();
        let subject_names: BTreeSet<_> = self
            .subjects
            .iter()
            .map(|x| normalized_name(&x.name, 64).unwrap_or_default())
            .collect();
        if names.len() != self.classes.len() || subject_names.len() != self.subjects.len() {
            return Err(E::InvalidCatalog);
        }
        let mut defaults = BTreeSet::new();
        if self.default_teachers.iter().any(|d| {
            !classes.contains(&d.class_id)
                || !subjects.contains(&d.subject_id)
                || !principals.contains(&d.teacher_id)
                || !defaults.insert((d.class_id, d.subject_id))
        }) {
            return Err(E::InvalidCatalog);
        }
        Ok(())
    }
}
fn complete<T: Copy + Ord>(
    pairs: &[(T, T)],
    source: impl Iterator<Item = T>,
    target: impl Iterator<Item = T>,
) -> Result<(), MigrationError> {
    let source: BTreeSet<_> = source.collect();
    let target: BTreeSet<_> = target.collect();
    let mut from = BTreeSet::new();
    let mut to = BTreeSet::new();
    for (a, b) in pairs {
        if !from.insert(*a) || !to.insert(*b) {
            return Err(MigrationError::DuplicateMapping);
        }
        if !target.contains(b) {
            return Err(MigrationError::MissingTarget);
        }
    }
    if from != source {
        return Err(MigrationError::IncompleteMapping);
    }
    Ok(())
}
impl MigrationMapping {
    pub fn validate(
        &self,
        source: &MigrationCatalog,
        target: &IdentityState,
    ) -> Result<(), MigrationError> {
        source.validate()?;
        target
            .validate()
            .map_err(|_| MigrationError::MissingTarget)?;
        if self.classes.len() != source.classes.len()
            || self.subjects.len() != source.subjects.len()
            || self.principals.len() != source.principals.len()
        {
            return Err(MigrationError::IncompleteMapping);
        }
        if self.source_school_id != source.school_id
            || target.installation.as_ref().map(|i| i.school_id) != Some(self.target_school_id)
        {
            return Err(MigrationError::WrongSchool);
        }
        complete(
            &self.classes,
            source.classes.iter().map(|x| x.id),
            target.classes.iter().map(|x| x.id),
        )?;
        complete(
            &self.subjects,
            source.subjects.iter().map(|x| x.id),
            target.subjects.iter().map(|x| x.id),
        )?;
        complete(
            &self.principals,
            source.principals.iter().map(|x| x.id),
            target.accounts.iter().map(|x| x.principal.id),
        )?;
        if let Some(c) = source.current_class {
            let c = self.class(c)?;
            if !target.classes.iter().any(|x| x.id == c && x.enabled) {
                return Err(MigrationError::CurrentAuthorization);
            }
        }
        for d in &source.default_teachers {
            if !target.is_teacher(
                self.principal(d.teacher_id)?,
                self.class(d.class_id)?,
                self.subject(d.subject_id)?,
            ) {
                return Err(MigrationError::CurrentAuthorization);
            }
        }
        Ok(())
    }
    pub fn class(&self, id: ClassId) -> Result<ClassId, MigrationError> {
        self.classes
            .iter()
            .find(|x| x.0 == id)
            .map(|x| x.1)
            .ok_or(MigrationError::IncompleteMapping)
    }
    pub fn subject(&self, id: SubjectId) -> Result<SubjectId, MigrationError> {
        self.subjects
            .iter()
            .find(|x| x.0 == id)
            .map(|x| x.1)
            .ok_or(MigrationError::IncompleteMapping)
    }
    pub fn principal(&self, id: PrincipalId) -> Result<PrincipalId, MigrationError> {
        self.principals
            .iter()
            .find(|x| x.0 == id)
            .map(|x| x.1)
            .ok_or(MigrationError::IncompleteMapping)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::*;
    fn state() -> IdentityState {
        let school = SchoolId::new(9).unwrap();
        IdentityState {
            revision: Revision::new(1),
            installation: Some(Installation {
                school_id: school,
                school_name: "current school".into(),
                current_class: Some(ClassId::new(20).unwrap()),
            }),
            accounts: vec![UserAccount {
                principal: Principal {
                    id: PrincipalId::new(40).unwrap(),
                    school_id: school,
                    role: Role::GlobalAdmin,
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: "current".into(),
                display_name: "current name".into(),
                must_change_password: true,
                credential_version: 1,
                recovery_version: 1,
            }],
            classes: vec![SchoolClass {
                id: ClassId::new(20).unwrap(),
                school_id: school,
                name: "current class".into(),
                enabled: true,
            }],
            subjects: vec![SchoolSubject {
                id: SubjectId::new(30).unwrap(),
                school_id: school,
                name: "current subject".into(),
                enabled: true,
                color: Rgb {
                    red: 0,
                    green: 0,
                    blue: 0,
                },
                revision: Revision::new(1),
            }],
            assignments: vec![TeachingAssignment {
                teacher_id: PrincipalId::new(40).unwrap(),
                class_id: ClassId::new(20).unwrap(),
                subject_id: SubjectId::new(30).unwrap(),
                enabled: true,
            }],
            default_teachers: vec![],
        }
    }
    fn source() -> MigrationCatalog {
        MigrationCatalog {
            school_id: SchoolId::new(1).unwrap(),
            school_name: "source school".into(),
            current_class: Some(ClassId::new(1).unwrap()),
            classes: vec![MigrationClassLabel {
                id: ClassId::new(1).unwrap(),
                name: "source class".into(),
            }],
            subjects: vec![MigrationSubjectLabel {
                id: SubjectId::new(1).unwrap(),
                name: "source subject".into(),
                color: Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
            }],
            principals: vec![MigrationPrincipalLabel {
                id: PrincipalId::new(1).unwrap(),
                display_name: "source name".into(),
            }],
            default_teachers: vec![DefaultTeacher {
                class_id: ClassId::new(1).unwrap(),
                subject_id: SubjectId::new(1).unwrap(),
                teacher_id: PrincipalId::new(1).unwrap(),
            }],
        }
    }
    fn mapping() -> MigrationMapping {
        MigrationMapping {
            source_school_id: SchoolId::new(1).unwrap(),
            target_school_id: SchoolId::new(9).unwrap(),
            classes: vec![(ClassId::new(1).unwrap(), ClassId::new(20).unwrap())],
            subjects: vec![(SubjectId::new(1).unwrap(), SubjectId::new(30).unwrap())],
            principals: vec![(PrincipalId::new(1).unwrap(), PrincipalId::new(40).unwrap())],
        }
    }
    #[test]
    fn explicit_complete_mapping_uses_actual_current_authority() {
        let s = source();
        let mut t = state();
        let m = mapping();
        assert_eq!(m.validate(&s, &t), Ok(()));
        t.assignments.clear();
        assert_eq!(
            m.validate(&s, &t),
            Err(MigrationError::CurrentAuthorization)
        );
        t.assignments = state().assignments;
        t.classes[0].enabled = false;
        t.installation.as_mut().unwrap().current_class = None;
        assert_eq!(
            m.validate(&s, &t),
            Err(MigrationError::CurrentAuthorization)
        );
    }
    #[test]
    fn numeric_ids_or_equal_labels_never_fill_missing_choices() {
        let s = source();
        let t = state();
        let mut m = mapping();
        m.classes.clear();
        assert_eq!(m.validate(&s, &t), Err(MigrationError::IncompleteMapping));
        m = mapping();
        m.classes[0].1 = ClassId::new(1).unwrap();
        assert_eq!(m.validate(&s, &t), Err(MigrationError::MissingTarget));
        m = mapping();
        m.classes[0].0 = ClassId::new(8).unwrap();
        assert_eq!(m.validate(&s, &t), Err(MigrationError::IncompleteMapping));
    }
    #[test]
    fn duplicate_target_and_unknown_catalog_reference_are_rejected() {
        let mut s = source();
        s.classes.push(MigrationClassLabel {
            id: ClassId::new(2).unwrap(),
            name: "second".into(),
        });
        let mut m = mapping();
        m.classes
            .push((ClassId::new(2).unwrap(), ClassId::new(20).unwrap()));
        assert_eq!(
            m.validate(&s, &state()),
            Err(MigrationError::DuplicateMapping)
        );
        s.default_teachers[0].teacher_id = PrincipalId::new(99).unwrap();
        assert_eq!(s.validate(), Err(MigrationError::InvalidCatalog));
    }
    #[test]
    fn historical_label_can_map_to_disabled_existing_account_without_creating_authority() {
        let mut s = source();
        s.current_class = None;
        s.default_teachers.clear();
        let mut t = state();
        let mut disabled = t.accounts[0].clone();
        disabled.principal.id = PrincipalId::new(41).unwrap();
        disabled.principal.enabled = false;
        disabled.principal.role = Role::User;
        disabled.login_name = "disabled".into();
        t.accounts.push(disabled);
        let mut m = mapping();
        m.principals[0].1 = PrincipalId::new(41).unwrap();
        assert_eq!(m.validate(&s, &t), Ok(()));
        assert!(!t.is_teacher(
            PrincipalId::new(41).unwrap(),
            ClassId::new(20).unwrap(),
            SubjectId::new(30).unwrap()
        ));
    }
}
