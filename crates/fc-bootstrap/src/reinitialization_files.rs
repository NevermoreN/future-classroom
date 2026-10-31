//! Explicit destructive deployment reinitialization, separate from ordinary
//! backup restore. Only fixed project-owned namespaces can participate.
pub const OWNED_TREES: [&str; 13] = [
    "state",
    "config",
    "teachers",
    "profile-content",
    "voice",
    "staging",
    "launch-copies",
    "web-material-staging",
    "exports",
    "open-copies",
    "quarantine",
    "cache",
    "recovery",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReinitializationFileError {
    Unavailable,
    NativePrepare { stage: &'static str, hresult: i32 },
    Cancelled,
    RepairRequired,
    InvalidReview,
}
impl std::fmt::Display for ReinitializationFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReinitializationFileError {}
pub type Result<T> = std::result::Result<T, ReinitializationFileError>;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
