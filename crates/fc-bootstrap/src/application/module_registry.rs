//! M00's fixed resource-module composition, not a plugin system. The registry
//! selects original typed factories and negative stop hooks. Async completion is
//! observed from the original owner; requesting stop never counts as release.
//! Stateless domain/usecase modules retain their ordinary typed constructors.
use super::*;
use fc_presentation::registry::{Registration, Registry};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    AuthorityWriter,
    SharedWorkers,
    ClassroomDisplay,
    TeacherHttps,
    VoicePlayback,
    MaterialOpen,
}
const KINDS: [Kind; 6] = [
    Kind::AuthorityWriter,
    Kind::SharedWorkers,
    Kind::ClassroomDisplay,
    Kind::TeacherHttps,
    Kind::VoicePlayback,
    Kind::MaterialOpen,
];
// Unlike type-erased bags, a wrong registered factory cannot manufacture a
// different resource type or silently use a fallback constructor.
enum Factory {
    Writer(fn(&mut App, DataPaths, Arc<CurrentClock>) -> std::result::Result<(), String>),
    Workers(WorkerFactory),
    Desktop(fn(bool) -> std::result::Result<desktop_controller::Controller, String>),
    Https(fn() -> https_runtime_controller::State),
    Voice(fn(&mut App, VoiceService) -> std::result::Result<(), String>),
    Shell(fn() -> fc_windows::materials::shell::ShellLauncher),
}
struct WorkerFactory {
    create: fn() -> std::io::Result<shared_pool::SharedPool<Work>>,
    dispose:
        fn(shared_pool::SharedPool<Work>) -> std::result::Result<(), fc_runtime::worker::JoinError>,
}
struct Module {
    create: Factory,
    stop: fn(&mut App),
    settled: Option<fn(&App) -> bool>,
}
static MODULES: &[Registration<Kind, Module>] = &[
    Registration {
        key: Kind::AuthorityWriter,
        implementation: Module {
            create: Factory::Writer(open_writer),
            stop: App::schedule_shutdown,
            settled: Some(writer_settled),
        },
    },
    Registration {
        key: Kind::SharedWorkers,
        implementation: Module {
            create: Factory::Workers(WorkerFactory {
                create: shared_pool::SharedPool::start,
                dispose: shared_pool::SharedPool::join,
            }),
            stop: stop_workers,
            settled: Some(workers_settled),
        },
    },
    // stop() revokes publication and cancels preparation on the UI owner. Its
    // background tickets belong to SharedWorkers, not a second completion bit.
    Registration {
        key: Kind::ClassroomDisplay,
        implementation: Module {
            create: Factory::Desktop(desktop_controller::Controller::new),
            stop: stop_desktop,
            settled: None,
        },
    },
    Registration {
        key: Kind::TeacherHttps,
        implementation: Module {
            create: Factory::Https(https_runtime_controller::State::default),
            stop: App::stop_teacher_https,
            settled: Some(App::teacher_https_quiescent),
        },
    },
    Registration {
        key: Kind::VoicePlayback,
        implementation: Module {
            create: Factory::Voice(open_voice),
            stop: stop_voice,
            settled: Some(voice_settled),
        },
    },
    Registration {
        key: Kind::MaterialOpen,
        implementation: Module {
            create: Factory::Shell(fc_windows::materials::shell::ShellLauncher::new),
            stop: stop_shell,
            settled: Some(shell_settled),
        },
    },
];
fn module(kind: Kind) -> std::result::Result<&'static Module, String> {
    let registry = Registry::new(MODULES).map_err(|_| "模块注册重复或越界".to_owned())?;
    registry
        .require(&KINDS)
        .map_err(|_| "模块注册不完整".to_owned())?;
    registry.get(kind).map_err(|_| "模块未注册".to_owned())
}
pub(super) fn validate() -> std::result::Result<(), String> {
    module(Kind::AuthorityWriter).map(|_| ())
}
fn wrong_factory() -> String {
    "模块工厂类型不一致".into()
}
pub(super) fn create_workers() -> std::result::Result<shared_pool::SharedPool<Work>, String> {
    let Factory::Workers(factory) = &module(Kind::SharedWorkers)?.create else {
        return Err(wrong_factory());
    };
    (factory.create)().map_err(|_| "无法启动后台执行池".to_owned())
}
pub(super) fn create_desktop(
    probe: bool,
) -> std::result::Result<desktop_controller::Controller, String> {
    let Factory::Desktop(create) = module(Kind::ClassroomDisplay)?.create else {
        return Err(wrong_factory());
    };
    create(probe)
}
pub(super) fn create_https() -> std::result::Result<https_runtime_controller::State, String> {
    let Factory::Https(create) = module(Kind::TeacherHttps)?.create else {
        return Err(wrong_factory());
    };
    Ok(create())
}
pub(super) fn create_shell(
) -> std::result::Result<fc_windows::materials::shell::ShellLauncher, String> {
    let Factory::Shell(create) = module(Kind::MaterialOpen)?.create else {
        return Err(wrong_factory());
    };
    Ok(create())
}
pub(super) fn start_writer(
    app: &mut App,
    paths: DataPaths,
    clock: Arc<CurrentClock>,
) -> std::result::Result<(), String> {
    let Factory::Writer(create) = module(Kind::AuthorityWriter)?.create else {
        return Err(wrong_factory());
    };
    create(app, paths, clock)
}
pub(super) fn bind_voice(app: &mut App, voice: VoiceService) -> std::result::Result<(), String> {
    let Factory::Voice(create) = module(Kind::VoicePlayback)?.create else {
        return Err(wrong_factory());
    };
    create(app, voice)
}
pub(super) fn stop(kind: Kind, app: &mut App) {
    match module(kind) {
        Ok(module) => (module.stop)(app),
        Err(_) => app.terminate_required = true,
    }
}
fn settled(kind: Kind, app: &App) -> bool {
    module(kind)
        .ok()
        .and_then(|module| module.settled)
        .is_some_and(|settled| settled(app))
}
pub(super) fn shutdown_complete(app: &App) -> bool {
    // Keep the existing conjunction/order, including the restore coordinator.
    settled(Kind::AuthorityWriter, app)
        && app.backup.exit_ready()
        && settled(Kind::TeacherHttps, app)
        && settled(Kind::SharedWorkers, app)
        && settled(Kind::MaterialOpen, app)
        && settled(Kind::VoicePlayback, app)
}
fn open_writer(
    app: &mut App,
    paths: DataPaths,
    clock: Arc<CurrentClock>,
) -> std::result::Result<(), String> {
    app.submit(Work::Open(paths, clock))
}
fn writer_settled(app: &App) -> bool {
    app.backup.irreversible() || app.backend.is_none() || app.writer_closed
}
fn workers_settled(app: &App) -> bool {
    app.pool.is_finished()
}
fn shell_settled(app: &App) -> bool {
    app.shell.is_finished()
}
fn voice_settled(app: &App) -> bool {
    app.audio.as_ref().is_none_or(|audio| {
        audio
            .status()
            .is_ok_and(|s| s.phase == RuntimePhase::Closed && !s.physical_thread_running)
    })
}
fn stop_workers(app: &mut App) {
    app.pool.request_stop();
}
fn stop_desktop(app: &mut App) {
    app.desktop.stop();
}
fn stop_shell(app: &mut App) {
    app.shell.stop();
}
fn stop_voice(app: &mut App) {
    if let Some(audio) = &app.audio {
        audio.shutdown();
    }
}
fn open_voice(app: &mut App, voice: VoiceService) -> std::result::Result<(), String> {
    let audio = VoiceRuntime::new(Arc::new(NativeFactory::new(
        voice.clone(),
        app.root.clone(),
        app.clock.clone(),
    )));
    voice
        .set_waker(audio.weak_waker())
        .map_err(|_| "无法绑定声音执行域".to_owned())?;
    let _ = app.audio_slot.set(audio.clone());
    if app.stopping {
        audio.shutdown();
    }
    app.audio = Some(audio);
    app.voice_service = Some(voice);
    Ok(())
}

pub(super) fn join_workers(pool: shared_pool::SharedPool<Work>) -> std::result::Result<(), String> {
    let Factory::Workers(factory) = &module(Kind::SharedWorkers)?.create else {
        return Err(wrong_factory());
    };
    (factory.dispose)(pool).map_err(|_| "后台执行池关闭失败".to_owned())
}
