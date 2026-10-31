//! UI-thread resident desktop orchestration. SQLite runs only through the shared
//! Work pool; the UI never takes Backend's mutex or refreshes a manager lease.
use super::{fatal, profile_content_flow, Outcome, Work};
use fc_bootstrap::backend::{Backend, DesktopLessonStamp, SessionControl};
use fc_bootstrap::profile_content_cache::TextFrame;
use fc_domain::{
    desktop::{ComponentKind, DesktopFacts},
    ids::{Revision, SceneGeneration},
    materials::MaterialCursor,
    profile_content::{PageContentRequest, PublishedContentDescriptor, PublishedContentSnapshot},
    schedule::AcademicDay,
    time::{LocalDate, LocalDateTime},
};
use fc_presentation::{
    card_body::card_body_spec,
    scene::{build_scene_with_content, CardContent, SceneSnapshot},
    Rect,
};
use fc_runtime::{
    health::{Health, HealthGate},
    worker::{JobTicket, WorkerPool},
};
use fc_windows::{
    desktop::{
        DesktopEvent, DesktopMetrics, DesktopRenderer, NativeTextMeasure, PresentOutcome,
        PublicationGuard, DESKTOP_SCOPE,
    },
    host::HostHints,
    materials::raster::{RasterCache, RasterFrame},
    system::CurrentClock,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const RETRY: Duration = Duration::from_secs(1);
const FACTS_DEADLINE: Duration = Duration::from_secs(5);
const IMAGES_DEADLINE: Duration = Duration::from_secs(30);
const DISPLAY_LEASE: Duration = Duration::from_secs(2);
// At most 4096 eligible assignment slots, so even one-record batches fit this
// complete, bounded history. Backward navigation never guesses a 64-item step.
const HISTORY_LIMIT: usize = fc_domain::identity::MAX_ASSIGNMENTS;

fn time_requires_scene(
    scene: Option<&SceneSnapshot>,
    previous: Option<LocalDateTime>,
    now: LocalDateTime,
) -> bool {
    scene.is_none_or(|scene| !scene.valid_at(now))
        || previous.is_some_and(|previous| now < previous)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Navigation {
    Refresh,
    Forward { from: u64 },
    Backward { from: u64, target: u64 },
    Material,
}
#[derive(Default)]
struct BatchHistory {
    previous: Vec<u64>,
}
impl BatchHistory {
    fn target(&self, scene: &SceneSnapshot, event: &DesktopEvent) -> Option<(u64, Navigation)> {
        let DesktopEvent::HomeworkBatch { generation, offset } = *event else {
            return None;
        };
        if generation != scene.generation {
            return None;
        }
        let page = scene
            .cards
            .iter()
            .find(|c| c.kind == ComponentKind::Homework)?
            .page;
        if offset > page.offset {
            let target = page.offset.checked_add(page.shown)?;
            (page.shown > 0 && target < page.total && target == offset)
                .then_some((target, Navigation::Forward { from: page.offset }))
        } else if offset < page.offset {
            let target = *self.previous.last()?;
            (target < page.offset).then_some((
                target,
                Navigation::Backward {
                    from: page.offset,
                    target,
                },
            ))
        } else {
            None
        }
    }
    fn accept(&mut self, navigation: Navigation, offset: u64) -> bool {
        match navigation {
            Navigation::Material => true,
            Navigation::Refresh => {
                self.previous.clear();
                offset == 0
            }
            Navigation::Forward { from } => {
                if offset <= from || self.previous.len() >= HISTORY_LIMIT {
                    return false;
                }
                self.previous.push(from);
                true
            }
            Navigation::Backward { from, target } => {
                if offset != target || target >= from || self.previous.last() != Some(&target) {
                    return false;
                }
                self.previous.pop();
                true
            }
        }
    }
    fn clear(&mut self) {
        self.previous.clear();
    }
}
#[derive(Clone)]
struct Request {
    offset: u64,
    navigation: Navigation,
    material_cursor: Option<MaterialCursor>,
}
struct Pending {
    ticket: JobTicket<super::Result<Outcome>>,
    source: Revision,
    generation: Option<SceneGeneration>,
    offset: u64,
    navigation: Navigation,
    material_cursor: Option<MaterialCursor>,
    cancelled: bool,
}
struct PendingImages {
    deadline: Instant,
    ticket: JobTicket<super::Result<Outcome>>,
    source: Revision,
    generation: SceneGeneration,
    cancelled: bool,
}
struct PendingPage {
    deadline: Instant,
    ticket: JobTicket<super::Result<Outcome>>,
    source: Revision,
    generation: SceneGeneration,
    request: PageContentRequest,
    cancelled: bool,
}
/// This linearization check is shared by image completion and pre-publication.
/// It never treats a cancellation signal as physical worker completion.
fn content_scene_current(
    scene: &SceneSnapshot,
    generation: SceneGeneration,
    source: Revision,
    now: LocalDateTime,
    gate: &HealthGate,
) -> bool {
    let status = gate.status();
    status.health == Health::Healthy
        && status.revision == source
        && scene.generation == generation
        && scene.source_revision == source
        && scene.valid_at(now)
}
fn intersects(a: Rect, b: Rect) -> bool {
    a.x.max(b.x) < (a.x + a.width).min(b.x + b.width)
        && a.y.max(b.y) < (a.y + a.height).min(b.y + b.height)
}
fn image_page_matches(scene: &SceneSnapshot, event: &DesktopEvent) -> bool {
    let DesktopEvent::ImagePage {
        generation,
        slot,
        content_revision,
        view_revision,
        page,
    } = event
    else {
        return false;
    };
    *generation == scene.generation
        && scene.cards.iter().enumerate().any(|(index, card)| {
            let (Some(identity), CardContent::Image(image)) =
                (card.content_identity, &card.content)
            else {
                return false;
            };
            if identity.slot != *slot
                || identity.revision != *content_revision
                || identity.view_revision != *view_revision
                || *page >= image.total_pages
                || *page == image.current_page
                || image.current_page.abs_diff(*page) != 1
            {
                return false;
            }
            let Some(node) = scene.paint_tree.as_ref().and_then(|tree| {
                tree.nodes.iter().find(|node| {
                    node.card_index == Some(index)
                        && node.key.scope == slot.scope
                        && node.key.id == slot.node_id
                        && node.effective_opacity > 0.
                })
            }) else {
                return false;
            };
            let Some(pager) = card_body_spec(card.kind, &card.content, false, None)
                .ok()
                .and_then(|spec| spec.image_geometry(card.rect.width, card.rect.height).ok())
                .and_then(|geometry| geometry.pager)
            else {
                return false;
            };
            let target = if *page < image.current_page {
                pager.previous
            } else {
                pager.next
            };
            intersects(
                Rect {
                    x: card.rect.x + target.x,
                    y: card.rect.y + target.y,
                    ..target
                },
                node.clip,
            )
        })
}
fn image_entries(
    scene: &SceneSnapshot,
    published: &[PublishedContentSnapshot],
) -> Vec<PublishedContentSnapshot> {
    let mut entries = Vec::new();
    for entry in published {
        let PublishedContentDescriptor::Image {
            current,
            total_pages,
        } = &entry.content
        else {
            continue;
        };
        if *total_pages < 2 {
            continue;
        }
        let exact=scene.cards.iter().any(|card|{
            let Some(identity)=card.content_identity else{return false;};
            identity.slot==entry.slot&&identity.revision==entry.revision&&identity.view_revision==entry.view_revision
                &&matches!(&card.content,CardContent::Image(image) if image.current==*current&&image.current_page==entry.current_page&&image.total_pages==*total_pages)
        });
        if !exact {
            continue;
        }
        let candidates = [
            entry.current_page.checked_sub(1),
            entry
                .current_page
                .checked_add(1)
                .filter(|page| *page < *total_pages),
        ];
        if candidates.into_iter().flatten().any(|page| {
            image_page_matches(
                scene,
                &DesktopEvent::ImagePage {
                    generation: scene.generation,
                    slot: entry.slot,
                    content_revision: entry.revision,
                    view_revision: entry.view_revision,
                    page,
                },
            )
        }) {
            entries.push(entry.clone());
        }
    }
    entries
}
#[derive(Default)]
struct RenderNotice {
    message: Option<String>,
    failed: bool,
}
impl RenderNotice {
    fn failed(&mut self, message: &str) {
        self.message = Some(message.to_owned());
        self.failed = true;
    }
    fn rendered(&mut self, outcome: PresentOutcome) {
        if self.failed
            && matches!(
                outcome,
                PresentOutcome::Published | PresentOutcome::Unchanged | PresentOutcome::Offscreen
            )
        {
            self.message = Some(String::new());
            self.failed = false;
        }
    }
    fn take(&mut self) -> Option<String> {
        self.message.take()
    }
}
pub(super) struct Controller {
    root: Option<PathBuf>,
    raster: Arc<RasterCache>,
    text: TextFrame,
    images: Option<PendingImages>,
    page: Option<PendingPage>,
    ready_images: Vec<Arc<RasterFrame>>,
    needs_scene: bool,
    next_images: Instant,
    published_content: Option<SceneGeneration>,
    images_verified: Option<(SceneGeneration, Revision)>,

    renderer: DesktopRenderer,
    measure: NativeTextMeasure,
    facts: Option<Box<DesktopFacts>>,
    scene: Option<SceneSnapshot>,
    generation: SceneGeneration,
    work: Option<Rect>,
    pending: Option<Pending>,
    requested: Option<Request>,
    control: Option<SessionControl>,
    material_events: Vec<DesktopEvent>,
    lesson_event: Option<DesktopLessonStamp>,
    refresh_at: Option<LocalDateTime>,
    history: BatchHistory,
    next_fetch: Instant,
    next_render: Instant,
    notice: RenderNotice,
    last_sample: Option<LocalDateTime>,
    stopped: bool,
    probe: bool,
    probe_presented: bool,
}
// The read context expires at every timetable boundary, even if a layout error
// prevented producing a SceneSnapshot. Retained material ownership is never aged
// into a new lesson by changing captured_at on an old snapshot.
fn next_context_boundary(facts: &DesktopFacts) -> Option<LocalDateTime> {
    let now = facts.captured_at;
    let d = now.date;
    let tomorrow = LocalDate::new(d.year(), d.month(), d.day() + 1).or_else(|_| {
        if d.month() < 12 {
            LocalDate::new(d.year(), d.month() + 1, 1)
        } else {
            LocalDate::new(d.year() + 1, 1, 1)
        }
    });
    let mut next = tomorrow
        .ok()
        .and_then(|date| LocalDateTime::new(date, 0).ok());
    if let Some(class) = facts
        .identity
        .installation
        .as_ref()
        .and_then(|i| i.current_class)
    {
        if let Ok(AcademicDay::Scheduled(day)) = facts.schedule.resolve_day(class, now.date) {
            let weekday = day.source_date.weekday();
            for row in &day.table.rows {
                if !row.enabled_days[weekday.index()] {
                    continue;
                }
                for boundary in [row.range.begin, row.range.end] {
                    let second = u32::from(boundary.get()) * 60;
                    if second > now.second_of_day() {
                        if let Ok(end) = LocalDateTime::new(now.date, second) {
                            if next.is_none_or(|old| end < old) {
                                next = Some(end);
                            }
                        }
                    }
                }
            }
        }
    }
    next
}
fn current_material_cursor(facts: &DesktopFacts) -> Option<MaterialCursor> {
    let page = facts.materials_page.as_ref()?;
    (page.offset > 0).then(|| MaterialCursor {
        folder_id: page.folder.id,
        folder_revision: page.folder.revision,
        scope: page.folder.scope,
        class_id: page.class_id,
        source: page.source.clone(),
        offset: page.offset,
    })
}
fn material_page_matches(facts: &DesktopFacts, cursor: Option<&MaterialCursor>) -> bool {
    match cursor {
        None => facts.materials_page.as_ref().is_none_or(|p| p.offset == 0),
        Some(cursor) => facts.materials_page.as_ref().is_some_and(|p| {
            p.offset == cursor.offset
                && cursor
                    .validate(&p.folder, p.source.as_ref(), p.class_id)
                    .is_ok()
        }),
    }
}
fn material_event_matches(scene: &SceneSnapshot, event: &DesktopEvent) -> bool {
    scene.cards.iter().any(|card| {
        let CardContent::Materials(content) = &card.content else {
            return false;
        };
        match event {
            DesktopEvent::MaterialOpen {
                generation,
                source,
                folder_id,
                folder_revision,
                material_id,
                resource_version,
            } => {
                *generation == scene.generation
                    && *source == content.source
                    && Some(*folder_id) == content.folder_id
                    && *folder_revision == content.folder_revision
                    && content.rows.iter().any(|r| {
                        r.material_id == *material_id && r.resource_version == *resource_version
                    })
            }
            DesktopEvent::MaterialPage {
                generation,
                source,
                cursor,
            } => {
                *generation == scene.generation
                    && *source == content.source
                    && content.next_cursor.as_ref() == Some(cursor)
            }
            _ => false,
        }
    })
}
impl Controller {
    pub(super) fn new(probe: bool) -> Result<Self, String> {
        let mut renderer = if probe {
            DesktopRenderer::new_hidden()
        } else {
            DesktopRenderer::new()
        }
        .map_err(|_| "桌面窗口初始化失败".to_owned())?;
        let raster = profile_content_flow::raster_cache();
        renderer.set_raster_cache(raster.clone());
        let measure = NativeTextMeasure::new().map_err(|_| "桌面文字测量初始化失败".to_owned())?;
        Ok(Self {
            root: None,
            raster,
            text: TextFrame::default(),
            images: None,
            page: None,
            ready_images: Vec::new(),
            needs_scene: true,
            next_images: Instant::now(),
            published_content: None,
            images_verified: None,
            renderer,
            measure,
            facts: None,
            scene: None,
            generation: SceneGeneration::default(),
            work: None,
            pending: None,
            requested: None,
            control: None,
            material_events: Vec::new(),
            lesson_event: None,
            refresh_at: None,
            history: BatchHistory::default(),
            next_fetch: Instant::now(),
            next_render: Instant::now(),
            notice: RenderNotice::default(),
            last_sample: None,
            stopped: false,
            probe,
            probe_presented: false,
        })
    }
    pub(super) fn set_root(&mut self, root: PathBuf) {
        if self.root.as_ref() != Some(&root) {
            self.invalidate();
            self.cancel_pending();
        }
        self.root = Some(root);
    }
    fn clear_content_capability(&mut self) {
        if self.published_content.take().is_some() {
            self.needs_scene = true;
        }
        if let Some(control) = &self.control {
            control.clear_profile_content_scene();
        }
    }
    fn cancel_content_jobs(&mut self) {
        if let Some(images) = &mut self.images {
            images.ticket.cancel();
            images.cancelled = true;
        }
        if let Some(page) = &mut self.page {
            page.ticket.cancel();
            page.cancelled = true;
        }
        self.ready_images.clear();
        self.images_verified = None;
    }
    fn expire_content_jobs(&mut self) {
        let expired =
            self.images
                .as_ref()
                .is_some_and(|pending| !pending.cancelled && Instant::now() >= pending.deadline)
                || self.page.as_ref().is_some_and(|pending| {
                    !pending.cancelled && Instant::now() >= pending.deadline
                });
        if expired {
            self.hide();
            self.cancel_pending();
        }
    }
    pub(super) fn hide(&mut self) {
        self.clear_content_capability();
        self.cancel_content_jobs();
        self.needs_scene = true;
        if let Some(control) = &self.control {
            control.clear_material_scene();
            control.clear_desktop_lesson_scene();
        }
        self.material_events.clear();
        self.lesson_event = None;
        self.renderer.hide();
        self.probe_presented = false;
    }
    /// The layout editor renders a separate synthetic scene. Release the hidden
    /// classroom surfaces first so its preview cannot coexist with that cache.
    pub(super) fn release_for_preview(&mut self) {
        self.invalidate();
        self.cancel_pending();
        self.next_fetch = Instant::now();
    }
    pub(super) fn stop(&mut self) {
        self.stopped = true;
        self.hide();
        if let Some(pending) = self.pending.take() {
            pending.ticket.cancel();
        }
        self.facts = None;
        self.text = TextFrame::default();
        self.scene = None;
        self.requested = None;
        self.history.clear();
        self.renderer.release_surfaces();
    }
    pub(super) fn probe_ready(&self) -> bool {
        self.probe && self.probe_presented && !self.stopped
    }
    pub(super) fn probe_scene(&self) -> Option<&SceneSnapshot> {
        self.scene.as_ref()
    }
    pub(super) fn probe_lesson_stamp(&self) -> Option<DesktopLessonStamp> {
        self.probe.then(|| self.lesson_stamp()).flatten()
    }
    pub(super) fn probe_lesson_click(&self) -> Result<(), String> {
        if !self.probe || !self.probe_presented {
            return Err("Hidden desktop lesson probe is not ready".into());
        }
        self.renderer
            .probe_lesson_control()
            .map_err(|_| "Hidden native lesson button unavailable".into())
    }
    pub(super) fn probe_metrics(&self) -> DesktopMetrics {
        self.renderer.diagnostics()
    }
    pub(super) fn take_lesson_event(&mut self) -> Option<DesktopLessonStamp> {
        self.lesson_event.take()
    }
    pub(super) fn take_material_events(&mut self) -> Vec<DesktopEvent> {
        std::mem::take(&mut self.material_events)
    }
    pub(super) fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }
    fn render_unavailable(&mut self, message: &str) {
        self.invalidate();
        self.cancel_pending();
        self.notice.failed(message);
        self.next_render = Instant::now() + RETRY;
    }
    fn lesson_stamp(&self) -> Option<DesktopLessonStamp> {
        let scene = self.scene.as_ref()?;
        let facts = self.facts.as_ref()?;
        let current = facts.lesson_context.current.as_ref()?;
        if scene.source_revision != facts.global_revision
            || !scene.lesson_control.as_ref().is_some_and(|c| c.can_change)
        {
            return None;
        }
        Some(DesktopLessonStamp {
            generation: scene.generation,
            source_revision: scene.source_revision,
            captured_at: scene.captured_at,
            valid_until: scene
                .valid_until
                .unwrap_or(current.effective_until)
                .min(current.effective_until),
            binding: fc_ports::DesktopLessonBinding {
                expected_revision: facts.lesson_context.revision,
                expected_identity_revision: facts.identity.revision,
                expected_schedule_revision: facts.schedule.revision,
                lesson: current.key.clone(),
            },
        })
    }
    fn rendered(&mut self, outcome: PresentOutcome) {
        let visible = matches!(
            outcome,
            PresentOutcome::Published | PresentOutcome::Unchanged
        ) || self.probe && outcome == PresentOutcome::Offscreen;
        if let Some(control) = &self.control {
            if !visible
                || !self
                    .lesson_stamp()
                    .is_some_and(|stamp| control.publish_desktop_lesson_scene(stamp))
            {
                control.clear_desktop_lesson_scene();
                self.lesson_event = None;
            }
        }
        if let Some(control) = &self.control {
            let published = visible
                && self.scene.as_ref().is_some_and(|scene| {
                    scene
                        .cards
                        .iter()
                        .find_map(|card| match &card.content {
                            CardContent::Materials(content) => Some(&content.source),
                            _ => None,
                        })
                        .is_some_and(|source| {
                            control.publish_material_scene(
                                source,
                                scene.generation,
                                scene.source_revision,
                            )
                        })
                });
            if !published {
                control.clear_material_scene();
                self.material_events.clear();
            }
        }
        if visible {
            if let (Some(control), Some(scene), Some(facts)) =
                (&self.control, &self.scene, &self.facts)
            {
                let entries = image_entries(scene, &facts.profile_contents);
                if !entries.is_empty() {
                    if !control.publish_profile_content_scene(
                        &entries,
                        scene.generation,
                        scene.source_revision,
                        scene.captured_at,
                        scene.valid_until,
                    ) {
                        self.hide();
                        return;
                    }
                    self.published_content = Some(scene.generation);
                } else {
                    self.clear_content_capability();
                }
            }
            // Native Cards now physically own their exact cache frames.
            self.ready_images.clear();
        } else if outcome == PresentOutcome::ContentPending {
            self.clear_content_capability();
            self.images_verified = None;
            self.ready_images.clear();
            self.next_images = Instant::now();
        } else {
            self.hide();
        }
        self.probe_presented = outcome == PresentOutcome::Offscreen;
        self.notice.rendered(outcome);
    }
    fn invalidate(&mut self) {
        self.hide();
        self.facts = None;
        self.text = TextFrame::default();
        self.scene = None;
        self.history.clear();
        self.requested = None;
        self.refresh_at = None;
        // Native Cards clone text/frame ownership. Clear them as well as the
        // scene/provider before admitting a fresh 4MiB/32MiB content set.
        self.renderer.release_surfaces();
    }
    fn cancel_pending(&mut self) {
        if let Some(pending) = &mut self.pending {
            pending.ticket.cancel();
            pending.cancelled = true;
        }
    }
    fn poll(
        &mut self,
        gate: &HealthGate,
        now: LocalDateTime,
        control: &SessionControl,
    ) -> Result<bool, String> {
        let completion = match &mut self.pending {
            Some(p) => match p.ticket.try_result() {
                Ok(Some(result)) => Some(Some(result)),
                Ok(None) => None,
                Err(_) => Some(None),
            },
            None => None,
        };
        let Some(completion) = completion else {
            return Ok(false);
        };
        let pending = self
            .pending
            .take()
            .expect("completion requires pending facts job");
        self.next_fetch = Instant::now() + RETRY;
        if pending.cancelled || self.stopped {
            return Ok(false);
        }
        let Some(completion) = completion else {
            self.invalidate();
            return Ok(false);
        };
        let (facts, text) = match completion {
            Ok(Outcome::Desktop(facts, text)) => (facts, text),
            Err(error) => {
                self.invalidate();
                if fatal(&error) {
                    control.fault();
                    return Err("桌面数据权威不可用".into());
                }
                return Ok(false);
            }
            Ok(_) => {
                self.invalidate();
                return Err("桌面后台结果类型不匹配".into());
            }
        };
        let status = gate.status();
        let navigation_current = pending.navigation == Navigation::Refresh
            || (pending.source == facts.global_revision
                && self.scene.as_ref().map(|s| s.generation) == pending.generation);
        if status.health != Health::Healthy
            || status.revision != facts.global_revision
            || facts.captured_at.date != now.date
            || facts.captured_at > now
            || next_context_boundary(&facts).is_some_and(|end| now >= end)
            || !material_page_matches(&facts, pending.material_cursor.as_ref())
            || facts.validate().is_err()
            || !text.complete_for(&facts.profile_contents)
            || !navigation_current
            || facts.homework_offset != pending.offset
        {
            self.invalidate();
            return Ok(false);
        }
        if !self
            .history
            .accept(pending.navigation, facts.homework_offset)
        {
            self.invalidate();
            return Ok(false);
        }
        self.refresh_at = next_context_boundary(&facts);
        self.facts = Some(facts);
        self.text = text;
        self.requested = None;
        Ok(true)
    }
    fn poll_content_jobs(
        &mut self,
        gate: &HealthGate,
        now: LocalDateTime,
        control: &SessionControl,
    ) -> Result<(), String> {
        let completion = self
            .images
            .as_mut()
            .map(|pending| pending.ticket.try_result());
        if !matches!(completion, None | Some(Ok(None))) {
            let pending = self.images.take().expect("image result requires ticket");
            self.next_images = Instant::now() + RETRY;
            match completion.expect("checked image completion") {
                Ok(Some(Ok(Outcome::ContentImages(result))))
                    if !pending.cancelled
                        && !self.stopped
                        && self.scene.as_ref().is_some_and(|scene| {
                            content_scene_current(
                                scene,
                                pending.generation,
                                pending.source,
                                now,
                                gate,
                            )
                        })
                        && result.generation == pending.generation
                        && result.source_revision == pending.source =>
                {
                    let scene = self.scene.as_ref().expect("matching scene");
                    let requests =
                        DesktopRenderer::image_requests(scene).map_err(|_| "桌面图片描述无效。")?;
                    let exact = requests.len() == result.frames.len()
                        && requests.iter().zip(&result.frames).all(|(request, frame)| {
                            let key = frame.key();
                            frame.source() == &request.reference
                                && fc_windows::materials::raster::contained_pixels(
                                    request.reference.width,
                                    request.reference.height,
                                    request.max_pixel_width,
                                    request.max_pixel_height,
                                )
                                .is_ok_and(|(width, height)| {
                                    key.width == width && key.height == height
                                })
                        });
                    if exact {
                        self.images_verified = Some((pending.generation, pending.source));
                        self.ready_images = result.frames;
                    } else {
                        self.render_unavailable("桌面图片结果与原场景不匹配，已隐藏并重新准备。");
                    }
                }
                Ok(Some(Err(error))) if !pending.cancelled => {
                    if fatal(&error) {
                        control.fault();
                        return Err("桌面图片权威不可用".into());
                    }
                    self.render_unavailable("桌面图片尚未准备完成，已隐藏并稍后重试。");
                }
                _ => (),
            }
        }
        let completion = self
            .page
            .as_mut()
            .map(|pending| pending.ticket.try_result());
        if !matches!(completion, None | Some(Ok(None))) {
            let pending = self.page.take().expect("page result requires ticket");
            let completion = completion.expect("checked page completion");
            let valid = match &completion {
                Ok(Some(Ok(Outcome::ContentPage {
                    generation,
                    snapshot,
                }))) => {
                    *generation == pending.generation
                        && snapshot.slot == pending.request.slot
                        && snapshot.revision == pending.request.expected_content_revision
                        && snapshot.view_revision > pending.request.expected_view_revision
                        && snapshot.view_revision > pending.source
                        && snapshot.current_page == pending.request.page
                        && snapshot.validate().is_ok()
                }
                _ => false,
            };
            // A page may have committed even if cancellation/deadline/Unknown
            // discarded its return value. Never apply a guessed local page;
            // always read a new coherent Facts+TextFrame from the authority.
            self.invalidate();
            self.cancel_pending();
            self.next_fetch = Instant::now();
            if !valid && !pending.cancelled {
                if let Ok(Some(Err(error))) = completion {
                    if fatal(&error) {
                        control.fault();
                        return Err("桌面翻页权威不可用".into());
                    }
                }
            }
        }
        Ok(())
    }
    fn prepare_images(
        &mut self,
        backend: &Arc<Mutex<Backend>>,
        gate: &HealthGate,
        pool: &WorkerPool<Work>,
        now: LocalDateTime,
    ) -> Result<bool, String> {
        let Some(scene) = &self.scene else {
            return Ok(false);
        };
        if !content_scene_current(scene, self.generation, scene.source_revision, now, gate) {
            return Ok(false);
        }
        let requests = DesktopRenderer::image_requests(scene).map_err(|_| "桌面图片描述无效。")?;
        if requests.is_empty() {
            return Ok(true);
        }
        if self.images_verified == Some((scene.generation, scene.source_revision)) {
            return Ok(true);
        }
        if self.images.is_some() || Instant::now() < self.next_images {
            return Ok(false);
        }
        let root = self.root.clone().ok_or("桌面图片数据目录未配置。")?;
        let generation = scene.generation;
        let source_revision = scene.source_revision;
        let job = profile_content_flow::ImageJob {
            root,
            backend: backend.clone(),
            generation,
            source_revision,
            captured_at: scene.captured_at,
            valid_until: scene.valid_until,
            requests,
            cache: self.raster.clone(),
        };
        let deadline = Instant::now() + IMAGES_DEADLINE;
        if let Ok(ticket) = pool
            .handle()
            .try_submit(Work::ContentImages(Box::new(job)), deadline)
        {
            self.images = Some(PendingImages {
                deadline,
                ticket,
                source: source_revision,
                generation,
                cancelled: false,
            });
        }
        self.next_images = Instant::now() + RETRY;
        Ok(false)
    }
    fn submit_page(
        &mut self,
        backend: &Arc<Mutex<Backend>>,
        gate: &HealthGate,
        pool: &WorkerPool<Work>,
        event: &DesktopEvent,
        now: LocalDateTime,
    ) -> Result<(), String> {
        let Some(scene) = &self.scene else {
            return Ok(());
        };
        let DesktopEvent::ImagePage {
            generation,
            slot,
            content_revision,
            view_revision,
            page,
        } = event
        else {
            return Ok(());
        };
        if self.page.is_some()
            || !image_page_matches(scene, event)
            || self.published_content != Some(scene.generation)
            || !content_scene_current(scene, *generation, scene.source_revision, now, gate)
        {
            return Ok(());
        }
        let request = PageContentRequest {
            command_id: match super::command_id() {
                Ok(id) => id,
                Err(_) => {
                    self.notice.failed("无法生成翻页操作标识，请稍后重试。");
                    return Ok(());
                }
            },
            slot: *slot,
            expected_content_revision: *content_revision,
            expected_view_revision: *view_revision,
            page: *page,
        };
        let source = scene.source_revision;
        let deadline = Instant::now() + FACTS_DEADLINE;
        if let Ok(ticket) = pool.handle().try_submit(
            Work::ContentPage {
                backend: backend.clone(),
                generation: *generation,
                source_revision: source,
                request: request.clone(),
            },
            deadline,
        ) {
            self.page = Some(PendingPage {
                deadline,
                ticket,
                source,
                generation: *generation,
                request,
                cancelled: false,
            });
        }
        Ok(())
    }
    fn submit(
        &mut self,
        backend: &Arc<Mutex<Backend>>,
        gate: &HealthGate,
        pool: &WorkerPool<Work>,
    ) -> Result<(), String> {
        if self.pending.is_some() || self.stopped || Instant::now() < self.next_fetch {
            return Ok(());
        }
        let Ok(status) = gate.check_sensitive() else {
            return Ok(());
        };
        let request = self.requested.clone().unwrap_or(Request {
            offset: 0,
            navigation: Navigation::Refresh,
            material_cursor: None,
        });
        let Request {
            offset,
            navigation,
            material_cursor,
        } = request;
        let Ok(index) = usize::try_from(offset) else {
            self.invalidate();
            return Err("桌面分页超出支持范围".into());
        };
        match pool.handle().try_submit(
            Work::Desktop(backend.clone(), index, material_cursor.clone()),
            Instant::now() + FACTS_DEADLINE,
        ) {
            Ok(ticket) => {
                self.pending = Some(Pending {
                    ticket,
                    source: status.revision,
                    generation: self.scene.as_ref().map(|s| s.generation),
                    offset,
                    navigation,
                    material_cursor,
                    cancelled: false,
                });
                self.next_fetch = Instant::now() + RETRY;
            }
            Err(_) => {
                self.next_fetch = Instant::now() + RETRY;
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn tick(
        &mut self,
        backend: Option<&Arc<Mutex<Backend>>>,
        control: Option<&SessionControl>,
        pool: &WorkerPool<Work>,
        clock: &CurrentClock,
        settings_open: bool,
        management_busy: bool,
        hints: HostHints,
    ) -> Result<(), String> {
        if self.stopped {
            return Ok(());
        }
        if let Some(control) = control {
            self.control = Some(control.clone());
        }
        self.expire_content_jobs();
        if hints.contains(HostHints::SHUTDOWN) {
            self.stop();
            return Ok(());
        }
        if hints.contains(HostHints::SESSION_LOCKED)
            || hints.contains(HostHints::SESSION_CHANGED)
            || hints.contains(HostHints::POWER_CHANGED)
        {
            self.hide();
            self.cancel_pending();
            return Ok(());
        }
        let (Some(backend), Some(control)) = (backend, control) else {
            self.hide();
            self.cancel_pending();
            return Ok(());
        };
        let gate = control.health_gate();
        let status = gate.status();
        if status.health != Health::Healthy {
            if status.health == Health::AuthorityUpdating
                && self.page.as_ref().is_some_and(|page| !page.cancelled)
            {
                // Keep the exact page writer's capability during its own guarded
                // transaction; hide native input without cancelling that writer.
                self.renderer.hide();
                self.material_events.clear();
                self.lesson_event = None;
                self.probe_presented = false;
                return Ok(());
            }
            self.hide();
            if matches!(status.health, Health::Faulted | Health::Stopping) {
                self.cancel_pending();
                self.facts = None;
                self.text = TextFrame::default();
                self.scene = None;
            }
            return Ok(());
        }
        let now = match clock.try_now() {
            Ok(now) => now,
            Err(_) => {
                self.invalidate();
                self.cancel_pending();
                return Ok(());
            }
        };
        self.poll_content_jobs(&gate, now, control)?;
        if management_busy {
            self.cancel_pending();
        }
        let received = self.poll(&gate, now, control)?;
        let status = gate.status();
        if status.health != Health::Healthy {
            self.hide();
            return Ok(());
        }
        let stale = self.facts.as_ref().is_some_and(|f| {
            f.global_revision != status.revision
                || f.captured_at.date != now.date
                || !received
                    && (hints.contains(HostHints::TIME_CHANGED)
                        || self.last_sample.is_some_and(|previous| now < previous)
                        || self.refresh_at.is_some_and(|end| now >= end))
        });
        if stale {
            self.invalidate();
            self.cancel_pending();
        }
        if settings_open {
            self.hide();
            return Ok(());
        }
        if self.facts.is_none() {
            self.hide();
            if !management_busy {
                self.submit(backend, &gate, pool)?;
            }
            self.last_sample = Some(now);
            return Ok(());
        }
        if Instant::now() < self.next_render
            && !hints.contains(HostHints::DISPLAY_CHANGED)
            && !hints.contains(HostHints::TIME_CHANGED)
        {
            self.hide();
            return Ok(());
        }
        let work = match self.renderer.work_area() {
            Ok(work) => work,
            Err(_) => {
                self.render_unavailable("桌面工作区域不可用，已隐藏并稍后重试。");
                return Ok(());
            }
        };
        let recompose = self.needs_scene
            || received
            || self.work != Some(work)
            || time_requires_scene(self.scene.as_ref(), self.last_sample, now)
            || hints.contains(HostHints::TIME_CHANGED)
            || hints.contains(HostHints::DISPLAY_CHANGED);
        if recompose {
            self.hide();
            let generation = match self.generation.next() {
                Ok(generation) => generation,
                Err(_) => {
                    self.render_unavailable("桌面场景代次已耗尽，请重启程序。");
                    self.stop();
                    return Ok(());
                }
            };
            let facts = self.facts.as_mut().expect("facts checked above");
            // Clock-only/work-area changes reuse this coherent snapshot. All lesson
            // boundaries and wall-clock changes fetched fresh material facts above.
            facts.captured_at = now;
            let scene = match build_scene_with_content(
                facts,
                generation,
                work,
                &self.measure,
                &self.text,
            ) {
                Ok(scene) => scene,
                Err(_) => {
                    self.render_unavailable(
                        "桌面布局暂不可用；请调整组件尺寸或显示区域，管理功能仍可使用。",
                    );
                    return Ok(());
                }
            };
            self.generation = generation;
            self.scene = Some(scene);
            self.work = Some(work);
            self.needs_scene = false;
        }
        self.last_sample = Some(now);
        match self.prepare_images(backend, &gate, pool, now) {
            Ok(true) => (),
            Ok(false) => {
                self.renderer.hide();
                self.clear_content_capability();
                self.probe_presented = false;
                return Ok(());
            }
            Err(message) => {
                self.render_unavailable(&message);
                return Ok(());
            }
        }
        let scene = self.scene.as_ref().expect("composed scene");
        let Ok(lease) =
            gate.lease_for_authorized_scope(DESKTOP_SCOPE, Instant::now() + DISPLAY_LEASE)
        else {
            self.hide();
            return Ok(());
        };
        let guard = PublicationGuard {
            gate: &gate,
            lease,
            current_generation: self.generation,
            settings_open,
        };
        let outcome = match self.renderer.present(scene, now, &guard) {
            Ok(outcome) => outcome,
            Err(error) => {
                if fc_windows::desktop::is_read_only_capacity_error(&error) {
                    self.render_unavailable("课表、时钟或日期内容无法在当前尺寸完整显示；请由管理员调整布局或显示区域。桌面只读内容不提供点击翻页。");
                } else {
                    self.render_unavailable("桌面图形发布暂不可用，已隐藏并稍后重试。");
                }
                return Ok(());
            }
        };
        self.rendered(outcome);
        if outcome == PresentOutcome::ContentPending {
            return Ok(());
        }
        let events = match self.renderer.take_events() {
            Ok(events) => events,
            Err(_) => {
                self.render_unavailable("桌面资料与分页输入暂不可用，已隐藏并稍后重试。");
                return Ok(());
            }
        };
        let scene = self.scene.as_ref().expect("composed scene");
        for event in events {
            match &event {
                DesktopEvent::ImagePage { .. } => {
                    if !management_busy
                        && self.pending.is_none()
                        && self.requested.is_none()
                        && self.page.is_none()
                        && image_page_matches(scene, &event)
                    {
                        self.submit_page(backend, &gate, pool, &event, now)?;
                        break;
                    }
                }
                DesktopEvent::LessonManage { generation } => {
                    if *generation == scene.generation
                        && scene.valid_at(now)
                        && scene.lesson_control.as_ref().is_some_and(|c| c.can_change)
                    {
                        self.lesson_event = self.lesson_stamp();
                    }
                }
                DesktopEvent::MaterialOpen { .. } => {
                    if material_event_matches(scene, &event)
                        && self.control.as_ref().is_some_and(|control| match &event {
                            DesktopEvent::MaterialOpen {
                                source, generation, ..
                            } => control.material_open_guard(
                                source,
                                *generation,
                                scene.source_revision,
                            ),
                            _ => false,
                        })
                        && self.material_events.len() < 8
                    {
                        self.material_events.push(event);
                    }
                }
                DesktopEvent::MaterialPage { cursor, .. } => {
                    if self.pending.is_none()
                        && self.requested.is_none()
                        && material_event_matches(scene, &event)
                    {
                        self.requested = Some(Request {
                            offset: self.facts.as_ref().map_or(0, |f| f.homework_offset),
                            navigation: Navigation::Material,
                            material_cursor: Some(cursor.clone()),
                        });
                        self.next_fetch = Instant::now();
                    }
                }
                DesktopEvent::HomeworkBatch { .. } => {
                    if self.pending.is_none() && self.requested.is_none() {
                        if let Some((offset, navigation)) = self.history.target(scene, &event) {
                            self.requested = Some(Request {
                                offset,
                                navigation,
                                material_cursor: self
                                    .facts
                                    .as_ref()
                                    .and_then(|f| current_material_cursor(f)),
                            });
                            self.next_fetch = Instant::now();
                        }
                    }
                }
            }
        }
        if self.requested.is_some() && !management_busy {
            self.submit(backend, &gate, pool)?;
        }
        Ok(())
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(second: u32) -> LocalDateTime {
        LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 30).unwrap(),
            second,
        )
        .unwrap()
    }
    fn scene() -> SceneSnapshot {
        SceneSnapshot {
            generation: SceneGeneration::new(1),
            source_revision: Revision::new(5),
            captured_at: sample(0),
            valid_until: Some(sample(60)),
            cards: Vec::new(),
            paint_tree: None,
            text_tone: fc_domain::desktop::TextTone::Light,
            clock_rollback: false,
            public_calendar_unverified: false,
            lesson_control: None,
        }
    }
    #[test]
    fn normal_seconds_reuse_scene_but_boundary_date_and_rollback_require_recomposition() {
        let scene = scene();
        assert!(!time_requires_scene(
            Some(&scene),
            Some(sample(1)),
            sample(2)
        ));
        assert!(!time_requires_scene(
            Some(&scene),
            Some(sample(58)),
            sample(59)
        ));
        assert!(time_requires_scene(
            Some(&scene),
            Some(sample(59)),
            sample(60)
        ));
        assert!(time_requires_scene(
            Some(&scene),
            Some(sample(25)),
            sample(20)
        ));
        let tomorrow =
            LocalDateTime::new(fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(), 0).unwrap();
        assert!(time_requires_scene(
            Some(&scene),
            Some(sample(59)),
            tomorrow
        ));
        assert!(time_requires_scene(None, Some(sample(1)), sample(2)));
    }
    #[test]
    fn bounded_page_history_reverses_actual_variable_batch_starts_without_skip() {
        let mut history = BatchHistory::default();
        assert!(history.accept(Navigation::Refresh, 0));
        assert!(history.accept(Navigation::Forward { from: 0 }, 29));
        assert!(history.accept(Navigation::Forward { from: 29 }, 57));
        assert!(history.accept(Navigation::Forward { from: 57 }, 71));
        // Native's offset-64 hint would be 7; the stored previous start is 57.
        assert_eq!(history.previous.last(), Some(&57));
        let mut scene = scene();
        scene.cards.push(fc_presentation::scene::SceneCard {
            content_identity: None,
            kind: ComponentKind::Homework,
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 200.,
                height: 100.,
            },
            font_dip: 22.,
            component_alpha: 1.,
            surface_alpha: 0.03,
            edge_alpha: 0.5,
            content: fc_presentation::scene::CardContent::Text(String::new()),
            visible_leaf_limit: 14,
            page: fc_presentation::scene::PageInfo {
                offset: 71,
                shown: 14,
                total: 100,
            },
        });
        assert_eq!(
            history.target(
                &scene,
                &DesktopEvent::HomeworkBatch {
                    generation: scene.generation,
                    offset: 7
                }
            ),
            Some((
                57,
                Navigation::Backward {
                    from: 71,
                    target: 57
                }
            ))
        );
        assert_eq!(
            history.target(
                &scene,
                &DesktopEvent::HomeworkBatch {
                    generation: scene.generation,
                    offset: 85
                }
            ),
            Some((85, Navigation::Forward { from: 71 }))
        );
        assert!(history
            .target(
                &scene,
                &DesktopEvent::HomeworkBatch {
                    generation: SceneGeneration::new(2),
                    offset: 7
                }
            )
            .is_none());
        assert!(history
            .target(
                &scene,
                &DesktopEvent::HomeworkBatch {
                    generation: scene.generation,
                    offset: 86
                }
            )
            .is_none());
        assert!(!history.accept(
            Navigation::Backward {
                from: 71,
                target: 7
            },
            7
        ));
        assert!(history.accept(
            Navigation::Backward {
                from: 71,
                target: 57
            },
            57
        ));
        assert!(history.accept(
            Navigation::Backward {
                from: 57,
                target: 29
            },
            29
        ));
        assert!(history.accept(
            Navigation::Backward {
                from: 29,
                target: 0
            },
            0
        ));
        assert!(history.previous.is_empty());
        assert!(history.accept(Navigation::Refresh, 0));
        for i in 0..HISTORY_LIMIT {
            assert!(history.accept(Navigation::Forward { from: i as u64 }, i as u64 + 1));
        }
        assert!(!history.accept(
            Navigation::Forward {
                from: HISTORY_LIMIT as u64
            },
            HISTORY_LIMIT as u64 + 1
        ));
        assert_eq!(history.previous.len(), HISTORY_LIMIT);
    }
    fn material_scene() -> SceneSnapshot {
        use fc_domain::{ids::*, materials::*, schedule::*, time::*};
        let mut scene = scene();
        let source = MaterialSource {
            kind: MaterialSourceKind::CurrentLesson,
            lesson: LessonInstanceKey {
                actual_date: sample(0).date,
                class_id: ClassId::new(1).unwrap(),
                version: ResolvedScheduleVersion {
                    actual_semester_id: SemesterId::new(1).unwrap(),
                    actual_revision: Revision::new(1),
                    source_semester_id: SemesterId::new(1).unwrap(),
                    source_revision: Revision::new(1),
                    calendar_mode: CalendarMode::School,
                    calendar_mode_revision: Revision::new(1),
                    applied_calendar_revision: Revision::new(1),
                    makeup_mapping_revision: Revision::new(1),
                    source_table_id: TableId::new(1).unwrap(),
                    source_row_id: RowId::new(1).unwrap(),
                    rules_revision: Revision::new(1),
                },
                range: MinuteRange::new(Minute::new(0).unwrap(), Minute::new(60).unwrap()).unwrap(),
            },
            subject_id: SubjectId::new(1).unwrap(),
            teacher_id: PrincipalId::new(2).unwrap(),
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
        scene.cards.push(fc_presentation::scene::SceneCard {
            content_identity: None,
            kind: ComponentKind::Materials,
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 200.,
                height: 100.,
            },
            font_dip: 22.,
            component_alpha: 1.,
            surface_alpha: 0.03,
            edge_alpha: 0.5,
            content: CardContent::Materials(Box::new(
                fc_presentation::scene::MaterialListContent {
                    source,
                    folder_id: Some(folder_id),
                    folder_revision,
                    teacher_name: "老师".into(),
                    subject_name: "语文".into(),
                    rows: vec![fc_presentation::scene::SceneMaterialRow {
                        material_id: MaterialId::new(5).unwrap(),
                        resource_version: ResourceVersionId::new(55).unwrap(),
                        name: "资料.txt".into(),
                    }],
                    next_cursor: Some(cursor),
                    total: 6,
                    visible_rows: 5,
                    row_gap_dip: 8,
                },
            )),
            page: fc_presentation::scene::PageInfo {
                offset: 0,
                shown: 1,
                total: 6,
            },
            visible_leaf_limit: 5,
        });
        scene
    }
    #[test]
    fn material_events_require_exact_published_generation_source_and_fixed_resource() {
        use fc_domain::ids::*;
        let scene = material_scene();
        let CardContent::Materials(content) = &scene.cards[0].content else {
            panic!()
        };
        let mut event = DesktopEvent::MaterialOpen {
            generation: scene.generation,
            source: content.source.clone(),
            folder_id: content.folder_id.unwrap(),
            folder_revision: content.folder_revision,
            material_id: content.rows[0].material_id,
            resource_version: content.rows[0].resource_version,
        };
        assert!(material_event_matches(&scene, &event));
        if let DesktopEvent::MaterialOpen {
            resource_version, ..
        } = &mut event
        {
            *resource_version = ResourceVersionId::new(56).unwrap();
        }
        assert!(!material_event_matches(&scene, &event));
        if let DesktopEvent::MaterialOpen {
            resource_version,
            generation,
            ..
        } = &mut event
        {
            *resource_version = ResourceVersionId::new(55).unwrap();
            *generation = SceneGeneration::new(2);
        }
        assert!(!material_event_matches(&scene, &event));
        if let DesktopEvent::MaterialOpen {
            generation, source, ..
        } = &mut event
        {
            *generation = scene.generation;
            source.teacher_id = PrincipalId::new(3).unwrap();
        }
        assert!(!material_event_matches(&scene, &event));
        let mut page = DesktopEvent::MaterialPage {
            generation: scene.generation,
            source: content.source.clone(),
            cursor: content.next_cursor.clone().unwrap(),
        };
        assert!(material_event_matches(&scene, &page));
        if let DesktopEvent::MaterialPage { cursor, .. } = &mut page {
            cursor.offset += 1;
        }
        assert!(!material_event_matches(&scene, &page));
    }
    #[test]
    fn material_navigation_preserves_variable_homework_page_history() {
        let mut history = BatchHistory::default();
        assert!(history.accept(Navigation::Refresh, 0));
        assert!(history.accept(Navigation::Forward { from: 0 }, 29));
        assert!(history.accept(Navigation::Material, 29));
        assert_eq!(history.previous, vec![0]);
        assert!(history.accept(
            Navigation::Backward {
                from: 29,
                target: 0
            },
            0
        ));
        assert!(history.previous.is_empty());
        // A material event can never masquerade as a homework batch.
        let scene = material_scene();
        let CardContent::Materials(content) = &scene.cards[0].content else {
            panic!()
        };
        assert!(history
            .target(
                &scene,
                &DesktopEvent::MaterialPage {
                    generation: scene.generation,
                    source: content.source.clone(),
                    cursor: content.next_cursor.clone().unwrap()
                }
            )
            .is_none());
    }
    #[test]
    fn recoverable_render_notice_is_reported_once_and_cleared_once_after_actual_success() {
        let mut notice = RenderNotice::default();
        notice.failed("layout configuration");
        assert_eq!(notice.take().as_deref(), Some("layout configuration"));
        assert!(notice.take().is_none());
        notice.rendered(PresentOutcome::Hidden);
        assert!(notice.take().is_none());
        notice.rendered(PresentOutcome::ContentPending);
        assert!(notice.take().is_none());
        notice.rendered(PresentOutcome::Offscreen);
        assert_eq!(notice.take(), Some(String::new()));
        assert!(notice.take().is_none());
        notice.rendered(PresentOutcome::Offscreen);
        assert!(notice.take().is_none());
    }
}

#[cfg(test)]
#[path = "desktop_content_tests.rs"]
mod desktop_content_tests;
