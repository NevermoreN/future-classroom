//! UI-owned subclass state for digit masks and a single painted timetable grid.
use super::*;
use fc_domain::time::TimeRangeInput;
use fc_presentation::workbench::Grid;
use windows::Win32::{
    System::{DataExchange::*, Memory::*},
    UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
};

const MASK_ID: usize = 0xfc61;
const GRID_ID: usize = 0xfc62;
const DIGIT_POS: [usize; 8] = [0, 1, 3, 4, 6, 7, 9, 10];

fn input(raw: &str) -> Result<TimeRangeInput> {
    let mut model = TimeRangeInput::default();
    if raw.is_empty() {
        return Ok(model);
    }
    let chars: Vec<_> = raw.chars().take(9).collect();
    if chars.len() != 8 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    for (index, c) in chars.into_iter().enumerate() {
        model
            .set_cursor(index)
            .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        if c != 'X' {
            model
                .insert(c)
                .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        }
    }
    model
        .set_cursor(0)
        .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
    Ok(model)
}
struct MaskState {
    model: RefCell<TimeRangeInput>,
    updating: Cell<bool>,
}
pub(super) struct MaskControl {
    hwnd: HWND,
    state: Box<MaskState>,
}
impl MaskControl {
    pub(super) fn attach(hwnd: HWND, raw: &str) -> Result<Self> {
        let control = Self {
            hwnd,
            state: Box::new(MaskState {
                model: RefCell::new(input(raw)?),
                updating: Cell::new(false),
            }),
        };
        // SAFETY: Box pointee is stable, exclusively UI-owned, and retained until
        // its subclass is detached in Drop. Callback uses only shared references.
        if !unsafe {
            SetWindowSubclass(
                hwnd,
                Some(mask_proc),
                MASK_ID,
                (&raw const *control.state) as usize,
            )
        }
        .as_bool()
        {
            return Err(Error::from_hresult(E_FAIL));
        }
        control.refresh()?;
        Ok(control)
    }
    pub(super) fn hwnd(&self) -> HWND {
        self.hwnd
    }
    pub(super) fn digits(&self) -> Result<String> {
        Ok(self
            .state
            .model
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
            .display()
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == 'X')
            .collect())
    }
    fn refresh(&self) -> Result<()> {
        refresh_mask(self.hwnd, &self.state)
    }
    #[cfg(test)]
    pub(super) fn probe_paste(&self, text: &str) -> Result<()> {
        paste_mask(self.hwnd, &self.state, text)
    }
}
fn paste_mask(hwnd: HWND, state: &MaskState, text: &str) -> Result<()> {
    state
        .model
        .try_borrow_mut()
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
        .paste(text)
        .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
    refresh_mask(hwnd, state)
}
impl Drop for MaskControl {
    fn drop(&mut self) {
        // SAFETY: same owning UI thread; no callback can retain the Box beyond detach.
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(mask_proc), MASK_ID);
        }
    }
}
fn refresh_mask(hwnd: HWND, state: &MaskState) -> Result<()> {
    let (display, cursor) = {
        let model = state
            .model
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        (model.display(), model.cursor())
    };
    state.updating.set(true);
    struct Guard<'a>(&'a Cell<bool>);
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            self.0.set(false)
        }
    }
    let _guard = Guard(&state.updating);
    // SAFETY: canonical non-secret display copied into our Edit, no model borrow
    // spans reentrant WM_SETTEXT/EN_CHANGE. Punctuation is never an input value.
    unsafe {
        SetWindowTextW(hwnd, &HSTRING::from(display))?;
        let position = if cursor == 8 { 11 } else { DIGIT_POS[cursor] };
        let _ = SendMessageW(
            hwnd,
            EM_SETSEL,
            Some(WPARAM(position)),
            Some(LPARAM(position as isize)),
        );
        let _ = SendMessageW(hwnd, EM_EMPTYUNDOBUFFER, None, None);
    }
    Ok(())
}
fn selection(hwnd: HWND) -> (usize, usize) {
    let mut start = 0u32;
    let mut end = 0u32;
    // SAFETY: own Edit synchronously writes these bounded local selection outputs.
    unsafe {
        let _ = SendMessageW(
            hwnd,
            EM_GETSEL,
            Some(WPARAM((&raw mut start) as usize)),
            Some(LPARAM((&raw mut end) as isize)),
        );
    }
    let start = DIGIT_POS.partition_point(|p| *p < (start as usize));
    let end = DIGIT_POS.partition_point(|p| *p < (end as usize));
    (start, end)
}
fn apply_selection(model: &mut TimeRangeInput, (start, end): (usize, usize)) {
    if start < end {
        let _ = model.select(start, end);
    } else {
        let _ = model.set_cursor(start);
    }
}
fn clipboard_digits(hwnd: HWND) -> Result<String> {
    // SAFETY: clipboard is only opened for reading and always closed; no contents
    // are replaced by this adapter. The global allocation stays locked while read.
    unsafe {
        OpenClipboard(Some(hwnd))?;
        struct Clipboard;
        impl Drop for Clipboard {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseClipboard();
                }
            }
        }
        let _clipboard = Clipboard;
        let handle = HGLOBAL(GetClipboardData(13)?.0);
        let bytes = GlobalSize(handle);
        if bytes < 2 || !bytes.is_multiple_of(2) {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let pointer = GlobalLock(handle);
        if pointer.is_null() {
            return Err(Error::from_hresult(E_FAIL));
        }
        struct Unlock(HGLOBAL);
        impl Drop for Unlock {
            fn drop(&mut self) {
                unsafe {
                    let _ = GlobalUnlock(self.0);
                }
            }
        }
        let _unlock = Unlock(handle);
        let units = std::slice::from_raw_parts(pointer.cast::<u16>(), (bytes / 2).min(9));
        let end = units
            .iter()
            .position(|u| *u == 0)
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        String::from_utf16(&units[..end]).map_err(|_| Error::from_hresult(E_INVALIDARG))
    }
}
unsafe extern "system" fn mask_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: subclass owner retains this stable Box; no mutable owner borrow
        // crosses native calls. NCDESTROY detaches before any late message.
        unsafe {
            let state = &*(data as *const MaskState);
            if message == WM_NCDESTROY {
                let _ = RemoveWindowSubclass(hwnd, Some(mask_proc), MASK_ID);
                return DefSubclassProc(hwnd, message, wparam, lparam);
            }
            if state.updating.get() {
                return DefSubclassProc(hwnd, message, wparam, lparam);
            }
            if message == WM_GETDLGCODE {
                return LRESULT((DLGC_WANTARROWS | DLGC_WANTCHARS | DLGC_HASSETSEL) as isize);
            }
            if message == WM_SETTEXT {
                if lparam.0 == 0 {
                    return LRESULT(0);
                }
                let mut units = Vec::new();
                for i in 0..=8 {
                    let unit = *(lparam.0 as *const u16).add(i);
                    if unit == 0 {
                        break;
                    }
                    units.push(unit);
                }
                let Ok(raw) = String::from_utf16(&units) else {
                    return LRESULT(0);
                };
                let Ok(next) = input(&raw) else {
                    return LRESULT(0);
                };
                state.model.replace(next);
                return LRESULT(if refresh_mask(hwnd, state).is_ok() {
                    1
                } else {
                    0
                });
            }
            if !IsWindowEnabled(hwnd).as_bool()
                && matches!(message, WM_CHAR | WM_KEYDOWN | WM_PASTE | WM_CLEAR | WM_CUT)
            {
                return LRESULT(0);
            }
            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
            let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
            let paste = message == WM_PASTE
                || message == WM_KEYDOWN
                    && (ctrl && wparam.0 == VK_V.0 as usize
                        || shift && wparam.0 == VK_INSERT.0 as usize);
            if paste {
                if let Ok(text) = clipboard_digits(hwnd) {
                    let _ = paste_mask(hwnd, state, &text);
                }
                return LRESULT(0);
            }
            if message == WM_UNDO {
                return LRESULT(0);
            }
            // IME/free-text replacement must not mutate the native buffer behind
            // the eight-digit authority. Full-width digits enter through WM_CHAR
            // or the bounded eight-digit paste path above.
            if matches!(
                message,
                WM_IME_STARTCOMPOSITION
                    | WM_IME_COMPOSITION
                    | WM_IME_CHAR
                    | EM_REPLACESEL
                    | EM_SETHANDLE
            ) {
                return LRESULT(0);
            }
            if message == WM_CHAR
                || message == WM_CLEAR
                || message == WM_CUT
                || message == WM_KEYDOWN
                    && [VK_LEFT.0, VK_RIGHT.0, VK_DELETE.0, VK_HOME.0, VK_END.0]
                        .contains(&(wparam.0 as u16))
            {
                if message == WM_KEYDOWN
                    && shift
                    && [VK_LEFT.0, VK_RIGHT.0, VK_HOME.0, VK_END.0].contains(&(wparam.0 as u16))
                {
                    return DefSubclassProc(hwnd, message, wparam, lparam);
                }
                let selected = selection(hwnd);
                let changed = {
                    let Ok(mut model) = state.model.try_borrow_mut() else {
                        return LRESULT(0);
                    };
                    apply_selection(&mut model, selected);
                    if message == WM_CLEAR || message == WM_CUT {
                        model.delete();
                        true
                    } else if message == WM_CHAR {
                        if wparam.0 == 8 {
                            model.backspace();
                            true
                        } else {
                            char::from_u32(wparam.0 as u32).is_some_and(|c| model.insert(c).is_ok())
                        }
                    } else {
                        match wparam.0 as u16 {
                            v if v == VK_LEFT.0 => model.move_left(),
                            v if v == VK_RIGHT.0 => model.move_right(),
                            v if v == VK_DELETE.0 => model.delete(),
                            v if v == VK_HOME.0 => {
                                let _ = model.set_cursor(0);
                            }
                            _ => {
                                let _ = model.set_cursor(8);
                            }
                        }
                        true
                    }
                };
                if changed {
                    let _ = refresh_mask(hwnd, state);
                }
                return LRESULT(0);
            }
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
    }));
    match result {
        Ok(value) => value,
        Err(_) => {
            unsafe {
                PostQuitMessage(1);
            }
            LRESULT(0)
        }
    }
}

struct GridState {
    grid: RefCell<Grid>,
    selected: Cell<Option<(usize, usize)>>,
    enabled: Cell<bool>,
    parent: *const State,
    font: HFONT,
}
pub(super) struct GridControl {
    hwnd: HWND,
    state: Box<GridState>,
}
fn row_positions(grid: &Grid) -> Vec<(i32, Option<i32>)> {
    let mut y = 34;
    let mut previous = "";
    grid.rows
        .iter()
        .map(|row| {
            let group = if row.group != previous {
                previous = &row.group;
                let top = y;
                y += 25;
                Some(top)
            } else {
                None
            };
            let top = y;
            y += 48;
            (top, group)
        })
        .collect()
}
impl GridControl {
    pub(super) fn attach(
        hwnd: HWND,
        grid: &Grid,
        parent: &State,
        font: HFONT,
        busy: bool,
    ) -> Result<Self> {
        grid.validate()
            .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        let result = Self {
            hwnd,
            state: Box::new(GridState {
                grid: RefCell::new(grid.clone()),
                selected: Cell::new(grid.selected),
                enabled: Cell::new(grid.enabled && !busy),
                parent: parent as *const _,
                font,
            }),
        };
        // SAFETY: Box and parent State remain UI-owned through native child life.
        if !unsafe {
            SetWindowSubclass(
                hwnd,
                Some(grid_proc),
                GRID_ID,
                (&raw const *result.state) as usize,
            )
        }
        .as_bool()
        {
            return Err(Error::from_hresult(E_FAIL));
        }
        Ok(result)
    }
    pub(super) fn hwnd(&self) -> HWND {
        self.hwnd
    }
    pub(super) fn height(&self) -> i32 {
        let grid = self.state.grid.borrow();
        row_positions(&grid).last().map_or(36, |(top, _)| top + 50)
    }
    pub(super) fn update(&self, grid: &Grid, busy: bool) -> Result<()> {
        grid.validate()
            .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        let changed = {
            let old = self.state.grid.borrow();
            old.selected != grid.selected || old.rows != grid.rows || old.columns != grid.columns
        };
        if changed {
            self.state.selected.set(grid.selected)
        }
        self.state.grid.replace(grid.clone());
        self.state.enabled.set(grid.enabled && !busy);
        // SAFETY: asynchronous invalidation retains no borrowed model pointer.
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
        Ok(())
    }
    pub(super) fn point(&self, row: usize, column: usize) -> Result<(HWND, i32, i32)> {
        if !self.state.enabled.get() || !unsafe { IsWindowEnabled(self.hwnd) }.as_bool() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let grid = self.state.grid.borrow();
        if row >= grid.rows.len() || column >= grid.columns.len() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(self.hwnd, &mut rect)?;
        }
        let first = (rect.right / 3).clamp(120, 260);
        let width = (rect.right - first) / (grid.columns.len() as i32 - 1);
        let x = if column == 0 {
            first / 2
        } else {
            first + (column as i32 - 1) * width + width / 2
        };
        Ok((self.hwnd, x, row_positions(&grid)[row].0 + 24))
    }
}
impl Drop for GridControl {
    fn drop(&mut self) {
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(grid_proc), GRID_ID);
        }
    }
}
fn select_grid(hwnd: HWND, state: &GridState, row: usize, column: usize, emit: bool) {
    if !state.enabled.get() {
        return;
    }
    let column = {
        let grid = state.grid.borrow();
        if row >= grid.rows.len() || column >= grid.columns.len() {
            return;
        }
        if grid.rows[row].merged && column > 0 {
            1
        } else {
            column
        }
    };
    state.selected.set(Some((row, column)));
    // SAFETY: parent outlives child/subclass owner, shared state uses bounded hints.
    unsafe {
        if emit {
            (&*state.parent).grid_selected(row, column)
        } else {
            (&*state.parent).input.set(true)
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}
fn paint_grid(hwnd: HWND, state: &GridState) {
    let grid = state.grid.borrow().clone();
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: Paint/DC ownership is paired even if drawing or text conversion panics.
    unsafe {
        let dc = BeginPaint(hwnd, &mut ps);
        struct Paint {
            hwnd: HWND,
            ps: PAINTSTRUCT,
            dc: HDC,
            old: HGDIOBJ,
        }
        impl Drop for Paint {
            fn drop(&mut self) {
                unsafe {
                    let _ = SelectObject(self.dc, self.old);
                    let _ = EndPaint(self.hwnd, &self.ps);
                }
            }
        }
        let old = SelectObject(dc, state.font.into());
        let _paint = Paint { hwnd, ps, dc, old };
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_err() {
            return;
        }
        let first = (rect.right / 3).clamp(120, 260);
        let width = ((rect.right - first) / (grid.columns.len() as i32 - 1)).max(1);
        struct Brush(HBRUSH);
        impl Drop for Brush {
            fn drop(&mut self) {
                unsafe {
                    let _ = DeleteObject(self.0.into());
                }
            }
        }
        let group = Brush(CreateSolidBrush(COLORREF(0x00f6e9df)));
        let chosen = Brush(CreateSolidBrush(COLORREF(0x00ffe2c7)));
        let border = Brush(CreateSolidBrush(COLORREF(0x00cfcfcf)));
        let white = HBRUSH(GetStockObject(WHITE_BRUSH).0);
        let _ = FillRect(dc, &rect, white);
        let _ = SetBkMode(dc, TRANSPARENT);
        let _ = SetTextColor(
            dc,
            COLORREF(if state.enabled.get() {
                0x00202020
            } else {
                0x00808080
            }),
        );
        let draw = |text: &str, mut bounds: RECT, brush: HBRUSH| {
            let _ = FillRect(dc, &bounds, brush);
            let _ = FrameRect(dc, &bounds, border.0);
            bounds.left += 6;
            bounds.right -= 6;
            bounds.top += 3;
            let mut text: Vec<u16> = text.encode_utf16().collect();
            // DrawTextW may inspect its pointer even for cchText=0; an empty
            // Rust slice has a dangling sentinel, so do not pass it to Win32.
            if text.is_empty() {
                return;
            }
            let _ = DrawTextW(
                dc,
                &mut text,
                &mut bounds,
                DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
            );
        };
        for (index, column) in grid.columns.iter().enumerate() {
            let left = if index == 0 {
                0
            } else {
                first + (index as i32 - 1) * width
            };
            let right = if index == 0 {
                first
            } else if index + 1 == grid.columns.len() {
                rect.right
            } else {
                left + width
            };
            draw(
                column,
                RECT {
                    left,
                    top: 0,
                    right,
                    bottom: 34,
                },
                group.0,
            );
        }
        for (index, (row, (top, heading))) in grid.rows.iter().zip(row_positions(&grid)).enumerate()
        {
            if let Some(heading) = heading {
                draw(
                    &row.group,
                    RECT {
                        left: 0,
                        top: heading,
                        right: rect.right,
                        bottom: heading + 25,
                    },
                    group.0,
                )
            }
            for column in 0..grid.columns.len() {
                if row.merged && column > 1 {
                    continue;
                }
                let left = if column == 0 {
                    0
                } else {
                    first + (column as i32 - 1) * width
                };
                let right = if column == 0 {
                    first
                } else if row.merged || column + 1 == grid.columns.len() {
                    rect.right
                } else {
                    left + width
                };
                let text = if column == 0 {
                    &row.label
                } else {
                    &row.cells[if row.merged { 0 } else { column - 1 }]
                };
                draw(
                    text,
                    RECT {
                        left,
                        top,
                        right,
                        bottom: top + 48,
                    },
                    if state.selected.get() == Some((index, column)) {
                        chosen.0
                    } else {
                        white
                    },
                );
            }
        }
    }
}
unsafe extern "system" fn grid_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        // SAFETY: subclass owner retains state and parent; callbacks never form &mut.
        let state = &*(data as *const GridState);
        match message {
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(grid_proc), GRID_ID);
            }
            WM_GETDLGCODE => {
                return LRESULT(
                    (DLGC_WANTARROWS
                        | DLGC_WANTCHARS
                        | if wparam.0 == VK_RETURN.0 as usize {
                            DLGC_WANTALLKEYS
                        } else {
                            0
                        }) as isize,
                )
            }
            WM_PAINT => {
                paint_grid(hwnd, state);
                return LRESULT(0);
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_LBUTTONDOWN if state.enabled.get() && IsWindowEnabled(hwnd).as_bool() => {
                let x = lparam.0 as u16 as i16 as i32;
                let y = (lparam.0 >> 16) as u16 as i16 as i32;
                let found = {
                    let grid = state.grid.borrow();
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);
                    let first = (rect.right / 3).clamp(120, 260);
                    let width = ((rect.right - first) / (grid.columns.len() as i32 - 1)).max(1);
                    row_positions(&grid)
                        .iter()
                        .position(|(top, _)| y >= *top && y < *top + 48)
                        .map(|r| {
                            (
                                r,
                                if x < first {
                                    0
                                } else {
                                    (1 + (x - first) / width)
                                        .clamp(1, grid.columns.len() as i32 - 1)
                                        as usize
                                },
                            )
                        })
                };
                if let Some((r, c)) = found {
                    if IsWindowVisible(hwnd).as_bool() {
                        let _ = SetFocus(Some(hwnd));
                    }
                    select_grid(hwnd, state, r, c, true)
                }
                return LRESULT(0);
            }
            WM_KEYDOWN if state.enabled.get() && IsWindowEnabled(hwnd).as_bool() => {
                let mut selected = state.selected.get().unwrap_or((0, 0));
                let rows = state.grid.borrow().rows.len();
                if rows == 0 {
                    return LRESULT(0);
                }
                let columns = state.grid.borrow().columns.len();
                match wparam.0 as u16 {
                    v if v == VK_LEFT.0 => selected.1 = selected.1.saturating_sub(1),
                    v if v == VK_RIGHT.0 => selected.1 = (selected.1 + 1).min(columns - 1),
                    v if v == VK_UP.0 => selected.0 = selected.0.saturating_sub(1),
                    v if v == VK_DOWN.0 => selected.0 = (selected.0 + 1).min(rows - 1),
                    v if v == VK_RETURN.0 || v == VK_SPACE.0 => {
                        select_grid(hwnd, state, selected.0, selected.1, true);
                        return LRESULT(0);
                    }
                    _ => return DefSubclassProc(hwnd, message, wparam, lparam),
                }
                select_grid(hwnd, state, selected.0, selected.1, false);
                return LRESULT(0);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }));
    match result {
        Ok(value) => value,
        Err(_) => {
            unsafe {
                PostQuitMessage(1);
            }
            LRESULT(0)
        }
    }
}
