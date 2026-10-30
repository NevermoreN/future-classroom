//! Durable intent and factual state of this installation's current-user Run item.
//! These values never grant permission to perform a system write.
use crate::ids::{AuthEpoch, CommandId, PrincipalId, Revision};

pub const MAX_AUTOSTART_HISTORY: usize = 32;
pub const MAX_AUTOSTART_COMMAND_BYTES: usize = 1024;
pub const MAX_AUTOSTART_COMMAND_UTF16: usize = 259;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutostartFailure {
    NotObserved,
    AccessDenied,
    InvalidValue,
    PathUnavailable,
    ReadFailed,
    WriteFailed,
    ReadbackMismatch,
    PreimageChanged,
    Cancelled,
    Interrupted,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutostartObservation {
    Absent,
    Command(String),
    Opaque { kind: u32, bytes: u32 },
    Unavailable(AutostartFailure),
}
impl Default for AutostartObservation {
    fn default() -> Self {
        Self::Unavailable(AutostartFailure::NotObserved)
    }
}
impl AutostartObservation {
    pub fn validate(&self) -> bool {
        match self {
            Self::Command(c) => {
                c.len() <= MAX_AUTOSTART_COMMAND_BYTES
                    && c.encode_utf16().count() <= MAX_AUTOSTART_COMMAND_UTF16
                    && !c.contains('\0')
            }
            _ => true,
        }
    }
    pub fn mutation_eligible(&self) -> bool {
        self.validate() && matches!(self, Self::Absent | Self::Command(_))
    }
    pub fn matches(&self, desired: bool, target: &str) -> bool {
        match self {
            Self::Absent => !desired,
            Self::Command(c) => desired && c == target,
            _ => false,
        }
    }
    pub fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Command(c) => c.capacity(),
                _ => 0,
            }
    }
}
/// Target is produced by the trusted current-executable adapter, never by a form.
/// The conservative Run limit includes the terminating UTF-16 NUL.
pub fn valid_autostart_target(command: &str) -> bool {
    if command.len() > MAX_AUTOSTART_COMMAND_BYTES
        || command.encode_utf16().count() > MAX_AUTOSTART_COMMAND_UTF16
        || command.chars().any(char::is_control)
    {
        return false;
    }
    let Some(path) = command
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix("\" --autostart"))
    else {
        return false;
    };
    let b = path.as_bytes();
    if b.len() < 8
        || !b[0].is_ascii_alphabetic()
        || b[1..3] != *b":\\"
        || !path.to_ascii_lowercase().ends_with(".exe")
        || path[3..].contains(['"', '/', ':', '*', '?', '<', '>', '|'])
    {
        return false;
    }
    path[3..]
        .split('\\')
        .all(|part| !part.is_empty() && part != "." && part != ".." && !part.ends_with([' ', '.']))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutostartPhase {
    Prepared,
    Applying,
    Succeeded,
    Failed,
    NeedsReconcile,
}
impl AutostartPhase {
    pub fn pending(self) -> bool {
        matches!(self, Self::Prepared | Self::Applying)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutostartTicket {
    pub command_id: CommandId,
    pub sequence: u64,
    pub prepared_revision: Revision,
    pub principal_id: PrincipalId,
    pub auth_epoch: AuthEpoch,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutostartOperation {
    pub ticket: AutostartTicket,
    pub desired: bool,
    pub target_command: String,
    pub previous_observed: AutostartObservation,
    pub phase: AutostartPhase,
    pub observed: Option<AutostartObservation>,
    pub failure: Option<AutostartFailure>,
    pub mutated: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutostartState {
    pub revision: Revision,
    pub sequence: u64,
    pub desired: bool,
    pub target_command: Option<String>,
    pub observed: AutostartObservation,
    pub history: Vec<AutostartOperation>,
}
impl AutostartState {
    pub fn current(&self) -> Option<&AutostartOperation> {
        self.history.last()
    }
    pub fn drifted(&self) -> bool {
        self.target_command
            .as_ref()
            .is_some_and(|target| !self.observed.matches(self.desired, target))
    }
    pub fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.target_command.as_ref().map_or(0, String::capacity)
            + self.observed.allocation_bytes()
            + self.history.capacity() * std::mem::size_of::<AutostartOperation>()
            + self
                .history
                .iter()
                .map(|op| {
                    op.target_command.capacity()
                        + op.previous_observed.allocation_bytes()
                        + op.observed
                            .as_ref()
                            .map_or(0, AutostartObservation::allocation_bytes)
                })
                .sum::<usize>()
    }
    pub fn validate(&self) -> bool {
        if !self.observed.validate()
            || self.history.len() > MAX_AUTOSTART_HISTORY
            || self
                .target_command
                .as_ref()
                .is_some_and(|c| !valid_autostart_target(c))
        {
            return false;
        }
        if self.history.is_empty() {
            return self.sequence == 0 && !self.desired && self.target_command.is_none();
        }
        if self.sequence.min(MAX_AUTOSTART_HISTORY as u64) != self.history.len() as u64 {
            return false;
        }
        let mut previous_sequence = self.sequence - self.history.len() as u64;
        let mut previous_revision = 0;
        let mut commands = std::collections::BTreeSet::new();
        for (index, op) in self.history.iter().enumerate() {
            if !commands.insert(op.ticket.command_id)
                || op.ticket.sequence == 0
                || previous_sequence.checked_add(1) != Some(op.ticket.sequence)
                || op.ticket.sequence > self.sequence
                || op.ticket.prepared_revision.get() <= previous_revision
                || op.ticket.prepared_revision > self.revision
                || !valid_autostart_target(&op.target_command)
                || !op.previous_observed.mutation_eligible()
                || op.observed.as_ref().is_some_and(|o| !o.validate())
                || (op.phase.pending() && index + 1 != self.history.len())
            {
                return false;
            }
            let valid_phase = match op.phase {
                AutostartPhase::Prepared | AutostartPhase::Applying => {
                    op.observed.is_none() && op.failure.is_none() && !op.mutated
                }
                AutostartPhase::Succeeded => {
                    op.failure.is_none()
                        && op
                            .observed
                            .as_ref()
                            .is_some_and(|o| o.matches(op.desired, &op.target_command))
                }
                AutostartPhase::Failed => op.failure.is_some() && op.observed.is_some(),
                AutostartPhase::NeedsReconcile => op.failure.is_some() && op.observed.is_some(),
            };
            if !valid_phase {
                return false;
            }
            previous_sequence = op.ticket.sequence;
            previous_revision = op.ticket.prepared_revision.get();
        }
        let last = self.history.last().unwrap();
        last.ticket.sequence == self.sequence
            && self.desired == last.desired
            && self.target_command.as_ref() == Some(&last.target_command)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutostartSystemOutcome {
    pub observation: AutostartObservation,
    pub failure: Option<AutostartFailure>,
    pub mutated: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_is_a_fixed_quoted_local_executable() {
        assert!(valid_autostart_target(
            "\"C:\\教室 空间\\FutureClassroom.exe\" --autostart"
        ));
        for s in [
            "C:\\x.exe --autostart",
            "\"\\\\server\\x.exe\" --autostart",
            "\"C:\\..\\x.exe\" --autostart",
            "\"C:\\x.exe\" --autostart --unlock",
            "\"C:\\x.exe:bad\" --autostart",
            "\"C:\\x.exe\" --autostart\n",
            "\"C:\\x .\\x.exe\" --autostart",
        ] {
            assert!(!valid_autostart_target(s), "{s}");
        }
    }
    #[test]
    fn target_counts_utf16_and_bytes_without_truncating() {
        let s = format!("\"C:\\{}.exe\" --autostart", "中".repeat(240));
        assert!(!valid_autostart_target(&s));
        assert!(!AutostartObservation::Command("x".repeat(260)).validate());
        assert!(!AutostartObservation::Command("a\0b".into()).mutation_eligible());
    }
    #[test]
    fn opaque_and_unavailable_never_count_as_disabled_or_writable() {
        for o in [
            AutostartObservation::Opaque { kind: 3, bytes: 4 },
            AutostartObservation::default(),
        ] {
            assert!(!o.matches(false, ""));
            assert!(!o.mutation_eligible());
        }
        assert!(AutostartState::default().validate());
    }
}
