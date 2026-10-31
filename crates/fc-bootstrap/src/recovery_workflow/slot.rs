use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Empty,
    Working,
    Ready,
    Retired,
}
struct State<T> {
    generation: u64,
    phase: Phase,
    closing: bool,
    value: Option<T>,
}
struct Inner<T> {
    state: Mutex<State<T>>,
    // Explicit cycle: a last UI Arc must never run a heavyweight destructor.
    // It is broken only when close has been requested AND the slot is empty.
    retained: Mutex<Option<Arc<Inner<T>>>>,
}
pub(super) struct Slot<T>(Arc<Inner<T>>);
impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Slot<T> {
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn new() -> Self {
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                generation: 0,
                phase: Phase::Empty,
                closing: false,
                value: None,
            }),
            retained: Mutex::new(None),
        });
        *inner.retained.lock().expect("new slot") = Some(inner.clone());
        Self(inner)
    }
    pub fn reserve(&self) -> Option<u64> {
        let mut state = self.0.state.lock().ok()?;
        if state.closing || state.phase != Phase::Empty {
            return None;
        }
        state.generation = state.generation.checked_add(1)?;
        state.phase = Phase::Working;
        Some(state.generation)
    }
    pub fn permits(&self, generation: u64) -> bool {
        self.0.state.lock().is_ok_and(|state| {
            state.generation == generation && state.phase != Phase::Retired && !state.closing
        })
    }
    /// Worker only. A cancelled in-flight result is destroyed on THIS worker.
    pub fn publish(&self, generation: u64, value: T) -> bool {
        let Ok(mut state) = self.0.state.lock() else {
            // The caller still owns the value on this worker.
            return false;
        };
        if state.generation != generation || state.phase != Phase::Working || state.closing {
            return false;
        }
        state.value = Some(value);
        state.phase = Phase::Ready;
        true
    }
    /// Caller must be a shared worker or final coordinator, never the UI.
    pub fn take_ready(&self, generation: u64) -> Option<T> {
        let mut state = self.0.state.lock().ok()?;
        if state.generation != generation || state.phase != Phase::Ready || state.closing {
            return None;
        }
        let value = state.value.take()?;
        state.phase = Phase::Working;
        Some(value)
    }
    pub fn publish_irreversible(&self, generation: u64, value: T) -> bool {
        let Ok(mut state) = self.0.state.lock() else {
            return false;
        };
        if state.generation != generation || state.value.is_some() || state.phase == Phase::Empty {
            return false;
        }
        state.value = Some(value);
        state.phase = Phase::Ready;
        true
    }
    pub fn inspect<R>(&self, f: impl FnOnce(u64, Option<&T>) -> R) -> Option<R> {
        let state = self.0.state.lock().ok()?;
        Some(f(state.generation, state.value.as_ref()))
    }
    pub fn take_irreversible(&self, generation: u64) -> Option<T> {
        let mut state = self.0.state.lock().ok()?;
        if state.generation != generation {
            return None;
        }
        let value = state.value.take()?;
        state.phase = Phase::Working;
        Some(value)
    }
    pub fn retire(&self, generation: u64) {
        if let Ok(mut state) = self.0.state.lock() {
            if state.generation == generation && state.phase != Phase::Empty {
                state.phase = Phase::Retired;
            }
        }
    }
    pub fn retirement_pending(&self) -> bool {
        self.0
            .state
            .lock()
            .is_ok_and(|state| state.phase == Phase::Retired && state.value.is_some())
    }
    /// Only the existing shared worker calls this, then drops the returned T.
    pub fn take_retired(&self) -> Option<(u64, T)> {
        let mut state = self.0.state.lock().ok()?;
        if state.phase != Phase::Retired {
            return None;
        }
        let value = state.value.take()?;
        state.phase = Phase::Working;
        Some((state.generation, value))
    }
    /// Called AFTER the physical value destructor has finished.
    pub fn finish(&self, generation: u64) {
        if let Ok(mut state) = self.0.state.lock() {
            if state.generation == generation && state.value.is_none() {
                state.phase = Phase::Empty;
            }
        }
        self.release_if_closed_and_idle();
    }
    pub fn is_idle(&self) -> bool {
        self.0
            .state
            .lock()
            .is_ok_and(|state| state.phase == Phase::Empty)
    }
    pub fn request_close(&self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.closing = true;
            if state.phase != Phase::Empty {
                state.phase = Phase::Retired;
            }
        }
        self.release_if_closed_and_idle();
    }
    fn release_if_closed_and_idle(&self) {
        let idle = self.0.state.lock().is_ok_and(|state| {
            state.closing && state.phase == Phase::Empty && state.value.is_none()
        });
        if idle {
            if let Ok(mut retained) = self.0.retained.lock() {
                retained.take();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread};
    struct Destruction(mpsc::Sender<thread::ThreadId>);
    impl Drop for Destruction {
        fn drop(&mut self) {
            self.0.send(thread::current().id()).unwrap();
        }
    }
    #[test]
    fn retirement_does_not_free_or_reuse_the_slot_until_real_destruction() {
        let (send, receive) = mpsc::channel();
        let slot = Slot::new();
        let generation = slot.reserve().unwrap();
        assert!(slot.publish(generation, Destruction(send)));
        slot.retire(generation);
        assert!(receive.try_recv().is_err());
        assert!(slot.reserve().is_none());
        let worker_slot = slot.clone();
        let worker = thread::spawn(move || {
            let (generation, value) = worker_slot.take_retired().unwrap();
            assert!(worker_slot.reserve().is_none());
            drop(value);
            worker_slot.finish(generation);
            thread::current().id()
        });
        let worker_id = worker.join().unwrap();
        assert_eq!(receive.recv().unwrap(), worker_id);
        let next = slot.reserve().unwrap();
        slot.retire(generation); // stale UI handle cannot cancel its successor.
        assert!(slot.permits(next));
        slot.finish(next);
        slot.request_close();
    }
    #[test]
    fn last_ui_owner_does_not_drop_a_preview_and_close_waits_for_worker() {
        let (send, receive) = mpsc::channel();
        let slot = Slot::new();
        let weak = Arc::downgrade(&slot.0);
        let generation = slot.reserve().unwrap();
        assert!(slot.publish(generation, Destruction(send)));
        slot.request_close();
        assert!(!slot.is_idle());
        drop(slot);
        assert!(receive.try_recv().is_err());
        let retained = Slot(weak.upgrade().unwrap());
        thread::spawn(move || {
            let (generation, value) = retained.take_retired().unwrap();
            drop(value);
            retained.finish(generation);
        })
        .join()
        .unwrap();
        receive.recv().unwrap();
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn cancellation_during_preflight_keeps_task_busy_until_worker_returns() {
        let slot = Slot::<()>::new();
        let generation = slot.reserve().unwrap();
        slot.retire(generation);
        assert!(!slot.permits(generation));
        assert!(!slot.retirement_pending());
        assert!(!slot.is_idle());
        assert!(slot.reserve().is_none());
        assert!(!slot.publish(generation, ()));
        slot.finish(generation);
        assert!(slot.is_idle());
        slot.request_close();
    }
    #[test]
    fn committed_confirmation_survives_cancellation_and_rejected_ui_result() {
        let slot = Slot::new();
        let generation = slot.reserve().unwrap();
        assert!(slot.same(&slot.clone()));
        assert!(slot.publish(generation, "preview"));
        assert_eq!(slot.take_ready(generation), Some("preview"));
        // Models lock/cancel after SQL COMMIT but before the worker's result.
        slot.request_close();
        assert!(slot.publish_irreversible(generation, "confirmed"));
        assert_eq!(
            slot.inspect(|key, value| (key, value.copied())),
            Some((generation, Some("confirmed")))
        );
        assert!(slot.reserve().is_none());
        assert_eq!(slot.take_irreversible(generation), Some("confirmed"));
        assert!(!slot.is_idle());
        slot.finish(generation);
        assert!(slot.is_idle());
    }
}
