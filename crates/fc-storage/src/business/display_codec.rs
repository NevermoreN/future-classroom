//! Explicit display/facts wire DTOs, bounded separately from persisted BLOBs.
use super::{codec::*, schedule_codec};
use crate::StorageError;
use fc_domain::desktop::*;
use fc_domain::ids::Revision;
use serde_json::{json, Value};

fn kind(value: ComponentKind) -> &'static str {
    match value {
        ComponentKind::Clock => "clock",
        ComponentKind::Date => "date",
        ComponentKind::Schedule => "schedule",
        ComponentKind::Homework => "homework",
        ComponentKind::Materials => "materials",
        ComponentKind::Reminder => "reminder",
        ComponentKind::DateCountdown => "date_countdown",
        ComponentKind::Recess => "recess",
    }
}
fn read_kind(value: &Value) -> Result<ComponentKind, StorageError> {
    match value.as_str() {
        Some("clock") => Ok(ComponentKind::Clock),
        Some("date") => Ok(ComponentKind::Date),
        Some("schedule") => Ok(ComponentKind::Schedule),
        Some("homework") => Ok(ComponentKind::Homework),
        Some("materials") => Ok(ComponentKind::Materials),
        Some("reminder") => Ok(ComponentKind::Reminder),
        Some("date_countdown") => Ok(ComponentKind::DateCountdown),
        Some("recess") => Ok(ComponentKind::Recess),
        _ => Err(StorageError::Database),
    }
}
fn small<T: TryFrom<u64>>(v: &Value, name: &str) -> Result<T, StorageError> {
    T::try_from(number(v, name)?).map_err(|_| StorageError::Database)
}
pub(super) fn settings(d: &DisplaySettings) -> Value {
    json!({
        "format":1,"revision":d.revision.get(),
        "clock_mode":match d.clock_mode {ClockMode::Digital24=>"digital24",ClockMode::Digital12=>"digital12",ClockMode::Analog=>"analog"},
        "clock_seconds":d.clock_seconds,"recess_enabled":d.recess_enabled,
        "subject_fill":d.subject_fill,"course_fill_percent":d.course_fill_percent,
        "homework_fill_percent":d.homework_fill_percent,
        "text_tone":match d.text_tone {TextTone::Light=>"light",TextTone::Dark=>"dark"},
        "glass_surface_percent":d.glass_surface_percent,"edge_reflection_percent":d.edge_reflection_percent,
        "components":d.components.iter().map(|c|json!({"kind":kind(c.kind),"visible":c.visible,
            "opacity_percent":c.opacity_percent,"font_dip":c.font_dip,"min_width":c.min_width,
            "max_width":c.max_width,"min_height":c.min_height,"max_height":c.max_height})).collect::<Vec<_>>(),
        "countdown":{"target":d.countdown.target.map(date),"label":d.countdown.label,
            "expired":match d.countdown.expired {CountdownExpired::Hide=>"hide",CountdownExpired::ShowCompleted=>"show_completed"}}
    })
}
pub(super) fn read_settings(v: &Value) -> Result<DisplaySettings, StorageError> {
    object_fields(
        v,
        &[
            "format",
            "revision",
            "clock_mode",
            "clock_seconds",
            "recess_enabled",
            "subject_fill",
            "course_fill_percent",
            "homework_fill_percent",
            "text_tone",
            "glass_surface_percent",
            "edge_reflection_percent",
            "components",
            "countdown",
        ],
    )?;
    if number(v, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let components = array(v, "components")?;
    if components.len() > 8 {
        return Err(StorageError::Database);
    }
    let components = components
        .iter()
        .map(|c| {
            object_fields(
                c,
                &[
                    "kind",
                    "visible",
                    "opacity_percent",
                    "font_dip",
                    "min_width",
                    "max_width",
                    "min_height",
                    "max_height",
                ],
            )?;
            Ok(ComponentSettings {
                kind: read_kind(&c["kind"])?,
                visible: boolean(c, "visible")?,
                opacity_percent: small(c, "opacity_percent")?,
                font_dip: small(c, "font_dip")?,
                min_width: small(c, "min_width")?,
                max_width: small(c, "max_width")?,
                min_height: small(c, "min_height")?,
                max_height: small(c, "max_height")?,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    let c = &v["countdown"];
    object_fields(c, &["target", "label", "expired"])?;
    let result = DisplaySettings {
        revision: Revision::new(number(v, "revision")?),
        clock_mode: match v["clock_mode"].as_str() {
            Some("digital24") => ClockMode::Digital24,
            Some("digital12") => ClockMode::Digital12,
            Some("analog") => ClockMode::Analog,
            _ => return Err(StorageError::Database),
        },
        clock_seconds: boolean(v, "clock_seconds")?,
        recess_enabled: boolean(v, "recess_enabled")?,
        subject_fill: boolean(v, "subject_fill")?,
        course_fill_percent: small(v, "course_fill_percent")?,
        homework_fill_percent: small(v, "homework_fill_percent")?,
        text_tone: match v["text_tone"].as_str() {
            Some("light") => TextTone::Light,
            Some("dark") => TextTone::Dark,
            _ => return Err(StorageError::Database),
        },
        glass_surface_percent: small(v, "glass_surface_percent")?,
        edge_reflection_percent: small(v, "edge_reflection_percent")?,
        components,
        countdown: DateCountdownSettings {
            target: if c["target"].is_null() {
                None
            } else {
                Some(read_date(&c["target"])?)
            },
            label: text(c, "label")?,
            expired: match c["expired"].as_str() {
                Some("hide") => CountdownExpired::Hide,
                Some("show_completed") => CountdownExpired::ShowCompleted,
                _ => return Err(StorageError::Database),
            },
        },
    };
    result.validate().map_err(|_| StorageError::Database)?;
    Ok(result)
}
pub(super) fn facts(d: &DesktopFacts) -> Value {
    json!({"format":1,"global_revision":d.global_revision.get(),"captured_at":time(d.captured_at),
        "date_watermark":date(d.date_watermark),"clock_rollback":d.clock_rollback,
        "lesson_context":super::lesson_codec::snapshot(&d.lesson_context),"identity":identity(&d.identity),"schedule":schedule_codec::document(&d.schedule),"display":settings(&d.display),
        "homework_offset":d.homework_offset,"homework_eligible_total":d.homework_eligible_total,
        "homework":d.homework.iter().map(record).collect::<Vec<_>>(),"reminder":d.reminder.as_ref().map(record),"materials_source":d.materials_source.as_ref().map(super::materials_codec::source),"materials_page":d.materials_page.as_ref().map(super::materials_codec::page)})
}
pub(super) fn read_facts(v: &Value) -> Result<DesktopFacts, StorageError> {
    object_fields(
        v,
        &[
            "format",
            "global_revision",
            "captured_at",
            "date_watermark",
            "clock_rollback",
            "identity",
            "schedule",
            "lesson_context",
            "display",
            "homework_offset",
            "homework_eligible_total",
            "homework",
            "reminder",
            "materials_source",
            "materials_page",
        ],
    )?;
    if number(v, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let homework = array(v, "homework")?;
    if homework.len() + usize::from(!v["reminder"].is_null()) > 64 {
        return Err(StorageError::Database);
    }
    let facts = DesktopFacts {
        global_revision: Revision::new(number(v, "global_revision")?),
        captured_at: read_time(&v["captured_at"])?,
        date_watermark: read_date(&v["date_watermark"])?,
        clock_rollback: boolean(v, "clock_rollback")?,
        identity: read_identity(&v["identity"])?,
        schedule: schedule_codec::read_document(&v["schedule"])?,
        lesson_context: super::lesson_codec::read_snapshot(&v["lesson_context"])?,
        display: read_settings(&v["display"])?,
        homework_offset: number(v, "homework_offset")?,
        homework_eligible_total: number(v, "homework_eligible_total")?,
        homework: homework
            .iter()
            .map(read_record)
            .collect::<Result<Vec<_>, _>>()?,
        reminder: if v["reminder"].is_null() {
            None
        } else {
            Some(read_record(&v["reminder"])?)
        },
        materials_source: if v["materials_source"].is_null() {
            None
        } else {
            Some(super::materials_codec::read_source(&v["materials_source"])?)
        },
        materials_page: if v["materials_page"].is_null() {
            None
        } else {
            Some(super::materials_codec::read_page(&v["materials_page"])?)
        },
    };
    facts.validate().map_err(|_| StorageError::Database)?;
    Ok(facts)
}
