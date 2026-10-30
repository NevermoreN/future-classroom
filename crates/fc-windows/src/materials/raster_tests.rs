use super::*;
use crate::{materials::files::ControlledFiles, system::DataPaths, InstanceGuard};
use fc_domain::{ids::LayoutNodeId, layout::LayoutScope};
use std::{
    io::Write,
    path::PathBuf,
    sync::{atomic::AtomicU64, Barrier},
};

fn reference(node: u64) -> MediaReference {
    MediaReference {
        version_id: MediaVersionId::new(1).unwrap(),
        owner: ContentSlotKey {
            scope: LayoutScope::Public,
            node_id: LayoutNodeId::new(node).unwrap(),
        },
        // SHA-256 of the complete 78-byte synthetic BMP made below. This is a
        // fixed known fixture digest, never a caller-provided production proof.
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
fn containment_handles_extreme_aspects_without_overflow_crop_or_enlargement() {
    assert_eq!(contained_pixels(4000, 2000, 333, 333).unwrap(), (333, 166));
    assert_eq!(contained_pixels(2000, 4000, 333, 333).unwrap(), (166, 333));
    assert_eq!(contained_pixels(4, 2, 800, 600).unwrap(), (4, 2));
    assert_eq!(contained_pixels(1, 8_000_000, 800, 600).unwrap(), (1, 600));
    assert_eq!(contained_pixels(8_000_000, 1, 800, 600).unwrap(), (800, 1));
    for bounds in [
        (0, 1, 1, 1),
        (1, 1, 0, 1),
        (1, 1, 1, 0),
        (4000, 2001, 1, 1),
        (u32::MAX, u32::MAX, 1, 1),
    ] {
        assert!(contained_pixels(bounds.0, bounds.1, bounds.2, bounds.3).is_err());
    }
}

#[test]
fn physical_frame_keeps_budget_after_cache_clear_until_final_arc_drops() {
    let cache = RasterCache::new();
    let source = reference(20);
    let key = RasterKey {
        owner: source.owner,
        version: source.version_id,
        sha256: source.sha256,
        width: 4,
        height: 2,
    };
    let retained = Arc::new(RasterFrame {
        key,
        source: source.clone(),
        pixels: vec![0; 32],
        _reservation: cache.budget.reserve(32).unwrap(),
    });
    cache.entries.lock().unwrap().insert(key, retained.clone());
    let painter = cache.get(&source, 4, 2).unwrap().unwrap();
    cache.clear().unwrap();
    assert!(cache.get(&source, 4, 2).unwrap().is_none());
    assert_eq!(cache.bytes_in_use(), 32);
    assert!(cache.budget.reserve(IMAGE_CACHE_BYTES - 31).is_err());
    drop(retained);
    assert_eq!(cache.bytes_in_use(), 32, "painting still owns actual bytes");
    assert_eq!(painter.pixels().len(), 32);
    drop(painter);
    assert_eq!(cache.bytes_in_use(), 0);
    let entire_budget = cache.budget.reserve(IMAGE_CACHE_BYTES).unwrap();
    assert!(cache.budget.reserve(1).is_err());
    drop(entire_budget);
    assert_eq!(cache.bytes_in_use(), 0);
}

#[test]
fn concurrent_reservations_cannot_oversubscribe_or_return_each_others_bytes() {
    let budget = Arc::new(Budget {
        used: AtomicUsize::new(0),
        limit: 64,
    });
    let barrier = Arc::new(Barrier::new(2));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let budget = budget.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            let reservation = budget.reserve(40).map_err(|_| ());
            barrier.wait();
            reservation
        }));
    }
    let reservations: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(reservations.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(budget.used.load(Ordering::Acquire), 40);
    drop(reservations);
    assert_eq!(budget.used.load(Ordering::Acquire), 0);
    assert!(budget.reserve(usize::MAX).is_err());
    assert_eq!(budget.used.load(Ordering::Acquire), 0);
}

static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    instance: Option<InstanceGuard>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Raster-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let paths = DataPaths::isolated_test(root.clone()).unwrap();
        let deployment = paths.prepare_for_current_user().unwrap();
        deployment.read_back().unwrap();
        Self {
            instance: Some(InstanceGuard::acquire(&root).unwrap()),
            root,
        }
    }
    fn bmp(&self) -> LockedFile {
        let files = ControlledFiles::new(&self.root).unwrap();
        let path = files
            .directory(&["content-quarantine".into()])
            .unwrap()
            .join("synthetic.bmp");
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
        let mut staging = files.create_new(&path).unwrap();
        staging.file_mut().write_all(&bytes).unwrap();
        staging.seal().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.instance.take());
        let result = std::fs::remove_dir_all(&self.root);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

#[test]
fn real_wic_decodes_same_pinned_bmp_after_hash_eof_and_keeps_node_identity() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let source = fixture.bmp();
    let mut reader = source.file();
    reader.seek(SeekFrom::End(0)).unwrap();
    let cache = RasterCache::new();
    let reference = reference(20);
    let frame = cache
        .decode_verified(&source, &reference, reference.sha256, 40, 40, || true)
        .unwrap();
    assert_eq!(frame.key().width, 4);
    assert_eq!(frame.key().height, 2);
    assert_eq!(frame.stride(), 16);
    assert_eq!(
        &frame.pixels()[..16],
        &[0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255]
    );
    let hit = cache
        .decode_verified(&source, &reference, reference.sha256, 40, 40, || true)
        .unwrap();
    assert!(Arc::ptr_eq(&frame, &hit));
    let mut other = reference.clone();
    other.owner.node_id = LayoutNodeId::new(21).unwrap();
    assert!(cache.get(&other, 40, 40).unwrap().is_none());
    let independent = cache
        .decode_verified(&source, &other, other.sha256, 40, 40, || true)
        .unwrap();
    assert!(!Arc::ptr_eq(&frame, &independent));
    assert_eq!(independent.key().owner, other.owner);
    let scaled = cache
        .decode_verified(&source, &reference, reference.sha256, 2, 2, || true)
        .unwrap();
    assert_eq!((scaled.key().width, scaled.key().height), (2, 1));
    assert_eq!(scaled.pixels().len(), 8);
    assert!(scaled.pixels().chunks_exact(4).all(|p| p[3] == 255));
    assert_eq!(cache.bytes_in_use(), 72);
    cache.clear().unwrap();
    assert_eq!(cache.bytes_in_use(), 72);
    drop((frame, hit, independent, scaled));
    assert_eq!(cache.bytes_in_use(), 0);
    // Preparation can finish before the UI takes any Arc. A subsequent node's
    // decode must leave the earlier ready frame cached when there is room.
    drop(
        cache
            .decode_verified(&source, &reference, reference.sha256, 4, 2, || true)
            .unwrap(),
    );
    drop(
        cache
            .decode_verified(&source, &other, other.sha256, 4, 2, || true)
            .unwrap(),
    );
    assert!(cache.get(&reference, 4, 2).unwrap().is_some());
    assert!(cache.get(&other, 4, 2).unwrap().is_some());
    assert_eq!(cache.bytes_in_use(), 64);
    cache.clear().unwrap();
    assert_eq!(cache.bytes_in_use(), 0);
}

#[test]
fn verification_metadata_and_cancellation_fail_without_leaking_decode_reservations() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let source = fixture.bmp();
    let cache = RasterCache::new();
    let reference = reference(20);
    assert!(cache
        .decode_verified(&source, &reference, [0; 32], 4, 2, || true)
        .is_err());
    assert!(cache
        .decode_verified(&source, &reference, reference.sha256, 4, 2, || false)
        .is_err());
    let mut wrong = reference.clone();
    wrong.width = 3;
    assert!(cache
        .decode_verified(&source, &wrong, wrong.sha256, 4, 2, || true)
        .is_err());
    wrong = reference.clone();
    wrong.format = ImageFormat::Png;
    assert!(cache
        .decode_verified(&source, &wrong, wrong.sha256, 4, 2, || true)
        .is_err());
    wrong = reference.clone();
    wrong.bytes -= 1;
    assert!(cache
        .decode_verified(&source, &wrong, wrong.sha256, 4, 2, || true)
        .is_err());
    assert_eq!(cache.bytes_in_use(), 0);
    assert!(cache.get(&reference, 4, 2).unwrap().is_none());
}
