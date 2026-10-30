//! Local Global profile management. Only invoke on the fixed application workers.
use super::*;
use fc_domain::profile::{LayoutDocument, SubjectProfileDocument, SubjectProfileKey};
use fc_ports::{
    ProfilePage, ProfileRepository, ProfileRequest, PublicLayoutRequest, PublicLayoutState,
};
impl Backend {
    pub fn read_public_layout(&self, lease: SessionLease) -> Result<PublicLayoutState> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .public_layout(&actor)?)
    }
    pub fn save_public_layout(
        &mut self,
        lease: SessionLease,
        expected_layout_revision: Revision,
        expected_display_revision: Revision,
        layout: LayoutDocument,
        display: DisplaySettings,
    ) -> Result<PublicLayoutState> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let request = PublicLayoutRequest {
            command_id: self.command_id()?,
            expected_layout_revision,
            expected_display_revision,
            layout,
            display,
        };
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .execute_public_layout(request, &actor, self.clock.as_ref())?)
    }
    pub fn read_subject_profile(
        &self,
        lease: SessionLease,
        key: SubjectProfileKey,
    ) -> Result<Option<SubjectProfileDocument>> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .subject_profile(key, &actor)?)
    }
    pub fn save_subject_profile(
        &mut self,
        lease: SessionLease,
        key: SubjectProfileKey,
        expected_revision: Revision,
        candidate: SubjectProfileDocument,
    ) -> Result<SubjectProfileDocument> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        let request = ProfileRequest {
            command_id: self.command_id()?,
            key,
            expected_revision,
            candidate,
        };
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .execute_subject_profile(request, &actor, self.clock.as_ref())?)
    }
    pub fn list_subject_profiles(
        &self,
        lease: SessionLease,
        after: Option<ProfileId>,
    ) -> Result<ProfilePage> {
        let state = self.state()?;
        let (actor, generation) = self.actor(&state, lease, SessionPurpose::Manage)?;
        Ok(self
            .guarded(lease, generation, SessionPurpose::Manage)
            .list_subject_profiles(after, &actor)?)
    }
}

#[cfg(test)]
mod tests;
