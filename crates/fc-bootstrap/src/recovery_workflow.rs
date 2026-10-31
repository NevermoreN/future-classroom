//! Native backup/recovery orchestration. Heavy preview ownership stays in a
//! fixed slot until an existing file worker, or the final cold coordinator,
//! actually retires it; dropping a UI handle never removes a directory tree.
#[cfg(any(windows, test))]
mod slot;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
