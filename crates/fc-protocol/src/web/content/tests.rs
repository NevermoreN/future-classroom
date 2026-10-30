use super::*;
use crate::web::{Route, WebBoundary};
use fc_ports::http::ListenPrefix;

fn today() -> LocalDate {
    LocalDate::new(2026, 10, 30).unwrap()
}
fn now() -> LocalDateTime {
    LocalDateTime::new(today(), 36_001).unwrap()
}
fn expires() -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 0).unwrap()
}
fn key() -> ContentKey {
    ContentKey::Homework(HomeworkKey {
        class_id: ClassId::new(u64::MAX).unwrap(),
        subject_id: SubjectId::new(2).unwrap(),
        owner_teacher_id: PrincipalId::new(3).unwrap(),
        actual_date: today(),
    })
}
fn body(text: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"commandId":u64::MAX.to_string(),"expectedRevision":"0","text":text})).unwrap()
}
fn value(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn canonical_routes_preserve_all_id_bits_and_reject_aliases_dates_and_offsets() {
    let boundary =
        WebBoundary::new(&ListenPrefix::https("https://board.school:443/fc/").unwrap()).unwrap();
    let root = "/fc/api/v1/";
    let path = format!("{root}content/homework/{}/2/3/2026-10-30", u64::MAX);
    assert!(
        matches!(boundary.route("GET",&path),Ok(Route::ContentRead{key:Key::Homework{class_id,..}})if class_id.get()==u64::MAX)
    );
    assert!(matches!(
        boundary.route("POST", &path),
        Ok(Route::ContentSave { .. })
    ));
    assert!(boundary.route("PUT", &path).is_err());
    for tail in [
        "content/homework/scopes/0",
        "content/reminders/scopes/4096",
        "content/reminders/1/2024-02-29",
        "content/commands/18446744073709551615",
    ] {
        assert!(
            boundary.route("GET", &format!("{root}{tail}")).is_ok(),
            "{tail}"
        );
    }
    for tail in [
        "content/homework/scopes/4097",
        "content/homework/scopes/00",
        "content/homework/scopes/-1",
        "content/reminders/0/2026-10-30",
        "content/homework/01/2/3/2026-10-30",
        "content/homework/1/2/3/2026-1-30",
        "content/reminders/1/2026-02-29",
        "content/reminders/1/0000-01-01",
        "content/reminders/1/2026-10-30/",
        "content/commands/0",
        "content/commands/18446744073709551616",
        "content/reminders/1/2026-10-30?actor=1",
    ] {
        assert!(
            boundary.route("GET", &format!("{root}{tail}")).is_err(),
            "{tail}"
        );
    }
}

#[test]
fn strict_save_rejects_duplicate_authority_injection_noncanonical_ids_and_invalid_text() {
    let input = save(&body("课文\n\r\t<script>plain</script>")).unwrap();
    assert_eq!(input.command_id.get(), u64::MAX);
    assert_eq!(input.expected_revision.get(), 0);
    for field in [
        r#""actor":{}"#,
        r#""origin":"Local""#,
        r#""key":{}"#,
        r#""ownerTeacherId":"2""#,
        r#""text":"overwrite""#,
        r#""te\u0078t":"overwrite""#,
    ] {
        let raw = String::from_utf8(body("initial")).unwrap();
        let added = format!("{},{} }}", &raw[..raw.len() - 1], field);
        assert!(save(added.as_bytes()).is_err(), "{field}");
    }
    for id in ["0", "01", "+1", "18446744073709551616"] {
        let b = body("x");
        let s = String::from_utf8(b)
            .unwrap()
            .replace(&u64::MAX.to_string(), id);
        assert!(save(s.as_bytes()).is_err());
    }
    for text in ["\0", "\u{1f}", "\u{7f}", "\u{85}", "\u{9f}"] {
        assert!(save(&body(text)).is_err());
    }
    assert!(save(&body(&"字".repeat(5461))).is_ok());
    assert!(save(&body(&"a".repeat(MAX_TEXT_BYTES))).is_ok());
    assert!(save(&body(&"a".repeat(MAX_TEXT_BYTES + 1))).is_err());
    assert!(
        save(br#"{"schemaVersion":1,"commandId":1,"expectedRevision":"0","text":"x"}"#).is_err()
    );
}

#[test]
fn exact_content_committed_and_command_result_disclosures_keep_revisions_after_clear() {
    let key = key();
    let empty = value(&document(&key, now(), expires(), None).unwrap());
    assert_eq!(empty["revision"], "0");
    assert_eq!(empty["text"], "");
    assert!(empty["lastActorId"].is_null());
    assert_eq!(empty["today"], "2026-10-30");
    assert_eq!(empty["serverNow"], "2026-10-30T10:00:01");
    assert_eq!(empty["expiresAt"], "2026-10-31T00:00:00");
    assert_eq!(empty["key"]["classId"], u64::MAX.to_string());
    let record = ContentRecord {
        key: key.clone(),
        text: String::new(),
        revision: Revision::new(u64::MAX),
        last_actor_id: PrincipalId::new(7).unwrap(),
        updated_at: now(),
    };
    let committed =
        value(&committed(CommandId::new(9).unwrap(), &key, now(), expires(), &record).unwrap());
    assert_eq!(committed["status"], "COMMITTED");
    assert_eq!(committed["commandId"], "9");
    assert_eq!(committed["content"]["revision"], u64::MAX.to_string());
    assert_eq!(committed["content"]["lastActorId"], "7");
    assert_eq!(committed["content"]["text"], "");
    let found = value(
        &command_result(
            CommandId::new(9).unwrap(),
            Some((&key, now(), expires(), &record)),
        )
        .unwrap(),
    );
    assert_eq!(found["state"], "COMMITTED");
    assert_eq!(found["content"], committed["content"]);
    let missing = value(&command_result(CommandId::new(10).unwrap(), None).unwrap());
    assert_eq!(missing["state"], "NOT_FOUND");
    assert!(missing["content"].is_null());
    assert_eq!(missing.as_object().unwrap().len(), 5);
}

#[test]
fn response_date_record_text_and_page_bounds_fail_closed_without_truncation() {
    let key = key();
    assert!(document(&key, now(), now(), None).is_err());
    let mut record = ContentRecord {
        key: key.clone(),
        text: "\0".into(),
        revision: Revision::new(1),
        last_actor_id: PrincipalId::new(1).unwrap(),
        updated_at: now(),
    };
    assert!(document(&key, now(), expires(), Some(&record)).is_err());
    record.text = "valid".into();
    record.revision = Revision::new(0);
    assert!(document(&key, now(), expires(), Some(&record)).is_err());
    record.revision = Revision::new(1);
    record.text = "\n".repeat(MAX_TEXT_BYTES);
    assert!(
        document(&key, now(), expires(), Some(&record))
            .unwrap()
            .len()
            < MAX_JSON_BYTES
    );
    record.text = "a".repeat(MAX_TEXT_BYTES + 1);
    assert!(document(&key, now(), expires(), Some(&record)).is_err());
    let next = LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 0).unwrap();
    assert!(document(&key, next, expires(), None).is_err());
    let item = Scope::Reminder {
        class_id: ClassId::new(1).unwrap(),
        class_name: "Synthetic class",
    };
    let page = value(
        &scopes(
            Kind::Reminder,
            today(),
            Revision::new(u64::MAX),
            0,
            1,
            &[item],
        )
        .unwrap(),
    );
    assert_eq!(page["identityRevision"], u64::MAX.to_string());
    assert_eq!(page["offset"], 0);
    assert!(page["nextOffset"].is_null());
    assert_eq!(page["items"][0].as_object().unwrap().len(), 2);
    assert!(scopes(Kind::Reminder, today(), Revision::new(1), 0, 1, &[]).is_err());
    assert!(scopes(Kind::Reminder, today(), Revision::new(1), 0, 129, &[]).is_err());
    assert!(scopes(Kind::Homework, today(), Revision::new(1), 0, 4097, &[]).is_err());
    assert!(scopes(Kind::Reminder, today(), Revision::new(1), 1, 0, &[]).is_err());
    assert!(scopes(Kind::Reminder, today(), Revision::new(1), 1, 1, &[]).is_ok());
}

#[test]
fn content_mutation_requires_json_same_origin_framing_and_exact_csrf() {
    use crate::web::WebError;
    use fc_ports::http::{Header, RequestHead, TokenIssuer};
    let boundary =
        WebBoundary::new(&ListenPrefix::https("https://board.school:443/fc/").unwrap()).unwrap();
    let mut head = RequestHead {
        token: TokenIssuer::new().unwrap().issue(0).unwrap(),
        method: "POST".into(),
        target: "/fc/api/v1/content/reminders/1/2026-10-30".into(),
        headers: vec![
            Header {
                name: "Host".into(),
                value: b"board.school".to_vec(),
            },
            Header {
                name: "Origin".into(),
                value: b"https://board.school".to_vec(),
            },
            Header {
                name: "Content-Type".into(),
                value: b"application/json".to_vec(),
            },
            Header {
                name: "Content-Length".into(),
                value: b"2".to_vec(),
            },
        ],
        tls: true,
        content_length: Some(2),
        has_body: true,
    };
    let route = boundary.preflight(&head).unwrap();
    assert!(route.requires_session());
    assert_eq!(route.body_limit(), MAX_JSON_BYTES as u64);
    let csrf = "a".repeat(64);
    assert_eq!(boundary.check(&head, Some(&csrf)), Err(WebError::Csrf));
    head.headers.push(Header {
        name: "X-FC-CSRF".into(),
        value: csrf.as_bytes().to_vec(),
    });
    assert_eq!(boundary.check(&head, Some(&csrf)), Ok(route));
    assert_eq!(
        boundary.check(&head, Some(&"b".repeat(64))),
        Err(WebError::Csrf)
    );
    head.headers[1].value = b"https://other.school".to_vec();
    assert_eq!(boundary.preflight(&head), Err(WebError::Origin));
    head.headers[1].value = b"https://board.school".to_vec();
    head.content_length = Some((MAX_JSON_BYTES + 1) as u64);
    head.headers[3].value = (MAX_JSON_BYTES + 1).to_string().into_bytes();
    assert_eq!(boundary.preflight(&head), Err(WebError::Body));
}
