//! A review captures intent, revision and physical preimage. Form fields never
//! supply a command, registry scope or privileged action.
use fc_bootstrap::autostart::Loaded;
use fc_domain::{
    autostart::*,
    ids::{PrincipalId, Revision},
};
use fc_presentation::workbench::*;
#[cfg(test)]
#[path = "autostart_editor_tests.rs"]
mod tests;

pub(super) const ENABLE: ActionId = ActionId(650);
pub(super) const DISABLE: ActionId = ActionId(651);
pub(super) const CONFIRM: ActionId = ActionId(652);
pub(super) const CANCEL: ActionId = ActionId(653);
pub(super) const REFRESH: ActionId = ActionId(654);
pub(super) const CLOSE: ActionId = ActionId(655);
pub(super) const ACK: FieldId = FieldId(650);
const TARGET: FieldId = FieldId(651);
const ACTUAL: FieldId = FieldId(652);

pub(super) enum Command {
    Stay,
    Close,
    Load,
    Change {
        expected: Revision,
        desired: bool,
        previous_observed: AutostartObservation,
        reviewed_target: String,
    },
}
struct Review {
    expected: Revision,
    desired: bool,
    previous_observed: AutostartObservation,
    reviewed_target: String,
}
pub(super) struct Editor {
    owner: PrincipalId,
    loaded: Loaded,
    review: Option<Review>,
    requires_refresh: bool,
}
fn display(command: &str) -> String {
    command
        .chars()
        .take(1024)
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}
fn observation(observed: &AutostartObservation, target: &str) -> String {
    match observed {
        AutostartObservation::Absent => "关闭：未发现本项目启动项".into(),
        AutostartObservation::Command(command) if command == target => "开启：指向当前程序".into(),
        AutostartObservation::Command(command) => {
            format!("发现其他路径或命令：{}", display(command))
        }
        AutostartObservation::Opaque { .. } => {
            "无法安全读取：启动项格式或长度异常，请先核对".into()
        }
        AutostartObservation::Unavailable(_) => "读取失败：实际状态尚未确认".into(),
    }
}
pub(super) fn phase(phase: AutostartPhase) -> &'static str {
    match phase {
        AutostartPhase::Prepared => "变更意图已保存",
        AutostartPhase::Applying => "正在应用，结果待核对",
        AutostartPhase::Succeeded => "已核对为目标状态",
        AutostartPhase::Failed => "本次变更失败",
        AutostartPhase::NeedsReconcile => "结果未确认，需重新读取并核对",
    }
}
impl Editor {
    pub fn new(owner: PrincipalId, loaded: Loaded) -> Self {
        Self {
            owner,
            loaded,
            review: None,
            requires_refresh: false,
        }
    }
    pub fn owner(&self) -> PrincipalId {
        self.owner
    }
    pub fn loaded(&self) -> &Loaded {
        &self.loaded
    }
    pub fn has_unsaved_changes(&self) -> bool {
        self.review.is_some()
    }
    pub fn invalidate(&mut self) {
        self.review = None;
        self.requires_refresh = true;
    }
    pub fn view(&self, status: &str, busy: bool) -> View {
        let enabled = !busy;
        let mut description = format!("期望状态：{}\n实际状态：{}\n最近结果：{}\n当前 Windows 用户登录时启动。移动 ZIP 程序目录后，请在新位置重新核对并明确修复。",
            if self.loaded.state.as_ref().is_some_and(|s| s.desired) { "开启" } else { "关闭" },
            observation(&self.loaded.observed, &self.loaded.target_command),
            self.loaded.state.as_ref().and_then(AutostartState::current).map_or("尚无操作记录", |o| phase(o.phase)));
        if self
            .loaded
            .state
            .as_ref()
            .is_some_and(AutostartState::drifted)
        {
            description
                .push_str("\n期望与实际状态不一致，请核对后明确选择；程序不会自动反复修复。");
        }
        let mut fields = vec![
            Field {
                id: TARGET,
                label: "当前程序的启动命令".into(),
                kind: FieldKind::ReadOnly,
                value: display(&self.loaded.target_command),
                max_utf16: 1024,
            },
            Field {
                id: ACTUAL,
                label: "本次读取的实际状态".into(),
                kind: FieldKind::ReadOnly,
                value: observation(&self.loaded.observed, &self.loaded.target_command),
                max_utf16: 2048,
            },
        ];
        let action = |id, label: &str, enabled| Action {
            id,
            label: label.into(),
            enabled,
        };
        let actions = if let Some(review) = &self.review {
            description.push_str(if review.desired {
                "\n确认后将本项目启动项设为上方当前程序命令。"
            } else {
                "\n确认后将删除本项目启动项。"
            });
            fields.push(Field {
                id: ACK,
                label: "核对当前路径与实际状态".into(),
                kind: FieldKind::Choice(vec![
                    Choice {
                        value: "0".into(),
                        label: "尚未确认".into(),
                    },
                    Choice {
                        value: "1".into(),
                        label: "已理解并确认本次变更".into(),
                    },
                ]),
                value: "0".into(),
                max_utf16: 1,
            });
            vec![
                action(CONFIRM, "确认变更", enabled),
                action(CANCEL, "取消本次确认", enabled),
            ]
        } else {
            let can_change = enabled
                && !self.requires_refresh
                && self.loaded.state.is_some()
                && self.loaded.observed.mutation_eligible();
            vec![
                action(ENABLE, "开启或修复到当前路径", can_change),
                action(DISABLE, "关闭登录自启动", can_change),
                action(REFRESH, "重新读取实际状态", enabled),
                action(CLOSE, "返回工作台", enabled),
            ]
        };
        View {
            title: "登录自启动".into(),
            description,
            status: status.into(),
            fields,
            actions,
            busy,
            grid: None,
        }
    }
    pub fn event(&mut self, action: ActionId, data: FormData) -> Result<Command, String> {
        match action {
            ENABLE | DISABLE if self.review.is_none() => {
                if self.requires_refresh {
                    return Err("请先重新读取实际状态，再确认新的变更。".into());
                }
                if !self.loaded.observed.mutation_eligible() {
                    return Err("实际启动项尚未安全读取，请先核对。".into());
                }
                let state = self.loaded.state.as_ref().ok_or("部署尚未初始化。")?;
                self.review = Some(Review {
                    expected: state.revision,
                    desired: action == ENABLE,
                    previous_observed: self.loaded.observed.clone(),
                    reviewed_target: self.loaded.target_command.clone(),
                });
                Ok(Command::Stay)
            }
            CONFIRM => {
                if data.get(ACK) != Some("1") {
                    return Err("请先核对路径与实际状态，再明确确认本次变更。".into());
                }
                let review = self.review.take().ok_or("当前没有待确认的变更。")?;
                Ok(Command::Change {
                    expected: review.expected,
                    desired: review.desired,
                    previous_observed: review.previous_observed,
                    reviewed_target: review.reviewed_target,
                })
            }
            CANCEL => {
                self.review = None;
                Ok(Command::Stay)
            }
            REFRESH => {
                self.review = None;
                Ok(Command::Load)
            }
            CLOSE => {
                self.review = None;
                Ok(Command::Close)
            }
            _ => Ok(Command::Stay),
        }
    }
}
