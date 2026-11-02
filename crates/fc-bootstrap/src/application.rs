//! Native product composition. All KDF/SQLite/file work runs on the one shared
//! two-thread pool; the window pump only handles bounded events and owned results.
mod autostart_controller;
mod autostart_editor;
mod autostart_probe;
mod backup_controller;
mod backup_editor;
mod backup_probe;
mod migration_controller;
mod migration_editor;
mod migration_probe;
mod migration_probe_fixture;
pub fn migration_probe() -> std::result::Result<(), String> {
    migration_probe::run()
}
pub fn migration_probe_reopen(root: &Path, nonce: &str) -> std::result::Result<(), String> {
    migration_probe::reopen(root, nonce)
}
mod reinitialize_probe;
pub fn reinitialization_probe() -> std::result::Result<(), String> {
    reinitialize_probe::run()
}
pub fn reinitialization_probe_stage(
    root: &Path,
    nonce: &str,
    stage: &str,
) -> std::result::Result<(), String> {
    reinitialize_probe::child(root, nonce, stage)
}
#[cfg(test)]
mod backup_shutdown_tests;
#[cfg(test)]
mod reinitialize_entry_tests;
pub fn recovery_probe() -> std::result::Result<(), String> {
    backup_probe::run()
}
pub fn recovery_probe_reopen(root: &Path, nonce: &str) -> std::result::Result<(), String> {
    backup_probe::reopen(root, nonce)
}
pub fn update_data_probe_stage(
    root: &Path,
    nonce: &str,
    stage: &str,
) -> std::result::Result<(), String> {
    crate::installation_startup::run(|| backup_probe::update_data_stage(root, nonce, stage))
}
mod http_business_probe;
mod http_executor;
pub fn tls_business_probe(thumbprint: &str, marker: &str) -> std::result::Result<(), String> {
    http_business_probe::run(thumbprint, marker)
}
mod shared_pool;
pub fn autostart_probe() -> std::result::Result<(), String> {
    autostart_probe::run()
}
mod calendar_probe;
pub fn calendar_probe() -> std::result::Result<(), String> {
    calendar_probe::run()
}
mod accounts_editor;
mod accounts_probe;
mod calendar_controller;
mod calendar_editor;
mod calendar_flow;
mod desktop_controller;
mod display_editor;
mod display_flow;
mod subject_visual_probe;
pub fn subject_visual_probe() -> std::result::Result<(), String> {
    subject_visual_probe::run()
}
#[cfg(test)]
mod material_batch_tests;
mod material_flow;
pub(crate) mod material_probe;
mod profile_content_editor;
mod profile_content_flow;
mod profile_content_probe;
mod profile_flow;
mod profiles_editor;
mod profiles_preview;
mod profiles_probe;
pub fn materials_probe() -> std::result::Result<(), String> {
    material_probe::run()
}
mod lesson_editor;
mod materials_editor;
mod public_desktop_probe;
mod public_lesson_editor;
pub fn public_desktop_probe() -> std::result::Result<(), String> {
    public_desktop_probe::run()
}
mod clock_source;
use clock_source::AppClock as CurrentClock;
mod teacher_browser_probe;
pub fn teacher_browser_probe(
    reference: &str,
    port: u16,
    old_package: &Path,
    new_package: &Path,
    marker: &str,
) -> std::result::Result<(), String> {
    teacher_browser_probe::run(reference, port, old_package, new_package, marker)
}
mod ordinary_https_probe;
pub fn ordinary_https_probe(
    reference: &str,
    port: u16,
    package: &Path,
    marker: &str,
) -> std::result::Result<(), String> {
    ordinary_https_probe::run(reference, port, package, marker)
}
mod https_runtime_controller;
mod https_settings_controller;
mod https_settings_probe;
pub fn https_settings_probe(reference: Option<&str>) -> std::result::Result<(), String> {
    https_settings_probe::run(reference)
}
mod https_settings_editor;
mod schedule_editor;
mod schedule_operations_probe;
mod voice_controller;
mod voice_editor;
mod voice_management_probe;
mod voice_output_probe;
mod web_editor;
pub fn voice_settings_probe() -> std::result::Result<(), String> {
    voice_management_probe::run()
}
pub fn voice_output_probe() -> std::result::Result<(), String> {
    voice_output_probe::run()
}
use fc_application::identity::{SessionLease, SessionPurpose};
use fc_bootstrap::backend::web::{WebService, WebServiceError};
use fc_bootstrap::backend::{Backend, BackendError, Initialize, SessionControl, VoiceService};
use fc_bootstrap::voice_runtime::{NativeFactory, RuntimePhase, StopReason, VoiceRuntime};
use fc_domain::desktop::{DesktopFacts, DisplaySettings};
use fc_domain::materials::{MaterialCursor, MaterialPage};
use fc_domain::schedule_document::ScheduleDocument;
use fc_domain::{auth::*, content::*, identity::*, ids::*};
use fc_ports::{MonotonicClock, RandomSource};
use fc_presentation::workbench::*;
use fc_runtime::worker::{JobContext, JobTicket, Task, WorkerPool};
use fc_security::{CredentialRecordCodec, Password, RecoveryCode};
use fc_windows::{
    host::{HiddenHost, HostHints},
    system::{CurrentUserDpapi, DataPaths, SecretBytes},
    workbench::Window,
    InstanceGuard, SystemRandom,
};
use schedule_editor::{Editor as ScheduleEditor, EditorCommand};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

const SCHOOL: FieldId = FieldId(1);
const LOGIN: FieldId = FieldId(2);
const NAME: FieldId = FieldId(3);
const PASSWORD: FieldId = FieldId(4);
const CONFIRM: FieldId = FieldId(5);
const NEW_PASSWORD: FieldId = FieldId(6);
const CODE: FieldId = FieldId(7);
const CLASS: FieldId = FieldId(8);
const SUBJECT: FieldId = FieldId(9);
const COLOR: FieldId = FieldId(11);
const OWNER: FieldId = FieldId(13);
const CONTENT: FieldId = FieldId(14);
const SAVE: ActionId = ActionId(1);
const BACK: ActionId = ActionId(2);
const LOG_IN: ActionId = ActionId(3);
const TO_RECOVERY: ActionId = ActionId(4);
const ACK: ActionId = ActionId(5);
const LOCK: ActionId = ActionId(6);
const TO_SCHOOL: ActionId = ActionId(7);
const TO_SUBJECTS: ActionId = ActionId(8);
const TO_ACCOUNTS: ActionId = ActionId(9);
const TO_HOMEWORK: ActionId = ActionId(10);
const TO_PASSWORD: ActionId = ActionId(11);
const EXIT: ActionId = ActionId(12);
const ADD_CLASS: ActionId = ActionId(13);
const BIND_CLASS: ActionId = ActionId(14);
const LOAD: ActionId = ActionId(15);
const TO_ROTATE: ActionId = ActionId(16);
const TO_SCHEDULE: ActionId = ActionId(17);
const TO_DISPLAY: ActionId = ActionId(18);
const TO_MATERIALS: ActionId = ActionId(21);
const TO_LESSON: ActionId = ActionId(22);
const TO_BACKUP: ActionId = ActionId(26);
const TO_WEB: ActionId = ActionId(23);
const TO_PROFILES: ActionId = ActionId(24);
const TO_AUTOSTART: ActionId = ActionId(25);
const TO_REINITIALIZE: ActionId = ActionId(27);
const TO_VOICE: ActionId = ActionId(28);
const RETURN_DRAFT: ActionId = ActionId(19);
const DISCARD_EXIT: ActionId = ActionId(20);
const MANIFEST: &[u8] = b"FutureClassroom.Deployment.v1\n";

mod module_registry;
mod settings_registry;
mod window_registry;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Starting,
    Setup,
    Login,
    Recovery,
    Code,
    Home,
    School,
    Subjects,
    Accounts,
    Password,
    Rotate,
    Homework,
    HomeworkEdit,
    Schedule,
    Calendar,
    Autostart,
    Backup,
    Restoring,
    Display,
    Profiles,
    ProfileContent,
    Materials,
    Lesson,
    Voice,
    Web,
    HttpsSettings,
    ExitDraft,
    Repair,
}
impl Screen {
    const ALL: &'static [Self] = &[
        Self::Starting,
        Self::Setup,
        Self::Login,
        Self::Recovery,
        Self::Code,
        Self::Home,
        Self::School,
        Self::Subjects,
        Self::Accounts,
        Self::Password,
        Self::Rotate,
        Self::Homework,
        Self::HomeworkEdit,
        Self::Schedule,
        Self::Calendar,
        Self::Autostart,
        Self::Backup,
        Self::Restoring,
        Self::Display,
        Self::Profiles,
        Self::ProfileContent,
        Self::Materials,
        Self::Lesson,
        Self::Voice,
        Self::Web,
        Self::HttpsSettings,
        Self::ExitDraft,
        Self::Repair,
    ];
}

#[derive(Debug)]
enum Failure {
    Backend(BackendError),
    Autostart(fc_bootstrap::autostart::Error),
    Web(WebServiceError),
    Input(&'static str),
    Deployment(&'static str),
    Stopped,
    #[cfg(test)]
    LostCompletedHistoryForTest,
}
fn fatal(error: &Failure) -> bool {
    if let Failure::Autostart(fc_bootstrap::autostart::Error::Unsettled { error, .. }) = error {
        return fatal(&Failure::Backend((**error).clone()));
    }
    use fc_storage::StorageError as S;
    matches!(
        error,
        Failure::Backend(
            BackendError::Identity(fc_ports::IdentityRepositoryError::Faulted)
                | BackendError::Content(fc_ports::RepositoryError::Faulted)
                | BackendError::Schedule(fc_ports::ScheduleRepositoryError::Faulted)
                | BackendError::Calendar(fc_ports::CalendarRepositoryError::Faulted)
                | BackendError::Autostart(fc_ports::autostart::AutostartRepositoryError::Faulted)
                | BackendError::Lesson(fc_ports::LessonRepositoryError::Faulted)
                | BackendError::Display(fc_ports::DisplayRepositoryError::Faulted)
                | BackendError::SubjectVisual(fc_ports::SubjectVisualRepositoryError::Faulted)
                | BackendError::HttpsSettings(fc_ports::HttpsSettingsRepositoryError::Faulted)
                | BackendError::Profile(fc_ports::ProfileRepositoryError::Faulted)
                | BackendError::ProfileContent(fc_ports::ProfileContentRepositoryError::Faulted)
                | BackendError::Material(fc_ports::MaterialRepositoryError::Faulted)
                | BackendError::Voice(fc_ports::voice::VoiceRepositoryError::Faulted)
                | BackendError::Storage(
                    S::AuthorityUnavailable
                        | S::Indeterminate
                        | S::WorkerPanicked
                        | S::PublicationFailed { .. }
                )
        )
    )
}
impl From<BackendError> for Failure {
    fn from(e: BackendError) -> Self {
        Self::Backend(e)
    }
}
impl From<fc_bootstrap::autostart::Error> for Failure {
    fn from(error: fc_bootstrap::autostart::Error) -> Self {
        match error {
            fc_bootstrap::autostart::Error::Backend(error) => Self::Backend(error),
            other => Self::Autostart(other),
        }
    }
}
type Result<T> = std::result::Result<T, Failure>;
fn invalid(message: &'static str) -> Failure {
    Failure::Input(message)
}
fn command_id() -> Result<CommandId> {
    for _ in 0..4 {
        let mut bytes = [0u8; 8];
        SystemRandom
            .fill(&mut bytes)
            .map_err(|_| invalid("系统随机源不可用。"))?;
        if let Ok(id) = CommandId::new(u64::from_le_bytes(bytes)) {
            return Ok(id);
        }
    }
    Err(invalid("无法生成本次操作标识。"))
}
fn text(data: &FormData, id: FieldId) -> Result<String> {
    data.get(id)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("请填写必填项。"))
}
fn password(data: &mut FormData, id: FieldId) -> Result<Password> {
    Password::new(data.take(id).ok_or_else(|| invalid("请输入口令。"))?)
        .map_err(|_| invalid("口令须为12至128个字符。"))
}
fn confirm(data: &FormData, left: FieldId) -> Result<()> {
    if data.get(left) != data.get(CONFIRM) {
        Err(invalid("两次输入的口令不一致。"))
    } else {
        Ok(())
    }
}
fn error_text(error: &Failure) -> String {
    match error {
        #[cfg(test)]
        Failure::LostCompletedHistoryForTest => "测试丢弃原完成交付；请查询原命令结果。".into(),
        Failure::Input(s) | Failure::Deployment(s) => (*s).into(),
        Failure::Autostart(fc_bootstrap::autostart::Error::Busy) => "启动项仍在核对，请稍后重新读取。".into(),
        Failure::Autostart(_) => "本次启动项结果尚未确认，请重新打开登录自启动并核对实际状态。".into(),
        Failure::Stopped => "未取得完成结果，请重新打开并核对已保存内容。".into(),
        Failure::Web(WebServiceError::Denied) => "教师设备权限或本地会话已变化，请重新登录并刷新核对。".into(),
        Failure::Web(WebServiceError::Busy) => "教师设备状态正在更新，请稍后刷新。".into(),
        Failure::Web(_) => "教师设备操作未完成，请刷新核对当前状态。".into(),
        Failure::Backend(e) => match e {
            BackendError::HttpsSettings(fc_ports::HttpsSettingsRepositoryError::Domain(fc_domain::https_settings::HttpsSettingsError::ConfigurationUnavailable)) => "教师会话准入仍开启或另一配置任务未结束。请返回设备管理核对并明确停用后再保存。".into(),
            BackendError::HttpsSettings(_) => "配置的权限或版本已变化，请保留草稿并核对原操作结果。".into(),
            BackendError::SubjectVisual(_) => "科目配色或显示设置的权限、版本已变化，本次未确认保存。草稿保留，请核对后明确取消并重新加载。".into(),
            BackendError::Voice(_) => "语音权限、任务或版本已变化，本次结果尚未确认，请刷新核对后再操作。".into(),
            BackendError::Autostart(_) => "启动项的权限、版本或实际状态已变化，请重新读取、核对后再明确确认。".into(),
            BackendError::Calendar(fc_ports::CalendarRepositoryError::Cancelled) => "本次校历任务已取消或超过处理期限，请重新读取当前状态。".into(),
            BackendError::Calendar(fc_ports::CalendarRepositoryError::Stale) => "校历候选或课表已更新，请重新加载、核对差异后再确认。".into(),
            BackendError::Calendar(fc_ports::CalendarRepositoryError::Capacity) => "校历缓存或采用资料超过容量上限，本次未生效，请保留数据并核对。".into(),
            BackendError::Calendar(fc_ports::CalendarRepositoryError::Denied) => "校历管理需要当前有效的本机主管理员会话；学校模式下须先切换并保存公共模式才能采用。".into(),
            BackendError::Calendar(_) => "本次校历操作未确认完成，请重新加载并核对缓存及已采用资料。".into(),
            BackendError::InvalidCredentials => "登录名或口令不正确。".into(),
            BackendError::Lesson(fc_ports::LessonRepositoryError::Domain(fc_domain::lesson_context::LessonContextError::LessonContextChanged)) => "本节课程已结束或课表已变化，本次未保存。请重新加载当前课程。".into(),
            BackendError::Lesson(fc_ports::LessonRepositoryError::Domain(fc_domain::lesson_context::LessonContextError::RevisionConflict { .. })) => "本节临时安排已被修改，本次未保存。请重新加载当前课程。".into(),
            BackendError::Lesson(fc_ports::LessonRepositoryError::Domain(fc_domain::lesson_context::LessonContextError::InvalidTarget)) => "请选择本班有效科目及任教教师。".into(),
            BackendError::Schedule(fc_ports::ScheduleRepositoryError::Domain(e)) => schedule_editor::problem(e.clone()),
            BackendError::Display(fc_ports::DisplayRepositoryError::Domain(e)) => display_editor::problem(e),
            BackendError::Profile(fc_ports::ProfileRepositoryError::Domain(fc_domain::profile::ProfileError::RevisionConflict { .. })) => "布局或科目配置已被修改。当前草稿保留，请重新加载并核对后再保存。".into(),
            BackendError::Profile(fc_ports::ProfileRepositoryError::Domain(fc_domain::profile::ProfileError::FolderMismatch)) => "资料目录不属于所选老师与科目，本次未保存。".into(),
            BackendError::Profile(fc_ports::ProfileRepositoryError::Domain(fc_domain::profile::ProfileError::InvalidScope)) => "请选择有效的班级、科目与任教老师；停用后的历史配置不会用于当前场景。".into(),
            BackendError::Profile(fc_ports::ProfileRepositoryError::DocumentTooLarge) => "布局或科目配置超过64KiB文档上限，本次未保存，草稿仍保留。".into(),
            BackendError::Profile(fc_ports::ProfileRepositoryError::Display(e)) => display_editor::problem(e),
            BackendError::ProfileContent(e) => profile_content_flow::problem(e).into(),
            BackendError::Schedule(fc_ports::ScheduleRepositoryError::DocumentTooLarge) => "课表和保留历史超过本阶段64KiB文档上限。请减少学期、行或备注；草稿仍保留，未截断或覆盖。".into(),
            BackendError::Storage(fc_storage::StorageError::LoginRateLimited {
                retry_after_millis,
            }) => format!("请等待{}秒后重试。", retry_after_millis.div_ceil(1000)),
            BackendError::Session(_) => "本地会话已失效，请重新登录。".into(),
            BackendError::Identity(fc_ports::IdentityRepositoryError::Domain(
                IdentityError::RevisionConflict,
            )) => "资料已被其他操作修改，请返回并重新打开；当前草稿仍保留在此页。".into(),
            BackendError::Content(fc_ports::RepositoryError::Domain(
                ContentError::RevisionConflict { .. },
            )) => "作业版本已更新，请重新加载后再保存。".into(),
            BackendError::Content(fc_ports::RepositoryError::Domain(ContentError::WrongDate)) => {
                "日期已变化，请重新选择今天的作业。".into()
            }
            BackendError::CorruptCredential
            | BackendError::Storage(
                fc_storage::StorageError::Database | fc_storage::StorageError::CredentialCorrupt,
            ) => "数据或凭据需要修复；请使用有效恢复码或受控备份，不会重新开放首次注册。".into(),
            BackendError::Identity(fc_ports::IdentityRepositoryError::Domain(IdentityError::LastAdministrator)) => "必须保留至少一名启用的主管理员；请先创建或启用另一名主管理员。".into(),
            BackendError::Identity(fc_ports::IdentityRepositoryError::Domain(IdentityError::InvalidScope)) => "任教范围无效或仍被课表、单双周保留历史或默认教师引用。请先修正引用后再保存；本次未修改账号，草稿仍保留。".into(),
            BackendError::Identity(fc_ports::IdentityRepositoryError::Domain(IdentityError::Capacity)) => "账号或任教关系已超过数量上限或实际64KiB身份文档容量。本次未保存，草稿仍保留；请减少条目或文字。".into(),
            BackendError::Identity(fc_ports::IdentityRepositoryError::Domain(
                IdentityError::DuplicateName,
            )) => "该登录名或名称已存在。".into(),
            _ => "操作未完成。请核对输入、任教范围和当前会话后重试。".into(),
        },
    }
}

/// The deployment marker makes missing key/database evidence fail closed. A
/// partially prepared directory is never mistaken for a brand-new installation.
#[derive(Clone, Copy)]
enum OpenPolicy {
    AllowInitialization,
    ExistingOnly,
}
fn verify_existing_deployment(root: &Path) -> Result<()> {
    if read_bounded(&root.join("config/deployment.marker"), 128)?.as_slice() != MANIFEST {
        return Err(Failure::Deployment(
            "部署标记损坏，需要修复；不会创建新学校。",
        ));
    }
    // These pins reject junctions and hard links, without reading school data or
    // allowing the missing files to be recreated before the normal open checks.
    for name in ["config/credential-key.dpapi", "state/classroom.sqlite3"] {
        let pin = fc_windows::materials::files::OwnedObjectPin::file_allow_empty(&root.join(name))
            .map_err(|_| Failure::Deployment("部署数据不完整，需要修复；不会创建新学校。"))?;
        if pin
            .file()
            .metadata()
            .map_err(|_| Failure::Deployment("部署文件无法读取。"))?
            .len()
            == 0
        {
            return Err(Failure::Deployment(
                "部署数据不完整，需要修复；不会创建新学校。",
            ));
        }
    }
    Ok(())
}
fn open_backend(root: &Path, clock: Arc<CurrentClock>, policy: OpenPolicy) -> Result<Backend> {
    let key_path = root.join("config/credential-key.dpapi");
    let database = root.join("state/classroom.sqlite3");
    let marker = root.join("config/deployment.marker");
    let key_exists = deployment_object_exists(&key_path)?;
    let db_exists = deployment_object_exists(&database)?;
    let marker_exists = deployment_object_exists(&marker)?;
    let dpapi = CurrentUserDpapi::default();
    let codec = if marker_exists {
        verify_existing_deployment(root)?;
        let encrypted = read_bounded(&key_path, 4096)?;
        let secret = dpapi.unprotect(&encrypted).map_err(|_| {
            Failure::Deployment("当前Windows账号无法解密部署凭据，请使用原部署账号或受控恢复。")
        })?;
        let key = <&[u8] as TryInto<[u8; 32]>>::try_into(secret.expose())
            .map_err(|_| Failure::Deployment("部署密钥格式损坏。"))?;
        CredentialRecordCodec::from_key(key)
            .map_err(|_| Failure::Deployment("部署密钥格式损坏。"))?
    } else {
        if matches!(policy, OpenPolicy::ExistingOnly) {
            return Err(Failure::Deployment(
                "旧部署缺少完整资料，需要修复；不会在旧目录新建学校。",
            ));
        }
        if key_exists || db_exists {
            return Err(Failure::Deployment(
                "发现未完成或损坏的部署，需要修复；不会覆盖已有数据。",
            ));
        }
        fc_storage::business::startup_state(&database)
            .map_err(|_| Failure::Deployment("无法创建受控数据文件。"))?;
        let codec = CredentialRecordCodec::generate(&mut SystemRandom)
            .map_err(|_| Failure::Deployment("系统随机源不可用。"))?;
        let encrypted = codec
            .protect_key(|key| {
                let secret = SecretBytes::new(key.to_vec())?;
                dpapi.protect(&secret)
            })
            .map_err(|_| Failure::Deployment("无法保护部署凭据。"))?;
        create_file(&key_path, &encrypted)?;
        create_file(&marker, MANIFEST)?;
        codec
    };
    Backend::open(database, codec, clock.clone(), clock).map_err(Failure::from)
}
fn deployment_object_exists(path: &Path) -> Result<bool> {
    // A broken link is still an existing object; following it must not turn a
    // damaged deployment into permission to create a new key or database.
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Failure::Deployment("无法读取部署状态。")),
    }
}
fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>> {
    let mut pin = fc_windows::materials::files::OwnedObjectPin::file_allow_empty(path)
        .map_err(|_| Failure::Deployment("部署文件无法读取。"))?;
    let metadata = pin
        .file()
        .metadata()
        .map_err(|_| Failure::Deployment("部署文件无法读取。"))?;
    if metadata.len() > max as u64 {
        return Err(Failure::Deployment("部署文件大小或类型无效。"));
    }
    let mut data = Vec::with_capacity(max.min(4096));
    pin.file_mut()
        .take(max as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| Failure::Deployment("部署文件无法读取。"))?;
    if data.len() > max {
        return Err(Failure::Deployment("部署文件过大。"));
    }
    Ok(data)
}
fn create_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| Failure::Deployment("部署文件已存在或无法创建，请检查数据后修复。"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Failure::Deployment("部署文件未能持久保存，请检查磁盘。"))
}

enum Outcome {
    Http(fc_bootstrap::web_transfer::HttpCompletion),
    HttpProbe(Box<http_business_probe::Completion>),
    Backup(backup_controller::Completed),
    Opened(
        Arc<Mutex<Backend>>,
        SessionControl,
        IdentityState,
        VoiceService,
    ),
    ExitAuthorized,
    Updated {
        state: IdentityState,
        lease: Option<SessionLease>,
        code: Option<RecoveryCode>,
        next: Screen,
    },
    Loaded {
        key: ContentKey,
        record: Option<ContentRecord>,
    },
    AccountsLoaded(IdentityState),
    AccountHistoryLoaded {
        lease: SessionLease,
        current: IdentityState,
        history: Box<NativeHistory>,
    },
    SubjectsLoaded {
        facts: settings_registry::SubjectFacts,
        history: Option<Box<NativeHistory>>,
        saved: bool,
    },
    SubjectSelfRevoked {
        state: IdentityState,
        history: Box<NativeHistory>,
    },
    DefaultTeacherLoaded {
        current: IdentityState,
        history: Box<NativeHistory>,
        saved: bool,
    },
    AccountSaved {
        state: IdentityState,
        self_changed: bool,
    },
    ScheduleLoaded(ScheduleDocument),
    ScheduleSaved(ScheduleDocument),
    Calendar(Box<calendar_flow::Loaded>),
    Autostart(Option<PrincipalId>, Box<fc_bootstrap::autostart::Loaded>),
    CalendarMaintained {
        attempted: bool,
    },
    PublicLessonLoaded(fc_ports::DesktopLessonSource),
    PublicLessonSaved(fc_ports::DesktopLessonSource),
    LessonLoaded(fc_domain::lesson_context::LessonContextSnapshot),
    LessonSaved(fc_domain::lesson_context::LessonContextSnapshot),
    DisplayLoaded(fc_ports::SubjectVisualState, Arc<IdentityState>),
    DisplaySaved(fc_ports::SubjectVisualState, Arc<IdentityState>),
    DisplayPreview {
        sequence: u64,
        source_revision: Revision,
        image: fc_windows::workbench::LayoutPreview,
    },
    Profile(Box<profile_flow::Loaded>),
    Desktop(
        Box<DesktopFacts>,
        fc_bootstrap::profile_content_cache::TextFrame,
    ),
    ContentImages(profile_content_flow::ImageCompletion),
    ContentPage {
        generation: SceneGeneration,
        snapshot: fc_domain::profile_content::PublishedContentSnapshot,
    },
    ProfileContent(Box<profile_content_flow::Loaded>),
    ContentMaintained {
        pending: bool,
    },
    Materials(Box<MaterialPage>),
    MaterialBatch(Box<material_flow::BatchResult>),
    MaterialLaunch(Box<material_flow::PreparedLaunch>),
    MaterialCleaned,
    LaunchCopies(material_flow::LaunchCopyReport),
    HttpsRuntime(Box<https_runtime_controller::Prepared>),
    Https(Box<https_settings_controller::Completion>),
    WebOpened(WebService, web_editor::Loaded),
    WebLoaded(web_editor::Loaded),
    VoiceLoaded(SessionLease, Box<voice_controller::Loaded>),
    Closed,
    Office(fc_materials::office::ContainerReport),
}
struct Request {
    backend: Arc<Mutex<Backend>>,
    screen: Screen,
    action: ActionId,
    data: FormData,
    lease: Option<SessionLease>,
    state: Arc<IdentityState>,
    key: Option<ContentKey>,
    revision: Revision,
    clock: Arc<CurrentClock>,
}
/// Immutable identity metadata outlives worker output loss. UI reads are
/// nonblocking; no metadata lock survives a sole-Writer or native wait.
/// The original client request is retained independently of its current-state
/// view. It is metadata only; only the backend can hash/admit this typed intent.
#[derive(Clone, Debug, PartialEq, Eq)]
enum NativeIntent {
    Account {
        expected: Revision,
        // Shared public metadata: NativeCommand clones never clone scope arrays.
        command: Arc<IdentityCommand>,
    },
    Subject {
        expected: Revision,
        command: IdentityCommand,
    },
    DefaultTeacher {
        expected: Revision,
        default: fc_domain::identity::DefaultTeacher,
    },
    VoicePolicy(voice_controller::PolicyIntent),
    CalendarPolicy {
        year: i32,
        expected: Revision,
        enabled: bool,
    },
    CalendarDownload {
        year: i32,
    },
}
#[derive(Clone, Debug)]
struct NativeCommand(
    Arc<Mutex<Option<fc_ports::command_window::IssuedCommand>>>,
    Option<NativeIntent>,
    Arc<Mutex<Option<Arc<fc_bootstrap::backend::AccountReceiptProof>>>>,
);
impl PartialEq for NativeCommand {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for NativeCommand {}
impl NativeCommand {
    fn new() -> Self {
        Self::with_intent(None)
    }
    fn with_intent(intent: Option<NativeIntent>) -> Self {
        Self(
            Arc::new(Mutex::new(None)),
            intent,
            Arc::new(Mutex::new(None)),
        )
    }
    fn intent(&self) -> Option<&NativeIntent> {
        self.1.as_ref()
    }
    fn bind_account_proof(&self, proof: fc_bootstrap::backend::AccountReceiptProof) -> Result<()> {
        let Some(NativeIntent::Account { expected, command }) = self.intent() else {
            return Err(Failure::Stopped);
        };
        if self.original()? != proof.issued()
            || proof.request().expected_revision != *expected
            || &proof.request().command != command.as_ref()
        {
            return Err(Failure::Stopped);
        }
        let mut slot = self.2.lock().map_err(|_| Failure::Stopped)?;
        if slot
            .as_ref()
            .is_some_and(|original| original.as_ref() != &proof)
        {
            return Err(Failure::Stopped);
        }
        *slot = Some(Arc::new(proof));
        Ok(())
    }
    fn account_proof(&self) -> Result<Option<Arc<fc_bootstrap::backend::AccountReceiptProof>>> {
        Ok(self.2.lock().map_err(|_| Failure::Stopped)?.clone())
    }
    fn is_account(&self) -> bool {
        matches!(self.intent(), Some(NativeIntent::Account { .. }))
    }
    fn permits_current_read_confirmation(&self) -> bool {
        self.intent().is_none()
    }
    fn snapshot(&self) -> Option<fc_ports::command_window::IssuedCommand> {
        self.0.try_lock().ok().and_then(|value| *value)
    }
    /// Only for a physically returned worker. A failed/poisoned metadata lock
    /// cannot prove that issuance never occurred.
    fn unissued_after_return(&self) -> bool {
        self.0.try_lock().is_ok_and(|value| value.is_none())
    }
    fn read_worker(&self) -> Result<Option<fc_ports::command_window::IssuedCommand>> {
        Ok(*self.0.lock().map_err(|_| Failure::Stopped)?)
    }
    fn original(&self) -> Result<fc_ports::command_window::IssuedCommand> {
        self.read_worker()?
            .ok_or_else(|| invalid("原操作尚未取得服务器票据，未发送写入；不会自动重试。"))
    }
    fn bind(&self, issued: fc_ports::command_window::IssuedCommand) -> Result<()> {
        let mut value = self.0.lock().map_err(|_| Failure::Stopped)?;
        if value.is_some_and(|original| original != issued) {
            return Err(Failure::Stopped);
        }
        *value = Some(issued);
        Ok(())
    }
    fn issue(
        &self,
        backend: &Backend,
        family: fc_ports::command_window::CommandFamily,
        lease: Option<SessionLease>,
        purpose: SessionPurpose,
        context: &JobContext,
    ) -> Result<fc_ports::command_window::IssuedCommand> {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        let issued = match self.read_worker()? {
            Some(original) => original,
            None => {
                let issued = backend.issue_native_command(family, lease, purpose)?;
                self.bind(issued)?;
                issued
            }
        };
        if issued.family != family {
            return Err(Failure::Stopped);
        }
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        Ok(issued)
    }
    fn run<T>(
        &self,
        backend: &mut Backend,
        family: fc_ports::command_window::CommandFamily,
        lease: Option<SessionLease>,
        purpose: SessionPurpose,
        context: &JobContext,
        action: impl FnOnce(&mut Backend) -> Result<T>,
    ) -> Result<T> {
        let mut execution = backend.begin_execution_scope(Arc::new(context.execution_fence()));
        let issued = self.issue(&execution, family, lease, purpose, context)?;
        let mut scope = execution.begin_command_scope(issued)?;
        action(&mut scope)
    }
    fn query<T>(
        &self,
        backend: &mut Backend,
        context: &JobContext,
        action: impl FnOnce(&mut Backend) -> Result<T>,
    ) -> Result<T> {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        let issued = self.original()?;
        let mut execution = backend.begin_execution_scope(Arc::new(context.execution_fence()));
        let mut scope = execution.begin_command_scope(issued)?;
        action(&mut scope)
    }
}
/// A bounded historical outcome carries the exact original command metadata.
/// It is not an execution permit, current snapshot, or physical-release proof.
struct NativeHistory {
    command: NativeCommand,
    issued: fc_ports::command_window::IssuedCommand,
    resolution: NativeResolution,
}
enum NativeResolution {
    Account(Option<fc_bootstrap::backend::OriginalAccountResult>),
    // An inaccessible original owner or missing local preparation proof is NOT
    // proof of failure/success. A current directory may still be shown separately.
    AccountUnavailable,
    SubjectRejected(IdentityError),
    Subject(Option<(Revision, fc_domain::identity::SchoolSubject)>),
    DefaultTeacherRejected(IdentityError),
    DefaultTeacher(Option<(Revision, fc_domain::identity::DefaultTeacher)>),
    Outcome(fc_ports::command_window::TypedCommandOutcome),
    Expired,
}
impl NativeHistory {
    fn completed(
        command: NativeCommand,
        issued: fc_ports::command_window::IssuedCommand,
        outcome: fc_ports::command_window::TypedCommandOutcome,
    ) -> Result<Self> {
        let history = Self {
            command,
            issued,
            resolution: NativeResolution::Outcome(outcome),
        };
        if !history.valid_result() {
            return Err(Failure::Stopped);
        }
        Ok(history)
    }
    fn valid_result(&self) -> bool {
        use fc_ports::command_window::{TypedCommandOutcome as O, TypedCommandResult as R};
        match (&self.resolution, self.command.intent()) {
            (
                NativeResolution::Account(saved),
                Some(NativeIntent::Account { expected, command }),
            ) => saved
                .as_ref()
                .is_none_or(|result| result.matches(*expected, command)),
            (NativeResolution::Account(_), _) => false,
            (NativeResolution::AccountUnavailable, Some(NativeIntent::Account { .. })) => true,
            (NativeResolution::AccountUnavailable, _) => false,
            (NativeResolution::Expired, Some(NativeIntent::Account { .. })) => true,
            (_, Some(NativeIntent::Account { .. })) => false,
            (
                NativeResolution::SubjectRejected(
                    IdentityError::RevisionConflict
                    | IdentityError::InvalidScope
                    | IdentityError::Capacity
                    | IdentityError::InvalidText
                    | IdentityError::DuplicateName
                    | IdentityError::DuplicateId
                    | IdentityError::NotFound
                    | IdentityError::VersionOverflow,
                ),
                Some(NativeIntent::Subject { .. }),
            ) => true,
            (NativeResolution::SubjectRejected(_), _) => false,
            (
                NativeResolution::Subject(saved),
                Some(NativeIntent::Subject {
                    expected,
                    command:
                        IdentityCommand::PutSubject {
                            id,
                            name,
                            enabled,
                            color,
                            expected_revision,
                        },
                }),
            ) => saved.as_ref().is_none_or(|(revision, subject)| {
                expected.next().ok() == Some(*revision)
                    && subject.id == *id
                    && subject.name == *name
                    && subject.enabled == *enabled
                    && subject.color == *color
                    && expected_revision.unwrap_or_default().next().ok() == Some(subject.revision)
            }),
            (NativeResolution::Subject(_), _) => false,
            (NativeResolution::Expired, Some(NativeIntent::Subject { .. })) => true,
            (_, Some(NativeIntent::Subject { .. })) => false,
            (
                NativeResolution::DefaultTeacherRejected(
                    IdentityError::RevisionConflict
                    | IdentityError::InvalidScope
                    | IdentityError::Capacity,
                ),
                Some(NativeIntent::DefaultTeacher { .. }),
            ) => true,
            (NativeResolution::DefaultTeacherRejected(_), _) => false,
            (
                NativeResolution::DefaultTeacher(saved),
                Some(NativeIntent::DefaultTeacher { expected, default }),
            ) => saved.is_none_or(|(revision, result)| {
                expected.next().ok() == Some(revision) && result == *default
            }),
            (NativeResolution::DefaultTeacher(_), _) => false,
            (NativeResolution::Expired, Some(NativeIntent::DefaultTeacher { .. })) => true,
            (_, Some(NativeIntent::DefaultTeacher { .. })) => false,
            (
                NativeResolution::Outcome(O::Completed(R::VoicePolicy(_))),
                Some(NativeIntent::VoicePolicy(_)),
            ) => true,
            (
                NativeResolution::Outcome(O::Completed(R::CalendarPolicy(_))),
                Some(NativeIntent::CalendarPolicy { .. }),
            ) => true,
            (
                NativeResolution::Outcome(O::Completed(R::CalendarManualFetch(result))),
                Some(NativeIntent::CalendarDownload { year }),
            ) => result.year == *year,
            (NativeResolution::Outcome(O::Completed(_)), _) => false,
            (_, Some(_)) => true,
            (_, None) => false,
        }
    }
    fn account(
        command: NativeCommand,
        issued: fc_ports::command_window::IssuedCommand,
        result: fc_bootstrap::backend::Result<Option<fc_bootstrap::backend::OriginalAccountResult>>,
    ) -> Result<Self> {
        use fc_ports::command_window::CommandWindowError;
        let resolution = match result {
            Ok(value) => NativeResolution::Account(value),
            Err(error) if error.command_window_error() == Some(CommandWindowError::Expired) => {
                NativeResolution::Expired
            }
            Err(error) if error.command_window_error() == Some(CommandWindowError::NotIssued) => {
                NativeResolution::AccountUnavailable
            }
            Err(error) => return Err(error.into()),
        };
        let history = Self {
            command,
            issued,
            resolution,
        };
        if !history.valid_result() {
            return Err(Failure::Stopped);
        }
        Ok(history)
    }
    fn subject(
        command: NativeCommand,
        issued: fc_ports::command_window::IssuedCommand,
        result: fc_bootstrap::backend::Result<
            Option<(Revision, fc_domain::identity::SchoolSubject)>,
        >,
    ) -> Result<Self> {
        let resolution = match result {
            Ok(value) => NativeResolution::Subject(value),
            Err(error)
                if error.command_window_error()
                    == Some(fc_ports::command_window::CommandWindowError::Expired) =>
            {
                NativeResolution::Expired
            }
            Err(error) => return Err(error.into()),
        };
        let history = Self {
            command,
            issued,
            resolution,
        };
        if !history.valid_result() {
            return Err(Failure::Stopped);
        }
        Ok(history)
    }
    fn subject_rejected(
        command: NativeCommand,
        issued: fc_ports::command_window::IssuedCommand,
        error: IdentityError,
    ) -> Result<Self> {
        let history = Self {
            command,
            issued,
            resolution: NativeResolution::SubjectRejected(error),
        };
        if !history.valid_result() {
            return Err(Failure::Stopped);
        }
        Ok(history)
    }
    fn default_teacher_rejection(
        command: NativeCommand,
        issued: fc_ports::command_window::IssuedCommand,
        error: IdentityError,
    ) -> Result<Self> {
        let history = Self {
            command,
            issued,
            resolution: NativeResolution::DefaultTeacherRejected(error),
        };
        if !history.valid_result() {
            return Err(Failure::Stopped);
        }
        Ok(history)
    }
    fn default_teacher(
        command: NativeCommand,
        issued: fc_ports::command_window::IssuedCommand,
        result: fc_bootstrap::backend::Result<
            Option<(Revision, fc_domain::identity::DefaultTeacher)>,
        >,
    ) -> Result<Self> {
        let resolution = match result {
            Ok(value) => NativeResolution::DefaultTeacher(value),
            Err(error)
                if error.command_window_error()
                    == Some(fc_ports::command_window::CommandWindowError::Expired) =>
            {
                NativeResolution::Expired
            }
            Err(error) => return Err(error.into()),
        };
        let history = Self {
            command,
            issued,
            resolution,
        };
        if !history.valid_result() {
            return Err(Failure::Stopped);
        }
        Ok(history)
    }
    fn from_query(
        command: NativeCommand,
        issued: fc_ports::command_window::IssuedCommand,
        result: fc_bootstrap::backend::Result<fc_ports::command_window::TypedCommandOutcome>,
    ) -> Result<Self> {
        match result {
            Ok(outcome) => Self::completed(command, issued, outcome),
            Err(error)
                if error.command_window_error()
                    == Some(fc_ports::command_window::CommandWindowError::Expired) =>
            {
                Ok(Self {
                    command,
                    issued,
                    resolution: NativeResolution::Expired,
                })
            }
            Err(error) => Err(error.into()),
        }
    }
    fn pending(&self) -> bool {
        // An expired account query cannot resolve whether its original write
        // committed. Only explicit end-query may unblock a new intent.
        (self.command.is_account() && matches!(self.resolution, NativeResolution::Expired))
            || matches!(
                self.resolution,
                NativeResolution::Outcome(fc_ports::command_window::TypedCommandOutcome::Pending)
                    | NativeResolution::DefaultTeacher(None)
                    | NativeResolution::Subject(None)
                    | NativeResolution::Account(None)
                    | NativeResolution::AccountUnavailable
            )
    }
    fn message(&self) -> &'static str {
        match &self.resolution {
            NativeResolution::Account(None) => "尚未查到原账号操作结果；原结果仍未知，不会重新保存。当前目录另列。",
            NativeResolution::Account(Some(_)) => "已查到原账号保存结果；当前目录另列。创建时的恢复码不能再次读取。",
            NativeResolution::AccountUnavailable => "原账号操作结果仍未知，当前无法核对。可明确结束这次核对，再按当前目录发起新操作；结束核对不会重发或证明原操作失败。",
            NativeResolution::SubjectRejected(IdentityError::InvalidScope) => "本次保存已被拒绝，未修改科目。请先调整课表中的相关课程和保留双表引用，再重新读取核对。",
            NativeResolution::SubjectRejected(_) => "本次科目保存已被明确拒绝，未修改设置。草稿保留，请重新读取后核对。",
            NativeResolution::Subject(None) => "尚未查到原科目保存结果，请重新读取核对；不会重复保存。当前目录另行显示。",
            NativeResolution::Subject(Some(_)) => "已查到原科目保存结果；当前目录另行显示，不会再次保存。",
            NativeResolution::DefaultTeacherRejected(_) => "本次默认老师保存已被明确拒绝，未修改目录。草稿和原修订已保留；请重新加载后核对，不会自动重发。",
            NativeResolution::DefaultTeacher(None) => {
                "原默认老师操作仍未查到持久结果；保留原票据，不会重新保存。当前目录另列。"
            }
            NativeResolution::DefaultTeacher(Some(_)) => {
                "已查到原默认老师操作的持久结果；当前目录另列，不重放原操作。"
            }
            NativeResolution::Expired => {
                "原操作的查询窗口已过期，未重新执行。请核对当前状态后，再明确发起新的操作。"
            }
            NativeResolution::Outcome(fc_ports::command_window::TypedCommandOutcome::Pending) => {
                "原操作结果仍未确认，已保留原请求和票据；当前状态单独显示，不会重新执行。"
            }
            NativeResolution::Outcome(
                fc_ports::command_window::TypedCommandOutcome::Completed(_),
            ) => "已查到原操作的持久结果；以下显示当前状态，设备释放仍需独立确认。",
            NativeResolution::Outcome(fc_ports::command_window::TypedCommandOutcome::Failed(_)) => {
                "原操作已有明确的失败结果；以下显示当前状态，不会重发原请求。"
            }
        }
    }
    fn matches(&self, command: &NativeCommand) -> bool {
        &self.command == command
            && self.command.intent() == command.intent()
            && command.snapshot() == Some(self.issued)
    }
}
impl Outcome {
    fn native_history(&self) -> Option<&NativeHistory> {
        match self {
            Self::VoiceLoaded(_, loaded) => loaded.history.as_ref(),
            Self::Calendar(loaded) => loaded.history.as_ref(),
            Self::DefaultTeacherLoaded { history, .. }
            | Self::AccountHistoryLoaded { history, .. } => Some(history),
            Self::SubjectsLoaded { history, .. } => history.as_deref(),
            Self::SubjectSelfRevoked { history, .. } => Some(history),
            _ => None,
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
enum NativeTarget {
    Identity,
    Accounts(SessionLease),
    Subjects(SessionLease),
    Schedule(SessionLease),
    Display(SessionLease),
    Lesson(SessionLease),
    PublicLesson,
    Materials(SessionLease, fc_domain::materials::MaterialScope),
    MaterialsBatch(SessionLease, fc_domain::materials::MaterialScope),
    ProfileContent(SessionLease, fc_domain::profile_content::ContentSlotKey),
    Homework(SessionLease, ContentKey),
    Calendar(SessionLease, i32),
    PublicProfile(SessionLease),
    SubjectProfile(SessionLease, fc_domain::profile::SubjectProfileKey),
    Voice(SessionLease),
}
impl NativeTarget {
    fn same_reader(actual: SessionLease, original: SessionLease) -> bool {
        actual.principal_id() == original.principal_id() && actual.origin() == original.origin()
    }
    fn confirms(&self, outcome: &Outcome) -> bool {
        match (self, outcome) {
            (Self::Identity, Outcome::Updated { .. })
            | (Self::Accounts(_), Outcome::AccountsLoaded(_))
            | (Self::Subjects(_), Outcome::SubjectsLoaded { .. })
            | (Self::Schedule(_), Outcome::ScheduleLoaded(_))
            | (Self::Display(_), Outcome::DisplayLoaded(..))
            | (Self::Lesson(_), Outcome::LessonLoaded(_))
            | (Self::PublicLesson, Outcome::PublicLessonLoaded(_)) => true,
            (Self::Voice(original), Outcome::VoiceLoaded(actual, _)) => {
                Self::same_reader(*actual, *original)
            }
            (Self::Materials(_, scope), Outcome::Materials(page)) => page.folder.scope == *scope,
            (Self::Calendar(_, year), Outcome::Calendar(loaded)) => {
                loaded.status.selected_year == *year
            }
            (Self::PublicProfile(_), Outcome::Profile(loaded)) => matches!(
                loaded.as_ref(),
                profile_flow::Loaded::Public { saved: false, .. }
            ),
            (Self::SubjectProfile(_, expected), Outcome::Profile(loaded)) => {
                matches!(loaded.as_ref(), profile_flow::Loaded::Subject { key, saved: false, .. } if key == expected)
            }
            (Self::ProfileContent(_, slot), Outcome::ProfileContent(loaded)) => {
                loaded.document.slot == *slot
            }
            (Self::Homework(_, expected), Outcome::Loaded { key, .. }) => expected == key,
            _ => false,
        }
    }
}
enum Work {
    Tracked(Box<Work>, NativeCommand),
    HttpsRuntime(Box<https_runtime_controller::Job>),
    Http(fc_bootstrap::web_transfer::HttpJob),
    HttpProbe(Box<http_business_probe::Job>),
    #[cfg(test)]
    HoldForShutdown(Arc<backup_shutdown_tests::Hold>),
    #[cfg(test)]
    HoldForMaterialBatch(Arc<material_batch_tests::Hold>),
    Backup(Box<backup_controller::Job>),
    Open(DataPaths, Arc<CurrentClock>),
    Request(Request),
    Refresh(Arc<Mutex<Backend>>),
    MaintainVoice(PathBuf, VoiceService),
    MaintainProfileContent(
        PathBuf,
        Arc<Mutex<Backend>>,
        Arc<Mutex<profile_content_flow::GcCursor>>,
    ),
    Shutdown(Arc<Mutex<Backend>>),
    ValidateOffice(PathBuf),
    LoadAccounts(Arc<Mutex<Backend>>, SessionLease),
    LoadSubjects(Arc<Mutex<Backend>>, SessionLease),
    SaveSubject(Arc<Mutex<Backend>>, SessionLease, Revision, IdentityCommand),
    SaveAccount(
        Arc<Mutex<Backend>>,
        SessionLease,
        Box<accounts_editor::Command>,
    ),
    LoadSchedule(Arc<Mutex<Backend>>, SessionLease),
    Calendar(Box<calendar_flow::Job>),
    Autostart(
        Box<fc_bootstrap::autostart::Job>,
        Option<fc_windows::system::RunScope>,
    ),
    LoadPublicLesson(
        Arc<Mutex<Backend>>,
        fc_bootstrap::backend::DesktopLessonGuard,
    ),
    ChangePublicLesson(
        Arc<Mutex<Backend>>,
        fc_bootstrap::backend::DesktopLessonGuard,
        fc_domain::lesson_context::LessonAction,
    ),
    LoadLesson(Arc<Mutex<Backend>>, SessionLease),
    ChangeLesson(
        Arc<Mutex<Backend>>,
        SessionLease,
        Revision,
        fc_domain::schedule::LessonInstanceKey,
        fc_domain::lesson_context::LessonAction,
    ),
    SaveSchedule(
        Arc<Mutex<Backend>>,
        SessionLease,
        Revision,
        ScheduleDocument,
    ),
    LoadDisplay(Arc<Mutex<Backend>>, SessionLease),
    SaveDisplay(
        Arc<Mutex<Backend>>,
        SessionLease,
        fc_domain::subject_visual::PaletteRevision,
        Revision,
        Vec<fc_domain::subject_visual::SubjectColor>,
        DisplaySettings,
    ),
    DisplayPreview(Box<display_flow::PreviewJob>),
    Profile(
        Arc<Mutex<Backend>>,
        SessionLease,
        Box<profiles_editor::Command>,
        u64,
        PathBuf,
    ),
    Desktop(Arc<Mutex<Backend>>, usize, Option<MaterialCursor>),
    ContentImages(Box<profile_content_flow::ImageJob>),
    ContentPage {
        backend: Arc<Mutex<Backend>>,
        generation: SceneGeneration,
        source_revision: Revision,
        request: fc_bootstrap::backend::ProfilePageTarget,
    },
    ProfileContent(Box<profile_content_flow::ContentJob>),
    Material(Box<material_flow::MaterialJob>),
    CleanupMaterial(PathBuf, PathBuf),
    Voice(VoiceService, SessionLease, voice_controller::Operation),
    Https(
        Arc<Mutex<Backend>>,
        SessionLease,
        https_settings_controller::Operation,
    ),
    OpenWeb(Arc<Mutex<Backend>>, SessionLease),
    Web(
        WebService,
        SessionLease,
        web_editor::Operation,
        web_editor::Readiness,
    ),
    CloseWeb(WebService, SessionLease),
}
impl Task for Work {
    type Output = Result<Outcome>;
    fn allocation_bytes(&self) -> Option<usize> {
        match self {
            Self::Tracked(work, _) => work
                .allocation_bytes()?
                .checked_add(std::mem::size_of::<NativeCommand>() + 128),
            Self::HttpsRuntime(job) => job.allocation_bytes(),
            Self::Https(_, _, operation) => operation.allocation_bytes(),
            Self::Http(job) => job.allocation_bytes(),
            Self::HttpProbe(job) => job.allocation_bytes(),
            #[cfg(test)]
            Self::HoldForShutdown(_) | Self::HoldForMaterialBatch(_) => Some(0),
            Self::Backup(job) => job.allocation_bytes(),
            Self::Open(paths, _) => Some(paths.root().as_os_str().len().saturating_mul(2)),
            Self::ValidateOffice(p)
            | Self::MaintainVoice(p, _)
            | Self::MaintainProfileContent(p, _, _) => Some(p.as_os_str().len().saturating_mul(2)),
            Self::Request(r) => r.data.allocation_bytes()?.checked_add(256 * 1024),
            Self::Refresh(_)
            | Self::Shutdown(_)
            | Self::LoadAccounts(_, _)
            | Self::LoadSubjects(_, _)
            | Self::LoadSchedule(_, _)
            | Self::LoadPublicLesson(_, _)
            | Self::ChangePublicLesson(..)
            | Self::LoadLesson(_, _)
            | Self::ChangeLesson(_, _, _, _, _)
            | Self::LoadDisplay(_, _)
            | Self::Voice(..)
            | Self::OpenWeb(_, _)
            | Self::Web(..)
            | Self::CloseWeb(..)
            | Self::Desktop(_, _, _) => Some(1024),
            Self::ContentPage { .. } => Some(1024),
            Self::Calendar(job) => job.allocation_bytes(),
            Self::Autostart(job, _) => job.allocation_bytes()?.checked_add(2048),
            Self::ContentImages(job) => job.allocation_bytes(),
            Self::ProfileContent(job) => job.allocation_bytes(),
            Self::SaveSubject(_, _, _, command) => match command {
                IdentityCommand::PutSubject { name, .. } => name.capacity().checked_add(4096),
                _ => None,
            },
            Self::SaveAccount(_, _, operation) => {
                let (strings, scopes) = match operation.as_ref() {
                    accounts_editor::Command::Create { account, .. } => (
                        account
                            .login_name
                            .capacity()
                            .checked_add(account.display_name.capacity())?,
                        account.scopes.capacity(),
                    ),
                    accounts_editor::Command::Update {
                        command:
                            IdentityCommand::UpdateAccount {
                                display_name,
                                scopes,
                                ..
                            },
                        ..
                    } => (display_name.capacity(), scopes.capacity()),
                    accounts_editor::Command::Update {
                        command: IdentityCommand::SetDefaultTeacher(_),
                        ..
                    } => (0, 0),
                    _ => return None,
                };
                strings
                    .checked_add(scopes.checked_mul(std::mem::size_of::<AssignmentScope>())?)?
                    .checked_mul(3)?
                    .checked_add(4096)
            }
            Self::Profile(_, _, operation, _, root) => profile_flow::allocation(operation)?
                .checked_add(root.as_os_str().len().checked_mul(2)?),
            Self::Material(job) => job.allocation_bytes(),
            Self::CleanupMaterial(root, path) => root
                .as_os_str()
                .len()
                .checked_add(path.as_os_str().len())?
                .checked_mul(2),
            Self::SaveSchedule(_, _, _, document) => document.allocation_bytes(),
            Self::SaveDisplay(_, _, palette, _, colors, settings) => {
                settings
                    .allocation_bytes()?
                    .checked_add(palette.allocation_bytes())?
                    .checked_add(colors.capacity().checked_mul(std::mem::size_of::<
                        fc_domain::subject_visual::SubjectColor,
                    >())?)
            }
            Self::DisplayPreview(job) => job.allocation_bytes(),
        }
    }
    fn run(self, context: &JobContext) -> Self::Output {
        match self {
            Self::Tracked(work, command) => work.run_tracked(context, command),
            work => work.run_tracked(context, NativeCommand::new()),
        }
    }
}
impl Work {
    fn native_mutation(&self) -> bool {
        match self {
            Self::Tracked(work, _) => work.native_mutation(),
            Self::Request(r) => matches!(
                (r.screen, r.action),
                (
                    Screen::Setup
                        | Screen::Recovery
                        | Screen::Password
                        | Screen::Rotate
                        | Screen::School
                        | Screen::HomeworkEdit,
                    SAVE
                ) | (Screen::School, ADD_CLASS | BIND_CLASS)
            ),
            Self::SaveAccount(..)
            | Self::SaveSubject(..)
            | Self::ChangePublicLesson(..)
            | Self::ChangeLesson(..)
            | Self::SaveSchedule(..)
            | Self::SaveDisplay(..)
            | Self::ContentPage { .. } => true,
            Self::Calendar(job) => matches!(
                &job.operation,
                calendar_flow::Operation::Adopt { .. } | calendar_flow::Operation::Policy { .. }
            ),
            Self::Profile(_, _, operation, ..) => matches!(
                operation.as_ref(),
                profiles_editor::Command::SavePublic { .. }
                    | profiles_editor::Command::SaveSubject { .. }
            ),
            Self::Voice(_, _, operation) => !matches!(
                operation,
                voice_controller::Operation::Refresh | voice_controller::Operation::Query(_)
            ),
            Self::Material(job) => matches!(
                job.job,
                material_flow::Job::Import { .. }
                    | material_flow::Job::ImportBatch { .. }
                    | material_flow::Job::Delete { .. }
                    | material_flow::Job::DeleteBatch { .. }
            ),
            Self::ProfileContent(job) => matches!(
                job.operation,
                profile_content_flow::Operation::Publish { .. }
            ),
            _ => false,
        }
    }
    fn native_intent(&self) -> Option<NativeIntent> {
        match self {
            Self::Tracked(work, _) => work.native_intent(),
            Self::SaveSubject(_, _, expected, command) => Some(NativeIntent::Subject {
                expected: *expected,
                command: command.clone(),
            }),
            Self::SaveAccount(_, _, operation) => match operation.as_ref() {
                accounts_editor::Command::Create {
                    expected, account, ..
                } => Some(NativeIntent::Account {
                    expected: *expected,
                    command: Arc::new(IdentityCommand::CreateAccount(account.clone())),
                }),
                accounts_editor::Command::Update {
                    expected,
                    command: account @ IdentityCommand::UpdateAccount { .. },
                } => Some(NativeIntent::Account {
                    expected: *expected,
                    command: Arc::new(account.clone()),
                }),
                accounts_editor::Command::Update {
                    expected,
                    command: IdentityCommand::SetDefaultTeacher(default),
                } => Some(NativeIntent::DefaultTeacher {
                    expected: *expected,
                    default: *default,
                }),
                _ => None,
            },
            Self::Voice(_, _, operation) => {
                operation.policy_intent().map(NativeIntent::VoicePolicy)
            }
            Self::Calendar(job) => match job.operation {
                calendar_flow::Operation::Policy {
                    year,
                    expected,
                    enabled,
                } => Some(NativeIntent::CalendarPolicy {
                    year,
                    expected,
                    enabled,
                }),
                calendar_flow::Operation::Download(year) => {
                    Some(NativeIntent::CalendarDownload { year })
                }
                _ => None,
            },
            _ => None,
        }
    }
    fn native_target(&self) -> Option<NativeTarget> {
        Some(match self {
            Self::Tracked(work, _) => return work.native_target(),
            Self::Request(r) if r.screen == Screen::HomeworkEdit && r.action == SAVE => {
                NativeTarget::Homework(r.lease?, r.key.clone()?)
            }
            Self::Request(_) if self.native_mutation() => NativeTarget::Identity,
            Self::SaveAccount(_, lease, _) => NativeTarget::Accounts(*lease),
            Self::SaveSubject(_, lease, ..) => NativeTarget::Subjects(*lease),
            Self::SaveSchedule(_, lease, ..) => NativeTarget::Schedule(*lease),
            Self::SaveDisplay(_, lease, ..) => NativeTarget::Display(*lease),
            Self::ChangeLesson(_, lease, ..) => NativeTarget::Lesson(*lease),
            Self::ChangePublicLesson(..) => NativeTarget::PublicLesson,
            Self::Calendar(job) => match &job.operation {
                calendar_flow::Operation::Adopt { year, .. }
                | calendar_flow::Operation::Policy { year, .. } => {
                    NativeTarget::Calendar(job.lease?, *year)
                }
                _ => return None,
            },
            Self::Profile(_, lease, operation, ..) => match operation.as_ref() {
                profiles_editor::Command::SavePublic { .. } => NativeTarget::PublicProfile(*lease),
                profiles_editor::Command::SaveSubject { key, .. } => {
                    NativeTarget::SubjectProfile(*lease, *key)
                }
                _ => return None,
            },
            Self::Voice(_, lease, operation)
                if !matches!(
                    operation,
                    voice_controller::Operation::Refresh | voice_controller::Operation::Query(_)
                ) =>
            {
                NativeTarget::Voice(*lease)
            }
            Self::Material(job) => match &job.job {
                material_flow::Job::Import { lease, scope, .. } => {
                    NativeTarget::Materials(*lease, *scope)
                }
                material_flow::Job::ImportBatch { lease, scope, .. }
                | material_flow::Job::DeleteBatch { lease, scope, .. } => {
                    NativeTarget::MaterialsBatch(*lease, *scope)
                }
                material_flow::Job::Delete { lease, request } => {
                    NativeTarget::Materials(*lease, request.scope)
                }
                _ => return None,
            },
            Self::ProfileContent(job) => match &job.operation {
                profile_content_flow::Operation::Publish { slot, .. } => {
                    NativeTarget::ProfileContent(job.lease, *slot)
                }
                _ => return None,
            },
            _ => return None,
        })
    }
    fn reads_native_target(&self, target: &NativeTarget) -> bool {
        match (self, target) {
            (Self::Tracked(work, _), target) => work.reads_native_target(target),
            (Self::Refresh(_), NativeTarget::Identity) => true,
            (Self::LoadAccounts(_, lease), NativeTarget::Accounts(original))
            | (Self::LoadSubjects(_, lease), NativeTarget::Subjects(original))
            | (Self::LoadSchedule(_, lease), NativeTarget::Schedule(original))
            | (Self::LoadDisplay(_, lease), NativeTarget::Display(original))
            | (Self::LoadLesson(_, lease), NativeTarget::Lesson(original)) => {
                NativeTarget::same_reader(*lease, *original)
            }
            (Self::LoadPublicLesson(..), NativeTarget::PublicLesson) => true,
            (Self::Calendar(job), NativeTarget::Calendar(lease, year)) => {
                job.lease
                    .is_some_and(|actual| NativeTarget::same_reader(actual, *lease))
                    && matches!(job.operation, calendar_flow::Operation::Load(actual) if actual == *year)
            }
            (Self::Profile(_, lease, operation, ..), NativeTarget::PublicProfile(original)) => {
                NativeTarget::same_reader(*lease, *original)
                    && matches!(operation.as_ref(), profiles_editor::Command::LoadPublic)
            }
            (
                Self::Profile(_, lease, operation, ..),
                NativeTarget::SubjectProfile(original, key),
            ) => {
                NativeTarget::same_reader(*lease, *original)
                    && matches!(operation.as_ref(), profiles_editor::Command::LoadSubject { key: actual } if actual == key)
            }
            (
                Self::Voice(
                    _,
                    lease,
                    voice_controller::Operation::Refresh | voice_controller::Operation::Query(_),
                ),
                NativeTarget::Voice(original),
            ) => NativeTarget::same_reader(*lease, *original),
            (Self::Material(job), NativeTarget::Materials(lease, scope)) => {
                matches!(&job.job, material_flow::Job::Load { lease: actual, request } if NativeTarget::same_reader(*actual, *lease) && request.scope == *scope)
            }
            (Self::ProfileContent(job), NativeTarget::ProfileContent(lease, slot)) => {
                NativeTarget::same_reader(job.lease, *lease)
                    && matches!(&job.operation, profile_content_flow::Operation::Load { slot: actual } if actual == slot)
            }
            (Self::Request(r), NativeTarget::Homework(lease, _)) => {
                r.screen == Screen::Homework
                    && r.action == LOAD
                    && r.lease
                        .is_some_and(|actual| NativeTarget::same_reader(actual, *lease))
            }
            _ => false,
        }
    }
    fn run_tracked(self, context: &JobContext, command: NativeCommand) -> Result<Outcome> {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        match self {
            Self::Tracked(..) => Err(Failure::Stopped),
            Self::HttpsRuntime(job) => job.run(context),
            Self::Https(backend, lease, operation) => {
                https_settings_controller::run(backend, lease, operation, context)
            }
            Self::Http(job) => Ok(Outcome::Http(job.run(context))),
            Self::HttpProbe(job) => job
                .run(context)
                .map(|done| Outcome::HttpProbe(Box::new(done))),
            #[cfg(test)]
            Self::HoldForShutdown(hold) => {
                hold.wait();
                Ok(Outcome::MaterialCleaned)
            }
            #[cfg(test)]
            Self::HoldForMaterialBatch(hold) => {
                hold.wait();
                Ok(Outcome::MaterialCleaned)
            }
            Self::Backup(job) => job.run_with_command(context, &command),
            Self::Open(paths, clock) => {
                let root = paths.root().to_owned();
                // The first frame is already visible. Recovery owns the same
                // worker deadline/cancellation and precedes full child creation,
                // SQLite, ordinary GC, audio and every network capability.
                fc_bootstrap::reinitialization_files::ReinitializationFiles::new(&paths)
                    .and_then(|files| files.reconcile_startup(&|| context.checkpoint().is_ok()))
                    .map_err(|_| {
                        Failure::Deployment("资料重新初始化状态需要修复；现有文件已保留。")
                    })?;
                fc_bootstrap::recovery_files::RecoveryFiles::new(&paths)
                    .and_then(|files| files.reconcile_startup(&|| context.checkpoint().is_ok()))
                    .map_err(|_| Failure::Deployment("备份恢复状态需要修复；原数据已保留。"))?;
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                let policy = if paths.requires_existing_deployment() {
                    // Original reset/restore journals get the first opportunity
                    // to restore a torn switch. Only then inspect the live set,
                    // before preparing any missing teaching directories.
                    verify_existing_deployment(&root)?;
                    OpenPolicy::ExistingOnly
                } else {
                    OpenPolicy::AllowInitialization
                };
                paths
                    .prepare_for_current_user()
                    .and_then(|deployment| deployment.read_back())
                    .map_err(|_| Failure::Deployment("数据目录权限读回失败，需要修复。"))?;
                let mut backend = open_backend(&root, clock, policy)?;
                if context.checkpoint().is_err() {
                    backend.close()?;
                    return Err(Failure::Stopped);
                }
                let backend = Arc::new(Mutex::new(backend));
                let opened = (|| {
                    let state = backend.lock().map_err(|_| Failure::Stopped)?.state()?;
                    // Startup holds the deployment's exclusive instance guard;
                    // this backend has not been released to any preparation yet.
                    // Reconcile even a new deployment, refusing orphan media in
                    // an uninstalled/older database instead of adopting it.
                    let content_files =
                        fc_bootstrap::profile_content_files::ControlledContentFiles::new(&root)
                            .map_err(BackendError::ProfileContent)?;
                    fc_bootstrap::profile_content_startup::reconcile_after_restart(
                        &*backend.lock().map_err(|_| Failure::Stopped)?,
                        &content_files,
                    )?;
                    // Inventory maintenance requires an installed identity. A
                    // new, valid deployment has no published resources yet;
                    // keep the first-administrator flow available.
                    if state.installation.is_some() {
                        // Read-only recovery: never replay a saved intent or repair
                        // a Run item during startup. Invalid current paths leave
                        // the normal application available for relocation.
                        if let Ok(scope) = fc_windows::system::RunScope::current_executable() {
                            let mut system = scope
                                .open()
                                .map_err(|_| invalid("启动项状态读取不可用。"))?;
                            fc_bootstrap::autostart::Job {
                                backend: backend.clone(),
                                lease: None,
                                operation: fc_bootstrap::autostart::Operation::ObserveStartup,
                            }
                            .run(context, &mut system)?;
                        }
                        material_flow::reconcile(
                            &fc_windows::materials::files::ControlledFiles::new(&root)
                                .map_err(|_| invalid("资料目录不可用。"))?,
                            &backend,
                            context,
                        )?;
                    }
                    let voice_files = fc_bootstrap::voice_files::ControlledVoiceFiles::new(&root)
                        .map_err(|_| invalid("声音目录不可用。"))?;
                    voice_files
                        .reconcile(&mut *backend.lock().map_err(|_| Failure::Stopped)?)
                        .map_err(|_| invalid("声音恢复清理未完成，请核对部署数据。"))?;
                    let control = backend
                        .lock()
                        .map_err(|_| Failure::Stopped)?
                        .session_control();
                    let voice = backend
                        .lock()
                        .map_err(|_| Failure::Stopped)?
                        .voice_service();
                    Ok((state, control, voice))
                })();
                match opened {
                    Ok((state, control, voice)) => {
                        Ok(Outcome::Opened(backend, control, state, voice))
                    }
                    Err(error) => {
                        // The UI has not received this writer yet. Every failed
                        // startup path closes/joins it on this worker before the
                        // deployment's instance guard can be released.
                        let mut owned = backend.lock().unwrap_or_else(|p| p.into_inner());
                        let closed = owned.close();
                        match closed {
                            Ok(()) => Err(error),
                            Err(close) => Err(Failure::Backend(close)),
                        }
                    }
                }
            }
            Self::MaintainVoice(root, service) => {
                fc_bootstrap::voice_files::ControlledVoiceFiles::new(&root)
                    .and_then(|files| files.reconcile_service(&service))
                    .map_err(|_| invalid("声音数据维护未完成。"))?;
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                Ok(Outcome::MaterialCleaned)
            }
            Self::MaintainProfileContent(root, backend, cursor) => {
                profile_content_flow::maintain(root, backend, cursor, context)
            }
            Self::Request(r) => settings_registry::execute(r, &command, context),
            Self::Voice(service, lease, operation) => {
                voice_controller::run(service, lease, operation, context, &command)
            }
            Self::OpenWeb(backend, lease) => {
                let service = backend.lock().map_err(|_| Failure::Stopped)?.web_service();
                let loaded = web_editor::load(&service, lease)?;
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                Ok(Outcome::WebOpened(service, loaded))
            }
            Self::Web(service, lease, operation, readiness) => {
                web_editor::run(&service, lease, operation, readiness, context)
                    .map(Outcome::WebLoaded)
            }
            Self::CloseWeb(service, lease) => {
                // A locked/expired approver already makes its window unusable.
                // Closing is independent cleanup on the shared worker, not UI IO.
                match service.close_pairing(lease) {
                    Ok(()) | Err(WebServiceError::Denied | WebServiceError::Disabled) => {}
                    Err(error) => return Err(Failure::Web(error)),
                }
                Ok(Outcome::MaterialCleaned)
            }
            Self::Refresh(backend) => {
                let state = backend.lock().map_err(|_| Failure::Stopped)?.state()?;
                let next = if state.installation.is_some() {
                    Screen::Login
                } else {
                    Screen::Setup
                };
                Ok(Outcome::Updated {
                    state,
                    lease: None,
                    code: None,
                    next,
                })
            }
            Self::Shutdown(backend) => {
                backend.lock().map_err(|_| Failure::Stopped)?.close()?;
                Ok(Outcome::Closed)
            }
            Self::LoadAccounts(backend, lease) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                if let Some(NativeIntent::DefaultTeacher { expected, default }) = command.intent() {
                    let issued = command.original()?;
                    let result = command.query(&mut backend, context, |backend| {
                        Ok(backend.default_teacher_outcome(lease, *expected, *default)?)
                    });
                    let history = match result {
                        Ok(value) => {
                            NativeHistory::default_teacher(command.clone(), issued, Ok(value))?
                        }
                        Err(Failure::Backend(error)) => {
                            NativeHistory::default_teacher(command.clone(), issued, Err(error))?
                        }
                        Err(error) => return Err(error),
                    };
                    let current = backend.management_identity(lease)?;
                    Ok(Outcome::DefaultTeacherLoaded {
                        current,
                        history: Box::new(history),
                        saved: false,
                    })
                } else if command.is_account() {
                    let issued = command.original()?;
                    let history = if let Some(proof) = command.account_proof()? {
                        let result = command.query(&mut backend, context, |backend| {
                            Ok(backend.account_outcome(lease, proof)?)
                        });
                        match result {
                            Ok(result) => {
                                NativeHistory::account(command.clone(), issued, Ok(result))?
                            }
                            Err(Failure::Backend(error)) => {
                                NativeHistory::account(command.clone(), issued, Err(error))?
                            }
                            Err(error) => return Err(error),
                        }
                    } else {
                        // The original write may not have reached preparation.
                        // Absence of this local metadata proves no COMMIT fact.
                        NativeHistory {
                            command: command.clone(),
                            issued,
                            resolution: NativeResolution::AccountUnavailable,
                        }
                    };
                    let current = backend.management_identity(lease)?;
                    Ok(Outcome::AccountHistoryLoaded {
                        lease,
                        current,
                        history: Box::new(history),
                    })
                } else {
                    Ok(Outcome::AccountsLoaded(backend.management_identity(lease)?))
                }
            }
            Self::LoadSubjects(backend, lease) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                let history = if let Some(NativeIntent::Subject {
                    expected,
                    command: intent,
                }) = command.intent()
                {
                    let issued = command.original()?;
                    let result = command.query(&mut backend, context, |b| {
                        Ok(b.subject_outcome(lease, *expected, intent.clone())?)
                    });
                    Some(Box::new(match result {
                        Ok(value) => NativeHistory::subject(command.clone(), issued, Ok(value))?,
                        Err(Failure::Backend(error)) => {
                            NativeHistory::subject(command.clone(), issued, Err(error))?
                        }
                        Err(error) => return Err(error),
                    }))
                } else {
                    None
                };
                let facts = settings_registry::load_subjects(&backend, lease, context)?;
                Ok(Outcome::SubjectsLoaded {
                    facts,
                    history,
                    saved: false,
                })
            }
            Self::SaveSubject(backend, lease, expected, intent) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                command.run(
                    &mut backend,
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(lease),
                    SessionPurpose::Manage,
                    context,
                    |b| {
                        let original_epoch = b
                            .state()?
                            .account(lease.principal_id())
                            .map_err(|_| Failure::Stopped)?
                            .principal
                            .auth_epoch;
                        let (history, saved) = match b.identity_at(lease, expected, intent.clone())
                        {
                            Ok(state) => {
                                let IdentityCommand::PutSubject { id, .. } = &intent else {
                                    return Err(Failure::Stopped);
                                };
                                let subject = state
                                    .subjects
                                    .iter()
                                    .find(|s| s.id == *id)
                                    .ok_or(Failure::Stopped)?
                                    .clone();
                                let history = NativeHistory::subject(
                                    command.clone(),
                                    command.original()?,
                                    Ok(Some((state.revision, subject))),
                                )?;
                                if state
                                    .account(lease.principal_id())
                                    .map_err(|_| Failure::Stopped)?
                                    .principal
                                    .auth_epoch
                                    != original_epoch
                                {
                                    // The committed operation revoked this caller's own teaching
                                    // session. Report only this original result; do not perform a
                                    // management read with the retired lease or issue a new one.
                                    return Ok(Outcome::SubjectSelfRevoked {
                                        state,
                                        history: Box::new(history),
                                    });
                                }
                                (history, true)
                            }
                            Err(BackendError::Identity(
                                fc_ports::IdentityRepositoryError::Domain(
                                    error @ (IdentityError::RevisionConflict
                                    | IdentityError::InvalidScope
                                    | IdentityError::Capacity
                                    | IdentityError::InvalidText
                                    | IdentityError::DuplicateName
                                    | IdentityError::DuplicateId
                                    | IdentityError::NotFound
                                    | IdentityError::VersionOverflow),
                                ),
                            )) => (
                                NativeHistory::subject_rejected(
                                    command.clone(),
                                    command.original()?,
                                    error,
                                )?,
                                false,
                            ),
                            Err(error) => return Err(error.into()),
                        };
                        Ok(Outcome::SubjectsLoaded {
                            facts: settings_registry::load_subjects(b, lease, context)?,
                            history: Some(Box::new(history)),
                            saved,
                        })
                    },
                )
            }
            Self::SaveAccount(backend, lease, operation) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                command.run(
                    &mut backend,
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(lease),
                    SessionPurpose::Manage,
                    context,
                    |backend| match *operation {
                        accounts_editor::Command::Create {
                            expected,
                            account,
                            password,
                        } => {
                            let code = backend.create_account_at_tracked(
                                lease,
                                expected,
                                account,
                                password,
                                |proof| {
                                    command
                                        .bind_account_proof(proof)
                                        .map_err(|_| BackendError::InvalidInput)
                                },
                            )?;
                            Ok(Outcome::Updated {
                                state: backend.state()?,
                                lease: Some(lease),
                                code: Some(code),
                                next: Screen::Code,
                            })
                        }
                        accounts_editor::Command::Update {
                            expected,
                            command:
                                IdentityCommand::UpdateAccount {
                                    id,
                                    display_name,
                                    role,
                                    enabled,
                                    scopes,
                                },
                        } => {
                            let self_changed = id == lease.principal_id();
                            let state = backend.update_account_at_tracked(
                                lease,
                                expected,
                                IdentityCommand::UpdateAccount {
                                    id,
                                    display_name,
                                    role,
                                    enabled,
                                    scopes,
                                },
                                |proof| {
                                    command
                                        .bind_account_proof(proof)
                                        .map_err(|_| BackendError::InvalidInput)
                                },
                            )?;
                            Ok(Outcome::AccountSaved {
                                state,
                                self_changed,
                            })
                        }
                        accounts_editor::Command::Update {
                            expected,
                            command: IdentityCommand::SetDefaultTeacher(default),
                        } => {
                            let state = match backend.identity_at(
                                lease,
                                expected,
                                IdentityCommand::SetDefaultTeacher(default),
                            ) {
                                Ok(state) => state,
                                Err(BackendError::Identity(
                                    fc_ports::IdentityRepositoryError::Domain(
                                        error @ (IdentityError::RevisionConflict
                                        | IdentityError::InvalidScope
                                        | IdentityError::Capacity),
                                    ),
                                )) => {
                                    // These exact errors are returned before COMMIT. Storage
                                    // publication/unknown/closed errors never enter this branch.
                                    let history = NativeHistory::default_teacher_rejection(
                                        command.clone(),
                                        command.original()?,
                                        error,
                                    )?;
                                    return Ok(Outcome::DefaultTeacherLoaded {
                                        current: backend.management_identity(lease)?,
                                        history: Box::new(history),
                                        saved: false,
                                    });
                                }
                                Err(error) => return Err(error.into()),
                            };
                            let history = NativeHistory::default_teacher(
                                command.clone(),
                                command.original()?,
                                Ok(Some((state.revision, default))),
                            )?;
                            Ok(Outcome::DefaultTeacherLoaded {
                                current: state,
                                history: Box::new(history),
                                saved: true,
                            })
                        }
                        _ => Err(invalid("账号操作在当前页面不可用。")),
                    },
                )
            }
            Self::Calendar(job) => job.run_with_command(context, &command),
            Self::Autostart(job, scope) => {
                let owner = job.lease.map(|lease| lease.principal_id());
                let scope = match scope { Some(scope) => scope, None => fc_windows::system::RunScope::current_executable().map_err(|_| invalid("当前程序路径无法用于登录自启动，请将程序移至较短的本地目录后重新核对。"))? };
                let mut system = scope
                    .open()
                    .map_err(|_| invalid("启动项适配器不可用，请重新读取。"))?;
                let issued = if matches!(
                    &job.operation,
                    fc_bootstrap::autostart::Operation::Change { .. }
                ) {
                    let backend = job.backend.lock().map_err(|_| Failure::Stopped)?;
                    Some(command.issue(
                        &backend,
                        fc_ports::command_window::CommandFamily::Autostart,
                        job.lease,
                        SessionPurpose::Manage,
                        context,
                    )?)
                } else {
                    None
                };
                job.run_with_command(context, &mut system, issued)
                    .map(|loaded| Outcome::Autostart(owner, Box::new(loaded)))
                    .map_err(Failure::from)
            }
            Self::LoadSchedule(backend, lease) => {
                let document = backend
                    .lock()
                    .map_err(|_| Failure::Stopped)?
                    .read_schedule(lease)?;
                Ok(Outcome::ScheduleLoaded(document))
            }
            Self::LoadPublicLesson(backend, guard) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                let scoped = backend.begin_execution_scope(Arc::new(context.execution_fence()));
                Ok(Outcome::PublicLessonLoaded(
                    scoped.read_desktop_lesson(&guard)?,
                ))
            }
            Self::ChangePublicLesson(backend, guard, action) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                command.run(
                    &mut backend,
                    fc_ports::command_window::CommandFamily::DesktopLesson,
                    None,
                    SessionPurpose::Manage,
                    context,
                    |scoped| {
                        Ok(Outcome::PublicLessonSaved(
                            scoped.change_desktop_lesson(&guard, action)?,
                        ))
                    },
                )
            }
            Self::LoadLesson(backend, lease) => {
                let snapshot = backend
                    .lock()
                    .map_err(|_| Failure::Stopped)?
                    .read_lesson(lease)?;
                Ok(Outcome::LessonLoaded(snapshot))
            }
            Self::ChangeLesson(backend, lease, expected, lesson, action) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                command.run(
                    &mut backend,
                    fc_ports::command_window::CommandFamily::Lesson,
                    Some(lease),
                    SessionPurpose::Manage,
                    context,
                    |backend| {
                        Ok(Outcome::LessonSaved(
                            backend.change_lesson(lease, expected, lesson, action)?,
                        ))
                    },
                )
            }
            Self::SaveSchedule(backend, lease, expected, document) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                command.run(
                    &mut backend,
                    fc_ports::command_window::CommandFamily::Schedule,
                    Some(lease),
                    SessionPurpose::Manage,
                    context,
                    |backend| {
                        Ok(Outcome::ScheduleSaved(
                            backend.save_schedule(lease, expected, document)?,
                        ))
                    },
                )
            }
            Self::LoadDisplay(backend, lease) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                let scoped = backend.begin_execution_scope(Arc::new(context.execution_fence()));
                let settings = scoped.read_subject_visuals(lease)?;
                let identity = Arc::new(scoped.state()?);
                Ok(Outcome::DisplayLoaded(settings, identity))
            }
            Self::SaveDisplay(backend, lease, palette, expected, colors, settings) => {
                let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
                command.run(
                    &mut backend,
                    fc_ports::command_window::CommandFamily::SubjectVisual,
                    Some(lease),
                    SessionPurpose::Manage,
                    context,
                    |scoped| {
                        let settings = scoped
                            .save_subject_visuals(lease, palette, expected, colors, settings)?;
                        let identity = Arc::new(scoped.state()?);
                        Ok(Outcome::DisplaySaved(settings, identity))
                    },
                )
            }
            Self::DisplayPreview(job) => job.run(context),
            Self::Profile(backend, lease, operation, sequence, root) => profile_flow::run(
                backend, lease, *operation, sequence, root, context, &command,
            )
            .map(|loaded| Outcome::Profile(Box::new(loaded))),
            Self::Material(job) => job.run(context, &command),
            Self::CleanupMaterial(root, path) => {
                let files = fc_windows::materials::files::ControlledFiles::new(&root)
                    .map_err(|_| invalid("副本清理目录不可用。"))?;
                if path.parent().and_then(Path::parent)
                    != Some(root.join("launch-copies").as_path())
                {
                    return Err(invalid("副本清理范围无效。"));
                }
                files.remove(&path).map_err(|_| invalid("副本仍被占用。"))?;
                if let Some(parent) = path.parent() {
                    let _ = fs::remove_dir(parent);
                }
                Ok(Outcome::MaterialCleaned)
            }
            Self::Desktop(backend, offset, cursor) => {
                // A display refresh must never occupy the second shared worker
                // waiting behind a management KDF/transaction holding Backend.
                let backend = backend.try_lock().map_err(|_| {
                    Failure::Backend(BackendError::Display(
                        fc_ports::DisplayRepositoryError::Unavailable,
                    ))
                })?;
                let facts = backend.desktop_facts_with_material_cursor(offset, cursor)?;
                let content = backend.hydrate_profile_content(&facts)?;
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                Ok(Outcome::Desktop(Box::new(facts), content))
            }
            Self::ContentImages(job) => job.run(context),
            Self::ProfileContent(job) => job.run(context, &command),
            Self::ContentPage {
                backend,
                generation,
                source_revision,
                request,
            } => {
                context.checkpoint().map_err(|_| Failure::Stopped)?;
                let mut backend = backend
                    .try_lock()
                    .map_err(|_| invalid("内容状态正在更新，请稍后刷新。"))?;
                if command.read_worker()?.is_none() {
                    let issued = backend.issue_profile_page_command(
                        generation,
                        source_revision,
                        &request,
                        Arc::new(context.execution_fence()),
                    )?;
                    command.bind(issued)?;
                }
                let snapshot = command.query(&mut backend, context, |backend| {
                    let request = fc_domain::profile_content::PageContentRequest {
                        command_id: backend.issued_command_id()?,
                        slot: request.slot,
                        expected_content_revision: request.expected_content_revision,
                        expected_view_revision: request.expected_view_revision,
                        page: request.page,
                    };
                    Ok(backend.page_profile_content_while(
                        generation,
                        source_revision,
                        request,
                        Arc::new(context.execution_fence()),
                    )?)
                })?;
                Ok(Outcome::ContentPage {
                    generation,
                    snapshot,
                })
            }
            Self::ValidateOffice(path) => {
                struct Control<'a>(&'a JobContext);
                impl fc_materials::office::ValidationControl for Control<'_> {
                    fn canceled(&self) -> bool {
                        self.0.checkpoint().is_err()
                    }
                    fn thread_cpu_time(
                        &self,
                    ) -> std::result::Result<Duration, fc_materials::office::ContainerError>
                    {
                        fc_windows::system::current_thread_cpu_time()
                            .map_err(|_| fc_materials::office::ContainerError::ClockUnavailable)
                    }
                }
                let mut file = File::open(path).map_err(|_| invalid("文件无法读取。"))?;
                let report = fc_materials::office::validate_office(
                    &mut file,
                    fc_materials::office::OfficeKind::Docx,
                    &Control(context),
                )
                .map_err(|_| invalid("Office容器校验未通过。"))?;
                Ok(Outcome::Office(report))
            }
        }
    }
}
fn find_class(state: &IdentityState, name: &str) -> Result<ClassId> {
    let key = normalized_name(name, 128).map_err(|_| invalid("请选择已有班级。"))?;
    state
        .classes
        .iter()
        .find(|c| c.enabled && normalized_name(&c.name, 128).as_ref() == Ok(&key))
        .map(|c| c.id)
        .ok_or_else(|| invalid("请选择已有且启用的班级。"))
}
fn find_subject(state: &IdentityState, name: &str) -> Result<SubjectId> {
    let key = normalized_name(name, 64).map_err(|_| invalid("请选择已有科目。"))?;
    state
        .subjects
        .iter()
        .find(|s| s.enabled && normalized_name(&s.name, 64).as_ref() == Ok(&key))
        .map(|s| s.id)
        .ok_or_else(|| invalid("请选择已有且启用的科目。"))
}
fn parse_color(value: &str) -> Result<Rgb> {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 || !value.is_ascii() {
        return Err(invalid("科目颜色请输入六位十六进制色值，例如70A5D8。"));
    }
    let number = u32::from_str_radix(value, 16).map_err(|_| invalid("科目颜色格式不正确。"))?;
    Ok(Rgb {
        red: (number >> 16) as u8,
        green: (number >> 8) as u8,
        blue: number as u8,
    })
}

struct Pending {
    ticket: JobTicket<Result<Outcome>>,
    generation: u64,
    self_revokes: bool,
    public_lesson: bool,
}
struct App {
    window: Window,
    pool: shared_pool::SharedPool<Work>,
    clock: Arc<CurrentClock>,
    backend: Option<Arc<Mutex<Backend>>>,
    control: Option<SessionControl>,
    voice_service: Option<VoiceService>,
    audio: Option<VoiceRuntime>,
    audio_slot: Arc<OnceLock<VoiceRuntime>>,
    audio_suspended: Arc<AtomicBool>,
    next_audio_availability: Instant,
    state: Arc<IdentityState>,
    lease: Option<SessionLease>,
    screen: Screen,
    status: String,
    desktop_notice: String,
    pending: Option<Pending>,
    generation: u64,
    key: Option<ContentKey>,
    record: Option<ContentRecord>,
    last_input: Instant,
    stopping: bool,
    shutdown: Option<JobTicket<Result<Outcome>>>,
    writer_closed: bool,
    shutdown_deadline: Option<Instant>,
    terminate_required: bool,
    reinitialize_requested: bool,
    schedule: Option<ScheduleEditor>,
    calendar: Option<calendar_editor::Editor>,
    autostart: Option<autostart_editor::Editor>,
    autostart_job: Option<Pending>,
    autostart_command: Option<NativeCommand>,
    autostart_unknown: bool,
    calendar_command: Option<NativeCommand>,
    calendar_unknown: bool,
    calendar_command_target: Option<(PrincipalId, i32)>,
    autostart_scope: Option<fc_windows::system::RunScope>,
    backup: backup_controller::State,
    calendar_job: Option<Pending>,
    next_calendar_maintenance: Instant,
    display: Option<display_editor::Editor>,
    accounts: Option<accounts_editor::Editor>,
    subject_editor: Option<settings_registry::SubjectEditor>,
    profiles: Option<profiles_editor::Editor>,
    profile_content: Option<profile_content_editor::Editor>,
    profile_content_owner: Option<PrincipalId>,
    preview_sequence: u64,
    preview_active: bool,
    desktop: desktop_controller::Controller,
    root: PathBuf,
    material_editor: Option<materials_editor::Editor>,
    copies_loaded_sequence: u64,
    lesson_editor: Option<lesson_editor::Editor>,
    public_lesson_editor: Option<public_lesson_editor::Editor>,
    public_lesson_flow: Option<fc_bootstrap::backend::DesktopLessonFlow>,
    public_lesson_command: Option<NativeCommand>,
    public_lesson_unknown: bool,
    web_service: Option<WebService>,
    web_editor: Option<web_editor::Editor>,
    https_editor: Option<https_settings_editor::Editor>,
    https_pending: Option<https_settings_controller::Pending>,
    https_unadmitted: Option<(Work, Instant)>,
    native_command: Option<NativeCommand>,
    native_unknown: bool,
    // A single bounded public summary survives explicit end-query. It never
    // claims a matched result and does not retain credentials or old authority.
    account_history_note: Option<accounts_editor::HistoryNotice>,
    native_target: Option<NativeTarget>,
    native_mutation_pending: bool,
    native_readback_pending: bool,
    #[cfg(test)]
    lose_completed_history_for_test: Option<NativeCommand>,
    voice_editor: Option<voice_editor::Editor>,
    web_readiness: web_editor::Readiness,
    teacher_https: https_runtime_controller::State,
    next_web_refresh: Instant,
    close_web: Option<(WebService, SessionLease)>,
    web_close_job: Option<JobTicket<Result<Outcome>>>,
    lesson_entry_pending: bool,
    lesson_flow_active: bool,
    offscreen_probe: bool,
    probe_trace: bool,
    shell: fc_windows::materials::shell::ShellLauncher,
    control_slot: Arc<OnceLock<SessionControl>>,
    launch_paths: HashMap<u64, PathBuf>,
    cleanup_paths: VecDeque<PathBuf>,
    cleanup_jobs: Vec<JobTicket<Result<Outcome>>>,
    voice_maintenance: Option<JobTicket<Result<Outcome>>>,
    next_voice_maintenance: Instant,
    content_maintenance: Option<JobTicket<Result<Outcome>>>,
    next_content_maintenance: Instant,
    content_gc: Arc<Mutex<profile_content_flow::GcCursor>>,
}
impl App {
    fn account_notice_owner(&self) -> Option<fc_ports::command_window::CommandOwner> {
        let lease = self.lease?;
        if lease.origin() != TrustedOrigin::Local
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
        {
            return None;
        }
        let a = self.state.account(lease.principal_id()).ok()?;
        if !a.principal.enabled || a.principal.role != Role::GlobalAdmin || a.must_change_password {
            return None;
        }
        Some(fc_ports::command_window::CommandOwner::Principal {
            principal: a.principal.id,
            school: a.principal.school_id,
            epoch: a.principal.auth_epoch,
            origin: lease.origin(),
        })
    }
    fn shutdown_complete(&self) -> bool {
        self.stopping && module_registry::shutdown_complete(self) && !self.terminate_required
    }
    fn view(&self) -> View {
        let status = match (self.status.is_empty(), self.desktop_notice.is_empty()) {
            (true, _) => self.desktop_notice.clone(),
            (_, true) => self.status.clone(),
            _ => format!("{}\n{}", self.status, self.desktop_notice),
        };
        let status = if self.screen == Screen::Web {
            self.teacher_https
                .failure_message()
                .map_or(status.clone(), |error| format!("{error}。\n{status}"))
        } else {
            status
        };
        let mut view = settings_registry::view(self, &status);
        if self.screen == Screen::Accounts {
            let unresolved = self.native_unknown
                && self
                    .native_command
                    .as_ref()
                    .is_some_and(NativeCommand::is_account);
            if unresolved {
                if let Some(issued) = self
                    .native_command
                    .as_ref()
                    .and_then(NativeCommand::snapshot)
                    .filter(|issued| self.account_notice_owner() == Some(issued.owner))
                {
                    view.status
                        .push_str(&format!("\n待核对账号操作编号：{}。", issued.ticket));
                }
            }
            accounts_editor::decorate_history(
                &mut view,
                self.account_history_note
                    .as_ref()
                    .and_then(|notice| notice.text_for(self.lease?, self.account_notice_owner()?)),
                unresolved,
                self.pending.is_none() && !self.stopping,
            );
        }
        view
    }
    fn render(&self) -> std::result::Result<(), String> {
        window_registry::render(&self.window, self.screen, &self.view())
            .map_err(|_| "工作台渲染失败".to_owned())?;
        if self.screen == Screen::Web {
            if let Some(code) = self
                .web_editor
                .as_ref()
                .and_then(web_editor::Editor::code_text)
            {
                self.window
                    .present_secret(web_editor::CODE, code.as_str())
                    .map_err(|_| "一次性配对码显示失败".to_owned())?;
            }
        }
        Ok(())
    }
    fn lock(&mut self) {
        // Account locking cannot manufacture an authentication requirement for
        // the independent public desktop chooser. OS/foreground events close
        // that capability explicitly before calling this management lock.
        if self.public_lesson_flow.is_some() && !self.stopping {
            if let Some(control) = &self.control {
                control.lock_local();
            }
            self.lease = None;
            return;
        }
        if self.backup.irreversible()
            || self
                .control
                .as_ref()
                .is_some_and(SessionControl::is_recovering)
        {
            if let Some(control) = &self.control {
                control.lock_local();
            }
            return;
        }
        if self.backend.is_none() && self.screen == Screen::Starting {
            return;
        }
        if self.web_readiness == web_editor::Readiness::Starting {
            self.stop_teacher_https();
        }
        self.suspend_https();
        self.leave_web();
        self.lock_backup();
        self.clear_profile_preview();
        if let Some(c) = &self.control {
            c.lock_local();
        }
        if let Some(editor) = &mut self.material_editor {
            editor.suspend();
        }
        settings_registry::suspend(self);
        // A locked review is discarded; a later session must read current
        // candidates and revisions again. The physical download ticket remains.
        self.calendar = None;
        self.autostart = None;
        self.voice_editor = None;
        if let Some(p) = &self.autostart_job {
            p.ticket.cancel();
        }
        if let Some(p) = &self.calendar_job {
            p.ticket.cancel();
        }
        self.generation = self.generation.saturating_add(1);
        if let Some(p) = &self.pending {
            p.ticket.cancel();
        }
        self.lease = None;
        self.key = None;
        self.record = None;
        self.lesson_editor = None;
        self.lesson_entry_pending = false;
        self.lesson_flow_active = false;
        self.screen = if self.state.installation.is_none() {
            Screen::Setup
        } else {
            Screen::Login
        };
        self.status.clear();
    }
    fn begin_shutdown(&mut self) {
        module_registry::stop(module_registry::Kind::TeacherHttps, self);
        self.close_public_lesson();
        if self
            .control
            .as_ref()
            .is_some_and(SessionControl::is_recovering)
        {
            let _ = self.poll_backup();
        }
        if self.backup.irreversible()
            || self
                .control
                .as_ref()
                .is_some_and(SessionControl::is_recovering)
        {
            self.close_backup();
            self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(5));
            module_registry::stop(module_registry::Kind::MaterialOpen, self);
            return;
        }
        if self.stopping {
            return;
        }
        self.stopping = true;
        self.suspend_https();
        self.https_unadmitted = None;
        self.close_backup();
        if let Some(ticket) = &self.content_maintenance {
            ticket.cancel();
        }
        if let Some(job) = &self.calendar_job {
            job.ticket.cancel();
        }
        module_registry::stop(module_registry::Kind::VoicePlayback, self);
        module_registry::stop(module_registry::Kind::ClassroomDisplay, self);
        module_registry::stop(module_registry::Kind::MaterialOpen, self);
        self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(5));
        self.lock();
        self.window.hide();
        module_registry::stop(module_registry::Kind::AuthorityWriter, self);
    }
    fn schedule_shutdown(&mut self) {
        // Network workers may still need the original Writer for negative
        // cleanup. Do not close it or stop the common worker domain first.
        if !self.teacher_https_quiescent() {
            return;
        }
        if self.writer_closed {
            return;
        }
        if self.backup.irreversible() || !self.backup.retirement_complete() {
            return;
        }
        if self.shutdown.is_some() || self.autostart_job.is_some() {
            return;
        }
        // Audio settles its old driver tokens while the unique writer remains
        // alive. The UI only polls; it never waits for native drain or a thread.
        if self.audio.as_ref().is_some_and(|audio| {
            audio.status().map_or(true, |s| {
                s.phase != RuntimePhase::Closed || s.physical_thread_running
            })
        }) {
            return;
        }
        if let Some(backend) = &self.backend {
            match self.pool.handle().try_submit(
                Work::Shutdown(backend.clone()),
                Instant::now() + Duration::from_secs(5),
            ) {
                Ok(ticket) => self.shutdown = Some(ticket),
                Err(rejected) => match rejected.kind {
                    fc_runtime::worker::Rejection::Busy
                    | fc_runtime::worker::Rejection::Deadline => {}
                    _ => {
                        self.terminate_required = true;
                        module_registry::stop(module_registry::Kind::SharedWorkers, self);
                    }
                },
            }
        } else if self.pending.is_none() {
            module_registry::stop(module_registry::Kind::SharedWorkers, self);
        }
    }
    fn apply_native_history(&mut self, history: &NativeHistory) {
        if self
            .native_command
            .as_ref()
            .is_some_and(|original| history.matches(original))
        {
            self.native_unknown = history.pending();
        }
        if self
            .calendar_command
            .as_ref()
            .is_some_and(|original| history.matches(original))
        {
            self.calendar_unknown = history.pending();
        }
    }
    fn submit(&mut self, work: Work) -> std::result::Result<(), String> {
        if self.pending.is_some() {
            return Ok(());
        }
        let public_lesson = matches!(
            &work,
            Work::LoadPublicLesson(..) | Work::ChangePublicLesson(..)
        );
        let mutation = work.native_mutation();
        let readback = if public_lesson {
            self.public_lesson_command.is_some()
                && work.reads_native_target(&NativeTarget::PublicLesson)
        } else {
            self.native_target
                .as_ref()
                .is_some_and(|target| work.reads_native_target(target))
        };
        let target = work.native_target();
        if mutation
            && if public_lesson {
                self.public_lesson_unknown
            } else {
                self.native_unknown
            }
        {
            self.status = "原操作结果尚未核对，票据与目标已保留。请明确读取当前状态后再决定新的操作，不会重发。".into();
            return self.render();
        }
        if self.offscreen_probe && self.probe_trace {
            let kind = match &work {
                Work::Request(_) => "request",
                Work::Material(_) => "material",
                Work::LoadLesson(_, _) => "lesson-load",
                Work::ChangeLesson(..) => "lesson-change",
                Work::SaveSchedule(..) => "schedule-save",
                Work::LoadSchedule(..) => "schedule-load",
                Work::LoadDisplay(..) => "display-load",
                Work::SaveDisplay(..) => "display-save",
                Work::DisplayPreview(..) => "display-preview",
                _ => "other",
            };
            eprintln!(
                "PROBE_SUBMIT tick={} kind={} page={}",
                self.clock.monotonic_millis(),
                kind,
                self.view().title
            );
        }
        // Successful own password/recovery/exit commands deliberately revoke the
        // current local lease before their completion reaches the window pump.
        // WTS/explicit lock still wins immediately; the heartbeat must allow this
        // particular completion to be consumed rather than cancel its own success.
        let self_revokes = matches!(&work,Work::Request(r) if r.action==EXIT||r.action==SAVE&&matches!(r.screen,Screen::Password|Screen::Rotate))
            || matches!(&work, Work::SaveAccount(_, lease, operation) if matches!(operation.as_ref(), accounts_editor::Command::Update { command: IdentityCommand::UpdateAccount { id, .. }, .. } if *id == lease.principal_id()))
            || matches!(&work, Work::SaveSubject(_, lease, _, IdentityCommand::PutSubject { id, enabled, .. })
                if self.state.subjects.iter().any(|s| s.id == *id && s.enabled != *enabled)
                    && self.state.assignments.iter().any(|a| a.teacher_id == lease.principal_id() && a.subject_id == *id && a.enabled));
        let command = match &work {
            Work::LoadSubjects(..)
                if readback
                    && self.native_unknown
                    && self.native_command.as_ref().is_some_and(|original| {
                        matches!(original.intent(), Some(NativeIntent::Subject { .. }))
                    }) =>
            {
                self.native_command
                    .as_ref()
                    .ok_or("原科目保存记录不可用。")?
                    .clone()
            }
            Work::LoadAccounts(..)
                if readback
                    && self.native_unknown
                    && self.native_command.as_ref().is_some_and(|original| {
                        matches!(
                            original.intent(),
                            Some(
                                NativeIntent::DefaultTeacher { .. } | NativeIntent::Account { .. }
                            )
                        )
                    }) =>
            {
                self.native_command
                    .as_ref()
                    .ok_or("原默认老师票据不可用。")?
                    .clone()
            }
            Work::Material(job) => match &job.job {
                material_flow::Job::ImportBatch { command, .. }
                | material_flow::Job::DeleteBatch { command, .. }
                | material_flow::Job::QueryBatch { command, .. } => command.clone(),
                _ => NativeCommand::with_intent(work.native_intent()),
            },
            _ => NativeCommand::with_intent(work.native_intent()),
        };
        let work = Work::Tracked(Box::new(work), command.clone());
        match self
            .pool
            .handle()
            .try_submit(work, Instant::now() + Duration::from_secs(90))
        {
            Ok(ticket) => {
                if mutation {
                    if public_lesson {
                        self.public_lesson_command = Some(command);
                    } else {
                        self.native_command = Some(command);
                        self.native_target = target;
                    }
                }
                self.native_mutation_pending = mutation;
                self.native_readback_pending = readback;
                self.pending = Some(Pending {
                    ticket,
                    generation: self.generation,
                    self_revokes,
                    public_lesson,
                });
                self.status = "正在处理，请稍候。".into();
            }
            Err(_) => self.status = "后台任务繁忙，请稍后重试。".into(),
        }
        self.render()
    }
    fn event(&mut self, event: Event) -> std::result::Result<(), String> {
        if self
            .control
            .as_ref()
            .is_some_and(SessionControl::is_recovering)
        {
            self.poll_backup()?;
        }
        if self.backup.irreversible() {
            return match event {
                Event::Submit(action, data) => self.backup_event(action, data),
                Event::Close => {
                    self.close_backup();
                    self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(30));
                    self.render()
                }
                _ => Ok(()),
            };
        }
        if self.public_lesson_flow.is_some() {
            return match event {
                Event::Submit(action, data) => self.public_lesson_event(action, data),
                Event::Close => {
                    self.close_public_lesson();
                    self.render()
                }
                Event::Input => {
                    if self.pending.is_none() {
                        if let (Some(editor), Ok(data)) = (
                            &mut self.public_lesson_editor,
                            self.window.draft_nonsecret(),
                        ) {
                            if editor.input(data) {
                                self.render()?;
                            }
                        }
                    }
                    Ok(())
                }
                _ => Ok(()),
            };
        }
        match event {
            Event::Input => {
                self.last_input = Instant::now();
                if let (Some(c), Some(l)) = (&self.control, self.lease) {
                    let _ = c.note_input(l, self.clock.monotonic_millis());
                }
                settings_registry::input(self)?;
            }
            Event::LayoutSelect {
                generation,
                scope,
                id,
            } => {
                self.layout_canvas_event(generation, scope, id, None)?;
            }
            Event::LayoutDrop {
                generation,
                scope,
                source,
                target,
                before,
            } => {
                self.layout_canvas_event(generation, scope, source, Some((target, before)))?;
            }
            Event::GridCell { row, column } => {
                self.last_input = Instant::now();
                if let (Some(c), Some(l)) = (&self.control, self.lease) {
                    let _ = c.note_input(l, self.clock.monotonic_millis());
                }
                settings_registry::grid(self, row, column)?;
            }
            Event::Close => {
                self.lock();
                self.window.hide();
                self.render()?;
            }
            Event::Submit(mut action, data) => {
                if self.offscreen_probe && self.probe_trace {
                    eprintln!(
                        "PROBE_EVENT tick={} action={} page={} pending={} generation={}",
                        self.clock.monotonic_millis(),
                        action.0,
                        self.view().title,
                        self.pending.is_some(),
                        self.generation
                    );
                }
                self.last_input = Instant::now();
                if let (Some(c), Some(l)) = (&self.control, self.lease) {
                    let _ = c.note_input(l, self.clock.monotonic_millis());
                }
                if action == LOCK && !self.stopping {
                    self.lock();
                    return self.render();
                }
                if !self.stopping {
                    if let Some(handler) = settings_registry::submit_handler(self.screen, true)? {
                        return handler(self, action, data);
                    }
                }
                if self.pending.is_some() || self.autostart_job.is_some() || self.stopping {
                    return Ok(());
                }
                if action == TO_REINITIALIZE {
                    if matches!(
                        self.screen,
                        Screen::Login | Screen::Recovery | Screen::Repair
                    ) {
                        // No destructive authority is granted here. Close the
                        // original process normally, including the writer and
                        // actual workers, before a fresh process may acquire
                        // its instance lock and show the two acknowledgements.
                        self.reinitialize_requested = true;
                        self.begin_shutdown();
                    }
                    return self.render();
                }
                if action == TO_PROFILES {
                    if self.screen == Screen::Display {
                        if let Some(editor) = &mut self.display {
                            editor.capture(data);
                        }
                    }
                    return self.open_profiles();
                }
                if action == TO_BACKUP {
                    if self.screen == Screen::Display {
                        if let Some(editor) = &mut self.display {
                            editor.capture(data);
                        }
                    }
                    return self.open_backup();
                }
                if action == TO_VOICE && self.screen == Screen::Display {
                    if let Some(editor) = &mut self.display {
                        editor.capture(data);
                    }
                    self.voice_editor = None;
                    return self.submit_voice_management(voice_controller::Operation::Refresh);
                }
                if action == TO_AUTOSTART {
                    if self.screen == Screen::Display {
                        if let Some(editor) = &mut self.display {
                            editor.capture(data);
                        }
                    }
                    return self.submit_autostart(fc_bootstrap::autostart::Operation::Load);
                }
                if let Some(handler) = settings_registry::submit_handler(self.screen, false)? {
                    return handler(self, action, data);
                }
                if action == EXIT && self.has_unsaved_changes() {
                    self.screen = Screen::ExitDraft;
                    self.status.clear();
                    return self.render();
                }
                if action == DISCARD_EXIT {
                    if self.screen != Screen::ExitDraft {
                        return Ok(());
                    }
                    action = EXIT;
                }
                match action {
                    TO_ACCOUNTS => self.open_accounts()?,
                    TO_WEB => self.open_web_editor()?,
                    TO_LESSON => {
                        self.open_lesson_editor()?;
                    }
                    RETURN_DRAFT if self.screen == Screen::ExitDraft => {
                        let owner = self.current_global_owner();
                        self.screen =
                            if self.schedule.as_ref().is_some_and(|e| {
                                Some(e.owner()) == owner && e.has_unsaved_changes()
                            }) {
                                Screen::Schedule
                            } else if owner.is_some_and(|owner| self.backup.has_review(owner)) {
                                Screen::Backup
                            } else if self.autostart.as_ref().is_some_and(|e| {
                                Some(e.owner()) == owner && e.has_unsaved_changes()
                            }) {
                                Screen::Autostart
                            } else if self.voice_editor.as_ref().is_some_and(|e| {
                                Some(e.owner()) == owner && e.has_unsaved_changes()
                            }) {
                                Screen::Voice
                            } else if self.calendar.as_ref().is_some_and(|e| {
                                Some(e.owner()) == owner && e.has_unsaved_changes()
                            }) {
                                Screen::Calendar
                            } else if self.display.as_ref().is_some_and(|e| {
                                Some(e.owner()) == owner && e.has_unsaved_changes()
                            }) {
                                Screen::Display
                            } else if self.subject_editor.as_ref().is_some_and(|e| {
                                Some(e.owner()) == owner && e.has_unsaved_changes()
                            }) {
                                Screen::Subjects
                            } else if self.accounts.as_ref().is_some_and(|e| {
                                Some(e.owner()) == owner && e.has_unsaved_changes()
                            }) {
                                Screen::Accounts
                            } else if self.profiles.as_ref().is_some_and(|e| {
                                Some(e.owner()) == owner && e.has_unsaved_changes()
                            }) {
                                Screen::Profiles
                            } else if self.profile_content_owner == owner
                                && self
                                    .profile_content
                                    .as_ref()
                                    .is_some_and(|editor| editor.is_dirty())
                            {
                                Screen::ProfileContent
                            } else {
                                Screen::Home
                            };
                        self.status.clear();
                        self.render()?;
                    }
                    TO_MATERIALS => {
                        if let Some(lease) = self.lease {
                            if self
                                .material_editor
                                .as_ref()
                                .is_none_or(|editor| !editor.owned_by(lease.principal_id()))
                            {
                                self.material_editor =
                                    Some(materials_editor::Editor::new(lease.principal_id()));
                            }
                            self.screen = Screen::Materials;
                            self.status.clear();
                            self.render()?;
                        }
                    }
                    TO_DISPLAY => {
                        if let Some(lease) = self.lease {
                            if self
                                .display
                                .as_ref()
                                .is_some_and(|e| e.owner() == lease.principal_id())
                            {
                                self.screen = Screen::Display;
                                self.status.clear();
                                self.render()?;
                            } else if let Some(backend) = &self.backend {
                                self.submit(Work::LoadDisplay(backend.clone(), lease))?;
                            }
                        }
                    }
                    TO_SCHEDULE => {
                        if let Some(lease) = self.lease {
                            if self
                                .schedule
                                .as_ref()
                                .is_some_and(|e| e.owner() == lease.principal_id())
                            {
                                self.screen = Screen::Schedule;
                                self.status.clear();
                                self.render()?;
                            } else if let Some(backend) = &self.backend {
                                self.submit(Work::LoadSchedule(backend.clone(), lease))?;
                            }
                        }
                    }
                    LOCK => {
                        self.lock();
                        self.render()?;
                    }
                    ACK => {
                        self.screen = if self.lease.is_some() {
                            Screen::Home
                        } else {
                            Screen::Login
                        };
                        self.status.clear();
                        self.render()?;
                    }
                    BACK => {
                        self.screen = match self.screen {
                            Screen::Recovery => Screen::Login,
                            Screen::HomeworkEdit => Screen::Homework,
                            _ => Screen::Home,
                        };
                        self.status.clear();
                        self.render()?;
                    }
                    TO_SUBJECTS => self.open_subjects()?,
                    TO_RECOVERY | TO_SCHOOL | TO_HOMEWORK | TO_PASSWORD | TO_ROTATE => {
                        self.screen = match action {
                            TO_RECOVERY => Screen::Recovery,
                            TO_SCHOOL => Screen::School,
                            TO_HOMEWORK => Screen::Homework,
                            TO_PASSWORD => Screen::Password,
                            _ => Screen::Rotate,
                        };
                        self.status.clear();
                        self.render()?;
                    }
                    _ => {
                        if let Some(backend) = &self.backend {
                            let request = Request {
                                backend: backend.clone(),
                                screen: self.screen,
                                action,
                                data,
                                lease: self.lease,
                                state: self.state.clone(),
                                key: self.key.clone(),
                                revision: self
                                    .record
                                    .as_ref()
                                    .map(|r| r.revision)
                                    .unwrap_or_default(),
                                clock: self.clock.clone(),
                            };
                            self.submit(Work::Request(request))?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn clear_profile_preview(&mut self) {
        self.window.clear_layout_preview();
        self.preview_active = false;
        self.preview_sequence = self.preview_sequence.saturating_add(1);
    }
    fn sync_layout_selection(&self) -> std::result::Result<(), String> {
        if self.preview_active && self.screen == Screen::Profiles {
            self.window
                .set_layout_selection(
                    self.profiles
                        .as_ref()
                        .and_then(profiles_editor::Editor::selected_node_key),
                )
                .map_err(|_| "布局选中框更新失败".to_owned())?;
        }
        Ok(())
    }
    fn profiles_memory_gate(&self) -> bool {
        self.control.as_ref().is_some_and(|control| {
            !control.is_local_locked() && control.health_gate().check_sensitive().is_ok()
        })
    }
    fn open_profiles(&mut self) -> std::result::Result<(), String> {
        if self.pending.is_some() || self.stopping {
            return Ok(());
        }
        let Some(lease) = self.lease else {
            return Ok(());
        };
        if self.current_global_owner() != Some(lease.principal_id()) || !self.profiles_memory_gate()
        {
            self.status = "公共布局与科目配置仅限已解锁的本地主管理员。".into();
            return self.render();
        }
        self.clear_profile_preview();
        if self.profile_content_owner == Some(lease.principal_id())
            && self
                .profile_content
                .as_ref()
                .is_some_and(|editor| editor.is_dirty())
        {
            self.screen = Screen::ProfileContent;
            self.status.clear();
            return self.render();
        }
        self.screen = Screen::Profiles;
        self.status.clear();
        if self
            .profiles
            .as_ref()
            .is_some_and(|e| e.owner() == lease.principal_id() && e.has_unsaved_changes())
        {
            return self.render();
        }
        self.profiles = None;
        self.dispatch_profile_command(profiles_editor::Command::ReloadList { after: None })?;
        self.render()
    }
    fn profiles_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        if self.current_global_owner() != Some(lease.principal_id()) || !self.profiles_memory_gate()
        {
            self.lock();
            return self.render();
        }
        let Some(editor) = &mut self.profiles else {
            return self.render();
        };
        if editor.owner() != lease.principal_id() {
            self.profiles = None;
            self.clear_profile_preview();
            self.screen = Screen::Home;
            return self.render();
        }
        match editor.handle(action, data, &self.state) {
            Ok(command) => self.dispatch_profile_command(command)?,
            Err(message) => self.status = message,
        }
        self.render()
    }
    fn dispatch_profile_command(
        &mut self,
        command: profiles_editor::Command,
    ) -> std::result::Result<(), String> {
        use profiles_editor::Command as C;
        match command {
            C::ManageContent { slot, kind } => {
                let Some(owner) = self.current_global_owner() else {
                    return self.render();
                };
                if !self.profiles_memory_gate() {
                    self.lock();
                    return self.render();
                }
                self.clear_profile_preview();
                let keep = self.profile_content_owner == Some(owner)
                    && self
                        .profile_content
                        .as_ref()
                        .is_some_and(|editor| editor.slot() == slot && editor.is_dirty());
                if !keep {
                    self.profile_content = Some(profile_content_editor::Editor::new(slot, kind)?);
                }
                self.profile_content_owner = Some(owner);
                self.screen = Screen::ProfileContent;
                self.status.clear();
                if !keep {
                    self.submit_profile_content(profile_content_flow::Operation::Load { slot })?;
                }
            }
            C::Stay => {
                self.clear_profile_preview();
                self.status.clear();
            }
            C::Close => {
                self.clear_profile_preview();
                self.screen = Screen::Home;
                self.status.clear();
            }
            operation => {
                let Some(lease) = self.lease else {
                    self.lock();
                    return Ok(());
                };
                self.clear_profile_preview();
                if matches!(&operation, C::Preview { .. }) {
                    self.desktop.release_for_preview();
                }
                self.preview_sequence = self
                    .preview_sequence
                    .checked_add(1)
                    .ok_or("预览代次已用尽")?;
                if let Some(backend) = &self.backend {
                    self.submit(Work::Profile(
                        backend.clone(),
                        lease,
                        Box::new(operation),
                        self.preview_sequence,
                        self.root.clone(),
                    ))?;
                }
            }
        }
        Ok(())
    }
    fn submit_profile_content(
        &mut self,
        operation: profile_content_flow::Operation,
    ) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        if !self.profiles_memory_gate() || self.current_global_owner() != self.profile_content_owner
        {
            self.lock();
            return self.render();
        }
        let Some(backend) = self.backend.clone() else {
            return Ok(());
        };
        self.submit(Work::ProfileContent(Box::new(
            profile_content_flow::ContentJob {
                root: self.root.clone(),
                backend,
                lease,
                operation,
            },
        )))?;
        if self.pending.is_none() {
            if let Some(editor) = &mut self.profile_content {
                editor.cancel_review();
            }
            self.status = "后台繁忙，本次未排入任务；请重新读取核对后再选择文件。".into();
        }
        Ok(())
    }
    fn profile_content_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        if !self.profiles_memory_gate() || self.current_global_owner() != self.profile_content_owner
        {
            self.lock();
            return self.render();
        }
        let Some(editor) = &mut self.profile_content else {
            return self.render();
        };
        use profile_content_editor::Command as C;
        match editor.command(action, &data) {
            Ok(C::Stay) => self.status.clear(),
            Ok(C::Close) => {
                self.profile_content = None;
                self.profile_content_owner = None;
                self.screen = Screen::Profiles;
                self.status.clear();
            }
            Ok(C::Reload) => {
                let slot = editor.slot();
                self.submit_profile_content(profile_content_flow::Operation::Load { slot })?;
            }
            Ok(C::PickTxt) => {
                let selected = self.window.select_content_txt();
                if !self.profiles_memory_gate()
                    || self.current_global_owner() != self.profile_content_owner
                {
                    self.lock();
                    return self.render();
                }
                self.last_input = Instant::now();
                match selected {
                    Ok(Some(file)) => {
                        if let Some(editor) = &mut self.profile_content {
                            if let Err(message) = editor.accept_txt(file) {
                                self.status = message;
                            } else {
                                self.status.clear();
                            }
                        }
                    }
                    Ok(None) => self.status = "已取消选择TXT。".into(),
                    Err(_) => self.status = "无法选择本机TXT文件。".into(),
                }
            }
            Ok(C::PickImages) => {
                let selected = self.window.select_content_images();
                if !self.profiles_memory_gate()
                    || self.current_global_owner() != self.profile_content_owner
                {
                    self.lock();
                    return self.render();
                }
                self.last_input = Instant::now();
                match selected {
                    Ok(files) if files.is_empty() => self.status = "已取消选择图片。".into(),
                    Ok(files) => {
                        if let Some(editor) = &mut self.profile_content {
                            if let Err(message) = editor.accept_images(files) {
                                self.status = message;
                            } else {
                                self.status.clear();
                            }
                        }
                    }
                    Err(_) => self.status = "无法选择图片；每次最多50张。".into(),
                }
            }
            Ok(C::Submit {
                slot,
                expected_revision,
                operation,
                input,
            }) => self.submit_profile_content(profile_content_flow::Operation::Publish {
                slot,
                expected_revision,
                operation,
                input,
            })?,
            Err(message) => self.status = message,
        }
        self.render()
    }
    fn layout_canvas_event(
        &mut self,
        generation: u64,
        scope: fc_domain::layout::LayoutScope,
        source: LayoutNodeId,
        drop: Option<(LayoutNodeId, Option<LayoutNodeId>)>,
    ) -> std::result::Result<(), String> {
        if !self.preview_active
            || generation != self.preview_sequence
            || self.pending.is_some()
            || self.stopping
            || self.screen != Screen::Profiles
        {
            return Ok(());
        }
        let Some(owner) = self.current_global_owner() else {
            return Ok(());
        };
        if !self.profiles_memory_gate() {
            self.lock();
            return self.render();
        }
        self.last_input = Instant::now();
        if let (Some(control), Some(lease)) = (&self.control, self.lease) {
            let _ = control.note_input(lease, self.clock.monotonic_millis());
        }
        let Some(editor) = &mut self.profiles else {
            return Ok(());
        };
        if editor.owner() != owner {
            return Ok(());
        }
        if let Ok(data) = self.window.draft_nonsecret() {
            editor.suspend(data);
        }
        let result = match drop {
            Some((target, before)) => editor.drop_node(scope, source, target, before),
            None => editor.select_node(scope, source),
        };
        match result {
            Err(message) => self.status = message,
            Ok(()) if drop.is_some() => {
                match editor.handle(profiles_editor::PREVIEW, FormData::new(), &self.state) {
                    Ok(command) => self.dispatch_profile_command(command)?,
                    Err(message) => self.status = message,
                }
            }
            Ok(()) => self.status.clear(),
        }
        self.render()?;
        self.sync_layout_selection()
    }
    fn profile_loaded(&mut self, loaded: profile_flow::Loaded) -> std::result::Result<(), String> {
        if self.screen != Screen::Profiles {
            return Ok(());
        }
        if !self.profiles_memory_gate() {
            self.lock();
            return self.render();
        }
        let Some(owner) = self.current_global_owner() else {
            return Ok(());
        };
        use profile_flow::Loaded as L;
        match loaded {
            L::Directory { identity, page } => {
                self.state = Arc::new(identity);
                if self.current_global_owner() != Some(owner) {
                    self.lock();
                    return self.render();
                }
                if let Some(editor) = &mut self.profiles {
                    if editor.owner() == owner {
                        editor.loaded_list(page)?;
                    } else {
                        self.profiles =
                            Some(profiles_editor::Editor::new(&self.state, owner, page)?);
                    }
                } else {
                    self.profiles = Some(profiles_editor::Editor::new(&self.state, owner, page)?);
                }
                self.status.clear();
            }
            L::Public { state, saved } => {
                if saved {
                    if let Some(editor) = self
                        .display
                        .as_mut()
                        .filter(|editor| editor.owner() == owner && !editor.has_unsaved_changes())
                    {
                        editor.saved(state.display.clone());
                    }
                }
                if let Some(editor) = &mut self.profiles {
                    editor.loaded_public(state)?;
                }
                self.status = if saved {
                    "公共布局与显示设置已原子保存。".into()
                } else {
                    String::new()
                };
            }
            L::Subject {
                key,
                document,
                saved,
            } => {
                if let Some(editor) = &mut self.profiles {
                    editor.loaded_subject(key, document)?;
                }
                self.status = if saved {
                    "科目配置已保存，按当前实际班级、科目与老师生效；资料内容未修改。".into()
                } else {
                    String::new()
                };
            }
            L::Preview(preview) => {
                if preview.sequence != self.preview_sequence {
                    return Ok(());
                }
                let source_revision = preview.source_revision;
                let preview_current = |control: &SessionControl| {
                    let status = control.health_gate().status();
                    status.health == fc_runtime::health::Health::Healthy
                        && status.revision == source_revision
                };
                if !self.profiles_memory_gate()
                    || self
                        .control
                        .as_ref()
                        .is_none_or(|control| !preview_current(control))
                {
                    self.status = "预览内容已变化，请重新预览。".into();
                    return self.render();
                }
                if self.offscreen_probe {
                    profile_content_probe::observe_preview(&preview);
                }
                let profile_flow::Preview {
                    identity,
                    public,
                    subject,
                    sequence,
                    now,
                    source_revision: _,
                    contents,
                    text,
                    images,
                } = *preview;
                let rendered = profiles_preview::render_with_content(
                    &identity,
                    &public,
                    subject.as_ref(),
                    sequence,
                    now,
                    &contents,
                    &text,
                    profile_content_flow::raster_cache(),
                );
                drop(images);
                let image = match rendered {
                    Ok(image) => image,
                    Err(message) => {
                        self.status = message;
                        return self.render();
                    }
                };
                if !self.profiles_memory_gate()
                    || self
                        .control
                        .as_ref()
                        .is_none_or(|control| !preview_current(control))
                {
                    self.status = "预览内容已变化，请重新预览。".into();
                    return self.render();
                }
                self.status = "布局预览中，内置组件使用示例，独立节点显示已发布内容，尚未发布的节点为空。树页可选择并拖动节点；预览不会保存修改。".into();
                self.render()?;
                self.window
                    .set_layout_preview(Some(image))
                    .map_err(|_| "布局预览显示失败".to_owned())?;
                self.preview_active = true;
                self.sync_layout_selection()?;
            }
        }
        Ok(())
    }
    fn open_accounts(&mut self) -> std::result::Result<(), String> {
        if self.pending.is_some() || self.stopping {
            return Ok(());
        }
        let Some(lease) = self.lease else {
            return Ok(());
        };
        if self.current_global_owner() != Some(lease.principal_id())
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
        {
            self.status = "账号与任教管理仅限已解锁的本地主管理员。".into();
            return self.render();
        }
        if self
            .accounts
            .as_ref()
            .is_some_and(|e| e.owner() != lease.principal_id())
        {
            self.accounts = None;
        }
        self.screen = Screen::Accounts;
        self.status.clear();
        if let Some(backend) = &self.backend {
            self.submit(Work::LoadAccounts(backend.clone(), lease))?;
        }
        Ok(())
    }
    fn accounts_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        if self.current_global_owner() != Some(lease.principal_id())
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
            || action == LOCK
        {
            self.lock();
            return self.render();
        }
        if action == accounts_editor::END_RESULT_QUERY {
            if self.pending.is_none()
                && !self.stopping
                && self.native_unknown
                && self
                    .native_command
                    .as_ref()
                    .is_some_and(NativeCommand::is_account)
            {
                let original = self
                    .native_command
                    .as_ref()
                    .and_then(NativeCommand::snapshot)
                    .filter(|issued| self.account_notice_owner() == Some(issued.owner))
                    .map(|issued| format!("（操作编号{}）", issued.ticket))
                    .unwrap_or_default();
                let Some(owner) = self.account_notice_owner() else {
                    return self.render();
                };
                self.account_history_note = Some(accounts_editor::HistoryNotice::new(lease, owner,
                    format!("原账号操作{original}的历史结果仍未知；你已结束这次核对。此动作不会重新保存，也不证明原操作成功或失败。新操作需按当前目录重新确认。")));
                self.native_unknown = false;
                self.native_command = None;
                self.native_target = None;
                self.accounts = None;
                if let Some(backend) = &self.backend {
                    self.submit(Work::LoadAccounts(backend.clone(), lease))?;
                }
            }
            return self.render();
        }
        let Some(editor) = &mut self.accounts else {
            return self.render();
        };
        if editor.owner() != lease.principal_id() {
            self.accounts = None;
            self.screen = Screen::Home;
            return self.render();
        }
        match editor.handle(action, data, &self.state) {
            Ok(accounts_editor::Command::Stay) => self.status.clear(),
            Ok(accounts_editor::Command::Close) => {
                self.screen = Screen::Home;
                self.status.clear();
            }
            Ok(accounts_editor::Command::Reload) => {
                self.accounts = None;
                if let Some(backend) = &self.backend {
                    self.submit(Work::LoadAccounts(backend.clone(), lease))?;
                }
            }
            Ok(
                operation @ (accounts_editor::Command::Create { .. }
                | accounts_editor::Command::Update { .. }),
            ) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::SaveAccount(
                        backend.clone(),
                        lease,
                        Box::new(operation),
                    ))?;
                }
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
    fn open_web_editor(&mut self) -> std::result::Result<(), String> {
        if self.pending.is_some() || self.stopping {
            return Ok(());
        }
        if self.close_web.is_some() || self.web_close_job.is_some() {
            self.status = "正在关闭上一配对窗口，请稍后重新打开。".into();
            return self.render();
        }
        let Some(lease) = self.lease else {
            return Ok(());
        };
        if !web_editor::authorized(&self.state, lease)
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
        {
            self.status = "教师设备管理仅限已解锁的本地主管理员。".into();
            return self.render();
        }
        self.web_editor = None;
        self.web_service = None;
        self.screen = Screen::Web;
        self.status.clear();
        if let Some(backend) = &self.backend {
            self.submit(Work::OpenWeb(backend.clone(), lease))?;
        }
        Ok(())
    }
    fn leave_web(&mut self) {
        if let Some(editor) = &mut self.web_editor {
            editor.clear_sensitive();
        }
        self.web_editor = None;
        if let (Some(service), Some(lease)) = (self.web_service.take(), self.lease) {
            self.close_web = Some((service, lease));
            self.next_web_refresh = Instant::now();
        }
    }
    fn web_event(&mut self, action: ActionId, data: FormData) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        if self
            .control
            .as_ref()
            .is_none_or(SessionControl::is_local_locked)
        {
            self.lock();
            return self.render();
        }
        let command = self
            .web_editor
            .as_mut()
            .ok_or("教师设备页面已关闭")?
            .handle(action, &data, &self.state, lease);
        match command {
            Ok(web_editor::Command::OpenHttpsSettings) => {
                return self.open_https_settings();
            }
            Ok(web_editor::Command::Stay) => {
                self.status.clear();
                self.render()?;
            }
            Ok(web_editor::Command::Back) => {
                self.leave_web();
                self.screen = Screen::Home;
                self.status.clear();
                self.render()?;
            }
            Ok(web_editor::Command::Lock) => {
                self.lock();
                self.render()?;
            }
            Ok(web_editor::Command::Run(web_editor::Operation::Enable))
                if self.web_readiness != web_editor::Readiness::Unavailable
                    && !self.web_readiness.synthetic() =>
            {
                return self.start_teacher_https(lease);
            }
            Ok(web_editor::Command::Run(web_editor::Operation::Disable))
                if !self.web_readiness.synthetic() =>
            {
                self.stop_teacher_https();
                self.status = "正在停用教师连接并清理未完成传输，本地工作台仍可使用。".into();
                return self.render();
            }
            Ok(web_editor::Command::Run(operation)) => {
                if let Some(service) = &self.web_service {
                    self.next_web_refresh = Instant::now() + Duration::from_secs(1);
                    self.submit(Work::Web(
                        service.clone(),
                        lease,
                        operation,
                        self.web_readiness,
                    ))?;
                }
            }
            Err(message) => {
                self.status = message;
                self.render()?;
            }
        }
        Ok(())
    }
    fn close_public_lesson(&mut self) {
        if let Some(flow) = self.public_lesson_flow.take() {
            flow.cancel();
            self.generation = self.generation.saturating_add(1);
            if let Some(pending) = &self.pending {
                pending.ticket.cancel();
            }
            self.public_lesson_editor = None;
            self.lesson_flow_active = false;
            self.window.hide();
            self.screen = if self.state.installation.is_none() {
                Screen::Setup
            } else if self.lease.is_some() {
                Screen::Home
            } else {
                Screen::Login
            };
            self.status.clear();
            self.desktop.hide();
        }
    }
    fn open_public_lesson(
        &mut self,
        stamp: fc_bootstrap::backend::DesktopLessonStamp,
    ) -> std::result::Result<(), String> {
        if self.pending.is_some() || self.stopping || self.public_lesson_flow.is_some() {
            return Ok(());
        }
        let Some(flow) = self
            .control
            .as_ref()
            .and_then(|c| c.begin_desktop_lesson_flow(&stamp).ok())
        else {
            self.desktop_notice = "本节课程已变化，请重新点击桌面按钮。".into();
            return Ok(());
        };
        let guard = flow.guard();
        self.public_lesson_flow = Some(flow);
        self.public_lesson_editor = None;
        self.lesson_flow_active = true;
        self.screen = Screen::Lesson;
        self.status.clear();
        if !self.offscreen_probe {
            self.window.show(true);
        }
        if let Some(backend) = &self.backend {
            self.submit(Work::LoadPublicLesson(backend.clone(), guard))?;
        }
        Ok(())
    }
    fn public_lesson_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        // This branch precedes every authenticated navigation/LOCK handler.
        if action == public_lesson_editor::CLOSE {
            if let Some(editor) = &mut self.public_lesson_editor {
                if let Err(message) = editor.handle(action, data) {
                    self.status = message;
                    return self.render();
                }
            } else if !data.is_empty() {
                return Ok(());
            }
            self.close_public_lesson();
            return self.render();
        }
        if self.pending.is_some() || self.stopping {
            return Ok(());
        }
        let Some(guard) = self.public_lesson_flow.as_ref().map(|f| f.guard()) else {
            return Ok(());
        };
        if !guard.is_current() {
            if !guard.can_keep_chooser_open() {
                self.close_public_lesson();
            }
            return self.render();
        }
        let Some(editor) = &mut self.public_lesson_editor else {
            return Ok(());
        };
        match editor.handle(action, data) {
            Ok(public_lesson_editor::Command::Stay) => {
                self.status.clear();
                self.render()
            }
            Ok(public_lesson_editor::Command::Close) => {
                self.close_public_lesson();
                self.render()
            }
            Ok(public_lesson_editor::Command::Reload) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::LoadPublicLesson(backend.clone(), guard))?;
                }
                Ok(())
            }
            Ok(public_lesson_editor::Command::Apply(action)) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::ChangePublicLesson(backend.clone(), guard, action))?;
                }
                Ok(())
            }
            Err(message) => {
                self.status = message;
                self.render()
            }
        }
    }
    fn open_lesson_editor(&mut self) -> std::result::Result<(), String> {
        if self.pending.is_some() || self.stopping {
            return Ok(());
        }
        self.lesson_flow_active = true;
        if !self.offscreen_probe {
            self.window.show(true);
        }
        let Some(lease) = self.lease else {
            self.lesson_entry_pending = true;
            self.screen = Screen::Login;
            self.status = "请以本地主管理员身份登录后操作本节临时换课。".into();
            return self.render();
        };
        if !self.state.account(lease.principal_id()).is_ok_and(|a| {
            a.principal.enabled && a.principal.role == Role::GlobalAdmin && !a.must_change_password
        }) {
            self.lesson_entry_pending = false;
            self.lesson_flow_active = false;
            self.status = "本节临时换课仅限本地主管理员。".into();
            return self.render();
        }
        self.lesson_entry_pending = false;
        if let Some(backend) = &self.backend {
            self.submit(Work::LoadLesson(backend.clone(), lease))?;
        }
        Ok(())
    }
    fn lesson_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        let command = self
            .lesson_editor
            .as_mut()
            .ok_or("本节课程页面已关闭")?
            .handle(action, data, &self.state);
        match command {
            Ok(lesson_editor::Command::Stay) => {
                self.status.clear();
                self.render()?;
            }
            Err(message) => {
                self.status = message;
                self.render()?;
            }
            Ok(lesson_editor::Command::Close) => {
                self.lesson_editor = None;
                self.lesson_flow_active = false;
                self.screen = Screen::Home;
                self.status.clear();
                self.render()?;
            }
            Ok(lesson_editor::Command::Lock) => {
                self.lock();
                self.render()?;
            }
            Ok(lesson_editor::Command::Reload) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::LoadLesson(backend.clone(), lease))?;
                }
            }
            Ok(lesson_editor::Command::Apply {
                expected,
                lesson,
                action,
            }) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::ChangeLesson(
                        backend.clone(),
                        lease,
                        expected,
                        lesson,
                        action,
                    ))?;
                }
            }
        }
        Ok(())
    }
    fn schedule_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        if !self
            .state
            .account(lease.principal_id())
            .is_ok_and(|a| a.principal.role == Role::GlobalAdmin)
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
        {
            self.lock();
            return self.render();
        }
        if action == LOCK {
            self.lock();
            return self.render();
        }
        if self
            .schedule
            .as_ref()
            .is_none_or(|e| e.owner() != lease.principal_id())
        {
            self.screen = Screen::Home;
            self.status = "当前账号没有可继续的课表草稿。".into();
            return self.render();
        }
        if action == schedule_editor::OPEN_PUBLIC_CALENDAR {
            if let Some(editor) = &mut self.schedule {
                editor.capture(data, &self.state);
            }
            return self.open_calendar();
        }
        let command =
            self.schedule
                .as_mut()
                .ok_or("课表草稿不可用")?
                .handle(action, data, &self.state);
        match command {
            Ok(EditorCommand::Stay) => self.status.clear(),
            Ok(EditorCommand::Close) => {
                self.screen = Screen::Home;
                self.status.clear();
            }
            Ok(EditorCommand::Reload) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::LoadSchedule(backend.clone(), lease))?;
                }
            }
            Ok(EditorCommand::Save { expected, document }) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::SaveSchedule(
                        backend.clone(),
                        lease,
                        expected,
                        *document,
                    ))?;
                }
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
    fn submit_material(&mut self, job: material_flow::Job) -> std::result::Result<(), String> {
        let original_batch = match &job {
            material_flow::Job::ImportBatch { scope, command, .. } => {
                Some((material_flow::BatchKind::Import, *scope, command.clone()))
            }
            material_flow::Job::DeleteBatch { scope, command, .. } => {
                Some((material_flow::BatchKind::Delete, *scope, command.clone()))
            }
            _ => None,
        };
        if let (Some(backend), Some(control)) = (&self.backend, &self.control) {
            self.submit(Work::Material(Box::new(material_flow::MaterialJob {
                root: self.root.clone(),
                backend: backend.clone(),
                control: control.clone(),
                job,
            })))?;
        }
        if self.pending.is_none() {
            if let (Some(editor), Some((kind, scope, command))) =
                (&mut self.material_editor, original_batch)
            {
                editor.batch_result(material_flow::BatchResult {
                    kind,
                    scope,
                    issued: command.snapshot(),
                    command,
                    outcome: material_flow::BatchOutcome::NotSubmitted(
                        "后台未接纳本批任务；没有执行文件准备或目录写入。".into(),
                    ),
                })?;
                return self.render();
            }
        }
        Ok(())
    }
    fn material_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        use materials_editor::Command;
        if self.pending.is_some() {
            self.status = "原任务尚未返回，请保留本批清单等待结果。".into();
            return self.render();
        }
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        if self
            .control
            .as_ref()
            .is_none_or(SessionControl::is_local_locked)
        {
            self.lock();
            return self.render();
        }
        let command = self
            .material_editor
            .as_mut()
            .ok_or("资料页面不可用")?
            .handle(action, data, &self.state);
        match command {
            Ok(Command::Stay) => self.status.clear(),
            Ok(Command::Copies { clean }) => {
                self.submit_material(material_flow::Job::Copies { lease, clean })?
            }
            Ok(Command::Close) => {
                self.screen = Screen::Home;
                self.status.clear();
            }
            Ok(Command::Lock) => self.lock(),
            Ok(Command::Load(request)) => {
                self.submit_material(material_flow::Job::Load { lease, request })?
            }
            Ok(Command::ImportBatch {
                scope,
                classes,
                selected,
                command,
            }) => {
                self.submit_material(material_flow::Job::ImportBatch {
                    lease,
                    scope,
                    classes,
                    selected,
                    command,
                })?;
            }
            Ok(Command::DeleteBatch {
                scope,
                records,
                command,
            }) => {
                self.submit_material(material_flow::Job::DeleteBatch {
                    lease,
                    scope,
                    records,
                    command,
                })?;
            }
            Ok(Command::QueryBatch {
                kind,
                scope,
                command,
            }) => {
                self.submit_material(material_flow::Job::QueryBatch {
                    kind,
                    lease,
                    scope,
                    command,
                })?;
            }
            Ok(Command::Delete {
                scope,
                material_id,
                expected_material_revision,
            }) => self.submit_material(material_flow::Job::Delete {
                lease,
                request: material_flow::DeleteDraft {
                    scope,
                    material_id,
                    expected_material_revision,
                },
            })?,
            Ok(Command::Open {
                scope,
                material_id,
                resource_version,
            }) => self.submit_material(material_flow::Job::Open(
                material_flow::OpenRequest::Owner {
                    lease,
                    scope,
                    id: material_id,
                    version: resource_version,
                },
            ))?,
            Ok(Command::Import {
                scope,
                allowed_class_ids,
            }) => {
                // Native modal loop still executes the host's independent WTS /
                // power callback. Never refresh a stale lease when it returns.
                match self.window.select_material_batch() {
                    Ok(selected) if selected.is_empty() => self.status = "已取消选择资料。".into(),
                    Ok(selected)
                        if self.control.as_ref().is_some_and(|c| {
                            c.note_input(lease, self.clock.monotonic_millis())
                        }) =>
                    {
                        self.last_input = Instant::now();
                        match self
                            .material_editor
                            .as_mut()
                            .ok_or("资料页面已关闭")?
                            .stage_batch(scope, allowed_class_ids, selected)
                        {
                            Ok(()) => self.status = "请调整并确认本批清单，尚未校验或导入。".into(),
                            Err(message) => self.status = message,
                        }
                    }
                    Ok(_) => {
                        self.lock();
                        self.status = "会话已锁定，请重新登录后添加资料。".into();
                    }
                    Err(_) => self.status = "无法选择本机资料文件。".into(),
                }
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
    fn dispatch_material(
        &mut self,
        launch: material_flow::PreparedLaunch,
    ) -> std::result::Result<(), String> {
        use fc_windows::materials::shell::*;
        let Some(control) = &self.control else {
            return Ok(());
        };
        let copy_path = launch.path.path().to_owned();
        let request_id = launch.request_id;
        let gate = control.health_gate();
        let Ok(lease) =
            gate.lease_for_authorized_scope(MATERIAL_OPEN_SCOPE, launch.guard.expires_at)
        else {
            self.cleanup_paths.push_back(copy_path);
            self.status = "打开权限已失效，请重新选择。".into();
            return Ok(());
        };
        let guard = launch.guard;
        let request = LaunchRequest {
            request_id: launch.request_id,
            version: launch.resource_version,
            path: launch.path,
            gate,
            lease,
            origin: if launch.from_scene {
                LaunchOrigin::CurrentDesktop
            } else {
                LaunchOrigin::OwnerWorkbench
            },
            recheck: Arc::new(move || guard.is_valid()),
        };
        self.status = match self.shell.submit(request) {
            Ok(()) => {
                self.launch_paths.insert(request_id, copy_path);
                "正在请求系统打开资料。".into()
            }
            Err(_) => {
                self.cleanup_paths.push_back(copy_path);
                "打开队列繁忙，请稍后重试。".into()
            }
        };
        Ok(())
    }
    fn poll_shell(&mut self) {
        use fc_windows::materials::shell::LaunchOutcome;
        for result in self.shell.take_results() {
            if let Some(path) = self.launch_paths.remove(&result.request_id) {
                if result.outcome != LaunchOutcome::Requested {
                    self.cleanup_paths.push_back(path);
                }
            }
            self.status = match result.outcome {
                LaunchOutcome::Requested => "已请求系统打开资料副本。".into(),
                LaunchOutcome::AssociationMissing => "本机尚未设置此类文件的打开程序。".into(),
                LaunchOutcome::Canceled => "打开已取消，或资料/会话/桌面状态已改变。".into(),
                LaunchOutcome::Failed(_) => "系统未能打开资料，请检查关联程序后重试。".into(),
            };
            if self.screen == Screen::Materials {
                let _ = self.render();
            }
        }
    }
    fn has_unsaved_changes(&self) -> bool {
        let Some(owner) = self.current_global_owner() else {
            return false;
        };
        self.backup.has_review(owner)
            || self
                .voice_editor
                .as_ref()
                .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
            || self
                .autostart
                .as_ref()
                .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
            || self
                .schedule
                .as_ref()
                .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
            || self
                .calendar
                .as_ref()
                .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
            || self
                .display
                .as_ref()
                .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
            || self.profile_content_owner == Some(owner)
                && self
                    .profile_content
                    .as_ref()
                    .is_some_and(|editor| editor.is_dirty())
            || self
                .accounts
                .as_ref()
                .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
            || self
                .subject_editor
                .as_ref()
                .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
            || self
                .profiles
                .as_ref()
                .is_some_and(|e| e.owner() == owner && e.has_unsaved_changes())
    }
    fn current_global_owner(&self) -> Option<PrincipalId> {
        let id = self.lease?.principal_id();
        self.state
            .account(id)
            .ok()
            .filter(|a| a.principal.enabled && a.principal.role == Role::GlobalAdmin)
            .map(|_| id)
    }
    fn display_event(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            self.lock();
            return self.render();
        };
        if !self
            .state
            .account(lease.principal_id())
            .is_ok_and(|a| a.principal.role == Role::GlobalAdmin)
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
        {
            self.lock();
            return self.render();
        }
        if action == LOCK {
            self.lock();
            return self.render();
        }
        if self
            .display
            .as_ref()
            .is_none_or(|e| e.owner() != lease.principal_id())
        {
            self.screen = Screen::Home;
            self.status = "当前账号没有可继续的显示设置草稿。".into();
            return self.render();
        }
        self.clear_profile_preview();
        match self
            .display
            .as_mut()
            .ok_or("显示设置草稿不可用")?
            .handle(action, data)
        {
            Ok(display_editor::Command::Stay) => self.status.clear(),
            Ok(display_editor::Command::Close) => {
                self.screen = Screen::Home;
                self.status.clear();
            }
            Ok(display_editor::Command::Reload) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::LoadDisplay(backend.clone(), lease))?;
                }
            }
            Ok(display_editor::Command::Save {
                expected_palette,
                expected,
                colors,
                settings,
            }) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::SaveDisplay(
                        backend.clone(),
                        lease,
                        expected_palette,
                        expected,
                        colors,
                        settings,
                    ))?;
                }
            }
            Ok(display_editor::Command::VisualPreview { subject_id }) => {
                if let (Some(backend), Some(editor)) = (&self.backend, &self.display) {
                    let (palette, settings) = editor.visual_preview_input();
                    self.submit(Work::DisplayPreview(Box::new(display_flow::PreviewJob {
                        backend: backend.clone(),
                        lease,
                        palette,
                        settings,
                        subject: subject_id,
                        sequence: self.preview_sequence,
                    })))?;
                }
            }
            Ok(display_editor::Command::PickColor {
                subject_id,
                initial,
            }) => {
                if self.offscreen_probe {
                    self.status = "隔离探针不打开系统颜色选择器。".into();
                } else {
                    let chosen = self.window.select_color(initial);
                    if self.control.as_ref().is_none_or(|control| {
                        !control.note_input(lease, self.clock.monotonic_millis())
                            || control.health_gate().check_sensitive().is_err()
                    }) || self.lease != Some(lease)
                        || self.screen != Screen::Display
                    {
                        self.lock();
                    } else if let Some(editor) = self
                        .display
                        .as_mut()
                        .filter(|e| e.owner() == lease.principal_id())
                    {
                        match chosen {
                            Ok(Some(color)) => {
                                if let Err(message) = editor.picked_color(subject_id, color) {
                                    self.status = message;
                                }
                            }
                            Ok(None) => self.status = "已取消颜色选择，草稿保留。".into(),
                            Err(_) => self.status = "系统颜色选择器未完成，草稿保留。".into(),
                        }
                    }
                }
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
    fn poll(&mut self) -> std::result::Result<(), String> {
        if self.shell.reap_nonblocking().is_err() && !self.terminate_required {
            self.terminate_required = true;
            self.begin_shutdown();
        }
        if self
            .public_lesson_flow
            .as_ref()
            .is_some_and(|f| !f.guard().can_keep_chooser_open())
        {
            self.close_public_lesson();
        }
        // Even irreversible restore must observe the HTTP thread's actual
        // return before its coordinator can take the original shared workers.
        self.poll_teacher_https()?;
        self.poll_backup()?;
        if self.backup.irreversible() {
            return Ok(());
        }
        if !self.stopping
            && self.control.as_ref().is_some_and(|control| {
                matches!(
                    control.health_gate().status().health,
                    fc_runtime::health::Health::Faulted | fc_runtime::health::Health::Stopping
                )
            })
        {
            // Independent Web/voice writers can fault authority while this UI
            // is idle. Revoke its preview without waiting for a user action or
            // for the shared worker pool itself to fail.
            self.begin_shutdown();
            self.status = "数据健康状态已失效，正在关闭；请保留数据供修复。".into();
            self.render()?;
        }
        self.poll_voice_management()?;
        self.poll_https_settings()?;
        if self
            .web_editor
            .as_mut()
            .is_some_and(|editor| editor.tick(Instant::now()))
        {
            self.render()?;
        }
        if let Some(ticket) = &mut self.web_close_job {
            match ticket.try_result() {
                Ok(None) => {}
                Ok(Some(Ok(_))) => {
                    self.web_close_job = None;
                    self.close_web = None;
                }
                Ok(Some(Err(Failure::Web(WebServiceError::Busy) | Failure::Stopped)))
                | Err(
                    fc_runtime::worker::JobError::Deadline
                    | fc_runtime::worker::JobError::Cancelled,
                ) => {
                    self.web_close_job = None;
                    self.next_web_refresh = Instant::now() + Duration::from_secs(1);
                }
                _ => {
                    self.web_close_job = None;
                    self.close_web = None;
                }
            }
        }
        if self.web_close_job.is_none() && Instant::now() >= self.next_web_refresh {
            if let Some((service, lease)) = &self.close_web {
                if let Ok(ticket) = self.pool.handle().try_submit(
                    Work::CloseWeb(service.clone(), *lease),
                    Instant::now() + Duration::from_secs(5),
                ) {
                    self.web_close_job = Some(ticket);
                } else {
                    self.next_web_refresh = Instant::now() + Duration::from_secs(1);
                }
            }
        }
        if self.screen == Screen::Web
            && self.pending.is_none()
            && !self.stopping
            && Instant::now() >= self.next_web_refresh
        {
            if let (Some(service), Some(lease)) = (&self.web_service, self.lease) {
                self.next_web_refresh = Instant::now() + Duration::from_secs(1);
                self.submit(Work::Web(
                    service.clone(),
                    lease,
                    web_editor::Operation::Refresh,
                    self.web_readiness,
                ))?;
            }
        }
        if let Some(audio) = &self.audio {
            if audio.reap_nonblocking().is_err() && !self.stopping {
                self.terminate_required = true;
                self.begin_shutdown();
            }
        }
        if !self.stopping && Instant::now() >= self.next_audio_availability {
            self.next_audio_availability = Instant::now() + Duration::from_secs(1);
            if let Some(audio) = &self.audio {
                let available = !self.audio_suspended.load(Ordering::Acquire)
                    && fc_windows::host::interactive_session_available();
                if audio.set_available(available).is_err() {
                    self.terminate_required = true;
                    self.begin_shutdown();
                }
            }
        }
        if self.stopping {
            self.schedule_shutdown();
        }
        if let Some(ticket) = self.voice_maintenance.as_mut() {
            match ticket.try_result() {
                Ok(None) => {}
                Ok(Some(Ok(Outcome::MaterialCleaned))) => {
                    self.voice_maintenance = None;
                }
                Ok(Some(Err(Failure::Stopped))) => {
                    // Backend was occupied by a foreground management worker;
                    // maintenance never waits behind its KDF or extends a lease.
                    self.voice_maintenance = None;
                    self.next_voice_maintenance = Instant::now() + Duration::from_secs(1);
                }
                _ if self.stopping => {
                    self.voice_maintenance = None;
                }
                _ => {
                    if let Some(control) = &self.control {
                        control.fault();
                    }
                    return Err("声音数据维护失败，已停止本次运行。".into());
                }
            }
        }
        if !self.stopping
            && self.voice_maintenance.is_none()
            && Instant::now() >= self.next_voice_maintenance
            && self.state.installation.is_some()
        {
            if let Some(service) = &self.voice_service {
                if let Ok(ticket) = self.pool.handle().try_submit(
                    Work::MaintainVoice(self.root.clone(), service.clone()),
                    Instant::now() + Duration::from_secs(5),
                ) {
                    self.voice_maintenance = Some(ticket);
                    self.next_voice_maintenance = Instant::now() + Duration::from_secs(15);
                }
            }
        }
        // Node removal can drop the last current/Undo reference without another
        // publication. Retry physical removal on the shared pool; retain this
        // ticket until its worker really exits, including after cancellation.
        if let Some(ticket) = self.content_maintenance.as_mut() {
            match ticket.try_result() {
                Ok(None) => {}
                Ok(Some(Ok(Outcome::ContentMaintained { pending }))) => {
                    self.content_maintenance = None;
                    self.next_content_maintenance =
                        Instant::now() + Duration::from_secs(if pending { 1 } else { 30 });
                }
                Ok(Some(Err(error))) if fatal(&error) => {
                    if let Some(control) = &self.control {
                        control.fault();
                    }
                    self.begin_shutdown();
                }
                _ => {
                    self.content_maintenance = None;
                    self.next_content_maintenance = Instant::now() + Duration::from_secs(1);
                }
            }
        }
        if !self.stopping
            && self.pending.is_none()
            && self.content_maintenance.is_none()
            && Instant::now() >= self.next_content_maintenance
            && self.state.installation.is_some()
        {
            if let Some(backend) = &self.backend {
                match self.pool.handle().try_submit(
                    Work::MaintainProfileContent(
                        self.root.clone(),
                        backend.clone(),
                        self.content_gc.clone(),
                    ),
                    Instant::now() + Duration::from_secs(5),
                ) {
                    Ok(ticket) => self.content_maintenance = Some(ticket),
                    Err(_) => {
                        self.next_content_maintenance = Instant::now() + Duration::from_secs(1);
                    }
                }
            }
        }
        self.poll_calendar()?;
        self.poll_autostart()?;
        self.cleanup_jobs
            .retain_mut(|job| matches!(job.try_result(), Ok(None)));
        if !self.stopping && self.cleanup_jobs.len() < 2 {
            if let Some(path) = self.cleanup_paths.pop_front() {
                match self.pool.handle().try_submit(
                    Work::CleanupMaterial(self.root.clone(), path.clone()),
                    Instant::now() + Duration::from_secs(5),
                ) {
                    Ok(ticket) => self.cleanup_jobs.push(ticket),
                    Err(_) => self.cleanup_paths.push_front(path),
                }
            }
        }
        if let Some(ticket) = &mut self.shutdown {
            match ticket.try_result() {
                Ok(Some(Ok(Outcome::Closed))) => {
                    self.writer_closed = true;
                    self.shutdown = None;
                    module_registry::stop(module_registry::Kind::SharedWorkers, self);
                }
                Ok(None) => {}
                _ => {
                    self.shutdown = None;
                    self.terminate_required = true;
                    module_registry::stop(module_registry::Kind::SharedWorkers, self);
                }
            }
        }
        let batch_pending = self.native_mutation_pending
            && matches!(self.native_target, Some(NativeTarget::MaterialsBatch(..)));
        let mut clean_batch_return = false;
        let completed = match self.pending.as_mut() {
            // Completion can be enqueued just before the worker's done flag.
            // Do not consume this batch terminal event until disposal is done.
            Some(p) if batch_pending && !p.ticket.is_finished() => None,
            Some(p) => match p.ticket.try_result() {
                Ok(Some(outcome)) => {
                    clean_batch_return =
                        p.ticket.is_finished() && matches!(outcome, Err(Failure::Stopped));
                    Some(outcome)
                }
                Ok(None) => None,
                Err(error) => {
                    clean_batch_return = p.ticket.is_finished()
                        && matches!(
                            error,
                            fc_runtime::worker::JobError::Cancelled
                                | fc_runtime::worker::JobError::Deadline
                                | fc_runtime::worker::JobError::Stopped
                        );
                    Some(Err(Failure::Stopped))
                }
            },
            None => None,
        };
        let completed = if batch_pending && clean_batch_return {
            match (
                &self.native_command,
                &self.native_target,
                &self.material_editor,
            ) {
                (Some(command), Some(NativeTarget::MaterialsBatch(_, scope)), Some(editor))
                    if command.unissued_after_return()
                        && editor.has_original_batch(command, *scope) =>
                {
                    Some(Ok(Outcome::MaterialBatch(Box::new(
                        material_flow::BatchResult {
                            kind: editor
                                .original_batch_kind(command, *scope)
                                .expect("checked original batch"),
                            scope: *scope,
                            command: command.clone(),
                            issued: None,
                            outcome: material_flow::BatchOutcome::NotSubmitted(
                                "原后台任务已取消并实际返回，尚未取得批次票据，没有送交目录写入。"
                                    .into(),
                            ),
                        },
                    ))))
                }
                _ => completed,
            }
        } else {
            completed
        };
        #[cfg(test)]
        let completed = tests::drop_completed_history_for_test(self, completed);
        if let Some(outcome) = completed {
            if self.offscreen_probe && self.probe_trace {
                let kind = match &outcome {
                    Ok(Outcome::Updated { .. }) => "updated",
                    Ok(Outcome::LaunchCopies(_)) => "copies",
                    Ok(Outcome::LessonLoaded(_) | Outcome::LessonSaved(_)) => "lesson",
                    Ok(Outcome::ScheduleLoaded(_) | Outcome::ScheduleSaved(_)) => "schedule",
                    Ok(
                        Outcome::DisplayLoaded(..)
                        | Outcome::DisplaySaved(..)
                        | Outcome::DisplayPreview { .. },
                    ) => "display",
                    Err(_) => "error",
                    _ => "other",
                };
                eprintln!(
                    "PROBE_COMPLETE tick={} kind={} page={}",
                    self.clock.monotonic_millis(),
                    kind,
                    self.view().title
                );
            }
            let public_command = self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.public_lesson);
            if self.native_mutation_pending {
                if public_command {
                    self.public_lesson_unknown = outcome.is_err();
                } else {
                    self.native_unknown = outcome.is_err();
                }
            } else if self.native_readback_pending
                && self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.generation == self.generation)
                && !self.stopping
                && outcome.as_ref().is_ok_and(|outcome| {
                    if public_command {
                        NativeTarget::PublicLesson.confirms(outcome)
                    } else {
                        self.native_command
                            .as_ref()
                            .is_none_or(NativeCommand::permits_current_read_confirmation)
                            && self
                                .native_target
                                .as_ref()
                                .is_some_and(|target| target.confirms(outcome))
                    }
                })
            {
                if public_command {
                    self.public_lesson_unknown = false;
                } else {
                    self.native_unknown = false;
                }
            }
            if self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.generation == self.generation)
                && !self.stopping
            {
                if let Ok(outcome) = &outcome {
                    if let Some(history) = outcome.native_history() {
                        self.apply_native_history(history);
                    }
                }
            }
            self.native_mutation_pending = false;
            self.native_readback_pending = false;
            let completed_pending = self.pending.take();
            let public_lesson = completed_pending.as_ref().is_some_and(|p| p.public_lesson);
            let generation = completed_pending.map_or(self.generation, |p| p.generation);
            if let Some(https) = self.https_pending.take() {
                return self.https_completed(https, generation, outcome);
            }
            if generation != self.generation && public_lesson {
                self.render()?;
                return Ok(());
            }
            if generation != self.generation {
                if let Ok(Outcome::MaterialBatch(result)) = &outcome {
                    if let material_flow::BatchOutcome::NotSubmitted(reason) = &result.outcome {
                        if let Some(editor) = &mut self.material_editor {
                            let _ = editor.batch_result(material_flow::BatchResult {
                                kind: result.kind,
                                scope: result.scope,
                                command: result.command.clone(),
                                issued: result.issued,
                                outcome: material_flow::BatchOutcome::NotSubmitted(reason.clone()),
                            });
                        }
                    }
                }
                if let Ok(Outcome::MaterialLaunch(launch)) = &outcome {
                    self.cleanup_paths.push_back(launch.path.path().to_owned());
                }
                self.status = "已锁定；请重新登录后核对保存结果。".into();
                if !self.stopping {
                    if let Some(backend) = &self.backend {
                        self.submit(Work::Refresh(backend.clone()))?;
                    }
                }
                self.render()?;
                return Ok(());
            }
            match outcome {
                Ok(Outcome::Opened(backend, control, state, voice)) => {
                    self.backend = Some(backend);
                    module_registry::bind_voice(self, voice)?;
                    let _ = self.control_slot.set(control.clone());
                    self.control = Some(control);
                    if !self.offscreen_probe {
                        self.set_web_readiness(web_editor::Readiness::Disabled);
                    }
                    self.state = Arc::new(state);
                    self.screen = if self.state.installation.is_none() {
                        Screen::Setup
                    } else {
                        Screen::Login
                    };
                    self.status.clear();
                    if self.stopping {
                        self.schedule_shutdown();
                    }
                }
                Ok(Outcome::Updated {
                    state,
                    lease,
                    code,
                    next,
                }) => {
                    self.state = Arc::new(state);
                    if next == Screen::Code {
                        self.accounts = None;
                    }
                    let previous_lease = self.lease;
                    self.lease = lease;
                    self.screen = next;
                    if lease.is_some() && lease != previous_lease {
                        self.last_input = Instant::now();
                    }
                    self.status = "已保存。".into();
                    if next == Screen::Home && self.lesson_entry_pending {
                        self.open_lesson_editor()?;
                        return Ok(());
                    }
                    self.render()?;
                    if let Some(code) = code {
                        let shown = code.reveal();
                        self.window
                            .present_secret(CODE, &shown)
                            .map_err(|_| "恢复码显示失败，请登录后重新生成".to_owned())?;
                    }
                    return Ok(());
                }
                Ok(Outcome::Loaded { key, record }) => {
                    self.key = Some(key);
                    self.record = record;
                    self.screen = Screen::HomeworkEdit;
                    self.status.clear();
                }
                Ok(Outcome::HttpsRuntime(_)) => {
                    self.status = "启动结果已不属于当前请求；未开启配对。".into();
                }
                Ok(Outcome::Https(_)) => {
                    self.status = "配置页面已变化，请重新读取核对。".into();
                }
                Ok(Outcome::WebOpened(service, loaded)) => {
                    if let Some(lease) = self.lease {
                        self.web_editor = Some(web_editor::Editor::new(
                            lease.principal_id(),
                            self.web_readiness,
                            loaded,
                        ));
                        self.web_service = Some(service);
                        self.screen = Screen::Web;
                        self.status.clear();
                        self.next_web_refresh = Instant::now() + Duration::from_secs(1);
                    }
                }
                Ok(Outcome::VoiceLoaded(lease, loaded)) => {
                    let history_message = loaded.history.as_ref().map(NativeHistory::message);
                    self.voice_management_loaded(lease, loaded.snapshot)?;
                    if let Some(message) = history_message {
                        self.status = message.into();
                    }
                }
                Ok(Outcome::WebLoaded(loaded)) => {
                    if self.screen == Screen::Web {
                        let changed = loaded.changed;
                        if let Some(editor) = &mut self.web_editor {
                            editor.loaded(loaded);
                        }
                        self.status = if changed {
                            "申请或授权已变化；已重新读取，请核对后重新选择。".into()
                        } else {
                            "教师设备状态已更新。".into()
                        };
                        self.next_web_refresh = Instant::now() + Duration::from_secs(1);
                    }
                }
                Ok(Outcome::Profile(loaded)) => self.profile_loaded(*loaded)?,
                Ok(Outcome::ProfileContent(loaded)) => {
                    if self.screen == Screen::ProfileContent
                        && self.current_global_owner() == self.profile_content_owner
                        && self.profiles_memory_gate()
                    {
                        if let Some(editor) = &mut self.profile_content {
                            editor.loaded(loaded.document, loaded.cleanup_pending)?;
                        }
                        self.status = if loaded.outcome.is_some() {
                            "节点内容已保存；桌面将读取这一内容版本。".into()
                        } else {
                            "节点内容已读取。".into()
                        };
                    }
                }
                Ok(Outcome::DefaultTeacherLoaded {
                    current,
                    history,
                    saved,
                }) => {
                    let original = match &history.resolution {
                        NativeResolution::DefaultTeacher(Some((_, default))) => format!(
                            "原操作结果：班级#{}／科目#{}的默认老师为#{}。",
                            default.class_id.get(),
                            default.subject_id.get(),
                            default.teacher_id.get()
                        ),
                        _ => history.message().into(),
                    };
                    let pending = history.pending();
                    let rejected = matches!(
                        history.resolution,
                        NativeResolution::DefaultTeacherRejected(_)
                    );
                    self.state = Arc::new(current);
                    if let Some(owner) = self.current_global_owner() {
                        if rejected {
                            if let Some(editor) = &mut self.accounts {
                                editor.default_rejected();
                            }
                        } else if saved
                            || !pending
                            || self
                                .accounts
                                .as_ref()
                                .is_none_or(|editor| !editor.has_unsaved_changes())
                        {
                            self.accounts = Some(accounts_editor::Editor::new(&self.state, owner)?);
                        }
                        self.screen = Screen::Accounts;
                        self.status =
                            format!("{original} 当前目录已单独读取；历史结果不会覆盖当前设置。");
                    }
                }
                Ok(Outcome::SubjectSelfRevoked { state, .. }) => {
                    self.state = Arc::new(state);
                    self.subject_editor = None;
                    self.lock();
                    self.status =
                        "科目修改已保存。你也任教该科目，原会话已失效；请重新登录。".into();
                }
                Ok(Outcome::SubjectsLoaded {
                    facts,
                    history,
                    saved,
                }) => self.subjects_loaded(facts, history, saved)?,
                Ok(Outcome::AccountHistoryLoaded {
                    lease,
                    current,
                    history,
                }) => {
                    self.account_history_note = Some(accounts_editor::HistoryNotice::new(
                        lease,
                        history.issued.owner,
                        format!(
                            "历史记录（操作编号{}）：\n{}",
                            history.issued.ticket,
                            accounts_editor::history_summary(
                                history.message(),
                                match &history.resolution {
                                    NativeResolution::Account(Some(result)) => Some(result),
                                    _ => None,
                                }
                            )
                        ),
                    ));
                    // Only the independently authenticated CURRENT directory is
                    // published to the editor. Original result remains a label.
                    self.state = Arc::new(current);
                    if let Some(owner) = self.current_global_owner() {
                        if self
                            .accounts
                            .as_ref()
                            .is_none_or(|e| e.owner() != owner || !e.has_unsaved_changes())
                        {
                            self.accounts = Some(accounts_editor::Editor::new(&self.state, owner)?);
                        }
                        self.screen = Screen::Accounts;
                        self.status.clear();
                    }
                }
                Ok(Outcome::AccountsLoaded(state)) => {
                    self.state = Arc::new(state);
                    if let Some(owner) = self.current_global_owner() {
                        if self
                            .accounts
                            .as_ref()
                            .is_none_or(|e| e.owner() != owner || !e.has_unsaved_changes())
                        {
                            self.accounts = Some(accounts_editor::Editor::new(&self.state, owner)?);
                        }
                        self.screen = Screen::Accounts;
                        self.status.clear();
                    }
                }
                Ok(Outcome::AccountSaved {
                    state,
                    self_changed,
                }) => {
                    self.state = Arc::new(state);
                    self.accounts = None;
                    if self_changed {
                        // The committed change revoked this exact local lease. A
                        // successful completion does not issue replacement authority.
                        self.lease = None;
                        self.screen = Screen::Login;
                        self.status =
                            "账号变更已保存，本地会话已失效；请使用当前权限重新登录。".into();
                    } else if let Some(owner) = self.current_global_owner() {
                        self.accounts = Some(accounts_editor::Editor::new(&self.state, owner)?);
                        self.screen = Screen::Accounts;
                        self.status = "账号与任教范围已保存，该账号原有会话已失效。".into();
                    }
                }
                Ok(Outcome::Autostart(..)) => {}
                Ok(Outcome::Backup(..)) => return Err("备份结果进入了普通请求通道".into()),
                Ok(Outcome::Calendar(loaded)) => self.calendar_loaded(*loaded),
                Ok(Outcome::CalendarMaintained { .. }) => {}
                Ok(Outcome::ScheduleLoaded(document)) => {
                    if let Some(lease) = self.lease {
                        self.schedule = Some(ScheduleEditor::new(document, lease.principal_id()));
                        self.screen = Screen::Schedule;
                        self.status.clear();
                    }
                }
                Ok(Outcome::PublicLessonLoaded(source)) => {
                    if self
                        .public_lesson_flow
                        .as_ref()
                        .is_some_and(|f| f.guard().is_current())
                    {
                        self.public_lesson_editor = Some(public_lesson_editor::Editor::new(source));
                        self.status.clear();
                    } else {
                        self.close_public_lesson();
                    }
                }
                Ok(Outcome::PublicLessonSaved(_source)) => {
                    // One successful action consumes the original scene flow.
                    self.close_public_lesson();
                }
                Ok(Outcome::LessonLoaded(snapshot)) => {
                    self.lesson_editor = Some(lesson_editor::Editor::new(snapshot));
                    self.screen = Screen::Lesson;
                    self.status.clear();
                }
                Ok(Outcome::LessonSaved(snapshot)) => {
                    self.lesson_editor = Some(lesson_editor::Editor::new(snapshot));
                    self.status = "本节课程已保存；桌面将按同一份最新事实刷新。".into();
                }
                Ok(Outcome::ScheduleSaved(document)) => {
                    if let Some(editor) = &mut self.schedule {
                        editor.saved(document);
                    }
                    self.status = "课表、作息和校历已整体保存。".into();
                }
                Ok(Outcome::MaterialBatch(result)) => {
                    use material_flow::BatchOutcome;
                    let message = if result.kind == material_flow::BatchKind::Delete {
                        match &result.outcome {
                            BatchOutcome::Saved(_) => {
                                "原批次已一次删除。可单独刷新当前目录；原票据继续保留删除结果。"
                                    .to_owned()
                            }
                            BatchOutcome::Unknown => {
                                "原删除票据尚无确定结果；保留原清单查询，不重新删除。".to_owned()
                            }
                            BatchOutcome::NotSubmitted(reason) => {
                                format!("整批删除未送交目录写入：{reason} 未执行删除。")
                            }
                            BatchOutcome::Rejected(reason) => {
                                format!("整批删除已被明确拒绝：{reason} 没有删除部分资料。")
                            }
                            BatchOutcome::Expired => {
                                "原删除查询窗口已过期；请明确刷新当前目录核对，不会重执行原票据。"
                                    .to_owned()
                            }
                        }
                    } else {
                        match &result.outcome {
                        BatchOutcome::Saved(_) => "原批次已一次保存。请单独刷新目录查看当前状态；结束本次操作后可开始新批次。".to_owned(),
                        BatchOutcome::Unknown => "原票据当前没有确定批次结果；保留原票据，不会重新导入。".to_owned(),
                        BatchOutcome::NotSubmitted(reason) => format!("整批未送交目录写入：{reason} 原文件未变；结束本次操作后可重新选择。"),
                        BatchOutcome::Rejected(reason) => format!("本批目录写入已被明确拒绝：{reason} 整批没有加入；请核对权限或容量后重新选择。"),
                        BatchOutcome::Expired => "原批次查询窗口已过期，无法据此确定原保存结果；请明确刷新当前目录核对。不会重执行原票据。".to_owned(),
                    }
                    };
                    let target_matches = matches!(&self.native_target,
                        Some(NativeTarget::MaterialsBatch(original, scope))
                        if *scope == result.scope && self.lease.is_some_and(|current| NativeTarget::same_reader(current, *original)));
                    let original = result.command.clone();
                    let settled = !matches!(result.outcome, BatchOutcome::Unknown);
                    self.material_editor
                        .as_mut()
                        .ok_or("资料页面已关闭")?
                        .batch_result(*result)?;
                    if self.native_command.as_ref() == Some(&original) && target_matches {
                        self.native_unknown = !settled;
                    }
                    self.status = message;
                }
                Ok(Outcome::Materials(page)) => {
                    self.material_editor
                        .as_mut()
                        .ok_or("资料页面已关闭")?
                        .loaded(*page)?;
                    self.status = "资料列表已更新。".into();
                }
                Ok(Outcome::LaunchCopies(report)) => {
                    self.copies_loaded_sequence = self
                        .copies_loaded_sequence
                        .checked_add(1)
                        .ok_or("副本请求序号已耗尽")?;
                    self.material_editor
                        .as_mut()
                        .ok_or("资料页面已关闭")?
                        .copies_loaded(report);
                    self.status = "打开副本占用已更新。".into();
                }
                Ok(Outcome::MaterialLaunch(launch)) => {
                    self.dispatch_material(*launch)?;
                }
                Ok(Outcome::DisplayLoaded(settings, identity)) => {
                    self.state = identity;
                    if let Some(lease) = self.lease {
                        self.display = Some(display_editor::Editor::new_visuals(
                            settings,
                            lease.principal_id(),
                        ));
                        self.screen = Screen::Display;
                        self.status.clear();
                    }
                }
                Ok(Outcome::DisplayPreview {
                    sequence,
                    source_revision,
                    image,
                }) => {
                    if sequence == self.preview_sequence
                        && self.screen == Screen::Display
                        && self.profiles_memory_gate()
                        && self
                            .control
                            .as_ref()
                            .is_some_and(|c| c.health_gate().status().revision == source_revision)
                        && self.display.as_ref().is_some_and(|e| e.visual_page())
                    {
                        self.window
                            .set_layout_preview(Some(image))
                            .map_err(|_| "科目颜色示例显示失败。")?;
                        self.preview_active = true;
                        self.status = "已显示当前草稿的颜色与透明度示例，尚未保存。".into();
                    }
                }
                Ok(Outcome::DisplaySaved(settings, identity)) => {
                    self.state = identity;
                    if let Some(editor) = &mut self.display {
                        editor.visuals_saved(settings);
                    }
                    self.status = "桌面显示设置已保存，关闭工作台后在系统桌面生效。".into();
                }
                Ok(Outcome::ExitAuthorized) => self.begin_shutdown(),
                Ok(Outcome::Closed) => return Err("关闭结果进入了普通请求通道".into()),
                Ok(
                    Outcome::Http(_)
                    | Outcome::HttpProbe(_)
                    | Outcome::Office(_)
                    | Outcome::Desktop(..)
                    | Outcome::ContentImages(_)
                    | Outcome::ContentPage { .. }
                    | Outcome::ContentMaintained { .. }
                    | Outcome::MaterialCleaned,
                ) => return Err("后台结果类型不匹配".into()),
                Err(error) => {
                    if self.public_lesson_flow.is_some() {
                        let fatal_error = fatal(&error);
                        self.close_public_lesson();
                        if fatal_error {
                            if let Some(control) = &self.control {
                                control.fault();
                            }
                            self.begin_shutdown();
                        }
                        return self.render();
                    }
                    if self.screen == Screen::Voice {
                        if let Some(editor) = &mut self.voice_editor {
                            editor.invalidate();
                        } else {
                            self.screen = Screen::Home;
                        }
                    }
                    if self.screen == Screen::ProfileContent {
                        if let Some(editor) = &mut self.profile_content {
                            editor.cancel_review();
                        }
                    }
                    if matches!(error, Failure::Web(WebServiceError::Denied)) {
                        self.lock();
                    }
                    if self.screen == Screen::Web {
                        if let Some(editor) = &mut self.web_editor {
                            editor.clear_sensitive();
                        }
                    }
                    if fatal(&error) {
                        if let Some(control) = &self.control {
                            control.fault();
                        }
                        self.begin_shutdown();
                    }
                    if matches!(error, Failure::Stopped) && !self.stopping {
                        if let Some(backend) = &self.backend {
                            self.lease = None;
                            if let Some(control) = &self.control {
                                control.lock_local();
                            }
                            self.submit(Work::Refresh(backend.clone()))?;
                        }
                    }
                    if self.backend.is_none() {
                        self.screen = Screen::Repair;
                    }
                    self.status = error_text(&error);
                    if self
                        .native_command
                        .as_ref()
                        .and_then(NativeCommand::snapshot)
                        .is_some()
                    {
                        self.status
                            .push_str(" 原操作票据已保留；请明确核对当前状态，勿重复提交。");
                    }
                }
            }
            self.render()?;
        }
        Ok(())
    }
}

/// Manual launch opens management; --autostart remains locked and hidden.
pub fn run(autostart: bool) -> std::result::Result<(), String> {
    crate::installation_startup::run(|| {
        let paths = DataPaths::program_data().map_err(|_| "无法定位本机数据目录".to_owned())?;
        run_at(paths, autostart, false)
    })
}
fn run_at(paths: DataPaths, autostart: bool, probe: bool) -> std::result::Result<(), String> {
    let can_launch_production_repair = !probe
        && DataPaths::program_data().is_ok_and(|production| production.root() == paths.root());
    if run_at_inner(paths, autostart, probe)? {
        if !can_launch_production_repair {
            return Err("隔离工作台不得启动正式资料清除向导".into());
        }
        // All owners, including InstanceGuard, have left the inner scope.
        // Never launch a second process merely on a logical shutdown request.
        std::process::Command::new(
            std::env::current_exe().map_err(|_| "无法定位资料清除向导程序")?,
        )
        .arg("--repair")
        .spawn()
        .map_err(|_| "原程序已正常关闭；无法打开资料清除向导，请重新运行程序并重试")?;
    }
    Ok(())
}
fn run_at_inner(
    paths: DataPaths,
    autostart: bool,
    probe: bool,
) -> std::result::Result<bool, String> {
    // A torn cold switch may have deliberately moved state/teachers away. Do
    // not create empty replacements before checking its durable journal.
    if paths
        .root()
        .try_exists()
        .map_err(|_| "无法读取部署目录".to_owned())?
    {
        paths
            .verify_root_for_current_user()
            .map_err(|_| "部署根目录权限不符合要求，需要修复".to_owned())?;
    } else {
        paths
            .prepare_for_current_user()
            .and_then(|deployment| deployment.read_back())
            .map_err(|_| "无法创建受控部署目录".to_owned())?;
    }
    paths
        .prepare_instance_runtime()
        .map_err(|_| "实例运行目录权限不符合要求，需要修复".to_owned())?;
    let _runtime = fc_windows::materials::files::PinnedDirectory::open(&paths.runtime())
        .map_err(|_| "实例运行目录不是受控本地目录".to_owned())?;
    let _instance = InstanceGuard::acquire(paths.root())
        .map_err(|_| "未来教室已运行，或数据目录被其他会话占用".to_owned())?;
    let host = if probe {
        HiddenHost::for_isolated_probe()
    } else {
        HiddenHost::new(true)
    }
    .map_err(|_| "无法创建托盘宿主".to_owned())?;
    let stop = host.controller();
    let clock = Arc::new(CurrentClock::new());
    let mut app = create_app(clock.clone(), probe)?;
    app.root = paths.root().to_owned();
    app.backup.workflow = Some(fc_bootstrap::recovery_workflow::RecoveryWorkflow::new(
        paths.clone(),
    ));
    app.desktop.set_root(paths.root().to_owned());
    let control_slot = app.control_slot.clone();
    let audio_slot = app.audio_slot.clone();
    let audio_suspended = app.audio_suspended.clone();
    host.set_control_callback(Arc::new(move |hints| {
        // This direct path also runs inside modal native pumps. No backend
        // mutex, database, allocation, worker slot, or join is involved.
        if hints.contains(HostHints::SUSPENDING) {
            audio_suspended.store(true, Ordering::Release);
        }
        if hints.contains(HostHints::RESUMED) {
            audio_suspended.store(false, Ordering::Release);
        }
        if let Some(audio) = audio_slot.get() {
            if hints.contains(HostHints::SHUTDOWN) {
                audio.shutdown();
            } else if hints.contains(HostHints::SUSPENDING) {
                audio.request_stop(StopReason::Suspend);
            } else if hints.contains(HostHints::SESSION_CHANGED) {
                audio.request_stop(StopReason::SessionUnavailable);
            } else if hints.contains(HostHints::EMERGENCY_STOP_AUDIO) {
                audio.request_stop(StopReason::Emergency);
            }
        }
        if hints.contains(HostHints::FOREGROUND_UNAVAILABLE) {
            if let Some(control) = control_slot.get() {
                control.cancel_desktop_lesson_flow();
            }
        }
        if hints.contains(HostHints::SESSION_LOCKED)
            || hints.contains(HostHints::SESSION_CHANGED)
            || hints.contains(HostHints::POWER_CHANGED)
            || hints.contains(HostHints::SHUTDOWN)
        {
            if let Some(control) = control_slot.get() {
                // Immediate negative capability fence also runs inside native
                // modal pumps; the UI still owns and drains the old job ticket.
                control.cancel_desktop_lesson_flow();
                control.lock_local();
            }
        }
    }))
    .map_err(|_| "无法绑定会话撤销处理")?;
    app.render()?;
    if !autostart {
        app.window.show(true);
    }
    module_registry::start_writer(&mut app, paths.clone(), clock)?;
    let started = Instant::now();
    let mut stage = 0u8;
    let mut lesson_stage = 0u8;
    let mut copies_probe = CopiesProbe::default();
    let mut accounts_creation = 0u8;
    let mut accounts_management = 0u8;
    let mut profiles_stage = 0u8;
    let mut profile_content_stage = 0usize;
    let mut profile_content_probe = profile_content_probe::State::default();
    let mut schedule_operations_stage = 0usize;
    let mut schedule_operations_probe = schedule_operations_probe::State::default();
    let mut probe_error = None;
    let mut office_verified = !probe;
    let mut office = if probe {
        let path = paths.root().join("quarantine/minimal.docx");
        create_file(&path, include_bytes!("../tests/fixtures/minimal.docx"))
            .map_err(|e| error_text(&e))?;
        Some(
            app.pool
                .handle()
                .try_submit(
                    Work::ValidateOffice(path),
                    Instant::now() + Duration::from_secs(30),
                )
                .map_err(|_| "共享材料任务未接纳".to_owned())?,
        )
    } else {
        None
    };
    let result = host.run(|hints| {
        if app.pool.is_faulted() && !app.terminate_required {
            if let Some(control) = &app.control {
                control.fault();
            }
            app.terminate_required = true;
            app.begin_shutdown();
        }
        if hints.contains(HostHints::SHUTDOWN) {
            app.begin_shutdown();
        }
        if hints.contains(HostHints::SESSION_LOCKED) || hints.contains(HostHints::SESSION_CHANGED) {
            app.desktop.hide();
            app.close_public_lesson();
            app.lock();
            app.window.hide();
            let _ = app.render();
        }
        if !probe
            && app.lesson_flow_active
            && hints.contains(HostHints::FOREGROUND_CHANGED)
            && !app.window.lesson_foreground_allowed()
        {
            app.close_public_lesson();
            app.lock();
            app.window.hide();
            let _ = app.render();
        }
        if hints.contains(HostHints::OPEN_STATUS) {
            app.desktop.hide();
            if !app.window.is_visible() {
                let _ = app.render();
            }
            app.window.show(true);
        }
        if hints.contains(HostHints::EXIT_REQUESTED) {
            app.desktop.hide();
            app.window.show(true);
            app.status = "请由主管理员登录后在工作台中退出。".into();
            let _ = app.render();
        }
        if app.lease.is_some()
            && (app.last_input.elapsed() >= Duration::from_secs(300)
                || (app
                    .control
                    .as_ref()
                    .is_some_and(SessionControl::is_local_locked)
                    && !app.pending.as_ref().is_some_and(|p| p.self_revokes)))
        {
            app.lock();
            let _ = app.render();
        }
        for event in app.window.take_events()? {
            if app.event(event).is_err() {
                stop.fault();
                return Ok(());
            }
        }
        if app.poll().is_err() {
            stop.fault();
            return Ok(());
        }
        if !app.stopping {
            if let Err(error) = app.desktop.tick(
                app.backend.as_ref(),
                app.control.as_ref(),
                &app.pool,
                &app.clock,
                app.window.is_visible() || app.preview_active || app.screen == Screen::Profiles,
                app.pending.is_some(),
                hints,
            ) {
                probe_error = Some(error);
                app.begin_shutdown();
            }
            if let Some(notice) = app.desktop.take_notice() {
                app.desktop_notice = notice;
            }
            if let Some(stamp) = app.desktop.take_lesson_event() {
                if let Err(error) = app.open_public_lesson(stamp) {
                    probe_error = Some(error);
                    app.begin_shutdown();
                }
            }
        }
        if !app.stopping {
            for event in app.desktop.take_material_events() {
                if let fc_windows::desktop::DesktopEvent::MaterialOpen {
                    generation,
                    source,
                    material_id,
                    resource_version,
                    ..
                } = event
                {
                    if app.pending.is_none()
                        && app
                            .submit_material(material_flow::Job::Open(
                                material_flow::OpenRequest::Scene {
                                    source,
                                    generation,
                                    id: material_id,
                                    version: resource_version,
                                },
                            ))
                            .is_err()
                    {
                        stop.fault();
                        return Ok(());
                    }
                }
            }
            app.poll_shell();
        }
        if let Some(ticket) = &mut office {
            match ticket.try_result() {
                Ok(Some(Ok(Outcome::Office(report))))
                    if report.entries == 3 && report.expanded_bytes > 0 =>
                {
                    office_verified = true;
                    office = None;
                }
                Ok(None) => {}
                _ => {
                    probe_error = Some("共享worker材料校验未通过".into());
                    office = None;
                    app.begin_shutdown();
                }
            }
        }
        if probe && !app.stopping && app.pending.is_none() {
            if stage == 10 {
                if let Err(error) = accounts_probe::create(&mut app, &mut accounts_creation) {
                    probe_error = Some(error);
                    app.begin_shutdown();
                } else if accounts_creation == accounts_probe::CREATION_STEPS {
                    stage = 11;
                }
            } else if stage == 83 && lesson_stage < 9 {
                if let Err(error) = lesson_probe_step(&mut app, &mut lesson_stage) {
                    probe_error = Some(error);
                    app.begin_shutdown();
                }
            } else if stage == 83 && copies_probe.stage < 6 {
                if let Err(error) = copies_probe_step(&mut app, &mut copies_probe) {
                    probe_error = Some(error);
                    app.begin_shutdown();
                }
            } else if stage == 83 && accounts_management < accounts_probe::MANAGEMENT_STEPS {
                if let Err(error) = accounts_probe::manage(&mut app, &mut accounts_management) {
                    probe_error = Some(error);
                    app.begin_shutdown();
                }
            } else if stage == 83 && profiles_stage < profiles_probe::STEPS {
                if let Err(error) = profiles_probe::run(&mut app, &mut profiles_stage) {
                    probe_error = Some(error);
                    app.begin_shutdown();
                }
            } else if stage == 83 && profile_content_stage < profile_content_probe::STEPS {
                if let Err(error) = profile_content_probe::run(
                    &mut app,
                    &mut profile_content_probe,
                    &mut profile_content_stage,
                ) {
                    probe_error = Some(error);
                    app.begin_shutdown();
                }
            } else if stage == 83 && schedule_operations_stage < schedule_operations_probe::STEPS {
                if let Err(error) = schedule_operations_probe::run(
                    &mut app,
                    &mut schedule_operations_probe,
                    &mut schedule_operations_stage,
                ) {
                    probe_error = Some(error);
                    app.begin_shutdown();
                }
            } else if let Err(e) = probe_step(&mut app, &mut stage) {
                probe_error = Some(e);
                app.begin_shutdown();
            }
        }
        if probe && started.elapsed() > Duration::from_secs(180) && !app.stopping {
            probe_error = Some("原生工作台闭环超时".into());
            app.begin_shutdown();
        }
        if app
            .shutdown_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
            && (app.terminate_required
                || (!app.backup.irreversible() && app.backend.is_some() && !app.writer_closed)
                || !app.pool.is_finished()
                || !app.shell.is_finished()
                || !app.backup.exit_ready()
                || app.audio.as_ref().is_some_and(|audio| {
                    audio.status().map_or(true, |s| {
                        s.phase != RuntimePhase::Closed || s.physical_thread_running
                    })
                }))
        {
            eprintln!("未来教室关闭超时，停止当前进程并保留数据供启动核对。");
            std::process::exit(2);
        }
        if app.shutdown_complete() {
            stop.stop();
        }
        Ok(())
    });
    // Native pump has ended. Normal authorized exit already closed SQLite on its
    // worker. Do not join a still-running domain from this UI owner.
    if result.is_err() || !app.shutdown_complete() {
        if let Some(control) = &app.control {
            control.fault();
        }
        app.window.hide();
        module_registry::stop(module_registry::Kind::SharedWorkers, &mut app);
        eprintln!("未来教室宿主故障，终止当前进程；保留数据库供下次启动核对。");
        std::process::exit(2);
    }
    let reinitialize_requested = app.reinitialize_requested;
    module_registry::stop(module_registry::Kind::SharedWorkers, &mut app);
    if app.pool.is_finished() {
        module_registry::join_workers(app.pool)?;
    }
    result.map_err(|_| "原生宿主已因错误关闭，请核对数据后重新启动".to_owned())?;
    if let Some(error) = probe_error {
        return Err(error);
    }
    if probe
        && (stage != 90
            || lesson_stage != 9
            || copies_probe.stage != 6
            || accounts_creation != accounts_probe::CREATION_STEPS
            || accounts_management != accounts_probe::MANAGEMENT_STEPS
            || profiles_stage != profiles_probe::STEPS
            || profile_content_stage != profile_content_probe::STEPS
            || schedule_operations_stage != schedule_operations_probe::STEPS
            || !office_verified)
    {
        return Err(format!("原生工作台闭环未结束：stage={stage}"));
    }
    Ok(reinitialize_requested)
}
fn create_app(clock: Arc<CurrentClock>, probe: bool) -> std::result::Result<App, String> {
    module_registry::validate()?;
    settings_registry::validate().map_err(|_| "设置页注册不完整或重复".to_owned())?;
    let window = window_registry::create_workbench().map_err(|_| "无法创建工作台".to_owned())?;
    window
        .set_lock_action(LOCK)
        .map_err(|_| "无法注册独立锁定操作".to_owned())?;
    window
        .set_empty_form_action(voice_editor::CHECK_OUTPUT)
        .map_err(|_| "无法注册默认输出检查操作".to_owned())?;
    Ok(App {
        window,
        pool: module_registry::create_workers()?,
        clock,
        backend: None,
        control: None,
        voice_service: None,
        audio: None,
        audio_slot: Arc::new(OnceLock::new()),
        audio_suspended: Arc::new(AtomicBool::new(false)),
        next_audio_availability: Instant::now(),
        state: Arc::new(IdentityState::default()),
        lease: None,
        screen: Screen::Starting,
        status: String::new(),
        desktop_notice: String::new(),
        pending: None,
        generation: 0,
        key: None,
        record: None,
        last_input: Instant::now(),
        stopping: false,
        shutdown: None,
        writer_closed: false,
        shutdown_deadline: None,
        terminate_required: false,
        reinitialize_requested: false,
        schedule: None,
        calendar: None,
        autostart: None,
        autostart_job: None,
        autostart_command: None,
        autostart_unknown: false,
        calendar_command: None,
        calendar_unknown: false,
        calendar_command_target: None,
        autostart_scope: None,
        backup: backup_controller::State::default(),
        calendar_job: None,
        next_calendar_maintenance: Instant::now() + Duration::from_secs(30),
        lesson_editor: None,
        public_lesson_editor: None,
        public_lesson_flow: None,
        public_lesson_command: None,
        public_lesson_unknown: false,
        web_service: None,
        web_editor: None,
        https_editor: None,
        https_pending: None,
        https_unadmitted: None,
        native_command: None,
        native_unknown: false,
        account_history_note: None,
        native_target: None,
        native_mutation_pending: false,
        native_readback_pending: false,
        #[cfg(test)]
        lose_completed_history_for_test: None,
        voice_editor: None,
        web_readiness: web_editor::Readiness::Unavailable,
        teacher_https: module_registry::create_https()?,
        next_web_refresh: Instant::now() + Duration::from_secs(1),
        close_web: None,
        web_close_job: None,
        lesson_entry_pending: false,
        lesson_flow_active: false,
        offscreen_probe: probe,
        probe_trace: true,
        display: None,
        accounts: None,
        subject_editor: None,
        profiles: None,
        profile_content: None,
        profile_content_owner: None,
        preview_sequence: 0,
        preview_active: false,
        desktop: module_registry::create_desktop(probe)?,
        root: PathBuf::new(),
        material_editor: None,
        copies_loaded_sequence: 0,
        shell: module_registry::create_shell()?,
        control_slot: Arc::new(OnceLock::new()),
        launch_paths: HashMap::new(),
        cleanup_paths: VecDeque::new(),
        cleanup_jobs: Vec::new(),
        voice_maintenance: None,
        next_voice_maintenance: Instant::now() + Duration::from_secs(15),
        content_maintenance: None,
        next_content_maintenance: Instant::now() + Duration::from_secs(30),
        content_gc: Arc::new(Mutex::new(profile_content_flow::GcCursor::default())),
    })
}

/// Exercise the actual native commands and application controller using only a
/// new protected temporary deployment. This never opens the user's ProgramData.
pub fn probe() -> std::result::Result<(), String> {
    let root = std::env::temp_dir().join(format!(
        "FutureClassroom-Test-Application-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "系统时间不可用")?
            .as_nanos()
    ));
    let paths = DataPaths::isolated_test(root.clone()).map_err(|_| "隔离路径不合法")?;
    let result = run_at(paths, true, true);
    let cleanup = fs::remove_dir_all(&root).map_err(|_| "隔离工作台数据未清理".to_owned());
    result?;
    cleanup?;
    println!("native_workbench_application=PASS account_editor_stable_ID_multi_scope_reference_reject_last_admin_self_revoke_native_worker=PASS profiles_public_joint_cas_subject_exact_native_canvas=PASS profile_native_steps={} setup_login_catalog_teacher_password_homework_exit=PASS timetable_grid_dual_calendar_draft_cas=PASS display_settings_cas_draft=PASS lesson_override_restore_native_worker_cas=PASS launch_copy_management_native_worker=PASS resident_scene_offscreen=PASS unsaved_exit_guard=PASS shared_kdf_office_workers=PASS isolated_cleanup=PASS", profiles_probe::STEPS);
    println!(
        "profile_content_text_images_clear_undo_real_preview=PASS profile_content_native_steps={}",
        profile_content_probe::STEPS
    );
    println!(
        "schedule_roles_activity_split_merge_confirm_native_worker=PASS schedule_operations_native_steps={}",
        schedule_operations_probe::STEPS
    );
    Ok(())
}
#[derive(Default)]
struct CopiesProbe {
    stage: u8,
    generation: u64,
    lease: Option<SessionLease>,
    expected_sequence: u64,
    retries: u8,
}
fn copies_probe_step(app: &mut App, probe: &mut CopiesProbe) -> std::result::Result<(), String> {
    if probe.stage == 0 {
        probe.generation = app.generation;
        probe.lease = app.lease;
    } else if app.generation != probe.generation || app.lease != probe.lease {
        return Err(format!(
            "PROBE_SESSION_INTERRUPTED copies stage={} generation={}->{} lease_changed={}",
            probe.stage,
            probe.generation,
            app.generation,
            probe.lease != app.lease
        ));
    }
    eprintln!(
        "PROBE_COPIES tick={} stage={} page={} sequence={} retries={}",
        app.clock.monotonic_millis(),
        probe.stage,
        app.view().title,
        app.copies_loaded_sequence,
        probe.retries
    );
    if matches!(probe.stage, 2 | 3) {
        if app.copies_loaded_sequence == probe.expected_sequence {
            if app.screen == Screen::Materials
                && probe.retries < 3
                && matches!(
                    app.status.as_str(),
                    "后台繁忙，请稍后重试。" | "后台任务繁忙，请稍后重试。"
                )
            {
                // Only a read-only inspection is retried. A dedicated completion
                // sequence and original lease/generation remain mandatory.
                probe.retries += 1;
                submit_copies_probe(app, materials_editor::COPIES)?;
                return Ok(());
            }
            return Err(format!(
                "副本请求没有专属回执：stage={} page={} status={}",
                probe.stage,
                app.view().title,
                app.status
            ));
        }
        if app.copies_loaded_sequence != probe.expected_sequence + 1 {
            return Err("副本探针收到意外请求回执".into());
        }
    }
    let action = match probe.stage {
        0 => {
            if app.screen != Screen::Home {
                return Err("副本管理诊断未从工作台开始".into());
            }
            TO_MATERIALS
        }
        1 => {
            if app.screen != Screen::Materials {
                return Err("副本管理入口未切换资料页".into());
            }
            probe.expected_sequence = app.copies_loaded_sequence;
            materials_editor::COPIES
        }
        2 => {
            let view = app.view();
            if view.title != "打开副本管理" || !view.description.contains("200MiB") {
                return Err(format!(
                    "副本占用回执未显示：page={} status={}",
                    view.title, app.status
                ));
            }
            probe.expected_sequence = app.copies_loaded_sequence;
            materials_editor::COPIES
        }
        3 => materials_editor::RETURN_COPIES,
        4 => BACK,
        5 => {
            if app.screen != Screen::Home {
                return Err("副本管理未返回工作台".into());
            }
            eprintln!(
                "PROBE_COPIES_ACK steps=6 completed_requests=2 retryable_busy={}",
                probe.retries
            );
            probe.stage += 1;
            return Ok(());
        }
        _ => return Err("副本管理探针阶段无效".into()),
    };
    submit_copies_probe(app, action)?;
    probe.stage += 1;
    Ok(())
}
// A real user edits native fields before clicking Lock. The diagnostic must
// reproduce that order instead of smuggling an edit through the empty lock event.
fn prepare_probe_form(
    app: &App,
    action: ActionId,
    data: FormData,
) -> std::result::Result<FormData, String> {
    if action != LOCK {
        return Ok(data);
    }
    for field in app.view().fields {
        if field.kind == FieldKind::Text {
            if let Some(value) = data.get(field.id) {
                app.window
                    .probe_edit(field.id, value)
                    .map_err(|_| "隔离锁定前非秘密文本未写入".to_owned())?;
            }
        }
    }
    Ok(FormData::new())
}
fn submit_copies_probe(app: &App, action: ActionId) -> std::result::Result<(), String> {
    let mut data = FormData::new();
    for field in app.view().fields {
        if field.kind != FieldKind::ReadOnly {
            data.insert(field.id, field.value)
                .map_err(|_| "副本探针表单无效")?;
        }
    }
    let data = prepare_probe_form(app, action, data)?;
    app.window
        .probe_submit(action, data)
        .map_err(|_| "副本原生命令拒绝".into())
}
fn lesson_probe_step(app: &mut App, stage: &mut u8) -> std::result::Result<(), String> {
    use fc_domain::{schedule::*, time::*};
    fn form(app: &App) -> std::result::Result<FormData, String> {
        let mut data = FormData::new();
        for field in app.view().fields {
            if field.kind != FieldKind::ReadOnly {
                if field.kind == FieldKind::Password {
                    return Err("换课探针不接收秘密字段".into());
                }
                data.insert(field.id, field.value)
                    .map_err(|_| "换课表单无效")?;
            }
        }
        Ok(data)
    }
    match *stage {
        0 => {
            // Test-fixture setup still commits through the production worker and
            // exact administrator lease, never through a fake save callback.
            let now = fc_ports::Clock::now(app.clock.as_ref());
            if now.second_of_day() >= 86_280 {
                return Err("课程原生探针需要距离午夜至少两分钟的隔离窗口。".into());
            }
            let lease = app.lease.ok_or("换课探针无会话")?;
            let mut document = app
                .schedule
                .as_ref()
                .ok_or("换课探针课表缺失")?
                .document
                .clone();
            let plan = document.semesters.first_mut().ok_or("换课探针学期缺失")?;
            plan.semester.start = now.date;
            plan.semester.end = now.date;
            let mut monday = now.date;
            while monday.weekday() != Weekday::Monday {
                monday = (1..=31)
                    .rev()
                    .find_map(|day| {
                        LocalDate::new(monday.year(), monday.month(), day)
                            .ok()
                            .filter(|d| *d < monday)
                    })
                    .or_else(|| {
                        let (year, month) = if monday.month() == 1 {
                            (monday.year() - 1, 12)
                        } else {
                            (monday.year(), monday.month() - 1)
                        };
                        (28..=31)
                            .rev()
                            .find_map(|day| LocalDate::new(year, month, day).ok())
                    })
                    .ok_or("换课探针日期越界")?;
            }
            plan.semester.first_week_monday = monday;
            let mut table = plan.semester.tables[0].clone();
            table.role = TableRole::Single;
            table.rows.truncate(1);
            let cell = CourseCell {
                subject_id: app.state.subjects[0].id,
                teacher: TeacherBinding::Unassigned,
            };
            table.rows[0].range =
                MinuteRange::new(Minute::new(0).unwrap(), Minute::new(1439).unwrap()).unwrap();
            table.rows[0].enabled_days = [true; 7];
            table.rows[0].courses = std::array::from_fn(|_| Some(cell.clone()));
            plan.semester.tables = vec![table];
            plan.dual_history = None;
            // The earlier grid probe created names for both week tables. This
            // fixture keeps one table, so discard metadata for the retired rows.
            plan.row_details.clear();
            plan.cell_details.clear();
            document.school_calendar.entries.clear();
            document.school_calendar.school_coverage = Some((now.date, now.date));
            document.validate_draft(&app.state).map_err(|error| {
                format!("换课探针课表准备无效：{}", schedule_editor::problem(error))
            })?;
            let backend = app.backend.as_ref().ok_or("换课探针后台缺失")?.clone();
            app.submit(Work::SaveSchedule(
                backend,
                lease,
                document.revision,
                document,
            ))?;
        }
        1 => {
            let now = fc_ports::Clock::now(app.clock.as_ref());
            let document = &app
                .schedule
                .as_ref()
                .ok_or("换课探针保存结果缺失")?
                .document;
            let resolved = document
                .resolve_day(
                    app.state
                        .installation
                        .as_ref()
                        .and_then(|i| i.current_class)
                        .ok_or("换课探针班级缺失")?,
                    now.date,
                )
                .map_err(|error| {
                    format!("换课探针课表读回无效：{}", schedule_editor::problem(error))
                })?;
            if !matches!(
                resolve_lesson(&resolved, now, document.automatic_gaps),
                LessonState::Course {
                    cell: CourseCell {
                        teacher: TeacherBinding::Unassigned,
                        ..
                    },
                    ..
                }
            ) {
                return Err(format!("换课探针课表未成功持久保存：{}", app.status));
            }
            app.open_lesson_editor()?;
        }
        2 => {
            let editor = app.lesson_editor.as_ref().ok_or("换课原生页面未加载")?;
            if editor
                .snapshot
                .current
                .as_ref()
                .is_none_or(|c| c.teacher_id.is_some())
                || editor.snapshot.active_override.is_some()
            {
                return Err(format!(
                    "当前课程捕获失败：status={}；snapshot={:?}",
                    app.status, editor.snapshot
                ));
            }
            let mut data = form(app)?;
            data.take(lesson_editor::TEACHER);
            data.insert(
                lesson_editor::TEACHER,
                app.state
                    .account_by_login("probe-teacher")
                    .map_err(|_| "探针教师缺失")?
                    .principal
                    .id
                    .get()
                    .to_string(),
            )
            .map_err(|_| "换课教师字段无效")?;
            app.window
                .probe_submit(lesson_editor::REPLACE, data)
                .map_err(|_| "换课原生命令拒绝")?;
        }
        3 => {
            let snapshot = &app
                .lesson_editor
                .as_ref()
                .ok_or("换课结果页面缺失")?
                .snapshot;
            if snapshot.active_override.is_none()
                || snapshot
                    .current
                    .as_ref()
                    .is_none_or(|c| c.teacher_id.is_none() || c.effective == c.base)
            {
                return Err(format!("真实换课未提交：{}", app.status));
            }
            let current = snapshot.current.as_ref().ok_or("换课当前实例缺失")?;
            let stale = snapshot
                .revision
                .get()
                .checked_sub(1)
                .ok_or("换课修订无效")?;
            app.submit(Work::ChangeLesson(
                app.backend.as_ref().unwrap().clone(),
                app.lease.unwrap(),
                Revision::new(stale),
                current.key.clone(),
                fc_domain::lesson_context::LessonAction::Restore,
            ))?;
        }
        4 => {
            if !app.status.contains("临时安排已被修改")
                || !app.native_unknown
                || app
                    .lesson_editor
                    .as_ref()
                    .unwrap()
                    .snapshot
                    .active_override
                    .is_none()
            {
                return Err("过期换课修订未保留原页面或未拒绝".into());
            }
            // Phase 3 is a definite precommit Lesson Conflict. Explicitly read
            // the current lesson before deciding a new restore intent. This is
            // not an original command receipt query and never replays that
            // rejected stale command or clears the production guard directly.
            app.window
                .probe_submit(lesson_editor::RELOAD, form(app)?)
                .map_err(|_| "冲突后本节重读拒绝")?;
        }
        5 => {
            let snapshot = &app
                .lesson_editor
                .as_ref()
                .ok_or("冲突后本节重读结果缺失")?
                .snapshot;
            if app.screen != Screen::Lesson
                || app.native_unknown
                || snapshot.active_override.is_none()
                || snapshot
                    .current
                    .as_ref()
                    .is_none_or(|c| c.teacher_id.is_none() || c.effective == c.base)
            {
                return Err(format!("冲突后未明确读回当前临时安排：{}", app.status));
            }
            // Confirm a new restore from the freshly read revision. The failed
            // original command remains failed; this is a new native C2 intent.
            app.window
                .probe_submit(lesson_editor::RESTORE, form(app)?)
                .map_err(|_| "恢复原课命令拒绝")?;
        }
        6 => {
            let snapshot = &app
                .lesson_editor
                .as_ref()
                .ok_or("恢复原课结果缺失")?
                .snapshot;
            if snapshot.active_override.is_some()
                || snapshot
                    .current
                    .as_ref()
                    .is_none_or(|c| c.base != c.effective || c.teacher_id.is_some())
            {
                return Err(format!("恢复原课未生效：{}", app.status));
            }
            app.window
                .probe_submit(lesson_editor::RELOAD, form(app)?)
                .map_err(|_| "本节重读拒绝")?;
        }
        7 => {
            if app
                .lesson_editor
                .as_ref()
                .is_none_or(|e| e.snapshot.active_override.is_some())
            {
                return Err("恢复原课持久化重读失败".into());
            }
            app.window
                .probe_submit(BACK, form(app)?)
                .map_err(|_| "换课返回拒绝")?;
        }
        8 => {
            if app.screen != Screen::Home {
                return Err("换课未返回主管理工作台".into());
            }
        }
        _ => return Err("换课探针阶段无效".into()),
    }
    *stage += 1;
    Ok(())
}
fn probe_step(app: &mut App, stage: &mut u8) -> std::result::Result<(), String> {
    if *stage == 6 {
        if app.screen == Screen::Home {
            *stage += 1;
            return Ok(());
        }
        let view = app.view();
        let action = if app.state.subjects.iter().any(|s| s.name == "数学")
            && view
                .actions
                .iter()
                .any(|a| a.id == settings_registry::SUBJECT_NEW)
        {
            BACK
        } else if view
            .actions
            .iter()
            .any(|a| a.id == settings_registry::SUBJECT_NEW)
        {
            settings_registry::SUBJECT_NEW
        } else if view
            .actions
            .iter()
            .any(|a| a.id == settings_registry::SUBJECT_REVIEW)
        {
            settings_registry::SUBJECT_REVIEW
        } else {
            settings_registry::SUBJECT_CONFIRM
        };
        let mut form = FormData::new();
        for field in view
            .fields
            .into_iter()
            .filter(|f| f.kind != FieldKind::ReadOnly)
        {
            let value = if field.id == SUBJECT {
                "数学".into()
            } else if field.id == COLOR {
                "70A5D8".into()
            } else {
                field.value
            };
            form.insert(field.id, value)
                .map_err(|_| "科目原生诊断表单无效")?;
        }
        app.window
            .probe_submit(action, form)
            .map_err(|_| "科目原生诊断操作未接纳")?;
        return Ok(());
    }
    const INITIAL: &str = "Probe-admin-password-2026";
    const TEACHER: &str = "Probe-teacher-initial-2026";
    const CHANGED: &str = "Probe-teacher-changed-2026";
    let action;
    let mut data = FormData::new();
    let mut put = |id, value: &str| {
        data.insert(id, value.to_owned())
            .map_err(|_| "隔离表单无效".to_owned())
    };
    let expected = match *stage {
        0 => {
            action = SAVE;
            put(SCHOOL, "隔离验证学校")?;
            put(LOGIN, "probe-admin")?;
            put(NAME, "验证主管理员")?;
            put(PASSWORD, INITIAL)?;
            put(CONFIRM, INITIAL)?;
            Screen::Setup
        }
        1 => {
            action = ACK;
            Screen::Code
        }
        2 => {
            action = LOG_IN;
            put(LOGIN, "probe-admin")?;
            put(PASSWORD, INITIAL)?;
            Screen::Login
        }
        3 => {
            action = TO_SCHOOL;
            Screen::Home
        }
        4 => {
            action = ADD_CLASS;
            put(SCHOOL, "隔离验证学校")?;
            put(CLASS, "验证班")?;
            Screen::School
        }
        5 => {
            action = TO_SUBJECTS;
            Screen::Home
        }
        7 => {
            action = TO_SCHOOL;
            Screen::Home
        }
        8 => {
            action = BIND_CLASS;
            put(SCHOOL, "隔离验证学校")?;
            put(CLASS, "验证班")?;
            Screen::School
        }
        9 => {
            action = TO_ACCOUNTS;
            Screen::Home
        }
        10 => return Err("账号创建必须通过新原生编辑器流程".into()),
        11 => {
            action = ACK;
            Screen::Code
        }
        12 => {
            action = TO_HOMEWORK;
            Screen::Home
        }
        13 => {
            action = LOAD;
            put(CLASS, "验证班")?;
            put(SUBJECT, "数学")?;
            put(OWNER, "probe-teacher")?;
            Screen::Homework
        }
        14 => {
            action = SAVE;
            put(CONTENT, "验证作业：完成今天的数学练习。")?;
            Screen::HomeworkEdit
        }
        15 => {
            action = LOCK;
            Screen::Home
        }
        16 => {
            action = LOG_IN;
            put(LOGIN, "probe-teacher")?;
            put(PASSWORD, TEACHER)?;
            Screen::Login
        }
        17 => {
            action = SAVE;
            put(PASSWORD, TEACHER)?;
            put(NEW_PASSWORD, CHANGED)?;
            put(CONFIRM, CHANGED)?;
            Screen::Password
        }
        18 => {
            action = LOG_IN;
            put(LOGIN, "probe-teacher")?;
            put(PASSWORD, CHANGED)?;
            Screen::Login
        }
        19 => {
            if app.view().actions.iter().any(|a| {
                matches!(
                    a.id,
                    TO_SCHOOL | TO_SUBJECTS | TO_ACCOUNTS | TO_SCHEDULE | TO_DISPLAY | EXIT
                )
            }) {
                return Err("教师工作台暴露主管理员入口".into());
            }
            action = TO_HOMEWORK;
            Screen::Home
        }
        20 => {
            action = LOAD;
            put(CLASS, "验证班")?;
            put(SUBJECT, "数学")?;
            put(OWNER, "probe-teacher")?;
            Screen::Homework
        }
        21 => {
            if app.record.as_ref().map(|r| r.text.as_str())
                != Some("验证作业：完成今天的数学练习。")
            {
                return Err("真实作业事务读回不一致".into());
            }
            put(CONTENT, "验证作业：完成今天的数学练习。")?;
            action = BACK;
            Screen::HomeworkEdit
        }
        22 => {
            put(CLASS, "验证班")?;
            put(SUBJECT, "数学")?;
            put(OWNER, "probe-teacher")?;
            action = BACK;
            Screen::Homework
        }
        23 => {
            action = LOCK;
            Screen::Home
        }
        24 => {
            action = LOG_IN;
            put(LOGIN, "probe-admin")?;
            put(PASSWORD, INITIAL)?;
            Screen::Login
        }
        25 => {
            action = TO_SCHEDULE;
            Screen::Home
        }
        26..=52 | 54..=58 => {
            return schedule_probe_step(app, stage);
        }
        53 => {
            action = TO_SCHEDULE;
            Screen::Home
        }
        59 => {
            *stage += 1;
            return Ok(());
        }
        60 => {
            action = LOG_IN;
            put(LOGIN, "probe-admin")?;
            put(PASSWORD, INITIAL)?;
            Screen::Login
        }
        61 => {
            action = TO_SCHEDULE;
            Screen::Home
        }
        62 => {
            use schedule_editor::*;
            if app
                .view()
                .fields
                .iter()
                .find(|f| f.id == START)
                .map(|f| f.value.as_str())
                != Some("2026-")
            {
                return Err("锁定后未保留未完成日期草稿".into());
            }
            action = EDIT_BACK;
            Screen::Schedule
        }
        63 => {
            action = schedule_editor::EDIT_BACK;
            Screen::Schedule
        }
        64 => {
            action = schedule_editor::EDIT_BACK;
            Screen::Schedule
        }
        65 => {
            action = TO_DISPLAY;
            Screen::Home
        }
        66..=75 | 77 | 80..=82 => return display_probe_step(app, stage),
        76 => {
            action = TO_DISPLAY;
            Screen::Home
        }
        78 => {
            action = LOG_IN;
            put(LOGIN, "probe-admin")?;
            put(PASSWORD, INITIAL)?;
            Screen::Login
        }
        79 => {
            action = TO_DISPLAY;
            Screen::Home
        }
        83 => {
            if !app.desktop.probe_ready() {
                return Ok(());
            }
            let scene = app.desktop.probe_scene().ok_or("常驻桌面没有生成场景")?;
            let metrics = app.desktop.probe_metrics();
            let display = app.display.as_ref().ok_or("显示草稿缺失")?;
            let tree = scene.paint_tree.as_ref().ok_or("常驻桌面没有发布布局树")?;
            // The profile flow first committed display 3 / layout 1; the
            // content flow then jointly saved its nodes and restored the same
            // public semantics in two more layout commits. An unchanged display
            // keeps its revision 3; the restored layout advances to revision 3.
            if display.settings.revision!=Revision::new(3)
                || display.settings.glass_surface_percent != 7
                || tree.public_layout_revision != Revision::new(3)
                || !scene.cards.iter().any(|c| matches!(&c.content,fc_presentation::scene::CardContent::Homework(rows) if rows.iter().any(|r|r.text=="验证作业：完成今天的数学练习。")))
                || metrics.drawn_frames==0 || metrics.visible_windows!=0 || metrics.input_regions!=0 {
                return Err(format!(
                    "真实事实至隐藏原生场景/持久显示修订核对失败：display={} layout={} glass={} frames={} visible={} input={}",
                    display.settings.revision.get(), tree.public_layout_revision.get(),
                    display.settings.glass_surface_percent, metrics.drawn_frames,
                    metrics.visible_windows, metrics.input_regions,
                ));
            }
            action = TO_DISPLAY;
            Screen::Home
        }
        84 => {
            action = display_editor::EDIT_BACK;
            put(display_editor::LABEL, "退出保护验证草稿")?;
            Screen::Display
        }
        85 | 88 => {
            action = EXIT;
            Screen::Home
        }
        86 => {
            action = RETURN_DRAFT;
            Screen::ExitDraft
        }
        87 => {
            action = display_editor::EDIT_BACK;
            Screen::Display
        }
        89 => {
            action = DISCARD_EXIT;
            Screen::ExitDraft
        }
        _ => return Ok(()),
    };
    if app.screen != expected {
        return Err(format!(
            "原生工作台页面转换失败：stage={stage}，{}",
            app.status
        ));
    }
    for f in app.view().fields {
        if f.kind != FieldKind::ReadOnly && data.get(f.id).is_none() {
            data.insert(f.id, f.value)
                .map_err(|_| "原生诊断表单重复字段")?;
        }
    }
    let data = prepare_probe_form(app, action, data)?;
    app.window
        .probe_submit(action, data)
        .map_err(|_| format!("原生工作台命令未接纳：stage={stage}"))?;
    *stage += 1;
    Ok(())
}

fn display_probe_step(app: &mut App, stage: &mut u8) -> std::result::Result<(), String> {
    use display_editor::*;
    if app.screen != Screen::Display {
        return Err(format!("显示设置页面错误：stage={stage}，{}", app.status));
    }
    let mut data = FormData::new();
    let mut put = |id, value: &str| {
        data.insert(id, value.into())
            .map_err(|_| "显示诊断表单无效")
    };
    let action = match *stage {
        66 => {
            put(CLOCK, "12")?;
            put(SECONDS, "1")?;
            put(TARGET, "2026-12-31")?;
            put(LABEL, "验证目标")?;
            SAVE_ALL
        }
        67 => COMPONENTS,
        68 => {
            put(ALPHA, "0")?;
            put(FONT, "32")?;
            APPLY
        }
        69 => {
            put(COMPONENT, "3")?;
            SWITCH_COMPONENT
        }
        70 => {
            put(ALPHA, "88")?;
            put(FONT, "24")?;
            APPLY
        }
        71 => EDIT_BACK,
        72 => SAVE_ALL,
        73 => CANCEL,
        74 | 81 => CONFIRM_CANCEL,
        82 => EDIT_BACK,
        75 => {
            let s = &app.display.as_ref().ok_or("显示草稿缺失")?.settings;
            if s.revision != Revision::new(2)
                || s.clock_mode != fc_domain::desktop::ClockMode::Digital12
                || !s.clock_seconds
                || s.component(fc_domain::desktop::ComponentKind::Clock)
                    .is_none_or(|c| c.opacity_percent != 0 || c.font_dip != 32)
                || s.component(fc_domain::desktop::ComponentKind::Homework)
                    .is_none_or(|c| c.opacity_percent != 88 || c.font_dip != 24)
            {
                return Err("显示设置CAS保存并重读不一致".into());
            }
            EDIT_BACK
        }
        77 => {
            put(TARGET, "2026-")?;
            put(LABEL, "未保存倒计时")?;
            LOCK
        }
        80 => {
            if app
                .view()
                .fields
                .iter()
                .find(|f| f.id == TARGET)
                .map(|f| f.value.as_str())
                != Some("2026-")
            {
                return Err("显示设置锁定后半日期丢失".into());
            }
            CANCEL
        }
        _ => return Err("显示诊断阶段无效".into()),
    };
    for f in app.view().fields {
        if f.kind != FieldKind::ReadOnly && data.get(f.id).is_none() {
            data.insert(f.id, f.value).map_err(|_| "显示诊断字段重复")?;
        }
    }
    let data = prepare_probe_form(app, action, data)?;
    app.window
        .probe_submit(action, data)
        .map_err(|_| format!("原生显示设置命令拒绝：stage={stage}"))?;
    *stage += 1;
    Ok(())
}

fn schedule_probe_step(app: &mut App, stage: &mut u8) -> std::result::Result<(), String> {
    use schedule_editor::*;
    if app.screen != Screen::Schedule {
        return Err(format!("课表页面未打开：stage={stage}，{}", app.status));
    }
    let mut data = FormData::new();
    let mut put = |id, value: &str| {
        data.insert(id, value.into())
            .map_err(|_| "课表诊断字段重复".to_owned())
    };
    let action = match *stage {
        26 => NEW_TERM,
        27 => {
            put(TERM_NAME, "验证学期")?;
            put(START, "2026-09-01")?;
            put(END, "2027-01-31")?;
            put(ANCHOR, "2026-08-31")?;
            APPLY
        }
        28 => ADD_ROW,
        29 => {
            put(ROW_NAME, "第一节")?;
            put(RANGE, "08200900")?;
            APPLY
        }
        30 => {
            app.window
                .probe_select_grid(0, 1)
                .map_err(|_| "原生网格选格失败")?;
            *stage += 1;
            return Ok(());
        }
        31 => {
            let subject = app
                .state
                .subjects
                .iter()
                .find(|s| s.name == "数学")
                .ok_or("验证科目缺失")?
                .id;
            app.window
                .probe_choice(SUBJECT, &subject.get().to_string())
                .map_err(|_| "原生科目选择失败")?;
            *stage += 1;
            return Ok(());
        }
        32 => {
            let teacher = app
                .state
                .account_by_login("probe-teacher")
                .map_err(|_| "验证教师缺失")?
                .principal
                .id;
            if app
                .view()
                .fields
                .iter()
                .find(|f| f.id == TEACHER)
                .map(|f| f.value.as_str())
                != Some(teacher.get().to_string().as_str())
            {
                return Err("唯一任课教师没有自动选择".into());
            }
            put(ROOM, "验证教室")?;
            put(NOTE, "备注随课程清空")?;
            APPLY
        }
        33 => SAVE_ALL,
        34 => ENABLE_DUAL,
        35 => {
            put(TABLE, "1")?;
            OPEN
        }
        36 => {
            app.window
                .probe_select_grid(0, 1)
                .map_err(|_| "原生双表选格失败")?;
            *stage += 1;
            return Ok(());
        }
        37 => {
            put(SUBJECT, "none")?;
            put(TEACHER, "unassigned")?;
            APPLY
        }
        38 => SAVE_ALL,
        39 => DISABLE_DUAL,
        40 => SAVE_ALL,
        41 => RESTORE_DUAL,
        42 => SAVE_ALL,
        43 => {
            put(TABLE, "0")?;
            OPEN
        }
        44 => PREVIEW,
        45 | 46 => EDIT_BACK,
        47 => CALENDAR,
        48 => {
            put(MODE, "school")?;
            put(START, "2026-09-01")?;
            put(END, "2027-01-31")?;
            APPLY
        }
        49 => {
            put(MAP_DATE, "2026-11-02")?;
            put(MAP_KIND, "rest")?;
            SET_MAPPING
        }
        50 => EDIT_BACK,
        51 => SAVE_ALL,
        52 => EDIT_BACK,
        53 => TO_SCHEDULE,
        54 => CANCEL,
        55 => CONFIRM_CANCEL,
        56 => OPEN,
        57 => {
            let doc = &app.schedule.as_ref().ok_or("课表草稿缺失")?.document;
            let p = doc.semesters.first().ok_or("持久化学期丢失")?;
            if doc.revision != Revision::new(5)
                || p.semester.tables.len() != 2
                || p.semester.tables[0].rows[0].courses[0].is_none()
                || p.semester.tables[1].rows[0].courses[0].is_some()
                || doc.calendar_mode != fc_domain::schedule::CalendarMode::School
                || doc.school_calendar.entries.len() != 1
                || !doc.public_calendar.entries.is_empty()
            {
                return Err("课表/双表差异/独立校历重读不一致".into());
            }
            HEADER
        }
        58 => {
            put(START, "2026-")?;
            put(TERM_NAME, "未保存名称")?;
            LOCK
        }
        59 => {
            *stage += 1;
            return Ok(());
        }
        _ => return Err("课表诊断阶段无效".into()),
    };
    // Stage52 leaves the schedule screen; the next entry is handled by the
    // normal workbench action path and then resumes this same stored draft.
    for f in app.view().fields {
        if f.kind != FieldKind::ReadOnly && data.get(f.id).is_none() {
            data.insert(f.id, f.value).map_err(|_| "诊断表单无效")?;
        }
    }
    let data = prepare_probe_form(app, action, data)?;
    app.window
        .probe_submit(action, data)
        .map_err(|_| format!("原生课表命令拒绝：stage={stage}"))?;
    *stage += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) static SERIAL: Mutex<()> = Mutex::new(());
    pub(super) struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let result = fs::remove_dir_all(&self.0);
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
    #[test]
    fn existing_only_open_never_initializes_empty_or_incomplete_deployment() {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-ExistingOnly-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = DataPaths::isolated_test(root.clone()).unwrap();
        paths.prepare_for_current_user().unwrap();
        let _scratch = Scratch(root.clone());
        for marker in [false, true] {
            if marker {
                create_file(&root.join("config/deployment.marker"), MANIFEST).unwrap();
            }
            assert!(open_backend(
                &root,
                Arc::new(CurrentClock::new()),
                OpenPolicy::ExistingOnly
            )
            .is_err());
            assert!(!root.join("config/credential-key.dpapi").exists());
            assert!(!paths.database().exists());
            assert_eq!(root.join("config/deployment.marker").exists(), marker);
        }
        fs::remove_file(root.join("config/deployment.marker")).unwrap();
        let clock = Arc::new(CurrentClock::new());
        let mut first =
            open_backend(&root, clock.clone(), OpenPolicy::AllowInitialization).unwrap();
        first.close().unwrap();
        let encrypted = fs::read(root.join("config/credential-key.dpapi")).unwrap();
        let mut reopened = open_backend(&root, clock, OpenPolicy::ExistingOnly).unwrap();
        assert!(reopened.state().unwrap().installation.is_none());
        reopened.close().unwrap();
        assert_eq!(
            fs::read(root.join("config/credential-key.dpapi")).unwrap(),
            encrypted
        );
    }
    pub(super) fn fixture() -> (Scratch, App) {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Lifecycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = DataPaths::isolated_test(root.clone()).unwrap();
        paths.prepare_for_current_user().unwrap();
        let mut app = create_app(Arc::new(CurrentClock::new()), true).unwrap();
        app.root = root.clone();
        app.desktop.set_root(root.clone());
        app.render().unwrap();
        app.submit(Work::Open(paths, app.clock.clone())).unwrap();
        (Scratch(root), app)
    }
    pub(super) fn wait_for(app: &mut App, condition: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !condition(app) {
            app.poll().unwrap();
            assert!(Instant::now() < deadline, "native lifecycle did not drain");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pub(super) fn drain(mut app: App) {
        app.begin_shutdown();
        wait_for(&mut app, |a| a.pool.is_finished());
        app.pool.join().unwrap();
    }
    pub(super) fn ready_management() -> (Scratch, App) {
        let (scratch, mut app) = fixture();
        wait_for(&mut app, |app| app.pending.is_none());
        let until = Instant::now() + Duration::from_secs(90);
        app.next_content_maintenance = until;
        app.next_voice_maintenance = until;
        app.next_calendar_maintenance = until;
        voice_management_probe::submit(
            &mut app,
            SAVE,
            &[
                (SCHOOL, "Synthetic typed history"),
                (LOGIN, voice_management_probe::LOGIN_NAME),
                (NAME, "Synthetic administrator"),
                (PASSWORD, voice_management_probe::PASSWORD_VALUE),
                (CONFIRM, voice_management_probe::PASSWORD_VALUE),
            ],
            until,
        )
        .unwrap();
        voice_management_probe::submit(&mut app, ACK, &[], until).unwrap();
        if app.screen == Screen::Login {
            voice_management_probe::login(&mut app, until).unwrap();
        }
        (scratch, app)
    }
    /// Drop only the verified successful metadata completion at the UI delivery
    /// boundary. Keep the original JobTicket for the normal pump to consume;
    /// reading it here would manufacture Consumed/Stopped and revoke the lease.
    pub(super) fn lose_worker_reply(app: &mut App) {
        let lease = app.lease.expect("original management lease");
        let original = app.native_command.as_ref().unwrap().clone();
        assert!(app.lose_completed_history_for_test.is_none());
        let until = Instant::now() + Duration::from_secs(30);
        while !app.pending.as_ref().unwrap().ticket.is_finished() {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        app.lose_completed_history_for_test = Some(original.clone());
        wait_for(app, |app| app.pending.is_none());
        assert!(app.lose_completed_history_for_test.is_none());
        assert!(app.native_unknown);
        assert_eq!(app.lease, Some(lease), "completion loss is not worker loss");
        assert!(app.native_command.as_ref() == Some(&original));
        app.backend
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .check_session(lease, SessionPurpose::Manage)
            .expect("original lease remains valid without reauthentication");
    }
    pub(super) fn drop_completed_history_for_test(
        app: &mut App,
        completed: Option<Result<Outcome>>,
    ) -> Option<Result<Outcome>> {
        if app.lose_completed_history_for_test.is_none() {
            return completed;
        }
        let outcome = match completed {
            None => return None,
            Some(Ok(outcome)) => outcome,
            Some(Err(error)) => panic!("expected actual successful history: {error:?}"),
        };
        let original = app.lose_completed_history_for_test.take().unwrap();
        assert!(app.native_mutation_pending);
        assert!(app.native_command.as_ref() == Some(&original));
        if let Outcome::MaterialBatch(result) = &outcome {
            assert_eq!(result.command, original);
            assert_eq!(result.issued, original.snapshot());
            assert!(matches!(
                result.outcome,
                material_flow::BatchOutcome::Saved(_)
            ));
        } else {
            let history = outcome
                .native_history()
                .expect("actual typed worker history");
            assert!(history.matches(&original));
            assert!(history.valid_result());
            assert!(matches!(
                history.resolution,
                NativeResolution::Outcome(
                    fc_ports::command_window::TypedCommandOutcome::Completed(_)
                ) | NativeResolution::DefaultTeacher(Some(_))
                    | NativeResolution::Subject(Some(_))
            ));
        }
        // Historical outputs contain metadata only; physical owners
        // remain with their original executor. No fake historical result is made.
        drop(outcome);
        Some(Err(Failure::LostCompletedHistoryForTest))
    }
    #[test]
    fn management_unknown_never_blocks_public_lesson_and_public_read_cannot_clear_it() {
        use fc_domain::{
            lesson_context::LessonAction, schedule::*, schedule_document::SemesterPlan, time::*,
        };
        use fc_ports::{Clock, DesktopLessonBinding};
        struct FixedCivil;
        impl Clock for FixedCivil {
            fn now(&self) -> LocalDateTime {
                LocalDateTime::new(LocalDate::new(2026, 11, 2).unwrap(), 10 * 3600 + 1800).unwrap()
            }
        }
        let _serial = SERIAL.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Public-Unknown-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let scratch = Scratch(root.clone());
        let paths = DataPaths::isolated_test(root.clone()).unwrap();
        paths
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        let clock = Arc::new(CurrentClock::new());
        let mut backend = Backend::open(
            paths.database(),
            CredentialRecordCodec::generate(&mut SystemRandom).unwrap(),
            Arc::new(FixedCivil),
            clock.clone(),
        )
        .unwrap();
        let issued = backend
            .issue_native_command(
                fc_ports::command_window::CommandFamily::Identity,
                None,
                SessionPurpose::Manage,
            )
            .unwrap();
        drop(
            backend
                .begin_command_scope(issued)
                .unwrap()
                .initialize(
                    Initialize {
                        school_name: "Synthetic public unknown".into(),
                        login_name: "public-unknown".into(),
                        display_name: "Synthetic administrator".into(),
                    },
                    Password::new("Synthetic public unknown 2026!".into()).unwrap(),
                )
                .unwrap(),
        );
        let lease = backend
            .login(
                "public-unknown",
                Password::new("Synthetic public unknown 2026!".into()).unwrap(),
            )
            .unwrap();
        let class = ClassId::new(1).unwrap();
        let subject = SubjectId::new(1).unwrap();
        for action in [
            IdentityCommand::CreateClass {
                id: class,
                name: "Synthetic class".into(),
            },
            IdentityCommand::BindCurrentClass { id: class },
            IdentityCommand::PutSubject {
                id: subject,
                name: "Synthetic subject".into(),
                enabled: true,
                color: Rgb {
                    red: 10,
                    green: 20,
                    blue: 30,
                },
                expected_revision: None,
            },
            IdentityCommand::UpdateAccount {
                id: lease.principal_id(),
                display_name: "Synthetic administrator".into(),
                role: Role::GlobalAdmin,
                enabled: true,
                scopes: vec![AssignmentScope {
                    class_id: class,
                    subject_id: subject,
                }],
            },
        ] {
            let issued = backend
                .issue_native_command(
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(lease),
                    SessionPurpose::Manage,
                )
                .unwrap();
            backend
                .begin_command_scope(issued)
                .unwrap()
                .identity(lease, action)
                .unwrap();
        }
        let lease = backend
            .login(
                "public-unknown",
                Password::new("Synthetic public unknown 2026!".into()).unwrap(),
            )
            .unwrap();
        let cell = CourseCell {
            subject_id: subject,
            teacher: TeacherBinding::Explicit(lease.principal_id()),
        };
        let document = ScheduleDocument {
            semesters: vec![SemesterPlan::new(
                Semester {
                    id: SemesterId::new(1).unwrap(),
                    class_id: class,
                    revision: Revision::default(),
                    enabled: true,
                    start: LocalDate::new(2026, 11, 1).unwrap(),
                    end: LocalDate::new(2026, 11, 30).unwrap(),
                    first_week_monday: LocalDate::new(2026, 10, 26).unwrap(),
                    tables: vec![WeekTable {
                        id: TableId::new(1).unwrap(),
                        role: TableRole::Single,
                        rows: vec![TimetableRow {
                            id: RowId::new(1).unwrap(),
                            logical_key: LogicalRowKey::new(1).unwrap(),
                            group: DayGroup::Morning,
                            kind: RowKind::Course,
                            range: MinuteRange::new(
                                Minute::new(600).unwrap(),
                                Minute::new(660).unwrap(),
                            )
                            .unwrap(),
                            enabled_days: [true; 7],
                            courses: std::array::from_fn(|_| Some(cell.clone())),
                        }],
                    }],
                },
                "Synthetic public lesson".into(),
            )],
            ..ScheduleDocument::default()
        };
        let mut app = create_app(clock, true).unwrap();
        app.root = root.clone();
        app.desktop.set_root(root);
        let control = backend.session_control();
        let _ = app.control_slot.set(control.clone());
        app.control = Some(control.clone());
        app.state = Arc::new(backend.state().unwrap());
        app.lease = Some(lease);
        app.screen = Screen::Home;
        let backend = Arc::new(Mutex::new(backend));
        app.backend = Some(backend.clone());
        let until = Instant::now() + Duration::from_secs(90);
        app.next_calendar_maintenance = until;
        app.next_content_maintenance = until;
        app.next_voice_maintenance = until;
        // Execute a real manager write, then deliberately lose only its UI handoff.
        app.submit(Work::SaveSchedule(
            backend.clone(),
            lease,
            Revision::default(),
            document,
        ))
        .unwrap();
        let manager = app.native_command.clone().unwrap();
        let pending = app.pending.take().unwrap();
        assert!(matches!(
            pending.ticket.wait().unwrap().unwrap(),
            Outcome::ScheduleSaved(_)
        ));
        assert!(manager.snapshot().is_some());
        app.native_mutation_pending = false;
        app.native_readback_pending = false;
        app.native_unknown = true;
        control.lock_local();
        app.lease = None;
        app.screen = Screen::Login;
        let open = |app: &mut App, generation| {
            let facts = backend.lock().unwrap().desktop_facts(0).unwrap();
            let current = facts.lesson_context.current.as_ref().unwrap();
            let stamp = fc_bootstrap::backend::DesktopLessonStamp {
                generation: SceneGeneration::new(generation),
                source_revision: facts.global_revision,
                binding: DesktopLessonBinding {
                    expected_revision: facts.lesson_context.revision,
                    expected_identity_revision: facts.identity.revision,
                    expected_schedule_revision: facts.schedule.revision,
                    lesson: current.key.clone(),
                },
                captured_at: facts.captured_at,
                valid_until: current.effective_until,
            };
            assert!(control.publish_desktop_lesson_scene(stamp.clone()));
            app.open_public_lesson(stamp).unwrap();
            wait_for(app, |app| app.pending.is_none());
            assert!(
                app.public_lesson_editor.is_some(),
                "actual public read needs no account lease"
            );
        };
        open(&mut app, 100);
        assert!(app.native_unknown);
        assert_eq!(app.native_command.as_ref(), Some(&manager));
        let guard = app.public_lesson_flow.as_ref().unwrap().guard();
        app.submit(Work::ChangePublicLesson(
            backend.clone(),
            guard,
            LessonAction::Replace(cell),
        ))
        .unwrap();
        assert!(
            app.pending.is_some(),
            "manager unknown cannot block public change admission"
        );
        wait_for(&mut app, |app| app.pending.is_none());
        assert!(backend
            .lock()
            .unwrap()
            .desktop_facts(0)
            .unwrap()
            .lesson_context
            .active_override
            .is_some());
        assert!(app.native_unknown);
        assert_eq!(app.native_command.as_ref(), Some(&manager));
        assert!(!app.public_lesson_unknown);
        open(&mut app, 101);
        let guard = app.public_lesson_flow.as_ref().unwrap().guard();
        app.submit(Work::ChangePublicLesson(
            backend.clone(),
            guard.clone(),
            LessonAction::Restore,
        ))
        .unwrap();
        let public = app.public_lesson_command.clone().unwrap();
        let pending = app.pending.take().unwrap();
        assert!(matches!(
            pending.ticket.wait().unwrap().unwrap(),
            Outcome::PublicLessonSaved(_)
        ));
        app.native_mutation_pending = false;
        app.native_readback_pending = false;
        app.public_lesson_unknown = true;
        let original = public.snapshot();
        assert!(original.is_some());
        app.submit(Work::ChangePublicLesson(
            backend.clone(),
            guard,
            LessonAction::Restore,
        ))
        .unwrap();
        assert!(
            app.pending.is_none(),
            "public unknown cannot replay its old body"
        );
        assert_eq!(public.snapshot(), original);
        assert!(app.native_unknown);
        app.close_public_lesson();
        open(&mut app, 102);
        assert!(
            !app.public_lesson_unknown,
            "a new exact public scene read reconciles only that chain"
        );
        assert!(app.native_unknown);
        assert_eq!(app.native_command.as_ref(), Some(&manager));
        let flow = app.public_lesson_flow.as_ref().unwrap();
        let guard = flow.guard();
        flow.cancel();
        app.submit(Work::ChangePublicLesson(
            backend.clone(),
            guard,
            LessonAction::Restore,
        ))
        .unwrap();
        wait_for(&mut app, |app| app.pending.is_none());
        assert!(
            backend
                .lock()
                .unwrap()
                .desktop_facts(0)
                .unwrap()
                .lesson_context
                .active_override
                .is_none(),
            "original cancelled scene still rejects mutation"
        );
        assert!(app.native_unknown);
        assert_eq!(app.native_command.as_ref(), Some(&manager));
        drain(app);
        drop(backend);
        drop(scratch);
    }
    #[test]
    fn unrelated_successful_read_cannot_retire_the_original_unknown_target() {
        let _serial = SERIAL.lock().unwrap();
        let (_scratch, mut app) = ready_management();
        let lease = app.lease.unwrap();
        let backend = app.backend.as_ref().unwrap().clone();
        let year = app.clock.try_now().unwrap().date.year();
        let before = backend
            .lock()
            .unwrap()
            .calendar_status(lease, year)
            .unwrap();
        // An empty NativeCommand only exercises the legacy current-read
        // fallback. This case must retain a real typed intent and issued pair.
        app.submit_calendar(calendar_flow::Operation::Policy {
            year,
            expected: before.policy.revision,
            enabled: !before.policy.enabled,
        })
        .unwrap();
        let original = app.native_command.as_ref().unwrap().clone();
        let intent = original.intent().cloned();
        assert!(matches!(intent, Some(NativeIntent::CalendarPolicy { .. })));
        lose_worker_reply(&mut app);
        let pair = original.snapshot().unwrap();
        let committed = backend
            .lock()
            .unwrap()
            .calendar_status(lease, year)
            .unwrap();
        assert_ne!(committed.policy.revision, before.policy.revision);
        app.submit(Work::LoadAccounts(backend.clone(), lease))
            .unwrap();
        wait_for(&mut app, |app| app.pending.is_none());
        assert!(
            app.accounts.is_some(),
            "unrelated actual current read succeeded"
        );
        assert!(
            app.native_unknown,
            "accounts do not reconcile a calendar policy"
        );
        assert_eq!(app.native_command.as_ref(), Some(&original));
        app.submit(Work::Calendar(Box::new(calendar_flow::Job {
            backend: backend.clone(),
            lease: Some(lease),
            operation: calendar_flow::Operation::Load(year),
        })))
        .unwrap();
        let until = Instant::now() + Duration::from_secs(15);
        while !app.pending.as_ref().unwrap().ticket.is_finished() {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        app.generation = app.generation.checked_add(1).unwrap();
        wait_for(&mut app, |app| app.pending.is_none());
        assert!(
            app.native_unknown,
            "a queued successful read from the old UI generation cannot reconcile"
        );
        assert_eq!(app.native_command.as_ref(), Some(&original));
        assert_eq!(original.snapshot(), Some(pair));
        assert!(original.intent() == intent.as_ref());
        // The real stale-generation path refreshes authority and returns to
        // Login. Authenticate again through the existing UI/worker, never by
        // copying the old lease or assigning a replacement command identity.
        assert!(app.screen == Screen::Login);
        assert!(app.lease.is_none());
        voice_management_probe::login(&mut app, Instant::now() + Duration::from_secs(90)).unwrap();
        let current_lease = app.lease.unwrap();
        assert_eq!(current_lease.principal_id(), lease.principal_id());
        assert!(app.native_unknown);
        assert_eq!(app.native_command.as_ref(), Some(&original));
        assert_eq!(original.snapshot(), Some(pair));
        app.submit(Work::Calendar(Box::new(calendar_flow::Job {
            backend: backend.clone(),
            lease: Some(current_lease),
            operation: calendar_flow::Operation::Load(year),
        })))
        .unwrap();
        wait_for(&mut app, |app| app.pending.is_none());
        assert!(
            app.calendar.is_some(),
            "matching actual current read succeeded"
        );
        assert!(
            app.native_unknown,
            "a matching current read is still not the original typed receipt"
        );
        assert_eq!(app.native_command.as_ref(), Some(&original));
        assert_eq!(original.snapshot(), Some(pair));
        assert!(original.intent() == intent.as_ref());
        // This explicit route queries the original pair/intent; it does not
        // issue or submit another policy change after reauthentication.
        app.submit_calendar(calendar_flow::Operation::Load(year))
            .unwrap();
        wait_for(&mut app, |app| app.pending.is_none());
        assert!(!app.native_unknown);
        assert_eq!(
            app.native_command.as_ref(),
            Some(&original),
            "history never replaces the old command identity"
        );
        assert_eq!(original.snapshot(), Some(pair));
        assert!(original.intent() == intent.as_ref());
        let after = backend
            .lock()
            .unwrap()
            .calendar_status(current_lease, year)
            .unwrap();
        assert_eq!(
            after.policy, committed.policy,
            "history cannot apply policy twice"
        );
        drain(app);
    }
    #[test]
    fn shutdown_during_open_hands_backend_to_shutdown_worker_before_pool_stops() {
        let _serial = SERIAL.lock().unwrap();
        let (_scratch, mut app) = fixture();
        app.begin_shutdown();
        assert!(app.pending.is_some());
        assert!(!app.pool.handle().is_faulted());
        wait_for(&mut app, |a| a.pool.is_finished());
        assert!(app.backend.is_some());
        assert!(app.control.as_ref().unwrap().is_local_locked());
        assert!(app
            .backend
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .state()
            .is_err());
        app.pool.join().unwrap();
    }
    #[test]
    fn shutdown_keeps_writer_alive_until_audio_acknowledges_native_release() {
        use fc_bootstrap::voice_runtime::{
            AudioDomain, DomainProgress, RuntimeControl, RuntimeError, RuntimeFactory, StopReasons,
        };
        struct ReleaseGate {
            entered: Arc<AtomicBool>,
            release: Arc<AtomicBool>,
        }
        struct Domain(ReleaseGate);
        impl RuntimeFactory for ReleaseGate {
            fn invalidate_runtime(&self) {}
            fn create(
                &self,
                _: RuntimeControl,
            ) -> std::result::Result<Box<dyn AudioDomain>, RuntimeError> {
                Ok(Box::new(Domain(ReleaseGate {
                    entered: self.entered.clone(),
                    release: self.release.clone(),
                })))
            }
        }
        impl AudioDomain for Domain {
            fn interrupt(&mut self, _: StopReasons) -> std::result::Result<(), RuntimeError> {
                self.0.entered.store(true, Ordering::Release);
                Ok(())
            }
            fn step(&mut self) -> std::result::Result<DomainProgress, RuntimeError> {
                Ok(if self.0.release.load(Ordering::Acquire) {
                    DomainProgress::Idle
                } else {
                    DomainProgress::Draining
                })
            }
            fn has_native_ownership(&self) -> bool {
                !self.0.release.load(Ordering::Acquire)
            }
            fn resume_admission(&mut self) -> std::result::Result<(), RuntimeError> {
                Ok(())
            }
            fn finish(&mut self) -> std::result::Result<bool, RuntimeError> {
                Ok(!self.has_native_ownership())
            }
        }
        let _serial = SERIAL.lock().unwrap();
        let (_scratch, mut app) = fixture();
        wait_for(&mut app, |a| a.pending.is_none());
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        // No real audio task was submitted and its initial supervisor has never
        // spawned. Replace only this isolated app's empty native factory.
        assert_eq!(
            app.audio
                .as_ref()
                .unwrap()
                .status()
                .unwrap()
                .physical_threads_started,
            0
        );
        app.audio = Some(VoiceRuntime::new(Arc::new(ReleaseGate {
            entered: entered.clone(),
            release: release.clone(),
        })));
        app.begin_shutdown();
        wait_for(&mut app, |_| entered.load(Ordering::Acquire));
        assert!(
            app.shutdown.is_none(),
            "writer shutdown was admitted while native audio was held"
        );
        assert!(app
            .backend
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .state()
            .is_ok());
        release.store(true, Ordering::Release);
        wait_for(&mut app, |a| a.pool.is_finished());
        assert!(app
            .backend
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .state()
            .is_err());
        app.pool.join().unwrap();
    }
    #[test]
    fn lost_initialization_result_refreshes_committed_state_instead_of_reopening_registration() {
        let _serial = SERIAL.lock().unwrap();
        let (_scratch, mut app) = fixture();
        wait_for(&mut app, |a| a.pending.is_none());
        assert!(app.screen == Screen::Setup, "{}", app.status);
        let mut data = FormData::new();
        for (id, value) in [
            (SCHOOL, "Lifecycle School"),
            (LOGIN, "admin"),
            (NAME, "Admin"),
            (PASSWORD, "Lifecycle-probe-password"),
            (CONFIRM, "Lifecycle-probe-password"),
        ] {
            data.insert(id, value.into()).unwrap();
        }
        let request = Request {
            backend: app.backend.as_ref().unwrap().clone(),
            screen: Screen::Setup,
            action: SAVE,
            data,
            lease: None,
            state: app.state.clone(),
            key: None,
            revision: Revision::default(),
            clock: app.clock.clone(),
        };
        app.submit(Work::Request(request)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !app.pending.as_ref().unwrap().ticket.is_finished() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        // COMMIT completed, but notification consumption lost ownership. The same
        // product error path must read persisted state and discard the one-time code.
        app.pending.as_ref().unwrap().ticket.cancel();
        app.poll().unwrap();
        wait_for(&mut app, |a| a.pending.is_none());
        assert!(app.screen == Screen::Login);
        assert!(app.state.installation.is_some());
        assert_eq!(app.state.accounts.len(), 1);
        assert!(app.lease.is_none());
        drain(app);
    }
    #[test]
    fn publication_failure_is_fatal_but_conflict_and_wrong_password_are_recoverable() {
        assert!(fatal(&Failure::Backend(BackendError::Identity(
            fc_ports::IdentityRepositoryError::Faulted
        ))));
        assert!(fatal(&Failure::Backend(BackendError::Storage(
            fc_storage::StorageError::Indeterminate
        ))));
        assert!(!fatal(&Failure::Backend(BackendError::InvalidCredentials)));
        assert!(!fatal(&Failure::Backend(BackendError::Identity(
            fc_ports::IdentityRepositoryError::Domain(IdentityError::RevisionConflict)
        ))));
    }
    #[test]
    fn exit_guard_returns_only_the_current_owners_unapplied_display_draft() {
        let _serial = SERIAL.lock().unwrap();
        let mut app = create_app(Arc::new(CurrentClock::new()), true).unwrap();
        let mut sessions = fc_application::identity::IdentitySessions::default();
        let expires = fc_domain::time::LocalDateTime::new(
            fc_domain::time::LocalDate::new(2030, 1, 1).unwrap(),
            0,
        )
        .unwrap();
        let account = |id| UserAccount {
            principal: Principal {
                id: PrincipalId::new(id).unwrap(),
                school_id: SchoolId::new(1).unwrap(),
                role: Role::GlobalAdmin,
                enabled: true,
                auth_epoch: AuthEpoch::new(1),
            },
            login_name: format!("admin{id}"),
            display_name: format!("Admin{id}"),
            must_change_password: false,
            credential_version: 1,
            recovery_version: 1,
        };
        let owner = sessions
            .issue_verified(&account(1), TrustedOrigin::Local, 0, 3600000, expires)
            .unwrap();
        let other = sessions
            .issue_verified(&account(2), TrustedOrigin::Local, 0, 3600000, expires)
            .unwrap();
        let mut editor =
            display_editor::Editor::new(DisplaySettings::default(), owner.principal_id());
        let mut data = FormData::new();
        for f in editor.view("", false).fields {
            let value = if f.id == display_editor::LABEL {
                "本账号未应用的名称".into()
            } else {
                f.value
            };
            data.insert(f.id, value).unwrap();
        }
        assert!(matches!(
            editor.handle(display_editor::EDIT_BACK, data),
            Ok(display_editor::Command::Close)
        ));
        app.display = Some(editor);
        app.state = Arc::new(IdentityState {
            accounts: vec![account(1)],
            ..IdentityState::default()
        });
        app.lease = Some(owner);
        app.screen = Screen::Home;
        app.event(Event::Submit(EXIT, FormData::new())).unwrap();
        assert!(app.screen == Screen::ExitDraft);
        app.event(Event::Submit(RETURN_DRAFT, FormData::new()))
            .unwrap();
        assert!(app.screen == Screen::Display);
        assert!(app
            .view()
            .fields
            .iter()
            .any(|f| f.id == display_editor::LABEL && f.value == "本账号未应用的名称"));
        app.lease = Some(other);
        app.screen = Screen::ExitDraft;
        assert!(!app.has_unsaved_changes());
        app.event(Event::Submit(RETURN_DRAFT, FormData::new()))
            .unwrap();
        assert!(app.screen == Screen::Home);
        assert!(app.display.as_ref().unwrap().has_unsaved_changes());
        drain(app);
    }

    #[test]
    fn default_teacher_native_confirm_original_receipt_after_reply_loss_and_later_change() {
        use fc_domain::identity::DefaultTeacher;
        use fc_ports::command_window::CommandFamily;
        let _serial = SERIAL.lock().unwrap();
        let (scratch, mut app) = ready_management();
        let lease = app.lease.unwrap();
        let backend = app.backend.as_ref().unwrap().clone();
        let class = ClassId::new(1).unwrap();
        let subject = SubjectId::new(1).unwrap();
        {
            let mut backend = backend.lock().unwrap();
            for operation in [
                IdentityCommand::CreateClass {
                    id: class,
                    name: "合成默认老师班级".into(),
                },
                IdentityCommand::PutSubject {
                    id: subject,
                    name: "合成默认老师科目".into(),
                    enabled: true,
                    color: Rgb {
                        red: 20,
                        green: 30,
                        blue: 40,
                    },
                    expected_revision: None,
                },
            ] {
                let expected = backend.state().unwrap().revision;
                let issued = backend
                    .issue_native_command(
                        CommandFamily::Identity,
                        Some(lease),
                        SessionPurpose::Manage,
                    )
                    .unwrap();
                backend
                    .begin_command_scope(issued)
                    .unwrap()
                    .identity_at(lease, expected, operation)
                    .unwrap();
            }
            for id in [2, 3] {
                let expected = backend.state().unwrap().revision;
                let issued = backend
                    .issue_native_command(
                        CommandFamily::Identity,
                        Some(lease),
                        SessionPurpose::Manage,
                    )
                    .unwrap();
                drop(
                    backend
                        .begin_command_scope(issued)
                        .unwrap()
                        .create_account_at(
                            lease,
                            expected,
                            NewAccount {
                                id: PrincipalId::new(id).unwrap(),
                                login_name: format!("default-teacher-{id}"),
                                display_name: "同名合成教师".into(),
                                role: Role::TeacherAdmin,
                                scopes: vec![AssignmentScope {
                                    class_id: class,
                                    subject_id: subject,
                                }],
                            },
                            Password::new("Synthetic default teacher password 2026!".into())
                                .unwrap(),
                        )
                        .unwrap(),
                );
            }
        }
        app.open_accounts().unwrap();
        wait_for(&mut app, |app| app.pending.is_none());
        let before = backend.lock().unwrap().state().unwrap().revision;
        let deadline = Instant::now() + Duration::from_secs(60);
        voice_management_probe::submit(&mut app, accounts_editor::DEFAULTS, &[], deadline).unwrap();
        voice_management_probe::submit(
            &mut app,
            accounts_editor::DEFAULT_SELECT,
            &[
                (accounts_editor::CLASS, "1"),
                (accounts_editor::SUBJECT, "1"),
            ],
            deadline,
        )
        .unwrap();
        voice_management_probe::submit(
            &mut app,
            accounts_editor::REVIEW,
            &[(accounts_editor::DEFAULT_TEACHER, "2")],
            deadline,
        )
        .unwrap();
        assert_eq!(
            backend.lock().unwrap().state().unwrap().revision,
            before,
            "scope and review cannot write"
        );
        app.window
            .probe_submit(accounts_editor::CONFIRM, FormData::new())
            .unwrap();
        for event in app.window.take_events().unwrap() {
            app.event(event).unwrap();
        }
        lose_worker_reply(&mut app);
        let original = app.native_command.as_ref().unwrap().clone();
        let original_pair = original.snapshot().unwrap();
        let original_default = DefaultTeacher {
            class_id: class,
            subject_id: subject,
            teacher_id: PrincipalId::new(2).unwrap(),
        };
        assert_eq!(
            backend.lock().unwrap().state().unwrap().default_teachers,
            [original_default]
        );
        assert_eq!(
            backend.lock().unwrap().state().unwrap().revision,
            before.next().unwrap()
        );
        let later = DefaultTeacher {
            teacher_id: PrincipalId::new(3).unwrap(),
            ..original_default
        };
        {
            let mut backend = backend.lock().unwrap();
            let expected = backend.state().unwrap().revision;
            let issued = backend
                .issue_native_command(CommandFamily::Identity, Some(lease), SessionPurpose::Manage)
                .unwrap();
            backend
                .begin_command_scope(issued)
                .unwrap()
                .identity_at(lease, expected, IdentityCommand::SetDefaultTeacher(later))
                .unwrap();
        }
        let after_later = backend.lock().unwrap().state().unwrap().revision;
        voice_management_probe::submit(&mut app, accounts_editor::RELOAD, &[], deadline).unwrap();
        assert!(
            app.native_unknown,
            "leaving the draft is not original result confirmation"
        );
        voice_management_probe::submit(&mut app, accounts_editor::DISCARD, &[], deadline).unwrap();
        assert!(!app.native_unknown);
        assert!(app.native_command.as_ref() == Some(&original));
        assert_eq!(
            app.native_command.as_ref().unwrap().snapshot(),
            Some(original_pair)
        );
        assert_eq!(
            app.state.default_teachers,
            [later],
            "historical state must never replace current authority"
        );
        assert!(
            app.status.contains("默认老师为#2"),
            "must show original result independently: {}",
            app.status
        );
        assert_eq!(
            backend.lock().unwrap().state().unwrap().revision,
            after_later
        );
        {
            let mut backend = backend.lock().unwrap();
            let mut scoped = backend.begin_command_scope(original_pair).unwrap();
            assert_eq!(
                scoped
                    .default_teacher_outcome(lease, before, original_default)
                    .unwrap(),
                Some((before.next().unwrap(), original_default))
            );
            assert!(matches!(
                scoped.default_teacher_outcome(lease, before, later),
                Err(BackendError::Identity(
                    fc_ports::IdentityRepositoryError::IdempotencyConflict
                ))
            ));
        }
        {
            let mut backend = backend.lock().unwrap();
            let expected = backend.state().unwrap().revision;
            let issued = backend
                .issue_native_command(CommandFamily::Identity, Some(lease), SessionPurpose::Manage)
                .unwrap();
            assert_eq!(
                backend
                    .begin_command_scope(issued)
                    .unwrap()
                    .default_teacher_outcome(lease, expected, original_default)
                    .unwrap(),
                None
            );
            assert_eq!(
                backend.state().unwrap().revision,
                expected,
                "missing receipt cannot execute the requested write"
            );
            assert_eq!(backend.state().unwrap().default_teachers, [later]);
        }

        // An actual concurrent identity write makes this captured UI CAS stale.
        // Its definite rejection preserves the draft without claiming UNKNOWN.
        voice_management_probe::submit(&mut app, accounts_editor::DEFAULTS, &[], deadline).unwrap();
        voice_management_probe::submit(
            &mut app,
            accounts_editor::DEFAULT_SELECT,
            &[
                (accounts_editor::CLASS, "1"),
                (accounts_editor::SUBJECT, "1"),
            ],
            deadline,
        )
        .unwrap();
        voice_management_probe::submit(
            &mut app,
            accounts_editor::REVIEW,
            &[(accounts_editor::DEFAULT_TEACHER, "2")],
            deadline,
        )
        .unwrap();
        let after_concurrent = {
            let mut backend = backend.lock().unwrap();
            let expected = backend.state().unwrap().revision;
            let issued = backend
                .issue_native_command(CommandFamily::Identity, Some(lease), SessionPurpose::Manage)
                .unwrap();
            let changed = backend
                .begin_command_scope(issued)
                .unwrap()
                .identity_at(
                    lease,
                    expected,
                    IdentityCommand::RenameSchool {
                        name: "Synthetic concurrent catalog change".into(),
                    },
                )
                .unwrap();
            changed.revision
        };
        voice_management_probe::submit(&mut app, accounts_editor::CONFIRM, &[], deadline).unwrap();
        assert!(
            !app.native_unknown,
            "an actual pre-COMMIT CAS refusal is a known rejection"
        );
        assert!(app.accounts.as_ref().unwrap().has_unsaved_changes());
        assert!(app.status.contains("明确拒绝"));
        assert_eq!(
            backend.lock().unwrap().state().unwrap().default_teachers,
            [later]
        );
        assert_eq!(
            backend.lock().unwrap().state().unwrap().revision,
            after_concurrent
        );
        voice_management_probe::submit(&mut app, accounts_editor::RELOAD, &[], deadline).unwrap();
        voice_management_probe::submit(&mut app, accounts_editor::DISCARD, &[], deadline).unwrap();
        assert!(!app.native_unknown);
        assert_eq!(
            backend.lock().unwrap().state().unwrap().revision,
            after_concurrent
        );
        drain(app);
        // Independent normal Open and actual login retain the original receipt;
        // no historical IdentityState is applied to the new authority.
        let paths = DataPaths::isolated_test(scratch.0.clone()).unwrap();
        let mut reopened = create_app(Arc::new(CurrentClock::new()), true).unwrap();
        reopened.root = scratch.0.clone();
        reopened.desktop.set_root(scratch.0.clone());
        reopened
            .submit(Work::Open(paths, reopened.clock.clone()))
            .unwrap();
        wait_for(&mut reopened, |app| app.pending.is_none());
        voice_management_probe::login(&mut reopened, Instant::now() + Duration::from_secs(60))
            .unwrap();
        {
            let mut backend = reopened.backend.as_ref().unwrap().lock().unwrap();
            let current = reopened.lease.unwrap();
            assert_eq!(
                backend
                    .begin_command_scope(original_pair)
                    .unwrap()
                    .default_teacher_outcome(current, before, original_default)
                    .unwrap(),
                Some((before.next().unwrap(), original_default))
            );
            assert_eq!(backend.state().unwrap().default_teachers, [later]);
            assert_eq!(backend.state().unwrap().revision, after_concurrent);
        }
        drain(reopened);
    }
    #[test]
    fn account_draft_exit_guard_is_owner_bound_and_native_lock_supersedes_pending_work() {
        let _serial = SERIAL.lock().unwrap();
        // UI ownership fixture only. These issued test leases are not backend
        // authentication evidence; the application probe covers real login/SQL.
        let mut app = create_app(Arc::new(CurrentClock::new()), true).unwrap();
        let now = fc_domain::time::LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(),
            36000,
        )
        .unwrap();
        let mut state = IdentityState::default()
            .transition(
                &IdentityCommand::FirstAdministrator {
                    school_id: SchoolId::new(1).unwrap(),
                    school_name: "UI ownership fixture".into(),
                    id: PrincipalId::new(1).unwrap(),
                    login_name: "owner".into(),
                    display_name: "Owner".into(),
                },
                None,
                now,
            )
            .unwrap()
            .next;
        let mut second = state.accounts[0].clone();
        second.principal.id = PrincipalId::new(2).unwrap();
        second.login_name = "other".into();
        state.accounts.push(second);
        state.validate().unwrap();
        let expires = fc_domain::time::LocalDateTime::new(
            fc_domain::time::LocalDate::new(2030, 1, 1).unwrap(),
            0,
        )
        .unwrap();
        let mut sessions = fc_application::identity::IdentitySessions::default();
        let owner = sessions
            .issue_verified(
                &state.accounts[0],
                TrustedOrigin::Local,
                0,
                3600000,
                expires,
            )
            .unwrap();
        let other = sessions
            .issue_verified(
                &state.accounts[1],
                TrustedOrigin::Local,
                0,
                3600000,
                expires,
            )
            .unwrap();
        let mut editor = accounts_editor::Editor::new(&state, owner.principal_id()).unwrap();
        editor.select(0, 0, &state).unwrap();
        editor
            .handle(accounts_editor::EDIT, FormData::new(), &state)
            .unwrap();
        let mut form = FormData::new();
        form.insert(accounts_editor::NAME, "未保存的账号姓名".into())
            .unwrap();
        editor
            .handle(accounts_editor::REVIEW, form, &state)
            .unwrap();
        app.accounts = Some(editor);
        app.state = Arc::new(state);
        app.lease = Some(owner);
        app.screen = Screen::Home;
        app.event(Event::Submit(EXIT, FormData::new())).unwrap();
        assert!(app.screen == Screen::ExitDraft);
        assert!(app.view().description.contains("账号"));
        app.event(Event::Submit(RETURN_DRAFT, FormData::new()))
            .unwrap();
        assert!(app.screen == Screen::Accounts);
        assert!(app.view().description.contains("未保存的账号姓名"));
        app.lease = Some(other);
        app.screen = Screen::ExitDraft;
        assert!(!app.has_unsaved_changes());
        app.event(Event::Submit(RETURN_DRAFT, FormData::new()))
            .unwrap();
        assert!(app.screen == Screen::Home);
        assert!(app.accounts.as_ref().unwrap().has_unsaved_changes());
        app.lease = Some(owner);
        app.screen = Screen::Accounts;
        app.submit(Work::ValidateOffice(PathBuf::from(
            "nonexistent-owned-lock-fixture.docx",
        )))
        .unwrap();
        assert!(app.pending.is_some());
        let old_generation = app.generation;
        // This invokes our own actual hidden native button. All other busy
        // actions remain unavailable; LOCK carries no copied form or password.
        app.window.probe_submit(LOCK, FormData::new()).unwrap();
        for event in app.window.take_events().unwrap() {
            app.event(event).unwrap();
        }
        assert_eq!(app.generation, old_generation + 1);
        assert!(app.lease.is_none());
        assert!(app.screen == Screen::Login);
        assert!(app.accounts.as_ref().unwrap().has_unsaved_changes());
        assert!(app
            .view()
            .fields
            .iter()
            .all(|f| f.id != accounts_editor::NAME));
        wait_for(&mut app, |a| a.pending.is_none());
        assert!(app.lease.is_none());
        assert!(app.screen == Screen::Login);
        drain(app);
    }
    #[test]
    fn profile_partial_draft_exit_owner_and_busy_native_lock_discard_old_canvas_events() {
        let _serial = SERIAL.lock().unwrap();
        // These issued leases exercise UI ownership only; the application probe
        // separately uses real credentials, native controls and the SQL writer.
        let mut app = create_app(Arc::new(CurrentClock::new()), true).unwrap();
        let now = fc_domain::time::LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(),
            36000,
        )
        .unwrap();
        let mut state = IdentityState::default()
            .transition(
                &IdentityCommand::FirstAdministrator {
                    school_id: SchoolId::new(1).unwrap(),
                    school_name: "Profile UI fixture".into(),
                    id: PrincipalId::new(1).unwrap(),
                    login_name: "owner".into(),
                    display_name: "Owner".into(),
                },
                None,
                now,
            )
            .unwrap()
            .next;
        let mut second = state.accounts[0].clone();
        second.principal.id = PrincipalId::new(2).unwrap();
        second.login_name = "other".into();
        state.accounts.push(second);
        state.validate().unwrap();
        let expires = fc_domain::time::LocalDateTime::new(
            fc_domain::time::LocalDate::new(2030, 1, 1).unwrap(),
            0,
        )
        .unwrap();
        let mut sessions = fc_application::identity::IdentitySessions::default();
        let owner = sessions
            .issue_verified(
                &state.accounts[0],
                TrustedOrigin::Local,
                0,
                3600000,
                expires,
            )
            .unwrap();
        let other = sessions
            .issue_verified(
                &state.accounts[1],
                TrustedOrigin::Local,
                0,
                3600000,
                expires,
            )
            .unwrap();
        let mut editor = profiles_editor::Editor::new(
            &state,
            owner.principal_id(),
            fc_ports::ProfilePage {
                items: Vec::new(),
                total: 0,
                next_after: None,
            },
        )
        .unwrap();
        editor
            .loaded_public(fc_ports::PublicLayoutState {
                layout: Default::default(),
                display: Default::default(),
            })
            .unwrap();
        editor
            .handle(profiles_editor::EDIT_NODE, FormData::new(), &state)
            .unwrap();
        let mut raw = FormData::new();
        raw.insert(profiles_editor::GAP, "2-".into()).unwrap();
        editor.suspend(raw);
        assert!(editor.has_unsaved_changes());
        app.profiles = Some(editor);
        app.state = Arc::new(state);
        app.lease = Some(owner);
        app.screen = Screen::Home;
        app.event(Event::Submit(EXIT, FormData::new())).unwrap();
        assert!(app.screen == Screen::ExitDraft);
        app.event(Event::Submit(RETURN_DRAFT, FormData::new()))
            .unwrap();
        assert!(app.screen == Screen::Profiles);
        assert!(app
            .view()
            .fields
            .iter()
            .any(|f| f.id == profiles_editor::GAP && f.value == "2-"));
        app.lease = Some(other);
        app.screen = Screen::ExitDraft;
        assert!(!app.has_unsaved_changes());
        app.event(Event::Submit(RETURN_DRAFT, FormData::new()))
            .unwrap();
        assert!(app.screen == Screen::Home);
        assert!(app.profiles.as_ref().unwrap().has_unsaved_changes());
        app.lease = Some(owner);
        app.screen = Screen::Profiles;
        app.render().unwrap();
        app.submit(Work::ValidateOffice(PathBuf::from(
            "owned-profile-lock-fixture-missing.docx",
        )))
        .unwrap();
        let old_generation = app.generation;
        let old_preview = app.preview_sequence;
        app.window.probe_submit(LOCK, FormData::new()).unwrap();
        for event in app.window.take_events().unwrap() {
            app.event(event).unwrap();
        }
        assert_eq!(app.generation, old_generation + 1);
        assert!(app.preview_sequence > old_preview);
        assert!(!app.preview_active);
        assert!(app.lease.is_none());
        assert!(app.screen == Screen::Login);
        app.event(Event::LayoutDrop {
            generation: old_preview,
            scope: fc_domain::layout::LayoutScope::Public,
            source: LayoutNodeId::new(2).unwrap(),
            target: LayoutNodeId::new(3).unwrap(),
            before: None,
        })
        .unwrap();
        assert!(app.profiles.as_ref().unwrap().has_unsaved_changes());
        assert!(app.screen == Screen::Login);
        wait_for(&mut app, |a| a.pending.is_none());
        drain(app);
    }
    #[test]
    fn idle_authority_fault_closes_preview_without_user_input_and_preserves_partial_draft() {
        let _serial = SERIAL.lock().unwrap();
        let (_scratch, mut app) = fixture();
        wait_for(&mut app, |a| a.pending.is_none() && a.control.is_some());
        // This test owns idle UI revocation. Drain the idle audio factory while
        // authority is healthy; faulted native ownership follows the product's
        // separate watchdog path and cannot promise a graceful writer close.
        let audio = app.audio.take().unwrap();
        audio.shutdown();
        wait_for(&mut app, |_| {
            // Detached from App.audio, this fixture must drive the same
            // nonblocking reaper that the normal application timer owns.
            audio.reap_nonblocking().unwrap();
            audio
                .status()
                .is_ok_and(|s| s.phase == RuntimePhase::Closed && !s.physical_thread_running)
        });
        // Only the writer/control lifecycle is real here. The UI state and
        // transparent image are isolated examples, never a backend login grant.
        let now = app.clock.try_now().unwrap();
        let state = IdentityState::default()
            .transition(
                &IdentityCommand::FirstAdministrator {
                    school_id: SchoolId::new(1).unwrap(),
                    school_name: "Idle UI fixture".into(),
                    id: PrincipalId::new(1).unwrap(),
                    login_name: "owner".into(),
                    display_name: "Owner".into(),
                },
                None,
                now,
            )
            .unwrap()
            .next;
        let mut editor = profiles_editor::Editor::new(
            &state,
            PrincipalId::new(1).unwrap(),
            fc_ports::ProfilePage {
                items: Vec::new(),
                total: 0,
                next_after: None,
            },
        )
        .unwrap();
        editor
            .loaded_public(fc_ports::PublicLayoutState {
                layout: Default::default(),
                display: Default::default(),
            })
            .unwrap();
        editor
            .handle(profiles_editor::EDIT_NODE, FormData::new(), &state)
            .unwrap();
        let mut raw = FormData::new();
        raw.insert(profiles_editor::GAP, "3-".into()).unwrap();
        editor.suspend(raw);
        app.profiles = Some(editor);
        app.state = Arc::new(state);
        app.screen = Screen::Profiles;
        app.render().unwrap();
        let key = fc_presentation::layout::SceneNodeKey {
            scope: fc_domain::layout::LayoutScope::Public,
            id: LayoutNodeId::new(1).unwrap(),
        };
        let rect = fc_presentation::Rect {
            x: 0.,
            y: 0.,
            width: 120.,
            height: 80.,
        };
        app.window
            .set_layout_preview(Some(fc_windows::workbench::LayoutPreview {
                generation: 1,
                width: 120,
                height: 80,
                bgra: vec![0; 120 * 80 * 4],
                nodes: vec![fc_windows::workbench::PreviewNode {
                    key,
                    parent: None,
                    rect,
                    clip: rect,
                }],
            }))
            .unwrap();
        app.preview_active = true;
        app.preview_sequence = 1;
        app.window.set_layout_selection(Some(key)).unwrap();
        app.control.as_ref().unwrap().fault();
        // No native input or pending UI command is needed to observe revocation.
        app.poll().unwrap();
        assert!(app.stopping);
        assert!(!app.preview_active);
        assert!(app.preview_sequence > 1);
        assert!(app.window.set_layout_selection(Some(key)).is_err());
        assert!(app.profiles.as_ref().unwrap().has_unsaved_changes());
        wait_for(&mut app, |a| a.pool.is_finished());
        app.pool.join().unwrap();
    }
    fn subject_test_identity(app: &App, command: IdentityCommand) -> IdentityState {
        let mut backend = app.backend.as_ref().unwrap().lock().unwrap();
        let lease = app.lease.unwrap();
        let expected = backend.state().unwrap().revision;
        let issued = backend
            .issue_native_command(
                fc_ports::command_window::CommandFamily::Identity,
                Some(lease),
                SessionPurpose::Manage,
            )
            .unwrap();
        let result = backend
            .begin_command_scope(issued)
            .unwrap()
            .identity_at(lease, expected, command)
            .unwrap();
        result
    }
    fn subject_test_ui(app: &mut App, action: ActionId, values: &[(FieldId, &str)]) {
        voice_management_probe::submit(
            app,
            action,
            values,
            Instant::now() + Duration::from_secs(60),
        )
        .unwrap();
    }
    fn subject_test_edit(app: &mut App, enabled: &str) {
        subject_test_ui(
            app,
            settings_registry::SUBJECT_EDIT,
            &[(settings_registry::SUBJECT_SELECT, "1")],
        );
        subject_test_ui(
            app,
            settings_registry::SUBJECT_REVIEW,
            &[(settings_registry::SUBJECT_ENABLED, enabled)],
        );
    }
    #[test]
    fn update_data_subject_text_real_cas_twice_stale_reject_preserves_originals_and_hidden_mode() {
        let _serial = SERIAL.lock().unwrap();
        let (scratch, mut app) = ready_management();
        let paths = DataPaths::isolated_test(scratch.0.clone()).unwrap();
        assert!(app.offscreen_probe && app.probe_trace);
        app.probe_trace = false;
        migration_probe_fixture::test_update_subject_text_cas(&mut app, &paths).unwrap();
        assert!(
            app.offscreen_probe,
            "quiet diagnostics retain hidden/picker protections"
        );
        drain(app);
        drop(scratch);
    }
    #[test]
    fn subjects_native_disable_receipt_after_lost_reply_rename_without_enabling_and_reenable_keep_originals(
    ) {
        use fc_domain::{
            layout::{LayoutScope, NodeKind, WidgetKind},
            materials::*,
            profile::SubjectProfileKey,
            profile_content::*,
        };
        use fc_ports::MaterialListRequest;
        let _serial = SERIAL.lock().unwrap();
        let (scratch, mut app) = ready_management();
        let paths = DataPaths::isolated_test(scratch.0.clone()).unwrap();
        migration_probe_fixture::seed_source(&mut app, &paths).unwrap();
        let backend = app.backend.as_ref().unwrap().clone();
        let lease = app.lease.unwrap();
        let scope = MaterialScope {
            teacher_id: PrincipalId::new(2).unwrap(),
            subject_id: SubjectId::new(1).unwrap(),
        };
        let profile_key = SubjectProfileKey {
            class_id: ClassId::new(1).unwrap(),
            subject_id: scope.subject_id,
            teacher_id: scope.teacher_id,
        };
        let (before, page, profile, images) = {
            let b = backend.lock().unwrap();
            let profile = b.read_subject_profile(lease, profile_key).unwrap().unwrap();
            let slot = ContentSlotKey {
                scope: LayoutScope::Subject(profile_key),
                node_id: profile
                    .tree
                    .nodes
                    .iter()
                    .find(|n| n.kind == NodeKind::Widget(WidgetKind::SubjectImage))
                    .unwrap()
                    .id,
            };
            (
                b.state().unwrap(),
                b.materials(
                    lease,
                    MaterialListRequest {
                        scope,
                        cursor: None,
                    },
                )
                .unwrap(),
                profile,
                b.read_profile_content(lease, slot).unwrap(),
            )
        };
        let original_path = paths.root().join("teachers/2/subjects/1/500/501.txt");
        let original_bytes = fs::read(&original_path).unwrap();
        app.open_subjects().unwrap();
        wait_for(&mut app, |a| a.pending.is_none());
        subject_test_edit(&mut app, "0");
        assert!(app.view().description.contains("默认老师1项"));
        subject_test_ui(&mut app, settings_registry::SUBJECT_CANCEL, &[]);
        assert_eq!(
            backend.lock().unwrap().state().unwrap(),
            before,
            "review/cancel never writes"
        );
        subject_test_edit(&mut app, "0");
        app.window
            .probe_submit(settings_registry::SUBJECT_CONFIRM, FormData::new())
            .unwrap();
        for event in app.window.take_events().unwrap() {
            app.event(event).unwrap();
        }
        lose_worker_reply(&mut app);
        let original = app.native_command.as_ref().unwrap().clone();
        let pair = original.snapshot().unwrap();
        let NativeIntent::Subject {
            expected,
            command: original_intent,
        } = original.intent().unwrap().clone()
        else {
            panic!("subject original intent");
        };
        let disabled = backend.lock().unwrap().state().unwrap();
        assert!(!disabled.subjects[0].enabled);
        assert!(disabled.default_teachers.is_empty());
        assert_eq!(disabled.assignments, before.assignments);
        assert_eq!(fs::read(&original_path).unwrap(), original_bytes);
        // A later real publication deliberately differs from the original result.
        let saved = &disabled.subjects[0];
        subject_test_identity(
            &app,
            IdentityCommand::PutSubject {
                id: saved.id,
                name: "后续停用名称".into(),
                enabled: false,
                color: saved.color,
                expected_revision: Some(saved.revision),
            },
        );
        let later = backend.lock().unwrap().state().unwrap();
        subject_test_ui(&mut app, settings_registry::SUBJECT_RELOAD, &[]);
        assert!(!app.native_unknown);
        assert_eq!(app.native_command.as_ref(), Some(&original));
        assert_eq!(
            app.state.as_ref(),
            &later,
            "historical identity cannot replace current authority"
        );
        assert!(app.status.contains("源合成科目一") && app.status.contains("停用"));
        assert_eq!(
            backend.lock().unwrap().state().unwrap(),
            later,
            "original receipt query never replays identity"
        );
        {
            let mut b = backend.lock().unwrap();
            let mut original_scope = b.begin_command_scope(pair).unwrap();
            let result = original_scope
                .subject_outcome(lease, expected, original_intent.clone())
                .unwrap()
                .unwrap();
            assert_eq!(result.1, disabled.subjects[0]);
            assert!(original_scope
                .subject_outcome(lease, expected.next().unwrap(), original_intent.clone())
                .is_err());
            let mut tampered = original_intent.clone();
            if let IdentityCommand::PutSubject { enabled, .. } = &mut tampered {
                *enabled = true;
            }
            assert!(
                original_scope
                    .subject_outcome(lease, expected, tampered)
                    .is_err(),
                "original C2 binds complete intent"
            );
        }
        subject_test_ui(
            &mut app,
            settings_registry::SUBJECT_EDIT,
            &[(settings_registry::SUBJECT_SELECT, "1")],
        );
        subject_test_ui(
            &mut app,
            settings_registry::SUBJECT_REVIEW,
            &[(SUBJECT, "界面改名仍停用")],
        );
        subject_test_ui(&mut app, settings_registry::SUBJECT_CONFIRM, &[]);
        assert!(
            !backend.lock().unwrap().state().unwrap().subjects[0].enabled,
            "renaming cannot silently enable"
        );
        subject_test_edit(&mut app, "1");
        subject_test_ui(&mut app, settings_registry::SUBJECT_CONFIRM, &[]);
        let b = backend.lock().unwrap();
        let enabled = b.state().unwrap();
        assert!(enabled.subjects[0].enabled);
        assert_eq!(enabled.subjects[0].id, before.subjects[0].id);
        assert_eq!(enabled.subjects[0].color, before.subjects[0].color);
        assert_eq!(enabled.assignments, before.assignments);
        assert!(
            enabled.default_teachers.is_empty(),
            "re-enable cannot silently choose a default teacher"
        );
        assert!(
            enabled
                .account(scope.teacher_id)
                .unwrap()
                .principal
                .auth_epoch
                > before
                    .account(scope.teacher_id)
                    .unwrap()
                    .principal
                    .auth_epoch
        );
        assert_eq!(
            b.materials(
                lease,
                MaterialListRequest {
                    scope,
                    cursor: None
                }
            )
            .unwrap(),
            page
        );
        assert_eq!(
            b.read_subject_profile(lease, profile_key).unwrap().unwrap(),
            profile
        );
        assert_eq!(
            b.read_profile_content(lease, images.slot).unwrap(),
            images,
            "both real image versions survive"
        );
        assert_eq!(fs::read(&original_path).unwrap(), original_bytes);
        drop(b);
        drain(app);
        drop(backend);
        drop(scratch);
    }
    #[test]
    fn subjects_native_stale_identity_and_retained_only_timetable_reference_are_definite_rejections(
    ) {
        use fc_domain::{schedule::*, schedule_document::SemesterPlan, time::*};
        let _serial = SERIAL.lock().unwrap();
        let (scratch, mut app) = ready_management();
        let paths = DataPaths::isolated_test(scratch.0.clone()).unwrap();
        migration_probe_fixture::seed_source(&mut app, &paths).unwrap();
        let backend = app.backend.as_ref().unwrap().clone();
        let lease = app.lease.unwrap();
        app.open_subjects().unwrap();
        wait_for(&mut app, |a| a.pending.is_none());
        subject_test_edit(&mut app, "0");
        subject_test_identity(
            &app,
            IdentityCommand::RenameSchool {
                name: "并发合成学校".into(),
            },
        );
        let concurrent = backend.lock().unwrap().state().unwrap();
        subject_test_ui(&mut app, settings_registry::SUBJECT_CONFIRM, &[]);
        assert_eq!(backend.lock().unwrap().state().unwrap(), concurrent);
        assert!(!app.native_unknown && app.status.contains("拒绝"));
        assert!(app.has_unsaved_changes());
        subject_test_ui(&mut app, settings_registry::SUBJECT_RELOAD, &[]);
        // Only the retained Even table references this subject. The surviving
        // Single table has no cells; counting only current rows would miss it.
        {
            let year = app.clock.try_now().unwrap().date.year();
            let anchor = (25..=31)
                .filter_map(|day| LocalDate::new(year - 1, 12, day).ok())
                .find(|d| d.weekday() == Weekday::Monday)
                .unwrap();
            let row = |id, cell: bool| TimetableRow {
                id: RowId::new(id).unwrap(),
                logical_key: LogicalRowKey::new(100).unwrap(),
                group: DayGroup::Morning,
                kind: RowKind::Course,
                range: MinuteRange::new(Minute::new(480).unwrap(), Minute::new(525).unwrap())
                    .unwrap(),
                enabled_days: [true; 7],
                courses: std::array::from_fn(|day| {
                    (cell && day == 0).then_some(CourseCell {
                        subject_id: SubjectId::new(1).unwrap(),
                        teacher: TeacherBinding::Default,
                    })
                }),
            };
            let plan = SemesterPlan::new(
                Semester {
                    id: SemesterId::new(100).unwrap(),
                    class_id: ClassId::new(1).unwrap(),
                    revision: Revision::default(),
                    enabled: false,
                    start: LocalDate::new(year, 1, 1).unwrap(),
                    end: LocalDate::new(year, 12, 31).unwrap(),
                    first_week_monday: anchor,
                    tables: vec![
                        WeekTable {
                            id: TableId::new(101).unwrap(),
                            role: TableRole::Odd,
                            rows: vec![row(102, false)],
                        },
                        WeekTable {
                            id: TableId::new(103).unwrap(),
                            role: TableRole::Even,
                            rows: vec![row(104, true)],
                        },
                    ],
                },
                "保留双表引用合成课表".into(),
            )
            .disable_dual(TableId::new(101).unwrap())
            .unwrap();
            let mut b = backend.lock().unwrap();
            let mut document = b.read_schedule(lease).unwrap();
            document.semesters = vec![plan];
            document.update_id_high_water();
            let issued = b
                .issue_native_command(
                    fc_ports::command_window::CommandFamily::Schedule,
                    Some(lease),
                    SessionPurpose::Manage,
                )
                .unwrap();
            b.begin_command_scope(issued)
                .unwrap()
                .save_schedule(lease, document.revision, document)
                .unwrap();
        }
        subject_test_ui(&mut app, settings_registry::SUBJECT_RELOAD, &[]);
        subject_test_edit(&mut app, "0");
        assert!(app.view().description.contains("课表1格（含保留双表）"));
        assert!(app.view().description.contains("保存会被拒绝"));
        let before = backend.lock().unwrap().state().unwrap();
        subject_test_ui(&mut app, settings_registry::SUBJECT_CONFIRM, &[]);
        assert_eq!(backend.lock().unwrap().state().unwrap(), before);
        assert!(!app.native_unknown && app.status.contains("保留双表"));
        assert!(app.has_unsaved_changes());
        // Lock discards the confirmation, preserving only non-secret draft.
        subject_test_ui(&mut app, settings_registry::SUBJECT_REVIEW, &[]);
        app.event(Event::Submit(LOCK, FormData::new())).unwrap();
        assert!(app.screen == Screen::Login && app.lease.is_none());
        assert_eq!(backend.lock().unwrap().state().unwrap(), before);
        drain(app);
        drop(backend);
        drop(scratch);
    }
    #[test]
    fn subjects_native_self_teaching_revocation_confirms_commit_without_new_lease_or_current_read()
    {
        let _serial = SERIAL.lock().unwrap();
        let (scratch, mut app) = ready_management();
        let paths = DataPaths::isolated_test(scratch.0.clone()).unwrap();
        migration_probe_fixture::seed_source(&mut app, &paths).unwrap();
        let backend = app.backend.as_ref().unwrap().clone();
        const INITIAL: &str = "Synthetic self subject administrator 2026!";
        const CHANGED: &str = "Synthetic self subject changed 2026!";
        {
            let mut b = backend.lock().unwrap();
            let lease = app.lease.unwrap();
            let issued = b
                .issue_native_command(
                    fc_ports::command_window::CommandFamily::Identity,
                    Some(lease),
                    SessionPurpose::Manage,
                )
                .unwrap();
            drop(
                b.begin_command_scope(issued)
                    .unwrap()
                    .create_account(
                        lease,
                        NewAccount {
                            id: PrincipalId::new(3).unwrap(),
                            login_name: "subject-self-admin".into(),
                            display_name: "合成任教主管理员".into(),
                            role: Role::GlobalAdmin,
                            scopes: vec![AssignmentScope {
                                class_id: ClassId::new(1).unwrap(),
                                subject_id: SubjectId::new(1).unwrap(),
                            }],
                        },
                        Password::new(INITIAL.into()).unwrap(),
                    )
                    .unwrap(),
            );
        }
        subject_test_ui(&mut app, LOCK, &[]);
        subject_test_ui(
            &mut app,
            LOG_IN,
            &[(LOGIN, "subject-self-admin"), (PASSWORD, INITIAL)],
        );
        assert!(app.screen == Screen::Password);
        subject_test_ui(
            &mut app,
            SAVE,
            &[
                (PASSWORD, INITIAL),
                (NEW_PASSWORD, CHANGED),
                (CONFIRM, CHANGED),
            ],
        );
        subject_test_ui(
            &mut app,
            LOG_IN,
            &[(LOGIN, "subject-self-admin"), (PASSWORD, CHANGED)],
        );
        let old_lease = app.lease.unwrap();
        assert_eq!(old_lease.principal_id().get(), 3);
        app.open_subjects().unwrap();
        wait_for(&mut app, |a| a.pending.is_none());
        subject_test_edit(&mut app, "0");
        app.window
            .probe_submit(settings_registry::SUBJECT_CONFIRM, FormData::new())
            .unwrap();
        for event in app.window.take_events().unwrap() {
            app.event(event).unwrap();
        }
        assert!(
            app.pending.as_ref().unwrap().self_revokes,
            "normal GUI heartbeat must consume its original completion"
        );
        wait_for(&mut app, |a| a.pending.is_none());
        assert!(app.screen == Screen::Login && app.lease.is_none());
        assert!(!app.native_unknown && app.status.contains("已保存"));
        assert!(app.control.as_ref().unwrap().is_local_locked());
        let original = app.native_command.as_ref().unwrap().clone();
        let pair = original.snapshot().unwrap();
        let NativeIntent::Subject { expected, command } = original.intent().unwrap().clone() else {
            panic!("subject original intent");
        };
        {
            let mut b = backend.lock().unwrap();
            let current = b.state().unwrap();
            assert!(!current.subjects[0].enabled);
            assert!(b.check_session(old_lease, SessionPurpose::Manage).is_err());
            let mut original_scope = b.begin_command_scope(pair).unwrap();
            assert!(
                original_scope
                    .subject_outcome(old_lease, expected, command)
                    .is_err(),
                "a saved receipt is never authority for the retired caller"
            );
        }
        app.open_subjects().unwrap();
        assert!(
            app.lease.is_none() && app.screen == Screen::Login,
            "original result display cannot mint or renew a management lease"
        );
        drain(app);
        drop(backend);
        drop(scratch);
    }
}

#[cfg(test)]
mod account_unknown_state_tests {
    use super::*;
    use fc_ports::command_window::{CommandFamily, CommandOwner, CommandTicket, IssuedCommand};
    // Verify unknown account results cannot be replaced by a current read.
    fn command() -> NativeCommand {
        let command = NativeCommand::with_intent(Some(NativeIntent::Account {
            expected: Revision::new(4),
            command: Arc::new(IdentityCommand::UpdateAccount {
                id: PrincipalId::new(2).unwrap(),
                display_name: "原姓名".into(),
                role: Role::User,
                enabled: true,
                scopes: Vec::new(),
            }),
        }));
        command
            .bind(IssuedCommand {
                ticket: CommandTicket::new([3; 16], 8).unwrap(),
                command_id: CommandId::new(8).unwrap(),
                family: CommandFamily::Identity,
                owner: CommandOwner::Principal {
                    principal: PrincipalId::new(1).unwrap(),
                    school: SchoolId::new(1).unwrap(),
                    epoch: AuthEpoch::new(1),
                    origin: TrustedOrigin::Local,
                },
                window_millis: 86_400_000,
            })
            .unwrap();
        command
    }
    #[test]
    fn account_current_read_never_confirms_original_unknown_write() {
        let original = command();
        assert!(!original.permits_current_read_confirmation());
        assert!(NativeCommand::new().permits_current_read_confirmation());
        let history =
            NativeHistory::account(original.clone(), original.original().unwrap(), Ok(None))
                .unwrap();
        assert!(history.matches(&original));
        assert!(history.pending());
        assert!(
            !history.matches(&command()),
            "a new UI intent cannot consume the original result"
        );
        assert!(
            original.account_proof().unwrap().is_none(),
            "missing proof cannot be invented from current data"
        );
    }
    #[test]
    fn inaccessible_or_expired_account_history_stays_unknown_until_explicit_end() {
        for error in [
            fc_ports::command_window::CommandWindowError::NotIssued,
            fc_ports::command_window::CommandWindowError::Expired,
        ] {
            let original = command();
            let history = NativeHistory::account(
                original.clone(),
                original.original().unwrap(),
                Err(BackendError::Identity(
                    fc_ports::IdentityRepositoryError::CommandWindow(error),
                )),
            )
            .unwrap();
            assert!(history.valid_result());
            assert!(history.pending());
        }
    }
    #[test]
    fn a_later_account_name_cannot_match_the_original_result() {
        let original = command();
        let mut result = fc_bootstrap::backend::OriginalAccountResult {
            revision: Revision::new(5),
            account: UserAccount {
                principal: Principal {
                    id: PrincipalId::new(2).unwrap(),
                    school_id: SchoolId::new(1).unwrap(),
                    role: Role::User,
                    enabled: true,
                    auth_epoch: AuthEpoch::new(2),
                },
                login_name: "teacher2".into(),
                display_name: "原姓名".into(),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            },
            scopes: Vec::new(),
        };
        assert!(NativeHistory::account(
            original.clone(),
            original.original().unwrap(),
            Ok(Some(result.clone()))
        )
        .is_ok());
        result.account.display_name = "后来的姓名".into();
        assert!(NativeHistory::account(
            original.clone(),
            original.original().unwrap(),
            Ok(Some(result))
        )
        .is_err());
    }
}
