//! A single domain registers before capturing its runtime handle. This fence
//! prevents settings from reopening admission before the old domain retires.
use super::*;
#[cfg(test)]
mod tests;

/// Not Clone. Keep as the native domain's last field so all device/monitor
/// fields are dropped first. Drop grants no release or thread-exit evidence.
pub struct VoiceNativeParticipation {
    inner: Arc<Inner>,
}

impl VoiceService {
    pub fn register_native_domain(&self) -> Result<VoiceNativeParticipation> {
        self.check_open()?;
        self.inner
            .native_participating
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| BackendError::Storage(StorageError::Busy))?;
        Ok(VoiceNativeParticipation {
            inner: self.inner.clone(),
        })
    }

    pub(super) fn has_native_domain(&self) -> bool {
        self.inner.native_participating.load(Ordering::Acquire)
    }
}

impl Drop for VoiceNativeParticipation {
    fn drop(&mut self) {
        // The supervisor still owns/join-checks the old physical OS thread.
        // Never wake, resume, retry work or publish Closed from this marker.
        self.inner
            .native_participating
            .store(false, Ordering::Release);
    }
}
