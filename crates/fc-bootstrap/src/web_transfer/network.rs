//! Explicit loopback candidate composition, not an ordinary listener entry.
//! One caller-owned Net domain drives the existing TLS owner and existing shared
//! executor. File owners remain in their fixed worker Hubs, never Net replies.
use super::{
    DownloadHub, DownloadHubError, DownloadHubFailure, DownloadKey, DownloadOperationResult,
    DownloadTask, HttpCompletion, HttpExecutor, HttpJob, HttpTicket, TransferCancellation,
    TransferControl, TransferLifetime, UploadCursor, UploadHub, UploadHubError, UploadHubFailure,
    UploadKey, UploadKind, UploadOperationResult, UploadTask, WebCompletion, WebTask,
};
use crate::{
    backend::Backend,
    web_application::{WebApplication, WebResponse},
    web_assets::{StaticAssets, StaticBody, StaticError},
};
use fc_ports::http::{Header, RequestHead, RequestToken, ResponseHead, CHUNK_BYTES, MAX_REQUESTS};
use fc_protocol::{
    web::{materials::MaterialRoute, responses, Route, WebError},
    MAX_JSON_BYTES,
};
use fc_runtime::worker::{Rejection, Task};
use fc_windows::tls_frontdoor::{TlsAcceptSnapshot, TlsHttpEvent, TlsHttpHost};
use std::{
    any::Any,
    cell::RefCell,
    mem::size_of,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

/// Read-only live JSON-body interval; no identity, route, headers or body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestInterval {
    pub token: RequestToken,
    pub admitted_at: Instant,
    pub deadline: Instant,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetworkReport {
    pub acceptance: TlsAcceptSnapshot,
    pub json_body_records: usize,
    pub json_body_intervals: [Option<RequestInterval>; MAX_REQUESTS],
    pub heads: u64,
    pub admissions: u64,
    pub body_acks: u64,
    pub responses: u64,
    pub closed: u64,
    pub retired: u64,
    pub rearms: u64,
    pub stale_events: u64,
    pub active_records: usize,
    pub peak_records: usize,
    pub pending_jobs: usize,
    pub cleanup_pending: usize,
    pub retained_cleanup: usize,
    pub native_drained: bool,
    pub stopping: bool,
    /// Current retained native, record, pending-job and bounded JSON capacities.
    /// Excludes queued/running executor allocations, caller-budgeted response
    /// fence closure storage, provider/kernel/runtime and delivered event temporaries.
    pub owned_bytes: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Head,
    Admitting,
    Beginning,
    JsonBody,
    StaticRequestBody,
    UploadBody,
    Writing,
    Finishing,
    JsonWork,
    DownloadBegin,
    DownloadRead,
    Sending,
    Closed,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Admit,
    Begin,
    Write { sequence: u64, end: u64, eof: bool },
    Finish,
    Json,
    Append,
    DownloadBegin,
    DownloadRead { sequence: u64, offset: u64 },
    Cleanup,
}
/// A worker reply advances no physical-send position. Only the matching native
/// ResponseReady/ResponseComplete event acknowledges the single staged chunk.
struct DownloadCursor {
    expected: u64,
    sequence: u64,
    offset: u64,
    pending: Option<(u64, bool)>,
    complete: bool,
}
impl DownloadCursor {
    fn new(expected: u64) -> Self {
        Self {
            expected,
            sequence: 1,
            offset: 0,
            pending: None,
            complete: false,
        }
    }
    fn position(&self) -> Result<(u64, u64), ()> {
        if self.pending.is_some() || self.complete {
            return Err(());
        }
        Ok((self.sequence, self.offset))
    }
    fn accept(&mut self, sequence: u64, offset: u64, bytes: usize, eof: bool) -> Result<(), ()> {
        let end = offset.checked_add(bytes as u64).ok_or(())?;
        if self.position()? != (sequence, offset)
            || bytes > CHUNK_BYTES
            || end > self.expected
            || eof != (end == self.expected)
            || (!eof && bytes == 0)
            || sequence.checked_add(1).is_none()
        {
            return Err(());
        }
        self.pending = Some((end, eof));
        Ok(())
    }
    fn acknowledge(&mut self, eof: bool) -> Result<(), ()> {
        let (end, final_chunk) = self.pending.ok_or(())?;
        if final_chunk != eof {
            return Err(());
        }
        self.pending = None;
        self.offset = end;
        self.sequence = self.sequence.checked_add(1).ok_or(())?;
        self.complete = eof;
        Ok(())
    }
}
fn static_chunk<'a>(
    cursor: &mut DownloadCursor,
    body: &'a mut StaticBody,
) -> Result<(&'a [u8], bool), ()> {
    // Check physical progress BEFORE advancing the immutable source cursor.
    let (sequence, offset) = cursor.position()?;
    let remaining = body.remaining_bytes();
    if remaining as u64 != cursor.expected.checked_sub(offset).ok_or(())? {
        return Err(());
    }
    let bytes = body.next_chunk().unwrap_or(b"");
    if remaining != 0 && bytes.is_empty() {
        return Err(());
    }
    let eof = remaining <= CHUNK_BYTES;
    cursor.accept(sequence, offset, bytes.len(), eof)?;
    Ok((bytes, eof))
}
enum JobState<T> {
    Pending(HttpJob),
    Running(T),
}
struct Work<T> {
    purpose: Purpose,
    state: JobState<T>,
}
struct Record<T> {
    token: RequestToken,
    phase: Phase,
    head: Option<RequestHead>,
    route: Option<Route>,
    control: Option<TransferControl>,
    life: Option<TransferLifetime>,
    cancellation: Option<TransferCancellation>,
    work: Option<Work<T>>,
    json: Vec<u8>,
    json_sequence: u64,
    key: Option<UploadKey>,
    cursor: Option<UploadCursor>,
    download_request: bool,
    download_key: Option<DownloadKey>,
    download_cursor: Option<DownloadCursor>,
    static_head: Option<ResponseHead>,
    static_body: Option<StaticBody>,
    static_cursor: Option<DownloadCursor>,
    deadline: Option<Instant>,
    hub_touched: bool,
    cleanup_done: bool,
    cleanup_failed: bool,
    material_ready: bool,
    response_complete: bool,
    closed: bool,
    closed_seen: bool,
    native_retired: bool,
    native_rearmed: bool,
}
impl<T: HttpTicket> Record<T> {
    fn new(token: RequestToken) -> Self {
        Self {
            token,
            phase: Phase::Head,
            head: None,
            route: None,
            control: None,
            life: None,
            cancellation: None,
            work: None,
            json: Vec::new(),
            json_sequence: 1,
            key: None,
            cursor: None,
            download_request: false,
            download_key: None,
            download_cursor: None,
            static_head: None,
            static_body: None,
            static_cursor: None,
            deadline: None,
            hub_touched: false,
            cleanup_done: true,
            cleanup_failed: false,
            material_ready: false,
            response_complete: false,
            closed: false,
            closed_seen: false,
            native_retired: false,
            native_rearmed: false,
        }
    }
    fn queue(&mut self, purpose: Purpose, job: HttpJob) -> Result<(), String> {
        if self.work.is_some() {
            return Err("one HTTP job per request record".into());
        }
        self.work = Some(Work {
            purpose,
            state: JobState::Pending(job),
        });
        Ok(())
    }
    fn cancel(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.phase = Phase::Closed;
        // A completed material response leaves its ORIGINAL control in Hub.
        if !(self.material_ready && self.response_complete) {
            if let Some(cancel) = &self.cancellation {
                cancel.cancel();
            }
            if let Some(control) = &self.control {
                control.cancel();
            }
        }
        if let Some(work) = &self.work {
            if work.purpose != Purpose::Cleanup {
                if let JobState::Running(ticket) = &work.state {
                    ticket.cancel();
                }
            }
        }
        // Pending is NOT accepted by the executor: dropping this original job
        // retires only bounded bytes/weak refs/atomic control, never a stage.
        if self.work.as_ref().is_some_and(|work| {
            work.purpose != Purpose::Cleanup && matches!(work.state, JobState::Pending(_))
        }) {
            self.work = None;
        }
    }
    fn may_release(&self) -> bool {
        self.native_retired && self.work.is_none() && (!self.hub_touched || self.cleanup_done)
    }
    fn buffer_json(
        &mut self,
        sequence: u64,
        offset: u64,
        bytes: &[u8],
        eof: bool,
    ) -> Result<u64, ()> {
        let expected = self.head.as_ref().ok_or(())?.content_length.unwrap_or(0);
        let end = offset.checked_add(bytes.len() as u64).ok_or(())?;
        if sequence != self.json_sequence
            || offset != self.json.len() as u64
            || expected > MAX_JSON_BYTES as u64
            || bytes.len() > MAX_JSON_BYTES
            || end > expected
            || (eof && end != expected)
            || (!eof && bytes.is_empty())
        {
            return Err(());
        }
        let next = sequence.checked_add(1).ok_or(())?;
        self.json.extend_from_slice(bytes);
        self.json_sequence = next;
        Ok(end)
    }
    fn owned_bytes(&self) -> usize {
        let head = self
            .head
            .as_ref()
            .and_then(super::work::head_allocation)
            .unwrap_or(0);
        let pending = self
            .work
            .as_ref()
            .and_then(|work| match &work.state {
                JobState::Pending(job) => job.allocation_bytes(),
                JobState::Running(_) => Some(0),
            })
            .unwrap_or(0);
        let static_head = self.static_head.as_ref().map_or(0, |head| {
            head.headers.capacity() * size_of::<Header>()
                + head
                    .headers
                    .iter()
                    .map(|header| header.name.capacity() + header.value.capacity())
                    .sum::<usize>()
        });
        // StaticBody's inline fields already belong to the fixed Record array;
        // shared payload is charged exactly once by StaticAssets below.
        let static_body = self.static_body.as_ref().map_or(0, |body| {
            body.owned_bytes().saturating_sub(size_of::<StaticBody>())
        });
        head + static_head + static_body + self.json.capacity() + pending
    }
}
struct Core<E: HttpExecutor> {
    host: TlsHttpHost,
    executor: E,
    backend: Weak<Mutex<Backend>>,
    app: Weak<WebApplication>,
    hub: UploadHub,
    downloads: DownloadHub,
    static_assets: Option<Arc<StaticAssets>>,
    started: bool,
    records: [Option<Record<E::Ticket>>; MAX_REQUESTS],
    report: NetworkReport,
}
thread_local! {
    // One exact owner, never replaced by a later capability. TlsHttpHost's
    // process admission lease prevents another host while this Core exists.
    static RETAINED_CORE: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
}
/// The external coordinator MUST keep the original app/Backend/Hub owner and
/// shared pool alive. Stop/poll to Net idle, then actually join that same pool
/// and call both UploadHubOwner::drain_after_join and
/// DownloadHubOwner::drain_after_join. Net idle is not global shutdown.
/// A premature Drop retains this bounded core on its original thread, including
/// its native owner and unfinished tickets. Recover it on that same thread to
/// continue draining. Thread exit falls back to the native owner's fail-closed
/// Drop; retention never creates a replacement listener, executor or owner.
pub struct NetCoordinator<E: HttpExecutor + 'static>
where
    E::Ticket: 'static,
{
    core: Option<Box<Core<E>>>,
}
impl<E: HttpExecutor + 'static> NetCoordinator<E>
where
    E::Ticket: 'static,
{
    pub fn new(
        host: TlsHttpHost,
        executor: E,
        backend: &Arc<Mutex<Backend>>,
        app: &Arc<WebApplication>,
        hub: UploadHub,
        downloads: DownloadHub,
    ) -> Self {
        // A retained original capability must be recovered, never superseded.
        RETAINED_CORE.with(|slot| assert!(slot.borrow().is_none(), "recover retained Net owner"));
        let mut records = std::array::from_fn(|_| None);
        for (index, token) in host.tokens().into_iter().enumerate() {
            records[index] = Some(Record::new(token));
        }
        let mut core = Box::new(Core {
            host,
            executor,
            backend: Arc::downgrade(backend),
            app: Arc::downgrade(app),
            hub,
            downloads,
            static_assets: None,
            started: false,
            records,
            report: NetworkReport {
                peak_records: 4,
                ..NetworkReport::default()
            },
        });
        core.refresh_report();
        Self { core: Some(core) }
    }
    /// Install the immutable worker-verified package before any native poll or
    /// Head processing. It cannot be replaced during this owner's lifetime.
    pub fn set_static_assets(&mut self, assets: Arc<StaticAssets>) -> Result<(), String> {
        let core = self.core_mut();
        if core.started || core.static_assets.is_some() {
            return Err("static package must be installed once before first Net poll".into());
        }
        core.static_assets = Some(assets);
        core.refresh_report();
        Ok(())
    }
    /// Take back the original stopping owner on its original thread. A type
    /// mismatch leaves the retained owner intact; it must not open a new host.
    pub fn recover_retained() -> Result<Option<Self>, String> {
        RETAINED_CORE.with(|slot| {
            let mut retained = slot.borrow_mut();
            let Some(owner) = retained.as_ref() else {
                return Ok(None);
            };
            if !owner.is::<Core<E>>() {
                return Err("retained Net owner has a different executor type".into());
            }
            let owner = retained.take().expect("checked retained owner");
            match owner.downcast::<Core<E>>() {
                Ok(core) => Ok(Some(Self { core: Some(core) })),
                Err(owner) => {
                    // Keep the exact capability even if the type check changes.
                    *retained = Some(owner);
                    Err("retained Net owner type mismatch".into())
                }
            }
        })
    }
    fn core(&self) -> &Core<E> {
        self.core.as_deref().expect("owned Net core")
    }
    fn core_mut(&mut self) -> &mut Core<E> {
        self.core.as_deref_mut().expect("owned Net core")
    }
    pub fn port_number(&self) -> u16 {
        self.core().host.port_number()
    }
    pub fn generation(&self) -> u64 {
        self.core().host.generation()
    }
    pub fn report(&self) -> NetworkReport {
        self.core().report
    }
    /// Fixed metadata only; does not poll, close or release a native owner.
    pub fn close_snapshot(&self) -> fc_windows::tls_frontdoor::TlsCloseSnapshot {
        self.core().host.close_snapshot()
    }
    pub fn is_idle(&self) -> bool {
        self.core().report.native_drained && self.core().records.iter().all(Option::is_none)
    }
    pub fn poll(&mut self) -> Result<NetworkReport, String> {
        self.core_mut().poll()
    }
    pub fn request_stop(&mut self) -> Result<(), String> {
        self.core_mut().request_stop()
    }
}
impl<E: HttpExecutor + 'static> Drop for NetCoordinator<E>
where
    E::Ticket: 'static,
{
    fn drop(&mut self) {
        let Some(mut core) = self.core.take() else {
            return;
        };
        if core.report.native_drained && core.records.iter().all(Option::is_none) {
            return;
        }
        let _ = core.request_stop();
        // All public construction/recovery is same-thread and exclusive. If
        // thread-local destruction has already begun, normal Core destruction
        // instead invokes the existing native fail-closed Drop path.
        let mut original = Some(core);
        let _ = RETAINED_CORE.try_with(|slot| {
            if let Ok(mut retained) = slot.try_borrow_mut() {
                if retained.is_none() {
                    *retained = original.take().map(|owner| owner as Box<dyn Any>);
                }
            }
        });
        // Never overwrite a retained owner. The occupied/reentrant case cannot
        // arise through the singleton host API; fallback still drops normally.
    }
}
fn error_response(status: u16, code: responses::ErrorCode) -> WebResponse {
    let body = responses::error(code).unwrap_or_else(|_| b"{}".to_vec());
    WebResponse {
        head: ResponseHead {
            status,
            headers: vec![
                Header {
                    name: "Content-Type".into(),
                    value: b"application/json".to_vec(),
                },
                Header {
                    name: "Cache-Control".into(),
                    value: b"no-store".to_vec(),
                },
            ],
            content_length: body.len() as u64,
        },
        body,
    }
}
fn static_error(error: StaticError) -> WebResponse {
    let (status, code) = match error {
        StaticError::NotFound | StaticError::Boundary(WebError::NotFound) => {
            (404, responses::ErrorCode::NotFound)
        }
        StaticError::Boundary(WebError::Method) => (405, responses::ErrorCode::MethodNotAllowed),
        StaticError::Boundary(
            WebError::Transport | WebError::Host | WebError::Origin | WebError::Csrf,
        ) => (403, responses::ErrorCode::Forbidden),
        StaticError::Boundary(_) => (400, responses::ErrorCode::InvalidRequest),
        StaticError::InvalidManifest | StaticError::PayloadMismatch => {
            (503, responses::ErrorCode::ServiceUnavailable)
        }
    };
    let mut response = error_response(status, code);
    if status == 405 {
        response.head.headers.push(Header {
            name: "Allow".into(),
            value: b"GET".to_vec(),
        });
    }
    response
}
fn event_token(event: &TlsHttpEvent) -> RequestToken {
    match event {
        TlsHttpEvent::Head { head, .. } => head.token,
        TlsHttpEvent::Body { token, .. } | TlsHttpEvent::Closed { token } => *token,
        TlsHttpEvent::ResponseReady(token)
        | TlsHttpEvent::ResponseComplete(token)
        | TlsHttpEvent::Retired(token) => *token,
    }
}
fn find_record<T>(
    records: &[Option<Record<T>>; MAX_REQUESTS],
    token: RequestToken,
) -> Option<usize> {
    records
        .iter()
        .position(|record| record.as_ref().is_some_and(|record| record.token == token))
}
impl<E: HttpExecutor> Core<E> {
    fn close_record(&mut self, record: &mut Record<E::Ticket>) {
        record.cancel();
        let _ = self.host.cancel(record.token);
    }
    fn live(&self, record: &Record<E::Ticket>) -> bool {
        !record.closed
            && record
                .life
                .as_ref()
                .is_some_and(TransferLifetime::is_active)
            && record
                .deadline
                .is_some_and(|deadline| Instant::now() < deadline)
    }
    fn composition(&self) -> Option<(Arc<Mutex<Backend>>, Arc<WebApplication>)> {
        Some((self.backend.upgrade()?, self.app.upgrade()?))
    }
    fn send_response(&mut self, record: &mut Record<E::Ticket>, response: WebResponse) {
        if record.closed {
            return;
        }
        let valid = response.body.len() <= MAX_JSON_BYTES
            && response.head.content_length == response.body.len() as u64
            && response.head.validate().is_ok();
        let response = if valid {
            response
        } else {
            error_response(503, responses::ErrorCode::ServiceUnavailable)
        };
        if response.head.status < 400 && !self.live(record) {
            self.close_record(record);
            return;
        }
        if self
            .host
            .respond(record.token, response.head, &response.body, true)
            .is_err()
        {
            self.close_record(record);
        } else {
            record.phase = Phase::Sending;
        }
    }
    fn head(
        &mut self,
        record: &mut Record<E::Ticket>,
        head: RequestHead,
        at: Instant,
    ) -> Result<(), String> {
        if record.phase != Phase::Head || record.head.is_some() || head.token != record.token {
            self.close_record(record);
            return Ok(());
        }
        self.started = true;
        self.report.heads = self.report.heads.saturating_add(1);
        let (control, life) =
            TransferControl::new(head.token, at).map_err(|_| "trusted HTTP admission time")?;
        record.cancellation = Some(control.cancellation_handle());
        record.control = Some(control);
        record.deadline = Some(life.deadline());
        record.life = Some(life.clone());
        record.head = Some(head.clone());
        if let Some(assets) = self
            .static_assets
            .as_ref()
            .filter(|assets| !assets.is_api_target(&head.target))
        {
            match assets.dispatch(&head, b"") {
                Ok(response) => {
                    record.static_head = Some(response.head);
                    record.static_body = Some(response.body);
                    record.phase = Phase::StaticRequestBody;
                    if self
                        .host
                        .admit_body(record.token, 0, life.deadline())
                        .is_err()
                    {
                        self.close_record(record);
                    } else {
                        self.report.admissions = self.report.admissions.saturating_add(1);
                    }
                }
                Err(error) => self.send_response(record, static_error(error)),
            }
            return Ok(());
        }
        let Some((backend, app)) = self.composition() else {
            self.send_response(
                record,
                error_response(503, responses::ErrorCode::ServiceUnavailable),
            );
            return Ok(());
        };
        let task =
            WebTask::admit(&backend, &app, head, life).map_err(|_| "bounded HTTP head task")?;
        record.phase = Phase::Admitting;
        record.queue(Purpose::Admit, HttpJob::Web(Box::new(task)))
    }
    fn admitted(&mut self, record: &mut Record<E::Ticket>, route: Route) -> Result<(), String> {
        if !self.live(record) {
            self.close_record(record);
            return Ok(());
        }
        record.route = Some(route);
        self.report.admissions = self.report.admissions.saturating_add(1);
        let kind = match route {
            Route::VoiceUpload { .. } => Some(UploadKind::Voice),
            Route::Materials {
                route: MaterialRoute::UploadBytes { .. },
            } => Some(UploadKind::Material),
            _ => None,
        };
        if let Some(kind) = kind {
            let Some(app) = self.app.upgrade() else {
                self.close_record(record);
                return Ok(());
            };
            let life = if kind == UploadKind::Voice {
                record
                    .life
                    .as_ref()
                    .unwrap()
                    .bounded(super::VOICE_BUDGET)
                    .map_err(|_| "voice duration budget")?
            } else {
                record.life.as_ref().unwrap().clone()
            };
            record.deadline = Some(life.deadline());
            record.life = Some(life.clone());
            let control = record.control.take().ok_or("original upload control")?;
            let task = UploadTask::begin(
                self.hub.clone(),
                &app,
                kind,
                record.head.as_ref().unwrap().clone(),
                control,
                life,
            )
            .map_err(|_| "bounded upload begin")?;
            record.phase = Phase::Beginning;
            record.queue(Purpose::Begin, HttpJob::Upload(Box::new(task)))
        } else {
            let count = record.head.as_ref().unwrap().content_length.unwrap_or(0);
            if count > MAX_JSON_BYTES as u64 || count > route.body_limit() {
                self.send_response(
                    record,
                    error_response(413, responses::ErrorCode::InvalidRequest),
                );
                return Ok(());
            }
            record.json = Vec::with_capacity(count as usize);
            record.phase = Phase::JsonBody;
            if self
                .host
                .admit_body(record.token, count, record.deadline.unwrap())
                .is_err()
            {
                self.close_record(record);
            }
            Ok(())
        }
    }
    fn start_static_response(&mut self, record: &mut Record<E::Ticket>) -> Result<(), String> {
        let head = record.static_head.take().ok_or("static response head")?;
        let body = record
            .static_body
            .as_mut()
            .ok_or("static response source")?;
        if body.remaining_bytes() as u64 != head.content_length || head.validate().is_err() {
            self.close_record(record);
            return Ok(());
        }
        let mut cursor = DownloadCursor::new(head.content_length);
        let (first, eof) = match static_chunk(&mut cursor, body) {
            Ok(chunk) => chunk,
            Err(()) => {
                self.close_record(record);
                return Ok(());
            }
        };
        let fence = record
            .life
            .as_ref()
            .ok_or("original static lifetime")?
            .execution_fence();
        if self.host.set_response_fence(record.token, fence).is_err()
            || self.host.respond(record.token, head, first, eof).is_err()
        {
            self.close_record(record);
        } else {
            record.static_cursor = Some(cursor);
            record.phase = Phase::Sending;
        }
        Ok(())
    }
    fn static_ready(&mut self, record: &mut Record<E::Ticket>) -> Result<(), String> {
        if !self.live(record) || record.phase != Phase::Sending || record.work.is_some() {
            self.close_record(record);
            return Ok(());
        }
        let cursor = record
            .static_cursor
            .as_mut()
            .ok_or("static physical send cursor")?;
        if cursor.acknowledge(false).is_err() {
            self.close_record(record);
            return Ok(());
        }
        let body = record
            .static_body
            .as_mut()
            .ok_or("static response source")?;
        let (bytes, eof) = match static_chunk(cursor, body) {
            Ok(chunk) => chunk,
            Err(()) => {
                self.close_record(record);
                return Ok(());
            }
        };
        if self
            .host
            .send_response_body(record.token, bytes, eof)
            .is_err()
        {
            self.close_record(record);
        }
        Ok(())
    }
    fn body(
        &mut self,
        record: &mut Record<E::Ticket>,
        sequence: u64,
        offset: u64,
        bytes: Vec<u8>,
        eof: bool,
    ) -> Result<(), String> {
        if !self.live(record) {
            self.close_record(record);
            return Ok(());
        }
        match record.phase {
            Phase::StaticRequestBody => {
                if sequence != 1
                    || offset != 0
                    || !bytes.is_empty()
                    || !eof
                    || self.host.ack_body(record.token, sequence, 0).is_err()
                {
                    self.close_record(record);
                } else {
                    self.report.body_acks = self.report.body_acks.saturating_add(1);
                    self.start_static_response(record)?;
                }
            }
            Phase::JsonBody => {
                let end = match record.buffer_json(sequence, offset, &bytes, eof) {
                    Ok(end) => end,
                    Err(()) => {
                        self.close_record(record);
                        return Ok(());
                    }
                };
                if self.host.ack_body(record.token, sequence, end).is_err() {
                    self.close_record(record);
                    return Ok(());
                }
                self.report.body_acks = self.report.body_acks.saturating_add(1);
                if eof {
                    let Some((backend, app)) = self.composition() else {
                        self.close_record(record);
                        return Ok(());
                    };
                    let head = record.head.as_ref().unwrap().clone();
                    let body = std::mem::take(&mut record.json);
                    let life = record.life.as_ref().unwrap().clone();
                    record.phase = Phase::JsonWork;
                    if matches!(
                        record.route,
                        Some(Route::Materials {
                            route: MaterialRoute::Download { .. }
                        })
                    ) {
                        let task =
                            DownloadTask::begin(self.downloads.clone(), &app, &backend, head, life)
                                .map_err(|_| "bounded material download begin")?;
                        record.download_request = true;
                        record.phase = Phase::DownloadBegin;
                        record.queue(Purpose::DownloadBegin, HttpJob::Download(Box::new(task)))?;
                    } else if matches!(
                        record.route,
                        Some(Route::Materials {
                            route: MaterialRoute::Append { .. }
                        })
                    ) {
                        let task = UploadTask::append_request(
                            self.hub.clone(),
                            &app,
                            &backend,
                            head,
                            body,
                            life,
                        )
                        .map_err(|_| "bounded material append")?;
                        record.queue(Purpose::Append, HttpJob::Upload(Box::new(task)))?;
                    } else {
                        let task = WebTask::json(&backend, &app, head, body, life)
                            .map_err(|_| "bounded JSON dispatch")?;
                        record.queue(Purpose::Json, HttpJob::Web(Box::new(task)))?;
                    }
                }
            }
            Phase::UploadBody => {
                let cursor = record.cursor.as_mut().ok_or("admitted upload cursor")?;
                let position = match cursor.plan_chunk(bytes.len()) {
                    Ok(position) => position,
                    Err(_) => {
                        self.close_record(record);
                        return Ok(());
                    }
                };
                if position.sequence != sequence || position.offset != offset {
                    self.close_record(record);
                    return Ok(());
                }
                let end = offset
                    .checked_add(bytes.len() as u64)
                    .ok_or("upload offset")?;
                let task = UploadTask::write(
                    self.hub.clone(),
                    record.key.unwrap(),
                    sequence,
                    offset,
                    bytes,
                    record.deadline.unwrap(),
                )
                .map_err(|_| "bounded upload write")?;
                record.phase = Phase::Writing;
                record.queue(
                    Purpose::Write { sequence, end, eof },
                    HttpJob::Upload(Box::new(task)),
                )?;
            }
            _ => self.close_record(record),
        }
        Ok(())
    }
    fn completion(
        &mut self,
        record: &mut Record<E::Ticket>,
        purpose: Purpose,
        result: HttpCompletion,
    ) -> Result<(), String> {
        if purpose == Purpose::Cleanup {
            match result {
                HttpCompletion::Upload(completion)
                    if !record.download_request && completion.token == record.token =>
                {
                    if matches!(completion.result,Ok(UploadOperationResult::CleanedRequest(token)) if token==record.token)
                    {
                        record.cleanup_done = true;
                        record.cleanup_failed = false;
                    } else {
                        record.cleanup_failed = true;
                    }
                }
                HttpCompletion::Download(completion)
                    if record.download_request && completion.token == record.token =>
                {
                    if matches!(completion.result, Ok(DownloadOperationResult::CleanedRequest(token)) if token == record.token)
                    {
                        record.cleanup_done = true;
                        record.cleanup_failed = false;
                    } else {
                        record.cleanup_failed = true;
                    }
                }
                _ => record.cleanup_failed = true,
            }
            return Ok(());
        }
        if record.closed {
            return Ok(());
        }
        match (purpose, result) {
            (Purpose::Admit, HttpCompletion::Web(WebCompletion::Admitted { token, route }))
                if token == record.token =>
            {
                self.admitted(record, route)?
            }
            (
                Purpose::Admit | Purpose::Json,
                HttpCompletion::Web(WebCompletion::Response { token, response }),
            ) if token == record.token => self.send_response(record, response),
            (purpose, HttpCompletion::Download(completion))
                if record.download_request && completion.token == record.token =>
            {
                match completion.result {
                    Err(DownloadHubFailure::Response(response))
                        if purpose == Purpose::DownloadBegin =>
                    {
                        self.send_response(record, response);
                    }
                    Err(DownloadHubFailure::Hub(error)) if purpose == Purpose::DownloadBegin => {
                        let (status, code) = match error {
                            DownloadHubError::Busy => (429, responses::ErrorCode::Capacity),
                            DownloadHubError::Invalid => {
                                (400, responses::ErrorCode::InvalidRequest)
                            }
                            DownloadHubError::Stale => (409, responses::ErrorCode::Expired),
                            DownloadHubError::Cancelled => {
                                self.close_record(record);
                                return Ok(());
                            }
                            _ => (503, responses::ErrorCode::ServiceUnavailable),
                        };
                        self.send_response(record, error_response(status, code));
                    }
                    Err(_) => self.close_record(record),
                    Ok(DownloadOperationResult::Admitted(admission))
                        if purpose == Purpose::DownloadBegin =>
                    {
                        if !self.live(record)
                            || record.phase != Phase::DownloadBegin
                            || admission.key.token() != record.token
                            || admission.head.status != 200
                            || admission.head.validate().is_err()
                            || admission.head.content_length > fc_ports::http::MAX_TRANSFER_BYTES
                        {
                            self.close_record(record);
                            return Ok(());
                        }
                        record.download_key = Some(admission.key);
                        record.download_cursor =
                            Some(DownloadCursor::new(admission.head.content_length));
                        if self
                            .host
                            .set_response_fence(record.token, admission.send_fence)
                            .is_err()
                            || self
                                .host
                                .respond(record.token, admission.head, b"", false)
                                .is_err()
                        {
                            self.close_record(record);
                        } else {
                            record.phase = Phase::Sending;
                        }
                    }
                    Ok(DownloadOperationResult::Read(chunk)) => {
                        let Purpose::DownloadRead { sequence, offset } = purpose else {
                            self.close_record(record);
                            return Ok(());
                        };
                        if !self.live(record)
                            || record.phase != Phase::DownloadRead
                            || record.download_key != Some(chunk.key)
                            || chunk.key.token() != record.token
                            || chunk.sequence != sequence
                            || chunk.offset != offset
                            || chunk.bytes.capacity() > CHUNK_BYTES
                            || record.download_cursor.as_mut().is_none_or(|cursor| {
                                cursor
                                    .accept(sequence, offset, chunk.bytes.len(), chunk.eof)
                                    .is_err()
                            })
                        {
                            self.close_record(record);
                            return Ok(());
                        }
                        if self
                            .host
                            .send_response_body(record.token, &chunk.bytes, chunk.eof)
                            .is_err()
                        {
                            self.close_record(record);
                        } else {
                            record.phase = Phase::Sending;
                        }
                    }
                    _ => self.close_record(record),
                }
            }
            (purpose, HttpCompletion::Upload(completion))
                if !record.download_request && completion.token == record.token =>
            {
                match completion.result {
                    Err(UploadHubFailure::Response(response)) => {
                        self.send_response(record, response)
                    }
                    Err(UploadHubFailure::Hub(error)) => {
                        let (status, code) = match error {
                            UploadHubError::Busy => (429, responses::ErrorCode::Capacity),
                            UploadHubError::Invalid => (400, responses::ErrorCode::InvalidRequest),
                            UploadHubError::Stale => (409, responses::ErrorCode::UploadExpired),
                            UploadHubError::Cancelled => {
                                self.close_record(record);
                                return Ok(());
                            }
                            _ => (503, responses::ErrorCode::ServiceUnavailable),
                        };
                        self.send_response(record, error_response(status, code));
                    }
                    Ok(UploadOperationResult::Admitted(admission)) if purpose == Purpose::Begin => {
                        if !self.live(record) || admission.key.token() != record.token {
                            self.close_record(record);
                            return Ok(());
                        }
                        record.deadline = Some(admission.deadline);
                        record.key = Some(admission.key);
                        record.cancellation = Some(admission.cancellation);
                        record.cursor = Some(
                            UploadCursor::new(
                                record.token,
                                admission.key.kind(),
                                admission.key.upload_id(),
                                admission.expected_bytes,
                            )
                            .map_err(|_| "upload admission cursor")?,
                        );
                        record.phase = Phase::UploadBody;
                        if self
                            .host
                            .admit_body(record.token, admission.expected_bytes, admission.deadline)
                            .is_err()
                        {
                            self.close_record(record);
                        }
                    }
                    Ok(UploadOperationResult::Written(ack)) => {
                        let Purpose::Write { sequence, end, eof } = purpose else {
                            self.close_record(record);
                            return Ok(());
                        };
                        if !self.live(record)
                            || ack.sequence() != sequence
                            || ack.next_offset() != end
                        {
                            self.close_record(record);
                            return Ok(());
                        }
                        let cursor = record.cursor.as_mut().ok_or("upload ACK cursor")?;
                        if cursor.acknowledge(ack).is_err() {
                            self.close_record(record);
                            return Ok(());
                        }
                        if self.host.ack_body(record.token, sequence, end).is_err() {
                            self.close_record(record);
                            return Ok(());
                        }
                        self.report.body_acks = self.report.body_acks.saturating_add(1);
                        if eof {
                            if cursor.finish().is_err() {
                                self.close_record(record);
                                return Ok(());
                            }
                            let Some(app) = self.app.upgrade() else {
                                self.close_record(record);
                                return Ok(());
                            };
                            let task = UploadTask::finish(
                                self.hub.clone(),
                                &app,
                                record.key.unwrap(),
                                record.deadline.unwrap(),
                            );
                            record.phase = Phase::Finishing;
                            record.queue(Purpose::Finish, HttpJob::Upload(Box::new(task)))?;
                        } else {
                            record.phase = Phase::UploadBody;
                        }
                    }
                    Ok(UploadOperationResult::Finished(finished)) if purpose == Purpose::Finish => {
                        if let Some(control) = finished.network_control {
                            if record.control.is_some()
                                || !Arc::ptr_eq(
                                    &control.cancelled,
                                    &record
                                        .life
                                        .as_ref()
                                        .ok_or("original upload lifetime")?
                                        .cancelled,
                                )
                            {
                                self.close_record(record);
                                return Ok(());
                            }
                            record.control = Some(control);
                            record.cleanup_done = true;
                        } else if record
                            .key
                            .is_some_and(|key| key.kind() == UploadKind::Material)
                            && finished.response.head.status == 200
                        {
                            record.material_ready = true;
                        }
                        self.send_response(record, finished.response);
                    }
                    Ok(UploadOperationResult::Appended(response)) if purpose == Purpose::Append => {
                        self.send_response(record, response)
                    }
                    _ => self.close_record(record),
                }
            }
            _ => self.close_record(record),
        }
        Ok(())
    }
    fn tick_work(&mut self, record: &mut Record<E::Ticket>) -> Result<(), String> {
        let Some(work) = record.work.take() else {
            return Ok(());
        };
        match work.state {
            JobState::Pending(job) => {
                if Instant::now() >= job.deadline() {
                    if work.purpose == Purpose::Cleanup {
                        record.cleanup_failed = true;
                    } else {
                        self.close_record(record);
                    }
                    return Ok(());
                }
                match self.executor.try_submit_http(job) {
                    Ok(ticket) => {
                        if matches!(
                            work.purpose,
                            Purpose::Begin | Purpose::Append | Purpose::DownloadBegin
                        ) {
                            record.hub_touched = true;
                            record.cleanup_done = false;
                        }
                        record.work = Some(Work {
                            purpose: work.purpose,
                            state: JobState::Running(ticket),
                        });
                    }
                    Err(rejected) if rejected.kind == Rejection::Busy => {
                        record.work = Some(Work {
                            purpose: work.purpose,
                            state: JobState::Pending(rejected.task),
                        })
                    }
                    Err(_) => {
                        if work.purpose == Purpose::Cleanup {
                            record.cleanup_failed = true;
                        } else {
                            self.close_record(record);
                        }
                    }
                }
            }
            JobState::Running(mut ticket) => {
                if !ticket.is_finished() {
                    record.work = Some(Work {
                        purpose: work.purpose,
                        state: JobState::Running(ticket),
                    });
                    return Ok(());
                }
                match ticket.try_result() {
                    Ok(Some(result)) => self.completion(record, work.purpose, result)?,
                    Ok(None) => {
                        record.work = Some(Work {
                            purpose: work.purpose,
                            state: JobState::Running(ticket),
                        })
                    }
                    Err(_) => {
                        if work.purpose == Purpose::Cleanup {
                            record.cleanup_failed = true;
                        } else {
                            self.close_record(record);
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn maybe_cleanup(&mut self, record: &mut Record<E::Ticket>) -> Result<(), String> {
        if record.closed
            && record.work.is_none()
            && record.hub_touched
            && !record.cleanup_done
            && !record.cleanup_failed
        {
            let deadline = Instant::now() + Duration::from_secs(5);
            let job = if record.download_request {
                HttpJob::Download(Box::new(DownloadTask::cleanup_request(
                    self.downloads.clone(),
                    record.token,
                    deadline,
                )))
            } else {
                let Some(app) = self.app.upgrade() else {
                    record.cleanup_failed = true;
                    return Ok(());
                };
                HttpJob::Upload(Box::new(UploadTask::cleanup_request(
                    self.hub.clone(),
                    &app,
                    record.token,
                    deadline,
                )))
            };
            record.queue(Purpose::Cleanup, job)?;
        }
        Ok(())
    }
    fn download_ready(&mut self, record: &mut Record<E::Ticket>) -> Result<(), String> {
        if !self.live(record) || record.phase != Phase::Sending || record.work.is_some() {
            self.close_record(record);
            return Ok(());
        }
        let Some(key) = record.download_key else {
            self.close_record(record);
            return Ok(());
        };
        let cursor = record
            .download_cursor
            .as_mut()
            .ok_or("download send cursor")?;
        if cursor.pending.is_some() && cursor.acknowledge(false).is_err() {
            self.close_record(record);
            return Ok(());
        }
        let (sequence, offset) = cursor
            .position()
            .map_err(|_| "download physical send cursor")?;
        let task = DownloadTask::read(
            self.downloads.clone(),
            key,
            sequence,
            offset,
            record.deadline.unwrap(),
        )
        .map_err(|_| "bounded download read")?;
        record.phase = Phase::DownloadRead;
        record.queue(
            Purpose::DownloadRead { sequence, offset },
            HttpJob::Download(Box::new(task)),
        )
    }
    fn event(&mut self, event: TlsHttpEvent) -> Result<(), String> {
        let token = event_token(&event);
        let Some(index) = find_record(&self.records, token) else {
            self.report.stale_events = self.report.stale_events.saturating_add(1);
            return Ok(());
        };
        let mut record = self.records[index].take().unwrap();
        let result = match event {
            TlsHttpEvent::Head { head, admitted_at } => self.head(&mut record, head, admitted_at),
            TlsHttpEvent::Body {
                sequence,
                offset,
                bytes,
                eof,
                ..
            } => self.body(&mut record, sequence, offset, bytes, eof),
            TlsHttpEvent::ResponseComplete(_) => {
                if !record.closed && record.phase == Phase::Sending {
                    if record.static_cursor.is_some() {
                        if record
                            .static_cursor
                            .as_mut()
                            .is_none_or(|cursor| cursor.acknowledge(true).is_err())
                        {
                            self.close_record(&mut record);
                            self.records[index] = Some(record);
                            return Ok(());
                        }
                        record.cancel();
                    } else if record.download_key.is_some() {
                        if record
                            .download_cursor
                            .as_mut()
                            .is_none_or(|cursor| cursor.acknowledge(true).is_err())
                        {
                            self.close_record(&mut record);
                            self.records[index] = Some(record);
                            return Ok(());
                        }
                        // Actual complete, not a read reply, ends the original
                        // transfer. Keep native close_notify independent and
                        // queue worker cleanup only after any actual job ends.
                        record.cancel();
                    }
                    record.response_complete = true;
                    self.report.responses = self.report.responses.saturating_add(1);
                    if record.material_ready {
                        record.cleanup_done = true;
                    }
                }
                Ok(())
            }
            TlsHttpEvent::ResponseReady(_) => {
                if record.static_cursor.is_some() {
                    self.static_ready(&mut record)
                } else {
                    self.download_ready(&mut record)
                }
            }
            TlsHttpEvent::Closed { .. } => {
                if !record.closed_seen {
                    record.closed_seen = true;
                    self.report.closed = self.report.closed.saturating_add(1);
                }
                record.cancel();
                Ok(())
            }
            TlsHttpEvent::Retired(_) => {
                if !record.native_retired {
                    record.native_retired = true;
                    self.report.retired = self.report.retired.saturating_add(1);
                }
                Ok(())
            }
        };
        self.records[index] = Some(record);
        result
    }
    fn retire_records(&mut self) -> Result<(), String> {
        for index in 0..MAX_REQUESTS {
            let Some(record) = self.records[index].as_ref() else {
                continue;
            };
            if record.may_release() {
                if self.report.stopping || record.native_rearmed {
                    self.records[index] = None;
                    continue;
                }
                let token = record.token;
                let next = self.host.rearm(token)?;
                self.records[index] = Some(Record::new(next));
                self.report.rearms = self.report.rearms.saturating_add(1);
            } else if record.native_retired && !record.native_rearmed && !self.report.stopping {
                let Some(vacant) = self.records.iter().position(Option::is_none) else {
                    continue;
                };
                let token = record.token;
                // Reserve the independent descriptor BEFORE reusing a TLS slot.
                // &mut Net ownership prevents another event from seeing this
                // temporary placeholder while the synchronous command executes.
                self.records[vacant] = Some(Record::new(token));
                match self.host.rearm(token) {
                    Ok(next) => {
                        self.records[vacant] = Some(Record::new(next));
                        self.records[index].as_mut().unwrap().native_rearmed = true;
                        self.report.rearms = self.report.rearms.saturating_add(1);
                        self.report.peak_records = self
                            .report
                            .peak_records
                            .max(self.records.iter().flatten().count());
                    }
                    Err(error) => {
                        self.records[vacant] = None;
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
    fn refresh_report(&mut self) {
        self.report.acceptance = self.host.acceptance_snapshot();
        self.report.json_body_intervals = [None; MAX_REQUESTS];
        self.report.json_body_records = 0;
        for record in self
            .records
            .iter()
            .flatten()
            .filter(|r| r.phase == Phase::JsonBody)
        {
            if let (Some(life), Some(deadline)) = (&record.life, record.deadline) {
                self.report.json_body_intervals[self.report.json_body_records] =
                    Some(RequestInterval {
                        token: record.token,
                        admitted_at: life.admitted_at(),
                        deadline,
                    });
                self.report.json_body_records += 1;
            }
        }
        self.report.active_records = self.records.iter().flatten().count();
        self.report.peak_records = self.report.peak_records.max(self.report.active_records);
        self.report.pending_jobs = self
            .records
            .iter()
            .flatten()
            .filter(|record| record.work.is_some())
            .count();
        self.report.cleanup_pending = self
            .records
            .iter()
            .flatten()
            .filter(|record| record.hub_touched && !record.cleanup_done)
            .count();
        self.report.retained_cleanup = self
            .records
            .iter()
            .flatten()
            .filter(|record| record.cleanup_failed)
            .count();
        self.report.owned_bytes = self
            .static_assets
            .as_ref()
            .map_or(0, |assets| assets.allocation_bytes())
            + self.host.app_owned_bytes()
            + size_of::<Self>()
            - size_of::<TlsHttpHost>()
            + size_of::<Option<Box<Self>>>()
            + self
                .records
                .iter()
                .flatten()
                .map(Record::owned_bytes)
                .sum::<usize>();
    }
    fn poll(&mut self) -> Result<NetworkReport, String> {
        self.started = true;
        let events = self.host.poll()?;
        if events.len() > fc_ports::http::MAX_POLL_EVENTS {
            return Err("bounded TLS event budget".into());
        }
        for event in events {
            self.event(event)?;
        }
        for index in 0..MAX_REQUESTS {
            let Some(mut record) = self.records[index].take() else {
                continue;
            };
            let result = (|| {
                if !record.closed
                    && record
                        .deadline
                        .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    self.close_record(&mut record);
                }
                // At most one task submission OR completion per record/poll.
                self.tick_work(&mut record)?;
                self.maybe_cleanup(&mut record)
            })();
            self.records[index] = Some(record);
            result?;
        }
        if self.report.stopping {
            self.report.native_drained = self.host.poll_shutdown()?;
        }
        self.retire_records()?;
        self.refresh_report();
        Ok(self.report)
    }
    fn request_stop(&mut self) -> Result<(), String> {
        if self.report.stopping {
            return Ok(());
        }
        self.report.stopping = true;
        let result = self.host.request_stop();
        for record in self.records.iter_mut().flatten() {
            record.cancel();
        }
        self.refresh_report();
        result
    }
}
