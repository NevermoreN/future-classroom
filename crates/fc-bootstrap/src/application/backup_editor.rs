//! Native backup/recovery review state. This editor owns text and intent only;
//! the coordinator owns every pinned file and the opaque preflight capability.
use fc_domain::ids::{PrincipalId, Revision, SchoolId};
use fc_presentation::workbench::*;

#[cfg(test)]
#[path = "backup_editor/tests.rs"]
mod tests;

pub(super) const EXPORT: ActionId = ActionId(670);
pub(super) const SELECT_RESTORE: ActionId = ActionId(671);
pub(super) const CONFIRM: ActionId = ActionId(672);
pub(super) const CANCEL: ActionId = ActionId(673);
pub(super) const CLOSE: ActionId = ActionId(674);
pub(super) const PREVIOUS_PATH: ActionId = ActionId(675);
pub(super) const NEXT_PATH: ActionId = ActionId(676);
pub(super) const SAVE_COPY: ActionId = ActionId(677);
pub(super) const ACK: FieldId = FieldId(670);
const SELECTED_PATH: FieldId = FieldId(671);
const VERSIONS: FieldId = FieldId(672);
const CONTENTS: FieldId = FieldId(673);
const PATH_PAGE: FieldId = FieldId(674);
const PATH_PAGE_UNITS: usize = 1024;
const MAX_PATH_UNITS: usize = 32767;

pub(super) enum Command {
    Stay,
    Close,
    Export,
    SelectRestore,
    SaveCopy,
    Cancel,
    Confirm { sequence: u64 },
}

pub(super) struct ReviewSummary {
    pub current_revision: Revision,
    pub source_revision: Revision,
    pub school_id: SchoolId,
    pub document_count: u64,
    pub material_count: u64,
    pub material_bytes: u64,
    pub profile_media_count: u64,
    pub profile_media_bytes: u64,
    pub database_bytes: u64,
    pub selected_file: String,
}

struct Review {
    sequence: u64,
    summary: ReviewSummary,
    path: DisplayPath,
}
struct ExportResult {
    bytes: u64,
    path: DisplayPath,
}
enum Stage {
    Home,
    Pending(u64),
    Review(Review),
    Confirmed,
    Invalid,
}
struct DisplayPath {
    pages: Vec<String>,
    page: usize,
}
fn unsafe_display_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}
fn status_text(value: &str) -> String {
    value
        .chars()
        .take(2048)
        .map(|c| {
            if unsafe_display_char(c) {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}
impl DisplayPath {
    fn new(path: &str) -> Result<Self, String> {
        if path.is_empty()
            || path.encode_utf16().count() > MAX_PATH_UNITS
            || path.chars().any(unsafe_display_char)
        {
            return Err("文件位置无法安全完整显示，请选择可读的文件名或目录。".into());
        }
        let mut pages = Vec::new();
        let mut page = String::new();
        let mut units = 0;
        for c in path.chars() {
            if units + c.len_utf16() > PATH_PAGE_UNITS {
                pages.push(std::mem::take(&mut page));
                units = 0;
            }
            units += c.len_utf16();
            page.push(c);
        }
        pages.push(page);
        Ok(Self { pages, page: 0 })
    }
    fn fields(&self, label: &str) -> Vec<Field> {
        vec![
            Field {
                id: SELECTED_PATH,
                label: label.into(),
                kind: FieldKind::ReadOnly,
                value: self.pages[self.page].clone(),
                max_utf16: PATH_PAGE_UNITS,
            },
            Field {
                id: PATH_PAGE,
                label: "文件位置显示页（按顺序拼接即为完整位置）".into(),
                kind: FieldKind::ReadOnly,
                value: format!("{} / {}", self.page + 1, self.pages.len()),
                max_utf16: 64,
            },
        ]
    }
}

pub(super) struct Editor {
    owner: PrincipalId,
    last_sequence: u64,
    stage: Stage,
    exported: Option<ExportResult>,
}
impl Editor {
    pub fn new(owner: PrincipalId) -> Self {
        Self {
            owner,
            last_sequence: 0,
            stage: Stage::Home,
            exported: None,
        }
    }
    pub fn owner(&self) -> PrincipalId {
        self.owner
    }
    pub fn has_unsaved_changes(&self) -> bool {
        matches!(self.stage, Stage::Pending(_) | Stage::Review(_))
    }
    pub fn invalidate(&mut self) {
        self.stage = Stage::Invalid;
        self.exported = None;
    }
    pub fn cancel_review(&mut self) {
        if matches!(self.stage, Stage::Pending(_) | Stage::Review(_)) {
            self.stage = Stage::Home;
        }
    }
    pub fn begin_review(&mut self, sequence: u64) -> Result<(), String> {
        if !matches!(self.stage, Stage::Home) || sequence <= self.last_sequence {
            return Err("当前不能开始此备份复核，请重新选择备份。".into());
        }
        self.last_sequence = sequence;
        self.stage = Stage::Pending(sequence);
        self.exported = None;
        Ok(())
    }
    pub fn set_review(
        &mut self,
        owner: PrincipalId,
        sequence: u64,
        summary: ReviewSummary,
    ) -> Result<(), String> {
        if owner != self.owner || !matches!(self.stage, Stage::Pending(s) if s == sequence) {
            return Err("此备份复核已失效，请重新选择备份。".into());
        }
        let path = match DisplayPath::new(&summary.selected_file) {
            Ok(path) => path,
            Err(error) => {
                self.stage = Stage::Home;
                return Err(error);
            }
        };
        self.stage = Stage::Review(Review {
            sequence,
            summary,
            path,
        });
        Ok(())
    }
    pub fn exported(&mut self, path: String, bytes: u64) -> Result<(), String> {
        if !matches!(self.stage, Stage::Home) {
            return Err("此导出结果已不属于当前页面。".into());
        }
        self.exported = Some(ExportResult {
            bytes,
            path: DisplayPath::new(&path)?,
        });
        Ok(())
    }
    fn path_mut(&mut self) -> Option<&mut DisplayPath> {
        match &mut self.stage {
            Stage::Review(review) => Some(&mut review.path),
            Stage::Home => self.exported.as_mut().map(|r| &mut r.path),
            _ => None,
        }
    }
    pub fn view(&self, status: &str, busy: bool) -> View {
        let enabled = !busy;
        let mut description = "完整备份包含本机学校已保存的教学数据、资料和图片原件。备份不包含密码、登录会话、语音文件及语音队列。请妥善保管备份。".to_string();
        let mut fields = Vec::new();
        let action = |id, label: &str, enabled| Action {
            id,
            label: label.into(),
            enabled,
        };
        let mut displayed_path = None;
        let mut actions = match &self.stage {
            Stage::Home => {
                if let Some(exported) = &self.exported {
                    description.push_str(&format!(
                        "\n备份已导出，共 {} 字节。下方完整位置可分页查看；请保留该文件。",
                        exported.bytes
                    ));
                    fields.extend(exported.path.fields("已导出备份的完整位置（当前页）"));
                    displayed_path = Some(&exported.path);
                }
                let mut actions = vec![
                    action(EXPORT, "导出完整备份", enabled),
                    action(SELECT_RESTORE, "选择备份并预检", enabled),
                    action(CLOSE, "返回工作台", enabled),
                ];
                if self.exported.is_some() {
                    actions.insert(1, action(SAVE_COPY, "另存备份到…", enabled));
                }
                if busy {
                    actions.push(action(CANCEL, "取消当前备份任务", true));
                }
                actions
            }
            Stage::Pending(_) => {
                description.push_str("\n正在预检所选备份；尚未修改正式教学数据。取消会丢弃本次复核。只有预检成功后才能明确确认恢复。");
                vec![action(CANCEL, "取消预检并返回", true)]
            }
            Stage::Review(review) => {
                let s = &review.summary;
                // School/install matching is enforced by preflight; the numeric
                // school identifier is not useful decision-making information.
                let _school = s.school_id;
                description.push_str("\n所选备份已通过本机学校一致性及完整性预检。恢复将替换教学学校、班级、科目、课表、校历、作业和提醒、布局、教学资料及当前版和上一版展示内容。\n保留当前密码、登录限速、账号停用状态、角色和任教权限，以及当前系统状态。不会恢复语音队列、语音文件或旧会话。\n确认前可取消，正式资料保持不变。未保存的修改不纳入备份，并会在恢复关闭时丢弃。确认后进入受控恢复，只允许安全中止或退出，核对结果会保留；程序随后关闭，必须重新启动并登录。恢复中断后，下次启动将先核对恢复状态。请先核对所选文件和下方数量，再明确确认。");
                fields.extend(review.path.fields("所选备份的完整位置（当前页）"));
                displayed_path = Some(&review.path);
                fields.push(Field {
                    id: VERSIONS,
                    label: "数据版本（修订号）".into(),
                    kind: FieldKind::ReadOnly,
                    value: format!(
                        "当前：{}；备份：{}",
                        s.current_revision.get(),
                        s.source_revision.get()
                    ),
                    max_utf16: 128,
                });
                fields.push(Field {
                    id: CONTENTS,
                    label: "预检确认的数据范围".into(),
                    kind: FieldKind::ReadOnly,
                    value: format!("教学记录：{}；数据库：{} 字节；资料：{} 份 / {} 字节；图片原件：{} 份 / {} 字节", s.document_count, s.database_bytes, s.material_count, s.material_bytes, s.profile_media_count, s.profile_media_bytes),
                    max_utf16: 512,
                });
                fields.push(Field {
                    id: ACK,
                    label: "确认恢复影响与重新启动要求".into(),
                    kind: FieldKind::Choice(vec![
                        Choice {
                            value: "0".into(),
                            label: "尚未确认".into(),
                        },
                        Choice {
                            value: format!("restore-{}", review.sequence),
                            label: "已核对备份并理解上述影响，确认本次恢复".into(),
                        },
                    ]),
                    value: "0".into(),
                    max_utf16: 32,
                });
                vec![
                    action(CONFIRM, "确认恢复并关闭程序", enabled),
                    action(CANCEL, "取消恢复并返回", enabled),
                    action(SELECT_RESTORE, "重新选择备份", enabled),
                    action(CLOSE, "返回工作台并丢弃复核", enabled),
                ]
            }
            Stage::Confirmed => {
                description.push_str("\n本次确认已交给恢复协调器，请等待恢复进度与核对结果。此页不能重复提交或返回修改教学数据。");
                Vec::new()
            }
            Stage::Invalid => {
                description.push_str(
                    "\n此页面已因锁定、身份变化或数据变化失效，请重新登录后打开备份与恢复。",
                );
                Vec::new()
            }
        };
        if let Some(path) = displayed_path {
            if path.pages.len() > 1 {
                description
                    .push_str("\n文件位置分为多页，未省略任何字符；使用上一页、下一页逐页核对。");
                actions.push(action(
                    PREVIOUS_PATH,
                    "文件位置上一页",
                    enabled && path.page > 0,
                ));
                actions.push(action(
                    NEXT_PATH,
                    "文件位置下一页",
                    enabled && path.page + 1 < path.pages.len(),
                ));
            }
        }
        View {
            title: "完整备份与恢复".into(),
            description,
            status: status_text(status),
            fields,
            actions,
            busy,
            grid: None,
        }
    }
    pub fn event(&mut self, action: ActionId, data: FormData) -> Result<Command, String> {
        if matches!(self.stage, Stage::Invalid | Stage::Confirmed) {
            return Err("此备份页面已失效，不能重复提交。".into());
        }
        match action {
            CONFIRM => {
                let Stage::Review(review) = &self.stage else {
                    return Err("当前没有有效的已预检备份。".into());
                };
                let expected_ack = format!("restore-{}", review.sequence);
                if data.get(ACK) != Some(expected_ack.as_str()) {
                    return Err("请先核对本次备份和恢复影响，再明确确认本次恢复。".into());
                }
                let sequence = review.sequence;
                self.stage = Stage::Confirmed;
                Ok(Command::Confirm { sequence })
            }
            CANCEL => {
                self.stage = Stage::Home;
                Ok(Command::Cancel)
            }
            CLOSE => {
                self.stage = Stage::Invalid;
                self.exported = None;
                Ok(Command::Close)
            }
            SELECT_RESTORE => {
                self.stage = Stage::Home;
                self.exported = None;
                Ok(Command::SelectRestore)
            }
            EXPORT if matches!(self.stage, Stage::Home) => Ok(Command::Export),
            SAVE_COPY if matches!(self.stage, Stage::Home) && self.exported.is_some() => {
                Ok(Command::SaveCopy)
            }
            PREVIOUS_PATH | NEXT_PATH => {
                if let Some(path) = self.path_mut() {
                    if action == PREVIOUS_PATH {
                        path.page = path.page.saturating_sub(1);
                    } else if path.page + 1 < path.pages.len() {
                        path.page += 1;
                    }
                }
                Ok(Command::Stay)
            }
            _ => Ok(Command::Stay),
        }
    }
}
