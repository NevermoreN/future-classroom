//! Read-only native proof that a Rust-owned Windows thread has terminated.
use std::{os::windows::io::AsRawHandle, thread::JoinHandle};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0},
    System::Threading::WaitForSingleObject,
};

/// Nonblocking completion check including TLS and native thread teardown.
/// Rust's `JoinHandle::is_finished` observes its main-function result packet,
/// which may finish while TLS destructors are still running. Only the native
/// thread object's terminal signal permits the caller to perform a quick join.
/// A failed query conservatively reports unfinished. No handle is closed,
/// consumed, duplicated, or passed to another thread by this operation.
pub fn thread_is_finished(thread: &JoinHandle<()>) -> bool {
    if !thread.is_finished() {
        return false;
    }
    // SAFETY: the borrowed live JoinHandle owns this real thread handle for the
    // full call. Timeout zero only queries its signal; it never waits or closes
    // the handle. WaitForSingleObject is available on the Win7 compatibility
    // baseline and includes thread-local/native destructor completion.
    unsafe { WaitForSingleObject(HANDLE(thread.as_raw_handle()), 0) == WAIT_OBJECT_0 }
}
