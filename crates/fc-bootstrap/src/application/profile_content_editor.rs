//! Local GlobalAdmin UI draft only. Persistence and file selection authorization
//! are checked again by the worker/Backend; no path or resource id text input.
use fc_bootstrap::profile_content_files::{ImagePageInput, LocalContentInput};
use fc_domain::{desktop::ComponentKind, ids::*, profile_content::*};
use fc_presentation::workbench::*;
use fc_windows::materials::chooser::SelectedFile;
pub(super) const TEXT: FieldId = FieldId(800);
pub(super) const REVIEW: ActionId = ActionId(800);
pub(super) const CONFIRM: ActionId = ActionId(801);
pub(super) const CANCEL_REVIEW: ActionId = ActionId(802);
pub(super) const CLEAR: ActionId = ActionId(803);
pub(super) const UNDO: ActionId = ActionId(804);
pub(super) const RELOAD: ActionId = ActionId(805);
pub(super) const CLOSE: ActionId = ActionId(806);
pub(super) const PICK_TXT: ActionId = ActionId(807);
pub(super) const PICK_IMAGES: ActionId = ActionId(808);
pub(super) const REPLACE_IMAGES: ActionId = ActionId(809);
pub(super) const UP: ActionId = ActionId(810);
pub(super) const DOWN: ActionId = ActionId(811);
pub(super) const REMOVE: ActionId = ActionId(812);
pub(super) const DISCARD: ActionId = ActionId(813);
pub(super) const KEEP: ActionId = ActionId(814);
pub(super) enum Command {
    Stay,
    Close,
    Reload,
    PickTxt,
    PickImages,
    Submit {
        slot: ContentSlotKey,
        expected_revision: Revision,
        operation: ContentOperation,
        input: LocalContentInput,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Edit,
    Review(ContentOperation),
    Discard(bool),
}
pub(super) struct Editor {
    slot: ContentSlotKey,
    kind: ComponentKind,
    document: Option<ProfileContentDocument>,
    text: String,
    txt: Option<SelectedFile>,
    pages: Vec<ImagePageInput>,
    selected: Option<usize>,
    replace_next: bool,
    phase: Phase,
    cleanup_pending: bool,
    submitted: bool,
    needs_reload: bool,
    reload_expected: bool,
    notice: String,
    restore_text: bool,
    restore_images: bool,
    submitted_page_count: usize,
    submitted_txt: bool,
}
fn action(id: ActionId, label: &str, enabled: bool) -> Action {
    Action {
        id,
        label: label.into(),
        enabled,
    }
}
fn short(s: &str) -> String {
    let mut c = s.chars();
    let text: String = c.by_ref().take(180).collect();
    if c.next().is_some() {
        format!("{text}…")
    } else {
        text
    }
}
impl Editor {
    pub(super) fn new(slot: ContentSlotKey, kind: ComponentKind) -> Result<Self, String> {
        if !matches!(
            kind,
            ComponentKind::SubjectText | ComponentKind::SubjectImage
        ) {
            return Err("请选择独立文字或独立图片节点。".into());
        }
        Ok(Self {
            slot,
            kind,
            document: None,
            text: String::new(),
            txt: None,
            pages: vec![],
            selected: None,
            replace_next: false,
            phase: Phase::Edit,
            cleanup_pending: false,
            submitted: false,
            needs_reload: false,
            reload_expected: false,
            notice: String::new(),
            restore_text: false,
            restore_images: false,
            submitted_page_count: 0,
            submitted_txt: false,
        })
    }
    pub(super) fn loaded(
        &mut self,
        doc: ProfileContentDocument,
        cleanup_pending: bool,
    ) -> Result<(), String> {
        doc.validate_structure()
            .map_err(|_| "内容记录无效，请重新读取。")?;
        if doc.slot != self.slot
            || [doc.current.as_ref(), doc.previous.as_ref()]
                .into_iter()
                .flatten()
                .any(|v| v.payload.kind().is_some_and(|k| k != self.payload_kind()))
        {
            return Err("后台返回了其他节点或类型的内容。".into());
        }
        if self.document.is_some() && self.is_dirty() && !self.submitted && !self.reload_expected {
            return Err("已有未保存草稿，不能被后台读取覆盖。".into());
        }
        let preserve = self.needs_reload;
        let retained_text = (preserve && self.restore_text).then(|| self.text.clone());
        let retained_pages =
            if preserve && self.restore_images && self.kind == ComponentKind::SubjectImage {
                Some(std::mem::take(&mut self.pages))
            } else {
                None
            };
        self.text = match doc.current.as_ref().map(|v| &v.payload) {
            Some(ContentPayload::Text(t)) => t.clone(),
            _ => String::new(),
        };
        self.pages = match doc.current.as_ref().map(|v| &v.payload) {
            Some(ContentPayload::Images(a)) => {
                a.iter().cloned().map(ImagePageInput::Existing).collect()
            }
            _ => vec![],
        };
        self.document = Some(doc);
        self.txt = None;
        self.selected = None;
        self.replace_next = false;
        self.phase = Phase::Edit;
        self.cleanup_pending = cleanup_pending;
        self.submitted = false;
        self.needs_reload = false;
        self.reload_expected = false;
        self.restore_text = false;
        self.restore_images = false;
        self.submitted_page_count = 0;
        self.submitted_txt = false;
        self.notice = String::new();
        if let Some(text) = retained_text {
            self.text = text;
            self.notice = "已核对后台修订，文字草稿保留；请重新查看发布摘要。".into();
        }
        if let Some(pages) = retained_pages {
            self.pages = pages;
            self.notice = "已核对后台修订，既有页面顺序保留；已交后台的新图片需重新选择。".into();
        }
        Ok(())
    }
    pub(super) fn slot(&self) -> ContentSlotKey {
        self.slot
    }
    fn payload_kind(&self) -> ContentPayloadKind {
        if self.kind == ComponentKind::SubjectText {
            ContentPayloadKind::Text
        } else {
            ContentPayloadKind::Images
        }
    }
    pub(super) fn is_dirty(&self) -> bool {
        if self.submitted || self.needs_reload {
            return true;
        }
        if self.txt.is_some() {
            return true;
        }
        let Some(d) = &self.document else {
            return !self.text.is_empty() || !self.pages.is_empty();
        };
        if self.kind == ComponentKind::SubjectText {
            let old = match d.current.as_ref().map(|v| &v.payload) {
                Some(ContentPayload::Text(t)) => t.as_str(),
                _ => "",
            };
            self.text != old
        } else {
            let old = match d.current.as_ref().map(|v| &v.payload) {
                Some(ContentPayload::Images(a)) => a.as_slice(),
                _ => &[],
            };
            self.pages.len() != old.len()
                || self
                    .pages
                    .iter()
                    .zip(old)
                    .any(|(p, r)| !matches!(p,ImagePageInput::Existing(actual)if actual==r))
        }
    }
    pub(super) fn cancel_review(&mut self) {
        if self.submitted {
            self.needs_reload = true;
            self.notice =
                "提交结果需重新读取核对；已交给后台的文件选择需重新选择。文字草稿保留。".into();
        }
        self.submitted = false;
        self.phase = Phase::Edit;
    }
    fn editable(&self) -> Result<(), String> {
        if self.document.is_none() {
            return Err("请先读取此节点的已保存内容。".into());
        }
        if self.submitted || self.needs_reload {
            return Err("请先重新读取并核对上次提交结果。".into());
        }
        Ok(())
    }
    pub(super) fn accept_txt(&mut self, file: SelectedFile) -> Result<(), String> {
        self.editable()?;
        if self.kind != ComponentKind::SubjectText {
            return Err("此节点不接受文字文件。".into());
        }
        if !file
            .display_name()
            .map_err(|_| "文件名无效。")?
            .to_ascii_lowercase()
            .ends_with(".txt")
        {
            return Err("请选择TXT文字文件。".into());
        }
        self.txt = Some(file);
        self.phase = Phase::Edit;
        self.notice = "已选择TXT；确认发布时才读取并校验正文。".into();
        Ok(())
    }
    pub(super) fn accept_images(&mut self, files: Vec<SelectedFile>) -> Result<(), String> {
        self.editable()?;
        if self.kind != ComponentKind::SubjectImage {
            return Err("此节点不接受图片。".into());
        }
        let base = if self.replace_next {
            0
        } else {
            self.pages.len()
        };
        if files.is_empty() || base + files.len() > MAX_CONTENT_PAGES {
            return Err("每个图片文档最多50页；请先移除页面或重新选择。".into());
        }
        for f in &files {
            let name = f.display_name().map_err(|_| "文件名无效。")?;
            if !matches!(
                fc_domain::materials::MaterialKind::from_name(&name),
                Some(
                    fc_domain::materials::MaterialKind::Png
                        | fc_domain::materials::MaterialKind::Jpeg
                        | fc_domain::materials::MaterialKind::Bmp
                )
            ) {
                return Err("只接受PNG、JPEG和BMP图片。".into());
            }
        }
        if self.replace_next {
            self.pages.clear();
        }
        self.pages
            .extend(files.into_iter().map(ImagePageInput::Selected));
        self.replace_next = false;
        self.selected = Some(base);
        self.phase = Phase::Edit;
        self.notice = "已更新页面草稿；确认发布后才验证图片并保存。".into();
        Ok(())
    }
    pub(super) fn select(&mut self, row: usize, column: usize) -> Result<(), String> {
        self.editable()?;
        if self.kind != ComponentKind::SubjectImage
            || self.phase != Phase::Edit
            || column >= 3
            || row >= self.pages.len()
        {
            return Err("请选择当前文档中的页面。".into());
        }
        self.selected = Some(row);
        Ok(())
    }
    pub(super) fn capture(&mut self, data: &FormData) -> Result<(), String> {
        if self.kind == ComponentKind::SubjectText && self.phase == Phase::Edit && !self.submitted {
            if let Some(text) = data.get(TEXT) {
                if text.contains('\0')
                    || text.encode_utf16().count() > 20000
                    || text.len() > MAX_CONTENT_TEXT_BYTES
                {
                    return Err("正文最多10000个字符、40000字节。草稿未被替换。".into());
                }
                if text != self.text {
                    self.txt = None;
                }
                self.text = text.into();
            }
        }
        Ok(())
    }
    pub(super) fn command(&mut self, id: ActionId, data: &FormData) -> Result<Command, String> {
        self.capture(data)?;
        if self.submitted {
            return Err("正在处理上次提交，请等待真实结果。".into());
        }
        if self.phase == Phase::Discard(true) || self.phase == Phase::Discard(false) {
            return match id {
                DISCARD => {
                    let close = self.phase == Phase::Discard(true);
                    if close {
                        Ok(Command::Close)
                    } else {
                        self.reload_expected = true;
                        self.phase = Phase::Edit;
                        Ok(Command::Reload)
                    }
                }
                KEEP => {
                    self.phase = Phase::Edit;
                    Ok(Command::Stay)
                }
                _ => Err("请选择保留草稿或明确放弃。".into()),
            };
        }
        if id == RELOAD && self.needs_reload {
            self.reload_expected = true;
            return Ok(Command::Reload);
        }
        if id == CLOSE || id == RELOAD {
            if self.is_dirty() {
                self.phase = Phase::Discard(id == CLOSE);
                return Ok(Command::Stay);
            }
            if id == CLOSE {
                return Ok(Command::Close);
            }
            self.reload_expected = true;
            return Ok(Command::Reload);
        }
        if id == CANCEL_REVIEW {
            self.cancel_review();
            return Ok(Command::Stay);
        }
        self.editable()?;
        if let Phase::Review(operation) = self.phase {
            if id != CONFIRM {
                return Err("请确认发布或返回修改。".into());
            }
            let expected = self.document.as_ref().ok_or("请先读取内容。")?.revision;
            let input = match operation {
                ContentOperation::Publish(ContentPayloadKind::Text) => {
                    self.restore_text = self.txt.is_none();
                    self.submitted_txt = self.txt.is_some();
                    if let Some(file) = self.txt.take() {
                        LocalContentInput::Txt(file)
                    } else {
                        ContentPayload::Text(self.text.clone())
                            .validate(self.slot)
                            .map_err(|_| "正文最多10000个字符、40000字节，请修改草稿。")?;
                        LocalContentInput::Text(self.text.clone())
                    }
                }
                ContentOperation::Publish(ContentPayloadKind::Images) => {
                    if self.pages.is_empty() {
                        return Err("请至少保留一页图片，或使用清空。".into());
                    }
                    self.submitted_page_count = self.pages.len();
                    self.restore_images = self
                        .pages
                        .iter()
                        .all(|p| matches!(p, ImagePageInput::Existing(_)));
                    let input = std::mem::take(&mut self.pages);
                    self.pages = input
                        .iter()
                        .filter_map(|p| match p {
                            ImagePageInput::Existing(r) => {
                                Some(ImagePageInput::Existing(r.clone()))
                            }
                            ImagePageInput::Selected(_) => None,
                        })
                        .collect();
                    LocalContentInput::ImagePages(input)
                }
                ContentOperation::Clear | ContentOperation::Undo { .. } => LocalContentInput::Empty,
            };
            self.submitted = true;
            return Ok(Command::Submit {
                slot: self.slot,
                expected_revision: expected,
                operation,
                input,
            });
        }
        match id {
            REVIEW => {
                if self.kind == ComponentKind::SubjectText && self.txt.is_none() {
                    ContentPayload::Text(self.text.clone())
                        .validate(self.slot)
                        .map_err(|_| "正文最多10000个字符、40000字节，请修改草稿。")?;
                }
                if self.kind == ComponentKind::SubjectImage && self.pages.is_empty() {
                    return Err("请至少选择一页图片，或使用清空。".into());
                }
                self.phase = Phase::Review(ContentOperation::Publish(self.payload_kind()));
            }
            CLEAR => {
                if self
                    .document
                    .as_ref()
                    .and_then(|d| d.current.as_ref())
                    .is_none_or(|v| v.payload == ContentPayload::Empty)
                {
                    return Err("此节点正文已为空；可撤销到保留的前一版本。".into());
                }
                self.phase = Phase::Review(ContentOperation::Clear);
            }
            UNDO => {
                let previous = self
                    .document
                    .as_ref()
                    .and_then(|d| d.previous.as_ref())
                    .ok_or("没有可撤销的前一已发布版本。")?;
                self.phase = Phase::Review(ContentOperation::Undo {
                    target_revision: previous.revision,
                });
            }
            PICK_TXT if self.kind == ComponentKind::SubjectText => return Ok(Command::PickTxt),
            PICK_IMAGES | REPLACE_IMAGES if self.kind == ComponentKind::SubjectImage => {
                self.replace_next = id == REPLACE_IMAGES;
                return Ok(Command::PickImages);
            }
            UP | DOWN | REMOVE if self.kind == ComponentKind::SubjectImage => {
                let row = self.selected.ok_or("请先选择页面。")?;
                if row >= self.pages.len() {
                    return Err("页面选择已失效。".into());
                }
                if id == REMOVE {
                    self.pages.remove(row);
                    self.selected = if self.pages.is_empty() {
                        None
                    } else {
                        Some(row.min(self.pages.len() - 1))
                    };
                } else if id == UP && row > 0 {
                    self.pages.swap(row, row - 1);
                    self.selected = Some(row - 1);
                } else if id == DOWN && row + 1 < self.pages.len() {
                    self.pages.swap(row, row + 1);
                    self.selected = Some(row + 1);
                }
            }
            _ => return Err("此页不支持该操作。".into()),
        }
        Ok(Command::Stay)
    }
    pub(super) fn view(&self, busy: bool) -> View {
        let blocked = busy || self.submitted;
        let mut v = View {
            title: if self.kind == ComponentKind::SubjectText {
                "独立文字内容"
            } else {
                "独立图片文档"
            }
            .into(),
            description: format!(
                "仅本地主管理员可发布。已读取内容修订{}；正文与节点样式分别保存。",
                self.document.as_ref().map_or(0, |d| d.revision.get())
            ),
            status: self.notice.clone(),
            fields: vec![],
            actions: vec![],
            busy: blocked,
            grid: None,
        };
        if self.cleanup_pending {
            v.status.push_str(" 原件清理仍在重试，配额暂未归还。");
        }
        if self.needs_reload {
            v.actions = vec![
                action(RELOAD, "重新读取核对", !busy),
                action(CLOSE, "返回", !busy),
            ];
            return v;
        }
        if let Phase::Review(op) = self.phase {
            v.description = match op {
                ContentOperation::Publish(ContentPayloadKind::Text) => {
                    if self.txt.is_some() || self.submitted && self.submitted_txt {
                        "确认读取所选TXT并发布到此节点？".into()
                    } else {
                        format!("确认发布{}个字符的文字？", self.text.chars().count())
                    }
                }
                ContentOperation::Publish(ContentPayloadKind::Images) => {
                    format!(
                        "确认按当前顺序发布{}页图片？",
                        if self.submitted {
                            self.submitted_page_count
                        } else {
                            self.pages.len()
                        }
                    )
                }
                ContentOperation::Clear => {
                    "确认清空此节点的已发布正文？保留前一版本以便撤销。".into()
                }
                ContentOperation::Undo { target_revision } => format!(
                    "确认恢复前一已发布版本{}？将作为新的内容版本发布。",
                    target_revision.get()
                ),
            };
            v.actions = vec![
                action(CONFIRM, "确认发布", !blocked),
                action(CANCEL_REVIEW, "返回修改", !blocked),
            ];
            return v;
        }
        if matches!(self.phase, Phase::Discard(_)) {
            v.description = "当前有未保存草稿。明确放弃后再返回或重新读取；取消会保留草稿。".into();
            v.actions = vec![
                action(DISCARD, "放弃草稿", !blocked),
                action(KEEP, "继续编辑", !blocked),
            ];
            return v;
        }
        if self.kind == ComponentKind::SubjectText {
            v.fields.push(Field {
                id: TEXT,
                label: "正文（最多10000字符，保留换行）".into(),
                kind: FieldKind::Multiline,
                value: self.text.clone(),
                max_utf16: 20000,
            });
            if let Some(file) = &self.txt {
                v.description.push_str(&format!(
                    " 已选TXT：{}。",
                    short(&file.display_name().unwrap_or_else(|_| "文件名无效".into()))
                ));
            }
            v.actions.push(action(PICK_TXT, "选择TXT…", !blocked));
        } else {
            v.grid = Some(Grid {
                columns: vec!["页面".into(), "来源".into(), "尺寸 / 状态".into()],
                rows: self
                    .pages
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let (name, detail) = match p {
                            ImagePageInput::Existing(r) => {
                                ("已保存页面".into(), format!("{} × {}", r.width, r.height))
                            }
                            ImagePageInput::Selected(file) => (
                                short(&file.display_name().unwrap_or_else(|_| "文件名无效".into())),
                                "待校验".into(),
                            ),
                        };
                        GridRow {
                            group: String::new(),
                            label: format!("第{}页", i + 1),
                            cells: vec![name, detail],
                            merged: false,
                        }
                    })
                    .collect(),
                selected: self.selected.map(|i| (i, 0)),
                enabled: !blocked,
            });
            v.actions.extend([
                action(PICK_IMAGES, "追加图片…", !blocked),
                action(REPLACE_IMAGES, "重新选择图片…", !blocked),
                action(UP, "上移", !blocked && self.selected.is_some_and(|i| i > 0)),
                action(
                    DOWN,
                    "下移",
                    !blocked && self.selected.is_some_and(|i| i + 1 < self.pages.len()),
                ),
                action(REMOVE, "移除此页", !blocked && self.selected.is_some()),
            ]);
        }
        v.actions.extend([
            action(REVIEW, "发布摘要…", !blocked && self.document.is_some()),
            action(
                CLEAR,
                "清空正文…",
                !blocked
                    && self
                        .document
                        .as_ref()
                        .and_then(|d| d.current.as_ref())
                        .is_some_and(|v| v.payload != ContentPayload::Empty),
            ),
            action(
                UNDO,
                "撤销到前一版本…",
                !blocked && self.document.as_ref().is_some_and(|d| d.previous.is_some()),
            ),
            action(RELOAD, "重新读取", !blocked),
            action(CLOSE, "返回布局", !blocked),
        ]);
        v
    }
}
#[cfg(test)]
#[path = "profile_content_editor_tests.rs"]
mod tests;
