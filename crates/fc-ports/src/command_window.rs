//! Server-issued command identities. Parsing a ticket is not permission to run
//! a command: the sole Writer must find its live issuance and exact binding.
use fc_domain::{
    auth::{ActorContext, TrustedOrigin},
    ids::{AuthEpoch, CommandId, PrincipalId, SchoolId},
};
use std::fmt;

pub const COMMAND_WINDOW_MILLIS: u64 = 24 * 60 * 60 * 1000;
pub const MAX_WINDOW_COMMANDS: u64 = 65_536;
pub const MAX_WINDOW_BYTES: u64 = 64 * 1024 * 1024;

/// A definite refusal before business execution/output disclosure. These are
/// distinct from an unknown COMMIT or response-delivery failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandWindowError {
    Expired,
    NotIssued,
    Capacity,
    Clock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCommandTicket;

/// Separate from the historic u64 payload id. Neither legacy u64::MAX nor a
/// retired legacy id can collide with this explicitly versioned namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandTicket {
    epoch: [u8; 16],
    serial: u64,
}
impl CommandTicket {
    pub fn new(epoch: [u8; 16], serial: u64) -> Result<Self, InvalidCommandTicket> {
        if epoch == [0; 16] || serial == 0 || serial > i64::MAX as u64 {
            return Err(InvalidCommandTicket);
        }
        Ok(Self { epoch, serial })
    }
    pub fn parse(text: &str) -> Result<Self, InvalidCommandTicket> {
        // Bound before splitting or parsing untrusted text.
        if !(37..=55).contains(&text.len()) || !text.starts_with("c2:") {
            return Err(InvalidCommandTicket);
        }
        let (hex, serial) = text[3..].split_once(':').ok_or(InvalidCommandTicket)?;
        if hex.len() != 32 || serial.starts_with('0') || !serial.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(InvalidCommandTicket);
        }
        let mut epoch = [0; 16];
        for (pair, byte) in hex.as_bytes().as_chunks::<2>().0.iter().zip(&mut epoch) {
            let nibble = |c| match c {
                b'0'..=b'9' => Ok(c - b'0'),
                b'a'..=b'f' => Ok(c - b'a' + 10),
                _ => Err(InvalidCommandTicket),
            };
            *byte = nibble(pair[0])? * 16 + nibble(pair[1])?;
        }
        Self::new(epoch, serial.parse().map_err(|_| InvalidCommandTicket)?)
    }
    pub const fn epoch(self) -> [u8; 16] {
        self.epoch
    }
    pub const fn serial(self) -> u64 {
        self.serial
    }
}
impl fmt::Display for CommandTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("c2:")?;
        for byte in self.epoch {
            write!(f, "{byte:02x}")?;
        }
        write!(f, ":{}", self.serial)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandFamily {
    Generic,
    Content,
    Identity,
    Schedule,
    Display,
    SubjectVisual,
    Lesson,
    DesktopLesson,
    Calendar,
    HttpsSettings,
    Profiles,
    ProfileContent,
    Materials,
    Voice,
    Autostart,
    Updates,
}
impl CommandFamily {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Content => "content",
            Self::Identity => "identity",
            Self::Schedule => "schedule",
            Self::Display => "display",
            Self::SubjectVisual => "subject-visual",
            Self::Lesson => "lesson",
            Self::DesktopLesson => "desktop-lesson",
            Self::Calendar => "calendar",
            Self::HttpsSettings => "https-settings",
            Self::Profiles => "profiles",
            Self::ProfileContent => "profile-content",
            Self::Materials => "materials",
            Self::Voice => "voice",
            Self::Autostart => "autostart",
            Self::Updates => "updates",
        }
    }
}

/// Transport authentication is still required on every submission/query. This
/// records which actor received the id; it never replaces a current lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandOwner {
    PublicDesktop,
    Principal {
        principal: PrincipalId,
        school: SchoolId,
        epoch: AuthEpoch,
        origin: TrustedOrigin,
    },
}
impl CommandOwner {
    pub fn authenticated(actor: &ActorContext) -> Self {
        Self::Principal {
            principal: actor.principal_id,
            school: actor.school_id,
            epoch: actor.auth_epoch,
            origin: actor.origin,
        }
    }
    pub fn binding(self) -> [u8; 26] {
        let mut bytes = [0; 26];
        if let Self::Principal {
            principal,
            school,
            epoch,
            origin,
        } = self
        {
            bytes[0] = 1;
            bytes[1..9].copy_from_slice(&principal.get().to_be_bytes());
            bytes[9..17].copy_from_slice(&school.get().to_be_bytes());
            bytes[17..25].copy_from_slice(&epoch.get().to_be_bytes());
            bytes[25] = match origin {
                TrustedOrigin::Local => 1,
                TrustedOrigin::Web => 2,
            };
        }
        bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IssuedCommand {
    pub ticket: CommandTicket,
    pub command_id: CommandId,
    pub family: CommandFamily,
    pub owner: CommandOwner,
    /// Writer logical clock. Exposed only as a duration limit, not a client wall-clock permission.
    pub window_millis: u64,
}

/// Closed original request data. These values carry no execution capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoicePolicyIntent {
    Enabled {
        enabled: bool,
        expected_voice: fc_domain::ids::Revision,
        expected_identity: fc_domain::ids::Revision,
        expected_runtime: u64,
    },
    Limits {
        limits: fc_domain::voice::VoiceLimits,
        expected_voice: fc_domain::ids::Revision,
        expected_identity: fc_domain::ids::Revision,
        expected_runtime: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedCommandIntent {
    VoicePolicy(VoicePolicyIntent),
    CalendarPolicy {
        enabled: bool,
        expected_policy: fc_domain::ids::Revision,
    },
    CalendarManualFetch {
        year: i32,
    },
    RestoreBarrier {
        expected_global: fc_domain::ids::Revision,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalendarManualResultKind {
    Downloaded,
    NotModified,
    Failed(crate::calendar::CalendarFetchFailure),
    RateLimited {
        next_allowed: fc_domain::time::LocalDateTime,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarManualResult {
    pub year: i32,
    pub attempt_sequence: Option<u64>,
    pub candidate_revision: fc_domain::ids::Revision,
    pub kind: CalendarManualResultKind,
    pub committed_revision: fc_domain::ids::Revision,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedCommandResult {
    VoicePolicy(fc_domain::voice::VoiceSnapshot),
    CalendarPolicy(crate::calendar::CalendarAutoPolicy),
    CalendarManualFetch(CalendarManualResult),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedCommandFailure {
    Denied,
    Conflict,
    Stale,
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedCommandOutcome {
    Pending,
    Completed(TypedCommandResult),
    Failed(TypedCommandFailure),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tickets_have_one_canonical_versioned_form() {
        let ticket = CommandTicket::new([0xab; 16], 1).unwrap();
        assert_eq!(CommandTicket::parse(&ticket.to_string()), Ok(ticket));
        for value in [
            "1",
            "c2:00000000000000000000000000000000:1",
            "c2:ABABABABABABABABABABABABABABABAB:1",
            "c2:abababababababababababababababab:01",
            "c2:abababababababababababababababab:0",
            "c2:abababababababababababababababab:18446744073709551615",
            "c2:abababababababababababababababab:1:2",
        ] {
            assert!(CommandTicket::parse(value).is_err(), "{value}");
        }
    }
}
