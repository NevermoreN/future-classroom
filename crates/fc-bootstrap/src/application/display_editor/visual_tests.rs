use super::*;
fn state() -> SubjectVisualState {
    let subjects = vec![
        SchoolSubject {
            id: SubjectId::new(2).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            name: "同名".into(),
            enabled: true,
            color: Rgb {
                red: 0x12,
                green: 0x34,
                blue: 0xAB,
            },
            revision: Revision::new(5),
        },
        SchoolSubject {
            id: SubjectId::new(9).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            name: "同名".into(),
            enabled: false,
            color: Rgb {
                red: 90,
                green: 100,
                blue: 110,
            },
            revision: Revision::new(8),
        },
    ];
    SubjectVisualState {
        palette: SubjectVisualPalette {
            revision: PaletteRevision::from_subjects(&subjects).unwrap(),
            subjects,
        },
        display: DisplaySettings {
            revision: Revision::new(7),
            ..Default::default()
        },
    }
}
fn editor() -> Editor {
    let mut e = Editor::new_visuals(state(), PrincipalId::new(1).unwrap());
    let data = form(&e);
    e.handle(VISUALS, data).unwrap();
    e
}
fn form(e: &Editor) -> FormData {
    let mut d = FormData::new();
    for f in e.view("", false).fields {
        if f.kind != FieldKind::ReadOnly {
            d.insert(f.id, f.value).unwrap();
        }
    }
    d
}
fn set(d: &mut FormData, id: FieldId, value: &str) {
    d.take(id);
    d.insert(id, value.into()).unwrap();
}
fn value(e: &Editor, id: FieldId) -> String {
    e.view("", false)
        .fields
        .into_iter()
        .find(|f| f.id == id)
        .unwrap()
        .value
}
#[test]
fn original_rgb_stable_ids_and_disabled_subjects_are_loaded_without_repainting_drafts() {
    let e = editor();
    assert_eq!(value(&e, OPAQUE_RGB), "1234AB");
    assert_eq!(value(&e, PALETTE_SUBJECT), "2");
    assert_eq!(e.colors().len(), 2);
    assert!(!e.has_unsaved_changes());
    e.view("", false).validate().unwrap();
}
#[test]
fn bad_rgb_bad_alpha_unknown_or_missing_fields_never_partially_change_joint_draft() {
    for (id, text) in [
        (OPAQUE_RGB, "#12345"),
        (OPAQUE_RGB, "12345"),
        (OPAQUE_RGB, "GGGGGG"),
        (OPAQUE_RGB, "abcdef00"),
        (COURSE_ALPHA, "101"),
        (HOMEWORK_ALPHA, "-1"),
        (PALETTE_SUBJECT, "3"),
    ] {
        let mut e = editor();
        let before = e.colors();
        let settings = e.settings.clone();
        let mut d = form(&e);
        set(&mut d, OPAQUE_RGB, "AABBCC");
        set(&mut d, COURSE_ALPHA, "77");
        set(&mut d, id, text);
        assert!(e.handle(APPLY, d).is_err());
        assert_eq!(e.colors(), before);
        assert_eq!(e.settings, settings);
    }
    let mut e = editor();
    let mut d = form(&e);
    d.insert(FieldId(900), "hidden-version".into()).unwrap();
    assert!(e.handle(SAVE_ALL, d).is_err());
    let mut d = form(&e);
    d.take(FILL);
    assert!(e.handle(APPLY, d).is_err());
    assert!(!e.has_unsaved_changes());
}
#[test]
fn save_captures_complete_original_version_vector_and_display_revision_until_success() {
    let mut e = editor();
    let original = state();
    let mut d = form(&e);
    set(&mut d, OPAQUE_RGB, "D02080");
    set(&mut d, FILL, "0");
    set(&mut d, COURSE_ALPHA, "73");
    let Command::Save {
        expected_palette,
        expected,
        colors,
        settings,
    } = e.handle(SAVE_ALL, d).unwrap()
    else {
        panic!()
    };
    assert_eq!(expected_palette, original.palette.revision);
    assert_eq!(expected, original.display.revision);
    assert_eq!(colors.len(), 2);
    assert_eq!(colors[1].color, original.palette.subjects[1].color);
    assert!(!settings.subject_fill);
    assert_eq!(settings.course_fill_percent, 73);
    // An error has no saved callback. A second explicit save still captures the
    // old versions, rather than silently rebasing either half of the draft.
    let d = form(&e);
    let Command::Save {
        expected_palette: again,
        expected: display,
        ..
    } = e.handle(SAVE_ALL, d).unwrap()
    else {
        panic!()
    };
    assert_eq!(again, expected_palette);
    assert_eq!(display, expected);
    assert!(e.has_unsaved_changes());
    let mut confirmed = original;
    confirmed.palette.subjects[0].color = colors[0].color;
    confirmed.palette.subjects[0].revision = Revision::new(6);
    confirmed.palette.revision =
        PaletteRevision::from_subjects(&confirmed.palette.subjects).unwrap();
    confirmed.display = settings;
    confirmed.display.revision = Revision::new(8);
    e.visuals_saved(confirmed);
    assert!(!e.has_unsaved_changes());
}
#[test]
fn switching_subjects_saves_old_color_and_loads_selected_original_not_same_name() {
    let mut e = editor();
    let mut d = form(&e);
    set(&mut d, PALETTE_SUBJECT, "9");
    set(&mut d, OPAQUE_RGB, "AABBCC");
    assert!(e.handle(SAVE_ALL, d).is_err());
    assert_eq!(e.selected_subject, Some(SubjectId::new(2).unwrap()));
    let d = form(&e);
    e.handle(SWITCH_SUBJECT, d).unwrap();
    assert_eq!(e.selected_subject, Some(SubjectId::new(9).unwrap()));
    assert_eq!(value(&e, OPAQUE_RGB), "5A646E");
    assert_eq!(
        e.colors()[0].color,
        Rgb {
            red: 0xAA,
            green: 0xBB,
            blue: 0xCC
        }
    );
}
#[test]
fn cancel_review_and_lock_capture_keep_partial_rgb_and_require_explicit_reload() {
    let mut e = editor();
    let mut d = form(&e);
    set(&mut d, OPAQUE_RGB, "A");
    e.capture(d);
    assert_eq!(value(&e, OPAQUE_RGB), "A");
    let d = form(&e);
    e.handle(CANCEL, d).unwrap();
    assert!(e.has_unsaved_changes());
    e.handle(KEEP, FormData::new()).unwrap();
    assert_eq!(value(&e, OPAQUE_RGB), "A");
    let d = form(&e);
    e.handle(CANCEL, d).unwrap();
    assert!(matches!(
        e.handle(CONFIRM_CANCEL, FormData::new()),
        Ok(Command::Reload)
    ));
    assert_eq!(
        e.colors(),
        state()
            .palette
            .subjects
            .iter()
            .map(|s| SubjectColor {
                subject_id: s.id,
                color: s.color
            })
            .collect::<Vec<_>>()
    );
}
#[test]
fn picker_has_only_current_subject_capability_and_preview_is_draft_only() {
    let mut e = editor();
    let d = form(&e);
    let Command::PickColor {
        subject_id,
        initial,
    } = e.handle(PICK_COLOR, d).unwrap()
    else {
        panic!()
    };
    assert_eq!(subject_id, SubjectId::new(2).unwrap());
    assert_eq!(initial, state().palette.subjects[0].color);
    assert!(!e.has_unsaved_changes());
    assert!(e.picked_color(SubjectId::new(9).unwrap(), initial).is_err());
    let chosen = Rgb {
        red: 1,
        green: 2,
        blue: 3,
    };
    e.picked_color(subject_id, chosen).unwrap();
    let d = form(&e);
    assert!(
        matches!(e.handle(VISUAL_PREVIEW,d),Ok(Command::VisualPreview{subject_id:id}) if id==subject_id)
    );
    assert_eq!(e.colors()[0].color, chosen);
    assert_eq!(e.expected, Revision::new(7));
    let d = form(&e);
    e.handle(EDIT_BACK, d).unwrap();
    assert!(e.picked_color(subject_id, initial).is_err());
}
#[test]
fn every_visual_schema_and_empty_school_are_bounded() {
    let mut e = editor();
    for page in [
        Page::Main,
        Page::Components,
        Page::SubjectVisuals,
        Page::Preview,
        Page::Discard,
    ] {
        e.page = page;
        e.view("", false).validate().unwrap();
    }
    let mut e = Editor::new(DisplaySettings::default(), PrincipalId::new(1).unwrap());
    e.page = Page::SubjectVisuals;
    e.view("", false).validate().unwrap();
    let d = form(&e);
    assert!(matches!(e.handle(SAVE_ALL,d),Ok(Command::Save{colors,..}) if colors.is_empty()));
}

#[test]
fn embedded_public_layout_style_never_exposes_or_accepts_school_palette_actions() {
    let mut editor = Editor::new(DisplaySettings::default(), PrincipalId::new(1).unwrap());
    assert!(!editor
        .view("", false)
        .actions
        .iter()
        .any(|action| action.id == VISUALS));
    for action in [VISUALS, SWITCH_SUBJECT, PICK_COLOR, VISUAL_PREVIEW] {
        let data = form(&editor);
        assert!(editor.handle(action, data).is_err());
    }
    let data = form(&editor);
    assert!(
        matches!(editor.handle(SAVE_ALL,data),Ok(Command::Save{colors,..}) if colors.is_empty())
    );
}
