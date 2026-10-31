//! Worker-only, bounded historical default-format facts. No observation is an
//! audio permission or evidence of a currently audible output.
use super::*;
use fc_ports::audio::{AudioOutputError, DefaultOutputSupport};
use fc_runtime::health::HealthStatus;
use std::sync::atomic::AtomicBool;

const LIFETIME: Duration = Duration::from_secs(30);
const LIFETIME_MILLIS: u64 = 30_000;

/// Conservative browser disclosure. Format support never becomes Available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebVoiceDeviceAvailability {
    Unverified,
    Unavailable,
}

/// Revisions and flags only; no cached identity, actor, session or authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    publication: Revision,
    identity: Revision,
    voice: Revision,
    runtime_word: u64,
    class: Option<ClassId>,
    configured_enabled: bool,
    memory_enabled: bool,
    admitting: bool,
    limits: VoiceLimits,
    health: HealthStatus,
}
impl Stamp {
    pub(super) fn new(
        view: &WebVoiceCapabilities,
        voice: Revision,
        runtime_word: u64,
        memory_enabled: bool,
        health: HealthStatus,
    ) -> Self {
        Self {
            publication: view.publication_revision,
            identity: view.identity_revision,
            voice,
            runtime_word,
            class: view.current_class.as_ref().map(|(id, _)| *id),
            configured_enabled: view.enabled,
            memory_enabled,
            admitting: view.admitting,
            limits: view.limits,
            health,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Observation {
    stamp: Stamp,
    result: WebVoiceDeviceAvailability,
    completed_tick: u64,
    completed: Instant,
}
impl Observation {
    fn fresh_at(&self, tick: u64, now: Instant) -> bool {
        tick.checked_sub(self.completed_tick)
            .is_some_and(|age| age < LIFETIME_MILLIS)
            && now
                .checked_duration_since(self.completed)
                .is_some_and(|age| age < LIFETIME)
    }
}
#[derive(Default)]
pub(super) struct OutputCache {
    observation: Mutex<Option<Observation>>,
    valid: AtomicBool,
    in_flight: AtomicBool,
}
// Counted once with VoiceService::Inner, with no secondary heap or native owner.
const _: () = assert!(std::mem::size_of::<OutputCache>() <= 512);

enum Lookup {
    Fresh(Observation),
    Missing,
    Busy,
}
impl OutputCache {
    fn lookup(&self, stamp: Stamp, tick: u64) -> Lookup {
        let Ok(mut slot) = self.observation.try_lock() else {
            return Lookup::Busy;
        };
        match *slot {
            Some(observation)
                if self.valid.load(Ordering::Acquire)
                    && observation.stamp == stamp
                    && observation.fresh_at(tick, Instant::now()) =>
            {
                Lookup::Fresh(observation)
            }
            _ => {
                self.valid.store(false, Ordering::Release);
                *slot = None;
                Lookup::Missing
            }
        }
    }
    fn begin(&self) -> Option<Flight<'_>> {
        self.in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Flight(self))
    }
    fn publish(&self, observation: Observation) -> Result<()> {
        let mut slot = self.observation.try_lock().map_err(|_| changed())?;
        *slot = Some(observation);
        self.valid.store(true, Ordering::Release);
        Ok(())
    }
    fn remove(&self, observation: Observation) {
        // Only the physical Flight publishes/withdraws, and it is still owned
        // here: no later query can have published a replacement. Negative
        // invalidation must not wait for a contended metadata lookup mutex.
        self.valid.store(false, Ordering::Release);
        let Ok(mut slot) = self.observation.try_lock() else {
            return;
        };
        if *slot == Some(observation) {
            *slot = None;
        }
    }
}
/// Cancellation and TTL never release this owner. It stays on the original
/// worker stack until the synchronous query actually returns (including panic).
struct Flight<'a>(&'a OutputCache);
impl Drop for Flight<'_> {
    fn drop(&mut self) {
        self.0.in_flight.store(false, Ordering::Release);
    }
}

fn changed() -> BackendError {
    BackendError::Storage(StorageError::Busy)
}
impl VoiceService {
    fn output_review_current(&self, lease: SessionLease, stamp: Stamp) -> Result<()> {
        // Exact session/role/password status is checked for every request and
        // delivery, even when this hardware fact came from another request.
        self.check_web_actor(lease)?;
        if self.inner.runtime_state.load(Ordering::Acquire) != stamp.runtime_word
            || self.inner.enabled.load(Ordering::Acquire) != stamp.memory_enabled
            || self.inner.control.health_gate().status() != stamp.health
        {
            return Err(changed());
        }
        self.execution_checkpoint()
    }
    fn output_finish(
        &self,
        lease: SessionLease,
        stamp: Stamp,
        observation: Option<Observation>,
    ) -> Result<WebVoiceCapabilities> {
        let (mut view, latest) = self.web_capabilities_state(lease)?;
        if latest != stamp {
            return Err(changed());
        }
        self.output_review_current(lease, stamp)?;
        if let Some(observation) = observation.filter(|o| {
            self.inner.output_observation.valid.load(Ordering::Acquire)
                && o.stamp == stamp
                && o.fresh_at(self.inner.monotonic.monotonic_millis(), Instant::now())
        }) {
            view.device_availability = observation.result;
        }
        self.output_review_current(lease, stamp)?;
        if observation.is_some_and(|o| {
            !self.inner.output_observation.valid.load(Ordering::Acquire)
                || !o.fresh_at(self.inner.monotonic.monotonic_millis(), Instant::now())
        }) {
            view.device_availability = WebVoiceDeviceAvailability::Unverified;
        }
        self.execution_checkpoint()?;
        Ok(view)
    }

    /// Final encoder/delivery check on the same request-scoped service. Its
    /// existing fence must still contain the original deadline/cancellation.
    /// This never queries hardware, renews a cached time, or grants playback.
    pub fn check_web_capabilities_output(
        &self,
        lease: SessionLease,
        view: &WebVoiceCapabilities,
    ) -> Result<()> {
        let (current, stamp) = self.web_capabilities_state(lease)?;
        if current.publication_revision != view.publication_revision
            || current.identity_revision != view.identity_revision
            || current.runtime_generation != view.runtime_generation
            || current.current_class != view.current_class
            || current.enabled != view.enabled
            || current.admitting != view.admitting
            || current.target_authorized != view.target_authorized
            || current.limits != view.limits
            || current.now < view.now
        {
            return Err(changed());
        }
        let observation = if view.device_availability == WebVoiceDeviceAvailability::Unavailable {
            if !current.enabled || !current.admitting || !current.target_authorized {
                return Err(changed());
            }
            match self
                .inner
                .output_observation
                .lookup(stamp, self.inner.monotonic.monotonic_millis())
            {
                Lookup::Fresh(observation)
                    if observation.result == WebVoiceDeviceAvailability::Unavailable =>
                {
                    Some(observation)
                }
                _ => return Err(changed()),
            }
        } else {
            None
        };
        self.output_review_current(lease, stamp)?;
        if observation.is_some_and(|o| {
            !self.inner.output_observation.valid.load(Ordering::Acquire)
                || !o.fresh_at(self.inner.monotonic.monotonic_millis(), Instant::now())
        }) {
            return Err(changed());
        }
        self.execution_checkpoint()
    }

    /// Existing shared worker only. `deadline` is the original absolute request
    /// bound; a scoped service must additionally carry that request's cancellation
    /// fence. This API never creates threads or resets the supplied deadline.
    ///
    /// All Backend/ledger/cache/writer guards must be released before invoking
    /// the trusted query closure. A blocked driver cannot be forcibly cancelled:
    /// its same worker/Flight remains occupied until actual return. Late results
    /// cannot enter the cache or response. Existing 100ms metadata bounds remain.
    pub fn web_capabilities_with_output(
        &self,
        lease: SessionLease,
        deadline: Instant,
        query: impl FnOnce() -> std::result::Result<DefaultOutputSupport, AudioOutputError>,
    ) -> Result<WebVoiceCapabilities> {
        let service = self.with_execution_fence(Arc::new(move || Instant::now() < deadline));
        service.execution_checkpoint()?;
        let (view, stamp) = service.web_capabilities_state(lease)?;
        if !view.target_authorized || !view.enabled || !view.admitting {
            return service.output_finish(lease, stamp, None);
        }
        let cache = &service.inner.output_observation;
        match cache.lookup(stamp, service.inner.monotonic.monotonic_millis()) {
            Lookup::Fresh(observation) => {
                return service.output_finish(lease, stamp, Some(observation));
            }
            Lookup::Busy => return service.output_finish(lease, stamp, None),
            Lookup::Missing => {}
        }
        let Some(_flight) = cache.begin() else {
            // Never wait for or replace another request's native query.
            return service.output_finish(lease, stamp, None);
        };
        // Another physical query may have completed between the first lookup
        // and claiming the now-free slot. Reuse its original time, never renew it.
        match cache.lookup(stamp, service.inner.monotonic.monotonic_millis()) {
            Lookup::Fresh(observation) => {
                return service.output_finish(lease, stamp, Some(observation));
            }
            Lookup::Busy => return service.output_finish(lease, stamp, None),
            Lookup::Missing => {}
        }
        let (_, latest) = service.web_capabilities_state(lease)?;
        if latest != stamp {
            return Err(changed());
        }
        service.output_review_current(lease, stamp)?;
        let started_tick = service.inner.monotonic.monotonic_millis();
        service.execution_checkpoint()?;
        // No metadata mutex or writer transaction survives into this call.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(query));
        // TTL starts at actual native completion, before any metadata wait or
        // response delivery. Query duration is never added to the request bound.
        let completed_tick = service.inner.monotonic.monotonic_millis();
        let completed = Instant::now();
        service.execution_checkpoint()?;
        if completed_tick < started_tick {
            return Err(changed());
        }
        let result = match result {
            Ok(Ok(DefaultOutputSupport::FormatUnsupported | DefaultOutputSupport::Unavailable)) => {
                WebVoiceDeviceAvailability::Unavailable
            }
            Ok(Ok(DefaultOutputSupport::Supported)) | Ok(Err(_)) | Err(_) => {
                WebVoiceDeviceAvailability::Unverified
            }
        };
        let observation = Observation {
            stamp,
            result,
            completed_tick,
            completed,
        };
        let mut view = service.output_finish(lease, stamp, None)?;
        service.output_review_current(lease, stamp)?;
        if !observation.fresh_at(service.inner.monotonic.monotonic_millis(), Instant::now()) {
            return Err(changed());
        }
        cache.publish(observation)?;
        // A concurrent revoke/stop can overtake publication. Withdraw only this
        // exact observation; never claim to roll back a previous caller's fact.
        if let Err(error) = service.output_review_current(lease, stamp) {
            cache.remove(observation);
            return Err(error);
        }
        if !cache.valid.load(Ordering::Acquire)
            || !observation.fresh_at(service.inner.monotonic.monotonic_millis(), Instant::now())
        {
            cache.remove(observation);
            return Err(changed());
        }
        view.device_availability = observation.result;
        service
            .execution_checkpoint()
            .inspect_err(|_| cache.remove(observation))?;
        Ok(view)
    }
}

#[cfg(test)]
mod tests;
