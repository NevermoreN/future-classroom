//! Composition root. This milestone exposes isolated diagnostics, not unfinished management APIs.
#![forbid(unsafe_code)]
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
        [] => {
            println!("未来教室 · Rust D11 开发里程碑\n这是基础验证程序，完整桌面与教师端尚未交付。\n--self-check             隔离验证 SQLite / 权威发布 / 密码学\n--native-probe <1..300>   Windows 原生透明绘制验证（秒）\n--version                版本及兼容状态");
            Ok(())
        }
        [flag] if flag == "--version" => {
            println!(
                "FutureClassroom {} / D11 / development; Win7 NOT_VALIDATED",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        [flag] if flag == "--self-check" => self_check(),
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
    let writer =
        StorageWriter::open(root.0.join("probe.db"), 128, publisher).map_err(debug_error)?;
    let handle = writer.handle();
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
    writer.shutdown().map_err(debug_error)?;
    println!("sqlite_commit=PASS authority_publish=PASS idempotency=PASS");
    #[cfg(windows)]
    {
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
