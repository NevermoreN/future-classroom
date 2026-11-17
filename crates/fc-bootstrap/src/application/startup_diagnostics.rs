//! Small private failure reports. A successful launch never erases evidence.
use fc_windows::{materials::files::ControlledFiles, system::DataPaths};
use std::{
    io::Write,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const REPORT_NAMES: [&str; 8] = [
    "startup-failure-01.log",
    "startup-failure-02.log",
    "startup-failure-03.log",
    "startup-failure-04.log",
    "startup-failure-05.log",
    "startup-failure-06.log",
    "startup-failure-07.log",
    "startup-failure-08.log",
];

/// The caller already holds the checked runtime directory and InstanceGuard.
/// Detail comes only from contextual native host errors, never user input or
/// school paths. This adds no account, command line, or environment information.
pub(super) fn host_failure(
    paths: &DataPaths,
    autostart: bool,
    probe: bool,
    stage: &str,
    code: u32,
    detail: &str,
    elapsed: Duration,
) -> String {
    let detail: String = detail
        .chars()
        .filter(|ch| !ch.is_control())
        .take(256)
        .collect();
    let mode = if probe {
        "probe"
    } else if autostart {
        "autostart"
    } else {
        "manual"
    };
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let report = format!(
        "FutureClassroom startup failure\nversion={}\nmode={mode}\nstage={stage}\nhresult=0x{code:08X}\nunix_seconds={at}\nelapsed_ms={}\ndetail={detail}\n",
        fc_domain::PRODUCT_DISPLAY_VERSION,
        elapsed.as_millis(),
    );
    // Each report is <= 2 KiB, with only eight fixed slots. Never follow,
    // truncate, delete or overwrite an existing object, even when all are full.
    let saved = (report.len() <= 2048)
        .then(|| ControlledFiles::new(&paths.runtime()).ok())
        .flatten()
        .and_then(|files| {
            for name in REPORT_NAMES {
                let Ok(mut file) = files.create_new(&paths.runtime().join(name)) else {
                    continue;
                };
                if file.file_mut().write_all(report.as_bytes()).is_err() {
                    return None;
                }
                return file.finish().ok().map(|_| name);
            }
            None
        });
    let evidence = match saved {
        Some(name) => format!("诊断记录已保存在程序数据目录的 runtime/{name}。"),
        None => "未能保存诊断记录（目录不可写或记录数量已达上限）；原有记录保持不变。".into(),
    };
    format!(
        "未来教室的 Windows 托盘与会话准备失败：{detail}（0x{code:08X}）。\n{evidence}\n请稍后重新打开；若仍然出现，请联系管理员并提供本提示和诊断记录。"
    )
}
