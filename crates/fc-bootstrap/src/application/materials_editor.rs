//! Path-free material workbench. Every selection captures immutable record IDs;
//! the backend rechecks the live lease, scope and revision in its writer transaction.
use fc_domain::{auth::Role, identity::IdentityState, ids::*, materials::*};
use fc_ports::MaterialListRequest;
use fc_presentation::workbench::*;

pub(super) const TEACHER: FieldId = FieldId(300);
pub(super) const SUBJECT: FieldId = FieldId(301);
pub(super) const CLASSES: FieldId = FieldId(302);
const FULL_NAME: FieldId = FieldId(303);
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

pub(super) struct Editor {
    owner: PrincipalId,
    scope: Option<MaterialScope>,
    page: Option<MaterialPage>,
    pending: Option<MaterialListRequest>,
    selected: Option<MaterialRecord>,
    confirmation: bool,
    copies: Option<super::material_flow::LaunchCopyReport>,
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
            copies: None,
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
                "资料按成功加入顺序从新到旧排列。本页{}项，共{}项（从第{}项起）。",
                page.items.len(),
                page.total,
                if page.total == 0 { 0 } else { page.offset + 1 }
            );
            view.grid = Some(Grid {
                columns: vec!["文件名".into(), "加入时间".into(), "大小".into()],
                rows: page
                    .items
                    .iter()
                    .map(|r| GridRow {
                        group: String::new(),
                        label: r.display_name.clone(),
                        cells: vec![time_text(r.added_at), format!("{} 字节", r.resource.bytes)],
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
            action(IMPORT, "导入资料", !busy && !current_classes.is_empty()),
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
        if self.confirmation || column >= 3 {
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
            let _ = self.request(scope, None);
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
        e.loaded(page(2, 0, 1)).unwrap();
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
}
