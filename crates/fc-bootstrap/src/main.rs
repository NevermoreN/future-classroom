//! Composition root. This milestone exposes isolated diagnostics, not unfinished management APIs.
#![forbid(unsafe_code)]
#[cfg(windows)]
mod application;
#[cfg(windows)]
mod calendar_download_probe;
#[cfg(windows)]
mod credential_probe;
#[cfg(windows)]
mod material_helper;
#[cfg(windows)]
mod reinitialize_application;
#[cfg(windows)]
mod voice_pipeline_probe;
#[cfg(windows)]
mod web_pairing_probe;
use fc_domain::ids::{AuthEpoch, Revision};
use fc_runtime::authority::AuthorityStore;
use fc_storage::{CommitCommand, Mutation, StorageWriter};
use std::{
    path::PathBuf,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("FutureClassroom: {e}");
            ExitCode::FAILURE
        }
    }
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, thumbprint, marker] if flag == "--tls-business-probe" => {
            #[cfg(windows)]
            {
                application::tls_business_probe(thumbprint, marker)
            }
            #[cfg(not(windows))]
            {
                let _ = (thumbprint, marker);
                Err("native TLS business probe requires Windows".into())
            }
        }
        [flag, thumbprint, marker] if flag == "--tls-lifecycle-probe" => {
            #[cfg(windows)]
            {
                fc_windows::tls_frontdoor::probe(thumbprint, marker)
            }
            #[cfg(not(windows))]
            {
                let _ = (thumbprint, marker);
                Err("native Schannel lifecycle probe requires Windows".into())
            }
        }
        [flag] if flag == "--migration-probe" => {
            #[cfg(windows)]
            {
                application::migration_probe()
            }
            #[cfg(not(windows))]
            Err("native migration probe requires Windows".into())
        }
        [flag, root, nonce] if flag == "--migration-probe-reopen" => {
            #[cfg(windows)]
            {
                application::migration_probe_reopen(std::path::Path::new(root), nonce)
            }
            #[cfg(not(windows))]
            {
                let _ = (root, nonce);
                Err("native migration reopen probe requires Windows".into())
            }
        }
        [flag] if flag == "--reinitialization-probe" => {
            #[cfg(windows)]
            {
                application::reinitialization_probe()
            }
            #[cfg(not(windows))]
            {
                Err("native reinitialization probe requires Windows".into())
            }
        }
        [flag, root, nonce, stage] if flag == "--reinitialization-probe-stage" => {
            #[cfg(windows)]
            {
                application::reinitialization_probe_stage(std::path::Path::new(root), nonce, stage)
            }
            #[cfg(not(windows))]
            {
                let _ = (root, nonce, stage);
                Err("native reinitialization child probe requires Windows".into())
            }
        }
        [flag] if flag == "--repair" => {
            #[cfg(windows)]
            {
                reinitialize_application::run()
            }
            #[cfg(not(windows))]
            {
                Err("本机资料重新初始化向导需要 Windows".into())
            }
        }
        [flag, root, nonce] if flag == "--recovery-probe-reopen" => {
            #[cfg(windows)]
            {
                application::recovery_probe_reopen(std::path::Path::new(root), nonce)
            }
            #[cfg(not(windows))]
            {
                let _ = (root, nonce);
                Err("native recovery reopen probe requires Windows".into())
            }
        }
        [flag] if flag == "--recovery-probe" => {
            #[cfg(windows)]
            {
                application::recovery_probe()
            }
            #[cfg(not(windows))]
            {
                Err("native recovery workflow probe requires Windows".into())
            }
        }
        [flag] if flag == "--voice-settings-probe" => {
            #[cfg(windows)]
            {
                application::voice_settings_probe()
            }
            #[cfg(not(windows))]
            Err("native voice settings probe requires Windows".into())
        }
        [flag] if flag == "--autostart-probe" => {
            #[cfg(windows)]
            {
                application::autostart_probe()
            }
            #[cfg(not(windows))]
            {
                Err("native autostart workflow probe requires Windows".into())
            }
        }
        [flag] if flag == "--calendar-probe" => {
            #[cfg(windows)]
            {
                application::calendar_probe()
            }
            #[cfg(not(windows))]
            {
                Err("native calendar workflow probe requires Windows".into())
            }
        }
        [flag] if flag == "--calendar-download-probe" => {
            #[cfg(windows)]
            {
                calendar_download_probe::run()
            }
            #[cfg(not(windows))]
            {
                Err("public calendar WinHTTP probe requires Windows".into())
            }
        }
        [flag] if flag == "--web-voice-probe" => {
            #[cfg(windows)]
            {
                web_pairing_probe::run_with_voice()
            }
            #[cfg(not(windows))]
            {
                Err("Web voice native zero-PCM probe requires Windows".into())
            }
        }
        [flag] if flag == "--web-pairing-probe" => {
            #[cfg(windows)]
            {
                web_pairing_probe::run()
            }
            #[cfg(not(windows))]
            {
                Err("Web pairing DPAPI probe requires Windows".into())
            }
        }
        [flag, port, marker, seconds] if flag == "--http-iocp-probe" => {
            #[cfg(windows)]
            {
                let port = port
                    .parse::<u16>()
                    .map_err(|_| "invalid isolated HTTP port")?;
                let seconds = seconds
                    .parse::<u32>()
                    .map_err(|_| "invalid HTTP probe deadline")?;
                let prefix = fc_ports::http::ListenPrefix::isolated_loopback(port, marker)
                    .map_err(|_| "invalid isolated HTTP prefix")?;
                let reports = fc_windows::http::serve_isolated_twice(prefix, seconds)
                    .map_err(|e| format!("HTTP_IOCP_FAIL {e:?}"))?;
                for report in reports {
                    println!("HTTP_IOCP_PASS {}", report.to_json());
                }
                Ok(())
            }
            #[cfg(not(windows))]
            {
                let _ = (port, marker, seconds);
                Err("HTTP.sys probe requires Windows".into())
            }
        }
        [flag, path] if flag == "--material-open-probe-receiver" => {
            #[cfg(windows)]
            {
                material_helper::open_probe_receiver(std::path::Path::new(path))
            }
            #[cfg(not(windows))]
            {
                Err("material receiver requires Windows".into())
            }
        }
        [flag] if flag == "--audio-silent-probe" => {
            #[cfg(windows)]
            {
                match fc_windows::audio::silent_probe() {
                    Ok(report) => {
                        println!("AUDIO_SILENT_PASS {report:?}");
                        Ok(())
                    }
                    Err(fc_ports::audio::AudioOutputError::DeviceUnavailable) => {
                        println!("AUDIO_SILENT_NOT_RUN no compatible output endpoint");
                        Ok(())
                    }
                    Err(error) => Err(format!("audio silent probe failed: {error:?}")),
                }?;
                voice_pipeline_probe::run()
            }
            #[cfg(not(windows))]
            {
                Err("silent audio probe requires Windows".into())
            }
        }
        [flag] if flag == "--materials-probe" => {
            #[cfg(windows)]
            {
                application::materials_probe()
            }
            #[cfg(not(windows))]
            {
                Err("material probe requires Windows".into())
            }
        }
        [flag, path] if flag == "--material-pdf-helper" => {
            #[cfg(windows)]
            {
                material_helper::pdf(std::path::Path::new(path))
            }
            #[cfg(not(windows))]
            {
                Err("material helper requires Windows".into())
            }
        }
        [] => {
            #[cfg(windows)]
            {
                application::run(false)
            }
            #[cfg(not(windows))]
            {
                println!("未来教室 · Rust D11 开发里程碑\n这是基础验证程序，完整桌面与教师端尚未交付。\n--self-check             隔离验证 SQLite / 权威发布 / 凭据轮换\n--system-probe           只读检查 Windows 系统适配\n--host-probe <1..30>      隐藏窗口与托盘生命周期验证（秒）\n--native-probe <1..300>   Windows 原生透明绘制验证（秒）\n--version                版本及兼容状态");
                Ok(())
            }
        }
        [flag] if flag == "--autostart" => {
            #[cfg(windows)]
            {
                application::run(true)
            }
            #[cfg(not(windows))]
            Err("native application must run on Windows".into())
        }
        [flag] if flag == "--application-probe" => {
            #[cfg(windows)]
            {
                application::probe()
            }
            #[cfg(not(windows))]
            Err("native application probe must run on Windows".into())
        }
        [flag] if flag == "--version" => {
            println!(
                "FutureClassroom {} / D11 / development; Win7/Win10/Win11 product acceptance NOT_VALIDATED",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        [flag] if flag == "--self-check" => self_check(),
        [flag] if flag == "--system-probe" => {
            #[cfg(windows)]
            {
                let report = fc_windows::system::probe().map_err(debug_error)?;
                println!("system_discovery=PASS program_data_absolute={} user_sid_resolved={} thread_cpu_clock=PASS autostart_readback={:?}", report.project_data_root.is_absolute(), !report.deployment_user_sid.is_empty(), report.autostart);
                Ok(())
            }
            #[cfg(not(windows))]
            Err("system probe must run on Windows".into())
        }
        [flag, seconds] if flag == "--host-probe" => {
            let seconds = seconds
                .parse::<u32>()
                .map_err(|_| "seconds must be an integer in 1..30")?;
            #[cfg(windows)]
            {
                let report =
                    fc_windows::host::probe_lifecycle(seconds, true).map_err(debug_error)?;
                println!("hidden_host=PASS tray_lifecycle=PASS ticks={} elapsed_ms={} session_notifications={}",report.ticks,report.elapsed_millis,report.session_notifications_registered);
                Ok(())
            }
            #[cfg(not(windows))]
            {
                let _ = seconds;
                Err("host probe must run on Windows".into())
            }
        }
        [flag, seconds] if flag == "--native-probe" => {
            let seconds = seconds
                .parse::<u32>()
                .map_err(|_| "seconds must be an integer in 1..300")?;
            #[cfg(windows)]
            {
                let report =
                    fc_windows::probe(seconds).map_err(|e| format!("native probe failed: {e}"))?;
                println!("native_lifecycle=PASS elapsed_ms={} rendered_frames={} visible_samples={} dib_bytes={} program_data_resolved={} offscreen_render_verified={}",report.elapsed_ms,report.rendered_frames,report.visible_samples,report.dib_bytes,report.program_data.is_absolute(),report.offscreen_render_verified);
                if report.rendered_frames == 0 {
                    println!("desktop_render=NOT_RUN (no confirmed desktop foreground); foreground/session behavior needs interactive QA");
                }
                Ok(())
            }
            #[cfg(not(windows))]
            {
                let _ = seconds;
                Err("Windows native probe must run on Windows".into())
            }
        }
        _ => Err("unknown arguments; run without arguments for usage".into()),
    }
}

struct IsolatedRoot(PathBuf);
impl IsolatedRoot {
    fn new() -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "clock before epoch")?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Probe-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root).map_err(|_| "could not create isolated probe directory")?;
        Ok(Self(root))
    }
    fn cleanup(self) -> Result<(), String> {
        std::fs::remove_dir_all(&self.0)
            .map_err(|_| "could not remove isolated probe directory".to_owned())
    }
}
impl Drop for IsolatedRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn debug_error<T: std::fmt::Debug>(e: T) -> String {
    format!("{e:?}")
}
/// Diagnostics always join the writer before their temporary directory is removed,
/// including a failed assertion or an early `?` return.
struct ProbeWriter<S: Send + 'static>(Option<StorageWriter<S>>);
impl<S: Send + 'static> ProbeWriter<S> {
    fn handle(&self) -> Result<fc_storage::StorageHandle<S>, String> {
        self.0
            .as_ref()
            .map(StorageWriter::handle)
            .ok_or_else(|| "probe writer is closed".into())
    }
    fn shutdown(&mut self) -> Result<(), String> {
        self.0
            .take()
            .map_or(Ok(()), |writer| writer.shutdown().map_err(debug_error))
    }
}
impl<S: Send + 'static> Drop for ProbeWriter<S> {
    fn drop(&mut self) {
        if let Some(writer) = self.0.take() {
            let _ = writer.shutdown();
        }
    }
}
fn self_check() -> Result<(), String> {
    let root = IsolatedRoot::new()?;
    #[cfg(windows)]
    let lock = {
        let first = fc_windows::InstanceGuard::acquire(&root.0).map_err(debug_error)?;
        if fc_windows::InstanceGuard::acquire(&root.0).is_ok() {
            return Err("exclusive directory lock admitted second instance".into());
        }
        first
    };
    let (mut publisher, reader) =
        AuthorityStore::new("initial", Revision::new(0), AuthEpoch::new(0));
    publisher.initialize().map_err(debug_error)?;
    let mut writer = ProbeWriter(Some(
        StorageWriter::open(root.0.join("probe.db"), 128, publisher).map_err(debug_error)?,
    ));
    let handle = writer.handle()?;
    let command = || {
        CommitCommand::new(
            "probe-commit-1".into(),
            Revision::new(0),
            vec![Mutation::Put {
                key: "probe".into(),
                value: b"synthetic-isolated-data".to_vec(),
            }],
        )
    };
    let pending = reader
        .prepare_update("committed", Revision::new(1), AuthEpoch::new(0))
        .map_err(debug_error)?;
    let receipt = handle
        .try_commit(command().map_err(debug_error)?, pending)
        .map_err(debug_error)?
        .wait()
        .map_err(debug_error)?;
    if receipt.revision != Revision::new(1)
        || receipt.replayed
        || reader.snapshot().map_err(debug_error)?.data != "committed"
    {
        return Err("commit and authority disagree".into());
    }
    let replay = reader
        .prepare_update("unused", Revision::new(2), AuthEpoch::new(0))
        .map_err(debug_error)?;
    let replay = handle
        .try_commit(command().map_err(debug_error)?, replay)
        .map_err(debug_error)?
        .wait()
        .map_err(debug_error)?;
    if !replay.replayed || replay.revision != Revision::new(1) {
        return Err("idempotent replay failed".into());
    }
    let document = handle
        .try_get("probe".into())
        .map_err(debug_error)?
        .wait()
        .map_err(debug_error)?
        .ok_or("committed document missing")?;
    if document.value != b"synthetic-isolated-data"
        || reader.snapshot().map_err(debug_error)?.revision != document.revision
    {
        return Err("stored value and snapshot disagree".into());
    }
    writer.shutdown()?;
    println!("sqlite_commit=PASS authority_publish=PASS idempotency=PASS");
    #[cfg(windows)]
    {
        credential_probe::run(&root.0)?;
        use fc_ports::RandomSource;
        let mut rng = fc_windows::SystemRandom;
        let mut random = [0u8; 32];
        rng.fill(&mut random).map_err(debug_error)?;
        let password = fc_security::Password::new("仅用于隔离密码学验证的口令2026".into())
            .map_err(debug_error)?;
        let record =
            fc_security::create_password_record(&password, &mut rng).map_err(debug_error)?;
        if !fc_security::verify_password(&password, &record).map_err(debug_error)? {
            return Err("password self-check failed".into());
        }
        println!("system_csprng=PASS pbkdf2_600000=PASS directory_lock=PASS");
        drop(lock);
    }
    #[cfg(not(windows))]
    println!("system_csprng=NOT_RUN directory_lock=NOT_RUN (Windows required)");
    drop(handle);
    root.cleanup()?;
    println!("isolated_test_cleanup=PASS");
    Ok(())
}
