//! Owned hidden-window diagnostics. Real native form events use the ordinary
//! shared worker, SQLite, strict preparation and management preview. Only the
//! modal picker is replaced by two tiny files in this isolated test deployment.
use super::*;
use fc_bootstrap::profile_content_cache::TextFrame;
use fc_domain::{
    layout::{LayoutScope, NodeKind, WidgetKind},
    profile_content::{ContentSlotKey, PublishedContentDescriptor},
};
use fc_ports::PublicLayoutState;
use fc_presentation::scene::ContentTextSource;
use fc_windows::materials::{chooser::SelectedFile, files::ControlledFiles};
use std::{io::Write, sync::Mutex};
use {profile_content_editor as c, profiles_editor as e};

pub(super) const STEPS: usize = 52;
const NAMES: [&str; STEPS] = [
    "open-display",
    "open-profiles",
    "read-public",
    "capture-original-select-outlet",
    "add-content-group",
    "configure-content-group",
    "select-existing-materials",
    "open-materials-operations",
    "reparent-materials",
    "return-tree",
    "select-content-group",
    "open-group-appearance",
    "enable-group-glass",
    "add-text-node",
    "open-text-style",
    "bound-text-viewport",
    "add-image-node",
    "open-image-style",
    "bound-image-viewport",
    "review-layout",
    "save-clean-layout",
    "verify-layout-save",
    "reread-public",
    "verify-node-styles-select-text",
    "manage-saved-text",
    "native-10000-emoji-review",
    "publish-text",
    "verify-text-and-reread",
    "verify-reread-clear-review",
    "clear-text",
    "verify-empty-undo-review",
    "undo-text",
    "verify-undo-return-tree",
    "select-image",
    "manage-saved-images",
    "select-two-owned-bmps",
    "review-two-images",
    "publish-images",
    "verify-images-select-second",
    "move-second-page-first",
    "review-reordered-images",
    "publish-reordered-images",
    "verify-reorder-and-reread",
    "verify-image-reread-return-tree",
    "request-actual-content-preview",
    "verify-real-arcs-native-preview-and-restore",
    "verify-restoration-save",
    "reread-restored-public",
    "verify-restored-public",
    "return-list",
    "return-home",
    "verify-home-cleanup",
];
#[derive(Default)]
pub(super) struct State {
    original: Option<PublicLayoutState>,
    group: Option<LayoutNodeId>,
    text: Option<LayoutNodeId>,
    image: Option<LayoutNodeId>,
    saved_revision: Revision,
    saved_high_water: u64,
    restored_revision: Revision,
    paths: Vec<PathBuf>,
}
impl Drop for State {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
        // No worker is admitted by this state; the main probe owns drain/close.
        if let Ok(mut witness) = WITNESS.try_lock() {
            *witness = Witness::default();
        }
    }
}
#[derive(Default)]
struct Witness {
    expected: Option<(ContentSlotKey, ContentSlotKey)>,
    sequence: u64,
    text: bool,
    image: bool,
}
static WITNESS: Mutex<Witness> = Mutex::new(Witness {
    expected: None,
    sequence: 0,
    text: false,
    image: false,
});

/// Called ONLY by the isolated hidden probe, after the ordinary result's exact
/// source/Global check, before rendering. Borrow real worker-owned Arcs, retain
/// only booleans/sequence, and never wait for a mutex or perform IO on the UI.
pub(super) fn observe_preview(preview: &profile_flow::Preview) {
    let Ok(mut witness) = WITNESS.try_lock() else {
        return;
    };
    let Some((text_slot, image_slot)) = witness.expected else {
        return;
    };
    let text = preview.contents.iter().find(|s| s.slot == text_slot);
    let image = preview.contents.iter().find(|s| s.slot == image_slot);
    witness.sequence = preview.sequence;
    witness.text = text.is_some_and(|snapshot| {
        snapshot.revision == Revision::new(3)
            && matches!(
                snapshot.content,
                PublishedContentDescriptor::Text {
                    characters: 10000,
                    utf8_bytes: 40000,
                    ..
                }
            )
            && exact_text(&preview.text, snapshot)
    });
    witness.image = image.is_some_and(|snapshot| {
        let PublishedContentDescriptor::Image {
            current,
            total_pages,
        } = &snapshot.content
        else {
            return false;
        };
        snapshot.revision == Revision::new(2)
            && snapshot.current_page == 0
            && *total_pages == 2
            && current.width == 2
            && current.height == 1
            && preview.images.iter().any(|frame| {
                frame.source() == current
                    && frame.key().owner == image_slot
                    && frame.key().width == 2
                    && frame.key().height == 1
                    && frame.pixels().len() == 8
                    && frame
                        .pixels()
                        .chunks_exact(4)
                        .all(|pixel| pixel == [0, 255, 0, 255])
            })
    });
}
fn exact_text(
    frame: &TextFrame,
    snapshot: &fc_domain::profile_content::PublishedContentSnapshot,
) -> bool {
    let PublishedContentDescriptor::Text { sha256, .. } = snapshot.content else {
        return false;
    };
    frame
        .text(snapshot.slot, snapshot.revision, sha256)
        .is_some_and(|body| {
            body.len() == 40000
                && body.chars().count() == 10000
                && body.chars().all(|ch| ch == '😀')
        })
}
fn submit(
    app: &App,
    action: ActionId,
    values: &[(FieldId, &str)],
) -> std::result::Result<(), String> {
    let mut data = FormData::new();
    for (id, value) in values {
        data.insert(*id, (*value).into())
            .map_err(|_| "独立内容探针字段无效")?;
    }
    for field in app.view().fields {
        if field.kind != FieldKind::ReadOnly && data.get(field.id).is_none() {
            data.insert(field.id, field.value)
                .map_err(|_| "独立内容探针字段重复")?;
        }
    }
    app.window.probe_submit(action, data).map_err(|_| {
        format!(
            "独立内容原生操作{}未接纳：{} / {}",
            action.0,
            app.view().description,
            app.status
        )
    })
}
fn public(app: &mut App) -> std::result::Result<PublicLayoutState, String> {
    match app.profiles.as_mut().ok_or("公共布局编辑器缺失")?.handle(
        e::PREVIEW,
        FormData::new(),
        &app.state,
    )? {
        e::Command::Preview {
            public: Some(state),
            subject: None,
        } => Ok(state),
        _ => Err("独立内容探针当前不是公共树".into()),
    }
}
fn select(app: &App, id: LayoutNodeId) -> std::result::Result<(), String> {
    let view = app.view();
    let row = view
        .grid
        .as_ref()
        .ok_or("节点表格缺失")?
        .rows
        .iter()
        .position(|row| row.label.trim() == format!("#{}", id.get()))
        .ok_or("稳定节点行缺失")?;
    app.window
        .probe_select_grid(row, 0)
        .map_err(|_| "原生节点选择失败".into())
}
fn selected(app: &App) -> std::result::Result<LayoutNodeId, String> {
    app.profiles
        .as_ref()
        .and_then(|editor| editor.selected_node_key())
        .filter(|key| key.scope == LayoutScope::Public)
        .map(|key| key.id)
        .ok_or("未选中公共稳定节点".into())
}
fn required(id: Option<LayoutNodeId>) -> std::result::Result<LayoutNodeId, String> {
    id.ok_or("探针节点尚未建立".into())
}
fn slot(id: LayoutNodeId) -> ContentSlotKey {
    ContentSlotKey {
        scope: LayoutScope::Public,
        node_id: id,
    }
}
fn text(app: &App, revision: u64, empty: bool) -> std::result::Result<(), String> {
    if app.screen != Screen::ProfileContent {
        return Err("未进入独立文字管理页".into());
    }
    let view = app.view();
    let field = view
        .fields
        .iter()
        .find(|field| field.id == c::TEXT)
        .ok_or("真实正文Edit缺失")?;
    let correct = if empty {
        field.value.is_empty()
    } else {
        field.value.len() == 40000
            && field.value.encode_utf16().count() == 20000
            && field.value.chars().count() == 10000
            && field.value.chars().all(|ch| ch == '😀')
    };
    if !correct
        || field.max_utf16 != 20000
        || !view
            .description
            .contains(&format!("已读取内容修订{revision}；"))
    {
        return Err(format!(
            "文字持久读回或完整Unicode/CAS不符：{} / {}",
            view.description, app.status
        ));
    }
    Ok(())
}
fn images(app: &App, revision: u64, reversed: bool) -> std::result::Result<(), String> {
    if app.screen != Screen::ProfileContent {
        return Err("未进入独立图片管理页".into());
    }
    let view = app.view();
    let grid = view.grid.as_ref().ok_or("图片页面表格缺失")?;
    let expected = if reversed {
        ["2 × 1", "4 × 2"]
    } else {
        ["4 × 2", "2 × 1"]
    };
    if grid.rows.len() != 2
        || !grid
            .rows
            .iter()
            .zip(expected)
            .all(|(row, expected)| row.cells.last().is_some_and(|cell| cell == expected))
        || !view
            .description
            .contains(&format!("已读取内容修订{revision}；"))
    {
        return Err(format!(
            "图片顺序/修订未真实持久读回：{} / {}",
            view.description, app.status
        ));
    }
    Ok(())
}
fn bmp(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let stride = (width * 3).div_ceil(4) * 4;
    let pixels = stride * height;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&(54 + pixels).to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&54u32.to_le_bytes());
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(width as i32).to_le_bytes());
    bytes.extend_from_slice(&(height as i32).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&pixels.to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    for _ in 0..height {
        for _ in 0..width {
            bytes.extend_from_slice(&[rgb[2], rgb[1], rgb[0]]);
        }
        bytes.resize(bytes.len() + (stride - width * 3) as usize, 0);
    }
    bytes
}
fn choose_images(app: &mut App, state: &mut State) -> std::result::Result<(), String> {
    // Probe fixture preparation only: no external path, dialog, source asset,
    // fake published media or filesystem work on a user's normal UI session.
    let files = ControlledFiles::new(&app.root).map_err(|_| "隔离文件目录无效")?;
    let mut selected = Vec::new();
    for (name, bytes) in [
        ("content-ui-probe-red.bmp", bmp(4, 2, [255, 0, 0])),
        ("content-ui-probe-green.bmp", bmp(2, 1, [0, 255, 0])),
    ] {
        // Only a successful exclusive create grants cleanup ownership; a
        // pre-existing name must not be deleted after create_new refuses it.
        let path = app.root.join("quarantine").join(name);
        let mut file = files.create_new(&path).map_err(|_| "无法创建自有BMP夹具")?;
        state.paths.push(path);
        file.file_mut()
            .write_all(&bytes)
            .map_err(|_| "无法写入自有BMP夹具")?;
        file.finish().map_err(|_| "无法封存自有BMP夹具")?;
        selected
            .push(SelectedFile::isolated_probe(&app.root, name).map_err(|_| "隔离选择证明无效")?);
    }
    app.profile_content
        .as_mut()
        .ok_or("图片编辑器缺失")?
        .accept_images(selected)?;
    app.render()
}
fn verify_layout(app: &mut App, state: &State) -> std::result::Result<(), String> {
    let value = public(app)?;
    if value.layout.revision != state.saved_revision
        || ![required(state.text)?, required(state.image)?]
            .into_iter()
            .all(|id| {
                value.layout.content_styles.iter().any(|style| {
                    style.node_id == id
                        && style.component.font_dip == 10
                        && style.component.max_height == 112
                })
            })
        || app
            .profiles
            .as_ref()
            .is_none_or(|editor| editor.has_unsaved_changes())
    {
        return Err("独立内容节点布局/样式未保持已保存clean状态".into());
    }
    Ok(())
}
fn verify_restored(app: &mut App, state: &State) -> std::result::Result<(), String> {
    let observed = public(app)?;
    let original = state.original.as_ref().ok_or("原公共配置快照缺失")?;
    let mut display = observed.display.clone();
    display.revision = original.display.revision;
    if observed.layout.tree != original.layout.tree
        || observed.layout.content_styles != original.layout.content_styles
        || observed.layout.revision != state.restored_revision
        || observed.layout.node_id_high_water < state.saved_high_water
        || display != original.display
    {
        return Err("独立内容探针未恢复原公共树/显示语义或单调修订".into());
    }
    Ok(())
}

/// One bounded step, called only after ordinary shared work has completed. The
/// final true means all52 steps passed; the parent retains the existing180s cap.
pub(super) fn run(
    app: &mut App,
    state: &mut State,
    stage: &mut usize,
) -> std::result::Result<bool, String> {
    if *stage >= STEPS {
        return Ok(true);
    }
    if app.pending.is_some() || app.stopping {
        return Ok(false);
    }
    match *stage {
        0 => {
            if app.screen != Screen::Home
                || app.current_global_owner().is_none()
                || app.preview_active
                || app.has_unsaved_changes()
            {
                return Err("独立内容探针需从clean本地主管理员首页开始".into());
            }
            submit(app, TO_DISPLAY, &[])?;
        }
        1 => submit(app, TO_PROFILES, &[])?,
        2 => submit(app, e::PUBLIC, &[])?,
        3 => {
            let value = public(app)?;
            if !value.layout.content_styles.is_empty() {
                return Err("合成夹具已有独立内容，拒绝覆盖".into());
            }
            let outlet = value
                .layout
                .tree
                .nodes
                .iter()
                .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
                .ok_or("公共内容出口缺失")?
                .id;
            state.original = Some(value);
            select(app, outlet)?;
        }
        4 => submit(app, e::ADD_CONTAINER, &[])?,
        5 => {
            state.group = Some(selected(app)?);
            submit(
                app,
                e::BACK,
                &[
                    (e::WIDTH_MIN, "160"),
                    (e::WIDTH_PREF, "300"),
                    (e::WIDTH_MAX, "800"),
                    (e::HEIGHT_MAX, "600"),
                    (e::PADDING, "4"),
                    (e::GAP, "8"),
                ],
            )?;
        }
        6 => {
            let value = public(app)?;
            let material = value
                .layout
                .tree
                .nodes
                .iter()
                .find(|n| n.kind == NodeKind::Widget(WidgetKind::Materials))
                .ok_or("公共资料节点缺失")?
                .id;
            select(app, material)?;
        }
        7 => submit(app, e::NODE_OPERATIONS, &[])?,
        8 => submit(
            app,
            e::REPARENT,
            &[(e::PARENT, &required(state.group)?.get().to_string())],
        )?,
        9 => submit(app, e::BACK, &[])?,
        10 => select(app, required(state.group)?)?,
        11 => submit(app, e::NODE_APPEARANCE, &[])?,
        12 => submit(app, e::BACK, &[(e::GLASS, "1")])?,
        13 => submit(app, e::ADD_WIDGET, &[(e::ADD_KIND, "text")])?,
        14 => {
            state.text = Some(selected(app)?);
            submit(app, e::NODE_APPEARANCE, &[])?;
        }
        15 => submit(
            app,
            e::BACK,
            &[
                (e::FONT, "10"),
                (e::MIN_HEIGHT, "64"),
                (e::MAX_HEIGHT, "112"),
            ],
        )?,
        16 => submit(app, e::ADD_WIDGET, &[(e::ADD_KIND, "image")])?,
        17 => {
            state.image = Some(selected(app)?);
            submit(app, e::NODE_APPEARANCE, &[])?;
        }
        18 => submit(
            app,
            e::BACK,
            &[
                (e::FONT, "10"),
                (e::MIN_HEIGHT, "64"),
                (e::MAX_HEIGHT, "112"),
            ],
        )?,
        19 => submit(app, e::REVIEW, &[])?,
        20 => submit(app, e::CONFIRM, &[])?,
        21 => {
            let saved = public(app)?;
            state.saved_revision = saved.layout.revision;
            state.saved_high_water = saved.layout.node_id_high_water;
            let original = state.original.as_ref().ok_or("原公共配置缺失")?;
            if state.saved_revision.get() != original.layout.revision.get() + 1 {
                return Err("新独立节点没有提交一次公共布局CAS".into());
            }
            verify_layout(app, state)?;
            submit(app, e::CANCEL, &[])?;
        }
        22 => submit(app, e::PUBLIC, &[])?,
        23 => {
            verify_layout(app, state)?;
            select(app, required(state.text)?)?;
        }
        24 => submit(app, e::MANAGE_CONTENT, &[])?,
        25 => {
            text(app, 0, true)?;
            submit(app, c::REVIEW, &[(c::TEXT, &"😀".repeat(10000))])?;
        }
        26 => submit(app, c::CONFIRM, &[])?,
        27 => {
            text(app, 1, false)?;
            submit(app, c::RELOAD, &[])?;
        }
        28 => {
            text(app, 1, false)?;
            submit(app, c::CLEAR, &[])?;
        }
        29 => submit(app, c::CONFIRM, &[])?,
        30 => {
            text(app, 2, true)?;
            submit(app, c::UNDO, &[])?;
        }
        31 => submit(app, c::CONFIRM, &[])?,
        32 => {
            text(app, 3, false)?;
            submit(app, c::CLOSE, &[])?;
        }
        33 => select(app, required(state.image)?)?,
        34 => submit(app, e::MANAGE_CONTENT, &[])?,
        35 => choose_images(app, state)?,
        36 => submit(app, c::REVIEW, &[])?,
        37 => submit(app, c::CONFIRM, &[])?,
        38 => {
            images(app, 1, false)?;
            app.window
                .probe_select_grid(1, 0)
                .map_err(|_| "第二图片原生选择失败")?;
        }
        39 => submit(app, c::UP, &[])?,
        40 => submit(app, c::REVIEW, &[])?,
        41 => submit(app, c::CONFIRM, &[])?,
        42 => {
            images(app, 2, true)?;
            submit(app, c::RELOAD, &[])?;
        }
        43 => {
            images(app, 2, true)?;
            submit(app, c::CLOSE, &[])?;
        }
        44 => {
            let mut witness = WITNESS.try_lock().map_err(|_| "预览见证繁忙")?;
            *witness = Witness {
                expected: Some((slot(required(state.text)?), slot(required(state.image)?))),
                ..Witness::default()
            };
            drop(witness);
            submit(app, e::PREVIEW, &[])?;
        }
        45 => {
            let witness = WITNESS.try_lock().map_err(|_| "预览见证繁忙")?;
            if !app.preview_active
                || witness.sequence != app.preview_sequence
                || !witness.text
                || !witness.image
            {
                return Err(format!(
                    "真实预览正文/图片Arc或native画布不符：text={} image={} seq={} active={} / {}",
                    witness.text, witness.image, witness.sequence, app.preview_active, app.status
                ));
            }
            drop(witness);
            let current = public(app)?;
            let original = state.original.as_ref().ok_or("原公共配置缺失")?;
            app.dispatch_profile_command(e::Command::SavePublic {
                expected_layout_revision: current.layout.revision,
                expected_display_revision: current.display.revision,
                layout: original.layout.clone(),
                display: original.display.clone(),
            })?;
        }
        46 => {
            state.restored_revision = public(app)?.layout.revision;
            if state.restored_revision.get() != state.saved_revision.get() + 1 {
                return Err("恢复公共配置未经过真实CAS提交".into());
            }
            verify_restored(app, state)?;
            submit(app, e::CANCEL, &[])?;
        }
        47 => submit(app, e::PUBLIC, &[])?,
        48 => verify_restored(app, state)?,
        49 => submit(app, e::CANCEL, &[])?,
        50 => submit(app, e::CLOSE, &[])?,
        51 => {
            if app.screen != Screen::Home
                || app.current_global_owner().is_none()
                || app.preview_active
                || app.has_unsaved_changes()
            {
                return Err("独立内容探针未恢复clean首页/释放预览".into());
            }
            for path in &state.paths {
                std::fs::remove_file(path).map_err(|_| "自有BMP夹具清理失败")?;
            }
            state.paths.clear();
            if let Ok(mut witness) = WITNESS.try_lock() {
                *witness = Witness::default();
            }
        }
        _ => unreachable!(),
    }
    eprintln!(
        "PROFILE_CONTENT_UI_STEP index={} name={}",
        *stage, NAMES[*stage]
    );
    *stage += 1;
    if *stage == STEPS {
        eprintln!("PROFILE_CONTENT_UI_PASS steps={STEPS} text=10000emoji clear=undo images=2 reorder=verified preview=actual-arcs restoration=readback");
    }
    Ok(*stage == STEPS)
}
