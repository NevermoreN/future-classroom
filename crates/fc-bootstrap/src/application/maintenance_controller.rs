//! The tray pump retains only control facts. Existing cold coordinators own the
//! same Backend, workers and audio during replacement; local HTTPS stays alive.
use super::*;
use fc_bootstrap::{
    maintenance_web::{ConfirmedMaintenance, JobContext as MaintenanceJob, MaintenanceService},
    recovery_workflow::{PreparedCoordinator, RestorePhase},
    reinitialization::ReinitializationBackend,
    reinitialization_workflow::{
        PreparedReinitializationCoordinator, ReinitializationOperation, ReinitializationPhase,
    },
};
use fc_protocol::web::maintenance::Phase;

#[derive(Default)]
pub(super) struct State {
    pub(super) service: Option<Arc<MaintenanceService>>,
    pending: Option<JobTicket<Result<Outcome>>>,
    confirmed: Option<ConfirmedMaintenance>,
    job: Option<MaintenanceJob>,
    reset: Option<ReinitializationOperation<Work>>,
    finished: bool,
}
impl State {
    pub(super) fn pending_work(&self) -> bool {
        self.pending.is_some()
    }
}
impl App {
    pub(super) fn cancel_web_maintenance(&self) {
        if let Some(operation) = &self.maintenance.reset {
            operation.cancel();
        }
    }
    pub(super) fn poll_web_maintenance(&mut self) -> std::result::Result<(), String> {
        let Some(service) = self.maintenance.service.clone() else {
            return Ok(());
        };
        if let Some(ticket) = &mut self.maintenance.pending {
            if ticket.is_finished() && !matches!(ticket.try_result(), Ok(None)) {
                self.maintenance.pending = None;
            }
        }
        if self.maintenance.confirmed.is_none() && self.maintenance.job.is_none() {
            if let Some(confirmed) = service.take_confirmed() {
                self.maintenance.job = Some(match &confirmed {
                    ConfirmedMaintenance::Restore { job, .. }
                    | ConfirmedMaintenance::AbortOriginal { job, .. }
                    | ConfirmedMaintenance::Reset { job, .. } => job.clone(),
                });
                self.maintenance.confirmed = Some(confirmed);
                self.backup.irreversible = true;
                self.stopping = true;
                self.lease = None;
                self.desktop.stop();
                self.shell.stop();
                self.close_public_lesson();
                self.clear_profile_preview();
                self.stop_teacher_https();
                if let Some(runtime) = &self.local_web.runtime {
                    runtime.control().request_lan_stop();
                    runtime.control().request_business_drain();
                }
                if let Some(pending) = &self.pending {
                    pending.ticket.cancel();
                }
                if let Some(pending) = &self.autostart_job {
                    pending.ticket.cancel();
                }
                if let Some(pending) = &self.calendar_job {
                    pending.ticket.cancel();
                }
                if let Some(pending) = &self.content_maintenance {
                    pending.cancel();
                }
                if let Some(pending) = &self.voice_maintenance {
                    pending.cancel();
                }
                if let Some(audio) = &self.audio {
                    audio.shutdown();
                }
                self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(30 * 60));
            }
        }
        if self.maintenance.confirmed.is_some()
            && self.shell.is_finished()
            && self.teacher_https_quiescent()
            // The updater retains original cache/download/file owners until
            // its worker and network drain acknowledge actual return. Do not
            // move the common pool into a restore/reset coordinator earlier.
            && self.updates_quiescent()
            && self
                .local_web
                .runtime
                .as_ref()
                .is_some_and(|r| r.control().lan_quiescent() && r.control().business_quiescent())
        {
            // Prepare a coordinator before moving any physical owner.
            let backend = self.backend.clone().ok_or("维护后台所有权缺失。")?;
            let is_reset = matches!(
                self.maintenance.confirmed,
                Some(ConfirmedMaintenance::Reset { .. })
            );
            if is_reset {
                let prepared = PreparedReinitializationCoordinator::<Work>::prepare()
                    .map_err(|_| "无法启动重新初始化协调器。")?;
                let pool = self
                    .pool
                    .take_for_restore()
                    .ok_or("维护执行池所有权缺失。")?;
                let Some(ConfirmedMaintenance::Reset {
                    job: _,
                    prepared: reset,
                }) = self.maintenance.confirmed.take()
                else {
                    unreachable!()
                };
                self.maintenance.reset = Some(prepared.start(
                    *reset,
                    ReinitializationBackend::Live(backend),
                    pool,
                    self.audio.take(),
                ));
            } else {
                let prepared =
                    PreparedCoordinator::<Work>::prepare().map_err(|_| "无法启动恢复协调器。")?;
                let pool = self
                    .pool
                    .take_for_restore()
                    .ok_or("维护执行池所有权缺失。")?;
                let audio = self.audio.take().ok_or("维护声音所有权缺失。")?;
                self.backup.coordinator = Some(
                    match self.maintenance.confirmed.take().expect("confirmed owner") {
                        ConfirmedMaintenance::Restore { bundle, .. } => {
                            prepared.start(bundle, backend, pool, audio)
                        }
                        ConfirmedMaintenance::AbortOriginal { workflow, .. } => {
                            workflow.request_close();
                            prepared.start_abort_original(backend, pool, audio)
                        }
                        ConfirmedMaintenance::Reset { .. } => unreachable!(),
                    },
                );
            }
        }
        if let Some(job) = self.maintenance.job.clone() {
            if let Some(operation) = &mut self.backup.coordinator {
                if job.cancellation_requested() {
                    operation.cancel();
                }
                let status = operation.status();
                let phase = match status.phase {
                    RestorePhase::Waiting | RestorePhase::Draining => Phase::Draining,
                    RestorePhase::PreparingDatabase => Phase::PreparingDatabase,
                    RestorePhase::Sealing => Phase::Sealing,
                    RestorePhase::Committing => Phase::Committing,
                    RestorePhase::Retained => Phase::Retained,
                    RestorePhase::Finished => Phase::Draining,
                };
                if !self.maintenance.finished {
                    let _ = job.update(phase, |_| {});
                }
                if operation.reap_nonblocking() && !self.maintenance.finished {
                    if let Some(result) = status.result {
                        self.finish_web_maintenance(result.is_ok());
                    }
                }
            }
            if let Some(operation) = &mut self.maintenance.reset {
                if job.cancellation_requested() {
                    operation.cancel();
                }
                let status = operation.status();
                let phase = match status.phase {
                    ReinitializationPhase::Waiting | ReinitializationPhase::Draining => {
                        Phase::Draining
                    }
                    ReinitializationPhase::Committing => Phase::Committing,
                    ReinitializationPhase::Retained => Phase::Retained,
                    ReinitializationPhase::Finished => Phase::Draining,
                };
                if !self.maintenance.finished {
                    let _ = job.update(phase, |_| {});
                }
                if operation.reap_nonblocking() && !self.maintenance.finished {
                    if let Some(result) = status.result {
                        self.finish_web_maintenance(result.is_ok());
                    }
                }
            }
        }
        if (!self.stopping || self.exiting)
            && self.maintenance.job.is_none()
            && self.maintenance.pending.is_none()
        {
            if let (Some(backend), Some(work)) = (&self.backend, service.take_work()) {
                match self.pool.handle().try_submit(
                    Work::Maintenance(Box::new(work), backend.clone()),
                    Instant::now() + Duration::from_secs(30 * 60),
                ) {
                    Ok(ticket) => self.maintenance.pending = Some(ticket),
                    Err(rejected) => service.return_work(*match rejected.task {
                        Work::Maintenance(work, _) => work,
                        _ => unreachable!(),
                    }),
                }
            }
        }
        Ok(())
    }
    fn finish_web_maintenance(&mut self, succeeded: bool) {
        self.maintenance.finished = true;
        self.backup.finished = true;
        self.backup.acknowledged = true;
        self.writer_closed = true;
        self.shutdown_deadline = None;
        if let Some(job) = &self.maintenance.job {
            let _ = job.update(Phase::Finished, |snapshot| {
                snapshot.restart_required = true;
                snapshot.error = (!succeeded)
                    .then(|| "维护未完成；原有数据及核对记录已保留。请退出并重新启动程序。".into());
            });
        }
    }
}
