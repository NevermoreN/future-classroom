use super::*;

fn fixture() -> Value {
    json!({
        "format":1,"revision":1,
        "installation":{"school":1,"name":"School","class":1},
        "accounts":[
            {"principal":{"id":1,"school":1,"role":"global","enabled":true,"epoch":1},"login":"admin","display":"Admin","change":false,"credential":1,"recovery":1},
            {"principal":{"id":2,"school":1,"role":"teacher","enabled":true,"epoch":1},"login":"teacher","display":"Teacher","change":false,"credential":1,"recovery":1}
        ],
        "classes":[{"id":1,"school":1,"name":"Class","enabled":true}],
        "subjects":[{"id":1,"school":1,"name":"Subject","enabled":true,"color":[20,40,60],"revision":1}],
        "assignments":[{"teacher":2,"class":1,"subject":1,"enabled":true}],
        "defaults":[{"class":1,"subject":1,"teacher":2}]
    })
}

#[test]
fn bounded_json_rejects_recursive_and_escaped_duplicate_keys_before_value_conversion() {
    for input in [
        r#"{"role":"user","role":"global"}"#,
        r#"{"outer":[{"safe":true,"nested":{"role":"user","role":"global"}}]}"#,
        r#"{"a":1,"\u0061":2}"#,
        r#"[{},[{"x":0,"x":1}]]"#,
    ] {
        assert_eq!(
            value(input.as_bytes()),
            Err(StorageError::Database),
            "{input}"
        );
    }
    let document = serde_json::to_string(&fixture()).unwrap();
    let duplicate = document.replace(
        "\"role\":\"teacher\"",
        "\"role\":\"user\",\"role\":\"teacher\"",
    );
    assert_ne!(document, duplicate);
    assert!(value(duplicate.as_bytes()).is_err());
    assert!(read_identity(&value(document.as_bytes()).unwrap()).is_ok());
}

#[test]
fn storage_json_enforces_exact_byte_depth_and_container_budgets() {
    let at_limit = format!("\"{}\"", "x".repeat(crate::MAX_VALUE_BYTES - 2));
    assert_eq!(at_limit.len(), crate::MAX_VALUE_BYTES);
    assert!(value(at_limit.as_bytes()).is_ok());
    assert!(value(format!("{at_limit} ").as_bytes()).is_err());
    let nested = |depth| format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
    assert!(value(nested(32).as_bytes()).is_ok());
    assert!(value(nested(33).as_bytes()).is_err());
    for count in [4096, 4097] {
        let array = format!("[{}]", vec!["0"; count].join(","));
        let object = format!(
            "{{{}}}",
            (0..count)
                .map(|i| format!("\"{i}\":0"))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(value(array.as_bytes()).is_ok(), count == 4096);
        assert_eq!(value(object.as_bytes()).is_ok(), count == 4096);
    }
    for invalid in [
        "",
        "null null",
        "{}[]",
        "{\"x\":1,}",
        "[1,]",
        "{\"x\":\"\\ud800\"}",
    ] {
        assert!(value(invalid.as_bytes()).is_err(), "{invalid}");
    }
}

#[test]
fn every_identity_object_rejects_unknown_and_missing_fields_including_nullable_fields() {
    let original = fixture();
    assert!(read_identity(&original).is_ok());
    for path in [
        "",
        "/installation",
        "/accounts/0",
        "/accounts/0/principal",
        "/accounts/1",
        "/accounts/1/principal",
        "/classes/0",
        "/subjects/0",
        "/assignments/0",
        "/defaults/0",
    ] {
        let fields: Vec<_> = original
            .pointer(path)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for field in fields {
            let mut missing = original.clone();
            missing
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(&field);
            assert!(
                read_identity(&missing).is_err(),
                "accepted missing field {path}/{field}"
            );
        }
        let mut unknown = original.clone();
        unknown
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("futureField".into(), Value::Null);
        assert!(
            read_identity(&unknown).is_err(),
            "accepted unknown field {path}/futureField"
        );
    }
    let mut nullable = original;
    nullable["installation"]["class"] = Value::Null;
    assert!(read_identity(&nullable).is_ok());
    let empty = identity(&IdentityState::default());
    assert!(read_identity(&empty).is_ok());
}

#[test]
fn record_objects_are_strict_and_preserve_existing_semantic_corruption_checks() {
    let record = json!({"key":{"kind":"homework","class":1,"date":[2026,10,30],"subject":1,"owner":2},"text":"Homework","revision":1,"actor":2,"at":{"date":[2026,10,30],"second":3600}});
    assert!(read_record(&record).is_ok());
    for path in ["", "/key", "/at"] {
        for field in record.pointer(path).unwrap().as_object().unwrap().keys() {
            let mut missing = record.clone();
            missing
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(read_record(&missing).is_err(), "missing {path}/{field}");
        }
        let mut unknown = record.clone();
        unknown
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), Value::Bool(true));
        assert!(read_record(&unknown).is_err(), "unknown {path}/extra");
    }
    let mut reminder = record.clone();
    reminder["key"] = json!({"kind":"reminder","class":1,"date":[2026,10,30]});
    assert!(read_record(&reminder).is_ok());
    reminder["key"]["owner"] = json!(2);
    assert!(read_record(&reminder).is_err());
    for (field, bad) in [
        ("revision", json!(0)),
        ("text", json!(" untrimmed ")),
        ("text", json!("x".repeat(16 * 1024 + 1))),
    ] {
        let mut invalid = record.clone();
        invalid[field] = bad;
        assert!(read_record(&invalid).is_err());
    }
    let mut wrong_date = record;
    wrong_date["at"]["date"] = json!([2026, 10, 31]);
    assert!(read_record(&wrong_date).is_err());
}

#[test]
fn exact_object_helper_rejects_missing_null_unknown_and_nonobjects() {
    assert!(object_fields(&json!({"repair":null}), &["repair"]).is_ok());
    assert!(object_fields(&json!({}), &["repair"]).is_err());
    assert!(object_fields(&json!({"repair":null,"extra":0}), &["repair"]).is_err());
    assert!(object_fields(&Value::Null, &[]).is_err());
}
