use super::super::tests::{drain, fixture, wait_for, Scratch, SERIAL};
use super::super::voice_management_probe::{submit, LOGIN_NAME, PASSWORD_VALUE};
use super::*;
use fc_runtime::worker::{JobError, Rejection};

fn initialized() -> (Scratch, App) {
    let (scratch, mut app) = fixture();
    wait_for(&mut app, |a| a.pending.is_none());
    let until = Instant::now() + Duration::from_secs(90);
    app.next_content_maintenance = until;
    app.next_voice_maintenance = until;
    app.next_calendar_maintenance = until;
    submit(
        &mut app,
        SAVE,
        &[
            (SCHOOL, "Synthetic HTTPS controller school"),
            (LOGIN, LOGIN_NAME),
            (NAME, "Synthetic HTTPS manager"),
            (PASSWORD, PASSWORD_VALUE),
            (CONFIRM, PASSWORD_VALUE),
        ],
        until,
    )
    .unwrap();
    submit(&mut app, ACK, &[], until).unwrap();
    if app.screen == Screen::Login {
        super::super::voice_management_probe::login(&mut app, until).unwrap();
    }
    assert!(app.screen == Screen::Home);
    (scratch, app)
}
fn request(base: &HttpsSettingsState) -> HttpsSettingsRequest {
    use fc_domain::https_settings::*;
    HttpsSettingsRequest {
        command_id: CommandId::new(700).unwrap(),
        expected_revision: base.settings.revision,
        expected_identity_revision: base.identity_revision,
        configuration: Some(
            HttpsConfiguration::new(
                HttpsHost::parse("localhost").unwrap(),
                8443,
                HttpsCertificateRef::parse(&"ab".repeat(20)).unwrap(),
            )
            .unwrap(),
        ),
    }
}
fn admit(app: &App, mut work: Work, deadline: Instant) -> JobTicket<Result<Outcome>> {
    loop {
        match app.pool.handle().try_submit(work, deadline) {
            Ok(ticket) => return ticket,
            Err(error) if error.kind == Rejection::Busy && Instant::now() < deadline => {
                work = error.task;
                std::thread::park_timeout(Duration::from_millis(1));
            }
            Err(error) => panic!("HTTPS test-only original task admission {:?}", error.kind),
        }
    }
}
#[test]
fn accepted_cancellation_while_backend_busy_keeps_original_fence_and_cannot_write() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, app) = initialized();
    let lease = app.lease.unwrap();
    let owner = app.backend.as_ref().unwrap().clone();
    let guard = owner.lock().unwrap();
    let before = guard.read_https_settings(lease).unwrap();
    let original = request(&before);
    let ticket = admit(
        &app,
        Work::Https(owner.clone(), lease, Operation::Save(original.clone())),
        Instant::now() + Duration::from_secs(15),
    );
    let until = Instant::now() + Duration::from_secs(15);
    while !app.pool.handle().try_stats().is_some_and(|s| s.running > 0) {
        assert!(Instant::now() < until);
        std::thread::park_timeout(Duration::from_millis(1));
    }
    ticket.cancel();
    drop(guard);
    while !ticket.is_finished() {
        assert!(Instant::now() < until);
        std::thread::park_timeout(Duration::from_millis(1));
    }
    assert!(matches!(ticket.wait(), Err(JobError::Cancelled)));
    assert_eq!(
        owner.lock().unwrap().read_https_settings(lease).unwrap(),
        before
    );
    assert!(owner
        .lock()
        .unwrap()
        .query_https_settings_outcome(lease, original)
        .unwrap()
        .is_none());
    drain(app);
}
#[test]
fn old_page_generation_cannot_deliver_positive_saved_state_or_erase_original_query() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app) = initialized();
    app.open_https_settings().unwrap();
    wait_for(&mut app, |a| a.pending.is_none());
    let limit = Instant::now() + Duration::from_secs(15);
    while app.https_unadmitted.is_some() {
        assert!(Instant::now() < limit);
        app.retry_https_unadmitted().unwrap();
        wait_for(&mut app, |a| a.pending.is_none());
    }
    let lease = app.lease.unwrap();
    let before = app
        .backend
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .read_https_settings(lease)
        .unwrap();
    let mut data = FormData::new();
    data.insert(editor::HOST, "localhost".into()).unwrap();
    data.insert(editor::PORT, "8443".into()).unwrap();
    data.insert(editor::CERTIFICATE, "ab".repeat(20)).unwrap();
    let e = app.https_editor.as_mut().unwrap();
    e.capture(data).unwrap();
    let original = e
        .prepare_save(CommandId::new(701).unwrap(), Instant::now())
        .unwrap();
    e.accepted();
    let mut fake_saved = before.clone();
    fake_saved.settings.revision = Revision::new(77);
    fake_saved.settings.configuration = original.configuration.clone();
    let publication = app
        .control
        .as_ref()
        .unwrap()
        .health_gate()
        .status()
        .revision;
    let stale_generation = app.generation.checked_add(1).unwrap();
    app.https_completed(
        Pending {
            kind: Kind::Save,
            lease,
        },
        stale_generation,
        Ok(Outcome::Https(Box::new(Completion {
            lease,
            publication,
            value: Value::Saved(fake_saved),
        }))),
    )
    .unwrap();
    assert_eq!(
        app.https_editor.as_ref().unwrap().query_request(),
        Some(original)
    );
    assert_eq!(
        app.backend
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .read_https_settings(lease)
            .unwrap(),
        before
    );
    assert!(app.https_editor.as_ref().unwrap().has_unknown());
    drain(app);
}
#[test]
fn native_same_pool_commit_cancel_relogin_explicit_query_and_certificate_facts_have_real_cleanup() {
    let _serial = SERIAL.lock().unwrap();
    super::super::https_settings_probe::run(None).unwrap();
}
#[test]
fn typed_https_fault_is_fatal_but_cas_conflict_is_not() {
    use fc_domain::https_settings::HttpsSettingsError;
    use fc_ports::HttpsSettingsRepositoryError as E;
    assert!(fatal(&Failure::Backend(BackendError::HttpsSettings(
        E::Faulted
    ))));
    assert!(!fatal(&Failure::Backend(BackendError::HttpsSettings(
        E::Domain(HttpsSettingsError::RevisionConflict {
            current: Revision::new(3)
        })
    ))));
}
