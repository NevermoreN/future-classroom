//! Writer-owned, explicitly mapped ordinary projection. Neither receipt is a
//! cold-switch authorization; the normal recovery barrier remains mandatory.
use super::*;
use fc_domain::recovery::{MigrationCatalog, MigrationMapping};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationSourceReceipt {
    current_revision: Revision,
    source_revision: Revision,
    school_id: SchoolId,
    installation_id: [u8; 32],
    database_bytes: u64,
    database_sha256: [u8; 32],
    catalog_sha256: [u8; 32],
    catalog: MigrationCatalog,
}
impl MigrationSourceReceipt {
    pub fn current_revision(&self) -> Revision {
        self.current_revision
    }
    pub fn source_revision(&self) -> Revision {
        self.source_revision
    }
    pub fn school_id(&self) -> SchoolId {
        self.school_id
    }
    pub fn installation_id(&self) -> [u8; 32] {
        self.installation_id
    }
    pub fn database_bytes(&self) -> u64 {
        self.database_bytes
    }
    pub fn database_sha256(&self) -> [u8; 32] {
        self.database_sha256
    }
    pub fn catalog_sha256(&self) -> [u8; 32] {
        self.catalog_sha256
    }
    pub fn catalog(&self) -> &MigrationCatalog {
        &self.catalog
    }
}
#[derive(Debug)]
pub struct MappedOrdinaryReceipt {
    source: MigrationSourceReceipt,
    mapping_sha256: [u8; 32],
    school_id: SchoolId,
    installation_id: [u8; 32],
    database_bytes: u64,
    database_sha256: [u8; 32],
    media: Vec<BackupEntry>,
}
impl MappedOrdinaryReceipt {
    pub fn source(&self) -> &MigrationSourceReceipt {
        &self.source
    }
    pub fn mapping_sha256(&self) -> [u8; 32] {
        self.mapping_sha256
    }
    pub fn current_revision(&self) -> Revision {
        self.source.current_revision
    }
    pub fn school_id(&self) -> SchoolId {
        self.school_id
    }
    pub fn installation_id(&self) -> [u8; 32] {
        self.installation_id
    }
    pub fn revision(&self) -> Revision {
        self.source.source_revision
    }
    pub fn database_bytes(&self) -> u64 {
        self.database_bytes
    }
    pub fn database_sha256(&self) -> [u8; 32] {
        self.database_sha256
    }
    pub fn media(&self) -> &[BackupEntry] {
        &self.media
    }
}
pub(crate) struct InspectJob {
    pub source: PathBuf,
    pub actor: ActorContext,
    pub session: SessionGuard,
    pub clock: Arc<dyn Clock + Send + Sync>,
    pub fence: ExecutionFence,
}
impl InspectJob {
    pub(crate) fn allocation_bytes(&self) -> usize {
        self.source.capacity()
    }
}
pub(crate) struct MapJob {
    pub inspection: InspectJob,
    pub destination: PathBuf,
    pub expected: MigrationSourceReceipt,
    pub mapping: MigrationMapping,
}
impl MapJob {
    pub(crate) fn allocation_bytes(&self) -> usize {
        self.inspection
            .allocation_bytes()
            .saturating_add(std::mem::size_of::<Self>())
            .saturating_add(self.destination.capacity())
            .saturating_add(business::migration::catalog_allocation(
                &self.expected.catalog,
            ))
            .saturating_add(self.mapping.classes.capacity().saturating_mul(16))
            .saturating_add(self.mapping.subjects.capacity().saturating_mul(16))
            .saturating_add(self.mapping.principals.capacity().saturating_mul(16))
    }
}
fn authorize(
    current: &IdentityState,
    job: &InspectJob,
    stop: &Arc<AtomicBool>,
) -> Result<(), StorageError> {
    if stop.load(Ordering::Acquire) {
        return Err(StorageError::Stopping);
    }
    check_fence(&job.fence)?;
    (job.session)(current, &job.actor)?;
    current
        .authorize_management(&job.actor, job.clock.now())
        .map_err(StorageError::Identity)
}
fn check(
    db: &Connection,
    job: &InspectJob,
    stop: &Arc<AtomicBool>,
) -> Result<IdentityState, StorageError> {
    let current = business::load_identity(&TransactionView(db))?;
    authorize(&current, job, stop)?;
    Ok(current)
}
fn source(
    db: &Connection,
    job: &InspectJob,
    stop: &Arc<AtomicBool>,
) -> Result<(Connection, MigrationSourceReceipt), StorageError> {
    let current = check(db, job, stop)?;
    let input = open_ordinary_fenced(&job.source, &job.fence, stop)?;
    let (database_bytes, database_sha256) = file_hash(&job.source, &job.fence, stop)?;
    let cat: Vec<u8> = sql(input.query_row(
        "SELECT catalog FROM backup_meta WHERE singleton=1",
        [],
        |r| read_limited_blob(r, 0),
    ))?;
    let catalog = business::migration::read_catalog(&business::codec::value(&cat)?)?;
    let installation: Vec<u8> = sql(input.query_row(
        "SELECT installation_id FROM backup_meta WHERE singleton=1",
        [],
        |r| read_limited_blob(r, 0),
    ))?;
    let receipt = MigrationSourceReceipt {
        current_revision: read_revision(db)?,
        source_revision: read_revision(&input)?,
        school_id: catalog.school_id,
        installation_id: installation
            .try_into()
            .map_err(|_| StorageError::RecoveryInvalidPackage)?,
        database_bytes,
        database_sha256,
        catalog_sha256: Sha256::digest(&cat).into(),
        catalog,
    };
    visit(&input, |_, row| {
        authorize(&current, job, stop)?;
        if row.revision > receipt.source_revision {
            Err(StorageError::RecoveryInvalidPackage)
        } else {
            Ok(())
        }
    })?;
    check(db, job, stop)?;
    if read_revision(db)? != receipt.current_revision {
        return Err(StorageError::RecoveryConflict);
    }
    Ok((input, receipt))
}
pub(crate) fn inspect(
    db: &Connection,
    job: InspectJob,
    stop: &Arc<AtomicBool>,
) -> Result<MigrationSourceReceipt, StorageError> {
    source(db, &job, stop).map(|(_, r)| r)
}
pub(crate) fn map(
    db: &Connection,
    job: MapJob,
    stop: &Arc<AtomicBool>,
) -> Result<MappedOrdinaryReceipt, StorageError> {
    let (input, source) = source(db, &job.inspection, stop)?;
    if source != job.expected {
        return Err(StorageError::RecoveryConflict);
    }
    let current = check(db, &job.inspection, stop)?;
    business::validate_documents(&TransactionView(db), &current)?;
    job.mapping
        .validate(&source.catalog, &current)
        .map_err(|_| StorageError::RecoveryConflict)?;
    let installation_id =
        model::installation(&TransactionView(db))?.ok_or(StorageError::RecoveryConflict)?;
    let identity = business::migration::mapped_identity(&source.catalog, &current, &job.mapping)?;
    let mut owned = own_new(&job.destination)?;
    let mut output = sql(Connection::open(&job.destination))?;
    sql(output.execute_batch(ORDINARY_SCHEMA))?;
    sql(output.pragma_update(None, "application_id", APPLICATION_ID))?;
    sql(output.pragma_update(None, "user_version", 1))?;
    let tx = sql(output.transaction())?;
    sql(tx.execute(
        "INSERT INTO state_meta VALUES(1,?1)",
        [source.source_revision.get()],
    ))?;
    let cat = business::codec::bytes(&model::catalog(&identity)?)?;
    sql(tx.execute(
        "INSERT INTO backup_meta VALUES(1,1,?1,?2,?3)",
        params![
            job.mapping.target_school_id.get(),
            installation_id.as_slice(),
            cat
        ],
    ))?;
    visit(&input, |key, row| {
        authorize(&current, &job.inspection, stop)?;
        let (mapped, value) = business::migration::map_document(&key, &row.value, &job.mapping)?;
        if TransactionView(&tx).get(&mapped)?.is_some() {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        write_row(&tx, &mapped, &value, row.revision)
    })?;
    let overlay = Projected {
        projection: &tx,
        current: db,
    };
    model::validate_ordinary(&overlay, &identity)?;
    let media = model::media(&TransactionView(&tx))?;
    authorize(&current, &job.inspection, stop)?;
    sql(tx.commit())?;
    authorize(&current, &job.inspection, stop)?;
    bounded_file(&output)?;
    sql(output.close().map_err(|(_, e)| e))?;
    let verified = open_ordinary_fenced(&job.destination, &job.inspection.fence, stop)?;
    model::validate_ordinary(
        &Projected {
            projection: &verified,
            current: db,
        },
        &identity,
    )?;
    let (database_bytes, database_sha256) =
        file_hash(&job.destination, &job.inspection.fence, stop)?;
    check(db, &job.inspection, stop)?;
    if read_revision(db)? != source.current_revision {
        return Err(StorageError::RecoveryConflict);
    }
    owned.finished = true;
    Ok(MappedOrdinaryReceipt {
        source,
        mapping_sha256: business::migration::mapping_sha256(&job.mapping)?,
        school_id: job.mapping.target_school_id,
        installation_id,
        database_bytes,
        database_sha256,
        media,
    })
}
