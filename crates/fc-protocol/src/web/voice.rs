//! Explicit bounded voice disclosures; no actor proof, path, audio or generation
//! of an individual native job enters browser JSON.
use crate::{ProtocolError, SCHEMA_VERSION};
use fc_domain::{ids::*, time::*, voice::*};
use serde::Serialize;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentClass<'a> {
    class_id: String,
    class_name: &'a str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Capabilities<'a> {
    schema_version: u16,
    status: &'static str,
    identity_revision: String,
    runtime_generation: String,
    server_now: String,
    current_class: Option<CurrentClass<'a>>,
    enabled: bool,
    admission: &'static str,
    target_authorized: bool,
    device_availability: DeviceAvailability,
    default_repeat_total: u8,
    min_repeat_total: u8,
    max_repeat_total: u8,
    min_duration_millis: u32,
    max_duration_millis: u32,
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u8,
    max_wav_bytes: u64,
    upload_ttl_seconds: u16,
}
/// A recent negative format observation is not a current-device reservation.
/// There is deliberately no positive/audibility claim in this response.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceAvailability {
    Unverified,
    Unavailable,
}
pub struct CapabilityPolicy {
    pub enabled: bool,
    pub admitting: bool,
    pub target_authorized: bool,
    pub device_availability: DeviceAvailability,
    pub limits: VoiceLimits,
}
pub fn capabilities(
    identity: Revision,
    runtime_generation: u64,
    now: LocalDateTime,
    class: Option<(ClassId, &str)>,
    policy: CapabilityPolicy,
) -> Result<Vec<u8>, ProtocolError> {
    policy
        .limits
        .validate()
        .map_err(|_| ProtocolError::InvalidCommand)?;
    super::responses::encode(&Capabilities {
        schema_version: SCHEMA_VERSION,
        status: "VOICE_CAPABILITIES",
        identity_revision: identity.get().to_string(),
        runtime_generation: runtime_generation.to_string(),
        server_now: super::content::datetime(now),
        current_class: class.map(|(class_id, class_name)| CurrentClass {
            class_id: class_id.get().to_string(),
            class_name,
        }),
        enabled: policy.enabled,
        admission: if policy.admitting { "OPEN" } else { "CLOSED" },
        target_authorized: policy.target_authorized,
        device_availability: policy.device_availability,
        default_repeat_total: policy.limits.default_repeat_total(),
        min_repeat_total: 1,
        max_repeat_total: policy.limits.max_repeat_total,
        min_duration_millis: 500,
        max_duration_millis: policy.limits.max_duration_millis,
        sample_rate: 16000,
        channels: 1,
        bits_per_sample: 16,
        max_wav_bytes: policy.limits.max_wav_bytes(),
        upload_ttl_seconds: UPLOAD_TTL_SECONDS as u16,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    job_id: String,
    command_id: String,
    class_id: String,
    repeat_total: u8,
    completed_repeats: u8,
    current_repeat: u8,
    state: &'static str,
    revision: String,
    failure: Option<&'static str>,
}
fn job(j: &VoiceJob) -> Result<Job, ProtocolError> {
    if j.id.get() != j.command_id.get()
        || j.revision.get() == 0
        || !(1..=10).contains(&j.repeat_total)
        || j.completed_repeats > j.repeat_total
        || j.current_repeat > j.repeat_total
    {
        return Err(ProtocolError::InvalidCommand);
    }
    Ok(Job {
        job_id: j.id.get().to_string(),
        command_id: j.command_id.get().to_string(),
        class_id: j.class_id.get().to_string(),
        repeat_total: j.repeat_total,
        completed_repeats: j.completed_repeats,
        current_repeat: j.current_repeat,
        state: match j.state {
            VoiceState::Queued => "QUEUED",
            VoiceState::Starting => "STARTING",
            VoiceState::Playing => "PLAYING",
            VoiceState::Gap => "GAP",
            VoiceState::Stopping => "STOPPING",
            VoiceState::Completed => "COMPLETED",
            VoiceState::Canceled => "CANCELED",
            VoiceState::Failed => "FAILED",
            VoiceState::Expired => "EXPIRED",
            VoiceState::Interrupted => "INTERRUPTED",
        },
        revision: j.revision.get().to_string(),
        failure: j.failure.map(|f| match f {
            VoiceFailure::Device => "DEVICE",
            VoiceFailure::Session => "SESSION",
            VoiceFailure::Revoked => "REVOKED",
            VoiceFailure::Disabled => "DISABLED",
            VoiceFailure::Timeout => "TIMEOUT",
            VoiceFailure::ClockRegression => "CLOCK_REGRESSION",
            VoiceFailure::Storage => "STORAGE",
        }),
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JobResponse {
    schema_version: u16,
    status: &'static str,
    job: Job,
}
pub fn status(j: &VoiceJob) -> Result<Vec<u8>, ProtocolError> {
    super::responses::encode(&JobResponse {
        schema_version: 1,
        status: "VOICE_JOB",
        job: job(j)?,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Mutation {
    schema_version: u16,
    status: &'static str,
    command_id: String,
    job: Job,
}
pub fn committed(j: &VoiceJob) -> Result<Vec<u8>, ProtocolError> {
    super::responses::encode(&Mutation {
        schema_version: 1,
        status: "COMMITTED",
        command_id: j.command_id.get().to_string(),
        job: job(j)?,
    })
}
pub fn cancelled(command_id: CommandId, j: &VoiceJob) -> Result<Vec<u8>, ProtocolError> {
    super::responses::encode(&Mutation {
        schema_version: 1,
        status: "CANCEL_REQUESTED",
        command_id: command_id.get().to_string(),
        job: job(j)?,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Jobs {
    schema_version: u16,
    status: &'static str,
    jobs: Vec<Job>,
}
pub fn jobs(items: &[VoiceJob]) -> Result<Vec<u8>, ProtocolError> {
    if items.len() > 24
        || items.iter().filter(|j| !j.state.terminal()).count() > 4
        || items.iter().filter(|j| j.state.terminal()).count() > 20
    {
        return Err(ProtocolError::TooLarge);
    }
    super::responses::encode(&Jobs {
        schema_version: 1,
        status: "VOICE_JOBS",
        jobs: items.iter().map(job).collect::<Result<_, _>>()?,
    })
}
/// Gregorian inverse of the domain's private civil epoch. Never use browser
/// timezone conversion or pretend these local persisted seconds are UTC.
pub fn civil_datetime(seconds: i64) -> Result<LocalDateTime, ProtocolError> {
    let day = seconds.div_euclid(86400);
    let second = seconds.rem_euclid(86400) as u32;
    let first = LocalDate::new(1, 1, 1).map_err(|_| ProtocolError::InvalidCommand)?;
    let last = LocalDate::new(9999, 12, 31).map_err(|_| ProtocolError::InvalidCommand)?;
    if day < first.day_number() || day > last.day_number() {
        return Err(ProtocolError::InvalidCommand);
    }
    let (mut low, mut high) = (1, 9999);
    while low < high {
        let mid = (low + high + 1) / 2;
        if LocalDate::new(mid, 1, 1)
            .map_err(|_| ProtocolError::InvalidCommand)?
            .day_number()
            <= day
        {
            low = mid
        } else {
            high = mid - 1
        }
    }
    for month in (1..=12).rev() {
        let start = LocalDate::new(low, month, 1).map_err(|_| ProtocolError::InvalidCommand)?;
        if start.day_number() <= day {
            return LocalDateTime::new(
                LocalDate::new(low, month, (day - start.day_number() + 1) as u8)
                    .map_err(|_| ProtocolError::InvalidCommand)?,
                second,
            )
            .map_err(|_| ProtocolError::InvalidCommand);
        }
    }
    Err(ProtocolError::InvalidCommand)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Uploaded {
    schema_version: u16,
    status: &'static str,
    asset_id: String,
    asset_revision: String,
    class_id: String,
    bytes: u64,
    duration_millis: u64,
    expires_at: String,
}
pub fn uploaded(asset: &VoiceAsset) -> Result<Vec<u8>, ProtocolError> {
    let pcm = asset.pcm.as_ref().ok_or(ProtocolError::InvalidCommand)?;
    pcm.validate().map_err(|_| ProtocolError::InvalidCommand)?;
    if asset.state != VoiceAssetState::Ready || asset.revision.get() == 0 {
        return Err(ProtocolError::InvalidCommand);
    }
    super::responses::encode(&Uploaded {
        schema_version: 1,
        status: "VOICE_UPLOADED",
        asset_id: asset.id.get().to_string(),
        asset_revision: asset.revision.get().to_string(),
        class_id: asset.class_id.get().to_string(),
        bytes: pcm.byte_length,
        duration_millis: pcm.duration_millis(),
        expires_at: super::content::datetime(civil_datetime(asset.expires_at)?),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_capabilities_publish_consistent_tighter_bounds_and_reject_invalid_policy() {
        let now = LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 36000).unwrap();
        let limits = VoiceLimits {
            max_duration_millis: 1250,
            max_pending_jobs: 3,
            max_pending_per_owner: 1,
            max_repeat_total: 2,
        };
        let bytes = capabilities(
            Revision::new(2),
            7,
            now,
            None,
            CapabilityPolicy {
                enabled: true,
                admitting: false,
                target_authorized: false,
                device_availability: DeviceAvailability::Unverified,
                limits,
            },
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["maxDurationMillis"], 1250);
        assert_eq!(value["maxWavBytes"], 40044);
        assert_eq!(value["maxRepeatTotal"], 2);
        assert_eq!(value["defaultRepeatTotal"], 2);
        assert_eq!(value["deviceAvailability"], "UNVERIFIED");
        assert!(capabilities(
            Revision::new(2),
            7,
            now,
            None,
            CapabilityPolicy {
                enabled: true,
                admitting: true,
                target_authorized: false,
                device_availability: DeviceAvailability::Unverified,
                limits: VoiceLimits {
                    max_pending_per_owner: 4,
                    ..limits
                }
            }
        )
        .is_err());
    }
    #[test]
    fn civil_roundtrip_boundaries_and_no_device_claim() {
        for (year, month, day) in [(1, 1, 1), (2000, 2, 29), (2026, 12, 31), (9999, 12, 31)] {
            let now = LocalDateTime::new(LocalDate::new(year, month, day).unwrap(), 86399).unwrap();
            assert_eq!(civil_datetime(civil_seconds(now)).unwrap(), now);
        }
        let now = LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), 36000).unwrap();
        let b = capabilities(
            Revision::new(1),
            2,
            now,
            None,
            CapabilityPolicy {
                enabled: false,
                admitting: false,
                target_authorized: false,
                device_availability: DeviceAvailability::Unverified,
                limits: VoiceLimits::default(),
            },
        )
        .unwrap();
        let s = std::str::from_utf8(&b).unwrap();
        assert!(s.contains("\"deviceAvailability\":\"UNVERIFIED\""));
        assert!(s.contains("\"currentClass\":null"));
        assert!(s.contains("\"runtimeGeneration\":\"2\""));
        assert!(!s.contains("owner"));
    }
}
