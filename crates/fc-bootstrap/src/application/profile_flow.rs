//! Owned, bounded profile work. Database waits stay on the existing shared pool.
use super::*;
use fc_domain::profile::{SubjectProfileDocument, SubjectProfileKey};
use fc_ports::{ProfilePage, PublicLayoutState};

pub(super) enum Loaded {
    Directory {
        identity: IdentityState,
        page: ProfilePage,
    },
    Public {
        state: PublicLayoutState,
        saved: bool,
    },
    Subject {
        key: SubjectProfileKey,
        document: Option<SubjectProfileDocument>,
        saved: bool,
    },
    Preview(Box<Preview>),
}
pub(super) struct Preview {
    pub identity: IdentityState,
    pub public: PublicLayoutState,
    pub subject: Option<SubjectProfileDocument>,
    pub sequence: u64,
}
pub(super) fn allocation(operation: &profiles_editor::Command) -> Option<usize> {
    use profiles_editor::Command as C;
    let public_bytes = |p: &PublicLayoutState| {
        p.layout
            .allocation_bytes()?
            .checked_add(p.display.allocation_bytes()?)
    };
    let bytes = match operation {
        C::SavePublic {
            layout, display, ..
        } => layout
            .allocation_bytes()?
            .checked_add(display.allocation_bytes()?)?,
        C::SaveSubject { candidate, .. } => candidate.allocation_bytes()?,
        C::Preview { public, subject } => {
            public.as_ref().map_or(Some(0), public_bytes)?.checked_add(
                subject
                    .as_ref()
                    .map_or(Some(0), SubjectProfileDocument::allocation_bytes)?,
            )?
        }
        C::ReloadList { .. } | C::LoadPublic | C::LoadSubject { .. } => 0,
        C::Stay | C::Close => return None,
    };
    bytes.checked_add(2048)
}
pub(super) fn run(
    backend: Arc<Mutex<Backend>>,
    lease: SessionLease,
    operation: profiles_editor::Command,
    sequence: u64,
    context: &JobContext,
) -> Result<Loaded> {
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    let mut backend = backend.lock().map_err(|_| Failure::Stopped)?;
    let identity = backend.management_identity(lease)?;
    use profiles_editor::Command as C;
    let result = match operation {
        C::ReloadList { after } => Loaded::Directory {
            identity,
            page: backend.list_subject_profiles(lease, after)?,
        },
        C::LoadPublic => Loaded::Public {
            state: backend.read_public_layout(lease)?,
            saved: false,
        },
        C::LoadSubject { key } => Loaded::Subject {
            key,
            document: backend.read_subject_profile(lease, key)?,
            saved: false,
        },
        C::SavePublic {
            expected_layout_revision,
            expected_display_revision,
            layout,
            display,
        } => Loaded::Public {
            state: backend.save_public_layout(
                lease,
                expected_layout_revision,
                expected_display_revision,
                layout,
                display,
            )?,
            saved: true,
        },
        C::SaveSubject {
            key,
            expected_revision,
            candidate,
        } => Loaded::Subject {
            key,
            document: Some(backend.save_subject_profile(
                lease,
                key,
                expected_revision,
                candidate,
            )?),
            saved: true,
        },
        C::Preview { public, subject } => {
            let public = match public {
                Some(p) => p,
                None => backend.read_public_layout(lease)?,
            };
            public
                .display
                .validate()
                .map_err(|_| invalid("预览显示设置无效。"))?;
            public
                .layout
                .tree
                .validate_composition(subject.as_ref().map(|s| &s.tree))
                .map_err(|_| {
                    invalid("草稿布局无法与当前公共内容区组合，请先调整节点、层级和尺寸约束。")
                })?;
            if let Some(subject) = &subject {
                subject
                    .validate_structure()
                    .map_err(|_| invalid("科目预览布局无效。"))?;
            }
            Loaded::Preview(Box::new(Preview {
                identity,
                public,
                subject,
                sequence,
            }))
        }
        C::Stay | C::Close => return Err(invalid("当前页面没有后台布局操作。")),
    };
    // Explicit lock/WTS can advance independently of this mutex. Do not return
    // directory or preview data under a revoked local lease.
    backend.management_identity(lease)?;
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    Ok(result)
}
