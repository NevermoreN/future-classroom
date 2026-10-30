//! Trusted composition services shared by the native application and integration tests.
#![forbid(unsafe_code)]
pub mod backend;
pub mod voice_runtime;
pub mod web_application;
pub mod web_assets;

#[cfg(windows)]
pub mod voice_files;
