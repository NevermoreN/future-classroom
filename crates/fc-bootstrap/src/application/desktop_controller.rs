//! UI-thread resident desktop orchestration. SQLite runs only through the shared
//! Work pool; the UI never takes Backend's mutex or refreshes a manager lease.
use super::{fatal, Outcome, Work};
use fc_bootstrap::backend::{Backend, SessionControl};
use fc_domain::{
    desktop::{ComponentKind, DesktopFacts},
    ids::{Revision, SceneGeneration},
    materials::MaterialCursor,
    schedule::AcademicDay,
    time::{LocalDate, LocalDateTime},
};
use fc_presentation::{
    scene::{build_scene, CardContent, SceneSnapshot},
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
    system::CurrentClock,
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const RETRY: Duration = Duration::from_secs(1);
const FACTS_DEADLINE: Duration = Duration::from_secs(5);
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
        if self.failed && outcome != PresentOutcome::Hidden {
            self.message = Some(String::new());
            self.failed = false;
        }
    }
    fn take(&mut self) -> Option<String> {
        self.message.take()
    }
}
pub(super) struct Controller {
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
        let renderer = if probe {
            DesktopRenderer::new_hidden()
        } else {
            DesktopRenderer::new()
        }
        .map_err(|_| "桌面窗口初始化失败".to_owned())?;
        let measure = NativeTextMeasure::new().map_err(|_| "桌面文字测量初始化失败".to_owned())?;
        Ok(Self {
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
    pub(super) fn hide(&mut self) {
        if let Some(control) = &self.control {
            control.clear_material_scene();
        }
        self.material_events.clear();
        self.renderer.hide();
        self.probe_presented = false;
    }
    pub(super) fn stop(&mut self) {
        self.stopped = true;
        self.hide();
        if let Some(pending) = self.pending.take() {
            pending.ticket.cancel();
        }
        self.facts = None;
        self.scene = None;
        self.requested = None;
        self.history.clear();
    }
    pub(super) fn probe_ready(&self) -> bool {
        self.probe && self.probe_presented && !self.stopped
    }
    pub(super) fn probe_scene(&self) -> Option<&SceneSnapshot> {
        self.scene.as_ref()
    }
    pub(super) fn probe_metrics(&self) -> DesktopMetrics {
        self.renderer.diagnostics()
    }
    pub(super) fn take_material_events(&mut self) -> Vec<DesktopEvent> {
        std::mem::take(&mut self.material_events)
    }
    pub(super) fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }
    fn render_unavailable(&mut self, message: &str) {
        self.hide();
        self.scene = None;
        self.notice.failed(message);
        self.next_render = Instant::now() + RETRY;
    }
    fn rendered(&mut self, outcome: PresentOutcome) {
        let visible = matches!(
            outcome,
            PresentOutcome::Published | PresentOutcome::Unchanged
        ) || self.probe && outcome == PresentOutcome::Offscreen;
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
        self.probe_presented = outcome == PresentOutcome::Offscreen;
        self.notice.rendered(outcome);
    }
    fn invalidate(&mut self) {
        self.hide();
        self.facts = None;
        self.scene = None;
        self.history.clear();
        self.requested = None;
        self.refresh_at = None;
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
        let facts = match completion {
            Ok(Outcome::Desktop(facts)) => facts,
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
        self.requested = None;
        Ok(true)
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
            self.hide();
            if matches!(status.health, Health::Faulted | Health::Stopping) {
                self.cancel_pending();
                self.facts = None;
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
        let recompose = received
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
            let scene = match build_scene(facts, generation, work, &self.measure) {
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
        }
        self.last_sample = Some(now);
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
            Err(_) => {
                self.render_unavailable("桌面图形发布暂不可用，已隐藏并稍后重试。");
                return Ok(());
            }
        };
        self.rendered(outcome);
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
            text_tone: fc_domain::desktop::TextTone::Light,
            clock_rollback: false,
            public_calendar_unverified: false,
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
                },
            )),
            page: fc_presentation::scene::PageInfo {
                offset: 0,
                shown: 1,
                total: 6,
            },
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
        notice.rendered(PresentOutcome::Offscreen);
        assert_eq!(notice.take(), Some(String::new()));
        assert!(notice.take().is_none());
        notice.rendered(PresentOutcome::Offscreen);
        assert!(notice.take().is_none());
    }
}
