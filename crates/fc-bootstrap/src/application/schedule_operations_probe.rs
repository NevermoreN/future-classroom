//! Hidden, owned-workbench verification of compound timetable operations.
//! All writes and reloads cross the ordinary App worker and sole SQLite writer.
//! The temporary disabled semester cannot change the actual desktop lesson.
use super::*;
use fc_domain::{schedule::*, schedule_document::*};
use schedule_editor as e;
use std::result::Result;

pub(super) const STEPS: usize = 65;
const NAME: &str = "独立作息操作验证";
const NAMES: [&str; STEPS] = [
    "open-schedule",
    "request-initial-reload",
    "reload-initial",
    "capture-and-create-term",
    "disable-owned-term",
    "add-activity",
    "configure-monday-wednesday-friday",
    "select-activity",
    "review-split",
    "cancel-split",
    "review-split-again",
    "lock-unapplied-review",
    "login-same-owner",
    "resume-schedule",
    "confirm-retained-split",
    "verify-three-days-and-save",
    "verify-first-cas-save",
    "cancel-split-from-terms",
    "reload-split",
    "verify-split-readback",
    "select-first-split",
    "open-first-merge",
    "review-first-merge",
    "confirm-first-merge",
    "verify-two-and-select",
    "open-second-merge",
    "review-second-merge",
    "confirm-second-merge",
    "verify-restored-days-and-enable-dual",
    "review-role-swap",
    "cancel-role-swap",
    "review-role-swap-again",
    "confirm-role-swap",
    "verify-identities-and-save",
    "verify-second-cas-save",
    "cancel-role-swap-from-terms",
    "reload-role-swap",
    "verify-role-readback",
    "add-course",
    "configure-course",
    "select-first-course-cell",
    "choose-first-subject",
    "set-first-course-metadata",
    "switch-second-table",
    "select-second-course-cell",
    "choose-second-subject",
    "set-second-course-metadata",
    "switch-first-table",
    "save-course-fixture",
    "verify-third-cas-save-and-select",
    "review-type-change",
    "cancel-type-change",
    "verify-cancelled-form-and-review",
    "confirm-type-change",
    "verify-synchronized-clear-and-save",
    "verify-fourth-cas-save",
    "cancel-type-change-from-terms",
    "reload-type-change",
    "verify-type-readback",
    "reject-stale-cas",
    "verify-rejection-and-reload",
    "verify-unchanged-and-restore",
    "verify-restoration-and-reload",
    "verify-restored-readback-and-return",
    "verify-home-clean",
];

#[derive(Default)]
pub(super) struct State {
    original: Option<ScheduleDocument>,
    checkpoint: Option<ScheduleDocument>,
    term: Option<SemesterId>,
    before_roles: Option<SemesterPlan>,
    before_type: Option<ScheduleDocument>,
    saved_revision: Revision,
    restored: Option<ScheduleDocument>,
}

fn submit(app: &App, action: ActionId, values: &[(FieldId, &str)]) -> Result<(), String> {
    let mut data = FormData::new();
    for (id, value) in values {
        data.insert(*id, (*value).into())
            .map_err(|_| "作息探针字段重复")?;
    }
    for field in app.view().fields {
        if field.kind != FieldKind::ReadOnly && data.get(field.id).is_none() {
            data.insert(field.id, field.value)
                .map_err(|_| "作息探针表单字段重复")?;
        }
    }
    app.window.probe_submit(action, data).map_err(|_| {
        format!(
            "作息原生命令{}未接纳：{} / {}",
            action.0,
            app.view().title,
            app.status
        )
    })
}
fn document(app: &App) -> Result<&ScheduleDocument, String> {
    Ok(&app.schedule.as_ref().ok_or("作息探针编辑器缺失")?.document)
}
fn term<'a>(app: &'a App, state: &State) -> Result<&'a SemesterPlan, String> {
    let id = state.term.ok_or("自有作息学期标识缺失")?;
    document(app)?
        .semesters
        .iter()
        .find(|p| p.semester.id == id)
        .ok_or_else(|| "自有作息学期缺失".into())
}
fn unchanged(app: &App, expected: &Option<ScheduleDocument>) -> Result<(), String> {
    if Some(document(app)?) != expected.as_ref() {
        return Err("复核或取消在确认前改变了课表草稿".into());
    }
    Ok(())
}
fn review(app: &App) -> Result<(), String> {
    if !app
        .view()
        .actions
        .iter()
        .any(|a| a.id == e::REVIEW_APPLY && a.enabled)
        || !app
            .view()
            .actions
            .iter()
            .any(|a| a.id == e::REVIEW_CANCEL && a.enabled)
    {
        return Err(format!(
            "复核页未出现：{} / {}",
            app.view().title,
            app.status
        ));
    }
    Ok(())
}
fn select(app: &App, row: usize, day: usize) -> Result<(), String> {
    app.window
        .probe_select_grid(row, day)
        .map_err(|_| "作息原生网格选择失败".into())
}
fn open_owned(app: &App, state: &State) -> Result<(), String> {
    let index = document(app)?
        .semesters
        .iter()
        .position(|p| Some(p.semester.id) == state.term)
        .ok_or("自有学期列表缺失")?;
    submit(app, e::OPEN, &[(e::TERM, &index.to_string())])
}
fn saved(app: &App, state: &mut State) -> Result<(), String> {
    let current = document(app)?;
    if current.revision != state.saved_revision.next().map_err(|_| "修订溢出")?
        || app
            .schedule
            .as_ref()
            .is_none_or(|e| e.has_unsaved_changes())
    {
        return Err(format!(
            "真实CAS保存未递增一次或草稿仍dirty：{}",
            app.status
        ));
    }
    state.saved_revision = current.revision;
    state.checkpoint = Some(current.clone());
    Ok(())
}
fn readback(app: &App, state: &State) -> Result<(), String> {
    unchanged(app, &state.checkpoint)?;
    if app.screen != Screen::Schedule
        || app
            .schedule
            .as_ref()
            .is_none_or(|e| e.has_unsaved_changes())
    {
        return Err("普通LoadSchedule未恢复完全相同的clean文档".into());
    }
    Ok(())
}
fn load(app: &mut App) -> Result<(), String> {
    app.submit(Work::LoadSchedule(
        app.backend.as_ref().ok_or("作息探针后台缺失")?.clone(),
        app.lease.ok_or("作息探针会话缺失")?,
    ))
}
fn activity_rows(app: &App, state: &State, expected: &[&[usize]]) -> Result<(), String> {
    let plan = term(app, state)?;
    for table in &plan.semester.tables {
        if table.rows.len() != expected.len() {
            return Err(format!(
                "活动拆合行数不符：{} != {}",
                table.rows.len(),
                expected.len()
            ));
        }
        for (row, days) in table.rows.iter().zip(expected) {
            if row.kind != RowKind::Activity
                || row.enabled_days != std::array::from_fn(|i| days.contains(&i))
                || row.courses.iter().any(Option::is_some)
                || row.range.begin.get() != 600
                || row.range.end.get() != 660
                || plan
                    .row_details
                    .iter()
                    .find(|d| d.row_id == row.id)
                    .is_none_or(|d| d.name != "周一三五活动")
            {
                return Err("活动拆合丢失星期、名称、时间或类型".into());
            }
        }
    }
    Ok(())
}
fn merge_with(app: &App, state: &State) -> Result<(), String> {
    let id = term(app, state)?.semester.tables[0]
        .rows
        .get(1)
        .ok_or("待合并活动缺失")?
        .id;
    submit(app, e::APPLY, &[(e::MERGE_WITH, &id.get().to_string())])
}
fn type_cleared(app: &App, state: &State) -> Result<(), String> {
    let before = state.before_type.as_ref().ok_or("类型变更前快照缺失")?;
    let old = before
        .semesters
        .iter()
        .find(|p| Some(p.semester.id) == state.term)
        .ok_or("类型变更前自有学期缺失")?;
    let next = term(app, state)?;
    let mut new_key = None;
    for (index, (new, old_table)) in next
        .semester
        .tables
        .iter()
        .zip(&old.semester.tables)
        .enumerate()
    {
        let row = new.rows.get(1).ok_or("转换后行缺失")?;
        let original = old_table.rows.get(1).ok_or("转换前行缺失")?;
        let actual_name = next.row_details.iter().find(|d| d.row_id == row.id);
        let expected_name = old.row_details.iter().find(|d| d.row_id == original.id);
        let name_valid = if index == 0 {
            actual_name.is_some_and(|d| d.name == "已确认活动")
        } else {
            actual_name == expected_name
        };
        if row.id != original.id
            || row.kind != RowKind::Activity
            || row.logical_key == original.logical_key
            || row.courses.iter().any(Option::is_some)
            || next.cell_details.iter().any(|d| d.row_id == row.id)
            || !name_valid
            || new_key.is_some_and(|key| key != row.logical_key)
        {
            return Err("类型确认未同步清两表课程/教室/备注或未建立共同新逻辑标识".into());
        }
        new_key = Some(row.logical_key);
    }
    if next.semester.tables.len() != 2 {
        return Err("类型确认后的双表丢失".into());
    }
    Ok(())
}
fn restored(app: &App, state: &State) -> Result<(), String> {
    let observed = document(app)?;
    let original = state.original.as_ref().ok_or("原课表快照缺失")?;
    let mut normalized = observed.clone();
    normalized.revision = original.revision;
    normalized.id_high_water = original.id_high_water;
    if normalized != *original
        || observed.revision <= original.revision
        || observed.id_high_water
            < state
                .checkpoint
                .as_ref()
                .ok_or("最后保存快照缺失")?
                .id_high_water
        || app
            .schedule
            .as_ref()
            .is_none_or(|e| e.has_unsaved_changes())
    {
        return Err("作息探针未恢复原课表语义或回退了已发布修订/标识高水位".into());
    }
    Ok(())
}

/// The parent calls only after pending shared work has drained, exactly once per
/// ordinary native-event tick. No direct DB access or authority bypass occurs.
pub(super) fn run(app: &mut App, state: &mut State, stage: &mut usize) -> Result<(), String> {
    if *stage >= STEPS || app.pending.is_some() || app.stopping {
        return Ok(());
    }
    match *stage {
        0 => {
            if app.screen != Screen::Home
                || app.current_global_owner().is_none()
                || app.has_unsaved_changes()
            {
                return Err("作息探针需要clean主管理员首页".into());
            }
            submit(app, TO_SCHEDULE, &[])?;
        }
        1 => {
            if app
                .schedule
                .as_ref()
                .is_none_or(|e| e.has_unsaved_changes())
            {
                return Err("初始课表不是clean草稿".into());
            }
            submit(app, e::CANCEL, &[])?;
        }
        2 | 18 | 36 | 57 => submit(app, e::CONFIRM_CANCEL, &[])?,
        3 => {
            let doc = document(app)?.clone();
            if doc.semesters.iter().any(|p| p.name == NAME) {
                return Err("拒绝覆盖已有同名学期".into());
            }
            state.saved_revision = doc.revision;
            state.original = Some(doc);
            submit(app, e::NEW_TERM, &[])?;
        }
        4 => {
            state.term = Some(
                document(app)?
                    .semesters
                    .last()
                    .ok_or("新学期未建立")?
                    .semester
                    .id,
            );
            submit(
                app,
                e::APPLY,
                &[
                    (e::TERM_NAME, NAME),
                    (e::START, "2026-09-01"),
                    (e::END, "2027-01-31"),
                    (e::ANCHOR, "2026-08-31"),
                    (e::ENABLED, "0"),
                    (e::TIMING_SYNC, "1"),
                ],
            )?;
        }
        5 | 38 => submit(app, e::ADD_ROW, &[])?,
        6 => submit(
            app,
            e::APPLY,
            &[
                (e::ROW_NAME, "周一三五活动"),
                (e::KIND, "activity"),
                (e::RANGE, "10001100"),
                (FieldId(e::DAY_BASE), "1"),
                (FieldId(e::DAY_BASE + 1), "0"),
                (FieldId(e::DAY_BASE + 2), "1"),
                (FieldId(e::DAY_BASE + 3), "0"),
                (FieldId(e::DAY_BASE + 4), "1"),
                (FieldId(e::DAY_BASE + 5), "0"),
                (FieldId(e::DAY_BASE + 6), "0"),
            ],
        )?,
        7 => {
            activity_rows(app, state, &[&[0, 2, 4]])?;
            state.checkpoint = Some(document(app)?.clone());
            select(app, 0, 0)?;
        }
        8 | 10 => submit(app, e::SPLIT_ACTIVITY, &[])?,
        9 => {
            review(app)?;
            unchanged(app, &state.checkpoint)?;
            submit(app, e::REVIEW_CANCEL, &[])?;
        }
        11 => {
            review(app)?;
            unchanged(app, &state.checkpoint)?;
            submit(app, LOCK, &[])?;
        }
        12 => {
            if app.screen != Screen::Login {
                return Err("复核锁定未进入登录页".into());
            }
            unchanged(app, &state.checkpoint)?;
            submit(
                app,
                LOG_IN,
                &[
                    (LOGIN, "probe-admin"),
                    (PASSWORD, "Probe-admin-password-2026"),
                ],
            )?;
        }
        13 => submit(app, TO_SCHEDULE, &[])?,
        14 => {
            review(app)?;
            unchanged(app, &state.checkpoint)?;
            submit(app, e::REVIEW_APPLY, &[])?;
        }
        15 => {
            activity_rows(app, state, &[&[0], &[2], &[4]])?;
            submit(app, e::SAVE_ALL, &[])?;
        }
        16 | 34 | 55 => {
            saved(app, state)?;
            // Grid has no discard action. Follow the same explicit native
            // navigation as a user: return to Terms, then discard/reload.
            submit(app, e::EDIT_BACK, &[])?;
        }
        17 | 35 | 56 => submit(app, e::CANCEL, &[])?,
        19 | 37 => {
            readback(app, state)?;
            open_owned(app, state)?;
        }
        20 => select(app, 0, 0)?,
        21 | 25 => submit(app, e::MERGE_ACTIVITY, &[])?,
        22 | 26 => {
            state.checkpoint = Some(document(app)?.clone());
            merge_with(app, state)?;
        }
        23 | 27 => {
            review(app)?;
            unchanged(app, &state.checkpoint)?;
            submit(app, e::REVIEW_APPLY, &[])?;
        }
        24 => {
            activity_rows(app, state, &[&[0, 2], &[4]])?;
            select(app, 0, 0)?;
        }
        28 => {
            activity_rows(app, state, &[&[0, 2, 4]])?;
            submit(app, e::ENABLE_DUAL, &[])?;
        }
        29 => {
            let p = term(app, state)?;
            if p.semester.tables.len() != 2
                || p.semester.tables[0].role != TableRole::Odd
                || p.semester.tables[1].role != TableRole::Even
            {
                return Err("双表未建立".into());
            }
            state.before_roles = Some(p.clone());
            state.checkpoint = Some(document(app)?.clone());
            submit(app, e::SWAP_ROLES, &[])?;
        }
        30 => {
            review(app)?;
            unchanged(app, &state.checkpoint)?;
            submit(app, e::REVIEW_CANCEL, &[])?;
        }
        31 => {
            unchanged(app, &state.checkpoint)?;
            submit(app, e::SWAP_ROLES, &[])?;
        }
        32 => {
            review(app)?;
            unchanged(app, &state.checkpoint)?;
            submit(app, e::REVIEW_APPLY, &[])?;
        }
        33 => {
            let p = term(app, state)?;
            let mut normalized = p.clone();
            normalized.semester.tables[0].role = TableRole::Odd;
            normalized.semester.tables[1].role = TableRole::Even;
            if Some(&normalized) != state.before_roles.as_ref()
                || p.semester.tables[0].role != TableRole::Even
                || p.semester.tables[1].role != TableRole::Odd
            {
                return Err("角色交换移动了物理表或改变了课程/元数据/标识".into());
            }
            submit(app, e::SAVE_ALL, &[])?;
        }
        39 => submit(
            app,
            e::APPLY,
            &[
                (e::ROW_NAME, "待转换课程"),
                (e::KIND, "course"),
                (e::RANGE, "11001200"),
            ],
        )?,
        40 | 44 => select(app, 1, 1)?,
        41 | 45 => {
            let subject = app
                .state
                .subjects
                .iter()
                .find(|s| s.name == "数学")
                .ok_or("验证数学科目缺失")?
                .id;
            app.window
                .probe_choice(SUBJECT, &subject.get().to_string())
                .map_err(|_| "作息原生科目选择失败")?;
        }
        42 | 46 => {
            let teacher = app
                .state
                .account_by_login("probe-teacher")
                .map_err(|_| "验证教师缺失")?
                .principal
                .id;
            submit(
                app,
                e::APPLY,
                &[
                    (e::TEACHER, &teacher.get().to_string()),
                    (
                        e::ROOM,
                        if *stage == 42 {
                            "第一表教室"
                        } else {
                            "第二表教室"
                        },
                    ),
                    (e::NOTE, "类型确认后必须真正清除的备注"),
                ],
            )?;
        }
        43 => submit(app, e::OPEN, &[(e::TABLE, "1")])?,
        47 => submit(app, e::OPEN, &[(e::TABLE, "0")])?,
        48 => submit(app, e::SAVE_ALL, &[])?,
        49 => {
            saved(app, state)?;
            let p = term(app, state)?;
            if p.cell_details.len() != 2
                || p.semester.tables.iter().any(|t| {
                    t.rows
                        .get(1)
                        .is_none_or(|r| r.kind != RowKind::Course || r.courses[0].is_none())
                })
            {
                return Err("双表课程及教室备注夹具未真实保存".into());
            }
            state.before_type = Some(document(app)?.clone());
            select(app, 1, 0)?;
        }
        50 => submit(
            app,
            e::APPLY,
            &[(e::KIND, "activity"), (e::ROW_NAME, "已确认活动")],
        )?,
        51 => {
            review(app)?;
            unchanged(app, &state.before_type)?;
            submit(app, e::REVIEW_CANCEL, &[])?;
        }
        52 => {
            unchanged(app, &state.before_type)?;
            let view = app.view();
            if view
                .fields
                .iter()
                .find(|f| f.id == e::KIND)
                .is_none_or(|f| f.value != "activity")
                || view
                    .fields
                    .iter()
                    .find(|f| f.id == e::ROW_NAME)
                    .is_none_or(|f| f.value != "已确认活动")
            {
                return Err("取消类型复核丢失未应用的原生行表单".into());
            }
            submit(app, e::APPLY, &[])?;
        }
        53 => {
            review(app)?;
            unchanged(app, &state.before_type)?;
            submit(app, e::REVIEW_APPLY, &[])?;
        }
        54 => {
            type_cleared(app, state)?;
            submit(app, e::SAVE_ALL, &[])?;
        }
        58 => {
            readback(app, state)?;
            type_cleared(app, state)?;
        }
        59 => {
            let mut stale = state.checkpoint.clone().ok_or("已保存文档缺失")?;
            let expected = Revision::new(stale.revision.get().checked_sub(1).ok_or("无旧修订")?);
            stale.revision = expected;
            app.submit(Work::SaveSchedule(
                app.backend.as_ref().ok_or("后台缺失")?.clone(),
                app.lease.ok_or("会话缺失")?,
                expected,
                stale,
            ))?;
        }
        60 => {
            readback(app, state)?;
            if !app.status.contains("保存版本已变化") {
                return Err(format!("旧CAS未明确拒绝：{}", app.status));
            }
            load(app)?;
        }
        61 => {
            readback(app, state)?;
            let mut original = state.original.clone().ok_or("原课表缺失")?;
            original.revision = state.saved_revision;
            original.id_high_water = document(app)?.id_high_water;
            app.submit(Work::SaveSchedule(
                app.backend.as_ref().ok_or("后台缺失")?.clone(),
                app.lease.ok_or("会话缺失")?,
                state.saved_revision,
                original,
            ))?;
        }
        62 => {
            restored(app, state)?;
            if document(app)?.revision != state.saved_revision.next().map_err(|_| "修订溢出")? {
                return Err("恢复保存未递增一次修订".into());
            }
            state.restored = Some(document(app)?.clone());
            load(app)?;
        }
        63 => {
            restored(app, state)?;
            unchanged(app, &state.restored)?;
            submit(app, e::EDIT_BACK, &[])?;
        }
        64 => {
            restored(app, state)?;
            if app.screen != Screen::Home
                || app.has_unsaved_changes()
                || app.current_global_owner().is_none()
            {
                return Err("作息探针未回到clean主管理员首页".into());
            }
        }
        _ => return Err("作息操作探针阶段无效".into()),
    }
    eprintln!(
        "SCHEDULE_OPERATIONS_PROBE step={} name={}",
        *stage, NAMES[*stage]
    );
    *stage += 1;
    Ok(())
}
