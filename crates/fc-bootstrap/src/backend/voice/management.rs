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
        let local_generation = self.inner.control.generation();
        let state = self
            .inner
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        let actor = self.management_actor(lease, local_generation, &state)?;
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
        let epoch = if enabled {
            if self.inner.runtime_state.load(Ordering::Acquire) != word {
                return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                    VoiceError::StaleGeneration,
                )));
            }
            expected_runtime
        } else {
            let next = expected_runtime
                .checked_add(1)
                .ok_or(BackendError::Overflow)?;
            self.inner
                .runtime_state
                .compare_exchange(word, next << 1, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| {
                    BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::StaleGeneration))
                })?;
            self.inner.enabled.store(false, Ordering::Release);
            // The independent driver must start stopping before the writer can
            // block or reject persistence. Never adopt a later stop's epoch.
            self.wake();
            if next >= (u64::MAX >> 1) {
                // Exhaustion permanently closes admission instead of leaving
                // the penultimate epoch running or wrapping to an old review.
                return Err(BackendError::Overflow);
            }
            next
        };
        let repository =
            self.repository_mode(epoch, None, false)
                .with_session_guard(self.exact_session_guard(
                    lease,
                    local_generation,
                    Some((expected_identity, epoch)),
                ));
        let persisted =
            self.conclude(repository.set_voice_enabled(enabled, expected_voice, &actor));
        if persisted.is_err() {
            self.wake();
        }
        let snapshot = persisted?;
        #[cfg(test)]
        self.after_committed();
        if self.generation() != epoch {
            self.inner.enabled.store(false, Ordering::Release);
            self.wake();
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            )));
        }
        self.inner
            .enabled
            .store(snapshot.enabled, Ordering::Release);
        for job in &snapshot.jobs {
            self.update(job)?;
        }
        if enabled && snapshot.jobs.iter().all(|job| job.state.terminal()) && !self.is_admitting() {
            // resume_after_stop uses CAS against this exact epoch, including
            // after its writer fence; a later stop can never be reopened here.
            if let Err(error) = self.resume_after_stop(epoch) {
                self.inner.enabled.store(false, Ordering::Release);
                self.wake();
                return Err(error);
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
