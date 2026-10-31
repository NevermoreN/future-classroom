//! One UI-thread hidden top-level host, tray, coalesced system hints and shutdown.
//! No writer/network joins, secret material, or privileged settings live here.
use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    panic::{catch_unwind, AssertUnwindSafe},
    rc::Rc,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
use windows::{
    core::{w, Error, Result},
    Win32::{
        Foundation::*,
        System::{LibraryLoader::*, RemoteDesktop::*},
        UI::{Accessibility::*, Shell::*, WindowsAndMessaging::*},
    },
};

const TIMER: usize = 0xfc01;
const TRAY_MESSAGE: u32 = WM_APP + 41;
const FOREGROUND_MESSAGE: u32 = WM_APP + 42;
thread_local! {
    // Out-of-context callbacks are delivered on the hook owner's UI thread.
    // Only an HWND value is retained; there is no cross-thread State pointer.
    static FOREGROUND_TARGET: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
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
    /// Intent only. Bootstrap must authenticate before invoking controller.stop().
    pub const EXIT_REQUESTED: Self = Self(512);
    pub const TASKBAR_RECREATED: Self = Self(1024);
    const TRAY_CONTEXT: Self = Self(2048);
    pub const FOREGROUND_CHANGED: Self = Self(4096);
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
    /// Composition root calls only after authorization, or during OS shutdown.
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
    tray: bool,
    session_registered: bool,
    class: u16,
    instance: HINSTANCE,
    foreground_hook: HWINEVENTHOOK,
    _thread: PhantomData<Rc<()>>,
}
impl HiddenHost {
    /// A tray is required in normal interactive operation. Headless tests can
    /// disable it explicitly; that does not count as a real tray acceptance test.
    pub fn new(enable_tray: bool) -> Result<Self> {
        // SAFETY: module pseudohandle retained only for same-process class lifetime.
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
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
            return Err(Error::from_thread());
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
            tray: false,
            session_registered: false,
            class: atom,
            instance,
            foreground_hook: HWINEVENTHOOK::default(),
            _thread: PhantomData,
        };
        if host.state.taskbar_recreated == 0 {
            return Err(Error::from_thread());
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
            )?
        };
        // SAFETY: host owns the window and unregisters notifications in Drop.
        unsafe {
            WTSRegisterSessionNotification(host.hwnd, NOTIFY_FOR_THIS_SESSION)?;
        }
        host.session_registered = true;
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
            return Err(Error::from_thread());
        }
        // SAFETY: window-owned timer has no callback/raw pointers; messages coalesce.
        if unsafe { SetTimer(Some(host.hwnd), TIMER, 1000, None) } == 0 {
            return Err(Error::from_thread());
        }
        if enable_tray {
            host.add_tray()?;
            host.tray = true;
        }
        Ok(host)
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
    /// Callback runs on UI thread after dispatch, must remain bounded/nonblocking.
    /// Background shutdown is coordinated by bootstrap, never joined by this loop.
    pub fn run(self, mut on_hints: impl FnMut(HostHints) -> Result<()>) -> Result<()> {
        let mut message = MSG::default();
        loop {
            let control = self.state.control.0.load(Ordering::Acquire);
            if control & FAULT != 0 {
                return Err(Error::from_hresult(E_UNEXPECTED));
            }
            if control & STOP != 0 {
                return Ok(());
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
            let hints = HostHints(self.state.hints.swap(0, Ordering::AcqRel));
            if hints.contains(HostHints::TASKBAR_RECREATED) && self.tray {
                self.add_tray()?;
            }
            if hints.contains(HostHints::TRAY_CONTEXT) {
                self.tray_menu()?;
            }
            // Child workbench commands can produce no host hints. Always let the
            // composition drain its bounded UI events immediately after dispatch.
            match catch_unwind(AssertUnwindSafe(|| on_hints(hints))) {
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
            // SAFETY: shared stock icon; Shell_NotifyIcon copies it, no DestroyIcon.
            hIcon: unsafe { LoadIconW(None, IDI_APPLICATION)? },
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
    fn add_tray(&self) -> Result<()> {
        let data = self.icon_data()?;
        // SAFETY: valid window and icon; Shell copies data; NIM_DELETE pairs in Drop.
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
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
            AppendMenuW(menu.0, MF_STRING, 1, w!("查看状态 / 管理"))?;
            AppendMenuW(menu.0, MF_STRING, 2, w!("紧急停止全部语音"))?;
            AppendMenuW(menu.0, MF_STRING, 3, w!("退出（需主管理员授权）"))?;
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
                if self.tray {
                    if let Ok(data) = self.icon_data() {
                        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                    }
                }
                let _ = KillTimer(Some(self.hwnd), TIMER);
                if self.session_registered {
                    let _ = WTSUnRegisterSessionNotification(self.hwnd);
                }
                let _ = DestroyWindow(self.hwnd);
            }
            if self.class != 0 {
                let _ = UnregisterClassW(w!("FutureClassroom.HiddenHost.v1"), Some(self.instance));
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
                    WM_DISPLAYCHANGE | WM_SETTINGCHANGE => state.hint(HostHints::DISPLAY_CHANGED),
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
