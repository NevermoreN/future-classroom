//! One maintenance job on the original bounded worker pool. The UI owns only
//! tickets and the shared control handle; all file/DPAPI/SQLite work is here.
use super::*;
use fc_bootstrap::updates::{UpdateControl, UpdateTick};

pub(super) enum Job {
    Open {
        backend: Arc<Mutex<Backend>>,
    },
    Retire {
        control: UpdateControl,
    },
    Bind {
        control: UpdateControl,
        local: fc_bootstrap::local_https::LocalControlHandle,
        app: Arc<fc_bootstrap::web_application::WebApplication>,
    },
    Tick {
        control: UpdateControl,
        backend: Arc<Mutex<Backend>>,
    },
    Abort {
        control: UpdateControl,
        backend: Arc<Mutex<Backend>>,
        job_id: [u8; 16],
    },
}
pub(super) enum Completed {
    Opened(UpdateControl),
    Tick(UpdateTick),
    Aborted,
    Retired,
    Bound,
    Error(String),
}
#[derive(Default)]
pub(super) struct State {
    pub(super) control: Option<UpdateControl>,
    pub(super) initialized: bool,
    pub(super) attempted: bool,
    pub(super) bound: bool,
    pending: Option<JobTicket<Result<Outcome>>>,
    next: Option<Instant>,
    next_io: Option<Instant>,
    abort: Option<[u8; 16]>,
    retired: bool,
    pub(super) failure: Option<String>,
}
impl Job {
    pub(super) fn run(self, context: &JobContext) -> Result<Outcome> {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        let result = match self {
            Self::Open { backend } => {
                let backend = backend.lock().map_err(|_| Failure::Stopped)?;
                backend
                    .update_runtime_state()
                    .map_err(|error| format!("更新启动状态不可用：{error:?}"))
                    .and_then(|state| UpdateControl::open_with_state(&state).map(Completed::Opened))
            }
            Self::Bind {
                control,
                local,
                app,
            } => local
                .attach_updates(control.network())
                .map(|()| {
                    app.set_update_control(control);
                    Completed::Bound
                })
                .map_err(|_| "更新网络入口未能绑定。".into()),
            Self::Retire { control } => control.retire().map(|()| Completed::Retired),
            Self::Tick { control, backend } => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                let live = context.execution_fence();
                control
                    .tick_cancellable(&mut backend, &|| !live())
                    .map(Completed::Tick)
            }
            Self::Abort {
                control,
                backend,
                job_id,
            } => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                control
                    .abort_handoff(&mut backend, job_id)
                    .map(|()| Completed::Aborted)
            }
        };
        Ok(Outcome::SoftwareUpdate(
            result.unwrap_or_else(Completed::Error),
        ))
    }
}
impl App {
    pub(super) fn updates_work_pending(&self) -> bool {
        !self.offscreen_probe
            && (self.updates.pending.is_some()
                || self.updates.abort.is_some()
                || self
                    .updates
                    .control
                    .as_ref()
                    .is_some_and(UpdateControl::has_network_work))
    }
    pub(super) fn poll_updates(&mut self) {
        if self.offscreen_probe {
            return;
        }
        let completed =
            self.updates
                .pending
                .as_mut()
                .and_then(|ticket| match ticket.try_result() {
                    Ok(Some(value)) => Some(value),
                    Ok(None) => None,
                    Err(_) => Some(Err(Failure::Stopped)),
                });
        if let Some(completed) = completed {
            self.updates.pending = None;
            self.updates.attempted = true;
            match completed {
                Ok(Outcome::SoftwareUpdate(Completed::Opened(control))) => {
                    self.updates.control = Some(control);
                    self.updates.initialized = true;
                    self.updates.failure = None;
                }
                Ok(Outcome::SoftwareUpdate(Completed::Tick(UpdateTick::Handoff(prepared)))) => {
                    if self.stopping
                        || self.exiting
                        || self.backup.irreversible()
                        || self
                            .control
                            .as_ref()
                            .is_some_and(SessionControl::is_recovering)
                    {
                        self.updates.abort = Some(prepared.job_id);
                    } else {
                        match fc_windows::update_install::spawn_handoff(&prepared) {
                            Ok(child) => {
                                // Child is independent and waits for the original
                                // physical startup lease, not a logical UI close.
                                drop(child);
                                self.begin_shutdown();
                            }
                            Err(_) => {
                                self.updates.abort = Some(prepared.job_id);
                                self.updates.failure =
                                    Some("更新协调器未能启动，现有程序继续运行。".into());
                            }
                        }
                    }
                }
                Ok(Outcome::SoftwareUpdate(
                    Completed::Tick(UpdateTick::Idle) | Completed::Aborted,
                )) => {}
                Ok(Outcome::SoftwareUpdate(Completed::Retired)) => self.updates.retired = true,
                Ok(Outcome::SoftwareUpdate(Completed::Bound)) => self.updates.bound = true,
                Ok(Outcome::SoftwareUpdate(Completed::Error(message))) => {
                    self.updates.initialized = self.updates.control.is_some();
                    self.updates.failure = Some(message);
                    self.updates.next = Some(Instant::now() + Duration::from_secs(30));
                }
                Err(error) => {
                    self.updates.initialized = self.updates.control.is_some();
                    self.updates.failure = Some(error_text(&error));
                    self.updates.next = Some(Instant::now() + Duration::from_secs(30));
                }
                _ => {
                    self.updates.failure =
                        Some("更新任务返回了不匹配的结果，已暂停本次操作。".into())
                }
            }
        }
        if self.updates.pending.is_some() {
            return;
        }
        let job = if let (Some(job_id), Some(control), Some(backend)) = (
            self.updates.abort,
            self.updates.control.clone(),
            self.backend.clone(),
        ) {
            Job::Abort {
                control,
                backend,
                job_id,
            }
        } else if self.stopping || self.exiting {
            if self.updates.retired {
                return;
            }
            let Some(control) = self.updates.control.clone() else {
                self.updates.retired = true;
                return;
            };
            Job::Retire { control }
        } else {
            if self.backup.irreversible()
                || self
                    .control
                    .as_ref()
                    .is_some_and(SessionControl::is_recovering)
                || if self
                    .updates
                    .control
                    .as_ref()
                    .is_some_and(UpdateControl::has_network_work)
                {
                    self.updates
                        .next_io
                        .is_some_and(|next| Instant::now() < next)
                } else {
                    self.updates.next.is_some_and(|next| Instant::now() < next)
                }
            {
                return;
            }
            if !self.updates.initialized {
                let Some(backend) = self.backend.clone() else {
                    return;
                };
                Job::Open { backend }
            } else {
                if !self.updates.bound && self.local_web.attached {
                    if let (Some(control), Some(runtime), Some(app)) = (
                        self.updates.control.clone(),
                        self.local_web.runtime.as_ref(),
                        self.local_web.lan_app.clone(),
                    ) {
                        let job = Job::Bind {
                            control,
                            local: runtime.control(),
                            app,
                        };
                        if let Ok(ticket) = self.pool.handle().try_submit(
                            Work::SoftwareUpdate(Box::new(job)),
                            Instant::now() + Duration::from_secs(10),
                        ) {
                            self.updates.pending = Some(ticket);
                        }
                        return;
                    }
                }
                let (Some(control), Some(backend)) =
                    (self.updates.control.clone(), self.backend.clone())
                else {
                    return;
                };
                Job::Tick { control, backend }
            }
        };
        let abort = matches!(job, Job::Abort { .. });
        if let Ok(ticket) = self.pool.handle().try_submit(
            Work::SoftwareUpdate(Box::new(job)),
            Instant::now() + Duration::from_secs(40),
        ) {
            if abort {
                self.updates.abort = None;
            }
            self.updates.pending = Some(ticket);
            self.updates.next = Some(Instant::now() + Duration::from_secs(1));
            self.updates.next_io = Some(Instant::now() + Duration::from_millis(20));
        }
    }
    pub(super) fn updates_quiescent(&self) -> bool {
        self.offscreen_probe
            || (self.updates.pending.is_none()
                && self.updates.abort.is_none()
                && self.updates.retired
                && self
                    .updates
                    .control
                    .as_ref()
                    .is_none_or(|control| control.network().is_idle()))
    }
}
