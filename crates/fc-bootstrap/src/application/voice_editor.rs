//! Local administrator consent for the remote voice setting. The editor owns
//! neither an audio device nor a connection service; only a fresh backend
//! snapshot can enable another change after submission or invalidation.
use fc_bootstrap::backend::VoiceManagementSnapshot;
use fc_domain::ids::{PrincipalId, Revision};
use fc_presentation::workbench::*;

#[cfg(test)]
#[path = "voice_editor/tests.rs"]
mod tests;

pub(super) const ENABLE: ActionId = ActionId(850);
pub(super) const DISABLE: ActionId = ActionId(851);
pub(super) const CONFIRM: ActionId = ActionId(852);
pub(super) const CANCEL: ActionId = ActionId(853);
pub(super) const REFRESH: ActionId = ActionId(854);
pub(super) const CLOSE: ActionId = ActionId(855);
pub(super) const ACK: FieldId = FieldId(850);

pub(super) enum Command {
    Stay,
    Close,
    Refresh,
    SetEnabled {
        expected: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
        enabled: bool,
    },
}

struct Review {
    identity_revision: Revision,
    expected: Revision,
    runtime_generation: u64,
    enabled: bool,
}

pub(super) struct Editor {
    owner: PrincipalId,
    snapshot: VoiceManagementSnapshot,
    review: Option<Review>,
    requires_refresh: bool,
}

fn controlled_status(status: &str) -> String {
    status
        .chars()
        .take(2048)
        .map(|c| {
            if c.is_control()
                || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}

impl Editor {
    pub fn new(owner: PrincipalId, snapshot: VoiceManagementSnapshot) -> Self {
        Self {
            owner,
            snapshot,
            review: None,
            requires_refresh: false,
        }
    }

    pub fn owner(&self) -> PrincipalId {
        self.owner
    }

    /// The coordinator checks the current local administrator lease before
    /// delivering a successful load. Every load discards the previous consent,
    /// even when its configuration revision happens to be unchanged.
    pub fn loaded(&mut self, snapshot: VoiceManagementSnapshot) {
        self.snapshot = snapshot;
        self.review = None;
        self.requires_refresh = false;
    }

    pub fn invalidate(&mut self) {
        self.review = None;
        self.requires_refresh = true;
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.review.is_some()
    }

    pub fn view(&self, status: &str, busy: bool) -> View {
        let mut description = format!(
            "远程语音配置：{}\n接收新播报：{}\n活跃任务：{}；排队任务：{}；播放中任务：{}。这些计数来自最近一次读取。\n教师连接服务当前未就绪，配置开启不表示已提供教师连接服务。\n播放限制：全机最多 4 个任务，每位教师最多 2 个；单段 0.5–60 秒；播放 1–10 遍，默认 3 遍，遍间隔 1 秒。\n关闭会取消全部当前和排队任务；物理播放停止尚未完成时，不能声称已停止。重新开启后，须由教师发起新的动作，不恢复旧任务。\n配置开关不能证明音频设备可用，也不能证明教室已听到声音。",
            if self.snapshot.enabled {
                "开启"
            } else {
                "关闭"
            },
            if self.snapshot.admitting {
                "允许（教师连接服务须另行就绪）"
            } else {
                "关闭"
            },
            self.snapshot.active_jobs,
            self.snapshot.queued_jobs,
            self.snapshot.playing_jobs,
        );
        if !self.snapshot.enabled
            && (self.snapshot.active_jobs != 0 || self.snapshot.playing_jobs != 0)
        {
            description.push_str(
                "\n配置已关闭，但最近计数仍有未结束任务；停止结果待实际完成后重新读取核对。",
            );
        }
        if self.requires_refresh {
            description.push_str("\n本次状态已失效或变更已提交，请重新读取后再操作。");
        }
        let action = |id, label: &str, enabled| Action {
            id,
            label: label.into(),
            enabled,
        };
        let mut fields = Vec::new();
        let actions = if let Some(review) = &self.review {
            description.push_str(if review.enabled {
                "\n待确认：开启远程语音配置。旧任务不会恢复，教师须发起新的动作；连接服务仍须另行就绪。"
            } else {
                "\n待确认：关闭远程语音配置，并取消全部当前和排队任务。物理播放停止结果须等待实际完成并核对。"
            });
            fields.push(Field {
                id: ACK,
                label: "确认本次远程语音配置变更的影响".into(),
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
                action(CONFIRM, "确认变更", !busy),
                action(CANCEL, "取消本次确认", !busy),
            ]
        } else {
            let can_change = !busy && !self.requires_refresh;
            vec![
                action(
                    ENABLE,
                    "开启远程语音配置",
                    can_change && !self.snapshot.enabled,
                ),
                action(
                    DISABLE,
                    "关闭远程语音配置",
                    can_change && self.snapshot.enabled,
                ),
                action(REFRESH, "重新读取状态", !busy),
                action(CLOSE, "返回工作台", !busy),
            ]
        };
        View {
            title: "远程语音设置".into(),
            description,
            status: controlled_status(status),
            fields,
            actions,
            busy,
            grid: None,
        }
    }

    pub fn handle(&mut self, action: ActionId, mut data: FormData) -> Result<Command, String> {
        // Native forms submit their visible acknowledgement on Cancel as well.
        // Cancel discards it without treating the checked value as consent.
        if action == CANCEL && self.review.is_some() {
            let _ = data.take(ACK);
        }
        if action != CONFIRM && !data.is_empty() {
            return Err("本次操作不接收配置、版本或其他表单参数，请重新核对。".into());
        }
        match action {
            ENABLE | DISABLE => {
                if self.requires_refresh {
                    return Err("请先重新读取远程语音状态，再确认新的变更。".into());
                }
                if self.review.is_some() {
                    return Err("请先确认或取消当前变更。".into());
                }
                let enabled = action == ENABLE;
                if enabled == self.snapshot.enabled {
                    return Err("配置已为所选状态，请重新读取核对。".into());
                }
                self.review = Some(Review {
                    identity_revision: self.snapshot.identity_revision,
                    expected: self.snapshot.revision,
                    runtime_generation: self.snapshot.runtime_generation,
                    enabled,
                });
                Ok(Command::Stay)
            }
            CONFIRM => {
                let acknowledgement = data.take(ACK);
                if !data.is_empty() || acknowledgement.as_deref() != Some("1") {
                    return Err("请核对本次影响并明确确认；配置与版本不能由表单指定。".into());
                }
                let review = self.review.as_ref().ok_or("当前没有待确认的变更。")?;
                if self.requires_refresh
                    || review.expected != self.snapshot.revision
                    || review.identity_revision != self.snapshot.identity_revision
                    || review.runtime_generation != self.snapshot.runtime_generation
                {
                    self.invalidate();
                    return Err("本次确认已失效，请重新读取状态。".into());
                }
                let review = self.review.take().ok_or("当前没有待确认的变更。")?;
                self.requires_refresh = true;
                Ok(Command::SetEnabled {
                    expected: review.expected,
                    expected_identity: review.identity_revision,
                    expected_runtime: review.runtime_generation,
                    enabled: review.enabled,
                })
            }
            CANCEL => {
                self.review = None;
                Ok(Command::Stay)
            }
            REFRESH => {
                self.invalidate();
                Ok(Command::Refresh)
            }
            CLOSE => {
                self.invalidate();
                Ok(Command::Close)
            }
            _ => Err("当前远程语音页面不接纳此操作。".into()),
        }
    }
}
