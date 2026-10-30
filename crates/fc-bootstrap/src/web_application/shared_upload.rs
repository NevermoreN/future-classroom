//! One process-wide physical budget shared by every authenticated Web upload.
//! A permit belongs to the real file/proof owner, never an application registry.
use std::sync::atomic::{AtomicUsize, Ordering};
static PHYSICAL_UPLOADS: AtomicUsize = AtomicUsize::new(0);
pub(super) struct PhysicalUploadPermit;
impl PhysicalUploadPermit {
    pub(super) fn take() -> Option<Self> {
        PHYSICAL_UPLOADS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 2).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for PhysicalUploadPermit {
    fn drop(&mut self) {
        let previous = PHYSICAL_UPLOADS.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0);
    }
}

// Independent fixture roots must not compete with each other's intentional
// process saturation. This test-only lock does not participate in production
// admission or serialize the two real owners exercised within each regression.
#[cfg(test)]
pub(super) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static SCENARIOS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SCENARIOS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
