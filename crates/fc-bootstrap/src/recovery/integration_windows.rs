//! Real protected deployment, sole writer, WIC originals and three-tree cold
//! replacement. No listener, Shell launch, Registry write or audio device starts.
use super::test_support::{password, Fixture, QuietFactory, QuietTask};
use super::ColdShutdown;
use crate::{
    backend::Backend,
    profile_content_files::{ContentSources, ControlledContentFiles, LocalContentInput},
    recovery_files::{entry_relative_path, RecoveryFiles},
    voice_runtime::VoiceRuntime,
};
use fc_application::identity::SessionLease;
use fc_domain::{
    auth::Role,
    desktop::ComponentKind,
    identity::{AssignmentScope, IdentityCommand, NewAccount, Rgb},
    ids::*,
    layout::*,
    materials::*,
    profile_content::*,
    recovery::BackupEntry,
    time::{LocalDate, LocalDateTime},
};
use fc_materials::office::{ContainerError, ValidationControl};
use fc_ports::{MaterialAppendMetadata, MaterialPreparation, MaterialRepositoryError};
use fc_runtime::worker::WorkerPool;
use fc_security::Password;
use fc_windows::{
    materials::{
        chooser::SelectedFile,
        files::{ControlledFiles, LockedFile},
        image,
    },
    system::DataPaths,
    SystemRandom,
};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn changed_password() -> Password {
    Password::new("Synthetic-current-R22-83491".into()).unwrap()
}
fn teacher_password() -> Password {
    Password::new("Synthetic-teacher-R22-71385".into()).unwrap()
}
fn catalog(f: &Fixture) -> MaterialScope {
    let mut backend = f.backend.lock().unwrap();
    backend
        .identity(
            f.lease,
            IdentityCommand::CreateClass {
                id: ClassId::new(1).unwrap(),
                name: "合成一班".into(),
            },
        )
        .unwrap();
    backend
        .identity(
            f.lease,
            IdentityCommand::PutSubject {
                id: SubjectId::new(1).unwrap(),
                name: "合成数学".into(),
                enabled: true,
                color: Rgb {
                    red: 70,
                    green: 110,
                    blue: 150,
                },
                expected_revision: None,
            },
        )
        .unwrap();
    backend
        .identity(
            f.lease,
            IdentityCommand::BindCurrentClass {
                id: ClassId::new(1).unwrap(),
            },
        )
        .unwrap();
    backend
        .create_account(
            f.lease,
            NewAccount {
                id: PrincipalId::new(2).unwrap(),
                login_name: "synthetic-teacher".into(),
                display_name: "合成教师".into(),
                role: Role::TeacherAdmin,
                scopes: vec![AssignmentScope {
                    class_id: ClassId::new(1).unwrap(),
                    subject_id: SubjectId::new(1).unwrap(),
                }],
            },
            teacher_password(),
        )
        .unwrap();
    MaterialScope {
        teacher_id: PrincipalId::new(2).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
    }
}
struct Check;
impl ValidationControl for Check {
    fn canceled(&self) -> bool {
        false
    }
    fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
        fc_windows::system::current_thread_cpu_time().map_err(|_| ContainerError::ClockUnavailable)
    }
}
/// The production preparation port is backed by real pinned/validated/flushed
/// files here. It does not manufacture a resource descriptor from arbitrary bytes.
struct TxtPreparation<'a> {
    files: &'a ControlledFiles,
    scope: MaterialScope,
    pin: Option<LockedFile>,
}
impl MaterialPreparation for TxtPreparation<'_> {
    type Input = SelectedFile;
    fn prepare(
        &mut self,
        selected: SelectedFile,
    ) -> Result<MaterialResource, MaterialRepositoryError> {
        fn unavailable<T>(_: T) -> MaterialRepositoryError {
            MaterialRepositoryError::Unavailable
        }
        let mut source = selected.open(MAX_MATERIAL_BYTES).map_err(unavailable)?;
        let report =
            fc_materials::validation::validate_file(source.file_mut(), "合成资料.txt", &Check)
                .map_err(|_| MaterialRepositoryError::Unavailable)?;
        source
            .file_mut()
            .seek(SeekFrom::Start(0))
            .map_err(unavailable)?;
        let directory = self
            .files
            .directory(&[
                "teachers".into(),
                self.scope.teacher_id.get().to_string(),
                "subjects".into(),
                self.scope.subject_id.get().to_string(),
                "500".into(),
            ])
            .map_err(unavailable)?;
        let stage = self
            .files
            .directory(&["quarantine".into()])
            .map_err(unavailable)?
            .join("material-501.part");
        let mut out = self.files.create_new(&stage).map_err(unavailable)?;
        fc_materials::copy_bounded(
            source.file_mut(),
            out.file_mut(),
            MAX_MATERIAL_BYTES,
            Duration::from_secs(30),
            || false,
        )
        .map_err(|_| MaterialRepositoryError::Unavailable)?;
        let sealed = out.seal().map_err(unavailable)?;
        self.pin = Some(
            sealed
                .publish(self.files, &directory.join("501.txt"))
                .map_err(unavailable)?,
        );
        Ok(MaterialResource {
            version_id: ResourceVersionId::new(501).unwrap(),
            sha256: report.sha256,
            kind: report.kind,
            bytes: report.bytes,
        })
    }
}
fn material(f: &Fixture, scope: MaterialScope) -> MaterialRecord {
    let files = ControlledFiles::new(&f.root).unwrap();
    let mut source = files
        .create_new(&f.root.join("quarantine").join("source.txt"))
        .unwrap();
    source
        .file_mut()
        .write_all("合成课堂资料：备份必须保留原件。\n".as_bytes())
        .unwrap();
    source.finish().unwrap();
    let mut preparation = TxtPreparation {
        files: &files,
        scope,
        pin: None,
    };
    let proof = fc_ports::prepare_material_append(
        &mut preparation,
        SelectedFile::isolated_probe(&f.root, "source.txt").unwrap(),
        MaterialAppendMetadata {
            command_id: CommandId::new(500).unwrap(),
            scope,
            material_id: MaterialId::new(500).unwrap(),
            display_name: "合成资料.txt".into(),
            allowed_class_ids: vec![ClassId::new(1).unwrap()],
        },
    )
    .unwrap();
    f.backend
        .lock()
        .unwrap()
        .append_material(f.lease, proof)
        .unwrap()
}
fn image_slot(f: &Fixture) -> ContentSlotKey {
    let mut backend = f.backend.lock().unwrap();
    let state = backend.read_public_layout(f.lease).unwrap();
    let mut layout = state.layout.clone();
    let id = layout.allocate_id().unwrap();
    let group = layout.allocate_id().unwrap();
    let outlet = layout
        .tree
        .nodes
        .iter_mut()
        .find(|node| matches!(node.kind, NodeKind::Outlet { .. }))
        .unwrap();
    let mut children = std::mem::replace(&mut outlet.children, vec![group]);
    children.push(id);
    layout.tree.nodes.push(LayoutNode {
        id: group,
        kind: NodeKind::Container(ContainerSettings {
            flow: Flow::Column,
            padding: 0,
            gap: 8,
            width: AxisSize::Fixed(300),
            height: AxisSize::Fixed(400),
            group_opacity_percent: 100,
            material: Some(GlassMaterial {
                surface_percent: 3,
                edge_percent: 50,
            }),
        }),
        children,
    });
    layout.tree.nodes.push(LayoutNode {
        id,
        kind: NodeKind::Widget(WidgetKind::SubjectImage),
        children: vec![],
    });
    let mut component = state.display.components[0].clone();
    component.kind = ComponentKind::SubjectImage;
    layout.content_styles.push(ContentNodeStyle {
        node_id: id,
        component,
        image_fit: ImageFit::Contain,
    });
    backend
        .save_public_layout(
            f.lease,
            state.layout.revision,
            state.display.revision,
            layout,
            state.display,
        )
        .unwrap();
    ContentSlotKey {
        scope: LayoutScope::Public,
        node_id: id,
    }
}
/// Small valid RGBA PNG, with real zlib stored block, Adler32 and PNG CRCs.
fn png(red: u8) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut crc = !0u32;
        for byte in kind.iter().chain(body) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
            }
        }
        out.extend_from_slice(&(!crc).to_be_bytes());
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    let pixels = [0, red, 70, 120, 255];
    let mut zlib = vec![0x78, 0x01, 0x01, 5, 0, 250, 255];
    zlib.extend_from_slice(&pixels);
    let (mut a, mut b) = (1u32, 0u32);
    for byte in pixels {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}
fn publish_image(
    backend: &Arc<Mutex<Backend>>,
    root: &Path,
    lease: SessionLease,
    slot: ContentSlotKey,
    command: u64,
    red: u8,
) -> ProfileContentDocument {
    let files = ControlledFiles::new(root).unwrap();
    let name = format!("source-{command}.png");
    let mut source = files
        .create_new(&root.join("quarantine").join(&name))
        .unwrap();
    source.file_mut().write_all(&png(red)).unwrap();
    source.finish().unwrap();
    let sources = ContentSources::open(
        LocalContentInput::Images(vec![SelectedFile::isolated_probe(root, &name).unwrap()]),
        &mut SystemRandom,
    )
    .unwrap();
    let operation = ContentOperation::Publish(ContentPayloadKind::Images);
    let current = backend
        .lock()
        .unwrap()
        .read_profile_content(lease, slot)
        .unwrap();
    let request = BeginContentRequest {
        command_id: CommandId::new(command).unwrap(),
        slot,
        expected_revision: current.revision,
        operation,
        payload_fingerprint: sources.intent_fingerprint(slot, operation).unwrap(),
        expires_at: LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 11 * 3600).unwrap(),
    };
    let admission = backend
        .lock()
        .unwrap()
        .begin_profile_content(lease, request)
        .unwrap();
    let reservation = backend
        .lock()
        .unwrap()
        .reserve_profile_media(lease, &admission, &sources.image_sizes().unwrap())
        .unwrap();
    let content_files = ControlledContentFiles::new(root).unwrap();
    {
        let guard = backend
            .lock()
            .unwrap()
            .profile_content_preparation_guard(lease)
            .unwrap();
        let check = || guard.is_context_valid();
        let mut preparation = content_files.preparation(Some(&reservation), &check);
        let prepared =
            fc_ports::prepare_profile_content(&mut preparation, admission, sources).unwrap();
        backend
            .lock()
            .unwrap()
            .commit_profile_content(lease, prepared)
            .unwrap();
    }
    backend
        .lock()
        .unwrap()
        .settle_profile_media(reservation)
        .unwrap();
    backend
        .lock()
        .unwrap()
        .read_profile_content(lease, slot)
        .unwrap()
}
fn media_paths(root: &Path, document: &ProfileContentDocument) -> Vec<PathBuf> {
    [&document.current, &document.previous]
        .into_iter()
        .flatten()
        .flat_map(|version| {
            let ContentPayload::Images(images) = &version.payload else {
                panic!("expected real image payload")
            };
            images
                .iter()
                .map(|reference| {
                    root.join(entry_relative_path(&BackupEntry::ProfileMedia {
                        version_id: reference.version_id,
                        format: reference.format,
                        bytes: reference.bytes,
                        sha256: reference.sha256,
                        width: reference.width,
                        height: reference.height,
                    }))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn complete_backup_cold_restore_preserves_current_security_txt_and_both_wic_image_versions() {
    let f = Fixture::new();
    let scope = catalog(&f);
    let record = material(&f, scope);
    let slot = image_slot(&f);
    publish_image(&f.backend, &f.root, f.lease, slot, 601, 50);
    let saved = publish_image(&f.backend, &f.root, f.lease, slot, 602, 180);
    assert!(saved.current.is_some() && saved.previous.is_some());
    let original_images: Vec<_> = media_paths(&f.root, &saved)
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect();
    let paths = DataPaths::isolated_test(f.root.clone()).unwrap();
    let files = RecoveryFiles::new(&paths).unwrap();
    let workspace = files.workspace().unwrap();
    let export_revision = f.revision();
    let access = f
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(f.lease, export_revision)
        .unwrap();
    let ordinary = workspace.ordinary_database();
    let receipt = access
        .export_database(ordinary.clone(), Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(receipt.media.len(), 3);
    let pinned = files.pin_export(&receipt, &ordinary, &|| true).unwrap();
    access.check_revision().unwrap();
    let exported = files.export_pinned(pinned, &|| true).unwrap();
    drop(workspace);
    drop(access);
    // These real post-backup changes must survive only where security requires.
    let later = publish_image(&f.backend, &f.root, f.lease, slot, 603, 240);
    assert_ne!(later.current, saved.current);
    {
        let mut backend = f.backend.lock().unwrap();
        backend
            .identity(
                f.lease,
                IdentityCommand::RenameSchool {
                    name: "Post-backup school".into(),
                },
            )
            .unwrap();
        backend
            .identity(
                f.lease,
                IdentityCommand::UpdateAccount {
                    id: scope.teacher_id,
                    display_name: "已停用合成教师".into(),
                    role: Role::User,
                    enabled: false,
                    scopes: vec![],
                },
            )
            .unwrap();
        backend
            .change_password(f.lease, password(), changed_password())
            .unwrap();
    }
    let lease = f
        .backend
        .lock()
        .unwrap()
        .login("admin", changed_password())
        .unwrap();
    let current_security = f.backend.lock().unwrap().state().unwrap();
    let autostart_before = f.backend.lock().unwrap().autostart_status(lease).unwrap();
    // Pin, stream/hash, decode, and inspect the REAL ordinary DB before review.
    let source = LockedFile::open(&exported.path, exported.bytes).unwrap();
    let prepared = files.preflight_pinned(source, &|| true).unwrap();
    let revision = f.revision();
    let access = f
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(lease, revision)
        .unwrap();
    let preview_path = prepared
        .ordinary_database()
        .with_file_name("preview.sqlite3");
    let preview = access
        .preflight_restore(
            prepared.ordinary_database().to_owned(),
            preview_path,
            prepared.manifest().clone(),
            prepared.manifest_sha256(),
            Arc::new(|| true),
        )
        .unwrap();
    assert_eq!(preview.current_revision(), revision);
    assert!(preview.document_count() > 0);
    let review = f
        .backend
        .lock()
        .unwrap()
        .review_preflighted_restore(lease, preview)
        .unwrap();
    let confirmed = review.confirm(Arc::new(|| true)).unwrap();
    assert!(f.backend.lock().unwrap().role(lease).is_err());
    let pool = WorkerPool::<QuietTask>::start().unwrap();
    let audio = VoiceRuntime::new(Arc::new(QuietFactory));
    let (mut drain, error) =
        ColdShutdown::begin_retained(confirmed, f.backend.clone(), pool, audio);
    assert_eq!(error, None);
    let until = Instant::now() + Duration::from_secs(30);
    let drained = loop {
        if let Some(drained) = drain.poll().unwrap() {
            break drained;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    };
    let permit = drained
        .prepare_database(prepared.candidate_database(), Arc::new(|| true))
        .unwrap();
    assert!(f.backend.lock().unwrap().state().is_err());
    let generation = permit.generation();
    let sealed = prepared
        .seal_candidate(permit.prepared_database(), &|| true)
        .unwrap();
    assert_eq!(
        sealed.commit_cold(permit, &|| true).unwrap().generation,
        generation
    );
    let startup = files.reconcile_startup(&|| true).unwrap();
    assert_eq!(startup.installed, 1);
    assert_eq!(startup.rolled_back, 0);
    assert_eq!(files.reconcile_startup(&|| true).unwrap().installed, 0);
    *f.backend.lock().unwrap() = f.reopen(&paths.database());
    let mut backend = f.backend.lock().unwrap();
    assert!(backend.login("admin", password()).is_err());
    let lease = backend.login("admin", changed_password()).unwrap();
    assert_eq!(backend.role(lease).unwrap(), Role::GlobalAdmin);
    assert!(backend.role(f.lease).is_err());
    let restored_security = backend.state().unwrap();
    assert_eq!(
        restored_security.installation.as_ref().unwrap().school_name,
        "Synthetic recovery school"
    );
    for old in &current_security.accounts {
        let new = restored_security.account(old.principal.id).unwrap();
        assert_eq!(new.credential_version, old.credential_version);
        assert_eq!(new.recovery_version, old.recovery_version);
        assert!(new.principal.auth_epoch > old.principal.auth_epoch);
    }
    assert!(
        !restored_security
            .account(scope.teacher_id)
            .unwrap()
            .principal
            .enabled
    );
    assert_eq!(
        restored_security
            .account(scope.teacher_id)
            .unwrap()
            .principal
            .role,
        Role::User
    );
    assert_eq!(
        restored_security
            .account(scope.teacher_id)
            .unwrap()
            .display_name,
        "合成教师"
    );
    assert!(!restored_security
        .assignments
        .iter()
        .any(|assignment| assignment.teacher_id == scope.teacher_id));
    assert_eq!(backend.autostart_status(lease).unwrap(), autostart_before);
    let voice = backend.voice_inventory().unwrap();
    assert!(voice.assets.is_empty());
    assert!(voice.jobs.iter().all(|j| j.state.terminal()));
    let restored = backend.read_profile_content(lease, slot).unwrap();
    assert_eq!(restored.current, saved.current);
    assert_eq!(restored.previous, saved.previous);
    assert!(restored.pending.is_none());
    let page = backend
        .materials(
            lease,
            fc_ports::MaterialListRequest {
                scope,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].resource, record.resource);
    drop(backend);
    let material_entry = BackupEntry::Material {
        teacher_id: scope.teacher_id,
        subject_id: scope.subject_id,
        material_id: record.id,
        version_id: record.resource.version_id,
        kind: record.resource.kind,
        bytes: record.resource.bytes,
        sha256: record.resource.sha256,
    };
    assert_eq!(
        fc_security::sha256(
            &std::fs::read(f.root.join(entry_relative_path(&material_entry))).unwrap()
        ),
        record.resource.sha256
    );
    for (path, expected) in media_paths(&f.root, &restored).iter().zip(original_images) {
        let mut source = LockedFile::open(path, MAX_CONTENT_IMAGE_BYTES).unwrap();
        let decoded = image::validate_image(&source, MaterialKind::Png, || false).unwrap();
        assert_eq!((decoded.width, decoded.height), (1, 1));
        source.file_mut().seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        source.file_mut().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, expected);
    }
    // The newer removed allocation remains retired after restoring an old pack.
    let next = publish_image(&f.backend, &f.root, lease, slot, 604, 10);
    let ContentPayload::Images(images) = &next.current.unwrap().payload else {
        panic!("image expected")
    };
    assert!(images[0].version_id.get() > 3);
    drop(files);
}
