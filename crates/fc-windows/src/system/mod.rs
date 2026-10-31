//! Deployment-only system adapters. Their constructors accept no web commands.
mod autorun;
mod clock;
mod crypto;
mod paths;
mod thread_exit;
pub use autorun::{AutostartReadback, RunAtLogin, RunScope};
pub use clock::CurrentClock;
pub use crypto::{CurrentUserDpapi, SecretBytes};
pub use paths::{current_user_sid, DataPaths, DeploymentDirectory, DirectoryAclReadback};
pub use thread_exit::thread_is_finished;

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
