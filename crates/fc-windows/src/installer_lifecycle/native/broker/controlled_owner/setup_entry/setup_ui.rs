//! Native, runtime-free visual shell. Only the original sealed Setup child can
//! authorize installation. Its process is polled on the window thread; closing
//! the window never kills or abandons a live installation.
use super::standalone::Package;
use super::*;
use std::process::Child;
use windows::{
    core::w,
    Win32::{
        Graphics::Gdi::*,
        UI::{
            Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS},
            Input::KeyboardAndMouse::EnableWindow,
        },
    },
};
const PRIMARY: usize = 201;
const CLOSE: usize = 202;
const UNINSTALL: usize = 203;
const DELETE_DATA: usize = 204;
struct State {
    _package: Package,
    installed: bool,
    upgrading: bool,
    uninstall: bool,
    delete_data: bool,
    child: Option<Child>,
    result: u32,
    complete: bool,
    message: String,
    font: HFONT,
    heading: HFONT,
}
impl Drop for State {
    fn drop(&mut self) {
        // A message-loop failure must still settle the real child before any
        // extraction pins disappear. No timeout is treated as child exit.
        if let Some(child) = self.child.as_mut() {
            let _ = child.wait();
        }
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font.0));
            let _ = DeleteObject(HGDIOBJ(self.heading.0));
        }
    }
}
struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(Some(self.0)).as_bool() {
                let _ = DestroyWindow(self.0);
            }
        }
    }
}
struct WindowClass(HINSTANCE);
impl Drop for WindowClass {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterClassW(w!("FutureClassroom.StandaloneSetup.v1"), Some(self.0));
        }
    }
}
fn font(size: i32, weight: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -size,
            0,
            0,
            0,
            weight,
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
    }
}
unsafe fn label(dc: HDC, font: HFONT, color: COLORREF, value: &str, mut rect: RECT) {
    unsafe {
        SelectObject(dc, HGDIOBJ(font.0));
        SetTextColor(dc, color);
        SetBkMode(dc, TRANSPARENT);
        let mut text: Vec<u16> = value.encode_utf16().collect();
        DrawTextW(
            dc,
            &mut text,
            &mut rect,
            DT_LEFT | DT_WORDBREAK | DT_NOPREFIX,
        );
    }
}
unsafe fn rounded(dc: HDC, rect: RECT, fill: COLORREF, edge: COLORREF) {
    unsafe {
        SelectObject(dc, GetStockObject(DC_BRUSH));
        SelectObject(dc, GetStockObject(DC_PEN));
        SetDCBrushColor(dc, fill);
        SetDCPenColor(dc, edge);
        let _ = RoundRect(dc, rect.left, rect.top, rect.right, rect.bottom, 18, 18);
    }
}
unsafe fn repaint(hwnd: HWND, state: &State) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut bounds = RECT::default();
        let _ = GetClientRect(hwnd, &mut bounds);
        SetDCBrushColor(dc, COLORREF(0x00fbf8f5));
        FillRect(dc, &bounds, HBRUSH(GetStockObject(DC_BRUSH).0));
        rounded(
            dc,
            RECT {
                left: 24,
                top: 22,
                right: 616,
                bottom: 350,
            },
            COLORREF(0x00ffffff),
            COLORREF(0x00ebdfd5),
        );
        if let Ok(icon) = crate::startup::application_icon() {
            let _ = DrawIconEx(dc, 46, 44, icon, 48, 48, 0, None, DI_NORMAL);
        }
        label(
            dc,
            state.heading,
            COLORREF(0x005b3526),
            "未来教室",
            RECT {
                left: 110,
                top: 45,
                right: 570,
                bottom: 85,
            },
        );
        label(
            dc,
            state.font,
            COLORREF(0x008f7568),
            &format!(
                "版本 {} · {}",
                fc_domain::PRODUCT_DISPLAY_VERSION,
                if state.uninstall {
                    "卸载向导"
                } else {
                    "安装与维护"
                }
            ),
            RECT {
                left: 110,
                top: 87,
                right: 575,
                bottom: 115,
            },
        );
        label(
            dc,
            state.heading,
            COLORREF(0x005b3526),
            if state.complete {
                if state.uninstall {
                    "卸载完成"
                } else {
                    "安装完成"
                }
            } else if state.child.is_some() {
                if state.uninstall {
                    "正在卸载，请稍候"
                } else {
                    "正在修复安装，请稍候"
                }
            } else {
                if state.uninstall {
                    "卸载未来教室"
                } else if state.upgrading {
                    "升级未来教室"
                } else if state.installed {
                    "修复或卸载未来教室"
                } else {
                    "让课堂准备更轻松"
                }
            },
            RECT {
                left: 46,
                top: 148,
                right: 575,
                bottom: 190,
            },
        );
        label(
            dc,
            state.font,
            COLORREF(0x00816a5b),
            &state.message,
            RECT {
                left: 46,
                top: 206,
                right: 575,
                bottom: 274,
            },
        );
        label(
            dc,
            state.font,
            COLORREF(0x009e8b7b),
            if state.delete_data {
                "将清除账号、设置、课表和课堂资料。"
            } else {
                "账号、设置、课表和课堂资料会保留。"
            },
            RECT {
                left: 26,
                top: 377,
                right: 260,
                bottom: 420,
            },
        );
        let _ = EndPaint(hwnd, &ps);
    }
}
unsafe fn set_button(hwnd: HWND, id: i32, text: &str, enabled: bool) {
    unsafe {
        if let Ok(button) = GetDlgItem(Some(hwnd), id) {
            let _ = SetWindowTextW(button, &HSTRING::from(text));
            let _ = EnableWindow(button, enabled);
            let _ = InvalidateRect(Some(button), None, true);
        }
    }
}
unsafe fn update(hwnd: HWND, state: &State) {
    unsafe {
        set_button(
            hwnd,
            PRIMARY as i32,
            if state.complete {
                "完成"
            } else if state.child.is_some() {
                if state.uninstall {
                    "正在卸载…"
                } else {
                    "正在安装…"
                }
            } else if state.result != 1602 {
                if state.uninstall {
                    "重试卸载"
                } else {
                    "重试修复"
                }
            } else {
                if state.uninstall {
                    "确认卸载"
                } else if state.upgrading {
                    "升级并保留数据"
                } else if state.installed {
                    "修复并保留数据"
                } else {
                    "立即安装"
                }
            },
            state.child.is_none(),
        );
        set_button(
            hwnd,
            CLOSE as i32,
            if state.complete { "完成" } else { "退出" },
            state.child.is_none(),
        );
        set_button(
            hwnd,
            UNINSTALL as i32,
            if state.uninstall { "返回" } else { "卸载" },
            state.child.is_none(),
        );
        for (id, visible) in [
            (
                UNINSTALL,
                state.installed && !state.upgrading && !state.complete,
            ),
            (DELETE_DATA, state.uninstall && !state.complete),
        ] {
            if let Ok(control) = GetDlgItem(Some(hwnd), id as i32) {
                let _ = ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE });
                let _ = EnableWindow(control, state.child.is_none());
            }
        }
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}
fn finish_child(hwnd: HWND, state: &mut State) {
    let Some(child) = state.child.as_mut() else {
        return;
    };
    let status = match child.try_wait() {
        Ok(None) => return,
        Ok(Some(status)) => status,
        Err(e) => {
            state.message = format!(
                "暂时无法读取安装状态（{}）。安装进程仍受监控，请等待。",
                e.raw_os_error().unwrap_or(1)
            );
            unsafe {
                update(hwnd, state);
            }
            return;
        }
    };
    state.child.take();
    state.result = status.code().unwrap_or(1603) as u32;
    state.complete = state.result == 0;
    state.message = if state.complete {
        if state.uninstall {
            if state.delete_data {
                "未来教室及所选数据已清理。关闭本窗口后，维护缓存会自动清除。".to_owned()
            } else {
                "未来教室已卸载，原有账号、设置和课堂资料已保留。下次安装可以继续使用。".to_owned()
            }
        } else {
            "安装完成，未来教室已自动启动。桌面和开始菜单的启动图标已准备好。".to_owned()
        }
    } else if matches!(state.result, 5101..=5104) {
        match state.result {
            5104 => if state.uninstall {"卸载已执行，但维护入口或快捷方式未完全清理。可再次运行卸载重试；数据不会因失败而删除。"} else {"程序已安装，但维护入口或快捷方式尚未准备完整。请再次运行安装包选择修复，已有数据会保留。"},
            5101 => {
                "程序已卸载，但部分数据未能清理。请关闭占用资料的窗口后重试卸载；再次安装仍可进行。"
            }
            5102 => "程序已卸载，但系统卸载入口清理未完成。可重新运行卸载进行清理。",
            _ => "程序已卸载，但维护缓存清理未能安排。原有数据的选择已执行。",
        }
        .to_owned()
    } else if state.result == 1602 {
        "安装已取消，原有账号、课表和资料会保留。可以点击重试安装。".to_owned()
    } else {
        format!(
            "操作没有完成（代码 {}），不会当作卸载成功。\n请将代码告知管理员；原数据不会因安装失败被自动删除。",
            state.result
        )
    };
    unsafe {
        update(hwnd, state);
    }
}
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as _);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let state = &mut *ptr;
        match message {
            WM_PAINT => {
                repaint(hwnd, state);
                LRESULT(0)
            }
            WM_CTLCOLORBTN | WM_CTLCOLORSTATIC => {
                let dc = HDC(wparam.0 as *mut _);
                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, COLORREF(0x00816a5b));
                SetBkColor(dc, COLORREF(0x00ffffff));
                LRESULT(GetStockObject(WHITE_BRUSH).0 as isize)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER => {
                finish_child(hwnd, state);
                LRESULT(0)
            }
            WM_CLOSE => {
                if state.child.is_none() {
                    let _ = DestroyWindow(hwnd);
                } else {
                    state.message="正在处理，请等待完成后再关闭。\n安装或修复成功后，未来教室会立即自动启动。".to_owned();
                    update(hwnd, state);
                }
                LRESULT(0)
            }
            WM_COMMAND if wparam.0 >> 16 == 0 => {
                match wparam.0 & 0xffff {
                    PRIMARY if state.child.is_none() && state.complete => {
                        let _ = DestroyWindow(hwnd);
                    }
                    UNINSTALL if state.child.is_none() => {
                        state.uninstall = !state.uninstall;
                        state.result = 1602;
                        state.delete_data = false;
                        if let Ok(control) = GetDlgItem(Some(hwnd), DELETE_DATA as i32) {
                            SendMessageW(control, BM_SETCHECK, Some(WPARAM(0)), None);
                        }
                        state.message = if state.uninstall {
                            "默认保留原有账号、设置和资料。需要彻底清理时，勾选下方删除数据；运行中的未来教室会自动结束。".to_owned()
                        } else {
                            "修复会重新安装程序文件，并保留原有账号、设置、课表和资料。".to_owned()
                        };
                        update(hwnd, state);
                    }
                    DELETE_DATA => {
                        state.delete_data = GetDlgItem(Some(hwnd), DELETE_DATA as i32)
                            .is_ok_and(|c| SendMessageW(c, BM_GETCHECK, None, None).0 == 1);
                        update(hwnd, state);
                    }
                    PRIMARY if state.child.is_none() => {
                        state.delete_data = state.uninstall
                            && GetDlgItem(Some(hwnd), DELETE_DATA as i32)
                                .is_ok_and(|c| SendMessageW(c, BM_GETCHECK, None, None).0 == 1);
                        if state.delete_data && MessageBoxW(Some(hwnd), w!("将永久删除未来教室的账号、设置、课表、课堂资料及更新缓存。确定完整清理吗？"), w!("确认删除全部数据"), MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2) != IDYES { return LRESULT(0); }
                        let started = (|| {
                            let path =
                                std::env::current_exe().map_err(|_| LifecycleError::Invalid)?;
                            std::process::Command::new(path)
                                .args([
                                    if state.uninstall {
                                        "maintain-uninstall"
                                    } else if state.installed {
                                        "maintain-repair"
                                    } else {
                                        "maintain-install"
                                    },
                                    "force",
                                    if state.delete_data { "delete" } else { "keep" },
                                ])
                                .spawn()
                                .map_err(|_| LifecycleError::Invalid)
                        })();
                        match started {
                            Ok(child) => {
                                state.child = Some(child);
                                state.message = if state.uninstall {
                                    "正在自动结束未来教室并卸载，请稍候。只有实际卸载成功后才会清理所选数据。".to_owned()
                                } else {
                                    "正在自动结束未来教室并安装，原有数据会保留。完成后立即启动；系统权限提示仍需允许。".to_owned()
                                };
                            }
                            Err(_) => {
                                state.result = 1603;
                                state.message="无法准备安装文件。请确认安装包完整，磁盘有可用空间，然后重试。".to_owned();
                            }
                        }
                        update(hwnd, state);
                    }
                    CLOSE if state.child.is_none() => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => (),
                }
                LRESULT(0)
            }
            WM_DRAWITEM => {
                let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
                let primary = item.CtlID == PRIMARY as u32;
                let enabled = item.itemState.0 & ODS_DISABLED.0 == 0;
                rounded(
                    item.hDC,
                    item.rcItem,
                    if !enabled {
                        COLORREF(0x00eee7e1)
                    } else if primary {
                        COLORREF(0x00e97836)
                    } else {
                        COLORREF(0x00ffffff)
                    },
                    COLORREF(0x00e6d6c9),
                );
                let mut text = [0u16; 64];
                let n = GetWindowTextW(item.hwndItem, &mut text) as usize;
                SelectObject(item.hDC, HGDIOBJ(state.font.0));
                SetBkMode(item.hDC, TRANSPARENT);
                SetTextColor(
                    item.hDC,
                    if primary && enabled {
                        COLORREF(0x00ffffff)
                    } else {
                        COLORREF(0x00704d37)
                    },
                );
                let mut rect = item.rcItem;
                DrawTextW(
                    item.hDC,
                    &mut text[..n],
                    &mut rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
                if item.itemState.0 & ODS_FOCUS.0 != 0 {
                    rect.left += 4;
                    rect.top += 4;
                    rect.right -= 4;
                    rect.bottom -= 4;
                    let _ = DrawFocusRect(item.hDC, &rect);
                }
                LRESULT(1)
            }
            WM_DESTROY => {
                let _ = KillTimer(Some(hwnd), 1);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
pub(super) fn show(package: Package, uninstall: bool) -> Result<u32> {
    unsafe {
        let module = GetModuleHandleW(None)?;
        let instance = HINSTANCE(module.0);
        let class = w!("FutureClassroom.StandaloneSetup.v1");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(windows::core::Error::from_thread().into());
        }
        let _class = WindowClass(instance);
        // The asInvoker manifest leaves DPI virtualization to Windows (also
        // available on Windows 7); GDI and child controls share these units.
        let installed = uninstall || super::maintenance::exists()?;
        let upgrading =
            super::maintenance::installed()?.is_some_and(|v| v != env!("CARGO_PKG_VERSION"));
        let mut state = State {
            _package: package,
            installed,
            upgrading,
            uninstall,
            delete_data: false,
            child: None,
            result: 1602,
            complete: false,
            message: if uninstall {
                "默认保留原有账号、设置和资料。需要完整清理时，请勾选删除数据。".to_owned()
            } else if upgrading {
                "检测到较低版本。将直接升级到本安装包的版本，保留账号、设置、课表和资料。"
                    .to_owned()
            } else if installed {
                "检测到已有安装或残留。修复会重新安装程序文件，并保留设置和课堂资料；也可选择卸载。"
                    .to_owned()
            } else {
                "将未来教室安装到当前Windows账号。点击立即安装开始。".to_owned()
            },
            font: font(15, 400),
            heading: font(25, 600),
        };

        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: 640,
            bottom: 440,
        };
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        AdjustWindowRect(&mut bounds, style, false)?;
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("未来教室安装与维护"),
            style,
            (GetSystemMetrics(SM_CXSCREEN) - width) / 2,
            (GetSystemMetrics(SM_CYSCREEN) - height) / 2,
            width,
            height,
            None,
            None,
            Some(instance),
            Some((&raw mut state).cast()),
        )?;
        let _window = Window(hwnd);
        let create = |id, caption: &str, x, width| -> Result<()> {
            let button = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BUTTON"),
                &HSTRING::from(caption),
                WS_VISIBLE | WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                x,
                373,
                width,
                43,
                Some(hwnd),
                Some(HMENU(id as *mut _)),
                Some(instance),
                None,
            )?;
            SendMessageW(
                button,
                WM_SETFONT,
                Some(WPARAM(state.font.0 as usize)),
                Some(LPARAM(1)),
            );
            Ok(())
        };
        create(PRIMARY, "立即安装", 280, 176)?;
        create(UNINSTALL, "卸载", 464, 70)?;
        create(CLOSE, "退出", 542, 74)?;
        {
            let (id, caption, y) = (DELETE_DATA, "删除全部数据（账号、设置、课表和资料）", 282);
            let checkbox = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BUTTON"),
                &HSTRING::from(caption),
                WS_VISIBLE | WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                46,
                y,
                540,
                24,
                Some(hwnd),
                Some(HMENU(id as *mut _)),
                Some(instance),
                None,
            )?;
            SendMessageW(
                checkbox,
                WM_SETFONT,
                Some(WPARAM(state.font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        update(hwnd, &state);
        if let Ok(icon) = crate::startup::application_icon() {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }
        if SetTimer(Some(hwnd), 1, 250, None) == 0 {
            let _ = DestroyWindow(hwnd);
            return Err(LifecycleError::Invalid);
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        let mut message = MSG::default();
        loop {
            let value = GetMessageW(&mut message, None, 0, 0).0;
            if value == 0 {
                break;
            }
            if value < 0 {
                if state.child.is_some() {
                    continue;
                }
                break;
            }
            if !IsDialogMessageW(hwnd, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(state.result)
    }
}
