//! One persisted, non-renewing grace for receipts that predate v2. Missing IDs
//! are never admitted. The live database boundary, not a request or backup,
//! freezes the allowlist. Oversized legacy history is denied and reclaimed in
//! fixed batches; it is still counted against issuance capacity until removed.
use crate::{
    business::command_window as catalog, StorageError, TransactionDocuments, TransactionView,
};
use fc_domain::ids::CommandId;
use fc_ports::command_window::{
    CommandFamily, COMMAND_WINDOW_MILLIS, MAX_WINDOW_BYTES, MAX_WINDOW_COMMANDS,
};
use rusqlite::{params, Connection};

fn predicate() -> String {
    catalog::slots().iter().map(|slot| {
        // Fixed code-owned prefixes contain no SQL quote or wildcard. Canonical
        // numeric suffixes only; v2 receipts are excluded from this projection.
        let start=slot.prefix.len()+1;
        format!("(key>='{}' AND key<'{}~' AND length(substr(key,{start})) BETWEEN 1 AND 20 AND substr(key,{start}) NOT GLOB '*[^0-9]*')",slot.prefix,slot.prefix)
    }).collect::<Vec<_>>().join(" OR ")
}
fn sql_error(_: rusqlite::Error) -> StorageError {
    StorageError::Database
}
pub(super) fn open(db: &Connection, logical: u64) -> Result<(), StorageError> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS command_window_legacy_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),deadline INTEGER NOT NULL,closed INTEGER NOT NULL CHECK(closed IN(0,1)),cursor TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS command_window_legacy_docs(key TEXT PRIMARY KEY,family TEXT NOT NULL,payload BLOB NOT NULL CHECK(length(payload)=8),bytes INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS command_window_legacy_journals(command_id TEXT PRIMARY KEY,bytes INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS command_window_legacy_pair ON command_window_legacy_docs(family,payload);") .map_err(sql_error)?;
    let existing: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM command_window_legacy_meta)",
            [],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    if existing {
        return Ok(());
    }
    let deadline = logical
        .checked_add(COMMAND_WINDOW_MILLIS)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(StorageError::CommandWindowClock)?;
    db.execute(
        "INSERT INTO command_window_legacy_meta VALUES(1,?1,0,'')",
        [deadline],
    )
    .map_err(sql_error)?;
    let query = format!(
        "SELECT key,length(value) FROM documents WHERE {} ORDER BY revision DESC,key LIMIT 65537",
        predicate()
    );
    let mut q = db.prepare(&query).map_err(sql_error)?;
    let mut rows = q.query([]).map_err(sql_error)?;
    let mut total = 0u64;
    let mut count = 0u64;
    while let Some(row) = rows.next().map_err(sql_error)? {
        let key: String = row.get(0).map_err(sql_error)?;
        let bytes: u64 = row.get(1).map_err(sql_error)?;
        let Some((slot, id)) = catalog::legacy(&key) else {
            return Err(StorageError::Database);
        };
        let charge = bytes
            .checked_add(key.len() as u64 + 96)
            .ok_or(StorageError::CommandWindowCapacity)?;
        if count >= MAX_WINDOW_COMMANDS
            || total
                .checked_add(charge)
                .is_none_or(|n| n > MAX_WINDOW_BYTES)
        {
            break;
        }
        db.execute(
            "INSERT INTO command_window_legacy_docs VALUES(?1,?2,?3,?4)",
            params![
                key,
                slot.family.name(),
                id.get().to_be_bytes().as_slice(),
                charge
            ],
        )
        .map_err(sql_error)?;
        total += charge;
        count += 1;
    }
    drop(rows);
    drop(q);
    let mut q=db.prepare("SELECT command_id,length(fingerprint)+length(command_id)+96 FROM command_journal ORDER BY revision DESC,command_id LIMIT 65537").map_err(sql_error)?;
    let mut rows = q.query([]).map_err(sql_error)?;
    while let Some(row) = rows.next().map_err(sql_error)? {
        let id: String = row.get(0).map_err(sql_error)?;
        let charge: u64 = row.get(1).map_err(sql_error)?;
        if count >= MAX_WINDOW_COMMANDS
            || total
                .checked_add(charge)
                .is_none_or(|n| n > MAX_WINDOW_BYTES)
        {
            break;
        }
        db.execute(
            "INSERT INTO command_window_legacy_journals VALUES(?1,?2)",
            params![id, charge],
        )
        .map_err(sql_error)?;
        total += charge;
        count += 1;
    }
    Ok(())
}
pub(super) fn check_journal(db: &Connection, logical: u64, id: &str) -> Result<(), StorageError> {
    let (deadline, closed): (u64, bool) = db
        .query_row(
            "SELECT deadline,closed FROM command_window_legacy_meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql_error)?;
    if closed || logical >= deadline {
        return Err(StorageError::CommandExpired);
    }
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM command_window_legacy_journals l JOIN command_journal j ON j.command_id=l.command_id WHERE l.command_id=?1)",[id],|r|r.get(0)).map_err(sql_error)?;
    if !exists {
        return Err(StorageError::CommandNotIssued);
    }
    Ok(())
}
pub(super) fn check(
    db: &Connection,
    logical: u64,
    family: CommandFamily,
    id: CommandId,
) -> Result<(), StorageError> {
    let (deadline, closed): (u64, bool) = db
        .query_row(
            "SELECT deadline,closed FROM command_window_legacy_meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql_error)?;
    if closed || logical >= deadline {
        return Err(StorageError::CommandExpired);
    }
    let mut q=db.prepare("SELECT key FROM command_window_legacy_docs WHERE family=?1 AND payload=?2 ORDER BY key LIMIT 5").map_err(sql_error)?;
    let keys = q
        .query_map(
            params![family.name(), id.get().to_be_bytes().as_slice()],
            |r| r.get::<_, String>(0),
        )
        .map_err(sql_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sql_error)?;
    if keys.is_empty() {
        return Err(StorageError::CommandNotIssued);
    }
    if keys.len() > 4 {
        return Err(StorageError::Database);
    }
    // All originally indexed rows must still exist. Losing a primary receipt
    // while retaining only a result must never turn replay into new execution.
    let mut primary = false;
    for key in keys {
        let (slot, _) = catalog::legacy(&key).ok_or(StorageError::Database)?;
        primary |= !slot.prefix.contains("/result/")
            && !slot.prefix.ends_with("/results/")
            && !slot.prefix.ends_with("/web-outcomes/");
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM documents WHERE key=?1)",
                [key],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        if !exists {
            return Err(StorageError::CommandNotIssued);
        }
    }
    if !primary {
        return Err(StorageError::CommandNotIssued);
    }
    Ok(())
}
pub(super) fn usage(db: &Connection) -> Result<(u64, u64), StorageError> {
    let query = format!(
        "SELECT COUNT(*),COALESCE(SUM(length(value)+length(key)+96),0) FROM documents WHERE {}",
        predicate()
    );
    let (count, bytes): (u64, u64) = db
        .query_row(&query, [], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(sql_error)?;
    let (jc,jb):(u64,u64)=db.query_row("SELECT COUNT(*),COALESCE(SUM(length(command_id)+length(fingerprint)+96),0) FROM command_journal",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(sql_error)?;
    Ok((
        count
            .checked_add(jc)
            .ok_or(StorageError::CommandWindowCapacity)?,
        bytes
            .checked_add(jb)
            .ok_or(StorageError::CommandWindowCapacity)?,
    ))
}
pub(super) fn maintain(
    db: &Connection,
    logical: u64,
    regression: bool,
) -> Result<(), StorageError> {
    db.execute("UPDATE command_window_legacy_meta SET closed=CASE WHEN ?1 OR ?2>=deadline THEN 1 ELSE closed END WHERE singleton=1",params![regression,logical]).map_err(sql_error)?;
    let (closed, cursor): (bool, String) = db
        .query_row(
            "SELECT closed,cursor FROM command_window_legacy_meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql_error)?;
    if !closed {
        return Ok(());
    }
    let query = format!(
        "SELECT key,value FROM documents WHERE ({}) AND key>?1 ORDER BY key LIMIT 64",
        predicate()
    );
    let rows: Vec<(String, Vec<u8>)> = {
        let mut q = db.prepare(&query).map_err(sql_error)?;
        let rows = q
            .query_map([&cursor], |r| {
                Ok((r.get(0)?, crate::read_limited_blob(r, 1)?))
            })
            .map_err(sql_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(sql_error)?;
        rows
    };
    let mut counted = 0;
    for (key, bytes) in &rows {
        let (slot, _) = catalog::legacy(key).ok_or(StorageError::Database)?;
        if slot.family == CommandFamily::ProfileContent && slot.prefix.ends_with("/receipts/") {
            let ad = crate::business::profile_content::read_owner_admission(
                &crate::business::codec::value(bytes)?,
            )?;
            if crate::business::profile_content::owner_is_held(&TransactionView(db), &ad, bytes)? {
                continue;
            }
        }
        // Voice physical ownership lives in its independently durable job state.
        // Keep a legacy result until that original job has actually returned.
        if slot.family == CommandFamily::Voice {
            let (_, id) = catalog::legacy(key).ok_or(StorageError::Database)?;
            if let Some(snapshot) = TransactionView(db).get("business/voice/v1")? {
                if crate::business::voice::command_window_holds(&snapshot.value)?
                    .iter()
                    .any(|(old, held)| *old == id && *held)
                {
                    continue;
                }
            }
        }
        db.execute("DELETE FROM documents WHERE key=?1", [key])
            .map_err(sql_error)?;
        db.execute("DELETE FROM command_window_legacy_docs WHERE key=?1", [key])
            .map_err(sql_error)?;
        counted += u64::from(slot.counted);
    }
    crate::business::profile_content::retire_window_receipts(db, counted)?;
    db.execute("DELETE FROM command_window_legacy_journals WHERE command_id IN(SELECT command_id FROM command_journal ORDER BY command_id LIMIT 64)",[]).map_err(sql_error)?;
    db.execute("DELETE FROM command_journal WHERE command_id IN(SELECT command_id FROM command_journal ORDER BY command_id LIMIT 64)",[]).map_err(sql_error)?;
    let next = rows.last().map_or("", |(key, _)| key.as_str());
    db.execute(
        "UPDATE command_window_legacy_meta SET cursor=?1 WHERE singleton=1",
        [next],
    )
    .map_err(sql_error)?;
    Ok(())
}
