//! Local, explicit fixed-copy opens. One lazy STA owns Shell calls; no caller
//! waits for Shell, an associated application, or worker shutdown.
//! Shell itself uses NOASYNC on its dedicated STA (not ASYNCOK); an in-flight
//! extension may block that thread, so the caller revokes without joining and
//! the composition root enforces its own bounded process-shutdown deadline.
use super::files::LaunchPath;
use fc_domain::{ids::ResourceVersionId, materials::MaterialKind};
use fc_runtime::health::{EffectIntent, EffectLease, HealthGate};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{w, Error, Result, HSTRING, PCWSTR},
    Win32::{
        Foundation::*,
        System::{Registry::*, RemoteDesktop::*, StationsAndDesktops::*, Threading::*},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

pub const MATERIAL_OPEN_SCOPE: u64 = 0xfc00_0017;
const MAX_OUTSTANDING: usize = 2;
const REMEMBERED_REQUESTS: usize = 64;
const RESOURCE_COOLDOWN: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchOrigin {
    CurrentDesktop,
    OwnerWorkbench,
}
/// This callback is trusted application memory only: exact source/version,
/// generation and session checks. It must not perform I/O, KDF, rendering, wait
/// for a lock held across native calls, or dispatch blocking messages.
pub type LaunchRecheck = Arc<dyn Fn() -> bool + Send + Sync>;
pub struct LaunchRequest {
    pub request_id: u64,
    pub version: ResourceVersionId,
    pub path: LaunchPath,
    pub gate: HealthGate,
    pub lease: EffectLease,
    pub origin: LaunchOrigin,
    pub recheck: LaunchRecheck,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitError {
    Stopped,
    Busy,
    Duplicate,
    InvalidCopy,
    ThreadUnavailable,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchOutcome {
    /// The system accepted the request; this never asserts a document was shown.
    Requested,
    AssociationMissing,
    Canceled,
    Failed(i32),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchResult {
    pub request_id: u64,
    pub outcome: LaunchOutcome,
}
struct Shared {
    stopped: AtomicBool,
    finished: AtomicBool,
    outstanding: AtomicUsize,
    accepting: Mutex<bool>,
}
struct Worker {
    sender: SyncSender<LaunchRequest>,
    results: Receiver<LaunchResult>,
    thread: Option<std::thread::JoinHandle<()>>,
}
#[derive(Clone)]
struct ProbeConfig {
    root: PathBuf,
    class: String,
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: uniquely owned registry key opened in this module.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
/// Owns only the exact unique test ProgID. Drop after the diagnostic launcher is
/// finished; it never deletes extension/default association keys.
pub struct ProbeAssociation {
    registry_path: String,
}
impl ProbeAssociation {
    pub fn registry_path(&self) -> &str {
        &self.registry_path
    }
    pub fn remove(&mut self) -> Result<()> {
        let path = HSTRING::from(&self.registry_path);
        // SAFETY: generated exclusive test-key path, never caller supplied.
        let code = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &path) };
        if code == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            code.ok()
        }
    }
}
impl Drop for ProbeAssociation {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}
/// There is no worker until the first accepted submit. At most one request is
/// executing and one is queued, including results not yet consumed by the UI.
/// Drop/stop only revoke and disconnect; they never join a potentially slow STA.
pub struct ShellLauncher {
    worker: Option<Worker>,
    shared: Arc<Shared>,
    recent: VecDeque<(u64, ResourceVersionId, Instant)>,
    probe: Option<ProbeConfig>,
    idle: Duration,
    thread_failed: bool,
}
impl Default for ShellLauncher {
    fn default() -> Self {
        Self::new()
    }
}
impl ShellLauncher {
    pub fn new() -> Self {
        Self {
            worker: None,
            shared: Arc::new(Shared {
                stopped: AtomicBool::new(false),
                finished: AtomicBool::new(true),
                outstanding: AtomicUsize::new(0),
                accepting: Mutex::new(true),
            }),
            recent: VecDeque::new(),
            probe: None,
            idle: Duration::from_secs(30),
            thread_failed: false,
        }
    }
    /// Explicit isolated diagnostics. Registers a unique private ProgID bound
    /// only to this executable's hidden receiver; never changes an extension or
    /// default user association. Keep the returned guard until stop has drained.
    pub fn isolated_probe(root: &Path) -> Result<(Self, ProbeAssociation)> {
        if !root.is_absolute()
            || root
                .file_name()
                .and_then(|n| n.to_str())
                .is_none_or(|n| !n.starts_with("FutureClassroom-Test-") || n.len() > 150)
        {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        use fc_ports::RandomSource;
        let mut random = [0u8; 16];
        crate::native::SystemRandom.fill(&mut random)?;
        let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let class = format!("FutureClassroom.Test.{}.{}", std::process::id(), nonce);
        let registry_path = format!("Software\\Classes\\{class}");
        let path = HSTRING::from(&registry_path);
        let mut key = HKEY::default();
        let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
        // SAFETY: unique current-user private key, with creation disposition
        // checked before any value is written. No default association mutation.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &path,
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                Some(&mut disposition),
            )
            .ok()?;
        }
        let owned = Key(key);
        if disposition != REG_CREATED_NEW_KEY {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let guard = ProbeAssociation { registry_path };
        let executable = std::env::current_exe()?;
        let executable = executable
            .to_str()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if executable.contains(['"', '\0']) {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let command = format!("\"{executable}\" --material-open-probe-receiver \"%1\"");
        let mut subkey = HKEY::default();
        // SAFETY: command child belongs exclusively to the just-created private
        // current-user test class; each opened registry handle has its own RAII.
        unsafe {
            RegCreateKeyExW(
                owned.0,
                w!("shell\\open\\command"),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut subkey,
                None,
            )
            .ok()?;
        }
        let subkey = Key(subkey);
        let bytes: Vec<u8> = command
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        // SAFETY: owned NUL-terminated UTF-16 REG_SZ, fixed private default verb.
        unsafe {
            RegSetValueExW(subkey.0, None, None, REG_SZ, Some(&bytes)).ok()?;
        }
        let mut launcher = Self::new();
        launcher.probe = Some(ProbeConfig {
            root: root.to_owned(),
            class,
        });
        Ok((launcher, guard))
    }
    pub fn submit(&mut self, request: LaunchRequest) -> std::result::Result<(), SubmitError> {
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err(SubmitError::Stopped);
        }
        self.reap_nonblocking()?;
        if request.request_id == 0 || !copy_allowed(request.path.path(), self.probe.as_ref()) {
            return Err(SubmitError::InvalidCopy);
        }
        // Submit and idle retirement share this short memory-only transition;
        // no native Shell call or wait occurs under it. A successful enqueue
        // cannot race a retiring receiver and silently lose the request.
        let shared = self.shared.clone();
        let mut accepting = shared.accepting.lock().map_err(|_| SubmitError::Stopped)?;
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.thread.is_none())
        {
            if shared.outstanding.load(Ordering::Acquire) != 0 {
                return Err(SubmitError::Busy);
            }
            self.worker = None;
            *accepting = true;
        }
        if !*accepting {
            return Err(SubmitError::Busy);
        }
        let now = Instant::now();
        if self.recent.iter().any(|(id, version, when)| {
            *id == request.request_id
                || *version == request.version && now.duration_since(*when) < RESOURCE_COOLDOWN
        }) {
            return Err(SubmitError::Duplicate);
        }
        if self.shared.outstanding.load(Ordering::Acquire) >= MAX_OUTSTANDING {
            return Err(SubmitError::Busy);
        }
        if self.worker.is_none() {
            let (sender, pending) = mpsc::sync_channel(1);
            let (complete, results) = mpsc::sync_channel(MAX_OUTSTANDING);
            let shared = self.shared.clone();
            let probe = self.probe.clone();
            let idle = self.idle;
            shared.finished.store(false, Ordering::Release);
            let thread = match std::thread::Builder::new()
                .name("fc-shell-sta".into())
                .spawn(move || {
                    struct Finished(Arc<Shared>);
                    impl Drop for Finished {
                        fn drop(&mut self) {
                            self.0.finished.store(true, Ordering::Release);
                        }
                    }
                    let _finished = Finished(shared.clone());
                    run(pending, complete, shared, probe, idle);
                }) {
                Ok(thread) => thread,
                Err(_) => {
                    self.shared.finished.store(true, Ordering::Release);
                    return Err(SubmitError::ThreadUnavailable);
                }
            };
            self.worker = Some(Worker {
                sender,
                results,
                thread: Some(thread),
            });
        }
        let id = request.request_id;
        let version = request.version;
        self.shared.outstanding.fetch_add(1, Ordering::AcqRel);
        match self
            .worker
            .as_ref()
            .expect("worker initialized")
            .sender
            .try_send(request)
        {
            Ok(()) => {
                if self.recent.len() == REMEMBERED_REQUESTS {
                    self.recent.pop_front();
                }
                self.recent.push_back((id, version, now));
                Ok(())
            }
            Err(error) => {
                self.shared.outstanding.fetch_sub(1, Ordering::AcqRel);
                Err(match error {
                    TrySendError::Full(_) => SubmitError::Busy,
                    TrySendError::Disconnected(_) => SubmitError::Stopped,
                })
            }
        }
    }
    pub fn take_results(&mut self) -> Vec<LaunchResult> {
        let Some(worker) = &self.worker else {
            return Vec::new();
        };
        let results: Vec<_> = worker.results.try_iter().take(MAX_OUTSTANDING).collect();
        self.shared
            .outstanding
            .fetch_sub(results.len(), Ordering::AcqRel);
        results
    }
    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::Release);
    }
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(|worker| {
            worker
                .thread
                .as_ref()
                .is_none_or(crate::system::thread_is_finished)
        })
    }
    /// Reap only an OS-terminated thread. A Rust closure/packet can finish before
    /// TLS destructors, so JoinHandle::is_finished alone is insufficient. The
    /// zero-timeout native thread query prevents blocking the UI in join.
    /// Unconsumed results stay in their receiver until take_results consumes them.
    pub fn reap_nonblocking(&mut self) -> std::result::Result<bool, SubmitError> {
        if self.thread_failed {
            return Err(SubmitError::ThreadUnavailable);
        }
        let Some(worker) = self.worker.as_mut() else {
            return Ok(true);
        };
        let Some(thread) = worker.thread.as_ref() else {
            return Ok(true);
        };
        if !crate::system::thread_is_finished(thread) {
            return Ok(false);
        }
        let thread = worker.thread.take().expect("finished thread still owned");
        if thread.join().is_err() {
            self.thread_failed = true;
            self.stop();
            return Err(SubmitError::ThreadUnavailable);
        }
        Ok(true)
    }
}
impl Drop for ShellLauncher {
    fn drop(&mut self) {
        self.stop();
        // Dropping the sender/receiver disconnects the detached thread. In-flight
        // Shell may already have been accepted; never replay, kill or wait for it.
    }
}
fn copy_allowed(path: &Path, probe: Option<&ProbeConfig>) -> bool {
    let allowed_kind = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(MaterialKind::from_name)
        .is_some();
    let launch_directory = path.parent().and_then(Path::parent);
    allowed_kind
        && path.is_absolute()
        && launch_directory
            .and_then(Path::file_name)
            .is_some_and(|n| n == "launch-copies")
        && probe.is_none_or(|p| launch_directory == Some(p.root.join("launch-copies").as_path()))
}
fn run(
    pending: Receiver<LaunchRequest>,
    complete: SyncSender<LaunchResult>,
    shared: Arc<Shared>,
    probe: Option<ProbeConfig>,
    idle: Duration,
) {
    let apartment = crate::native::Apartment::new();
    let mut active = Instant::now();
    loop {
        pump_messages();
        let request = match pending.recv_timeout(Duration::from_millis(50)) {
            Ok(request) => request,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if shared.stopped.load(Ordering::Acquire) {
                    break;
                }
                if active.elapsed() >= idle {
                    let Ok(mut accepting) = shared.accepting.lock() else {
                        break;
                    };
                    if shared.outstanding.load(Ordering::Acquire) == 0 {
                        *accepting = false;
                        break;
                    }
                }
                continue;
            }
        };
        active = Instant::now();
        let outcome = if shared.stopped.load(Ordering::Acquire) {
            LaunchOutcome::Canceled
        } else if let Err(error) = &apartment {
            LaunchOutcome::Failed(error.code().0)
        } else {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute(&request, &shared, probe.as_ref())
            })) {
                Ok(outcome) => outcome,
                Err(_) => {
                    request.gate.fault();
                    LaunchOutcome::Failed(E_UNEXPECTED.0)
                }
            }
        };
        // There are <=2 outstanding including undelivered results, so this slot
        // exists unless the UI dropped. Never block the STA on result delivery.
        if complete
            .try_send(LaunchResult {
                request_id: request.request_id,
                outcome,
            })
            .is_err()
        {
            break;
        }
    }
}
/// This STA owns its whole queue; bounded dispatch keeps cancellation observable
/// even if a COM extension posts messages continuously. No cross-thread Send.
fn pump_messages() -> usize {
    let mut count = 0;
    let mut message = MSG::default();
    unsafe {
        while count < 32 && PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if message.message != WM_QUIT {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            count += 1;
        }
    }
    count
}
fn execute(request: &LaunchRequest, shared: &Shared, probe: Option<&ProbeConfig>) -> LaunchOutcome {
    if !copy_allowed(request.path.path(), probe) || shared.stopped.load(Ordering::Acquire) {
        return LaunchOutcome::Canceled;
    }
    let permit = match request.gate.try_start(
        request.lease,
        EffectIntent {
            scope: MATERIAL_OPEN_SCOPE,
            job_id: request.request_id,
        },
    ) {
        Ok(permit) => permit,
        Err(_) => return LaunchOutcome::Canceled,
    };
    let path = HSTRING::from(request.path.path().as_os_str());
    let class = probe.map(|p| HSTRING::from(&p.class));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC
            | SEE_MASK_UNICODE
            | SEE_MASK_FLAG_NO_UI
            | if class.is_some() {
                SEE_MASK_CLASSNAME
            } else {
                0
            },
        lpVerb: w!("open"),
        lpFile: windows::core::PCWSTR(path.as_ptr()),
        // No parameters, cwd, elevation verb, UI HWND, network path or shell cmd.
        lpClass: class
            .as_ref()
            .map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
        nShow: if probe.is_some() {
            SW_HIDE.0
        } else {
            SW_SHOWNORMAL.0
        },
        ..Default::default()
    };
    // Recheck at the actual irreversible call site, after preparing all owned
    // native strings. No application lock/transaction is retained across Shell.
    if shared.stopped.load(Ordering::Acquire)
        || probe.is_none() && !system_allows(request.origin).unwrap_or(false)
        || !(request.recheck)()
        || request.gate.check_permit(&permit).is_err()
    {
        return LaunchOutcome::Canceled;
    }
    // SAFETY: this dedicated initialized STA owns info and all strings through
    // synchronous NOASYNC dispatch. LaunchPath pins file+parents until return.
    // NOCLOSEPROCESS is intentionally absent; no external-process handle/wait.
    match unsafe { ShellExecuteExW(&mut info) } {
        Ok(()) => LaunchOutcome::Requested,
        Err(e)
            if e.code() == ERROR_NO_ASSOCIATION.to_hresult()
                || matches!(info.hInstApp.0 as usize, n if n == SE_ERR_NOASSOC as usize || n == SE_ERR_ASSOCINCOMPLETE as usize) =>
        {
            LaunchOutcome::AssociationMissing
        }
        Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => LaunchOutcome::Canceled,
        Err(e) => LaunchOutcome::Failed(e.code().0),
    }
}
fn system_allows(origin: LaunchOrigin) -> Result<bool> {
    if !crate::native::input_desktop_available()
        || origin == LaunchOrigin::CurrentDesktop
            && crate::native::desktop_foreground() != fc_presentation::Foreground::Desktop
    {
        return Ok(false);
    }
    // SAFETY: owned WTS output is released on every path; pseudo-process/session
    // identifiers are borrowed, never passed to CloseHandle.
    unsafe {
        let mut sid = 0;
        ProcessIdToSessionId(GetCurrentProcessId(), &mut sid)?;
        if sid == 0 {
            return Ok(false);
        }
        let mut ptr = windows::core::PWSTR::null();
        let mut bytes = 0;
        WTSQuerySessionInformationW(None, sid, WTSConnectState, &mut ptr, &mut bytes)?;
        struct Buffer(windows::core::PWSTR);
        impl Drop for Buffer {
            fn drop(&mut self) {
                unsafe { WTSFreeMemory(self.0 .0.cast()) }
            }
        }
        let buffer = Buffer(ptr);
        if bytes < std::mem::size_of::<WTS_CONNECTSTATE_CLASS>() as u32
            || buffer.0.is_null()
            || *buffer.0 .0.cast::<WTS_CONNECTSTATE_CLASS>() != WTSActive
        {
            return Ok(false);
        }
        let desktop = match OpenInputDesktop(DF_ALLOWOTHERACCOUNTHOOK, false, DESKTOP_READOBJECTS) {
            Ok(desktop) => desktop,
            Err(_) => return Ok(false),
        };
        let mut name = [0u16; 64];
        let mut bytes = 0;
        let ok = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            std::mem::size_of_val(&name) as u32,
            Some(&mut bytes),
        )
        .is_ok();
        let _ = CloseDesktop(desktop);
        Ok(ok
            && bytes <= std::mem::size_of_val(&name) as u32
            && name[..8] == [68, 101, 102, 97, 117, 108, 116, 0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::ids::{AuthEpoch, Revision};
    use fc_runtime::authority::AuthorityStore;
    use std::{fs, sync::atomic::AtomicU32};
    struct LogicalFinished(Arc<Shared>);
    impl Drop for LogicalFinished {
        fn drop(&mut self) {
            self.0.finished.store(true, Ordering::Release);
        }
    }
    struct HeldTls {
        entered: SyncSender<()>,
        release: Receiver<()>,
    }
    impl Drop for HeldTls {
        fn drop(&mut self) {
            let _ = self.entered.send(());
            let _ = self.release.recv();
        }
    }
    thread_local! {
        static HELD_TLS: std::cell::RefCell<Option<HeldTls>> = const { std::cell::RefCell::new(None) };
    }
    struct ReleaseTls(Option<SyncSender<()>>);
    impl ReleaseTls {
        fn release(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
    impl Drop for ReleaseTls {
        fn drop(&mut self) {
            self.release();
        }
    }
    fn install_worker(
        launcher: &mut ShellLauncher,
        results: Receiver<LaunchResult>,
        thread: std::thread::JoinHandle<()>,
    ) {
        let (sender, _) = mpsc::sync_channel(1);
        launcher.worker = Some(Worker {
            sender,
            results,
            thread: Some(thread),
        });
    }
    fn wait_finished(launcher: &ShellLauncher) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !launcher.is_finished() {
            assert!(Instant::now() < deadline, "native STA thread end deadline");
            std::thread::yield_now();
        }
    }
    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "FutureClassroom-Test-Shell-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(root.join("launch-copies").join("fixed-version")).unwrap();
            for name in ["one.txt", "two.txt", "bad.exe"] {
                fs::write(
                    root.join("launch-copies").join("fixed-version").join(name),
                    b"own synthetic content",
                )
                .unwrap();
            }
            Self(root)
        }
        fn path(&self, name: &str) -> LaunchPath {
            LaunchPath::open(
                &self
                    .0
                    .join("launch-copies")
                    .join("fixed-version")
                    .join(name),
                100,
            )
            .unwrap()
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("own Shell test cleanup");
        }
    }
    fn gate() -> HealthGate {
        let (mut writer, reader) = AuthorityStore::new((), Revision::new(1), AuthEpoch::new(1));
        writer.initialize().unwrap();
        reader.gate()
    }
    fn request(tree: &Tree, id: u64, gate: &HealthGate, recheck: LaunchRecheck) -> LaunchRequest {
        LaunchRequest {
            request_id: id,
            version: ResourceVersionId::new(id).unwrap(),
            path: tree.path(if id == 1 { "one.txt" } else { "two.txt" }),
            gate: gate.clone(),
            lease: gate
                .lease_for_authorized_scope(
                    MATERIAL_OPEN_SCOPE,
                    Instant::now() + Duration::from_secs(10),
                )
                .unwrap(),
            origin: LaunchOrigin::CurrentDesktop,
            recheck,
        }
    }
    fn results(launcher: &mut ShellLauncher, count: usize) -> Vec<LaunchResult> {
        let until = Instant::now() + Duration::from_secs(3);
        let mut results = Vec::new();
        while results.len() < count {
            results.extend(launcher.take_results());
            assert!(Instant::now() < until, "STA result deadline");
            std::thread::sleep(Duration::from_millis(2));
        }
        results
    }
    fn finish(launcher: &ShellLauncher) {
        launcher.stop();
        let until = Instant::now() + Duration::from_secs(3);
        while !launcher.is_finished() {
            assert!(Instant::now() < until, "STA shutdown deadline");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn registry_exists(path: &str) -> bool {
        let mut key = HKEY::default();
        let code = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(path),
                None,
                KEY_READ,
                &mut key,
            )
        };
        if code == ERROR_SUCCESS {
            drop(Key(key));
            true
        } else {
            assert_eq!(code, ERROR_FILE_NOT_FOUND);
            false
        }
    }
    #[test]
    fn sta_queue_results_and_shutdown_stay_bounded_and_private_association_is_removed() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let tree = Tree::new();
        let (mut launcher, mut association) = ShellLauncher::isolated_probe(&tree.0).unwrap();
        assert!(
            launcher.is_finished(),
            "lazy, no thread before first submit"
        );
        let registry_path = association.registry_path().to_owned();
        assert!(registry_exists(&registry_path));
        let gate = gate();
        let (entered, started) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let wait = std::sync::Mutex::new(wait);
        // Deliberate injected barrier only; production rechecks never wait.
        let blocking: LaunchRecheck = Arc::new(move || {
            entered.send(()).unwrap();
            wait.lock().unwrap().recv().unwrap();
            false
        });
        launcher.submit(request(&tree, 1, &gate, blocking)).unwrap();
        started.recv_timeout(Duration::from_secs(3)).unwrap();
        launcher
            .submit(request(&tree, 2, &gate, Arc::new(|| false)))
            .unwrap();
        assert_eq!(
            launcher.submit(request(&tree, 3, &gate, Arc::new(|| false))),
            Err(SubmitError::Busy)
        );
        assert_eq!(
            launcher.submit(request(&tree, 1, &gate, Arc::new(|| false))),
            Err(SubmitError::Duplicate)
        );
        let begin = Instant::now();
        launcher.stop();
        assert!(begin.elapsed() < Duration::from_millis(50));
        assert!(
            !launcher.is_finished(),
            "stop must not join the injected in-flight call"
        );
        release.send(()).unwrap();
        let result = results(&mut launcher, 2);
        assert_eq!(
            result.iter().map(|r| r.request_id).collect::<Vec<_>>(),
            [1, 2]
        );
        assert!(result.iter().all(|r| r.outcome == LaunchOutcome::Canceled));
        finish(&launcher);
        assert_eq!(
            launcher.submit(request(&tree, 4, &gate, Arc::new(|| true))),
            Err(SubmitError::Stopped)
        );
        drop(launcher);
        association.remove().unwrap();
        assert!(!registry_exists(&registry_path));
    }
    #[test]
    fn stale_scope_expiry_and_memory_guard_cancel_before_any_native_open() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let tree = Tree::new();
        let (mut launcher, association) = ShellLauncher::isolated_probe(&tree.0).unwrap();
        let checks = Arc::new(AtomicU32::new(0));
        let count = checks.clone();
        let recheck: LaunchRecheck = Arc::new(move || {
            count.fetch_add(1, Ordering::AcqRel);
            false
        });
        let gate = gate();
        let mut wrong_scope = request(&tree, 1, &gate, recheck.clone());
        wrong_scope.lease.scope += 1;
        launcher.submit(wrong_scope).unwrap();
        assert_eq!(
            results(&mut launcher, 1)[0].outcome,
            LaunchOutcome::Canceled
        );
        let mut expired = request(&tree, 2, &gate, recheck.clone());
        expired.lease.expires_at = Instant::now();
        launcher.submit(expired).unwrap();
        assert_eq!(
            results(&mut launcher, 1)[0].outcome,
            LaunchOutcome::Canceled
        );
        assert_eq!(checks.load(Ordering::Acquire), 0);
        launcher.submit(request(&tree, 3, &gate, recheck)).unwrap();
        assert_eq!(
            results(&mut launcher, 1)[0].outcome,
            LaunchOutcome::Canceled
        );
        assert_eq!(checks.load(Ordering::Acquire), 1);
        let mut cooldown = request(&tree, 4, &gate, Arc::new(|| false));
        cooldown.version = ResourceVersionId::new(3).unwrap();
        assert_eq!(launcher.submit(cooldown), Err(SubmitError::Duplicate));
        let mut invalid = request(&tree, 5, &gate, Arc::new(|| false));
        invalid.path = tree.path("bad.exe");
        assert_eq!(launcher.submit(invalid), Err(SubmitError::InvalidCopy));
        let stale = request(
            &tree,
            6,
            &gate,
            Arc::new(|| panic!("stale callback must not run")),
        );
        gate.stop();
        launcher.submit(stale).unwrap();
        assert_eq!(
            results(&mut launcher, 1)[0].outcome,
            LaunchOutcome::Canceled
        );
        finish(&launcher);
        let path = association.registry_path().to_owned();
        drop(launcher);
        drop(association);
        assert!(!registry_exists(&path));
    }
    #[test]
    fn recheck_panic_faults_gate_and_production_owner_cannot_open_from_noninteractive_session() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let tree = Tree::new();
        let (mut launcher, association) = ShellLauncher::isolated_probe(&tree.0).unwrap();
        let gate = gate();
        launcher
            .submit(request(
                &tree,
                1,
                &gate,
                Arc::new(|| panic!("injected memory-guard panic")),
            ))
            .unwrap();
        assert_eq!(
            results(&mut launcher, 1)[0].outcome,
            LaunchOutcome::Failed(E_UNEXPECTED.0)
        );
        assert_eq!(gate.status().health, fc_runtime::health::Health::Faulted);
        finish(&launcher);
        drop(launcher);
        drop(association);
        let mut session = 0;
        unsafe {
            ProcessIdToSessionId(GetCurrentProcessId(), &mut session).unwrap();
        }
        if session == 0 {
            assert!(!system_allows(LaunchOrigin::CurrentDesktop).unwrap());
            assert!(!system_allows(LaunchOrigin::OwnerWorkbench).unwrap());
        }
    }
    #[test]
    fn lazy_sta_retires_and_restarts_without_losing_a_queued_request() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let tree = Tree::new();
        let (mut launcher, association) = ShellLauncher::isolated_probe(&tree.0).unwrap();
        launcher.idle = Duration::from_millis(5);
        let gate = gate();
        launcher
            .submit(request(&tree, 1, &gate, Arc::new(|| false)))
            .unwrap();
        assert_eq!(
            results(&mut launcher, 1)[0].outcome,
            LaunchOutcome::Canceled
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while !launcher.is_finished() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        launcher
            .submit(request(&tree, 2, &gate, Arc::new(|| false)))
            .unwrap();
        assert_eq!(results(&mut launcher, 1)[0].request_id, 2);
        finish(&launcher);
        drop(launcher);
        drop(association);
        // A separate owned queue proves that one pump drains no more than 32.
        std::thread::spawn(|| unsafe {
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            for _ in 0..64 {
                PostThreadMessageW(GetCurrentThreadId(), WM_APP + 99, WPARAM(0), LPARAM(0))
                    .unwrap();
            }
            assert_eq!(pump_messages(), 32);
            assert_eq!(pump_messages(), 32);
            assert_eq!(pump_messages(), 0);
        })
        .join()
        .unwrap();
    }
    #[test]
    fn logical_finish_and_rust_packet_do_not_hide_a_held_tls_destructor() {
        let tree = Tree::new();
        let mut launcher = ShellLauncher::new();
        launcher.shared.finished.store(false, Ordering::Release);
        *launcher.shared.accepting.lock().unwrap() = false;
        let shared = launcher.shared.clone();
        let (entered, in_tls) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let mut release = ReleaseTls(Some(release));
        let (complete, results) = mpsc::sync_channel(MAX_OUTSTANDING);
        let thread = std::thread::spawn(move || {
            let _finished = LogicalFinished(shared);
            HELD_TLS.with(|slot| {
                *slot.borrow_mut() = Some(HeldTls {
                    entered,
                    release: wait,
                })
            });
            drop(complete);
        });
        let id = thread.thread().id();
        install_worker(&mut launcher, results, thread);
        in_tls.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(launcher.shared.finished.load(Ordering::Acquire));
        assert!(
            launcher
                .worker
                .as_ref()
                .unwrap()
                .thread
                .as_ref()
                .unwrap()
                .is_finished(),
            "the Rust packet already finished while TLS is still executing"
        );
        assert!(!launcher.is_finished());
        let before = Instant::now();
        assert_eq!(launcher.reap_nonblocking(), Ok(false));
        assert!(before.elapsed() < Duration::from_millis(50));
        let gate = gate();
        assert_eq!(
            launcher.submit(request(&tree, 1, &gate, Arc::new(|| false))),
            Err(SubmitError::Busy)
        );
        assert_eq!(
            launcher
                .worker
                .as_ref()
                .unwrap()
                .thread
                .as_ref()
                .unwrap()
                .thread()
                .id(),
            id
        );
        release.release();
        wait_finished(&launcher);
        assert_eq!(launcher.reap_nonblocking(), Ok(true));
        assert!(launcher.worker.as_ref().unwrap().thread.is_none());
    }
    #[test]
    fn native_reap_keeps_unconsumed_results_and_blocks_replacement_until_consumed() {
        let tree = Tree::new();
        let mut launcher = ShellLauncher::new();
        let shared = launcher.shared.clone();
        shared.finished.store(false, Ordering::Release);
        shared.outstanding.store(1, Ordering::Release);
        let (complete, results) = mpsc::sync_channel(MAX_OUTSTANDING);
        let thread = std::thread::spawn(move || {
            let _finished = LogicalFinished(shared);
            complete
                .send(LaunchResult {
                    request_id: 9,
                    outcome: LaunchOutcome::Canceled,
                })
                .unwrap();
        });
        install_worker(&mut launcher, results, thread);
        wait_finished(&launcher);
        assert_eq!(launcher.reap_nonblocking(), Ok(true));
        let gate = gate();
        assert_eq!(
            launcher.submit(request(&tree, 1, &gate, Arc::new(|| false))),
            Err(SubmitError::Busy)
        );
        assert_eq!(launcher.shared.outstanding.load(Ordering::Acquire), 1);
        assert_eq!(
            launcher.take_results(),
            [LaunchResult {
                request_id: 9,
                outcome: LaunchOutcome::Canceled
            }]
        );
        assert_eq!(launcher.shared.outstanding.load(Ordering::Acquire), 0);
        assert_eq!(launcher.reap_nonblocking(), Ok(true));
    }
    #[test]
    fn outer_thread_panic_is_sticky_and_stops_new_admission_after_reap() {
        let tree = Tree::new();
        let mut launcher = ShellLauncher::new();
        let shared = launcher.shared.clone();
        shared.finished.store(false, Ordering::Release);
        let (_, results) = mpsc::sync_channel(MAX_OUTSTANDING);
        let thread = std::thread::spawn(move || {
            let _finished = LogicalFinished(shared);
            panic!("injected outer STA failure, without any Shell call");
        });
        install_worker(&mut launcher, results, thread);
        wait_finished(&launcher);
        assert_eq!(
            launcher.reap_nonblocking(),
            Err(SubmitError::ThreadUnavailable)
        );
        assert_eq!(
            launcher.reap_nonblocking(),
            Err(SubmitError::ThreadUnavailable)
        );
        let gate = gate();
        assert_eq!(
            launcher.submit(request(&tree, 1, &gate, Arc::new(|| false))),
            Err(SubmitError::Stopped)
        );
    }
}
