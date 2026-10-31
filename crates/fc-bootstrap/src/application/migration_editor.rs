//! Pure cross-installation mapping draft and explicit consent. Source files,
//! previews, credentials and cold-switch capabilities stay with the coordinator.
use fc_domain::{
    auth::Role,
    identity::IdentityState,
    ids::{ClassId, PrincipalId, Revision, SubjectId},
    recovery::{MigrationCatalog, MigrationError, MigrationMapping},
};
use fc_presentation::workbench::*;
use std::collections::BTreeSet;

#[cfg(test)]
#[path = "migration_editor/tests.rs"]
mod tests;

pub(super) const PREVIOUS: ActionId = ActionId(740);
pub(super) const NEXT: ActionId = ActionId(741);
pub(super) const SAVE_DRAFT: ActionId = ActionId(742);
pub(super) const PREFLIGHT: ActionId = ActionId(743);
pub(super) const CONFIRM: ActionId = ActionId(744);
pub(super) const CANCEL: ActionId = ActionId(745);
pub(super) const CLOSE: ActionId = ActionId(746);
pub(super) const MANAGE_TARGETS: ActionId = ActionId(747);
pub(super) const PREVIOUS_PATH: ActionId = ActionId(748);
pub(super) const NEXT_PATH: ActionId = ActionId(749);
pub(super) const ACK_REPLACEMENT: FieldId = FieldId(753);
pub(super) const ACK_AUTHORITY: FieldId = FieldId(754);
const SOURCE: FieldId = FieldId(740);
const TARGET: FieldId = FieldId(741);
const PAGE: FieldId = FieldId(742);
const PATH: FieldId = FieldId(743);
const PATH_PAGE: FieldId = FieldId(744);
const ROW_FIRST: u16 = 745;
const PER_PAGE: usize = 5;

// Five source rows give 3 metadata + 2 path + 10 row fields = 15. Review
// acknowledgement fields exist only after all mapping row fields disappear.
pub(super) struct Inspection {
    pub source_catalog: MigrationCatalog,
    pub source_installation: [u8; 32],
    pub source_revision: Revision,
    pub target_identity: IdentityState,
    pub target_installation: [u8; 32],
    pub target_global_revision: Revision,
    pub selected_file: String,
}
pub(super) struct ReviewSummary {
    pub source_revision: Revision,
    pub current_revision: Revision,
    pub document_count: u64,
    pub material_count: u64,
    pub material_bytes: u64,
    pub profile_media_count: u64,
    pub profile_media_bytes: u64,
    pub database_bytes: u64,
}
pub(super) enum Command {
    Stay,
    Cancel,
    Close,
    ManageTargets,
    Preflight {
        sequence: u64,
        draft_revision: u64,
        mapping: MigrationMapping,
    },
    Confirm {
        sequence: u64,
        draft_revision: u64,
    },
}
#[derive(Clone, Copy)]
enum Kind {
    Class,
    Subject,
    Principal,
    Summary,
}
impl Kind {
    fn key(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Subject => "subject",
            Self::Principal => "account",
            Self::Summary => "summary",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Class => "班级",
            Self::Subject => "科目",
            Self::Principal => "账号",
            Self::Summary => "完整映射摘要",
        }
    }
}
struct Page {
    kind: Kind,
    sources: Vec<u64>,
}
enum Stage {
    Draft,
    Pending,
    Review(ReviewSummary),
    Confirmed,
    Invalid,
}
struct DisplayPath {
    pages: Vec<String>,
    page: usize,
}
fn safe_char(c: char) -> char {
    if c.is_control()
        || matches!(c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
    {
        '\u{fffd}'
    } else {
        c
    }
}
fn text(value: &str, max_bytes: usize) -> String {
    let mut out = String::new();
    for c in value.chars().map(safe_char) {
        if out.len() + c.len_utf8() > max_bytes {
            break;
        }
        out.push(c);
    }
    out
}
fn choice_label(value: &str) -> String {
    let mut out = text(value, 470);
    if out.len() < value.len() {
        out.push('…');
    }
    out
}
fn installation(value: &[u8; 32]) -> String {
    value.iter().map(|b| format!("{b:02x}")).collect()
}
fn readonly(id: FieldId, label: &str, value: String, max_utf16: usize) -> Field {
    Field {
        id,
        label: label.into(),
        kind: FieldKind::ReadOnly,
        value,
        max_utf16,
    }
}
impl DisplayPath {
    fn new(value: &str) -> Result<Self, String> {
        if value.is_empty()
            || value.encode_utf16().count() > 32767
            || value.chars().any(|c| safe_char(c) != c)
        {
            return Err("所选备份位置无法安全完整显示，请重新选择可读路径。".into());
        }
        let mut pages = Vec::new();
        let mut page = String::new();
        let mut units = 0;
        for c in value.chars() {
            if units + c.len_utf16() > 1024 {
                pages.push(std::mem::take(&mut page));
                units = 0;
            }
            units += c.len_utf16();
            page.push(c);
        }
        pages.push(page);
        Ok(Self { pages, page: 0 })
    }
}
pub(super) struct Editor {
    owner: PrincipalId,
    sequence: u64,
    draft_revision: u64,
    page_generation: u64,
    inspection: Inspection,
    path: DisplayPath,
    pages: Vec<Page>,
    page: usize,
    mapping: MigrationMapping,
    stage: Stage,
}
impl Editor {
    pub fn new(owner: PrincipalId, sequence: u64, inspection: Inspection) -> Result<Self, String> {
        inspection.source_catalog.validate().map_err(problem)?;
        inspection
            .target_identity
            .validate()
            .map_err(|_| "当前学校目录无效，请先通过现有管理页面修复。".to_string())?;
        let account = inspection
            .target_identity
            .account(owner)
            .map_err(|_| "当前主管理员身份已变化。".to_string())?;
        if sequence == 0
            || inspection.source_installation == [0; 32]
            || inspection.target_installation == [0; 32]
            || inspection.source_revision == Revision::default()
            || inspection.target_global_revision == Revision::default()
            || !account.principal.enabled
            || account.principal.role != Role::GlobalAdmin
            || account.must_change_password
        {
            return Err("跨安装映射需要当前有效的主管理员和完整来源信息。".into());
        }
        let school = inspection
            .target_identity
            .installation
            .as_ref()
            .ok_or("目标学校尚未设置。")?
            .school_id;
        let path = DisplayPath::new(&inspection.selected_file)?;
        let mut pages = Vec::new();
        for (kind, ids) in [
            (
                Kind::Class,
                inspection
                    .source_catalog
                    .classes
                    .iter()
                    .map(|x| x.id.get())
                    .collect::<Vec<_>>(),
            ),
            (
                Kind::Subject,
                inspection
                    .source_catalog
                    .subjects
                    .iter()
                    .map(|x| x.id.get())
                    .collect(),
            ),
            (
                Kind::Principal,
                inspection
                    .source_catalog
                    .principals
                    .iter()
                    .map(|x| x.id.get())
                    .collect(),
            ),
        ] {
            if ids.is_empty() {
                pages.push(Page {
                    kind,
                    sources: vec![],
                });
            } else {
                for chunk in ids.chunks(PER_PAGE) {
                    pages.push(Page {
                        kind,
                        sources: chunk.to_vec(),
                    });
                }
            }
        }
        pages.push(Page {
            kind: Kind::Summary,
            sources: vec![],
        });
        let mapping = MigrationMapping {
            source_school_id: inspection.source_catalog.school_id,
            target_school_id: school,
            classes: vec![],
            subjects: vec![],
            principals: vec![],
        };
        Ok(Self {
            owner,
            sequence,
            draft_revision: 1,
            page_generation: 1,
            inspection,
            path,
            pages,
            page: 0,
            mapping,
            stage: Stage::Draft,
        })
    }
    pub fn owner(&self) -> PrincipalId {
        self.owner
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn has_unsaved_changes(&self) -> bool {
        matches!(self.stage, Stage::Draft | Stage::Pending | Stage::Review(_))
    }
    pub fn invalidate(&mut self) {
        self.stage = Stage::Invalid;
    }
    pub fn inspection_lost(&mut self, owner: PrincipalId, sequence: u64) -> Result<(), String> {
        if owner != self.owner || sequence != self.sequence {
            return Err("此来源失效通知不属于当前映射。".into());
        }
        // The worker consumed and retired its physical source capability.
        // Returning to the old draft would falsely imply it can be retried.
        self.invalidate();
        Ok(())
    }
    pub fn set_review(
        &mut self,
        owner: PrincipalId,
        sequence: u64,
        draft_revision: u64,
        summary: ReviewSummary,
    ) -> Result<(), String> {
        if owner != self.owner
            || sequence != self.sequence
            || draft_revision != self.draft_revision
            || !matches!(self.stage, Stage::Pending)
            || summary.current_revision != self.inspection.target_global_revision
            || summary.source_revision != self.inspection.source_revision
        {
            return Err("来源、目标目录或映射复核已变化，请重新开始映射。".into());
        }
        self.bump_page()?;
        self.stage = Stage::Review(summary);
        Ok(())
    }
    fn bump_page(&mut self) -> Result<(), String> {
        match self.page_generation.checked_add(1) {
            Some(next) => {
                self.page_generation = next;
                Ok(())
            }
            None => {
                self.invalidate();
                Err("映射页面序号已用尽，请重新打开。".into())
            }
        }
    }
    fn token(&self, kind: Kind, source: u64, target: Option<u64>) -> String {
        format!(
            "map-{}-{}-{}-{}-{}",
            self.sequence,
            self.page_generation,
            kind.key(),
            source,
            target.map_or_else(|| "none".into(), |id| id.to_string())
        )
    }
    fn mapped(mapping: &MigrationMapping, kind: Kind, source: u64) -> Option<u64> {
        match kind {
            Kind::Class => mapping
                .classes
                .iter()
                .find(|x| x.0.get() == source)
                .map(|x| x.1.get()),
            Kind::Subject => mapping
                .subjects
                .iter()
                .find(|x| x.0.get() == source)
                .map(|x| x.1.get()),
            Kind::Principal => mapping
                .principals
                .iter()
                .find(|x| x.0.get() == source)
                .map(|x| x.1.get()),
            Kind::Summary => None,
        }
    }
    fn targets(&self, kind: Kind) -> Vec<(u64, String)> {
        let enabled = |value| if value { "启用" } else { "停用" };
        match kind {
            Kind::Class => self
                .inspection
                .target_identity
                .classes
                .iter()
                .map(|x| {
                    (
                        x.id.get(),
                        format!(
                            "#{} {}（{}）",
                            x.id.get(),
                            text(&x.name, 1024),
                            enabled(x.enabled)
                        ),
                    )
                })
                .collect(),
            Kind::Subject => self
                .inspection
                .target_identity
                .subjects
                .iter()
                .map(|x| {
                    (
                        x.id.get(),
                        format!(
                            "#{} {}（{}）",
                            x.id.get(),
                            text(&x.name, 1024),
                            enabled(x.enabled)
                        ),
                    )
                })
                .collect(),
            Kind::Principal => self
                .inspection
                .target_identity
                .accounts
                .iter()
                .map(|x| {
                    (
                        x.principal.id.get(),
                        format!(
                            "#{} {} / {}（{}，{}）",
                            x.principal.id.get(),
                            text(&x.login_name, 512),
                            text(&x.display_name, 1024),
                            match x.principal.role {
                                Role::GlobalAdmin => "主管理员",
                                Role::TeacherAdmin => "任课教师",
                                Role::User => "普通用户",
                            },
                            enabled(x.principal.enabled)
                        ),
                    )
                })
                .collect(),
            Kind::Summary => vec![],
        }
    }
    fn source_label(&self, kind: Kind, id: u64) -> String {
        match kind {
            Kind::Class => self
                .inspection
                .source_catalog
                .classes
                .iter()
                .find(|x| x.id.get() == id)
                .map(|x| text(&x.name, 1024)),
            Kind::Subject => self
                .inspection
                .source_catalog
                .subjects
                .iter()
                .find(|x| x.id.get() == id)
                .map(|x| text(&x.name, 1024)),
            Kind::Principal => self
                .inspection
                .source_catalog
                .principals
                .iter()
                .find(|x| x.id.get() == id)
                .map(|x| text(&x.display_name, 1024)),
            Kind::Summary => None,
        }
        .unwrap_or_default()
    }
    fn save_page(&mut self, mut data: FormData) -> Result<(), String> {
        let page = &self.pages[self.page];
        let targets = self.targets(page.kind);
        let mut selected = Vec::new();
        for (row, source) in page.sources.iter().enumerate() {
            let field = FieldId(ROW_FIRST + (row as u16) * 2 + 1);
            let value = data.take(field).ok_or("页面字段缺失，请重新核对当前页。")?;
            let target = if value == self.token(page.kind, *source, None) {
                None
            } else {
                Some(
                    targets
                        .iter()
                        .find(|(id, _)| value == self.token(page.kind, *source, Some(*id)))
                        .map(|x| x.0)
                        .ok_or("页面已变化或目标不属于当前目录，请重新选择。")?,
                )
            };
            selected.push((*source, target));
        }
        if !data.is_empty() {
            return Err("映射页面含有未知字段，请重新核对当前页。".into());
        }
        let mut next = self.mapping.clone();
        for (source, target) in selected {
            match page.kind {
                Kind::Class => {
                    next.classes.retain(|x| x.0.get() != source);
                    if let Some(target) = target {
                        next.classes
                            .push((ClassId::new(source).unwrap(), ClassId::new(target).unwrap()));
                    }
                }
                Kind::Subject => {
                    next.subjects.retain(|x| x.0.get() != source);
                    if let Some(target) = target {
                        next.subjects.push((
                            SubjectId::new(source).unwrap(),
                            SubjectId::new(target).unwrap(),
                        ));
                    }
                }
                Kind::Principal => {
                    next.principals.retain(|x| x.0.get() != source);
                    if let Some(target) = target {
                        next.principals.push((
                            PrincipalId::new(source).unwrap(),
                            PrincipalId::new(target).unwrap(),
                        ));
                    }
                }
                Kind::Summary => {}
            }
        }
        next.classes.sort_by_key(|pair| pair.0);
        next.subjects.sort_by_key(|pair| pair.0);
        next.principals.sort_by_key(|pair| pair.0);
        let unique =
            |ids: Vec<u64>| ids.iter().copied().collect::<BTreeSet<_>>().len() == ids.len();
        if !unique(next.classes.iter().map(|x| x.1.get()).collect())
            || !unique(next.subjects.iter().map(|x| x.1.get()).collect())
            || !unique(next.principals.iter().map(|x| x.1.get()).collect())
        {
            return Err("同类的多个来源不能对应同一个目标；请为每项选择不同目标。".into());
        }
        if next != self.mapping {
            let Some(revision) = self.draft_revision.checked_add(1) else {
                self.invalidate();
                return Err("映射草稿序号已用尽，请重新打开。".into());
            };
            self.mapping = next;
            self.draft_revision = revision;
        }
        self.bump_page()
    }
    fn metadata(&self) -> Vec<Field> {
        let target = self
            .inspection
            .target_identity
            .installation
            .as_ref()
            .expect("validated target school");
        vec![
            readonly(
                SOURCE,
                "来源学校、安装与教学修订",
                format!(
                    "{}；学校 #{}；安装 {}；修订 {}",
                    text(&self.inspection.source_catalog.school_name, 1024),
                    self.inspection.source_catalog.school_id.get(),
                    installation(&self.inspection.source_installation),
                    self.inspection.source_revision.get()
                ),
                1024,
            ),
            readonly(
                TARGET,
                "目标当前学校、安装与全局修订",
                format!(
                    "{}；学校 #{}；安装 {}；修订 {}",
                    text(&target.school_name, 1024),
                    target.school_id.get(),
                    installation(&self.inspection.target_installation),
                    self.inspection.target_global_revision.get()
                ),
                1024,
            ),
            readonly(
                PAGE,
                "映射页及完整覆盖进度",
                format!(
                    "{} / {} · {}；已选班级 {}/{}、科目 {}/{}、账号 {}/{}",
                    self.page + 1,
                    self.pages.len(),
                    self.pages[self.page].kind.label(),
                    self.mapping.classes.len(),
                    self.inspection.source_catalog.classes.len(),
                    self.mapping.subjects.len(),
                    self.inspection.source_catalog.subjects.len(),
                    self.mapping.principals.len(),
                    self.inspection.source_catalog.principals.len()
                ),
                512,
            ),
            readonly(
                PATH,
                "完整来源备份位置（当前页）",
                self.path.pages[self.path.page].clone(),
                1024,
            ),
            readonly(
                PATH_PAGE,
                "来源位置显示页（顺序拼接为完整路径）",
                format!("{} / {}", self.path.page + 1, self.path.pages.len()),
                64,
            ),
        ]
    }
    pub fn view(&self, status: &str, busy: bool) -> View {
        let mut description="这是跨安装显式映射，不会按相同数字编号或相同名称自动对应。必须为来源全部班级、科目和账号分别选择目标；同类目标不能重复，也不能漏项或跳过教学对象。保存映射及翻页只改草稿，不修改正式资料。\n目标学校、安装标识、目录名称和颜色、账号登录名和显示标签、当前密码和恢复码、角色、停用状态、任教权限及系统状态全部保留。来源账号只提供教学引用标签，映射不会授予角色或任教权限。\n确认后全部旧教学数据整体替换，不是按所选目标局部合并：未映射的目标目录本身保留，但其原有教学数据也会被替换。来源当前班级、默认教师和全部教学引用按本次映射恢复；不恢复旧会话、语音文件或语音队列。\n缺少目标时请取消并返回现有学校、科目和账号管理，先创建目录与账号、发放新的初始口令，并通过正常主管理员流程授予所需任教权限，再重新选择来源备份。".to_string();
        let mut fields = self.metadata();
        let action = |id, label: &str, enabled| Action {
            id,
            label: label.into(),
            enabled,
        };
        let mut actions = match &self.stage {
            Stage::Draft => {
                let page = &self.pages[self.page];
                let targets = self.targets(page.kind);
                for (row, source) in page.sources.iter().enumerate() {
                    let selected = Self::mapped(&self.mapping, page.kind, *source);
                    let detail = selected
                        .and_then(|id| targets.iter().find(|x| x.0 == id))
                        .map_or("尚未选择", |x| x.1.as_str());
                    fields.push(readonly(
                        FieldId(ROW_FIRST + (row as u16) * 2),
                        &format!("来源{} #{} 与当前草稿对应", page.kind.label(), source),
                        format!(
                            "{}；目标：{}",
                            self.source_label(page.kind, *source),
                            detail
                        ),
                        2048,
                    ));
                    let mut choices = vec![Choice {
                        value: self.token(page.kind, *source, None),
                        label: "尚未选择（不自动匹配）".into(),
                    }];
                    choices.extend(targets.iter().map(|(id, label)| Choice {
                        value: self.token(page.kind, *source, Some(*id)),
                        label: choice_label(label),
                    }));
                    fields.push(Field {
                        id: FieldId(ROW_FIRST + (row as u16) * 2 + 1),
                        label: format!("为来源{} #{} 明确选择目标", page.kind.label(), source),
                        kind: FieldKind::Choice(choices),
                        value: self.token(page.kind, *source, selected),
                        max_utf16: 128,
                    });
                }
                if page.sources.is_empty() && !matches!(page.kind, Kind::Summary) {
                    description.push_str("\n本类来源没有对象，无需编造对应项；请继续下一页。");
                }
                if matches!(page.kind, Kind::Summary) {
                    description.push_str("\n请回看各页的完整来源标签与当前目标详情。完整映射提交预检仍未授权恢复；预检成功后必须再分别明确确认教学替换和目标安全资料保留。");
                }
                vec![
                    action(PREVIOUS, "保存本页并上一页", !busy && self.page > 0),
                    action(
                        NEXT,
                        "保存本页并下一页",
                        !busy && self.page + 1 < self.pages.len(),
                    ),
                    action(SAVE_DRAFT, "仅保存本页映射草稿", !busy),
                    action(PREFLIGHT, "提交完整映射并预检", !busy),
                    action(MANAGE_TARGETS, "取消映射，先准备目标目录与账号", !busy),
                    action(CANCEL, "取消本次映射", true),
                    action(CLOSE, "丢弃映射并返回工作台", !busy),
                ]
            }
            Stage::Pending => {
                description.push_str("\n正在对完整包和本次映射进行正式预检，尚未授权替换教学数据。可取消或锁定，后台实际结束后才释放来源文件。");
                vec![action(CANCEL, "取消本次映射预检", true)]
            }
            Stage::Review(summary) => {
                fields.push(readonly(
                    FieldId(ROW_FIRST),
                    "完整预检确认的数据",
                    format!(
                        "记录 {}；数据库 {} 字节；资料 {} 份 / {} 字节；图片原件 {} 份 / {} 字节",
                        summary.document_count,
                        summary.database_bytes,
                        summary.material_count,
                        summary.material_bytes,
                        summary.profile_media_count,
                        summary.profile_media_bytes
                    ),
                    512,
                ));
                description.push_str("\n完整映射与全部教学引用预检已通过。确认前可取消，正式资料不变；若需修改映射，请取消后重新选择备份。确认后进入真实后台排空和受控恢复，只能请求安全中止或退出并保留核对事实。程序随后关闭，必须重新启动并用目标当前账号登录。");
                for (id,key,label) in [(ACK_REPLACEMENT,"replace","我已核对全量映射，理解全部旧教学数据整体替换及未映射目录原教学数据也被替换"),(ACK_AUTHORITY,"authority","我理解目标学校、账号标签、当前密码和授权保留，来源包不授予角色或任教权限，恢复后须重启登录")] {
                    fields.push(Field {id,label:if id==ACK_REPLACEMENT {"确认完整教学替换"} else {"确认目标身份和权限边界"}.into(),kind:FieldKind::Choice(vec![Choice {value:"0".into(),label:"尚未确认".into()},Choice {value:self.ack(key),label:label.into()}]),value:"0".into(),max_utf16:128});
                }
                vec![
                    action(CONFIRM, "按明确映射恢复并关闭程序", !busy),
                    action(CANCEL, "取消恢复，保留正式资料", true),
                    action(MANAGE_TARGETS, "取消并返回目标管理", !busy),
                    action(CLOSE, "丢弃本次复核并返回", !busy),
                ]
            }
            Stage::Confirmed => {
                description.push_str(
                    "\n本次明确确认已交给恢复协调器，不能重复提交。请等待真实后台关闭和核对结果。",
                );
                vec![]
            }
            Stage::Invalid => {
                description.push_str("\n本次来源或目标目录复核已失效，不能复活原映射确认；请退出后重新选择来源和当前目录。");
                vec![action(CLOSE, "返回并重新开始", true)]
            }
        };
        if self.path.pages.len() > 1 && matches!(self.stage, Stage::Draft | Stage::Review(_)) {
            description.push_str("\n完整文件位置分多页显示，未省略字符；请按页核对。列表长名称可能简写，请以编号和上方完整选中详情核对。");
            actions.push(action(
                PREVIOUS_PATH,
                "来源位置上一页",
                !busy && self.path.page > 0,
            ));
            actions.push(action(
                NEXT_PATH,
                "来源位置下一页",
                !busy && self.path.page + 1 < self.path.pages.len(),
            ));
        }
        if busy {
            for action in &mut actions {
                action.enabled = action.id == CANCEL;
            }
        }
        // Admission is explicit per action; keeping the native View pump live
        // allows cancellation while ordinary task acceptance is disabled.
        View {
            title: "跨安装备份 · 显式映射恢复".into(),
            description,
            status: text(status, 8192),
            fields,
            actions,
            busy: false,
            grid: None,
        }
    }
    fn ack(&self, key: &str) -> String {
        format!(
            "migration-{key}-{}-{}-{}",
            self.sequence, self.draft_revision, self.page_generation
        )
    }
    pub fn event(&mut self, action: ActionId, mut data: FormData) -> Result<Command, String> {
        if matches!(self.stage, Stage::Pending) && action != CANCEL {
            return Err("完整映射正在预检，当前只可取消。".into());
        }
        if action == CLOSE && !matches!(self.stage, Stage::Confirmed) {
            self.invalidate();
            return Ok(Command::Close);
        }
        if matches!(self.stage, Stage::Confirmed | Stage::Invalid) {
            return Err("此映射页面已失效，不能重复提交。".into());
        }
        if action == CANCEL || action == MANAGE_TARGETS {
            self.invalidate();
            return Ok(if action == CANCEL {
                Command::Cancel
            } else {
                Command::ManageTargets
            });
        }
        if action == CONFIRM {
            if !matches!(self.stage, Stage::Review(_)) {
                return Err("尚无有效的完整映射预检结果。".into());
            }
            let replacement = data.take(ACK_REPLACEMENT);
            let authority = data.take(ACK_AUTHORITY);
            if !data.is_empty()
                || replacement.as_deref() != Some(self.ack("replace").as_str())
                || authority.as_deref() != Some(self.ack("authority").as_str())
            {
                return Err(
                    "请分别明确确认本次完整教学替换和目标身份权限边界；旧页面确认不能沿用。".into(),
                );
            }
            self.stage = Stage::Confirmed;
            return Ok(Command::Confirm {
                sequence: self.sequence,
                draft_revision: self.draft_revision,
            });
        }
        if action == PREVIOUS_PATH || action == NEXT_PATH {
            if matches!(self.stage, Stage::Draft) {
                self.save_page(data)?;
            } else if matches!(self.stage, Stage::Review(_)) {
                data.take(ACK_REPLACEMENT);
                data.take(ACK_AUTHORITY);
                if !data.is_empty() {
                    return Err("来源位置页面包含未知字段。".into());
                }
                self.bump_page()?;
            } else {
                return Err("当前不能翻看来源位置。".into());
            }
            if action == PREVIOUS_PATH {
                self.path.page = self.path.page.saturating_sub(1);
            } else if self.path.page + 1 < self.path.pages.len() {
                self.path.page += 1;
            }
            return Ok(Command::Stay);
        }
        if !matches!(self.stage, Stage::Draft) {
            return Err("完整映射正在预检，当前只可取消。".into());
        }
        match action {
            PREVIOUS | NEXT | SAVE_DRAFT | PREFLIGHT => {
                self.save_page(data)?;
                match action {
                    PREVIOUS => self.page = self.page.saturating_sub(1),
                    NEXT => {
                        if self.page + 1 < self.pages.len() {
                            self.page += 1;
                        }
                    }
                    PREFLIGHT => {
                        self.mapping
                            .validate(
                                &self.inspection.source_catalog,
                                &self.inspection.target_identity,
                            )
                            .map_err(problem)?;
                        // Every attempt has a fresh receipt binding even when
                        // the user retries exactly the same unchanged mapping.
                        let Some(next) = self.draft_revision.checked_add(1) else {
                            self.invalidate();
                            return Err("映射复核序号已用尽，请重新打开。".into());
                        };
                        self.draft_revision = next;
                        self.stage = Stage::Pending;
                        return Ok(Command::Preflight {
                            sequence: self.sequence,
                            draft_revision: self.draft_revision,
                            mapping: self.mapping.clone(),
                        });
                    }
                    _ => {}
                }
                Ok(Command::Stay)
            }
            _ => Err("当前映射页不接纳此操作。".into()),
        }
    }
}
fn problem(error: MigrationError) -> String {
    match error {
        MigrationError::IncompleteMapping=>"仍有来源班级、科目或账号未选择目标；必须完整映射，不允许跳过教学对象。",
        MigrationError::DuplicateMapping=>"同类来源或目标出现重复；每个来源必须一一对应不同的目标。",
        MigrationError::MissingTarget=>"目标目录或账号不存在，请取消并通过现有管理页面准备目标后重新开始。",
        MigrationError::CurrentAuthorization=>"映射后的当前班级或默认任课关系不符合目标现有授权；请先正常管理目标权限，不会从来源包授予权限。",
        MigrationError::WrongSchool=>"来源或目标学校身份已变化，请重新选择并映射。",
        MigrationError::Capacity=>"目录数量超过完整映射上限，不能截断或跳过对象。",
        MigrationError::InvalidCatalog=>"来源目录不完整或无效，不能开始映射。",
    }.into()
}
