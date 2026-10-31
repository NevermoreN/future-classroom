//! Trusted composition services shared by the native application and integration tests.
#![forbid(unsafe_code)]
pub mod autostart;
pub mod backend;
mod physical_thread;
pub mod profile_content_cache;
#[cfg(windows)]
pub mod profile_content_files;
#[cfg(windows)]
pub mod profile_content_startup;
pub mod recovery;
pub mod recovery_files;
pub mod recovery_workflow;
pub mod reinitialization;
pub mod reinitialization_files;
pub mod reinitialization_workflow;
pub mod voice_runtime;
pub mod web_application;
pub mod web_assets;

#[cfg(windows)]
pub mod voice_files;
