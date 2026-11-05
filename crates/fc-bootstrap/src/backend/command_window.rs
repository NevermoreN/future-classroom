//! Issuance and immutable command scopes run on the existing shared worker.
//! Neither a ticket nor this scope replaces the original task's actor/CAS,
//! deadline, local flow capability, or physical-owner checks.
use super::*;
use fc_ports::command_window::{
    CommandFamily, CommandOwner, CommandTicket, CommandWindowError, IssuedCommand,
    COMMAND_WINDOW_MILLIS,
};

pub(super) struct ApplicationWindowClock {
    pub(super) wall: Arc<dyn Clock + Send + Sync>,
    pub(super) monotonic: Arc<dyn MonotonicClock + Send + Sync>,
}
impl fc_storage::command_window::WindowClock for ApplicationWindowClock {
    fn wall_millis(&self) -> std::result::Result<u64, StorageError> {
        u64::try_from(fc_domain::voice::civil_seconds(self.wall.now()))
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000))
            .ok_or(StorageError::CommandWindowClock)
    }
    fn monotonic_millis(&self) -> u64 {
        self.monotonic.monotonic_millis()
    }
}

pub struct BackendCommand<'a> {
    backend: &'a mut Backend,
    previous_repository: Option<Repository>,
    previous_command: Option<IssuedCommand>,
}
impl std::ops::Deref for BackendCommand<'_> {
    type Target = Backend;
    fn deref(&self) -> &Backend {
        self.backend
    }
}
impl std::ops::DerefMut for BackendCommand<'_> {
    fn deref_mut(&mut self) -> &mut Backend {
        self.backend
    }
}
impl Drop for BackendCommand<'_> {
    fn drop(&mut self) {
        if let Some(previous) = self.previous_repository.take() {
            self.backend.repository = previous;
        }
        self.backend.current_command = self.previous_command.take();
    }
}
impl BackendError {
    pub fn command_window_error(&self) -> Option<CommandWindowError> {
        use BackendError as B;
        match self {
            B::Storage(error) => error.command_window_error(),
            B::Content(RepositoryError::CommandWindow(e))
            | B::Identity(IdentityRepositoryError::CommandWindow(e))
            | B::Schedule(ScheduleRepositoryError::CommandWindow(e))
            | B::Display(DisplayRepositoryError::CommandWindow(e))
            | B::Lesson(LessonRepositoryError::CommandWindow(e))
            | B::Material(MaterialRepositoryError::CommandWindow(e))
            | B::Calendar(fc_ports::CalendarRepositoryError::CommandWindow(e))
            | B::Autostart(fc_ports::autostart::AutostartRepositoryError::CommandWindow(e))
            | B::HttpsSettings(fc_ports::HttpsSettingsRepositoryError::CommandWindow(e))
            | B::SubjectVisual(
                fc_ports::subject_visual::SubjectVisualRepositoryError::CommandWindow(e),
            )
            | B::Profile(fc_ports::ProfileRepositoryError::CommandWindow(e))
            | B::ProfileContent(fc_ports::ProfileContentRepositoryError::CommandWindow(e))
            | B::Voice(fc_ports::voice::VoiceRepositoryError::CommandWindow(e)) => Some(*e),
            _ => None,
        }
    }
}
impl Backend {
    pub(super) fn issue_for_owner(
        &self,
        family: CommandFamily,
        owner: CommandOwner,
    ) -> Result<IssuedCommand> {
        for _ in 0..4 {
            let mut entropy = [0; 24];
            self.crypto_random(&mut entropy)?;
            if entropy[..16] == [0; 16] || entropy[16..] == [0; 8] {
                continue;
            }
            let result = self
                .handle
                .try_issue_command(fc_storage::command_window::IssueCommand {
                    family,
                    owner,
                    entropy,
                })?
                .wait();
            match result {
                Err(StorageError::CommandIdReused) => continue,
                other => return other.map_err(BackendError::from),
            }
        }
        Err(BackendError::CryptoUnavailable)
    }
    fn web_command_owner(&self, lease: SessionLease) -> Result<CommandOwner> {
        if lease.origin() != TrustedOrigin::Web {
            return Err(BackendError::InvalidInput);
        }
        let state = self.state()?;
        let (actor, _) = self.actor(&state, lease, SessionPurpose::Manage)?;
        if state
            .account(actor.principal_id)
            .map_err(|e| BackendError::Identity(IdentityRepositoryError::Domain(e)))?
            .principal
            .role
            == Role::User
        {
            return Err(BackendError::Identity(IdentityRepositoryError::Domain(
                IdentityError::Permission(PermissionError::RoleDenied),
            )));
        }
        Ok(CommandOwner::authenticated(&actor))
    }
    pub fn issue_web_command(
        &self,
        lease: SessionLease,
        family: CommandFamily,
    ) -> Result<IssuedCommand> {
        if !matches!(
            family,
            CommandFamily::Content
                | CommandFamily::Materials
                | CommandFamily::Voice
                | CommandFamily::Updates
        ) {
            return Err(BackendError::InvalidInput);
        }
        let owner = self.web_command_owner(lease)?;
        if family == CommandFamily::Updates && self.role(lease)? != Role::GlobalAdmin {
            return Err(BackendError::InvalidCredentials);
        }
        let issued = self.issue_for_owner(family, owner)?;
        if self.web_command_owner(lease)? != owner {
            return Err(BackendError::Storage(StorageError::CommandNotIssued));
        }
        Ok(issued)
    }
    pub fn resolve_web_command(
        &self,
        lease: SessionLease,
        family: CommandFamily,
        ticket: CommandTicket,
        id: CommandId,
    ) -> Result<IssuedCommand> {
        if !matches!(
            family,
            CommandFamily::Content
                | CommandFamily::Materials
                | CommandFamily::Voice
                | CommandFamily::Updates
        ) {
            return Err(BackendError::InvalidInput);
        }
        if family == CommandFamily::Updates && self.role(lease)? != Role::GlobalAdmin {
            return Err(BackendError::InvalidCredentials);
        }
        let issued = IssuedCommand {
            ticket,
            command_id: id,
            family,
            owner: self.web_command_owner(lease)?,
            window_millis: COMMAND_WINDOW_MILLIS,
        };
        self.handle.try_check_command(issued)?.wait()?;
        Ok(issued)
    }
    /// Call only on a Work/application worker. No UI or HTTP event loop waits
    /// for this Writer request. Public classroom commands intentionally have no
    /// account lease; their original DesktopLessonGuard is still mandatory.
    pub fn issue_native_command(
        &self,
        family: CommandFamily,
        lease: Option<SessionLease>,
        purpose: SessionPurpose,
    ) -> Result<IssuedCommand> {
        let state = self.state()?;
        let owner = match (family, lease) {
            (CommandFamily::DesktopLesson, None) => CommandOwner::PublicDesktop,
            (CommandFamily::Identity, None) if state.installation.is_none() => {
                CommandOwner::PublicDesktop
            }
            (_, Some(lease)) if lease.origin() == TrustedOrigin::Local => {
                CommandOwner::authenticated(&self.actor(&state, lease, purpose)?.0)
            }
            _ => return Err(BackendError::InvalidInput),
        };
        self.issue_for_owner(family, owner)
    }
    pub fn begin_command_scope(&mut self, issued: IssuedCommand) -> Result<BackendCommand<'_>> {
        if self
            .current_command
            .is_some_and(|current| current != issued)
        {
            return Err(BackendError::Storage(StorageError::CommandIdReused));
        }
        self.handle.try_check_command(issued)?.wait()?;
        let repository = self.repository.with_command_window(issued);
        let previous_repository = Some(std::mem::replace(&mut self.repository, repository));
        let previous_command = self.current_command.replace(issued);
        Ok(BackendCommand {
            backend: self,
            previous_repository,
            previous_command,
        })
    }
    pub fn issued_command_id(&self) -> Result<CommandId> {
        self.current_command
            .map(|command| command.command_id)
            .ok_or(BackendError::Storage(StorageError::CommandNotIssued))
    }
}

#[cfg(test)]
impl Backend {
    /// Fixture composition follows the production issuer/scope boundary. It
    /// never supplies a raw id, bypasses a Writer check, or changes permissions.
    pub(crate) fn test_native_command<T>(
        &mut self,
        family: fc_ports::command_window::CommandFamily,
        lease: Option<SessionLease>,
        purpose: SessionPurpose,
        action: impl FnOnce(&mut Backend) -> Result<T>,
    ) -> Result<T> {
        let issued = self.issue_native_command(family, lease, purpose)?;
        let mut scoped = self.begin_command_scope(issued)?;
        action(&mut scoped)
    }
}
