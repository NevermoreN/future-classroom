//! Private, bounded diagnostics while the instance lock and runtime pin are held.
use fc_windows::{
    materials::files::{ControlledFiles, LockedFile},
    system::DataPaths,
};
use std::{
    collections::VecDeque,
    fmt::Write as _,
    io::{Read, Write as _},
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const HEADER: &[u8] = b"FutureClassroom runtime diagnostic\n";
const MAX_REPORT_BYTES: usize = 2048;
const REPORT_SLOTS: usize = 8;
const MAX_EVENTS: usize = 16;

pub(super) struct RuntimeDiagnostics<'a> {
    paths: &'a DataPaths,
    mode: &'static str,
    started: Instant,
    run_id: String,
    events: VecDeque<(u128, &'static str, Option<u32>)>,
    first_failure: Option<(&'static str, u32)>,
}

impl<'a> RuntimeDiagnostics<'a> {
    pub(super) fn new(paths: &'a DataPaths, autostart: bool, probe: bool) -> Self {
        let mode = if probe {
            "probe"
        } else if autostart {
            "autostart"
        } else {
            "manual"
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            paths,
            mode,
            started: Instant::now(),
            run_id: format!("{}-{}", now.as_millis(), std::process::id()),
            events: VecDeque::with_capacity(MAX_EVENTS),
            first_failure: None,
        }
    }

    pub(super) fn note(&mut self, event: &'static str, code: Option<u32>) {
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events
            .push_back((self.started.elapsed().as_millis(), event, code));
    }

    pub(super) fn warning(&self, stage: &'static str, code: u32) {
        self.save(stage, code, "warning");
    }

    pub(super) fn failure(&mut self, stage: &'static str, code: u32) {
        self.first_failure.get_or_insert((stage, code));
        self.note(stage, Some(code));
    }

    /// `detail` is for the local user prompt. Only fixed labels and codes are
    /// written to disk, even if a caller's error contains a filename or input.
    pub(super) fn report_failure(
        &mut self,
        stage: &'static str,
        code: u32,
        detail: &str,
    ) -> String {
        self.failure(stage, code);
        let evidence = match self.save(stage, code, "failure") {
            Some(name) => format!("诊断记录已保存在程序数据目录的 runtime/{name}。"),
            None => "未能写入新的运行诊断记录，请核对程序数据目录权限。".into(),
        };
        format!(
            "未来教室运行遇到问题。{detail}（0x{code:08X}）。\n{evidence}\n诊断编号 {}。若再次出现，请提供这个编号和诊断记录。",
            self.run_id,
        )
    }

    pub(super) fn flush_failure(&self) {
        if let Some((stage, code)) = self.first_failure {
            self.save(stage, code, "failure");
        }
    }

    fn save(&self, stage: &str, code: u32, severity: &str) -> Option<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let mut report = format!(
            "FutureClassroom runtime diagnostic\nversion={}\nrun_id={}\nmode={}\nseverity={severity}\nstage={stage}\nhresult=0x{code:08X}\nunix_seconds={}\nelapsed_ms={}\n",
            fc_domain::PRODUCT_DISPLAY_VERSION, self.run_id, self.mode, now.as_secs(),
            self.started.elapsed().as_millis(),
        );
        if let Some((first, code)) = self.first_failure {
            let _ = writeln!(report, "first_failure={first} 0x{code:08X}");
        }
        for (elapsed, event, code) in &self.events {
            match code {
                Some(code) => writeln!(report, "event={elapsed} {event} 0x{code:08X}"),
                None => writeln!(report, "event={elapsed} {event}"),
            }
            .ok()?;
        }
        write_report(&self.paths.runtime(), report.as_bytes())
    }
}

fn write_report(runtime: &Path, report: &[u8]) -> Option<String> {
    if report.len() > MAX_REPORT_BYTES {
        return None;
    }
    let files = ControlledFiles::new(runtime).ok()?;
    let write = |name: &str| {
        let mut output = files.create_new(&runtime.join(name)).ok()?;
        output.file_mut().write_all(report).ok()?;
        output.finish().ok()?;
        Some(name.to_owned())
    };
    for slot in 1..=REPORT_SLOTS {
        if let Some(name) = write(&format!("runtime-diagnostic-{slot:02}.log")) {
            return Some(name);
        }
    }
    // Only rotate our own bounded, single-link records. Keep the identity and
    // verify the marker again on the exact handle used to delete the record.
    let mut oldest = None;
    for slot in 1..=REPORT_SLOTS {
        let name = format!("runtime-diagnostic-{slot:02}.log");
        let Ok(mut file) = LockedFile::open(&runtime.join(&name), MAX_REPORT_BYTES as u64) else {
            continue;
        };
        if !is_log(file.file_mut()) {
            continue;
        }
        let Ok(time) = file.file().metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if oldest
            .as_ref()
            .is_none_or(|(previous, _, _)| time < *previous)
        {
            if let Ok(identity) = file.identity() {
                oldest = Some((time, name, identity));
            }
        }
    }
    let (_, name, identity) = oldest?;
    if !files
        .remove_object_expected(&runtime.join(&name), false, identity, is_log)
        .ok()?
    {
        return None;
    }
    write(&name)
}

fn is_log(file: &mut std::fs::File) -> bool {
    let mut header = [0; HEADER.len()];
    file.read_exact(&mut header).is_ok() && header == HEADER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_rotate_and_leave_historical_logs_intact() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("fc-diagnostics-{}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _temp = Temp(dir.clone());
        let old = dir.join("startup-failure-01.log");
        std::fs::write(&old, b"historic").unwrap();
        for counter in 0..=REPORT_SLOTS {
            let report = [HEADER, format!("sample={counter}").as_bytes()].concat();
            let name = write_report(&dir, &report).unwrap();
            assert_eq!(std::fs::read(dir.join(name)).unwrap(), report);
        }
        assert_eq!(std::fs::read(old).unwrap(), b"historic");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), REPORT_SLOTS + 1);
    }
}
