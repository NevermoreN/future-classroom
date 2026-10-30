//! Pure, transaction-consistent desktop composition. Native visibility and health
//! checks remain mandatory immediately before every publication.
use crate::{layout_with_minimum, ClockStyle, LayoutError, Opacity, Rect, Widget, WidgetKind};
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
}
#[derive(Clone, Debug, PartialEq)]
pub enum CardContent {
    Clock(ClockContent),
    Text(String),
    Courses(Vec<CourseCapsule>),
    Homework(Vec<HomeworkEntry>),
    Recess { deadline: LocalDateTime },
    MaterialsUnavailable,
    Materials(Box<MaterialListContent>),
}
#[derive(Clone, Debug, PartialEq)]
pub struct SceneCard {
    pub kind: ComponentKind,
    pub rect: Rect,
    pub font_dip: f32,
    pub component_alpha: f32,
    pub surface_alpha: f32,
    pub edge_alpha: f32,
    pub content: CardContent,
    pub page: PageInfo,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SceneSnapshot {
    pub generation: SceneGeneration,
    pub source_revision: Revision,
    pub captured_at: LocalDateTime,
    pub valid_until: Option<LocalDateTime>,
    pub cards: Vec<SceneCard>,
    pub text_tone: TextTone,
    pub clock_rollback: bool,
    pub public_calendar_unverified: bool,
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
        self.cards
            .iter()
            .filter(|c| c.component_alpha > 0.)
            .any(|c| match c.content {
                CardContent::Clock(clock) => clock.show_seconds,
                CardContent::Recess { .. } => true,
                _ => false,
            })
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum SceneError {
    InvalidFacts(DisplayError),
    Layout(LayoutError),
    Schedule,
    Capacity,
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
fn widget_kind(kind: ComponentKind) -> Option<WidgetKind> {
    Some(match kind {
        ComponentKind::Clock => WidgetKind::Clock,
        ComponentKind::Date => WidgetKind::Date,
        ComponentKind::Schedule => WidgetKind::Schedule,
        ComponentKind::Homework => WidgetKind::Homework,
        ComponentKind::Materials => WidgetKind::Materials,
        ComponentKind::Reminder => WidgetKind::Reminder,
        ComponentKind::DateCountdown => WidgetKind::DateCountdown,
        ComponentKind::Recess => return None,
    })
}
fn kind(widget: WidgetKind) -> ComponentKind {
    match widget {
        WidgetKind::Clock => ComponentKind::Clock,
        WidgetKind::Date => ComponentKind::Date,
        WidgetKind::Schedule => ComponentKind::Schedule,
        WidgetKind::Homework => ComponentKind::Homework,
        WidgetKind::Materials => ComponentKind::Materials,
        WidgetKind::Reminder => ComponentKind::Reminder,
        WidgetKind::DateCountdown => ComponentKind::DateCountdown,
    }
}
fn measure_text(content: &CardContent) -> String {
    let text = match content {
        CardContent::Clock(clock) => {
            if clock.mode == ClockMode::Analog {
                "时钟\n\n\n".into()
            } else {
                "时钟\n23:59:59".into()
            }
        }
        CardContent::Text(s) => format!("信息\n{s}"),
        CardContent::Courses(rows) => rows
            .iter()
            .map(|r| {
                format!(
                    "{}{}\n\n",
                    r.name,
                    if r.current { "\n正在上课" } else { "" }
                )
            })
            .collect(),
        CardContent::Homework(rows) => rows
            .iter()
            .map(|r| format!("{} · {}\n{}\n\n", r.subject_name, r.teacher_name, r.text))
            .collect(),
        CardContent::MaterialsUnavailable => "课堂资料\n当前无有效授课老师".into(),
        CardContent::Materials(list) => format!(
            "{} · {}\n{}",
            list.teacher_name,
            list.subject_name,
            if list.rows.is_empty() {
                "该老师暂无课堂资料".into()
            } else {
                list.rows
                    .iter()
                    .map(|row| format!("{}\n", row.name))
                    .collect::<String>()
            }
        ),
        CardContent::Recess { .. } => "00:00".into(),
    };
    // Only the layout sample is bounded. Full owned texts remain in the DTO for
    // native DirectWrite paging; no text is silently truncated at publication.
    let mut end = text.len().min(16_384);
    while !text.is_char_boundary(end) {
        end -= 1
    }
    text[..end].into()
}
fn card(
    settings: &DisplaySettings,
    k: ComponentKind,
    rect: Rect,
    content: CardContent,
    page: PageInfo,
) -> Result<SceneCard, SceneError> {
    let c = settings
        .component(k)
        .ok_or(SceneError::InvalidFacts(DisplayError::InvalidSettings))?;
    let alpha = f32::from(c.opacity_percent) / 100.;
    Ok(SceneCard {
        kind: k,
        rect,
        font_dip: f32::from(c.font_dip),
        component_alpha: alpha,
        surface_alpha: alpha * f32::from(settings.glass_surface_percent) / 100.,
        edge_alpha: alpha * f32::from(settings.edge_reflection_percent) / 100.,
        content,
        page,
    })
}

pub fn build_scene(
    facts: &DesktopFacts,
    generation: SceneGeneration,
    work: Rect,
    measure: &dyn TextMeasure,
) -> Result<SceneSnapshot, SceneError> {
    facts.validate().map_err(SceneError::InvalidFacts)?;
    let settings = &facts.display;
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
            let subject = identity
                .subjects
                .iter()
                .find(|s| s.id == cell.subject_id && s.enabled)
                .ok_or(SceneError::Schedule)?;
            let key = day.lesson_key(row);
            let current = matches!(&lesson,LessonState::Course{key:active,..}if *active==key);
            let alpha = f32::from(
                settings
                    .component(ComponentKind::Schedule)
                    .ok_or(SceneError::Schedule)?
                    .opacity_percent,
            ) / 100.;
            course_rows.push(CourseCapsule {
                key,
                subject_id: subject.id,
                name: subject.name.clone(),
                color: soft_subject_color(subject.color),
                current,
                fill_alpha: if settings.subject_fill {
                    f32::from(settings.course_fill_percent) / 100. * alpha
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
        let alpha = f32::from(
            settings
                .component(ComponentKind::Homework)
                .ok_or(SceneError::Schedule)?
                .opacity_percent,
        ) / 100.;
        homework.push(HomeworkEntry {
            subject_id: subject.id,
            teacher_id: teacher.principal.id,
            subject_name: subject.name.clone(),
            teacher_name: teacher.display_name.clone(),
            text: r.text.clone(),
            color: soft_subject_color(subject.color),
            fill_alpha: if settings.subject_fill {
                f32::from(settings.homework_fill_percent) / 100. * alpha
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
    entries.retain(|(k, _)| settings.component(*k).is_some_and(|c| c.visible));
    let widgets: Vec<_> = entries
        .iter()
        .filter_map(|(k, content)| {
            Some(Widget {
                kind: widget_kind(*k)?,
                text: if *k == ComponentKind::Schedule && public_unverified {
                    format!("调休资料未核验\n{}", measure_text(content))
                } else {
                    measure_text(content)
                },
                font_dip: f32::from(settings.component(*k)?.font_dip),
                opacity: Opacity::new(settings.component(*k)?.opacity_percent)?,
            })
        })
        .collect();
    let recess = if settings.recess_enabled
        && settings
            .component(ComponentKind::Recess)
            .is_some_and(|c| c.visible)
    {
        match lesson {
            LessonState::Recess { deadline, .. } if deadline > now => Some(deadline),
            _ => None,
        }
    } else {
        None
    };
    let minimum = |widget| {
        settings.component(kind(widget)).map_or((0., 84.), |c| {
            let height =
                if c.kind == ComponentKind::Clock && settings.clock_mode == ClockMode::Analog {
                    160.
                } else {
                    84.
                };
            (f32::from(c.min_width), f32::from(c.min_height).max(height))
        })
    };
    let geometry = layout_with_minimum(&widgets, work, recess.is_some(), measure, &minimum)
        .map_err(SceneError::Layout)?;
    let mut cards = Vec::new();
    for placed in geometry.widgets {
        let k = kind(placed.kind);
        let pos = entries
            .iter()
            .position(|(kind, _)| *kind == k)
            .ok_or(SceneError::Schedule)?;
        let (_, content) = entries.remove(pos);
        let c = settings.component(k).ok_or(SceneError::Schedule)?;
        let mut rect = placed.rect;
        // Configured bounds constrain derived geometry; impossible constraints fail
        // visibly instead of moving cards beyond the work area or shrinking fonts.
        let minimum_height = if matches!(&content, CardContent::Clock(clock) if clock.mode == ClockMode::Analog)
        {
            f32::from(c.min_height).max(160.)
        } else {
            f32::from(c.min_height).max(84.)
        };
        if rect.width < f32::from(c.min_width) || rect.height < minimum_height {
            return Err(SceneError::Layout(LayoutError::OutOfSpace));
        }
        rect.width = rect.width.min(f32::from(c.max_width));
        rect.height = rect.height.min(f32::from(c.max_height));
        let page = if k == ComponentKind::Homework {
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
        cards.push(card(settings, k, rect, content, page)?);
    }
    if let (Some(deadline), Some(rect)) = (recess, geometry.recess_region) {
        cards.push(card(
            settings,
            ComponentKind::Recess,
            rect,
            CardContent::Recess { deadline },
            PageInfo::default(),
        )?)
    }
    if cards.len() > 8 {
        return Err(SceneError::Capacity);
    }
    Ok(SceneSnapshot {
        generation,
        source_revision: facts.global_revision,
        captured_at: now,
        valid_until,
        cards,
        text_tone: settings.text_tone,
        clock_rollback: facts.clock_rollback,
        public_calendar_unverified: public_unverified,
    })
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
