//! Private foundation for a guardian that owns Begin before any installation.
//! No production entry currently constructs native observations for this route.
//! Receipt fields are native-private; external flags/Join0 never authorize arm.
use super::*;

/// Exclusive physical gate with no Pending and no installation permit. A watched
/// guardian must be accounted for even when Begin fails or no receipt arrives.
pub(crate) struct MaintenanceDraft {
    resources: Option<native::Resources>,
    deadline: Instant,
    guardian: Option<GuardianIdentity>,
    watch_attempted: bool,
    begin: Option<(u32, u32)>,
    begin_observed: bool,
    arming_attempted: bool,
}

impl LifecycleGate {
    pub(crate) fn try_draft_maintenance(
        identity: &InstallIdentity,
        original_deadline: Instant,
    ) -> Result<MaintenanceDraft> {
        if Instant::now() >= original_deadline {
            return Err(LifecycleError::Expired);
        }
        let resources = native::Resources::open(identity, true)?;
        Ok(MaintenanceDraft {
            resources: Some(resources),
            deadline: original_deadline,
            guardian: None,
            watch_attempted: false,
            begin: None,
            begin_observed: false,
            arming_attempted: false,
        })
    }
}

impl MaintenanceDraft {
    fn resources(&self) -> Result<&native::Resources> {
        self.resources.as_ref().ok_or(LifecycleError::Invalid)
    }
    fn current(&self) -> Result<()> {
        self.resources()?.check_live()?;
        if Instant::now() >= self.deadline {
            return Err(LifecycleError::Expired);
        }
        Ok(())
    }
    pub(crate) fn watch_guardian(
        &mut self,
        child: &std::process::Child,
    ) -> Result<GuardianIdentity> {
        self.current()?;
        if self.watch_attempted || self.arming_attempted {
            return Err(LifecycleError::Invalid);
        }
        // Set before the native call: a partially acquired original capability
        // cannot disappear through the no-guardian abort or ordinary Drop path.
        self.watch_attempted = true;
        let guardian = self
            .resources
            .as_mut()
            .ok_or(LifecycleError::Invalid)?
            .watch_guardian(child)?;
        self.resources()?.guardian_running()?;
        self.guardian = Some(guardian);
        Ok(guardian)
    }
    /// Only native code can construct a non-test observation. It must be the
    /// original watched G's actual Begin return, before any Run permit exists.
    pub(crate) fn observe_begin(&mut self, receipt: native::BeginObservation) -> Result<()> {
        self.current()?;
        if self.begin_observed || self.arming_attempted || self.guardian.is_none() {
            return Err(LifecycleError::Invalid);
        }
        let (guardian, return_code, transaction_id) = receipt.values();
        if self.guardian != Some(guardian) {
            return Err(LifecycleError::Unsettled);
        }
        self.resources()?.guardian_running()?;
        self.begin_observed = true;
        self.begin = Some((return_code, transaction_id));
        if return_code != 0 || transaction_id == 0 {
            return Err(LifecycleError::Unsettled);
        }
        Ok(())
    }
    /// The caller first captures/rechecks effects and actually returns all pins
    /// obstructing MSI mutation. Successful return is the native adapter's only
    /// possible point to authorize Run. This foundation does not send Run itself.
    /// A failed/expired arm never exposes an owner or an installation permit.
    pub(crate) fn arm(&mut self, identity: &InstallIdentity) -> Result<MaintenanceOwner> {
        self.current()?;
        let Some((0, transaction_id)) = self.begin else {
            return Err(LifecycleError::Unsettled);
        };
        if transaction_id == 0 || self.arming_attempted {
            return Err(LifecycleError::Unsettled);
        }
        let guardian = self.guardian.ok_or(LifecycleError::Unsettled)?;
        self.resources()?.guardian_running()?;
        self.arming_attempted = true;
        let resources = self.resources.as_mut().ok_or(LifecycleError::Invalid)?;
        // Original tag5, exact original pending inode, <=4096 bytes. No new
        // durable tag, alternate family, capacity or deadline is introduced.
        let operation_id =
            resources.create_pending(identity, MaintenanceOperation::SameManifest)?;
        if Instant::now() >= self.deadline {
            return Err(LifecycleError::Expired);
        }
        let resources = self.resources.take().ok_or(LifecycleError::Invalid)?;
        Ok(MaintenanceOwner {
            resources,
            operation_id,
            deadline: self.deadline,
            guardian: Some(guardian),
            watch_attempted: true,
            settled: false,
            transaction_origin: TransactionOrigin::OwnedByBegin { transaction_id },
        })
    }
    /// Cleanup of an unarmed Draft is not a renewed install deadline. No Pending
    /// has ever been created. If G was watched, its native return must prove no
    /// Run was authorized, actual empty Rollback0 when Begin0, and control return;
    /// independently require the original G object signaled with exit0.
    pub(crate) fn try_abort(
        &mut self,
        receipt: Option<native::DraftAbortObservation>,
    ) -> Result<()> {
        self.resources()?.check_live()?;
        if self.arming_attempted {
            return Err(LifecycleError::Pending);
        }
        match (self.watch_attempted, self.guardian, receipt) {
            (false, None, None) => (),
            (true, Some(original), Some(receipt)) => {
                let (guardian, begin, end, controls) = receipt.values();
                if guardian != original || controls != 0 {
                    return Err(LifecycleError::Unsettled);
                }
                // No Begin observation is not evidence of a failed Begin. It
                // needs a native terminal observation explicitly saying either
                // not-called or an actual failure, with its controls returned.
                if self.begin_observed && begin != self.begin {
                    return Err(LifecycleError::Unsettled);
                }
                match (begin, end) {
                    (None, None) if !self.begin_observed => (),
                    (Some((code, 0)), None) if code != 0 => (),
                    (Some((0, id)), Some(0)) if id != 0 => (),
                    _ => return Err(LifecycleError::Unsettled),
                }
                self.resources()?.guardian_returned()?;
            }
            _ => return Err(LifecycleError::Unsettled),
        }
        self.resources
            .as_mut()
            .ok_or(LifecycleError::Invalid)?
            .release_draft()
    }
    #[cfg(test)]
    pub(super) fn expire_for_test(&mut self) {
        self.deadline = Instant::now();
    }
    #[cfg(test)]
    pub(super) fn inject_close_failure_for_test(&mut self, role: &str) {
        self.resources
            .as_mut()
            .unwrap()
            .inject_close_failure_at(role);
    }
}

impl Drop for MaintenanceDraft {
    fn drop(&mut self) {
        if let Some(resources) = &mut self.resources {
            if self.watch_attempted && !resources.released() {
                // Even a logical Begin error is not the original G's physical
                // return. With no durable Pending, retain the SAME physical gate
                // in the existing bounded ledger rather than silently releasing.
                resources.retain_until_exit();
            }
        }
    }
}

impl MaintenanceOwner {
    pub(crate) fn record_controlled_settlement(
        &mut self,
        receipt: native::TransactionObservation,
    ) -> Result<()> {
        let (operation_id, guardian, ownership, outcome, install, end, controls, effects) =
            receipt.values();
        let (origin, ownership_return) = match ownership {
            native::TransactionOwnership::OwnedByBegin {
                begin_return,
                transaction_id,
            } => (
                TransactionOrigin::OwnedByBegin { transaction_id },
                begin_return,
            ),
            native::TransactionOwnership::OwnedByJoin { join_return } => {
                (TransactionOrigin::OwnedByJoin, join_return)
            }
        };
        if controls != 0
            || install == u32::MAX
            || (outcome == SettledOutcome::Commit && install != 0)
        {
            return Err(LifecycleError::Unsettled);
        }
        self.record_transaction_settlement(
            operation_id,
            guardian,
            outcome,
            origin,
            ownership_return,
            end,
            effects,
        )
    }
}
