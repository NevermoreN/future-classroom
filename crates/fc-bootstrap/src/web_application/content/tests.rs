//! Real SQLite plus authenticated HTTP-port evidence; no browser/TLS claim.
use super::*;
use fc_domain::{
    auth::Role,
    identity::{AssignmentScope, IdentityCommand, NewAccount, Rgb},
    ids::{ClassId, PrincipalId, SubjectId},
};

impl BackendFixture {
    fn catalog(&mut self, subjects: u64) {
        for id in [2, 1] {
            self.backend
                .identity(
                    self.admin,
                    IdentityCommand::CreateClass {
                        id: ClassId::new(id).unwrap(),
                        name: format!("Synthetic class {id}"),
                    },
                )
                .unwrap();
        }
        for id in (1..=subjects).rev() {
            self.backend
                .identity(
                    self.admin,
                    IdentityCommand::PutSubject {
                        id: SubjectId::new(id).unwrap(),
                        name: format!("Synthetic subject {id}"),
                        enabled: true,
                        color: Rgb {
                            red: 4,
                            green: 5,
                            blue: 6,
                        },
                        expected_revision: None,
                    },
                )
                .unwrap();
        }
        self.backend
            .identity(
                self.admin,
                IdentityCommand::UpdateAccount {
                    id: PrincipalId::new(1).unwrap(),
                    display_name: "Synthetic global".into(),
                    role: Role::GlobalAdmin,
                    enabled: true,
                    scopes: (1..=subjects)
                        .rev()
                        .map(|id| AssignmentScope {
                            class_id: ClassId::new(1).unwrap(),
                            subject_id: SubjectId::new(id).unwrap(),
                        })
                        .collect(),
                },
            )
            .unwrap();
        // Updating even one's own scopes replaces the exact local lease.
        self.admin = self
            .backend
            .login("admin", Password::new(PASSWORD.into()).unwrap())
            .unwrap();
    }
    fn csrf(&mut self, cookie: &str) -> String {
        let response = self.dispatch(&head("GET", "session", 0, Some(cookie), None), b"");
        assert_eq!(response.head.status, 200);
        field(&response.body, "csrf")
    }
    fn get_content(&mut self, cookie: &str, route: &str) -> WebResponse {
        self.dispatch(&head("GET", route, 0, Some(cookie), None), b"")
    }
    fn save_content(&mut self, cookie: &str, csrf: &str, route: &str, body: &str) -> WebResponse {
        self.dispatch(
            &head("POST", route, body.len(), Some(cookie), Some(csrf)),
            body.as_bytes(),
        )
    }
}
fn save(id: u64, revision: u64, json_text: &str) -> String {
    format!(
        r#"{{"schemaVersion":1,"commandId":"{id}","expectedRevision":"{revision}","text":{json_text}}}"#
    )
}
fn ok(response: &WebResponse) -> &str {
    assert_eq!(response.head.status, 200, "{}", text(&response.body));
    assert!(response
        .head
        .headers
        .iter()
        .any(|h| h.name == "Cache-Control" && h.value == b"no-store"));
    text(&response.body)
}
fn error(response: WebResponse, status: u16, code: &str) {
    assert_eq!(response.head.status, status, "{}", text(&response.body));
    assert!(
        text(&response.body).contains(code),
        "{}",
        text(&response.body)
    );
    assert!(!text(&response.body).contains("currentRevision"));
    assert!(!text(&response.body).contains("\"text\""));
}

#[test]
fn content_http_clear_keeps_cas_and_receipts_keep_original_result_without_automatic_rewrite() {
    let mut f = BackendFixture::new();
    f.catalog(1);
    let key = fc_domain::content::ContentKey::Homework(fc_domain::content::HomeworkKey {
        class_id: ClassId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
        owner_teacher_id: PrincipalId::new(1).unwrap(),
        actual_date: f.clock.now().date,
    });
    assert!(matches!(
        f.backend.web_content_read(f.admin, key),
        Err(crate::backend::BackendError::Content(
            fc_ports::RepositoryError::Domain(fc_domain::content::ContentError::Permission(
                fc_domain::auth::PermissionError::WebOnly
            ))
        ))
    ));
    let cookie = f.pair("content-browser", false);
    let csrf = f.csrf(&cookie);
    let route = "content/homework/1/1/1/2026-10-30";
    let absent = f.get_content(&cookie, route);
    let absent = ok(&absent);
    for expected in [
        "\"revision\":\"0\"",
        "\"text\":\"\"",
        "\"lastActorId\":null",
        "\"updatedAt\":null",
        "\"serverNow\":\"2026-10-30T10:00:00\"",
        "\"expiresAt\":\"2026-10-31T00:00:00\"",
    ] {
        assert!(absent.contains(expected), "{absent}");
    }
    let first_body = save(101, 0, r#"" \t练习\n第二行\t保留\r字符 \r\n""#);
    let first = f.save_content(&cookie, &csrf, route, &first_body);
    assert!(ok(&first).contains("\"status\":\"COMMITTED\""));
    assert!(ok(&first).contains("\"revision\":\"1\""));
    assert!(ok(&first).contains("\"text\":\"练习\\n第二行\\t保留\\r字符\""));
    let clear = f.save_content(&cookie, &csrf, route, &save(102, 1, "\"\""));
    assert!(ok(&clear).contains("\"revision\":\"2\""));
    let cleared = f.get_content(&cookie, route);
    assert!(ok(&cleared).contains("\"revision\":\"2\""));
    assert!(ok(&cleared).contains("\"text\":\"\",\"lastActorId\":\"1\""));
    let later = f.save_content(&cookie, &csrf, route, &save(103, 2, "\"Later version\""));
    assert!(ok(&later).contains("\"revision\":\"3\""));
    let original = f.get_content(&cookie, "content/commands/101");
    assert!(ok(&original).contains("\"state\":\"COMMITTED\""));
    assert!(ok(&original).contains("\"revision\":\"1\""));
    assert!(ok(&original).contains("练习\\n第二行\\t保留\\r字符"));
    assert!(!ok(&original).contains("Later version"));
    let replay = f.save_content(&cookie, &csrf, route, &first_body);
    assert!(ok(&replay).contains("\"revision\":\"1\""));
    error(
        f.save_content(&cookie, &csrf, route, &save(101, 0, "\"different\"")),
        409,
        "IDEMPOTENCY_CONFLICT",
    );
    error(
        f.save_content(
            &cookie,
            &csrf,
            route,
            &save(101, 0, r#""练习\n第二行\t保留\r字符""#),
        ),
        409,
        "IDEMPOTENCY_CONFLICT",
    );
    error(
        f.save_content(&cookie, &csrf, route, &save(104, 0, "\"stale\"")),
        409,
        "REVISION_CONFLICT",
    );
    let unknown = f.get_content(&cookie, "content/commands/104");
    assert!(ok(&unknown).contains("\"state\":\"NOT_FOUND\",\"content\":null"));
    let latest = f.get_content(&cookie, route);
    assert!(ok(&latest).contains("\"revision\":\"3\""));
}

#[test]
fn content_http_authority_and_csrf_injection_fail_before_durable_mutation() {
    let mut f = BackendFixture::new();
    f.catalog(1);
    let cookie = f.pair("guard-browser", false);
    let csrf = f.csrf(&cookie);
    let route = "content/homework/1/1/1/2026-10-30";
    let valid = save(201, 0, "\"must never be stored\"");
    error(
        f.dispatch(
            &head("POST", route, valid.len(), None, Some(&csrf)),
            valid.as_bytes(),
        ),
        401,
        "UNAUTHORIZED",
    );
    error(
        f.save_content(&cookie, &"f".repeat(64), route, &valid),
        403,
        "FORBIDDEN",
    );
    let injected = [
        r#"{"schemaVersion":1,"commandId":"201","expectedRevision":"0","text":"x","actor":{"role":"GLOBAL_ADMIN"}}"#,
        r#"{"schemaVersion":1,"commandId":"201","expectedRevision":"0","text":"x","origin":"LOCAL"}"#,
        r#"{"schemaVersion":1,"commandId":"201","expectedRevision":"0","text":"x","key":{"classId":"2"}}"#,
        r#"{"schemaVersion":1,"commandId":"201","command\u0049d":"202","expectedRevision":"0","text":"x"}"#,
        r#"{"schemaVersion":1,"commandId":"201","expectedRevision":"00","text":"x"}"#,
        r#"{"schemaVersion":1,"commandId":"201","expectedRevision":"0","text":"\u0085"}"#,
    ];
    for body in injected {
        error(
            f.save_content(&cookie, &csrf, route, body),
            400,
            "INVALID_REQUEST",
        );
    }
    error(
        f.get_content(&cookie, "content/homework/01/1/1/2026-10-30"),
        404,
        "NOT_FOUND",
    );
    let absent = f.get_content(&cookie, route);
    assert!(ok(&absent).contains("\"revision\":\"0\""));
    let missing = f.get_content(&cookie, "content/commands/201");
    assert!(ok(&missing).contains("\"state\":\"NOT_FOUND\""));
    let committed = f.save_content(&cookie, &csrf, route, &valid);
    assert!(ok(&committed).contains("\"revision\":\"1\""));
}

#[test]
fn content_http_teacher_scope_global_pagination_and_receipt_privacy_use_actual_accounts() {
    let mut f = BackendFixture::new();
    f.catalog(21);
    let teacher_id = PrincipalId::new(2).unwrap();
    drop(
        f.backend
            .create_account(
                f.admin,
                NewAccount {
                    id: teacher_id,
                    login_name: "teacher".into(),
                    display_name: "Synthetic teacher".into(),
                    role: Role::TeacherAdmin,
                    scopes: vec![AssignmentScope {
                        class_id: ClassId::new(1).unwrap(),
                        subject_id: SubjectId::new(1).unwrap(),
                    }],
                },
                Password::new("Synthetic-Teacher-Initial-Password-2026".into()).unwrap(),
            )
            .unwrap(),
    );
    let initial = f
        .backend
        .login(
            "teacher",
            Password::new("Synthetic-Teacher-Initial-Password-2026".into()).unwrap(),
        )
        .unwrap();
    assert!(f
        .backend
        .check_session(initial, SessionPurpose::Manage)
        .is_err());
    f.backend
        .change_password(
            initial,
            Password::new("Synthetic-Teacher-Initial-Password-2026".into()).unwrap(),
            Password::new("Synthetic-Teacher-Changed-Password-2026".into()).unwrap(),
        )
        .unwrap();
    assert!(f
        .backend
        .check_session(initial, SessionPurpose::ChangeInitialPassword)
        .is_err());
    f.admin = f
        .backend
        .login("admin", Password::new(PASSWORD.into()).unwrap())
        .unwrap();
    let admin = f.pair("global-content", false);
    let teacher = f.pair_login("teacher", "teacher-content", false);
    let teacher_csrf = f.csrf(&teacher);
    let all = f.get_content(&admin, "content/homework/scopes/0");
    let all = ok(&all);
    assert!(all.contains("\"total\":22,\"nextOffset\":20"), "{all}");
    assert_eq!(all.matches("\"classId\"").count(), 20);
    assert!(
        all.find("\"ownerTeacherId\":\"1\"").unwrap()
            < all.find("\"ownerTeacherId\":\"2\"").unwrap()
    );
    assert!(all.find("\"subjectId\":\"2\"").unwrap() < all.find("\"subjectId\":\"10\"").unwrap());
    let end = f.get_content(&admin, "content/homework/scopes/20");
    assert!(ok(&end).contains("\"offset\":20,\"total\":22,\"nextOffset\":null"));
    assert_eq!(ok(&end).matches("\"classId\"").count(), 2);
    let empty = f.get_content(&admin, "content/homework/scopes/22");
    assert!(ok(&empty).contains("\"items\":[]"));
    error(
        f.get_content(&admin, "content/homework/scopes/23"),
        400,
        "INVALID_REQUEST",
    );
    let own = f.get_content(&teacher, "content/homework/scopes/0");
    assert!(ok(&own).contains("\"total\":1,\"nextOffset\":null"));
    assert!(ok(&own).contains("\"ownerTeacherId\":\"2\""));
    assert_eq!(
        field(&own.body, "identityRevision"),
        field(all.as_bytes(), "identityRevision")
    );
    let reminders = f.get_content(&teacher, "content/reminders/scopes/0");
    assert!(ok(&reminders).contains("\"total\":2"));
    assert!(ok(&reminders).contains("\"classId\":\"2\""));
    let foreign = "content/homework/1/1/1/2026-10-30";
    error(f.get_content(&teacher, foreign), 403, "FORBIDDEN");
    error(
        f.save_content(
            &teacher,
            &teacher_csrf,
            foreign,
            &save(301, 0, "\"foreign\""),
        ),
        403,
        "FORBIDDEN",
    );
    let mine = "content/homework/1/1/2/2026-10-30";
    let saved = f.save_content(
        &teacher,
        &teacher_csrf,
        mine,
        &save(302, 0, "\"Teacher owned\""),
    );
    assert!(ok(&saved).contains("\"lastActorId\":\"2\""));
    let global_read = f.get_content(&admin, mine);
    assert!(ok(&global_read).contains("Teacher owned"));
    let hidden_receipt = f.get_content(&admin, "content/commands/302");
    assert!(ok(&hidden_receipt).contains("\"state\":\"NOT_FOUND\""));
    let own_receipt = f.get_content(&teacher, "content/commands/302");
    assert!(ok(&own_receipt).contains("\"state\":\"COMMITTED\""));
    let shared = f.save_content(
        &teacher,
        &teacher_csrf,
        "content/reminders/2/2026-10-30",
        &save(303, 0, "\"Other managed class reminder\""),
    );
    assert!(ok(&shared).contains("Other managed class reminder"));
    f.backend
        .identity(
            f.admin,
            IdentityCommand::UpdateAccount {
                id: teacher_id,
                display_name: "Synthetic teacher".into(),
                role: Role::TeacherAdmin,
                enabled: true,
                scopes: vec![AssignmentScope {
                    class_id: ClassId::new(2).unwrap(),
                    subject_id: SubjectId::new(21).unwrap(),
                }],
            },
        )
        .unwrap();
    error(f.get_content(&teacher, mine), 401, "UNAUTHORIZED");
    error(f.get_content(&admin, mine), 403, "FORBIDDEN");
    let global = f.get_content(&admin, "content/homework/scopes/0");
    assert!(ok(&global).contains("\"total\":22"));
    assert_ne!(
        field(&global.body, "identityRevision"),
        field(&own.body, "identityRevision")
    );
}

#[test]
fn content_http_midnight_rejects_old_routes_and_receipts_and_day_floor_survives_clock_rollback() {
    let mut f = BackendFixture::new();
    f.catalog(1);
    let cookie = f.pair("midnight-content", false);
    let csrf = f.csrf(&cookie);
    let old = "content/homework/1/1/1/2026-10-30";
    let stored = f.save_content(&cookie, &csrf, old, &save(401, 0, "\"Yesterday secret\""));
    assert!(ok(&stored).contains("Yesterday secret"));
    f.clock.1.store(86_401, Ordering::Release);
    f.clock.0.fetch_add(1_000, Ordering::AcqRel);
    error(f.get_content(&cookie, old), 409, "DAY_CHANGED");
    error(
        f.get_content(&cookie, "content/commands/401"),
        409,
        "DAY_CHANGED",
    );
    error(
        f.save_content(&cookie, &csrf, old, &save(402, 1, "\"old date write\"")),
        409,
        "DAY_CHANGED",
    );
    let today = "content/homework/1/1/1/2026-10-31";
    let empty = f.get_content(&cookie, today);
    assert!(ok(&empty).contains("\"revision\":\"0\""));
    assert!(ok(&empty).contains("\"expiresAt\":\"2026-11-01T00:00:00\""));
    f.clock.1.store(36_001, Ordering::Release);
    f.clock.0.fetch_add(1_000, Ordering::AcqRel);
    error(f.get_content(&cookie, old), 409, "DAY_CHANGED");
    error(
        f.save_content(&cookie, &csrf, old, &save(403, 1, "\"must not revive\"")),
        409,
        "DAY_CHANGED",
    );
}

#[path = "subject_visual_tests.rs"]
mod subject_visual_tests;
