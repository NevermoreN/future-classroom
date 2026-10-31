//! The same two workers remain the only ordinary computation domain. During
//! restore their physical ownership moves once to the final coordinator.
use fc_runtime::worker::{JoinError, Task, WorkerHandle, WorkerPool};
use std::ops::Deref;

pub(super) struct SharedPool<T: Task> {
    owned: Option<WorkerPool<T>>,
    handle: WorkerHandle<T>,
}
impl<T: Task> SharedPool<T> {
    pub fn start() -> std::io::Result<Self> {
        let owned = WorkerPool::start()?;
        let handle = owned.handle();
        Ok(Self {
            owned: Some(owned),
            handle,
        })
    }
    pub fn take_for_restore(&mut self) -> Option<WorkerPool<T>> {
        self.owned.take()
    }
    pub fn handle(&self) -> WorkerHandle<T> {
        self.handle.clone()
    }
    pub fn request_stop(&self) {
        self.handle.request_stop();
    }
    pub fn is_faulted(&self) -> bool {
        self.handle.is_faulted()
    }
    /// An absent pool has another physical owner. The App also checks that
    /// coordinator's real join before it allows the native pump to end.
    pub fn is_finished(&self) -> bool {
        self.owned
            .as_ref()
            .is_none_or(|pool| pool.is_finished_with(fc_windows::system::thread_is_finished))
    }
    pub fn join(mut self) -> Result<(), JoinError> {
        self.owned.take().map_or(Ok(()), WorkerPool::join)
    }
}
impl<T: Task> Deref for SharedPool<T> {
    type Target = WorkerPool<T>;
    fn deref(&self) -> &Self::Target {
        self.owned
            .as_ref()
            .expect("ordinary work is forbidden after restore handoff")
    }
}
