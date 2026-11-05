//! Trusted composition services shared by the native application and integration tests.
#![forbid(unsafe_code)]
pub mod autostart;
pub mod backend;
#[cfg(windows)]
pub mod https_runtime;
#[cfg(windows)]
pub mod local_data_migration;
#[cfg(windows)]
pub mod local_https;
pub mod local_management;
pub mod maintenance_web;
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
#[cfg(windows)]
pub mod repair_web;
#[cfg(windows)]
pub mod update_network;
pub mod update_packages;
pub mod updates;
pub mod voice_runtime;
pub mod web_application;
pub mod web_assets;
pub mod web_transfer;

#[cfg(feature = "school-gateway")]
pub mod school_gateway;

#[cfg(windows)]
pub mod voice_files;

#[cfg(windows)]
#[path = "application/profiles_preview.rs"]
#[allow(dead_code)] // Library uses geometry; the native adapter uses bitmap helpers.
pub mod profile_preview;
