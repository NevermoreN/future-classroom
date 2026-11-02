//! Static settings-page implementations. Business authorization remains in the
//! original usecases; registration does not create a management capability.
use super::*;
use fc_presentation::registry::{Registration, RegistrationError, Registry};
type SubmitHandler = fn(&mut App, ActionId, FormData) -> std::result::Result<(), String>;
struct Page {
    render: fn(&App, &str) -> View,
    submit: Option<SubmitHandler>,
    early_submit: bool,
    input: fn(&mut App) -> std::result::Result<(), String>,
    grid: fn(&mut App, usize, usize) -> std::result::Result<(), String>,
    suspend: fn(&mut App),
}
static PAGES: &[Registration<Screen, Page>] = &[
    Registration {
        key: Screen::Starting,
        implementation: Page {
            render: page_starting,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Setup,
        implementation: Page {
            render: page_setup,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Login,
        implementation: Page {
            render: page_login,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Recovery,
        implementation: Page {
            render: page_recovery,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Code,
        implementation: Page {
            render: page_code,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Home,
        implementation: Page {
            render: page_home,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::School,
        implementation: Page {
            render: page_school,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Subjects,
        implementation: Page {
            render: page_subjects,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Accounts,
        implementation: Page {
            render: page_accounts,
            submit: Some(App::accounts_event),
            early_submit: false,
            input: no_input,
            grid: grid_accounts,
            suspend: suspend_accounts,
        },
    },
    Registration {
        key: Screen::Password,
        implementation: Page {
            render: page_password,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Rotate,
        implementation: Page {
            render: page_rotate,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Homework,
        implementation: Page {
            render: page_homework,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::HomeworkEdit,
        implementation: Page {
            render: page_homeworkedit,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Materials,
        implementation: Page {
            render: page_materials,
            submit: Some(App::material_event),
            early_submit: false,
            input: no_input,
            grid: grid_materials,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Lesson,
        implementation: Page {
            render: page_lesson,
            submit: Some(App::lesson_event),
            early_submit: false,
            input: input_lesson,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Voice,
        implementation: Page {
            render: page_voice,
            submit: Some(App::voice_management_event),
            early_submit: false,
            input: input_voice,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::HttpsSettings,
        implementation: Page {
            render: page_httpssettings,
            submit: Some(App::https_event),
            early_submit: true,
            input: input_httpssettings,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Web,
        implementation: Page {
            render: page_web,
            submit: Some(App::web_event),
            early_submit: false,
            input: input_web,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Schedule,
        implementation: Page {
            render: page_schedule,
            submit: Some(App::schedule_event),
            early_submit: false,
            input: input_schedule,
            grid: grid_schedule,
            suspend: suspend_schedule,
        },
    },
    Registration {
        key: Screen::Backup,
        implementation: Page {
            render: page_backup,
            submit: Some(App::backup_event),
            early_submit: true,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Restoring,
        implementation: Page {
            render: page_backup,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Autostart,
        implementation: Page {
            render: page_autostart,
            submit: Some(App::autostart_event),
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Calendar,
        implementation: Page {
            render: page_calendar,
            submit: Some(App::calendar_event),
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Display,
        implementation: Page {
            render: page_display,
            submit: Some(App::display_event),
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: suspend_display,
        },
    },
    Registration {
        key: Screen::Profiles,
        implementation: Page {
            render: page_profiles,
            submit: Some(App::profiles_event),
            early_submit: false,
            input: no_input,
            grid: grid_profiles,
            suspend: suspend_profiles,
        },
    },
    Registration {
        key: Screen::ProfileContent,
        implementation: Page {
            render: page_profilecontent,
            submit: Some(App::profile_content_event),
            early_submit: false,
            input: input_profilecontent,
            grid: grid_profilecontent,
            suspend: suspend_profilecontent,
        },
    },
    Registration {
        key: Screen::ExitDraft,
        implementation: Page {
            render: page_exitdraft,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
    Registration {
        key: Screen::Repair,
        implementation: Page {
            render: page_repair,
            submit: None,
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: no_suspend,
        },
    },
];
pub(super) fn validate() -> std::result::Result<(), RegistrationError> {
    Registry::new(PAGES)?.require(Screen::ALL)
}
pub(super) fn view(app: &App, status: &str) -> View {
    match Registry::new(PAGES).and_then(|registry| registry.get(app.screen)) {
        Ok(page) => (page.render)(app, status),
        Err(_) => View {
            title: "页面注册不可用".into(),
            description: "已停止页面输入，请正常关闭后核对程序。".into(),
            status: String::new(),
            fields: vec![],
            actions: vec![],
            busy: true,
            grid: None,
        },
    }
}
fn field(id: FieldId, label: &str, kind: FieldKind, value: String, max_utf16: usize) -> Field {
    Field {
        id,
        label: label.into(),
        kind,
        value,
        max_utf16,
    }
}
fn action(id: ActionId, label: &str) -> Action {
    Action {
        id,
        label: label.into(),
        enabled: true,
    }
}
fn plain(id: FieldId, label: &str, value: String) -> Field {
    field(id, label, FieldKind::Text, value, 128)
}
fn secret(id: FieldId, label: &str) -> Field {
    field(id, label, FieldKind::Password, String::new(), 256)
}
fn frame(app: &App, status: &str) -> View {
    View {
        title: "未来教室".into(),
        description: String::new(),
        status: status.into(),
        fields: vec![],
        actions: vec![],
        busy: app.pending.is_some() || app.stopping,
        grid: None,
    }
}
fn public_view(app: &App, status: &str) -> Option<View> {
    app.public_lesson_flow.as_ref()?;
    if let Some(editor) = &app.public_lesson_editor {
        return Some(
            editor.view(
                status,
                app.pending.is_some()
                    || app.stopping
                    || app
                        .public_lesson_flow
                        .as_ref()
                        .is_some_and(|flow| flow.guard().is_waiting_for_authority()),
            ),
        );
    }
    Some(View {
        title: "正在读取本节课程".into(),
        description: "本节临时换课仅影响当前这一节。".into(),
        status: status.into(),
        fields: vec![],
        actions: vec![Action {
            id: public_lesson_editor::CLOSE,
            label: "关闭".into(),
            enabled: true,
        }],
        busy: app.pending.is_some() || app.stopping,
        grid: None,
    })
}

fn page_starting(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "正在打开未来教室".into();
    view.description = "正在核对数据和本机凭据，请稍候。".into();
    view
}

fn page_setup(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "首次设置 · 创建主管理员".into();
    view.description = "主管理员管理学校与账号。创建后将展示一次恢复码，请妥善保存。".into();
    view.fields = vec![
        plain(SCHOOL, "学校名称", String::new()),
        plain(LOGIN, "主管理员登录名", String::new()),
        plain(NAME, "姓名", String::new()),
        secret(PASSWORD, "设置口令（12–128字符）"),
        secret(CONFIRM, "再次输入口令"),
    ];
    view.actions = vec![action(SAVE, "创建主管理员")];
    view
}

fn page_login(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    let school = app
        .state
        .installation
        .as_ref()
        .map(|i| i.school_name.as_str())
        .unwrap_or("未配置学校");
    view.title = format!("{school} · 登录工作台");
    view.fields = vec![
        plain(LOGIN, "登录名", String::new()),
        secret(PASSWORD, "口令"),
    ];
    view.actions = vec![
        action(LOG_IN, "登录"),
        action(TO_RECOVERY, "使用恢复码"),
        action(TO_REINITIALIZE, "无恢复码：进入全部资料清除向导"),
    ];
    view
}

fn page_recovery(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "使用恢复码重设口令".into();
    view.description = "恢复成功后，旧恢复码和所有账号会话失效，并展示新的恢复码。".into();
    view.fields = vec![
        plain(LOGIN, "登录名", String::new()),
        field(CODE, "恢复码", FieldKind::Password, String::new(), 128),
        secret(NEW_PASSWORD, "新口令"),
        secret(CONFIRM, "再次输入新口令"),
    ];
    view.actions = vec![
        action(SAVE, "恢复账号"),
        action(BACK, "返回登录"),
        action(TO_REINITIALIZE, "无恢复码：进入全部资料清除向导"),
    ];
    view
}

fn page_code(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "请保存恢复码".into();
    view.description="此恢复码只展示一次。关闭窗口后将不再显示；可登录后重新生成。请将恢复码交给对应账号本人妥善保管。".into();
    view.fields = vec![field(
        CODE,
        "一次性恢复码",
        FieldKind::ReadOnly,
        String::new(),
        128,
    )];
    view.actions = vec![action(ACK, "我已妥善保存")];
    view
}

fn page_home(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    let school = app
        .state
        .installation
        .as_ref()
        .map(|i| i.school_name.as_str())
        .unwrap_or("未配置学校");
    view.title = format!("{school} · 工作台");
    let account = app
        .lease
        .and_then(|l| app.state.account(l.principal_id()).ok());
    view.description = account
        .map(|a| {
            format!(
                "{}（{}）\n关闭工作台会锁定本地会话；今日提醒请在教师网页管理。",
                a.display_name,
                match a.principal.role {
                    Role::GlobalAdmin => "主管理员",
                    Role::TeacherAdmin => "教师管理员",
                    Role::User => "普通用户",
                }
            )
        })
        .unwrap_or_default();
    if account.is_some_and(|a| a.principal.role == Role::GlobalAdmin) {
        view.actions.extend([
            action(TO_SCHOOL, "学校与班级"),
            action(TO_SUBJECTS, "基础科目"),
            action(TO_ACCOUNTS, "账号与任教管理"),
            action(TO_SCHEDULE, "课表、作息与校历"),
            action(TO_DISPLAY, "桌面与系统设置"),
            action(TO_LESSON, "本节临时换课"),
            action(TO_WEB, "教师设备与配对"),
        ]);
    }
    if account.is_some_and(|a| a.principal.role != Role::User) {
        view.actions.push(action(TO_HOMEWORK, "今天的作业"));
        view.actions.push(action(TO_MATERIALS, "课堂资料"));
    }
    view.actions.extend([
        action(TO_PASSWORD, "修改本人口令"),
        action(LOCK, "锁定工作台"),
    ]);
    if account.is_some_and(|a| a.principal.role == Role::GlobalAdmin) {
        view.actions.push(action(EXIT, "退出未来教室"));
    }
    view
}

fn page_school(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    let school = app
        .state
        .installation
        .as_ref()
        .map(|i| i.school_name.as_str())
        .unwrap_or("未配置学校");
    view.title = "学校与班级".into();
    view.description = format!(
        "已有班级：{}",
        app.state
            .classes
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join("、")
    );
    view.fields = vec![
        plain(SCHOOL, "学校名称", school.into()),
        plain(CLASS, "班级名称", String::new()),
    ];
    view.actions = vec![
        action(SAVE, "保存学校名称"),
        action(ADD_CLASS, "添加班级"),
        action(BIND_CLASS, "将本白板绑定此班级"),
        action(BACK, "返回"),
    ];
    view
}

fn page_subjects(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "学校基础科目".into();
    view.description = format!(
                    "已有科目：{}。修改时填写现有名称；改名保留原颜色及引用，颜色请在桌面显示设置的“科目颜色与显示”中修改。",
                    app.state
                        .subjects
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join("、")
                );
    view.fields = vec![
        plain(OLD_NAME, "现有科目（新建时留空）", String::new()),
        plain(SUBJECT, "科目名称", String::new()),
        plain(COLOR, "新建科目颜色（改名时保留原色）", "70A5D8".into()),
    ];
    view.actions = vec![action(SAVE, "保存科目"), action(BACK, "返回")];
    view
}

fn page_accounts(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if app.screen == Screen::Accounts {
        if let Some(editor) = &app.accounts {
            let mut view = editor.view(&app.state);
            if !status.is_empty() {
                if !view.status.is_empty() {
                    view.status.push('\n');
                }
                view.status.push_str(&status);
            }
            view.actions.push(Action {
                id: LOCK,
                label: "锁定".into(),
                enabled: true,
            });
            view.busy = app.pending.is_some() || app.stopping;
            return view;
        }
    }
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "账号与任教管理".into();
    view.description = "正在读取当前账号目录与权限，请稍候。".into();
    view.actions = vec![action(LOCK, "锁定")];
    view
}

fn page_password(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "修改本人口令".into();
    view.description = "首次登录须先修改初始口令。成功后请使用新口令重新登录。".into();
    view.fields = vec![
        secret(PASSWORD, "原口令"),
        secret(NEW_PASSWORD, "新口令"),
        secret(CONFIRM, "再次输入新口令"),
    ];
    view.actions = vec![action(SAVE, "修改口令")];
    if app
        .lease
        .and_then(|lease| app.state.account(lease.principal_id()).ok())
        .is_some_and(|account| !account.must_change_password)
    {
        view.actions.extend([
            action(TO_ROTATE, "重新生成恢复码"),
            action(BACK, "返回工作台"),
        ]);
    }
    view.actions.push(action(LOCK, "锁定并返回登录"));
    view
}

fn page_rotate(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "重新生成恢复码".into();
    view.description = "请输入本人当前口令。保存成功后旧恢复码失效，新码仅展示一次。".into();
    view.fields = vec![secret(PASSWORD, "当前口令")];
    view.actions = vec![action(SAVE, "生成新恢复码"), action(BACK, "返回")];
    view
}

fn page_homework(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "选择今天的作业".into();
    view.description =
        "请输入已有班级、科目和任课教师登录名。教师仅能编辑本人任教范围内的作业。".into();
    let login = app
        .lease
        .and_then(|l| app.state.account(l.principal_id()).ok())
        .map(|a| a.login_name.clone())
        .unwrap_or_default();
    view.fields = vec![
        plain(CLASS, "班级", String::new()),
        plain(SUBJECT, "科目", String::new()),
        plain(OWNER, "任课教师登录名", login),
    ];
    view.actions = vec![action(LOAD, "加载今日作业"), action(BACK, "返回")];
    view
}

fn page_homeworkedit(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "编辑今天的作业".into();
    view.description = "保存空正文即可清空。其他人修改或跨日后，旧草稿不能覆盖新版本。".into();
    view.fields = vec![field(
        CONTENT,
        "作业正文",
        FieldKind::Multiline,
        app.record
            .as_ref()
            .map(|r| r.text.clone())
            .unwrap_or_default(),
        16384,
    )];
    view.actions = vec![action(SAVE, "保存作业"), action(BACK, "返回选择")];
    view
}

fn page_materials(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    if app.screen == Screen::Materials {
        if let Some(editor) = &app.material_editor {
            return editor.view(&app.state, &status, app.pending.is_some() || app.stopping);
        }
    }
    let mut view = frame(app, &status);
    view.title = "课堂资料".into();
    view
}

fn page_lesson(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    if app.screen == Screen::Lesson {
        if let Some(editor) = &app.lesson_editor {
            return editor.view(&app.state, &status, app.pending.is_some() || app.stopping);
        }
    }
    let mut view = frame(app, &status);
    view.title = "正在加载本节课程".into();
    view
}

fn page_voice(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    if app.screen == Screen::Voice {
        if let Some(editor) = &app.voice_editor {
            let mut view = editor.view(&status, app.pending.is_some() || app.stopping);
            view.actions.push(Action {
                id: LOCK,
                label: "锁定".into(),
                enabled: true,
            });
            return view;
        }
    }
    let mut view = frame(app, &status);
    view.title = "远程语音设置".into();
    view.description =
        "尚未取得当前语音设置。后台繁忙或状态已变化时，请重新读取；也可返回工作台。".into();
    view.actions = vec![
        action(voice_editor::REFRESH, "重新读取状态"),
        action(voice_editor::CLOSE, "返回工作台"),
        action(LOCK, "锁定"),
    ];
    view
}

fn page_httpssettings(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    if app.screen == Screen::HttpsSettings {
        if let Some(view) = app.https_view(&status, app.pending.is_some() || app.stopping) {
            return view;
        }
    }
    let mut view = frame(app, &status);
    view.title = "本机 HTTPS 连接配置".into();
    view.description = "正在读取配置；这里不会启动或启用教师连接。".into();
    view.actions = vec![action(LOCK, "锁定工作台")];
    view
}

fn page_web(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    if app.screen == Screen::Web {
        if let Some(editor) = &app.web_editor {
            return editor.view(&app.state, &status, app.pending.is_some() || app.stopping);
        }
    }
    let mut view = frame(app, &status);
    view.title = "正在加载教师设备".into();
    view.description = web_editor::NOT_READY.into();
    view
}

fn page_schedule(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    if app.screen == Screen::Schedule {
        if let Some(editor) = &app.schedule {
            return editor.view(&app.state, &status, app.pending.is_some() || app.stopping);
        }
    }
    let mut view = frame(app, &status);
    view.title = "正在加载课表".into();
    view
}

fn page_backup(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if matches!(app.screen, Screen::Backup | Screen::Restoring) {
        if let Some(view) = app.backup.view(&status) {
            return view;
        }
    }
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "完整备份与恢复".into();
    view.description = "正在核对备份服务，请稍候。".into();
    view
}

fn page_autostart(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    if app.screen == Screen::Autostart {
        if let Some(editor) = &app.autostart {
            let mut view = editor.view(&status, app.autostart_job.is_some() || app.stopping);
            view.actions.push(Action {
                id: LOCK,
                label: "锁定".into(),
                enabled: true,
            });
            return view;
        }
    }
    let mut view = frame(app, &status);
    view.title = "正在读取登录自启动".into();
    view
}

fn page_calendar(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    if app.screen == Screen::Calendar {
        if let Some(editor) = &app.calendar {
            let mut view = editor.view(
                app.pending.is_some() || app.calendar_job.is_some() || app.stopping,
                if status.is_empty() {
                    None
                } else {
                    Some(status.clone())
                },
            );
            view.actions.push(Action {
                id: LOCK,
                label: "锁定".into(),
                enabled: true,
            });
            return view;
        }
    }
    let mut view = frame(app, &status);
    view.title = "正在加载公共校历候选".into();
    view
}

fn page_display(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    if app.screen == Screen::Display {
        if let Some(editor) = &app.display {
            let mut view = editor.view(&status, app.pending.is_some() || app.stopping);
            if view
                .fields
                .iter()
                .any(|field| field.id == display_editor::CLOCK)
            {
                view.actions.push(Action {
                    id: TO_PROFILES,
                    label: "公共布局与科目配置".into(),
                    enabled: true,
                });
                view.actions.push(Action {
                    id: TO_AUTOSTART,
                    label: "登录自启动".into(),
                    enabled: true,
                });
                view.actions.push(Action {
                    id: TO_BACKUP,
                    label: "完整备份与恢复".into(),
                    enabled: true,
                });
                view.actions.push(Action {
                    id: TO_VOICE,
                    label: "远程语音设置".into(),
                    enabled: true,
                });
            }
            return view;
        }
    }
    let mut view = frame(app, &status);
    view.title = "正在加载显示设置".into();
    view
}

fn page_profiles(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if app.screen == Screen::Profiles {
        if let Some(editor) = &app.profiles {
            let mut view = editor.view(&status, app.pending.is_some() || app.stopping);
            if !view.actions.iter().any(|action| action.id == LOCK) {
                view.actions.push(Action {
                    id: LOCK,
                    label: "锁定并保留草稿".into(),
                    enabled: true,
                });
            }
            return view;
        }
    }
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "正在加载布局与科目配置".into();
    view
}

fn page_profilecontent(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if app.screen == Screen::ProfileContent {
        if let Some(editor) = &app.profile_content {
            let mut view = editor.view(app.pending.is_some() || app.stopping);
            if !status.is_empty() {
                if !view.status.is_empty() {
                    view.status.push('\n');
                }
                view.status.push_str(&status);
            }
            view.actions.push(Action {
                id: LOCK,
                label: "锁定并保留草稿".into(),
                enabled: true,
            });
            return view;
        }
    }
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "正在读取独立节点内容".into();
    view
}

fn page_exitdraft(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "退出前处理未保存草稿".into();
    view.description =
        "课表、显示设置、账号、布局或独立内容有未保存修改。退出后，本次运行中的草稿将丢失。".into();
    view.actions = vec![
        action(RETURN_DRAFT, "返回草稿"),
        action(DISCARD_EXIT, "丢弃草稿并退出"),
        action(LOCK, "锁定并保留草稿"),
    ];
    view
}

fn page_repair(app: &App, status: &str) -> View {
    let status = status.to_owned();
    if let Some(view) = public_view(app, &status) {
        return view;
    }
    let mut view = frame(app, &status);
    view.title = "未来教室需要修复".into();
    view.description="数据暂不可用。请保留现有文件，使用原部署Windows账号或受控备份修复；不会重新开放首次管理员注册。".into();
    view.actions = vec![action(TO_REINITIALIZE, "放弃全部本机资料：进入清除向导")];
    view
}

struct Transition {
    next: Screen,
    lease: Option<SessionLease>,
    code: Option<RecoveryCode>,
}
type RequestHandler = fn(&mut Backend, &mut Request, &mut Transition) -> Result<Option<Outcome>>;
static REQUESTS: &[Registration<(Screen, ActionId), RequestHandler>] = &[
    Registration {
        key: (Screen::Setup, SAVE),
        implementation: request_setup_save,
    },
    Registration {
        key: (Screen::Login, LOG_IN),
        implementation: request_login_log_in,
    },
    Registration {
        key: (Screen::Recovery, SAVE),
        implementation: request_recovery_save,
    },
    Registration {
        key: (Screen::Password, SAVE),
        implementation: request_password_save,
    },
    Registration {
        key: (Screen::Rotate, SAVE),
        implementation: request_rotate_save,
    },
    Registration {
        key: (Screen::School, SAVE),
        implementation: request_school_save,
    },
    Registration {
        key: (Screen::School, ADD_CLASS),
        implementation: request_school_add_class,
    },
    Registration {
        key: (Screen::School, BIND_CLASS),
        implementation: request_school_bind_class,
    },
    Registration {
        key: (Screen::Subjects, SAVE),
        implementation: request_subjects_save,
    },
    Registration {
        key: (Screen::Homework, LOAD),
        implementation: request_homework_load,
    },
    Registration {
        key: (Screen::HomeworkEdit, SAVE),
        implementation: request_homeworkedit_save,
    },
    Registration {
        key: (Screen::Starting, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Setup, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Login, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Recovery, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Code, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Home, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::School, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Subjects, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Accounts, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Password, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Rotate, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Homework, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::HomeworkEdit, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Schedule, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Calendar, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Autostart, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Backup, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Restoring, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Display, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Profiles, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::ProfileContent, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Materials, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Lesson, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Voice, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Web, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::HttpsSettings, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::ExitDraft, EXIT),
        implementation: request_any_exit,
    },
    Registration {
        key: (Screen::Repair, EXIT),
        implementation: request_any_exit,
    },
];
fn require_lease(r: &Request) -> Result<SessionLease> {
    r.lease.ok_or_else(|| invalid("请先登录。"))
}
pub(super) fn execute(mut r: Request) -> Result<Outcome> {
    let handler = Registry::new(REQUESTS)
        .and_then(|registry| registry.get((r.screen, r.action)))
        .map_err(|_| invalid("此操作在当前页面不可用。"))?;
    let owner = Arc::clone(&r.backend);
    let mut backend = owner.lock().map_err(|_| Failure::Stopped)?;
    let mut transition = Transition {
        next: Screen::Home,
        lease: r.lease,
        code: None,
    };
    if let Some(outcome) = handler(&mut backend, &mut r, &mut transition)? {
        return Ok(outcome);
    }
    Ok(Outcome::Updated {
        state: backend.state()?,
        lease: transition.lease,
        code: transition.code,
        next: transition.next,
    })
}
fn request_setup_save(
    backend: &mut Backend,
    r: &mut Request,
    transition: &mut Transition,
) -> Result<Option<Outcome>> {
    confirm(&r.data, PASSWORD)?;
    let options = Initialize {
        school_name: text(&r.data, SCHOOL)?,
        login_name: text(&r.data, LOGIN)?,
        display_name: text(&r.data, NAME)?,
    };
    transition.code = Some(backend.initialize(options, password(&mut r.data, PASSWORD)?)?);
    transition.lease = None;
    transition.next = Screen::Code;
    Ok(None)
}
fn request_login_log_in(
    backend: &mut Backend,
    r: &mut Request,
    transition: &mut Transition,
) -> Result<Option<Outcome>> {
    let name = text(&r.data, LOGIN)?;
    let proof = backend.login(&name, password(&mut r.data, PASSWORD)?)?;
    let state = backend.state()?;
    transition.next = if state
        .account(proof.principal_id())
        .map_err(|_| invalid("账号不可用。"))?
        .must_change_password
    {
        Screen::Password
    } else {
        Screen::Home
    };
    transition.lease = Some(proof);
    Ok(None)
}
fn request_recovery_save(
    backend: &mut Backend,
    r: &mut Request,
    transition: &mut Transition,
) -> Result<Option<Outcome>> {
    confirm(&r.data, NEW_PASSWORD)?;
    let login = text(&r.data, LOGIN)?;
    let recovery = RecoveryCode::parse(r.data.take(CODE).ok_or_else(|| invalid("请输入恢复码。"))?)
        .map_err(|_| invalid("恢复码格式不正确。"))?;
    transition.code =
        Some(backend.recover(&login, recovery, password(&mut r.data, NEW_PASSWORD)?)?);
    transition.lease = None;
    transition.next = Screen::Code;
    Ok(None)
}
fn request_password_save(
    backend: &mut Backend,
    r: &mut Request,
    transition: &mut Transition,
) -> Result<Option<Outcome>> {
    confirm(&r.data, NEW_PASSWORD)?;
    let proof = require_lease(r)?;
    let old = password(&mut r.data, PASSWORD)?;
    let new = password(&mut r.data, NEW_PASSWORD)?;
    backend.change_password(proof, old, new)?;
    transition.lease = None;
    transition.next = Screen::Login;
    Ok(None)
}
fn request_rotate_save(
    backend: &mut Backend,
    r: &mut Request,
    transition: &mut Transition,
) -> Result<Option<Outcome>> {
    transition.code =
        Some(backend.rotate_recovery(require_lease(r)?, password(&mut r.data, PASSWORD)?)?);
    transition.lease = None;
    transition.next = Screen::Code;
    Ok(None)
}
fn request_school_save(
    backend: &mut Backend,
    r: &mut Request,
    _transition: &mut Transition,
) -> Result<Option<Outcome>> {
    backend.identity_at(
        require_lease(r)?,
        r.state.revision,
        IdentityCommand::RenameSchool {
            name: text(&r.data, SCHOOL)?,
        },
    )?;
    Ok(None)
}
fn request_school_add_class(
    backend: &mut Backend,
    r: &mut Request,
    _transition: &mut Transition,
) -> Result<Option<Outcome>> {
    let id = ClassId::new(
        r.state
            .classes
            .iter()
            .map(|c| c.id.get())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("班级数量已达上限。"))?,
    )
    .map_err(|_| invalid("班级编号不可用。"))?;
    backend.identity_at(
        require_lease(r)?,
        r.state.revision,
        IdentityCommand::CreateClass {
            id,
            name: text(&r.data, CLASS)?,
        },
    )?;
    Ok(None)
}
fn request_school_bind_class(
    backend: &mut Backend,
    r: &mut Request,
    _transition: &mut Transition,
) -> Result<Option<Outcome>> {
    let class = find_class(&r.state, &text(&r.data, CLASS)?)?;
    backend.identity_at(
        require_lease(r)?,
        r.state.revision,
        IdentityCommand::BindCurrentClass { id: class },
    )?;
    Ok(None)
}
fn request_subjects_save(
    backend: &mut Backend,
    r: &mut Request,
    _transition: &mut Transition,
) -> Result<Option<Outcome>> {
    let old = r.data.get(OLD_NAME).unwrap_or("").trim();
    let existing = if old.is_empty() {
        None
    } else {
        Some(find_subject(&r.state, old)?)
    };
    let id = match existing {
        Some(id) => id,
        None => SubjectId::new(
            r.state
                .subjects
                .iter()
                .map(|s| s.id.get())
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| invalid("科目数量已达上限。"))?,
        )
        .map_err(|_| invalid("科目编号不可用。"))?,
    };
    let expected_revision = existing.and_then(|id| {
        r.state
            .subjects
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.revision)
    });
    let color = if let Some(id) = existing {
        r.state
            .subjects
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| invalid("科目已变化。"))?
            .color
    } else {
        parse_color(r.data.get(COLOR).unwrap_or(""))?
    };
    backend.identity_at(
        require_lease(r)?,
        r.state.revision,
        IdentityCommand::PutSubject {
            id,
            name: text(&r.data, SUBJECT)?,
            enabled: true,
            color,
            expected_revision,
        },
    )?;
    Ok(None)
}
fn request_homework_load(
    backend: &mut Backend,
    r: &mut Request,
    _transition: &mut Transition,
) -> Result<Option<Outcome>> {
    let proof = require_lease(r)?;
    let class = find_class(&r.state, &text(&r.data, CLASS)?)?;
    let subject = find_subject(&r.state, &text(&r.data, SUBJECT)?)?;
    let owner = r
        .state
        .account_by_login(&text(&r.data, OWNER)?)
        .map_err(|_| invalid("请选择已有任课教师的登录名。"))?
        .principal
        .id;
    if !r.state.is_teacher(owner, class, subject) {
        return Err(invalid("该教师没有此班级和科目的有效任教关系。"));
    }
    let key = ContentKey::Homework(HomeworkKey {
        class_id: class,
        subject_id: subject,
        owner_teacher_id: owner,
        actual_date: r
            .clock
            .try_now()
            .map_err(|_| invalid("系统时间不可用。"))?
            .date,
    });
    let record = backend.read_content(proof, key.clone())?;
    Ok(Some(Outcome::Loaded { key, record }))
}
fn request_homeworkedit_save(
    backend: &mut Backend,
    r: &mut Request,
    _transition: &mut Transition,
) -> Result<Option<Outcome>> {
    backend.content(
        require_lease(r)?,
        ContentCommand {
            command_id: command_id()?,
            key: r.key.take().ok_or_else(|| invalid("请重新加载作业。"))?,
            expected_revision: r.revision,
            text: r.data.get(CONTENT).unwrap_or("").to_owned(),
        },
    )?;
    Ok(None)
}
fn request_any_exit(
    backend: &mut Backend,
    r: &mut Request,
    _transition: &mut Transition,
) -> Result<Option<Outcome>> {
    backend.authorize_exit(require_lease(r)?)?;
    Ok(Some(Outcome::ExitAuthorized))
}

fn page(screen: Screen) -> std::result::Result<&'static Page, String> {
    Registry::new(PAGES)
        .and_then(|registry| registry.get(screen))
        .map_err(|_| "页面注册不可用".to_owned())
}
pub(super) fn submit_handler(
    screen: Screen,
    early: bool,
) -> std::result::Result<Option<SubmitHandler>, String> {
    let page = page(screen)?;
    Ok(if page.early_submit == early {
        page.submit
    } else {
        None
    })
}
pub(super) fn input(app: &mut App) -> std::result::Result<(), String> {
    (page(app.screen)?.input)(app)
}
pub(super) fn grid(app: &mut App, row: usize, column: usize) -> std::result::Result<(), String> {
    (page(app.screen)?.grid)(app, row, column)
}
pub(super) fn suspend(app: &mut App) {
    // A broken static page cannot stop the caller's management lock and secret
    // cleanup. This hook only retains the original nonsecret editing draft.
    if let Ok(page) = page(app.screen) {
        (page.suspend)(app);
    }
}
fn no_input(_: &mut App) -> std::result::Result<(), String> {
    Ok(())
}
fn no_grid(_: &mut App, _: usize, _: usize) -> std::result::Result<(), String> {
    Ok(())
}
fn no_suspend(_: &mut App) {}
fn input_schedule(app: &mut App) -> std::result::Result<(), String> {
    if app.screen == Screen::Schedule && app.pending.is_none() {
        if let (Some(editor), Ok(data)) = (&mut app.schedule, app.window.draft_nonsecret()) {
            if editor.input(data, &app.state) {
                app.render()?;
            }
        }
    }
    Ok(())
}
fn input_lesson(app: &mut App) -> std::result::Result<(), String> {
    if app.screen == Screen::Lesson && app.pending.is_none() {
        if let (Some(editor), Ok(data)) = (&mut app.lesson_editor, app.window.draft_nonsecret()) {
            if editor.input(data, &app.state) {
                app.render()?;
            }
        }
    }
    Ok(())
}
fn input_httpssettings(app: &mut App) -> std::result::Result<(), String> {
    if app.screen == Screen::HttpsSettings && app.pending.is_none() {
        if let Ok(data) = app.window.draft_nonsecret() {
            app.https_capture(data);
        }
    }
    Ok(())
}
fn input_web(app: &mut App) -> std::result::Result<(), String> {
    if app.screen == Screen::Web && app.pending.is_none() {
        if let (Some(editor), Ok(data)) = (&mut app.web_editor, app.window.draft_nonsecret()) {
            editor.input(&data);
            app.render()?;
        }
    }
    Ok(())
}
fn input_profilecontent(app: &mut App) -> std::result::Result<(), String> {
    if app.screen == Screen::ProfileContent && app.pending.is_none() {
        if let (Some(editor), Ok(data)) = (&mut app.profile_content, app.window.draft_nonsecret()) {
            if let Err(message) = editor.capture(&data) {
                app.status = message;
            }
        }
    }
    Ok(())
}
fn input_voice(app: &mut App) -> std::result::Result<(), String> {
    if app.screen == Screen::Voice && app.pending.is_none() {
        if let (Some(editor), Ok(data)) = (&mut app.voice_editor, app.window.draft_nonsecret()) {
            if let Err(message) = editor.capture(data) {
                app.status = message;
            }
        }
    }
    Ok(())
}
fn grid_accounts(app: &mut App, row: usize, column: usize) -> std::result::Result<(), String> {
    if app.screen == Screen::Accounts && app.pending.is_none() && !app.stopping {
        if let Some(editor) = &mut app.accounts {
            if let Ok(data) = app.window.draft_nonsecret() {
                editor.suspend(data);
            }
            if let Err(message) = editor.select(row, column, &app.state) {
                app.status = message;
            }
            app.render()?;
        }
    }
    Ok(())
}
fn grid_profiles(app: &mut App, row: usize, column: usize) -> std::result::Result<(), String> {
    if app.screen == Screen::Profiles && app.pending.is_none() && !app.stopping {
        if let Some(editor) = &mut app.profiles {
            if let Ok(data) = app.window.draft_nonsecret() {
                editor.suspend(data);
            }
            if let Err(message) = editor.select(row, column) {
                app.status = message;
            }
            app.render()?;
            app.sync_layout_selection()?;
        }
    }
    Ok(())
}
fn grid_profilecontent(
    app: &mut App,
    row: usize,
    column: usize,
) -> std::result::Result<(), String> {
    if app.screen == Screen::ProfileContent && app.pending.is_none() && !app.stopping {
        if let Some(editor) = &mut app.profile_content {
            if let Err(message) = editor.select(row, column) {
                app.status = message;
            }
            app.render()?;
        }
    }
    Ok(())
}
fn grid_materials(app: &mut App, row: usize, column: usize) -> std::result::Result<(), String> {
    if app.screen == Screen::Materials && app.pending.is_none() && !app.stopping {
        if let Some(editor) = &mut app.material_editor {
            if let Err(message) = editor.select(row, column) {
                app.status = message;
            }
            app.render()?;
        }
    }
    Ok(())
}
fn grid_schedule(app: &mut App, row: usize, column: usize) -> std::result::Result<(), String> {
    if app.screen == Screen::Schedule && app.pending.is_none() && !app.stopping {
        if let Some(editor) = &mut app.schedule {
            if let Err(message) = editor.select_cell(row, column, &app.state) {
                app.status = message;
            }
            app.render()?;
        }
    }
    Ok(())
}
fn suspend_accounts(app: &mut App) {
    if app.screen == Screen::Accounts {
        if let (Some(editor), Ok(data)) = (&mut app.accounts, app.window.draft_nonsecret()) {
            editor.suspend(data);
        }
    }
}
fn suspend_profiles(app: &mut App) {
    if app.screen == Screen::Profiles {
        if let (Some(editor), Ok(data)) = (&mut app.profiles, app.window.draft_nonsecret()) {
            editor.suspend(data);
        }
    }
}
fn suspend_profilecontent(app: &mut App) {
    if app.screen == Screen::ProfileContent {
        if let Some(editor) = &mut app.profile_content {
            if let Ok(data) = app.window.draft_nonsecret() {
                let _ = editor.capture(&data);
            }
            editor.cancel_review();
        }
    }
}
fn suspend_schedule(app: &mut App) {
    if app.screen == Screen::Schedule {
        if let (Some(editor), Ok(data)) = (&mut app.schedule, app.window.draft_nonsecret()) {
            editor.capture(data, &app.state);
        }
    }
}
fn suspend_display(app: &mut App) {
    if app.screen == Screen::Display {
        if let (Some(editor), Ok(data)) = (&mut app.display, app.window.draft_nonsecret()) {
            editor.capture(data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_page_routes_and_cross_page_business_actions_fail_closed() {
        validate().unwrap();
        let requests = Registry::new(REQUESTS).unwrap();
        assert!(requests.get((Screen::Login, SAVE)).is_err());
        assert!(requests.get((Screen::Setup, LOG_IN)).is_err());
        assert!(requests.get((Screen::Home, ADD_CLASS)).is_err());
        assert!(requests.get((Screen::School, ADD_CLASS)).is_ok());
        for screen in Screen::ALL {
            assert!(requests.get((*screen, EXIT)).is_ok());
        }
        // Backup and HTTPS keep the original pre-pending dispatch so their own
        // cancellation/suspension handlers remain reachable during work.
        assert!(submit_handler(Screen::Backup, true).unwrap().is_some());
        assert!(submit_handler(Screen::Backup, false).unwrap().is_none());
        assert!(submit_handler(Screen::Profiles, true).unwrap().is_none());
        assert!(submit_handler(Screen::Profiles, false).unwrap().is_some());
    }
}
