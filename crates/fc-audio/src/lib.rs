//! Bounded single-stream broadcast state machine. This crate never claims device output.
#![forbid(unsafe_code)]
use fc_domain::ids::PrincipalId;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Queued,
    Starting,
    Playing,
    Gap { until_ms: u64 },
    Stopping,
    Completed,
    Canceled,
    Failed,
    Expired,
    Interrupted,
}
impl State {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Canceled | Self::Failed | Self::Expired | Self::Interrupted
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub owner: PrincipalId,
    pub total: u8,
    pub completed: u8,
    pub state: State,
    pub generation: u64,
    pub latest_start_ms: u64,
    release_generation: Option<u64>,
    pending_terminal: Option<State>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    Full,
    OwnerLimit,
    RepeatOutOfRange,
    DuplicateId,
    InvalidId,
    Missing,
    Forbidden,
    StaleCompletion,
    InvalidState,
    ClockOverflow,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attempt {
    pub job_id: u64,
    pub generation: u64,
    pub repeat: u8,
}
/// Native cleanup completion must carry this exact generation. An ID alone cannot
/// distinguish a late release from a subsequent attempt after queue entries are drained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseToken {
    job_id: u64,
    generation: u64,
}
impl ReleaseToken {
    pub fn job_id(self) -> u64 {
        self.job_id
    }
    pub fn generation(self) -> u64 {
        self.generation
    }
}
#[derive(Default)]
pub struct BroadcastQueue {
    jobs: VecDeque<Job>,
    next_generation: u64,
}
impl BroadcastQueue {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn jobs(&self) -> impl Iterator<Item = &Job> {
        self.jobs.iter()
    }
    /// Admit only after the application has durably accepted and deduplicated commandId.
    /// In-memory terminal entries are returned by `drain_terminal` for bounded durable audit.
    pub fn enqueue_committed(
        &mut self,
        id: u64,
        owner: PrincipalId,
        total: u8,
        now_ms: u64,
    ) -> Result<(), QueueError> {
        if id == 0 {
            return Err(QueueError::InvalidId);
        }
        if !(1..=10).contains(&total) {
            return Err(QueueError::RepeatOutOfRange);
        }
        if self.jobs.iter().any(|j| j.id == id) {
            return Err(QueueError::DuplicateId);
        }
        if self.jobs.len() >= 4 {
            return Err(QueueError::Full);
        }
        if self
            .jobs
            .iter()
            .filter(|j| j.owner == owner && !j.state.terminal())
            .count()
            >= 2
        {
            return Err(QueueError::OwnerLimit);
        }
        let latest_start_ms = now_ms
            .checked_add(300_000)
            .ok_or(QueueError::ClockOverflow)?;
        self.jobs.push_back(Job {
            id,
            owner,
            total,
            completed: 0,
            state: State::Queued,
            generation: 0,
            latest_start_ms,
            release_generation: None,
            pending_terminal: None,
        });
        Ok(())
    }
    /// Called in the audio execution domain immediately before native submission. The
    /// caller must obtain a fresh HealthGate StartPermit in `authorize`, on EVERY repeat.
    pub fn prepare_next(
        &mut self,
        now_ms: u64,
        mut authorize: impl FnMut(&Job) -> bool,
    ) -> Result<Option<Attempt>, QueueError> {
        if self
            .jobs
            .iter()
            .any(|j| matches!(j.state, State::Starting | State::Playing | State::Stopping))
        {
            return Ok(None);
        }
        let Some(i) = self.jobs.iter().position(|j| !j.state.terminal()) else {
            return Ok(None);
        };
        let job = &mut self.jobs[i];
        match job.state {
            State::Queued if now_ms >= job.latest_start_ms => {
                job.state = State::Expired;
                return Ok(None);
            }
            State::Gap { until_ms } if now_ms < until_ms => return Ok(None),
            State::Queued | State::Gap { .. } => {}
            _ => return Ok(None),
        }
        if !authorize(job) {
            job.state = State::Failed;
            return Err(QueueError::Forbidden);
        }
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(QueueError::ClockOverflow)?;
        job.generation = self.next_generation;
        job.state = State::Starting;
        Ok(Some(Attempt {
            job_id: job.id,
            generation: job.generation,
            repeat: job.completed + 1,
        }))
    }
    fn matching(&mut self, a: Attempt) -> Result<&mut Job, QueueError> {
        self.jobs
            .iter_mut()
            .find(|j| {
                j.id == a.job_id && j.generation == a.generation && j.completed + 1 == a.repeat
            })
            .ok_or(QueueError::StaleCompletion)
    }
    /// Only call after successful waveOutWrite, never merely after obtaining a permit.
    pub fn submitted(&mut self, a: Attempt) -> Result<(), QueueError> {
        let j = self.matching(a)?;
        if j.state != State::Starting {
            return Err(QueueError::InvalidState);
        }
        j.state = State::Playing;
        Ok(())
    }
    /// All buffers in a repeat naturally returned; reset/cancel returns MUST NOT call this.
    pub fn naturally_completed(&mut self, a: Attempt, now_ms: u64) -> Result<(), QueueError> {
        let j = self.matching(a)?;
        if j.state != State::Playing {
            return Err(QueueError::InvalidState);
        }
        let next = if j.completed + 1 == j.total {
            State::Completed
        } else {
            State::Gap {
                until_ms: now_ms.checked_add(1000).ok_or(QueueError::ClockOverflow)?,
            }
        };
        j.completed += 1;
        j.state = next;
        Ok(())
    }
    fn begin_release(j: &mut Job, terminal: State) -> Option<ReleaseToken> {
        if j.state.terminal() {
            return None;
        }
        if matches!(j.state, State::Playing | State::Starting | State::Stopping) {
            let generation = *j.release_generation.get_or_insert(j.generation);
            j.pending_terminal.get_or_insert(terminal);
            j.generation = 0;
            j.state = State::Stopping;
            Some(ReleaseToken {
                job_id: j.id,
                generation,
            })
        } else {
            j.generation = 0;
            j.state = terminal;
            None
        }
    }
    /// After any device/submission failure, stop/reset and release all partially prepared
    /// resources. Even a Starting failure cannot admit another stream before confirmation.
    pub fn output_failed(&mut self, a: Attempt) -> Result<ReleaseToken, QueueError> {
        let j = self.matching(a)?;
        if !matches!(j.state, State::Starting | State::Playing) {
            return Err(QueueError::InvalidState);
        }
        Self::begin_release(j, State::Failed).ok_or(QueueError::InvalidState)
    }
    pub fn request_stop(
        &mut self,
        id: u64,
        actor: PrincipalId,
        global: bool,
    ) -> Result<Option<ReleaseToken>, QueueError> {
        let j = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(QueueError::Missing)?;
        if !global && actor != j.owner {
            return Err(QueueError::Forbidden);
        }
        Ok(Self::begin_release(j, State::Canceled))
    }
    pub fn confirm_released(&mut self, token: ReleaseToken) -> Result<(), QueueError> {
        let j = self
            .jobs
            .iter_mut()
            .find(|j| j.id == token.job_id && j.release_generation == Some(token.generation))
            .ok_or(QueueError::StaleCompletion)?;
        if j.state != State::Stopping {
            return Err(QueueError::InvalidState);
        }
        j.state = j.pending_terminal.take().ok_or(QueueError::InvalidState)?;
        j.release_generation = None;
        Ok(())
    }
    /// Lock/sleep/exit cancellation never restores a queue after restart.
    pub fn emergency_stop_all(&mut self) -> Vec<ReleaseToken> {
        self.jobs
            .iter_mut()
            .filter_map(|j| Self::begin_release(j, State::Canceled))
            .collect()
    }
    pub fn drain_terminal(&mut self) -> Vec<Job> {
        let mut out = Vec::new();
        self.jobs.retain(|j| {
            if j.state.terminal() {
                out.push(j.clone());
                false
            } else {
                true
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner(n: u64) -> PrincipalId {
        PrincipalId::new(n).unwrap()
    }
    #[test]
    fn total_three_means_three_and_one_second_gaps() {
        let mut q = BroadcastQueue::new();
        q.enqueue_committed(1, owner(1), 3, 0).unwrap();
        let a = q.prepare_next(0, |_| true).unwrap().unwrap();
        q.submitted(a).unwrap();
        q.naturally_completed(a, 500).unwrap();
        assert!(q.prepare_next(1499, |_| true).unwrap().is_none());
        let a = q.prepare_next(1500, |_| true).unwrap().unwrap();
        assert_eq!(a.repeat, 2);
        q.submitted(a).unwrap();
        q.naturally_completed(a, 2000).unwrap();
        let a = q.prepare_next(3000, |_| true).unwrap().unwrap();
        assert_eq!(a.repeat, 3);
        q.submitted(a).unwrap();
        q.naturally_completed(a, 3500).unwrap();
        let done = q.drain_terminal();
        assert_eq!(done[0].completed, 3);
        assert_eq!(done[0].state, State::Completed);
    }
    #[test]
    fn cancellation_done_is_never_counted_and_next_job_waits_for_release() {
        let mut q = BroadcastQueue::new();
        q.enqueue_committed(1, owner(1), 3, 0).unwrap();
        q.enqueue_committed(2, owner(2), 1, 0).unwrap();
        let a = q.prepare_next(0, |_| true).unwrap().unwrap();
        q.submitted(a).unwrap();
        let release = q.request_stop(1, owner(1), false).unwrap().unwrap();
        assert_eq!(
            q.naturally_completed(a, 1),
            Err(QueueError::StaleCompletion)
        );
        assert!(q.prepare_next(2, |_| true).unwrap().is_none());
        q.confirm_released(release).unwrap();
        assert_eq!(q.jobs().next().unwrap().completed, 0);
        assert_eq!(q.prepare_next(3, |_| true).unwrap().unwrap().job_id, 2);
    }
    #[test]
    fn quota_expiry_revocation_and_emergency_are_independent_of_desktop() {
        let mut q = BroadcastQueue::new();
        for id in 1..=2 {
            q.enqueue_committed(id, owner(1), 1, 0).unwrap();
        }
        assert_eq!(
            q.enqueue_committed(3, owner(1), 1, 0),
            Err(QueueError::OwnerLimit)
        );
        q.enqueue_committed(3, owner(2), 1, 0).unwrap();
        q.enqueue_committed(4, owner(2), 1, 0).unwrap();
        assert_eq!(
            q.enqueue_committed(5, owner(3), 1, 0),
            Err(QueueError::Full)
        );
        assert!(q.prepare_next(300000, |_| true).unwrap().is_none());
        q.emergency_stop_all();
        assert!(q.jobs().all(|j| j.state.terminal()));
        q.drain_terminal();
        q.enqueue_committed(8, owner(2), 3, 0).unwrap();
        let a = q.prepare_next(0, |_| true).unwrap().unwrap();
        q.submitted(a).unwrap();
        q.naturally_completed(a, 1).unwrap();
        assert_eq!(q.prepare_next(1001, |_| false), Err(QueueError::Forbidden));
    }
    #[test]
    fn late_cleanup_cannot_finish_a_reused_id_with_a_new_generation() {
        let mut q = BroadcastQueue::new();
        q.enqueue_committed(1, owner(1), 1, 0).unwrap();
        let a = q.prepare_next(0, |_| true).unwrap().unwrap();
        q.submitted(a).unwrap();
        let old = q.request_stop(1, owner(1), false).unwrap().unwrap();
        q.confirm_released(old).unwrap();
        q.drain_terminal();
        q.enqueue_committed(1, owner(1), 1, 10).unwrap();
        let b = q.prepare_next(10, |_| true).unwrap().unwrap();
        q.submitted(b).unwrap();
        let current = q.request_stop(1, owner(1), false).unwrap().unwrap();
        assert_ne!(old, current);
        assert_eq!(q.confirm_released(old), Err(QueueError::StaleCompletion));
        assert_eq!(q.jobs().next().unwrap().state, State::Stopping);
        q.confirm_released(current).unwrap();
    }
    #[test]
    fn submission_failure_waits_for_release_and_never_counts_as_a_repeat() {
        let mut q = BroadcastQueue::new();
        q.enqueue_committed(1, owner(1), 3, 0).unwrap();
        q.enqueue_committed(2, owner(2), 1, 0).unwrap();
        let a = q.prepare_next(0, |_| true).unwrap().unwrap();
        let release = q.output_failed(a).unwrap();
        assert_eq!(q.submitted(a), Err(QueueError::StaleCompletion));
        assert!(q.prepare_next(1, |_| true).unwrap().is_none());
        assert_eq!(q.request_stop(1, owner(1), false), Ok(Some(release)));
        q.confirm_released(release).unwrap();
        let failed = q.drain_terminal();
        assert_eq!(failed[0].state, State::Failed);
        assert_eq!(failed[0].completed, 0);
        assert_eq!(q.prepare_next(2, |_| true).unwrap().unwrap().job_id, 2);
    }
    #[test]
    fn later_jobs_do_not_start_during_repeat_gap_and_emergency_cancels_all() {
        let mut q = BroadcastQueue::new();
        q.enqueue_committed(1, owner(1), 2, 0).unwrap();
        q.enqueue_committed(2, owner(2), 1, 0).unwrap();
        let a = q.prepare_next(0, |_| true).unwrap().unwrap();
        q.submitted(a).unwrap();
        q.naturally_completed(a, 20).unwrap();
        assert!(q.prepare_next(1019, |_| true).unwrap().is_none());
        let b = q.prepare_next(1020, |_| true).unwrap().unwrap();
        assert_eq!(b.job_id, 1);
        assert_eq!(b.repeat, 2);
        q.submitted(b).unwrap();
        let release = q.emergency_stop_all();
        assert_eq!(release.len(), 1);
        assert_eq!(
            q.naturally_completed(b, 1021),
            Err(QueueError::StaleCompletion)
        );
        assert!(q.prepare_next(1022, |_| true).unwrap().is_none());
        q.confirm_released(release[0]).unwrap();
        assert!(q.jobs().all(|j| j.state.terminal()));
    }
}
