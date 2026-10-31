//! A worker-owned query receipt, never a global cache or an audio permission.
use super::*;
use fc_ports::audio::{AudioOutputError, DefaultOutputSupport};
use fc_runtime::worker::JobContext;

const OBSERVATION_LIFETIME: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceDeviceCheckResult {
    Supported,
    FormatUnsupported,
    Unavailable,
    /// No native error code, path, endpoint identity or driver text is exposed.
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceDeviceObservation {
    pub result: VoiceDeviceCheckResult,
    pub observed_at: LocalDateTime,
    pub observed_monotonic_millis: u64,
    observed: Instant,
    lease: SessionLease,
    publication_revision: Revision,
    identity_revision: Revision,
    voice_revision: Revision,
    runtime_generation: u64,
}

impl VoiceDeviceObservation {
    /// Remaining historical observation lifetime, measured from completion of
    /// the native query, never from UI delivery. Zero means expired.
    pub fn remaining_validity(&self) -> Duration {
        Instant::now()
            .checked_duration_since(self.observed)
            .map(|age| OBSERVATION_LIFETIME.saturating_sub(age))
            .unwrap_or(Duration::ZERO)
    }

    pub fn is_fresh(&self) -> bool {
        !self.remaining_validity().is_zero()
    }

    /// Historical format support only. This does not check current hardware,
    /// authorize a session, establish physical release, or promise audible sound.
    /// The UI must first accept the fresh snapshot through its normal lease gate.
    pub fn is_current_for(&self, snapshot: &VoiceManagementSnapshot, lease: SessionLease) -> bool {
        self.current_at(snapshot, lease, Instant::now())
    }

    fn current_at(
        &self,
        snapshot: &VoiceManagementSnapshot,
        lease: SessionLease,
        now: Instant,
    ) -> bool {
        lease == self.lease
            && snapshot.publication_revision == self.publication_revision
            && snapshot.identity_revision == self.identity_revision
            && snapshot.revision == self.voice_revision
            && snapshot.runtime_generation == self.runtime_generation
            && now
                .checked_duration_since(self.observed)
                .is_some_and(|age| age < OBSERVATION_LIFETIME)
    }
}

impl VoiceManagementSnapshot {
    pub fn current_device_observation(
        &self,
        lease: SessionLease,
    ) -> Option<&VoiceDeviceObservation> {
        self.device_observation
            .as_ref()
            .filter(|observation| observation.is_current_for(self, lease))
    }
}

impl VoiceService {
    /// Invoke on an existing shared worker, with a trusted query-only platform
    /// closure. No Backend/writer/session lock is held across that closure.
    /// Cancellation is cooperative: an in-flight synchronous query still owns
    /// its worker until return, but its late result cannot become an observation.
    #[allow(clippy::too_many_arguments)]
    pub fn check_default_output_reviewed(
        &self,
        lease: SessionLease,
        expected_publication: Revision,
        expected_voice: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
        context: &JobContext,
        query: impl FnOnce() -> std::result::Result<DefaultOutputSupport, AudioOutputError>,
    ) -> Result<VoiceManagementSnapshot> {
        let checkpoint = || {
            context
                .checkpoint()
                .map_err(|_| BackendError::Storage(StorageError::ExecutionCancelled))
        };
        checkpoint()?;
        // This clone scopes every metadata writer wait to this job, without
        // installing the job lifetime in the shared audio service.
        let service = self.with_execution_fence(Arc::new(context.execution_fence()));
        let matches_review = |view: &VoiceManagementSnapshot| -> Result<()> {
            if view.runtime_generation != expected_runtime {
                return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                    VoiceError::StaleGeneration,
                )));
            }
            if view.publication_revision != expected_publication
                || view.identity_revision != expected_identity
                || view.revision != expected_voice
            {
                return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                    VoiceError::RevisionConflict,
                )));
            }
            Ok(())
        };
        let initial = service.management_snapshot(lease)?;
        matches_review(&initial)?;
        let local_generation = service.inner.control.generation();
        let authority = service
            .inner
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        if authority.revision != expected_publication
            || authority.data.revision != expected_identity
        {
            return Err(BackendError::Storage(StorageError::Busy));
        }
        let actor = service.management_actor(lease, local_generation, &authority)?;
        let health = service.management_current(&authority)?;
        let runtime = service.inner.runtime_state.load(Ordering::Acquire);
        let before_tick = service.inner.monotonic.monotonic_millis();
        let interval_current = || -> Result<()> {
            checkpoint()?;
            let tick = service.inner.monotonic.monotonic_millis();
            if tick < before_tick
                || service.management_actor(lease, local_generation, &authority)? != actor
                || service.management_current(&authority)? != health
                || service.inner.runtime_state.load(Ordering::Acquire) != runtime
                || runtime >> 1 != expected_runtime
            {
                return Err(BackendError::Storage(StorageError::Busy));
            }
            checkpoint()
        };
        interval_current()?;
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(query)) {
            Ok(Ok(DefaultOutputSupport::Supported)) => VoiceDeviceCheckResult::Supported,
            Ok(Ok(DefaultOutputSupport::FormatUnsupported)) => {
                VoiceDeviceCheckResult::FormatUnsupported
            }
            Ok(Ok(DefaultOutputSupport::Unavailable)) => VoiceDeviceCheckResult::Unavailable,
            Ok(Err(_)) | Err(_) => VoiceDeviceCheckResult::Failed,
        };
        interval_current()?;
        let observed_at = service.inner.clock.now();
        let observed_monotonic_millis = service.inner.monotonic.monotonic_millis();
        let observed = Instant::now();
        let mut view = service.management_snapshot(lease)?;
        matches_review(&view)?;
        interval_current()?;
        if service.inner.monotonic.monotonic_millis() < observed_monotonic_millis {
            return Err(BackendError::Storage(StorageError::Busy));
        }
        view.device_observation = Some(VoiceDeviceObservation {
            result,
            observed_at,
            observed_monotonic_millis,
            observed,
            lease,
            publication_revision: expected_publication,
            identity_revision: expected_identity,
            voice_revision: expected_voice,
            runtime_generation: expected_runtime,
        });
        interval_current()?;
        if view.current_device_observation(lease).is_none() {
            return Err(BackendError::Storage(StorageError::ExecutionCancelled));
        }
        interval_current()?;
        Ok(view)
    }
}

#[cfg(test)]
mod tests;
