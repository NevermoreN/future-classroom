//! Explicit isolated vertical diagnostic. Generates only silence, never records,
//! changes volume/foreground state, or runs a recovered notification.
use fc_audio::execution::{ExecutionError, VoiceExecutionPort, VoiceExecutor};
use fc_bootstrap::{
    backend::{Backend, Initialize, VoicePlaybackAuthorization},
    voice_files::ControlledVoiceFiles,
    voice_runtime::{NativeFactory, RuntimePhase, StopReason, VoiceRuntime},
};
use fc_domain::{auth::TrustedOrigin, identity::IdentityCommand, ids::*, voice::*};
use fc_ports::{
    audio::{AudioAuthorization, AudioOutputError},
    voice::*,
    Clock, MonotonicClock,
};
use fc_security::{CredentialRecordCodec, Password};
use fc_windows::{
    audio::{VerifiedWave, WinMmAudioOutput},
    materials::files::LockedFile,
    system::{CurrentClock, DataPaths},
    SystemRandom,
};
use std::{
    cell::Cell,
    fs,
    io::Write,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
thread_local! {
    static PHASE: Cell<&'static str> = const { Cell::new("startup") };
}
fn phase(value: &'static str) {
    if PHASE.with(|phase| phase.replace(value)) != value {
        eprintln!("VOICE_PIPELINE_STAGE {value}");
    }
}
fn check<T, E: std::fmt::Debug>(r: Result<T, E>) -> Result<T, String> {
    r.map_err(|e| PHASE.with(|phase| format!("voice-pipeline/{}: {e:?}", phase.get())))
}
fn repo<T>(r: fc_bootstrap::backend::Result<T>) -> Result<T, VoiceRepositoryError> {
    r.map_err(|_| VoiceRepositoryError::Unavailable)
}
struct Port<'a> {
    backend: &'a mut Backend,
    files: &'a ControlledVoiceFiles,
    authorization: Option<VoicePlaybackAuthorization>,
    clock: &'a CurrentClock,
}
impl Port<'_> {
    fn refresh(&mut self, id: VoiceJobId) {
        self.authorization = self.backend.voice_guard(id).ok();
    }
}
impl VoiceExecutionPort<VerifiedWave> for Port<'_> {
    fn civil_seconds(&self) -> i64 {
        civil_seconds(self.clock.now())
    }
    fn monotonic_millis(&self) -> u64 {
        self.clock.monotonic_millis()
    }
    fn inventory(&mut self) -> Result<VoiceSnapshot, VoiceRepositoryError> {
        let snapshot = repo(self.backend.voice_inventory())?;
        self.authorization = None;
        if let Some(job) = snapshot
            .jobs
            .iter()
            .filter(|j| !j.state.terminal())
            .min_by_key(|j| j.sequence)
        {
            self.refresh(job.id);
        }
        Ok(snapshot)
    }
    fn drive(&mut self, update: VoiceDriverUpdate) -> Result<VoiceJob, VoiceRepositoryError> {
        let job = repo(self.backend.voice_drive(update))?;
        self.refresh(job.id);
        Ok(job)
    }
    fn authorize(&mut self, job: &VoiceJob) -> AudioAuthorization {
        self.authorization
            .as_ref()
            .map_or(AudioAuthorization::Revoked, |a| {
                if a.job.id == job.id
                    && a.job.generation == job.generation
                    && a.job.revision == job.revision
                    && a.job.state == job.state
                {
                    a.guard.decision()
                } else {
                    AudioAuthorization::Revoked
                }
            })
    }
    fn source(&mut self, asset: &VoiceAsset) -> Result<VerifiedWave, VoiceRepositoryError> {
        self.files
            .source(asset)
            .map_err(|_| VoiceRepositoryError::Domain(VoiceError::InvalidState))
    }
}
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn run() -> Result<(), String> {
    std::thread::Builder::new()
        .name("fc-voice-pipeline-probe".into())
        .spawn(run_owned)
        .map_err(|_| "voice probe thread unavailable")?
        .join()
        .map_err(|_| "voice probe panicked")?
}
fn run_owned() -> Result<(), String> {
    phase("deployment");
    let stamp = check(SystemTime::now().duration_since(UNIX_EPOCH))?.as_nanos();
    let root = Cleanup(std::env::temp_dir().join(format!(
        "FutureClassroom-Test-Voice-{}-{stamp}",
        std::process::id()
    )));
    let paths = check(DataPaths::isolated_test(root.0.clone()))?;
    let deployment = check(paths.prepare_for_current_user())?;
    let codec = check(CredentialRecordCodec::generate(&mut SystemRandom))?;
    let clock = Arc::new(CurrentClock::new());
    phase("backend-open");
    let mut backend = check(Backend::open(
        root.0.join("state/voice-probe.sqlite"),
        codec,
        clock.clone(),
        clock.clone(),
    ))?;
    const PASSWORD: &str = "Synthetic voice probe password 2026!";
    phase("initialize");
    drop(check(backend.initialize(
        Initialize {
            school_name: "Synthetic voice school".into(),
            login_name: "probe".into(),
            display_name: "Probe".into(),
        },
        check(Password::new(PASSWORD.into()))?,
    ))?);
    phase("local-login");
    let local = check(backend.login("probe", check(Password::new(PASSWORD.into()))?))?;
    let class = check(ClassId::new(1))?;
    phase("create-class");
    check(backend.identity(
        local,
        IdentityCommand::CreateClass {
            id: class,
            name: "Synthetic voice class".into(),
        },
    ))?;
    phase("bind-class");
    check(backend.identity(local, IdentityCommand::BindCurrentClass { id: class }))?;
    phase("web-login");
    let web = check(backend.login_origin(
        "probe",
        check(Password::new(PASSWORD.into()))?,
        TrustedOrigin::Web,
    ))?;
    let files = check(ControlledVoiceFiles::new(&root.0))?;
    let input = root.0.join("voice-input.wav");
    let data_bytes = 16_000u32; // 0.5s, every PCM sample is zero.
    let mut h = [0u8; 44];
    h[..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(data_bytes + 36).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&16_000u32.to_le_bytes());
    h[28..32].copy_from_slice(&32_000u32.to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    {
        let mut out = check(fs::File::create(&input))?;
        check(out.write_all(&h))?;
        check(out.write_all(&[0; 16_000]))?;
        check(out.sync_all())?;
    }
    let mut output = VoiceExecutor::new(WinMmAudioOutput::new());
    let mut unavailable = false;
    for (index, total, cancel) in [(1, 2, false), (2, 3, true)] {
        phase("direct-reserve");
        let reservation = check(backend.voice_reserve(web, class))?;
        let authorize = || {
            backend
                .check_session(web, fc_application::identity::SessionPurpose::Manage)
                .is_ok()
        };
        let pending = if index == 1 {
            check(files.prepare_body(check(fs::File::open(&input))?, reservation, &authorize))?
        } else {
            check(files.prepare(
                check(LockedFile::open(&input, MAX_WAV_BYTES))?,
                reservation,
                &authorize,
            ))?
        };
        let (prepared, pin) = pending.into_parts();
        phase("direct-publish");
        let asset = check(backend.voice_publish(web, prepared))?;
        drop(pin);
        phase("direct-submit");
        let job = check(backend.voice_submit(
            web,
            VoiceSubmitRequest {
                command_id: check(CommandId::new(index))?,
                asset_id: asset.id,
                expected_asset_revision: asset.revision,
                class_id: class,
                repeat_total: total,
            },
        ))?;
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut did_cancel = false;
        loop {
            let result = {
                let mut port = Port {
                    backend: &mut backend,
                    files: &files,
                    authorization: None,
                    clock: &clock,
                };
                output.tick(
                    &mut port,
                    civil_seconds(clock.now()),
                    clock.monotonic_millis(),
                    20,
                )
            };
            if let Err(error) = result {
                if error == ExecutionError::Output(AudioOutputError::DeviceUnavailable) {
                    unavailable = true;
                    break;
                }
                return Err(format!("voice executor failed: {error:?}"));
            }
            phase("direct-status");
            let current = check(backend.voice_status(web, job.id))?;
            if current.authorization.is_some() {
                return Err("public voice status leaked private authorization".into());
            }
            if cancel && !did_cancel && current.state == VoiceState::Playing {
                check(backend.voice_cancel(
                    web,
                    VoiceCancelRequest {
                        command_id: check(CommandId::new(100))?,
                        job_id: job.id,
                        expected_revision: current.revision,
                    },
                ))?;
                did_cancel = true;
            }
            if current.state.terminal() {
                let expected = if cancel {
                    VoiceState::Canceled
                } else {
                    VoiceState::Completed
                };
                if current.state != expected
                    || current.completed_repeats != if cancel { 0 } else { total }
                    || output.has_native_ownership()
                {
                    return Err("voice final state/repeat/release mismatch".into());
                }
                break;
            }
            if Instant::now() >= deadline {
                return Err("voice pipeline timeout".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        phase("direct-cleanup");
        check(files.reconcile(&mut backend))?;
        if unavailable {
            break;
        }
    }
    drop(output);
    if !unavailable {
        normal_runtime_probe(
            &mut backend,
            web,
            class,
            &files,
            &input,
            &root.0,
            clock.clone(),
        )?;
    } else {
        println!("VOICE_RUNTIME_NOT_RUN no compatible output endpoint");
    }
    phase("final-cleanup");
    check(files.reconcile(&mut backend))?;
    let remaining = check(backend.voice_inventory())?;
    if !remaining.assets.is_empty() || remaining.jobs.iter().any(|j| !j.state.terminal()) {
        return Err("voice active references remain".into());
    }
    phase("backend-close");
    check(backend.close())?;
    drop(backend);
    drop(deployment);
    check(fs::remove_dir_all(&root.0))?;
    if unavailable {
        println!("VOICE_PIPELINE_NOT_RUN no compatible output endpoint; durable cleanup=PASS");
    } else {
        println!("VOICE_PIPELINE_PASS canonical_zero_pcm_prepare_exact_web_lease_sqlite_fifo_two_natural_repeats_cancel_generation_native_release_body_cleanup=PASS");
    }
    Ok(())
}

/// Exercise the same on-demand supervisor as normal composition. All WAV bytes
/// are the already-created canonical zero PCM fixture; no microphone, volume,
/// foreground changes or unrelated sessions are involved.
fn normal_runtime_probe(
    backend: &mut Backend,
    web: fc_application::identity::SessionLease,
    class: ClassId,
    files: &ControlledVoiceFiles,
    input: &std::path::Path,
    root: &std::path::Path,
    clock: Arc<CurrentClock>,
) -> Result<(), String> {
    phase("runtime-create");
    let service = backend.voice_service();
    let runtime = VoiceRuntime::new(Arc::new(NativeFactory::new(
        service.clone(),
        root.to_owned(),
        clock,
    )));
    check(service.set_waker(runtime.weak_waker()))?;
    check(runtime.set_available(true))?;
    fn poll(runtime: &VoiceRuntime) -> Result<fc_bootstrap::voice_runtime::RuntimeStatus, String> {
        check(runtime.reap_nonblocking())?;
        let status = check(runtime.status())?;
        if status.faulted {
            return Err(format!("normal audio runtime faulted: {status:?}"));
        }
        Ok(status)
    }
    let idle = |runtime: &VoiceRuntime| -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let status = poll(runtime)?;
            if status.phase == RuntimePhase::Idle && !status.physical_thread_running {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!("audio idle timeout: {status:?}"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    phase("runtime-startup-fence");
    idle(&runtime)?;
    if !service.is_admitting() {
        return Err("audio startup fence did not admit fresh work".into());
    }
    let prepare_asset = |backend: &mut Backend| -> Result<VoiceAsset, String> {
        phase("runtime-reserve");
        let reservation = check(backend.voice_reserve(web, class))?;
        let pending = check(files.prepare_body(
            check(fs::File::open(input))?,
            reservation,
            &|| service.check_web_session(web).is_ok(),
        ))?;
        let (prepared, pin) = pending.into_parts();
        phase("runtime-publish");
        let asset = check(backend.voice_publish(web, prepared))?;
        drop(pin);
        Ok(asset)
    };
    let submit = |backend: &mut Backend,
                  asset: VoiceAsset,
                  id: u64,
                  total: u8|
     -> Result<VoiceJob, String> {
        phase("runtime-submit");
        check(backend.voice_submit(
            web,
            VoiceSubmitRequest {
                command_id: check(CommandId::new(id))?,
                asset_id: asset.id,
                expected_asset_revision: asset.revision,
                class_id: class,
                repeat_total: total,
            },
        ))
    };
    // Closing/locking local management must not revoke an independent Web job.
    backend.session_control().lock_local();
    let first_asset = prepare_asset(backend)?;
    let first = submit(backend, first_asset, 201, 2)?;
    phase("runtime-first-job");
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        poll(&runtime)?;
        let current = check(service.status(web, first.id))?;
        if current.state.terminal() {
            if current.state != VoiceState::Completed || current.completed_repeats != 2 {
                return Err(format!(
                    "local lock interrupted Web audio: {:?}",
                    current.state
                ));
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err("normal runtime natural playback timeout".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    idle(&runtime)?;
    // Finish filesystem preparation before waking either job. The stop assertion
    // permits already completed repeats of the active job, never a terminal one.
    let active_asset = prepare_asset(backend)?;
    let queued_asset = prepare_asset(backend)?;
    let active = submit(backend, active_asset, 202, 10)?;
    let queued = submit(backend, queued_asset, 203, 3)?;
    phase("runtime-await-emergency");
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        poll(&runtime)?;
        let current = check(service.status(web, active.id))?;
        if current.state == VoiceState::Playing {
            break;
        }
        if current.state.terminal() || Instant::now() >= deadline {
            return Err("normal runtime did not start fresh zero PCM".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    runtime.request_stop(StopReason::Emergency);
    phase("runtime-emergency-fence");
    idle(&runtime)?;
    for id in [active.id, queued.id] {
        let job = check(service.status(web, id))?;
        if job.state != VoiceState::Interrupted
            || job.completed_repeats >= job.repeat_total
            || (id == queued.id && job.completed_repeats != 0)
        {
            return Err(format!(
                "emergency did not retire entire old queue: {:?}",
                job.state
            ));
        }
    }
    // A new explicit submission can wake a new physical thread after clean stop;
    // neither an old task nor a retained driver is reopened automatically.
    let fresh_asset = prepare_asset(backend)?;
    let fresh = submit(backend, fresh_asset, 204, 1)?;
    phase("runtime-fresh-job");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        poll(&runtime)?;
        let current = check(service.status(web, fresh.id))?;
        if current.state.terminal() {
            if current.state != VoiceState::Completed || current.completed_repeats != 1 {
                return Err("fresh audio after clean stop failed".into());
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err("fresh audio timeout".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    idle(&runtime)?;
    runtime.shutdown();
    phase("runtime-shutdown");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = poll(&runtime)?;
        if status.phase == RuntimePhase::Closed && !status.physical_thread_running {
            println!("VOICE_RUNTIME_PASS on_demand_local_lock_independent_two_repeats_emergency_all_pending_fresh_explicit_job_shutdown_before_writer threads={} stop_generation={} ack_generation={}",status.physical_threads_started,status.stop_generation,status.ack_generation);
            break;
        }
        if Instant::now() >= deadline {
            return Err(format!("normal audio shutdown timeout: {status:?}"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
