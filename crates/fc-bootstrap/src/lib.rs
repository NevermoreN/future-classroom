//! Trusted composition services shared by the native application and integration tests.
#![forbid(unsafe_code)]
pub mod backend;

#[cfg(windows)]
pub mod voice_files;
