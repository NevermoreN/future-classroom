//! Real SQLite publication, native file pins and the shared two-worker fixture.
//! No connected listener or transport acknowledgement is simulated here.
use super::*;
use crate::web_transfer::{
    uploads::tests::{Fixture, Pool},
    TransferControl,
};
use std::{
    fs::OpenOptions,
    io::{Cursor, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

fn field(body: &[u8], name: &str) -> String {
    std::str::from_utf8(body)
        .unwrap()
        .split_once(&format!("\"{name}\":\""))
        .unwrap()
        .1
        .split('"')
        .next()
        .unwrap()
        .to_owned()
}
pub(in crate::web_transfer) fn published(
    f: &mut Fixture,
    pool: &Pool,
) -> (RequestHead, PathBuf, Vec<u8>) {
    let bytes = b"Synthetic worker-owned download data\n".repeat(4000);
    let (id, head, staged) = f.reserve(bytes.len());
    let root = staged.parent().unwrap().parent().unwrap().to_owned();
    let app = f.app.clone();
    let source = bytes.clone();
    let response = pool.call(move |_| app.dispatch_material_upload(&head, Cursor::new(source)));
    assert_eq!(response.head.status, 200);
    let (head, body) = f.append(id, 98301);
    let app = f.app.clone();
    let backend = f.backend.clone();
    let response = pool.call(move |_| app.dispatch_material_append(&backend, &head, &body));
    assert_eq!(
        response.head.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    let material_id = field(&response.body, "materialId");
    let mut head = f.head(&format!("materials/download/1/1/{material_id}/{id}"), 0);
    head.method = "GET".into();
    head.content_length = None;
    head.has_body = false;
    head.headers.retain(|header| {
        !header.name.eq_ignore_ascii_case("Content-Type")
            && !header.name.eq_ignore_ascii_case("Content-Length")
    });
    let path = root
        .join("teachers/1/subjects/1")
        .join(material_id)
        .join(format!("{id}.txt"));
    (head, path, bytes)
}
fn begin(
    pool: &Pool,
    hub: &DownloadHub,
    f: &Fixture,
    head: RequestHead,
    life: TransferLifetime,
) -> DownloadAdmission {
    let hub = hub.clone();
    let app = f.app.clone();
    let backend = f.backend.clone();
    pool.call(
        move |context| match hub.begin(&app, &backend, &head, life, context) {
            Ok(admission) => admission,
            Err(DownloadHubFailure::Hub(error)) => panic!("download Hub {error:?}"),
            Err(DownloadHubFailure::Response(response)) => {
                panic!("download response {}", response.head.status)
            }
        },
    )
}
fn cleanup(pool: &Pool, hub: &DownloadHub, token: RequestToken) {
    let hub = hub.clone();
    pool.call(move |context| hub.cleanup_request(token, context))
        .unwrap();
}
#[test]
fn download_head_requires_verified_hash_and_chunks_keep_native_pin_until_fresh_cleanup() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (head, path, expected) = published(&mut f, &pool);
    let (mut owner, hub) = DownloadHubOwner::new().unwrap();
    let (control, life) =
        TransferControl::new(head.token, Instant::now() - Duration::from_secs(30)).unwrap();
    let original_deadline = life.deadline();
    let admitted = begin(&pool, &hub, &f, head, life.clone());
    let key = admitted.key;
    assert_eq!(admitted.head.content_length, expected.len() as u64);
    assert!((admitted.send_fence)());
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    drop(admitted);
    let mut actual = Vec::new();
    let mut sequence = 1;
    let mut offset = 0;
    loop {
        let hub = hub.clone();
        // No Backend mutex spans a later file read or cached-first-block reply.
        let held = f.backend.lock().unwrap();
        let chunk = pool.call(
            move |context| match hub.read(key, sequence, offset, context) {
                Ok(chunk) => chunk,
                Err(_) => panic!("download read failed"),
            },
        );
        drop(held);
        assert_eq!(
            (chunk.key, chunk.sequence, chunk.offset),
            (key, sequence, offset)
        );
        assert!(!chunk.bytes.is_empty() && chunk.bytes.len() <= CHUNK_BYTES);
        offset += chunk.bytes.len() as u64;
        actual.extend_from_slice(&chunk.bytes);
        if chunk.eof {
            break;
        }
        sequence += 1;
    }
    assert_eq!(actual, expected);
    assert_eq!(life.deadline(), original_deadline);
    assert!(life.is_active());
    assert!(
        OpenOptions::new().write(true).open(&path).is_err(),
        "EOF metadata never closes a pin on Net"
    );
    control.cancel();
    let old = hub.clone();
    assert!(pool.call(move |context| matches!(
        old.read(key, sequence + 1, offset, context),
        Err(DownloadHubFailure::Hub(DownloadHubError::Cancelled))
    )));
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    cleanup(&pool, &hub, key.token());
    drop(OpenOptions::new().write(true).open(&path).unwrap());
    pool.join().unwrap();
    assert!(owner.drain_after_join().is_drained());
    drop(owner);
}
#[test]
fn eight_pins_are_bounded_duplicate_tokens_and_old_serials_cannot_replace_current_owner() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (head, path, _) = published(&mut f, &pool);
    let (mut owner, hub) = DownloadHubOwner::new().unwrap();
    let mut controls = Vec::new();
    let mut keys = Vec::new();
    for index in 0..MAX_REQUESTS {
        let mut request = head.clone();
        request.token = fc_ports::http::TokenIssuer::new()
            .unwrap()
            .issue(index)
            .unwrap();
        let (control, life) = TransferControl::new(request.token, Instant::now()).unwrap();
        keys.push(begin(&pool, &hub, &f, request, life).key);
        controls.push(control);
    }
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    let mut next = head.clone();
    next.token = fc_ports::http::TokenIssuer::new()
        .unwrap()
        .issue(0)
        .unwrap();
    let (next_control, next_life) = TransferControl::new(next.token, Instant::now()).unwrap();
    let worker = hub.clone();
    let app = f.app.clone();
    let backend = f.backend.clone();
    let pending = next.clone();
    let life = next_life.clone();
    assert!(pool.call(move |context| matches!(
        worker.begin(&app, &backend, &pending, life, context),
        Err(DownloadHubFailure::Hub(DownloadHubError::Busy))
    )));
    // Duplicate full token remains busy even if an independent cancellation
    // cell is supplied; it cannot claim or close one of the current pins.
    let mut duplicate = head;
    duplicate.token = keys[0].token();
    let (duplicate_control, duplicate_life) =
        TransferControl::new(duplicate.token, Instant::now()).unwrap();
    let worker = hub.clone();
    let app = f.app.clone();
    let backend = f.backend.clone();
    assert!(pool.call(move |context| matches!(
        worker.begin(&app, &backend, &duplicate, duplicate_life, context),
        Err(DownloadHubFailure::Hub(DownloadHubError::Busy))
    )));
    cleanup(&pool, &hub, keys[0].token());
    let replacement = begin(&pool, &hub, &f, next, next_life).key;
    assert_eq!(replacement.slot, keys[0].slot);
    assert!(replacement.serial > keys[0].serial);
    let stale = keys[0];
    let worker = hub.clone();
    assert!(pool.call(move |context| matches!(
        worker.read(stale, 1, 0, context),
        Err(DownloadHubFailure::Hub(DownloadHubError::Stale))
    )));
    for key in keys.into_iter().skip(1).chain([replacement]) {
        cleanup(&pool, &hub, key.token());
    }
    drop(OpenOptions::new().write(true).open(&path).unwrap());
    drop((controls, next_control, duplicate_control));
    pool.join().unwrap();
    assert!(owner.drain_after_join().is_drained());
    drop(owner);
}
#[test]
fn corrupt_published_bytes_cannot_escape_as_admitted_response_headers() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (head, path, _) = published(&mut f, &pool);
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .write_all(b"X")
        .unwrap();
    let (mut owner, hub) = DownloadHubOwner::new().unwrap();
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let worker = hub.clone();
    let app = f.app.clone();
    let backend = f.backend.clone();
    assert!(pool.call(move |context| matches!(
        worker.begin(&app, &backend, &head, life, context),
        Err(DownloadHubFailure::Hub(DownloadHubError::ReadFailed))
    )));
    assert!(
        OpenOptions::new().write(true).open(&path).is_err(),
        "failed hash keeps the original pin until actual cleanup"
    );
    cleanup(&pool, &hub, token);
    drop(OpenOptions::new().write(true).open(&path).unwrap());
    drop(control);
    pool.join().unwrap();
    assert!(owner.drain_after_join().is_drained());
    drop(owner);
}
#[test]
fn worker_unwind_closes_pin_but_original_domain_is_retained_until_real_join_and_drain() {
    let _physical = crate::web_application::upload_test_guard();
    let mut f = Fixture::new();
    let mut pool = Pool::new();
    let (head, path, _) = published(&mut f, &pool);
    let (owner, hub) = DownloadHubOwner::new().unwrap();
    let (control, life) = TransferControl::new(head.token, Instant::now()).unwrap();
    let key = begin(&pool, &hub, &f, head, life).key;
    let worker = hub.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pool.call(move |context| {
            let shared = worker_shared(&worker, context, true).unwrap();
            let _lease = shared.take(key).unwrap();
            drop(owner); // Cannot close the actual pin held outside the registry.
            assert!(matches!(
                DownloadHubOwner::new(),
                Err(DownloadHubError::Busy)
            ));
            panic!("diagnostic held-download lease unwind");
        })
    }));
    assert!(result.is_err());
    assert!(matches!(
        DownloadHubOwner::new(),
        Err(DownloadHubError::Busy)
    ));
    assert_eq!(pool.join(), Err(fc_runtime::worker::JoinError::Faulted));
    let mut original = DownloadHubOwner::recover_retained().unwrap();
    assert!(original.drain_after_join().is_drained());
    drop(original);
    drop(OpenOptions::new().write(true).open(&path).unwrap());
    drop(control);
    let (mut next, _) = DownloadHubOwner::new().unwrap();
    assert!(next.drain_after_join().is_drained());
    drop(next);
}
