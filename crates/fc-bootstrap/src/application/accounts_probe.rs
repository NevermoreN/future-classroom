//! Owned native-control checks inside the existing isolated application fixture.
use super::*;
use accounts_editor as e;

pub(super) const CREATION_STEPS: u8 = 8;
pub(super) const MANAGEMENT_STEPS: u8 = 36;

fn submit(
    app: &App,
    action: ActionId,
    values: &[(FieldId, &str)],
) -> std::result::Result<(), String> {
    let mut data = FormData::new();
    for (id, value) in values {
        data.insert(*id, (*value).into())
            .map_err(|_| "账号探针字段无效")?;
    }
    for field in app.view().fields {
        if field.kind != FieldKind::ReadOnly && data.get(field.id).is_none() {
            data.insert(field.id, field.value)
                .map_err(|_| "账号探针字段重复")?;
        }
    }
    app.window.probe_submit(action, data).map_err(|_| {
        format!(
            "账号原生探针操作{}未接纳：{} / {}",
            action.0,
            app.view().description,
            app.status
        )
    })
}
fn select(app: &App, login: &str) -> std::result::Result<(), String> {
    let row = app
        .view()
        .grid
        .as_ref()
        .and_then(|g| {
            g.rows
                .iter()
                .position(|r| r.cells.first().is_some_and(|s| s == login))
        })
        .ok_or("账号列表未包含预期登录名")?;
    app.window
        .probe_select_grid(row, 0)
        .map_err(|_| "账号原生行选择未接纳".into())
}

/// Replaces the earlier single-scope free-text creation step with the real editor.
pub(super) fn create(app: &mut App, stage: &mut u8) -> std::result::Result<(), String> {
    match *stage {
        0 => submit(app, e::NEW, &[])?,
        1 => submit(
            app,
            e::SCOPES,
            &[
                (e::LOGIN, "probe-teacher"),
                (e::NAME, "验证教师"),
                (e::ROLE, "teacher"),
            ],
        )?,
        2 => submit(app, e::ADD_SCOPE, &[(e::CLASS, "1"), (e::SUBJECT, "1")])?,
        3 => submit(app, e::RETURN, &[])?,
        4 => submit(app, e::REVIEW, &[])?,
        5 => submit(app, e::CONFIRM, &[])?,
        6 => submit(
            app,
            e::CONFIRM,
            &[
                (e::PASSWORD, "Probe-teacher-initial-2026"),
                (e::CONFIRM_PASSWORD, "Probe-teacher-initial-2026"),
            ],
        )?,
        7 => {
            if app.screen != Screen::Code || app.state.accounts.len() != 2 {
                return Err(format!("账号原生创建未产生一次恢复码：{}", app.status));
            }
            let account = app
                .state
                .account_by_login("probe-teacher")
                .map_err(|_| "教师未持久化")?;
            if !account.must_change_password
                || app
                    .state
                    .assignments
                    .iter()
                    .filter(|s| s.teacher_id == account.principal.id)
                    .count()
                    != 1
            {
                return Err("账号原生创建任教范围或首次改密状态错误".into());
            }
        }
        _ => return Ok(()),
    }
    *stage += 1;
    Ok(())
}

/// Uses only the protected fixture and real own controls; never global input.
pub(super) fn manage(app: &mut App, stage: &mut u8) -> std::result::Result<(), String> {
    match *stage {
        0 => {
            let teacher = app
                .state
                .account_by_login("probe-teacher")
                .map_err(|_| "引用负例教师缺失")?
                .principal
                .id;
            let mut document = app
                .schedule
                .as_ref()
                .ok_or("引用负例课表缺失")?
                .document
                .clone();
            let cell = document
                .semesters
                .first_mut()
                .and_then(|p| p.semester.tables.first_mut())
                .and_then(|t| t.rows.first_mut())
                .and_then(|r| r.courses[0].as_mut())
                .ok_or("引用负例课程缺失")?;
            cell.teacher = fc_domain::schedule::TeacherBinding::Explicit(teacher);
            document
                .validate_draft(&app.state)
                .map_err(|_| "引用负例课表无效")?;
            let backend = app.backend.as_ref().ok_or("引用负例后台缺失")?.clone();
            let lease = app.lease.ok_or("引用负例无主管理员会话")?;
            app.submit(Work::SaveSchedule(
                backend,
                lease,
                document.revision,
                document,
            ))?;
        }

        1 => {
            let teacher = app
                .state
                .account_by_login("probe-teacher")
                .map_err(|_| "引用负例教师缺失")?
                .principal
                .id;
            if !app
                .schedule
                .as_ref()
                .and_then(|e| e.document.semesters.first())
                .and_then(|p| p.semester.tables.first())
                .and_then(|t| t.rows.first())
                .and_then(|r| r.courses[0].as_ref())
                .is_some_and(|c| {
                    c.teacher == fc_domain::schedule::TeacherBinding::Explicit(teacher)
                })
            {
                return Err("引用负例未通过生产writer实际保存".into());
            }
            submit(app, TO_SCHOOL, &[])?;
        }
        2 => submit(
            app,
            ADD_CLASS,
            &[(SCHOOL, "隔离验证学校"), (CLASS, "第二验证班")],
        )?,
        3 => submit(app, TO_ACCOUNTS, &[])?,
        4 => select(app, "probe-teacher")?,
        5 => submit(app, e::EDIT, &[])?,
        6 => submit(app, e::SCOPES, &[])?,
        7 => submit(app, e::ADD_SCOPE, &[(e::CLASS, "2"), (e::SUBJECT, "1")])?,
        8 => submit(app, e::RETURN, &[])?,
        9 => submit(app, e::REVIEW, &[])?,
        10 => submit(app, e::CONFIRM, &[])?,
        11 => {
            let teacher = app
                .state
                .account_by_login("probe-teacher")
                .map_err(|_| "教师缺失")?
                .principal
                .id;
            if app
                .state
                .assignments
                .iter()
                .filter(|s| s.teacher_id == teacher && s.enabled)
                .count()
                != 2
                || app.screen != Screen::Accounts
            {
                return Err(format!("多班级任教变更未保存：{}", app.status));
            }
            select(app, "probe-teacher")?;
        }
        12 => submit(app, e::EDIT, &[])?,
        13 => submit(app, e::REVIEW, &[(e::ENABLED, "0")])?,
        14 => submit(app, e::CONFIRM, &[])?,
        15 => {
            if !app
                .state
                .account_by_login("probe-teacher")
                .is_ok_and(|a| a.principal.enabled)
                || !app.status.contains("先修正引用")
            {
                return Err(format!(
                    "被课表引用的账号停用未原子拒绝或未显示原因：{}",
                    app.status
                ));
            }
            submit(app, e::RETURN, &[])?;
        }
        16 => submit(app, e::CANCEL, &[])?,
        17 => submit(app, e::DISCARD, &[])?,
        18 => select(app, "probe-admin")?,
        19 => submit(app, e::EDIT, &[])?,
        20 => submit(app, e::REVIEW, &[(e::NAME, "验证主管理员改名")])?,
        21 => submit(app, e::CONFIRM, &[])?,
        22 => {
            if app.screen != Screen::Login
                || app.lease.is_some()
                || !app
                    .state
                    .account_by_login("probe-admin")
                    .is_ok_and(|a| a.display_name == "验证主管理员改名")
                || !app.status.contains("变更已保存")
            {
                return Err(format!(
                    "账号自修改成功被心跳取消或残留旧授权：{}",
                    app.status
                ));
            }
            submit(
                app,
                LOG_IN,
                &[
                    (LOGIN, "probe-admin"),
                    (PASSWORD, "Probe-admin-password-2026"),
                ],
            )?;
        }
        23 => submit(app, TO_ACCOUNTS, &[])?,
        24 => select(app, "probe-admin")?,
        25 => submit(app, e::EDIT, &[])?,
        26 => submit(app, e::REVIEW, &[(e::ENABLED, "0")])?,
        27 => {
            if !app.status.contains("最后")
                || !app
                    .state
                    .account_by_login("probe-admin")
                    .is_ok_and(|a| a.principal.enabled)
            {
                return Err("最后主管理员预检保护未生效".into());
            }
            submit(app, e::CANCEL, &[])?;
        }
        28 => submit(app, e::DISCARD, &[])?,
        29 => select(app, "probe-teacher")?,
        30 => submit(app, e::EDIT, &[])?,
        31 => submit(app, e::APPLY_ROLE, &[(e::ROLE, "user")])?,
        32 => {
            if !app.view().description.contains("普通用户不能任教") {
                return Err("角色降级未要求明确清空范围".into());
            }
            submit(app, e::KEEP, &[])?;
        }
        33 => {
            // Keeping the old role must leave no silent scope loss. Finish with
            // an explicit cancel; an unchanged draft returns to the account list.
            let teacher = app
                .state
                .account_by_login("probe-teacher")
                .map_err(|_| "教师缺失")?;
            if teacher.principal.role != Role::TeacherAdmin || !teacher.principal.enabled {
                return Err("取消角色变更误写账号".into());
            }
            submit(app, e::CANCEL, &[])?;
        }
        34 => submit(app, e::CLOSE, &[])?,
        35 => {
            if app.screen != Screen::Home {
                return Err("账号探针结束未返回主管理员工作台".into());
            }
        }
        _ => return Err("账号管理探针阶段无效".into()),
    }
    *stage += 1;
    Ok(())
}
