//! Exhaustive, schema-directed cross-installation reference mapping.
use super::*;
use fc_domain::{
    layout::LayoutScope,
    materials::MaterialScope,
    profile::SubjectProfileKey,
    profile_content::*,
    recovery::*,
    schedule::{CourseCell, TeacherBinding},
};
use sha2::{Digest, Sha256};
mod documents;
pub(crate) use documents::map_document;
fn invalid<T>(_: T) -> StorageError {
    StorageError::RecoveryInvalidPackage
}
pub(crate) fn catalog_allocation(c: &MigrationCatalog) -> usize {
    c.school_name.capacity()
        + c.classes.capacity() * std::mem::size_of::<MigrationClassLabel>()
        + c.subjects.capacity() * std::mem::size_of::<MigrationSubjectLabel>()
        + c.principals.capacity() * std::mem::size_of::<MigrationPrincipalLabel>()
        + c.default_teachers.capacity() * std::mem::size_of::<DefaultTeacher>()
        + c.classes.iter().map(|x| x.name.capacity()).sum::<usize>()
        + c.subjects.iter().map(|x| x.name.capacity()).sum::<usize>()
        + c.principals
            .iter()
            .map(|x| x.display_name.capacity())
            .sum::<usize>()
}
pub(crate) fn read_catalog(v: &Value) -> Result<MigrationCatalog, StorageError> {
    codec::object_fields(
        v,
        &[
            "format",
            "school",
            "name",
            "class",
            "classes",
            "subjects",
            "defaults",
            "account_labels",
        ],
    )?;
    if codec::number(v, "format")? != 1 {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    if codec::array(v, "classes")?.len() > MAX_CLASSES
        || codec::array(v, "subjects")?.len() > MAX_SUBJECTS
        || codec::array(v, "account_labels")?.len() > MAX_ACCOUNTS
        || codec::array(v, "defaults")?.len() > MAX_ASSIGNMENTS
    {
        return Err(StorageError::ResourceQuota);
    }
    let id = |v: &Value, k: &str| codec::number(v, k);
    let c = MigrationCatalog {
        school_id: SchoolId::new(id(v, "school")?).map_err(invalid)?,
        school_name: codec::text(v, "name")?,
        current_class: if v["class"].is_null() {
            None
        } else {
            Some(ClassId::new(id(v, "class")?).map_err(invalid)?)
        },
        classes: codec::array(v, "classes")?
            .iter()
            .map(|x| {
                codec::object_fields(x, &["id", "name"])?;
                Ok(MigrationClassLabel {
                    id: ClassId::new(id(x, "id")?).map_err(invalid)?,
                    name: codec::text(x, "name")?,
                })
            })
            .collect::<Result<_, StorageError>>()?,
        subjects: codec::array(v, "subjects")?
            .iter()
            .map(|x| {
                codec::object_fields(x, &["id", "name", "color"])?;
                let a = codec::array(x, "color")?;
                if a.len() != 3 {
                    return Err(StorageError::RecoveryInvalidPackage);
                };
                let ch = |n: usize| {
                    u8::try_from(a[n].as_u64().ok_or(StorageError::RecoveryInvalidPackage)?)
                        .map_err(invalid)
                };
                Ok(MigrationSubjectLabel {
                    id: SubjectId::new(id(x, "id")?).map_err(invalid)?,
                    name: codec::text(x, "name")?,
                    color: Rgb {
                        red: ch(0)?,
                        green: ch(1)?,
                        blue: ch(2)?,
                    },
                })
            })
            .collect::<Result<_, StorageError>>()?,
        principals: codec::array(v, "account_labels")?
            .iter()
            .map(|x| {
                codec::object_fields(x, &["id", "display_name"])?;
                Ok(MigrationPrincipalLabel {
                    id: PrincipalId::new(id(x, "id")?).map_err(invalid)?,
                    display_name: codec::text(x, "display_name")?,
                })
            })
            .collect::<Result<_, StorageError>>()?,
        default_teachers: codec::array(v, "defaults")?
            .iter()
            .map(|x| {
                codec::object_fields(x, &["class", "subject", "teacher"])?;
                Ok(DefaultTeacher {
                    class_id: ClassId::new(id(x, "class")?).map_err(invalid)?,
                    subject_id: SubjectId::new(id(x, "subject")?).map_err(invalid)?,
                    teacher_id: PrincipalId::new(id(x, "teacher")?).map_err(invalid)?,
                })
            })
            .collect::<Result<_, StorageError>>()?,
    };
    c.validate().map_err(invalid)?;
    Ok(c)
}
pub(crate) fn mapped_identity(
    source: &MigrationCatalog,
    current: &IdentityState,
    m: &MigrationMapping,
) -> Result<IdentityState, StorageError> {
    m.validate(source, current)
        .map_err(|_| StorageError::RecoveryConflict)?;
    let mut i = current.clone();
    i.installation
        .as_mut()
        .ok_or(StorageError::RecoveryConflict)?
        .current_class = source
        .current_class
        .map(|c| m.class(c).map_err(invalid))
        .transpose()?;
    i.default_teachers = source
        .default_teachers
        .iter()
        .map(|d| {
            Ok(DefaultTeacher {
                class_id: m.class(d.class_id).map_err(invalid)?,
                subject_id: m.subject(d.subject_id).map_err(invalid)?,
                teacher_id: m.principal(d.teacher_id).map_err(invalid)?,
            })
        })
        .collect::<Result<_, StorageError>>()?;
    i.validate().map_err(StorageError::Identity)?;
    Ok(i)
}
pub(crate) fn mapping_sha256(m: &MigrationMapping) -> Result<[u8; 32], StorageError> {
    if m.classes.len() > MAX_CLASSES
        || m.subjects.len() > MAX_SUBJECTS
        || m.principals.len() > MAX_ACCOUNTS
    {
        return Err(StorageError::ResourceQuota);
    }
    fn unique<T: Copy + Ord>(p: &[(T, T)]) -> bool {
        let mut from = std::collections::BTreeSet::new();
        let mut to = std::collections::BTreeSet::new();
        p.iter().all(|(a, b)| from.insert(*a) && to.insert(*b))
    }
    if !unique(&m.classes) || !unique(&m.subjects) || !unique(&m.principals) {
        return Err(StorageError::RecoveryConflict);
    }
    let mut h = Sha256::new();
    h.update(b"FutureClassroom migration mapping v1\0");
    h.update(m.source_school_id.get().to_be_bytes());
    h.update(m.target_school_id.get().to_be_bytes());
    fn add(h: &mut Sha256, mut p: Vec<(u64, u64)>) {
        p.sort_unstable();
        h.update((p.len() as u64).to_be_bytes());
        for (a, b) in p {
            h.update(a.to_be_bytes());
            h.update(b.to_be_bytes());
        }
    }
    add(
        &mut h,
        m.classes.iter().map(|(a, b)| (a.get(), b.get())).collect(),
    );
    add(
        &mut h,
        m.subjects.iter().map(|(a, b)| (a.get(), b.get())).collect(),
    );
    add(
        &mut h,
        m.principals
            .iter()
            .map(|(a, b)| (a.get(), b.get()))
            .collect(),
    );
    Ok(h.finalize().into())
}
fn profile(k: SubjectProfileKey, m: &MigrationMapping) -> Result<SubjectProfileKey, StorageError> {
    Ok(SubjectProfileKey {
        class_id: m.class(k.class_id).map_err(invalid)?,
        subject_id: m.subject(k.subject_id).map_err(invalid)?,
        teacher_id: m.principal(k.teacher_id).map_err(invalid)?,
    })
}
fn slot(s: ContentSlotKey, m: &MigrationMapping) -> Result<ContentSlotKey, StorageError> {
    Ok(ContentSlotKey {
        scope: match s.scope {
            LayoutScope::Public => LayoutScope::Public,
            LayoutScope::Subject(k) => LayoutScope::Subject(profile(k, m)?),
        },
        ..s
    })
}
fn material(s: MaterialScope, m: &MigrationMapping) -> Result<MaterialScope, StorageError> {
    Ok(MaterialScope {
        teacher_id: m.principal(s.teacher_id).map_err(invalid)?,
        subject_id: m.subject(s.subject_id).map_err(invalid)?,
    })
}
fn course(c: &mut CourseCell, m: &MigrationMapping) -> Result<(), StorageError> {
    c.subject_id = m.subject(c.subject_id).map_err(invalid)?;
    if let TeacherBinding::Explicit(t) = &mut c.teacher {
        *t = m.principal(*t).map_err(invalid)?;
    }
    Ok(())
}
