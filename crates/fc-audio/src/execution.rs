//! Durable inventory is the queue. This coordinator stores only native ownership
//! and a live monotonic deadline; it never restores a process-local playing task.
use fc_domain::voice::*;
use fc_ports::{audio::*, voice::VoiceRepositoryError};

/// Trusted composition owns storage/authority/source preparation. Authorization
/// must compare the current durable job ID/generation/state as well as current
/// health, original session, class and teaching assignment; a cloned actor is
/// insufficient. Calls execute in the audio domain, never the UI or writer.
pub trait VoiceExecutionPort<S> {
    /// The SAME live monotonic clock used by tick; re-read after blocking source
    /// validation/device waits and immediately before every native submission.
    fn monotonic_millis(&self) -> u64;
    /// Trusted current civil clock, not a client timestamp; queried again after
    /// source validation/open so the first-start deadline cannot become stale.
    fn civil_seconds(&self) -> i64;
    fn inventory(&mut self) -> Result<VoiceSnapshot, VoiceRepositoryError>;
    fn drive(&mut self, update: VoiceDriverUpdate) -> Result<VoiceJob, VoiceRepositoryError>;
    fn authorize(&mut self, job: &VoiceJob) -> AudioAuthorization;
    fn source(&mut self, asset: &VoiceAsset) -> Result<S, VoiceRepositoryError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionError {
    Repository(VoiceRepositoryError),
    Output(AudioOutputError),
    Invariant,
    ClockRegression,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionStatus {
    Idle,
    Active {
        job_id: VoiceJobId,
        generation: u64,
    },
    Waiting {
        job_id: VoiceJobId,
        until_civil: i64,
    },
    Retained {
        job_id: VoiceJobId,
        generation: u64,
    },
    /// No further start until process restart and exclusive-instance recovery.
    Halted,
}
struct Ownership {
    job_id: VoiceJobId,
    generation: u64,
    /// True only after the output port confirms actual native release.
    released: bool,
}
struct Budget {
    job_id: VoiceJobId,
    deadline_ms: u64,
    gap_until_ms: Option<u64>,
    total_ms: u64,
}
pub struct VoiceExecutor<O: AudioOutputPort> {
    output: O,
    active: Option<Ownership>,
    budget: Option<Budget>,
    last_clock: Option<(i64, u64)>,
    halted: bool,
}
impl<O: AudioOutputPort> VoiceExecutor<O> {
    pub fn new(output: O) -> Self {
        Self {
            output,
            active: None,
            budget: None,
            last_clock: None,
            halted: false,
        }
    }
    pub fn has_native_ownership(&self) -> bool {
        self.active.as_ref().is_some_and(|a| !a.released)
    }
    pub fn halted(&self) -> bool {
        self.halted
    }
    fn update<P: VoiceExecutionPort<O::Source>>(
        port: &mut P,
        job: &VoiceJob,
        action: VoiceDriverAction,
    ) -> Result<VoiceJob, ExecutionError> {
        let generation = if action == VoiceDriverAction::Released {
            job.release_generation.ok_or(ExecutionError::Invariant)?
        } else {
            job.generation
        };
        port.drive(VoiceDriverUpdate {
            job_id: job.id,
            expected_revision: job.revision,
            generation,
            action,
        })
        .map_err(ExecutionError::Repository)
    }
    /// Health/storage uncertainty stops the device immediately and latches starts
    /// closed. Subsequent ticks may settle known release using fresh inventory,
    /// but cannot play anything else or count a late DONE as natural completion.
    fn fail_closed(&mut self) {
        self.halted = true;
        if let Some(active) = &mut self.active {
            if !active.released {
                let released = |progress| {
                    matches!(progress,
                    Ok(AudioOutputProgress::Released { generation }) if generation == active.generation)
                        || matches!(progress, Ok(AudioOutputProgress::Idle))
                };
                if released(self.output.stop(active.generation))
                    || released(self.output.poll(0, &mut || AudioAuthorization::Revoked))
                {
                    active.released = true;
                }
            }
        }
    }
    fn release<P: VoiceExecutionPort<O::Source>>(
        &mut self,
        port: &mut P,
        job: &VoiceJob,
    ) -> Result<VoiceJob, ExecutionError> {
        let active = self.active.as_ref().ok_or(ExecutionError::Invariant)?;
        if !active.released
            || active.job_id != job.id
            || job.release_generation != Some(active.generation)
        {
            return Err(ExecutionError::Invariant);
        }
        let result = Self::update(port, job, VoiceDriverAction::Released)?;
        self.active = None;
        if result.state.terminal() {
            self.budget = None;
        }
        Ok(result)
    }
    fn stop_job<P: VoiceExecutionPort<O::Source>>(
        &mut self,
        port: &mut P,
        mut job: VoiceJob,
        failure: Option<VoiceFailure>,
        wait_ms: u32,
    ) -> Result<ExecutionStatus, ExecutionError> {
        if let Some(failure) = failure {
            if job.state != VoiceState::Stopping && !job.state.terminal() {
                // Durable generation invalidation precedes reset. On an unknown
                // storage result tick's outer error path still stops fail-closed.
                job = Self::update(port, &job, VoiceDriverAction::Fail(failure))?;
            }
        }
        let active = self.active.as_mut().ok_or(ExecutionError::Invariant)?;
        if !active.released {
            let mut progress = self
                .output
                .stop(active.generation)
                .map_err(ExecutionError::Output)?;
            if matches!(progress, AudioOutputProgress::Playing { .. }) {
                progress = self
                    .output
                    .poll(wait_ms.min(50), &mut || AudioAuthorization::Revoked)
                    .map_err(ExecutionError::Output)?;
            }
            match progress {
                AudioOutputProgress::Released { generation } if generation == active.generation => {
                    active.released = true
                }
                AudioOutputProgress::Idle => active.released = true,
                AudioOutputProgress::Retained { generation } if generation == active.generation => {
                    self.halted = true;
                    return Ok(ExecutionStatus::Retained {
                        job_id: active.job_id,
                        generation,
                    });
                }
                AudioOutputProgress::Playing { generation } if generation == active.generation => {
                    return Ok(ExecutionStatus::Active {
                        job_id: active.job_id,
                        generation,
                    });
                }
                _ => return Err(ExecutionError::Invariant),
            }
        }
        self.release(port, &job)?;
        Ok(if self.halted {
            ExecutionStatus::Halted
        } else {
            ExecutionStatus::Idle
        })
    }
    /// Each call performs bounded work and waits at most 50 ms for the output
    /// event. The same audio domain calls it during playback, gaps and shutdown.
    pub fn tick<P: VoiceExecutionPort<O::Source>>(
        &mut self,
        port: &mut P,
        now_civil: i64,
        monotonic_ms: u64,
        wait_ms: u32,
    ) -> Result<ExecutionStatus, ExecutionError> {
        let was_halted = self.halted;
        if self
            .last_clock
            .is_some_and(|(civil, mono)| now_civil < civil || monotonic_ms < mono)
        {
            self.fail_closed();
            // The first rollback reports the fault. Once halted, still perform
            // the independent cleanup path on subsequent ticks; do not trap
            // durable Stopping forever behind the same regressed clock.
            if !was_halted {
                return Err(ExecutionError::ClockRegression);
            }
        }
        self.last_clock = Some(
            self.last_clock
                .map_or((now_civil, monotonic_ms), |(civil, mono)| {
                    (civil.max(now_civil), mono.max(monotonic_ms))
                }),
        );
        let result = self.tick_inner(port, now_civil, monotonic_ms, wait_ms.min(50));
        let observed = port.monotonic_millis();
        let observed_civil = port.civil_seconds();
        if let Some((civil, _)) = self.last_clock.as_mut() {
            *civil = (*civil).max(observed_civil);
        }
        if let Some((_, mono)) = self.last_clock.as_mut() {
            *mono = (*mono).max(observed);
        }
        if result.is_err() {
            self.fail_closed();
        }
        result
    }
    fn tick_inner<P: VoiceExecutionPort<O::Source>>(
        &mut self,
        port: &mut P,
        now: i64,
        mono: u64,
        wait_ms: u32,
    ) -> Result<ExecutionStatus, ExecutionError> {
        let snapshot = port.inventory().map_err(ExecutionError::Repository)?;
        snapshot.validate().map_err(|_| ExecutionError::Invariant)?;
        let fresh_civil = port.civil_seconds();
        if fresh_civil < now && !self.halted {
            return Err(ExecutionError::ClockRegression);
        }
        let now = fresh_civil;
        let fresh_mono = port.monotonic_millis();
        if fresh_mono < mono && !self.halted {
            return Err(ExecutionError::ClockRegression);
        }
        let mono = fresh_mono;
        if let Some(active) = self.active.as_ref() {
            let job = snapshot
                .jobs
                .iter()
                .find(|j| j.id == active.job_id)
                .cloned()
                .ok_or(ExecutionError::Invariant)?;
            if job.release_generation != Some(active.generation) {
                // A successful Released commit whose response was lost is proven
                // only by current inventory AND already confirmed native release.
                if active.released && job.release_generation.is_none() {
                    self.active = None;
                    self.budget = None;
                    return Ok(ExecutionStatus::Halted);
                }
                return Err(ExecutionError::Invariant);
            }
            if self.halted {
                return self.stop_job(port, job, Some(VoiceFailure::Storage), wait_ms);
            }
            if job.state == VoiceState::Stopping || job.generation != active.generation {
                return self.stop_job(port, job, None, wait_ms);
            }
            if !snapshot.enabled {
                return self.stop_job(port, job, Some(VoiceFailure::Disabled), wait_ms);
            }
            if job.state == VoiceState::Starting
                && job.first_started_at.is_none()
                && now >= job.latest_start_at
            {
                let job = Self::update(port, &job, VoiceDriverAction::ExpireBeforeWrite)?;
                return self.stop_job(port, job, None, 0);
            }
            if port.authorize(&job) == AudioAuthorization::Revoked {
                return self.stop_job(port, job, Some(VoiceFailure::Revoked), wait_ms);
            }
            if self
                .budget
                .as_ref()
                .is_none_or(|b| b.job_id != job.id || mono >= b.deadline_ms)
            {
                return self.stop_job(port, job, Some(VoiceFailure::Timeout), wait_ms);
            }
            if !matches!(job.state, VoiceState::Playing | VoiceState::Starting) {
                return Err(ExecutionError::Invariant);
            }
            let deadline = self
                .budget
                .as_ref()
                .ok_or(ExecutionError::Invariant)?
                .deadline_ms;
            let progress = match self.output.poll(wait_ms, &mut || {
                if port.monotonic_millis() >= deadline
                    || job.first_started_at.is_none() && port.civil_seconds() >= job.latest_start_at
                {
                    AudioAuthorization::Revoked
                } else {
                    port.authorize(&job)
                }
            }) {
                Ok(progress) => progress,
                Err(error) => {
                    if error == AudioOutputError::Unauthorized {
                        if job.first_started_at.is_none()
                            && port.civil_seconds() >= job.latest_start_at
                        {
                            let job =
                                Self::update(port, &job, VoiceDriverAction::ExpireBeforeWrite)?;
                            return self.stop_job(port, job, None, 0);
                        }
                        return self.stop_job(port, job, Some(VoiceFailure::Revoked), 0);
                    }
                    self.stop_job(port, job, Some(VoiceFailure::Device), 0)?;
                    return Err(ExecutionError::Output(error));
                }
            };
            let after_wait = port.monotonic_millis();
            if after_wait < mono {
                return Err(ExecutionError::ClockRegression);
            }
            if after_wait >= deadline {
                return self.stop_job(port, job, Some(VoiceFailure::Timeout), 0);
            }
            match progress {
                AudioOutputProgress::Deferred { generation } if generation == active.generation => {
                    Ok(ExecutionStatus::Waiting {
                        job_id: job.id,
                        until_civil: now,
                    })
                }
                AudioOutputProgress::FirstWrite { generation }
                    if generation == active.generation && job.state == VoiceState::Starting =>
                {
                    if job.first_started_at.is_none() {
                        let budget = self.budget.as_mut().ok_or(ExecutionError::Invariant)?;
                        budget.deadline_ms = after_wait
                            .checked_add(budget.total_ms)
                            .ok_or(ExecutionError::Invariant)?;
                    }
                    let job = Self::update(port, &job, VoiceDriverAction::FirstWrite)?;
                    if job.state == VoiceState::Stopping {
                        return self.stop_job(port, job, None, 0);
                    }
                    Ok(ExecutionStatus::Active {
                        job_id: job.id,
                        generation,
                    })
                }
                AudioOutputProgress::Playing { generation } if generation == active.generation => {
                    Ok(ExecutionStatus::Active {
                        job_id: job.id,
                        generation,
                    })
                }
                AudioOutputProgress::NaturallyCompleted { generation }
                    if generation == active.generation =>
                {
                    self.active
                        .as_mut()
                        .ok_or(ExecutionError::Invariant)?
                        .released = true;
                    let gap_until = after_wait
                        .checked_add(1000)
                        .ok_or(ExecutionError::Invariant)?;
                    let job = Self::update(port, &job, VoiceDriverAction::NaturalComplete)?;
                    if job.state == VoiceState::Gap {
                        self.budget
                            .as_mut()
                            .ok_or(ExecutionError::Invariant)?
                            .gap_until_ms = Some(gap_until);
                    }
                    let job = self.release(port, &job)?;
                    Ok(if job.state == VoiceState::Gap {
                        ExecutionStatus::Waiting {
                            job_id: job.id,
                            until_civil: job.gap_until.ok_or(ExecutionError::Invariant)?,
                        }
                    } else {
                        ExecutionStatus::Idle
                    })
                }
                AudioOutputProgress::Released { generation } if generation == active.generation => {
                    self.active
                        .as_mut()
                        .ok_or(ExecutionError::Invariant)?
                        .released = true;
                    self.stop_job(port, job, Some(VoiceFailure::Revoked), 0)
                }
                AudioOutputProgress::Retained { generation } if generation == active.generation => {
                    self.halted = true;
                    self.stop_job(port, job, Some(VoiceFailure::Device), 0)
                }
                _ => Err(ExecutionError::Invariant),
            }
        } else {
            if self.halted {
                return Ok(ExecutionStatus::Halted);
            }
            let Some(job) = snapshot
                .jobs
                .iter()
                .filter(|j| !j.state.terminal())
                .min_by_key(|j| j.sequence)
                .cloned()
            else {
                self.budget = None;
                return Ok(ExecutionStatus::Idle);
            };
            if !matches!(job.state, VoiceState::Queued | VoiceState::Gap)
                || job.release_generation.is_some()
            {
                // Do not synthesize release after process recovery; only the
                // exclusive-instance startup recovery may interrupt old ownership.
                return Err(ExecutionError::Invariant);
            }
            if job.state == VoiceState::Gap
                && self.budget.as_ref().is_none_or(|b| b.job_id != job.id)
            {
                return Err(ExecutionError::Invariant);
            }
            if job.state == VoiceState::Queued
                && self.budget.as_ref().is_some_and(|b| b.job_id != job.id)
            {
                self.budget = None;
            }
            if job.first_started_at.is_none() && now >= job.latest_start_at {
                Self::update(port, &job, VoiceDriverAction::ExpireBeforeWrite)?;
                self.budget = None;
                return Ok(ExecutionStatus::Idle);
            }
            let authorization = port.authorize(&job);
            if self.budget.as_ref().is_some_and(|b| mono >= b.deadline_ms) {
                Self::update(port, &job, VoiceDriverAction::Fail(VoiceFailure::Timeout))?;
                self.budget = None;
                return Ok(ExecutionStatus::Idle);
            }
            if authorization == AudioAuthorization::Deferred && snapshot.enabled {
                return Ok(ExecutionStatus::Waiting {
                    job_id: job.id,
                    until_civil: now,
                });
            }
            if !snapshot.enabled || authorization == AudioAuthorization::Revoked {
                Self::update(
                    port,
                    &job,
                    VoiceDriverAction::Fail(if snapshot.enabled {
                        VoiceFailure::Revoked
                    } else {
                        VoiceFailure::Disabled
                    }),
                )?;
                self.budget = None;
                return Ok(ExecutionStatus::Idle);
            }
            if let Some(until) = job.gap_until {
                if now < until {
                    return Ok(ExecutionStatus::Waiting {
                        job_id: job.id,
                        until_civil: until,
                    });
                }
                let monotonic_gap = self
                    .budget
                    .as_ref()
                    .and_then(|b| b.gap_until_ms)
                    .ok_or(ExecutionError::Invariant)?;
                if mono < monotonic_gap {
                    return Ok(ExecutionStatus::Waiting {
                        job_id: job.id,
                        until_civil: until,
                    });
                }
            }
            if self.budget.as_ref().is_some_and(|b| mono >= b.deadline_ms) {
                Self::update(port, &job, VoiceDriverAction::Fail(VoiceFailure::Timeout))?;
                self.budget = None;
                return Ok(ExecutionStatus::Idle);
            }
            let asset = snapshot
                .assets
                .iter()
                .find(|a| Some(a.id) == job.asset_id)
                .ok_or(ExecutionError::Invariant)?;
            let duration = asset
                .pcm
                .as_ref()
                .ok_or(ExecutionError::Invariant)?
                .duration_millis();
            let job = Self::update(port, &job, VoiceDriverAction::Start)?;
            if job.state == VoiceState::Expired {
                self.budget = None;
                return Ok(ExecutionStatus::Idle);
            }
            if job.state != VoiceState::Starting {
                return Err(ExecutionError::Invariant);
            }
            let generation = job.generation;
            self.active = Some(Ownership {
                job_id: job.id,
                generation,
                released: false,
            });
            let total = duration
                .checked_mul(u64::from(job.repeat_total))
                .and_then(|t| t.checked_add(u64::from(job.repeat_total - 1) * 1000))
                .and_then(|t| t.checked_add(5000))
                .ok_or(ExecutionError::Invariant)?;
            let first_attempt = self.budget.is_none();
            if first_attempt {
                self.budget = Some(Budget {
                    job_id: job.id,
                    // Preparation is separately bounded. Successful first Write
                    // starts the full playback budget using a fresh clock below.
                    deadline_ms: mono.checked_add(5000).ok_or(ExecutionError::Invariant)?,
                    gap_until_ms: None,
                    total_ms: total,
                });
            }
            if let Some(budget) = self.budget.as_mut() {
                budget.gap_until_ms = None;
            }
            let source = port.source(asset);
            if port.civil_seconds() < now {
                return Err(ExecutionError::ClockRegression);
            }
            if job.first_started_at.is_none() && port.civil_seconds() >= job.latest_start_at {
                let job = Self::update(port, &job, VoiceDriverAction::ExpireBeforeWrite)?;
                return self.stop_job(port, job, None, 0);
            }
            let source = match source {
                Ok(source) => source,
                Err(_) => return self.stop_job(port, job, Some(VoiceFailure::Storage), 0),
            };
            let deadline = self
                .budget
                .as_ref()
                .ok_or(ExecutionError::Invariant)?
                .deadline_ms;
            if port.monotonic_millis() < mono {
                return Err(ExecutionError::ClockRegression);
            }
            if port.monotonic_millis() >= deadline {
                return self.stop_job(port, job, Some(VoiceFailure::Timeout), 0);
            }
            match self.output.start(source, generation, &mut || {
                if port.monotonic_millis() >= deadline
                    || job.first_started_at.is_none() && port.civil_seconds() >= job.latest_start_at
                {
                    AudioAuthorization::Revoked
                } else {
                    port.authorize(&job)
                }
            }) {
                Ok(()) => {
                    if first_attempt {
                        self.budget
                            .as_mut()
                            .ok_or(ExecutionError::Invariant)?
                            .deadline_ms = port
                            .monotonic_millis()
                            .checked_add(total)
                            .ok_or(ExecutionError::Invariant)?;
                    }
                    // start Ok is the first successful native Write, never mere
                    // device open. This commit precedes every poll/natural result.
                    let job = Self::update(port, &job, VoiceDriverAction::FirstWrite)?;
                    if job.state == VoiceState::Stopping {
                        return self.stop_job(port, job, None, 0);
                    }
                    if job.state != VoiceState::Playing {
                        return Err(ExecutionError::Invariant);
                    }
                    Ok(ExecutionStatus::Active {
                        job_id: job.id,
                        generation,
                    })
                }
                Err(error) => {
                    if error == AudioOutputError::Deferred {
                        return Ok(ExecutionStatus::Waiting {
                            job_id: job.id,
                            until_civil: now,
                        });
                    }
                    if error == AudioOutputError::Unauthorized
                        && job.first_started_at.is_none()
                        && port.civil_seconds() >= job.latest_start_at
                    {
                        let job = Self::update(port, &job, VoiceDriverAction::ExpireBeforeWrite)?;
                        return self.stop_job(port, job, None, 0);
                    }
                    let failure = if error == AudioOutputError::Unauthorized {
                        if port.monotonic_millis() >= deadline {
                            VoiceFailure::Timeout
                        } else {
                            VoiceFailure::Revoked
                        }
                    } else {
                        VoiceFailure::Device
                    };
                    let status = self.stop_job(port, job, Some(failure), 0)?;
                    if error == AudioOutputError::Unauthorized {
                        Ok(status)
                    } else {
                        Err(ExecutionError::Output(error))
                    }
                }
            }
        }
    }
    /// The caller first persists emergency interruption using its independent
    /// control path. This latches off new starts even if that persistence fails;
    /// follow tick with fresh inventory to settle only actual native release.
    pub fn stop_accepting(&mut self) {
        self.fail_closed();
    }
}

#[cfg(test)]
mod tests;
