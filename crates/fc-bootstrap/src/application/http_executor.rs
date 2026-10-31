//! HTTP adapter for the application's existing two-worker domain. Construction
//! clones/receives a handle only; it neither starts workers nor enables a server.
use super::{Failure, Outcome, Work};
use fc_bootstrap::web_transfer::{HttpCompletion, HttpExecutor, HttpJob, HttpTicket};
use fc_runtime::worker::{JobError, JobTicket, SubmitError, WorkerHandle};

#[derive(Clone)]
pub(super) struct SharedHttpExecutor {
    handle: WorkerHandle<Work>,
}
impl SharedHttpExecutor {
    // The explicit candidate coordinator will supply this existing handle.
    // Ordinary startup must not create a listener merely to use this adapter.
    pub(super) fn new(handle: WorkerHandle<Work>) -> Self {
        Self { handle }
    }
}

pub(super) struct SharedHttpTicket {
    inner: JobTicket<super::Result<Outcome>>,
}
impl HttpExecutor for SharedHttpExecutor {
    type Ticket = SharedHttpTicket;
    fn try_submit_http(&self, task: HttpJob) -> Result<Self::Ticket, SubmitError<HttpJob>> {
        let deadline = task.deadline();
        match self.handle.try_submit(Work::Http(task), deadline) {
            Ok(inner) => Ok(SharedHttpTicket { inner }),
            Err(rejected) => {
                // WorkerHandle returns the same unexecuted input on rejection.
                let Work::Http(task) = rejected.task else {
                    unreachable!("shared worker replaced a rejected HTTP input")
                };
                Err(SubmitError {
                    kind: rejected.kind,
                    task,
                })
            }
        }
    }
}
fn completion(outcome: super::Result<Outcome>) -> Result<HttpCompletion, JobError> {
    match outcome {
        Ok(Outcome::Http(output)) => Ok(output),
        Err(Failure::Stopped) => Err(JobError::Stopped),
        // A type/routing or application failure cannot become a positive HTTP
        // reply. Work::Http itself only constructs metadata/byte completions.
        Ok(_) | Err(_) => Err(JobError::Faulted),
    }
}
impl HttpTicket for SharedHttpTicket {
    fn cancel(&self) {
        self.inner.cancel();
    }
    fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }
    fn try_result(&mut self) -> Result<Option<HttpCompletion>, JobError> {
        match self.inner.try_result()? {
            Some(outcome) => completion(outcome).map(Some),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_bootstrap::{
        backend::Backend,
        web_application::WebApplication,
        web_transfer::{TransferControl, WebCompletion, WebTask},
    };
    use fc_ports::http::{Header, ListenPrefix, RequestHead, TokenIssuer};
    use fc_runtime::worker::Rejection;
    use fc_security::CredentialRecordCodec;
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    // The task keeps Weak references. Retiring the uninstalled fixture before
    // submission produces a deterministic metadata-only 503 without KDF/UI/TLS.
    fn metadata_job(
        admitted_at: Instant,
    ) -> (TransferControl, HttpJob, fc_ports::http::RequestToken) {
        let directory = Directory(std::env::temp_dir().join(format!(
                "FutureClassroom-Test-Shared-HTTP-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&directory.0).unwrap();
        let clock = Arc::new(fc_windows::system::CurrentClock::new());
        let backend = Arc::new(Mutex::new(
            Backend::open(
                directory.0.join("state.sqlite3"),
                CredentialRecordCodec::from_key([39; 32]).unwrap(),
                clock.clone(),
                clock,
            )
            .unwrap(),
        ));
        let app = Arc::new(
            WebApplication::new(
                &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
                backend.lock().unwrap().web_service(),
            )
            .unwrap(),
        );
        let token = TokenIssuer::new().unwrap().issue(0).unwrap();
        let (control, life) = TransferControl::new(token, admitted_at).unwrap();
        let head = RequestHead {
            token,
            method: "GET".into(),
            target: "/fc/api/v1/status".into(),
            headers: vec![Header {
                name: "Host".into(),
                value: b"board.school".to_vec(),
            }],
            tls: true,
            content_length: None,
            has_body: false,
        };
        let job = HttpJob::Web(Box::new(
            WebTask::admit(&backend, &app, head, life).unwrap(),
        ));
        backend.lock().unwrap().close().unwrap();
        drop(app);
        drop(backend);
        drop(directory);
        (control, job, token)
    }
    #[test]
    fn http_metadata_uses_the_existing_work_pool_and_preserves_rejected_job_ownership() {
        let pool = crate::application::shared_pool::SharedPool::<Work>::start().unwrap();
        let executor = SharedHttpExecutor::new(pool.handle());
        let (_control, job, token) = metadata_job(Instant::now());
        let mut ticket = match executor.try_submit_http(job) {
            Ok(ticket) => ticket,
            Err(error) => panic!("unexpected rejection: {:?}", error.kind),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        let output = loop {
            match ticket.try_result() {
                Ok(Some(output)) => break output,
                Ok(None) if Instant::now() < deadline => std::thread::yield_now(),
                Ok(None) => panic!("shared HTTP worker did not return"),
                Err(error) => panic!("unexpected shared HTTP error: {error:?}"),
            }
        };
        assert!(ticket.is_finished());
        let HttpCompletion::Web(WebCompletion::Response {
            token: actual,
            response,
        }) = output
        else {
            panic!("wrong HTTP completion")
        };
        assert_eq!(actual, token);
        assert_eq!(response.head.status, 503);
        let (_control, job, _) = metadata_job(Instant::now() - Duration::from_secs(121));
        let deadline = job.deadline();
        let address = match &job {
            HttpJob::Web(web) => std::ptr::from_ref(web.as_ref()),
            _ => unreachable!(),
        };
        let rejected = match executor.try_submit_http(job) {
            Err(rejected) => rejected,
            Ok(_) => panic!("expired HTTP job was admitted"),
        };
        assert_eq!(rejected.kind, Rejection::Deadline);
        assert_eq!(rejected.task.deadline(), deadline);
        let HttpJob::Web(web) = rejected.task else {
            panic!("rejection changed the task kind")
        };
        assert_eq!(std::ptr::from_ref(web.as_ref()), address);
        drop(web);
        drop(ticket);
        drop(executor);
        pool.request_stop();
        pool.join().unwrap();
    }
    #[test]
    fn non_http_outcomes_and_application_errors_are_never_positive_replies() {
        assert!(matches!(
            completion(Ok(Outcome::MaterialCleaned)),
            Err(JobError::Faulted)
        ));
        assert!(matches!(
            completion(Err(Failure::Stopped)),
            Err(JobError::Stopped)
        ));
        assert!(matches!(
            completion(Err(Failure::Input("wrong job route"))),
            Err(JobError::Faulted)
        ));
    }
}
