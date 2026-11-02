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
            submit: Some(App::subjects_event),
            early_submit: false,
            input: no_input,
            grid: no_grid,
            suspend: suspend_subjects,
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
    if let Some(view) = public_view(app, status) {
        return view;
    }
    if let Some(editor) = &app.subject_editor {
        let mut view = editor.view(&app.state, status);
        view.busy = app.pending.is_some() || app.stopping;
        return view;
    }
    let mut view = frame(app, status);
    view.title = "学校基础科目".into();
    view.description = "正在读取科目状态与引用，请稍候。".into();
    view.actions = vec![action(LOCK, "锁定")];
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
type RequestHandler = fn(
    &mut Backend,
    &mut Request,
    &mut Transition,
    &NativeCommand,
    &JobContext,
) -> Result<Option<Outcome>>;
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
pub(super) fn execute(
    mut r: Request,
    command: &NativeCommand,
    context: &JobContext,
) -> Result<Outcome> {
    let handler = Registry::new(REQUESTS)
        .and_then(|registry| registry.get((r.screen, r.action)))
        .map_err(|_| invalid("此操作在当前页面不可用。"))?;
    let owner = Arc::clone(&r.backend);
    let mut backend = owner.lock().map_err(|_| Failure::Stopped)?;
    let family = match (r.screen, r.action) {
        (Screen::Setup | Screen::Password | Screen::Rotate | Screen::School, SAVE)
        | (Screen::School, ADD_CLASS | BIND_CLASS) => {
            Some(fc_ports::command_window::CommandFamily::Identity)
        }
        (Screen::HomeworkEdit, SAVE) => Some(fc_ports::command_window::CommandFamily::Content),
        _ => None,
    };
    let purpose = if r.screen == Screen::Password {
        SessionPurpose::ChangeInitialPassword
    } else {
        SessionPurpose::Manage
    };
    let lease = r.lease;
    let mut execute = |backend: &mut Backend| {
        let mut transition = Transition {
            next: Screen::Home,
            lease: r.lease,
            code: None,
        };
        if let Some(outcome) = handler(backend, &mut r, &mut transition, command, context)? {
            return Ok(outcome);
        }
        Ok(Outcome::Updated {
            state: backend.state()?,
            lease: transition.lease,
            code: transition.code,
            next: transition.next,
        })
    };
    if let Some(family) = family {
        command.run(&mut backend, family, lease, purpose, context, execute)
    } else {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        let mut scoped = backend.begin_execution_scope(Arc::new(context.execution_fence()));
        execute(&mut scoped)
    }
}

fn request_setup_save(
    backend: &mut Backend,
    r: &mut Request,
    transition: &mut Transition,
    _command: &NativeCommand,
    _context: &JobContext,
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
    _command: &NativeCommand,
    _context: &JobContext,
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
    command: &NativeCommand,
    context: &JobContext,
) -> Result<Option<Outcome>> {
    confirm(&r.data, NEW_PASSWORD)?;
    let login = text(&r.data, LOGIN)?;
    let recovery = RecoveryCode::parse(r.data.take(CODE).ok_or_else(|| invalid("请输入恢复码。"))?)
        .map_err(|_| invalid("恢复码格式不正确。"))?;
    transition.code = Some(backend.recover_window(
        &login,
        recovery,
        password(&mut r.data, NEW_PASSWORD)?,
        |issued| context.checkpoint().is_ok() && command.bind(issued).is_ok(),
    )?);
    transition.lease = None;
    transition.next = Screen::Code;
    Ok(None)
}
fn request_password_save(
    backend: &mut Backend,
    r: &mut Request,
    transition: &mut Transition,
    _command: &NativeCommand,
    _context: &JobContext,
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
    _command: &NativeCommand,
    _context: &JobContext,
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
    _command: &NativeCommand,
    _context: &JobContext,
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
    _command: &NativeCommand,
    _context: &JobContext,
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
    _command: &NativeCommand,
    _context: &JobContext,
) -> Result<Option<Outcome>> {
    let class = find_class(&r.state, &text(&r.data, CLASS)?)?;
    backend.identity_at(
        require_lease(r)?,
        r.state.revision,
        IdentityCommand::BindCurrentClass { id: class },
    )?;
    Ok(None)
}

fn request_homework_load(
    backend: &mut Backend,
    r: &mut Request,
    _transition: &mut Transition,
    _command: &NativeCommand,
    _context: &JobContext,
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
    _command: &NativeCommand,
    _context: &JobContext,
) -> Result<Option<Outcome>> {
    backend.content(
        require_lease(r)?,
        ContentCommand {
            command_id: backend.issued_command_id()?,
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
    _command: &NativeCommand,
    _context: &JobContext,
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
fn suspend_subjects(app: &mut App) {
    if app.screen == Screen::Subjects {
        if let (Some(editor), Ok(data)) = (&mut app.subject_editor, app.window.draft_nonsecret()) {
            editor.suspend(&data);
        }
    }
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

pub(super) const SUBJECT_SELECT: FieldId = FieldId(1100);
pub(super) const SUBJECT_ENABLED: FieldId = FieldId(1101);
pub(super) const SUBJECT_NEW: ActionId = ActionId(1100);
pub(super) const SUBJECT_EDIT: ActionId = ActionId(1101);
pub(super) const SUBJECT_REVIEW: ActionId = ActionId(1102);
pub(super) const SUBJECT_CONFIRM: ActionId = ActionId(1103);
pub(super) const SUBJECT_CANCEL: ActionId = ActionId(1104);
pub(super) const SUBJECT_RETURN: ActionId = ActionId(1105);
pub(super) const SUBJECT_RELOAD: ActionId = ActionId(1106);
#[derive(Clone)]
pub(super) struct SubjectFacts {
    pub(super) state: IdentityState,
    course_cells: Vec<(SubjectId, usize)>,
    profiles: Vec<(SubjectId, usize)>,
}
pub(super) fn load_subjects(
    backend: &Backend,
    lease: SessionLease,
    context: &JobContext,
) -> Result<SubjectFacts> {
    let state = backend.management_identity(lease)?;
    let schedule = backend.read_schedule(lease)?;
    let mut course_cells = Vec::<(SubjectId, usize)>::new();
    for plan in &schedule.semesters {
        for table in plan
            .semester
            .tables
            .iter()
            .chain(plan.dual_history.iter().flat_map(|h| &h.tables))
        {
            for cell in table
                .rows
                .iter()
                .flat_map(|row| row.courses.iter().flatten())
            {
                if let Some((_, count)) = course_cells
                    .iter_mut()
                    .find(|(id, _)| *id == cell.subject_id)
                {
                    *count += 1;
                } else {
                    course_cells.push((cell.subject_id, 1));
                }
            }
        }
    }
    let mut profiles = Vec::<(SubjectId, usize)>::new();
    let mut after = None;
    let mut total = 0usize;
    loop {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        let page = backend.list_subject_profiles(lease, after)?;
        total = total
            .checked_add(page.items.len())
            .ok_or(Failure::Stopped)?;
        if total > fc_domain::profile::MAX_SUBJECT_PROFILES {
            return Err(Failure::Stopped);
        }
        for profile in &page.items {
            if let Some((_, count)) = profiles
                .iter_mut()
                .find(|(id, _)| *id == profile.key.subject_id)
            {
                *count += 1;
            } else {
                profiles.push((profile.key.subject_id, 1));
            }
        }
        match page.next_after {
            Some(next) if Some(next) > after && page.items.last().is_some_and(|p| p.id == next) => {
                after = Some(next)
            }
            None => break,
            _ => return Err(Failure::Stopped),
        }
    }
    if backend.management_identity(lease)?.revision != state.revision {
        return Err(invalid("科目目录已更新，请重新读取。"));
    }
    Ok(SubjectFacts {
        state,
        course_cells,
        profiles,
    })
}
#[derive(Clone)]
struct SubjectDraft {
    id: SubjectId,
    expected_subject: Option<Revision>,
    name: String,
    enabled: bool,
    color: Rgb,
}
pub(super) struct SubjectEditor {
    owner: PrincipalId,
    expected: Revision,
    facts: SubjectFacts,
    draft: Option<SubjectDraft>,
    reviewing: bool,
    submitted: bool,
}
enum SubjectCommand {
    Stay,
    Close,
    Reload,
    Save {
        expected: Revision,
        command: IdentityCommand,
    },
}
impl SubjectEditor {
    fn new(owner: PrincipalId, facts: SubjectFacts) -> Self {
        Self {
            owner,
            expected: facts.state.revision,
            facts,
            draft: None,
            reviewing: false,
            submitted: false,
        }
    }
    pub(super) fn owner(&self) -> PrincipalId {
        self.owner
    }
    pub(super) fn has_unsaved_changes(&self) -> bool {
        self.draft.is_some()
    }
    fn capture(&mut self, data: &FormData) -> std::result::Result<(), String> {
        let draft = self.draft.as_mut().ok_or("请选择科目或新建科目。")?;
        draft.name = data.get(SUBJECT).unwrap_or(&draft.name).trim().into();
        draft.enabled = match data.get(SUBJECT_ENABLED) {
            Some("1") => true,
            Some("0") => false,
            None => draft.enabled,
            _ => return Err("请选择启用或停用。".into()),
        };
        if draft.expected_subject.is_none() && data.get(COLOR).is_some() {
            draft.color =
                parse_color(data.get(COLOR).unwrap_or("")).map_err(|_| "请输入六位新科目颜色。")?;
        }
        Ok(())
    }
    fn suspend(&mut self, data: &FormData) {
        if !self.submitted {
            let _ = self.capture(data);
            self.reviewing = false;
        }
    }
    fn rejected(&mut self) {
        self.submitted = false;
        self.reviewing = false;
    }
    fn handle(
        &mut self,
        action: ActionId,
        data: &FormData,
    ) -> std::result::Result<SubjectCommand, String> {
        if action == SUBJECT_RELOAD {
            return Ok(SubjectCommand::Reload);
        }
        if action == BACK && self.draft.is_none() {
            return Ok(SubjectCommand::Close);
        }
        if self.submitted {
            return Err("正在核对原保存结果，请重新读取；不会重复保存。".into());
        }
        match action {
            SUBJECT_NEW if self.draft.is_none() => {
                let next = self
                    .facts
                    .state
                    .subjects
                    .iter()
                    .map(|s| s.id.get())
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or("科目编号已用尽。")?;
                self.draft = Some(SubjectDraft {
                    id: SubjectId::new(next).map_err(|_| "科目编号不可用。")?,
                    expected_subject: None,
                    name: String::new(),
                    enabled: true,
                    color: Rgb {
                        red: 0x70,
                        green: 0xA5,
                        blue: 0xD8,
                    },
                });
            }
            SUBJECT_EDIT if self.draft.is_none() => {
                let value = data.get(SUBJECT_SELECT).unwrap_or("");
                let id = value
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| SubjectId::new(n).ok())
                    .ok_or("请选择已有科目。")?;
                let subject = self
                    .facts
                    .state
                    .subjects
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or("所选科目已变化，请重新读取。")?;
                self.draft = Some(SubjectDraft {
                    id,
                    expected_subject: Some(subject.revision),
                    name: subject.name.clone(),
                    enabled: subject.enabled,
                    color: subject.color,
                });
            }
            SUBJECT_REVIEW if self.draft.is_some() && !self.reviewing => {
                self.capture(data)?;
                let draft = self.draft.as_ref().unwrap();
                let normalized = normalized_name(&draft.name, 64)
                    .map_err(|_| "请输入不超过64字的有效科目名称。")?;
                if self.facts.state.subjects.iter().any(|s| {
                    s.id != draft.id
                        && normalized_name(&s.name, 64).ok().as_ref() == Some(&normalized)
                }) {
                    return Err("已有同名科目，请选择原科目进行修改。".into());
                }
                self.reviewing = true;
            }
            SUBJECT_CONFIRM if self.reviewing => {
                let draft = self.draft.as_ref().ok_or("修改摘要已失效。")?;
                self.submitted = true;
                return Ok(SubjectCommand::Save {
                    expected: self.expected,
                    command: IdentityCommand::PutSubject {
                        id: draft.id,
                        name: draft.name.clone(),
                        enabled: draft.enabled,
                        color: draft.color,
                        expected_revision: draft.expected_subject,
                    },
                });
            }
            SUBJECT_RETURN if self.reviewing => self.reviewing = false,
            SUBJECT_CANCEL if self.draft.is_some() => {
                self.draft = None;
                self.reviewing = false;
            }
            _ => return Err("此操作在当前科目页面不可用。".into()),
        }
        Ok(SubjectCommand::Stay)
    }
    fn view(&self, current: &IdentityState, status: &str) -> View {
        let mut view = View {
            title: "学校基础科目".into(),
            description: String::new(),
            status: status.into(),
            fields: vec![],
            actions: vec![],
            grid: None,
            busy: false,
        };
        if let Some(draft) = &self.draft {
            let old = self.facts.state.subjects.iter().find(|s| s.id == draft.id);
            let state = |enabled| if enabled { "启用" } else { "停用" };
            view.description = format!(
                "科目 #{}：{} → {}；名称：{} → {}。{}",
                draft.id.get(),
                old.map_or("新建", |s| state(s.enabled)),
                state(draft.enabled),
                old.map_or("（新建）", |s| s.name.as_str()),
                draft.name,
                if old.is_some() {
                    "颜色保持原值，改色请进入桌面显示设置。"
                } else {
                    "新建后可在桌面显示设置中调整科目颜色。"
                }
            );
            if self.reviewing || self.submitted {
                let courses = self
                    .facts
                    .course_cells
                    .iter()
                    .find(|(id, _)| *id == draft.id)
                    .map_or(0, |(_, count)| *count);
                let profiles = self
                    .facts
                    .profiles
                    .iter()
                    .find(|(id, _)| *id == draft.id)
                    .map_or(0, |(_, count)| *count);
                let assignments = self
                    .facts
                    .state
                    .assignments
                    .iter()
                    .filter(|a| a.subject_id == draft.id)
                    .count();
                let defaults = self
                    .facts
                    .state
                    .default_teachers
                    .iter()
                    .filter(|d| d.subject_id == draft.id)
                    .count();
                view.title = "确认科目修改".into();
                view.description.push_str(&format!("\n已读取的引用：课表{courses}格（含保留双表）、任教{assignments}项、默认老师{defaults}项、科目布局{profiles}份。课表、任教记录、布局、课堂资料索引和原件均保留。"));
                if !draft.enabled && courses > 0 {
                    view.description.push_str("\n课表仍有引用时保存会被拒绝，请先调整课表和保留双表；不会自动删除或替换课程。");
                }
                if old.is_some_and(|s| s.enabled != draft.enabled) {
                    view.description.push_str("\n启用状态变化后，相关老师原会话失效；当前课堂与临时换课会重新核对有效性。");
                    if !draft.enabled {
                        view.description.push_str("停用将解除当前默认老师绑定，科目停止参与有效选课；重新启用后请重新选择默认老师。");
                    } else {
                        view.description.push_str(
                            "重新启用保留原任教关系和资料；原默认老师绑定不会自动恢复，可在账号与任教管理中重新选择。",
                        );
                    }
                }
                view.description
                    .push_str("\n确认后保存以上更改；如果设置已被更新，请重新读取并核对。");
                if !self.submitted {
                    view.actions = vec![
                        action(SUBJECT_CONFIRM, "确认保存"),
                        action(SUBJECT_RETURN, "继续修改"),
                        action(SUBJECT_CANCEL, "取消本次修改"),
                    ];
                }
            } else {
                view.fields = vec![
                    plain(SUBJECT, "科目名称", draft.name.clone()),
                    field(
                        SUBJECT_ENABLED,
                        "启用状态",
                        FieldKind::Choice(vec![
                            Choice {
                                value: "1".into(),
                                label: "启用".into(),
                            },
                            Choice {
                                value: "0".into(),
                                label: "停用".into(),
                            },
                        ]),
                        if draft.enabled { "1" } else { "0" }.into(),
                        1,
                    ),
                    field(
                        COLOR,
                        if draft.expected_subject.is_some() {
                            "原科目颜色（保留）"
                        } else {
                            "新科目颜色"
                        },
                        if draft.expected_subject.is_some() {
                            FieldKind::ReadOnly
                        } else {
                            FieldKind::Text
                        },
                        format!(
                            "{:02X}{:02X}{:02X}",
                            draft.color.red, draft.color.green, draft.color.blue
                        ),
                        6,
                    ),
                ];
                view.actions = vec![
                    action(SUBJECT_REVIEW, "查看修改摘要"),
                    action(SUBJECT_CANCEL, "取消本次修改"),
                ];
            }
        } else {
            view.description =
                "选择已有科目可改名、停用或重新启用。停用保留历史和原件；科目编号始终不变。".into();
            view.fields = vec![field(
                SUBJECT_SELECT,
                "已有科目",
                FieldKind::Choice(
                    current
                        .subjects
                        .iter()
                        .map(|s| Choice {
                            value: s.id.get().to_string(),
                            label: format!(
                                "{} · #{} · {}",
                                s.name,
                                s.id.get(),
                                if s.enabled { "启用" } else { "停用" }
                            ),
                        })
                        .collect(),
                ),
                current
                    .subjects
                    .first()
                    .map_or(String::new(), |s| s.id.get().to_string()),
                20,
            )];
            view.actions = vec![
                action(SUBJECT_NEW, "新建科目"),
                Action {
                    id: SUBJECT_EDIT,
                    label: "修改所选科目".into(),
                    enabled: !current.subjects.is_empty(),
                },
                action(BACK, "返回"),
            ];
        }
        view.actions.push(action(
            SUBJECT_RELOAD,
            if self.submitted {
                "核对原保存结果"
            } else if self.draft.is_some() {
                "放弃草稿并重新读取"
            } else {
                "重新读取"
            },
        ));
        view.actions.push(action(LOCK, "锁定"));
        view
    }
}
impl App {
    pub(super) fn open_subjects(&mut self) -> std::result::Result<(), String> {
        let Some(lease) = self.lease else {
            return self.render();
        };
        if self.current_global_owner() != Some(lease.principal_id())
            || self
                .control
                .as_ref()
                .is_none_or(SessionControl::is_local_locked)
        {
            self.status = "科目管理仅限已解锁的本地主管理员。".into();
            return self.render();
        }
        if self
            .subject_editor
            .as_ref()
            .is_some_and(|e| e.owner != lease.principal_id())
        {
            self.subject_editor = None;
        }
        self.screen = Screen::Subjects;
        self.status.clear();
        if let Some(backend) = &self.backend {
            self.submit(Work::LoadSubjects(backend.clone(), lease))?;
        }
        Ok(())
    }
    pub(super) fn subjects_event(
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
        {
            self.lock();
            return self.render();
        }
        let Some(editor) = &mut self.subject_editor else {
            return self.render();
        };
        if editor.owner != lease.principal_id() {
            self.subject_editor = None;
            self.screen = Screen::Home;
            return self.render();
        }
        match editor.handle(action, &data) {
            Ok(SubjectCommand::Stay) => self.status.clear(),
            Ok(SubjectCommand::Close) => {
                self.screen = Screen::Home;
                self.status.clear();
            }
            Ok(SubjectCommand::Reload) => {
                if !self.native_unknown {
                    self.subject_editor = None;
                }
                if let Some(backend) = &self.backend {
                    self.submit(Work::LoadSubjects(backend.clone(), lease))?;
                }
            }
            Ok(SubjectCommand::Save { expected, command }) => {
                if let Some(backend) = &self.backend {
                    self.submit(Work::SaveSubject(backend.clone(), lease, expected, command))?;
                }
                if self.pending.is_none() {
                    if let Some(editor) = &mut self.subject_editor {
                        editor.rejected();
                    }
                }
            }
            Err(message) => self.status = message,
        }
        self.render()
    }
    pub(super) fn subjects_loaded(
        &mut self,
        facts: SubjectFacts,
        history: Option<Box<NativeHistory>>,
        saved: bool,
    ) -> std::result::Result<(), String> {
        let pending = history.as_ref().is_some_and(|h| h.pending());
        let rejected = history
            .as_ref()
            .is_some_and(|h| matches!(h.resolution, NativeResolution::SubjectRejected(_)));
        let message = history
            .as_ref()
            .map(|h| match &h.resolution {
                NativeResolution::Subject(Some((_, subject))) => format!(
                    "原保存结果：科目 #{}“{}”为{}。当前列表另行显示。",
                    subject.id.get(),
                    subject.name,
                    if subject.enabled { "启用" } else { "停用" }
                ),
                _ => h.message().into(),
            })
            .unwrap_or_default();
        self.state = Arc::new(facts.state.clone());
        if let Some(owner) = self.current_global_owner() {
            if rejected {
                if let Some(editor) = &mut self.subject_editor {
                    editor.rejected();
                }
            } else if saved
                || (!pending && history.is_some())
                || self
                    .subject_editor
                    .as_ref()
                    .is_none_or(|e| e.owner != owner || !e.has_unsaved_changes())
            {
                self.subject_editor = Some(SubjectEditor::new(owner, facts));
            }
            self.screen = Screen::Subjects;
            self.status = message;
        }
        self.render()
    }
}

#[cfg(test)]
mod subject_editor_tests {
    use super::*;
    fn facts() -> SubjectFacts {
        let mut state = IdentityState {
            revision: Revision::new(17),
            ..IdentityState::default()
        };
        state.subjects.push(SchoolSubject {
            id: SubjectId::new(42).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            name: "合成停用科目".into(),
            enabled: false,
            color: Rgb {
                red: 7,
                green: 19,
                blue: 31,
            },
            revision: Revision::new(9),
        });
        SubjectFacts {
            state,
            course_cells: vec![],
            profiles: vec![],
        }
    }
    fn form(items: &[(FieldId, &str)]) -> FormData {
        let mut data = FormData::new();
        for (id, value) in items {
            data.insert(*id, (*value).into()).unwrap();
        }
        data
    }
    #[test]
    fn disabled_subject_rename_uses_stable_id_preserves_status_color_and_original_cas_until_confirm(
    ) {
        let facts = facts();
        let before = facts.state.clone();
        let mut editor = SubjectEditor::new(PrincipalId::new(1).unwrap(), facts);
        assert!(matches!(
            editor.handle(SUBJECT_EDIT, &form(&[(SUBJECT_SELECT, "42")])),
            Ok(SubjectCommand::Stay)
        ));
        assert!(matches!(
            editor.handle(
                SUBJECT_REVIEW,
                &form(&[(SUBJECT, "合成停用改名"), (COLOR, "FFFFFF")])
            ),
            Ok(SubjectCommand::Stay)
        ));
        editor.handle(SUBJECT_RETURN, &FormData::new()).unwrap();
        editor.handle(SUBJECT_CANCEL, &FormData::new()).unwrap();
        assert!(editor.draft.is_none());
        assert_eq!(editor.facts.state, before);
        editor
            .handle(SUBJECT_EDIT, &form(&[(SUBJECT_SELECT, "42")]))
            .unwrap();
        editor
            .handle(SUBJECT_REVIEW, &form(&[(SUBJECT, "合成停用改名")]))
            .unwrap();
        let saved = editor
            .handle(
                SUBJECT_CONFIRM,
                &form(&[(SUBJECT_ENABLED, "1"), (SUBJECT, "伪造确认内容")]),
            )
            .unwrap();
        assert!(
            matches!(saved, SubjectCommand::Save { expected, command: IdentityCommand::PutSubject { id, name, enabled: false, color, expected_revision: Some(revision) } }
            if expected == Revision::new(17) && id.get() == 42 && name == "合成停用改名" && color == before.subjects[0].color && revision == Revision::new(9))
        );
        assert!(
            editor.handle(SUBJECT_CONFIRM, &FormData::new()).is_err(),
            "confirmed command is not duplicated"
        );
        assert_eq!(
            editor.facts.state, before,
            "editor never mutates durable/current authority"
        );
    }
}
