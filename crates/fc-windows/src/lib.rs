//! Windows-only adapters. Native resources never cross into domain/port values.
#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(windows)]
pub mod audio;
pub mod calendar_download;
#[cfg(windows)]
mod com_task;
pub mod http;
#[cfg(windows)]
mod native;
pub mod startup;
pub mod tcp_frontdoor;
pub mod tls_frontdoor;
#[cfg(windows)]
pub use native::{probe, program_data, InstanceGuard, NativeProbeReport, SystemRandom};
#[cfg(windows)]
pub mod host;
#[cfg(windows)]
pub mod installer_lifecycle;
#[cfg(windows)]
pub mod system;
#[cfg(windows)]
pub mod workbench;

#[cfg(not(windows))]
pub fn probe(_seconds: u32) -> Result<(), &'static str> {
    Err("Windows native probe requires Windows; host tests are not platform evidence")
}

#[cfg(windows)]
pub mod desktop;

#[cfg(all(test, windows))]
pub(crate) static NATIVE_TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(windows)]
pub mod materials;
