use super::*;
use fc_domain::{desktop::*, ids::Revision};
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Component {
    pub kind: String,
    pub visible: bool,
    pub opacity_percent: u8,
    pub font_dip: u16,
    pub min_width: u16,
    pub max_width: u16,
    pub min_height: u16,
    pub max_height: u16,
}
pub fn kind(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Clock => "clock",
        ComponentKind::Date => "date",
        ComponentKind::Schedule => "schedule",
        ComponentKind::Homework => "homework",
        ComponentKind::Materials => "materials",
        ComponentKind::Reminder => "reminder",
        ComponentKind::DateCountdown => "date_countdown",
        ComponentKind::Recess => "recess",
        ComponentKind::SubjectText => "subject_text",
        ComponentKind::SubjectImage => "subject_image",
    }
}
pub fn read_kind(value: &str) -> Result<ComponentKind, ProtocolError> {
    Ok(match value {
        "clock" => ComponentKind::Clock,
        "date" => ComponentKind::Date,
        "schedule" => ComponentKind::Schedule,
        "homework" => ComponentKind::Homework,
        "materials" => ComponentKind::Materials,
        "reminder" => ComponentKind::Reminder,
        "date_countdown" => ComponentKind::DateCountdown,
        "recess" => ComponentKind::Recess,
        "subject_text" => ComponentKind::SubjectText,
        "subject_image" => ComponentKind::SubjectImage,
        _ => return Err(ProtocolError::InvalidCommand),
    })
}
impl From<&ComponentSettings> for Component {
    fn from(c: &ComponentSettings) -> Self {
        Self {
            kind: kind(c.kind).into(),
            visible: c.visible,
            opacity_percent: c.opacity_percent,
            font_dip: c.font_dip,
            min_width: c.min_width,
            max_width: c.max_width,
            min_height: c.min_height,
            max_height: c.max_height,
        }
    }
}
impl Component {
    pub fn domain(self) -> Result<ComponentSettings, ProtocolError> {
        Ok(ComponentSettings {
            kind: read_kind(&self.kind)?,
            visible: self.visible,
            opacity_percent: self.opacity_percent,
            font_dip: self.font_dip,
            min_width: self.min_width,
            max_width: self.max_width,
            min_height: self.min_height,
            max_height: self.max_height,
        })
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Countdown {
    pub target: Option<String>,
    pub label: String,
    pub expired: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Data {
    pub target_monitor: Option<String>,
    pub clock_mode: String,
    pub clock_seconds: bool,
    pub recess_enabled: bool,
    pub subject_fill: bool,
    pub course_fill_percent: u8,
    pub homework_fill_percent: u8,
    pub text_tone: String,
    pub glass_surface_percent: u8,
    pub edge_reflection_percent: u8,
    pub components: Vec<Component>,
    pub countdown: Countdown,
}
impl From<&DisplaySettings> for Data {
    fn from(d: &DisplaySettings) -> Self {
        Self {
            target_monitor: d.target_monitor.clone(),
            clock_mode: match d.clock_mode {
                ClockMode::Digital24 => "digital24",
                ClockMode::Digital12 => "digital12",
                ClockMode::Analog => "analog",
            }
            .into(),
            clock_seconds: d.clock_seconds,
            recess_enabled: d.recess_enabled,
            subject_fill: d.subject_fill,
            course_fill_percent: d.course_fill_percent,
            homework_fill_percent: d.homework_fill_percent,
            text_tone: match d.text_tone {
                TextTone::Light => "light",
                TextTone::Dark => "dark",
            }
            .into(),
            glass_surface_percent: d.glass_surface_percent,
            edge_reflection_percent: d.edge_reflection_percent,
            components: d.components.iter().map(Component::from).collect(),
            countdown: Countdown {
                target: d.countdown.target.map(date_text),
                label: d.countdown.label.clone(),
                expired: match d.countdown.expired {
                    CountdownExpired::Hide => "hide",
                    CountdownExpired::ShowCompleted => "show_completed",
                }
                .into(),
            },
        }
    }
}
impl Data {
    pub fn domain(self, revision: Revision) -> Result<DisplaySettings, ProtocolError> {
        if self.components.len() > 8 {
            return Err(ProtocolError::InvalidCommand);
        }
        let settings = DisplaySettings {
            revision,
            target_monitor: self.target_monitor,
            clock_mode: match self.clock_mode.as_str() {
                "digital24" => ClockMode::Digital24,
                "digital12" => ClockMode::Digital12,
                "analog" => ClockMode::Analog,
                _ => return Err(ProtocolError::InvalidCommand),
            },
            clock_seconds: self.clock_seconds,
            recess_enabled: self.recess_enabled,
            subject_fill: self.subject_fill,
            course_fill_percent: self.course_fill_percent,
            homework_fill_percent: self.homework_fill_percent,
            text_tone: match self.text_tone.as_str() {
                "light" => TextTone::Light,
                "dark" => TextTone::Dark,
                _ => return Err(ProtocolError::InvalidCommand),
            },
            glass_surface_percent: self.glass_surface_percent,
            edge_reflection_percent: self.edge_reflection_percent,
            components: self
                .components
                .into_iter()
                .map(Component::domain)
                .collect::<Result<_, _>>()?,
            countdown: DateCountdownSettings {
                target: self.countdown.target.map(|v| date(&v)).transpose()?,
                label: self.countdown.label,
                expired: match self.countdown.expired.as_str() {
                    "hide" => CountdownExpired::Hide,
                    "show_completed" => CountdownExpired::ShowCompleted,
                    _ => return Err(ProtocolError::InvalidCommand),
                },
            },
        };
        settings
            .validate()
            .map_err(|_| ProtocolError::InvalidCommand)?;
        Ok(settings)
    }
}
pub fn document(settings: &DisplaySettings) -> Result<Vec<u8>, ProtocolError> {
    encode(settings.revision, &Data::from(settings))
}
pub fn mutation(bytes: &[u8]) -> Result<(Revision, DisplaySettings), ProtocolError> {
    let request = Mutation::decode(bytes)?;
    if request.action != "save" {
        return Err(ProtocolError::InvalidCommand);
    }
    let (revision, data) = request.candidate::<Data>()?;
    Ok((revision, data.domain(revision)?))
}
