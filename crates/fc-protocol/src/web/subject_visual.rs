//! Bounded single-subject preview disclosure, with no management request DTO.
use super::content;
use crate::{ProtocolError, SCHEMA_VERSION};
use fc_domain::{content::HomeworkKey, identity::Rgb, ids::Revision};
use serde::Serialize;

/// Initial shared soft-subject-color formula. Changing its meaning requires a
/// new rules version; the browser consumes the derived RGB without re-softening.
pub const RULES_VERSION: u16 = 1;
pub struct Preview<'a> {
    pub key: &'a HomeworkKey,
    pub subject_name: &'a str,
    pub subject_revision: Revision,
    pub display_revision: Revision,
    pub effective_rgb: Rgb,
    pub filled: bool,
    pub homework_fill_percent: u8,
    pub course_fill_percent: u8,
    pub homework_component_opacity_percent: u8,
    pub course_component_opacity_percent: u8,
    pub homework_visible: bool,
    pub course_visible: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Key {
    kind: &'static str,
    class_id: String,
    subject_id: String,
    owner_teacher_id: String,
    date: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Wire<'a> {
    schema_version: u16,
    status: &'static str,
    key: Key,
    subject_id: String,
    subject_name: &'a str,
    subject_revision: String,
    display_revision: String,
    rules_version: u16,
    effective_rgb: String,
    mode: &'static str,
    homework_fill_percent: u8,
    course_fill_percent: u8,
    homework_component_opacity_percent: u8,
    course_component_opacity_percent: u8,
    homework_visible: bool,
    course_visible: bool,
}
pub fn preview(p: Preview<'_>) -> Result<Vec<u8>, ProtocolError> {
    if p.subject_name.trim().is_empty()
        || p.subject_name.chars().count() > 64
        || p.subject_name.chars().any(|c| c.is_control())
        || p.subject_revision.get() == 0
        || [
            p.homework_fill_percent,
            p.course_fill_percent,
            p.homework_component_opacity_percent,
            p.course_component_opacity_percent,
        ]
        .into_iter()
        .any(|p| p > 100)
    {
        return Err(ProtocolError::InvalidCommand);
    }
    let rgb = p.effective_rgb;
    let result = serde_json::to_vec(&Wire {
        schema_version: SCHEMA_VERSION,
        status: "SUBJECT_VISUAL",
        key: Key {
            kind: "HOMEWORK",
            class_id: p.key.class_id.get().to_string(),
            subject_id: p.key.subject_id.get().to_string(),
            owner_teacher_id: p.key.owner_teacher_id.get().to_string(),
            date: content::date(p.key.actual_date),
        },
        subject_id: p.key.subject_id.get().to_string(),
        subject_name: p.subject_name,
        subject_revision: p.subject_revision.get().to_string(),
        display_revision: p.display_revision.get().to_string(),
        rules_version: RULES_VERSION,
        effective_rgb: format!("#{:02X}{:02X}{:02X}", rgb.red, rgb.green, rgb.blue),
        mode: if p.filled { "FILLED" } else { "OUTLINED" },
        homework_fill_percent: p.homework_fill_percent,
        course_fill_percent: p.course_fill_percent,
        homework_component_opacity_percent: p.homework_component_opacity_percent,
        course_component_opacity_percent: p.course_component_opacity_percent,
        homework_visible: p.homework_visible,
        course_visible: p.course_visible,
    })
    .map_err(|_| ProtocolError::InvalidCommand)?;
    if result.len() > crate::MAX_JSON_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::{ids::*, time::LocalDate};
    #[test]
    fn exact_single_subject_wire_keeps_independent_revisions_and_zero_filled_mode() {
        let key = HomeworkKey {
            class_id: ClassId::new(1).unwrap(),
            subject_id: SubjectId::new(2).unwrap(),
            owner_teacher_id: PrincipalId::new(3).unwrap(),
            actual_date: LocalDate::new(2026, 11, 1).unwrap(),
        };
        let make = |filled, opacity| Preview {
            key: &key,
            subject_name: "数学",
            subject_revision: Revision::new(u64::MAX),
            display_revision: Revision::new(4),
            effective_rgb: Rgb {
                red: 170,
                green: 187,
                blue: 204,
            },
            filled,
            homework_fill_percent: 0,
            course_fill_percent: 20,
            homework_component_opacity_percent: opacity,
            course_component_opacity_percent: 100,
            homework_visible: true,
            course_visible: false,
        };
        let value: serde_json::Value =
            serde_json::from_slice(&preview(make(true, 50)).unwrap()).unwrap();
        assert_eq!(value["subjectRevision"], u64::MAX.to_string());
        assert_eq!(value["displayRevision"], "4");
        assert_eq!(value["mode"], "FILLED");
        assert_eq!(value["homeworkFillPercent"], 0);
        assert_eq!(value["effectiveRgb"], "#AABBCC");
        assert!(value.get("palette").is_none());
        assert!(value.get("layout").is_none());
        assert!(preview(make(false, 101)).is_err());
        let outlined: serde_json::Value =
            serde_json::from_slice(&preview(make(false, 100)).unwrap()).unwrap();
        assert_eq!(outlined["mode"], "OUTLINED");
        assert_eq!(outlined["effectiveRgb"], value["effectiveRgb"]);
    }
}
