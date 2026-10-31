//! Real SQLite append receipt through the actual upload task after the transient
//! Ready slot was consumed. No second validation or publication is replayed.
use super::*;
use crate::web_transfer::uploads::{
    tests::{Fixture, Pool},
    UploadHubOwner,
};

fn operation(pool: &Pool, task: UploadTask) -> Result<UploadOperationResult, UploadHubFailure> {
    pool.call(move |context| task.run(context)).result
}
fn success<T>(result: Result<T, UploadHubFailure>) -> T {
    match result {
        Ok(value) => value,
        Err(UploadHubFailure::Hub(error)) => panic!("Hub error: {error:?}"),
        Err(UploadHubFailure::Response(response)) => panic!(
            "HTTP {}: {}",
            response.head.status,
            String::from_utf8_lossy(&response.body)
        ),
    }
}
fn response(result: Result<UploadOperationResult, UploadHubFailure>) -> WebResponse {
    match result {
        Ok(UploadOperationResult::Appended(response))
        | Err(UploadHubFailure::Response(response)) => response,
        Ok(_) => panic!("wrong append completion"),
        Err(UploadHubFailure::Hub(error)) => {
            panic!("receipt must precede transient Ready lookup: {error:?}")
        }
    }
}
fn append(
    pool: &Pool,
    hub: &UploadHub,
    f: &Fixture,
    head: RequestHead,
    body: Vec<u8>,
    cancel: bool,
) -> WebResponse {
    let (control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let task =
        UploadTask::append_request(hub.clone(), &f.app, &f.backend, head, body, life).unwrap();
    if cancel {
        control.cancel();
    }
    let result = response(operation(pool, task));
    drop(control);
    result
}
#[test]
fn successful_append_retry_reads_exact_durable_receipt_before_the_now_vacant_hub() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (mut owner, hub) = UploadHubOwner::new().unwrap();
    let bytes = b"Synthetic exact append receipt\n";
    let (id, head, path) = f.reserve(bytes.len());
    let (control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let task = UploadTask::begin(
        hub.clone(),
        &f.app,
        UploadKind::Material,
        head,
        control,
        life,
    )
    .unwrap();
    let UploadOperationResult::Admitted(admission) = success(operation(&pool, task)) else {
        panic!("missing admission")
    };
    let key = admission.key;
    let task =
        UploadTask::write(hub.clone(), key, 1, 0, bytes.to_vec(), admission.deadline).unwrap();
    assert!(matches!(
        success(operation(&pool, task)),
        UploadOperationResult::Written(_)
    ));
    let task = UploadTask::finish(hub.clone(), &f.app, key, admission.deadline);
    let UploadOperationResult::Finished(finished) = success(operation(&pool, task)) else {
        panic!("missing Ready result")
    };
    assert_eq!(finished.response.head.status, 200);
    let (head, body) = f.append(id, 97001);
    let first = append(&pool, &hub, &f, head, body, false);
    assert_eq!(
        first.head.status,
        200,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.ready_material_key(id, context))
            .unwrap(),
        None
    );
    assert!(!path.exists(), "the original staging object was consumed");
    let revision = f.backend.lock().unwrap().global_revision().unwrap();

    let (head, body) = f.append(id, 97001);
    let retried = append(&pool, &hub, &f, head, body, false);
    assert_eq!(retried.head.status, 200);
    assert_eq!(
        retried.body, first.body,
        "retry returns the historical exact receipt"
    );

    // A new command sharing the old ID cannot silently change any field of the
    // authoritative append tuple, even though its Ready key no longer exists.
    let other_id = id.checked_add(1).unwrap_or(id - 1);
    let (head, body) = f.append(other_id, 97001);
    let conflict = append(&pool, &hub, &f, head, body, false);
    assert_eq!(conflict.head.status, 409);
    assert!(String::from_utf8_lossy(&conflict.body).contains("IDEMPOTENCY_CONFLICT"));
    for (route, classes) in [
        ("materials/folders/1/1/append", "2"),
        ("materials/folders/1/2/append", "1"),
    ] {
        let body=format!(r#"{{"schemaVersion":1,"commandId":"97001","uploadId":"{id}","allowedClassIds":["{classes}"]}}"#).into_bytes();
        let head = f.head(route, body.len());
        let conflict = append(&pool, &hub, &f, head, body, false);
        assert_eq!(conflict.head.status, 409);
        assert!(String::from_utf8_lossy(&conflict.body).contains("IDEMPOTENCY_CONFLICT"));
    }

    let (head, body) = f.append(id, 97001);
    let cancelled = append(&pool, &hub, &f, head, body, true);
    assert_eq!(cancelled.head.status, 503);
    assert!(!String::from_utf8_lossy(&cancelled.body).contains("COMMITTED"));
    let (mut head, body) = f.append(id, 97001);
    head.headers
        .iter_mut()
        .find(|header| header.name.eq_ignore_ascii_case("x-fc-csrf"))
        .unwrap()
        .value = b"invalid".to_vec();
    assert_eq!(append(&pool, &hub, &f, head, body, false).head.status, 403);
    assert_eq!(
        f.backend.lock().unwrap().global_revision().unwrap(),
        revision,
        "receipt retries cannot replay SQL mutation"
    );
    let worker = hub.clone();
    assert_eq!(
        pool.call(move |context| worker.ready_material_key(id, context))
            .unwrap(),
        None
    );
    pool.join().unwrap();
    assert!(owner.drain_after_join(&f.app).is_drained());
}
