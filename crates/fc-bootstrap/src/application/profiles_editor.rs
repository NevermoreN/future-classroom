//! Non-secret public/subject layout drafts. Node selections use stable IDs;
//! only the Backend's exact local lease and writer CAS authorize persistence.
use super::display_editor;
use fc_domain::{auth::Role, desktop::*, identity::IdentityState, ids::*, layout::*, profile::*};
use fc_ports::{ProfilePage, PublicLayoutState};
use fc_presentation::workbench::*;
use std::collections::{BTreeMap, HashSet};

pub(super) const CLASS: FieldId = FieldId(700);
pub(super) const SUBJECT: FieldId = FieldId(701);
pub(super) const TEACHER: FieldId = FieldId(702);
pub(super) const ADD_KIND: FieldId = FieldId(703);
pub(super) const PARENT: FieldId = FieldId(704);
pub(super) const FLOW: FieldId = FieldId(705);
pub(super) const ANCHOR: FieldId = FieldId(706);
pub(super) const WIDTH_MODE: FieldId = FieldId(707);
pub(super) const WIDTH_MIN: FieldId = FieldId(708);
pub(super) const WIDTH_PREF: FieldId = FieldId(709);
pub(super) const WIDTH_MAX: FieldId = FieldId(710);
pub(super) const WIDTH_PERCENT: FieldId = FieldId(711);
pub(super) const HEIGHT_MODE: FieldId = FieldId(712);
pub(super) const HEIGHT_MIN: FieldId = FieldId(713);
pub(super) const HEIGHT_PREF: FieldId = FieldId(714);
pub(super) const HEIGHT_MAX: FieldId = FieldId(715);
pub(super) const HEIGHT_PERCENT: FieldId = FieldId(716);
pub(super) const PADDING: FieldId = FieldId(717);
pub(super) const GAP: FieldId = FieldId(718);
pub(super) const GROUP_ALPHA: FieldId = FieldId(719);
pub(super) const GLASS: FieldId = FieldId(720);
pub(super) const SURFACE: FieldId = FieldId(721);
pub(super) const EDGE: FieldId = FieldId(722);
pub(super) const ENABLED: FieldId = FieldId(723);
pub(super) const VISIBLE: FieldId = FieldId(724);
pub(super) const ALPHA: FieldId = FieldId(725);
pub(super) const FONT: FieldId = FieldId(726);
pub(super) const MIN_WIDTH: FieldId = FieldId(727);
pub(super) const MAX_WIDTH: FieldId = FieldId(728);
pub(super) const MIN_HEIGHT: FieldId = FieldId(729);
pub(super) const MAX_HEIGHT: FieldId = FieldId(730);
pub(super) const ROWS: FieldId = FieldId(731);
pub(super) const ROW_GAP: FieldId = FieldId(732);
pub(super) const PUBLIC: ActionId = ActionId(700);
pub(super) const NEW: ActionId = ActionId(701);
pub(super) const OPEN: ActionId = ActionId(702);
pub(super) const NEXT: ActionId = ActionId(703);
pub(super) const FIRST: ActionId = ActionId(704);
pub(super) const PREVIOUS: ActionId = ActionId(705);
pub(super) const EDIT_NODE: ActionId = ActionId(706);
pub(super) const ADD_CONTAINER: ActionId = ActionId(707);
pub(super) const ADD_WIDGET: ActionId = ActionId(708);
pub(super) const UP: ActionId = ActionId(709);
pub(super) const DOWN: ActionId = ActionId(710);
pub(super) const REPARENT: ActionId = ActionId(711);
pub(super) const NODE_APPEARANCE: ActionId = ActionId(712);
pub(super) const APPLY: ActionId = ActionId(713);
pub(super) const BACK: ActionId = ActionId(714);
pub(super) const REVIEW: ActionId = ActionId(715);
pub(super) const CONFIRM: ActionId = ActionId(716);
pub(super) const CANCEL: ActionId = ActionId(717);
pub(super) const CLOSE: ActionId = ActionId(718);
pub(super) const DISCARD: ActionId = ActionId(719);
pub(super) const KEEP: ActionId = ActionId(720);
pub(super) const DELETE: ActionId = ActionId(721);
pub(super) const REMOVE_SUBTREE: ActionId = ActionId(722);
pub(super) const PROMOTE_CHILDREN: ActionId = ActionId(723);
pub(super) const RESET: ActionId = ActionId(724);
pub(super) const CONFIRM_RESET: ActionId = ActionId(725);
pub(super) const MATERIALS: ActionId = ActionId(726);
pub(super) const PUBLIC_STYLE: ActionId = ActionId(727);
pub(super) const PREVIEW: ActionId = ActionId(728);
pub(super) const CLEAR_FOLDER: ActionId = ActionId(729);
pub(super) const NODE_OPERATIONS: ActionId = ActionId(730);

#[derive(Debug)]
pub(super) enum Command {
    Stay,
    Close,
    ReloadList {
        after: Option<ProfileId>,
    },
    LoadPublic,
    LoadSubject {
        key: SubjectProfileKey,
    },
    SavePublic {
        expected_layout_revision: Revision,
        expected_display_revision: Revision,
        layout: LayoutDocument,
        display: DisplaySettings,
    },
    SaveSubject {
        key: SubjectProfileKey,
        expected_revision: Revision,
        candidate: SubjectProfileDocument,
    },
    Preview {
        public: Option<PublicLayoutState>,
        subject: Option<SubjectProfileDocument>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    List,
    Tree,
    Operations,
    Geometry,
    Appearance,
    Materials,
    PublicStyle,
    Review,
    Discard,
    Delete,
    Reset,
}
#[derive(Clone, Debug)]
enum Draft {
    Public(PublicLayoutState),
    Subject(SubjectProfileDocument),
}
#[derive(Clone, Copy)]
enum Destination {
    Close,
    List,
    Public,
    Subject(SubjectProfileKey),
    ListAfter(Option<ProfileId>),
}
pub(super) struct Editor {
    owner: PrincipalId,
    identity: IdentityState,
    list: ProfilePage,
    page: Page,
    list_after: Option<ProfileId>,
    history: Vec<Option<ProfileId>>,
    selected_profile: Option<ProfileId>,
    draft: Option<Draft>,
    original: Option<Draft>,
    creating: bool,
    selected: Option<LayoutNodeId>,
    raw: BTreeMap<FieldId, String>,
    raw_original: BTreeMap<FieldId, String>,
    style: Option<display_editor::Editor>,
    style_original: Option<DisplaySettings>,
    discard_return: Page,
    destination: Destination,
    submitted: bool,
}
fn action(id: ActionId, label: &str, enabled: bool) -> Action {
    Action {
        id,
        label: label.into(),
        enabled,
    }
}
fn field(id: FieldId, label: &str, kind: FieldKind, value: String, max: usize) -> Field {
    Field {
        id,
        label: label.into(),
        kind,
        value,
        max_utf16: max,
    }
}
fn text(id: FieldId, label: &str, value: impl ToString) -> Field {
    field(id, label, FieldKind::Text, value.to_string(), 16)
}
fn choices(id: FieldId, label: &str, value: String, items: Vec<Choice>) -> Field {
    field(id, label, FieldKind::Choice(items), value, 64)
}
fn choose(id: FieldId, label: &str, value: &str, items: &[(&str, &str)]) -> Field {
    choices(
        id,
        label,
        value.into(),
        items
            .iter()
            .map(|(v, l)| Choice {
                value: (*v).into(),
                label: (*l).into(),
            })
            .collect(),
    )
}
fn toggle(id: FieldId, label: &str, value: bool) -> Field {
    choose(
        id,
        label,
        if value { "1" } else { "0" },
        &[("1", "启用"), ("0", "关闭")],
    )
}
fn short(value: &str) -> String {
    if value.len() <= 430 {
        return value.into();
    }
    let mut n = 427;
    while !value.is_char_boundary(n) {
        n -= 1;
    }
    format!("{}…", &value[..n])
}
fn caption(name: &str, id: u64, enabled: bool) -> String {
    format!(
        "#{id} {}{}",
        short(name),
        if enabled { "" } else { "（停用）" }
    )
}
fn authorized(identity: &IdentityState, owner: PrincipalId) -> Result<(), String> {
    let a = identity
        .account(owner)
        .map_err(|_| "当前主管理员不存在。")?;
    if a.principal.role != Role::GlobalAdmin || !a.principal.enabled || a.must_change_password {
        return Err("仅本地主管理员可以编辑公共与科目布局，请重新登录。".into());
    }
    Ok(())
}
fn issue(error: impl std::fmt::Debug) -> String {
    format!("布局未通过校验：{error:?}。草稿保留，请核对层级、范围和尺寸。")
}
fn widget_value(kind: WidgetKind) -> &'static str {
    match kind {
        WidgetKind::Clock => "clock",
        WidgetKind::Date => "date",
        WidgetKind::Schedule => "schedule",
        WidgetKind::Homework => "homework",
        WidgetKind::Reminder => "reminder",
        WidgetKind::DateCountdown => "countdown",
        WidgetKind::Recess => "recess",
        WidgetKind::Materials => "materials",
        WidgetKind::SubjectText => "text",
        WidgetKind::SubjectImage => "image",
    }
}
fn widget_label(kind: WidgetKind) -> &'static str {
    match kind {
        WidgetKind::Clock => "时钟",
        WidgetKind::Date => "日期",
        WidgetKind::Schedule => "今日课表",
        WidgetKind::Homework => "全科作业",
        WidgetKind::Reminder => "共同提醒",
        WidgetKind::DateCountdown => "日期倒计时",
        WidgetKind::Recess => "课间剩余区",
        WidgetKind::Materials => "课堂资料",
        WidgetKind::SubjectText => "独立文字（尚未开放）",
        WidgetKind::SubjectImage => "独立图片（尚未开放）",
    }
}
fn widget_parse(value: &str) -> Result<WidgetKind, String> {
    [
        WidgetKind::Clock,
        WidgetKind::Date,
        WidgetKind::Schedule,
        WidgetKind::Homework,
        WidgetKind::Reminder,
        WidgetKind::DateCountdown,
        WidgetKind::Recess,
        WidgetKind::Materials,
    ]
    .into_iter()
    .find(|k| widget_value(*k) == value)
    .ok_or("请选择已支持的组件类型。".into())
}
fn kind_label(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Root { .. } => "透明逻辑根".into(),
        NodeKind::Container(c) => if c.material.is_some() {
            "玻璃容器"
        } else {
            "透明结构容器"
        }
        .into(),
        NodeKind::Outlet { .. } => "科目内容出口（公共默认）".into(),
        NodeKind::Widget(k) => widget_label(*k).into(),
    }
}
fn flow_value(flow: Flow) -> &'static str {
    match flow {
        Flow::Row => "row",
        Flow::Column => "column",
        Flow::Wrap => "wrap",
    }
}
fn flow_parse(value: &str) -> Result<Flow, String> {
    match value {
        "row" => Ok(Flow::Row),
        "column" => Ok(Flow::Column),
        "wrap" => Ok(Flow::Wrap),
        _ => Err("请选择排列方式。".into()),
    }
}
fn anchor_value(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::TopLeft => "tl",
        Anchor::TopRight => "tr",
        Anchor::BottomLeft => "bl",
        Anchor::BottomRight => "br",
    }
}
fn axis_fields(width: bool, size: AxisSize) -> Vec<Field> {
    let ids = if width {
        [WIDTH_MODE, WIDTH_MIN, WIDTH_PREF, WIDTH_MAX, WIDTH_PERCENT]
    } else {
        [
            HEIGHT_MODE,
            HEIGHT_MIN,
            HEIGHT_PREF,
            HEIGHT_MAX,
            HEIGHT_PERCENT,
        ]
    };
    let label = if width { "宽" } else { "高" };
    let (mode, min, pref, max, percent) = match size {
        AxisSize::Fixed(n) => ("fixed", n, n, n, 10000),
        AxisSize::Content {
            min,
            preferred,
            max,
        } => ("content", min, preferred, max, 10000),
        AxisSize::Percent {
            basis_points,
            min,
            max,
        } => ("percent", min, min, max, basis_points),
    };
    vec![
        choose(
            ids[0],
            &format!("{label}尺寸模式"),
            mode,
            &[
                ("fixed", "固定DIP（使用首选值）"),
                ("content", "随内容"),
                ("percent", "父尺寸比例"),
            ],
        ),
        text(ids[1], &format!("{label}最小DIP"), min),
        text(ids[2], &format!("{label}首选 / 固定DIP"), pref),
        text(ids[3], &format!("{label}最大DIP"), max),
        text(ids[4], &format!("{label}比例（1..10000=100%）"), percent),
    ]
}
impl Editor {
    pub(super) fn new(
        identity: &IdentityState,
        owner: PrincipalId,
        page: ProfilePage,
    ) -> Result<Self, String> {
        identity.validate().map_err(issue)?;
        authorized(identity, owner)?;
        Self::validate_list(&page)?;
        Ok(Self {
            owner,
            identity: identity.clone(),
            list: page,
            page: Page::List,
            list_after: None,
            history: vec![],
            selected_profile: None,
            draft: None,
            original: None,
            creating: false,
            selected: None,
            raw: BTreeMap::new(),
            raw_original: BTreeMap::new(),
            style: None,
            style_original: None,
            discard_return: Page::Tree,
            destination: Destination::List,
            submitted: false,
        })
    }
    pub(super) fn owner(&self) -> PrincipalId {
        self.owner
    }
    fn validate_list(page: &ProfilePage) -> Result<(), String> {
        if page.items.len() > 20
            || page.total > MAX_SUBJECT_PROFILES as u64
            || page.total < page.items.len() as u64
            || page.items.windows(2).any(|w| w[0].id >= w[1].id)
            || page
                .next_after
                .is_some_and(|id| page.items.last().is_none_or(|last| last.id != id))
        {
            return Err("科目配置列表格式无效，请重新加载。".into());
        }
        Ok(())
    }
    pub(super) fn loaded_list(&mut self, page: ProfilePage) -> Result<(), String> {
        Self::validate_list(&page)?;
        if self.has_unsaved_changes() {
            return Err("先保存或明确丢弃当前草稿。".into());
        }
        self.list = page;
        self.page = Page::List;
        self.selected_profile = None;
        Ok(())
    }
    pub(super) fn loaded_public(&mut self, state: PublicLayoutState) -> Result<(), String> {
        state.layout.validate_structure().map_err(issue)?;
        state.display.validate().map_err(issue)?;
        if self.has_unsaved_changes() && !self.submitted {
            return Err("已有未保存草稿，不能用后台读取覆盖。".into());
        }
        self.draft = Some(Draft::Public(state.clone()));
        self.original = Some(Draft::Public(state));
        self.creating = false;
        self.finish_loaded();
        Ok(())
    }
    pub(super) fn loaded_subject(
        &mut self,
        key: SubjectProfileKey,
        document: Option<SubjectProfileDocument>,
    ) -> Result<(), String> {
        if self.has_unsaved_changes() && !self.submitted {
            return Err("已有未保存草稿，不能用后台读取覆盖。".into());
        }
        if document.as_ref().is_some_and(|d| d.key != key) {
            return Err("后台配置目标已改变。".into());
        }
        let creating = document.is_none();
        let document = document.unwrap_or_else(|| {
            SubjectProfileDocument::new(
                ProfileId::new(1).expect("constant nonzero provisional profile ID"),
                key,
            )
        });
        document.validate_structure().map_err(issue)?;
        self.original = Some(Draft::Subject(document.clone()));
        self.draft = Some(Draft::Subject(document));
        self.creating = creating;
        self.finish_loaded();
        Ok(())
    }
    fn finish_loaded(&mut self) {
        self.page = Page::Tree;
        self.selected = self.tree().map(|t| t.root);
        self.raw.clear();
        self.raw_original.clear();
        self.style = None;
        self.style_original = None;
        self.submitted = false;
    }
    fn tree(&self) -> Option<&LayoutTree> {
        match self.draft.as_ref()? {
            Draft::Public(s) => Some(&s.layout.tree),
            Draft::Subject(s) => Some(&s.tree),
        }
    }
    fn tree_mut(&mut self) -> Option<&mut LayoutTree> {
        match self.draft.as_mut()? {
            Draft::Public(s) => Some(&mut s.layout.tree),
            Draft::Subject(s) => Some(&mut s.tree),
        }
    }
    fn allocate(&mut self) -> Result<LayoutNodeId, String> {
        match self.draft.as_mut().ok_or("没有布局草稿。")? {
            Draft::Public(s) => s.layout.allocate_id().map_err(issue),
            Draft::Subject(s) => s.allocate_id().map_err(issue),
        }
    }
    fn selected_node(&self) -> Option<&LayoutNode> {
        self.tree()?.node(self.selected?)
    }
    fn parent(&self, id: LayoutNodeId) -> Option<LayoutNodeId> {
        self.tree()?
            .nodes
            .iter()
            .find(|n| n.children.contains(&id))
            .map(|n| n.id)
    }
    fn descendants(&self, id: LayoutNodeId) -> HashSet<LayoutNodeId> {
        let mut result = HashSet::new();
        let mut queue = vec![id];
        while let Some(id) = queue.pop() {
            if !result.insert(id) {
                continue;
            }
            if let Some(node) = self.tree().and_then(|t| t.node(id)) {
                queue.extend(node.children.iter().copied());
            }
        }
        result
    }
    fn order(&self) -> Vec<(LayoutNodeId, usize)> {
        let Some(tree) = self.tree() else {
            return vec![];
        };
        let mut out = Vec::new();
        let mut queue = vec![(tree.root, 0)];
        let mut seen = HashSet::new();
        while let Some((id, depth)) = queue.pop() {
            if !seen.insert(id) {
                continue;
            }
            out.push((id, depth));
            if let Some(node) = tree.node(id) {
                queue.extend(node.children.iter().rev().map(|n| (*n, depth + 1)));
            }
        }
        out
    }
    fn semantic_dirty(&self) -> bool {
        match (&self.draft, &self.original) {
            (Some(Draft::Public(a)), Some(Draft::Public(b))) => {
                a.layout.tree != b.layout.tree || a.display != b.display
            }
            (Some(Draft::Subject(a)), Some(Draft::Subject(b))) => {
                self.creating
                    || a.enabled != b.enabled
                    || a.tree != b.tree
                    || a.folder_id != b.folder_id
                    || a.materials != b.materials
            }
            (None, None) => false,
            _ => true,
        }
    }
    pub(super) fn has_unsaved_changes(&self) -> bool {
        if self.draft.is_none() {
            return false;
        }
        self.semantic_dirty()
            || self.style.as_ref().is_some_and(|s| s.has_unsaved_changes())
            || self.raw.iter().any(|(id, value)| {
                self.raw_original
                    .get(id)
                    .is_some_and(|original| value != original)
            })
    }
    pub(super) fn suspend(&mut self, data: FormData) {
        if self.page == Page::PublicStyle {
            if let Some(style) = &mut self.style {
                style.capture(data);
            }
            return;
        }
        for field in self
            .fields()
            .iter()
            .filter(|f| f.kind != FieldKind::Password && f.kind != FieldKind::ReadOnly)
        {
            if let Some(value) = data.get(field.id) {
                if !value.contains('\0')
                    && value.encode_utf16().count() <= field.max_utf16
                    && !matches!(&field.kind,FieldKind::Choice(items) if !value.is_empty()&&!items.iter().any(|c|c.value==value))
                {
                    if matches!(
                        self.page,
                        Page::Geometry | Page::Appearance | Page::Materials
                    ) {
                        self.raw_original
                            .entry(field.id)
                            .or_insert_with(|| field.value.clone());
                    }
                    self.raw.insert(field.id, value.into());
                }
            }
        }
    }
    fn value(&self, id: FieldId) -> String {
        let field = self.fields().into_iter().find(|f| f.id == id);
        if let Some(raw) = self.raw.get(&id) {
            if !matches!(field.as_ref().map(|f|&f.kind),Some(FieldKind::Choice(items)) if !raw.is_empty()&&!items.iter().any(|c|c.value==*raw))
            {
                return raw.clone();
            }
        }
        field.map(|f| f.value).unwrap_or_default()
    }
    fn integer(&self, id: FieldId) -> Result<u16, String> {
        let v = self.value(id);
        if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err("请输入完整非负整数，草稿会保留。".into());
        }
        v.parse().map_err(|_| "数字超出范围。".into())
    }
    fn percent(&self, id: FieldId) -> Result<u8, String> {
        let n = self.integer(id)?;
        if n > 100 {
            return Err("透明度范围为0至100。".into());
        }
        Ok(n as u8)
    }
    fn boolean(&self, id: FieldId) -> Result<bool, String> {
        match self.value(id).as_str() {
            "1" => Ok(true),
            "0" => Ok(false),
            _ => Err("请选择启用或关闭。".into()),
        }
    }
    fn axis(&self, width: bool) -> Result<AxisSize, String> {
        let ids = if width {
            [WIDTH_MODE, WIDTH_MIN, WIDTH_PREF, WIDTH_MAX, WIDTH_PERCENT]
        } else {
            [
                HEIGHT_MODE,
                HEIGHT_MIN,
                HEIGHT_PREF,
                HEIGHT_MAX,
                HEIGHT_PERCENT,
            ]
        };
        let size = match self.value(ids[0]).as_str() {
            "fixed" => AxisSize::Fixed(self.integer(ids[2])?),
            "content" => AxisSize::Content {
                min: self.integer(ids[1])?,
                preferred: self.integer(ids[2])?,
                max: self.integer(ids[3])?,
            },
            "percent" => AxisSize::Percent {
                basis_points: self.integer(ids[4])?,
                min: self.integer(ids[1])?,
                max: self.integer(ids[3])?,
            },
            _ => return Err("请选择尺寸模式。".into()),
        };
        size.validate().map_err(issue)?;
        Ok(size)
    }
    fn target(&self) -> Result<SubjectProfileKey, String> {
        let parse = |id| {
            self.value(id)
                .parse::<u64>()
                .map_err(|_| "请选择班级、科目和实际老师。".to_string())
        };
        let key = SubjectProfileKey {
            class_id: ClassId::new(parse(CLASS)?).map_err(|_| "班级无效。")?,
            subject_id: SubjectId::new(parse(SUBJECT)?).map_err(|_| "科目无效。")?,
            teacher_id: PrincipalId::new(parse(TEACHER)?).map_err(|_| "老师无效。")?,
        };
        if !self
            .identity
            .is_teacher(key.teacher_id, key.class_id, key.subject_id)
        {
            return Err("该老师没有所选班级与科目的有效任教关系。".into());
        }
        Ok(key)
    }
    fn current_class(&self) -> Option<ClassId> {
        self.raw
            .get(&CLASS)
            .and_then(|s| s.parse::<u64>().ok())
            .and_then(|n| ClassId::new(n).ok())
            .filter(|id| {
                self.identity
                    .classes
                    .iter()
                    .any(|c| c.id == *id && c.enabled)
            })
            .or_else(|| {
                self.identity
                    .classes
                    .iter()
                    .find(|c| c.enabled)
                    .map(|c| c.id)
            })
    }
    fn current_subject(&self) -> Option<SubjectId> {
        self.raw
            .get(&SUBJECT)
            .and_then(|s| s.parse::<u64>().ok())
            .and_then(|n| SubjectId::new(n).ok())
            .filter(|id| {
                self.identity
                    .subjects
                    .iter()
                    .any(|s| s.id == *id && s.enabled)
            })
            .or_else(|| {
                self.identity
                    .subjects
                    .iter()
                    .find(|s| s.enabled)
                    .map(|s| s.id)
            })
    }
    fn fields(&self) -> Vec<Field> {
        match self.page {
            Page::List => {
                let class = self.current_class();
                let subject = self.current_subject();
                let classes = self
                    .identity
                    .classes
                    .iter()
                    .filter(|c| c.enabled)
                    .map(|c| Choice {
                        value: c.id.get().to_string(),
                        label: caption(&c.name, c.id.get(), true),
                    })
                    .collect();
                let subjects = self
                    .identity
                    .subjects
                    .iter()
                    .filter(|s| s.enabled)
                    .map(|s| Choice {
                        value: s.id.get().to_string(),
                        label: caption(&s.name, s.id.get(), true),
                    })
                    .collect();
                let teachers: Vec<_> = self
                    .identity
                    .accounts
                    .iter()
                    .filter(|a| {
                        class
                            .zip(subject)
                            .is_some_and(|(c, s)| self.identity.is_teacher(a.principal.id, c, s))
                    })
                    .map(|a| Choice {
                        value: a.principal.id.get().to_string(),
                        label: caption(&a.display_name, a.principal.id.get(), true),
                    })
                    .collect();
                let teacher = self
                    .raw
                    .get(&TEACHER)
                    .cloned()
                    .filter(|v| teachers.iter().any(|c| c.value == *v))
                    .or_else(|| teachers.first().map(|c| c.value.clone()))
                    .unwrap_or_default();
                vec![
                    choices(
                        CLASS,
                        "新配置班级",
                        class.map(|id| id.get().to_string()).unwrap_or_default(),
                        classes,
                    ),
                    choices(
                        SUBJECT,
                        "新配置科目",
                        subject.map(|id| id.get().to_string()).unwrap_or_default(),
                        subjects,
                    ),
                    choices(TEACHER, "新配置实际老师", teacher, teachers),
                ]
            }
            Page::Tree | Page::Operations => {
                let mut out = vec![];
                let subject = matches!(self.draft, Some(Draft::Subject(_)));
                let kinds = if subject {
                    vec![WidgetKind::Materials]
                } else {
                    vec![
                        WidgetKind::Clock,
                        WidgetKind::Date,
                        WidgetKind::Schedule,
                        WidgetKind::Homework,
                        WidgetKind::Reminder,
                        WidgetKind::DateCountdown,
                        WidgetKind::Recess,
                        WidgetKind::Materials,
                    ]
                };
                out.push(choices(
                    ADD_KIND,
                    "新增组件类型",
                    self.raw
                        .get(&ADD_KIND)
                        .cloned()
                        .unwrap_or("materials".into()),
                    kinds
                        .iter()
                        .map(|k| Choice {
                            value: widget_value(*k).into(),
                            label: widget_label(*k).into(),
                        })
                        .collect(),
                ));
                let excluded = self
                    .selected
                    .map(|id| self.descendants(id))
                    .unwrap_or_default();
                let parents: Vec<_> = self
                    .tree()
                    .into_iter()
                    .flat_map(|t| t.nodes.iter())
                    .filter(|n| !excluded.contains(&n.id) && !matches!(n.kind, NodeKind::Widget(_)))
                    .map(|n| Choice {
                        value: n.id.get().to_string(),
                        label: format!("#{} {}", n.id.get(), kind_label(&n.kind)),
                    })
                    .collect();
                let value = self
                    .raw
                    .get(&PARENT)
                    .cloned()
                    .filter(|v| parents.iter().any(|c| c.value == *v))
                    .or_else(|| {
                        self.selected
                            .and_then(|id| self.parent(id))
                            .map(|id| id.get().to_string())
                    })
                    .unwrap_or_default();
                out.push(choices(PARENT, "重挂到父节点", value, parents));
                if self.page == Page::Operations {
                    out.remove(0);
                }
                out
            }
            Page::Geometry => {
                let Some(node) = self.selected_node() else {
                    return vec![];
                };
                let (flow, padding, gap, width, height, anchor) = match &node.kind {
                    NodeKind::Root {
                        flow,
                        padding,
                        gap,
                        width,
                        height,
                        anchor,
                    } => (*flow, *padding, *gap, *width, *height, Some(*anchor)),
                    NodeKind::Container(c) => (c.flow, c.padding, c.gap, c.width, c.height, None),
                    NodeKind::Outlet { width, height } => {
                        (Flow::Column, 0, 0, *width, *height, None)
                    }
                    NodeKind::Widget(_) => return vec![],
                };
                let mut out = vec![
                    choose(
                        FLOW,
                        "子节点排列",
                        flow_value(flow),
                        &[("row", "水平"), ("column", "垂直"), ("wrap", "自动换行")],
                    ),
                    text(PADDING, "内边距DIP（0..256）", padding),
                    text(GAP, "节点间距DIP（0..256）", gap),
                ];
                if let Some(anchor) = anchor {
                    out.push(choose(
                        ANCHOR,
                        "根锚点",
                        anchor_value(anchor),
                        &[
                            ("tl", "左上"),
                            ("tr", "右上"),
                            ("bl", "左下"),
                            ("br", "右下"),
                        ],
                    ));
                }
                out.extend(axis_fields(true, width));
                out.extend(axis_fields(false, height));
                out
            }
            Page::Appearance => {
                let Some(LayoutNode {
                    kind: NodeKind::Container(c),
                    ..
                }) = self.selected_node()
                else {
                    return vec![];
                };
                let material = c.material.unwrap_or_default();
                vec![
                    text(GROUP_ALPHA, "父组整体Alpha 0..100", c.group_opacity_percent),
                    toggle(GLASS, "绘制容器玻璃", c.material.is_some()),
                    text(SURFACE, "容器玻璃填充 0..100", material.surface_percent),
                    text(EDGE, "容器边缘反射 0..100", material.edge_percent),
                ]
            }
            Page::Materials => {
                let Some(Draft::Subject(s)) = &self.draft else {
                    return vec![];
                };
                let c = &s.materials.component;
                vec![
                    toggle(ENABLED, "启用本科目配置", s.enabled),
                    toggle(VISIBLE, "显示资料磁贴", c.visible),
                    text(ALPHA, "资料整体Alpha 0..100", c.opacity_percent),
                    text(FONT, "资料字号DIP（10..72）", c.font_dip),
                    text(MIN_WIDTH, "资料最小宽DIP", c.min_width),
                    text(MAX_WIDTH, "资料最大宽DIP", c.max_width),
                    text(MIN_HEIGHT, "资料最小高DIP", c.min_height),
                    text(MAX_HEIGHT, "资料最大高DIP", c.max_height),
                    text(ROWS, "每页可见文件条数（1..5）", s.materials.visible_rows),
                    text(ROW_GAP, "文件行距DIP（0..64）", s.materials.row_gap_dip),
                    field(
                        FieldId(733),
                        "资料目录引用",
                        FieldKind::ReadOnly,
                        s.folder_id
                            .map_or("使用此精确老师/科目的当前目录".into(), |id| {
                                format!("固定目录 #{}", id.get())
                            }),
                        128,
                    ),
                ]
            }
            _ => vec![],
        }
    }
    fn apply_fields(&mut self) -> Result<(), String> {
        match self.page {
            Page::Geometry => {
                let id = self.selected.ok_or("请选择节点。")?;
                let width = self.axis(true)?;
                let height = self.axis(false)?;
                let flow = flow_parse(&self.value(FLOW))?;
                let padding = self.integer(PADDING)?;
                let gap = self.integer(GAP)?;
                if padding > 256 || gap > 256 {
                    return Err("内边距与间距范围为0..256DIP。".into());
                }
                let old = self.selected_node().ok_or("节点不存在。")?.kind.clone();
                let kind = match old {
                    NodeKind::Root { .. } => NodeKind::Root {
                        flow,
                        padding,
                        gap,
                        width,
                        height,
                        anchor: match self.value(ANCHOR).as_str() {
                            "tl" => Anchor::TopLeft,
                            "tr" => Anchor::TopRight,
                            "bl" => Anchor::BottomLeft,
                            "br" => Anchor::BottomRight,
                            _ => return Err("请选择根锚点。".into()),
                        },
                    },
                    NodeKind::Container(mut c) => {
                        c.flow = flow;
                        c.padding = padding;
                        c.gap = gap;
                        c.width = width;
                        c.height = height;
                        NodeKind::Container(c)
                    }
                    NodeKind::Outlet { .. } => NodeKind::Outlet { width, height },
                    NodeKind::Widget(_) => return Err("组件尺寸来自其显示设置。".into()),
                };
                self.tree_mut()
                    .and_then(|t| t.nodes.iter_mut().find(|n| n.id == id))
                    .ok_or("节点不存在。")?
                    .kind = kind;
            }
            Page::Appearance => {
                let opacity = self.percent(GROUP_ALPHA)?;
                let glass = self.boolean(GLASS)?;
                let material = if glass {
                    Some(GlassMaterial {
                        surface_percent: self.percent(SURFACE)?,
                        edge_percent: self.percent(EDGE)?,
                    })
                } else {
                    None
                };
                let id = self.selected.ok_or("请选择容器。")?;
                let node = self
                    .tree_mut()
                    .and_then(|t| t.nodes.iter_mut().find(|n| n.id == id))
                    .ok_or("节点不存在。")?;
                let NodeKind::Container(c) = &mut node.kind else {
                    return Err("此节点不是容器。".into());
                };
                c.group_opacity_percent = opacity;
                c.material = material;
            }
            Page::Materials => {
                let Some(Draft::Subject(old)) = &self.draft else {
                    return Err("公共样式请进入公共显示设置。".into());
                };
                let mut next = old.clone();
                next.enabled = self.boolean(ENABLED)?;
                let c = &mut next.materials.component;
                c.visible = self.boolean(VISIBLE)?;
                c.opacity_percent = self.percent(ALPHA)?;
                c.font_dip = self.integer(FONT)?;
                c.min_width = self.integer(MIN_WIDTH)?;
                c.max_width = self.integer(MAX_WIDTH)?;
                c.min_height = self.integer(MIN_HEIGHT)?;
                c.max_height = self.integer(MAX_HEIGHT)?;
                next.materials.visible_rows =
                    u8::try_from(self.integer(ROWS)?).map_err(|_| "可见条数超界。")?;
                next.materials.row_gap_dip = self.integer(ROW_GAP)?;
                next.materials.validate().map_err(issue)?;
                self.draft = Some(Draft::Subject(next));
            }
            _ => {}
        }
        self.raw.clear();
        self.raw_original.clear();
        Ok(())
    }
    fn preflight(&self) -> Result<(), String> {
        match self.draft.as_ref().ok_or("请先读取公共布局或科目配置。")? {
            Draft::Public(s) => {
                s.layout.validate_structure().map_err(issue)?;
                s.display.validate().map_err(issue)?;
            }
            Draft::Subject(s) => {
                s.validate_structure().map_err(issue)?;
                if s.enabled
                    && !self
                        .identity
                        .is_teacher(s.key.teacher_id, s.key.class_id, s.key.subject_id)
                {
                    return Err(
                        "此配置的任教关系已失效。可停用保留，重新启用前请恢复任教关系。".into(),
                    );
                }
            }
        }
        Ok(())
    }
    fn clear(&mut self) {
        self.draft = None;
        self.original = None;
        self.style = None;
        self.style_original = None;
        self.raw.clear();
        self.raw_original.clear();
        self.selected = None;
        self.creating = false;
        self.submitted = false;
        self.page = Page::List;
    }
    fn depart(&mut self, destination: Destination) -> Result<Command, String> {
        if self.has_unsaved_changes() {
            self.destination = destination;
            self.discard_return = self.page;
            self.page = Page::Discard;
            return Ok(Command::Stay);
        }
        self.finish_departure(destination)
    }
    fn finish_departure(&mut self, destination: Destination) -> Result<Command, String> {
        self.clear();
        Ok(match destination {
            Destination::Close => Command::Close,
            Destination::List => Command::Stay,
            Destination::Public => Command::LoadPublic,
            Destination::Subject(key) => Command::LoadSubject { key },
            Destination::ListAfter(after) => {
                self.list_after = after;
                Command::ReloadList { after }
            }
        })
    }
    pub(super) fn select(&mut self, row: usize, column: usize) -> Result<(), String> {
        match self.page {
            Page::List if column < 4 => {
                self.selected_profile =
                    Some(self.list.items.get(row).ok_or("配置选择超出本页。")?.id);
            }
            Page::Tree if column < 3 => {
                self.selected = Some(self.order().get(row).ok_or("节点选择超出范围。")?.0);
                self.raw.remove(&PARENT);
            }
            _ => return Err("此页不能选择列表行。".into()),
        }
        Ok(())
    }
    fn parent_for_add(&self) -> Result<LayoutNodeId, String> {
        let node = self
            .selected_node()
            .ok_or("请先选择父节点或其中一个组件。")?;
        if matches!(node.kind, NodeKind::Widget(_)) {
            self.parent(node.id).ok_or("没有可用父节点。".into())
        } else {
            Ok(node.id)
        }
    }
    fn add_node(&mut self, kind: NodeKind) -> Result<(), String> {
        if self
            .tree()
            .is_none_or(|t| t.nodes.len() >= MAX_LAYOUT_NODES)
        {
            return Err("布局最多128个配置节点。".into());
        }
        let parent = self.parent_for_add()?;
        let id = self.allocate()?;
        let tree = self.tree_mut().ok_or("没有草稿。")?;
        tree.nodes
            .iter_mut()
            .find(|n| n.id == parent)
            .ok_or("父节点不存在。")?
            .children
            .push(id);
        tree.nodes.push(LayoutNode {
            id,
            kind,
            children: vec![],
        });
        self.selected = Some(id);
        self.raw.clear();
        self.raw_original.clear();
        Ok(())
    }
    fn move_node(&mut self, up: bool) -> Result<(), String> {
        let id = self.selected.ok_or("请选择节点。")?;
        let parent = self.parent(id).ok_or("透明根没有同级顺序。")?;
        let children = &mut self
            .tree_mut()
            .and_then(|t| t.nodes.iter_mut().find(|n| n.id == parent))
            .ok_or("父节点不存在。")?
            .children;
        let index = children
            .iter()
            .position(|n| *n == id)
            .ok_or("节点已改变。")?;
        if up && index > 0 {
            children.swap(index, index - 1);
        } else if !up && index + 1 < children.len() {
            children.swap(index, index + 1);
        }
        self.raw.clear();
        self.raw_original.clear();
        Ok(())
    }
    fn reparent(&mut self) -> Result<(), String> {
        let source = self.selected.ok_or("请选择节点。")?;
        let target = self
            .value(PARENT)
            .parse::<u64>()
            .ok()
            .and_then(|n| LayoutNodeId::new(n).ok())
            .ok_or("请选择新父节点。")?;
        if self.parent(source) == Some(target) {
            self.raw.clear();
            self.raw_original.clear();
            return Ok(());
        }
        let scope = self.tree().ok_or("没有草稿。")?.scope;
        let page = self.page;
        self.page = Page::Tree;
        let result = self.drop_node(scope, source, target, None);
        self.page = page;
        result
    }
    fn remove(&mut self, subtree: bool) -> Result<(), String> {
        let id = self.selected.ok_or("请选择节点。")?;
        let node = self.selected_node().ok_or("节点不存在。")?.clone();
        if matches!(node.kind, NodeKind::Root { .. } | NodeKind::Outlet { .. }) {
            return Err("透明根与唯一内容出口不能删除；可编辑它们的约束。".into());
        }
        let parent = self.parent(id).ok_or("父节点不存在。")?;
        let remove = if subtree {
            self.descendants(id)
        } else {
            HashSet::from([id])
        };
        if subtree
            && self.tree().is_some_and(|tree| {
                tree.nodes
                    .iter()
                    .any(|n| remove.contains(&n.id) && matches!(n.kind, NodeKind::Outlet { .. }))
            })
        {
            return Err("此子树含唯一科目出口。请先重挂出口，或仅移除父节点并保留子节点。".into());
        }
        let tree = self.tree_mut().ok_or("没有草稿。")?;
        let children = &mut tree
            .nodes
            .iter_mut()
            .find(|n| n.id == parent)
            .ok_or("父节点不存在。")?
            .children;
        let index = children
            .iter()
            .position(|n| *n == id)
            .ok_or("节点已改变。")?;
        children.remove(index);
        if !subtree {
            for (i, child) in node.children.iter().enumerate() {
                children.insert(index + i, *child);
            }
        }
        tree.nodes.retain(|n| !remove.contains(&n.id));
        self.selected = Some(parent);
        self.page = Page::Tree;
        self.raw.clear();
        self.raw_original.clear();
        Ok(())
    }
    pub(super) fn handle(
        &mut self,
        action: ActionId,
        data: FormData,
        identity: &IdentityState,
    ) -> Result<Command, String> {
        authorized(identity, self.owner)?;
        self.identity = identity.clone();
        if self.page == Page::PublicStyle && action != CLOSE && action != CANCEL {
            return self.style_action(action, data);
        }
        self.suspend(data);
        if action == CLOSE {
            return self.depart(Destination::Close);
        }
        match (self.page, action) {
            (Page::List, PUBLIC) => return self.depart(Destination::Public),
            (Page::List, NEW) => {
                let key = self.target()?;
                return self.depart(Destination::Subject(key));
            }
            (Page::List, OPEN) => {
                let id = self.selected_profile.ok_or("请先选择配置行。")?;
                let key = self
                    .list
                    .items
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or("配置行已改变。")?
                    .key;
                return self.depart(Destination::Subject(key));
            }
            (Page::List, NEXT) => {
                let after = self.list.next_after.ok_or("已是最后一页。")?;
                self.history.push(self.list_after);
                return self.depart(Destination::ListAfter(Some(after)));
            }
            (Page::List, PREVIOUS) => {
                let after = self.history.pop().unwrap_or(None);
                return self.depart(Destination::ListAfter(after));
            }
            (Page::List, FIRST) => {
                self.history.clear();
                return self.depart(Destination::ListAfter(None));
            }
            (Page::Tree, NODE_OPERATIONS) => self.page = Page::Operations,
            (Page::Operations, BACK) => {
                self.page = Page::Tree;
                self.raw.clear();
                self.raw_original.clear();
            }
            (Page::Tree, EDIT_NODE) => {
                let node = self.selected_node().ok_or("请选择节点。")?;
                if matches!(node.kind, NodeKind::Widget(_)) {
                    return Err(
                        "公共组件样式由公共显示设置管理；科目资料样式请打开资料设置。".into(),
                    );
                }
                self.page = Page::Geometry;
                self.raw.clear();
                self.raw_original.clear();
            }
            (Page::Tree, ADD_CONTAINER) => {
                self.add_node(transparent_default())?;
                self.page = Page::Geometry;
            }
            (Page::Tree, ADD_WIDGET) => {
                let kind = widget_parse(&self.value(ADD_KIND))?;
                self.add_node(NodeKind::Widget(kind))?;
            }
            (Page::Tree | Page::Operations, UP) => self.move_node(true)?,
            (Page::Tree | Page::Operations, DOWN) => self.move_node(false)?,
            (Page::Tree | Page::Operations, REPARENT) => self.reparent()?,
            (Page::Tree, NODE_APPEARANCE) => {
                if !matches!(
                    self.selected_node().map(|n| &n.kind),
                    Some(NodeKind::Container(_))
                ) {
                    return Err("只有容器有父组透明度与材质。".into());
                }
                self.page = Page::Appearance;
                self.raw.clear();
                self.raw_original.clear();
            }
            (Page::Geometry | Page::Appearance | Page::Materials, APPLY) => self.apply_fields()?,
            (Page::Geometry | Page::Appearance | Page::Materials, BACK) => {
                self.apply_fields()?;
                self.page = Page::Tree;
            }
            (Page::Tree, MATERIALS) => {
                if !matches!(self.draft, Some(Draft::Subject(_))) {
                    return Err("请选择科目配置后调整其资料显示。".into());
                }
                self.page = Page::Materials;
                self.raw.clear();
                self.raw_original.clear();
            }
            (Page::Materials, CLEAR_FOLDER) => {
                self.apply_fields()?;
                if let Some(Draft::Subject(s)) = &mut self.draft {
                    s.folder_id = None;
                }
            }
            (Page::Tree, PUBLIC_STYLE) => {
                let Some(Draft::Public(s)) = &self.draft else {
                    return Err("科目配置不能修改公共组件样式。".into());
                };
                self.style_original = Some(s.display.clone());
                self.style = Some(display_editor::Editor::new(s.display.clone(), self.owner));
                self.page = Page::PublicStyle;
                self.raw.clear();
                self.raw_original.clear();
            }
            (Page::Tree | Page::Operations, DELETE) => {
                let node = self.selected_node().ok_or("请选择节点。")?;
                if matches!(node.kind, NodeKind::Root { .. } | NodeKind::Outlet { .. }) {
                    return Err("根与内容出口不能删除。".into());
                }
                self.page = Page::Delete;
            }
            (Page::Delete, REMOVE_SUBTREE) => self.remove(true)?,
            (Page::Delete, PROMOTE_CHILDREN) => self.remove(false)?,
            (Page::Delete | Page::Reset, KEEP) => self.page = Page::Tree,
            (Page::Tree | Page::Operations, RESET) => self.page = Page::Reset,
            (Page::Reset, CONFIRM_RESET) => {
                match self.draft.as_mut().ok_or("没有草稿。")? {
                    Draft::Public(s) => s.layout.reset_tree().map_err(issue)?,
                    Draft::Subject(s) => s.reset_tree().map_err(issue)?,
                }
                self.selected = self.tree().map(|t| t.root);
                self.raw.clear();
                self.raw_original.clear();
                self.page = Page::Tree;
            }
            (Page::Tree, REVIEW) => {
                self.preflight()?;
                self.page = Page::Review;
                self.submitted = false;
                self.raw.clear();
                self.raw_original.clear();
            }
            (Page::Review, BACK) => {
                self.page = Page::Tree;
                self.submitted = false;
            }
            (Page::Review, CONFIRM) if !self.submitted => {
                self.preflight()?;
                self.submitted = true;
                return match (self.draft.as_ref(), self.original.as_ref()) {
                    (Some(Draft::Public(s)), Some(Draft::Public(old))) => Ok(Command::SavePublic {
                        expected_layout_revision: old.layout.revision,
                        expected_display_revision: old.display.revision,
                        layout: s.layout.clone(),
                        display: s.display.clone(),
                    }),
                    (Some(Draft::Subject(s)), Some(Draft::Subject(old))) => {
                        Ok(Command::SaveSubject {
                            key: s.key,
                            expected_revision: old.revision,
                            candidate: s.clone(),
                        })
                    }
                    _ => Err("保存目标已改变。".into()),
                };
            }
            (Page::Tree | Page::Review, PREVIEW) => {
                self.preflight()?;
                return match self.draft.as_ref() {
                    Some(Draft::Public(s)) => Ok(Command::Preview {
                        public: Some(s.clone()),
                        subject: None,
                    }),
                    Some(Draft::Subject(s)) => Ok(Command::Preview {
                        public: None,
                        subject: Some(s.clone()),
                    }),
                    None => Err("没有草稿。".into()),
                };
            }
            (_, CANCEL) => return self.depart(Destination::List),
            (Page::Discard, KEEP) => self.page = self.discard_return,
            (Page::Discard, DISCARD) => return self.finish_departure(self.destination),
            _ => return Err("此页不支持该操作。".into()),
        }
        Ok(Command::Stay)
    }
    fn style_action(&mut self, action: ActionId, data: FormData) -> Result<Command, String> {
        let style = self.style.as_mut().ok_or("公共显示草稿不存在。")?;
        let view = style.view("", false);
        let leave = action == BACK
            || action == display_editor::EDIT_BACK
                && view.fields.iter().any(|f| f.id == display_editor::CLOCK);
        let action = if leave {
            display_editor::SAVE_ALL
        } else {
            action
        };
        match style.handle(action, data)? {
            display_editor::Command::Save { settings, .. } => {
                if let Some(Draft::Public(s)) = &mut self.draft {
                    s.display = settings;
                }
                self.style = None;
                self.style_original = None;
                self.page = Page::Tree;
            }
            display_editor::Command::Reload => {
                let Some(Draft::Public(s)) = &self.draft else {
                    return Err("公共草稿不存在。".into());
                };
                let settings = self
                    .style_original
                    .clone()
                    .unwrap_or_else(|| s.display.clone());
                if let Some(Draft::Public(s)) = &mut self.draft {
                    s.display = settings.clone();
                }
                self.style = Some(display_editor::Editor::new(settings, self.owner));
            }
            display_editor::Command::Close => {
                self.page = Page::Tree;
                self.style = None;
            }
            display_editor::Command::Stay => {
                if let (Some(style), Some(Draft::Public(s))) = (&self.style, &mut self.draft) {
                    s.display = style.settings.clone();
                }
            }
        }
        Ok(Command::Stay)
    }
    pub(super) fn view(&self, status: &str, busy: bool) -> View {
        if self.page == Page::PublicStyle {
            if let Some(style) = &self.style {
                let mut view = style.view(status, busy);
                view.title = "公共布局 · 显示样式草稿".into();
                view.description="这里的更改先应用到公共布局草稿，返回树后核对摘要，再联合比较布局与显示修订提交。组件选择与未完整数字仍会保留。".into();
                for a in &mut view.actions {
                    if a.id == display_editor::SAVE_ALL {
                        a.label = "应用显示设置并返回布局".into();
                    }
                }
                return view;
            }
        }
        let mut view = View {
            title: "公共布局与科目配置".into(),
            description: String::new(),
            status: status.into(),
            fields: self.fields(),
            actions: vec![],
            busy,
            grid: None,
        };
        for f in &mut view.fields {
            if let Some(raw) = self.raw.get(&f.id) {
                if !matches!(&f.kind,FieldKind::Choice(items) if !raw.is_empty()&&!items.iter().any(|c|c.value==*raw))
                {
                    f.value = raw.clone();
                }
            }
        }
        match self.page {
            Page::List => {
                view.description = format!(
                    "共{}个科目配置，每页最多20条。目标按班级、科目、实际老师的稳定编号隔离；停用与撤权的历史配置仍可维护。公共时钟/作业等不随科目配置移动。",
                    self.list.total
                );
                let rows = self
                    .list
                    .items
                    .iter()
                    .map(|p| {
                        let teacher = self.identity.account(p.key.teacher_id).ok();
                        let class = self
                            .identity
                            .classes
                            .iter()
                            .find(|c| c.id == p.key.class_id);
                        let subject = self
                            .identity
                            .subjects
                            .iter()
                            .find(|s| s.id == p.key.subject_id);
                        GridRow {
                            group: String::new(),
                            label: format!(
                                "配置#{} · {}",
                                p.id.get(),
                                teacher.map_or_else(
                                    || format!("老师#{}", p.key.teacher_id.get()),
                                    |a| caption(
                                        &a.display_name,
                                        a.principal.id.get(),
                                        a.principal.enabled
                                    )
                                )
                            ),
                            cells: vec![
                                class.map_or_else(
                                    || format!("班级#{}", p.key.class_id.get()),
                                    |c| caption(&c.name, c.id.get(), c.enabled),
                                ),
                                subject.map_or_else(
                                    || format!("科目#{}", p.key.subject_id.get()),
                                    |s| caption(&s.name, s.id.get(), s.enabled),
                                ),
                                if p.enabled { "启用" } else { "停用" }.into(),
                            ],
                            merged: false,
                        }
                    })
                    .collect();
                view.grid = Some(Grid {
                    columns: vec![
                        "配置 / 老师".into(),
                        "班级".into(),
                        "科目".into(),
                        "状态".into(),
                    ],
                    rows,
                    selected: self
                        .selected_profile
                        .and_then(|id| self.list.items.iter().position(|p| p.id == id))
                        .map(|r| (r, 0)),
                    enabled: !busy,
                });
                view.actions = vec![
                    action(PUBLIC, "编辑公共布局", true),
                    action(NEW, "读取 / 创建所选目标", true),
                    action(OPEN, "编辑选中配置", self.selected_profile.is_some()),
                    action(PREVIOUS, "上一页", !self.history.is_empty()),
                    action(NEXT, "下一页", self.list.next_after.is_some()),
                    action(FIRST, "刷新 / 回第一页", true),
                    action(CLOSE, "返回", true),
                ];
            }
            Page::Tree => {
                let order = self.order();
                view.description="选择节点后编辑尺寸、顺序或父节点。公共内容出口限定科目树边界；玻璃最多两级（作业/课表内卡也计一级），超屏内容分页。重排不会修改作业、课表或资料原件。".into();
                let rows = order
                    .iter()
                    .filter_map(|(id, depth)| {
                        self.tree()?.node(*id).map(|node| GridRow {
                            group: String::new(),
                            label: format!("{}#{}", "  ".repeat(*depth), id.get()),
                            cells: vec![
                                self.parent(*id)
                                    .map_or("根".into(), |p| format!("#{}", p.get())),
                                kind_label(&node.kind),
                            ],
                            merged: false,
                        })
                    })
                    .collect();
                view.grid = Some(Grid {
                    columns: vec!["节点编号".into(), "父节点".into(), "类型".into()],
                    rows,
                    selected: self
                        .selected
                        .and_then(|id| order.iter().position(|(n, _)| *n == id))
                        .map(|r| (r, 0)),
                    enabled: !busy,
                });
                view.actions = vec![
                    action(EDIT_NODE, "尺寸 / 排列", true),
                    action(NODE_APPEARANCE, "容器材质 / 组Alpha", true),
                    action(ADD_CONTAINER, "添加透明容器", true),
                    action(ADD_WIDGET, "添加组件", true),
                    action(
                        if matches!(self.draft, Some(Draft::Public(_))) {
                            PUBLIC_STYLE
                        } else {
                            MATERIALS
                        },
                        if matches!(self.draft, Some(Draft::Public(_))) {
                            "公共显示样式"
                        } else {
                            "资料显示 / 启停"
                        },
                        true,
                    ),
                    action(NODE_OPERATIONS, "顺序 / 重挂 / 删除 / 重置…", true),
                    action(PREVIEW, "只读预览草稿", true),
                    action(REVIEW, "保存摘要", true),
                    action(CANCEL, "返回配置列表", true),
                ];
            }
            Page::Operations => {
                view.description="通过所选稳定节点编号调整同级顺序或重挂到父节点。删除父节点时须明确保留重挂子节点，或移除整个展示子树；恢复默认使用全新节点编号。".into();
                view.actions = vec![
                    action(UP, "同级上移", true),
                    action(DOWN, "同级下移", true),
                    action(REPARENT, "重挂到所选父节点", true),
                    action(DELETE, "移除展示节点…", true),
                    action(RESET, "恢复默认树…", true),
                    action(BACK, "返回布局树", true),
                    action(CANCEL, "取消整份草稿…", true),
                ];
            }
            Page::Geometry | Page::Appearance | Page::Materials => {
                view.description=match self.page{Page::Geometry=>"尺寸先确定宽度，再测量内容高度。Content父高度不能依赖子百分比高度；比例以万分位填写。透明根的锚点以系统工作区或科目内容区为边界。",Page::Appearance=>"容器玻璃填充不乘到子文字。父组整体Alpha应用到整个子组一次；0%保留布局占位但无展示命中。",_=>"科目配置只控制自己的资料内容区，不能修改公共时钟、学校科目色或全科作业。目录仍按当前有效老师与科目授权；完整名字与只读分页保留。"}.into();
                view.actions = vec![
                    action(APPLY, "应用到草稿", true),
                    action(BACK, "应用并返回树", true),
                    action(CANCEL, "取消整份草稿…", true),
                ];
                if self.page == Page::Materials {
                    view.actions
                        .push(action(CLEAR_FOLDER, "改用此目标的当前目录", true));
                }
            }
            Page::Review => {
                let tree = self.tree();
                let info = match (&self.draft, &self.original) {
                    (Some(Draft::Public(s)), Some(Draft::Public(old))) => format!(
                        "公共布局修订{}，显示修订{}。树{}，显示样式{}。内容出口更改后，不兼容的科目配置会使用公共默认，请核对。",
                        old.layout.revision.get(),
                        old.display.revision.get(),
                        if s.layout.tree != old.layout.tree {
                            "已修改"
                        } else {
                            "未改"
                        },
                        if s.display != old.display {
                            "已修改"
                        } else {
                            "未改"
                        }
                    ),
                    (Some(Draft::Subject(s)), Some(Draft::Subject(old))) => format!(
                        "目标：班级#{} / 科目#{} / 老师#{}；配置{}，捕获修订{}。配置{}，资料每页{}条、字号{}、Alpha{}%。非当前科目保存后上课时自动使用；停用保留历史与资料。",
                        s.key.class_id.get(),
                        s.key.subject_id.get(),
                        s.key.teacher_id.get(),
                        if self.creating {
                            "保存时分配新编号"
                        } else {
                            "已有稳定编号"
                        },
                        old.revision.get(),
                        if s.enabled { "启用" } else { "停用" },
                        s.materials.visible_rows,
                        s.materials.component.font_dip,
                        s.materials.component.opacity_percent
                    ),
                    _ => "没有保存目标".into(),
                };
                view.description = format!(
                    "确认保存：{info}\n当前树{}个节点。保存采用打开时捕获的修订；冲突或失败保留草稿，不自动刷新重试。预览不保存，也不会发布到课堂。",
                    tree.map_or(0, |t| t.nodes.len())
                );
                view.actions = vec![
                    action(CONFIRM, "确认提交", !self.submitted),
                    action(PREVIEW, "只读预览草稿", true),
                    action(BACK, "继续编辑", true),
                    action(CANCEL, "取消整份草稿…", true),
                ];
            }
            Page::Discard => {
                view.description="存在未保存布局或显示草稿。是否明确丢弃？保留会恢复当前树、选中节点和未完整数字，不提交后台。".into();
                view.actions = vec![
                    action(DISCARD, "确认丢弃", true),
                    action(KEEP, "保留并继续编辑", true),
                ];
            }
            Page::Delete => {
                view.description = format!(
                    "移除展示节点#{}：请选择将子节点重挂到原父节点，或明确删除整个展示子树。两者都不会删除作业正文、课表或资料原件。",
                    self.selected.map_or(0, |id| id.get())
                );
                view.actions = vec![
                    action(PROMOTE_CHILDREN, "移除父节点，子节点重挂原父", true),
                    action(REMOVE_SUBTREE, "移除整个展示子树", true),
                    action(KEEP, "保留节点", true),
                ];
            }
            Page::Reset => {
                view.description="恢复默认排列会替换整棵展示树，为所有节点分配高水位之后的新编号。显示设置、科目资料和业务内容保留；只有确认保存后才生效。".into();
                view.actions = vec![
                    action(CONFIRM_RESET, "确认恢复默认树", true),
                    action(KEEP, "保留当前树", true),
                ];
            }
            Page::PublicStyle => {}
        }
        view
    }
}
fn transparent_default() -> NodeKind {
    NodeKind::Container(ContainerSettings {
        flow: Flow::Column,
        padding: 0,
        gap: 12,
        width: AxisSize::Content {
            min: 0,
            preferred: 200,
            max: 800,
        },
        height: AxisSize::Content {
            min: 0,
            preferred: 0,
            max: 2160,
        },
        group_opacity_percent: 100,
        material: None,
    })
}

impl Editor {
    /// Canvas and Grid both address the exact currently edited scope. Selection
    /// never switches profiles or silently abandons an unparsed node form.
    pub(super) fn selected_node_key(&self) -> Option<fc_presentation::layout::SceneNodeKey> {
        let tree = self.tree()?;
        let id = self.selected?;
        tree.node(id)?;
        Some(fc_presentation::layout::SceneNodeKey {
            scope: tree.scope,
            id,
        })
    }
    pub(super) fn select_node(
        &mut self,
        scope: LayoutScope,
        id: LayoutNodeId,
    ) -> Result<(), String> {
        if self.page != Page::Tree {
            return Err("请先应用当前字段并返回布局树，再选择画布节点。".into());
        }
        let tree = self.tree().ok_or("没有布局草稿。")?;
        if tree.scope != scope || tree.node(id).is_none() {
            return Err("画布节点不属于当前布局作用域。".into());
        }
        self.selected = Some(id);
        self.raw.remove(&PARENT);
        Ok(())
    }
    /// Explicit drag/drop changes only logical order/parent links. Candidate
    /// validation completes before replacing the draft; failures leave it intact.
    pub(super) fn drop_node(
        &mut self,
        scope: LayoutScope,
        source: LayoutNodeId,
        target: LayoutNodeId,
        before: Option<LayoutNodeId>,
    ) -> Result<(), String> {
        if self.page != Page::Tree {
            return Err("请先应用当前字段并返回布局树，再拖动节点。".into());
        }
        let tree = self.tree().ok_or("没有布局草稿。")?;
        if tree.scope != scope {
            return Err("不能跨公共/科目或跨老师重挂节点。".into());
        }
        if source == tree.root {
            return Err("透明根不能移动。".into());
        }
        if tree.node(source).is_none()
            || tree
                .node(target)
                .is_none_or(|n| matches!(n.kind, NodeKind::Widget(_)))
        {
            return Err("源节点或目标容器已改变。".into());
        }
        if self.descendants(source).contains(&target) {
            return Err("不能将节点重挂到自身或后代。".into());
        }
        let old_parent = self.parent(source).ok_or("源节点没有父节点。")?;
        if before == Some(source) && old_parent == target {
            return Ok(());
        }
        if before.is_some_and(|id| tree.node(target).is_none_or(|p| !p.children.contains(&id))) {
            return Err("插入位置不是目标容器的当前子节点。".into());
        }
        let mut candidate = self.draft.clone().ok_or("没有布局草稿。")?;
        let tree = match &mut candidate {
            Draft::Public(s) => &mut s.layout.tree,
            Draft::Subject(s) => &mut s.tree,
        };
        tree.nodes
            .iter_mut()
            .find(|n| n.id == old_parent)
            .ok_or("原父节点不存在。")?
            .children
            .retain(|n| *n != source);
        let children = &mut tree
            .nodes
            .iter_mut()
            .find(|n| n.id == target)
            .ok_or("目标父节点不存在。")?
            .children;
        let position = before
            .and_then(|id| children.iter().position(|n| *n == id))
            .unwrap_or(children.len());
        children.insert(position, source);
        tree.validate_structure().map_err(issue)?;
        self.draft = Some(candidate);
        self.selected = Some(source);
        self.raw.clear();
        self.raw_original.clear();
        self.submitted = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::{auth::*, identity::*};
    fn pid(n: u64) -> PrincipalId {
        PrincipalId::new(n).unwrap()
    }
    fn nid(n: u64) -> LayoutNodeId {
        LayoutNodeId::new(n).unwrap()
    }
    fn key() -> SubjectProfileKey {
        SubjectProfileKey {
            class_id: ClassId::new(1).unwrap(),
            subject_id: SubjectId::new(1).unwrap(),
            teacher_id: pid(2),
        }
    }
    fn identity() -> IdentityState {
        let school = SchoolId::new(1).unwrap();
        let accounts = (1..=3)
            .map(|n| UserAccount {
                principal: Principal {
                    id: pid(n),
                    school_id: school,
                    role: if n == 1 {
                        Role::GlobalAdmin
                    } else {
                        Role::TeacherAdmin
                    },
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("teacher{n}"),
                display_name: format!("老师{n}"),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            })
            .collect();
        let state = IdentityState {
            revision: Revision::new(5),
            installation: Some(Installation {
                school_id: school,
                school_name: "学校".into(),
                current_class: Some(key().class_id),
            }),
            accounts,
            classes: vec![SchoolClass {
                id: key().class_id,
                school_id: school,
                name: "班级".into(),
                enabled: true,
            }],
            subjects: vec![SchoolSubject {
                id: key().subject_id,
                school_id: school,
                name: "数学".into(),
                enabled: true,
                color: Rgb {
                    red: 20,
                    green: 30,
                    blue: 40,
                },
                revision: Revision::new(1),
            }],
            assignments: [2, 3]
                .into_iter()
                .map(|n| TeachingAssignment {
                    teacher_id: pid(n),
                    class_id: key().class_id,
                    subject_id: key().subject_id,
                    enabled: true,
                })
                .collect(),
            default_teachers: vec![],
        };
        state.validate().unwrap();
        state
    }
    fn empty() -> ProfilePage {
        ProfilePage {
            items: vec![],
            total: 0,
            next_after: None,
        }
    }
    fn editor() -> Editor {
        Editor::new(&identity(), pid(1), empty()).unwrap()
    }
    fn public() -> PublicLayoutState {
        PublicLayoutState {
            layout: LayoutDocument {
                revision: Revision::new(3),
                ..LayoutDocument::default()
            },
            display: DisplaySettings {
                revision: Revision::new(4),
                ..DisplaySettings::default()
            },
        }
    }
    fn subject() -> SubjectProfileDocument {
        let mut d = SubjectProfileDocument::new(ProfileId::new(7).unwrap(), key());
        d.revision = Revision::new(5);
        d.layout_revision = Revision::new(2);
        d
    }
    fn form(e: &Editor, overrides: &[(FieldId, &str)]) -> FormData {
        let mut data = FormData::new();
        for f in e.view("", false).fields {
            let value = overrides
                .iter()
                .find(|(id, _)| *id == f.id)
                .map_or(f.value, |(_, s)| (*s).into());
            data.insert(f.id, value).unwrap();
        }
        data
    }
    fn act(e: &mut Editor, action: ActionId, overrides: &[(FieldId, &str)]) -> Command {
        let data = form(e, overrides);
        e.handle(action, data, &identity()).unwrap()
    }
    fn valid(e: &Editor) {
        e.view("", false).validate().unwrap();
    }
    fn tree(e: &Editor) -> LayoutTree {
        e.tree().unwrap().clone()
    }
    #[test]
    fn exact_owner_and_scope_keys_are_required_without_caching_secrets() {
        assert!(Editor::new(&identity(), pid(2), empty()).is_err());
        let mut e = editor();
        assert_eq!(e.owner(), pid(1));
        valid(&e);
        let command = act(&mut e, NEW, &[(TEACHER, "3")]);
        assert!(matches!(command,Command::LoadSubject{key:k} if k.teacher_id==pid(3)));
        e.loaded_subject(key(), None).unwrap();
        assert!(e.has_unsaved_changes());
        assert!(e.creating);
        valid(&e);
        let mut data = form(&e, &[]);
        data.insert(FieldId(606), "do not cache a password".into())
            .unwrap();
        e.suspend(data);
        assert!(!e.raw.contains_key(&FieldId(606)));
        let mut revoked = identity();
        revoked.accounts[0].principal.role = Role::User;
        assert!(e.handle(REVIEW, FormData::new(), &revoked).is_err());
    }
    #[test]
    fn directory_pages_keep_stable_profile_ids_and_valid_long_labels() {
        let mut identity = identity();
        identity.accounts[1].display_name = "🦀".repeat(128);
        identity.classes[0].name = "🦀".repeat(128);
        identity.subjects[0].name = "🦀".repeat(64);
        identity.validate().unwrap();
        let items = (1..=20)
            .map(|n| fc_ports::ProfileSummary {
                id: ProfileId::new(n).unwrap(),
                key: key(),
                revision: Revision::new(1),
                enabled: n % 2 == 0,
            })
            .collect();
        let page = ProfilePage {
            items,
            total: 4096,
            next_after: Some(ProfileId::new(20).unwrap()),
        };
        let mut e = Editor::new(&identity, pid(1), page).unwrap();
        valid(&e);
        assert_eq!(e.view("", false).grid.unwrap().rows.len(), 20);
        e.select(19, 3).unwrap();
        assert!(
            matches!(e.handle(OPEN,FormData::new(),&identity).unwrap(),Command::LoadSubject{key:k} if k==key())
        );
        e.loaded_list(ProfilePage {
            items: vec![],
            total: 4096,
            next_after: None,
        })
        .unwrap();
        assert!(e.select(0, 0).is_err());
        let mut too_many = empty();
        too_many.items = (1..=21)
            .map(|n| fc_ports::ProfileSummary {
                id: ProfileId::new(n).unwrap(),
                key: key(),
                revision: Revision::new(1),
                enabled: true,
            })
            .collect();
        too_many.total = 21;
        assert!(e.loaded_list(too_many).is_err());
    }
    #[test]
    fn first_page_refresh_reads_created_profile_and_later_disabled_state() {
        let identity = identity();
        let date = fc_domain::time::LocalDate::new(2026, 10, 31).unwrap();
        let now = fc_domain::time::LocalDateTime::new(date, 9 * 3600).unwrap();
        let actor = ActorContext {
            principal_id: pid(1),
            school_id: identity.installation.as_ref().unwrap().school_id,
            auth_epoch: identity.accounts[0].principal.auth_epoch,
            expires_at: fc_domain::time::LocalDateTime::new(date, 86399).unwrap(),
            origin: TrustedOrigin::Local,
        };
        let page = |document: &SubjectProfileDocument| ProfilePage {
            items: vec![fc_ports::ProfileSummary {
                id: document.id,
                key: document.key,
                revision: document.revision,
                enabled: document.enabled,
            }],
            total: 1,
            next_after: None,
        };
        let mut e = editor();
        e.loaded_subject(key(), None).unwrap();
        act(&mut e, REVIEW, &[]);
        let Command::SaveSubject {
            expected_revision,
            mut candidate,
            ..
        } = act(&mut e, CONFIRM, &[])
        else {
            panic!("expected create command")
        };
        candidate.id = ProfileId::new(7).unwrap();
        let created = SubjectProfileDocument::prepare_transition(
            None,
            candidate,
            expected_revision,
            &identity,
            &actor,
            now,
            None,
        )
        .unwrap();
        e.loaded_subject(key(), Some(created.clone())).unwrap();
        assert!(!e.has_unsaved_changes());
        assert!(matches!(act(&mut e, CANCEL, &[]), Command::Stay));
        assert_eq!(e.view("", false).grid.unwrap().rows.len(), 0);
        let refresh = e
            .view("", false)
            .actions
            .into_iter()
            .find(|a| a.id == FIRST)
            .unwrap();
        assert!(refresh.enabled);
        assert_eq!(refresh.label, "刷新 / 回第一页");
        assert!(matches!(
            act(&mut e, FIRST, &[]),
            Command::ReloadList { after: None }
        ));
        e.loaded_list(page(&created)).unwrap();
        valid(&e);
        assert_eq!(e.view("", false).grid.unwrap().rows[0].cells[2], "启用");
        e.select(0, 0).unwrap();
        assert!(matches!(act(&mut e, OPEN, &[]), Command::LoadSubject { key:k } if k == key()));
        e.loaded_subject(key(), Some(created.clone())).unwrap();
        act(&mut e, MATERIALS, &[]);
        act(&mut e, BACK, &[(ENABLED, "0")]);
        act(&mut e, REVIEW, &[]);
        let Command::SaveSubject {
            expected_revision,
            candidate,
            ..
        } = act(&mut e, CONFIRM, &[])
        else {
            panic!("expected disable command")
        };
        let disabled = SubjectProfileDocument::prepare_transition(
            Some(&created),
            candidate,
            expected_revision,
            &identity,
            &actor,
            now,
            None,
        )
        .unwrap();
        assert!(!disabled.enabled);
        e.loaded_subject(key(), Some(disabled.clone())).unwrap();
        act(&mut e, CANCEL, &[]);
        assert!(
            e.view("", false)
                .actions
                .iter()
                .find(|a| a.id == FIRST)
                .unwrap()
                .enabled
        );
        assert!(matches!(
            act(&mut e, FIRST, &[]),
            Command::ReloadList { after: None }
        ));
        e.loaded_list(page(&disabled)).unwrap();
        valid(&e);
        assert_eq!(e.view("", false).grid.unwrap().rows[0].cells[2], "停用");
        assert_eq!(e.list.items[0].id, created.id);
        assert_eq!(e.list.items[0].revision, disabled.revision);
    }
    #[test]
    fn public_style_is_one_joint_draft_and_keeps_both_captured_revisions() {
        let mut e = editor();
        e.loaded_public(public()).unwrap();
        assert!(!e.has_unsaved_changes());
        valid(&e);
        act(&mut e, PUBLIC_STYLE, &[]);
        valid(&e);
        act(
            &mut e,
            display_editor::APPLY,
            &[
                (display_editor::GLASS, "0"),
                (display_editor::REFLECTION, "100"),
            ],
        );
        assert!(e.has_unsaved_changes());
        act(&mut e, display_editor::EDIT_BACK, &[]);
        assert_eq!(e.page, Page::Tree);
        act(&mut e, REVIEW, &[]);
        valid(&e);
        match act(&mut e, CONFIRM, &[]) {
            Command::SavePublic {
                expected_layout_revision,
                expected_display_revision,
                layout,
                display,
            } => {
                assert_eq!(expected_layout_revision, Revision::new(3));
                assert_eq!(expected_display_revision, Revision::new(4));
                assert_eq!(layout.revision, Revision::new(3));
                assert_eq!(display.glass_surface_percent, 0);
                assert_eq!(display.edge_reflection_percent, 100);
            }
            _ => panic!("expected joint save"),
        }
        assert!(e.has_unsaved_changes());
        assert!(e.handle(CONFIRM, FormData::new(), &identity()).is_err());
        act(&mut e, BACK, &[]);
        act(&mut e, REVIEW, &[]);
        assert!(matches!(
            act(&mut e, CONFIRM, &[]),
            Command::SavePublic { .. }
        ));
    }
    #[test]
    fn public_style_local_cancel_restores_entry_style_without_losing_tree_changes() {
        let mut e = editor();
        e.loaded_public(public()).unwrap();
        e.select_node(LayoutScope::Public, nid(4)).unwrap();
        act(&mut e, DOWN, &[]);
        let reordered = tree(&e);
        act(&mut e, PUBLIC_STYLE, &[]);
        act(
            &mut e,
            display_editor::APPLY,
            &[(display_editor::COURSE_ALPHA, "55")],
        );
        act(&mut e, display_editor::CANCEL, &[]);
        act(&mut e, display_editor::CONFIRM_CANCEL, &[]);
        let Some(Draft::Public(s)) = &e.draft else {
            panic!()
        };
        assert_eq!(s.display, public().display);
        assert_eq!(s.layout.tree, reordered);
        assert!(e.has_unsaved_changes());
        valid(&e);
    }
    #[test]
    fn unfinished_numbers_survive_lock_cancel_keep_and_never_mutate_geometry() {
        let mut e = editor();
        e.loaded_subject(key(), Some(subject())).unwrap();
        let before = tree(&e);
        act(&mut e, EDIT_NODE, &[]);
        let data = form(&e, &[(HEIGHT_PREF, "")]);
        assert!(e.handle(APPLY, data, &identity()).is_err());
        assert_eq!(tree(&e), before);
        assert!(e.has_unsaved_changes());
        valid(&e);
        let data = form(&e, &[]);
        e.suspend(data);
        assert_eq!(e.page, Page::Geometry);
        assert_eq!(
            e.view("", false)
                .fields
                .iter()
                .find(|f| f.id == HEIGHT_PREF)
                .unwrap()
                .value,
            ""
        );
        act(&mut e, CLOSE, &[]);
        assert_eq!(e.page, Page::Discard);
        assert!(e.has_unsaved_changes());
        e.suspend(FormData::new());
        assert!(e.has_unsaved_changes());
        act(&mut e, KEEP, &[]);
        assert_eq!(e.page, Page::Geometry);
        assert!(e.select_node(LayoutScope::Subject(key()), nid(2)).is_err());
        act(&mut e, CANCEL, &[]);
        assert!(matches!(act(&mut e, DISCARD, &[]), Command::Stay));
        assert_eq!(e.page, Page::List);
        assert!(!e.has_unsaved_changes());
    }
    #[test]
    fn material_settings_are_scoped_bounded_and_backend_failure_preserves_draft() {
        let mut e = editor();
        e.loaded_subject(key(), Some(subject())).unwrap();
        act(&mut e, MATERIALS, &[]);
        valid(&e);
        let data = form(&e, &[(ROWS, "6"), (FONT, "28")]);
        assert!(e.handle(APPLY, data, &identity()).is_err());
        let Some(Draft::Subject(s)) = &e.draft else {
            panic!()
        };
        assert_eq!(s.materials.visible_rows, 5);
        assert_eq!(s.materials.component.font_dip, 22);
        act(
            &mut e,
            BACK,
            &[(ROWS, "2"), (FONT, "28"), (ALPHA, "0"), (ROW_GAP, "16")],
        );
        e.select_node(LayoutScope::Subject(key()), nid(2)).unwrap();
        assert_eq!(
            e.selected_node_key(),
            Some(fc_presentation::layout::SceneNodeKey {
                scope: LayoutScope::Subject(key()),
                id: nid(2)
            })
        );
        act(&mut e, REVIEW, &[]);
        valid(&e);
        match act(&mut e, CONFIRM, &[]) {
            Command::SaveSubject {
                expected_revision,
                candidate,
                key: k,
            } => {
                assert_eq!(expected_revision, Revision::new(5));
                assert_eq!(k, key());
                assert_eq!(candidate.materials.visible_rows, 2);
                assert_eq!(candidate.materials.component.opacity_percent, 0);
                assert_eq!(candidate.materials.component.font_dip, 28);
                assert_eq!(candidate.materials.row_gap_dip, 16);
            }
            _ => panic!(),
        }
        assert!(e.has_unsaved_changes());
        act(&mut e, BACK, &[]);
        act(&mut e, REVIEW, &[]);
        assert!(
            matches!(act(&mut e,CONFIRM,&[]),Command::SaveSubject{expected_revision,..} if expected_revision==Revision::new(5))
        );
    }
    #[test]
    fn historical_revoked_profile_can_be_disabled_but_cannot_be_reenabled() {
        let mut e = editor();
        e.loaded_subject(key(), Some(subject())).unwrap();
        let mut revoked = identity();
        revoked.accounts[1].principal.role = Role::User;
        revoked.assignments.retain(|a| a.teacher_id != pid(2));
        revoked.validate().unwrap();
        assert!(e.handle(REVIEW, FormData::new(), &revoked).is_err());
        e.handle(MATERIALS, FormData::new(), &revoked).unwrap();
        let data = form(&e, &[(ENABLED, "0")]);
        e.handle(BACK, data, &revoked).unwrap();
        e.handle(REVIEW, FormData::new(), &revoked).unwrap();
        assert!(
            matches!(e.handle(CONFIRM,FormData::new(),&revoked).unwrap(),Command::SaveSubject{candidate,..} if !candidate.enabled)
        );
        e.handle(BACK, FormData::new(), &revoked).unwrap();
        e.handle(MATERIALS, FormData::new(), &revoked).unwrap();
        let data = form(&e, &[(ENABLED, "1")]);
        e.handle(BACK, data, &revoked).unwrap();
        assert!(e.handle(REVIEW, FormData::new(), &revoked).is_err());
    }
    #[test]
    fn canvas_drop_is_atomic_scope_bound_and_keeps_stable_order() {
        let mut e = editor();
        e.loaded_public(public()).unwrap();
        let before = tree(&e);
        assert!(e
            .drop_node(LayoutScope::Subject(key()), nid(4), nid(2), None)
            .is_err());
        assert_eq!(tree(&e), before);
        assert!(e
            .drop_node(LayoutScope::Public, nid(1), nid(2), None)
            .is_err());
        assert!(e
            .drop_node(LayoutScope::Public, nid(2), nid(4), None)
            .is_err());
        assert!(e
            .drop_node(LayoutScope::Public, nid(11), nid(2), None)
            .is_err());
        assert!(e
            .drop_node(LayoutScope::Public, nid(2), nid(2), None)
            .is_err());
        assert_eq!(tree(&e), before);
        e.drop_node(LayoutScope::Public, nid(7), nid(2), Some(nid(4)))
            .unwrap();
        assert_eq!(
            e.tree().unwrap().node(nid(2)).unwrap().children,
            vec![nid(7), nid(4), nid(5), nid(6)]
        );
        assert_eq!(e.selected, Some(nid(7)));
        e.tree().unwrap().validate().unwrap();
        assert!(e.has_unsaved_changes());
    }
    #[test]
    fn deleting_parent_requires_an_explicit_strategy_and_keeps_child_ids() {
        let mut e = editor();
        e.loaded_subject(key(), Some(subject())).unwrap();
        act(&mut e, ADD_CONTAINER, &[]);
        act(&mut e, BACK, &[]);
        let container = e.selected.unwrap();
        e.drop_node(LayoutScope::Subject(key()), nid(2), container, None)
            .unwrap();
        e.select_node(LayoutScope::Subject(key()), container)
            .unwrap();
        act(&mut e, DELETE, &[]);
        assert_eq!(e.page, Page::Delete);
        valid(&e);
        act(&mut e, PROMOTE_CHILDREN, &[]);
        assert!(e.tree().unwrap().node(container).is_none());
        assert!(e.tree().unwrap().node(nid(2)).is_some());
        assert_eq!(
            e.tree().unwrap().node(nid(1)).unwrap().children,
            vec![nid(2)]
        );
        e.select_node(LayoutScope::Subject(key()), nid(1)).unwrap();
        act(&mut e, ADD_CONTAINER, &[]);
        assert!(e.selected.unwrap().get() > container.get());
        let mut e = editor();
        e.loaded_public(public()).unwrap();
        e.select_node(LayoutScope::Public, nid(3)).unwrap();
        act(&mut e, DELETE, &[]);
        assert!(e
            .handle(REMOVE_SUBTREE, FormData::new(), &identity())
            .is_err());
        assert!(e.tree().unwrap().node(nid(10)).is_some());
    }
    #[test]
    fn reset_ids_are_fresh_and_late_loads_cannot_overwrite_dirty_drafts() {
        let mut e = editor();
        e.loaded_subject(key(), Some(subject())).unwrap();
        act(&mut e, RESET, &[]);
        valid(&e);
        act(&mut e, CONFIRM_RESET, &[]);
        assert!(e.tree().unwrap().nodes.iter().all(|n| n.id.get() > 2));
        assert!(e.loaded_public(public()).is_err());
        assert!(e.loaded_subject(key(), Some(subject())).is_err());
        let source = tree(&e);
        act(&mut e, CLOSE, &[]);
        act(&mut e, KEEP, &[]);
        assert_eq!(tree(&e), source);
        assert_eq!(e.page, Page::Tree);
        act(&mut e, CLOSE, &[]);
        assert!(matches!(act(&mut e, DISCARD, &[]), Command::Close));
        assert!(!e.has_unsaved_changes());
        e.loaded_public(public()).unwrap();
        valid(&e);
    }
    #[test]
    fn every_page_reserves_the_native_lock_action_budget() {
        fn budget(e: &Editor) {
            let view = e.view("", false);
            view.validate().unwrap();
            let has_lock = view.actions.iter().any(|a| a.id == ActionId(6));
            assert!(view.actions.len() + usize::from(!has_lock) <= 12);
        }
        let mut e = editor();
        budget(&e);
        e.loaded_public(public()).unwrap();
        budget(&e);
        act(&mut e, NODE_OPERATIONS, &[]);
        budget(&e);
        act(&mut e, BACK, &[]);
        act(&mut e, EDIT_NODE, &[]);
        budget(&e);
        act(&mut e, BACK, &[]);
        e.select_node(LayoutScope::Public, nid(2)).unwrap();
        act(&mut e, NODE_APPEARANCE, &[]);
        budget(&e);
        act(&mut e, BACK, &[]);
        act(&mut e, PUBLIC_STYLE, &[]);
        budget(&e);
        act(&mut e, display_editor::COMPONENTS, &[]);
        budget(&e);
        act(&mut e, display_editor::EDIT_BACK, &[]);
        act(&mut e, display_editor::EDIT_BACK, &[]);
        act(&mut e, RESET, &[]);
        budget(&e);
        act(&mut e, KEEP, &[]);
        e.select_node(LayoutScope::Public, nid(4)).unwrap();
        act(&mut e, DELETE, &[]);
        budget(&e);
        act(&mut e, KEEP, &[]);
        act(&mut e, REVIEW, &[]);
        budget(&e);
        act(&mut e, BACK, &[]);
        e.select_node(LayoutScope::Public, nid(4)).unwrap();
        act(&mut e, DOWN, &[]);
        act(&mut e, CANCEL, &[]);
        budget(&e);
        let mut e = editor();
        e.loaded_subject(key(), Some(subject())).unwrap();
        act(&mut e, MATERIALS, &[]);
        budget(&e);
    }
    #[test]
    fn container_geometry_and_group_material_are_separate_nonsecret_settings() {
        let mut e = editor();
        e.loaded_subject(key(), Some(subject())).unwrap();
        act(&mut e, ADD_CONTAINER, &[]);
        act(
            &mut e,
            BACK,
            &[
                (FLOW, "row"),
                (WIDTH_MODE, "fixed"),
                (WIDTH_PREF, "300"),
                (HEIGHT_MODE, "fixed"),
                (HEIGHT_PREF, "200"),
                (PADDING, "16"),
                (GAP, "8"),
            ],
        );
        let container = e.selected.unwrap();
        act(&mut e, NODE_APPEARANCE, &[]);
        act(
            &mut e,
            BACK,
            &[
                (GROUP_ALPHA, "50"),
                (GLASS, "1"),
                (SURFACE, "3"),
                (EDGE, "25"),
            ],
        );
        e.drop_node(LayoutScope::Subject(key()), nid(2), container, None)
            .unwrap();
        e.tree().unwrap().validate().unwrap();
        let NodeKind::Container(c) = &e.tree().unwrap().node(container).unwrap().kind else {
            panic!()
        };
        assert_eq!(c.width, AxisSize::Fixed(300));
        assert_eq!(c.height, AxisSize::Fixed(200));
        assert_eq!(c.flow, Flow::Row);
        assert_eq!(c.padding, 16);
        assert_eq!(c.group_opacity_percent, 50);
        assert_eq!(
            c.material,
            Some(GlassMaterial {
                surface_percent: 3,
                edge_percent: 25
            })
        );
        let Some(Draft::Subject(s)) = &e.draft else {
            panic!()
        };
        assert_eq!(s.materials.component.opacity_percent, 100);
        assert_eq!(s.materials.component.font_dip, 22);
    }
    #[test]
    fn configured_node_capacity_refuses_addition_without_changing_ids_or_tree() {
        let mut state = public();
        for n in 13..=128 {
            state.layout.tree.nodes[0].children.push(nid(n));
            state.layout.tree.nodes.push(LayoutNode {
                id: nid(n),
                kind: transparent_default(),
                children: vec![],
            });
        }
        state.layout.update_id_high_water();
        state.layout.validate_structure().unwrap();
        let mut e = editor();
        e.loaded_public(state).unwrap();
        let before = tree(&e);
        valid(&e);
        assert_eq!(e.view("", false).grid.unwrap().rows.len(), 128);
        assert!(e
            .handle(ADD_CONTAINER, FormData::new(), &identity())
            .is_err());
        assert_eq!(tree(&e), before);
        assert!(!e.has_unsaved_changes());
    }
    #[test]
    fn review_preview_owns_candidate_without_committing_or_advancing_cas() {
        let mut e = editor();
        e.loaded_subject(key(), Some(subject())).unwrap();
        act(&mut e, RESET, &[]);
        act(&mut e, CONFIRM_RESET, &[]);
        let expected = tree(&e);
        act(&mut e, REVIEW, &[]);
        let Command::Preview { public, subject } = act(&mut e, PREVIEW, &[]) else {
            panic!()
        };
        assert!(public.is_none());
        let subject = subject.unwrap();
        assert_eq!(subject.tree, expected);
        assert_eq!(subject.revision, Revision::new(5));
        assert_eq!(e.page, Page::Review);
        assert!(e.has_unsaved_changes());
        assert!(!e.submitted);
    }
}
