//! Cancellation is independent of ordinary mailbox capacity.
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Clone, Debug, Default)]
pub struct CancellationSource(Arc<AtomicU64>);

#[derive(Clone, Debug)]
pub struct CancellationToken {
    source: Arc<AtomicU64>,
    generation: u64,
}

impl CancellationSource {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn token(&self) -> CancellationToken {
        CancellationToken {
            source: self.0.clone(),
            generation: self.generation(),
        }
    }
    pub fn generation(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
    /// Saturation permanently cancels tokens, including subsequently created tokens.
    pub fn cancel(&self) -> u64 {
        let previous = self
            .0
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                Some(n.saturating_add(1))
            })
            .unwrap_or_else(|n| n);
        previous.saturating_add(1)
    }
}

impl CancellationToken {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn is_cancelled(&self) -> bool {
        let now = self.source.load(Ordering::Acquire);
        now == u64::MAX || now != self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_never_revives_an_old_token() {
        let source = CancellationSource::new();
        let first = source.token();
        source.cancel();
        let second = source.token();
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        source.cancel();
        assert!(second.is_cancelled());
    }
    #[test]
    fn overflow_cannot_revalidate_tokens() {
        let source = CancellationSource(Arc::new(AtomicU64::new(u64::MAX - 1)));
        let token = source.token();
        assert_eq!(source.cancel(), u64::MAX);
        assert!(token.is_cancelled());
        assert!(source.token().is_cancelled());
        assert_eq!(source.cancel(), u64::MAX);
    }
}
