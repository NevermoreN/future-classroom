//! One consumption lane for raw requests and the closed local typed intents.
//! Only the Writer can mint a first-execution reply, after durable acceptance.
use crate::{StorageError, MAX_VALUE_BYTES};
use fc_ports::command_window::{IssuedCommand, TypedCommandIntent, MAX_WINDOW_BYTES};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Query,
    Accept,
    Atomic,
    ManualReserve,
    Complete,
    ManualFinish,
    Barrier,
    Fail(fc_ports::command_window::TypedCommandFailure),
}
/// Opaque trusted-task description, never a client-selected execution permit.
pub struct WindowAction {
    pub(crate) intent: TypedCommandIntent,
    pub(crate) mode: Mode,
    pub(crate) original: Option<IssuedCommand>,
}
impl WindowAction {
    pub(crate) fn new(intent: TypedCommandIntent, mode: Mode) -> Self {
        Self {
            intent,
            mode,
            original: None,
        }
    }
    pub(crate) fn owner_return(&self) -> bool {
        matches!(self.mode, Mode::ManualFinish | Mode::Fail(_))
    }
}
#[derive(Clone)]
pub struct StoredTyped {
    pub(crate) phase: u8,
    pub(crate) value: Vec<u8>,
}
impl StoredTyped {
    pub(crate) fn pending() -> Self {
        Self {
            phase: 1,
            value: Vec::new(),
        }
    }
}
/// Non-Clone, non-serializable and without a public constructor. The sole
/// constructor consumes the Writer's first Accepted reply. Completion moves it.
pub struct FirstExecutionPermit {
    pub(crate) issued: IssuedCommand,
    pub(crate) intent: TypedCommandIntent,
}
impl FirstExecutionPermit {
    pub fn intent(&self) -> &TypedCommandIntent {
        &self.intent
    }
}
pub enum TypedAdmission {
    Fresh(FirstExecutionPermit),
    History(fc_ports::command_window::TypedCommandOutcome),
}

pub(crate) fn open(db: &Connection) -> Result<(), StorageError> {
    let existed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='command_window_consumption')",[],|r|r.get(0)).map_err(sql)?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS command_window_consumption(
        serial INTEGER PRIMARY KEY REFERENCES command_window_entries(serial),
        kind INTEGER NOT NULL CHECK(kind BETWEEN 0 AND 5), intent BLOB NOT NULL,
        phase INTEGER NOT NULL CHECK(phase BETWEEN 1 AND 3), result BLOB NOT NULL);",
    )
    .map_err(sql)?;
    if !existed {
        // Earlier unpublished v2 NoRaw tasks did not retain consumption. Their
        // absence is not Fresh. Retire only client admission; preserve all
        // original held entries, ADs and private negative owner capabilities.
        db.execute(
            "UPDATE command_window_meta SET retired=issued WHERE singleton=1",
            [],
        )
        .map_err(sql)?;
    }
    let malformed: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM command_window_consumption c LEFT JOIN command_window_entries e ON e.serial=c.serial WHERE e.serial IS NULL OR length(c.intent)>512 OR length(c.result)>65536 OR (c.kind=0 AND (length(c.intent)<>0 OR length(c.result)<>0 OR c.phase NOT IN (2,3))) OR (c.phase=1 AND length(c.result)<>0))", [], |r|r.get(0)).map_err(sql)?;
    if malformed {
        return Err(StorageError::Database);
    }
    Ok(())
}
// Raw phase 2 is consumed; raw phase 3 is a replay-inert business-prune
// tombstone. It retains the original entry/count/base charge until normal GC.
pub(crate) fn raw_retired(db: &Connection, serial: u64) -> Result<bool, StorageError> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM command_window_consumption WHERE serial=?1 AND kind=0 AND phase=3)",
        [serial],
        |r| r.get(0),
    ).map_err(sql)
}
pub(crate) fn retire_raw(db: &Connection, serial: u64) -> Result<(), StorageError> {
    let changed = db.execute(
        "UPDATE command_window_consumption SET phase=3 WHERE serial=?1 AND kind=0 AND phase=2 AND length(intent)=0 AND length(result)=0",
        [serial],
    ).map_err(sql)?;
    if changed != 1 {
        return Err(StorageError::Database);
    }
    Ok(())
}
pub(crate) fn raw_check(db: &Connection, issued: IssuedCommand) -> Result<(), StorageError> {
    let state: Option<(u8, u8)> = db
        .query_row(
            "SELECT kind,phase FROM command_window_consumption WHERE serial=?1",
            [issued.ticket.serial()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    match state {
        Some((0, 3)) => Err(StorageError::CommandExpired),
        Some((0, 2)) | None => Ok(()),
        Some((0, _)) => Err(StorageError::Database),
        Some(_) => Err(StorageError::CommandIdReused),
    }
}
pub(crate) fn raw_claim(db: &Connection, issued: IssuedCommand) -> Result<(), StorageError> {
    raw_check(db, issued)?;
    db.execute(
        "INSERT OR IGNORE INTO command_window_consumption VALUES(?1,0,X'',2,X'')",
        [issued.ticket.serial()],
    )
    .map_err(sql)?;
    Ok(())
}
pub(crate) fn read(
    db: &Connection,
    issued: IssuedCommand,
    action: &WindowAction,
) -> Result<Option<StoredTyped>, StorageError> {
    let (kind, intent) = crate::business::typed_commands::intent_bytes(&action.intent)?;
    let row: Option<(u8, Vec<u8>, u8, Vec<u8>)> = db
        .query_row(
            "SELECT kind,intent,phase,result FROM command_window_consumption WHERE serial=?1",
            [issued.ticket.serial()],
            |r| {
                Ok((
                    r.get(0)?,
                    crate::read_limited_blob(r, 1)?,
                    r.get(2)?,
                    crate::read_limited_blob(r, 3)?,
                ))
            },
        )
        .optional()
        .map_err(sql)?;
    if let Some((actual_kind, actual_intent, phase, value)) = row {
        if actual_kind != kind || actual_intent != intent {
            return Err(StorageError::CommandIdReused);
        }
        return Ok(Some(StoredTyped { phase, value }));
    }
    let recorded: bool = db
        .query_row(
            "SELECT recorded FROM command_window_entries WHERE serial=?1",
            [issued.ticket.serial()],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if recorded {
        return Err(StorageError::CommandIdReused);
    }
    if matches!(
        action.mode,
        Mode::Query | Mode::Complete | Mode::ManualFinish | Mode::Fail(_)
    ) {
        return Err(StorageError::CommandNotIssued);
    }
    Ok(None)
}
pub(crate) fn persist(
    db: &Connection,
    issued: IssuedCommand,
    action: &WindowAction,
    previous: Option<&StoredTyped>,
    stored: &StoredTyped,
) -> Result<(), StorageError> {
    let (kind, intent) = crate::business::typed_commands::intent_bytes(&action.intent)?;
    if stored.value.len() > MAX_VALUE_BYTES || intent.len() > 512 {
        return Err(StorageError::DocumentTooLarge);
    }
    if previous.is_some_and(|p| p.phase != 1) {
        return Err(StorageError::CommandIdReused);
    }
    let old_extra = previous.map_or(0, |p| charge(intent.len(), p));
    let extra = charge(intent.len(), stored);
    let total: u64 = db
        .query_row(
            "SELECT COALESCE(SUM(charged_bytes),0) FROM command_window_entries",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if total
        .checked_sub(old_extra)
        .and_then(|n| n.checked_add(extra))
        .is_none_or(|n| n > MAX_WINDOW_BYTES)
    {
        return Err(StorageError::CommandWindowCapacity);
    }
    db.execute("INSERT INTO command_window_consumption VALUES(?1,?2,?3,?4,?5) ON CONFLICT(serial) DO UPDATE SET phase=excluded.phase,result=excluded.result",params![issued.ticket.serial(),kind,intent,stored.phase,stored.value]).map_err(sql)?;
    db.execute("UPDATE command_window_entries SET charged_bytes=charged_bytes-?1+?2,held=?3 WHERE serial=?4",params![old_extra,extra,stored.phase==1,issued.ticket.serial()]).map_err(sql)?;
    Ok(())
}
fn charge(intent_len: usize, stored: &StoredTyped) -> u64 {
    128 + intent_len as u64
        + if stored.phase == 1 {
            MAX_VALUE_BYTES as u64
        } else {
            stored.value.len() as u64
        }
}
fn sql(_: rusqlite::Error) -> StorageError {
    StorageError::Database
}

/// The original Complete task has consumed its private permit and returned a
/// definite business rejection after a checked SQL/authority rollback. Record
/// that failure, without repeating a native effect or settling an audio owner.
pub(crate) fn record_known_failure(
    db: &mut Connection,
    action: &WindowAction,
    error: &StorageError,
) -> Result<(), StorageError> {
    use fc_domain::{identity::IdentityError, voice::VoiceError};
    if action.mode != Mode::Complete {
        return Ok(());
    }
    let code = match error {
        StorageError::Voice(VoiceError::RevisionConflict) => 2,
        StorageError::Voice(VoiceError::StaleGeneration) => 3,
        StorageError::Voice(VoiceError::Permission(_))
        | StorageError::Identity(IdentityError::Permission(_)) => 1,
        StorageError::ExecutionCancelled | StorageError::CommandExpired => 4,
        _ => return Ok(()),
    };
    let original = action.original.ok_or(StorageError::CommandNotIssued)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql)?;
    let previous = read(&tx, original, action)?.ok_or(StorageError::Database)?;
    if previous.phase != 1 {
        return Err(StorageError::Database);
    }
    persist(
        &tx,
        original,
        action,
        Some(&previous),
        &StoredTyped {
            phase: 3,
            value: vec![code],
        },
    )?;
    tx.commit().map_err(|_| StorageError::Indeterminate)
}
