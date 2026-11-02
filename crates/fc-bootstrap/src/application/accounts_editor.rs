//! Non-secret account drafts. This editor previews domain rules; only the
//! composition's real local lease and writer transaction authorize a mutation.
use fc_domain::{auth::*, identity::*, ids::*, time::*};
use fc_presentation::workbench::*;
use fc_security::Password;

pub(super) const LOGIN: FieldId = FieldId(600);
pub(super) const NAME: FieldId = FieldId(601);
pub(super) const ROLE: FieldId = FieldId(602);
pub(super) const ENABLED: FieldId = FieldId(603);
pub(super) const CLASS: FieldId = FieldId(604);
pub(super) const SUBJECT: FieldId = FieldId(605);
pub(super) const PASSWORD: FieldId = FieldId(606);
pub(super) const CONFIRM_PASSWORD: FieldId = FieldId(607);
pub(super) const DEFAULT_TEACHER: FieldId = FieldId(608);
pub(super) const NEW: ActionId = ActionId(600);
pub(super) const EDIT: ActionId = ActionId(601);
pub(super) const NEXT: ActionId = ActionId(602);
pub(super) const PREVIOUS: ActionId = ActionId(603);
pub(super) const RELOAD: ActionId = ActionId(604);
pub(super) const CLOSE: ActionId = ActionId(605);
pub(super) const SCOPES: ActionId = ActionId(606);
pub(super) const ADD_SCOPE: ActionId = ActionId(607);
pub(super) const REMOVE_SCOPE: ActionId = ActionId(608);
pub(super) const RETURN: ActionId = ActionId(609);
pub(super) const REVIEW: ActionId = ActionId(610);
pub(super) const CONFIRM: ActionId = ActionId(611);
pub(super) const CANCEL: ActionId = ActionId(612);
pub(super) const DISCARD: ActionId = ActionId(613);
pub(super) const KEEP: ActionId = ActionId(614);
pub(super) const CLEAR_SCOPES: ActionId = ActionId(615);
pub(super) const APPLY_ROLE: ActionId = ActionId(616);
pub(super) const DEFAULTS: ActionId = ActionId(617);
pub(super) const DEFAULT_SELECT: ActionId = ActionId(618);
pub(super) const END_RESULT_QUERY: ActionId = ActionId(619);
pub(super) const TRANSFER: ActionId = ActionId(620);
pub(super) const QUERY_TRANSFER: ActionId = ActionId(621);
const PAGE_SIZE: usize = 20;

#[derive(Debug)]
pub(super) enum Command {
    Stay,
    Close,
    Reload,
    Create {
        expected: Revision,
        account: NewAccount,
        password: Password,
    },
    Update {
        expected: Revision,
        command: IdentityCommand,
    },
    Transfer {
        expected: Revision,
        from: PrincipalId,
        target: PrincipalId,
        expected_credential_version: u64,
        password: Password,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    List,
    Edit,
    Scopes,
    Confirm,
    Password,
    Discard,
    ClearScopes,
    DefaultScope,
    DefaultEdit,
    DefaultConfirm,
    Transfer,
}
#[derive(Clone, Copy)]
enum Destination {
    List,
    Close,
    Reload,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Draft {
    id: PrincipalId,
    creating: bool,
    login: String,
    name: String,
    role: Role,
    enabled: bool,
    scopes: Vec<AssignmentScope>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct DefaultDraft {
    class_id: ClassId,
    subject_id: SubjectId,
    original: Option<PrincipalId>,
    selected: Option<PrincipalId>,
}
pub(super) struct Editor {
    owner: PrincipalId,
    expected: Revision,
    accounts: Vec<UserAccount>,
    page: Page,
    account_page: usize,
    selected_account: Option<PrincipalId>,
    draft: Option<Draft>,
    original: Option<Draft>,
    scope_page: usize,
    selected_scope: Option<AssignmentScope>,
    selected_class: Option<ClassId>,
    selected_subject: Option<SubjectId>,
    pending_role: Role,
    discard_return: Page,
    destination: Destination,
    submitted: bool,
    default_draft: Option<DefaultDraft>,
}
fn action(id: ActionId, label: &str, enabled: bool) -> Action {
    Action {
        id,
        label: label.into(),
        enabled,
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
fn choice(id: FieldId, label: &str, value: String, choices: Vec<Choice>) -> Field {
    field(id, label, FieldKind::Choice(choices), value, 64)
}
fn role_name(role: Role) -> &'static str {
    match role {
        Role::GlobalAdmin => "主管理员",
        Role::TeacherAdmin => "教师管理员",
        Role::User => "普通用户",
    }
}
fn role_value(role: Role) -> &'static str {
    match role {
        Role::GlobalAdmin => "global",
        Role::TeacherAdmin => "teacher",
        Role::User => "user",
    }
}
fn parse_role(value: &str) -> Result<Role, String> {
    match value {
        "global" => Ok(Role::GlobalAdmin),
        "teacher" => Ok(Role::TeacherAdmin),
        "user" => Ok(Role::User),
        _ => Err("请选择三级角色之一。".into()),
    }
}
fn shorten(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.into();
    }
    let mut end = max_bytes.saturating_sub(3).min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}
fn caption(name: &str, id: u64, enabled: bool) -> String {
    format!(
        "#{id} {}{}",
        shorten(name, 440),
        if enabled { "" } else { "（停用）" }
    )
}
fn authorized(state: &IdentityState, owner: PrincipalId) -> Result<(), String> {
    let a = state
        .account(owner)
        .map_err(|_| "当前主管理员账号不存在。")?;
    if state.installation.is_none()
        || a.principal.role != Role::GlobalAdmin
        || !a.principal.enabled
        || a.must_change_password
    {
        return Err("仅已完成改密的本地主管理员可以管理账号；请重新登录。".into());
    }
    Ok(())
}
fn problem(error: IdentityError) -> String {
    match error {
        IdentityError::LastAdministrator => "不能停用或降级最后一个启用的主管理员。",
        IdentityError::ScopeRequired => "教师管理员至少需要一个班级与科目范围。",
        IdentityError::InvalidScope => "任教范围无效、重复或已停用；请明确移除或重新选择范围。",
        IdentityError::Capacity => "账号或任教范围达到容量上限。",
        IdentityError::DuplicateName => "登录名已存在，请使用其他登录名。",
        IdentityError::InvalidText => "登录名须为1至64字，姓名须为1至128字，且不得含控制字符。",
        IdentityError::VersionOverflow => "修订或账号编号已达上限。",
        _ => "账号变更不符合身份规则；草稿保留，请核对后重试。",
    }
    .into()
}
fn same_scopes(a: &[AssignmentScope], b: &[AssignmentScope]) -> bool {
    a.len() == b.len() && a.iter().all(|s| b.contains(s))
}
fn same_draft(a: &Draft, b: &Draft) -> bool {
    a.id == b.id
        && a.creating == b.creating
        && a.login == b.login
        && a.name == b.name
        && a.role == b.role
        && a.enabled == b.enabled
        && same_scopes(&a.scopes, &b.scopes)
}
impl Editor {
    pub(super) fn new(state: &IdentityState, owner: PrincipalId) -> Result<Self, String> {
        state.validate().map_err(problem)?;
        authorized(state, owner)?;
        let mut accounts = state.accounts.clone();
        accounts.sort_by_key(|a| a.principal.id.get());
        Ok(Self {
            owner,
            expected: state.revision,
            accounts,
            page: Page::List,
            account_page: 0,
            selected_account: None,
            draft: None,
            original: None,
            scope_page: 0,
            selected_scope: None,
            selected_class: None,
            selected_subject: None,
            pending_role: Role::User,
            discard_return: Page::Edit,
            destination: Destination::List,
            submitted: false,
            default_draft: None,
        })
    }
    pub(super) fn default_rejected(&mut self) {
        if self.page == Page::DefaultConfirm {
            self.submitted = false;
        }
    }
    pub(super) fn owner(&self) -> PrincipalId {
        self.owner
    }
    pub(super) fn has_unsaved_changes(&self) -> bool {
        if (self.page == Page::Transfer
            || self.page == Page::Discard && self.discard_return == Page::Transfer)
            && !self.submitted
        {
            return true;
        }
        if self
            .default_draft
            .as_ref()
            .is_some_and(|draft| draft.selected != draft.original)
        {
            return true;
        }
        match (&self.draft, &self.original) {
            (Some(d), Some(o)) => !same_draft(d, o) || self.pending_role != o.role,
            _ => false,
        }
    }
    fn fresh(&self, state: &IdentityState) -> Result<(), String> {
        authorized(state, self.owner)?;
        if state.revision != self.expected {
            return Err("账号目录已更新。草稿保留；请取消并重新加载后核对，不会自动重试。".into());
        }
        Ok(())
    }
    fn begin(&mut self, state: &IdentityState, id: Option<PrincipalId>) -> Result<(), String> {
        self.fresh(state)?;
        let draft = if let Some(id) = id {
            let account = state.account(id).map_err(problem)?;
            Draft {
                id,
                creating: false,
                login: account.login_name.clone(),
                name: account.display_name.clone(),
                role: account.principal.role,
                enabled: account.principal.enabled,
                scopes: state
                    .assignments
                    .iter()
                    .filter(|s| s.teacher_id == id && s.enabled)
                    .map(|s| AssignmentScope {
                        class_id: s.class_id,
                        subject_id: s.subject_id,
                    })
                    .collect(),
            }
        } else {
            if state.accounts.len() >= MAX_ACCOUNTS {
                return Err(problem(IdentityError::Capacity));
            }
            let id = state
                .accounts
                .iter()
                .map(|a| a.principal.id.get())
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .and_then(|id| PrincipalId::new(id).ok())
                .ok_or_else(|| problem(IdentityError::VersionOverflow))?;
            Draft {
                id,
                creating: true,
                login: String::new(),
                name: String::new(),
                role: Role::TeacherAdmin,
                enabled: true,
                scopes: Vec::new(),
            }
        };
        self.pending_role = draft.role;
        self.original = Some(draft.clone());
        self.draft = Some(draft);
        self.page = Page::Edit;
        self.scope_page = 0;
        self.selected_scope = None;
        self.submitted = false;
        self.selected_class = state.classes.iter().find(|c| c.enabled).map(|c| c.id);
        self.selected_subject = state.subjects.iter().find(|s| s.enabled).map(|s| s.id);
        Ok(())
    }
    /// White-list capture. Passwords and confirmation values remain in FormData's
    /// zeroizing storage and are dropped on navigation, lock and suspension.
    fn capture(&mut self, data: &FormData) -> Result<(), String> {
        if self.page == Page::DefaultScope {
            if let Some(value) = data.get(CLASS) {
                self.selected_class = if value.is_empty() {
                    None
                } else {
                    Some(
                        ClassId::new(value.parse::<u64>().map_err(|_| "请选择有效班级。")?)
                            .map_err(|_| "请选择有效班级。")?,
                    )
                };
            }
            if let Some(value) = data.get(SUBJECT) {
                self.selected_subject = if value.is_empty() {
                    None
                } else {
                    Some(
                        SubjectId::new(value.parse::<u64>().map_err(|_| "请选择有效科目。")?)
                            .map_err(|_| "请选择有效科目。")?,
                    )
                };
            }
            return Ok(());
        }
        if self.page == Page::DefaultEdit {
            if let Some(value) = data.get(DEFAULT_TEACHER) {
                let selected = if value.is_empty() {
                    None
                } else {
                    Some(
                        PrincipalId::new(value.parse::<u64>().map_err(|_| "请选择有效任课教师。")?)
                            .map_err(|_| "请选择有效任课教师。")?,
                    )
                };
                self.default_draft
                    .as_mut()
                    .ok_or("请先选择班级和科目。")?
                    .selected = selected;
            }
            return Ok(());
        }
        if !matches!(self.page, Page::Edit | Page::Scopes) {
            return Ok(());
        }
        let Some(draft) = &mut self.draft else {
            return Ok(());
        };
        let mut next = draft.clone();
        if self.page == Page::Edit {
            if let Some(value) = data.get(LOGIN) {
                if !next.creating && value != next.login {
                    return Err("编辑账号不能更改登录名。".into());
                }
                if value.encode_utf16().count() > 128 || value.contains('\0') {
                    return Err("登录名输入超过界面容量。".into());
                }
                if next.creating {
                    next.login = value.into();
                }
            }
            if let Some(value) = data.get(NAME) {
                if value.encode_utf16().count() > 256 || value.contains('\0') {
                    return Err("姓名输入超过界面容量。".into());
                }
                next.name = value.into();
            }
            let pending_role = if let Some(value) = data.get(ROLE) {
                parse_role(value)?
            } else {
                self.pending_role
            };
            if let Some(value) = data.get(ENABLED) {
                next.enabled = match value {
                    "1" => true,
                    "0" if !next.creating => false,
                    _ => return Err("请选择启用或停用。".into()),
                };
            }
            self.pending_role = pending_role;
        }
        if self.page == Page::Scopes {
            if let Some(value) = data.get(CLASS) {
                self.selected_class = if value.is_empty() {
                    None
                } else {
                    Some(
                        value
                            .parse::<u64>()
                            .ok()
                            .and_then(|n| ClassId::new(n).ok())
                            .ok_or("请选择有效班级。")?,
                    )
                };
            }
            if let Some(value) = data.get(SUBJECT) {
                self.selected_subject = if value.is_empty() {
                    None
                } else {
                    Some(
                        value
                            .parse::<u64>()
                            .ok()
                            .and_then(|n| SubjectId::new(n).ok())
                            .ok_or("请选择有效科目。")?,
                    )
                };
            }
        }
        *draft = next;
        Ok(())
    }
    pub(super) fn suspend(&mut self, data: FormData) {
        let _ = self.capture(&data);
        if self.page == Page::Transfer
            || self.page == Page::Discard && self.discard_return == Page::Transfer
        {
            self.page = Page::List;
            self.selected_account = None;
            self.submitted = false;
        }
    }
    pub(super) fn transfer_rejected(&mut self) {
        if self.page == Page::Transfer {
            self.submitted = false;
        }
    }
    fn transfer_target<'a>(&self, state: &'a IdentityState) -> Result<&'a UserAccount, String> {
        self.fresh(state)?;
        let target = state
            .account(self.selected_account.ok_or("请先选择接任账号。")?)
            .map_err(problem)?;
        if target.principal.id == self.owner
            || !target.principal.enabled
            || target.principal.role == Role::GlobalAdmin
            || target.must_change_password
        {
            return Err("接任账号须为另一位已启用、已完成首次改密的教师或用户。".into());
        }
        Ok(target)
    }
    fn apply_role(&mut self) -> bool {
        let Some(draft) = &mut self.draft else {
            return false;
        };
        if self.pending_role == Role::User && !draft.scopes.is_empty() {
            self.page = Page::ClearScopes;
            return false;
        }
        draft.role = self.pending_role;
        true
    }
    fn command(&self) -> Result<IdentityCommand, String> {
        let draft = self.draft.as_ref().ok_or("请先选择账号。")?;
        if draft.creating {
            Ok(IdentityCommand::CreateAccount(NewAccount {
                id: draft.id,
                login_name: draft.login.clone(),
                display_name: draft.name.clone(),
                role: draft.role,
                scopes: draft.scopes.clone(),
            }))
        } else {
            Ok(IdentityCommand::UpdateAccount {
                id: draft.id,
                display_name: draft.name.clone(),
                role: draft.role,
                enabled: draft.enabled,
                scopes: draft.scopes.clone(),
            })
        }
    }
    fn preflight(&self, state: &IdentityState) -> Result<(), String> {
        self.preflight_command(state, &self.command()?)
    }
    fn default_command(&self) -> Result<IdentityCommand, String> {
        let draft = self.default_draft.as_ref().ok_or("请先选择班级和科目。")?;
        Ok(IdentityCommand::SetDefaultTeacher(DefaultTeacher {
            class_id: draft.class_id,
            subject_id: draft.subject_id,
            teacher_id: draft
                .selected
                .ok_or("此班此科目没有选定的有效任课教师；请先在账号管理配置任教范围。")?,
        }))
    }
    fn preflight_command(
        &self,
        state: &IdentityState,
        command: &IdentityCommand,
    ) -> Result<(), String> {
        self.fresh(state)?;
        // Preview only: fixed civil instants remove time/transport concerns from
        // this pure rule check. This context never leaves the editor and is never
        // submitted as authentication evidence. Backend must use the real lease.
        let now = LocalDateTime::new(LocalDate::new(2000, 1, 1).map_err(|_| "预检日期无效。")?, 0)
            .map_err(|_| "预检日期无效。")?;
        let expires_at = LocalDateTime::new(now.date, 1).map_err(|_| "预检日期无效。")?;
        let owner = state.account(self.owner).map_err(problem)?;
        let actor = ActorContext {
            principal_id: self.owner,
            school_id: owner.principal.school_id,
            auth_epoch: owner.principal.auth_epoch,
            expires_at,
            origin: TrustedOrigin::Local,
        };
        state
            .transition(command, Some(&actor), now)
            .map_err(problem)?;
        Ok(())
    }
    fn depart(
        &mut self,
        destination: Destination,
        state: &IdentityState,
    ) -> Result<Command, String> {
        if self.has_unsaved_changes() {
            self.discard_return = self.page;
            self.destination = destination;
            self.page = Page::Discard;
            return Ok(Command::Stay);
        }
        self.finish_departure(destination, state)
    }
    fn finish_departure(
        &mut self,
        destination: Destination,
        _state: &IdentityState,
    ) -> Result<Command, String> {
        match destination {
            Destination::Close | Destination::Reload => {
                self.draft = None;
                self.original = None;
                self.default_draft = None;
                self.page = Page::List;
                self.submitted = false;
                Ok(if matches!(destination, Destination::Close) {
                    Command::Close
                } else {
                    Command::Reload
                })
            }
            Destination::List => {
                self.draft = None;
                self.original = None;
                self.default_draft = None;
                self.page = Page::List;
                Ok(Command::Stay)
            }
        }
    }
    pub(super) fn select(
        &mut self,
        row: usize,
        column: usize,
        state: &IdentityState,
    ) -> Result<(), String> {
        authorized(state, self.owner)?;
        match self.page {
            Page::List if column < 4 => {
                let index = self
                    .account_page
                    .checked_mul(PAGE_SIZE)
                    .and_then(|n| n.checked_add(row))
                    .ok_or("账号选择超出范围。")?;
                if row >= PAGE_SIZE {
                    return Err("账号选择超出范围。".into());
                }
                self.selected_account = Some(
                    self.accounts
                        .get(index)
                        .ok_or("账号选择超出范围。")?
                        .principal
                        .id,
                );
                Ok(())
            }
            Page::Scopes if column < 2 => {
                let index = self
                    .scope_page
                    .checked_mul(PAGE_SIZE)
                    .and_then(|n| n.checked_add(row))
                    .ok_or("任教范围选择超出范围。")?;
                if row >= PAGE_SIZE {
                    return Err("任教范围选择超出范围。".into());
                }
                self.selected_scope = Some(
                    *self
                        .draft
                        .as_ref()
                        .and_then(|d| d.scopes.get(index))
                        .ok_or("任教范围选择超出范围。")?,
                );
                Ok(())
            }
            _ => Err("此页不能选择账号或任教范围。".into()),
        }
    }
    pub(super) fn handle(
        &mut self,
        action: ActionId,
        mut data: FormData,
        state: &IdentityState,
    ) -> Result<Command, String> {
        authorized(state, self.owner)?;
        self.capture(&data)?;
        if action == CLOSE {
            return self.depart(Destination::Close, state);
        }
        if action == RELOAD {
            return self.depart(Destination::Reload, state);
        }
        match (self.page, action) {
            (Page::List, TRANSFER) => {
                self.transfer_target(state)?;
                self.submitted = false;
                self.page = Page::Transfer;
            }
            (Page::Transfer, RETURN) if !self.submitted => {
                self.page = Page::List;
            }
            (Page::Transfer, CONFIRM) if !self.submitted => {
                let target = self.transfer_target(state)?.principal.id;
                let expected_credential_version = state
                    .account(self.owner)
                    .map_err(problem)?
                    .credential_version;
                let password =
                    Password::new(data.take(PASSWORD).ok_or("请输入你当前账号的口令。")?)
                        .map_err(|_| "当前口令须为12至128字。")?;
                self.submitted = true;
                return Ok(Command::Transfer {
                    expected: self.expected,
                    from: self.owner,
                    target,
                    expected_credential_version,
                    password,
                });
            }
            (Page::List, DEFAULTS) => {
                self.fresh(state)?;
                self.default_draft = None;
                self.selected_class = state.classes.iter().find(|c| c.enabled).map(|c| c.id);
                self.selected_subject = state.subjects.iter().find(|s| s.enabled).map(|s| s.id);
                self.page = Page::DefaultScope;
                self.submitted = false;
            }
            (Page::DefaultScope, DEFAULT_SELECT) => {
                self.fresh(state)?;
                let class_id = self.selected_class.ok_or("请选择已启用班级。")?;
                let subject_id = self.selected_subject.ok_or("请选择已启用科目。")?;
                if !state.classes.iter().any(|c| c.id == class_id && c.enabled)
                    || !state
                        .subjects
                        .iter()
                        .any(|s| s.id == subject_id && s.enabled)
                {
                    return Err("默认老师只能配置到已启用的班级和科目。".into());
                }
                let original = state
                    .default_teachers
                    .iter()
                    .find(|d| d.class_id == class_id && d.subject_id == subject_id)
                    .map(|d| d.teacher_id);
                self.default_draft = Some(DefaultDraft {
                    class_id,
                    subject_id,
                    original,
                    selected: original,
                });
                self.page = Page::DefaultEdit;
            }
            (Page::DefaultScope, CANCEL) | (Page::DefaultEdit, CANCEL) => {
                return self.depart(Destination::List, state)
            }
            (Page::DefaultEdit, REVIEW) => {
                let command = self.default_command()?;
                self.preflight_command(state, &command)?;
                if !self.has_unsaved_changes() {
                    return Err("默认老师没有变化，无需保存。".into());
                }
                self.page = Page::DefaultConfirm;
                self.submitted = false;
            }
            (Page::DefaultConfirm, RETURN) if !self.submitted => self.page = Page::DefaultEdit,
            (Page::DefaultConfirm, CONFIRM) if !self.submitted => {
                let command = self.default_command()?;
                self.preflight_command(state, &command)?;
                self.submitted = true;
                return Ok(Command::Update {
                    expected: self.expected,
                    command,
                });
            }
            (Page::List, NEW) => {
                self.begin(state, None)?;
            }
            (Page::List, EDIT) => {
                self.begin(
                    state,
                    Some(self.selected_account.ok_or("请先选择账号行。")?),
                )?;
            }
            (Page::List, NEXT) => {
                if (self.account_page + 1) * PAGE_SIZE < self.accounts.len() {
                    self.account_page += 1;
                    self.selected_account = None;
                }
            }
            (Page::List, PREVIOUS) => {
                self.account_page = self.account_page.saturating_sub(1);
                self.selected_account = None;
            }
            (Page::Edit, APPLY_ROLE) => {
                self.apply_role();
            }
            (Page::Edit, SCOPES) => {
                if self.apply_role() {
                    self.page = Page::Scopes;
                }
            }
            (Page::Edit, REVIEW) => {
                if self.apply_role() {
                    self.preflight(state)?;
                    self.page = Page::Confirm;
                    self.submitted = false;
                }
            }
            (Page::Edit, CANCEL) => return self.depart(Destination::List, state),
            (Page::Scopes, ADD_SCOPE) => {
                self.fresh(state)?;
                let draft = self.draft.as_mut().ok_or("没有账号草稿。")?;
                if draft.role == Role::User {
                    return Err("普通用户不能设置任教范围。".into());
                }
                let scope = AssignmentScope {
                    class_id: self.selected_class.ok_or("请选择班级。")?,
                    subject_id: self.selected_subject.ok_or("请选择科目。")?,
                };
                if !state
                    .classes
                    .iter()
                    .any(|c| c.id == scope.class_id && c.enabled)
                    || !state
                        .subjects
                        .iter()
                        .any(|s| s.id == scope.subject_id && s.enabled)
                {
                    return Err("新增范围需要启用的班级与科目。".into());
                }
                if draft.scopes.contains(&scope) {
                    return Err("此班级与科目范围已存在。".into());
                }
                let other = state
                    .assignments
                    .iter()
                    .filter(|s| s.teacher_id != draft.id)
                    .count();
                if draft.scopes.len() >= MAX_ASSIGNMENTS
                    || other.saturating_add(draft.scopes.len()) >= MAX_ASSIGNMENTS
                {
                    return Err(problem(IdentityError::Capacity));
                }
                draft.scopes.push(scope);
                self.scope_page = (draft.scopes.len() - 1) / PAGE_SIZE;
                self.selected_scope = Some(scope);
            }
            (Page::Scopes, REMOVE_SCOPE) => {
                let selected = self.selected_scope.ok_or("请先选择要移除的范围行。")?;
                let draft = self.draft.as_mut().ok_or("没有账号草稿。")?;
                let index = draft
                    .scopes
                    .iter()
                    .position(|s| *s == selected)
                    .ok_or("范围选择已改变。")?;
                draft.scopes.remove(index);
                self.selected_scope = None;
                self.scope_page = self
                    .scope_page
                    .min(draft.scopes.len().saturating_sub(1) / PAGE_SIZE);
            }
            (Page::Scopes, NEXT) => {
                if self
                    .draft
                    .as_ref()
                    .is_some_and(|d| (self.scope_page + 1) * PAGE_SIZE < d.scopes.len())
                {
                    self.scope_page += 1;
                    self.selected_scope = None;
                }
            }
            (Page::Scopes, PREVIOUS) => {
                self.scope_page = self.scope_page.saturating_sub(1);
                self.selected_scope = None;
            }
            (Page::Scopes, RETURN) => self.page = Page::Edit,
            (Page::ClearScopes, CLEAR_SCOPES) => {
                if let Some(draft) = &mut self.draft {
                    draft.scopes.clear();
                    draft.role = Role::User;
                }
                self.pending_role = Role::User;
                self.selected_scope = None;
                self.scope_page = 0;
                self.page = Page::Edit;
            }
            (Page::ClearScopes, KEEP) => {
                self.pending_role = self.draft.as_ref().map_or(Role::User, |d| d.role);
                self.page = Page::Edit;
            }
            (Page::Confirm, RETURN) | (Page::Password, RETURN) => {
                self.page = Page::Edit;
                self.submitted = false;
            }
            (Page::Confirm, CONFIRM) if !self.submitted => {
                self.preflight(state)?;
                if self.draft.as_ref().is_some_and(|d| d.creating) {
                    self.page = Page::Password;
                } else {
                    self.submitted = true;
                    return Ok(Command::Update {
                        expected: self.expected,
                        command: self.command()?,
                    });
                }
            }
            (Page::Password, CONFIRM) if !self.submitted => {
                self.preflight(state)?;
                if data.get(PASSWORD) != data.get(CONFIRM_PASSWORD) || data.get(PASSWORD).is_none()
                {
                    return Err("两次初始口令不一致；请重新输入。".into());
                }
                let password = Password::new(data.take(PASSWORD).ok_or("请输入初始口令。")?)
                    .map_err(|_| "初始口令须为12至128字。")?;
                let IdentityCommand::CreateAccount(account) = self.command()? else {
                    return Err("此页仅能创建账号。".into());
                };
                self.submitted = true;
                return Ok(Command::Create {
                    expected: self.expected,
                    account,
                    password,
                });
            }
            (Page::Discard, KEEP) => self.page = self.discard_return,
            (Page::Discard, DISCARD) => return self.finish_departure(self.destination, state),
            _ => return Err("此页不支持该操作。".into()),
        }
        Ok(Command::Stay)
    }
    pub(super) fn view(&self, state: &IdentityState) -> View {
        let mut view = View {
            title: "账号与任教范围".into(),
            description: String::new(),
            status: String::new(),
            fields: Vec::new(),
            actions: Vec::new(),
            busy: false,
            grid: None,
        };
        if let Err(message) = authorized(state, self.owner) {
            view.status = message;
            return view;
        }
        if state.revision != self.expected {
            view.status = "目录已更新；草稿保留，提交将拒绝旧修订。请取消并重新加载核对。".into();
        }
        if self.page == Page::List {
            let start = self.account_page * PAGE_SIZE;
            let rows = self
                .accounts
                .iter()
                .skip(start)
                .take(PAGE_SIZE)
                .map(|a| GridRow {
                    group: String::new(),
                    label: caption(&a.display_name, a.principal.id.get(), a.principal.enabled),
                    cells: vec![
                        a.login_name.clone(),
                        role_name(a.principal.role).into(),
                        if a.principal.enabled {
                            "启用"
                        } else {
                            "停用"
                        }
                        .into(),
                    ],
                    merged: false,
                })
                .collect();
            let selected = self
                .selected_account
                .and_then(|id| {
                    self.accounts
                        .iter()
                        .skip(start)
                        .take(PAGE_SIZE)
                        .position(|a| a.principal.id == id)
                })
                .map(|r| (r, 0));
            view.description = format!(
                "共{}个账号，第{}页，每页最多20条。按行选择稳定账号编号后编辑；没有删除或重置他人口令入口。",
                self.accounts.len(),
                self.account_page + 1
            );
            view.grid = Some(Grid {
                columns: vec![
                    "姓名 / 编号".into(),
                    "登录名".into(),
                    "角色".into(),
                    "状态".into(),
                ],
                rows,
                selected,
                enabled: true,
            });
            view.actions = vec![
                action(NEW, "创建账号", self.accounts.len() < MAX_ACCOUNTS),
                action(EDIT, "编辑选中账号", self.selected_account.is_some()),
                action(PREVIOUS, "上一页", self.account_page > 0),
                action(NEXT, "下一页", start + PAGE_SIZE < self.accounts.len()),
                action(DEFAULTS, "班级／科目默认老师", true),
                action(
                    TRANSFER,
                    "交接主管理员给选中账号",
                    self.transfer_target(state).is_ok(),
                ),
                action(RELOAD, "重新加载", true),
                action(CLOSE, "返回", true),
            ];
            return view;
        }
        if self.page == Page::Transfer {
            view.title = "主管理员交接 · 再次验证".into();
            match (state.account(self.owner), self.transfer_target(state)) {
                (Ok(from), Ok(target)) => {
                    let after = if state.assignments.iter().any(|a| a.teacher_id == self.owner) {
                        "教师管理员"
                    } else {
                        "普通用户"
                    };
                    view.description = format!(
                        "你正在把主管理员权限从{}（{}，#{}）交给{}（{}，#{}）。\n接任账号将成为主管理员；你的账号将变为{after}。双方任教范围、登录名、口令和恢复码保留，课表与默认老师不变。\n交接会一次生效，并使双方原有本地和教师网页会话失效；完成后须重新登录。其他主管理员不受影响。请核对接任人，再输入你当前账号的口令确认。",
                        from.display_name, from.login_name, self.owner.get(),
                        target.display_name, target.login_name, target.principal.id.get()
                    );
                    if !self.submitted {
                        view.fields.push(field(
                            PASSWORD,
                            "你当前账号的口令",
                            FieldKind::Password,
                            String::new(),
                            256,
                        ));
                    } else {
                        view.description.push_str(
                            "\n本次已送交后台。若结果不确定，请查询原交接记录，不要重复确认。",
                        );
                    }
                }
                (_, Err(message)) => view.status = message,
                _ => view.status = "当前账号已变化，请重新登录。".into(),
            }
            view.actions = vec![
                action(
                    CONFIRM,
                    "验证口令并交接",
                    !self.submitted && self.transfer_target(state).is_ok(),
                ),
                action(RETURN, "取消交接", !self.submitted),
                action(RELOAD, "重新读取并核对原结果", true),
                action(CLOSE, "返回", true),
            ];
            return view;
        }
        if matches!(
            self.page,
            Page::DefaultScope | Page::DefaultEdit | Page::DefaultConfirm
        ) {
            return self.default_view(state, view);
        }
        if self.page == Page::Discard && self.default_draft.is_some() {
            view.title = "默认老师草稿".into();
            view.description = if self.submitted {
                "本次已经提交。离开只丢弃本页选择，不撤销已提交操作，也不证明它未保存；原票据仍由工作台保留，重新加载时核对原操作结果。"
            } else { "默认老师选择尚未保存。确认丢弃后返回；保留则继续当前选择。" }.into();
            view.actions = vec![
                action(DISCARD, "确认离开此草稿", true),
                action(KEEP, "保留并继续", true),
            ];
            return view;
        }
        if self.page == Page::Discard && self.discard_return == Page::Transfer {
            view.title = "取消主管理员交接复核".into();
            view.description = "尚未确认交接；离开将丢弃这次复核，双方权限不变。".into();
            view.actions = vec![
                action(DISCARD, "确认离开", true),
                action(KEEP, "继续核对", true),
            ];
            return view;
        }
        let Some(draft) = &self.draft else {
            view.status = "没有账号草稿，请重新加载。".into();
            return view;
        };
        match self.page {
            Page::Edit => {
                view.description = format!(
                    "{} #{}。创建的账号首次登录必须改密；姓名、角色、启用状态与任教范围的修改需要确认。登录名与现有凭据不能在编辑中更改。",
                    if draft.creating {
                        "创建账号"
                    } else {
                        "编辑账号"
                    },
                    draft.id.get()
                );
                view.fields = vec![
                    field(
                        LOGIN,
                        "登录名",
                        if draft.creating {
                            FieldKind::Text
                        } else {
                            FieldKind::ReadOnly
                        },
                        draft.login.clone(),
                        128,
                    ),
                    field(NAME, "姓名", FieldKind::Text, draft.name.clone(), 256),
                    choice(
                        ROLE,
                        "三级角色",
                        role_value(self.pending_role).into(),
                        [
                            ("global", "主管理员"),
                            ("teacher", "教师管理员"),
                            ("user", "普通用户"),
                        ]
                        .iter()
                        .map(|(value, label)| Choice {
                            value: (*value).into(),
                            label: (*label).into(),
                        })
                        .collect(),
                    ),
                    choice(
                        ENABLED,
                        "状态",
                        if draft.enabled { "1" } else { "0" }.into(),
                        if draft.creating {
                            vec![Choice {
                                value: "1".into(),
                                label: "启用（新账号）".into(),
                            }]
                        } else {
                            vec![
                                Choice {
                                    value: "1".into(),
                                    label: "启用".into(),
                                },
                                Choice {
                                    value: "0".into(),
                                    label: "停用".into(),
                                },
                            ]
                        },
                    ),
                ];
                view.status.push_str(&format!(
                    "任教范围{}条。{}",
                    draft.scopes.len(),
                    if self.has_unsaved_changes() {
                        "有未保存更改。"
                    } else {
                        "尚无更改。"
                    }
                ));
                view.actions = vec![
                    action(APPLY_ROLE, "应用角色选择", true),
                    action(SCOPES, "逐条编辑任教范围", true),
                    action(REVIEW, "查看变更摘要", true),
                    action(CANCEL, "返回账号列表", true),
                    action(CLOSE, "返回", true),
                ];
            }
            Page::Scopes => {
                view.description = format!(
                    "账号 #{}：共{}条范围，第{}页。新增只可选择启用目录；已有停用项保留并标记，可明确移除。",
                    draft.id.get(),
                    draft.scopes.len(),
                    self.scope_page + 1
                );
                let classes: Vec<_> = state
                    .classes
                    .iter()
                    .filter(|c| c.enabled)
                    .map(|c| Choice {
                        value: c.id.get().to_string(),
                        label: caption(&c.name, c.id.get(), true),
                    })
                    .collect();
                let subjects: Vec<_> = state
                    .subjects
                    .iter()
                    .filter(|s| s.enabled)
                    .map(|s| Choice {
                        value: s.id.get().to_string(),
                        label: caption(&s.name, s.id.get(), true),
                    })
                    .collect();
                let selected_class = self
                    .selected_class
                    .map(|id| id.get().to_string())
                    .filter(|id| classes.iter().any(|c| c.value == *id))
                    .unwrap_or_default();
                let selected_subject = self
                    .selected_subject
                    .map(|id| id.get().to_string())
                    .filter(|id| subjects.iter().any(|s| s.value == *id))
                    .unwrap_or_default();
                view.fields = vec![
                    choice(CLASS, "新增范围：班级", selected_class, classes),
                    choice(SUBJECT, "新增范围：科目", selected_subject, subjects),
                ];
                let rows = draft
                    .scopes
                    .iter()
                    .skip(self.scope_page * PAGE_SIZE)
                    .take(PAGE_SIZE)
                    .map(|s| GridRow {
                        group: String::new(),
                        label: state
                            .classes
                            .iter()
                            .find(|c| c.id == s.class_id)
                            .map_or_else(
                                || format!("#{}（已不可用）", s.class_id.get()),
                                |c| caption(&c.name, c.id.get(), c.enabled),
                            ),
                        cells: vec![state
                            .subjects
                            .iter()
                            .find(|x| x.id == s.subject_id)
                            .map_or_else(
                                || format!("#{}（已不可用）", s.subject_id.get()),
                                |x| caption(&x.name, x.id.get(), x.enabled),
                            )],
                        merged: false,
                    })
                    .collect();
                let selected = self
                    .selected_scope
                    .and_then(|s| {
                        draft
                            .scopes
                            .iter()
                            .skip(self.scope_page * PAGE_SIZE)
                            .take(PAGE_SIZE)
                            .position(|x| *x == s)
                    })
                    .map(|r| (r, 0));
                view.grid = Some(Grid {
                    columns: vec!["班级 / 编号".into(), "科目 / 编号".into()],
                    rows,
                    selected,
                    enabled: true,
                });
                view.actions = vec![
                    action(ADD_SCOPE, "添加这一范围", draft.role != Role::User),
                    action(REMOVE_SCOPE, "移除选中范围", self.selected_scope.is_some()),
                    action(PREVIOUS, "上一页", self.scope_page > 0),
                    action(
                        NEXT,
                        "下一页",
                        (self.scope_page + 1) * PAGE_SIZE < draft.scopes.len(),
                    ),
                    action(RETURN, "返回账号草稿", true),
                    action(CLOSE, "返回", true),
                ];
            }
            Page::Confirm | Page::Password => {
                let old = self.original.as_ref();
                let added = draft
                    .scopes
                    .iter()
                    .filter(|s| old.is_none_or(|o| !o.scopes.contains(s)))
                    .count();
                let removed = old.map_or(0, |o| {
                    o.scopes
                        .iter()
                        .filter(|s| !draft.scopes.contains(s))
                        .count()
                });
                view.description = format!(
                    "确认{}账号 #{}\n登录名：{}\n姓名：{} → {}\n角色：{} → {}\n状态：{} → {}\n任教范围：{}条，新增{}、移除{}。\n将使用进入草稿时的目录修订{}；发生冲突会保留草稿，不自动重试。变更会撤销该账号旧会话；修改自己后需要重新登录。",
                    if draft.creating { "创建" } else { "更新" },
                    draft.id.get(),
                    draft.login,
                    old.map_or("", |o| o.name.as_str()),
                    draft.name,
                    old.map_or("", |o| role_name(o.role)),
                    role_name(draft.role),
                    old.map_or("", |o| if o.enabled { "启用" } else { "停用" }),
                    if draft.enabled { "启用" } else { "停用" },
                    draft.scopes.len(),
                    added,
                    removed,
                    self.expected.get()
                );
                if self.page == Page::Password {
                    view.description.push_str("\n最后一步：设置12至128字的初始口令。翻页、返回或锁定会清空口令；编辑器不会缓存。新账号首次登录必须改密。");
                    view.fields = vec![
                        field(
                            PASSWORD,
                            "初始口令",
                            FieldKind::Password,
                            String::new(),
                            256,
                        ),
                        field(
                            CONFIRM_PASSWORD,
                            "再次输入初始口令",
                            FieldKind::Password,
                            String::new(),
                            256,
                        ),
                    ];
                }
                view.actions = vec![
                    action(
                        CONFIRM,
                        if self.page == Page::Password {
                            "创建账号"
                        } else if draft.creating {
                            "确认摘要，输入初始口令"
                        } else {
                            "确认提交变更"
                        },
                        !self.submitted,
                    ),
                    action(RETURN, "返回继续编辑", true),
                    action(CLOSE, "返回", true),
                ];
            }
            Page::ClearScopes => {
                view.description = format!(
                    "普通用户不能任教。是否明确清除当前{}条班级与科目范围，并改为普通用户？当前草稿尚未保存。",
                    draft.scopes.len()
                );
                view.actions = vec![
                    action(CLEAR_SCOPES, "确认清空范围并改为普通用户", true),
                    action(KEEP, "保留原角色与范围", true),
                ];
            }
            Page::Discard => {
                view.description = "有未保存的账号草稿。是否丢弃？返回继续编辑可保留姓名、角色与范围；口令始终不会保留。".into();
                view.actions = vec![
                    action(DISCARD, "确认丢弃草稿", true),
                    action(KEEP, "保留并继续编辑", true),
                ];
            }
            Page::List
            | Page::DefaultScope
            | Page::DefaultEdit
            | Page::DefaultConfirm
            | Page::Transfer => {}
        }
        view
    }
    fn default_view(&self, state: &IdentityState, mut view: View) -> View {
        view.title = "班级／科目默认老师".into();
        if self.page == Page::DefaultScope {
            let classes: Vec<_> = state
                .classes
                .iter()
                .filter(|c| c.enabled)
                .map(|c| Choice {
                    value: c.id.get().to_string(),
                    label: caption(&c.name, c.id.get(), true),
                })
                .collect();
            let subjects: Vec<_> = state
                .subjects
                .iter()
                .filter(|s| s.enabled)
                .map(|s| Choice {
                    value: s.id.get().to_string(),
                    label: caption(&s.name, s.id.get(), true),
                })
                .collect();
            view.description = "先选择班级和科目，再核对当前默认老师。此设置只用于课表明确选择“默认老师”的课程；显式老师不变，临时换课不改此设置。".into();
            let class_value = self
                .selected_class
                .map(|id| id.get().to_string())
                .filter(|id| classes.iter().any(|c| c.value == *id))
                .unwrap_or_default();
            let subject_value = self
                .selected_subject
                .map(|id| id.get().to_string())
                .filter(|id| subjects.iter().any(|s| s.value == *id))
                .unwrap_or_default();
            view.fields = vec![
                choice(CLASS, "班级", class_value, classes),
                choice(SUBJECT, "科目", subject_value, subjects),
            ];
            view.actions = vec![
                action(
                    DEFAULT_SELECT,
                    "查看当前默认老师",
                    self.selected_class.is_some() && self.selected_subject.is_some(),
                ),
                action(CANCEL, "返回账号列表", true),
            ];
            return view;
        }
        let Some(draft) = &self.default_draft else {
            view.status = "请重新加载后选择班级和科目。".into();
            return view;
        };
        let class = state
            .classes
            .iter()
            .find(|c| c.id == draft.class_id)
            .map_or_else(
                || format!("#{}", draft.class_id.get()),
                |c| caption(&c.name, c.id.get(), c.enabled),
            );
        let subject = state
            .subjects
            .iter()
            .find(|s| s.id == draft.subject_id)
            .map_or_else(
                || format!("#{}", draft.subject_id.get()),
                |s| caption(&s.name, s.id.get(), s.enabled),
            );
        let teacher = |id: Option<PrincipalId>| {
            id.map_or_else(
                || "未设置默认老师".into(),
                |id| {
                    state.account(id).map_or_else(
                        |_| format!("#{}, 当前不可用", id.get()),
                        |a| {
                            format!(
                                "{}（{}）",
                                caption(&a.display_name, id.get(), a.principal.enabled),
                                a.login_name
                            )
                        },
                    )
                },
            )
        };
        view.description = format!(
            "班级：{class}\n科目：{subject}\n进入本页时的默认老师：{}",
            teacher(draft.original)
        );
        if self.page == Page::DefaultEdit {
            let candidates: Vec<_> = state
                .accounts
                .iter()
                .filter(|a| state.is_teacher(a.principal.id, draft.class_id, draft.subject_id))
                .map(|a| Choice {
                    value: a.principal.id.get().to_string(),
                    label: shorten(&teacher(Some(a.principal.id)), 500),
                })
                .collect();
            let selected = draft
                .selected
                .map(|id| id.get().to_string())
                .filter(|id| candidates.iter().any(|c| c.value == *id))
                .unwrap_or_default();
            let available = !candidates.is_empty();
            if !available {
                view.description.push_str(
                    "\n此班此科目没有有效任课教师。请取消后在账号与任教管理添加或启用相应范围。",
                );
            }
            view.description.push_str("\n只显示有效任课教师，编号区分同名账号。选择和取消不会写入；保存须再次确认。可设置或更换默认老师。");
            view.fields = vec![choice(
                DEFAULT_TEACHER,
                "选定默认老师",
                selected,
                candidates,
            )];
            view.actions = vec![
                action(REVIEW, "查看变更摘要", available),
                action(CANCEL, "取消并返回账号列表", true),
                action(RELOAD, "重新加载", true),
            ];
        } else {
            view.description.push_str(&format!(
                "\n拟设默认老师：{}\n确认后保存以上更改。如果设置已被更新，请重新读取后核对。",
                teacher(draft.selected)
            ));
            if self.submitted {
                view.description
                    .push_str("\n本次已经提交；结果不确定时，请核对原操作与当前状态，不重复保存。");
            }
            view.actions = vec![
                action(CONFIRM, "确认保存默认老师", !self.submitted),
                action(RETURN, "返回继续编辑", !self.submitted),
                action(RELOAD, "核对原操作与当前状态", true),
                action(CLOSE, "返回", true),
            ];
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn administrator_transfer_review_clears_secrets_and_expires_on_lock() {
        let state = state();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        editor.select(1, 0, &state).unwrap();
        editor.handle(TRANSFER, FormData::new(), &state).unwrap();
        let review = editor.view(&state);
        review.validate().unwrap();
        assert!(review.description.contains("双方任教范围"));
        assert_eq!(review.fields.len(), 1);
        assert_eq!(review.fields[0].kind, FieldKind::Password);
        assert!(review.fields[0].value.is_empty());
        let command = editor
            .handle(
                CONFIRM,
                form(&[(PASSWORD, "synthetic-transfer-password")]),
                &state,
            )
            .unwrap();
        assert!(
            matches!(command, Command::Transfer { expected, from, target, expected_credential_version: 1, .. }
            if expected == state.revision && from == id(1) && target == id(2))
        );
        assert!(editor.view(&state).fields.is_empty());
        assert!(editor.handle(CONFIRM, FormData::new(), &state).is_err());
        editor.suspend(form(&[(PASSWORD, "synthetic-transfer-password")]));
        assert_eq!(editor.page, Page::List);
        assert!(editor.selected_account.is_none());
        editor.select(2, 0, &state).unwrap();
        editor.handle(TRANSFER, FormData::new(), &state).unwrap();
        editor.handle(CLOSE, FormData::new(), &state).unwrap();
        assert_eq!(editor.page, Page::Discard);
        editor.view(&state).validate().unwrap();
        editor.handle(KEEP, FormData::new(), &state).unwrap();
        assert_eq!(editor.page, Page::Transfer);
        let mut changed = state.clone();
        changed.revision = changed.revision.next().unwrap();
        assert!(editor
            .handle(
                CONFIRM,
                form(&[(PASSWORD, "synthetic-transfer-password")]),
                &changed
            )
            .is_err());
        assert!(editor
            .view(&changed)
            .fields
            .iter()
            .all(|f| f.value.is_empty()));
    }
    fn id(n: u64) -> PrincipalId {
        PrincipalId::new(n).unwrap()
    }
    fn class(n: u64) -> ClassId {
        ClassId::new(n).unwrap()
    }
    fn subject(n: u64) -> SubjectId {
        SubjectId::new(n).unwrap()
    }
    fn scope(c: u64, s: u64) -> AssignmentScope {
        AssignmentScope {
            class_id: class(c),
            subject_id: subject(s),
        }
    }
    fn form(values: &[(FieldId, &str)]) -> FormData {
        let mut data = FormData::new();
        for &(id, value) in values {
            data.insert(id, value.into()).unwrap();
        }
        data
    }
    fn state() -> IdentityState {
        let school = SchoolId::new(1).unwrap();
        let account = |n, role| UserAccount {
            principal: Principal {
                id: id(n),
                school_id: school,
                role,
                enabled: true,
                auth_epoch: AuthEpoch::new(1),
            },
            login_name: format!("account{n}"),
            display_name: format!("姓名{n}"),
            must_change_password: false,
            credential_version: 1,
            recovery_version: 1,
        };
        let state = IdentityState {
            revision: Revision::new(9),
            installation: Some(Installation {
                school_id: school,
                school_name: "测试学校".into(),
                current_class: Some(class(1)),
            }),
            accounts: vec![
                account(1, Role::GlobalAdmin),
                account(2, Role::TeacherAdmin),
                account(3, Role::User),
            ],
            classes: (1..=2)
                .map(|n| SchoolClass {
                    id: class(n),
                    school_id: school,
                    name: format!("班级{n}"),
                    enabled: true,
                })
                .collect(),
            subjects: (1..=2)
                .map(|n| SchoolSubject {
                    id: subject(n),
                    school_id: school,
                    name: format!("科目{n}"),
                    enabled: true,
                    color: Rgb {
                        red: 1,
                        green: 2,
                        blue: 3,
                    },
                    revision: Revision::new(1),
                })
                .collect(),
            assignments: vec![TeachingAssignment {
                teacher_id: id(2),
                class_id: class(1),
                subject_id: subject(1),
                enabled: true,
            }],
            default_teachers: Vec::new(),
        };
        state.validate().unwrap();
        state
    }
    fn act(
        editor: &mut Editor,
        action: ActionId,
        data: FormData,
        state: &IdentityState,
    ) -> Command {
        editor.handle(action, data, state).unwrap()
    }
    fn edit(editor: &mut Editor, row: usize, state: &IdentityState) {
        editor.select(row, 0, state).unwrap();
        act(editor, EDIT, FormData::new(), state);
    }
    fn validate(editor: &Editor, state: &IdentityState) {
        editor.view(state).validate().unwrap();
    }
    #[test]
    fn real_roles_owner_gate_and_stable_id_selection() {
        let state = state();
        assert!(Editor::new(&state, id(2)).is_err());
        assert!(Editor::new(&state, id(3)).is_err());
        let mut editor = Editor::new(&state, id(1)).unwrap();
        assert_eq!(editor.owner(), id(1));
        edit(&mut editor, 1, &state);
        assert_eq!(editor.draft.as_ref().unwrap().id, id(2));
        assert!(!editor.has_unsaved_changes());
        let mut disabled = state.clone();
        disabled.accounts[0].principal.enabled = false;
        assert!(editor.handle(REVIEW, FormData::new(), &disabled).is_err());
        assert!(editor.view(&disabled).fields.is_empty());
        let mut forced = state;
        forced.accounts[0].must_change_password = true;
        assert!(Editor::new(&forced, id(1)).is_err());
    }
    #[test]
    fn multiple_scopes_explicit_user_clear_and_dirty_discard_keep() {
        let state = state();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        edit(&mut editor, 1, &state);
        act(&mut editor, SCOPES, form(&[(NAME, "新姓名")]), &state);
        act(
            &mut editor,
            ADD_SCOPE,
            form(&[(CLASS, "2"), (SUBJECT, "2")]),
            &state,
        );
        assert_eq!(
            editor.draft.as_ref().unwrap().scopes,
            vec![scope(1, 1), scope(2, 2)]
        );
        assert!(editor.handle(ADD_SCOPE, FormData::new(), &state).is_err());
        editor.select(0, 1, &state).unwrap();
        act(&mut editor, REMOVE_SCOPE, FormData::new(), &state);
        assert_eq!(editor.draft.as_ref().unwrap().scopes, vec![scope(2, 2)]);
        act(&mut editor, RETURN, FormData::new(), &state);
        act(&mut editor, REVIEW, form(&[(ROLE, "user")]), &state);
        assert_eq!(editor.page, Page::ClearScopes);
        assert_eq!(editor.draft.as_ref().unwrap().scopes.len(), 1);
        act(&mut editor, KEEP, FormData::new(), &state);
        assert_eq!(editor.draft.as_ref().unwrap().role, Role::TeacherAdmin);
        act(&mut editor, APPLY_ROLE, form(&[(ROLE, "user")]), &state);
        act(&mut editor, CLEAR_SCOPES, FormData::new(), &state);
        assert!(editor.draft.as_ref().unwrap().scopes.is_empty());
        assert!(editor.has_unsaved_changes());
        act(&mut editor, CLOSE, FormData::new(), &state);
        assert_eq!(editor.page, Page::Discard);
        validate(&editor, &state);
        act(&mut editor, KEEP, FormData::new(), &state);
        assert_eq!(editor.page, Page::Edit);
        assert_eq!(editor.draft.as_ref().unwrap().name, "新姓名");
        act(&mut editor, CANCEL, FormData::new(), &state);
        act(&mut editor, DISCARD, FormData::new(), &state);
        assert_eq!(editor.page, Page::List);
        assert!(!editor.has_unsaved_changes());
    }
    #[test]
    fn last_admin_teacher_scope_and_readonly_login_use_domain_preflight() {
        let state = state();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        edit(&mut editor, 0, &state);
        assert!(editor
            .handle(REVIEW, form(&[(ENABLED, "0")]), &state)
            .unwrap_err()
            .contains("最后"));
        assert_eq!(editor.page, Page::Edit);
        assert!(editor.has_unsaved_changes());
        assert!(editor
            .handle(REVIEW, form(&[(LOGIN, "renamed")]), &state)
            .is_err());
        assert_eq!(editor.draft.as_ref().unwrap().login, "account1");
        let mut editor = Editor::new(&state, id(1)).unwrap();
        edit(&mut editor, 1, &state);
        act(&mut editor, SCOPES, FormData::new(), &state);
        editor.select(0, 0, &state).unwrap();
        act(&mut editor, REMOVE_SCOPE, FormData::new(), &state);
        act(&mut editor, RETURN, FormData::new(), &state);
        assert!(editor
            .handle(REVIEW, FormData::new(), &state)
            .unwrap_err()
            .contains("至少"));
    }
    #[test]
    fn confirmation_holds_cas_backend_failure_allows_explicit_reentry() {
        let state = state();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        edit(&mut editor, 2, &state);
        act(
            &mut editor,
            REVIEW,
            form(&[(NAME, "新普通用户姓名")]),
            &state,
        );
        assert_eq!(editor.page, Page::Confirm);
        validate(&editor, &state);
        let mut changed = state.clone();
        changed.revision = Revision::new(10);
        assert!(editor.handle(CONFIRM, FormData::new(), &changed).is_err());
        assert_eq!(editor.expected, Revision::new(9));
        assert!(editor.has_unsaved_changes());
        match act(&mut editor, CONFIRM, FormData::new(), &state) {
            Command::Update {
                expected,
                command:
                    IdentityCommand::UpdateAccount {
                        id: target,
                        display_name,
                        ..
                    },
            } => {
                assert_eq!(expected, Revision::new(9));
                assert_eq!(target, id(3));
                assert_eq!(display_name, "新普通用户姓名");
            }
            _ => panic!("expected update"),
        }
        assert!(editor.handle(CONFIRM, FormData::new(), &state).is_err());
        act(&mut editor, RETURN, FormData::new(), &state);
        act(&mut editor, REVIEW, FormData::new(), &state);
        assert!(matches!(
            act(&mut editor, CONFIRM, FormData::new(), &state),
            Command::Update { .. }
        ));
        act(&mut editor, RELOAD, FormData::new(), &state);
        assert_eq!(editor.page, Page::Discard);
        assert!(matches!(
            act(&mut editor, DISCARD, FormData::new(), &state),
            Command::Reload
        ));
    }
    #[test]
    fn password_only_final_submit_never_retained_by_navigation_or_suspend() {
        let state = state();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        act(&mut editor, NEW, FormData::new(), &state);
        act(
            &mut editor,
            REVIEW,
            form(&[
                (LOGIN, "new-user"),
                (NAME, "新用户"),
                (ROLE, "user"),
                (PASSWORD, "SHOULD NEVER CACHE"),
            ]),
            &state,
        );
        assert_eq!(editor.page, Page::Confirm);
        assert!(editor.view(&state).fields.is_empty());
        act(&mut editor, CONFIRM, FormData::new(), &state);
        assert_eq!(editor.page, Page::Password);
        validate(&editor, &state);
        editor.suspend(form(&[
            (PASSWORD, "secret initial password"),
            (CONFIRM_PASSWORD, "secret initial password"),
        ]));
        assert!(editor
            .view(&state)
            .fields
            .iter()
            .all(|f| f.value.is_empty()));
        assert!(editor.handle(CONFIRM, FormData::new(), &state).is_err());
        act(
            &mut editor,
            RETURN,
            form(&[(PASSWORD, "secret initial password")]),
            &state,
        );
        assert_eq!(editor.page, Page::Edit);
        act(&mut editor, REVIEW, FormData::new(), &state);
        act(&mut editor, CONFIRM, FormData::new(), &state);
        assert!(editor
            .handle(
                CONFIRM,
                form(&[
                    (PASSWORD, "first secret password"),
                    (CONFIRM_PASSWORD, "other secret password")
                ]),
                &state
            )
            .is_err());
        match act(
            &mut editor,
            CONFIRM,
            form(&[
                (PASSWORD, "new initial password"),
                (CONFIRM_PASSWORD, "new initial password"),
            ]),
            &state,
        ) {
            Command::Create {
                expected,
                account,
                password,
            } => {
                assert_eq!(expected, Revision::new(9));
                assert_eq!(account.id, id(4));
                assert_eq!(account.login_name, "new-user");
                assert_eq!(account.role, Role::User);
                assert!(account.scopes.is_empty());
                assert!(!format!("{password:?}").contains("new initial"));
            }
            _ => panic!("expected create"),
        }
        assert!(editor.has_unsaved_changes());
        assert!(editor
            .view(&state)
            .fields
            .iter()
            .all(|f| f.value.is_empty()));
    }
    #[test]
    fn disabled_catalog_relationships_stay_visible_and_can_be_explicitly_removed() {
        let mut state = state();
        state.installation.as_mut().unwrap().current_class = Some(class(2));
        state.classes[0].enabled = false;
        state.subjects[0].enabled = false;
        state.validate().unwrap();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        edit(&mut editor, 1, &state);
        act(&mut editor, SCOPES, FormData::new(), &state);
        let view = editor.view(&state);
        view.validate().unwrap();
        assert!(view.grid.as_ref().unwrap().rows[0].label.contains("停用"));
        assert!(view.grid.as_ref().unwrap().rows[0].cells[0].contains("停用"));
        assert!(editor
            .handle(ADD_SCOPE, form(&[(CLASS, "1"), (SUBJECT, "1")]), &state)
            .is_err());
        act(
            &mut editor,
            ADD_SCOPE,
            form(&[(CLASS, "2"), (SUBJECT, "2")]),
            &state,
        );
        editor.select(0, 0, &state).unwrap();
        act(&mut editor, REMOVE_SCOPE, FormData::new(), &state);
        act(&mut editor, RETURN, FormData::new(), &state);
        act(&mut editor, REVIEW, FormData::new(), &state);
        assert_eq!(editor.page, Page::Confirm);
    }
    #[test]
    fn empty_active_catalog_does_not_block_removing_retained_scope_or_returning() {
        let mut state = state();
        state.installation.as_mut().unwrap().current_class = None;
        for c in &mut state.classes {
            c.enabled = false;
        }
        for s in &mut state.subjects {
            s.enabled = false;
        }
        state.validate().unwrap();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        edit(&mut editor, 1, &state);
        act(&mut editor, SCOPES, FormData::new(), &state);
        validate(&editor, &state);
        editor.select(0, 0, &state).unwrap();
        act(
            &mut editor,
            REMOVE_SCOPE,
            form(&[(CLASS, ""), (SUBJECT, "")]),
            &state,
        );
        act(
            &mut editor,
            RETURN,
            form(&[(CLASS, ""), (SUBJECT, "")]),
            &state,
        );
        assert_eq!(editor.page, Page::Edit);
        assert!(editor.draft.as_ref().unwrap().scopes.is_empty());
    }
    #[test]
    fn full_capacity_pagination_and_long_unicode_labels_are_valid_views() {
        let mut state = state();
        let school = state.installation.as_ref().unwrap().school_id;
        state.accounts = (1..=512)
            .map(|n| {
                let mut a = state.accounts[2].clone();
                a.principal.id = id(n);
                a.login_name = format!("a{n}");
                a.display_name = format!("{}{:03}", "🦀".repeat(125), n);
                a.principal.role = if n == 1 {
                    Role::GlobalAdmin
                } else if n == 2 {
                    Role::TeacherAdmin
                } else {
                    Role::User
                };
                a
            })
            .collect();
        state.classes = (1..=128)
            .map(|n| SchoolClass {
                id: class(n),
                school_id: school,
                name: format!("{}{:03}", "🦀".repeat(125), n),
                enabled: true,
            })
            .collect();
        state.subjects = (1..=256)
            .map(|n| SchoolSubject {
                id: subject(n),
                school_id: school,
                name: format!("{}{:03}", "🦀".repeat(61), n),
                enabled: true,
                color: Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                revision: Revision::new(1),
            })
            .collect();
        state.assignments = (1..=128)
            .flat_map(|c| {
                (1..=32).map(move |s| TeachingAssignment {
                    teacher_id: id(2),
                    class_id: class(c),
                    subject_id: subject(s),
                    enabled: true,
                })
            })
            .collect();
        state.validate().unwrap();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        validate(&editor, &state);
        for _ in 0..25 {
            act(&mut editor, NEXT, FormData::new(), &state);
        }
        assert_eq!(editor.view(&state).grid.unwrap().rows.len(), 12);
        editor.select(11, 3, &state).unwrap();
        assert_eq!(editor.selected_account, Some(id(512)));
        assert!(editor.select(12, 0, &state).is_err());
        editor.account_page = 0;
        edit(&mut editor, 1, &state);
        act(&mut editor, SCOPES, FormData::new(), &state);
        validate(&editor, &state);
        for _ in 0..204 {
            act(&mut editor, NEXT, FormData::new(), &state);
        }
        assert_eq!(editor.scope_page, 204);
        let view = editor.view(&state);
        view.validate().unwrap();
        assert_eq!(view.grid.unwrap().rows.len(), 16);
        editor.select(15, 1, &state).unwrap();
        assert_eq!(editor.selected_scope, Some(scope(128, 32)));
        assert!(editor.select(16, 0, &state).is_err());
        assert!(!editor.has_unsaved_changes());
    }
    #[test]
    fn suspension_preserves_same_owner_nonsecret_half_draft_without_changing_page() {
        let state = state();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        edit(&mut editor, 2, &state);
        editor.suspend(form(&[
            (NAME, "锁定前姓名"),
            (ROLE, "global"),
            (PASSWORD, "never retained secret"),
        ]));
        assert_eq!(editor.page, Page::Edit);
        assert_eq!(editor.owner(), id(1));
        assert!(editor.has_unsaved_changes());
        assert_eq!(
            editor
                .view(&state)
                .fields
                .iter()
                .find(|f| f.id == NAME)
                .unwrap()
                .value,
            "锁定前姓名"
        );
        let fresh = Editor::new(&state, id(1)).unwrap();
        assert!(!fresh.has_unsaved_changes());
        assert_eq!(fresh.page, Page::List);
        editor.pending_role = Role::User;
        act(&mut editor, SCOPES, FormData::new(), &state);
        editor.suspend(form(&[
            (CLASS, "2"),
            (SUBJECT, "2"),
            (PASSWORD, "never retained secret"),
        ]));
        assert_eq!(editor.page, Page::Scopes);
        assert_eq!(editor.selected_class, Some(class(2)));
        assert_eq!(editor.selected_subject, Some(subject(2)));
        validate(&editor, &state);
    }
    fn default_scope(editor: &mut Editor, state: &IdentityState, class_id: &str, subject_id: &str) {
        assert!(matches!(
            act(editor, DEFAULTS, FormData::new(), state),
            Command::Stay
        ));
        assert!(matches!(
            act(
                editor,
                DEFAULT_SELECT,
                form(&[(CLASS, class_id), (SUBJECT, subject_id)]),
                state
            ),
            Command::Stay
        ));
    }
    #[test]
    fn defaults_selection_cancel_and_explicit_confirm_keep_stable_scope_and_expected_revision() {
        let state = state();
        let before = state.clone();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        default_scope(&mut editor, &state, "1", "1");
        validate(&editor, &state);
        assert!(editor.view(&state).description.contains("未设置默认老师"));
        assert!(matches!(
            act(&mut editor, REVIEW, form(&[(DEFAULT_TEACHER, "2")]), &state),
            Command::Stay
        ));
        assert_eq!(editor.page, Page::DefaultConfirm);
        assert_eq!(state, before);
        assert!(matches!(
            act(&mut editor, RETURN, FormData::new(), &state),
            Command::Stay
        ));
        assert!(matches!(
            act(&mut editor, CANCEL, FormData::new(), &state),
            Command::Stay
        ));
        assert_eq!(editor.page, Page::Discard);
        validate(&editor, &state);
        act(&mut editor, KEEP, FormData::new(), &state);
        assert_eq!(editor.page, Page::DefaultEdit);
        act(&mut editor, REVIEW, FormData::new(), &state);
        let result = act(&mut editor, CONFIRM, FormData::new(), &state);
        assert!(
            matches!(result,Command::Update{expected,command:IdentityCommand::SetDefaultTeacher(DefaultTeacher{class_id,subject_id,teacher_id})} if expected==before.revision && class_id==class(1) && subject_id==subject(1) && teacher_id==id(2))
        );
        assert!(editor.handle(CONFIRM, FormData::new(), &state).is_err());
        assert!(
            !editor
                .view(&state)
                .actions
                .iter()
                .find(|a| a.id == CONFIRM)
                .unwrap()
                .enabled
        );
        assert_eq!(state, before);
    }
    #[test]
    fn defaults_current_choice_same_name_teachers_and_replacement_are_unambiguous() {
        let mut state = state();
        let mut second = state.accounts[1].clone();
        second.principal.id = id(4);
        second.login_name = "other-teacher".into();
        state.accounts.push(second);
        state.assignments.push(TeachingAssignment {
            teacher_id: id(4),
            class_id: class(1),
            subject_id: subject(1),
            enabled: true,
        });
        state.default_teachers.push(DefaultTeacher {
            class_id: class(1),
            subject_id: subject(1),
            teacher_id: id(2),
        });
        state.validate().unwrap();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        default_scope(&mut editor, &state, "1", "1");
        let view = editor.view(&state);
        view.validate().unwrap();
        assert!(view.description.contains("#2"));
        let field = view
            .fields
            .iter()
            .find(|f| f.id == DEFAULT_TEACHER)
            .unwrap();
        assert_eq!(field.value, "2");
        let FieldKind::Choice(choices) = &field.kind else {
            panic!("teacher choices")
        };
        assert_eq!(
            choices.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(),
            ["2", "4"]
        );
        assert_ne!(choices[0].label, choices[1].label);
        assert!(editor
            .handle(REVIEW, FormData::new(), &state)
            .unwrap_err()
            .contains("没有变化"));
        act(&mut editor, REVIEW, form(&[(DEFAULT_TEACHER, "4")]), &state);
        assert!(
            matches!(act(&mut editor,CONFIRM,FormData::new(),&state),Command::Update{command:IdentityCommand::SetDefaultTeacher(d),..} if d.teacher_id==id(4))
        );
        assert_eq!(state.default_teachers[0].teacher_id, id(2));
    }
    #[test]
    fn defaults_no_candidates_and_forged_teacher_or_scope_never_make_a_command() {
        let state = state();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        default_scope(&mut editor, &state, "2", "2");
        let view = editor.view(&state);
        view.validate().unwrap();
        assert!(view.description.contains("没有有效任课教师"));
        assert!(
            !view
                .actions
                .iter()
                .find(|a| a.id == REVIEW)
                .unwrap()
                .enabled
        );
        assert!(editor
            .handle(REVIEW, form(&[(DEFAULT_TEACHER, "2")]), &state)
            .is_err());
        act(&mut editor, CANCEL, FormData::new(), &state);
        act(&mut editor, DISCARD, FormData::new(), &state);
        default_scope(&mut editor, &state, "1", "1");
        assert!(editor
            .handle(REVIEW, form(&[(DEFAULT_TEACHER, "3")]), &state)
            .is_err());
        assert!(!editor.submitted);
        let mut invalid = Editor::new(&state, id(1)).unwrap();
        act(&mut invalid, DEFAULTS, FormData::new(), &state);
        assert!(invalid
            .handle(
                DEFAULT_SELECT,
                form(&[(CLASS, "999"), (SUBJECT, "1")]),
                &state
            )
            .is_err());
    }
    #[test]
    fn defaults_revocation_and_cas_conflict_keep_the_original_nonsecret_draft() {
        let state = state();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        default_scope(&mut editor, &state, "1", "1");
        act(&mut editor, REVIEW, form(&[(DEFAULT_TEACHER, "2")]), &state);
        let original = editor.default_draft.clone();
        let mut changed = state.clone();
        changed.revision = changed.revision.next().unwrap();
        assert!(editor
            .handle(CONFIRM, FormData::new(), &changed)
            .unwrap_err()
            .contains("目录已更新"));
        assert_eq!(editor.default_draft, original);
        assert!(!editor.submitted);
        validate(&editor, &changed);
        changed = state.clone();
        changed.accounts[0].principal.enabled = false;
        assert!(editor.handle(CONFIRM, FormData::new(), &changed).is_err());
        assert!(editor.view(&changed).fields.is_empty());
        changed = state.clone();
        changed.assignments.clear();
        assert!(editor.handle(CONFIRM, FormData::new(), &changed).is_err());
        assert!(!editor.submitted);
    }
    #[test]
    fn defaults_unknown_reload_is_explicit_and_never_returns_another_update() {
        let state = state();
        let mut editor = Editor::new(&state, id(1)).unwrap();
        default_scope(&mut editor, &state, "1", "1");
        act(&mut editor, REVIEW, form(&[(DEFAULT_TEACHER, "2")]), &state);
        act(&mut editor, CONFIRM, FormData::new(), &state);
        assert!(editor.handle(RETURN, FormData::new(), &state).is_err());
        assert!(matches!(
            act(&mut editor, RELOAD, FormData::new(), &state),
            Command::Stay
        ));
        validate(&editor, &state);
        assert!(editor.view(&state).description.contains("不证明它未保存"));
        assert!(matches!(
            act(&mut editor, DISCARD, FormData::new(), &state),
            Command::Reload
        ));
        assert!(!editor.has_unsaved_changes());
    }
}

/// Cached public history is still owner-scoped. A replacement login must query
/// again under its real exact lease rather than inherit another session's view.
pub(super) struct HistoryNotice {
    lease: fc_application::identity::SessionLease,
    owner: fc_ports::command_window::CommandOwner,
    text: String,
}
impl HistoryNotice {
    pub(super) fn new(
        lease: fc_application::identity::SessionLease,
        owner: fc_ports::command_window::CommandOwner,
        text: String,
    ) -> Self {
        Self { lease, owner, text }
    }
    pub(super) fn text_for(
        &self,
        lease: fc_application::identity::SessionLease,
        owner: fc_ports::command_window::CommandOwner,
    ) -> Option<&str> {
        (self.lease == lease && self.owner == owner).then_some(self.text.as_str())
    }
}
/// Result notices live outside a draft: refreshing the current directory must
/// neither confirm an earlier write nor discard its unresolved history notice.
pub(super) fn decorate_history(view: &mut View, note: Option<&str>, unresolved: bool, ready: bool) {
    if let Some(note) = note {
        if !view.status.is_empty() {
            view.status.push('\n');
        }
        view.status.push_str(note);
    }
    if unresolved {
        if !view.status.is_empty() {
            view.status.push('\n');
        }
        view.status.push_str("原账号操作结果仍未知。重新加载只会核对原保存结果并另列当前目录，不会重新保存。结束这次核对后，新操作仍需重新确认。");
        view.actions.push(action(
            END_RESULT_QUERY,
            "结束这次核对（原结果仍未知）",
            ready,
        ));
    }
}
/// Bounded, explicitly historical public summary. Full scopes were compared to
/// the original request by storage; rendering uses stable IDs and a visible cap.
/// Neither this string nor OriginalAccountResult contains a recoverable code.
pub(super) fn history_summary(
    message: &str,
    result: Option<&fc_bootstrap::backend::OriginalAccountResult>,
) -> String {
    let Some(result) = result else {
        return message.into();
    };
    let a = &result.account;
    let scopes = result
        .scopes
        .iter()
        .take(20)
        .map(|s| format!("班级#{}／科目#{}", s.class_id.get(), s.subject_id.get()))
        .collect::<Vec<_>>()
        .join("；");
    format!("{message}\n原保存结果：目录修订{}；账号#{}；登录名{}；姓名{}；{}；{}；任教范围{}条{}。{}\n以下账号列表来自当前目录。",
        result.revision.get(), a.principal.id.get(), a.login_name, a.display_name,
        role_name(a.principal.role), if a.principal.enabled { "启用" } else { "停用" },
        result.scopes.len(), if result.scopes.len() > 20 { "（此摘要仅列前20条）" } else { "" }, scopes)
}

#[cfg(test)]
mod account_history_notice_tests {
    use super::*;
    // Historical summaries remain bound to the current authorized reader.
    fn empty_view() -> View {
        View {
            title: String::new(),
            description: String::new(),
            status: String::new(),
            fields: Vec::new(),
            actions: Vec::new(),
            busy: false,
            grid: None,
        }
    }
    #[test]
    fn unknown_notice_and_explicit_end_are_separate_from_current_directory() {
        let mut view = empty_view();
        decorate_history(&mut view, Some("当前目录读取成功。"), true, true);
        assert!(view.status.contains("原账号操作结果仍未知"));
        assert!(view
            .actions
            .iter()
            .any(|a| a.id == END_RESULT_QUERY && a.enabled));
        let mut ended = empty_view();
        decorate_history(
            &mut ended,
            Some("原账号操作的历史结果仍未知；你已结束这次核对。"),
            false,
            true,
        );
        assert!(ended.status.contains("历史结果仍未知"));
        assert!(!ended.actions.iter().any(|a| a.id == END_RESULT_QUERY));
    }
    #[test]
    fn cached_history_requires_exact_reading_lease_and_complete_owner() {
        use fc_application::identity::IdentitySessions;
        use fc_ports::command_window::CommandOwner;
        let account = UserAccount {
            principal: Principal {
                id: PrincipalId::new(1).unwrap(),
                school_id: SchoolId::new(1).unwrap(),
                role: Role::GlobalAdmin,
                enabled: true,
                auth_epoch: AuthEpoch::new(4),
            },
            login_name: "admin".into(),
            display_name: "管理员".into(),
            must_change_password: false,
            credential_version: 1,
            recovery_version: 1,
        };
        let owner = CommandOwner::Principal {
            principal: account.principal.id,
            school: account.principal.school_id,
            epoch: account.principal.auth_epoch,
            origin: TrustedOrigin::Local,
        };
        let until = LocalDateTime::new(LocalDate::new(2026, 11, 3).unwrap(), 3600).unwrap();
        let mut sessions = IdentitySessions::default();
        let first = sessions
            .issue_verified(&account, TrustedOrigin::Local, 0, 1000, until)
            .unwrap();
        let notice = HistoryNotice::new(first, owner, "原保存姓名".into());
        assert_eq!(notice.text_for(first, owner), Some("原保存姓名"));
        let other_owner = CommandOwner::Principal {
            principal: account.principal.id,
            school: account.principal.school_id,
            epoch: AuthEpoch::new(5),
            origin: TrustedOrigin::Local,
        };
        assert_eq!(notice.text_for(first, other_owner), None);
        let next = sessions
            .issue_verified(&account, TrustedOrigin::Local, 1, 1000, until)
            .unwrap();
        assert_eq!(
            notice.text_for(next, owner),
            None,
            "new lease must authorize and query again"
        );
    }
    #[test]
    fn query_end_is_unavailable_while_original_worker_has_not_returned() {
        let mut view = empty_view();
        decorate_history(&mut view, None, true, false);
        assert!(view
            .actions
            .iter()
            .any(|a| a.id == END_RESULT_QUERY && !a.enabled));
    }
}
