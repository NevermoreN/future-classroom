//! Captured current-lesson command. Reload is explicit: a stale draft is never
//! rebound to the next lesson merely because a save was rejected.
use super::*;
use fc_domain::{lesson_context::*, schedule::*};

pub(super) const SUBJECT: FieldId = FieldId(400);
pub(super) const TEACHER: FieldId = FieldId(401);
pub(super) const RELOAD: ActionId = ActionId(400);
pub(super) const REPLACE: ActionId = ActionId(401);
pub(super) const RESTORE: ActionId = ActionId(402);
pub(super) const CONFIRM_UNASSIGNED: ActionId = ActionId(403);
const CANCEL_UNASSIGNED: ActionId = ActionId(404);

pub(super) enum Command {
    Stay,
    Close,
    Lock,
    Reload,
    Apply {
        expected: Revision,
        lesson: LessonInstanceKey,
        action: LessonAction,
    },
}

pub(super) struct Editor {
    pub(super) snapshot: LessonContextSnapshot,
    subject: Option<SubjectId>,
    teacher: Option<PrincipalId>,
    confirm_unassigned: bool,
}
fn teachers(identity: &IdentityState, class: ClassId, subject: SubjectId) -> Vec<Choice> {
    identity
        .accounts
        .iter()
        .filter(|a| identity.is_teacher(a.principal.id, class, subject))
        .map(|a| Choice {
            value: a.principal.id.get().to_string(),
            label: format!(
                "{}{}（{}）",
                a.display_name.chars().take(50).collect::<String>(),
                if a.display_name.chars().count() > 50 {
                    "…"
                } else {
                    ""
                },
                a.login_name
            ),
        })
        .collect()
}
fn selected_subject(data: &FormData) -> Option<SubjectId> {
    data.get(SUBJECT)?
        .parse()
        .ok()
        .and_then(|id| SubjectId::new(id).ok())
}
fn selected_teacher(data: &FormData) -> Option<PrincipalId> {
    data.get(TEACHER)?
        .parse()
        .ok()
        .and_then(|id| PrincipalId::new(id).ok())
}
impl Editor {
    pub(super) fn new(snapshot: LessonContextSnapshot) -> Self {
        let subject = snapshot.current.as_ref().map(|c| c.effective.subject_id);
        let teacher = snapshot.current.as_ref().and_then(|c| c.teacher_id);
        Self {
            snapshot,
            subject,
            teacher,
            confirm_unassigned: false,
        }
    }
    pub(super) fn input(&mut self, data: FormData, identity: &IdentityState) -> bool {
        if self.confirm_unassigned {
            return false;
        }
        let subject = selected_subject(&data);
        if subject == self.subject {
            self.teacher = selected_teacher(&data);
            return false;
        }
        self.subject = subject;
        self.confirm_unassigned = false;
        self.teacher = self
            .snapshot
            .current
            .as_ref()
            .and_then(|c| subject.map(|s| teachers(identity, c.key.class_id, s)))
            .filter(|v| v.len() == 1)
            .and_then(|v| v[0].value.parse().ok())
            .and_then(|id| PrincipalId::new(id).ok());
        true
    }
    pub(super) fn view(&self, identity: &IdentityState, status: &str, busy: bool) -> View {
        let mut view = View {
            title: "本节临时换课".into(),
            description: "仅影响当前这一节，下课自动结束。课程安排变化后，请重新加载再操作。"
                .into(),
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
        if let Some(current) = &self.snapshot.current {
            if self.confirm_unassigned {
                view.description="此科目在本班没有有效任教教师。确认后本节将显示‘未分配老师’，课堂资料保持空白。".into();
                view.actions = vec![
                    action(CONFIRM_UNASSIGNED, "确认以未分配老师换课", true),
                    action(CANCEL_UNASSIGNED, "返回选择", true),
                    action(LOCK, "锁定工作台", true),
                ];
                return view;
            }
            let name = |id| {
                identity
                    .subjects
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.name.as_str())
                    .unwrap_or("科目已失效")
            };
            let teacher = current
                .teacher_id
                .and_then(|id| identity.account(id).ok())
                .map(|a| a.display_name.as_str())
                .unwrap_or("任课教师待确认");
            view.description=format!("{:02}:{:02}–{:02}:{:02}　原课：{}；当前：{} / {}。\n仅本节有效，下课后恢复基础课表。可随时恢复原课；课程安排变化后请重新加载。",current.key.range.begin.get()/60,current.key.range.begin.get()%60,current.key.range.end.get()/60,current.key.range.end.get()%60,name(current.base.subject_id),name(current.effective.subject_id),teacher);
            let mut subjects: Vec<_> = identity
                .subjects
                .iter()
                .filter(|s| s.enabled)
                .map(|s| Choice {
                    value: s.id.get().to_string(),
                    label: s.name.clone(),
                })
                .collect();
            let subject_value = self
                .subject
                .map(|s| s.get().to_string())
                .unwrap_or_default();
            if !subject_value.is_empty() && !subjects.iter().any(|c| c.value == subject_value) {
                subjects.push(Choice {
                    value: subject_value.clone(),
                    label: "原科目已失效，请重新选择".into(),
                });
            }
            let mut candidates = vec![Choice {
                value: "choose".into(),
                label: "请选择任课教师".into(),
            }];
            if let Some(subject) = self.subject {
                let allowed = teachers(identity, current.key.class_id, subject);
                if allowed.is_empty() {
                    candidates = vec![Choice {
                        value: "unassigned".into(),
                        label: "未分配老师（保存时需确认）".into(),
                    }];
                } else {
                    candidates.extend(allowed);
                }
            }
            let teacher_value = self
                .teacher
                .map(|s| s.get().to_string())
                .unwrap_or_else(|| candidates[0].value.clone());
            if !candidates.iter().any(|c| c.value == teacher_value) {
                candidates.push(Choice {
                    value: teacher_value.clone(),
                    label: "原任教关系已失效，请重新选择".into(),
                });
            }
            view.fields = vec![
                Field {
                    id: SUBJECT,
                    label: "临时科目".into(),
                    kind: FieldKind::Choice(subjects),
                    value: subject_value,
                    max_utf16: 32,
                },
                Field {
                    id: TEACHER,
                    label: "临时任课教师".into(),
                    kind: FieldKind::Choice(candidates),
                    value: teacher_value,
                    max_utf16: 32,
                },
            ];
            view.actions.push(action(REPLACE, "保存本节临时换课", true));
            view.actions.push(action(
                RESTORE,
                "恢复本节原课",
                self.snapshot.active_override.is_some(),
            ));
        } else {
            view.description="当前不是已确认的课程时段，不能临时换课。课间、午休、活动、休息日和待确认安排均不接纳提交；进入课程时段后请重新加载。".into();
        }
        view.actions.extend([
            action(RELOAD, "重新加载当前课程", true),
            action(BACK, "返回工作台", true),
            action(LOCK, "锁定工作台", true),
        ]);
        view
    }
    pub(super) fn handle(
        &mut self,
        action: ActionId,
        data: FormData,
        identity: &IdentityState,
    ) -> std::result::Result<Command, String> {
        match action {
            BACK => return Ok(Command::Close),
            LOCK => return Ok(Command::Lock),
            RELOAD => return Ok(Command::Reload),
            _ => {}
        }
        if action == CANCEL_UNASSIGNED {
            self.confirm_unassigned = false;
            return Ok(Command::Stay);
        }
        let current = self
            .snapshot
            .current
            .as_ref()
            .ok_or("当前不是课程时段，请重新加载后核对。")?;
        let change = if action == RESTORE {
            if self.snapshot.active_override.is_none() {
                return Err("本节没有可恢复的临时换课。".into());
            }
            LessonAction::Restore
        } else if action == REPLACE || action == CONFIRM_UNASSIGNED {
            let subject = if action == CONFIRM_UNASSIGNED {
                self.subject
            } else {
                selected_subject(&data)
            }
            .ok_or("请选择有效科目。")?;
            if !identity
                .subjects
                .iter()
                .any(|s| s.id == subject && s.enabled)
            {
                return Err("请选择有效科目。".into());
            }
            let candidates = teachers(identity, current.key.class_id, subject);
            if candidates.is_empty() {
                if action == REPLACE {
                    self.subject = Some(subject);
                    self.teacher = None;
                    self.confirm_unassigned = true;
                    return Ok(Command::Stay);
                }
                if !self.confirm_unassigned {
                    return Err("请先选择科目并确认未分配老师。".into());
                }
                LessonAction::Replace(CourseCell {
                    subject_id: subject,
                    teacher: TeacherBinding::Unassigned,
                })
            } else {
                if action == CONFIRM_UNASSIGNED {
                    return Err("任教候选已变化，请重新加载并选择教师。".into());
                }
                let teacher = selected_teacher(&data).ok_or("请选择任教候选教师。")?;
                self.subject = Some(subject);
                self.teacher = Some(teacher);
                if !identity.is_teacher(teacher, current.key.class_id, subject) {
                    return Err("该教师不在本班此科目的有效任教范围内。".into());
                }
                LessonAction::Replace(CourseCell {
                    subject_id: subject,
                    teacher: TeacherBinding::Explicit(teacher),
                })
            }
        } else {
            return Err("此换课操作不可用。".into());
        };
        Ok(Command::Apply {
            expected: self.snapshot.revision,
            lesson: current.key.clone(),
            action: change,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::time::*;
    fn fixture() -> (Editor, IdentityState) {
        let class = ClassId::new(1).unwrap();
        let school = SchoolId::new(1).unwrap();
        let subject = SubjectId::new(1).unwrap();
        let teacher = PrincipalId::new(2).unwrap();
        let mut identity = IdentityState {
            revision: Revision::new(5),
            installation: Some(Installation {
                school_id: school,
                school_name: "Lesson test".into(),
                current_class: Some(class),
            }),
            classes: vec![SchoolClass {
                id: class,
                school_id: school,
                name: "Class".into(),
                enabled: true,
            }],
            subjects: vec![],
            accounts: vec![],
            assignments: vec![TeachingAssignment {
                teacher_id: teacher,
                class_id: class,
                subject_id: subject,
                enabled: true,
            }],
            default_teachers: vec![],
        };
        for id in 1..=2 {
            identity.subjects.push(SchoolSubject {
                id: SubjectId::new(id).unwrap(),
                school_id: school,
                name: format!("Subject {id}"),
                enabled: true,
                color: Rgb {
                    red: 0,
                    green: 100,
                    blue: 200,
                },
                revision: Revision::new(1),
            });
            identity.accounts.push(UserAccount {
                principal: Principal {
                    id: PrincipalId::new(id).unwrap(),
                    school_id: school,
                    role: if id == 1 {
                        Role::GlobalAdmin
                    } else {
                        Role::TeacherAdmin
                    },
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("user{id}"),
                display_name: format!("User {id}"),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            });
        }
        let date = LocalDate::new(2026, 10, 30).unwrap();
        let range = MinuteRange::new(Minute::new(480).unwrap(), Minute::new(540).unwrap()).unwrap();
        let key = LessonInstanceKey {
            actual_date: date,
            class_id: class,
            range,
            version: ResolvedScheduleVersion {
                actual_semester_id: SemesterId::new(1).unwrap(),
                actual_revision: Revision::new(2),
                source_semester_id: SemesterId::new(1).unwrap(),
                source_revision: Revision::new(2),
                calendar_mode: CalendarMode::School,
                calendar_mode_revision: Revision::new(1),
                applied_calendar_revision: Revision::new(1),
                makeup_mapping_revision: Revision::new(1),
                source_table_id: TableId::new(1).unwrap(),
                source_row_id: RowId::new(1).unwrap(),
                rules_revision: Revision::new(1),
            },
        };
        let cell = CourseCell {
            subject_id: subject,
            teacher: TeacherBinding::Explicit(teacher),
        };
        let snapshot = LessonContextSnapshot {
            revision: Revision::new(7),
            identity_revision: identity.revision,
            observed_at: Some(LocalDateTime::new(date, 480 * 60).unwrap()),
            class_id: Some(class),
            current: Some(EffectiveLessonContext {
                key,
                base: cell.clone(),
                effective: cell,
                teacher_id: Some(teacher),
                effective_from: LocalDateTime::new(date, 480 * 60).unwrap(),
                effective_until: LocalDateTime::new(date, 540 * 60).unwrap(),
            }),
            ..LessonContextSnapshot::default()
        };
        (Editor::new(snapshot), identity)
    }
    fn form(editor: &Editor, identity: &IdentityState) -> FormData {
        let mut data = FormData::new();
        for field in editor.view(identity, "", false).fields {
            data.insert(field.id, field.value).unwrap();
        }
        data
    }
    #[test]
    fn submission_carries_original_lesson_and_revision_and_requires_explicit_unassigned_confirmation(
    ) {
        let (mut editor, identity) = fixture();
        editor.view(&identity, "", false).validate().unwrap();
        let data = form(&editor, &identity);
        let Command::Apply {
            expected,
            lesson,
            action: LessonAction::Replace(target),
        } = editor.handle(REPLACE, data, &identity).unwrap()
        else {
            panic!()
        };
        assert_eq!(expected, Revision::new(7));
        assert_eq!(lesson, editor.snapshot.current.as_ref().unwrap().key);
        assert_eq!(
            target.teacher,
            TeacherBinding::Explicit(PrincipalId::new(2).unwrap())
        );
        let mut data = form(&editor, &identity);
        data.take(SUBJECT);
        data.insert(SUBJECT, "2".into()).unwrap();
        assert!(editor.input(data, &identity));
        let data = form(&editor, &identity);
        assert!(matches!(
            editor.handle(REPLACE, data, &identity),
            Ok(Command::Stay)
        ));
        editor.view(&identity, "", false).validate().unwrap();
        assert!(matches!(
            editor.handle(CONFIRM_UNASSIGNED, FormData::new(), &identity),
            Ok(Command::Apply {
                action: LessonAction::Replace(CourseCell {
                    teacher: TeacherBinding::Unassigned,
                    ..
                }),
                ..
            })
        ));
        assert_eq!(editor.snapshot.revision, Revision::new(7));
    }
    #[test]
    fn non_course_has_no_save_actions_and_cannot_submit_the_next_lesson() {
        let (mut editor, identity) = fixture();
        editor.snapshot.current = None;
        let view = editor.view(&identity, "", false);
        view.validate().unwrap();
        assert!(!view
            .actions
            .iter()
            .any(|a| a.id == REPLACE || a.id == RESTORE));
        assert!(editor.handle(REPLACE, FormData::new(), &identity).is_err());
        assert!(matches!(
            editor.handle(RELOAD, FormData::new(), &identity),
            Ok(Command::Reload)
        ));
    }
}
