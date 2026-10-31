//! Durable account backoff plus process-local, bounded KDF reservations.
//! No wall-clock input can erase a penalty. A restart conservatively reapplies the
//! last complete waiting interval; it never reconstructs an authenticated session.
use crate::{
    business::{credential_key, load_identity},
    *,
};
use fc_domain::{
    auth::Principal,
    ids::{AuthEpoch, PrincipalId},
};
use fc_ports::MonotonicClock;
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicU64, OnceLock, Weak},
};

pub const MAX_KDF_WORKERS: usize = 2;
pub const RESERVATION_MILLIS: u64 = 60_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialKind {
    Password,
    Recovery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verification {
    Verified,
    Rejected,
    CorruptRecord,
}
/// Not Clone, Deserialize, or constructible by a transport. The real KDF worker
/// owns it until exit. Timeout revokes authority but cannot release this CPU slot.
pub struct Reservation {
    generation: u64,
    sequence: u64,
    principal: PrincipalId,
    permit: Arc<PhysicalPermit>,
}
impl Reservation {
    pub fn principal_id(&self) -> PrincipalId {
        self.principal
    }
}
impl std::fmt::Debug for Reservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reservation([OPAQUE])")
    }
}
pub struct Admission {
    pub reservation: Reservation,
    pub principal: Principal,
    pub credential_version: u64,
    pub recovery_version: u64,
    pub kind: CredentialKind,
    /// Protected verifier bytes, never plaintext or ordinary authority snapshot data.
    pub protected_record: Vec<u8>,
}
impl std::fmt::Debug for Admission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Admission([REDACTED])")
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Completion {
    Authenticated {
        principal_id: PrincipalId,
        auth_epoch: AuthEpoch,
        credential_version: u64,
        recovery_version: u64,
    },
    Rejected {
        failures: u32,
        wait_seconds: u32,
    },
}
#[derive(Debug)]
pub enum Response {
    Admitted(Admission),
    Completed(Completion),
}
pub(crate) enum Operation {
    Admit(PrincipalId, CredentialKind),
    Finish(Reservation, Verification),
}

pub(crate) struct ProcessClock(Instant);
impl ProcessClock {
    pub(crate) fn new() -> Self {
        Self(Instant::now())
    }
}
impl MonotonicClock for ProcessClock {
    fn monotonic_millis(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}
struct PhysicalPermit(Arc<AtomicUsize>);
impl Drop for PhysicalPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
static GENERATION: AtomicU64 = AtomicU64::new(1);
static CPU_POOL: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
fn cpu_pool() -> Arc<AtomicUsize> {
    CPU_POOL
        .get_or_init(|| Arc::new(AtomicUsize::new(0)))
        .clone()
}
#[derive(Default, Clone)]
struct Penalty {
    failures: u32,
    wait_seconds: u32,
    pending: bool,
    repair: Vec<(CredentialKind, u64)>,
}
fn delay(failures: u32) -> u32 {
    if failures < 5 {
        0
    } else {
        1u32.checked_shl((failures - 5).min(10))
            .unwrap_or(900)
            .min(900)
    }
}
impl Penalty {
    fn failed(&mut self) {
        self.failures = self.failures.saturating_add(1);
        self.wait_seconds = delay(self.failures);
        self.pending = false;
    }
    fn read(db: &Connection, id: PrincipalId) -> Result<Self, StorageError> {
        let row: Option<Vec<u8>> = db
            .query_row(
                "SELECT value FROM documents WHERE key=?1",
                [penalty_key(id)],
                |r| read_limited_blob(r, 0),
            )
            .optional()
            .map_err(|_| StorageError::Database)?;
        let Some(row) = row else {
            return Ok(Self::default());
        };
        let value = crate::business::codec::value(&row)?;
        crate::business::codec::object_fields(
            &value,
            &["format", "failures", "wait_seconds", "pending", "repair"],
        )?;
        if value["format"].as_u64() != Some(1) {
            return Err(StorageError::Database);
        }
        let failures = u32::try_from(value["failures"].as_u64().ok_or(StorageError::Database)?)
            .map_err(|_| StorageError::Database)?;
        let wait_seconds = u32::try_from(
            value["wait_seconds"]
                .as_u64()
                .ok_or(StorageError::Database)?,
        )
        .map_err(|_| StorageError::Database)?;
        let pending = value["pending"].as_bool().ok_or(StorageError::Database)?;
        if wait_seconds != delay(failures) {
            return Err(StorageError::Database);
        }
        let entries = match &value["repair"] {
            serde_json::Value::Null => Vec::new(),
            serde_json::Value::Array(entries) if entries.len() <= 2 => {
                entries.iter().collect::<Vec<_>>()
            }
            object if object.is_object() => vec![object], // pre-release single-marker record
            _ => return Err(StorageError::Database),
        };
        let mut repair = Vec::new();
        for r in entries {
            crate::business::codec::object_fields(r, &["kind", "version"])?;
            let kind = match r["kind"].as_str() {
                Some("password") => CredentialKind::Password,
                Some("recovery") => CredentialKind::Recovery,
                _ => return Err(StorageError::Database),
            };
            let version = r["version"]
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or(StorageError::Database)?;
            if repair.iter().any(|(existing, _)| *existing == kind) {
                return Err(StorageError::Database);
            }
            repair.push((kind, version));
        }
        Ok(Self {
            failures,
            wait_seconds,
            pending,
            repair,
        })
    }
    fn write(&self, db: &Connection, id: PrincipalId) -> Result<(), StorageError> {
        let repair=self.repair.iter().map(|(kind,version)|json!({"kind":match kind{CredentialKind::Password=>"password",CredentialKind::Recovery=>"recovery"},"version":version})).collect::<Vec<_>>();
        let value=serde_json::to_vec(&json!({"format":1,"failures":self.failures,"wait_seconds":self.wait_seconds,"pending":self.pending,"repair":repair})).map_err(|_|StorageError::Database)?;
        // Security state has its own revision and cannot invalidate teaching authority.
        db.execute("INSERT INTO documents(key,value,revision) VALUES (?1,?2,1) ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=documents.revision+1",params![penalty_key(id),value]).map_err(|_|StorageError::Database)?;
        Ok(())
    }
}
fn penalty_key(id: PrincipalId) -> String {
    format!("business/security/login/{}", id.get())
}
#[derive(Clone)]
struct Active {
    sequence: u64,
    credential_version: u64,
    recovery_version: u64,
    epoch: AuthEpoch,
    kind: CredentialKind,
    deadline: u64,
    permit: Weak<PhysicalPermit>,
    settled: bool,
}
pub(crate) struct Limiter {
    generation: u64,
    sequence: u64,
    clock: Arc<dyn MonotonicClock + Send + Sync>,
    observed: Option<u64>,
    deadlines: BTreeMap<PrincipalId, u64>,
    active: BTreeMap<PrincipalId, Active>,
    cpu: Arc<AtomicUsize>,
}
impl Limiter {
    pub(crate) fn new(clock: Arc<dyn MonotonicClock + Send + Sync>) -> Self {
        Self {
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            sequence: 0,
            clock,
            observed: None,
            deadlines: BTreeMap::new(),
            active: BTreeMap::new(),
            cpu: cpu_pool(),
        }
    }
    fn now(&mut self) -> Result<u64, StorageError> {
        let now = self.clock.monotonic_millis();
        if self.observed.is_some_and(|old| now < old) {
            return Err(StorageError::LoginClockRegression);
        }
        self.observed = Some(now);
        Ok(now)
    }
    pub(crate) fn execute(
        &mut self,
        db: &mut Connection,
        operation: Operation,
        stop: &AtomicBool,
    ) -> Result<Response, StorageError> {
        let now = self.now()?;
        self.reap(db, now, stop)?;
        match operation {
            Operation::Admit(id, kind) => self.admit(db, id, kind, now, stop),
            Operation::Finish(token, outcome) => self.finish(db, token, outcome, now, stop),
        }
    }
    fn commit(tx: rusqlite::Transaction<'_>, stop: &AtomicBool) -> Result<(), StorageError> {
        if stop.load(Ordering::Acquire) {
            tx.rollback().map_err(|_| StorageError::Indeterminate)?;
            return Err(StorageError::Stopping);
        }
        tx.commit().map_err(|_| StorageError::Indeterminate)
    }
    fn reap(
        &mut self,
        db: &mut Connection,
        now: u64,
        stop: &AtomicBool,
    ) -> Result<(), StorageError> {
        let expired: Vec<_> = self
            .active
            .iter()
            .filter(|(_, a)| !a.settled && (now >= a.deadline || a.permit.strong_count() == 0))
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| StorageError::Database)?;
            let mut penalty = Penalty::read(&tx, id)?;
            if penalty.pending {
                penalty.failed();
                penalty.write(&tx, id)?;
            }
            let committed_at = self.now()?;
            let due = committed_at
                .checked_add(u64::from(penalty.wait_seconds) * 1000)
                .ok_or(StorageError::LoginClockRegression)?;
            Self::commit(tx, stop)?;
            self.deadlines.insert(id, due);
            if let Some(active) = self.active.get_mut(&id) {
                active.settled = true;
            }
        }
        self.active.retain(|_, a| a.permit.strong_count() > 0);
        Ok(())
    }
    fn admit(
        &mut self,
        db: &mut Connection,
        id: PrincipalId,
        kind: CredentialKind,
        now: u64,
        stop: &AtomicBool,
    ) -> Result<Response, StorageError> {
        if self.active.contains_key(&id) {
            return Err(StorageError::LoginInProgress);
        }
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| StorageError::Database)?;
        let state = load_identity(&TransactionView(&tx))?;
        let account = state.account(id).map_err(StorageError::Identity)?;
        if !account.principal.enabled {
            return Err(StorageError::LoginStaleCredential);
        }
        let mut penalty = Penalty::read(&tx, id)?;
        let recovered = penalty.pending;
        if recovered {
            penalty.failed();
            penalty.write(&tx, id)?;
        }
        penalty.repair.retain(|(repair_kind, version)| {
            let current = match repair_kind {
                CredentialKind::Password => account.credential_version,
                CredentialKind::Recovery => account.recovery_version,
            };
            *version == current
        });
        if penalty
            .repair
            .iter()
            .any(|(repair_kind, _)| *repair_kind == kind)
        {
            return Err(StorageError::CredentialCorrupt);
        }
        let due = *self.deadlines.entry(id).or_insert(
            now.checked_add(u64::from(penalty.wait_seconds) * 1000)
                .ok_or(StorageError::LoginClockRegression)?,
        );
        if now < due {
            if recovered {
                let fresh = self.now()?;
                let due = fresh
                    .checked_add(u64::from(penalty.wait_seconds) * 1000)
                    .ok_or(StorageError::LoginClockRegression)?;
                Self::commit(tx, stop)?;
                self.deadlines.insert(id, due);
                return Err(StorageError::LoginRateLimited {
                    retry_after_millis: u64::from(penalty.wait_seconds) * 1000,
                });
            } else {
                tx.rollback().map_err(|_| StorageError::Database)?;
            }
            return Err(StorageError::LoginRateLimited {
                retry_after_millis: due - now,
            });
        }
        self.cpu
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_KDF_WORKERS).then_some(n + 1)
            })
            .map_err(|_| StorageError::LoginCapacity)?;
        let permit = Arc::new(PhysicalPermit(self.cpu.clone()));
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(StorageError::InvalidCommand)?;
        let version = match kind {
            CredentialKind::Password => account.credential_version,
            CredentialKind::Recovery => account.recovery_version,
        };
        let key = credential_key(
            id,
            match kind {
                CredentialKind::Password => "password",
                CredentialKind::Recovery => "recovery",
            },
            version,
        );
        let protected_record = TransactionView(&tx)
            .get(&key)?
            .ok_or(StorageError::CredentialCorrupt)?
            .value;
        let admission = Admission {
            reservation: Reservation {
                generation: self.generation,
                sequence,
                principal: id,
                permit: permit.clone(),
            },
            principal: account.principal.clone(),
            credential_version: account.credential_version,
            recovery_version: account.recovery_version,
            kind,
            protected_record,
        };
        penalty.pending = true;
        penalty.write(&tx, id)?;
        let deadline = self
            .now()?
            .checked_add(RESERVATION_MILLIS)
            .ok_or(StorageError::LoginClockRegression)?;
        Self::commit(tx, stop)?;
        self.sequence = sequence;
        self.active.insert(
            id,
            Active {
                sequence,
                credential_version: admission.credential_version,
                recovery_version: admission.recovery_version,
                epoch: admission.principal.auth_epoch,
                kind,
                deadline,
                permit: Arc::downgrade(&permit),
                settled: false,
            },
        );
        Ok(Response::Admitted(admission))
    }
    fn finish(
        &mut self,
        db: &mut Connection,
        token: Reservation,
        outcome: Verification,
        now: u64,
        stop: &AtomicBool,
    ) -> Result<Response, StorageError> {
        let id = token.principal;
        let active = self
            .active
            .get(&id)
            .filter(|a| {
                token.generation == self.generation
                    && token.sequence == a.sequence
                    && a.permit.ptr_eq(&Arc::downgrade(&token.permit))
            })
            .cloned()
            .ok_or(StorageError::LoginReservationExpired)?;
        if active.settled || now >= active.deadline {
            return Err(StorageError::LoginReservationExpired);
        }
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| StorageError::Database)?;
        let state = load_identity(&TransactionView(&tx))?;
        let account = state.account(id).map_err(StorageError::Identity)?;
        let mut penalty = Penalty::read(&tx, id)?;
        if !penalty.pending {
            return Err(StorageError::LoginReservationExpired);
        }
        if !account.principal.enabled
            || account.principal.auth_epoch != active.epoch
            || account.credential_version != active.credential_version
            || account.recovery_version != active.recovery_version
        {
            penalty.failed();
            penalty.write(&tx, id)?;
            let committed_at = self.now()?;
            Self::commit(tx, stop)?;
            self.deadlines.insert(
                id,
                committed_at
                    .checked_add(u64::from(penalty.wait_seconds) * 1000)
                    .ok_or(StorageError::LoginClockRegression)?,
            );
            self.active.remove(&id);
            return Err(StorageError::LoginStaleCredential);
        }
        let original_penalty = penalty.clone();
        let completion = match outcome {
            Verification::Verified => {
                penalty = Penalty {
                    repair: penalty.repair,
                    ..Penalty::default()
                };
                Completion::Authenticated {
                    principal_id: id,
                    auth_epoch: account.principal.auth_epoch,
                    credential_version: account.credential_version,
                    recovery_version: account.recovery_version,
                }
            }
            Verification::Rejected => {
                penalty.failed();
                Completion::Rejected {
                    failures: penalty.failures,
                    wait_seconds: penalty.wait_seconds,
                }
            }
            Verification::CorruptRecord => {
                penalty.pending = false;
                let version = match active.kind {
                    CredentialKind::Password => active.credential_version,
                    CredentialKind::Recovery => active.recovery_version,
                };
                penalty.repair.retain(|(kind, _)| *kind != active.kind);
                penalty.repair.push((active.kind, version));
                penalty.write(&tx, id)?;
                self.now()?;
                Self::commit(tx, stop)?;
                self.active.remove(&id);
                return Err(StorageError::CredentialCorrupt);
            }
        };
        penalty.write(&tx, id)?;
        let committed_at = self.now()?;
        if committed_at >= active.deadline {
            let mut expired = original_penalty;
            expired.failed();
            expired.write(&tx, id)?;
            let due = committed_at
                .checked_add(u64::from(expired.wait_seconds) * 1000)
                .ok_or(StorageError::LoginClockRegression)?;
            Self::commit(tx, stop)?;
            self.deadlines.insert(id, due);
            self.active.remove(&id);
            return Err(StorageError::LoginReservationExpired);
        }
        let due = committed_at
            .checked_add(u64::from(penalty.wait_seconds) * 1000)
            .ok_or(StorageError::LoginClockRegression)?;
        Self::commit(tx, stop)?;
        self.deadlines.insert(id, due);
        self.active.remove(&id);
        Ok(Response::Completed(completion))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_security_blob_is_rejected_without_creating_an_owned_copy() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE documents(key TEXT PRIMARY KEY,value BLOB,revision INTEGER);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO documents VALUES (?1,zeroblob(?2),1)",
            params![
                penalty_key(PrincipalId::new(1).unwrap()),
                (MAX_VALUE_BYTES + 1) as u64
            ],
        )
        .unwrap();
        assert!(matches!(
            Penalty::read(&db, PrincipalId::new(1).unwrap()),
            Err(StorageError::Database)
        ));
    }
}
