//! Real Windows synchronization and WIC tests. Each synthetic protected fixture
//! owns its paths before deployment preparation; no desktop, input or source asset.
use super::*;
use crate::{
    materials::{files::ControlledFiles, raster::RasterCache},
    system::DataPaths,
    InstanceGuard,
};
use fc_domain::{
    ids::{LayoutNodeId, MediaVersionId},
    layout::LayoutScope,
    profile_content::{ContentSlotKey, ImageFormat, MediaReference},
};
use std::{
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    instance: Option<InstanceGuard>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Wic-Admission-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut fixture = Self {
            root,
            instance: None,
        };
        DataPaths::isolated_test(fixture.root.clone())
            .unwrap()
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        fixture.instance = Some(InstanceGuard::acquire(&fixture.root).unwrap());
        fixture
    }
    fn source(&self) -> LockedFile {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&78u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&54u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&4i32.to_le_bytes());
        bytes.extend_from_slice(&2i32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&24u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        for _ in 0..2 {
            bytes.extend_from_slice(&[0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255]);
        }
        assert_eq!(bytes.len(), 78);
        let files = ControlledFiles::new(&self.root).unwrap();
        let path = self.root.join("quarantine").join("gate.bmp");
        let mut file = files.create_new(&path).unwrap();
        file.file_mut().write_all(&bytes).unwrap();
        let mut source = file.seal().unwrap();
        source.file_mut().seek(SeekFrom::Start(0)).unwrap();
        source
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.instance.take());
        let removed = std::fs::remove_dir_all(&self.root);
        if !thread::panicking() {
            removed.unwrap();
        }
    }
}
fn reference() -> MediaReference {
    MediaReference {
        version_id: MediaVersionId::new(7).unwrap(),
        owner: ContentSlotKey {
            scope: LayoutScope::Public,
            node_id: LayoutNodeId::new(41).unwrap(),
        },
        sha256: [
            237, 138, 223, 91, 9, 234, 181, 68, 189, 135, 125, 199, 189, 22, 107, 247, 80, 64, 90,
            124, 78, 77, 87, 247, 13, 35, 224, 116, 35, 120, 183, 43,
        ],
        bytes: 78,
        width: 4,
        height: 2,
        format: ImageFormat::Bmp,
    }
}

#[test]
fn waiting_validation_cancels_without_decoder_or_early_owner_release() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let source = fixture.source();
    thread::scope(|scope| {
        // This guard lives inside the scope closure so a failed assertion drops
        // it BEFORE scoped threads are joined, including panic cleanup.
        let owner = DECODER.lock().unwrap();
        let canceled = Arc::new(AtomicBool::new(false));
        let flag = canceled.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let source = &source;
        let worker = scope.spawn(move || {
            validate_image(source, MaterialKind::Bmp, || {
                let _ = tx.try_send(());
                flag.load(Ordering::Acquire)
            })
            .map_err(|error| error.code())
        });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(20));
        assert!(
            !worker.is_finished(),
            "legal contention must wait, not fail ERROR_BUSY"
        );
        canceled.store(true, Ordering::Release);
        let error = worker.join().unwrap().unwrap_err();
        assert_eq!(error, ERROR_CANCELLED.to_hresult());
        assert!(
            matches!(DECODER.try_lock(), Err(std::sync::TryLockError::WouldBlock)),
            "canceling another waiter cannot release the physical decoder owner"
        );
        drop(owner);
    });
}

#[test]
fn admission_original_deadline_and_post_acquire_cancel_precede_wic() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let owner = DECODER.lock().unwrap();
    let budget = DecoderBudget {
        started: Instant::now() - Duration::from_secs(31),
        cpu_started: crate::system::current_thread_cpu_time().unwrap(),
    };
    let error = budget
        .admit(&|| true)
        .expect_err("expired budget must not wait or acquire");
    assert_eq!(error.code(), ERROR_CANCELLED.to_hresult());
    assert!(matches!(
        DECODER.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
    drop(owner);
    let checks = AtomicUsize::new(0);
    let budget = DecoderBudget::new().unwrap();
    let error = budget
        .admit(&|| checks.fetch_add(1, Ordering::AcqRel) == 0)
        .expect_err("revocation at acquisition must release without WIC");
    assert_eq!(error.code(), ERROR_CANCELLED.to_hresult());
    assert_eq!(checks.load(Ordering::Acquire), 2);
    assert!(DECODER.try_lock().is_ok());
}

#[test]
fn legal_validation_waits_for_physical_owner_then_full_wic_completes() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let source = fixture.source();
    thread::scope(|scope| {
        let owner = DECODER.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        let source = &source;
        let worker = scope.spawn(move || {
            validate_image(source, MaterialKind::Bmp, || {
                let _ = tx.try_send(());
                false
            })
            .map_err(|error| error.code())
        });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(20));
        assert!(!worker.is_finished());
        drop(owner);
        let report = worker.join().unwrap().unwrap();
        assert_eq!(
            (report.width, report.height, report.decoded_bytes),
            (4, 2, 32)
        );
        assert!(DECODER.try_lock().is_ok());
    });
}

#[test]
fn raster_same_gate_cancel_charges_no_pixels_and_retry_decodes_after_release() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let source = fixture.source();
    let cache = Arc::new(RasterCache::new());
    let reference = reference();
    thread::scope(|scope| {
        let owner = DECODER.lock().unwrap();
        let current = Arc::new(AtomicBool::new(true));
        let flag = current.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let cache_worker = cache.clone();
        let source = &source;
        let reference = &reference;
        let worker = scope.spawn(move || {
            cache_worker
                .decode_verified(source, reference, reference.sha256, 4, 2, || {
                    let _ = tx.try_send(());
                    flag.load(Ordering::Acquire)
                })
                .map_err(|error| error.code())
        });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(20));
        assert!(!worker.is_finished());
        assert_eq!(cache.bytes_in_use(), 0);
        current.store(false, Ordering::Release);
        let error = worker
            .join()
            .unwrap()
            .err()
            .expect("canceled raster must not decode");
        assert_eq!(error, ERROR_CANCELLED.to_hresult());
        assert_eq!(cache.bytes_in_use(), 0);
        assert!(cache.get(reference, 4, 2).unwrap().is_none());
        assert!(matches!(
            DECODER.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        let (tx, rx) = mpsc::sync_channel(1);
        let cache_worker = cache.clone();
        let worker = scope.spawn(move || {
            cache_worker
                .decode_verified(source, reference, reference.sha256, 4, 2, || {
                    let _ = tx.try_send(());
                    true
                })
                .map_err(|error| error.code())
        });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(20));
        assert!(!worker.is_finished());
        assert_eq!(cache.bytes_in_use(), 0);
        drop(owner);
        let frame = worker.join().unwrap().unwrap();
        assert_eq!(frame.source(), reference);
        assert_eq!(frame.pixels().len(), 32);
        assert_eq!(&frame.pixels()[..4], &[0, 0, 255, 255]);
        assert_eq!(cache.bytes_in_use(), 32);
        assert!(cache.get(reference, 4, 2).unwrap().is_some());
        assert!(DECODER.try_lock().is_ok());
    });
}
