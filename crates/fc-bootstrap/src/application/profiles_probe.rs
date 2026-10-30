//! Real owned controls and shared-worker persistence in the isolated application
//! fixture. Synthetic preview pixels never authorize desktop or file effects.
use super::*;
use fc_domain::{
    layout::{LayoutScope, LayoutTree, NodeKind, WidgetKind},
    profile::{SubjectProfileDocument, SubjectProfileKey},
};
use fc_ports::PublicLayoutState;
use fc_presentation::layout::SceneNodeKey;
use profiles_editor::{self as e, Command as C};

pub(super) const STEPS: u8 = 39;

fn submit(
    app: &App,
    action: ActionId,
    values: &[(FieldId, &str)],
) -> std::result::Result<(), String> {
    let mut data = FormData::new();
    for (id, value) in values {
        data.insert(*id, (*value).into())
            .map_err(|_| "布局探针字段无效")?;
    }
    for field in app.view().fields {
        if field.kind != FieldKind::ReadOnly && data.get(field.id).is_none() {
            data.insert(field.id, field.value)
                .map_err(|_| "布局探针字段重复")?;
        }
    }
    app.window.probe_submit(action, data).map_err(|_| {
        format!(
            "布局原生探针操作{}未接纳：{} / {}",
            action.0,
            app.view().description,
            app.status
        )
    })
}

// PREVIEW only clones a validated non-secret draft; it neither marks the draft
// submitted nor performs a write. Actual preview work is separately dispatched
// by native Submit events and the ordinary shared worker.
fn drafts(
    app: &mut App,
) -> std::result::Result<(Option<PublicLayoutState>, Option<SubjectProfileDocument>), String> {
    let identity = app.state.clone();
    match app.profiles.as_mut().ok_or("布局编辑器缺失")?.handle(
        e::PREVIEW,
        FormData::new(),
        &identity,
    )? {
        C::Preview { public, subject } => Ok((public, subject)),
        _ => Err("布局探针没有取得当前草稿".into()),
    }
}
fn public(app: &mut App) -> std::result::Result<PublicLayoutState, String> {
    drafts(app)?.0.ok_or("布局探针当前不是公共草稿".into())
}
fn subject(app: &mut App) -> std::result::Result<SubjectProfileDocument, String> {
    drafts(app)?.1.ok_or("布局探针当前不是科目草稿".into())
}
fn widget(tree: &LayoutTree, kind: WidgetKind) -> std::result::Result<LayoutNodeId, String> {
    tree.nodes
        .iter()
        .find(|n| n.kind == NodeKind::Widget(kind))
        .map(|n| n.id)
        .ok_or("布局探针组件不存在".into())
}
fn reordered(tree: &LayoutTree) -> bool {
    let (Ok(clock), Ok(date)) = (
        widget(tree, WidgetKind::Clock),
        widget(tree, WidgetKind::Date),
    ) else {
        return false;
    };
    tree.nodes.iter().any(|n| {
        match (
            n.children.iter().position(|id| *id == date),
            n.children.iter().position(|id| *id == clock),
        ) {
            (Some(d), Some(c)) => d < c,
            _ => false,
        }
    })
}
fn verify_public(value: &PublicLayoutState, saved: bool) -> std::result::Result<(), String> {
    if value.layout.revision != Revision::new(u64::from(saved))
        || !value.layout.tree.nodes.iter().any(|n| {
            n.id == value.layout.tree.root && matches!(n.kind, NodeKind::Root { gap: 23, .. })
        })
        || value.display.glass_surface_percent != 7
        || !reordered(&value.layout.tree)
    {
        return Err("公共布局联合保存的版本、根间距、公共样式或拖拽顺序不符".into());
    }
    Ok(())
}
fn key(app: &App) -> std::result::Result<SubjectProfileKey, String> {
    Ok(SubjectProfileKey {
        class_id: ClassId::new(1).map_err(|_| "探针班级无效")?,
        subject_id: SubjectId::new(1).map_err(|_| "探针科目无效")?,
        teacher_id: app
            .state
            .account_by_login("probe-teacher")
            .map_err(|_| "探针教师缺失")?
            .principal
            .id,
    })
}
fn verify_subject(
    app: &App,
    value: &SubjectProfileDocument,
    revision: u64,
    enabled: bool,
) -> std::result::Result<(), String> {
    if value.key != key(app)?
        || value.revision != Revision::new(revision)
        || (revision > 0 && value.layout_revision != Revision::new(1))
        || value.enabled != enabled
        || value.materials.visible_rows != 3
        || value.materials.row_gap_dip != 23
    {
        return Err("科目配置精确任教、独立修订、启停或资料样式未持久化".into());
    }
    Ok(())
}
fn row(app: &App, disabled: bool) -> std::result::Result<(), String> {
    let view = app.view();
    let grid = view.grid.as_ref().ok_or("科目配置目录缺少表格")?;
    if grid.rows.len() != 1 || (disabled && !grid.rows[0].cells.iter().any(|s| s.contains("停用")))
    {
        return Err("科目配置目录没有精确返回新配置及启停状态".into());
    }
    app.window
        .probe_select_grid(0, 0)
        .map_err(|_| "科目配置原生行选择失败".into())
}

/// Computes geometry with the actual hidden native renderer. No screenshot,
/// global input, foreground activation or desktop publication is used.
fn canvas(app: &mut App, drag: bool) -> std::result::Result<(), String> {
    if !app.preview_active || app.preview_sequence == 0 {
        return Err(format!("真实原生预览未成功安装：{}", app.status));
    }
    let value = public(app)?;
    let clock = widget(&value.layout.tree, WidgetKind::Clock)?;
    let date = widget(&value.layout.tree, WidgetKind::Date)?;
    let now = app.clock.try_now().map_err(|_| "布局探针时钟不可用")?;
    let image = profiles_preview::render(&app.state, &value, None, app.preview_sequence, now)?;
    if image.bgra.is_empty()
        || image.width > 960
        || image.height > 540
        || image.bgra.len() > 2 * 1024 * 1024
    {
        return Err("原生预览像素容量不符".into());
    }
    let point = |id, edge| -> std::result::Result<(u16, u16), String> {
        let n = image
            .nodes
            .iter()
            .find(|n| {
                n.key
                    == SceneNodeKey {
                        scope: LayoutScope::Public,
                        id,
                    }
            })
            .ok_or("原生预览节点缺失")?;
        let left = n.rect.x.max(n.clip.x).max(0.);
        let top = n.rect.y.max(n.clip.y).max(0.);
        let right = (n.rect.x + n.rect.width)
            .min(n.clip.x + n.clip.width)
            .min(f32::from(image.width));
        let bottom = (n.rect.y + n.rect.height)
            .min(n.clip.y + n.clip.height)
            .min(f32::from(image.height));
        if right - left < 4. || bottom - top < 4. {
            return Err("原生预览节点没有可点击区域".into());
        }
        Ok((
            ((left + right) / 2.) as u16,
            // Stay inside after the owned child maps raster coordinates to
            // integer client pixels and back; one pixel is too close to an
            // antialiased fractional boundary on a downscaled preview.
            (if edge { top + 3. } else { (top + bottom) / 2. }) as u16,
        ))
    };
    let from = point(if drag { date } else { clock }, false)?;
    let to = point(clock, drag)?;
    drop(image);
    app.window
        .probe_layout_drag(from, to)
        .map_err(|_| "原生预览选择或拖拽未接纳".into())
}

/// The caller invokes one stage only after all previous shared work completes.
/// The deliberately stale save also uses the real shared worker, never a UI DB
/// read or write, and confirms the persisted result remains intact afterwards.
pub(super) fn run(app: &mut App, stage: &mut u8) -> std::result::Result<(), String> {
    match *stage {
        0 => {
            if app.screen != Screen::Home {
                return Err("布局探针必须从主管理员首页开始".into());
            }
            submit(app, TO_DISPLAY, &[])?;
        }
        1 => submit(app, TO_PROFILES, &[])?,
        2 => submit(app, e::PUBLIC, &[])?,
        3 => submit(app, e::PREVIEW, &[])?,
        4 => {
            let value = public(app)?;
            if value.layout.revision != Revision::default()
                || value
                    .display
                    .component(fc_domain::desktop::ComponentKind::Clock)
                    .is_none_or(|c| c.opacity_percent != 0)
            {
                return Err("隔离夹具必须为未保存公共布局及 0% 时钟".into());
            }
            canvas(app, false)?;
        }
        5 => {
            let value = public(app)?;
            let expected = SceneNodeKey {
                scope: LayoutScope::Public,
                id: widget(&value.layout.tree, WidgetKind::Clock)?,
            };
            if app.profiles.as_ref().and_then(|e| e.selected_node_key()) != Some(expected)
                || !app.preview_active
            {
                return Err("0% 时钟原生选择或同页 Choice 更新丢失了预览画布".into());
            }
            canvas(app, true)?;
        }
        6 => {
            if !reordered(&public(app)?.layout.tree) {
                return Err(format!("原生画布拖拽未改变稳定节点顺序：{}", app.status));
            }
            app.window
                .probe_select_grid(0, 0)
                .map_err(|_| "布局根行选择失败")?;
        }
        7 => submit(app, e::EDIT_NODE, &[])?,
        8 => submit(app, e::BACK, &[(e::GAP, "23")])?,
        9 => submit(app, e::PUBLIC_STYLE, &[])?,
        10 => submit(
            app,
            display_editor::SAVE_ALL,
            &[(display_editor::GLASS, "7")],
        )?,
        11 => submit(app, e::PREVIEW, &[])?,
        12 => {
            if !app.preview_active {
                return Err(format!("修改后的原生预览失败：{}", app.status));
            }
            verify_public(&public(app)?, false)?;
            submit(app, e::REVIEW, &[])?;
        }
        13 => submit(app, e::CONFIRM, &[])?,
        14 => {
            verify_public(&public(app)?, true)?;
            submit(app, e::CANCEL, &[])?;
        }
        15 => submit(app, e::PUBLIC, &[])?,
        16 => {
            let value = public(app)?;
            verify_public(&value, true)?;
            app.dispatch_profile_command(C::SavePublic {
                expected_layout_revision: Revision::default(),
                expected_display_revision: value.display.revision,
                layout: value.layout,
                display: value.display,
            })?;
        }
        17 => {
            if !app.status.contains("已被修改") {
                return Err(format!("旧公共布局 CAS 未明确拒绝：{}", app.status));
            }
            verify_public(&public(app)?, true)?;
            submit(app, e::CANCEL, &[])?;
        }
        18 => {
            let teacher = key(app)?.teacher_id.get().to_string();
            submit(
                app,
                e::NEW,
                &[(e::CLASS, "1"), (e::SUBJECT, "1"), (e::TEACHER, &teacher)],
            )?;
        }
        19 => submit(app, e::MATERIALS, &[])?,
        20 => submit(app, e::BACK, &[(e::ROWS, "3"), (e::ROW_GAP, "23")])?,
        21 => submit(app, e::PREVIEW, &[])?,
        22 => {
            if !app.preview_active {
                return Err(format!("科目配置真实原生预览失败：{}", app.status));
            }
            let value = subject(app)?;
            verify_subject(app, &value, 0, true)?;
            submit(app, e::REVIEW, &[])?;
        }
        23 => submit(app, e::CONFIRM, &[])?,
        24 => {
            let value = subject(app)?;
            verify_subject(app, &value, 1, true)?;
            submit(app, e::CANCEL, &[])?;
        }
        25 => submit(app, e::FIRST, &[])?,
        26 => row(app, false)?,
        27 => submit(app, e::OPEN, &[])?,
        28 => {
            let value = subject(app)?;
            verify_subject(app, &value, 1, true)?;
            submit(app, e::MATERIALS, &[])?;
        }
        29 => submit(app, e::BACK, &[(e::ENABLED, "0")])?,
        30 => submit(app, e::REVIEW, &[])?,
        31 => submit(app, e::CONFIRM, &[])?,
        32 => {
            let value = subject(app)?;
            verify_subject(app, &value, 2, false)?;
            submit(app, e::CANCEL, &[])?;
        }
        33 => submit(app, e::FIRST, &[])?,
        34 => row(app, true)?,
        35 => submit(app, e::OPEN, &[])?,
        36 => {
            let value = subject(app)?;
            verify_subject(app, &value, 2, false)?;
            submit(app, e::CANCEL, &[])?;
        }
        37 => submit(app, e::CLOSE, &[])?,
        38 => {
            if app.screen != Screen::Home
                || app.current_global_owner().is_none()
                || app.has_unsaved_changes()
                || app.preview_active
            {
                return Err("布局探针未清理画布或残留未保存草稿".into());
            }
        }
        _ => return Ok(()),
    }
    *stage += 1;
    Ok(())
}
