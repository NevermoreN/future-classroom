use super::*;
use crate::{
    backend::{
        BackendError, VoiceNativeParticipation, VoicePlaybackAuthorization, VoicePolicyHandoff,
        VoicePolicyPhase, VoiceRuntimeHandle, VoiceService,
    },
    voice_files::ControlledVoiceFiles,
};
use fc_audio::execution::{ExecutionError, ExecutionStatus, VoiceExecutionPort, VoiceExecutor};
use fc_domain::voice::*;
use fc_ports::{audio::AudioAuthorization, voice::VoiceRepositoryError, Clock, MonotonicClock};
use fc_windows::{
    audio::{device::DefaultRenderMonitor, VerifiedWave, WinMmAudioOutput},
    system::CurrentClock,
};
use std::path::PathBuf;

pub struct NativeFactory {
    service: VoiceService,
    root: PathBuf,
    clock: Arc<CurrentClock>,
}
impl NativeFactory {
    /// No filesystem/COM/device access on the caller. All native ownership is
    /// constructed lazily in the sole audio execution thread.
    pub fn new(service: VoiceService, root: PathBuf, clock: Arc<CurrentClock>) -> Self {
        Self {
            service,
            root,
            clock,
        }
    }
}
impl RuntimeFactory for NativeFactory {
    fn invalidate_runtime(&self) {
        self.service.invalidate_runtime();
    }
    fn create(&self, control: RuntimeControl) -> Result<Box<dyn AudioDomain>, RuntimeError> {
        // Register before capturing a service epoch. Reviewed setters may only
        // auto-resume an empty deployment with no live native participant.
        let participation = self
            .service
            .register_native_domain()
            .map_err(|_| RuntimeError::Factory)?;
        let handle = self.service.runtime_handle();
        // A fresh domain at an already committed epoch must play fresh jobs,
        // rather than consume the prior domain's retirement receipt again.
        let ignored_committed_serial = self
            .service
            .policy_handoff(handle.generation())
            .map_err(|_| RuntimeError::Storage)?
            .filter(|handoff| {
                handoff.phase == VoicePolicyPhase::Committed
                    && handoff.generation == handle.generation()
            })
            .map(|handoff| handoff.serial);
        Ok(Box::new(NativeDomain {
            service: self.service.clone(),
            stop_epoch: handle.generation(),
            ignored_committed_serial,
            failed_interrupted_serial: None,
            handle,
            files: ControlledVoiceFiles::new(&self.root).map_err(|_| RuntimeError::Factory)?,
            clock: self.clock.clone(),
            generation: control.generation(),
            control,
            executor: VoiceExecutor::new(WinMmAudioOutput::new()),
            monitor: None,
            stopping: false,
            authorization: None,
            _participation: participation,
        }))
    }
}
fn repository<T>(result: crate::backend::Result<T>) -> Result<T, VoiceRepositoryError> {
    result.map_err(|error| match error {
        BackendError::Voice(error) => error,
        _ => VoiceRepositoryError::Unavailable,
    })
}
struct Port<'a> {
    service: &'a VoiceService,
    policy_generation: u64,
    ignored_committed_serial: Option<u64>,
    handle: &'a VoiceRuntimeHandle,
    files: &'a ControlledVoiceFiles,
    clock: &'a CurrentClock,
    control: &'a RuntimeControl,
    generation: u64,
    authorization: &'a mut Option<VoicePlaybackAuthorization>,
}
impl Port<'_> {
    fn policy_handoff(&self) -> Option<VoicePolicyHandoff> {
        self.service
            .policy_handoff(self.policy_generation)
            .ok()
            .flatten()
            .filter(|handoff| {
                !(handoff.phase == VoicePolicyPhase::Committed
                    && handoff.generation == self.policy_generation
                    && self.ignored_committed_serial == Some(handoff.serial))
            })
    }
    fn refresh(&mut self, job: &VoiceJob) {
        if self.authorization.as_ref().is_some_and(|a| {
            a.job.id == job.id
                && a.job.generation == job.generation
                && a.job.revision == job.revision
                && a.job.state == job.state
        }) {
            return;
        }
        *self.authorization = if job.state.terminal() {
            None
        } else {
            self.handle.guard_for(job).ok()
        };
    }
}
impl VoiceExecutionPort<VerifiedWave> for Port<'_> {
    fn monotonic_millis(&self) -> u64 {
        self.clock.monotonic_millis()
    }
    fn civil_seconds(&self) -> i64 {
        civil_seconds(self.clock.now())
    }
    fn inventory(&mut self) -> Result<VoiceSnapshot, VoiceRepositoryError> {
        let snapshot = repository(self.handle.inventory())?;
        if let Some(job) = snapshot
            .jobs
            .iter()
            .filter(|j| !j.state.terminal())
            .min_by_key(|j| j.sequence)
        {
            self.refresh(job);
        } else {
            *self.authorization = None;
        }
        Ok(snapshot)
    }
    fn drive(&mut self, mut update: VoiceDriverUpdate) -> Result<VoiceJob, VoiceRepositoryError> {
        // An independent stop may arrive while the executor is inside a source
        // read/native wait. Its guard denial is interruption, not a new playback
        // failure; generation invalidation still precedes native reset.
        if self.control.generation() != self.generation
            && matches!(update.action, VoiceDriverAction::Fail(_))
        {
            update.action = VoiceDriverAction::Interrupt;
        }
        if self
            .policy_handoff()
            .is_some_and(|handoff| handoff.phase == VoicePolicyPhase::Pending)
        {
            return Err(VoiceRepositoryError::Domain(VoiceError::StaleGeneration));
        }
        let result = repository(self.handle.drive(update));
        // A policy can start while a native poll is in progress. A denied old
        // epoch must not be misreported as a new output failure: the bounded
        // driver guard forbids its SQL mutation until the policy settles.
        let job = match result {
            Err(VoiceRepositoryError::Domain(VoiceError::Permission(_)))
                if self.policy_handoff().is_some() =>
            {
                return Err(VoiceRepositoryError::Domain(VoiceError::StaleGeneration));
            }
            result => result?,
        };
        self.refresh(&job);
        Ok(job)
    }
    fn authorize(&mut self, job: &VoiceJob) -> AudioAuthorization {
        if !self.control.permits(self.generation) {
            return AudioAuthorization::Revoked;
        }
        if let Some(a) = self.authorization.as_ref().filter(|a| {
            a.job.id == job.id
                && a.job.generation == job.generation
                && a.job.revision == job.revision
                && a.job.state == job.state
        }) {
            return a.guard.decision();
        }
        match self.handle.availability_decision() {
            AudioAuthorization::Deferred => AudioAuthorization::Deferred,
            _ => AudioAuthorization::Revoked,
        }
    }
    fn source(&mut self, asset: &VoiceAsset) -> Result<VerifiedWave, VoiceRepositoryError> {
        self.files
            .source(asset)
            .map_err(|_| VoiceRepositoryError::Domain(VoiceError::InvalidWav))
    }
}
struct NativeDomain {
    service: VoiceService,
    handle: VoiceRuntimeHandle,
    files: ControlledVoiceFiles,
    clock: Arc<CurrentClock>,
    control: RuntimeControl,
    generation: u64,
    stop_epoch: u64,
    ignored_committed_serial: Option<u64>,
    failed_interrupted_serial: Option<u64>,
    executor: VoiceExecutor<WinMmAudioOutput>,
    monitor: Option<DefaultRenderMonitor>,
    stopping: bool,
    authorization: Option<VoicePlaybackAuthorization>,
    // Rust drops fields in declaration order. Keep the participation fence
    // until every native executor/listener/control field has been destroyed.
    // It is not a Released receipt or proof of physical OS-thread termination.
    _participation: VoiceNativeParticipation,
}
impl NativeDomain {
    fn policy_handoff(&self) -> Result<Option<VoicePolicyHandoff>, RuntimeError> {
        Ok(self
            .service
            .policy_handoff(self.stop_epoch)
            .map_err(|_| RuntimeError::Storage)?
            .filter(|handoff| {
                !(handoff.phase == VoicePolicyPhase::Committed
                    && handoff.generation == self.stop_epoch
                    && self.ignored_committed_serial == Some(handoff.serial))
            }))
    }
}
impl AudioDomain for NativeDomain {
    fn interrupt(&mut self, _: StopReasons) -> Result<(), RuntimeError> {
        self.stopping = true;
        self.stop_epoch = self.service.generation();
        // This query/commit has the service's independent 100 ms writer bound.
        // Even an unknown result must physically stop; no Backend mutex is held.
        let result = self.handle.interrupt_all();
        self.executor.stop_accepting();
        result.map(|_| ()).map_err(|_| RuntimeError::Storage)
    }
    fn step(&mut self) -> Result<DomainProgress, RuntimeError> {
        if let Some(handoff) = self.policy_handoff()? {
            self.stopping = true;
            // stop_accepting performs native reset/poll without a writer call.
            // Keep this same owner/domain until the reviewed transaction has
            // settled; do not race it with a durable Fail/Released mutation.
            self.executor.stop_accepting();
            match handoff.phase {
                VoicePolicyPhase::Pending => return Ok(DomainProgress::Draining),
                VoicePolicyPhase::Failed => {
                    // An unsuccessful save authorizes only negative cleanup.
                    // No new starts or admission reopening follow this receipt.
                    if self.failed_interrupted_serial != Some(handoff.serial) {
                        self.handle
                            .interrupt_all()
                            .map_err(|_| RuntimeError::Storage)?;
                        // interrupt_all wakes the supervisor even if no SQL was
                        // needed. Repeating it on every Idle tick would prevent
                        // the stable-wake retirement fence from ever passing.
                        self.failed_interrupted_serial = Some(handoff.serial);
                    }
                }
                VoicePolicyPhase::Committed => {}
            }
        }
        let inventory = self.handle.inventory().map_err(|_| RuntimeError::Storage)?;
        if !self.stopping
            && self.monitor.is_none()
            && self.control.permits(self.generation)
            && inventory.jobs.iter().any(|job| !job.state.terminal())
        {
            let control = self.control.clone();
            self.monitor = Some(
                DefaultRenderMonitor::start(Arc::new(move || {
                    control.request_stop(StopReason::DeviceChanged)
                }))
                .map_err(|_| RuntimeError::Driver)?,
            );
        }
        let mut port = Port {
            service: &self.service,
            policy_generation: self.stop_epoch,
            ignored_committed_serial: self.ignored_committed_serial,
            handle: &self.handle,
            files: &self.files,
            clock: &self.clock,
            control: &self.control,
            generation: self.generation,
            authorization: &mut self.authorization,
        };
        let result = self.executor.tick(
            &mut port,
            civil_seconds(self.clock.now()),
            self.clock.monotonic_millis(),
            20,
        );
        let status = match result {
            Ok(status) => status,
            Err(ExecutionError::Repository(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            ))) if self.policy_handoff()?.is_some() => {
                self.stopping = true;
                self.executor.stop_accepting();
                return Ok(DomainProgress::Draining);
            }
            Err(_) => {
                self.executor.stop_accepting();
                return Err(RuntimeError::Driver);
            }
        };
        match status {
            ExecutionStatus::Active { .. } => Ok(if self.stopping {
                DomainProgress::Draining
            } else {
                DomainProgress::Active
            }),
            ExecutionStatus::Waiting { .. } => Ok(DomainProgress::Gap),
            ExecutionStatus::Retained { .. } => Ok(DomainProgress::Retained),
            ExecutionStatus::Idle | ExecutionStatus::Halted => {
                let inventory = self.handle.inventory().map_err(|_| RuntimeError::Storage)?;
                if inventory.jobs.iter().any(|job| !job.state.terminal())
                    || self.executor.has_native_ownership()
                {
                    Ok(if self.stopping {
                        DomainProgress::Draining
                    } else {
                        DomainProgress::Active
                    })
                } else {
                    Ok(DomainProgress::Idle)
                }
            }
        }
    }
    fn has_native_ownership(&self) -> bool {
        self.executor.has_native_ownership()
    }
    fn resume_admission(&mut self) -> Result<(), RuntimeError> {
        if self.executor.has_native_ownership() {
            return Err(RuntimeError::Driver);
        }
        if let Some(handoff) = self.policy_handoff()? {
            match handoff.phase {
                VoicePolicyPhase::Pending => return Err(RuntimeError::Storage),
                VoicePolicyPhase::Failed => return Ok(()),
                VoicePolicyPhase::Committed if !handoff.enabled => return Ok(()),
                VoicePolicyPhase::Committed => {
                    // Only this committed continuous policy receipt permits an
                    // old domain to acknowledge a different service epoch. The
                    // CAS below cannot adopt a subsequent independent stop.
                    if self.service.is_admitting() {
                        return Ok(());
                    }
                    return self
                        .service
                        .resume_after_stop(handoff.generation)
                        .map_err(|_| RuntimeError::Storage);
                }
            }
        }
        if self.service.is_admitting() {
            if self.handle.is_current() {
                return Ok(());
            }
            return Err(RuntimeError::Storage);
        }
        self.service
            .resume_after_stop(self.stop_epoch)
            .map_err(|_| RuntimeError::Storage)
    }
    fn finish(&mut self) -> Result<bool, RuntimeError> {
        if self.executor.has_native_ownership() {
            return Ok(false);
        }
        if let Some(monitor) = &mut self.monitor {
            if !monitor.close().map_err(|_| RuntimeError::Driver)? {
                return Ok(false);
            }
        }
        self.monitor = None;
        // Retirement/deletion happens with no native output and on this worker,
        // never in UI. Failed cleanup retains quota for startup reconciliation.
        self.files
            .reconcile_service(&self.service)
            .map_err(|_| RuntimeError::Storage)?;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "native_policy_tests.rs"]
mod policy_tests;
