use super::*;
use fc_domain::{auth::*, ids::*, time::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Default)]
struct Device {
    generation: Option<u64>,
    starts: usize,
    stops: usize,
    natural: bool,
    hold_release: bool,
    error: bool,
    after_write_cancel: bool,
    unavailable: bool,
    wait_advance_ms: u64,
    open_advance_ms: u64,
    open_advance_civil: i64,
    defer_start: bool,
    pending: bool,
}
struct Output {
    device: Rc<RefCell<Device>>,
    storage: Rc<RefCell<VoiceSnapshot>>,
    monotonic: Rc<Cell<u64>>,
    civil: Rc<Cell<i64>>,
}
impl AudioOutputPort for Output {
    type Source = ();
    fn start(
        &mut self,
        _: (),
        generation: u64,
        auth: &mut dyn FnMut() -> AudioAuthorization,
    ) -> Result<(), AudioOutputError> {
        self.monotonic
            .set(self.monotonic.get() + self.device.borrow().open_advance_ms);
        self.civil
            .set(self.civil.get() + self.device.borrow().open_advance_civil);
        let decision = auth();
        if decision == AudioAuthorization::Revoked {
            return Err(AudioOutputError::Unauthorized);
        }
        let mut d = self.device.borrow_mut();
        assert!(d.generation.is_none());
        if d.unavailable {
            return Err(AudioOutputError::DeviceUnavailable);
        }
        d.generation = Some(generation);
        if decision == AudioAuthorization::Deferred || d.defer_start {
            d.pending = true;
            return Err(AudioOutputError::Deferred);
        }
        d.pending = false;
        d.starts += 1;
        if d.after_write_cancel {
            let mut state = self.storage.borrow_mut();
            let job = state.jobs[0].clone();
            state
                .cancel(job.id, job.revision, CommandId::new(90).unwrap(), 1)
                .unwrap();
        }
        Ok(())
    }
    fn poll(
        &mut self,
        _: u32,
        auth: &mut dyn FnMut() -> AudioAuthorization,
    ) -> Result<AudioOutputProgress, AudioOutputError> {
        let mut d = self.device.borrow_mut();
        self.monotonic.set(self.monotonic.get() + d.wait_advance_ms);
        let Some(generation) = d.generation else {
            return Ok(AudioOutputProgress::Idle);
        };
        if d.error {
            return Err(AudioOutputError::Driver(1));
        }
        let decision = auth();
        if decision == AudioAuthorization::Deferred {
            return Ok(AudioOutputProgress::Deferred { generation });
        }
        if decision == AudioAuthorization::Revoked {
            if d.hold_release {
                return Ok(AudioOutputProgress::Playing { generation });
            }
            d.generation = None;
            return Ok(AudioOutputProgress::Released { generation });
        }
        if d.pending {
            d.pending = false;
            d.starts += 1;
            return Ok(AudioOutputProgress::FirstWrite { generation });
        }
        if d.natural {
            d.natural = false;
            d.generation = None;
            Ok(AudioOutputProgress::NaturallyCompleted { generation })
        } else {
            Ok(AudioOutputProgress::Playing { generation })
        }
    }
    fn stop(&mut self, generation: u64) -> Result<AudioOutputProgress, AudioOutputError> {
        let mut d = self.device.borrow_mut();
        d.stops += 1;
        if let Some(active) = d.generation {
            assert_eq!(active, generation);
            if d.hold_release {
                return Ok(AudioOutputProgress::Playing { generation });
            }
            d.generation = None;
            d.pending = false;
            Ok(AudioOutputProgress::Released { generation })
        } else {
            Ok(AudioOutputProgress::Idle)
        }
    }
}
struct Port {
    state: Rc<RefCell<VoiceSnapshot>>,
    civil: Rc<Cell<i64>>,
    allowed: bool,
    read_failure: bool,
    first_write_failure: bool,
    released_response_lost: bool,
    cancel_before_natural: bool,
    source_failure: bool,
    actions: Vec<VoiceDriverAction>,
    sources: usize,
    authorizations: usize,
    monotonic: Rc<Cell<u64>>,
    source_advance_ms: u64,
    source_advance_civil: i64,
    deferred: bool,
}
impl VoiceExecutionPort<()> for Port {
    fn monotonic_millis(&self) -> u64 {
        self.monotonic.get()
    }
    fn civil_seconds(&self) -> i64 {
        self.civil.get()
    }
    fn inventory(&mut self) -> Result<VoiceSnapshot, VoiceRepositoryError> {
        if self.read_failure {
            Err(VoiceRepositoryError::Unavailable)
        } else {
            Ok(self.state.borrow().clone())
        }
    }
    fn drive(&mut self, update: VoiceDriverUpdate) -> Result<VoiceJob, VoiceRepositoryError> {
        if update.action == VoiceDriverAction::FirstWrite && self.first_write_failure {
            return Err(VoiceRepositoryError::Unavailable);
        }
        if update.action == VoiceDriverAction::NaturalComplete && self.cancel_before_natural {
            self.cancel_before_natural = false;
            let mut s = self.state.borrow_mut();
            let j = s.jobs[0].clone();
            s.cancel(
                j.id,
                j.revision,
                CommandId::new(91).unwrap(),
                self.civil.get(),
            )
            .unwrap();
        }
        let now = if matches!(
            update.action,
            VoiceDriverAction::Fail(_) | VoiceDriverAction::Released | VoiceDriverAction::Interrupt
        ) {
            self.civil
                .get()
                .max(self.state.borrow().observed_at.unwrap_or(self.civil.get()))
        } else {
            self.civil.get()
        };
        let job = self
            .state
            .borrow_mut()
            .drive(&update, now)
            .map_err(VoiceRepositoryError::Domain)?;
        self.actions.push(update.action);
        if update.action == VoiceDriverAction::Released && self.released_response_lost {
            self.released_response_lost = false;
            return Err(VoiceRepositoryError::Unavailable);
        }
        Ok(job)
    }
    fn authorize(&mut self, job: &VoiceJob) -> AudioAuthorization {
        self.authorizations += 1;
        if !(self.allowed
            && self.state.borrow().jobs.iter().any(|j| j == job)
            && (job.first_started_at.is_some() || self.civil.get() < job.latest_start_at))
        {
            AudioAuthorization::Revoked
        } else if self.deferred {
            AudioAuthorization::Deferred
        } else {
            AudioAuthorization::Authorized
        }
    }
    fn source(&mut self, _: &VoiceAsset) -> Result<(), VoiceRepositoryError> {
        self.sources += 1;
        self.monotonic
            .set(self.monotonic.get() + self.source_advance_ms);
        self.civil.set(self.civil.get() + self.source_advance_civil);
        if self.source_failure {
            Err(VoiceRepositoryError::Domain(VoiceError::InvalidWav))
        } else {
            Ok(())
        }
    }
}
fn fixture(total: u8, frames: u32) -> (VoiceExecutor<Output>, Port, Rc<RefCell<Device>>) {
    let actor = ActorContext {
        principal_id: PrincipalId::new(1).unwrap(),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 0).unwrap(),
        origin: TrustedOrigin::Web,
    };
    let mut state = VoiceSnapshot::default();
    let aid = VoiceAssetId::new(1).unwrap();
    let asset = state
        .reserve(aid, &actor, ClassId::new(1).unwrap(), 0)
        .unwrap();
    let asset = state
        .publish(
            aid,
            asset.revision,
            VoicePcmMetadata {
                byte_length: 44 + u64::from(frames) * 2,
                frame_count: frames,
                sha256: [1; 32],
            },
            0,
        )
        .unwrap();
    state
        .submit(
            CommandId::new(1).unwrap(),
            aid,
            asset.revision,
            total,
            &actor,
            0,
        )
        .unwrap();
    let state = Rc::new(RefCell::new(state));
    let device = Rc::new(RefCell::new(Device::default()));
    let monotonic = Rc::new(Cell::new(0));
    let civil = Rc::new(Cell::new(0));
    let output = Output {
        device: device.clone(),
        storage: state.clone(),
        monotonic: monotonic.clone(),
        civil: civil.clone(),
    };
    let port = Port {
        state,
        civil,
        allowed: true,
        read_failure: false,
        first_write_failure: false,
        released_response_lost: false,
        cancel_before_natural: false,
        source_failure: false,
        actions: Vec::new(),
        sources: 0,
        authorizations: 0,
        monotonic,
        source_advance_ms: 0,
        source_advance_civil: 0,
        deferred: false,
    };
    (VoiceExecutor::new(output), port, device)
}
fn tick(
    e: &mut VoiceExecutor<Output>,
    p: &mut Port,
    seconds: i64,
) -> Result<ExecutionStatus, ExecutionError> {
    p.civil.set(seconds);
    p.monotonic.set(seconds as u64 * 1000);
    e.tick(p, seconds, seconds as u64 * 1000, 0)
}
fn cancel(p: &mut Port, now: i64) {
    let job = p.state.borrow().jobs[0].clone();
    p.state
        .borrow_mut()
        .cancel(job.id, job.revision, CommandId::new(80).unwrap(), now)
        .unwrap();
}
#[test]
fn durable_three_repeats_have_first_write_before_natural_and_release_before_next() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    for n in 0..3 {
        tick(&mut e, &mut p, n * 2).unwrap();
        assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Playing);
        d.borrow_mut().natural = true;
        tick(&mut e, &mut p, n * 2 + 1).unwrap();
        assert!(!e.has_native_ownership());
        assert!(p.state.borrow().jobs[0].release_generation.is_none());
        if n < 2 {
            assert!(matches!(
                tick(&mut e, &mut p, n * 2 + 1).unwrap(),
                ExecutionStatus::Waiting { .. }
            ));
        }
    }
    let s = p.state.borrow();
    assert_eq!(s.jobs[0].state, VoiceState::Completed);
    assert_eq!(s.jobs[0].completed_repeats, 3);
    assert_eq!(d.borrow().starts, 3);
    assert_eq!(p.sources, 3);
    assert_eq!(
        p.actions,
        [
            VoiceDriverAction::Start,
            VoiceDriverAction::FirstWrite,
            VoiceDriverAction::NaturalComplete,
            VoiceDriverAction::Released
        ]
        .repeat(3)
    );
}
#[test]
fn updating_defers_queued_first_write_and_natural_without_reopening_or_counting() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    p.deferred = true;
    tick(&mut e, &mut p, 0).unwrap();
    assert_eq!(p.sources, 0);
    assert!(p.actions.is_empty());
    p.deferred = false;
    d.borrow_mut().defer_start = true;
    tick(&mut e, &mut p, 0).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Starting);
    assert_eq!(d.borrow().starts, 0);
    assert_eq!(p.sources, 1);
    p.deferred = true;
    tick(&mut e, &mut p, 1).unwrap();
    assert_eq!(d.borrow().stops, 0);
    p.deferred = false;
    d.borrow_mut().defer_start = false;
    tick(&mut e, &mut p, 2).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Playing);
    assert_eq!(d.borrow().starts, 1);
    assert_eq!(p.sources, 1);
    d.borrow_mut().natural = true;
    p.deferred = true;
    tick(&mut e, &mut p, 3).unwrap();
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
    assert_eq!(d.borrow().stops, 0);
    p.deferred = false;
    tick(&mut e, &mut p, 4).unwrap();
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 1);
    assert_eq!(
        p.actions
            .iter()
            .filter(|a| **a == VoiceDriverAction::FirstWrite)
            .count(),
        1
    );
    let (mut e, mut p, d) = fixture(1, 16_000);
    d.borrow_mut().defer_start = true;
    tick(&mut e, &mut p, 299).unwrap();
    p.deferred = true;
    tick(&mut e, &mut p, 300).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Expired);
    assert_eq!(d.borrow().starts, 0);
    assert!(!p.actions.contains(&VoiceDriverAction::FirstWrite));
}
#[test]
fn ten_max_duration_repeats_are_not_expired_at_the_initial_five_minute_deadline() {
    let (mut e, mut p, d) = fixture(10, 960_000);
    for n in 0..10 {
        tick(&mut e, &mut p, n * 61).unwrap();
        d.borrow_mut().natural = true;
        tick(&mut e, &mut p, n * 61 + 60).unwrap();
    }
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 10);
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Completed);
    assert_eq!(d.borrow().starts, 10);
}
#[test]
fn crossing_a_civil_second_does_not_shortcut_the_full_monotonic_repeat_gap() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    p.civil.set(0);
    p.monotonic.set(0);
    e.tick(&mut p, 0, 0, 0).unwrap();
    d.borrow_mut().natural = true;
    p.monotonic.set(900);
    e.tick(&mut p, 0, 900, 0).unwrap();
    assert_eq!(d.borrow().starts, 1);
    p.civil.set(1);
    p.monotonic.set(950);
    assert!(matches!(
        e.tick(&mut p, 1, 950, 0).unwrap(),
        ExecutionStatus::Waiting { .. }
    ));
    assert_eq!(d.borrow().starts, 1);
    p.civil.set(2);
    p.monotonic.set(1900);
    assert!(matches!(
        e.tick(&mut p, 2, 1900, 0).unwrap(),
        ExecutionStatus::Active { .. }
    ));
    assert_eq!(d.borrow().starts, 2);
}
#[test]
fn wait_return_time_starts_gap_and_expired_wait_cannot_count_completion() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    d.borrow_mut().natural = true;
    d.borrow_mut().wait_advance_ms = 50;
    p.monotonic.set(900);
    e.tick(&mut p, 0, 900, 50).unwrap();
    d.borrow_mut().wait_advance_ms = 0;
    p.civil.set(1);
    p.monotonic.set(1900);
    assert!(matches!(
        e.tick(&mut p, 1, 1900, 0).unwrap(),
        ExecutionStatus::Waiting { .. }
    ));
    assert_eq!(d.borrow().starts, 1);
    p.civil.set(2);
    p.monotonic.set(1950);
    e.tick(&mut p, 2, 1950, 0).unwrap();
    assert_eq!(d.borrow().starts, 2);
    let (mut e, mut p, d) = fixture(1, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    d.borrow_mut().natural = true;
    d.borrow_mut().wait_advance_ms = 20;
    p.civil.set(5);
    p.monotonic.set(5990);
    e.tick(&mut p, 5, 5990, 50).unwrap();
    assert_eq!(
        p.state.borrow().jobs[0].failure,
        Some(VoiceFailure::Timeout)
    );
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
}
#[test]
fn preparation_and_open_deadlines_are_fresh_and_playback_budget_starts_at_first_write() {
    let (mut e, mut p, d) = fixture(1, 16_000);
    p.source_advance_ms = 4000;
    d.borrow_mut().open_advance_ms = 500;
    tick(&mut e, &mut p, 0).unwrap();
    assert_eq!(d.borrow().starts, 1);
    assert_eq!(p.monotonic.get(), 4500);
    d.borrow_mut().natural = true;
    tick(&mut e, &mut p, 9).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Completed);
    let (mut e, mut p, d) = fixture(1, 16_000);
    p.source_advance_ms = 5001;
    tick(&mut e, &mut p, 0).unwrap();
    assert_eq!(d.borrow().starts, 0);
    assert_eq!(
        p.state.borrow().jobs[0].failure,
        Some(VoiceFailure::Timeout)
    );
    let (mut e, mut p, d) = fixture(1, 16_000);
    d.borrow_mut().open_advance_ms = 5001;
    tick(&mut e, &mut p, 0).unwrap();
    assert_eq!(d.borrow().starts, 0);
    assert_eq!(
        p.state.borrow().jobs[0].failure,
        Some(VoiceFailure::Timeout)
    );
    let (mut e, mut p, d) = fixture(1, 16_000);
    p.source_advance_ms = 1000;
    p.source_advance_civil = 1;
    tick(&mut e, &mut p, 299).unwrap();
    assert_eq!(d.borrow().starts, 0);
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Expired);
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
    assert!(!p.actions.contains(&VoiceDriverAction::FirstWrite));
    assert_eq!(p.state.borrow().jobs[0].first_started_at, None);
    assert!(p.actions.contains(&VoiceDriverAction::ExpireBeforeWrite));
    let (mut e, mut p, d) = fixture(1, 16_000);
    d.borrow_mut().open_advance_ms = 1000;
    d.borrow_mut().open_advance_civil = 1;
    tick(&mut e, &mut p, 299).unwrap();
    assert_eq!(d.borrow().starts, 0);
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Expired);
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
    assert!(!p.actions.contains(&VoiceDriverAction::FirstWrite));
    assert_eq!(p.state.borrow().jobs[0].first_started_at, None);
    assert!(p.state.borrow().jobs[0].release_generation.is_none());
    let (mut e, mut p, d) = fixture(1, 16_000);
    p.allowed = false;
    tick(&mut e, &mut p, 300).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Expired);
    assert_eq!(d.borrow().starts, 0);
    assert_eq!(p.sources, 0);
}
#[test]
fn persistent_clock_rollback_still_settles_durable_terminal_without_replay() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    tick(&mut e, &mut p, 1).unwrap();
    assert_eq!(
        tick(&mut e, &mut p, 0),
        Err(ExecutionError::ClockRegression)
    );
    assert_eq!(tick(&mut e, &mut p, 0), Ok(ExecutionStatus::Halted));
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Failed);
    assert!(p.state.borrow().jobs[0].release_generation.is_none());
    assert_eq!(tick(&mut e, &mut p, 0), Ok(ExecutionStatus::Halted));
    assert_eq!(d.borrow().starts, 1);
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
}
#[test]
fn first_five_minute_expiry_opens_no_file_or_device() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    tick(&mut e, &mut p, 300).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Expired);
    assert_eq!(p.sources, 0);
    assert_eq!(d.borrow().starts, 0);
}
#[test]
fn cancel_before_done_drains_exact_old_generation_without_counting() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    let old = p.state.borrow().jobs[0].generation;
    d.borrow_mut().hold_release = true;
    cancel(&mut p, 1);
    d.borrow_mut().natural = true;
    tick(&mut e, &mut p, 1).unwrap();
    assert!(e.has_native_ownership());
    assert_eq!(p.state.borrow().jobs[0].release_generation, Some(old));
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
    d.borrow_mut().hold_release = false;
    tick(&mut e, &mut p, 2).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Canceled);
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
    assert!(!e.has_native_ownership());
}
#[test]
fn fifo_next_job_cannot_start_until_the_canceled_device_is_confirmed_released() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    {
        let mut s = p.state.borrow_mut();
        let actor = s.jobs[0].authorization.clone().unwrap();
        let id = VoiceAssetId::new(2).unwrap();
        let a = s.reserve(id, &actor, ClassId::new(1).unwrap(), 0).unwrap();
        let a = s
            .publish(
                id,
                a.revision,
                VoicePcmMetadata {
                    byte_length: 32_044,
                    frame_count: 16_000,
                    sha256: [2; 32],
                },
                0,
            )
            .unwrap();
        s.submit(CommandId::new(2).unwrap(), id, a.revision, 1, &actor, 0)
            .unwrap();
    }
    tick(&mut e, &mut p, 0).unwrap();
    d.borrow_mut().hold_release = true;
    cancel(&mut p, 1);
    tick(&mut e, &mut p, 1).unwrap();
    tick(&mut e, &mut p, 2).unwrap();
    assert_eq!(d.borrow().starts, 1);
    assert_eq!(p.state.borrow().jobs[1].state, VoiceState::Queued);
    d.borrow_mut().hold_release = false;
    tick(&mut e, &mut p, 3).unwrap();
    assert_eq!(d.borrow().starts, 1);
    tick(&mut e, &mut p, 4).unwrap();
    assert_eq!(d.borrow().starts, 2);
    assert_eq!(p.state.borrow().jobs[1].state, VoiceState::Playing);
}
#[test]
fn cancel_between_native_first_write_and_commit_and_between_done_and_commit_wins() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    d.borrow_mut().after_write_cancel = true;
    assert!(tick(&mut e, &mut p, 0).is_err());
    assert!(e.halted());
    tick(&mut e, &mut p, 1).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Canceled);
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
    assert_eq!(d.borrow().starts, 1);
    let (mut e, mut p, d) = fixture(3, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    p.cancel_before_natural = true;
    d.borrow_mut().natural = true;
    assert!(tick(&mut e, &mut p, 1).is_err());
    tick(&mut e, &mut p, 1).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Canceled);
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
}
#[test]
fn unknown_storage_after_first_write_stops_and_cannot_replay_on_recovery() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    p.first_write_failure = true;
    assert!(tick(&mut e, &mut p, 0).is_err());
    assert_eq!(d.borrow().starts, 1);
    p.first_write_failure = false;
    tick(&mut e, &mut p, 1).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Failed);
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
    assert_eq!(tick(&mut e, &mut p, 2), Ok(ExecutionStatus::Halted));
    assert_eq!(d.borrow().starts, 1);
}
#[test]
fn lost_release_response_is_settled_only_from_inventory_and_known_device_release() {
    let (mut e, mut p, d) = fixture(1, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    p.released_response_lost = true;
    d.borrow_mut().natural = true;
    assert!(tick(&mut e, &mut p, 1).is_err());
    assert!(!e.has_native_ownership());
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Completed);
    assert_eq!(tick(&mut e, &mut p, 2), Ok(ExecutionStatus::Halted));
    assert_eq!(d.borrow().starts, 1);
}
#[test]
fn read_fault_source_fault_revocation_timeout_and_rollback_fail_closed() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    p.read_failure = true;
    assert!(tick(&mut e, &mut p, 1).is_err());
    assert!(!e.has_native_ownership());
    p.read_failure = false;
    tick(&mut e, &mut p, 2).unwrap();
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Failed);
    assert_eq!(d.borrow().starts, 1);
    let (mut e, mut p, d) = fixture(3, 16_000);
    p.source_failure = true;
    tick(&mut e, &mut p, 0).unwrap();
    assert_eq!(d.borrow().starts, 0);
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Failed);
    let (mut e, mut p, _) = fixture(3, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    p.allowed = false;
    tick(&mut e, &mut p, 1).unwrap();
    assert_eq!(
        p.state.borrow().jobs[0].failure,
        Some(VoiceFailure::Revoked)
    );
    let (mut e, mut p, _) = fixture(1, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    tick(&mut e, &mut p, 7).unwrap();
    assert_eq!(
        p.state.borrow().jobs[0].failure,
        Some(VoiceFailure::Timeout)
    );
    let (mut e, mut p, _) = fixture(3, 16_000);
    tick(&mut e, &mut p, 1).unwrap();
    assert_eq!(
        tick(&mut e, &mut p, 0),
        Err(ExecutionError::ClockRegression)
    );
    assert!(!e.has_native_ownership());
}
#[test]
fn new_executor_cannot_adopt_persisted_live_or_gap_ownership() {
    let (mut e, mut p, _) = fixture(3, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    let d = Rc::new(RefCell::new(Device::default()));
    let mut replacement = VoiceExecutor::new(Output {
        device: d.clone(),
        storage: p.state.clone(),
        monotonic: p.monotonic.clone(),
        civil: p.civil.clone(),
    });
    assert_eq!(
        tick(&mut replacement, &mut p, 1),
        Err(ExecutionError::Invariant)
    );
    assert_eq!(d.borrow().starts, 0);
}
#[test]
fn no_device_and_midstream_driver_error_fail_and_drain_then_preserve_error_without_replay() {
    let (mut e, mut p, d) = fixture(3, 16_000);
    d.borrow_mut().unavailable = true;
    assert_eq!(
        tick(&mut e, &mut p, 0),
        Err(ExecutionError::Output(AudioOutputError::DeviceUnavailable))
    );
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Failed);
    assert_eq!(p.state.borrow().jobs[0].failure, Some(VoiceFailure::Device));
    assert!(!e.has_native_ownership());
    assert!(e.halted());
    d.borrow_mut().unavailable = false;
    assert_eq!(tick(&mut e, &mut p, 1), Ok(ExecutionStatus::Halted));
    assert_eq!(d.borrow().starts, 0);
    let (mut e, mut p, d) = fixture(3, 16_000);
    tick(&mut e, &mut p, 0).unwrap();
    d.borrow_mut().error = true;
    assert_eq!(
        tick(&mut e, &mut p, 1),
        Err(ExecutionError::Output(AudioOutputError::Driver(1)))
    );
    assert_eq!(p.state.borrow().jobs[0].state, VoiceState::Failed);
    assert_eq!(p.state.borrow().jobs[0].failure, Some(VoiceFailure::Device));
    assert_eq!(p.state.borrow().jobs[0].completed_repeats, 0);
    assert!(!e.has_native_ownership());
    assert!(e.halted());
}
