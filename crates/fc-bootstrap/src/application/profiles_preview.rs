//! Isolated, synthetic layout preview. These samples are never DesktopFacts or
//! an authorization source. Hidden renderer surfaces never publish to desktop.
use fc_domain::{
    desktop::{ClockMode, ComponentKind, ComponentSettings},
    identity::{IdentityState, Rgb},
    ids::*,
    layout::{LayoutScope, LayoutTree, NodeKind, WidgetKind},
    materials::{MaterialSource, MaterialSourceEvidence, MaterialSourceKind},
    profile::{ProfileSelectionReason, SubjectProfileDocument},
    profile_content::{ContentSlotKey, PublishedContentDescriptor, PublishedContentSnapshot},
    schedule::{CalendarMode, LessonInstanceKey, ResolvedScheduleVersion},
    time::{LocalDateTime, Minute, MinuteRange},
};
use fc_ports::PublicLayoutState;
use fc_presentation::{
    card_body::card_body_spec,
    layout::{compose_layout, PaintGlass, SceneProfileStamp, WidgetMeasure},
    scene::{
        CardContent, ClockContent, ContentTextSource, CourseCapsule, HomeworkEntry,
        MaterialListContent, PageInfo, SceneCard, SceneContentIdentity, SceneImageContent,
        SceneMaterialRow, SceneNodeKey, SceneSnapshot,
    },
    Rect,
};
use fc_runtime::authority::AuthorityStore;
use fc_windows::materials::raster::RasterCache;
use fc_windows::{
    desktop::{
        DesktopRenderer, NativeTextMeasure, PresentOutcome, PublicationGuard, DESKTOP_SCOPE,
    },
    workbench::{LayoutPreview, PreviewNode},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const WORK: Rect = Rect {
    x: 0.,
    y: 0.,
    width: 1920.,
    height: 1080.,
};
const MAX_PREVIEW_BYTES: usize = 2 * 1024 * 1024;

pub(super) fn render(
    identity: &IdentityState,
    public: &PublicLayoutState,
    subject: Option<&SubjectProfileDocument>,
    sequence: u64,
    now: LocalDateTime,
) -> Result<LayoutPreview, String> {
    render_impl(
        identity,
        public,
        subject,
        sequence,
        now,
        None,
        None,
        Arc::new(RasterCache::new()),
    )
}
/// Synthetic colour samples use the selected stable school subject and the
/// common display draft. No product scene, desktop capture or published layout.
pub(super) fn render_subject_visuals(
    identity: &IdentityState,
    display: &fc_domain::desktop::DisplaySettings,
    selected: SubjectId,
    sequence: u64,
    now: LocalDateTime,
) -> Result<LayoutPreview, String> {
    let mut example = identity.clone();
    let index = example
        .subjects
        .iter()
        .position(|s| s.id == selected)
        .ok_or("所选科目已不存在，请重新加载。")?;
    example.subjects.swap(0, index);
    let public = PublicLayoutState {
        layout: Default::default(),
        display: display.clone(),
    };
    let mut image = render(&example, &public, None, sequence, now)?;
    // Colour samples are read-only; never expose layout drag/drop capabilities.
    image.nodes.clear();
    Ok(image)
}
/// The ordinary preview supplies no content authority. Published independent
/// content requires this exact-slot cache path; no body or pixels are fabricated.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_with_content(
    identity: &IdentityState,
    public: &PublicLayoutState,
    subject: Option<&SubjectProfileDocument>,
    sequence: u64,
    now: LocalDateTime,
    contents: &[PublishedContentSnapshot],
    text: &dyn ContentTextSource,
    raster: Arc<RasterCache>,
) -> Result<LayoutPreview, String> {
    render_impl(
        identity,
        public,
        subject,
        sequence,
        now,
        Some(contents),
        Some(text),
        raster,
    )
}
#[allow(clippy::too_many_arguments)]
fn render_impl(
    identity: &IdentityState,
    public: &PublicLayoutState,
    subject: Option<&SubjectProfileDocument>,
    sequence: u64,
    now: LocalDateTime,
    contents: Option<&[PublishedContentSnapshot]>,
    text: Option<&dyn ContentTextSource>,
    raster: Arc<RasterCache>,
) -> Result<LayoutPreview, String> {
    let scene = prepare_scene(identity, public, subject, sequence, now, contents, text)?;
    let editable = subject.map_or(LayoutScope::Public, |p| LayoutScope::Subject(p.key));
    // This private, ephemeral gate is only for synthetic hidden DIB rendering.
    // It cannot authorize product effects, real classroom facts or file opening.
    let (mut publisher, reader) = AuthorityStore::new((), scene.source_revision, AuthEpoch::new(1));
    publisher.initialize().map_err(|_| "无法初始化隔离预览。")?;
    let gate = reader.gate();
    let lease = gate
        .lease_for_authorized_scope(DESKTOP_SCOPE, Instant::now() + Duration::from_secs(30))
        .map_err(|_| "无法建立隔离预览租约。")?;
    let guard = PublicationGuard {
        gate: &gate,
        lease,
        current_generation: scene.generation,
        settings_open: false,
    };
    let mut renderer = DesktopRenderer::new_hidden().map_err(|_| "无法创建隐藏预览渲染器。")?;
    renderer.set_raster_cache(raster);
    match renderer
        .present(&scene, now, &guard)
        .map_err(|_| "原生示例预览绘制失败。")?
    {
        PresentOutcome::Offscreen => (),
        PresentOutcome::ContentPending => return Err("图片尚未准备；请先完成预览图片准备。".into()),
        _ => return Err("隔离预览被拒绝。".into()),
    }
    let image = renderer
        .export_preview_bgra(WORK, 960, 540)
        .map_err(|_| "预览位图超出预算或无法导出。")?;
    if image.bgra.len() > MAX_PREVIEW_BYTES || image.bgra.capacity() > MAX_PREVIEW_BYTES {
        return Err("预览位图超过 2 MiB。".into());
    }
    let tree = scene.paint_tree.as_ref().ok_or("预览树缺失。")?;
    let nodes = tree
        .nodes
        .iter()
        .filter(|n| n.key.scope == editable)
        .map(|node| PreviewNode {
            key: node.key,
            parent: node.parent.filter(|p| p.scope == editable),
            rect: normalize(node.rect, image.source_work, image.scale_x, image.scale_y),
            clip: normalize(node.clip, image.source_work, image.scale_x, image.scale_y),
        })
        .collect();
    Ok(LayoutPreview {
        generation: sequence,
        width: image.width,
        height: image.height,
        bgra: image.bgra,
        nodes,
    })
}

/// Computes the same final geometry as render_with_content, without allocating
/// surfaces or pixels. The caller prepares these references on its shared worker.
#[allow(clippy::too_many_arguments)]
pub(super) fn image_requests_with_content(
    identity: &IdentityState,
    public: &PublicLayoutState,
    subject: Option<&SubjectProfileDocument>,
    sequence: u64,
    now: LocalDateTime,
    contents: &[PublishedContentSnapshot],
    text: &dyn ContentTextSource,
) -> Result<Vec<fc_windows::desktop::ImageRenderRequest>, String> {
    let scene = prepare_scene(
        identity,
        public,
        subject,
        sequence,
        now,
        Some(contents),
        Some(text),
    )?;
    DesktopRenderer::image_requests(&scene).map_err(|_| "图片预览元数据无效。".into())
}
#[allow(clippy::too_many_arguments)]
fn prepare_scene(
    identity: &IdentityState,
    public: &PublicLayoutState,
    subject: Option<&SubjectProfileDocument>,
    sequence: u64,
    now: LocalDateTime,
    contents: Option<&[PublishedContentSnapshot]>,
    text: Option<&dyn ContentTextSource>,
) -> Result<SceneSnapshot, String> {
    if sequence == 0 {
        return Err("预览代次无效。".into());
    }
    public
        .layout
        .validate_structure()
        .map_err(|_| "公共预览结构无效。")?;
    public
        .display
        .validate()
        .map_err(|_| "预览显示设置无效。")?;
    if let Some(profile) = subject {
        profile
            .validate_structure()
            .map_err(|_| "科目预览结构无效。")?;
    }
    let measure = NativeTextMeasure::new().map_err(|_| "无法初始化原生文字测量。")?;
    let base = samples(identity, public, subject, now)?;
    let samples = bound_samples(public, subject, &base, contents, text)?;
    let specs: Vec<_> = samples.iter().map(|sample| {
        let c = &sample.component;
        Ok(WidgetMeasure {
            key: sample.key,
            kind: sample.widget,
            body: card_body_spec(c.kind, &sample.content, false, None)
                .map_err(|_| "预览内容无法完成有界测量。".to_owned())?,
            font_dip: f32::from(c.font_dip),
            opacity: f32::from(c.opacity_percent) / 100.,
            min_width: f32::from(c.min_width), max_width: f32::from(c.max_width),
            min_height: f32::from(c.min_height).max(if matches!(sample.content, CardContent::Clock(clock) if clock.mode == ClockMode::Analog) {160.} else {84.}),
            max_height: f32::from(c.max_height),
            preferred_width: if c.kind == ComponentKind::Homework {480.} else {280.},
            glass: PaintGlass {
                surface_alpha: f32::from(public.display.glass_surface_percent) / 100.,
                edge_alpha: f32::from(public.display.edge_reflection_percent) / 100.,
            },
        })
    }).collect::<Result<_, String>>()?;
    let mut tree = compose_layout(
        &public.layout.tree,
        subject.map(|p| &p.tree),
        &specs,
        WORK,
        &measure,
    )
    .map_err(|_| "布局在 1920×1080 示例工作区中无法完成排版，请检查尺寸和内容区约束。")?;
    tree.public_layout_revision = public.layout.revision;
    tree.subject_profile = subject.map(|p| SceneProfileStamp {
        id: p.id,
        key: p.key,
        revision: p.revision,
        layout_revision: p.layout_revision,
    });
    tree.selection_reason = if subject.is_some() {
        ProfileSelectionReason::Selected
    } else {
        ProfileSelectionReason::Public
    };
    let mut cards = Vec::new();
    for node in &mut tree.nodes {
        let Some(kind) = node.widget_kind else {
            continue;
        };
        let sample = samples
            .iter()
            .find(|s| s.key == node.key && s.widget == kind)
            .ok_or("预览组件样例缺失。")?;
        node.card_index = Some(cards.len());
        cards.push(SceneCard {
            content_identity: sample.content_identity,
            kind: sample.component.kind,
            rect: node.rect,
            font_dip: f32::from(sample.component.font_dip),
            component_alpha: f32::from(sample.component.opacity_percent) / 100.,
            surface_alpha: f32::from(public.display.glass_surface_percent) / 100.,
            edge_alpha: f32::from(public.display.edge_reflection_percent) / 100.,
            content: sample.content.clone(),
            page: sample.page,
            visible_leaf_limit: sample.leaf_limit,
        });
    }
    // Repeated content widgets share the same finite visible-leaf budget as
    // production. Complete sample rows remain owned for native pagination.
    if cards.len() > 64 {
        return Err("预览组件超过可见叶子预算。".into());
    }
    let wanted: Vec<_> = cards.iter().map(|c| c.visible_leaf_limit).collect();
    for card in &mut cards {
        card.visible_leaf_limit = 1;
    }
    let mut remaining = 64 - cards.len();
    loop {
        let mut assigned = false;
        for (card, wanted) in cards.iter_mut().zip(&wanted) {
            if remaining > 0 && card.visible_leaf_limit < *wanted {
                card.visible_leaf_limit += 1;
                remaining -= 1;
                assigned = true;
            }
        }
        if !assigned {
            break;
        }
    }
    let scene = SceneSnapshot {
        generation: SceneGeneration::new(sequence),
        source_revision: Revision::new(1),
        captured_at: now,
        valid_until: None,
        cards,
        paint_tree: Some(tree),
        text_tone: public.display.text_tone,
        clock_rollback: false,
        public_calendar_unverified: false,
        lesson_control: None,
    };
    Ok(scene)
}

fn normalize(rect: Rect, work: Rect, sx: f32, sy: f32) -> Rect {
    Rect {
        x: (rect.x - work.x) * sx,
        y: (rect.y - work.y) * sy,
        width: rect.width * sx,
        height: rect.height * sy,
    }
}
struct BoundSample<'a> {
    key: SceneNodeKey,
    sample: std::borrow::Cow<'a, Sample>,
    content_identity: Option<SceneContentIdentity>,
}
impl std::ops::Deref for BoundSample<'_> {
    type Target = Sample;
    fn deref(&self) -> &Sample {
        &self.sample
    }
}
fn selected_nodes(tree: &LayoutTree, replace: bool) -> Vec<(SceneNodeKey, WidgetKind)> {
    fn visit(
        tree: &LayoutTree,
        id: LayoutNodeId,
        replace: bool,
        out: &mut Vec<(SceneNodeKey, WidgetKind)>,
    ) {
        let Some(node) = tree.nodes.iter().find(|n| n.id == id) else {
            return;
        };
        if replace && matches!(node.kind, NodeKind::Outlet { .. }) {
            return;
        }
        if let NodeKind::Widget(widget) = node.kind {
            out.push((
                SceneNodeKey {
                    scope: tree.scope,
                    id,
                },
                widget,
            ));
        }
        for child in &node.children {
            visit(tree, *child, replace, out);
        }
    }
    let mut out = Vec::new();
    visit(tree, tree.root, replace, &mut out);
    out
}
fn bound_samples<'a>(
    public: &PublicLayoutState,
    subject: Option<&SubjectProfileDocument>,
    base: &'a [Sample],
    contents: Option<&[PublishedContentSnapshot]>,
    provider: Option<&dyn ContentTextSource>,
) -> Result<Vec<BoundSample<'a>>, String> {
    let mut nodes = selected_nodes(&public.layout.tree, subject.is_some());
    if let Some(subject) = subject {
        nodes.extend(selected_nodes(&subject.tree, false));
    }
    let mut result = Vec::new();
    for (key, widget) in nodes {
        if !matches!(widget, WidgetKind::SubjectText | WidgetKind::SubjectImage) {
            if let Some(sample) = base.iter().find(|s| s.widget == widget) {
                result.push(BoundSample {
                    key,
                    sample: std::borrow::Cow::Borrowed(sample),
                    content_identity: None,
                });
            }
            continue;
        }
        let styles = match key.scope {
            LayoutScope::Public => &public.layout.content_styles,
            LayoutScope::Subject(_) => &subject.ok_or("科目预览缺失。")?.content_styles,
        };
        let style = styles
            .iter()
            .find(|s| s.node_id == key.id)
            .ok_or("独立内容样式缺失。")?;
        if !style.component.visible {
            continue;
        }
        let slot = ContentSlotKey {
            scope: key.scope,
            node_id: key.id,
        };
        let snapshot = contents
            .and_then(|snapshots| snapshots.iter().find(|s| s.slot == slot))
            .ok_or("独立内容尚未准备；请使用已发布内容预览。")?;
        snapshot.validate().map_err(|_| "独立内容描述无效。")?;
        let content = match &snapshot.content {
            PublishedContentDescriptor::Empty => continue,
            PublishedContentDescriptor::Text {
                characters,
                utf8_bytes,
                sha256,
            } => {
                if widget != WidgetKind::SubjectText {
                    return Err("独立内容类型不匹配。".into());
                }
                let text = provider
                    .and_then(|p| p.text(slot, snapshot.revision, *sha256))
                    .ok_or("文字尚未准备。")?;
                if text.len() != *utf8_bytes as usize
                    || text.chars().count() != *characters as usize
                    || text.len() > 40_000
                    || text.chars().count() > 10_000
                    || text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                {
                    return Err("文字缓存与发布内容不匹配。".into());
                }
                CardContent::StandaloneText(text)
            }
            PublishedContentDescriptor::Image {
                current,
                total_pages,
            } => {
                if widget != WidgetKind::SubjectImage || current.owner != slot {
                    return Err("图片槽位不匹配。".into());
                }
                CardContent::Image(SceneImageContent {
                    current: current.clone(),
                    current_page: snapshot.current_page,
                    total_pages: *total_pages,
                })
            }
        };
        result.push(BoundSample {
            key,
            content_identity: Some(SceneContentIdentity {
                slot,
                revision: snapshot.revision,
                view_revision: snapshot.view_revision,
            }),
            sample: std::borrow::Cow::Owned(Sample {
                widget,
                component: style.component.clone(),
                content,
                page: PageInfo {
                    offset: 0,
                    shown: 1,
                    total: 1,
                },
                leaf_limit: 1,
            }),
        });
    }
    Ok(result)
}
#[derive(Clone)]
struct Sample {
    widget: WidgetKind,
    component: ComponentSettings,
    content: CardContent,
    page: PageInfo,
    leaf_limit: u8,
}
fn samples(
    identity: &IdentityState,
    public: &PublicLayoutState,
    subject: Option<&SubjectProfileDocument>,
    now: LocalDateTime,
) -> Result<Vec<Sample>, String> {
    // Arbitrary positive fixture IDs are deliberately confined to this example;
    // no timetable, material repository, Shell or product snapshot receives them.
    let class_id = subject
        .map(|s| s.key.class_id)
        .or_else(|| identity.classes.first().map(|c| c.id))
        .unwrap_or(ClassId::new(1).map_err(|_| "示例 ID 无效。")?);
    let subject_id = subject
        .map(|s| s.key.subject_id)
        .or_else(|| identity.subjects.first().map(|s| s.id))
        .unwrap_or(SubjectId::new(1).map_err(|_| "示例 ID 无效。")?);
    let teacher_id = subject
        .map(|s| s.key.teacher_id)
        .or_else(|| identity.accounts.first().map(|a| a.principal.id))
        .unwrap_or(PrincipalId::new(1).map_err(|_| "示例 ID 无效。")?);
    let subject_name = identity
        .subjects
        .iter()
        .find(|s| s.id == subject_id)
        .map_or("示例科目", |s| s.name.as_str());
    let teacher_name = identity
        .accounts
        .iter()
        .find(|a| a.principal.id == teacher_id)
        .map_or("示例老师", |a| a.display_name.as_str());
    let color = identity
        .subjects
        .iter()
        .find(|s| s.id == subject_id)
        .map_or(
            Rgb {
                red: 175,
                green: 195,
                blue: 215,
            },
            |s| s.color,
        );
    let color = fc_presentation::scene::soft_subject_color(color);
    let semester = SemesterId::new(1).map_err(|_| "示例 ID 无效。")?;
    let lesson = LessonInstanceKey {
        actual_date: now.date,
        class_id,
        range: MinuteRange::new(
            Minute::new(480).map_err(|_| "示例时间无效。")?,
            Minute::new(525).map_err(|_| "示例时间无效。")?,
        )
        .map_err(|_| "示例时间无效。")?,
        version: ResolvedScheduleVersion {
            actual_semester_id: semester,
            actual_revision: Revision::new(1),
            source_semester_id: semester,
            source_revision: Revision::new(1),
            calendar_mode: CalendarMode::School,
            calendar_mode_revision: Revision::new(1),
            applied_calendar_revision: Revision::new(1),
            makeup_mapping_revision: Revision::new(1),
            source_table_id: TableId::new(1).map_err(|_| "示例 ID 无效。")?,
            source_row_id: RowId::new(1).map_err(|_| "示例 ID 无效。")?,
            rules_revision: Revision::new(1),
        },
    };
    let materials_settings = subject.map(|p| &p.materials);
    let rows = materials_settings.map_or(5, |m| m.visible_rows);
    let folder_id = subject
        .and_then(|p| p.folder_id)
        .unwrap_or(FolderId::new(1).map_err(|_| "示例 ID 无效。")?);
    let mut files = Vec::new();
    for number in 1..=u64::from(rows) {
        files.push(SceneMaterialRow {
            material_id: MaterialId::new(number).map_err(|_| "示例 ID 无效。")?,
            resource_version: ResourceVersionId::new(number).map_err(|_| "示例 ID 无效。")?,
            name: format!("示例资料 {number} · 课堂练习.pdf"),
        });
    }
    let course_fill = if public.display.subject_fill {
        f32::from(public.display.course_fill_percent) / 100.
    } else {
        0.
    };
    let entries = vec![
        (
            WidgetKind::Clock,
            CardContent::Clock(ClockContent {
                mode: public.display.clock_mode,
                show_seconds: public.display.clock_seconds,
            }),
            1,
        ),
        (
            WidgetKind::Date,
            CardContent::Text(format!(
                "示例日期 · {}年{}月{}日",
                now.date.year(),
                now.date.month(),
                now.date.day()
            )),
            1,
        ),
        (
            WidgetKind::Schedule,
            CardContent::Courses(vec![CourseCapsule {
                key: lesson.clone(),
                subject_id,
                name: format!("示例 · {subject_name}"),
                color,
                current: true,
                temporary: false,
                fill_alpha: course_fill,
                stroke_width: if public.display.subject_fill { 0. } else { 1. },
            }]),
            1,
        ),
        (
            WidgetKind::Homework,
            CardContent::Homework(vec![HomeworkEntry {
                subject_id,
                teacher_id,
                subject_name: format!("示例 · {subject_name}"),
                teacher_name: teacher_name.into(),
                text: "示例作业：阅读课堂材料，完成练习并记录思考。\n此内容仅用于布局预览。".into(),
                color,
                fill_alpha: if public.display.subject_fill {
                    f32::from(public.display.homework_fill_percent) / 100.
                } else {
                    0.
                },
            }]),
            1,
        ),
        (
            WidgetKind::Reminder,
            CardContent::Text("示例共同提醒：请准备课堂用品。".into()),
            1,
        ),
        (
            WidgetKind::DateCountdown,
            CardContent::Text("示例活动 · 还有 12 天".into()),
            1,
        ),
        (
            WidgetKind::Materials,
            CardContent::Materials(Box::new(MaterialListContent {
                source: MaterialSource {
                    kind: MaterialSourceKind::CurrentLesson,
                    lesson,
                    subject_id,
                    teacher_id,
                    evidence: MaterialSourceEvidence::BaseTimetableOnly,
                },
                folder_id: Some(folder_id),
                folder_revision: Revision::new(1),
                teacher_name: format!("示例 · {teacher_name}"),
                subject_name: subject_name.into(),
                rows: files,
                next_cursor: None,
                total: u64::from(rows),
                visible_rows: rows,
                row_gap_dip: materials_settings.map_or(8, |m| m.row_gap_dip),
            })),
            rows,
        ),
    ];
    let mut result = Vec::new();
    for (widget, content, leaf_limit) in entries {
        let kind = widget.component_kind().ok_or("未知预览组件。")?;
        let component = if kind == ComponentKind::Materials {
            materials_settings
                .map(|m| &m.component)
                .or_else(|| public.display.component(kind))
        } else {
            public.display.component(kind)
        }
        .ok_or("预览样式缺失。")?;
        if component.visible {
            result.push(Sample {
                widget,
                component: component.clone(),
                content,
                page: PageInfo {
                    offset: 0,
                    shown: if kind == ComponentKind::Materials {
                        u64::from(rows)
                    } else {
                        1
                    },
                    total: if kind == ComponentKind::Materials {
                        u64::from(rows)
                    } else {
                        1
                    },
                },
                leaf_limit,
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subject_visual_samples_use_shared_soft_color_and_both_fill_modes() {
        let subject_id = SubjectId::new(7).unwrap();
        let raw = Rgb {
            red: 240,
            green: 10,
            blue: 100,
        };
        let identity = IdentityState {
            subjects: vec![fc_domain::identity::SchoolSubject {
                id: subject_id,
                school_id: SchoolId::new(1).unwrap(),
                name: "Synthetic palette".into(),
                enabled: true,
                color: raw,
                revision: Revision::new(1),
            }],
            ..Default::default()
        };
        let now =
            LocalDateTime::new(fc_domain::time::LocalDate::new(2026, 11, 1).unwrap(), 0).unwrap();
        for enabled in [true, false] {
            let public = PublicLayoutState {
                layout: Default::default(),
                display: fc_domain::desktop::DisplaySettings {
                    subject_fill: enabled,
                    course_fill_percent: 73,
                    homework_fill_percent: 41,
                    ..Default::default()
                },
            };
            let samples = samples(&identity, &public, None, now).unwrap();
            let course = samples
                .iter()
                .find_map(|s| {
                    if let CardContent::Courses(c) = &s.content {
                        c.first()
                    } else {
                        None
                    }
                })
                .unwrap();
            let homework = samples
                .iter()
                .find_map(|s| {
                    if let CardContent::Homework(c) = &s.content {
                        c.first()
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(
                course.color,
                fc_presentation::scene::soft_subject_color(raw)
            );
            assert_eq!(homework.color, course.color);
            assert_eq!(course.fill_alpha, if enabled { 0.73 } else { 0. });
            assert_eq!(homework.fill_alpha, if enabled { 0.41 } else { 0. });
            assert_eq!(course.stroke_width, if enabled { 0. } else { 1. });
        }
    }
    #[test]
    fn subject_visual_actual_hidden_pixels_change_with_color_and_alpha_under_original_budget() {
        let subject_id = SubjectId::new(7).unwrap();
        let mut identity = IdentityState {
            subjects: vec![fc_domain::identity::SchoolSubject {
                id: subject_id,
                school_id: SchoolId::new(1).unwrap(),
                name: "Synthetic palette".into(),
                enabled: true,
                color: Rgb {
                    red: 240,
                    green: 10,
                    blue: 100,
                },
                revision: Revision::new(1),
            }],
            ..Default::default()
        };
        let now =
            LocalDateTime::new(fc_domain::time::LocalDate::new(2026, 11, 1).unwrap(), 0).unwrap();
        let mut display = fc_domain::desktop::DisplaySettings {
            course_fill_percent: 73,
            homework_fill_percent: 41,
            ..Default::default()
        };
        let first = render_subject_visuals(&identity, &display, subject_id, 1, now).unwrap();
        identity.subjects[0].color = Rgb {
            red: 20,
            green: 200,
            blue: 100,
        };
        let color = render_subject_visuals(&identity, &display, subject_id, 2, now).unwrap();
        display.subject_fill = false;
        let outlined = render_subject_visuals(&identity, &display, subject_id, 3, now).unwrap();
        assert_eq!((first.width, first.height), (960, 540));
        assert!(first.bgra.capacity() <= MAX_PREVIEW_BYTES);
        assert_ne!(first.bgra, color.bgra);
        assert_ne!(color.bgra, outlined.bgra);
    }
    struct Provider(Vec<(ContentSlotKey, Revision, [u8; 32], Arc<str>)>);
    impl ContentTextSource for Provider {
        fn text(
            &self,
            slot: ContentSlotKey,
            revision: Revision,
            hash: [u8; 32],
        ) -> Option<Arc<str>> {
            self.0
                .iter()
                .find(|entry| entry.0 == slot && entry.1 == revision && entry.2 == hash)
                .map(|entry| entry.3.clone())
        }
    }
    fn independent_public() -> (PublicLayoutState, Vec<PublishedContentSnapshot>, Provider) {
        use fc_domain::{
            layout::{AxisSize, ContainerSettings, Flow, GlassMaterial, LayoutNode},
            profile_content::{ContentNodeStyle, ContentPayloadKind},
        };
        let mut public = PublicLayoutState {
            layout: Default::default(),
            display: Default::default(),
        };
        public.layout.revision = Revision::new(1);
        public.layout.node_id_high_water = 101;
        for component in &mut public.display.components {
            component.visible = false;
        }
        let fallback: Vec<_> = public
            .layout
            .tree
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Widget(WidgetKind::Materials)))
            .map(|n| n.id)
            .collect();
        public
            .layout
            .tree
            .nodes
            .retain(|n| !fallback.contains(&n.id));
        let outlet = public
            .layout
            .tree
            .nodes
            .iter_mut()
            .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
            .unwrap();
        // Both independent cards share one top-level glass group. The seven
        // protected builtin cards remain in the structural budget even while
        // their display styles are invisible in this focused fixture.
        let group_id = LayoutNodeId::new(99).unwrap();
        outlet.children = vec![group_id];
        public.layout.tree.nodes.push(LayoutNode {
            id: group_id,
            kind: NodeKind::Container(ContainerSettings {
                flow: Flow::Column,
                padding: 0,
                gap: 8,
                width: AxisSize::Content {
                    min: 160,
                    preferred: 300,
                    max: 800,
                },
                height: AxisSize::Content {
                    min: 0,
                    preferred: 0,
                    max: 2160,
                },
                group_opacity_percent: 100,
                material: Some(GlassMaterial::default()),
            }),
            children: vec![
                LayoutNodeId::new(100).unwrap(),
                LayoutNodeId::new(101).unwrap(),
            ],
        });
        let mut snapshots = Vec::new();
        let mut entries = Vec::new();
        for (id, text) in [
            (100, "第一份真实正文\n保留换行"),
            (101, "第二份真实正文，与第一份不同。"),
        ] {
            let id = LayoutNodeId::new(id).unwrap();
            public.layout.tree.nodes.push(LayoutNode {
                id,
                kind: NodeKind::Widget(WidgetKind::SubjectText),
                children: vec![],
            });
            let mut style = ContentNodeStyle::new(id, ContentPayloadKind::Text);
            style.component.font_dip = 18;
            public.layout.content_styles.push(style);
            let slot = ContentSlotKey {
                scope: LayoutScope::Public,
                node_id: id,
            };
            let hash = [id.get() as u8; 32];
            snapshots.push(PublishedContentSnapshot {
                slot,
                revision: Revision::new(2),
                view_revision: Revision::new(3),
                current_page: 0,
                content: PublishedContentDescriptor::Text {
                    characters: text.chars().count() as u32,
                    utf8_bytes: text.len() as u32,
                    sha256: hash,
                },
            });
            entries.push((slot, Revision::new(2), hash, Arc::from(text)));
        }
        public.layout.validate().unwrap();
        let metrics = public.layout.tree.metrics().unwrap();
        assert_eq!(metrics.top_level_glass, 8);
        assert_eq!(metrics.glass_depth, 2);
        (public, snapshots, Provider(entries))
    }
    #[test]
    fn real_content_preview_uses_exact_slots_and_refuses_missing_or_wrong_cache() {
        let (public, snapshots, mut provider) = independent_public();
        let now = LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(),
            9 * 3600,
        )
        .unwrap();
        let scene = prepare_scene(
            &IdentityState::default(),
            &public,
            None,
            200,
            now,
            Some(&snapshots),
            Some(&provider),
        )
        .unwrap();
        assert_eq!(scene.cards.len(), 2);
        assert_ne!(
            scene.cards[0].content_identity,
            scene.cards[1].content_identity
        );
        assert!(
            matches!(&scene.cards[0].content,CardContent::StandaloneText(text) if text.contains("第一份")&&text.contains('\n'))
        );
        assert!(
            matches!(&scene.cards[1].content,CardContent::StandaloneText(text) if text.contains("第二份"))
        );
        assert!(render(&IdentityState::default(), &public, None, 201, now)
            .unwrap_err()
            .contains("尚未准备"));
        provider.0[0].2 = [0; 32];
        assert!(prepare_scene(
            &IdentityState::default(),
            &public,
            None,
            202,
            now,
            Some(&snapshots),
            Some(&provider)
        )
        .unwrap_err()
        .contains("尚未准备"));
    }
    #[test]
    fn actual_hidden_preview_renders_exact_published_text_without_placeholder() {
        let (public, snapshots, provider) = independent_public();
        let now = LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(),
            9 * 3600,
        )
        .unwrap();
        let image = render_with_content(
            &IdentityState::default(),
            &public,
            None,
            203,
            now,
            &snapshots,
            &provider,
            Arc::new(RasterCache::new()),
        )
        .unwrap();
        assert!(image
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0));
        for id in [100, 101] {
            assert!(image
                .nodes
                .iter()
                .any(|node| node.key.id.get() == id && node.rect.height > 0.));
        }
        assert!(image.bgra.capacity() <= MAX_PREVIEW_BYTES);
    }
    #[test]
    fn image_coordinates_use_actual_export_transform() {
        assert_eq!(
            normalize(
                Rect {
                    x: 120.,
                    y: 70.,
                    width: 200.,
                    height: 100.
                },
                Rect {
                    x: 20.,
                    y: 10.,
                    width: 1920.,
                    height: 1080.
                },
                0.5,
                0.5
            ),
            Rect {
                x: 50.,
                y: 30.,
                width: 100.,
                height: 50.
            }
        );
    }
    #[test]
    fn sample_is_explicit_non_recess_and_respects_hidden_and_transparent_components() {
        let mut public = PublicLayoutState {
            layout: Default::default(),
            display: Default::default(),
        };
        let now = LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(),
            9 * 3600,
        )
        .unwrap();
        public
            .display
            .components
            .iter_mut()
            .find(|c| c.kind == ComponentKind::Clock)
            .unwrap()
            .opacity_percent = 0;
        public
            .display
            .components
            .iter_mut()
            .find(|c| c.kind == ComponentKind::Reminder)
            .unwrap()
            .visible = false;
        let entries = samples(&IdentityState::default(), &public, None, now).unwrap();
        assert_eq!(
            entries
                .iter()
                .find(|s| s.widget == WidgetKind::Clock)
                .unwrap()
                .component
                .opacity_percent,
            0
        );
        assert!(!entries
            .iter()
            .any(|s| matches!(s.widget, WidgetKind::Recess | WidgetKind::Reminder)));
        let CardContent::Materials(files) = &entries
            .iter()
            .find(|s| s.widget == WidgetKind::Materials)
            .unwrap()
            .content
        else {
            panic!("material samples missing")
        };
        assert_eq!(files.rows.len(), 5);
        assert!(files.rows.iter().all(|r| r.name.starts_with("示例")));
        assert!(files.folder_id.is_some());
    }
    #[test]
    fn native_hidden_preview_is_bounded_and_selects_only_exact_subject_even_at_zero_alpha() {
        let key = fc_domain::profile::SubjectProfileKey {
            class_id: ClassId::new(11).unwrap(),
            subject_id: SubjectId::new(12).unwrap(),
            teacher_id: PrincipalId::new(13).unwrap(),
        };
        let public = PublicLayoutState {
            layout: Default::default(),
            display: Default::default(),
        };
        let mut subject = SubjectProfileDocument::new(ProfileId::new(7).unwrap(), key);
        subject.materials.component.opacity_percent = 0;
        subject.materials.visible_rows = 2;
        subject.materials.row_gap_dip = 16;
        let now = LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(),
            9 * 3600,
        )
        .unwrap();
        let image = render(&IdentityState::default(), &public, Some(&subject), 99, now).unwrap();
        assert_eq!(
            (image.generation, image.width, image.height),
            (99, 960, 540)
        );
        assert_eq!(image.bgra.len(), 960 * 540 * 4);
        assert!(image.bgra.capacity() <= MAX_PREVIEW_BYTES);
        assert_eq!(image.nodes.len(), 2);
        assert!(image
            .nodes
            .iter()
            .all(|n| n.key.scope == LayoutScope::Subject(key)));
        let root = image
            .nodes
            .iter()
            .find(|n| n.key.id == subject.tree.root)
            .unwrap();
        assert!(root.parent.is_none());
        assert!(image
            .nodes
            .iter()
            .any(|n| n.key.id != subject.tree.root && n.rect.width > 0. && n.rect.height > 0.));
        assert!(image.bgra.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        assert!(image
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3].iter().all(|c| *c <= p[3])));
    }

    #[test]
    fn native_preview_material_spacing_expands_the_actual_subject_card() {
        let key = fc_domain::profile::SubjectProfileKey {
            class_id: ClassId::new(11).unwrap(),
            subject_id: SubjectId::new(12).unwrap(),
            teacher_id: PrincipalId::new(13).unwrap(),
        };
        let mut public = PublicLayoutState {
            layout: Default::default(),
            display: Default::default(),
        };
        for component in &mut public.display.components {
            component.visible = component.kind == ComponentKind::Materials;
        }
        let mut subject = SubjectProfileDocument::new(ProfileId::new(7).unwrap(), key);
        subject.materials.visible_rows = 3;
        subject.materials.component.max_height = 1000;
        subject.materials.row_gap_dip = 0;
        let now = LocalDateTime::new(
            fc_domain::time::LocalDate::new(2026, 10, 31).unwrap(),
            9 * 3600,
        )
        .unwrap();
        let compact = render(&IdentityState::default(), &public, Some(&subject), 101, now).unwrap();
        subject.materials.row_gap_dip = 64;
        let spacious =
            render(&IdentityState::default(), &public, Some(&subject), 102, now).unwrap();
        let leaf_height = |image: &LayoutPreview| {
            image
                .nodes
                .iter()
                .find(|n| n.key.id != subject.tree.root)
                .unwrap()
                .rect
                .height
        };
        // This observes the exported native preview's actual model rectangle,
        // rather than deriving an expected value from the shared measurer.
        assert!(leaf_height(&spacious) > leaf_height(&compact) + 80.);
        assert_eq!(compact.nodes.len(), spacious.nodes.len());
        assert_eq!((spacious.width, spacious.height), (960, 540));
        assert_eq!(spacious.bgra.len(), 960 * 540 * 4);
        assert!(spacious.bgra.capacity() <= MAX_PREVIEW_BYTES);
    }
}
