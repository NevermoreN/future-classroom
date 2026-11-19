//! One UI-thread hidden top-level host, tray, coalesced system hints and shutdown.
//! No writer/network joins, secret material, or privileged settings live here.
use crate::host_retry::{HostRetry, RetryDecision};
use std::{
    cell::{Cell, RefCell},
    fs::{File, OpenOptions},
    io::{Read, Write},
    marker::PhantomData,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    rc::Rc,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Instant,
};
use windows::{
    core::{w, Error, Result, HSTRING},
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{
            LibraryLoader::*,
            RemoteDesktop::*,
            Threading::{
                GetCurrentProcessId, OpenEventW, WaitForSingleObject, SYNCHRONIZATION_SYNCHRONIZE,
            },
        },
        UI::{Accessibility::*, Shell::*, WindowsAndMessaging::*},
    },
};

const TIMER: usize = 0xfc01;
const TRAY_MESSAGE: u32 = WM_APP + 41;
const FOREGROUND_MESSAGE: u32 = WM_APP + 42;
const OPEN_INSTANCE_MESSAGE: u32 = WM_APP + 43;

fn host_error(stage: &str, error: Error) -> Error {
    Error::new(
        error.code(),
        format!("{stage}失败（HRESULT {:#010X}）", error.code().0),
    )
}

fn host_io_error(stage: &str, error: std::io::Error) -> Error {
    let code = error
        .raw_os_error()
        .map_or(E_FAIL, |v| windows::core::HRESULT::from_win32(v as u32));
    host_error(stage, Error::from_hresult(code))
}

/// Caller holds the original InstanceGuard and checked runtime directory.
/// A crash or a restored disk image can retain a record after its OS handles
/// disappeared. Never use its PID to terminate a process or infer ownership.
fn create_instance_owner(root: &Path) -> Result<File> {
    let path = root.join("runtime/instance.owner");
    let create = || {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .access_mode(GENERIC_WRITE.0 | DELETE.0)
            .share_mode(FILE_SHARE_READ.0)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(&path)
    };
    match create() {
        Ok(file) => return Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(host_io_error("创建实例路由记录", error)),
    }
    // Exclusive open refuses a live publisher or reader. Deletion is scheduled
    // only AFTER validating this exact file's shape and contents.
    let mut stale = OpenOptions::new()
        .read(true)
        .access_mode(GENERIC_READ.0 | DELETE.0)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(&path)
        .map_err(|e| host_io_error("独占检查残留实例路由记录", e))?;
    let handle = HANDLE(stale.as_raw_handle());
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: stale owns the exclusive live file handle; no path is followed.
    unsafe { GetFileInformationByHandle(handle, &mut info) }
        .map_err(|e| host_error("检查残留实例路由记录属性", e))?;
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0) != 0
        || info.nNumberOfLinks != 1
        || info.nFileSizeHigh != 0
        || info.nFileSizeLow != 12
    {
        return Err(host_error(
            "残留实例路由记录格式不符，未删除",
            Error::from_hresult(E_INVALIDARG),
        ));
    }
    let mut record = [0u8; 12];
    stale
        .read_exact(&mut record)
        .map_err(|e| host_io_error("读取残留实例路由记录", e))?;
    if &record[..4] != b"FCI1" {
        return Err(host_error(
            "残留实例路由记录内容不符，未删除",
            Error::from_hresult(E_INVALIDARG),
        ));
    }
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the validated, exclusively held file has DELETE access. Delete
    // by handle, then CREATE_NEW; never truncate or overwrite a replacement.
    unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            &disposition as *const _ as _,
            std::mem::size_of_val(&disposition) as u32,
        )
    }
    .map_err(|e| host_error("清理残留实例路由记录", e))?;
    drop(stale);
    create().map_err(|e| host_io_error("重建实例路由记录", e))
}
thread_local! {
    // Out-of-context callbacks are delivered on the hook owner's UI thread.
    // Only an HWND value is retained; there is no cross-thread State pointer.
    #[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "nightly-2026-10-29 rebuilt Win7 std wraps this const initializer in a non-const function"))]
    static FOREGROUND_TARGET: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    #[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "nightly-2026-10-29 rebuilt Win7 std wraps this const initializer in a non-const function"))]
    static FOREGROUND_PENDING: Cell<bool> = const { Cell::new(false) };
}
const STOP: u32 = 1;
const FAULT: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostHints(u32);
impl HostHints {
    pub const TICK: Self = Self(1);
    pub const TIME_CHANGED: Self = Self(2);
    pub const DISPLAY_CHANGED: Self = Self(4);
    pub const POWER_CHANGED: Self = Self(8);
    pub const SESSION_CHANGED: Self = Self(16);
    pub const SESSION_LOCKED: Self = Self(32);
    pub const SHUTDOWN: Self = Self(64);
    pub const OPEN_STATUS: Self = Self(128);
    pub const EMERGENCY_STOP_AUDIO: Self = Self(256);
    /// Explicit tray exit: bootstrap starts ordinary graceful shutdown without login.
    pub const EXIT_REQUESTED: Self = Self(512);
    pub const TASKBAR_RECREATED: Self = Self(1024);
    const TRAY_CONTEXT: Self = Self(2048);
    pub const FOREGROUND_CHANGED: Self = Self(4096);
    pub const OPEN_SETTINGS: Self = Self(65536);
    pub const OPEN_QUICK_CONNECT: Self = Self(131072);
    pub const OPEN_ABOUT: Self = Self(262144);
    /// Negative read-only sample at native message dispatch. The original OS
    /// event-to-dispatch latency is not measured or eliminated by this fence.
    pub const FOREGROUND_UNAVAILABLE: Self = Self(32768);
    pub const SUSPENDING: Self = Self(8192);
    pub const RESUMED: Self = Self(16384);
    pub fn contains(self, hint: Self) -> bool {
        self.0 & hint.0 != 0
    }
}

/// Independent atomic control channel: ordinary mailbox capacity cannot block it.
#[derive(Clone)]
pub struct HostControl(Arc<AtomicU32>);
impl HostControl {
    /// Composition root calls after graceful shutdown, or during OS shutdown.
    pub fn stop(&self) {
        self.0.fetch_or(STOP, Ordering::Release);
    }
    pub fn fault(&self) {
        self.0.fetch_or(FAULT | STOP, Ordering::Release);
    }
}

pub type ControlCallback = Arc<dyn Fn(HostHints) + Send + Sync>;

/// Audio needs the process's active, unlocked interactive session, independently
/// of which application is foreground. No desktop switch or input is performed.
pub fn interactive_session_available() -> bool {
    use windows::Win32::System::{StationsAndDesktops::*, Threading::*};
    // SAFETY: WTS owns its output buffer (freed below); the returned desktop
    // handle is queried synchronously into a bounded buffer and always closed.
    unsafe {
        let mut sid = 0;
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut sid).is_err() || sid == 0 {
            return false;
        }
        let mut ptr = windows::core::PWSTR::null();
        let mut bytes = 0;
        if WTSQuerySessionInformationW(None, sid, WTSConnectState, &mut ptr, &mut bytes).is_err() {
            if !ptr.is_null() {
                WTSFreeMemory(ptr.0.cast());
            }
            return false;
        }
        let active = !ptr.is_null()
            && bytes as usize >= std::mem::size_of::<WTS_CONNECTSTATE_CLASS>()
            && *ptr.0.cast::<WTS_CONNECTSTATE_CLASS>() == WTSActive;
        if !ptr.is_null() {
            WTSFreeMemory(ptr.0.cast());
        }
        if !active {
            return false;
        }
        let desktop = match OpenInputDesktop(DF_ALLOWOTHERACCOUNTHOOK, false, DESKTOP_READOBJECTS) {
            Ok(d) => d,
            Err(_) => return false,
        };
        let mut name = [0u16; 64];
        let mut needed = 0;
        let valid = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            std::mem::size_of_val(&name) as u32,
            Some(&mut needed),
        )
        .is_ok();
        let closed = CloseDesktop(desktop).is_ok();
        valid && closed && needed == 16 && name[..8] == [68, 101, 102, 97, 117, 108, 116, 0]
    }
}

struct State {
    hints: AtomicU32,
    control: HostControl,
    taskbar_recreated: u32,
    control_callback: RefCell<Option<ControlCallback>>,
}
impl State {
    fn control_hint(&self, hint: HostHints) {
        self.hint(hint);
        let callback = match self.control_callback.try_borrow() {
            Ok(value) => value.clone(),
            Err(_) => {
                self.control.fault();
                unsafe { PostQuitMessage(1) };
                return;
            }
        };
        if let Some(callback) = callback {
            if catch_unwind(AssertUnwindSafe(|| callback(hint))).is_err() {
                self.control.fault();
                unsafe { PostQuitMessage(1) }
            }
        }
    }
    fn foreground_hint(&self, allowed: bool) {
        self.hint(HostHints::FOREGROUND_CHANGED);
        if !allowed {
            self.control_hint(HostHints::FOREGROUND_UNAVAILABLE);
        }
    }
    fn hint(&self, value: HostHints) {
        self.hints.fetch_or(value.0, Ordering::Release);
    }
}

/// !Send / !Sync: HWND and all native registration lifetimes belong to creator.
pub struct HiddenHost {
    hwnd: HWND,
    state: Box<State>,
    tray_requested: bool,
    tray: Cell<bool>,
    session_registered: Cell<bool>,
    session_ready_event: Cell<HANDLE>,
    retry: RefCell<Option<HostRetry>>,
    retry_error: RefCell<Option<Error>>,
    class: u16,
    instance: HINSTANCE,
    foreground_hook: HWINEVENTHOOK,
    instance_owner: Option<File>,
    idle_interval: u32,
    background_pending: Cell<bool>,
    _thread: PhantomData<Rc<()>>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum InstanceRequest {
    Requested,
    OtherSession(u32),
}
impl HiddenHost {
    /// A tray is required in normal interactive operation. Headless tests can
    /// disable it explicitly; that does not count as a real tray acceptance test.
    pub fn new(enable_tray: bool) -> Result<Self> {
        Self::with_interval(enable_tray, 1000)
    }
    /// Publish only while the caller holds this root's original InstanceGuard.
    /// The readable owner record is deleted by the OS when this host closes,
    /// including process termination. It is routing information, not authority.
    pub fn for_instance(root: &Path, enable_tray: bool) -> Result<Self> {
        if !root.is_absolute() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let mut host = Self::new(enable_tray)?;
        let title = HSTRING::from(root.join("runtime/instance.lock").as_os_str());
        let pid = unsafe { GetCurrentProcessId() };
        let mut session = 0;
        unsafe {
            ProcessIdToSessionId(pid, &mut session).map_err(|e| host_error("读取启动会话", e))?;
            SetWindowTextW(host.hwnd, &title).map_err(|e| host_error("设置实例窗口标识", e))?;
        }
        let mut owner = create_instance_owner(root)?;
        let mut record = [0u8; 12];
        record[..4].copy_from_slice(b"FCI1");
        record[4..8].copy_from_slice(&pid.to_le_bytes());
        record[8..].copy_from_slice(&session.to_le_bytes());
        owner
            .write_all(&record)
            .map_err(|e| host_io_error("写入实例路由记录", e))?;
        host.instance_owner = Some(owner);
        Ok(host)
    }
    /// Called only after the root's lock is busy. No data connection is opened,
    /// and no message is sent to another Windows session or another data root.
    pub fn request_existing(root: &Path) -> Result<InstanceRequest> {
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(root.join("runtime/instance.owner"))
            .map_err(|_| Error::from_hresult(E_FAIL))?;
        let metadata = file.metadata().map_err(|_| Error::from_hresult(E_FAIL))?;
        if !metadata.is_file()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || metadata.len() != 12
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let mut record = [0u8; 12];
        file.read_exact(&mut record)
            .map_err(|_| Error::from_hresult(E_FAIL))?;
        if &record[..4] != b"FCI1" {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let pid = u32::from_le_bytes(record[4..8].try_into().unwrap());
        let session = u32::from_le_bytes(record[8..].try_into().unwrap());
        let mut observed = 0;
        let mut current = 0;
        // SAFETY: scalar process/session identities only. FindWindow searches
        // this desktop; the matching title binds the original data root.
        unsafe {
            ProcessIdToSessionId(pid, &mut observed)?;
            if session != observed {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            ProcessIdToSessionId(GetCurrentProcessId(), &mut current)?;
            if session != current {
                return Ok(InstanceRequest::OtherSession(session));
            }
            let title = HSTRING::from(root.join("runtime/instance.lock").as_os_str());
            let hwnd = FindWindowW(w!("FutureClassroom.HiddenHost.v1"), &title)?;
            let mut actual_pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut actual_pid));
            if actual_pid != pid {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            PostMessageW(Some(hwnd), OPEN_INSTANCE_MESSAGE, WPARAM(0), LPARAM(0))?;
        }
        Ok(InstanceRequest::Requested)
    }
    /// Pace the explicitly isolated application diagnostic without waiting a
    /// production second for each synthetic UI step or worker completion.
    pub fn for_isolated_probe() -> Result<Self> {
        Self::with_interval(false, 16)
    }
    fn with_interval(enable_tray: bool, interval_millis: u32) -> Result<Self> {
        // SAFETY: module pseudohandle retained only for same-process class lifetime.
        let instance = HINSTANCE(
            unsafe { GetModuleHandleW(None) }
                .map_err(|e| host_error("读取程序模块", e))?
                .0,
        );
        let class_name = w!("FutureClassroom.HiddenHost.v1");
        let class = WNDCLASSW {
            hInstance: instance,
            lpszClassName: class_name,
            lpfnWndProc: Some(window_proc),
            ..Default::default()
        };
        // SAFETY: static class name and correct ABI callback, alive until unregister.
        let atom = unsafe { RegisterClassW(&class) };
        if atom == 0 {
            return Err(host_error("注册宿主窗口类", Error::from_thread()));
        }
        let mut host = Self {
            hwnd: HWND::default(),
            state: Box::new(State {
                hints: AtomicU32::new(0),
                control_callback: RefCell::new(None),
                control: HostControl(Arc::new(AtomicU32::new(0))),
                // SAFETY: fixed application-known registered message name.
                taskbar_recreated: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            }),
            tray_requested: enable_tray,
            tray: Cell::new(false),
            session_registered: Cell::new(false),
            session_ready_event: Cell::new(HANDLE::default()),
            retry: RefCell::new(Some(HostRetry::new(Instant::now()))),
            retry_error: RefCell::new(None),
            class: atom,
            instance,
            foreground_hook: HWINEVENTHOOK::default(),
            instance_owner: None,
            idle_interval: interval_millis,
            background_pending: Cell::new(false),
            _thread: PhantomData,
        };
        if host.state.taskbar_recreated == 0 {
            return Err(host_error("注册任务栏恢复消息", Error::from_thread()));
        }
        // SAFETY: Box pointee never moves and outlives the HWND. WM_NCDESTROY clears
        // userdata before Box release. Top-level hidden window receives broadcasts.
        host.hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name,
                w!("未来教室宿主"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                Some((&raw const *host.state).cast()),
            )
            .map_err(|e| host_error("创建隐藏宿主窗口", e))?
        };
        FOREGROUND_TARGET.with(|target| target.set(host.hwnd));
        FOREGROUND_PENDING.with(|pending| pending.set(false));
        // SAFETY: callback is out-of-context on this UI thread and only posts a
        // coalesced hint. Own-process settings events must not be skipped.
        host.foreground_hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if host.foreground_hook.is_invalid() {
            return Err(host_error("注册前台窗口通知", Error::from_thread()));
        }
        // SAFETY: window-owned timer has no callback/raw pointers; messages coalesce.
        if unsafe { SetTimer(Some(host.hwnd), TIMER, interval_millis, None) } == 0 {
            return Err(host_error("创建宿主定时器", Error::from_thread()));
        }
        host.advance_readiness()?;
        Ok(host)
    }

    fn is_ready(&self) -> bool {
        self.session_registered.get() && (!self.tray_requested || self.tray.get())
    }

    /// Keep one host and one UI timer while Windows finishes logon. No business
    /// state may be initialized until this succeeds. False is ordinary cancellation.
    pub fn wait_until_ready(&self) -> Result<bool> {
        loop {
            let control = self.state.control.0.load(Ordering::Acquire);
            if control & FAULT != 0 {
                return Err(host_error(
                    "等待系统启动",
                    Error::from_hresult(E_UNEXPECTED),
                ));
            }
            let pending = HostHints(self.state.hints.load(Ordering::Acquire));
            if control & STOP != 0
                || pending.contains(HostHints::SHUTDOWN)
                || pending.contains(HostHints::EXIT_REQUESTED)
            {
                return Ok(false);
            }
            self.advance_readiness()?;
            if self.is_ready() {
                return Ok(true);
            }
            let mut message = MSG::default();
            // SAFETY: this UI thread dispatches its original windows/timer only;
            // no nested business callbacks or background worker is started.
            let status = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
            if status == -1 {
                return Err(host_error("等待启动消息", Error::from_thread()));
            }
            if status == 0 {
                return if message.wParam.0 == 0 {
                    Ok(false)
                } else {
                    Err(host_error(
                        "等待启动消息",
                        Error::from_hresult(E_UNEXPECTED),
                    ))
                };
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            // Retain OPEN_STATUS and safety hints for normal initialization.
            let pending = self.state.hints.fetch_and(
                !(HostHints::TASKBAR_RECREATED.0 | HostHints::TRAY_CONTEXT.0),
                Ordering::AcqRel,
            );
            if pending & HostHints::TASKBAR_RECREATED.0 != 0 {
                self.taskbar_recreated();
            }
            if pending & HostHints::TRAY_CONTEXT.0 != 0
                && pending & (HostHints::SHUTDOWN.0 | HostHints::EXIT_REQUESTED.0) == 0
            {
                self.tray_menu()?;
            }
        }
    }

    fn taskbar_recreated(&self) {
        if self.tray_requested {
            self.tray.set(false);
            // Repeated broadcasts during the same outage must not extend it.
            self.retry
                .borrow_mut()
                .get_or_insert_with(|| HostRetry::new(Instant::now()));
        }
    }

    fn advance_readiness(&self) -> Result<()> {
        if self.is_ready() {
            self.retry.borrow_mut().take();
            self.retry_error.borrow_mut().take();
            return Ok(());
        }
        let (decision, attempts) = {
            let mut retry = self.retry.borrow_mut();
            let retry = retry.get_or_insert_with(|| HostRetry::new(Instant::now()));
            (retry.decide(Instant::now()), retry.attempts)
        };
        match decision {
            RetryDecision::Wait => return Ok(()),
            RetryDecision::Expired => {
                let last = self.retry_error.borrow();
                let code = last.as_ref().map_or(E_FAIL, Error::code);
                return Err(Error::new(
                    code,
                    format!(
                        "等待 Windows 会话服务或任务栏就绪已超时（60秒，尝试{attempts}次）；{}",
                        last.as_ref().map(ToString::to_string).unwrap_or_default()
                    ),
                ));
            }
            RetryDecision::Attempt => {}
        }
        if !self.session_registered.get() {
            let event = self.session_ready_event.get();
            // The readiness event is advisory. If it is unavailable, probe WTS
            // once per second instead; never change a Windows service setting.
            let event_pending =
                !event.is_invalid() && unsafe { WaitForSingleObject(event, 0) } == WAIT_TIMEOUT;
            if !event_pending {
                match unsafe { WTSRegisterSessionNotification(self.hwnd, NOTIFY_FOR_THIS_SESSION) }
                {
                    Ok(()) => self.session_registered.set(true),
                    Err(e)
                        if e.code()
                            // RPC_S_INVALID_BINDING (1702), documented by WTS.
                            == windows::core::HRESULT::from_win32(1702) =>
                    {
                        *self.retry_error.borrow_mut() =
                            Some(host_error("注册会话通知，系统服务尚未就绪", e));
                        if event.is_invalid() {
                            match unsafe {
                                OpenEventW(
                                    SYNCHRONIZATION_SYNCHRONIZE,
                                    false,
                                    w!("Global\\TermSrvReadyEvent"),
                                )
                            } {
                                Ok(event) => self.session_ready_event.set(event),
                                Err(event_error) => {
                                    *self.retry_error.borrow_mut() = Some(Error::new(
                                        windows::core::HRESULT::from_win32(1702),
                                        format!("注册会话通知失败（RPC 1702）；就绪事件不可用（HRESULT {:#010X}），已改为定时重试", event_error.code().0),
                                    ));
                                }
                            }
                        }
                    }
                    Err(e) => return Err(host_error("注册会话通知", e)),
                }
            }
        }
        if self.tray_requested && !self.tray.get() {
            if self.add_tray()? {
                self.tray.set(true);
            } else if self.session_registered.get() {
                *self.retry_error.borrow_mut() = Some(host_error(
                    "注册托盘图标，任务栏暂不可用",
                    Error::from_hresult(E_FAIL),
                ));
            }
        }
        if self.is_ready() {
            self.retry.borrow_mut().take();
            self.retry_error.borrow_mut().take();
        }
        Ok(())
    }
    /// Invoked synchronously by control-message dispatch, including native modal
    /// nested pumps. The trusted callback must only revoke/clear in-memory state:
    /// no database, KDF, rendering, blocking channel send or worker join is allowed.
    /// A short UI-only borrow is released before calling it; panic faults the host.
    pub fn set_control_callback(&self, callback: ControlCallback) -> Result<()> {
        *self
            .state
            .control_callback
            .try_borrow_mut()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))? = Some(callback);
        Ok(())
    }
    pub fn controller(&self) -> HostControl {
        self.state.control.clone()
    }
    /// The original window timer paces bounded background work while an IO
    /// owner is active. Idle hosts retain their original one-second cadence;
    /// no additional timer, thread, queue, or foreground window is introduced.
    pub fn set_background_work_pending(&self, pending: bool) -> Result<()> {
        if self.background_pending.get() == pending {
            return Ok(());
        }
        let interval = if pending {
            self.idle_interval.min(20)
        } else {
            self.idle_interval
        };
        // SAFETY: this UI thread owns hwnd and the existing TIMER identifier.
        // Replacing its interval retains the single coalescing WM_TIMER source.
        if unsafe { SetTimer(Some(self.hwnd), TIMER, interval, None) } == 0 {
            return Err(Error::from_thread());
        }
        self.background_pending.set(pending);
        Ok(())
    }
    /// Callback runs on UI thread after dispatch, must remain bounded/nonblocking.
    /// Background shutdown is coordinated by bootstrap, never joined by this loop.
    pub fn run(self, mut on_hints: impl FnMut(HostHints) -> Result<()>) -> Result<()> {
        self.run_with_host(|hints, _| on_hints(hints))
    }
    /// The callback may use this host's tray without retaining a native handle.
    pub fn run_with_host(
        self,
        mut on_hints: impl FnMut(HostHints, &Self) -> Result<()>,
    ) -> Result<()> {
        if !self.wait_until_ready()? {
            return Ok(());
        }
        let mut recovery_error = None;
        let mut message = MSG::default();
        loop {
            let control = self.state.control.0.load(Ordering::Acquire);
            if control & FAULT != 0 {
                return Err(Error::from_hresult(E_UNEXPECTED));
            }
            if control & STOP != 0 {
                return recovery_error.map_or(Ok(()), Err);
            }
            // SAFETY: MSG is exclusively owned; no filter means all this thread's UI
            // windows are dispatched. No HWND/payload pointer crosses threads.
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
            if result == -1 {
                return Err(Error::from_thread());
            }
            if result == 0 {
                return if message.wParam.0 == 0 {
                    Ok(())
                } else {
                    Err(Error::from_hresult(E_UNEXPECTED))
                };
            }
            // SAFETY: message produced by GetMessage; window procedures handle ABI panics.
            unsafe {
                if !crate::workbench::dialog_message(&message) {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            let mut hints = HostHints(self.state.hints.swap(0, Ordering::AcqRel));
            if hints.contains(HostHints::TASKBAR_RECREATED) {
                self.taskbar_recreated();
            }
            // Do not return a tray failure across active writer/audio ownership.
            // Revoke through the original callback, then drain ordinary shutdown.
            if recovery_error.is_none()
                && !hints.contains(HostHints::SHUTDOWN)
                && !hints.contains(HostHints::EXIT_REQUESTED)
            {
                if let Err(error) = self.advance_readiness() {
                    recovery_error = Some(error);
                    self.state.control_hint(HostHints::SHUTDOWN);
                    hints.0 |= HostHints::SHUTDOWN.0;
                }
            }
            if hints.contains(HostHints::TRAY_CONTEXT) && !hints.contains(HostHints::SHUTDOWN) {
                self.tray_menu()?;
            }
            // Child workbench commands can produce no host hints. Always let the
            // composition drain its bounded UI events immediately after dispatch.
            match catch_unwind(AssertUnwindSafe(|| on_hints(hints, &self))) {
                Ok(result) => result?,
                Err(_) => {
                    self.state.control.fault();
                    return Err(Error::from_hresult(E_UNEXPECTED));
                }
            }
            // SHUTDOWN is an intent, not completion. The composition root owns
            // asynchronous writer/worker draining and eventually calls stop().
        }
    }
    fn icon_data(&self) -> Result<NOTIFYICONDATAW> {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY_MESSAGE,
            hIcon: crate::startup::application_icon()?,
            ..Default::default()
        };
        let mut tooltip = [0u16; 128];
        for (slot, unit) in tooltip
            .iter_mut()
            .zip("未来教室 · 双击查看状态".encode_utf16())
        {
            *slot = unit;
        }
        data.szTip = tooltip;
        Ok(data)
    }
    fn add_tray(&self) -> Result<bool> {
        let data = self
            .icon_data()
            .map_err(|e| host_error("加载托盘图标资源", e))?;
        // SAFETY: valid window and icon; Shell copies data; NIM_DELETE pairs in Drop.
        // This API only documents a BOOL result, not a reliable GetLastError.
        Ok(unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool())
    }
    /// Reuse the single tray icon for public, nonsecret feedback. Shell owns the
    /// notification UI; this does not activate a window, play sound or open login.
    pub fn show_public_notice(&self, message: &str) -> Result<()> {
        if !self.tray.get() {
            return Ok(());
        }
        let mut data = self.icon_data()?;
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND;
        let mut title = [0u16; 64];
        for (slot, unit) in title.iter_mut().zip("未来教室 · 打开资料".encode_utf16()) {
            *slot = unit;
        }
        let mut body = [0u16; 256];
        for (slot, unit) in body.iter_mut().take(255).zip(message.encode_utf16()) {
            *slot = unit;
        }
        data.szInfoTitle = title;
        data.szInfo = body;
        // SAFETY: the registered icon belongs to this live host; Shell copies
        // the bounded, terminated text before returning. No focus API is used.
        if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
            return Err(Error::from_hresult(E_FAIL));
        }
        Ok(())
    }
    fn tray_menu(&self) -> Result<()> {
        struct Menu(HMENU);
        impl Drop for Menu {
            fn drop(&mut self) {
                // SAFETY: unique successful CreatePopupMenu result.
                unsafe {
                    let _ = DestroyMenu(self.0);
                }
            }
        }
        // SAFETY: all strings/menu data owned throughout the modal call. There is no
        // mutable State borrow across the nested message loop, only shared atomics.
        unsafe {
            let menu = Menu(CreatePopupMenu()?);
            AppendMenuW(menu.0, MF_STRING, 1, w!("查看状态"))?;
            AppendMenuW(menu.0, MF_STRING, 4, w!("设置"))?;
            AppendMenuW(menu.0, MF_STRING, 5, w!("快捷连接"))?;
            AppendMenuW(menu.0, MF_STRING, 6, w!("关于"))?;
            AppendMenuW(menu.0, MF_STRING, 2, w!("紧急停止全部语音"))?;
            AppendMenuW(menu.0, MF_STRING, 3, w!("退出程序"))?;
            let mut point = POINT::default();
            GetCursorPos(&mut point)?;
            let _ = SetForegroundWindow(self.hwnd); // only in response to user's tray click
            let selected = TrackPopupMenu(
                menu.0,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                self.hwnd,
                None,
            )
            .0;
            match selected {
                1 => self.state.hint(HostHints::OPEN_STATUS),
                4 => self.state.hint(HostHints::OPEN_SETTINGS),
                5 => self.state.hint(HostHints::OPEN_QUICK_CONNECT),
                6 => self.state.hint(HostHints::OPEN_ABOUT),
                2 => self.state.control_hint(HostHints::EMERGENCY_STOP_AUDIO),
                3 => self.state.hint(HostHints::EXIT_REQUESTED),
                _ => {}
            }
            let _ = PostMessageW(Some(self.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        }
        Ok(())
    }
}
impl Drop for HiddenHost {
    fn drop(&mut self) {
        // SAFETY: reverse initialization order on owning thread; state remains alive
        // throughout DestroyWindow reentrancy and userdata is detached by NCDESTROY.
        self.state.control_callback.replace(None);
        unsafe {
            FOREGROUND_TARGET.with(|target| target.set(HWND::default()));
            FOREGROUND_PENDING.with(|pending| pending.set(false));
            if !self.foreground_hook.is_invalid() {
                let _ = UnhookWinEvent(self.foreground_hook);
            }
            if !self.hwnd.is_invalid() {
                if self.tray.get() {
                    if let Ok(data) = self.icon_data() {
                        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                    }
                }
                let _ = KillTimer(Some(self.hwnd), TIMER);
                if self.session_registered.get() {
                    let _ = WTSUnRegisterSessionNotification(self.hwnd);
                }
                let _ = DestroyWindow(self.hwnd);
            }
            if self.class != 0 {
                let _ = UnregisterClassW(w!("FutureClassroom.HiddenHost.v1"), Some(self.instance));
            }
            let event = self.session_ready_event.get();
            if !event.is_invalid() {
                let _ = CloseHandle(event);
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Catch is *inside* the system ABI boundary, including creation/destruction.
    match catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: system supplies CREATESTRUCT during NCCREATE; otherwise userdata
        // is either null or our live Box<State>. No mutable reference is ever formed.
        unsafe {
            if message == WM_NCCREATE {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as _);
            }
            let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
            if message == WM_NCDESTROY {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                return DefWindowProcW(hwnd, message, wparam, lparam);
            }
            if let Some(state) = pointer.as_ref() {
                if state.control.0.load(Ordering::Acquire) & STOP != 0 {
                    // Also escape our tray menu's nested pump when shutdown wins.
                    let _ = EndMenu();
                }
                if message == state.taskbar_recreated {
                    state.hint(HostHints::TASKBAR_RECREATED);
                    return LRESULT(0);
                }
                match message {
                    OPEN_INSTANCE_MESSAGE if wparam.0 == 0 && lparam.0 == 0 => {
                        state.hint(HostHints::OPEN_STATUS);
                        return LRESULT(0);
                    }
                    FOREGROUND_MESSAGE => {
                        FOREGROUND_PENDING.with(|pending| pending.set(false));
                        // Sample once before the main application pump. The
                        // callback receives only a negative memory fact; neither
                        // the writer nor its execution fence calls Windows.
                        state.foreground_hint(crate::workbench::lesson_foreground_allowed());
                        return LRESULT(0);
                    }
                    WM_TIMER if wparam.0 == TIMER => {
                        state.hint(HostHints::TICK);
                        return LRESULT(0);
                    }
                    WM_TIMECHANGE => state.hint(HostHints::TIME_CHANGED),
                    WM_DISPLAYCHANGE
                    | WM_SETTINGCHANGE
                    | WM_SYSCOLORCHANGE
                    | WM_THEMECHANGED
                    | WM_DWMCOMPOSITIONCHANGED => state.hint(HostHints::DISPLAY_CHANGED),
                    WM_POWERBROADCAST => {
                        let event = match wparam.0 as u32 {
                            PBT_APMSUSPEND => HostHints::SUSPENDING.0,
                            PBT_APMRESUMEAUTOMATIC
                            | PBT_APMRESUMESUSPEND
                            | PBT_APMRESUMECRITICAL => HostHints::RESUMED.0,
                            _ => 0,
                        };
                        state.control_hint(HostHints(HostHints::POWER_CHANGED.0 | event));
                        return LRESULT(1);
                    }
                    WM_WTSSESSION_CHANGE => {
                        let bits = HostHints::SESSION_CHANGED.0
                            | if wparam.0 as u32 == WTS_SESSION_LOCK {
                                HostHints::SESSION_LOCKED.0
                            } else {
                                0
                            };
                        state.control_hint(HostHints(bits));
                    }
                    WM_QUERYENDSESSION => return LRESULT(1),
                    WM_ENDSESSION if wparam.0 != 0 => {
                        state.control_hint(HostHints::SHUTDOWN);
                        let _ = EndMenu();
                    }
                    WM_CLOSE => {
                        state.hint(HostHints::EXIT_REQUESTED);
                        return LRESULT(0);
                    }
                    TRAY_MESSAGE => {
                        match lparam.0 as u32 {
                            WM_LBUTTONDBLCLK => state.hint(HostHints::OPEN_STATUS),
                            WM_RBUTTONUP => state.hint(HostHints::TRAY_CONTEXT),
                            _ => {}
                        }
                        return LRESULT(0);
                    }
                    _ => {}
                }
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
    })) {
        Ok(value) => value,
        Err(_) => {
            // SAFETY: a panic in dispatch irreversibly stops this UI event loop;
            // no unwind escapes into Windows. WM_QUIT carries failure, not success.
            unsafe {
                PostQuitMessage(1);
            }
            LRESULT(0)
        }
    }
}

unsafe extern "system" fn foreground_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    // ABI containment includes TLS access. Native callback never borrows State,
    // changes focus, or renders; ordinary dispatch re-samples the actual system.
    if catch_unwind(|| {
        FOREGROUND_TARGET.with(|target| {
            let hwnd = target.get();
            if !hwnd.is_invalid() && !FOREGROUND_PENDING.with(|pending| pending.replace(true)) {
                // SAFETY: message carries no pointer; target is cleared before unhook.
                if unsafe { PostMessageW(Some(hwnd), FOREGROUND_MESSAGE, WPARAM(0), LPARAM(0)) }
                    .is_err()
                {
                    FOREGROUND_PENDING.with(|pending| pending.set(false));
                }
            }
        })
    })
    .is_err()
    {
        // SAFETY: fail the current UI thread rather than unwind through Win32.
        unsafe { PostQuitMessage(1) }
    }
}

#[derive(Debug)]
pub struct HostLifecycleReport {
    pub elapsed_millis: u128,
    pub ticks: u32,
    pub tray_requested: bool,
    pub session_notifications_registered: bool,
}

/// Bounded non-disruptive native lifecycle check: never changes foreground,
/// minimizes another window, or emits global input. Interactive tray evidence
/// requires this process to run in the logged-in desktop session.
pub fn probe_lifecycle(seconds: u32, with_tray: bool) -> Result<HostLifecycleReport> {
    if !(1..=30).contains(&seconds) {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let host = HiddenHost::new(with_tray)?;
    let stop = host.controller();
    let started = std::time::Instant::now();
    let mut ticks = 0;
    host.run(|hints| {
        if hints.contains(HostHints::TICK) {
            ticks += 1;
        }
        if started.elapsed() >= std::time::Duration::from_secs(u64::from(seconds)) {
            stop.stop();
        }
        Ok(())
    })?;
    Ok(HostLifecycleReport {
        elapsed_millis: started.elapsed().as_millis(),
        ticks,
        tray_requested: with_tray,
        session_notifications_registered: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn instance_owner_recovers_stale_record_and_preserves_live_or_unknown_files() {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-StaleInstanceOwner-{}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        struct Scratch(std::path::PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let result = std::fs::remove_dir_all(&self.0);
                if !std::thread::panicking() {
                    result.unwrap();
                }
            }
        }
        let _scratch = Scratch(root.clone());
        let _guard = crate::InstanceGuard::acquire(&root).unwrap();
        let path = root.join("runtime/instance.owner");
        let mut old = [0u8; 12];
        old[..4].copy_from_slice(b"FCI1");
        old[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&path, old).unwrap();

        // A valid file with no owner is replaced while the original lock stays
        // held, then retains the normal delete-on-close publication lifetime.
        let mut owner = create_instance_owner(&root).unwrap();
        assert_eq!(owner.metadata().unwrap().len(), 0);
        owner.write_all(&old).unwrap();
        let error = create_instance_owner(&root).unwrap_err();
        assert_eq!(
            error.code(),
            windows::core::HRESULT::from_win32(ERROR_SHARING_VIOLATION.0)
        );
        assert_eq!(std::fs::read(&path).unwrap(), old);
        drop(owner);
        assert!(!path.exists());

        // Same-sized unknown files and hard links are never deleted or reset.
        let unknown = b"not-instance";
        assert_eq!(unknown.len(), 12);
        std::fs::write(&path, unknown).unwrap();
        assert!(create_instance_owner(&root).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), unknown);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, old).unwrap();
        let link = root.join("keep.bin");
        std::fs::hard_link(&path, &link).unwrap();
        assert!(create_instance_owner(&root).is_err());
        assert_eq!(std::fs::read(&link).unwrap(), old);
        assert!(path.exists());
    }

    #[test]
    fn readiness_wait_can_be_cancelled_without_consuming_open_request() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut host = HiddenHost::new(false).unwrap();
        host.wait_until_ready().unwrap();
        // Only the retry scheduling is synthetic; no real Shell registration
        // fails and no system service or taskbar is changed by this test.
        host.tray_requested = true;
        let mut retry = HostRetry::new(Instant::now());
        retry.decide(Instant::now());
        *host.retry.borrow_mut() = Some(retry);
        host.state.hint(HostHints::OPEN_STATUS);
        unsafe {
            PostMessageW(Some(host.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
        }
        assert!(!host.wait_until_ready().unwrap());
        assert!(
            HostHints(host.state.hints.load(Ordering::Acquire)).contains(HostHints::OPEN_STATUS)
        );
    }

    #[test]
    fn recovery_timeout_delivers_shutdown_before_returning_error() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut host = HiddenHost::new(false).unwrap();
        host.wait_until_ready().unwrap();
        // Model an already-added icon; no real icon is installed in this test.
        host.tray_requested = true;
        host.tray.set(true);
        let stop = host.controller();
        let revoked = Arc::new(AtomicU32::new(0));
        let seen = revoked.clone();
        host.set_control_callback(Arc::new(move |hints| {
            seen.fetch_or(hints.0, Ordering::Release);
        }))
        .unwrap();
        let mut recovering = false;
        let mut shutdown = false;
        let error = host
            .run_with_host(|hints, host| {
                if hints.contains(HostHints::SHUTDOWN) {
                    shutdown = true;
                    assert_ne!(revoked.load(Ordering::Acquire) & HostHints::SHUTDOWN.0, 0);
                    stop.stop();
                } else if !recovering {
                    host.taskbar_recreated();
                    *host.retry.borrow_mut() = Some(HostRetry::new(
                        Instant::now() - std::time::Duration::from_secs(61),
                    ));
                    recovering = true;
                }
                Ok(())
            })
            .unwrap_err();
        assert!(shutdown);
        assert!(error.to_string().contains("超时"));
    }

    #[test]
    fn repeated_start_requests_only_the_original_root_without_foreground_changes() {
        use std::os::windows::process::CommandExt;
        const CHILD_ROOT: &str = "FC_TEST_INSTANCE_REQUEST_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            assert_eq!(
                HiddenHost::request_existing(Path::new(&root)).unwrap(),
                InstanceRequest::Requested
            );
            return;
        }
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let foreground = unsafe { GetForegroundWindow() };
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-InstanceRequest-{}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        struct Scratch(std::path::PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let result = std::fs::remove_dir_all(&self.0);
                if !std::thread::panicking() {
                    result.unwrap();
                }
            }
        }
        let _scratch = Scratch(root.clone());
        let guard = crate::InstanceGuard::acquire(&root).unwrap();
        assert_eq!(
            crate::InstanceGuard::acquire(&root).err().unwrap().code(),
            windows::core::HRESULT::from_win32(ERROR_SHARING_VIOLATION.0)
        );
        let host = HiddenHost::for_instance(&root, false).unwrap();
        assert!(HiddenHost::request_existing(&root.join("other-root")).is_err());
        assert_eq!(
            host.state.hints.load(Ordering::Acquire) & HostHints::OPEN_STATUS.0,
            0
        );
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0)
            .args([
                "--exact",
                "host::tests::repeated_start_requests_only_the_original_root_without_foreground_changes",
                "--test-threads=1",
            ])
            .env(CHILD_ROOT, &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child request failed: {:?}",
            output
        );
        let stop = host.controller();
        let started = std::time::Instant::now();
        let mut requested = false;
        host.run(|hints| {
            if hints.contains(HostHints::OPEN_STATUS) {
                requested = true;
                stop.stop();
            } else if started.elapsed() >= std::time::Duration::from_secs(3) {
                stop.stop();
            }
            Ok(())
        })
        .unwrap();
        assert!(requested);
        assert!(!root.join("runtime/instance.owner").exists());
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
        drop(guard);
        drop(crate::InstanceGuard::acquire(&root).unwrap());
    }
    #[test]
    fn hidden_top_level_routes_events_and_control_stops_without_queue() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let host = HiddenHost::new(false).unwrap();
        let stop = host.controller();
        let direct = Arc::new(AtomicU32::new(0));
        let seen = direct.clone();
        host.set_control_callback(Arc::new(move |hint| {
            seen.fetch_or(hint.0, Ordering::Release);
        }))
        .unwrap();
        // SAFETY: own live hidden window only, no desktop-wide broadcast/input.
        unsafe {
            let _ = SendMessageW(host.hwnd, WM_TIMECHANGE, Some(WPARAM(0)), Some(LPARAM(0)));
            let _ = SendMessageW(
                host.hwnd,
                WM_WTSSESSION_CHANGE,
                Some(WPARAM(WTS_SESSION_LOCK as usize)),
                Some(LPARAM(0)),
            );
            let _ = SendMessageW(
                host.hwnd,
                WM_POWERBROADCAST,
                Some(WPARAM(PBT_APMSUSPEND as usize)),
                Some(LPARAM(0)),
            );
            let _ = SendMessageW(
                host.hwnd,
                WM_POWERBROADCAST,
                Some(WPARAM(PBT_APMRESUMEAUTOMATIC as usize)),
                Some(LPARAM(0)),
            );
            let _ = SendMessageW(host.hwnd, WM_ENDSESSION, Some(WPARAM(1)), Some(LPARAM(0)));
        }
        // The outer run loop has not started: this proves revocation reaches a
        // nested modal dispatch directly rather than waiting for on_hints.
        let bits = direct.load(Ordering::Acquire);
        assert_ne!(bits & HostHints::SESSION_LOCKED.0, 0);
        assert_ne!(bits & HostHints::POWER_CHANGED.0, 0);
        assert_ne!(bits & HostHints::SUSPENDING.0, 0);
        assert_ne!(bits & HostHints::RESUMED.0, 0);
        assert_ne!(bits & HostHints::SHUTDOWN.0, 0);
        let mut ticks = 0;
        let mut time_changed = false;
        let mut shutdown_seen = false;
        host.run(|hints| {
            time_changed |= hints.contains(HostHints::TIME_CHANGED);
            shutdown_seen |= hints.contains(HostHints::SHUTDOWN);
            if hints.contains(HostHints::TICK) {
                ticks += 1;
            }
            if ticks >= 2 {
                stop.stop();
            }
            Ok(())
        })
        .unwrap();
        assert!(time_changed);
        assert!(shutdown_seen);
        assert_eq!(ticks, 2);
    }
    #[test]
    fn foreground_dispatch_revokes_external_before_main_pump_but_not_own_chooser() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let host = HiddenHost::new(false).unwrap();
        let seen = Arc::new(AtomicU32::new(0));
        let received = seen.clone();
        host.set_control_callback(Arc::new(move |hint| {
            received.fetch_or(hint.0, Ordering::Release);
        }))
        .unwrap();
        host.state.foreground_hint(true);
        assert_eq!(
            seen.load(Ordering::Acquire),
            0,
            "own chooser/desktop does not cancel"
        );
        host.state.foreground_hint(false);
        assert_ne!(
            seen.load(Ordering::Acquire) & HostHints::FOREGROUND_UNAVAILABLE.0,
            0,
            "negative callback ran before host.run/main application pump"
        );
        assert_ne!(
            host.state.hints.load(Ordering::Acquire) & HostHints::FOREGROUND_CHANGED.0,
            0
        );
    }
}
