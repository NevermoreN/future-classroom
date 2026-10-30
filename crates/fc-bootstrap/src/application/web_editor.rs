//! Native teacher-device administration. Secrets never enter a View or form draft.
use super::*;
use fc_bootstrap::backend::web::{ManagementSnapshot, PairingWindow, WebService};
use fc_security::web::{PairingCode, SecretText};

pub(super) const CODE: FieldId = FieldId(500);
pub(super) const REQUEST: FieldId = FieldId(501);
pub(super) const DEVICE: FieldId = FieldId(502);
pub(super) const REFRESH: ActionId = ActionId(500);
pub(super) const ENABLE: ActionId = ActionId(501);
pub(super) const BEGIN: ActionId = ActionId(502);
pub(super) const CLOSE: ActionId = ActionId(503);
pub(super) const APPROVE: ActionId = ActionId(504);
pub(super) const REJECT: ActionId = ActionId(505);
pub(super) const REVOKE: ActionId = ActionId(506);
pub(super) const DISABLE: ActionId = ActionId(507);
const PREVIOUS_SCOPE: ActionId = ActionId(508);
const NEXT_SCOPE: ActionId = ActionId(509);
pub(super) const CONFIRM: ActionId = ActionId(510);
const CANCEL: ActionId = ActionId(511);
const SCOPE_PAGE: usize = 8;
pub(super) const NOT_READY: &str = "教师连接服务尚未就绪，暂不能通过浏览器配对。";

#[derive(Clone, Copy)]
pub(super) enum Readiness {
    Unavailable,
    // Only the private test fixture may simulate readiness, never a user setting.
    #[cfg(test)]
    SyntheticFixture,
}
impl Readiness {
    fn ready(self) -> bool {
        match self {
            Self::Unavailable => false,
            #[cfg(test)]
            Self::SyntheticFixture => true,
        }
    }
}
pub(super) enum Operation {
    Refresh,
    Enable,
    Begin,
    Close,
    Approve { request_id: u64, expected: Revision },
    Reject(u64),
    Revoke(u64),
    Disable,
}
pub(super) struct Loaded {
    pub(super) snapshot: ManagementSnapshot,
    pub(super) observed: Instant,
    pub(super) changed: bool,
    opened: Option<PairingWindow>,
}
pub(super) fn load(service: &WebService, admin: SessionLease) -> Result<Loaded> {
    let observed = Instant::now();
    let snapshot = service.management_snapshot(admin).map_err(Failure::Web)?;
    Ok(Loaded {
        snapshot,
        observed,
        opened: None,
        changed: false,
    })
}
pub(super) fn run(
    service: &WebService,
    admin: SessionLease,
    operation: Operation,
    readiness: Readiness,
    context: &JobContext,
) -> Result<Loaded> {
    if matches!(operation, Operation::Enable | Operation::Begin) && !readiness.ready() {
        return Err(invalid(NOT_READY));
    }
    let applied = match operation {
        Operation::Refresh => Ok(None),
        Operation::Enable => service.set_enabled(admin, true).map(|_| None),
        Operation::Begin => service.begin_pairing(admin).map(Some),
        Operation::Close => service.close_pairing(admin).map(|_| None),
        Operation::Approve {
            request_id,
            expected,
        } => service.approve(admin, request_id, expected).map(|_| None),
        Operation::Reject(id) => service.reject(admin, id).map(|_| None),
        Operation::Revoke(id) => service.revoke_device(admin, id).map(|_| None),
        Operation::Disable => service.set_enabled(admin, false).map(|_| None),
    };
    let (opened, changed) = match applied {
        Ok(opened) => (opened, false),
        // A stale selected request is not a reason to retain its old metadata.
        // A new management snapshot below still must prove this exact admin.
        Err(WebServiceError::Denied) => (None, true),
        Err(error) => return Err(Failure::Web(error)),
    };
    if context.checkpoint().is_err() {
        if opened.is_some() {
            let _ = service.close_pairing(admin);
        }
        return Err(Failure::Stopped);
    }
    let mut result = match load(service, admin) {
        Ok(result) => result,
        Err(error) => {
            if opened.is_some() {
                let _ = service.close_pairing(admin);
            }
            return Err(error);
        }
    };
    result.opened = opened;
    result.changed = changed;
    Ok(result)
}
pub(super) enum Command {
    Stay,
    Back,
    Lock,
    Run(Operation),
}
enum Confirmation {
    Device(u64),
    Disable,
}
pub(super) fn authorized(identity: &IdentityState, lease: SessionLease) -> bool {
    lease.origin() == TrustedOrigin::Local
        && identity.account(lease.principal_id()).is_ok_and(|a| {
            a.principal.enabled && !a.must_change_password && a.principal.role == Role::GlobalAdmin
        })
}
pub(super) struct Editor {
    owner: PrincipalId,
    readiness: Readiness,
    snapshot: ManagementSnapshot,
    deadline: Option<Instant>,
    selected_request: Option<(u64, Revision)>,
    selected_device: Option<u64>,
    confirmation: Option<Confirmation>,
    scope_page: usize,
    code: Option<PairingCode>,
    code_window: Option<u64>,
}
impl Editor {
    pub(super) fn new(owner: PrincipalId, readiness: Readiness, loaded: Loaded) -> Self {
        let mut editor = Self {
            owner,
            readiness,
            snapshot: loaded.snapshot.clone(),
            deadline: None,
            selected_request: None,
            selected_device: None,
            confirmation: None,
            scope_page: 0,
            code: None,
            code_window: None,
        };
        editor.loaded(loaded);
        editor
    }
    fn clear_code(&mut self) {
        self.code = None;
        self.code_window = None;
    }
    pub(super) fn clear_sensitive(&mut self) {
        self.clear_code();
        self.snapshot.pending.clear();
        self.selected_request = None;
        self.confirmation = None;
    }
    pub(super) fn loaded(&mut self, mut loaded: Loaded) {
        if let Some((id, revision)) = self.selected_request {
            let old = self
                .snapshot
                .pending
                .iter()
                .find(|r| r.request_id == id && r.identity_revision == revision);
            let fresh = loaded
                .snapshot
                .pending
                .iter()
                .find(|r| r.request_id == id && r.identity_revision == revision);
            if old.is_none() || old != fresh {
                self.selected_request = None;
                self.scope_page = 0;
            }
        }
        if self
            .selected_device
            .is_some_and(|id| !loaded.snapshot.devices.iter().any(|d| d.device_id == id))
        {
            self.selected_device = None;
            self.confirmation = None;
        }
        if let Some(opened) = loaded.opened.take() {
            self.code_window = Some(opened.id);
            self.code = Some(opened.code);
        }
        self.deadline = loaded.snapshot.window.as_ref().and_then(|w| {
            loaded
                .observed
                .checked_add(Duration::from_millis(w.remaining_millis))
        });
        if loaded
            .snapshot
            .window
            .as_ref()
            .is_none_or(|w| !w.code_available || Some(w.id) != self.code_window)
            || !loaded.snapshot.pending.is_empty()
        {
            self.clear_code();
        }
        self.snapshot = loaded.snapshot;
        self.tick(Instant::now());
    }
    pub(super) fn tick(&mut self, now: Instant) -> bool {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.deadline = None;
            self.snapshot.window = None;
            self.clear_sensitive();
            return true;
        }
        false
    }
    pub(super) fn code_text(&self) -> Option<SecretText> {
        self.code.as_ref().map(PairingCode::expose_text)
    }
    pub(super) fn input(&mut self, data: &FormData) {
        if self.confirmation.is_some() {
            return;
        }
        let request = data.get(REQUEST).and_then(|s| s.parse::<u64>().ok());
        let captured = request
            .and_then(|id| self.snapshot.pending.iter().find(|r| r.request_id == id))
            .map(|r| (r.request_id, r.identity_revision));
        if self.selected_request != captured {
            self.scope_page = 0;
        }
        self.selected_request = captured;
        self.selected_device = data
            .get(DEVICE)
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|id| self.snapshot.devices.iter().any(|d| d.device_id == *id));
    }
    pub(super) fn handle(
        &mut self,
        action: ActionId,
        data: &FormData,
        identity: &IdentityState,
        lease: SessionLease,
    ) -> std::result::Result<Command, String> {
        if self.owner != lease.principal_id() || !authorized(identity, lease) {
            self.clear_sensitive();
            return Err("教师设备管理仅限当前本地主管理员。".into());
        }
        if action == LOCK {
            self.clear_sensitive();
            return Ok(Command::Lock);
        }
        if action == BACK {
            self.clear_sensitive();
            return Ok(Command::Back);
        }
        if action == CANCEL {
            self.confirmation = None;
            return Ok(Command::Stay);
        }
        if action == CONFIRM {
            return match self.confirmation.take() {
                Some(Confirmation::Device(id))
                    if self.snapshot.devices.iter().any(|d| d.device_id == id) =>
                {
                    Ok(Command::Run(Operation::Revoke(id)))
                }
                Some(Confirmation::Disable) => {
                    self.clear_sensitive();
                    Ok(Command::Run(Operation::Disable))
                }
                _ => Err("所选设备已变化，请重新选择。".into()),
            };
        }
        if self.confirmation.is_some() {
            return Ok(Command::Stay);
        }
        match action {
            REFRESH => Ok(Command::Run(Operation::Refresh)),
            ENABLE | BEGIN if !self.readiness.ready() => Err(NOT_READY.into()),
            ENABLE => Ok(Command::Run(Operation::Enable)),
            BEGIN => {
                self.clear_code();
                Ok(Command::Run(Operation::Begin))
            }
            CLOSE => {
                self.clear_sensitive();
                Ok(Command::Run(Operation::Close))
            }
            APPROVE | REJECT => {
                let (id, revision) = self
                    .selected_request
                    .ok_or("请先明确选择待批准的教师设备。")?;
                if action == APPROVE
                    && self
                        .snapshot
                        .pending
                        .iter()
                        .any(|r| r.request_id == id && r.approved)
                {
                    return Err("此申请已批准，正在等待原申请设备领取。".into());
                }
                if data.get(REQUEST) != Some(id.to_string().as_str()) {
                    return Err("申请选择已变化，请重新选择。".into());
                }
                if action == APPROVE {
                    Ok(Command::Run(Operation::Approve {
                        request_id: id,
                        expected: revision,
                    }))
                } else {
                    Ok(Command::Run(Operation::Reject(id)))
                }
            }
            REVOKE => {
                let id = self.selected_device.ok_or("请先选择要撤销的设备。")?;
                if data.get(DEVICE) != Some(id.to_string().as_str()) {
                    return Err("设备选择已变化，请重新选择。".into());
                }
                self.confirmation = Some(Confirmation::Device(id));
                Ok(Command::Stay)
            }
            DISABLE => {
                self.clear_code();
                self.confirmation = Some(Confirmation::Disable);
                Ok(Command::Stay)
            }
            PREVIOUS_SCOPE => {
                self.scope_page = self.scope_page.saturating_sub(1);
                Ok(Command::Stay)
            }
            NEXT_SCOPE => {
                let last = self
                    .selected_request
                    .and_then(|(id, revision)| {
                        self.snapshot
                            .pending
                            .iter()
                            .find(|r| r.request_id == id && r.identity_revision == revision)
                    })
                    .map_or(0, |r| {
                        r.assignments.len().div_ceil(SCOPE_PAGE).saturating_sub(1)
                    });
                self.scope_page = self.scope_page.saturating_add(1).min(last);
                Ok(Command::Stay)
            }
            _ => Err("此页面不支持该操作。".into()),
        }
    }
    pub(super) fn view(&self, identity: &IdentityState, status: &str, busy: bool) -> View {
        let action = |id, label: &str, enabled| Action {
            id,
            label: label.into(),
            enabled,
        };
        let mut view = View {
            title: "教师设备与配对管理".into(),
            description: NOT_READY.into(),
            status: status.into(),
            fields: vec![],
            actions: vec![],
            busy,
            grid: None,
        };
        if let Some(confirmation) = &self.confirmation {
            view.description = match confirmation {
                Confirmation::Device(id) => {
                    let device = self.snapshot.devices.iter().find(|d| d.device_id == *id);
                    format!("确认撤销设备ID {id}（{}，账号ID {}）？只撤销此设备的Web会话，不注销同账号其他设备或本地会话。",
                        device.map_or("设备已变化", |d| d.device_name.as_str()),
                        device.map(|d| d.principal_id.get().to_string()).unwrap_or_else(|| "不可用".into()))
                }
                Confirmation::Disable => {
                    "确认停用并撤销全部教师设备？已有Web会话和待批准申请都会失效；本地工作台保持。"
                        .into()
                }
            };
            view.actions = vec![
                action(
                    CONFIRM,
                    match confirmation {
                        Confirmation::Device(_) => "确认撤销此设备",
                        Confirmation::Disable => "确认停用并撤销全部",
                    },
                    true,
                ),
                action(CANCEL, "返回选择", true),
                action(LOCK, "锁定工作台", true),
            ];
            return view;
        }
        let logical = if self.snapshot.enabled {
            "内部教师会话准入已开启"
        } else {
            "内部教师会话准入已停用"
        };
        view.description = format!(
            "{NOT_READY}\n{logical}。这里不表示HTTPS或浏览器已经可用。可查看并撤销现有设备。{}",
            if self.readiness.ready() {
                "\n当前为自有隔离测试夹具，不是真实TLS。"
            } else {
                ""
            }
        );
        let code_label = if let Some(deadline) = self.deadline {
            let seconds = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .div_ceil(1000);
            format!("一次性配对短码（窗口剩余{seconds}秒；申请后隐藏）")
        } else {
            "一次性配对短码（当前没有配对窗口）".into()
        };
        let short = |s: &str| s.chars().take(24).collect::<String>();
        view.fields.push(Field {
            id: CODE,
            label: code_label,
            kind: FieldKind::ReadOnly,
            value: String::new(),
            max_utf16: 8,
        });
        view.fields.push(Field {
            id: REQUEST,
            label: "待批准的教师和设备".into(),
            kind: FieldKind::Choice(
                self.snapshot
                    .pending
                    .iter()
                    .map(|r| Choice {
                        value: r.request_id.to_string(),
                        label: format!(
                            "{} / {}（{}）",
                            short(&r.display_name),
                            short(&r.device_name),
                            short(&r.login_name)
                        ),
                    })
                    .collect(),
            ),
            value: self
                .selected_request
                .map(|r| r.0.to_string())
                .unwrap_or_default(),
            max_utf16: 32,
        });
        view.fields.push(Field {
            id: DEVICE,
            label: "已授权设备".into(),
            kind: FieldKind::Choice(
                self.snapshot
                    .devices
                    .iter()
                    .map(|d| Choice {
                        value: d.device_id.to_string(),
                        label: format!(
                            "设备{}：{} / {}",
                            d.device_id,
                            short(&d.device_name),
                            identity
                                .account(d.principal_id)
                                .map(|a| short(&a.display_name))
                                .unwrap_or_else(|_| format!("账号{}", d.principal_id.get()))
                        ),
                    })
                    .collect(),
            ),
            value: self
                .selected_device
                .map(|id| id.to_string())
                .unwrap_or_default(),
            max_utf16: 32,
        });
        let mut more_scopes = false;
        if let Some((id, revision)) = self.selected_request {
            if let Some(review) = self
                .snapshot
                .pending
                .iter()
                .find(|r| r.request_id == id && r.identity_revision == revision)
            {
                let pages = review.assignments.len().div_ceil(SCOPE_PAGE).max(1);
                let page = self.scope_page.min(pages - 1);
                let row = |label: &str, value: String| GridRow {
                    group: String::new(),
                    label: label.into(),
                    cells: vec![value],
                    merged: false,
                };
                let mut rows = vec![
                    row(
                        "原申请与身份修订",
                        format!("申请{id} / 修订{}", revision.get()),
                    ),
                    row(
                        "教师账号",
                        format!("{}（{}）", review.display_name, review.login_name),
                    ),
                    row("申请设备", review.device_name.clone()),
                    row(
                        "账号角色",
                        match review.role {
                            Role::GlobalAdmin => "主管理员",
                            Role::TeacherAdmin => "教师管理员",
                            Role::User => "普通用户",
                        }
                        .into(),
                    ),
                    row(
                        "白板当前班级",
                        review
                            .current_class
                            .map(|c| c.get().to_string())
                            .unwrap_or_else(|| "未绑定".into()),
                    ),
                    row(
                        "全部现有任教范围",
                        format!(
                            "第{}/{}页，共{}项；配对不增加现有角色或权限。",
                            page + 1,
                            pages,
                            review.assignments.len()
                        ),
                    ),
                ];
                rows.extend(
                    review
                        .assignments
                        .iter()
                        .skip(page * SCOPE_PAGE)
                        .take(SCOPE_PAGE)
                        .map(|a| {
                            row(
                                "任教班级 / 科目",
                                format!("班级{} / 科目{}", a.class_id.get(), a.subject_id.get()),
                            )
                        }),
                );
                view.grid = Some(Grid {
                    columns: vec!["核对项目".into(), "服务器记录".into()],
                    rows,
                    selected: None,
                    enabled: false,
                });
                more_scopes = page + 1 < pages;
            }
        }
        view.actions = vec![
            action(REFRESH, "刷新申请和设备", true),
            action(
                ENABLE,
                "开启教师会话准入",
                self.readiness.ready() && !self.snapshot.enabled,
            ),
            action(
                BEGIN,
                "开启短时配对窗口",
                self.readiness.ready() && self.snapshot.enabled,
            ),
            action(CLOSE, "关闭本次配对", self.snapshot.window.is_some()),
            action(
                APPROVE,
                "批准所选申请",
                self.selected_request.is_some_and(|(id, _)| {
                    self.snapshot
                        .pending
                        .iter()
                        .any(|r| r.request_id == id && !r.approved)
                }),
            ),
            action(REJECT, "拒绝所选申请", self.selected_request.is_some()),
            action(REVOKE, "撤销所选设备", self.selected_device.is_some()),
            action(
                DISABLE,
                "停用并撤销全部设备",
                self.snapshot.enabled || !self.snapshot.devices.is_empty(),
            ),
            action(BACK, "返回工作台", true),
            action(LOCK, "锁定工作台", true),
            action(PREVIOUS_SCOPE, "上一页任教范围", self.scope_page > 0),
            action(NEXT_SCOPE, "下一页任教范围", more_scopes),
        ];
        view
    }
}

#[cfg(test)]
mod tests;
