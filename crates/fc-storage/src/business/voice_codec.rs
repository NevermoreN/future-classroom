//! Strict, bounded DTO codec. Domain intentionally has no serde dependency.
use super::{codec::*, *};
use fc_domain::voice::*;
pub(super) fn state(v: VoiceState) -> &'static str {
    match v {
        VoiceState::Queued => "queued",
        VoiceState::Starting => "starting",
        VoiceState::Playing => "playing",
        VoiceState::Gap => "gap",
        VoiceState::Stopping => "stopping",
        VoiceState::Completed => "completed",
        VoiceState::Canceled => "canceled",
        VoiceState::Failed => "failed",
        VoiceState::Expired => "expired",
        VoiceState::Interrupted => "interrupted",
    }
}
fn read_state(v: &Value) -> Result<VoiceState, StorageError> {
    Ok(match v.as_str() {
        Some("queued") => VoiceState::Queued,
        Some("starting") => VoiceState::Starting,
        Some("playing") => VoiceState::Playing,
        Some("gap") => VoiceState::Gap,
        Some("stopping") => VoiceState::Stopping,
        Some("completed") => VoiceState::Completed,
        Some("canceled") => VoiceState::Canceled,
        Some("failed") => VoiceState::Failed,
        Some("expired") => VoiceState::Expired,
        Some("interrupted") => VoiceState::Interrupted,
        _ => return Err(StorageError::Database),
    })
}
fn failure(v: VoiceFailure) -> &'static str {
    match v {
        VoiceFailure::Device => "device",
        VoiceFailure::Session => "session",
        VoiceFailure::Revoked => "revoked",
        VoiceFailure::Disabled => "disabled",
        VoiceFailure::Timeout => "timeout",
        VoiceFailure::ClockRegression => "clock",
        VoiceFailure::Storage => "storage",
    }
}
fn read_failure(v: &Value) -> Result<VoiceFailure, StorageError> {
    Ok(match v.as_str() {
        Some("device") => VoiceFailure::Device,
        Some("session") => VoiceFailure::Session,
        Some("revoked") => VoiceFailure::Revoked,
        Some("disabled") => VoiceFailure::Disabled,
        Some("timeout") => VoiceFailure::Timeout,
        Some("clock") => VoiceFailure::ClockRegression,
        Some("storage") => VoiceFailure::Storage,
        _ => return Err(StorageError::Database),
    })
}
fn id<T>(
    v: &Value,
    n: &str,
    make: impl FnOnce(u64) -> Result<T, InvalidId>,
) -> Result<T, StorageError> {
    make(number(v, n)?).map_err(|_| StorageError::Database)
}
pub(super) fn valid_timestamp(at: i64) -> bool {
    use fc_domain::time::*;
    let first = LocalDate::new(1, 1, 1)
        .map(|d| d.day_number() * 86_400)
        .unwrap_or(i64::MAX);
    let last = LocalDate::new(9999, 12, 31)
        .map(|d| d.day_number() * 86_400 + 86_399)
        .unwrap_or(i64::MIN);
    (first..=last).contains(&at)
}
fn signed(v: &Value, key: &str) -> Result<i64, StorageError> {
    v.get(key)
        .and_then(Value::as_i64)
        .filter(|at| valid_timestamp(*at))
        .ok_or(StorageError::Database)
}
fn optional<T>(
    v: &Value,
    f: impl FnOnce(&Value) -> Result<T, StorageError>,
) -> Result<Option<T>, StorageError> {
    if v.is_null() {
        Ok(None)
    } else {
        f(v).map(Some)
    }
}
fn u64_value(v: &Value) -> Result<u64, StorageError> {
    v.as_u64().ok_or(StorageError::Database)
}
fn i64_value(v: &Value) -> Result<i64, StorageError> {
    v.as_i64()
        .filter(|at| valid_timestamp(*at))
        .ok_or(StorageError::Database)
}
pub(super) fn asset(a: &VoiceAsset) -> Value {
    json!({"id":a.id.get(),"owner":a.owner.get(),"class":a.class_id.get(),"state":match a.state{VoiceAssetState::Reserved=>"reserved",VoiceAssetState::Ready=>"ready",VoiceAssetState::InUse=>"in_use",VoiceAssetState::DeletePending=>"delete_pending"},"revision":a.revision.get(),"charged":a.charged_bytes,"created":a.created_at,"expires":a.expires_at,"pcm":a.pcm.as_ref().map(|p|json!({"bytes":p.byte_length,"frames":p.frame_count,"hash":p.sha256.to_vec()}))})
}
pub(super) fn read_asset(v: &Value) -> Result<VoiceAsset, StorageError> {
    object_fields(
        v,
        &[
            "id", "owner", "class", "state", "revision", "charged", "created", "expires", "pcm",
        ],
    )?;
    Ok(VoiceAsset {
        id: VoiceAssetId::new(number(v, "id")?).map_err(|_| StorageError::Database)?,
        owner: id(v, "owner", PrincipalId::new)?,
        class_id: id(v, "class", ClassId::new)?,
        state: match text(v, "state")?.as_str() {
            "reserved" => VoiceAssetState::Reserved,
            "ready" => VoiceAssetState::Ready,
            "in_use" => VoiceAssetState::InUse,
            "delete_pending" => VoiceAssetState::DeletePending,
            _ => return Err(StorageError::Database),
        },
        revision: Revision::new(number(v, "revision")?),
        charged_bytes: number(v, "charged")?,
        created_at: signed(v, "created")?,
        expires_at: signed(v, "expires")?,
        pcm: optional(&v["pcm"], |p| {
            object_fields(p, &["bytes", "frames", "hash"])?;
            let hash: Vec<u8> =
                serde_json::from_value(p["hash"].clone()).map_err(|_| StorageError::Database)?;
            Ok(VoicePcmMetadata {
                byte_length: number(p, "bytes")?,
                frame_count: u32::try_from(number(p, "frames")?)
                    .map_err(|_| StorageError::Database)?,
                sha256: hash.try_into().map_err(|_| StorageError::Database)?,
            })
        })?,
    })
}
pub(super) fn job(j: &VoiceJob) -> Value {
    json!({"id":j.id.get(),"command":j.command_id.get(),"asset":j.asset_id.map(|v|v.get()),"owner":j.owner.get(),"class":j.class_id.get(),"total":j.repeat_total,"completed":j.completed_repeats,"current":j.current_repeat,"state":state(j.state),"revision":j.revision.get(),"generation":j.generation,"release":j.release_generation,"pending":j.pending_terminal.map(state),"sequence":j.sequence,"submitted":j.submitted_at,"latest":j.latest_start_at,"first":j.first_started_at,"gap":j.gap_until,"ended":j.ended_at,"failure":j.failure.map(failure),"actor":j.authorization.as_ref().map(voice_actor),"cancel":j.cancel_command.map(|v|v.get())})
}
pub(super) fn read_job(v: &Value) -> Result<VoiceJob, StorageError> {
    object_fields(
        v,
        &[
            "id",
            "command",
            "asset",
            "owner",
            "class",
            "total",
            "completed",
            "current",
            "state",
            "revision",
            "generation",
            "release",
            "pending",
            "sequence",
            "submitted",
            "latest",
            "first",
            "gap",
            "ended",
            "failure",
            "actor",
            "cancel",
        ],
    )?;
    Ok(VoiceJob {
        id: VoiceJobId::new(number(v, "id")?).map_err(|_| StorageError::Database)?,
        command_id: id(v, "command", CommandId::new)?,
        asset_id: optional(&v["asset"], |v| {
            VoiceAssetId::new(u64_value(v)?).map_err(|_| StorageError::Database)
        })?,
        owner: id(v, "owner", PrincipalId::new)?,
        class_id: id(v, "class", ClassId::new)?,
        repeat_total: u8::try_from(number(v, "total")?).map_err(|_| StorageError::Database)?,
        completed_repeats: u8::try_from(number(v, "completed")?)
            .map_err(|_| StorageError::Database)?,
        current_repeat: u8::try_from(number(v, "current")?).map_err(|_| StorageError::Database)?,
        state: read_state(&v["state"])?,
        revision: Revision::new(number(v, "revision")?),
        generation: number(v, "generation")?,
        release_generation: optional(&v["release"], u64_value)?,
        pending_terminal: optional(&v["pending"], read_state)?,
        sequence: number(v, "sequence")?,
        submitted_at: signed(v, "submitted")?,
        latest_start_at: signed(v, "latest")?,
        first_started_at: optional(&v["first"], i64_value)?,
        gap_until: optional(&v["gap"], i64_value)?,
        ended_at: optional(&v["ended"], i64_value)?,
        failure: optional(&v["failure"], read_failure)?,
        authorization: optional(&v["actor"], read_actor)?,
        cancel_command: optional(&v["cancel"], |v| {
            CommandId::new(u64_value(v)?).map_err(|_| StorageError::Database)
        })?,
    })
}
fn voice_actor(a: &ActorContext) -> Value {
    json!({"principal":a.principal_id.get(),"school":a.school_id.get(),"epoch":a.auth_epoch.get(),"expiry":time(a.expires_at),"origin":match a.origin{TrustedOrigin::Local=>"local",TrustedOrigin::Web=>"web"}})
}
fn read_actor(v: &Value) -> Result<ActorContext, StorageError> {
    object_fields(v, &["principal", "school", "epoch", "expiry", "origin"])?;
    Ok(ActorContext {
        principal_id: id(v, "principal", PrincipalId::new)?,
        school_id: id(v, "school", SchoolId::new)?,
        auth_epoch: AuthEpoch::new(number(v, "epoch")?),
        expires_at: read_time(&v["expiry"])?,
        origin: match text(v, "origin")?.as_str() {
            "local" => TrustedOrigin::Local,
            "web" => TrustedOrigin::Web,
            _ => return Err(StorageError::Database),
        },
    })
}
// Terminal audits omit execution-only null fields and the original lease. A
// strict fixed tuple keeps the full 100-entry audit budget below one document.
fn stored_job(j: &VoiceJob) -> Value {
    if !j.state.terminal() {
        return job(j);
    }
    json!([
        j.id.get(),
        j.owner.get(),
        j.class_id.get(),
        j.repeat_total,
        j.completed_repeats,
        j.current_repeat,
        state(j.state),
        j.revision.get(),
        j.generation,
        j.sequence,
        j.submitted_at,
        j.latest_start_at,
        j.first_started_at,
        j.ended_at,
        j.failure.map(failure),
        j.cancel_command.map(|v| v.get()),
        j.asset_id.map(|v| v.get())
    ])
}
fn read_stored_job(v: &Value) -> Result<VoiceJob, StorageError> {
    let Some(a) = v.as_array() else {
        return read_job(v);
    };
    if a.len() != 17 {
        return Err(StorageError::Database);
    }
    let id = VoiceJobId::new(u64_value(&a[0])?).map_err(|_| StorageError::Database)?;
    let state = read_state(&a[6])?;
    if !state.terminal() {
        return Err(StorageError::Database);
    }
    Ok(VoiceJob {
        id,
        command_id: CommandId::new(id.get()).map_err(|_| StorageError::Database)?,
        asset_id: optional(&a[16], |v| {
            VoiceAssetId::new(u64_value(v)?).map_err(|_| StorageError::Database)
        })?,
        owner: PrincipalId::new(u64_value(&a[1])?).map_err(|_| StorageError::Database)?,
        class_id: ClassId::new(u64_value(&a[2])?).map_err(|_| StorageError::Database)?,
        repeat_total: u8::try_from(u64_value(&a[3])?).map_err(|_| StorageError::Database)?,
        completed_repeats: u8::try_from(u64_value(&a[4])?).map_err(|_| StorageError::Database)?,
        current_repeat: u8::try_from(u64_value(&a[5])?).map_err(|_| StorageError::Database)?,
        state,
        revision: Revision::new(u64_value(&a[7])?),
        generation: u64_value(&a[8])?,
        release_generation: None,
        pending_terminal: None,
        sequence: u64_value(&a[9])?,
        submitted_at: i64_value(&a[10])?,
        latest_start_at: i64_value(&a[11])?,
        first_started_at: optional(&a[12], i64_value)?,
        gap_until: None,
        ended_at: optional(&a[13], i64_value)?,
        failure: optional(&a[14], read_failure)?,
        authorization: None,
        cancel_command: optional(&a[15], |v| {
            CommandId::new(u64_value(v)?).map_err(|_| StorageError::Database)
        })?,
    })
}
pub(super) fn snapshot(s: &VoiceSnapshot) -> Value {
    json!({"format":1,"revision":s.revision.get(),"enabled":s.enabled,"sequence":s.next_sequence,"generation":s.next_generation,"observed":s.observed_at,"assets":s.assets.iter().map(asset).collect::<Vec<_>>(),"jobs":s.jobs.iter().map(stored_job).collect::<Vec<_>>()})
}
pub(super) fn read_snapshot(v: &Value) -> Result<VoiceSnapshot, StorageError> {
    object_fields(
        v,
        &[
            "format",
            "revision",
            "enabled",
            "sequence",
            "generation",
            "observed",
            "assets",
            "jobs",
        ],
    )?;
    if number(v, "format")? != 1
        || array(v, "assets")?.len() > MAX_VOICE_ASSETS
        || array(v, "jobs")?.len() > MAX_VOICE_JOBS
    {
        return Err(StorageError::Database);
    }
    let s = VoiceSnapshot {
        revision: Revision::new(number(v, "revision")?),
        enabled: v["enabled"].as_bool().ok_or(StorageError::Database)?,
        next_sequence: number(v, "sequence")?,
        next_generation: number(v, "generation")?,
        observed_at: optional(&v["observed"], i64_value)?,
        assets: array(v, "assets")?
            .iter()
            .map(read_asset)
            .collect::<Result<_, _>>()?,
        jobs: array(v, "jobs")?
            .iter()
            .map(read_stored_job)
            .collect::<Result<_, _>>()?,
    };
    s.validate().map_err(|_| StorageError::Database)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::time::*;
    fn maximal_snapshot() -> VoiceSnapshot {
        let date = LocalDate::new(2026, 10, 30).unwrap();
        let at = LocalDateTime::new(date, 3600).unwrap();
        let now = civil_seconds(at);
        let mut s = VoiceSnapshot {
            revision: Revision::new(u64::MAX),
            next_sequence: 104,
            next_generation: u64::MAX,
            observed_at: Some(now),
            ..VoiceSnapshot::default()
        };
        for n in 0..100 {
            s.jobs.push(VoiceJob {
                id: VoiceJobId::new(u64::MAX - n).unwrap(),
                command_id: CommandId::new(u64::MAX - n).unwrap(),
                asset_id: None,
                owner: PrincipalId::new(u64::MAX).unwrap(),
                class_id: ClassId::new(u64::MAX).unwrap(),
                repeat_total: 10,
                completed_repeats: 9,
                current_repeat: 10,
                state: VoiceState::Canceled,
                revision: Revision::new(u64::MAX),
                generation: u64::MAX,
                release_generation: None,
                pending_terminal: None,
                sequence: n + 1,
                submitted_at: now,
                latest_start_at: now + 300,
                first_started_at: Some(now),
                gap_until: None,
                ended_at: Some(now),
                failure: Some(VoiceFailure::Device),
                authorization: None,
                cancel_command: Some(CommandId::new(u64::MAX - 1000 - n).unwrap()),
            });
        }
        for n in 0..MAX_VOICE_ASSETS {
            let owner = PrincipalId::new(u64::MAX - (n % 2) as u64).unwrap();
            let class_id = ClassId::new(u64::MAX).unwrap();
            let asset_id = VoiceAssetId::new(u64::MAX - 500 - n as u64).unwrap();
            s.assets.push(VoiceAsset {
                id: asset_id,
                owner,
                class_id,
                state: if n < 4 {
                    VoiceAssetState::InUse
                } else {
                    VoiceAssetState::DeletePending
                },
                revision: Revision::new(u64::MAX),
                charged_bytes: 16_044,
                pcm: Some(VoicePcmMetadata {
                    byte_length: 16_044,
                    frame_count: 8_000,
                    sha256: [255; 32],
                }),
                created_at: now,
                expires_at: now + 600,
            });
            if n < 4 {
                let id = u64::MAX - 100 - n as u64;
                s.jobs.push(VoiceJob {
                    id: VoiceJobId::new(id).unwrap(),
                    command_id: CommandId::new(id).unwrap(),
                    asset_id: Some(asset_id),
                    owner,
                    class_id,
                    repeat_total: 10,
                    completed_repeats: 0,
                    current_repeat: 0,
                    state: VoiceState::Queued,
                    revision: Revision::new(u64::MAX),
                    generation: 0,
                    release_generation: None,
                    pending_terminal: None,
                    sequence: 101 + n as u64,
                    submitted_at: now,
                    latest_start_at: now + 300,
                    first_started_at: None,
                    gap_until: None,
                    ended_at: None,
                    failure: None,
                    authorization: Some(ActorContext {
                        principal_id: owner,
                        school_id: SchoolId::new(u64::MAX).unwrap(),
                        auth_epoch: AuthEpoch::new(u64::MAX),
                        expires_at: LocalDateTime::new(date, 86399).unwrap(),
                        origin: TrustedOrigin::Web,
                    }),
                    cancel_command: None,
                });
            }
        }
        s
    }
    #[test]
    fn full_audit_with_twenty_digit_ids_four_active_and_max_pending_assets_fits_document() {
        let s = maximal_snapshot();
        s.validate().unwrap();
        let value = snapshot(&s);
        let bytes = codec::bytes(&value).unwrap();
        eprintln!("VOICE_MAX_METADATA_BYTES {}", bytes.len());
        assert!(bytes.len() < 48 * 1024);
        assert_eq!(read_snapshot(&value).unwrap(), s);
    }
    #[test]
    fn damaged_time_and_active_state_in_compact_audit_fail_closed() {
        let s = maximal_snapshot();
        let mut v = snapshot(&s);
        v["observed"] = json!(i64::MIN);
        assert_eq!(read_snapshot(&v), Err(StorageError::Database));
        let mut v = snapshot(&s);
        v["assets"][0]["created"] = json!(i64::MIN);
        assert_eq!(read_snapshot(&v), Err(StorageError::Database));
        let mut v = snapshot(&s);
        v["jobs"][0][6] = json!("playing");
        assert_eq!(read_snapshot(&v), Err(StorageError::Database));
        let mut v = snapshot(&s);
        v["jobs"][100]["release"] = json!(1);
        assert_eq!(read_snapshot(&v), Err(StorageError::Database));
    }
}
