//! Pure native management state: these tests never open a registry scope.
use super::*;

const CURRENT: &str = "\"C:\\New classroom\\FutureClassroom.exe\" --autostart";
const OLD: &str = "\"C:\\Old classroom\\FutureClassroom.exe\" --autostart";
fn loaded(observed: AutostartObservation) -> Loaded {
    Loaded {
        state: Some(AutostartState {
            revision: Revision::new(42),
            ..Default::default()
        }),
        observed,
        target_command: CURRENT.into(),
        operation_outcome: None,
    }
}
fn editor(observed: AutostartObservation) -> Editor {
    Editor::new(PrincipalId::new(7).unwrap(), loaded(observed))
}
fn acknowledged() -> FormData {
    let mut data = FormData::new();
    data.insert(ACK, "1".into()).unwrap();
    data
}
fn act(editor: &mut Editor, action: ActionId) -> Result<Command, String> {
    editor.event(action, FormData::new())
}
#[test]
fn confirmation_requires_a_live_review_and_explicit_acknowledgement() {
    let mut e = editor(AutostartObservation::Absent);
    assert!(e.event(CONFIRM, acknowledged()).is_err());
    assert!(!e.has_unsaved_changes());
    assert!(matches!(act(&mut e, ENABLE).unwrap(), Command::Stay));
    assert!(e.has_unsaved_changes());
    assert!(act(&mut e, CONFIRM).is_err());
    assert!(
        e.has_unsaved_changes(),
        "missing ACK must preserve the review"
    );
    let mut invalid = FormData::new();
    invalid.insert(ACK, "true".into()).unwrap();
    assert!(e.event(CONFIRM, invalid).is_err());
    assert!(matches!(
        e.event(CONFIRM, acknowledged()).unwrap(),
        Command::Change { desired: true, .. }
    ));
    assert!(!e.has_unsaved_changes());
    assert!(e.event(CONFIRM, acknowledged()).is_err());
}
#[test]
fn opaque_or_unavailable_observations_cannot_be_changed_by_forged_actions() {
    for observed in [
        AutostartObservation::Opaque {
            kind: 3,
            bytes: u32::MAX,
        },
        AutostartObservation::Unavailable(AutostartFailure::AccessDenied),
    ] {
        let mut e = editor(observed);
        e.view("", false).validate().unwrap();
        assert!(e
            .view("", false)
            .actions
            .iter()
            .filter(|a| a.id == ENABLE || a.id == DISABLE)
            .all(|a| !a.enabled));
        assert!(act(&mut e, ENABLE).is_err());
        assert!(act(&mut e, DISABLE).is_err());
        assert!(e.event(CONFIRM, acknowledged()).is_err());
        assert!(!e.has_unsaved_changes());
    }
}
#[test]
fn review_uses_captured_intent_revision_preimage_and_owner_despite_forged_fields() {
    let observed = AutostartObservation::Command(OLD.into());
    let mut e = editor(observed.clone());
    act(&mut e, ENABLE).unwrap();
    // A forged second intent cannot replace the first captured review.
    assert!(matches!(act(&mut e, DISABLE).unwrap(), Command::Stay));
    let mut data = acknowledged();
    for (id, value) in [
        (TARGET, "malicious target"),
        (ACTUAL, "absent"),
        (FieldId(777), "999999"),
        (FieldId(778), "false"),
        (FieldId(779), "another owner"),
    ] {
        data.insert(id, value.into()).unwrap();
    }
    let Command::Change {
        expected,
        desired,
        reviewed_target,
        previous_observed,
    } = e.event(CONFIRM, data).unwrap()
    else {
        panic!("captured explicit change expected");
    };
    assert_eq!(expected, Revision::new(42));
    assert!(desired);
    assert_eq!(reviewed_target, CURRENT);
    assert_eq!(previous_observed, observed);
    assert_eq!(e.owner(), PrincipalId::new(7).unwrap());
    assert_eq!(e.loaded().target_command, CURRENT);
}
#[test]
fn invalidation_discards_review_and_requires_a_successful_new_load() {
    let mut e = editor(AutostartObservation::Absent);
    act(&mut e, ENABLE).unwrap();
    e.invalidate();
    assert!(!e.has_unsaved_changes());
    assert!(e.event(CONFIRM, acknowledged()).is_err());
    assert!(act(&mut e, ENABLE).is_err());
    assert!(matches!(act(&mut e, REFRESH).unwrap(), Command::Load));
    assert!(
        act(&mut e, ENABLE).is_err(),
        "requesting refresh does not mean it succeeded"
    );
    let mut e = Editor::new(e.owner(), loaded(AutostartObservation::Absent));
    assert!(matches!(act(&mut e, ENABLE).unwrap(), Command::Stay));
}
#[test]
fn cancel_close_and_refresh_do_not_submit_a_change() {
    for action in [CANCEL, CLOSE, REFRESH] {
        let mut e = editor(AutostartObservation::Absent);
        act(&mut e, DISABLE).unwrap();
        let result = act(&mut e, action).unwrap();
        assert!(!matches!(result, Command::Change { .. }));
        assert!(!e.has_unsaved_changes());
        assert!(e.event(CONFIRM, acknowledged()).is_err());
    }
}
#[test]
fn moved_path_and_current_target_are_visible_and_busy_actions_are_disabled() {
    let mut e = editor(AutostartObservation::Command(OLD.into()));
    let view = e.view("", false);
    view.validate().unwrap();
    assert_eq!(
        view.fields.iter().find(|f| f.id == TARGET).unwrap().value,
        CURRENT
    );
    assert!(view
        .fields
        .iter()
        .find(|f| f.id == ACTUAL)
        .unwrap()
        .value
        .contains(OLD));
    assert!(view.description.contains("移动 ZIP 程序目录"));
    assert!(e.view("", true).actions.iter().all(|a| !a.enabled));
    act(&mut e, ENABLE).unwrap();
    let view = e.view("", true);
    view.validate().unwrap();
    assert!(view.actions.iter().all(|a| !a.enabled));
}
#[test]
fn uninitialized_deployment_has_no_mutation_review() {
    let mut l = loaded(AutostartObservation::Absent);
    l.state = None;
    let mut e = Editor::new(PrincipalId::new(7).unwrap(), l);
    assert!(act(&mut e, ENABLE).is_err());
    assert!(act(&mut e, DISABLE).is_err());
    assert!(!e.has_unsaved_changes());
    assert!(e.event(CONFIRM, acknowledged()).is_err());
}
