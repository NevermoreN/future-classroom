//! The resident desktop's UI-thread renderer. No private pump or worker thread.
//! Native publication always rechecks a real system sample and an exact lease.
use crate::native::{Apartment, Dib};
use fc_domain::{
    desktop::{ClockMode, ComponentKind, TextTone},
    identity::Rgb,
    ids::SceneGeneration,
    time::LocalDateTime,
};
use fc_ports::{MeasureError, TextMeasure};
use fc_presentation::{
    scene::{CardContent, SceneCard, SceneSnapshot},
    Rect,
};
use fc_runtime::health::{EffectIntent, EffectLease, HealthGate};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    marker::PhantomData,
    path::Path,
    rc::Rc,
};
use windows::{
    core::{w, Error, Result},
    Win32::{
        Foundation::*,
        Graphics::{Direct2D::Common::*, Direct2D::*, DirectWrite::*, Dxgi::Common::*, Gdi::*},
        System::{LibraryLoader::*, RemoteDesktop::*, StationsAndDesktops::*, Threading::*},
        UI::WindowsAndMessaging::*,
    },
};

pub const DESKTOP_SCOPE: u64 = 0xfc00_0001;
pub const DIB_BUDGET: u64 = 16 * 1024 * 1024;
const MAX_TEXT_UNITS: usize = 262_144;

pub struct PublicationGuard<'a> {
    pub gate: &'a HealthGate,
    pub lease: EffectLease,
    pub current_generation: SceneGeneration,
    pub settings_open: bool,
}
#[derive(Clone, Debug, Default)]
pub struct DesktopMetrics {
    pub components: usize,
    pub surfaces: usize,
    pub dib_bytes: u64,
    pub drawn_frames: u64,
    pub uploaded_frames: u64,
    pub visible_windows: usize,
    pub input_regions: usize,
    pub forced_uploads: u64,
    pub denied_publications: u64,
    pub text_layouts: u64,
    pub pages: Vec<PageMetrics>,
}
#[derive(Clone, Debug)]
pub struct PageMetrics {
    pub component: ComponentKind,
    pub page: usize,
    pub pages: usize,
    pub utf16_units: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentOutcome {
    Published,
    Unchanged,
    Hidden,
    Offscreen,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopEvent {
    HomeworkBatch {
        generation: SceneGeneration,
        offset: u64,
    },
}
#[derive(Clone)]
struct InputContext {
    generation: SceneGeneration,
    source_revision: fc_domain::ids::Revision,
    captured_at: LocalDateTime,
    valid_until: Option<LocalDateTime>,
    gate: HealthGate,
    lease: EffectLease,
    probe: bool,
}
struct InputState {
    context: RefCell<Option<InputContext>>,
    pressed: Cell<Option<(SceneGeneration, bool)>>,
    turns: RefCell<VecDeque<(SceneGeneration, bool)>>,
}
struct InputStrip {
    surface: Surface,
    state: Box<InputState>,
}
impl InputStrip {
    fn hide(&self) {
        self.state.context.replace(None);
        self.state.pressed.set(None);
        self.state.turns.borrow_mut().clear();
        self.surface.window.hide();
    }
}

struct Class {
    instance: HINSTANCE,
}
impl Class {
    fn new() -> Result<Self> {
        // SAFETY: static class name and ABI callback remain alive until all windows drop.
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let wc = WNDCLASSW {
                hInstance: instance,
                lpszClassName: w!("FutureClassroom.Desktop.v1"),
                lpfnWndProc: Some(window_proc),
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(Error::from_thread());
            }
            Ok(Self { instance })
        }
    }
}
impl Drop for Class {
    fn drop(&mut self) {
        // SAFETY: every owned HWND is destroyed before its registration owner.
        unsafe {
            let _ = UnregisterClassW(w!("FutureClassroom.Desktop.v1"), Some(self.instance));
        }
    }
}
struct NativeWindow(HWND);
impl NativeWindow {
    fn new(class: &Class) -> Result<Self> {
        // SAFETY: initially hidden, non-topmost, non-activating and always click-through.
        unsafe {
            Ok(Self(CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("FutureClassroom.Desktop.v1"),
                w!("未来教室桌面组件"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(class.instance),
                None,
            )?))
        }
    }
    fn hide(&self) {
        // SAFETY: this UI-thread owner retains the live HWND; hiding cannot activate.
        unsafe {
            let _ = ShowWindow(self.0, SW_HIDE);
        }
    }
}
impl Drop for NativeWindow {
    fn drop(&mut self) {
        // SAFETY: unique HWND ownership, destroyed before any callback state drops.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: creation parameters point to a stable UI-owned Box until NCDESTROY;
    // callbacks form shared references only and detach userdata before Box release.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as _);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const InputState;
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if let Some(state) = state.as_ref() {
            if let Some(result) = input_message(hwnd, msg, lp, state) {
                return result;
            }
        }
        match msg {
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_CLOSE => {
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    })) {
        Ok(v) => v,
        Err(_) => {
            // SAFETY: contain failure on this callback thread and its own window.
            // SAFETY: this callback's own HWND, with no outstanding mutable borrow.
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
                PostQuitMessage(1);
            }
            LRESULT(0)
        }
    }
}

struct Graphics {
    factory: ID2D1Factory,
    write: IDWriteFactory,
}
impl Graphics {
    fn new() -> Result<Self> {
        // SAFETY: the owning STA apartment precedes these UI-thread COM factories.
        unsafe {
            Ok(Self {
                factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                write: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
            })
        }
    }
    fn format(&self, size: f32, center: bool) -> Result<IDWriteTextFormat> {
        // SAFETY: static strings and apartment-owned factory; COM returns owned refs.
        unsafe {
            let f = self.write.CreateTextFormat(
                w!("Microsoft YaHei"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("zh-CN"),
            )?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
            if center {
                f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            }
            Ok(f)
        }
    }
    fn layout(&self, text: &str, width: f32, size: f32, center: bool) -> Result<IDWriteTextLayout> {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        if units.is_empty() {
            units.push(0x200b);
        }
        if units.len() > MAX_TEXT_UNITS
            || !width.is_finite()
            || width < 8.
            || !(10. ..=96.).contains(&size)
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        // SAFETY: bounded UTF-16 and owned format remain alive for synchronous copy.
        unsafe {
            self.write
                .CreateTextLayout(&units, &self.format(size, center)?, width, 1_000_000.)
        }
    }
    fn target(&self, dib: &Dib) -> Result<ID2D1DCRenderTarget> {
        // SAFETY: the caller retains this selected DIB/DC through target EndDraw/drop.
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
            let target = self.factory.CreateDCRenderTarget(&props)?;
            target.BindDC(
                dib.dc,
                &RECT {
                    left: 0,
                    top: 0,
                    right: dib.width,
                    bottom: dib.height,
                },
            )?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(target)
        }
    }
}
/// Real DirectWrite measurement used by the same scene compositor as presentation.
pub struct NativeTextMeasure {
    graphics: Graphics,
    _apartment: Apartment,
}
impl NativeTextMeasure {
    pub fn new() -> Result<Self> {
        let a = Apartment::new()?;
        Ok(Self {
            graphics: Graphics::new()?,
            _apartment: a,
        })
    }
}
impl TextMeasure for NativeTextMeasure {
    fn height(&self, text: &str, width: f32, font: f32) -> std::result::Result<f32, MeasureError> {
        let layout = self
            .graphics
            .layout(text, width, font, false)
            .map_err(|_| MeasureError::Unmeasurable)?;
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe {
            layout
                .GetMetrics(&mut m)
                .map_err(|_| MeasureError::Unmeasurable)?;
        }
        Ok(m.height)
    }
}
#[derive(Clone)]
struct Block {
    layout: IDWriteTextLayout,
    top: f32,
    height: f32,
    fill: Option<(Rgb, f32, f32)>,
    lines: Vec<(f32, f32)>,
}
struct ContentLayout {
    blocks: Vec<Block>,
    pages: Vec<f32>,
    units: usize,
}
struct Surface {
    window: NativeWindow,
    dib: Dib,
    rect: Rect,
    dirty: bool,
}
struct Card {
    source: SceneCard,
    tone: TextTone,
    calendar_unverified: bool,
    render_pending: bool,
    input: Option<InputStrip>,
    body: ContentLayout,
    page: usize,
    surface: Surface,
    // Recess's static large card and its small clock overlay are separate, so a
    // one-second update never repaints or uploads a large work-area-sized card.
    dynamic: Option<Surface>,
    clock_tick: Option<u32>,
}
impl Card {
    fn hide(&self) {
        self.surface.window.hide();
        if let Some(d) = &self.dynamic {
            d.window.hide();
        }
        if let Some(i) = &self.input {
            i.hide();
        }
    }
}

pub struct DesktopRenderer {
    cards: Vec<Card>,
    graphics: Graphics,
    class: Class,
    _apartment: Apartment,
    hidden_probe: bool,
    hidden: bool,
    last_scene: Option<SceneGeneration>,
    last_revision: Option<fc_domain::ids::Revision>,
    metrics: DesktopMetrics,
    _thread: PhantomData<Rc<()>>,
}
impl DesktopRenderer {
    fn input_strip(&self, rect: Rect) -> Result<InputStrip> {
        let state = Box::new(InputState {
            context: RefCell::new(None),
            pressed: Cell::new(None),
            turns: RefCell::new(VecDeque::with_capacity(2)),
        });
        // SAFETY: stable UI-owned Box is retained through HWND destruction. Only
        // these two small button rectangles receive input; no activation occurs.
        let window = NativeWindow(unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("FutureClassroom.Desktop.v1"),
                w!("只读翻页"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(self.class.instance),
                Some((&raw const *state).cast()),
            )?
        });
        let surface = Surface {
            window,
            dib: Dib::new(rect.width.ceil() as i32, rect.height.ceil() as i32)?,
            rect,
            dirty: true,
        };
        Ok(InputStrip { surface, state })
    }
    pub fn take_events(&mut self) -> Result<Vec<DesktopEvent>> {
        let mut output = Vec::with_capacity(2);
        for card in &mut self.cards {
            let Some(input) = &card.input else { continue };
            let turns: Vec<_> = input.state.turns.borrow_mut().drain(..).collect();
            for (generation, forward) in turns {
                if self.last_scene != Some(generation) {
                    continue;
                }
                let valid = input
                    .state
                    .context
                    .borrow()
                    .as_ref()
                    .is_some_and(input_allowed);
                if !valid {
                    continue;
                }
                if forward && card.page + 1 < card.body.pages.len() {
                    card.page += 1;
                    card.render_pending = true;
                } else if !forward && card.page > 0 {
                    card.page -= 1;
                    card.render_pending = true;
                } else if card.source.kind == ComponentKind::Homework {
                    let offset = if forward {
                        card.source
                            .page
                            .offset
                            .saturating_add(card.source.page.shown)
                    } else {
                        card.source.page.offset.saturating_sub(1)
                    };
                    if offset < card.source.page.total
                        && offset != card.source.page.offset
                        && output.len() < 2
                    {
                        output.push(DesktopEvent::HomeworkBatch { generation, offset });
                    }
                }
            }
        }
        Ok(output)
    }
    pub fn probe_next_page(&self, kind: ComponentKind, forward: bool) -> Result<()> {
        if !self.hidden_probe {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let input = self
            .cards
            .iter()
            .find(|c| c.source.kind == kind)
            .and_then(|c| c.input.as_ref())
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        let x = if forward { 54 } else { 18 };
        let coordinate = LPARAM(x | (12 << 16));
        // SAFETY: own hidden button window; no global input, focus or cursor change.
        unsafe {
            let _ = SendMessageW(
                input.surface.window.0,
                WM_LBUTTONDOWN,
                Some(WPARAM(0)),
                Some(coordinate),
            );
            let _ = SendMessageW(
                input.surface.window.0,
                WM_LBUTTONUP,
                Some(WPARAM(0)),
                Some(coordinate),
            );
        }
        Ok(())
    }
    pub fn new() -> Result<Self> {
        Self::create(false)
    }
    /// This explicit diagnostic mode never calls UpdateLayeredWindow or ShowWindow.
    pub fn new_hidden() -> Result<Self> {
        Self::create(true)
    }
    fn create(hidden_probe: bool) -> Result<Self> {
        let a = Apartment::new()?;
        let g = Graphics::new()?;
        let class = Class::new()?;
        Ok(Self {
            cards: Vec::new(),
            graphics: g,
            class,
            _apartment: a,
            hidden_probe,
            hidden: true,
            last_scene: None,
            last_revision: None,
            metrics: DesktopMetrics::default(),
            _thread: PhantomData,
        })
    }
    pub fn work_area(&self) -> Result<Rect> {
        work_area()
    }
    pub fn last_metrics(&self) -> DesktopMetrics {
        self.metrics.clone()
    }
    pub fn diagnostics(&self) -> DesktopMetrics {
        self.last_metrics()
    }
    pub fn hide(&mut self) {
        for c in &mut self.cards {
            c.hide();
            c.surface.dirty = true;
            if let Some(d) = &mut c.dynamic {
                d.dirty = true;
            }
        }
        self.hidden = true;
        self.metrics.visible_windows = 0;
        self.metrics.input_regions = 0;
    }
    /// Read-only pagination changes no domain data and receives no arbitrary path.
    /// The composition root can expose it through its explicit trusted controls.
    pub fn next_page(&mut self, kind: ComponentKind) -> Result<bool> {
        let Some(c) = self.cards.iter_mut().find(|c| c.source.kind == kind) else {
            return Ok(false);
        };
        if c.body.pages.len() < 2 {
            return Ok(false);
        };
        c.page = (c.page + 1) % c.body.pages.len();
        c.render_pending = true;
        Ok(true)
    }
    pub fn present(
        &mut self,
        scene: &SceneSnapshot,
        now: LocalDateTime,
        guard: &PublicationGuard<'_>,
    ) -> Result<PresentOutcome> {
        let result = self.present_inner(scene, now, guard);
        if result.is_err() {
            self.hide();
            self.refresh_metrics();
        }
        result
    }
    fn present_inner(
        &mut self,
        scene: &SceneSnapshot,
        now: LocalDateTime,
        guard: &PublicationGuard<'_>,
    ) -> Result<PresentOutcome> {
        if !scene_allowed(scene, now, guard) {
            self.metrics.denied_publications += 1;
            self.hide();
            return Ok(PresentOutcome::Hidden);
        }
        if !self.hidden_probe && !system_allows()? {
            self.hide();
            return Ok(PresentOutcome::Hidden);
        }
        let changed = self.last_scene != Some(scene.generation)
            || self.cards.len() != scene.cards.len()
            || self.cards.iter().zip(&scene.cards).any(|(a, b)| {
                a.source != *b
                    || a.tone != scene.text_tone
                    || a.calendar_unverified != scene.public_calendar_unverified
            });
        if changed {
            // Never expose a partially prepared scene. Drop old surfaces first so
            // old+new DIB allocations cannot briefly exceed the aggregate budget.
            let prior_pages: Vec<_> = if self.last_revision == Some(scene.source_revision) {
                self.cards
                    .iter()
                    .map(|c| (c.source.kind, c.source.page.offset, c.page))
                    .collect()
            } else {
                Vec::new()
            };
            self.hide();
            self.cards.clear();
            self.metrics.dib_bytes = 0;
            if scene.cards.len() > 8 {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            let mut total = 0u64;
            let mut kinds = Vec::with_capacity(8);
            for card in &scene.cards {
                if kinds.contains(&card.kind) {
                    return Err(Error::from_hresult(E_INVALIDARG));
                }
                kinds.push(card.kind);
                validate_card(card)?;
                let body = prepare_content(&self.graphics, card, scene.public_calendar_unverified)?;
                let dynamic_rect = match card.content {
                    CardContent::Recess { .. } => Some(Rect {
                        x: card.rect.x + 16.,
                        y: card.rect.y + 60.,
                        width: (card.rect.width - 32.).min(560.),
                        height: 100.,
                    }),
                    CardContent::Clock(_) => Some(Rect {
                        x: card.rect.x + 16.,
                        y: card.rect.y + 42.,
                        width: card.rect.width - 32.,
                        height: card.rect.height - 54.,
                    }),
                    _ => None,
                };
                let input_rect = (body.pages.len() > 1
                    || card.kind == ComponentKind::Homework
                        && (card.page.offset > 0
                            || card.page.offset + card.page.shown < card.page.total))
                    .then_some(Rect {
                        x: card.rect.x + card.rect.width - 86.,
                        y: card.rect.y + 8.,
                        width: 72.,
                        height: 24.,
                    });
                for r in std::iter::once(card.rect)
                    .chain(dynamic_rect)
                    .chain(input_rect)
                {
                    total = total
                        .checked_add(surface_bytes(r)?)
                        .ok_or_else(|| Error::from_hresult(E_OUTOFMEMORY))?;
                }
                if total > DIB_BUDGET {
                    self.hide();
                    self.cards.clear();
                    return Err(Error::from_hresult(E_OUTOFMEMORY));
                }
                let surface = self.surface(card.rect)?;
                let dynamic = dynamic_rect.map(|r| self.surface(r)).transpose()?;
                let input = input_rect.map(|r| self.input_strip(r)).transpose()?;
                let page = prior_pages
                    .iter()
                    .find(|(kind, offset, _)| *kind == card.kind && *offset == card.page.offset)
                    .map_or(0, |(_, _, page)| (*page).min(body.pages.len() - 1));
                let mut c = Card {
                    source: card.clone(),
                    tone: scene.text_tone,
                    calendar_unverified: scene.public_calendar_unverified,
                    render_pending: true,
                    input,
                    body,
                    page,
                    surface,
                    dynamic,
                    clock_tick: None,
                };
                if card.component_alpha > 0. {
                    render_card(&self.graphics, &mut c)?;
                    self.metrics.drawn_frames += 1;
                }
                self.metrics.text_layouts += c.body.blocks.len() as u64;
                self.cards.push(c);
            }
            self.last_scene = Some(scene.generation);
            self.last_revision = Some(scene.source_revision);
            self.refresh_metrics();
        }
        // Rendering is paused while policy hides a normal renderer. Diagnostic
        // rendering is explicit and stays completely offscreen on any foreground.
        let mut updated = false;
        for c in &mut self.cards {
            if c.source.component_alpha == 0. {
                continue;
            }
            if c.render_pending {
                render_card(&self.graphics, c)?;
                self.metrics.drawn_frames += 1;
                updated = true;
            }
            let tick = match &c.source.content {
                CardContent::Clock(v) => Some(if v.show_seconds {
                    now.second_of_day()
                } else {
                    now.second_of_day() / 60
                }),
                CardContent::Recess { .. } => Some(now.second_of_day()),
                _ => None,
            };
            if tick.is_some() && c.clock_tick != tick {
                render_dynamic(&self.graphics, c, now)?;
                c.clock_tick = tick;
                self.metrics.drawn_frames += 1;
                updated = true;
            }
        }
        for c in &self.cards {
            if let Some(input) = &c.input {
                if c.source.component_alpha > 0. {
                    input.state.context.replace(Some(InputContext {
                        generation: scene.generation,
                        source_revision: scene.source_revision,
                        captured_at: scene.captured_at,
                        valid_until: scene.valid_until,
                        gate: guard.gate.clone(),
                        lease: guard.lease,
                        probe: self.hidden_probe,
                    }));
                }
            }
        }
        if self.hidden_probe {
            self.refresh_metrics();
            return Ok(PresentOutcome::Offscreen);
        }
        let restoring = self.hidden;
        for index in 0..self.cards.len() {
            if self.cards[index].source.component_alpha == 0. {
                continue;
            }
            let need = self.cards[index].surface.dirty || restoring;
            if need {
                if !publication_allowed(scene, guard)? {
                    self.hide();
                    return Ok(PresentOutcome::Hidden);
                }
                upload(&self.cards[index].surface)?;
                self.cards[index].surface.dirty = false;
                self.metrics.uploaded_frames += 1;
                if restoring {
                    self.metrics.forced_uploads += 1;
                }
            }
            if let Some(d) = &self.cards[index].dynamic {
                if d.dirty || restoring {
                    if !publication_allowed(scene, guard)? {
                        self.hide();
                        return Ok(PresentOutcome::Hidden);
                    }
                    upload(d)?;
                    self.metrics.uploaded_frames += 1;
                    if restoring {
                        self.metrics.forced_uploads += 1;
                    }
                    self.cards[index].dynamic.as_mut().unwrap().dirty = false;
                }
            }
            if self.cards[index]
                .input
                .as_ref()
                .is_some_and(|i| i.surface.dirty || restoring)
            {
                if !publication_allowed(scene, guard)? {
                    self.hide();
                    return Ok(PresentOutcome::Hidden);
                }
                upload(&self.cards[index].input.as_ref().unwrap().surface)?;
                self.cards[index].input.as_mut().unwrap().surface.dirty = false;
                self.metrics.uploaded_frames += 1;
            }
        }
        // All updates complete before any window becomes visible. Each individual
        // show is checked again; a failed sample hides the entire scene immediately.
        if restoring {
            for index in 0..self.cards.len() {
                if self.cards[index].source.component_alpha == 0. {
                    continue;
                }
                if !publication_allowed(scene, guard)? {
                    self.hide();
                    return Ok(PresentOutcome::Hidden);
                }
                // SAFETY: exact owned HWND, after fresh system/lease/date checks.
                unsafe {
                    let _ = ShowWindow(self.cards[index].surface.window.0, SW_SHOWNOACTIVATE);
                }
                if self.cards[index].dynamic.is_some() {
                    if !publication_allowed(scene, guard)? {
                        self.hide();
                        return Ok(PresentOutcome::Hidden);
                    }
                    // SAFETY: exact owned overlay HWND, freshly rechecked above.
                    unsafe {
                        let _ = ShowWindow(
                            self.cards[index].dynamic.as_ref().unwrap().window.0,
                            SW_SHOWNOACTIVATE,
                        );
                    }
                }
                if self.cards[index].input.is_some() {
                    if !publication_allowed(scene, guard)? {
                        self.hide();
                        return Ok(PresentOutcome::Hidden);
                    }
                    // SAFETY: exact owned overlay HWND, freshly rechecked above.
                    unsafe {
                        let _ = ShowWindow(
                            self.cards[index].input.as_ref().unwrap().surface.window.0,
                            SW_SHOWNOACTIVATE,
                        );
                    }
                }
            }
            self.hidden = false;
        }
        self.refresh_metrics();
        Ok(if changed || updated || restoring {
            PresentOutcome::Published
        } else {
            PresentOutcome::Unchanged
        })
    }
    fn surface(&self, rect: Rect) -> Result<Surface> {
        Ok(Surface {
            window: NativeWindow::new(&self.class)?,
            dib: Dib::new(rect.width.ceil() as i32, rect.height.ceil() as i32)?,
            rect,
            dirty: true,
        })
    }
    fn refresh_metrics(&mut self) {
        self.metrics.components = self.cards.len();
        self.metrics.surfaces = self
            .cards
            .iter()
            .map(|c| 1 + usize::from(c.dynamic.is_some()) + usize::from(c.input.is_some()))
            .sum();
        self.metrics.dib_bytes = self
            .cards
            .iter()
            .map(|c| {
                surface_bytes(c.surface.rect).unwrap_or(0)
                    + c.dynamic
                        .as_ref()
                        .map_or(0, |d| surface_bytes(d.rect).unwrap_or(0))
                    + c.input
                        .as_ref()
                        .map_or(0, |i| surface_bytes(i.surface.rect).unwrap_or(0))
            })
            .sum();
        self.metrics.visible_windows = if self.hidden {
            0
        } else {
            self.cards
                .iter()
                .filter(|c| c.source.component_alpha > 0.)
                .map(|c| 1 + usize::from(c.dynamic.is_some()) + usize::from(c.input.is_some()))
                .sum()
        };
        self.metrics.input_regions = if self.hidden {
            0
        } else {
            self.cards
                .iter()
                .filter(|c| c.source.component_alpha > 0. && c.input.is_some())
                .count()
                * 2
        };
        self.metrics.pages = self
            .cards
            .iter()
            .map(|c| PageMetrics {
                component: c.source.kind,
                page: c.page,
                pages: c.body.pages.len(),
                utf16_units: c.body.units,
            })
            .collect();
    }
    /// Export only the renderer's own offscreen synthetic scene, never a desktop
    /// screenshot. The one-shot BMP composition buffer is bounded to 64 MiB.
    pub fn probe_capture_bmp(&self, path: &Path) -> Result<()> {
        if !self.hidden_probe || !path.is_absolute() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let work = self.work_area()?;
        let w = work.width.ceil() as usize;
        let h = work.height.ceil() as usize;
        let bytes = w
            .checked_mul(h)
            .and_then(|v| v.checked_mul(4))
            .filter(|v| *v <= 64 * 1024 * 1024)
            .ok_or_else(|| Error::from_hresult(E_OUTOFMEMORY))?;
        let mut pixels = vec![0u8; bytes];
        for p in pixels.chunks_exact_mut(4) {
            p.copy_from_slice(&[45, 36, 28, 255]);
        }
        for c in &self.cards {
            if c.source.component_alpha == 0. {
                continue;
            }
            for surface in std::iter::once(&c.surface)
                .chain(c.dynamic.iter())
                .chain(c.input.iter().map(|i| &i.surface))
            {
                let source = surface.dib.pixels();
                let sw = surface.dib.width as usize;
                for y in 0..surface.dib.height as usize {
                    for x in 0..sw {
                        let dx = (surface.rect.x - work.x).round() as isize + x as isize;
                        let dy = (surface.rect.y - work.y).round() as isize + y as isize;
                        if dx < 0 || dy < 0 || dx >= w as isize || dy >= h as isize {
                            continue;
                        }
                        let s = &source[(y * sw + x) * 4..(y * sw + x + 1) * 4];
                        let d = &mut pixels[(dy as usize * w + dx as usize) * 4
                            ..(dy as usize * w + dx as usize + 1) * 4];
                        for k in 0..3 {
                            d[k] = (u16::from(s[k])
                                + u16::from(d[k]) * (255 - u16::from(s[3])) / 255)
                                .min(255) as u8;
                        }
                    }
                }
            }
        }
        let mut bmp = Vec::with_capacity(bytes + 54);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&((bytes + 54) as u32).to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&(w as i32).to_le_bytes());
        bmp.extend_from_slice(&(-(h as i32)).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&32u16.to_le_bytes());
        bmp.extend_from_slice(&[0; 24]);
        bmp.extend_from_slice(&pixels);
        std::fs::write(path, bmp).map_err(|_| Error::from_hresult(E_FAIL))
    }
}
impl Drop for DesktopRenderer {
    fn drop(&mut self) {
        self.hide();
        self.cards.clear();
    }
}

fn work_area() -> Result<Rect> {
    let mut r = RECT::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut r).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
    }
    Ok(Rect {
        x: r.left as f32,
        y: r.top as f32,
        width: (r.right - r.left) as f32,
        height: (r.bottom - r.top) as f32,
    })
}
fn surface_bytes(r: Rect) -> Result<u64> {
    if ![r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
        || r.width < 8.
        || r.height < 8.
        || r.width > 8192.
        || r.height > 8192.
    {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    Ok(r.width.ceil() as u64 * r.height.ceil() as u64 * 4)
}
fn validate_card(c: &SceneCard) -> Result<()> {
    surface_bytes(c.rect)?;
    if !c.font_dip.is_finite() || !(10. ..=72.).contains(&c.font_dip) {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    if ![c.component_alpha, c.surface_alpha, c.edge_alpha]
        .iter()
        .all(|v| v.is_finite() && (0. ..=1.).contains(v))
    {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    Ok(())
}
fn scene_allowed(scene: &SceneSnapshot, now: LocalDateTime, g: &PublicationGuard<'_>) -> bool {
    !g.settings_open
        && g.current_generation == scene.generation
        && g.lease.scope == DESKTOP_SCOPE
        && scene.valid_at(now)
        && g.gate.status().revision == scene.source_revision
        && g.gate
            .try_start(
                g.lease,
                EffectIntent {
                    scope: DESKTOP_SCOPE,
                    job_id: scene.generation.get(),
                },
            )
            .and_then(|p| g.gate.check_permit(&p))
            .is_ok()
}
fn publication_allowed(scene: &SceneSnapshot, g: &PublicationGuard<'_>) -> Result<bool> {
    Ok(system_allows()? && scene_allowed(scene, crate::native::local_now()?, g))
}
fn system_allows() -> Result<bool> {
    if crate::native::desktop_foreground() != fc_presentation::Foreground::Desktop
        || !crate::native::input_desktop_available()
    {
        return Ok(false);
    }
    // The active process session must be interactive; service/session-0 and
    // disconnected remote sessions cannot publish through an accessible desktop.
    unsafe {
        let mut sid = 0;
        ProcessIdToSessionId(GetCurrentProcessId(), &mut sid)?;
        let mut ptr = windows::core::PWSTR::null();
        let mut bytes = 0;
        if WTSQuerySessionInformationW(None, sid, WTSConnectState, &mut ptr, &mut bytes).is_err() {
            return Ok(false);
        }
        struct Buffer(windows::core::PWSTR);
        impl Drop for Buffer {
            fn drop(&mut self) {
                unsafe { WTSFreeMemory(self.0 .0.cast()) }
            }
        }
        let b = Buffer(ptr);
        if bytes < std::mem::size_of::<WTS_CONNECTSTATE_CLASS>() as u32 || b.0.is_null() {
            return Ok(false);
        }
        let active = *(b.0 .0.cast::<WTS_CONNECTSTATE_CLASS>()) == WTSActive;
        let desktop = match OpenInputDesktop(DF_ALLOWOTHERACCOUNTHOOK, false, DESKTOP_READOBJECTS) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        let mut name = [0u16; 64];
        let mut needed = 0;
        let good = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            128,
            Some(&mut needed),
        )
        .is_ok();
        let _ = CloseDesktop(desktop);
        Ok(active
            && good
            && needed <= 128
            && String::from_utf16_lossy(&name[..name.iter().position(|c| *c == 0).unwrap_or(64)])
                == "Default")
    }
}
fn upload(s: &Surface) -> Result<()> {
    // SAFETY: synchronous copy from the retained selected DIB to its own layered
    // HWND, following the caller's immediately preceding publication check.
    unsafe {
        let point = POINT {
            x: s.rect.x.round() as i32,
            y: s.rect.y.round() as i32,
        };
        let size = SIZE {
            cx: s.dib.width,
            cy: s.dib.height,
        };
        let src = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(
            s.window.0,
            None,
            Some(&point),
            Some(&size),
            Some(s.dib.dc),
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
    }
}
fn xy(x: f32, y: f32) -> D2D1_ELLIPSE {
    let mut e = D2D1_ELLIPSE::default();
    e.point.X = x;
    e.point.Y = y;
    e
}
fn prepare_content(g: &Graphics, c: &SceneCard, unverified: bool) -> Result<ContentLayout> {
    let width = c.rect.width - 32.;
    let viewport = c.rect.height - body_top(c, unverified) - 12.;
    if viewport < 20. {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let mut blocks = Vec::new();
    let mut top = 0.;
    let mut units = 0usize;
    let mut add = |text: String, center: bool, fill: Option<(Rgb, f32, f32)>| -> Result<()> {
        units = units
            .checked_add(text.encode_utf16().count())
            .filter(|n| *n <= MAX_TEXT_UNITS)
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        let layout = g.layout(&text, width - 16., c.font_dip, center)?;
        let mut m = DWRITE_TEXT_METRICS::default();
        // SAFETY: exclusively owned local metrics output; apartment-owned layout.
        unsafe {
            layout.GetMetrics(&mut m)?;
        }
        let mut count = 0;
        // SAFETY: count query intentionally supplies no buffer, then allocates the
        // exact bounded count and checks the second synchronous fill.
        let _ = unsafe { layout.GetLineMetrics(None, &mut count) };
        if count > MAX_TEXT_UNITS as u32 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let mut native = vec![DWRITE_LINE_METRICS::default(); count as usize];
        if count > 0 {
            unsafe {
                layout.GetLineMetrics(Some(&mut native), &mut count)?;
            }
        }
        let mut y = top + 8.;
        let lines = native
            .into_iter()
            .map(|line| {
                let result = (y, line.height);
                y += line.height;
                result
            })
            .collect();
        let height = (m.height + 16.).max(if fill.is_some() { 44. } else { 24. });
        blocks.push(Block {
            layout,
            top,
            height,
            fill,
            lines,
        });
        top += height + 8.;
        Ok(())
    };
    match &c.content {
        CardContent::Text(text) => add(text.clone(), false, None)?,
        CardContent::Courses(rows) => {
            if rows.len() > 128 {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            for r in rows {
                if !r.fill_alpha.is_finite()
                    || !(0. ..=1.).contains(&r.fill_alpha)
                    || !r.stroke_width.is_finite()
                    || !(0. ..=4.).contains(&r.stroke_width)
                {
                    return Err(Error::from_hresult(E_INVALIDARG));
                }
                add(
                    if r.current {
                        format!("{}\n正在上课", r.name)
                    } else {
                        r.name.clone()
                    },
                    true,
                    Some((r.color, r.fill_alpha, r.stroke_width)),
                )?;
            }
        }
        CardContent::Homework(rows) => {
            if rows.len() > 64 {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            for r in rows {
                add(
                    format!("{} · {}", r.subject_name, r.teacher_name),
                    false,
                    Some((r.color, r.fill_alpha, 0.)),
                )?;
                add(r.text.clone(), false, None)?;
            }
        }
        CardContent::MaterialsUnavailable => add("资料尚未就绪".into(), false, None)?,
        _ => {}
    }
    let total = blocks.last().map_or(0., |b| {
        b.top + b.height - if b.fill.is_none() { 8. } else { 0. }
    });
    let mut pages = vec![0.];
    if total > viewport {
        let mut starts = Vec::new();
        for b in &blocks {
            starts.push(b.top);
            // A course capsule that fits the viewport stays together; only
            // oversized text blocks may split on measured native line boundaries.
            if b.fill.is_none() || b.height > viewport {
                starts.extend(b.lines.iter().map(|(y, _)| (*y).max(b.top)));
            }
        }
        starts.sort_by(f32::total_cmp);
        starts.dedup();
        let mut offset = 0.;
        while total - offset > viewport {
            let next = starts
                .iter()
                .copied()
                .rfind(|y| *y > offset + 0.5 && *y <= offset + viewport)
                .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
            pages.push(next);
            offset = next;
            if pages.len() > fc_presentation::MAX_PAGES as usize {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
        }
    }
    Ok(ContentLayout {
        blocks,
        pages,
        units,
    })
}
fn color(rgb: Rgb, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(rgb.red) / 255.,
        g: f32::from(rgb.green) / 255.,
        b: f32::from(rgb.blue) / 255.,
        a,
    }
}
fn ink(tone: TextTone, a: f32) -> D2D1_COLOR_F {
    let v = if tone == TextTone::Light { 0.97 } else { 0.08 };
    D2D1_COLOR_F {
        r: v,
        g: v,
        b: v,
        a,
    }
}
fn bounds(l: f32, t: f32, r: f32, b: f32, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: l,
            top: t,
            right: r,
            bottom: b,
        },
        radiusX: radius,
        radiusY: radius,
    }
}
fn title(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Clock => "时钟",
        ComponentKind::Date => "日期",
        ComponentKind::Schedule => "今日课表",
        ComponentKind::Homework => "今日作业",
        ComponentKind::Materials => "课堂资料",
        ComponentKind::Reminder => "今日提醒",
        ComponentKind::DateCountdown => "日期倒计时",
        ComponentKind::Recess => "课间倒计时",
    }
}
fn render_card(g: &Graphics, c: &mut Card) -> Result<()> {
    // SAFETY below: all COM targets/brushes/layouts and selected DIBs belong to this
    // UI thread; local targets drop before DIB release, no UI calls borrow State.
    let target = g.target(&c.surface.dib)?;
    let w = c.surface.rect.width;
    let h = c.surface.rect.height;
    unsafe {
        let glass = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.,
                g: 1.,
                b: 1.,
                a: c.source.surface_alpha,
            },
            None,
        )?;
        let edge = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.,
                g: 1.,
                b: 1.,
                a: c.source.edge_alpha,
            },
            None,
        )?;
        let text = target.CreateSolidColorBrush(&ink(c.tone, c.source.component_alpha), None)?;
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F::default()));
        if c.source.component_alpha > 0. {
            let shape = bounds(1., 1., w - 1., h - 1., 20.);
            target.FillRoundedRectangle(&shape, &glass);
            target.DrawRoundedRectangle(&shape, &edge, 0.75, None);
            let heading = g.layout(
                title(c.source.kind),
                if c.input.is_some() {
                    (w - 112.).max(28.)
                } else {
                    w - 32.
                },
                if c.input.is_some() { 11. } else { 13. },
                false,
            )?;
            target.DrawTextLayout(
                xy(16., if c.input.is_some() { 8. } else { 12. }).point,
                &heading,
                &text,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            if c.source.kind == ComponentKind::Schedule && c.calendar_unverified {
                let subtitle = g.layout("调休资料未核验", w - 32., 11., false)?;
                target.DrawTextLayout(
                    xy(16., 34.).point,
                    &subtitle,
                    &text,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            let offset = c.body.pages[c.page];
            let page_span = c.body.pages.get(c.page + 1).map_or(
                h - body_top(&c.source, c.calendar_unverified) - 12.,
                |next| *next - offset,
            );
            target.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: 12.,
                    top: body_top(&c.source, c.calendar_unverified),
                    right: w - 12.,
                    bottom: body_top(&c.source, c.calendar_unverified) + page_span,
                },
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
            let bottom = offset + page_span;
            for block in &c.body.blocks {
                if block.top + block.height < offset || block.top >= bottom {
                    continue;
                }
                let y = body_top(&c.source, c.calendar_unverified) + block.top - offset;
                if let Some((rgb, alpha, stroke)) = block.fill {
                    let brush = target.CreateSolidColorBrush(
                        &color(
                            rgb,
                            if stroke == 0. {
                                alpha
                            } else {
                                c.source.component_alpha
                            },
                        ),
                        None,
                    )?;
                    let pill = bounds(16., y, w - 16., y + block.height, block.height / 2.);
                    if stroke == 0. {
                        target.FillRoundedRectangle(&pill, &brush);
                    } else {
                        target.DrawRoundedRectangle(&pill, &brush, stroke, None);
                    }
                }
                target.DrawTextLayout(
                    xy(24., y + 8.).point,
                    &block.layout,
                    &text,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            target.PopAxisAlignedClip();
            if let CardContent::Clock(clock) = c.source.content {
                if clock.mode == ClockMode::Analog {
                    draw_dial(&target, c)?;
                }
            }
        }
        target.EndDraw(None, None)?;
    }
    c.surface.dirty = true;
    c.render_pending = false;
    if let Some(input) = &mut c.input {
        render_input(
            g,
            input,
            c.source.component_alpha,
            c.tone,
            c.page,
            c.body.pages.len(),
        )?;
    }
    Ok(())
}
fn draw_dial(target: &ID2D1DCRenderTarget, c: &Card) -> Result<()> {
    // SAFETY: drawing synchronously uses owned COM brushes and value-only points.
    unsafe {
        let Some(d) = &c.dynamic else { return Ok(()) };
        let w = d.rect.width;
        let h = d.rect.height;
        let cx = d.rect.x - c.source.rect.x + w / 2.;
        let cy = d.rect.y - c.source.rect.y + h / 2. - 12.;
        let radius = (w.min(h - 28.) / 2. - 5.).max(8.);
        let brush = target.CreateSolidColorBrush(&ink(c.tone, c.source.component_alpha), None)?;
        let mut e = xy(cx, cy);
        e.radiusX = radius;
        e.radiusY = radius;
        target.DrawEllipse(&e, &brush, 1., None);
        for tick in 0..12 {
            let a = tick as f32 * std::f32::consts::TAU / 12.;
            let (sin, cos) = a.sin_cos();
            target.DrawLine(
                xy(cx + sin * (radius - 5.), cy - cos * (radius - 5.)).point,
                xy(cx + sin * radius, cy - cos * radius).point,
                &brush,
                1.5,
                None,
            );
        }
        Ok(())
    }
}
fn render_dynamic(g: &Graphics, c: &mut Card, now: LocalDateTime) -> Result<()> {
    // SAFETY below: this small UI-owned DIB survives its synchronous COM target.
    let Some(surface) = &mut c.dynamic else {
        return Ok(());
    };
    let target = g.target(&surface.dib)?;
    let w = surface.rect.width;
    let h = surface.rect.height;
    unsafe {
        let brush = target.CreateSolidColorBrush(&ink(c.tone, c.source.component_alpha), None)?;
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F::default()));
        if c.source.component_alpha > 0. {
            match c.source.content {
                CardContent::Clock(clock) => {
                    if clock.mode == ClockMode::Analog {
                        let hands = fc_presentation::scene::clock_hands(clock, now);
                        let cx = w / 2.;
                        let cy = h / 2. - 12.;
                        let radius = (w.min(h - 28.) / 2. - 5.).max(8.);
                        for (turn, length, thickness) in [
                            (Some(hands.hour), 0.53, 3.),
                            (Some(hands.minute), 0.82, 2.),
                            (hands.second, 0.92, 1.),
                        ] {
                            if let Some(turn) = turn {
                                let (sin, cos) = (turn * std::f32::consts::TAU).sin_cos();
                                target.DrawLine(
                                    xy(cx, cy).point,
                                    xy(cx + sin * radius * length, cy - cos * radius * length)
                                        .point,
                                    &brush,
                                    thickness,
                                    None,
                                );
                            }
                        }
                        let label = fc_presentation::scene::clock_label(clock, now);
                        let layout = g.layout(&label, w, 12., true)?;
                        target.DrawTextLayout(
                            xy(0., h - 25.).point,
                            &layout,
                            &brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                        );
                    } else {
                        let text = fc_presentation::scene::clock_label(clock, now);
                        let layout = g.layout(&text, w, c.source.font_dip.min(40.), true)?;
                        let mut metrics = DWRITE_TEXT_METRICS::default();
                        layout.GetMetrics(&mut metrics)?;
                        target.DrawTextLayout(
                            xy(0., ((h - metrics.height) / 2.).max(0.)).point,
                            &layout,
                            &brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                        );
                    }
                }
                CardContent::Recess { deadline } => {
                    let text =
                        fc_presentation::scene::recess_label(deadline, now).unwrap_or_default();
                    let layout = g.layout(&text, w, 64., true)?;
                    target.DrawTextLayout(
                        xy(0., 6.).point,
                        &layout,
                        &brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
                _ => {}
            }
        }
        target.EndDraw(None, None)?;
    }
    surface.dirty = true;
    Ok(())
}
fn input_allowed(c: &InputContext) -> bool {
    if !c.probe && !system_allows().unwrap_or(false) {
        return false;
    }
    let Ok(now) = crate::native::local_now() else {
        return false;
    };
    now.date == c.captured_at.date
        && now >= c.captured_at
        && c.valid_until.is_none_or(|end| now < end)
        && c.gate.status().revision == c.source_revision
        && c.gate
            .try_start(
                c.lease,
                EffectIntent {
                    scope: DESKTOP_SCOPE,
                    job_id: c.generation.get(),
                },
            )
            .and_then(|p| c.gate.check_permit(&p))
            .is_ok()
}

fn input_message(hwnd: HWND, msg: u32, lp: LPARAM, state: &InputState) -> Option<LRESULT> {
    match msg {
        WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
        WM_NCHITTEST => Some(LRESULT(
            if state.context.borrow().as_ref().is_some_and(input_allowed) {
                HTCLIENT as i32
            } else {
                HTTRANSPARENT
            } as isize,
        )),
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            let x = lp.0 as u16 as i16;
            let y = (lp.0 >> 16) as u16 as i16;
            let context = state.context.borrow().clone();
            let Some(context) = context.filter(input_allowed) else {
                state.pressed.set(None);
                return Some(LRESULT(0));
            };
            if !(0..72).contains(&x) || !(0..24).contains(&y) {
                state.pressed.set(None);
                return Some(LRESULT(0));
            }
            let turn = (context.generation, x >= 36);
            if msg == WM_LBUTTONDOWN {
                state.pressed.set(Some(turn));
            } else if state.pressed.replace(None) == Some(turn) {
                let mut queue = state.turns.borrow_mut();
                if queue.len() < 2 {
                    queue.push_back(turn);
                    // SAFETY: bounded asynchronous wakeup to this own HWND. The
                    // shared host dispatches it so a local page turn need not wait
                    // for the next one-second clock tick; no state pointer is sent.
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                    }
                }
            }
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            state.context.replace(None);
            state.turns.borrow_mut().clear();
            state.pressed.set(None);
            // SAFETY: this callback's own HWND, with no outstanding mutable borrow.
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            Some(LRESULT(0))
        }
        _ => None,
    }
}
fn render_input(
    g: &Graphics,
    input: &mut InputStrip,
    alpha: f32,
    tone: TextTone,
    page: usize,
    pages: usize,
) -> Result<()> {
    let target = g.target(&input.surface.dib)?;
    unsafe {
        let brush = target.CreateSolidColorBrush(&ink(tone, alpha), None)?;
        let background = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.3,
                g: 0.3,
                b: 0.3,
                a: 0.16 * alpha,
            },
            None,
        )?;
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F::default()));
        target.FillRoundedRectangle(&bounds(0., 0., 72., 24., 8.), &background);
        let label = g.layout(&format!("‹ {}/{} ›", page + 1, pages), 72., 11., true)?;
        target.DrawTextLayout(
            xy(0., 4.).point,
            &label,
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        target.EndDraw(None, None)?;
    }
    input.surface.dirty = true;
    Ok(())
}

fn body_top(c: &SceneCard, unverified: bool) -> f32 {
    if c.kind == ComponentKind::Schedule && unverified {
        50.
    } else {
        32.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::{
        ids::{AuthEpoch, PrincipalId, Revision, SubjectId},
        time::LocalDate,
    };
    use fc_presentation::scene::{ClockContent, HomeworkEntry, PageInfo};
    use fc_runtime::authority::AuthorityStore;
    use std::time::{Duration, Instant};
    fn authority() -> (fc_runtime::authority::AuthorityPublisher<()>, HealthGate) {
        let (mut publisher, reader) = AuthorityStore::new((), Revision::new(5), AuthEpoch::new(1));
        publisher.initialize().unwrap();
        (publisher, reader.gate())
    }
    fn guard(gate: &HealthGate, generation: SceneGeneration) -> PublicationGuard<'_> {
        PublicationGuard {
            gate,
            lease: gate
                .lease_for_authorized_scope(DESKTOP_SCOPE, Instant::now() + Duration::from_secs(60))
                .unwrap(),
            current_generation: generation,
            settings_open: false,
        }
    }
    fn scene(now: LocalDateTime) -> SceneSnapshot {
        let mut cards = Vec::new();
        for (index, kind) in ComponentKind::ALL.into_iter().enumerate() {
            let content=match kind {
                ComponentKind::Clock=>CardContent::Clock(ClockContent{mode:ClockMode::Digital24,show_seconds:true}),
                ComponentKind::Homework=>CardContent::Homework(vec![HomeworkEntry{subject_id:SubjectId::new(1).unwrap(),teacher_id:PrincipalId::new(1).unwrap(),subject_name:"合成语文".into(),teacher_name:"合成教师".into(),text:"独立离屏验证：阅读并复述课文，练习汉字。\n保留全部长正文，通过只读按钮逐页阅读。".repeat(12),color:Rgb{red:201,green:164,blue:164},fill_alpha:0.03}]),
                ComponentKind::Materials=>CardContent::MaterialsUnavailable,
                ComponentKind::Recess=>CardContent::Recess{deadline:LocalDateTime::new(now.date,(now.second_of_day()+120).min(86399)).unwrap()},
                ComponentKind::Schedule=>{
                    use fc_domain::{ids::{ClassId,SemesterId,TableId,RowId},schedule::{LessonInstanceKey,ResolvedScheduleVersion,CalendarMode},time::{Minute,MinuteRange}};
                    let key=LessonInstanceKey{actual_date:now.date,class_id:ClassId::new(1).unwrap(),range:MinuteRange::new(Minute::new(480).unwrap(),Minute::new(525).unwrap()).unwrap(),version:ResolvedScheduleVersion{actual_semester_id:SemesterId::new(1).unwrap(),actual_revision:Revision::new(1),source_semester_id:SemesterId::new(1).unwrap(),source_revision:Revision::new(1),calendar_mode:CalendarMode::School,calendar_mode_revision:Revision::new(1),applied_calendar_revision:Revision::new(1),makeup_mapping_revision:Revision::new(1),source_table_id:TableId::new(1).unwrap(),source_row_id:RowId::new(1).unwrap(),rules_revision:Revision::new(1)}};
                    CardContent::Courses(["合成语文","合成数学","合成英语"].into_iter().enumerate().map(|(i,name)|fc_presentation::scene::CourseCapsule{key:key.clone(),subject_id:SubjectId::new(i as u64+1).unwrap(),name:name.into(),color:Rgb{red:201,green:164,blue:164},current:i==0,fill_alpha:0.2,stroke_width:0.}).collect())
                },
                ComponentKind::Date=>CardContent::Text("本地日期 · 合成离屏场景".into()),
                ComponentKind::Reminder=>CardContent::Text("请按时整理课堂用品。".into()),
                ComponentKind::DateCountdown=>CardContent::Text("学校活动 · 还有12天".into()),
            };
            cards.push(SceneCard {
                kind,
                rect: Rect {
                    x: 20. + (index % 4) as f32 * 250.,
                    y: 20. + (index / 4) as f32 * 300.,
                    width: 230.,
                    height: 260.,
                },
                font_dip: 18.,
                component_alpha: 1.,
                surface_alpha: 0.03,
                edge_alpha: 0.5,
                content,
                page: PageInfo {
                    offset: 0,
                    shown: 1,
                    total: 2,
                },
            });
        }
        SceneSnapshot {
            generation: SceneGeneration::new(1),
            source_revision: Revision::new(5),
            captured_at: now,
            valid_until: None,
            cards,
            text_tone: TextTone::Light,
            clock_rollback: false,
            public_calendar_unverified: true,
        }
    }
    #[test]
    fn real_com_dib_text_pages_and_three_clocks_remain_offscreen() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_publisher, gate) = authority();
        let now = crate::native::local_now().unwrap();
        let mut scene = scene(now);
        let mut r = DesktopRenderer::new_hidden().unwrap();
        let measure = NativeTextMeasure::new().unwrap();
        assert!(
            measure.height("中文\n换行", 200., 18.).unwrap()
                > measure.height("中文", 200., 18.).unwrap()
        );
        let mut minimal = scene.clone();
        minimal.cards.retain(|c| c.kind == ComponentKind::Date);
        minimal.cards[0].rect.height = 84.;
        minimal.cards[0].content = CardContent::Text("合成日期".into());
        r.present(&minimal, now, &guard(&gate, minimal.generation))
            .unwrap();
        assert_eq!(r.last_metrics().pages[0].pages, 1);
        for (index, mode) in [
            ClockMode::Digital24,
            ClockMode::Digital12,
            ClockMode::Analog,
        ]
        .into_iter()
        .enumerate()
        {
            scene.generation = SceneGeneration::new(index as u64 + 1);
            scene.cards[0].content = CardContent::Clock(ClockContent {
                mode,
                show_seconds: true,
            });
            assert_eq!(
                r.present(&scene, now, &guard(&gate, scene.generation))
                    .unwrap(),
                PresentOutcome::Offscreen
            );
            assert!(r
                .cards
                .iter()
                .all(|c| c.surface.dib.pixels().iter().any(|v| *v != 0)));
        }
        let checksum = |renderer: &DesktopRenderer| {
            renderer
                .cards
                .iter()
                .map(|c| {
                    c.surface.dib.pixels().iter().fold(0u64, |sum, b| {
                        sum.wrapping_mul(31).wrapping_add(u64::from(*b))
                    })
                })
                .collect::<Vec<_>>()
        };
        let before_static = checksum(&r);
        let before_frames = r.last_metrics().drawn_frames;
        if now.second_of_day() < 86398 {
            let tick = LocalDateTime::new(now.date, now.second_of_day() + 1).unwrap();
            r.present(&scene, tick, &guard(&gate, scene.generation))
                .unwrap();
            assert_eq!(r.last_metrics().drawn_frames - before_frames, 2);
            assert_eq!(checksum(&r), before_static);
        }
        let metrics = r.last_metrics();
        assert_eq!(metrics.components, 8);
        assert!(metrics.dib_bytes <= DIB_BUDGET);
        assert_eq!(metrics.uploaded_frames, 0);
        assert_eq!(metrics.visible_windows, 0);
        assert_eq!(metrics.input_regions, 0);
        assert!(
            metrics
                .pages
                .iter()
                .find(|p| p.component == ComponentKind::Homework)
                .unwrap()
                .pages
                > 1
        );
        if let Some(path) = std::env::var_os("FC_DESKTOP_CAPTURE") {
            r.probe_capture_bmp(Path::new(&path)).unwrap();
        }
    }
    #[test]
    fn stale_gate_generation_revision_expired_date_and_alpha_zero_never_draw() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (mut publisher, gate) = authority();
        let now = crate::native::local_now().unwrap();
        let mut scene = scene(now);
        for card in &mut scene.cards {
            card.component_alpha = 0.;
            card.surface_alpha = 0.;
            card.edge_alpha = 0.;
        }
        let mut r = DesktopRenderer::new_hidden().unwrap();
        let g = guard(&gate, scene.generation);
        r.present(&scene, now, &g).unwrap();
        assert_eq!(r.last_metrics().drawn_frames, 0);
        let mut wrong = guard(&gate, SceneGeneration::new(2));
        assert_eq!(
            r.present(&scene, now, &wrong).unwrap(),
            PresentOutcome::Hidden
        );
        wrong.current_generation = scene.generation;
        wrong.lease.expires_at = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            r.present(&scene, now, &wrong).unwrap(),
            PresentOutcome::Hidden
        );
        scene.source_revision = Revision::new(4);
        assert_eq!(r.present(&scene, now, &g).unwrap(), PresentOutcome::Hidden);
        scene.source_revision = Revision::new(5);
        let future = LocalDateTime::new(
            LocalDate::new(now.date.year() + 1, 1, 1).unwrap(),
            now.second_of_day(),
        )
        .unwrap();
        assert_eq!(
            r.present(&scene, future, &g).unwrap(),
            PresentOutcome::Hidden
        );
        scene.valid_until = Some(now);
        assert_eq!(r.present(&scene, now, &g).unwrap(), PresentOutcome::Hidden);
        scene.valid_until = None;
        let mut over_budget = scene.clone();
        over_budget.generation = SceneGeneration::new(3);
        over_budget.cards.truncate(1);
        over_budget.cards[0].rect.width = 4096.;
        over_budget.cards[0].rect.height = 4096.;
        assert!(r
            .present(&over_budget, now, &guard(&gate, over_budget.generation))
            .is_err());
        assert_eq!(r.last_metrics().dib_bytes, 0);
        let mut too_much_text = scene.clone();
        too_much_text.generation = SceneGeneration::new(4);
        too_much_text.cards.truncate(1);
        too_much_text.cards[0].content = CardContent::Text("X".repeat(MAX_TEXT_UNITS + 1));
        assert!(r
            .present(&too_much_text, now, &guard(&gate, too_much_text.generation))
            .is_err());
        assert_eq!(r.last_metrics().dib_bytes, 0);
        let update = publisher.begin_update().unwrap();
        assert_eq!(r.present(&scene, now, &g).unwrap(), PresentOutcome::Hidden);
        drop(update);
        assert_eq!(r.last_metrics().drawn_frames, 0);
        assert_eq!(r.last_metrics().uploaded_frames, 0);
        assert_eq!(r.last_metrics().input_regions, 0);
    }
    #[test]
    fn native_readonly_buttons_turn_local_pages_then_request_bounded_homework_batches() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_publisher, gate) = authority();
        let now = crate::native::local_now().unwrap();
        let mut scene = scene(now);
        let mut r = DesktopRenderer::new_hidden().unwrap();
        let g = guard(&gate, scene.generation);
        r.present(&scene, now, &g).unwrap();
        let initial = r.last_metrics().drawn_frames;
        r.probe_next_page(ComponentKind::Homework, true).unwrap();
        assert!(r.take_events().unwrap().is_empty());
        assert_eq!(r.last_metrics().drawn_frames, initial);
        r.present(&scene, now, &g).unwrap();
        assert_eq!(
            r.last_metrics()
                .pages
                .iter()
                .find(|p| p.component == ComponentKind::Homework)
                .unwrap()
                .page,
            1
        );
        // A queued click belongs to its source scene; replacing that scene
        // clears it before the new generation can consume a stale navigation.
        r.probe_next_page(ComponentKind::Homework, true).unwrap();
        scene.generation = SceneGeneration::new(2);
        r.present(&scene, now, &guard(&gate, scene.generation))
            .unwrap();
        assert_eq!(
            r.last_metrics()
                .pages
                .iter()
                .find(|p| p.component == ComponentKind::Homework)
                .unwrap()
                .page,
            1
        );
        assert!(r.take_events().unwrap().is_empty());
        let pages = r
            .last_metrics()
            .pages
            .iter()
            .find(|p| p.component == ComponentKind::Homework)
            .unwrap()
            .pages;
        for _ in 1..pages - 1 {
            r.probe_next_page(ComponentKind::Homework, true).unwrap();
            assert!(r.take_events().unwrap().is_empty());
        }
        r.probe_next_page(ComponentKind::Homework, true).unwrap();
        assert_eq!(
            r.take_events().unwrap(),
            vec![DesktopEvent::HomeworkBatch {
                generation: scene.generation,
                offset: 1
            }]
        );
        r.probe_next_page(ComponentKind::Homework, true).unwrap();
        r.hide();
        assert!(r.take_events().unwrap().is_empty());
        assert_eq!(r.last_metrics().input_regions, 0);
    }
    #[test]
    fn one_hundred_real_scene_rebuilds_release_handles_and_denied_ticks_do_zero_work() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_publisher, gate) = authority();
        let now = crate::native::local_now().unwrap();
        let mut scene = scene(now);
        let mut r = DesktopRenderer::new_hidden().unwrap();
        r.present(&scene, now, &guard(&gate, scene.generation))
            .unwrap();
        let count = || unsafe {
            (
                GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS),
                GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS),
            )
        };
        let before = count();
        for index in 2..102 {
            scene.generation = SceneGeneration::new(index);
            r.present(&scene, now, &guard(&gate, scene.generation))
                .unwrap();
        }
        let after = count();
        assert!(after.0 <= before.0 + 2, "GDI {before:?}->{after:?}");
        assert!(after.1 <= before.1 + 2, "USER {before:?}->{after:?}");
        let initial = r.last_metrics();
        let mut denied = guard(&gate, scene.generation);
        denied.settings_open = true;
        for _ in 0..100 {
            assert_eq!(
                r.present(&scene, now, &denied).unwrap(),
                PresentOutcome::Hidden
            );
        }
        assert_eq!(r.last_metrics().drawn_frames, initial.drawn_frames);
        assert_eq!(r.last_metrics().uploaded_frames, 0);
        assert_eq!(r.last_metrics().input_regions, 0);
    }
}
