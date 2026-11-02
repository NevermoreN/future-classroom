//! Native, UI-thread-only form adapter. No browser engine or synchronous worker wait.
use fc_presentation::workbench::{ActionId, Event, FieldId, FieldKind, FormData, View, WindowRole};
#[path = "workbench/color_picker.rs"]
mod color_picker;
#[path = "workbench/preview.rs"]
mod preview;
#[path = "workbench/registry.rs"]
mod registry;
#[path = "workbench/widgets.rs"]
mod widgets;
pub use preview::{LayoutPreview, PreviewNode};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    marker::PhantomData,
    panic::{catch_unwind, AssertUnwindSafe},
    rc::Rc,
    sync::{
        atomic::{compiler_fence, Ordering},
        Mutex,
    },
};
use widgets::{GridControl, MaskControl};
use windows::{
    core::{w, Error, Result, HSTRING, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{
            LibraryLoader::*,
            SystemServices::{SS_LEFT, SS_NOPREFIX, SS_NOTIFY},
        },
        UI::{Controls::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

/// The native host samples this same policy on foreground-message dispatch.
/// This is a read-only UI-thread observation, never a worker/SQL predicate.
pub(crate) fn lesson_foreground_allowed() -> bool {
    if !crate::native::input_desktop_available() {
        return false;
    }
    if crate::native::desktop_foreground() == fc_presentation::Foreground::Desktop {
        return true;
    }
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return false;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        lesson_foreground_process_allowed(
            pid,
            windows::Win32::System::Threading::GetCurrentProcessId(),
        )
    }
}

fn lesson_foreground_process_allowed(observed_pid: u32, current_pid: u32) -> bool {
    observed_pid != 0 && observed_pid == current_pid
}

const MAX_SUBMITS: usize = 8;
const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;
const FIELD_ID_BASE: u16 = 2000;
const ACTION_ID_BASE: u16 = 3000;
static CLASSES: Mutex<usize> = Mutex::new(0);
thread_local! {
    #[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "nightly-2026-10-29 rebuilt Win7 std wraps this const initializer in a non-const function"))]
    static WORKBENCHES: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
}

/// Used by the owning thread's message pump for native Tab/default-button
/// navigation. Only messages targeting an explicitly visible owned workbench
/// are considered, so hidden probes never change another window's focus.
pub(crate) fn dialog_message(message: &MSG) -> bool {
    let keyboard = (WM_KEYFIRST..=WM_KEYLAST).contains(&message.message);
    let mouse = (WM_MOUSEFIRST..=WM_MOUSELAST).contains(&message.message);
    if !keyboard && !mouse {
        return false;
    }
    let windows = WORKBENCHES.with(|windows| windows.borrow().clone());
    for hwnd in windows {
        // SAFETY: thread-local registered live HWNDs; the list borrow is released
        // before the nested dialog message dispatch can reenter this adapter.
        unsafe {
            if IsWindowVisible(hwnd).as_bool()
                && (message.hwnd == hwnd || IsChild(hwnd, message.hwnd).as_bool())
            {
                let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
                if let Some(state) = pointer.as_ref() {
                    if !state.suppress.get() {
                        state.input.set(true);
                    }
                }
                if keyboard && IsDialogMessageW(hwnd, message).as_bool() {
                    return true;
                }
            }
        }
    }
    false
}

struct ClassLease(HINSTANCE);
impl ClassLease {
    fn acquire() -> Result<Self> {
        let mut owners = CLASSES
            .lock()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        // SAFETY: module handle is borrowed for this process's native class lifetime.
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        if *owners == 0 {
            let class = WNDCLASSW {
                hInstance: instance,
                lpszClassName: w!("FutureClassroom.Workbench.v1"),
                lpfnWndProc: Some(window_proc),
                // SAFETY: stock brush/cursor are shared borrowed OS resources.
                hbrBackground: HBRUSH(unsafe { GetStockObject(WHITE_BRUSH) }.0),
                hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
                ..Default::default()
            };
            // SAFETY: static class name and correct ABI callback; lease outlives HWND.
            if unsafe { RegisterClassW(&class) } == 0 {
                return Err(Error::from_thread());
            }
        }
        *owners += 1;
        Ok(Self(instance))
    }
}
impl Drop for ClassLease {
    fn drop(&mut self) {
        if let Ok(mut owners) = CLASSES.lock() {
            *owners = owners.saturating_sub(1);
            if *owners == 0 {
                // SAFETY: all leased windows have already destroyed their HWNDs.
                unsafe {
                    let _ = UnregisterClassW(w!("FutureClassroom.Workbench.v1"), Some(self.0));
                }
            }
        }
    }
}
struct Font(HFONT);
impl Font {
    fn new() -> Result<Self> {
        // SAFETY: fixed owned font; used only on this UI thread, deleted after controls.
        let font = unsafe {
            CreateFontW(
                -19,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                w!("Microsoft YaHei UI"),
            )
        };
        if font.is_invalid() {
            return Err(Error::from_thread());
        }
        Ok(Self(font))
    }
}
impl Drop for Font {
    fn drop(&mut self) {
        // SAFETY: uniquely owned font, after every control referencing it was destroyed.
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}
struct Child(HWND);
impl Drop for Child {
    fn drop(&mut self) {
        // SAFETY: unique child owner; failure after parent destruction is harmless.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeKind {
    Text,
    Password,
    Multiline,
    ReadOnly,
    Choice,
    TimeRange,
}
impl From<&FieldKind> for NativeKind {
    fn from(kind: &FieldKind) -> Self {
        match kind {
            FieldKind::Text => Self::Text,
            FieldKind::Password => Self::Password,
            FieldKind::Multiline => Self::Multiline,
            FieldKind::ReadOnly => Self::ReadOnly,
            FieldKind::Choice(_) => Self::Choice,
            FieldKind::TimeRange => Self::TimeRange,
        }
    }
}
#[derive(Clone, Copy)]
struct FieldBinding {
    id: FieldId,
    kind: NativeKind,
    hwnd: HWND,
    native_id: u16,
    max_utf16: usize,
}
#[derive(Clone, Copy)]
struct ActionBinding {
    id: ActionId,
    hwnd: HWND,
    native_id: u16,
}
#[derive(Default)]
struct Controls {
    owned: Vec<Child>,
    fields: Vec<FieldBinding>,
    actions: Vec<ActionBinding>,
    labels: Vec<HWND>,
    description: HWND,
    status: HWND,
    masks: Vec<MaskControl>,
    grid: Option<GridControl>,
}
impl Drop for Controls {
    fn drop(&mut self) {
        self.grid.take();
        self.masks.clear();
        while let Some(child) = self.owned.pop() {
            drop(child);
        }
    }
}
struct State {
    controls: RefCell<Controls>,
    view: RefCell<Option<View>>,
    events: RefCell<VecDeque<Event>>,
    event_bytes: Cell<usize>,
    input: Cell<bool>,
    close: Cell<bool>,
    suppress: Cell<bool>,
    capturing: Cell<bool>,
    error: Cell<i32>,
    scroll: Cell<i32>,
    presented_secrets: RefCell<Vec<FieldId>>,
    grid_cell: Cell<Option<(usize, usize)>>,
    lock_action: Cell<Option<ActionId>>,
    empty_form_action: Cell<Option<ActionId>>,
    preview: RefCell<Option<preview::Canvas>>,
}
impl State {
    fn new() -> Self {
        Self {
            controls: RefCell::new(Controls::default()),
            view: RefCell::new(None),
            events: RefCell::new(VecDeque::new()),
            event_bytes: Cell::new(0),
            input: Cell::new(false),
            close: Cell::new(false),
            suppress: Cell::new(false),
            capturing: Cell::new(false),
            error: Cell::new(0),
            scroll: Cell::new(0),
            presented_secrets: RefCell::new(Vec::new()),
            grid_cell: Cell::new(None),
            lock_action: Cell::new(None),
            empty_form_action: Cell::new(None),
            preview: RefCell::new(None),
        }
    }
    fn fail(&self, error: Error) {
        self.error.set(error.code().0);
    }
    fn clear_passwords(&self) {
        let fields = match self.controls.try_borrow() {
            Ok(c) => c.fields.clone(),
            Err(_) => {
                self.error.set(E_UNEXPECTED.0);
                return;
            }
        };
        let suppress = Suppress::new(self);
        let presented = match self.presented_secrets.try_borrow_mut() {
            Ok(mut ids) => std::mem::take(&mut *ids),
            Err(_) => {
                self.error.set(E_UNEXPECTED.0);
                return;
            }
        };
        for field in fields {
            if field.kind == NativeKind::Password || presented.contains(&field.id) {
                // SAFETY: only this adapter's owned Edit controls; clearing also
                // removes native undo data. Temporary overwrite contains no secret.
                unsafe {
                    let count = GetWindowTextLengthW(field.hwnd).max(0) as usize;
                    if count <= field.max_utf16 {
                        let overwrite = vec![b' ' as u16; count];
                        let mut terminated = overwrite;
                        terminated.push(0);
                        let _ = SetWindowTextW(field.hwnd, PCWSTR(terminated.as_ptr()));
                    }
                    let _ = SetWindowTextW(field.hwnd, w!(""));
                    let _ = SendMessageW(field.hwnd, EM_EMPTYUNDOBUFFER, None, None);
                }
            }
        }
        drop(suppress);
    }
    fn discard_forms(&self) {
        self.clear_layout_preview();
        self.grid_cell.set(None);
        if let Ok(mut events) = self.events.try_borrow_mut() {
            events.clear();
            self.event_bytes.set(0);
        } else {
            self.error.set(E_UNEXPECTED.0);
        }
    }
    fn clear_layout_preview(&self) {
        let old = match self.preview.try_borrow_mut() {
            Ok(mut preview) => preview.take(),
            Err(_) => {
                self.error.set(E_UNEXPECTED.0);
                return;
            }
        };
        drop(old);
        self.discard_layout_events();
    }
    fn discard_layout_events(&self) {
        if let Ok(mut events) = self.events.try_borrow_mut() {
            events.retain(|event| {
                !matches!(event, Event::LayoutSelect { .. } | Event::LayoutDrop { .. })
            });
            self.event_bytes.set(
                events
                    .iter()
                    .map(|e| match e {
                        Event::Submit(_, form) => {
                            form.allocation_bytes().unwrap_or(MAX_EVENT_BYTES)
                        }
                        _ => std::mem::size_of::<Event>(),
                    })
                    .fold(0usize, usize::saturating_add),
            );
        } else {
            self.error.set(E_UNEXPECTED.0)
        }
    }
    fn queue_layout(&self, event: Event) {
        if self.suppress.get()
            || self.capturing.get()
            || self
                .view
                .try_borrow()
                .map_or(true, |v| v.as_ref().is_none_or(|v| v.busy))
        {
            return;
        }
        if let Ok(mut events) = self.events.try_borrow_mut() {
            let size = std::mem::size_of::<Event>();
            if events.len() < MAX_SUBMITS
                && self.event_bytes.get().saturating_add(size) <= MAX_EVENT_BYTES
            {
                events.push_back(event);
                self.event_bytes.set(self.event_bytes.get() + size);
                self.input.set(true);
            }
        } else {
            self.error.set(E_UNEXPECTED.0)
        }
    }
    fn submit(&self, action: ActionBinding) -> Result<()> {
        if !self.action_allowed(action.id)? {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        if self.capturing.replace(true) {
            return Err(Error::from_hresult(E_UNEXPECTED));
        }
        struct Capture<'a>(&'a Cell<bool>);
        impl Drop for Capture<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _capture = Capture(&self.capturing);
        struct Clear<'a>(&'a State);
        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                self.0.clear_passwords();
            }
        }
        let _clear = Clear(self);
        if self.lock_action.get() == Some(action.id) {
            // Lock supersedes queued forms, including their zeroizing secrets.
            // Never read the native fields or carry their values into this event.
            self.discard_forms();
            return self.queue_submit(action.id, FormData::new());
        }
        if self.empty_form_action.get() == Some(action.id) {
            return self.queue_submit(action.id, FormData::new());
        }
        let fields = self
            .controls
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .fields
            .clone();
        let mut form = FormData::new();
        for field in fields {
            if field.kind != NativeKind::ReadOnly {
                let text = self.read_field(field)?;
                form.insert(field.id, text)
                    .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
            }
        }
        self.queue_submit(action.id, form)
    }
    fn action_allowed(&self, id: ActionId) -> Result<bool> {
        let view = self
            .view
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        Ok(view.as_ref().is_some_and(|view| {
            (!view.busy || self.lock_action.get() == Some(id))
                && view.actions.iter().any(|a| a.id == id && a.enabled)
        }))
    }
    fn queue_submit(&self, id: ActionId, form: FormData) -> Result<()> {
        let bytes = form
            .allocation_bytes()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        let mut events = self
            .events
            .try_borrow_mut()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        let next = self
            .event_bytes
            .get()
            .checked_add(bytes)
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if events.len() >= MAX_SUBMITS || next > MAX_EVENT_BYTES {
            return Err(Error::from_hresult(E_OUTOFMEMORY));
        }
        events.push_back(Event::Submit(id, form));
        self.event_bytes.set(next);
        Ok(())
    }
    fn read_field(&self, field: FieldBinding) -> Result<String> {
        match field.kind {
            NativeKind::Choice => {
                // SAFETY: synchronous query of this UI thread's owned ComboBox.
                let index = unsafe { SendMessageW(field.hwnd, CB_GETCURSEL, None, None) }.0;
                if index < 0 {
                    return Ok(String::new());
                }
                let view = self
                    .view
                    .try_borrow()
                    .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
                let field = view
                    .as_ref()
                    .and_then(|v| v.fields.iter().find(|f| f.id == field.id))
                    .ok_or_else(|| Error::from_hresult(E_UNEXPECTED))?;
                match &field.kind {
                    FieldKind::Choice(options) => options
                        .get(index as usize)
                        .map(|c| c.value.clone())
                        .ok_or_else(|| Error::from_hresult(E_INVALIDARG)),
                    _ => Err(Error::from_hresult(E_UNEXPECTED)),
                }
            }
            NativeKind::TimeRange => self
                .controls
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
                .masks
                .iter()
                .find(|m| m.hwnd() == field.hwnd)
                .ok_or_else(|| Error::from_hresult(E_UNEXPECTED))?
                .digits(),
            _ => read_edit(field),
        }
    }
    fn grid_selected(&self, row: usize, column: usize) {
        self.grid_cell.set(Some((row, column)));
        self.input.set(true);
    }
}
struct Suppress<'a> {
    state: &'a State,
    previous: bool,
}
impl<'a> Suppress<'a> {
    fn new(state: &'a State) -> Self {
        Self {
            previous: state.suppress.replace(true),
            state,
        }
    }
}
impl Drop for Suppress<'_> {
    fn drop(&mut self) {
        self.state.suppress.set(self.previous);
    }
}

struct WipingUtf16(Vec<u16>);
impl Drop for WipingUtf16 {
    fn drop(&mut self) {
        for unit in &mut self.0 {
            // SAFETY: exclusive live temporary allocation, wiped before deallocation.
            unsafe {
                std::ptr::write_volatile(unit, 0);
            }
        }
        compiler_fence(Ordering::SeqCst);
    }
}
struct WipingText(String);
impl Drop for WipingText {
    fn drop(&mut self) {
        // SAFETY: unique owned String; all bytes are made zero, preserving UTF-8, and
        // no borrow or observer survives this wipe. No secret is formatted/logged.
        unsafe {
            for byte in self.0.as_mut_vec() {
                std::ptr::write_volatile(byte, 0);
            }
        }
        compiler_fence(Ordering::SeqCst);
    }
}
fn read_edit(field: FieldBinding) -> Result<String> {
    // SAFETY: the UI thread owns this Edit; returned count is checked before any
    // allocation. The API synchronously copies into a uniquely owned fixed buffer.
    let length = unsafe { GetWindowTextLengthW(field.hwnd) };
    if length < 0 || length as usize > field.max_utf16 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let mut buffer = WipingUtf16(vec![0; length as usize + 1]);
    let copied = unsafe { GetWindowTextW(field.hwnd, &mut buffer.0) };
    if copied != length || unsafe { GetWindowTextLengthW(field.hwnd) } != length {
        return Err(Error::from_hresult(E_UNEXPECTED));
    }
    // At most three UTF-8 bytes per UTF-16 code unit. Allocate once so growing a
    // secret String cannot free earlier, unwiped reallocations during decoding.
    let capacity = (length as usize)
        .checked_mul(3)
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    let mut text = WipingText(String::with_capacity(capacity));
    for character in char::decode_utf16(buffer.0[..length as usize].iter().copied()) {
        text.0
            .push(character.map_err(|_| Error::from_hresult(E_INVALIDARG))?);
    }
    Ok(std::mem::take(&mut text.0))
}

pub struct Window {
    hwnd: HWND,
    role: Cell<WindowRole>,
    state: Box<State>,
    font: Font,
    class: ClassLease,
    _thread: PhantomData<Rc<()>>,
}
impl Window {
    /// Register this composition's sole lock action. Registration is optional
    /// and immutable; only an enabled action present in the current View may
    /// bypass busy. Lock emits empty FormData and clears secret native controls.
    pub fn set_lock_action(&self, id: ActionId) -> Result<()> {
        if id.0 == 0 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        if self.state.lock_action.get().is_some_and(|old| old != id)
            || self.state.empty_form_action.get() == Some(id)
        {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let view = self
            .state
            .view
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        let controls = self
            .state
            .controls
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        self.state.lock_action.set(Some(id));
        if let Some(view) = view.as_ref() {
            for action in &controls.actions {
                let enabled = view.actions.iter().any(|a| a.id == action.id && a.enabled)
                    && (!view.busy || action.id == id);
                // SAFETY: current controls are owned by this window/UI thread;
                // EnableWindow changes no focus and does not read form contents.
                unsafe {
                    let _ = EnableWindow(action.hwnd, enabled);
                }
            }
        }
        Ok(())
    }
    /// Register one immutable action that submits no fields. Unlike Lock, this
    /// action never bypasses busy or discards queued forms/nonsecret drafts.
    /// The normal password-clearing rule still applies after an actual submit.
    pub fn set_empty_form_action(&self, id: ActionId) -> Result<()> {
        if id.0 == 0 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        if self
            .state
            .empty_form_action
            .get()
            .is_some_and(|old| old != id)
            || self.state.lock_action.get() == Some(id)
        {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        self.state.empty_form_action.set(Some(id));
        Ok(())
    }
    /// Initially hidden. Construction and hidden tests never change foreground.
    pub fn new() -> Result<Self> {
        (registry::hooks(WindowRole::Workbench)?.create)()
    }
    fn create_native() -> Result<Self> {
        let class = ClassLease::acquire()?;
        let font = Font::new()?;
        let mut window = Self {
            hwnd: HWND::default(),
            role: Cell::new(WindowRole::Workbench),
            state: Box::new(State::new()),
            font,
            class,
            _thread: PhantomData,
        };
        // SAFETY: Box pointee is stable through HWND lifetime; NCDESTROY detaches
        // userdata before Box drop. No WS_VISIBLE means no activation on creation.
        window.hwnd = unsafe {
            CreateWindowExW(
                WS_EX_APPWINDOW,
                w!("FutureClassroom.Workbench.v1"),
                w!("未来教室工作台"),
                WS_OVERLAPPEDWINDOW | WS_VSCROLL,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                960,
                760,
                None,
                None,
                Some(window.class.0),
                Some((&raw const *window.state).cast()),
            )?
        };
        WORKBENCHES.with(|windows| windows.borrow_mut().push(window.hwnd));
        Ok(window)
    }
    /// An owned bounded renderer snapshot, shown only inside this workbench.
    /// Same-generation refreshes retain the existing image; owners/page changes
    /// must clear it before installing a different editing context.
    pub fn set_layout_preview(&self, image: Option<LayoutPreview>) -> Result<()> {
        (registry::hooks(WindowRole::Preview)?.preview)(self, image)
    }
    fn bind_preview_native(&self, image: Option<LayoutPreview>) -> Result<()> {
        let Some(image) = image else {
            self.clear_layout_preview();
            return Ok(());
        };
        if self
            .state
            .view
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .is_none()
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        if let Err(error) = image.validate() {
            self.state.clear_layout_preview();
            return Err(error);
        }
        let generation = self
            .state
            .preview
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .as_ref()
            .and_then(|p| p.generation());
        if generation != Some(image.generation) {
            self.state.discard_layout_events();
        }
        if self
            .state
            .preview
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .is_none()
        {
            let canvas = preview::Canvas::new(self.hwnd, &self.state, self.class.0)?;
            self.state.preview.replace(Some(canvas));
        }
        self.state
            .preview
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .as_ref()
            .ok_or_else(|| Error::from_hresult(E_UNEXPECTED))?
            .update(image)?;
        layout(self.hwnd, &self.state)
    }
    /// Pure programmatic outline update for the editor's selected stable node.
    /// It emits no user input, command, or authentication renewal.
    pub fn set_layout_selection(
        &self,
        key: Option<fc_presentation::layout::SceneNodeKey>,
    ) -> Result<()> {
        let preview = self
            .state
            .preview
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        match preview.as_ref() {
            Some(canvas) => canvas.select(key),
            None if key.is_none() => Ok(()),
            None => Err(Error::from_hresult(E_INVALIDARG)),
        }
    }
    /// Isolated diagnostics only: dispatch mouse messages to this hidden owned
    /// canvas, never global input, activation, or another process's window.
    pub fn probe_layout_drag(&self, from: (u16, u16), to: (u16, u16)) -> Result<()> {
        if self.is_visible() || self.state.suppress.get() || self.state.capturing.get() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        if self.state.view.borrow().as_ref().is_none_or(|v| v.busy) {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let (hwnd, x, y, tx, ty) = {
            let preview = self
                .state
                .preview
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
            let canvas = preview
                .as_ref()
                .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
            let (hwnd, x, y) = canvas.probe_point(from.0, from.1)?;
            let (_, tx, ty) = canvas.probe_point(to.0, to.1)?;
            (hwnd, x, y, tx, ty)
        };
        let pack = |x: i32, y: i32| LPARAM(((y as u16 as u32) << 16 | x as u16 as u32) as isize);
        // SAFETY: copied HWND/coordinates refer only to our hidden child. No
        // parent borrow survives synchronous subclass dispatch; it never captures.
        unsafe {
            let _ = SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(pack(x, y)));
            let _ = SendMessageW(hwnd, WM_MOUSEMOVE, Some(WPARAM(1)), Some(pack(tx, ty)));
            let _ = SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(pack(tx, ty)));
        }
        Ok(())
    }
    pub fn clear_layout_preview(&self) {
        match registry::hooks(WindowRole::Preview) {
            Ok(hooks) => (hooks.clear_preview)(self),
            Err(_) => self.clear_preview_native(),
        }
    }
    fn clear_preview_native(&self) {
        self.state.clear_layout_preview();
        if let Err(error) = layout(self.hwnd, &self.state) {
            self.state.fail(error);
        }
    }
    pub fn render(&self, view: &View) -> Result<()> {
        self.render_for(WindowRole::Workbench, view)
    }
    pub fn render_for(&self, role: WindowRole, view: &View) -> Result<()> {
        let hooks = registry::hooks(role)?;
        (hooks.bind)(view)?;
        let changed = (hooks.render)(self, view)?;
        self.role.set(role);
        if changed {
            (hooks.measure)(self)?;
        }
        Ok(())
    }
    fn render_native(&self, view: &View) -> Result<bool> {
        let old = self
            .state
            .view
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .clone();
        if old.as_ref() == Some(view) {
            return Ok(false);
        }
        let _suppress = Suppress::new(&self.state);
        if old.as_ref().is_some_and(|old| old.grid != view.grid) {
            self.state.grid_cell.set(None);
        }
        // A new page/status never retains a previously revealed recovery secret.
        // Exactly equal rerenders preserve drafts and an explicitly presented code.
        self.state.clear_passwords();
        let same_structure =
            old.as_ref().is_some_and(|old| {
                old.fields.len() == view.fields.len()
                    && old.actions.len() == view.actions.len()
                    && old.fields.iter().zip(&view.fields).all(|(a, b)| {
                        a.id == b.id && a.kind == b.kind && a.max_utf16 == b.max_utf16
                    })
                    && old
                        .actions
                        .iter()
                        .zip(&view.actions)
                        .all(|(a, b)| a.id == b.id)
            }) && old
                .as_ref()
                .is_some_and(|old| old.grid.is_some() == view.grid.is_some());
        // ComboBox options may change when selecting a different node on the
        // same tree page. Their ordinary controls can be rebuilt independently
        // without discarding the image or a live canvas drag/capture.
        let page_changed = old.as_ref().is_some_and(|old| {
            old.title != view.title
                || old.description != view.description
                || old
                    .fields
                    .iter()
                    .map(|f| f.id)
                    .ne(view.fields.iter().map(|f| f.id))
                || old
                    .actions
                    .iter()
                    .map(|a| a.id)
                    .ne(view.actions.iter().map(|a| a.id))
                || old.grid.is_some() != view.grid.is_some()
        });
        if page_changed {
            self.state.clear_layout_preview();
        } else if view.busy {
            if let Some(canvas) = self.state.preview.borrow().as_ref() {
                canvas.cancel();
            }
            self.state.discard_layout_events();
        }
        if !same_structure {
            self.state.clear_passwords();
            let controls = self.build_controls(view)?;
            let old_controls = self.state.controls.replace(controls);
            drop(old_controls);
            self.state.scroll.set(0);
            self.state.grid_cell.set(None);
        }
        let (fields, actions, labels, description, status) = {
            let c = self
                .state
                .controls
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
            (
                c.fields.clone(),
                c.actions.clone(),
                c.labels.clone(),
                c.description,
                c.status,
            )
        };
        // SAFETY: synchronous updates target only owned controls, with no live
        // RefCell mutable borrow across possible reentrant messages.
        unsafe {
            SetWindowTextW(self.hwnd, &HSTRING::from(view.title.as_str()))?;
            SetWindowTextW(description, &HSTRING::from(view.description.as_str()))?;
            SetWindowTextW(status, &HSTRING::from(view.status.as_str()))?;
            for (index, (binding, field)) in fields.iter().zip(&view.fields).enumerate() {
                SetWindowTextW(labels[index], &HSTRING::from(field.label.as_str()))?;
                if same_structure
                    && field.kind != FieldKind::Password
                    && old
                        .as_ref()
                        .is_some_and(|old| old.fields[index].value != field.value)
                {
                    if let FieldKind::Choice(options) = &field.kind {
                        set_choice(binding.hwnd, options, &field.value)?;
                    } else {
                        SetWindowTextW(binding.hwnd, &HSTRING::from(field.value.as_str()))?;
                    }
                }
                let _ = EnableWindow(binding.hwnd, !view.busy);
            }
            for (binding, action) in actions.iter().zip(&view.actions) {
                SetWindowTextW(binding.hwnd, &HSTRING::from(action.label.as_str()))?;
                let _ = EnableWindow(
                    binding.hwnd,
                    action.enabled
                        && (!view.busy || self.state.lock_action.get() == Some(action.id)),
                );
            }
        }
        if let Some(grid) = &view.grid {
            let controls = self
                .state
                .controls
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
            if let Some(control) = &controls.grid {
                control.update(grid, view.busy)?;
            }
        }
        self.state.view.replace(Some(view.clone()));
        Ok(true)
    }
    fn measure_native(&self) -> Result<()> {
        layout(self.hwnd, &self.state)
    }
    fn build_controls(&self, view: &View) -> Result<Controls> {
        let mut controls = Controls::default();
        controls.description = self.child(
            &mut controls,
            w!("STATIC"),
            &view.description,
            WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0),
            0,
        )?;
        controls.status = self.child(
            &mut controls,
            w!("STATIC"),
            &view.status,
            WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0),
            0,
        )?;
        for (index, field) in view.fields.iter().enumerate() {
            let label = self.child(
                &mut controls,
                w!("STATIC"),
                &field.label,
                WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0),
                0,
            )?;
            controls.labels.push(label);
            let kind = match &field.kind {
                FieldKind::Text => ES_AUTOHSCROLL,
                FieldKind::Password => ES_AUTOHSCROLL | ES_PASSWORD,
                FieldKind::Multiline => ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN,
                FieldKind::ReadOnly => ES_AUTOHSCROLL | ES_READONLY,
                FieldKind::Choice(_) => 0,
                FieldKind::TimeRange => ES_AUTOHSCROLL,
            };
            let mut style = WS_BORDER | WS_TABSTOP | WINDOW_STYLE(kind as u32);
            if field.kind == FieldKind::Multiline {
                style |= WS_VSCROLL;
            }
            let native_id = FIELD_ID_BASE + index as u16;
            let hwnd = if let FieldKind::Choice(options) = &field.kind {
                let hwnd = self.child(
                    &mut controls,
                    w!("COMBOBOX"),
                    "",
                    WS_TABSTOP
                        | WS_VSCROLL
                        | WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_HASSTRINGS) as u32),
                    native_id,
                )?;
                set_choice(hwnd, options, &field.value)?;
                hwnd
            } else {
                self.child(
                    &mut controls,
                    w!("EDIT"),
                    if field.kind == FieldKind::TimeRange {
                        ""
                    } else {
                        &field.value
                    },
                    style,
                    native_id,
                )?
            };
            // SAFETY: exact owned Edit HWND and bounded numeric control messages.
            unsafe {
                let _ = SendMessageW(hwnd, EM_SETLIMITTEXT, Some(WPARAM(field.max_utf16)), None);
            }
            controls.fields.push(FieldBinding {
                id: field.id,
                kind: NativeKind::from(&field.kind),
                hwnd,
                native_id,
                max_utf16: field.max_utf16,
            });
            if field.kind == FieldKind::TimeRange {
                controls
                    .masks
                    .push(MaskControl::attach(hwnd, &field.value)?);
            }
        }
        if let Some(grid) = &view.grid {
            let hwnd = self.child(
                &mut controls,
                w!("STATIC"),
                "",
                WS_TABSTOP | WINDOW_STYLE(0x100),
                4000,
            )?;
            controls.grid = Some(GridControl::attach(
                hwnd,
                grid,
                &self.state,
                self.font.0,
                view.busy,
            )?);
        }
        for (index, action) in view.actions.iter().enumerate() {
            let native_id = ACTION_ID_BASE + index as u16;
            let hwnd = self.child(
                &mut controls,
                w!("BUTTON"),
                &action.label,
                WS_TABSTOP
                    | WINDOW_STYLE(if index == 0 {
                        BS_DEFPUSHBUTTON
                    } else {
                        BS_PUSHBUTTON
                    } as u32),
                native_id,
            )?;
            controls.actions.push(ActionBinding {
                id: action.id,
                hwnd,
                native_id,
            });
        }
        Ok(controls)
    }
    fn child(
        &self,
        controls: &mut Controls,
        class: PCWSTR,
        text: &str,
        style: WINDOW_STYLE,
        id: u16,
    ) -> Result<HWND> {
        // SAFETY: parent/font/class live on this UI thread; no secret initial text
        // is permitted by View::validate. Handle ownership is recorded immediately.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE | style,
                0,
                0,
                1,
                1,
                Some(self.hwnd),
                Some(HMENU(usize::from(id) as *mut _)),
                Some(self.class.0),
                None,
            )?
        };
        controls.owned.push(Child(hwnd));
        unsafe {
            let _ = SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(self.font.0 .0 as usize)),
                Some(LPARAM(0)),
            );
        }
        Ok(hwnd)
    }
    /// At most eight submissions plus coalesced Input/Close. Forms are owned and
    /// wiped on drop; native capture failures are surfaced without their contents.
    pub fn take_events(&self) -> Result<Vec<Event>> {
        (registry::hooks(self.role.get())?.input)(self)
    }
    fn input_native(&self) -> Result<Vec<Event>> {
        let error = self.state.error.replace(0);
        if error != 0 {
            self.state.discard_forms();
            self.state.clear_passwords();
            self.state.input.set(false);
            return Err(Error::from_hresult(windows::core::HRESULT(error)));
        }
        let mut output = Vec::with_capacity(MAX_SUBMITS + 3);
        if self.state.close.replace(false) {
            output.push(Event::Close);
        }
        if self.state.input.replace(false) {
            output.push(Event::Input);
        }
        if let Some((row, column)) = self.state.grid_cell.take() {
            output.push(Event::GridCell { row, column });
        }
        let generation = self
            .state
            .preview
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .as_ref()
            .and_then(|p| p.generation());
        let enabled = !self.state.suppress.get()
            && self
                .state
                .view
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
                .as_ref()
                .is_some_and(|v| !v.busy);
        for event in self
            .state
            .events
            .try_borrow_mut()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .drain(..)
        {
            let current = match &event {
                Event::LayoutSelect { generation: g, .. }
                | Event::LayoutDrop { generation: g, .. } => enabled && generation == Some(*g),
                _ => true,
            };
            if current {
                output.push(event)
            }
        }
        self.state.event_bytes.set(0);
        Ok(output)
    }
    /// One-time local result presentation only. The borrowed secret is copied
    /// directly into a controlled native field, never the cached non-secret View.
    /// ReadOnly displays it; Password remains masked. Page change/hide/close wipes
    /// its observable text/undo state. Windows control-internal heap copies are
    /// owned by the OS and cannot be claimed as cryptographically wiped here.
    pub fn present_secret(&self, id: FieldId, secret: &str) -> Result<()> {
        let binding = self
            .state
            .controls
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .fields
            .iter()
            .find(|field| field.id == id)
            .copied()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if !matches!(binding.kind, NativeKind::ReadOnly | NativeKind::Password)
            || secret.contains('\0')
            || secret.encode_utf16().count() > binding.max_utf16
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        self.state.clear_passwords();
        let _suppress = Suppress::new(&self.state);
        self.state
            .presented_secrets
            .try_borrow_mut()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .push(id);
        // Fixed-size allocation: no secret-bearing intermediate reallocations.
        let mut utf16 = WipingUtf16(vec![0; secret.encode_utf16().count() + 1]);
        for (output, unit) in utf16.0.iter_mut().zip(secret.encode_utf16()) {
            *output = unit;
        }
        // SAFETY: bounded uniquely owned terminated buffer lives through the
        // synchronous copy. Drop volatile-wipes this sole temporary UTF-16 copy.
        unsafe {
            SetWindowTextW(binding.hwnd, PCWSTR(utf16.0.as_mut_ptr()))?;
            let _ = SendMessageW(binding.hwnd, EM_EMPTYUNDOBUFFER, None, None);
        }
        Ok(())
    }
    pub fn show(&self, activate: bool) {
        // SAFETY: owned top-level HWND. Foreground request is only for the user's
        // explicit open action; hidden/test paths use activate=false or stay hidden.
        unsafe {
            let _ = ShowWindow(
                self.hwnd,
                if activate { SW_SHOW } else { SW_SHOWNOACTIVATE },
            );
            if activate {
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }
    /// Owned modal picker. The workbench cannot enqueue edits behind an unowned
    /// dialog; independent hidden-host controls still revoke WTS/power immediately.
    pub fn select_color(
        &self,
        initial: fc_domain::identity::Rgb,
    ) -> Result<Option<fc_domain::identity::Rgb>> {
        color_picker::choose(self.hwnd, initial)
    }
    pub fn select_material(&self) -> Result<Option<crate::materials::chooser::SelectedFile>> {
        crate::materials::chooser::select(Some(self.hwnd))
    }
    pub fn select_content_txt(&self) -> Result<Option<crate::materials::chooser::SelectedFile>> {
        crate::materials::chooser::select_content_txt(Some(self.hwnd))
    }
    pub fn select_content_images(&self) -> Result<Vec<crate::materials::chooser::SelectedFile>> {
        crate::materials::chooser::select_content_images(Some(self.hwnd))
    }
    pub fn hide(&self) {
        self.state.clear_passwords();
        self.state.discard_forms();
        // SAFETY: affects only this adapter's window.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
    pub fn is_visible(&self) -> bool {
        // SAFETY: unique owned live window handle.
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }
    /// Read-only policy for the explicitly opened temporary-lesson editor.
    /// Unknown or external foregrounds never retain its pending write intent.
    pub fn lesson_foreground_allowed(&self) -> bool {
        lesson_foreground_allowed()
    }
    /// Explicit isolated diagnostic only. This method refuses a visible window,
    /// fills only its own existing native Edit controls and invokes the same
    /// button notification path as the UI. It bypasses no model/worker validation
    /// and sends no global input or foreground request.
    pub fn probe_submit(&self, id: ActionId, mut form: FormData) -> Result<()> {
        if self.is_visible() || self.state.suppress.get() || self.state.capturing.get() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let view = self
            .state
            .view
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .clone()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        let lock = self.state.lock_action.get() == Some(id);
        if (view.busy && !lock)
            || !view
                .actions
                .iter()
                .any(|action| action.id == id && action.enabled)
        {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let (fields, action) = {
            let controls = self
                .state
                .controls
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
            (
                controls.fields.clone(),
                controls
                    .actions
                    .iter()
                    .find(|action| action.id == id)
                    .copied()
                    .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?,
            )
        };
        if lock {
            if !form.is_empty() {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            // SAFETY: this binding comes from this window's current owned controls.
            if !unsafe { IsWindowEnabled(action.hwnd) }.as_bool() {
                return Err(Error::from_hresult(E_ACCESSDENIED));
            }
            // SAFETY: exact current owned button notification, with no global
            // input/focus change. Normal WndProc performs the same lock checks.
            unsafe {
                let _ = SendMessageW(
                    self.hwnd,
                    WM_COMMAND,
                    Some(WPARAM(
                        usize::from(action.native_id) | ((BN_CLICKED as usize) << 16),
                    )),
                    Some(LPARAM(action.hwnd.0 as isize)),
                );
            }
            let events = self
                .state
                .events
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
            if self.state.error.get() != 0
                || events.len() != 1
                || !events.front().is_some_and(|event| {
                    matches!(event,
                    Event::Submit(action_id, data) if *action_id == id && data.is_empty())
                })
            {
                return Err(Error::from_hresult(E_UNEXPECTED));
            }
            return Ok(());
        }
        if self.state.empty_form_action.get() == Some(id) {
            if !form.is_empty() {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            // SAFETY: exact current owned button; this path cannot enable it.
            if !unsafe { IsWindowEnabled(action.hwnd) }.as_bool() {
                return Err(Error::from_hresult(E_ACCESSDENIED));
            }
            if self.state.error.get() != 0 {
                return Err(Error::from_hresult(E_UNEXPECTED));
            }
            let before = self
                .state
                .events
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
                .len();
            // SAFETY: same owned-button WM_COMMAND used by a real click; no
            // field values are read or replaced and no global input is sent.
            unsafe {
                let _ = SendMessageW(
                    self.hwnd,
                    WM_COMMAND,
                    Some(WPARAM(
                        usize::from(action.native_id) | ((BN_CLICKED as usize) << 16),
                    )),
                    Some(LPARAM(action.hwnd.0 as isize)),
                );
            }
            let events = self
                .state
                .events
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
            if self.state.error.get() != 0
                || events.len() != before + 1
                || !events.get(before).is_some_and(|event| {
                    matches!(event,
                    Event::Submit(action_id, data) if *action_id == id && data.is_empty())
                })
            {
                return Err(Error::from_hresult(E_UNEXPECTED));
            }
            return Ok(());
        }
        // Validate the entire supplied form before modifying even one native
        // control. Every editable field is required; readonly/unknown IDs remain
        // in FormData and are rejected, including fields with empty values.
        let mut prepared = Vec::with_capacity(fields.len());
        for field in fields {
            if field.kind == NativeKind::ReadOnly {
                continue;
            }
            let text = WipingText(
                form.take(field.id)
                    .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?,
            );
            if text.0.contains('\0') || text.0.encode_utf16().count() > field.max_utf16 {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            if field.kind == NativeKind::Choice {
                let allowed =
                    view.fields
                        .iter()
                        .find(|f| f.id == field.id)
                        .is_some_and(|f| match &f.kind {
                            FieldKind::Choice(options) => {
                                text.0.is_empty() || options.iter().any(|c| c.value == text.0)
                            }
                            _ => false,
                        });
                if !allowed {
                    return Err(Error::from_hresult(E_INVALIDARG));
                }
            }
            if field.kind == NativeKind::TimeRange
                && !text.0.is_empty()
                && (text.0.len() != 8 || !text.0.bytes().all(|b| b.is_ascii_digit() || b == b'X'))
            {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            prepared.push((field, text));
        }
        if !form.is_empty() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        // SAFETY: native enablement is checked in addition to the immutable View;
        // these handles belong to this UI-thread-owned workbench only.
        if !unsafe { IsWindowEnabled(action.hwnd) }.as_bool()
            || prepared
                .iter()
                .any(|(field, _)| !unsafe { IsWindowEnabled(field.hwnd) }.as_bool())
        {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        if self.state.error.get() != 0 {
            return Err(Error::from_hresult(E_UNEXPECTED));
        }
        struct Clear<'a>(&'a State);
        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                self.0.clear_passwords();
            }
        }
        let _clear = Clear(&self.state);
        let before = self
            .state
            .events
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .len();
        for (field, text) in prepared {
            if field.kind == NativeKind::Choice {
                let options = view
                    .fields
                    .iter()
                    .find(|f| f.id == field.id)
                    .and_then(|f| match &f.kind {
                        FieldKind::Choice(c) => Some(c),
                        _ => None,
                    })
                    .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
                set_choice(field.hwnd, options, &text.0)?;
                continue;
            }
            let mut utf16 = WipingUtf16(vec![0; text.0.encode_utf16().count() + 1]);
            for (slot, unit) in utf16.0.iter_mut().zip(text.0.encode_utf16()) {
                *slot = unit;
            }
            // SAFETY: uniquely owned bounded terminated temporary; synchronous
            // copy into our existing Edit, followed by volatile-wiping both owned
            // temporary encodings. Actual EN_CHANGE traverses the normal WndProc.
            unsafe {
                SetWindowTextW(field.hwnd, PCWSTR(utf16.0.as_mut_ptr()))?;
            }
        }
        // SAFETY: exact button HWND/id pair from current owned controls. This is
        // a local notification to our hidden window, never desktop input synthesis.
        unsafe {
            let _ = SendMessageW(
                self.hwnd,
                WM_COMMAND,
                Some(WPARAM(
                    usize::from(action.native_id) | ((BN_CLICKED as usize) << 16),
                )),
                Some(LPARAM(action.hwnd.0 as isize)),
            );
        }
        let error = self.state.error.get();
        if error != 0 {
            self.state.discard_forms();
            return Err(Error::from_hresult(windows::core::HRESULT(error)));
        }
        if self
            .state
            .events
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .len()
            != before + 1
        {
            return Err(Error::from_hresult(E_UNEXPECTED));
        }
        Ok(())
    }
    pub fn draft_nonsecret(&self) -> Result<FormData> {
        let fields = self
            .state
            .controls
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .fields
            .clone();
        let mut result = FormData::new();
        for field in fields {
            if !matches!(field.kind, NativeKind::Password | NativeKind::ReadOnly) {
                result
                    .insert(field.id, self.state.read_field(field)?)
                    .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
            }
        }
        Ok(result)
    }
    pub fn probe_select_grid(&self, row: usize, column: usize) -> Result<()> {
        if self.is_visible() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let (hwnd, x, y) = {
            let controls = self
                .state
                .controls
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
            controls
                .grid
                .as_ref()
                .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?
                .point(row, column)?
        };
        // SAFETY: bounded coordinates target only this hidden owned grid.
        unsafe {
            let _ = SendMessageW(
                hwnd,
                WM_LBUTTONDOWN,
                Some(WPARAM(0)),
                Some(LPARAM(
                    (x as u16 as usize | ((y as u16 as usize) << 16)) as isize,
                )),
            );
        }
        Ok(())
    }
    /// Hidden diagnostic edit of an existing non-secret Text field. Incomplete
    /// drafts are allowed, as for actual typing; model validation is unchanged.
    /// Does not submit a form, reveal a password or synthesize global input.
    pub fn probe_edit(&self, id: FieldId, value: &str) -> Result<()> {
        if self.is_visible() || self.state.suppress.get() || self.state.capturing.get() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let current = self
            .state
            .view
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        let view = current
            .as_ref()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if view.busy {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let model = view
            .fields
            .iter()
            .find(|f| f.id == id && f.kind == FieldKind::Text)
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if value.contains('\0') || value.encode_utf16().count() > model.max_utf16 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let field = self
            .state
            .controls
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .fields
            .iter()
            .find(|f| f.id == id && f.kind == NativeKind::Text)
            .copied()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        // Release the immutable View borrow before the synchronous native edit
        // can dispatch EN_CHANGE back through our normal window procedure.
        drop(current);
        // SAFETY: exact current owned Text Edit HWND on this UI thread; bounded
        // non-secret text only, with no focus/global keyboard or mouse change.
        unsafe {
            if !IsWindowEnabled(field.hwnd).as_bool() {
                return Err(Error::from_hresult(E_ACCESSDENIED));
            }
            SetWindowTextW(field.hwnd, &HSTRING::from(value))?;
        }
        Ok(())
    }
    /// Hidden diagnostic selection through the native ComboBox notification path.
    pub fn probe_choice(&self, id: FieldId, value: &str) -> Result<()> {
        if self.is_visible() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let view = self
            .state
            .view
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .clone()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if view.busy {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let options = view
            .fields
            .iter()
            .find(|f| f.id == id)
            .and_then(|f| match &f.kind {
                FieldKind::Choice(options) => Some(options),
                _ => None,
            })
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if !options.iter().any(|choice| choice.value == value) {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let field = self
            .state
            .controls
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .fields
            .iter()
            .find(|field| field.id == id)
            .copied()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        // SAFETY: only this owned enabled ComboBox is changed, no focus/global input.
        unsafe {
            if !IsWindowEnabled(field.hwnd).as_bool() {
                return Err(Error::from_hresult(E_ACCESSDENIED));
            }
            set_choice(field.hwnd, options, value)?;
            let _ = SendMessageW(
                self.hwnd,
                WM_COMMAND,
                Some(WPARAM(
                    field.native_id as usize | ((CBN_SELCHANGE as usize) << 16),
                )),
                Some(LPARAM(field.hwnd.0 as isize)),
            );
        }
        Ok(())
    }
}

fn set_choice(
    hwnd: HWND,
    options: &[fc_presentation::workbench::Choice],
    value: &str,
) -> Result<()> {
    let index = if value.is_empty() {
        None
    } else {
        Some(
            options
                .iter()
                .position(|c| c.value == value)
                .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?,
        )
    };
    // SAFETY: owned strings copied synchronously by our unsorted ComboBox.
    unsafe {
        let _ = SendMessageW(hwnd, CB_RESETCONTENT, None, None);
        for option in options {
            let text = HSTRING::from(option.label.as_str());
            if SendMessageW(
                hwnd,
                CB_ADDSTRING,
                None,
                Some(LPARAM(text.as_ptr() as isize)),
            )
            .0 < 0
            {
                return Err(Error::from_hresult(E_OUTOFMEMORY));
            }
        }
        let _ = SendMessageW(
            hwnd,
            CB_SETCURSEL,
            Some(WPARAM(index.unwrap_or(usize::MAX))),
            None,
        );
    }
    Ok(())
}
impl Drop for Window {
    fn drop(&mut self) {
        // Static validation happens before creation. Even a broken descriptor
        // must never strand an owned HWND during unwinding.
        match registry::hooks(self.role.get()) {
            Ok(hooks) => (hooks.dispose)(self),
            Err(_) => self.dispose_native(),
        }
    }
}
impl Window {
    fn dispose_native(&mut self) {
        WORKBENCHES.with(|windows| windows.borrow_mut().retain(|hwnd| *hwnd != self.hwnd));
        self.state.suppress.set(true);
        self.state.clear_passwords();
        self.state.discard_forms();
        let controls = self.state.controls.replace(Controls::default());
        drop(controls);
        if !self.hwnd.is_invalid() {
            // SAFETY: children gone, State and font still live; NCDESTROY detaches data.
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

fn layout(hwnd: HWND, state: &State) -> Result<()> {
    let Some(view) = state
        .view
        .try_borrow()
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
        .clone()
    else {
        return Ok(());
    };
    let (fields, actions, labels, description, status) = {
        let c = state
            .controls
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        (
            c.fields.clone(),
            c.actions.clone(),
            c.labels.clone(),
            c.description,
            c.status,
        )
    };
    let mut client = RECT::default();
    // SAFETY: exclusive local output; child positions prepared without mutable UI borrows.
    unsafe {
        GetClientRect(hwnd, &mut client)?;
    }
    let width = (client.right - client.left - 40).max(160);
    let mut positions = vec![(description, 20, 16, width, 58)];
    let mut y = 82;
    if let Some(canvas) = state
        .preview
        .try_borrow()
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
        .as_ref()
    {
        let (canvas_width, height) = canvas.size(width);
        positions.push((canvas.hwnd(), 20, y, canvas_width, height));
        y += height + 20;
    }

    if let Some(grid) = state
        .controls
        .try_borrow()
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
        .grid
        .as_ref()
    {
        let height = grid.height();
        positions.push((grid.hwnd(), 20, y, width, height));
        y += height + 20;
    }
    for (index, (binding, field)) in fields.iter().zip(&view.fields).enumerate() {
        positions.push((labels[index], 20, y, width, 25));
        let height = if field.kind == FieldKind::Multiline {
            88
        } else {
            30
        };
        positions.push((
            binding.hwnd,
            20,
            y + 28,
            width,
            if matches!(field.kind, FieldKind::Choice(_)) {
                240
            } else {
                height
            },
        ));
        y += height + 42;
    }
    positions.push((status, 20, y + 4, width, 58));
    y += 76;
    let columns = (width / 200).max(1);
    let button_width = (width / columns - 10).max(80);
    for (index, action) in actions.iter().enumerate() {
        positions.push((
            action.hwnd,
            20 + index as i32 % columns * (button_width + 10),
            y + index as i32 / columns * 44,
            button_width,
            34,
        ));
    }
    let rows = (actions.len() as i32 + columns - 1) / columns;
    let content_height = y + rows * 44 + 20;
    let viewport = (client.bottom - client.top).max(1);
    let scroll = state
        .scroll
        .get()
        .clamp(0, (content_height - viewport).max(0));
    state.scroll.set(scroll);
    let info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: content_height - 1,
        nPage: viewport as u32,
        nPos: scroll,
        ..Default::default()
    };
    // SAFETY: only owned controls, copied positions, no live mutable state borrow.
    unsafe {
        let _ = SetScrollInfo(hwnd, SB_VERT, &info, true);
        for (child, x, y, w, h) in positions {
            MoveWindow(child, x, y - scroll, w, h, true)?;
        }
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Windows supplies CREATESTRUCT during creation; userdata holds
        // our stable Box until NCDESTROY. Only shared references/interior short
        // borrows exist, never a &mut crossing a reentrant system call.
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
                match message {
                    WM_CLOSE => {
                        state.clear_passwords();
                        state.discard_forms();
                        state.close.set(true);
                        let _ = ShowWindow(hwnd, SW_HIDE);
                        return LRESULT(0);
                    }
                    WM_SIZE => {
                        if let Err(error) = layout(hwnd, state) {
                            state.fail(error);
                        }
                        return LRESULT(0);
                    }
                    WM_VSCROLL => {
                        let request = wparam.0 as u16 as u32;
                        let mut info = SCROLLINFO {
                            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                            fMask: SIF_ALL,
                            ..Default::default()
                        };
                        if GetScrollInfo(hwnd, SB_VERT, &mut info).is_ok() {
                            let position = match request {
                                0 => info.nPos - 40,
                                1 => info.nPos + 40,
                                2 => info.nPos - info.nPage as i32,
                                3 => info.nPos + info.nPage as i32,
                                4 | 5 => info.nTrackPos,
                                6 => info.nMin,
                                7 => info.nMax,
                                _ => info.nPos,
                            };
                            state.scroll.set(position);
                            if let Err(error) = layout(hwnd, state) {
                                state.fail(error);
                            }
                        }
                        return LRESULT(0);
                    }
                    WM_MOUSEWHEEL => {
                        let delta = ((wparam.0 >> 16) as u16) as i16;
                        state
                            .scroll
                            .set(state.scroll.get() - i32::from(delta) / 120 * 120);
                        if let Err(error) = layout(hwnd, state) {
                            state.fail(error);
                        }
                        return LRESULT(0);
                    }
                    WM_COMMAND if !state.suppress.get() => {
                        let native_id = wparam.0 as u16;
                        let notification = ((wparam.0 >> 16) & 0xffff) as u32;
                        let source = HWND(lparam.0 as *mut _);
                        let (field, action) = match state.controls.try_borrow() {
                            Ok(c) => (
                                c.fields
                                    .iter()
                                    .find(|f| f.native_id == native_id && f.hwnd == source)
                                    .copied(),
                                c.actions
                                    .iter()
                                    .find(|a| a.native_id == native_id && a.hwnd == source)
                                    .copied(),
                            ),
                            Err(_) => {
                                state.error.set(E_UNEXPECTED.0);
                                return LRESULT(0);
                            }
                        };
                        if field.is_some()
                            && (notification == EN_CHANGE || notification == CBN_SELCHANGE)
                        {
                            state.input.set(true);
                        }
                        if let Some(action) = action {
                            if notification == BN_CLICKED
                                && IsWindowEnabled(action.hwnd).as_bool()
                                && state.action_allowed(action.id).unwrap_or(false)
                            {
                                state.input.set(true);
                                if let Err(error) = state.submit(action) {
                                    state.fail(error);
                                }
                            }
                        }
                        return LRESULT(0);
                    }
                    _ => {}
                }
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
    }));
    match result {
        Ok(result) => result,
        Err(_) => {
            // SAFETY: fail this process's UI pump without unwinding across Win32 ABI.
            unsafe {
                PostQuitMessage(1);
            }
            LRESULT(0)
        }
    }
}

#[cfg(test)]
#[path = "workbench/empty_action_tests.rs"]
mod empty_action_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NATIVE_TEST_SERIAL;
    use fc_presentation::workbench::{Action, Field};

    fn view() -> View {
        View {
            title: "未来教室 · 合成测试".into(),
            description: "本测试窗口始终隐藏".into(),
            status: String::new(),
            busy: false,
            grid: None,
            fields: vec![
                Field {
                    id: FieldId(1),
                    label: "用户名".into(),
                    kind: FieldKind::Text,
                    value: "Default".into(),
                    max_utf16: 32,
                },
                Field {
                    id: FieldId(2),
                    label: "密码".into(),
                    kind: FieldKind::Password,
                    value: String::new(),
                    max_utf16: 128,
                },
                Field {
                    id: FieldId(3),
                    label: "说明".into(),
                    kind: FieldKind::Multiline,
                    value: String::new(),
                    max_utf16: 1024,
                },
                Field {
                    id: FieldId(4),
                    label: "只读".into(),
                    kind: FieldKind::ReadOnly,
                    value: String::new(),
                    max_utf16: 128,
                },
            ],
            actions: vec![Action {
                id: ActionId(1),
                label: "提交".into(),
                enabled: true,
            }],
        }
    }
    fn field(window: &Window, id: u16) -> FieldBinding {
        *window
            .state
            .controls
            .borrow()
            .fields
            .iter()
            .find(|f| f.id == FieldId(id))
            .unwrap()
    }
    fn button(window: &Window) -> ActionBinding {
        window.state.controls.borrow().actions[0]
    }
    fn set(field: FieldBinding, text: &str) {
        let text = HSTRING::from(text); // synthetic, never production credential data
                                        // SAFETY: targets our own hidden test control, never global keyboard input.
        unsafe {
            SetWindowTextW(field.hwnd, &text).unwrap();
        }
    }
    fn click(window: &Window) {
        let button = button(window);
        // SAFETY: exact owned button HWND/id pair; this is a direct local command
        // to our hidden window, not input simulation or foreground manipulation.
        unsafe {
            let _ = SendMessageW(
                window.hwnd,
                WM_COMMAND,
                Some(WPARAM(
                    usize::from(button.native_id) | ((BN_CLICKED as usize) << 16),
                )),
                Some(LPARAM(button.hwnd.0 as isize)),
            );
        }
    }
    fn notify(window: &Window, button: ActionBinding) {
        // SAFETY: direct notification to this test's hidden owned window only.
        unsafe {
            let _ = SendMessageW(
                window.hwnd,
                WM_COMMAND,
                Some(WPARAM(
                    usize::from(button.native_id) | ((BN_CLICKED as usize) << 16),
                )),
                Some(LPARAM(button.hwnd.0 as isize)),
            );
        }
    }
    fn lock_view() -> View {
        let mut view = view();
        view.actions.push(Action {
            id: ActionId(2),
            label: "锁定".into(),
            enabled: true,
        });
        view
    }
    #[test]
    fn registered_roles_keep_identical_view_drafts_and_dispose_the_original_window() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let foreground = unsafe { GetForegroundWindow() };
        let count = WORKBENCHES.with(|windows| windows.borrow().len());
        let hwnd;
        {
            let window = Window::new().unwrap();
            hwnd = window.hwnd;
            let model = view();
            window.render_for(WindowRole::Workbench, &model).unwrap();
            set(field(&window, 2), "synthetic-role-draft");
            for role in WindowRole::FORMS {
                window.render_for(role, &model).unwrap();
                assert_eq!(
                    read_edit(field(&window, 2)).unwrap(),
                    "synthetic-role-draft"
                );
                assert_eq!(window.role.get(), role);
                assert!(window
                    .take_events()
                    .unwrap()
                    .iter()
                    .all(|event| !matches!(event, Event::Submit(_, _))));
            }
            assert_eq!(
                WORKBENCHES.with(|windows| windows.borrow().len()),
                count + 1
            );
        }
        assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool());
        assert_eq!(WORKBENCHES.with(|windows| windows.borrow().len()), count);
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
    }
    #[test]
    fn hidden_multiline_edit_preserves_10000_astral_scalars_without_truncation() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let foreground = unsafe { GetForegroundWindow() };
        let window = Window::new().unwrap();
        let mut model = view();
        model.fields[2].max_utf16 = 20_000;
        window.render(&model).unwrap();
        let text = "😀".repeat(10_000);
        set(field(&window, 3), &text);
        assert_eq!(read_edit(field(&window, 3)).unwrap(), text);
        click(&window);
        let mut submitted = window
            .take_events()
            .unwrap()
            .into_iter()
            .find_map(|event| match event {
                Event::Submit(ActionId(1), data) => Some(data),
                _ => None,
            })
            .expect("own hidden control submit");
        assert_eq!(submitted.take(FieldId(3)).unwrap(), text);
        assert!(!window.is_visible());
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
    }
    #[test]
    fn registered_lock_during_busy_emits_only_empty_event_and_clears_secrets_without_reading_fields(
    ) {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: observes foreground only; all windows below remain hidden.
        let foreground = unsafe { GetForegroundWindow() };
        let window = Window::new().unwrap();
        assert!(window.set_lock_action(ActionId(0)).is_err());
        window.set_lock_action(ActionId(2)).unwrap();
        window.set_lock_action(ActionId(2)).unwrap();
        assert!(window.set_lock_action(ActionId(3)).is_err());
        let mut view = lock_view();
        window.render(&view).unwrap();
        // Lock must supersede even a full queue of old secret-bearing forms.
        for _ in 0..MAX_SUBMITS {
            set(field(&window, 2), "SyntheticQueuedPassword");
            click(&window);
        }
        view.busy = true;
        window.render(&view).unwrap();
        set(field(&window, 1), &"x".repeat(33)); // invalid if any form read occurs
        set(field(&window, 2), "SyntheticBusyPassword");
        window
            .present_secret(FieldId(4), "SyntheticRecovery")
            .unwrap();
        assert!(window.probe_submit(ActionId(1), FormData::new()).is_err());
        let mut supplied = FormData::new();
        supplied
            .insert(FieldId(2), "MustNotEnterLockEvent".into())
            .unwrap();
        assert!(window.probe_submit(ActionId(2), supplied).is_err());
        window.probe_submit(ActionId(2), FormData::new()).unwrap();
        let mut submits = window
            .take_events()
            .unwrap()
            .into_iter()
            .filter_map(|e| match e {
                Event::Submit(id, data) => Some((id, data)),
                _ => None,
            });
        let (id, data) = submits.next().unwrap();
        assert_eq!(id, ActionId(2));
        assert!(data.is_empty());
        assert!(submits.next().is_none());
        assert_eq!(read_edit(field(&window, 2)).unwrap(), "");
        assert_eq!(read_edit(field(&window, 4)).unwrap(), "");
        // This field deliberately exceeds its configured bound. The ordinary
        // bounded reader must still reject it after Lock; observe only its
        // native length to prove Lock did not read or truncate the text.
        assert!(read_edit(field(&window, 1)).is_err());
        // SAFETY: read only the length of this hidden fixture's nonsecret Edit.
        assert_eq!(unsafe { GetWindowTextLengthW(field(&window, 1).hwnd) }, 33);
        assert!(!window.is_visible());
        // SAFETY: no mutation of the observed process foreground.
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
    }
    #[test]
    fn lock_requires_registration_enabled_current_view_and_exact_native_binding() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let window = Window::new().unwrap();
        let mut view = lock_view();
        view.busy = true;
        window.render(&view).unwrap();
        let lock = window.state.controls.borrow().actions[1];
        assert!(window.probe_submit(ActionId(2), FormData::new()).is_err());
        notify(&window, lock);
        assert!(window.take_events().unwrap().is_empty());
        // Registration after a render enables only this exact current button.
        window.set_lock_action(ActionId(2)).unwrap();
        set(field(&window, 2), "SyntheticNativeLock");
        notify(&window, lock);
        assert!(window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Submit(ActionId(2), data) if data.is_empty())));
        assert_eq!(read_edit(field(&window, 2)).unwrap(), "");
        assert!(window.probe_submit(ActionId(99), FormData::new()).is_err());
        view.actions[1].enabled = false;
        window.render(&view).unwrap();
        assert!(window.probe_submit(ActionId(2), FormData::new()).is_err());
        notify(&window, lock);
        assert!(window.take_events().unwrap().is_empty());
        view.actions.pop();
        window.render(&view).unwrap();
        assert!(window.probe_submit(ActionId(2), FormData::new()).is_err());
        notify(&window, lock); // old destroyed button is not in the current binding table
        assert!(window.take_events().unwrap().is_empty());
        set(field(&window, 2), "SyntheticBusyClose");
        // SAFETY: close only this hidden owned window, without quitting the UI thread.
        unsafe {
            let _ = SendMessageW(window.hwnd, WM_CLOSE, None, None);
        }
        assert_eq!(read_edit(field(&window, 2)).unwrap(), "");
        assert!(window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Close)));
    }
    #[test]
    fn hidden_probe_edit_preserves_half_date_through_empty_lock_and_refuses_secret_or_busy_fields()
    {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let window = Window::new().unwrap();
        window.set_lock_action(ActionId(2)).unwrap();
        let mut view = lock_view();
        window.render(&view).unwrap();
        window.probe_edit(FieldId(1), "2026-1").unwrap();
        assert!(window.probe_edit(FieldId(2), "ForbiddenPassword").is_err());
        assert!(window.probe_edit(FieldId(3), "ForbiddenMultiline").is_err());
        assert!(window.probe_edit(FieldId(4), "ForbiddenReadonly").is_err());
        assert!(window.probe_edit(FieldId(99), "Unknown").is_err());
        assert!(window.probe_edit(FieldId(1), "bad\0text").is_err());
        assert!(window.probe_edit(FieldId(1), &"x".repeat(33)).is_err());
        set(field(&window, 2), "SyntheticPasswordNotCopiedByLock");
        window
            .present_secret(FieldId(4), "SyntheticOneTimeRecovery")
            .unwrap();
        let mut old_style = FormData::new();
        old_style
            .insert(FieldId(1), "MustNotBeLockPayload".into())
            .unwrap();
        assert!(window.probe_submit(ActionId(2), old_style).is_err());
        window.probe_submit(ActionId(2), FormData::new()).unwrap();
        let draft = window.draft_nonsecret().unwrap();
        assert_eq!(draft.get(FieldId(1)), Some("2026-1"));
        assert!(draft.get(FieldId(2)).is_none());
        assert_eq!(read_edit(field(&window, 2)).unwrap(), "");
        assert_eq!(read_edit(field(&window, 4)).unwrap(), "");
        assert!(window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Submit(ActionId(2), data) if data.is_empty())));
        view.busy = true;
        window.render(&view).unwrap();
        assert!(window.probe_edit(FieldId(1), "busy").is_err());
        assert_eq!(
            window.draft_nonsecret().unwrap().get(FieldId(1)),
            Some("2026-1")
        );
        view.busy = false;
        view.fields.remove(0);
        window.render(&view).unwrap();
        assert!(window.probe_edit(FieldId(1), "RemovedViewField").is_err());
        assert!(!window.is_visible());
    }
    #[test]
    fn hidden_native_form_preserves_drafts_masks_and_consumes_password_without_activation() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: observational only, no foreground mutation.
        let foreground = unsafe { GetForegroundWindow() };
        let window = Window::new().unwrap();
        let view = view();
        window.render(&view).unwrap();
        assert!(!window.is_visible());
        let username = field(&window, 1);
        let password = field(&window, 2);
        let multiline = field(&window, 3);
        // SAFETY: reads only owned Edit metadata; ES_PASSWORD must be applied by
        // class style independently of any custom renderer or placeholder text.
        unsafe {
            assert_ne!(
                GetWindowLongPtrW(password.hwnd, GWL_STYLE) as i64 & i64::from(ES_PASSWORD),
                0
            );
            assert_ne!(
                SendMessageW(password.hwnd, EM_GETPASSWORDCHAR, None, None).0,
                0
            );
        }
        set(username, "AbC中文用户名");
        set(password, "Synthetic密语aBc123");
        set(multiline, "第一行\r\nSecond Line");
        window.render(&view).unwrap();
        assert_eq!(field(&window, 1).hwnd, username.hwnd);
        assert_eq!(read_edit(username).unwrap(), "AbC中文用户名");
        assert_eq!(read_edit(password).unwrap(), "Synthetic密语aBc123");
        click(&window);
        let events = window.take_events().unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::Input)));
        let form = events
            .into_iter()
            .find_map(|event| match event {
                Event::Submit(ActionId(1), form) => Some(form),
                _ => None,
            })
            .unwrap();
        assert_eq!(form.get(FieldId(1)), Some("AbC中文用户名"));
        assert_eq!(form.get(FieldId(2)), Some("Synthetic密语aBc123"));
        assert_eq!(form.get(FieldId(3)), Some("第一行\r\nSecond Line"));
        assert_eq!(form.get(FieldId(4)), None);
        assert_eq!(read_edit(password).unwrap(), "");
        // SAFETY: inspect own native undo state after consuming the password.
        assert_eq!(
            unsafe { SendMessageW(password.hwnd, EM_CANUNDO, None, None) }.0,
            0
        );
        assert!(!format!("{form:?}").contains("Synthetic"));
        assert_eq!(read_edit(username).unwrap(), "AbC中文用户名");
        // SAFETY: observational and process-local; all test actions stayed hidden.
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
    }
    #[test]
    fn busy_and_disabled_actions_refuse_commands_and_close_wipes_revealed_secret() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let window = Window::new().unwrap();
        let mut view = view();
        window.render(&view).unwrap();
        let mut busy = view.clone();
        busy.busy = true;
        window.render(&busy).unwrap();
        click(&window);
        assert!(!window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Submit(..))));
        view.actions[0].enabled = false;
        window.render(&view).unwrap();
        click(&window);
        assert!(!window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Submit(..))));
        view.actions[0].enabled = true;
        window.render(&view).unwrap();
        window
            .present_secret(FieldId(4), "FC1-Synthetic一显恢复码")
            .unwrap();
        assert_eq!(
            read_edit(field(&window, 4)).unwrap(),
            "FC1-Synthetic一显恢复码"
        );
        assert_eq!(
            window.state.view.borrow().as_ref().unwrap().fields[3].value,
            ""
        );
        window.render(&view).unwrap(); // unchanged view must not erase pending one-time display
        assert!(!read_edit(field(&window, 4)).unwrap().is_empty());
        assert!(window.present_secret(FieldId(1), "forbidden").is_err());
        assert!(window.present_secret(FieldId(4), &"x".repeat(129)).is_err());
        assert!(window.present_secret(FieldId(4), "bad\0text").is_err());
        set(field(&window, 2), "SyntheticToCancel");
        // SAFETY: own hidden top-level close, which must not quit this thread/process.
        unsafe {
            let _ = SendMessageW(window.hwnd, WM_CLOSE, None, None);
        }
        assert!(!window.is_visible());
        assert_eq!(read_edit(field(&window, 2)).unwrap(), "");
        assert_eq!(read_edit(field(&window, 4)).unwrap(), "");
        assert!(window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Close)));
        let mut message = MSG::default();
        // SAFETY: observe only this test thread's WM_QUIT; do not dispatch/alter others.
        assert!(
            !unsafe { PeekMessageW(&mut message, None, WM_QUIT, WM_QUIT, PM_NOREMOVE) }.as_bool()
        );
        window
            .present_secret(FieldId(4), "SyntheticRecoveryAgain")
            .unwrap();
        view.status = "新页面".into();
        window.render(&view).unwrap();
        assert_eq!(read_edit(field(&window, 4)).unwrap(), "");
        window
            .present_secret(FieldId(4), "SyntheticHidden")
            .unwrap();
        window.hide();
        assert_eq!(read_edit(field(&window, 4)).unwrap(), "");
    }
    #[test]
    fn hidden_events_are_bounded_and_invalid_native_text_is_never_truncated_into_a_form() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let window = Window::new().unwrap();
        window.render(&view()).unwrap();
        for _ in 0..MAX_SUBMITS {
            click(&window);
        }
        let events = window.take_events().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::Submit(..)))
                .count(),
            MAX_SUBMITS
        );
        for _ in 0..=MAX_SUBMITS {
            click(&window);
        }
        assert!(window.take_events().is_err());
        assert!(window.take_events().unwrap().is_empty());
        // WM_SETTEXT can bypass Edit's user-input limit, so the consumption path
        // independently validates length instead of silently truncating credentials.
        set(field(&window, 1), &"x".repeat(33));
        click(&window);
        assert!(window.take_events().is_err());
        assert!(window.take_events().unwrap().is_empty());
        set(field(&window, 1), "Native中文");
        click(&window);
        assert!(window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Submit(..))));
    }
    #[test]
    fn repeated_hidden_render_and_submit_do_not_accumulate_native_resources() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetGuiResources, GetProcessHandleCount, GR_GDIOBJECTS,
            GR_USEROBJECTS,
        };
        let window = Window::new().unwrap();
        let mut view = view();
        window.render(&view).unwrap();
        set(field(&window, 1), "RepeatedNative中文");
        click(&window);
        drop(window.take_events().unwrap());
        let counts = || {
            let mut handles = 0;
            // SAFETY: borrowed current-process pseudo handle and exclusive output;
            // these are measurements only, no handle ownership is transferred.
            unsafe {
                let process = GetCurrentProcess();
                GetProcessHandleCount(process, &mut handles).unwrap();
                (
                    handles,
                    GetGuiResources(process, GR_GDIOBJECTS),
                    GetGuiResources(process, GR_USEROBJECTS),
                )
            }
        };
        let before = counts();
        let stable = field(&window, 1).hwnd;
        for i in 0..100 {
            view.status = format!("本窗口合成循环 {i}");
            window.render(&view).unwrap();
            assert_eq!(field(&window, 1).hwnd, stable);
            click(&window);
            drop(window.take_events().unwrap());
        }
        let after = counts();
        assert!(
            after.0 <= before.0 + 2,
            "kernel handles {before:?} -> {after:?}"
        );
        assert!(after.1 <= before.1 + 1, "GDI {before:?} -> {after:?}");
        assert!(after.2 <= before.2 + 1, "USER {before:?} -> {after:?}");
    }
    #[test]
    fn isolated_probe_submission_uses_real_controls_and_refuses_invalid_forms_before_changes() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let window = Window::new().unwrap();
        let mut view = view();
        window.render(&view).unwrap();
        let form = || {
            let mut form = FormData::new();
            form.insert(FieldId(1), "DiagnosticAbC中文".into()).unwrap();
            form.insert(FieldId(2), "SyntheticProbeSecret".into())
                .unwrap();
            form.insert(FieldId(3), "第一行\r\nProbe Line".into())
                .unwrap();
            form
        };
        assert!(!window.is_visible());
        // SAFETY: observation only; the probe never shows or activates a window.
        let foreground = unsafe { GetForegroundWindow() };
        for rejected_id in [FieldId(4), FieldId(65000)] {
            let mut invalid = form();
            invalid.insert(rejected_id, String::new()).unwrap();
            assert!(window.probe_submit(ActionId(1), invalid).is_err());
            assert_eq!(read_edit(field(&window, 1)).unwrap(), "Default");
            assert!(window.take_events().unwrap().is_empty());
        }
        let mut missing = form();
        drop(WipingText(missing.take(FieldId(3)).unwrap()));
        assert!(window.probe_submit(ActionId(1), missing).is_err());
        let mut oversized = form();
        drop(WipingText(oversized.take(FieldId(1)).unwrap()));
        oversized.insert(FieldId(1), "x".repeat(33)).unwrap();
        assert!(window.probe_submit(ActionId(1), oversized).is_err());
        assert!(window.probe_submit(ActionId(65000), form()).is_err());
        view.busy = true;
        window.render(&view).unwrap();
        assert!(window.probe_submit(ActionId(1), form()).is_err());
        view.busy = false;
        view.actions[0].enabled = false;
        window.render(&view).unwrap();
        assert!(window.probe_submit(ActionId(1), form()).is_err());
        view.actions[0].enabled = true;
        window.render(&view).unwrap();
        window.probe_submit(ActionId(1), form()).unwrap();
        let submitted = window
            .take_events()
            .unwrap()
            .into_iter()
            .find_map(|event| match event {
                Event::Submit(ActionId(1), form) => Some(form),
                _ => None,
            })
            .unwrap();
        assert_eq!(submitted.get(FieldId(1)), Some("DiagnosticAbC中文"));
        assert_eq!(submitted.get(FieldId(2)), Some("SyntheticProbeSecret"));
        assert_eq!(submitted.get(FieldId(3)), Some("第一行\r\nProbe Line"));
        assert_eq!(read_edit(field(&window, 2)).unwrap(), "");
        assert!(!window.is_visible());
        // SAFETY: observation only, no global input used by any test operation.
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
    }
    fn timetable_view() -> View {
        use fc_presentation::workbench::{Choice, Grid, GridRow};
        let mut v = view();
        v.fields = vec![
            Field {
                id: FieldId(10),
                label: "时间".into(),
                kind: FieldKind::TimeRange,
                value: "08200900".into(),
                max_utf16: 8,
            },
            Field {
                id: FieldId(11),
                label: "科目".into(),
                kind: FieldKind::Choice(vec![
                    Choice {
                        value: "subject-1".into(),
                        label: "合成语文".into(),
                    },
                    Choice {
                        value: "subject-2".into(),
                        label: "合成数学".into(),
                    },
                ]),
                value: "subject-1".into(),
                max_utf16: 128,
            },
        ];
        v.grid = Some(Grid {
            columns: vec!["时段 / 时间".into(), "星期一".into(), "星期二".into()],
            rows: vec![
                GridRow {
                    group: "上午".into(),
                    label: "第1课\n08：20-09：00".into(),
                    cells: vec!["合成语文".into(), "合成数学".into()],
                    merged: false,
                },
                GridRow {
                    group: "上午".into(),
                    label: "活动".into(),
                    cells: vec!["跨星期合成活动".into()],
                    merged: true,
                },
                GridRow {
                    group: "下午".into(),
                    label: "第2课".into(),
                    cells: vec![String::new(), "合成语文".into()],
                    merged: false,
                },
            ],
            selected: None,
            enabled: true,
        });
        v
    }
    #[test]
    fn native_timetable_grid_and_combo_use_bounded_stable_selections() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let window = Window::new().unwrap();
        let mut view = timetable_view();
        window.render(&view).unwrap();
        window.probe_select_grid(0, 2).unwrap();
        let events = window.take_events().unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::GridCell { row: 0, column: 2 })));
        window.probe_select_grid(1, 2).unwrap();
        assert!(window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::GridCell { row: 1, column: 1 })));
        assert!(window.probe_select_grid(3, 0).is_err());
        window.probe_choice(FieldId(11), "subject-2").unwrap();
        assert!(window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Input)));
        assert_eq!(
            window.draft_nonsecret().unwrap().get(FieldId(11)),
            Some("subject-2")
        );
        assert!(window.probe_choice(FieldId(11), "合成数学").is_err());
        let grid_hwnd = window.state.controls.borrow().grid.as_ref().unwrap().hwnd();
        // SAFETY: keyboard notification only to our hidden owned grid, no focus/global input.
        unsafe {
            let _ = SendMessageW(
                grid_hwnd,
                WM_KEYDOWN,
                Some(WPARAM(VK_DOWN.0 as usize)),
                None,
            );
            let _ = SendMessageW(
                grid_hwnd,
                WM_KEYDOWN,
                Some(WPARAM(VK_RETURN.0 as usize)),
                None,
            );
        }
        assert!(window
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::GridCell { row: 2, column: 1 })));
        view.busy = true;
        window.render(&view).unwrap();
        assert!(window.probe_select_grid(0, 0).is_err());
        assert!(window.probe_choice(FieldId(11), "subject-1").is_err());
        view.busy = false;
        view.grid.as_mut().unwrap().enabled = false;
        window.render(&view).unwrap();
        assert!(window.probe_select_grid(0, 0).is_err());
        assert!(!window.is_visible());
    }
    #[test]
    fn native_numeric_time_mask_preserves_punctuation_segment_edits_and_partial_drafts() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let window = Window::new().unwrap();
        window.render(&timetable_view()).unwrap();
        let time = field(&window, 10);
        let display = || {
            let mut buffer = [0u16; 32];
            let count = unsafe { GetWindowTextW(time.hwnd, &mut buffer) };
            String::from_utf16(&buffer[..count as usize]).unwrap()
        };
        assert_eq!(display(), "08：20-09：00");
        // SAFETY: attempts to inject free text into this owned hidden mask must
        // not replace the digit model or its fixed punctuation.
        unsafe {
            let replacement = HSTRING::from("arbitrary text");
            let _ = SendMessageW(
                time.hwnd,
                EM_REPLACESEL,
                Some(WPARAM(0)),
                Some(LPARAM(replacement.as_ptr() as isize)),
            );
            let _ = SendMessageW(time.hwnd, WM_IME_CHAR, Some(WPARAM('课' as usize)), None);
        }
        assert_eq!(display(), "08：20-09：00");
        // SAFETY: select just the minute segment in our own hidden native Edit.
        unsafe {
            let _ = SendMessageW(time.hwnd, EM_SETSEL, Some(WPARAM(3)), Some(LPARAM(5)));
            let _ = SendMessageW(time.hwnd, WM_CHAR, Some(WPARAM('３' as usize)), None);
            let _ = SendMessageW(time.hwnd, WM_CHAR, Some(WPARAM('0' as usize)), None);
        }
        assert_eq!(display(), "08：30-09：00");
        assert_eq!(
            window.draft_nonsecret().unwrap().get(FieldId(10)),
            Some("08300900")
        );
        unsafe {
            let _ = SendMessageW(
                time.hwnd,
                WM_KEYDOWN,
                Some(WPARAM(VK_LEFT.0 as usize)),
                None,
            );
            let _ = SendMessageW(
                time.hwnd,
                WM_KEYDOWN,
                Some(WPARAM(VK_DELETE.0 as usize)),
                None,
            );
        }
        assert_eq!(
            window.draft_nonsecret().unwrap().get(FieldId(10)),
            Some("083X0900")
        );
        {
            let controls = window.state.controls.borrow();
            let mask = controls.masks.first().unwrap();
            mask.probe_paste("０８２００９００").unwrap();
            assert!(mask.probe_paste("08:20-09:00").is_err());
        }
        assert_eq!(display(), "08：20-09：00");
        unsafe {
            let _ = SendMessageW(time.hwnd, EM_SETSEL, Some(WPARAM(11)), Some(LPARAM(11)));
            let _ = SendMessageW(time.hwnd, WM_CHAR, Some(WPARAM(8)), None);
        }
        assert_eq!(
            window.draft_nonsecret().unwrap().get(FieldId(10)),
            Some("0820090X")
        );
        click(&window);
        let form = window
            .take_events()
            .unwrap()
            .into_iter()
            .find_map(|e| match e {
                Event::Submit(_, form) => Some(form),
                _ => None,
            })
            .unwrap();
        assert_eq!(form.get(FieldId(10)), Some("0820090X"));
        let mut busy = timetable_view();
        busy.busy = true;
        window.render(&busy).unwrap();
        let before = window.draft_nonsecret().unwrap().take(FieldId(10)).unwrap();
        unsafe {
            let _ = SendMessageW(time.hwnd, WM_CHAR, Some(WPARAM('9' as usize)), None);
        }
        assert_eq!(
            window.draft_nonsecret().unwrap().get(FieldId(10)),
            Some(before.as_str())
        );
    }
    #[test]
    fn repeated_grid_paint_and_dynamic_choice_mask_rebuilds_release_native_resources() {
        let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS, GR_USEROBJECTS,
        };
        let window = Window::new().unwrap();
        let mut view = timetable_view();
        window.render(&view).unwrap();
        let counts = || unsafe {
            (
                GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS),
                GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS),
            )
        };
        // Realize the font and native paint cache before measuring retained
        // handles; compare the next hundred rebuilds against an already painted UI.
        let grid = window.state.controls.borrow().grid.as_ref().unwrap().hwnd();
        unsafe {
            let _ = SendMessageW(grid, WM_PAINT, None, None);
        }
        let before = counts();
        for index in 0..100 {
            view.fields[1].kind = FieldKind::Choice(vec![fc_presentation::workbench::Choice {
                value: "subject-1".into(),
                label: format!("合成重命名 {index}"),
            }]);
            view.fields[1].value = "subject-1".into();
            window.render(&view).unwrap();
            window.probe_select_grid(0, 1).unwrap();
            let grid = window.state.controls.borrow().grid.as_ref().unwrap().hwnd();
            unsafe {
                let _ = SendMessageW(grid, WM_PAINT, None, None);
            }
            drop(window.take_events().unwrap());
        }
        let after = counts();
        assert!(after.0 <= before.0 + 2, "GDI {before:?}->{after:?}");
        assert!(after.1 <= before.1 + 2, "USER {before:?}->{after:?}");
    }
}

#[cfg(test)]
mod public_foreground_policy_tests {
    use super::*;
    #[test]
    fn foreground_policy_accepts_only_observed_own_process_and_rejects_unknown_or_external() {
        let own = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
        assert_ne!(own, 0);
        assert!(lesson_foreground_process_allowed(own, own));
        assert!(!lesson_foreground_process_allowed(0, own));
        assert!(!lesson_foreground_process_allowed(own.wrapping_add(1), own));
    }
}
