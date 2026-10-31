//! Public, single-lesson chooser. The worker owns the original scene binding;
//! this form contains only business choices and never account authority.
use super::*;
use fc_domain::{
    lesson_context::LessonAction,
    schedule::{CourseCell, TeacherBinding},
};
use fc_ports::DesktopLessonSource;

pub(super) const SUBJECT: FieldId = FieldId(410);
pub(super) const TEACHER: FieldId = FieldId(411);
pub(super) const RELOAD: ActionId = ActionId(410);
pub(super) const REPLACE: ActionId = ActionId(411);
pub(super) const RESTORE: ActionId = ActionId(412);
pub(super) const CONFIRM_UNASSIGNED: ActionId = ActionId(413);
pub(super) const CANCEL_UNASSIGNED: ActionId = ActionId(414);
pub(super) const CLOSE: ActionId = ActionId(415);

pub(super) enum Command {
    Stay,
    Close,
    Reload,
    Apply(LessonAction),
}
pub(super) struct Editor {
    pub(super) source: DesktopLessonSource,
    subject: Option<SubjectId>,
    teacher: Option<PrincipalId>,
    review: bool,
}
impl Editor {
    pub(super) fn new(source: DesktopLessonSource) -> Self {
        let subject = source
            .context
            .current
            .as_ref()
            .map(|c| c.effective.subject_id);
        let teacher = source.context.current.as_ref().and_then(|c| c.teacher_id);
        Self {
            source,
            subject,
            teacher,
            review: false,
        }
    }
    fn candidates(
        &self,
        subject: SubjectId,
    ) -> impl Iterator<Item = &fc_ports::DesktopLessonTeacher> {
        self.source
            .teachers
            .iter()
            .filter(move |t| t.subject_ids.contains(&subject))
    }
    fn read_choices(
        mut data: FormData,
    ) -> std::result::Result<(SubjectId, Option<PrincipalId>), String> {
        let subject = data.take(SUBJECT).ok_or("请选择科目。")?;
        let teacher = data.take(TEACHER).ok_or("请选择任课教师。")?;
        if !data.is_empty() {
            return Err("此页面仅接收科目和任课教师选择。".into());
        }
        let subject = subject
            .parse::<u64>()
            .ok()
            .and_then(|n| SubjectId::new(n).ok())
            .ok_or("请选择有效科目。")?;
        let teacher = if teacher == "choose" || teacher == "unassigned" {
            None
        } else {
            Some(
                teacher
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| PrincipalId::new(n).ok())
                    .ok_or("请选择有效任课教师。")?,
            )
        };
        Ok((subject, teacher))
    }
    fn capture(&mut self, data: FormData) -> std::result::Result<bool, String> {
        let (subject, teacher) = Self::read_choices(data)?;
        if !self
            .source
            .subjects
            .iter()
            .any(|s| s.id == subject && s.enabled)
        {
            return Err("请选择有效科目。".into());
        }
        let changed = self.subject != Some(subject);
        self.subject = Some(subject);
        self.teacher = if changed {
            let mut ids = self.candidates(subject).map(|t| t.id);
            let first = ids.next();
            if ids.next().is_none() {
                first
            } else {
                None
            }
        } else {
            teacher
        };
        Ok(changed)
    }
    pub(super) fn input(&mut self, data: FormData) -> bool {
        if self.review {
            return false;
        }
        self.capture(data).unwrap_or(false)
    }
    pub(super) fn view(&self, status: &str, busy: bool) -> View {
        let mut v = View {
            title: "本节临时换课".into(),
            description:
                "仅影响当前这一节，下课自动结束。课程发生变化时，请关闭并重新点击桌面按钮。".into(),
            status: status.into(),
            fields: vec![],
            actions: vec![],
            busy,
            grid: None,
        };
        let action = |id, label: &str, enabled| Action {
            id,
            label: label.into(),
            enabled,
        };
        if self.review {
            v.description =
                "此科目在本班没有任课教师。确认后本节显示‘未分配老师’，课堂资料保持空白。".into();
            v.actions = vec![
                action(CONFIRM_UNASSIGNED, "确认以未分配老师换课", true),
                action(CANCEL_UNASSIGNED, "返回选择", true),
                action(CLOSE, "关闭", true),
            ];
            return v;
        }
        if let Some(current) = &self.source.context.current {
            let name = |id| {
                self.source
                    .subjects
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.name.as_str())
                    .unwrap_or("科目已变化")
            };
            let teacher = current
                .teacher_id
                .and_then(|id| self.source.teachers.iter().find(|t| t.id == id))
                .map(|t| t.display_name.as_str())
                .unwrap_or("未分配老师");
            v.description=format!("{:02}:{:02}–{:02}:{:02}　原课：{}；当前：{} / {}。\n仅本节有效，下课自动结束；保存后关闭，后续操作请重新点击桌面按钮。",current.key.range.begin.get()/60,current.key.range.begin.get()%60,current.key.range.end.get()/60,current.key.range.end.get()%60,name(current.base.subject_id),name(current.effective.subject_id),teacher);
            let subjects = self
                .source
                .subjects
                .iter()
                .filter(|s| s.enabled)
                .map(|s| Choice {
                    value: s.id.get().to_string(),
                    label: s.name.clone(),
                })
                .collect();
            let mut teachers = vec![Choice {
                value: "choose".into(),
                label: "请选择任课教师".into(),
            }];
            if let Some(subject) = self.subject {
                let allowed: Vec<_> = self
                    .candidates(subject)
                    .map(|t| Choice {
                        value: t.id.get().to_string(),
                        label: t.display_name.clone(),
                    })
                    .collect();
                if allowed.is_empty() {
                    teachers = vec![Choice {
                        value: "unassigned".into(),
                        label: "未分配老师（保存时需确认）".into(),
                    }];
                } else {
                    teachers.extend(allowed);
                }
            }
            let teacher = self
                .teacher
                .map(|t| t.get().to_string())
                .filter(|id| teachers.iter().any(|t| t.value == *id))
                .unwrap_or_else(|| teachers[0].value.clone());
            v.fields = vec![
                Field {
                    id: SUBJECT,
                    label: "临时科目".into(),
                    kind: FieldKind::Choice(subjects),
                    value: self
                        .subject
                        .map(|s| s.get().to_string())
                        .unwrap_or_default(),
                    max_utf16: 32,
                },
                Field {
                    id: TEACHER,
                    label: "临时任课教师".into(),
                    kind: FieldKind::Choice(teachers),
                    value: teacher,
                    max_utf16: 32,
                },
            ];
            v.actions.push(action(REPLACE, "保存本节临时换课", true));
            v.actions.push(action(
                RESTORE,
                "恢复本节原课",
                self.source.context.active_override.is_some(),
            ));
        }
        v.actions.extend([
            action(RELOAD, "重新读取本节课程", true),
            action(CLOSE, "关闭", true),
        ]);
        v
    }
    pub(super) fn handle(
        &mut self,
        action: ActionId,
        mut data: FormData,
    ) -> std::result::Result<Command, String> {
        // Ordinary native buttons carry the currently visible form. Consume only
        // the two known choices; neither cancel nor reload can become a write.
        if matches!(action, CLOSE | RELOAD | CANCEL_UNASSIGNED) {
            let _ = data.take(SUBJECT);
            let _ = data.take(TEACHER);
            if !data.is_empty() {
                return Err("此页面不接收其他字段。".into());
            }
            return Ok(if action == CLOSE {
                Command::Close
            } else if action == RELOAD {
                Command::Reload
            } else {
                self.review = false;
                Command::Stay
            });
        }
        if self.source.context.current.is_none() {
            return Err("当前课程已结束，请关闭并重新点击桌面按钮。".into());
        }
        if action == RESTORE {
            let _ = Self::read_choices(data)?;
            if self.source.context.active_override.is_none() {
                return Err("本节没有临时换课。".into());
            }
            return Ok(Command::Apply(LessonAction::Restore));
        }
        if action == CONFIRM_UNASSIGNED {
            if !data.is_empty() || !self.review {
                return Err("请先选择科目并确认未分配老师。".into());
            }
            let subject = self.subject.ok_or("请选择科目。")?;
            if self.candidates(subject).next().is_some() {
                return Err("任课教师已变化，请关闭后重新选择。".into());
            }
            return Ok(Command::Apply(LessonAction::Replace(CourseCell {
                subject_id: subject,
                teacher: TeacherBinding::Unassigned,
            })));
        }
        if action != REPLACE || self.review {
            return Err("此操作不可用。".into());
        }
        let (subject, teacher) = Self::read_choices(data)?;
        if !self
            .source
            .subjects
            .iter()
            .any(|s| s.id == subject && s.enabled)
        {
            return Err("请选择有效科目。".into());
        }
        self.subject = Some(subject);
        self.teacher = teacher;
        if self.candidates(subject).next().is_none() {
            self.teacher = None;
            self.review = true;
            return Ok(Command::Stay);
        }
        let teacher = teacher
            .filter(|id| self.candidates(subject).any(|t| t.id == *id))
            .ok_or("请选择本班此科目的任课教师。")?;
        Ok(Command::Apply(LessonAction::Replace(CourseCell {
            subject_id: subject,
            teacher: TeacherBinding::Explicit(teacher),
        })))
    }
}

#[cfg(test)]
mod tests;
