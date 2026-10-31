use super::*;
use crate::{auth::*, identity::*, ids::*, time::*};
fn principal(id: u64) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn now() -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 11, 1).unwrap(), 3600).unwrap()
}
fn actor(origin: TrustedOrigin) -> ActorContext {
    ActorContext {
        principal_id: principal(1),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(now().date, 7200).unwrap(),
        origin,
    }
}
fn colors(state: &IdentityState) -> Vec<SubjectColor> {
    state
        .subjects
        .iter()
        .map(|s| SubjectColor {
            subject_id: s.id,
            color: s.color,
        })
        .collect()
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
fn complete_version_vector_is_canonical_and_bounded() {
    let id = SubjectId::new(1).unwrap();
    assert!(PaletteRevision::new(vec![(id, Revision::default())]).is_err());
    assert!(PaletteRevision::new(vec![(id, Revision::new(1)), (id, Revision::new(2))]).is_err());
    assert!(PaletteRevision::new(
        (1..=257)
            .map(|id| (SubjectId::new(id).unwrap(), Revision::new(1)))
            .collect()
    )
    .is_err());
    let mut state = state();
    let mut second = state.subjects[0].clone();
    second.id = SubjectId::new(2).unwrap();
    second.name = "数学".into();
    second.enabled = false;
    state.subjects.insert(0, second);
    let palette = SubjectVisualPalette::from_identity(&state).unwrap();
    assert_eq!(
        palette
            .revision
            .entries()
            .iter()
            .map(|e| e.0.get())
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(!palette.subjects[1].enabled);
    assert!(PaletteRevision::new(vec![
        (SubjectId::new(2).unwrap(), Revision::new(1)),
        (id, Revision::new(1))
    ])
    .is_err());
}
#[test]
fn color_updates_preserve_catalog_and_advance_only_changed_subjects() {
    let state = state();
    let palette = SubjectVisualPalette::from_identity(&state).unwrap();
    assert_eq!(
        palette
            .transition(
                &colors(&state),
                &palette.revision,
                &state,
                &actor(TrustedOrigin::Local),
                now()
            )
            .unwrap(),
        state
    );
    let mut desired = colors(&state);
    desired[0].color.red += 1;
    let next = palette
        .transition(
            &desired,
            &palette.revision,
            &state,
            &actor(TrustedOrigin::Local),
            now(),
        )
        .unwrap();
    assert_eq!(next.revision, state.revision.next().unwrap());
    assert_eq!(
        next.subjects[0].revision,
        state.subjects[0].revision.next().unwrap()
    );
    let mut expected = state.clone();
    expected.revision = next.revision;
    expected.subjects[0].color = desired[0].color;
    expected.subjects[0].revision = next.subjects[0].revision;
    assert_eq!(next, expected);
    assert!(palette
        .transition(
            &[],
            &palette.revision,
            &state,
            &actor(TrustedOrigin::Local),
            now()
        )
        .is_err());
    desired[0].subject_id = SubjectId::new(3).unwrap();
    assert!(palette
        .transition(
            &desired,
            &palette.revision,
            &state,
            &actor(TrustedOrigin::Local),
            now()
        )
        .is_err());
}
#[test]
fn legacy_put_subject_aba_rename_and_new_catalog_ids_invalidate_palette() {
    let original = state();
    let palette = SubjectVisualPalette::from_identity(&original).unwrap();
    let mut state = original.clone();
    for (name, color) in [
        (
            "改名",
            Rgb {
                red: 100,
                green: 120,
                blue: 140,
            },
        ),
        ("语文", original.subjects[0].color),
    ] {
        let subject = &state.subjects[0];
        state = state
            .transition(
                &IdentityCommand::PutSubject {
                    id: subject.id,
                    name: name.into(),
                    enabled: true,
                    color,
                    expected_revision: Some(subject.revision),
                },
                Some(&actor(TrustedOrigin::Local)),
                now(),
            )
            .unwrap()
            .next;
    }
    assert_eq!(state.subjects[0].color, original.subjects[0].color);
    assert!(matches!(
        SubjectVisualPalette::from_identity(&state)
            .unwrap()
            .transition(
                &colors(&state),
                &palette.revision,
                &state,
                &actor(TrustedOrigin::Local),
                now()
            ),
        Err(SubjectVisualError::RevisionConflict { .. })
    ));
    let current = SubjectVisualPalette::from_identity(&state).unwrap();
    state = state
        .transition(
            &IdentityCommand::PutSubject {
                id: SubjectId::new(2).unwrap(),
                name: "数学".into(),
                enabled: false,
                color: original.subjects[0].color,
                expected_revision: None,
            },
            Some(&actor(TrustedOrigin::Local)),
            now(),
        )
        .unwrap()
        .next;
    assert_ne!(
        current.revision,
        PaletteRevision::from_subjects(&state.subjects).unwrap()
    );
}
#[test]
fn unrelated_identity_change_does_not_conflict_but_web_teacher_and_overflow_fail() {
    let mut state = state();
    let expected = PaletteRevision::from_subjects(&state.subjects).unwrap();
    state.revision = Revision::new(12);
    state.accounts[0].display_name = "新显示名".into();
    let palette = SubjectVisualPalette::from_identity(&state).unwrap();
    assert!(palette
        .transition(
            &colors(&state),
            &expected,
            &state,
            &actor(TrustedOrigin::Local),
            now()
        )
        .is_ok());
    assert!(palette
        .transition(
            &colors(&state),
            &expected,
            &state,
            &actor(TrustedOrigin::Web),
            now()
        )
        .is_err());
    let mut teacher = actor(TrustedOrigin::Local);
    teacher.principal_id = principal(2);
    assert!(palette
        .transition(&colors(&state), &expected, &state, &teacher, now())
        .is_err());
    state.subjects[0].revision = Revision::new(u64::MAX);
    let palette = SubjectVisualPalette::from_identity(&state).unwrap();
    let mut desired = colors(&state);
    desired[0].color.red += 1;
    assert_eq!(
        palette.transition(
            &desired,
            &palette.revision,
            &state,
            &actor(TrustedOrigin::Local),
            now()
        ),
        Err(SubjectVisualError::VersionOverflow)
    );
}
