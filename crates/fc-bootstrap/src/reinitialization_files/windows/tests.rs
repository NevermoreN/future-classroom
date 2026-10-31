use super::*;
use fc_windows::InstanceGuard;
use std::{
    fs,
    os::windows::process::CommandExt,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(super) struct Fixture {
    pub paths: DataPaths,
    instance: Option<InstanceGuard>,
    outside: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        Self::with_min_root_units(0)
    }
    fn with_min_root_units(minimum: usize) -> Self {
        use std::os::windows::ffi::OsStrExt;
        let mut root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Reinitialization-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let current = root.as_os_str().encode_wide().count();
        if current < minimum {
            let mut name = root.file_name().unwrap().to_os_string();
            name.push("x".repeat(minimum - current));
            root.set_file_name(name);
        }
        let outside = root.with_extension("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside survives").unwrap();
        let paths = DataPaths::isolated_test(root).unwrap();
        paths
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        paths.prepare_instance_runtime().unwrap();
        let instance = Some(InstanceGuard::acquire(paths.root()).unwrap());
        let value = Self {
            paths,
            instance,
            outside,
        };
        let files = ControlledFiles::new(value.paths.root()).unwrap();
        for name in OWNED_TREES {
            files.directory(&[name.into()]).unwrap();
            value.write(
                &value.paths.root().join(name).join("own.bin"),
                b"old opaque bytes",
            );
        }
        value.write(&value.paths.database(), b"");
        value.write(
            &value
                .paths
                .root()
                .join("config")
                .join("credential-key.dpapi"),
            b"",
        );
        value.write(
            &value.paths.root().join("config").join("deployment.marker"),
            b"malformed marker",
        );
        value.write(
            &value.paths.runtime().join("reinitialization-probe.marker"),
            b"runtime survives",
        );
        value
    }
    pub fn files(&self) -> ReinitializationFiles {
        ReinitializationFiles::new(&self.paths).unwrap()
    }
    pub fn prepared(&self) -> PreparedReinitialization {
        let files = self.files();
        let review = files.review(42, &|| true).unwrap();
        files.prepare(review, &|| true).unwrap()
    }
    pub fn write(&self, path: &Path, bytes: &[u8]) {
        let files = ControlledFiles::new(self.paths.root()).unwrap();
        let mut file = files.create_new(path).unwrap();
        file.file_mut().write_all(bytes).unwrap();
        file.finish().unwrap();
    }
    pub fn assert_runtime(&self) {
        assert_eq!(
            fs::read(self.paths.runtime().join("reinitialization-probe.marker")).unwrap(),
            b"runtime survives"
        );
        assert!(InstanceGuard::acquire(self.paths.root()).is_err());
        assert_eq!(
            fs::read(self.outside.join("sentinel")).unwrap(),
            b"outside survives"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.instance.take();
        let root = fs::remove_dir_all(self.paths.root());
        let outside = fs::remove_dir_all(&self.outside);
        if !std::thread::panicking() {
            root.unwrap();
            outside.unwrap();
        }
    }
}
#[test]
fn review_and_prepare_allow_live_writable_files_without_reading_old_credentials() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let mut live = fs::OpenOptions::new()
        .write(true)
        .share_mode(3)
        .open(fixture.paths.database())
        .unwrap();
    let files = fixture.files();
    let review = files.review(42, &|| true).unwrap();
    assert_eq!(review.present_tree_count(), 13);
    let prepared = files.prepare(review, &|| true).unwrap();
    live.write_all(b"a legitimate final checkpoint").unwrap();
    drop(live);
    assert_eq!(prepared.sequence(), 42);
    assert_eq!(prepared.root(), fixture.paths.root());
    prepared.discard(&|| true).unwrap();
    fixture.assert_runtime();
    assert_eq!(
        fs::read(fixture.paths.database()).unwrap(),
        b"a legitimate final checkpoint"
    );
}
#[test]
fn unknown_root_entry_and_zero_sequence_refuse_without_mutation() {
    let fixture = Fixture::new();
    let files = fixture.files();
    assert!(files.review(0, &|| true).is_err());
    fs::create_dir(fixture.paths.root().join("unrelated")).unwrap();
    fixture.write(
        &fixture.paths.root().join("unrelated").join("keep"),
        b"keep",
    );
    assert!(matches!(
        files.review(1, &|| true),
        Err(ReinitializationFileError::RepairRequired)
    ));
    assert_eq!(
        files.reconcile_startup(&|| true).unwrap(),
        ResetStartup::default()
    );
    assert_eq!(
        fs::read(fixture.paths.root().join("unrelated").join("keep")).unwrap(),
        b"keep"
    );
    fixture.assert_runtime();
}
#[test]
fn zero_length_raw_objects_are_inspected_but_hardlinks_are_preserved_and_refused() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let original = fixture.outside.join("sentinel");
    let alias = fixture.paths.root().join("teachers").join("linked.bin");
    fs::hard_link(&original, &alias).unwrap();
    assert!(matches!(
        journal::arm_for_test(&prepared, &|| true),
        Err(ReinitializationFileError::RepairRequired)
    ));
    assert!(alias.exists());
    assert_eq!(fs::read(&original).unwrap(), b"outside survives");
    // Unarmed cleanup removes only its own empty candidate and metadata.
    prepared.discard(&|| true).unwrap();
    fixture.assert_runtime();
}
#[test]
fn owned_tree_junction_is_never_followed_or_deleted() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let junction = fixture.paths.root().join("teachers").join("junction");
    let result = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&fixture.outside)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(result.status.success(), "isolated directory junction setup");
    let armed = journal::arm_for_test(&prepared, &|| true);
    assert!(matches!(
        armed,
        Err(ReinitializationFileError::RepairRequired)
    ));
    fixture.assert_runtime();
    fs::remove_dir(&junction).unwrap();
    prepared.discard(&|| true).unwrap();
}
#[test]
fn canceled_preparation_is_explicitly_reconciled_without_touching_live() {
    let fixture = Fixture::new();
    let files = fixture.files();
    let review = files.review(42, &|| true).unwrap();
    let canceled = std::cell::Cell::new(false);
    let result = files.prepare(review, &|| {
        let namespace = fixture.paths.root().join(NAMESPACE);
        let owner_exists = fs::read_dir(&namespace).ok().is_some_and(|entries| {
            entries
                .filter_map(std::result::Result::ok)
                .any(|entry| entry.path().join("owner").exists())
        });
        if owner_exists {
            canceled.set(true);
            false
        } else {
            true
        }
    });
    assert!(canceled.get());
    assert!(matches!(result, Err(ReinitializationFileError::Cancelled)));
    files.reconcile_startup(&|| true).unwrap();
    assert_eq!(fs::read(fixture.paths.database()).unwrap(), b"");
    fixture.assert_runtime();
}
#[test]
fn expected_identity_rename_rejects_a_different_object_without_moving_it() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let source = prepared.operation.join("new").join("state");
    let target = prepared.operation.join("old").join("state");
    let pin = OwnedObjectPin::directory(&source).unwrap();
    let identity = pin.identity();
    drop(pin);
    assert!(prepared
        .service
        .files
        .rename_directory_expected(&source, &target, (identity.0, identity.1 ^ 1))
        .is_err());
    assert!(source.exists());
    assert!(!target.exists());
    prepared.discard(&|| true).unwrap();
    fixture.assert_runtime();
}

#[test]
fn normal_start_without_reset_namespace_does_not_claim_unknown_project_files() {
    let fixture = Fixture::new();
    fixture.write(
        &fixture.paths.root().join("recovery-probe-owner.marker"),
        b"ordinary recovery diagnostic owner",
    );
    let files = fixture.files();
    assert_eq!(
        files.reconcile_startup(&|| true).unwrap(),
        ResetStartup::default()
    );
    assert!(files.review(1, &|| true).is_err());
    assert_eq!(
        fs::read(fixture.paths.root().join("recovery-probe-owner.marker")).unwrap(),
        b"ordinary recovery diagnostic owner"
    );
    fixture.assert_runtime();
}
#[test]
fn pending_reinitialization_refuses_unknown_root_objects_before_cleanup() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let operation = prepared.operation.clone();
    fixture.write(
        &fixture.paths.root().join("unrelated.txt"),
        b"preserve unexpected root object",
    );
    drop(prepared);
    assert_eq!(
        fixture.files().reconcile_startup(&|| true),
        Err(ReinitializationFileError::RepairRequired)
    );
    assert!(operation.exists());
    assert_eq!(
        fs::read(fixture.paths.root().join("unrelated.txt")).unwrap(),
        b"preserve unexpected root object"
    );
    fixture.assert_runtime();
}

#[test]
fn owner_only_reset_namespace_does_not_apply_reset_scope_to_normal_startup() {
    let fixture = Fixture::new();
    let files = fixture.files();
    files.namespace(true).unwrap();
    fixture.write(
        &fixture.paths.root().join("ordinary-evidence.marker"),
        b"ordinary startup evidence",
    );
    assert_eq!(
        files.reconcile_startup(&|| true).unwrap(),
        ResetStartup::default()
    );
    assert!(files.review(1, &|| true).is_err());
    fixture.assert_runtime();
}

#[test]
fn reset_candidate_creation_and_acl_readback_cross_legacy_248_and_260_limits() {
    use std::os::windows::ffi::OsStrExt;
    let fixture = Fixture::with_min_root_units(200);
    let prepared = fixture.prepared();
    let child = prepared.operation.join("new").join("web-material-staging");
    assert!(child.as_os_str().encode_wide().count() > 260);
    // prepare verifies protected DACLs at creation/read-back for every fixed
    // child using the same Win7-compatible extended path, without long-path
    // policy changes or an application-manifest opt-in.
    assert!(child.is_dir());
    assert_eq!(
        prepared
            .service
            .files
            .entries(&prepared.operation.join("new"), 14)
            .unwrap()
            .len(),
        13
    );
    prepared.discard(&|| true).unwrap();
    assert_eq!(fs::read(fixture.paths.database()).unwrap(), b"");
    fixture.assert_runtime();
}
