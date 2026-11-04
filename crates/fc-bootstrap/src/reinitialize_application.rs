//! Explicit OS repair uses a process-bound, loopback-only Web capability. The
//! teaching Backend and old credentials are never opened by this bootstrap.
// Historical isolated native diagnostics stay separate from the product entry.
#[path = "reinitialize_application/legacy_probe.rs"]
mod legacy_probe;
use fc_bootstrap::{
    https_runtime::RuntimePhase,
    local_https::{LocalRuntime, PreparedLocal},
    reinitialization::{NeverOpenedProof, ReinitializationBackend},
    reinitialization_workflow::{
        PreparedReinitializationCoordinator, ReinitializationOperation, ReinitializationPhase,
    },
    repair_web::{RepairService, RepairWork},
    web_transfer::{HttpCompletion, HttpExecutor, HttpJob, HttpTicket},
};
use fc_ports::RandomPort;
use fc_runtime::worker::{
    JobContext, JobError, JobTicket, SubmitError, Task, WorkerHandle, WorkerPool,
};
use fc_windows::{
    host::{HiddenHost, HostHints},
    materials::files::PinnedDirectory,
    system::{thread_is_finished, DataPaths},
    InstanceGuard, SystemRandom,
};
pub(crate) use legacy_probe::run_at_with;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, String>;
enum Work {
    Prepare,
    Repair(RepairWork),
    Http(HttpJob),
}
enum Outcome {
    Prepared(PreparedLocal),
    Repair,
    Http(HttpCompletion),
}
impl Task for Work {
    type Output = Result<Outcome>;
    fn allocation_bytes(&self) -> Option<usize> {
        match self {
            Self::Prepare => Some(4096),
            Self::Repair(v) => v.allocation_bytes(),
            Self::Http(v) => v.allocation_bytes(),
        }
    }
    fn run(self, context: &JobContext) -> Self::Output {
        match self {
            Self::Prepare => PreparedLocal::prepare(context)
                .map(Outcome::Prepared)
                .map_err(|e| e.message().into()),
            Self::Repair(v) => v
                .run(context)
                .map(|_| Outcome::Repair)
                .map_err(str::to_owned),
            Self::Http(v) => Ok(Outcome::Http(v.run(context))),
        }
    }
}
#[derive(Clone)]
struct Executor(WorkerHandle<Work>);
struct Ticket(JobTicket<Result<Outcome>>);
impl HttpExecutor for Executor {
    type Ticket = Ticket;
    fn try_submit_http(&self, task: HttpJob) -> std::result::Result<Ticket, SubmitError<HttpJob>> {
        let deadline = task.deadline();
        match self.0.try_submit(Work::Http(task), deadline) {
            Ok(v) => Ok(Ticket(v)),
            Err(error) => {
                let Work::Http(task) = error.task else {
                    unreachable!()
                };
                Err(SubmitError {
                    kind: error.kind,
                    task,
                })
            }
        }
    }
}
impl HttpTicket for Ticket {
    fn cancel(&self) {
        self.0.cancel()
    }
    fn is_finished(&self) -> bool {
        self.0.is_finished()
    }
    fn try_result(&mut self) -> std::result::Result<Option<HttpCompletion>, JobError> {
        match self.0.try_result()? {
            None => Ok(None),
            Some(Ok(Outcome::Http(v))) => Ok(Some(v)),
            Some(_) => Err(JobError::Faulted),
        }
    }
}
struct App {
    paths: DataPaths,
    pool: Option<WorkerPool<Work>>,
    handle: WorkerHandle<Work>,
    prepare: Option<JobTicket<Result<Outcome>>>,
    pending: Option<JobTicket<Result<Outcome>>>,
    runtime: Option<LocalRuntime>,
    service: Option<Arc<RepairService>>,
    proof: Option<NeverOpenedProof>,
    operation: Option<ReinitializationOperation<Work>>,
    url: Option<String>,
    opened: bool,
    closing: bool,
    joined: bool,
    finished: bool,
    error: Option<String>,
}
impl App {
    fn reopen(&mut self) -> Result<()> {
        if self.closing {
            return Ok(());
        }
        if let (Some(service), Some(runtime)) = (&self.service, &self.runtime) {
            let cap = service
                .browser_launch_capability(&mut SystemRandom)
                .map_err(str::to_owned)?;
            let url = format!(
                "https://127.0.0.1:{}/fc/#/repair/{}",
                runtime.port(),
                cap.as_str()
            );
            fc_windows::startup::open_local_management(&url)
                .map_err(|_| "无法打开本机修复网页。")?;
        }
        Ok(())
    }
    fn close(&mut self) {
        self.closing = true;
        if let Some(service) = &self.service {
            service.request_close();
        }
        if let Some(operation) = &self.operation {
            operation.cancel();
        }
    }
    fn fail(&mut self, error: String) {
        self.error.get_or_insert(error);
        self.close();
    }
    fn poll(&mut self) -> Result<()> {
        if let Some(ticket) = self.prepare.as_mut() {
            match ticket.try_result() {
                Ok(None) => {}
                Ok(Some(Ok(Outcome::Prepared(prepared)))) => {
                    self.prepare = None;
                    let proof = self.proof.as_ref().ok_or("修复证明已交接。")?;
                    let (service, cap) = RepairService::new(
                        self.paths.clone(),
                        proof,
                        &prepared.prefix,
                        &mut SystemRandom,
                    )
                    .map_err(str::to_owned)?;
                    let url = format!("{}#/repair/{}", prepared.prefix.as_str(), cap.as_str());
                    let runtime = LocalRuntime::start(prepared, Executor(self.handle.clone()))
                        .map_err(|e| e.message().to_string())?;
                    runtime
                        .attach_repair(service.clone())
                        .map_err(|e| e.message().to_string())?;
                    self.url = Some(url);
                    self.service = Some(service);
                    self.runtime = Some(runtime);
                    if self.closing {
                        self.service.as_ref().unwrap().request_close();
                    }
                }
                Ok(Some(Err(error))) => {
                    self.prepare = None;
                    return Err(error);
                }
                Ok(Some(_)) | Err(_) => {
                    self.prepare = None;
                    return Err("修复Web准备任务未完成。".into());
                }
            }
        }
        if let Some(runtime) = self.runtime.as_mut() {
            let status = runtime.poll();
            if status.phase == RuntimePhase::Faulted && !self.closing {
                return Err(status.error.map_or("修复HTTPS入口不可用。".into(), |e| {
                    e.message().into()
                }));
            }
            if status.phase == RuntimePhase::Running && !self.opened && !self.closing {
                let url = self.url.take().ok_or("修复浏览器入口已失效。")?;
                fc_windows::startup::open_local_management(&url)
                    .map_err(|_| "无法打开本机修复网页。")?;
                self.opened = true;
            }
        }
        if let Some(ticket) = self.pending.as_mut() {
            if ticket.is_finished() {
                let _ = ticket.try_result();
                self.pending = None;
            }
        }
        if let Some(service) = self.service.clone() {
            if service.exit_requested() {
                self.close();
            }
            if let Some(operation) = self.operation.as_mut() {
                if service.cancellation_requested() {
                    operation.cancel();
                }
                let status = operation.status();
                let phase = match status.phase {
                    ReinitializationPhase::Waiting | ReinitializationPhase::Draining => {
                        fc_protocol::web::repair::Phase::Draining
                    }
                    ReinitializationPhase::Committing => {
                        fc_protocol::web::repair::Phase::Committing
                    }
                    ReinitializationPhase::Retained => fc_protocol::web::repair::Phase::Retained,
                    ReinitializationPhase::Finished => fc_protocol::web::repair::Phase::Finished,
                };
                service.update(phase, None, status.result.as_ref().map(|v| v.is_ok()));
                if operation.reap_nonblocking() {
                    if let Some(result) = operation.status().result {
                        self.finished = true;
                        if result.is_err() {
                            service.update(
                                fc_protocol::web::repair::Phase::Finished,
                                Some("重新初始化未完成，受控记录已保留；请退出后重新修复。".into()),
                                Some(false),
                            );
                        }
                    }
                }
            }
            if self.operation.is_none() && !self.closing && service.has_confirmed() {
                let coordinator = PreparedReinitializationCoordinator::prepare()
                    .map_err(|_| "无法创建独立修复关闭协调器。")?;
                if let Some(prepared) = service.take_confirmed() {
                    let pool = self.pool.take().ok_or("后台执行池已交接。")?;
                    let proof = self.proof.take().ok_or("修复证明已交接。")?;
                    self.operation = Some(coordinator.start(
                        prepared,
                        ReinitializationBackend::NeverOpened(proof),
                        pool,
                        None,
                    ));
                }
            }
            if self.operation.is_none() && self.pending.is_none() {
                if let Some(work) = service.take_work() {
                    match self.handle.try_submit(
                        Work::Repair(work),
                        Instant::now() + Duration::from_secs(30 * 60),
                    ) {
                        Ok(v) => self.pending = Some(v),
                        Err(error) => {
                            let Work::Repair(work) = error.task else {
                                unreachable!()
                            };
                            service.return_work(work);
                        }
                    }
                }
            }
        }
        if self.closing && self.prepare.is_none() && self.pending.is_none() {
            let retired = self
                .service
                .as_ref()
                .is_none_or(|s| s.retirement_complete());
            let operation_done = self.operation.is_none() || self.finished;
            if retired && operation_done {
                if let Some(runtime) = self.runtime.as_mut() {
                    runtime.request_stop();
                    runtime.poll();
                    if !runtime.is_quiescent() {
                        return Ok(());
                    }
                }
                if let Some(pool) = self.pool.as_ref() {
                    pool.request_stop();
                    if !pool.is_finished_with(thread_is_finished) {
                        return Ok(());
                    }
                    let pool = self.pool.take().unwrap();
                    pool.join().map_err(|_| "修复执行池未能正常关闭。")?;
                }
                self.joined = true;
            }
        }
        Ok(())
    }
}
pub fn run() -> Result<()> {
    crate::installation_startup::run(|| {
        let paths = DataPaths::program_data().map_err(|_| "无法定位本机部署目录。")?;
        run_at(paths)
    })
}
fn run_at(paths: DataPaths) -> Result<()> {
    paths
        .verify_root_for_current_user()
        .map_err(|_| "当前部署目录不存在或权限不符合要求。")?;
    paths
        .prepare_instance_runtime()
        .map_err(|_| "运行目录不可安全使用。")?;
    let _pin = PinnedDirectory::open(&paths.runtime()).map_err(|_| "无法固定本机运行目录。")?;
    let _instance = InstanceGuard::acquire(paths.root())
        .map_err(|_| "未来教室仍在运行；请先从托盘正常退出。")?;
    let mut bytes = [0u8; 8];
    SystemRandom
        .fill(&mut bytes)
        .map_err(|_| "无法创建修复标识。")?;
    let sequence = u64::from_le_bytes(bytes);
    if sequence == 0 {
        return Err("无法创建修复标识，请重新启动。".into());
    }
    let proof = NeverOpenedProof::claim_repair_startup(&paths, sequence)
        .map_err(|_| "当前进程已打开过教学数据，不能启动独立修复。")?;
    let pool = WorkerPool::start().map_err(|_| "无法启动修复执行池。")?;
    let handle = pool.handle();
    let prepare = handle
        .try_submit(Work::Prepare, Instant::now() + Duration::from_secs(120))
        .map_err(|_| "无法排队准备修复Web。")?;
    let host = HiddenHost::new(true).map_err(|_| "无法创建修复消息宿主。")?;
    let stop = host.controller();
    let mut app = App {
        paths,
        pool: Some(pool),
        handle,
        prepare: Some(prepare),
        pending: None,
        runtime: None,
        service: None,
        proof: Some(proof),
        operation: None,
        url: None,
        opened: false,
        closing: false,
        joined: false,
        finished: false,
        error: None,
    };
    let hosted = host.run(|hints| {
        if hints.contains(HostHints::OPEN_STATUS) || hints.contains(HostHints::OPEN_SETTINGS) {
            if let Err(error) = app.reopen() {
                app.fail(error);
            }
        }
        if hints.contains(HostHints::EXIT_REQUESTED)
            || hints.contains(HostHints::SHUTDOWN)
            || hints.contains(HostHints::SESSION_LOCKED)
            || hints.contains(HostHints::SESSION_CHANGED)
        {
            app.close();
        }
        if let Err(error) = app.poll() {
            app.fail(error);
        }
        if app.joined {
            stop.stop();
        }
        Ok(())
    });
    if hosted.is_err() {
        app.fail("修复消息宿主异常退出。".into());
    }
    while !app.joined {
        if let Err(error) = app.poll() {
            app.fail(error);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Some(error) = app.error {
        Err(error)
    } else {
        Ok(())
    }
}
