//! The complete owned body description shared by pure measurement and native
//! DirectWrite layout. Geometry contains no handles and never truncates text.
use crate::scene::{CardContent, ClockContent};
use fc_domain::{
    desktop::{ClockMode, ComponentKind},
    identity::Rgb,
};
use fc_ports::TextMeasure;

pub const MAX_CARD_TEXT_UNITS: usize = 262_144;
pub const MAX_CARD_BLOCKS: usize = 128;
pub const MAX_MEASURED_HEIGHT: f32 = 16_777_216.;
pub const CARD_BODY_TEXT_PADDING: f32 = 8.;
pub const CARD_BODY_TEXT_INSET: f32 = 24.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardBodyRole {
    Plain,
    MaterialRow(usize),
    MaterialMore,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CardBodyBlock {
    pub text: String,
    pub center: bool,
    pub fill: Option<(Rgb, f32, f32)>,
    pub leaf: Option<usize>,
    pub role: CardBodyRole,
    pub min_height: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub enum CardBodyDynamic {
    Static,
    Image { width: u32, height: u32, pages: u8 },
    DigitalClock(ClockContent),
    AnalogClock,
    Recess,
    LessonControl { caption: String, can_change: bool },
}
#[derive(Clone, Debug, PartialEq)]
pub struct CardBodySpec {
    pub blocks: Vec<CardBodyBlock>,
    pub gap: f32,
    pub top_inset: f32,
    pub bottom_inset: f32,
    pub dynamic: CardBodyDynamic,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardBodyPosition {
    pub top: f32,
    pub height: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CardBodyGeometry {
    pub blocks: Vec<CardBodyPosition>,
    /// Includes the final block's complete bottom padding, with no trailing gap.
    pub total_body_height: f32,
}
/// Pure card-relative image and page controls, shared by native painting/input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageBodyGeometry {
    pub viewport: crate::Rect,
    pub image: crate::Rect,
    pub pager: Option<ImagePagerGeometry>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImagePagerGeometry {
    pub rect: crate::Rect,
    pub previous: crate::Rect,
    pub next: crate::Rect,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardBodyError {
    ContentLimit,
    InvalidContent,
    InvalidGeometry,
    InvalidMeasurement,
    Measure,
}

pub fn body_top(kind: ComponentKind, unverified: bool) -> f32 {
    if kind == ComponentKind::Schedule && unverified {
        50.
    } else {
        32.
    }
}
fn fill_valid(fill: Option<(Rgb, f32, f32)>) -> bool {
    fill.is_none_or(|(_, a, s)| {
        a.is_finite() && (0. ..=1.).contains(&a) && s.is_finite() && (0. ..=4.).contains(&s)
    })
}
fn add_units(total: &mut usize, text: &str) -> Result<(), CardBodyError> {
    let units = text.encode_utf16().take(MAX_CARD_TEXT_UNITS + 1).count();
    *total = total
        .checked_add(units)
        .filter(|n| *n <= MAX_CARD_TEXT_UNITS)
        .ok_or(CardBodyError::ContentLimit)?;
    Ok(())
}
impl CardBodySpec {
    pub fn text_units(&self) -> Result<usize, CardBodyError> {
        let mut units = 0;
        for block in &self.blocks {
            add_units(&mut units, &block.text)?;
        }
        if let CardBodyDynamic::LessonControl { caption, .. } = &self.dynamic {
            add_units(&mut units, caption)?;
        }
        Ok(units)
    }
    pub fn validate(&self) -> Result<(), CardBodyError> {
        if self.blocks.len() > MAX_CARD_BLOCKS {
            return Err(CardBodyError::ContentLimit);
        }
        self.text_units()?;
        if !self.gap.is_finite()
            || !(0. ..=64.).contains(&self.gap)
            || !self.top_inset.is_finite()
            || !(0. ..=64.).contains(&self.top_inset)
            || !self.bottom_inset.is_finite()
            || !(0. ..=64.).contains(&self.bottom_inset)
        {
            return Err(CardBodyError::InvalidGeometry);
        }
        if !matches!(self.dynamic, CardBodyDynamic::Static) && !self.blocks.is_empty() {
            return Err(CardBodyError::InvalidContent);
        }
        if let CardBodyDynamic::Image {
            width,
            height,
            pages,
        } = self.dynamic
        {
            if width == 0
                || height == 0
                || u64::from(width) * u64::from(height) > 8_000_000
                || !(1..=50).contains(&pages)
            {
                return Err(CardBodyError::InvalidContent);
            }
        }
        for block in &self.blocks {
            let minimum = if block.fill.is_some() || block.role != CardBodyRole::Plain {
                44.
            } else {
                24.
            };
            if !fill_valid(block.fill)
                || !block.min_height.is_finite()
                || block.min_height < minimum
                || block.min_height > MAX_MEASURED_HEIGHT
            {
                return Err(CardBodyError::InvalidContent);
            }
        }
        Ok(())
    }
    /// Inner body text uses 24 DIP on each side, matching native layout and ink.
    pub fn text_width(&self, outer_width: f32) -> Result<f32, CardBodyError> {
        let inset = match &self.dynamic {
            CardBodyDynamic::Static => 2. * CARD_BODY_TEXT_INSET,
            CardBodyDynamic::LessonControl {
                can_change: true, ..
            } => 180.,
            _ => 32.,
        };
        if !outer_width.is_finite() || outer_width <= inset || outer_width > 16_384. {
            return Err(CardBodyError::InvalidGeometry);
        }
        Ok(outer_width - inset)
    }
    /// Heights come from the exact text layouts which native will subsequently
    /// draw. Caller-provided count/NaN/infinity/negative/overflow is rejected.
    pub fn arrange(&self, measured_heights: &[f32]) -> Result<CardBodyGeometry, CardBodyError> {
        self.validate()?;
        if measured_heights.len() != self.blocks.len() {
            return Err(CardBodyError::InvalidMeasurement);
        }
        let mut blocks = Vec::with_capacity(self.blocks.len());
        let mut top = 0.;
        for (i, (block, measured)) in self.blocks.iter().zip(measured_heights).enumerate() {
            if !measured.is_finite() || !(0. ..=MAX_MEASURED_HEIGHT).contains(measured) {
                return Err(CardBodyError::InvalidMeasurement);
            }
            let height = (*measured + 2. * CARD_BODY_TEXT_PADDING).max(block.min_height);
            if i > 0 {
                top += self.gap;
            }
            blocks.push(CardBodyPosition { top, height });
            top += height;
            if !top.is_finite() || top > MAX_MEASURED_HEIGHT {
                return Err(CardBodyError::InvalidMeasurement);
            }
        }
        Ok(CardBodyGeometry {
            blocks,
            total_body_height: top,
        })
    }
    /// Contain the complete original aspect ratio in the allocated card. No
    /// pixel buffers, handles, paths or display-resource side effects occur here.
    pub fn image_geometry(
        &self,
        outer_width: f32,
        outer_height: f32,
    ) -> Result<ImageBodyGeometry, CardBodyError> {
        self.validate()?;
        let CardBodyDynamic::Image {
            width,
            height,
            pages,
        } = self.dynamic
        else {
            return Err(CardBodyError::InvalidContent);
        };
        let available_width = self.text_width(outer_width)?;
        let nav = if pages > 1 { 52. } else { 0. };
        let available_height = outer_height - self.top_inset - self.bottom_inset - nav;
        if !outer_height.is_finite()
            || outer_height > 16_384.
            || available_height <= 0.
            || available_width < 96.
        {
            return Err(CardBodyError::InvalidGeometry);
        }
        let viewport = crate::Rect {
            x: 16.,
            y: self.top_inset,
            width: available_width,
            height: available_height,
        };
        let scale = (available_width / width as f32).min(available_height / height as f32);
        let image_width = width as f32 * scale;
        let image_height = height as f32 * scale;
        let image = crate::Rect {
            x: viewport.x + (viewport.width - image_width) / 2.,
            y: viewport.y + (viewport.height - image_height) / 2.,
            width: image_width,
            height: image_height,
        };
        let pager = (pages > 1).then(|| {
            let rect = crate::Rect {
                x: 16.,
                y: outer_height - self.bottom_inset - 44.,
                width: available_width,
                height: 44.,
            };
            ImagePagerGeometry {
                rect,
                previous: crate::Rect { width: 44., ..rect },
                next: crate::Rect {
                    x: rect.x + rect.width - 44.,
                    width: 44.,
                    ..rect
                },
            }
        });
        Ok(ImageBodyGeometry {
            viewport,
            image,
            pager,
        })
    }
    /// Finite intrinsic whole-card height at the already selected outer width.
    /// Dynamic rules remain independent of wall-clock ticks and body block pages.
    pub fn intrinsic_height(
        &self,
        outer_width: f32,
        font: f32,
        measure: &dyn TextMeasure,
    ) -> Result<f32, CardBodyError> {
        self.validate()?;
        if !font.is_finite() || !(10. ..=72.).contains(&font) {
            return Err(CardBodyError::InvalidGeometry);
        }
        let width = self.text_width(outer_width)?;
        let measured = |text: &str, size: f32| -> Result<f32, CardBodyError> {
            let height = measure
                .height(text, width, size)
                .map_err(|_| CardBodyError::Measure)?;
            if !height.is_finite() || !(0. ..=MAX_MEASURED_HEIGHT).contains(&height) {
                return Err(CardBodyError::InvalidMeasurement);
            }
            Ok(height)
        };
        let height = match &self.dynamic {
            CardBodyDynamic::Static => {
                let heights = self
                    .blocks
                    .iter()
                    .map(|b| measured(&b.text, font))
                    .collect::<Result<Vec<_>, _>>()?;
                self.top_inset + self.arrange(&heights)?.total_body_height + self.bottom_inset
            }
            CardBodyDynamic::DigitalClock(clock) => {
                let label = match (clock.mode, clock.show_seconds) {
                    (ClockMode::Digital24, false) => "23:59",
                    (ClockMode::Digital24, true) => "23:59:59",
                    (_, false) => "下午 12:59",
                    (_, true) => "下午 12:59:59",
                };
                measured(label, font)? + 54.
            }
            CardBodyDynamic::AnalogClock => 160.,
            CardBodyDynamic::Image {
                width: image_width,
                height: image_height,
                pages,
            } => {
                self.top_inset
                    + width * (*image_height as f32 / *image_width as f32)
                    + self.bottom_inset
                    + if *pages > 1 { 52. } else { 0. }
            }
            // Recess is assigned the remaining protected region by compose;
            // only its fixed 100-DIP dynamic patch refreshes once a second.
            CardBodyDynamic::Recess => 100.,
            CardBodyDynamic::LessonControl { caption, .. } => {
                (measured(caption, font)? + 24.).max(56.)
            }
        };
        if !height.is_finite() || height > MAX_MEASURED_HEIGHT {
            return Err(CardBodyError::InvalidMeasurement);
        }
        Ok(height)
    }
}

pub fn card_body_spec(
    kind: ComponentKind,
    content: &CardContent,
    unverified: bool,
    note: Option<&str>,
) -> Result<CardBodySpec, CardBodyError> {
    let mut spec = CardBodySpec {
        blocks: Vec::new(),
        gap: 8.,
        top_inset: body_top(kind, unverified),
        bottom_inset: 12.,
        dynamic: CardBodyDynamic::Static,
    };
    // Bound source strings before formatting labels or cloning complete bodies.
    let mut units = 0;
    if let Some(note) = note {
        if kind != ComponentKind::Materials {
            return Err(CardBodyError::InvalidContent);
        }
        add_units(&mut units, note)?;
    }
    match content {
        CardContent::Text(text) => add_units(&mut units, text)?,
        CardContent::StandaloneText(text) => {
            if kind != ComponentKind::SubjectText
                || text.len() > 40_000
                || text.chars().count() > 10_000
                || text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err(CardBodyError::ContentLimit);
            }
            add_units(&mut units, text)?;
        }
        CardContent::Image(image) => {
            if kind != ComponentKind::SubjectImage
                || image.current.validate().is_err()
                || !(1..=50).contains(&image.total_pages)
                || image.current_page >= image.total_pages
            {
                return Err(CardBodyError::InvalidContent);
            }
        }
        CardContent::Courses(rows) => {
            if kind != ComponentKind::Schedule || rows.len() > 128 {
                return Err(CardBodyError::ContentLimit);
            }
            for row in rows {
                add_units(&mut units, &row.name)?;
            }
        }
        CardContent::Homework(rows) => {
            if kind != ComponentKind::Homework || rows.len() > 64 {
                return Err(CardBodyError::ContentLimit);
            }
            for row in rows {
                for text in [&row.subject_name, &row.teacher_name, &row.text] {
                    add_units(&mut units, text)?;
                }
            }
        }
        CardContent::Materials(list) => {
            if kind != ComponentKind::Materials
                || list.rows.len() > 5
                || !(1..=5).contains(&list.visible_rows)
                || list.row_gap_dip > 64
                || !list.rows.is_empty() && list.folder_id.is_none()
            {
                return Err(CardBodyError::ContentLimit);
            }
            add_units(&mut units, &list.teacher_name)?;
            add_units(&mut units, &list.subject_name)?;
            for row in &list.rows {
                add_units(&mut units, &row.name)?;
            }
        }
        CardContent::LessonControl { caption, .. } => add_units(&mut units, caption)?,
        _ => {}
    }
    let registry =
        crate::registry::Registry::new(BODIES).map_err(|_| CardBodyError::InvalidContent)?;
    let builder = registry
        .get((kind, BodyKind::of(content)))
        .map_err(|_| CardBodyError::InvalidContent)?;
    builder(&mut spec, content, note)?;
    spec.validate()?;
    Ok(spec)
}

#[cfg(test)]
#[path = "card_body_tests.rs"]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyKind {
    Clock,
    Recess,
    LessonControl,
    Text,
    StandaloneText,
    Image,
    Courses,
    Homework,
    Materials,
    MaterialsUnavailable,
}
impl BodyKind {
    fn of(content: &CardContent) -> Self {
        match content {
            CardContent::Clock(_) => Self::Clock,
            CardContent::Recess { .. } => Self::Recess,
            CardContent::LessonControl { .. } => Self::LessonControl,
            CardContent::Text(_) => Self::Text,
            CardContent::StandaloneText(_) => Self::StandaloneText,
            CardContent::Image(_) => Self::Image,
            CardContent::Courses(_) => Self::Courses,
            CardContent::Homework(_) => Self::Homework,
            CardContent::Materials(_) => Self::Materials,
            CardContent::MaterialsUnavailable => Self::MaterialsUnavailable,
        }
    }
}
type BodyBuilder = fn(&mut CardBodySpec, &CardContent, Option<&str>) -> Result<(), CardBodyError>;
static BODIES: &[crate::registry::Registration<(ComponentKind, BodyKind), BodyBuilder>] = &[
    crate::registry::Registration {
        key: (ComponentKind::Clock, BodyKind::Clock),
        implementation: bind_clock,
    },
    crate::registry::Registration {
        key: (ComponentKind::Recess, BodyKind::Recess),
        implementation: bind_recess,
    },
    crate::registry::Registration {
        key: (ComponentKind::Schedule, BodyKind::LessonControl),
        implementation: bind_lesson,
    },
    crate::registry::Registration {
        key: (ComponentKind::Clock, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::Date, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::Schedule, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::Homework, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::Materials, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::Reminder, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::DateCountdown, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::Recess, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::SubjectText, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::SubjectImage, BodyKind::Text),
        implementation: bind_text,
    },
    crate::registry::Registration {
        key: (ComponentKind::SubjectText, BodyKind::StandaloneText),
        implementation: bind_standalone,
    },
    crate::registry::Registration {
        key: (ComponentKind::SubjectImage, BodyKind::Image),
        implementation: bind_image,
    },
    crate::registry::Registration {
        key: (ComponentKind::Schedule, BodyKind::Courses),
        implementation: bind_courses,
    },
    crate::registry::Registration {
        key: (ComponentKind::Homework, BodyKind::Homework),
        implementation: bind_homework,
    },
    crate::registry::Registration {
        key: (ComponentKind::Materials, BodyKind::Materials),
        implementation: bind_materials,
    },
    crate::registry::Registration {
        key: (ComponentKind::Materials, BodyKind::MaterialsUnavailable),
        implementation: bind_unavailable,
    },
];
fn bind_clock(
    spec: &mut CardBodySpec,
    content: &CardContent,
    _note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::Clock(clock) = content else {
        return Err(CardBodyError::InvalidContent);
    };

    spec.dynamic = if clock.mode == ClockMode::Analog {
        CardBodyDynamic::AnalogClock
    } else {
        CardBodyDynamic::DigitalClock(*clock)
    };
    Ok(())
}
fn bind_recess(
    spec: &mut CardBodySpec,
    content: &CardContent,
    _note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::Recess { .. } = content else {
        return Err(CardBodyError::InvalidContent);
    };

    spec.dynamic = CardBodyDynamic::Recess;
    Ok(())
}
fn bind_lesson(
    spec: &mut CardBodySpec,
    content: &CardContent,
    _note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::LessonControl {
        caption,
        can_change,
    } = content
    else {
        return Err(CardBodyError::InvalidContent);
    };

    spec.dynamic = CardBodyDynamic::LessonControl {
        caption: caption.clone(),
        can_change: *can_change,
    };
    Ok(())
}
fn bind_text(
    spec: &mut CardBodySpec,
    content: &CardContent,
    _note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::Text(text) = content else {
        return Err(CardBodyError::InvalidContent);
    };
    push(spec, text.clone(), false, None, None, CardBodyRole::Plain);
    Ok(())
}
fn bind_standalone(
    spec: &mut CardBodySpec,
    content: &CardContent,
    _note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::StandaloneText(text) = content else {
        return Err(CardBodyError::InvalidContent);
    };

    push(
        spec,
        text.to_string(),
        false,
        None,
        None,
        CardBodyRole::Plain,
    );
    Ok(())
}
fn bind_image(
    spec: &mut CardBodySpec,
    content: &CardContent,
    _note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::Image(image) = content else {
        return Err(CardBodyError::InvalidContent);
    };

    spec.dynamic = CardBodyDynamic::Image {
        width: image.current.width,
        height: image.current.height,
        pages: image.total_pages,
    };
    Ok(())
}
fn bind_courses(
    spec: &mut CardBodySpec,
    content: &CardContent,
    _note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::Courses(rows) = content else {
        return Err(CardBodyError::InvalidContent);
    };

    for (i, row) in rows.iter().enumerate() {
        push(
            spec,
            if row.current {
                format!(
                    "{}\n{}",
                    row.name,
                    if row.temporary {
                        "临时 · 仅本节有效"
                    } else {
                        "正在上课"
                    }
                )
            } else {
                row.name.clone()
            },
            true,
            Some((row.color, row.fill_alpha, row.stroke_width)),
            Some(i),
            CardBodyRole::Plain,
        );
    }
    Ok(())
}
fn bind_homework(
    spec: &mut CardBodySpec,
    content: &CardContent,
    _note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::Homework(rows) = content else {
        return Err(CardBodyError::InvalidContent);
    };

    for (i, row) in rows.iter().enumerate() {
        push(
            spec,
            format!("{} · {}", row.subject_name, row.teacher_name),
            false,
            Some((row.color, row.fill_alpha, 0.)),
            Some(i),
            CardBodyRole::Plain,
        );
        push(
            spec,
            row.text.clone(),
            false,
            None,
            Some(i),
            CardBodyRole::Plain,
        );
    }
    Ok(())
}
fn bind_materials(
    spec: &mut CardBodySpec,
    content: &CardContent,
    note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::Materials(list) = content else {
        return Err(CardBodyError::InvalidContent);
    };

    spec.gap = f32::from(list.row_gap_dip);
    if let Some(note) = note {
        push(spec, note.into(), false, None, None, CardBodyRole::Plain);
    }
    push(
        spec,
        format!("{} · {}", list.subject_name, list.teacher_name),
        false,
        None,
        None,
        CardBodyRole::Plain,
    );
    if list.rows.is_empty() {
        push(
            spec,
            "该老师暂无课堂资料".into(),
            false,
            None,
            None,
            CardBodyRole::Plain,
        );
    }
    for (i, row) in list.rows.iter().enumerate() {
        push(
            spec,
            row.name.clone(),
            false,
            None,
            Some(i),
            CardBodyRole::MaterialRow(i),
        );
    }
    if list.next_cursor.is_some() {
        push(
            spec,
            "更多资料 ›".into(),
            false,
            None,
            None,
            CardBodyRole::MaterialMore,
        );
    };
    Ok(())
}
fn bind_unavailable(
    spec: &mut CardBodySpec,
    content: &CardContent,
    note: Option<&str>,
) -> Result<(), CardBodyError> {
    let CardContent::MaterialsUnavailable = content else {
        return Err(CardBodyError::InvalidContent);
    };

    if let Some(note) = note {
        push(spec, note.into(), false, None, None, CardBodyRole::Plain);
    }
    push(
        spec,
        "资料尚未就绪".into(),
        false,
        None,
        None,
        CardBodyRole::Plain,
    );
    Ok(())
}

fn push(
    spec: &mut CardBodySpec,
    text: String,
    center: bool,
    fill: Option<(Rgb, f32, f32)>,
    leaf: Option<usize>,
    role: CardBodyRole,
) {
    spec.blocks.push(CardBodyBlock {
        text,
        center,
        fill,
        leaf,
        role,
        min_height: if fill.is_some() || role != CardBodyRole::Plain {
            44.
        } else {
            24.
        },
    });
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    #[test]
    fn neutral_placeholders_bind_without_borrowing_another_components_builder() {
        let content = CardContent::Text("等待当前课程数据".into());
        for kind in ComponentKind::ALL
            .into_iter()
            .chain([ComponentKind::SubjectText, ComponentKind::SubjectImage])
        {
            let body = card_body_spec(kind, &content, false, None).unwrap();
            assert_eq!(body.blocks.len(), 1);
            assert_eq!(body.blocks[0].text, "等待当前课程数据");
            assert_eq!(body.dynamic, CardBodyDynamic::Static);
        }
        let clock = CardContent::Clock(ClockContent {
            mode: ClockMode::Analog,
            show_seconds: false,
        });
        assert!(matches!(
            card_body_spec(ComponentKind::Clock, &clock, false, None)
                .unwrap()
                .dynamic,
            CardBodyDynamic::AnalogClock
        ));
        assert_eq!(
            card_body_spec(ComponentKind::Date, &clock, false, None),
            Err(CardBodyError::InvalidContent)
        );
    }
    #[test]
    fn public_lesson_control_keeps_its_own_binding_without_changing_schedule_content() {
        let content = CardContent::LessonControl {
            caption: "仅本节".into(),
            can_change: true,
        };
        let control = card_body_spec(ComponentKind::Schedule, &content, true, None).unwrap();
        assert!(control.blocks.is_empty());
        assert_eq!(
            control.dynamic,
            CardBodyDynamic::LessonControl {
                caption: "仅本节".into(),
                can_change: true
            }
        );
        assert_eq!(
            card_body_spec(ComponentKind::Date, &content, false, None),
            Err(CardBodyError::InvalidContent)
        );
        let schedule = card_body_spec(
            ComponentKind::Schedule,
            &CardContent::Courses(vec![]),
            true,
            None,
        )
        .unwrap();
        assert_eq!(schedule.dynamic, CardBodyDynamic::Static);
        assert_eq!(schedule.top_inset, control.top_inset);
    }
}
