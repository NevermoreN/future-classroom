//! Synthetic, isolated native vertical check. Uses exactly the product's shared
//! two-worker domain; it never invokes the user's actual file associations.
use super::{
    material_flow::{Job, MaterialJob, OpenRequest, PreparedLaunch},
    *,
};
use fc_domain::materials::{MaterialRecord, MaterialScope};
use fc_ports::{MaterialDeleteRequest, MaterialListRequest};
use fc_runtime::worker::WorkerHandle;
use fc_windows::materials::{chooser::SelectedFile, shell::*};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

type Check<T> = std::result::Result<T, String>;
const PASSWORD: &str = "Synthetic probe password 2026!";
fn checked<T, E>(value: std::result::Result<T, E>, stage: &str) -> Check<T> {
    value.map_err(|_| format!("materials-probe: {stage}"))
}
fn require(value: bool, stage: &str) -> Check<()> {
    if value {
        Ok(())
    } else {
        Err(format!("materials-probe: {stage}"))
    }
}
fn pass(label: &str) {
    println!("MATERIALS_PASS {label}");
}

enum ProbeTask {
    Work(Box<Work>),
    Setup(Arc<Mutex<Backend>>),
    Login(Arc<Mutex<Backend>>),
    DeleteMetadata(Arc<Mutex<Backend>>, SessionLease, MaterialDeleteRequest),
    Write(PathBuf, Vec<u8>),
    Barrier(Arc<AtomicUsize>, Arc<AtomicBool>),
}
enum ProbeOutput {
    Work(Box<Outcome>),
    Ready(SessionLease),
    Done,
}
impl Task for ProbeTask {
    type Output = Check<ProbeOutput>;
    fn allocation_bytes(&self) -> Option<usize> {
        match self {
            Self::Work(work) => work
                .allocation_bytes()?
                .checked_add(std::mem::size_of::<Work>()),
            Self::Write(path, bytes) => path
                .as_os_str()
                .len()
                .checked_mul(2)?
                .checked_add(bytes.capacity()),
            _ => Some(1024),
        }
    }
    fn run(self, context: &JobContext) -> Self::Output {
        checked(context.checkpoint(), "worker admission")?;
        match self {
            Self::Work(work) => work
                .run(context)
                .map(|outcome| ProbeOutput::Work(Box::new(outcome)))
                .map_err(|error| format!("materials-probe: product work: {error:?}")),
            Self::Login(backend) => {
                let mut backend = checked(backend.lock(), "login lock")?;
                let password = checked(Password::new(PASSWORD.into()), "password")?;
                checked(backend.login("probe", password), "real login").map(ProbeOutput::Ready)
            }
            Self::Setup(backend) => {
                let mut backend = checked(backend.lock(), "setup lock")?;
                let password = checked(Password::new(PASSWORD.into()), "password")?;
                // The recovery value is never printed, cloned or placed in a DTO.
                drop(checked(
                    backend.initialize(
                        Initialize {
                            school_name: "Synthetic materials school".into(),
                            login_name: "probe".into(),
                            display_name: "Probe teacher".into(),
                        },
                        password,
                    ),
                    "initialize",
                )?);
                let lease = checked(
                    backend.login(
                        "probe",
                        checked(Password::new(PASSWORD.into()), "password")?,
                    ),
                    "login",
                )?;
                let class = checked(ClassId::new(1), "class")?;
                let subject = checked(SubjectId::new(1), "subject")?;
                for command in [
                    IdentityCommand::CreateClass {
                        id: class,
                        name: "Synthetic class".into(),
                    },
                    IdentityCommand::BindCurrentClass { id: class },
                    IdentityCommand::PutSubject {
                        id: subject,
                        name: "Synthetic subject".into(),
                        enabled: true,
                        color: Rgb {
                            red: 40,
                            green: 80,
                            blue: 160,
                        },
                        expected_revision: None,
                    },
                    IdentityCommand::UpdateAccount {
                        id: lease.principal_id(),
                        display_name: "Probe teacher".into(),
                        role: Role::GlobalAdmin,
                        enabled: true,
                        scopes: vec![AssignmentScope {
                            class_id: class,
                            subject_id: subject,
                        }],
                    },
                ] {
                    checked(backend.identity(lease, command), "identity fixture")?;
                }
                checked(
                    backend.login(
                        "probe",
                        checked(Password::new(PASSWORD.into()), "password")?,
                    ),
                    "renew exact lease",
                )
                .map(ProbeOutput::Ready)
            }
            Self::DeleteMetadata(backend, lease, request) => {
                checked(
                    checked(backend.lock(), "retirement metadata lock")?
                        .delete_material(lease, request),
                    "durable delete before restart",
                )?;
                Ok(ProbeOutput::Done)
            }
            Self::Write(path, bytes) => {
                checked(fs::write(path, bytes), "synthetic write")?;
                Ok(ProbeOutput::Done)
            }
            Self::Barrier(entered, release) => {
                entered.fetch_add(1, Ordering::Release);
                while !release.load(Ordering::Acquire) {
                    checked(context.checkpoint(), "barrier cancellation")?;
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(ProbeOutput::Done)
            }
        }
    }
}
fn submit(
    handle: &WorkerHandle<ProbeTask>,
    task: ProbeTask,
) -> Check<JobTicket<Check<ProbeOutput>>> {
    handle
        .try_submit(task, Instant::now() + Duration::from_secs(45))
        .map_err(|_| "materials-probe: shared queue admission".into())
}
fn execute(handle: &WorkerHandle<ProbeTask>, task: ProbeTask) -> Check<ProbeOutput> {
    checked(submit(handle, task)?.wait(), "worker completion")?
}
fn product(handle: &WorkerHandle<ProbeTask>, work: Work) -> Check<Outcome> {
    match execute(handle, ProbeTask::Work(Box::new(work)))? {
        ProbeOutput::Work(outcome) => Ok(*outcome),
        _ => Err("materials-probe: unexpected worker result".into()),
    }
}
fn ready(handle: &WorkerHandle<ProbeTask>, task: ProbeTask) -> Check<SessionLease> {
    match execute(handle, task)? {
        ProbeOutput::Ready(lease) => Ok(lease),
        _ => Err("materials-probe: missing exact lease".into()),
    }
}
struct Harness {
    root: PathBuf,
    pool: Option<WorkerPool<ProbeTask>>,
    backend: Option<Arc<Mutex<Backend>>>,
    shell: Option<ShellLauncher>,
    association: Option<ProbeAssociation>,
}
impl Harness {
    fn cleanup(&mut self) -> Check<()> {
        // Headless coordinator only: drain every owned worker before touching DB
        // handles or deleting paths, including an earlier validation failure.
        let mut failure = None;
        if let Some(pool) = self.pool.take() {
            if pool.join().is_err() {
                failure = Some("materials-probe: worker shutdown".into());
            }
        }
        if let Some(backend) = self.backend.take() {
            if backend
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .close()
                .is_err()
            {
                failure = Some("materials-probe: writer shutdown".into());
            }
        }
        if let Some(shell) = self.shell.take() {
            shell.stop();
            let until = Instant::now() + Duration::from_secs(10);
            while !shell.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            if !shell.is_finished() {
                self.shell = Some(shell);
                return Err("materials-probe: shell still running; test tree retained".into());
            }
        }
        if let Some(mut association) = self.association.take() {
            checked(association.remove(), "remove private association")?;
        }
        if self.root.exists() {
            checked(fs::remove_dir_all(&self.root), "remove isolated test tree")?;
        }
        failure.map_or(Ok(()), Err)
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
struct Session {
    root: PathBuf,
    backend: Arc<Mutex<Backend>>,
    control: SessionControl,
    lease: SessionLease,
    scope: MaterialScope,
    class: ClassId,
}
impl Session {
    fn work(&self, job: Job) -> Work {
        Work::Material(Box::new(MaterialJob {
            root: self.root.clone(),
            backend: self.backend.clone(),
            control: self.control.clone(),
            job,
        }))
    }
    fn list(
        &self,
        handle: &WorkerHandle<ProbeTask>,
        cursor: Option<fc_domain::materials::MaterialCursor>,
    ) -> Check<MaterialPage> {
        match product(
            handle,
            self.work(Job::Load {
                lease: self.lease,
                request: MaterialListRequest {
                    scope: self.scope,
                    cursor,
                },
            }),
        )? {
            Outcome::Materials(page) => Ok(*page),
            _ => Err("materials-probe: missing page".into()),
        }
    }
    fn import(
        &self,
        handle: &WorkerHandle<ProbeTask>,
        name: &str,
        bytes: Vec<u8>,
    ) -> Check<MaterialPage> {
        execute(
            handle,
            ProbeTask::Write(self.root.join("quarantine").join(name), bytes),
        )?;
        let selected = checked(
            SelectedFile::isolated_probe(&self.root, name),
            "synthetic selection",
        )?;
        match product(
            handle,
            self.work(Job::Import {
                lease: self.lease,
                scope: self.scope,
                classes: vec![self.class],
                selected,
            }),
        )
        .map_err(|error| format!("materials-probe: import synthetic {name}: {error}"))?
        {
            Outcome::Materials(page) => Ok(*page),
            _ => Err("materials-probe: missing imported page".into()),
        }
    }
    fn open(
        &self,
        handle: &WorkerHandle<ProbeTask>,
        record: &MaterialRecord,
    ) -> Check<PreparedLaunch> {
        match product(
            handle,
            self.work(Job::Open(OpenRequest::Owner {
                lease: self.lease,
                scope: self.scope,
                id: record.id,
                version: record.resource.version_id,
            })),
        )? {
            Outcome::MaterialLaunch(launch) => Ok(*launch),
            _ => Err("materials-probe: missing fixed launch copy".into()),
        }
    }
    fn delete(&self, handle: &WorkerHandle<ProbeTask>, record: &MaterialRecord) -> Check<()> {
        product(
            handle,
            self.work(Job::Delete {
                lease: self.lease,
                request: MaterialDeleteRequest {
                    command_id: checked(command_id(), "delete id")?,
                    scope: self.scope,
                    material_id: record.id,
                    expected_material_revision: record.revision,
                },
            }),
        )?;
        Ok(())
    }
    fn original(&self, record: &MaterialRecord, extension: &str) -> PathBuf {
        self.root
            .join("teachers")
            .join(self.scope.teacher_id.get().to_string())
            .join("subjects")
            .join(self.scope.subject_id.get().to_string())
            .join(record.id.get().to_string())
            .join(format!(
                "{}.{}",
                record.resource.version_id.get(),
                extension
            ))
    }
}
fn dispatch(
    shell: &mut ShellLauncher,
    control: &SessionControl,
    launch: PreparedLaunch,
) -> Check<LaunchOutcome> {
    let gate = control.health_gate();
    let lease = checked(
        gate.lease_for_authorized_scope(MATERIAL_OPEN_SCOPE, launch.guard.expires_at),
        "shell health lease",
    )?;
    let guard = launch.guard;
    let id = launch.request_id;
    checked(
        shell.submit(LaunchRequest {
            request_id: id,
            version: launch.resource_version,
            path: launch.path,
            gate,
            lease,
            origin: LaunchOrigin::OwnerWorkbench,
            recheck: Arc::new(move || guard.is_valid()),
        }),
        "shell queue",
    )?;
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        for result in shell.take_results() {
            if result.request_id == id {
                return Ok(result.outcome);
            }
        }
        require(Instant::now() < end, "shell result timeout")?;
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn run() -> Check<()> {
    let mut nonce = [0u8; 8];
    checked(SystemRandom.fill(&mut nonce), "system random")?;
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-materials-{}-{:016x}",
        std::process::id(),
        u64::from_le_bytes(nonce)
    ));
    let paths = checked(DataPaths::isolated_test(root.clone()), "isolated paths")?;
    let mut harness = Harness {
        root: root.clone(),
        pool: None,
        backend: None,
        shell: None,
        association: None,
    };
    let result = (|| {
        let _deployment = checked(
            paths.prepare_for_current_user(),
            "protected test deployment",
        )?;
        harness.pool = Some(checked(WorkerPool::start(), "two-worker startup")?);
        let handle = harness
            .pool
            .as_ref()
            .ok_or("materials-probe: pool missing")?
            .handle();
        let clock = Arc::new(CurrentClock::new());
        let (backend, control) = match product(&handle, Work::Open(root.clone(), clock.clone()))? {
            Outcome::Opened(backend, control, _) => (backend, control),
            _ => return Err("materials-probe: open result".into()),
        };
        harness.backend = Some(backend.clone());
        let lease = ready(&handle, ProbeTask::Setup(backend.clone()))?;
        let mut session = Session {
            root: root.clone(),
            backend,
            control,
            lease,
            scope: MaterialScope {
                teacher_id: lease.principal_id(),
                subject_id: checked(SubjectId::new(1), "subject")?,
            },
            class: checked(ClassId::new(1), "class")?,
        };
        pass("real-dpapi-sqlite-kdf-exact-lease");
        let first = session
            .import(&handle, "same.txt", b"synthetic first original\n".to_vec())?
            .items[0]
            .clone();
        let page = session.import(&handle, "same.txt", b"synthetic second original\n".to_vec())?;
        require(
            page.total == 2
                && page.items[0].id != first.id
                && page.items[0].resource.version_id != first.resource.version_id,
            "same-name independent records",
        )?;
        for _ in 0..19 {
            session.import(&handle, "same.txt", b"synthetic paged material\n".to_vec())?;
        }
        let page = session.list(&handle, None)?;
        require(
            page.items.len() == 20 && page.total == 21,
            "management page bound",
        )?;
        let cursor = page
            .next_cursor
            .clone()
            .ok_or("materials-probe: missing cursor")?;
        require(
            session.list(&handle, Some(cursor.clone()))?.items.len() == 1,
            "second management page",
        )?;
        let newest = session
            .import(&handle, "same.txt", b"new folder revision\n".to_vec())?
            .items[0]
            .clone();
        require(
            session.list(&handle, Some(cursor)).is_err(),
            "stale folder cursor",
        )?;
        pass("same-name-independent-append-management20-cursor");
        session.import(&handle, "safe.pdf", pdf(false))?;
        require(
            session.import(&handle, "active.pdf", pdf(true)).is_err(),
            "active PDF rejected by constrained helper",
        )?;
        session.import(&handle, "pixel.bmp", bmp())?;
        session.import(&handle, "sample.docx", docx())?;
        pass("pdf-helper-job-wic-bmp-streamed-docx");
        let fixed = session.open(&handle, &first)?;
        let copy_path = fixed.path.path().to_owned();
        let original = session.original(&first, "txt");
        require(
            checked(fs::read(&original), "original contents")? == b"synthetic first original\n",
            "exact original preserved",
        )?;
        session.delete(&handle, &first)?;
        require(
            !original.exists()
                && checked(fs::read(&copy_path), "retained fixed copy")?
                    == b"synthetic first original\n"
                && !fixed.guard.is_context_valid(),
            "delete retires original and invalidates authorization without altering fixed copy",
        )?;
        drop(fixed);
        let tamper = session.original(&newest, "txt");
        execute(
            &handle,
            ProbeTask::Write(tamper, b"tampered bytes\n".to_vec()),
        )?;
        require(
            session.open(&handle, &newest).is_err(),
            "tampered original hash",
        )?;
        session.delete(&handle, &newest)?;
        pass("fixed-version-copy-delete-retirement-original-tamper");
        // Two barriers prove both physical shared workers are occupied. Queued
        // cancellation is deterministic and cannot begin filesystem/SQLite work.
        let entered = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(AtomicBool::new(false));
        let a = submit(
            &handle,
            ProbeTask::Barrier(entered.clone(), release.clone()),
        )?;
        let b = submit(
            &handle,
            ProbeTask::Barrier(entered.clone(), release.clone()),
        )?;
        let until = Instant::now() + Duration::from_secs(5);
        while entered.load(Ordering::Acquire) != 2 {
            if Instant::now() >= until {
                release.store(true, Ordering::Release);
                return Err("materials-probe: two physical workers".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let canceled = submit(
            &handle,
            ProbeTask::Work(Box::new(session.work(Job::Load {
                lease: session.lease,
                request: MaterialListRequest {
                    scope: session.scope,
                    cursor: None,
                },
            }))),
        )?;
        canceled.cancel();
        release.store(true, Ordering::Release);
        checked(a.wait(), "first shared worker")??;
        checked(b.wait(), "second shared worker")??;
        require(canceled.wait().is_err(), "queued material cancellation")?;
        let record = session.list(&handle, None)?.items[0].clone();
        let stale = session.open(&handle, &record)?;
        session.control.lock_local();
        require(!stale.guard.is_valid(), "lock invalidates prepared launch")?;
        let (shell, association) = checked(
            ShellLauncher::isolated_probe(&root),
            "private receiver association",
        )?;
        harness.shell = Some(shell);
        harness.association = Some(association);
        require(
            dispatch(
                harness
                    .shell
                    .as_mut()
                    .ok_or("materials-probe: missing launcher")?,
                &session.control,
                stale,
            )? == LaunchOutcome::Canceled,
            "stale launch denied before shell",
        )?;
        // The production Shell queue applies a 500ms resource cooldown even
        // after a canceled request. Headless probe pacing must respect it.
        std::thread::sleep(Duration::from_millis(510));
        session.lease = ready(&handle, ProbeTask::Login(session.backend.clone()))?;
        let launch = session.open(&handle, &record)?;
        let receipt = launch
            .path
            .path()
            .parent()
            .ok_or("materials-probe: copy directory")?
            .join(".future-classroom-open-receipt");
        require(
            dispatch(
                harness
                    .shell
                    .as_mut()
                    .ok_or("materials-probe: missing launcher")?,
                &session.control,
                launch,
            )? == LaunchOutcome::Requested,
            "real isolated ShellExecuteEx",
        )?;
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            // Creation makes the name visible before NewFile::finish closes its
            // exclusive writer. Observe the complete readable receipt, not just
            // existence; an in-flight flush/sharing violation is not a failure.
            if let Ok(bytes) = fs::read(&receipt) {
                require(
                    bytes == b"LAUNCH_REQUEST_RECEIVED\n",
                    "real receiver receipt",
                )?;
                break;
            }
            require(Instant::now() < until, "private receiver receipt timeout")?;
            std::thread::sleep(Duration::from_millis(10));
        }
        checked(fs::remove_file(receipt), "receipt cleanup")?;
        pass("two-shared-workers-cancel-lock-stale-shell-private-receiver");
        // Crash-window fixture: delete commits, but the process has not yet
        // performed filesystem retirement. Startup must resolve this DB truth.
        let pending_retirement = session.original(&record, "docx");
        execute(
            &handle,
            ProbeTask::DeleteMetadata(
                session.backend.clone(),
                session.lease,
                MaterialDeleteRequest {
                    command_id: checked(command_id(), "restart delete id")?,
                    scope: session.scope,
                    material_id: record.id,
                    expected_material_revision: record.revision,
                },
            ),
        )?;
        require(
            pending_retirement.exists(),
            "unreferenced original retained until startup reconciliation",
        )?;
        let orphan = root.join("staging").join("synthetic-orphan.part");
        execute(
            &handle,
            ProbeTask::Write(orphan.clone(), b"uncommitted synthetic orphan".to_vec()),
        )?;
        product(&handle, Work::Shutdown(session.backend.clone()))?;
        harness.backend = None;
        let restored = match product(&handle, Work::Open(root.clone(), clock))? {
            Outcome::Opened(backend, control, _) => {
                session.backend = backend.clone();
                session.control = control;
                backend
            }
            _ => return Err("materials-probe: restart result".into()),
        };
        harness.backend = Some(restored.clone());
        session.lease = ready(&handle, ProbeTask::Login(restored))?;
        require(
            !orphan.exists()
                && !original.exists()
                && !pending_retirement.exists()
                && session.list(&handle, None)?.total == 22,
            "restart durable inventory and orphan reconciliation",
        )?;
        pass("restart-durable-records-orphan-reconcile");
        Ok(())
    })();
    let cleanup = harness.cleanup();
    result.and(cleanup)?;
    pass("all-owned-processes-registry-files-cleaned");
    Ok(())
}

fn pdf(active: bool) -> Vec<u8> {
    let extra = if active {
        "/AA << /O << /S /JavaScript /JS (probe) >> >>"
    } else {
        ""
    };
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] {extra} >>"),
    ];
    let mut data = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).bytes());
    }
    let xref = data.len();
    data.extend(b"xref\n0 4\n0000000000 65535 f \n");
    for offset in offsets {
        data.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    data.extend(format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").bytes());
    data
}
fn bmp() -> Vec<u8> {
    let mut data = vec![0u8; 58];
    data[..2].copy_from_slice(b"BM");
    for (offset, value) in [(2, 58u32), (10, 54), (14, 40), (18, 1), (22, 1), (34, 4)] {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    data[26..28].copy_from_slice(&1u16.to_le_bytes());
    data[28..30].copy_from_slice(&24u16.to_le_bytes());
    data[54..57].copy_from_slice(&[40, 80, 160]);
    data
}
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}
fn docx() -> Vec<u8> {
    let parts = [
        (
            "[Content_Types].xml",
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>",
        ),
        (
            "_rels/.rels",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>",
        ),
        (
            "word/document.xml",
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>Synthetic document</w:t></w:r></w:p></w:body></w:document>",
        ),
    ];
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, text) in parts {
        let offset = out.len() as u32;
        let size = text.len() as u32;
        let crc = crc32(text.as_bytes());
        out.extend(0x04034b50u32.to_le_bytes());
        for n in [20u16, 0x800, 0, 0, 0] {
            out.extend(n.to_le_bytes());
        }
        for n in [crc, size, size] {
            out.extend(n.to_le_bytes());
        }
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name.bytes());
        out.extend(text.bytes());
        central.extend(0x02014b50u32.to_le_bytes());
        for n in [20u16, 20, 0x800, 0, 0, 0] {
            central.extend(n.to_le_bytes());
        }
        for n in [crc, size, size] {
            central.extend(n.to_le_bytes());
        }
        central.extend((name.len() as u16).to_le_bytes());
        for _ in 0..4 {
            central.extend(0u16.to_le_bytes());
        }
        central.extend(0u32.to_le_bytes());
        central.extend(offset.to_le_bytes());
        central.extend(name.bytes());
    }
    let offset = out.len() as u32;
    let size = central.len() as u32;
    out.extend(central);
    out.extend(0x06054b50u32.to_le_bytes());
    for n in [0u16, 0, 3, 3] {
        out.extend(n.to_le_bytes());
    }
    out.extend(size.to_le_bytes());
    out.extend(offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}
