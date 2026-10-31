//! Bounded synthetic colour preview on the original shared worker domain.
use super::*;
use fc_domain::subject_visual::SubjectVisualPalette;
pub(super) struct PreviewJob {
    pub backend: Arc<Mutex<Backend>>,
    pub lease: SessionLease,
    pub palette: SubjectVisualPalette,
    pub settings: DisplaySettings,
    pub subject: SubjectId,
    pub sequence: u64,
}
impl PreviewJob {
    pub(super) fn allocation_bytes(&self) -> Option<usize> {
        self.palette.subjects.iter().try_fold(
            self.settings
                .allocation_bytes()?
                .checked_add(self.palette.revision.allocation_bytes())?
                .checked_add(
                    self.palette
                        .subjects
                        .capacity()
                        .checked_mul(std::mem::size_of::<SchoolSubject>())?,
                )?,
            |sum, s| sum.checked_add(s.name.capacity()),
        )
    }
    pub(super) fn run(self, context: &JobContext) -> Result<Outcome> {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        let (mut identity, source_revision) = {
            let mut backend = self.backend.lock().map_err(|_| Failure::Stopped)?;
            let scoped = backend.begin_execution_scope(Arc::new(context.execution_fence()));
            // Recheck this exact local management lease; do not renew or mint it.
            scoped.read_subject_visuals(self.lease)?;
            (scoped.state()?, scoped.global_revision()?)
        };
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        for subject in &mut identity.subjects {
            if let Some(draft) = self.palette.subjects.iter().find(|s| s.id == subject.id) {
                subject.color = draft.color;
            }
        }
        let now = CurrentClock::new()
            .try_now()
            .map_err(|_| invalid("系统时间不可用。"))?;
        let image = profiles_preview::render_subject_visuals(
            &identity,
            &self.settings,
            self.subject,
            self.sequence,
            now,
        )
        .map_err(|_| invalid("颜色示例无法完成有界绘制，请检查显示尺寸。"))?;
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        let backend = self.backend.lock().map_err(|_| Failure::Stopped)?;
        backend.read_subject_visuals(self.lease)?;
        if backend.global_revision()? != source_revision {
            return Err(invalid("预览期间学校资料已变化，请重新预览。"));
        }
        Ok(Outcome::DisplayPreview {
            sequence: self.sequence,
            source_revision,
            image,
        })
    }
}
