//! Common real startup boundary. Isolated probes call their run_at variants
//! directly; they never acquire the production installation namespace here.
use fc_windows::installer_lifecycle::{self, InstallIdentity, LifecycleError};

pub(crate) fn run(body: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let identity = InstallIdentity::for_current_user().map_err(error_text)?;
    installer_lifecycle::with_startup_lease(&identity, body).map_err(error_text)?
}
fn error_text(error: LifecycleError) -> String {
    match error {
        LifecycleError::Busy => "未来教室正在安装或维护，暂时无法启动。".into(),
        LifecycleError::Pending => "上次安装或维护尚未确认完成，暂时无法启动。".into(),
        _ => "无法安全确认安装状态，请关闭程序后再检查。".into(),
    }
}
