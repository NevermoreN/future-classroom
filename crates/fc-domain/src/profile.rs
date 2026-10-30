//! Independently versioned public layout and exact teacher/subject/class profile.
//! Historical inactive references remain owned data, never current authorization.
use crate::{
    auth::ActorContext,
    desktop::{ComponentKind, ComponentSettings, DisplaySettings},
    identity::{IdentityError, IdentityState},
    ids::*,
    layout::*,
    materials::TeacherMaterialFolder,
    time::LocalDateTime,
};

pub const MAX_SUBJECT_PROFILES: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SubjectProfileKey {
    pub class_id: ClassId,
    pub subject_id: SubjectId,
    pub teacher_id: PrincipalId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialsDisplaySettings {
    pub component: ComponentSettings,
    pub visible_rows: u8,
    pub row_gap_dip: u16,
}
impl Default for MaterialsDisplaySettings {
    fn default() -> Self {
        Self {
            component: DisplaySettings::default()
                .component(ComponentKind::Materials)
                .expect("default materials style exists")
                .clone(),
            visible_rows: 5,
            row_gap_dip: 8,
        }
    }
}
impl MaterialsDisplaySettings {
    pub fn validate(&self) -> Result<(), ProfileError> {
        let c = &self.component;
        if c.kind != ComponentKind::Materials
            || c.opacity_percent > 100
            || !(10..=72).contains(&c.font_dip)
            || !(120..=4096).contains(&c.min_width)
            || c.min_width > c.max_width
            || c.max_width > 4096
            || !(32..=2160).contains(&c.min_height)
            || c.min_height > c.max_height
            || c.max_height > 2160
            || !(1..=5).contains(&self.visible_rows)
            || self.row_gap_dip > 64
        {
            return Err(ProfileError::InvalidSettings);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutDocument {
    pub revision: Revision,
    pub node_id_high_water: u64,
    pub tree: LayoutTree,
}
impl Default for LayoutDocument {
    fn default() -> Self {
        let tree = default_public_layout();
        Self {
            revision: Revision::default(),
            node_id_high_water: tree.maximum_id(),
            tree,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectProfileDocument {
    pub id: ProfileId,
    pub key: SubjectProfileKey,
    pub enabled: bool,
    pub revision: Revision,
    pub layout_revision: Revision,
    pub node_id_high_water: u64,
    pub tree: LayoutTree,
    pub folder_id: Option<FolderId>,
    pub materials: MaterialsDisplaySettings,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    Identity(IdentityError),
    Layout(LayoutError),
    RevisionConflict { current: Revision },
    InvalidScope,
    FolderMismatch,
    InvalidSettings,
    IdMismatch,
    Capacity,
    VersionOverflow,
    InvalidSnapshot,
}
impl From<LayoutError> for ProfileError {
    fn from(e: LayoutError) -> Self {
        Self::Layout(e)
    }
}
fn authorize(
    identity: &IdentityState,
    actor: &ActorContext,
    now: LocalDateTime,
) -> Result<(), ProfileError> {
    identity.validate().map_err(ProfileError::Identity)?;
    identity
        .authorize_management(actor, now)
        .map_err(ProfileError::Identity)
}
fn canonical_water(
    old_tree: Option<&LayoutTree>,
    old_water: u64,
    candidate: &LayoutTree,
) -> Result<u64, ProfileError> {
    let old_max = old_tree.map_or(0, LayoutTree::maximum_id);
    let floor = old_water.max(old_max);
    if let Some(old) = old_tree {
        for n in &candidate.nodes {
            if n.id.get() <= floor && old.node(n.id).is_none() {
                return Err(LayoutError::RetiredNodeId.into());
            }
        }
    }
    Ok(floor.max(candidate.maximum_id()))
}
fn allocate(tree: &LayoutTree, water: &mut u64) -> Result<LayoutNodeId, ProfileError> {
    let value = (*water)
        .max(tree.maximum_id())
        .checked_add(1)
        .ok_or(ProfileError::VersionOverflow)?;
    let id = LayoutNodeId::new(value).map_err(|_| ProfileError::VersionOverflow)?;
    *water = value;
    Ok(id)
}
impl LayoutDocument {
    pub fn validate_structure(&self) -> Result<(), ProfileError> {
        if self.tree.scope != LayoutScope::Public {
            return Err(ProfileError::InvalidScope);
        }
        self.tree.validate_structure()?;
        if self.node_id_high_water < self.tree.maximum_id() {
            return Err(ProfileError::Layout(LayoutError::RetiredNodeId));
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), ProfileError> {
        self.validate_structure()
    }
    pub fn transition(
        &self,
        mut candidate: Self,
        expected: Revision,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<Self, ProfileError> {
        authorize(identity, actor, now)?;
        self.validate_structure()?;
        if expected != self.revision {
            return Err(ProfileError::RevisionConflict {
                current: self.revision,
            });
        }
        candidate.node_id_high_water =
            canonical_water(Some(&self.tree), self.node_id_high_water, &candidate.tree)?;
        candidate.validate_structure()?;
        candidate.revision = if candidate.tree == self.tree && self.revision.get() != 0 {
            self.revision
        } else {
            self.revision
                .next()
                .map_err(|_| ProfileError::VersionOverflow)?
        };
        Ok(candidate)
    }
    pub fn allocate_id(&mut self) -> Result<LayoutNodeId, ProfileError> {
        allocate(&self.tree, &mut self.node_id_high_water)
    }
    pub fn update_id_high_water(&mut self) {
        self.node_id_high_water = self.node_id_high_water.max(self.tree.maximum_id());
    }
    pub fn reset_tree(&mut self) -> Result<(), ProfileError> {
        let tree = default_public_layout()
            .remap_fresh_ids(self.node_id_high_water.max(self.tree.maximum_id()))?;
        self.node_id_high_water = tree.maximum_id();
        self.tree = tree;
        Ok(())
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.tree.allocation_bytes()
    }
}
impl SubjectProfileDocument {
    pub fn new(id: ProfileId, key: SubjectProfileKey) -> Self {
        let tree = default_subject_layout(key);
        Self {
            id,
            key,
            enabled: true,
            revision: Revision::default(),
            layout_revision: Revision::default(),
            node_id_high_water: tree.maximum_id(),
            tree,
            folder_id: None,
            materials: MaterialsDisplaySettings::default(),
        }
    }
    pub fn validate_structure(&self) -> Result<(), ProfileError> {
        if self.tree.scope != LayoutScope::Subject(self.key) {
            return Err(ProfileError::InvalidScope);
        }
        self.tree.validate_structure()?;
        if self.node_id_high_water < self.tree.maximum_id() {
            return Err(ProfileError::Layout(LayoutError::RetiredNodeId));
        }
        self.materials.validate()
    }
    /// Called only for a current candidate/save or selected classroom profile.
    /// Do not use this to reject the entire DB when a teacher was later revoked.
    pub fn validate_references(
        &self,
        identity: &IdentityState,
        folder: Option<&TeacherMaterialFolder>,
    ) -> Result<(), ProfileError> {
        self.validate_structure()?;
        identity.validate().map_err(ProfileError::Identity)?;
        if identity.account(self.key.teacher_id).is_err()
            || !identity.classes.iter().any(|c| c.id == self.key.class_id)
            || !identity
                .subjects
                .iter()
                .any(|s| s.id == self.key.subject_id)
        {
            return Err(ProfileError::InvalidScope);
        }
        if folder.is_some_and(|f| {
            f.scope.teacher_id != self.key.teacher_id || f.scope.subject_id != self.key.subject_id
        }) {
            return Err(ProfileError::FolderMismatch);
        }
        if let Some(id) = self.folder_id {
            let f = folder.ok_or(ProfileError::FolderMismatch)?;
            if f.id != id
                || f.scope.teacher_id != self.key.teacher_id
                || f.scope.subject_id != self.key.subject_id
            {
                return Err(ProfileError::FolderMismatch);
            }
        }
        Ok(())
    }
    pub fn validate_active(
        &self,
        identity: &IdentityState,
        folder: Option<&TeacherMaterialFolder>,
    ) -> Result<(), ProfileError> {
        self.validate_references(identity, folder)?;
        if !self.enabled
            || !identity.is_teacher(self.key.teacher_id, self.key.class_id, self.key.subject_id)
        {
            return Err(ProfileError::InvalidScope);
        }
        Ok(())
    }
    pub fn prepare_transition(
        current: Option<&Self>,
        mut candidate: Self,
        expected: Revision,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
        folder: Option<&TeacherMaterialFolder>,
    ) -> Result<Self, ProfileError> {
        authorize(identity, actor, now)?;
        let revision = current.map_or(Revision::default(), |d| d.revision);
        if expected != revision {
            return Err(ProfileError::RevisionConflict { current: revision });
        }
        if let Some(old) = current {
            old.validate_structure()?;
            if old.id != candidate.id || old.key != candidate.key {
                return Err(ProfileError::IdMismatch);
            }
        }
        candidate.node_id_high_water = canonical_water(
            current.map(|d| &d.tree),
            current.map_or(0, |d| d.node_id_high_water),
            &candidate.tree,
        )?;
        if candidate.enabled {
            candidate.validate_active(identity, folder)?;
        } else {
            candidate.validate_references(identity, folder)?;
        }
        let tree_changed = current.is_none_or(|old| old.tree != candidate.tree);
        let changed = current.is_none_or(|old| {
            old.tree != candidate.tree
                || old.enabled != candidate.enabled
                || old.folder_id != candidate.folder_id
                || old.materials != candidate.materials
        });
        candidate.layout_revision = if tree_changed {
            current
                .map_or(Revision::default(), |d| d.layout_revision)
                .next()
                .map_err(|_| ProfileError::VersionOverflow)?
        } else {
            current.map_or(Revision::default(), |d| d.layout_revision)
        };
        candidate.revision = if changed {
            revision.next().map_err(|_| ProfileError::VersionOverflow)?
        } else {
            revision
        };
        Ok(candidate)
    }
    pub fn allocate_id(&mut self) -> Result<LayoutNodeId, ProfileError> {
        allocate(&self.tree, &mut self.node_id_high_water)
    }
    pub fn update_id_high_water(&mut self) {
        self.node_id_high_water = self.node_id_high_water.max(self.tree.maximum_id());
    }
    pub fn reset_tree(&mut self) -> Result<(), ProfileError> {
        let tree = default_subject_layout(self.key)
            .remap_fresh_ids(self.node_id_high_water.max(self.tree.maximum_id()))?;
        self.node_id_high_water = tree.maximum_id();
        self.tree = tree;
        Ok(())
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.tree.allocation_bytes()
    }
}

/// The only profile data admitted to desktop facts: one public document and at
/// most the current exact subject profile. Never a full management catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileSelectionReason {
    Public,
    Selected,
    Missing,
    Disabled,
    Unassigned,
    InvalidAssignment,
    FolderMismatch,
    LayoutMismatch,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileSelectionSnapshot {
    pub public_layout: LayoutDocument,
    pub subject_profile: Option<SubjectProfileDocument>,
    pub reason: ProfileSelectionReason,
}
impl Default for ProfileSelectionSnapshot {
    fn default() -> Self {
        Self {
            public_layout: LayoutDocument::default(),
            subject_profile: None,
            reason: ProfileSelectionReason::Public,
        }
    }
}
impl ProfileSelectionSnapshot {
    pub fn validate(
        &self,
        identity: &IdentityState,
        context: &crate::lesson_context::LessonContextSnapshot,
        now: LocalDateTime,
        folder: Option<&TeacherMaterialFolder>,
    ) -> Result<(), ProfileError> {
        self.public_layout.validate_structure()?;
        if self.public_layout.revision.get() == 0 && self.public_layout != LayoutDocument::default()
        {
            return Err(ProfileError::InvalidSnapshot);
        }
        let current = context.current.as_ref();
        let Some(current) = current else {
            if self.subject_profile.is_some() || self.reason != ProfileSelectionReason::Public {
                return Err(ProfileError::InvalidSnapshot);
            }
            return Ok(());
        };
        if current.key.actual_date != now.date
            || current.key.class_id
                != identity
                    .installation
                    .as_ref()
                    .and_then(|i| i.current_class)
                    .ok_or(ProfileError::InvalidSnapshot)?
            || now < current.effective_from
            || now >= current.effective_until
        {
            return Err(ProfileError::InvalidSnapshot);
        }
        let Some(teacher_id) = current.teacher_id else {
            if self.subject_profile.is_some() || self.reason != ProfileSelectionReason::Unassigned {
                return Err(ProfileError::InvalidSnapshot);
            }
            return Ok(());
        };
        let key = SubjectProfileKey {
            class_id: current.key.class_id,
            subject_id: current.effective.subject_id,
            teacher_id,
        };
        if !identity.is_teacher(teacher_id, key.class_id, key.subject_id) {
            if self.subject_profile.is_some()
                || self.reason != ProfileSelectionReason::InvalidAssignment
            {
                return Err(ProfileError::InvalidSnapshot);
            }
            return Ok(());
        }
        match (&self.subject_profile, self.reason) {
            (Some(profile), ProfileSelectionReason::Selected) if profile.key == key => {
                if profile.revision.get() == 0 || profile.layout_revision.get() == 0 {
                    return Err(ProfileError::InvalidSnapshot);
                }
                profile.validate_active(identity, folder)?;
                self.public_layout
                    .tree
                    .validate_composition(Some(&profile.tree))?;
                Ok(())
            }
            (
                None,
                ProfileSelectionReason::Missing
                | ProfileSelectionReason::Disabled
                | ProfileSelectionReason::FolderMismatch
                | ProfileSelectionReason::LayoutMismatch,
            ) => Ok(()),
            _ => Err(ProfileError::InvalidSnapshot),
        }
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.public_layout.allocation_bytes()?.checked_add(
            self.subject_profile
                .as_ref()
                .map_or(Some(0), SubjectProfileDocument::allocation_bytes)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::*, identity::*, lesson_context::*, materials::MaterialScope, schedule::*, time::*,
    };
    fn nid(n: u64) -> LayoutNodeId {
        LayoutNodeId::new(n).unwrap()
    }
    fn pid(n: u64) -> PrincipalId {
        PrincipalId::new(n).unwrap()
    }
    fn key(teacher: u64) -> SubjectProfileKey {
        SubjectProfileKey {
            class_id: ClassId::new(1).unwrap(),
            subject_id: SubjectId::new(1).unwrap(),
            teacher_id: pid(teacher),
        }
    }
    fn now() -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 8 * 3600 + 30 * 60).unwrap()
    }
    fn identity() -> IdentityState {
        let school = SchoolId::new(1).unwrap();
        let accounts = (1..=3)
            .map(|n| UserAccount {
                principal: Principal {
                    id: pid(n),
                    school_id: school,
                    role: if n == 1 {
                        Role::GlobalAdmin
                    } else {
                        Role::TeacherAdmin
                    },
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("teacher{n}"),
                display_name: format!("教师{n}"),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            })
            .collect();
        let state = IdentityState {
            revision: Revision::new(5),
            installation: Some(Installation {
                school_id: school,
                school_name: "学校".into(),
                current_class: Some(key(2).class_id),
            }),
            accounts,
            classes: vec![SchoolClass {
                id: key(2).class_id,
                school_id: school,
                name: "一班".into(),
                enabled: true,
            }],
            subjects: vec![SchoolSubject {
                id: key(2).subject_id,
                school_id: school,
                name: "数学".into(),
                enabled: true,
                color: Rgb {
                    red: 30,
                    green: 60,
                    blue: 90,
                },
                revision: Revision::new(1),
            }],
            assignments: vec![2, 3]
                .into_iter()
                .map(|n| TeachingAssignment {
                    teacher_id: pid(n),
                    class_id: key(n).class_id,
                    subject_id: key(n).subject_id,
                    enabled: true,
                })
                .collect(),
            default_teachers: vec![],
        };
        state.validate().unwrap();
        state
    }
    fn actor(id: u64, origin: TrustedOrigin) -> ActorContext {
        ActorContext {
            principal_id: pid(id),
            school_id: SchoolId::new(1).unwrap(),
            auth_epoch: AuthEpoch::new(1),
            expires_at: LocalDateTime::new(now().date, 10 * 3600).unwrap(),
            origin,
        }
    }
    fn new_profile() -> SubjectProfileDocument {
        SubjectProfileDocument::new(ProfileId::new(7).unwrap(), key(2))
    }
    fn saved_profile() -> SubjectProfileDocument {
        SubjectProfileDocument::prepare_transition(
            None,
            new_profile(),
            Revision::default(),
            &identity(),
            &actor(1, TrustedOrigin::Local),
            now(),
            None,
        )
        .unwrap()
    }
    fn transparent(width: AxisSize, height: AxisSize) -> NodeKind {
        NodeKind::Container(ContainerSettings {
            flow: Flow::Column,
            padding: 0,
            gap: 0,
            width,
            height,
            group_opacity_percent: 100,
            material: None,
        })
    }
    fn fixed() -> AxisSize {
        AxisSize::Fixed(300)
    }
    fn context() -> LessonContextSnapshot {
        let date = now().date;
        let cell = CourseCell {
            subject_id: key(2).subject_id,
            teacher: TeacherBinding::Explicit(pid(2)),
        };
        let lesson = LessonInstanceKey {
            actual_date: date,
            class_id: key(2).class_id,
            range: MinuteRange::new(
                Minute::from_hm(8, 0).unwrap(),
                Minute::from_hm(9, 0).unwrap(),
            )
            .unwrap(),
            version: ResolvedScheduleVersion {
                actual_semester_id: SemesterId::new(1).unwrap(),
                actual_revision: Revision::new(1),
                source_semester_id: SemesterId::new(1).unwrap(),
                source_revision: Revision::new(1),
                calendar_mode: CalendarMode::Public,
                calendar_mode_revision: Revision::new(1),
                applied_calendar_revision: Revision::new(1),
                makeup_mapping_revision: Revision::new(1),
                source_table_id: TableId::new(1).unwrap(),
                source_row_id: RowId::new(1).unwrap(),
                rules_revision: Revision::new(1),
            },
        };
        LessonContextSnapshot {
            revision: Revision::new(1),
            identity_revision: Revision::new(5),
            observed_at: Some(now()),
            class_id: Some(key(2).class_id),
            current: Some(EffectiveLessonContext {
                key: lesson,
                base: cell.clone(),
                effective: cell,
                teacher_id: Some(pid(2)),
                effective_from: LocalDateTime::new(date, 8 * 3600).unwrap(),
                effective_until: LocalDateTime::new(date, 9 * 3600).unwrap(),
            }),
            ..LessonContextSnapshot::default()
        }
    }
    #[test]
    fn deterministic_defaults_are_legal_and_do_not_have_unknown_height_cycles() {
        let public = default_public_layout();
        let subject = default_subject_layout(key(2));
        public.validate().unwrap();
        subject.validate().unwrap();
        assert_eq!(public, default_public_layout());
        assert_eq!(public.metrics().unwrap().top_level_glass, 8);
        assert_eq!(public.metrics().unwrap().glass_depth, 2);
        public.validate_composition(Some(&subject)).unwrap();
        let mut wrong = subject.clone();
        if let NodeKind::Root { height, .. } = &mut wrong.nodes[0].kind {
            *height = AxisSize::Percent {
                basis_points: 10000,
                min: 0,
                max: 2160,
            };
        }
        assert_eq!(
            public.validate_composition(Some(&wrong)),
            Err(LayoutError::SizeCycle)
        );
        assert!(
            public.allocation_bytes().unwrap()
                >= public.nodes.capacity() * std::mem::size_of::<LayoutNode>()
        );
    }
    #[test]
    fn duplicate_missing_multiple_parent_cycle_unreachable_and_depth_are_rejected() {
        let source = default_subject_layout(key(2));
        let mut t = source.clone();
        t.nodes.push(t.nodes[1].clone());
        assert_eq!(t.validate(), Err(LayoutError::DuplicateId));
        let mut t = source.clone();
        t.nodes[0].children.push(nid(999));
        assert_eq!(t.validate(), Err(LayoutError::MissingNode));
        let mut t = source.clone();
        t.nodes[0].children.push(nid(2));
        assert_eq!(t.validate(), Err(LayoutError::MultipleParents));
        let mut t = source.clone();
        t.nodes.push(LayoutNode {
            id: nid(3),
            kind: transparent(fixed(), fixed()),
            children: vec![nid(4)],
        });
        t.nodes.push(LayoutNode {
            id: nid(4),
            kind: transparent(fixed(), fixed()),
            children: vec![nid(3)],
        });
        assert_eq!(t.validate(), Err(LayoutError::Cycle));
        let mut t = source.clone();
        t.nodes.push(LayoutNode {
            id: nid(3),
            kind: NodeKind::Widget(WidgetKind::Materials),
            children: vec![],
        });
        assert_eq!(t.validate(), Err(LayoutError::Unreachable));
        let mut t = source.clone();
        t.nodes[0].children = vec![nid(3)];
        for n in 3..=18 {
            t.nodes.push(LayoutNode {
                id: nid(n),
                kind: transparent(fixed(), fixed()),
                children: vec![if n == 18 { nid(2) } else { nid(n + 1) }],
            });
        }
        assert_eq!(t.validate(), Err(LayoutError::Capacity));
    }
    #[test]
    fn protected_scopes_unsupported_extensions_and_nested_actual_glass_are_rejected() {
        let mut t = default_subject_layout(key(2));
        t.nodes[1].kind = NodeKind::Widget(WidgetKind::Homework);
        assert_eq!(t.validate(), Err(LayoutError::ProtectedWidget));
        t.nodes[1].kind = NodeKind::Widget(WidgetKind::SubjectImage);
        assert_eq!(t.validate(), Err(LayoutError::UnsupportedWidget));
        let mut t = default_public_layout();
        let parent = t.nodes.iter_mut().find(|n| n.id == nid(2)).unwrap();
        if let NodeKind::Container(c) = &mut parent.kind {
            c.material = Some(GlassMaterial::default());
        }
        assert_eq!(t.validate(), Err(LayoutError::GlassDepth));
        let mut t = default_public_layout();
        let outlet = t
            .nodes
            .iter_mut()
            .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
            .unwrap();
        outlet.children.push(nid(8));
        t.nodes
            .iter_mut()
            .find(|n| n.id == nid(3))
            .unwrap()
            .children
            .retain(|n| *n != nid(8));
        assert_eq!(t.validate(), Err(LayoutError::ProtectedWidget));
    }
    #[test]
    fn auto_parent_percentage_child_is_rejected_but_fixed_boundary_breaks_dependency() {
        let mut t = default_subject_layout(key(2));
        t.nodes[0].children = vec![nid(3)];
        t.nodes.push(LayoutNode {
            id: nid(3),
            kind: transparent(
                AxisSize::Content {
                    min: 0,
                    preferred: 200,
                    max: 800,
                },
                AxisSize::Content {
                    min: 0,
                    preferred: 0,
                    max: 800,
                },
            ),
            children: vec![nid(4)],
        });
        t.nodes.push(LayoutNode {
            id: nid(4),
            kind: transparent(
                AxisSize::Percent {
                    basis_points: 5000,
                    min: 0,
                    max: 800,
                },
                fixed(),
            ),
            children: vec![nid(2)],
        });
        assert_eq!(t.validate(), Err(LayoutError::SizeCycle));
        if let NodeKind::Container(c) = &mut t.nodes[2].kind {
            c.width = fixed();
        }
        t.validate().unwrap();
        if let NodeKind::Container(c) = &mut t.nodes[3].kind {
            c.group_opacity_percent = 101;
        }
        assert_eq!(t.validate(), Err(LayoutError::InvalidConstraint));
    }
    #[test]
    fn composition_counts_selected_templates_not_fallback_or_inactive_recess() {
        let public = default_public_layout();
        let mut subject = default_subject_layout(key(2));
        subject.nodes[0].children.push(nid(3));
        subject.nodes.push(LayoutNode {
            id: nid(3),
            kind: NodeKind::Widget(WidgetKind::Materials),
            children: vec![],
        });
        assert_eq!(
            public
                .validate_composition(Some(&subject))
                .unwrap()
                .top_level_glass,
            8
        );
        subject.nodes[0].children.push(nid(4));
        subject.nodes.push(LayoutNode {
            id: nid(4),
            kind: NodeKind::Widget(WidgetKind::Materials),
            children: vec![],
        });
        assert_eq!(
            public.validate_composition(Some(&subject)),
            Err(LayoutError::TooManyTopLevel)
        );
    }
    #[test]
    fn composed_root_is_counted_and_joined_depth_is_bounded() {
        let public = default_public_layout();
        let mut subject = default_subject_layout(key(2));
        for n in 3..=118 {
            subject.nodes[0].children.push(nid(n));
            subject.nodes.push(LayoutNode {
                id: nid(n),
                kind: transparent(fixed(), fixed()),
                children: vec![],
            });
        }
        assert_eq!(
            public.validate_composition(Some(&subject)).unwrap().nodes,
            128
        );
        subject.nodes[0].children.push(nid(119));
        subject.nodes.push(LayoutNode {
            id: nid(119),
            kind: transparent(fixed(), fixed()),
            children: vec![],
        });
        subject.validate().unwrap();
        assert_eq!(
            public.validate_composition(Some(&subject)),
            Err(LayoutError::Capacity)
        );
        let mut subject = default_subject_layout(key(2));
        subject.nodes[0].children = vec![nid(3)];
        for n in 3..=16 {
            subject.nodes.push(LayoutNode {
                id: nid(n),
                kind: transparent(fixed(), fixed()),
                children: vec![if n == 16 { nid(2) } else { nid(n + 1) }],
            });
        }
        subject.validate().unwrap();
        assert_eq!(
            public.validate_composition(Some(&subject)),
            Err(LayoutError::Capacity)
        );
    }
    #[test]
    fn public_cas_first_commit_noop_permissions_and_untrusted_revision_are_exact() {
        let state = identity();
        let old = LayoutDocument::default();
        let mut candidate = old.clone();
        candidate.revision = Revision::new(999);
        candidate.node_id_high_water = u64::MAX;
        let saved = old
            .transition(
                candidate,
                Revision::default(),
                &state,
                &actor(1, TrustedOrigin::Local),
                now(),
            )
            .unwrap();
        assert_eq!(saved.revision, Revision::new(1));
        assert_eq!(saved.node_id_high_water, 12);
        assert_eq!(
            saved
                .transition(
                    saved.clone(),
                    saved.revision,
                    &state,
                    &actor(1, TrustedOrigin::Local),
                    now()
                )
                .unwrap(),
            saved
        );
        assert!(matches!(
            saved.transition(
                saved.clone(),
                Revision::default(),
                &state,
                &actor(1, TrustedOrigin::Local),
                now()
            ),
            Err(ProfileError::RevisionConflict { .. })
        ));
        assert!(old
            .transition(
                old.clone(),
                Revision::default(),
                &state,
                &actor(2, TrustedOrigin::Local),
                now()
            )
            .is_err());
        assert!(old
            .transition(
                old.clone(),
                Revision::default(),
                &state,
                &actor(1, TrustedOrigin::Web),
                now()
            )
            .is_err());
    }
    #[test]
    fn deleted_node_cannot_be_reused_after_save_reopen_and_reset_uses_fresh_ids() {
        let state = identity();
        let owner = actor(1, TrustedOrigin::Local);
        let first = LayoutDocument::default()
            .transition(
                LayoutDocument::default(),
                Revision::default(),
                &state,
                &owner,
                now(),
            )
            .unwrap();
        let mut candidate = first.clone();
        candidate.tree.nodes.retain(|n| n.id != nid(12));
        candidate
            .tree
            .nodes
            .iter_mut()
            .find(|n| n.id == nid(10))
            .unwrap()
            .children
            .clear();
        candidate.node_id_high_water = 0;
        let saved = first
            .transition(candidate, first.revision, &state, &owner, now())
            .unwrap();
        assert_eq!(saved.node_id_high_water, 12);
        let mut reopened = saved.clone();
        assert_eq!(reopened.allocate_id().unwrap(), nid(13));
        let mut reuse = saved.clone();
        reuse.tree.nodes.push(LayoutNode {
            id: nid(12),
            kind: NodeKind::Widget(WidgetKind::Materials),
            children: vec![],
        });
        reuse
            .tree
            .nodes
            .iter_mut()
            .find(|n| n.id == nid(10))
            .unwrap()
            .children
            .push(nid(12));
        assert_eq!(
            saved.transition(reuse, saved.revision, &state, &owner, now()),
            Err(ProfileError::Layout(LayoutError::RetiredNodeId))
        );
        let mut reset = saved.clone();
        reset.reset_tree().unwrap();
        assert!(reset.tree.nodes.iter().all(|n| n.id.get() > 12));
        saved
            .transition(reset, saved.revision, &state, &owner, now())
            .unwrap();
        let mut damaged = saved;
        damaged.node_id_high_water = 0;
        assert!(damaged.validate_structure().is_err());
        assert!(damaged
            .transition(
                LayoutDocument::default(),
                damaged.revision,
                &state,
                &owner,
                now()
            )
            .is_err());
    }
    #[test]
    fn remapping_and_allocator_overflow_never_partially_mutate() {
        let mut profile = saved_profile();
        profile.node_id_high_water = u64::MAX;
        let before = profile.clone();
        assert_eq!(profile.allocate_id(), Err(ProfileError::VersionOverflow));
        assert_eq!(profile, before);
        assert_eq!(
            profile.reset_tree(),
            Err(ProfileError::Layout(LayoutError::VersionOverflow))
        );
        assert_eq!(profile, before);
        assert_eq!(
            default_public_layout().remap_fresh_ids(u64::MAX - 1),
            Err(LayoutError::VersionOverflow)
        );
    }
    #[test]
    fn subject_versions_settings_and_folder_owner_are_independent_and_exact() {
        let state = identity();
        let owner = actor(1, TrustedOrigin::Local);
        let saved = saved_profile();
        assert_eq!(
            (saved.revision, saved.layout_revision),
            (Revision::new(1), Revision::new(1))
        );
        let mut candidate = saved.clone();
        candidate.materials.visible_rows = 2;
        candidate.revision = Revision::new(999);
        candidate.layout_revision = Revision::new(999);
        let next = SubjectProfileDocument::prepare_transition(
            Some(&saved),
            candidate,
            saved.revision,
            &state,
            &owner,
            now(),
            None,
        )
        .unwrap();
        assert_eq!(
            (next.revision, next.layout_revision),
            (Revision::new(2), Revision::new(1))
        );
        assert_eq!(
            SubjectProfileDocument::prepare_transition(
                Some(&next),
                next.clone(),
                next.revision,
                &state,
                &owner,
                now(),
                None
            )
            .unwrap(),
            next
        );
        let mut c = next.clone();
        c.folder_id = Some(FolderId::new(10).unwrap());
        let folder = TeacherMaterialFolder {
            id: FolderId::new(10).unwrap(),
            scope: MaterialScope {
                teacher_id: pid(3),
                subject_id: key(2).subject_id,
            },
            revision: Revision::new(1),
            last_added_sequence: 0,
        };
        assert_eq!(
            SubjectProfileDocument::prepare_transition(
                Some(&next),
                c,
                next.revision,
                &state,
                &owner,
                now(),
                Some(&folder)
            ),
            Err(ProfileError::FolderMismatch)
        );
        let mut c = next.clone();
        c.materials.component.kind = ComponentKind::Clock;
        assert_eq!(
            SubjectProfileDocument::prepare_transition(
                Some(&next),
                c,
                next.revision,
                &state,
                &owner,
                now(),
                None
            ),
            Err(ProfileError::InvalidSettings)
        );
        for count in [0, 6] {
            let mut c = next.clone();
            c.materials.visible_rows = count;
            assert!(c.validate_structure().is_err());
        }
    }
    #[test]
    fn revoked_profile_is_retained_and_can_be_disabled_without_reviving_authority() {
        let old = saved_profile();
        let mut state = identity();
        state.accounts[1].principal.role = Role::User;
        state.assignments.retain(|a| a.teacher_id != pid(2));
        state.validate().unwrap();
        old.validate_structure().unwrap();
        assert!(old.validate_active(&state, None).is_err());
        let mut candidate = old.clone();
        candidate.enabled = false;
        let disabled = SubjectProfileDocument::prepare_transition(
            Some(&old),
            candidate,
            old.revision,
            &state,
            &actor(1, TrustedOrigin::Local),
            now(),
            None,
        )
        .unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.id, old.id);
        assert_eq!(disabled.tree, old.tree);
        assert!(disabled.validate_active(&state, None).is_err());
        let mut enable = disabled.clone();
        enable.enabled = true;
        assert!(SubjectProfileDocument::prepare_transition(
            Some(&disabled),
            enable,
            disabled.revision,
            &state,
            &actor(1, TrustedOrigin::Local),
            now(),
            None
        )
        .is_err());
    }
    #[test]
    fn snapshot_matches_effective_teacher_and_break_never_inherits_private_visuals() {
        let state = identity();
        let context = context();
        context.validate().unwrap();
        let selected = ProfileSelectionSnapshot {
            public_layout: LayoutDocument::default(),
            subject_profile: Some(saved_profile()),
            reason: ProfileSelectionReason::Selected,
        };
        selected.validate(&state, &context, now(), None).unwrap();
        let mut switched = context.clone();
        switched.current.as_mut().unwrap().teacher_id = Some(pid(3));
        switched.current.as_mut().unwrap().effective.teacher = TeacherBinding::Explicit(pid(3));
        assert!(selected.validate(&state, &switched, now(), None).is_err());
        let mut public = selected.clone();
        public.subject_profile = None;
        public.reason = ProfileSelectionReason::Missing;
        public.validate(&state, &switched, now(), None).unwrap();
        public.reason = ProfileSelectionReason::LayoutMismatch;
        public.validate(&state, &switched, now(), None).unwrap();
        let break_context = LessonContextSnapshot::default();
        assert!(selected
            .validate(&state, &break_context, now(), None)
            .is_err());
        assert!(public
            .validate(&state, &break_context, now(), None)
            .is_err());
        ProfileSelectionSnapshot::default()
            .validate(&state, &break_context, now(), None)
            .unwrap();
        let mut unassigned = context;
        unassigned.current.as_mut().unwrap().teacher_id = None;
        unassigned.current.as_mut().unwrap().effective.teacher = TeacherBinding::Unassigned;
        let snapshot = ProfileSelectionSnapshot {
            reason: ProfileSelectionReason::Unassigned,
            ..ProfileSelectionSnapshot::default()
        };
        snapshot.validate(&state, &unassigned, now(), None).unwrap();
    }
}
