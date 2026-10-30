//! Windows-only composition checks. Synthetic protected deployments, actual DPAPI,
//! SQLite, strict text decoding and WIC; no windows/audio/Shell are activated.
use super::super::{Initialize, SessionControl, Work};
use super::*;
use fc_bootstrap::backend::MediaReservation;
use fc_bootstrap::profile_content_files::ImagePageInput;
use fc_domain::{
    desktop::DisplaySettings, identity::IdentityCommand, layout::*, profile::LayoutDocument,
};
use fc_presentation::scene::{ContentTextSource, SceneContentIdentity};
use fc_runtime::worker::{JobTicket, Task, WorkerHandle, WorkerPool};
use fc_security::Password;
use fc_windows::{
    materials::{chooser::SelectedFile, files::ControlledFiles},
    system::DataPaths,
    InstanceGuard,
};
use std::{
    io::Write,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

type Check<T> = std::result::Result<T, String>;
const PASSWORD: &str = "Synthetic content-flow password 2026!";
static SERIAL: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);
fn checked<T, E: std::fmt::Debug>(result: std::result::Result<T, E>) -> Check<T> {
    result.map_err(|error| format!("isolated content flow: {error:?}"))
}
struct Setup {
    lease: SessionLease,
    slots: [ContentSlotKey; 4],
}
enum Reply {
    Work(Box<Outcome>),
    Setup(Box<Setup>),
    Lease(SessionLease),
    Exited(Vec<MediaVersionId>),
    Prefix(Box<PrefixOutput>),
}
struct PrefixInput {
    root: PathBuf,
    backend: Arc<Mutex<Backend>>,
    lease: SessionLease,
    slot: ContentSlotKey,
    expected: Revision,
}
struct PrefixOutput {
    ids: Vec<MediaVersionId>,
    retained: MediaReservation,
}
struct HeldInput {
    root: PathBuf,
    backend: Arc<Mutex<Backend>>,
    lease: SessionLease,
    slot: ContentSlotKey,
    expected: Revision,
    selected: SelectedFile,
    reserved: std::sync::mpsc::SyncSender<Vec<MediaVersionId>>,
    release: Arc<AtomicBool>,
    physical_exit: Arc<AtomicBool>,
    settle: Arc<AtomicBool>,
}
enum Job {
    Work(Box<Work>),
    Setup(Arc<Mutex<Backend>>),
    Login(Arc<Mutex<Backend>>),
    Held(Box<HeldInput>),
    Prefix(Box<PrefixInput>),
    Settle(Box<(Arc<Mutex<Backend>>, MediaReservation)>),
}
impl Task for Job {
    type Output = Check<Reply>;
    fn allocation_bytes(&self) -> Option<usize> {
        match self {
            Self::Work(work) => work
                .allocation_bytes()?
                .checked_add(std::mem::size_of::<Work>()),
            Self::Setup(_) | Self::Login(_) => Some(0),
            Self::Held(job) => job
                .root
                .as_os_str()
                .len()
                .checked_mul(2)?
                .checked_add(job.selected.allocation_bytes())?
                .checked_add(std::mem::size_of::<HeldInput>() + 256),
            Self::Prefix(job) => job
                .root
                .as_os_str()
                .len()
                .checked_mul(2)?
                .checked_add(std::mem::size_of::<PrefixInput>()),
            Self::Settle(job) => job
                .1
                .ids
                .capacity()
                .checked_mul(std::mem::size_of::<MediaVersionId>())?
                .checked_add(
                    job.1
                        .bytes
                        .capacity()
                        .checked_mul(std::mem::size_of::<u64>())?,
                )?
                .checked_add(std::mem::size_of::<(Arc<Mutex<Backend>>, MediaReservation)>()),
        }
    }
    fn run(self, context: &JobContext) -> Self::Output {
        checked(context.checkpoint())?;
        match self {
            Self::Work(work) => {
                checked(work.run(context)).map(|outcome| Reply::Work(Box::new(outcome)))
            }
            Self::Login(backend) => checked(backend.lock())
                .and_then(|mut backend| {
                    checked(backend.login("content-flow", checked(Password::new(PASSWORD.into()))?))
                })
                .map(Reply::Lease),
            Self::Setup(backend) => {
                let mut backend = checked(backend.lock())?;
                drop(checked(backend.initialize(
                    Initialize {
                        school_name: "Synthetic content flow".into(),
                        login_name: "content-flow".into(),
                        display_name: "Synthetic global administrator".into(),
                    },
                    checked(Password::new(PASSWORD.into()))?,
                ))?);
                let lease = checked(
                    backend.login("content-flow", checked(Password::new(PASSWORD.into()))?),
                )?;
                let class = ClassId::new(1).unwrap();
                checked(backend.identity(
                    lease,
                    IdentityCommand::CreateClass {
                        id: class,
                        name: "Synthetic class".into(),
                    },
                ))?;
                checked(backend.identity(lease, IdentityCommand::BindCurrentClass { id: class }))?;
                let mut layout = LayoutDocument::default();
                // Two real glass groups replace fallback; Recess's display slot
                // is explicitly removed so six builtins + two groups stays at 8.
                let removed: Vec<_> = layout
                    .tree
                    .nodes
                    .iter()
                    .filter(|node| {
                        matches!(
                            node.kind,
                            NodeKind::Widget(WidgetKind::Materials | WidgetKind::Recess)
                        )
                    })
                    .map(|node| node.id)
                    .collect();
                layout.tree.nodes.retain(|node| !removed.contains(&node.id));
                for node in &mut layout.tree.nodes {
                    node.children.retain(|id| !removed.contains(id));
                }
                let outlet = layout
                    .tree
                    .nodes
                    .iter()
                    .find(|node| matches!(node.kind, NodeKind::Outlet { .. }))
                    .unwrap()
                    .id;
                let mut slots = Vec::new();
                for kind in [ContentPayloadKind::Text, ContentPayloadKind::Images] {
                    let parent = checked(layout.allocate_id())?;
                    let mut children = Vec::new();
                    for _ in 0..2 {
                        let id = checked(layout.allocate_id())?;
                        children.push(id);
                        layout.tree.nodes.push(LayoutNode {
                            id,
                            children: vec![],
                            kind: NodeKind::Widget(match kind {
                                ContentPayloadKind::Text => WidgetKind::SubjectText,
                                ContentPayloadKind::Images => WidgetKind::SubjectImage,
                            }),
                        });
                        layout.content_styles.push(ContentNodeStyle::new(id, kind));
                        slots.push(ContentSlotKey {
                            scope: LayoutScope::Public,
                            node_id: id,
                        });
                    }
                    layout.tree.nodes.push(LayoutNode {
                        id: parent,
                        children,
                        kind: NodeKind::Container(ContainerSettings {
                            flow: Flow::Column,
                            padding: 8,
                            gap: 8,
                            width: AxisSize::Fixed(400),
                            height: AxisSize::Content {
                                min: 0,
                                preferred: 0,
                                max: 1080,
                            },
                            group_opacity_percent: 100,
                            material: Some(GlassMaterial::default()),
                        }),
                    });
                    layout
                        .tree
                        .nodes
                        .iter_mut()
                        .find(|node| node.id == outlet)
                        .unwrap()
                        .children
                        .push(parent);
                }
                checked(backend.save_public_layout(
                    lease,
                    Revision::default(),
                    Revision::default(),
                    layout,
                    DisplaySettings::default(),
                ))?;
                Ok(Reply::Setup(Box::new(Setup {
                    lease,
                    slots: slots.try_into().unwrap(),
                })))
            }
            Self::Held(job) => held_preparation(*job, context),
            Self::Prefix(job) => {
                registry_prefix(*job, context).map(|prefix| Reply::Prefix(Box::new(prefix)))
            }
            Self::Settle(job) => {
                let (backend, reservation) = *job;
                let ids = reservation.ids.clone();
                checked(checked(backend.lock())?.settle_profile_media(reservation))?;
                Ok(Reply::Exited(ids))
            }
        }
    }
}
fn registry_prefix(job: PrefixInput, context: &JobContext) -> Check<PrefixOutput> {
    let mut ids = Vec::new();
    for batch in 0..4 {
        checked(context.checkpoint())?;
        let sources = checked(ContentSources::open(
            LocalContentInput::Images(
                (0..if batch == 3 { 1 } else { 50 })
                    .map(|_| SelectedFile::isolated_probe(&job.root, "pixel.bmp"))
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| format!("synthetic source: {e:?}"))?,
            ),
            &mut SystemRandom,
        ))?;
        let now = checked(CurrentClock::new().try_now())?;
        let operation = ContentOperation::Publish(ContentPayloadKind::Images);
        let request = BeginContentRequest {
            command_id: CommandId::new(2000 + batch).unwrap(),
            slot: job.slot,
            expected_revision: job.expected,
            operation,
            payload_fingerprint: checked(sources.intent_fingerprint(job.slot, operation))?,
            expires_at: LocalDateTime::new(now.date, (now.second_of_day() + 300).min(86399))
                .unwrap(),
        };
        let admission =
            checked(checked(job.backend.lock())?.begin_profile_content(job.lease, request))?;
        let reservation = checked(checked(job.backend.lock())?.reserve_profile_media(
            job.lease,
            &admission,
            &sources.image_sizes().unwrap(),
        ))?;
        // These are real admitted imports cancelled before copying: all source
        // handles really exit before settlement, no fabricated MediaRef/proof.
        drop(sources);
        if batch == 3 {
            return Ok(PrefixOutput {
                ids,
                retained: reservation,
            });
        }
        ids.extend_from_slice(&reservation.ids);
        checked(checked(job.backend.lock())?.settle_profile_media(reservation))?;
        checked(checked(job.backend.lock())?.fail_profile_content(job.lease, admission))?;
    }
    unreachable!("the last reservation is deliberately retained until explicit settle")
}
fn held_preparation(job: HeldInput, context: &JobContext) -> Check<Reply> {
    let sources = checked(ContentSources::open(
        LocalContentInput::Images(vec![job.selected]),
        &mut SystemRandom,
    ))?;
    let now = checked(CurrentClock::new().try_now())?;
    let operation = ContentOperation::Publish(ContentPayloadKind::Images);
    let request = BeginContentRequest {
        command_id: CommandId::new(900).unwrap(),
        slot: job.slot,
        expected_revision: job.expected,
        operation,
        payload_fingerprint: checked(sources.intent_fingerprint(job.slot, operation))?,
        expires_at: LocalDateTime::new(now.date, (now.second_of_day() + 300).min(86399)).unwrap(),
    };
    let admission =
        checked(checked(job.backend.lock())?.begin_profile_content(job.lease, request))?;
    let reservation = checked(checked(job.backend.lock())?.reserve_profile_media(
        job.lease,
        &admission,
        &sources.image_sizes().unwrap(),
    ))?;
    let ids = reservation.ids.clone();
    let files = checked(ControlledContentFiles::new(&job.root))?;
    let guard = checked(checked(job.backend.lock())?.profile_content_preparation_guard(job.lease))?;
    let notified = AtomicBool::new(false);
    let state = || {
        if context.checkpoint().is_err() {
            return PreparationState::Revoked;
        }
        // Signal only once the real guard and physical source owner exist.
        // Receiving reservation IDs alone before guard admission is not an ACK.
        if !notified.swap(true, Ordering::AcqRel) && job.reserved.send(ids.clone()).is_err() {
            return PreparationState::Revoked;
        }
        if !job.release.load(Ordering::Acquire) {
            return PreparationState::AuthorityUpdating;
        }
        match guard.state() {
            fc_bootstrap::backend::ProfileContentPreparationState::Ready => PreparationState::Ready,
            fc_bootstrap::backend::ProfileContentPreparationState::AuthorityUpdating => {
                PreparationState::AuthorityUpdating
            }
            fc_bootstrap::backend::ProfileContentPreparationState::Revoked => {
                PreparationState::Revoked
            }
        }
    };
    // Pins and sources belong to the actual preparation scope. The test can
    // inspect its Reserved registry while that physical owner is still alive.
    let result = {
        let mut preparation = files.preparation_while(Some(&reservation), &state);
        prepare_profile_content(&mut preparation, admission.clone(), sources)
    };
    if result.is_ok() {
        return Err("locked preparation unexpectedly produced a proof".into());
    }
    job.physical_exit.store(true, Ordering::Release);
    while !job.settle.load(Ordering::Acquire) {
        if context.checkpoint().is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    checked(checked(job.backend.lock())?.settle_profile_media(reservation))?;
    // Revocation also denies failure writes from the old lease. No publication
    // is inferred from a missing result; a new owner must reopen the document.
    let _ = checked(job.backend.lock())?.fail_profile_content(job.lease, admission);
    Ok(Reply::Exited(ids))
}
fn submit(handle: &WorkerHandle<Job>, mut task: Job) -> JobTicket<Check<Reply>> {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(90);
    let admission_limit = now + Duration::from_secs(5);
    loop {
        match handle.try_submit(task, deadline) {
            Ok(ticket) => return ticket,
            // Receiver startup can briefly hold the mailbox lock. Retain the
            // returned owner and retry only before any job/SQL was admitted.
            Err(error)
                if error.kind == fc_runtime::worker::Rejection::Busy
                    && Instant::now() < admission_limit =>
            {
                task = error.task;
                std::thread::yield_now();
            }
            Err(error) => panic!("shared content queue: {:?}", error.kind),
        }
    }
}
fn execute(handle: &WorkerHandle<Job>, task: Job) -> Check<Reply> {
    checked(submit(handle, task).wait())?
}
fn product(handle: &WorkerHandle<Job>, work: Work) -> Check<Outcome> {
    match execute(handle, Job::Work(Box::new(work)))? {
        Reply::Work(outcome) => Ok(*outcome),
        _ => Err("unexpected worker reply".into()),
    }
}
fn loaded(outcome: Outcome) -> Loaded {
    match outcome {
        Outcome::ProfileContent(loaded) => *loaded,
        _ => panic!("missing real content reply"),
    }
}
struct Harness {
    owner: Owner,
    control: SessionControl,
    lease: SessionLease,
    slots: [ContentSlotKey; 4],
    gc: Arc<Mutex<GcCursor>>,
}
struct Owner {
    root: PathBuf,
    pool: Option<WorkerPool<Job>>,
    backend: Option<Arc<Mutex<Backend>>>,
    instance: Option<InstanceGuard>,
}
impl std::ops::Deref for Harness {
    type Target = Owner;
    fn deref(&self) -> &Owner {
        &self.owner
    }
}
impl std::ops::DerefMut for Harness {
    fn deref_mut(&mut self) -> &mut Owner {
        &mut self.owner
    }
}
impl Harness {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Content-Flow-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(!root.exists(), "exclusive synthetic test root");
        // Own cleanup even if protected-directory creation/readback fails.
        let mut owner = Owner {
            root: root.clone(),
            pool: None,
            backend: None,
            instance: None,
        };
        DataPaths::isolated_test(root.clone())
            .unwrap()
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        owner.instance = Some(InstanceGuard::acquire(&root).unwrap());
        owner.pool = Some(WorkerPool::start().unwrap());
        let handle = owner.pool.as_ref().unwrap().handle();
        let (backend, control) = match product(
            &handle,
            Work::Open(root.clone(), Arc::new(CurrentClock::new())),
        )
        .unwrap()
        {
            Outcome::Opened(backend, control, _, _) => (backend, control),
            _ => panic!("real Work::Open"),
        };
        owner.backend = Some(backend.clone());
        let setup = match execute(&handle, Job::Setup(backend.clone())).unwrap() {
            Reply::Setup(setup) => setup,
            _ => panic!("worker setup"),
        };
        Self {
            owner,
            control,
            lease: setup.lease,
            slots: setup.slots,
            gc: Arc::new(Mutex::new(GcCursor::default())),
        }
    }
    fn handle(&self) -> WorkerHandle<Job> {
        self.pool.as_ref().unwrap().handle()
    }
    fn backend(&self) -> Arc<Mutex<Backend>> {
        self.backend.as_ref().unwrap().clone()
    }
    fn work(
        &self,
        slot: ContentSlotKey,
        revision: u64,
        id: u64,
        operation: ContentOperation,
        input: LocalContentInput,
    ) -> Work {
        Work::ProfileContent(Box::new(ContentJob {
            root: self.root.clone(),
            backend: self.backend(),
            lease: self.lease,
            operation: Operation::Publish {
                slot,
                expected_revision: Revision::new(revision),
                command_id: CommandId::new(id).unwrap(),
                operation,
                input,
            },
        }))
    }
    fn publish(
        &self,
        slot: ContentSlotKey,
        revision: u64,
        id: u64,
        kind: ContentPayloadKind,
        input: LocalContentInput,
    ) -> Loaded {
        loaded(
            product(
                &self.handle(),
                self.work(slot, revision, id, ContentOperation::Publish(kind), input),
            )
            .unwrap(),
        )
    }
    fn load(&self, slot: ContentSlotKey) -> ProfileContentDocument {
        loaded(
            product(
                &self.handle(),
                Work::ProfileContent(Box::new(ContentJob {
                    root: self.root.clone(),
                    backend: self.backend(),
                    lease: self.lease,
                    operation: Operation::Load { slot },
                })),
            )
            .unwrap(),
        )
        .document
    }
    fn input(&self, name: &str, bytes: &[u8]) -> SelectedFile {
        let files = ControlledFiles::new(&self.root).unwrap();
        let path = self.root.join("quarantine").join(name);
        let mut file = files.create_new(&path).unwrap();
        file.file_mut().write_all(bytes).unwrap();
        file.finish().unwrap();
        self.selected(name)
    }
    fn selected(&self, name: &str) -> SelectedFile {
        SelectedFile::isolated_probe(&self.root, name).unwrap()
    }
    fn reopen(&mut self) {
        let handle = self.handle();
        assert!(matches!(
            product(&handle, Work::Shutdown(self.backend())).unwrap(),
            Outcome::Closed
        ));
        self.backend = None;
        let (backend, control) = match product(
            &handle,
            Work::Open(self.root.clone(), Arc::new(CurrentClock::new())),
        )
        .unwrap()
        {
            Outcome::Opened(backend, control, _, _) => (backend, control),
            _ => panic!("real reopened writer"),
        };
        self.backend = Some(backend.clone());
        self.lease = match execute(&handle, Job::Login(backend.clone())).unwrap() {
            Reply::Lease(lease) => lease,
            _ => panic!("fresh real login"),
        };
        self.control = control;
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        // Test coordinator only. Join real owners before closing SQLite and
        // releasing the instance guard; never delete another deployment path.
        let joined = self.pool.take().map(WorkerPool::join);
        let closed = self
            .backend
            .take()
            .map(|backend| backend.lock().unwrap_or_else(|p| p.into_inner()).close());
        drop(self.instance.take());
        let cleaned = std::fs::remove_dir_all(&self.root);
        if !std::thread::panicking() {
            if let Some(result) = joined {
                result.unwrap();
            }
            if let Some(result) = closed {
                result.unwrap();
            }
            cleaned.unwrap();
        }
    }
}
fn text(document: &ProfileContentDocument) -> &str {
    match &document.current.as_ref().unwrap().payload {
        ContentPayload::Text(text) => text,
        _ => panic!("expected exact current text"),
    }
}
fn pages(document: &ProfileContentDocument) -> Vec<MediaReference> {
    match &document.current.as_ref().unwrap().payload {
        ContentPayload::Images(pages) => pages.clone(),
        _ => panic!("expected exact ordered image pages"),
    }
}
fn inventory(backend: &Arc<Mutex<Backend>>) -> Vec<fc_bootstrap::backend::MediaEntry> {
    let mut result = Vec::new();
    let mut after = None;
    loop {
        let page = backend
            .lock()
            .unwrap()
            .inventory_profile_media(after, 128)
            .unwrap();
        result.extend(page.items);
        match page.next_after {
            Some(next) => after = Some(next),
            None => return result,
        }
    }
}
fn maintained(h: &Harness) -> bool {
    match product(
        &h.handle(),
        Work::MaintainProfileContent(h.root.clone(), h.backend(), h.gc.clone()),
    )
    .unwrap()
    {
        Outcome::ContentMaintained { pending } => pending,
        _ => panic!("actual periodic maintenance reply"),
    }
}
fn assert_removed(
    files: &ControlledContentFiles,
    entry: &fc_bootstrap::backend::MediaEntry,
    reserved_bytes: u64,
) {
    assert_eq!(entry.state, MediaState::Removed, "{entry:?}");
    assert_eq!(entry.refs, 0, "{entry:?}");
    // Inventory retains the original reservation size even after Finalize
    // subtracts it from the catalog. Zero would be a corrupt registry record.
    assert_eq!(entry.charged_bytes, reserved_bytes, "{entry:?}");
    assert!(
        !files
            .owned_inventory()
            .unwrap()
            .iter()
            .any(|physical| physical.id == entry.id),
        "Removed media must have neither an original nor quarantine file: {entry:?}"
    );
}
fn bmp() -> Vec<u8> {
    let mut bytes = vec![0; 58];
    bytes[..2].copy_from_slice(b"BM");
    for (offset, value) in [(2, 58u32), (10, 54), (14, 40), (18, 1), (22, 1), (34, 4)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
    bytes[54..57].copy_from_slice(&[40, 80, 160]);
    bytes
}

#[test]
fn real_worker_txt_unicode_two_nodes_clear_explicit_undo_and_dpapi_sqlite_reopen() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let mut h = Harness::new();
    let handle = h.handle();
    let emoji = "😀".repeat(10_000);
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(emoji.as_bytes());
    let selected = h.input("ten-thousand.txt", &bytes);
    let first = submit(
        &handle,
        Job::Work(Box::new(h.work(
            h.slots[0],
            0,
            101,
            ContentOperation::Publish(ContentPayloadKind::Text),
            LocalContentInput::Text("independent first".into()),
        ))),
    );
    let second = submit(
        &handle,
        Job::Work(Box::new(h.work(
            h.slots[1],
            0,
            102,
            ContentOperation::Publish(ContentPayloadKind::Text),
            LocalContentInput::Txt(selected),
        ))),
    );
    let first = match first.wait().unwrap().unwrap() {
        Reply::Work(outcome) => loaded(*outcome),
        _ => panic!("first"),
    };
    let second = match second.wait().unwrap().unwrap() {
        Reply::Work(outcome) => loaded(*outcome),
        _ => panic!("second"),
    };
    assert_eq!(text(&first.document), "independent first");
    assert_eq!(text(&second.document), emoji);
    assert_eq!(first.document.revision, Revision::new(1));
    assert_eq!(second.document.revision, Revision::new(1));
    assert_eq!(first.document.latest_publish_sequence, 1);
    assert_eq!(second.document.latest_publish_sequence, 1);
    let invalid = h.input("invalid.txt", &[0xff, 0xfe, 0x41]); // incomplete UTF16 code unit
    assert!(product(
        &handle,
        h.work(
            h.slots[1],
            1,
            103,
            ContentOperation::Publish(ContentPayloadKind::Text),
            LocalContentInput::Txt(invalid)
        ),
    )
    .is_err());
    let unchanged = h.load(h.slots[1]);
    assert_eq!(unchanged.revision, Revision::new(1));
    assert_eq!(text(&unchanged), emoji);
    assert!(unchanged.latest_publish_sequence > 1);
    assert!(product(
        &handle,
        h.work(
            h.slots[1],
            1,
            104,
            ContentOperation::Publish(ContentPayloadKind::Text),
            LocalContentInput::Text("😀".repeat(10_001))
        )
    )
    .is_err());
    let cleared = loaded(
        product(
            &handle,
            h.work(
                h.slots[0],
                1,
                105,
                ContentOperation::Clear,
                LocalContentInput::Empty,
            ),
        )
        .unwrap(),
    );
    assert_eq!(
        cleared.document.current.as_ref().unwrap().payload,
        ContentPayload::Empty
    );
    let undone = loaded(
        product(
            &handle,
            h.work(
                h.slots[0],
                2,
                106,
                ContentOperation::Undo {
                    target_revision: Revision::new(1),
                },
                LocalContentInput::Empty,
            ),
        )
        .unwrap(),
    );
    assert_eq!(text(&undone.document), "independent first");
    assert_eq!(undone.document.revision, Revision::new(3));
    h.reopen();
    assert_eq!(text(&h.load(h.slots[0])), "independent first");
    assert_eq!(text(&h.load(h.slots[1])), emoji);
    match product(&handle, Work::Desktop(h.backend(), 0, None)).unwrap() {
        Outcome::Desktop(facts, frame) => {
            let snapshot = facts
                .profile_contents
                .iter()
                .find(|snapshot| snapshot.slot == h.slots[1])
                .unwrap();
            let PublishedContentDescriptor::Text { sha256, .. } = snapshot.content else {
                panic!("text descriptor")
            };
            assert_eq!(
                frame
                    .text(snapshot.slot, snapshot.revision, sha256)
                    .unwrap()
                    .as_ref(),
                emoji
            );
        }
        _ => panic!("real desktop facts hydration"),
    }
}

#[test]
fn real_worker_wic_fifty_pages_existing_reorder_partial_failure_and_physical_settlement() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let mut h = Harness::new();
    let handle = h.handle();
    drop(h.input("pixel.bmp", &bmp()));
    let first = submit(
        &handle,
        Job::Work(Box::new(h.work(
            h.slots[2],
            0,
            201,
            ContentOperation::Publish(ContentPayloadKind::Images),
            LocalContentInput::Images((0..50).map(|_| h.selected("pixel.bmp")).collect()),
        ))),
    );
    let second = submit(
        &handle,
        Job::Work(Box::new(h.work(
            h.slots[3],
            0,
            202,
            ContentOperation::Publish(ContentPayloadKind::Images),
            LocalContentInput::Images(vec![h.selected("pixel.bmp")]),
        ))),
    );
    let original = match first.wait().unwrap().unwrap() {
        Reply::Work(outcome) => loaded(*outcome),
        _ => panic!("50-page WIC"),
    };
    let independent = match second.wait().unwrap().unwrap() {
        Reply::Work(outcome) => loaded(*outcome),
        _ => panic!("independent WIC"),
    };
    let original_pages = pages(&original.document);
    assert_eq!(original_pages.len(), 50);
    assert_eq!(pages(&independent.document).len(), 1);
    assert!(original_pages
        .iter()
        .all(|reference| reference.sha256 == fc_security::sha256(&bmp())));
    let over_limit = Job::Work(Box::new(h.work(
        h.slots[2],
        1,
        203,
        ContentOperation::Publish(ContentPayloadKind::Images),
        LocalContentInput::Images((0..51).map(|_| h.selected("pixel.bmp")).collect()),
    )));
    match handle.try_submit(over_limit, Instant::now() + Duration::from_secs(90)) {
        Err(error) => assert_eq!(error.kind, fc_runtime::worker::Rejection::TooLarge),
        Ok(_) => panic!("51-page descriptor must be rejected before admission"),
    }
    let edited = h.publish(
        h.slots[2],
        1,
        204,
        ContentPayloadKind::Images,
        LocalContentInput::ImagePages(vec![
            ImagePageInput::Existing(original_pages[49].clone()),
            ImagePageInput::Selected(h.selected("pixel.bmp")),
            ImagePageInput::Existing(original_pages[0].clone()),
        ]),
    );
    let edited_pages = pages(&edited.document);
    assert_eq!(edited_pages[0], original_pages[49]);
    assert_eq!(edited_pages[2], original_pages[0]);
    assert!(!original_pages
        .iter()
        .any(|page| page.version_id == edited_pages[1].version_id));
    let previous = match &edited.document.previous.as_ref().unwrap().payload {
        ContentPayload::Images(pages) => pages,
        _ => panic!("previous manifest"),
    };
    assert_eq!(previous, &original_pages);
    let deleted = h.publish(
        h.slots[2],
        2,
        205,
        ContentPayloadKind::Images,
        LocalContentInput::ImagePages(vec![
            ImagePageInput::Existing(edited_pages[1].clone()),
            ImagePageInput::Existing(edited_pages[0].clone()),
        ]),
    );
    assert_eq!(
        pages(&deleted.document),
        vec![edited_pages[1].clone(), edited_pages[0].clone()]
    );
    let registry = inventory(&h.backend());
    for reference in &edited_pages {
        let entry = registry
            .iter()
            .find(|entry| entry.id == reference.version_id)
            .unwrap();
        assert_eq!(entry.state, MediaState::Referenced);
        assert!(entry.refs > 0);
        assert_eq!(entry.charged_bytes, reference.bytes);
    }
    assert!(registry
        .iter()
        .any(|entry| entry.state == MediaState::Removed));
    let clear = loaded(
        product(
            &handle,
            h.work(
                h.slots[2],
                3,
                207,
                ContentOperation::Clear,
                LocalContentInput::Empty,
            ),
        )
        .unwrap(),
    );
    assert_eq!(
        clear.document.current.as_ref().unwrap().payload,
        ContentPayload::Empty
    );
    let restored = loaded(
        product(
            &handle,
            h.work(
                h.slots[2],
                4,
                208,
                ContentOperation::Undo {
                    target_revision: Revision::new(3),
                },
                LocalContentInput::Empty,
            ),
        )
        .unwrap(),
    );
    assert_eq!(pages(&restored.document), pages(&deleted.document));
    let bad = h.input("invalid.bmp", b"BMnot-an-image");
    assert!(product(
        &handle,
        h.work(
            h.slots[2],
            5,
            206,
            ContentOperation::Publish(ContentPayloadKind::Images),
            LocalContentInput::Images(vec![h.selected("pixel.bmp"), bad])
        )
    )
    .is_err());
    let after_failure = h.load(h.slots[2]);
    assert_eq!(after_failure.current, restored.document.current);
    assert_eq!(after_failure.previous, restored.document.previous);
    assert_eq!(after_failure.revision, restored.document.revision);
    assert!(after_failure.latest_publish_sequence > restored.document.latest_publish_sequence);
    let files = ControlledContentFiles::new(&h.root).unwrap();
    let failed_entries = || {
        inventory(&h.backend())
            .into_iter()
            .filter(|entry| entry.command_id == CommandId::new(206).unwrap())
            .collect::<Vec<_>>()
    };
    let physical_failed = || {
        let failed = failed_entries();
        files
            .owned_inventory()
            .unwrap()
            .into_iter()
            .filter(|physical| failed.iter().any(|entry| entry.id == physical.id))
            .collect::<Vec<_>>()
    };
    let mut failed = failed_entries();
    eprintln!(
        "partial-bundle command=206 after physical owner exit: registry={failed:?}, physical={:?}",
        physical_failed()
    );
    assert_eq!(failed.len(), 2, "{failed:?}");
    for (entry, size) in failed
        .iter()
        .zip([bmp().len() as u64, b"BMnot-an-image".len() as u64])
    {
        assert_eq!(entry.owner, h.slots[2], "{entry:?}");
        assert_eq!(
            entry.refs, 0,
            "failed preparation cannot publish references"
        );
        assert_eq!(entry.charged_bytes, size, "{entry:?}");
        assert!(
            matches!(
                entry.state,
                MediaState::Unreferenced | MediaState::Retiring | MediaState::Removed
            ),
            "the exited owner must settle Reserved; failed preparation cannot be Referenced: {entry:?}"
        );
    }
    // A failed Publish performs opportunistic, nonblocking GC. Busy IO/locks
    // may leave Unreferenced/Retiring entries fully charged; no contract grants
    // an immediate refund. Drive the actual periodic Work and shared cursor,
    // rather than settling another owner or pretending those bytes are zero.
    let pending_bytes: u64 = failed
        .into_iter()
        .filter(|entry| entry.state != MediaState::Removed)
        .map(|entry| entry.charged_bytes)
        .sum();
    if pending_bytes > 0 {
        eprintln!("partial-bundle command=206 still charged before maintenance: {pending_bytes}");
    }
    failed = failed_entries();
    for pass in 0..4 {
        if failed
            .iter()
            .all(|entry| entry.state == MediaState::Removed)
        {
            break;
        }
        let pending = maintained(&h);
        failed = failed_entries();
        eprintln!(
            "partial-bundle command=206 maintenance pass={pass} pending={pending}: registry={failed:?}, physical={:?}",
            physical_failed()
        );
        assert!(
            pending
                || failed
                    .iter()
                    .all(|entry| entry.state == MediaState::Removed),
            "maintenance must not report completion while failed resources remain: {failed:?}"
        );
    }
    assert_eq!(failed.len(), 2, "{failed:?}");
    for (entry, size) in failed
        .iter()
        .zip([bmp().len() as u64, b"BMnot-an-image".len() as u64])
    {
        assert_removed(&files, entry, size);
    }
    let after_cleanup = h.load(h.slots[2]);
    assert_eq!(after_cleanup.current, after_failure.current);
    assert_eq!(after_cleanup.previous, after_failure.previous);
    assert_eq!(after_cleanup.revision, after_failure.revision);
    assert_eq!(
        after_cleanup.latest_publish_sequence,
        after_failure.latest_publish_sequence
    );
    let facts = h.backend().lock().unwrap().desktop_facts(0).unwrap();
    let requests = facts
        .profile_contents
        .iter()
        .filter_map(|snapshot| match &snapshot.content {
            PublishedContentDescriptor::Image { current, .. } => Some(ImageRenderRequest {
                identity: SceneContentIdentity {
                    slot: snapshot.slot,
                    revision: snapshot.revision,
                    view_revision: snapshot.view_revision,
                },
                reference: current.clone(),
                max_pixel_width: 32,
                max_pixel_height: 32,
            }),
            _ => None,
        })
        .collect();
    match product(
        &handle,
        Work::ContentImages(Box::new(ImageJob {
            root: h.root.clone(),
            backend: h.backend(),
            generation: SceneGeneration::new(1),
            source_revision: facts.global_revision,
            captured_at: facts.captured_at,
            valid_until: None,
            requests,
            cache: Arc::new(RasterCache::new()),
        })),
    )
    .unwrap()
    {
        Outcome::ContentImages(completion) => assert_eq!(completion.frames.len(), 2),
        _ => panic!("actual two-node raster hydration"),
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let release = Arc::new(AtomicBool::new(false));
    let physical_exit = Arc::new(AtomicBool::new(false));
    let settle = Arc::new(AtomicBool::new(false));
    struct Release(Arc<AtomicBool>, Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
            self.1.store(true, Ordering::Release);
        }
    }
    let _release = Release(release.clone(), settle.clone());
    let held = submit(
        &handle,
        Job::Held(Box::new(HeldInput {
            root: h.root.clone(),
            backend: h.backend(),
            lease: h.lease,
            slot: h.slots[3],
            expected: Revision::new(1),
            selected: h.selected("pixel.bmp"),
            reserved: sender,
            release: release.clone(),
            physical_exit: physical_exit.clone(),
            settle: settle.clone(),
        })),
    );
    let ids = receiver.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(!collect_garbage(&files, &h.backend()).unwrap());
    assert!(
        !maintained(&h),
        "periodic worker must not settle a live Reserved owner"
    );
    for id in &ids {
        let entry = inventory(&h.backend())
            .into_iter()
            .find(|entry| entry.id == *id)
            .unwrap();
        assert_eq!(entry.state, MediaState::Reserved);
        assert!(entry.charged_bytes > 0);
    }
    assert!(!physical_exit.load(Ordering::Acquire));
    h.control.lock_local();
    release.store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !physical_exit.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    for id in &ids {
        assert_eq!(
            inventory(&h.backend())
                .into_iter()
                .find(|entry| entry.id == *id)
                .unwrap()
                .state,
            MediaState::Reserved
        );
    }
    settle.store(true, Ordering::Release);
    match held.wait().unwrap().unwrap() {
        Reply::Exited(exited) => assert_eq!(exited, ids),
        _ => panic!("physical owner exit"),
    };
    for id in &ids {
        assert_eq!(
            inventory(&h.backend())
                .into_iter()
                .find(|entry| entry.id == *id)
                .unwrap()
                .state,
            MediaState::Unreferenced
        );
    }
    assert!(!maintained(&h));
    for id in &ids {
        let entry = inventory(&h.backend())
            .into_iter()
            .find(|entry| entry.id == *id)
            .unwrap();
        assert_removed(&files, &entry, bmp().len() as u64);
    }
    assert!(product(
        &handle,
        h.work(
            h.slots[0],
            0,
            901,
            ContentOperation::Publish(ContentPayloadKind::Text),
            LocalContentInput::Text("old lease cannot publish a different node".into()),
        )
    )
    .is_err());
    h.reopen();
    assert!(h.load(h.slots[0]).current.is_none());
    assert_eq!(h.load(h.slots[2]).current, restored.document.current);
    assert_eq!(h.load(h.slots[3]).current, independent.document.current);

    // Retire an actual published original while a native reader still pins its
    // handle. Automatic cleanup and the periodic worker must retain its charge,
    // then physically remove/finalize only after the reader really releases it.
    let old = pages(&independent.document)[0].clone();
    let original = h
        .root
        .join("profile-content")
        .join("originals")
        .join(format!("{:020}.bmp", old.version_id.get()));
    let pin =
        fc_windows::materials::files::LockedFile::open(&original, MAX_CONTENT_IMAGE_BYTES).unwrap();
    let second = h.publish(
        h.slots[3],
        1,
        902,
        ContentPayloadKind::Images,
        LocalContentInput::Images(vec![h.selected("pixel.bmp")]),
    );
    assert_eq!(
        inventory(&h.backend())
            .into_iter()
            .find(|entry| entry.id == old.version_id)
            .unwrap()
            .state,
        MediaState::Referenced
    );
    let third = h.publish(
        h.slots[3],
        2,
        903,
        ContentPayloadKind::Images,
        LocalContentInput::Images(vec![h.selected("pixel.bmp")]),
    );
    assert!(third.cleanup_pending);
    assert!(original.exists());
    assert!(
        maintained(&h),
        "busy native reader is pending physical cleanup"
    );
    let retiring = inventory(&h.backend())
        .into_iter()
        .find(|entry| entry.id == old.version_id)
        .unwrap();
    assert_eq!(retiring.state, MediaState::Retiring);
    assert_eq!(retiring.charged_bytes, old.bytes);
    drop(pin);
    assert!(!maintained(&h));
    assert!(
        !original.exists(),
        "periodic work must actually delete the immutable original"
    );
    assert!(!files
        .owned_inventory()
        .unwrap()
        .iter()
        .any(|entry| entry.id == old.version_id));
    let registry = inventory(&h.backend());
    let removed = registry
        .iter()
        .find(|entry| entry.id == old.version_id)
        .unwrap();
    assert_removed(&files, removed, old.bytes);
    for reference in pages(&second.document)
        .iter()
        .chain(pages(&third.document).iter())
    {
        let entry = registry
            .iter()
            .find(|entry| entry.id == reference.version_id)
            .unwrap();
        assert_eq!(entry.state, MediaState::Referenced);
        assert!(entry.refs > 0);
    }
    assert_eq!(h.load(h.slots[3]).current, third.document.current);
    assert_eq!(h.load(h.slots[2]).current, restored.document.current);

    // A real >128-entry SQLite registry, including an explicitly un-settled
    // admission, precedes the next actually published original. The prefix
    // imports own real source pins, then exit before their trusted settlement.
    let prefix = match execute(
        &handle,
        Job::Prefix(Box::new(PrefixInput {
            root: h.root.clone(),
            backend: h.backend(),
            lease: h.lease,
            slot: h.slots[3],
            expected: Revision::new(3),
        })),
    )
    .unwrap()
    {
        Reply::Prefix(prefix) => *prefix,
        _ => panic!("real registry prefix"),
    };
    assert_eq!(prefix.ids.len(), 150);
    let retained_id = prefix.retained.ids[0];
    assert!(inventory(&h.backend()).len() > 128);
    let tail = h.publish(
        h.slots[3],
        3,
        2100,
        ContentPayloadKind::Images,
        LocalContentInput::Images(vec![h.selected("pixel.bmp")]),
    );
    let tail_reference = pages(&tail.document)[0].clone();
    assert!(tail_reference.version_id.get() > 128);
    let tail_path = h
        .root
        .join("profile-content")
        .join("originals")
        .join(format!("{:020}.bmp", tail_reference.version_id.get()));
    let tail_pin =
        fc_windows::materials::files::LockedFile::open(&tail_path, MAX_CONTENT_IMAGE_BYTES)
            .unwrap();
    let tail_previous = h.publish(
        h.slots[3],
        4,
        2101,
        ContentPayloadKind::Images,
        LocalContentInput::Images(vec![h.selected("pixel.bmp")]),
    );
    let tail_last = h.publish(
        h.slots[3],
        5,
        2102,
        ContentPayloadKind::Images,
        LocalContentInput::Images(vec![h.selected("pixel.bmp")]),
    );
    assert!(h.gc.lock().unwrap().after.is_none());
    let cancel_at = prefix.ids[10];
    let before_tail = || {
        h.gc.lock()
            .unwrap()
            .after
            .is_none_or(|id| id.get() < cancel_at.get())
    };
    assert!(collect_garbage_while(&files, &h.backend(), &before_tail, &h.gc).unwrap());
    assert_eq!(
        h.gc.lock().unwrap().after,
        Some(cancel_at),
        "cancelled result must not erase completed entry progress"
    );
    assert!(maintained(&h));
    assert!(
        h.gc.lock()
            .unwrap()
            .after
            .is_some_and(|id| id.get() > cancel_at.get()),
        "next physical job must continue past prefix"
    );
    assert!(
        maintained(&h),
        "busy tail is reached despite cancelled prefix"
    );
    assert!(
        h.gc.lock().unwrap().after.is_none(),
        "registry tail wraps for retrying earlier busy entries"
    );
    let tail_entry = inventory(&h.backend())
        .into_iter()
        .find(|entry| entry.id == tail_reference.version_id)
        .unwrap();
    assert_eq!(tail_entry.state, MediaState::Retiring);
    assert_eq!(tail_entry.charged_bytes, tail_reference.bytes);
    assert_eq!(
        inventory(&h.backend())
            .into_iter()
            .find(|entry| entry.id == retained_id)
            .unwrap()
            .state,
        MediaState::Reserved
    );
    drop(tail_pin);

    // Fixture-only cancellation trigger at a real physical effect, independent
    // of callback counts: once deletion is observable, stop before finalize.
    // Production callbacks remain the memory-only JobContext checkpoint.
    for _ in 0..4 {
        if !tail_path.exists() {
            break;
        }
        assert!(
            collect_garbage_while(&files, &h.backend(), &|| tail_path.exists(), &h.gc).unwrap()
        );
    }
    assert!(!tail_path.exists());
    let before_finalize = inventory(&h.backend())
        .into_iter()
        .find(|entry| entry.id == tail_reference.version_id)
        .unwrap();
    assert_eq!(before_finalize.state, MediaState::Retiring);
    assert_eq!(
        before_finalize.charged_bytes, tail_reference.bytes,
        "physical deletion alone cannot refund unsettled registry"
    );
    assert!(
        h.gc.lock()
            .unwrap()
            .after
            .is_some_and(|id| id.get() < tail_reference.version_id.get()),
        "mid-entry cancellation cannot advance past unfinished finalize"
    );
    assert!(!maintained(&h));
    assert!(h.gc.lock().unwrap().after.is_none());
    let registry = inventory(&h.backend());
    let removed = registry
        .iter()
        .find(|entry| entry.id == tail_reference.version_id)
        .unwrap();
    assert_removed(&files, removed, tail_reference.bytes);
    let reserved = registry
        .iter()
        .find(|entry| entry.id == retained_id)
        .unwrap();
    assert_eq!(reserved.state, MediaState::Reserved);
    assert!(reserved.charged_bytes > 0);
    for reference in pages(&tail_previous.document)
        .iter()
        .chain(pages(&tail_last.document).iter())
    {
        assert_eq!(
            registry
                .iter()
                .find(|entry| entry.id == reference.version_id)
                .unwrap()
                .state,
            MediaState::Referenced
        );
    }
    match execute(
        &handle,
        Job::Settle(Box::new((h.backend(), prefix.retained))),
    )
    .unwrap()
    {
        Reply::Exited(ids) => assert_eq!(ids, vec![retained_id]),
        _ => panic!("explicit original-owner settlement"),
    }
    for _ in 0..3 {
        if !maintained(&h) {
            break;
        }
    }
    let settled = inventory(&h.backend())
        .into_iter()
        .find(|entry| entry.id == retained_id)
        .unwrap();
    assert_removed(&files, &settled, bmp().len() as u64);
    assert_eq!(h.load(h.slots[3]).current, tail_last.document.current);
    assert_eq!(h.load(h.slots[2]).current, restored.document.current);
    // Work::Open performs real startup validation, including catalog bytes ==
    // the sum of original reservation sizes for every non-Removed registry
    // entry. Thus retained per-entry sizes are not mistaken for current debt,
    // and physical deletion plus Finalize must persist a consistent refund.
    drop(files);
    h.reopen();
    let reopened_files = ControlledContentFiles::new(&h.root).unwrap();
    let registry = inventory(&h.backend());
    for entry in registry
        .iter()
        .filter(|entry| entry.state == MediaState::Removed)
    {
        assert_removed(&reopened_files, entry, entry.charged_bytes);
    }
    assert_eq!(h.load(h.slots[3]).current, tail_last.document.current);
    assert_eq!(h.load(h.slots[2]).current, restored.document.current);
}
