//! Path-free material workbench. Every selection captures immutable record IDs;
//! the backend rechecks the live lease, scope and revision in its writer transaction.
use super::{material_flow::BatchKind, NativeCommand};
use fc_domain::{auth::Role, identity::IdentityState, ids::*, materials::*};
use fc_ports::MaterialListRequest;
use fc_presentation::workbench::*;
use fc_windows::materials::chooser::SelectedFile;

pub(super) const TEACHER: FieldId = FieldId(300);
pub(super) const SUBJECT: FieldId = FieldId(301);
pub(super) const CLASSES: FieldId = FieldId(302);
const FULL_NAME: FieldId = FieldId(303);
const BATCH_RESULT: FieldId = FieldId(304);
pub(super) const LOAD: ActionId = ActionId(300);
pub(super) const IMPORT: ActionId = ActionId(301);
pub(super) const DELETE: ActionId = ActionId(302);
pub(super) const OPEN: ActionId = ActionId(303);
pub(super) const NEXT: ActionId = ActionId(304);
pub(super) const FIRST: ActionId = ActionId(305);
pub(super) const CONFIRM_DELETE: ActionId = ActionId(306);
pub(super) const CANCEL_DELETE: ActionId = ActionId(307);
pub(super) const COPIES: ActionId = ActionId(308);
pub(super) const CLEAN_COPIES: ActionId = ActionId(309);
pub(super) const RETURN_COPIES: ActionId = ActionId(310);
pub(super) const CONFIRM_BATCH: ActionId = ActionId(311);
pub(super) const CANCEL_BATCH: ActionId = ActionId(312);
pub(super) const BATCH_UP: ActionId = ActionId(313);
pub(super) const BATCH_DOWN: ActionId = ActionId(314);
pub(super) const BATCH_REMOVE: ActionId = ActionId(315);
pub(super) const QUERY_BATCH: ActionId = ActionId(316);
pub(super) const DISMISS_BATCH: ActionId = ActionId(317);
pub(super) const TOGGLE_DELETE: ActionId = ActionId(318);
pub(super) const SELECT_DELETE_PAGE: ActionId = ActionId(319);
pub(super) const CLEAR_DELETE: ActionId = ActionId(320);
pub(super) const DELETE_BATCH: ActionId = ActionId(321);
pub(super) const CONFIRM_DELETE_BATCH: ActionId = ActionId(322);
pub(super) const CANCEL_DELETE_BATCH: ActionId = ActionId(323);
const BACK: ActionId = ActionId(2);
const LOCK: ActionId = ActionId(6);

pub(super) enum Command {
    Stay,
    Copies {
        clean: bool,
    },
    Load(MaterialListRequest),
    Import {
        scope: MaterialScope,
        allowed_class_ids: Vec<ClassId>,
    },
    ImportBatch {
        scope: MaterialScope,
        classes: Vec<ClassId>,
        selected: Vec<SelectedFile>,
        command: NativeCommand,
    },
    DeleteBatch {
        scope: MaterialScope,
        records: Vec<MaterialRecord>,
        command: NativeCommand,
    },
    QueryBatch {
        kind: BatchKind,
        scope: MaterialScope,
        command: NativeCommand,
    },
    Delete {
        scope: MaterialScope,
        material_id: MaterialId,
        expected_material_revision: Revision,
    },
    Open {
        scope: MaterialScope,
        material_id: MaterialId,
        resource_version: ResourceVersionId,
    },
    Close,
    Lock,
}

struct BatchDraft {
    scope: MaterialScope,
    classes: Vec<ClassId>,
    files: Vec<SelectedFile>,
    names: Vec<String>,
    selected: usize,
}
struct BatchHistory {
    kind: BatchKind,
    delete_originals: Vec<MaterialRecord>,
    scope: MaterialScope,
    names: Vec<String>,
    command: NativeCommand,
    classes: Vec<ClassId>,
    outcome: super::material_flow::BatchOutcome,
}
pub(super) struct Editor {
    owner: PrincipalId,
    scope: Option<MaterialScope>,
    page: Option<MaterialPage>,
    pending: Option<MaterialListRequest>,
    selected: Option<MaterialRecord>,
    confirmation: bool,
    delete_selection: Vec<MaterialRecord>,
    delete_confirmation: bool,
    delete_mode: bool,
    copies: Option<super::material_flow::LaunchCopyReport>,
    batch: Option<BatchDraft>,
    history: Option<BatchHistory>,
}

fn action(id: ActionId, label: &str, enabled: bool) -> Action {
    Action {
        id,
        label: label.into(),
        enabled,
    }
}
fn choice(id: FieldId, label: &str, value: String, choices: Vec<Choice>) -> Field {
    Field {
        id,
        label: label.into(),
        kind: FieldKind::Choice(choices),
        value,
        max_utf16: 128,
    }
}
fn valid_role(identity: &IdentityState, owner: PrincipalId) -> Option<Role> {
    let account = identity.account(owner).ok()?;
    (account.principal.enabled
        && !account.must_change_password
        && account.principal.role != Role::User)
        .then_some(account.principal.role)
}
fn allowed_scope(identity: &IdentityState, owner: PrincipalId, scope: MaterialScope) -> bool {
    match valid_role(identity, owner) {
        Some(Role::GlobalAdmin) => {
            identity
                .accounts
                .iter()
                .any(|a| a.principal.id == scope.teacher_id)
                && identity.subjects.iter().any(|s| s.id == scope.subject_id)
        }
        Some(Role::TeacherAdmin) => {
            scope.teacher_id == owner
                && identity.assignments.iter().any(|a| {
                    a.teacher_id == owner
                        && a.subject_id == scope.subject_id
                        && identity.is_teacher(owner, a.class_id, a.subject_id)
                })
        }
        _ => false,
    }
}
fn classes(identity: &IdentityState, scope: MaterialScope) -> Vec<ClassId> {
    identity
        .classes
        .iter()
        .filter(|c| identity.is_teacher(scope.teacher_id, c.id, scope.subject_id))
        .map(|c| c.id)
        .collect()
}
fn subject_choices(
    identity: &IdentityState,
    owner: PrincipalId,
    teacher: PrincipalId,
) -> Vec<Choice> {
    identity
        .subjects
        .iter()
        .filter(|s| {
            allowed_scope(
                identity,
                owner,
                MaterialScope {
                    teacher_id: teacher,
                    subject_id: s.id,
                },
            )
        })
        .map(|s| Choice {
            value: s.id.get().to_string(),
            label: format!("{}{}", s.name, if s.enabled { "" } else { "（已停用）" }),
        })
        .collect()
}
fn teacher_choices(identity: &IdentityState, owner: PrincipalId) -> Vec<Choice> {
    let global = valid_role(identity, owner) == Some(Role::GlobalAdmin);
    identity
        .accounts
        .iter()
        .filter(|a| global || a.principal.id == owner && valid_role(identity, owner).is_some())
        .map(|a| Choice {
            value: a.principal.id.get().to_string(),
            label: format!(
                "{}{}",
                a.display_name,
                if a.principal.enabled {
                    ""
                } else {
                    "（已停用）"
                }
            ),
        })
        .collect()
}
fn time_text(at: fc_domain::time::LocalDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        at.date.year(),
        at.date.month(),
        at.date.day(),
        at.second_of_day() / 3600,
        at.second_of_day() / 60 % 60,
        at.second_of_day() % 60
    )
}

impl Editor {
    pub(super) fn new(owner: PrincipalId) -> Self {
        Self {
            owner,
            scope: None,
            page: None,
            pending: None,
            selected: None,
            confirmation: false,
            delete_selection: Vec::new(),
            delete_confirmation: false,
            delete_mode: false,
            copies: None,
            batch: None,
            history: None,
        }
    }
    fn displayed_scope(&self, identity: &IdentityState) -> Option<MaterialScope> {
        self.scope
            .filter(|s| allowed_scope(identity, self.owner, *s))
            .or_else(|| {
                let teacher = teacher_choices(identity, self.owner)
                    .first()?
                    .value
                    .parse()
                    .ok()
                    .and_then(|v| PrincipalId::new(v).ok())?;
                let subject = subject_choices(identity, self.owner, teacher)
                    .first()?
                    .value
                    .parse()
                    .ok()
                    .and_then(|v| SubjectId::new(v).ok())?;
                Some(MaterialScope {
                    teacher_id: teacher,
                    subject_id: subject,
                })
            })
    }
    pub(super) fn view(&self, identity: &IdentityState, status: &str, busy: bool) -> View {
        let mut view = View {
            title: "资料管理".into(),
            description: "选择老师与科目后载入目录。原文件保持不变，打开时使用独立副本。".into(),
            status: status.into(),
            fields: Vec::new(),
            actions: Vec::new(),
            busy,
            grid: None,
        };
        let global = valid_role(identity, self.owner) == Some(Role::GlobalAdmin);
        if let Some(copies) = self.copies.filter(|_| global) {
            view.title = "打开副本管理".into();
            view.description = format!(
                "占用 {} 字节 / 200MiB，{} / 128 份；已到期 {} 份。本次已清理 {} 份，占用中保留 {} 份。清理只删除最后写入已满24小时且未被占用的副本。原件和未到期副本继续保留。",
                copies.bytes, copies.count, copies.expired, copies.removed, copies.retained);
            view.actions = vec![
                action(COPIES, "刷新占用", !busy),
                action(
                    CLEAN_COPIES,
                    "确认清理已到期副本",
                    !busy && copies.expired > 0,
                ),
                action(RETURN_COPIES, "返回资料目录", !busy),
                action(LOCK, "锁定", true),
            ];
            return view;
        }
        let Some(scope) = self.displayed_scope(identity) else {
            view.description = "当前账号没有可管理的资料目录。".into();
            view.actions = vec![action(BACK, "返回", !busy), action(LOCK, "锁定", true)];
            if global {
                view.actions
                    .insert(0, action(COPIES, "打开副本管理", !busy));
            }
            return view;
        };
        if let Some(batch) = &self.batch {
            view.title = "确认批量导入课堂资料".into();
            view.description = format!("共{}个文件，将按下列顺序校验后一次提交。最后一项是本批最新项，提交后显示在目录最上方；任一文件准备失败，整批不加入目录。", batch.files.len());
            view.grid = Some(Grid {
                columns: vec!["文件名".into(), "加入顺序".into()],
                rows: batch
                    .names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| GridRow {
                        group: String::new(),
                        label: name.clone(),
                        cells: vec![format!(
                            "{}{}",
                            index + 1,
                            if index + 1 == batch.files.len() {
                                "（本批最新）"
                            } else {
                                ""
                            }
                        )],
                        merged: false,
                    })
                    .collect(),
                selected: Some((batch.selected, 0)),
                enabled: !busy,
            });
            view.actions = vec![
                action(BATCH_UP, "上移（较早加入）", !busy && batch.selected > 0),
                action(
                    BATCH_DOWN,
                    "下移（较新加入）",
                    !busy && batch.selected + 1 < batch.files.len(),
                ),
                action(BATCH_REMOVE, "移除选中文件", !busy),
                action(
                    CONFIRM_BATCH,
                    "确认校验并一次加入整批",
                    !busy && !batch.files.is_empty(),
                ),
                action(CANCEL_BATCH, "取消本批", !busy),
                action(LOCK, "锁定", true),
            ];
            return view;
        }
        if let Some(history) = &self.history {
            let state = match &history.outcome {
                super::material_flow::BatchOutcome::Saved(_) => "原批次已保存",
                super::material_flow::BatchOutcome::NotSubmitted(_) => "本批未送交目录写入",
                super::material_flow::BatchOutcome::Rejected(_) => "本批写入已被明确拒绝",
                super::material_flow::BatchOutcome::Expired => {
                    "原批次查询窗口已过期（不是保存证明）"
                }
                super::material_flow::BatchOutcome::Unknown => "原批次尚未取得确定结果",
            };
            view.fields.push(Field {
                id: BATCH_RESULT, label: state.into(), kind: FieldKind::ReadOnly,
                value: match history.kind {
                    BatchKind::Import => format!("{}个文件；本批最新：{}。当前目录刷新不代表原命令结果；查询使用原票据，不重新导入。", history.names.len(), history.names.last().map_or("", String::as_str)),
                    BatchKind::Delete => format!("原批次删除{}项固定资料。当前目录刷新不代表原命令结果；查询使用原票据，不重新删除。", history.names.len()),
                },
                max_utf16: 1024,
            });
        }
        if self.delete_confirmation {
            view.title = "确认批量删除课堂资料".into();
            view.description = format!("将一次删除以下{}项固定资料。任一资料版本已变，整批不删除；打开副本按原有规则安全回收。", self.delete_selection.len());
            view.grid = Some(Grid {
                columns: vec!["完整文件名".into(), "固定资料 / 资源版本 / 修订".into()],
                rows: self
                    .delete_selection
                    .iter()
                    .map(|record| GridRow {
                        group: String::new(),
                        label: record.display_name.clone(),
                        cells: vec![format!(
                            "{} / {} / {}",
                            record.id.get(),
                            record.resource.version_id.get(),
                            record.revision.get()
                        )],
                        merged: false,
                    })
                    .collect(),
                selected: None,
                enabled: false,
            });
            view.actions = vec![
                action(
                    CONFIRM_DELETE_BATCH,
                    "确认一次删除整批",
                    !busy && !self.delete_selection.is_empty(),
                ),
                action(CANCEL_DELETE_BATCH, "保留本批资料", !busy),
                action(LOCK, "锁定", true),
            ];
            return view;
        }
        if self.confirmation {
            if let Some(record) = &self.selected {
                if self.scope == Some(scope) {
                    view.title = "确认删除资料".into();
                    view.description =
                        "删除后立即从资料列表隐藏；正在使用的打开副本由后台安全回收。".into();
                    view.fields.push(Field {
                        id: FULL_NAME,
                        label: "完整文件名".into(),
                        kind: FieldKind::ReadOnly,
                        value: record.display_name.clone(),
                        max_utf16: 512,
                    });
                    view.actions = vec![
                        action(CONFIRM_DELETE, "确认删除", !busy),
                        action(CANCEL_DELETE, "保留资料", !busy),
                        action(LOCK, "锁定", true),
                    ];
                    return view;
                }
            }
        }
        view.fields.push(choice(
            TEACHER,
            "资料所属老师",
            scope.teacher_id.get().to_string(),
            teacher_choices(identity, self.owner),
        ));
        view.fields.push(choice(
            SUBJECT,
            "科目",
            scope.subject_id.get().to_string(),
            subject_choices(identity, self.owner, scope.teacher_id),
        ));
        let current_classes = classes(identity, scope);
        let mut options = vec![Choice {
            value: "all".into(),
            label: "该老师该科目全部有效任教班级".into(),
        }];
        options.extend(
            current_classes
                .iter()
                .filter_map(|id| identity.classes.iter().find(|c| c.id == *id))
                .map(|c| Choice {
                    value: c.id.get().to_string(),
                    label: c.name.clone(),
                }),
        );
        view.fields
            .push(choice(CLASSES, "本次导入可见班级", "all".into(), options));
        let page = self.page.as_ref().filter(|p| p.folder.scope == scope);
        let selected = self
            .selected
            .as_ref()
            .filter(|r| page.is_some_and(|p| p.items.iter().any(|v| v == *r)));
        if let Some(record) = selected {
            view.fields.push(Field {
                id: FULL_NAME,
                label: "选中资料完整文件名".into(),
                kind: FieldKind::ReadOnly,
                value: record.display_name.clone(),
                max_utf16: 512,
            });
        }
        if let Some(page) = page {
            view.description = format!(
                "资料按成功加入顺序从新到旧排列。本页{}项，共{}项（从第{}项起）。已勾选{}项；刷新、翻页或切换目录会清空勾选。",
                page.items.len(),
                page.total,
                if page.total == 0 { 0 } else { page.offset + 1 },
                self.delete_selection.len()
            );
            view.grid = Some(Grid {
                columns: vec![
                    "文件名".into(),
                    "加入时间".into(),
                    "大小 / 批量删除勾选".into(),
                ],
                rows: page
                    .items
                    .iter()
                    .map(|r| GridRow {
                        group: String::new(),
                        label: r.display_name.clone(),
                        cells: vec![
                            time_text(r.added_at),
                            format!(
                                "{} 字节 {}",
                                r.resource.bytes,
                                if self.delete_selection.iter().any(|selected| selected == r) {
                                    "☑ 已勾选"
                                } else {
                                    "☐ 未勾选"
                                }
                            ),
                        ],
                        merged: false,
                    })
                    .collect(),
                selected: selected
                    .and_then(|r| page.items.iter().position(|v| v == r))
                    .map(|row| (row, 0)),
                enabled: !busy,
            });
        }
        view.actions = vec![
            action(LOAD, "载入目录 / 刷新", !busy),
            action(
                IMPORT,
                "批量导入资料",
                !busy && !current_classes.is_empty() && self.history.is_none(),
            ),
            action(OPEN, "打开选中资料", !busy && selected.is_some()),
            action(DELETE, "删除选中资料", !busy && selected.is_some()),
            action(
                NEXT,
                "下一页",
                !busy && page.is_some_and(|p| p.next_cursor.is_some()),
            ),
            action(FIRST, "回到第一页", !busy && page.is_some()),
            action(BACK, "返回", !busy),
            action(LOCK, "锁定", true),
        ];
        if self.delete_mode {
            view.actions = vec![
                action(
                    TOGGLE_DELETE,
                    "勾选 / 取消当前资料",
                    !busy && selected.is_some(),
                ),
                action(
                    SELECT_DELETE_PAGE,
                    "勾选本页（最多20项）",
                    !busy && page.is_some_and(|page| !page.items.is_empty()),
                ),
                action(
                    CLEAR_DELETE,
                    "清空勾选",
                    !busy && !self.delete_selection.is_empty(),
                ),
                action(
                    DELETE_BATCH,
                    "核对已勾选删除清单…",
                    !busy && !self.delete_selection.is_empty(),
                ),
                action(CANCEL_DELETE_BATCH, "退出批量删除", !busy),
                action(LOAD, "刷新并清空勾选", !busy),
                action(
                    NEXT,
                    "下一页并清空勾选",
                    !busy && page.is_some_and(|page| page.next_cursor.is_some()),
                ),
                action(BACK, "返回", !busy),
                action(LOCK, "锁定", true),
            ];
        } else {
            view.actions.push(action(
                DELETE_BATCH,
                "批量删除…",
                !busy && page.is_some_and(|page| !page.items.is_empty()) && self.history.is_none(),
            ));
        }
        if let Some(history) = &self.history {
            view.actions
                .push(action(QUERY_BATCH, "查询原批次结果（不重发）", !busy));
            view.actions.push(action(
                DISMISS_BATCH,
                "结束本次操作（不重新执行）",
                !busy && !matches!(history.outcome, super::material_flow::BatchOutcome::Unknown),
            ));
        }
        if global {
            view.actions
                .insert(6, action(COPIES, "打开副本管理", !busy));
        }
        view
    }
    pub(super) fn copies_loaded(&mut self, report: super::material_flow::LaunchCopyReport) {
        self.copies = Some(report);
        self.clear_selection();
    }
    fn clear_selection(&mut self) {
        self.selected = None;
        self.confirmation = false;
        self.delete_selection.clear();
        self.delete_confirmation = false;
        self.delete_mode = false;
    }
    fn request(&mut self, scope: MaterialScope, cursor: Option<MaterialCursor>) -> Command {
        self.scope = Some(scope);
        self.page = None;
        self.clear_selection();
        let request = MaterialListRequest { scope, cursor };
        self.pending = Some(request.clone());
        Command::Load(request)
    }
    /// A late result for another folder/page is never adopted into this editor.
    pub(super) fn loaded(&mut self, page: MaterialPage) -> Result<(), String> {
        let pending = self.pending.as_ref().ok_or("目录请求已经失效，请刷新。")?;
        let expected_offset = pending.cursor.as_ref().map_or(0, |c| c.offset);
        if page.folder.scope != pending.scope
            || page.source.is_some()
            || page.class_id.is_some()
            || page.offset != expected_offset
            || page.items.len() > MANAGEMENT_MATERIAL_PAGE
            || page.items.iter().any(|r| {
                r.folder_id != page.folder.id
                    || r.status != MaterialStatus::Published
                    || r.validate().is_err()
            })
            || pending
                .cursor
                .as_ref()
                .is_some_and(|c| c.validate(&page.folder, None, None).is_err())
            || page.next_cursor.as_ref().is_some_and(|c| {
                c.validate(&page.folder, None, None).is_err()
                    || c.offset != page.offset + page.items.len() as u64
            })
        {
            return Err("目录结果已失效，请回到第一页刷新。".into());
        }
        self.scope = Some(page.folder.scope);
        self.page = Some(page);
        self.pending = None;
        self.clear_selection();
        Ok(())
    }
    pub(super) fn select(&mut self, row: usize, column: usize) -> Result<(), String> {
        if let Some(batch) = &mut self.batch {
            if column >= 2 || row >= batch.files.len() {
                return Err("请选择本批有效文件。".into());
            }
            batch.selected = row;
            return Ok(());
        }
        if self.confirmation || self.delete_confirmation || column >= 3 {
            return Err("当前不能改变资料选择。".into());
        }
        let record = self
            .page
            .as_ref()
            .and_then(|p| p.items.get(row))
            .ok_or("请选择有效资料。")?;
        self.selected = Some(record.clone());
        Ok(())
    }
    pub(super) fn stage_batch(
        &mut self,
        scope: MaterialScope,
        classes: Vec<ClassId>,
        files: Vec<SelectedFile>,
    ) -> Result<(), String> {
        if files.is_empty()
            || files.len() > MAX_MATERIAL_BATCH_FILES
            || self.history.is_some()
            || classes.is_empty()
        {
            return Err("请先处理原批次结果；每批选择1至20个文件。".into());
        }
        let names = files
            .iter()
            .map(|file| {
                file.display_name()
                    .map_err(|_| "无法读取文件名。".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if names
            .iter()
            .any(|name| validate_material_name(name).is_err())
        {
            return Err("本批含无效或不受支持的文件名，请重新选择。".into());
        }
        self.batch = Some(BatchDraft {
            scope,
            classes,
            files,
            names,
            selected: 0,
        });
        self.clear_selection();
        Ok(())
    }
    pub(super) fn has_original_batch(&self, command: &NativeCommand, scope: MaterialScope) -> bool {
        self.history
            .as_ref()
            .is_some_and(|history| &history.command == command && history.scope == scope)
    }
    pub(super) fn original_batch_kind(
        &self,
        command: &NativeCommand,
        scope: MaterialScope,
    ) -> Option<BatchKind> {
        self.history
            .as_ref()
            .filter(|history| &history.command == command && history.scope == scope)
            .map(|history| history.kind)
    }
    pub(super) fn owned_by(&self, owner: PrincipalId) -> bool {
        self.owner == owner
    }
    pub(super) fn suspend(&mut self) {
        self.batch = None;
        self.clear_selection();
    }
    pub(super) fn batch_result(
        &mut self,
        result: super::material_flow::BatchResult,
    ) -> Result<(), String> {
        use super::material_flow::BatchOutcome;
        let history = self.history.as_mut().ok_or("原批次页面已关闭。")?;
        if history.kind != result.kind
            || history.command != result.command
            || history.scope != result.scope
            || history.command.snapshot() != result.issued
        {
            return Err("批次结果与原请求或完整票据不一致。".into());
        }
        if let BatchOutcome::Saved(records) = &result.outcome {
            if result.issued.is_none()
                || records.len() != history.names.len()
                || records.iter().zip(&history.names).any(|(record, name)| {
                    record.display_name != *name
                        || record.validate().is_err()
                        || match history.kind {
                            BatchKind::Import => {
                                record.allowed_class_ids != history.classes
                                    || record.status != MaterialStatus::Published
                            }
                            BatchKind::Delete => record.status != MaterialStatus::Deleted,
                        }
                })
                || history.kind == BatchKind::Import
                    && records.windows(2).any(|pair| {
                        pair[1].added_sequence != pair[0].added_sequence.checked_add(1).unwrap_or(0)
                    })
                || history.kind == BatchKind::Delete
                    && (records.len() != history.delete_originals.len()
                        || records.iter().zip(&history.delete_originals).any(
                            |(record, original)| {
                                let mut expected = original.clone();
                                let Ok(revision) = expected.revision.next() else {
                                    return true;
                                };
                                expected.revision = revision;
                                expected.status = MaterialStatus::Deleted;
                                *record != expected
                            },
                        ))
                || records.iter().enumerate().any(|(index, record)| {
                    records[..index].iter().any(|prior| {
                        prior.id == record.id
                            || prior.resource.version_id == record.resource.version_id
                    })
                })
            {
                return Err("批次结果不完整或顺序不同，请保留原票据查询。".into());
            }
        }
        // A later absent query never erases an already authoritative receipt or
        // the known fact that this worker did not call the Writer.
        if !matches!(result.outcome, BatchOutcome::Unknown)
            || matches!(history.outcome, BatchOutcome::Unknown)
        {
            history.outcome = result.outcome;
        }
        Ok(())
    }
    fn scope_from(
        &self,
        form: &FormData,
        identity: &IdentityState,
    ) -> Result<MaterialScope, String> {
        let teacher_id = form
            .get(TEACHER)
            .and_then(|v| v.parse().ok())
            .and_then(|v| PrincipalId::new(v).ok())
            .ok_or("请选择资料所属老师。")?;
        let subject_id = form
            .get(SUBJECT)
            .and_then(|v| v.parse().ok())
            .and_then(|v| SubjectId::new(v).ok())
            .ok_or("请选择科目。")?;
        let scope = MaterialScope {
            teacher_id,
            subject_id,
        };
        if !allowed_scope(identity, self.owner, scope) {
            return Err("当前账号不能管理这个老师和科目的资料。".into());
        }
        Ok(scope)
    }
    pub(super) fn handle(
        &mut self,
        action: ActionId,
        form: FormData,
        identity: &IdentityState,
    ) -> Result<Command, String> {
        if action == BACK {
            return Ok(Command::Close);
        }
        if action == LOCK {
            return Ok(Command::Lock);
        }
        if valid_role(identity, self.owner).is_none() {
            return Err("当前账号没有资料管理权限。".into());
        }
        if matches!(action, COPIES | CLEAN_COPIES | RETURN_COPIES) {
            if valid_role(identity, self.owner) != Some(Role::GlobalAdmin) {
                return Err("只有本地全局管理员可管理共享打开副本。".into());
            }
            if action == RETURN_COPIES {
                self.copies = None;
                return Ok(Command::Stay);
            }
            if action == CLEAN_COPIES && self.copies.is_none() {
                return Err("请先查看副本占用并确认清理范围。".into());
            }
            return Ok(Command::Copies {
                clean: action == CLEAN_COPIES,
            });
        }
        if self.copies.is_some() {
            return Err("请先返回资料目录。".into());
        }
        if let Some(batch) = &mut self.batch {
            if !allowed_scope(identity, self.owner, batch.scope) {
                self.batch = None;
                return Err("原批次目录权限已失效，请重新选择。".into());
            }
            match action {
                CANCEL_BATCH => {
                    self.batch = None;
                    return Ok(Command::Stay);
                }
                BATCH_REMOVE => {
                    batch.files.remove(batch.selected);
                    batch.names.remove(batch.selected);
                    if batch.files.is_empty() {
                        self.batch = None;
                    } else {
                        batch.selected = batch.selected.min(batch.files.len() - 1);
                    }
                    return Ok(Command::Stay);
                }
                BATCH_UP | BATCH_DOWN => {
                    let other = if action == BATCH_UP {
                        batch.selected.checked_sub(1)
                    } else {
                        batch
                            .selected
                            .checked_add(1)
                            .filter(|v| *v < batch.files.len())
                    }
                    .ok_or("不能继续移动。")?;
                    batch.files.swap(batch.selected, other);
                    batch.names.swap(batch.selected, other);
                    batch.selected = other;
                    return Ok(Command::Stay);
                }
                CONFIRM_BATCH => {
                    let batch = self.batch.take().ok_or("本批已失效。")?;
                    let command = NativeCommand::new();
                    self.history = Some(BatchHistory {
                        kind: BatchKind::Import,
                        delete_originals: Vec::new(),
                        scope: batch.scope,
                        names: batch.names,
                        command: command.clone(),
                        classes: batch.classes.clone(),
                        outcome: super::material_flow::BatchOutcome::Unknown,
                    });
                    return Ok(Command::ImportBatch {
                        scope: batch.scope,
                        classes: batch.classes,
                        selected: batch.files,
                        command,
                    });
                }
                _ => return Err("请先确认或取消本批资料。".into()),
            }
        }
        if action == QUERY_BATCH {
            let history = self.history.as_ref().ok_or("没有原批次可查询。")?;
            return Ok(Command::QueryBatch {
                kind: history.kind,
                scope: history.scope,
                command: history.command.clone(),
            });
        }
        if action == DISMISS_BATCH {
            if self
                .history
                .as_ref()
                .is_none_or(|h| matches!(h.outcome, super::material_flow::BatchOutcome::Unknown))
            {
                return Err("原批次结果尚未确定，请查询原票据。".into());
            }
            self.history = None;
            return Ok(Command::Stay);
        }
        if action == CANCEL_DELETE_BATCH {
            if self.delete_confirmation {
                self.delete_confirmation = false;
            } else {
                self.delete_selection.clear();
                self.delete_mode = false;
            }
            return Ok(Command::Stay);
        }
        if action == CONFIRM_DELETE_BATCH {
            if !self.delete_confirmation
                || self.history.is_some()
                || self.delete_selection.is_empty()
            {
                return Err("请先核对固定删除清单。".into());
            }
            let scope = self.scope.ok_or("资料目录已经失效。")?;
            if !allowed_scope(identity, self.owner, scope) {
                self.clear_selection();
                return Err("原删除目录权限已失效，请重新选择。".into());
            }
            let records = std::mem::take(&mut self.delete_selection);
            let command = NativeCommand::new();
            self.history = Some(BatchHistory {
                kind: BatchKind::Delete,
                scope,
                names: records
                    .iter()
                    .map(|record| record.display_name.clone())
                    .collect(),
                delete_originals: records.clone(),
                classes: Vec::new(),
                command: command.clone(),
                outcome: super::material_flow::BatchOutcome::Unknown,
            });
            self.clear_selection();
            return Ok(Command::DeleteBatch {
                scope,
                records,
                command,
            });
        }
        if self.delete_confirmation {
            return Err("请先确认整批删除或保留本批资料。".into());
        }
        if action == CANCEL_DELETE {
            self.confirmation = false;
            return Ok(Command::Stay);
        }
        if action == CONFIRM_DELETE {
            if !self.confirmation {
                return Err("请先选择资料并确认删除。".into());
            }
            let scope = self.scope.ok_or("资料目录已经失效。")?;
            if !allowed_scope(identity, self.owner, scope) {
                return Err("当前账号不能管理这个目录。".into());
            }
            let record = self.selected.as_ref().ok_or("资料选择已经失效。")?;
            let result = Command::Delete {
                scope,
                material_id: record.id,
                expected_material_revision: record.revision,
            };
            // Import/delete workers return a freshly authorized first page.
            // Reserve its exact scope/offset before dispatch, just as Load does.
            let _ = self.request(scope, None);
            return Ok(result);
        }
        if self.confirmation {
            return Err("请先确认删除或保留资料。".into());
        }
        let scope = self.scope_from(&form, identity)?;
        if action == LOAD || action == FIRST {
            return Ok(self.request(scope, None));
        }
        if action == IMPORT {
            if self.history.is_some() {
                return Err("请先核对原批次结果，不会重复导入。".into());
            }
            let available = classes(identity, scope);
            let allowed_class_ids = match form.get(CLASSES) {
                Some("all") => available.clone(),
                Some(value) => {
                    let id = value
                        .parse()
                        .ok()
                        .and_then(|v| ClassId::new(v).ok())
                        .filter(|id| available.contains(id))
                        .ok_or("请选择有效的任教班级。")?;
                    vec![id]
                }
                _ => return Err("请选择本次导入可见班级。".into()),
            };
            if allowed_class_ids.is_empty() {
                return Err("这个老师在该科目没有有效任教班级。".into());
            }
            if self.scope != Some(scope) {
                self.page = None;
                self.pending = None;
                self.clear_selection();
            }
            self.scope = Some(scope);
            return Ok(Command::Import {
                scope,
                allowed_class_ids,
            });
        }
        if self.scope != Some(scope) {
            self.scope = Some(scope);
            self.page = None;
            self.pending = None;
            self.clear_selection();
            return Err("目录已切换，请先载入目录并重新选择资料。".into());
        }
        if matches!(
            action,
            TOGGLE_DELETE | SELECT_DELETE_PAGE | CLEAR_DELETE | DELETE_BATCH
        ) {
            if self.history.is_some() {
                return Err("请先核对原批次结果，不会重复删除。".into());
            }
            let page = self.page.as_ref().ok_or("请先载入当前目录。")?;
            if action == DELETE_BATCH && !self.delete_mode {
                self.delete_mode = true;
                return Ok(Command::Stay);
            }
            if !self.delete_mode {
                return Err("请先进入批量删除并核对本页资料。".into());
            }
            match action {
                CLEAR_DELETE => self.delete_selection.clear(),
                SELECT_DELETE_PAGE => self.delete_selection = page.items.clone(),
                TOGGLE_DELETE => {
                    let record = self
                        .selected
                        .as_ref()
                        .filter(|record| page.items.contains(record))
                        .ok_or("请先选择当前页的资料。")?;
                    if let Some(index) = self
                        .delete_selection
                        .iter()
                        .position(|selected| selected == record)
                    {
                        self.delete_selection.remove(index);
                    } else if self.delete_selection.len() < MAX_MATERIAL_BATCH_FILES {
                        self.delete_selection.push(record.clone());
                    } else {
                        return Err("每批最多勾选20项资料。".into());
                    }
                }
                DELETE_BATCH => {
                    if self.delete_selection.is_empty()
                        || self.delete_selection.len() > MAX_MATERIAL_BATCH_FILES
                        || self
                            .delete_selection
                            .iter()
                            .any(|record| !page.items.contains(record))
                    {
                        return Err("勾选资料已失效，请刷新后重新选择。".into());
                    }
                    self.delete_confirmation = true;
                }
                _ => unreachable!(),
            }
            return Ok(Command::Stay);
        }
        if action == NEXT {
            let cursor = self
                .page
                .as_ref()
                .and_then(|p| p.next_cursor.clone())
                .ok_or("当前没有下一页，请刷新目录。")?;
            return Ok(self.request(scope, Some(cursor)));
        }
        if action == OPEN || action == DELETE {
            let record = self.selected.as_ref().ok_or("请先点击选择资料。")?;
            if action == DELETE {
                self.confirmation = true;
                return Ok(Command::Stay);
            }
            return Ok(Command::Open {
                scope,
                material_id: record.id,
                resource_version: record.resource.version_id,
            });
        }
        Err("未知资料操作。".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::{
        auth::{Principal, TeachingAssignment},
        identity::*,
        time::*,
    };
    fn principal(v: u64) -> PrincipalId {
        PrincipalId::new(v).unwrap()
    }
    fn scope(teacher: u64) -> MaterialScope {
        MaterialScope {
            teacher_id: principal(teacher),
            subject_id: SubjectId::new(1).unwrap(),
        }
    }
    fn identity() -> IdentityState {
        let school = SchoolId::new(1).unwrap();
        IdentityState {
            revision: Revision::new(1),
            installation: Some(Installation {
                school_id: school,
                school_name: "测试学校".into(),
                current_class: Some(ClassId::new(1).unwrap()),
            }),
            accounts: (1..=4)
                .map(|id| UserAccount {
                    principal: Principal {
                        id: principal(id),
                        school_id: school,
                        role: match id {
                            1 => Role::GlobalAdmin,
                            4 => Role::User,
                            _ => Role::TeacherAdmin,
                        },
                        enabled: true,
                        auth_epoch: AuthEpoch::new(1),
                    },
                    login_name: format!("account{id}"),
                    display_name: format!("老师{id}"),
                    must_change_password: false,
                    credential_version: 1,
                    recovery_version: 1,
                })
                .collect(),
            classes: (1..=2)
                .map(|id| SchoolClass {
                    id: ClassId::new(id).unwrap(),
                    school_id: school,
                    name: format!("{id}班"),
                    enabled: true,
                })
                .collect(),
            subjects: vec![SchoolSubject {
                id: SubjectId::new(1).unwrap(),
                school_id: school,
                name: "语文".into(),
                enabled: true,
                color: Rgb {
                    red: 20,
                    green: 40,
                    blue: 80,
                },
                revision: Revision::new(1),
            }],
            assignments: [(2, 1), (2, 2), (3, 1)]
                .into_iter()
                .map(|(teacher, class)| TeachingAssignment {
                    teacher_id: principal(teacher),
                    class_id: ClassId::new(class).unwrap(),
                    subject_id: SubjectId::new(1).unwrap(),
                    enabled: true,
                })
                .collect(),
            default_teachers: Vec::new(),
        }
    }
    fn form(teacher: u64, class: &str) -> FormData {
        let mut f = FormData::new();
        for (id, value) in [
            (TEACHER, teacher.to_string()),
            (SUBJECT, "1".into()),
            (CLASSES, class.into()),
        ] {
            f.insert(id, value).unwrap();
        }
        f
    }
    fn page(teacher: u64, offset: u64, total: u64) -> MaterialPage {
        let folder = TeacherMaterialFolder {
            id: FolderId::new(teacher).unwrap(),
            scope: scope(teacher),
            revision: Revision::new(30),
            last_added_sequence: 30,
        };
        let end = (offset + 20).min(total);
        let items = (offset..end)
            .map(|i| MaterialRecord {
                id: MaterialId::new(100 + i).unwrap(),
                folder_id: folder.id,
                display_name: format!("完整资料文件名-{i}.txt"),
                resource: MaterialResource {
                    version_id: ResourceVersionId::new(1000 + i).unwrap(),
                    sha256: [7; 32],
                    kind: MaterialKind::Txt,
                    bytes: 10,
                },
                allowed_class_ids: vec![ClassId::new(1).unwrap()],
                added_at: LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 8 * 3600)
                    .unwrap(),
                added_sequence: 30 - i,
                status: MaterialStatus::Published,
                revision: Revision::new(1),
            })
            .collect();
        let next_cursor = (end < total).then_some(MaterialCursor {
            folder_id: folder.id,
            folder_revision: folder.revision,
            scope: folder.scope,
            class_id: None,
            source: None,
            offset: end,
        });
        MaterialPage {
            folder,
            source: None,
            class_id: None,
            offset,
            total,
            items,
            next_cursor,
        }
    }
    fn load(e: &mut Editor, identity: &IdentityState, teacher: u64, page: MaterialPage) {
        assert!(matches!(
            e.handle(LOAD, form(teacher, "all"), identity).unwrap(),
            Command::Load(_)
        ));
        page.validate(identity, 20).unwrap();
        e.loaded(page).unwrap();
    }
    #[test]
    fn delete_confirmation_and_open_capture_fixed_selected_record_and_version() {
        let state = identity();
        state.validate().unwrap();
        let mut e = Editor::new(principal(1));
        let p = page(2, 0, 3);
        let selected = p.items[1].clone();
        load(&mut e, &state, 2, p);
        e.select(1, 2).unwrap();
        let Command::Open {
            scope: actual,
            material_id,
            resource_version,
        } = e.handle(OPEN, form(2, "all"), &state).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            (actual, material_id, resource_version),
            (scope(2), selected.id, selected.resource.version_id)
        );
        assert!(matches!(
            e.handle(DELETE, form(2, "all"), &state).unwrap(),
            Command::Stay
        ));
        assert!(e.select(0, 0).is_err());
        e.view(&state, "", false).validate().unwrap();
        let Command::Delete {
            scope: actual,
            material_id,
            expected_material_revision,
        } = e.handle(CONFIRM_DELETE, form(3, "all"), &state).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            (actual, material_id, expected_material_revision),
            (scope(2), selected.id, selected.revision)
        );
        assert!(e.handle(CONFIRM_DELETE, FormData::new(), &state).is_err());
        let mut remaining = page(2, 0, 1);
        remaining.folder.revision = Revision::new(31);
        e.loaded(remaining).unwrap();
    }
    #[test]
    fn switching_folder_clears_selection_and_rejects_late_old_folder_results() {
        let state = identity();
        let mut e = Editor::new(principal(1));
        load(&mut e, &state, 2, page(2, 0, 1));
        e.select(0, 0).unwrap();
        assert!(e.handle(OPEN, form(3, "all"), &state).is_err());
        assert!(e.selected.is_none());
        assert!(e.loaded(page(2, 0, 1)).is_err());
        assert!(matches!(
            e.handle(LOAD, form(3, "all"), &state).unwrap(),
            Command::Load(_)
        ));
        assert!(e.loaded(page(2, 0, 1)).is_err());
        e.loaded(page(3, 0, 1)).unwrap();
        assert!(e.handle(DELETE, form(3, "all"), &state).is_err());
        e.view(&state, "", false).validate().unwrap();
    }
    #[test]
    fn next_page_uses_exact_backend_cursor_and_refresh_resets_to_first_page() {
        let state = identity();
        let mut e = Editor::new(principal(2));
        let first = page(2, 0, 23);
        let expected = first.next_cursor.clone();
        load(&mut e, &state, 2, first);
        e.view(&state, "", false).validate().unwrap();
        let Command::Load(request) = e.handle(NEXT, form(2, "all"), &state).unwrap() else {
            panic!()
        };
        assert_eq!(request.cursor, expected);
        assert!(e.loaded(page(2, 0, 23)).is_err());
        e.loaded(page(2, 20, 23)).unwrap();
        e.view(&state, "", false).validate().unwrap();
        assert!(e.handle(NEXT, form(2, "all"), &state).is_err());
        let Command::Load(request) = e.handle(FIRST, form(2, "all"), &state).unwrap() else {
            panic!()
        };
        assert!(request.cursor.is_none());
        assert!(e.selected.is_none());
    }
    #[test]
    fn teacher_scope_and_visible_classes_are_stable_choices_and_user_has_no_entry() {
        let state = identity();
        let mut e = Editor::new(principal(2));
        assert!(e.handle(LOAD, form(3, "all"), &state).is_err());
        let Command::Import {
            scope: actual,
            allowed_class_ids,
        } = e.handle(IMPORT, form(2, "all"), &state).unwrap()
        else {
            panic!()
        };
        assert_eq!(actual, scope(2));
        load(&mut e, &state, 2, page(2, 0, 1));
        assert_eq!(
            allowed_class_ids,
            vec![ClassId::new(1).unwrap(), ClassId::new(2).unwrap()]
        );
        let Command::Import {
            allowed_class_ids, ..
        } = e.handle(IMPORT, form(2, "2"), &state).unwrap()
        else {
            panic!()
        };
        assert_eq!(allowed_class_ids, vec![ClassId::new(2).unwrap()]);
        assert!(e.handle(IMPORT, form(2, "999"), &state).is_err());
        e.view(&state, "", false).validate().unwrap();
        let mut user = Editor::new(principal(4));
        let view = user.view(&state, "", false);
        assert!(view.grid.is_none());
        assert!(!view.actions.iter().any(|a| a.id == LOAD || a.id == IMPORT));
        assert!(user.handle(IMPORT, form(4, "all"), &state).is_err());
        assert!(matches!(
            user.handle(BACK, FormData::new(), &state),
            Ok(Command::Close)
        ));
        assert!(matches!(
            user.handle(LOCK, FormData::new(), &state),
            Ok(Command::Lock)
        ));
    }
    #[test]
    fn copy_cleanup_requires_global_role_and_inspection_even_without_material_scopes() {
        let mut state = identity();
        let mut global = Editor::new(principal(1));
        assert!(global
            .handle(CLEAN_COPIES, FormData::new(), &state)
            .is_err());
        state.subjects.clear();
        assert!(global
            .view(&state, "", false)
            .actions
            .iter()
            .any(|a| a.id == COPIES));
        assert!(matches!(
            global.handle(COPIES, FormData::new(), &state),
            Ok(Command::Copies { clean: false })
        ));
        global.copies_loaded(super::super::material_flow::LaunchCopyReport {
            count: 3,
            bytes: 100,
            expired: 2,
            removed: 0,
            retained: 0,
        });
        global.view(&state, "", false).validate().unwrap();
        assert!(matches!(
            global.handle(CLEAN_COPIES, FormData::new(), &state),
            Ok(Command::Copies { clean: true })
        ));
        for id in [2, 4] {
            let mut other = Editor::new(principal(id));
            other.copies_loaded(global.copies.unwrap());
            assert!(other.handle(COPIES, FormData::new(), &state).is_err());
            assert!(other.handle(CLEAN_COPIES, FormData::new(), &state).is_err());
        }
        assert!(matches!(
            global.handle(RETURN_COPIES, FormData::new(), &state),
            Ok(Command::Stay)
        ));
        assert!(global.copies.is_none());
    }
    fn selected(name: &str) -> SelectedFile {
        SelectedFile::isolated_probe(
            std::path::Path::new(r"C:\FutureClassroom-Test-batch-editor"),
            name,
        )
        .unwrap()
    }
    #[test]
    fn batch_confirmation_preserves_reordered_manifest_and_last_item_is_newest() {
        let state = identity();
        let mut editor = Editor::new(principal(2));
        editor
            .stage_batch(
                scope(2),
                vec![ClassId::new(1).unwrap()],
                vec![
                    selected("first.txt"),
                    selected("remove.txt"),
                    selected("last.txt"),
                ],
            )
            .unwrap();
        editor.view(&state, "", false).validate().unwrap();
        editor.select(1, 0).unwrap();
        editor
            .handle(BATCH_REMOVE, FormData::new(), &state)
            .unwrap();
        editor.select(1, 0).unwrap();
        editor.handle(BATCH_UP, FormData::new(), &state).unwrap();
        let view = editor.view(&state, "", false);
        assert_eq!(view.grid.as_ref().unwrap().rows[0].label, "last.txt");
        assert!(view.grid.as_ref().unwrap().rows[1].cells[0].contains("本批最新"));
        let Command::ImportBatch {
            selected,
            classes,
            command,
            ..
        } = editor
            .handle(CONFIRM_BATCH, FormData::new(), &state)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            selected
                .iter()
                .map(|file| file.display_name().unwrap())
                .collect::<Vec<_>>(),
            vec!["last.txt", "first.txt"]
        );
        assert_eq!(classes, vec![ClassId::new(1).unwrap()]);
        assert!(
            command.snapshot().is_none(),
            "confirmation itself does not issue on the UI thread"
        );
        assert!(
            editor
                .handle(CONFIRM_BATCH, FormData::new(), &state)
                .is_err(),
            "cannot dispatch the same confirmed list twice"
        );
        assert!(
            editor
                .handle(DISMISS_BATCH, FormData::new(), &state)
                .is_err(),
            "unknown is not a completed batch"
        );
        editor.view(&state, "", false).validate().unwrap();
    }
    #[test]
    fn wrong_original_batch_is_rejected_and_current_directory_never_resolves_history() {
        let state = identity();
        let mut editor = Editor::new(principal(2));
        editor
            .stage_batch(
                scope(2),
                vec![ClassId::new(1).unwrap()],
                vec![selected("one.txt")],
            )
            .unwrap();
        let Command::ImportBatch { command, .. } = editor
            .handle(CONFIRM_BATCH, FormData::new(), &state)
            .unwrap()
        else {
            panic!()
        };
        assert!(editor
            .batch_result(super::super::material_flow::BatchResult {
                kind: BatchKind::Import,
                scope: scope(2),
                command: NativeCommand::new(),
                issued: None,
                outcome: super::super::material_flow::BatchOutcome::NotSubmitted(
                    "another worker".into()
                ),
            })
            .is_err());
        load(&mut editor, &state, 2, page(2, 0, 1));
        editor.select(0, 0).unwrap();
        editor.view(&state, "", false).validate().unwrap();
        assert!(matches!(
            editor.history.as_ref().unwrap().outcome,
            super::super::material_flow::BatchOutcome::Unknown
        ));
        let Command::QueryBatch {
            command: queried, ..
        } = editor.handle(QUERY_BATCH, FormData::new(), &state).unwrap()
        else {
            panic!()
        };
        assert_eq!(queried, command);
        editor
            .batch_result(super::super::material_flow::BatchResult {
                kind: BatchKind::Import,
                scope: scope(2),
                command,
                issued: None,
                outcome: super::super::material_flow::BatchOutcome::NotSubmitted(
                    "worker did not enter Writer".into(),
                ),
            })
            .unwrap();
        editor
            .handle(DISMISS_BATCH, FormData::new(), &state)
            .unwrap();
        assert!(editor.history.is_none());
    }
    #[test]
    fn empty_oversized_and_revoked_batch_drafts_cannot_be_confirmed() {
        let mut editor = Editor::new(principal(2));
        assert!(editor
            .stage_batch(scope(2), vec![ClassId::new(1).unwrap()], Vec::new())
            .is_err());
        assert!(editor
            .stage_batch(
                scope(2),
                vec![ClassId::new(1).unwrap()],
                (0..=MAX_MATERIAL_BATCH_FILES)
                    .map(|_| selected("one.txt"))
                    .collect()
            )
            .is_err());
        editor
            .stage_batch(
                scope(2),
                vec![ClassId::new(1).unwrap()],
                vec![selected("one.txt")],
            )
            .unwrap();
        let mut state = identity();
        state.assignments.clear();
        assert!(editor
            .handle(CONFIRM_BATCH, FormData::new(), &state)
            .is_err());
        assert!(editor.batch.is_none());
        assert!(editor.history.is_none());
    }
    #[test]
    fn bulk_delete_selection_is_fixed_and_clears_when_page_or_scope_changes() {
        let state = identity();
        let mut editor = Editor::new(principal(1));
        load(&mut editor, &state, 2, page(2, 0, 23));
        editor.handle(DELETE_BATCH, form(2, "all"), &state).unwrap();
        editor.select(1, 0).unwrap();
        editor
            .handle(TOGGLE_DELETE, form(2, "all"), &state)
            .unwrap();
        assert_eq!(editor.delete_selection[0].id, MaterialId::new(101).unwrap());
        editor
            .handle(TOGGLE_DELETE, form(2, "all"), &state)
            .unwrap();
        assert!(editor.delete_selection.is_empty());
        editor
            .handle(SELECT_DELETE_PAGE, form(2, "all"), &state)
            .unwrap();
        assert_eq!(editor.delete_selection.len(), 20);
        editor.view(&state, "", false).validate().unwrap();
        assert!(matches!(
            editor.handle(NEXT, form(2, "all"), &state).unwrap(),
            Command::Load(_)
        ));
        assert!(editor.delete_selection.is_empty());
        editor.loaded(page(2, 20, 23)).unwrap();
        editor.handle(DELETE_BATCH, form(2, "all"), &state).unwrap();
        editor
            .handle(SELECT_DELETE_PAGE, form(2, "all"), &state)
            .unwrap();
        assert_eq!(editor.delete_selection[0].id, MaterialId::new(120).unwrap());
        assert!(editor
            .handle(TOGGLE_DELETE, form(3, "all"), &state)
            .is_err());
        assert!(editor.delete_selection.is_empty());
        assert!(!editor.delete_confirmation);
    }
    #[test]
    fn bulk_delete_confirmation_keeps_versions_and_uses_distinct_history_kind() {
        use super::super::material_flow::{BatchOutcome, BatchResult};
        let state = identity();
        let mut editor = Editor::new(principal(1));
        let original = page(2, 0, 3);
        let expected = original.items.clone();
        load(&mut editor, &state, 2, original);
        editor.handle(DELETE_BATCH, form(2, "all"), &state).unwrap();
        editor
            .handle(SELECT_DELETE_PAGE, form(2, "all"), &state)
            .unwrap();
        editor.handle(DELETE_BATCH, form(2, "all"), &state).unwrap();
        editor.view(&state, "", false).validate().unwrap();
        assert!(editor.select(0, 0).is_err());
        assert!(editor.handle(NEXT, form(2, "all"), &state).is_err());
        let Command::DeleteBatch {
            scope: captured,
            records,
            command,
        } = editor
            .handle(CONFIRM_DELETE_BATCH, form(3, "all"), &state)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(captured, scope(2));
        assert_eq!(records, expected);
        assert!(command.snapshot().is_none());
        assert!(editor
            .handle(CONFIRM_DELETE_BATCH, FormData::new(), &state)
            .is_err());
        assert!(editor
            .batch_result(BatchResult {
                kind: BatchKind::Import,
                scope: captured,
                command: command.clone(),
                issued: None,
                outcome: BatchOutcome::NotSubmitted("wrong kind".into())
            })
            .is_err());
        let Command::QueryBatch {
            kind,
            command: queried,
            ..
        } = editor.handle(QUERY_BATCH, FormData::new(), &state).unwrap()
        else {
            panic!()
        };
        assert_eq!(kind, BatchKind::Delete);
        assert_eq!(queried, command);
        editor
            .batch_result(BatchResult {
                kind: BatchKind::Delete,
                scope: captured,
                command,
                issued: None,
                outcome: BatchOutcome::NotSubmitted(
                    "actual worker returned before issuance".into(),
                ),
            })
            .unwrap();
        editor
            .handle(DISMISS_BATCH, FormData::new(), &state)
            .unwrap();
        editor.view(&state, "", false).validate().unwrap();
    }
}
