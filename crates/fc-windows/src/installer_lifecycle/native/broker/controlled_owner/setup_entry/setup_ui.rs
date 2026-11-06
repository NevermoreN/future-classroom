//! Native, runtime-free visual shell. Only the original sealed Setup child can
//! authorize installation. Its process is polled on the window thread; closing
//! the window never kills or abandons a live installation.
use super::standalone::{Extracted, Package};
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
struct State {
    package: Package,
    extracted: Option<Extracted>,
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
            &format!("版本 {} · 安装向导", fc_domain::PRODUCT_DISPLAY_VERSION),
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
                "安装完成"
            } else if state.child.is_some() {
                "正在安装，请稍候"
            } else {
                "让课堂准备更轻松"
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
                bottom: 325,
            },
        );
        label(
            dc,
            state.font,
            COLORREF(0x009e8b7b),
            "资料与课表会保留在这台电脑。",
            RECT {
                left: 26,
                top: 377,
                right: 355,
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
                "打开未来教室"
            } else if state.child.is_some() {
                "正在安装…"
            } else if state.result != 1602 {
                "重试安装"
            } else {
                "立即安装"
            },
            state.child.is_none(),
        );
        set_button(
            hwnd,
            CLOSE as i32,
            if state.complete { "完成" } else { "退出" },
            state.child.is_none(),
        );
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
        "安装成功！点击下方按钮打开未来教室。\n桌面和开始菜单也已准备好启动图标。".to_owned()
    } else if state.result == 1602 {
        "安装已取消，原有账号、课表和资料会保留。可以点击重试安装。".to_owned()
    } else {
        format!(
            "安装没有完成（返回代码 {}）。\n原有资料会保留，请关闭正在运行的未来教室后重试；如仍失败，请将代码告知管理员。",
            state.result
        )
    };
    if let Some(mut extracted) = state.extracted.take() {
        if extracted.cleanup().is_err() {
            state
                .message
                .push_str("\n临时安装文件未能清理，请关闭相关文件窗口后再退出。");
            state.extracted = Some(extracted);
        }
    }
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
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER => {
                finish_child(hwnd, state);
                LRESULT(0)
            }
            WM_CLOSE => {
                if state.child.is_none() {
                    let _ = DestroyWindow(hwnd);
                } else {
                    state.message="正在安装，请等待完成后再关闭。\n不要关闭电脑；安装完成后可以直接打开未来教室。".to_owned();
                    update(hwnd, state);
                }
                LRESULT(0)
            }
            WM_COMMAND if wparam.0 >> 16 == 0 => {
                match wparam.0 & 0xffff {
                    PRIMARY if state.child.is_none() && state.complete => {
                        match crate::system::DataPaths::installed_bootstrap_for_current_user() {
                            Ok(path) => {
                                if std::process::Command::new(path).spawn().is_ok() {
                                    let _ = DestroyWindow(hwnd);
                                } else {
                                    state.message =
                                        "未来教室已安装，但暂时无法打开。请从桌面或开始菜单启动。"
                                            .to_owned();
                                    update(hwnd, state);
                                }
                            }
                            Err(_) => {
                                state.message =
                                    "未来教室已安装，请从桌面或开始菜单启动。".to_owned();
                                update(hwnd, state);
                            }
                        }
                    }
                    PRIMARY if state.child.is_none() => {
                        let started = (|| {
                            let extracted = state.package.extract()?;
                            let child = extracted.start("install")?;
                            Ok::<_, LifecycleError>((extracted, child))
                        })();
                        match started {
                            Ok((extracted, child)) => {
                                state.extracted = Some(extracted);
                                state.child = Some(child);
                                state.message="正在核验并安装未来教室。\n如果出现 Windows 权限提示，请确认是本安装程序后允许。".to_owned();
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
pub(super) fn show(package: Package) -> Result<u32> {
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
        let mut state=State{package,extracted:None,child:None,result:1602,complete:false,message:"将未来教室安装到当前 Windows 账号。\n已有账号、课表、桌面排布和课堂资料会保留。\n点击“立即安装”开始。".to_owned(),font:font(15,400),heading:font(25,600)};
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
            w!("未来教室安装"),
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
        create(PRIMARY, "立即安装", 358, 148)?;
        create(CLOSE, "退出", 518, 98)?;
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
