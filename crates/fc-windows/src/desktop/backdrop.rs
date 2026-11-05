//! Full-size static rounded glass without an application-owned full-size DIB.
//! Fill and reflection use independent constant-alpha layered GDI regions. Their
//! static curved highlights share geometry/colours with the D2D card material. Windows
//! owns any compositor/backing allocations; those are not counted as our DIBs.
use super::*;

const BACKDROP_CLASS: windows::core::PCWSTR = w!("FutureClassroom.Desktop.Backdrop.v1");
// Reflection strength is a multiplier of this fixed material baseline, never of
// the centre fill. No wallpaper sampling or time-dependent material state.
const REFLECTION_BASE: f32 = 0.32;
#[derive(Clone)]
struct MaterialCell {
    rect: RECT,
    rgb: [u8; 3],
}
fn cells(region: &Region, width: i32, height: i32, border: bool) -> Result<Vec<MaterialCell>> {
    let mut result = Vec::new();
    for r in region.rectangles()? {
        let step_x = (width / 32).max(1);
        let step_y = (height / 32).max(1);
        let mut y = r.top;
        while y < r.bottom {
            let bottom = if border {
                (y + if r.bottom - r.top <= 4 { 1 } else { step_y }).min(r.bottom)
            } else {
                r.bottom
            };
            let mut x = r.left;
            while x < r.right {
                let right = if border {
                    (x + if r.right - r.left <= 4 { 1 } else { step_x }).min(r.right)
                } else {
                    r.right
                };
                let u = (x + right) as f32 / (2. * width as f32);
                let v = (y + bottom) as f32 / (2. * height as f32);
                // Keep a continuous fixed-width rim; only its lighting varies.
                let rgb = if border {
                    let shine = (1. - ((u - 0.20) * 2.).abs()).max(0.);
                    let distance = ((x + right) as f32 / 2.)
                        .min(width as f32 - (x + right) as f32 / 2.)
                        .min((y + bottom) as f32 / 2.)
                        .min(height as f32 - (y + bottom) as f32 / 2.);
                    let refracted = if distance > 2. { 0.08 } else { 0. };
                    let light = (0.48 + 0.39 * (1. - v) + 0.13 * shine - refracted).min(1.);
                    [
                        (255. * light) as u8,
                        (255. * (light + 0.025).min(1.)) as u8,
                        (255. * (light + 0.07).min(1.)) as u8,
                    ]
                } else {
                    [255; 3]
                };
                result.push(MaterialCell {
                    rect: RECT {
                        left: x,
                        top: y,
                        right,
                        bottom,
                    },
                    rgb,
                });

                x = right;
            }
            y = bottom;
        }
    }
    Ok(result)
}
fn material(width: i32, height: i32, border: bool) -> Result<(Region, Vec<MaterialCell>)> {
    let original = Region::shape(width, height, border)?;
    let cells = cells(&original, width, height, border)?;
    if !border {
        return Ok((original, cells));
    }
    let region = Region::owned(unsafe { CreateRectRgn(0, 0, 0, 0) })?;
    for cell in &cells {
        let r = cell.rect;
        let part = Region::owned(unsafe { CreateRectRgn(r.left, r.top, r.right, r.bottom) })?;
        if unsafe { CombineRgn(Some(region.0), Some(region.0), Some(part.0), RGN_OR) } == RGN_ERROR
        {
            return Err(Error::from_thread());
        }
    }
    Ok((region, cells))
}
fn draw_cells(
    target: &ID2D1DCRenderTarget,
    cells: &[MaterialCell],
    x: f32,
    y: f32,
    alpha: f32,
) -> Result<()> {
    unsafe {
        let brush = target.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)?;
        for cell in cells {
            brush.SetColor(&D2D1_COLOR_F {
                r: f32::from(cell.rgb[0]) / 255.,
                g: f32::from(cell.rgb[1]) / 255.,
                b: f32::from(cell.rgb[2]) / 255.,
                a: alpha,
            });
            let r = cell.rect;
            target.FillRectangle(
                &D2D_RECT_F {
                    left: x + r.left as f32,
                    top: y + r.top as f32,
                    right: x + r.right as f32,
                    bottom: y + r.bottom as f32,
                },
                &brush,
            );
        }
    }
    Ok(())
}
pub(super) fn draw_material(
    target: &ID2D1DCRenderTarget,
    width: f32,
    height: f32,
    surface: f32,
    edge: f32,
) -> Result<()> {
    for (border, alpha) in [(false, surface), (true, edge * REFLECTION_BASE)] {
        let (_, cells) = material(width.ceil() as i32, height.ceil() as i32, border)?;
        draw_cells(target, &cells, 0., 0., alpha)?;
    }
    Ok(())
}
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
                let inner = Self::owned(CreateRoundRectRgn(3, 3, width - 3, height - 3, 36, 36))?;
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
    // Declaration order is destruction order: DestroyWindow (including its
    // final callbacks) completes before the immutable userdata Box is released.
    pub(super) window: NativeWindow,
    region: Region,
    pub(super) rect: Rect,
    alpha: u8,
    paint_region: Option<Vec<MaterialCell>>,
    paint_alpha: f32,
    paint: Box<NativePaint>,
    region_attached: Cell<bool>,
    alpha_set: Cell<bool>,
}
struct NativePaint {
    cells: Vec<MaterialCell>,
    raw: f32,
    fill: f32,
}
impl NativePaint {
    fn color(&self, cell: &MaterialCell) -> [u8; 3] {
        let alpha = self.raw + self.fill * (1. - self.raw);
        cell.rgb.map(|c| {
            if alpha == 0. {
                c
            } else {
                ((f32::from(c) * self.raw + 255. * self.fill * (1. - self.raw)) / alpha).round()
                    as u8
            }
        })
    }
    fn draw(&self, dc: HDC) {
        // Borrow the DC stock brush; no bitmap, captured desktop or retained DC.
        unsafe {
            let brush = HBRUSH(GetStockObject(DC_BRUSH).0);
            let previous = GetDCBrushColor(dc);
            for cell in &self.cells {
                let rgb = self.color(cell);
                SetDCBrushColor(
                    dc,
                    COLORREF(u32::from(rgb[0]) | u32::from(rgb[1]) << 8 | u32::from(rgb[2]) << 16),
                );
                let _ = FillRect(dc, &cell.rect, brush);
            }
            SetDCBrushColor(dc, previous);
        }
    }
}
impl Layer {
    fn new(class: &Class, rect: Rect, alpha: f32, border: bool) -> Result<Self> {
        let (region, cells) =
            material(rect.width.ceil() as i32, rect.height.ceil() as i32, border)?;
        let alpha = alpha * if border { REFLECTION_BASE } else { 1. };
        Self::with_region(
            class,
            rect,
            alpha,
            region,
            None,
            alpha,
            NativePaint {
                cells,
                raw: alpha,
                fill: 0.,
            },
        )
    }
    fn with_region(
        class: &Class,
        rect: Rect,
        alpha: f32,
        region: Region,
        paint_region: Option<Vec<MaterialCell>>,
        paint_alpha: f32,
        paint: NativePaint,
    ) -> Result<Self> {
        // SAFETY: initially hidden and always transparent/non-activating.
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
            paint: Box::new(paint),
            region_attached: Cell::new(false),
            alpha_set: Cell::new(false),
        };
        // Box address remains stable until Layer destroys its HWND. The callback
        // only borrows immutable paint data during synchronous native painting.
        unsafe {
            SetWindowLongPtrW(
                layer.window.0,
                GWLP_USERDATA,
                (&*layer.paint as *const NativePaint) as _,
            );
        }
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
        let (paint, cells) = material(rect.width.ceil() as i32, rect.height.ceil() as i32, border)?;
        let region = paint.copy()?;
        let raw = raw * if border { REFLECTION_BASE } else { 1. };
        // SAFETY: each HRGN is locally owned. Difference results stay in the
        // destination owner and are transferred only by with_region/apply.
        unsafe {
            if !border {
                let (edge, _) =
                    material(rect.width.ceil() as i32, rect.height.ceil() as i32, true)?;
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
        Self::with_region(
            class,
            rect,
            native * component,
            region,
            Some(cells.clone()),
            raw,
            NativePaint {
                cells,
                raw,
                fill: if border { fill } else { 0. },
            },
        )
    }
    fn draw_patch(&self, target: &ID2D1DCRenderTarget, patch: Rect) -> Result<()> {
        let Some(cells) = &self.paint_region else {
            return Ok(());
        };
        // SAFETY: only this scene's exact retained GDI region is rasterized onto
        // its own small D2D target. It is the large card's shape, no inner frame.
        draw_cells(
            target,
            cells,
            self.rect.x - patch.x,
            self.rect.y - patch.y,
            self.paint_alpha,
        )
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
        for (r, rgb) in self.painted_rectangles()? {
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
                    for (channel, source) in p[..3].iter_mut().zip([rgb[2], rgb[1], rgb[0]]) {
                        *channel = ((u16::from(source) * u16::from(self.alpha)
                            + u16::from(*channel) * (255 - u16::from(self.alpha))
                            + 127)
                            / 255) as u8;
                    }
                    p[3] = (u16::from(self.alpha)
                        + (u16::from(p[3]) * (255 - u16::from(self.alpha)) + 127) / 255)
                        as u8;
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
            // This scene owns immutable material geometry and alpha. Restore always
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
        for (r, rgb) in self.painted_rectangles()? {
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
                    for (channel, source) in p[..3].iter_mut().zip([rgb[2], rgb[1], rgb[0]]) {
                        *channel = ((u16::from(source) * u16::from(self.alpha)
                            + u16::from(*channel) * (255 - u16::from(self.alpha)))
                            / 255) as u8;
                    }
                }
            }
        }
        Ok(())
    }
    fn painted_rectangles(&self) -> Result<Vec<(RECT, [u8; 3])>> {
        let mut result = Vec::new();
        for r in self.region.rectangles()? {
            for cell in &self.paint.cells {
                let rect = RECT {
                    left: r.left.max(cell.rect.left),
                    top: r.top.max(cell.rect.top),
                    right: r.right.min(cell.rect.right),
                    bottom: r.bottom.min(cell.rect.bottom),
                };
                if rect.left < rect.right && rect.top < rect.bottom {
                    result.push((rect, self.paint.color(cell)));
                }
            }
        }
        Ok(result)
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
        let cell = self
            .paint
            .cells
            .first()
            .ok_or_else(|| Error::from_hresult(E_FAIL))?;
        // SAFETY: explicit print of this own hidden static background into an 8x8
        // owned test DC; it never captures another window or changes the desktop.
        unsafe {
            let _ = SetViewportOrgEx(sample.dc, -cell.rect.left, -cell.rect.top, None);
            let _ = SendMessageW(
                self.window.0,
                WM_PRINTCLIENT,
                Some(WPARAM(sample.dc.0 as usize)),
                Some(LPARAM(0)),
            );
        }
        let rgb = self.paint.color(cell);
        if sample.pixels()[..3] != [rgb[2], rgb[1], rgb[0]] {
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
                let paint = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const NativePaint;
                if let Some(paint) = paint.as_ref() {
                    paint.draw(dc);
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_PRINTCLIENT => {
                let paint = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const NativePaint;
                if let Some(paint) = paint.as_ref() {
                    paint.draw(HDC(wp.0 as *mut _));
                }
                LRESULT(0)
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, msg, wp, lp)
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
