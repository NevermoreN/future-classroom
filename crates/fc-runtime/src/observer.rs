//! Observer updates coalesce into one pending value. Render leases enforce the
//! separate maximum of two snapshots in use without relying on Arc counts.
use std::{
    ops::Deref,
    sync::{Arc, Mutex, TryLockError},
};

struct State<T> {
    latest: Option<Arc<T>>,
    renderers: usize,
}
pub struct LatestSnapshot<T> {
    state: Arc<Mutex<State<T>>>,
}
pub struct RenderSnapshot<T> {
    value: Arc<T>,
    state: Arc<Mutex<State<T>>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotBusy {
    Contended,
    RenderLimit,
    Empty,
    Faulted,
}

impl<T> Clone for LatestSnapshot<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}
impl<T> Default for LatestSnapshot<T> {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                latest: None,
                renderers: 0,
            })),
        }
    }
}
impl<T> LatestSnapshot<T> {
    /// Busy returns ownership so the producer can retain/retry only its newest
    /// snapshot. Do not queue a retry per lost observer notification.
    pub fn replace_latest(&self, value: Arc<T>) -> Result<(), (SnapshotBusy, Arc<T>)> {
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Err((SnapshotBusy::Contended, value)),
            Err(TryLockError::Poisoned(_)) => return Err((SnapshotBusy::Faulted, value)),
        };
        let old = state.latest.replace(value);
        drop(state);
        drop(old);
        Ok(())
    }
    pub fn take_for_render(&self) -> Result<RenderSnapshot<T>, SnapshotBusy> {
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Err(SnapshotBusy::Contended),
            Err(TryLockError::Poisoned(_)) => return Err(SnapshotBusy::Faulted),
        };
        if state.renderers >= 2 {
            return Err(SnapshotBusy::RenderLimit);
        }
        let value = state.latest.take().ok_or(SnapshotBusy::Empty)?;
        state.renderers += 1;
        Ok(RenderSnapshot {
            value,
            state: self.state.clone(),
        })
    }
}
impl<T> Deref for RenderSnapshot<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
impl<T> Drop for RenderSnapshot<T> {
    fn drop(&mut self) {
        // Cleanup cannot fail due to transient producer contention. This critical
        // section only updates a counter, and never invokes user code.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.renderers = state.renderers.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_latest_pending_and_two_live_render_versions_are_retained() {
        let sink = LatestSnapshot::default();
        sink.replace_latest(Arc::new(0)).unwrap();
        let first = sink.take_for_render().unwrap();
        sink.replace_latest(Arc::new(1)).unwrap();
        let second = sink.take_for_render().unwrap();
        let pending = Arc::new(2);
        let old_pending = Arc::downgrade(&pending);
        sink.replace_latest(pending).unwrap();
        sink.replace_latest(Arc::new(3)).unwrap();
        assert!(old_pending.upgrade().is_none());
        assert!(matches!(
            sink.take_for_render(),
            Err(SnapshotBusy::RenderLimit)
        ));
        drop(first);
        let third = sink.take_for_render().unwrap();
        assert_eq!(*second, 1);
        assert_eq!(*third, 3);
    }
}
