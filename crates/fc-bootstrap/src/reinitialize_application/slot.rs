//! Closing and publication share one lock. A cancelled UI cannot observe an
//! empty slot, retire it, then receive a late prepared capability afterwards.
use std::sync::Mutex;

pub(super) struct PreparationSlot<T> {
    state: Mutex<State<T>>,
}
struct State<T> {
    closed: bool,
    value: Option<T>,
}
impl<T> PreparationSlot<T> {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                closed: false,
                value: None,
            }),
        }
    }
    pub fn close(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closed = true;
    }
    /// The rejected value goes back to the preparing worker for explicit
    /// physical discard. It is never silently dropped while holding the lock.
    pub fn publish(&self, value: T) -> Result<(), T> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed || state.value.is_some() {
            return Err(value);
        }
        state.value = Some(value);
        Ok(())
    }
    pub fn is_retained(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .value
            .is_some()
    }
    pub fn take_ready(&self) -> Option<T> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            None
        } else {
            state.value.take()
        }
    }
    pub fn retire(&self) -> Option<T> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        state.value.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc, Barrier};
    #[test]
    fn cancelled_just_before_publish_and_retired_empty_slot_cannot_receive_a_late_capability() {
        let slot = Arc::new(PreparationSlot::new());
        let before_publish = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let (discarded, observed) = mpsc::channel();
        let worker_slot = slot.clone();
        let worker_before = before_publish.clone();
        let worker_release = release.clone();
        let worker = std::thread::spawn(move || {
            let worker_id = std::thread::current().id();
            let prepared = "synthetic prepared capability";
            worker_before.wait();
            worker_release.wait();
            let rejected = worker_slot
                .publish(prepared)
                .expect_err("late publish must return owned capability");
            // Models the explicit Files::discard performed by Work::Prepare,
            // rather than a UI destructor or a second task freeing live input.
            discarded.send((rejected, worker_id)).unwrap();
            worker_id
        });
        before_publish.wait();
        slot.close();
        assert!(
            slot.retire().is_none(),
            "retirement may run before prepare returns"
        );
        release.wait();
        let worker_id = worker.join().unwrap();
        assert_eq!(
            observed.recv().unwrap(),
            ("synthetic prepared capability", worker_id)
        );
        assert!(!slot.is_retained());
        assert!(slot.take_ready().is_none());
    }
    #[test]
    fn published_before_cancel_is_retired_once_and_slot_never_reopens() {
        let slot = PreparationSlot::new();
        assert_eq!(slot.publish(7), Ok(()));
        assert_eq!(slot.publish(8), Err(8));
        slot.close();
        assert!(slot.take_ready().is_none());
        assert_eq!(slot.retire(), Some(7));
        assert_eq!(slot.retire(), None);
        assert_eq!(slot.publish(9), Err(9));
        assert!(!slot.is_retained());
    }
    #[test]
    fn confirmation_transfers_exact_capability_once_without_retiring_another_value() {
        let slot = PreparationSlot::new();
        assert_eq!(slot.publish(11), Ok(()));
        assert_eq!(slot.take_ready(), Some(11));
        assert_eq!(slot.take_ready(), None);
        slot.close();
        assert_eq!(slot.publish(12), Err(12));
        assert!(slot.retire().is_none());
    }
}
