//! Native, UI-thread-only form adapter. No browser engine or synchronous worker wait.
use fc_presentation::workbench::{ActionId, Event, FieldId, FieldKind, FormData, View};
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
use windows::{
    core::{w, Error, Result, HSTRING, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{
            LibraryLoader::*,
            SystemServices::{SS_LEFT, SS_NOPREFIX},
        },
        UI::{Controls::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

const MAX_SUBMITS: usize = 8;
const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;
const FIELD_ID_BASE: u16 = 2000;
const ACTION_ID_BASE: u16 = 3000;
static CLASSES: Mutex<usize> = Mutex::new(0);
thread_local! { static WORKBENCHES: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) }; }

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
#[derive(Clone, Copy)]
struct FieldBinding {
    id: FieldId,
    kind: FieldKind,
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
}
impl Drop for Controls {
    fn drop(&mut self) {
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
            if field.kind == FieldKind::Password || presented.contains(&field.id) {
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
        if let Ok(mut events) = self.events.try_borrow_mut() {
            events.clear();
            self.event_bytes.set(0);
        } else {
            self.error.set(E_UNEXPECTED.0);
        }
    }
    fn submit(&self, action: ActionBinding) -> Result<()> {
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
        let fields = self
            .controls
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .fields
            .clone();
        let mut form = FormData::new();
        for field in fields {
            if field.kind != FieldKind::ReadOnly {
                let text = read_edit(field)?;
                form.insert(field.id, text)
                    .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
            }
        }
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
        events.push_back(Event::Submit(action.id, form));
        self.event_bytes.set(next);
        Ok(())
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
    state: Box<State>,
    font: Font,
    class: ClassLease,
    _thread: PhantomData<Rc<()>>,
}
impl Window {
    /// Initially hidden. Construction and hidden tests never change foreground.
    pub fn new() -> Result<Self> {
        let class = ClassLease::acquire()?;
        let font = Font::new()?;
        let mut window = Self {
            hwnd: HWND::default(),
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
    pub fn render(&self, view: &View) -> Result<()> {
        view.validate()
            .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        let old = self
            .state
            .view
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .clone();
        if old.as_ref() == Some(view) {
            return Ok(());
        }
        let _suppress = Suppress::new(&self.state);
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
            });
        if !same_structure {
            self.state.clear_passwords();
            let controls = self.build_controls(view)?;
            let old_controls = self.state.controls.replace(controls);
            drop(old_controls);
            self.state.scroll.set(0);
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
                    SetWindowTextW(binding.hwnd, &HSTRING::from(field.value.as_str()))?;
                }
                let _ = EnableWindow(binding.hwnd, !view.busy);
            }
            for (binding, action) in actions.iter().zip(&view.actions) {
                SetWindowTextW(binding.hwnd, &HSTRING::from(action.label.as_str()))?;
                let _ = EnableWindow(binding.hwnd, action.enabled && !view.busy);
            }
        }
        self.state.view.replace(Some(view.clone()));
        layout(self.hwnd, &self.state)?;
        Ok(())
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
            let kind = match field.kind {
                FieldKind::Text => ES_AUTOHSCROLL,
                FieldKind::Password => ES_AUTOHSCROLL | ES_PASSWORD,
                FieldKind::Multiline => ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN,
                FieldKind::ReadOnly => ES_AUTOHSCROLL | ES_READONLY,
            };
            let mut style = WS_BORDER | WS_TABSTOP | WINDOW_STYLE(kind as u32);
            if field.kind == FieldKind::Multiline {
                style |= WS_VSCROLL;
            }
            let native_id = FIELD_ID_BASE + index as u16;
            let hwnd = self.child(&mut controls, w!("EDIT"), &field.value, style, native_id)?;
            // SAFETY: exact owned Edit HWND and bounded numeric control messages.
            unsafe {
                let _ = SendMessageW(hwnd, EM_SETLIMITTEXT, Some(WPARAM(field.max_utf16)), None);
            }
            controls.fields.push(FieldBinding {
                id: field.id,
                kind: field.kind,
                hwnd,
                native_id,
                max_utf16: field.max_utf16,
            });
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
        let error = self.state.error.replace(0);
        if error != 0 {
            self.state.discard_forms();
            self.state.clear_passwords();
            self.state.input.set(false);
            return Err(Error::from_hresult(windows::core::HRESULT(error)));
        }
        let mut output = Vec::with_capacity(MAX_SUBMITS + 2);
        if self.state.close.replace(false) {
            output.push(Event::Close);
        }
        if self.state.input.replace(false) {
            output.push(Event::Input);
        }
        output.extend(
            self.state
                .events
                .try_borrow_mut()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
                .drain(..),
        );
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
        if !matches!(binding.kind, FieldKind::ReadOnly | FieldKind::Password)
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
        if view.busy
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
        // Validate the entire supplied form before modifying even one native
        // control. Every editable field is required; readonly/unknown IDs remain
        // in FormData and are rejected, including fields with empty values.
        let mut prepared = Vec::with_capacity(fields.len());
        for field in fields {
            if field.kind == FieldKind::ReadOnly {
                continue;
            }
            let text = WipingText(
                form.take(field.id)
                    .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?,
            );
            if text.0.contains('\0') || text.0.encode_utf16().count() > field.max_utf16 {
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
}
impl Drop for Window {
    fn drop(&mut self) {
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
    for (index, (binding, field)) in fields.iter().zip(&view.fields).enumerate() {
        positions.push((labels[index], 20, y, width, 25));
        let height = if field.kind == FieldKind::Multiline {
            88
        } else {
            30
        };
        positions.push((binding.hwnd, 20, y + 28, width, height));
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
                        if field.is_some() && notification == EN_CHANGE {
                            state.input.set(true);
                        }
                        if let Some(action) = action {
                            if notification == BN_CLICKED && IsWindowEnabled(action.hwnd).as_bool()
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
mod tests {
    use super::*;
    use fc_presentation::workbench::{Action, Field};

    fn view() -> View {
        View {
            title: "未来教室 · 合成测试".into(),
            description: "本测试窗口始终隐藏".into(),
            status: String::new(),
            busy: false,
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
    #[test]
    fn hidden_native_form_preserves_drafts_masks_and_consumes_password_without_activation() {
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
}
