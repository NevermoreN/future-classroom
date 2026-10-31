//! Actual heterogeneous HttpJob completion suppression and fresh token cleanup.
use super::*;
use crate::web_transfer::{
    downloads::{tests::published, DownloadHubOwner},
    uploads::tests::{Fixture, Pool},
    HttpCompletion, HttpJob, TransferControl,
};
use fc_runtime::worker::{JobError, WorkerPool};
use std::{fs::OpenOptions, time::Duration};

#[test]
fn cancelled_positive_download_admission_stays_worker_owned_until_fresh_http_cleanup_task() {
    let _physical = crate::web_application::upload_test_guard();
    let mut fixture = Fixture::new();
    let mut setup = Pool::new();
    let (head, path, _) = published(&mut fixture, &setup);
    setup.join().unwrap();
    let (mut owner, hub) = DownloadHubOwner::new().unwrap();
    let token = head.token;
    let (control, life) = TransferControl::new(token, Instant::now()).unwrap();
    let task =
        DownloadTask::begin(hub.clone(), &fixture.app, &fixture.backend, head, life).unwrap();
    let deadline = task.deadline();
    let pool = WorkerPool::<HttpJob>::start().unwrap();
    let mut ticket = match pool
        .handle()
        .try_submit(HttpJob::Download(Box::new(task)), deadline)
    {
        Ok(ticket) => ticket,
        Err(error) => panic!("real Http download admission rejected {:?}", error.kind),
    };
    let until = Instant::now() + Duration::from_secs(5);
    while !ticket.is_finished() {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    // The real positive metadata is already queued. Its late suppression is
    // disposed on this caller, and must not close the file in the worker Hub.
    ticket.cancel();
    assert!(matches!(ticket.try_result(), Err(JobError::Cancelled)));
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    drop(control);
    let deadline = Instant::now() + Duration::from_secs(5);
    let cleanup = DownloadTask::cleanup_request(hub, token, deadline);
    let closed = match pool
        .handle()
        .try_submit(HttpJob::Download(Box::new(cleanup)), deadline)
    {
        Ok(ticket) => ticket.wait().unwrap(),
        Err(error) => panic!("fresh download cleanup rejected {:?}", error.kind),
    };
    match closed {
        HttpCompletion::Download(DownloadCompletion {
            token: saved,
            result: Ok(DownloadOperationResult::CleanedRequest(cleaned)),
        }) => {
            assert_eq!(saved, token);
            assert_eq!(cleaned, token);
        }
        _ => panic!("exact token cleanup completion missing"),
    }
    drop(OpenOptions::new().write(true).open(&path).unwrap());
    pool.join().unwrap();
    assert!(owner.drain_after_join().is_drained());
    drop(owner);
}
