use fc_domain::{
    ids::{AuthEpoch, Revision},
    time::{LocalDate, LocalDateTime},
};
use fc_presentation::{desktop_visible, format_clock, ClockStyle, Foreground, VisibilityInput};
use fc_runtime::authority::AuthorityStore;
use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};
use windows::{
    core::{w, Error, Result, HSTRING},
    Win32::{
        Foundation::*,
        Graphics::{Direct2D::Common::*, Direct2D::*, DirectWrite::*, Dxgi::Common::*, Gdi::*},
        Security::Cryptography::*,
        Storage::FileSystem::*,
        System::{Com::*, LibraryLoader::*, StationsAndDesktops::*, SystemInformation::*},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

// The current metadata binds CoTaskMemFree to combase.dll (Win8+). The same
// documented function is exported by ole32.dll on Win7. Keep this one corrected
// import narrow instead of accidentally importing the Windows Runtime DLL.
#[link(name = "ole32")]
unsafe extern "system" {
    #[link_name = "CoTaskMemFree"]
    fn win7_co_task_mem_free(memory: *const core::ffi::c_void);
}

pub struct SystemRandom;
impl fc_ports::RandomSource for SystemRandom {
    type Error = Error;
    fn fill(&mut self, output: &mut [u8]) -> Result<()> {
        if output.is_empty() {
            return Ok(());
        }
        let _ = u32::try_from(output.len()).map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        // SAFETY: owned mutable byte slice has exact checked DWORD length; the system
        // preferred provider is synchronous, retains no pointer and works on Win7 SP1.
        unsafe { BCryptGenRandom(None, output, BCRYPT_USE_SYSTEM_PREFERRED_RNG).ok() }
    }
}

struct Apartment(PhantomData<Rc<()>>);
impl Apartment {
    fn new() -> Result<Self> {
        // SAFETY: called once on this UI thread; successful init is paired in Drop.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self(PhantomData))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: same thread as successful CoInitializeEx; !Send/!Sync marker enforces it.
        unsafe { CoUninitialize() }
    }
}

struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    width: i32,
    height: i32,
    _thread: PhantomData<Rc<()>>,
}
impl Dib {
    fn new(width: i32, height: i32) -> Result<Self> {
        if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) * 4 > 16 * 1024 * 1024
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        // SAFETY: valid native structures, top-down 32bpp fixed DIB. All half-created
        // objects are released on error. Selected original object is restored in Drop.
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err(Error::from_thread());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            let old = SelectObject(dc, bitmap.into());
            if old.is_invalid() {
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(dc);
                return Err(Error::from_thread());
            }
            Ok(Self {
                dc,
                bitmap,
                old,
                width,
                height,
                _thread: PhantomData,
            })
        }
    }
}
impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: all render target/DC users have returned; restore before deleting.
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

struct Window(HWND, PhantomData<Rc<()>>);
impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: HWND is UI-owned; IsWindow avoids double destroy after WM_CLOSE.
        unsafe {
            if IsWindow(Some(self.0)).as_bool() {
                let _ = DestroyWindow(self.0);
            }
        }
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // No borrowed UI state or owned message payload crosses a reentrant callback.
    // Unwind is contained within the system ABI; process failure remains explicit.
    match std::panic::catch_unwind(|| {
        // SAFETY: callback arguments originate from Windows and live for this call.
        unsafe {
            match msg {
                WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
                WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
                WM_CLOSE => {
                    let _ = DestroyWindow(hwnd);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
    }) {
        Ok(result) => result,
        Err(_) => {
            // SAFETY: hide and quit on this callback's own UI thread; no panic text leaks.
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
                PostQuitMessage(1);
            }
            LRESULT(0)
        }
    }
}

fn local_now() -> Result<LocalDateTime> {
    // SAFETY: GetLocalTime returns an owned SYSTEMTIME value, no pointers retained.
    let t = unsafe { GetLocalTime() };
    let date = LocalDate::new(i32::from(t.wYear), t.wMonth as u8, t.wDay as u8)
        .map_err(|_| Error::from_hresult(E_FAIL))?;
    LocalDateTime::new(
        date,
        u32::from(t.wHour) * 3600 + u32::from(t.wMinute) * 60 + u32::from(t.wSecond),
    )
    .map_err(|_| Error::from_hresult(E_FAIL))
}
fn desktop_foreground() -> Foreground {
    // SAFETY: sampling only; these HWNDs are neither retained nor freed by us.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return Foreground::Unknown;
        }
        if hwnd == GetDesktopWindow() {
            return Foreground::Desktop;
        }
        let mut name = [0u16; 128];
        let n = GetClassNameW(hwnd, &mut name);
        if n <= 0 {
            return Foreground::Unknown;
        }
        match String::from_utf16_lossy(&name[..n as usize]).as_str() {
            "Progman" | "WorkerW" => Foreground::Desktop,
            _ => Foreground::ExternalApp,
        }
    }
}
fn draw(dib: &Dib, factory: &ID2D1Factory, write: &IDWriteFactory, text: &str) -> Result<()> {
    // SAFETY: all COM interfaces and the DIB belong to this apartment and are used
    // synchronously. Target drops before Dib. Strings/rectangles outlive each call.
    unsafe {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.,
            dpiY: 96.,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let target = factory.CreateDCRenderTarget(&props)?;
        let rect = RECT {
            left: 0,
            top: 0,
            right: dib.width,
            bottom: dib.height,
        };
        target.BindDC(dib.dc, &rect)?;
        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let format = write.CreateTextFormat(
            w!("Microsoft YaHei"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            22.,
            w!("zh-CN"),
        )?;
        let brush = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.,
                g: 1.,
                b: 1.,
                a: 1.,
            },
            None,
        )?;
        let glass = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.,
                g: 1.,
                b: 1.,
                a: 0.03,
            },
            None,
        )?;
        let edge = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.,
                g: 1.,
                b: 1.,
                a: 0.25,
            },
            None,
        )?;
        let bounds = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: 1.,
                top: 1.,
                right: dib.width as f32 - 1.,
                bottom: dib.height as f32 - 1.,
            },
            radiusX: 24.,
            radiusY: 24.,
        };
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 0.,
        }));
        target.FillRoundedRectangle(&bounds, &glass);
        target.DrawRoundedRectangle(&bounds, &edge, 1., None);
        let layout = D2D_RECT_F {
            left: 24.,
            top: 20.,
            right: dib.width as f32 - 24.,
            bottom: dib.height as f32 - 12.,
        };
        target.DrawText(
            &text.encode_utf16().collect::<Vec<_>>(),
            &format,
            &layout,
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
        target.EndDraw(None, None)?;
    }
    Ok(())
}

/// Evidence for the isolated native probe, never a full application acceptance report.
#[derive(Debug)]
pub struct NativeProbeReport {
    pub elapsed_ms: u128,
    pub rendered_frames: u32,
    pub offscreen_render_verified: bool,
    pub visible_samples: u32,
    pub dib_bytes: u64,
    pub program_data: PathBuf,
}

pub fn program_data() -> Result<PathBuf> {
    // SAFETY: Windows allocates the NUL-terminated KnownFolder result with CoTaskMem;
    // convert to owned UTF-16 string before freeing it exactly once, including errors.
    unsafe {
        let ptr = SHGetKnownFolderPath(&FOLDERID_ProgramData, KF_FLAG_DEFAULT, None)?;
        let value = ptr.to_string();
        win7_co_task_mem_free(ptr.0.cast());
        Ok(PathBuf::from(value?).join("FutureClassroom"))
    }
}

pub fn probe(seconds: u32) -> Result<NativeProbeReport> {
    if !(1..=300).contains(&seconds) {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let _apartment = Apartment::new()?;
    let (mut publisher, authority) = AuthorityStore::new((), Revision::new(0), AuthEpoch::new(0));
    publisher
        .initialize()
        .map_err(|_| Error::from_hresult(E_FAIL))?;
    // SAFETY: native factories and windows stay on this UI thread; all pointers in
    // class/creation structures reference static or local storage through each call.
    unsafe {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let instance = GetModuleHandleW(None)?;
        let class = w!("FutureClassroomRustNativeProbe");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(Error::from_thread());
        }
        struct Registration {
            instance: HINSTANCE,
        }
        impl Drop for Registration {
            fn drop(&mut self) {
                // SAFETY: all probe windows have been destroyed first by reverse local Drop.
                unsafe {
                    let _ =
                        UnregisterClassW(w!("FutureClassroomRustNativeProbe"), Some(self.instance));
                }
            }
        }
        let _registration = Registration {
            instance: instance.into(),
        };
        let mut work = RECT::default();
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&mut work as *mut RECT).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
        let width = 400.min(work.right - work.left);
        let height = 180.min(work.bottom - work.top);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class,
            w!("未来教室 · 原生验证"),
            WS_POPUP,
            work.right - width - 20,
            work.top + 20,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let window = Window(hwnd, PhantomData);
        let dib = Dib::new(width, height)?;
        // Exercise real D2D/DirectWrite even while another application is foreground.
        // This is offscreen and does not change focus, minimize or capture user windows.
        draw(
            &dib,
            &factory,
            &write,
            "未来教室 · 原生离屏验证\n3% 玻璃 · Direct2D / DirectWrite",
        )?;
        let started = Instant::now();
        let mut frames = 0u32;
        let mut visible_samples = 0u32;
        let mut last_minute = None;
        let timer = SetTimer(Some(hwnd), 1, 250, None);
        if timer == 0 {
            return Err(Error::from_thread());
        }
        let mut visible = false;
        let mut message = MSG::default();
        while started.elapsed() < Duration::from_secs(u64::from(seconds)) {
            let status = GetMessageW(&mut message, None, 0, 0).0;
            if status == -1 {
                return Err(Error::from_thread());
            }
            if status == 0 {
                if message.wParam.0 != 0 {
                    return Err(Error::from_hresult(E_FAIL));
                }
                break;
            }
            if message.message == WM_TIMER {
                let now = local_now()?;
                let healthy = authority.snapshot().is_ok();
                // Unknown/session transitions are conservative. OpenInputDesktop is
                // independently queried, so locked desktop cannot render probe content.
                let session_active = input_desktop_available();
                let allow = desktop_visible(VisibilityInput {
                    foreground: desktop_foreground(),
                    session_active,
                    healthy,
                    stopping: false,
                });
                if allow {
                    visible_samples += 1;
                    if last_minute != Some(now.second_of_day() / 60) {
                        let text=format!("{}\n未来教室 · Rust 原生绘制探针\n3% 玻璃 · 点击穿透 · 前台避让\n本窗口为技术验证，非完整产品",format_clock(now,ClockStyle::Digital24,false));
                        draw(&dib, &factory, &write, &text)?;
                        let size = SIZE {
                            cx: width,
                            cy: height,
                        };
                        let point = POINT {
                            x: work.right - width - 20,
                            y: work.top + 20,
                        };
                        let src = POINT { x: 0, y: 0 };
                        let blend = BLENDFUNCTION {
                            BlendOp: AC_SRC_OVER as u8,
                            BlendFlags: 0,
                            SourceConstantAlpha: 255,
                            AlphaFormat: AC_SRC_ALPHA as u8,
                        };
                        // Re-sample foreground before submission; never activate/topmost.
                        if desktop_foreground() == Foreground::Desktop
                            && authority.snapshot().is_ok()
                        {
                            UpdateLayeredWindow(
                                hwnd,
                                None,
                                Some(&point),
                                Some(&size),
                                Some(dib.dc),
                                Some(&src),
                                COLORREF(0),
                                Some(&blend),
                                ULW_ALPHA,
                            )?;
                            last_minute = Some(now.second_of_day() / 60);
                            frames += 1;
                        }
                    }
                    if !visible && desktop_foreground() == Foreground::Desktop {
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        visible = true;
                    }
                } else if visible {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    visible = false;
                }
            } else {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let _ = KillTimer(Some(hwnd), 1);
        let _ = ShowWindow(hwnd, SW_HIDE);
        authority.gate().stop();
        let report = NativeProbeReport {
            elapsed_ms: started.elapsed().as_millis(),
            rendered_frames: frames,
            offscreen_render_verified: true,
            visible_samples,
            dib_bytes: (width as u64) * (height as u64) * 4,
            program_data: program_data()?,
        };
        drop(dib);
        drop(window);
        Ok(report)
    }
}
fn input_desktop_available() -> bool {
    // SAFETY: this check never switches desktop. Every successful handle is closed.
    unsafe {
        match OpenInputDesktop(DF_ALLOWOTHERACCOUNTHOOK, false, DESKTOP_READOBJECTS) {
            Ok(d) => {
                let _ = CloseDesktop(d);
                true
            }
            Err(_) => false,
        }
    }
}

/// Lock a caller-selected project data directory. The file itself may remain after
/// normal exit; sharing is denied by the live OS handle, not its mere existence.
pub struct InstanceGuard {
    handle: HANDLE,
    _thread: PhantomData<Rc<()>>,
}
impl InstanceGuard {
    pub fn acquire(root: &Path) -> Result<Self> {
        if !root.is_absolute() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        std::fs::create_dir_all(root.join("runtime"))
            .map_err(|_| Error::from_hresult(E_ACCESSDENIED))?;
        let file = HSTRING::from(root.join("runtime").join("instance.lock").as_os_str());
        // SAFETY: path is owned through call, share mode zero provides cross-session
        // OS arbitration; normal user privileges only, no arbitrary remote path input.
        unsafe {
            let handle = CreateFileW(
                &file,
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )?;
            Ok(Self {
                handle,
                _thread: PhantomData,
            })
        }
    }
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: this wrapper solely owns a successful CreateFile handle.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}
