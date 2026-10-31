use super::*;
use crate::NATIVE_TEST_SERIAL;
use fc_presentation::workbench::{Action, Choice, Field};

const SAVE: ActionId = ActionId(1);
const EMPTY: ActionId = ActionId(2);
const LOCK: ActionId = ActionId(3);

fn view() -> View {
    View {
        title: "Synthetic empty action".into(),
        description: "Hidden owned controls only".into(),
        status: String::new(),
        busy: false,
        grid: None,
        fields: vec![
            Field {
                id: FieldId(1),
                label: "Draft".into(),
                kind: FieldKind::Text,
                value: "60000".into(),
                max_utf16: 32,
            },
            Field {
                id: FieldId(2),
                label: "Confirmation".into(),
                kind: FieldKind::Choice(vec![
                    Choice {
                        value: "no".into(),
                        label: "Not confirmed".into(),
                    },
                    Choice {
                        value: "yes".into(),
                        label: "Confirmed".into(),
                    },
                ]),
                value: "no".into(),
                max_utf16: 32,
            },
            Field {
                id: FieldId(3),
                label: "Synthetic password".into(),
                kind: FieldKind::Password,
                value: String::new(),
                max_utf16: 64,
            },
        ],
        actions: [SAVE, EMPTY, LOCK]
            .into_iter()
            .map(|id| Action {
                id,
                label: format!("Synthetic {}", id.0),
                enabled: true,
            })
            .collect(),
    }
}

fn field(window: &Window, id: u16) -> FieldBinding {
    *window
        .state
        .controls
        .borrow()
        .fields
        .iter()
        .find(|field| field.id == FieldId(id))
        .unwrap()
}

fn action(window: &Window, id: ActionId) -> ActionBinding {
    *window
        .state
        .controls
        .borrow()
        .actions
        .iter()
        .find(|action| action.id == id)
        .unwrap()
}

fn text(window: &Window, id: u16, text: &str) {
    let text = HSTRING::from(text);
    // SAFETY: only this hidden fixture's current owned control.
    unsafe { SetWindowTextW(field(window, id).hwnd, &text).unwrap() };
}

fn click(window: &Window, action: ActionBinding) {
    // SAFETY: exact owned button notification; no desktop input or focus request.
    unsafe {
        let _ = SendMessageW(
            window.hwnd,
            WM_COMMAND,
            Some(WPARAM(
                usize::from(action.native_id) | ((BN_CLICKED as usize) << 16),
            )),
            Some(LPARAM(action.hwnd.0 as isize)),
        );
    }
}

#[test]
fn empty_action_preserves_native_drafts_confirmation_and_prior_forms() {
    let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: observe only; this test never shows a window.
    let foreground = unsafe { GetForegroundWindow() };
    let window = Window::new().unwrap();
    window.set_empty_form_action(EMPTY).unwrap();
    let model = view();
    window.render(&model).unwrap();
    let FieldKind::Choice(options) = &model.fields[1].kind else {
        unreachable!()
    };
    set_choice(field(&window, 2).hwnd, options, "yes").unwrap();
    text(&window, 1, "queued draft");
    text(&window, 3, "SyntheticQueuedSecret");
    click(&window, action(&window, SAVE));
    // If the empty action reads this native draft, normal bounded capture fails.
    text(&window, 1, &"x".repeat(33));
    text(&window, 3, "SyntheticCurrentSecret");
    click(&window, action(&window, EMPTY));
    window.probe_submit(EMPTY, FormData::new()).unwrap();
    let submits: Vec<_> = window
        .take_events()
        .unwrap()
        .into_iter()
        .filter_map(|event| match event {
            Event::Submit(id, form) => Some((id, form)),
            _ => None,
        })
        .collect();
    assert_eq!(submits.len(), 3);
    assert_eq!(submits[0].0, SAVE);
    assert_eq!(submits[0].1.get(FieldId(1)), Some("queued draft"));
    assert_eq!(submits[0].1.get(FieldId(2)), Some("yes"));
    assert_eq!(submits[0].1.get(FieldId(3)), Some("SyntheticQueuedSecret"));
    for (id, form) in &submits[1..] {
        assert_eq!(*id, EMPTY);
        assert!(form.is_empty());
    }
    assert_eq!(window.state.read_field(field(&window, 2)).unwrap(), "yes");
    assert_eq!(window.state.read_field(field(&window, 3)).unwrap(), "");
    // SAFETY: read only this owned nonsecret Edit's native length.
    assert_eq!(unsafe { GetWindowTextLengthW(field(&window, 1).hwnd) }, 33);
    text(&window, 1, "500");
    click(&window, action(&window, SAVE));
    let form = window
        .take_events()
        .unwrap()
        .into_iter()
        .find_map(|event| match event {
            Event::Submit(id, form) if id == SAVE => Some(form),
            _ => None,
        })
        .unwrap();
    assert_eq!(form.get(FieldId(1)), Some("500"));
    assert_eq!(form.get(FieldId(2)), Some("yes"));
    assert_eq!(form.get(FieldId(3)), Some(""));
    assert!(!window.is_visible());
    // SAFETY: no mutation of the foreground window.
    assert_eq!(unsafe { GetForegroundWindow() }, foreground);
}

#[test]
fn empty_action_registration_is_immutable_and_disjoint_from_lock() {
    let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let window = Window::new().unwrap();
    assert!(window.set_empty_form_action(ActionId(0)).is_err());
    window.set_empty_form_action(EMPTY).unwrap();
    window.set_empty_form_action(EMPTY).unwrap();
    assert!(window.set_empty_form_action(ActionId(4)).is_err());
    assert!(window.set_lock_action(EMPTY).is_err());
    window.set_lock_action(LOCK).unwrap();
    assert!(window.set_empty_form_action(LOCK).is_err());
    let reverse = Window::new().unwrap();
    reverse.set_lock_action(LOCK).unwrap();
    assert!(reverse.set_empty_form_action(LOCK).is_err());
    reverse.set_empty_form_action(EMPTY).unwrap();
    assert!(reverse.set_lock_action(EMPTY).is_err());
}

#[test]
fn empty_action_rejects_nonempty_disabled_busy_missing_and_spoofed_commands() {
    let _serial = NATIVE_TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let window = Window::new().unwrap();
    window.set_empty_form_action(EMPTY).unwrap();
    window.set_lock_action(LOCK).unwrap();
    assert!(window.probe_submit(EMPTY, FormData::new()).is_err());
    let mut model = view();
    window.render(&model).unwrap();
    text(&window, 1, "untouched draft");
    text(&window, 3, "SyntheticNotSubmitted");
    let mut nonempty = FormData::new();
    nonempty.insert(FieldId(1), String::new()).unwrap();
    assert!(window.probe_submit(EMPTY, nonempty).is_err());
    assert_eq!(
        window.state.read_field(field(&window, 1)).unwrap(),
        "untouched draft"
    );
    assert_eq!(
        window.state.read_field(field(&window, 3)).unwrap(),
        "SyntheticNotSubmitted"
    );
    let empty = action(&window, EMPTY);
    let save = action(&window, SAVE);
    // SAFETY: deliberately mismatched ids/handles still target only our hidden
    // owned window; the binding table must reject this local malformed message.
    unsafe {
        let _ = SendMessageW(
            window.hwnd,
            WM_COMMAND,
            Some(WPARAM(usize::from(save.native_id))),
            Some(LPARAM(empty.hwnd.0 as isize)),
        );
    }
    assert!(!window
        .take_events()
        .unwrap()
        .iter()
        .any(|event| matches!(event, Event::Submit(..))));
    model.busy = true;
    window.render(&model).unwrap();
    assert!(window.probe_submit(EMPTY, FormData::new()).is_err());
    click(&window, empty);
    assert!(window.take_events().unwrap().is_empty());
    // Lock retains its original busy exemption and empty-event behavior.
    window.probe_submit(LOCK, FormData::new()).unwrap();
    assert!(window
        .take_events()
        .unwrap()
        .iter()
        .any(|event| matches!(event, Event::Submit(id, data) if *id == LOCK && data.is_empty())));
    model.busy = false;
    model.actions[1].enabled = false;
    window.render(&model).unwrap();
    assert!(window.probe_submit(EMPTY, FormData::new()).is_err());
    click(&window, empty);
    assert!(window.take_events().unwrap().is_empty());
    model.actions.remove(1);
    window.render(&model).unwrap();
    assert!(window.probe_submit(EMPTY, FormData::new()).is_err());
    click(&window, empty);
    assert!(window.take_events().unwrap().is_empty());
}
