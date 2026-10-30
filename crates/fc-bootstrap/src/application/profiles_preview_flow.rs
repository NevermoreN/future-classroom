//! Management preview preparation. This path permits exact saved profile scopes
//! outside the current lesson, using a fresh Local GlobalAdmin lease throughout.
//! It never turns a missing/denied persisted read into an unpublished draft.
use super::*;
use fc_bootstrap::{
    backend::ProfileContentPreparationGuard, profile_content_files::ControlledContentFiles,
};
use fc_domain::{
    layout::{LayoutScope, LayoutTree, NodeKind, WidgetKind},
    profile_content::{ContentPayload, ContentSlotKey, PublishedContentDescriptor},
};
use fc_runtime::health::Health;
use fc_windows::system::CurrentClock;

fn active_slots(tree: &LayoutTree, replace: bool) -> Vec<(ContentSlotKey, WidgetKind)> {
    fn visit(
        tree: &LayoutTree,
        id: LayoutNodeId,
        replace: bool,
        result: &mut Vec<(ContentSlotKey, WidgetKind)>,
    ) {
        let Some(node) = tree.nodes.iter().find(|node| node.id == id) else {
            return;
        };
        if replace && matches!(node.kind, NodeKind::Outlet { .. }) {
            return;
        }
        if let NodeKind::Widget(kind @ (WidgetKind::SubjectText | WidgetKind::SubjectImage)) =
            node.kind
        {
            result.push((
                ContentSlotKey {
                    scope: tree.scope,
                    node_id: id,
                },
                kind,
            ));
        }
        for id in &node.children {
            visit(tree, *id, replace, result);
        }
    }
    let mut result = Vec::new();
    visit(tree, tree.root, replace, &mut result);
    result
}
/// Only a genuinely new high-water ID may acquire a synthetic Empty descriptor.
/// A retired slot or a different persisted node kind must be an explicit error.
fn saved_slot(
    slot: ContentSlotKey,
    kind: WidgetKind,
    baseline: &LayoutTree,
    water: u64,
) -> Result<bool> {
    if baseline.scope != slot.scope {
        return Err(invalid("预览内容所属布局不一致。"));
    }
    match baseline.nodes.iter().find(|node| node.id == slot.node_id) {
        Some(node) if node.kind == NodeKind::Widget(kind) => Ok(true),
        Some(_) => Err(invalid("已有节点类型已变化，请重新加载布局。")),
        None if slot.node_id.get() > water => Ok(false),
        None => Err(invalid("草稿使用了已退役内容节点，请重新创建该节点。")),
    }
}
struct Budget {
    started: Instant,
    cpu: Duration,
}
impl Budget {
    fn new() -> Result<Self> {
        Ok(Self {
            started: Instant::now(),
            cpu: fc_windows::system::current_thread_cpu_time().map_err(|_| Failure::Stopped)?,
        })
    }
    fn current(
        &self,
        context: &JobContext,
        guard: &ProfileContentPreparationGuard,
        gate: &fc_runtime::health::HealthGate,
        revision: Revision,
    ) -> bool {
        use fc_bootstrap::backend::ProfileContentPreparationState;
        loop {
            if context.checkpoint().is_err()
                || self.started.elapsed() >= Duration::from_secs(30)
                || !fc_windows::system::current_thread_cpu_time()
                    .is_ok_and(|cpu| cpu.saturating_sub(self.cpu) < Duration::from_secs(10))
            {
                return false;
            }
            match guard.state() {
                ProfileContentPreparationState::Revoked => return false,
                ProfileContentPreparationState::AuthorityUpdating => (),
                ProfileContentPreparationState::Ready => {
                    let status = gate.status();
                    match status.health {
                        Health::Healthy => return status.revision == revision,
                        Health::AuthorityUpdating => (),
                        _ => return false,
                    }
                }
            }
            // Worker-only bounded pause. Updating neither revokes this lease
            // nor returns a live slot; cancellation and both budgets still run.
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    backend: Arc<Mutex<Backend>>,
    lease: SessionLease,
    public: Option<PublicLayoutState>,
    subject: Option<SubjectProfileDocument>,
    sequence: u64,
    root: PathBuf,
    context: &JobContext,
) -> Result<Preview> {
    if sequence == 0 {
        return Err(invalid("预览代次无效。"));
    }
    let budget = Budget::new()?;
    let now = CurrentClock::new()
        .try_now()
        .map_err(|_| invalid("无法读取固定预览时间。"))?;
    let (identity, public, baseline_public, baseline_subject, revision, guard, gate) = {
        let backend = backend
            .try_lock()
            .map_err(|_| invalid("内容正在更新，请稍后预览。"))?;
        let revision = backend.global_revision()?;
        let identity = backend.management_identity(lease)?;
        let baseline_public = backend.read_public_layout(lease)?;
        let baseline_subject = subject
            .as_ref()
            .map(|subject| backend.read_subject_profile(lease, subject.key))
            .transpose()?
            .flatten();
        let guard = backend.profile_content_preparation_guard(lease)?;
        let gate = backend.session_control().health_gate();
        if backend.global_revision()? != revision {
            return Err(invalid("预览内容已变化，请重新加载。"));
        }
        let public = public.unwrap_or_else(|| baseline_public.clone());
        (
            identity,
            public,
            baseline_public,
            baseline_subject,
            revision,
            guard,
            gate,
        )
    }; // No Backend lock is held during hashing, DWrite, file IO or WIC.
    public
        .display
        .validate()
        .map_err(|_| invalid("预览显示设置无效。"))?;
    public
        .layout
        .validate_structure()
        .map_err(|_| invalid("公共预览布局无效。"))?;
    if let Some(subject) = &subject {
        subject
            .validate_structure()
            .map_err(|_| invalid("科目预览布局无效。"))?;
        if baseline_subject
            .as_ref()
            .is_some_and(|baseline| baseline.id != subject.id)
        {
            return Err(invalid("科目配置标识已变化，请重新加载。"));
        }
        if baseline_subject.is_none()
            && (subject.revision.get() != 0 || subject.layout_revision.get() != 0)
        {
            return Err(invalid("已保存科目配置不存在，请重新加载。"));
        }
    }
    public
        .layout
        .tree
        .validate_composition(subject.as_ref().map(|subject| &subject.tree))
        .map_err(|_| invalid("草稿布局无法与当前公共内容区组合。"))?;
    let current = || budget.current(context, &guard, &gate, revision);
    if !current() {
        return Err(Failure::Stopped);
    }
    let mut slots = active_slots(&public.layout.tree, subject.is_some());
    if let Some(subject) = &subject {
        slots.extend(active_slots(&subject.tree, false));
    }
    if slots.len() > fc_domain::layout::MAX_VISIBLE_LEAVES {
        return Err(invalid("预览内容超过可见叶子预算。"));
    }
    let mut contents = Vec::with_capacity(slots.len());
    let mut text = TextFrame::default();
    let mut persisted = Vec::new();
    for (slot, kind) in slots {
        if !current() {
            return Err(Failure::Stopped);
        }
        let saved = match slot.scope {
            LayoutScope::Public => saved_slot(
                slot,
                kind,
                &baseline_public.layout.tree,
                baseline_public.layout.node_id_high_water,
            )?,
            LayoutScope::Subject(_) => match &baseline_subject {
                Some(baseline) => {
                    saved_slot(slot, kind, &baseline.tree, baseline.node_id_high_water)?
                }
                None => false,
            },
        };
        if !saved {
            contents.push(PublishedContentSnapshot {
                slot,
                revision: Revision::default(),
                view_revision: Revision::default(),
                current_page: 0,
                content: PublishedContentDescriptor::Empty,
            });
            continue;
        }
        let document = {
            let backend = backend
                .try_lock()
                .map_err(|_| invalid("内容正在更新，请稍后预览。"))?;
            if backend.global_revision()? != revision {
                return Err(invalid("预览内容已变化，请重新加载。"));
            }
            backend.read_profile_content(lease, slot)?
        };
        let snapshot = document
            .snapshot()
            .map_err(|_| invalid("已发布内容无效。"))?;
        if snapshot.slot != slot {
            return Err(invalid("预览内容槽位不匹配。"));
        }
        match (&snapshot.content, document.current) {
            (PublishedContentDescriptor::Text { .. }, Some(version))
                if kind == WidgetKind::SubjectText =>
            {
                let ContentPayload::Text(body) = version.payload else {
                    return Err(invalid("文字预览类型不匹配。"));
                };
                text.insert_verified(&snapshot, body)
                    .map_err(|_| invalid("文字预览缓存未通过实际哈希验证或已满。"))?;
            }
            (PublishedContentDescriptor::Image { .. }, _) if kind == WidgetKind::SubjectImage => (),
            (PublishedContentDescriptor::Empty, _) => (),
            _ => return Err(invalid("预览内容类型不匹配。")),
        }
        persisted.push(snapshot.clone());
        contents.push(snapshot);
    }
    if !current() {
        return Err(Failure::Stopped);
    }
    let requests = super::super::profiles_preview::image_requests_with_content(
        &identity,
        &public,
        subject.as_ref(),
        sequence,
        now,
        &contents,
        &text,
    )
    .map_err(|_| invalid("预览布局无法完成有界排版。"))?;
    let cache = super::super::profile_content_flow::raster_cache();
    let mut images = Vec::with_capacity(requests.len());
    if !requests.is_empty() {
        let files = ControlledContentFiles::new(&root).map_err(BackendError::ProfileContent)?;
        for request in requests {
            if !current() {
                return Err(Failure::Stopped);
            }
            images.push(
                files
                    .decode_current(
                        &cache,
                        &request.reference,
                        request.max_pixel_width,
                        request.max_pixel_height,
                        &current,
                    )
                    .map_err(BackendError::ProfileContent)?,
            );
        }
    }
    if !current() {
        return Err(Failure::Stopped);
    }
    {
        let backend = backend
            .try_lock()
            .map_err(|_| invalid("内容正在更新，请稍后预览。"))?;
        backend.management_identity(lease)?;
        if backend.global_revision()? != revision
            || backend.read_public_layout(lease)? != baseline_public
        {
            return Err(invalid("预览期间布局已变化，请重新加载。"));
        }
        if let Some(subject) = &subject {
            if backend.read_subject_profile(lease, subject.key)? != baseline_subject {
                return Err(invalid("预览期间科目配置已变化，请重新加载。"));
            }
        }
        for expected in &persisted {
            let observed = backend
                .read_profile_content(lease, expected.slot)?
                .snapshot()
                .map_err(|_| invalid("已发布内容无效。"))?;
            if observed != *expected {
                return Err(invalid("预览期间内容或页码已变化，请重新加载。"));
            }
        }
        if backend.global_revision()? != revision {
            return Err(invalid("预览内容已变化，请重新加载。"));
        }
    }
    if !current() {
        return Err(Failure::Stopped);
    }
    Ok(Preview {
        identity,
        public,
        subject,
        sequence,
        now,
        source_revision: revision,
        contents,
        text,
        images,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::layout::{default_public_layout, LayoutNode};
    #[test]
    fn only_fresh_ids_become_empty_never_denied_or_retired_saved_slots() {
        let mut baseline = default_public_layout();
        let id = LayoutNodeId::new(40).unwrap();
        baseline.nodes.push(LayoutNode {
            id,
            kind: NodeKind::Widget(WidgetKind::SubjectText),
            children: vec![],
        });
        let slot = ContentSlotKey {
            scope: LayoutScope::Public,
            node_id: id,
        };
        assert!(saved_slot(slot, WidgetKind::SubjectText, &baseline, 41).unwrap());
        assert!(saved_slot(slot, WidgetKind::SubjectImage, &baseline, 41).is_err());
        assert!(saved_slot(
            ContentSlotKey {
                node_id: LayoutNodeId::new(41).unwrap(),
                ..slot
            },
            WidgetKind::SubjectText,
            &baseline,
            41
        )
        .is_err());
        assert!(!saved_slot(
            ContentSlotKey {
                node_id: LayoutNodeId::new(42).unwrap(),
                ..slot
            },
            WidgetKind::SubjectText,
            &baseline,
            41
        )
        .unwrap());
        let wrong_scope = ContentSlotKey {
            scope: LayoutScope::Subject(SubjectProfileKey {
                class_id: ClassId::new(1).unwrap(),
                subject_id: SubjectId::new(1).unwrap(),
                teacher_id: PrincipalId::new(1).unwrap(),
            }),
            ..slot
        };
        assert!(saved_slot(wrong_scope, WidgetKind::SubjectText, &baseline, 41).is_err());
    }
    #[test]
    fn active_preview_slots_replace_public_fallback_without_loading_its_content() {
        let mut baseline = default_public_layout();
        let id = LayoutNodeId::new(40).unwrap();
        baseline.nodes.push(LayoutNode {
            id,
            kind: NodeKind::Widget(WidgetKind::SubjectImage),
            children: vec![],
        });
        baseline
            .nodes
            .iter_mut()
            .find(|node| matches!(node.kind, NodeKind::Outlet { .. }))
            .unwrap()
            .children
            .push(id);
        assert_eq!(
            active_slots(&baseline, false),
            vec![(
                ContentSlotKey {
                    scope: LayoutScope::Public,
                    node_id: id
                },
                WidgetKind::SubjectImage
            )]
        );
        assert!(active_slots(&baseline, true).is_empty());
    }
}
