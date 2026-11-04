//! One process-owned maintenance job. Status/cancellation stay available when
//! teaching sessions, the database and the shared worker pool have stopped.
use fc_domain::ids::{PrincipalId, Revision};
use fc_ports::RandomPort;
use fc_protocol::web::maintenance::{Kind, Phase, Snapshot};
use fc_security::web::{SecretText, SecretVerifier, WebSecret};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaintenanceError {
    Busy,
    Stale,
    Denied,
    Invalid,
    Conflict,
    Cancelled,
    Unavailable,
    Retained,
}
pub type Result<T> = std::result::Result<T, MaintenanceError>;
struct Current {
    id: u64,
    owner: PrincipalId,
    verifier: SecretVerifier,
    snapshot: Snapshot,
    cancelled: Arc<AtomicBool>,
}
#[derive(Default)]
struct State {
    serial: u64,
    current: Option<Current>,
}
#[derive(Clone, Default)]
pub struct MaintenanceControl(Arc<Mutex<State>>);
pub struct CreatedJob {
    pub id: u64,
    pub proof: SecretText,
    pub snapshot: Snapshot,
}
#[derive(Clone)]
pub struct JobContext {
    control: MaintenanceControl,
    id: u64,
    cancelled: Arc<AtomicBool>,
}
impl JobContext {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn cancellation_requested(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let state = self
            .control
            .0
            .lock()
            .map_err(|_| MaintenanceError::Retained)?;
        state
            .current
            .as_ref()
            .filter(|v| v.id == self.id)
            .map(|v| {
                let mut snapshot = v.snapshot.clone();
                snapshot.cancellation_requested = v.cancelled.load(Ordering::Acquire);
                snapshot
            })
            .ok_or(MaintenanceError::Stale)
    }
    pub fn active(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self.control.0.lock().is_ok_and(|s| {
                s.current.as_ref().is_some_and(|v| {
                    v.id == self.id
                        && !matches!(
                            v.snapshot.phase,
                            Phase::Cancelled | Phase::Failed | Phase::Retained
                        )
                })
            })
    }
    /// Called only by the original worker/coordinator, never by a request DTO.
    pub fn update(&self, phase: Phase, change: impl FnOnce(&mut Snapshot)) -> Result<()> {
        let mut state = self
            .control
            .0
            .lock()
            .map_err(|_| MaintenanceError::Retained)?;
        let current = state
            .current
            .as_mut()
            .filter(|v| v.id == self.id)
            .ok_or(MaintenanceError::Stale)?;
        current.snapshot.phase = phase;
        current.snapshot.can_cancel = phase.can_cancel();
        current.snapshot.cancellation_requested = current.cancelled.load(Ordering::Acquire);
        if matches!(
            phase,
            Phase::Draining
                | Phase::PreparingDatabase
                | Phase::Sealing
                | Phase::Committing
                | Phase::Retained
                | Phase::Failed
                | Phase::Cancelled
        ) {
            current.snapshot.reset_review = None;
            current.snapshot.review = None;
            current.snapshot.mapping = None;
        }
        change(&mut current.snapshot);
        Ok(())
    }
    pub fn fail(&self, error: &str) -> Result<()> {
        self.update(
            if self.cancelled.load(Ordering::Acquire) {
                Phase::Cancelled
            } else {
                Phase::Failed
            },
            |s| s.error = Some(error.chars().take(256).collect()),
        )
    }
}
impl MaintenanceControl {
    pub fn detached_active(&self) -> bool {
        self.0.lock().is_ok_and(|s| {
            s.current.as_ref().is_some_and(|v| {
                matches!(
                    v.snapshot.phase,
                    Phase::Draining
                        | Phase::PreparingDatabase
                        | Phase::Sealing
                        | Phase::Committing
                        | Phase::Retained
                ) || (v.snapshot.phase == Phase::Finished && v.snapshot.restart_required)
            })
        })
    }
    pub fn begin(
        &self,
        owner: PrincipalId,
        revision: Revision,
        kind: Kind,
        random: &mut impl RandomPort,
    ) -> Result<(CreatedJob, JobContext)> {
        let secret = WebSecret::generate(random).map_err(|_| MaintenanceError::Unavailable)?;
        let mut state = self.0.lock().map_err(|_| MaintenanceError::Retained)?;
        if state
            .current
            .as_ref()
            .is_some_and(|v| !v.snapshot.phase.terminal())
        {
            return Err(MaintenanceError::Busy);
        }
        state.serial = state
            .serial
            .checked_add(1)
            .ok_or(MaintenanceError::Unavailable)?;
        let id = state.serial;
        let cancelled = Arc::new(AtomicBool::new(false));
        let snapshot = Snapshot {
            id: id.to_string(),
            kind,
            phase: Phase::Waiting,
            can_cancel: true,
            cancellation_requested: false,
            expected_revision: revision.get().to_string(),
            file_name: None,
            bytes: None,
            sha256: None,
            reset_review: None,
            review: None,
            mapping: None,
            error: None,
            restart_required: false,
        };
        if let Some(previous) = &state.current {
            previous.cancelled.store(true, Ordering::Release);
        }
        state.current = Some(Current {
            id,
            owner,
            verifier: secret.verifier(),
            snapshot: snapshot.clone(),
            cancelled: cancelled.clone(),
        });
        Ok((
            CreatedJob {
                id,
                proof: secret.expose_hex(),
                snapshot,
            },
            JobContext {
                control: self.clone(),
                id,
                cancelled,
            },
        ))
    }
    pub fn snapshot(&self, id: u64, proof: &WebSecret) -> Result<Snapshot> {
        let state = self.0.lock().map_err(|_| MaintenanceError::Retained)?;
        let current = state
            .current
            .as_ref()
            .filter(|v| v.id == id)
            .ok_or(MaintenanceError::Stale)?;
        if !current.verifier.verify(proof) {
            return Err(MaintenanceError::Denied);
        }
        let mut snapshot = current.snapshot.clone();
        snapshot.cancellation_requested = current.cancelled.load(Ordering::Acquire);
        Ok(snapshot)
    }
    pub fn authorize(&self, id: u64, proof: &WebSecret, owner: PrincipalId) -> Result<JobContext> {
        let state = self.0.lock().map_err(|_| MaintenanceError::Retained)?;
        let current = state
            .current
            .as_ref()
            .filter(|v| v.id == id)
            .ok_or(MaintenanceError::Stale)?;
        if current.owner != owner || !current.verifier.verify(proof) {
            return Err(MaintenanceError::Denied);
        }
        Ok(JobContext {
            control: self.clone(),
            id,
            cancelled: current.cancelled.clone(),
        })
    }
    pub fn detached_snapshot(&self, id: u64, proof: &WebSecret) -> Result<Snapshot> {
        let mut snapshot = self.snapshot(id, proof)?;
        if !matches!(
            snapshot.phase,
            Phase::Draining
                | Phase::PreparingDatabase
                | Phase::Sealing
                | Phase::Committing
                | Phase::Retained
        ) && !(snapshot.phase == Phase::Finished && snapshot.restart_required)
        {
            return Err(MaintenanceError::Denied);
        }
        snapshot.reset_review = None;
        snapshot.review = None;
        snapshot.mapping = None;
        Ok(snapshot)
    }
    /// This tiny memory operation needs only the current job proof and verified
    /// LocalControl transport; it intentionally does not read the closed DB.
    pub fn cancel(&self, id: u64, proof: &WebSecret) -> Result<()> {
        let state = self.0.lock().map_err(|_| MaintenanceError::Retained)?;
        let current = state
            .current
            .as_ref()
            .filter(|v| v.id == id)
            .ok_or(MaintenanceError::Stale)?;
        if !current.verifier.verify(proof) {
            return Err(MaintenanceError::Denied);
        }
        if !current.snapshot.can_cancel {
            return Err(MaintenanceError::Conflict);
        }
        current.cancelled.store(true, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Random(u8);
    impl RandomPort for Random {
        type Error = ();
        fn fill(&mut self, bytes: &mut [u8]) -> std::result::Result<(), ()> {
            self.0 += 1;
            bytes.fill(self.0);
            Ok(())
        }
    }
    #[test]
    fn proof_is_job_scoped_and_replacement_revokes_the_old_context() {
        let control = MaintenanceControl::default();
        let owner = PrincipalId::new(1).unwrap();
        let mut random = Random(0);
        let (first, job) = control
            .begin(owner, Revision::new(1), Kind::Export, &mut random)
            .unwrap();
        let proof = WebSecret::parse_hex(first.proof.as_str().to_owned()).unwrap();
        let wrong = WebSecret::generate(&mut random).unwrap();
        assert!(matches!(
            control.snapshot(first.id, &wrong),
            Err(MaintenanceError::Denied)
        ));
        assert!(matches!(
            control.authorize(first.id, &proof, PrincipalId::new(2).unwrap()),
            Err(MaintenanceError::Denied)
        ));
        assert!(control.detached_snapshot(first.id, &proof).is_err());
        job.update(Phase::Finished, |_| {}).unwrap();
        let (second, _) = control
            .begin(owner, Revision::new(1), Kind::Export, &mut random)
            .unwrap();
        assert_ne!(first.id, second.id);
        assert!(!job.active());
        assert!(control.snapshot(first.id, &proof).is_err());
    }
    #[test]
    fn cancellation_remains_available_after_backend_close_until_commit() {
        let control = MaintenanceControl::default();
        let mut random = Random(0);
        let (created, job) = control
            .begin(
                PrincipalId::new(1).unwrap(),
                Revision::new(2),
                Kind::Import,
                &mut random,
            )
            .unwrap();
        let proof = WebSecret::parse_hex(created.proof.as_str().to_owned()).unwrap();
        job.update(Phase::Draining, |_| {}).unwrap();
        assert!(control.detached_snapshot(created.id, &proof).is_ok());
        control.cancel(created.id, &proof).unwrap();
        assert!(!job.active());
        job.update(Phase::Committing, |_| {}).unwrap();
        assert!(matches!(
            control.cancel(created.id, &proof),
            Err(MaintenanceError::Conflict)
        ));
    }
}
