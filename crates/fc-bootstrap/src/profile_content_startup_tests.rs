//! Protected Windows filesystem + actual Backend/SQLite/DPAPI integration.
//! Publication metadata proof is synthetic; this suite does NOT claim import
//! WIC validation. The separate content-files tests exercise real WIC prep.
use super::*;
use crate::backend::Initialize;
use fc_domain::{ids::*, layout::*, profile_content::*, time::*};
use fc_ports::{
    prepare_profile_content, Clock, MonotonicClock, ProfileContentPreparation,
    ProfileContentRepositoryError,
};
use fc_security::{CredentialRecordCodec, Password};
use fc_windows::{
    materials::files::ControlledFiles,
    system::{CurrentUserDpapi, DataPaths, SecretBytes},
    InstanceGuard,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const SECRET: &str = "Synthetic startup content password 2026!";
struct Time;
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 3600).unwrap()
    }
}
impl MonotonicClock for Time {
    fn monotonic_millis(&self) -> u64 {
        1000
    }
}
struct Fixture {
    backend: Option<Backend>,
    root: PathBuf,
    instance: Option<InstanceGuard>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Content-Startup-R18-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Own cleanup before protected deployment preparation can fail.
        let mut f = Self {
            backend: None,
            root,
            instance: None,
        };
        DataPaths::isolated_test(f.root.clone())
            .unwrap()
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        f.instance = Some(InstanceGuard::acquire(&f.root).unwrap());
        f.open();
        f
    }
    fn codec(&self) -> CredentialRecordCodec {
        let path = self.root.join("synthetic-key.dpapi");
        if !path.exists() {
            let codec = CredentialRecordCodec::from_key([43; 32]).unwrap();
            let protected = codec.protect_key(|key| {
                CurrentUserDpapi::default()
                    .protect(&SecretBytes::new(key.to_vec()).unwrap())
                    .unwrap()
            });
            std::fs::write(&path, protected).unwrap();
        }
        let protected = std::fs::read(path).unwrap();
        let clear = CurrentUserDpapi::default().unprotect(&protected).unwrap();
        CredentialRecordCodec::from_key(clear.expose().try_into().unwrap()).unwrap()
    }
    fn open(&mut self) {
        self.backend = Some(
            Backend::open(
                self.root.join("state.sqlite3"),
                self.codec(),
                Arc::new(Time),
                Arc::new(Time),
            )
            .unwrap(),
        );
    }
    fn b(&self) -> &Backend {
        self.backend.as_ref().unwrap()
    }
    fn bm(&mut self) -> &mut Backend {
        self.backend.as_mut().unwrap()
    }
    fn reopen(&mut self) {
        self.bm().close().unwrap();
        drop(self.backend.take());
        self.open();
    }
    fn write(&self, path: &Path, bytes: &[u8]) {
        let files = ControlledFiles::new(&self.root).unwrap();
        let mut staging = files.create_new(path).unwrap();
        staging.file_mut().write_all(bytes).unwrap();
        staging.finish().unwrap();
    }
    fn path(&self, id: MediaVersionId, location: ContentFileLocation) -> PathBuf {
        let (dir, ext) = match location {
            ContentFileLocation::Original(ImageFormat::Bmp) => ("originals", "bmp"),
            ContentFileLocation::Original(ImageFormat::Png) => ("originals", "png"),
            ContentFileLocation::Original(ImageFormat::Jpeg) => ("originals", "jpg"),
            ContentFileLocation::Quarantine => ("quarantine", "part"),
        };
        self.root
            .join("profile-content")
            .join(dir)
            .join(format!("{:020}.{ext}", id.get()))
    }
    fn setup(&mut self) -> (fc_application::identity::SessionLease, ContentSlotKey) {
        drop(
            self.bm()
                .initialize(
                    Initialize {
                        school_name: "Synthetic startup school".into(),
                        login_name: "admin".into(),
                        display_name: "Admin".into(),
                    },
                    Password::new(SECRET.into()).unwrap(),
                )
                .unwrap(),
        );
        let lease = self
            .bm()
            .login("admin", Password::new(SECRET.into()).unwrap())
            .unwrap();
        let state = self.b().read_public_layout(lease).unwrap();
        let mut layout = state.layout;
        let id = layout.allocate_id().unwrap();
        let outlet = layout
            .tree
            .nodes
            .iter_mut()
            .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
            .unwrap();
        let removed = std::mem::replace(&mut outlet.children, vec![id]);
        layout.tree.nodes.retain(|n| !removed.contains(&n.id));
        layout.tree.nodes.push(LayoutNode {
            id,
            kind: NodeKind::Widget(WidgetKind::SubjectImage),
            children: vec![],
        });
        layout
            .content_styles
            .push(ContentNodeStyle::new(id, ContentPayloadKind::Images));
        self.bm()
            .save_public_layout(
                lease,
                layout.revision,
                state.display.revision,
                layout,
                state.display,
            )
            .unwrap();
        (
            lease,
            ContentSlotKey {
                scope: LayoutScope::Public,
                node_id: id,
            },
        )
    }
    fn begin(
        &mut self,
        lease: fc_application::identity::SessionLease,
        slot: ContentSlotKey,
        command: u64,
        revision: u64,
    ) -> ContentAdmission {
        self.bm()
            .begin_profile_content(
                lease,
                BeginContentRequest {
                    command_id: CommandId::new(command).unwrap(),
                    slot,
                    expected_revision: Revision::new(revision),
                    operation: ContentOperation::Publish(ContentPayloadKind::Images),
                    payload_fingerprint: [command as u8; 32],
                    expires_at: LocalDateTime::new(Time.now().date, 7200).unwrap(),
                },
            )
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut b) = self.backend.take() {
            let closed = b.close();
            if !std::thread::panicking() {
                closed.unwrap();
            }
        }
        drop(self.instance.take());
        let cleaned = std::fs::remove_dir_all(&self.root);
        if !std::thread::panicking() {
            cleaned.unwrap();
        }
    }
}
fn bmp() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&58u32.to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&1i32.to_le_bytes());
    b.extend_from_slice(&1i32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&4u32.to_le_bytes());
    b.extend_from_slice(&[0; 16]);
    b.extend_from_slice(&[0, 0, 255, 0]);
    assert_eq!(b.len(), 58);
    b
}
struct SyntheticPrepared(ContentPayload);
impl ProfileContentPreparation for SyntheticPrepared {
    type Input = ();
    fn prepare(
        &mut self,
        _: &ContentAdmission,
        _: (),
    ) -> Result<ContentPayload, ProfileContentRepositoryError> {
        Ok(self.0.clone())
    }
    fn sha256(&self, b: &[u8]) -> Result<[u8; 32], ProfileContentRepositoryError> {
        Ok(fc_security::sha256(b))
    }
}
#[test]
fn startup_validates_everything_before_deletion_then_reconciles_reserved_orphans_removed_and_restart(
) {
    let mut f = Fixture::new();
    let files = ControlledContentFiles::new(&f.root).unwrap();
    let (lease, slot) = f.setup();
    let data = bmp();
    let ad = f.begin(lease, slot, 71_001, 0);
    let r = f
        .b()
        .reserve_profile_media(lease, &ad, &[data.len() as u64])
        .unwrap();
    let resource = MediaReference {
        version_id: r.ids[0],
        owner: slot,
        sha256: fc_security::sha256(&data),
        bytes: data.len() as u64,
        width: 1,
        height: 1,
        format: ImageFormat::Bmp,
    };
    let original = f.path(
        resource.version_id,
        ContentFileLocation::Original(ImageFormat::Bmp),
    );
    f.write(&original, &data);
    let prepared = prepare_profile_content(
        &mut SyntheticPrepared(ContentPayload::Images(vec![resource.clone()])),
        ad,
        (),
    )
    .unwrap();
    f.bm().commit_profile_content(lease, prepared).unwrap();
    f.b().settle_profile_media(r).unwrap();
    let ad = f.begin(lease, slot, 71_002, 1);
    let incomplete = f
        .b()
        .reserve_profile_media(lease, &ad, &[data.len() as u64, data.len() as u64])
        .unwrap();
    let partial = f.path(
        incomplete.ids[0],
        ContentFileLocation::Original(ImageFormat::Bmp),
    );
    let empty = f.path(incomplete.ids[1], ContentFileLocation::Quarantine);
    f.write(&partial, &data);
    f.write(&empty, &[]);
    let orphan = f.path(
        MediaVersionId::new(900).unwrap(),
        ContentFileLocation::Original(ImageFormat::Bmp),
    );
    let orphan_empty = f.path(
        MediaVersionId::new(901).unwrap(),
        ContentFileLocation::Quarantine,
    );
    f.write(&orphan, &data);
    f.write(&orphan_empty, &[]);
    let unknown = f
        .root
        .join("profile-content")
        .join("quarantine")
        .join("unexpected.tmp");
    f.write(&unknown, b"untrusted name");
    assert!(reconcile_after_restart(f.b(), &files).is_err());
    assert!(orphan.exists() && orphan_empty.exists() && partial.exists() && empty.exists());
    ControlledFiles::new(&f.root)
        .unwrap()
        .remove(&unknown)
        .unwrap();
    let mut corrupt = data.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    ControlledFiles::new(&f.root)
        .unwrap()
        .remove(&original)
        .unwrap();
    f.write(&original, &corrupt);
    assert!(reconcile_after_restart(f.b(), &files).is_err());
    assert!(orphan.exists() && partial.exists());
    assert_eq!(
        f.b().inventory_profile_media(None, 128).unwrap().items[1].state,
        MediaState::Reserved
    );
    ControlledFiles::new(&f.root)
        .unwrap()
        .remove(&original)
        .unwrap();
    f.write(&original, &data);
    let extra = f.path(
        resource.version_id,
        ContentFileLocation::Original(ImageFormat::Png),
    );
    f.write(&extra, &data);
    assert!(reconcile_after_restart(f.b(), &files).is_err());
    assert!(orphan.exists());
    ControlledFiles::new(&f.root)
        .unwrap()
        .remove(&extra)
        .unwrap();
    f.b().session_control().lock_local();
    let report = reconcile_after_restart(f.b(), &files).unwrap();
    assert_eq!(report.verified_originals, 1);
    assert_eq!(report.settled_reservations, 2);
    assert_eq!(report.finalized_resources, 2);
    assert_eq!(report.removed_orphans, 2);
    assert!(original.exists());
    assert!(!partial.exists() && !empty.exists() && !orphan.exists() && !orphan_empty.exists());
    let again = reconcile_after_restart(f.b(), &files).unwrap();
    assert_eq!(again.finalized_resources, 0);
    assert_eq!(again.removed_orphans, 0);
    f.write(&partial, &data);
    let removed_leftover = reconcile_after_restart(f.b(), &files).unwrap();
    assert_eq!(removed_leftover.removed_orphans, 1);
    assert!(!partial.exists());
    f.reopen();
    assert_eq!(
        reconcile_after_restart(f.b(), &files)
            .unwrap()
            .verified_originals,
        1
    );
}
#[test]
fn startup_uninitialized_owned_files_are_repair_not_new_registration_or_silent_cleanup() {
    let f = Fixture::new();
    let files = ControlledContentFiles::new(&f.root).unwrap();
    assert_eq!(
        reconcile_after_restart(f.b(), &files).unwrap(),
        StartupContentReport::default()
    );
    let path = f.path(
        MediaVersionId::new(1).unwrap(),
        ContentFileLocation::Quarantine,
    );
    f.write(&path, &[]);
    assert!(reconcile_after_restart(f.b(), &files).is_err());
    assert!(path.exists());
    assert!(f.b().state().unwrap().installation.is_none());
}
#[test]
fn owned_inventory_rejects_noncanonical_names_links_and_nonregular_before_deleting() {
    let f = Fixture::new();
    let files = ControlledContentFiles::new(&f.root).unwrap();
    let dir = f.root.join("profile-content").join("quarantine");
    for invalid in [
        "00000000000000000000.part",
        "00000000000000000001.PART",
        "18446744073709551616.part",
    ] {
        let path = dir.join(invalid);
        f.write(&path, b"x");
        assert!(files.owned_inventory().is_err());
        assert!(path.exists());
        ControlledFiles::new(&f.root)
            .unwrap()
            .remove(&path)
            .unwrap();
    }
    let bad = dir.join("1.part");
    f.write(&bad, b"x");
    assert!(files.owned_inventory().is_err());
    assert!(bad.exists());
    ControlledFiles::new(&f.root).unwrap().remove(&bad).unwrap();
    let stage = f.path(
        MediaVersionId::new(2).unwrap(),
        ContentFileLocation::Quarantine,
    );
    f.write(&stage, b"x");
    let linked = f.path(
        MediaVersionId::new(3).unwrap(),
        ContentFileLocation::Quarantine,
    );
    std::fs::hard_link(&stage, &linked).unwrap();
    assert!(files.owned_inventory().is_err());
    assert!(stage.exists() && linked.exists());
    std::fs::remove_file(&linked).unwrap();
    ControlledFiles::new(&f.root)
        .unwrap()
        .remove(&stage)
        .unwrap();
    let directory = f.path(
        MediaVersionId::new(4).unwrap(),
        ContentFileLocation::Quarantine,
    );
    std::fs::create_dir(&directory).unwrap();
    assert!(files.owned_inventory().is_err());
    std::fs::remove_dir(directory).unwrap();
    assert!(files.owned_inventory().unwrap().is_empty());
}

#[test]
fn startup_faulted_writer_cannot_enter_physical_recovery() {
    let f = Fixture::new();
    let files = ControlledContentFiles::new(&f.root).unwrap();
    let orphan = f.path(
        MediaVersionId::new(91).unwrap(),
        ContentFileLocation::Quarantine,
    );
    f.write(&orphan, b"must remain after authority fault");
    f.b().session_control().fault();
    assert_eq!(
        f.b().session_control().health_gate().status().health,
        fc_runtime::health::Health::Faulted
    );
    // SessionControl::fault irreversibly closes this Backend as well as the
    // authority gate; check_open therefore returns Closed before any SQL task.
    assert_eq!(
        reconcile_after_restart(f.b(), &files),
        Err(BackendError::Closed)
    );
    assert_eq!(files.owned_inventory().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(&orphan).unwrap(),
        b"must remain after authority fault"
    );
}
