//! Real writer/HTTP-port tests; synthetic TLS metadata is not live TLS evidence.
use super::*;
use fc_application::identity::SessionError;
use fc_domain::{
    desktop::ComponentKind,
    subject_visual::{SubjectColor, SubjectVisualError},
};
use fc_ports::subject_visual::{SubjectVisualRepositoryError, SubjectVisualState};
fn colors(state: &SubjectVisualState) -> Vec<SubjectColor> {
    state
        .palette
        .subjects
        .iter()
        .map(|s| SubjectColor {
            subject_id: s.id,
            color: s.color,
        })
        .collect()
}
fn preview_key() -> fc_domain::content::HomeworkKey {
    fc_domain::content::HomeworkKey {
        class_id: ClassId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
        owner_teacher_id: PrincipalId::new(1).unwrap(),
        actual_date: fc_domain::time::LocalDate::new(2026, 10, 30).unwrap(),
    }
}
#[test]
fn subject_visual_joint_save_keeps_exact_lease_and_web_preview_discloses_only_one_derived_subject()
{
    let mut f = BackendFixture::new();
    f.catalog(2);
    let cookie = f.pair("visual-browser", false);
    let web = f
        .service
        .authenticate(&cookie_secret(cookie.as_bytes()).unwrap())
        .unwrap()
        .lease();
    assert!(f.backend.read_subject_visuals(web).is_err());
    let old = f.backend.read_subject_visuals(f.admin).unwrap();
    let mut changed = colors(&old);
    changed[0].color = Rgb {
        red: 255,
        green: 0,
        blue: 0,
    };
    let mut display = old.display.clone();
    display.homework_fill_percent = 0;
    let homework = display
        .components
        .iter_mut()
        .find(|c| c.kind == ComponentKind::Homework)
        .unwrap();
    homework.opacity_percent = 50;
    homework.visible = false;
    let saved = f
        .backend
        .save_subject_visuals(
            f.admin,
            old.palette.revision.clone(),
            old.display.revision,
            changed,
            display,
        )
        .unwrap();
    assert_eq!(
        saved.palette.subjects[0].revision,
        old.palette.subjects[0].revision.next().unwrap()
    );
    assert_eq!(saved.palette.subjects[1], old.palette.subjects[1]);
    assert_eq!(saved.palette.subjects[0].name, old.palette.subjects[0].name);
    assert_eq!(f.backend.role(f.admin).unwrap(), Role::GlobalAdmin); // no replacement lease
    assert_eq!(f.backend.read_subject_visuals(f.admin).unwrap(), saved);
    let response = f.get_content(&cookie, "content/homework/1/1/1/2026-10-30/visual");
    let body = ok(&response);
    for expected in [
        "\"status\":\"SUBJECT_VISUAL\"",
        "\"effectiveRgb\":\"#C98282\"",
        "\"mode\":\"FILLED\"",
        "\"homeworkFillPercent\":0",
        "\"homeworkComponentOpacityPercent\":50",
        "\"homeworkVisible\":false",
    ] {
        assert!(body.contains(expected), "{body}");
    }
    for forbidden in [
        "Synthetic subject 2",
        "palette",
        "layout",
        "#FF0000",
        "authEpoch",
    ] {
        assert!(!body.contains(forbidden));
    }
    // A rejected joint CAS must not restore colors or partially change display.
    let stale = f.backend.save_subject_visuals(
        f.admin,
        old.palette.revision.clone(),
        saved.display.revision,
        colors(&old),
        saved.display.clone(),
    );
    assert!(matches!(
        stale,
        Err(crate::backend::BackendError::SubjectVisual(
            SubjectVisualRepositoryError::Domain(SubjectVisualError::RevisionConflict { .. })
        ))
    ));
    assert_eq!(f.backend.read_subject_visuals(f.admin).unwrap(), saved);
    let stale_display = f.backend.save_subject_visuals(
        f.admin,
        saved.palette.revision.clone(),
        old.display.revision,
        colors(&saved),
        saved.display.clone(),
    );
    assert!(matches!(
        stale_display,
        Err(crate::backend::BackendError::SubjectVisual(
            SubjectVisualRepositoryError::Display(
                fc_domain::desktop::DisplayError::RevisionConflict { .. }
            )
        ))
    ));
    assert_eq!(f.backend.read_subject_visuals(f.admin).unwrap(), saved);
}
#[test]
fn subject_visual_get_is_read_only_and_original_content_schema_still_rejects_visual_fields() {
    let mut f = BackendFixture::new();
    f.catalog(1);
    let cookie = f.pair("read-only-visual", false);
    let csrf = f.csrf(&cookie);
    let route = "content/homework/1/1/1/2026-10-30/visual";
    for method in ["POST", "HEAD", "DELETE"] {
        let response = f.dispatch(&head(method, route, 0, Some(&cookie), Some(&csrf)), b"");
        assert_eq!(response.head.status, 405);
        assert!(response
            .head
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("allow") && h.value == b"GET"));
    }
    error(
        f.get_content(&cookie, "content/reminders/1/2026-10-30/visual"),
        404,
        "NOT_FOUND",
    );
    error(
        f.get_content(&cookie, "content/homework/2/1/1/2026-10-30/visual"),
        403,
        "FORBIDDEN",
    );
    error(
        f.dispatch(&head("GET", route, 0, None, None), b""),
        401,
        "UNAUTHORIZED",
    );
    let before = f.backend.read_subject_visuals(f.admin).unwrap();
    let mixed = r##"{"schemaVersion":1,"commandId":"76001","expectedRevision":"0","text":"not saved","effectiveRgb":"#FFFFFF"}"##;
    error(
        f.save_content(&cookie, &csrf, "content/homework/1/1/1/2026-10-30", mixed),
        400,
        "INVALID_REQUEST",
    );
    assert_eq!(f.backend.read_subject_visuals(f.admin).unwrap(), before);
}
#[test]
fn subject_visual_cancel_lock_and_day_floor_cannot_return_old_scope_as_current() {
    let mut f = BackendFixture::new();
    f.catalog(1);
    let cookie = f.pair("guarded-visual", false);
    let web = f
        .service
        .authenticate(&cookie_secret(cookie.as_bytes()).unwrap())
        .unwrap()
        .lease();
    let before = f.backend.read_subject_visuals(f.admin).unwrap();
    {
        let scope = f.backend.begin_execution_scope(Arc::new(|| false));
        assert!(matches!(
            scope.web_subject_visual_preview(web, preview_key()),
            Err(crate::backend::BackendError::Storage(
                fc_storage::StorageError::ExecutionCancelled
            ))
        ));
        assert!(matches!(
            scope.read_subject_visuals(f.admin),
            Err(crate::backend::BackendError::Storage(
                fc_storage::StorageError::ExecutionCancelled
            ))
        ));
    }
    assert_eq!(f.backend.read_subject_visuals(f.admin).unwrap(), before);
    f.backend.session_control().lock_local();
    assert!(f.backend.read_subject_visuals(f.admin).is_err());
    let route = "content/homework/1/1/1/2026-10-30/visual";
    assert!(ok(&f.get_content(&cookie, route)).contains("SUBJECT_VISUAL")); // local lock preserves exact Web lease
    f.clock.1.store(86_401, Ordering::Release);
    f.clock.0.fetch_add(1000, Ordering::AcqRel);
    error(f.get_content(&cookie, route), 409, "DAY_CHANGED");
    assert!(
        ok(&f.get_content(&cookie, "content/homework/1/1/1/2026-10-31/visual"))
            .contains("SUBJECT_VISUAL")
    );
    f.clock.1.store(36_001, Ordering::Release);
    f.clock.0.fetch_add(1000, Ordering::AcqRel);
    error(f.get_content(&cookie, route), 409, "DAY_CHANGED");
}

#[test]
fn subject_visual_management_denies_initial_teacher_user_and_preview_remains_owned() {
    use crate::backend::BackendError;
    use fc_domain::auth::PermissionError;
    use fc_ports::IdentityRepositoryError;
    for role in [Role::TeacherAdmin, Role::User] {
        let mut f = BackendFixture::new();
        f.catalog(1);
        let before = f.backend.read_subject_visuals(f.admin).unwrap();
        let original = "Synthetic-Visual-Initial-Password-2026";
        let changed = "Synthetic-Visual-Changed-Password-2026";
        drop(
            f.backend
                .create_account(
                    f.admin,
                    NewAccount {
                        id: PrincipalId::new(2).unwrap(),
                        login_name: "visual-user".into(),
                        display_name: "Synthetic visual user".into(),
                        role,
                        scopes: if role == Role::TeacherAdmin {
                            vec![AssignmentScope {
                                class_id: ClassId::new(1).unwrap(),
                                subject_id: SubjectId::new(1).unwrap(),
                            }]
                        } else {
                            vec![]
                        },
                    },
                    Password::new(original.into()).unwrap(),
                )
                .unwrap(),
        );
        let initial = f
            .backend
            .login("visual-user", Password::new(original.into()).unwrap())
            .unwrap();
        assert!(matches!(
            f.backend.read_subject_visuals(initial),
            Err(BackendError::Session(SessionError::PasswordChangeRequired))
        ));
        f.backend
            .change_password(
                initial,
                Password::new(original.into()).unwrap(),
                Password::new(changed.into()).unwrap(),
            )
            .unwrap();
        let user = f
            .backend
            .login("visual-user", Password::new(changed.into()).unwrap())
            .unwrap();
        assert!(matches!(
            f.backend.read_subject_visuals(user),
            Err(BackendError::Identity(IdentityRepositoryError::Domain(
                fc_domain::identity::IdentityError::Permission(PermissionError::RoleDenied)
            )))
        ));
        assert!(matches!(
            f.backend.save_subject_visuals(
                user,
                before.palette.revision.clone(),
                before.display.revision,
                colors(&before),
                before.display.clone()
            ),
            Err(BackendError::Identity(IdentityRepositoryError::Domain(
                fc_domain::identity::IdentityError::Permission(PermissionError::RoleDenied)
            )))
        ));
        f.admin = f
            .backend
            .login("admin", Password::new(PASSWORD.into()).unwrap())
            .unwrap();
        assert_eq!(f.backend.read_subject_visuals(f.admin).unwrap(), before);
        if role == Role::TeacherAdmin {
            let cookie = f.pair_login("visual-user", "teacher-visual-only", false);
            assert!(
                ok(&f.get_content(&cookie, "content/homework/1/1/2/2026-10-30/visual"))
                    .contains("SUBJECT_VISUAL")
            );
            error(
                f.get_content(&cookie, "content/homework/1/1/1/2026-10-30/visual"),
                403,
                "FORBIDDEN",
            );
            f.backend
                .identity(
                    f.admin,
                    IdentityCommand::UpdateAccount {
                        id: PrincipalId::new(2).unwrap(),
                        display_name: "Synthetic visual user".into(),
                        role,
                        enabled: false,
                        scopes: vec![AssignmentScope {
                            class_id: ClassId::new(1).unwrap(),
                            subject_id: SubjectId::new(1).unwrap(),
                        }],
                    },
                )
                .unwrap();
            error(
                f.get_content(&cookie, "content/homework/1/1/2/2026-10-30/visual"),
                401,
                "UNAUTHORIZED",
            );
        }
    }
}
