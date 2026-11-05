//! Deployment-only system adapters. Their constructors accept no web commands.
mod autorun;
mod clock;
mod crypto;
mod paths;
mod thread_exit;
mod update_firewall;
pub use update_firewall::{prepare_update_firewall, UpdateFirewallStatus};
mod update_version;
pub use autorun::{AutostartReadback, RunAtLogin, RunScope};
pub use clock::CurrentClock;
pub use crypto::{CurrentUserDpapi, SecretBytes};
pub use paths::{current_user_sid, DataPaths, DeploymentDirectory, DirectoryAclReadback};
pub use thread_exit::thread_is_finished;
pub use update_version::update_windows_baseline;

#[derive(Debug)]
pub struct SystemProbeReport {
    pub project_data_root: std::path::PathBuf,
    pub deployment_user_sid: String,
    pub autostart: AutostartReadback,
    pub thread_cpu_time: std::time::Duration,
}

/// Read-only discovery. Does not initialize ProgramData or modify a Run value.
pub fn probe() -> windows::core::Result<SystemProbeReport> {
    Ok(SystemProbeReport {
        project_data_root: DataPaths::program_data()?.root().to_owned(),
        deployment_user_sid: current_user_sid()?,
        autostart: RunAtLogin::current_executable()?.read_back()?,
        thread_cpu_time: current_thread_cpu_time()?,
    })
}

/// Calling thread's kernel + user CPU time. The pseudo-handle is borrowed and
/// must never be closed; no clock handle crosses into materials/domain code.
pub fn current_thread_cpu_time() -> windows::core::Result<std::time::Duration> {
    use windows::Win32::{
        Foundation::{E_UNEXPECTED, FILETIME},
        System::Threading::{GetCurrentThread, GetThreadTimes},
    };
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: current-thread pseudo handle and exclusive valid output structures;
    // the call is synchronous and retains none of these pointers.
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )?;
    }
    let ticks =
        |value: FILETIME| (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime);
    let total = ticks(kernel)
        .checked_add(ticks(user))
        .ok_or_else(|| windows::core::Error::from_hresult(E_UNEXPECTED))?;
    let seconds = total / 10_000_000;
    let nanos = (total % 10_000_000) as u32 * 100;
    Ok(std::time::Duration::new(seconds, nanos))
}

#[derive(Clone, Debug)]
pub struct ProcessResources {
    pub sampled_at: std::time::Instant,
    pub cpu_time: std::time::Duration,
    pub working_set_bytes: u64,
    pub private_bytes: u64,
    pub handles: u32,
    pub gdi_objects: u32,
    pub user_objects: u32,
}

/// Current process only; individual system counters are read sequentially, not
/// as an atomic snapshot. No handles, paths, or other-process data are retained.
pub fn current_process_resources() -> windows::core::Result<ProcessResources> {
    use windows::{
        core::{Error, Result},
        Win32::{
            Foundation::{GetLastError, SetLastError, ERROR_SUCCESS, E_UNEXPECTED, FILETIME},
            System::{
                ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
                Threading::*,
            },
        },
    };
    let process = unsafe { GetCurrentProcess() };
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let mut handles = 0;
    // SAFETY: borrowed pseudo-handle is never closed; all exclusive output
    // structures outlive these synchronous calls and retain no native pointers.
    unsafe {
        GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user)?;
        GetProcessMemoryInfo(
            process,
            (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            memory.cb,
        )?;
        GetProcessHandleCount(process, &mut handles)?;
    }
    let gui = |flags| -> Result<u32> {
        // Zero GUI objects is legitimate; clear last-error before distinguishing
        // that value from a failed query. This queries only our own process.
        unsafe {
            SetLastError(ERROR_SUCCESS);
            let count = GetGuiResources(process, flags);
            if count == 0 && GetLastError() != ERROR_SUCCESS {
                return Err(Error::from_thread());
            }
            Ok(count)
        }
    };
    let ticks = |v: FILETIME| u64::from(v.dwHighDateTime) << 32 | u64::from(v.dwLowDateTime);
    let cpu = ticks(kernel)
        .checked_add(ticks(user))
        .ok_or_else(|| Error::from_hresult(E_UNEXPECTED))?;
    Ok(ProcessResources {
        sampled_at: std::time::Instant::now(),
        cpu_time: std::time::Duration::new(cpu / 10_000_000, (cpu % 10_000_000) as u32 * 100),
        working_set_bytes: memory.WorkingSetSize as u64,
        private_bytes: memory.PrivateUsage as u64,
        handles,
        gdi_objects: gui(GR_GDIOBJECTS)?,
        user_objects: gui(GR_USEROBJECTS)?,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_thread_cpu_clock_is_monotonic() {
        let before = super::current_thread_cpu_time().unwrap();
        let mut value = 7u64;
        for _ in 0..10_000 {
            value = std::hint::black_box(value.wrapping_mul(19).wrapping_add(17));
        }
        std::hint::black_box(value);
        let after = super::current_thread_cpu_time().unwrap();
        assert!(after >= before);
    }
}
