//! Local administrator consent for the remote voice setting. The editor owns
//! neither an audio device nor a connection service; only a fresh backend
//! snapshot can enable another change after submission or invalidation.
use fc_application::identity::SessionLease;
use fc_bootstrap::backend::{
    VoiceDeviceCheckResult, VoiceDeviceObservation, VoiceManagementSnapshot,
};
use fc_domain::{
    ids::{PrincipalId, Revision},
    time::LocalDateTime,
    voice::VoiceLimits,
};
use fc_presentation::workbench::*;
use std::time::Instant;

#[cfg(test)]
#[path = "voice_editor/tests.rs"]
mod tests;

pub(super) const ENABLE: ActionId = ActionId(850);
pub(super) const DISABLE: ActionId = ActionId(851);
pub(super) const CONFIRM: ActionId = ActionId(852);
pub(super) const CANCEL: ActionId = ActionId(853);
pub(super) const REFRESH: ActionId = ActionId(854);
pub(super) const CLOSE: ActionId = ActionId(855);
pub(super) const SAVE_LIMITS: ActionId = ActionId(856);
pub(super) const DISCARD_LIMITS: ActionId = ActionId(857);
pub(super) const CHECK_OUTPUT: ActionId = ActionId(858);
pub(super) const ACK: FieldId = FieldId(850);
pub(super) const DURATION_SECONDS: FieldId = FieldId(851);
pub(super) const PENDING_JOBS: FieldId = FieldId(852);
pub(super) const PENDING_PER_OWNER: FieldId = FieldId(853);
pub(super) const REPEAT_TOTAL: FieldId = FieldId(854);

pub(super) enum Command {
    Stay,
    Close,
    Refresh,
    CheckOutput {
        expected_publication: Revision,
        expected: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
    },
    SetLimits {
        expected: Revision,
        expected_identity: Revision,
        expected_runtime: u64,
        limits: VoiceLimits,
    },
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
    target: Target,
}
/// Presentation metadata only. The controller accepts the backend's exact
/// lease receipt before this projection is created; it owns no audio handle.
struct DeviceDisplay {
    result: VoiceDeviceCheckResult,
    observed_at: LocalDateTime,
    expires: Instant,
    current: bool,
}
#[derive(Clone, Copy)]
enum Target {
    Enabled(bool),
    Limits(VoiceLimits),
}
#[derive(Clone, PartialEq, Eq)]
struct Draft {
    values: [String; 4],
}
fn seconds(millis: u32) -> String {
    let whole = millis / 1000;
    let fraction = millis % 1000;
    if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fraction:03}")
            .trim_end_matches('0')
            .to_owned()
    }
}
impl From<VoiceLimits> for Draft {
    fn from(limits: VoiceLimits) -> Self {
        Self {
            values: [
                seconds(limits.max_duration_millis),
                limits.max_pending_jobs.to_string(),
                limits.max_pending_per_owner.to_string(),
                limits.max_repeat_total.to_string(),
            ],
        }
    }
}
impl Draft {
    fn parse(&self) -> Result<VoiceLimits, String> {
        let duration = &self.values[0];
        let mut parts = duration.split('.');
        let whole = parts.next().unwrap_or("");
        let fraction = parts.next();
        if whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || parts.next().is_some()
            || fraction.is_some_and(|s| {
                s.is_empty() || s.len() > 3 || !s.bytes().all(|b| b.is_ascii_digit())
            })
        {
            return Err("时长须为 0.5–60 秒的十进制数，最多三位小数。".into());
        }
        let integer = whole.parse::<u32>().map_err(|_| "时长超出范围。")?;
        let decimal = match fraction {
            None => 0,
            Some(value) => {
                value.parse::<u32>().map_err(|_| "时长格式不正确。")?
                    * 10u32.pow(3 - value.len() as u32)
            }
        };
        let max_duration_millis = integer
            .checked_mul(1000)
            .and_then(|v| v.checked_add(decimal))
            .ok_or("时长超出范围。")?;
        let number = |text: &str| -> Result<u8, String> {
            if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err("任务数和遍数须为十进制整数。".into());
            }
            text.parse::<u8>()
                .map_err(|_| "任务数或遍数超出范围。".into())
        };
        let limits = VoiceLimits {
            max_duration_millis,
            max_pending_jobs: number(&self.values[1])?,
            max_pending_per_owner: number(&self.values[2])?,
            max_repeat_total: number(&self.values[3])?,
        };
        limits.validate().map_err(|_| {
            "限制须符合：0.5–60 秒，全机 1–4 个、本人 1–2 个且不超过全机，播放 1–10 遍。".to_owned()
        })?;
        Ok(limits)
    }
}

pub(super) struct Editor {
    owner: PrincipalId,
    snapshot: VoiceManagementSnapshot,
    draft: Draft,
    review: Option<Review>,
    requires_refresh: bool,
    device: Option<DeviceDisplay>,
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
        let mut editor = Self {
            owner,
            draft: Draft::from(snapshot.limits),
            snapshot,
            review: None,
            requires_refresh: false,
            device: None,
        };
        editor.adopt_device_observation();
        editor
    }

    pub fn owner(&self) -> PrincipalId {
        self.owner
    }

    /// The coordinator checks the current local administrator lease before
    /// delivering a successful load. Every load discards the previous consent,
    /// even when its configuration revision happens to be unchanged.
    pub fn loaded(&mut self, snapshot: VoiceManagementSnapshot) {
        if self.draft == Draft::from(self.snapshot.limits) {
            self.draft = Draft::from(snapshot.limits);
        }
        self.snapshot = snapshot;
        self.review = None;
        self.requires_refresh = false;
        self.expire_device();
        self.adopt_device_observation();
    }

    pub fn invalidate(&mut self) {
        self.review = None;
        self.requires_refresh = true;
        self.snapshot.device_observation = None;
        self.expire_device();
    }

    fn expire_device(&mut self) {
        if let Some(device) = &mut self.device {
            device.current = false;
        }
    }

    fn adopt_device_observation(&mut self) {
        // Capture UI time BEFORE reading the remaining backend lifetime, so
        // delivery never starts another 30 seconds or extends the original TTL.
        let now = Instant::now();
        if let Some(observation) = &self.snapshot.device_observation {
            let remaining = observation.remaining_validity();
            self.device = Some(DeviceDisplay {
                result: observation.result,
                observed_at: observation.observed_at,
                expires: now.checked_add(remaining).unwrap_or(now),
                current: !remaining.is_zero(),
            });
        }
    }

    pub fn retained_device_observation(
        &self,
        snapshot: &VoiceManagementSnapshot,
        lease: SessionLease,
    ) -> Option<VoiceDeviceObservation> {
        self.snapshot
            .device_observation
            .as_ref()
            .filter(|observation| observation.is_current_for(snapshot, lease))
            .cloned()
    }

    pub fn loaded_versions(&self) -> (Revision, u64) {
        (
            self.snapshot.publication_revision,
            self.snapshot.runtime_generation,
        )
    }

    /// Local-only repaint predicate. No query, job, lease renewal or timer is
    /// created. Invalidation preserves the administrator's unsaved limits.
    pub fn tick(&mut self, now: Instant, context_current: bool) -> bool {
        let mut changed = false;
        if !context_current && !self.requires_refresh {
            self.invalidate();
            changed = true;
        }
        if let Some(device) = &mut self.device {
            if device.current && now >= device.expires {
                device.current = false;
                changed = true;
            }
        }
        changed
    }

    pub fn needs_tick(&self, now: Instant, context_current: bool) -> bool {
        (!context_current && !self.requires_refresh)
            || self
                .device
                .as_ref()
                .is_some_and(|device| device.current && now >= device.expires)
    }

    pub fn is_reviewing(&self) -> bool {
        self.review.is_some()
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.review.is_some() || self.draft != Draft::from(self.snapshot.limits)
    }

    /// Input owns only a bounded draft; no validation or saving is implicit.
    pub fn capture(&mut self, mut data: FormData) -> Result<(), String> {
        if self.review.is_some() {
            return Err("请先取消本次确认，再编辑限制草稿。".into());
        }
        self.take_draft(&mut data, true)?;
        if !data.is_empty() {
            return Err("限制草稿包含未识别字段。".into());
        }
        Ok(())
    }

    fn take_draft(&mut self, data: &mut FormData, required: bool) -> Result<(), String> {
        let fields = [
            DURATION_SECONDS,
            PENDING_JOBS,
            PENDING_PER_OWNER,
            REPEAT_TOTAL,
        ];
        let values = fields.map(|id| data.take(id));
        if !required && values.iter().all(Option::is_none) {
            return Ok(());
        }
        if values.iter().any(|v| {
            v.as_ref()
                .is_none_or(|s| s.len() > 16 || s.chars().any(char::is_control))
        }) {
            return Err("请完整填写四项限制草稿；每项最多 16 个字符。".into());
        }
        if !data.is_empty() {
            return Err("限制草稿包含未识别字段。".into());
        }
        self.draft = Draft {
            values: values.map(Option::unwrap),
        };
        Ok(())
    }

    pub fn view(&self, status: &str, busy: bool) -> View {
        let mut description = format!(
            "远程语音配置：{}\n接收新播报：{}\n活跃任务：{}；排队任务：{}；播放中任务：{}。这些计数来自最近一次读取。\n教师连接服务当前未就绪，配置开启不表示已提供教师连接服务。\n每遍之间固定静音 1 秒；时长、任务数和遍数以当前已保存限制为准。\n关闭会取消全部当前和排队任务；物理播放停止尚未完成时，不能声称已停止。重新开启后，须由教师发起新的动作，不恢复旧任务。\n配置开关不能证明音频设备可用，也不能证明教室已听到声音。",
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
        description.push_str(&format!("\n已保存限制：单段最多 {} 秒；全机最多 {} 个任务；本人最多 {} 个；播放最多 {} 遍，新录制默认 {} 遍。\n修改限制会取消当前及排队任务和旧上传，教师须发起新动作；已录制或已排队的遍数不会被设置改写。取消确认保留草稿；丢弃草稿恢复已保存值；返回工作台会丢弃草稿。", seconds(self.snapshot.limits.max_duration_millis), self.snapshot.limits.max_pending_jobs, self.snapshot.limits.max_pending_per_owner, self.snapshot.limits.max_repeat_total, self.snapshot.limits.default_repeat_total()));
        if self.draft != Draft::from(self.snapshot.limits) {
            description.push_str("\n有未保存的限制草稿；刷新或开关结果不会自动保存或清除该草稿。");
        }
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
        description.push_str("\n默认输出只在点击检查时读取；不播放声音。上次格式检查不代表设备当前可用或教室实际有声，30 秒后须重新检查。");
        if let Some(device) = &self.device {
            let time = device.observed_at;
            let date = time.date;
            let seconds = time.second_of_day();
            description.push_str(&format!(
                "\n检查时间：{:04}-{:02}-{:02} {:02}:{:02}:{:02}。",
                date.year(),
                date.month(),
                date.day(),
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60,
            ));
            if !device.current || Instant::now() >= device.expires {
                description.push_str("上次检查结果已过期，请重新检查。");
            } else {
                description.push_str(match device.result {
                    VoiceDeviceCheckResult::Supported => {
                        "上次检查：支持播报格式（不代表实际有声）。"
                    }
                    VoiceDeviceCheckResult::FormatUnsupported => {
                        "上次检查：默认输出不支持播报格式。"
                    }
                    VoiceDeviceCheckResult::Unavailable => "上次检查：未发现可用默认输出。",
                    VoiceDeviceCheckResult::Failed => "上次检查：检查失败，请稍后重新检查。",
                });
            }
        } else {
            description.push_str("\n尚未检查默认输出。");
        }
        let action = |id, label: &str, enabled| Action {
            id,
            label: label.into(),
            enabled,
        };
        let mut fields = Vec::new();
        let actions = if let Some(review) = &self.review {
            match review.target {
                Target::Enabled(true) => description.push_str("\n待确认：开启远程语音配置。旧任务不会恢复，教师须发起新的动作；连接服务仍须另行就绪。"),
                Target::Enabled(false) => description.push_str("\n待确认：关闭远程语音配置，并取消全部当前和排队任务。物理播放停止结果须等待实际完成并核对。"),
                Target::Limits(limits) => description.push_str(&format!("\n待确认保存限制：单段最多 {} 秒，全机 {} 个，本人 {} 个，最多 {} 遍，新录制默认 {} 遍。保存将取消当前和排队任务及旧上传，须由教师发起新动作；不会改写已录制或已排队任务的遍数。物理停止结果须另行核对。", seconds(limits.max_duration_millis), limits.max_pending_jobs, limits.max_pending_per_owner, limits.max_repeat_total, limits.default_repeat_total())),
            }
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
            for (id, label, value) in [
                (
                    DURATION_SECONDS,
                    "单段最长时长（秒，0.5–60）",
                    &self.draft.values[0],
                ),
                (PENDING_JOBS, "全机最多任务数（1–4）", &self.draft.values[1]),
                (
                    PENDING_PER_OWNER,
                    "每位教师最多任务数（1–2，且不超过全机）",
                    &self.draft.values[2],
                ),
                (REPEAT_TOTAL, "最多播放遍数（1–10）", &self.draft.values[3]),
            ] {
                fields.push(Field {
                    id,
                    label: label.into(),
                    kind: FieldKind::Text,
                    value: value.clone(),
                    max_utf16: 16,
                });
            }
            vec![
                action(SAVE_LIMITS, "核对并保存限制草稿", can_change),
                action(DISCARD_LIMITS, "丢弃限制草稿", !busy),
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
                action(CHECK_OUTPUT, "检查默认输出", can_change),
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
        if action == CHECK_OUTPUT {
            if !data.is_empty() {
                return Err("检查默认输出不接纳表单字段。".into());
            }
            if self.requires_refresh || self.review.is_some() {
                return Err("请先重新读取状态或取消当前确认，再检查默认输出。".into());
            }
            return Ok(Command::CheckOutput {
                expected_publication: self.snapshot.publication_revision,
                expected: self.snapshot.revision,
                expected_identity: self.snapshot.identity_revision,
                expected_runtime: self.snapshot.runtime_generation,
            });
        }
        if action == CANCEL {
            let _ = data.take(ACK);
        }
        if action != CONFIRM {
            self.take_draft(&mut data, action == SAVE_LIMITS)?;
            if !data.is_empty() {
                return Err("本次操作包含未识别字段或隐式版本。".into());
            }
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
                    target: Target::Enabled(enabled),
                });
                Ok(Command::Stay)
            }
            SAVE_LIMITS => {
                if self.requires_refresh || self.review.is_some() {
                    return Err("请先重新读取状态或取消现有确认。".into());
                }
                let limits = self.draft.parse()?;
                self.draft = Draft::from(limits);
                if limits == self.snapshot.limits {
                    return Err("限制与已保存值相同。".into());
                }
                self.review = Some(Review {
                    identity_revision: self.snapshot.identity_revision,
                    expected: self.snapshot.revision,
                    runtime_generation: self.snapshot.runtime_generation,
                    target: Target::Limits(limits),
                });
                Ok(Command::Stay)
            }
            DISCARD_LIMITS => {
                self.review = None;
                self.draft = Draft::from(self.snapshot.limits);
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
                self.snapshot.device_observation = None;
                self.expire_device();
                Ok(match review.target {
                    Target::Enabled(enabled) => Command::SetEnabled {
                        expected: review.expected,
                        expected_identity: review.identity_revision,
                        expected_runtime: review.runtime_generation,
                        enabled,
                    },
                    Target::Limits(limits) => Command::SetLimits {
                        expected: review.expected,
                        expected_identity: review.identity_revision,
                        expected_runtime: review.runtime_generation,
                        limits,
                    },
                })
            }
            CANCEL => {
                self.review = None;
                Ok(Command::Stay)
            }
            REFRESH => {
                self.review = None;
                self.requires_refresh = true;
                Ok(Command::Refresh)
            }
            CLOSE => {
                self.draft = Draft::from(self.snapshot.limits);
                self.invalidate();
                Ok(Command::Close)
            }
            _ => Err("当前远程语音页面不接纳此操作。".into()),
        }
    }
}
