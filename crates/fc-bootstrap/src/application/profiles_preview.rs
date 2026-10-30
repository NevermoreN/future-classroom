//! Isolated, synthetic layout preview. These samples are never DesktopFacts or
//! an authorization source. Hidden renderer surfaces never publish to desktop.
use fc_domain::{
    desktop::{ClockMode, ComponentKind, ComponentSettings},
    identity::{IdentityState, Rgb},
    ids::*,
    layout::{LayoutScope, WidgetKind},
    materials::{MaterialSource, MaterialSourceEvidence, MaterialSourceKind},
    profile::{ProfileSelectionReason, SubjectProfileDocument},
    schedule::{CalendarMode, LessonInstanceKey, ResolvedScheduleVersion},
    time::{LocalDateTime, Minute, MinuteRange},
};
use fc_ports::PublicLayoutState;
use fc_presentation::{
    card_body::card_body_spec,
    layout::{compose_layout, PaintGlass, SceneProfileStamp, WidgetMeasure},
    scene::{
        CardContent, ClockContent, CourseCapsule, HomeworkEntry, MaterialListContent, PageInfo,
        SceneCard, SceneMaterialRow, SceneSnapshot,
    },
    Rect,
};
use fc_runtime::authority::AuthorityStore;
use fc_windows::{
    desktop::{
        DesktopRenderer, NativeTextMeasure, PresentOutcome, PublicationGuard, DESKTOP_SCOPE,
    },
    workbench::{LayoutPreview, PreviewNode},
};
use std::time::{Duration, Instant};

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
    if sequence == 0 {
        return Err("预览代次无效。".into());
    }
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
    let samples = samples(identity, public, subject, now)?;
    let specs: Vec<_> = samples.iter().map(|sample| {
        let c = &sample.component;
        Ok(WidgetMeasure {
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
            .find(|s| s.widget == kind)
            .ok_or("预览组件样例缺失。")?;
        node.card_index = Some(cards.len());
        cards.push(SceneCard {
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
    let editable = subject.map_or(LayoutScope::Public, |p| LayoutScope::Subject(p.key));
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
    if renderer
        .present(&scene, now, &guard)
        .map_err(|_| "原生示例预览绘制失败。")?
        != PresentOutcome::Offscreen
    {
        return Err("隔离预览被拒绝。".into());
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

fn normalize(rect: Rect, work: Rect, sx: f32, sy: f32) -> Rect {
    Rect {
        x: (rect.x - work.x) * sx,
        y: (rect.y - work.y) * sy,
        width: rect.width * sx,
        height: rect.height * sy,
    }
}
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
                stroke_width: 0.,
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
                fill_alpha: f32::from(public.display.homework_fill_percent) / 100.,
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
        assert!(image.bgra.chunks_exact(4).any(|p| p[3] > 0));
        assert!(image
            .bgra
            .chunks_exact(4)
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
