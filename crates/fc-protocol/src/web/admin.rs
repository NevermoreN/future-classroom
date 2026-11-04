//! Closed, typed administrator API. No database blobs or credentials in reads.
use crate::{ProtocolError, SCHEMA_VERSION};
use fc_domain::ids::Revision;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
pub mod account_security;
pub mod display;
pub mod identity;
pub mod images;
pub mod layout;
pub mod platform;
pub mod preview;
pub mod profile_content;
pub mod profiles;
pub mod schedule;
pub mod voice;
pub const MAX_ADMIN_JSON_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Status,
    Identity,
    AccountSecurity,
    Display,
    Schedule,
    Layout,
    Network,
    Autostart,
    Voice,
    PreviewSource,
    Preview {
        scope: fc_domain::layout::LayoutScope,
    },
    ImageManifest {
        slot: fc_domain::profile_content::ContentSlotKey,
    },
    ImageBytes {
        slot: fc_domain::profile_content::ContentSlotKey,
        batch: fc_domain::ids::CommandId,
        upload: fc_domain::ids::MediaVersionId,
    },
    ImageCancel {
        slot: fc_domain::profile_content::ContentSlotKey,
        batch: fc_domain::ids::CommandId,
    },
    ImageMedia {
        slot: fc_domain::profile_content::ContentSlotKey,
        media: fc_domain::ids::MediaVersionId,
    },
    ProfileContent {
        slot: fc_domain::profile_content::ContentSlotKey,
    },
    Profiles {
        after: Option<fc_domain::ids::ProfileId>,
    },
    Profile {
        key: fc_domain::profile::SubjectProfileKey,
    },
}
impl Resource {
    pub fn path(path: &str) -> Option<Self> {
        if path == "preview/source" {
            return Some(Self::PreviewSource);
        }
        if let Some(path) = path.strip_prefix("preview/") {
            if path == "public" {
                return Some(Self::Preview {
                    scope: fc_domain::layout::LayoutScope::Public,
                });
            }
            let slot = profile_content::slot(&format!("{path}/1"))?;
            return Some(Self::Preview { scope: slot.scope });
        }

        if let Some(path) = path.strip_prefix("content/") {
            if let Some((slot, tail)) = path.split_once("/media/") {
                return Some(Self::ImageMedia {
                    slot: profile_content::slot(slot)?,
                    media: fc_domain::ids::MediaVersionId::new(id(tail).ok()?).ok()?,
                });
            }
            if let Some(slot) = path.strip_suffix("/uploads") {
                return Some(Self::ImageManifest {
                    slot: profile_content::slot(slot)?,
                });
            }
            if let Some((slot, tail)) = path.split_once("/uploads/") {
                let slot = profile_content::slot(slot)?;
                let parts = tail.split('/').collect::<Vec<_>>();
                match parts.as_slice() {
                    [batch, "cancel"] => {
                        return Some(Self::ImageCancel {
                            slot,
                            batch: fc_domain::ids::CommandId::new(id(batch).ok()?).ok()?,
                        })
                    }
                    [batch, upload, "bytes"] => {
                        return Some(Self::ImageBytes {
                            slot,
                            batch: fc_domain::ids::CommandId::new(id(batch).ok()?).ok()?,
                            upload: fc_domain::ids::MediaVersionId::new(id(upload).ok()?).ok()?,
                        })
                    }
                    _ => return None,
                }
            }
            return Some(Self::ProfileContent {
                slot: profile_content::slot(path)?,
            });
        }
        if let Some(after) = path.strip_prefix("profiles/") {
            let after = revision(after).ok()?.get();
            return Some(Self::Profiles {
                after: if after == 0 {
                    None
                } else {
                    Some(fc_domain::ids::ProfileId::new(after).ok()?)
                },
            });
        }
        if let Some(path) = path.strip_prefix("profile/") {
            let ids = path.split('/').collect::<Vec<_>>();
            if ids.len() != 3 {
                return None;
            }
            return Some(Self::Profile {
                key: fc_domain::profile::SubjectProfileKey {
                    class_id: fc_domain::ids::ClassId::new(id(ids[0]).ok()?).ok()?,
                    subject_id: fc_domain::ids::SubjectId::new(id(ids[1]).ok()?).ok()?,
                    teacher_id: fc_domain::ids::PrincipalId::new(id(ids[2]).ok()?).ok()?,
                },
            });
        }
        Some(match path {
            "status" => Self::Status,
            "identity" => Self::Identity,
            "account-security" => Self::AccountSecurity,
            "display" => Self::Display,
            "schedule" => Self::Schedule,
            "layout" => Self::Layout,
            "network" => Self::Network,
            "autostart" => Self::Autostart,
            "voice" => Self::Voice,
            _ => return None,
        })
    }
}
pub fn revision(value: &str) -> Result<Revision, ProtocolError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(ProtocolError::InvalidCommand);
    }
    value
        .parse::<u64>()
        .map(Revision::new)
        .map_err(|_| ProtocolError::InvalidCommand)
}
pub fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ProtocolError> {
    if bytes.len() > MAX_ADMIN_JSON_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    use serde::de::DeserializeSeed;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    crate::Bounded(0)
        .deserialize(&mut deserializer)
        .map_err(|_| ProtocolError::InvalidJson)?;
    deserializer.end().map_err(|_| ProtocolError::InvalidJson)?;
    serde_json::from_slice(bytes).map_err(|_| ProtocolError::InvalidJson)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Mutation {
    pub schema_version: u16,
    pub expected_revision: String,
    pub action: String,
    pub candidate: serde_json::Value,
}
impl Mutation {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let value: Self = parse(bytes)?;
        if value.schema_version != SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedSchema);
        }
        revision(&value.expected_revision)?;
        Ok(value)
    }
    pub fn candidate<T: DeserializeOwned>(self) -> Result<(Revision, T), ProtocolError> {
        Ok((
            revision(&self.expected_revision)?,
            serde_json::from_value(self.candidate).map_err(|_| ProtocolError::InvalidCommand)?,
        ))
    }
}
pub fn encode<T: Serialize>(revision: Revision, data: &T) -> Result<Vec<u8>, ProtocolError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope<'a, T> {
        schema_version: u16,
        revision: String,
        data: &'a T,
    }
    struct Writer(Vec<u8>);
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_ADMIN_JSON_BYTES - self.0.len() {
                return Err(std::io::Error::other("admin JSON limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Writer(Vec::new());
    serde_json::to_writer(
        &mut output,
        &Envelope {
            schema_version: SCHEMA_VERSION,
            revision: revision.get().to_string(),
            data,
        },
    )
    .map_err(|_| ProtocolError::TooLarge)?;
    Ok(output.0)
}
pub fn id(value: &str) -> Result<u64, ProtocolError> {
    let id = revision(value)?.get();
    if id == 0 {
        Err(ProtocolError::InvalidCommand)
    } else {
        Ok(id)
    }
}
pub fn date(value: &str) -> Result<fc_domain::time::LocalDate, ProtocolError> {
    if value.len() != 10 || value.as_bytes()[4] != b'-' || value.as_bytes()[7] != b'-' {
        return Err(ProtocolError::InvalidCommand);
    }
    let fields = value.split('-').collect::<Vec<_>>();
    fc_domain::time::LocalDate::new(
        fields[0]
            .parse()
            .map_err(|_| ProtocolError::InvalidCommand)?,
        fields[1]
            .parse()
            .map_err(|_| ProtocolError::InvalidCommand)?,
        fields[2]
            .parse()
            .map_err(|_| ProtocolError::InvalidCommand)?,
    )
    .map_err(|_| ProtocolError::InvalidCommand)
}
pub fn date_text(date: fc_domain::time::LocalDate) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}
