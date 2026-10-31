//! Bounded writer-owned recovery jobs. Ordinary files are freshly constructed
//! logical projections, never copied SQLite pages containing deleted secrets.
//! Paths and quiescence are supplied by trusted deployment composition; this
//! layer does not claim to own filesystem pins or perform a live cold switch.
use crate::{
    business::{self, recovery as model, SessionGuard},
    *,
};
use fc_domain::{
    auth::ActorContext,
    identity::IdentityState,
    ids::{PrincipalId, SchoolId},
    recovery::BackupEntry,
    time::*,
};
use fc_ports::Clock;
use rusqlite::OpenFlags;
use serde_json::json;

pub type ExecutionFence = Arc<dyn Fn() -> bool + Send + Sync>;
pub struct RecoveryBarrier {
    revision: Revision,
    generation: u64,
    principal: PrincipalId,
}
impl RecoveryBarrier {
    pub(crate) fn new(revision: Revision, generation: u64, principal: PrincipalId) -> Self {
        Self {
            revision,
            generation,
            principal,
        }
    }
    pub fn revision(&self) -> Revision {
        self.revision
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn principal_id(&self) -> PrincipalId {
        self.principal
    }
}
pub(crate) fn decode_barrier(bytes: Vec<u8>) -> Result<RecoveryBarrier, StorageError> {
    let v = business::codec::value(&bytes)?;
    business::codec::object_fields(&v, &["revision", "generation", "principal"])?;
    Ok(RecoveryBarrier::new(
        Revision::new(business::codec::number(&v, "revision")?),
        business::codec::number(&v, "generation")?,
        PrincipalId::new(business::codec::number(&v, "principal")?)
            .map_err(|_| StorageError::Database)?,
    ))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryDatabaseReceipt {
    /// CURRENT live revision whose security/catalog were validated. A preview
    /// is review evidence only and grants no cold-switch permission.
    pub current_revision: Revision,
    pub revision: Revision,
    pub source_revision: Revision,
    pub school_id: SchoolId,
    pub installation_id: [u8; 32],
    pub recovery_generation: u64,
    pub document_count: u64,
    pub database_bytes: u64,
    pub database_sha256: [u8; 32],
    pub source_database_bytes: u64,
    pub source_database_sha256: [u8; 32],
    /// Typed metadata at exactly this revision. Caller must retain physical
    /// pins and quiescence until all these files have been copied and hashed.
    pub media: Vec<BackupEntry>,
}
pub(crate) enum Job {
    Export {
        destination: PathBuf,
        actor: ActorContext,
        session: SessionGuard,
        clock: Arc<dyn Clock + Send + Sync>,
        fence: ExecutionFence,
    },
    Candidate {
        source: PathBuf,
        destination: PathBuf,
        authorization: PreparationAuthorization,
        fence: ExecutionFence,
        clock: Arc<dyn Clock + Send + Sync>,
    },
}
pub(crate) enum PreparationAuthorization {
    Barrier(RecoveryBarrier),
    Preview {
        actor: ActorContext,
        session: SessionGuard,
    },
}
impl Job {
    pub(crate) fn allocation_bytes(&self) -> usize {
        match self {
            Self::Export { destination, .. } => destination.capacity(),
            Self::Candidate {
                source,
                destination,
                ..
            } => source.capacity() + destination.capacity(),
        }
    }
}
pub(crate) fn validate_path(path: &Path) -> Result<(), StorageError> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        Err(StorageError::InvalidCommand)
    } else {
        Ok(())
    }
}
pub(crate) fn check_fence(fence: &ExecutionFence) -> Result<(), StorageError> {
    if catch_unwind(AssertUnwindSafe(|| fence())).unwrap_or(false) {
        Ok(())
    } else {
        Err(StorageError::RecoveryCancelled)
    }
}
const ORDINARY_SCHEMA:&str="CREATE TABLE state_meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL CHECK(revision>=0));\nCREATE TABLE documents (key TEXT PRIMARY KEY, value BLOB NOT NULL, revision INTEGER NOT NULL CHECK(revision>=1));\nCREATE TABLE backup_meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), format INTEGER NOT NULL CHECK(format=1), school_id INTEGER NOT NULL CHECK(school_id>0), installation_id BLOB NOT NULL CHECK(length(installation_id)=32), catalog BLOB NOT NULL);";
const APPLICATION_ID: i64 = 0x46434231;
pub(crate) const RETIRED: &str = "business/security/recovery-command/v1/";
pub(crate) const ORDINARY_RETIRED: &str = "business/recovery/retired-command/v1/";
pub(crate) fn is_receipt(key: &str) -> bool {
    [
        "business/receipt/content/",
        "business/receipt/identity/",
        "business/receipt/schedule/",
        "business/receipt/display/",
        "business/receipt/lesson/",
        "business/receipt/calendar/",
        "business/profiles/v1/receipts/",
        "business/profile-content/v1/receipts/",
        "business/profile-content/v1/page-receipts/",
        "business/material/v1/receipts/",
        "business/voice-receipt/v1/",
    ]
    .iter()
    .any(|p| {
        key.strip_prefix(p).is_some_and(|s| {
            !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
        })
    })
}
fn receipt_hash(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn check_retired_command(db: &Connection, key: &str) -> Result<(), StorageError> {
    if !is_receipt(key) {
        return Ok(());
    }
    let marker = format!("{RETIRED}{}", receipt_hash(key));
    let row: Option<Vec<u8>> = db
        .query_row("SELECT value FROM documents WHERE key=?1", [marker], |r| {
            read_limited_blob(r, 0)
        })
        .optional()
        .map_err(|_| StorageError::Database)?;
    if let Some(row) = row {
        let v = business::codec::value(&row)?;
        business::codec::object_fields(&v, &["format", "generation"])?;
        if business::codec::number(&v, "format")? != 1
            || business::codec::number(&v, "generation")? == 0
        {
            return Err(StorageError::Database);
        }
        return Err(StorageError::CommandIdReused);
    }
    Ok(())
}
fn file_hash(
    path: &Path,
    fence: &ExecutionFence,
    stop: &AtomicBool,
) -> Result<(u64, [u8; 32]), StorageError> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|_| StorageError::Database)?;
    let mut sha = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 32768];
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(StorageError::Stopping);
        }
        check_fence(fence)?;
        let n = file.read(&mut buffer).map_err(|_| StorageError::Database)?;
        if n == 0 {
            break;
        }
        size = size
            .checked_add(n as u64)
            .filter(|n| *n <= fc_domain::recovery::MAX_DATABASE_BYTES)
            .ok_or(StorageError::ResourceQuota)?;
        sha.update(&buffer[..n]);
    }
    Ok((size, sha.finalize().into()))
}
fn sql<T>(r: rusqlite::Result<T>) -> Result<T, StorageError> {
    r.map_err(|_| StorageError::RecoveryInvalidPackage)
}
fn own_new(path: &Path) -> Result<BackupFile, StorageError> {
    validate_path(path)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                StorageError::BackupDestinationExists
            } else {
                StorageError::Database
            }
        })?;
    Ok(BackupFile {
        path: path.to_owned(),
        finished: false,
    })
}
fn write_row(
    db: &Connection,
    key: &str,
    value: &[u8],
    revision: Revision,
) -> Result<(), StorageError> {
    if key.is_empty() || key.len() > 256 || value.len() > MAX_VALUE_BYTES {
        return Err(StorageError::DocumentTooLarge);
    }
    sql(db.execute("INSERT INTO documents(key,value,revision) VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=excluded.revision",
        params![key,value,i64::try_from(revision.get()).map_err(|_|StorageError::RecoveryConflict)?]))?;
    Ok(())
}
fn visit(
    db: &Connection,
    mut f: impl FnMut(String, Document) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let d = TransactionView(db);
    let mut after = None;
    loop {
        let page = d.scan_prefix("business/", after.as_deref(), 128)?;
        for (k, row) in page.entries {
            f(k, row)?;
        }
        after = page.next_key;
        if after.is_none() {
            break;
        }
    }
    Ok(())
}
fn floor(
    db: &Connection,
    identity: &IdentityState,
    now: LocalDate,
) -> Result<LocalDate, StorageError> {
    Ok(model::read_floor(&TransactionView(db), identity)?.map_or(now, |d| d.max(now)))
}
/// A projected file intentionally lacks credentials and identity. Validation
/// borrows the CURRENT real security documents, never inventing authentication.
struct Projected<'a> {
    projection: &'a Connection,
    current: &'a Connection,
}
impl TransactionDocuments for Projected<'_> {
    fn get(&self, k: &str) -> Result<Option<Document>, StorageError> {
        if model::protected(k) {
            TransactionView(self.current).get(k)
        } else {
            TransactionView(self.projection).get(k)
        }
    }
    fn revision(&self) -> Result<Revision, StorageError> {
        read_revision(self.projection)
    }
    fn scan_prefix(
        &self,
        p: &str,
        a: Option<&str>,
        l: usize,
    ) -> Result<DocumentPage, StorageError> {
        TransactionView(self.projection).scan_prefix(p, a, l)
    }
}
fn recalculate(
    db: &Connection,
    identity: &IdentityState,
    day: LocalDate,
    revision: Revision,
    restoring: bool,
) -> Result<(), StorageError> {
    let mut bytes = 0u64;
    let mut after = None;
    loop {
        let p = TransactionView(db).scan_prefix(
            "business/profile-media/v1/entries/",
            after.as_deref(),
            128,
        )?;
        for (key, row) in p.entries {
            let v = business::codec::value(&row.value)?;
            let id = key
                .strip_prefix("business/profile-media/v1/entries/")
                .ok_or(StorageError::RecoveryInvalidPackage)?
                .parse::<u64>()
                .map_err(|_| StorageError::RecoveryInvalidPackage)?;
            write_row(
                db,
                &format!("business/profile-media/v1/recovery-owners/{id:020}"),
                &business::codec::bytes(&json!({"format":1,"id":id}))?,
                revision,
            )?;
            if v["state"] != "removed" {
                bytes = bytes
                    .checked_add(business::codec::number(&v, "bytes")?)
                    .ok_or(StorageError::ResourceQuota)?;
            }
        }
        after = p.next_key;
        if after.is_none() {
            break;
        }
    }
    if let Some(row) = TransactionView(db).get("business/profile-media/v1/catalog")? {
        let mut v = business::codec::value(&row.value)?;
        v["bytes"] = json!(bytes);
        write_row(
            db,
            "business/profile-media/v1/catalog",
            &business::codec::bytes(&v)?,
            revision,
        )?;
    }
    write_row(
        db,
        "business/desktop/max-observed-day/v1",
        &business::codec::bytes(&json!({"format":1,"date":business::codec::date(day)}))?,
        revision,
    )?;
    if restoring {
        model::retain_lesson_audit(db, identity, revision)?;
        business::recovery::reset_lesson(db, identity, day, revision)?;
    }
    Ok(())
}
pub(crate) fn execute(
    db: &mut Connection,
    job: Job,
    stop: &Arc<AtomicBool>,
) -> Result<RecoveryDatabaseReceipt, StorageError> {
    match job {
        Job::Export {
            destination,
            actor,
            session,
            clock,
            fence,
        } => {
            let old = business::load_identity(&TransactionView(db))?;
            let authorize = || {
                if stop.load(Ordering::Acquire) {
                    return Err(StorageError::Stopping);
                }
                check_fence(&fence)?;
                session(&old, &actor)?;
                old.authorize_management(&actor, clock.now())
                    .map_err(StorageError::Identity)
            };
            authorize()?;
            business::validate_documents(&TransactionView(db), &old)?;
            let revision = read_revision(db)?;
            let installation_id =
                model::installation(&TransactionView(db))?.ok_or(StorageError::RecoveryConflict)?;
            let i = old
                .installation
                .as_ref()
                .ok_or(StorageError::RecoveryConflict)?;
            let day = floor(db, &old, clock.now().date)?;
            let mut owned = own_new(&destination)?;
            let mut output = sql(Connection::open(&destination))?;
            sql(output.execute_batch(ORDINARY_SCHEMA))?;
            sql(output.pragma_update(None, "application_id", APPLICATION_ID))?;
            sql(output.pragma_update(None, "user_version", 1))?;
            let tx = sql(output.transaction())?;
            sql(tx.execute("INSERT INTO state_meta VALUES(1,?1)", [revision.get()]))?;
            let cat = business::codec::bytes(&model::catalog(&old)?)?;
            sql(tx.execute(
                "INSERT INTO backup_meta VALUES(1,1,?1,?2,?3)",
                params![i.school_id.get(), installation_id.as_slice(), cat],
            ))?;
            visit(db, |key, row| {
                authorize()?;
                if is_receipt(&key) {
                    write_row(
                        &tx,
                        &format!("{ORDINARY_RETIRED}{}", receipt_hash(&key)),
                        &business::codec::bytes(&json!({"format":1}))?,
                        revision,
                    )?;
                } else if let Some(hash) = key.strip_prefix(RETIRED) {
                    model::validate_retired_hash(hash)?;
                    write_row(
                        &tx,
                        &format!("{ORDINARY_RETIRED}{hash}"),
                        &business::codec::bytes(&json!({"format":1}))?,
                        revision,
                    )?;
                }
                if model::allowed(&key) {
                    if let Some(v) = model::project(&key, &row.value, day, None, &old)? {
                        write_row(&tx, &key, &v, row.revision)?;
                    }
                } else if !model::known_omitted(&key) {
                    return Err(StorageError::RecoveryInvalidPackage);
                }
                Ok(())
            })?;
            recalculate(&tx, &old, day, revision, false)?;
            let overlay = Projected {
                projection: &tx,
                current: db,
            };
            business::validate_documents(&overlay, &old)?;
            model::validate_ordinary(&overlay, &old)?;
            let media = model::media(&TransactionView(&tx))?;
            let count = sql(tx.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0)))?;
            authorize()?;
            sql(tx.commit())?;
            bounded_file(&output)?;
            sql(output.close().map_err(|(_, e)| e))?;
            let (database_bytes, database_sha256) = file_hash(&destination, &fence, stop)?;
            authorize()?;
            owned.finished = true;
            Ok(RecoveryDatabaseReceipt {
                current_revision: revision,
                revision,
                source_revision: revision,
                school_id: i.school_id,
                installation_id,
                recovery_generation: model::generation(&TransactionView(db))?,
                document_count: count,
                database_bytes,
                database_sha256,
                source_database_bytes: database_bytes,
                source_database_sha256: database_sha256,
                media,
            })
        }
        Job::Candidate {
            source,
            destination,
            authorization,
            fence,
            clock,
        } => {
            let current = business::load_identity(&TransactionView(db))?;
            let current_revision = read_revision(db)?;
            let check = || {
                if stop.load(Ordering::Acquire) {
                    return Err(StorageError::Stopping);
                }
                check_fence(&fence)?;
                if let PreparationAuthorization::Preview { actor, session } = &authorization {
                    session(&current, actor)?;
                    current
                        .authorize_management(actor, clock.now())
                        .map_err(StorageError::Identity)?;
                }
                Ok(())
            };
            check()?;
            let (principal, generation) = match &authorization {
                PreparationAuthorization::Preview { actor, .. } => (
                    actor.principal_id,
                    model::generation(&TransactionView(db))?
                        .checked_add(1)
                        .ok_or(StorageError::RecoveryConflict)?,
                ),
                PreparationAuthorization::Barrier(barrier) => {
                    if read_revision(db)? < barrier.revision
                        || model::generation(&TransactionView(db))? != barrier.generation
                    {
                        return Err(StorageError::RecoveryConflict);
                    }
                    let security = TransactionView(db)
                        .get(model::SECURITY)?
                        .ok_or(StorageError::RecoveryConflict)?;
                    let security_value = business::codec::value(&security.value)?;
                    if business::codec::number(&security_value, "principal")?
                        != barrier.principal.get()
                        || business::codec::number(&security_value, "revision")?
                            != barrier.revision.get()
                    {
                        return Err(StorageError::RecoveryConflict);
                    }
                    (
                        barrier.principal,
                        barrier
                            .generation
                            .checked_add(1)
                            .ok_or(StorageError::RecoveryConflict)?,
                    )
                }
            };
            let input = open_ordinary_fenced(&source, &fence, stop).or_else(|e| {
                check()?;
                Err(e)
            })?;
            let (source_database_bytes, source_database_sha256) = file_hash(&source, &fence, stop)?;
            let installation_id =
                model::installation(&TransactionView(db))?.ok_or(StorageError::RecoveryConflict)?;
            let imported_installation: Vec<u8> = sql(input.query_row(
                "SELECT installation_id FROM backup_meta WHERE singleton=1",
                [],
                |r| read_limited_blob(r, 0),
            ))?;
            if imported_installation.as_slice() != installation_id {
                return Err(StorageError::RecoveryConflict);
            }
            let source_revision = read_revision(&input)?;
            business::validate_documents(&TransactionView(db), &current)?;
            let cat: Vec<u8> = sql(input.query_row(
                "SELECT catalog FROM backup_meta WHERE singleton=1",
                [],
                |r| read_limited_blob(r, 0),
            ))?;
            let identity = model::advance_identity(&model::merge_catalog(
                &current,
                &business::codec::value(&cat)?,
            )?)?;
            let ordinary = Projected {
                projection: &input,
                current: db,
            };
            model::validate_ordinary(&ordinary, &identity)?;
            let revision = Revision::new(read_revision(db)?.get().max(source_revision.get()))
                .next()
                .map_err(|_| StorageError::RecoveryConflict)?;
            i64::try_from(revision.get()).map_err(|_| StorageError::RecoveryConflict)?;
            let source_floor = model::read_floor_untrusted(&TransactionView(&input))?;
            let day = floor(db, &current, clock.now().date)?
                .max(source_floor.unwrap_or(clock.now().date));
            // Copy only CURRENT local security into the isolated candidate. The
            // ordinary file has never contained these pages or credentials.
            bounded_file(db)?;
            backup_database(db, &destination, stop)?;
            let mut owned = BackupFile {
                path: destination.clone(),
                finished: false,
            };
            let mut output = sql(Connection::open(&destination))?;
            let tx = sql(output.transaction_with_behavior(TransactionBehavior::Immediate))?;
            // Persist non-replayable command identities without old results,
            // actors, credentials or request capabilities.
            visit(db, |key, _| {
                check()?;
                if is_receipt(&key) {
                    write_row(
                        &tx,
                        &format!("{RETIRED}{}", receipt_hash(&key)),
                        &business::codec::bytes(&json!({"format":1,"generation":generation}))?,
                        revision,
                    )?;
                }
                Ok(())
            })?;
            // Clear the restorable generation, including old receipts and pending
            // async admissions. Current credentials/login/OS journal stay exact.
            let mut after = None;
            loop {
                let p = TransactionView(&tx).scan_prefix("business/", after.as_deref(), 128)?;
                for (k, _) in p.entries {
                    check()?;
                    if !model::protected(&k) {
                        sql(tx.execute("DELETE FROM documents WHERE key=?1", [k]))?;
                    }
                }
                after = p.next_key;
                if after.is_none() {
                    break;
                }
            }
            sql(tx.execute(
                "UPDATE state_meta SET revision=?1 WHERE singleton=1",
                [revision.get()],
            ))?;
            visit(&input, |key, row| {
                check()?;
                if !model::allowed(&key) || row.revision > source_revision {
                    return Err(StorageError::RecoveryInvalidPackage);
                }
                if let Some(hash) = key.strip_prefix(ORDINARY_RETIRED) {
                    model::validate_retired_hash(hash)?;
                    write_row(
                        &tx,
                        &format!("{RETIRED}{hash}"),
                        &business::codec::bytes(&json!({"format":1,"generation":generation}))?,
                        revision,
                    )?;
                    return Ok(());
                }
                if let Some(v) = model::project(&key, &row.value, day, Some(revision), &identity)? {
                    write_row(&tx, &key, &v, revision)?;
                }
                Ok(())
            })?;
            // Identity catalog revisions and all authorization epochs move from
            // CURRENT state; no backup role or assignment is ever consulted.
            identity.validate().map_err(StorageError::Identity)?;
            write_row(
                &tx,
                "business/identity/v1",
                &business::codec::bytes(&business::codec::identity(&identity))?,
                revision,
            )?;
            write_row(
                &tx,
                model::SECURITY,
                &business::codec::bytes(&model::security_value(
                    generation, revision, principal, &identity,
                ))?,
                revision,
            )?;
            recalculate(&tx, &identity, day, revision, true)?;
            model::merge_allocator_floors(&tx, db, &identity, revision)?;
            business::recovery::merge_calendar_floor(&tx, db, revision)?;
            business::validate_documents(&TransactionView(&tx), &identity)?;
            let media = model::media(&TransactionView(&tx))?;
            let count = sql(tx.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0)))?;
            check()?;
            sql(tx.commit())?;
            bounded_file(&output)?;
            sql(output.close().map_err(|(_, e)| e))?;
            check()?;
            // Reopen through production startup checks, not just PRAGMA or row
            // shape validation. The old database remains the running authority.
            let reopened = business::startup_state(&destination)?;
            if reopened.revision != revision || reopened.identity != identity {
                return Err(StorageError::RecoveryConflict);
            }
            let (database_bytes, database_sha256) = file_hash(&destination, &fence, stop)?;
            check()?;
            owned.finished = true;
            Ok(RecoveryDatabaseReceipt {
                current_revision,
                revision,
                source_revision,
                school_id: identity
                    .installation
                    .as_ref()
                    .ok_or(StorageError::RecoveryConflict)?
                    .school_id,
                installation_id,
                recovery_generation: generation,
                document_count: count,
                database_bytes,
                database_sha256,
                source_database_bytes,
                source_database_sha256,
                media,
            })
        }
    }
}
fn bounded_file(db: &Connection) -> Result<(), StorageError> {
    let pages: u64 = sql(db.pragma_query_value(None, "page_count", |r| r.get(0)))?;
    let size: u64 = sql(db.pragma_query_value(None, "page_size", |r| r.get(0)))?;
    if pages
        .checked_mul(size)
        .is_none_or(|n| n > fc_domain::recovery::MAX_DATABASE_BYTES)
    {
        return Err(StorageError::ResourceQuota);
    }
    Ok(())
}
fn open_ordinary_fenced(
    path: &Path,
    fence: &ExecutionFence,
    stop: &Arc<AtomicBool>,
) -> Result<Connection, StorageError> {
    use rusqlite::limits::Limit;
    use std::io::Read;
    check_fence(fence)?;
    let meta = std::fs::metadata(path).map_err(|_| StorageError::RecoveryInvalidPackage)?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > fc_domain::recovery::MAX_DATABASE_BYTES {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    let mut header = [0u8; 100];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .map_err(|_| StorageError::RecoveryInvalidPackage)?;
    // Ordinary backups are standalone DELETE-journal files. Reject WAL mode
    // before SQLite can consult or create any sidecar beside the pinned file.
    if &header[..16] != b"SQLite format 3\0" || header[18] != 1 || header[19] != 1 {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    let db = sql(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    // Apply SQLite's own per-value/row/SQL limits before any schema query or
    // attacker-controlled BLOB can be materialized by its VM. These complement
    // the 64-KiB Rust document checks; they are not a hard bound on all SQLite
    // pager/schema bookkeeping or a wall-clock preemption guarantee.
    for (limit, value) in [
        (Limit::SQLITE_LIMIT_LENGTH, 128 * 1024),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, 4096),
        (Limit::SQLITE_LIMIT_COLUMN, 32),
        (Limit::SQLITE_LIMIT_EXPR_DEPTH, 32),
        (Limit::SQLITE_LIMIT_COMPOUND_SELECT, 8),
        (Limit::SQLITE_LIMIT_VDBE_OP, 100000),
        (Limit::SQLITE_LIMIT_FUNCTION_ARG, 16),
        (Limit::SQLITE_LIMIT_ATTACHED, 0),
        (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 16),
        (Limit::SQLITE_LIMIT_TRIGGER_DEPTH, 0),
        (Limit::SQLITE_LIMIT_WORKER_THREADS, 0),
    ] {
        db.set_limit(limit, value);
    }
    let cancelled = fence.clone();
    let stopping = stop.clone();
    db.progress_handler(
        1000,
        Some(move || stopping.load(Ordering::Acquire) || check_fence(&cancelled).is_err()),
    );
    sql(db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;"))?;
    sql(db.pragma_update(None, "cache_size", -2048))?;
    let app: i64 = sql(db.pragma_query_value(None, "application_id", |r| r.get(0)))?;
    let version: i64 = sql(db.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if app != APPLICATION_ID || version != 1 {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    // Exact trusted schema text and object set: no triggers, views, virtual
    // tables or executable schema supplied by the imported package is accepted.
    let reference = sql(Connection::open_in_memory())?;
    sql(reference.execute_batch(ORDINARY_SCHEMA))?;
    type SchemaObject = (String, String, String, Option<String>);
    fn schema(db: &Connection) -> Result<Vec<SchemaObject>, StorageError> {
        let count: u64 = sql(db.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0)))?;
        if count > 8 {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        let mut q =
            sql(db.prepare("SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name"))?;
        fn text(r: &rusqlite::Row<'_>, n: usize, max: usize) -> rusqlite::Result<String> {
            match r.get_ref(n)? {
                rusqlite::types::ValueRef::Text(bytes) if bytes.len() <= max => {
                    std::str::from_utf8(bytes)
                        .map(str::to_owned)
                        .map_err(|_| rusqlite::Error::InvalidQuery)
                }
                _ => Err(rusqlite::Error::InvalidQuery),
            }
        }
        let rows = sql(q.query_map([], |r| {
            Ok((
                text(r, 0, 32)?,
                text(r, 1, 128)?,
                text(r, 2, 128)?,
                if matches!(r.get_ref(3)?, rusqlite::types::ValueRef::Null) {
                    None
                } else {
                    Some(text(r, 3, 4096)?)
                },
            ))
        }))?;
        rows.map(sql).collect()
    }
    if schema(&db)? != schema(&reference)? {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    let count: u64 = sql(db.query_row("SELECT COUNT(*) FROM backup_meta", [], |r| r.get(0)))?;
    let states: u64 = sql(db.query_row("SELECT COUNT(*) FROM state_meta", [], |r| r.get(0)))?;
    if count != 1 || states != 1 || read_revision(&db)?.get() == 0 {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    let (format, school, cat): (u64, u64, Vec<u8>) = sql(db.query_row(
        "SELECT format,school_id,catalog FROM backup_meta WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, read_limited_blob(r, 2)?)),
    ))?;
    let catalog = business::codec::value(&cat)?;
    if format != 1 || school == 0 || business::codec::number(&catalog, "school")? != school {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    let installation: Vec<u8> = sql(db.query_row(
        "SELECT installation_id FROM backup_meta WHERE singleton=1",
        [],
        |r| read_limited_blob(r, 0),
    ))?;
    if installation.len() != 32 || installation.iter().all(|b| *b == 0) {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    let integrity: String = sql(db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0)))?;
    if integrity != "ok" {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    let invalid:u64=sql(db.query_row("SELECT COUNT(*) FROM documents WHERE typeof(key)<>'text' OR length(CAST(key AS BLOB))>256 OR length(key)=0 OR typeof(value)<>'blob' OR length(value)>65536 OR revision<1",[],|r|r.get(0)))?;
    if invalid != 0 {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    // A full ordered bounded scan catches keys outside the sole business prefix.
    let all: u64 = sql(db.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0)))?;
    let mut seen = 0;
    visit(&db, |key, row| {
        if !model::allowed(&key) {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        business::codec::value(&row.value)?;
        seen += 1;
        Ok(())
    })?;
    if seen != all {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    Ok(db)
}
#[cfg(test)]
fn open_ordinary(path: &Path) -> Result<Connection, StorageError> {
    let fence: ExecutionFence = Arc::new(|| true);
    open_ordinary_fenced(path, &fence, &Arc::new(AtomicBool::new(false)))
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
