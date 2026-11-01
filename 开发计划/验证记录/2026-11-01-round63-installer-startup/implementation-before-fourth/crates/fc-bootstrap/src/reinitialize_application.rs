//! A separate native repair process. It never opens teaching storage, DPAPI,
//! audio, shell integration or Web services before destructive confirmation.
#[path = "application/reinitialize_editor.rs"]
mod editor;
mod slot;
use slot::PreparationSlot;

use fc_bootstrap::{
    reinitialization::{NeverOpenedProof, ReinitializationBackend},
    reinitialization_files::{PreparedReinitialization, ReinitializationFiles},
    reinitialization_workflow::{
        PreparedReinitializationCoordinator, ReinitializationOperation, ReinitializationPhase,
    },
};
use fc_ports::RandomSource;
use fc_presentation::workbench::{Action, ActionId, Event, View};
use fc_runtime::worker::{JobContext, JobTicket, Task, WorkerHandle, WorkerPool};
use fc_windows::{
    host::{HiddenHost, HostHints},
    materials::files::PinnedDirectory,
    system::{thread_is_finished, DataPaths},
    workbench::Window,
    InstanceGuard, SystemRandom,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;
const FINISH: ActionId = ActionId(703);
const CANCEL_COMMIT: ActionId = ActionId(704);
const WORK_DEADLINE: Duration = Duration::from_secs(30 * 60);
type PreparedSlot = Arc<PreparationSlot<PreparedReinitialization>>;

#[derive(Clone, Copy)]
enum JobKind {
    Reconcile,
    Prepare,
    Retire,
}
enum Work {
    Reconcile(DataPaths),
    Prepare(DataPaths, u64, PreparedSlot),
    Retire(PreparedSlot),
}
enum Outcome {
    Reconciled,
    Prepared,
    Retired,
}
impl Task for Work {
    type Output = Result<Outcome>;
    fn allocation_bytes(&self) -> Option<usize> {
        match self {
            Self::Reconcile(paths) | Self::Prepare(paths, _, _) => paths
                .root()
                .as_os_str()
                .len()
                .checked_mul(2)?
                .checked_add(4096),
            Self::Retire(_) => Some(4096),
        }
    }
    fn run(self, context: &JobContext) -> Self::Output {
        let check = || context.checkpoint().is_ok();
        match self {
            Self::Reconcile(paths) => {
                let files = ReinitializationFiles::new(&paths)
                    .map_err(|error| format!("reinitialization reconcile files: {error:?}"))?;
                files
                    .reconcile_startup(&check)
                    .map_err(|error| format!("reinitialization startup: {error:?}"))?;
                Ok(Outcome::Reconciled)
            }
            Self::Prepare(paths, sequence, slot) => {
                let files = ReinitializationFiles::new(&paths)
                    .map_err(|error| format!("reinitialization prepare files: {error:?}"))?;
                let review = files
                    .review(sequence, &check)
                    .map_err(|error| format!("reinitialization inventory: {error:?}"))?;
                let prepared = match files.prepare(review, &check) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        // This is the single preparation in a proven-never-opened
                        // repair process. Retire only service-verified unarmed
                        // staging after cancellation; ambiguous bytes are retained.
                        let cleanup = files.reconcile_startup(&|| true);
                        return Err(match cleanup {
                            Ok(_)=>format!("reinitialization prepare: {error:?}"),
                            Err(cleanup)=>format!("reinitialization prepare: {error:?}; retained cleanup: {cleanup:?}"),
                        });
                    }
                };
                if !check() {
                    prepared.discard(&|| true).map_err(|error| {
                        format!("reinitialization cancelled preparation cleanup: {error:?}")
                    })?;
                    return Err("reinitialization preparation cancelled".into());
                }
                if let Err(prepared) = slot.publish(prepared) {
                    prepared.discard(&|| true).map_err(|error| {
                        format!("reinitialization rejected preparation cleanup: {error:?}")
                    })?;
                    return Err(
                        "reinitialization preparation cancelled or slot already owned".into(),
                    );
                }
                Ok(Outcome::Prepared)
            }
            Self::Retire(slot) => {
                // The mutex protects only a move. Recursive workspace cleanup
                // belongs to this ordinary worker, never the window thread.
                let prepared = slot.retire();
                if let Some(prepared) = prepared {
                    prepared.discard(&|| true).map_err(|error| {
                        format!("reinitialization cancellation cleanup: {error:?}")
                    })?;
                }
                Ok(Outcome::Retired)
            }
        }
    }
}

/// Diagnostic drivers can inspect native controls and request ordinary events;
/// none of the destructive capability or proof is exposed by this harness.
pub(crate) struct RepairHarness {
    paths: DataPaths,
    sequence: u64,
    window: Window,
    editor: editor::Editor,
    ready: bool,
    pool: Option<WorkerPool<Work>>,
    handle: WorkerHandle<Work>,
    job: Option<(JobKind, JobTicket<Result<Outcome>>)>,
    slot: PreparedSlot,
    proof: Option<NeverOpenedProof>,
    coordinator: Option<ReinitializationOperation<Work>>,
    last_phase: Option<ReinitializationPhase>,
    irreversible: bool,
    finished: bool,
    succeeded: bool,
    closing: bool,
    joined: bool,
    status: String,
    failure: Option<String>,
    shutdown_at: Option<Instant>,
}
impl Drop for RepairHarness {
    fn drop(&mut self) {
        // Unexpected failure must not bulk-release a still-owned prepared
        // capability's pins on the UI thread. Its evidence remains available
        // for the next repair process; normal exits empty the slot on a worker.
        if self.slot.is_retained() {
            std::mem::forget(self.slot.clone());
        }
    }
}
impl RepairHarness {
    pub(crate) fn window(&self) -> &Window {
        &self.window
    }
    pub(crate) fn sequence(&self) -> u64 {
        self.editor.sequence()
    }
    pub(crate) fn ready(&self) -> bool {
        self.ready && !self.closing && !self.irreversible
    }
    pub(crate) fn finished(&self) -> bool {
        self.finished
    }
    pub(crate) fn succeeded(&self) -> bool {
        self.finished && self.succeeded
    }
    pub(crate) fn irreversible(&self) -> bool {
        self.irreversible
    }
    pub(crate) fn exit_ready(&self) -> bool {
        self.closing && self.joined
    }
    pub(crate) fn view(&self) -> View {
        if self.irreversible() {
            return View {
                title:"正在清除本机资料并重新初始化".into(),
                description:"本次清除已明确确认。正在等待实际后台结束并核对本机目录。此页不能返回编辑旧资料；完成后关闭程序，重新启动并设置新的学校和主管理员。".into(),
                status:self.status.clone(),fields:vec![],grid:None,busy:false,
                actions:if self.finished { vec![Action { id:FINISH,label:"关闭程序，稍后重新启动".into(),enabled:true }] } else { vec![Action { id:CANCEL_COMMIT,label:"请求安全中止并等待关闭".into(),enabled:!self.closing }] },
            };
        }
        if !self.ready || self.closing {
            return View {
                title:if self.closing { "正在安全关闭修复窗口" } else { "正在核对本机重新初始化状态" }.into(),
                description:"尚未授权清除当前资料。正在等待真实后台任务结束；取消会关闭本次修复进程，不会返回已打开的普通工作台。".into(),
                status:self.status.clone(),fields:vec![],grid:None,busy:false,
                actions:vec![Action { id:editor::CANCEL,label:"取消并关闭修复进程".into(),enabled:!self.closing }],
            };
        }
        self.editor.view(&self.status, self.job.is_some())
    }
    fn render(&self) -> Result<()> {
        self.window
            .render(&self.view())
            .map_err(|_| "无法呈现重新初始化复核窗口。".into())
    }
    fn submit(&mut self, kind: JobKind, work: Work) -> Result<()> {
        if self.job.is_some() || self.pool.is_none() {
            return Err("重新初始化后台仍被占用。".into());
        }
        let ticket = self
            .handle
            .try_submit(work, Instant::now() + WORK_DEADLINE)
            .map_err(|_| "重新初始化后台暂不可用。".to_string())?;
        self.job = Some((kind, ticket));
        Ok(())
    }
    pub(crate) fn close(&mut self) {
        self.slot.close();
        self.closing = true;
        self.editor.invalidate();
        if let Some((_, ticket)) = &self.job {
            ticket.cancel();
        }
        if let Some(coordinator) = &self.coordinator {
            coordinator.cancel();
        }
        self.status = "已请求安全关闭；正在等待实际后台任务结束。".into();
    }
    fn fail(&mut self, error: String) {
        if self.failure.is_none() {
            self.failure = Some(error);
        }
        self.close();
    }
    pub(crate) fn event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Close => self.close(),
            Event::Submit(action, _) if self.irreversible => {
                if action == FINISH && self.finished {
                    self.closing = true;
                } else if action == CANCEL_COMMIT {
                    self.close();
                }
            }
            Event::Submit(action, _)
                if action == editor::CANCEL && (!self.ready || self.closing) =>
            {
                self.close()
            }
            Event::Submit(action, data) if self.ready && !self.closing => {
                if self.job.is_some() && action != editor::CANCEL {
                    return Ok(());
                }
                match self.editor.event(action, data) {
                    Ok(editor::Command::Prepare) => {
                        if let Err(error) = self.submit(
                            JobKind::Prepare,
                            Work::Prepare(self.paths.clone(), self.sequence, self.slot.clone()),
                        ) {
                            self.fail(error);
                        } else {
                            self.status =
                                "正在核对本机目录并准备；正式资料尚未清除，可取消。".into();
                        }
                    }
                    Ok(editor::Command::Cancel) => self.close(),
                    Ok(editor::Command::Confirm { sequence }) => {
                        if sequence != self.sequence() {
                            self.fail("重新初始化复核序号已失效。".into());
                        } else {
                            self.start_coordinator();
                        }
                    }
                    Err(error) => self.status = error,
                }
            }
            _ => {}
        }
        self.render()
    }
    fn start_coordinator(&mut self) {
        // Create the real coordinator thread before moving any irreversible
        // capability or physical pool ownership out of the repair process UI.
        let prepared_coordinator = match PreparedReinitializationCoordinator::<Work>::prepare() {
            Ok(prepared) => prepared,
            Err(_) => {
                self.fail("无法启动重新初始化执行器；未清除正式资料。".into());
                return;
            }
        };
        let prepared = self.slot.take_ready();
        let Some(prepared) = prepared else {
            self.fail("重新初始化准备已失效；未清除正式资料。".into());
            return;
        };
        // Sequence/root equality was prepared on the worker and independently
        // checked again by the opaque backend/permit coordinator.
        let proof = self
            .proof
            .take()
            .expect("repair proof has one owner until confirmation");
        let pool = self
            .pool
            .take()
            .expect("repair pool has one owner until confirmation");
        self.irreversible = true;
        self.ready = false;
        self.status = "已提交本次清除确认，正在排空后台并执行受控重新初始化。".into();
        self.coordinator = Some(prepared_coordinator.start(
            prepared,
            ReinitializationBackend::NeverOpened(proof),
            pool,
            None,
        ));
    }
    pub(crate) fn poll(&mut self) -> Result<()> {
        let mut redraw = false;
        if let Some(at) = self.shutdown_at {
            if Instant::now() >= at && !self.exit_ready() {
                // Only an actual OS shutdown grants this final bounded process
                // exit. Journals and quarantined bytes remain for next startup.
                std::process::exit(1);
            }
        }
        let complete = self
            .job
            .as_mut()
            .and_then(|(_, ticket)| match ticket.try_result() {
                Ok(None) => None,
                Ok(Some(value)) => Some(value),
                Err(error) => Some(Err(format!("repair worker completion: {error:?}"))),
            });
        if let Some(complete) = complete {
            let (kind, _) = self.job.take().expect("observed repair ticket retained");
            match complete {
                Ok(Outcome::Reconciled) if !self.closing => {
                    self.ready = true;
                    self.status = "本机重新初始化状态已核对。尚未授权清除资料。".into();
                }
                Ok(Outcome::Prepared) if !self.closing => {
                    self.editor.set_review(self.sequence)?;
                    self.status = "本次准备已完成，请分别核对资料丢失和全新设置要求。".into();
                }
                Ok(Outcome::Retired) => {}
                Ok(_) => {}
                Err(error) => {
                    if !self.closing || matches!(kind, JobKind::Reconcile) {
                        self.failure.get_or_insert(error);
                    }
                    self.close();
                }
            }
            self.render()?;
        }
        if let Some(coordinator) = &mut self.coordinator {
            let status = coordinator.status();
            if self.last_phase != Some(status.phase) {
                redraw = true;
                self.last_phase = Some(status.phase);
                self.status = match status.phase {
                    ReinitializationPhase::Waiting | ReinitializationPhase::Draining => "正在等待实际后台任务关闭；尚不能离开本次修复进程。",
                    ReinitializationPhase::Committing => "正在执行已确认的本机目录重新初始化，并保存完整核对记录。",
                    ReinitializationPhase::Retained => "实际后台结束尚未确认，操作所有权与核对资料仍被保留；不能声称已经关闭，请继续等待安全核对。",
                    ReinitializationPhase::Finished => "重新初始化执行器已返回，正在核对实际线程结束。",
                }.into();
            }
            if coordinator.reap_nonblocking() && !self.finished {
                let status = coordinator.status();
                if let Some(result) = status.result {
                    self.finished = true;
                    self.succeeded = result.is_ok();
                    if let Err(error) = &result {
                        self.failure.get_or_insert(format!(
                            "重新初始化未完成，请保留目录并重新启动核对：{error:?}"
                        ));
                    }
                    self.status=if result.is_ok() { "本机未来教室资料已重新初始化。请关闭程序后重新启动，设置新的学校并创建新的主管理员。" } else { "本次重新初始化未完成或已安全中止。请保留现有目录，关闭程序后重新启动并先核对恢复状态。" }.into();
                    self.render()?;
                }
            }
            if self.finished && self.closing {
                self.joined = true;
            }
        } else if self.closing && self.job.is_none() {
            let retained = self.slot.is_retained();
            if retained {
                if self.handle.is_faulted() {
                    let retained_status =
                        "后台故障，准备所有权仍被保留；请保留目录并等待安全结束。";
                    if self.status != retained_status {
                        self.status = retained_status.into();
                        redraw = true;
                    }
                } else if let Err(error) =
                    self.submit(JobKind::Retire, Work::Retire(self.slot.clone()))
                {
                    self.failure.get_or_insert(error);
                }
            } else if let Some(pool) = &self.pool {
                pool.request_stop();
                if pool.is_finished_with(thread_is_finished) {
                    let pool = self.pool.take().expect("finished repair pool retained");
                    if let Err(error) = pool.join() {
                        self.failure
                            .get_or_insert(format!("repair physical worker join: {error:?}"));
                    }
                    self.joined = true;
                }
            }
        }
        if redraw && !self.finished {
            self.render()?;
        }
        Ok(())
    }
}

pub fn run() -> Result<()> {
    let paths = DataPaths::program_data().map_err(|_| "无法定位本机未来教室目录。".to_string())?;
    run_at_with(paths, false, |_, _| Ok(()))
}
/// Root's isolated diagnostics supply a driver for the actual hidden native
/// window. The same startup proof, real workers and final coordinator are used.
pub(crate) fn run_at_with(
    paths: DataPaths,
    probe: bool,
    mut drive: impl FnMut(&mut RepairHarness, HostHints) -> Result<()>,
) -> Result<()> {
    paths
        .verify_root_for_current_user()
        .map_err(|_| "当前部署目录不存在或权限不符合要求；未创建或修改正式资料。".to_string())?;
    paths
        .prepare_instance_runtime()
        .map_err(|_| "当前运行目录不可安全使用；未在外部目录创建锁文件。".to_string())?;
    let _runtime_pin = PinnedDirectory::open(&paths.runtime())
        .map_err(|_| "当前运行目录不可安全固定；未启动重新初始化。".to_string())?;
    let _instance = InstanceGuard::acquire(paths.root())
        .map_err(|_| "未来教室仍在运行，或当前数据目录被占用；请先正常关闭原程序。".to_string())?;
    let mut random = [0u8; 8];
    SystemRandom
        .fill(&mut random)
        .map_err(|_| "无法创建本次重新初始化复核标识。".to_string())?;
    let sequence = u64::from_le_bytes(random);
    if sequence == 0 {
        return Err("无法创建本次重新初始化复核标识，请重新启动。".into());
    }
    let proof = NeverOpenedProof::claim_repair_startup(&paths, sequence).map_err(|error| {
        format!("无法证明当前进程未打开过教学数据，请关闭并以独立修复进程启动：{error:?}")
    })?;
    let window = Window::new().map_err(|_| "无法创建重新初始化原生窗口。".to_string())?;
    // The adapter's independent lock action is a generic out-of-band action:
    // here it is cancellation, so full worker queues cannot block safe close.
    window
        .set_lock_action(editor::CANCEL)
        .map_err(|_| "无法注册独立取消操作。".to_string())?;
    let host = HiddenHost::new(false).map_err(|_| "无法创建独立修复宿主。".to_string())?;
    let stop = host.controller();
    let pool = WorkerPool::start().map_err(|_| "无法启动固定后台执行池。".to_string())?;
    let handle = pool.handle();
    let mut app = RepairHarness {
        paths: paths.clone(),
        sequence,
        window,
        editor: editor::Editor::new(sequence),
        ready: false,
        pool: Some(pool),
        handle,
        job: None,
        slot: Arc::new(PreparationSlot::new()),
        proof: Some(proof),
        coordinator: None,
        last_phase: None,
        irreversible: false,
        finished: false,
        succeeded: false,
        closing: false,
        joined: false,
        status: "正在核对上次重新初始化状态；尚未打开教学数据。".into(),
        failure: None,
        shutdown_at: None,
    };
    if let Err(error) = app
        .submit(JobKind::Reconcile, Work::Reconcile(paths))
        .and_then(|()| app.render())
    {
        app.fail(error);
        while !app.exit_ready() {
            if let Err(error) = app.poll() {
                app.fail(error);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        return Err(app
            .failure
            .take()
            .unwrap_or_else(|| "无法启动重新初始化核对。".into()));
    }
    if !probe {
        app.window.show(true);
    }
    let hosted = host.run(|hints| {
        if hints.contains(HostHints::SHUTDOWN) {
            app.shutdown_at
                .get_or_insert(Instant::now() + Duration::from_secs(5));
            app.close();
        } else if hints.contains(HostHints::SESSION_LOCKED)
            || hints.contains(HostHints::SESSION_CHANGED)
        {
            app.close();
            app.window.hide();
        }
        if let Err(error) = drive(&mut app, hints) {
            app.fail(error);
        }
        match app.window.take_events() {
            Ok(events) => {
                // Rendering between commands invalidates assumptions in queued
                // forms. A single native state-changing submission is accepted
                // per batch; fresh sequence-bound choices are still mandatory.
                let mut submitted = false;
                for event in events {
                    if matches!(&event, Event::Submit(_, _)) {
                        if submitted {
                            continue;
                        }
                        submitted = true;
                    }
                    if let Err(error) = app.event(event) {
                        app.fail(error);
                    }
                }
            }
            Err(_) => app.fail("无法读取重新初始化窗口操作。".into()),
        }
        if let Err(error) = app.poll() {
            app.fail(error);
        }
        if app.exit_ready() {
            stop.stop();
        }
        Ok(())
    });
    if hosted.is_err() || !app.exit_ready() {
        // An adapter failure is never evidence that a coordinator physically
        // ended. Leave the journal and capability slot retained for diagnosis.
        app.fail("独立修复窗口异常结束；请保留数据目录并重新启动核对。".into());
        while !app.exit_ready() {
            if let Err(error) = app.poll() {
                app.fail(error);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    if let Some(error) = app.failure.take() {
        Err(error)
    } else {
        Ok(())
    }
}
