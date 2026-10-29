use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ControlSignal {
    Stop = 1,
    Fault = 2,
    AuthorityInvalidated = 4,
    ForegroundHidden = 8,
}

/// One instance per execution domain: producers clone that domain's handle.
/// Consumers drain bits before waiting and after waking; no payload allocation.
#[derive(Clone, Debug, Default)]
pub struct ControlSignals(Arc<AtomicU8>);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PendingControls(u8);

impl ControlSignals {
    pub fn raise(&self, signal: ControlSignal) {
        self.0.fetch_or(signal as u8, Ordering::Release);
    }
    pub fn pending(&self) -> PendingControls {
        PendingControls(self.0.load(Ordering::Acquire))
    }
    pub fn take(&self) -> PendingControls {
        PendingControls(self.0.swap(0, Ordering::AcqRel))
    }
}

impl PendingControls {
    pub fn contains(self, signal: ControlSignal) -> bool {
        self.0 & signal as u8 != 0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}
