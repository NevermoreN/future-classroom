//! Standalone repair capability. Created only by a process holding InstanceGuard
//! and NeverOpenedProof; no Backend, credential store or teacher identity opens.
use crate::{
    reinitialization::NeverOpenedProof,
    reinitialization_files::{PreparedReinitialization, ReinitializationFiles, ResetReview},
    web_application::WebResponse,
    web_transfer::{TransferLifetime, WebCompletion},
};
use fc_ports::{
    http::{Header, ListenPrefix, RequestHead, ResponseHead},
    RandomPort,
};
use fc_protocol::web::{
    maintenance::ResetScope,
    repair as wire,
    repair::{Phase, Resource},
    Route, WebBoundary, WebError,
};
use fc_runtime::worker::JobContext;
use fc_security::web::{SecretText, SecretVerifier, WebSecret};
use fc_windows::system::DataPaths;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
type Result<T> = std::result::Result<T, &'static str>;
struct Session {
    token: SecretVerifier,
    csrf: SecretVerifier,
}
struct Bootstrap {
    verifier: SecretVerifier,
    issued: Option<(WebSecret, WebSecret)>,
}
struct State {
    bootstrap: Option<Bootstrap>,
    session: Option<Session>,
    status: wire::Status,
    review: Option<ResetReview>,
    prepared: Option<PreparedReinitialization>,
    work: Option<RepairWorkKind>,
    busy: bool,
    closed: bool,
}
pub struct RepairService {
    paths: DataPaths,
    boundary: WebBoundary,
    state: Mutex<State>,
    cancel: AtomicBool,
    exit: AtomicBool,
}
#[derive(Clone, Copy)]
enum RepairWorkKind {
    Reconcile,
    Review,
    Prepare,
    Retire,
}
pub struct RepairWork {
    service: Arc<RepairService>,
    kind: RepairWorkKind,
}
impl RepairService {
    pub fn new(
        paths: DataPaths,
        proof: &NeverOpenedProof,
        prefix: &ListenPrefix,
        random: &mut impl RandomPort,
    ) -> Result<(Arc<Self>, SecretText)> {
        if paths.root() != proof.root() || !prefix.as_str().starts_with("https://127.0.0.1:") {
            return Err("修复入口与当前部署不匹配。");
        }
        let secret = WebSecret::generate(random).map_err(|_| "无法创建修复入口授权。")?;
        let service = Arc::new(Self {
            paths,
            boundary: WebBoundary::new(prefix).map_err(|_| "修复本机地址无效。")?,
            cancel: AtomicBool::new(false),
            exit: AtomicBool::new(false),
            state: Mutex::new(State {
                bootstrap: Some(Bootstrap {
                    verifier: secret.verifier(),
                    issued: None,
                }),
                session: None,
                status: wire::Status {
                    sequence: proof.sequence().to_string(),
                    phase: Phase::Initializing,
                    scope: None,
                    can_cancel: true,
                    cancellation_requested: false,
                    error: None,
                    succeeded: None,
                },
                review: None,
                prepared: None,
                work: Some(RepairWorkKind::Reconcile),
                busy: false,
                closed: false,
            }),
        });
        Ok((service, secret.expose_hex()))
    }
    /// Only the native tray/OS composition calls this; no Web route can mint a
    /// bootstrap capability. A replacement is consumed once on browser exchange.
    pub fn browser_launch_capability(&self, random: &mut impl RandomPort) -> Result<SecretText> {
        let token = WebSecret::generate(random).map_err(|_| "无法创建本机修复入口授权。")?;
        let session = WebSecret::generate(random).map_err(|_| "无法准备修复页面会话。")?;
        let csrf = WebSecret::generate(random).map_err(|_| "无法准备修复页面会话。")?;
        let mut state = self.state.lock().map_err(|_| "修复入口不可用。")?;
        if state.closed {
            return Err("修复程序正在退出。");
        }
        state.bootstrap = Some(Bootstrap {
            verifier: token.verifier(),
            issued: Some((session, csrf)),
        });
        Ok(token.expose_hex())
    }
    pub fn preflight(&self, head: &RequestHead) -> std::result::Result<Resource, WebError> {
        let Route::Repair { resource } = self.boundary.preflight(head)? else {
            return Err(WebError::NotFound);
        };
        if head.content_length.unwrap_or(0) > wire::MAX_BODY as u64 {
            return Err(WebError::Body);
        }
        Ok(resource)
    }
    pub fn detached_response(
        self: &Arc<Self>,
        head: &RequestHead,
        body: &[u8],
    ) -> Option<WebResponse> {
        let phase = self.status().phase;
        if !matches!(
            phase,
            Phase::Draining | Phase::Committing | Phase::Retained | Phase::Finished
        ) {
            return None;
        }
        let resource = match self.preflight(head) {
            Ok(v) => v,
            Err(_) => return Some(conflict()),
        };
        if body.len() > wire::MAX_BODY || body.len() as u64 != head.content_length.unwrap_or(0) {
            return Some(conflict());
        }
        if resource == Resource::Session {
            return Some(self.exchange(body, false));
        }
        if !matches!(
            resource,
            Resource::Status | Resource::Cancel | Resource::Exit
        ) {
            return Some(denied());
        }
        if body.len() > wire::MAX_BODY
            || body.len() as u64 != head.content_length.unwrap_or(0)
            || (resource == Resource::Status && !body.is_empty())
            || (resource != Resource::Status
                && fc_protocol::web::maintenance::empty_action(body).is_err())
        {
            return Some(conflict());
        }
        if self.authorize(head, resource != Resource::Status).is_err() {
            return Some(denied());
        }
        match resource {
            Resource::Cancel => {
                if !self.status().can_cancel {
                    return Some(conflict());
                }
                self.cancel.store(true, Ordering::Release);
            }
            Resource::Exit => {
                self.exit.store(true, Ordering::Release);
                self.cancel.store(true, Ordering::Release);
            }
            _ => {}
        }
        Some(response(200, wire::status(&self.status())))
    }
    pub fn take_work(self: &Arc<Self>) -> Option<RepairWork> {
        let mut s = self.state.try_lock().ok()?;
        if s.busy {
            return None;
        }
        let kind = s.work.take()?;
        s.busy = true;
        Some(RepairWork {
            service: self.clone(),
            kind,
        })
    }
    pub fn return_work(&self, work: RepairWork) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.busy = false;
        if s.work.is_none() {
            s.work = Some(work.kind);
        }
    }
    pub fn has_confirmed(&self) -> bool {
        self.state
            .try_lock()
            .is_ok_and(|s| !s.closed && s.status.phase == Phase::Draining && s.prepared.is_some())
    }
    pub fn take_confirmed(&self) -> Option<PreparedReinitialization> {
        let mut s = self.state.try_lock().ok()?;
        if s.closed || s.status.phase != Phase::Draining {
            return None;
        }
        s.prepared.take()
    }
    pub fn cancellation_requested(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
    pub fn exit_requested(&self) -> bool {
        self.exit.load(Ordering::Acquire)
    }
    pub fn request_close(&self) {
        self.exit.store(true, Ordering::Release);
        self.cancel.store(true, Ordering::Release);
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.closed = true;
        if !s.busy && (s.prepared.is_some() || s.review.is_some() || s.work.is_some()) {
            s.work = Some(RepairWorkKind::Retire);
        }
    }
    pub fn retirement_complete(&self) -> bool {
        self.state.try_lock().is_ok_and(|s| {
            !s.busy && s.work.is_none() && s.prepared.is_none() && s.review.is_none()
        })
    }
    pub fn update(&self, phase: Phase, error: Option<String>, succeeded: Option<bool>) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.status.phase = phase;
        s.status.can_cancel = !matches!(
            phase,
            Phase::Committing
                | Phase::Retained
                | Phase::Finished
                | Phase::Cancelled
                | Phase::Failed
        );
        s.status.error = error;
        s.status.succeeded = succeeded;
        s.status.cancellation_requested = self.cancel.load(Ordering::Acquire);
        if matches!(
            phase,
            Phase::Draining | Phase::Committing | Phase::Retained | Phase::Finished
        ) {
            s.status.scope = None;
        }
    }
    fn status(&self) -> wire::Status {
        let mut status = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone();
        status.cancellation_requested = self.cancel.load(Ordering::Acquire);
        status
    }
    fn authorize(&self, head: &RequestHead, post: bool) -> Result<()> {
        let token = header_secret(head, wire::SESSION_HEADER)?;
        let csrf = if post {
            Some(header_secret(head, "x-fc-csrf")?)
        } else {
            None
        };
        let s = self.state.lock().map_err(|_| "修复会话不可用。")?;
        let session = s
            .session
            .as_ref()
            .ok_or("需要通过本机修复启动入口打开此页。")?;
        if !session.token.verify(&token) || csrf.as_ref().is_some_and(|v| !session.csrf.verify(v)) {
            return Err("修复会话无效。");
        }
        Ok(())
    }
    fn exchange(&self, body: &[u8], worker_generation_allowed: bool) -> WebResponse {
        let exchange = match wire::Exchange::decode(body) {
            Ok(v) => v,
            Err(_) => return denied(),
        };
        let capability = match WebSecret::parse_hex(exchange.capability) {
            Ok(v) => v,
            Err(_) => return denied(),
        };
        let mut state = match self.state.lock() {
            Ok(v) => v,
            Err(_) => return denied(),
        };
        if state.closed
            || state
                .bootstrap
                .as_ref()
                .is_none_or(|v| !v.verifier.verify(&capability))
        {
            return denied();
        }
        // Cold Net only consumes the bounded reply prepared by a real OS tray
        // action. It does not call CSPRNG, open storage or enqueue a worker.
        if !worker_generation_allowed && state.bootstrap.as_ref().is_none_or(|v| v.issued.is_none())
        {
            return denied();
        }
        let issued = state.bootstrap.as_mut().and_then(|v| v.issued.take());
        let (token, csrf) = match issued {
            Some(v) => v,
            None => {
                let token = match WebSecret::generate(&mut fc_windows::SystemRandom) {
                    Ok(v) => v,
                    Err(_) => return unavailable(),
                };
                let csrf = match WebSecret::generate(&mut fc_windows::SystemRandom) {
                    Ok(v) => v,
                    Err(_) => return unavailable(),
                };
                (token, csrf)
            }
        };
        state.bootstrap = None;
        state.session = Some(Session {
            token: token.verifier(),
            csrf: csrf.verifier(),
        });
        response(
            200,
            wire::session(token.expose_hex().as_str(), csrf.expose_hex().as_str()),
        )
    }
    fn dispatch(
        self: &Arc<Self>,
        head: &RequestHead,
        body: &[u8],
        context: &JobContext,
    ) -> WebResponse {
        let resource = match self.preflight(head) {
            Ok(v) => v,
            Err(_) => return conflict(),
        };
        if body.len() > wire::MAX_BODY
            || body.len() as u64 != head.content_length.unwrap_or(0)
            || context.checkpoint().is_err()
        {
            return response(400, Err(fc_protocol::ProtocolError::InvalidCommand));
        }
        if resource == Resource::Session {
            return self.exchange(body, true);
        }
        if self.authorize(head, resource != Resource::Status).is_err() {
            return denied();
        }
        if resource == Resource::Status {
            return response(200, wire::status(&self.status()));
        }
        if resource != Resource::Confirm
            && fc_protocol::web::maintenance::empty_action(body).is_err()
        {
            return response(400, Err(fc_protocol::ProtocolError::InvalidCommand));
        }
        match resource {
            Resource::Review => {
                let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if s.closed || s.busy || s.work.is_some() || s.status.phase != Phase::Ready {
                    return conflict();
                }
                s.work = Some(RepairWorkKind::Review);
                s.status.phase = Phase::Preparing;
            }
            Resource::Confirm => {
                let confirmation = match wire::Confirm::decode(body) {
                    Ok(v) => v,
                    Err(_) => return conflict(),
                };
                let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if s.closed
                    || s.busy
                    || s.work.is_some()
                    || s.review.is_none()
                    || s.status.phase != Phase::ReviewReady
                    || confirmation.sequence != s.status.sequence
                {
                    return conflict();
                }
                s.work = Some(RepairWorkKind::Prepare);
                s.status.phase = Phase::Preparing;
            }
            Resource::Cancel => {
                if !self.status().can_cancel {
                    return conflict();
                }
                self.cancel.store(true, Ordering::Release);
                let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if !s.busy && !matches!(s.status.phase, Phase::Draining) {
                    s.work = Some(RepairWorkKind::Retire);
                }
            }
            Resource::Exit => self.request_close(),
            _ => return conflict(),
        }
        response(200, wire::status(&self.status()))
    }
}
impl RepairWork {
    pub fn allocation_bytes(&self) -> Option<usize> {
        Some(4096 + self.service.paths.root().as_os_str().len() * 2)
    }
    pub fn run(self, context: &JobContext) -> Result<()> {
        let service = &self.service;
        let check = || context.checkpoint().is_ok() && !service.cancel.load(Ordering::Acquire);
        let files =
            ReinitializationFiles::new(&service.paths).map_err(|_| "无法打开受控修复目录。");
        let result = (|| {
            let files = files?;
            match self.kind {
                RepairWorkKind::Reconcile => files
                    .reconcile_startup(&check)
                    .map_err(|_| "上次修复状态需要保留人工核对。")
                    .map(|_| {
                        service.update(Phase::Ready, None, None);
                    }),
                RepairWorkKind::Review => {
                    let sequence = service
                        .status()
                        .sequence
                        .parse()
                        .map_err(|_| "修复标识无效。")?;
                    let review = files
                        .review(sequence, &check)
                        .map_err(|_| "无法复核本机清除范围。");
                    review.map(|review| {
                        let scope = ResetScope {
                            present_tree_count: review.present_tree_count(),
                            scope: vec![
                                "账号与凭据",
                                "学校、班级、学科与任教关系",
                                "课程表、布局与设置",
                                "资料、缩略图、索引与录音",
                                "提醒与命令历史",
                            ],
                        };
                        let mut s = service.state.lock().unwrap_or_else(|e| e.into_inner());
                        s.review = Some(review);
                        s.status.scope = Some(scope);
                        s.status.phase = Phase::ReviewReady;
                    })
                }
                RepairWorkKind::Prepare => {
                    let review = service
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .review
                        .take()
                        .ok_or("请重新复核清除范围。");
                    let review = review?;
                    let prepared = match files.prepare(review, &check) {
                        Ok(v) => v,
                        Err(_) => {
                            files
                                .reconcile_startup(&|| true)
                                .map_err(|_| "取消后的修复记录已保留，需要重新核对。")?;
                            return Err("修复准备未完成，原资料未执行替换。");
                        }
                    };
                    let mut s = service.state.lock().unwrap_or_else(|e| e.into_inner());
                    if s.closed || !check() {
                        drop(s);
                        prepared
                            .discard(&|| true)
                            .map_err(|_| "修复暂存未能清理，已保留记录。")?;
                        return Err("修复已取消。");
                    }
                    s.prepared = Some(prepared);
                    s.status.phase = Phase::Draining;
                    s.status.scope = None;
                    Ok(())
                }
                RepairWorkKind::Retire => {
                    let prepared = {
                        let mut s = service.state.lock().unwrap_or_else(|e| e.into_inner());
                        s.review = None;
                        s.prepared.take()
                    };
                    if let Some(prepared) = prepared {
                        prepared
                            .discard(&|| true)
                            .map_err(|_| "修复暂存未能清理，已保留记录。")?;
                    }
                    service.update(Phase::Cancelled, None, None);
                    Ok(())
                }
            }
        })();
        let mut s = service.state.lock().unwrap_or_else(|e| e.into_inner());
        s.busy = false;
        if let Err(error) = result {
            s.status.phase = if service.cancel.load(Ordering::Acquire) {
                Phase::Cancelled
            } else {
                Phase::Failed
            };
            s.status.can_cancel = false;
            s.status.error = Some(error.into());
        }
        if s.closed || service.cancel.load(Ordering::Acquire) {
            if s.prepared.is_some() || s.review.is_some() {
                s.work = Some(RepairWorkKind::Retire);
            } else {
                s.work = None;
            }
        }
        result
    }
}
pub struct RepairHttpTask {
    service: Arc<RepairService>,
    head: RequestHead,
    body: Vec<u8>,
    life: TransferLifetime,
}
impl RepairHttpTask {
    pub fn new(
        service: Arc<RepairService>,
        head: RequestHead,
        body: Vec<u8>,
        life: TransferLifetime,
    ) -> std::result::Result<Self, crate::web_transfer::TransferError> {
        if head.token != life.token() || body.len() > wire::MAX_BODY {
            return Err(crate::web_transfer::TransferError::InvalidAdmission);
        }
        Ok(Self {
            service,
            head,
            body,
            life,
        })
    }
    pub fn deadline(&self) -> Instant {
        self.life.deadline()
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        Some(
            std::mem::size_of::<Self>()
                + self.body.capacity()
                + self
                    .head
                    .headers
                    .iter()
                    .map(|h| h.name.capacity() + h.value.capacity())
                    .sum::<usize>()
                + self.head.method.capacity()
                + self.head.target.capacity()
                + 4096,
        )
    }
    pub fn run(mut self, context: &JobContext) -> WebCompletion {
        let token = self.head.token;
        let response = if self.life.is_active() && context.checkpoint().is_ok() {
            self.service.dispatch(&self.head, &self.body, context)
        } else {
            unavailable()
        };
        self.body.fill(0);
        WebCompletion::Response { token, response }
    }
}
fn header_secret(head: &RequestHead, name: &str) -> Result<WebSecret> {
    let mut values = head
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case(name));
    let value = match (values.next(), values.next()) {
        (Some(v), None) => v,
        _ => return Err("缺少修复会话。"),
    };
    std::str::from_utf8(&value.value)
        .ok()
        .and_then(|v| WebSecret::parse_hex(v.to_owned()).ok())
        .ok_or("修复会话无效。")
}
fn response(
    status: u16,
    body: std::result::Result<Vec<u8>, fc_protocol::ProtocolError>,
) -> WebResponse {
    let (status, body) = match body {
        Ok(body) => (status, body),
        Err(_) => {
            use fc_protocol::web::responses::{error, ErrorCode};
            let (status, code) = match status {
                403 => (403, ErrorCode::Forbidden),
                404 => (404, ErrorCode::NotFound),
                409 => (409, ErrorCode::RevisionConflict),
                400 => (400, ErrorCode::InvalidRequest),
                _ => (503, ErrorCode::ServiceUnavailable),
            };
            (
                status,
                error(code).unwrap_or_else(|_| {
                    br#"{"schemaVersion":1,"code":"SERVICE_UNAVAILABLE"}"#.to_vec()
                }),
            )
        }
    };
    WebResponse {
        head: ResponseHead {
            status,
            headers: vec![
                Header {
                    name: "X-Content-Type-Options".into(),
                    value: b"nosniff".to_vec(),
                },
                Header {
                    name: "Content-Type".into(),
                    value: b"application/json; charset=utf-8".to_vec(),
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
fn denied() -> WebResponse {
    response(403, Err(fc_protocol::ProtocolError::InvalidCommand))
}
fn conflict() -> WebResponse {
    response(409, Err(fc_protocol::ProtocolError::InvalidCommand))
}
fn unavailable() -> WebResponse {
    response(503, Err(fc_protocol::ProtocolError::InvalidCommand))
}
