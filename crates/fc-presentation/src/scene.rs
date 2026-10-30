//! Pure, transaction-consistent desktop composition. Native visibility and health
//! checks remain mandatory immediately before every publication.
use crate::card_body::card_body_spec;
use crate::layout::{compose_layout, TreeLayoutError, WidgetMeasure};
pub use crate::layout::{
    PaintGlass, SceneNodeKey, ScenePaintNode, ScenePaintTree, SceneProfileStamp,
};
use crate::{ClockStyle, LayoutError, Rect};
use fc_domain::layout::{LayoutScope, LayoutTree, NodeKind, WidgetKind};
use fc_domain::profile_content::{ContentSlotKey, MediaReference, PublishedContentDescriptor};
use fc_domain::{
    content::ContentKey,
    desktop::*,
    identity::Rgb,
    ids::*,
    materials::{MaterialCursor, MaterialSource},
    schedule::{resolve_lesson, AcademicDay, LessonInstanceKey, LessonState, RowKind},
    time::LocalDateTime,
};
use fc_ports::TextMeasure;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageInfo {
    pub offset: u64,
    pub shown: u64,
    pub total: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockContent {
    pub mode: ClockMode,
    pub show_seconds: bool,
}
/// Fractions of a complete clockwise revolution, beginning at twelve o'clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockHands {
    pub hour: f32,
    pub minute: f32,
    pub second: Option<f32>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CourseCapsule {
    pub key: LessonInstanceKey,
    pub subject_id: SubjectId,
    pub name: String,
    pub color: Rgb,
    pub current: bool,
    pub temporary: bool,
    pub fill_alpha: f32,
    pub stroke_width: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct HomeworkEntry {
    pub subject_id: SubjectId,
    pub teacher_id: PrincipalId,
    pub subject_name: String,
    pub teacher_name: String,
    pub text: String,
    pub color: Rgb,
    pub fill_alpha: f32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneMaterialRow {
    pub material_id: MaterialId,
    pub resource_version: ResourceVersionId,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialListContent {
    pub source: MaterialSource,
    pub folder_id: Option<FolderId>,
    pub folder_revision: Revision,
    pub teacher_name: String,
    pub subject_name: String,
    pub rows: Vec<SceneMaterialRow>,
    pub next_cursor: Option<MaterialCursor>,
    pub total: u64,
    pub visible_rows: u8,
    pub row_gap_dip: u16,
}
/// Trusted cache boundary. The producer verifies SHA-256 against actual UTF-8
/// before insertion. Lookup must use the exact owner, revision and digest; no
/// fallback to another revision or another node with the same kind is permitted.
pub trait ContentTextSource {
    fn text(&self, slot: ContentSlotKey, revision: Revision, sha256: [u8; 32]) -> Option<Arc<str>>;
}
struct EmptyContentTextSource;
impl ContentTextSource for EmptyContentTextSource {
    fn text(&self, _: ContentSlotKey, _: Revision, _: [u8; 32]) -> Option<Arc<str>> {
        None
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneContentIdentity {
    pub slot: ContentSlotKey,
    pub revision: Revision,
    pub view_revision: Revision,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneImageContent {
    pub current: MediaReference,
    pub current_page: u8,
    pub total_pages: u8,
}
#[derive(Clone, Debug, PartialEq)]
pub enum CardContent {
    Clock(ClockContent),
    Text(String),
    StandaloneText(Arc<str>),
    Image(SceneImageContent),
    Courses(Vec<CourseCapsule>),
    Homework(Vec<HomeworkEntry>),
    Recess {
        deadline: LocalDateTime,
    },
    MaterialsUnavailable,
    Materials(Box<MaterialListContent>),
    /// A separate, small desktop surface. Only its explicit button is interactive.
    LessonControl {
        caption: String,
        can_change: bool,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct LessonControlContent {
    pub rect: Rect,
    pub caption: String,
    pub can_change: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SceneCard {
    pub content_identity: Option<SceneContentIdentity>,
    pub kind: ComponentKind,
    pub rect: Rect,
    pub font_dip: f32,
    pub component_alpha: f32,
    pub surface_alpha: f32,
    pub edge_alpha: f32,
    pub content: CardContent,
    pub page: PageInfo,
    /// Maximum actual data leaves on this card's native page; complete DTO data remains owned.
    pub visible_leaf_limit: u8,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SceneSnapshot {
    pub generation: SceneGeneration,
    pub source_revision: Revision,
    pub captured_at: LocalDateTime,
    pub valid_until: Option<LocalDateTime>,
    pub cards: Vec<SceneCard>,
    /// Production scenes always publish an authoritative composed tree.
    pub paint_tree: Option<ScenePaintTree>,
    pub text_tone: TextTone,
    pub clock_rollback: bool,
    pub public_calendar_unverified: bool,
    pub lesson_control: Option<LessonControlContent>,
}
impl SceneSnapshot {
    /// A date change, same-day rollback or reached lesson boundary invalidates the
    /// entire scene. Clock and recess ticks cannot authorize stale static content.
    pub fn valid_at(&self, now: LocalDateTime) -> bool {
        now.date == self.captured_at.date
            && now >= self.captured_at
            && self.valid_until.is_none_or(|end| now < end)
    }
    pub fn needs_second_tick(&self) -> bool {
        self.cards.iter().enumerate().any(|(index, c)| {
            let alpha = self
                .paint_tree
                .as_ref()
                .and_then(|t| t.nodes.iter().find(|n| n.card_index == Some(index)))
                .map_or(c.component_alpha, |n| n.effective_opacity);
            alpha > 0.
                && match c.content {
                    CardContent::Clock(clock) => clock.show_seconds,
                    CardContent::Recess { .. } => true,
                    _ => false,
                }
        })
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum SceneError {
    InvalidFacts(DisplayError),
    Layout(LayoutError),
    TreeLayout(TreeLayoutError),
    Schedule,
    Capacity,
    ContentPending(ContentSlotKey),
    InvalidContent(ContentSlotKey),
}
/// A neutral fallback is explicit; it never suggests a different teacher's profile.
pub fn profile_configuration_warning(
    reason: fc_domain::profile::ProfileSelectionReason,
) -> Option<&'static str> {
    use fc_domain::profile::ProfileSelectionReason::*;
    Some(match reason {
        Public | Selected => return None,
        Missing => "科目配置缺失 · 使用公共布局",
        Disabled => "科目配置已停用 · 使用公共布局",
        Unassigned => "课程未分配老师 · 使用公共布局",
        InvalidAssignment => "当前授课关系无效 · 使用公共布局",
        FolderMismatch => "资料目录与科目配置不匹配 · 使用公共布局",
        LayoutMismatch => "科目布局与公共布局不兼容 · 使用公共布局",
    })
}
pub fn clock_label(clock: ClockContent, now: LocalDateTime) -> String {
    crate::format_clock(
        now,
        match clock.mode {
            ClockMode::Digital24 => ClockStyle::Digital24,
            ClockMode::Digital12 | ClockMode::Analog => ClockStyle::Digital12,
        },
        clock.show_seconds,
    )
}
pub fn clock_hands(clock: ClockContent, now: LocalDateTime) -> ClockHands {
    let s = now.second_of_day();
    ClockHands {
        hour: ((s / 60) % 720) as f32 / 720.,
        minute: ((s / 60) % 60) as f32 / 60.,
        second: clock.show_seconds.then_some((s % 60) as f32 / 60.),
    }
}
pub fn recess_label(deadline: LocalDateTime, now: LocalDateTime) -> Option<String> {
    (deadline.date == now.date && now < deadline)
        .then(|| crate::countdown_label(deadline.second_of_day() - now.second_of_day()))
}

/// Shared RGB resolver; input RGB is never mutated and alpha is independent.
pub fn soft_subject_color(raw: Rgb) -> Rgb {
    let r = f32::from(raw.red) / 255.;
    let g = f32::from(raw.green) / 255.;
    let b = f32::from(raw.blue) / 255.;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let l = (max + min) / 2.;
    let (h, s) = if delta == 0. {
        (0., 0.)
    } else {
        let s = delta / (1. - (2. * l - 1.).abs());
        let h = if max == r {
            ((g - b) / delta).rem_euclid(6.)
        } else if max == g {
            (b - r) / delta + 2.
        } else {
            (r - g) / delta + 4.
        };
        (h, s.min(0.4))
    };
    let l = l.clamp(0.65, 0.82);
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
    let m = l - c / 2.;
    let (r, g, b) = match h as u8 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    Rgb {
        red: ((r + m) * 255.).round() as u8,
        green: ((g + m) * 255.).round() as u8,
        blue: ((b + m) * 255.).round() as u8,
    }
}

fn selected_widgets(tree: &LayoutTree, replace_outlet: bool) -> Vec<(SceneNodeKey, WidgetKind)> {
    fn walk(
        tree: &LayoutTree,
        id: LayoutNodeId,
        replace: bool,
        out: &mut Vec<(SceneNodeKey, WidgetKind)>,
    ) {
        let n = tree.node(id).expect("validated tree");
        if let NodeKind::Widget(kind) = n.kind {
            out.push((
                SceneNodeKey {
                    scope: tree.scope,
                    id,
                },
                kind,
            ));
        }
        if replace && matches!(n.kind, NodeKind::Outlet { .. }) {
            return;
        }
        for child in &n.children {
            walk(tree, *child, replace, out);
        }
    }
    let mut out = Vec::new();
    walk(tree, tree.root, replace_outlet, &mut out);
    out
}

fn card(
    settings: &DisplaySettings,
    k: ComponentKind,
    rect: Rect,
    content: CardContent,
    page: PageInfo,
    component: &ComponentSettings,
) -> Result<SceneCard, SceneError> {
    let c = component;
    let alpha = f32::from(c.opacity_percent) / 100.;
    Ok(SceneCard {
        content_identity: None,
        kind: k,
        rect,
        font_dip: f32::from(c.font_dip),
        component_alpha: alpha,
        surface_alpha: f32::from(settings.glass_surface_percent) / 100.,
        edge_alpha: f32::from(settings.edge_reflection_percent) / 100.,
        content,
        page,
        visible_leaf_limit: 1,
    })
}

pub fn build_scene(
    facts: &DesktopFacts,
    generation: SceneGeneration,
    work: Rect,
    measure: &dyn TextMeasure,
) -> Result<SceneSnapshot, SceneError> {
    // Compatibility entry point has no content-cache authority. Independent
    // content requires the explicit hydrated composition path, even for images
    // whose pixels will subsequently be admitted by the native cache adapter.
    facts.validate().map_err(SceneError::InvalidFacts)?;
    if let Some(snapshot) = facts
        .profile_contents
        .iter()
        .find(|s| !matches!(s.content, PublishedContentDescriptor::Empty))
    {
        return Err(SceneError::ContentPending(snapshot.slot));
    }
    build_scene_with_content(facts, generation, work, measure, &EmptyContentTextSource)
}

pub fn build_scene_with_content(
    facts: &DesktopFacts,
    generation: SceneGeneration,
    work: Rect,
    measure: &dyn TextMeasure,
    source: &dyn ContentTextSource,
) -> Result<SceneSnapshot, SceneError> {
    facts.validate().map_err(SceneError::InvalidFacts)?;
    let settings = &facts.display;
    let profile = facts.profiles.subject_profile.as_ref();
    let material_style = profile.map(|p| &p.materials);
    let component = |kind| {
        if kind == ComponentKind::Materials {
            material_style
                .map(|m| &m.component)
                .or_else(|| settings.component(kind))
        } else {
            settings.component(kind)
        }
    };
    let now = facts.captured_at;
    let identity = &facts.identity;
    let class = identity.installation.as_ref().and_then(|i| i.current_class);
    let day = if facts.clock_rollback {
        AcademicDay::Unconfirmed
    } else if let Some(class) = class {
        facts
            .schedule
            .resolve_day(class, now.date)
            .map_err(|_| SceneError::Schedule)?
    } else {
        AcademicDay::Unconfirmed
    };
    let public_unverified =
        matches!(&day, AcademicDay::Scheduled(day) if day.public_data_unverified);
    let lesson = resolve_lesson(&day, now, facts.schedule.automatic_gaps);
    let mut valid_until = None;
    let mut course_rows = Vec::new();
    if let AcademicDay::Scheduled(day) = &day {
        let weekday = day.source_date.weekday();
        let mut rows: Vec<_> = day
            .table
            .rows
            .iter()
            .filter(|r| r.enabled_days[weekday.index()])
            .collect();
        rows.sort_by_key(|r| r.range.begin);
        for row in rows {
            for boundary in [row.range.begin, row.range.end] {
                let second = u32::from(boundary.get()) * 60;
                if second > now.second_of_day() {
                    let end =
                        LocalDateTime::new(now.date, second).map_err(|_| SceneError::Schedule)?;
                    if valid_until.is_none_or(|old| end < old) {
                        valid_until = Some(end)
                    }
                }
            }
            if row.kind != RowKind::Course {
                continue;
            }
            let Some(cell) = &row.courses[weekday.index()] else {
                continue;
            };
            let key = day.lesson_key(row);
            let current = matches!(&lesson,LessonState::Course{key:active,..}if *active==key);
            let effective = facts
                .lesson_context
                .current
                .as_ref()
                .filter(|c| current && c.key == key);
            let subject_id = effective.map_or(cell.subject_id, |c| c.effective.subject_id);
            let subject = identity
                .subjects
                .iter()
                .find(|s| s.id == subject_id && s.enabled)
                .ok_or(SceneError::Schedule)?;
            let temporary = current
                && facts
                    .lesson_context
                    .active_override
                    .as_ref()
                    .is_some_and(|o| o.key == key);
            let name = if temporary {
                let original = identity
                    .subjects
                    .iter()
                    .find(|s| s.id == cell.subject_id)
                    .ok_or(SceneError::Schedule)?;
                format!("临时：{} · 原{}", subject.name, original.name)
            } else {
                subject.name.clone()
            };
            course_rows.push(CourseCapsule {
                key,
                subject_id: subject.id,
                name,
                color: soft_subject_color(subject.color),
                current,
                temporary,
                fill_alpha: if settings.subject_fill {
                    f32::from(settings.course_fill_percent) / 100.
                } else {
                    0.
                },
                stroke_width: if settings.subject_fill { 0. } else { 1. },
            });
        }
    }
    let schedule_content = if course_rows.is_empty() {
        CardContent::Text(
            match &day {
                AcademicDay::Rest => "今日休息",
                AcademicDay::OutOfTerm => "当前无有效学期",
                AcademicDay::Unconfirmed => "今日安排未确认",
                AcademicDay::Scheduled(day) if day.public_data_unverified => {
                    "调休资料未核验 · 今日无课"
                }
                _ => "今日无课",
            }
            .into(),
        )
    } else {
        CardContent::Courses(course_rows)
    };
    let mut homework = Vec::with_capacity(facts.homework.len());
    for r in &facts.homework {
        let ContentKey::Homework(k) = &r.key else {
            return Err(SceneError::Schedule);
        };
        let subject = identity
            .subjects
            .iter()
            .find(|s| s.id == k.subject_id && s.enabled)
            .ok_or(SceneError::Schedule)?;
        let teacher = identity
            .accounts
            .iter()
            .find(|a| a.principal.id == k.owner_teacher_id)
            .ok_or(SceneError::Schedule)?;
        homework.push(HomeworkEntry {
            subject_id: subject.id,
            teacher_id: teacher.principal.id,
            subject_name: subject.name.clone(),
            teacher_name: teacher.display_name.clone(),
            text: r.text.clone(),
            color: soft_subject_color(subject.color),
            fill_alpha: if settings.subject_fill {
                f32::from(settings.homework_fill_percent) / 100.
            } else {
                0.
            },
        })
    }
    let material_content = if let Some(source) = &facts.materials_source {
        let teacher = identity
            .accounts
            .iter()
            .find(|a| a.principal.id == source.teacher_id)
            .ok_or(SceneError::Schedule)?;
        let subject = identity
            .subjects
            .iter()
            .find(|s| s.id == source.subject_id && s.enabled)
            .ok_or(SceneError::Schedule)?;
        CardContent::Materials(Box::new(MaterialListContent {
            source: source.clone(),
            folder_id: facts.materials_page.as_ref().map(|p| p.folder.id),
            folder_revision: facts
                .materials_page
                .as_ref()
                .map_or(Revision::default(), |p| p.folder.revision),
            teacher_name: teacher.display_name.clone(),
            subject_name: subject.name.clone(),
            rows: facts.materials_page.as_ref().map_or_else(Vec::new, |p| {
                p.items
                    .iter()
                    .map(|r| SceneMaterialRow {
                        material_id: r.id,
                        resource_version: r.resource.version_id,
                        name: r.display_name.clone(),
                    })
                    .collect()
            }),
            next_cursor: facts
                .materials_page
                .as_ref()
                .and_then(|p| p.next_cursor.clone()),
            total: facts.materials_page.as_ref().map_or(0, |p| p.total),
            visible_rows: material_style.map_or(5, |m| m.visible_rows),
            row_gap_dip: material_style.map_or(8, |m| m.row_gap_dip),
        }))
    } else {
        CardContent::MaterialsUnavailable
    };
    let mut entries = vec![
        (
            ComponentKind::Clock,
            CardContent::Clock(ClockContent {
                mode: settings.clock_mode,
                show_seconds: settings.clock_seconds,
            }),
        ),
        (
            ComponentKind::Date,
            CardContent::Text(format!(
                "{}年{}月{}日",
                now.date.year(),
                now.date.month(),
                now.date.day()
            )),
        ),
        (ComponentKind::Schedule, schedule_content),
        (
            ComponentKind::Homework,
            if homework.is_empty() {
                CardContent::Text("今日暂无已发布作业".into())
            } else {
                CardContent::Homework(homework)
            },
        ),
        (ComponentKind::Materials, material_content),
    ];
    if let Some(reminder) = &facts.reminder {
        entries.push((
            ComponentKind::Reminder,
            CardContent::Text(reminder.text.clone()),
        ))
    }
    if let Some(target) = settings.countdown.target {
        let days = target.days_since(now.date);
        if days >= 0 || settings.countdown.expired == CountdownExpired::ShowCompleted {
            entries.push((
                ComponentKind::DateCountdown,
                CardContent::Text(if days < 0 {
                    format!("{} · 已结束", settings.countdown.label)
                } else {
                    format!("{} · {days}天", settings.countdown.label)
                }),
            ))
        }
    }
    entries.retain(|(k, _)| component(*k).is_some_and(|c| c.visible));
    let recess = if settings.recess_enabled
        && settings
            .component(ComponentKind::Recess)
            .is_some_and(|c| c.visible)
    {
        match &lesson {
            LessonState::Recess { deadline, .. } if *deadline > now => Some(*deadline),
            _ => None,
        }
    } else {
        None
    };
    let lesson_control = if class.is_some() {
        let current = facts
            .lesson_context
            .current
            .as_ref()
            .filter(|_| !facts.clock_rollback);
        let caption = if let Some(context) = current {
            let subject = identity
                .subjects
                .iter()
                .find(|s| s.id == context.effective.subject_id && s.enabled)
                .ok_or(SceneError::Schedule)?;
            let teacher = context
                .teacher_id
                .and_then(|id| identity.accounts.iter().find(|a| a.principal.id == id))
                .map_or("未分配老师", |a| a.display_name.as_str());
            if facts.lesson_context.active_override.is_some() {
                let base = identity
                    .subjects
                    .iter()
                    .find(|s| s.id == context.base.subject_id)
                    .ok_or(SceneError::Schedule)?;
                format!(
                    "临时：{} · {} · 原{} · 仅本节有效",
                    subject.name, teacher, base.name
                )
            } else {
                format!("当前：{} · {}", subject.name, teacher)
            }
        } else {
            match lesson {
                LessonState::Recess { .. } => "当前课间，不能临时换课",
                LessonState::Course { .. } => "课堂上下文待确认，不能临时换课",
                _ => "当前没有正在进行的课程，不能临时换课",
            }
            .into()
        };
        let can_change = current.is_some() && !facts.clock_rollback;
        let width = work.width.min(1000.);
        let text_width = width - if can_change { 180. } else { 32. };
        if text_width < 80. {
            return Err(SceneError::Layout(LayoutError::OutOfSpace));
        }
        let height = card_body_spec(
            ComponentKind::Schedule,
            &CardContent::LessonControl {
                caption: caption.clone(),
                can_change,
            },
            false,
            None,
        )
        .and_then(|body| body.intrinsic_height(width, 16., measure))
        .map_err(|_| SceneError::Capacity)?;
        // Preserve the existing 384-DIP measured-caption admission limit.
        if !(56. ..=408.).contains(&height) {
            return Err(SceneError::Capacity);
        }
        Some(LessonControlContent {
            rect: Rect {
                x: work.x,
                y: work.y,
                width,
                height: height.ceil(),
            },
            caption,
            can_change,
        })
    } else {
        None
    };
    let work = if let Some(control) = &lesson_control {
        let height = work.height - control.rect.height - 12.;
        if height < 84. {
            return Err(SceneError::Layout(LayoutError::OutOfSpace));
        }
        Rect {
            y: work.y + control.rect.height + 12.,
            height,
            ..work
        }
    } else {
        work
    };
    if let Some(deadline) = recess {
        entries.push((ComponentKind::Recess, CardContent::Recess { deadline }));
    }
    // Resolve every active widget by its immutable scoped identity. Public
    // outlet fallback is never hydrated while an exact subject tree replaces it.
    let mut bound = Vec::new();
    let mut nodes = selected_widgets(&facts.profiles.public_layout.tree, profile.is_some());
    if let Some(profile) = profile {
        nodes.extend(selected_widgets(&profile.tree, false));
    }
    for (key, widget) in nodes {
        let kind = widget.component_kind().ok_or(SceneError::Capacity)?;
        let independent = matches!(widget, WidgetKind::SubjectText | WidgetKind::SubjectImage);
        let (style, content, identity) = if independent {
            let styles = match key.scope {
                LayoutScope::Public => &facts.profiles.public_layout.content_styles,
                LayoutScope::Subject(_) => &profile.ok_or(SceneError::Schedule)?.content_styles,
            };
            let style = styles
                .iter()
                .find(|s| s.node_id == key.id)
                .ok_or(SceneError::Schedule)?;
            if !style.component.visible {
                continue;
            }
            let slot = ContentSlotKey {
                scope: key.scope,
                node_id: key.id,
            };
            let snapshot = facts
                .profile_contents
                .iter()
                .find(|s| s.slot == slot)
                .ok_or(SceneError::InvalidContent(slot))?;
            let content = match &snapshot.content {
                PublishedContentDescriptor::Empty => continue,
                PublishedContentDescriptor::Text {
                    characters,
                    utf8_bytes,
                    sha256,
                } => {
                    let text = source
                        .text(slot, snapshot.revision, *sha256)
                        .ok_or(SceneError::ContentPending(slot))?;
                    if text.len() != *utf8_bytes as usize
                        || text.chars().count() != *characters as usize
                        || text.len() > 40_000
                        || text.chars().count() > 10_000
                        || text
                            .chars()
                            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                    {
                        return Err(SceneError::InvalidContent(slot));
                    }
                    CardContent::StandaloneText(text)
                }
                PublishedContentDescriptor::Image {
                    current,
                    total_pages,
                } => CardContent::Image(SceneImageContent {
                    current: current.clone(),
                    current_page: snapshot.current_page,
                    total_pages: *total_pages,
                }),
            };
            (
                style.component.clone(),
                content,
                Some(SceneContentIdentity {
                    slot,
                    revision: snapshot.revision,
                    view_revision: snapshot.view_revision,
                }),
            )
        } else {
            let Some((_, content)) = entries.iter().find(|(k, _)| *k == kind) else {
                continue;
            };
            (
                component(kind).ok_or(SceneError::Schedule)?.clone(),
                content.clone(),
                None,
            )
        };
        bound.push((key, widget, style, content, identity));
    }
    let specs: Vec<_> = bound.iter().map(|(key, widget, c, content, _)| {
        Ok(WidgetMeasure {
            key: *key, kind: *widget,
            body: card_body_spec(c.kind, content, public_unverified, if c.kind == ComponentKind::Materials { profile_configuration_warning(facts.profiles.reason) } else { None }).map_err(|_| SceneError::Capacity)?,
            font_dip: c.font_dip as f32, opacity: c.opacity_percent as f32 / 100.,
            min_width: c.min_width as f32, max_width: c.max_width as f32,
            min_height: (c.min_height as f32).max(if matches!(content, CardContent::Clock(clock) if clock.mode == ClockMode::Analog) {160.} else {84.}),
            max_height: c.max_height as f32,
            preferred_width: if c.kind == ComponentKind::Homework {480.} else {280.},
            glass: PaintGlass { surface_alpha: settings.glass_surface_percent as f32 / 100., edge_alpha: settings.edge_reflection_percent as f32 / 100. },
        })
    }).collect::<Result<_, SceneError>>()?;
    let mut paint_tree = compose_layout(
        &facts.profiles.public_layout.tree,
        profile.map(|p| &p.tree),
        &specs,
        work,
        measure,
    )
    .map_err(SceneError::TreeLayout)?;
    paint_tree.public_layout_revision = facts.profiles.public_layout.revision;
    paint_tree.selection_reason = facts.profiles.reason;
    paint_tree.subject_profile = profile.map(|p| SceneProfileStamp {
        id: p.id,
        key: p.key,
        revision: p.revision,
        layout_revision: p.layout_revision,
    });
    let mut cards = Vec::new();
    for node in &mut paint_tree.nodes {
        let Some(widget) = node.widget_kind else {
            continue;
        };
        let k = widget.component_kind().ok_or(SceneError::Capacity)?;
        let (_, _, style, content, identity) = bound
            .iter()
            .find(|(key, _, _, _, _)| *key == node.key)
            .ok_or(SceneError::Schedule)?;
        let content = content.clone();
        let page = if let CardContent::Image(image) = &content {
            PageInfo {
                offset: u64::from(image.current_page),
                shown: 1,
                total: u64::from(image.total_pages),
            }
        } else if k == ComponentKind::Homework {
            PageInfo {
                offset: facts.homework_offset,
                shown: facts.homework.len() as u64,
                total: facts.homework_eligible_total,
            }
        } else if k == ComponentKind::Materials {
            facts
                .materials_page
                .as_ref()
                .map_or(PageInfo::default(), |p| PageInfo {
                    offset: p.offset,
                    shown: p.items.len() as u64,
                    total: p.total,
                })
        } else {
            PageInfo {
                offset: 0,
                shown: 1,
                total: 1,
            }
        };
        node.card_index = Some(cards.len());
        cards.push(card(settings, k, node.rect, content, page, style)?);
        cards.last_mut().expect("just pushed").content_identity = *identity;
    }
    if cards.len() > 64 {
        return Err(SceneError::Capacity);
    }
    // Every static card costs one leaf. Distribute the remaining finite data-row
    // slots fairly; native pages must enforce these limits, never truncate DTOs.
    let mut remaining = 64usize.saturating_sub(cards.len());
    loop {
        let mut added = false;
        for card in &mut cards {
            let wanted = match &card.content {
                CardContent::Courses(rows) => rows.len().max(1),
                CardContent::Homework(rows) => rows.len().max(1),
                CardContent::Materials(list) => {
                    list.rows.len().max(1).min(list.visible_rows as usize)
                }
                _ => 1,
            };
            if remaining > 0 && (card.visible_leaf_limit as usize) < wanted {
                card.visible_leaf_limit += 1;
                remaining -= 1;
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    Ok(SceneSnapshot {
        generation,
        source_revision: facts.global_revision,
        captured_at: now,
        valid_until,
        cards,
        paint_tree: Some(paint_tree),
        text_tone: settings.text_tone,
        clock_rollback: facts.clock_rollback,
        public_calendar_unverified: public_unverified,
        lesson_control,
    })
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
