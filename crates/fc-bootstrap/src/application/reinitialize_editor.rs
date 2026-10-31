//! Explicit destructive reinitialization consent. This editor owns no path,
//! credential, database or filesystem capability; preparation and the final
//! irreversible operation remain the coordinator's responsibility.
use fc_presentation::workbench::*;

#[cfg(test)]
#[path = "reinitialize_editor/tests.rs"]
mod tests;

pub(super) const PREPARE: ActionId = ActionId(700);
pub(super) const CONFIRM: ActionId = ActionId(701);
pub(super) const CANCEL: ActionId = ActionId(702);
pub(super) const ACK_DATA_LOSS: FieldId = FieldId(700);
pub(super) const ACK_NEW_INSTALLATION: FieldId = FieldId(701);

pub(super) enum Command {
    Prepare,
    Cancel,
    Confirm { sequence: u64 },
}
#[derive(Clone, Copy)]
enum Stage {
    Intro,
    Pending,
    Review,
    Confirmed,
    Invalid,
}
pub(super) struct Editor {
    sequence: u64,
    stage: Stage,
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
    pub fn new(sequence: u64) -> Self {
        Self {
            sequence,
            stage: if sequence == 0 {
                Stage::Invalid
            } else {
                Stage::Intro
            },
        }
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn invalidate(&mut self) {
        self.stage = Stage::Invalid;
    }
    /// Only the current preparation's successful completion may open consent.
    /// Merely rendering the page or submitting form fields cannot do so.
    pub fn set_review(&mut self, sequence: u64) -> Result<(), String> {
        if sequence != self.sequence || !matches!(self.stage, Stage::Pending) {
            return Err("本次重新初始化准备已失效，请重新进入并核对。".into());
        }
        self.stage = Stage::Review;
        Ok(())
    }
    pub fn view(&self, status: &str, busy: bool) -> View {
        let mut description = "重新初始化会清除本机未来教室的全部教学数据、资料和图片、学校与账号、密码和恢复码、设备配对以及语音数据。当前部署目录内导出的备份及缓存一并清除；已另存到部署目录以外的备份副本不在本次清除范围。\n本操作不需要现有密码或恢复码，也不会找回旧密码或解锁原账号；请仅在明确决定放弃当前全部本机资料时使用。\n完成后当前程序关闭，重新启动后始终必须重新设置学校并创建新的主管理员。新部署不会读取旧部署保留的隔离资料，也不提供从这些资料恢复原账号或教学数据的入口。\n不会重置 Windows、Windows 登录账号或其他应用，不会更改 Windows 现有自动启动、网络及 SSH 授权和设置。".to_string();
        let action = |id, label: &str, enabled| Action {
            id,
            label: label.into(),
            enabled,
        };
        let mut fields = Vec::new();
        let actions = match self.stage {
            Stage::Intro => {
                description.push_str("\n本页尚未执行清除。下一步先由后台核对本机数据目录并准备操作，准备成功后仍需分别确认资料丢失和全新设置要求。准备和复核阶段均可取消。");
                vec![
                    action(PREPARE, "核对本机资料并准备重新初始化", !busy),
                    action(CANCEL, "取消并返回", true),
                ]
            }
            Stage::Pending => {
                description.push_str("\n正在后台核对和准备；尚未授权清除正式资料。可以取消，后台工作会安全结束后清理本次准备。");
                vec![action(CANCEL, "取消本次准备", true)]
            }
            Stage::Review => {
                description.push_str("\n本次本机目录核对与准备已完成。以下两项必须分别明确选择；默认均未确认。确认前可取消，正式资料不会被本次准备清除。\n确认清除后不能从此页撤回或继续编辑原资料，程序会进入受控关闭与重新初始化。不要依赖本操作自动保留备份或恢复原数据；如仍需当前资料，请现在取消。随后必须重新启动并完成全新设置。");
                for (id, label, value, confirmed_label) in [
                    (
                        ACK_DATA_LOSS,
                        "确认放弃当前全部本机未来教室资料",
                        format!("erase-data-{}", self.sequence),
                        "我理解教学数据、全部账号、密码、恢复码、配对和语音数据将被清除",
                    ),
                    (
                        ACK_NEW_INSTALLATION,
                        "确认重新启动并进行全新设置",
                        format!("new-setup-{}", self.sequence),
                        "我理解程序将关闭，重新启动后须设置学校并创建新的主管理员",
                    ),
                ] {
                    fields.push(Field {
                        id,
                        label: label.into(),
                        kind: FieldKind::Choice(vec![
                            Choice {
                                value: "0".into(),
                                label: "尚未确认".into(),
                            },
                            Choice {
                                value,
                                label: confirmed_label.into(),
                            },
                        ]),
                        value: "0".into(),
                        max_utf16: 40,
                    });
                }
                vec![
                    action(CONFIRM, "清除全部本机资料并关闭程序", !busy),
                    action(CANCEL, "取消，保留当前资料", true),
                ]
            }
            Stage::Confirmed => {
                description.push_str("\n本次清除确认已提交，不能重复提交或返回编辑原资料。请等待实际后台关闭与核对结果，再按提示重新启动程序。");
                Vec::new()
            }
            Stage::Invalid => {
                description.push_str(
                    "\n本次页面已取消或失效，不能用于确认清除。如需继续，必须重新进入并重新核对。",
                );
                Vec::new()
            }
        };
        View {
            title: "清除全部本机资料并重新初始化".into(),
            description,
            status: controlled_status(status),
            fields,
            actions,
            busy,
            grid: None,
        }
    }
    pub fn event(&mut self, action: ActionId, mut data: FormData) -> Result<Command, String> {
        if matches!(self.stage, Stage::Invalid | Stage::Confirmed) {
            return Err("本次重新初始化页面已失效，不能重复确认或恢复使用。".into());
        }
        match action {
            PREPARE if matches!(self.stage, Stage::Intro) => {
                if !data.is_empty() {
                    return Err("本次准备不接收表单参数，请重新进入核对。".into());
                }
                self.stage = Stage::Pending;
                Ok(Command::Prepare)
            }
            CANCEL => {
                self.stage = Stage::Invalid;
                Ok(Command::Cancel)
            }
            CONFIRM if matches!(self.stage, Stage::Review) => {
                let data_loss = format!("erase-data-{}", self.sequence);
                let new_installation = format!("new-setup-{}", self.sequence);
                let submitted_data_loss = data.take(ACK_DATA_LOSS);
                let submitted_new_installation = data.take(ACK_NEW_INSTALLATION);
                if !data.is_empty() {
                    return Err("本次清除确认包含未识别字段，请重新核对两项影响。".into());
                }
                if submitted_data_loss.as_deref() != Some(data_loss.as_str())
                    || submitted_new_installation.as_deref() != Some(new_installation.as_str())
                {
                    return Err("请分别核对并明确确认全部资料丢失，以及重新启动后全新设置学校和主管理员的要求。".into());
                }
                self.stage = Stage::Confirmed;
                Ok(Command::Confirm {
                    sequence: self.sequence,
                })
            }
            _ => Err("当前步骤不接纳此操作；必须先完成本次准备，再分别确认两项影响。".into()),
        }
    }
}
