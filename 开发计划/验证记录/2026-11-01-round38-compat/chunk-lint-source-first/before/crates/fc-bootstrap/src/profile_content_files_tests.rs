//! Real protected filesystem and WIC preparation, deliberately without claiming
//! SQLite authorization/commit or full desktop integration from adapter tests.
use super::*;
use fc_domain::{
    ids::*,
    time::{LocalDate, LocalDateTime},
};
use fc_ports::prepare_profile_content;
use fc_windows::{system::DataPaths, InstanceGuard, SystemRandom};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
static NATIVE_FILES_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct Fixture {
    root: PathBuf,
    instance: Option<InstanceGuard>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Content-Files-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DataPaths::isolated_test(root.clone())
            .unwrap()
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        Self {
            instance: Some(InstanceGuard::acquire(&root).unwrap()),
            root,
        }
    }
    fn input(&self, name: &str, bytes: &[u8]) -> SelectedFile {
        let files = ControlledFiles::new(&self.root).unwrap();
        let path = self.root.join("quarantine").join(name);
        let mut new = files.create_new(&path).unwrap();
        new.file_mut().write_all(bytes).unwrap();
        new.finish().unwrap();
        SelectedFile::isolated_probe(&self.root, name).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.instance.take());
        let cleaned = std::fs::remove_dir_all(&self.root);
        if !std::thread::panicking() {
            cleaned.unwrap();
        }
    }
}
fn bmp(width: u32, height: u32) -> Vec<u8> {
    let stride = (width * 3).div_ceil(4) * 4;
    let pixels = stride * height;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&(54 + pixels).to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&54u32.to_le_bytes());
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(width as i32).to_le_bytes());
    bytes.extend_from_slice(&(height as i32).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&pixels.to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    for _ in 0..height {
        for _ in 0..width {
            bytes.extend_from_slice(&[0, 0, 255]);
        }
        bytes.resize(bytes.len() + (stride - width * 3) as usize, 0);
    }
    bytes
}
fn admission(input: &ContentSources, kind: ContentPayloadKind) -> ContentAdmission {
    let slot = ContentSlotKey {
        scope: LayoutScope::Public,
        node_id: LayoutNodeId::new(20).unwrap(),
    };
    let operation = ContentOperation::Publish(kind);
    ContentAdmission {
        request: BeginContentRequest {
            command_id: CommandId::new(1).unwrap(),
            slot,
            expected_revision: Revision::default(),
            operation,
            payload_fingerprint: input.intent_fingerprint(slot, operation).unwrap(),
            expires_at: LocalDateTime::new(LocalDate::new(2099, 1, 1).unwrap(), 0).unwrap(),
        },
        sequence: 1,
        actor_id: PrincipalId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
    }
}
fn reservation(input: &ContentSources, admission: &ContentAdmission) -> MediaReservation {
    let bytes = input.image_sizes().unwrap();
    MediaReservation {
        admission: admission.clone(),
        ids: (1..=bytes.len())
            .map(|n| MediaVersionId::new(n as u64).unwrap())
            .collect(),
        bytes,
    }
}
fn ticket(id: MediaVersionId) -> MediaRetirement {
    MediaRetirement {
        id,
        revision: Revision::new(1),
    }
}

#[test]
fn native_txt_preparation_preserves_utf16_and_hashes_actual_text_not_intent() {
    let fixture = Fixture::new();
    let files = ControlledContentFiles::new(&fixture.root).unwrap();
    let expected = "\t中文😀\r\n第二行\n";
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(expected.encode_utf16().flat_map(u16::to_be_bytes));
    let selected = fixture.input("synthetic.txt", &bytes);
    let input = ContentSources::open(LocalContentInput::Txt(selected), &mut SystemRandom).unwrap();
    let admission = admission(&input, ContentPayloadKind::Text);
    let mut preparation = files.preparation(None, &|| true);
    let prepared = prepare_profile_content(&mut preparation, admission.clone(), input).unwrap();
    assert_eq!(prepared.payload(), &ContentPayload::Text(expected.into()));
    assert_eq!(
        prepared.digests().text_sha256,
        Some(fc_security::sha256(expected.as_bytes()))
    );
    assert_ne!(
        prepared.digests().payload_sha256,
        admission.request.payload_fingerprint
    );
    assert_eq!(
        prepared.digests().payload_sha256,
        fc_security::sha256(
            &prepared
                .payload()
                .fingerprint_bytes(admission.request.slot)
                .unwrap()
        )
    );
}

#[test]
fn real_ordered_image_bundle_seals_hashes_decodes_and_pins_until_commit_owner_exits() {
    let _serial = NATIVE_FILES_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let files = ControlledContentFiles::new(&fixture.root).unwrap();
    let first = bmp(4, 2);
    let second = bmp(2, 1);
    let input = ContentSources::open(
        LocalContentInput::Images(vec![
            fixture.input("first.bmp", &first),
            fixture.input("second.bmp", &second),
        ]),
        &mut SystemRandom,
    )
    .unwrap();
    let admission = admission(&input, ContentPayloadKind::Images);
    let reservation = reservation(&input, &admission);
    let mut preparation = files.preparation(Some(&reservation), &|| true);
    let prepared = prepare_profile_content(&mut preparation, admission.clone(), input).unwrap();
    let ContentPayload::Images(pages) = prepared.payload() else {
        panic!("image proof");
    };
    assert_eq!(pages.len(), 2);
    assert_eq!((pages[0].width, pages[0].height), (4, 2));
    assert_eq!((pages[1].width, pages[1].height), (2, 1));
    assert_eq!(pages[0].sha256, fc_security::sha256(&first));
    assert_eq!(pages[1].sha256, fc_security::sha256(&second));
    assert_eq!(pages[0].version_id, reservation.ids[0]);
    assert_eq!(pages[1].version_id, reservation.ids[1]);
    assert!(pages
        .iter()
        .all(|page| page.owner == admission.request.slot));
    let original = files.original(pages[0].version_id, ImageFormat::Bmp);
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(&original)
        .is_err());
    assert!(
        files.remove_retired(&ticket(pages[0].version_id)).is_err(),
        "actual pinned preparation forbids physical deletion"
    );
    // Commit must finish and drop the sealed exclusive file object before a
    // demand hydration opens a new reader. Expiry never bypasses physical pins.
    drop(preparation);
    let cache = RasterCache::new();
    let frame = files
        .decode_current(&cache, &pages[0], 2, 2, &|| true)
        .unwrap();
    assert_eq!((frame.key().width, frame.key().height), (2, 1));
    assert!(frame
        .pixels()
        .chunks_exact(4)
        .all(|p| p == [0, 0, 255, 255]));
    let mut forged = pages[0].clone();
    forged.sha256 = [0; 32];
    assert!(files
        .decode_current(&cache, &forged, 2, 2, &|| true)
        .is_err());
    for id in &reservation.ids {
        files.remove_retired(&ticket(*id)).unwrap();
    }
    assert!(!original.exists());
    assert!(files.files.entries(&files.originals, 4).unwrap().is_empty());
    assert!(files
        .files
        .entries(&files.quarantine, 4)
        .unwrap()
        .is_empty());
    cache.clear().unwrap();
    assert_eq!(
        cache.bytes_in_use(),
        8,
        "render frame still physically owns its pixels"
    );
    drop(frame);
    assert_eq!(cache.bytes_in_use(), 0);
}

#[test]
fn failed_later_image_keeps_owned_paths_retirable_and_does_not_publish_partial_proof() {
    let _serial = NATIVE_FILES_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let files = ControlledContentFiles::new(&fixture.root).unwrap();
    let input = ContentSources::open(
        LocalContentInput::Images(vec![
            fixture.input("first.bmp", &bmp(4, 2)),
            fixture.input("bad.bmp", b"not a bitmap"),
        ]),
        &mut SystemRandom,
    )
    .unwrap();
    let admission = admission(&input, ContentPayloadKind::Images);
    let reservation = reservation(&input, &admission);
    let mut preparation = files.preparation(Some(&reservation), &|| true);
    assert!(prepare_profile_content(&mut preparation, admission, input).is_err());
    assert!(files
        .original(reservation.ids[0], ImageFormat::Bmp)
        .exists());
    assert!(files.staging(reservation.ids[1]).exists());
    assert!(files.remove_retired(&ticket(reservation.ids[0])).is_err());
    drop(preparation);
    // The caller's registry remains charged through settle and retire. These
    // adapter-level tests only establish actual removal, never fake a DB finalize.
    for id in &reservation.ids {
        files.remove_retired(&ticket(*id)).unwrap();
    }
    assert!(files.files.entries(&files.originals, 4).unwrap().is_empty());
    assert!(files
        .files
        .entries(&files.quarantine, 4)
        .unwrap()
        .is_empty());
}

#[test]
fn mismatched_admission_and_reservation_fail_before_creating_any_owned_image() {
    let fixture = Fixture::new();
    let files = ControlledContentFiles::new(&fixture.root).unwrap();
    for wrong_reservation in [false, true] {
        let input = ContentSources::open(
            LocalContentInput::Images(vec![fixture.input(
                if wrong_reservation {
                    "two.bmp"
                } else {
                    "one.bmp"
                },
                &bmp(4, 2),
            )]),
            &mut SystemRandom,
        )
        .unwrap();
        let mut admission = admission(&input, ContentPayloadKind::Images);
        let mut reservation = reservation(&input, &admission);
        if wrong_reservation {
            reservation.bytes[0] += 1;
        } else {
            admission.request.payload_fingerprint[0] ^= 1;
        }
        let mut preparation = files.preparation(Some(&reservation), &|| true);
        assert!(prepare_profile_content(&mut preparation, admission, input).is_err());
        assert!(files.files.entries(&files.originals, 4).unwrap().is_empty());
        assert!(files
            .files
            .entries(&files.quarantine, 4)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn authority_update_pauses_actual_source_read_then_resumes_or_revokes_without_consuming_bytes() {
    use std::sync::{
        atomic::{AtomicBool, AtomicU8},
        mpsc,
    };
    let fixture = Fixture::new();
    let files = ControlledContentFiles::new(&fixture.root).unwrap();
    for revoke in [false, true] {
        let selected = fixture.input(
            if revoke { "revoke.txt" } else { "resume.txt" },
            b"actual source body",
        );
        let input =
            ContentSources::open(LocalContentInput::Txt(selected), &mut SystemRandom).unwrap();
        let mut observer = match &input.sources {
            Sources::Txt(source) => source.file().try_clone().unwrap(),
            _ => unreachable!(),
        };
        let admission = admission(&input, ContentPayloadKind::Text);
        let state = AtomicU8::new(0);
        let signalled = AtomicBool::new(false);
        let (entered, receiver) = mpsc::channel();
        std::thread::scope(|scope| {
            let gate = || {
                let current = state.load(Ordering::Acquire);
                if !signalled.swap(true, Ordering::AcqRel) {
                    entered.send(()).unwrap();
                }
                match current {
                    0 => PreparationState::AuthorityUpdating,
                    1 => PreparationState::Ready,
                    _ => PreparationState::Revoked,
                }
            };
            let (sender, completion) = mpsc::channel();
            let files = &files;
            let preparing = scope.spawn(move || {
                let mut preparation = files.preparation_while(None, &gate);
                let result = prepare_profile_content(&mut preparation, admission, input);
                sender
                    .send(result)
                    .unwrap_or_else(|_| panic!("test receiver disappeared"));
            });
            receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(
                observer.stream_position().unwrap(),
                0,
                "Updating cannot begin sensitive source reads"
            );
            assert!(matches!(
                completion.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            state.store(if revoke { 2 } else { 1 }, Ordering::Release);
            let result = completion.recv_timeout(Duration::from_secs(5)).unwrap();
            preparing.join().unwrap();
            if revoke {
                assert!(matches!(result, Err(Error::Cancelled)));
                assert_eq!(observer.stream_position().unwrap(), 0);
            } else {
                assert_eq!(
                    result.unwrap().payload(),
                    &ContentPayload::Text("actual source body".into())
                );
                assert_eq!(observer.stream_position().unwrap(), 18);
            }
        });
    }
}
