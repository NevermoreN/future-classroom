//! Pure presentation policy; native handles and COM remain in fc-windows.
#![forbid(unsafe_code)]
pub mod card_body;
pub mod layout;
pub mod scene;
pub mod workbench;
use fc_domain::time::LocalDateTime;
use fc_ports::{MeasureError, TextMeasure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Foreground {
    Desktop,
    ExternalApp,
    Settings,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityInput {
    pub foreground: Foreground,
    pub session_active: bool,
    pub healthy: bool,
    pub stopping: bool,
}
pub fn desktop_visible(i: VisibilityInput) -> bool {
    i.foreground == Foreground::Desktop && i.session_active && i.healthy && !i.stopping
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockStyle {
    Digital24,
    Digital12,
    Analog,
}
pub fn format_clock(now: LocalDateTime, style: ClockStyle, seconds: bool) -> String {
    let total = now.second_of_day();
    let h = total / 3600;
    let m = total / 60 % 60;
    let s = total % 60;
    let suffix = if seconds {
        format!(":{s:02}")
    } else {
        String::new()
    };
    match style {
        ClockStyle::Digital24 => format!("{h:02}:{m:02}{suffix}"),
        ClockStyle::Digital12 | ClockStyle::Analog => format!(
            "{} {}:{m:02}{suffix}",
            if h < 12 { "上午" } else { "下午" },
            match h % 12 {
                0 => 12,
                hour => hour,
            }
        ),
    }
}
pub fn countdown_label(remaining_seconds: u32) -> String {
    if remaining_seconds >= 3600 {
        format!(
            "{:02}:{:02}:{:02}",
            remaining_seconds / 3600,
            remaining_seconds / 60 % 60,
            remaining_seconds % 60
        )
    } else {
        format!(
            "{:02}:{:02}",
            remaining_seconds / 60,
            remaining_seconds % 60
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opacity(u8);
impl Opacity {
    pub fn new(percent: u8) -> Option<Self> {
        if percent <= 100 {
            Some(Self(percent))
        } else {
            None
        }
    }
    pub const fn percent(self) -> u8 {
        self.0
    }
    pub fn alpha(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassMaterial {
    pub surface: Opacity,
    pub reflection: Opacity,
    pub component: Opacity,
}
impl Default for GlassMaterial {
    fn default() -> Self {
        Self {
            surface: Opacity(3),
            reflection: Opacity(50),
            component: Opacity(100),
        }
    }
}
impl GlassMaterial {
    pub fn fill_alpha(self, ancestor: f32) -> f32 {
        self.surface.alpha() * self.component.alpha() * ancestor.clamp(0., 1.)
    }
    /// Text is intentionally independent of glass surface alpha.
    pub fn text_alpha(self, ancestor: f32) -> f32 {
        self.component.alpha() * ancestor.clamp(0., 1.)
    }
    pub fn accepts_input(self) -> bool {
        self.component.0 != 0
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PillStyle {
    pub fill_alpha: f32,
    pub stroke_width: f32,
}
pub fn pill_style(filled: bool, opacity: Opacity) -> PillStyle {
    if filled {
        PillStyle {
            fill_alpha: opacity.alpha(),
            stroke_width: 0.,
        }
    } else {
        PillStyle {
            fill_alpha: 0.,
            stroke_width: 1.,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetKind {
    Clock,
    Date,
    Schedule,
    Homework,
    Materials,
    Reminder,
    DateCountdown,
}
#[derive(Debug, Clone)]
pub struct Widget {
    pub kind: WidgetKind,
    pub text: String,
    pub font_dip: f32,
    pub opacity: Opacity,
}
#[derive(Debug, Clone)]
pub struct PlacedWidget {
    pub kind: WidgetKind,
    pub rect: Rect,
    pub pages: u32,
    pub opacity: Opacity,
}
#[derive(Debug, Clone)]
pub struct DesktopLayout {
    pub widgets: Vec<PlacedWidget>,
    pub recess_region: Option<Rect>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum LayoutError {
    InvalidWorkArea,
    TooManyWidgets,
    DuplicateWidget,
    ContentLimit,
    Measure,
    OutOfSpace,
}
pub const MAX_PAGES: u32 = 16_384;
impl From<MeasureError> for LayoutError {
    fn from(_: MeasureError) -> Self {
        Self::Measure
    }
}

/// One directional measure then arrange. Expired/empty reminders and hidden components
/// are removed before positioning, so they cannot leave gaps or input regions.
pub fn layout(
    widgets: &[Widget],
    work: Rect,
    recess: bool,
    measure: &dyn TextMeasure,
) -> Result<DesktopLayout, LayoutError> {
    layout_with_minimum(widgets, work, recess, measure, &|_| (0., 64.))
}

pub(crate) fn layout_with_minimum(
    widgets: &[Widget],
    work: Rect,
    recess: bool,
    measure: &dyn TextMeasure,
    minimum: &dyn Fn(WidgetKind) -> (f32, f32),
) -> Result<DesktopLayout, LayoutError> {
    if ![
        work.x,
        work.y,
        work.width,
        work.height,
        work.x + work.width,
        work.y + work.height,
    ]
    .iter()
    .all(|v| v.is_finite())
        || work.width < 480.
        || work.height < 240.
        || work.x + work.width <= work.x
        || work.y + work.height <= work.y
    {
        return Err(LayoutError::InvalidWorkArea);
    }
    if widgets.len() > 7 {
        return Err(LayoutError::TooManyWidgets);
    }
    let mut kinds = Vec::new();
    for w in widgets {
        if kinds.contains(&w.kind) {
            return Err(LayoutError::DuplicateWidget);
        }
        kinds.push(w.kind);
    }
    let gap = 12.;
    let margin = 16.;
    let column = |kind: WidgetKind| match kind {
        WidgetKind::Clock | WidgetKind::Date | WidgetKind::Schedule | WidgetKind::DateCountdown => {
            0
        }
        _ => 1,
    };
    let right_width = (work.width * 0.46).clamp(420., 800.);
    let mut minimum_width = [0_f32; 2];
    for w in widgets.iter().filter(|w| !w.text.is_empty()) {
        let (width, height) = minimum(w.kind);
        if !width.is_finite() || !height.is_finite() || width < 0. || height < 32. {
            return Err(LayoutError::ContentLimit);
        }
        minimum_width[column(w.kind)] = minimum_width[column(w.kind)].max(width);
    }
    let left_width = (right_width * 0.35).max(160.).max(minimum_width[0]);
    let main_width = (right_width - (right_width * 0.35).max(160.) - gap).max(minimum_width[1]);
    let right_width = left_width + gap + main_width;
    if right_width > work.width - 2. * margin {
        return Err(LayoutError::OutOfSpace);
    }
    let right_x = work.x + work.width - margin - right_width;
    let start = work.y + margin;
    let bottom = work.y + work.height - margin;
    let mut y = [start, start];
    let mut placed = Vec::new();
    for (index, w) in widgets.iter().enumerate() {
        if w.text.is_empty() {
            continue;
        }
        if w.text.len() > 16_384 || !w.font_dip.is_finite() || !(10.0..=72.0).contains(&w.font_dip)
        {
            return Err(LayoutError::ContentLimit);
        }
        let col = column(w.kind);
        let width = if col == 0 { left_width } else { main_width };
        let available = bottom - y[col];
        let minimum_height = minimum(w.kind).1;
        let reserved: f32 = widgets[index + 1..]
            .iter()
            .filter(|next| !next.text.is_empty() && column(next.kind) == col)
            .map(|next| minimum(next.kind).1 + gap)
            .sum();
        if available < minimum_height + reserved {
            return Err(LayoutError::OutOfSpace);
        }
        let measured = measure.height(&w.text, width - 32., w.font_dip)?;
        if !measured.is_finite() || measured < 0. {
            return Err(LayoutError::Measure);
        }
        // Reserve a finite share for following cards in the same column.
        let cap = (available - reserved)
            .min(
                work.height
                    * if w.kind == WidgetKind::Homework {
                        0.55
                    } else {
                        0.32
                    },
            )
            .max(minimum_height);
        let full_height = (measured + 32.).max(minimum_height);
        let height = full_height.min(cap);
        if !full_height.is_finite() || full_height / height > MAX_PAGES as f32 {
            return Err(LayoutError::Measure);
        }
        let pages = (full_height / height).ceil() as u32;
        let x = right_x + if col == 0 { 0. } else { left_width + gap };
        placed.push(PlacedWidget {
            kind: w.kind,
            rect: Rect {
                x,
                y: y[col],
                width,
                height,
            },
            pages,
            opacity: w.opacity,
        });
        y[col] += height + gap;
    }
    let remaining_width = right_x - work.x - margin - gap;
    let recess_region = if recess && remaining_width >= 200. {
        Some(Rect {
            x: work.x + margin,
            y: start,
            width: remaining_width,
            height: work.height - 2. * margin,
        })
    } else {
        None
    };
    Ok(DesktopLayout {
        widgets: placed,
        recess_region,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Measure;
    impl TextMeasure for Measure {
        fn height(&self, text: &str, width: f32, font: f32) -> Result<f32, MeasureError> {
            Ok((text.chars().count() as f32 * font / width).ceil().max(1.) * font * 1.4)
        }
    }
    fn widget(kind: WidgetKind, text: &str) -> Widget {
        Widget {
            kind,
            text: text.into(),
            font_dip: 18.,
            opacity: Opacity(100),
        }
    }
    fn work() -> Rect {
        Rect {
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1040.,
        }
    }
    #[test]
    fn settings_unknown_session_and_fault_all_hide() {
        for foreground in [
            Foreground::ExternalApp,
            Foreground::Settings,
            Foreground::Unknown,
        ] {
            assert!(!desktop_visible(VisibilityInput {
                foreground,
                session_active: true,
                healthy: true,
                stopping: false
            }));
        }
        assert!(!desktop_visible(VisibilityInput {
            foreground: Foreground::Desktop,
            session_active: true,
            healthy: false,
            stopping: false
        }));
    }
    #[test]
    fn transparent_components_keep_layout_and_empty_reminders_leave_no_gap() {
        let a = layout(
            &[
                widget(WidgetKind::Reminder, ""),
                widget(WidgetKind::Materials, "file.pptx"),
            ],
            work(),
            true,
            &Measure,
        )
        .unwrap();
        assert_eq!(a.widgets.len(), 1);
        assert_eq!(a.widgets[0].rect.y, 16.);
        let region = a.recess_region.unwrap();
        assert!(region.x + region.width < a.widgets[0].rect.x);
        let mut w = widget(WidgetKind::Materials, "file");
        w.opacity = Opacity(0);
        let transparent = layout(&[w], work(), false, &Measure).unwrap();
        assert_eq!(transparent.widgets.len(), 1);
        assert_eq!(transparent.widgets[0].opacity.percent(), 0);
        assert_eq!(transparent.widgets[0].rect.y, 16.);
    }
    #[test]
    fn glass_does_not_fade_text_and_filled_course_never_has_border() {
        let g = GlassMaterial::default();
        assert_eq!(g.fill_alpha(1.), 0.03);
        assert_eq!(g.text_alpha(1.), 1.);
        assert_eq!(pill_style(true, Opacity(0)).stroke_width, 0.);
        assert_eq!(pill_style(false, Opacity(20)).fill_alpha, 0.);
    }
    #[test]
    fn twelve_hour_boundaries_and_countdown() {
        let date = fc_domain::time::LocalDate::new(2026, 10, 30).unwrap();
        assert_eq!(
            format_clock(
                LocalDateTime::new(date, 0).unwrap(),
                ClockStyle::Digital12,
                false
            ),
            "上午 12:00"
        );
        assert_eq!(
            format_clock(
                LocalDateTime::new(date, 43200).unwrap(),
                ClockStyle::Digital12,
                false
            ),
            "下午 12:00"
        );
        assert_eq!(countdown_label(3601), "01:00:01");
    }
    #[test]
    fn overlong_content_is_paginated_without_unbounded_growth() {
        let a = layout(
            &[widget(WidgetKind::Homework, &"长作业".repeat(1000))],
            work(),
            false,
            &Measure,
        )
        .unwrap();
        assert!(a.widgets[0].pages > 1);
        assert!(a.widgets[0].rect.height <= work().height * 0.55);
    }
    #[test]
    fn tall_first_cards_reserve_space_for_following_cards_on_narrow_screens() {
        let area = Rect {
            x: -480.,
            y: 10.,
            width: 480.,
            height: 400.,
        };
        let widgets = [
            WidgetKind::Clock,
            WidgetKind::Date,
            WidgetKind::Schedule,
            WidgetKind::DateCountdown,
        ]
        .map(|kind| widget(kind, &"长".repeat(1000)));
        let result = layout(&widgets, area, false, &Measure).unwrap();
        assert_eq!(result.widgets.len(), 4);
        for card in &result.widgets {
            assert!(card.rect.x >= area.x + 16.);
            assert!(card.rect.x + card.rect.width <= area.x + area.width - 16.);
            assert!(card.rect.y + card.rect.height <= area.y + area.height - 16.);
        }
        for pair in result.widgets.windows(2) {
            assert!(pair[0].rect.y + pair[0].rect.height + 12. <= pair[1].rect.y);
        }
    }
    #[test]
    fn invalid_dip_coordinates_fonts_and_extreme_measurements_are_rejected() {
        for area in [
            Rect {
                x: f32::MAX,
                y: 0.,
                width: f32::MAX,
                height: 400.,
            },
            Rect {
                x: 1e30,
                y: 0.,
                width: 480.,
                height: 400.,
            },
            Rect {
                x: 0.,
                y: f32::NAN,
                width: 480.,
                height: 400.,
            },
        ] {
            assert!(matches!(
                layout(&[], area, false, &Measure),
                Err(LayoutError::InvalidWorkArea)
            ));
        }
        let mut card = widget(WidgetKind::Clock, "time");
        card.font_dip = f32::INFINITY;
        assert!(matches!(
            layout(&[card], work(), false, &Measure),
            Err(LayoutError::ContentLimit)
        ));
        struct Huge;
        impl TextMeasure for Huge {
            fn height(&self, _: &str, _: f32, _: f32) -> Result<f32, MeasureError> {
                Ok(f32::MAX)
            }
        }
        assert!(matches!(
            layout(
                &[widget(WidgetKind::Homework, "text")],
                work(),
                false,
                &Huge
            ),
            Err(LayoutError::Measure)
        ));
    }
}
