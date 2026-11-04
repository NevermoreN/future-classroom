use super::*;
use fc_domain::voice::VoiceLimits;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Limits {
    pub max_duration_millis: u32,
    pub max_pending_jobs: u8,
    pub max_pending_per_owner: u8,
    pub max_repeat_total: u8,
}
impl From<VoiceLimits> for Limits {
    fn from(v: VoiceLimits) -> Self {
        Self {
            max_duration_millis: v.max_duration_millis,
            max_pending_jobs: v.max_pending_jobs,
            max_pending_per_owner: v.max_pending_per_owner,
            max_repeat_total: v.max_repeat_total,
        }
    }
}
impl Limits {
    fn validated(self) -> Result<VoiceLimits, ProtocolError> {
        let v = VoiceLimits {
            max_duration_millis: self.max_duration_millis,
            max_pending_jobs: self.max_pending_jobs,
            max_pending_per_owner: self.max_pending_per_owner,
            max_repeat_total: self.max_repeat_total,
        };
        v.validate().map_err(|_| ProtocolError::InvalidCommand)?;
        Ok(v)
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub result: &'static str,
    pub observed_at: String,
    pub remaining_validity_millis: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    pub publication_revision: String,
    pub identity_revision: String,
    pub runtime_generation: String,
    pub enabled: bool,
    pub admitting: bool,
    pub active_jobs: usize,
    pub queued_jobs: usize,
    pub playing_jobs: usize,
    pub limits: Limits,
    pub device_observation: Option<Observation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Enabled {
    expected_identity_revision: String,
    expected_runtime_generation: String,
    enabled: bool,
    reviewed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SaveLimits {
    expected_identity_revision: String,
    expected_runtime_generation: String,
    limits: Limits,
    reviewed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Check {
    expected_identity_revision: String,
    expected_runtime_generation: String,
    expected_publication_revision: String,
}
pub enum Change {
    Enabled {
        enabled: bool,
        identity: Revision,
        runtime: u64,
    },
    Limits {
        limits: VoiceLimits,
        identity: Revision,
        runtime: u64,
    },
    Check {
        publication: Revision,
        identity: Revision,
        runtime: u64,
    },
}
pub fn mutation(bytes: &[u8]) -> Result<(Revision, Change), ProtocolError> {
    let input = Mutation::decode(bytes)?;
    let expected = revision(&input.expected_revision)?;
    let change = match input.action.as_str() {
        "setEnabled" => {
            let (_, v) = input.candidate::<Enabled>()?;
            if !v.reviewed {
                return Err(ProtocolError::InvalidCommand);
            }
            Change::Enabled {
                enabled: v.enabled,
                identity: revision(&v.expected_identity_revision)?,
                runtime: revision(&v.expected_runtime_generation)?.get(),
            }
        }
        "setLimits" => {
            let (_, v) = input.candidate::<SaveLimits>()?;
            if !v.reviewed {
                return Err(ProtocolError::InvalidCommand);
            }
            Change::Limits {
                limits: v.limits.validated()?,
                identity: revision(&v.expected_identity_revision)?,
                runtime: revision(&v.expected_runtime_generation)?.get(),
            }
        }
        "checkOutput" => {
            let (_, v) = input.candidate::<Check>()?;
            Change::Check {
                publication: revision(&v.expected_publication_revision)?,
                identity: revision(&v.expected_identity_revision)?,
                runtime: revision(&v.expected_runtime_generation)?.get(),
            }
        }
        _ => return Err(ProtocolError::InvalidCommand),
    };
    Ok((expected, change))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn voice_policy_requires_explicit_review_and_keeps_original_hard_limits() {
        let body = |reviewed: bool, duration: u32| {
            format!(
                r#"{{"schemaVersion":1,"expectedRevision":"0","action":"setLimits","candidate":{{"expectedIdentityRevision":"1","expectedRuntimeGeneration":"2","reviewed":{reviewed},"limits":{{"maxDurationMillis":{duration},"maxPendingJobs":4,"maxPendingPerOwner":2,"maxRepeatTotal":10}}}}}}"#
            )
        };
        assert!(mutation(body(true, 60_000).as_bytes()).is_ok());
        assert!(mutation(body(false, 60_000).as_bytes()).is_err());
        assert!(mutation(body(true, 60_001).as_bytes()).is_err());
    }
}
