//! The no-code entry must close the original deployment before a separate
//! repair process can offer destructive confirmation. It cannot unlock it.
use super::{
    tests::{fixture, wait_for, SERIAL},
    *,
};

#[test]
fn reset_entry_never_runs_during_open_or_from_an_unrelated_screen() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app) = fixture();
    app.event(Event::Submit(TO_REINITIALIZE, FormData::new()))
        .unwrap();
    assert!(!app.reinitialize_requested);
    wait_for(&mut app, |a| a.pending.is_none());
    assert!(app.screen == Screen::Setup);
    assert!(!app.view().actions.iter().any(|a| a.id == TO_REINITIALIZE));
    app.event(Event::Submit(TO_REINITIALIZE, FormData::new()))
        .unwrap();
    assert!(!app.reinitialize_requested && !app.stopping);
    app.begin_shutdown();
    wait_for(&mut app, |a| a.pool.is_finished());
    assert!(app.writer_closed);
    app.pool.join().unwrap();
}

#[test]
fn no_code_entry_keeps_original_data_and_requires_actual_writer_shutdown() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app) = fixture();
    wait_for(&mut app, |a| a.pending.is_none());
    let mut data = FormData::new();
    for (field, value) in [
        (SCHOOL, "Synthetic original school"),
        (LOGIN, "former-admin"),
        (NAME, "Former administrator"),
        (PASSWORD, "Synthetic entry password 2026!"),
        (CONFIRM, "Synthetic entry password 2026!"),
    ] {
        data.insert(field, value.into()).unwrap();
    }
    app.event(Event::Submit(SAVE, data)).unwrap();
    wait_for(&mut app, |a| a.pending.is_none());
    assert!(app.screen == Screen::Code);
    app.event(Event::Submit(ACK, FormData::new())).unwrap();
    let mut login = FormData::new();
    login.insert(LOGIN, "former-admin".into()).unwrap();
    login
        .insert(PASSWORD, "Synthetic entry password 2026!".into())
        .unwrap();
    app.event(Event::Submit(LOG_IN, login)).unwrap();
    wait_for(&mut app, |a| a.pending.is_none());
    assert!(app.screen == Screen::Home);
    // A forged entry from an authenticated management screen cannot interrupt
    // unrelated work. The actual no-code entry is on the locked login screen.
    app.event(Event::Submit(TO_REINITIALIZE, FormData::new()))
        .unwrap();
    assert!(!app.reinitialize_requested);
    app.lock();
    assert!(app.screen == Screen::Login);
    assert!(app.view().actions.iter().any(|a| a.id == TO_REINITIALIZE));
    let root = app.root.clone();
    let database = root.join("state/classroom.sqlite3");
    let key = std::fs::read(root.join("config/credential-key.dpapi")).unwrap();
    app.event(Event::Submit(TO_REINITIALIZE, FormData::new()))
        .unwrap();
    assert!(app.reinitialize_requested && app.stopping);
    assert!(
        !app.writer_closed,
        "a request is not writer-close acknowledgement"
    );
    assert!(
        !app.shutdown_complete(),
        "a host exit cannot authorize a restart yet"
    );
    wait_for(&mut app, |a| a.pool.is_finished());
    assert!(app.writer_closed && !app.terminate_required);
    assert!(app.shutdown_complete());
    assert!(app
        .backend
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .state()
        .is_err());
    app.pool.join().unwrap();
    assert!(
        database.is_file(),
        "entry alone cannot erase teaching storage"
    );
    assert_eq!(
        std::fs::read(root.join("config/credential-key.dpapi")).unwrap(),
        key
    );
}
