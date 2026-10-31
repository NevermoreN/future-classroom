//! Finite capability for the trusted, rendered local desktop entry. This is not
//! an account/session and is deliberately absent from every Web request model.
use super::*;
use fc_ports::{
    DesktopLessonBinding, DesktopLessonRepository, DesktopLessonRequest, DesktopLessonSource,
};

const FLOW_BUDGET: Duration = Duration::from_secs(120);

/// Captured together from the actual accepted Scene and its original facts.
/// Only the native desktop controller publishes these stamps; FormData never
/// supplies a scene, lesson key, identity or expected revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopLessonStamp {
    pub generation: SceneGeneration,
    pub source_revision: Revision,
    pub binding: DesktopLessonBinding,
    pub captured_at: LocalDateTime,
    pub valid_until: LocalDateTime,
}

#[derive(Default)]
struct Published {
    high_water: u64,
    scene: Option<DesktopLessonStamp>,
    scene_epoch: u64,
    next_flow: u64,
}
pub(super) struct DesktopLessonControl {
    published: Mutex<Published>,
    active: AtomicU64,
    epoch: AtomicU64,
    clock: Arc<dyn Clock + Send + Sync>,
    monotonic: Arc<dyn MonotonicClock + Send + Sync>,
}
impl DesktopLessonControl {
    pub(super) fn new(
        clock: Arc<dyn Clock + Send + Sync>,
        monotonic: Arc<dyn MonotonicClock + Send + Sync>,
    ) -> Self {
        Self {
            published: Mutex::new(Published::default()),
            active: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            clock,
            monotonic,
        }
    }
}

struct FlowState {
    id: u64,
    epoch: u64,
    stamp: DesktopLessonStamp,
    expires_at: Instant,
    last_tick: AtomicU64,
    expires_tick: u64,
    cancelled: AtomicBool,
}
// The existing Work allocation reserves 1024 bytes for this fixed capability.
const _: () = assert!(std::mem::size_of::<FlowState>() + 2 * std::mem::size_of::<usize>() <= 1024);

/// The UI owns this non-Clone cancellation owner until it closes the chooser.
/// Dropping a worker's cloned Guard cannot cancel or renew another flow.
pub struct DesktopLessonFlow {
    guard: DesktopLessonGuard,
}
impl DesktopLessonFlow {
    pub fn guard(&self) -> DesktopLessonGuard {
        self.guard.clone()
    }
    pub fn cancel(&self) {
        self.guard.cancel();
    }
}
impl Drop for DesktopLessonFlow {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Opaque worker permission, bound to its exact Backend and original interval.
#[derive(Clone)]
pub struct DesktopLessonGuard {
    state: Arc<FlowState>,
    control: SessionControl,
}
impl DesktopLessonGuard {
    fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release);
        let _ = self.control.0.desktop_lesson.active.compare_exchange(
            self.state.id,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    pub fn is_current(&self) -> bool {
        self.interval_current(false)
    }
    // Only the sole writer uses this during its own checked publication pulse.
    fn writer_current(&self) -> bool {
        self.interval_current(true)
    }
    fn interval_current(&self, writing: bool) -> bool {
        let healthy = |health| {
            health == fc_runtime::health::Health::Healthy
                || (writing && health == fc_runtime::health::Health::AuthorityUpdating)
        };
        let control = &self.control.0;
        if self.state.cancelled.load(Ordering::Acquire)
            || control.closed.load(Ordering::Acquire)
            || control.recovering.load(Ordering::Acquire)
            || control.desktop_lesson.active.load(Ordering::Acquire) != self.state.id
            || control.desktop_lesson.epoch.load(Ordering::Acquire) != self.state.epoch
            || !healthy(control.health.status().health)
        {
            return false;
        }
        let now = control.desktop_lesson.clock.now();
        let mut prior = self.state.last_tick.load(Ordering::Acquire);
        let mut tick = control.desktop_lesson.monotonic.monotonic_millis();
        if Instant::now() >= self.state.expires_at
            || now < self.state.stamp.captured_at
            || now >= self.state.stamp.valid_until
            || tick >= self.state.expires_tick
        {
            self.cancel();
            return false;
        }
        // A regression cannot become valid again by returning to a later tick.
        loop {
            if tick < prior {
                self.cancel();
                return false;
            }
            match self.state.last_tick.compare_exchange_weak(
                prior,
                tick,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(value) => {
                    prior = value;
                    tick = control.desktop_lesson.monotonic.monotonic_millis();
                    if tick >= self.state.expires_tick {
                        self.cancel();
                        return false;
                    }
                }
            }
        }
        // Publication changes are checked by the repository's exact binding,
        // not by the entry's old global revision (our own COMMIT changes it).
        !self.state.cancelled.load(Ordering::Acquire)
            && control.desktop_lesson.active.load(Ordering::Acquire) == self.state.id
            && control.desktop_lesson.epoch.load(Ordering::Acquire) == self.state.epoch
            && !control.closed.load(Ordering::Acquire)
            && !control.recovering.load(Ordering::Acquire)
            && healthy(control.health.status().health)
            && Instant::now() < self.state.expires_at
    }
    /// Shared metadata allocation counted once by its UI owner, not per clone.
    /// No file, database or native owner is retained by this capability.
    pub fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<FlowState>() + 2 * std::mem::size_of::<usize>()
    }
}

fn cancelled() -> BackendError {
    BackendError::Storage(StorageError::ExecutionCancelled)
}

impl SessionControl {
    fn desktop_entry_current(&self, stamp: &DesktopLessonStamp) -> bool {
        let status = self.0.health.status();
        let now = self.0.desktop_lesson.clock.now();
        stamp.generation.get() != 0
            && !self.0.closed.load(Ordering::Acquire)
            && !self.is_recovering()
            && status.health == fc_runtime::health::Health::Healthy
            && status.revision == stamp.source_revision
            && stamp.captured_at.date == stamp.binding.lesson.actual_date
            && stamp.valid_until.date == stamp.captured_at.date
            && stamp.valid_until > stamp.captured_at
            && stamp.valid_until.second_of_day()
                <= u32::from(stamp.binding.lesson.range.end.get()) * 60
            && stamp
                .binding
                .lesson
                .range
                .contains_second(stamp.captured_at.second_of_day())
            && now >= stamp.captured_at
            && now < stamp.valid_until
    }
    /// Trusted native renderer acknowledgement only. No SQL/KDF or actor mint.
    pub fn publish_desktop_lesson_scene(&self, stamp: DesktopLessonStamp) -> bool {
        if !self.desktop_entry_current(&stamp) {
            return false;
        }
        let Ok(mut published) = self.0.desktop_lesson.published.lock() else {
            return false;
        };
        let epoch = self.0.desktop_lesson.epoch.load(Ordering::Acquire);
        if stamp.generation.get() < published.high_water
            || (stamp.generation.get() == published.high_water
                && (published.scene_epoch != epoch
                    || published.scene.as_ref().is_none_or(|old| old != &stamp)))
            || !self.desktop_entry_current(&stamp)
            || self.0.desktop_lesson.epoch.load(Ordering::Acquire) != epoch
        {
            return false;
        }
        published.high_water = stamp.generation.get();
        published.scene = Some(stamp);
        published.scene_epoch = epoch;
        true
    }
    /// Hiding the desktop removes only future entry eligibility. The already
    /// minted chooser remains usable until its owner explicitly cancels/drops.
    pub fn clear_desktop_lesson_scene(&self) {
        if let Ok(mut published) = self.0.desktop_lesson.published.lock() {
            published.scene = None;
        }
    }
    /// Immediate OS-session/power/shutdown revocation. Unlike account locking,
    /// this invalidates both the active flow and every old render acknowledgement.
    /// The native control callback must call this before returning; it never
    /// waits for Backend, the writer, or even the short scene-metadata mutex.
    pub fn cancel_desktop_lesson_flow(&self) {
        if self
            .0
            .desktop_lesson
            .epoch
            .try_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                epoch.checked_add(1)
            })
            .is_err()
        {
            self.0.closed.store(true, Ordering::Release);
        }
        self.0.desktop_lesson.active.store(0, Ordering::Release);
        // Epoch mismatch already revokes an old scene even when a concurrent
        // mint/render owns this mutex. Clearing the optional value is only tidy.
        if let Ok(mut published) = self.0.desktop_lesson.published.try_lock() {
            published.scene = None;
        }
    }
    pub fn begin_desktop_lesson_flow(
        &self,
        stamp: &DesktopLessonStamp,
    ) -> Result<DesktopLessonFlow> {
        if !self.desktop_entry_current(stamp) {
            return Err(cancelled());
        }
        let mut published = self
            .0
            .desktop_lesson
            .published
            .lock()
            .map_err(|_| cancelled())?;
        let epoch = self.0.desktop_lesson.epoch.load(Ordering::Acquire);
        if published.scene.as_ref() != Some(stamp)
            || published.scene_epoch != epoch
            || !self.desktop_entry_current(stamp)
        {
            return Err(cancelled());
        }
        let now = self.0.desktop_lesson.clock.now();
        let tick = self.0.desktop_lesson.monotonic.monotonic_millis();
        let seconds = stamp
            .valid_until
            .second_of_day()
            .checked_sub(now.second_of_day())
            .ok_or_else(cancelled)?;
        let budget = FLOW_BUDGET.min(Duration::from_secs(u64::from(seconds)));
        if budget.is_zero() {
            return Err(cancelled());
        }
        let expires_at = Instant::now()
            .checked_add(budget)
            .ok_or(BackendError::Overflow)?;
        let expires_tick = tick
            .checked_add(
                budget
                    .as_millis()
                    .try_into()
                    .map_err(|_| BackendError::Overflow)?,
            )
            .ok_or(BackendError::Overflow)?;
        let id = published
            .next_flow
            .checked_add(1)
            .ok_or(BackendError::Overflow)?;
        self.0
            .desktop_lesson
            .active
            .compare_exchange(0, id, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| cancelled())?;
        published.next_flow = id;
        // A scene acknowledgement is consumed once. A later real render must
        // supply a higher generation before another independent flow can mint.
        published.scene = None;
        let flow = DesktopLessonFlow {
            guard: DesktopLessonGuard {
                state: Arc::new(FlowState {
                    id,
                    epoch,
                    stamp: stamp.clone(),
                    expires_at,
                    last_tick: AtomicU64::new(tick),
                    expires_tick,
                    cancelled: AtomicBool::new(false),
                }),
                control: self.clone(),
            },
        };
        if !flow.guard.is_current() || !self.desktop_entry_current(stamp) {
            return Err(cancelled());
        }
        Ok(flow)
    }
}

impl Backend {
    fn check_desktop_lesson(&self, guard: &DesktopLessonGuard) -> Result<()> {
        self.check_open()?;
        if !Arc::ptr_eq(&self.control.0, &guard.control.0) || !guard.is_current() {
            return Err(cancelled());
        }
        Ok(())
    }
    /// Run on the existing shared worker. Reload never rebases the original
    /// captured key or any CAS; external changes require a new desktop entry.
    pub fn read_desktop_lesson(&self, guard: &DesktopLessonGuard) -> Result<DesktopLessonSource> {
        self.check_desktop_lesson(guard)?;
        let permission = guard.clone();
        let result = self
            .repository
            .clone()
            .with_desktop_lesson_guard(Arc::new(move || permission.writer_current()))
            .desktop_lesson(guard.state.stamp.binding.clone())?;
        self.check_desktop_lesson(guard)?;
        Ok(result)
    }
    pub fn change_desktop_lesson(
        &mut self,
        guard: &DesktopLessonGuard,
        action: LessonAction,
    ) -> Result<DesktopLessonSource> {
        self.check_desktop_lesson(guard)?;
        let request = DesktopLessonRequest {
            command_id: self.command_id()?,
            binding: guard.state.stamp.binding.clone(),
            action,
        };
        let permission = guard.clone();
        let result = self
            .repository
            .clone()
            .with_desktop_lesson_guard(Arc::new(move || permission.writer_current()))
            .execute_desktop_lesson(request)?;
        self.check_desktop_lesson(guard)?;
        Ok(result)
    }
}
