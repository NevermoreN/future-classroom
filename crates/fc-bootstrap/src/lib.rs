//! Trusted composition services shared by the native application and integration tests.
#![forbid(unsafe_code)]
pub mod autostart;
pub mod backend;
pub mod profile_content_cache;
#[cfg(windows)]
pub mod profile_content_files;
#[cfg(windows)]
pub mod profile_content_startup;
pub mod recovery;
pub mod recovery_files;
pub mod voice_runtime;
pub mod web_application;
pub mod web_assets;

#[cfg(windows)]
pub mod voice_files;
