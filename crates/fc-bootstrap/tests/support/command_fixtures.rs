//! Trusted fixture metadata only. Every binding below is actually issued by
//! the production sole Writer and is checked again by its normal Task gate.
//! Labels never enter a production request. Keep this object across a fixture
//! reopen; an expired or restored-away pair is returned unchanged and denied.
use fc_ports::command_window::{CommandFamily, CommandOwner, IssuedCommand};
use fc_storage::{command_window::IssueCommand, StorageError, StorageHandle};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

static NEXT_RAW: AtomicU64 = AtomicU64::new(10_000_000);
#[derive(Default)]
pub(crate) struct FixtureCommands {
    pairs: Mutex<BTreeMap<(&'static str, String), IssuedCommand>>,
}
impl FixtureCommands {
    pub(crate) fn issue<S: Send + 'static>(
        &self,
        handle: &StorageHandle<S>,
        family: CommandFamily,
        label: &str,
        owner: CommandOwner,
    ) -> Result<IssuedCommand, StorageError> {
        if label.is_empty() || label.len() > 128 {
            return Err(StorageError::InvalidCommand);
        }
        let key = (family.name(), label.to_owned());
        let mut pairs = self
            .pairs
            .lock()
            .map_err(|_| StorageError::WorkerPanicked)?;
        if let Some(issued) = pairs.get(&key) {
            return if issued.owner == owner {
                Ok(*issued)
            } else {
                Err(StorageError::CommandNotIssued)
            };
        }
        if pairs.len() >= 65536 {
            return Err(StorageError::CommandWindowCapacity);
        }
        // Deterministic, nonzero trusted test entropy. Production backend callers
        // continue using their CSPRNG; no test-only path is installed in Writer.
        let raw = NEXT_RAW
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| StorageError::InvalidCommand)?;
        let mut entropy = [83; 24];
        entropy[16..].copy_from_slice(&raw.to_be_bytes());
        let issued = handle
            .try_issue_command(IssueCommand {
                family,
                owner,
                entropy,
            })?
            .wait()?;
        pairs.insert(key, issued);
        Ok(issued)
    }
}
