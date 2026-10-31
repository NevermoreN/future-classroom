//! One bounded receipt links only known policy transitions. Independent atomic
//! stop generations never become a permission to reopen an older native owner.
use super::*;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoicePolicyPhase {
    Pending,
    Committed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoicePolicyHandoff {
    pub serial: u64,
    pub from_generation: u64,
    pub generation: u64,
    pub phase: VoicePolicyPhase,
    pub enabled: bool,
}

#[derive(Clone, Copy)]
struct Receipt {
    view: VoicePolicyHandoff,
    source: u64,
}
#[derive(Default)]
pub(super) struct PolicySlot {
    sequence: u64,
    receipt: Option<Receipt>,
    active: Option<u64>,
}

pub(super) struct PolicyChangeTicket {
    service: VoiceService,
    serial: u64,
    source: u64,
    epoch: u64,
    previous: Option<Receipt>,
    claimed: bool,
    settled: bool,
    boundary: bool,
}

impl VoiceService {
    /// A receipt is metadata, not native release evidence. A new domain at the
    /// committed endpoint must not treat an earlier commit as its own stop;
    /// native composition tracks whether it participated in this serial.
    pub fn policy_handoff(&self, from_epoch: u64) -> Result<Option<VoicePolicyHandoff>> {
        let slot = self.inner.policy.lock().map_err(|_| BackendError::Closed)?;
        let current = self.generation();
        Ok(slot
            .receipt
            .filter(|receipt| {
                let view = receipt.view;
                from_epoch >= view.from_generation
                    && from_epoch <= view.generation
                    && (current == view.generation
                        || view.phase == VoicePolicyPhase::Pending && current == receipt.source)
            })
            .map(|receipt| receipt.view))
    }

    pub(super) fn policy_drivers_allowed(&self) -> Result<bool> {
        let slot = self.inner.policy.lock().map_err(|_| BackendError::Closed)?;
        let current = self.generation();
        Ok(!slot.receipt.is_some_and(|receipt| {
            receipt.view.phase == VoicePolicyPhase::Pending
                && (current == receipt.source || current == receipt.view.generation)
        }))
    }

    pub(super) fn begin_policy_change(
        &self,
        source: u64,
        epoch: u64,
        boundary: bool,
    ) -> Result<PolicyChangeTicket> {
        let mut slot = self.inner.policy.lock().map_err(|_| BackendError::Closed)?;
        if slot.active.is_some() {
            return Err(BackendError::Storage(StorageError::Busy));
        }
        if self.generation() != source {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            )));
        }
        let previous = slot.receipt;
        let serial = slot.sequence.checked_add(1).ok_or(BackendError::Overflow)?;
        let from = previous
            .filter(|receipt| receipt.view.generation == source)
            .map_or(source, |receipt| receipt.view.from_generation);
        slot.sequence = serial;
        slot.active = Some(serial);
        if boundary {
            slot.receipt = Some(Receipt {
                source,
                view: VoicePolicyHandoff {
                    serial,
                    from_generation: from,
                    generation: epoch,
                    phase: VoicePolicyPhase::Pending,
                    enabled: false,
                },
            });
        }
        drop(slot);
        Ok(PolicyChangeTicket {
            service: self.clone(),
            serial,
            source,
            epoch,
            previous,
            claimed: false,
            settled: false,
            boundary,
        })
    }
}

impl PolicyChangeTicket {
    pub(super) fn claim(&mut self) {
        self.claimed = true;
    }

    pub(super) fn committed(&mut self, enabled: bool) -> Result<()> {
        let mut slot = self
            .service
            .inner
            .policy
            .lock()
            .map_err(|_| BackendError::Closed)?;
        if slot.active != Some(self.serial) {
            return Err(BackendError::Closed);
        }
        if self.service.generation() != self.epoch {
            return Err(BackendError::Voice(VoiceRepositoryError::Domain(
                VoiceError::StaleGeneration,
            )));
        }
        if self.boundary {
            let receipt = slot
                .receipt
                .as_mut()
                .filter(|receipt| {
                    receipt.view.serial == self.serial
                        && receipt.view.phase == VoicePolicyPhase::Pending
                })
                .ok_or(BackendError::Closed)?;
            receipt.view.phase = VoicePolicyPhase::Committed;
            receipt.view.enabled = enabled;
        } else if let Some(receipt) = slot
            .receipt
            .as_mut()
            .filter(|receipt| receipt.view.generation == self.epoch)
        {
            // Enabling grants no new stop boundary or serial to an already
            // running native domain. A prior stopped participant may finish.
            receipt.view.phase = VoicePolicyPhase::Committed;
            receipt.view.enabled = enabled;
        }
        slot.active = None;
        self.settled = true;
        Ok(())
    }

    pub(super) fn failed(&mut self) {
        self.service.inner.enabled.store(false, Ordering::Release);
        // An uncertain result cannot reopen this known policy endpoint. Never
        // advance/adopt a later independently stopped generation here.
        let _ = self.service.inner.runtime_state.compare_exchange(
            self.epoch << 1 | 1,
            self.epoch << 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if let Ok(mut slot) = self.service.inner.policy.lock() {
            if slot.active == Some(self.serial) {
                let from = self
                    .previous
                    .filter(|receipt| receipt.view.generation == self.source)
                    .map_or(self.source, |receipt| receipt.view.from_generation);
                slot.receipt = Some(Receipt {
                    source: self.source,
                    view: VoicePolicyHandoff {
                        serial: self.serial,
                        from_generation: from,
                        generation: self.epoch,
                        phase: VoicePolicyPhase::Failed,
                        enabled: false,
                    },
                });
                slot.active = None;
            }
        }
        self.settled = true;
    }
}

impl Drop for PolicyChangeTicket {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        if self.claimed {
            self.failed();
        } else if let Ok(mut slot) = self.service.inner.policy.lock() {
            if slot.active == Some(self.serial) {
                // CAS did not prove this policy owns the planned next epoch.
                // Restore only metadata; do not adopt an independent stop.
                slot.receipt = self.previous;
                slot.active = None;
            }
        }
        // No policy mutex survives the callback.
        self.service.wake();
    }
}
