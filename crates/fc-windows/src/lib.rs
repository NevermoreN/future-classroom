//! Windows-only adapters. Native resources never cross into domain/port values.
#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{probe, program_data, InstanceGuard, NativeProbeReport, SystemRandom};
#[cfg(windows)]
pub mod host;
#[cfg(windows)]
pub mod system;
#[cfg(windows)]
pub mod workbench;

#[cfg(not(windows))]
pub fn probe(_seconds: u32) -> Result<(), &'static str> {
    Err("Windows native probe requires Windows; host tests are not platform evidence")
}
