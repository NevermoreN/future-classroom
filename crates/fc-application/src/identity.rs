//! Identity admission and process-local channel leases. The writer must revalidate the
//! captured lease before commit; reading an ActorContext never renews its idle timer.
use fc_domain::{auth::*, identity::*, ids::*, time::LocalDateTime};
use fc_ports::{Clock, IdentityRepository, IdentityRepositoryError, IdentityRequest};

pub fn execute_identity<R: IdentityRepository>(
    repository: &mut R,
    clock: &dyn Clock,
    actor: Option<&ActorContext>,
    request: IdentityRequest,
) -> Result<IdentityTransition, IdentityRepositoryError> {
    // Password material never enters this public command path.
    if matches!(
        request.command,
        IdentityCommand::FirstAdministrator { .. }
            | IdentityCommand::CreateAccount(_)
            | IdentityCommand::PasswordChanged { .. }
            | IdentityCommand::RecoveryConsumed { .. }
            | IdentityCommand::RecoveryRotated { .. }
    ) {
        return Err(IdentityRepositoryError::Domain(IdentityError::Permission(
            PermissionError::RoleDenied,
        )));
    }
    // The writer resolves a matching command replay before reapplying CAS. Admission
    // must not reject successful retries merely because state has since advanced.
    repository.execute_identity(request, actor, clock)
}

pub const LOCAL_IDLE_MILLIS: u64 = 300_000;
pub const MAX_LIVE_LEASES: usize = 1024;
pub const MAX_SESSION_MILLIS: u64 = 86_400_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionError {
    Expired,
    RateLimited { retry_after_millis: u64 },
    Revoked,
    PasswordChangeRequired,
    Capacity,
    InvalidDuration,
    ClockRegression,
    Overflow,
    Identity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPurpose {
    Manage,
    ChangeInitialPassword,
}
/// Not a bearer token: the authenticated transport privately maps its CSPRNG token to
/// this proof. No lease IDs are persisted, deserialized from requests, or reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionLease {
    id: u64,
    principal_id: PrincipalId,
    auth_epoch: AuthEpoch,
    origin: TrustedOrigin,
}
impl SessionLease {
    pub fn id(self) -> u64 {
        self.id
    }
    pub fn principal_id(self) -> PrincipalId {
        self.principal_id
    }
    pub fn origin(self) -> TrustedOrigin {
        self.origin
    }
}
#[derive(Clone, Debug)]
struct LiveSession {
    proof: SessionLease,
    issued: u64,
    last_input: u64,
    absolute_deadline: u64,
    wall_expires_at: LocalDateTime,
    purpose: SessionPurpose,
}
#[derive(Default)]
pub struct IdentitySessions {
    sequence: u64,
    sessions: Vec<LiveSession>,
}
impl IdentitySessions {
    /// Only after successful password verification against precisely this account version.
    /// Completing asynchronous verification must re-read epoch/credential version first.
    pub fn issue_verified(
        &mut self,
        account: &UserAccount,
        origin: TrustedOrigin,
        now_millis: u64,
        lifetime_millis: u64,
        wall_expires_at: LocalDateTime,
    ) -> Result<SessionLease, SessionError> {
        if !account.principal.enabled {
            return Err(SessionError::Revoked);
        }
        if lifetime_millis == 0 || lifetime_millis > MAX_SESSION_MILLIS {
            return Err(SessionError::InvalidDuration);
        }
        let absolute_deadline = now_millis
            .checked_add(lifetime_millis)
            .ok_or(SessionError::Overflow)?;
        let id = self.sequence.checked_add(1).ok_or(SessionError::Overflow)?;
        let replaces = self
            .sessions
            .iter()
            .any(|s| s.proof.principal_id == account.principal.id && s.proof.origin == origin);
        if !replaces && self.sessions.len() >= MAX_LIVE_LEASES {
            return Err(SessionError::Capacity);
        }
        self.sequence = id;
        self.sessions
            .retain(|s| s.proof.principal_id != account.principal.id || s.proof.origin != origin);
        let proof = SessionLease {
            id,
            principal_id: account.principal.id,
            auth_epoch: account.principal.auth_epoch,
            origin,
        };
        self.sessions.push(LiveSession {
            proof,
            issued: now_millis,
            last_input: now_millis,
            absolute_deadline,
            wall_expires_at,
            purpose: if account.must_change_password {
                SessionPurpose::ChangeInitialPassword
            } else {
                SessionPurpose::Manage
            },
        });
        Ok(proof)
    }
    fn valid<'a>(
        &'a self,
        state: &IdentityState,
        proof: SessionLease,
        now: LocalDateTime,
        tick: u64,
    ) -> Result<&'a LiveSession, SessionError> {
        let session = self
            .sessions
            .iter()
            .find(|s| s.proof == proof)
            .ok_or(SessionError::Revoked)?;
        if tick < session.issued || tick < session.last_input {
            return Err(SessionError::ClockRegression);
        }
        if tick >= session.absolute_deadline
            || now >= session.wall_expires_at
            || (proof.origin == TrustedOrigin::Local
                && tick - session.last_input >= LOCAL_IDLE_MILLIS)
        {
            return Err(SessionError::Expired);
        }
        let account = state
            .account(proof.principal_id)
            .map_err(|_| SessionError::Identity)?;
        if !account.principal.enabled || account.principal.auth_epoch != proof.auth_epoch {
            return Err(SessionError::Revoked);
        }
        Ok(session)
    }
    pub fn actor(
        &self,
        state: &IdentityState,
        proof: SessionLease,
        now: LocalDateTime,
        tick: u64,
        purpose: SessionPurpose,
    ) -> Result<ActorContext, SessionError> {
        let session = self.valid(state, proof, now, tick)?;
        if session.purpose == SessionPurpose::ChangeInitialPassword
            && purpose != SessionPurpose::ChangeInitialPassword
        {
            return Err(SessionError::PasswordChangeRequired);
        }
        let account = state
            .account(proof.principal_id)
            .map_err(|_| SessionError::Identity)?;
        Ok(ActorContext {
            principal_id: proof.principal_id,
            school_id: account.principal.school_id,
            auth_epoch: proof.auth_epoch,
            expires_at: session.wall_expires_at,
            origin: proof.origin,
        })
    }
    /// Trusted local input event only. Polling/snapshot reads/HTTP traffic cannot touch.
    pub fn local_input(
        &mut self,
        state: &IdentityState,
        proof: SessionLease,
        now: LocalDateTime,
        tick: u64,
    ) -> Result<(), SessionError> {
        self.valid(state, proof, now, tick)?;
        if proof.origin != TrustedOrigin::Local {
            return Err(SessionError::Revoked);
        }
        let session = self
            .sessions
            .iter_mut()
            .find(|s| s.proof == proof)
            .ok_or(SessionError::Revoked)?;
        session.last_input = tick;
        Ok(())
    }
    /// Lock screen/user switch/leave workbench revoke only local channels.
    pub fn lock_local(&mut self) {
        self.sessions
            .retain(|s| s.proof.origin != TrustedOrigin::Local);
    }
    /// Explicit account logout revokes all channels, provided the captured initiating
    /// lease is still current. An old queued logout cannot target a replacement lease.
    pub fn logout_account(
        &mut self,
        state: &IdentityState,
        proof: SessionLease,
        now: LocalDateTime,
        tick: u64,
    ) -> Result<(), SessionError> {
        self.valid(state, proof, now, tick)?;
        self.sessions
            .retain(|s| s.proof.principal_id != proof.principal_id);
        Ok(())
    }
    pub fn apply_events(&mut self, events: &[IdentityEvent]) {
        for event in events {
            match event {
                IdentityEvent::AllAuthorizationRevoked => self.sessions.clear(),
                IdentityEvent::AuthorizationRevoked(id) => {
                    self.sessions.retain(|s| s.proof.principal_id != *id)
                }
                _ => {}
            }
        }
    }
    pub fn revoke_web(&mut self) {
        self.sessions
            .retain(|s| s.proof.origin != TrustedOrigin::Web);
    }
}

/// Persist with the account on EVERY failed attempt. Restoring uses the complete last
/// penalty, so restarts/wall-clock rollback cannot erase an outstanding waiting period.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoginPenalty {
    pub failures: u32,
    pub wait_seconds: u32,
}
impl LoginPenalty {
    pub fn failure(self) -> Self {
        let failures = self.failures.saturating_add(1);
        let wait_seconds = if failures < 5 {
            0
        } else {
            1u32.checked_shl((failures - 5).min(10))
                .unwrap_or(900)
                .min(900)
        };
        Self {
            failures,
            wait_seconds,
        }
    }
    pub fn validate(self) -> Result<(), SessionError> {
        let expected = if self.failures < 5 {
            0
        } else {
            1u32.checked_shl((self.failures - 5).min(10))
                .unwrap_or(900)
                .min(900)
        };
        if self.wait_seconds == expected {
            Ok(())
        } else {
            Err(SessionError::Identity)
        }
    }
}
pub struct AuthenticationBackoff {
    penalty: LoginPenalty,
    not_before: u64,
    observed: u64,
}
impl AuthenticationBackoff {
    pub fn restore(penalty: LoginPenalty, tick: u64) -> Result<Self, SessionError> {
        penalty.validate()?;
        Ok(Self {
            penalty,
            not_before: tick
                .checked_add(u64::from(penalty.wait_seconds) * 1000)
                .ok_or(SessionError::Overflow)?,
            observed: tick,
        })
    }
    pub fn admit(&mut self, tick: u64) -> Result<(), SessionError> {
        if tick < self.observed {
            return Err(SessionError::ClockRegression);
        }
        self.observed = tick;
        if tick < self.not_before {
            Err(SessionError::RateLimited {
                retry_after_millis: self.not_before - tick,
            })
        } else {
            Ok(())
        }
    }
    pub fn failed(&mut self, tick: u64) -> Result<LoginPenalty, SessionError> {
        self.admit(tick)?;
        let penalty = self.penalty.failure();
        let deadline = tick
            .checked_add(u64::from(penalty.wait_seconds) * 1000)
            .ok_or(SessionError::Overflow)?;
        self.penalty = penalty;
        self.not_before = deadline;
        Ok(penalty)
    }
    pub fn succeeded(&mut self, tick: u64) -> Result<LoginPenalty, SessionError> {
        self.admit(tick)?;
        self.penalty = LoginPenalty::default();
        self.not_before = tick;
        Ok(self.penalty)
    }
}

#[cfg(test)]
mod tests;
