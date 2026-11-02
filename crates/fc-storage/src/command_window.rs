//! The command issuance ledger belongs to the existing sole SQLite Writer.
//! Its monotonic retirement watermark is committed before bounded physical GC.
//! A missing or retired issuance is never permission to execute again.
use crate::StorageError;
use fc_domain::ids::CommandId;
use fc_ports::command_window::{
    CommandFamily, CommandOwner, CommandTicket, IssuedCommand, COMMAND_WINDOW_MILLIS,
    MAX_WINDOW_BYTES, MAX_WINDOW_COMMANDS,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const ENTRY_BYTES: u64 = 256;
const GC_BATCH: i64 = 64;

/// Clock samples are supplied by one clock, not by a remote request. Injection
/// keeps restart and rollback behavior testable without sleeping or changing OS time.
pub trait WindowClock: Send + Sync {
    fn wall_millis(&self) -> Result<u64, StorageError>;
    fn monotonic_millis(&self) -> u64;
}
pub(crate) struct ProcessClock {
    pub(crate) monotonic: Arc<dyn fc_ports::MonotonicClock + Send + Sync>,
}
impl WindowClock for ProcessClock {
    fn wall_millis(&self) -> Result<u64, StorageError> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StorageError::CommandWindowClock)?;
        u64::try_from(elapsed.as_millis()).map_err(|_| StorageError::CommandWindowClock)
    }
    fn monotonic_millis(&self) -> u64 {
        self.monotonic.monotonic_millis()
    }
}

/// The trusted backend supplies fresh CSPRNG bytes. Issuance stays on the
/// Writer; clients can neither select the epoch nor register their own ticket.
#[derive(Clone, Copy)]
pub struct IssueCommand {
    pub family: CommandFamily,
    pub owner: CommandOwner,
    pub entropy: [u8; 24],
}

pub(crate) struct Window {
    clock: Arc<dyn WindowClock>,
    last_monotonic: u64,
    last_wall: u64,
    logical: u64,
    wall_anchor: u64,
    logical_anchor: u64,
}
impl Window {
    pub(crate) fn open(
        db: &mut Connection,
        clock: Arc<dyn WindowClock>,
    ) -> Result<Self, StorageError> {
        let wall = bounded_time(clock.wall_millis()?)?;
        let mono = clock.monotonic_millis();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS command_window_clock(singleton INTEGER PRIMARY KEY CHECK(singleton=1),logical INTEGER NOT NULL CHECK(logical>=0),wall INTEGER NOT NULL CHECK(wall>=0));
            CREATE TABLE IF NOT EXISTS command_window_meta (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1), epoch BLOB NOT NULL CHECK(length(epoch)=16),
            issued INTEGER NOT NULL CHECK(issued>=0), retired INTEGER NOT NULL CHECK(retired>=0 AND retired<=issued),
            logical INTEGER NOT NULL CHECK(logical>=0), wall INTEGER NOT NULL CHECK(wall>=0));
            CREATE TABLE IF NOT EXISTS command_window_entries (
            serial INTEGER PRIMARY KEY CHECK(serial>0), payload_id BLOB NOT NULL CHECK(length(payload_id)=8),
            family TEXT NOT NULL, owner BLOB NOT NULL CHECK(length(owner)=26),
            deadline INTEGER NOT NULL CHECK(deadline>=0), charged_bytes INTEGER NOT NULL CHECK(charged_bytes>=256),
            held INTEGER NOT NULL CHECK(held IN (0,1)), recorded INTEGER NOT NULL CHECK(recorded IN (0,1)), UNIQUE(payload_id));
            CREATE TABLE IF NOT EXISTS command_window_documents (
            key TEXT PRIMARY KEY, serial INTEGER NOT NULL REFERENCES command_window_entries(serial));
            CREATE INDEX IF NOT EXISTS command_window_documents_serial ON command_window_documents(serial);
            CREATE TABLE IF NOT EXISTS command_window_profile_owners (
            sequence INTEGER NOT NULL, serial INTEGER PRIMARY KEY NOT NULL REFERENCES command_window_entries(serial),
            admission BLOB NOT NULL UNIQUE, receipt_key TEXT UNIQUE NOT NULL);
            CREATE TABLE IF NOT EXISTS command_journal_v2(command_id TEXT PRIMARY KEY,fingerprint BLOB NOT NULL CHECK(length(fingerprint)=32),revision INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS command_window_journals (
            command_id TEXT PRIMARY KEY, serial INTEGER NOT NULL REFERENCES command_window_entries(serial));") .map_err(sql)?;
        prepare_profile_owner_schema(&tx)?;
        crate::typed_command::open(&tx)?;
        let old_meta = tx
            .query_row(
                "SELECT logical,wall FROM command_window_meta WHERE singleton=1",
                [],
                |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let old_clock: Option<(u64, u64)> = tx
            .query_row(
                "SELECT logical,wall FROM command_window_clock WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let old = old_clock.or(old_meta);
        let restart_regression = old.is_some_and(|(_, previous_wall)| wall < previous_wall);
        let logical = if let Some((logical, previous_wall)) = old {
            // A backwards clock across restart cannot prove remaining lifetime.
            // Retire all issued ids durably instead of extending their window.
            let logical = logical
                .checked_add(wall.saturating_sub(previous_wall))
                .ok_or(StorageError::CommandWindowClock)?;
            bounded_time(logical)?;
            tx.execute("UPDATE command_window_meta SET logical=?1,wall=?2,retired=CASE WHEN ?2<wall THEN issued ELSE retired END WHERE singleton=1", params![logical,wall]).map_err(sql)?;
            logical
        } else {
            wall
        };
        tx.execute("INSERT INTO command_window_clock VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET logical=excluded.logical,wall=excluded.wall",params![logical,wall]).map_err(sql)?;
        crate::command_window_legacy::open(&tx, logical)?;
        if restart_regression {
            tx.execute(
                "UPDATE command_window_legacy_meta SET closed=1 WHERE singleton=1",
                [],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        Ok(Self {
            clock,
            last_monotonic: mono,
            last_wall: wall,
            logical,
            wall_anchor: wall,
            logical_anchor: logical,
        })
    }

    /// This separate transaction is deliberately committed even if the following
    /// query/submission is expired. Dropping an error must not undo retirement.
    pub(crate) fn maintain(&mut self, db: &mut Connection) -> Result<(), StorageError> {
        let wall = bounded_time(self.clock.wall_millis()?)?;
        let mono = self.clock.monotonic_millis();
        let regression = mono < self.last_monotonic || wall < self.last_wall;
        let logical = self.sample_logical(wall, mono)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute("UPDATE command_window_meta SET logical=?1,wall=?2,retired=CASE WHEN ?3 THEN issued ELSE MAX(retired,COALESCE((SELECT MAX(serial) FROM command_window_entries WHERE deadline<=?1),0)) END WHERE singleton=1", params![logical,wall,regression]).map_err(sql)?;
        tx.execute(
            "UPDATE command_window_clock SET logical=?1,wall=?2 WHERE singleton=1",
            params![logical, wall],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        self.logical = logical;
        self.last_wall = wall;
        self.last_monotonic = mono;
        if regression {
            self.wall_anchor = wall;
            self.logical_anchor = logical;
        }
        // The grace closure is durable before any physical legacy cleanup.
        let legacy_tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        crate::command_window_legacy::maintain(&legacy_tx, logical, regression)?;
        legacy_tx.commit().map_err(sql)?;
        // A small, fixed batch bounds writer work. Rows not yet deleted remain
        // charged and cannot be reissued, even if capacity is temporarily full.
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let serials: Vec<u64> = {
            let mut query=tx.prepare("SELECT serial FROM command_window_entries WHERE held=0 AND serial<=(SELECT retired FROM command_window_meta WHERE singleton=1) ORDER BY serial LIMIT ?1").map_err(sql)?;
            let rows = query
                .query_map([GC_BATCH], |r| r.get(0))
                .map_err(sql)?
                .collect::<rusqlite::Result<_>>()
                .map_err(sql)?;
            rows
        };
        for serial in serials {
            let keys: Vec<String> = {
                let mut query = tx.prepare("SELECT key FROM command_window_documents WHERE serial=?1 ORDER BY key LIMIT 5").map_err(sql)?;
                let rows = query
                    .query_map([serial], |r| r.get(0))
                    .map_err(sql)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql)?;
                rows
            };
            if keys.len() > 4 {
                return Err(StorageError::Database);
            }
            let mut counted = 0;
            for key in keys {
                let (slot, ticket) =
                    crate::business::command_window::owned(&key).ok_or(StorageError::Database)?;
                if ticket.serial() != serial {
                    return Err(StorageError::Database);
                }
                counted += u64::from(slot.counted);
            }
            crate::business::profile_content::retire_window_receipts(&tx, counted)?;
            tx.execute("DELETE FROM documents WHERE key IN (SELECT key FROM command_window_documents WHERE serial=?1)", [serial]).map_err(sql)?;
            tx.execute(
                "DELETE FROM command_window_documents WHERE serial=?1",
                [serial],
            )
            .map_err(sql)?;
            tx.execute("DELETE FROM command_journal_v2 WHERE command_id IN (SELECT command_id FROM command_window_journals WHERE serial=?1)", [serial]).map_err(sql)?;
            tx.execute(
                "DELETE FROM command_window_journals WHERE serial=?1",
                [serial],
            )
            .map_err(sql)?;
            tx.execute(
                "DELETE FROM command_window_profile_owners WHERE serial=?1",
                [serial],
            )
            .map_err(sql)?;
            tx.execute(
                "DELETE FROM command_window_consumption WHERE serial=?1",
                [serial],
            )
            .map_err(sql)?;
            tx.execute(
                "DELETE FROM command_window_entries WHERE serial=?1",
                [serial],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }
    fn sample_logical(&self, wall: u64, mono: u64) -> Result<u64, StorageError> {
        let monotonic = self
            .logical
            .checked_add(mono.saturating_sub(self.last_monotonic))
            .ok_or(StorageError::CommandWindowClock)?;
        let wall = self
            .logical_anchor
            .checked_add(wall.saturating_sub(self.wall_anchor))
            .ok_or(StorageError::CommandWindowClock)?;
        bounded_time(monotonic.max(wall))
    }
    pub(crate) fn check(&self, db: &Connection, issued: IssuedCommand) -> Result<(), StorageError> {
        let wall = bounded_time(self.clock.wall_millis()?)?;
        let mono = self.clock.monotonic_millis();
        if wall < self.last_wall || mono < self.last_monotonic {
            return Err(StorageError::CommandWindowClock);
        }
        let now = self.sample_logical(wall, mono)?;
        check_at(db, issued, now)
    }
    pub(crate) fn check_legacy_journal(
        &self,
        db: &Connection,
        id: &str,
    ) -> Result<(), StorageError> {
        let wall = bounded_time(self.clock.wall_millis()?)?;
        crate::command_window_legacy::check_journal(
            db,
            self.sample_logical(wall, self.clock.monotonic_millis())?,
            id,
        )
    }
    pub(crate) fn check_legacy(
        &self,
        db: &Connection,
        family: CommandFamily,
        id: CommandId,
    ) -> Result<(), StorageError> {
        let wall = bounded_time(self.clock.wall_millis()?)?;
        let now = self.sample_logical(wall, self.clock.monotonic_millis())?;
        crate::command_window_legacy::check(db, now, family, id)
    }
    pub(crate) fn issue(
        &mut self,
        db: &mut Connection,
        request: IssueCommand,
    ) -> Result<IssuedCommand, StorageError> {
        self.maintain(db)?;
        let epoch: [u8; 16] = request.entropy[..16]
            .try_into()
            .map_err(|_| StorageError::InvalidCommand)?;
        let payload = u64::from_be_bytes(
            request.entropy[16..]
                .try_into()
                .map_err(|_| StorageError::InvalidCommand)?,
        );
        let command_id = CommandId::new(payload).map_err(|_| StorageError::InvalidCommand)?;
        if epoch == [0; 16] {
            return Err(StorageError::InvalidCommand);
        }
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute(
            "INSERT OR IGNORE INTO command_window_meta VALUES(1,?1,0,0,?2,?3)",
            params![epoch.as_slice(), self.logical, self.last_wall],
        )
        .map_err(sql)?;
        let (epoch, issued): (Vec<u8>, u64) = tx
            .query_row(
                "SELECT epoch,issued FROM command_window_meta WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql)?;
        let (count, bytes): (u64, u64) = tx
            .query_row(
                "SELECT COUNT(*),COALESCE(SUM(charged_bytes),0) FROM command_window_entries",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql)?;
        let (legacy_count, legacy_bytes) = crate::command_window_legacy::usage(&tx)?;
        if count
            .checked_add(legacy_count)
            .is_none_or(|n| n >= MAX_WINDOW_COMMANDS)
            || bytes
                .checked_add(legacy_bytes)
                .ok_or(StorageError::CommandWindowCapacity)?
                .checked_add(ENTRY_BYTES)
                .is_none_or(|n| n > MAX_WINDOW_BYTES)
        {
            return Err(StorageError::CommandWindowCapacity);
        }
        let serial = issued
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or(StorageError::CommandWindowCapacity)?;
        let ticket = CommandTicket::new(
            epoch.try_into().map_err(|_| StorageError::Database)?,
            serial,
        )
        .map_err(|_| StorageError::Database)?;
        let collision: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM command_window_entries WHERE payload_id=?1)",
                [payload.to_be_bytes().as_slice()],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if collision {
            return Err(StorageError::CommandIdReused);
        }
        let deadline = bounded_time(
            self.logical
                .checked_add(COMMAND_WINDOW_MILLIS)
                .ok_or(StorageError::CommandWindowClock)?,
        )?;
        tx.execute(
            "INSERT INTO command_window_entries VALUES(?1,?2,?3,?4,?5,?6,0,0)",
            params![
                serial,
                payload.to_be_bytes().as_slice(),
                request.family.name(),
                request.owner.binding().as_slice(),
                deadline,
                ENTRY_BYTES
            ],
        )
        .map_err(sql)?;
        tx.execute(
            "UPDATE command_window_meta SET issued=?1 WHERE singleton=1",
            [serial],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(IssuedCommand {
            ticket,
            command_id,
            family: request.family,
            owner: request.owner,
            window_millis: COMMAND_WINDOW_MILLIS,
        })
    }
}

/// Called within the same transaction that reads a receipt or writes business
/// state. A retired watermark dominates even an undeleted receipt/index row.
/// Owner association uses the original full admission and server sequence, never
/// a raw client-id search across epochs. Missing v2 owner is allowed only for an
/// actual legacy receipt at its exact legacy key.
fn validate_profile_owner(
    db: &Connection,
    serial: u64,
    admission: &fc_domain::profile_content::ContentAdmission,
    key: &str,
) -> Result<(), StorageError> {
    let Some((slot, ticket)) = crate::business::command_window::owned(key) else {
        return Err(StorageError::Database);
    };
    if slot.family != CommandFamily::ProfileContent
        || !slot.prefix.ends_with("/receipts/")
        || ticket.serial() != serial
    {
        return Err(StorageError::Database);
    }
    type OwnerRow = (Vec<u8>, String, Vec<u8>, Vec<u8>);
    let row:Option<OwnerRow>=db.query_row("SELECT e.payload_id,e.family,e.owner,d.value FROM command_window_entries e JOIN command_window_documents w ON w.serial=e.serial JOIN documents d ON d.key=w.key WHERE e.serial=?1 AND w.key=?2",params![serial,key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,crate::read_limited_blob(r,3)?))).optional().map_err(sql)?;
    let (raw, family, owner, receipt) = row.ok_or(StorageError::Database)?;
    if raw.as_slice() != admission.request.command_id.get().to_be_bytes()
        || family != CommandFamily::ProfileContent.name()
        || owner.len() != 26
        || owner[0] != 1
        || owner[1..9] != admission.actor_id.get().to_be_bytes()
        || owner[17..25] != admission.auth_epoch.get().to_be_bytes()
        || owner[25] != 1
    {
        return Err(StorageError::Database);
    }
    if crate::business::profile_content::read_owner_admission(&crate::business::codec::value(
        &receipt,
    )?)? != *admission
    {
        return Err(StorageError::Database);
    }
    Ok(())
}
/// The previous unpublished candidate used a slot-local sequence as a global
/// primary key. Preserve every original owner, original receipt key/C2 scope,
/// admission byte and serial when replacing that exact known schema.
fn prepare_profile_owner_schema(db: &Connection) -> Result<(), StorageError> {
    let columns: Vec<(String, String, i64, i64)> = {
        let mut q = db
            .prepare("PRAGMA table_info(command_window_profile_owners)")
            .map_err(sql)?;
        let rows = q
            .query_map([], |r| Ok((r.get(1)?, r.get(2)?, r.get(3)?, r.get(5)?)))
            .map_err(sql)?
            .collect::<rusqlite::Result<_>>()
            .map_err(sql)?;
        rows
    };
    let old = vec![
        ("sequence".to_owned(), "INTEGER".to_owned(), 0, 1),
        ("serial".to_owned(), "INTEGER".to_owned(), 1, 0),
        ("admission".to_owned(), "BLOB".to_owned(), 1, 0),
        ("receipt_key".to_owned(), "TEXT".to_owned(), 1, 0),
    ];
    let new = vec![
        ("sequence".to_owned(), "INTEGER".to_owned(), 1, 0),
        ("serial".to_owned(), "INTEGER".to_owned(), 1, 1),
        ("admission".to_owned(), "BLOB".to_owned(), 1, 0),
        ("receipt_key".to_owned(), "TEXT".to_owned(), 1, 0),
    ];
    if columns == new {
        return Ok(());
    }
    if columns != old {
        return Err(StorageError::Database);
    }
    let mut count = 0u64;
    {
        let mut q=db.prepare("SELECT sequence,serial,admission,receipt_key FROM command_window_profile_owners ORDER BY serial").map_err(sql)?;
        let mut rows = q.query([]).map_err(sql)?;
        while let Some(row) = rows.next().map_err(sql)? {
            count = count
                .checked_add(1)
                .filter(|n| *n <= MAX_WINDOW_COMMANDS)
                .ok_or(StorageError::Database)?;
            let sequence: u64 = row.get(0).map_err(sql)?;
            let serial: u64 = row.get(1).map_err(sql)?;
            let bytes = crate::read_limited_blob(row, 2).map_err(sql)?;
            let key: String = row.get(3).map_err(sql)?;
            let admission = crate::business::profile_content::read_admission_bytes(&bytes)?;
            if admission.sequence != sequence
                || crate::business::profile_content::owner_admission_bytes(&admission)? != bytes
            {
                return Err(StorageError::Database);
            }
            validate_profile_owner(db, serial, &admission, &key)?;
        }
    }
    db.execute_batch("CREATE TABLE command_window_profile_owners_next(sequence INTEGER NOT NULL,serial INTEGER PRIMARY KEY NOT NULL REFERENCES command_window_entries(serial),admission BLOB NOT NULL UNIQUE,receipt_key TEXT UNIQUE NOT NULL);INSERT INTO command_window_profile_owners_next SELECT sequence,serial,admission,receipt_key FROM command_window_profile_owners;").map_err(sql)?;
    let copied: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM command_window_profile_owners_next",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if copied != count {
        return Err(StorageError::Database);
    }
    db.execute_batch("DROP TABLE command_window_profile_owners;ALTER TABLE command_window_profile_owners_next RENAME TO command_window_profile_owners;").map_err(sql)?;
    Ok(())
}
pub(crate) fn profile_content_owner_key(
    db: &Connection,
    admission: &fc_domain::profile_content::ContentAdmission,
) -> Result<String, StorageError> {
    let present:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='command_window_profile_owners')",[],|r|r.get(0)).map_err(sql)?;
    if !present {
        return Ok(format!(
            "business/profile-content/v1/receipts/{:020}",
            admission.request.command_id.get()
        ));
    }
    let bytes = crate::business::profile_content::owner_admission_bytes(admission)?;
    let row: Option<(u64, String)> = db
        .query_row(
            "SELECT serial,receipt_key FROM command_window_profile_owners WHERE admission=?1",
            [bytes],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    if let Some((serial, key)) = row {
        validate_profile_owner(db, serial, admission, &key)?;
        Ok(key)
    } else {
        Ok(format!(
            "business/profile-content/v1/receipts/{:020}",
            admission.request.command_id.get()
        ))
    }
}
/// Reconcile physical preparation ownership after mutations have really been
/// applied inside this same SQL transaction. Client TTL never returns an owner.
pub(crate) fn sync_profile_content_owners(
    db: &Connection,
    changed: &[String],
) -> Result<(), StorageError> {
    let mut admissions = std::collections::BTreeMap::new();
    for key in changed {
        if let Some((slot, _)) = crate::business::command_window::owned(key) {
            if slot.family == CommandFamily::ProfileContent && slot.prefix.ends_with("/receipts/") {
                let row: Option<Vec<u8>> = db
                    .query_row(
                        "SELECT admission FROM command_window_profile_owners WHERE receipt_key=?1",
                        [key],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql)?;
                let bytes = row.ok_or(StorageError::Database)?;
                let ad = crate::business::profile_content::read_admission_bytes(&bytes)?;
                admissions.insert(
                    crate::business::profile_content::owner_admission_bytes(&ad)?,
                    ad,
                );
            }
        } else if let Some(ad) = crate::business::profile_content::media_entry_admission(
            &crate::TransactionView(db),
            key,
        )? {
            admissions.insert(
                crate::business::profile_content::owner_admission_bytes(&ad)?,
                ad,
            );
        }
    }
    for admission in admissions.into_values() {
        let row: Option<(u64, String)> = db
            .query_row(
                "SELECT serial,receipt_key FROM command_window_profile_owners WHERE admission=?1",
                [crate::business::profile_content::owner_admission_bytes(
                    &admission,
                )?],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let Some((serial, key)) = row else { continue }; // Original legacy reservation.
        if profile_content_owner_key(db, &admission)? != key {
            return Err(StorageError::Database);
        }
        let value: Vec<u8> = db
            .query_row("SELECT value FROM documents WHERE key=?1", [&key], |r| {
                crate::read_limited_blob(r, 0)
            })
            .map_err(sql)?;
        let held = crate::business::profile_content::owner_is_held(
            &crate::TransactionView(db),
            &admission,
            &value,
        )?;
        db.execute(
            "UPDATE command_window_entries SET held=?1 WHERE serial=?2",
            params![held, serial],
        )
        .map_err(sql)?;
    }
    Ok(())
}
fn profile_negative_receipt(
    db: &Connection,
    key: &str,
    value: Option<&[u8]>,
) -> Result<bool, StorageError> {
    let Some((slot, ticket)) = crate::business::command_window::owned(key) else {
        return Ok(false);
    };
    if slot.family != CommandFamily::ProfileContent || !slot.prefix.ends_with("/receipts/") {
        return Ok(false);
    }
    let Some(value) = value else { return Ok(false) };
    let old:Vec<u8>=db.query_row("SELECT d.value FROM documents d JOIN command_window_documents w ON w.key=d.key WHERE w.key=?1 AND w.serial=?2",params![key,ticket.serial()],|r|crate::read_limited_blob(r,0)).map_err(sql)?;
    crate::business::profile_content::validate_negative_receipt(&old, value)?;
    let ad = crate::business::profile_content::read_owner_admission(
        &crate::business::codec::value(value)?,
    )?;
    if profile_content_owner_key(db, &ad)? != key {
        return Err(StorageError::Database);
    }
    Ok(true)
}
pub(crate) fn register_profile_negative_receipts(
    db: &Connection,
    mutations: &[crate::Mutation],
) -> Result<(), StorageError> {
    for mutation in mutations {
        let crate::Mutation::Put { key, value } = mutation else {
            continue;
        };
        if profile_negative_receipt(db, key, Some(value))? {
            let (_, ticket) =
                crate::business::command_window::owned(key).ok_or(StorageError::Database)?;
            let old: u64 = db
                .query_row(
                    "SELECT length(value) FROM documents WHERE key=?1",
                    [key],
                    |r| r.get(0),
                )
                .map_err(sql)?;
            let delta = i64::try_from(value.len()).map_err(|_| StorageError::Database)?
                - i64::try_from(old).map_err(|_| StorageError::Database)?;
            // Accepted -> Failed removes bytes; negative return never asks for
            // new capacity and cannot consume another command's reserved budget.
            if delta > 2 {
                return Err(StorageError::Database);
            }
            let delta = delta.min(0); // Four precharged bytes cover terminal status growth.
            db.execute(
                "UPDATE command_window_entries SET charged_bytes=charged_bytes+?1 WHERE serial=?2",
                params![delta, ticket.serial()],
            )
            .map_err(sql)?;
        }
    }
    Ok(())
}

/// Closing a namespace is part of the original business COMMIT. The hash is
/// public namespace metadata, not authority. It is rooted in the actual CURRENT
/// live epoch/high-water, durable business revision and recovery generation.
pub(crate) fn retire_epoch(
    db: &Connection,
    boundary: crate::CommandRetirement,
    source: &[u8],
) -> Result<(), StorageError> {
    use sha2::{Digest, Sha256};
    let domain = match boundary {
        crate::CommandRetirement::IdentityRecovery => b"identity-recovery".as_slice(),
        crate::CommandRetirement::DatabaseRestoreBarrier => b"database-restore".as_slice(),
        crate::CommandRetirement::None => return Err(StorageError::InvalidCommand),
    };
    let old: Option<(Vec<u8>, u64)> = db
        .query_row(
            "SELECT epoch,issued FROM command_window_meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    let Some((epoch, issued)) = old else {
        // No v2 issuance can exist without meta. Still permanently close legacy.
        db.execute(
            "UPDATE command_window_legacy_meta SET closed=1 WHERE singleton=1",
            [],
        )
        .map_err(sql)?;
        return Ok(());
    };
    let view = crate::TransactionView(db);
    let generation = crate::business::recovery::generation(&view)?;
    let revision = crate::read_revision(db)?;
    let mut hash = Sha256::new();
    hash.update(b"FutureClassroom-command-epoch-v2\0");
    hash.update(domain);
    hash.update(epoch);
    hash.update(issued.to_le_bytes());
    hash.update(generation.to_le_bytes());
    hash.update(revision.get().to_le_bytes());
    hash.update(source);
    let digest = hash.finalize();
    let next = &digest[..16];
    if next == [0; 16] {
        return Err(StorageError::CommandWindowClock);
    }
    db.execute(
        "UPDATE command_window_meta SET epoch=?1,retired=issued WHERE singleton=1",
        [next],
    )
    .map_err(sql)?;
    db.execute(
        "UPDATE command_window_legacy_meta SET closed=1 WHERE singleton=1",
        [],
    )
    .map_err(sql)?;
    Ok(())
}
/// Only a quiesced restore candidate may discard closed client history. Any
/// persisted original owner still held makes preparation fail; no TTL or new
/// receipt can settle it. CURRENT live state remains untouched on failure.
pub(crate) enum RestoreProjection {
    Preview,
    Confirmed,
}
pub(crate) fn retire_restore_candidate(
    db: &Connection,
    source: &[u8],
    projection: RestoreProjection,
) -> Result<(), StorageError> {
    let held: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM command_window_entries WHERE held=1)",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if held && matches!(projection, RestoreProjection::Confirmed) {
        return Err(StorageError::RecoveryConflict);
    }
    retire_epoch(db, crate::CommandRetirement::DatabaseRestoreBarrier, source)?;
    db.execute(
        "DELETE FROM documents WHERE key IN(SELECT key FROM command_window_documents)",
        [],
    )
    .map_err(sql)?;
    db.execute_batch("DELETE FROM command_window_consumption;DELETE FROM command_window_profile_owners;DELETE FROM command_window_documents;DELETE FROM command_window_journals;DELETE FROM command_journal_v2;DELETE FROM command_window_entries;DELETE FROM command_window_legacy_docs;DELETE FROM command_window_legacy_journals;").map_err(sql)?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn check(db: &Connection, issued: IssuedCommand) -> Result<(), StorageError> {
    check_at(db, issued, 0)
}
fn check_at(db: &Connection, issued: IssuedCommand, sampled: u64) -> Result<(), StorageError> {
    let meta: Option<(Vec<u8>, u64, u64, u64)> = db
        .query_row(
            "SELECT epoch,issued,retired,logical FROM command_window_meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(sql)?;
    let Some((epoch, highest, retired, logical)) = meta else {
        return Err(StorageError::CommandNotIssued);
    };
    if epoch.as_slice() != issued.ticket.epoch() || issued.ticket.serial() <= retired {
        return Err(StorageError::CommandExpired);
    }
    if issued.ticket.serial() > highest {
        return Err(StorageError::CommandNotIssued);
    }
    let row: Option<(Vec<u8>, String, Vec<u8>, u64)> = db
        .query_row(
            "SELECT payload_id,family,owner,deadline FROM command_window_entries WHERE serial=?1",
            [issued.ticket.serial()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(sql)?;
    let (payload, family, owner, deadline) = row.ok_or(StorageError::Database)?;
    if deadline <= logical.max(sampled) {
        return Err(StorageError::CommandExpired);
    }
    if payload.as_slice() != issued.command_id.get().to_be_bytes()
        || family != issued.family.name()
        || owner.as_slice() != issued.owner.binding()
    {
        return Err(StorageError::CommandNotIssued);
    }
    if crate::typed_command::raw_retired(db, issued.ticket.serial())? {
        return Err(StorageError::CommandExpired);
    }
    Ok(())
}

/// Account the actual stored generic receipt and its ownership before COMMIT.
/// A capacity error rolls back the business mutation and journal together.
pub(crate) fn register_journal(
    db: &Connection,
    issued: IssuedCommand,
    id: &str,
) -> Result<(), StorageError> {
    let charge = u64::try_from(id.len()).map_err(|_| StorageError::CommandWindowCapacity)? + 64;
    let total: u64 = db
        .query_row(
            "SELECT COALESCE(SUM(charged_bytes),0) FROM command_window_entries",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if total
        .checked_add(charge)
        .is_none_or(|n| n > MAX_WINDOW_BYTES)
    {
        return Err(StorageError::CommandWindowCapacity);
    }
    db.execute(
        "INSERT INTO command_window_journals VALUES(?1,?2)",
        params![id, issued.ticket.serial()],
    )
    .map_err(sql)?;
    db.execute(
        "UPDATE command_window_entries SET charged_bytes=charged_bytes+?1,recorded=1 WHERE serial=?2",
        params![charge, issued.ticket.serial()],
    )
    .map_err(sql)?;
    Ok(())
}
fn bounded_time(value: u64) -> Result<u64, StorageError> {
    if value > i64::MAX as u64 {
        Err(StorageError::CommandWindowClock)
    } else {
        Ok(value)
    }
}
fn sql(_: rusqlite::Error) -> StorageError {
    StorageError::Database
}

/// Namespace only exact catalog entries. Business payload JSON and actor
/// fingerprints stay byte-for-byte unchanged. Ownership/charge are in the same
/// transaction as the corresponding receipt/result and domain state.
pub(crate) fn register_documents(
    db: &Connection,
    issued: IssuedCommand,
    mutations: &mut [crate::Mutation],
) -> Result<(), StorageError> {
    use crate::{business::command_window as catalog, Mutation};
    let mut added: i64 = 0;
    let mut owned = 0;
    for mutation in mutations {
        let (key, value) = match mutation {
            Mutation::Put { key, value } => (key, Some(value)),
            Mutation::Delete { key } => (key, None),
        };
        let Some((slot, id)) = catalog::legacy(key) else {
            // Business tasks must request their own numeric id; accepting an
            // already namespaced foreign key would let a task alias a receipt.
            if catalog::owned(key).is_some()
                && !profile_negative_receipt(db, key, value.map(|bytes| bytes.as_slice()))?
            {
                return Err(StorageError::CommandIdReused);
            }
            continue;
        };
        if value.is_none() && slot.family == CommandFamily::Voice && id != issued.command_id {
            continue;
        }
        if slot.family != issued.family || id != issued.command_id {
            return Err(StorageError::CommandIdReused);
        }
        owned += 1;
        if owned > 4 {
            return Err(StorageError::InvalidCommand);
        }
        *key = catalog::namespaced(slot, issued.ticket);
        let previous:Option<(u64,u64)>=db.query_row("SELECT w.serial,length(d.value) FROM command_window_documents w JOIN documents d ON d.key=w.key WHERE w.key=?1",[key.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
        if previous.is_some_and(|(serial, _)| serial != issued.ticket.serial()) {
            return Err(StorageError::Database);
        }
        let base =
            i64::try_from(key.len() + 32).map_err(|_| StorageError::CommandWindowCapacity)?;
        if let Some((_, old)) = previous {
            added -= base + i64::try_from(old).map_err(|_| StorageError::Database)?;
        }
        if let Some(value) = value {
            if slot.family == CommandFamily::ProfileContent && slot.prefix.ends_with("/receipts/") {
                let raw = crate::business::codec::value(value)?;
                let admission = crate::business::profile_content::read_owner_admission(&raw)?;
                let admission_bytes =
                    serde_json::to_vec(&raw["admission"]).map_err(|_| StorageError::Database)?;
                let previous_owner: Option<(Vec<u8>,String)> = db.query_row("SELECT admission,receipt_key FROM command_window_profile_owners WHERE serial=?1", [issued.ticket.serial()], |r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
                if let Some((bytes, stored_key)) = previous_owner {
                    if bytes != admission_bytes || stored_key != *key {
                        return Err(StorageError::Database);
                    }
                } else {
                    added = added
                        .checked_add(4)
                        .ok_or(StorageError::CommandWindowCapacity)?;
                    db.execute(
                        "INSERT INTO command_window_profile_owners VALUES(?1,?2,?3,?4)",
                        params![
                            admission.sequence,
                            issued.ticket.serial(),
                            admission_bytes,
                            key.as_str()
                        ],
                    )
                    .map_err(sql)?;
                    added = added
                        .checked_add(
                            i64::try_from(admission_bytes.len() + key.len() + 64)
                                .map_err(|_| StorageError::CommandWindowCapacity)?,
                        )
                        .ok_or(StorageError::CommandWindowCapacity)?;
                }
                let held =
                    raw.get("status").and_then(serde_json::Value::as_str) == Some("accepted");
                db.execute(
                    "UPDATE command_window_entries SET held=?1 WHERE serial=?2",
                    params![held, issued.ticket.serial()],
                )
                .map_err(sql)?;
            }
            added = added
                .checked_add(
                    base + i64::try_from(value.len())
                        .map_err(|_| StorageError::CommandWindowCapacity)?,
                )
                .ok_or(StorageError::CommandWindowCapacity)?;
            db.execute("INSERT INTO command_window_documents(key,serial) VALUES(?1,?2) ON CONFLICT(key) DO NOTHING",params![key.as_str(),issued.ticket.serial()]).map_err(sql)?;
        } else {
            db.execute(
                "DELETE FROM command_window_documents WHERE key=?1",
                [key.as_str()],
            )
            .map_err(sql)?;
        }
    }
    let count: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM command_window_documents WHERE serial=?1",
            [issued.ticket.serial()],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if count > 4 {
        return Err(StorageError::InvalidCommand);
    }
    let total: i64 = db
        .query_row(
            "SELECT COALESCE(SUM(charged_bytes),0) FROM command_window_entries",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    let (_, legacy_bytes) = crate::command_window_legacy::usage(db)?;
    if total
        .checked_add(i64::try_from(legacy_bytes).map_err(|_| StorageError::CommandWindowCapacity)?)
        .and_then(|n| n.checked_add(added))
        .is_none_or(|n| n < 0 || n > MAX_WINDOW_BYTES as i64)
    {
        return Err(StorageError::CommandWindowCapacity);
    }
    if owned > 0 {
        db.execute(
            "UPDATE command_window_entries SET charged_bytes=charged_bytes+?1,recorded=1 WHERE serial=?2",
            params![added, issued.ticket.serial()],
        )
        .map_err(sql)?;
    }
    Ok(())
}

pub(crate) fn check_document(
    db: &Connection,
    issued: IssuedCommand,
    key: &str,
) -> Result<(), StorageError> {
    let recorded: bool = db
        .query_row(
            "SELECT recorded FROM command_window_entries WHERE serial=?1",
            [issued.ticket.serial()],
            |r| r.get(0),
        )
        .map_err(sql)?;
    let mapping: Option<u64> = db
        .query_row(
            "SELECT serial FROM command_window_documents WHERE key=?1",
            [key],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE key=?1)",
            [key],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if recorded != mapping.is_some()
        || mapping.is_some_and(|n| n != issued.ticket.serial())
        || mapping.is_some() != exists
    {
        return Err(StorageError::Database);
    }
    Ok(())
}
pub(crate) fn check_journal(
    db: &Connection,
    issued: IssuedCommand,
    exists: bool,
) -> Result<(), StorageError> {
    let recorded: bool = db
        .query_row(
            "SELECT recorded FROM command_window_entries WHERE serial=?1",
            [issued.ticket.serial()],
            |r| r.get(0),
        )
        .map_err(sql)?;
    let mapped:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM command_window_journals WHERE serial=?1 AND command_id=?2)",params![issued.ticket.serial(),issued.ticket.to_string()],|r|r.get(0)).map_err(sql)?;
    if recorded != exists || recorded != mapped {
        return Err(StorageError::Database);
    }
    Ok(())
}

/// Original business state is the source of physical ownership. This runs for
/// internal driver/settlement transactions too, without making them client
/// retries and without requiring a still-live client issuance.
pub(crate) fn sync_owner_holds(
    db: &Connection,
    mutations: &[crate::Mutation],
    issued: Option<IssuedCommand>,
    revision: i64,
) -> Result<(), StorageError> {
    for mutation in mutations {
        let crate::Mutation::Put { key, value } = mutation else {
            continue;
        };
        if key == "business/voice/v1" {
            sync_voice_bindings(db, value, mutations, issued, revision)?;
        }
        if key == "business/autostart/v1" {
            sync_autostart_bindings(db, value, issued, revision)?;
        }
        let (family, holds) = match key.as_str() {
            "business/voice/v1" => (
                CommandFamily::Voice,
                crate::business::voice::command_window_holds(value)?,
            ),
            "business/autostart/v1" => (
                CommandFamily::Autostart,
                crate::business::autostart::command_window_holds(value)?,
            ),
            _ => continue,
        };
        for (id, held) in holds {
            db.execute(
                "UPDATE command_window_entries SET held=?1 WHERE family=?2 AND payload_id=?3",
                params![held, family.name(), id.get().to_be_bytes().as_slice()],
            )
            .map_err(sql)?;
        }
    }
    Ok(())
}

fn sync_autostart_bindings(
    db: &Connection,
    bytes: &[u8],
    issued: Option<IssuedCommand>,
    revision: i64,
) -> Result<(), StorageError> {
    use crate::business::autostart;
    let operations = autostart::command_window_operations(bytes)?;
    let expected = operations
        .iter()
        .map(|o| autostart::command_binding_key(&o.ticket))
        .collect::<std::collections::BTreeSet<_>>();
    let existing: Vec<String> = {
        let mut q = db
            .prepare("SELECT key FROM documents WHERE key>=?1 AND key<?2 ORDER BY key LIMIT 65")
            .map_err(sql)?;
        let rows = q
            .query_map(
                params![
                    autostart::COMMAND_BINDING_PREFIX,
                    "business/security/autostart-command/v3/"
                ],
                |r| r.get(0),
            )
            .map_err(sql)?
            .collect::<rusqlite::Result<_>>()
            .map_err(sql)?;
        rows
    };
    if existing.len() > fc_domain::autostart::MAX_AUTOSTART_HISTORY {
        return Err(StorageError::Database);
    }
    for key in existing {
        if !expected.contains(&key) {
            db.execute("DELETE FROM documents WHERE key=?1", [key])
                .map_err(sql)?;
        }
    }
    if let Some(issued) = issued.filter(|i| i.family == CommandFamily::Autostart) {
        let operation = operations
            .iter()
            .find(|o| o.ticket.command_id == issued.command_id)
            .ok_or(StorageError::CommandNotIssued)?;
        let key = autostart::command_binding_key(&operation.ticket);
        let value = autostart::command_binding_value(&operation.ticket, issued)?;
        let previous: Option<Vec<u8>> = db
            .query_row("SELECT value FROM documents WHERE key=?1", [&key], |r| {
                crate::read_limited_blob(r, 0)
            })
            .optional()
            .map_err(sql)?;
        if let Some(previous) = previous {
            if previous != value {
                return Err(StorageError::CommandNotIssued);
            }
        } else {
            db.execute(
                "INSERT INTO documents VALUES(?1,?2,?3)",
                params![key, value, revision],
            )
            .map_err(sql)?;
        }
    }
    Ok(())
}

/// Business pruning never deletes a live issuance. The indexed original raw
/// receipt becomes an exact per-command tombstone in the same business TX.
/// Its base entry/count remain bounded until the normal client-window GC.
fn retire_voice_receipt(
    db: &Connection,
    job: &fc_domain::voice::VoiceJob,
    id: CommandId,
) -> Result<(), StorageError> {
    use crate::business::{command_window as catalog, voice};
    let entry: Option<(u64,u64,bool)> = db.query_row(
        "SELECT serial,charged_bytes,recorded FROM command_window_entries WHERE family=?1 AND payload_id=?2",
        params![CommandFamily::Voice.name(),id.get().to_be_bytes().as_slice()],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).optional().map_err(sql)?;
    // Legacy receipts, or already normally expired/GC'd C2 entries, have no
    // matching issuance. Their original ordinary prune deletes remain intact.
    let Some((serial, charged, recorded)) = entry else {
        return Ok(());
    };
    if !recorded {
        return Ok(());
    }
    let keys: Vec<String> = {
        let mut q = db
            .prepare(
                "SELECT key FROM command_window_documents WHERE serial=?1 ORDER BY key LIMIT 2",
            )
            .map_err(sql)?;
        let rows = q
            .query_map([serial], |r| r.get(0))
            .map_err(sql)?
            .collect::<rusqlite::Result<_>>()
            .map_err(sql)?;
        rows
    };
    if keys.len() != 1 {
        return Err(StorageError::Database);
    }
    let key = &keys[0];
    let (slot, ticket) = catalog::owned(key).ok_or(StorageError::Database)?;
    if slot.family != CommandFamily::Voice
        || slot.prefix != "business/voice-receipt/v1/"
        || ticket.serial() != serial
        || catalog::namespaced(slot, ticket) != *key
    {
        return Err(StorageError::Database);
    }
    let (epoch, retired): (Vec<u8>, u64) = db
        .query_row(
            "SELECT epoch,retired FROM command_window_meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql)?;
    if serial > retired && epoch.as_slice() != ticket.epoch() {
        return Err(StorageError::Database);
    }
    if id == job.command_id {
        let binding: Option<Vec<u8>> = db
            .query_row(
                "SELECT value FROM documents WHERE key=?1",
                [voice::command_binding_key(job.id)],
                |r| crate::read_limited_blob(r, 0),
            )
            .optional()
            .map_err(sql)?;
        if binding
            .as_deref()
            .map(voice::read_command_binding)
            .transpose()?
            .is_some_and(|original| original != ticket)
        {
            return Err(StorageError::Database);
        }
    }
    let value: Vec<u8> = db
        .query_row("SELECT value FROM documents WHERE key=?1", [key], |r| {
            crate::read_limited_blob(r, 0)
        })
        .map_err(sql)?;
    voice::command_window_receipt_matches(&value, job, id)?;
    let extra = u64::try_from(key.len())
        .map_err(|_| StorageError::Database)?
        .checked_add(32)
        .and_then(|n| n.checked_add(value.len() as u64))
        .ok_or(StorageError::Database)?;
    if charged.checked_sub(extra) != Some(ENTRY_BYTES) {
        return Err(StorageError::Database);
    }
    crate::typed_command::retire_raw(db, serial)?;
    if db
        .execute("DELETE FROM documents WHERE key=?1", [key])
        .map_err(sql)?
        != 1
        || db
            .execute(
                "DELETE FROM command_window_documents WHERE key=?1 AND serial=?2",
                params![key, serial],
            )
            .map_err(sql)?
            != 1
    {
        return Err(StorageError::Database);
    }
    db.execute(
        "UPDATE command_window_entries SET charged_bytes=?1,held=0 WHERE serial=?2",
        params![ENTRY_BYTES, serial],
    )
    .map_err(sql)?;
    Ok(())
}

fn sync_voice_bindings(
    db: &Connection,
    bytes: &[u8],
    mutations: &[crate::Mutation],
    issued: Option<IssuedCommand>,
    revision: i64,
) -> Result<(), StorageError> {
    use crate::business::voice;
    let previous: Option<Vec<u8>> = db
        .query_row(
            "SELECT value FROM documents WHERE key='business/voice/v1'",
            [],
            |r| crate::read_limited_blob(r, 0),
        )
        .optional()
        .map_err(sql)?;
    if let Some(previous) = previous {
        for job in voice::command_window_pruned_jobs(&previous, bytes, mutations)? {
            for id in std::iter::once(job.command_id).chain(job.cancel_command) {
                retire_voice_receipt(db, &job, id)?;
            }
        }
    }
    let jobs = voice::command_window_jobs(bytes)?;
    let keys = jobs
        .iter()
        .map(|job| voice::command_binding_key(job.id))
        .collect::<std::collections::BTreeSet<_>>();
    let existing: Vec<String> = {
        let mut query = db
            .prepare("SELECT key FROM documents WHERE key>=?1 AND key<?2 ORDER BY key LIMIT ?3")
            .map_err(sql)?;
        let rows = query
            .query_map(
                params![
                    voice::COMMAND_BINDING_PREFIX,
                    "business/voice-job-command/v3/",
                    fc_domain::voice::MAX_VOICE_JOBS + 1
                ],
                |r| r.get(0),
            )
            .map_err(sql)?
            .collect::<rusqlite::Result<_>>()
            .map_err(sql)?;
        rows
    };
    if existing.len() > fc_domain::voice::MAX_VOICE_JOBS {
        return Err(StorageError::Database);
    }
    for key in existing {
        if !keys.contains(&key) {
            db.execute("DELETE FROM documents WHERE key=?1", [key])
                .map_err(sql)?;
        }
    }
    if let Some(issued) = issued.filter(|issued| issued.family == CommandFamily::Voice) {
        for job in jobs
            .into_iter()
            .filter(|job| job.command_id == issued.command_id)
        {
            let key = voice::command_binding_key(job.id);
            let old: Option<Vec<u8>> = db
                .query_row(
                    "SELECT value FROM documents WHERE key=?1",
                    [key.as_str()],
                    |r| crate::read_limited_blob(r, 0),
                )
                .optional()
                .map_err(sql)?;
            if let Some(old) = old {
                if voice::read_command_binding(&old)? != issued.ticket {
                    return Err(StorageError::CommandIdReused);
                }
            } else {
                let value = crate::business::codec::bytes(
                    &serde_json::json!({"format":2,"ticket":issued.ticket.to_string()}),
                )?;
                db.execute(
                    "INSERT INTO documents VALUES(?1,?2,?3)",
                    params![key, value, revision],
                )
                .map_err(sql)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Clock {
        wall: AtomicU64,
        mono: AtomicU64,
    }
    impl Clock {
        fn new(wall: u64) -> Arc<Self> {
            Arc::new(Self {
                wall: AtomicU64::new(wall),
                mono: AtomicU64::new(0),
            })
        }
        fn advance(&self, delta: u64) {
            self.wall.fetch_add(delta, Ordering::SeqCst);
            self.mono.fetch_add(delta, Ordering::SeqCst);
        }
    }
    impl WindowClock for Clock {
        fn wall_millis(&self) -> Result<u64, StorageError> {
            Ok(self.wall.load(Ordering::SeqCst))
        }
        fn monotonic_millis(&self) -> u64 {
            self.mono.load(Ordering::SeqCst)
        }
    }
    fn db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE documents(key TEXT PRIMARY KEY,value BLOB NOT NULL,revision INTEGER NOT NULL); CREATE TABLE command_journal(command_id TEXT PRIMARY KEY,fingerprint BLOB,revision INTEGER);").unwrap();
        db
    }
    fn request(id: u64) -> IssueCommand {
        let mut entropy = [7; 24];
        entropy[16..].copy_from_slice(&id.to_be_bytes());
        IssueCommand {
            family: CommandFamily::Content,
            owner: CommandOwner::PublicDesktop,
            entropy,
        }
    }
    #[test]
    fn issued_unsubmitted_expires_and_cannot_reexecute_after_physical_deletion() {
        let mut db = db();
        let clock = Clock::new(1000);
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        let id = window.issue(&mut db, request(u64::MAX)).unwrap();
        assert_eq!(id.ticket.serial(), 1);
        assert_eq!(check(&db, id), Ok(()));
        clock.advance(COMMAND_WINDOW_MILLIS);
        window.maintain(&mut db).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM command_window_entries", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(check(&db, id), Err(StorageError::CommandExpired));
        let new = window.issue(&mut db, request(u64::MAX)).unwrap();
        assert_eq!(new.ticket.serial(), 2);
        assert_eq!(check(&db, id), Err(StorageError::CommandExpired));
        assert_ne!(new.ticket, id.ticket);
    }
    #[test]
    fn exact_family_owner_payload_and_epoch_are_checked_not_client_window_field() {
        let mut db = db();
        let clock = Clock::new(1000);
        let mut window = Window::open(&mut db, clock).unwrap();
        let id = window.issue(&mut db, request(1)).unwrap();
        let mut wrong = id;
        wrong.family = CommandFamily::Identity;
        assert_eq!(check(&db, wrong), Err(StorageError::CommandNotIssued));
        wrong = id;
        wrong.command_id = CommandId::new(2).unwrap();
        assert_eq!(check(&db, wrong), Err(StorageError::CommandNotIssued));
        wrong = id;
        wrong.ticket = CommandTicket::new([8; 16], 1).unwrap();
        assert_eq!(check(&db, wrong), Err(StorageError::CommandExpired));
        wrong = id;
        wrong.ticket = CommandTicket::new(id.ticket.epoch(), 2).unwrap();
        assert_eq!(check(&db, wrong), Err(StorageError::CommandNotIssued));
        wrong = id;
        wrong.window_millis = u64::MAX;
        assert_eq!(check(&db, wrong), Ok(()));
    }
    #[test]
    fn restart_preserves_original_age_and_wall_rollback_retires_without_renewal() {
        let mut db = db();
        let clock = Clock::new(1000);
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        let id = window.issue(&mut db, request(1)).unwrap();
        drop(window);
        clock
            .wall
            .store(1000 + COMMAND_WINDOW_MILLIS - 1, Ordering::SeqCst);
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        window.maintain(&mut db).unwrap();
        assert_eq!(check(&db, id), Ok(()));
        clock.advance(1);
        window.maintain(&mut db).unwrap();
        assert_eq!(check(&db, id), Err(StorageError::CommandExpired));
        let id = window.issue(&mut db, request(2)).unwrap();
        drop(window);
        clock.wall.store(1, Ordering::SeqCst);
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        window.maintain(&mut db).unwrap();
        assert_eq!(check(&db, id), Err(StorageError::CommandExpired));
        let fresh = window.issue(&mut db, request(3)).unwrap();
        assert_eq!(check(&db, fresh), Ok(()));
    }
    #[test]
    fn gc_is_bounded_and_watermark_dominates_not_yet_deleted_entries() {
        let mut db = db();
        let clock = Clock::new(1000);
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        let mut last = None;
        for n in 1..=65 {
            last = Some(window.issue(&mut db, request(n)).unwrap());
        }
        clock.advance(COMMAND_WINDOW_MILLIS);
        window.maintain(&mut db).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM command_window_entries", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            1
        );
        assert_eq!(check(&db, last.unwrap()), Err(StorageError::CommandExpired));
        window.maintain(&mut db).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM command_window_entries", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn outstanding_unsubmitted_and_byte_charges_both_prevent_issuance() {
        let mut db = db();
        let clock = Clock::new(1000);
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        window.issue(&mut db, request(1)).unwrap();
        db.execute(
            "UPDATE command_window_entries SET charged_bytes=?1",
            [MAX_WINDOW_BYTES],
        )
        .unwrap();
        assert_eq!(
            window.issue(&mut db, request(2)),
            Err(StorageError::CommandWindowCapacity)
        );
        assert_eq!(
            db.query_row("SELECT issued FROM command_window_meta", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            1
        );
        // Capacity applies to ids issued but never submitted as well. Bulk insert
        // avoids 65k filesystem transactions and tests the real SQL accounting.
        db.execute("UPDATE command_window_entries SET charged_bytes=256", [])
            .unwrap();
        db.execute_batch("WITH RECURSIVE ids(n) AS (SELECT 2 UNION ALL SELECT n+1 FROM ids WHERE n<65536) INSERT INTO command_window_entries SELECT n,CAST(printf('%08d',n) AS BLOB),'content',zeroblob(26),999999999,256,0,0 FROM ids;").unwrap();
        assert_eq!(
            window.issue(&mut db, request(2)),
            Err(StorageError::CommandWindowCapacity)
        );
    }
    struct Publisher;
    impl fc_ports::AuthorityPublisher for Publisher {
        type Snapshot = u64;
        type Update = ();
        type Error = ();
        fn snapshot_revision(&self, value: &u64) -> fc_domain::ids::Revision {
            fc_domain::ids::Revision::new(*value)
        }
        fn begin_update(&mut self) -> Result<(), ()> {
            Ok(())
        }
        fn publish(&mut self, _: (), _: u64) -> Result<(), ()> {
            Ok(())
        }
        fn rollback(&mut self, _: ()) -> Result<(), ()> {
            Ok(())
        }
        fn fault(&mut self) {}
    }
    impl fc_ports::MonotonicClock for Clock {
        fn monotonic_millis(&self) -> u64 {
            self.mono.load(Ordering::SeqCst)
        }
    }
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "fc-command-window-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> std::path::PathBuf {
            self.0.join("state.sqlite3")
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("exact command-window fixture cleanup");
        }
    }
    fn generic(id: CommandId, revision: u64, value: u8) -> crate::CommitCommand {
        crate::CommitCommand::new(
            id.get().to_string(),
            fc_domain::ids::Revision::new(revision),
            vec![crate::Mutation::Put {
                key: "counter".into(),
                value: vec![value],
            }],
        )
        .unwrap()
    }
    #[test]
    fn real_writer_generic_commit_replay_and_expiry_are_bound_to_full_ticket() {
        let temp = Temp::new();
        let clock = Clock::new(1000);
        let writer = crate::StorageWriter::open_with_clocks(
            temp.path(),
            8,
            Publisher,
            clock.clone(),
            clock.clone(),
        )
        .unwrap();
        let handle = writer.handle();
        let mut req = request(7);
        req.family = CommandFamily::Generic;
        let id = handle.try_issue_command(req).unwrap().wait().unwrap();
        let scoped = handle.with_command_window(id);
        assert!(
            !scoped
                .try_commit(generic(id.command_id, 0, 1), 1)
                .unwrap()
                .wait()
                .unwrap()
                .replayed
        );
        assert!(
            scoped
                .try_commit(generic(id.command_id, 0, 1), 1)
                .unwrap()
                .wait()
                .unwrap()
                .replayed
        );
        assert_eq!(
            scoped
                .try_commit(generic(id.command_id, 0, 2), 1)
                .unwrap()
                .wait(),
            Err(StorageError::CommandIdReused)
        );
        clock.advance(COMMAND_WINDOW_MILLIS);
        assert_eq!(
            handle.try_check_command(id).unwrap().wait(),
            Err(StorageError::CommandExpired)
        );
        assert_eq!(
            scoped
                .try_commit(generic(id.command_id, 1, 2), 2)
                .unwrap()
                .wait(),
            Err(StorageError::CommandExpired)
        );
        let next = handle.try_issue_command(req).unwrap().wait().unwrap();
        assert_ne!(next.ticket, id.ticket);
        assert!(
            !handle
                .with_command_window(next)
                .try_commit(generic(next.command_id, 1, 2), 2)
                .unwrap()
                .wait()
                .unwrap()
                .replayed
        );
        assert_eq!(
            scoped
                .try_commit(generic(id.command_id, 1, 2), 2)
                .unwrap()
                .wait(),
            Err(StorageError::CommandExpired)
        );
        writer.shutdown().unwrap();
        let db = Connection::open(temp.path()).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM command_journal_v2", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            1
        );
        drop(db);
    }
    struct ExpireDuringTask {
        clock: Arc<Clock>,
        write: bool,
    }
    impl crate::TransactionTask<u64> for ExpireDuringTask {
        fn allocation_bytes(&self) -> usize {
            std::mem::size_of::<Self>()
        }
        fn evaluate(
            self: Box<Self>,
            d: &dyn crate::TransactionDocuments,
        ) -> Result<crate::TransactionDecision<u64>, StorageError> {
            self.clock.advance(COMMAND_WINDOW_MILLIS);
            Ok(crate::TransactionDecision {
                precommit: None,
                outcome: b"must not escape".to_vec(),
                commit: if self.write {
                    Some((
                        vec![crate::Mutation::Put {
                            key: "must-not-commit".into(),
                            value: vec![1],
                        }],
                        d.revision()?.get() + 1,
                    ))
                } else {
                    None
                },
            })
        }
    }
    #[test]
    fn original_writer_checks_expiration_again_before_commit_or_positive_read_reply() {
        let temp = Temp::new();
        let clock = Clock::new(1000);
        let writer = crate::StorageWriter::open_with_clocks(
            temp.path(),
            8,
            Publisher,
            clock.clone(),
            clock.clone(),
        )
        .unwrap();
        let handle = writer.handle();
        for (raw, write) in [(1, true), (2, false)] {
            let id = handle
                .try_issue_command(request(raw))
                .unwrap()
                .wait()
                .unwrap();
            assert_eq!(
                handle
                    .with_command_window(id)
                    .try_transaction(ExpireDuringTask {
                        clock: clock.clone(),
                        write
                    })
                    .unwrap()
                    .wait(),
                Err(StorageError::CommandExpired)
            );
            assert_eq!(handle.try_revision().unwrap().wait().unwrap().get(), 0);
            assert!(handle
                .try_get("must-not-commit".into())
                .unwrap()
                .wait()
                .unwrap()
                .is_none());
        }
        writer.shutdown().unwrap();
    }
    #[test]
    fn expired_owned_receipts_decrement_only_the_exact_counter_and_preserve_heads() {
        let mut db = db();
        let clock = Clock::new(1000);
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        let mut req = request(1);
        req.family = CommandFamily::ProfileContent;
        let id = window.issue(&mut db, req).unwrap();
        db.execute(
            "INSERT INTO documents VALUES('business/profile-content/v1/counter',?1,1)",
            [br#"{"format":1,"heads":7,"receipts":2}"#.as_slice()],
        )
        .unwrap();
        db.execute("INSERT INTO documents VALUES('business/profile-content/v1/heads/existing',X'010203',1)",[]).unwrap();
        let mut mutations = vec![
            crate::Mutation::Put {
                key: format!("business/profile-content/v1/receipts/{:020}", 1),
                value: serde_json::to_vec(&serde_json::json!({"format":1,"admission":{"request":{"command":1,"slot":{"scope":null,"node":1},"expected":0,"operation":{"kind":"clear"},"fingerprint":vec![1;32],"expires":{"date":[2026,11,2],"second":7200}},"sequence":1,"actor":1,"epoch":1},"status":"accepted","snapshot":null,"digest":null})).unwrap(),
            },
            crate::Mutation::Put {
                key: format!("business/profile-content/v1/page-receipts/{:020}", 1),
                value: b"page result".to_vec(),
            },
        ];
        let tx = db.transaction().unwrap();
        register_documents(&tx, id, &mut mutations).unwrap();
        for mutation in mutations {
            if let crate::Mutation::Put { key, value } = mutation {
                tx.execute("INSERT INTO documents VALUES(?1,?2,1)", params![key, value])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
        // A still-owned admission must remain charged even after client expiry.
        db.execute("UPDATE command_window_entries SET held=1", [])
            .unwrap();
        clock.advance(COMMAND_WINDOW_MILLIS);
        window.maintain(&mut db).unwrap();
        assert_eq!(check(&db, id), Err(StorageError::CommandExpired));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM command_window_documents", [], |r| r
                .get::<_, u64>(
                0
            ))
            .unwrap(),
            2
        );
        // Model a separately completed negative settlement. This SQL fixture is
        // not a production permission to clear owners; the held GC predicate is
        // what this unit exercises, not physical worker-return authorization.
        db.execute("UPDATE command_window_entries SET held=0", [])
            .unwrap();
        window.maintain(&mut db).unwrap();
        let bytes: Vec<u8> = db
            .query_row(
                "SELECT value FROM documents WHERE key='business/profile-content/v1/counter'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let count: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(count["receipts"], 0);
        assert_eq!(count["heads"], 7);
        assert_eq!(db.query_row("SELECT value FROM documents WHERE key='business/profile-content/v1/heads/existing'",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),vec![1,2,3]);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM command_window_entries", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn legacy_max_id_has_finite_grace_missing_ids_never_execute_and_restart_does_not_renew() {
        let mut db = db();
        let clock = Clock::new(1000);
        let old = format!("business/receipt/content/{}", u64::MAX);
        db.execute("INSERT INTO documents VALUES(?1,X'01',1)", [&old])
            .unwrap();
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        assert_eq!(
            window.check_legacy(
                &db,
                CommandFamily::Content,
                CommandId::new(u64::MAX).unwrap()
            ),
            Ok(())
        );
        assert_eq!(
            window.check_legacy(&db, CommandFamily::Content, CommandId::new(17).unwrap()),
            Err(StorageError::CommandNotIssued)
        );
        let issued = window.issue(&mut db, request(1)).unwrap();
        assert_eq!(issued.ticket.serial(), 1); // Never max(legacy arbitrary u64)+1.
        clock.advance(COMMAND_WINDOW_MILLIS - 1);
        window = Window::open(&mut db, clock.clone()).unwrap();
        assert_eq!(
            window.check_legacy(
                &db,
                CommandFamily::Content,
                CommandId::new(u64::MAX).unwrap()
            ),
            Ok(())
        );
        clock.advance(1);
        window.maintain(&mut db).unwrap();
        assert_eq!(
            window.check_legacy(
                &db,
                CommandFamily::Content,
                CommandId::new(u64::MAX).unwrap()
            ),
            Err(StorageError::CommandExpired)
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM documents WHERE key=?1", [&old], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
        window = Window::open(&mut db, clock).unwrap();
        assert_eq!(
            window.check_legacy(
                &db,
                CommandFamily::Content,
                CommandId::new(u64::MAX).unwrap()
            ),
            Err(StorageError::CommandExpired)
        );
    }
    #[test]
    fn legacy_only_database_clock_rollback_retires_before_first_v2_issuance() {
        let mut db = db();
        let clock = Clock::new(1000);
        db.execute(
            "INSERT INTO documents VALUES('business/receipt/content/5',X'01',1)",
            [],
        )
        .unwrap();
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        clock.advance(5000);
        window.maintain(&mut db).unwrap();
        clock.wall.store(500, Ordering::SeqCst);
        let window = Window::open(&mut db, clock.clone()).unwrap();
        assert_eq!(
            window.check_legacy(&db, CommandFamily::Content, CommandId::new(5).unwrap()),
            Err(StorageError::CommandExpired)
        );
        clock.wall.store(6000, Ordering::SeqCst);
        let window = Window::open(&mut db, clock).unwrap();
        assert_eq!(
            window.check_legacy(&db, CommandFamily::Content, CommandId::new(5).unwrap()),
            Err(StorageError::CommandExpired)
        );
    }
    #[test]
    fn legacy_result_without_primary_receipt_cannot_authorize_new_execution() {
        let mut db = db();
        let clock = Clock::new(1000);
        db.execute(
            "INSERT INTO documents VALUES('business/result/schedule/9',X'01',1)",
            [],
        )
        .unwrap();
        let window = Window::open(&mut db, clock).unwrap();
        assert_eq!(
            window.check_legacy(&db, CommandFamily::Schedule, CommandId::new(9).unwrap()),
            Err(StorageError::CommandNotIssued)
        );
    }
    #[test]
    fn coarse_wall_samples_do_not_double_age_and_forward_jumps_do_not_pause_aging() {
        let mut db = db();
        let clock = Clock::new(1000);
        let mut window = Window::open(&mut db, clock.clone()).unwrap();
        let id = window.issue(&mut db, request(1)).unwrap();
        clock.mono.store(500, Ordering::SeqCst);
        window.maintain(&mut db).unwrap();
        clock.mono.store(1000, Ordering::SeqCst);
        clock.wall.store(2000, Ordering::SeqCst);
        window.maintain(&mut db).unwrap();
        assert_eq!(window.logical, 2000);
        clock.wall.store(3602000, Ordering::SeqCst);
        window.maintain(&mut db).unwrap();
        let after_jump = window.logical;
        clock.mono.fetch_add(1000, Ordering::SeqCst);
        window.maintain(&mut db).unwrap();
        assert_eq!(window.logical, after_jump + 1000);
        assert_eq!(check(&db, id), Ok(()));
    }
}
