//! Native worker integration, with real issuance and Writer publication. The
//! picker itself stays outside the synthetic fixture; no user file is selected.
use super::tests::{drain, lose_worker_reply, ready_management, wait_for, Scratch, SERIAL};
use super::*;
use fc_domain::materials::*;
use fc_ports::{command_window::CommandFamily, MaterialListRequest};
use fc_windows::materials::chooser::SelectedFile;

fn initialized() -> (Scratch, App, MaterialScope) {
    let (scratch, mut app) = ready_management();
    let owner = app.backend.as_ref().unwrap().clone();
    let mut backend = owner.lock().unwrap();
    let lease = app.lease.unwrap();
    let class = ClassId::new(1).unwrap();
    let subject = SubjectId::new(1).unwrap();
    for action in [
        IdentityCommand::CreateClass {
            id: class,
            name: "Synthetic batch class".into(),
        },
        IdentityCommand::PutSubject {
            id: subject,
            name: "Synthetic batch subject".into(),
            enabled: true,
            color: Rgb {
                red: 40,
                green: 80,
                blue: 120,
            },
            expected_revision: None,
        },
        IdentityCommand::UpdateAccount {
            id: lease.principal_id(),
            display_name: "Synthetic batch owner".into(),
            role: Role::GlobalAdmin,
            enabled: true,
            scopes: vec![fc_domain::identity::AssignmentScope {
                class_id: class,
                subject_id: subject,
            }],
        },
    ] {
        let issued = backend
            .issue_native_command(CommandFamily::Identity, Some(lease), SessionPurpose::Manage)
            .unwrap();
        backend
            .begin_command_scope(issued)
            .unwrap()
            .identity(lease, action)
            .unwrap();
    }
    let renewed = backend
        .login(
            voice_management_probe::LOGIN_NAME,
            Password::new(voice_management_probe::PASSWORD_VALUE.into()).unwrap(),
        )
        .unwrap();
    app.state = Arc::new(backend.state().unwrap());
    app.lease = Some(renewed);
    drop(backend);
    app.event(Event::Submit(TO_MATERIALS, FormData::new()))
        .unwrap();
    let scope = MaterialScope {
        teacher_id: renewed.principal_id(),
        subject_id: subject,
    };
    (scratch, app, scope)
}
fn selection(app: &App, name: &str, bytes: &[u8]) -> SelectedFile {
    let dir = app.root.join("quarantine");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(name), bytes).unwrap();
    SelectedFile::isolated_probe(&app.root, name).unwrap()
}
fn confirm(app: &mut App, scope: MaterialScope, files: Vec<SelectedFile>) {
    app.material_editor
        .as_mut()
        .unwrap()
        .stage_batch(scope, vec![ClassId::new(1).unwrap()], files)
        .unwrap();
    app.material_event(materials_editor::CONFIRM_BATCH, FormData::new())
        .unwrap();
}
fn current(app: &App, scope: MaterialScope) -> MaterialPage {
    app.backend
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .materials(
            app.lease.unwrap(),
            MaterialListRequest {
                scope,
                cursor: None,
            },
        )
        .unwrap()
}
#[test]
fn lost_batch_reply_relogin_current_read_cannot_confirm_but_original_pair_query_can() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app, scope) = initialized();
    let first = selection(&app, "first.txt", b"first synthetic material");
    let last = selection(&app, "last.txt", b"last synthetic material");
    confirm(&mut app, scope, vec![first, last]);
    lose_worker_reply(&mut app);
    let original = app.native_command.clone().unwrap();
    let pair = original.snapshot().unwrap();
    let before = current(&app, scope);
    assert_eq!(before.total, 2);
    assert_eq!(before.items[0].display_name, "last.txt");
    assert_eq!(before.items[1].display_name, "first.txt");
    let old_lease = app.lease.unwrap();
    app.lock();
    let owner = app.backend.as_ref().unwrap().clone();
    let new_lease = owner
        .lock()
        .unwrap()
        .login(
            voice_management_probe::LOGIN_NAME,
            Password::new(voice_management_probe::PASSWORD_VALUE.into()).unwrap(),
        )
        .unwrap();
    assert_ne!(new_lease.id(), old_lease.id());
    assert_eq!(new_lease.principal_id(), old_lease.principal_id());
    app.lease = Some(new_lease);
    app.screen = Screen::Home;
    app.event(Event::Submit(TO_MATERIALS, FormData::new()))
        .unwrap();
    let mut form = FormData::new();
    form.insert(
        materials_editor::TEACHER,
        scope.teacher_id.get().to_string(),
    )
    .unwrap();
    form.insert(
        materials_editor::SUBJECT,
        scope.subject_id.get().to_string(),
    )
    .unwrap();
    app.material_event(materials_editor::LOAD, form).unwrap();
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(
        app.native_unknown,
        "a fresh lease and current directory do not prove the original batch result"
    );
    assert_eq!(app.native_command.as_ref(), Some(&original));
    assert!(!app
        .view()
        .actions
        .iter()
        .any(|action| action.id == materials_editor::IMPORT && action.enabled));
    app.material_event(materials_editor::QUERY_BATCH, FormData::new())
        .unwrap();
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(!app.native_unknown);
    assert_eq!(
        original.snapshot(),
        Some(pair),
        "query never issues a replacement ticket"
    );
    assert_eq!(
        current(&app, scope),
        before,
        "history does not append or alter current files"
    );
    assert!(app
        .view()
        .actions
        .iter()
        .any(|action| action.id == materials_editor::DISMISS_BATCH && action.enabled));
    drain(app);
}
#[test]
fn invalid_second_file_keeps_directory_empty_and_reclaims_all_uncommitted_blobs() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app, scope) = initialized();
    let first = selection(&app, "valid.txt", b"validated but uncommitted");
    let bad = selection(&app, "broken.png", b"this is not an image");
    let before = current(&app, scope);
    confirm(&mut app, scope, vec![first, bad]);
    wait_for(&mut app, |app| app.pending.is_none());
    assert_eq!(current(&app, scope), before);
    assert!(
        !app.native_unknown,
        "the worker knows it never called the batch Writer"
    );
    assert!(app.status.contains("整批未送交目录写入"));
    let files = fc_windows::materials::files::ControlledFiles::new(&app.root).unwrap();
    let staging = files.directory(&["staging".into()]).unwrap();
    assert!(files.entries(&staging, 128).unwrap().is_empty());
    // Parent ID directories may remain, but no prepared inode can survive.
    fn file_count(path: &std::path::Path) -> usize {
        if !path.exists() {
            return 0;
        }
        fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    file_count(&path)
                } else {
                    1
                }
            })
            .sum()
    }
    assert_eq!(file_count(&app.root.join("teachers")), 0);
    app.material_event(materials_editor::DISMISS_BATCH, FormData::new())
        .unwrap();
    assert!(app
        .view()
        .actions
        .iter()
        .any(|action| action.id == materials_editor::IMPORT && action.enabled));
    drain(app);
}

/// Holds the existing two physical workers so cancellation is proven before the
/// batch is dequeued, rather than relying on a scheduler race or fake job result.
#[derive(Default)]
pub(super) struct Hold {
    state: Mutex<(usize, bool)>,
    changed: std::sync::Condvar,
}
impl Hold {
    pub(super) fn wait(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.0 += 1;
        self.changed.notify_all();
        while !state.1 {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
    fn release(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .1 = true;
        self.changed.notify_all();
    }
    fn wait_for_both(&self) {
        let until = Instant::now() + Duration::from_secs(30);
        while self.state.lock().unwrap().0 != 2 {
            assert!(
                Instant::now() < until,
                "original shared workers did not enter"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
struct ReleaseOnDrop(Arc<Hold>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}
fn relogin_materials(app: &mut App) {
    voice_management_probe::login(app, Instant::now() + Duration::from_secs(90)).unwrap();
    app.event(Event::Submit(TO_MATERIALS, FormData::new()))
        .unwrap();
}
#[test]
fn lock_before_batch_dequeue_waits_for_real_worker_return_then_allows_explicit_retry() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app, scope) = initialized();
    let hold = Arc::new(Hold::default());
    let release = ReleaseOnDrop(hold.clone());
    let handles = (0..2)
        .map(|_| {
            app.pool
                .handle()
                .try_submit(
                    Work::HoldForMaterialBatch(hold.clone()),
                    Instant::now() + Duration::from_secs(120),
                )
                .unwrap_or_else(|_| panic!("shared worker hold admission"))
        })
        .collect::<Vec<_>>();
    hold.wait_for_both();
    let selected = selection(&app, "cancel-before-start.txt", b"never prepared");
    confirm(&mut app, scope, vec![selected]);
    let original = app.native_command.clone().unwrap();
    assert!(original.unissued_after_return());
    assert!(!app.pending.as_ref().unwrap().ticket.is_finished());
    app.lock();
    app.poll().unwrap();
    assert!(
        app.pending.is_some(),
        "cancellation did not manufacture worker completion"
    );
    assert!(!app
        .material_editor
        .as_ref()
        .unwrap()
        .view(&app.state, "", false)
        .actions
        .iter()
        .any(|action| action.id == materials_editor::DISMISS_BATCH && action.enabled));
    hold.release();
    for ticket in handles {
        assert!(matches!(ticket.wait(), Ok(Ok(Outcome::MaterialCleaned))));
    }
    drop(release);
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(
        original.unissued_after_return(),
        "cancelled batch did not issue a replacement ticket"
    );
    relogin_materials(&mut app);
    assert_eq!(current(&app, scope).total, 0);
    assert!(app
        .view()
        .fields
        .iter()
        .any(|field| field.label == "本批未送交目录写入"));
    assert!(app
        .view()
        .actions
        .iter()
        .any(|action| action.id == materials_editor::DISMISS_BATCH && action.enabled));
    app.material_event(materials_editor::DISMISS_BATCH, FormData::new())
        .unwrap();
    assert!(app
        .view()
        .actions
        .iter()
        .any(|action| action.id == materials_editor::IMPORT && action.enabled));
    drain(app);
}
#[test]
fn cancellation_after_issued_commit_keeps_unknown_until_original_receipt_query() {
    let _serial = SERIAL.lock().unwrap();
    let (_scratch, mut app, scope) = initialized();
    let selected = selection(&app, "cancel-after-commit.txt", b"actually committed");
    confirm(&mut app, scope, vec![selected]);
    let original = app.native_command.clone().unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    while !app.pending.as_ref().unwrap().ticket.is_finished() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    let pair = original
        .snapshot()
        .expect("real worker issued the original pair");
    let committed = current(&app, scope);
    assert_eq!(committed.total, 1);
    // The real ticket suppresses the queued positive metadata reply on cancel.
    // Its original pair proves this cannot be classified as pre-issue failure.
    app.pending.as_ref().unwrap().ticket.cancel();
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(app.native_unknown);
    relogin_materials(&mut app);
    assert!(app.native_unknown);
    assert!(!app
        .view()
        .actions
        .iter()
        .any(|action| action.id == materials_editor::DISMISS_BATCH && action.enabled));
    app.material_event(materials_editor::QUERY_BATCH, FormData::new())
        .unwrap();
    wait_for(&mut app, |app| app.pending.is_none());
    assert!(!app.native_unknown);
    assert_eq!(original.snapshot(), Some(pair));
    assert_eq!(current(&app, scope), committed);
    drain(app);
}
#[test]
fn locked_or_poisoned_command_metadata_never_proves_issuance_absent() {
    let original = NativeCommand::new();
    let guard = original.0.lock().unwrap();
    assert!(!original.unissued_after_return());
    drop(guard);
    assert!(original.unissued_after_return());
    let metadata = original.0.clone();
    assert!(std::thread::spawn(move || {
        let _guard = metadata.lock().unwrap();
        panic!("synthetic command metadata poison");
    })
    .join()
    .is_err());
    assert!(!original.unissued_after_return());
}
