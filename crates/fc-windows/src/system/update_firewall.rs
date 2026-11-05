//! Product-owned update ingress only. Never modifies firewall profiles, SSH,
//! unrelated rules or the existing teacher/discovery rules. Win7 Firewall COM.
use sha2::{Digest, Sha256};
use windows::{
    core::{Result, BSTR},
    Win32::{Foundation::*, NetworkManagement::WindowsFirewall::*, System::Com::*},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateFirewallStatus {
    Ready,
    NeedsAdministrator,
    PrivateNetworkRequired,
    Restricted,
}
impl UpdateFirewallStatus {
    pub fn can_share(self) -> bool {
        self == Self::Ready
    }
    pub fn message(self) -> &'static str {
        match self {
        Self::Ready=>"已准备本程序的内网更新防火墙规则。",
        Self::NeedsAdministrator=>"更新分享受限：请管理员为本程序运行 Enable-ClassroomLAN.ps1，开放专用/域网络的更新端口 8445。",
        Self::PrivateNetworkRequired=>"更新分享受限：当前网络为公用网络，请学校管理员确认校园网的专用/域网络配置。",
        Self::Restricted=>"更新分享受限：学校防火墙策略或本程序的更新规则尚未就绪，请管理员检查部署配置。",
    }
    }
}
struct Apartment(bool);
impl Apartment {
    fn open() -> Result<Self> {
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if status == RPC_E_CHANGED_MODE {
            Ok(Self(false))
        } else {
            status.ok()?;
            Ok(Self(true))
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() }
        }
    }
}
const REMOTE: &str = "10.0.0.0/8,172.16.0.0/12,192.168.0.0/16";
fn remote_matches(value: &str) -> bool {
    let mut values: Vec<_> = value
        .split(',')
        .map(|value| match value.trim() {
            "10.0.0.0/255.0.0.0" => "10.0.0.0/8",
            "172.16.0.0/255.240.0.0" => "172.16.0.0/12",
            "192.168.0.0/255.255.0.0" => "192.168.0.0/16",
            value => value,
        })
        .collect();
    values.sort_unstable();
    values == ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"]
}
fn matches(rule: &INetFwRule, program: &str) -> Result<bool> {
    // COM interfaces and returned BSTRs stay on this synchronous worker. No
    // borrowed interface or apartment owner escapes the operation.
    unsafe {
        Ok(rule
            .ApplicationName()?
            .to_string()
            .eq_ignore_ascii_case(program)
            && rule.Protocol()? == 6
            && rule.LocalPorts()? == "8445"
            && remote_matches(&rule.RemoteAddresses()?.to_string())
            && rule.Profiles()? == 3
            && rule.Direction()? == NET_FW_RULE_DIR_IN
            && rule.Action()? == NET_FW_ACTION_ALLOW
            && rule.Enabled()?.0 != 0
            && rule.ServiceName()?.is_empty()
            && rule.RemotePorts()? == "*"
            && rule.LocalAddresses()? == "*"
            && rule.EdgeTraversal()?.0 == 0)
    }
}
fn prepare() -> Result<UpdateFirewallStatus> {
    let program =
        std::env::current_exe().map_err(|_| windows::core::Error::from_hresult(E_FAIL))?;
    if program
        .file_name()
        .is_none_or(|name| name != "fc-bootstrap.exe")
    {
        return Ok(UpdateFirewallStatus::Restricted);
    }
    let program = program
        .to_str()
        .ok_or_else(|| windows::core::Error::from_hresult(E_INVALIDARG))?;
    let digest = Sha256::digest(program.to_lowercase().as_bytes());
    let suffix: String = digest[..6]
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    let name = BSTR::from(format!("FutureClassroom-LAN-{suffix}-Update"));
    let _apartment = Apartment::open()?;
    let policy: INetFwPolicy2 =
        unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }?;
    let rules = unsafe { policy.Rules() }?;
    match unsafe { rules.Item(&name) } {
        Ok(rule) => {
            if !matches(&rule, program)? {
                return Ok(UpdateFirewallStatus::Restricted);
            }
        }
        Err(error) if error.code() == HRESULT_FROM_WIN32_FILE_NOT_FOUND => {
            let rule: INetFwRule =
                unsafe { CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER) }?;
            unsafe {
                rule.SetName(&name)?;
                rule.SetDescription(&BSTR::from(
                    "FutureClassroom school update distribution; private/domain RFC1918 only",
                ))?;
                rule.SetApplicationName(&BSTR::from(program))?;
                rule.SetProtocol(6)?;
                rule.SetLocalPorts(&BSTR::from("8445"))?;
                rule.SetRemotePorts(&BSTR::from("*"))?;
                rule.SetLocalAddresses(&BSTR::from("*"))?;
                rule.SetRemoteAddresses(&BSTR::from(REMOTE))?;
                rule.SetProfiles(3)?;
                rule.SetDirection(NET_FW_RULE_DIR_IN)?;
                rule.SetAction(NET_FW_ACTION_ALLOW)?;
                rule.SetEnabled(VARIANT_TRUE)?;
                rule.SetEdgeTraversal(VARIANT_FALSE)?;
                if let Err(error) = rules.Add(&rule) {
                    if error.code() == E_ACCESSDENIED {
                        return Ok(UpdateFirewallStatus::NeedsAdministrator);
                    }
                    return Err(error);
                }
            }
            if !matches(&unsafe { rules.Item(&name) }?, program)? {
                return Ok(UpdateFirewallStatus::Restricted);
            }
        }
        Err(error) => return Err(error),
    }
    unsafe {
        let current = policy.CurrentProfileTypes()?;
        if current & 3 == 0 {
            return Ok(UpdateFirewallStatus::PrivateNetworkRequired);
        }
        if policy.LocalPolicyModifyState()? != NET_FW_MODIFY_STATE_OK
            || current & 1 != 0 && policy.get_BlockAllInboundTraffic(NET_FW_PROFILE2_DOMAIN)?.0 != 0
            || current & 2 != 0
                && policy
                    .get_BlockAllInboundTraffic(NET_FW_PROFILE2_PRIVATE)?
                    .0
                    != 0
        {
            return Ok(UpdateFirewallStatus::Restricted);
        }
    }
    Ok(UpdateFirewallStatus::Ready)
}
const HRESULT_FROM_WIN32_FILE_NOT_FOUND: windows::core::HRESULT =
    windows::core::HRESULT(0x80070002u32 as i32);
/// Try to add only this executable's missing exact update rule. Standard-user
/// access denial is a disclosed restriction; no UAC bypass or broad rule fallback.
pub fn prepare_update_firewall() -> UpdateFirewallStatus {
    match prepare() {
        Ok(value) => value,
        Err(error) if error.code() == E_ACCESSDENIED => UpdateFirewallStatus::NeedsAdministrator,
        Err(_) => UpdateFirewallStatus::Restricted,
    }
}
