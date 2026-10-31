//! Full-size static rounded glass without an application-owned full-size DIB.
//! Fill and reflection use independent constant-alpha layered GDI regions. Windows
//! owns any compositor/backing allocations; those are not counted as our DIBs.
use super::*;

const BACKDROP_CLASS: windows::core::PCWSTR = w!("FutureClassroom.Desktop.Backdrop.v1");
pub(super) fn register(instance: HINSTANCE) -> Result<()> {
    let class = WNDCLASSW {
        hInstance: instance,
        lpszClassName: BACKDROP_CLASS,
        lpfnWndProc: Some(proc),
        hbrBackground: HBRUSH(unsafe { GetStockObject(WHITE_BRUSH) }.0),
        ..Default::default()
    };
    // SAFETY: static class/callback and borrowed stock brush outlive all windows.
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(Error::from_thread());
    }
    Ok(())
}
pub(super) fn unregister(instance: HINSTANCE) {
    // SAFETY: renderer destroys every backdrop window before class registration.
    unsafe {
        let _ = UnregisterClassW(BACKDROP_CLASS, Some(instance));
    }
}
struct Region(HRGN);
impl Region {
    fn owned(handle: HRGN) -> Result<Self> {
        if handle.is_invalid() {
            Err(Error::from_thread())
        } else {
            Ok(Self(handle))
        }
    }
    fn shape(width: i32, height: i32, border: bool) -> Result<Self> {
        // SAFETY: bounded integer geometry; each successful region has unique ownership.
        unsafe {
            let outer = Self::owned(CreateRoundRectRgn(1, 1, width - 1, height - 1, 40, 40))?;
            if border {
                let inner = Self::owned(CreateRoundRectRgn(2, 2, width - 2, height - 2, 38, 38))?;
                if CombineRgn(Some(outer.0), Some(outer.0), Some(inner.0), RGN_DIFF) == RGN_ERROR {
                    return Err(Error::from_thread());
                }
            }
            Ok(outer)
        }
    }
    fn copy(&self) -> Result<Self> {
        // SAFETY: source remains owned; destination is a separately owned region.
        unsafe {
            let copy = Self::owned(CreateRectRgn(0, 0, 0, 0))?;
            if CombineRgn(Some(copy.0), Some(self.0), None, RGN_COPY) == RGN_ERROR {
                return Err(Error::from_thread());
            }
            Ok(copy)
        }
    }
    fn rectangles(&self) -> Result<Vec<RECT>> {
        // SAFETY: GetRegionData copies only our own native region; a bounded,
        // sufficiently aligned buffer remains alive through the synchronous fill.
        unsafe {
            let bytes = GetRegionData(self.0, 0, None) as usize;
            if bytes < std::mem::size_of::<RGNDATAHEADER>() || bytes > 1024 * 1024 {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            let mut data = vec![0u64; bytes.div_ceil(8)];
            let written =
                GetRegionData(self.0, bytes as u32, Some(data.as_mut_ptr().cast())) as usize;
            if written != bytes {
                return Err(Error::from_thread());
            }
            let base = data.as_ptr().cast::<u8>();
            let header = &*base.cast::<RGNDATAHEADER>();
            let start = header.dwSize as usize;
            let count = header.nCount as usize;
            if start < std::mem::size_of::<RGNDATAHEADER>()
                || count > 4096
                || start
                    .checked_add(count * std::mem::size_of::<RECT>())
                    .is_none_or(|end| end > written)
            {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            Ok((0..count)
                .map(|i| {
                    std::ptr::read_unaligned(
                        base.add(start + i * std::mem::size_of::<RECT>())
                            .cast::<RECT>(),
                    )
                })
                .collect())
        }
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        // SAFETY: this region is uniquely owned and was never transferred to Windows.
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}
pub(super) struct Layer {
    pub(super) window: NativeWindow,
    region: Region,
    pub(super) rect: Rect,
    alpha: u8,
    paint_region: Option<Region>,
    paint_alpha: f32,
    region_attached: Cell<bool>,
    alpha_set: Cell<bool>,
}
impl Layer {
    fn new(class: &Class, rect: Rect, alpha: f32, border: bool) -> Result<Self> {
        let region = Region::shape(rect.width.ceil() as i32, rect.height.ceil() as i32, border)?;
        Self::with_region(class, rect, alpha, region, None, alpha)
    }
    fn with_region(
        class: &Class,
        rect: Rect,
        alpha: f32,
        region: Region,
        paint_region: Option<Region>,
        paint_alpha: f32,
    ) -> Result<Self> {
        // SAFETY: initially hidden and always transparent/non-activating/non-topmost.
        let window = NativeWindow(unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                BACKDROP_CLASS,
                w!("未来教室课间静态玻璃"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(class.instance),
                None,
            )?
        });
        let layer = Self {
            window,
            region,
            rect,
            alpha: (alpha * 255.).round() as u8,
            paint_region,
            paint_alpha,
            region_attached: Cell::new(false),
            alpha_set: Cell::new(false),
        };
        layer.apply()?;
        Ok(layer)
    }
    fn partitioned(
        class: &Class,
        rect: Rect,
        raw: f32,
        component: f32,
        border: bool,
        fill: f32,
        excluded: &[Rect],
    ) -> Result<Self> {
        let paint = Region::shape(rect.width.ceil() as i32, rect.height.ceil() as i32, border)?;
        let region = paint.copy()?;
        // SAFETY: each HRGN is locally owned. Difference results stay in the
        // destination owner and are transferred only by with_region/apply.
        unsafe {
            if !border {
                let edge =
                    Region::shape(rect.width.ceil() as i32, rect.height.ceil() as i32, true)?;
                if CombineRgn(Some(region.0), Some(region.0), Some(edge.0), RGN_DIFF) == RGN_ERROR {
                    return Err(Error::from_thread());
                }
            }
            for r in excluded {
                let hole = Region::owned(CreateRectRgn(
                    (r.x - rect.x).round() as i32,
                    (r.y - rect.y).round() as i32,
                    (r.x - rect.x).round() as i32 + r.width.ceil() as i32,
                    (r.y - rect.y).round() as i32 + r.height.ceil() as i32,
                ))?;
                if CombineRgn(Some(region.0), Some(region.0), Some(hole.0), RGN_DIFF) == RGN_ERROR {
                    return Err(Error::from_thread());
                }
            }
        }
        let native = if border { raw + fill * (1. - raw) } else { raw };
        Self::with_region(class, rect, native * component, region, Some(paint), raw)
    }
    fn draw_patch(&self, target: &ID2D1DCRenderTarget, patch: Rect) -> Result<()> {
        let Some(region) = &self.paint_region else {
            return Ok(());
        };
        let rectangles = region.rectangles()?;
        // SAFETY: only this scene's exact retained GDI region is rasterized onto
        // its own small D2D target. It is the large card's shape, no inner frame.
        unsafe {
            let brush = target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.,
                    g: 1.,
                    b: 1.,
                    a: self.paint_alpha,
                },
                None,
            )?;
            for r in rectangles {
                target.FillRectangle(
                    &D2D_RECT_F {
                        left: self.rect.x - patch.x + r.left as f32,
                        top: self.rect.y - patch.y + r.top as f32,
                        right: self.rect.x - patch.x + r.right as f32,
                        bottom: self.rect.y - patch.y + r.bottom as f32,
                    },
                    &brush,
                );
            }
        }
        Ok(())
    }
    pub(super) fn composite_scaled(
        &self,
        pixels: &mut [u8],
        work: Rect,
        width: usize,
        height: usize,
    ) -> Result<()> {
        let sx = width as f32 / work.width;
        let sy = height as f32 / work.height;
        for r in self.region.rectangles()? {
            let left = ((self.rect.x - work.x + r.left as f32) * sx)
                .ceil()
                .clamp(0., width as f32) as usize;
            let right = ((self.rect.x - work.x + r.right as f32) * sx)
                .ceil()
                .clamp(0., width as f32) as usize;
            let top = ((self.rect.y - work.y + r.top as f32) * sy)
                .ceil()
                .clamp(0., height as f32) as usize;
            let bottom = ((self.rect.y - work.y + r.bottom as f32) * sy)
                .ceil()
                .clamp(0., height as f32) as usize;
            for y in top..bottom {
                for x in left..right {
                    let p = &mut pixels[(y * width + x) * 4..(y * width + x + 1) * 4];
                    for channel in p.iter_mut() {
                        *channel = (self.alpha as u16
                            + (*channel as u16 * (255 - self.alpha as u16) + 127) / 255)
                            .min(255) as u8;
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn apply(&self) -> Result<()> {
        // Preserve native HRESULT only. windows::Error::new with a message would
        // import RoOriginateErrorW (Win8) even on the dedicated Win7 baseline.
        // Geometry/alpha are immutable for this scene-owned window. Configure
        // their native ownership once; restoration forces the current native
        // paint without replacing unchanged region/style state.
        // SAFETY: native region ownership transfers only after successful
        // SetWindowRgn. No full-size application DIB is allocated by these calls.
        unsafe {
            if !self.region_attached.get() {
                let transferred = self.region.copy()?;
                if SetWindowRgn(self.window.0, Some(transferred.0), false) == 0 {
                    return Err(Error::from_thread());
                }
                std::mem::forget(transferred);
                self.region_attached.set(true);
                SetWindowPos(
                    self.window.0,
                    None,
                    self.rect.x.round() as i32,
                    self.rect.y.round() as i32,
                    self.rect.width.ceil() as i32,
                    self.rect.height.ceil() as i32,
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
                )?;
            }
            if !self.alpha_set.get() {
                SetLayeredWindowAttributes(self.window.0, COLORREF(0), self.alpha, LWA_ALPHA)?;
                self.alpha_set.set(true);
            }
            // This scene owns immutable white geometry and alpha. Restore always
            // forces current paint; immutable style needs no reinitialization.
            if !RedrawWindow(
                Some(self.window.0),
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW,
            )
            .as_bool()
            {
                return Err(Error::from_thread());
            }
        }
        Ok(())
    }
    pub(super) fn composite(
        &self,
        pixels: &mut [u8],
        work: Rect,
        width: usize,
        height: usize,
    ) -> Result<()> {
        // Use the same actual GDI region geometry as the native background, not a
        // screenshot of an input desktop or a separately guessed rounded shape.
        for r in self.region.rectangles()? {
            let left = ((self.rect.x - work.x).round() as i64 + i64::from(r.left))
                .clamp(0, width as i64) as usize;
            let right = ((self.rect.x - work.x).round() as i64 + i64::from(r.right))
                .clamp(0, width as i64) as usize;
            let top = ((self.rect.y - work.y).round() as i64 + i64::from(r.top))
                .clamp(0, height as i64) as usize;
            let bottom = ((self.rect.y - work.y).round() as i64 + i64::from(r.bottom))
                .clamp(0, height as i64) as usize;
            for y in top..bottom {
                for x in left..right {
                    let p = &mut pixels[(y * width + x) * 4..(y * width + x + 1) * 4];
                    for channel in &mut p[..3] {
                        *channel = (u16::from(self.alpha)
                            + u16::from(*channel) * (255 - u16::from(self.alpha)) / 255)
                            as u8;
                    }
                }
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn verify_native_paint(&self) -> Result<()> {
        let mut alpha = 0;
        let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
        let mut client = RECT::default();
        // SAFETY: own HWND with exclusively owned fixed-size readback outputs.
        unsafe {
            GetLayeredWindowAttributes(self.window.0, None, Some(&mut alpha), Some(&mut flags))?;
            GetClientRect(self.window.0, &mut client)?;
        }
        if alpha != self.alpha
            || !flags.contains(LWA_ALPHA)
            || client.right != self.rect.width.ceil() as i32
            || client.bottom != self.rect.height.ceil() as i32
        {
            return Err(Error::from_hresult(E_FAIL));
        }
        let copy = Region::owned(unsafe { CreateRectRgn(0, 0, 0, 0) })?;
        // SAFETY: copy is an owned output region; verify the actual HWND received
        // precisely the intended full-fill/hollow-border geometry.
        unsafe {
            if GetWindowRgn(self.window.0, copy.0) == RGN_ERROR
                || !EqualRgn(copy.0, self.region.0).as_bool()
            {
                return Err(Error::from_hresult(E_FAIL));
            }
        }
        let sample = Dib::new(8, 8)?;
        // SAFETY: explicit print of this own hidden white background into an 8x8
        // owned test DC; it never captures another window or changes the desktop.
        unsafe {
            let _ = SendMessageW(
                self.window.0,
                WM_PRINTCLIENT,
                Some(WPARAM(sample.dc.0 as usize)),
                Some(LPARAM(0)),
            );
        }
        if !sample
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [255, 255, 255])
        {
            return Err(Error::from_hresult(E_FAIL));
        }
        Ok(())
    }
}
pub(super) struct Backdrop {
    pub(super) fill: Layer,
    pub(super) edge: Layer,
}
impl Backdrop {
    pub(super) fn new(
        class: &Class,
        rect: Rect,
        surface_alpha: f32,
        edge_alpha: f32,
    ) -> Result<Self> {
        Ok(Self {
            fill: Layer::new(class, rect, surface_alpha, false)?,
            edge: Layer::new(class, rect, edge_alpha, true)?,
        })
    }
    pub(super) fn new_partitioned(
        class: &Class,
        rect: Rect,
        surface: f32,
        edge: f32,
        component: f32,
        excluded: &[Rect],
    ) -> Result<Self> {
        Ok(Self {
            fill: Layer::partitioned(class, rect, surface, component, false, surface, excluded)?,
            edge: Layer::partitioned(class, rect, edge, component, true, surface, excluded)?,
        })
    }
    pub(super) fn draw_patch(&self, target: &ID2D1DCRenderTarget, patch: Rect) -> Result<()> {
        self.fill.draw_patch(target, patch)?;
        self.edge.draw_patch(target, patch)
    }
    pub(super) fn hide(&self) {
        self.fill.window.hide();
        self.edge.window.hide();
    }
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: this callback owns no userdata or retained DC; every native paint
    // pairs BeginPaint/EndPaint, and all ABI unwinding is contained here.
    match std::panic::catch_unwind(|| unsafe {
        match msg {
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let _ = FillRect(dc, &ps.rcPaint, HBRUSH(GetStockObject(WHITE_BRUSH).0));
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_PRINTCLIENT => {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let _ = FillRect(
                    HDC(wp.0 as *mut _),
                    &rect,
                    HBRUSH(GetStockObject(WHITE_BRUSH).0),
                );
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }) {
        Ok(v) => v,
        Err(_) => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
                PostQuitMessage(1);
            }
            LRESULT(0)
        }
    }
}
