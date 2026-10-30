use super::{codec, *};
use fc_domain::autostart::*;
fn failure_name(f: AutostartFailure) -> &'static str {
    match f {
        AutostartFailure::NotObserved => "not_observed",
        AutostartFailure::AccessDenied => "access_denied",
        AutostartFailure::InvalidValue => "invalid_value",
        AutostartFailure::PathUnavailable => "path_unavailable",
        AutostartFailure::ReadFailed => "read_failed",
        AutostartFailure::WriteFailed => "write_failed",
        AutostartFailure::ReadbackMismatch => "readback_mismatch",
        AutostartFailure::PreimageChanged => "preimage_changed",
        AutostartFailure::Cancelled => "cancelled",
        AutostartFailure::Interrupted => "interrupted",
        AutostartFailure::Unavailable => "unavailable",
    }
}
fn read_failure(v: &Value) -> Result<AutostartFailure, StorageError> {
    Ok(match v.as_str().ok_or(StorageError::Database)? {
        "not_observed" => AutostartFailure::NotObserved,
        "access_denied" => AutostartFailure::AccessDenied,
        "invalid_value" => AutostartFailure::InvalidValue,
        "path_unavailable" => AutostartFailure::PathUnavailable,
        "read_failed" => AutostartFailure::ReadFailed,
        "write_failed" => AutostartFailure::WriteFailed,
        "readback_mismatch" => AutostartFailure::ReadbackMismatch,
        "preimage_changed" => AutostartFailure::PreimageChanged,
        "cancelled" => AutostartFailure::Cancelled,
        "interrupted" => AutostartFailure::Interrupted,
        "unavailable" => AutostartFailure::Unavailable,
        _ => return Err(StorageError::Database),
    })
}
pub(super) fn observation(o: &AutostartObservation) -> Value {
    match o {
        AutostartObservation::Absent => json!({"kind":"absent"}),
        AutostartObservation::Command(c) => json!({"kind":"command","command":c}),
        AutostartObservation::Opaque { kind, bytes } => {
            json!({"kind":"opaque","type":kind,"bytes":bytes})
        }
        AutostartObservation::Unavailable(f) => {
            json!({"kind":"unavailable","failure":failure_name(*f)})
        }
    }
}
fn read_observation(v: &Value) -> Result<AutostartObservation, StorageError> {
    let o = match v["kind"].as_str().ok_or(StorageError::Database)? {
        "absent" => {
            codec::object_fields(v, &["kind"])?;
            AutostartObservation::Absent
        }
        "command" => {
            codec::object_fields(v, &["kind", "command"])?;
            AutostartObservation::Command(codec::text(v, "command")?)
        }
        "opaque" => {
            codec::object_fields(v, &["kind", "type", "bytes"])?;
            AutostartObservation::Opaque {
                kind: u32::try_from(codec::number(v, "type")?)
                    .map_err(|_| StorageError::Database)?,
                bytes: u32::try_from(codec::number(v, "bytes")?)
                    .map_err(|_| StorageError::Database)?,
            }
        }
        "unavailable" => {
            codec::object_fields(v, &["kind", "failure"])?;
            AutostartObservation::Unavailable(read_failure(&v["failure"])?)
        }
        _ => return Err(StorageError::Database),
    };
    if !o.validate() {
        return Err(StorageError::Database);
    }
    Ok(o)
}
fn ticket(t: &AutostartTicket) -> Value {
    json!({"command_id":t.command_id.get(),"sequence":t.sequence,"prepared_revision":t.prepared_revision.get(),"principal":t.principal_id.get(),"auth_epoch":t.auth_epoch.get()})
}
fn read_ticket(v: &Value) -> Result<AutostartTicket, StorageError> {
    codec::object_fields(
        v,
        &[
            "command_id",
            "sequence",
            "prepared_revision",
            "principal",
            "auth_epoch",
        ],
    )?;
    Ok(AutostartTicket {
        command_id: CommandId::new(codec::number(v, "command_id")?)
            .map_err(|_| StorageError::Database)?,
        sequence: codec::number(v, "sequence")?,
        prepared_revision: Revision::new(codec::number(v, "prepared_revision")?),
        principal_id: PrincipalId::new(codec::number(v, "principal")?)
            .map_err(|_| StorageError::Database)?,
        auth_epoch: AuthEpoch::new(codec::number(v, "auth_epoch")?),
    })
}
pub(super) fn operation(o: &AutostartOperation) -> Value {
    json!({"ticket":ticket(&o.ticket),"desired":o.desired,"target":o.target_command,"previous":observation(&o.previous_observed),"phase":match o.phase{AutostartPhase::Prepared=>"prepared",AutostartPhase::Applying=>"applying",AutostartPhase::Succeeded=>"succeeded",AutostartPhase::Failed=>"failed",AutostartPhase::NeedsReconcile=>"needs_reconcile"},"observed":o.observed.as_ref().map(observation),"failure":o.failure.map(failure_name),"mutated":o.mutated})
}
pub(super) fn read_operation(v: &Value) -> Result<AutostartOperation, StorageError> {
    codec::object_fields(
        v,
        &[
            "ticket", "desired", "target", "previous", "phase", "observed", "failure", "mutated",
        ],
    )?;
    Ok(AutostartOperation {
        ticket: read_ticket(&v["ticket"])?,
        desired: codec::boolean(v, "desired")?,
        target_command: codec::text(v, "target")?,
        previous_observed: read_observation(&v["previous"])?,
        phase: match v["phase"].as_str().ok_or(StorageError::Database)? {
            "prepared" => AutostartPhase::Prepared,
            "applying" => AutostartPhase::Applying,
            "succeeded" => AutostartPhase::Succeeded,
            "failed" => AutostartPhase::Failed,
            "needs_reconcile" => AutostartPhase::NeedsReconcile,
            _ => return Err(StorageError::Database),
        },
        observed: if v["observed"].is_null() {
            None
        } else {
            Some(read_observation(&v["observed"])?)
        },
        failure: if v["failure"].is_null() {
            None
        } else {
            Some(read_failure(&v["failure"])?)
        },
        mutated: codec::boolean(v, "mutated")?,
    })
}
pub(super) fn state(s: &AutostartState) -> Value {
    json!({"version":1,"revision":s.revision.get(),"sequence":s.sequence,"desired":s.desired,"target":s.target_command,"observed":observation(&s.observed),"history":s.history.iter().map(operation).collect::<Vec<_>>()})
}
pub(super) fn read_state(v: &Value) -> Result<AutostartState, StorageError> {
    codec::object_fields(
        v,
        &[
            "version", "revision", "sequence", "desired", "target", "observed", "history",
        ],
    )?;
    if codec::number(v, "version")? != 1 {
        return Err(StorageError::Database);
    }
    let s = AutostartState {
        revision: Revision::new(codec::number(v, "revision")?),
        sequence: codec::number(v, "sequence")?,
        desired: codec::boolean(v, "desired")?,
        target_command: if v["target"].is_null() {
            None
        } else {
            Some(codec::text(v, "target")?)
        },
        observed: read_observation(&v["observed"])?,
        history: codec::array(v, "history")?
            .iter()
            .map(read_operation)
            .collect::<Result<_, _>>()?,
    };
    if !s.validate() {
        return Err(StorageError::Database);
    }
    Ok(s)
}
