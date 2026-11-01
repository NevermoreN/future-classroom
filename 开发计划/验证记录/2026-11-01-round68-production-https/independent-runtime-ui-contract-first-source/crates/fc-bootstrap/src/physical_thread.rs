use std::thread::JoinHandle;

/// Windows UI owners must not join while native TLS destruction still runs.
/// Other hosts have no native application pump in this deployment.
pub(crate) fn is_finished(thread: &JoinHandle<()>) -> bool {
    #[cfg(windows)]
    {
        fc_windows::system::thread_is_finished(thread)
    }
    #[cfg(not(windows))]
    {
        thread.is_finished()
    }
}
