//! Real owned SQLite/DPAPI deployment, authenticated HTTP application port only.
//! A successful diagnostic does not imply a listener or a TLS handshake.
use super::*;
use fc_application::identity::SessionLease;
use fc_bootstrap::web_application::WebResponse;
use fc_domain::{
    content::{ContentKey, HomeworkKey},
    ids::PrincipalId,
};

fn contains(response: &WebResponse, expected: &str) -> bool {
    std::str::from_utf8(&response.body).is_ok_and(|s| s.contains(expected))
}
fn post(
    backend: &mut Backend,
    application: &WebApplication,
    route: &str,
    cookie: &str,
    csrf: &str,
    body: &[u8],
) -> Result<WebResponse, String> {
    Ok(application.dispatch(
        backend,
        &head("POST", route, body, Some(cookie), Some(csrf))?,
        body,
    ))
}
pub(super) fn run(
    backend: &mut Backend,
    application: &WebApplication,
    cookie: &str,
    csrf: &str,
    lease: SessionLease,
) -> Result<(), String> {
    let scopes = check(backend.web_content_scopes(
        lease,
        fc_bootstrap::backend::WebContentKind::Homework,
        0,
    ))?;
    require(
        scopes.items.len() == 1 && scopes.total == 1,
        "content scopes do not match configured assignment",
    )?;
    let date = fc_protocol::web::content::date(scopes.today);
    let route = format!("content/homework/7/9/{}/{date}", lease.principal_id().get());
    let key = ContentKey::Homework(HomeworkKey {
        class_id: check(ClassId::new(7))?,
        subject_id: check(SubjectId::new(9))?,
        owner_teacher_id: check(PrincipalId::new(lease.principal_id().get()))?,
        actual_date: scopes.today,
    });
    let scope_response = application.dispatch(
        backend,
        &head("GET", "content/homework/scopes/0", &[], Some(cookie), None)?,
        &[],
    );
    require(
        scope_response.head.status == 200 && contains(&scope_response, "Synthetic content subject"),
        "scopes were not served through the real authenticated application",
    )?;
    let reminder_scope = application.dispatch(
        backend,
        &head("GET", "content/reminders/scopes/0", &[], Some(cookie), None)?,
        &[],
    );
    require(
        reminder_scope.head.status == 200 && contains(&reminder_scope, "Synthetic content class"),
        "reminder scope missing configured class",
    )?;
    let first = br#"{"schemaVersion":1,"commandId":"900001","expectedRevision":"0","text":"synthetic homework v1"}"#;
    let wrong = if csrf.starts_with('a') {
        "b".repeat(64)
    } else {
        "a".repeat(64)
    };
    let rejected = post(backend, application, &route, cookie, &wrong, first)?;
    require(
        rejected.head.status == 403 && check(backend.read_content(lease, key.clone()))?.is_none(),
        "CSRF failure mutated content",
    )?;
    let saved = post(backend, application, &route, cookie, csrf, first)?;
    require(
        saved.head.status == 200 && contains(&saved, "\"COMMITTED\""),
        "homework application save failed",
    )?;
    let original = check(backend.read_content(lease, key.clone()))?.ok_or("content row missing")?;
    require(
        original.revision.get() == 1 && original.text == "synthetic homework v1",
        "homework did not persist exact original",
    )?;
    let before = check(backend.global_revision())?;
    let replay = post(backend, application, &route, cookie, csrf, first)?;
    require(
        replay.head.status == 200 && check(backend.global_revision())? == before,
        "same command wrote twice",
    )?;
    let conflict = post(
        backend,
        application,
        &route,
        cookie,
        csrf,
        br#"{"schemaVersion":1,"commandId":"900002","expectedRevision":"0","text":"stale"}"#,
    )?;
    require(
        conflict.head.status == 409 && contains(&conflict, "REVISION_CONFLICT"),
        "stale CAS accepted",
    )?;
    let cleared = post(
        backend,
        application,
        &route,
        cookie,
        csrf,
        br#"{"schemaVersion":1,"commandId":"900003","expectedRevision":"1","text":""}"#,
    )?;
    require(cleared.head.status == 200, "clear failed")?;
    let cleared_row =
        check(backend.read_content(lease, key.clone()))?.ok_or("cleared row lost its CAS")?;
    require(
        cleared_row.revision.get() == 2 && cleared_row.text.is_empty(),
        "clear reset revision",
    )?;
    let read = application.dispatch(backend, &head("GET", &route, &[], Some(cookie), None)?, &[]);
    require(
        read.head.status == 200
            && contains(&read, "\"revision\":\"2\"")
            && contains(&read, "\"text\":\"\""),
        "Web read lost cleared CAS",
    )?;
    let restored = post(backend, application, &route, cookie, csrf, br#"{"schemaVersion":1,"commandId":"900004","expectedRevision":"2","text":"synthetic homework v3"}"#)?;
    require(restored.head.status == 200, "save after clear failed")?;
    let changed_id = post(backend, application, &route, cookie, csrf, br#"{"schemaVersion":1,"commandId":"900004","expectedRevision":"2","text":"tampered command"}"#)?;
    require(
        changed_id.head.status == 409 && contains(&changed_id, "IDEMPOTENCY_CONFLICT"),
        "command ID reused with another payload",
    )?;
    let result = application.dispatch(
        backend,
        &head("GET", "content/commands/900004", &[], Some(cookie), None)?,
        &[],
    );
    require(
        result.head.status == 200
            && contains(&result, "\"state\":\"COMMITTED\"")
            && contains(&result, "synthetic homework v3"),
        "original result query failed",
    )?;
    let absent = application.dispatch(
        backend,
        &head("GET", "content/commands/900099", &[], Some(cookie), None)?,
        &[],
    );
    require(
        absent.head.status == 200 && contains(&absent, "NOT_FOUND"),
        "absent result query fabricated a commit",
    )?;
    let reminder_route = format!("content/reminders/7/{date}");
    let reminder = post(backend, application, &reminder_route, cookie, csrf, br#"{"schemaVersion":1,"commandId":"900005","expectedRevision":"0","text":"synthetic shared reminder"}"#)?;
    require(reminder.head.status == 200, "reminder save failed")?;
    require(
        check(backend.desktop_facts(0))?
            .reminder
            .is_some_and(|r| r.text == "synthetic shared reminder"),
        "committed reminder missing from real desktop facts",
    )?;
    let cleared = post(
        backend,
        application,
        &reminder_route,
        cookie,
        csrf,
        br#"{"schemaVersion":1,"commandId":"900006","expectedRevision":"1","text":""}"#,
    )?;
    require(
        cleared.head.status == 200 && check(backend.desktop_facts(0))?.reminder.is_none(),
        "cleared reminder left an active desktop fact",
    )?;
    println!("WEB_CONTENT_PASS real_DPAPI_SQLite_exact_Web_lease_scopes_CSRF_save_read_clear_CAS_idempotency_query_reminder_desktop_facts=PASS synthetic_request_metadata=true wire_TLS_browser_listener=NOT_RUN");
    Ok(())
}
