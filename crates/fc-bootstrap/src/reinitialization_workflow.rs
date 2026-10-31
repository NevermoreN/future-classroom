//! Final reinitialization coordinator, independent of the stopped shared pool.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
