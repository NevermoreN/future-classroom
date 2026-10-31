//! Durable voice metadata and transitions. No audio bytes, filesystem paths,
//! database handles or device handles cross this boundary.
use crate::{auth::*, identity::IdentityState, ids::*, time::LocalDateTime};

pub const MAX_VOICE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_WAV_BYTES: u64 = 1_920_044;
// Bounds metadata held when filesystem deletion repeatedly fails; this is
// independent of the original-byte quota and never evicts an active recording.
pub const MAX_VOICE_ASSETS: usize = 32;
pub const MAX_VOICE_JOBS: usize = 104;
pub const MAX_VOICE_AUDIT: usize = 100;
pub const UPLOAD_TTL_SECONDS: i64 = 600;
pub const LATEST_START_SECONDS: i64 = 300;
pub const AUDIT_TTL_SECONDS: i64 = 86_400;
pub const DEFAULT_REPEAT_TOTAL: u8 = 3;
/// Persisted local admission policy. These values can only tighten the fixed
/// audio/resource bounds; old audits and outstanding device ownership continue
/// to be validated against the hard bounds rather than a newer policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceLimits {
    pub max_duration_millis: u32,
    pub max_pending_jobs: u8,
    pub max_pending_per_owner: u8,
    pub max_repeat_total: u8,
}
impl Default for VoiceLimits {
    fn default() -> Self {
        Self {
            max_duration_millis: 60_000,
            max_pending_jobs: 4,
            max_pending_per_owner: 2,
            max_repeat_total: 10,
        }
    }
}
impl VoiceLimits {
    pub fn validate(&self) -> Result<(), VoiceError> {
        if !(500..=60_000).contains(&self.max_duration_millis)
            || !(1..=4).contains(&self.max_pending_jobs)
            || !(1..=2.min(self.max_pending_jobs)).contains(&self.max_pending_per_owner)
            || !(1..=10).contains(&self.max_repeat_total)
        {
            Err(VoiceError::InvalidState)
        } else {
            Ok(())
        }
    }
    pub fn max_frame_count(&self) -> u32 {
        self.max_duration_millis * 16
    }
    pub fn max_wav_bytes(&self) -> u64 {
        44 + 2 * u64::from(self.max_frame_count())
    }
    pub fn default_repeat_total(&self) -> u8 {
        DEFAULT_REPEAT_TOTAL.min(self.max_repeat_total)
    }
}
macro_rules! voice_ids {
    ($($name:ident),+) => {$ (
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);
        impl $name {
            pub fn new(id:u64)->Result<Self,VoiceError>{if id==0 {Err(VoiceError::InvalidState)}else{Ok(Self(id))}}
            pub const fn get(self)->u64{self.0}
        }
    )+};
}
voice_ids!(VoiceAssetId, VoiceJobId);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceError {
    Permission(PermissionError),
    TargetMismatch,
    Disabled,
    InvalidWav,
    RepeatOutOfRange,
    QueueFull,
    OwnerLimit,
    Quota,
    Capacity,
    Missing,
    RevisionConflict,
    StaleGeneration,
    InvalidState,
    Expired,
    ClockRegression,
    Overflow,
    RateLimited,
}
impl From<PermissionError> for VoiceError {
    fn from(e: PermissionError) -> Self {
        Self::Permission(e)
    }
}
/// Civil seconds are used only for persisted deadlines. Native buffering uses
/// its own monotonic clock and must interrupt on a civil clock rollback.
pub fn civil_seconds(now: LocalDateTime) -> i64 {
    now.date.day_number() * 86_400 + i64::from(now.second_of_day())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoicePcmMetadata {
    pub byte_length: u64,
    pub frame_count: u32,
    pub sha256: [u8; 32],
}
impl VoicePcmMetadata {
    pub fn validate(&self) -> Result<(), VoiceError> {
        if !(8_000..=960_000).contains(&self.frame_count)
            || self.byte_length != 44 + u64::from(self.frame_count) * 2
        {
            Err(VoiceError::InvalidWav)
        } else {
            Ok(())
        }
    }
    pub fn duration_millis(&self) -> u64 {
        u64::from(self.frame_count) * 1_000 / 16_000
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceAssetState {
    Reserved,
    Ready,
    InUse,
    DeletePending,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceAsset {
    pub id: VoiceAssetId,
    pub owner: PrincipalId,
    pub class_id: ClassId,
    pub state: VoiceAssetState,
    pub revision: Revision,
    /// Reserved uploads charge MAX_WAV_BYTES before any body is accepted.
    pub charged_bytes: u64,
    pub pcm: Option<VoicePcmMetadata>,
    pub created_at: i64,
    pub expires_at: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceState {
    Queued,
    Starting,
    Playing,
    Gap,
    Stopping,
    Completed,
    Canceled,
    Failed,
    Expired,
    Interrupted,
}
impl VoiceState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Canceled | Self::Failed | Self::Expired | Self::Interrupted
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceFailure {
    Device,
    Session,
    Revoked,
    Disabled,
    Timeout,
    ClockRegression,
    Storage,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceJob {
    pub id: VoiceJobId,
    pub command_id: CommandId,
    pub asset_id: Option<VoiceAssetId>,
    pub owner: PrincipalId,
    pub class_id: ClassId,
    pub repeat_total: u8,
    pub completed_repeats: u8,
    pub current_repeat: u8,
    pub state: VoiceState,
    pub revision: Revision,
    pub generation: u64,
    /// Old device ownership remains live after cancel increments generation.
    pub release_generation: Option<u64>,
    pub pending_terminal: Option<VoiceState>,
    pub sequence: u64,
    pub submitted_at: i64,
    pub latest_start_at: i64,
    pub first_started_at: Option<i64>,
    pub gap_until: Option<i64>,
    pub ended_at: Option<i64>,
    pub failure: Option<VoiceFailure>,
    /// Exact original Web lease, removed from the terminal audit.
    pub authorization: Option<ActorContext>,
    pub cancel_command: Option<CommandId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceSnapshot {
    pub revision: Revision,
    pub enabled: bool,
    pub limits: VoiceLimits,
    pub next_sequence: u64,
    pub next_generation: u64,
    /// Never moves backwards, so rollback cannot renew an expired upload/job.
    pub observed_at: Option<i64>,
    pub assets: Vec<VoiceAsset>,
    pub jobs: Vec<VoiceJob>,
}
impl Default for VoiceSnapshot {
    fn default() -> Self {
        Self {
            revision: Revision::default(),
            enabled: true,
            limits: VoiceLimits::default(),
            next_sequence: 0,
            next_generation: 0,
            observed_at: None,
            assets: Vec::new(),
            jobs: Vec::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceDriverAction {
    Start,
    FirstWrite,
    /// First native Write has not happened, but its admission deadline passed.
    ExpireBeforeWrite,
    NaturalComplete,
    Released,
    Fail(VoiceFailure),
    Interrupt,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceDriverUpdate {
    pub job_id: VoiceJobId,
    pub expected_revision: Revision,
    pub generation: u64,
    pub action: VoiceDriverAction,
}
/// A Web actor must target the installation's actual current class. Teacher
/// authorization is any currently enabled teaching assignment in that class.
pub fn authorize_submit(
    i: &IdentityState,
    actor: &ActorContext,
    class_id: ClassId,
    now: LocalDateTime,
) -> Result<(), VoiceError> {
    let account = i
        .account(actor.principal_id)
        .map_err(|_| VoiceError::Permission(PermissionError::StaleAuthorization))?;
    if account.must_change_password {
        return Err(PermissionError::PasswordChangeRequired.into());
    }
    let a = i
        .authority(actor.principal_id)
        .map_err(|_| VoiceError::Permission(PermissionError::StaleAuthorization))?;
    a.validate_actor(actor, now)?;
    if actor.origin != TrustedOrigin::Web {
        return Err(PermissionError::WebOnly.into());
    }
    a.require_class(class_id)?;
    if i.installation.as_ref().and_then(|v| v.current_class) != Some(class_id) {
        return Err(VoiceError::TargetMismatch);
    }
    match a.principal.role {
        Role::GlobalAdmin => Ok(()),
        Role::TeacherAdmin
            if a.assignments.iter().any(|v| {
                v.enabled && v.teacher_id == actor.principal_id && v.class_id == class_id
            }) =>
        {
            Ok(())
        }
        Role::TeacherAdmin => Err(PermissionError::OutsideTeachingScope.into()),
        Role::User => Err(PermissionError::RoleDenied.into()),
    }
}
pub fn authorize_job(
    i: &IdentityState,
    actor: &ActorContext,
    job: &VoiceJob,
    now: LocalDateTime,
) -> Result<(), VoiceError> {
    let account = i
        .account(actor.principal_id)
        .map_err(|_| VoiceError::Permission(PermissionError::StaleAuthorization))?;
    if account.must_change_password {
        return Err(PermissionError::PasswordChangeRequired.into());
    }
    let a = i
        .authority(actor.principal_id)
        .map_err(|_| VoiceError::Permission(PermissionError::StaleAuthorization))?;
    a.validate_actor(actor, now)?;
    if actor.origin != TrustedOrigin::Web {
        return Err(PermissionError::WebOnly.into());
    }
    match a.principal.role {
        Role::GlobalAdmin => Ok(()),
        Role::TeacherAdmin if job.owner == actor.principal_id => Ok(()),
        Role::TeacherAdmin => Err(PermissionError::WrongOwner.into()),
        Role::User => Err(PermissionError::RoleDenied.into()),
    }
}
impl VoiceSnapshot {
    pub fn charged_bytes(&self) -> Result<u64, VoiceError> {
        self.assets.iter().try_fold(0u64, |n, a| {
            n.checked_add(a.charged_bytes).ok_or(VoiceError::Overflow)
        })
    }
    pub fn validate(&self) -> Result<(), VoiceError> {
        self.limits.validate()?;
        if self.assets.len() > MAX_VOICE_ASSETS
            || self.jobs.len() > MAX_VOICE_JOBS
            || self.charged_bytes()? > MAX_VOICE_BYTES
        {
            return Err(VoiceError::Capacity);
        }
        let active: Vec<_> = self.jobs.iter().filter(|j| !j.state.terminal()).collect();
        if active.len() > 4
            || self.jobs.iter().filter(|j| j.state.terminal()).count() > MAX_VOICE_AUDIT
        {
            return Err(VoiceError::QueueFull);
        }
        if active
            .iter()
            .any(|j| active.iter().filter(|v| v.owner == j.owner).count() > 2)
            || self
                .assets
                .iter()
                .filter(|a| a.state == VoiceAssetState::Reserved)
                .count()
                > 2
        {
            return Err(VoiceError::OwnerLimit);
        }
        for (n, a) in self.assets.iter().enumerate() {
            if a.revision == Revision::default()
                || a.expires_at
                    != a.created_at
                        .checked_add(UPLOAD_TTL_SECONDS)
                        .ok_or(VoiceError::Overflow)?
                || self.assets[..n].iter().any(|v| v.id == a.id)
            {
                return Err(VoiceError::InvalidState);
            }
            if self.observed_at.is_none_or(|t| a.created_at > t) {
                return Err(VoiceError::InvalidState);
            }
            match (&a.pcm, a.state) {
                (None, VoiceAssetState::Reserved | VoiceAssetState::DeletePending)
                    if a.charged_bytes == MAX_WAV_BYTES => {}
                (
                    Some(p),
                    VoiceAssetState::Ready
                    | VoiceAssetState::InUse
                    | VoiceAssetState::DeletePending,
                ) => {
                    p.validate()?;
                    if a.charged_bytes != p.byte_length {
                        return Err(VoiceError::InvalidState);
                    }
                }
                _ => return Err(VoiceError::InvalidState),
            }
            if matches!(a.state, VoiceAssetState::Reserved | VoiceAssetState::Ready)
                && self
                    .assets
                    .iter()
                    .filter(|v| {
                        v.owner == a.owner
                            && matches!(v.state, VoiceAssetState::Reserved | VoiceAssetState::Ready)
                    })
                    .count()
                    > 2
            {
                return Err(VoiceError::OwnerLimit);
            }
            let refs = self
                .jobs
                .iter()
                .filter(|j| j.asset_id == Some(a.id))
                .count();
            if refs > 1
                || (a.state == VoiceAssetState::InUse && refs != 1)
                || (matches!(a.state, VoiceAssetState::Reserved | VoiceAssetState::Ready)
                    && refs != 0)
            {
                return Err(VoiceError::InvalidState);
            }
        }
        for (n, j) in self.jobs.iter().enumerate() {
            if self.jobs[..n]
                .iter()
                .any(|v| v.id == j.id || v.command_id == j.command_id || v.sequence == j.sequence)
                || j.id.get() != j.command_id.get()
                || j.sequence == 0
                || j.sequence > self.next_sequence
                || !(1..=10).contains(&j.repeat_total)
                || j.completed_repeats > j.repeat_total
                || j.current_repeat > j.repeat_total
                || j.revision == Revision::default()
                || j.latest_start_at
                    != j.submitted_at
                        .checked_add(LATEST_START_SECONDS)
                        .ok_or(VoiceError::Overflow)?
                || j.generation > self.next_generation
                || j.release_generation
                    .is_some_and(|g| g == 0 || g > self.next_generation)
                || j.pending_terminal.is_some_and(|s| !s.terminal())
            {
                return Err(VoiceError::InvalidState);
            }
            if j.state.terminal() {
                if j.authorization.is_some()
                    || j.ended_at.is_none()
                    || j.release_generation.is_some()
                    || j.pending_terminal.is_some()
                {
                    return Err(VoiceError::InvalidState);
                }
            } else if j
                .authorization
                .as_ref()
                .is_none_or(|a| a.principal_id != j.owner || a.origin != TrustedOrigin::Web)
                || j.asset_id.is_none()
                || j.ended_at.is_some()
            {
                return Err(VoiceError::InvalidState);
            }
            if j.state == VoiceState::Stopping && j.pending_terminal.is_none() {
                return Err(VoiceError::InvalidState);
            }
            if j.state != VoiceState::Stopping && j.pending_terminal.is_some() {
                return Err(VoiceError::InvalidState);
            }
            match j.state {
                VoiceState::Queued
                    if j.generation != 0
                        || j.completed_repeats != 0
                        || j.current_repeat != 0
                        || j.release_generation.is_some()
                        || j.first_started_at.is_some()
                        || j.gap_until.is_some() =>
                {
                    return Err(VoiceError::InvalidState)
                }
                VoiceState::Starting | VoiceState::Playing
                    if j.generation == 0
                        || j.release_generation != Some(j.generation)
                        || j.current_repeat
                            != j.completed_repeats
                                .checked_add(1)
                                .ok_or(VoiceError::Overflow)?
                        || j.gap_until.is_some() =>
                {
                    return Err(VoiceError::InvalidState)
                }
                VoiceState::Playing if j.first_started_at.is_none() => {
                    return Err(VoiceError::InvalidState)
                }
                VoiceState::Gap
                    if j.completed_repeats == 0
                        || j.completed_repeats >= j.repeat_total
                        || j.current_repeat != j.completed_repeats
                        || j.first_started_at.is_none()
                        || j.gap_until.is_none() =>
                {
                    return Err(VoiceError::InvalidState)
                }
                VoiceState::Stopping if j.release_generation.is_none() => {
                    return Err(VoiceError::InvalidState)
                }
                VoiceState::Completed if j.first_started_at.is_none() => {
                    return Err(VoiceError::InvalidState)
                }
                _ => {}
            }
            if j.first_started_at.is_some_and(|t| t < j.submitted_at)
                || j.ended_at.is_some_and(|t| {
                    t < j.submitted_at || j.first_started_at.is_some_and(|f| t < f)
                })
                || self.observed_at.is_none_or(|t| {
                    j.submitted_at > t
                        || j.ended_at.is_some_and(|end| end > t)
                        || j.first_started_at.is_some_and(|first| first > t)
                })
            {
                return Err(VoiceError::InvalidState);
            }
            if j.state == VoiceState::Completed && j.completed_repeats != j.repeat_total {
                return Err(VoiceError::InvalidState);
            }
            if let Some(id) = j.asset_id {
                let a = self
                    .assets
                    .iter()
                    .find(|a| a.id == id)
                    .ok_or(VoiceError::InvalidState)?;
                if a.owner != j.owner
                    || a.class_id != j.class_id
                    || a.pcm.is_none()
                    || (!j.state.terminal() && a.state != VoiceAssetState::InUse)
                    || (j.state.terminal() && a.state != VoiceAssetState::DeletePending)
                {
                    return Err(VoiceError::InvalidState);
                }
            }
        }
        Ok(())
    }
    fn touch(&mut self, now: i64) -> Result<(), VoiceError> {
        if self.observed_at.is_some_and(|old| now < old) {
            return Err(VoiceError::ClockRegression);
        }
        self.observed_at = Some(now);
        self.revision = self.revision.next().map_err(|_| VoiceError::Overflow)?;
        Ok(())
    }
    pub fn reserve(
        &mut self,
        id: VoiceAssetId,
        actor: &ActorContext,
        class_id: ClassId,
        now: i64,
    ) -> Result<VoiceAsset, VoiceError> {
        self.limits.validate()?;
        if !self.enabled {
            return Err(VoiceError::Disabled);
        }
        if self.assets.iter().any(|v| v.id == id) {
            return Err(VoiceError::InvalidState);
        }
        if self.assets.len() >= MAX_VOICE_ASSETS {
            return Err(VoiceError::Capacity);
        }
        if self
            .assets
            .iter()
            .filter(|a| a.state == VoiceAssetState::Reserved)
            .count()
            >= 2
        {
            return Err(VoiceError::Capacity);
        }
        if self
            .assets
            .iter()
            .filter(|a| {
                a.owner == actor.principal_id
                    && matches!(a.state, VoiceAssetState::Reserved | VoiceAssetState::Ready)
            })
            .count()
            >= 2
        {
            return Err(VoiceError::OwnerLimit);
        }
        if self
            .charged_bytes()?
            .checked_add(MAX_WAV_BYTES)
            .ok_or(VoiceError::Overflow)?
            > MAX_VOICE_BYTES
        {
            return Err(VoiceError::Quota);
        }
        // The writer additionally persists rate counters, including removed uploads.
        if self
            .assets
            .iter()
            .filter(|a| a.owner == actor.principal_id && now.saturating_sub(a.created_at) < 60)
            .count()
            >= 10
        {
            return Err(VoiceError::RateLimited);
        }
        self.touch(now)?;
        let a = VoiceAsset {
            id,
            owner: actor.principal_id,
            class_id,
            state: VoiceAssetState::Reserved,
            revision: Revision::new(1),
            charged_bytes: MAX_WAV_BYTES,
            pcm: None,
            created_at: now,
            expires_at: now
                .checked_add(UPLOAD_TTL_SECONDS)
                .ok_or(VoiceError::Overflow)?,
        };
        self.assets.push(a.clone());
        Ok(a)
    }
    pub fn publish(
        &mut self,
        id: VoiceAssetId,
        expected: Revision,
        pcm: VoicePcmMetadata,
        now: i64,
    ) -> Result<VoiceAsset, VoiceError> {
        self.limits.validate()?;
        pcm.validate()?;
        if pcm.frame_count > self.limits.max_frame_count() {
            return Err(VoiceError::InvalidWav);
        }
        let n = self
            .assets
            .iter()
            .position(|a| a.id == id)
            .ok_or(VoiceError::Missing)?;
        let old = &self.assets[n];
        if old.revision != expected {
            return Err(VoiceError::RevisionConflict);
        }
        if old.state != VoiceAssetState::Reserved {
            return Err(VoiceError::InvalidState);
        }
        if now >= old.expires_at {
            return Err(VoiceError::Expired);
        }
        self.touch(now)?;
        let a = &mut self.assets[n];
        a.pcm = Some(pcm.clone());
        a.charged_bytes = pcm.byte_length;
        a.state = VoiceAssetState::Ready;
        a.revision = a.revision.next().map_err(|_| VoiceError::Overflow)?;
        Ok(a.clone())
    }
    pub fn submit(
        &mut self,
        command: CommandId,
        asset: VoiceAssetId,
        expected: Revision,
        total: u8,
        actor: &ActorContext,
        now: i64,
    ) -> Result<VoiceJob, VoiceError> {
        self.limits.validate()?;
        if !self.enabled {
            return Err(VoiceError::Disabled);
        }
        if !(1..=self.limits.max_repeat_total).contains(&total) {
            return Err(VoiceError::RepeatOutOfRange);
        }
        if self.jobs.iter().any(|j| j.command_id == command) {
            return Err(VoiceError::InvalidState);
        }
        if self.jobs.iter().filter(|j| !j.state.terminal()).count()
            >= usize::from(self.limits.max_pending_jobs)
        {
            return Err(VoiceError::QueueFull);
        }
        if self
            .jobs
            .iter()
            .filter(|j| !j.state.terminal() && j.owner == actor.principal_id)
            .count()
            >= usize::from(self.limits.max_pending_per_owner)
        {
            return Err(VoiceError::OwnerLimit);
        }
        if self.jobs.len() >= MAX_VOICE_JOBS {
            return Err(VoiceError::Capacity);
        }
        let n = self
            .assets
            .iter()
            .position(|a| a.id == asset)
            .ok_or(VoiceError::Missing)?;
        let a = &self.assets[n];
        if a.revision != expected {
            return Err(VoiceError::RevisionConflict);
        }
        if a.owner != actor.principal_id {
            return Err(PermissionError::WrongOwner.into());
        }
        if a.state != VoiceAssetState::Ready {
            return Err(VoiceError::InvalidState);
        }
        let pcm = a.pcm.as_ref().ok_or(VoiceError::InvalidState)?;
        pcm.validate()?;
        if pcm.frame_count > self.limits.max_frame_count() {
            return Err(VoiceError::InvalidWav);
        }
        if now >= a.expires_at {
            return Err(VoiceError::Expired);
        }
        if self
            .jobs
            .iter()
            .filter(|j| j.owner == actor.principal_id && now.saturating_sub(j.submitted_at) < 60)
            .count()
            >= 10
        {
            return Err(VoiceError::RateLimited);
        }
        self.touch(now)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(VoiceError::Overflow)?;
        let a = &mut self.assets[n];
        a.state = VoiceAssetState::InUse;
        a.revision = a.revision.next().map_err(|_| VoiceError::Overflow)?;
        let j = VoiceJob {
            id: VoiceJobId::new(command.get())?,
            command_id: command,
            asset_id: Some(asset),
            owner: actor.principal_id,
            class_id: a.class_id,
            repeat_total: total,
            completed_repeats: 0,
            current_repeat: 0,
            state: VoiceState::Queued,
            revision: Revision::new(1),
            generation: 0,
            release_generation: None,
            pending_terminal: None,
            sequence: self.next_sequence,
            submitted_at: now,
            latest_start_at: now
                .checked_add(LATEST_START_SECONDS)
                .ok_or(VoiceError::Overflow)?,
            first_started_at: None,
            gap_until: None,
            ended_at: None,
            failure: None,
            authorization: Some(actor.clone()),
            cancel_command: None,
        };
        self.jobs.push(j.clone());
        Ok(j)
    }
    fn finish(&mut self, n: usize, state: VoiceState, now: i64) -> Result<(), VoiceError> {
        let j = &mut self.jobs[n];
        j.state = state;
        j.ended_at = Some(now);
        j.authorization = None;
        j.pending_terminal = None;
        j.release_generation = None;
        j.gap_until = None;
        if let Some(id) = j.asset_id {
            let a = self
                .assets
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or(VoiceError::InvalidState)?;
            a.state = VoiceAssetState::DeletePending;
            a.revision = a.revision.next().map_err(|_| VoiceError::Overflow)?;
        }
        Ok(())
    }
    fn stop(&mut self, n: usize, state: VoiceState, now: i64) -> Result<(), VoiceError> {
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(VoiceError::Overflow)?;
        self.jobs[n].generation = self.next_generation;
        if self.jobs[n].release_generation.is_some() {
            self.jobs[n].state = VoiceState::Stopping;
            self.jobs[n].pending_terminal = Some(state);
        } else {
            self.finish(n, state, now)?;
        }
        Ok(())
    }
    pub fn cancel(
        &mut self,
        id: VoiceJobId,
        expected: Revision,
        command: CommandId,
        now: i64,
    ) -> Result<VoiceJob, VoiceError> {
        let n = self
            .jobs
            .iter()
            .position(|j| j.id == id)
            .ok_or(VoiceError::Missing)?;
        if self.jobs[n].revision != expected {
            return Err(VoiceError::RevisionConflict);
        }
        if self.jobs[n].state.terminal() || self.jobs[n].state == VoiceState::Stopping {
            return Err(VoiceError::InvalidState);
        }
        self.touch(now)?;
        self.jobs[n].cancel_command = Some(command);
        self.stop(n, VoiceState::Canceled, now)?;
        self.jobs[n].revision = self.jobs[n]
            .revision
            .next()
            .map_err(|_| VoiceError::Overflow)?;
        Ok(self.jobs[n].clone())
    }
    pub fn drive(&mut self, u: &VoiceDriverUpdate, now: i64) -> Result<VoiceJob, VoiceError> {
        let n = self
            .jobs
            .iter()
            .position(|j| j.id == u.job_id)
            .ok_or(VoiceError::Missing)?;
        let j = &self.jobs[n];
        if j.revision != u.expected_revision {
            return Err(VoiceError::RevisionConflict);
        }
        let token = if u.action == VoiceDriverAction::Released {
            j.release_generation
        } else {
            Some(j.generation)
        };
        if token != Some(u.generation) {
            return Err(VoiceError::StaleGeneration);
        }
        if j.state.terminal() {
            return Err(VoiceError::InvalidState);
        }
        match u.action {
            VoiceDriverAction::Start => {
                if !self.enabled {
                    return Err(VoiceError::Disabled);
                }
                if !matches!(j.state, VoiceState::Queued | VoiceState::Gap)
                    || j.release_generation.is_some()
                    || j.gap_until.is_some_and(|t| now < t)
                {
                    return Err(VoiceError::InvalidState);
                }
                if self
                    .jobs
                    .iter()
                    .any(|other| !other.state.terminal() && other.sequence < j.sequence)
                {
                    return Err(VoiceError::InvalidState);
                }
                if j.first_started_at.is_none() && now >= j.latest_start_at {
                    self.touch(now)?;
                    self.finish(n, VoiceState::Expired, now)?;
                } else {
                    self.touch(now)?;
                    self.next_generation = self
                        .next_generation
                        .checked_add(1)
                        .ok_or(VoiceError::Overflow)?;
                    let j = &mut self.jobs[n];
                    j.generation = self.next_generation;
                    j.release_generation = Some(j.generation);
                    j.state = VoiceState::Starting;
                    j.current_repeat = j.completed_repeats + 1;
                    j.gap_until = None;
                }
            }
            VoiceDriverAction::FirstWrite => {
                if j.state != VoiceState::Starting {
                    return Err(VoiceError::InvalidState);
                }
                if j.first_started_at.is_none() && now >= j.latest_start_at {
                    self.touch(now)?;
                    self.stop(n, VoiceState::Expired, now)?;
                } else {
                    self.touch(now)?;
                    let j = &mut self.jobs[n];
                    j.state = VoiceState::Playing;
                    j.first_started_at.get_or_insert(now);
                }
            }
            VoiceDriverAction::ExpireBeforeWrite => {
                if !matches!(j.state, VoiceState::Queued | VoiceState::Starting)
                    || j.first_started_at.is_some()
                    || now < j.latest_start_at
                {
                    return Err(VoiceError::InvalidState);
                }
                // This is a termination operation. A previous trusted higher
                // civil observation cannot prevent draining an expired device.
                let at = now.max(self.observed_at.unwrap_or(now));
                self.touch(at)?;
                if self.jobs[n].state == VoiceState::Starting {
                    self.stop(n, VoiceState::Expired, at)?;
                } else {
                    self.finish(n, VoiceState::Expired, at)?;
                }
            }
            VoiceDriverAction::NaturalComplete => {
                if j.state != VoiceState::Playing {
                    return Err(VoiceError::InvalidState);
                }
                self.touch(now)?;
                let j = &mut self.jobs[n];
                j.completed_repeats = j
                    .completed_repeats
                    .checked_add(1)
                    .ok_or(VoiceError::Overflow)?;
                if j.completed_repeats == j.repeat_total {
                    j.state = VoiceState::Stopping;
                    j.pending_terminal = Some(VoiceState::Completed);
                } else {
                    j.state = VoiceState::Gap;
                    j.gap_until = Some(now.checked_add(1).ok_or(VoiceError::Overflow)?);
                }
            }
            VoiceDriverAction::Released => {
                if !matches!(j.state, VoiceState::Stopping | VoiceState::Gap) {
                    return Err(VoiceError::InvalidState);
                }
                self.touch(now)?;
                let terminal = self.jobs[n].pending_terminal;
                self.jobs[n].release_generation = None;
                if let Some(state) = terminal {
                    self.finish(n, state, now)?;
                }
            }
            VoiceDriverAction::Fail(f) => {
                self.touch(now)?;
                self.jobs[n].failure = Some(f);
                self.stop(n, VoiceState::Failed, now)?;
            }
            VoiceDriverAction::Interrupt => {
                self.touch(now)?;
                self.stop(n, VoiceState::Interrupted, now)?;
            }
        }
        self.jobs[n].revision = self.jobs[n]
            .revision
            .next()
            .map_err(|_| VoiceError::Overflow)?;
        Ok(self.jobs[n].clone())
    }
    /// Exclusive-instance startup only: the old process and its native device
    /// ownership no longer exist. No recovered task can become playable again.
    pub fn recover(&mut self, now: i64) -> Result<(), VoiceError> {
        if self.jobs.iter().all(|j| j.state.terminal())
            && self
                .assets
                .iter()
                .all(|a| a.state == VoiceAssetState::DeletePending)
        {
            return Ok(());
        }
        let now = now.max(self.observed_at.unwrap_or(now));
        self.touch(now)?;
        for n in 0..self.jobs.len() {
            if !self.jobs[n].state.terminal() {
                self.next_generation = self
                    .next_generation
                    .checked_add(1)
                    .ok_or(VoiceError::Overflow)?;
                self.jobs[n].generation = self.next_generation;
                self.finish(n, VoiceState::Interrupted, now)?;
                self.jobs[n].revision = self.jobs[n]
                    .revision
                    .next()
                    .map_err(|_| VoiceError::Overflow)?;
            }
        }
        for a in &mut self.assets {
            if a.state != VoiceAssetState::DeletePending {
                a.state = VoiceAssetState::DeletePending;
                a.revision = a.revision.next().map_err(|_| VoiceError::Overflow)?;
            }
        }
        Ok(())
    }
    /// Returns pruned jobs so the same writer commit can delete their receipts.
    pub fn maintain(&mut self, now: i64) -> Result<Vec<VoiceJob>, VoiceError> {
        let before = self.clone();
        let now = now.max(self.observed_at.unwrap_or(now));
        for a in &mut self.assets {
            if matches!(a.state, VoiceAssetState::Reserved | VoiceAssetState::Ready)
                && now >= a.expires_at
            {
                a.state = VoiceAssetState::DeletePending;
                a.revision = a.revision.next().map_err(|_| VoiceError::Overflow)?;
            }
        }
        for n in 0..self.jobs.len() {
            if self.jobs[n].state == VoiceState::Queued && now >= self.jobs[n].latest_start_at {
                self.finish(n, VoiceState::Expired, now)?;
                self.jobs[n].revision = self.jobs[n]
                    .revision
                    .next()
                    .map_err(|_| VoiceError::Overflow)?;
            }
        }
        let mut removed = Vec::new();
        loop {
            let terminals = self.jobs.iter().filter(|j| j.state.terminal()).count();
            let candidate = self
                .jobs
                .iter()
                .enumerate()
                .filter(|(_, j)| j.state.terminal())
                .min_by_key(|(_, j)| j.sequence)
                .map(|(n, _)| n);
            let Some(n) = candidate else { break };
            if terminals <= MAX_VOICE_AUDIT
                && self.jobs[n]
                    .ended_at
                    .is_some_and(|end| now.saturating_sub(end) < AUDIT_TTL_SECONDS)
            {
                break;
            }
            removed.push(self.jobs.remove(n));
        }
        if *self != before {
            self.touch(now)?;
        }
        Ok(removed)
    }
    /// Only after exclusive deletion of the actual controlled object succeeds.
    pub fn finalize_delete(
        &mut self,
        id: VoiceAssetId,
        expected: Revision,
        now: i64,
    ) -> Result<(), VoiceError> {
        let n = self
            .assets
            .iter()
            .position(|a| a.id == id)
            .ok_or(VoiceError::Missing)?;
        let a = &self.assets[n];
        if a.revision != expected {
            return Err(VoiceError::RevisionConflict);
        }
        if a.state != VoiceAssetState::DeletePending
            || self
                .jobs
                .iter()
                .any(|j| j.asset_id == Some(id) && !j.state.terminal())
        {
            return Err(VoiceError::InvalidState);
        }
        self.touch(now.max(self.observed_at.unwrap_or(now)))?;
        for j in &mut self.jobs {
            if j.asset_id == Some(id) {
                j.asset_id = None;
            }
        }
        self.assets.remove(n);
        Ok(())
    }
    /// Backup has no active bytes or replayable capability. Source is unchanged.
    pub fn backup_snapshot(&self, now: i64) -> Result<Self, VoiceError> {
        let mut backup = self.clone();
        backup.recover(now)?;
        for j in &mut backup.jobs {
            j.asset_id = None;
        }
        backup.assets.clear();
        backup.maintain(now)?;
        Ok(backup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{LocalDate, LocalDateTime};
    fn actor(n: u64) -> ActorContext {
        ActorContext {
            principal_id: PrincipalId::new(n).unwrap(),
            school_id: SchoolId::new(1).unwrap(),
            auth_epoch: AuthEpoch::new(1),
            expires_at: LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 0).unwrap(),
            origin: TrustedOrigin::Web,
        }
    }
    fn pcm() -> VoicePcmMetadata {
        VoicePcmMetadata {
            byte_length: 16_044,
            frame_count: 8_000,
            sha256: [1; 32],
        }
    }
    fn job(s: &mut VoiceSnapshot, id: u64, total: u8) -> VoiceJob {
        let a = actor(id);
        let id = VoiceAssetId::new(id).unwrap();
        let asset = s.reserve(id, &a, ClassId::new(1).unwrap(), 1).unwrap();
        let asset = s.publish(id, asset.revision, pcm(), 1).unwrap();
        s.submit(
            CommandId::new(id.get()).unwrap(),
            id,
            asset.revision,
            total,
            &a,
            1,
        )
        .unwrap()
    }
    fn drive(
        s: &mut VoiceSnapshot,
        j: &VoiceJob,
        action: VoiceDriverAction,
        now: i64,
    ) -> Result<VoiceJob, VoiceError> {
        s.drive(
            &VoiceDriverUpdate {
                job_id: j.id,
                expected_revision: j.revision,
                generation: if action == VoiceDriverAction::Released {
                    j.release_generation.unwrap()
                } else {
                    j.generation
                },
                action,
            },
            now,
        )
    }
    #[test]
    fn canonical_metadata_limits_and_upload_reservation_quota_are_real() {
        assert!(pcm().validate().is_ok());
        assert_eq!(
            VoicePcmMetadata {
                frame_count: 7_999,
                ..pcm()
            }
            .validate(),
            Err(VoiceError::InvalidWav)
        );
        assert_eq!(
            VoicePcmMetadata {
                frame_count: 960_001,
                byte_length: 1_920_046,
                ..pcm()
            }
            .validate(),
            Err(VoiceError::InvalidWav)
        );
        let mut s = VoiceSnapshot::default();
        s.reserve(
            VoiceAssetId::new(1).unwrap(),
            &actor(1),
            ClassId::new(1).unwrap(),
            0,
        )
        .unwrap();
        s.reserve(
            VoiceAssetId::new(2).unwrap(),
            &actor(2),
            ClassId::new(1).unwrap(),
            0,
        )
        .unwrap();
        assert_eq!(s.charged_bytes().unwrap(), 2 * MAX_WAV_BYTES);
        assert_eq!(
            s.reserve(
                VoiceAssetId::new(3).unwrap(),
                &actor(3),
                ClassId::new(1).unwrap(),
                0
            ),
            Err(VoiceError::Capacity)
        );
        let bytes = s.charged_bytes().unwrap();
        s.maintain(600).unwrap();
        assert_eq!(s.charged_bytes().unwrap(), bytes);
        let a = s.assets[0].clone();
        s.finalize_delete(a.id, a.revision, 600).unwrap();
        assert_eq!(s.charged_bytes().unwrap(), MAX_WAV_BYTES);
    }
    #[test]
    fn actual_first_write_natural_completion_gap_and_release_are_distinct() {
        let mut s = VoiceSnapshot::default();
        let j = job(&mut s, 1, 3);
        let mut j = drive(&mut s, &j, VoiceDriverAction::Start, 4).unwrap();
        assert_eq!(j.state, VoiceState::Starting);
        assert_eq!(j.first_started_at, None);
        for n in 1..=3 {
            j = drive(&mut s, &j, VoiceDriverAction::FirstWrite, 4 + n * 3).unwrap();
            assert_eq!(j.state, VoiceState::Playing);
            assert_eq!(j.completed_repeats, n as u8 - 1);
            j = drive(&mut s, &j, VoiceDriverAction::NaturalComplete, 5 + n * 3).unwrap();
            assert_eq!(j.completed_repeats, n as u8);
            if n < 3 {
                assert_eq!(j.state, VoiceState::Gap);
                assert_eq!(
                    drive(&mut s, &j, VoiceDriverAction::Start, 6 + n * 3),
                    Err(VoiceError::InvalidState)
                );
                j = drive(&mut s, &j, VoiceDriverAction::Released, 5 + n * 3).unwrap();
                assert_eq!(
                    drive(&mut s, &j, VoiceDriverAction::Start, 5 + n * 3),
                    Err(VoiceError::InvalidState)
                );
                j = drive(&mut s, &j, VoiceDriverAction::Start, 6 + n * 3).unwrap();
            } else {
                assert_eq!(j.state, VoiceState::Stopping);
                j = drive(&mut s, &j, VoiceDriverAction::Released, 5 + n * 3).unwrap();
                assert_eq!(j.state, VoiceState::Completed);
            }
        }
        assert!(j.authorization.is_none());
        assert_eq!(s.assets[0].state, VoiceAssetState::DeletePending);
        s.validate().unwrap();
    }
    #[test]
    fn cancel_invalidates_callbacks_before_device_release_and_preserves_fifo() {
        let mut s = VoiceSnapshot::default();
        let first = job(&mut s, 1, 3);
        let second = job(&mut s, 2, 3);
        let old = drive(&mut s, &first, VoiceDriverAction::Start, 4).unwrap();
        let old = drive(&mut s, &old, VoiceDriverAction::FirstWrite, 4).unwrap();
        let canceled = s
            .cancel(old.id, old.revision, CommandId::new(90).unwrap(), 5)
            .unwrap();
        assert_eq!(canceled.state, VoiceState::Stopping);
        assert_ne!(canceled.generation, old.generation);
        assert_eq!(
            s.drive(
                &VoiceDriverUpdate {
                    job_id: old.id,
                    expected_revision: canceled.revision,
                    generation: old.generation,
                    action: VoiceDriverAction::NaturalComplete
                },
                5
            ),
            Err(VoiceError::StaleGeneration)
        );
        assert_eq!(
            drive(&mut s, &second, VoiceDriverAction::Start, 5),
            Err(VoiceError::InvalidState)
        );
        let done = drive(&mut s, &canceled, VoiceDriverAction::Released, 5).unwrap();
        assert_eq!(done.state, VoiceState::Canceled);
        assert_eq!(done.completed_repeats, 0);
        assert!(drive(&mut s, &second, VoiceDriverAction::Start, 5).is_ok());
        s.validate().unwrap();
    }
    #[test]
    fn expire_before_write_preserves_exact_release_without_claiming_playback() {
        let mut s = VoiceSnapshot::default();
        let queued = job(&mut s, 1, 3);
        assert_eq!(
            drive(&mut s, &queued, VoiceDriverAction::ExpireBeforeWrite, 300),
            Err(VoiceError::InvalidState)
        );
        let expired = drive(&mut s, &queued, VoiceDriverAction::ExpireBeforeWrite, 301).unwrap();
        assert_eq!(expired.state, VoiceState::Expired);
        assert_eq!(expired.generation, 0);
        assert_eq!(expired.completed_repeats, 0);
        assert!(expired.first_started_at.is_none() && expired.release_generation.is_none());
        s.validate().unwrap();
        let mut s = VoiceSnapshot::default();
        let queued = job(&mut s, 1, 3);
        let starting = drive(&mut s, &queued, VoiceDriverAction::Start, 300).unwrap();
        let stopping = drive(&mut s, &starting, VoiceDriverAction::ExpireBeforeWrite, 301).unwrap();
        assert_eq!(stopping.state, VoiceState::Stopping);
        assert_eq!(stopping.pending_terminal, Some(VoiceState::Expired));
        assert_eq!(stopping.release_generation, Some(starting.generation));
        assert_ne!(stopping.generation, starting.generation);
        assert_eq!(stopping.completed_repeats, 0);
        assert!(stopping.first_started_at.is_none());
        assert_eq!(
            s.drive(
                &VoiceDriverUpdate {
                    job_id: stopping.id,
                    expected_revision: stopping.revision,
                    generation: starting.generation,
                    action: VoiceDriverAction::FirstWrite
                },
                301
            ),
            Err(VoiceError::StaleGeneration)
        );
        let expired = drive(&mut s, &stopping, VoiceDriverAction::Released, 301).unwrap();
        assert_eq!(expired.state, VoiceState::Expired);
        assert_eq!(expired.completed_repeats, 0);
        assert!(expired.first_started_at.is_none());
        s.validate().unwrap();
        let mut s = VoiceSnapshot::default();
        let queued = job(&mut s, 1, 3);
        let started = drive(&mut s, &queued, VoiceDriverAction::Start, 2).unwrap();
        let playing = drive(&mut s, &started, VoiceDriverAction::FirstWrite, 2).unwrap();
        assert_eq!(
            drive(&mut s, &playing, VoiceDriverAction::ExpireBeforeWrite, 301),
            Err(VoiceError::InvalidState)
        );
        let gap = drive(&mut s, &playing, VoiceDriverAction::NaturalComplete, 301).unwrap();
        let gap = drive(&mut s, &gap, VoiceDriverAction::Released, 301).unwrap();
        let repeat = drive(&mut s, &gap, VoiceDriverAction::Start, 302).unwrap();
        assert_eq!(
            drive(&mut s, &repeat, VoiceDriverAction::ExpireBeforeWrite, 302),
            Err(VoiceError::InvalidState)
        );
    }
    #[test]
    fn latest_start_recovery_backup_and_old_callbacks_cannot_replay() {
        let mut s = VoiceSnapshot::default();
        let j = job(&mut s, 1, 1);
        let j = drive(&mut s, &j, VoiceDriverAction::Start, 303).unwrap();
        assert_eq!(j.state, VoiceState::Expired);
        let mut s = VoiceSnapshot::default();
        let j = job(&mut s, 1, 10);
        let j = drive(&mut s, &j, VoiceDriverAction::Start, 4).unwrap();
        let original = s.clone();
        let backup = s.backup_snapshot(5).unwrap();
        assert_eq!(s, original);
        assert!(backup.assets.is_empty());
        assert_eq!(backup.jobs[0].state, VoiceState::Interrupted);
        assert!(backup.jobs[0].asset_id.is_none());
        backup.validate().unwrap();
        s.recover(5).unwrap();
        assert_eq!(s.jobs[0].state, VoiceState::Interrupted);
        assert_eq!(s.assets[0].state, VoiceAssetState::DeletePending);
        assert_eq!(
            drive(&mut s, &j, VoiceDriverAction::FirstWrite, 6),
            Err(VoiceError::RevisionConflict)
        );
        s.validate().unwrap();
    }
    #[test]
    fn configured_limits_validate_exact_ranges_and_derive_canonical_bounds() {
        let default = VoiceLimits::default();
        assert_eq!(default.max_frame_count(), 960_000);
        assert_eq!(default.max_wav_bytes(), MAX_WAV_BYTES);
        assert_eq!(default.default_repeat_total(), 3);
        let small = VoiceLimits {
            max_duration_millis: 500,
            max_pending_jobs: 1,
            max_pending_per_owner: 1,
            max_repeat_total: 1,
        };
        small.validate().unwrap();
        assert_eq!(small.max_frame_count(), 8_000);
        assert_eq!(small.max_wav_bytes(), 16_044);
        assert_eq!(small.default_repeat_total(), 1);
        for invalid in [
            VoiceLimits {
                max_duration_millis: 499,
                ..default
            },
            VoiceLimits {
                max_duration_millis: 60_001,
                ..default
            },
            VoiceLimits {
                max_pending_jobs: 0,
                ..default
            },
            VoiceLimits {
                max_pending_jobs: 5,
                ..default
            },
            VoiceLimits {
                max_pending_per_owner: 0,
                ..default
            },
            VoiceLimits {
                max_pending_per_owner: 3,
                ..default
            },
            VoiceLimits {
                max_pending_jobs: 1,
                max_pending_per_owner: 2,
                ..default
            },
            VoiceLimits {
                max_repeat_total: 0,
                ..default
            },
            VoiceLimits {
                max_repeat_total: 11,
                ..default
            },
        ] {
            assert_eq!(invalid.validate(), Err(VoiceError::InvalidState));
        }
    }
    #[test]
    fn configured_duration_and_repeat_enforced_at_publish_and_existing_ready_submit() {
        let mut s = VoiceSnapshot::default();
        let a = actor(1);
        let id = VoiceAssetId::new(1).unwrap();
        let reserved = s.reserve(id, &a, ClassId::new(1).unwrap(), 1).unwrap();
        let long = VoicePcmMetadata {
            byte_length: 32_044,
            frame_count: 16_000,
            ..pcm()
        };
        s.limits.max_duration_millis = 500;
        let before = s.clone();
        assert_eq!(
            s.publish(id, reserved.revision, long.clone(), 1),
            Err(VoiceError::InvalidWav)
        );
        assert_eq!(s, before);
        s.limits.max_duration_millis = 1000;
        let ready = s.publish(id, reserved.revision, long, 1).unwrap();
        s.limits.max_duration_millis = 500;
        assert_eq!(
            s.submit(CommandId::new(1).unwrap(), id, ready.revision, 1, &a, 1),
            Err(VoiceError::InvalidWav)
        );
        s.limits.max_duration_millis = 1000;
        s.limits.max_repeat_total = 1;
        assert_eq!(
            s.submit(CommandId::new(1).unwrap(), id, ready.revision, 2, &a, 1),
            Err(VoiceError::RepeatOutOfRange)
        );
        assert_eq!(
            s.submit(CommandId::new(1).unwrap(), id, ready.revision, 1, &a, 1)
                .unwrap()
                .repeat_total,
            1
        );
    }
    #[test]
    fn configured_queue_and_owner_caps_do_not_reinterpret_old_jobs_as_corrupt() {
        let mut s = VoiceSnapshot::default();
        job(&mut s, 1, 10);
        job(&mut s, 2, 10);
        s.limits = VoiceLimits {
            max_duration_millis: 500,
            max_pending_jobs: 1,
            max_pending_per_owner: 1,
            max_repeat_total: 1,
        };
        // New policy is admission-only; existing nonterminal evidence remains
        // valid until exact physical stop/release receipts retire it.
        s.validate().unwrap();
        let a = actor(3);
        let id = VoiceAssetId::new(3).unwrap();
        let reserved = s.reserve(id, &a, ClassId::new(1).unwrap(), 1).unwrap();
        let ready = s.publish(id, reserved.revision, pcm(), 1).unwrap();
        assert_eq!(
            s.submit(CommandId::new(3).unwrap(), id, ready.revision, 1, &a, 1),
            Err(VoiceError::QueueFull)
        );
        s.limits.max_pending_jobs = 4;
        let own = actor(1);
        let id = VoiceAssetId::new(4).unwrap();
        let reserved = s.reserve(id, &own, ClassId::new(1).unwrap(), 1).unwrap();
        let ready = s.publish(id, reserved.revision, pcm(), 1).unwrap();
        assert_eq!(
            s.submit(CommandId::new(4).unwrap(), id, ready.revision, 1, &own, 1),
            Err(VoiceError::OwnerLimit)
        );
        let id = VoiceAssetId::new(5).unwrap();
        s.reserve(id, &own, ClassId::new(1).unwrap(), 1).unwrap();
        assert_eq!(
            s.reserve(
                VoiceAssetId::new(6).unwrap(),
                &own,
                ClassId::new(1).unwrap(),
                1
            ),
            Err(VoiceError::OwnerLimit)
        );
    }
}
