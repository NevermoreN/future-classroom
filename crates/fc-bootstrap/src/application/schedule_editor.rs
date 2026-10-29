//! Owned, non-secret timetable draft. Native controls only edit this draft;
//! the explicit final Save transfers a complete candidate to the shared worker.
use super::*;
use fc_domain::{schedule::*, schedule_document::*, time::*};

pub(super) const TERM: FieldId = FieldId(100);
pub(super) const TERM_NAME: FieldId = FieldId(101);
pub(super) const START: FieldId = FieldId(102);
pub(super) const END: FieldId = FieldId(103);
pub(super) const ANCHOR: FieldId = FieldId(104);
pub(super) const TABLE: FieldId = FieldId(105);
pub(super) const RANGE: FieldId = FieldId(106);
pub(super) const GROUP: FieldId = FieldId(107);
pub(super) const KIND: FieldId = FieldId(108);
pub(super) const TEACHER: FieldId = FieldId(109);
pub(super) const MODE: FieldId = FieldId(110);
pub(super) const MAP_DATE: FieldId = FieldId(111);
pub(super) const MAP_SOURCE: FieldId = FieldId(112);
pub(super) const MAP_KIND: FieldId = FieldId(113);
pub(super) const COPY_FROM: FieldId = FieldId(114);
pub(super) const COPY_TO: FieldId = FieldId(115);
pub(super) const TIMING_SYNC: FieldId = FieldId(116);
pub(super) const ROOM: FieldId = FieldId(117);
pub(super) const NOTE: FieldId = FieldId(118);
pub(super) const ENABLED: FieldId = FieldId(119);
pub(super) const AUTO_GAPS: FieldId = FieldId(120);
pub(super) const ROW_NAME: FieldId = FieldId(121);
pub(super) const DAY_BASE: u16 = 130;
pub(super) const OPEN: ActionId = ActionId(100);
pub(super) const NEW_TERM: ActionId = ActionId(101);
pub(super) const HEADER: ActionId = ActionId(102);
pub(super) const ADD_ROW: ActionId = ActionId(103);
pub(super) const APPLY: ActionId = ActionId(104);
pub(super) const EDIT_BACK: ActionId = ActionId(105);
pub(super) const SAVE_ALL: ActionId = ActionId(106);
pub(super) const CANCEL: ActionId = ActionId(107);
pub(super) const CONFIRM_CANCEL: ActionId = ActionId(108);
pub(super) const ENABLE_DUAL: ActionId = ActionId(109);
pub(super) const RESTORE_DUAL: ActionId = ActionId(110);
pub(super) const DISABLE_DUAL: ActionId = ActionId(111);
pub(super) const SYNC: ActionId = ActionId(112);
pub(super) const REMOVE: ActionId = ActionId(113);
pub(super) const MOVE_UP: ActionId = ActionId(114);
pub(super) const MOVE_DOWN: ActionId = ActionId(115);
pub(super) const COPY: ActionId = ActionId(116);
pub(super) const CALENDAR: ActionId = ActionId(117);
pub(super) const SET_MAPPING: ActionId = ActionId(118);
pub(super) const REMOVE_MAPPING: ActionId = ActionId(119);
pub(super) const PREVIEW: ActionId = ActionId(120);
pub(super) const DRAFT_TEXT: ActionId = ActionId(121);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Terms,
    Header,
    Grid,
    Row(Option<usize>),
    Cell(usize, usize),
    Calendar,
    Preview,
    Discard,
}
pub(super) enum EditorCommand {
    Stay,
    Close,
    Reload,
    Save {
        expected: Revision,
        document: ScheduleDocument,
    },
}
pub(super) struct Editor {
    pub(super) document: ScheduleDocument,
    owner: PrincipalId,
    expected: Revision,
    page: Page,
    term: Option<usize>,
    table: usize,
    suspended: Option<FormData>,
    dirty: bool,
    cell_subject: Option<SubjectId>,
}
fn date_text(d: LocalDate) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}
fn parse_date(s: &str) -> std::result::Result<LocalDate, String> {
    let bytes = s.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return Err("日期请输入YYYY-MM-DD，例如2026-09-01。".into());
    }
    LocalDate::new(
        s[..4].parse().unwrap_or(0),
        s[5..7].parse().unwrap_or(0),
        s[8..].parse().unwrap_or(0),
    )
    .map_err(|_| "日期不存在。".into())
}
fn field_value(data: &FormData, id: FieldId) -> &str {
    data.get(id).unwrap_or("")
}
fn index_value(data: &FormData, id: FieldId, len: usize) -> std::result::Result<usize, String> {
    field_value(data, id)
        .parse::<usize>()
        .ok()
        .filter(|i| *i < len)
        .ok_or_else(|| "请选择有效项目。".into())
}
fn yes(data: &FormData, id: FieldId) -> bool {
    field_value(data, id) == "1"
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
fn plain(id: FieldId, label: &str, value: String) -> Field {
    field(id, label, FieldKind::Text, value, 128)
}
fn choose(id: FieldId, label: &str, value: String, mut options: Vec<(String, String)>) -> Field {
    if !value.is_empty() && !options.iter().any(|(v, _)| v == &value) {
        options.push((value.clone(), "原选择已失效，请重新选择".into()));
    }
    field(
        id,
        label,
        FieldKind::Choice(
            options
                .into_iter()
                .map(|(value, label)| Choice { value, label })
                .collect(),
        ),
        value,
        128,
    )
}
fn toggle(id: FieldId, label: &str, value: bool) -> Field {
    choose(
        id,
        label,
        if value { "1" } else { "0" }.into(),
        vec![("1".into(), "启用".into()), ("0".into(), "关闭".into())],
    )
}
fn action(id: ActionId, label: &str) -> Action {
    Action {
        id,
        label: label.into(),
        enabled: true,
    }
}
fn role_label(role: TableRole) -> &'static str {
    match role {
        TableRole::Single => "统一表",
        TableRole::Odd => "单周表",
        TableRole::Even => "双周表",
    }
}
pub(super) fn problem(e: ScheduleDocumentError) -> String {
    let l = e.location;
    let where_ = format!(
        "{}{}{}{}",
        l.semester_id
            .map(|n| format!("学期{} ", n.get()))
            .unwrap_or_default(),
        l.table_id
            .map(|n| format!("表{} ", n.get()))
            .unwrap_or_default(),
        l.row_id
            .map(|n| format!("行{} ", n.get()))
            .unwrap_or_default(),
        l.day
            .map(|d| format!("星期{} ", d.index() + 1))
            .unwrap_or_default()
    );
    let text = match e.kind {
        DocumentErrorKind::RevisionConflict { .. } => {
            "保存版本已变化，草稿仍保留。请取消并重新加载后核对。"
        }
        DocumentErrorKind::Schedule(ScheduleError::Overlap { .. }) => {
            "启用日期的作息时间重叠，请修改对应行。"
        }
        DocumentErrorKind::Schedule(ScheduleError::AnchorNotMonday) => "第一教学周锚点必须是周一。",
        DocumentErrorKind::InvalidReference => "班级、科目或老师引用已失效，请重新选择。",
        DocumentErrorKind::Capacity => "课表达到容量上限，请减少行、学期或备注。",
        DocumentErrorKind::InvalidCalendar | DocumentErrorKind::MappingCycle => {
            "校历覆盖或补课来源无效，请核对学期、日期和来源。"
        }
        DocumentErrorKind::Schedule(ScheduleError::InvalidTerm | ScheduleError::AmbiguousTerm) => {
            "学期日期或启用学期范围冲突。"
        }
        DocumentErrorKind::InvalidHistory => "双表历史不能恢复，请明确选择重新生成。",
        _ => "草稿尚不完整或结构不一致，请检查学期、作息和课程格。",
    };
    format!("{where_}{text}")
}
impl Editor {
    pub(super) fn new(document: ScheduleDocument, owner: PrincipalId) -> Self {
        Self {
            expected: document.revision,
            document,
            owner,
            page: Page::Terms,
            term: None,
            table: 0,
            suspended: None,
            dirty: false,
            cell_subject: None,
        }
    }
    pub(super) fn owner(&self) -> PrincipalId {
        self.owner
    }
    pub(super) fn saved(&mut self, document: ScheduleDocument) {
        self.expected = document.revision;
        self.document = document;
        self.dirty = false;
        self.suspended = None;
    }
    pub(super) fn capture(&mut self, data: FormData) {
        self.suspended = Some(data);
    }
    fn plan(&self) -> std::result::Result<&SemesterPlan, String> {
        self.term
            .and_then(|i| self.document.semesters.get(i))
            .ok_or_else(|| "请先选择学期。".into())
    }
    fn table(&self) -> std::result::Result<&WeekTable, String> {
        self.plan()?
            .semester
            .tables
            .get(self.table)
            .ok_or_else(|| "请重新选择周表。".into())
    }
    fn next_id(&self) -> std::result::Result<u64, String> {
        let mut max = self.document.id_high_water;
        for p in &self.document.semesters {
            max = max.max(p.semester.id.get());
            for t in p
                .semester
                .tables
                .iter()
                .chain(p.dual_history.iter().flat_map(|h| h.tables.iter()))
            {
                max = max.max(t.id.get());
                for r in &t.rows {
                    max = max.max(r.id.get()).max(r.logical_key.get());
                }
            }
        }
        max.checked_add(1)
            .ok_or_else(|| "课表标识已达上限。".into())
    }
    fn replace_plan(&mut self, p: SemesterPlan) {
        self.document.semesters[self.term.unwrap()] = p;
        if let Ok(next) = self.next_id() {
            self.document.id_high_water = self.document.id_high_water.max(next - 1);
        }
        self.dirty = true;
    }
    pub(super) fn select_cell(
        &mut self,
        row: usize,
        column: usize,
        _identity: &IdentityState,
    ) -> std::result::Result<(), String> {
        let r = self.table()?.rows.get(row).ok_or("请选择有效行。")?;
        if column > 7 {
            return Err("请选择有效星期。".into());
        }
        let subject = if column > 0 {
            r.courses[column - 1].as_ref().map(|c| c.subject_id)
        } else {
            None
        };
        let page = if column == 0 || r.kind != RowKind::Course {
            Page::Row(Some(row))
        } else {
            Page::Cell(row, column - 1)
        };
        self.page = page;
        self.cell_subject = subject;
        self.suspended = None;
        Ok(())
    }
    pub(super) fn input(&mut self, mut data: FormData, identity: &IdentityState) -> bool {
        if let Page::Cell(_, _) = self.page {
            let subject = data
                .get(SUBJECT)
                .and_then(|s| s.parse::<u64>().ok())
                .and_then(|n| SubjectId::new(n).ok());
            if subject != self.cell_subject {
                self.cell_subject = subject;
                let candidates = self
                    .plan()
                    .ok()
                    .map(|p| {
                        identity
                            .assignments
                            .iter()
                            .filter(|a| {
                                a.enabled
                                    && a.class_id == p.semester.class_id
                                    && Some(a.subject_id) == subject
                                    && identity.account(a.teacher_id).is_ok_and(|a| {
                                        a.principal.enabled && a.principal.role != Role::User
                                    })
                            })
                            .map(|a| a.teacher_id)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let value = if candidates.len() == 1 {
                    candidates[0].get().to_string()
                } else {
                    "unassigned".into()
                };
                data.take(TEACHER);
                let _ = data.insert(TEACHER, value);
                self.suspended = Some(data);
                return true;
            }
        }
        false
    }
    pub(super) fn view(&self, identity: &IdentityState, status: &str, busy: bool) -> View {
        let mut v = View {
            title: "课表、作息与校历".into(),
            description: format!(
                "{}保存修订{}。所有修改先留在草稿，点击“保存全部”才写入。",
                if self.dirty {
                    "有未保存修改。"
                } else {
                    ""
                },
                self.expected.get()
            ),
            status: status.into(),
            fields: vec![],
            actions: vec![],
            busy,
            grid: None,
        };
        let plan = self.plan().ok();
        let table = self.table().ok();
        match self.page {
            Page::Terms => {
                let options = self
                    .document
                    .semesters
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        (
                            i.to_string(),
                            format!(
                                "{} · {}",
                                p.name,
                                identity
                                    .classes
                                    .iter()
                                    .find(|c| c.id == p.semester.class_id)
                                    .map(|c| c.name.as_str())
                                    .unwrap_or("班级失效")
                            ),
                        )
                    })
                    .collect::<Vec<_>>();
                if !options.is_empty() {
                    v.fields.push(choose(
                        TERM,
                        "选择学期",
                        self.term.unwrap_or(0).to_string(),
                        options,
                    ));
                    v.actions.push(action(OPEN, "编辑此学期"));
                }
                v.actions.extend([
                    action(NEW_TERM, "新建学期"),
                    action(CALENDAR, "校历模式与日期映射"),
                    action(SAVE_ALL, "保存全部"),
                    action(EDIT_BACK, "保留草稿并返回"),
                    action(CANCEL, "取消全部修改"),
                ]);
            }
            Page::Header => {
                if let Some(p) = plan {
                    let s = &p.semester;
                    v.title = format!("学期设置 · {}", p.name);
                    v.fields = vec![
                        plain(TERM_NAME, "学期名称", p.name.clone()),
                        choose(
                            CLASS,
                            "班级",
                            s.class_id.get().to_string(),
                            identity
                                .classes
                                .iter()
                                .filter(|c| c.enabled)
                                .map(|c| (c.id.get().to_string(), c.name.clone()))
                                .collect(),
                        ),
                        plain(START, "开始日期", date_text(s.start)),
                        plain(END, "结束日期", date_text(s.end)),
                        plain(
                            ANCHOR,
                            "第一教学周周一（请确认）",
                            date_text(s.first_week_monday),
                        ),
                        toggle(ENABLED, "启用此学期", s.enabled),
                        toggle(TIMING_SYNC, "双表作息结构同步（不复制课程）", p.timing_sync),
                    ];
                    v.actions = vec![action(APPLY, "应用到草稿"), action(EDIT_BACK, "返回")];
                }
            }
            Page::Grid => {
                if let (Some(p), Some(t)) = (plan, table) {
                    v.title = format!("{} · {}", p.name, role_label(t.role));
                    v.description.push_str("\n选择时间列编辑作息，选择星期格编辑课程。活动行跨星期显示。课程与老师只可从已有项目选择。");
                    v.fields.push(choose(
                        TABLE,
                        "当前周表",
                        self.table.to_string(),
                        p.semester
                            .tables
                            .iter()
                            .enumerate()
                            .map(|(i, t)| (i.to_string(), role_label(t.role).into()))
                            .collect(),
                    ));
                    v.fields
                        .push(choose(COPY_FROM, "复制来源星期", "0".into(), days()));
                    v.fields
                        .push(choose(COPY_TO, "复制目标星期", "1".into(), days()));
                    let rows = t
                        .rows
                        .iter()
                        .map(|r| {
                            let name = p
                                .row_details
                                .iter()
                                .find(|m| m.row_id == r.id)
                                .map(|m| m.name.as_str())
                                .unwrap_or("");
                            let group = match r.group {
                                DayGroup::Morning => "上午",
                                DayGroup::Afternoon => "下午",
                                DayGroup::Other(_) => "其他",
                            };
                            let label = format!(
                                "{name} {:02}：{:02}-{:02}：{:02}",
                                r.range.begin.get() / 60,
                                r.range.begin.get() % 60,
                                r.range.end.get() / 60,
                                r.range.end.get() % 60
                            );
                            let mut cells = (0..7)
                                .map(|i| {
                                    if !r.enabled_days[i] {
                                        "不适用".into()
                                    } else {
                                        match r.kind {
                                            RowKind::Course => r.courses[i]
                                                .as_ref()
                                                .map(|c| {
                                                    identity
                                                        .subjects
                                                        .iter()
                                                        .find(|s| s.id == c.subject_id)
                                                        .map(|s| s.name.clone())
                                                        .unwrap_or("科目失效".into())
                                                })
                                                .unwrap_or("无课".into()),
                                            RowKind::Activity => {
                                                if name.is_empty() {
                                                    "活动".into()
                                                } else {
                                                    name.into()
                                                }
                                            }
                                            RowKind::Rest(RestPurpose::Lunch) => "午休".into(),
                                            RowKind::Rest(RestPurpose::Recess) => "课间".into(),
                                            RowKind::Rest(_) => "休息".into(),
                                        }
                                    }
                                })
                                .collect::<Vec<String>>();
                            let merged = r.kind != RowKind::Course;
                            if merged {
                                cells = vec![cells
                                    .iter()
                                    .find(|s| s.as_str() != "不适用")
                                    .cloned()
                                    .unwrap_or("不适用".into())];
                            }
                            GridRow {
                                group: group.into(),
                                label,
                                cells,
                                merged,
                            }
                        })
                        .collect();
                    v.grid = Some(Grid {
                        columns: std::iter::once("时段 / 时间".into())
                            .chain(days().into_iter().map(|(_, label)| label))
                            .collect(),
                        rows,
                        selected: None,
                        enabled: true,
                    });
                    v.actions = vec![
                        action(OPEN, "切换周表"),
                        action(HEADER, "学期设置"),
                        action(ADD_ROW, "添加作息行"),
                        action(COPY, "复制整列星期"),
                        action(SYNC, "同步当前表作息"),
                        action(SAVE_ALL, "保存全部"),
                        action(PREVIEW, "预览今日状态"),
                        action(EDIT_BACK, "返回学期列表"),
                    ];
                    if p.semester.tables.len() == 1 {
                        v.actions.push(action(ENABLE_DUAL, "重新生成单双表"));
                        if p.dual_history.is_some() {
                            v.actions.push(action(RESTORE_DUAL, "恢复保留的双表"));
                        }
                    } else {
                        v.actions.push(action(DISABLE_DUAL, "关闭双周：保留当前表"));
                    }
                }
            }
            Page::Row(selected) => {
                if let Some(t) = table {
                    let r = selected.and_then(|i| t.rows.get(i));
                    v.title = if selected.is_some() {
                        "编辑作息行"
                    } else {
                        "添加作息行"
                    }
                    .into();
                    v.description.push_str("\n时间只填8个数字，例如08200900；每个星期的启用行不得重叠。行名称不会决定课间/午休类型。");
                    let name = plan
                        .and_then(|p| {
                            r.and_then(|r| p.row_details.iter().find(|n| n.row_id == r.id))
                        })
                        .map(|n| n.name.clone())
                        .unwrap_or_default();
                    let range = r
                        .map(|r| {
                            format!(
                                "{:02}{:02}{:02}{:02}",
                                r.range.begin.get() / 60,
                                r.range.begin.get() % 60,
                                r.range.end.get() / 60,
                                r.range.end.get() % 60
                            )
                        })
                        .unwrap_or("08200900".into());
                    v.fields = vec![
                        field(ROW_NAME, "行名称", FieldKind::Text, name, 64),
                        field(RANGE, "起止时间", FieldKind::TimeRange, range, 16),
                        choose(
                            GROUP,
                            "分组",
                            r.map(|r| match r.group {
                                DayGroup::Morning => "morning",
                                DayGroup::Afternoon => "afternoon",
                                DayGroup::Other(_) => "other",
                            })
                            .unwrap_or("morning")
                            .into(),
                            vec![
                                ("morning".into(), "上午".into()),
                                ("afternoon".into(), "下午".into()),
                                ("other".into(), "其他".into()),
                            ],
                        ),
                        choose(
                            KIND,
                            "类型 / 休息用途",
                            r.map(|r| match r.kind {
                                RowKind::Course => "course",
                                RowKind::Activity => "activity",
                                RowKind::Rest(RestPurpose::Recess) => "recess",
                                RowKind::Rest(RestPurpose::Lunch) => "lunch",
                                RowKind::Rest(RestPurpose::Other) => "rest",
                                RowKind::Rest(RestPurpose::Unknown) => "unknown",
                            })
                            .unwrap_or("course")
                            .into(),
                            vec![
                                ("course".into(), "课程".into()),
                                ("activity".into(), "活动（跨星期）".into()),
                                ("recess".into(), "课间".into()),
                                ("lunch".into(), "午休".into()),
                                ("rest".into(), "其他休息".into()),
                                ("unknown".into(), "未确认休息用途".into()),
                            ],
                        ),
                    ];
                    for (i, (_, name)) in days().into_iter().enumerate() {
                        v.fields.push(toggle(
                            FieldId(DAY_BASE + i as u16),
                            &name,
                            r.map(|r| r.enabled_days[i]).unwrap_or(i < 5),
                        ));
                    }
                    v.actions = vec![
                        action(APPLY, "应用到草稿"),
                        action(EDIT_BACK, "取消本行编辑"),
                    ];
                    if selected.is_some() {
                        v.actions.extend([
                            action(REMOVE, "删除此行"),
                            action(MOVE_UP, "向上移动"),
                            action(MOVE_DOWN, "向下移动"),
                        ]);
                    }
                }
            }
            Page::Cell(row, day) => {
                if let (Some(p), Some(t)) = (plan, table) {
                    if let Some(r) = t.rows.get(row) {
                        v.title = format!("课程格 · 星期{}", day + 1);
                        let c = r.courses[day].as_ref();
                        let mut subjects = vec![("none".into(), "无课（清空格子）".into())];
                        subjects.extend(
                            identity
                                .subjects
                                .iter()
                                .filter(|s| s.enabled)
                                .map(|s| (s.id.get().to_string(), s.name.clone())),
                        );
                        let mut teachers =
                            vec![("unassigned".into(), "未分配老师（使用默认布局）".into())];
                        if self.cell_subject.is_some_and(|subject| {
                            identity
                                .resolve_teacher(p.semester.class_id, subject, None)
                                .is_ok()
                        }) {
                            teachers.push(("default".into(), "使用此班此科目的默认老师".into()));
                        }
                        teachers.extend(
                            identity
                                .accounts
                                .iter()
                                .filter(|a| {
                                    a.principal.enabled
                                        && a.principal.role != Role::User
                                        && self.cell_subject.is_some_and(|subject| {
                                            identity.is_teacher(
                                                a.principal.id,
                                                p.semester.class_id,
                                                subject,
                                            )
                                        })
                                })
                                .map(|a| {
                                    (
                                        a.principal.id.get().to_string(),
                                        format!("{}（{}）", a.display_name, a.login_name),
                                    )
                                }),
                        );
                        let details = p
                            .cell_details
                            .iter()
                            .find(|m| m.row_id == r.id && m.day.index() == day);
                        v.fields = vec![
                            choose(
                                SUBJECT,
                                "科目",
                                c.map(|c| c.subject_id.get().to_string())
                                    .unwrap_or("none".into()),
                                subjects,
                            ),
                            choose(
                                TEACHER,
                                "任课老师",
                                c.map(|c| match c.teacher {
                                    TeacherBinding::Default => "default".into(),
                                    TeacherBinding::Unassigned => "unassigned".into(),
                                    TeacherBinding::Explicit(n) => n.get().to_string(),
                                })
                                .unwrap_or("unassigned".into()),
                                teachers,
                            ),
                            field(
                                ROOM,
                                "教室（可选）",
                                FieldKind::Text,
                                details.map(|d| d.room.clone()).unwrap_or_default(),
                                64,
                            ),
                            field(
                                NOTE,
                                "备注（可选）",
                                FieldKind::Multiline,
                                details.map(|d| d.note.clone()).unwrap_or_default(),
                                512,
                            ),
                        ];
                        v.actions = vec![
                            action(APPLY, "应用到草稿"),
                            action(EDIT_BACK, "取消本格编辑"),
                        ];
                    }
                }
            }
            Page::Calendar => {
                let cal = if self.document.calendar_mode == CalendarMode::School {
                    &self.document.school_calendar
                } else {
                    &self.document.public_calendar
                };
                v.title = "校历模式与日期映射".into();
                v.description.push_str("\n学校/公共资料相互隔离；切换模式不会复制资料。学校覆盖缺失时显示待确认，不自动回退。公共资料未经来源核验时仍标未核验。补课选择具体来源日期，不递归应用来源的覆盖。");
                let sample = cal
                    .entries
                    .iter()
                    .take(8)
                    .map(|e| {
                        format!(
                            "{}：{}",
                            date_text(e.date),
                            match e.mapping {
                                DayMapping::Rest => "休息".into(),
                                DayMapping::Unconfirmed => "待确认".into(),
                                DayMapping::SourceDate(d) => format!("按{}", date_text(d)),
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("；");
                v.description.push_str(&format!(
                    "\n当前模式已有{}条：{}",
                    cal.entries.len(),
                    sample
                ));
                let current = self.term.unwrap_or(0).to_string();
                v.fields = vec![
                    choose(
                        MODE,
                        "当前模式",
                        if self.document.calendar_mode == CalendarMode::School {
                            "school"
                        } else {
                            "public"
                        }
                        .into(),
                        vec![
                            ("public".into(), "公共模式".into()),
                            ("school".into(), "学校模式".into()),
                        ],
                    ),
                    plain(
                        START,
                        "学校覆盖开始（可留空）",
                        self.document
                            .school_calendar
                            .school_coverage
                            .map(|(s, _)| date_text(s))
                            .unwrap_or_default(),
                    ),
                    plain(
                        END,
                        "学校覆盖结束（可留空）",
                        self.document
                            .school_calendar
                            .school_coverage
                            .map(|(_, e)| date_text(e))
                            .unwrap_or_default(),
                    ),
                    toggle(
                        AUTO_GAPS,
                        "自动计算同分组课程间空档",
                        self.document.automatic_gaps,
                    ),
                ];
                if !self.document.semesters.is_empty() {
                    v.fields.extend([
                        choose(
                            TERM,
                            "特殊日实际所属学期",
                            current,
                            self.document
                                .semesters
                                .iter()
                                .enumerate()
                                .map(|(i, p)| (i.to_string(), p.name.clone()))
                                .collect(),
                        ),
                        plain(MAP_DATE, "实际日期", String::new()),
                        choose(
                            MAP_KIND,
                            "特殊日安排",
                            "rest".into(),
                            vec![
                                ("rest".into(), "休息".into()),
                                ("unconfirmed".into(), "待确认".into()),
                                ("source".into(), "按具体日期补课".into()),
                            ],
                        ),
                        plain(MAP_SOURCE, "补课来源日期（补课时必填）", String::new()),
                    ]);
                }
                v.actions = vec![
                    action(APPLY, "应用模式与覆盖到草稿"),
                    action(EDIT_BACK, "返回学期列表"),
                ];
                if !self.document.semesters.is_empty() {
                    v.actions.extend([
                        action(SET_MAPPING, "添加或更新特殊日"),
                        action(REMOVE_MAPPING, "删除此特殊日"),
                    ]);
                }
            }
            Page::Preview => {
                v.title = "未保存课表预览".into();
                let text = plan
                    .map(|p| {
                        // This clone is used only for human-readable draft preview;
                        // no lesson key or permission escapes before publication.
                        let mut draft=self.document.clone();
                        draft.revision=Revision::default();
                        CurrentClock::new()
                            .try_now()
                            .ok()
                            .map(|now| {
                                match draft.resolve_day(p.semester.class_id, now.date) {
                                    Ok(day) => format!("{}\n{}{}", date_text(now.date), lesson_text(resolve_lesson(&day, now, self.document.automatic_gaps),identity), if matches!(&day,AcademicDay::Scheduled(d) if d.public_data_unverified) {"\n公共调休资料未核验；普通课表仍按已保存作息解析。"} else {""}),
                                    Err(_) => "课表不完整，请先修正草稿。".into(),
                                }
                            })
                            .unwrap_or("系统时间不可用".into())
                    })
                    .unwrap_or("请先选择学期".into());
                v.description.push_str(&format!("\n{text}"));
                v.actions = vec![action(EDIT_BACK, "返回周表")];
            }
            Page::Discard => {
                v.title = "取消课表修改".into();
                v.description =
                    "确认取消后会丢弃本地全部课表草稿，并重新读取已保存版本；这不会修改数据库。"
                        .into();
                v.actions = vec![
                    action(CONFIRM_CANCEL, "确认丢弃并重新加载"),
                    action(DRAFT_TEXT, "保留草稿"),
                ];
            }
        }
        v.actions.push(action(LOCK, "锁定并保留草稿"));
        if let Some(data) = &self.suspended {
            for f in &mut v.fields {
                if let Some(s) = data.get(f.id) {
                    f.value = s.to_owned();
                    if let FieldKind::Choice(choices) = &mut f.kind {
                        if !s.is_empty() && !choices.iter().any(|c| c.value == s) {
                            choices.push(Choice {
                                value: s.into(),
                                label: "原选择已失效，请重新选择".into(),
                            });
                        }
                    }
                }
            }
        }
        v
    }
    pub(super) fn handle(
        &mut self,
        action: ActionId,
        data: FormData,
        identity: &IdentityState,
    ) -> std::result::Result<EditorCommand, String> {
        // Keep invalid/incomplete native values after a validation failure or lock.
        let result = self.apply(action, &data, identity);
        if result.is_err() {
            self.suspended = Some(data);
        } else {
            self.suspended = None;
        }
        result
    }
    fn apply(
        &mut self,
        action: ActionId,
        data: &FormData,
        identity: &IdentityState,
    ) -> std::result::Result<EditorCommand, String> {
        match action {
            SAVE_ALL => {
                self.document.validate_draft(identity).map_err(problem)?;
                return Ok(EditorCommand::Save {
                    expected: self.expected,
                    document: self.document.clone(),
                });
            }
            CANCEL => self.page = Page::Discard,
            CONFIRM_CANCEL => return Ok(EditorCommand::Reload),
            DRAFT_TEXT => self.page = Page::Terms,
            OPEN => {
                if self.page == Page::Terms {
                    self.term = Some(index_value(data, TERM, self.document.semesters.len())?);
                    self.table = 0;
                    self.page = Page::Grid;
                } else {
                    self.table = index_value(data, TABLE, self.plan()?.semester.tables.len())?;
                }
            }
            NEW_TERM => {
                if self.document.semesters.len() >= MAX_SEMESTERS {
                    return Err("学期数量已达上限。".into());
                }
                let class = identity
                    .installation
                    .as_ref()
                    .and_then(|i| i.current_class)
                    .or_else(|| identity.classes.iter().find(|c| c.enabled).map(|c| c.id))
                    .ok_or("请先创建班级。")?;
                let today = CurrentClock::new()
                    .try_now()
                    .map_err(|_| "系统时间不可用。")?
                    .date;
                let mut monday = today;
                for _ in 0..6 {
                    if monday.weekday() == Weekday::Monday {
                        break;
                    }
                    monday = previous_day(monday)?;
                }
                let seed = self.next_id()?;
                self.document.semesters.push(SemesterPlan {
                    semester: Semester {
                        id: SemesterId::new(seed).unwrap(),
                        class_id: class,
                        revision: Revision::default(),
                        enabled: true,
                        start: today,
                        end: today,
                        first_week_monday: monday,
                        tables: vec![WeekTable {
                            id: TableId::new(seed.checked_add(1).ok_or("标识已达上限")?).unwrap(),
                            role: TableRole::Single,
                            rows: vec![],
                        }],
                    },
                    name: "新学期".into(),
                    timing_sync: true,
                    dual_history: None,
                    row_details: vec![],
                    cell_details: vec![],
                });
                self.term = Some(self.document.semesters.len() - 1);
                self.table = 0;
                self.document.update_id_high_water();
                self.page = Page::Header;
                self.dirty = true;
            }
            HEADER => self.page = Page::Header,
            ADD_ROW => self.page = Page::Row(None),
            CALENDAR => self.page = Page::Calendar,
            PREVIEW => self.page = Page::Preview,
            EDIT_BACK => {
                self.page = match self.page {
                    Page::Terms => return Ok(EditorCommand::Close),
                    Page::Header | Page::Row(_) | Page::Cell(_, _) | Page::Preview => Page::Grid,
                    _ => Page::Terms,
                }
            }
            APPLY => match self.page {
                Page::Header => {
                    let mut p = self.plan()?.clone();
                    p.name = field_value(data, TERM_NAME).trim().into();
                    p.semester.class_id =
                        ClassId::new(field_value(data, CLASS).parse().map_err(|_| "请选择班级")?)
                            .map_err(|_| "请选择班级")?;
                    p.semester.start = parse_date(field_value(data, START))?;
                    p.semester.end = parse_date(field_value(data, END))?;
                    p.semester.first_week_monday = parse_date(field_value(data, ANCHOR))?;
                    p.semester.enabled = yes(data, ENABLED);
                    p.timing_sync = yes(data, TIMING_SYNC);
                    p.semester
                        .validate()
                        .map_err(|_| "学期起止、周一锚点或已有作息无效。")?;
                    self.replace_plan(p);
                    self.page = Page::Grid;
                }
                Page::Row(index) => {
                    let mut input = TimeRangeInput::default();
                    input
                        .paste(field_value(data, RANGE))
                        .map_err(|_| "请输入完整的8位起止时间。")?;
                    let range = input
                        .finish()
                        .map_err(|_| "起止时间须为00:00至23:59的有效分钟，且开始早于结束。")?;
                    let mut row = if let Some(index) = index {
                        self.table()?.rows.get(index).ok_or("作息行不存在")?.clone()
                    } else {
                        let id = self.next_id()?;
                        TimetableRow {
                            id: RowId::new(id).unwrap(),
                            logical_key: LogicalRowKey::new(id).unwrap(),
                            group: DayGroup::Morning,
                            kind: RowKind::Course,
                            range,
                            enabled_days: [false; 7],
                            courses: std::array::from_fn(|_| None),
                        }
                    };
                    row.range = range;
                    row.group = match field_value(data, GROUP) {
                        "morning" => DayGroup::Morning,
                        "afternoon" => DayGroup::Afternoon,
                        _ => match row.group {
                            DayGroup::Other(id) => DayGroup::Other(id),
                            _ => DayGroup::Other(1),
                        },
                    };
                    row.kind = match field_value(data, KIND) {
                        "course" => RowKind::Course,
                        "activity" => RowKind::Activity,
                        "recess" => RowKind::Rest(RestPurpose::Recess),
                        "lunch" => RowKind::Rest(RestPurpose::Lunch),
                        "rest" => RowKind::Rest(RestPurpose::Other),
                        "unknown" => RowKind::Rest(RestPurpose::Unknown),
                        _ => return Err("请选择作息类型".into()),
                    };
                    for i in 0..7 {
                        row.enabled_days[i] = yes(data, FieldId(DAY_BASE + i as u16));
                    }
                    if row.kind != RowKind::Course {
                        row.courses = std::array::from_fn(|_| None);
                    }
                    let id = row.id;
                    let s = self.plan()?.semester.id;
                    let t = self.table()?.id;
                    let edit = if index.is_some() {
                        DraftEdit::SetRow {
                            semester_id: s,
                            table_id: t,
                            row,
                        }
                    } else {
                        DraftEdit::InsertRow {
                            semester_id: s,
                            table_id: t,
                            index: self.table()?.rows.len(),
                            row,
                        }
                    };
                    let mut next = self.document.edit(&edit, identity).map_err(problem)?;
                    let p = &mut next.semesters[self.term.unwrap()];
                    p.row_details.retain(|d| d.row_id != id);
                    let name = field_value(data, ROW_NAME).trim();
                    if !name.is_empty() {
                        p.row_details.push(RowDetails {
                            row_id: id,
                            name: name.into(),
                        });
                    }
                    next.validate_draft_metadata().map_err(problem)?;
                    self.document = next;
                    self.dirty = true;
                    self.page = Page::Grid;
                }
                Page::Cell(row, day) => {
                    let p = self.plan()?;
                    let t = self.table()?;
                    let row_id = t.rows.get(row).ok_or("作息行不存在")?.id;
                    let mut next = self.document.clone();
                    let value = field_value(data, SUBJECT);
                    let has_course = value != "none";
                    let cell = if !has_course {
                        None
                    } else {
                        let subject = SubjectId::new(value.parse().map_err(|_| "请选择已有科目")?)
                            .map_err(|_| "请选择已有科目")?;
                        let raw = field_value(data, TEACHER);
                        let teacher = match raw {
                            "default" => TeacherBinding::Default,
                            "unassigned" => TeacherBinding::Unassigned,
                            _ => TeacherBinding::Explicit(
                                PrincipalId::new(raw.parse().map_err(|_| "请选择任课老师")?)
                                    .map_err(|_| "请选择任课老师")?,
                            ),
                        };
                        Some(CourseCell {
                            subject_id: subject,
                            teacher,
                        })
                    };
                    next = next
                        .edit(
                            &DraftEdit::SetCell {
                                semester_id: p.semester.id,
                                table_id: t.id,
                                row_id,
                                day: Weekday::from_monday_index(day as u8).unwrap(),
                                cell,
                            },
                            identity,
                        )
                        .map_err(problem)?;
                    let p = &mut next.semesters[self.term.unwrap()];
                    p.cell_details
                        .retain(|d| d.row_id != row_id || d.day.index() != day);
                    let room = field_value(data, ROOM).trim();
                    let note = field_value(data, NOTE)
                        .replace("\r\n", "\n")
                        .replace('\r', "\n");
                    if has_course && (!room.is_empty() || !note.is_empty()) {
                        p.cell_details.push(CellDetails {
                            row_id,
                            day: Weekday::from_monday_index(day as u8).unwrap(),
                            room: room.into(),
                            note,
                        });
                    }
                    next.validate_draft_metadata().map_err(problem)?;
                    self.document = next;
                    self.dirty = true;
                    self.page = Page::Grid;
                }
                Page::Calendar => {
                    self.apply_calendar(data)?;
                }
                _ => return Err("当前页面不能应用此编辑。".into()),
            },
            ENABLE_DUAL | RESTORE_DUAL => {
                let p = self.plan()?;
                let mut seed = self.next_id()?;
                let enable = if action == RESTORE_DUAL {
                    EnableDual::RestoreHistory
                } else {
                    let mut alloc = || {
                        let id = seed;
                        seed = seed.checked_add(1).ok_or("标识已达上限")?;
                        Ok::<_, String>(id)
                    };
                    let odd_id = TableId::new(alloc()?).unwrap();
                    let even_id = TableId::new(alloc()?).unwrap();
                    let n = self.table()?.rows.len();
                    let mut odd_rows = vec![];
                    let mut even_rows = vec![];
                    for _ in 0..n {
                        odd_rows.push(RowId::new(alloc()?).unwrap());
                        even_rows.push(RowId::new(alloc()?).unwrap());
                    }
                    EnableDual::Rebuild {
                        odd_id,
                        even_id,
                        odd_rows,
                        even_rows,
                    }
                };
                self.replace_plan(p.enable_dual(enable).map_err(problem)?);
                self.table = 0;
            }
            DISABLE_DUAL => {
                let next = self
                    .plan()?
                    .disable_dual(self.table()?.id)
                    .map_err(problem)?;
                self.replace_plan(next);
                self.table = 0;
            }
            SYNC => {
                let next = self
                    .plan()?
                    .synchronize_timing(self.table()?.id)
                    .map_err(problem)?;
                self.replace_plan(next);
            }
            COPY => {
                let s = self.plan()?.semester.id;
                let t = self.table()?.id;
                self.document = self
                    .document
                    .edit(
                        &DraftEdit::CopyDay {
                            semester_id: s,
                            table_id: t,
                            from:
                                Weekday::from_monday_index(index_value(data, COPY_FROM, 7)? as u8)
                                    .unwrap(),
                            to: Weekday::from_monday_index(index_value(data, COPY_TO, 7)? as u8)
                                .unwrap(),
                        },
                        identity,
                    )
                    .map_err(problem)?;
                self.dirty = true;
            }
            REMOVE | MOVE_UP | MOVE_DOWN => {
                if let Page::Row(Some(index)) = self.page {
                    let s = self.plan()?.semester.id;
                    let t = self.table()?;
                    let row = t.rows.get(index).ok_or("作息行不存在")?.id;
                    let table = t.id;
                    let max = t.rows.len();
                    let edit = if action == REMOVE {
                        DraftEdit::RemoveRow {
                            semester_id: s,
                            table_id: table,
                            row_id: row,
                        }
                    } else {
                        DraftEdit::MoveRow {
                            semester_id: s,
                            table_id: table,
                            row_id: row,
                            index: if action == MOVE_UP {
                                index.saturating_sub(1)
                            } else {
                                (index + 1).min(max.saturating_sub(1))
                            },
                        }
                    };
                    self.document = self.document.edit(&edit, identity).map_err(problem)?;
                    self.dirty = true;
                    self.page = Page::Grid;
                } else {
                    return Err("请选择作息行。".into());
                }
            }
            SET_MAPPING | REMOVE_MAPPING => {
                // Apply mode/coverage first to a local clone, then the mapping;
                // any error leaves the complete original draft intact.
                let original = self.document.clone();
                let was_dirty = self.dirty;
                let operation = (|| {
                    self.apply_calendar(data)?;
                    let term = index_value(data, TERM, self.document.semesters.len())?;
                    let semester_id = self.document.semesters[term].semester.id;
                    let date = parse_date(field_value(data, MAP_DATE))?;
                    let cal = if self.document.calendar_mode == CalendarMode::School {
                        &mut self.document.school_calendar
                    } else {
                        &mut self.document.public_calendar
                    };
                    cal.entries
                        .retain(|e| e.date != date || e.actual_semester_id != semester_id);
                    if action == SET_MAPPING {
                        let mapping = match field_value(data, MAP_KIND) {
                            "rest" => DayMapping::Rest,
                            "unconfirmed" => DayMapping::Unconfirmed,
                            "source" => {
                                DayMapping::SourceDate(parse_date(field_value(data, MAP_SOURCE))?)
                            }
                            _ => return Err("请选择安排类型。".into()),
                        };
                        cal.entries.push(CalendarEntry {
                            actual_semester_id: semester_id,
                            date,
                            mapping,
                        });
                    }
                    self.document.validate_draft(identity).map_err(problem)?;
                    Ok::<_, String>(())
                })();
                if let Err(e) = operation {
                    self.document = original;
                    self.dirty = was_dirty;
                    return Err(e);
                }
                self.dirty = true;
            }
            _ => return Err("此课表操作在当前页面不可用。".into()),
        }
        Ok(EditorCommand::Stay)
    }
    fn apply_calendar(&mut self, data: &FormData) -> std::result::Result<(), String> {
        let mut next = self.document.clone();
        next.calendar_mode = match field_value(data, MODE) {
            "school" => CalendarMode::School,
            "public" => CalendarMode::Public,
            _ => return Err("请选择校历模式。".into()),
        };
        let a = field_value(data, START);
        let b = field_value(data, END);
        next.school_calendar.school_coverage = if a.is_empty() && b.is_empty() {
            None
        } else {
            let a = parse_date(a)?;
            let b = parse_date(b)?;
            if a > b {
                return Err("学校校历覆盖开始不得晚于结束。".into());
            }
            Some((a, b))
        };
        next.automatic_gaps = yes(data, AUTO_GAPS);
        next.validate_draft_structure().map_err(problem)?;
        self.document = next;
        self.dirty = true;
        Ok(())
    }
}
fn days() -> Vec<(String, String)> {
    [
        "星期一",
        "星期二",
        "星期三",
        "星期四",
        "星期五",
        "星期六",
        "星期日",
    ]
    .iter()
    .enumerate()
    .map(|(i, s)| (i.to_string(), (*s).into()))
    .collect()
}
fn previous_day(d: LocalDate) -> std::result::Result<LocalDate, String> {
    if d.day() > 1 {
        return LocalDate::new(d.year(), d.month(), d.day() - 1).map_err(|_| "日期无效".into());
    }
    let (year, month) = if d.month() == 1 {
        (d.year() - 1, 12)
    } else {
        (d.year(), d.month() - 1)
    };
    (28..=31)
        .rev()
        .find_map(|day| LocalDate::new(year, month, day).ok())
        .ok_or_else(|| "日期已达范围下限。".into())
}

fn lesson_text(state: LessonState, identity: &IdentityState) -> String {
    match state {
        LessonState::Course { cell, .. } => format!(
            "正在上课：{}",
            identity
                .subjects
                .iter()
                .find(|s| s.id == cell.subject_id)
                .map(|s| s.name.as_str())
                .unwrap_or("科目不可用")
        ),
        LessonState::Recess {
            remaining_seconds,
            until_next_course,
            ..
        } => format!(
            "课间：距{}还有{:02}分{:02}秒",
            if until_next_course {
                "下节上课"
            } else {
                "本次休息结束"
            },
            remaining_seconds / 60,
            remaining_seconds % 60
        ),
        LessonState::Lunch => "午休，不显示课间倒计时。".into(),
        LessonState::Activity => "活动时段，不显示课间倒计时。".into(),
        LessonState::Rest => "休息日或休息时段。".into(),
        LessonState::Idle => "当前无课。".into(),
        LessonState::Unconfirmed => "校历或补课安排待确认，不高亮课程。".into(),
        LessonState::OutOfTerm => "今天不在已启用的学期范围内。".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Editor, IdentityState) {
        let school = SchoolId::new(1).unwrap();
        let class = ClassId::new(1).unwrap();
        let subject = SubjectId::new(1).unwrap();
        let account = |id, role| UserAccount {
            principal: Principal {
                id: PrincipalId::new(id).unwrap(),
                school_id: school,
                role,
                enabled: true,
                auth_epoch: AuthEpoch::new(1),
            },
            login_name: format!("user{id}"),
            display_name: format!("用户{id}"),
            must_change_password: false,
            credential_version: 1,
            recovery_version: 1,
        };
        let identity = IdentityState {
            revision: Revision::new(1),
            installation: Some(Installation {
                school_id: school,
                school_name: "验证学校".into(),
                current_class: Some(class),
            }),
            classes: vec![SchoolClass {
                id: class,
                school_id: school,
                name: "一班".into(),
                enabled: true,
            }],
            subjects: vec![SchoolSubject {
                id: subject,
                school_id: school,
                name: "数学".into(),
                enabled: true,
                color: Rgb {
                    red: 70,
                    green: 120,
                    blue: 180,
                },
                revision: Revision::new(1),
            }],
            accounts: vec![
                account(1, Role::GlobalAdmin),
                account(2, Role::TeacherAdmin),
                account(3, Role::GlobalAdmin),
            ],
            assignments: vec![TeachingAssignment {
                teacher_id: PrincipalId::new(2).unwrap(),
                class_id: class,
                subject_id: subject,
                enabled: true,
            }],
            default_teachers: vec![],
        };
        identity.validate().unwrap();
        let row = TimetableRow {
            id: RowId::new(3).unwrap(),
            logical_key: LogicalRowKey::new(3).unwrap(),
            group: DayGroup::Morning,
            kind: RowKind::Course,
            range: MinuteRange::new(
                Minute::from_hm(8, 20).unwrap(),
                Minute::from_hm(9, 0).unwrap(),
            )
            .unwrap(),
            enabled_days: [true, true, true, true, true, false, false],
            courses: std::array::from_fn(|_| None),
        };
        let plan = SemesterPlan::new(
            Semester {
                id: SemesterId::new(1).unwrap(),
                class_id: class,
                revision: Revision::default(),
                enabled: true,
                start: parse_date("2026-09-01").unwrap(),
                end: parse_date("2027-01-31").unwrap(),
                first_week_monday: parse_date("2026-08-31").unwrap(),
                tables: vec![WeekTable {
                    id: TableId::new(2).unwrap(),
                    role: TableRole::Single,
                    rows: vec![row],
                }],
            },
            "验证学期".into(),
        );
        let mut document = ScheduleDocument::default();
        document.semesters.push(plan);
        let mut e = Editor::new(document, PrincipalId::new(1).unwrap());
        e.term = Some(0);
        (e, identity)
    }
    fn form(v: &View) -> FormData {
        let mut d = FormData::new();
        for f in &v.fields {
            d.insert(f.id, f.value.clone()).unwrap();
        }
        d
    }
    fn set(data: &mut FormData, id: FieldId, value: &str) {
        data.take(id);
        data.insert(id, value.into()).unwrap();
    }
    #[test]
    fn every_editor_page_has_a_bounded_valid_native_schema() {
        let (mut e, identity) = fixture();
        for page in [
            Page::Terms,
            Page::Header,
            Page::Grid,
            Page::Row(None),
            Page::Row(Some(0)),
            Page::Cell(0, 0),
            Page::Calendar,
            Page::Preview,
            Page::Discard,
        ] {
            e.page = page;
            e.view(&identity, "", false).validate().unwrap();
        }
    }
    #[test]
    fn incomplete_time_and_date_survive_capture_without_mutating_the_document() {
        let (mut e, identity) = fixture();
        let original = e.document.clone();
        e.page = Page::Row(Some(0));
        let mut data = form(&e.view(&identity, "", false));
        set(&mut data, RANGE, "08X00900");
        assert!(e.handle(APPLY, data, &identity).is_err());
        assert_eq!(
            e.view(&identity, "", false)
                .fields
                .iter()
                .find(|f| f.id == RANGE)
                .unwrap()
                .value,
            "08X00900"
        );
        assert_eq!(e.document, original);
        e.page = Page::Header;
        let mut data = form(&e.view(&identity, "", false));
        set(&mut data, START, "2026-");
        e.capture(data);
        assert_eq!(
            e.view(&identity, "", false)
                .fields
                .iter()
                .find(|f| f.id == START)
                .unwrap()
                .value,
            "2026-"
        );
        assert_eq!(e.document, original);
    }
    #[test]
    fn subject_selection_filters_teachers_and_auto_selects_the_only_candidate() {
        let (mut e, identity) = fixture();
        e.select_cell(0, 1, &identity).unwrap();
        let mut data = form(&e.view(&identity, "", false));
        set(&mut data, SUBJECT, "1");
        set(&mut data, ROOM, "保留教室草稿");
        assert!(e.input(data, &identity));
        let v = e.view(&identity, "", false);
        let teacher = v.fields.iter().find(|f| f.id == TEACHER).unwrap();
        assert_eq!(teacher.value, "2");
        let FieldKind::Choice(options) = &teacher.kind else {
            panic!()
        };
        assert!(options.iter().any(|c| c.value == "2"));
        assert!(!options.iter().any(|c| c.value == "3"));
        assert_eq!(
            v.fields.iter().find(|f| f.id == ROOM).unwrap().value,
            "保留教室草稿"
        );
        e.handle(APPLY, form(&v), &identity).unwrap();
        assert_eq!(
            e.document.semesters[0].semester.tables[0].rows[0].courses[0]
                .as_ref()
                .unwrap()
                .teacher,
            TeacherBinding::Explicit(PrincipalId::new(2).unwrap())
        );
        assert_eq!(e.document.revision, Revision::default());
    }
    #[test]
    fn save_preserves_captured_revision_and_cancel_requires_explicit_confirmation() {
        let (mut e, identity) = fixture();
        e.expected = Revision::new(8);
        e.document.revision = Revision::new(8);
        let original = e.document.clone();
        match e.handle(SAVE_ALL, FormData::new(), &identity).unwrap() {
            EditorCommand::Save { expected, document } => {
                assert_eq!(expected, Revision::new(8));
                assert_eq!(document, original);
            }
            _ => panic!(),
        }
        assert!(matches!(
            e.handle(CANCEL, FormData::new(), &identity).unwrap(),
            EditorCommand::Stay
        ));
        assert_eq!(e.page, Page::Discard);
        assert_eq!(e.document, original);
        assert!(matches!(
            e.handle(CONFIRM_CANCEL, FormData::new(), &identity)
                .unwrap(),
            EditorCommand::Reload
        ));
    }
    #[test]
    fn date_parsing_rejects_untrusted_shapes_and_handles_previous_year_boundary() {
        for text in [
            "２０２６-09-01",
            "2026-9-01",
            "2026-02-29",
            "2026-01-01x",
            "2026-00-01",
        ] {
            assert!(parse_date(text).is_err());
        }
        assert_eq!(
            date_text(previous_day(parse_date("2027-01-01").unwrap()).unwrap()),
            "2026-12-31"
        );
        assert_eq!(
            date_text(previous_day(parse_date("2024-03-01").unwrap()).unwrap()),
            "2024-02-29"
        );
    }
    #[test]
    fn clearing_course_with_existing_room_and_note_removes_metadata_atomically() {
        let (mut e, identity) = fixture();
        let p = &mut e.document.semesters[0];
        p.semester.tables[0].rows[0].courses[0] = Some(CourseCell {
            subject_id: SubjectId::new(1).unwrap(),
            teacher: TeacherBinding::Explicit(PrincipalId::new(2).unwrap()),
        });
        p.cell_details.push(CellDetails {
            row_id: RowId::new(3).unwrap(),
            day: Weekday::Monday,
            room: "原教室".into(),
            note: "原备注".into(),
        });
        e.select_cell(0, 1, &identity).unwrap();
        let mut data = form(&e.view(&identity, "", false));
        set(&mut data, SUBJECT, "none");
        e.handle(APPLY, data, &identity).unwrap();
        assert!(e.document.semesters[0].semester.tables[0].rows[0].courses[0].is_none());
        assert!(e.document.semesters[0].cell_details.is_empty());
        e.document.validate_draft(&identity).unwrap();
    }
    #[test]
    fn invalid_metadata_does_not_poison_a_draft_and_can_be_corrected() {
        let (mut e, identity) = fixture();
        e.page = Page::Row(Some(0));
        let original = e.document.clone();
        let mut data = form(&e.view(&identity, "", false));
        set(&mut data, ROW_NAME, &"a".repeat(65));
        assert!(e.handle(APPLY, data, &identity).is_err());
        assert_eq!(e.document, original);
        let mut data = form(&e.view(&identity, "", false));
        set(&mut data, ROW_NAME, "第一节");
        e.handle(APPLY, data, &identity).unwrap();
        e.document.validate_draft_metadata().unwrap();
    }
    #[test]
    fn unavailable_choices_remain_visible_for_repair_without_replacing_the_teacher() {
        let (mut e, mut identity) = fixture();
        e.document.semesters[0].semester.tables[0].rows[0].courses[0] = Some(CourseCell {
            subject_id: SubjectId::new(1).unwrap(),
            teacher: TeacherBinding::Explicit(PrincipalId::new(2).unwrap()),
        });
        e.select_cell(0, 1, &identity).unwrap();
        identity.subjects[0].enabled = false;
        identity.accounts[1].principal.enabled = false;
        let v = e.view(&identity, "", false);
        v.validate().unwrap();
        for (id, value) in [(SUBJECT, "1"), (TEACHER, "2")] {
            let f = v.fields.iter().find(|f| f.id == id).unwrap();
            assert_eq!(f.value, value);
            let FieldKind::Choice(options) = &f.kind else {
                panic!()
            };
            assert!(options
                .iter()
                .any(|c| c.value == value && c.label.contains("失效")));
        }
        let original = e.document.clone();
        assert!(e.handle(APPLY, form(&v), &identity).is_err());
        assert_eq!(e.document, original);
        e.page = Page::Header;
        identity.classes[0].enabled = false;
        e.suspended = None;
        e.view(&identity, "", false).validate().unwrap();
    }
    #[test]
    fn failed_mapping_does_not_apply_mode_coverage_or_dirty_state() {
        let (mut e, identity) = fixture();
        e.page = Page::Calendar;
        let original = e.document.clone();
        let mut data = form(&e.view(&identity, "", false));
        set(&mut data, MODE, "school");
        set(&mut data, START, "2026-09-01");
        set(&mut data, END, "2027-01-31");
        set(&mut data, MAP_DATE, "2025-01-01");
        assert!(e.handle(SET_MAPPING, data, &identity).is_err());
        assert_eq!(e.document, original);
        assert!(!e.dirty);
    }
}
