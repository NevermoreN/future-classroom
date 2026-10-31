//! School-owned opaque subject colours and their independent catalog version.
use crate::{
    auth::{ActorContext, PermissionError, TrustedOrigin},
    identity::{IdentityError, IdentityState, Rgb, SchoolSubject, MAX_SUBJECTS},
    ids::{Revision, SubjectId},
    time::LocalDateTime,
};

/// A complete, canonical version vector. The durable revisions remain owned by
/// SchoolSubject; unrelated identity or document revisions are deliberately absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRevision(Vec<(SubjectId, Revision)>);
impl PaletteRevision {
    pub fn new(entries: Vec<(SubjectId, Revision)>) -> Result<Self, SubjectVisualError> {
        if entries.len() > MAX_SUBJECTS
            || entries.iter().any(|(_, revision)| revision.get() == 0)
            || entries.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err(SubjectVisualError::InvalidPalette);
        }
        Ok(Self(entries))
    }
    pub fn from_subjects(subjects: &[SchoolSubject]) -> Result<Self, SubjectVisualError> {
        if subjects.len() > MAX_SUBJECTS {
            return Err(SubjectVisualError::InvalidPalette);
        }
        let mut entries: Vec<_> = subjects.iter().map(|s| (s.id, s.revision)).collect();
        entries.sort_unstable_by_key(|entry| entry.0);
        Self::new(entries)
    }
    pub fn allocation_bytes(&self) -> usize {
        self.0
            .capacity()
            .saturating_mul(std::mem::size_of::<(SubjectId, Revision)>())
    }
    pub fn entries(&self) -> &[(SubjectId, Revision)] {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubjectColor {
    pub subject_id: SubjectId,
    pub color: Rgb,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectVisualPalette {
    pub revision: PaletteRevision,
    pub subjects: Vec<SchoolSubject>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubjectVisualError {
    Identity(IdentityError),
    RevisionConflict { current: PaletteRevision },
    InvalidPalette,
    VersionOverflow,
}
impl SubjectVisualPalette {
    pub fn from_identity(identity: &IdentityState) -> Result<Self, SubjectVisualError> {
        identity.validate().map_err(SubjectVisualError::Identity)?;
        let revision = PaletteRevision::from_subjects(&identity.subjects)?;
        let mut subjects = identity.subjects.clone();
        subjects.sort_unstable_by_key(|subject| subject.id);
        Ok(Self { revision, subjects })
    }
    /// Changes only colours. Names, enablement and teaching assignments remain
    /// the writer's current state, even if unrelated identity changes intervened.
    pub fn transition(
        &self,
        colors: &[SubjectColor],
        expected: &PaletteRevision,
        identity: &IdentityState,
        actor: &ActorContext,
        now: LocalDateTime,
    ) -> Result<IdentityState, SubjectVisualError> {
        if actor.origin != TrustedOrigin::Local {
            return Err(SubjectVisualError::Identity(IdentityError::Permission(
                PermissionError::LocalOnly,
            )));
        }
        identity
            .authorize_management(actor, now)
            .map_err(SubjectVisualError::Identity)?;
        let current = Self::from_identity(identity)?;
        if current.revision != *expected {
            return Err(SubjectVisualError::RevisionConflict {
                current: current.revision,
            });
        }
        if *self != current || colors.len() != current.subjects.len() {
            return Err(SubjectVisualError::InvalidPalette);
        }
        let mut colors = colors.to_vec();
        colors.sort_unstable_by_key(|color| color.subject_id);
        if colors
            .iter()
            .zip(&current.subjects)
            .any(|(color, subject)| color.subject_id != subject.id)
        {
            return Err(SubjectVisualError::InvalidPalette);
        }
        let mut next = identity.clone();
        let mut changed = false;
        for color in colors {
            let subject = next
                .subjects
                .iter_mut()
                .find(|subject| subject.id == color.subject_id)
                .ok_or(SubjectVisualError::InvalidPalette)?;
            if subject.color != color.color {
                subject.color = color.color;
                subject.revision = subject
                    .revision
                    .next()
                    .map_err(|_| SubjectVisualError::VersionOverflow)?;
                changed = true;
            }
        }
        if changed {
            next.revision = next
                .revision
                .next()
                .map_err(|_| SubjectVisualError::VersionOverflow)?;
        }
        next.validate().map_err(SubjectVisualError::Identity)?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests;
