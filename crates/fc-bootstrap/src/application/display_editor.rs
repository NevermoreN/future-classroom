//! Non-secret global display draft. Saving publishes one independent revision.
use super::*;
use fc_domain::{desktop::*, time::LocalDate};
pub(super) const CLOCK: FieldId = FieldId(200);
pub(super) const SECONDS: FieldId = FieldId(201);
pub(super) const RECESS: FieldId = FieldId(202);
pub(super) const FILL: FieldId = FieldId(203);
pub(super) const COURSE_ALPHA: FieldId = FieldId(204);
pub(super) const HOMEWORK_ALPHA: FieldId = FieldId(205);
pub(super) const TONE: FieldId = FieldId(206);
pub(super) const TARGET: FieldId = FieldId(207);
pub(super) const LABEL: FieldId = FieldId(208);
pub(super) const EXPIRED: FieldId = FieldId(209);
pub(super) const COMPONENT: FieldId = FieldId(210);
pub(super) const VISIBLE: FieldId = FieldId(211);
pub(super) const ALPHA: FieldId = FieldId(212);
pub(super) const FONT: FieldId = FieldId(213);
pub(super) const MIN_WIDTH: FieldId = FieldId(214);
pub(super) const MAX_WIDTH: FieldId = FieldId(215);
pub(super) const MIN_HEIGHT: FieldId = FieldId(216);
pub(super) const MAX_HEIGHT: FieldId = FieldId(217);
pub(super) const APPLY: ActionId = ActionId(200);
pub(super) const SAVE_ALL: ActionId = ActionId(201);
pub(super) const COMPONENTS: ActionId = ActionId(202);
pub(super) const SWITCH_COMPONENT: ActionId = ActionId(203);
pub(super) const PREVIEW: ActionId = ActionId(204);
pub(super) const EDIT_BACK: ActionId = ActionId(205);
pub(super) const CANCEL: ActionId = ActionId(206);
pub(super) const CONFIRM_CANCEL: ActionId = ActionId(207);
const KEEP: ActionId = ActionId(208);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Main,
    Components,
    Preview,
    Discard,
}
pub(super) enum Command {
    Stay,
    Close,
    Reload,
    Save {
        expected: Revision,
        settings: DisplaySettings,
    },
}
pub(super) struct Editor {
    pub(super) settings: DisplaySettings,
    original: DisplaySettings,
    owner: PrincipalId,
    expected: Revision,
    page: Page,
    discard_return: Page,
    component: usize,
    suspended: Option<FormData>,
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
fn plain(id: FieldId, label: &str, value: String, max: usize) -> Field {
    field(id, label, FieldKind::Text, value, max)
}
fn choose(id: FieldId, label: &str, value: &str, options: &[(&str, &str)]) -> Field {
    field(
        id,
        label,
        FieldKind::Choice(
            options
                .iter()
                .map(|(v, l)| Choice {
                    value: (*v).into(),
                    label: (*l).into(),
                })
                .collect(),
        ),
        value.into(),
        64,
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
fn action(id: ActionId, label: &str) -> Action {
    Action {
        id,
        label: label.into(),
        enabled: true,
    }
}
fn raw(data: &FormData, id: FieldId) -> &str {
    data.get(id).unwrap_or("")
}
fn yes(data: &FormData, id: FieldId) -> std::result::Result<bool, String> {
    match raw(data, id) {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err("请选择启用或关闭。".into()),
    }
}
fn integer(data: &FormData, id: FieldId) -> std::result::Result<u16, String> {
    let s = raw(data, id).trim();
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err("请输入整数百分比或尺寸。".into());
    }
    s.parse().map_err(|_| "整数超出允许范围。".into())
}
fn percent(data: &FormData, id: FieldId) -> std::result::Result<u8, String> {
    let n = integer(data, id)?;
    u8::try_from(n)
        .ok()
        .filter(|n| *n <= 100)
        .ok_or_else(|| "透明度范围为0至100，0%完全透明、100%不透明。".into())
}
fn label(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Clock => "时钟",
        ComponentKind::Date => "日期",
        ComponentKind::Schedule => "今日课表",
        ComponentKind::Homework => "今日作业",
        ComponentKind::Materials => "课堂资料",
        ComponentKind::Reminder => "今日提醒",
        ComponentKind::DateCountdown => "日期倒计时",
        ComponentKind::Recess => "课间大字倒计时",
    }
}
fn format_date(d: LocalDate) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}
fn parse_date(value: &str) -> std::result::Result<Option<LocalDate>, String> {
    let s = value.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if s.len() != 10
        || s.as_bytes()[4] != b'-'
        || s.as_bytes()[7] != b'-'
        || s.bytes()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return Err("目标日期请输入YYYY-MM-DD，留空可不设置目标。".into());
    }
    LocalDate::new(
        s[..4].parse().unwrap_or(0),
        s[5..7].parse().unwrap_or(0),
        s[8..].parse().unwrap_or(0),
    )
    .map(Some)
    .map_err(|_| "目标日期不存在。".into())
}
pub(super) fn problem(error: &DisplayError) -> String {
    match error {
        DisplayError::RevisionConflict { .. } => {
            "显示设置已被其他操作更新。草稿仍保留，请取消并重新加载后核对。".into()
        }
        DisplayError::Identity(_) => "当前会话无权修改显示设置，请由主管理员重新登录。".into(),
        DisplayError::Capacity => "标签或显示设置超过容量上限。".into(),
        DisplayError::VersionOverflow => "显示设置修订已达上限。".into(),
        _ => "显示设置无效，请检查透明度、字号以及最小/最大尺寸。".into(),
    }
}
impl Editor {
    pub(super) fn new(settings: DisplaySettings, owner: PrincipalId) -> Self {
        Self {
            expected: settings.revision,
            original: settings.clone(),
            settings,
            owner,
            page: Page::Main,
            discard_return: Page::Main,
            component: 0,
            suspended: None,
        }
    }
    pub(super) fn owner(&self) -> PrincipalId {
        self.owner
    }
    pub(super) fn saved(&mut self, settings: DisplaySettings) {
        self.expected = settings.revision;
        self.original = settings.clone();
        self.settings = settings;
        self.suspended = None;
    }
    pub(super) fn has_unsaved_changes(&self) -> bool {
        self.settings != self.original || self.suspended.is_some()
    }
    pub(super) fn capture(&mut self, data: FormData) {
        self.suspended = None;
        if matches!(self.page, Page::Main | Page::Components) {
            let view = self.view("", false);
            let mut pending = FormData::new();
            let mut changed = false;
            for field in view.fields.iter().filter(|f| f.kind != FieldKind::ReadOnly) {
                let Some(value) = data.get(field.id) else {
                    continue;
                };
                // Malformed transport values cannot poison the next native View.
                // Valid bounded text (including an unfinished number/date) is kept.
                if value.contains('\0')
                    || value.encode_utf16().count() > field.max_utf16
                    || matches!(&field.kind, FieldKind::Choice(choices) if !choices.iter().any(|c| c.value == value))
                {
                    continue;
                }
                if pending.insert(field.id, value.into()).is_err() {
                    continue;
                }
                changed |= value != field.value;
            }
            if changed {
                self.suspended = Some(pending);
            }
        }
    }
    pub(super) fn view(&self, status: &str, busy: bool) -> View {
        let mut v=View{title:"桌面显示设置".into(),description:"修改先保留为草稿，点击“保存全部”才生效。草稿仅在本次运行保留，退出前请保存。玻璃材质3%，边缘反射50%，文字透明度与玻璃分开。".into(),status:status.into(),fields:vec![],actions:vec![],busy,grid:None};
        match self.page {
            Page::Main => {
                let s = &self.settings;
                v.fields = vec![
                    choose(
                        CLOCK,
                        "时钟样式",
                        match s.clock_mode {
                            ClockMode::Digital24 => "24",
                            ClockMode::Digital12 => "12",
                            ClockMode::Analog => "analog",
                        },
                        &[
                            ("24", "24小时数字"),
                            ("12", "12小时数字"),
                            ("analog", "模拟表盘"),
                        ],
                    ),
                    toggle(SECONDS, "显示秒数 / 逐秒秒针", s.clock_seconds),
                    toggle(RECESS, "自动显示有效课间大卡", s.recess_enabled),
                    toggle(FILL, "显示科目底色", s.subject_fill),
                    plain(
                        COURSE_ALPHA,
                        "课程胶囊填充透明度（0–100）",
                        s.course_fill_percent.to_string(),
                        3,
                    ),
                    plain(
                        HOMEWORK_ALPHA,
                        "作业科目标记透明度（0–100）",
                        s.homework_fill_percent.to_string(),
                        3,
                    ),
                    choose(
                        TONE,
                        "文字主题",
                        if s.text_tone == TextTone::Light {
                            "light"
                        } else {
                            "dark"
                        },
                        &[
                            ("light", "浅色文字（深色壁纸）"),
                            ("dark", "深色文字（浅色壁纸）"),
                        ],
                    ),
                    plain(
                        TARGET,
                        "倒计时目标日期（可留空）",
                        s.countdown.target.map(format_date).unwrap_or_default(),
                        10,
                    ),
                    plain(LABEL, "倒计时名称", s.countdown.label.clone(), 64),
                    choose(
                        EXPIRED,
                        "目标日期已过",
                        if s.countdown.expired == CountdownExpired::Hide {
                            "hide"
                        } else {
                            "completed"
                        },
                        &[("hide", "隐藏倒计时"), ("completed", "显示已结束")],
                    ),
                ];
                v.actions = vec![
                    action(APPLY, "应用到草稿"),
                    action(COMPONENTS, "应用并编辑逐组件显示"),
                    action(PREVIEW, "应用并预览参数"),
                    action(SAVE_ALL, "保存全部"),
                    action(EDIT_BACK, "保留草稿并返回"),
                    action(CANCEL, "取消全部修改"),
                ];
            }
            Page::Components => {
                v.title = "逐组件显示与自适应约束".into();
                v.description.push_str("\n0%组件保留布局占位，但不绘制、不接受点击；关闭“显示组件”才移除布局。最小/最大约束不得越过工作区，过紧约束会显示诊断。");
                let c = &self.settings.components[self.component];
                let options = self
                    .settings
                    .components
                    .iter()
                    .enumerate()
                    .map(|(i, c)| Choice {
                        value: i.to_string(),
                        label: label(c.kind).into(),
                    })
                    .collect();
                v.fields = vec![
                    field(
                        COMPONENT,
                        "选择组件",
                        FieldKind::Choice(options),
                        self.component.to_string(),
                        4,
                    ),
                    toggle(VISIBLE, "显示组件", c.visible),
                    plain(
                        ALPHA,
                        "组件整体透明度（0–100）",
                        c.opacity_percent.to_string(),
                        3,
                    ),
                    plain(FONT, "字号（10–72 DIP）", c.font_dip.to_string(), 2),
                    plain(
                        MIN_WIDTH,
                        "最小宽度（120–4096 DIP）",
                        c.min_width.to_string(),
                        4,
                    ),
                    plain(
                        MAX_WIDTH,
                        "最大宽度（120–4096 DIP）",
                        c.max_width.to_string(),
                        4,
                    ),
                    plain(
                        MIN_HEIGHT,
                        "最小高度（32–2160 DIP）",
                        c.min_height.to_string(),
                        4,
                    ),
                    plain(
                        MAX_HEIGHT,
                        "最大高度（32–2160 DIP）",
                        c.max_height.to_string(),
                        4,
                    ),
                ];
                v.actions = vec![
                    action(APPLY, "应用当前组件到草稿"),
                    action(SWITCH_COMPONENT, "应用当前编辑并切换组件"),
                    action(SAVE_ALL, "保存全部"),
                    action(EDIT_BACK, "应用并返回显示设置"),
                ];
            }
            Page::Preview => {
                v.title = "显示参数预览".into();
                let s = &self.settings;
                let now = CurrentClock::new().try_now().ok();
                let style = match s.clock_mode {
                    ClockMode::Digital24 => fc_presentation::ClockStyle::Digital24,
                    _ => fc_presentation::ClockStyle::Digital12,
                };
                v.description=format!("时钟示例：{}\n课程底色{}%，作业标记{}%；文字不乘底色Alpha。\n{}\n此页为参数预览。保存后关闭工作台，展示只在系统桌面前台出现；PPT、其他应用或锁屏时隐藏。",now.map(|n|fc_presentation::format_clock(n,style,s.clock_seconds)).unwrap_or("系统时间不可用".into()),s.course_fill_percent,s.homework_fill_percent,s.components.iter().map(|c|format!("{}：{}，{}%，字号{}",label(c.kind),if c.visible{"显示"}else{"隐藏"},c.opacity_percent,c.font_dip)).collect::<Vec<_>>().join("\n"));
                v.actions = vec![action(EDIT_BACK, "返回显示设置")];
            }
            Page::Discard => {
                v.title = "取消显示设置修改".into();
                v.description =
                    "确认后丢弃本地显示设置草稿并重新读取已保存版，不修改数据库。".into();
                v.actions = vec![
                    action(CONFIRM_CANCEL, "确认丢弃并重读"),
                    action(KEEP, "保留草稿"),
                ];
            }
        }
        v.actions.push(action(LOCK, "锁定并保留草稿"));
        if let Some(data) = &self.suspended {
            for f in &mut v.fields {
                if let Some(s) = data.get(f.id) {
                    f.value = s.into();
                }
            }
        }
        v
    }
    pub(super) fn handle(
        &mut self,
        action: ActionId,
        data: FormData,
    ) -> std::result::Result<Command, String> {
        // Navigation must preserve unparsed non-secret text. A discard prompt is
        // not confirmation, and closing the main page is explicitly "keep draft".
        if action == CANCEL && matches!(self.page, Page::Main | Page::Components) {
            self.capture(data);
            self.discard_return = self.page;
            self.page = Page::Discard;
            return Ok(Command::Stay);
        }
        if action == KEEP && self.page == Page::Discard {
            self.page = self.discard_return;
            return Ok(Command::Stay);
        }
        if action == EDIT_BACK && self.page == Page::Main {
            self.capture(data);
            return Ok(Command::Close);
        }
        if action == CONFIRM_CANCEL && self.page != Page::Discard {
            return Err("请先确认取消全部修改。".into());
        }
        let original = self.settings.clone();
        let result = self.apply(action, &data);
        if result.is_err() {
            self.settings = original;
            self.capture(data);
        } else {
            self.suspended = None;
        }
        result
    }
    fn apply(&mut self, action: ActionId, data: &FormData) -> std::result::Result<Command, String> {
        match action {
            APPLY | SAVE_ALL | COMPONENTS | PREVIEW | SWITCH_COMPONENT => {
                match self.page {
                    Page::Main => self.apply_main(data)?,
                    Page::Components => self.apply_component(data)?,
                    _ => return Err("当前页面不能应用显示设置。".into()),
                }
                if action == SAVE_ALL {
                    return Ok(Command::Save {
                        expected: self.expected,
                        settings: self.settings.clone(),
                    });
                }
                if action == COMPONENTS {
                    self.page = Page::Components;
                }
                if action == PREVIEW {
                    self.page = Page::Preview;
                }
                if action == SWITCH_COMPONENT {
                    let n = raw(data, COMPONENT)
                        .parse::<usize>()
                        .ok()
                        .filter(|n| *n < self.settings.components.len())
                        .ok_or("请选择有效组件。")?;
                    self.component = n;
                }
            }
            EDIT_BACK => {
                if self.page == Page::Components {
                    self.apply_component(data)?;
                }
                self.page = Page::Main;
            }
            CANCEL => self.page = Page::Discard,
            CONFIRM_CANCEL => return Ok(Command::Reload),
            KEEP => self.page = Page::Main,
            _ => return Err("当前页面不能执行此操作。".into()),
        }
        Ok(Command::Stay)
    }
    fn apply_main(&mut self, data: &FormData) -> std::result::Result<(), String> {
        let mut next = self.settings.clone();
        next.clock_mode = match raw(data, CLOCK) {
            "24" => ClockMode::Digital24,
            "12" => ClockMode::Digital12,
            "analog" => ClockMode::Analog,
            _ => return Err("请选择时钟样式。".into()),
        };
        next.clock_seconds = yes(data, SECONDS)?;
        next.recess_enabled = yes(data, RECESS)?;
        next.subject_fill = yes(data, FILL)?;
        next.course_fill_percent = percent(data, COURSE_ALPHA)?;
        next.homework_fill_percent = percent(data, HOMEWORK_ALPHA)?;
        next.text_tone = match raw(data, TONE) {
            "light" => TextTone::Light,
            "dark" => TextTone::Dark,
            _ => return Err("请选择文字主题。".into()),
        };
        next.countdown.target = parse_date(raw(data, TARGET))?;
        next.countdown.label = raw(data, LABEL).trim().into();
        next.countdown.expired = match raw(data, EXPIRED) {
            "hide" => CountdownExpired::Hide,
            "completed" => CountdownExpired::ShowCompleted,
            _ => return Err("请选择到期显示方式。".into()),
        };
        next.validate().map_err(|e| problem(&e))?;
        self.settings = next;
        Ok(())
    }
    fn apply_component(&mut self, data: &FormData) -> std::result::Result<(), String> {
        let mut next = self.settings.clone();
        let c = &mut next.components[self.component];
        c.visible = yes(data, VISIBLE)?;
        c.opacity_percent = percent(data, ALPHA)?;
        c.font_dip = integer(data, FONT)?;
        c.min_width = integer(data, MIN_WIDTH)?;
        c.max_width = integer(data, MAX_WIDTH)?;
        c.min_height = integer(data, MIN_HEIGHT)?;
        c.max_height = integer(data, MAX_HEIGHT)?;
        next.validate().map_err(|e| problem(&e))?;
        self.settings = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor() -> Editor {
        let settings = DisplaySettings {
            revision: Revision::new(7),
            ..DisplaySettings::default()
        };
        Editor::new(settings, PrincipalId::new(1).unwrap())
    }
    fn form(editor: &Editor) -> FormData {
        let mut data = FormData::new();
        for field in editor.view("", false).fields {
            data.insert(field.id, field.value).unwrap();
        }
        data
    }
    fn set(data: &mut FormData, id: FieldId, value: &str) {
        data.take(id);
        data.insert(id, value.into()).unwrap();
    }
    fn value(editor: &Editor, id: FieldId) -> String {
        editor
            .view("", false)
            .fields
            .into_iter()
            .find(|f| f.id == id)
            .unwrap()
            .value
    }
    #[test]
    fn invalid_partial_main_survives_error_close_and_lock_capture() {
        let mut e = editor();
        let original = e.settings.clone();
        let mut data = form(&e);
        set(&mut data, TARGET, "2026-1");
        set(&mut data, COURSE_ALPHA, "21");
        assert!(e.handle(APPLY, data).is_err());
        assert_eq!(e.settings, original);
        assert_eq!(value(&e, TARGET), "2026-1");
        let data = form(&e);
        assert!(matches!(e.handle(EDIT_BACK, data), Ok(Command::Close)));
        assert_eq!(value(&e, TARGET), "2026-1");
        assert!(e.has_unsaved_changes());
        let data = form(&e);
        e.capture(data);
        assert_eq!(value(&e, TARGET), "2026-1");
        assert_eq!(e.owner(), PrincipalId::new(1).unwrap());
    }
    #[test]
    fn switching_components_applies_old_fields_atomically_and_keeps_ids_stable() {
        let mut e = editor();
        let data = form(&e);
        e.handle(COMPONENTS, data).unwrap();
        let before = e.settings.clone();
        let mut data = form(&e);
        set(&mut data, COMPONENT, "1");
        set(&mut data, ALPHA, "27");
        set(&mut data, FONT, "X");
        assert!(e.handle(SWITCH_COMPONENT, data).is_err());
        assert_eq!(e.component, 0);
        assert_eq!(e.settings, before);
        assert_eq!(value(&e, FONT), "X");
        let mut data = form(&e);
        set(&mut data, FONT, "32");
        e.handle(SWITCH_COMPONENT, data).unwrap();
        assert_eq!(e.component, 1);
        assert_eq!(e.settings.components[0].kind, ComponentKind::Clock);
        assert_eq!(e.settings.components[0].opacity_percent, 27);
        assert_eq!(e.settings.components[0].font_dip, 32);
        assert_eq!(e.settings.components[1], before.components[1]);
    }
    #[test]
    fn back_from_components_applies_valid_fields_and_refuses_to_lose_invalid_text() {
        let mut e = editor();
        let data = form(&e);
        e.handle(COMPONENTS, data).unwrap();
        let mut data = form(&e);
        set(&mut data, ALPHA, "42");
        set(&mut data, MIN_WIDTH, "X");
        assert!(e.handle(EDIT_BACK, data).is_err());
        assert_eq!(e.page, Page::Components);
        assert_eq!(value(&e, MIN_WIDTH), "X");
        assert_eq!(e.settings.components[0].opacity_percent, 100);
        let mut data = form(&e);
        set(&mut data, MIN_WIDTH, "160");
        e.handle(EDIT_BACK, data).unwrap();
        assert_eq!(e.page, Page::Main);
        assert_eq!(e.settings.components[0].opacity_percent, 42);
    }
    #[test]
    fn save_captures_original_expected_cas_until_successful_saved_callback() {
        let mut e = editor();
        let mut data = form(&e);
        set(&mut data, COURSE_ALPHA, "25");
        let command = e.handle(SAVE_ALL, data).unwrap();
        let Command::Save { expected, settings } = command else {
            panic!("save expected")
        };
        assert_eq!(expected, Revision::new(7));
        assert_eq!(settings.revision, Revision::new(7));
        assert_eq!(settings.course_fill_percent, 25);
        let mut confirmed = settings;
        confirmed.revision = Revision::new(8);
        e.saved(confirmed);
        assert!(!e.has_unsaved_changes());
        let data = form(&e);
        let Command::Save { expected, .. } = e.handle(SAVE_ALL, data).unwrap() else {
            panic!()
        };
        assert_eq!(expected, Revision::new(8));
    }
    #[test]
    fn discard_requires_confirmation_and_keep_restores_component_partial_text() {
        let mut e = editor();
        assert!(e.handle(CONFIRM_CANCEL, FormData::new()).is_err());
        let data = form(&e);
        e.handle(COMPONENTS, data).unwrap();
        let mut data = form(&e);
        set(&mut data, FONT, "X");
        assert!(matches!(e.handle(CANCEL, data), Ok(Command::Stay)));
        assert_eq!(e.page, Page::Discard);
        assert!(e.has_unsaved_changes());
        e.handle(KEEP, FormData::new()).unwrap();
        assert_eq!(e.page, Page::Components);
        assert_eq!(value(&e, FONT), "X");
        let data = form(&e);
        e.handle(CANCEL, data).unwrap();
        assert!(matches!(
            e.handle(CONFIRM_CANCEL, FormData::new()),
            Ok(Command::Reload)
        ));
        assert_eq!(e.settings, e.original);
    }
    #[test]
    fn incomplete_dates_malformed_toggles_and_overflow_sizes_never_partially_apply() {
        assert!(parse_date("2026-02-30").is_err());
        assert!(parse_date("２０２６-10-30").is_err());
        for (id, text) in [
            (SECONDS, "maybe"),
            (COURSE_ALPHA, "256"),
            (TARGET, "2026-02-30"),
        ] {
            let mut e = editor();
            let original = e.settings.clone();
            let mut data = form(&e);
            set(&mut data, id, text);
            assert!(e.handle(APPLY, data).is_err());
            assert_eq!(e.settings, original);
        }
    }
}

#[cfg(test)]
mod schema_tests {
    use super::*;
    #[test]
    fn failed_transport_values_cannot_poison_native_view_schema() {
        let mut e = Editor::new(DisplaySettings::default(), PrincipalId::new(1).unwrap());
        let mut data = FormData::new();
        for field in e.view("", false).fields {
            data.insert(
                field.id,
                if field.id == SECONDS {
                    "invalid-choice".into()
                } else {
                    field.value
                },
            )
            .unwrap();
        }
        assert!(e.handle(APPLY, data).is_err());
        e.view("", false).validate().unwrap();
    }
    #[test]
    fn every_display_page_is_a_bounded_native_schema() {
        let mut e = Editor::new(DisplaySettings::default(), PrincipalId::new(1).unwrap());
        for page in [Page::Main, Page::Components, Page::Preview, Page::Discard] {
            e.page = page;
            e.view("", false).validate().unwrap();
        }
    }
}
