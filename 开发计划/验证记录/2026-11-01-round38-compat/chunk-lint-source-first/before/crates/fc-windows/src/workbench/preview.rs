//! Owned, opaque editing canvas. It renders only a bounded synthetic-background
//! snapshot supplied by the trusted renderer, never a desktop/wallpaper capture.
use super::*;
use fc_presentation::{layout::SceneNodeKey, Rect};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
const SUBCLASS: usize = 0xfc63;
const MAX_BYTES: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewNode {
    pub key: SceneNodeKey,
    pub parent: Option<SceneNodeKey>,
    pub rect: Rect,
    pub clip: Rect,
}
pub struct LayoutPreview {
    pub generation: u64,
    pub width: u16,
    pub height: u16,
    pub bgra: Vec<u8>,
    pub nodes: Vec<PreviewNode>,
}
impl std::fmt::Debug for LayoutPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutPreview")
            .field("generation", &self.generation)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pixel_bytes", &self.bgra.len())
            .field("node_count", &self.nodes.len())
            .finish()
    }
}
impl LayoutPreview {
    pub(super) fn validate(&self) -> Result<()> {
        let bytes = usize::from(self.width)
            .checked_mul(usize::from(self.height))
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if self.generation == 0
            || self.width == 0
            || self.height == 0
            || self.width > 960
            || self.height > 540
            || bytes > MAX_BYTES
            || self.bgra.len() != bytes
            || self.bgra.capacity() > MAX_BYTES
            || self.nodes.capacity() > 128
            || self.nodes.len() > 128
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let rect = |r: Rect| {
            [r.x, r.y, r.width, r.height]
                .into_iter()
                .all(|v| v.is_finite() && v.abs() <= 32768.)
                && r.width >= 0.
                && r.height >= 0.
        };
        for (i, n) in self.nodes.iter().enumerate() {
            if !rect(n.rect)
                || !rect(n.clip)
                || self.nodes[..i].iter().any(|old| old.key == n.key)
                || n.parent == Some(n.key)
                || n.parent
                    .is_some_and(|p| !self.nodes.iter().any(|n| n.key == p))
            {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
        }
        for n in &self.nodes {
            let mut next = n.parent;
            let mut depth = 0;
            while let Some(key) = next {
                depth += 1;
                if depth > 16 {
                    return Err(Error::from_hresult(E_INVALIDARG));
                }
                next = self
                    .node(key)
                    .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?
                    .parent;
            }
        }
        if self
            .bgra
            .chunks_exact(4)
            .any(|p| p[..3].iter().any(|c| *c > p[3]))
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        Ok(())
    }
    fn checker(&mut self) {
        let width = usize::from(self.width);
        for (i, p) in self.bgra.chunks_exact_mut(4).enumerate() {
            let tone = if (i % width / 16 + i / width / 16) % 2 == 0 {
                224u16
            } else {
                245
            };
            let a = u16::from(p[3]);
            for c in &mut p[..3] {
                *c = (u16::from(*c) + (tone * (255 - a) + 127) / 255).min(255) as u8
            }
            p[3] = 255;
        }
    }
    fn node(&self, key: SceneNodeKey) -> Option<&PreviewNode> {
        self.nodes.iter().find(|n| n.key == key)
    }
    fn hit(&self, x: f32, y: f32) -> Option<&PreviewNode> {
        if x < 0. || y < 0. || x >= self.width as f32 || y >= self.height as f32 {
            return None;
        }
        self.nodes
            .iter()
            .rev()
            .find(|n| n.rect.contains(x, y) && n.clip.contains(x, y))
    }
}
#[derive(Clone, Copy)]
struct Press {
    generation: u64,
    key: SceneNodeKey,
    x: f32,
    y: f32,
}
struct CanvasState {
    image: RefCell<Option<LayoutPreview>>,
    selected: Cell<Option<SceneNodeKey>>,
    press: Cell<Option<Press>>,
    hover: Cell<Option<SceneNodeKey>>,
    parent: *const State,
}
pub(super) struct Canvas {
    hwnd: HWND,
    state: Box<CanvasState>,
}
impl Canvas {
    pub(super) fn new(parent_hwnd: HWND, parent: &State, instance: HINSTANCE) -> Result<Self> {
        let state = Box::new(CanvasState {
            image: RefCell::new(None),
            selected: Cell::new(None),
            press: Cell::new(None),
            hover: Cell::new(None),
            parent: parent as *const _,
        });
        // SAFETY: an owned child of this UI-thread workbench; no activation or focus.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(SS_NOTIFY.0),
                0,
                0,
                1,
                1,
                Some(parent_hwnd),
                None,
                Some(instance),
                None,
            )?
        };
        let canvas = Self { hwnd, state };
        // SAFETY: stable boxed state and parent live until Drop removes subclass.
        if !unsafe {
            SetWindowSubclass(
                hwnd,
                Some(canvas_proc),
                SUBCLASS,
                (&raw const *canvas.state) as usize,
            )
        }
        .as_bool()
        {
            return Err(Error::from_hresult(E_FAIL));
        }
        Ok(canvas)
    }
    pub(super) fn hwnd(&self) -> HWND {
        self.hwnd
    }
    pub(super) fn generation(&self) -> Option<u64> {
        self.state.image.borrow().as_ref().map(|p| p.generation)
    }
    pub(super) fn size(&self, width: i32) -> (i32, i32) {
        self.state.image.borrow().as_ref().map_or((1, 1), |p| {
            let w = width.min(p.width as i32).max(1);
            (w, (w * p.height as i32 / p.width as i32).max(1))
        })
    }
    pub(super) fn probe_point(&self, x: u16, y: u16) -> Result<(HWND, i32, i32)> {
        let image = self
            .state
            .image
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        let image = image
            .as_ref()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if x >= image.width || y >= image.height {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let mut client = RECT::default();
        unsafe {
            GetClientRect(self.hwnd, &mut client)?;
        }
        Ok((
            self.hwnd,
            i32::from(x) * client.right / i32::from(image.width),
            i32::from(y) * client.bottom / i32::from(image.height),
        ))
    }
    pub(super) fn update(&self, mut image: LayoutPreview) -> Result<()> {
        image.validate()?;
        if self.generation() == Some(image.generation) {
            return Ok(());
        }
        self.cancel();
        image.checker();
        self.state.image.replace(Some(image));
        self.state.selected.set(None);
        self.state.hover.set(None);
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
        Ok(())
    }
    pub(super) fn select(&self, key: Option<SceneNodeKey>) -> Result<()> {
        if let Some(key) = key {
            if self
                .state
                .image
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_UNEXPECTED))?
                .as_ref()
                .is_none_or(|p| p.node(key).is_none())
            {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
        }
        self.state.selected.set(key);
        // Selection tools have their own opaque brush; component/group alpha
        // changes only the supplied raster and never disables this outline.
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        Ok(())
    }
    pub(super) fn cancel(&self) {
        cancel(self.hwnd, &self.state);
    }
    #[cfg(test)]
    pub(super) fn pixel(&self, index: usize) -> Option<[u8; 4]> {
        self.state
            .image
            .borrow()
            .as_ref()?
            .bgra
            .get(index * 4..index * 4 + 4)?
            .try_into()
            .ok()
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        self.cancel();
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(canvas_proc), SUBCLASS);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
fn cancel(hwnd: HWND, state: &CanvasState) {
    state.press.set(None);
    state.hover.set(None);
    unsafe {
        if GetCapture() == hwnd {
            let _ = ReleaseCapture();
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}
fn enabled(state: &CanvasState) -> bool {
    let Some(parent) = (unsafe { state.parent.as_ref() }) else {
        return false;
    };
    !parent.suppress.get()
        && !parent.capturing.get()
        && parent
            .view
            .try_borrow()
            .is_ok_and(|v| v.as_ref().is_some_and(|v| !v.busy))
}
fn point(hwnd: HWND, lparam: LPARAM, image: &LayoutPreview) -> Result<(f32, f32)> {
    let mut r = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut r)?;
    }
    let w = (r.right - r.left).max(1) as f32;
    let h = (r.bottom - r.top).max(1) as f32;
    let x = (lparam.0 as u16) as i16 as f32;
    let y = ((lparam.0 >> 16) as u16) as i16 as f32;
    Ok((x * image.width as f32 / w, y * image.height as f32 / h))
}
fn emit(state: &CanvasState, event: Event) {
    if enabled(state) {
        if let Some(parent) = unsafe { state.parent.as_ref() } {
            parent.queue_layout(event);
        }
    }
}
fn down(hwnd: HWND, state: &CanvasState, lparam: LPARAM) -> Result<()> {
    if !enabled(state) {
        cancel(hwnd, state);
        return Ok(());
    }
    let found = {
        let image = state
            .image
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        let Some(image) = image.as_ref() else {
            return Ok(());
        };
        let (x, y) = point(hwnd, lparam, image)?;
        image.hit(x, y).map(|n| {
            (
                image.generation,
                n.key,
                x,
                y,
                n.parent.is_some_and(|p| p.scope == n.key.scope),
            )
        })
    };
    if let Some((generation, key, x, y, movable)) = found {
        state.selected.set(Some(key));
        state.press.set(movable.then_some(Press {
            generation,
            key,
            x,
            y,
        }));
        state.hover.set(None);
        unsafe {
            if movable && IsWindowVisible(hwnd).as_bool() {
                let _ = SetCapture(hwnd);
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        emit(
            state,
            Event::LayoutSelect {
                generation,
                scope: key.scope,
                id: key.id,
            },
        );
    }
    Ok(())
}
fn motion(hwnd: HWND, state: &CanvasState, lparam: LPARAM) -> Result<()> {
    if state.press.get().is_none() {
        return Ok(());
    }
    if !enabled(state) {
        cancel(hwnd, state);
        return Ok(());
    }
    let hover = {
        let image = state
            .image
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        let Some(image) = image.as_ref() else {
            return Ok(());
        };
        let (x, y) = point(hwnd, lparam, image)?;
        image.hit(x, y).map(|n| n.key)
    };
    state.hover.set(hover);
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    Ok(())
}
fn up(hwnd: HWND, state: &CanvasState, lparam: LPARAM) -> Result<()> {
    let press = state.press.take();
    state.hover.set(None);
    unsafe {
        if GetCapture() == hwnd {
            let _ = ReleaseCapture();
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    let Some(press) = press else { return Ok(()) };
    if !enabled(state) {
        return Ok(());
    }
    let event = {
        let image = state
            .image
            .try_borrow()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
        let Some(image) = image.as_ref() else {
            return Ok(());
        };
        let (x, y) = point(hwnd, lparam, image)?;
        if image.generation != press.generation || (x - press.x).hypot(y - press.y) < 6. {
            return Ok(());
        }
        let Some(node) = image.hit(x, y) else {
            return Ok(());
        };
        if node.key.scope != press.key.scope || node.key == press.key {
            return Ok(());
        }
        let edge = (y - node.rect.y) < (node.rect.height * 0.2).clamp(3., 12.)
            || (x - node.rect.x) < (node.rect.width * 0.15).clamp(3., 12.);
        let (target, before) = if edge {
            let Some(parent) = node.parent else {
                return Ok(());
            };
            if parent.scope != press.key.scope {
                return Ok(());
            }
            (parent.id, Some(node.key.id))
        } else {
            (node.key.id, None)
        };
        if target == press.key.id {
            return Ok(());
        }
        Some(Event::LayoutDrop {
            generation: press.generation,
            scope: press.key.scope,
            source: press.key.id,
            target,
            before,
        })
    };
    if let Some(event) = event {
        emit(state, event)
    }
    Ok(())
}
struct Paint {
    hwnd: HWND,
    ps: PAINTSTRUCT,
    dc: HDC,
}
impl Paint {
    fn begin(hwnd: HWND) -> Self {
        let mut ps = PAINTSTRUCT::default();
        let dc = unsafe { BeginPaint(hwnd, &mut ps) };
        Self { hwnd, ps, dc }
    }
}
impl Drop for Paint {
    fn drop(&mut self) {
        unsafe {
            let _ = EndPaint(self.hwnd, &self.ps);
        }
    }
}
fn paint(hwnd: HWND, state: &CanvasState) -> Result<()> {
    let paint = Paint::begin(hwnd);
    if paint.dc.is_invalid() {
        return Err(Error::from_thread());
    }
    let mut client = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut client)?;
        let _ = FillRect(paint.dc, &client, HBRUSH(GetStockObject(WHITE_BRUSH).0));
    }
    let image = state
        .image
        .try_borrow()
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
    let Some(image) = image.as_ref() else {
        return Ok(());
    };
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: image.width as i32,
            biHeight: -(image.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let result = StretchDIBits(
            paint.dc,
            0,
            0,
            client.right,
            client.bottom,
            0,
            0,
            image.width as i32,
            image.height as i32,
            Some(image.bgra.as_ptr().cast()),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        if result == -1 || result == 0 && IsWindowVisible(hwnd).as_bool() {
            return Err(Error::from_hresult(E_FAIL));
        }
    }
    for (key, color) in [
        (state.selected.get(), COLORREF(0x00b65a00)),
        (state.hover.get(), COLORREF(0x00009700)),
    ] {
        let Some(key) = key else { continue };
        let Some(node) = image.node(key) else {
            continue;
        };
        let sx = client.right as f32 / image.width as f32;
        let sy = client.bottom as f32 / image.height as f32;
        let l = node.rect.x.max(node.clip.x).max(0.);
        let t = node.rect.y.max(node.clip.y).max(0.);
        let r = (node.rect.x + node.rect.width)
            .min(node.clip.x + node.clip.width)
            .min(image.width as f32);
        let b = (node.rect.y + node.rect.height)
            .min(node.clip.y + node.clip.height)
            .min(image.height as f32);
        if r > l && b > t {
            let rect = RECT {
                left: (l * sx) as i32,
                top: (t * sy) as i32,
                right: (r * sx).ceil() as i32,
                bottom: (b * sy).ceil() as i32,
            };
            unsafe {
                let brush = CreateSolidBrush(color);
                if brush.is_invalid() {
                    return Err(Error::from_thread());
                }
                let _ = FrameRect(paint.dc, &rect, brush);
                let _ = DeleteObject(brush.into());
            }
        }
    }
    Ok(())
}
unsafe extern "system" fn canvas_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    reference: usize,
) -> LRESULT {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let Some(state) = (unsafe { (reference as *const CanvasState).as_ref() }) else {
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        };
        if message == WM_NCDESTROY {
            cancel(hwnd, state);
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(canvas_proc), SUBCLASS);
                return DefSubclassProc(hwnd, message, wparam, lparam);
            }
        }
        let handled = match message {
            WM_PAINT => Some(paint(hwnd, state)),
            WM_ERASEBKGND => return LRESULT(1),
            WM_LBUTTONDOWN => Some(down(hwnd, state, lparam)),
            WM_MOUSEMOVE => Some(motion(hwnd, state, lparam)),
            WM_LBUTTONUP => Some(up(hwnd, state, lparam)),
            WM_CANCELMODE | WM_CAPTURECHANGED => {
                cancel(hwnd, state);
                Some(Ok(()))
            }
            _ => None,
        };
        if let Some(result) = handled {
            if let Err(error) = result {
                if let Some(parent) = unsafe { state.parent.as_ref() } {
                    parent.fail(error)
                }
            }
            LRESULT(0)
        } else {
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
    }));
    match result {
        Ok(result) => result,
        Err(_) => {
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
    use fc_domain::{ids::LayoutNodeId, layout::LayoutScope};
    use fc_presentation::workbench::Action;
    fn key(n: u64) -> SceneNodeKey {
        SceneNodeKey {
            scope: LayoutScope::Public,
            id: LayoutNodeId::new(n).unwrap(),
        }
    }
    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
    fn image(generation: u64) -> LayoutPreview {
        let all = rect(0., 0., 240., 120.);
        LayoutPreview {
            generation,
            width: 240,
            height: 120,
            bgra: vec![0; 240 * 120 * 4],
            nodes: vec![
                PreviewNode {
                    key: key(1),
                    parent: None,
                    rect: all,
                    clip: all,
                },
                PreviewNode {
                    key: key(2),
                    parent: Some(key(1)),
                    rect: rect(10., 10., 90., 90.),
                    clip: all,
                },
                PreviewNode {
                    key: key(3),
                    parent: Some(key(1)),
                    rect: rect(130., 10., 90., 90.),
                    clip: all,
                },
            ],
        }
    }
    fn view() -> View {
        View {
            title: "Native layout preview test".into(),
            description: "Owned preview".into(),
            status: String::new(),
            fields: vec![],
            actions: vec![Action {
                id: ActionId(1),
                label: "Lock".into(),
                enabled: true,
            }],
            busy: false,
            grid: None,
        }
    }
    fn window() -> Window {
        let w = Window::new().unwrap();
        w.set_lock_action(ActionId(1)).unwrap();
        w.render(&view()).unwrap();
        w
    }
    fn raw_point(w: &Window, x: u16, y: u16) -> (HWND, LPARAM) {
        let (hwnd, x, y) = w
            .state
            .preview
            .borrow()
            .as_ref()
            .unwrap()
            .probe_point(x, y)
            .unwrap();
        (
            hwnd,
            LPARAM(((y as u16 as u32) << 16 | x as u16 as u32) as isize),
        )
    }
    #[test]
    fn native_layout_preview_opaque_checker_select_drag_reorder_and_hidden_capture_is_untouched() {
        let w = window();
        let original = unsafe { GetCapture() };
        w.set_layout_preview(Some(image(10))).unwrap();
        w.set_layout_selection(Some(key(2))).unwrap();
        assert_eq!(
            w.state
                .preview
                .borrow()
                .as_ref()
                .unwrap()
                .state
                .selected
                .get(),
            Some(key(2))
        );
        assert!(w.take_events().unwrap().is_empty());
        assert_eq!(
            w.state.preview.borrow().as_ref().unwrap().pixel(0),
            Some([224, 224, 224, 255])
        );
        assert_eq!(
            w.state.preview.borrow().as_ref().unwrap().pixel(16),
            Some([245, 245, 245, 255])
        );
        w.probe_layout_drag((50, 50), (170, 50)).unwrap();
        assert_eq!(unsafe { GetCapture() }, original);
        let events = w.take_events().unwrap();
        assert!(events.iter().any(|e|matches!(e,Event::LayoutSelect{generation:10,scope:LayoutScope::Public,id}if *id==key(2).id)));
        assert!(events.iter().any(|e|matches!(e,Event::LayoutDrop{generation:10,source,target,before:None,..}if *source==key(2).id&&*target==key(3).id)));
        w.probe_layout_drag((50, 50), (132, 13)).unwrap();
        let events = w.take_events().unwrap();
        assert!(events.iter().any(|e|matches!(e,Event::LayoutDrop{generation:10,source,target,before:Some(before),..}if *source==key(2).id&&*target==key(1).id&&*before==key(3).id)));
        let (hwnd, _) = raw_point(&w, 50, 50);
        unsafe {
            let _ = SendMessageW(hwnd, WM_PAINT, None, None);
        }
        w.take_events().unwrap();
        assert_eq!(unsafe { GetCapture() }, original);
        assert!(!w.is_visible());
    }
    #[test]
    fn native_layout_preview_replacement_busy_clip_scope_and_lock_reject_stale_gestures() {
        let w = window();
        w.set_layout_preview(Some(image(20))).unwrap();
        let (hwnd, start) = raw_point(&w, 50, 50);
        unsafe {
            let _ = SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(start));
        }
        w.set_layout_preview(Some(image(21))).unwrap();
        let (hwnd, end) = raw_point(&w, 170, 50);
        unsafe {
            let _ = SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(end));
        }
        assert!(!w
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::LayoutSelect { .. } | Event::LayoutDrop { .. })));
        let mut clipped = image(22);
        clipped.nodes[1].clip = rect(0., 0., 1., 1.);
        w.set_layout_preview(Some(clipped)).unwrap();
        w.probe_layout_drag((50, 50), (170, 50)).unwrap();
        assert!(!w
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e,Event::LayoutSelect{id,..}if *id==key(2).id)));
        let mut cross = image(23);
        cross.nodes[2].key.scope = LayoutScope::Subject(fc_domain::profile::SubjectProfileKey {
            class_id: fc_domain::ids::ClassId::new(1).unwrap(),
            subject_id: fc_domain::ids::SubjectId::new(1).unwrap(),
            teacher_id: fc_domain::ids::PrincipalId::new(2).unwrap(),
        });
        w.set_layout_preview(Some(cross)).unwrap();
        w.probe_layout_drag((50, 50), (170, 50)).unwrap();
        assert!(!w
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::LayoutDrop { .. })));
        let mut busy = view();
        busy.busy = true;
        w.render(&busy).unwrap();
        assert!(w.probe_layout_drag((50, 50), (170, 50)).is_err());
        let (hwnd, start) = raw_point(&w, 50, 50);
        unsafe {
            let _ = SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(start));
        }
        assert!(!w
            .take_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::LayoutSelect { .. } | Event::LayoutDrop { .. })));
        w.render(&view()).unwrap();
        w.probe_submit(ActionId(1), FormData::new()).unwrap();
        assert!(w.state.preview.borrow().is_none());
        let events = w.take_events().unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e,Event::Submit(ActionId(1),f)if f.is_empty())));
        assert!(!events
            .iter()
            .any(|e| matches!(e, Event::LayoutSelect { .. } | Event::LayoutDrop { .. })));
    }
    #[test]
    fn native_layout_preview_strict_capacity_generations_and_owned_child_cleanup() {
        let w = window();
        w.set_layout_preview(Some(image(30))).unwrap();
        let mut malformed = image(31);
        malformed.bgra[0] = 255;
        assert!(w.set_layout_preview(Some(malformed)).is_err());
        assert!(w.state.preview.borrow().is_none());
        let mut oversized = image(32);
        oversized.width = 961;
        assert!(w.set_layout_preview(Some(oversized)).is_err());
        let mut duplicate = image(33);
        duplicate.nodes.push(duplicate.nodes[1].clone());
        assert!(w.set_layout_preview(Some(duplicate)).is_err());
        let mut nonfinite = image(34);
        nonfinite.nodes[0].rect.x = f32::NAN;
        assert!(w.set_layout_preview(Some(nonfinite)).is_err());
        let mut cycle = image(35);
        cycle.nodes[0].parent = Some(key(2));
        assert!(w.set_layout_preview(Some(cycle)).is_err());
        for generation in 40..72 {
            w.set_layout_preview(Some(image(generation))).unwrap();
            let hwnd = w.state.preview.borrow().as_ref().unwrap().hwnd();
            w.clear_layout_preview();
            assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool());
            assert!(w.state.preview.borrow().is_none());
        }
        w.set_layout_preview(Some(image(80))).unwrap();
        let hwnd = w.state.preview.borrow().as_ref().unwrap().hwnd();
        w.render(&view()).unwrap();
        assert_eq!(w.state.preview.borrow().as_ref().unwrap().hwnd(), hwnd);
        let mut dynamic = view();
        dynamic.fields.push(fc_presentation::workbench::Field {
            id: FieldId(777),
            label: "Parent".into(),
            kind: FieldKind::Choice(vec![]),
            value: String::new(),
            max_utf16: 64,
        });
        w.render(&dynamic).unwrap();
        w.set_layout_preview(Some(image(81))).unwrap();
        let dynamic_hwnd = w.state.preview.borrow().as_ref().unwrap().hwnd();
        dynamic.fields[0].kind = FieldKind::Choice(vec![fc_presentation::workbench::Choice {
            value: "1".into(),
            label: "Root".into(),
        }]);
        dynamic.fields[0].value = "1".into();
        w.render(&dynamic).unwrap();
        assert_eq!(
            w.state.preview.borrow().as_ref().unwrap().hwnd(),
            dynamic_hwnd
        );
        let mut other = view();
        other.title = "Different owner page".into();
        w.render(&other).unwrap();
        assert!(w.state.preview.borrow().is_none());
        assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool());
        w.set_layout_preview(Some(image(82))).unwrap();
        w.hide();
        assert!(w.state.preview.borrow().is_none());
    }
}
