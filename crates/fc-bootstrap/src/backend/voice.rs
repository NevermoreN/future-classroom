//! Trusted voice composition. Persisted ActorContext is never substituted for
//! the exact process-local Web session that admitted this upload and job.
use super::*;
use fc_domain::voice::*;
use fc_ports::voice::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct VoiceSessions {
    uploads: BTreeMap<VoiceAssetId, SessionLease>,
    jobs: BTreeMap<VoiceJobId, VoiceSession>,
}
struct VoiceSession {
    lease: SessionLease,
    stamp: Arc<Mutex<Stamp>>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    revision: Revision,
    generation: u64,
    state: VoiceState,
}
impl From<&VoiceJob> for Stamp {
    fn from(job: &VoiceJob) -> Self {
        Self {
            revision: job.revision,
            generation: job.generation,
            state: job.state,
        }
    }
}
/// This capability is short in authority lifetime, not renewed by elapsed time.
/// Any writer commit requires a new capability. Native per-buffer checks never
/// read SQLite or wait on the Backend mutex.
#[derive(Clone)]
pub struct VoiceGuard {
    check: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl VoiceGuard {
    pub fn is_valid(&self) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.check)())).unwrap_or(false)
    }
}
pub struct VoicePlaybackAuthorization {
    pub job: VoiceJob,
    pub guard: VoiceGuard,
}
fn denied() -> BackendError {
    BackendError::Voice(VoiceRepositoryError::Domain(VoiceError::Permission(
        PermissionError::StaleAuthorization,
    )))
}
impl Backend {
    fn voice_actor(&self, lease: SessionLease) -> Result<(ActorContext, u64)> {
        if lease.origin() != TrustedOrigin::Web {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::Permission(PermissionError::WebOnly),
            )));
        }
        self.actor(&self.state()?, lease, SessionPurpose::Manage)
    }
    pub fn voice_reserve(&mut self, lease: SessionLease, class: ClassId) -> Result<VoiceAsset> {
        let (actor, generation) = self.voice_actor(lease)?;
        if self.voice.uploads.len() >= MAX_VOICE_ASSETS {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::Capacity,
            )));
        }
        let mut bytes = [0; 8];
        let id = (0..8)
            .find_map(|_| {
                PlatformRandom.fill(&mut bytes).ok()?;
                VoiceAssetId::new(u64::from_le_bytes(bytes)).ok()
            })
            .ok_or(BackendError::CryptoUnavailable)?;
        let asset = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .reserve_voice_upload(id, class, &actor)?;
        self.voice.uploads.insert(asset.id, lease);
        Ok(asset)
    }
    pub fn voice_publish(
        &mut self,
        lease: SessionLease,
        prepared: PreparedVoiceUpload,
    ) -> Result<VoiceAsset> {
        if self.voice.uploads.get(&prepared.reservation().id) != Some(&lease) {
            return Err(denied());
        }
        let (actor, generation) = self.voice_actor(lease)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .publish_voice_upload(prepared, &actor)?)
    }
    pub fn voice_submit(
        &mut self,
        lease: SessionLease,
        request: VoiceSubmitRequest,
    ) -> Result<VoiceJob> {
        let (actor, generation) = self.voice_actor(lease)?;
        // A retry can refer to an already consumed asset, but must never replace
        // the original job's session with this caller's newer session.
        let id = VoiceJobId::new(request.command_id.get())
            .map_err(|e| BackendError::Voice(VoiceRepositoryError::Domain(e)))?;
        let existing = self
            .voice_inventory()?
            .jobs
            .into_iter()
            .find(|j| j.id == id);
        if let Some(original) = self.voice.jobs.get(&id) {
            if !existing.as_ref().is_some_and(|j| j.state.terminal()) && original.lease != lease {
                return Err(denied());
            }
        } else if existing.as_ref().is_some_and(|j| j.state.terminal()) {
            // A terminal retry can return the current authorized result, but can
            // never recreate a process-local playback authorization.
            self.voice_status(lease, id)?;
        } else if existing.is_some() || self.voice.uploads.get(&request.asset_id) != Some(&lease) {
            return Err(denied());
        }
        let asset_id = request.asset_id;
        let job = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .submit_voice(request, &actor)?;
        self.voice.uploads.remove(&asset_id);
        if existing.is_none() && !job.state.terminal() {
            self.voice
                .jobs
                .entry(job.id)
                .or_insert_with(|| VoiceSession {
                    lease,
                    stamp: Arc::new(Mutex::new(Stamp::from(&job))),
                });
        }
        Ok(job)
    }
    pub fn voice_status(&self, lease: SessionLease, id: VoiceJobId) -> Result<VoiceJob> {
        let (actor, generation) = self.voice_actor(lease)?;
        let mut job = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .voice_job(id, &actor)?;
        job.authorization = None;
        Ok(job)
    }
    pub fn voice_cancel(
        &mut self,
        lease: SessionLease,
        request: VoiceCancelRequest,
    ) -> Result<VoiceJob> {
        let (actor, generation) = self.voice_actor(lease)?;
        let job = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .cancel_voice(request, &actor)?;
        self.voice_update(&job)?;
        Ok(job)
    }
    fn voice_update(&mut self, job: &VoiceJob) -> Result<()> {
        if let Some(entry) = self.voice.jobs.get(&job.id) {
            *entry.stamp.lock().map_err(|_| BackendError::Closed)? = Stamp::from(job);
        }
        if job.state.terminal() {
            self.voice.jobs.remove(&job.id);
        }
        Ok(())
    }
    /// Internal inventory contains private authorizations. Never serialize it to Web.
    pub fn voice_inventory(&self) -> Result<VoiceSnapshot> {
        self.check_open()?;
        Ok(self.repository.voice_inventory()?)
    }
    /// Exclusive-instance startup only, before creating any audio executor.
    pub fn voice_recover(&mut self) -> Result<VoiceSnapshot> {
        self.check_open()?;
        let state = self.repository.recover_voice()?;
        for entry in self.voice.jobs.values() {
            entry.stamp.lock().map_err(|_| BackendError::Closed)?.state = VoiceState::Interrupted;
        }
        self.voice.jobs.clear();
        self.voice.uploads.clear();
        Ok(state)
    }
    pub fn voice_drive(&mut self, update: VoiceDriverUpdate) -> Result<VoiceJob> {
        self.check_open()?;
        let job = if matches!(
            update.action,
            VoiceDriverAction::Start | VoiceDriverAction::FirstWrite
        ) {
            let lease = self
                .voice
                .jobs
                .get(&update.job_id)
                .ok_or_else(denied)?
                .lease;
            let (_, generation) = self.voice_actor(lease)?;
            self.guarded(lease, generation, SessionPurpose::Manage)
                .drive_voice(update)?
        } else {
            // Revocation must not prevent settling a previously owned device.
            self.repository.drive_voice(update)?
        };
        self.voice_update(&job)?;
        Ok(job)
    }
    pub fn voice_guard(&self, id: VoiceJobId) -> Result<VoicePlaybackAuthorization> {
        let inventory = self.voice_inventory()?;
        if !inventory.enabled {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::Disabled,
            )));
        }
        let job = inventory
            .jobs
            .into_iter()
            .find(|j| {
                j.id == id
                    && matches!(
                        j.state,
                        VoiceState::Queued
                            | VoiceState::Starting
                            | VoiceState::Playing
                            | VoiceState::Gap
                    )
            })
            .ok_or_else(denied)?;
        let entry = self.voice.jobs.get(&id).ok_or_else(denied)?;
        let lease = entry.lease;
        let expected = Stamp::from(&job);
        if *entry.stamp.lock().map_err(|_| BackendError::Closed)? != expected {
            return Err(denied());
        }
        let state = self
            .authority
            .snapshot()
            .map_err(|_| BackendError::Closed)?;
        let (actor, generation) = self.actor(&state.data, lease, SessionPurpose::Manage)?;
        if job.authorization.as_ref() != Some(&actor) {
            return Err(denied());
        }
        authorize_submit(&state.data, &actor, job.class_id, self.clock.now())
            .map_err(|e| BackendError::Voice(VoiceRepositoryError::Domain(e)))?;
        let status = self
            .control
            .health_gate()
            .check_sensitive()
            .map_err(|_| BackendError::Closed)?;
        if state.revision != status.revision {
            return Err(BackendError::Closed);
        }
        let stamp = entry.stamp.clone();
        let control = self.control.clone();
        let clock = self.clock.clone();
        let monotonic = self.monotonic.clone();
        let ledger = self.ledger.clone();
        let class = job.class_id;
        let issued_at = self.clock.now();
        let latest_start = job
            .first_started_at
            .is_none()
            .then_some(job.latest_start_at);
        let gate = control.health_gate();
        let guard = VoiceGuard {
            check: Arc::new(move || {
                let now = clock.now();
                let fresh = gate.status();
                if now < issued_at
                    || latest_start.is_some_and(|latest| civil_seconds(now) >= latest)
                    || fresh != status
                    || control.check(lease, generation).is_err()
                    || stamp.lock().map_or(true, |s| *s != expected)
                {
                    return false;
                }
                let Ok(mut ledger) = ledger.lock() else {
                    return false;
                };
                ledger.synchronize(&control, &state.data, now, monotonic.monotonic_millis());
                let Ok(actual) = ledger.sessions.actor(
                    &state.data,
                    lease,
                    now,
                    monotonic.monotonic_millis(),
                    SessionPurpose::Manage,
                ) else {
                    return false;
                };
                if actual != actor || authorize_submit(&state.data, &actual, class, now).is_err() {
                    return false;
                }
                let expires = Instant::now() + Duration::from_millis(50);
                let Ok(effect) = gate.lease_for_authorized_scope(id.get(), expires) else {
                    return false;
                };
                let Ok(permit) = gate.try_start(
                    effect,
                    fc_runtime::health::EffectIntent {
                        scope: id.get(),
                        job_id: id.get(),
                    },
                ) else {
                    return false;
                };
                gate.check_permit(&permit).is_ok() && gate.status() == status
            }),
        };
        Ok(VoicePlaybackAuthorization { job, guard })
    }
    /// Local onsite emergency path; it grants no manager permission.
    pub fn voice_emergency_stop(&mut self) -> Result<VoiceSnapshot> {
        self.check_open()?;
        let snapshot = self.repository.emergency_stop_voice()?;
        for job in &snapshot.jobs {
            self.voice_update(job)?;
        }
        Ok(snapshot)
    }
    pub fn voice_enabled(
        &mut self,
        lease: SessionLease,
        enabled: bool,
        expected: Revision,
    ) -> Result<VoiceSnapshot> {
        let (actor, generation) = self.actor(&self.state()?, lease, SessionPurpose::Manage)?;
        let snapshot = self
            .guarded(lease, generation, SessionPurpose::Manage)
            .set_voice_enabled(enabled, expected, &actor)?;
        for job in &snapshot.jobs {
            self.voice_update(job)?;
        }
        Ok(snapshot)
    }
    pub fn voice_maintain(&mut self) -> Result<VoiceSnapshot> {
        self.check_open()?;
        let snapshot = self.repository.maintain_voice()?;
        for job in &snapshot.jobs {
            self.voice_update(job)?;
        }
        self.voice.jobs.retain(|id, _| {
            snapshot
                .jobs
                .iter()
                .any(|j| j.id == *id && !j.state.terminal())
        });
        self.voice.uploads.retain(|id, _| {
            snapshot.assets.iter().any(|a| {
                a.id == *id && matches!(a.state, VoiceAssetState::Reserved | VoiceAssetState::Ready)
            })
        });
        Ok(snapshot)
    }
    pub fn voice_finalize_delete(&mut self, id: VoiceAssetId, expected: Revision) -> Result<()> {
        self.check_open()?;
        self.repository.finalize_voice_delete(id, expected)?;
        self.voice.uploads.remove(&id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct ClockFixture(Mutex<LocalDateTime>);
    impl Clock for ClockFixture {
        fn now(&self) -> LocalDateTime {
            *self.0.lock().unwrap()
        }
    }
    impl MonotonicClock for ClockFixture {
        fn monotonic_millis(&self) -> u64 {
            0
        }
    }
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let r = std::fs::remove_dir_all(&self.0);
            if !std::thread::panicking() {
                r.unwrap();
            }
        }
    }
    struct Service(Backend);
    impl Drop for Service {
        fn drop(&mut self) {
            let r = self.0.close();
            if !std::thread::panicking() {
                r.unwrap();
            }
        }
    }
    fn issue(backend: &Backend, origin: TrustedOrigin) -> SessionLease {
        let state = backend.state().unwrap();
        let account = state.account(PrincipalId::new(1).unwrap()).unwrap();
        let lease = backend
            .ledger
            .lock()
            .unwrap()
            .sessions
            .issue_verified(
                account,
                origin,
                0,
                SESSION_MILLIS,
                LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 3600).unwrap(),
            )
            .unwrap();
        if origin == TrustedOrigin::Local {
            backend
                .control
                .0
                .active_local
                .store(lease.id(), Ordering::Release);
            backend
                .control
                .0
                .active_principal
                .store(lease.principal_id().get(), Ordering::Release);
        }
        lease
    }
    struct MetadataPreparation;
    impl VoicePreparation for MetadataPreparation {
        type Input = ();
        fn prepare(
            &mut self,
            _: (),
            _: &VoiceAsset,
        ) -> std::result::Result<VoicePcmMetadata, VoiceRepositoryError> {
            Ok(VoicePcmMetadata {
                byte_length: 16_044,
                frame_count: 8_000,
                sha256: [1; 32],
            })
        }
    }
    fn prepared(asset: VoiceAsset) -> PreparedVoiceUpload {
        prepare_voice_upload(&mut MetadataPreparation, (), asset).unwrap()
    }
    fn submit(
        backend: &mut Backend,
        lease: SessionLease,
        class: ClassId,
        id: u64,
    ) -> VoiceSubmitRequest {
        let reservation = backend.voice_reserve(lease, class).unwrap();
        let asset = backend.voice_publish(lease, prepared(reservation)).unwrap();
        VoiceSubmitRequest {
            command_id: CommandId::new(id).unwrap(),
            asset_id: asset.id,
            expected_asset_revision: asset.revision,
            class_id: class,
            repeat_total: 3,
        }
    }
    #[test]
    fn exact_original_voice_session_guards_cancel_disable_and_terminal_retry_are_real() {
        let directory = Directory(std::env::temp_dir().join(format!(
                "FutureClassroom-Voice-Backend-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&directory.0).unwrap();
        let clock = Arc::new(ClockFixture(Mutex::new(
            LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 3600).unwrap(),
        )));
        let mut service = Service(
            Backend::open(
                directory.0.join("state.sqlite3"),
                CredentialRecordCodec::from_key([91; 32]).unwrap(),
                clock.clone(),
                clock.clone(),
            )
            .unwrap(),
        );
        let backend = &mut service.0;
        drop(
            backend
                .initialize(
                    Initialize {
                        school_name: "Synthetic voice".into(),
                        login_name: "admin".into(),
                        display_name: "Synthetic administrator".into(),
                    },
                    Password::new("Synthetic voice credential 2026!".into()).unwrap(),
                )
                .unwrap(),
        );
        let local = issue(backend, TrustedOrigin::Local);
        let class = ClassId::new(1).unwrap();
        backend
            .identity(
                local,
                IdentityCommand::CreateClass {
                    id: class,
                    name: "Synthetic class".into(),
                },
            )
            .unwrap();
        backend
            .identity(local, IdentityCommand::BindCurrentClass { id: class })
            .unwrap();
        let old = issue(backend, TrustedOrigin::Web);
        let request = submit(backend, old, class, 71);
        let job = backend.voice_submit(old, request.clone()).unwrap();
        assert!(job.authorization.is_none());
        let queued = backend.voice_guard(job.id).unwrap();
        assert!(queued.guard.is_valid());
        let newer = issue(backend, TrustedOrigin::Web);
        assert!(
            !queued.guard.is_valid(),
            "same principal's new session cannot revive old queued permit"
        );
        assert!(backend.voice_submit(newer, request.clone()).is_err());
        assert!(backend
            .voice_drive(VoiceDriverUpdate {
                job_id: job.id,
                expected_revision: job.revision,
                generation: job.generation,
                action: VoiceDriverAction::Start
            })
            .is_err());
        // Current authenticated owner may cancel the old job, but cannot replace
        // the original session to play it. Terminal retry returns current state.
        let canceled = backend
            .voice_cancel(
                newer,
                VoiceCancelRequest {
                    command_id: CommandId::new(72).unwrap(),
                    job_id: job.id,
                    expected_revision: job.revision,
                },
            )
            .unwrap();
        assert_eq!(canceled.state, VoiceState::Canceled);
        let replay = backend.voice_submit(newer, request).unwrap();
        assert_eq!(replay.state, VoiceState::Canceled);
        assert!(replay.authorization.is_none());
        assert!(backend.voice_guard(job.id).is_err());
        assert!(!backend.voice.jobs.contains_key(&job.id));
        let request = submit(backend, newer, class, 73);
        let job = backend.voice_submit(newer, request).unwrap();
        let queued = backend.voice_guard(job.id).unwrap();
        let started = backend
            .voice_drive(VoiceDriverUpdate {
                job_id: job.id,
                expected_revision: job.revision,
                generation: 0,
                action: VoiceDriverAction::Start,
            })
            .unwrap();
        assert!(
            !queued.guard.is_valid(),
            "a COMMIT invalidates the previously captured guard"
        );
        let active = backend.voice_guard(job.id).unwrap();
        assert!(active.guard.is_valid());
        let stopped = backend
            .voice_cancel(
                newer,
                VoiceCancelRequest {
                    command_id: CommandId::new(74).unwrap(),
                    job_id: job.id,
                    expected_revision: started.revision,
                },
            )
            .unwrap();
        assert_eq!(stopped.state, VoiceState::Stopping);
        assert!(!active.guard.is_valid());
        assert!(backend.voice_guard(job.id).is_err());
        backend
            .voice_drive(VoiceDriverUpdate {
                job_id: stopped.id,
                expected_revision: stopped.revision,
                generation: started.release_generation.unwrap(),
                action: VoiceDriverAction::Released,
            })
            .unwrap();
        let request = submit(backend, newer, class, 75);
        let job = backend.voice_submit(newer, request).unwrap();
        let permit = backend.voice_guard(job.id).unwrap();
        let revision = backend.voice_inventory().unwrap().revision;
        backend.voice_enabled(local, false, revision).unwrap();
        assert!(!permit.guard.is_valid());
        assert!(backend.voice_guard(job.id).is_err());
        assert_eq!(
            backend.voice_status(newer, job.id).unwrap().state,
            VoiceState::Failed
        );
        backend.voice_maintain().unwrap();
        assert!(backend.voice.jobs.is_empty());
        let revision = backend.voice_inventory().unwrap().revision;
        backend.voice_enabled(local, true, revision).unwrap();
        let request = submit(backend, newer, class, 76);
        let queued = backend.voice_submit(newer, request).unwrap();
        let starting = backend
            .voice_drive(VoiceDriverUpdate {
                job_id: queued.id,
                expected_revision: queued.revision,
                generation: 0,
                action: VoiceDriverAction::Start,
            })
            .unwrap();
        let before_deadline = backend.voice_guard(starting.id).unwrap();
        assert!(before_deadline.guard.is_valid());
        let day = clock.now().date;
        *clock.0.lock().unwrap() = LocalDateTime::new(day, 3900).unwrap();
        assert!(
            !before_deadline.guard.is_valid(),
            "source or device open crossing latest start cannot write a first buffer"
        );
    }
}
