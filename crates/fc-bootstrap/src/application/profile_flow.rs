//! Owned, bounded profile work. Database waits stay on the existing shared pool.
use super::*;
use fc_bootstrap::profile_content_cache::TextFrame;
use fc_domain::profile::{SubjectProfileDocument, SubjectProfileKey};
use fc_domain::{profile_content::PublishedContentSnapshot, time::LocalDateTime};
use fc_ports::{ProfilePage, PublicLayoutState};
use fc_windows::materials::raster::RasterFrame;
#[path = "profiles_preview_flow.rs"]
mod preview_flow;

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
    pub now: LocalDateTime,
    pub source_revision: Revision,
    pub contents: Vec<PublishedContentSnapshot>,
    pub text: TextFrame,
    /// Physical raster reservations stay owned until the UI render returns.
    pub images: Vec<Arc<RasterFrame>>,
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
        C::Stay | C::Close | C::ManageContent { .. } => return None,
    };
    bytes.checked_add(2048)
}
pub(super) fn run(
    backend: Arc<Mutex<Backend>>,
    lease: SessionLease,
    operation: profiles_editor::Command,
    sequence: u64,
    root: PathBuf,
    context: &JobContext,
) -> Result<Loaded> {
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    let operation = match operation {
        profiles_editor::Command::Preview { public, subject } => {
            return preview_flow::prepare(backend, lease, public, subject, sequence, root, context)
                .map(|preview| Loaded::Preview(Box::new(preview)))
        }
        operation => operation,
    };
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
        C::Preview { .. } | C::Stay | C::Close | C::ManageContent { .. } => {
            return Err(invalid("当前页面没有后台布局操作。"))
        }
    };
    // Explicit lock/WTS can advance independently of this mutex. Do not return
    // directory or preview data under a revoked local lease.
    backend.management_identity(lease)?;
    context.checkpoint().map_err(|_| Failure::Stopped)?;
    Ok(result)
}
