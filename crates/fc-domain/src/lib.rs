#![forbid(unsafe_code)]

/// Public product version; Cargo/MSI retain their three-field install identity.
pub const PRODUCT_DISPLAY_VERSION: &str = include_str!("../PRODUCT_VERSION");

pub mod auth;
pub mod autostart;
pub mod calendar;
pub mod content;
pub mod desktop;
pub mod identity;
pub mod ids;
pub mod layout;
pub mod lesson_context;
pub mod materials;
pub mod profile;
pub mod profile_content;
pub mod recovery;
pub mod schedule;
pub mod schedule_document;
pub mod time;

pub mod voice;

pub mod subject_visual;

pub mod https_settings;
pub mod update_state;
pub mod updates;
