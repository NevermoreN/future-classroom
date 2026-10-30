//! The resident desktop's UI-thread renderer. No private pump or worker thread.
//! Native publication always rechecks a real system sample and an exact lease.
use crate::native::{Apartment, Dib};
use fc_domain::{
    desktop::{ClockMode, ComponentKind, TextTone},
    identity::Rgb,
    ids::{FolderId, MaterialId, ResourceVersionId, Revision, SceneGeneration},
    materials::{MaterialCursor, MaterialSource},
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
        UI::{
            Controls::WM_MOUSELEAVE,
            Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT},
            WindowsAndMessaging::*,
        },
    },
};

#[path = "desktop/backdrop.rs"]
mod backdrop;
use backdrop::Backdrop;

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
    /// Native constant-alpha GDI windows have no application DIB. Windows-managed
    /// compositor/backing memory remains a separate real-machine measurement.
    pub gdi_backdrop_layers: usize,
    pub backdrop_covered_pixels: u64,
    pub backdrop_updates: u64,
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
#[allow(clippy::large_enum_variant)] // At most eight owned metadata events, no full scene or file bytes.
pub enum DesktopEvent {
    LessonManage {
        generation: SceneGeneration,
    },
    HomeworkBatch {
        generation: SceneGeneration,
        offset: u64,
    },
    MaterialOpen {
        generation: SceneGeneration,
        source: MaterialSource,
        folder_id: FolderId,
        folder_revision: Revision,
        material_id: MaterialId,
        resource_version: ResourceVersionId,
    },
    MaterialPage {
        generation: SceneGeneration,
        source: MaterialSource,
        cursor: MaterialCursor,
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
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)] // At most two queued actions per input component.
enum InputAction {
    LessonManage,
    Page(bool),
    MaterialOpen {
        source: MaterialSource,
        folder_id: FolderId,
        folder_revision: Revision,
        material_id: MaterialId,
        resource_version: ResourceVersionId,
    },
    MaterialPage {
        source: MaterialSource,
        cursor: MaterialCursor,
    },
}
#[derive(Clone, Debug, PartialEq)]
struct HitTarget {
    rect: Rect,
    action: InputAction,
}
struct InputState {
    context: RefCell<Option<InputContext>>,
    targets: RefCell<Vec<HitTarget>>,
    layout_epoch: Cell<u64>,
    pressed: Cell<Option<(SceneGeneration, u64, usize)>>,
    turns: RefCell<VecDeque<(SceneGeneration, InputAction)>>,
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
            if let Err(error) = backdrop::register(instance) {
                let _ = UnregisterClassW(w!("FutureClassroom.Desktop.v1"), Some(instance));
                return Err(error);
            }
            Ok(Self { instance })
        }
    }
}
impl Drop for Class {
    fn drop(&mut self) {
        // SAFETY: every owned HWND is destroyed before its registration owner.
        unsafe {
            backdrop::unregister(self.instance);
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
    action: Option<InputAction>,
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
    backdrop: Option<Backdrop>,
    // Recess's static large card and its small clock overlay are separate, so a
    // one-second update never repaints or uploads a large work-area-sized card.
    dynamic: Option<Surface>,
    clock_tick: Option<u32>,
}
impl Card {
    fn hide(&self) {
        if let Some(backdrop) = &self.backdrop {
            backdrop.hide();
        }
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
            targets: RefCell::new(Vec::new()),
            layout_epoch: Cell::new(0),
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
        let surface = make_surface(window, rect)?;
        Ok(InputStrip { surface, state })
    }
    pub fn take_events(&mut self) -> Result<Vec<DesktopEvent>> {
        let mut output = Vec::with_capacity(2);
        for card in &mut self.cards {
            let Some(input) = &card.input else { continue };
            let turns: Vec<_> = input.state.turns.borrow_mut().drain(..).collect();
            for (generation, action) in turns {
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
                let forward = match action {
                    InputAction::LessonManage => {
                        if output.len() < 8 {
                            output.push(DesktopEvent::LessonManage { generation });
                        }
                        continue;
                    }
                    InputAction::Page(forward) => forward,
                    InputAction::MaterialOpen {
                        source,
                        folder_id,
                        folder_revision,
                        material_id,
                        resource_version,
                    } => {
                        if output.len() < 8 {
                            output.push(DesktopEvent::MaterialOpen {
                                generation,
                                source,
                                folder_id,
                                folder_revision,
                                material_id,
                                resource_version,
                            });
                        }
                        continue;
                    }
                    InputAction::MaterialPage { source, cursor } => {
                        if output.len() < 8 {
                            output.push(DesktopEvent::MaterialPage {
                                generation,
                                source,
                                cursor,
                            });
                        }
                        continue;
                    }
                };
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
    pub fn probe_lesson_control(&self) -> Result<()> {
        self.probe_input(ComponentKind::Schedule, |a| *a == InputAction::LessonManage)
    }
    pub fn probe_next_page(&self, kind: ComponentKind, forward: bool) -> Result<()> {
        self.probe_input(kind, |action| *action == InputAction::Page(forward))
    }
    pub fn probe_material_open(&self, id: MaterialId) -> Result<()> {
        self.probe_input(
            ComponentKind::Materials,
            |action| matches!(action,InputAction::MaterialOpen{material_id,..} if *material_id==id),
        )
    }
    pub fn probe_material_more(&self) -> Result<()> {
        self.probe_input(ComponentKind::Materials, |action| {
            matches!(action, InputAction::MaterialPage { .. })
        })
    }
    fn probe_input(
        &self,
        kind: ComponentKind,
        select: impl Fn(&InputAction) -> bool,
    ) -> Result<()> {
        if !self.hidden_probe {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let input = self
            .cards
            .iter()
            .find(|c| {
                c.source.kind == kind
                    && c.input
                        .as_ref()
                        .is_some_and(|i| i.state.targets.borrow().iter().any(|t| select(&t.action)))
            })
            .and_then(|c| c.input.as_ref())
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        let rect = input
            .state
            .targets
            .borrow()
            .iter()
            .find(|target| select(&target.action))
            .map(|target| target.rect)
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        let x = (rect.x + rect.width / 2.).round() as i32;
        let y = (rect.y + rect.height / 2.).round() as i32;
        let position = LPARAM(((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize);
        // SAFETY: these notifications target only a currently rendered own hidden
        // hit rectangle. No global input, cursor movement or activation occurs.
        unsafe {
            let _ = SendMessageW(
                input.surface.window.0,
                WM_LBUTTONDOWN,
                Some(WPARAM(0)),
                Some(position),
            );
            let _ = SendMessageW(
                input.surface.window.0,
                WM_LBUTTONUP,
                Some(WPARAM(0)),
                Some(position),
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
        let control_card = scene.lesson_control.as_ref().map(|control| SceneCard {
            kind: ComponentKind::Schedule,
            rect: control.rect,
            font_dip: 16.,
            component_alpha: 1.,
            surface_alpha: 0.03,
            edge_alpha: 0.5,
            content: CardContent::LessonControl {
                caption: control.caption.clone(),
                can_change: control.can_change,
            },
            page: Default::default(),
        });
        let scene_cards: Vec<_> = scene.cards.iter().chain(control_card.iter()).collect();
        let changed = self.last_scene != Some(scene.generation)
            || self.cards.len() != scene_cards.len()
            || self.cards.iter().zip(&scene_cards).any(|(a, b)| {
                a.source != **b
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
            for card in scene_cards {
                let control = matches!(card.content, CardContent::LessonControl { .. });
                if kinds.contains(&card.kind) && !control {
                    return Err(Error::from_hresult(E_INVALIDARG));
                }
                if !control {
                    kinds.push(card.kind);
                }
                validate_card(card)?;
                let body = prepare_content(&self.graphics, card, scene.public_calendar_unverified)?;
                let dynamic_rect = match card.content {
                    CardContent::Recess { .. } => {
                        let width = (card.rect.width - 32.).min(560.);
                        Some(Rect {
                            x: card.rect.x + (card.rect.width - width) / 2.,
                            y: card.rect.y + (card.rect.height - 100.) / 2.,
                            width,
                            height: 100.,
                        })
                    }
                    CardContent::Clock(_) => Some(Rect {
                        x: card.rect.x + 16.,
                        y: card.rect.y + 42.,
                        width: card.rect.width - 32.,
                        height: card.rect.height - 54.,
                    }),
                    _ => None,
                };
                let input_rect = if matches!(
                    card.content,
                    CardContent::LessonControl {
                        can_change: true,
                        ..
                    }
                ) {
                    Some(Rect {
                        x: card.rect.x + 8.,
                        y: card.rect.y + 8.,
                        width: 144.,
                        height: 40.,
                    })
                } else if matches!(&card.content,CardContent::Materials(list) if !list.rows.is_empty() || list.next_cursor.is_some() || body.pages.len()>1)
                {
                    Some(card.rect)
                } else if matches!(card.content, CardContent::Materials(_)) {
                    None
                } else {
                    (body.pages.len() > 1
                        || card.kind == ComponentKind::Homework
                            && (card.page.offset > 0
                                || card.page.offset.saturating_add(card.page.shown)
                                    < card.page.total))
                        .then_some(Rect {
                            x: card.rect.x + card.rect.width - 86.,
                            y: card.rect.y + 8.,
                            width: 72.,
                            height: 24.,
                        })
                };
                let static_rect = if matches!(card.content, CardContent::Recess { .. }) {
                    Rect {
                        x: card.rect.x,
                        y: card.rect.y,
                        width: card.rect.width.min(560.),
                        height: 48.,
                    }
                } else {
                    card.rect
                };
                for r in std::iter::once(static_rect)
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
                let backdrop = if card.component_alpha > 0.
                    && matches!(card.content, CardContent::Recess { .. })
                {
                    Some(Backdrop::new(
                        &self.class,
                        card.rect,
                        card.surface_alpha,
                        card.edge_alpha,
                    )?)
                } else {
                    None
                };
                let surface = self.surface(static_rect)?;
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
                    backdrop,
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
            if need && self.cards[index].backdrop.is_some() {
                for side in 0..2 {
                    if !publication_allowed(scene, guard)? {
                        self.hide();
                        return Ok(PresentOutcome::Hidden);
                    }
                    let backdrop = self.cards[index].backdrop.as_ref().unwrap();
                    let layer = if side == 0 {
                        &backdrop.fill
                    } else {
                        &backdrop.edge
                    };
                    layer.apply()?;
                    self.metrics.backdrop_updates += 1;
                }
            }
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
                if self.cards[index].backdrop.is_some() {
                    for side in 0..2 {
                        if !publication_allowed(scene, guard)? {
                            self.hide();
                            return Ok(PresentOutcome::Hidden);
                        }
                        // SAFETY: own constant-alpha background, freshly checked,
                        // no activation or topmost placement is requested.
                        unsafe {
                            let backdrop = self.cards[index].backdrop.as_ref().unwrap();
                            let layer = if side == 0 {
                                &backdrop.fill
                            } else {
                                &backdrop.edge
                            };
                            let _ = ShowWindow(layer.window.0, SW_SHOWNOACTIVATE);
                        }
                    }
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
        make_surface(NativeWindow::new(&self.class)?, rect)
    }
    fn refresh_metrics(&mut self) {
        self.metrics.components = self.cards.len();
        self.metrics.gdi_backdrop_layers =
            self.cards.iter().filter(|c| c.backdrop.is_some()).count() * 2;
        self.metrics.backdrop_covered_pixels = self
            .cards
            .iter()
            .filter(|c| c.backdrop.is_some())
            .map(|c| c.source.rect.width.ceil() as u64 * c.source.rect.height.ceil() as u64)
            .sum();
        self.metrics.surfaces = self
            .cards
            .iter()
            .map(|c| {
                1 + usize::from(c.dynamic.is_some())
                    + usize::from(c.input.is_some())
                    + usize::from(c.backdrop.is_some()) * 2
            })
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
                .map(|c| {
                    1 + usize::from(c.dynamic.is_some())
                        + usize::from(c.input.is_some())
                        + usize::from(c.backdrop.is_some()) * 2
                })
                .sum()
        };
        self.metrics.input_regions = if self.hidden {
            0
        } else {
            self.cards
                .iter()
                .filter(|c| c.source.component_alpha > 0.)
                .filter_map(|c| c.input.as_ref())
                .map(|input| input.state.targets.borrow().len())
                .sum()
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
        self.probe_capture_bmp_for_area(path, self.work_area()?)
    }
    pub fn probe_capture_bmp_for_area(&self, path: &Path, work: Rect) -> Result<()> {
        if !self.hidden_probe || !path.is_absolute() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        surface_bytes(work)?;
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
            if let Some(backdrop) = &c.backdrop {
                backdrop.fill.composite(&mut pixels, work, w, h)?;
                backdrop.edge.composite(&mut pixels, work, w, h)?;
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

fn make_surface(window: NativeWindow, rect: Rect) -> Result<Surface> {
    let dib = Dib::new(rect.width.ceil() as i32, rect.height.ceil() as i32)?;
    // SAFETY: own still-hidden HWND is positioned without activation or z-order
    // change, so hidden probes exercise the actual client geometry as well as DIBs.
    unsafe {
        SetWindowPos(
            window.0,
            None,
            rect.x.round() as i32,
            rect.y.round() as i32,
            dib.width,
            dib.height,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
        )?;
    }
    Ok(Surface {
        window,
        dib,
        rect,
        dirty: true,
    })
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
    if matches!(c.content, CardContent::LessonControl { .. }) {
        return Ok(ContentLayout {
            blocks: Vec::new(),
            pages: vec![0.],
            units: 0,
        });
    }
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
            action: None,
        });
        top += height + 8.;
        Ok(())
    };
    match &c.content {
        CardContent::LessonControl { .. } => unreachable!("control handled above"),
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
                        format!(
                            "{}\n{}",
                            r.name,
                            if r.temporary {
                                "临时 · 仅本节有效"
                            } else {
                                "正在上课"
                            }
                        )
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
        CardContent::Materials(list) => {
            if list.rows.len() > fc_domain::materials::DESKTOP_MATERIAL_PAGE
                || !list.rows.is_empty() && list.folder_id.is_none()
            {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            add(
                format!("{} · {}", list.subject_name, list.teacher_name),
                false,
                None,
            )?;
            if list.rows.is_empty() {
                add("该老师暂无课堂资料".into(), false, None)?;
            }
            for row in &list.rows {
                add(row.name.clone(), false, None)?;
            }
            if list.next_cursor.is_some() {
                add("更多资料 ›".into(), false, None)?;
            }
        }
        CardContent::MaterialsUnavailable => add("资料尚未就绪".into(), false, None)?,
        _ => {}
    }
    if let CardContent::Materials(list) = &c.content {
        for (block, row) in blocks.iter_mut().skip(1).zip(&list.rows) {
            block.action = Some(InputAction::MaterialOpen {
                source: list.source.clone(),
                folder_id: list
                    .folder_id
                    .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?,
                folder_revision: list.folder_revision,
                material_id: row.material_id,
                resource_version: row.resource_version,
            });
        }
        if let Some(cursor) = &list.next_cursor {
            blocks
                .last_mut()
                .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?
                .action = Some(InputAction::MaterialPage {
                source: list.source.clone(),
                cursor: cursor.clone(),
            });
        }
        let mut y = 0.;
        for block in &mut blocks {
            let delta = y - block.top;
            for (line, _) in &mut block.lines {
                *line += delta;
            }
            block.top = y;
            if block.action.is_some() {
                block.height = block.height.max(44.);
            }
            y += block.height + 8.;
        }
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
            if b.fill.is_none() && b.action.is_none() || b.height > viewport {
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
    if let CardContent::LessonControl {
        caption,
        can_change,
    } = &c.source.content
    {
        return render_lesson_control(g, c, caption.clone(), *can_change);
    }
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
            if c.backdrop.is_none() {
                target.FillRoundedRectangle(&shape, &glass);
                target.DrawRoundedRectangle(&shape, &edge, 0.75, None);
            }
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
    render_inputs(g, c)?;
    Ok(())
}
fn render_lesson_control(
    g: &Graphics,
    c: &mut Card,
    caption: String,
    can_change: bool,
) -> Result<()> {
    let target = g.target(&c.surface.dib)?;
    let left = if can_change { 164. } else { 16. };
    let layout = g.layout(
        &caption,
        c.source.rect.width - left - 16.,
        c.source.font_dip,
        false,
    )?;
    unsafe {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics)?;
        if metrics.height > c.source.rect.height - 24. + 0.5 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let text = target.CreateSolidColorBrush(&ink(c.tone, 1.), None)?;
        let glass = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.3,
                g: 0.3,
                b: 0.3,
                a: 0.16,
            },
            None,
        )?;
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F::default()));
        target.FillRoundedRectangle(
            &bounds(
                1.,
                1.,
                c.surface.rect.width - 1.,
                c.surface.rect.height - 1.,
                12.,
            ),
            &glass,
        );
        target.DrawTextLayout(
            xy(left, 12.).point,
            &layout,
            &text,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        target.EndDraw(None, None)?;
    }
    c.surface.dirty = true;
    c.render_pending = false;
    render_inputs(g, c)
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
    let local = |screen: bool| {
        let mut p = POINT {
            x: lp.0 as u16 as i16 as i32,
            y: (lp.0 >> 16) as u16 as i16 as i32,
        };
        if screen {
            unsafe {
                let _ = ScreenToClient(hwnd, &mut p);
            }
        }
        p
    };
    match msg {
        WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
        WM_NCHITTEST => {
            let point = local(true);
            let allowed = state.context.borrow().as_ref().is_some_and(input_allowed)
                && state
                    .targets
                    .borrow()
                    .iter()
                    .any(|t| t.rect.contains(point.x as f32, point.y as f32));
            Some(LRESULT(if allowed {
                HTCLIENT as isize
            } else {
                HTTRANSPARENT as isize
            }))
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            let point = local(false);
            let context = state.context.borrow().clone();
            let Some(context) = context.filter(input_allowed) else {
                state.pressed.set(None);
                return Some(LRESULT(0));
            };
            let target = state
                .targets
                .borrow()
                .iter()
                .enumerate()
                .find(|(_, t)| t.rect.contains(point.x as f32, point.y as f32))
                .map(|(i, t)| (i, t.action.clone()));
            let Some((index, action)) = target else {
                state.pressed.set(None);
                return Some(LRESULT(0));
            };
            let press = (context.generation, state.layout_epoch.get(), index);
            if msg == WM_LBUTTONDOWN {
                state.pressed.set(Some(press));
                // SAFETY: subscribe only this own visible button/row window to a
                // leave notification; no cursor, focus or mouse capture is changed.
                unsafe {
                    if IsWindowVisible(hwnd).as_bool() {
                        let mut tracking = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut tracking);
                    }
                }
            } else if state.pressed.replace(None) == Some(press) {
                let mut events = state.turns.borrow_mut();
                if events.len() < 2 {
                    events.push_back((context.generation, action));
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                    }
                }
            }
            Some(LRESULT(0))
        }
        WM_CANCELMODE | WM_MOUSELEAVE => {
            state.pressed.set(None);
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            state.context.replace(None);
            state.turns.borrow_mut().clear();
            state.pressed.set(None);
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            Some(LRESULT(0))
        }
        _ => None,
    }
}
fn render_inputs(g: &Graphics, c: &mut Card) -> Result<()> {
    let Some(input) = &mut c.input else {
        return Ok(());
    };
    let material = matches!(c.source.content, CardContent::Materials(_));
    let mut targets = Vec::new();
    let control = matches!(c.source.content, CardContent::LessonControl { .. });
    let pagination = !control && (!material || c.body.pages.len() > 1);
    let origin = if material {
        (c.source.rect.width - 86., 8.)
    } else {
        (0., 0.)
    };
    if control {
        targets.push(HitTarget {
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 144.,
                height: 40.,
            },
            action: InputAction::LessonManage,
        });
    }
    if pagination {
        for (index, forward) in [false, true].into_iter().enumerate() {
            targets.push(HitTarget {
                rect: Rect {
                    x: origin.0 + index as f32 * 36.,
                    y: origin.1,
                    width: 36.,
                    height: 24.,
                },
                action: InputAction::Page(forward),
            });
        }
    }
    if material {
        let top = body_top(&c.source, c.calendar_unverified);
        let offset = c.body.pages[c.page];
        let span = c
            .body
            .pages
            .get(c.page + 1)
            .map_or(c.source.rect.height - top - 12., |next| *next - offset);
        for block in &c.body.blocks {
            let Some(action) = &block.action else {
                continue;
            };
            let first = (top + block.top - offset).max(top);
            let last = (top + block.top + block.height - offset).min(top + span);
            let visible_text = block
                .lines
                .iter()
                .any(|(y, height)| *y >= offset - 0.1 && *y + *height <= offset + span + 0.1);
            if last > first && visible_text {
                targets.push(HitTarget {
                    rect: Rect {
                        x: 16.,
                        y: first,
                        width: c.source.rect.width - 32.,
                        height: last - first,
                    },
                    action: action.clone(),
                });
            }
        }
    }
    if *input.state.targets.borrow() != targets {
        input.state.pressed.set(None);
        input
            .state
            .layout_epoch
            .set(input.state.layout_epoch.get().saturating_add(1));
        input.state.targets.replace(targets.clone());
    }
    let target = g.target(&input.surface.dib)?;
    // SAFETY: owned small card input DIB and local layouts remain on this UI
    // thread. Only visible row/button rectangles receive a nonzero alpha mask.
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F::default()));
        let hit = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.,
                g: 1.,
                b: 1.,
                a: 1. / 255.,
            },
            None,
        )?;
        if material {
            for item in &targets {
                if !matches!(item.action, InputAction::Page(_)) {
                    let r = item.rect;
                    target.FillRectangle(
                        &D2D_RECT_F {
                            left: r.x,
                            top: r.y,
                            right: r.x + r.width,
                            bottom: r.y + r.height,
                        },
                        &hit,
                    );
                }
            }
        }
        if control {
            let brush = target.CreateSolidColorBrush(&ink(c.tone, 1.), None)?;
            let background = target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.3,
                    g: 0.3,
                    b: 0.3,
                    a: 0.35,
                },
                None,
            )?;
            target.FillRoundedRectangle(&bounds(0., 0., 144., 40., 10.), &background);
            let label = g.layout("临时换课 ▾", 144., 16., true)?;
            target.DrawTextLayout(
                xy(0., 9.).point,
                &label,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        if pagination {
            let brush =
                target.CreateSolidColorBrush(&ink(c.tone, c.source.component_alpha), None)?;
            let background = target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.3,
                    g: 0.3,
                    b: 0.3,
                    a: 0.16 * c.source.component_alpha,
                },
                None,
            )?;
            target.FillRoundedRectangle(
                &bounds(origin.0, origin.1, origin.0 + 72., origin.1 + 24., 8.),
                &background,
            );
            let label = g.layout(
                &format!("‹ {}/{} ›", c.page + 1, c.body.pages.len()),
                72.,
                11.,
                true,
            )?;
            target.DrawTextLayout(
                xy(origin.0, origin.1 + 4.).point,
                &label,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
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
                    CardContent::Courses(["合成语文","合成数学","合成英语"].into_iter().enumerate().map(|(i,name)|fc_presentation::scene::CourseCapsule{key:key.clone(),subject_id:SubjectId::new(i as u64+1).unwrap(),name:name.into(),color:Rgb{red:201,green:164,blue:164},current:i==0,temporary:false,fill_alpha:0.2,stroke_width:0.}).collect())
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
            lesson_control: None,
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
        assert_eq!(r.last_metrics().gdi_backdrop_layers, 0);
        assert!(r.cards.iter().all(|card| card.backdrop.is_none()));
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
    fn material_scene(now: LocalDateTime) -> SceneSnapshot {
        use fc_domain::materials::{MaterialScope, MaterialSourceEvidence, MaterialSourceKind};
        let mut scene = scene(now);
        let lesson = match &scene
            .cards
            .iter()
            .find(|c| c.kind == ComponentKind::Schedule)
            .unwrap()
            .content
        {
            CardContent::Courses(rows) => rows[0].key.clone(),
            _ => unreachable!(),
        };
        let source = MaterialSource {
            kind: MaterialSourceKind::CurrentLesson,
            lesson,
            subject_id: SubjectId::new(1).unwrap(),
            teacher_id: PrincipalId::new(1).unwrap(),
            evidence: MaterialSourceEvidence::BaseTimetableOnly,
        };
        let folder_id = FolderId::new(7).unwrap();
        let folder_revision = Revision::new(9);
        let cursor = MaterialCursor {
            folder_id,
            folder_revision,
            scope: MaterialScope {
                teacher_id: source.teacher_id,
                subject_id: source.subject_id,
            },
            class_id: Some(source.lesson.class_id),
            source: Some(source.clone()),
            offset: 5,
        };
        let content = fc_presentation::scene::MaterialListContent {
            source,
            folder_id: Some(folder_id),
            folder_revision,
            teacher_name: "合成教师".into(),
            subject_name: "合成语文".into(),
            rows: (1..=5)
                .map(|id| fc_presentation::scene::SceneMaterialRow {
                    material_id: MaterialId::new(id).unwrap(),
                    resource_version: ResourceVersionId::new(id + 100).unwrap(),
                    name: format!("合成课堂资料{id:02}.pptx"),
                })
                .collect(),
            next_cursor: Some(cursor),
            total: 8,
        };
        let card = scene
            .cards
            .iter_mut()
            .find(|c| c.kind == ComponentKind::Materials)
            .unwrap();
        card.rect = Rect {
            x: 20.,
            y: 20.,
            width: 460.,
            height: 520.,
        };
        card.content = CardContent::Materials(Box::new(content));
        card.page = PageInfo {
            offset: 0,
            shown: 5,
            total: 8,
        };
        scene.cards.retain(|c| c.kind == ComponentKind::Materials);
        scene
    }
    #[test]
    fn full_1080p_and_4k_recess_regions_preserve_alpha_with_bounded_dibs() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        fn memory() -> (usize, usize) {
            use windows::Win32::System::ProcessStatus::{
                GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
            };
            let mut counters = PROCESS_MEMORY_COUNTERS_EX {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                ..Default::default()
            };
            // SAFETY: own process pseudo-handle is borrowed, not closed; the EX
            // structure extends the documented prefix and cb covers that buffer.
            unsafe {
                GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb)
                    .unwrap();
            }
            (counters.WorkingSetSize, counters.PrivateUsage)
        }
        let (_publisher, gate) = authority();
        let now = crate::native::local_now().unwrap();
        let mut r = DesktopRenderer::new_hidden().unwrap();
        for (generation, width, height) in [(1, 1920., 1080.), (2, 3840., 2160.)] {
            let mut scene = scene(now);
            scene.generation = SceneGeneration::new(generation);
            let x = width - 816.;
            let mut left_y = 16.;
            let mut right_y = 16.;
            for card in &mut scene.cards {
                card.rect = match card.kind {
                    ComponentKind::Recess => Rect {
                        x: 16.,
                        y: 16.,
                        width: width - 844.,
                        height: height - 32.,
                    },
                    ComponentKind::Clock
                    | ComponentKind::Date
                    | ComponentKind::Schedule
                    | ComponentKind::DateCountdown => {
                        let h = match card.kind {
                            ComponentKind::Clock => 160.,
                            ComponentKind::Schedule => (height * 0.35).min(400.),
                            _ => 100.,
                        };
                        let rect = Rect {
                            x,
                            y: left_y,
                            width: 280.,
                            height: h,
                        };
                        left_y += h + 12.;
                        rect
                    }
                    _ => {
                        let h = height
                            * match card.kind {
                                ComponentKind::Homework => 0.4,
                                ComponentKind::Materials => 0.25,
                                _ => 0.15,
                            };
                        let rect = Rect {
                            x: x + 292.,
                            y: right_y,
                            width: 508.,
                            height: h,
                        };
                        right_y += h + 12.;
                        rect
                    }
                };
            }
            let before_memory = memory();
            assert_eq!(
                r.present(&scene, now, &guard(&gate, scene.generation))
                    .unwrap(),
                PresentOutcome::Offscreen
            );
            let recess = r
                .cards
                .iter()
                .find(|c| c.source.kind == ComponentKind::Recess)
                .unwrap();
            let background = recess.backdrop.as_ref().unwrap();
            background.fill.verify_native_paint().unwrap();
            background.edge.verify_native_paint().unwrap();
            let count = || unsafe {
                (
                    GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS),
                    GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS),
                )
            };
            let resources = count();
            // Exercise forced native paint restoration at each real 1080p/4K
            // size without changing immutable scene geometry or showing windows.
            for iteration in 0..100 {
                background
                    .fill
                    .apply()
                    .unwrap_or_else(|e| panic!("fill iteration {iteration}: {e:?}"));
                background
                    .edge
                    .apply()
                    .unwrap_or_else(|e| panic!("edge iteration {iteration}: {e:?}"));
            }
            background.fill.verify_native_paint().unwrap();
            background.edge.verify_native_paint().unwrap();
            let after = count();
            let after_memory = memory();
            eprintln!("FC_ROUND6_MEMORY width={} height={} before_working_set={} before_private_bytes={} after_working_set={} after_private_bytes={} context=hidden_process_before_synthetic_capture_not_DWM", width, height, before_memory.0, before_memory.1, after_memory.0, after_memory.1);
            assert_eq!(after, resources);

            let metrics = r.last_metrics();
            assert!(metrics.dib_bytes < DIB_BUDGET);
            assert_eq!(metrics.gdi_backdrop_layers, 2);
            eprintln!("FC_ROUND6_METRICS width={} height={} dib_bytes={} backdrop_pixels={} gdi_before={:?} gdi_after={:?}",width,height,metrics.dib_bytes,metrics.backdrop_covered_pixels,resources,after);
            assert_eq!(
                metrics.backdrop_covered_pixels,
                ((width - 844.) * (height - 32.)) as u64
            );
            assert_eq!(metrics.uploaded_frames, 0);
            assert_eq!(metrics.visible_windows, 0);
            // Static-region fill covers the complete interior at independently
            // quantized 3%; a different 50% hollow region provides only the edge.
            let area = Rect {
                x: 0.,
                y: 0.,
                width,
                height,
            };
            let mut sample = vec![0u8; (width * height * 4.) as usize];
            for pixel in sample.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[45, 36, 28, 255]);
            }
            background
                .fill
                .composite(&mut sample, area, width as usize, height as usize)
                .unwrap();
            let center = ((height as usize / 2) * width as usize + width as usize / 4) * 4;
            assert_eq!(&sample[center..center + 3], &[51, 42, 35]);
            let before_center = sample[center..center + 3].to_vec();
            background
                .edge
                .composite(&mut sample, area, width as usize, height as usize)
                .unwrap();
            assert_eq!(&sample[center..center + 3], before_center.as_slice());
            if let Some(directory) = std::env::var_os("FC_DESKTOP_CAPTURE_DIR") {
                let path =
                    Path::new(&directory).join(format!("resident-{width:.0}x{height:.0}.bmp"));
                r.probe_capture_bmp_for_area(&path, area).unwrap();
            }
            let checksum = |renderer: &DesktopRenderer| {
                renderer
                    .cards
                    .iter()
                    .map(|c| {
                        c.surface
                            .dib
                            .pixels()
                            .iter()
                            .fold(0u64, |a, v| a.wrapping_mul(31).wrapping_add(u64::from(*v)))
                    })
                    .collect::<Vec<_>>()
            };
            let previous = checksum(&r);
            let draws = r.last_metrics().drawn_frames;
            if now.second_of_day() < 86398 {
                r.present(
                    &scene,
                    LocalDateTime::new(now.date, now.second_of_day() + 1).unwrap(),
                    &guard(&gate, scene.generation),
                )
                .unwrap();
                assert_eq!(r.last_metrics().drawn_frames - draws, 2);
                assert_eq!(checksum(&r), previous);
                assert_eq!(r.last_metrics().backdrop_updates, 0);
            }
        }
    }
    #[test]
    fn filename_hit_rectangles_bind_source_and_resource_versions_and_reject_stale_actions() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (mut publisher, gate) = authority();
        let now = crate::native::local_now().unwrap();
        let mut scene = material_scene(now);
        let mut r = DesktopRenderer::new_hidden().unwrap();
        r.present(&scene, now, &guard(&gate, scene.generation))
            .unwrap();
        if let Some(directory) = std::env::var_os("FC_DESKTOP_CAPTURE_DIR") {
            r.probe_capture_bmp_for_area(
                &Path::new(&directory).join("materials-own-hit-regions.bmp"),
                Rect {
                    x: 0.,
                    y: 0.,
                    width: 1024.,
                    height: 768.,
                },
            )
            .unwrap();
        }
        let expected = match &scene.cards[0].content {
            CardContent::Materials(list) => list.as_ref().clone(),
            _ => unreachable!(),
        };
        r.probe_material_open(MaterialId::new(3).unwrap()).unwrap();
        assert_eq!(
            r.take_events().unwrap(),
            vec![DesktopEvent::MaterialOpen {
                generation: scene.generation,
                source: expected.source.clone(),
                folder_id: expected.folder_id.unwrap(),
                folder_revision: expected.folder_revision,
                material_id: MaterialId::new(3).unwrap(),
                resource_version: ResourceVersionId::new(103).unwrap()
            }]
        );
        r.probe_material_more().unwrap();
        assert_eq!(
            r.take_events().unwrap(),
            vec![DesktopEvent::MaterialPage {
                generation: scene.generation,
                source: expected.source.clone(),
                cursor: expected.next_cursor.clone().unwrap()
            }]
        );
        assert!(r.probe_material_open(MaterialId::new(99).unwrap()).is_err());
        let input = r.cards[0].input.as_ref().unwrap();
        let outside = POINT {
            x: scene.cards[0].rect.x as i32 + 2,
            y: scene.cards[0].rect.y as i32 + 2,
        };
        let lp = LPARAM(((outside.x as u16 as u32) | ((outside.y as u16 as u32) << 16)) as isize);
        assert_eq!(
            unsafe { SendMessageW(input.surface.window.0, WM_NCHITTEST, None, Some(lp)) }.0,
            HTTRANSPARENT as isize
        );
        r.probe_material_open(MaterialId::new(3).unwrap()).unwrap();
        let prepared = publisher
            .prepare((), Revision::new(6), AuthEpoch::new(1))
            .unwrap();
        let update = publisher.begin_update().unwrap();
        publisher.publish(update, prepared).unwrap();
        scene.source_revision = Revision::new(6);
        scene.generation = SceneGeneration::new(2);
        if let CardContent::Materials(list) = &mut scene.cards[0].content {
            list.folder_revision = Revision::new(10);
            list.rows[2].resource_version = ResourceVersionId::new(999).unwrap();
            list.next_cursor.as_mut().unwrap().folder_revision = Revision::new(10);
        }
        r.present(&scene, now, &guard(&gate, scene.generation))
            .unwrap();
        assert!(r.take_events().unwrap().is_empty());
        r.probe_material_open(MaterialId::new(3).unwrap()).unwrap();
        assert!(
            matches!(&r.take_events().unwrap()[0],DesktopEvent::MaterialOpen{generation,resource_version,folder_revision,..}if *generation==scene.generation&&resource_version.get()==999&&folder_revision.get()==10)
        );
        r.probe_material_open(MaterialId::new(3).unwrap()).unwrap();
        r.hide();
        assert!(r.take_events().unwrap().is_empty());
        scene.generation = SceneGeneration::new(3);
        scene.cards[0].component_alpha = 0.;
        r.present(&scene, now, &guard(&gate, scene.generation))
            .unwrap();
        assert!(r.probe_material_open(MaterialId::new(3).unwrap()).is_err());
    }
    #[test]
    fn long_filename_pages_preserve_extension_and_only_visible_text_accepts_input() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_publisher, gate) = authority();
        let now = crate::native::local_now().unwrap();
        let mut scene = material_scene(now);
        scene.cards[0].rect.width = 230.;
        scene.cards[0].rect.height = 160.;
        if let CardContent::Materials(list) = &mut scene.cards[0].content {
            list.rows[4].name = "合成中文长文件名".repeat(20) + ".pptx";
        }
        let mut r = DesktopRenderer::new_hidden().unwrap();
        let g = guard(&gate, scene.generation);
        r.present(&scene, now, &g).unwrap();
        assert!(r.probe_material_open(MaterialId::new(5).unwrap()).is_err());
        let pages = r.last_metrics().pages[0].pages;
        assert!(pages > 1);
        let filename_block=r.cards[0].body.blocks.iter().find(|b|matches!(b.action,Some(InputAction::MaterialOpen{material_id,..})if material_id.get()==5)).unwrap();
        let mut lines = vec![DWRITE_LINE_METRICS::default(); filename_block.lines.len()];
        let mut count = 0;
        unsafe {
            filename_block
                .layout
                .GetLineMetrics(Some(&mut lines), &mut count)
                .unwrap();
        }
        let expected_units = match &scene.cards[0].content {
            CardContent::Materials(list) => list.rows[4].name.encode_utf16().count(),
            _ => unreachable!(),
        };
        assert_eq!(
            lines.iter().map(|line| line.length as usize).sum::<usize>(),
            expected_units
        );
        let mut found = false;
        let mut extension_visible = false;
        for _ in 0..pages {
            if r.probe_material_open(MaterialId::new(5).unwrap()).is_ok() {
                found = true;
                let events = r.take_events().unwrap();
                assert!(
                    matches!(&events[0],DesktopEvent::MaterialOpen{material_id,..}if material_id.get()==5)
                );
            }
            let card = &r.cards[0];
            let offset = card.body.pages[card.page];
            let end = card.body.pages.get(card.page + 1).copied().unwrap_or(
                card.source.rect.height - body_top(&card.source, card.calendar_unverified) - 12.
                    + offset,
            );
            let block=card.body.blocks.iter().find(|b|matches!(b.action,Some(InputAction::MaterialOpen{material_id,..})if material_id.get()==5)).unwrap();
            let (line, height) = block.lines.last().unwrap();
            if *line >= offset && *line + *height <= end + 0.1 {
                extension_visible = true;
                if let Some(directory) = std::env::var_os("FC_DESKTOP_CAPTURE_DIR") {
                    r.probe_capture_bmp_for_area(
                        &Path::new(&directory).join("materials-long-filename-tail.bmp"),
                        Rect {
                            x: 0.,
                            y: 0.,
                            width: 320.,
                            height: 240.,
                        },
                    )
                    .unwrap();
                }
            }
            r.next_page(ComponentKind::Materials).unwrap();
            r.present(&scene, now, &g).unwrap();
        }
        assert!(found);
        assert!(extension_visible);
        assert!(match &r.cards[0].source.content {
            CardContent::Materials(list) => list.rows[4].name.ends_with(".pptx"),
            _ => false,
        });
        gate.stop();
        assert!(r.take_events().unwrap().is_empty());
        r.hide();
        assert_eq!(r.last_metrics().input_regions, 0);
    }
    #[test]
    fn lesson_control_uses_only_its_button_and_cancels_old_scene_input() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_publisher, gate) = authority();
        let now = crate::native::local_now().unwrap();
        let mut scene = scene(now);
        scene.cards.clear();
        scene.lesson_control = Some(fc_presentation::scene::LessonControlContent {
            rect: Rect {
                x: 20.,
                y: 20.,
                width: 900.,
                height: 80.,
            },
            caption: "临时：合成语文 · 合成教师 · 原合成数学 · 仅本节有效".into(),
            can_change: true,
        });
        let mut renderer = DesktopRenderer::new_hidden().unwrap();
        renderer
            .present(&scene, now, &guard(&gate, scene.generation))
            .unwrap();
        let input = renderer.cards[0].input.as_ref().unwrap();
        assert_eq!(input.surface.rect.width, 144.);
        assert_eq!(input.surface.rect.height, 40.);
        assert_eq!(input.state.targets.borrow().len(), 1);
        assert_eq!(
            unsafe {
                SendMessageW(
                    input.surface.window.0,
                    WM_MOUSEACTIVATE,
                    Some(WPARAM(0)),
                    Some(LPARAM(0)),
                )
            }
            .0,
            MA_NOACTIVATE as isize
        );
        renderer.probe_lesson_control().unwrap();
        assert_eq!(
            renderer.take_events().unwrap(),
            vec![DesktopEvent::LessonManage {
                generation: scene.generation
            }]
        );
        renderer.probe_lesson_control().unwrap();
        scene.generation = SceneGeneration::new(2);
        renderer
            .present(&scene, now, &guard(&gate, scene.generation))
            .unwrap();
        assert!(renderer.take_events().unwrap().is_empty());
        renderer.probe_lesson_control().unwrap();
        renderer.hide();
        assert!(renderer.take_events().unwrap().is_empty());
        scene.lesson_control.as_mut().unwrap().can_change = false;
        renderer
            .present(&scene, now, &guard(&gate, scene.generation))
            .unwrap();
        assert!(renderer.cards[0].input.is_none());
        assert!(renderer.probe_lesson_control().is_err());
        assert_eq!(renderer.last_metrics().visible_windows, 0);
    }
}
