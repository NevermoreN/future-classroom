//! Bounded local draft and original-command reconciliation. This editor cannot
//! authenticate, access SQLite, inspect a certificate or enable a listener.
use super::*;
use fc_bootstrap::backend::HttpsCertificateObservation;
use fc_domain::https_settings::{HttpsCertificateRef, HttpsConfiguration, HttpsHost};
use fc_ports::{HttpsSettingsRequest, HttpsSettingsState};
use fc_windows::tls_frontdoor::certificate_query::{CertificateCheck, CertificateQueryReport};

pub(super) const HOST: FieldId = FieldId(920);
pub(super) const PORT: FieldId = FieldId(921);
pub(super) const CERTIFICATE: FieldId = FieldId(922);
pub(super) const SAVE_CONFIG: ActionId = ActionId(920);
pub(super) const RELOAD: ActionId = ActionId(921);
pub(super) const QUERY: ActionId = ActionId(922);
pub(super) const CANCEL_DRAFT: ActionId = ActionId(923);
pub(super) const CANCEL_WAIT: ActionId = ActionId(924);
pub(super) const CONFIRM_DISCARD: ActionId = ActionId(925);
pub(super) const KEEP: ActionId = ActionId(926);
pub(super) const CLOSE: ActionId = ActionId(927);
pub(super) const CHECK_CERTIFICATE: ActionId = ActionId(928);
pub(super) const SAVED: &str = "配置已保存；教师连接尚未启用。证书引用尚未检查。";
pub(super) const UNCONFIRMED: &str = "尚未确认本次保存结果。请查询原操作结果，不要重复保存。";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Owner {
    pub(super) school: SchoolId,
    pub(super) principal: PrincipalId,
}
#[derive(Clone, Default, PartialEq, Eq)]
struct Draft {
    host: String,
    port: String,
    certificate: String,
}
impl Draft {
    fn from_state(state: &HttpsSettingsState) -> Self {
        state
            .settings
            .configuration
            .as_ref()
            .map_or_else(Self::default, |c| Self {
                host: c.host.as_str().into(),
                port: c.port.to_string(),
                certificate: c.certificate.to_hex(),
            })
    }
    fn read(mut data: FormData) -> std::result::Result<Self, String> {
        let draft = Self {
            host: data.take(HOST).unwrap_or_default(),
            port: data.take(PORT).unwrap_or_default(),
            certificate: data.take(CERTIFICATE).unwrap_or_default(),
        };
        if !data.is_empty()
            || draft.host.encode_utf16().count() > 253
            || draft.port.encode_utf16().count() > 5
            || draft.certificate.encode_utf16().count() > 40
            || [&draft.host, &draft.port, &draft.certificate]
                .iter()
                .any(|v| v.contains('\0'))
        {
            return Err("表单内容不受支持，请重新打开配置页面。".into());
        }
        Ok(draft)
    }
    fn configuration(&self) -> std::result::Result<HttpsConfiguration, String> {
        let host = HttpsHost::parse(&self.host)
            .map_err(|_| "请填写有效域名或IP，不要包含网址前缀、端口或路径。")?;
        if self.port.is_empty() || !self.port.bytes().all(|b| b.is_ascii_digit()) {
            return Err("端口须为1至65535的整数。".into());
        }
        let port = self
            .port
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or("端口须为1至65535的整数。")?;
        let certificate = HttpsCertificateRef::parse(&self.certificate)
            .map_err(|_| "证书引用须为40位十六进制字符，不要填写证书文件路径。")?;
        HttpsConfiguration::new(host, port, certificate)
            .map_err(|_| "请核对域名、端口和证书引用。".into())
    }
}
struct Original {
    request: HttpsSettingsRequest,
    deadline: Instant,
    accepted: bool,
}
#[derive(Clone, Copy)]
enum Leave {
    Reload,
    Close,
    CancelDraft,
}
pub(super) enum Command {
    Stay,
    Close,
    Reload,
    Save,
    Query,
    CheckCertificate,
}
pub(super) struct Editor {
    owner: Owner,
    base: Option<HttpsSettingsState>,
    draft: Draft,
    original: Option<Original>,
    historical: Option<Option<HttpsSettingsState>>,
    confirmation: Option<Leave>,
    certificate_observation: Option<HttpsCertificateObservation>,
}
impl Editor {
    pub(super) fn new(owner: Owner) -> Self {
        Self {
            owner,
            base: None,
            draft: Draft::default(),
            original: None,
            historical: None,
            confirmation: None,
            certificate_observation: None,
        }
    }
    pub(super) fn owns(&self, owner: Owner) -> bool {
        self.owner == owner
    }
    pub(super) fn loaded(&mut self, state: HttpsSettingsState) {
        self.certificate_observation = None;
        self.draft = Draft::from_state(&state);
        self.base = Some(state);
        self.original = None;
        self.historical = None;
        self.confirmation = None;
    }
    pub(super) fn capture(&mut self, data: FormData) -> std::result::Result<(), String> {
        if data.is_empty() {
            return Ok(());
        }
        let next = Draft::read(data)?;
        if next != self.draft {
            self.certificate_observation = None;
            // Only an UNADMITTED request can be replaced after the draft changes.
            if self.original.as_ref().is_some_and(|o| !o.accepted) {
                self.original = None;
            }
            self.draft = next;
        }
        Ok(())
    }
    pub(super) fn dirty(&self) -> bool {
        self.base
            .as_ref()
            .map_or(self.draft != Draft::default(), |s| {
                self.draft != Draft::from_state(s)
            })
    }
    pub(super) fn has_unknown(&self) -> bool {
        self.original.as_ref().is_some_and(|o| o.accepted)
    }
    pub(super) fn prepare_save(
        &mut self,
        id: CommandId,
        now: Instant,
    ) -> std::result::Result<HttpsSettingsRequest, String> {
        if self.has_unknown() {
            return Err(UNCONFIRMED.into());
        }
        let base = self.base.as_ref().ok_or("请先读取当前配置。")?;
        let config = self.draft.configuration()?;
        if let Some(original) = &self.original {
            if now >= original.deadline {
                return Err("原保存操作已过处理期限，请明确重新读取后再确认。".into());
            }
            return Ok(original.request.clone());
        }
        let request = HttpsSettingsRequest {
            command_id: id,
            expected_revision: base.settings.revision,
            expected_identity_revision: base.identity_revision,
            configuration: Some(config),
        };
        self.original = Some(Original {
            request: request.clone(),
            deadline: now + Duration::from_secs(90),
            accepted: false,
        });
        Ok(request)
    }
    pub(super) fn save_deadline(&self) -> Option<Instant> {
        self.original.as_ref().map(|o| o.deadline)
    }
    pub(super) fn accepted(&mut self) {
        self.certificate_observation = None;
        if let Some(original) = &mut self.original {
            original.accepted = true;
        }
    }
    pub(super) fn query_request(&self) -> Option<HttpsSettingsRequest> {
        self.original
            .as_ref()
            .filter(|o| o.accepted)
            .map(|o| o.request.clone())
    }
    pub(super) fn queried(&mut self, result: Option<HttpsSettingsState>) {
        self.historical = Some(result);
    }
    pub(super) fn suspended(&mut self) {
        self.certificate_observation = None;
        self.confirmation = None;
    }
    pub(super) fn certificate_state(&self) -> Option<HttpsSettingsState> {
        self.base
            .as_ref()
            .filter(|s| s.settings.configuration.is_some() && !self.dirty() && !self.has_unknown())
            .cloned()
    }
    pub(super) fn clear_certificate(&mut self) {
        self.certificate_observation = None;
    }
    pub(super) fn certificate_observation(&self) -> Option<&HttpsCertificateObservation> {
        self.certificate_observation.as_ref()
    }
    pub(super) fn certificate_loaded(&mut self, observation: HttpsCertificateObservation) {
        self.certificate_observation = Some(observation);
    }
    pub(super) fn invalidate_certificate(
        &mut self,
        lease: SessionLease,
        control: &SessionControl,
    ) -> bool {
        let stale = self.certificate_observation.as_ref().is_some_and(|o| {
            self.base
                .as_ref()
                .is_none_or(|s| !o.is_current_for(s, lease, control))
                || self.dirty()
                || self.has_unknown()
        });
        if stale {
            self.certificate_observation = None;
        }
        stale
    }
    pub(super) fn handle(
        &mut self,
        owner: Owner,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<Command, String> {
        if !self.owns(owner) {
            if !data.is_empty() {
                return Err("请先放弃暂存内容或返回。".into());
            }
            return match action {
                CONFIRM_DISCARD => {
                    *self = Self::new(owner);
                    Ok(Command::Reload)
                }
                CLOSE => Ok(Command::Close),
                _ => Err("请先确认是否放弃本机暂存内容。".into()),
            };
        }
        if self.confirmation.is_some() {
            if !data.is_empty() {
                return Err("请只选择保留或放弃暂存内容。".into());
            }
            return match action {
                KEEP => {
                    self.confirmation = None;
                    Ok(Command::Stay)
                }
                CONFIRM_DISCARD => match self.confirmation.take() {
                    Some(Leave::Reload) => {
                        self.original = None;
                        self.historical = None;
                        Ok(Command::Reload)
                    }
                    Some(Leave::Close) => {
                        self.original = None;
                        self.historical = None;
                        self.draft = self
                            .base
                            .as_ref()
                            .map_or_else(Draft::default, Draft::from_state);
                        Ok(Command::Close)
                    }
                    Some(Leave::CancelDraft) => {
                        self.original = None;
                        self.historical = None;
                        self.draft = self
                            .base
                            .as_ref()
                            .map_or_else(Draft::default, Draft::from_state);
                        Ok(Command::Stay)
                    }
                    None => Ok(Command::Stay),
                },
                _ => Err("请先选择保留或放弃暂存内容。".into()),
            };
        }
        self.capture(data)?;
        if matches!(action, CANCEL_DRAFT | CLOSE | RELOAD) {
            self.certificate_observation = None;
        }
        match action {
            SAVE_CONFIG if self.has_unknown() => Err(UNCONFIRMED.into()),
            SAVE_CONFIG => {
                self.draft.configuration()?;
                if self.base.is_none() {
                    return Err("请先读取当前配置。".into());
                }
                Ok(Command::Save)
            }
            CHECK_CERTIFICATE if self.certificate_state().is_some() => {
                self.certificate_observation = None;
                Ok(Command::CheckCertificate)
            }
            CHECK_CERTIFICATE => {
                Err("请先保存并重新核对配置，完成原操作结果查询后再检查证书。".into())
            }
            QUERY if self.has_unknown() => Ok(Command::Query),
            QUERY => Err("当前没有需要查询的原保存操作。".into()),
            RELOAD | CLOSE | CANCEL_DRAFT if self.dirty() || self.original.is_some() => {
                self.confirmation = Some(match action {
                    RELOAD => Leave::Reload,
                    CLOSE => Leave::Close,
                    _ => Leave::CancelDraft,
                });
                Ok(Command::Stay)
            }
            RELOAD => Ok(Command::Reload),
            CLOSE => Ok(Command::Close),
            CANCEL_DRAFT => Ok(Command::Stay),
            _ => Err("此页面不支持该操作。".into()),
        }
    }
    pub(super) fn view(&self, owner: Owner, status: &str, busy: bool) -> View {
        let action = |id, label: &str, enabled| Action {
            id,
            label: label.into(),
            enabled,
        };
        let mut view = View {
            title: "本机 HTTPS 连接配置".into(),
            description: String::new(),
            status: status.into(),
            fields: vec![],
            actions: vec![],
            // Only this page permits Cancel while work runs. Its fields become
            // read-only and all other ordinary actions are disabled below.
            busy: false,
            grid: None,
        };
        if !self.owns(owner) {
            view.description = "本机还暂存着另一管理会话的编辑内容。此处不显示该内容。放弃暂存不会撤销已经保存的配置，也不能确认先前操作是否保存。".into();
            view.actions = vec![
                action(CONFIRM_DISCARD, "放弃暂存并读取当前配置", !busy),
                action(CLOSE, "返回设备管理", !busy),
                action(LOCK, "锁定工作台", true),
            ];
            return view;
        }
        if self.confirmation.is_some() {
            view.description = "是否放弃本次暂存的编辑内容和原操作记录？这不会撤销已经保存的配置。如果先前结果未确认，放弃记录也不能证明没有保存。".into();
            view.actions = vec![
                action(KEEP, "保留暂存内容", true),
                action(CONFIRM_DISCARD, "确认放弃", !busy),
                action(LOCK, "锁定工作台", true),
            ];
            return view;
        }
        view.description = "这里只保存本机连接配置，不启动网络监听，也不启用教师连接。证书存储固定为当前程序用户的MY；填写引用不代表证书可用或其他设备已信任。修改前须在设备管理中明确停用教师会话准入。".into();
        if let Some(base) = &self.base {
            view.description.push_str(&format!(
                "\n当前读取的配置版本：{}。",
                base.settings.revision.get()
            ));
            if base.settings.configuration.is_none() {
                view.description
                    .push_str("本机尚未配置HTTPS，端口没有默认值。");
            }
        }
        if self.has_unknown() {
            view.description.push_str(&format!("\n{UNCONFIRMED}"));
        }
        if let Some(result) = &self.historical {
            match result {
                Some(result) => view.description.push_str(&format!("\n原操作的历史记录：已保存为版本{}。这不是重新读取的当前配置，也不表示教师连接已启用。", result.settings.revision.get())),
                None => view.description.push_str("\n未查到原操作结果。"),
            }
        }
        let field = |id, label: &str, value: &str, max_utf16| Field {
            id,
            label: label.into(),
            kind: if busy {
                FieldKind::ReadOnly
            } else {
                FieldKind::Text
            },
            value: value.into(),
            max_utf16,
        };
        view.fields = vec![
            field(
                HOST,
                "域名或IP（不含网址前缀、端口和路径）",
                &self.draft.host,
                253,
            ),
            field(PORT, "HTTPS端口（1至65535）", &self.draft.port, 5),
            field(
                CERTIFICATE,
                "当前程序用户MY证书引用（40位十六进制）",
                &self.draft.certificate,
                40,
            ),
        ];
        if self.certificate_observation.is_none() {
            view.description
                .push_str("\n尚无关联到本次配置的证书检查记录；可在保存并核对配置后明确检查。");
        }
        if let Some(observation) = &self.certificate_observation {
            let date = observation.observed_at.date;
            let time = observation.observed_at.second_of_day();
            view.description.push_str(&format!("\n检查时间：{:04}-{:02}-{:02} {:02}:{:02}:{:02}。以下仅为这次只读检查的历史事实。仅使用本机现有缓存，不证明其他设备信任、在线撤销状态或教师连接可用。",date.year(),date.month(),date.day(),time/3600,time/60%60,time%60));
            view.grid = Some(certificate_grid(&observation.report));
        }
        view.actions = vec![
            action(
                CHECK_CERTIFICATE,
                "检查已保存的证书",
                !busy && self.certificate_state().is_some(),
            ),
            action(
                SAVE_CONFIG,
                "保存配置",
                !busy && self.base.is_some() && !self.has_unknown(),
            ),
            action(QUERY, "查询原操作结果", !busy && self.has_unknown()),
            action(RELOAD, "重新读取当前配置", !busy),
            action(CANCEL_DRAFT, "取消修改", !busy),
            action(CLOSE, "返回设备管理", !busy),
            action(CANCEL_WAIT, "取消等待", busy),
            action(LOCK, "锁定工作台", true),
        ];
        view
    }
}
#[cfg(test)]
mod tests;

fn certificate_grid(report: &CertificateQueryReport) -> Grid {
    let fact = |label: &str, check: CertificateCheck| GridRow {
        group: String::new(),
        label: label.into(),
        cells: vec![match check {
            CertificateCheck::Satisfied => "检查时满足",
            CertificateCheck::Rejected => "检查时不满足",
            CertificateCheck::Unverified => "未验证",
            CertificateCheck::NotChecked => "未检查",
        }
        .into()],
        merged: false,
    };
    Grid {
        columns: vec!["核对项目".into(), "本次观察（不表示可启用）".into()],
        rows: vec![
            fact("当前程序用户的证书存储中找到引用", report.found.check),
            fact("本次检查时的证书有效期", report.current_time.check),
            fact(
                "现有私钥可静默取得（不代表后续签名成功）",
                report.private_key.check,
            ),
            fact("服务器身份验证用途", report.server_auth_eku.check),
            fact(
                "填写的域名是否匹配（IP可能未验证）",
                report.server_name.check,
            ),
            fact("本机SSL证书链策略", report.ssl_chain_policy.check),
            fact(
                "本机缓存的撤销信息（不含根证书）",
                report.cached_revocation.check,
            ),
        ],
        selected: None,
        enabled: false,
    }
}
