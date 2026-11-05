//! Process-owned update control. HTTP handlers queue bounded work; they never
//! install software or keep a maintenance job dependent on a browser session.
use std::sync::{Arc, Mutex};

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::UpdateTick;

#[derive(Clone)]
pub struct UpdateControl {
    inner: Arc<Mutex<State>>,
    #[cfg(windows)]
    native: Arc<Mutex<native::Native>>,
    #[cfg(windows)]
    network: crate::update_network::NetworkControl,
}
#[derive(Clone)]
pub struct UpdateStatus {
    pub version: String,
    pub automatic_updates: bool,
    pub update_available: bool,
    pub update_message: String,
    pub action_token: String,
    pub phase: String,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub group_id: Option<String>,
}
struct State {
    status: UpdateStatus,
    check: bool,
}
pub struct GroupEnrollment {
    pub group_id: [u8; 16],
    pub token: [u8; 16],
}
impl UpdateControl {
    #[cfg(all(test, not(windows)))]
    pub(crate) fn for_test(status: UpdateStatus) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                status,
                check: false,
            })),
        }
    }
    #[cfg(all(test, not(windows)))]
    pub(crate) fn test_check_requested(&self) -> bool {
        self.inner.lock().unwrap().check
    }
    pub fn status(&self) -> UpdateStatus {
        self.inner
            .lock()
            .expect("update control poisoned")
            .status
            .clone()
    }
    pub fn request_check(&self, token: &str) -> Result<(), String> {
        let mut state = self.inner.lock().map_err(|_| "更新服务不可用。")?;
        if token.is_empty() || token != state.status.action_token {
            return Err("更新请求已失效，请刷新页面。".into());
        }
        state.check = true;
        Ok(())
    }
    #[cfg(not(windows))]
    pub fn prepare_group_create(&self) -> Result<GroupEnrollment, String> {
        Err("仅Windows产品支持更新配置。".into())
    }
    #[cfg(not(windows))]
    pub fn prepare_group_join(&self, _: &str) -> Result<GroupEnrollment, String> {
        Err("仅Windows产品支持更新配置。".into())
    }
    #[cfg(not(windows))]
    pub fn commit_group(&self, _: [u8; 16]) -> Result<(), String> {
        Err("仅Windows产品支持更新配置。".into())
    }
    #[cfg(not(windows))]
    pub fn discard_group(&self, _: [u8; 16]) -> Result<(), String> {
        Ok(())
    }
    #[cfg(not(windows))]
    pub fn export_group(&self) -> Result<String, String> {
        Err("仅Windows产品支持更新配置。".into())
    }
    #[cfg(not(windows))]
    pub fn begin_import(&self, _: u64) -> Result<String, String> {
        Err("仅Windows产品支持更新导入。".into())
    }
    #[cfg(not(windows))]
    pub fn append_import(&self, _: &str, _: u64, _: &[u8]) -> Result<(), String> {
        Err("仅Windows产品支持更新导入。".into())
    }
    #[cfg(not(windows))]
    pub fn finish_import(&self, _: &str) -> Result<(), String> {
        Err("仅Windows产品支持更新导入。".into())
    }
    #[cfg(not(windows))]
    pub fn cancel_import(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}
#[cfg(windows)]
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
#[cfg(windows)]
fn unhex<const N: usize>(text: &str) -> Result<[u8; N], String> {
    if text.len() != N * 2
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("格式不正确。".into());
    }
    let mut out = [0; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| "格式不正确。")?;
    }
    Ok(out)
}
