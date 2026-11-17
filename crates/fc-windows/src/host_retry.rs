//! Bounded pacing shared by initial Shell readiness and taskbar recovery.
use std::time::{Duration, Instant};

const BUDGET: Duration = Duration::from_secs(60);
const INTERVAL: Duration = Duration::from_secs(1);

pub(crate) enum RetryDecision {
    Attempt,
    Wait,
    Expired,
}

pub(crate) struct HostRetry {
    started: Instant,
    next: Instant,
    pub(crate) attempts: u32,
}

impl HostRetry {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            started: now,
            next: now,
            attempts: 0,
        }
    }

    pub(crate) fn decide(&mut self, now: Instant) -> RetryDecision {
        if now.duration_since(self.started) >= BUDGET {
            RetryDecision::Expired
        } else if now < self.next {
            RetryDecision::Wait
        } else {
            self.next = now + INTERVAL;
            self.attempts += 1;
            RetryDecision::Attempt
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_do_not_bypass_pacing_or_extend_deadline() {
        let start = Instant::now();
        let mut retry = HostRetry::new(start);
        assert!(matches!(retry.decide(start), RetryDecision::Attempt));
        for millis in 1..1000 {
            assert!(matches!(
                retry.decide(start + Duration::from_millis(millis)),
                RetryDecision::Wait
            ));
        }
        assert!(matches!(
            retry.decide(start + INTERVAL),
            RetryDecision::Attempt
        ));
        assert_eq!(retry.attempts, 2);
        assert!(matches!(
            retry.decide(start + BUDGET),
            RetryDecision::Expired
        ));
        assert_eq!(retry.attempts, 2);
    }

    #[test]
    fn separate_taskbar_recovery_gets_its_own_finite_window() {
        let start = Instant::now();
        let mut startup = HostRetry::new(start);
        assert!(matches!(
            startup.decide(start + Duration::from_secs(59)),
            RetryDecision::Attempt
        ));
        assert!(matches!(
            startup.decide(start + BUDGET),
            RetryDecision::Expired
        ));
        let restart = start + Duration::from_secs(120);
        let mut recovery = HostRetry::new(restart);
        assert!(matches!(recovery.decide(restart), RetryDecision::Attempt));
        assert!(matches!(
            recovery.decide(restart + BUDGET),
            RetryDecision::Expired
        ));
    }
}
