//! One process-wide physical budget shared by every authenticated Web upload.
//! A permit belongs to the real file/proof owner, never an application registry.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
const SLOTS: usize = 2;
const FULL: usize = (1 << SLOTS) - 1;
static PHYSICAL_UPLOADS: AtomicUsize = AtomicUsize::new(0);
static QUARANTINED: AtomicUsize = AtomicUsize::new(0);
pub(super) struct PhysicalUploadPermit {
    slot: usize,
}
impl PhysicalUploadPermit {
    pub(super) fn take() -> Option<Self> {
        let previous = PHYSICAL_UPLOADS
            .try_update(Ordering::AcqRel, Ordering::Acquire, |mask| {
                if mask == FULL {
                    None
                } else {
                    Some(mask | (1 << ((!mask & FULL).trailing_zeros() as usize)))
                }
            })
            .ok()?;
        Some(Self {
            slot: (!previous & FULL).trailing_zeros() as usize,
        })
    }
}
impl Drop for PhysicalUploadPermit {
    fn drop(&mut self) {
        let bit = 1 << self.slot;
        let previous = PHYSICAL_UPLOADS.fetch_and(!bit, Ordering::AcqRel);
        debug_assert!(previous & bit != 0);
    }
}

struct Record {
    // Cleanup captures must retire before the original permit is returned.
    cleanup: Box<dyn FnMut() -> bool + Send>,
    permit: PhysicalUploadPermit,
}
static CLEANUPS: Mutex<[Option<Record>; SLOTS]> = Mutex::new([None, None]);

/// Transfer a failed exact cleanup and its ORIGINAL permit, never a new charge.
/// The permit's private slot guarantees an empty bounded destination, including
/// while another record's retry runs outside this mutex. No IO runs under it.
pub(super) fn quarantine_cleanup(
    permit: PhysicalUploadPermit,
    cleanup: impl FnMut() -> bool + Send + 'static,
) {
    let slot = permit.slot;
    let record = Record {
        cleanup: Box::new(cleanup),
        permit,
    };
    let mut records = CLEANUPS.lock().unwrap_or_else(|p| p.into_inner());
    // A non-Clone permit is the sole live owner of this exact slot. Seeing
    // another record here is an internal double-owner bug, never saturation.
    assert!(
        records[slot].is_none(),
        "duplicate physical upload cleanup owner"
    );
    records[slot] = Some(record);
    QUARANTINED.fetch_or(1 << slot, Ordering::Release);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UploadCleanupStatus {
    pub attempted: usize,
    pub reclaimed: usize,
    pub retained: usize,
}
/// Includes records temporarily outside the registry in a running retry.
pub fn quarantined_uploads() -> usize {
    QUARANTINED.load(Ordering::Acquire).count_ones() as usize
}

/// Shared worker/final coordinator ONLY. Retained objects forbid reporting
/// complete cleanup. A callback panic is negative and preserves the same owner.
pub fn retry_quarantined_uploads() -> UploadCleanupStatus {
    let mut work = {
        let mut records = CLEANUPS.lock().unwrap_or_else(|p| p.into_inner());
        std::array::from_fn::<_, SLOTS, _>(|slot| records[slot].take())
    };
    let mut status = UploadCleanupStatus::default();
    for record in work.iter_mut().filter_map(Option::take) {
        let mut record = record;
        status.attempted += 1;
        let cleaned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (record.cleanup)()))
            .unwrap_or(false);
        let slot = record.permit.slot;
        if cleaned {
            let Record { cleanup, permit } = record;
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(cleanup))).is_err() {
                // An unexpected destructor panic cannot prove native cleanup.
                // Preserve this exact bounded charge; never return a permit on
                // an unknown owner retirement or try an unrelated replacement.
                quarantine_cleanup(permit, || false);
                continue;
            }
            // Every real cleanup/captured owner has retired before this bit is
            // cleared. No new owner of this slot can exist until permit Drop.
            QUARANTINED.fetch_and(!(1 << slot), Ordering::AcqRel);
            drop(permit);
            status.reclaimed += 1;
        } else {
            let mut records = CLEANUPS.lock().unwrap_or_else(|p| p.into_inner());
            assert!(records[slot].is_none(), "duplicate retry cleanup owner");
            records[slot] = Some(record);
        }
    }
    status.retained = quarantined_uploads();
    status
}

#[cfg(test)]
mod cleanup_tests;

// Independent fixture roots must not compete with each other's intentional
// process saturation. This test-only lock does not participate in production
// admission or serialize the two real owners exercised within each regression.
#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static SCENARIOS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SCENARIOS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
