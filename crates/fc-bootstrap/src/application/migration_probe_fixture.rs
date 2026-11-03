//! Synthetic production-port data for the hidden cross-installation probe.
//! Caller owns the isolated deployment, authenticated administrator and actual
//! worker lifetime. No arbitrary resource descriptor or credential row is used.
use super::{App, Backend, SessionLease};
use fc_application::identity::SessionPurpose;
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
    command_window::{CommandFamily, IssuedCommand},
    MaterialAppendMetadata, MaterialListRequest, MaterialPreparation, MaterialRepositoryError,
    ProfileContentStatus,
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
/// Issue on the real Writer, then restore the unscoped Backend after this
/// single database operation. Physical preparation must stay outside this call.
fn command<T>(
    backend: &mut Backend,
    lease: SessionLease,
    family: CommandFamily,
    action: impl FnOnce(&mut Backend) -> Check<T>,
) -> Check<T> {
    let issued = checked(
        backend.issue_native_command(family, Some(lease), SessionPurpose::Manage),
        "issue native command",
    )?;
    let mut scoped = checked(
        backend.begin_command_scope(issued),
        "check native command scope",
    )?;
    action(&mut scoped)
}
fn with_issued<T>(
    backend: &mut Backend,
    issued: IssuedCommand,
    action: impl FnOnce(&mut Backend) -> Check<T>,
) -> Check<T> {
    let mut scoped = checked(
        backend.begin_command_scope(issued),
        "check original command scope",
    )?;
    action(&mut scoped)
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
    command(&mut backend, lease, CommandFamily::Identity, |backend| {
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
        )
    })?;
    if target {
        command(&mut backend, lease, CommandFamily::Identity, |backend| {
            checked(
                backend.identity(
                    lease,
                    IdentityCommand::CreateClass {
                        id: ClassId::new(99).unwrap(),
                        name: TARGET_EXTRA_CLASS_NAME.into(),
                    },
                ),
                "create preserved extra class",
            )
        })?;
    }
    command(&mut backend, lease, CommandFamily::Identity, |backend| {
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
        )
    })?;
    command(&mut backend, lease, CommandFamily::Identity, |backend| {
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
        )
    })?;
    command(&mut backend, lease, CommandFamily::Identity, |backend| {
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
        )
    })?;
    command(&mut backend, lease, CommandFamily::Identity, |backend| {
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
        )
    })?;
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
    seed_material_using(app, paths, target, None)
}
fn seed_material_using(
    app: &App,
    paths: &DataPaths,
    target: bool,
    update_nonce: Option<&str>,
) -> Check<MaterialRecord> {
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
    let issued = checked(
        checked(backend.lock(), "issue material lock")?.issue_native_command(
            CommandFamily::Materials,
            Some(lease),
            SessionPurpose::Manage,
        ),
        "issue material append",
    )?;
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
                match update_nonce {
                    Some(nonce) => SelectedFile::update_probe(paths.root(), nonce, filename),
                    None => SelectedFile::isolated_probe(paths.root(), filename),
                },
                "owned synthetic TXT selection",
            )?,
            MaterialAppendMetadata {
                command_id: issued.command_id,
                scope,
                material_id,
                display_name: MATERIAL_NAME.into(),
                allowed_class_ids: vec![k.class_id],
            },
        ),
        "prepare validated material append",
    )?;
    let record = {
        let mut backend = checked(backend.lock(), "append material lock")?;
        with_issued(&mut backend, issued, |backend| {
            checked(
                backend.append_material(lease, proof),
                "append prepared material",
            )
        })?
    };
    Ok(record)
}
fn seed_homework(app: &App, target: bool) -> Check<()> {
    let (backend, lease) = handles(app)?;
    let k = key(target);
    let now = checked(app.clock.try_now(), "homework clock")?;
    let mut backend = checked(backend.lock(), "homework lock")?;
    command(&mut backend, lease, CommandFamily::Content, |backend| {
        checked(
            backend.content(
                lease,
                ContentCommand {
                    command_id: checked(backend.issued_command_id(), "issued homework command")?,
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
        )
    })?;
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
    command(&mut backend, lease, CommandFamily::Profiles, |backend| {
        checked(
            backend.save_subject_profile(lease, k, Revision::default(), profile),
            "save subject-owned layout",
        )
    })?;
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
pub(super) fn publish(
    app: &App,
    paths: &DataPaths,
    slot: ContentSlotKey,
    intent_label: u64,
    input: LocalContentInput,
    kind: ContentPayloadKind,
) -> Check<()> {
    publish_with_receipt(app, paths, slot, intent_label, input, kind).map(|_| ())
}
fn publish_with_receipt(
    app: &App,
    paths: &DataPaths,
    slot: ContentSlotKey,
    _intent_label: u64,
    input: LocalContentInput,
    kind: ContentPayloadKind,
) -> Check<IssuedCommand> {
    publish_with_receipt_expected(app, paths, slot, _intent_label, input, kind, None)
}
fn publish_with_receipt_expected(
    app: &App,
    paths: &DataPaths,
    slot: ContentSlotKey,
    _intent_label: u64,
    input: LocalContentInput,
    kind: ContentPayloadKind,
    expected_revision: Option<Revision>,
) -> Check<IssuedCommand> {
    // The fixture's 601/602/603 labels name synthetic input files only. They
    // never supply the permission-bearing command id.
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
    // Verification writes bind the manifest's original expected revision.
    // Only older seed callers derive their CAS from an explicit current read.
    let expected_revision = match expected_revision {
        Some(expected) => expected,
        None => {
            checked(
                checked(backend.lock(), "read content lock")?.read_profile_content(lease, slot),
                "read current content",
            )?
            .revision
        }
    };
    let fingerprint = checked(
        sources.intent_fingerprint(slot, operation),
        "content intent fingerprint",
    )?;
    let (issued, admission, owner) = {
        let mut backend = checked(backend.lock(), "content admission lock")?;
        let issued = checked(
            backend.issue_native_command(
                CommandFamily::ProfileContent,
                Some(lease),
                SessionPurpose::Manage,
            ),
            "issue content publish",
        )?;
        let (admission, owner) = with_issued(&mut backend, issued, |backend| {
            let command_id = checked(backend.issued_command_id(), "issued content command")?;
            checked(
                backend.begin_profile_content_owned(
                    lease,
                    BeginContentRequest {
                        command_id,
                        slot,
                        expected_revision,
                        operation,
                        payload_fingerprint: fingerprint,
                        expires_at,
                    },
                ),
                "admit content publish",
            )
        })?;
        (issued, admission, owner)
    };
    let reserved: Check<_> = (|| match sources.image_sizes().filter(|sizes| !sizes.is_empty()) {
        Some(sizes) => {
            let mut backend = checked(backend.lock(), "reserve image lock")?;
            with_issued(&mut backend, issued, |backend| {
                checked(
                    backend.reserve_profile_media(lease, &admission, &sizes),
                    "reserve real image IDs",
                )
            })
            .map(Some)
        }
        None => Ok(None),
    })();
    let reservation = match reserved {
        Ok(reservation) => reservation,
        Err(error) => {
            // Sources must physically return before the original owner marks
            // Begin failed. This negative receipt does not recheck client TTL.
            drop(sources);
            checked(
                checked(backend.lock(), "failed reserve lock")?.fail_profile_content_owned(owner),
                "return failed content owner",
            )?;
            return Err(error);
        }
    };
    let result: Check<()> = (|| {
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
        let prepared = checked(
            fc_ports::prepare_profile_content(&mut preparation, admission, sources),
            "prepare WIC/text content",
        )?;
        let mut backend = checked(backend.lock(), "content commit lock")?;
        with_issued(&mut backend, issued, |backend| {
            checked(
                backend.commit_profile_content(lease, prepared),
                "commit prepared content",
            )
        })?;
        Ok(())
    })(); // All source/preparation pins have actually returned, including Err.
    let settled = if let Some(reservation) = reservation {
        checked(
            checked(backend.lock(), "settle content lock")?.settle_profile_media(reservation),
            "settle media reservation",
        )
    } else {
        Ok(())
    };
    if result.is_err() {
        // The original owned Begin, never a replacement command/scope, closes
        // the failed flow after its physical producers have returned.
        checked(
            checked(backend.lock(), "failed content lock")?.fail_profile_content_owned(owner),
            "return failed content owner",
        )?;
    }
    settled?;
    result?;
    // Query the result using the original pair; a newly issued command would
    // change the flow being observed and cannot prove this publication.
    let mut backend = checked(backend.lock(), "content outcome lock")?;
    let outcome = with_issued(&mut backend, issued, |backend| {
        let command_id = checked(
            backend.issued_command_id(),
            "issued content outcome command",
        )?;
        checked(
            backend.profile_content_outcome(lease, command_id),
            "query original content outcome",
        )
    })?;
    require(
        outcome.is_some_and(|outcome| outcome.status == ProfileContentStatus::Committed),
        "original content command committed",
    )?;
    Ok(issued)
}

pub(super) fn publish_image(
    app: &App,
    paths: &DataPaths,
    slot: ContentSlotKey,
    command: u64,
    red: u8,
) -> Check<()> {
    publish_image_using(app, paths, slot, command, red, None).map(|_| ())
}
fn publish_image_using(
    app: &App,
    paths: &DataPaths,
    slot: ContentSlotKey,
    command: u64,
    red: u8,
    update_nonce: Option<&str>,
) -> Check<IssuedCommand> {
    let files = checked(ControlledFiles::new(paths.root()), "image input files")?;
    let name = format!("migration-source-{command}.png");
    let mut input = checked(
        files.create_new(&paths.root().join("quarantine").join(&name)),
        "new synthetic PNG",
    )?;
    checked(input.file_mut().write_all(&png(red)), "write synthetic PNG")?;
    checked(input.finish(), "flush synthetic PNG")?;
    publish_with_receipt(
        app,
        paths,
        slot,
        command,
        LocalContentInput::Images(vec![checked(
            match update_nonce {
                Some(nonce) => SelectedFile::update_probe(paths.root(), nonce, &name),
                None => SelectedFile::isolated_probe(paths.root(), &name),
            },
            "owned synthetic PNG selection",
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

// Upgrade preservation uses the actual default deployment, not a backup import
// or a second data root. Only public fixture IDs, hashes and original C2 pairs
// enter this bounded, immutable manifest chain. Passwords/recovery records do not.
const UPDATE_SECOND_TEXT: &str = "软件更新保留的第二份合成原件。\n";
pub(super) const UPDATE_TEACHER_PASSWORD: &str = "Synthetic update changed teacher 2026!";
const UPDATE_HASH_FIELDS: &[&str] = &[
    "identity",
    "display",
    "schedule",
    "public_layout",
    "profile",
    "materials",
    "images",
    "text",
    "homework",
    "key_envelope",
    "file_objects",
];
const UPDATE_FIELDS: &[&str] = &[
    "nonce",
    "sequence",
    "previous_sha256",
    "date",
    "owner_epoch",
    "default_ticket",
    "default_command",
    "default_expected",
    "default_result",
    "image_ticket",
    "image_command",
    "image_result",
    "materials_ticket",
    "materials_command",
    "materials_result",
    "verification_revision",
    "verification_hash",
    "identity",
    "display",
    "schedule",
    "public_layout",
    "profile",
    "materials",
    "images",
    "text",
    "homework",
    "key_envelope",
    "file_objects",
];
#[derive(Clone)]
pub(super) struct UpdateManifest(std::collections::BTreeMap<String, String>);
fn update_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn update_digest(value: &impl std::fmt::Debug) -> String {
    update_hex(&fc_security::sha256(format!("{value:?}").as_bytes()))
}
fn update_date(date: fc_domain::time::LocalDate) -> String {
    format!("{:04}{:02}{:02}", date.year(), date.month(), date.day())
}
fn update_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl UpdateManifest {
    pub(super) fn value(&self, name: &str) -> Check<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("update fixture: missing {name}"))
    }
    fn set(&mut self, name: &str, value: impl ToString) {
        self.0.insert(name.into(), value.to_string());
    }
    fn number(&self, name: &str) -> Check<u64> {
        let value = self.value(name)?;
        require(
            !value.is_empty()
                && value.bytes().all(|b| b.is_ascii_digit())
                && (value == "0" || !value.starts_with('0')),
            "canonical manifest number",
        )?;
        value
            .parse()
            .map_err(|_| "update fixture: invalid manifest number".into())
    }
    pub(super) fn sequence(&self) -> Check<u64> {
        self.number("sequence")
    }
    fn date(&self) -> Check<fc_domain::time::LocalDate> {
        let d = self.value("date")?;
        require(
            d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()),
            "manifest date shape",
        )?;
        checked(
            fc_domain::time::LocalDate::new(
                d[..4].parse().map_err(|_| "update fixture: date year")?,
                d[4..6].parse().map_err(|_| "update fixture: date month")?,
                d[6..].parse().map_err(|_| "update fixture: date day")?,
            ),
            "manifest civil date",
        )
    }
    fn issued(&self, label: &str, family: CommandFamily) -> Check<IssuedCommand> {
        use fc_ports::command_window::{CommandOwner, CommandTicket, COMMAND_WINDOW_MILLIS};
        Ok(IssuedCommand {
            ticket: checked(
                CommandTicket::parse(self.value(&format!("{label}_ticket"))?),
                "original C2 ticket",
            )?,
            command_id: checked(
                CommandId::new(self.number(&format!("{label}_command"))?),
                "original payload ID",
            )?,
            family,
            owner: CommandOwner::Principal {
                principal: PrincipalId::new(1).unwrap(),
                school: SchoolId::new(1).unwrap(),
                epoch: AuthEpoch::new(self.number("owner_epoch")?),
                origin: fc_domain::auth::TrustedOrigin::Local,
            },
            window_millis: COMMAND_WINDOW_MILLIS,
        })
    }
    fn record_issued(&mut self, name: &str, issued: IssuedCommand) -> Check<()> {
        use fc_ports::command_window::CommandOwner;
        let CommandOwner::Principal {
            principal,
            school,
            epoch,
            origin: fc_domain::auth::TrustedOrigin::Local,
        } = issued.owner
        else {
            return Err("update fixture: expected original local principal".into());
        };
        require(
            principal.get() == 1 && school.get() == 1,
            "synthetic original command owner",
        )?;
        if let Some(previous) = self.0.get("owner_epoch") {
            require(
                previous == &epoch.get().to_string(),
                "consistent original owner epoch",
            )?;
        }
        self.set("owner_epoch", epoch.get());
        self.set(&format!("{name}_ticket"), issued.ticket);
        self.set(&format!("{name}_command"), issued.command_id.get());
        Ok(())
    }
    pub(super) fn set_predecessor(&mut self, bytes: &[u8]) {
        self.set("previous_sha256", update_hex(&fc_security::sha256(bytes)));
    }
    pub(super) fn encode(&self) -> String {
        let mut text = String::from("FC_UPDATE_DATA_V1\n");
        for (key, value) in &self.0 {
            text.push_str(&format!("{key}={value}\n"));
        }
        text
    }
    pub(super) fn parse(text: &str, nonce: &str) -> Check<Self> {
        require(
            text.len() <= 8192 && text.ends_with('\n'),
            "bounded complete public manifest",
        )?;
        let mut lines = text.lines();
        require(lines.next() == Some("FC_UPDATE_DATA_V1"), "manifest schema")?;
        let mut values = std::collections::BTreeMap::new();
        for line in lines {
            let (name, value) = line
                .split_once('=')
                .ok_or("update fixture: malformed manifest line")?;
            require(
                UPDATE_FIELDS.contains(&name)
                    && !value.is_empty()
                    && value.len() <= 80
                    && value.bytes().all(|b| b.is_ascii_graphic())
                    && values.insert(name.into(), value.into()).is_none(),
                "closed unique public fields",
            )?;
        }
        let manifest = Self(values);
        require(
            manifest.0.len() == UPDATE_FIELDS.len() && manifest.value("nonce")? == nonce,
            "complete nonce-owned manifest",
        )?;
        require(manifest.sequence()? <= 16, "bounded verification sequence")?;
        manifest.date()?;
        for field in UPDATE_HASH_FIELDS.iter().copied().chain([
            "previous_sha256",
            "materials_result",
            "verification_hash",
        ]) {
            require(
                update_hash(manifest.value(field)?),
                "manifest SHA-256 shape",
            )?;
        }
        for field in [
            "owner_epoch",
            "default_expected",
            "default_result",
            "image_result",
            "verification_revision",
        ] {
            manifest.number(field)?;
        }
        manifest.issued("default", CommandFamily::Identity)?;
        manifest.issued("image", CommandFamily::ProfileContent)?;
        manifest.issued("materials", CommandFamily::Materials)?;
        require(
            manifest.number("default_result")?
                == manifest
                    .number("default_expected")?
                    .checked_add(1)
                    .ok_or("update fixture: revision overflow")?
                && manifest.number("image_result")? == 1
                && manifest.number("verification_revision")? == manifest.sequence()?,
            "manifest original and subsequent revision rules",
        )?;
        Ok(manifest)
    }
    pub(super) fn check_successor(&self, previous: &Self, previous_bytes: &[u8]) -> Check<()> {
        require(
            self.sequence()? == previous.sequence()? + 1
                && self.value("previous_sha256")?
                    == update_hex(&fc_security::sha256(previous_bytes)),
            "manifest predecessor hash and sequence",
        )?;
        for field in UPDATE_FIELDS {
            if !matches!(
                *field,
                "sequence" | "previous_sha256" | "verification_revision" | "verification_hash"
            ) {
                require(
                    self.value(field)? == previous.value(field)?,
                    "immutable original preservation manifest",
                )?;
            }
        }
        Ok(())
    }
}

fn seed_update_second(app: &App, paths: &DataPaths, nonce: &str) -> Check<(IssuedCommand, String)> {
    let (backend, lease) = handles(app)?;
    let files = checked(
        ControlledFiles::new(paths.root()),
        "update controlled originals",
    )?;
    let mut input = checked(
        files.create_new(&paths.root().join("quarantine/update-second.txt")),
        "fresh second TXT source",
    )?;
    checked(
        input.file_mut().write_all(UPDATE_SECOND_TEXT.as_bytes()),
        "write second synthetic original",
    )?;
    checked(input.finish(), "flush second source")?;
    let scope = MaterialScope {
        teacher_id: key(false).teacher_id,
        subject_id: key(false).subject_id,
    };
    let issued = checked(
        checked(backend.lock(), "update material issue lock")?.issue_native_command(
            CommandFamily::Materials,
            Some(lease),
            SessionPurpose::Manage,
        ),
        "issue original material batch",
    )?;
    let mut preparation = TxtPreparation {
        files: &files,
        scope,
        material_id: MaterialId::new(400).unwrap(),
        version_id: ResourceVersionId::new(401).unwrap(),
        pin: None,
    };
    let proof = checked(
        fc_ports::prepare_material_append(
            &mut preparation,
            checked(
                SelectedFile::update_probe(paths.root(), nonce, "update-second.txt"),
                "owned second original selection",
            )?,
            MaterialAppendMetadata {
                command_id: issued.command_id,
                scope,
                material_id: MaterialId::new(400).unwrap(),
                display_name: "软件更新第二份资料.txt".into(),
                allowed_class_ids: vec![key(false).class_id],
            },
        ),
        "validate second original",
    )?;
    let batch = checked(
        fc_ports::prepare_material_batch_append(vec![proof]),
        "prepare original atomic batch",
    )?;
    let mut backend = checked(backend.lock(), "update batch commit lock")?;
    with_issued(&mut backend, issued, |b| {
        checked(
            b.append_material_batch(lease, batch),
            "commit original material batch",
        )
    })?;
    let outcome = checked(
        backend.material_batch_outcome(lease, scope, issued),
        "read original material batch receipt",
    )?
    .ok_or("update fixture: original batch receipt absent")?;
    Ok((issued, update_digest(&outcome)))
}

pub(super) fn seed_update_data(
    app: &mut App,
    paths: &DataPaths,
    nonce: &str,
) -> Check<UpdateManifest> {
    require(
        app.root == paths.root()
            && paths.root() == checked(DataPaths::program_data(), "default data root")?.root(),
        "actual default deployment only",
    )?;
    let mut manifest = UpdateManifest(std::collections::BTreeMap::new());
    manifest.set("nonce", nonce);
    manifest.set("sequence", 0);
    manifest.set("previous_sha256", "0".repeat(64));
    manifest.set("verification_revision", 0);
    manifest.set("verification_hash", update_hex(&fc_security::sha256(b"")));
    manifest.set(
        "date",
        update_date(checked(app.clock.try_now(), "seed civil date")?.date),
    );
    for field in UPDATE_HASH_FIELDS {
        manifest.set(field, "0".repeat(64));
    }
    seed_catalog(app, false)?;
    let material = seed_material_using(app, paths, false, Some(nonce))?;
    let (material_issued, material_digest) = seed_update_second(app, paths, nonce)?;
    manifest.record_issued("materials", material_issued)?;
    manifest.set("materials_result", material_digest);
    let (image_slot, text_slot) = seed_profile(app, &material)?;
    let image_issued = publish_image_using(app, paths, image_slot, 601, 50, Some(nonce))?;
    manifest.record_issued("image", image_issued)?;
    manifest.set("image_result", 1);
    publish_image_using(app, paths, image_slot, 602, 180, Some(nonce))?;
    publish(
        app,
        paths,
        text_slot,
        603,
        LocalContentInput::Text(SUBJECT_TEXT.into()),
        ContentPayloadKind::Text,
    )?;
    seed_homework(app, false)?;
    seed_update_settings(app)?;
    // Publish a distinct later identity revision, so only the original read
    // receipt can prove the earlier default-teacher result after reopening.
    let (backend, lease) = handles(app)?;
    let mut backend = checked(backend.lock(), "update default receipt lock")?;
    let expected = checked(backend.state(), "update expected identity")?.revision;
    let issued = checked(
        backend.issue_native_command(CommandFamily::Identity, Some(lease), SessionPurpose::Manage),
        "issue original default teacher",
    )?;
    let state = with_issued(&mut backend, issued, |b| {
        checked(
            b.identity_at(
                lease,
                expected,
                IdentityCommand::SetDefaultTeacher(DefaultTeacher {
                    class_id: key(false).class_id,
                    subject_id: key(false).subject_id,
                    teacher_id: key(false).teacher_id,
                }),
            ),
            "save original default teacher",
        )
    })?;
    manifest.record_issued("default", issued)?;
    manifest.set("default_expected", expected.get());
    manifest.set("default_result", state.revision.get());
    command(&mut backend, lease, CommandFamily::Identity, |b| {
        checked(
            b.identity(
                lease,
                IdentityCommand::RenameSchool {
                    name: "软件更新合成学校（保留验证）".into(),
                },
            ),
            "later distinct identity revision",
        )
    })?;
    drop(backend);
    refresh(app)?;
    Ok(manifest)
}

fn seed_update_settings(app: &App) -> Check<()> {
    use fc_domain::{
        desktop::ClockMode,
        schedule::*,
        schedule_document::{CellDetails, RowDetails, SemesterPlan},
        time::*,
    };
    let (backend, lease) = handles(app)?;
    let mut backend = checked(backend.lock(), "update settings lock")?;
    let year = checked(app.clock.try_now(), "update calendar clock")?
        .date
        .year();
    require(year > 1, "upgrade fixture calendar year")?;
    let first = checked(LocalDate::new(year, 1, 1), "fixture year start")?;
    let anchor = (25..=31)
        .filter_map(|day| LocalDate::new(year - 1, 12, day).ok())
        .find(|date| date.weekday() == Weekday::Monday)
        .ok_or("update fixture: monday anchor")?;
    let mut document = checked(backend.read_schedule(lease), "update schedule draft")?;
    let row = RowId::new(203).unwrap();
    document.semesters = vec![SemesterPlan {
        semester: Semester {
            id: SemesterId::new(201).unwrap(),
            class_id: key(false).class_id,
            revision: Revision::default(),
            enabled: true,
            start: first,
            end: LocalDate::new(year, 12, 31).unwrap(),
            first_week_monday: anchor,
            tables: vec![WeekTable {
                id: TableId::new(202).unwrap(),
                role: TableRole::Single,
                rows: vec![TimetableRow {
                    id: row,
                    logical_key: LogicalRowKey::new(204).unwrap(),
                    group: DayGroup::Morning,
                    kind: RowKind::Course,
                    range: MinuteRange::new(Minute::new(480).unwrap(), Minute::new(525).unwrap())
                        .unwrap(),
                    enabled_days: [true, true, true, true, true, false, false],
                    courses: std::array::from_fn(|day| {
                        (day < 5).then_some(CourseCell {
                            subject_id: key(false).subject_id,
                            teacher: if day == 1 {
                                TeacherBinding::Explicit(key(false).teacher_id)
                            } else {
                                TeacherBinding::Default
                            },
                        })
                    }),
                }],
            }],
        },
        name: "软件更新保留课表".into(),
        timing_sync: false,
        dual_history: None,
        row_details: vec![RowDetails {
            row_id: row,
            name: "合成第一节".into(),
        }],
        cell_details: vec![CellDetails {
            row_id: row,
            day: Weekday::Monday,
            room: "合成教室".into(),
            note: "升级前后保持".into(),
        }],
    }];
    document.update_id_high_water();
    command(&mut backend, lease, CommandFamily::Schedule, |b| {
        checked(
            b.save_schedule(lease, document.revision, document),
            "save real timetable",
        )
    })?;
    let mut public = checked(
        backend.read_public_layout(lease),
        "update public composition",
    )?;
    let root_id = public.layout.tree.root;
    let root = public
        .layout
        .tree
        .nodes
        .iter_mut()
        .find(|n| n.id == root_id)
        .ok_or("update fixture: public root missing")?;
    let NodeKind::Root { padding, gap, .. } = &mut root.kind else {
        return Err("update fixture: public root kind changed".into());
    };
    *gap = 17;
    *padding = 9;
    public.display.clock_mode = ClockMode::Digital12;
    public.display.clock_seconds = true;
    public.display.course_fill_percent = 67;
    public.display.homework_fill_percent = 19;
    public.display.glass_surface_percent = 11;
    public.display.edge_reflection_percent = 37;
    public.display.countdown.label = "软件更新保留倒计时".into();
    public.display.countdown.target = Some(LocalDate::new(year, 12, 20).unwrap());
    command(&mut backend, lease, CommandFamily::Profiles, |b| {
        checked(
            b.save_public_layout(
                lease,
                public.layout.revision,
                public.display.revision,
                public.layout,
                public.display,
            ),
            "save actual public layout and display",
        )
    })?;
    let mut profile = checked(
        backend.read_subject_profile(lease, key(false)),
        "verification subject layout",
    )?
    .ok_or("update fixture: missing subject layout")?;
    let original_texts: Vec<_> = profile
        .tree
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Widget(WidgetKind::SubjectText))
        .map(|node| node.id)
        .collect();
    require(
        original_texts.len() == 1,
        "one original subject text before verification slot",
    )?;
    let verification = checked(
        profile.allocate_id(),
        "allocate independent verification text",
    )?;
    let container = profile
        .tree
        .nodes
        .iter_mut()
        .find(|node| {
            matches!(node.kind, NodeKind::Container(_))
                && node.children.contains(&original_texts[0])
        })
        .ok_or("update fixture: original subject content container missing")?;
    container.children.push(verification);
    profile.tree.nodes.push(LayoutNode {
        id: verification,
        kind: NodeKind::Widget(WidgetKind::SubjectText),
        children: vec![],
    });
    profile.content_styles.push(ContentNodeStyle::new(
        verification,
        ContentPayloadKind::Text,
    ));
    checked(
        profile.validate_structure(),
        "verification subject structure",
    )?;
    command(&mut backend, lease, CommandFamily::Profiles, |b| {
        checked(
            b.save_subject_profile(lease, key(false), profile.revision, profile),
            "save independent verification text slot",
        )
    })?;
    Ok(())
}

fn update_slot(profile: &SubjectProfileDocument, widget: WidgetKind) -> Check<ContentSlotKey> {
    let node_id = profile
        .tree
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Widget(widget))
        .map(|n| n.id)
        .ok_or("update fixture: content widget missing")?;
    Ok(ContentSlotKey {
        scope: LayoutScope::Subject(key(false)),
        node_id,
    })
}
fn update_text_slots(profile: &SubjectProfileDocument) -> Check<(ContentSlotKey, ContentSlotKey)> {
    let mut ids: Vec<_> = profile
        .tree
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Widget(WidgetKind::SubjectText))
        .map(|node| node.id)
        .collect();
    ids.sort();
    require(
        ids.len() == 2 && ids[0] != ids[1],
        "exact two stable subject text nodes",
    )?;
    Ok((
        ContentSlotKey {
            scope: LayoutScope::Subject(profile.key),
            node_id: ids[0],
        },
        ContentSlotKey {
            scope: LayoutScope::Subject(profile.key),
            node_id: ids[1],
        },
    ))
}
fn update_snapshot(
    app: &App,
    paths: &DataPaths,
    manifest: &UpdateManifest,
) -> Check<std::collections::BTreeMap<String, String>> {
    let (backend, lease) = handles(app)?;
    let backend = checked(backend.lock(), "update snapshot lock")?;
    let state = checked(
        backend.management_identity(lease),
        "actual identity preservation read",
    )?;
    require(
        state.accounts.len() == 2
            && state.classes.len() == 1
            && state.subjects.len() == 1
            && state.is_teacher(
                key(false).teacher_id,
                key(false).class_id,
                key(false).subject_id,
            )
            && state.default_teachers
                == vec![DefaultTeacher {
                    class_id: key(false).class_id,
                    subject_id: key(false).subject_id,
                    teacher_id: key(false).teacher_id,
                }],
        "complete synthetic identity and teaching set",
    )?;
    let mut result = std::collections::BTreeMap::new();
    result.insert("identity".into(), update_digest(&state));
    result.insert(
        "display".into(),
        update_digest(&checked(
            backend.read_display(lease),
            "read preserved display",
        )?),
    );
    result.insert(
        "schedule".into(),
        update_digest(&checked(
            backend.read_schedule(lease),
            "read preserved timetable",
        )?),
    );
    result.insert(
        "public_layout".into(),
        update_digest(&checked(
            backend.read_public_layout(lease),
            "read preserved public layout",
        )?),
    );
    let profile = checked(
        backend.read_subject_profile(lease, key(false)),
        "read preserved subject layout",
    )?
    .ok_or("update fixture: subject layout missing")?;
    result.insert("profile".into(), update_digest(&profile));
    let page = checked(
        backend.materials(
            lease,
            MaterialListRequest {
                scope: MaterialScope {
                    teacher_id: key(false).teacher_id,
                    subject_id: key(false).subject_id,
                },
                cursor: None,
            },
        ),
        "read preserved originals index",
    )?;
    require(
        page.total == 2
            && page.items.len() == 2
            && page.next_cursor.is_none()
            && page.items.iter().map(|r| r.id.get()).collect::<Vec<_>>() == vec![400, 500],
        "stable newest-first ordering independent of ID",
    )?;
    result.insert("materials".into(), update_digest(&page));
    let mut objects = Vec::new();
    for record in &page.items {
        let expected = match record.id.get() {
            500 => MATERIAL_TEXT.as_bytes(),
            400 => UPDATE_SECOND_TEXT.as_bytes(),
            _ => return Err("update fixture: unexpected material identity".into()),
        };
        require(
            record.status == MaterialStatus::Published
                && record.resource.kind == MaterialKind::Txt
                && record.resource.bytes == expected.len() as u64
                && record.resource.sha256 == fc_security::sha256(expected),
            "original bytes match published index",
        )?;
        let relative = entry_relative_path(&BackupEntry::Material {
            teacher_id: key(false).teacher_id,
            subject_id: key(false).subject_id,
            material_id: record.id,
            version_id: record.resource.version_id,
            kind: record.resource.kind,
            bytes: record.resource.bytes,
            sha256: record.resource.sha256,
        });
        let pin = exact_file(&paths.root().join(&relative), expected)?;
        objects.push((
            relative,
            checked(pin.identity(), "material original object identity")?,
        ));
    }
    let images = checked(
        backend.read_profile_content(lease, update_slot(&profile, WidgetKind::SubjectImage)?),
        "read both preserved image versions",
    )?;
    require(
        images.current.is_some() && images.previous.is_some() && images.pending.is_none(),
        "both real image versions retained",
    )?;
    for (version, red) in [
        (images.current.as_ref().unwrap(), 180),
        (images.previous.as_ref().unwrap(), 50),
    ] {
        content_fingerprint(&images, version)?;
        let ContentPayload::Images(refs) = &version.payload else {
            return Err("update fixture: image payload changed".into());
        };
        require(refs.len() == 1, "exact image page order")?;
        let reference = &refs[0];
        let bytes = png(red);
        require(
            reference.bytes == bytes.len() as u64
                && reference.sha256 == fc_security::sha256(&bytes)
                && reference.width == 1
                && reference.height == 1
                && reference.format == ImageFormat::Png,
            "preserved image immutable metadata",
        )?;
        let relative = entry_relative_path(&BackupEntry::ProfileMedia {
            version_id: reference.version_id,
            format: reference.format,
            bytes: reference.bytes,
            sha256: reference.sha256,
            width: reference.width,
            height: reference.height,
        });
        let mut pin = exact_file(&paths.root().join(&relative), &bytes)?;
        checked(
            pin.file_mut().seek(SeekFrom::Start(0)),
            "rewind preserved image before WIC",
        )?;
        let decoded = checked(
            image::validate_image(&pin, MaterialKind::Png, || false),
            "WIC decode preserved original",
        )?;
        require(
            decoded.width == 1 && decoded.height == 1,
            "actual original decoder readback",
        )?;
        checked(
            pin.file_mut().seek(SeekFrom::Start(0)),
            "rewind preserved original",
        )?;
        objects.push((
            relative,
            checked(pin.identity(), "image original object identity")?,
        ));
    }
    let inventory = checked(
        backend.inventory_profile_media(None, 16),
        "preserved image resource ownership",
    )?;
    require(
        inventory.items.len() == 2
            && inventory.next_after.is_none()
            && inventory
                .items
                .iter()
                .all(|e| e.refs > 0 && e.resource.is_some()),
        "exact charged image inventory",
    )?;
    result.insert("images".into(), update_digest(&(images, inventory)));
    let text = checked(
        backend.read_profile_content(lease, update_text_slots(&profile)?.0),
        "read preserved node text",
    )?;
    require(
        text.current
            .as_ref()
            .is_some_and(|v| v.payload == ContentPayload::Text(SUBJECT_TEXT.into())),
        "preserved independent node text",
    )?;
    result.insert("text".into(), update_digest(&text));
    let homework = checked(backend.read_content(lease, ContentKey::Homework(HomeworkKey {
        class_id: key(false).class_id, actual_date: manifest.date()?, subject_id: key(false).subject_id, owner_teacher_id: key(false).teacher_id })), "read preserved homework")?
        .ok_or("update fixture: original homework missing (civil-date rollover is not accepted by this probe)")?;
    require(
        homework.text == HOMEWORK_TEXT,
        "preserved actual homework body",
    )?;
    result.insert("homework".into(), update_digest(&homework));
    // Hash only the protected envelope; never unprotect or emit credential bytes.
    let mut envelope = checked(
        LockedFile::open(&paths.root().join("config/credential-key.dpapi"), 4096),
        "pin protected credential key envelope",
    )?;
    let mut encrypted = Vec::new();
    checked(
        envelope.file_mut().read_to_end(&mut encrypted),
        "hash protected envelope",
    )?;
    require(
        !encrypted.is_empty() && encrypted.len() <= 4096,
        "bounded protected envelope",
    )?;
    result.insert(
        "key_envelope".into(),
        update_hex(&fc_security::sha256(&encrypted)),
    );
    objects.push((
        std::path::PathBuf::from("config/credential-key.dpapi"),
        checked(envelope.identity(), "credential envelope object identity")?,
    ));
    result.insert("file_objects".into(), update_digest(&objects));
    Ok(result)
}

pub(super) fn capture_update_data(
    app: &App,
    paths: &DataPaths,
    manifest: &mut UpdateManifest,
) -> Check<()> {
    for (name, digest) in update_snapshot(app, paths, manifest)? {
        manifest.set(&name, digest);
    }
    // Round-trip validates the exact public format before any manifest publication.
    UpdateManifest::parse(&manifest.encode(), manifest.value("nonce")?)?;
    verify_update_data(app, paths, manifest)
}
pub(super) fn verify_update_data(
    app: &App,
    paths: &DataPaths,
    manifest: &UpdateManifest,
) -> Check<()> {
    require(checked(app.clock.try_now(), "verification civil clock")?.date == manifest.date()?,
        "this preservation chain must remain within original current-day content and C2 retention window")?;
    let snapshot = update_snapshot(app, paths, manifest)?;
    for field in UPDATE_HASH_FIELDS {
        require(
            snapshot.get(*field).map(String::as_str) == Some(manifest.value(field)?),
            "preserved public object/envelope/original differs",
        )?;
    }
    let (backend, lease) = handles(app)?;
    let mut backend = checked(backend.lock(), "original upgrade receipt read lock")?;
    let default = DefaultTeacher {
        class_id: key(false).class_id,
        subject_id: key(false).subject_id,
        teacher_id: key(false).teacher_id,
    };
    let original = with_issued(
        &mut backend,
        manifest.issued("default", CommandFamily::Identity)?,
        |b| {
            checked(
                b.default_teacher_outcome(
                    lease,
                    Revision::new(manifest.number("default_expected")?),
                    default,
                ),
                "read original default-teacher receipt without replay",
            )
        },
    )?
    .ok_or("update fixture: original default-teacher receipt absent/retired")?;
    require(
        original == (Revision::new(manifest.number("default_result")?), default)
            && checked(
                backend.state(),
                "current identity independent of original result",
            )?
            .revision
                > original.0,
        "original default result is historical, never a substitute current read",
    )?;
    let profile = checked(
        backend.read_subject_profile(lease, key(false)),
        "original image receipt slot",
    )?
    .ok_or("update fixture: missing profile")?;
    let image = with_issued(
        &mut backend,
        manifest.issued("image", CommandFamily::ProfileContent)?,
        |b| {
            checked(
                b.profile_content_outcome(
                    lease,
                    checked(b.issued_command_id(), "original image command ID")?,
                ),
                "read original earlier image receipt",
            )
        },
    )?
    .ok_or("update fixture: original image receipt absent/retired")?;
    require(
        image.status == ProfileContentStatus::Committed
            && image.slot == update_slot(&profile, WidgetKind::SubjectImage)?
            && image.committed_revision == Some(Revision::new(manifest.number("image_result")?)),
        "first image receipt retained after second version",
    )?;
    let batch = checked(
        backend.material_batch_outcome(
            lease,
            MaterialScope {
                teacher_id: key(false).teacher_id,
                subject_id: key(false).subject_id,
            },
            manifest.issued("materials", CommandFamily::Materials)?,
        ),
        "read original material batch receipt",
    )?
    .ok_or("update fixture: original batch receipt absent/retired")?;
    require(
        update_digest(&batch) == manifest.value("materials_result")?,
        "original complete batch receipt unchanged",
    )?;
    drop(backend);
    verify_update_content(app, manifest)
}
fn verification_text(sequence: u64) -> String {
    format!("软件更新独立验证的新内容，第{sequence}次。")
}
fn read_update_content(app: &App) -> Check<ProfileContentDocument> {
    let (backend, lease) = handles(app)?;
    let backend = checked(backend.lock(), "verification content read lock")?;
    let profile = checked(
        backend.read_subject_profile(lease, key(false)),
        "verification content profile",
    )?
    .ok_or("update fixture: missing verification profile")?;
    checked(
        backend.read_profile_content(lease, update_text_slots(&profile)?.1),
        "independent verification text read",
    )
}
fn verify_update_content(app: &App, manifest: &UpdateManifest) -> Check<()> {
    let document = read_update_content(app)?;
    require(
        document.revision.get() == manifest.number("verification_revision")?,
        "exact verification content CAS revision",
    )?;
    if manifest.sequence()? == 0 {
        require(
            document.revision == Revision::default()
                && document.current.is_none()
                && document.previous.is_none()
                && document.pending.is_none()
                && document.latest_publish_sequence == 0
                && manifest.value("verification_hash")? == update_hex(&fc_security::sha256(b"")),
            "seed has empty independent verification text slot",
        )?;
    } else {
        let version = document
            .current
            .as_ref()
            .ok_or("update fixture: prior verification write missing")?;
        content_fingerprint(&document, version)?;
        require(
            version.revision == document.revision
                && version.payload == ContentPayload::Text(verification_text(manifest.sequence()?))
                && update_hex(&version.digests.payload_sha256)
                    == manifest.value("verification_hash")?
                && version.last_actor_id.get() == 1
                && document.pending.is_none(),
            "exact latest verification text/revision/payload hash preserved",
        )?;
    }
    Ok(())
}
pub(super) fn write_update_content(
    app: &App,
    paths: &DataPaths,
    manifest: &mut UpdateManifest,
) -> Check<()> {
    let next = manifest
        .sequence()?
        .checked_add(1)
        .filter(|s| *s <= 16)
        .ok_or("update fixture: verification sequence exhausted")?;
    let expected = Revision::new(manifest.number("verification_revision")?);
    let (backend, lease) = handles(app)?;
    let profile = checked(
        checked(backend.lock(), "verification slot lock")?.read_subject_profile(lease, key(false)),
        "verification write profile",
    )?
    .ok_or("update fixture: missing verification profile")?;
    let slot = update_text_slots(&profile)?.1;
    let text = verification_text(next);
    publish_with_receipt_expected(
        app,
        paths,
        slot,
        next,
        LocalContentInput::Text(text.clone()),
        ContentPayloadKind::Text,
        Some(expected),
    )?;
    let document = read_update_content(app)?;
    let saved = document
        .current
        .as_ref()
        .ok_or("update fixture: new verification text missing")?;
    content_fingerprint(&document, saved)?;
    require(
        document.revision == checked(expected.next(), "verification revision increment")?
            && document.revision.get() == next
            && saved.revision == document.revision
            && saved.payload == ContentPayload::Text(text)
            && document.pending.is_none(),
        "new independent content commit and full readback",
    )?;
    manifest.set("sequence", next);
    manifest.set("verification_revision", saved.revision.get());
    manifest.set(
        "verification_hash",
        update_hex(&saved.digests.payload_sha256),
    );
    verify_update_content(app, manifest)
}

/// Real App/SQLite regression helper; isolated test deployment only. The public
/// update command still refuses alternate roots and retains its full manifest.
#[cfg(test)]
pub(super) fn test_update_subject_text_cas(app: &mut App, paths: &DataPaths) -> Check<()> {
    fn preserved(
        app: &App,
        paths: &DataPaths,
    ) -> Check<std::collections::BTreeMap<String, String>> {
        let (backend, lease) = handles(app)?;
        let backend = checked(backend.lock(), "test preserved snapshot lock")?;
        let profile = checked(
            backend.read_subject_profile(lease, key(false)),
            "test subject profile",
        )?
        .ok_or("test: missing profile")?;
        let slots = update_text_slots(&profile)?;
        let original_text = checked(
            backend.read_profile_content(lease, slots.0),
            "test original text",
        )?;
        require(
            original_text.current.as_ref().is_some_and(|version| {
                version.payload == ContentPayload::Text(SUBJECT_TEXT.into())
            }),
            "test original text unchanged",
        )?;
        let images = checked(
            backend.read_profile_content(lease, update_slot(&profile, WidgetKind::SubjectImage)?),
            "test original images",
        )?;
        let page = checked(
            backend.materials(
                lease,
                MaterialListRequest {
                    scope: MaterialScope {
                        teacher_id: key(false).teacher_id,
                        subject_id: key(false).subject_id,
                    },
                    cursor: None,
                },
            ),
            "test original material index",
        )?;
        let mut objects = Vec::new();
        require(
            page.items.len() == 1 && page.total == 1,
            "test exact original index",
        )?;
        for record in &page.items {
            let relative = entry_relative_path(&BackupEntry::Material {
                teacher_id: key(false).teacher_id,
                subject_id: key(false).subject_id,
                material_id: record.id,
                version_id: record.resource.version_id,
                kind: record.resource.kind,
                bytes: record.resource.bytes,
                sha256: record.resource.sha256,
            });
            let pin = exact_file(&paths.root().join(&relative), MATERIAL_TEXT.as_bytes())?;
            objects.push((
                relative,
                checked(pin.identity(), "test original material object")?,
            ));
        }
        require(
            images.current.is_some() && images.previous.is_some(),
            "test both original image versions",
        )?;
        for (version, red) in [
            (images.current.as_ref().unwrap(), 180),
            (images.previous.as_ref().unwrap(), 50),
        ] {
            content_fingerprint(&images, version)?;
            let ContentPayload::Images(references) = &version.payload else {
                return Err("test: expected images".into());
            };
            require(references.len() == 1, "test exact original image count")?;
            let reference = &references[0];
            let relative = entry_relative_path(&BackupEntry::ProfileMedia {
                version_id: reference.version_id,
                format: reference.format,
                bytes: reference.bytes,
                sha256: reference.sha256,
                width: reference.width,
                height: reference.height,
            });
            let mut pin = exact_file(&paths.root().join(&relative), &png(red))?;
            checked(
                pin.file_mut().seek(SeekFrom::Start(0)),
                "test rewind original for WIC",
            )?;
            let decoded = checked(
                image::validate_image(&pin, MaterialKind::Png, || false),
                "test WIC retained original",
            )?;
            require(
                decoded.width == 1 && decoded.height == 1,
                "test WIC original dimensions",
            )?;
            objects.push((
                relative,
                checked(pin.identity(), "test original image object")?,
            ));
        }
        let mut snapshot = std::collections::BTreeMap::new();
        snapshot.insert(
            "identity".into(),
            update_digest(&checked(
                backend.management_identity(lease),
                "test identity",
            )?),
        );
        snapshot.insert(
            "schedule".into(),
            update_digest(&checked(backend.read_schedule(lease), "test timetable")?),
        );
        snapshot.insert(
            "display".into(),
            update_digest(&checked(backend.read_display(lease), "test display")?),
        );
        snapshot.insert(
            "public".into(),
            update_digest(&checked(
                backend.read_public_layout(lease),
                "test public layout",
            )?),
        );
        snapshot.insert("profile".into(), update_digest(&profile));
        snapshot.insert("materials".into(), update_digest(&page));
        snapshot.insert("images".into(), update_digest(&images));
        snapshot.insert("original_text".into(), update_digest(&original_text));
        snapshot.insert(
            "inventory".into(),
            update_digest(&checked(
                backend.inventory_profile_media(None, 16),
                "test charged image inventory",
            )?),
        );
        snapshot.insert("original_files".into(), update_digest(&objects));
        let today = checked(app.clock.try_now(), "test content date")?.date;
        snapshot.insert(
            "homework".into(),
            update_digest(&checked(
                backend.read_content(
                    lease,
                    ContentKey::Homework(HomeworkKey {
                        class_id: key(false).class_id,
                        actual_date: today,
                        subject_id: key(false).subject_id,
                        owner_teacher_id: key(false).teacher_id,
                    }),
                ),
                "test original homework",
            )?),
        );
        Ok(snapshot)
    }
    seed_source(app, paths)?;
    seed_update_settings(app)?;
    let baseline = preserved(app, paths)?;
    let mut manifest = UpdateManifest(std::collections::BTreeMap::new());
    manifest.set("sequence", 0);
    manifest.set("verification_revision", 0);
    manifest.set("verification_hash", update_hex(&fc_security::sha256(b"")));
    verify_update_content(app, &manifest)?;
    let stale = manifest.clone();
    write_update_content(app, paths, &mut manifest)?;
    require(
        manifest.sequence()? == 1 && manifest.number("verification_revision")? == 1,
        "test first real subject text publication",
    )?;
    verify_update_content(app, &manifest)?;
    require(
        preserved(app, paths)? == baseline,
        "test first publication preserved originals and metadata",
    )?;
    write_update_content(app, paths, &mut manifest)?;
    require(
        manifest.sequence()? == 2 && manifest.number("verification_revision")? == 2,
        "test second real subject text publication",
    )?;
    verify_update_content(app, &manifest)?;
    let current = read_update_content(app)?;
    require(
        current.previous.as_ref().is_some_and(|version| {
            version.revision.get() == 1
                && version.payload == ContentPayload::Text(verification_text(1))
        }),
        "test independent first publication retained as previous",
    )?;
    let mut stale = stale;
    let stale_before = stale.encode();
    let error = write_update_content(app, paths, &mut stale)
        .err()
        .ok_or("test: stale manifest unexpectedly committed")?;
    require(
        error.contains("RevisionConflict"),
        "test stale manifest rejected by real content CAS",
    )?;
    require(
        stale.encode() == stale_before && read_update_content(app)? == current,
        "test stale CAS did not change published state or manifest",
    )?;
    verify_update_content(app, &manifest)?;
    require(
        preserved(app, paths)? == baseline,
        "test all original data and charged resources unchanged",
    )
}

#[cfg(test)]
mod update_manifest_tests {
    use super::*;
    #[test]
    fn update_manifest_rejects_duplicate_unknown_and_incomplete_public_fields() {
        let nonce = "0123456789abcdef0123456789abcdef";
        assert!(UpdateManifest::parse("FC_UPDATE_DATA_V1\nnonce=x\nnonce=y\n", nonce).is_err());
        assert!(UpdateManifest::parse("FC_UPDATE_DATA_V1\npassword=secret\n", nonce).is_err());
        assert!(UpdateManifest::parse("FC_UPDATE_DATA_V1\n", nonce).is_err());
        assert!(UpdateManifest::parse(&"x".repeat(8193), nonce).is_err());
    }
}
