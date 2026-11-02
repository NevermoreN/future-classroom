//! Exact receipt/result catalog shared by namespacing, bounded retirement and
//! backup filtering. Other business documents are not command-history garbage.
use fc_domain::ids::CommandId;
use fc_ports::command_window::{CommandFamily as Family, CommandTicket};

#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) prefix: &'static str,
    pub(crate) family: Family,
    pub(crate) padded: bool,
    pub(crate) counted: bool,
}
const SLOTS: &[Slot] = &[
    Slot {
        prefix: "business/receipt/content/",
        family: Family::Content,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/receipt/identity/",
        family: Family::Identity,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/receipt/schedule/",
        family: Family::Schedule,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/result/schedule/",
        family: Family::Schedule,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/receipt/display/",
        family: Family::Display,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/result/display/",
        family: Family::Display,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/receipt/subject-visual/",
        family: Family::SubjectVisual,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/result/subject-visual/",
        family: Family::SubjectVisual,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/receipt/lesson/",
        family: Family::Lesson,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/receipt/desktop-lesson/",
        family: Family::DesktopLesson,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/receipt/calendar/",
        family: Family::Calendar,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/result/calendar/",
        family: Family::Calendar,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/receipt/https-settings/",
        family: Family::HttpsSettings,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/result/https-settings/",
        family: Family::HttpsSettings,
        padded: false,
        counted: false,
    },
    Slot {
        prefix: "business/profiles/v1/receipts/",
        family: Family::Profiles,
        padded: true,
        counted: false,
    },
    Slot {
        prefix: "business/profiles/v1/results/",
        family: Family::Profiles,
        padded: true,
        counted: false,
    },
    Slot {
        prefix: "business/profile-content/v1/receipts/",
        family: Family::ProfileContent,
        padded: true,
        counted: true,
    },
    Slot {
        prefix: "business/profile-content/v1/page-receipts/",
        family: Family::ProfileContent,
        padded: true,
        counted: true,
    },
    Slot {
        prefix: "business/material/v1/receipts/",
        family: Family::Materials,
        padded: true,
        counted: false,
    },
    Slot {
        prefix: "business/material/v1/web-outcomes/",
        family: Family::Materials,
        padded: true,
        counted: false,
    },
    Slot {
        prefix: "business/voice-receipt/v1/",
        family: Family::Voice,
        padded: true,
        counted: false,
    },
];
pub(crate) fn slots() -> &'static [Slot] {
    SLOTS
}
pub(crate) fn legacy(key: &str) -> Option<(Slot, CommandId)> {
    for slot in SLOTS {
        if let Some(suffix) = key.strip_prefix(slot.prefix) {
            if suffix.is_empty() || suffix.len() > 20 || !suffix.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let id = CommandId::new(suffix.parse().ok()?).ok()?;
            let canonical = if slot.padded {
                format!("{:020}", id.get())
            } else {
                id.get().to_string()
            };
            return (canonical == suffix).then_some((*slot, id));
        }
    }
    None
}
pub(crate) fn namespaced(slot: Slot, ticket: CommandTicket) -> String {
    format!(
        "{}v2/{}/{}",
        slot.prefix,
        hex(ticket.epoch()),
        ticket.serial()
    )
}
fn hex(epoch: [u8; 16]) -> String {
    epoch.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn owned(key: &str) -> Option<(Slot, CommandTicket)> {
    for slot in SLOTS {
        if let Some(suffix) = key
            .strip_prefix(slot.prefix)
            .and_then(|s| s.strip_prefix("v2/"))
        {
            let (epoch, serial) = suffix.split_once('/')?;
            let ticket = CommandTicket::parse(&format!("c2:{epoch}:{serial}")).ok()?;
            return Some((*slot, ticket));
        }
    }
    None
}
