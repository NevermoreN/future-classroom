//! Local management metadata. Format observations grant no playback capability.
use super::*;
use fc_runtime::{authority::AuthoritySnapshot, health::HealthStatus};
mod device_check;
pub use device_check::{VoiceDeviceCheckResult, VoiceDeviceObservation};
#[cfg(test)]
mod tests;

fn typed_voice_error(error: StorageError) -> BackendError {
    match error {
        StorageError::Voice(error) => BackendError::Voice(VoiceRepositoryError::Domain(error)),
        other => BackendError::Storage(other),
    }
}

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
    /// Explicit historical format check only; ordinary reads never query native.
    pub device_observation: Option<VoiceDeviceObservation>,
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
        match self.apply_policy_window(
            lease,
            fc_ports::command_window::VoicePolicyIntent::Enabled {
                enabled,
                expected_voice,
                expected_identity,
                expected_runtime,
            },
        )? {
            fc_ports::command_window::TypedCommandOutcome::Completed(
                fc_ports::command_window::TypedCommandResult::VoicePolicy(snapshot),
            ) => Ok(snapshot),
            _ => Err(BackendError::Storage(StorageError::Indeterminate)),
        }
    }

    pub fn set_limits_reviewed(
        &self,
        lease: SessionLease,
        limits: VoiceLimits,
        expected_voice: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
    ) -> Result<VoiceSnapshot> {
        match self.apply_policy_window(
            lease,
            fc_ports::command_window::VoicePolicyIntent::Limits {
                limits,
                expected_voice,
                expected_identity,
                expected_runtime,
            },
        )? {
            fc_ports::command_window::TypedCommandOutcome::Completed(
                fc_ports::command_window::TypedCommandResult::VoicePolicy(snapshot),
            ) => Ok(snapshot),
            _ => Err(BackendError::Storage(StorageError::Indeterminate)),
        }
    }

    pub fn query_policy_outcome_native(
        &self,
        lease: SessionLease,
        issued: fc_ports::command_window::IssuedCommand,
        intent: fc_ports::command_window::VoicePolicyIntent,
    ) -> Result<fc_ports::command_window::TypedCommandOutcome> {
        let service = self.with_command_window(lease, issued)?;
        let actor = service.actor(lease, false)?;
        service
            .repository(service.generation(), Some(lease))
            .query_typed_command(
                fc_ports::command_window::TypedCommandIntent::VoicePolicy(intent),
                &actor,
            )
            .map_err(typed_voice_error)
    }
    pub fn apply_policy_window(
        &self,
        lease: SessionLease,
        intent: fc_ports::command_window::VoicePolicyIntent,
    ) -> Result<fc_ports::command_window::TypedCommandOutcome> {
        use fc_ports::command_window::{TypedCommandOutcome, VoicePolicyIntent};
        let issued = self.command_window.ok_or(StorageError::CommandNotIssued)?;
        // History uses current authority, not an obsolete business/runtime CAS.
        match self.query_policy_outcome_native(lease, issued, intent.clone()) {
            Ok(history) => return Ok(history),
            Err(BackendError::Storage(StorageError::CommandNotIssued)) => (),
            Err(error) => return Err(error),
        }
        let (change, expected_voice, expected_identity, expected_runtime) = match &intent {
            VoicePolicyIntent::Enabled {
                enabled,
                expected_voice,
                expected_identity,
                expected_runtime,
            } => (
                PolicyChange::Enabled(*enabled),
                *expected_voice,
                *expected_identity,
                *expected_runtime,
            ),
            VoicePolicyIntent::Limits {
                limits,
                expected_voice,
                expected_identity,
                expected_runtime,
            } => (
                PolicyChange::Limits(*limits),
                *expected_voice,
                *expected_identity,
                *expected_runtime,
            ),
        };
        let local_generation = self.inner.control.generation();
        let state = self
            .inner
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        let actor = self.management_actor(lease, local_generation, &state)?;
        let repository = self.repository(self.generation(), Some(lease));
        let admission = repository
            .accept_voice_policy(
                intent.clone(),
                &actor,
                self.exact_session_guard(
                    lease,
                    local_generation,
                    Some((expected_identity, expected_runtime)),
                ),
            )
            .map_err(typed_voice_error)?;
        match admission {
            fc_storage::typed_command::TypedAdmission::History(outcome) => Ok(outcome),
            fc_storage::typed_command::TypedAdmission::Fresh(permit) => {
                let mut permit = Some(permit);
                match self.execute_policy_first(
                    lease,
                    change,
                    expected_voice,
                    expected_identity,
                    expected_runtime,
                    &mut permit,
                ) {
                    Ok(snapshot) => Ok(TypedCommandOutcome::Completed(
                        fc_ports::command_window::TypedCommandResult::VoicePolicy(snapshot),
                    )),
                    Err(error) => {
                        if let Some(permit) = permit {
                            let failure = match &error {
                                BackendError::Voice(VoiceRepositoryError::Domain(
                                    VoiceError::RevisionConflict,
                                )) => Some(fc_ports::command_window::TypedCommandFailure::Conflict),
                                BackendError::Voice(VoiceRepositoryError::Domain(
                                    VoiceError::StaleGeneration,
                                )) => Some(fc_ports::command_window::TypedCommandFailure::Stale),
                                BackendError::Voice(VoiceRepositoryError::Domain(
                                    VoiceError::Permission(_),
                                )) => Some(fc_ports::command_window::TypedCommandFailure::Denied),
                                BackendError::Storage(StorageError::ExecutionCancelled) => {
                                    Some(fc_ports::command_window::TypedCommandFailure::Cancelled)
                                }
                                _ => None,
                            };
                            // A failed metadata write stays Pending; it never grants
                            // another native execution or fabricates owner return.
                            if let Some(failure) = failure {
                                let _ = self
                                    .repository(self.generation(), Some(lease))
                                    .fail_typed_command(permit, failure);
                            }
                        }
                        Err(error)
                    }
                }
            }
        }
    }

    fn execute_policy_first(
        &self,
        lease: SessionLease,
        change: PolicyChange,
        expected_voice: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
        permit: &mut Option<fc_storage::typed_command::FirstExecutionPermit>,
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
        let persisted = repository
            .complete_voice_policy(permit.take().ok_or(StorageError::CommandNotIssued)?, &actor);
        let snapshot = match persisted {
            Ok(fc_ports::command_window::TypedCommandOutcome::Completed(
                fc_ports::command_window::TypedCommandResult::VoicePolicy(snapshot),
            )) => snapshot,
            Ok(_) => {
                handoff.failed();
                self.wake();
                return Err(StorageError::Indeterminate.into());
            }
            Err(error) => {
                // The known policy endpoint is already closed. Do not use
                // conclude() to advance/adopt a different stop on uncertainty.
                handoff.failed();
                self.wake();
                return Err(typed_voice_error(error));
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
            device_observation: None,
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
