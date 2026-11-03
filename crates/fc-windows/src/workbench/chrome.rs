//! Paint native controls without replacing their input, accessibility or dialog behavior.
use super::*;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};

const SKIN_ID: usize = 0xfc63;
const EDIT: usize = 1;
const CHOICE: usize = 2;
const BUTTON: usize = 3;
const PRIMARY: usize = 4;
const NAVIGATION: usize = 8;

pub(super) fn field(hwnd: HWND, choice: bool) -> Result<()> {
    attach(hwnd, if choice { CHOICE } else { EDIT })?;
    if !choice {
        // Keep native caret, password masking, selection and IME handling.
        unsafe {
            SendMessageW(
                hwnd,
                EM_SETMARGINS,
                Some(WPARAM(3)),
                Some(LPARAM(10 | 10 << 16)),
            );
        }
    }
    Ok(())
}

pub(super) fn button(hwnd: HWND, primary: bool, navigation: bool) -> Result<()> {
    attach(
        hwnd,
        BUTTON | if primary { PRIMARY } else { 0 } | if navigation { NAVIGATION } else { 0 },
    )
}

fn attach(hwnd: HWND, kind: usize) -> Result<()> {
    // No heap state or GDI objects survive a paint; the HWND owns this subclass.
    if unsafe { SetWindowSubclass(hwnd, Some(control_proc), SKIN_ID, kind) }.as_bool() {
        Ok(())
    } else {
        Err(Error::from_hresult(E_FAIL))
    }
}

unsafe fn paint(hwnd: HWND, dc: HDC, kind: usize) {
    unsafe {
        let saved = SaveDC(dc);
        let colors = theme::palette();
        let mut bounds = RECT::default();
        let _ = GetClientRect(hwnd, &mut bounds);
        let enabled = IsWindowEnabled(hwnd).as_bool();
        let focused = GetFocus() == hwnd;
        let pressed = kind & 3 == BUTTON
            && SendMessageW(hwnd, BM_GETSTATE, None, None).0 as u32 & BST_PUSHED != 0;
        let mut text = [0u16; 512];
        let count = GetWindowTextW(hwnd, &mut text).max(0) as usize;
        let navigation = kind & NAVIGATION != 0;
        let active = navigation && text.first() == Some(&('●' as u16));
        let primary = kind & PRIMARY != 0 && !navigation;
        let mut cursor = POINT::default();
        let hot = GetCursorPos(&mut cursor).is_ok() && WindowFromPoint(cursor) == hwnd;
        let background = if !enabled {
            colors.soft
        } else if primary {
            if pressed || hot {
                colors.primary_hover
            } else {
                colors.primary
            }
        } else if active || pressed || hot {
            colors.selected
        } else {
            colors.surface
        };
        let foreground = if !enabled {
            colors.disabled
        } else if primary {
            colors.on_primary
        } else if active {
            colors.selected_text
        } else {
            colors.text
        };
        let brush = HBRUSH(GetStockObject(DC_BRUSH).0);
        let _ = SetDCBrushColor(dc, colors.surface);
        let _ = FillRect(dc, &bounds, brush);
        let _ = SelectObject(dc, GetStockObject(DC_BRUSH));
        let _ = SelectObject(dc, GetStockObject(DC_PEN));
        let _ = SetDCBrushColor(dc, background);
        let _ = SetDCPenColor(
            dc,
            if focused || primary || active {
                colors.primary
            } else {
                colors.border
            },
        );
        let _ = RoundRect(dc, 0, 0, bounds.right, bounds.bottom, 12, 12);
        let font = SendMessageW(hwnd, WM_GETFONT, None, None);
        let _ = SelectObject(dc, HGDIOBJ(font.0 as *mut _));
        let _ = SetBkMode(dc, TRANSPARENT);
        let _ = SetTextColor(dc, foreground);
        bounds.left += 12;
        bounds.right -= if kind & 3 == CHOICE { 32 } else { 12 };
        let start = if active {
            text[..count]
                .iter()
                .position(|c| *c != '●' as u16 && *c != 32)
                .unwrap_or(0)
        } else {
            0
        };
        if start < count {
            let flags = if navigation || kind & 3 == CHOICE {
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX
            } else {
                // Keep complete action names readable in the fixed command area.
                let mut measured = bounds;
                let _ = DrawTextW(
                    dc,
                    &mut text[start..count],
                    &mut measured,
                    DT_CENTER | DT_WORDBREAK | DT_NOPREFIX | DT_CALCRECT,
                );
                bounds.top = ((bounds.bottom - (measured.bottom - measured.top)) / 2).max(2);
                bounds.bottom -= 2;
                DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX
            };
            let _ = DrawTextW(dc, &mut text[start..count], &mut bounds, flags);
        }
        if kind & 3 == CHOICE {
            let x = bounds.right + 14;
            let y = bounds.bottom / 2;
            let _ = SetDCPenColor(dc, colors.muted);
            let _ = MoveToEx(dc, x - 4, y - 2, None);
            let _ = LineTo(dc, x, y + 2);
            let _ = LineTo(dc, x + 4, y - 2);
        }
        if focused {
            let mut focus = RECT {
                left: 4,
                top: 4,
                right: bounds.right + if kind & 3 == CHOICE { 28 } else { 8 },
                bottom: bounds.bottom - 4,
            };
            let _ = DrawFocusRect(dc, &mut focus);
        }
        let _ = RestoreDC(dc, saved);
    }
}

unsafe extern "system" fn control_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    kind: usize,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(control_proc), SKIN_ID);
            }
            WM_MOUSEMOVE if kind & 3 != EDIT => {
                let mut tracking = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tracking);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            WM_MOUSELEAVE => {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            WM_PAINT if kind & 3 != EDIT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint(hwnd, dc, kind);
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }
            WM_PRINTCLIENT if kind & 3 != EDIT => {
                paint(hwnd, HDC(wparam.0 as *mut _), kind);
                return LRESULT(0);
            }
            WM_NCPAINT if kind & 3 == EDIT => {
                let result = DefSubclassProc(hwnd, message, wparam, lparam);
                let dc = GetWindowDC(Some(hwnd));
                if !dc.is_invalid() {
                    let colors = theme::palette();
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    rect.right -= rect.left;
                    rect.bottom -= rect.top;
                    rect.left = 0;
                    rect.top = 0;
                    let previous = SetDCBrushColor(
                        dc,
                        if GetFocus() == hwnd {
                            colors.primary
                        } else {
                            colors.border
                        },
                    );
                    let _ = FrameRect(dc, &rect, HBRUSH(GetStockObject(DC_BRUSH).0));
                    let _ = SetDCBrushColor(dc, previous);
                    let _ = ReleaseDC(Some(hwnd), dc);
                }
                return result;
            }
            _ => {}
        }
        let result = DefSubclassProc(hwnd, message, wparam, lparam);
        if matches!(
            message,
            WM_SETFOCUS
                | WM_KILLFOCUS
                | WM_ENABLE
                | WM_SETTEXT
                | WM_THEMECHANGED
                | WM_SYSCOLORCHANGE
                | BM_SETSTATE
                | BM_SETSTYLE
                | CB_SETCURSEL
        ) {
            let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_FRAME);
        }
        result
    }
}
