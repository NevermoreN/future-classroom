//! Candidate installation coordination only. Not yet wired into application entry
//! or MSI actions. A Pending record is durable negative state, never liveness.
//! The trusted broker must separately authenticate transaction receipts and verify
//! the original payload state; this module does not perform/approve MSI actions.
use std::{path::PathBuf, time::Instant};
use windows::{Win32::Foundation::ERROR_SHARING_VIOLATION, core::HRESULT};

mod native;
#[cfg(test)]
mod tests;

pub const INSTALL_FAMILY: &str = "99378807-098D-5F04-8D4D-F181DFA218D5";
pub const MAX_PENDING_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleError {
    Busy,
    Pending,
    Invalid,
    Expired,
    GuardianRunning,
    Unsettled,
    Faulted,
    Native(HRESULT),
}
impl std::fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "installation lifecycle: {self:?}")
    }
}
impl std::error::Error for LifecycleError {}
impl From<windows::core::Error> for LifecycleError {
    fn from(error: windows::core::Error) -> Self {
        if error.code() == ERROR_SHARING_VIOLATION.to_hresult() {
            Self::Busy
        } else {
            Self::Native(error.code())
        }
    }
}
pub type Result<T> = std::result::Result<T, LifecycleError>;

/// No caller-selected path, SID or install family. Discovery creates no files.
/// Current-user identity is rechecked when each physical gate is acquired.
#[derive(Debug)]
pub struct InstallIdentity {
    local: PathBuf,
    sid: Vec<u8>,
    #[cfg(test)]
    leaf: Option<String>,
}
impl InstallIdentity {
    pub fn for_current_user() -> Result<Self> {
        native::identity()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaintenanceOperation {
    Install,
    Repair,
    Upgrade,
    Uninstall,
}
impl MaintenanceOperation {
    fn tag(self) -> u8 {
        match self {
            Self::Install => 1,
            Self::Repair => 2,
            Self::Upgrade => 3,
            Self::Uninstall => 4,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationId([u8; 16]);
impl OperationId {
    pub fn bytes(self) -> [u8; 16] {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardianIdentity {
    pub process_id: u32,
    pub creation_filetime: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettledOutcome {
    Commit,
    Rollback,
}
/// Trusted broker input, NOT authentication of an external message. The broker
/// must validate exact Join/End ownership, authenticated receipt, actual effects
/// and native cleanup before constructing it. A successful End alone is not
/// `effects_reconciled` (in particular a skipped rollback error is not clean).
#[derive(Clone, Copy, Debug)]
pub struct Settlement {
    pub operation_id: OperationId,
    pub guardian: GuardianIdentity,
    pub outcome: SettledOutcome,
    pub join_return: u32,
    pub end_return: u32,
    pub effects_reconciled: bool,
}

pub struct LifecycleGate;
impl LifecycleGate {
    pub fn acquire_startup(identity: &InstallIdentity) -> Result<StartupLease> {
        Ok(StartupLease {
            resources: native::Resources::open(identity, false)?,
        })
    }
    pub fn try_begin_maintenance(
        identity: &InstallIdentity,
        operation: MaintenanceOperation,
        original_deadline: Instant,
    ) -> Result<MaintenanceOwner> {
        if Instant::now() >= original_deadline {
            return Err(LifecycleError::Expired);
        }
        let mut resources = native::Resources::open(identity, true)?;
        let operation_id = resources.create_pending(identity, operation)?;
        if Instant::now() >= original_deadline {
            // Pending deliberately survives this failure. No automatic retry or
            // adoption of this record by a later process/operation is provided.
            return Err(LifecycleError::Expired);
        }
        Ok(MaintenanceOwner {
            resources,
            operation_id,
            deadline: original_deadline,
            guardian: None,
            watch_attempted: false,
            settled: false,
        })
    }
}

/// Nonclone, non-Send physical shared handle; keep through actual worker/SQLite/
/// shell/TLS/thread return. Drop has no Released receipt; close failure retains
/// the remaining exact native resources and permanently faults this process.
pub struct StartupLease {
    resources: native::Resources,
}
impl StartupLease {
    pub fn try_release(&mut self) -> Result<()> {
        self.resources.release()
    }
    pub fn is_released(&self) -> bool {
        self.resources.released()
    }
}

/// Original exclusive gate + original Pending inode. Drop closes what can be
/// returned but never clears Pending. Owner death therefore blocks next startup
/// even though the OS has returned its share handle.
pub struct MaintenanceOwner {
    resources: native::Resources,
    operation_id: OperationId,
    deadline: Instant,
    guardian: Option<GuardianIdentity>,
    watch_attempted: bool,
    settled: bool,
}
impl MaintenanceOwner {
    pub fn operation_id(&self) -> OperationId {
        self.operation_id
    }
    /// Pins the original child process object and its actual SID/creation time.
    /// Launch and executable provenance remain the trusted broker's job. No
    /// arbitrary path/command is accepted by this coordination module.
    pub fn watch_guardian(&mut self, child: &std::process::Child) -> Result<GuardianIdentity> {
        self.current()?;
        if self.watch_attempted {
            return Err(LifecycleError::Invalid);
        }
        self.watch_attempted = true;
        let identity = self.resources.watch_guardian(child)?;
        self.guardian = Some(identity);
        Ok(identity)
    }
    fn current(&self) -> Result<()> {
        self.resources.check_live()?;
        if Instant::now() >= self.deadline {
            return Err(LifecycleError::Expired);
        }
        Ok(())
    }
    /// Nonblocking. Receipt or process exit alone cannot clear the negative
    /// record. Original process must be signaled with exit0 as well as matching
    /// the trusted settlement and original operation/creation identity.
    pub fn record_settlement(&mut self, receipt: Settlement) -> Result<()> {
        self.current()?;
        if self.settled
            || receipt.operation_id != self.operation_id
            || self.guardian != Some(receipt.guardian)
            || receipt.join_return != 0
            || receipt.end_return != 0
            || !receipt.effects_reconciled
        {
            return Err(LifecycleError::Unsettled);
        }
        self.resources.guardian_returned()?;
        self.settled = true;
        Ok(())
    }
    /// The same owner only; never stale-record recovery. Even a late successful
    /// receipt cannot renew the original deadline. Exact readback precedes
    /// handle-based deletion, and the exclusive gate is the last file closed.
    pub fn try_release(&mut self) -> Result<()> {
        self.current()?;
        if !self.settled {
            return Err(LifecycleError::Unsettled);
        }
        self.resources.guardian_returned()?;
        self.resources.clear_pending()?;
        self.resources.release()
    }
    pub fn is_released(&self) -> bool {
        self.resources.released()
    }
}
