//! Update prefilters only. Verified metadata and native installer admission remain
//! independent requirements; none of these values authorizes executing Setup.
use crate::time::Minute;

pub const UPDATE_INSTALLER_ABI: u16 = 2;
pub const UPDATE_GUARD_CAPABILITY: u16 = 3;
pub const MIN_UPDATE_IDLE_SECONDS: u64 = 300;
pub const MIN_NEXT_LESSON_SECONDS: u64 = 1800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidUpdateValue;

/// Human-readable three-part version; release sequence, not this label, orders
/// different builds. Accepting one spelling avoids aliases such as `01.2.3`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}
impl ReleaseVersion {
    pub fn parse(value: &str) -> Result<Self, InvalidUpdateValue> {
        fn part(value: Option<&str>) -> Result<u32, InvalidUpdateValue> {
            let value = value.ok_or(InvalidUpdateValue)?;
            if value.is_empty()
                || value.len() > 10
                || (value.len() > 1 && value.starts_with('0'))
                || !value.bytes().all(|v| v.is_ascii_digit())
            {
                return Err(InvalidUpdateValue);
            }
            value.parse().map_err(|_| InvalidUpdateValue)
        }
        let mut parts = value.split('.');
        let version = Self {
            major: part(parts.next())?,
            minor: part(parts.next())?,
            patch: part(parts.next())?,
        };
        if parts.next().is_some() {
            return Err(InvalidUpdateValue);
        }
        Ok(version)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseSequence(u64);
impl ReleaseSequence {
    pub fn new(value: u64) -> Result<Self, InvalidUpdateValue> {
        if value == 0 {
            Err(InvalidUpdateValue)
        } else {
            Ok(Self(value))
        }
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseIdentity {
    pub sequence: ReleaseSequence,
    pub payload_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowsBaseline {
    Windows7Sp1,
    Windows10,
    Windows11,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateArchitecture {
    X86,
    X64,
}

/// Observations must come from the native sealed source/installed package, not
/// from a peer announcement or a caller's version string. Full envelope identity
/// includes the original payload manifest and package/product identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactPredecessor {
    pub product_code: [u8; 16],
    pub package_code: [u8; 16],
    pub source_commit: [u8; 20],
    pub source_manifest_sha256: [u8; 32],
    pub producer_sha256: [u8; 32],
    pub cached_msi_sha256: [u8; 32],
    pub payload_envelope_sha256: [u8; 32],
    pub template_sha256: [u8; 32],
    pub guard_sha256: [u8; 32],
    pub logical_sha256: [u8; 32],
    pub template_bytes: u32,
    pub installer_abi: u16,
    pub guard_capability: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatibilityRange {
    first: u32,
    last: u32,
}
impl CompatibilityRange {
    pub fn new(first: u32, last: u32) -> Result<Self, InvalidUpdateValue> {
        if first > last {
            return Err(InvalidUpdateValue);
        }
        Ok(Self { first, last })
    }
    pub const fn contains(self, value: u32) -> bool {
        self.first <= value && value <= self.last
    }
    pub const fn first(self) -> u32 {
        self.first
    }
    pub const fn last(self) -> u32 {
        self.last
    }
}

pub struct ReleaseCompatibility<'a> {
    pub product: &'a str,
    pub channel: &'a str,
    pub identity: ReleaseIdentity,
    pub minimum_windows: WindowsBaseline,
    pub architecture: UpdateArchitecture,
    pub installer_abi: u16,
    pub guard_capability: u16,
    pub minimum_coordinator: ReleaseVersion,
    pub source_schema: CompatibilityRange,
    pub source_protocol: CompatibilityRange,
    pub allowed_predecessors: &'a [ExactPredecessor],
}
pub struct InstalledUpdateContext<'a> {
    pub product: &'a str,
    pub channel: &'a str,
    pub installed_release: ReleaseIdentity,
    pub highest_accepted_release: Option<ReleaseIdentity>,
    pub windows: WindowsBaseline,
    /// Installation target, rather than native CPU: x86 installations can run
    /// on x64 Windows but must still receive their exact x86 successor.
    pub architecture: UpdateArchitecture,
    pub coordinator: ReleaseVersion,
    pub schema: u32,
    pub protocol: u32,
    /// None covers unqualified Showcase/ZIP and unavailable native evidence.
    pub formal_predecessor: Option<&'a ExactPredecessor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateEligibilityFailure {
    WrongProduct,
    WrongChannel,
    ReleaseRollback,
    ReleaseConflict,
    AlreadyInstalled,
    WrongPlatform,
    UnsupportedInstaller,
    CoordinatorTooOld,
    IncompatibleSchema,
    IncompatibleProtocol,
    UnqualifiedInstallation,
    PredecessorMismatch,
}

pub fn preflight_release(
    release: &ReleaseCompatibility<'_>,
    installed: &InstalledUpdateContext<'_>,
) -> Result<(), UpdateEligibilityFailure> {
    use UpdateEligibilityFailure::*;
    if release.product != installed.product {
        return Err(WrongProduct);
    }
    if release.channel != installed.channel {
        return Err(WrongChannel);
    }
    for accepted in [
        Some(installed.installed_release),
        installed.highest_accepted_release,
    ]
    .into_iter()
    .flatten()
    {
        if release.identity.sequence < accepted.sequence {
            return Err(ReleaseRollback);
        }
        if release.identity.sequence == accepted.sequence
            && release.identity.payload_sha256 != accepted.payload_sha256
        {
            return Err(ReleaseConflict);
        }
    }
    if release.identity.sequence == installed.installed_release.sequence {
        return Err(AlreadyInstalled);
    }
    if installed.windows < release.minimum_windows || installed.architecture != release.architecture
    {
        return Err(WrongPlatform);
    }
    if release.installer_abi != UPDATE_INSTALLER_ABI
        || release.guard_capability != UPDATE_GUARD_CAPABILITY
    {
        return Err(UnsupportedInstaller);
    }
    if installed.coordinator < release.minimum_coordinator {
        return Err(CoordinatorTooOld);
    }
    if !release.source_schema.contains(installed.schema) {
        return Err(IncompatibleSchema);
    }
    if !release.source_protocol.contains(installed.protocol) {
        return Err(IncompatibleProtocol);
    }
    let predecessor = installed
        .formal_predecessor
        .ok_or(UnqualifiedInstallation)?;
    if predecessor.installer_abi != UPDATE_INSTALLER_ABI
        || predecessor.guard_capability != UPDATE_GUARD_CAPABILITY
    {
        return Err(UnsupportedInstaller);
    }
    if !release.allowed_predecessors.contains(predecessor) {
        return Err(PredecessorMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaintenanceWindow {
    pub start: Minute,
    pub end: Minute,
}
impl MaintenanceWindow {
    /// Equal endpoints are disabled, not a hidden all-day maintenance window.
    pub fn contains(self, minute: Minute) -> bool {
        if self.start < self.end {
            self.start <= minute && minute < self.end
        } else if self.start > self.end {
            minute >= self.start || minute < self.end
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomaticInstallBlocker {
    AutomaticAfterDownloadDisabled,
    WorkerOwnersPending,
    UnknownActivity,
    UntrustedTime,
    OutsideMaintenanceWindow,
    CurrentLesson,
    NextLessonSoon,
    RecentInput,
    Audio,
    FilePreparation,
    WriterBusy,
    RecoveryPending,
    FullscreenApplication,
}

/// Physical ownership observations after requesting a normal shutdown. A
/// cancellation request or timeout is not evidence that an owner has returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateHandoffActivity {
    pub writer_busy: bool,
    pub recovery_pending: bool,
    pub worker_owners_returned: bool,
    pub audio_owners_returned: bool,
    pub file_owners_returned: bool,
}

/// For an already authenticated, verified and locally qualified update only.
/// Explicit immediate-restart consent bypasses classroom, foreground, input and
/// maintenance-window scheduling, but never bypasses physical resource return.
/// This prefilter neither authorizes Setup nor permits terminating an owner.
pub fn automatic_after_download_blocker(
    policy: &crate::update_state::UpdatePolicy,
    activity: Option<&UpdateHandoffActivity>,
) -> Option<AutomaticInstallBlocker> {
    use AutomaticInstallBlocker::*;
    if !policy.automatic_after_download_enabled() {
        return Some(AutomaticAfterDownloadDisabled);
    }
    let Some(activity) = activity else {
        return Some(UnknownActivity);
    };
    [
        (activity.writer_busy, WriterBusy),
        (activity.recovery_pending, RecoveryPending),
        (!activity.worker_owners_returned, WorkerOwnersPending),
        (!activity.audio_owners_returned, Audio),
        (!activity.file_owners_returned, FilePreparation),
    ]
    .into_iter()
    .find_map(|(blocked, reason)| blocked.then_some(reason))
}

/// The adapter must provide a coherent current snapshot and repeat this check
/// after the countdown. Unknown snapshots fail closed; no clock is modified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstallationActivity {
    pub time_trusted: bool,
    pub minute: Minute,
    pub current_lesson: bool,
    /// None means a known schedule has no next lesson, not an unknown schedule.
    pub seconds_until_next_lesson: Option<u64>,
    pub idle_seconds: u64,
    pub audio_active: bool,
    pub file_preparation: bool,
    pub writer_busy: bool,
    pub recovery_pending: bool,
    pub fullscreen_application: bool,
}
pub fn automatic_install_blocker(
    window: MaintenanceWindow,
    activity: Option<&InstallationActivity>,
) -> Option<AutomaticInstallBlocker> {
    use AutomaticInstallBlocker::*;
    let Some(activity) = activity else {
        return Some(UnknownActivity);
    };
    if !activity.time_trusted {
        return Some(UntrustedTime);
    }
    if !window.contains(activity.minute) {
        return Some(OutsideMaintenanceWindow);
    }
    if activity.current_lesson {
        return Some(CurrentLesson);
    }
    if activity
        .seconds_until_next_lesson
        .is_some_and(|seconds| seconds < MIN_NEXT_LESSON_SECONDS)
    {
        return Some(NextLessonSoon);
    }
    if activity.idle_seconds < MIN_UPDATE_IDLE_SECONDS {
        return Some(RecentInput);
    }
    [
        (activity.audio_active, Audio),
        (activity.file_preparation, FilePreparation),
        (activity.writer_busy, WriterBusy),
        (activity.recovery_pending, RecoveryPending),
        (activity.fullscreen_application, FullscreenApplication),
    ]
    .into_iter()
    .find_map(|(active, reason)| active.then_some(reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_restart_waits_for_actual_owners_but_has_no_lesson_or_idle_gate() {
        use crate::update_state::{UpdateInstallMode, UpdatePolicy};
        let mut policy = UpdatePolicy {
            group_id: Some([1; 16]),
            enabled: true,
            download: true,
            install_mode: UpdateInstallMode::AutomaticAfterDownload,
            ..Default::default()
        };
        let ready = UpdateHandoffActivity {
            writer_busy: false,
            recovery_pending: false,
            worker_owners_returned: true,
            audio_owners_returned: true,
            file_owners_returned: true,
        };
        assert_eq!(
            automatic_after_download_blocker(&policy, Some(&ready)),
            None
        );
        for (activity, expected) in [
            (
                UpdateHandoffActivity {
                    writer_busy: true,
                    ..ready
                },
                AutomaticInstallBlocker::WriterBusy,
            ),
            (
                UpdateHandoffActivity {
                    recovery_pending: true,
                    ..ready
                },
                AutomaticInstallBlocker::RecoveryPending,
            ),
            (
                UpdateHandoffActivity {
                    worker_owners_returned: false,
                    ..ready
                },
                AutomaticInstallBlocker::WorkerOwnersPending,
            ),
            (
                UpdateHandoffActivity {
                    audio_owners_returned: false,
                    ..ready
                },
                AutomaticInstallBlocker::Audio,
            ),
            (
                UpdateHandoffActivity {
                    file_owners_returned: false,
                    ..ready
                },
                AutomaticInstallBlocker::FilePreparation,
            ),
        ] {
            assert_eq!(
                automatic_after_download_blocker(&policy, Some(&activity)),
                Some(expected)
            );
        }
        assert_eq!(
            automatic_after_download_blocker(&policy, None),
            Some(AutomaticInstallBlocker::UnknownActivity)
        );
        policy.install_mode = UpdateInstallMode::MaintenanceWindow;
        assert_eq!(
            automatic_after_download_blocker(&policy, Some(&ready)),
            Some(AutomaticInstallBlocker::AutomaticAfterDownloadDisabled)
        );
    }

    fn identity(sequence: u64) -> ReleaseIdentity {
        ReleaseIdentity {
            sequence: ReleaseSequence::new(sequence).unwrap(),
            payload_sha256: [sequence as u8; 32],
        }
    }
    fn predecessor() -> ExactPredecessor {
        ExactPredecessor {
            product_code: [1; 16],
            package_code: [2; 16],
            source_commit: [3; 20],
            source_manifest_sha256: [4; 32],
            producer_sha256: [5; 32],
            cached_msi_sha256: [6; 32],
            payload_envelope_sha256: [7; 32],
            template_sha256: [8; 32],
            guard_sha256: [9; 32],
            logical_sha256: [10; 32],
            template_bytes: 1156,
            installer_abi: 2,
            guard_capability: 3,
        }
    }
    #[test]
    fn versions_reject_ambiguous_or_non_numeric_labels() {
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.-3",
            "1.2.3-beta",
            " 1.2.3",
            "1.2.4294967296",
        ] {
            assert_eq!(ReleaseVersion::parse(bad), Err(InvalidUpdateValue));
        }
        assert!(
            ReleaseVersion::parse("0.10.0").unwrap() > ReleaseVersion::parse("0.9.99").unwrap()
        );
        assert_eq!(ReleaseSequence::new(0), Err(InvalidUpdateValue));
    }
    #[test]
    fn exact_source_and_release_highwater_control_eligibility() {
        let predecessors = [predecessor()];
        let mut installed = InstalledUpdateContext {
            product: "future-classroom",
            channel: "stable",
            installed_release: identity(1),
            highest_accepted_release: Some(identity(2)),
            windows: WindowsBaseline::Windows11,
            architecture: UpdateArchitecture::X86,
            coordinator: ReleaseVersion::parse("0.2.0").unwrap(),
            schema: 3,
            protocol: 1,
            formal_predecessor: Some(&predecessors[0]),
        };
        let mut release = ReleaseCompatibility {
            product: "future-classroom",
            channel: "stable",
            identity: identity(2),
            minimum_windows: WindowsBaseline::Windows7Sp1,
            architecture: UpdateArchitecture::X86,
            installer_abi: 2,
            guard_capability: 3,
            minimum_coordinator: ReleaseVersion::parse("0.2.0").unwrap(),
            source_schema: CompatibilityRange::new(2, 3).unwrap(),
            source_protocol: CompatibilityRange::new(1, 1).unwrap(),
            allowed_predecessors: &predecessors,
        };
        assert_eq!(preflight_release(&release, &installed), Ok(()));
        release.identity.payload_sha256[0] ^= 1;
        assert_eq!(
            preflight_release(&release, &installed),
            Err(UpdateEligibilityFailure::ReleaseConflict)
        );
        release.identity = identity(1);
        assert_eq!(
            preflight_release(&release, &installed),
            Err(UpdateEligibilityFailure::ReleaseRollback)
        );
        release.identity = identity(2);
        installed.formal_predecessor = None;
        assert_eq!(
            preflight_release(&release, &installed),
            Err(UpdateEligibilityFailure::UnqualifiedInstallation)
        );
        let mut changed = predecessor();
        changed.cached_msi_sha256[0] ^= 1;
        installed.formal_predecessor = Some(&changed);
        assert_eq!(
            preflight_release(&release, &installed),
            Err(UpdateEligibilityFailure::PredecessorMismatch)
        );
        installed.formal_predecessor = Some(&predecessors[0]);
        release.architecture = UpdateArchitecture::X64;
        assert_eq!(
            preflight_release(&release, &installed),
            Err(UpdateEligibilityFailure::WrongPlatform)
        );
        release.architecture = UpdateArchitecture::X86;
        release.source_schema = CompatibilityRange::new(4, 5).unwrap();
        assert_eq!(
            preflight_release(&release, &installed),
            Err(UpdateEligibilityFailure::IncompatibleSchema)
        );
    }
    #[test]
    fn overnight_window_and_activity_thresholds_are_not_install_permission() {
        let minute = |value| Minute::new(value).unwrap();
        let window = MaintenanceWindow {
            start: minute(1200),
            end: minute(360),
        };
        assert!(window.contains(minute(1200)));
        assert!(window.contains(minute(0)));
        assert!(!window.contains(minute(360)));
        assert!(!MaintenanceWindow {
            start: minute(0),
            end: minute(0)
        }
        .contains(minute(0)));
        let mut activity = InstallationActivity {
            time_trusted: true,
            minute: minute(1200),
            current_lesson: false,
            seconds_until_next_lesson: Some(1800),
            idle_seconds: 300,
            audio_active: false,
            file_preparation: false,
            writer_busy: false,
            recovery_pending: false,
            fullscreen_application: false,
        };
        assert_eq!(automatic_install_blocker(window, Some(&activity)), None);
        activity.seconds_until_next_lesson = Some(1799);
        assert_eq!(
            automatic_install_blocker(window, Some(&activity)),
            Some(AutomaticInstallBlocker::NextLessonSoon)
        );
        activity.seconds_until_next_lesson = None;
        activity.idle_seconds = 299;
        assert_eq!(
            automatic_install_blocker(window, Some(&activity)),
            Some(AutomaticInstallBlocker::RecentInput)
        );
        activity.idle_seconds = 300;
        activity.audio_active = true;
        assert_eq!(
            automatic_install_blocker(window, Some(&activity)),
            Some(AutomaticInstallBlocker::Audio)
        );
        activity.time_trusted = false;
        assert_eq!(
            automatic_install_blocker(window, Some(&activity)),
            Some(AutomaticInstallBlocker::UntrustedTime)
        );
        assert_eq!(
            automatic_install_blocker(window, None),
            Some(AutomaticInstallBlocker::UnknownActivity)
        );
    }
}
