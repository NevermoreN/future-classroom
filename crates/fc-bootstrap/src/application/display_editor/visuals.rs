use super::*;

pub(in crate::application) const VISUALS: ActionId = ActionId(209);
pub(in crate::application) const SWITCH_SUBJECT: ActionId = ActionId(210);
pub(in crate::application) const PICK_COLOR: ActionId = ActionId(211);
pub(in crate::application) const VISUAL_PREVIEW: ActionId = ActionId(212);
pub(in crate::application) const PALETTE_SUBJECT: FieldId = FieldId(220);
pub(in crate::application) const OPAQUE_RGB: FieldId = FieldId(221);
const EFFECTIVE_RGB: FieldId = FieldId(222);

pub(super) fn rgb_text(rgb: Rgb) -> String {
    format!("{:02X}{:02X}{:02X}", rgb.red, rgb.green, rgb.blue)
}
fn parse_rgb(text: &str) -> std::result::Result<Rgb, String> {
    // Exactly six opaque RGB digits: no CSS, alpha, '#' or whitespace coercion.
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("颜色请输入六位不透明RGB，例如 A6BDCE。".into());
    }
    let byte = |start| {
        u8::from_str_radix(&text[start..start + 2], 16).map_err(|_| "颜色无效。".to_owned())
    };
    Ok(Rgb {
        red: byte(0)?,
        green: byte(2)?,
        blue: byte(4)?,
    })
}
pub(super) fn checked_form(mut data: FormData) -> std::result::Result<FormData, String> {
    let fields = [
        PALETTE_SUBJECT,
        OPAQUE_RGB,
        EFFECTIVE_RGB,
        FILL,
        COURSE_ALPHA,
        HOMEWORK_ALPHA,
    ];
    let values = fields.map(|id| data.take(id));
    if !data.is_empty() {
        return Err("科目颜色表单包含未知字段，请重新打开。".into());
    }
    let mut clean = FormData::new();
    for (id, value) in fields.into_iter().zip(values) {
        if let Some(value) = value {
            clean.insert(id, value).map_err(|_| "科目颜色表单无效。")?;
        }
    }
    Ok(clean)
}
impl Editor {
    pub(in crate::application) fn colors(&self) -> Vec<SubjectColor> {
        self.palette
            .subjects
            .iter()
            .map(|s| SubjectColor {
                subject_id: s.id,
                color: s.color,
            })
            .collect()
    }
    pub(in crate::application) fn visuals_saved(&mut self, state: SubjectVisualState) {
        self.expected = state.display.revision;
        self.original = state.display.clone();
        self.settings = state.display;
        self.palette = state.palette;
        self.original_colors = self.colors();
        if !self
            .palette
            .subjects
            .iter()
            .any(|s| Some(s.id) == self.selected_subject)
        {
            self.selected_subject = self.palette.subjects.first().map(|s| s.id);
        }
        self.suspended = None;
    }
    pub(in crate::application) fn visual_preview_input(
        &self,
    ) -> (SubjectVisualPalette, DisplaySettings) {
        (self.palette.clone(), self.settings.clone())
    }
    pub(in crate::application) fn visual_page(&self) -> bool {
        self.page == Page::SubjectVisuals
    }
    pub(in crate::application) fn picked_color(
        &mut self,
        subject: SubjectId,
        color: Rgb,
    ) -> std::result::Result<(), String> {
        if !self.visual_page() || self.selected_subject != Some(subject) {
            return Err("科目或页面已变化，请重新选择颜色。".into());
        }
        self.palette
            .subjects
            .iter_mut()
            .find(|s| s.id == subject)
            .ok_or("科目已变化。")?
            .color = color;
        self.suspended = None;
        Ok(())
    }
    pub(super) fn visuals_view(&self, view: &mut View) {
        view.title = "科目颜色与显示".into();
        view.description="科目按固定编号区分。切换科目前先保留当前颜色；关闭底色不会删除色值。颜色与全部显示设置共用草稿，点击“保存全部”才一起生效。预览使用柔和处理后的颜色与当前透明度，仅绘制示例。".into();
        if let Some(subject) = self
            .palette
            .subjects
            .iter()
            .find(|s| Some(s.id) == self.selected_subject)
        {
            view.fields.push(field(
                PALETTE_SUBJECT,
                "选择科目（选择后点击切换）",
                FieldKind::Choice(
                    self.palette
                        .subjects
                        .iter()
                        .map(|s| Choice {
                            value: s.id.get().to_string(),
                            label: format!(
                                "{} · {}{}",
                                s.name,
                                s.id.get(),
                                if s.enabled { "" } else { "（已停用）" }
                            ),
                        })
                        .collect(),
                ),
                subject.id.get().to_string(),
                20,
            ));
            view.fields.push(plain(
                OPAQUE_RGB,
                "原始颜色（六位RGB）",
                rgb_text(subject.color),
                6,
            ));
            view.fields.push(field(
                EFFECTIVE_RGB,
                "应用到草稿后的柔和颜色",
                FieldKind::ReadOnly,
                rgb_text(fc_presentation::scene::soft_subject_color(subject.color)),
                6,
            ));
        } else {
            view.description
                .push_str("\n尚无学校科目，可返回工作台新建科目。");
        }
        view.fields.extend([
            toggle(FILL, "显示科目底色", self.settings.subject_fill),
            plain(
                COURSE_ALPHA,
                "课程胶囊填充不透明度（0–100%）",
                self.settings.course_fill_percent.to_string(),
                3,
            ),
            plain(
                HOMEWORK_ALPHA,
                "作业科目标记不透明度（0–100%）",
                self.settings.homework_fill_percent.to_string(),
                3,
            ),
        ]);
        view.actions = vec![
            action(APPLY, "应用到共同草稿"),
            action(SWITCH_SUBJECT, "保留当前颜色并切换科目"),
            action(PICK_COLOR, "选择颜色…"),
            action(VISUAL_PREVIEW, "应用并预览实际颜色"),
            action(SAVE_ALL, "保存全部"),
            action(EDIT_BACK, "应用并返回显示设置"),
            action(CANCEL, "取消全部修改"),
        ];
        if self.selected_subject.is_none() {
            for action in &mut view.actions {
                if matches!(action.id, SWITCH_SUBJECT | PICK_COLOR | VISUAL_PREVIEW) {
                    action.enabled = false;
                }
            }
        }
    }
    pub(super) fn apply_visuals(
        &mut self,
        data: &FormData,
        switch: bool,
    ) -> std::result::Result<(), String> {
        let fill = yes(data, FILL)?;
        let course = percent(data, COURSE_ALPHA)?;
        let homework = percent(data, HOMEWORK_ALPHA)?;
        let mut target = self.selected_subject;
        let mut color = None;
        if let Some(selected) = self.selected_subject {
            let requested = raw(data, PALETTE_SUBJECT)
                .parse::<u64>()
                .ok()
                .and_then(|id| SubjectId::new(id).ok())
                .filter(|id| self.palette.subjects.iter().any(|s| s.id == *id))
                .ok_or("请选择当前列表内的科目。")?;
            if !switch && requested != selected {
                return Err("请先点击切换科目，再编辑该科目颜色。".into());
            }
            color = Some(parse_rgb(raw(data, OPAQUE_RGB))?);
            if switch {
                target = Some(requested);
            }
        } else if data.get(PALETTE_SUBJECT).is_some() || data.get(OPAQUE_RGB).is_some() {
            return Err("没有可编辑的学校科目。".into());
        }
        // All fields pass before changing any part of the common draft.
        if let Some(color) = color {
            self.palette
                .subjects
                .iter_mut()
                .find(|s| Some(s.id) == self.selected_subject)
                .ok_or("科目已变化。")?
                .color = color;
        }
        self.settings.subject_fill = fill;
        self.settings.course_fill_percent = course;
        self.settings.homework_fill_percent = homework;
        self.selected_subject = target;
        Ok(())
    }
}
