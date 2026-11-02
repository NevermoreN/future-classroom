//! Trusted fixture metadata only. Every binding below is actually issued by
//! the production sole Writer and is checked again by its normal Task gate.
//! Labels never enter a production request. Keep this object across a fixture
//! reopen; an expired or restored-away pair is returned unchanged and denied.
use crate::{command_window::IssueCommand, CommitCommand, StorageError, StorageHandle};
use fc_ports::command_window::{CommandFamily, CommandOwner, IssuedCommand};
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
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
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
    /// Read original creator metadata by a fixture label, without issuing or
    /// changing the actor on a query/foreign-owner negative test.
    pub(crate) fn labelled(&self, family: CommandFamily, label: &str) -> Option<IssuedCommand> {
        self.pairs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&(family.name(), label.to_owned()))
            .copied()
    }
    /// Exact already-issued fixture pair, including after expiry/reopen. This
    /// neither renews it nor substitutes the current actor for its original owner.
    pub(crate) fn original(
        &self,
        id: fc_domain::ids::CommandId,
    ) -> Result<IssuedCommand, StorageError> {
        self.pairs
            .lock()
            .map_err(|_| StorageError::WorkerPanicked)?
            .values()
            .find(|issued| issued.command_id == id)
            .copied()
            .ok_or(StorageError::CommandNotIssued)
    }
    pub(crate) fn commit<S: Send + 'static>(
        &self,
        handle: &StorageHandle<S>,
        command: CommitCommand,
        snapshot: S,
    ) -> Result<crate::Ticket<crate::CommitReceipt>, StorageError> {
        let (scoped, command) = self.prepare_generic(handle, command)?;
        scoped.try_commit(command, snapshot)
    }
    /// Call before starting a publisher/Writer hold. Sending this command later
    /// still uses the original scope, preserving queue and byte-budget tests.
    pub(crate) fn prepare_generic<S: Send + 'static>(
        &self,
        handle: &StorageHandle<S>,
        mut command: CommitCommand,
    ) -> Result<(StorageHandle<S>, CommitCommand), StorageError> {
        let issued = self.issue(
            handle,
            CommandFamily::Generic,
            &command.id,
            CommandOwner::PublicDesktop,
        )?;
        command.id = issued.command_id.get().to_string();
        Ok((handle.with_command_window(issued), command))
    }
}
