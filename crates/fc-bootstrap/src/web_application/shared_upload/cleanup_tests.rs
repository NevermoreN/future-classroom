use super::*;
use fc_windows::materials::files::{ControlledFiles, LockedFile};
use std::{
    io::Write,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn bounded_cleanup_keeps_original_slots_through_callback_panic_and_reentrant_observation() {
    let _scenario = test_guard();
    let first = PhysicalUploadPermit::take().unwrap();
    let second = PhysicalUploadPermit::take().unwrap();
    let allow = Arc::new(AtomicBool::new(false));
    let observed = allow.clone();
    quarantine_cleanup(first, move || {
        let expected = if observed.load(Ordering::Acquire) {
            1
        } else {
            2
        };
        assert_eq!(quarantined_uploads(), expected);
        let inner = retry_quarantined_uploads();
        assert_eq!(inner.attempted, 0);
        assert_eq!(inner.retained, expected);
        assert!(PhysicalUploadPermit::take().is_none());
        if !observed.load(Ordering::Acquire) {
            panic!("synthetic cleanup callback failure");
        }
        true
    });
    quarantine_cleanup(second, || true);
    let failed = retry_quarantined_uploads();
    assert_eq!(
        (failed.attempted, failed.reclaimed, failed.retained),
        (2, 1, 1)
    );
    let available = PhysicalUploadPermit::take().unwrap();
    assert!(PhysicalUploadPermit::take().is_none());
    allow.store(true, Ordering::Release);
    assert_eq!(retry_quarantined_uploads().retained, 0);
    drop(available);
}

#[test]
fn cleanup_capture_retires_before_its_physical_permit_is_returned() {
    let _scenario = test_guard();
    struct Witness(Arc<AtomicBool>);
    impl Drop for Witness {
        fn drop(&mut self) {
            self.0
                .store(PhysicalUploadPermit::take().is_none(), Ordering::Release);
        }
    }
    let permit = PhysicalUploadPermit::take().unwrap();
    let other = PhysicalUploadPermit::take().unwrap();
    let closed_before_release = Arc::new(AtomicBool::new(false));
    let witness = Witness(closed_before_release.clone());
    quarantine_cleanup(permit, move || {
        let _ = &witness;
        true
    });
    assert_eq!(retry_quarantined_uploads().reclaimed, 1);
    assert!(closed_before_release.load(Ordering::Acquire));
    let replacement = PhysicalUploadPermit::take().unwrap();
    assert!(PhysicalUploadPermit::take().is_none());
    drop(replacement);
    drop(other);
}

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn actual_windows_delete_conflict_retains_charge_then_retries_only_the_exact_inode() {
    let _scenario = test_guard();
    for replace in [false, true] {
        let directory = Directory(std::env::temp_dir().join(format!(
                "FutureClassroom-Test-Upload-Cleanup-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&directory.0).unwrap();
        let files = Arc::new(ControlledFiles::new(&directory.0).unwrap());
        let path = directory.0.join("owned.part");
        let mut original = files.create_new(&path).unwrap();
        original
            .file_mut()
            .write_all(b"synthetic owned bytes")
            .unwrap();
        let identity = original.identity().unwrap();
        original.finish().unwrap();
        let reader = LockedFile::open(&path, 128).unwrap();
        assert!(files
            .remove_object_expected(&path, false, identity, |_| true)
            .is_err());
        let permit = PhysicalUploadPermit::take().unwrap();
        let other = PhysicalUploadPermit::take().unwrap();
        let retry_files = files.clone();
        let retry_path = path.clone();
        quarantine_cleanup(permit, move || {
            retry_files
                .remove_object_expected(&retry_path, false, identity, |_| true)
                .is_ok()
        });
        assert_eq!(quarantined_uploads(), 1);
        assert!(PhysicalUploadPermit::take().is_none());
        let blocked = retry_quarantined_uploads();
        assert_eq!((blocked.reclaimed, blocked.retained), (0, 1));
        assert!(path.exists());
        drop(reader);
        if replace {
            files
                .remove_object_expected(&path, false, identity, |_| true)
                .unwrap();
            let mut replacement = files.create_new(&path).unwrap();
            replacement
                .file_mut()
                .write_all(b"different synthetic inode")
                .unwrap();
            let replacement_id = replacement.identity().unwrap();
            assert_ne!(replacement_id, identity);
            replacement.finish().unwrap();
            assert_eq!(retry_quarantined_uploads().retained, 1);
            assert!(
                path.exists(),
                "an unrelated replacement cannot be deleted by old cleanup"
            );
            files
                .remove_object_expected(&path, false, replacement_id, |_| true)
                .unwrap();
        }
        let cleaned = retry_quarantined_uploads();
        assert_eq!((cleaned.reclaimed, cleaned.retained), (1, 0));
        assert!(!path.exists());
        let available = PhysicalUploadPermit::take().unwrap();
        assert!(PhysicalUploadPermit::take().is_none());
        drop(available);
        drop(other);
        drop(files);
    }
}
