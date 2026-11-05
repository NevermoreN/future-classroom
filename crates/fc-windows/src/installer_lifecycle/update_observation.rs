//! Read-only preflight. These observations never replace the sealed Setup's
//! native admission or grant permission to an MSI custom action.
use super::{native, Result};
use fc_domain::updates::ExactPredecessor;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct UpdateSourceFacts {
    pub product_code: String,
    pub version: String,
    pub installed_exe_sha256: [u8; 32],
    pub predecessor_wire: Vec<u8>,
    pub predecessor: ExactPredecessor,
}

pub fn inspect_update_sources(directory: &Path) -> Result<UpdateSourceFacts> {
    native::broker::maintenance::update::inspect(directory)
}

/// Compare a signed candidate's exact predecessor to native current-account
/// registration, its complete cached MSI Binary streams and installed files.
/// Cached MSI bytes can differ from distribution bytes; its logical tables and
/// complete payload envelope are checked instead, just as in native Setup.
pub fn qualify_update_predecessor(wire: &[u8], expected: &ExactPredecessor) -> Result<()> {
    native::broker::maintenance::update::qualify(wire, expected)
}

pub(crate) fn reconcile_failed_update(
    wire: &[u8],
    expected: &ExactPredecessor,
    target: &str,
    setup: &Path,
    coordinator: &Path,
    commit_failed: impl FnOnce() -> Result<()>,
) -> Result<()> {
    native::broker::maintenance::update::reconcile_failed(
        wire,
        expected,
        target,
        setup,
        coordinator,
        commit_failed,
    )
}
