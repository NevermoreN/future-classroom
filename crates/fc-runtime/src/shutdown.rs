//! Non-blocking shutdown orchestration. The platform owns actual thread joins and
//! controlled process termination; neither belongs in a UI callback.
use crate::health::HealthGate;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU16, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ExecutionDomain {
    Ui = 1,
    Writer = 2,
    WorkerOne = 4,
    WorkerTwo = 8,
    Http = 16,
    Audio = 32,
    Shell = 64,
    Calendar = 128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownProgress {
    NotStarted,
    Waiting { pending: u16 },
    Ready,
    TerminateRequired { pending: u16 },
}

/// Give this handle only to its corresponding execution domain. Acknowledge
/// after native callbacks are detached, resources drained and writer flushed.
#[derive(Clone)]
pub struct ShutdownAck {
    bits: Arc<AtomicU16>,
    started: Arc<AtomicBool>,
    domain: ExecutionDomain,
}
impl ShutdownAck {
    pub fn acknowledge_drained(&self) {
        if !self.started.load(Ordering::Acquire) {
            return;
        }
        self.bits.fetch_or(self.domain as u16, Ordering::Release);
    }
}

pub struct ShutdownCoordinator {
    gate: HealthGate,
    expected: u16,
    acks: Arc<AtomicU16>,
    accepting_acks: Arc<AtomicBool>,
    budget: Duration,
    deadline: Option<Instant>,
    started: bool,
    timed_out: bool,
}

impl ShutdownCoordinator {
    pub fn new(gate: HealthGate, domains: &[ExecutionDomain], budget: Duration) -> Self {
        Self {
            gate,
            expected: domains.iter().fold(0, |mask, d| mask | *d as u16),
            acks: Arc::new(AtomicU16::new(0)),
            accepting_acks: Arc::new(AtomicBool::new(false)),
            budget,
            deadline: None,
            started: false,
            timed_out: false,
        }
    }
    pub fn ack_handle(&self, domain: ExecutionDomain) -> Option<ShutdownAck> {
        (self.expected & domain as u16 != 0).then(|| ShutdownAck {
            bits: self.acks.clone(),
            started: self.accepting_acks.clone(),
            domain,
        })
    }
    pub fn begin(&mut self, now: Instant) {
        if self.started {
            return;
        }
        self.started = true;
        self.accepting_acks.store(true, Ordering::Release);
        self.gate.stop();
        self.deadline = now.checked_add(self.budget);
        if self.deadline.is_none() {
            self.timed_out = true;
            self.gate.fault();
        }
    }
    /// A timeout is irreversible even if late acknowledgements subsequently arrive.
    /// The platform must retain live native allocations until it terminates.
    pub fn poll(&mut self, now: Instant) -> ShutdownProgress {
        if !self.started {
            return ShutdownProgress::NotStarted;
        }
        let pending = self.expected & !self.acks.load(Ordering::Acquire);
        if self.timed_out {
            return ShutdownProgress::TerminateRequired { pending };
        }
        if pending == 0 {
            return ShutdownProgress::Ready;
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.timed_out = true;
            self.gate.fault();
            ShutdownProgress::TerminateRequired { pending }
        } else {
            ShutdownProgress::Waiting { pending }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{authority::AuthorityStore, health::Health};
    use fc_domain::ids::{AuthEpoch, Revision};
    #[test]
    fn timeout_requires_termination_and_late_native_completion_cannot_resume() {
        let (mut publisher, reader) = AuthorityStore::new((), Revision::new(0), AuthEpoch::new(0));
        publisher.initialize().unwrap();
        let now = Instant::now();
        let mut shutdown = ShutdownCoordinator::new(
            reader.gate(),
            &[ExecutionDomain::Audio, ExecutionDomain::Writer],
            Duration::from_secs(5),
        );
        let writer = shutdown.ack_handle(ExecutionDomain::Writer).unwrap();
        let audio = shutdown.ack_handle(ExecutionDomain::Audio).unwrap();
        shutdown.begin(now);
        writer.acknowledge_drained();
        assert!(matches!(
            shutdown.poll(now),
            ShutdownProgress::Waiting { .. }
        ));
        assert!(matches!(
            shutdown.poll(now + Duration::from_secs(5)),
            ShutdownProgress::TerminateRequired { .. }
        ));
        audio.acknowledge_drained();
        assert_eq!(
            shutdown.poll(now + Duration::from_secs(6)),
            ShutdownProgress::TerminateRequired { pending: 0 }
        );
        assert_eq!(reader.gate().status().health, Health::Faulted);
    }
    #[test]
    fn ready_requires_all_registered_domains() {
        let (mut publisher, reader) = AuthorityStore::new((), Revision::new(0), AuthEpoch::new(0));
        publisher.initialize().unwrap();
        let now = Instant::now();
        let mut shutdown = ShutdownCoordinator::new(
            reader.gate(),
            &[ExecutionDomain::Writer],
            Duration::from_secs(5),
        );
        let writer = shutdown.ack_handle(ExecutionDomain::Writer).unwrap();
        writer.acknowledge_drained(); // Idle before shutdown is not shutdown acknowledgement.
        shutdown.begin(now);
        assert!(matches!(
            shutdown.poll(now),
            ShutdownProgress::Waiting { .. }
        ));
        writer.acknowledge_drained();
        assert_eq!(shutdown.poll(now), ShutdownProgress::Ready);
    }
}
