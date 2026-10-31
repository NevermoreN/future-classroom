//! Synthetic production-port data for the hidden cross-installation probe.
//! Caller owns the isolated deployment, authenticated administrator and actual
//! worker lifetime. No arbitrary resource descriptor or credential row is used.
use super::{App, Backend, SessionLease};
use fc_bootstrap::{
    profile_content_files::{ContentSources, ControlledContentFiles, LocalContentInput},
    recovery_files::entry_relative_path,
};
use fc_domain::{
    auth::Role,
    content::{ContentCommand, ContentKey, HomeworkKey},
    identity::{AssignmentScope, DefaultTeacher, IdentityCommand, NewAccount, Rgb},
    ids::*,
    layout::{
        AxisSize, ContainerSettings, Flow, GlassMaterial, LayoutNode, LayoutScope, NodeKind,
        WidgetKind,
    },
    materials::*,
    profile::{SubjectProfileDocument, SubjectProfileKey},
    profile_content::*,
    recovery::BackupEntry,
    time::LocalDateTime,
};
use fc_materials::office::{ContainerError, ValidationControl};
use fc_ports::{
    MaterialAppendMetadata, MaterialListRequest, MaterialPreparation, MaterialRepositoryError,
};
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
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

type Check<T> = Result<T, String>;
pub(super) const SOURCE_TEACHER_LOGIN: &str = "migration-source-teacher";
pub(super) const TARGET_TEACHER_LOGIN: &str = "migration-target-teacher";
pub(super) const SOURCE_TEACHER_PASSWORD: &str = "Synthetic migration source teacher 2026!";
pub(super) const TARGET_TEACHER_PASSWORD: &str = "Synthetic migration target teacher 2026!";
pub(super) const TARGET_CLASS_NAME: &str = "目标保留十一班";
pub(super) const TARGET_EXTRA_CLASS_NAME: &str = "目标保留额外九十九班";
pub(super) const TARGET_SUBJECT_NAME: &str = "目标保留科目十二";
pub(super) const TARGET_TEACHER_NAME: &str = "目标保留教师二十二";
pub(super) const TARGET_SUBJECT_COLOR: Rgb = Rgb {
    red: 17,
    green: 91,
    blue: 203,
};
const MATERIAL_TEXT: &str = "合成迁移课堂资料：只迁移教学内容，保留目标账号和权限。\n";
const MATERIAL_NAME: &str = "合成迁移资料.txt";
const SUBJECT_TEXT: &str = "合成科目正文\n源科目独立内容映射到目标任教范围。";
const HOMEWORK_TEXT: &str = "合成迁移作业：完成目标班级和科目的引用复核。";
const TARGET_HOMEWORK_SENTINEL: &str = "合成目标旧教学哨兵，应由完整教学集合替换。";

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>, stage: &str) -> Check<T> {
    value.map_err(|error| format!("migration fixture: {stage}: {error:?}"))
}
fn require(value: bool, stage: &str) -> Check<()> {
    if value {
        Ok(())
    } else {
        Err(format!("migration fixture: {stage}"))
    }
}
fn handles(app: &App) -> Check<(Arc<Mutex<Backend>>, SessionLease)> {
    Ok((
        app.backend
            .as_ref()
            .ok_or("migration fixture: backend absent")?
            .clone(),
        app.lease
            .ok_or("migration fixture: administrator lease absent")?,
    ))
}
fn key(target: bool) -> SubjectProfileKey {
    SubjectProfileKey {
        class_id: ClassId::new(if target { 11 } else { 1 }).unwrap(),
        subject_id: SubjectId::new(if target { 12 } else { 1 }).unwrap(),
        teacher_id: PrincipalId::new(if target { 22 } else { 2 }).unwrap(),
    }
}
fn refresh(app: &mut App) -> Check<()> {
    let (backend, _) = handles(app)?;
    app.state = Arc::new(checked(
        checked(backend.lock(), "backend lock")?.state(),
        "fresh identity",
    )?);
    Ok(())
}
fn seed_catalog(app: &mut App, target: bool) -> Check<()> {
    let (backend, lease) = handles(app)?;
    let mut backend = checked(backend.lock(), "catalog lock")?;
    let k = key(target);
    checked(
        backend.identity(
            lease,
            IdentityCommand::CreateClass {
                id: k.class_id,
                name: if target {
                    TARGET_CLASS_NAME
                } else {
                    "源合成一班"
                }
                .into(),
            },
        ),
        "create class",
    )?;
    if target {
        checked(
            backend.identity(
                lease,
                IdentityCommand::CreateClass {
                    id: ClassId::new(99).unwrap(),
                    name: TARGET_EXTRA_CLASS_NAME.into(),
                },
            ),
            "create preserved extra class",
        )?;
    }
    checked(
        backend.identity(
            lease,
            IdentityCommand::PutSubject {
                id: k.subject_id,
                name: if target {
                    TARGET_SUBJECT_NAME
                } else {
                    "源合成科目一"
                }
                .into(),
                enabled: true,
                color: if target {
                    TARGET_SUBJECT_COLOR
                } else {
                    Rgb {
                        red: 170,
                        green: 42,
                        blue: 66,
                    }
                },
                expected_revision: None,
            },
        ),
        "create subject",
    )?;
    checked(
        backend.create_account(
            lease,
            NewAccount {
                id: k.teacher_id,
                login_name: if target {
                    TARGET_TEACHER_LOGIN
                } else {
                    SOURCE_TEACHER_LOGIN
                }
                .into(),
                display_name: if target {
                    TARGET_TEACHER_NAME
                } else {
                    "源合成教师二"
                }
                .into(),
                role: Role::TeacherAdmin,
                scopes: vec![AssignmentScope {
                    class_id: k.class_id,
                    subject_id: k.subject_id,
                }],
            },
            checked(
                Password::new(
                    if target {
                        TARGET_TEACHER_PASSWORD
                    } else {
                        SOURCE_TEACHER_PASSWORD
                    }
                    .into(),
                ),
                "synthetic teacher password",
            )?,
        ),
        "create teacher through credential port",
    )?;
    checked(
        backend.identity(
            lease,
            IdentityCommand::BindCurrentClass {
                id: if target {
                    ClassId::new(99).unwrap()
                } else {
                    k.class_id
                },
            },
        ),
        "bind current class",
    )?;
    checked(
        backend.identity(
            lease,
            IdentityCommand::SetDefaultTeacher(DefaultTeacher {
                class_id: k.class_id,
                subject_id: k.subject_id,
                teacher_id: k.teacher_id,
            }),
        ),
        "set default teacher",
    )?;
    Ok(())
}

struct Validation;
impl ValidationControl for Validation {
    fn canceled(&self) -> bool {
        false
    }
    fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
        fc_windows::system::current_thread_cpu_time().map_err(|_| ContainerError::ClockUnavailable)
    }
}
struct TxtPreparation<'a> {
    files: &'a ControlledFiles,
    scope: MaterialScope,
    material_id: MaterialId,
    version_id: ResourceVersionId,
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
            fc_materials::validation::validate_file(source.file_mut(), MATERIAL_NAME, &Validation)
                .map_err(unavailable)?;
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
                self.material_id.get().to_string(),
            ])
            .map_err(unavailable)?;
        let stage = self
            .files
            .directory(&["quarantine".into()])
            .map_err(unavailable)?
            .join(format!("migration-material-{}.part", self.version_id.get()));
        let mut output = self.files.create_new(&stage).map_err(unavailable)?;
        fc_materials::copy_bounded(
            source.file_mut(),
            output.file_mut(),
            MAX_MATERIAL_BYTES,
            Duration::from_secs(30),
            || false,
        )
        .map_err(unavailable)?;
        self.pin = Some(
            output
                .seal()
                .map_err(unavailable)?
                .publish(
                    self.files,
                    &directory.join(format!("{}.txt", self.version_id.get())),
                )
                .map_err(unavailable)?,
        );
        Ok(MaterialResource {
            version_id: self.version_id,
            sha256: report.sha256,
            kind: report.kind,
            bytes: report.bytes,
        })
    }
}
fn seed_material(app: &App, paths: &DataPaths, target: bool) -> Check<MaterialRecord> {
    let (backend, lease) = handles(app)?;
    let files = checked(
        ControlledFiles::new(paths.root()),
        "controlled material files",
    )?;
    let filename = if target {
        "migration-target-source.txt"
    } else {
        "migration-source.txt"
    };
    let mut input = checked(
        files.create_new(&paths.root().join("quarantine").join(filename)),
        "new synthetic TXT",
    )?;
    let text = if target {
        TARGET_HOMEWORK_SENTINEL
    } else {
        MATERIAL_TEXT
    };
    checked(
        input.file_mut().write_all(text.as_bytes()),
        "write synthetic TXT",
    )?;
    checked(input.finish(), "flush synthetic TXT")?;
    let k = key(target);
    let material_id = MaterialId::new(if target { 900 } else { 500 }).unwrap();
    let version_id = ResourceVersionId::new(if target { 901 } else { 501 }).unwrap();
    let scope = MaterialScope {
        teacher_id: k.teacher_id,
        subject_id: k.subject_id,
    };
    let mut preparation = TxtPreparation {
        files: &files,
        scope,
        material_id,
        version_id,
        pin: None,
    };
    let proof = checked(
        fc_ports::prepare_material_append(
            &mut preparation,
            checked(
                SelectedFile::isolated_probe(paths.root(), filename),
                "isolated TXT selection",
            )?,
            MaterialAppendMetadata {
                command_id: CommandId::new(if target { 900 } else { 500 }).unwrap(),
                scope,
                material_id,
                display_name: MATERIAL_NAME.into(),
                allowed_class_ids: vec![k.class_id],
            },
        ),
        "prepare validated material append",
    )?;
    let record = checked(
        checked(backend.lock(), "append material lock")?.append_material(lease, proof),
        "append prepared material",
    )?;
    Ok(record)
}
fn seed_homework(app: &App, target: bool) -> Check<()> {
    let (backend, lease) = handles(app)?;
    let k = key(target);
    let now = checked(app.clock.try_now(), "homework clock")?;
    checked(
        checked(backend.lock(), "homework lock")?.content(
            lease,
            ContentCommand {
                command_id: CommandId::new(if target { 950 } else { 550 }).unwrap(),
                key: ContentKey::Homework(HomeworkKey {
                    class_id: k.class_id,
                    actual_date: now.date,
                    subject_id: k.subject_id,
                    owner_teacher_id: k.teacher_id,
                }),
                expected_revision: Revision::default(),
                text: if target {
                    TARGET_HOMEWORK_SENTINEL
                } else {
                    HOMEWORK_TEXT
                }
                .into(),
            },
        ),
        "persist homework",
    )?;
    Ok(())
}
fn seed_profile(app: &App, record: &MaterialRecord) -> Check<(ContentSlotKey, ContentSlotKey)> {
    let (backend, lease) = handles(app)?;
    let k = key(false);
    let mut profile = SubjectProfileDocument::new(ProfileId::new(1).unwrap(), k);
    profile.folder_id = Some(record.folder_id);
    profile.materials.visible_rows = 3;
    profile.materials.row_gap_dip = 23;
    let image = checked(profile.allocate_id(), "allocate subject image node")?;
    let text = checked(profile.allocate_id(), "allocate subject text node")?;
    let group = checked(profile.allocate_id(), "allocate subject content group")?;
    let root_id = profile.tree.root;
    let root = checked(
        profile
            .tree
            .nodes
            .iter_mut()
            .find(|n| n.id == root_id)
            .ok_or("subject root absent"),
        "subject root",
    )?;
    // The public template already consumes its top-level glass allowance. The
    // selected subject replaces one fallback glass, so its three leaves must
    // share one real top-level group; each leaf remains at glass depth two.
    let mut children = std::mem::replace(&mut root.children, vec![group]);
    children.extend([image, text]);
    profile.tree.nodes.push(LayoutNode {
        id: group,
        kind: NodeKind::Container(ContainerSettings {
            flow: Flow::Column,
            padding: 0,
            gap: 8,
            width: AxisSize::Fixed(800),
            height: AxisSize::Fixed(800),
            group_opacity_percent: 100,
            material: Some(GlassMaterial::default()),
        }),
        children,
    });
    for (id, widget, payload) in [
        (image, WidgetKind::SubjectImage, ContentPayloadKind::Images),
        (text, WidgetKind::SubjectText, ContentPayloadKind::Text),
    ] {
        profile.tree.nodes.push(LayoutNode {
            id,
            kind: NodeKind::Widget(widget),
            children: vec![],
        });
        profile
            .content_styles
            .push(ContentNodeStyle::new(id, payload));
    }
    let mut backend = checked(backend.lock(), "subject profile lock")?;
    checked(profile.validate_structure(), "synthetic subject structure")?;
    let public = checked(
        backend.read_public_layout(lease),
        "current public composition",
    )?;
    checked(
        public.layout.tree.validate_composition(Some(&profile.tree)),
        "synthetic subject/public composition",
    )?;
    checked(
        backend.save_subject_profile(lease, k, Revision::default(), profile),
        "save subject-owned layout",
    )?;
    Ok((
        ContentSlotKey {
            scope: LayoutScope::Subject(k),
            node_id: image,
        },
        ContentSlotKey {
            scope: LayoutScope::Subject(k),
            node_id: text,
        },
    ))
}

/// Tiny valid RGBA PNG, including zlib stored block, Adler32 and PNG CRCs.
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
fn publish(
    app: &App,
    paths: &DataPaths,
    slot: ContentSlotKey,
    command: u64,
    input: LocalContentInput,
    kind: ContentPayloadKind,
) -> Check<()> {
    let (backend, lease) = handles(app)?;
    let sources = checked(
        ContentSources::open(input, &mut SystemRandom),
        "open real content sources",
    )?;
    let operation = ContentOperation::Publish(kind);
    let now = checked(app.clock.try_now(), "content clock")?;
    let expires_at = checked(
        LocalDateTime::new(now.date, (now.second_of_day() + 300).min(86_399)),
        "content expiration",
    )?;
    require(
        expires_at > now,
        "content publication crossed civil midnight",
    )?;
    let current = checked(
        checked(backend.lock(), "read content lock")?.read_profile_content(lease, slot),
        "read current content",
    )?;
    let admission = checked(
        checked(backend.lock(), "content admission lock")?.begin_profile_content(
            lease,
            BeginContentRequest {
                command_id: CommandId::new(command).unwrap(),
                slot,
                expected_revision: current.revision,
                operation,
                payload_fingerprint: checked(
                    sources.intent_fingerprint(slot, operation),
                    "content intent fingerprint",
                )?,
                expires_at,
            },
        ),
        "admit content publish",
    )?;
    let reservation = match sources.image_sizes().filter(|sizes| !sizes.is_empty()) {
        Some(sizes) => Some(checked(
            checked(backend.lock(), "reserve image lock")?
                .reserve_profile_media(lease, &admission, &sizes),
            "reserve real image IDs",
        )?),
        None => None,
    };
    let files = checked(
        ControlledContentFiles::new(paths.root()),
        "controlled content originals",
    )?;
    let guard = checked(
        checked(backend.lock(), "content guard lock")?.profile_content_preparation_guard(lease),
        "content preparation guard",
    )?;
    let check = || guard.is_context_valid();
    let mut preparation = files.preparation(reservation.as_ref(), &check);
    let result = (|| {
        let prepared = checked(
            fc_ports::prepare_profile_content(&mut preparation, admission, sources),
            "prepare WIC/text content",
        )?;
        checked(
            checked(backend.lock(), "content commit lock")?.commit_profile_content(lease, prepared),
            "commit prepared content",
        )?;
        Ok(())
    })();
    // Real preparation owns original-file pins. Release those owners before
    // settling the durable reservation, as in the production worker path.
    drop(preparation);
    if let Some(reservation) = reservation {
        checked(
            checked(backend.lock(), "settle content lock")?.settle_profile_media(reservation),
            "settle media reservation",
        )?;
    }
    result
}
fn publish_image(
    app: &App,
    paths: &DataPaths,
    slot: ContentSlotKey,
    command: u64,
    red: u8,
) -> Check<()> {
    let files = checked(ControlledFiles::new(paths.root()), "image input files")?;
    let name = format!("migration-source-{command}.png");
    let mut input = checked(
        files.create_new(&paths.root().join("quarantine").join(&name)),
        "new synthetic PNG",
    )?;
    checked(input.file_mut().write_all(&png(red)), "write synthetic PNG")?;
    checked(input.finish(), "flush synthetic PNG")?;
    publish(
        app,
        paths,
        slot,
        command,
        LocalContentInput::Images(vec![checked(
            SelectedFile::isolated_probe(paths.root(), &name),
            "isolated PNG selection",
        )?]),
        ContentPayloadKind::Images,
    )
}

pub(super) fn seed_source(app: &mut App, paths: &DataPaths) -> Check<()> {
    require(app.root == paths.root(), "source deployment mismatch")?;
    seed_catalog(app, false)?;
    let material = seed_material(app, paths, false)?;
    let (image, text) = seed_profile(app, &material)?;
    publish_image(app, paths, image, 601, 50)?;
    publish_image(app, paths, image, 602, 180)?;
    publish(
        app,
        paths,
        text,
        603,
        LocalContentInput::Text(SUBJECT_TEXT.into()),
        ContentPayloadKind::Text,
    )?;
    seed_homework(app, false)?;
    refresh(app)
}
pub(super) fn seed_target(app: &mut App, paths: &DataPaths) -> Check<()> {
    require(app.root == paths.root(), "target deployment mismatch")?;
    seed_catalog(app, true)?;
    seed_material(app, paths, true)?;
    seed_homework(app, true)?;
    refresh(app)
}

fn exact_file(path: &Path, expected: &[u8]) -> Check<LockedFile> {
    let mut pin = checked(
        LockedFile::open(path, expected.len() as u64),
        "open exact mapped original",
    )?;
    require(
        checked(pin.len(), "mapped original length")? == expected.len() as u64,
        "mapped original length differs",
    )?;
    let mut bytes = Vec::with_capacity(expected.len());
    checked(
        pin.file_mut().read_to_end(&mut bytes),
        "read mapped original handle",
    )?;
    require(
        bytes == expected && fc_security::sha256(&bytes) == fc_security::sha256(expected),
        "mapped original bytes/hash differs",
    )?;
    Ok(pin)
}
fn content_fingerprint(document: &ProfileContentDocument, version: &ContentVersion) -> Check<()> {
    let bytes = checked(
        version.payload.fingerprint_bytes(document.slot),
        "mapped payload canonical fingerprint",
    )?;
    require(
        version.digests.payload_sha256 == fc_security::sha256(&bytes),
        "mapped payload fingerprint differs",
    )?;
    require(
        version.last_actor_id == PrincipalId::new(1).unwrap(),
        "mapped content actor differs",
    )
}
pub(super) fn verify_mapped(app: &mut App, paths: &DataPaths) -> Check<()> {
    require(app.root == paths.root(), "mapped deployment mismatch")?;
    let (backend, lease) = handles(app)?;
    let backend = checked(backend.lock(), "mapped verification lock")?;
    let state = checked(backend.state(), "mapped current identity")?;
    let k = key(true);
    let teacher = checked(state.account(k.teacher_id), "retained target teacher")?;
    require(
        teacher.login_name == TARGET_TEACHER_LOGIN
            && teacher.display_name == TARGET_TEACHER_NAME
            && teacher.principal.role == Role::TeacherAdmin
            && teacher.principal.enabled
            && teacher.must_change_password
            && teacher.credential_version == 1
            && teacher.recovery_version == 1,
        "target teacher security/display fields changed",
    )?;
    require(
        state.account(PrincipalId::new(2).unwrap()).is_err()
            && state.account_by_login(SOURCE_TEACHER_LOGIN).is_err(),
        "source account was imported",
    )?;
    require(
        state.is_teacher(k.teacher_id, k.class_id, k.subject_id)
            && state
                .assignments
                .iter()
                .filter(|a| a.teacher_id == k.teacher_id)
                .count()
                == 1,
        "target exact teaching authority changed",
    )?;
    require(
        state
            .classes
            .iter()
            .any(|c| c.id == k.class_id && c.name == TARGET_CLASS_NAME && c.enabled)
            && state.classes.iter().any(|c| {
                c.id == ClassId::new(99).unwrap() && c.name == TARGET_EXTRA_CLASS_NAME && c.enabled
            }),
        "target class labels/extra catalog changed",
    )?;
    require(
        state.subjects.iter().any(|s| {
            s.id == k.subject_id
                && s.name == TARGET_SUBJECT_NAME
                && s.color == TARGET_SUBJECT_COLOR
                && s.enabled
        }),
        "target subject label/color changed",
    )?;
    require(
        state.classes.len() == 2 && state.subjects.len() == 1 && state.accounts.len() == 2,
        "source catalog identities were imported instead of mapped",
    )?;
    require(
        state.installation.as_ref().and_then(|i| i.current_class) == Some(k.class_id)
            && state.default_teachers
                == vec![DefaultTeacher {
                    class_id: k.class_id,
                    subject_id: k.subject_id,
                    teacher_id: k.teacher_id,
                }],
        "source current class/default teacher not explicitly mapped",
    )?;
    let scope = MaterialScope {
        teacher_id: k.teacher_id,
        subject_id: k.subject_id,
    };
    let page = checked(
        backend.materials(
            lease,
            MaterialListRequest {
                scope,
                cursor: None,
            },
        ),
        "mapped material catalog",
    )?;
    require(
        page.folder.scope == scope
            && page.items.len() == 1
            && page.total == 1
            && page.next_cursor.is_none(),
        "material full-set replacement differs",
    )?;
    let material = &page.items[0];
    require(
        material.id == MaterialId::new(500).unwrap()
            && material.resource.version_id == ResourceVersionId::new(501).unwrap()
            && material.display_name == MATERIAL_NAME
            && material.status == MaterialStatus::Published
            && material.allowed_class_ids == vec![k.class_id]
            && material.resource.kind == MaterialKind::Txt
            && material.resource.bytes == MATERIAL_TEXT.len() as u64
            && material.resource.sha256 == fc_security::sha256(MATERIAL_TEXT.as_bytes()),
        "mapped material metadata differs",
    )?;
    let path = paths
        .root()
        .join(entry_relative_path(&BackupEntry::Material {
            teacher_id: k.teacher_id,
            subject_id: k.subject_id,
            material_id: material.id,
            version_id: material.resource.version_id,
            kind: material.resource.kind,
            bytes: material.resource.bytes,
            sha256: material.resource.sha256,
        }));
    drop(exact_file(&path, MATERIAL_TEXT.as_bytes())?);
    require(
        !paths.root().join("teachers").join("2").exists()
            && !paths
                .root()
                .join("teachers")
                .join("22")
                .join("subjects")
                .join("12")
                .join("900")
                .exists(),
        "source or old target material directory survived",
    )?;
    let profile = checked(
        backend.read_subject_profile(lease, k),
        "mapped subject profile",
    )?
    .ok_or("migration fixture: subject profile absent")?;
    require(
        profile.key == k
            && profile.tree.scope == LayoutScope::Subject(k)
            && profile.enabled
            && profile.folder_id == Some(page.folder.id)
            && profile.materials.visible_rows == 3
            && profile.materials.row_gap_dip == 23,
        "subject scope/folder/style differs",
    )?;
    checked(
        profile.validate_active(&state, Some(&page.folder)),
        "mapped active subject references",
    )?;
    let node = |widget| {
        profile
            .tree
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Widget(widget))
            .map(|n| n.id)
            .ok_or("migration fixture: mapped content node absent")
    };
    let image_slot = ContentSlotKey {
        scope: LayoutScope::Subject(k),
        node_id: node(WidgetKind::SubjectImage)?,
    };
    let image_document = checked(
        backend.read_profile_content(lease, image_slot),
        "mapped image versions",
    )?;
    checked(
        image_document.validate_structure(),
        "mapped image document structure",
    )?;
    require(
        image_document.current.is_some()
            && image_document.previous.is_some()
            && image_document.pending.is_none(),
        "both committed image versions not retained",
    )?;
    let inventory = checked(
        backend.inventory_profile_media(None, 16),
        "mapped media owner inventory",
    )?;
    require(
        inventory.items.len() == 2 && inventory.next_after.is_none(),
        "mapped original media inventory differs",
    )?;
    for (version, red) in [
        (image_document.current.as_ref().unwrap(), 180),
        (image_document.previous.as_ref().unwrap(), 50),
    ] {
        content_fingerprint(&image_document, version)?;
        let ContentPayload::Images(images) = &version.payload else {
            return Err("migration fixture: image payload kind changed".into());
        };
        require(
            images.len() == 1 && version.digests.text_sha256.is_none(),
            "mapped image page count/digest differs",
        )?;
        let reference = &images[0];
        let bytes = png(red);
        require(
            reference.owner == image_slot
                && reference.format == ImageFormat::Png
                && reference.width == 1
                && reference.height == 1
                && reference.bytes == bytes.len() as u64
                && reference.sha256 == fc_security::sha256(&bytes),
            "mapped image owner/reference differs",
        )?;
        require(
            inventory.items.iter().any(|e| {
                e.id == reference.version_id
                    && e.owner == image_slot
                    && e.resource.as_ref() == Some(reference)
                    && e.refs > 0
            }),
            "mapped owner registry differs",
        )?;
        let path = paths
            .root()
            .join(entry_relative_path(&BackupEntry::ProfileMedia {
                version_id: reference.version_id,
                format: reference.format,
                bytes: reference.bytes,
                sha256: reference.sha256,
                width: reference.width,
                height: reference.height,
            }));
        let mut pin = exact_file(&path, &bytes)?;
        checked(
            pin.file_mut().seek(SeekFrom::Start(0)),
            "rewind WIC original",
        )?;
        let decoded = checked(
            image::validate_image(&pin, MaterialKind::Png, || false),
            "decode actual mapped PNG",
        )?;
        require(
            decoded.width == 1 && decoded.height == 1,
            "mapped WIC dimensions differ",
        )?;
    }
    let text_slot = ContentSlotKey {
        scope: LayoutScope::Subject(k),
        node_id: node(WidgetKind::SubjectText)?,
    };
    let text_document = checked(
        backend.read_profile_content(lease, text_slot),
        "mapped subject text",
    )?;
    let text = text_document
        .current
        .as_ref()
        .ok_or("migration fixture: subject text absent")?;
    content_fingerprint(&text_document, text)?;
    require(
        text.payload == ContentPayload::Text(SUBJECT_TEXT.into())
            && text.digests.text_sha256 == Some(fc_security::sha256(SUBJECT_TEXT.as_bytes())),
        "mapped subject text body/digest differs",
    )?;
    let now = checked(app.clock.try_now(), "mapped homework date")?;
    let homework = checked(
        backend.read_content(
            lease,
            ContentKey::Homework(HomeworkKey {
                class_id: k.class_id,
                actual_date: now.date,
                subject_id: k.subject_id,
                owner_teacher_id: k.teacher_id,
            }),
        ),
        "mapped homework",
    )?
    .ok_or("migration fixture: mapped homework absent")?;
    require(
        homework.text == HOMEWORK_TEXT && homework.last_actor_id == PrincipalId::new(1).unwrap(),
        "homework typed owner/body mapping differs",
    )?;
    drop(backend);
    refresh(app)
}
