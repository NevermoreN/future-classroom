//! HTTP.sys v2 fixed-slot transport. The caller owns one HTTP execution domain;
//! poll performs bounded IOCP work and never authenticates or touches SQLite.
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod lifecycle;
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::HttpHost;

#[cfg(windows)]
mod diagnostic;
#[cfg(windows)]
pub use diagnostic::{serve_isolated, serve_isolated_twice, HttpProbeReport};
