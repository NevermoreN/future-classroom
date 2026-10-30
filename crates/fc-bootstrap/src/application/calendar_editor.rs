//! Owner-bound review of non-active public candidates. Every write requires an
//! explicit confirmation; workers repeat authorization, revision and source checks.
use fc_domain::{
    calendar::{
        CalendarCandidate, CalendarDifference, CalendarProposal, PublicDayKind, PublicDaySummary,
        PROVIDER_ID, PROVIDER_LICENSE, PROVIDER_VERSION,
    },
    ids::{PrincipalId, Revision},
    schedule::CalendarMode,
    schedule_document::ScheduleDocument,
    time::{LocalDate, LocalDateTime},
};
use fc_ports::{CalendarCandidateRef, CalendarFetchFailure, CalendarStatus};
use fc_presentation::workbench::*;
use std::collections::BTreeSet;

pub(super) const YEAR: FieldId = FieldId(600);
pub(super) const ACK: FieldId = FieldId(601);
const SOURCE_URL: FieldId = FieldId(602);
pub(super) const SELECT: ActionId = ActionId(600);
pub(super) const DOWNLOAD: ActionId = ActionId(601);
pub(super) const REVIEW: ActionId = ActionId(602);
pub(super) const CONFIRM_ADOPT: ActionId = ActionId(603);
pub(super) const CANCEL_REVIEW: ActionId = ActionId(604);
pub(super) const AUTO_REVIEW: ActionId = ActionId(605);
pub(super) const CONFIRM_AUTO: ActionId = ActionId(606);
pub(super) const PREVIOUS: ActionId = ActionId(607);
pub(super) const NEXT: ActionId = ActionId(608);
pub(super) const CLOSE: ActionId = ActionId(609);
const PAGE_SIZE: usize = 12;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
    Stay,
    Close,
    Load(i32),
    Download(i32),
    Adopt {
        expected: Revision,
        candidates: Vec<CalendarCandidateRef>,
    },
    Policy {
        expected: Revision,
        enabled: bool,
    },
}
struct Review {
    proposal: CalendarProposal,
    candidate: CalendarCandidate,
    reference: CalendarCandidateRef,
    expected: Revision,
}
enum Phase {
    List,
    Review(Box<Review>),
    Auto { expected: Revision, enabled: bool },
}
pub(super) struct Editor {
    owner: PrincipalId,
    document: ScheduleDocument,
    status: CalendarStatus,
    phase: Phase,
    page: usize,
}
fn action(id: ActionId, label: &str, enabled: bool) -> Action {
    Action {
        id,
        label: label.into(),
        enabled,
    }
}
fn date(d: LocalDate) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}
fn time(t: Option<LocalDateTime>) -> String {
    t.map(|t| {
        format!(
            "{} {:02}:{:02}:{:02}",
            date(t.date),
            t.second_of_day() / 3600,
            t.second_of_day() / 60 % 60,
            t.second_of_day() % 60
        )
    })
    .unwrap_or_else(|| "无".into())
}
fn kind(kind: Option<PublicDayKind>) -> &'static str {
    match kind {
        Some(PublicDayKind::Rest) => "放假",
        Some(PublicDayKind::Makeup) => "补班：课程待确认",
        None => "来源冲突：待确认",
    }
}
fn summary(s: Option<&PublicDaySummary>) -> String {
    s.map(|s| {
        format!(
            "{}；{}；来源年{}",
            kind(s.kind),
            s.names.join("/"),
            s.source_years
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join("/")
        )
    })
    .unwrap_or_else(|| "无公共特殊安排".into())
}
fn bounded_message(message: Option<String>) -> String {
    let mut text = String::new();
    for ch in message.unwrap_or_default().chars() {
        let ch = if ch == '\0' { '\u{fffd}' } else { ch };
        if text.len() + ch.len_utf8() >= 8192 {
            break;
        }
        text.push(ch);
    }
    text
}
fn pages(count: usize) -> usize {
    count.div_ceil(PAGE_SIZE).max(1)
}
fn sha(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}
fn failure(f: CalendarFetchFailure) -> &'static str {
    match f {
        CalendarFetchFailure::Cancelled => "已取消",
        CalendarFetchFailure::Network => "网络失败",
        CalendarFetchFailure::Tls => "TLS校验或握手失败",
        CalendarFetchFailure::Http => "HTTP失败",
        CalendarFetchFailure::InvalidData => "资料格式/来源校验失败",
        CalendarFetchFailure::TooLarge => "资料超出大小限制",
        CalendarFetchFailure::Deadline => "下载期限已到",
    }
}
fn ack_field(label: &str) -> Field {
    Field {
        id: ACK,
        label: label.into(),
        kind: FieldKind::Choice(vec![
            Choice {
                value: "0".into(),
                label: "尚未确认".into(),
            },
            Choice {
                value: "1".into(),
                label: "已理解并确认".into(),
            },
        ]),
        value: "0".into(),
        max_utf16: 1,
    }
}
fn difference_row(d: &CalendarDifference) -> GridRow {
    GridRow {
        group: if d.before.is_none() {
            "新增"
        } else if d.after.is_none() {
            "删除"
        } else {
            "变化"
        }
        .into(),
        label: date(d.date),
        cells: vec![
            summary(d.before.as_ref()),
            summary(d.after.as_ref()),
            if d.manual_overrides.is_empty() {
                "无".into()
            } else {
                format!(
                    "保留人工覆盖；学期{}",
                    d.manual_overrides
                        .iter()
                        .map(|s| s.get().to_string())
                        .collect::<Vec<_>>()
                        .join("/")
                )
            },
        ],
        merged: false,
    }
}
impl Editor {
    pub(super) fn new(
        owner: PrincipalId,
        document: ScheduleDocument,
        status: CalendarStatus,
    ) -> Self {
        Self {
            owner,
            document,
            status,
            phase: Phase::List,
            page: 0,
        }
    }
    pub(super) fn owner(&self) -> PrincipalId {
        self.owner
    }
    pub(super) fn selected_year(&self) -> i32 {
        self.status.selected_year
    }
    pub(super) fn has_unsaved_changes(&self) -> bool {
        !matches!(self.phase, Phase::List)
    }
    /// Only the controller holding this owner's editor may replace its snapshots.
    /// A new cache/schedule invalidates every older review and confirmation.
    pub(super) fn replace(&mut self, document: ScheduleDocument, status: CalendarStatus) {
        self.document = document;
        self.status = status;
        self.phase = Phase::List;
        self.page = 0;
    }
    fn candidate(&self) -> Option<&CalendarCandidate> {
        self.status
            .candidate
            .as_ref()
            .filter(|c| c.source_year == self.status.selected_year && c.validate().is_ok())
    }
    fn candidate_ref(&self) -> Option<CalendarCandidateRef> {
        let c = self.candidate()?;
        self.status
            .years
            .iter()
            .find(|y| y.year == c.source_year && y.candidate_revision.get() != 0)
            .map(|y| CalendarCandidateRef {
                year: y.year,
                revision: y.candidate_revision,
            })
    }
    fn related_years(&self) -> Vec<i32> {
        let mut years = BTreeSet::new();
        years.insert(self.status.selected_year);
        for plan in &self.document.semesters {
            for y in plan.semester.start.year()..=plan.semester.end.year() {
                years.insert(y);
                if y < 9999 {
                    years.insert(y + 1);
                }
                if years.len() >= 64 {
                    break;
                }
            }
        }
        years.extend(self.status.years.iter().map(|y| y.year));
        let mut result = vec![self.status.selected_year];
        result.extend(
            years
                .into_iter()
                .filter(|y| *y != self.status.selected_year)
                .take(31),
        );
        result
    }
    fn total_pages(&self) -> usize {
        match &self.phase {
            Phase::List => pages(self.candidate().map_or(0, |c| c.days.len())),
            Phase::Review(review) => {
                let conflicts = if review.proposal.conflicts.is_empty() {
                    0
                } else {
                    pages(review.proposal.conflicts.len())
                };
                let old_papers = self
                    .document
                    .public_calendar_data
                    .years
                    .iter()
                    .find(|y| y.source_year == review.candidate.source_year)
                    .map_or(0, |y| y.papers.len());
                pages(review.proposal.differences.len())
                    + conflicts
                    + review.candidate.papers.len()
                    + old_papers
            }
            Phase::Auto { .. } => 1,
        }
    }
    pub(super) fn view(&self, busy: bool, message: Option<String>) -> View {
        let mode = if self.document.calendar_mode == CalendarMode::School {
            "学校"
        } else {
            "公共"
        };
        let mut v=View{title:"公共校历候选资料".into(),description:format!("当前为{mode}模式；候选下载和复核不会改变当天安排。学校与公共资料相互隔离。开源资料仅为可替换候选，政府链接为候选声明的依据，尚未读取核验通知正文。\n提供器：{PROVIDER_ID} / {PROVIDER_VERSION}；许可证：{PROVIDER_LICENSE}。"),status:bounded_message(message),fields:vec![],actions:vec![],busy,grid:None};
        match &self.phase {
            Phase::List => {
                v.description.push_str(&format!("\n自动采用纯放假变化：{}（默认关闭，须主管理员明确授权）。\n来源年份：{}；相关年份建议：{}（跨年须分别拉取，下一年文件可能带上年12月安排）。",if self.status.policy.enabled{"开启"}else{"关闭"},self.status.selected_year,self.related_years().iter().map(i32::to_string).collect::<Vec<_>>().join("、")));
                if let Some(y) = self
                    .status
                    .years
                    .iter()
                    .find(|y| y.year == self.status.selected_year)
                {
                    v.description.push_str(&format!("\n最近尝试：{}；最近成功：{}；下次允许：{}；上次预约未确认结束（可能中断）：{}；最近错误：{}。失败保留已采用资料和有效缓存，限频不会因重启或失败重置；可按限频再次尝试。",time(y.last_attempt),time(y.last_success),time(y.next_allowed),if y.pending{"是"}else{"否"},y.last_error.map(failure).unwrap_or("无")));
                }
                v.fields.push(Field {
                    id: YEAR,
                    label: "来源年份（1–9999；输入后读取该年）".into(),
                    kind: FieldKind::Text,
                    value: if (1..=9999).contains(&self.status.selected_year) {
                        self.status.selected_year.to_string()
                    } else {
                        String::new()
                    },
                    max_utf16: 4,
                });
                if let Some(c) = self.candidate() {
                    self.describe_candidate(&mut v, c);
                    v.grid = Some(Grid {
                        columns: vec!["日期".into(), "候选安排".into(), "名称".into()],
                        rows: c
                            .days
                            .iter()
                            .skip(self.page * PAGE_SIZE)
                            .take(PAGE_SIZE)
                            .map(|d| GridRow {
                                group: if d.date.year() != c.source_year {
                                    "上年12月"
                                } else {
                                    "本年"
                                }
                                .into(),
                                label: date(d.date),
                                cells: vec![kind(Some(d.kind)).into(), d.name.clone()],
                                merged: false,
                            })
                            .collect(),
                        selected: None,
                        enabled: false,
                    });
                } else {
                    v.description.push_str("\n该年暂无通过校验的候选。空年度或下载失败不能证明当年没有调休；已采用资料仍离线有效。");
                }
                v.actions = vec![
                    action(SELECT, "读取输入年份", !busy),
                    action(DOWNLOAD, "按限频下载所选年", !busy),
                    action(
                        REVIEW,
                        "复核差异与全部来源",
                        !busy && self.candidate_ref().is_some(),
                    ),
                    action(AUTO_REVIEW, "修改自动采用开关…", !busy),
                    action(CLOSE, "返回", !busy),
                ];
            }
            Phase::Review(review) => {
                v.title = "采用前复核：公共校历".into();
                self.describe_candidate(&mut v, &review.candidate);
                let p = &review.proposal;
                let added = p.differences.iter().filter(|d| d.before.is_none()).count();
                let removed = p.differences.iter().filter(|d| d.after.is_none()).count();
                let shadowed = p
                    .differences
                    .iter()
                    .filter(|d| !d.manual_overrides.is_empty())
                    .count();
                v.description.push_str(&format!("\n共{}处差异：新增{added}、删除{removed}、变化{}；其中{shadowed}处被人工安排覆盖，人工安排保留不变；合并后{}处来源冲突。未映射补班和未人工解决的冲突当天为待确认，必须到课表校历页选择具体补课来源日期。空白日仍用基础周表。",p.differences.len(),p.differences.len()-added-removed,p.conflicts.len()));
                if self.document.calendar_mode == CalendarMode::School {
                    v.description.push_str("\n当前学校模式仅允许复核候选。请先在课表中明确切换并保存公共模式，再重新复核采用。");
                }
                let diff_pages = pages(p.differences.len());
                let conflict_pages = if p.conflicts.is_empty() {
                    0
                } else {
                    pages(p.conflicts.len())
                };
                let (columns, rows) = if self.page < diff_pages {
                    if p.differences.is_empty() {
                        v.description
                            .push_str("\n日期安排没有差异；来源、抓取信息与文件摘要仍需人工核对。");
                    }
                    (
                        vec![
                            "日期".into(),
                            "原已采用".into(),
                            "新候选".into(),
                            "人工安排".into(),
                        ],
                        p.differences
                            .iter()
                            .skip(self.page * PAGE_SIZE)
                            .take(PAGE_SIZE)
                            .map(difference_row)
                            .collect(),
                    )
                } else if self.page < diff_pages + conflict_pages {
                    v.description.push_str("\n本页列出合并后全部来源冲突，包含此次未改变的旧冲突；人工日期覆盖继续优先，否则该日待确认。");
                    (
                        vec!["冲突日期".into(), "公共来源安排".into(), "人工安排".into()],
                        p.conflicts
                            .iter()
                            .skip((self.page - diff_pages) * PAGE_SIZE)
                            .take(PAGE_SIZE)
                            .map(|d| {
                                let manual: BTreeSet<_> = self
                                    .document
                                    .public_calendar
                                    .entries
                                    .iter()
                                    .filter(|m| m.date == *d)
                                    .map(|m| m.actual_semester_id.get())
                                    .collect();
                                GridRow {
                                    group: "冲突".into(),
                                    label: date(*d),
                                    cells: vec![
                                        summary(p.data.merged_day(*d).as_ref()),
                                        if manual.is_empty() {
                                            "无；该日待确认".into()
                                        } else {
                                            format!(
                                                "保留人工覆盖；学期{}",
                                                manual
                                                    .iter()
                                                    .map(u64::to_string)
                                                    .collect::<Vec<_>>()
                                                    .join("/")
                                            )
                                        },
                                    ],
                                    merged: false,
                                }
                            })
                            .collect(),
                    )
                } else {
                    v.description.push_str("\n本页列出原采用及新候选声明的全部政府通知链接。程序校验来源地址形式及TLS下载的候选结构，未声称已经下载或自动解释通知正文；请对照通知核对。");
                    let old = self
                        .document
                        .public_calendar_data
                        .years
                        .iter()
                        .find(|y| y.source_year == review.candidate.source_year);
                    let rows: Vec<GridRow> =
                        old.into_iter()
                            .flat_map(|c| {
                                c.papers.iter().enumerate().map(|(i, url)| GridRow {
                                    group: "原来源".into(),
                                    label: format!("原采用{}", i + 1),
                                    cells: vec![url.clone()],
                                    merged: false,
                                })
                            })
                            .chain(review.candidate.papers.iter().enumerate().map(|(i, url)| {
                                GridRow {
                                    group: "来源".into(),
                                    label: format!("新候选{}", i + 1),
                                    cells: vec![url.clone()],
                                    merged: false,
                                }
                            }))
                            .skip(self.page - diff_pages - conflict_pages)
                            .take(1)
                            .collect();
                    (
                        vec!["来源序号".into(), "政府通知链接（候选声明）".into()],
                        rows,
                    )
                };
                if self.page >= diff_pages + conflict_pages {
                    if let Some(row) = rows.first() {
                        v.fields.push(Field {
                            id: SOURCE_URL,
                            label: format!("{}完整链接（可横向查看及复制）", row.label),
                            kind: FieldKind::ReadOnly,
                            value: row.cells[0].clone(),
                            max_utf16: 512,
                        });
                    }
                }
                v.grid = Some(Grid {
                    columns,
                    rows,
                    selected: None,
                    enabled: false,
                });
                v.fields
                    .push(ack_field("确认候选来源，理解补班须人工映射且人工安排不变"));
                v.actions = vec![
                    action(
                        CONFIRM_ADOPT,
                        "确认采用所选年",
                        !busy && self.document.calendar_mode == CalendarMode::Public,
                    ),
                    action(CANCEL_REVIEW, "取消本次采用", !busy),
                ];
            }
            Phase::Auto { enabled, .. } => {
                v.title = "确认公共校历自动采用设置".into();
                v.description.push_str(if *enabled {"\n将开启纯放假变化自动采用。只有已经人工采用过的同一年、同一组政府来源，且变化严格仅放假新增或删除时才允许；不新增、删除或猜测补班课程，不改变人工确认安排，不改变学校模式。来源、名称或冲突变化继续等待人工复核。授权人被停用、撤权或认证代次变化后授权失效。"}else{"\n将关闭自动采用。已采用安排和有效候选缓存继续保留；后续更新需人工复核采用。"});
                v.description.push_str("\n模式切换后的在途下载只能进入候选缓存。低频下载有持久限频、字节、日期条数和期限上限；断网时继续使用已保存资料。");
                v.fields.push(ack_field("确认修改自动采用策略"));
                v.actions = vec![
                    action(
                        CONFIRM_AUTO,
                        if *enabled {
                            "确认开启"
                        } else {
                            "确认关闭"
                        },
                        !busy,
                    ),
                    action(CANCEL_REVIEW, "保留原设置", !busy),
                ];
            }
        }
        let total = self.total_pages();
        if !matches!(self.phase, Phase::Auto { .. }) {
            v.description.push_str(&format!(
                "\n第{}页 / {total}页，差异和冲突每页最多{PAGE_SIZE}行，来源每页1条完整链接。",
                self.page + 1
            ));
            v.actions
                .push(action(PREVIOUS, "上一页", !busy && self.page > 0));
            v.actions
                .push(action(NEXT, "下一页", !busy && self.page + 1 < total));
        }
        v
    }
    fn describe_candidate(&self, v: &mut View, c: &CalendarCandidate) {
        let spill = c
            .days
            .iter()
            .filter(|d| d.date.year() != c.source_year)
            .count();
        let coverage = c
            .days
            .iter()
            .map(|d| d.date)
            .min()
            .zip(c.days.iter().map(|d| d.date).max())
            .map(|(a, b)| format!("{}至{}", date(a), date(b)))
            .unwrap_or_else(|| "空".into());
        let adopted = self
            .document
            .public_calendar_data
            .years
            .iter()
            .find(|y| y.source_year == c.source_year);
        v.description.push_str(&format!("\n候选源：{}\n原始文件SHA-256：{}\n拉取成功：{}；来源年{}，{}个特殊日（上年12月{spill}日），日期范围{coverage}，{}个政府通知链接。仅来源年可以作为已核验年度，附带日期不会核验邻年。\n本机该年{}。候选补班不代表已知应补哪天课程。",c.source_url(),sha(&c.source_sha256),time(Some(c.fetched_at)),c.source_year,c.days.len(),c.papers.len(),if adopted.is_some(){"已有采用版本，采用前将比较更新"}else{"尚无采用版本"}));
        if let Some(old) = adopted {
            v.description.push_str(&format!(
                "\n原采用文件SHA-256：{}；原抓取成功：{}；{}个特殊日，{}个政府来源。",
                sha(&old.source_sha256),
                time(Some(old.fetched_at)),
                old.days.len(),
                old.papers.len()
            ));
        }
    }
    pub(super) fn event(&mut self, action: ActionId, data: FormData) -> Result<Command, String> {
        if action == PREVIOUS || action == NEXT {
            if matches!(self.phase, Phase::Auto { .. }) {
                return Err("当前确认页没有分页。".into());
            }
            let total = self.total_pages();
            self.page = if action == PREVIOUS {
                self.page.checked_sub(1).ok_or("已经是第一页。")?
            } else {
                self.page
                    .checked_add(1)
                    .filter(|p| *p < total)
                    .ok_or("已经是最后一页。")?
            };
            return Ok(Command::Stay);
        }
        if action == CANCEL_REVIEW {
            if matches!(self.phase, Phase::List) {
                return Err("当前没有待取消的确认。".into());
            }
            self.phase = Phase::List;
            self.page = 0;
            return Ok(Command::Stay);
        }
        if action == CLOSE {
            return Ok(Command::Close);
        }
        match (&self.phase, action) {
            (Phase::List, SELECT) => {
                let text = data.get(YEAR).unwrap_or("");
                let year = text
                    .parse::<i32>()
                    .ok()
                    .filter(|y| {
                        (1..=9999).contains(y)
                            && !text.is_empty()
                            && text.len() <= 4
                            && text.bytes().all(|b| b.is_ascii_digit())
                    })
                    .ok_or("请输入1至9999的来源年份。")?;
                Ok(Command::Load(year))
            }
            (Phase::List, DOWNLOAD) => {
                if data
                    .get(YEAR)
                    .is_some_and(|year| year != self.status.selected_year.to_string())
                {
                    return Err("输入的年份尚未读取，请先点击读取输入年份。".into());
                }
                if !(1..=9999).contains(&self.status.selected_year) {
                    return Err("请先读取有效年份。".into());
                }
                Ok(Command::Download(self.status.selected_year))
            }
            (Phase::List, REVIEW) => {
                let reference = self
                    .candidate_ref()
                    .ok_or("没有可复核的有效缓存，请先下载或重新读取。")?;
                let candidate = self
                    .candidate()
                    .ok_or("候选年份不匹配或校验失败。")?
                    .clone();
                let proposal = self
                    .document
                    .public_calendar_data
                    .proposal(
                        std::slice::from_ref(&candidate),
                        &self.document.public_calendar.entries,
                    )
                    .map_err(|_| "候选无法形成完整有效的采用差异，请重新读取。")?;
                self.phase = Phase::Review(Box::new(Review {
                    proposal,
                    candidate,
                    reference,
                    expected: self.document.revision,
                }));
                self.page = 0;
                Ok(Command::Stay)
            }
            (Phase::Review(review), CONFIRM_ADOPT) => {
                if self.document.calendar_mode != CalendarMode::Public {
                    return Err("学校模式不能采用公共资料；请先明确切换并保存公共模式。".into());
                }
                if data.get(ACK) != Some("1") {
                    return Err("请明确确认来源及补班课程仍须人工映射。".into());
                }
                let command = Command::Adopt {
                    expected: review.expected,
                    candidates: vec![review.reference],
                };
                self.phase = Phase::List;
                self.page = 0;
                Ok(command)
            }
            (Phase::List, AUTO_REVIEW) => {
                self.phase = Phase::Auto {
                    expected: self.status.policy.revision,
                    enabled: !self.status.policy.enabled,
                };
                self.page = 0;
                Ok(Command::Stay)
            }
            (Phase::Auto { expected, enabled }, CONFIRM_AUTO) => {
                if data.get(ACK) != Some("1") {
                    return Err("请明确确认是否修改自动采用策略。".into());
                }
                let command = Command::Policy {
                    expected: *expected,
                    enabled: *enabled,
                };
                self.phase = Phase::List;
                self.page = 0;
                Ok(command)
            }
            _ => Err("此操作不属于当前页面，请重新复核。".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::{
        calendar::PublicCalendarDay,
        ids::SemesterId,
        schedule::{CalendarEntry, DayMapping},
    };
    use fc_ports::{CalendarAutoPolicy, CalendarYearSummary};
    fn candidate(count: usize, papers: usize) -> CalendarCandidate {
        CalendarCandidate {
            source_year: 2026,
            fetched_at: LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 36000).unwrap(),
            source_sha256: [7; 32],
            papers: (0..papers)
                .map(|i| {
                    format!("https://www.gov.cn/zhengce/zhengceku/202511/content_70470{i:02}.htm")
                })
                .collect(),
            days: (0..count)
                .map(|i| PublicCalendarDay {
                    date: LocalDate::new(2026, (i / 28 + 1) as u8, (i % 28 + 1) as u8).unwrap(),
                    kind: if i == 0 {
                        PublicDayKind::Makeup
                    } else {
                        PublicDayKind::Rest
                    },
                    name: "合成假日".into(),
                })
                .collect(),
            etag: None,
            last_modified: None,
        }
    }
    fn status(count: usize, papers: usize) -> CalendarStatus {
        CalendarStatus {
            policy: CalendarAutoPolicy::default(),
            years: vec![CalendarYearSummary {
                year: 2026,
                candidate_revision: Revision::new(8),
                last_attempt: None,
                next_allowed: None,
                last_success: None,
                last_error: None,
                pending: false,
            }],
            selected_year: 2026,
            candidate: Some(candidate(count, papers)),
        }
    }
    fn editor() -> Editor {
        Editor::new(
            PrincipalId::new(1).unwrap(),
            ScheduleDocument::default(),
            status(30, 1),
        )
    }
    fn form(id: FieldId, value: &str) -> FormData {
        let mut data = FormData::new();
        data.insert(id, value.into()).unwrap();
        data
    }
    fn yes() -> FormData {
        form(ACK, "1")
    }
    #[test]
    fn selection_only_loads_and_download_never_guesses_typed_year() {
        let mut e = editor();
        assert_eq!(e.selected_year(), 2026);
        assert_eq!(
            e.event(SELECT, form(YEAR, "2027")).unwrap(),
            Command::Load(2027)
        );
        assert_eq!(
            e.event(DOWNLOAD, FormData::new()).unwrap(),
            Command::Download(2026)
        );
        assert!(e.event(DOWNLOAD, form(YEAR, "2027")).is_err());
        for invalid in ["", "0", "10000", "-1", "20a6", "2026 ", "２０２６"] {
            assert!(e.event(SELECT, form(YEAR, invalid)).is_err());
        }
    }
    #[test]
    fn review_is_read_only_and_confirmation_needs_acknowledgment() {
        let mut e = editor();
        assert!(!e.has_unsaved_changes());
        assert!(e.event(CONFIRM_ADOPT, yes()).is_err());
        assert_eq!(e.event(REVIEW, FormData::new()).unwrap(), Command::Stay);
        assert!(e.has_unsaved_changes());
        assert!(e.event(CONFIRM_ADOPT, FormData::new()).is_err());
        assert_eq!(
            e.event(CONFIRM_ADOPT, yes()).unwrap(),
            Command::Adopt {
                expected: Revision::default(),
                candidates: vec![CalendarCandidateRef {
                    year: 2026,
                    revision: Revision::new(8)
                }]
            }
        );
        assert!(!e.has_unsaved_changes());
        assert!(e.event(CONFIRM_ADOPT, yes()).is_err());
    }
    #[test]
    fn confirmation_uses_frozen_revisions_and_ignores_foreign_form_year() {
        let mut e = editor();
        e.document.revision = Revision::new(21);
        e.event(REVIEW, FormData::new()).unwrap();
        let mut data = yes();
        data.insert(YEAR, "2027".into()).unwrap();
        assert_eq!(
            e.event(CONFIRM_ADOPT, data).unwrap(),
            Command::Adopt {
                expected: Revision::new(21),
                candidates: vec![CalendarCandidateRef {
                    year: 2026,
                    revision: Revision::new(8)
                }]
            }
        );
    }
    #[test]
    fn replace_discards_old_review_and_preserves_owner_identity() {
        let mut e = editor();
        let owner = e.owner();
        e.event(REVIEW, FormData::new()).unwrap();
        let mut next = status(2, 1);
        next.selected_year = 2027;
        next.candidate = None;
        e.replace(ScheduleDocument::default(), next);
        assert_eq!(e.owner(), owner);
        assert_eq!(e.selected_year(), 2027);
        assert!(!e.has_unsaved_changes());
        assert!(e.event(CONFIRM_ADOPT, yes()).is_err());
        assert!(e.event(REVIEW, FormData::new()).is_err());
        let other = Editor::new(
            PrincipalId::new(2).unwrap(),
            ScheduleDocument::default(),
            status(1, 1),
        );
        assert_ne!(e.owner(), other.owner());
    }
    #[test]
    fn cancel_changes_neither_adopted_data_nor_policy() {
        let mut e = editor();
        let original = e.document.clone();
        let policy = e.status.policy.clone();
        e.event(REVIEW, FormData::new()).unwrap();
        assert_eq!(
            e.event(CANCEL_REVIEW, FormData::new()).unwrap(),
            Command::Stay
        );
        assert_eq!(e.document, original);
        assert_eq!(e.status.policy, policy);
        assert!(!e.has_unsaved_changes());
        e.event(AUTO_REVIEW, FormData::new()).unwrap();
        e.event(CANCEL_REVIEW, FormData::new()).unwrap();
        assert_eq!(e.status.policy, policy);
        assert!(e.event(CONFIRM_AUTO, yes()).is_err());
    }
    #[test]
    fn school_can_review_sources_and_download_but_cannot_adopt() {
        let mut e = editor();
        e.document.calendar_mode = CalendarMode::School;
        assert_eq!(
            e.event(DOWNLOAD, FormData::new()).unwrap(),
            Command::Download(2026)
        );
        e.event(REVIEW, FormData::new()).unwrap();
        let view = e.view(false, None);
        assert!(view.description.contains("仅允许复核候选"));
        assert!(
            !view
                .actions
                .iter()
                .find(|a| a.id == CONFIRM_ADOPT)
                .unwrap()
                .enabled
        );
        assert!(e.event(CONFIRM_ADOPT, yes()).is_err());
        assert_eq!(e.document.calendar_mode, CalendarMode::School);
    }
    #[test]
    fn every_difference_and_all_sixteen_sources_remain_accessible_in_bounded_pages() {
        let mut e = Editor::new(
            PrincipalId::new(1).unwrap(),
            ScheduleDocument::default(),
            status(100, 16),
        );
        e.event(REVIEW, FormData::new()).unwrap();
        let mut differences = 0;
        let mut sources = Vec::new();
        loop {
            let view = e.view(false, None);
            assert_eq!(view.validate(), Ok(()));
            assert!(view.description.len() < 8192);
            let grid = view.grid.as_ref().unwrap();
            assert!(grid.rows.len() <= PAGE_SIZE);
            for row in &grid.rows {
                if row.group == "来源" {
                    sources.push(row.cells[0].clone());
                    assert_eq!(
                        view.fields
                            .iter()
                            .find(|f| f.id == SOURCE_URL)
                            .unwrap()
                            .value,
                        row.cells[0]
                    );
                } else {
                    differences += 1;
                }
            }
            if !view.actions.iter().find(|a| a.id == NEXT).unwrap().enabled {
                break;
            }
            e.event(NEXT, FormData::new()).unwrap();
        }
        assert_eq!(differences, 100);
        assert_eq!(sources, candidate(100, 16).papers);
        assert!(e.event(NEXT, FormData::new()).is_err());
        e.event(PREVIOUS, FormData::new()).unwrap();
    }
    #[test]
    fn full_diff_includes_removed_changed_conflict_and_shadowed_human_arrangement() {
        let mut e = editor();
        let mut old = candidate(3, 1);
        old.days[0].kind = PublicDayKind::Rest;
        e.document.public_calendar_data.years.push(old.clone());
        let conflict_date = LocalDate::new(2026, 12, 31).unwrap();
        e.document.public_calendar_data.years[0]
            .days
            .push(PublicCalendarDay {
                date: conflict_date,
                kind: PublicDayKind::Rest,
                name: "旧年安排".into(),
            });
        let mut next_year = candidate(1, 1);
        next_year.source_year = 2027;
        next_year.days = vec![
            PublicCalendarDay {
                date: LocalDate::new(2027, 1, 1).unwrap(),
                kind: PublicDayKind::Rest,
                name: "新年".into(),
            },
            PublicCalendarDay {
                date: conflict_date,
                kind: PublicDayKind::Makeup,
                name: "冲突".into(),
            },
        ];
        e.document.public_calendar_data.years.push(next_year);
        let c = e.status.candidate.as_mut().unwrap();
        c.days.retain(|d| d.date != old.days[1].date);
        c.days.push(PublicCalendarDay {
            date: conflict_date,
            kind: PublicDayKind::Rest,
            name: "旧年安排".into(),
        });
        e.document.public_calendar.entries.push(CalendarEntry {
            actual_semester_id: SemesterId::new(4).unwrap(),
            date: old.days[0].date,
            mapping: DayMapping::Rest,
        });
        e.event(REVIEW, FormData::new()).unwrap();
        let view = e.view(false, None);
        assert!(view.description.contains("删除1"));
        assert!(view.description.contains("1处被人工安排覆盖"));
        assert!(view.description.contains("1处来源冲突"));
        assert!(view.grid.unwrap().rows.iter().any(|r| r.group == "删除"));
        let mut conflicts = Vec::new();
        let mut old_sources = Vec::new();
        loop {
            let view = e.view(false, None);
            assert_eq!(view.validate(), Ok(()));
            for row in &view.grid.as_ref().unwrap().rows {
                if row.group == "冲突" {
                    conflicts.push(row.label.clone());
                }
                if row.group == "原来源" {
                    old_sources.push(row.cells[0].clone());
                }
            }
            if !view.actions.iter().find(|a| a.id == NEXT).unwrap().enabled {
                break;
            }
            e.event(NEXT, FormData::new()).unwrap();
        }
        assert_eq!(conflicts, vec![date(conflict_date)]);
        assert_eq!(old_sources, old.papers);
        assert!(e
            .view(false, None)
            .description
            .contains("原采用文件SHA-256"));
    }
    #[test]
    fn policy_toggle_is_explicit_and_captures_exact_policy_revision() {
        let mut e = editor();
        e.status.policy.revision = Revision::new(17);
        assert!(!e.status.policy.enabled);
        e.event(AUTO_REVIEW, FormData::new()).unwrap();
        assert!(e.event(CONFIRM_AUTO, FormData::new()).is_err());
        assert_eq!(
            e.event(CONFIRM_AUTO, yes()).unwrap(),
            Command::Policy {
                expected: Revision::new(17),
                enabled: true
            }
        );
        assert!(!e.status.policy.enabled);
        assert!(e.event(CONFIRM_AUTO, yes()).is_err());
        e.status.policy.enabled = true;
        e.event(AUTO_REVIEW, FormData::new()).unwrap();
        assert_eq!(
            e.event(CONFIRM_AUTO, yes()).unwrap(),
            Command::Policy {
                expected: Revision::new(17),
                enabled: false
            }
        );
    }
    #[test]
    fn interrupted_reservation_can_retry_under_writer_limits_and_invalid_cache_cannot_adopt() {
        let mut e = editor();
        e.status.years[0].pending = true;
        assert_eq!(
            e.event(DOWNLOAD, FormData::new()).unwrap(),
            Command::Download(2026)
        );
        assert!(
            e.view(false, None)
                .actions
                .iter()
                .find(|a| a.id == DOWNLOAD)
                .unwrap()
                .enabled
        );
        assert!(e.view(false, None).description.contains("可能中断"));
        let mut bad = status(2, 1);
        bad.candidate.as_mut().unwrap().papers.clear();
        e.replace(ScheduleDocument::default(), bad);
        assert!(e.event(REVIEW, FormData::new()).is_err());
        assert!(e
            .view(false, None)
            .description
            .contains("空年度或下载失败不能证明"));
        let mut wrong = status(1, 1);
        wrong.selected_year = 2027;
        e.replace(ScheduleDocument::default(), wrong);
        assert!(e.event(REVIEW, FormData::new()).is_err());
    }
    #[test]
    fn list_and_review_views_are_valid_and_busy_disables_all_actions() {
        let mut e = editor();
        let view = e.view(true, Some("\0🙂".repeat(9000)));
        assert_eq!(view.validate(), Ok(()));
        assert!(view.actions.iter().all(|a| !a.enabled));
        assert!(view.description.contains("尚未读取核验通知正文"));
        assert!(view.description.contains("MIT"));
        e.event(REVIEW, FormData::new()).unwrap();
        let view = e.view(true, None);
        assert_eq!(view.validate(), Ok(()));
        assert!(view.actions.iter().all(|a| !a.enabled));
        e.event(CANCEL_REVIEW, FormData::new()).unwrap();
        e.event(AUTO_REVIEW, FormData::new()).unwrap();
        assert_eq!(e.view(false, None).validate(), Ok(()));
    }
    #[test]
    fn cancelled_review_cannot_be_repurposed_as_policy_confirmation() {
        let mut e = editor();
        e.event(REVIEW, FormData::new()).unwrap();
        assert!(e.event(CONFIRM_AUTO, yes()).is_err());
        assert!(e.event(AUTO_REVIEW, FormData::new()).is_err());
        e.event(CANCEL_REVIEW, FormData::new()).unwrap();
        e.event(AUTO_REVIEW, FormData::new()).unwrap();
        assert!(e.event(CONFIRM_ADOPT, yes()).is_err());
    }
}
