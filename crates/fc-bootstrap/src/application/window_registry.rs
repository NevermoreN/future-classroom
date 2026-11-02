//! Production window composition. Native registrations own creation, binding,
//! measurement, captured input and disposal; this layer selects only the role.
use super::*;
use fc_presentation::{
    registry::{Registration, Registry},
    workbench::WindowRole,
};
static WINDOWS: &[Registration<Screen, WindowRole>] = &[
    Registration {
        key: Screen::Starting,
        implementation: WindowRole::Status,
    },
    Registration {
        key: Screen::Setup,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Login,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Recovery,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Code,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Home,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::School,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Subjects,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Accounts,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Password,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Rotate,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Homework,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::HomeworkEdit,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Schedule,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Calendar,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Autostart,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Backup,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Restoring,
        implementation: WindowRole::Status,
    },
    Registration {
        key: Screen::Display,
        implementation: WindowRole::LayoutEditor,
    },
    Registration {
        key: Screen::Profiles,
        implementation: WindowRole::LayoutEditor,
    },
    Registration {
        key: Screen::ProfileContent,
        implementation: WindowRole::LayoutEditor,
    },
    Registration {
        key: Screen::Materials,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Lesson,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Voice,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Web,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::HttpsSettings,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::ExitDraft,
        implementation: WindowRole::Workbench,
    },
    Registration {
        key: Screen::Repair,
        implementation: WindowRole::Status,
    },
];
pub(super) fn create_workbench() -> std::result::Result<Window, String> {
    let registry = Registry::new(WINDOWS).map_err(|_| "窗口注册不完整或重复".to_owned())?;
    registry
        .require(Screen::ALL)
        .map_err(|_| "窗口注册不完整或重复".to_owned())?;
    Window::new().map_err(|_| "无法创建工作台".to_owned())
}
pub(super) fn render(
    window: &Window,
    screen: Screen,
    view: &View,
) -> std::result::Result<(), String> {
    let role = Registry::new(WINDOWS)
        .and_then(|registry| registry.get(screen))
        .map_err(|_| "窗口注册不完整或重复".to_owned())?;
    window
        .render_for(*role, view)
        .map_err(|_| "工作台渲染失败".to_owned())
}
