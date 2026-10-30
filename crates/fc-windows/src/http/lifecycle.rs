//! OS-independent ownership ledger, shared by the native adapter and fake tests.
use fc_ports::http::{HttpError, HttpState, RequestToken, TokenIssuer, MAX_REQUESTS};
/// Win32 synchronous success still queues IOCP; only a synchronous error other
/// than ERROR_IO_PENDING means no native operation was accepted.
pub(super) fn submission_accepted(code: u32) -> bool {
    matches!(code, 0 | 997)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Head,
    Body,
    SendHead { last: bool },
    SendBody { last: bool },
}
#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    token: Option<RequestToken>,
    active: bool,
    operation: Option<Operation>,
    cancel: bool,
}
pub(super) struct Ledger {
    pub generation: u64,
    pub state: HttpState,
    entries: [Entry; MAX_REQUESTS],
    issuer: TokenIssuer,
}
impl Ledger {
    pub fn new() -> Result<Self, HttpError> {
        let issuer = TokenIssuer::new()?;
        Ok(Self {
            generation: issuer.generation(),
            state: HttpState::Running,
            entries: [Entry::default(); MAX_REQUESTS],
            issuer,
        })
    }
    pub fn reserve(&mut self) -> Result<RequestToken, HttpError> {
        if self.state != HttpState::Running {
            return Err(HttpError::Stopping);
        }
        let n = self
            .entries
            .iter()
            .position(|e| !e.active)
            .ok_or(HttpError::Busy)?;
        let token = self.issuer.issue(n)?;
        self.entries[n] = Entry {
            token: Some(token),
            active: true,
            ..Entry::default()
        };
        self.token(n)
    }
    pub fn token(&self, n: usize) -> Result<RequestToken, HttpError> {
        let e = self
            .entries
            .get(n)
            .filter(|e| e.active)
            .ok_or(HttpError::StaleRequest)?;
        e.token.ok_or(HttpError::StaleRequest)
    }
    pub fn check(&self, t: RequestToken) -> Result<(), HttpError> {
        if t.generation() != self.generation || self.token(t.slot())? != t {
            return Err(HttpError::StaleRequest);
        }
        Ok(())
    }
    pub fn begin(&mut self, t: RequestToken, op: Operation) -> Result<(), HttpError> {
        self.check(t)?;
        if self.state != HttpState::Running {
            return Err(HttpError::Stopping);
        }
        let e = &mut self.entries[t.slot()];
        if e.operation.is_some() || e.cancel {
            return Err(HttpError::Busy);
        }
        e.operation = Some(op);
        Ok(())
    }
    pub fn begin_cancel(&mut self, t: RequestToken) -> Result<bool, HttpError> {
        self.check(t)?;
        let e = &mut self.entries[t.slot()];
        if e.cancel {
            return Ok(false);
        }
        e.cancel = true;
        Ok(true)
    }
    pub fn complete(
        &mut self,
        n: usize,
        key: u64,
        cancel: bool,
    ) -> Result<Option<Operation>, HttpError> {
        if key != self.generation {
            return Err(HttpError::Invariant);
        }
        let e = self
            .entries
            .get_mut(n)
            .filter(|e| e.active)
            .ok_or(HttpError::Invariant)?;
        if cancel {
            if !e.cancel {
                return Err(HttpError::Invariant);
            }
            e.cancel = false;
            Ok(None)
        } else {
            e.operation.take().map(Some).ok_or(HttpError::Invariant)
        }
    }
    pub fn operation(&self, n: usize) -> Option<Operation> {
        self.entries[n].operation
    }
    pub fn cancel_pending(&self, n: usize) -> bool {
        self.entries[n].cancel
    }
    pub fn release(&mut self, t: RequestToken) -> Result<(), HttpError> {
        self.check(t)?;
        let e = &mut self.entries[t.slot()];
        if e.operation.is_some() || e.cancel {
            return Err(HttpError::Busy);
        }
        e.active = false;
        Ok(())
    }
    pub fn in_flight(&self) -> usize {
        self.entries
            .iter()
            .map(|e| usize::from(e.operation.is_some()) + usize::from(e.cancel))
            .sum()
    }
    pub fn active(&self) -> usize {
        self.entries.iter().filter(|e| e.active).count()
    }
    pub fn heads(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.operation == Some(Operation::Head))
            .count()
    }
    pub fn bodies(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.operation == Some(Operation::Body))
            .count()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sync_success_and_pending_keep_ownership_until_one_completion() {
        let mut l = Ledger::new().unwrap();
        let t = l.reserve().unwrap();
        l.begin(t, Operation::Head).unwrap();
        assert_eq!(l.release(t), Err(HttpError::Busy));
        assert_eq!(
            l.complete(t.slot(), l.generation, false).unwrap(),
            Some(Operation::Head)
        );
        l.release(t).unwrap();
        assert_eq!(
            l.complete(t.slot(), l.generation, false),
            Err(HttpError::Invariant)
        );
    }
    #[test]
    fn cancellation_ack_never_releases_the_original_io() {
        let mut l = Ledger::new().unwrap();
        let t = l.reserve().unwrap();
        l.begin(t, Operation::Body).unwrap();
        assert!(l.begin_cancel(t).unwrap());
        assert_eq!(l.in_flight(), 2);
        l.complete(t.slot(), l.generation, true).unwrap();
        assert_eq!(l.release(t), Err(HttpError::Busy));
        l.complete(t.slot(), l.generation, false).unwrap();
        l.release(t).unwrap();
    }
    #[test]
    fn closing_generation_cannot_publish_into_new_cycle_or_reuse_slot_early() {
        let mut l = Ledger::new().unwrap();
        let t = l.reserve().unwrap();
        l.begin(t, Operation::Body).unwrap();
        l.state = HttpState::Draining;
        assert_eq!(l.reserve(), Err(HttpError::Stopping));
        assert_eq!(
            l.complete(t.slot(), l.generation + 1, false),
            Err(HttpError::Invariant)
        );
        l.complete(t.slot(), l.generation, false).unwrap();
        l.release(t).unwrap();
        let mut next = Ledger::new().unwrap();
        let replacement = next.reserve().unwrap();
        assert_eq!(next.check(t), Err(HttpError::StaleRequest));
        assert_ne!(t, replacement);
    }
    #[test]
    fn both_accepted_return_codes_wait_but_inline_failure_withdraws_without_packet() {
        for code in [0, 997] {
            let mut l = Ledger::new().unwrap();
            let t = l.reserve().unwrap();
            l.begin(t, Operation::SendHead { last: true }).unwrap();
            assert!(submission_accepted(code));
            assert_eq!(l.release(t), Err(HttpError::Busy));
            assert_eq!(l.in_flight(), 1);
            l.complete(t.slot(), l.generation, false).unwrap();
            l.release(t).unwrap();
            assert_eq!(l.in_flight(), 0);
        }
        let mut l = Ledger::new().unwrap();
        let t = l.reserve().unwrap();
        l.begin(t, Operation::SendBody { last: true }).unwrap();
        assert!(!submission_accepted(87));
        l.complete(t.slot(), l.generation, false).unwrap();
        l.release(t).unwrap();
        assert_eq!(l.in_flight(), 0);
    }
    #[test]
    fn original_completion_before_cancel_ack_also_retains_the_slot() {
        let mut l = Ledger::new().unwrap();
        let t = l.reserve().unwrap();
        l.begin(t, Operation::Body).unwrap();
        l.begin_cancel(t).unwrap();
        l.complete(t.slot(), l.generation, false).unwrap();
        assert_eq!(l.release(t), Err(HttpError::Busy));
        l.complete(t.slot(), l.generation, true).unwrap();
        l.release(t).unwrap();
    }
    #[test]
    fn fixed_eight_slots_bound_all_admission() {
        let mut l = Ledger::new().unwrap();
        for _ in 0..MAX_REQUESTS {
            l.reserve().unwrap();
        }
        assert_eq!(l.reserve(), Err(HttpError::Busy));
    }
}
