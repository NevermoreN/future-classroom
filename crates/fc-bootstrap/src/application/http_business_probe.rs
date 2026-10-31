//! Explicit finite loopback API diagnostic. It uses the desktop's same Work
//! enum/two workers; it neither starts the desktop nor enables normal listening.
//! Local approval is synthetic trusted test input, not visible-admin UI evidence.
use super::*;
use fc_bootstrap::{
    web_application::{quarantined_uploads, retry_quarantined_uploads, WebApplication},
    web_assets::StaticAssets,
    web_transfer::{network::NetCoordinator, DownloadHubOwner, UploadHubOwner},
};
use fc_ports::http::ListenPrefix;
use fc_windows::tls_frontdoor::TlsHttpHost;

const PROBE_SECONDS: u64 = 45;
const LOGIN_NAME: &str = "probe";
const PASSWORD: &str = "Synthetic TLS business password 2026!";

pub(super) enum Job {
    LoadStatic {
        parent: PathBuf,
        marker: String,
    },
    Setup {
        parent: PathBuf,
        marker: String,
        origin: String,
    },
    Approve {
        backend: std::sync::Weak<Mutex<Backend>>,
        admin: SessionLease,
    },
}
pub(super) enum Completion {
    StaticAssets(StaticAssets),
    Setup(Setup),
    Approved(usize),
}
pub(super) struct Setup {
    root: PathBuf,
    backend: Arc<Mutex<Backend>>,
    application: Arc<WebApplication>,
    service: WebService,
    admin: SessionLease,
    pairing_code: String,
}
fn checked<T, E>(value: std::result::Result<T, E>, stage: &'static str) -> Result<T> {
    value.map_err(|_| Failure::Deployment(stage))
}
fn require(value: bool, stage: &'static str) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Failure::Deployment(stage))
    }
}
impl Job {
    pub(super) fn allocation_bytes(&self) -> Option<usize> {
        let heap = match self {
            Self::LoadStatic { parent, marker } => parent
                .as_os_str()
                .len()
                .checked_mul(2)?
                .checked_add(marker.capacity())?,
            Self::Setup {
                parent,
                marker,
                origin,
            } => parent
                .as_os_str()
                .len()
                .checked_mul(2)?
                .checked_add(marker.capacity())?
                .checked_add(origin.capacity())?,
            Self::Approve { .. } => 0,
        };
        heap.checked_add(std::mem::size_of::<Self>())
    }
    pub(super) fn run(self, context: &JobContext) -> Result<Completion> {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        match self {
            Self::LoadStatic { parent, marker } => {
                validate_parent(&parent, &marker)?;
                let prefix = checked(
                    ListenPrefix::https("https://localhost:443/fc/"),
                    "business unbound static prefix",
                )?;
                let assets = checked(
                    StaticAssets::load_in_job(&prefix, &parent.join("web-package"), context),
                    "business verified static package before native open",
                )?;
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                Ok(Completion::StaticAssets(assets))
            }
            Self::Setup {
                parent,
                marker,
                origin,
            } => setup(parent, &marker, &origin, context).map(Completion::Setup),
            Self::Approve { backend, admin } => {
                let backend = backend.upgrade().ok_or(Failure::Stopped)?;
                let mut backend = match backend.try_lock() {
                    Ok(backend) => backend,
                    Err(std::sync::TryLockError::WouldBlock) => return Ok(Completion::Approved(0)),
                    Err(std::sync::TryLockError::Poisoned(_)) => return Err(Failure::Stopped),
                };
                let scoped = backend.begin_execution_scope(Arc::new(context.execution_fence()));
                let service = scoped.web_service();
                let pending = match service.pending(admin) {
                    Ok(pending) => pending,
                    Err(WebServiceError::Busy) => return Ok(Completion::Approved(0)),
                    Err(error) => return Err(Failure::Web(error)),
                };
                let mut approved = 0;
                for review in pending.into_iter().filter(|review| !review.approved) {
                    context.checkpoint().map_err(|_| Failure::Stopped)?;
                    service
                        .approve(admin, review.request_id, review.identity_revision)
                        .map_err(Failure::Web)?;
                    approved += 1;
                }
                Ok(Completion::Approved(approved))
            }
        }
    }
}
fn validate_parent(parent: &std::path::Path, marker: &str) -> Result<()> {
    require(
        parent.file_name().and_then(|s| s.to_str()) == Some(marker),
        "business probe parent identity",
    )?;
    require(
        checked(
            fs::read_to_string(parent.join(".tls-probe-root")),
            "business probe marker read",
        )? == marker,
        "business probe marker mismatch",
    )?;
    Ok(())
}
fn setup(parent: PathBuf, marker: &str, origin: &str, context: &JobContext) -> Result<Setup> {
    validate_parent(&parent, marker)?;
    let root = parent.join("FutureClassroom-Test-Business");
    require(
        !root.exists(),
        "business probe refuses pre-existing deployment",
    )?;
    let paths = checked(
        DataPaths::isolated_test(root.clone()),
        "business isolated path",
    )?;
    let deployment = checked(
        paths.prepare_for_current_user(),
        "business isolated deployment ACL",
    )?;
    checked(deployment.read_back(), "business deployment ACL read-back")?;
    let generated = checked(
        CredentialRecordCodec::generate(&mut SystemRandom),
        "business CNG credential key",
    )?;
    let dpapi = CurrentUserDpapi::default();
    let protected = checked(
        generated.protect_key(|key| dpapi.protect(&SecretBytes::new(key.to_vec())?)),
        "business DPAPI protection",
    )?;
    drop(generated);
    let key = checked(dpapi.unprotect(&protected), "business DPAPI reopen")?;
    let bytes = checked(
        <[u8; 32]>::try_from(key.expose()),
        "business credential key length",
    )?;
    let clock = Arc::new(CurrentClock::new());
    let mut backend = Backend::open(
        paths.database(),
        checked(
            CredentialRecordCodec::from_key(bytes),
            "business credential codec",
        )?,
        clock.clone(),
        clock,
    )?;
    drop(key);
    let admin = {
        let mut scoped = backend.begin_execution_scope(Arc::new(context.execution_fence()));
        drop(scoped.initialize(
            Initialize {
                school_name: "Synthetic TLS school".into(),
                login_name: LOGIN_NAME.into(),
                display_name: "Synthetic TLS administrator".into(),
            },
            checked(
                Password::new(PASSWORD.into()),
                "business synthetic password",
            )?,
        )?);
        let admin = scoped.login(
            LOGIN_NAME,
            checked(Password::new(PASSWORD.into()), "business synthetic login")?,
        )?;
        let class = checked(ClassId::new(1), "business synthetic class")?;
        let subject = checked(SubjectId::new(1), "business synthetic subject")?;
        scoped.identity(
            admin,
            IdentityCommand::CreateClass {
                id: class,
                name: "Synthetic TLS class".into(),
            },
        )?;
        scoped.identity(admin, IdentityCommand::BindCurrentClass { id: class })?;
        scoped.identity(
            admin,
            IdentityCommand::PutSubject {
                id: subject,
                name: "Synthetic TLS subject".into(),
                enabled: true,
                color: Rgb {
                    red: 2,
                    green: 3,
                    blue: 4,
                },
                expected_revision: None,
            },
        )?;
        scoped.identity(
            admin,
            IdentityCommand::UpdateAccount {
                id: admin.principal_id(),
                display_name: "Synthetic TLS administrator".into(),
                role: Role::GlobalAdmin,
                enabled: true,
                scopes: vec![AssignmentScope {
                    class_id: class,
                    subject_id: subject,
                }],
            },
        )?;
        let admin = scoped.login(
            LOGIN_NAME,
            checked(
                Password::new(PASSWORD.into()),
                "business refreshed synthetic login",
            )?,
        )?;
        // Do not retain the job-specific WebService clone beyond this call.
        admin
    };
    let service = backend.web_service();
    service.set_enabled(admin, true).map_err(Failure::Web)?;
    let pairing = service.begin_pairing(admin).map_err(Failure::Web)?;
    let pairing_code = pairing.code.expose_text().as_str().to_owned();
    let prefix = checked(
        ListenPrefix::https(&format!("{origin}/fc/")),
        "business HTTPS prefix",
    )?;
    let mut application = checked(
        WebApplication::new(&prefix, service.clone()),
        "business Web boundary",
    )?;
    checked(
        application.configure_material_files(&root),
        "business material files",
    )?;
    checked(
        application.configure_voice_files(&backend, &root),
        "business voice files",
    )?;
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    Ok(Setup {
        root,
        backend: Arc::new(Mutex::new(backend)),
        application: Arc::new(application),
        service,
        admin,
        pairing_code,
    })
}
fn startup_call(
    pool: &shared_pool::SharedPool<Work>,
    job: Job,
    deadline: Instant,
) -> std::result::Result<Completion, String> {
    let mut task = Work::HttpProbe(Box::new(job));
    let mut ticket = loop {
        match pool.handle().try_submit(task, deadline) {
            Ok(ticket) => break ticket,
            Err(rejected)
                if rejected.kind == fc_runtime::worker::Rejection::Busy
                    && Instant::now() < deadline =>
            {
                task = rejected.task;
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(rejected) => {
                return Err(format!("business startup admission: {:?}", rejected.kind))
            }
        }
    };
    loop {
        if ticket.is_finished() {
            return match ticket.try_result() {
                Ok(Some(Ok(Outcome::HttpProbe(completed)))) => Ok(*completed),
                Ok(Some(Err(error))) => Err(format!("business startup: {error:?}")),
                Err(error) => Err(format!("business startup worker: {error:?}")),
                _ => Err("business startup result correlation".into()),
            };
        }
        if Instant::now() >= deadline {
            ticket.cancel();
            return Err("business startup deadline; actual worker join still required".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn stop_failed_start(pool: shared_pool::SharedPool<Work>, error: String) -> String {
    pool.request_stop();
    match pool.join() {
        Ok(()) => error,
        Err(join) => format!("{error}; business startup physical join: {join:?}"),
    }
}
pub(super) fn run(thumbprint: &str, marker: &str) -> std::result::Result<(), String> {
    require(
        marker.starts_with("FutureClassroom-Test-TLS-")
            && marker.len() <= 96
            && marker
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "business marker validation",
    )
    .map_err(|e| format!("{e:?}"))?;
    let parent = std::env::current_dir().map_err(|_| "business current directory")?;
    let pool =
        shared_pool::SharedPool::<Work>::start().map_err(|_| "business shared worker startup")?;
    // One original startup deadline includes both jobs, queueing and metadata
    // rebinding. No listener exists while untrusted package bytes are read.
    let deadline = Instant::now() + Duration::from_secs(25);
    let static_assets = match startup_call(
        &pool,
        Job::LoadStatic {
            parent: parent.clone(),
            marker: marker.to_owned(),
        },
        deadline,
    ) {
        Ok(Completion::StaticAssets(assets)) => assets,
        Ok(_) => {
            return Err(stop_failed_start(
                pool,
                "business static result type".into(),
            ))
        }
        Err(error) => return Err(stop_failed_start(pool, error)),
    };
    let host = match TlsHttpHost::open(thumbprint, marker) {
        Ok(host) => host,
        Err(error) => return Err(stop_failed_start(pool, error)),
    };
    let port = host.port_number();
    let generation = host.generation();
    let origin = format!("https://localhost:{port}");
    let prefix = match ListenPrefix::https(&format!("{origin}/fc/")) {
        Ok(prefix) => prefix,
        Err(error) => {
            return Err(stop_failed_start(
                pool,
                format!("business actual prefix: {error:?}"),
            ))
        }
    };
    let static_assets = match static_assets.with_prefix(&prefix) {
        Ok(assets) => Arc::new(assets),
        Err(error) => {
            return Err(stop_failed_start(
                pool,
                format!("business static prefix: {error:?}"),
            ))
        }
    };
    // The shared bytes stay in this coordinator, never in the 4MiB work queue.
    let setup = match startup_call(
        &pool,
        Job::Setup {
            parent,
            marker: marker.to_owned(),
            origin: origin.clone(),
        },
        deadline,
    ) {
        Ok(Completion::Setup(setup)) => setup,
        Ok(_) => return Err(stop_failed_start(pool, "business setup result type".into())),
        Err(error) => return Err(stop_failed_start(pool, error)),
    };
    let Setup {
        root,
        backend,
        application,
        service,
        admin,
        pairing_code,
    } = setup;
    let (mut uploads, hub) =
        UploadHubOwner::new().map_err(|e| format!("business upload owner: {e:?}"))?;
    let (mut downloads, download_hub) =
        DownloadHubOwner::new().map_err(|e| format!("business download owner: {e:?}"))?;
    let mut net = NetCoordinator::new(
        host,
        http_executor::SharedHttpExecutor::new(pool.handle()),
        &backend,
        &application,
        hub,
        download_hub,
    );
    let static_count = static_assets.asset_count();
    net.set_static_assets(static_assets)?;
    println!("TLS_BUSINESS_READY {{\"port\":{port},\"generation\":{generation},\"marker\":\"{marker}\",\"pairingCode\":\"{pairing_code}\",\"origin\":\"{origin}\",\"basePath\":\"/fc/\",\"classId\":\"1\",\"subjectId\":\"1\",\"loginName\":\"{LOGIN_NAME}\",\"staticAssets\":{static_count}}}");
    checked(std::io::stdout().flush(), "business READY flush").map_err(|e| format!("{e:?}"))?;
    let until = Instant::now() + Duration::from_secs(PROBE_SECONDS);
    let mut approval: Option<JobTicket<Result<Outcome>>> = None;
    let mut approvals = 0usize;
    let mut next_approval = Instant::now();
    let active = (|| -> std::result::Result<(), String> {
        while Instant::now() < until {
            net.poll()?;
            if let Some(ticket) = approval.as_mut() {
                match ticket.try_result() {
                    Ok(Some(Ok(Outcome::HttpProbe(completed)))) => {
                        match *completed {
                            Completion::Approved(n) => approvals += n,
                            _ => return Err("business approval result type".into()),
                        }
                        approval = None;
                    }
                    Ok(None) => {}
                    Ok(Some(Err(e))) => return Err(format!("business approval: {e:?}")),
                    Err(e) => return Err(format!("business approval worker: {e:?}")),
                    _ => return Err("business approval wrong work outcome".into()),
                }
            }
            // This finite fixture authorizes one synthetic device. Stop polling
            // after it is approved so an unnecessary admin pending() read does
            // not contend with the transport's nonwaiting send fence.
            if approvals == 0 && approval.is_none() && Instant::now() >= next_approval {
                let job = Work::HttpProbe(Box::new(Job::Approve {
                    backend: Arc::downgrade(&backend),
                    admin,
                }));
                match pool.handle().try_submit(job, until) {
                    Ok(ticket) => approval = Some(ticket),
                    Err(e) if e.kind == fc_runtime::worker::Rejection::Busy => {}
                    Err(e)
                        if e.kind == fc_runtime::worker::Rejection::Deadline
                            && Instant::now() >= until =>
                    {
                        break
                    }
                    Err(e) => return Err(format!("business approval admission: {:?}", e.kind)),
                }
                next_approval = Instant::now() + Duration::from_millis(100);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })();
    net.request_stop()?;
    if let Some(ticket) = approval.as_ref() {
        ticket.cancel();
    }
    let drain_until = Instant::now() + Duration::from_secs(20);
    loop {
        net.poll()?;
        let approval_done = approval.as_ref().is_none_or(JobTicket::is_finished);
        if net.is_idle() && approval_done {
            break;
        }
        if Instant::now() >= drain_until {
            pool.request_stop();
            return Err(
                "business retained owner: native/job cleanup not physically complete".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(approval);
    let report = net.report();
    println!("TLS_BUSINESS_REPORT {report:?}");
    pool.request_stop();
    pool.join()
        .map_err(|e| format!("business physical shared-worker join: {e:?}"))?;
    let cleanup = uploads.drain_after_join(&application);
    if !cleanup.is_drained() {
        return Err(format!("business retained uploads: {cleanup:?}"));
    }
    let download_cleanup = downloads.drain_after_join();
    if !download_cleanup.is_drained() {
        return Err(format!("business retained downloads: {download_cleanup:?}"));
    }
    drop(net);
    drop(uploads);
    drop(downloads);
    require(
        Arc::strong_count(&application) == 1,
        "business application still physically owned",
    )
    .map_err(|e| format!("{e:?}"))?;
    drop(application);
    let status = retry_quarantined_uploads();
    if status.retained != 0 || quarantined_uploads() != 0 {
        return Err("business retained physical cleanup".into());
    }
    backend
        .lock()
        .map_err(|_| "business final backend lock")?
        .close()
        .map_err(|e| format!("business writer close: {e:?}"))?;
    drop(service);
    drop(backend);
    let paths = DataPaths::isolated_test(root.clone()).map_err(|_| "business cleanup scope")?;
    paths
        .verify_root_for_current_user()
        .map_err(|_| "business cleanup root identity")?;
    fs::remove_dir_all(&root).map_err(|_| "business owned deployment cleanup")?;
    if root.exists() {
        return Err("business owned deployment remains".into());
    }
    active?;
    println!("TLS_BUSINESS_PASS {{\"physicalNativeDrain\":true,\"sharedWorkers\":2,\"workerJoin\":true,\"hubPending\":0,\"downloadHubPending\":0,\"quarantined\":0,\"setupCleanup\":true,\"approvals\":{approvals},\"heads\":{},\"responses\":{},\"peakRecords\":{}}}", report.heads, report.responses, report.peak_records);
    std::io::stdout()
        .flush()
        .map_err(|_| "business summary flush")?;
    Ok(())
}
