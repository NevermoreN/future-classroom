//! Native-domain retirement after real reviewed policy commits. These tests
//! construct no output device; physical playing/release is a separate probe.
use super::*;
use crate::backend::{Backend, Initialize};
use fc_application::identity::SessionLease;
use fc_security::{CredentialRecordCodec, Password};
use fc_windows::{system::DataPaths, SystemRandom};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
    backend: Option<Backend>,
    lease: SessionLease,
    clock: Arc<CurrentClock>,
}
impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-NativePolicy-{}-{nonce}",
            std::process::id()
        ));
        let paths = DataPaths::isolated_test(root.clone()).unwrap();
        let deployment = paths.prepare_for_current_user().unwrap();
        let clock = Arc::new(CurrentClock::new());
        let codec = CredentialRecordCodec::generate(&mut SystemRandom).unwrap();
        let mut backend = Backend::open(
            root.join("state/native-policy.sqlite"),
            codec,
            clock.clone(),
            clock.clone(),
        )
        .unwrap();
        let password = || Password::new("Synthetic native policy fixture 2026!".into()).unwrap();
        drop(
            backend
                .initialize(
                    Initialize {
                        school_name: "Synthetic native policy".into(),
                        login_name: "policy".into(),
                        display_name: "Policy".into(),
                    },
                    password(),
                )
                .unwrap(),
        );
        let lease = backend.login("policy", password()).unwrap();
        // No device or deployment handle is retained by this test fixture.
        drop(deployment);
        Self {
            root,
            backend: Some(backend),
            lease,
            clock,
        }
    }
    fn service(&self) -> VoiceService {
        self.backend.as_ref().unwrap().voice_service()
    }
    fn domain(&self) -> Box<dyn AudioDomain> {
        let factory = Arc::new(NativeFactory::new(
            self.service(),
            self.root.clone(),
            self.clock.clone(),
        ));
        let runtime = VoiceRuntime::new(factory.clone());
        let domain = factory.create(runtime.control()).unwrap();
        // The direct empty-domain test deliberately has no physical thread.
        // A known startup fence is opened before capturing the policy review.
        let service = self.service();
        service.resume_after_stop(service.generation()).unwrap();
        domain
    }
    fn save(&self) {
        let service = self.service();
        let review = service.management_snapshot(self.lease).unwrap();
        let saved = service
            .set_limits_reviewed(
                self.lease,
                VoiceLimits {
                    max_duration_millis: 1000,
                    max_pending_jobs: 2,
                    max_pending_per_owner: 1,
                    max_repeat_total: 2,
                },
                review.revision,
                review.identity_revision,
                review.runtime_generation,
            )
            .unwrap();
        assert_eq!(saved.limits.max_duration_millis, 1000);
        assert!(saved.enabled);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut backend) = self.backend.take() {
            let closed = backend.close();
            if !std::thread::panicking() {
                closed.unwrap();
            }
            drop(backend);
        }
        let removed = std::fs::remove_dir_all(&self.root);
        if !std::thread::panicking() {
            removed.unwrap();
        }
    }
}

#[test]
fn reviewed_policy_commit_allows_exact_old_native_domain_to_retire_without_fault() {
    let fixture = Fixture::new();
    let mut old = fixture.domain();
    assert!(!old.has_native_ownership());
    fixture.save();
    assert!(old.finish().unwrap());
    assert_eq!(
        old.resume_admission(),
        Ok(()),
        "a committed reviewed policy must not strand its old native domain"
    );
    drop(old);
}

#[test]
fn later_independent_stop_cannot_be_reopened_by_old_native_policy_retirement() {
    let fixture = Fixture::new();
    let mut old = fixture.domain();
    fixture.save();
    let service = fixture.service();
    service.invalidate_runtime();
    assert!(!service.is_admitting());
    assert!(old.finish().unwrap());
    assert!(old.resume_admission().is_err());
    assert!(!service.is_admitting());
    drop(old);
}

#[test]
fn two_contiguous_committed_policies_use_one_bounded_old_domain_handoff() {
    let fixture = Fixture::new();
    let mut old = fixture.domain();
    let service = fixture.service();
    let original = service.generation();
    fixture.save();
    fixture.save();
    let receipt = service.policy_handoff(original).unwrap().unwrap();
    assert_eq!(receipt.phase, VoicePolicyPhase::Committed);
    assert_eq!(receipt.generation, original + 2);
    assert!(old.finish().unwrap());
    assert_eq!(old.resume_admission(), Ok(()));
    assert!(service.is_admitting());
    drop(old);
}

#[test]
fn disabled_committed_policy_retires_old_domain_and_keeps_admission_closed() {
    let fixture = Fixture::new();
    let mut old = fixture.domain();
    let service = fixture.service();
    let review = service.management_snapshot(fixture.lease).unwrap();
    service
        .set_enabled_reviewed(
            fixture.lease,
            false,
            review.revision,
            review.identity_revision,
            review.runtime_generation,
        )
        .unwrap();
    assert!(!service.is_admitting());
    assert!(old.finish().unwrap());
    assert_eq!(old.resume_admission(), Ok(()));
    assert!(!service.is_admitting());
    drop(old);
}

#[test]
fn failed_reviewed_policy_authorizes_only_negative_retirement_and_never_resume() {
    let fixture = Fixture::new();
    let mut old = fixture.domain();
    let service = fixture.service();
    let control = fixture.backend.as_ref().unwrap().session_control();
    let review = service.management_snapshot(fixture.lease).unwrap();
    service
        .set_waker(Arc::new(move || {
            control.lock_local();
        }))
        .unwrap();
    let result = service.set_limits_reviewed(
        fixture.lease,
        VoiceLimits {
            max_repeat_total: 2,
            ..VoiceLimits::default()
        },
        review.revision,
        review.identity_revision,
        review.runtime_generation,
    );
    service.clear_waker().unwrap();
    assert!(result.is_err());
    let receipt = service
        .policy_handoff(review.runtime_generation)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, VoicePolicyPhase::Failed);
    assert!(!service.is_admitting());
    assert_eq!(old.step(), Ok(DomainProgress::Idle));
    assert!(old.finish().unwrap());
    assert_eq!(old.resume_admission(), Ok(()));
    assert!(!service.is_admitting());
    drop(old);
}

#[test]
fn independent_stop_gap_between_policies_never_extends_old_retirement_permission() {
    let fixture = Fixture::new();
    let mut old = fixture.domain();
    let service = fixture.service();
    let original = service.generation();
    fixture.save();
    service.invalidate_runtime();
    fixture.save();
    assert!(service.policy_handoff(original).unwrap().is_none());
    assert!(old.finish().unwrap());
    assert!(old.resume_admission().is_err());
    drop(old);
}

#[derive(Default)]
struct PendingGate {
    entered: AtomicBool,
    released: Mutex<bool>,
    signal: Condvar,
}
impl PendingGate {
    fn wait(&self) {
        self.entered.store(true, Ordering::Release);
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.signal.wait(released).unwrap();
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.signal.notify_all();
    }
}
struct PolicyWorker {
    gate: Arc<PendingGate>,
    thread: Option<std::thread::JoinHandle<crate::backend::Result<VoiceSnapshot>>>,
}
impl Drop for PolicyWorker {
    fn drop(&mut self) {
        self.gate.release();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn pending_policy_stops_native_domain_without_sql_or_retiring_before_commit() {
    let fixture = Fixture::new();
    let mut old = fixture.domain();
    let service = fixture.service();
    let review = service.management_snapshot(fixture.lease).unwrap();
    let before = service.inventory().unwrap();
    let gate = Arc::new(PendingGate::default());
    let entered = gate.clone();
    service.set_waker(Arc::new(move || entered.wait())).unwrap();
    let committing = service.clone();
    let lease = fixture.lease;
    let mut worker = PolicyWorker {
        gate: gate.clone(),
        thread: Some(std::thread::spawn(move || {
            committing.set_limits_reviewed(
                lease,
                VoiceLimits {
                    max_repeat_total: 2,
                    ..VoiceLimits::default()
                },
                review.revision,
                review.identity_revision,
                review.runtime_generation,
            )
        })),
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !gate.entered.load(Ordering::Acquire) {
        assert!(
            std::time::Instant::now() < deadline,
            "policy did not publish its pre-writer Pending receipt"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(old.step(), Ok(DomainProgress::Draining));
    assert!(!old.has_native_ownership());
    assert!(!service.is_admitting());
    assert_eq!(service.inventory().unwrap(), before);
    gate.release();
    worker.thread.take().unwrap().join().unwrap().unwrap();
    service.clear_waker().unwrap();
    assert_eq!(old.step(), Ok(DomainProgress::Idle));
    assert!(old.finish().unwrap());
    assert_eq!(old.resume_admission(), Ok(()));
    drop(old);
}

#[test]
fn failed_policy_supervisor_retires_physical_thread_without_self_wake_loop() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let runtime = VoiceRuntime::new(Arc::new(NativeFactory::new(
        service.clone(),
        fixture.root.clone(),
        fixture.clock.clone(),
    )));
    service.set_waker(runtime.weak_waker()).unwrap();
    runtime.set_available(true).unwrap();
    let wait = |predicate: &dyn Fn(RuntimeStatus) -> bool, millis: u64| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(millis);
        loop {
            let _ = runtime.reap_nonblocking();
            let status = runtime.status().unwrap();
            if predicate(status) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    };
    let startup = wait(
        &|status| {
            status.phase == RuntimePhase::Idle
                && !status.physical_thread_running
                && service.is_admitting()
        },
        3000,
    );
    let review = service.management_snapshot(fixture.lease).unwrap();
    let control = fixture.backend.as_ref().unwrap().session_control();
    let wake = runtime.weak_waker();
    service
        .set_waker(Arc::new(move || {
            control.lock_local();
            wake();
        }))
        .unwrap();
    let result = service.set_limits_reviewed(
        fixture.lease,
        VoiceLimits {
            max_repeat_total: 2,
            ..VoiceLimits::default()
        },
        review.revision,
        review.identity_revision,
        review.runtime_generation,
    );
    let failed_receipt = service
        .policy_handoff(review.runtime_generation)
        .unwrap()
        .is_some_and(|receipt| receipt.phase == VoicePolicyPhase::Failed);
    // A real on-demand OS thread must finish and be reaped while this Failed
    // receipt still governs the service. A repeated interrupt_all wake prevents
    // the supervisor's stable-wake retirement condition from ever succeeding.
    let retired = wait(
        &|status| status.phase == RuntimePhase::Idle && !status.physical_thread_running,
        2000,
    );
    let remained_closed = !service.is_admitting();
    // Settle our own actual thread even when the desired assertion fails. This
    // independent Shutdown epoch ends the historical loop without deleting a
    // live domain, touching another session, or leaving a background test job.
    runtime.shutdown();
    let closed = wait(
        &|status| status.phase == RuntimePhase::Closed && !status.physical_thread_running,
        3000,
    );
    service.clear_waker().unwrap();
    assert!(
        closed,
        "isolated failed-policy runtime did not finish shutdown"
    );
    drop(runtime);
    assert!(startup);
    assert!(result.is_err());
    assert!(failed_receipt);
    assert!(remained_closed);
    assert!(
        retired,
        "Failed policy caused the real audio supervisor to wake itself forever instead of retiring"
    );
}

struct MetadataPreparation;
impl fc_ports::voice::VoicePreparation for MetadataPreparation {
    type Input = ();
    fn prepare(&mut self, _: (), _: &VoiceAsset) -> Result<VoicePcmMetadata, VoiceRepositoryError> {
        // Trusted synthetic metadata only: this regression never opens an output
        // device. The separate zero-PCM probe validates actual file/device I/O.
        Ok(VoicePcmMetadata {
            byte_length: 16_044,
            frame_count: 8_000,
            sha256: [17; 32],
        })
    }
}
fn explicit_job(
    service: &VoiceService,
    web: SessionLease,
    class: fc_domain::ids::ClassId,
) -> VoiceJob {
    let reservation = service.reserve(web, class).unwrap();
    let proof =
        fc_ports::voice::prepare_voice_upload(&mut MetadataPreparation, (), reservation).unwrap();
    let asset = service.publish(web, proof).unwrap();
    service
        .submit(
            web,
            fc_ports::voice::VoiceSubmitRequest {
                command_id: fc_domain::ids::CommandId::new(5001).unwrap(),
                asset_id: asset.id,
                expected_asset_revision: asset.revision,
                class_id: class,
                repeat_total: 1,
            },
        )
        .unwrap()
}

#[test]
fn reviewed_idle_domain_never_strands_fresh_explicit_job_before_old_retirement() {
    let mut fixture = Fixture::new();
    let class = fc_domain::ids::ClassId::new(1).unwrap();
    let backend = fixture.backend.as_mut().unwrap();
    backend
        .identity(
            fixture.lease,
            fc_domain::identity::IdentityCommand::CreateClass {
                id: class,
                name: "Synthetic policy class".into(),
            },
        )
        .unwrap();
    backend
        .identity(
            fixture.lease,
            fc_domain::identity::IdentityCommand::BindCurrentClass { id: class },
        )
        .unwrap();
    let web = backend
        .login_origin(
            "policy",
            Password::new("Synthetic native policy fixture 2026!".into()).unwrap(),
            fc_domain::auth::TrustedOrigin::Web,
        )
        .unwrap();
    let mut old = fixture.domain();
    let service = fixture.service();
    fixture.save();
    // Either a closed participation fence defers all fresh admissions until
    // retirement, or an already admitted fresh task must survive old-domain
    // retirement. A halted old executor may never strand that task forever.
    let admitted_early = service.is_admitting();
    let early = if admitted_early {
        Some(explicit_job(&service, web, class))
    } else {
        assert!(service.reserve(web, class).is_err());
        None
    };
    let progress = old.step();
    let early_preserved = early
        .as_ref()
        .is_none_or(|job| service.status(web, job.id).unwrap().state == VoiceState::Queued);
    assert!(!old.has_native_ownership());
    assert!(old.finish().unwrap());
    old.resume_admission().unwrap();
    drop(old);
    let fresh = early.unwrap_or_else(|| explicit_job(&service, web, class));
    let guard = service
        .runtime_handle()
        .guard_for(
            &service
                .inventory()
                .unwrap()
                .jobs
                .into_iter()
                .find(|job| job.id == fresh.id)
                .unwrap(),
        )
        .unwrap();
    assert!(guard.guard.is_valid());
    assert_eq!(
        service.status(web, fresh.id).unwrap().state,
        VoiceState::Queued
    );
    // This last assertion reports the before-fix failure only after the old
    // native object has been dropped and all synthetic metadata is owned by the
    // fixture's deterministic writer/directory cleanup.
    assert!(early_preserved);
    assert_eq!(
        progress,
        Ok(DomainProgress::Idle),
        "old policy-stopped domain stranded a new explicit queue instead of retiring"
    );
}
