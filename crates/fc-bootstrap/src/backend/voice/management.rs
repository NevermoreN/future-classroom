//! Local management metadata. No upload, device or playback capability is exposed.
use super::*;
use fc_runtime::{authority::AuthoritySnapshot, health::HealthStatus};
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceManagementSnapshot {
    /// Global publication revision for delivery freshness, distinct from identity.
    pub publication_revision: Revision,
    pub identity_revision: Revision,
    pub revision: Revision,
    pub limits: VoiceLimits,
    /// Persisted setting, which can differ from fail-closed runtime admission.
    pub enabled: bool,
    /// Effective logical admission, never a claim that native audio has stopped.
    pub admitting: bool,
    pub runtime_generation: u64,
    /// Every nonterminal state, including Starting, Gap and Stopping.
    pub active_jobs: usize,
    pub queued_jobs: usize,
    /// Only the durable Playing state; excludes Starting and Stopping.
    pub playing_jobs: usize,
}

#[derive(Clone, Copy)]
enum PolicyChange {
    Enabled(bool),
    Limits(VoiceLimits),
}

impl VoiceService {
    /// Binds consent to identity, voice CAS and one exact runtime epoch. The
    /// writer repeats the identity/epoch/exact-lease check before COMMIT.
    pub fn set_enabled_reviewed(
        &self,
        lease: SessionLease,
        enabled: bool,
        expected_voice: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
    ) -> Result<VoiceSnapshot> {
        self.change_policy_reviewed(
            lease,
            PolicyChange::Enabled(enabled),
            expected_voice,
            expected_identity,
            expected_runtime,
        )
    }

    /// Saving limits retires previous recordings and jobs even when enabled is
    /// unchanged. It grants no Web configuration permission or replay action.
    pub fn set_limits_reviewed(
        &self,
        lease: SessionLease,
        limits: VoiceLimits,
        expected_voice: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
    ) -> Result<VoiceSnapshot> {
        self.change_policy_reviewed(
            lease,
            PolicyChange::Limits(limits),
            expected_voice,
            expected_identity,
            expected_runtime,
        )
    }

    fn change_policy_reviewed(
        &self,
        lease: SessionLease,
        change: PolicyChange,
        expected_voice: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
    ) -> Result<VoiceSnapshot> {
        let local_generation = self.inner.control.generation();
        let state = self
            .inner
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        let actor = self.management_actor(lease, local_generation, &state)?;
        if let PolicyChange::Limits(limits) = &change {
            limits
                .validate()
                .map_err(|error| BackendError::Voice(VoiceRepositoryError::Domain(error)))?;
        }
        if state.data.revision != expected_identity {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::RevisionConflict,
            )));
        }
        let word = self.inner.runtime_state.load(Ordering::Acquire);
        if expected_runtime >= (u64::MAX >> 1) || word >> 1 != expected_runtime {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            )));
        }
        if self.inventory()?.revision != expected_voice {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::RevisionConflict,
            )));
        }
        if self.management_actor(lease, local_generation, &state)? != actor {
            return Err(denied());
        }
        let boundary = !matches!(change, PolicyChange::Enabled(true));
        let epoch = if boundary {
            expected_runtime
                .checked_add(1)
                .ok_or(BackendError::Overflow)?
        } else {
            expected_runtime
        };
        // Publish bounded Pending before the CAS/wake so an already running
        // domain cannot settle old driver writes ahead of this policy ticket.
        let mut handoff = self.begin_policy_change(expected_runtime, epoch, boundary)?;
        if !boundary {
            if self.inner.runtime_state.load(Ordering::Acquire) != word {
                return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                    VoiceError::StaleGeneration,
                )));
            }
            handoff.claim();
        } else {
            self.inner
                .runtime_state
                .compare_exchange(word, epoch << 1, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| {
                    BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::StaleGeneration))
                })?;
            handoff.claim();
            self.inner.enabled.store(false, Ordering::Release);
            // The independent driver must start stopping before the writer can
            // block or reject persistence. Never adopt a later stop's epoch.
            self.wake();
            if epoch >= (u64::MAX >> 1) {
                // Exhaustion permanently closes admission instead of leaving
                // the penultimate epoch running or wrapping to an old review.
                return Err(BackendError::Overflow);
            }
        }
        let repository =
            self.repository_mode(epoch, None, false)
                .with_session_guard(self.exact_session_guard(
                    lease,
                    local_generation,
                    Some((expected_identity, epoch)),
                ));
        let persisted = match change {
            PolicyChange::Enabled(enabled) => {
                repository.set_voice_enabled(enabled, expected_voice, &actor)
            }
            PolicyChange::Limits(limits) => {
                repository.set_voice_limits(limits, expected_voice, &actor)
            }
        };
        let snapshot = match persisted {
            Ok(snapshot) => snapshot,
            Err(error) => {
                // The known policy endpoint is already closed. Do not use
                // conclude() to advance/adopt a different stop on uncertainty.
                handoff.failed();
                self.wake();
                return Err(BackendError::Voice(error));
            }
        };
        #[cfg(test)]
        self.after_committed();
        if self.generation() != epoch {
            self.inner.enabled.store(false, Ordering::Release);
            self.wake();
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            )));
        }
        for job in &snapshot.jobs {
            self.update(job)?;
        }
        if let Err(error) = handoff.committed(snapshot.enabled) {
            handoff.failed();
            self.wake();
            return Err(error);
        }
        self.inner
            .enabled
            .store(snapshot.enabled, Ordering::Release);
        if snapshot.enabled
            && snapshot.jobs.iter().all(|job| job.state.terminal())
            && !self.has_native_domain()
            && !self.is_admitting()
        {
            // resume_after_stop uses CAS against this exact epoch, including
            // after its writer fence; a later stop can never be reopened here.
            if let Err(error) = self.resume_after_stop(epoch) {
                // The exact old native participant may have won this same
                // epoch's safe resume CAS after the committed receipt.
                if !self.current(epoch) {
                    self.inner.enabled.store(false, Ordering::Release);
                    self.wake();
                    return Err(error);
                }
            }
        }
        if self.generation() != epoch {
            self.inner.enabled.store(false, Ordering::Release);
            self.wake();
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            )));
        }
        self.wake();
        Ok(snapshot)
    }

    fn management_current(
        &self,
        snapshot: &AuthoritySnapshot<IdentityState>,
    ) -> Result<HealthStatus> {
        self.check_open()?;
        let status = self.inner.control.health_gate().status();
        if status.health != Health::Healthy
            || status.revision != snapshot.revision
            || status.auth_epoch != snapshot.auth_epoch
        {
            return Err(BackendError::Storage(StorageError::Busy));
        }
        Ok(status)
    }

    pub(super) fn management_actor(
        &self,
        lease: SessionLease,
        local_generation: u64,
        snapshot: &AuthoritySnapshot<IdentityState>,
    ) -> Result<ActorContext> {
        if lease.origin() != TrustedOrigin::Local {
            return Err(denied());
        }
        let before = self.management_current(snapshot)?;
        self.inner.control.check(lease, local_generation)?;
        let actor = self.actor_from_state(lease, &snapshot.data)?;
        snapshot
            .data
            .authorize_management(&actor, self.inner.clock.now())
            .map_err(|_| denied())?;
        self.inner.control.check(lease, local_generation)?;
        if self.management_current(snapshot)? != before {
            return Err(BackendError::Storage(StorageError::Busy));
        }
        Ok(actor)
    }

    /// Reads only bounded metadata through the sole writer. Neither this read
    /// nor its final exact-lease check renews the manager's idle timer.
    pub fn management_snapshot(&self, lease: SessionLease) -> Result<VoiceManagementSnapshot> {
        self.check_open()?;
        let generation = self.inner.control.generation();
        let snapshot = self
            .inner
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        let actor = self.management_actor(lease, generation, &snapshot)?;
        let health = self.management_current(&snapshot)?;
        let runtime = self.inner.runtime_state.load(Ordering::Acquire);
        let enabled = self.inner.enabled.load(Ordering::Acquire);
        let inventory = self.inventory()?;
        let view = VoiceManagementSnapshot {
            publication_revision: snapshot.revision,
            identity_revision: snapshot.data.revision,
            revision: inventory.revision,
            limits: inventory.limits,
            enabled: inventory.enabled,
            admitting: inventory.enabled && enabled && runtime & 1 == 1,
            runtime_generation: runtime >> 1,
            active_jobs: inventory
                .jobs
                .iter()
                .filter(|job| !job.state.terminal())
                .count(),
            queued_jobs: inventory
                .jobs
                .iter()
                .filter(|job| job.state == VoiceState::Queued)
                .count(),
            playing_jobs: inventory
                .jobs
                .iter()
                .filter(|job| job.state == VoiceState::Playing)
                .count(),
        };
        if self.management_actor(lease, generation, &snapshot)? != actor
            || self.management_current(&snapshot)? != health
            || self.inner.runtime_state.load(Ordering::Acquire) != runtime
            || self.inner.enabled.load(Ordering::Acquire) != enabled
        {
            return Err(BackendError::Storage(StorageError::Busy));
        }
        Ok(view)
    }
}
