use super::*;
use crate::{auth::*, identity::*};
fn principal(id: u64) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn state() -> IdentityState {
    let school = SchoolId::new(1).unwrap();
    IdentityState {
        revision: Revision::new(1),
        installation: Some(Installation {
            school_id: school,
            school_name: "测试学校".into(),
            current_class: Some(ClassId::new(1).unwrap()),
        }),
        accounts: (1..=4)
            .map(|id| UserAccount {
                principal: Principal {
                    id: principal(id),
                    school_id: school,
                    role: match id {
                        1 => Role::GlobalAdmin,
                        4 => Role::User,
                        _ => Role::TeacherAdmin,
                    },
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("user{id}"),
                display_name: format!("账号{id}"),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            })
            .collect(),
        classes: vec![SchoolClass {
            id: ClassId::new(1).unwrap(),
            school_id: school,
            name: "一班".into(),
            enabled: true,
        }],
        subjects: vec![SchoolSubject {
            id: SubjectId::new(1).unwrap(),
            school_id: school,
            name: "语文".into(),
            enabled: true,
            color: Rgb {
                red: 20,
                green: 40,
                blue: 60,
            },
            revision: Revision::new(1),
        }],
        assignments: [2, 3]
            .into_iter()
            .map(|id| TeachingAssignment {
                teacher_id: principal(id),
                class_id: ClassId::new(1).unwrap(),
                subject_id: SubjectId::new(1).unwrap(),
                enabled: true,
            })
            .collect(),
        default_teachers: vec![],
    }
}
#[test]
fn actorless_desktop_uses_honest_origin_and_same_key_history_restore_rules() {
    let mut identity = state();
    identity.revision = Revision::new(5);
    let mut extra = identity.subjects[0].clone();
    extra.id = SubjectId::new(2).unwrap();
    extra.name = "数学".into();
    identity.subjects.push(extra);
    let original = evidence();
    let key = original.current.as_ref().unwrap().key.clone();
    let action = LessonAction::Replace(CourseCell {
        subject_id: SubjectId::new(2).unwrap(),
        teacher: TeacherBinding::Unassigned,
    });
    let saved = original
        .apply_local_desktop(
            original.revision,
            &key,
            action,
            OverrideId::new(9).unwrap(),
            &identity,
            at(8 * 3600 + 31 * 60),
        )
        .unwrap();
    assert_eq!(
        saved.active_override.as_ref().unwrap().initiator,
        LessonInitiator::LocalDesktop
    );
    assert_eq!(saved.current.as_ref().unwrap().key, key);
    assert_eq!(
        saved.current.as_ref().unwrap().effective_until,
        original.current.as_ref().unwrap().effective_until
    );
    assert_eq!(saved.history.len(), original.history.len() + 1);
    let restored = saved
        .apply_local_desktop(
            saved.revision,
            &key,
            LessonAction::Restore,
            OverrideId::new(10).unwrap(),
            &identity,
            at(8 * 3600 + 32 * 60),
        )
        .unwrap();
    assert!(restored.active_override.is_none());
    assert_eq!(
        restored.current.as_ref().unwrap().effective,
        original.current.as_ref().unwrap().base
    );
    assert!(saved
        .apply_local_desktop(
            original.revision,
            &key,
            LessonAction::Restore,
            OverrideId::new(11).unwrap(),
            &identity,
            at(8 * 3600 + 32 * 60)
        )
        .is_err());
    assert!(saved
        .apply_local_desktop(
            saved.revision,
            &key,
            LessonAction::Restore,
            OverrideId::new(11).unwrap(),
            &identity,
            at(9 * 3600)
        )
        .is_err());
    let mut other = identity.clone();
    other.installation.as_mut().unwrap().current_class = None;
    assert!(saved
        .apply_local_desktop(
            saved.revision,
            &key,
            LessonAction::Restore,
            OverrideId::new(11).unwrap(),
            &other,
            at(8 * 3600 + 32 * 60)
        )
        .is_err());
}
#[test]
fn management_keeps_exact_local_admin_authority_and_authenticated_audit() {
    let mut identity = state();
    identity.revision = Revision::new(5);
    let original = evidence();
    let key = original.current.as_ref().unwrap().key.clone();
    let admin = ActorContext {
        principal_id: principal(1),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: at(10 * 3600),
        origin: TrustedOrigin::Local,
    };
    let target = original.current.as_ref().unwrap().base.clone();
    let action = LessonAction::Replace(target);
    let saved = original
        .apply(
            original.revision,
            &key,
            action.clone(),
            OverrideId::new(9).unwrap(),
            &identity,
            &admin,
            at(8 * 3600 + 31 * 60),
        )
        .unwrap();
    assert_eq!(
        saved.active_override.unwrap().initiator,
        LessonInitiator::Authenticated(principal(1))
    );
    let mut teacher = admin.clone();
    teacher.principal_id = principal(2);
    assert!(original
        .apply(
            original.revision,
            &key,
            action.clone(),
            OverrideId::new(9).unwrap(),
            &identity,
            &teacher,
            at(8 * 3600 + 31 * 60)
        )
        .is_err());
    let mut web = admin.clone();
    web.origin = TrustedOrigin::Web;
    assert!(original
        .apply(
            original.revision,
            &key,
            action.clone(),
            OverrideId::new(9).unwrap(),
            &identity,
            &web,
            at(8 * 3600 + 31 * 60)
        )
        .is_err());
    identity.accounts[0].must_change_password = true;
    assert!(original
        .apply(
            original.revision,
            &key,
            action.clone(),
            OverrideId::new(9).unwrap(),
            &identity,
            &admin,
            at(8 * 3600 + 31 * 60)
        )
        .is_err());
    assert!(original
        .apply_local_desktop(
            original.revision,
            &key,
            action,
            OverrideId::new(9).unwrap(),
            &identity,
            at(8 * 3600 + 31 * 60)
        )
        .is_ok());
}
#[test]
fn desktop_cannot_invent_teacher_or_default_target_or_cross_clock_barriers() {
    let mut identity = state();
    identity.revision = Revision::new(5);
    let original = evidence();
    let key = original.current.as_ref().unwrap().key.clone();
    for target in [
        CourseCell {
            subject_id: SubjectId::new(1).unwrap(),
            teacher: TeacherBinding::Explicit(principal(4)),
        },
        CourseCell {
            subject_id: SubjectId::new(1).unwrap(),
            teacher: TeacherBinding::Default,
        },
        CourseCell {
            subject_id: SubjectId::new(99).unwrap(),
            teacher: TeacherBinding::Unassigned,
        },
    ] {
        assert_eq!(
            original.apply_local_desktop(
                original.revision,
                &key,
                LessonAction::Replace(target),
                OverrideId::new(9).unwrap(),
                &identity,
                at(8 * 3600 + 31 * 60)
            ),
            Err(LessonContextError::InvalidTarget)
        );
    }
    let action = LessonAction::Replace(original.current.as_ref().unwrap().base.clone());
    assert!(original
        .apply_local_desktop(
            original.revision,
            &key,
            action.clone(),
            OverrideId::new(9).unwrap(),
            &identity,
            at(8 * 3600 + 29 * 60)
        )
        .is_err());
    let mut wrong = key.clone();
    wrong.version.rules_revision = Revision::new(2);
    assert!(original
        .apply_local_desktop(
            original.revision,
            &wrong,
            action,
            OverrideId::new(9).unwrap(),
            &identity,
            at(8 * 3600 + 31 * 60)
        )
        .is_err());
}
