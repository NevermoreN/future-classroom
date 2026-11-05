//! Independent software-maintenance state. Revisions never identify a teaching
//! authority snapshot. A prepared job is durable intent, not installer admission.
use crate::{
    ids::{AuthEpoch, CommandId, PrincipalId, Revision},
    time::Minute,
    updates::{MaintenanceWindow, ReleaseIdentity},
};

pub const MAX_UPDATE_JOBS: usize = 32;
pub const MAX_UPDATE_RECEIPTS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateInstallMode {
    NotifyOnly,
    ManualInstall,
    MaintenanceWindow,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdatePolicy {
    pub revision: Revision,
    /// Public group identity only; the shared key belongs to DPAPI storage.
    pub group_id: Option<[u8; 16]>,
    pub enabled: bool,
    pub channel: String,
    pub download: bool,
    pub share: bool,
    pub install_mode: UpdateInstallMode,
    pub maintenance: MaintenanceWindow,
    pub download_bytes_per_second: u32,
    pub upload_bytes_per_second: u32,
}
impl Default for UpdatePolicy {
    fn default() -> Self {
        Self {
            revision: Revision::default(),
            group_id: None,
            enabled: false,
            channel: "stable".into(),
            download: false,
            share: false,
            install_mode: UpdateInstallMode::NotifyOnly,
            maintenance: MaintenanceWindow {
                start: Minute::new(1200).unwrap(),
                end: Minute::new(360).unwrap(),
            },
            download_bytes_per_second: 4 * 1024 * 1024,
            upload_bytes_per_second: 2 * 1024 * 1024,
        }
    }
}
impl UpdatePolicy {
    pub fn validate(&self) -> bool {
        !self.channel.is_empty()
            && self.channel.len() <= 32
            && self
                .channel
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && self.group_id != Some([0; 16])
            && (!self.enabled || self.group_id.is_some())
            && (!self.download || self.enabled)
            && (!self.share || self.enabled)
            && (1..=4 * 1024 * 1024).contains(&self.download_bytes_per_second)
            && (1..=2 * 1024 * 1024).contains(&self.upload_bytes_per_second)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateJobPhase {
    Prepared,
    Downloading,
    Ready,
    WaitingIdle,
    Handoff,
    Installing,
    NeedsReconcile,
    Succeeded,
    Failed,
    Cancelled,
}
impl UpdateJobPhase {
    pub fn settled(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
    pub fn can_advance_to(self, next: Self) -> bool {
        use UpdateJobPhase::*;
        matches!(
            (self, next),
            (Prepared, Downloading | Ready | Failed | Cancelled)
                | (Downloading, Ready | Failed | Cancelled)
                | (Ready, WaitingIdle | Handoff | Failed | Cancelled)
                | (WaitingIdle, Ready | Handoff | Failed | Cancelled)
                | (Handoff, Installing | NeedsReconcile)
                | (Installing, Succeeded | Failed | NeedsReconcile)
                | (NeedsReconcile, Succeeded | Failed)
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateJob {
    pub id: u64,
    pub command_id: CommandId,
    pub principal_id: PrincipalId,
    pub auth_epoch: AuthEpoch,
    pub policy_revision: Revision,
    pub release: ReleaseIdentity,
    pub phase: UpdateJobPhase,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateCommandReceipt {
    pub command_id: CommandId,
    pub expected_revision: Revision,
    pub principal_id: PrincipalId,
    pub auth_epoch: AuthEpoch,
    pub fingerprint: [u8; 32],
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateMaintenanceState {
    pub revision: Revision,
    pub sequence: u64,
    pub policy: UpdatePolicy,
    pub jobs: Vec<UpdateJob>,
    pub receipts: Vec<UpdateCommandReceipt>,
}
impl UpdateMaintenanceState {
    pub fn validate(&self) -> bool {
        self.policy.validate()
            && self.policy.revision <= self.revision
            && self.jobs.len() <= MAX_UPDATE_JOBS
            && self.receipts.len() <= MAX_UPDATE_RECEIPTS
            && self.jobs.iter().all(|job| {
                job.id != 0
                    && job.id <= self.sequence
                    && job.release.payload_sha256 != [0; 32]
                    && job.policy_revision <= self.policy.revision
            })
            && self.jobs.windows(2).all(|pair| pair[0].id < pair[1].id)
            && self
                .receipts
                .iter()
                .all(|receipt| receipt.expected_revision < self.revision)
            && self.receipts.iter().enumerate().all(|(index, receipt)| {
                self.receipts[..index]
                    .iter()
                    .all(|prior| prior.command_id != receipt.command_id)
            })
    }
    /// Only settled records can be discarded; an interrupted native transaction
    /// remains even when newer command receipts rotate out of the bounded log.
    pub fn reserve_job(&mut self) -> bool {
        if self.jobs.len() == MAX_UPDATE_JOBS {
            let Some(index) = self.jobs.iter().position(|job| job.phase.settled()) else {
                return false;
            };
            self.jobs.remove(index);
        }
        self.jobs.len() < MAX_UPDATE_JOBS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_disable_distribution_and_joining_does_not_enable_it() {
        let mut policy = UpdatePolicy::default();
        assert!(policy.validate());
        policy.group_id = Some([1; 16]);
        assert!(!policy.enabled && !policy.download && !policy.share);
        policy.group_id = None;
        policy.enabled = true;
        assert!(!policy.validate());
    }
    #[test]
    fn physical_handoff_cannot_be_cancelled_or_report_success_before_installing() {
        assert!(!UpdateJobPhase::Handoff.can_advance_to(UpdateJobPhase::Cancelled));
        assert!(!UpdateJobPhase::Ready.can_advance_to(UpdateJobPhase::Succeeded));
        assert!(!UpdateJobPhase::NeedsReconcile.settled());
        assert!(UpdateJobPhase::NeedsReconcile.can_advance_to(UpdateJobPhase::Failed));
    }
}
