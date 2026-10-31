use super::*;
use crate::{system::DataPaths, InstanceGuard};
use std::{
    fs::OpenOptions,
    io::Write,
    os::windows::process::CommandExt,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    instance: Option<InstanceGuard>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Backup-Copy-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut value = Self {
            root,
            instance: None,
        };
        DataPaths::isolated_test(value.root.clone())
            .unwrap()
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        value.instance = Some(InstanceGuard::acquire(&value.root).unwrap());
        value
    }
    fn source(&self, bytes: &[u8]) -> (LockedFile, [u8; 32]) {
        let path = self.root.join("exports").join(format!(
            "source-{}.fcbak",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let files = ControlledFiles::new(&self.root).unwrap();
        let mut file = files.create_new(&path).unwrap();
        file.file_mut().write_all(bytes).unwrap();
        let mut hash = Sha256::new().unwrap();
        hash.update(bytes).unwrap();
        (file.seal().unwrap(), hash.finish().unwrap())
    }
    fn destination(&self, name: &str) -> BackupDestination {
        BackupDestination::isolated_probe(&self.root, name).unwrap()
    }
    fn target(&self, name: &str) -> PathBuf {
        self.root.join("quarantine").join(name)
    }
    fn no_partial(&self) {
        for path in std::fs::read_dir(self.root.join("quarantine")).unwrap() {
            assert!(!path
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".future-classroom-backup-"));
        }
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
fn cng_sha256_matches_known_vector_and_copy_uses_actual_bytes() {
    let fixture = Fixture::new();
    let (source, hash) = fixture.source(b"abc");
    assert_eq!(
        hash,
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad
        ]
    );
    let receipt = fixture
        .destination("export.fcbak")
        .copy_from(source, 3, hash, || true)
        .unwrap();
    assert_eq!(receipt.bytes, 3);
    assert_eq!(receipt.sha256, hash);
    assert_eq!(
        std::fs::read(fixture.target("export.fcbak")).unwrap(),
        b"abc"
    );
    fixture.no_partial();
}
#[test]
fn copy_cancellation_and_late_cancel_delete_only_the_owned_inode() {
    let fixture = Fixture::new();
    let (source, hash) = fixture.source(&vec![9; 100_000]);
    let mut calls = 0;
    assert!(fixture
        .destination("cancel.fcbak")
        .copy_from(source, 100_000, hash, || {
            calls += 1;
            calls < 4
        })
        .is_err());
    assert!(!fixture.target("cancel.fcbak").exists());
    fixture.no_partial();
    let (source, hash) = fixture.source(b"abc");
    let target = fixture.target("late.fcbak");
    let mut published = false;
    assert!(fixture
        .destination("late.fcbak")
        .copy_from(source, 3, hash, || {
            if target.exists() {
                published = true;
                false
            } else {
                true
            }
        })
        .is_err());
    assert!(
        published,
        "the cancellation actually followed the handle rename"
    );
    assert!(!target.exists());
    fixture.no_partial();
}
#[test]
fn existing_and_racing_destinations_are_never_overwritten() {
    let fixture = Fixture::new();
    let target = fixture.target("existing.fcbak");
    std::fs::write(&target, b"user bytes").unwrap();
    let (source, hash) = fixture.source(b"abc");
    assert!(fixture
        .destination("existing.fcbak")
        .copy_from(source, 3, hash, || true)
        .is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"user bytes");
    let target = fixture.target("race.fcbak");
    let (source, hash) = fixture.source(b"abc");
    let mut calls = 0;
    assert!(fixture
        .destination("race.fcbak")
        .copy_from(source, 3, hash, || {
            calls += 1;
            if calls == 2 {
                std::fs::write(&target, b"racing user bytes").unwrap();
            }
            true
        })
        .is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"racing user bytes");
    fixture.no_partial();
}
#[test]
fn source_pin_denies_mutation_and_bad_hash_or_tail_has_no_published_output() {
    let fixture = Fixture::new();
    let (source, hash) = fixture.source(b"abc");
    assert!(OpenOptions::new().write(true).open(source.path()).is_err());
    assert!(fixture
        .destination("hash.fcbak")
        .copy_from(source, 3, [0; 32], || true)
        .is_err());
    let (source, _) = fixture.source(b"abcd");
    assert!(fixture
        .destination("tail.fcbak")
        .copy_from(source, 3, hash, || true)
        .is_err());
    let (source, hash) = fixture.source(b"abc");
    assert!(fixture
        .destination("zero.fcbak")
        .copy_from(source, 0, hash, || true)
        .is_err());
    let (source, hash) = fixture.source(b"abc");
    assert!(fixture
        .destination("large.fcbak")
        .copy_from(source, MAX_PACKAGE_BYTES + 1, hash, || true)
        .is_err());
    for name in ["hash.fcbak", "tail.fcbak", "zero.fcbak", "large.fcbak"] {
        assert!(!fixture.target(name).exists());
    }
    fixture.no_partial();
}
#[test]
fn destination_reparse_directory_is_rejected_before_any_copy() {
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let junction = fixture.root.join("quarantine").join("junction");
    // An isolated local directory junction needs no symlink privilege. There is
    // no desktop/input or association change; arguments are owned synthetic paths.
    let result = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(result.status.success(), "synthetic junction setup failed");
    let (source, hash) = fixture.source(b"abc");
    let result = BackupDestination::from_picker(junction.join("escaped.fcbak"))
        .unwrap()
        .copy_from(source, 3, hash, || true);
    std::fs::remove_dir(&junction).unwrap();
    assert!(result.is_err());
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    fixture.no_partial();
}
#[test]
fn diagnostic_destinations_require_the_isolated_marker_and_safe_backup_name() {
    assert!(
        BackupDestination::isolated_probe(Path::new("C:\\Users\\Teacher"), "test.fcbak").is_err()
    );
    assert!(BackupDestination::isolated_probe(
        Path::new("C:\\FutureClassroom-Test-copy"),
        "..\\test.fcbak"
    )
    .is_err());
}

#[test]
fn destination_constructor_rejects_undisplayable_complete_paths_before_io() {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    assert!(BackupDestination::from_picker(PathBuf::from("C:\\课堂😀\\备份.fcbak")).is_ok());
    for path in [
        "C:\\目录\u{202e}\\备份.fcbak",
        "C:\\目录\\备\u{2066}份.fcbak",
        "C:\\目录\n\\备份.fcbak",
    ] {
        assert!(BackupDestination::from_picker(PathBuf::from(path)).is_err());
    }
    let mut units: Vec<u16> = "C:\\".encode_utf16().collect();
    units.push(0xd800);
    units.extend("\\backup.fcbak".encode_utf16());
    let non_unicode = PathBuf::from(OsString::from_wide(&units));
    assert!(BackupDestination::from_picker(non_unicode).is_err());
    assert!(BackupDestination::from_picker(PathBuf::from(format!(
        "C:\\{}\\备份.fcbak",
        "a".repeat(32767)
    )))
    .is_err());
}
