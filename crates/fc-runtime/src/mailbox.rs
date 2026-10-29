//! Ordinary messages can never block a UI/FFI producer, even on mutex contention.
//! File bytes, pixels, credentials and other large buffers do not belong here.
use std::{
    collections::VecDeque,
    mem::size_of,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, TryLockError,
    },
};

pub const COMMAND_CAPACITY: usize = 128;
pub const WORKER_CAPACITY: usize = 8;
pub const DESCRIPTOR_BYTE_BUDGET: usize = 4 * 1024 * 1024;

/// Report owned heap capacity, including nested strings/collections, with checked
/// arithmetic. Returning None rejects an overflow. Inline storage is added here.
pub trait MailboxMessage {
    fn heap_bytes(&self) -> Option<usize>;
}
impl MailboxMessage for String {
    fn heap_bytes(&self) -> Option<usize> {
        Some(self.capacity())
    }
}
impl MailboxMessage for Vec<u8> {
    fn heap_bytes(&self) -> Option<usize> {
        Some(self.capacity())
    }
}
impl MailboxMessage for u64 {
    fn heap_bytes(&self) -> Option<usize> {
        Some(0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitErrorKind {
    Busy,
    TooLarge,
    Closed,
    Faulted,
}
#[derive(Debug)]
pub struct SubmitError<T> {
    pub kind: SubmitErrorKind,
    pub message: T,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiveError {
    Empty,
    Busy,
    Closed,
    Faulted,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidBudget;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MailboxStats {
    pub items: usize,
    pub bytes: usize,
}
struct Entry<T> {
    message: T,
    bytes: usize,
}
struct Queue<T> {
    entries: VecDeque<Entry<T>>,
    bytes: usize,
}
struct Shared<T> {
    queue: Mutex<Queue<T>>,
    closed: AtomicBool,
    capacity: usize,
    byte_budget: usize,
}
pub struct MailboxSender<T> {
    shared: Arc<Shared<T>>,
}
pub struct MailboxReceiver<T> {
    shared: Arc<Shared<T>>,
}
impl<T> Clone for MailboxSender<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

pub fn bounded<T: MailboxMessage>(
    capacity: usize,
    byte_budget: usize,
) -> Result<(MailboxSender<T>, MailboxReceiver<T>), InvalidBudget> {
    if capacity == 0 || byte_budget == 0 {
        return Err(InvalidBudget);
    }
    let shared = Arc::new(Shared {
        queue: Mutex::new(Queue {
            entries: VecDeque::with_capacity(capacity),
            bytes: 0,
        }),
        closed: AtomicBool::new(false),
        capacity,
        byte_budget,
    });
    Ok((
        MailboxSender {
            shared: shared.clone(),
        },
        MailboxReceiver { shared },
    ))
}

impl<T: MailboxMessage> MailboxSender<T> {
    pub fn try_send(&self, message: T) -> Result<(), SubmitError<T>> {
        let bytes = match message
            .heap_bytes()
            .and_then(|heap| heap.checked_add(size_of::<T>()))
        {
            Some(bytes) if bytes <= self.shared.byte_budget => bytes,
            _ => {
                return Err(SubmitError {
                    kind: SubmitErrorKind::TooLarge,
                    message,
                })
            }
        };
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(SubmitError {
                kind: SubmitErrorKind::Closed,
                message,
            });
        }
        let mut queue = match self.shared.queue.try_lock() {
            Ok(queue) => queue,
            Err(TryLockError::WouldBlock) => {
                return Err(SubmitError {
                    kind: SubmitErrorKind::Busy,
                    message,
                })
            }
            Err(TryLockError::Poisoned(_)) => {
                self.shared.closed.store(true, Ordering::Release);
                return Err(SubmitError {
                    kind: SubmitErrorKind::Faulted,
                    message,
                });
            }
        };
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(SubmitError {
                kind: SubmitErrorKind::Closed,
                message,
            });
        }
        if queue.entries.len() == self.shared.capacity
            || bytes > self.shared.byte_budget.saturating_sub(queue.bytes)
        {
            return Err(SubmitError {
                kind: SubmitErrorKind::Busy,
                message,
            });
        }
        queue.bytes += bytes;
        queue.entries.push_back(Entry { message, bytes });
        Ok(())
    }
    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
    }
    pub fn try_stats(&self) -> Option<MailboxStats> {
        self.shared.queue.try_lock().ok().map(|q| MailboxStats {
            items: q.entries.len(),
            bytes: q.bytes,
        })
    }
}

impl<T> MailboxReceiver<T> {
    pub fn try_recv(&self) -> Result<T, ReceiveError> {
        let mut queue = match self.shared.queue.try_lock() {
            Ok(queue) => queue,
            Err(TryLockError::WouldBlock) => return Err(ReceiveError::Busy),
            Err(TryLockError::Poisoned(_)) => {
                self.shared.closed.store(true, Ordering::Release);
                return Err(ReceiveError::Faulted);
            }
        };
        match queue.entries.pop_front() {
            Some(entry) => {
                queue.bytes -= entry.bytes;
                Ok(entry.message)
            }
            None if self.shared.closed.load(Ordering::Acquire) => Err(ReceiveError::Closed),
            None => Err(ReceiveError::Empty),
        }
    }
    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
    }
}
impl<T> Drop for MailboxReceiver<T> {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{ControlSignal, ControlSignals};
    #[test]
    fn count_and_actual_owned_capacity_are_independent_limits() {
        let byte_budget = 2 * size_of::<String>() + 40;
        let (send, recv) = bounded::<String>(2, byte_budget).unwrap();
        send.try_send(String::with_capacity(30)).unwrap();
        assert_eq!(
            send.try_send(String::with_capacity(30)).unwrap_err().kind,
            SubmitErrorKind::Busy
        );
        recv.try_recv().unwrap();
        assert_eq!(send.try_stats().unwrap().bytes, 0);
        send.try_send(String::new()).unwrap();
        send.try_send(String::new()).unwrap();
        assert_eq!(
            send.try_send(String::new()).unwrap_err().kind,
            SubmitErrorKind::Busy
        );
        assert_eq!(
            send.try_send(String::with_capacity(byte_budget + 1))
                .unwrap_err()
                .kind,
            SubmitErrorKind::TooLarge
        );
    }
    #[test]
    fn a_contended_producer_returns_ownership_without_waiting() {
        let (send, _recv) = bounded::<u64>(1, 8).unwrap_or_else(|_| unreachable!());
        let _held = send.shared.queue.lock().unwrap();
        let result = send.try_send(42).unwrap_err();
        assert_eq!(result.kind, SubmitErrorKind::Busy);
        assert_eq!(result.message, 42);
    }
    #[test]
    fn saturated_ordinary_work_does_not_consume_stop_or_hide() {
        let (send, recv) = bounded::<u64>(1, 8).unwrap();
        send.try_send(1).unwrap();
        assert_eq!(send.try_send(2).unwrap_err().kind, SubmitErrorKind::Busy);
        let controls = ControlSignals::default();
        controls.raise(ControlSignal::Stop);
        controls.raise(ControlSignal::ForegroundHidden);
        assert!(controls.take().contains(ControlSignal::Stop));
        controls.raise(ControlSignal::ForegroundHidden);
        assert!(controls.take().contains(ControlSignal::ForegroundHidden));
        recv.close();
        assert_eq!(send.try_send(3).unwrap_err().kind, SubmitErrorKind::Closed);
        assert_eq!(recv.try_recv(), Ok(1));
        assert_eq!(recv.try_recv(), Err(ReceiveError::Closed));
    }
}
