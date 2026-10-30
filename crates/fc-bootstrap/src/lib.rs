//! Trusted composition services shared by the native application and integration tests.
#![forbid(unsafe_code)]
pub mod backend;
pub mod voice_runtime;

#[cfg(windows)]
pub mod voice_files;
